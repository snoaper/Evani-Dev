//! `sweep.run` (docs/LIVE_API.md 6.29): try a list of values for one
//! parameter (or a small grid of several) on the fixed camera path and
//! return one compact table of per-pass costs and look-check measures.
//!
//! Built on the bench machinery of the parent module: for each row the
//! run applies S0 with the row's values (no history entry), captures the
//! look frames on the path, measures `frames_per_value` frames, optionally
//! captures the same frames with bloom off (for bloom coverage), computes
//! the row's measures and drops its images. The run always ends by applying
//! S0 again, also after a failure (`Run::fail`).

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use serde_json::{Map, Value, json};

use super::{
    Info, Poll, Run, RunKind, Step, Target, apply, bad, capture_step, measure_step, num, percentile,
    png_b64, run_target, set_param, tier_param, width_param, with_changes,
};
use crate::change::FullState;
use crate::img::Rgb;
use crate::look::{self, Look};
use crate::telemetry::{MAX_PASSES, Sample};
use crate::{LiveError, commands};

/// A parameter value for the table: f32 numbers at f32 precision
/// (`0.3`, not `0.30000001192092896`), recursively for vectors.
pub(super) fn tidy(v: &Value) -> Value {
    match v {
        Value::Number(n) if !(n.is_i64() || n.is_u64()) => n
            .as_f64()
            .and_then(|x| format!("{}", x as f32).parse::<f64>().ok())
            .map_or_else(|| v.clone(), |x| json!(x)),
        Value::Array(a) => Value::Array(a.iter().map(tidy).collect()),
        _ => v.clone(),
    }
}

/// One requested row: parameter name to value.
type Row = Map<String, Value>;

/// Most rows (values, or grid combinations) per sweep.
pub const MAX_ROWS: usize = 32;
/// Most look capture times per row.
const MAX_LOOK_AT: usize = 8;
const BLOOM_SWITCH: &str = "fx.bloom.enabled";
/// `side` of the shared bloom-off reference shots.
const SHARED: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bloom {
    /// No bloom coverage column.
    Off,
    /// One bloom-off reference (S0, bloom off) for every row: the sweep
    /// only changes `fx.bloom.*`, which bloom-off frames ignore.
    Shared,
    /// A bloom-off reference per row (the row's state, bloom off).
    PerRow,
}

/// What a sweep keeps between its steps and reports.
#[derive(Debug, Clone)]
pub struct SweepInfo {
    names: Vec<String>,
    /// The applied (clamped) values per row, in `names` order.
    rows: Vec<Vec<Value>>,
    base_row: Option<usize>,
    clamped: Vec<Value>,
    tier: String,
    frames: u32,
    warmup_s: f64,
    look_at: Vec<f64>,
    look_width: u32,
    bloom: Bloom,
    thumbs: Option<(u32, u32)>,
    looks: Vec<Option<Look>>,
    thumb_imgs: Vec<Option<Rgb>>,
}

/// The width of a thumbnail of a `sw` x `sh` frame that fits `w` x `h`
/// (`h` 0: no height limit). Never 0.
pub(super) fn fit(sw: u32, sh: u32, w: u32, h: u32) -> u32 {
    let mut out = w;
    if h > 0 && sh > 0 {
        out = out.min((u64::from(h) * u64::from(sw) / u64::from(sh)) as u32);
    }
    out.max(1)
}

/// Add `row` to a failure's `data` when the failing step belongs to a row.
pub(super) fn with_row(mut e: LiveError, step: &Step) -> LiveError {
    let row = match step {
        Step::Capture(c) if c.side != SHARED => Some(c.side),
        Step::Measure(m) => Some(m.side),
        Step::SweepRow(r) => Some(*r),
        _ => None,
    };
    if let Some(r) = row {
        let mut d = e.data.take().unwrap_or_else(|| json!({}));
        if !d.is_object() {
            d = json!({ "detail": d });
        }
        d["row"] = json!(r);
        e.data = Some(d);
    }
    e
}

/// The requested rows: the swept names (columns) and one `{name: value}`
/// object per row. `param` + `values`, or `grid` (every combination, the
/// first name varying slowest; names in sorted order).
pub(super) fn rows_param(params: &Value) -> Result<(Vec<String>, Vec<Row>), LiveError> {
    let grid = params.get("grid").filter(|v| !v.is_null());
    let param = params.get("param").filter(|v| !v.is_null());
    match (param, grid) {
        (Some(_), Some(_)) => Err(LiveError::new("bad_request", "give params.param with params.values, or params.grid, not both")),
        (None, None) => Err(LiveError::new(
            "bad_request",
            "params.param and params.values are required (or params.grid for several parameters)",
        )),
        (Some(p), None) => {
            let name = p
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| LiveError::new("bad_request", "params.param must be a parameter name"))?;
            let values = params
                .get("values")
                .and_then(Value::as_array)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| LiveError::new("bad_request", "params.values must be a non-empty array"))?;
            if values.len() > MAX_ROWS {
                return Err(bad(format!("a sweep takes at most {MAX_ROWS} values")));
            }
            let rows = values
                .iter()
                .map(|v| {
                    let mut m = Map::new();
                    m.insert(name.to_string(), v.clone());
                    m
                })
                .collect();
            Ok((vec![name.to_string()], rows))
        }
        (None, Some(g)) => {
            let g = g
                .as_object()
                .filter(|o| !o.is_empty())
                .ok_or_else(|| LiveError::new("bad_request", "params.grid must be an object of name: [values]"))?;
            let mut names = Vec::new();
            let mut lists = Vec::new();
            for (k, v) in g {
                let list = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| LiveError::new("bad_request", format!("params.grid.{k} must be a non-empty array")))?;
                names.push(k.clone());
                lists.push(list);
            }
            let n = lists.iter().try_fold(1usize, |acc, l| acc.checked_mul(l.len()));
            if n.is_none_or(|n| n > MAX_ROWS) {
                return Err(bad(format!("a sweep takes at most {MAX_ROWS} rows (the grid has more)")));
            }
            let mut rows = vec![Map::new()];
            for (name, list) in names.iter().zip(&lists) {
                let mut next = Vec::with_capacity(rows.len() * list.len());
                for r in &rows {
                    for v in *list {
                        let mut m = r.clone();
                        m.insert(name.clone(), v.clone());
                        next.push(m);
                    }
                }
                rows = next;
            }
            Ok((names, rows))
        }
    }
}

fn look_at_param(params: &Value, seconds: f64) -> Result<Vec<f64>, LiveError> {
    let Some(v) = params.get("look_at").filter(|v| !v.is_null()) else {
        return Ok([1.0 / 6.0, 0.5, 5.0 / 6.0]
            .iter()
            .map(|f| (seconds * f * 1000.0).round() / 1000.0)
            .collect());
    };
    let arr = v
        .as_array()
        .ok_or_else(|| bad("look_at must be an array of path times in seconds"))?;
    if arr.len() > MAX_LOOK_AT {
        return Err(bad(format!("look_at takes at most {MAX_LOOK_AT} times")));
    }
    arr.iter()
        .map(|x| match x.as_f64() {
            Some(t) if t.is_finite() && (0.0..=seconds).contains(&t) => Ok(t),
            _ => Err(bad(format!("look_at times must be within 0-{seconds} s (frames_per_value / 60)"))),
        })
        .collect()
}

/// `thumbs`: `false`/absent, `true` (160 wide) or `{w, h}` (each 32-480;
/// `h` optional). Other keys (`inline`, `dir`) are the MCP tool's.
fn thumbs_param(params: &Value) -> Result<Option<(u32, u32)>, LiveError> {
    let dim = |o: &Value, k: &str, d: u32| -> Result<u32, LiveError> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => match v.as_u64() {
                Some(x) if (32..=480).contains(&x) => Ok(x as u32),
                _ => Err(bad(format!("thumbs.{k} must be 32-480"))),
            },
        }
    };
    match params.get("thumbs") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
        Some(Value::Bool(true)) => Ok(Some((160, 0))),
        Some(o @ Value::Object(_)) => Ok(Some((dim(o, "w", 160)?, dim(o, "h", 0)?))),
        Some(_) => Err(bad("thumbs must be true, false or {w, h}")),
    }
}

fn bool_param(params: &Value, key: &str, default: bool) -> Result<bool, LiveError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(format!("{key} must be true or false"))),
    }
}

/// Plan a `sweep.run` into `run` (called by `start`). Every row is checked
/// before anything runs; a bad one fails with its usual error and `data.row`.
pub(super) fn plan(world: &World, run: &mut Run, params: &Value) -> Result<(), LiveError> {
    let (names, req_rows) = rows_param(params)?;
    if names.iter().any(|n| n == "aq.enabled") {
        return Err(bad("aq.enabled can't be swept: the run holds adaptive quality off"));
    }
    match params.get("path") {
        None | Some(Value::Null) => {}
        Some(Value::String(p)) if p == "bench" => {}
        Some(Value::String(p)) => {
            return Err(LiveError::new("unknown_target", format!("No camera path {p}"))
                .with(json!({ "did_you_mean": ["bench"] })));
        }
        Some(_) => return Err(bad("path must be a camera path name (\"bench\")")),
    }
    let frames = num(params, "frames_per_value", 120.0, 10.0, 3600.0)?.round() as u32;
    let seconds = f64::from(frames) / 60.0;
    let warmup_s = num(params, "warmup_s", 0.25, 0.0, 10.0)?;
    let tier = tier_param(world, params)?;
    let look_at = look_at_param(params, seconds)?;
    let look_width = width_param(params, "look_width", 320)?;
    let thumbs = thumbs_param(params)?;
    if thumbs.is_some() && look_at.is_empty() {
        return Err(bad("thumbs need at least one look_at time"));
    }
    let has_bloom = world.res::<ParamRegistry>().id(BLOOM_SWITCH).is_some();
    let want_bloom = bool_param(params, "bloom", has_bloom)?;
    if want_bloom && !has_bloom {
        return Err(LiveError::new("not_compiled_in", "bloom is not in this build; leave out bloom"));
    }
    let baseline = bool_param(params, "baseline", true)?;
    let bloom = if !want_bloom || look_at.is_empty() {
        Bloom::Off
    } else if names.iter().all(|n| n.starts_with("fx.bloom.")) {
        Bloom::Shared
    } else {
        Bloom::PerRow
    };

    let s0 = FullState::capture(world);
    // Check and plan every requested row first.
    let mut planned: Vec<(FullState, Vec<Value>)> = Vec::new();
    let mut clamped = Vec::new();
    let mut row_vals = Vec::new();
    for (i, row) in req_rows.iter().enumerate() {
        let p = commands::plan(world, "param.set", &json!({ "values": row })).map_err(|mut e| {
            let mut d = e.data.take().unwrap_or_else(|| json!({}));
            if !d.is_object() {
                d = json!({ "detail": d });
            }
            d["row"] = json!(i);
            e.data = Some(d);
            e
        })?;
        let mut vals = Vec::with_capacity(names.len());
        for n in &names {
            let a = p.applied.iter().find(|a| a["name"] == n.as_str());
            vals.push(a.map_or(Value::Null, |a| tidy(&a["new"])));
            if let Some(a) = a.filter(|a| a["clamped"] == true) {
                clamped.push(json!({ "row": i, "name": n, "requested": a["requested"], "applied": tidy(&a["new"]) }));
            }
        }
        row_vals.push(vals);
        planned.push((with_changes(&s0, &p.changes), Vec::new()));
    }
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut states: Vec<FullState> = Vec::new();
    let base_row = if baseline {
        let reg = world.res::<ParamRegistry>();
        let vals = names
            .iter()
            .map(|n| {
                reg.id(n)
                    .and_then(|id| Some(tidy(&reg.def(id)?.value_to_json(&reg.value(id)?))))
                    .unwrap_or(Value::Null)
            })
            .collect();
        rows.push(vals);
        states.push(s0.clone());
        // Requested rows' indexes in `clamped` move down by one.
        for c in &mut clamped {
            c["row"] = json!(c["row"].as_u64().unwrap_or(0) + 1);
        }
        Some(0)
    } else {
        None
    };
    rows.extend(row_vals);
    states.extend(planned.into_iter().map(|(s, _)| s));

    let bloom_off = |state: &FullState| -> Target {
        let mut s = state.clone();
        let id = world.res::<ParamRegistry>().id(BLOOM_SWITCH);
        if let Some(id) = id {
            set_param(&mut s, id, evani_core::params::ParamValue::Bool(false));
        }
        let mut t = run_target(world, &s0, s, tier.as_deref(), true);
        t.skip.retain(|x| Some(*x) != id);
        t
    };
    let cap = |side: usize, t: f64, reference: bool, thumb: Option<(u32, u32)>| {
        let mut st = capture_step(side, Some(t), true);
        if let Step::Capture(c) = &mut st {
            c.reference = reference;
            c.thumb = thumb;
        }
        st
    };

    run.kind = RunKind::Sweep;
    run.cmd = "sweep.run";
    run.width = look_width;
    run.samples = vec![Vec::new(); rows.len()];
    let first = run_target(world, &s0, s0.clone(), tier.as_deref(), true);
    let tier_id = first.state.tier.clone();
    if bloom == Bloom::Shared {
        run.steps.push_back(apply(bloom_off(&s0), false));
        for t in &look_at {
            run.steps.push_back(cap(SHARED, *t, true, None));
        }
    }
    let mid = look_at.len() / 2;
    for (i, state) in states.iter().enumerate() {
        run.steps
            .push_back(apply(run_target(world, &s0, state.clone(), tier.as_deref(), true), false));
        for (k, t) in look_at.iter().enumerate() {
            run.steps.push_back(cap(i, *t, false, thumbs.filter(|_| k == mid)));
        }
        run.steps.push_back(measure_step(i, seconds, warmup_s));
        if bloom == Bloom::PerRow {
            run.steps.push_back(apply(bloom_off(state), false));
            for t in &look_at {
                run.steps.push_back(cap(i, *t, true, None));
            }
        }
        run.steps.push_back(Step::SweepRow(i));
    }
    run.steps.push_back(apply(
        Target {
            state: s0.clone(),
            skip: Vec::new(),
        },
        true,
    ));
    let shots = look_at.len() as f64 * 100.0;
    let per_row = 150.0 + shots + (warmup_s + seconds) * 1000.0 + if bloom == Bloom::PerRow { 150.0 + shots } else { 0.0 };
    run.est_ms = rows.len() as f64 * per_row + if bloom == Bloom::Shared { 150.0 + shots } else { 0.0 } + 150.0;
    run.s0 = s0;
    let n = rows.len();
    run.info = Info::Sweep(Box::new(SweepInfo {
        names,
        rows,
        base_row,
        clamped,
        tier: tier_id,
        frames,
        warmup_s,
        look_at,
        look_width,
        bloom,
        thumbs,
        looks: vec![None; n],
        thumb_imgs: vec![None; n],
    }));
    Ok(())
}

/// A row's captures are in: compute its look measures (mean over the look
/// times), keep its thumbnail and drop its images.
pub(super) fn finish_row(run: &mut Run, row: usize) -> Poll {
    let Run { info, shots, .. } = run;
    let Info::Sweep(info) = info else {
        return Poll::Done;
    };
    let mut looks = Vec::new();
    for s in shots.iter().filter(|s| s.side == row && !s.reference) {
        let reference = (info.bloom != Bloom::Off)
            .then(|| {
                shots
                    .iter()
                    .find(|r| r.reference && (r.side == row || r.side == SHARED) && r.t == s.t)
                    .map(|r| &r.img)
            })
            .flatten();
        match look::compute(&s.img, reference) {
            Ok(l) => looks.push(l),
            Err(e) => return Poll::Fail(LiveError::new("internal", format!("look measures: {e}"))),
        }
        if s.thumb.is_some() {
            info.thumb_imgs[row].clone_from(&s.thumb);
        }
    }
    info.looks[row] = Look::mean(&looks);
    shots.retain(|s| s.side != row);
    Poll::Done
}

/// Median (nearest rank) of the finite values, `None` without any.
fn median(v: impl Iterator<Item = f32>) -> Option<f64> {
    let mut v: Vec<f32> = v.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(f64::from(percentile(&v, 50.0)))
}

fn round_to(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (v * k).round() / k
}

/// The costs of one row: work, CPU and GPU ms (medians), then per pass.
fn costs(samples: &[Sample], passes: &[(usize, &str)]) -> Vec<Option<f64>> {
    let mut out = vec![
        median(samples.iter().map(|s| s.work_ms)),
        median(samples.iter().map(|s| s.cpu_ms)),
        median(samples.iter().map(|s| s.gpu_total)),
    ];
    for (i, _) in passes {
        out.push(median(samples.iter().map(|s| s.passes[*i])));
    }
    out
}

pub(super) fn result(run: &Run, info: &SweepInfo, pass_names: &[&'static str]) -> Value {
    // Pass columns: every pass any row measured.
    let passes: Vec<(usize, &str)> = pass_names
        .iter()
        .enumerate()
        .take(MAX_PASSES)
        .filter(|(i, _)| run.samples.iter().flatten().any(|s| s.passes[*i].is_finite()))
        .map(|(i, n)| (i, *n))
        .collect();
    let mut columns: Vec<String> = info.names.clone();
    columns.extend(["work_ms", "cpu_ms", "gpu_ms"].map(String::from));
    columns.extend(passes.iter().map(|(_, n)| format!("gpu.{n}")));
    let look_cols = if info.look_at.is_empty() {
        0
    } else if info.bloom == Bloom::Off {
        look::NAMES.len() - 1
    } else {
        look::NAMES.len()
    };
    columns.extend(look::NAMES[..look_cols].iter().map(|n| (*n).to_string()));
    let num = |v: Option<f64>, places: i32| v.map_or(Value::Null, |x| json!(round_to(x, places)));
    let mut gpu_any = false;
    let rows: Vec<Value> = info
        .rows
        .iter()
        .enumerate()
        .map(|(i, vals)| {
            let mut r: Vec<Value> = vals.clone();
            let empty = Vec::new();
            let c = costs(run.samples.get(i).unwrap_or(&empty), &passes);
            gpu_any |= c[2].is_some();
            r.extend(c.into_iter().map(|v| num(v, 3)));
            let lv = info.looks[i].map(|l| l.values());
            for (k, name) in look::NAMES[..look_cols].iter().enumerate() {
                let places = if *name == "colorfulness" { 2 } else { 4 };
                r.push(num(lv.and_then(|l| l[k]), places));
            }
            Value::Array(r)
        })
        .collect();
    let mut v = json!({
        "run": run.id, "path": "bench", "tier": info.tier, "frames_per_value": info.frames,
        "warmup_s": info.warmup_s, "look_at": info.look_at, "look_width": info.look_width,
        "gpu_timing": if gpu_any { "timestamp" } else { "unavailable" },
        "truncated": run.truncated, "restored": true, "base_row": info.base_row,
        "columns": columns, "rows": rows,
    });
    if !info.clamped.is_empty() {
        v["clamped"] = json!(info.clamped);
    }
    if info.thumbs.is_some() {
        let thumbs: Vec<Value> = info
            .thumb_imgs
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.as_ref().map(|t| json!({ "row": i, "png_base64": png_b64(t), "width": t.w, "height": t.h }))
            })
            .collect();
        v["thumbs"] = json!(thumbs);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_from_values_and_grid() {
        let (n, r) = rows_param(&json!({ "param": "fx.bloom.intensity", "values": [0.2, 0.4] })).unwrap();
        assert_eq!(n, vec!["fx.bloom.intensity"]);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1]["fx.bloom.intensity"], 0.4);
        let (n, r) = rows_param(&json!({ "grid": { "b": [1, 2, 3], "a": [true, false] } })).unwrap();
        assert_eq!(n, vec!["a", "b"], "grid names in sorted order");
        assert_eq!(r.len(), 6);
        assert_eq!((r[0]["a"].clone(), r[0]["b"].clone()), (json!(true), json!(1)));
        assert_eq!((r[1]["a"].clone(), r[1]["b"].clone()), (json!(true), json!(2)), "the last name varies fastest");
        assert_eq!(r[3]["a"], false);
        let code = |v: Value| rows_param(&v).err().unwrap().code;
        assert_eq!(code(json!({})), "bad_request");
        assert_eq!(code(json!({ "param": "x" })), "bad_request");
        assert_eq!(code(json!({ "param": "x", "values": [] })), "bad_request");
        assert_eq!(code(json!({ "param": "x", "values": [1], "grid": { "y": [1] } })), "bad_request");
        assert_eq!(code(json!({ "grid": { "y": [] } })), "bad_request");
        assert_eq!(code(json!({ "param": "x", "values": vec![1; MAX_ROWS + 1] })), "invalid_value");
        assert_eq!(code(json!({ "grid": { "a": [1, 2, 3, 4, 5, 6], "b": [1, 2, 3, 4, 5, 6] } })), "invalid_value");
    }

    #[test]
    fn table_values_are_tidy() {
        assert_eq!(tidy(&json!(0.30000001192092896)), json!(0.3));
        assert_eq!(tidy(&json!([0.20000000298023224, 1, true])), json!([0.2, 1, true]));
        assert_eq!(tidy(&json!(5)), json!(5));
        assert_eq!(tidy(&json!("x")), json!("x"));
    }

    #[test]
    fn thumbnails_fit_the_box() {
        assert_eq!(fit(1280, 720, 160, 0), 160);
        assert_eq!(fit(1280, 720, 160, 45), 80);
        assert_eq!(fit(100, 100, 160, 0), 160, "downscale never widens; capped later");
        assert_eq!(fit(10, 1000, 160, 32), 1);
    }

    #[test]
    fn medians_skip_missing_values() {
        assert_eq!(median([3.0, f32::NAN, 1.0, 2.0].into_iter()), Some(2.0));
        assert_eq!(median([f32::NAN].into_iter()), None);
        assert_eq!(round_to(1.23456, 3), 1.235);
    }
}
