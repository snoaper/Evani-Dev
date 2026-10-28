//! A look's capture moments and limits, the check, and the old-vs-new diff
//! a proposal shows (docs/LIVE_API.md 15.3, 15.4).
//!
//! Two layers, both in `limits.json`:
//!
//! - **Layer A (drift):** each moment against its own baseline, with a
//!   tolerance per stat (`abs`, `rel` or one-sided `up`).
//! - **Layer B (band):** each stat against the look's band: min..max over
//!   the baseline moments, widened on each side by
//!   max(`fraction` × range, the stat's floor).
//!
//! Every number is a placeholder until tuned on real baselines; all of them
//! are part of the guard set, so changing any needs Chris's approval.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::stats::{self, Mode, STATS, StatDef, Stats, fmt, round6};

pub const LIMITS_FORMAT: &str = "evani-look-limits/1";
pub const MOMENTS_FORMAT: &str = "evani-look-moments/1";
/// Default Layer B widening: 10 % of the baseline range.
pub const BAND_FRACTION: f64 = 0.10;

// ---------------------------------------------------------------- moments

/// One scripted capture moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Moment {
    pub name: String,
    /// The `capture.frame` params that frame it: only `sim_time`, `camera`
    /// and `moment` are allowed.
    pub capture: Value,
}

/// `moments.json`: which game, tier and width, and the moments.
#[derive(Debug, Clone, PartialEq)]
pub struct Moments {
    pub game: String,
    pub tier: String,
    pub width: u32,
    pub moments: Vec<Moment>,
}

fn check_capture(name: &str, c: &Value) -> Result<(), String> {
    let o = c.as_object().ok_or_else(|| format!("moment '{name}': capture must be an object"))?;
    for (k, v) in o {
        match k.as_str() {
            "sim_time" => {
                if !v.as_f64().is_some_and(|t| t.is_finite() && t >= 0.0) {
                    return Err(format!("moment '{name}': sim_time must be a number ≥ 0"));
                }
            }
            "camera" => {
                let ok = matches!(v.as_str(), Some("current" | "path"))
                    || (v["eye"].as_array().is_some_and(|a| a.len() == 3) && v["target"].as_array().is_some_and(|a| a.len() == 3));
                if !ok {
                    return Err(format!("moment '{name}': camera must be \"current\", \"path\" or {{eye, target}}"));
                }
            }
            "moment" => {
                if !v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 64) {
                    return Err(format!("moment '{name}': moment must be a game moment name"));
                }
            }
            other => return Err(format!("moment '{name}': capture key {other} is not allowed (sim_time, camera, moment)")),
        }
    }
    Ok(())
}

impl Moments {
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("moments.json is not JSON: {e}"))?;
        Self::from_json(&v)
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        if v["format"].as_str() != Some(MOMENTS_FORMAT) {
            return Err(format!("moments.json format is not {MOMENTS_FORMAT}"));
        }
        let game = v["game"].as_str().filter(|s| !s.is_empty()).ok_or("moments.json has no game")?;
        let tier = v["tier"].as_str().filter(|s| !s.is_empty()).ok_or("moments.json has no tier")?;
        let width = v["width"].as_u64().filter(|w| (64..=4096).contains(w)).ok_or("moments.json width must be 64–4096")? as u32;
        let list = v["moments"].as_array().filter(|a| !a.is_empty()).ok_or("moments.json has no moments")?;
        let mut moments: Vec<Moment> = Vec::new();
        for m in list {
            let name = m["name"].as_str().ok_or("a moment has no name")?;
            if !crate::guardset::is_moment_name(name) {
                return Err(format!("moment name {name:?} must be [a-z0-9_-], 1–64 chars"));
            }
            if moments.iter().any(|x| x.name == name) {
                return Err(format!("moment '{name}' is listed twice"));
            }
            let capture = m.get("capture").cloned().unwrap_or_else(|| json!({}));
            check_capture(name, &capture)?;
            moments.push(Moment { name: name.into(), capture });
        }
        if moments.len() > 16 {
            return Err("at most 16 moments".into());
        }
        Ok(Moments { game: game.into(), tier: tier.into(), width, moments })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "format": MOMENTS_FORMAT,
            "game": self.game,
            "tier": self.tier,
            "width": self.width,
            "moments": self.moments.iter().map(|m| json!({ "name": m.name, "capture": m.capture })).collect::<Vec<_>>(),
        })
    }

    pub fn file_text(&self) -> String {
        pretty(&self.to_json())
    }
}

/// Pretty JSON with one trailing newline (how guard files are written).
pub fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

// ---------------------------------------------------------------- limits

/// A Layer A tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    pub mode: Mode,
    pub value: f64,
    /// `rel` only: the smallest allowed difference.
    pub min: f64,
}

impl Tolerance {
    pub fn default_for(d: &StatDef) -> Self {
        Tolerance { mode: d.mode, value: d.tol, min: d.tol_min }
    }

    /// The allowed (low, high) around `base`. `up` has no low limit.
    pub fn bounds(&self, base: f64) -> (Option<f64>, f64) {
        match self.mode {
            Mode::Abs => (Some(base - self.value), base + self.value),
            Mode::Rel => {
                let d = (self.value * base.abs()).max(self.min);
                (Some(base - d), base + d)
            }
            Mode::Up => (None, base + self.value),
        }
    }

    fn to_json(self) -> Value {
        let mut m = Map::new();
        m.insert("mode".into(), json!(self.mode.as_str()));
        m.insert("value".into(), json!(round6(self.value)));
        if self.mode == Mode::Rel {
            m.insert("min".into(), json!(round6(self.min)));
        }
        Value::Object(m)
    }

    fn from_json(id: &str, v: &Value) -> Result<Self, String> {
        let mode = v["mode"].as_str().and_then(Mode::parse).ok_or_else(|| format!("tolerance {id}: mode must be abs, rel or up"))?;
        let value = v["value"].as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("tolerance {id}: value must be ≥ 0"))?;
        let min = match &v["min"] {
            Value::Null => 0.0,
            x => x.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("tolerance {id}: min must be ≥ 0"))?,
        };
        Ok(Tolerance { mode, value, min })
    }

    fn describe(&self, d: &StatDef) -> String {
        match self.mode {
            Mode::Abs => format!("±{}", fmt(d, self.value)),
            Mode::Rel => format!("±{:.0}% (at least {})", self.value * 100.0, fmt(d, self.min)),
            Mode::Up => format!("up to +{}", fmt(d, self.value)),
        }
    }
}

/// `limits.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    pub look: String,
    pub tolerances: BTreeMap<String, Tolerance>,
    pub band_fraction: f64,
    pub band_floors: BTreeMap<String, f64>,
    /// Baseline stats per moment.
    pub baseline: BTreeMap<String, Stats>,
    /// Layer B band per stat: (low, high).
    pub band: BTreeMap<String, (f64, f64)>,
}

/// The band: min..max over the baseline moments, widened by
/// max(fraction × range, floor) on each side.
pub fn compute_band(
    baseline: &BTreeMap<String, Stats>,
    fraction: f64,
    floors: &BTreeMap<String, f64>,
) -> BTreeMap<String, (f64, f64)> {
    let mut band = BTreeMap::new();
    for d in &STATS {
        let vals: Vec<f64> = baseline.values().filter_map(|s| s.get(d.id)).collect();
        if vals.is_empty() {
            continue;
        }
        let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let floor = floors.get(d.id).copied().unwrap_or(d.band_floor);
        let w = (fraction * (hi - lo)).max(floor);
        band.insert(d.id.to_string(), (round6(lo - w), round6(hi + w)));
    }
    band
}

impl Limits {
    /// Limits for a new baseline. Tolerances and widening come from `prev`
    /// (the look's current limits) or the defaults.
    pub fn generate(look: &str, baseline: BTreeMap<String, Stats>, prev: Option<&Limits>) -> Self {
        let tolerances = prev.map_or_else(
            || STATS.iter().map(|d| (d.id.to_string(), Tolerance::default_for(d))).collect(),
            |p| p.tolerances.clone(),
        );
        let band_fraction = prev.map_or(BAND_FRACTION, |p| p.band_fraction);
        let band_floors = prev.map_or_else(
            || STATS.iter().map(|d| (d.id.to_string(), d.band_floor)).collect(),
            |p| p.band_floors.clone(),
        );
        let baseline: BTreeMap<String, Stats> = baseline.into_iter().map(|(k, s)| (k, round_stats(&s))).collect();
        let band = compute_band(&baseline, band_fraction, &band_floors);
        Limits { look: look.into(), tolerances, band_fraction, band_floors, baseline, band }
    }

    pub fn tolerance(&self, id: &str) -> Tolerance {
        self.tolerances
            .get(id)
            .copied()
            .or_else(|| stats::def(id).map(Tolerance::default_for))
            .unwrap_or(Tolerance { mode: Mode::Abs, value: 0.0, min: 0.0 })
    }

    pub fn to_json(&self) -> Value {
        let tol: Map<String, Value> = self.tolerances.iter().map(|(k, t)| (k.clone(), t.to_json())).collect();
        let floors: Map<String, Value> = self.band_floors.iter().map(|(k, f)| (k.clone(), json!(round6(*f)))).collect();
        let base: Map<String, Value> = self.baseline.iter().map(|(k, s)| (k.clone(), s.to_json())).collect();
        let band: Map<String, Value> = self.band.iter().map(|(k, (lo, hi))| (k.clone(), json!([lo, hi]))).collect();
        json!({
            "format": LIMITS_FORMAT,
            "look": self.look,
            "stat_version": crate::guardset::STAT_VERSION,
            "tolerances": tol,
            "band_widen": { "fraction": round6(self.band_fraction), "floors": floors },
            "baseline": base,
            "band": band,
        })
    }

    pub fn file_text(&self) -> String {
        pretty(&self.to_json())
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("limits.json is not JSON: {e}"))?;
        if v["format"].as_str() != Some(LIMITS_FORMAT) {
            return Err(format!("limits.json format is not {LIMITS_FORMAT}"));
        }
        if v["stat_version"].as_u64() != Some(u64::from(crate::guardset::STAT_VERSION)) {
            return Err(format!(
                "limits.json was made with stat version {}, this engine computes version {}: re-baseline (needs approval)",
                v["stat_version"], crate::guardset::STAT_VERSION
            ));
        }
        let look = v["look"].as_str().ok_or("limits.json has no look")?.to_string();
        let mut tolerances = BTreeMap::new();
        for (k, t) in v["tolerances"].as_object().ok_or("limits.json has no tolerances")? {
            if stats::def(k).is_none() {
                return Err(format!("limits.json: unknown stat {k}"));
            }
            tolerances.insert(k.clone(), Tolerance::from_json(k, t)?);
        }
        let band_fraction = v["band_widen"]["fraction"].as_f64().filter(|f| f.is_finite() && *f >= 0.0).ok_or("limits.json band_widen.fraction must be ≥ 0")?;
        let mut band_floors = BTreeMap::new();
        if let Some(o) = v["band_widen"]["floors"].as_object() {
            for (k, f) in o {
                band_floors.insert(k.clone(), f.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("band floor {k} must be ≥ 0"))?);
            }
        }
        let mut baseline = BTreeMap::new();
        for (k, s) in v["baseline"].as_object().filter(|o| !o.is_empty()).ok_or("limits.json has no baseline")? {
            baseline.insert(k.clone(), Stats::from_json(s).map_err(|e| format!("baseline {k}: {e}"))?);
        }
        let mut band = BTreeMap::new();
        for (k, b) in v["band"].as_object().ok_or("limits.json has no band")? {
            let lo = b[0].as_f64().ok_or_else(|| format!("band {k} must be [low, high]"))?;
            let hi = b[1].as_f64().ok_or_else(|| format!("band {k} must be [low, high]"))?;
            if lo > hi {
                return Err(format!("band {k} has low above high"));
            }
            band.insert(k.clone(), (lo, hi));
        }
        Ok(Limits { look, tolerances, band_fraction, band_floors, baseline, band })
    }

    /// These limits with new settings (what `look.limits.propose` asks
    /// for), the band recomputed from the same baseline. `patch`:
    /// `{"tolerances": {stat: {mode, value, min?}}, "band_widen":
    /// {"fraction"?: f, "floors"?: {stat: f}}}`; anything left out stays.
    pub fn with_settings(&self, patch: &Value) -> Result<Limits, String> {
        let o = patch.as_object().ok_or("the limits change must be an object")?;
        for k in o.keys() {
            if !matches!(k.as_str(), "tolerances" | "band_widen") {
                return Err(format!("unknown limits setting {k} (tolerances, band_widen)"));
            }
        }
        let mut out = self.clone();
        if let Some(t) = patch.get("tolerances").filter(|v| !v.is_null()) {
            for (k, v) in t.as_object().ok_or("tolerances must be an object of stat to {mode, value, min}")? {
                if stats::def(k).is_none() {
                    let ids: Vec<&str> = STATS.iter().map(|d| d.id).collect();
                    return Err(format!("unknown stat {k} (one of {})", ids.join(", ")));
                }
                out.tolerances.insert(k.clone(), Tolerance::from_json(k, v)?);
            }
        }
        if let Some(b) = patch.get("band_widen").filter(|v| !v.is_null()) {
            if let Some(f) = b.get("fraction").filter(|v| !v.is_null()) {
                out.band_fraction = f.as_f64().filter(|x| x.is_finite() && (0.0..=10.0).contains(x)).ok_or("band_widen.fraction must be 0-10")?;
            }
            if let Some(fl) = b.get("floors").filter(|v| !v.is_null()) {
                for (k, v) in fl.as_object().ok_or("band_widen.floors must be an object of stat to number")? {
                    if stats::def(k).is_none() {
                        return Err(format!("unknown stat {k} in band_widen.floors"));
                    }
                    out.band_floors.insert(k.clone(), v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("band floor {k} must be ≥ 0"))?);
                }
            }
        }
        out.band = compute_band(&out.baseline, out.band_fraction, &out.band_floors);
        Ok(out)
    }
}

fn round_stats(s: &Stats) -> Stats {
    Stats {
        mean_luma: round6(s.mean_luma),
        log_avg: round6(s.log_avg),
        luma_std: round6(s.luma_std),
        luma_range: round6(s.luma_range),
        saturation: round6(s.saturation),
        colorfulness: round6(s.colorfulness),
        clipped: round6(s.clipped),
        crushed: round6(s.crushed),
        bloom: s.bloom.map(round6),
    }
}

// ---------------------------------------------------------------- check

/// One stat outside the limits.
#[derive(Debug, Clone, PartialEq)]
pub struct Breach {
    pub moment: String,
    pub stat: String,
    /// `drift` (Layer A), `band` (Layer B) or `missing` (moment not captured).
    pub layer: &'static str,
    pub value: f64,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub baseline: Option<f64>,
    pub sentence: String,
}

impl Breach {
    /// Stable id used in the bake approval hash: `moment:stat:layer`.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.moment, self.stat, self.layer)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "moment": self.moment, "stat": self.stat, "layer": self.layer,
            "value": round6(self.value), "low": self.low.map(round6), "high": self.high.map(round6),
            "baseline": self.baseline.map(round6), "sentence": self.sentence,
        })
    }
}

fn limit_text(d: &StatDef, low: Option<f64>, high: f64) -> String {
    match low {
        Some(lo) => format!("{}–{}", fmt(d, lo), fmt(d, high)),
        None => fmt(d, high),
    }
}

/// Compare the current stats per moment with the limits. Every baseline
/// moment must be present; extra moments are ignored.
pub fn check(limits: &Limits, current: &BTreeMap<String, Stats>) -> Vec<Breach> {
    let mut out = Vec::new();
    for (moment, base) in &limits.baseline {
        let Some(cur) = current.get(moment) else {
            out.push(Breach {
                moment: moment.clone(),
                stat: "capture".into(),
                layer: "missing",
                value: 0.0,
                low: None,
                high: None,
                baseline: None,
                sentence: format!("moment '{moment}' was not captured, so it could not be checked"),
            });
            continue;
        };
        for d in &STATS {
            let (v, b) = match (cur.get(d.id), base.get(d.id)) {
                (Some(v), Some(b)) => (v, b),
                (None, Some(b)) => {
                    // Measured in the baseline but not now (e.g. the bloom-off
                    // capture failed): flagged, never skipped.
                    out.push(Breach {
                        moment: moment.clone(),
                        stat: d.id.into(),
                        layer: "missing",
                        value: 0.0,
                        low: None,
                        high: None,
                        baseline: Some(b),
                        sentence: format!("{} could not be measured on moment '{moment}' (baseline {})", d.label, fmt(d, b)),
                    });
                    continue;
                }
                _ => continue,
            };
            let tol = limits.tolerance(d.id);
            let (lo, hi) = tol.bounds(b);
            let eps = 1e-9;
            if v > hi + eps || lo.is_some_and(|lo| v < lo - eps) {
                let shown_lo = if tol.mode == Mode::Up { None } else { lo };
                out.push(Breach {
                    moment: moment.clone(),
                    stat: d.id.into(),
                    layer: "drift",
                    value: v,
                    low: shown_lo,
                    high: Some(hi),
                    baseline: Some(b),
                    sentence: format!(
                        "{} {} vs limit {} on moment '{moment}' (baseline {})",
                        d.label,
                        fmt(d, v),
                        limit_text(d, shown_lo, hi),
                        fmt(d, b)
                    ),
                });
            }
            if let Some(&(blo, bhi)) = limits.band.get(d.id) {
                let up_only = tol.mode == Mode::Up;
                if v > bhi + eps || (!up_only && v < blo - eps) {
                    let shown_lo = if up_only { None } else { Some(blo) };
                    out.push(Breach {
                        moment: moment.clone(),
                        stat: d.id.into(),
                        layer: "band",
                        value: v,
                        low: shown_lo,
                        high: Some(bhi),
                        baseline: Some(b),
                        sentence: format!(
                            "{} {} vs the look's band {} on moment '{moment}' (baseline {})",
                            d.label,
                            fmt(d, v),
                            limit_text(d, shown_lo, bhi),
                            fmt(d, b)
                        ),
                    });
                }
            }
        }
    }
    out
}

/// The hash a flagged bake's approval is bound to: the look, its guard
/// hash, the capture set, the bake bundle and the flagged stats.
pub fn bake_hash(look: &str, guard_hash: &str, captures_hash: &str, bundle_hash: &str, breaches: &[Breach]) -> String {
    let mut ids: Vec<String> = breaches.iter().map(Breach::id).collect();
    ids.sort();
    ids.dedup();
    let text = format!(
        "evani-look-bake/1\nlook={look}\nguard={guard_hash}\ncaptures={captures_hash}\nbundle={bundle_hash}\nflags={}\n",
        ids.join(",")
    );
    crate::sha256_hex(text.as_bytes())
}

// ---------------------------------------------------------------- diff

/// One old-vs-new row of a proposal, for Chris's approval card.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    /// `tolerance`, `band`, `band_widen`, `baseline` or `moments`.
    pub section: &'static str,
    pub moment: Option<String>,
    pub stat: Option<String>,
    pub old: Value,
    pub new: Value,
    /// Lets more through than before (or moves what counts as normal).
    pub looser: bool,
    pub text: String,
}

impl DiffRow {
    pub fn to_json(&self) -> Value {
        json!({
            "section": self.section, "moment": self.moment, "stat": self.stat,
            "old": self.old, "new": self.new, "looser": self.looser, "text": self.text,
        })
    }
}

fn changed(a: f64, b: f64) -> bool {
    (a - b).abs() > 1e-9
}

/// The rows that differ between the current guard set and a proposed one.
/// `old` is `None` for a look's first limits (every row is new).
pub fn diff(
    old: Option<(&Moments, &Limits)>,
    new: (&Moments, &Limits),
) -> Vec<DiffRow> {
    let (nm, nl) = new;
    let mut rows = Vec::new();
    let Some((om, ol)) = old else {
        rows.push(DiffRow {
            section: "moments",
            moment: None,
            stat: None,
            old: Value::Null,
            new: json!(nm.moments.iter().map(|m| m.name.clone()).collect::<Vec<_>>()),
            looser: false,
            text: format!("first limits for {}: {} moments on {} at tier {}", nl.look, nm.moments.len(), nm.game, nm.tier),
        });
        for d in &STATS {
            if let Some(&(lo, hi)) = nl.band.get(d.id) {
                let t = nl.tolerance(d.id);
                rows.push(DiffRow {
                    section: "band",
                    moment: None,
                    stat: Some(d.id.into()),
                    old: Value::Null,
                    new: json!([lo, hi]),
                    looser: false,
                    text: format!("{}: band {}–{}, drift {}", d.label, fmt(d, lo), fmt(d, hi), t.describe(d)),
                });
            }
        }
        return rows;
    };
    // Moments.
    for m in &nm.moments {
        match om.moments.iter().find(|x| x.name == m.name) {
            None => rows.push(DiffRow {
                section: "moments", moment: Some(m.name.clone()), stat: None, old: Value::Null, new: m.capture.clone(),
                looser: true, text: format!("moment '{}' added", m.name),
            }),
            Some(o) if o.capture != m.capture => rows.push(DiffRow {
                section: "moments", moment: Some(m.name.clone()), stat: None, old: o.capture.clone(), new: m.capture.clone(),
                looser: true, text: format!("moment '{}' is framed differently", m.name),
            }),
            Some(_) => {}
        }
    }
    for o in &om.moments {
        if !nm.moments.iter().any(|m| m.name == o.name) {
            rows.push(DiffRow {
                section: "moments", moment: Some(o.name.clone()), stat: None, old: o.capture.clone(), new: Value::Null,
                looser: true, text: format!("moment '{}' removed (no longer checked)", o.name),
            });
        }
    }
    for (k, a, b) in [("game", &om.game, &nm.game), ("tier", &om.tier, &nm.tier)] {
        if a != b {
            rows.push(DiffRow {
                section: "moments", moment: None, stat: None, old: json!(a), new: json!(b),
                looser: true, text: format!("captures use {k} {b} instead of {a}"),
            });
        }
    }
    if om.width != nm.width {
        rows.push(DiffRow {
            section: "moments", moment: None, stat: None, old: json!(om.width), new: json!(nm.width),
            looser: true, text: format!("captures are {} px wide instead of {}", nm.width, om.width),
        });
    }
    // Tolerances.
    for d in &STATS {
        let (a, b) = (ol.tolerance(d.id), nl.tolerance(d.id));
        if a != b {
            let looser = a.mode != b.mode || b.value > a.value + 1e-12 || (b.mode == Mode::Rel && b.min > a.min + 1e-12);
            rows.push(DiffRow {
                section: "tolerance", moment: None, stat: Some(d.id.into()), old: a.to_json(), new: b.to_json(), looser,
                text: format!("{} drift limit {} → {}{}", d.label, a.describe(d), b.describe(d), if looser { " (looser)" } else { " (tighter)" }),
            });
        }
    }
    if changed(ol.band_fraction, nl.band_fraction) {
        let looser = nl.band_fraction > ol.band_fraction;
        rows.push(DiffRow {
            section: "band_widen", moment: None, stat: None, old: json!(ol.band_fraction), new: json!(nl.band_fraction), looser,
            text: format!("band widening {:.0}% → {:.0}% of the baseline range", ol.band_fraction * 100.0, nl.band_fraction * 100.0),
        });
    }
    for d in &STATS {
        let (a, b) = (ol.band_floors.get(d.id).copied(), nl.band_floors.get(d.id).copied());
        if let (Some(a), Some(b)) = (a, b)
            && changed(a, b)
        {
            rows.push(DiffRow {
                section: "band_widen", moment: None, stat: Some(d.id.into()), old: json!(a), new: json!(b), looser: b > a,
                text: format!("{} band floor {} → {}", d.label, fmt(d, a), fmt(d, b)),
            });
        }
    }
    // Band.
    for d in &STATS {
        let (a, b) = (ol.band.get(d.id).copied(), nl.band.get(d.id).copied());
        match (a, b) {
            (Some((alo, ahi)), Some((blo, bhi))) if changed(alo, blo) || changed(ahi, bhi) => {
                let up_only = nl.tolerance(d.id).mode == Mode::Up;
                let looser = bhi > ahi + 1e-12 || (!up_only && blo < alo - 1e-12);
                rows.push(DiffRow {
                    section: "band", moment: None, stat: Some(d.id.into()), old: json!([alo, ahi]), new: json!([blo, bhi]), looser,
                    text: format!(
                        "{} band {}–{} → {}–{}{}",
                        d.label, fmt(d, alo), fmt(d, ahi), fmt(d, blo), fmt(d, bhi),
                        if looser { " (looser)" } else { " (tighter)" }
                    ),
                });
            }
            (None, Some((blo, bhi))) => rows.push(DiffRow {
                section: "band", moment: None, stat: Some(d.id.into()), old: Value::Null, new: json!([blo, bhi]), looser: false,
                text: format!("{} band {}–{} (new)", d.label, fmt(d, blo), fmt(d, bhi)),
            }),
            (Some((alo, ahi)), None) => rows.push(DiffRow {
                section: "band", moment: None, stat: Some(d.id.into()), old: json!([alo, ahi]), new: Value::Null, looser: true,
                text: format!("{} band removed (no longer checked)", d.label),
            }),
            _ => {}
        }
    }
    // Baseline values: re-baselining moves what counts as normal, so every
    // moved value is shown as looser.
    for (moment, ns) in &nl.baseline {
        let os = ol.baseline.get(moment);
        for d in &STATS {
            let (a, b) = (os.and_then(|s| s.get(d.id)), ns.get(d.id));
            let moved = match (a, b) {
                (Some(a), Some(b)) => changed(a, b),
                (None, None) => false,
                _ => true,
            };
            if moved {
                rows.push(DiffRow {
                    section: "baseline", moment: Some(moment.clone()), stat: Some(d.id.into()), old: json!(a), new: json!(b), looser: true,
                    text: format!(
                        "{} baseline on '{moment}' {} → {}",
                        d.label,
                        a.map_or("none".into(), |a| fmt(d, a)),
                        b.map_or("none".into(), |b| fmt(d, b))
                    ),
                });
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::compute;
    use crate::stats::tests::{bands, solid};

    fn moments() -> Moments {
        Moments::parse(
            r#"{"format":"evani-look-moments/1","game":"sandbox","tier":"medium","width":480,
                "moments":[{"name":"a","capture":{"camera":"path","sim_time":2}},{"name":"b","capture":{"camera":"path","sim_time":8}}]}"#,
        )
        .unwrap()
    }

    /// A scene-like picture: sky, mid, shadow, a few bright pixels.
    fn scene(shift: i16) -> crate::png::Rgb {
        let c = |r: i16, g: i16, b: i16| [(r + shift).clamp(0, 255) as u8, (g + shift).clamp(0, 255) as u8, (b + shift).clamp(0, 255) as u8];
        bands(20, 40, &[c(120, 160, 220), c(110, 140, 90), c(90, 110, 70), c(40, 45, 50), c(150, 120, 90), c(60, 80, 40), c(200, 190, 170), c(30, 30, 35)])
    }

    fn baseline() -> BTreeMap<String, Stats> {
        let mut m = BTreeMap::new();
        m.insert("a".into(), compute(&scene(0), Some(&scene(0))).unwrap());
        m.insert("b".into(), compute(&scene(10), Some(&scene(10))).unwrap());
        m
    }

    #[test]
    fn generated_from_a_baseline_set() {
        let l = Limits::generate("evani-look-basic", baseline(), None);
        assert_eq!(l.baseline.len(), 2);
        assert_eq!(l.tolerances.len(), STATS.len());
        let (lo, hi) = l.band["mean_luma"];
        let (a, b) = (l.baseline["a"].mean_luma, l.baseline["b"].mean_luma);
        let w = (0.1 * (b - a)).max(0.02);
        assert!((lo - (a - w)).abs() < 1e-5 && (hi - (b + w)).abs() < 1e-5, "{lo} {hi} {a} {b}");
        // File round trip.
        let back = Limits::parse(&l.file_text()).unwrap();
        assert_eq!(back, l);
        // Stat version must match.
        let mut v = l.to_json();
        v["stat_version"] = json!(99);
        assert!(Limits::parse(&v.to_string()).unwrap_err().contains("stat version"));
    }

    #[test]
    fn clean_capture_passes() {
        let l = Limits::generate("evani-look-basic", baseline(), None);
        let mut cur = baseline();
        cur.get_mut("a").unwrap().mean_luma += 0.01;
        assert!(check(&l, &cur).is_empty(), "{:?}", check(&l, &cur));
    }

    #[test]
    fn brightened_desaturated_and_clipped_are_flagged() {
        let l = Limits::generate("evani-look-basic", baseline(), None);
        // Brightened by 40 levels.
        let mut cur = baseline();
        cur.insert("a".into(), compute(&scene(40), Some(&scene(40))).unwrap());
        let b = check(&l, &cur);
        let s = b.iter().find(|x| x.stat == "mean_luma" && x.layer == "drift").unwrap();
        assert!(s.sentence.starts_with("average brightness 0.") && s.sentence.contains("vs limit") && s.sentence.contains("on moment 'a' (baseline 0."), "{}", s.sentence);
        // Desaturated: grey version.
        let grey = {
            let mut g = scene(0);
            for p in g.px.as_chunks_mut::<3>().0 {
                let y = ((u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 3) as u8;
                *p = [y, y, y];
            }
            g
        };
        let mut cur = baseline();
        cur.insert("a".into(), compute(&grey, Some(&grey)).unwrap());
        let b = check(&l, &cur);
        assert!(b.iter().any(|x| x.stat == "saturation" && x.sentence.starts_with("saturation 0.000 vs limit")), "{b:?}");
        assert!(b.iter().any(|x| x.stat == "colorfulness"));
        // Clipped: a quarter of the frame blown out.
        let mut clip = scene(0);
        for p in clip.px.as_chunks_mut::<3>().0.iter_mut().take(200) {
            *p = [255, 255, 255];
        }
        let mut cur = baseline();
        cur.insert("a".into(), compute(&clip, Some(&scene(0))).unwrap());
        let b = check(&l, &cur);
        let c = b.iter().find(|x| x.stat == "clipped" && x.layer == "drift").unwrap();
        assert_eq!(c.sentence, "highlights clipped 25.0% vs limit 1.0% on moment 'a' (baseline 0.0%)");
        // Bloom spreading over the frame.
        let mut cur = baseline();
        cur.insert("a".into(), compute(&scene(10), Some(&scene(0))).unwrap());
        assert!(check(&l, &cur).iter().any(|x| x.stat == "bloom" && x.sentence.starts_with("bloom coverage 100.0% vs limit")));
        // A missing moment is flagged, never skipped.
        let mut cur = baseline();
        cur.remove("b");
        assert!(check(&l, &cur).iter().any(|x| x.layer == "missing" && x.moment == "b"));
    }

    #[test]
    fn band_catches_what_drift_allows() {
        let mut base = BTreeMap::new();
        base.insert("a".into(), compute(&solid(4, 4, [100, 100, 100]), None).unwrap());
        let mut l = Limits::generate("evani-look-basic", base, None);
        // Very loose drift, but the band still holds.
        l.tolerances.insert("mean_luma".into(), Tolerance { mode: Mode::Abs, value: 1.0, min: 0.0 });
        let mut cur = BTreeMap::new();
        cur.insert("a".into(), compute(&solid(4, 4, [140, 140, 140]), None).unwrap());
        let b = check(&l, &cur);
        assert!(b.iter().any(|x| x.stat == "mean_luma" && x.layer == "band" && x.sentence.contains("vs the look's band")), "{b:?}");
        assert!(!b.iter().any(|x| x.stat == "mean_luma" && x.layer == "drift"));
    }

    #[test]
    fn bake_hash_binds_everything() {
        let br = |m: &str, s: &str| Breach { moment: m.into(), stat: s.into(), layer: "drift", value: 0.0, low: None, high: None, baseline: None, sentence: String::new() };
        let h = bake_hash("l", "g", "c", "b", &[br("a", "clipped"), br("a", "bloom")]);
        assert_eq!(h, bake_hash("l", "g", "c", "b", &[br("a", "bloom"), br("a", "clipped")]));
        assert_ne!(h, bake_hash("l", "g", "c", "b", &[br("a", "bloom")]));
        assert_ne!(h, bake_hash("l", "g", "c2", "b", &[br("a", "bloom"), br("a", "clipped")]));
        assert_ne!(h, bake_hash("l", "g", "c", "b2", &[br("a", "bloom"), br("a", "clipped")]));
        assert_ne!(h, bake_hash("l", "g2", "c", "b", &[br("a", "bloom"), br("a", "clipped")]));
    }

    #[test]
    fn diff_marks_looser_and_tighter() {
        let m = moments();
        let old = Limits::generate("evani-look-basic", baseline(), None);
        let mut new = old.clone();
        new.tolerances.insert("clipped".into(), Tolerance { mode: Mode::Up, value: 0.05, min: 0.0 });
        new.tolerances.insert("mean_luma".into(), Tolerance { mode: Mode::Abs, value: 0.02, min: 0.0 });
        let rows = diff(Some((&m, &old)), (&m, &new));
        let c = rows.iter().find(|r| r.stat.as_deref() == Some("clipped")).unwrap();
        assert!(c.looser && c.text == "highlights clipped drift limit up to +1.0% → up to +5.0% (looser)", "{}", c.text);
        let t = rows.iter().find(|r| r.stat.as_deref() == Some("mean_luma")).unwrap();
        assert!(!t.looser && t.text.ends_with("(tighter)"));
        // Re-baselining counts as loosening.
        let mut b2 = baseline();
        b2.get_mut("a").unwrap().mean_luma += 0.1;
        let rebased = Limits::generate("evani-look-basic", b2, Some(&old));
        let rows = diff(Some((&m, &old)), (&m, &rebased));
        assert!(rows.iter().any(|r| r.section == "baseline" && r.looser && r.moment.as_deref() == Some("a")));
        // A removed moment is looser.
        let mut m2 = m.clone();
        m2.moments.pop();
        assert!(diff(Some((&m, &old)), (&m2, &old)).iter().any(|r| r.section == "moments" && r.looser));
        // No change, no rows.
        assert!(diff(Some((&m, &old)), (&m, &old)).is_empty());
        // First limits: listed as new.
        assert!(diff(None, (&m, &old)).iter().all(|r| r.old.is_null()));
    }

    #[test]
    fn settings_patch_recomputes_the_band_and_shows_in_the_diff() {
        let m = moments();
        let old = Limits::generate("evani-look-basic", baseline(), None);
        let new = old
            .with_settings(&json!({ "tolerances": { "clipped": { "mode": "up", "value": 0.03 } }, "band_widen": { "fraction": 0.5 } }))
            .unwrap();
        assert_eq!(new.tolerance("clipped").value, 0.03);
        assert_eq!(new.baseline, old.baseline, "the baseline is untouched");
        let (lo0, hi0) = old.band["mean_luma"];
        let (lo1, hi1) = new.band["mean_luma"];
        assert!(lo1 <= lo0 && hi1 >= hi0, "a wider band");
        let rows = diff(Some((&m, &old)), (&m, &new));
        assert!(rows.iter().any(|r| r.section == "tolerance" && r.looser));
        assert!(rows.iter().any(|r| r.section == "band_widen" && r.looser));
        assert!(old.with_settings(&json!({ "tolerances": { "nope": { "mode": "abs", "value": 1 } } })).is_err());
        assert!(old.with_settings(&json!({ "baseline": {} })).is_err(), "the baseline can't be patched");
        assert!(old.with_settings(&json!({ "band_widen": { "fraction": -1 } })).is_err());
    }

    #[test]
    fn moments_shape() {
        assert!(Moments::parse(r#"{"format":"evani-look-moments/1","game":"g","tier":"t","width":480,"moments":[{"name":"x","capture":{"values":{}}}]}"#).is_err());
        assert!(Moments::parse(r#"{"format":"evani-look-moments/1","game":"g","tier":"t","width":480,"moments":[{"name":"X"}]}"#).is_err());
        let m = moments();
        assert_eq!(Moments::parse(&m.file_text()).unwrap(), m);
    }
}
