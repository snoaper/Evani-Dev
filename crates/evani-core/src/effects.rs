//! Effect metadata and the effect points budget calculator, shared by
//! evani-fx (which implements the effects), looks, tiers, the overlay and
//! evani-live. Lives in core so a game built without evani-fx can still
//! describe its budget, and so every tunable stays reachable from core.

use serde::{Deserialize, Serialize};

use crate::params::{BoolParam, F32Param};

/// Which art-style classes an effect works with (bit set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct StyleClasses(pub u8);

impl StyleClasses {
    pub const TWO_D: Self = Self(1);
    pub const PSEUDO_3D: Self = Self(2);
    pub const THREE_D: Self = Self(4);
    pub const ALL: Self = Self(7);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn names(self) -> impl Iterator<Item = &'static str> {
        [(1u8, "2d"), (2, "pseudo-3d"), (4, "3d")]
            .into_iter()
            .filter(move |(bit, _)| self.0 & bit != 0)
            .map(|(_, n)| n)
    }
}

/// Static description of an effect.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectInfo {
    /// Short id, also the Cargo feature name: `bloom`, `vignette`.
    pub name: &'static str,
    /// Budget cost in points, 0..=3.
    pub cost: u8,
    /// Default visual value weight (the live value is a registered param,
    /// `fx.<name>.value_weight`, so an AI can retune degradation order).
    pub value: f32,
    pub styles: StyleClasses,
    /// Forced off in VR for comfort.
    pub vr_comfort_off: bool,
    pub description: &'static str,
}

impl EffectInfo {
    /// Value per point; cost-0 effects are free and never need switching off.
    pub fn value_per_point(&self, value: f32) -> f32 {
        if self.cost == 0 {
            f32::INFINITY
        } else {
            value / f32::from(self.cost)
        }
    }
}

/// Budget usage, written by evani-fx (or by the app from the look alone when
/// fx is compiled out) and shown by the overlay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BudgetStatus {
    pub used: u32,
    pub total: u32,
}

/// One compiled-in effect and what the budget made of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectStatus {
    pub info: &'static EffectInfo,
    /// `fx.<name>.enabled`: the player's / tier's wish.
    pub enabled: Option<BoolParam>,
    /// `fx.<name>.value_weight`: drives the budget order.
    pub weight: Option<F32Param>,
    /// Render-graph passes the effect adds (toggled together).
    pub passes: &'static [&'static str],
    pub state: EffectState,
}

impl EffectStatus {
    pub fn name(&self) -> &'static str {
        self.info.name
    }
}

/// World resource written by evani-fx: every effect compiled into this
/// build. Lives in core so the debug panel, overlay and evani-live can list
/// effects without depending on evani-fx (absent when a game lists none).
#[derive(Debug, Clone, Default)]
pub struct EffectsStatus {
    pub effects: Vec<EffectStatus>,
}

impl EffectsStatus {
    pub fn get(&self, name: &str) -> Option<&EffectStatus> {
        self.effects.iter().find(|e| e.info.name == name)
    }

    pub fn is_on(&self, name: &str) -> bool {
        self.get(name).is_some_and(|e| e.state.is_on())
    }
}

/// What the player, tier or look asks for one effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectWant {
    Off,
    /// On if the budget allows.
    On,
    /// On regardless of budget (live tuning, explicit override). Still
    /// subject to style and VR comfort rules.
    Forced,
}

/// Outcome for one effect after [`plan_budget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffectState {
    On,
    #[default]
    OffByRequest,
    OffByBudget,
    /// The look's style classes don't include any the effect supports.
    OffByStyle,
    /// The tier is stereo (VR) and the effect is comfort-off.
    OffByVrComfort,
    /// Wanted on, but paused by the adaptive-quality controller to hold the frame
    /// rate; it comes back when there is headroom again.
    OffByQuality,
}

impl EffectState {
    pub fn is_on(self) -> bool {
        self == EffectState::On
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EffectState::On => "on",
            EffectState::OffByRequest => "off",
            EffectState::OffByBudget => "off (budget)",
            EffectState::OffByStyle => "off (style)",
            EffectState::OffByVrComfort => "off (vr comfort)",
            EffectState::OffByQuality => "on, paused to hold frame rate",
        }
    }
}

/// One compiled-in effect as seen by the budget calculator.
#[derive(Debug, Clone, Copy)]
pub struct BudgetItem<'a> {
    pub info: &'a EffectInfo,
    /// Live value weight (the `fx.<name>.value_weight` param).
    pub value: f32,
    pub want: EffectWant,
}

/// The effect points budget: decide which effects run on a tier.
///
/// 1. Effects not wanted, not matching the look's style classes, or
///    comfort-off on a stereo tier are off.
/// 2. While look points plus effect costs exceed the tier's points, the
///    wanted (not forced) effect with the lowest value per point is switched
///    off. Ties: the more expensive one goes first, then the later one in
///    `items` (deterministic; no hashing).
///
/// Writes one state per item into `out` (same length) and returns the
/// points used. No allocation.
pub fn plan_budget(
    tier_points: u32,
    stereo: bool,
    look_points: u32,
    look_styles: StyleClasses,
    items: &[BudgetItem],
    out: &mut [EffectState],
) -> BudgetStatus {
    let mut used = look_points;
    for (item, state) in items.iter().zip(out.iter_mut()) {
        *state = if item.want == EffectWant::Off {
            EffectState::OffByRequest
        } else if item.info.styles.0 & look_styles.0 == 0 {
            EffectState::OffByStyle
        } else if stereo && item.info.vr_comfort_off {
            EffectState::OffByVrComfort
        } else {
            used += u32::from(item.info.cost);
            EffectState::On
        };
    }
    while used > tier_points {
        let mut victim: Option<(usize, f32, u8)> = None;
        for (i, (item, state)) in items.iter().zip(out.iter()).enumerate() {
            if !state.is_on() || item.want == EffectWant::Forced || item.info.cost == 0 {
                continue;
            }
            let vpp = item.info.value_per_point(item.value.max(0.0));
            let better = match victim {
                None => true,
                Some((_, v, c)) => vpp < v || (vpp == v && item.info.cost >= c),
            };
            if better {
                victim = Some((i, vpp, item.info.cost));
            }
        }
        let Some((i, _, cost)) = victim else { break };
        out[i] = EffectState::OffByBudget;
        used -= u32::from(cost);
    }
    BudgetStatus {
        used,
        total: tier_points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_bits() {
        let s = StyleClasses::TWO_D.union(StyleClasses::THREE_D);
        assert!(s.contains(StyleClasses::TWO_D));
        assert!(!s.contains(StyleClasses::PSEUDO_3D));
        assert_eq!(s.names().collect::<Vec<_>>(), ["2d", "3d"]);
        assert!(StyleClasses::ALL.contains(s));
    }

    const BLOOM: EffectInfo = EffectInfo {
        name: "bloom",
        cost: 3,
        value: 0.8,
        styles: StyleClasses(StyleClasses::THREE_D.0 | StyleClasses::PSEUDO_3D.0),
        vr_comfort_off: false,
        description: "",
    };
    const VIGNETTE: EffectInfo = EffectInfo {
        name: "vignette",
        cost: 1,
        value: 0.3,
        styles: StyleClasses::ALL,
        vr_comfort_off: true,
        description: "",
    };
    const GRAIN: EffectInfo = EffectInfo {
        name: "grain",
        cost: 2,
        value: 0.2,
        styles: StyleClasses::ALL,
        vr_comfort_off: false,
        description: "",
    };

    fn plan(
        points: u32,
        stereo: bool,
        look: u32,
        styles: StyleClasses,
        items: &[BudgetItem],
    ) -> (BudgetStatus, Vec<EffectState>) {
        let mut out = vec![EffectState::default(); items.len()];
        let s = plan_budget(points, stereo, look, styles, items, &mut out);
        (s, out)
    }

    fn item(info: &EffectInfo, want: EffectWant) -> BudgetItem<'_> {
        BudgetItem {
            info,
            value: info.value,
            want,
        }
    }

    #[test]
    fn everything_fits() {
        let items = [
            item(&BLOOM, EffectWant::On),
            item(&VIGNETTE, EffectWant::On),
        ];
        let (s, out) = plan(16, false, 4, StyleClasses::THREE_D, &items);
        assert_eq!(s, BudgetStatus { used: 8, total: 16 });
        assert_eq!(out, [EffectState::On, EffectState::On]);
    }

    #[test]
    fn lowest_value_per_point_goes_first() {
        // bloom 0.8/3 = 0.267, vignette 0.3/1 = 0.3, grain 0.2/2 = 0.1
        let items = [
            item(&BLOOM, EffectWant::On),
            item(&VIGNETTE, EffectWant::On),
            item(&GRAIN, EffectWant::On),
        ];
        let (s, out) = plan(7, false, 2, StyleClasses::THREE_D, &items);
        // 2 + 3 + 1 + 2 = 8 > 7 -> grain off -> 6.
        assert_eq!(
            out,
            [EffectState::On, EffectState::On, EffectState::OffByBudget]
        );
        assert_eq!(s.used, 6);
        // Tighter: 5 points -> grain, then bloom off.
        let (s, out) = plan(5, false, 2, StyleClasses::THREE_D, &items);
        assert_eq!(
            out,
            [
                EffectState::OffByBudget,
                EffectState::On,
                EffectState::OffByBudget
            ]
        );
        assert_eq!(s.used, 3);
    }

    #[test]
    fn weights_change_the_order() {
        let mut items = [
            item(&BLOOM, EffectWant::On),
            item(&VIGNETTE, EffectWant::On),
        ];
        items[0].value = 0.6; // 0.2 per point < vignette's 0.3
        let (_, out) = plan(3, false, 0, StyleClasses::THREE_D, &items);
        assert_eq!(out, [EffectState::OffByBudget, EffectState::On]);
        items[0].value = 3.0; // 1.0 per point > 0.3
        let (_, out) = plan(3, false, 0, StyleClasses::THREE_D, &items);
        assert_eq!(out, [EffectState::On, EffectState::OffByBudget]);
    }

    #[test]
    fn style_vr_forced_and_overflow() {
        let items = [
            item(&BLOOM, EffectWant::On),
            item(&VIGNETTE, EffectWant::On),
        ];
        // 2D look: bloom (3d/pseudo-3d only) is off by style.
        let (s, out) = plan(16, false, 0, StyleClasses::TWO_D, &items);
        assert_eq!(out, [EffectState::OffByStyle, EffectState::On]);
        assert_eq!(s.used, 1);
        // VR: vignette is comfort-off.
        let (_, out) = plan(9, true, 0, StyleClasses::THREE_D, &items);
        assert_eq!(out, [EffectState::On, EffectState::OffByVrComfort]);
        // Forced stays on even over budget; the look alone may overflow.
        let items = [
            item(&BLOOM, EffectWant::Forced),
            item(&VIGNETTE, EffectWant::Off),
        ];
        let (s, out) = plan(2, false, 4, StyleClasses::THREE_D, &items);
        assert_eq!(out, [EffectState::On, EffectState::OffByRequest]);
        assert_eq!(s, BudgetStatus { used: 7, total: 2 });
    }

    #[test]
    fn value_per_point() {
        let mut e = EffectInfo {
            name: "x",
            cost: 2,
            value: 1.0,
            styles: StyleClasses::ALL,
            vr_comfort_off: false,
            description: "",
        };
        assert_eq!(e.value_per_point(1.0), 0.5);
        e.cost = 0;
        assert!(e.value_per_point(1.0).is_infinite());
    }
}
