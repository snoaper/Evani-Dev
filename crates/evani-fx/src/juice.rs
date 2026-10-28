//! Damage numbers and tweens (feature `juice`): no render pass, params
//! only. Games read `EffectsStatus::is_on("juice")` and these params to add
//! squash and stretch, pop-and-spin tweens, floating score text and combo
//! callouts; off, things just move and vanish. Cost 0 (the catalog's
//! "feel" group: CPU tweens, pooled text).

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, RenderGraph};

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "juice",
    cost: 0,
    value: 0.5,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: false,
    description: "Squash and stretch, pops, floating score and callouts",
};

/// Defaults (also used when a param is missing).
pub const POP_SCALE: f32 = 1.3;
pub const FLOAT_RISE: f32 = 0.9;

#[derive(Debug, Clone, Copy)]
pub struct JuiceParams {
    pub pop_scale: F32Param,
    pub float_rise: F32Param,
}

#[derive(Debug, Default)]
pub struct Juice {
    pub params: Option<JuiceParams>,
}

fn f(name: &str, d: f32, min: f32, max: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(0.01)
        .unit(unit)
        .describe(desc)
        .owner("fx.juice")
        .bake(BakeTarget::Look)
}

impl Effect for Juice {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(JuiceParams {
            pop_scale: ctx.param_f32(f(
                "fx.juice.pop_scale",
                POP_SCALE,
                1.0,
                2.5,
                "x",
                "How big a cleared thing swells before it pops.",
            ))?,
            float_rise: ctx.param_f32(f(
                "fx.juice.float_rise",
                FLOAT_RISE,
                0.0,
                4.0,
                "cells",
                "How far floating score text rises over its life.",
            ))?,
        });
        Ok(())
    }

    /// No passes: the game animates its own sprites and text.
    fn passes(&self, _g: &mut RenderGraph) -> Result<(), GraphError> {
        Ok(())
    }
}

impl PostEffect for Juice {
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
    fn free_params_only_and_kept_in_vr() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Juice::default()));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("juice"));
        assert_eq!(a.world.res::<BudgetStatus>().used, 0);
        let reg = a.world.res::<ParamRegistry>();
        let h = reg.f32_handle("fx.juice.pop_scale").unwrap();
        assert_eq!(reg.f32(h), POP_SCALE);
        drop(reg);
        a.world.res_mut::<Tiers>().set_xr_session(true);
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("juice"), "tweens are comfortable in VR");
    }
}
