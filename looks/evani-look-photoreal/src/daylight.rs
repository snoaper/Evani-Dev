//! The `daylight` preset: the photoreal look under a
//! low, golden late-afternoon sun with long shadows, a deep blue sky with
//! fair-weather clouds, a cool sky fill and a light distance haze matched to
//! that sky, graded by the look's own filmic stage and grade (warm
//! highlights, cool shadows, a gentle contrast, colour pulled back a little:
//! the catalog's photoreal look is desaturated, even in sunshine).
//!
//! On top of the look's materials it adds `world` and `decal` (white, for
//! procedural surfaces painted through the kit-material hook, such as
//! evani-material-proc) and an unlit white `sky` (that module's sky dome).
//! It never depends on the module: the sky-matched colours are constants
//! (evani-material-proc's tests check them against its sky).
//!
//! A game opts in by adding [`DaylightLook`] instead of `PhotorealLook`. It
//! is deliberately not behind a Cargo feature: declaring any feature on this
//! crate changes its `-C metadata` (cargo 1.98), and with it the bytes of
//! every game that links the look, even with the feature off. A game that
//! never names `DaylightLook` links none of this code (tactical-fps ships
//! byte for byte as before; DECISIONS "ChimpDuty materials, sky and daylight
//! intake").

use evani_core::params::ParamRegistry;
use evani_core::plugin::{LookContext, LookPlugin, MaterialDesc, PluginError};
use serde_json::json;

use crate::{FilmicSettings, GradeSettings, PhotorealLook};

/// The sun's height above the horizon (degrees): low enough for long,
/// raking shadows, high enough that open ground still has sunlight.
pub const SUN_ELEVATION: f32 = 15.0;
/// The sun's bearing (degrees from +z towards +x).
pub const SUN_AZIMUTH: f32 = 56.0;
/// The golden late-afternoon sunlight (linear).
pub const SUN_COLOR: [f32; 3] = [1.0, 0.78, 0.52];
/// The clear colour (what the haze fades to): the default Preetham sky's
/// mean horizon colour (3 percent up) at this sun, scaled into 0..1.
pub const HORIZON: [f32; 3] = [0.989, 1.0, 0.770];
/// The ambient sky tints' shape (only their hue matters; the level stays
/// `render.ambient`): the dome's zenith and horizon hues, half-way to white,
/// because a shadowed face sees much sunlit ground, not only open sky.
pub const SKY_ZENITH: [f32; 3] = [0.549, 0.664, 1.0];
pub const SKY_HORIZON: [f32; 3] = [0.994, 1.0, 0.885];
/// The bounce from sunlit ground.
pub const SKY_GROUND: [f32; 3] = [0.80, 0.60, 0.38];

/// The material names the preset adds.
pub const WORLD: &str = "world";
pub const DECAL: &str = "decal";
pub const SKY: &str = "sky";

/// The fog slot body: a thin haze from 30 m, thinning with height, at most
/// 0.5, and none past 450 m (a sky dome carries its own aerial haze).
pub const FOG_BODY: &str = "    // evani-look-photoreal daylight: distance haze, none on a sky dome\n    if (distance > 450.0) {\n        return 0.0;\n    }\n    let d = max(distance - 30.0, 0.0);\n    let density = 0.0045 * exp(-max(height, 0.0) / 40.0);\n    return min(1.0 - exp(-d * density), 0.5);";

/// The filmic stage: a slightly earlier shoulder than the overcast default
/// (bright sun, punchier highlights) and a lighter toe.
pub const FILMIC: FilmicSettings = FilmicSettings {
    white: 8.0,
    toe: 0.02,
};

/// The grade: colour pulled back a little (0.88, against the overcast
/// default's 0.76; catalog/looks.json `photoreal` is "desaturated"), a
/// gentle contrast, cool shadows and warm highlights, no black lift.
pub const GRADE: GradeSettings = GradeSettings {
    contrast: 1.1,
    saturation: 0.88,
    lift: 0.0,
    split: 0.3,
    shadow_tint: [0.88, 0.93, 1.0],
    highlight_tint: [1.0, 0.94, 0.85],
};

/// The direction towards the sun at `elevation` degrees up and `azimuth`
/// degrees from +z towards +x.
pub fn sun_direction(elevation: f32, azimuth: f32) -> [f32; 3] {
    let (e, a) = (elevation.to_radians(), azimuth.to_radians());
    [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()]
}

/// The preset's sun direction (also where a sky dome's sun disc goes).
pub fn sun() -> [f32; 3] {
    sun_direction(SUN_ELEVATION, SUN_AZIMUTH)
}

impl PhotorealLook {
    /// The daylight preset: the look's filmic and grade at daylight values.
    /// Add it with `add_look(&DaylightLook)` for the matching sun, sky and
    /// haze as well.
    pub fn daylight() -> Self {
        PhotorealLook::default()
            .with_filmic(FILMIC)
            .with_grade(GRADE)
    }
}

/// The photoreal look with the daylight preset (catalog look `photoreal`).
#[derive(Debug, Clone, Copy, Default)]
pub struct DaylightLook;

impl LookPlugin for DaylightLook {
    fn name(&self) -> &'static str {
        "photoreal"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn points(&self) -> u32 {
        crate::COST_POINTS
    }

    fn styles(&self) -> evani_core::effects::StyleClasses {
        evani_core::effects::StyleClasses::THREE_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        // The daylight slots first, so they keep low slot numbers when this
        // look is layered over another (the renderer has 16).
        let mut w = MaterialDesc::new(WORLD, [1.0; 4]);
        w.roughness = 0.85;
        look.material(w);
        let mut d = MaterialDesc::new(DECAL, [1.0; 4]);
        d.roughness = 0.9;
        look.material(d);
        let mut s = MaterialDesc::new(SKY, [1.0; 4]);
        s.unlit = true;
        look.material(s);
        PhotorealLook::daylight().build(look)?;

        // After the look's own: later values win.
        look.shader_slot("fog", FOG_BODY)?;
        look.post_effect("bloom", false);
        look.post_param("render.clear_color", json!(HORIZON));
        // Neutral-warm: an upward face sees mostly the blue zenith, so a blue
        // ambient here turns floor shadows navy.
        look.post_param("render.ambient", json!([0.40, 0.37, 0.33]));
        look.post_param("render.light.direction", json!(sun()));
        look.post_param("render.light.color", json!(SUN_COLOR));
        look.post_param("render.light.intensity", json!(2.9));
        look.post_param("render.exposure", json!(0.62));
        look.post_param("render.shadow_tint_strength", json!(0.0));
        // Engine values the game may not build (an fx effect it doesn't
        // list; the sky tints and reflections without evani-render's `ibl`)
        // are left out instead of logging "not in this build".
        let has = |name: &str| {
            look.world
                .try_res::<ParamRegistry>()
                .is_some_and(|r| r.id(name).is_some())
        };
        let (tints, ibl, shadows) = (
            has("render.sky.zenith"),
            has("render.ibl.specular"),
            has("fx.shadows.distance"),
        );
        if tints {
            look.post_param("render.sky.zenith", json!(SKY_ZENITH));
            look.post_param("render.sky.horizon", json!(SKY_HORIZON));
            look.post_param("render.sky.ground", json!(SKY_GROUND));
        }
        // Dusty stone and concrete barely reflect, and the sky reflection is
        // not occluded by baked AO: keep it faint and matte.
        if ibl {
            look.post_param("render.ibl.specular", json!(0.015));
            look.post_param("render.ibl.roughness", json!(0.95));
        }
        if shadows {
            look.post_param("fx.shadows.distance", json!(45.0));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preset_colours_are_in_range_and_the_sun_is_low() {
        for c in [
            HORIZON,
            SKY_ZENITH,
            SKY_HORIZON,
            SKY_GROUND,
            SUN_COLOR,
            GRADE.shadow_tint,
            GRADE.highlight_tint,
        ] {
            assert!(c.iter().all(|v| (0.0..=1.0).contains(v)), "{c:?}");
        }
        let s = sun();
        assert!(s[1] > 0.2 && s[1] < 0.3, "{s:?}");
        assert!(((s[0] * s[0] + s[1] * s[1] + s[2] * s[2]) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn daylight_differs_from_the_overcast_default_only_in_its_settings() {
        let d = PhotorealLook::daylight();
        assert_eq!(d.filmic, FILMIC);
        assert_eq!(d.grade, GRADE);
        assert_ne!(d.grade, GradeSettings::default());
    }
}
