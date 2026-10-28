//! Feature `look-effects` (ChimpDuty intake, limit 2): an effect the look
//! asked for with `LookContext::request_effect` starts on where the tier has
//! the points for it. The rule is in [`evani_core::look_fx`]: tier points
//! first, then the look's own veto, then the budget.

use evani_core::ecs::World;
use evani_core::look_fx::LookEffectRequests;
use evani_core::tiers::TierPreset;

/// Whether the look's requests turn `effect` on under `tier`.
pub(crate) fn requested(world: &World, effect: &str, tier: &TierPreset) -> bool {
    world.try_res::<LookEffectRequests>().is_some_and(|r| r.wants(effect, tier))
}

#[cfg(test)]
#[cfg(feature = "fog")]
mod tests {
    use crate::{FxModule, FxStatus};
    use evani_core::effects::StyleClasses;
    use evani_core::plugin::{LookContext, LookPlugin, PluginError};
    use evani_core::tiers::Tiers;
    use evani_core::App;
    use evani_render::RenderModule;

    /// A 3D look that asks for fog (or not), and may also veto it.
    struct Look {
        request: Option<u32>,
        veto: bool,
    }
    impl LookPlugin for Look {
        fn name(&self) -> &'static str {
            "t"
        }
        fn points(&self) -> u32 {
            0
        }
        fn styles(&self) -> StyleClasses {
            StyleClasses::THREE_D
        }
        fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
            if let Some(min) = self.request {
                look.request_effect_from("fog", min);
            }
            if self.veto {
                look.post_effect("fog", false);
            }
            Ok(())
        }
    }

    fn fog_on(look: Look, tier: &str) -> bool {
        let mut a = App::new("t");
        a.world.insert_resource(Tiers::builtin().unwrap());
        let mut a = a.with_look(look).with_module(RenderModule::new()).with_module(FxModule::new().with_effect(crate::fog::Fog::default()));
        assert!(a.world.res_mut::<Tiers>().set_active_preset_run(tier));
        a.run_headless(2, 16.0);
        a.world.res::<FxStatus>().is_on("fog")
    }

    #[test]
    fn a_look_turns_fog_on_where_the_tier_allows() {
        let min = evani_core::look_fx::DEFAULT_MIN_POINTS;
        // No tier lists fog: without a request it stays off.
        assert!(!fog_on(Look { request: None, veto: false }, "high"));
        assert!(fog_on(Look { request: Some(min), veto: false }, "high"));
        assert!(fog_on(Look { request: Some(min), veto: false }, "medium"));
        // Low (7 points) and Lite (5) veto the default request.
        assert!(!fog_on(Look { request: Some(min), veto: false }, "low"));
        assert!(!fog_on(Look { request: Some(min), veto: false }, "lite"));
        // A request from 0 points reaches Low; the look's own veto still wins.
        assert!(fog_on(Look { request: Some(0), veto: false }, "low"));
        assert!(!fog_on(Look { request: Some(0), veto: true }, "high"));
    }
}
