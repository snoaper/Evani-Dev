//! A gun: magazine and reserve, fire rate, reload, recoil, spread, and
//! damage falloff over range.
//!
//! Spread is a cone sampled over a uniform disc (`r = sqrt(u) * cone`), so
//! most bullets land near the middle. The cone is small for the first shot
//! of a burst standing still, and grows with heat (shots in quick
//! succession), movement over walking speed and being in the air. Recoil
//! kicks the view up (and a little sideways, random sign) per shot, grows
//! with heat, and the view comes back down once the trigger rests.
//!
//! Falloff: a hit at distance `t` does
//! `damage * range_mod ^ (t / RANGE_UNIT)` ([`damage_at`]), with the
//! deterministic [`crate::dmath::powf`].

use core::f32::consts::TAU;

use evani_core::num::clamp;
use evani_core::rng::Pcg32;
use evani_physics::collide::V3;
use evani_physics::collide::math::{cross, fmax, fmin, normalize};
use evani_physics::collide::trig::{cos, sin};

use crate::dmath::{powf, tan};
use crate::look_dir;
use crate::tune::WeaponStats;

/// The distance over which a gun keeps `range_mod` of its damage (m).
pub const RANGE_UNIT: f32 = 9.525;

/// Damage (before zone and armour) of a hit `t` metres away:
/// `damage * range_mod ^ (t / RANGE_UNIT)`; `t` under 0 counts as 0, and
/// nothing past the gun's `range` (when it has one).
pub fn damage_at(stats: &WeaponStats, t: f32) -> f32 {
    let t = fmax(t, 0.0);
    if stats.range > 0.0 && t > stats.range {
        return 0.0;
    }
    stats.damage * powf(clamp(stats.range_mod, 0.0, 1.0), t / RANGE_UNIT)
}

/// How the shooter is moving when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stance {
    /// Horizontal speed (m/s).
    pub speed: f32,
    pub airborne: bool,
    pub crouched: bool,
}

/// One bullet leaving the gun.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    /// Unit direction after spread.
    pub dir: V3,
    /// View kick to add: `[yaw, pitch]` (rad).
    pub kick: [f32; 2],
    /// The cone it was drawn from (rad).
    pub cone: f32,
}

/// A gun someone carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Gun {
    pub stats: WeaponStats,
    pub mag: u32,
    pub reserve: u32,
    cooldown: f32,
    reload_left: f32,
    heat: f32,
    since: f32,
    kick: [f32; 2],
    held: bool,
}

impl Gun {
    /// A full gun.
    pub fn new(stats: WeaponStats) -> Self {
        Gun {
            mag: stats.mag,
            reserve: stats.reserve,
            stats,
            cooldown: 0.0,
            reload_left: 0.0,
            heat: 0.0,
            since: 10.0,
            kick: [0.0; 2],
            held: false,
        }
    }

    /// Full magazine and reserve, settled.
    pub fn refill(&mut self) {
        *self = Gun::new(self.stats.clone());
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }

    /// Seconds since the last shot.
    pub fn since_shot(&self) -> f32 {
        self.since
    }

    /// Shots of heat built up.
    pub fn heat(&self) -> f32 {
        self.heat
    }

    /// Start a reload if it would add anything.
    pub fn start_reload(&mut self) -> bool {
        if self.reloading() || self.mag >= self.stats.mag || self.reserve == 0 {
            return false;
        }
        self.reload_left = fmax(self.stats.reload, 0.01);
        true
    }

    /// Stop a reload (switching away); no ammo moves.
    pub fn cancel_reload(&mut self) {
        self.reload_left = 0.0;
    }

    /// Advance `dt`: fire delay, reload, heat; returns the view recovery
    /// `[yaw, pitch]` to add this step.
    pub fn tick(&mut self, dt: f32) -> [f32; 2] {
        self.since += dt;
        self.cooldown = fmax(self.cooldown - dt, 0.0);
        if self.reload_left > 0.0 {
            self.reload_left -= dt;
            if self.reload_left <= 0.0 {
                self.reload_left = 0.0;
                let n = (self.stats.mag - self.mag.min(self.stats.mag)).min(self.reserve);
                self.mag += n;
                self.reserve -= n;
                self.heat = 0.0;
            }
        }
        let rest = self.stats.interval * 1.5;
        if self.since > rest {
            self.heat = fmax(self.heat - self.stats.cool * dt, 0.0);
        }
        let mut back = [0.0; 2];
        if self.since > self.stats.interval * 1.2 {
            let l = (self.kick[0] * self.kick[0] + self.kick[1] * self.kick[1]).sqrt();
            if l > 0.0 {
                let k = fmin((self.stats.recover * dt) / l, 1.0);
                back = [-self.kick[0] * k, -self.kick[1] * k];
                self.kick[0] += back[0];
                self.kick[1] += back[1];
            }
        }
        back
    }

    /// The cone half-angle a shot would have now (rad).
    pub fn spread(&self, s: Stance, run_speed: f32) -> f32 {
        let w = &self.stats;
        let m = if run_speed > 0.0 {
            clamp(s.speed / run_speed, 0.0, 1.0)
        } else {
            0.0
        };
        // Up to walking pace (about a third of a run) costs nothing.
        let moving = w.spread_move * fmax((m - 0.35) / 0.65, 0.0);
        let air = if s.airborne { w.spread_air } else { 0.0 };
        let cone = w.spread + moving + air + w.spread_fire * self.heat;
        if s.crouched {
            cone * w.crouch_mult
        } else {
            cone
        }
    }

    /// Pull (or hold) the trigger looking along `yaw`/`pitch`: a shot when
    /// the gun can fire. Semi-automatic guns need the trigger let go between
    /// shots. An empty magazine starts a reload by itself.
    pub fn fire(
        &mut self,
        trigger: bool,
        yaw: f32,
        pitch: f32,
        s: Stance,
        run_speed: f32,
        rng: &mut Pcg32,
    ) -> Option<Shot> {
        let was = self.held;
        self.held = trigger;
        if !trigger || self.reloading() || self.cooldown > 1e-4 || (!self.stats.auto && was) {
            return None;
        }
        if self.mag == 0 {
            self.start_reload();
            return None;
        }
        let cone = self.spread(s, run_speed);
        let dir = cone_dir(look_dir(yaw, pitch), cone, rng);
        self.mag -= 1;
        self.cooldown = self.stats.interval;
        let up = fmin(
            self.stats.kick * (1.0 + 0.12 * fmin(self.heat, 10.0)),
            fmax(self.stats.kick_max - self.kick[1], 0.0),
        );
        let side = self.stats.kick_side * self.stats.kick * (rng.next_f32() * 2.0 - 1.0);
        self.kick[0] += side;
        self.kick[1] += up;
        self.heat += 1.0;
        self.since = 0.0;
        if self.mag == 0 {
            self.start_reload();
        }
        Some(Shot {
            dir,
            kick: [side, up],
            cone,
        })
    }
}

/// A direction inside a cone of half-angle `cone` round unit `d`, uniform
/// over the disc it cuts.
pub fn cone_dir(d: V3, cone: f32, rng: &mut Pcg32) -> V3 {
    let r = rng.next_f32().sqrt() * fmax(cone, 0.0);
    let th = rng.next_f32() * TAU;
    let right = normalize(cross(d, [0.0, 1.0, 0.0])).unwrap_or([1.0, 0.0, 0.0]);
    let up = cross(right, d);
    let tr = tan(r);
    let (a, b) = (tr * cos(th), tr * sin(th));
    let v = [
        d[0] + right[0] * a + up[0] * b,
        d[1] + right[1] * a + up[1] * b,
        d[2] + right[2] * a + up[2] * b,
    ];
    normalize(v).unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{Health, Zone};
    use crate::hitscan::{Body, Hit, trace};
    use crate::tune::Tuning;
    use evani_physics::collide::WorldBuilder;
    use evani_physics::collide::math::dot;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn rifle_fires_at_600_rpm_and_reload_moves_ammo() {
        let mut g = Gun::new(Tuning::default().rifle);
        let mut rng = Pcg32::seeded(1);
        let mut shots = 0;
        for _ in 0..60 {
            if g.fire(true, 0.0, 0.0, Stance::default(), 4.76, &mut rng)
                .is_some()
            {
                shots += 1;
            }
            g.tick(DT);
        }
        assert_eq!(shots, 10);
        assert_eq!(g.mag, 20);
        assert!(g.start_reload());
        for _ in 0..200 {
            g.tick(DT);
        }
        assert_eq!((g.mag, g.reserve), (30, 80));
    }

    #[test]
    fn pistol_needs_a_fresh_press() {
        let mut g = Gun::new(Tuning::default().pistol);
        let mut rng = Pcg32::seeded(2);
        let mut shots = 0;
        for i in 0..120 {
            let press = (i / 20) % 2 == 0;
            if g.fire(press, 0.0, 0.0, Stance::default(), 4.76, &mut rng)
                .is_some()
            {
                shots += 1;
            }
            g.tick(DT);
        }
        assert_eq!(shots, 3);
    }

    #[test]
    fn first_shot_accurate_standing_and_penalties() {
        let t = Tuning::default();
        let g = Gun::new(t.rifle.clone());
        let still = g.spread(Stance::default(), t.run_speed);
        assert!(still <= 0.003);
        assert_eq!(
            g.spread(
                Stance {
                    speed: 1.5,
                    ..Stance::default()
                },
                t.run_speed
            ),
            still
        );
        assert!(
            g.spread(
                Stance {
                    speed: 4.76,
                    ..Stance::default()
                },
                t.run_speed
            ) > still * 10.0
        );
        assert!(
            g.spread(
                Stance {
                    airborne: true,
                    ..Stance::default()
                },
                t.run_speed
            ) > still * 20.0
        );
        // Spraying heats the gun up; the cone grows, the view climbs, then
        // both settle once the trigger rests.
        let mut g = g;
        let mut rng = Pcg32::seeded(3);
        let mut climb = 0.0;
        for _ in 0..60 {
            if let Some(s) = g.fire(true, 0.0, 0.0, Stance::default(), t.run_speed, &mut rng) {
                climb += s.kick[1];
                assert!(dot(s.dir, look_dir(0.0, 0.0)) >= (s.cone + 1e-4).cos());
            }
            g.tick(DT);
        }
        assert!(climb > 0.1 && g.spread(Stance::default(), t.run_speed) > still * 5.0);
        let mut back = 0.0;
        for _ in 0..240 {
            back += g
                .fire(false, 0.0, 0.0, Stance::default(), t.run_speed, &mut rng)
                .map_or(0.0, |_| 1.0);
            back += g.tick(DT)[1];
        }
        assert!(
            (back + climb).abs() < 1e-3,
            "the view comes back: {back} vs {climb}"
        );
        assert_eq!(g.spread(Stance::default(), t.run_speed), still);
    }

    #[test]
    fn headshot_does_four_times_body_damage() {
        let w = WorldBuilder::new().build();
        let r = Tuning::default().rifle;
        let b = Body::new([0.0, 0.0, 6.0], 1.372, 1.219);
        let shoot = |y: f32| -> f32 {
            let Some(Hit::Body { zone, .. }) =
                trace(&w, [0.0, y, 0.0], [0.0, 0.0, 1.0], 100.0, &[b], None)
            else {
                panic!("missed at {y}");
            };
            let mut h = Health::new(1000.0, 0.0, false);
            h.take(r.damage * zone.mult(&r), r.armor_pen, zone).hp
        };
        let head = shoot(b.head_centre()[1]);
        let chest = shoot(b.chest()[1]);
        assert_eq!(chest, 36.0);
        assert_eq!(head, 144.0);
        assert_eq!(Zone::Legs.mult(&r), 0.75);
    }

    #[test]
    fn damage_falls_off_with_range() {
        let t = Tuning::default();
        let (r, p) = (&t.rifle, &t.pistol);
        // Point blank: full damage; one range unit: range_mod of it; two:
        // its square.
        assert_eq!(damage_at(r, 0.0), r.damage);
        assert_eq!(damage_at(r, -3.0), r.damage);
        assert!((damage_at(r, RANGE_UNIT) - r.damage * 0.98).abs() < 1e-4);
        assert!((damage_at(r, 2.0 * RANGE_UNIT) - r.damage * 0.98 * 0.98).abs() < 1e-4);
        // Steadily lower with distance; the pistol loses more than the rifle.
        let mut last = f32::INFINITY;
        for i in 0..40 {
            let d = damage_at(p, i as f32 * 3.0);
            assert!(d < last, "falls at {i}: {d} vs {last}");
            last = d;
        }
        assert!(damage_at(p, 30.0) / p.damage < damage_at(r, 30.0) / r.damage);
        // A gun with no falloff keeps all of it; past the range, none.
        let flat = WeaponStats {
            range_mod: 1.0,
            ..r.clone()
        };
        assert_eq!(damage_at(&flat, 100.0), r.damage);
        assert_eq!(damage_at(r, r.range + 1.0), 0.0);
        // Matches the library's power to float precision (test only).
        for d in [1.0f32, 7.5, 22.0, 60.0, 150.0] {
            let want = r.damage * 0.98f32.powf(d / RANGE_UNIT);
            assert!(
                (damage_at(r, d) - want).abs() < 1e-4,
                "{d}: {} vs {want}",
                damage_at(r, d)
            );
        }
    }

    #[test]
    fn spread_draws_replay_from_the_seed() {
        let go = |seed: u64| {
            let mut g = Gun::new(Tuning::default().rifle);
            let mut rng = Pcg32::seeded(seed);
            let mut bits = Vec::new();
            for _ in 0..120 {
                if let Some(s) = g.fire(
                    true,
                    0.3,
                    0.1,
                    Stance {
                        speed: 4.0,
                        ..Stance::default()
                    },
                    4.76,
                    &mut rng,
                ) {
                    bits.extend(s.dir.iter().map(|x| x.to_bits()));
                    bits.push(s.kick[0].to_bits());
                }
                g.tick(DT);
            }
            bits
        };
        assert_eq!(go(11), go(11));
        assert_ne!(go(11), go(12));
    }
}
