//! Per-frame telemetry (docs/LIVE_API.md sections 6.12, 8): a preallocated
//! ring of [`Sample`]s, window aggregation in the docs' shape, and
//! `telemetry.subscribe` / `telemetry.unsubscribe`.
//!
//! The per-frame path ([`Telemetry::sample`]) writes one `Copy` sample into a
//! fixed ring and never allocates. Aggregation sorts in a scratch buffer
//! whose capacity is reserved up front.

use evani_core::ecs::World;
use evani_core::effects::BudgetStatus;
use evani_core::overlay::RenderStats;
use evani_core::quality::QualityState;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use serde_json::{Map, Value, json};

use crate::LiveError;
use crate::session::LiveSession;

/// Samples kept (about 17 s at 60 fps).
pub const RING: usize = 1024;
/// Distinct GPU pass names tracked.
pub const MAX_PASSES: usize = 16;
/// Subscriptions at once (the docs allow 4 per connection; the page cannot
/// see connections, so this is the session-wide cap).
pub const MAX_SUBS: usize = 16;

/// One frame's numbers. `NaN` marks a missing GPU value.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub now_ms: f64,
    pub frame: u64,
    pub frame_ms: f32,
    pub cpu_ms: f32,
    pub gpu_total: f32,
    pub passes: [f32; MAX_PASSES],
    pub draw_calls: u32,
    pub triangles: u32,
    pub sprites: u32,
    pub budget_used: u32,
    pub budget_total: u32,
    pub render_scale: f32,
    pub tier: u16,
    /// Work time this frame: CPU plus GPU ms (CPU alone without a GPU
    /// figure). What auto-revert judges; the frame interval is vsync-bound.
    pub work_ms: f32,
    /// Stalled on purpose (capture, held clock, bench or A/B run, settle
    /// time): not judged by auto-revert.
    pub held: bool,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            now_ms: 0.0,
            frame: 0,
            frame_ms: 0.0,
            cpu_ms: 0.0,
            gpu_total: f32::NAN,
            passes: [f32::NAN; MAX_PASSES],
            draw_calls: 0,
            triangles: 0,
            sprites: 0,
            budget_used: 0,
            budget_total: 0,
            render_scale: 1.0,
            tier: 0,
            work_ms: 0.0,
            held: false,
        }
    }
}

/// Aggregate over a set of samples.
#[derive(Debug, Clone, Copy, Default)]
pub struct Agg {
    pub frames: usize,
    pub avg: f32,
    pub p95: f32,
    pub max: f32,
    pub cpu_avg: f32,
    /// Mean work ms (CPU + GPU) of the samples not held; `None` if all were.
    pub work_avg: Option<f32>,
    /// `None` when any sample lacked GPU timing.
    pub gpu_avg: Option<f32>,
    pub passes: [f32; MAX_PASSES],
    pub draw_calls: u32,
    pub triangles: u32,
    pub sprites: u32,
    /// The newest sample in the set.
    pub last: Sample,
}

/// One `telemetry.subscribe`.
#[derive(Debug, Clone, Copy)]
pub struct Sub {
    pub id: u64,
    pub rate_hz: f32,
    /// Bit per entry of [`FIELDS`].
    pub fields: u16,
    pub next_ms: f64,
    pub last_ms: f64,
}

/// `fields` names (6.12). `tier`, `t`, `frame` and `fps` are always sent.
pub const FIELDS: &[&str] = &[
    "frame_ms",
    "cpu_ms",
    "work_ms",
    "gpu_ms",
    "draw_calls",
    "triangles",
    "sprites",
    "budget",
    "render_scale",
];

/// The ring, the pass-name table and the subscriptions.
pub struct Telemetry {
    ring: Vec<Sample>,
    /// Next write position.
    next: usize,
    len: usize,
    pass_names: [&'static str; MAX_PASSES],
    pass_count: usize,
    scratch: Vec<f32>,
    pub subs: [Option<Sub>; MAX_SUBS],
    pub next_sub: u64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            ring: vec![Sample::default(); RING],
            next: 0,
            len: 0,
            pass_names: [""; MAX_PASSES],
            pass_count: 0,
            scratch: Vec::with_capacity(RING),
            subs: [None; MAX_SUBS],
            next_sub: 0,
        }
    }
}

impl core::fmt::Debug for Telemetry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Telemetry")
            .field("len", &self.len)
            .field("passes", &&self.pass_names[..self.pass_count])
            .finish_non_exhaustive()
    }
}

impl Telemetry {
    /// Ring capacity (fixed).
    pub fn capacity(&self) -> usize {
        self.ring.len()
    }

    /// Scratch capacity (fixed; aggregation never grows it).
    pub fn scratch_capacity(&self) -> usize {
        self.scratch.capacity()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn pass_names(&self) -> &[&'static str] {
        &self.pass_names[..self.pass_count]
    }

    /// The pass slot for `name`, assigned on first sight.
    fn pass_slot(&mut self, name: &'static str) -> Option<usize> {
        if let Some(i) = self.pass_names[..self.pass_count]
            .iter()
            .position(|n| *n == name)
        {
            return Some(i);
        }
        if self.pass_count < MAX_PASSES {
            self.pass_names[self.pass_count] = name;
            self.pass_count += 1;
            return Some(self.pass_count - 1);
        }
        None
    }

    /// Record one frame. No allocation.
    pub fn record(&mut self, mut s: Sample, passes: &[(&'static str, f32)]) {
        for &(name, ms) in passes {
            if let Some(i) = self.pass_slot(name) {
                s.passes[i] = ms;
            }
        }
        self.push(s);
    }

    fn push(&mut self, s: Sample) {
        if let Some(slot) = self.ring.get_mut(self.next) {
            *slot = s;
        }
        self.next = (self.next + 1) % RING;
        self.len = (self.len + 1).min(RING);
    }

    /// Samples oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Sample> {
        let start = (self.next + RING - self.len) % RING;
        (0..self.len).filter_map(move |k| self.ring.get((start + k) % RING))
    }

    pub fn latest(&self) -> Option<&Sample> {
        if self.len == 0 {
            return None;
        }
        self.ring.get((self.next + RING - 1) % RING)
    }

    /// Mean frame ms of samples with `from_ms < now_ms <= to_ms` and
    /// `frame >= min_frame`; `None` if there are none. No allocation.
    pub fn mean_frame_ms(&self, from_ms: f64, to_ms: f64, min_frame: u64) -> Option<f32> {
        let (mut sum, mut n) = (0.0f64, 0u32);
        for s in self.iter() {
            if s.now_ms > from_ms && s.now_ms <= to_ms && s.frame >= min_frame {
                sum += f64::from(s.frame_ms);
                n += 1;
            }
        }
        (n > 0).then(|| (sum / f64::from(n)) as f32)
    }

    /// Mean work ms (CPU + GPU) of the samples in the same window that are
    /// not held; `None` if there are none. No allocation.
    pub fn mean_work_ms(&self, from_ms: f64, to_ms: f64, min_frame: u64) -> Option<f32> {
        let (mut sum, mut n) = (0.0f64, 0u32);
        for s in self.iter() {
            if s.now_ms > from_ms && s.now_ms <= to_ms && s.frame >= min_frame && !s.held {
                sum += f64::from(s.work_ms);
                n += 1;
            }
        }
        (n > 0).then(|| (sum / f64::from(n)) as f32)
    }

    /// Aggregate samples with `from_ms < now_ms <= to_ms` and
    /// `frame >= min_frame`. Uses the preallocated scratch for p95.
    pub fn aggregate(&mut self, from_ms: f64, to_ms: f64, min_frame: u64) -> Option<Agg> {
        let mut scratch = core::mem::take(&mut self.scratch);
        scratch.clear();
        let mut a = Agg::default();
        let (mut sum, mut cpu, mut gpu, mut gpu_ok) = (0.0f64, 0.0f64, 0.0f64, true);
        let (mut work, mut work_n) = (0.0f64, 0u32);
        let mut pass_sum = [0.0f64; MAX_PASSES];
        let mut pass_n = [0u32; MAX_PASSES];
        let (mut dc, mut tri, mut spr) = (0u64, 0u64, 0u64);
        for s in self.iter() {
            if !(s.now_ms > from_ms && s.now_ms <= to_ms && s.frame >= min_frame) {
                continue;
            }
            a.frames += 1;
            if scratch.len() < scratch.capacity() {
                scratch.push(s.frame_ms);
            }
            sum += f64::from(s.frame_ms);
            a.max = a.max.max(s.frame_ms);
            cpu += f64::from(s.cpu_ms);
            if !s.held {
                work += f64::from(s.work_ms);
                work_n += 1;
            }
            if s.gpu_total.is_nan() {
                gpu_ok = false;
            } else {
                gpu += f64::from(s.gpu_total);
            }
            for (i, v) in s.passes.iter().enumerate() {
                if !v.is_nan() {
                    pass_sum[i] += f64::from(*v);
                    pass_n[i] += 1;
                }
            }
            dc += u64::from(s.draw_calls);
            tri += u64::from(s.triangles);
            spr += u64::from(s.sprites);
            a.last = *s;
        }
        let result = if a.frames == 0 {
            None
        } else {
            let n = a.frames as f64;
            a.avg = (sum / n) as f32;
            a.cpu_avg = (cpu / n) as f32;
            a.work_avg = (work_n > 0).then(|| (work / f64::from(work_n)) as f32);
            a.gpu_avg = gpu_ok.then(|| (gpu / n) as f32);
            for i in 0..MAX_PASSES {
                a.passes[i] = if pass_n[i] > 0 {
                    (pass_sum[i] / f64::from(pass_n[i])) as f32
                } else {
                    f32::NAN
                };
            }
            a.draw_calls = (dc as f64 / n).round() as u32;
            a.triangles = (tri as f64 / n).round() as u32;
            a.sprites = (spr as f64 / n).round() as u32;
            let k = ((scratch.len() as f64 * 0.95).ceil() as usize).clamp(1, scratch.len()) - 1;
            let (_, p, _) = scratch.select_nth_unstable_by(k, f32::total_cmp);
            a.p95 = *p;
            Some(a)
        };
        self.scratch = scratch;
        result
    }

    /// `gpu_ms` in the docs' shape, or `null` without GPU timing.
    pub fn gpu_json(&self, a: &Agg) -> Value {
        match a.gpu_avg {
            None => Value::Null,
            Some(total) => {
                let mut passes = Map::new();
                for (i, name) in self.pass_names().iter().enumerate() {
                    if let Some(v) = a.passes.get(i).filter(|v| !v.is_nan()) {
                        passes.insert((*name).to_string(), json!(round2(*v)));
                    }
                }
                json!({ "avg_total": round2(total), "passes": passes })
            }
        }
    }
}

pub fn round2(v: f32) -> f64 {
    (f64::from(v) * 100.0).round() / 100.0
}

/// Read this frame's numbers from the world and record them (called once per
/// frame from [`crate::tick`]). No allocation: the pass list is copied into a
/// fixed array.
pub fn sample(world: &World) {
    let Some(time) = world
        .try_res::<Time>()
        .map(|t| (t.now_ms, t.frame, t.frame_dt, t.cpu_ms))
    else {
        return;
    };
    let (now_ms, frame, frame_dt, cpu_ms) = time;
    if world
        .try_res::<LiveSession>()
        .is_some_and(|l| l.tel.latest().is_some_and(|s| s.frame == frame))
    {
        return;
    }
    let mut s = Sample {
        now_ms,
        frame,
        frame_ms: frame_dt * 1000.0,
        cpu_ms,
        ..Sample::default()
    };
    let mut passes: [(&'static str, f32); MAX_PASSES] = [("", 0.0); MAX_PASSES];
    let mut np = 0;
    if let Some(r) = world.try_res::<RenderStats>() {
        s.gpu_total = r.gpu_ms.unwrap_or(f32::NAN);
        s.draw_calls = r.draw_calls;
        s.triangles = r.triangles;
        s.sprites = r.sprites;
        for p in r.passes.iter().take(MAX_PASSES) {
            if let Some(slot) = passes.get_mut(np) {
                *slot = (p.name, p.gpu_ms);
                np += 1;
            }
        }
    }
    if let Some(b) = world.try_res::<BudgetStatus>() {
        s.budget_used = b.used;
        s.budget_total = b.total;
    }
    let mut scale = 1.0;
    if let Some(t) = world.try_res::<Tiers>() {
        s.tier = u16::try_from(t.active_index()).unwrap_or(u16::MAX);
        scale = t.active().render_scale_or_native();
    }
    if let Some(q) = world.try_res::<QualityState>() {
        scale *= q.scale;
        s.held = q.held;
    }
    s.render_scale = scale;
    s.work_ms = cpu_ms + if s.gpu_total.is_nan() { 0.0 } else { s.gpu_total };
    if let Some(mut l) = world.try_res_mut::<LiveSession>() {
        // A bench or compare run stalls and changes state on purpose.
        s.held |= l.run.is_some();
        l.tel.record(s, &passes[..np]);
    }
}

fn tier_name(world: &World, index: u16) -> Value {
    world.try_res::<Tiers>().map_or(Value::Null, |t| {
        t.presets()
            .get(usize::from(index))
            .map_or(Value::Null, |p| json!(p.id))
    })
}

/// The change result's `telemetry` (5.4) over `(from_ms, to_ms]` from
/// `min_frame`, plus `baseline_frame_ms`.
pub fn change_telemetry(
    world: &World,
    from_ms: f64,
    to_ms: f64,
    min_frame: u64,
    baseline: Option<f32>,
) -> Value {
    let mut s = world.res_mut::<LiveSession>();
    let window = (to_ms - from_ms).round();
    let Some(a) = s.tel.aggregate(from_ms, to_ms, min_frame) else {
        return json!({ "window_ms": window, "frames": 0, "frame_ms": null, "cpu_ms": null,
                       "gpu_ms": null, "gpu_timing": "unavailable",
                       "baseline_frame_ms": baseline.map(round2) });
    };
    let gpu = s.tel.gpu_json(&a);
    json!({
        "window_ms": window, "frames": a.frames,
        "frame_ms": { "avg": round2(a.avg), "p95": round2(a.p95), "max": round2(a.max) },
        "cpu_ms": { "avg": round2(a.cpu_avg) },
        "work_ms": { "avg": a.work_avg.map(round2) },
        "gpu_timing": if gpu.is_null() { "unavailable" } else { "timestamp" },
        "gpu_ms": gpu,
        "draw_calls": a.draw_calls, "triangles": a.triangles, "sprites": a.sprites,
        "baseline_frame_ms": baseline.map(round2),
    })
}

// ---- subscriptions ----------------------------------------------------------------

/// `telemetry.subscribe` (6.12).
pub fn subscribe(world: &World, params: &Value) -> Result<Value, LiveError> {
    let rate = match params.get("rate_hz") {
        None | Some(Value::Null) => 2.0,
        Some(v) => v
            .as_f64()
            .filter(|r| (0.5..=60.0).contains(r))
            .ok_or_else(|| LiveError::new("invalid_value", "rate_hz must be 0.5–60"))?,
    } as f32;
    let mut fields = 0u16;
    match params.get("fields") {
        None | Some(Value::Null) => fields = u16::MAX,
        Some(Value::Array(list)) => {
            for f in list {
                let name = f.as_str().unwrap_or("");
                let bit = FIELDS.iter().position(|n| *n == name).ok_or_else(|| {
                    LiveError::new("invalid_value", format!("unknown telemetry field {name}"))
                        .with(json!({ "fields": FIELDS }))
                })?;
                fields |= 1 << bit;
            }
        }
        Some(_) => return Err(LiveError::new("bad_request", "fields must be a list")),
    }
    let now = world.try_res::<Time>().map_or(0.0, |t| t.now_ms);
    let mut s = world.res_mut::<LiveSession>();
    let Some(slot) = s.tel.subs.iter().position(Option::is_none) else {
        return Err(LiveError::new(
            "limit",
            format!("at most {MAX_SUBS} telemetry subscriptions"),
        ));
    };
    s.tel.next_sub += 1;
    let id = s.tel.next_sub;
    let period = 1000.0 / f64::from(rate);
    if let Some(e) = s.tel.subs.get_mut(slot) {
        *e = Some(Sub {
            id,
            rate_hz: rate,
            fields,
            next_ms: now + period,
            last_ms: now,
        });
    }
    Ok(json!({ "sub": format!("t-{id}"), "rate_hz": rate }))
}

/// `telemetry.unsubscribe` (6.13).
pub fn unsubscribe(world: &World, params: &Value) -> Result<Value, LiveError> {
    let name = params
        .get("sub")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.sub is required"))?;
    let id = name
        .strip_prefix("t-")
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or_else(|| LiveError::new("not_found", format!("no subscription {name}")))?;
    let mut s = world.res_mut::<LiveSession>();
    let slot = s
        .tel
        .subs
        .iter_mut()
        .find(|e| e.is_some_and(|x| x.id == id))
        .ok_or_else(|| LiveError::new("not_found", format!("no subscription {name}")))?;
    *slot = None;
    Ok(json!({ "sub": name, "stopped": true }))
}

/// Push a `telemetry` event for every subscription that is due.
pub fn pump_subs(world: &World) {
    let Some(now) = world.try_res::<Time>().map(|t| t.now_ms) else {
        return;
    };
    let due: [Option<Sub>; MAX_SUBS] = {
        let s = world.res::<LiveSession>();
        let mut d = [None; MAX_SUBS];
        for (i, e) in s.tel.subs.iter().enumerate() {
            if let Some(sub) = e.filter(|x| now >= x.next_ms) {
                d[i] = Some(sub);
            }
        }
        d
    };
    for (i, sub) in due.iter().enumerate() {
        let Some(sub) = sub else { continue };
        let ev = telemetry_event(world, sub, now);
        let mut s = world.res_mut::<LiveSession>();
        let period = 1000.0 / f64::from(sub.rate_hz);
        if let Some(Some(e)) = s.tel.subs.get_mut(i) {
            e.last_ms = now;
            // Catch up without bursts after a stall.
            e.next_ms = (e.next_ms + period).max(now + period * 0.5);
        }
        if let Some(ev) = ev {
            s.events.push(ev);
        }
    }
}

fn telemetry_event(world: &World, sub: &Sub, now: f64) -> Option<String> {
    let (a, gpu) = {
        let mut s = world.res_mut::<LiveSession>();
        let a = s.tel.aggregate(sub.last_ms, now, 0)?;
        let gpu = s.tel.gpu_json(&a);
        (a, gpu)
    };
    let has = |name: &str| {
        FIELDS
            .iter()
            .position(|n| *n == name)
            .is_some_and(|b| sub.fields & (1 << b) != 0)
    };
    let mut d = json!({
        "t": (a.last.now_ms / 10.0).round() / 100.0,
        "frame": a.last.frame,
        "tier": tier_name(world, a.last.tier),
        "fps": if a.avg > 0.0 { round2(1000.0 / a.avg) } else { 0.0 },
    });
    if has("frame_ms") {
        d["frame_ms"] = json!({ "avg": round2(a.avg), "p95": round2(a.p95), "max": round2(a.max) });
    }
    if has("cpu_ms") {
        d["cpu_ms"] = json!({ "avg": round2(a.cpu_avg) });
    }
    if has("work_ms") {
        d["work_ms"] = json!({ "avg": a.work_avg.map(round2) });
    }
    if has("gpu_ms") {
        d["gpu_ms"] = gpu;
    }
    if has("draw_calls") {
        d["draw_calls"] = json!(a.draw_calls);
    }
    if has("triangles") {
        d["triangles"] = json!(a.triangles);
    }
    if has("sprites") {
        d["sprites"] = json!(a.sprites);
    }
    if has("budget") {
        d["budget"] = json!({ "used": a.last.budget_used, "limit": a.last.budget_total });
    }
    if has("render_scale") {
        d["render_scale"] = json!(round2(a.last.render_scale));
        d["dpr"] = Value::Null;
    }
    Some(
        json!({ "type": "event", "v": 1, "event": "telemetry", "sub": format!("t-{}", sub.id),
                "data": d })
        .to_string(),
    )
}
