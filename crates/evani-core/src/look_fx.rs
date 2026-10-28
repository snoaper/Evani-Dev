//! Looks asking for an effect to be on (ChimpDuty intake, limit 2).
//!
//! By default a look can only veto an effect (`LookContext::post_effect(name,
//! false)`); which effects start on is the quality tier's `effects_on` list.
//! A look may also *request* an effect on with
//! [`LookContext::request_effect`]. The request is only read by evani-fx
//! when the game turns on its `look-effects` feature, and only for an effect
//! compiled into the game. The tier still decides on cost:
//!
//! 1. the tier must have at least the request's minimum points (default
//!    [`DEFAULT_MIN_POINTS`] = 9: VR, Medium, High and Ultra; Low has 7 and
//!    Lite 5, so they veto it);
//! 2. a look that also prefers the effect off wins (the veto is applied
//!    after the request);
//! 3. the effect then goes through the tier's budget like any other: it is
//!    wanted on, never forced, so a tight budget, the VR comfort rule or the
//!    adaptive-quality controller can still turn it off.
//!
//! Nothing here is stored in `LookData` or a tier, so a game whose look
//! never calls it carries none of this code.

use crate::plugin::LookContext;
use crate::tiers::TierPreset;

/// The minimum tier points of a plain [`LookContext::request_effect`].
pub const DEFAULT_MIN_POINTS: u32 = 9;

/// One look request: `effect` on where the tier has at least `min_points`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRequest {
    pub effect: String,
    pub min_points: u32,
}

/// The active look's effect requests. World resource, made by the first
/// request.
#[derive(Debug, Clone, Default)]
pub struct LookEffectRequests {
    pub requests: Vec<EffectRequest>,
}

impl LookEffectRequests {
    /// Whether the look asks for `effect` on under `tier` (rule 1; the
    /// caller applies the look's veto and the budget).
    pub fn wants(&self, effect: &str, tier: &TierPreset) -> bool {
        self.requests.iter().rev().find(|r| r.effect == effect).is_some_and(|r| tier.points >= r.min_points)
    }
}

impl LookContext<'_, '_> {
    /// Ask for `effect` (`"fog"`, `"grade"`, `"decals"`, ...) on in tiers of
    /// at least [`DEFAULT_MIN_POINTS`] points. Read only with evani-fx's
    /// `look-effects` feature, and only if the effect is compiled in; the
    /// budget still decides (see the module docs).
    pub fn request_effect(&mut self, effect: &str) {
        self.request_effect_from(effect, DEFAULT_MIN_POINTS);
    }

    /// [`Self::request_effect`] with the tier points the effect needs (0:
    /// every tier, the budget permitting). A later request for the same
    /// effect replaces an earlier one.
    pub fn request_effect_from(&mut self, effect: &str, min_points: u32) {
        let req = EffectRequest { effect: effect.to_string(), min_points };
        if let Some(mut r) = self.ctx.world.try_res_mut::<LookEffectRequests>() {
            r.requests.push(req);
            return;
        }
        self.ctx.resource(LookEffectRequests { requests: vec![req] });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiers::Tiers;

    #[test]
    fn requests_follow_tier_points() {
        let tiers = Tiers::builtin().unwrap();
        let tier = |id: &str| tiers.presets().iter().find(|t| t.id == id).unwrap().clone();
        let mut r = LookEffectRequests::default();
        r.requests.push(EffectRequest { effect: "fog".into(), min_points: DEFAULT_MIN_POINTS });
        for (id, on) in [("lite", false), ("low", false), ("vr", true), ("medium", true), ("high", true), ("ultra", true)] {
            assert_eq!(r.wants("fog", &tier(id)), on, "{id}");
        }
        assert!(!r.wants("grade", &tier("ultra")));
        // A later request replaces an earlier one.
        r.requests.push(EffectRequest { effect: "fog".into(), min_points: 0 });
        assert!(r.wants("fog", &tier("lite")));
    }
}
