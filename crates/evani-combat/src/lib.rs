//! Combat (`evani-combat`, off unless a kit depends on it): general shooter
//! combat on an [`evani_physics::collide::StaticWorld`].
//!
//! * [`hitscan`]: bullets against the map's real triangles (hit point,
//!   normal, tag) and against body hitboxes (a head sphere over a body
//!   cylinder), nearest first; a wall in front of a body stops the bullet.
//! * [`gun`]: magazine and reserve, fire rate, recoil that climbs and
//!   recovers, spread with an accurate first shot and move and air
//!   penalties, and damage falloff over range ([`gun::damage_at`]).
//! * [`health`]: health, armour (penetration share, helmet) and hit-zone
//!   multipliers.
//! * [`grenade`]: a swept sphere that bounces (`v -= (1+e)(v.n)n`), slides
//!   with friction, comes to rest and goes off when its fuse ends.
//! * [`rocket`]: a fast swept sphere with a little gravity that goes off on
//!   the first wall or body it touches (bodies widened by its size, the
//!   shooter skipped), or when its flight time ends.
//! * [`blast`]: linear falloff with line of sight to the chest or the head.
//! * [`path`]: Dijkstra with parents over any graph, and a line-of-sight
//!   string pull.
//! * [`bot`]: a bot brain: a small state machine (idle, hunt, hold, attack,
//!   investigate, retreat), hearing, aim error that settles, a reaction
//!   gate, line of sight through the world.
//! * [`tune`]: every number in one table ([`Tuning::default`]).
//! * [`params`]: the live `combat.*` parameters ([`CombatModule`] registers
//!   them; [`CombatParams::tuning`] reads the current table each step).
//!
//! Angles: yaw 0 faces +z and positive yaw turns towards +x
//! (`forward = (sin yaw, cos yaw)`); pitch is positive looking up.
//!
//! Deterministic: the game steps everything at the fixed 60 Hz tick; random
//! numbers come only from the seeded [`evani_core::rng::Pcg32`] a caller
//! passes in (or a [`Brain`]'s own, seeded at creation); no clock; sine,
//! cosine and atan2 are the polynomial ones in `collide::trig`, and the
//! range falloff power is [`dmath::powf`], so native and wasm give the same
//! bits.
//!
//! ```
//! use evani_combat::{Body, Gun, Hit, Stance, Tuning, trace};
//! use evani_core::rng::Pcg32;
//! use evani_physics::collide::WorldBuilder;
//! let mut b = WorldBuilder::new();
//! b.aabb([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0], 1);
//! let world = b.build();
//! let t = Tuning::default();
//! let mut gun = Gun::new(t.rifle.clone());
//! let mut rng = Pcg32::seeded(1);
//! let shot = gun.fire(true, 0.0, 0.0, Stance::default(), t.run_speed, &mut rng);
//! let target = [Body::new([0.0, 0.0, 6.0], 1.8, 1.6)];
//! let dir = shot.map(|s| s.dir).unwrap_or([0.0, 0.0, 1.0]);
//! let hit = trace(&world, [0.0, 1.2, 0.0], dir, t.rifle.range, &target, None);
//! assert!(matches!(hit, Some(Hit::Body { index: 0, .. })));
//! ```

pub mod blast;
pub mod bot;
pub mod dmath;
pub mod grenade;
pub mod gun;
pub mod health;
pub mod hitscan;
pub mod params;
pub mod path;
pub mod rocket;
pub mod tune;

pub use blast::blast_damage;
pub use bot::{BotIn, BotOut, BotState, Brain, Foe, Orders, Router, Sound};
pub use grenade::{Grenade, GrenadeEvent};
pub use gun::{Gun, Shot, Stance, damage_at};
pub use health::{Damage, Health, Zone};
pub use hitscan::{Body, Hit, trace};
pub use params::{CombatModule, CombatParams};
pub use rocket::{Impact, Rocket};
pub use tune::{BotTune, GrenadeStats, RocketStats, SoundTune, Tuning, WeaponStats};

use evani_physics::collide::V3;
use evani_physics::collide::math::{len, normalize};
use evani_physics::collide::trig::{atan2, cos, sin};

/// The unit look direction for `yaw` and `pitch`.
pub fn look_dir(yaw: f32, pitch: f32) -> V3 {
    let cp = cos(pitch);
    [cp * sin(yaw), sin(pitch), cp * cos(yaw)]
}

/// The yaw and pitch that look along `d` (any length; zero looks ahead).
pub fn yaw_pitch_of(d: V3) -> (f32, f32) {
    let h = (d[0] * d[0] + d[2] * d[2]).sqrt();
    if h <= 1e-9 && d[1].abs() <= 1e-9 {
        return (0.0, 0.0);
    }
    (atan2(d[0], d[2]), atan2(d[1], h))
}

/// An angle wrapped into `-pi..pi`.
pub fn wrap(a: f32) -> f32 {
    evani_core::num::wrap_angle(a)
}

/// Distance between two points.
pub fn dist(a: V3, b: V3) -> f32 {
    len([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

/// Horizontal distance between two points.
pub fn dist_xz(a: V3, b: V3) -> f32 {
    let (x, z) = (a[0] - b[0], a[2] - b[2]);
    (x * x + z * z).sqrt()
}

/// The unit direction from `a` to `b`, or `None` when they coincide.
pub fn dir_to(a: V3, b: V3) -> Option<V3> {
    normalize([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
}

#[cfg(test)]
mod tests;
