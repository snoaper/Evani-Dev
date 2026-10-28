//! Particles (feature `particles`): sparks, sparkles and confetti on the
//! shared GPU particle system. Cost 2, as in the catalog.
//!
//! Kits emit from their fixed step into `evani_core::particles::ParticleQueue`
//! (seeded `Pcg32` only); the feature turns on `evani-render/particles`,
//! which drains the queue into a ring of spawn records sized to the tier's
//! `max_particles` (lowered by `fx.particles.max`). This effect adds the
//! `particles.sim` compute pass that evaluates them and makes the forward
//! and sprites passes, which draw them, read its buffer. Toggled off (by
//! the budget or live), the pass is disabled: emits are dropped and nothing
//! draws, and the game itself never changes. `fx.particles.density` and
//! `fx.particles.gravity` are hints a kit may scale its emits by.

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph, TargetDesc, TargetKind};
use evani_render::particles::{BUFFER_TARGET, SIM_PASS};

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "particles",
    cost: 2,
    value: 0.6,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: false,
    description: "Sparks, sparkles and confetti",
};

/// Defaults (also used when a param is missing).
pub const MAX: f32 = 600.0;
pub const DENSITY: f32 = 1.0;
pub const GRAVITY: f32 = 14.0;

#[derive(Debug, Clone, Copy)]
pub struct ParticleParams {
    pub max: F32Param,
    pub density: F32Param,
    pub gravity: F32Param,
}

#[derive(Debug, Default)]
pub struct Particles {
    pub params: Option<ParticleParams>,
}

fn f(name: &str, d: f32, min: f32, max: f32, step: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(step)
        .unit(unit)
        .describe(desc)
        .owner("fx.particles")
        .bake(BakeTarget::Look)
}

impl Effect for Particles {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(ParticleParams {
            max: ctx.param_f32(f(
                "fx.particles.max",
                MAX,
                0.0,
                250_000.0,
                1.0,
                "count",
                "Most particles alive at once (the tier's max_particles caps it too).",
            ))?,
            density: ctx.param_f32(f(
                "fx.particles.density",
                DENSITY,
                0.0,
                4.0,
                0.05,
                "x",
                "Particles per burst, as a multiple of the game's default.",
            ))?,
            gravity: ctx.param_f32(f(
                "fx.particles.gravity",
                GRAVITY,
                -40.0,
                80.0,
                0.5,
                "units/s2",
                "Downward pull on particles, in the game's own units (cells for a board game).",
            ))?,
        });
        Ok(())
    }

    /// The `particles.sim` compute pass writes the instance buffer the
    /// forward pass (world particles) and the sprites pass (screen
    /// particles) draw from, so the graph runs it before both.
    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        g.add_target(TargetDesc::new(BUFFER_TARGET, TargetKind::Buffer))?;
        g.add_pass(PassDesc::compute(SIM_PASS, "fx.particles").writes(&[BUFFER_TARGET]))?;
        for pass in ["forward", "sprites"] {
            g.add_input(pass, BUFFER_TARGET);
        }
        Ok(())
    }
}

impl PostEffect for Particles {
    fn name(&self) -> &'static str {
        INFO.name
    }

    fn pass_names(&self) -> &'static [&'static str] {
        &[SIM_PASS]
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
    fn costs_two_points_and_registers_its_cap() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Particles::default()));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("particles"));
        assert_eq!(a.world.res::<BudgetStatus>().used, 2);
        let reg = a.world.res::<ParamRegistry>();
        for (n, d) in [("fx.particles.max", MAX), ("fx.particles.density", DENSITY), ("fx.particles.gravity", GRAVITY)] {
            let h = reg.f32_handle(n).unwrap();
            assert_eq!(reg.f32(h), d, "{n}");
        }
        drop(reg);
        // Its compute pass runs before the passes that draw particles.
        let g = a.world.res::<RenderGraph>();
        assert!(g.pass(SIM_PASS).is_some_and(|p| p.enabled));
        let order: Vec<&str> = g.ordered().map(|p| p.name).collect();
        let at = |n: &str| order.iter().position(|p| *p == n).unwrap();
        assert!(at(SIM_PASS) < at("sprites"), "{order:?}");
        // The forward pass (world particles) exists only with the 3D path.
        assert_eq!(order.contains(&"forward"), evani_render::FORWARD_PATH, "{order:?}");
        if evani_render::FORWARD_PATH {
            assert!(at(SIM_PASS) < at("forward"), "{order:?}");
        }
        drop(g);
        assert!(a.world.res::<evani_render::particles::ParticleSystem>().is_on());
    }
}
