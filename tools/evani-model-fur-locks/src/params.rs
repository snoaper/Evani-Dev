//! The module's registered tunables, named `kit.fur_locks.<name>`
//! (Evani.Dev's parameter rule), plus the per-region table
//! (`kit.fur_locks.region.<key>.<name>`), read from and written to JSON so
//! the live tools can drive them. Sizes are in millimetres.

use crate::{LockError, lerr};
use serde_json::{Map, Value, json};

/// One registered scalar parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub doc: &'static str,
}

const fn p(name: &'static str, default: f64, min: f64, max: f64, doc: &'static str) -> Param {
    Param { name, default, min, max, doc }
}

/// Hard cap on the solid's triangles: 1,750 locks of 8 triangles.
pub const MAX_TRIS: usize = 14_000;

/// Hard cap on the lock count.
pub const MAX_LOCKS: usize = 1_750;

/// Every scalar parameter this module reads.
pub const PARAMS: [Param; 22] = [
    p("kit.fur_locks.count", 1_750.0, 0.0, 1_750.0, "Locks wanted (fewer when the triangle cap or the surface allows fewer)"),
    p("kit.fur_locks.segments", 4.0, 3.0, 5.0, "Segments along a lock; a lock is 2 x segments triangles (6, 8 or 10)"),
    p("kit.fur_locks.seed", 1.0, 0.0, 1.0e9, "Random seed (whole number); same seed, same locks"),
    p("kit.fur_locks.length_mm", 40.0, 2.0, 300.0, "Lock length for fur regions without their own length"),
    p("kit.fur_locks.length_jitter", 0.25, 0.0, 0.8, "Random length change, as a share of the length (plus or minus)"),
    p("kit.fur_locks.width_mm", 12.0, 1.0, 80.0, "Lock width at the root"),
    p("kit.fur_locks.tip_width", 0.18, 0.02, 1.0, "Width at the tip as a share of the root width"),
    p("kit.fur_locks.lift", 0.3, 0.0, 1.0, "How far a lock rises off the skin at the root (0 lies flat, 1 stands straight out)"),
    p("kit.fur_locks.droop", 0.6, 0.0, 2.0, "Gravity bend towards the tip"),
    p("kit.fur_locks.curl", 0.25, 0.0, 1.0, "Random sideways bend towards the tip"),
    p("kit.fur_locks.twist", 0.5, 0.0, 1.5, "Random roll of a ribbon about its own axis (radians, plus or minus)"),
    p("kit.fur_locks.clump", 0.55, 0.0, 0.95, "How far tips pull towards their clump's leading lock"),
    p("kit.fur_locks.clump_mm", 30.0, 3.0, 300.0, "Clump size: spacing of the leading locks"),
    p("kit.fur_locks.flow_noise", 0.45, 0.0, 1.5, "Wobble of the flow direction (radians, plus or minus)"),
    p("kit.fur_locks.flow_scale_mm", 90.0, 5.0, 1_000.0, "Size of the flow wobble's swirls"),
    p("kit.fur_locks.gear_boost", 2.0, 1.0, 4.0, "Length factor where fur meets gear"),
    p("kit.fur_locks.gear_band_mm", 30.0, 0.0, 300.0, "Distance from gear over which the boost fades to none"),
    p("kit.fur_locks.cover_mm", 25.0, 0.0, 300.0, "Skip roots with gear above them within this distance (fur under a sleeve)"),
    p("kit.fur_locks.spacing", 0.55, 0.0, 1.0, "Smallest root spacing, as a share of the even spacing"),
    p("kit.fur_locks.flow_x", 0.0, -1.0, 1.0, "Default flow direction, x (fur regions without their own flow)"),
    p("kit.fur_locks.flow_y", -1.0, -1.0, 1.0, "Default flow direction, y (up is +y)"),
    p("kit.fur_locks.flow_z", -0.3, -1.0, 1.0, "Default flow direction, z (front is +z)"),
];

/// Which way a region's hair lies before the wobble, droop and clumping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flow {
    /// One direction everywhere (projected onto the surface).
    Dir([f64; 3]),
    /// Away from a point (a crown, the centre of a face).
    Away([f64; 3]),
    /// Towards a point (a part line, an elbow).
    Toward([f64; 3]),
}

/// Fur settings for one region key: a colour region (`fur`) or a solid and
/// region (`head.fur`, which wins over `fur` on that solid).
#[derive(Clone, Debug, PartialEq)]
pub struct RegionFur {
    pub key: String,
    /// Lock length (millimetres).
    pub length_mm: f64,
    /// Locks per area relative to other regions (0 grows none).
    pub density: f64,
    /// Width factor on `kit.fur_locks.width_mm`.
    pub width: f64,
    /// Own flow (`None`: the default flow).
    pub flow: Option<Flow>,
}

impl RegionFur {
    pub fn new(key: &str, length_mm: f64) -> Self {
        RegionFur { key: key.to_string(), length_mm, density: 1.0, width: 1.0, flow: None }
    }
    pub fn density(mut self, d: f64) -> Self {
        self.density = d;
        self
    }
    pub fn width(mut self, w: f64) -> Self {
        self.width = w;
        self
    }
    pub fn flow(mut self, f: Flow) -> Self {
        self.flow = Some(f);
        self
    }
}

/// Per-region names and ranges (`kit.fur_locks.region.<key>.<name>`).
pub const REGION_FIELDS: [(&str, f64, f64); 3] = [("length_mm", 2.0, 300.0), ("density", 0.0, 10.0), ("width", 0.1, 4.0)];

/// Regions that are bare skin: neither fur nor gear (no locks, no boost).
pub const DEFAULT_BARE: [&str; 12] =
    ["face", "brow", "socket", "lip", "ear", "eye_white", "iris", "dark", "mouth", "teeth", "skin", "nose"];

/// Every setting of the generator.
#[derive(Clone, Debug, PartialEq)]
pub struct LockParams {
    vals: [f64; PARAMS.len()],
    /// Fur regions (only these grow locks).
    pub regions: Vec<RegionFur>,
    /// Regions that are neither fur nor gear. Every other region is gear.
    pub bare: Vec<String>,
}

impl Default for LockParams {
    fn default() -> Self {
        let mut vals = [0.0; PARAMS.len()];
        for (v, p) in vals.iter_mut().zip(PARAMS.iter()) {
            *v = p.default;
        }
        LockParams {
            vals,
            regions: vec![RegionFur::new("fur", 40.0), RegionFur::new("face_fur", 18.0).density(1.6).width(0.7)],
            bare: DEFAULT_BARE.iter().map(|s| s.to_string()).collect(),
        }
    }
}

fn find(name: &str) -> Option<usize> {
    PARAMS.iter().position(|p| p.name == name)
}

fn check(name: &str, v: f64, min: f64, max: f64) -> Result<(), LockError> {
    if v >= min && v <= max { Ok(()) } else { Err(lerr(format!("parameter `{name}` = {v} is outside {min}..={max}"))) }
}

impl LockParams {
    /// A scalar value by name (`None` for an unknown name).
    pub fn get(&self, name: &str) -> Option<f64> {
        find(name).map(|i| self.vals[i])
    }

    /// A value by name, in metres when its name ends in `_mm`.
    pub(crate) fn m(&self, name: &str) -> f64 {
        let v = self.get(name).unwrap_or(0.0);
        if name.ends_with("_mm") { v * 0.001 } else { v }
    }

    /// Set one scalar value (checked against its range).
    pub fn set(&mut self, name: &str, v: f64) -> Result<(), LockError> {
        let Some(i) = find(name) else {
            return Err(lerr(format!("unknown parameter `{name}`")));
        };
        let p = PARAMS[i];
        check(name, v, p.min, p.max)?;
        self.vals[i] = v;
        Ok(())
    }

    /// The default flow direction (not normalised).
    pub fn flow(&self) -> [f64; 3] {
        [self.m("kit.fur_locks.flow_x"), self.m("kit.fur_locks.flow_y"), self.m("kit.fur_locks.flow_z")]
    }

    /// Set (or add) a region's entry.
    pub fn set_region(&mut self, r: RegionFur) -> &mut Self {
        match self.regions.iter_mut().find(|x| x.key == r.key) {
            Some(x) => *x = r,
            None => self.regions.push(r),
        }
        self
    }

    /// The entry for a triangle of `solid` in colour region `region`:
    /// `<solid>.<region>` wins over `<region>`.
    pub fn region_for(&self, solid: &str, region: &str) -> Option<&RegionFur> {
        let full = format!("{solid}.{region}");
        self.regions.iter().find(|r| r.key == full).or_else(|| self.regions.iter().find(|r| r.key == region))
    }

    /// Read `{"kit.fur_locks.length_mm": 40, "kit.fur_locks.region.head.fur.length_mm": 55, ...}`.
    /// Missing names keep their defaults (a region named only here starts
    /// from the default length, density 1 and width 1); unknown
    /// `kit.fur_locks.*` names and out-of-range values fail; other keys are
    /// ignored (one JSON can carry several modules' values). Flows are set
    /// from code ([`RegionFur::flow`]).
    pub fn from_json(v: &Value) -> Result<Self, LockError> {
        let Some(obj) = v.as_object() else {
            return Err(lerr("parameters must be a JSON object"));
        };
        let mut out = LockParams::default();
        // Scalars first, so a new region picks up a changed default length.
        for (k, x) in obj {
            if !k.starts_with("kit.fur_locks.") || k.starts_with("kit.fur_locks.region.") {
                continue;
            }
            let Some(f) = x.as_f64() else {
                return Err(lerr(format!("parameter `{k}` must be a number")));
            };
            out.set(k, f)?;
        }
        for (k, x) in obj {
            let Some(rest) = k.strip_prefix("kit.fur_locks.region.") else { continue };
            let Some((key, field)) = rest.rsplit_once('.') else {
                return Err(lerr(format!("parameter `{k}` needs a region key and a field")));
            };
            let Some(&(_, min, max)) = REGION_FIELDS.iter().find(|f| f.0 == field) else {
                return Err(lerr(format!("unknown region field in `{k}`")));
            };
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.') {
                return Err(lerr(format!("bad region key in `{k}`")));
            }
            let Some(f) = x.as_f64() else {
                return Err(lerr(format!("parameter `{k}` must be a number")));
            };
            check(k, f, min, max)?;
            let default_len = out.vals[find("kit.fur_locks.length_mm").unwrap_or(0)];
            if !out.regions.iter().any(|r| r.key == key) {
                out.regions.push(RegionFur::new(key, default_len));
            }
            if let Some(r) = out.regions.iter_mut().find(|r| r.key == key) {
                match field {
                    "length_mm" => r.length_mm = f,
                    "density" => r.density = f,
                    _ => r.width = f,
                }
            }
        }
        Ok(out)
    }

    /// The values as JSON (every registered scalar and every region field).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        for (p, v) in PARAMS.iter().zip(self.vals.iter()) {
            m.insert(p.name.into(), json!(v));
        }
        for r in &self.regions {
            let base = format!("kit.fur_locks.region.{}", r.key);
            m.insert(format!("{base}.length_mm"), json!(r.length_mm));
            m.insert(format!("{base}.density"), json!(r.density));
            m.insert(format!("{base}.width"), json!(r.width));
        }
        Value::Object(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_rule_and_round_trip() {
        for p in PARAMS {
            assert!(p.name.starts_with("kit.fur_locks."), "{}", p.name);
            assert!(p.name.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'), "{}", p.name);
            assert!(p.default >= p.min && p.default <= p.max, "{}", p.name);
        }
        let d = LockParams::default();
        assert_eq!(LockParams::from_json(&d.to_json()).unwrap(), d);
        let v = json!({
            "kit.fur_locks.length_mm": 50.0,
            "kit.fur_locks.region.head.fur.length_mm": 70.0,
            "kit.fur_locks.region.fur.density": 2.0,
            "kit.pals.band_mm": 20.0,
            "look.x": 1
        });
        let k = LockParams::from_json(&v).unwrap();
        assert_eq!(k.get("kit.fur_locks.length_mm"), Some(50.0));
        assert!((k.m("kit.fur_locks.length_mm") - 0.05).abs() < 1e-12);
        assert_eq!(k.region_for("head", "fur").map(|r| r.length_mm), Some(70.0));
        assert_eq!(k.region_for("arm", "fur").map(|r| (r.length_mm, r.density)), Some((40.0, 2.0)));
        assert!(k.region_for("arm", "shirt").is_none());
        assert!(LockParams::from_json(&json!({ "kit.fur_locks.nope": 1 })).is_err());
        assert!(LockParams::from_json(&json!({ "kit.fur_locks.count": 5000 })).is_err());
        assert!(LockParams::from_json(&json!({ "kit.fur_locks.count": "x" })).is_err());
        assert!(LockParams::from_json(&json!({ "kit.fur_locks.region.fur.colour": 1 })).is_err());
        assert!(LockParams::from_json(&json!({ "kit.fur_locks.region.Fur!.density": 1 })).is_err());
        assert!(LockParams::from_json(&json!([1])).is_err());
        let mut z = LockParams::default();
        z.set("kit.fur_locks.flow_z", 0.5).unwrap();
        assert_eq!(z.flow()[2], 0.5);
        assert!(z.set("kit.fur_locks.flow_z", 2.0).is_err());
    }
}
