//! HDR background and fog colours, and a fog colour of its own (ChimpDuty
//! intake, limit 3). Without these features `render.clear_color` is a 0..1
//! colour and the fog fades to it.
//!
//! * Feature `hdr-color`: `render.clear_color` takes values up to
//!   [`HDR_MAX`] (linear RGB). The scene renders into an Rgba16Float target
//!   before exposure and the tonemap, so a background of 3.0 is a bright
//!   horizon the tonemap rolls off, the same way a lit surface above 1 is.
//! * Feature `fog-color`: `render.fog.own_color` (default off: the fog keeps
//!   the background colour, as before) and `render.fog.color` (the haze
//!   colour while it is on; HDR too when `hdr-color` is on). The forward and
//!   model shaders already take the fog colour as its own uniform, so this
//!   only changes which colour goes into it.
//!
//! Both are read once a frame at the start of the 3D path.

use evani_core::params::{ParamError, ParamRegistry, ParamValue};
#[cfg(feature = "fog-color")]
use evani_core::params::ParamDef;
use evani_core::plugin::PluginContext;

/// Highest channel value of an HDR colour (linear RGB).
pub const HDR_MAX: f32 = 64.0;
/// The background colour param.
pub const CLEAR_PARAM: &str = "render.clear_color";
/// Feature `fog-color`: the fog uses `render.fog.color` instead of the background.
pub const OWN_PARAM: &str = "render.fog.own_color";
/// Feature `fog-color`: the fog colour while `render.fog.own_color` is on.
pub const COLOR_PARAM: &str = "render.fog.color";

/// Highest channel value the colour params take in this build.
pub const fn color_max() -> f32 {
    if cfg!(feature = "hdr-color") {
        HDR_MAX
    } else {
        1.0
    }
}

/// Widen the background colour and register the fog colour params (render
/// module build, right after `render.clear_color`).
pub(crate) fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    #[cfg(feature = "hdr-color")]
    {
        let mut reg = ctx.world.res_mut::<ParamRegistry>();
        if let Some(id) = reg.id(CLEAR_PARAM) {
            reg.set_range(id, 0.0, HDR_MAX)?;
        }
    }
    #[cfg(feature = "fog-color")]
    {
        ctx.param_bool(
            ParamDef::bool(OWN_PARAM, false)
                .describe("Fog uses render.fog.color instead of the background colour")
                .gpu(),
        )?;
        ctx.param_vec3(
            ParamDef::color(COLOR_PARAM, [0.02, 0.025, 0.035])
                .range(0.0, color_max())
                .describe("Fog and haze colour (linear RGB) while render.fog.own_color is on")
                .gpu(),
        )?;
    }
    #[cfg(not(any(feature = "hdr-color", feature = "fog-color")))]
    let _ = ctx;
    Ok(())
}

/// The fog colour this frame: the background unless `render.fog.own_color`
/// is on (feature `fog-color`; a missing param keeps the background).
pub fn fog_color(reg: &ParamRegistry, background: [f32; 3]) -> [f32; 3] {
    let value = |name: &str| reg.id(name).and_then(|id| reg.value(id));
    match (value(OWN_PARAM), value(COLOR_PARAM)) {
        (Some(ParamValue::Bool(true)), Some(ParamValue::Color(c) | ParamValue::Vec3(c))) => c,
        _ => background,
    }
}

/// `lighting` with this frame's fog colour (the hook in the 3D path).
pub fn apply(reg: &ParamRegistry, mut lighting: crate::shaders::Lighting) -> crate::shaders::Lighting {
    lighting.fog_color = fog_color(reg, lighting.fog_color);
    lighting
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::ParamDef;

    fn registry() -> ParamRegistry {
        let mut r = ParamRegistry::new();
        r.register(ParamDef::color(CLEAR_PARAM, [0.1, 0.2, 0.3])).unwrap();
        r.register(ParamDef::bool(OWN_PARAM, false)).unwrap();
        r.register(ParamDef::color(COLOR_PARAM, [0.0; 3]).range(0.0, HDR_MAX)).unwrap();
        r
    }

    #[test]
    fn fog_keeps_the_background_by_default() {
        let r = registry();
        assert_eq!(fog_color(&r, [0.1, 0.2, 0.3]), [0.1, 0.2, 0.3]);
        assert_eq!(fog_color(&ParamRegistry::new(), [0.4; 3]), [0.4; 3]);
    }

    #[test]
    fn own_fog_colour_is_separate_and_hdr() {
        let mut r = registry();
        r.set_by_name(COLOR_PARAM, ParamValue::Vec3([2.5, 2.0, 1.5])).unwrap();
        assert_eq!(fog_color(&r, [0.1, 0.2, 0.3]), [0.1, 0.2, 0.3]);
        r.set_by_name(OWN_PARAM, ParamValue::Bool(true)).unwrap();
        assert_eq!(fog_color(&r, [0.1, 0.2, 0.3]), [2.5, 2.0, 1.5]);
    }

    #[test]
    fn colour_max_follows_the_feature() {
        assert_eq!(color_max(), if cfg!(feature = "hdr-color") { HDR_MAX } else { 1.0 });
    }
}
