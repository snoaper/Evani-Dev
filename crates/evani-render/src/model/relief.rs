//! Surface relief on textured models (feature `relief`, S3-10): a
//! material's `extras.evani.relief` (docs/MODEL_FORMAT.md) read into the
//! words the relief shader (`shaders/model_relief.wgsl`) reads, and the
//! parallax step count of each tier.
//!
//! * `height`: a height map (a glTF texture, red channel, 1 = top) and its
//!   depth in UV units. Parallax occlusion on every tier: a fine march on
//!   High and Ultra, a 2-step one elsewhere; the normal map carries the same
//!   relief everywhere, so the surface reads the same on every device.
//! * `micro`: a pattern recipe (id, period, depth, angle, mask) the look's
//!   `micro_detail` slot evaluates on every tier, faded out below 1.5 px.
//!
//! A key that is missing or out of range turns only its own part off; the
//! model still draws (`cargo xtask check` reports bad recipes).

use evani_gltf::{Gltf, Material};
use serde_json::Value;

use super::TexNeed;

/// Floats per material relief uniform (2 vec4): height on, depth, mask,
/// unused; pattern id, period, depth, angle (radians).
pub const RELIEF_WORDS: usize = 8;

/// Deepest height map accepted (UV units): deeper swims at grazing angles.
pub const MAX_DEPTH: f32 = 0.25;

/// A material's relief as the model pipeline reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Relief {
    pub words: [f32; RELIEF_WORDS],
    /// The height map: an index into [`super::Prepared::textures`].
    pub height: Option<usize>,
}

/// The micro patterns, by name, in id order (id = position + 1).
pub const PATTERNS: [&str; 4] = ["knurl", "ribs", "dots", "grain"];

/// Parallax occlusion steps on a tier (by its id). High and Ultra march the
/// height map finely; Lite, Low, Medium and VR take 2 steps (3 height
/// samples, the last two blended), the cheap parallax that keeps raised
/// panels reading raised on phones (Chris's same-look rule; without it the
/// relief read clearly flatter there, DECISIONS "Surface relief").
pub fn parallax_steps(tier: &str) -> u32 {
    match tier {
        "high" => 8,
        "ultra" => 16,
        _ => 2,
    }
}

/// The mask code of `mask`: 0 none, 1 to 4 the base map's r g b a, 5 to 8
/// the normal map's. `None`: not a valid mask.
pub fn mask_code(mask: &Value) -> Option<f32> {
    match mask {
        Value::Null => Some(0.0),
        Value::String(s) if s == "none" => Some(0.0),
        Value::Object(o) => {
            let tex = match o.get("texture").and_then(Value::as_str)? {
                "base" => 0,
                "normal" => 4,
                _ => return None,
            };
            let ch = match o.get("channel").and_then(Value::as_str)? {
                "r" => 1,
                "g" => 2,
                "b" => 3,
                "a" => 4,
                _ => return None,
            };
            Some((tex + ch) as f32)
        }
        _ => None,
    }
}

fn num(v: &Value, key: &str) -> Option<f32> {
    v.get(key).and_then(Value::as_f64).map(|x| x as f32).filter(|x| x.is_finite())
}

/// The relief of material `m` (none: all zero), listing its height map.
pub fn material(g: &Gltf, m: Option<&Material>, textures: &mut Vec<TexNeed>) -> Relief {
    let mut r = Relief::default();
    let Some(rel) = m.and_then(|m| m.extras.get("evani")).and_then(|e| e.get("relief")) else {
        return r;
    };
    if let Some(h) = rel.get("height") {
        let depth = num(h, "depth").filter(|d| *d > 0.0 && *d <= MAX_DEPTH);
        let index = h.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok());
        if let (Some(d), Some(i)) = (depth, index) {
            r.height = super::need(g, i, false, textures);
            if r.height.is_some() {
                r.words[0] = 1.0;
                r.words[1] = d;
            }
        }
    }
    if let Some(mc) = rel.get("micro") {
        let id = mc.get("pattern").and_then(Value::as_str).and_then(|p| PATTERNS.iter().position(|n| *n == p));
        let size = num(mc, "size").filter(|s| *s > 0.0);
        let depth = num(mc, "depth").unwrap_or(0.5);
        let mask = mask_code(mc.get("mask").unwrap_or(&Value::Null));
        if let (Some(id), Some(size), Some(mask), true) = (id, size, mask, (0.0..=1.0).contains(&depth)) {
            r.words[2] = mask;
            r.words[4] = (id + 1) as f32;
            r.words[5] = size;
            r.words[6] = depth;
            r.words[7] = num(mc, "angle").unwrap_or(0.0).to_radians();
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_and_patterns_read_as_documented() {
        assert_eq!(mask_code(&json!("none")), Some(0.0));
        assert_eq!(mask_code(&Value::Null), Some(0.0));
        assert_eq!(mask_code(&json!({"texture": "base", "channel": "r"})), Some(1.0));
        assert_eq!(mask_code(&json!({"texture": "normal", "channel": "a"})), Some(8.0));
        assert_eq!(mask_code(&json!({"texture": "height", "channel": "r"})), None, "the height map is not on every tier");
        assert_eq!(mask_code(&json!({"texture": "base", "channel": "x"})), None);
        assert_eq!(PATTERNS, ["knurl", "ribs", "dots", "grain"]);
    }
}
