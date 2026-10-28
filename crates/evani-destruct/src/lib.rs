//! Destruction module (`evani.module.destruct`): Hexagon's destruction
//! mechanics rebuilt for Evani.Dev. See docs/DESTRUCTION.md.
//!
//! * [`HoleField`]: the damage model (hole spheres, capped, never drops a hit).
//! * [`Structure`]: floor grid + 1 m cover cells; support, tipping, falling,
//!   landing craters and floor islands, stepped at the fixed tick.
//! * [`Debris`]: the fixed, pooled, visual-only rubble and dust.
//! * [`DestructModule`]: registers the live `destruct.*` params
//!   ([`DestructParams`]); [`DestructParams::read`] gives the [`Tuning`].
//! * [`draw`]: helpers that push it all into the `RenderWorld`.
//!
//! Opt-in: a game that does not list the `destruct` feature compiles none of it.

pub mod debris;
pub mod draw;
pub mod holes;
pub mod params;
pub mod structure;
pub mod support;
pub mod tuning;

use core::fmt::Write;

use evani_core::ecs::World;
use evani_core::plugin::{EngineModule, PluginContext, PluginError};

pub use debris::Debris;
pub use holes::{Hole, HoleField};
pub use params::DestructParams;
pub use structure::{DestructEvent, Fall, FloorGrid, FloorKind, Structure, StructureDesc};
pub use tuning::Tuning;

/// The engine module: registers the `destruct.*` params and inserts the
/// [`DestructParams`] resource.
#[derive(Debug, Default)]
pub struct DestructModule {
    params: Option<DestructParams>,
}

impl DestructModule {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EngineModule for DestructModule {
    fn id(&self) -> &'static str {
        "evani.module.destruct"
    }

    fn label(&self) -> &'static str {
        "destruct"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = DestructParams::register(ctx)?;
        ctx.resource(p);
        self.params = Some(p);
        Ok(())
    }

    fn status(&self, world: &World, out: &mut String) {
        let _ = write!(out, "{} params", params::NAMES.len());
        if let Some(p) = self.params {
            let t = p.read(&world.res::<evani_core::params::ParamRegistry>());
            let _ = write!(out, " · fall max {}", t.fall_max);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::{ParamRegistry, ParamValue};

    #[test]
    fn registers_every_param_with_hexagons_defaults() {
        let mut a = App::new("t").with_module(DestructModule::new());
        a.run_headless(1, 16.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in params::NAMES {
            assert!(reg.id(n).is_some(), "{n} missing");
        }
        let p = *a.world.res::<DestructParams>();
        assert_eq!(p.read(&reg), Tuning::default());
    }

    #[test]
    fn a_param_change_reaches_the_tuning() {
        let mut a = App::new("t").with_module(DestructModule::new());
        a.run_headless(1, 16.0);
        let p = *a.world.res::<DestructParams>();
        let mut reg = a.world.res_mut::<ParamRegistry>();
        assert!(
            reg.set_by_name("destruct.debris_size_max", ParamValue::F32(0.9))
                .is_ok()
        );
        assert!(
            reg.set_by_name("destruct.fall_max", ParamValue::I32(3))
                .is_ok()
        );
        let t = p.read(&reg);
        assert_eq!((t.debris_size_max, t.fall_max), (0.9, 3));
    }
}
