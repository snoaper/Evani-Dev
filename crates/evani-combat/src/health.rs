//! Health and armour.
//!
//! Armour takes the share of a hit that does not penetrate
//! (`hp = dmg * pen`), losing half of that share itself; when the armour
//! runs out the rest comes off health. Legs are never armoured, the head
//! only with a helmet.

use evani_core::num::clamp;
use evani_physics::collide::math::fmin;

use crate::tune::WeaponStats;

/// Where a hit landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Head,
    Chest,
    Stomach,
    Legs,
}

impl Zone {
    /// The damage multiplier a gun gives this zone.
    pub fn mult(self, w: &WeaponStats) -> f32 {
        match self {
            Zone::Head => w.head_mult,
            Zone::Chest => 1.0,
            Zone::Stomach => w.stomach_mult,
            Zone::Legs => w.leg_mult,
        }
    }
}

/// What a hit did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Damage {
    /// Health taken.
    pub hp: f32,
    /// Armour taken.
    pub armour: f32,
    /// This hit killed.
    pub killed: bool,
}

/// One body's health.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    pub hp: f32,
    pub armour: f32,
    pub helmet: bool,
}

/// Armour lost per point of damage it stops.
const ARMOUR_BONUS: f32 = 0.5;

impl Health {
    pub fn new(hp: f32, armour: f32, helmet: bool) -> Self {
        Health { hp, armour, helmet }
    }

    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }

    /// Take `dmg` (zone multiplier already applied) of a weapon with armour
    /// penetration `pen` in `zone`. A dead body takes nothing.
    pub fn take(&mut self, dmg: f32, pen: f32, zone: Zone) -> Damage {
        if !self.alive() || dmg <= 0.0 {
            return Damage {
                hp: 0.0,
                armour: 0.0,
                killed: false,
            };
        }
        let covered = self.armour > 0.0
            && match zone {
                Zone::Head => self.helmet,
                Zone::Legs => false,
                Zone::Chest | Zone::Stomach => true,
            };
        let (mut hp, mut armour) = (dmg, 0.0);
        if covered {
            hp = dmg * clamp(pen, 0.0, 1.0);
            armour = (dmg - hp) * ARMOUR_BONUS;
            if armour > self.armour {
                armour = self.armour;
                hp = dmg - armour / ARMOUR_BONUS;
            }
        }
        let hp = fmin(hp, self.hp);
        self.armour -= armour;
        self.hp -= hp;
        if self.hp < 0.01 {
            self.hp = 0.0;
        }
        Damage {
            hp,
            armour,
            killed: !self.alive(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tune::Tuning;

    #[test]
    fn armour_reduces_damage() {
        let r = Tuning::default().rifle;
        let mut bare = Health::new(100.0, 0.0, false);
        let mut kev = Health::new(100.0, 100.0, true);
        let a = bare.take(r.damage, r.armor_pen, Zone::Chest);
        let b = kev.take(r.damage, r.armor_pen, Zone::Chest);
        assert_eq!(a.hp, 36.0);
        assert!((b.hp - 36.0 * 0.775).abs() < 1e-4, "{b:?}");
        assert!((b.armour - 36.0 * 0.225 * 0.5).abs() < 1e-4);
        assert!(kev.hp > bare.hp);
    }

    #[test]
    fn legs_ignore_armour_and_head_needs_a_helmet() {
        let mut h = Health::new(100.0, 100.0, false);
        assert_eq!(h.take(20.0, 0.5, Zone::Legs).hp, 20.0);
        assert_eq!(h.take(20.0, 0.5, Zone::Head).hp, 20.0);
        h.helmet = true;
        assert_eq!(h.take(20.0, 0.5, Zone::Head).hp, 10.0);
    }

    #[test]
    fn spent_armour_lets_the_rest_through_and_kills_once() {
        let mut h = Health::new(100.0, 2.0, false);
        let d = h.take(40.0, 0.5, Zone::Chest);
        // 2 armour stops 4 damage.
        assert!((d.hp - 36.0).abs() < 1e-4 && d.armour == 2.0 && h.armour == 0.0);
        let k = h.take(500.0, 0.5, Zone::Chest);
        assert!(k.killed && h.hp == 0.0 && (k.hp - 64.0).abs() < 1e-4);
        assert!(!h.take(10.0, 0.5, Zone::Chest).killed);
    }
}
