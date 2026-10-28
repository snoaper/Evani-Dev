//! Dynamic shadows (feature `shadows`, catalog `shadows`, 2 points, 3D):
//! one directional shadow map. The drawing lives in the renderer (feature
//! `evani-render/shadows`, turned on with this one): a depth-only `shadows`
//! pass from the light, then a PCF lookup in the forward pass. This effect is
//! the budgeted switch and the tunables; the renderer draws the map while
//! `EffectsStatus::is_on("shadows")` and the active tier gives a
//! `shadow_map` size (Low gives none: kits' blob shadows ground things
//! there, and whenever this is off).

use evani_core::params::{BakeTarget, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, RenderGraph};

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "shadows",
    cost: 2,
    value: 0.9,
    // Catalog works_with: 3d.
    styles: StyleClasses::THREE_D,
    vr_comfort_off: false,
    description: "Directional shadow map (blob shadows when off)",
};

/// Param names the renderer reads (it has no dependency on this crate).
pub const DISTANCE: &str = "fx.shadows.distance";
pub const STRENGTH: &str = "fx.shadows.strength";
pub const BIAS: &str = "fx.shadows.bias";
pub const NORMAL_OFFSET: &str = "fx.shadows.normal_offset";

#[derive(Debug, Default)]
pub struct Shadows;

fn f(name: &str, d: f32, min: f32, max: f32, step: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(step)
        .unit(unit)
        .describe(desc)
        .owner("fx.shadows")
        .bake(BakeTarget::Look)
        .gpu()
}

impl Effect for Shadows {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        ctx.param_f32(f(
            DISTANCE,
            evani_render::shadow::DISTANCE,
            4.0,
            200.0,
            0.5,
            "m",
            "Half the width of the shadowed area around the camera's target (smaller = sharper).",
        ))?;
        ctx.param_f32(f(
            STRENGTH,
            evani_render::shadow::STRENGTH,
            0.0,
            1.0,
            0.01,
            "",
            "How dark shadows are (1 = the look's darkest band).",
        ))?;
        ctx.param_f32(f(
            BIAS,
            evani_render::shadow::BIAS,
            0.0,
            0.02,
            0.0001,
            "depth",
            "Depth bias against shadow acne (too high and shadows detach).",
        ))?;
        ctx.param_f32(f(
            NORMAL_OFFSET,
            evani_render::shadow::NORMAL_OFFSET,
            0.0,
            0.5,
            0.005,
            "m",
            "Lookup moved along the surface normal against acne on slopes.",
        ))?;
        Ok(())
    }

    /// No graph pass: the renderer runs its depth pass before the graph.
    fn passes(&self, _g: &mut RenderGraph) -> Result<(), GraphError> {
        Ok(())
    }
}

impl PostEffect for Shadows {
    fn name(&self) -> &'static str {
        INFO.name
    }

    fn pass_names(&self) -> &'static [&'static str] {
        &[]
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &[]
    }

    fn write_uniforms(&self, _: &str, _: &ParamRegistry, _: &UniformFrame, _: &mut [f32]) -> usize {
        0
    }

    fn record(&self, _: &str, _: &ParamRegistry, _: &mut dyn PostRecorder) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FxModule, FxStatus};
    use evani_core::App;
    use evani_core::effects::BudgetStatus;
    use evani_core::tiers::Tiers;
    use evani_render::RenderModule;

    #[test]
    fn costs_two_points_and_follows_the_tier() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Shadows));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(!a.world.res::<FxStatus>().is_on("shadows"), "Low: blob shadows only");
        for tier in ["medium", "high", "ultra", "vr"] {
            // VR is only selectable inside an XR session; the preset run is the bench's way in.
            let set = if tier == "vr" {
                a.world.res_mut::<Tiers>().set_active_preset_run(tier)
            } else {
                a.world.res_mut::<Tiers>().set_active(tier)
            };
            assert!(set, "{tier}");
            a.run_headless(1, 16.0);
            assert!(a.world.res::<FxStatus>().is_on("shadows"), "{tier}");
            assert_eq!(a.world.res::<BudgetStatus>().used, 2, "{tier}");
            assert!(a.world.res::<Tiers>().active().shadow_map.is_some(), "{tier}");
        }
        let reg = a.world.res::<ParamRegistry>();
        for n in [DISTANCE, STRENGTH, BIAS, NORMAL_OFFSET] {
            assert!(reg.f32_handle(n).is_some(), "{n}");
        }
    }
}
