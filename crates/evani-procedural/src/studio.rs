//! The realism studio (procedural realism round 8): the light, shadow,
//! ambient, reflection and tone settings the Material library shows every
//! realism asset in, as one list of live params. A game that applies
//! [`apply`] (once, after its modules registered) sees a library asset as
//! the library shows it; the viewer adds only its own camera, backdrop and
//! depth of field on top (apps/material-view `VIEWER_EXTRAS`).
//!
//! What the params need built (a param the game does not build is skipped
//! and named in [`apply`]'s answer, never an error):
//! * evani-render features `ibl` and `studio-sky` (the studio panels in the
//!   sky light: `render.sky.studio`), `gtao` (ambient occlusion on the
//!   ambient light only), `pcss-stable` and `shadow-cascades` (soft
//!   shadows of a physical size), with evani-fx `shadows`;
//! * the stylizedpbr look with features `film` (the studio hemisphere and
//!   the Khronos PBR Neutral tone curve: `look.stylizedpbr.studio`,
//!   `look.stylizedpbr.tonemap`) and `pcss` (`look.stylizedpbr.pcss_*`);
//! * the materials' recipes in `[materials] recipes` (docs/PROCEDURAL.md
//!   "Applying a realism material in a game").
//!
//! The sun's direction is the game's own (`render.light.direction`); the
//! library's is [`SUN`].

use evani_core::params::{ParamRegistry, ParamValue};

/// The library's sun: elevation and bearing (degrees; the bearing from +z
/// towards +x, the library camera at 30): low, from the right and a little
/// behind, as in the reference pictures.
pub const SUN: (f32, f32) = (32.0, 110.0);

/// The studio's params and values.
pub const PRESET: [(&str, ParamValue); 25] = [
    // Light: a warm low key, no rim, kick or wrap, no camera-tied fill (the
    // studio panels fill), no shadow tint (no blue shadow sides).
    ("render.light.color", ParamValue::Color([1.0, 0.87, 0.72])),
    ("render.light.intensity", ParamValue::F32(2.3)),
    ("render.ambient", ParamValue::Color([0.6, 0.58, 0.55])),
    ("render.shadow_tint_strength", ParamValue::F32(0.0)),
    ("look.stylizedpbr.rim", ParamValue::F32(0.0)),
    ("look.stylizedpbr.rim_tint", ParamValue::F32(0.0)),
    ("look.stylizedpbr.kick", ParamValue::F32(0.0)),
    ("look.stylizedpbr.wrap", ParamValue::F32(0.0)),
    ("look.stylizedpbr.fill", ParamValue::F32(0.0)),
    // Ambient and reflections: the sky light as studio panels, the AO a
    // tight contact band.
    ("look.stylizedpbr.studio", ParamValue::F32(1.0)),
    ("look.stylizedpbr.env_spec", ParamValue::F32(0.8)),
    ("render.sky.studio", ParamValue::F32(1.0)),
    ("render.sky.sun_glow", ParamValue::F32(0.0)),
    ("render.ibl.strength", ParamValue::F32(1.0)),
    ("render.ibl.roughness", ParamValue::F32(0.6)),
    ("render.ao.radius", ParamValue::F32(0.3)),
    ("render.ao.strength", ParamValue::F32(1.0)),
    // Soft shadows: a key about 7 degrees across (about 12 cm of penumbra a
    // metre from the contact), the near cascade over the subject.
    ("look.stylizedpbr.pcss_light_deg", ParamValue::F32(7.0)),
    ("look.stylizedpbr.pcss_search", ParamValue::F32(14.0)),
    ("render.shadow_cascade_split", ParamValue::F32(0.8)),
    // The picture: Khronos PBR Neutral at a fixed exposure, saturation 1.
    ("look.stylizedpbr.tonemap", ParamValue::F32(1.0)),
    ("render.exposure", ParamValue::F32(0.62)),
    ("render.saturation", ParamValue::F32(1.0)),
    // The library's grass reach (evani-grass, where built), and no carpet of
    // its own under the blades: a library terrain's turf is the ground (the
    // carpet, drawn at the patch's height, hid the turf's dry and worn
    // patches).
    ("material.grass.distance", ParamValue::F32(2.0)),
    ("material.grass.ground", ParamValue::Bool(false)),
];

/// `v` as a value of the kind `like` is (a colour into a vec3 param and the
/// other way); `None` when the kinds differ otherwise.
pub fn coerce(like: ParamValue, v: ParamValue) -> Option<ParamValue> {
    match (like, v) {
        (ParamValue::Vec3(_), ParamValue::Color(c)) => Some(ParamValue::Vec3(c)),
        (ParamValue::Color(_), ParamValue::Vec3(c)) => Some(ParamValue::Color(c)),
        (a, b) if a.ty() == b.ty() => Some(b),
        _ => None,
    }
}

/// Set every [`PRESET`] param this build has; the names it lacks come back
/// (features to turn on for the full look).
pub fn apply(reg: &mut ParamRegistry) -> Vec<&'static str> {
    let mut missing = Vec::new();
    for (name, v) in PRESET {
        match reg.id(name).and_then(|id| {
            reg.value(id)
                .and_then(|like| coerce(like, v))
                .map(|v| (id, v))
        }) {
            Some((id, v)) => {
                let _ = reg.set(id, v);
            }
            None => missing.push(name),
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::ParamDef;

    #[test]
    fn the_studio_sets_what_is_built_and_names_the_rest() {
        let mut reg = ParamRegistry::new();
        let _ = reg.register_f32(ParamDef::f32("render.exposure", 1.0));
        let _ = reg.register_vec3(ParamDef::color("render.ambient", [0.1; 3]));
        let missing = apply(&mut reg);
        assert_eq!(
            reg.value(reg.id("render.exposure").unwrap()),
            Some(ParamValue::F32(0.62))
        );
        assert!(matches!(
            reg.value(reg.id("render.ambient").unwrap()),
            Some(ParamValue::Color(_) | ParamValue::Vec3(_))
        ));
        assert_eq!(missing.len(), PRESET.len() - 2);
        assert!(missing.contains(&"render.sky.studio"));
    }
}
