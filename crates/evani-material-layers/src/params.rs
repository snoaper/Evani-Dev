//! Live strengths and the tier plan. Every layer's strength multiplies the
//! factor the model file gives it, so one slider dims or boosts that layer
//! on every material at once; the shared per-frame uniform carries them
//! (no pipeline rebuild when a value moves).
//!
//! Tier plan (`material.layers.quality` = auto):
//!
//! | tier            | path  | sheen | clear coat | anisotropy  | iridescence | weave            |
//! |-----------------|-------|-------|------------|-------------|-------------|------------------|
//! | lite, low       | cheap | yes   | yes        | Kajiya-Kay  | off         | colour, 1 tap    |
//! | medium and up   | full  | yes   | yes        | GGX, bent env | thin film | colour + relief, 3 taps |

use evani_core::params::{EnumParam, F32Param, ParamDef, ParamError, ParamRegistry};
use evani_core::plugin::PluginContext;

use crate::FRAME_WORDS;

pub const SHEEN_STRENGTH: &str = "material.layers.sheen_strength";
pub const CLEARCOAT_STRENGTH: &str = "material.layers.clearcoat_strength";
pub const ANISOTROPY_STRENGTH: &str = "material.layers.anisotropy_strength";
pub const IRIDESCENCE_STRENGTH: &str = "material.layers.iridescence_strength";
pub const WEAVE_STRENGTH: &str = "material.layers.weave_strength";
pub const WEAVE_SCALE: &str = "material.layers.weave_scale";
pub const QUALITY: &str = "material.layers.quality";

/// Every parameter this crate registers.
pub const NAMES: [&str; 7] = [
    SHEEN_STRENGTH,
    CLEARCOAT_STRENGTH,
    ANISOTROPY_STRENGTH,
    IRIDESCENCE_STRENGTH,
    WEAVE_STRENGTH,
    WEAVE_SCALE,
    QUALITY,
];

/// Which path the layered shader takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// Phones: Kajiya-Kay anisotropy, no iridescence, a one-tap weave.
    Cheap,
    /// Medium and up: stretched GGX, thin film, a weave with relief.
    Full,
}

impl Quality {
    /// The tier's own choice (`lite` and `low` cheap, the rest full).
    pub fn for_tier(tier: &str) -> Self {
        match tier {
            "lite" | "low" => Self::Cheap,
            _ => Self::Full,
        }
    }

    /// The `material.layers.quality` option (0 auto, 1 cheap, 2 full) on
    /// tier `tier`.
    pub fn pick(option: u32, tier: &str) -> Self {
        match option {
            1 => Self::Cheap,
            2 => Self::Full,
            _ => Self::for_tier(tier),
        }
    }
}

/// The parameter handles, kept as a world resource by [`register`].
#[derive(Clone, Copy, Debug)]
pub struct LayerParams {
    pub sheen: F32Param,
    pub clearcoat: F32Param,
    pub anisotropy: F32Param,
    pub iridescence: F32Param,
    pub weave: F32Param,
    pub weave_scale: F32Param,
    pub quality: EnumParam,
}

fn strength(name: &str, what: &str) -> ParamDef {
    ParamDef::f32(name, 1.0)
        .range(0.0, 2.0)
        .step(0.01)
        .unit("x")
        .describe(what)
        .gpu()
}

/// Register the parameters and keep their handles as a resource.
pub fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    let p = LayerParams {
        sheen: ctx.param_f32(strength(SHEEN_STRENGTH, "Cloth sheen on every layered material, times the model's KHR_materials_sheen colour (0 = off, 1 = as authored)"))?,
        clearcoat: ctx.param_f32(strength(CLEARCOAT_STRENGTH, "Clear coat on every layered material, times the model's KHR_materials_clearcoat factor"))?,
        anisotropy: ctx.param_f32(strength(ANISOTROPY_STRENGTH, "Brushed-metal stretch of highlights, times the model's KHR_materials_anisotropy strength"))?,
        iridescence: ctx.param_f32(strength(IRIDESCENCE_STRENGTH, "Thin-film rainbow colours, times the model's KHR_materials_iridescence factor (Medium tier and up)"))?,
        weave: ctx.param_f32(strength(WEAVE_STRENGTH, "Woven or knitted detail on cloth materials (extras.evani_cloth.weave)"))?,
        weave_scale: ctx.param_f32(
            ParamDef::f32(WEAVE_SCALE, 1.0)
                .range(0.1, 8.0)
                .step(0.01)
                .unit("x")
                .describe("Weave repeats, times each cloth material's own scale (higher = finer threads)")
                .gpu(),
        )?,
        quality: ctx.param_enum(
            ParamDef::enumeration(QUALITY, &["auto", "cheap", "full"], 0)
                .describe("Layered material path: auto follows the tier (Lite and Low cheap), cheap = no iridescence and a simple brushed-metal band, full = everything")
                .gpu(),
        )?,
    };
    ctx.resource(p);
    Ok(())
}

/// The shared per-frame uniform (`LayerFrame`): sheen, clear coat,
/// anisotropy and iridescence strengths (iridescence 0 on the cheap path),
/// then weave strength, weave scale, the path (0 cheap, 1 full) and 0.
pub fn frame_words(reg: &ParamRegistry, p: &LayerParams, tier: &str) -> [f32; FRAME_WORDS] {
    let q = Quality::pick(reg.enum_index(p.quality), tier);
    let full = q == Quality::Full;
    [
        reg.f32(p.sheen),
        reg.f32(p.clearcoat),
        reg.f32(p.anisotropy),
        if full { reg.f32(p.iridescence) } else { 0.0 },
        reg.f32(p.weave),
        reg.f32(p.weave_scale),
        if full { 1.0 } else { 0.0 },
        0.0,
    ]
}

/// The frame words with every strength at 1 on `tier` (no registry yet).
pub fn default_frame(tier: &str) -> [f32; FRAME_WORDS] {
    let full = Quality::for_tier(tier) == Quality::Full;
    [
        1.0,
        1.0,
        1.0,
        if full { 1.0 } else { 0.0 },
        1.0,
        1.0,
        if full { 1.0 } else { 0.0 },
        0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::ParamValue;
    use evani_core::plugin::{EngineModule, PluginError};

    struct Probe;
    impl EngineModule for Probe {
        fn id(&self) -> &'static str {
            "evani.module.layers_probe"
        }
        fn label(&self) -> &'static str {
            "layers"
        }
        fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
            register(ctx)?;
            Ok(())
        }
    }

    #[test]
    fn tiers_pick_the_path() {
        for t in ["lite", "low"] {
            assert_eq!(Quality::for_tier(t), Quality::Cheap, "{t}");
        }
        for t in ["medium", "high", "ultra", "vr"] {
            assert_eq!(Quality::for_tier(t), Quality::Full, "{t}");
        }
        assert_eq!(Quality::pick(2, "lite"), Quality::Full, "forced full");
        assert_eq!(Quality::pick(1, "ultra"), Quality::Cheap, "forced cheap");
        assert_eq!(default_frame("low")[3], 0.0, "no iridescence on phones");
        assert_eq!(default_frame("high")[3], 1.0);
    }

    #[test]
    fn registers_live_params_that_feed_the_frame_uniform() {
        let mut app = App::new("t").with_module(Probe);
        app.run_headless(1, 16.0);
        let p = *app.world.res::<LayerParams>();
        let mut reg = app.world.res_mut::<ParamRegistry>();
        for n in NAMES {
            assert!(reg.id(n).is_some(), "{n}");
        }
        assert_eq!(frame_words(&reg, &p, "medium"), default_frame("medium"));
        assert_eq!(frame_words(&reg, &p, "lite"), default_frame("lite"));
        reg.set_by_name(SHEEN_STRENGTH, ParamValue::F32(0.25))
            .unwrap();
        reg.set_by_name(IRIDESCENCE_STRENGTH, ParamValue::F32(1.5))
            .unwrap();
        reg.set_by_name(QUALITY, ParamValue::Enum(2)).unwrap();
        let w = frame_words(&reg, &p, "lite");
        assert_eq!(
            (w[0], w[3], w[6]),
            (0.25, 1.5, 1.0),
            "forced full keeps iridescence on Lite"
        );
        let _ = reg.set_by_name(CLEARCOAT_STRENGTH, ParamValue::F32(9.0));
        assert!(reg.f32(p.clearcoat) <= 2.0, "strengths are capped at 2");
    }
}
