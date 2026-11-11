//! Recipes as data (docs/MODELGEN_PLAN.md 3.4): a `*.construct.json` file is
//! read once ([`Recipe::parse`]) and built per render tier ([`Recipe::build`]).
//!
//! ```json
//! { "construct": 1, "id": "sea-chest", "seed": 7,
//!   "params": { "width": { "value": 0.9, "min": 0.5, "max": 1.5, "unit": "m", "live": true } },
//!   "let": { "hw": "width / 2" },
//!   "tiers": { "low": { "budget": 400 }, "high": { "budget": 4000 } },
//!   "materials": { "wood.oak_weathered": { "colour": "#8a6a48" } },
//!   "curves": { "rim": { "kind": "polyline", "points": [["-hw", 0, 0], ["hw", 0, 0]] } },
//!   "surfaces": { "front": { "kind": "ruled", "a": "rim", "b": "top" } },
//!   "protos": { "stud": { "parts": [ { "op": "lathe", "profile": [[0, 0], [0.01, 0], [0, 0.006]], "material": "metal.iron_cast" } ] } },
//!   "parts": [ { "name": "front", "op": "boards", "on": "front", "count": 4, "material": "wood.oak_weathered" } ],
//!   "anchors": { "lid_hinge": { "at": [0, 0.5, "-hw"] } },
//!   "stages": { "order": ["intact", "damaged"], "rules": { "damaged": { "knock_out": { "parts": ["front"], "share": 0.25 } } } },
//!   "split": { "groups": ["body"], "normal": [1, 0, 0], "d": 0, "names": ["a", "b"] } }
//! ```
//!
//! **Numbers** anywhere may be a JSON number, a formula string over the
//! parameters and `let` values (the [`crate::formula`] language), or a
//! per-tier object (`{ "low": 6, "high": 12 }`: a tier without its own entry
//! takes the nearest lower tier that has one, else the lowest given).
//!
//! **Per-tier detail.** A part may list the `tiers` it appears in (dropped
//! elsewhere, with the reason recorded) and give `detail`: an object of
//! overrides, each for its own tier only, merged over the part's own keys (`"detail": { "low":
//! { "count": 1, "butts": null } }` lays a band as one strip at Low).
//!
//! **Operations** (`op`): `boards` (strips of a band on a surface, cut at
//! staggered butts, each a board with thickness, gap and proud offset),
//! `sweep` (a profile along a curve, in jointed lengths: timbers, bands,
//! ropes), `lathe` (a solid of revolution), `block` (a box) and `along`
//! (instances of a prototype along a curve, with anchors). Any part can be
//! mirrored across x (`"mirror": true`). Close-up detail: a board's
//! `piece.bevel` (m) chamfers its long face edges; a sweep's profile may be
//! `{ "rope": [radius, strands, points per strand] }` and its `twist`
//! (degrees per metre) turns the profile along the path (laid rope); an
//! `along` with `"bake": true` merges its copies into its node's mesh
//! (nails and rivets: no draw call each). The Rust escape hatch for new
//! operations (`Node`, plan 3.4) comes with the first family that needs it.
//!
//! **Seeds** are per piece ([`crate::seed`]): every piece has a stable path
//! id (`front/s2/p1`), and its tone, variation value and stage choices draw
//! from that id's own stream.

use crate::curve::{Curve, Path};
use crate::formula::Formula;
use crate::mesh::{self, Mesh};
use crate::model::{Anchor, Instance, Model, Piece, Proto};
use crate::seed::Vary;
use crate::surface::{self, Band, Board, Fx, Surface};
use crate::vec::*;
use serde_json::{Map, Value};

/// The render tiers a recipe may name, lowest first.
pub const TIERS: [&str; 5] = ["low", "medium", "high", "ultra", "vr"];

/// A declared parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: String,
    pub live: bool,
}

/// One tier the recipe builds, with its triangle budget (0: none given).
#[derive(Clone, Debug, PartialEq)]
pub struct TierSpec {
    pub name: String,
    pub budget: usize,
}

/// A parsed recipe: the JSON, its parameters and a content hash.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub seed: u64,
    pub params: Vec<Param>,
    pub tiers: Vec<TierSpec>,
    /// FNV-1a 64 of the canonical JSON (sorted keys, no spaces), as 16 hex
    /// digits: whitespace and key order never make an output stale.
    pub hash: String,
    json: Value,
}

/// Canonical text of a JSON value: object keys sorted, no spaces, numbers
/// and strings as serde_json writes them. Independent of how the map type
/// orders keys, so every tool that hashes a recipe agrees.
pub fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                if let Some(x) = o.get(*k) {
                    canonical(x, out);
                }
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Content hash of a recipe (the stale-output check uses it): FNV-1a 64 of
/// its [`canonical`] text, as 16 hex digits.
pub fn content_hash(v: &Value) -> String {
    let mut canon = String::new();
    canonical(v, &mut canon);
    format!("{:016x}", crate::seed::path_hash(&canon))
}

fn tier_rank(t: &str) -> usize {
    TIERS.iter().position(|x| *x == t).unwrap_or(0)
}

/// The value of a per-tier object for `tier`, or `v` itself.
fn per_tier<'a>(v: &'a Value, tier: &str) -> &'a Value {
    let Value::Object(o) = v else { return v };
    if o.is_empty() || !o.keys().all(|k| TIERS.contains(&k.as_str())) {
        return v;
    }
    let want = tier_rank(tier);
    let mut best: Option<(usize, &Value)> = None;
    let mut lowest: Option<(usize, &Value)> = None;
    for (k, x) in o {
        let r = tier_rank(k);
        if r <= want && best.is_none_or(|b| r > b.0) {
            best = Some((r, x));
        }
        if lowest.is_none_or(|l| r < l.0) {
            lowest = Some((r, x));
        }
    }
    best.or(lowest).map_or(v, |b| b.1)
}

/// Shallow merge of `over` onto `base` (a `null` removes the key).
fn merged(base: &Map<String, Value>, over: &Value) -> Map<String, Value> {
    let mut m = base.clone();
    if let Value::Object(o) = over {
        for (k, v) in o {
            if v.is_null() {
                m.remove(k);
            } else {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    m
}

/// Hex `#rrggbb` (sRGB) to linear bytes.
fn hex_linear(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        let l = if c <= 0.04045 { c / 12.92 } else { crate::dmath::pow((c + 0.055) / 1.055, 2.4) };
        (l * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Some([lin(b(0)?), lin(b(2)?), lin(b(4)?)])
}

/// Everything a tier's build needs while it walks the recipe.
struct Cx<'a> {
    tier: &'a str,
    names: Vec<String>,
    vals: Vec<f64>,
    vary: Vary,
    curves: Vec<(String, Curve)>,
    surfaces: Vec<(String, Surface)>,
    materials: &'a Map<String, Value>,
}

impl Cx<'_> {
    fn formula(&self, src: &str, lead: &[&str]) -> Result<Fx, String> {
        let mut names: Vec<String> = lead.iter().map(|s| s.to_string()).collect();
        names.extend(self.names.iter().cloned());
        Ok(Fx { f: Formula::parse(src, &names)?, env: self.vals.clone() })
    }
    /// A number: JSON number, formula string or per-tier object.
    fn num(&self, v: &Value, what: &str) -> Result<f64, String> {
        match per_tier(v, self.tier) {
            Value::Number(n) => n.as_f64().ok_or_else(|| format!("{what}: not a number")),
            Value::String(s) => {
                let f = Formula::parse(s, &self.names).map_err(|e| format!("{what}: {e}"))?;
                Ok(f.eval(&self.vals))
            }
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            other => Err(format!("{what}: expected a number or formula, got {other}")),
        }
    }
    fn num_or(&self, o: &Map<String, Value>, key: &str, default: f64, what: &str) -> Result<f64, String> {
        match o.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(v) => self.num(v, &format!("{what}.{key}")),
        }
    }
    fn count(&self, o: &Map<String, Value>, key: &str, default: usize, what: &str) -> Result<usize, String> {
        let x = self.num_or(o, key, default as f64, what)?;
        if !(0.0..=1e6).contains(&x) {
            return Err(format!("{what}.{key}: {x} is not a count"));
        }
        Ok(x.round() as usize)
    }
    fn v3(&self, v: &Value, what: &str) -> Result<V3, String> {
        match per_tier(v, self.tier) {
            Value::Array(a) if a.len() == 3 => Ok([self.num(&a[0], what)?, self.num(&a[1], what)?, self.num(&a[2], what)?]),
            other => Err(format!("{what}: expected [x, y, z], got {other}")),
        }
    }
    fn pts3(&self, v: &Value, what: &str) -> Result<Vec<V3>, String> {
        let a = v.as_array().ok_or_else(|| format!("{what}: expected a list of points"))?;
        a.iter().enumerate().map(|(i, p)| self.v3(p, &format!("{what}[{i}]"))).collect()
    }
    fn pts2(&self, v: &Value, what: &str) -> Result<Vec<[f64; 2]>, String> {
        let a = per_tier(v, self.tier).as_array().ok_or_else(|| format!("{what}: expected a list of [x, y]"))?;
        a.iter()
            .enumerate()
            .map(|(i, p)| match p.as_array() {
                Some(q) if q.len() == 2 => Ok([self.num(&q[0], what)?, self.num(&q[1], what)?]),
                _ => Err(format!("{what}[{i}]: expected [x, y]")),
            })
            .collect()
    }
    fn curve(&self, name: &str, what: &str) -> Result<&Curve, String> {
        let name = name.strip_prefix("curve:").unwrap_or(name);
        self.curves.iter().find(|c| c.0 == name).map(|c| &c.1).ok_or_else(|| format!("{what}: no curve `{name}`"))
    }
    fn surface(&self, name: &str, what: &str) -> Result<&Surface, String> {
        self.surfaces.iter().find(|c| c.0 == name).map(|c| &c.1).ok_or_else(|| format!("{what}: no surface `{name}`"))
    }
    /// A piece's colour: the material's base colour with seeded tone.
    fn colour(&self, material: &str, id: &str, tone: f64) -> [u8; 4] {
        let base = self.materials.get(material).and_then(|m| m.get("colour")).and_then(Value::as_str).and_then(hex_linear).unwrap_or([180, 180, 180]);
        let k = 1.0 + self.vary.signed(id, "tone", tone);
        let c = |x: u8| (f64::from(x) * k).round().clamp(0.0, 255.0) as u8;
        [c(base[0]), c(base[1]), c(base[2]), 255]
    }
}

fn str_of<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

fn build_curve(cx: &Cx, c: &Map<String, Value>, what: &str) -> Result<Curve, String> {
    let kind = str_of(c, "kind").unwrap_or("polyline");
    let deg = |key: &str, d: f64| -> Result<f64, String> { Ok(crate::dmath::deg(cx.num_or(c, key, d, what)?)) };
    let pts = || cx.pts3(c.get("points").unwrap_or(&Value::Null), &format!("{what}.points"));
    Ok(match kind {
        "polyline" => Curve::Polyline(pts()?),
        "catmull_rom" => Curve::CatmullRom(pts()?),
        "bezier" => Curve::Bezier(pts()?),
        "arc" => {
            let u = c.get("u").map(|v| cx.v3(v, what)).transpose()?.unwrap_or([1.0, 0.0, 0.0]);
            let v = c.get("v").map(|v| cx.v3(v, what)).transpose()?.unwrap_or([0.0, 0.0, -1.0]);
            Curve::Arc {
                centre: c.get("centre").map(|v| cx.v3(v, what)).transpose()?.unwrap_or([0.0; 3]),
                radius: cx.num_or(c, "radius", 1.0, what)?,
                a0: deg("a0", 0.0)?,
                a1: deg("a1", 180.0)?,
                u: norm_or(u, [1.0, 0.0, 0.0]),
                v: norm_or(v, [0.0, 0.0, -1.0]),
            }
        }
        "helix" => Curve::Helix {
            centre: c.get("centre").map(|v| cx.v3(v, what)).transpose()?.unwrap_or([0.0; 3]),
            radius: cx.num_or(c, "radius", 1.0, what)?,
            pitch: cx.num_or(c, "pitch", 1.0, what)?,
            turns: cx.num_or(c, "turns", 1.0, what)?,
            a0: deg("a0", 0.0)?,
        },
        "catenary" => Curve::Catenary {
            a: cx.v3(c.get("a").unwrap_or(&Value::Null), &format!("{what}.a"))?,
            b: cx.v3(c.get("b").unwrap_or(&Value::Null), &format!("{what}.b"))?,
            sag: cx.num_or(c, "sag", 0.0, what)?,
        },
        "formula" => {
            // x, y, z of t in [0, 1], sampled densely into a polyline.
            let n = cx.count(c, "samples", 64, what)?.max(1);
            let f = |key: &str| -> Result<Fx, String> {
                let src = match c.get(key) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Number(x)) => x.to_string(),
                    _ => "0".to_string(),
                };
                cx.formula(&src, &["t", "_"]).map_err(|e| format!("{what}.{key}: {e}"))
            };
            let (fx, fy, fz) = (f("x")?, f("y")?, f("z")?);
            Curve::Polyline((0..=n).map(|i| {
                let t = i as f64 / n as f64;
                [fx.eval(t, 0.0), fy.eval(t, 0.0), fz.eval(t, 0.0)]
            }).collect())
        }
        other => return Err(format!("{what}: unknown curve kind `{other}` (polyline, catmull_rom, bezier, arc, helix, catenary, formula)")),
    })
}

fn build_surface(cx: &Cx, s: &Map<String, Value>, what: &str) -> Result<Surface, String> {
    let kind = str_of(s, "kind").ok_or_else(|| format!("{what}: no kind"))?;
    let fx = |key: &str, lead: &[&str], default: &str| -> Result<Fx, String> {
        let src = match s.get(key) {
            Some(Value::String(x)) => x.clone(),
            Some(Value::Number(x)) => x.to_string(),
            _ => default.to_string(),
        };
        cx.formula(&src, lead).map_err(|e| format!("{what}.{key}: {e}"))
    };
    let path = |key: &str| -> Result<Path, String> {
        let name = str_of(s, key).ok_or_else(|| format!("{what}.{key}: name a curve"))?;
        Ok(Path::new(cx.curve(name, &format!("{what}.{key}"))?))
    };
    Ok(match kind {
        "lines" => Surface::Lines {
            length: cx.num_or(s, "length", 1.0, what)?,
            half_breadth: fx("half_breadth", &["t", "y"], "0")?,
            bottom: fx("bottom", &["t", "_"], "0")?,
            top: fx("top", &["t", "_"], "1")?,
            side: if cx.num_or(s, "side", 1.0, what)? < 0.0 { -1.0 } else { 1.0 },
        },
        "loft" => {
            let n = cx.count(s, "samples", 16, what)?.max(1);
            let secs = s.get("sections").and_then(Value::as_array).ok_or_else(|| format!("{what}.sections: a list of curve names"))?;
            let sections = secs
                .iter()
                .map(|x| {
                    let name = x.as_str().ok_or_else(|| format!("{what}.sections: curve names"))?;
                    Ok(Path::new(cx.curve(name, what)?).resample(n))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Surface::Loft { sections }
        }
        "ruled" => Surface::Ruled { a: path("a")?, b: path("b")? },
        "sweep" => {
            let n = cx.count(s, "segments", 16, what)?;
            let up = s.get("up").map(|v| cx.v3(v, what)).transpose()?;
            Surface::Sweep { frames: path("along")?.frames(n, up), profile: cx.pts2(s.get("profile").unwrap_or(&Value::Null), &format!("{what}.profile"))? }
        }
        "lathe" => Surface::Lathe {
            profile: cx.pts2(s.get("profile").unwrap_or(&Value::Null), &format!("{what}.profile"))?,
            a0: crate::dmath::deg(cx.num_or(s, "a0", 0.0, what)?),
            a1: crate::dmath::deg(cx.num_or(s, "a1", 360.0, what)?),
            centre: s.get("centre").map(|v| cx.v3(v, what)).transpose()?.unwrap_or([0.0; 3]),
        },
        "formula" => Surface::Formula { x: fx("x", &["u", "v"], "0")?, y: fx("y", &["u", "v"], "0")?, z: fx("z", &["u", "v"], "0")? },
        other => return Err(format!("{what}: unknown surface kind `{other}` (lines, loft, ruled, sweep, lathe, formula)")),
    })
}

/// A profile: `{ "rect": [w, h] }`, `{ "circle": [r, sides] }` or points.
fn profile(cx: &Cx, v: &Value, what: &str) -> Result<(Vec<[f64; 2]>, bool), String> {
    let v = per_tier(v, cx.tier);
    if let Some(r) = v.get("rect").and_then(Value::as_array) {
        if r.len() != 2 {
            return Err(format!("{what}.rect: [width, height]"));
        }
        return Ok((mesh::rect(cx.num(&r[0], what)?, cx.num(&r[1], what)?, true), false));
    }
    if let Some(c) = v.get("circle").and_then(Value::as_array) {
        let r = cx.num(c.first().unwrap_or(&Value::Null), what)?;
        let n = c.get(1).map(|x| cx.num(x, what)).transpose()?.unwrap_or(8.0).round().max(3.0) as usize;
        return Ok((mesh::circle(r, n), true));
    }
    // `rope: [radius, strands, points per strand]`: laid strands, round
    // crowns with creases between them (sweep it with `twist`).
    if let Some(c) = v.get("rope").and_then(Value::as_array) {
        let r = cx.num(c.first().unwrap_or(&Value::Null), what)?;
        let strands = c.get(1).map(|x| cx.num(x, what)).transpose()?.unwrap_or(3.0).round().max(2.0) as usize;
        let per = c.get(2).map(|x| cx.num(x, what)).transpose()?.unwrap_or(4.0).round().max(2.0) as usize;
        return Ok((mesh::rope(r, strands, per), true));
    }
    Ok((cx.pts2(v, what)?, false))
}

/// A part's raw pieces (before material, colour and mirror).
struct Raw {
    id: String,
    mesh: Mesh,
}

/// Cuts at staggered butts: `u` ranges of one strip, by arc length along
/// its middle line.
fn butt_cuts(s: &Surface, strip: &Band, length: f64, offset: f64) -> Vec<(f64, f64)> {
    const N: usize = 64;
    let mid = |u: f64| {
        let (a, b) = strip.v_range(u);
        s.point(u, 0.5 * (a + b))
    };
    let us: Vec<f64> = (0..=N).map(|i| strip.u0 + (strip.u1 - strip.u0) * i as f64 / N as f64).collect();
    let mut cum = vec![0.0];
    for i in 1..=N {
        let l = cum[i - 1] + dist(mid(us[i - 1]), mid(us[i]));
        cum.push(l);
    }
    let total = cum[N];
    if length <= 1e-9 || total <= length * 1.0001 {
        return vec![(strip.u0, strip.u1)];
    }
    let u_at = |sl: f64| {
        let i = cum.partition_point(|c| *c < sl).clamp(1, N);
        let f = if cum[i] - cum[i - 1] > 1e-300 { (sl - cum[i - 1]) / (cum[i] - cum[i - 1]) } else { 0.0 };
        us[i - 1] + (us[i] - us[i - 1]) * f.clamp(0.0, 1.0)
    };
    // Cut points at offset + n * length; a sliver shorter than a quarter
    // length at either end joins its neighbour.
    let mut cuts = vec![0.0];
    let mut x = offset.rem_euclid(length);
    if x < 1e-9 {
        x += length;
    }
    while x < total - 1e-9 {
        cuts.push(x);
        x += length;
    }
    cuts.push(total);
    let min = 0.25 * length;
    if cuts.len() > 2 && cuts[1] - cuts[0] < min {
        cuts.remove(1);
    }
    let k = cuts.len();
    if k > 2 && cuts[k - 1] - cuts[k - 2] < min {
        cuts.remove(k - 2);
    }
    cuts.windows(2).map(|w| (u_at(w[0]), u_at(w[1]))).collect()
}

fn op_boards(cx: &Cx, p: &Map<String, Value>, name: &str) -> Result<Vec<Raw>, String> {
    let on = str_of(p, "on").ok_or_else(|| format!("{name}: boards need `on` (a surface)"))?;
    let s = cx.surface(on, name)?;
    let line = |key: &str, d: f64| -> Result<Vec<[f64; 2]>, String> {
        match p.get(key) {
            Some(Value::String(src)) if Formula::parse(src, &cx.names).is_err() => {
                // A formula of u: sampled at 17 points across the band.
                let f = cx.formula(src, &["u", "_"]).map_err(|e| format!("{name}.{key}: {e}"))?;
                Ok((0..=16).map(|i| {
                    let u = i as f64 / 16.0;
                    [u, f.eval(u, 0.0)]
                }).collect())
            }
            Some(v) => Ok(vec![[0.0, cx.num(v, &format!("{name}.{key}"))?]]),
            None => Ok(vec![[0.0, d]]),
        }
    };
    let band = Band { u0: cx.num_or(p, "u0", 0.0, name)?, u1: cx.num_or(p, "u1", 1.0, name)?, a: line("v0", 0.0)?, b: line("v1", 1.0)? };
    let count = cx.count(p, "count", 1, name)?.max(1);
    let pc = p.get("piece").and_then(Value::as_object).cloned().unwrap_or_default();
    let what = format!("{name}.piece");
    let proud = cx.num_or(&pc, "proud", 0.0, &what)?;
    let proud_every = cx.count(&pc, "proud_every", 0, &what)?;
    let base = Board {
        thickness: cx.num_or(&pc, "thickness", 0.02, &what)?,
        gap: cx.num_or(&pc, "gap", 0.0, &what)?,
        end_gap: cx.num_or(&pc, "end_gap", 0.0, &what)?,
        rows: cx.count(p, "rows", 1, name)?.max(1),
        cols: cx.count(p, "cols", 1, name)?.max(1),
        proud: 0.0,
        bevel: cx.num_or(&pc, "bevel", 0.0, &what)?.max(0.0),
    };
    let butts = p.get("butts").and_then(Value::as_object);
    let (blen, stagger) = match butts {
        Some(b) => {
            let st: Vec<f64> = match b.get("stagger") {
                Some(Value::Array(a)) => a.iter().map(|x| cx.num(x, &format!("{name}.butts.stagger"))).collect::<Result<_, _>>()?,
                _ => vec![0.0],
            };
            (cx.num_or(b, "length", 0.0, name)?, st)
        }
        None => (0.0, vec![0.0]),
    };
    let mut out = Vec::new();
    for (k, strip) in band.strips(count).iter().enumerate() {
        let off = stagger.get(k % stagger.len().max(1)).copied().unwrap_or(0.0) * blen;
        // `proud_every: n` lifts every n-th strip (lapped strakes); 0: all.
        let mut b = base;
        if proud_every == 0 || k % proud_every == proud_every - 1 {
            b.proud = proud;
        }
        for (j, (u0, u1)) in butt_cuts(s, strip, blen, off).into_iter().enumerate() {
            let piece = Band { u0, u1, a: strip.a.clone(), b: strip.b.clone() };
            out.push(Raw { id: format!("{name}/s{k}/p{j}"), mesh: surface::board(s, &piece, &b) });
        }
    }
    Ok(out)
}

fn op_sweep(cx: &Cx, p: &Map<String, Value>, name: &str) -> Result<Vec<Raw>, String> {
    let along = str_of(p, "along").ok_or_else(|| format!("{name}: sweep needs `along` (a curve)"))?;
    let path = Path::new(cx.curve(along, name)?);
    let (prof, round) = profile(cx, p.get("profile").unwrap_or(&Value::Null), &format!("{name}.profile"))?;
    if prof.len() < 3 {
        return Err(format!("{name}.profile: at least 3 points"));
    }
    let smooth = p.get("smooth").and_then(Value::as_bool).unwrap_or(round);
    let up = p.get("up").map(|v| cx.v3(v, name)).transpose()?;
    let segs = cx.count(p, "segments", 8, name)?.max(1);
    let joints = cx.count(p, "joints", 1, name)?.max(1);
    let gap = cx.num_or(p, "joint_gap", 0.0, name)?;
    let taper = match p.get("taper").and_then(Value::as_array) {
        Some(t) if t.len() == 2 => (cx.num(&t[0], name)?, cx.num(&t[1], name)?),
        _ => (1.0, 1.0),
    };
    let twist = cx.num_or(p, "twist", 0.0, name)?;
    let total = path.length();
    let mut out = Vec::new();
    for j in 0..joints {
        let s0 = total * j as f64 / joints as f64 + if j > 0 { gap / 2.0 } else { 0.0 };
        let s1 = total * (j + 1) as f64 / joints as f64 - if j + 1 < joints { gap / 2.0 } else { 0.0 };
        if s1 - s0 <= 1e-9 {
            continue;
        }
        let n = (segs / joints).max(1);
        let pts: Vec<V3> = (0..=n * 8).map(|i| path.at(s0 + (s1 - s0) * i as f64 / (n * 8) as f64)).collect();
        let sub_path = Path::from_points(pts);
        let frames = sub_path.frames(n, up);
        let scale = |i: usize| {
            let f = (s0 + (s1 - s0) * i as f64 / n as f64) / total.max(1e-12);
            taper.0 + (taper.1 - taper.0) * f
        };
        // `twist`: the profile turns about the path, degrees per metre (laid rope).
        let rings = if twist == 0.0 {
            mesh::sweep_rings(&frames, &prof, &scale)
        } else {
            frames.iter().enumerate().map(|(i, f)| {
                let s = scale(i);
                let (sn, cs) = crate::dmath::sin_cos(crate::dmath::deg(twist * (s0 + (s1 - s0) * i as f64 / n as f64)));
                prof.iter().map(|p| f.local((p[0] * cs - p[1] * sn) * s, (p[0] * sn + p[1] * cs) * s, 0.0)).collect()
            }).collect()
        };
        out.push(Raw { id: if joints == 1 { name.to_string() } else { format!("{name}/j{j}") }, mesh: mesh::ring_solid(&rings, smooth) });
    }
    Ok(out)
}

fn op_lathe(cx: &Cx, p: &Map<String, Value>, name: &str) -> Result<Vec<Raw>, String> {
    let prof = cx.pts2(p.get("profile").unwrap_or(&Value::Null), &format!("{name}.profile"))?;
    let seg = cx.count(p, "segments", 12, name)?.max(3);
    let at = p.get("at").map(|v| cx.v3(v, name)).transpose()?.unwrap_or([0.0; 3]);
    let smooth = p.get("smooth").and_then(Value::as_bool).unwrap_or(true);
    let m = mesh::ring_solid(&mesh::lathe_rings(&prof, seg, [0.0; 3]), smooth);
    let q = rotation(cx, p, name)?;
    Ok(vec![Raw { id: name.to_string(), mesh: m.transformed(q, at, false) }])
}

fn rotation(cx: &Cx, p: &Map<String, Value>, name: &str) -> Result<Quat, String> {
    match p.get("rotate").and_then(Value::as_array) {
        Some(r) if r.len() == 3 => {
            let d = |i: usize| -> Result<f64, String> { Ok(crate::dmath::deg(cx.num(&r[i], &format!("{name}.rotate"))?)) };
            Ok(quat_ypr(d(0)?, d(1)?, d(2)?))
        }
        Some(_) => Err(format!("{name}.rotate: [yaw, pitch, roll] in degrees")),
        None => Ok(QID),
    }
}

fn op_block(cx: &Cx, p: &Map<String, Value>, name: &str) -> Result<Vec<Raw>, String> {
    let at = p.get("at").map(|v| cx.v3(v, name)).transpose()?.unwrap_or([0.0; 3]);
    let half = cx.v3(p.get("half").unwrap_or(&Value::Null), &format!("{name}.half"))?;
    Ok(vec![Raw { id: name.to_string(), mesh: mesh::block(at, rotation(cx, p, name)?, half) }])
}

/// The tiers a part appears in (`tiers`), and its per-tier overrides.
fn part_for_tier(p: &Map<String, Value>, tier: &str) -> Option<Map<String, Value>> {
    if let Some(Value::Array(ts)) = p.get("tiers")
        && !ts.iter().any(|t| t.as_str() == Some(tier))
    {
        return None;
    }
    // Overrides are for their own tier only (never inherited upwards).
    let over = p.get("detail").and_then(|d| d.get(tier)).cloned().unwrap_or(Value::Null);
    Some(merged(p, &over))
}

/// `name` matches `pat` (a trailing `*` matches any rest).
fn matches(pat: &str, name: &str) -> bool {
    match pat.strip_suffix('*') {
        Some(pre) => name.starts_with(pre),
        None => pat == name,
    }
}

impl Recipe {
    /// Read a recipe's JSON text.
    pub fn parse(text: &str) -> Result<Recipe, String> {
        let json: Value = serde_json::from_str(text).map_err(|e| format!("recipe JSON: {e}"))?;
        Recipe::from_json(json)
    }
    pub fn from_json(json: Value) -> Result<Recipe, String> {
        if json.get("construct").and_then(Value::as_u64) != Some(1) {
            return Err("recipe: `construct` must be 1 (the recipe format)".into());
        }
        let id = json.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("recipe: no `id`")?.to_string();
        if !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(format!("recipe: id `{id}` must be lowercase letters, digits, - and _"));
        }
        let seed = json.get("seed").and_then(Value::as_u64).unwrap_or(0);
        let mut params = Vec::new();
        if let Some(Value::Object(ps)) = json.get("params") {
            for (name, p) in ps {
                let (value, o) = match p {
                    Value::Number(n) => (n.as_f64().unwrap_or(0.0), Map::new()),
                    Value::Object(o) => (o.get("value").and_then(Value::as_f64).ok_or_else(|| format!("param {name}: no numeric `value`"))?, o.clone()),
                    _ => return Err(format!("param {name}: a number or {{ value, min, max, unit, live }}")),
                };
                let min = o.get("min").and_then(Value::as_f64);
                let max = o.get("max").and_then(Value::as_f64);
                if min.is_some_and(|m| value < m) || max.is_some_and(|m| value > m) {
                    return Err(format!("param {name}: value {value} is outside its range"));
                }
                params.push(Param { name: name.clone(), value, min, max, unit: o.get("unit").and_then(Value::as_str).unwrap_or("").to_string(), live: o.get("live").and_then(Value::as_bool).unwrap_or(false) });
            }
        }
        let mut tiers = Vec::new();
        match json.get("tiers") {
            Some(Value::Object(ts)) => {
                for t in TIERS {
                    if let Some(spec) = ts.get(t) {
                        tiers.push(TierSpec { name: t.to_string(), budget: spec.get("budget").and_then(Value::as_u64).unwrap_or(0) as usize });
                    }
                }
                if let Some(bad) = ts.keys().find(|k| !TIERS.contains(&k.as_str())) {
                    return Err(format!("recipe: unknown tier `{bad}` (low, medium, high, ultra, vr)"));
                }
            }
            _ => tiers.extend(["low", "medium", "high", "ultra"].map(|t| TierSpec { name: t.into(), budget: 0 })),
        }
        let hash = content_hash(&json);
        Ok(Recipe { id, seed, params, tiers, hash, json })
    }
    pub fn json(&self) -> &Value {
        &self.json
    }
    /// The parameter values with `overrides` (name, value) applied.
    pub fn values(&self, overrides: &[(String, f64)]) -> Result<Vec<(String, f64)>, String> {
        let mut v: Vec<(String, f64)> = self.params.iter().map(|p| (p.name.clone(), p.value)).collect();
        for (k, x) in overrides {
            let Some(p) = self.params.iter().find(|p| &p.name == k) else {
                return Err(format!("no parameter `{k}` in recipe {}", self.id));
            };
            if p.min.is_some_and(|m| *x < m) || p.max.is_some_and(|m| *x > m) {
                return Err(format!("parameter {k} = {x} is outside its range"));
            }
            if let Some(slot) = v.iter_mut().find(|s| &s.0 == k) {
                slot.1 = *x;
            }
        }
        Ok(v)
    }

    /// Build the model at `tier` with parameter `overrides`.
    pub fn build(&self, tier: &str, overrides: &[(String, f64)]) -> Result<Model, String> {
        let j = &self.json;
        let empty = Map::new();
        let obj = |k: &str| j.get(k).and_then(Value::as_object).unwrap_or(&empty);
        let values = self.values(overrides)?;
        let mut cx = Cx {
            tier,
            names: values.iter().map(|v| v.0.clone()).collect(),
            vals: values.iter().map(|v| v.1).collect(),
            vary: Vary::new(self.seed),
            curves: Vec::new(),
            surfaces: Vec::new(),
            materials: obj("materials"),
        };
        // `let`: derived values over the parameters and each other, in
        // whatever order resolves them (a cycle or unknown name is an error).
        let mut pending: Vec<(&String, &Value)> = obj("let").iter().collect();
        pending.sort_by(|a, b| a.0.cmp(b.0));
        while !pending.is_empty() {
            let before = pending.len();
            let mut still = Vec::new();
            for (name, v) in pending {
                match cx.num(v, &format!("let.{name}")) {
                    Ok(x) => {
                        cx.names.push(name.clone());
                        cx.vals.push(x);
                    }
                    Err(_) => still.push((name, v)),
                }
            }
            if still.len() == before {
                let (name, v) = still[0];
                return Err(cx.num(v, &format!("let.{name}")).err().unwrap_or_else(|| format!("let.{name}: a cycle")));
            }
            pending = still;
        }
        for (name, c) in obj("curves") {
            let c = c.as_object().ok_or_else(|| format!("curve {name}: an object"))?;
            let built = build_curve(&cx, c, &format!("curve {name}"))?;
            cx.curves.push((name.clone(), built));
        }
        for (name, s) in obj("surfaces") {
            let s = s.as_object().ok_or_else(|| format!("surface {name}: an object"))?;
            let built = build_surface(&cx, s, &format!("surface {name}"))?;
            cx.surfaces.push((name.clone(), built));
        }
        let mut model = Model { id: self.id.clone(), seed: self.seed, tier: tier.to_string(), params: values.clone(), recipe_hash: self.hash.clone(), ..Model::default() };
        if let Some(Value::Array(order)) = j.pointer("/stages/order") {
            model.stages = order.iter().filter_map(Value::as_str).map(str::to_string).collect();
        }
        // Prototypes: pieces in their own frame, written once.
        for (name, pr) in obj("protos") {
            let parts = pr.get("parts").and_then(Value::as_array).ok_or_else(|| format!("proto {name}: no parts"))?;
            let mut pieces = Vec::new();
            for (k, part) in parts.iter().enumerate() {
                let part = part.as_object().ok_or_else(|| format!("proto {name}: parts are objects"))?;
                let Some(part) = part_for_tier(part, tier) else { continue };
                let pid = format!("{name}#{k}");
                for raw in self.raw_pieces(&cx, &part, &pid)? {
                    pieces.push(self.finish(&cx, &part, raw, "", Vec::new()));
                }
            }
            if !pieces.is_empty() {
                model.protos.push(Proto { name: name.clone(), pieces });
            }
        }
        let rules = j.pointer("/stages/rules").and_then(Value::as_object).unwrap_or(&empty);
        let parts = j.get("parts").and_then(Value::as_array).ok_or("recipe: no `parts` list")?;
        for (k, part) in parts.iter().enumerate() {
            let part0 = part.as_object().ok_or_else(|| format!("part {k}: an object"))?;
            let name = str_of(part0, "name").ok_or_else(|| format!("part {k}: no name"))?.to_string();
            let Some(part) = part_for_tier(part0, tier) else {
                model.dropped.push((name.clone(), format!("not at {tier} (recipe `tiers`)")));
                continue;
            };
            let group = str_of(&part, "node").unwrap_or(&name).to_string();
            if str_of(&part, "op") == Some("along") {
                self.along(&cx, &part, &name, &group, &mut model)?;
                continue;
            }
            let explicit: Option<Vec<String>> = part.get("stages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
            for raw in self.raw_pieces(&cx, &part, &name)? {
                // Stage rules: a knocked-out piece leaves the stages that knock it out.
                let mut stages = explicit.clone().unwrap_or_default();
                if explicit.is_none() {
                    let mut out_of = Vec::new();
                    for (st, rule) in rules {
                        if let Some(ko) = rule.get("knock_out") {
                            let pats = ko.get("parts").and_then(Value::as_array).cloned().unwrap_or_default();
                            let share = ko.get("share").and_then(Value::as_f64).unwrap_or(0.0);
                            let hit = pats.iter().filter_map(Value::as_str).any(|p| matches(p, &name));
                            if hit && cx.vary.unit(&raw.id, &format!("stage:{st}")) < share {
                                out_of.push(st.clone());
                            }
                        }
                    }
                    if !out_of.is_empty() {
                        stages = model.stages.iter().filter(|s| !out_of.contains(s)).cloned().collect();
                    }
                }
                let mirror = part.get("mirror").and_then(Value::as_bool).unwrap_or(false);
                let mirrored = mirror.then(|| Raw { id: format!("{}.m", raw.id), mesh: raw.mesh.transformed(QID, [0.0; 3], true) });
                model.pieces.push(self.finish(&cx, &part, raw, &group, stages.clone()));
                if let Some(m) = mirrored {
                    model.pieces.push(self.finish(&cx, &part, m, &group, stages));
                }
            }
        }
        // A prototype nothing places at this tier (tier-only parts, baked
        // copies) is not written: its mesh would cost bytes and show nothing.
        let used: Vec<String> = model.instances.iter().map(|i| i.proto.clone()).collect();
        model.protos.retain(|p| used.contains(&p.name));
        for (name, a) in obj("anchors") {
            let a = a.as_object().ok_or_else(|| format!("anchor {name}: an object"))?;
            let q = match a.get("q").and_then(Value::as_array) {
                Some(q) if q.len() == 4 => {
                    let v = [cx.num(&q[0], name)?, cx.num(&q[1], name)?, cx.num(&q[2], name)?, cx.num(&q[3], name)?];
                    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3]).sqrt();
                    if l < 1e-9 {
                        return Err(format!("anchor {name}: q is zero"));
                    }
                    Some([v[0] / l, v[1] / l, v[2] / l, v[3] / l])
                }
                _ => None,
            };
            model.anchors.push(Anchor { name: name.clone(), p: cx.v3(a.get("at").unwrap_or(&Value::Null), &format!("anchor {name}.at"))?, q, group: str_of(a, "node").unwrap_or("").to_string() });
        }
        if let Some(sp) = j.get("split").and_then(Value::as_object) {
            let (groups, n, d, names) = split_spec(&cx, sp)?;
            let g: Vec<&str> = groups.iter().map(String::as_str).collect();
            model.split(&g, n, d, (&names.0, &names.1));
        }
        Ok(model)
    }

    fn raw_pieces(&self, cx: &Cx, part: &Map<String, Value>, name: &str) -> Result<Vec<Raw>, String> {
        match str_of(part, "op") {
            Some("boards") => op_boards(cx, part, name),
            Some("sweep") => op_sweep(cx, part, name),
            Some("lathe") => op_lathe(cx, part, name),
            Some("block") => op_block(cx, part, name),
            Some("along") => Err(format!("{name}: `along` places prototypes and cannot be inside one")),
            Some(other) => Err(format!("{name}: unknown op `{other}` (boards, sweep, lathe, block, along)")),
            None => Err(format!("{name}: no op")),
        }
    }

    fn finish(&self, cx: &Cx, part: &Map<String, Value>, raw: Raw, group: &str, stages: Vec<String>) -> Piece {
        let material = str_of(part, "material").unwrap_or("default").to_string();
        let tone = part.get("vary").and_then(|v| v.get("tone")).and_then(Value::as_f64).unwrap_or(0.0);
        Piece { colour: cx.colour(&material, &raw.id, tone), var: cx.vary.unit(&raw.id, "var"), id: raw.id, group: group.to_string(), material, stages, mesh: raw.mesh }
    }

    fn along(&self, cx: &Cx, part: &Map<String, Value>, name: &str, group: &str, model: &mut Model) -> Result<(), String> {
        let proto = str_of(part, "proto").ok_or_else(|| format!("{name}: along needs `proto`"))?;
        let curve = str_of(part, "curve").ok_or_else(|| format!("{name}: along needs `curve`"))?;
        if model.proto(proto).is_none() {
            // A prototype with nothing at this tier places nothing.
            model.dropped.push((name.to_string(), format!("prototype {proto} has no pieces at {}", cx.tier)));
            return Ok(());
        }
        let path = Path::new(cx.curve(curve, name)?);
        let len = path.length();
        let inset = cx.num_or(part, "inset", 0.0, name)?;
        let count = match part.get("spacing") {
            Some(v) => {
                let sp = cx.num(v, &format!("{name}.spacing"))?;
                if sp <= 1e-9 {
                    return Err(format!("{name}.spacing: must be above 0"));
                }
                (((len - 2.0 * inset) / sp).floor() as usize) + 1
            }
            None => cx.count(part, "count", 1, name)?,
        };
        let up = part.get("up").map(|v| cx.v3(v, name)).transpose()?;
        let jitter = cx.num_or(part, "jitter", 0.0, name)?;
        let mirror = part.get("mirror").and_then(Value::as_bool).unwrap_or(false);
        let anchor = str_of(part, "anchor");
        // `bake: true`: the copies become pieces of the part's node (merged
        // into its mesh, no draw call each): nails, rivets, small hardware.
        let bake = part.get("bake").and_then(Value::as_bool).unwrap_or(false);
        let proto_pieces: Vec<Piece> = model.proto(proto).map(|p| p.pieces.clone()).unwrap_or_default();
        for i in 0..count {
            let f = if count == 1 { 0.5 } else { i as f64 / (count - 1) as f64 };
            let iid = format!("{name}_{i}");
            let s = inset + (len - 2.0 * inset) * f + cx.vary.signed(&iid, "slide", jitter);
            // One frame at `s`: a short path around it gives the tangent.
            let h = (len * 1e-3).max(1e-6);
            let local = Path::from_points(vec![path.at(s - h), path.at(s + h)]);
            let fr = local.frames(1, up);
            let q = fr.first().map_or(QID, |x| x.quat());
            let t = path.at(s);
            for (side, mir) in [(0, false), (1, true)] {
                if side == 1 && !mirror {
                    continue;
                }
                let nm = if mir { format!("{iid}.m") } else { iid.clone() };
                let (tt, qq) = if mir { ([-t[0], t[1], t[2]], [q[0], -q[1], -q[2], q[3]]) } else { (t, q) };
                if bake {
                    for (k, pc) in proto_pieces.iter().enumerate() {
                        let mut w = pc.clone();
                        w.id = if proto_pieces.len() == 1 { nm.clone() } else { format!("{nm}#{k}") };
                        w.group = group.to_string();
                        w.mesh = pc.mesh.transformed(qq, tt, mir);
                        model.pieces.push(w);
                    }
                } else {
                    model.instances.push(Instance { name: nm.clone(), proto: proto.to_string(), group: group.to_string(), q: qq, t: tt, mirror: mir });
                }
                if let Some(pat) = anchor {
                    let an = pat.replace("{i}", &i.to_string());
                    model.anchors.push(Anchor { name: if mir { format!("{an}.m") } else { an }, p: tt, q: Some(qq), group: group.to_string() });
                }
            }
        }
        Ok(())
    }

    /// The recipe's split (for check 4): groups, plane normal and offset, half names.
    pub fn split(&self, tier: &str) -> Result<Option<(Vec<String>, V3, f64, (String, String))>, String> {
        let Some(sp) = self.json.get("split").and_then(Value::as_object) else { return Ok(None) };
        let values = self.values(&[])?;
        let cx = Cx { tier, names: values.iter().map(|v| v.0.clone()).collect(), vals: values.iter().map(|v| v.1).collect(), vary: Vary::new(self.seed), curves: Vec::new(), surfaces: Vec::new(), materials: &Map::new() };
        split_spec(&cx, sp).map(Some)
    }

    /// The model with the split undone (every piece back in its source
    /// group), for check 4.
    pub fn unsplit(model: &Model, groups: &[String], names: &(String, String)) -> Model {
        let mut m = model.clone();
        for p in &mut m.pieces {
            for g in groups {
                if p.group == format!("{g}_{}", names.0) || p.group == format!("{g}_{}", names.1) {
                    p.group = g.clone();
                }
            }
        }
        m
    }
}

fn split_spec(cx: &Cx, sp: &Map<String, Value>) -> Result<(Vec<String>, V3, f64, (String, String)), String> {
    let groups: Vec<String> = sp.get("groups").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let n = norm_or(cx.v3(sp.get("normal").unwrap_or(&Value::Null), "split.normal")?, [1.0, 0.0, 0.0]);
    let d = cx.num_or(sp, "d", 0.0, "split")?;
    let names = match sp.get("names").and_then(Value::as_array) {
        Some(a) if a.len() == 2 => (a[0].as_str().unwrap_or("a").to_string(), a[1].as_str().unwrap_or("b").to_string()),
        _ => ("a".to_string(), "b".to_string()),
    };
    Ok((groups, n, d, names))
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: &str = r##"{
      "construct": 1, "id": "t-crate", "seed": 3,
      "params": { "w": { "value": 1.0, "min": 0.5, "max": 2, "unit": "m", "live": true }, "h": 0.6 },
      "let": { "hw": "w / 2", "a_quarter": "hw / 2" },
      "tiers": { "low": { "budget": 400 }, "high": { "budget": 4000 } },
      "materials": { "wood": { "colour": "#8a6a48" }, "iron": { "colour": "#333333" } },
      "curves": {
        "bottom": { "kind": "polyline", "points": [["-hw", 0, 0], ["hw", 0, 0]] },
        "top": { "kind": "polyline", "points": [["-hw", "h", 0], ["hw", "h", 0]] },
        "band": { "kind": "polyline", "points": [["-hw", "h / 2", 0.03], ["hw", "h / 2", 0.03]] }
      },
      "surfaces": { "front": { "kind": "ruled", "a": "top", "b": "bottom" } },
      "protos": { "stud": { "parts": [ { "op": "lathe", "profile": [[0, 0], [0.012, 0], [0.01, 0.006], [0, 0.008]], "segments": { "low": 6, "high": 10 }, "material": "iron" } ] } },
      "parts": [
        { "name": "front", "node": "body", "op": "boards", "on": "front", "count": 4,
          "butts": { "length": 0.45, "stagger": [0, 0.5] },
          "piece": { "thickness": 0.02, "gap": 0.004 }, "material": "wood", "vary": { "tone": 0.1 },
          "detail": { "low": { "count": 1, "butts": null } } },
        { "name": "band", "node": "bands", "op": "sweep", "along": "band", "profile": { "rect": [0.03, 0.006] }, "up": [0, 0, 1], "material": "iron" },
        { "name": "studs", "node": "bands", "op": "along", "curve": "band", "count": 5, "inset": 0.05, "proto": "stud", "anchor": "stud_{i}", "tiers": ["high"] },
        { "name": "knob", "op": "lathe", "profile": [[0, 0], [0.03, 0], [0.02, 0.04], [0, 0.05]], "at": [0, "h", 0], "tiers": ["high"], "material": "iron" }
      ],
      "anchors": { "lid": { "at": [0, "h", 0], "q": [0, 0, 0, 1] } },
      "stages": { "order": ["intact", "damaged"], "rules": { "damaged": { "knock_out": { "parts": ["fr*"], "share": 0.5 } } } },
      "split": { "groups": ["body"], "normal": [1, 0, 0], "d": 0, "names": ["r", "l"] }
    }"##;

    #[test]
    fn a_recipe_builds_per_tier_deterministically() {
        let r = Recipe::parse(R).unwrap();
        assert_eq!(r.tiers.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["low", "high"]);
        let lo = r.build("low", &[]).unwrap();
        let hi = r.build("high", &[]).unwrap();
        // Low: the band is one strip in one length.
        assert_eq!(lo.pieces.iter().filter(|p| p.id.starts_with("front/")).count(), 1);
        assert!(lo.dropped.iter().any(|d| d.0 == "studs") && lo.dropped.iter().any(|d| d.0 == "knob"));
        // High: 4 strips, each cut at staggered butts (0.45 m over 1 m).
        let boards: Vec<&Piece> = hi.pieces.iter().filter(|p| p.id.starts_with("front/")).collect();
        assert!(boards.len() >= 8, "{}", boards.len());
        assert_eq!(hi.instances.len(), 5);
        assert_eq!(hi.anchors.iter().filter(|a| a.name.starts_with("stud_")).count(), 5);
        assert!(crate::check::pieces(&hi).is_empty(), "{:?}", crate::check::pieces(&hi));
        // Same recipe, same seed: the same model.
        assert_eq!(hi, r.build("high", &[]).unwrap());
        // Stage rule: some boards leave the damaged stage, and only those.
        assert!(boards.iter().any(|p| p.stages == ["intact"]));
        assert!(boards.iter().any(|p| p.stages.is_empty()));
        // The split put boards into body_r / body_l, and check 4 holds.
        let (groups, n, d, names) = r.split("high").unwrap().unwrap();
        let whole = Recipe::unsplit(&hi, &groups, &names);
        let g: Vec<&str> = groups.iter().map(String::as_str).collect();
        assert!(crate::model::check_split(&whole, &hi, &g, (&names.0, &names.1)).is_empty());
        // At most one board per strip straddles x = 0 (it goes whole to one side).
        let body = Model { pieces: whole.pieces.iter().filter(|p| p.group == "body").cloned().collect(), ..Model::default() };
        assert!(crate::model::crossing(&body, n, d) <= 4);
        // Tone varies per piece; a parameter override rebuilds wider.
        let tones: Vec<[u8; 4]> = boards.iter().map(|p| p.colour).collect();
        assert!(tones.windows(2).any(|w| w[0] != w[1]));
        let wide = r.build("high", &[("w".to_string(), 1.5)]).unwrap();
        let span = |m: &Model| m.pieces.iter().fold(Aabb::EMPTY, |b, p| b.union(&p.mesh.bounds())).size()[0];
        assert!((span(&wide) - span(&hi) - 0.5).abs() < 0.02, "{} {}", span(&wide), span(&hi));
        assert!(r.build("high", &[("w".to_string(), 9.0)]).is_err());
    }

    #[test]
    fn per_piece_seeds_do_not_ripple() {
        // Changing one part's count leaves every other piece's tone alone.
        let r = Recipe::parse(R).unwrap();
        let a = r.build("high", &[]).unwrap();
        let more = R.replace("\"count\": 5", "\"count\": 7");
        let b = Recipe::parse(&more).unwrap().build("high", &[]).unwrap();
        for p in &a.pieces {
            let q = b.pieces.iter().find(|q| q.id == p.id).unwrap();
            assert_eq!((p.colour, p.var.to_bits()), (q.colour, q.var.to_bits()), "{}", p.id);
        }
        assert_ne!(r.hash, Recipe::parse(&more).unwrap().hash);
        // Whitespace does not change the hash.
        assert_eq!(r.hash, Recipe::parse(&R.replace("\n", "\n  ")).unwrap().hash);
    }

    #[test]
    fn bad_recipes_say_what_is_wrong() {
        assert!(Recipe::parse("{}").unwrap_err().contains("construct"));
        let e = Recipe::parse(&R.replace("\"op\": \"block\"", "x").replace("\"op\": \"sweep\"", "\"op\": \"twist\"")).unwrap().build("low", &[]).unwrap_err();
        assert!(e.contains("unknown op `twist`"), "{e}");
        let e = Recipe::parse(&R.replace("\"w / 2\"", "\"q / 2\"")).unwrap().build("low", &[]).unwrap_err();
        assert!(e.contains("unknown name"), "{e}");
    }
}
