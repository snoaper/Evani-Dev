//! Physics module (`evani.module.physics`). M1 stub: it compiles, registers
//! its tunables and reports in the overlay, so `physics = "simple" | "rigid"`
//! in evani.toml is a real build switch from day one. The solvers come later.

use core::fmt::Write; #[cfg(feature = "collide")] pub mod collide;

use evani_core::ecs::World;
use evani_core::params::{BakeTarget, I32Param, ParamDef, Vec3Param};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};

/// Which backend this build contains. `rigid` wins if both features are on.
pub const BACKEND: &str = if cfg!(feature = "rigid") {
    "rigid"
} else if cfg!(feature = "simple") {
    "simple"
} else {
    "none"
};

/// World resource: the active backend's settings, read by the solver.
#[derive(Debug, Clone, Copy)]
pub struct PhysicsParams {
    pub gravity: Vec3Param,
    pub substeps: I32Param,
}

#[derive(Debug, Default)]
pub struct PhysicsModule {
    params: Option<PhysicsParams>,
}

impl PhysicsModule {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EngineModule for PhysicsModule {
    fn id(&self) -> &'static str {
        "evani.module.physics"
    }

    fn label(&self) -> &'static str {
        "physics"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = PhysicsParams {
            gravity: ctx.param_vec3(
                ParamDef::vec3("physics.gravity", [0.0, -9.81, 0.0])
                    .range(-50.0, 50.0)
                    .unit("m/s²")
                    .describe("World gravity.")
                    .owner("physics")
                    .bake(BakeTarget::Kit),
            )?,
            substeps: ctx.param_i32(
                ParamDef::i32("physics.substeps", 1)
                    .range(1.0, 8.0)
                    .unit("steps/tick")
                    .describe("Solver substeps per 60 Hz simulation tick.")
                    .owner("physics")
                    .bake(BakeTarget::Kit),
            )?,
        };
        ctx.resource(p); #[cfg(feature = "collide")] collide::params::register(ctx)?;
        self.params = Some(p);
        Ok(())
    }

    fn status(&self, world: &World, out: &mut String) {
        out.push_str(BACKEND);
        out.push_str(" backend (stub)"); #[cfg(feature = "collide")] out.push_str(" · collide");
        if let Some(p) = self.params {
            let reg = world.res::<evani_core::params::ParamRegistry>();
            let _ = write!(out, " · substeps {}", reg.i32(p.substeps));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::ParamRegistry;

    #[test]
    fn registers_and_reports() {
        let mut a = App::new("t").with_module(PhysicsModule::new());
        a.run_headless(1, 16.0);
        assert!(a.world.res::<ParamRegistry>().id("physics.gravity").is_some());
        assert!(a.world.has_resource::<PhysicsParams>());
        let m = PhysicsModule::new();
        let mut s = String::new();
        m.status(&a.world, &mut s);
        assert!(s.starts_with(BACKEND));
    }

    #[test]
    fn rigid_wins() {
        if cfg!(feature = "rigid") {
            assert_eq!(BACKEND, "rigid");
        } else if cfg!(feature = "simple") {
            assert_eq!(BACKEND, "simple");
        }
    }
}
