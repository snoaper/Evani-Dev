//! The test hooks' param writes (feature `hooks`, dev builds only):
//! `window.__engine.paramSet` (gpu/hooks.rs) queues each value here, already
//! in its param's own kind (GPU code only reads the registry: the frame
//! holds it), and this system sets them at the next update.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;

/// Values the hooks asked for, waiting for the next update.
#[derive(Debug, Default)]
pub struct ParamSets(pub Vec<(String, ParamValue)>);

pub fn register(ctx: &mut PluginContext) {
    ctx.resource(ParamSets::default());
    ctx.system(SystemDesc::new("render.hooks.param_set", Stage::Update, apply));
}

fn apply(world: &mut World) {
    let sets = world.try_res_mut::<ParamSets>().map(|mut s| std::mem::take(&mut s.0)).unwrap_or_default();
    if sets.is_empty() {
        return;
    }
    let mut reg = world.res_mut::<ParamRegistry>();
    for (name, v) in sets {
        if let Some(id) = reg.id(&name) {
            let _ = reg.set(id, v);
        }
    }
}
