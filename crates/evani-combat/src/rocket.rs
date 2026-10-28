//! A rocket: a small sphere flying fast along its launch line with a little
//! gravity, swept through the world each substep and tested against body
//! hitboxes (widened by its own size). It goes off where it first touches
//! something, or in the air when its flight time ends. The caller turns the
//! [`Impact`] into a blast (see [`crate::blast`]).

use evani_physics::collide::math::{fmax, len, mad, normalize};
use evani_physics::collide::{StaticWorld, V3};

use crate::hitscan::{Body, ray_body};
use crate::tune::RocketStats;

/// Longest physics substep (s).
const SUBSTEP: f32 = 1.0 / 120.0;
/// How far the blast centre sits off the surface it hit (m), so its line of
/// sight is not taken by that surface.
pub const STANDOFF: f32 = 0.1;

/// Where a rocket went off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Impact {
    /// The contact point (on the wall or the body; the rocket's own
    /// position when it ran out of flight time).
    pub point: V3,
    /// The surface normal there, facing the rocket (for a body: back along
    /// the flight; in the air: up).
    pub normal: V3,
    /// Where the blast goes: [`STANDOFF`] off the point along the normal.
    pub centre: V3,
    /// The body it struck (its index in the slice), if any.
    pub body: Option<usize>,
    /// The world triangle's tag, when it hit the map.
    pub tag: Option<u32>,
}

/// A rocket in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rocket {
    pub pos: V3,
    /// Position before the last step (for drawing between steps and trails).
    pub prev: V3,
    pub vel: V3,
    /// Seconds in flight.
    pub age: f32,
    /// Who fired it (the game's index): its own body is never struck.
    pub owner: usize,
}

impl Rocket {
    /// Fired from `from` along `dir` (any length; zero fires along +z).
    pub fn launched(from: V3, dir: V3, stats: &RocketStats, owner: usize) -> Self {
        let d = normalize(dir).unwrap_or([0.0, 0.0, 1.0]);
        Rocket {
            pos: from,
            prev: from,
            vel: [d[0] * stats.speed, d[1] * stats.speed, d[2] * stats.speed],
            age: 0.0,
            owner,
        }
    }

    /// The unit flight direction (along the velocity).
    pub fn dir(&self) -> V3 {
        normalize(self.vel).unwrap_or([0.0, 0.0, 1.0])
    }

    /// Advance `dt` against the map and `bodies` (the owner's index is
    /// skipped, as are dead bodies). Returns where it went off, if it did
    /// (the caller then removes it).
    pub fn step(
        &mut self,
        world: &StaticWorld,
        bodies: &[Body],
        stats: &RocketStats,
        dt: f32,
    ) -> Option<Impact> {
        self.prev = self.pos;
        if dt <= 0.0 {
            return None;
        }
        let n = fmax((dt / SUBSTEP).ceil(), 1.0) as u32;
        let h = dt / n as f32;
        for _ in 0..n {
            if self.age >= stats.life {
                let up = [0.0, 1.0, 0.0];
                return Some(Impact {
                    point: self.pos,
                    normal: up,
                    centre: self.pos,
                    body: None,
                    tag: None,
                });
            }
            self.age += h;
            self.vel[1] -= stats.gravity * h;
            if let Some(hit) = self.substep(world, bodies, stats, h) {
                return Some(hit);
            }
        }
        None
    }

    fn substep(
        &mut self,
        world: &StaticWorld,
        bodies: &[Body],
        stats: &RocketStats,
        h: f32,
    ) -> Option<Impact> {
        let motion = [self.vel[0] * h, self.vel[1] * h, self.vel[2] * h];
        let dist = len(motion);
        if dist < 1e-7 {
            return None;
        }
        let d = [motion[0] / dist, motion[1] / dist, motion[2] / dist];
        let wall = world.cast_sphere(self.pos, stats.size, motion);
        let mut reach = wall.map_or(dist, |w| w.distance);
        let mut struck: Option<(usize, f32)> = None;
        for (i, b) in bodies.iter().enumerate() {
            if i == self.owner || !b.alive {
                continue;
            }
            // The body widened by the rocket's radius: a sphere against a
            // body is a ray against the body grown by that much.
            let wide = Body {
                radius: b.radius + stats.size,
                head: b.head + stats.size,
                ..*b
            };
            if let Some((t, _)) = ray_body(self.pos, d, &wide)
                && t <= reach
            {
                reach = t;
                struck = Some((i, t));
            }
        }
        if let Some((i, t)) = struck {
            let back = [-d[0], -d[1], -d[2]];
            // The contact on the body's skin: the rocket's centre plus its
            // radius along the flight.
            let point = mad(self.pos, d, t + stats.size);
            self.pos = mad(self.pos, d, t);
            return Some(Impact {
                point,
                normal: back,
                centre: mad(point, back, STANDOFF),
                body: Some(i),
                tag: None,
            });
        }
        if let Some(w) = wall {
            self.pos = mad(self.pos, motion, w.fraction);
            let centre = mad(w.point, w.normal, STANDOFF);
            return Some(Impact {
                point: w.point,
                normal: w.normal,
                centre,
                body: None,
                tag: Some(w.tag),
            });
        }
        self.pos = mad(self.pos, motion, 1.0);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blast::blast_damage;
    use crate::tune::Tuning;
    use evani_physics::collide::WorldBuilder;

    /// A floor and a wall whose front face is at z = 10.
    fn range() -> StaticWorld {
        let mut b = WorldBuilder::new();
        b.aabb([-20.0, -0.2, -20.0], [20.0, 0.0, 30.0], 1);
        b.aabb([-5.0, 0.0, 10.0], [5.0, 4.0, 10.3], 2);
        b.build()
    }

    fn fly(
        r: &mut Rocket,
        w: &StaticWorld,
        bodies: &[Body],
        s: &RocketStats,
        dt: f32,
    ) -> (Impact, u32) {
        for tick in 1..=600 {
            if let Some(i) = r.step(w, bodies, s, dt) {
                return (i, tick);
            }
        }
        panic!("never went off: {r:?}");
    }

    #[test]
    fn rocket_hits_a_wall_and_goes_off_at_the_contact_point() {
        let w = range();
        let s = Tuning::default().rocket;
        let mut r = Rocket::launched([0.0, 1.5, 0.0], [0.0, 0.0, 1.0], &s, 0);
        let (hit, ticks) = fly(&mut r, &w, &[], &s, 1.0 / 60.0);
        assert_eq!((hit.body, hit.tag), (None, Some(2)));
        assert!(
            (hit.point[2] - 10.0).abs() < 1e-3,
            "on the wall face: {:?}",
            hit.point
        );
        assert!(hit.normal[2] < -0.99, "facing back: {:?}", hit.normal);
        assert!((hit.centre[2] - (10.0 - STANDOFF)).abs() < 1e-3);
        // Dropped about 10 cm over 10 m at 35 m/s (g/2 t^2), and got there
        // in about 0.29 s.
        let drop = 1.5 - hit.point[1];
        assert!(
            (drop - 0.5 * s.gravity * (10.0f32 / s.speed).powi(2)).abs() < 0.01,
            "{drop}"
        );
        let t = ticks as f32 / 60.0;
        assert!((t - 10.0 / s.speed).abs() < 2.0 / 60.0, "{t}");
        // The rocket's centre stops one radius short of the wall.
        assert!(
            r.pos[2] <= 10.0 - s.size + 1e-3 && r.pos[2] > 10.0 - s.size - 0.01,
            "{:?}",
            r.pos
        );
    }

    #[test]
    fn a_thin_wall_stops_it_even_on_long_frames() {
        let mut b = WorldBuilder::new();
        b.aabb([-5.0, 0.0, 6.0], [5.0, 4.0, 6.02], 7);
        let w = b.build();
        let s = Tuning::default().rocket;
        let mut r = Rocket::launched([0.0, 1.5, 0.0], [0.0, 0.0, 1.0], &s, 0);
        let (hit, _) = fly(&mut r, &w, &[], &s, 1.0 / 15.0);
        assert_eq!(hit.tag, Some(7));
        assert!((hit.point[2] - 6.0).abs() < 1e-3);
    }

    #[test]
    fn rocket_strikes_a_body_before_the_wall_but_never_its_owner() {
        let w = range();
        let s = Tuning::default().rocket;
        // The shooter stands at the launch point; a foe at z = 6.
        let bodies = [
            Body::new([0.0, 0.0, 0.0], 1.372, 1.219),
            Body::new([0.0, 0.0, 6.0], 1.372, 1.219),
        ];
        let mut r = Rocket::launched([0.0, 1.0, 0.3], [0.0, 0.0, 1.0], &s, 0);
        let (hit, _) = fly(&mut r, &w, &bodies, &s, 1.0 / 60.0);
        assert_eq!(hit.body, Some(1));
        assert!(
            (hit.point[2] - (6.0 - bodies[1].radius)).abs() < 1e-3,
            "on its skin: {:?}",
            hit.point
        );
        assert!(hit.centre[2] < hit.point[2]);
        // A direct hit through armour kills.
        let raw = blast_damage(&w, hit.centre, s.damage, s.radius, &bodies[1]);
        assert!(raw * s.armor_pen >= 100.0, "{raw}");
        // A dead foe is flown through, to the wall.
        let dead = [
            bodies[0],
            Body {
                alive: false,
                ..bodies[1]
            },
        ];
        let mut r = Rocket::launched([0.0, 1.0, 0.3], [0.0, 0.0, 1.0], &s, 0);
        assert_eq!(fly(&mut r, &w, &dead, &s, 1.0 / 60.0).0.tag, Some(2));
    }

    #[test]
    fn a_near_miss_still_strikes_with_the_rockets_size() {
        let w = range();
        let s = Tuning::default().rocket;
        // The line passes 0.24 m from the body's axis: outside the body
        // (0.2) but within it plus the rocket (0.26).
        let bodies = [Body::new([0.24, 0.0, 6.0], 1.372, 1.219)];
        let mut r = Rocket::launched([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], &s, 9);
        assert_eq!(fly(&mut r, &w, &bodies, &s, 1.0 / 60.0).0.body, Some(0));
    }

    #[test]
    fn rocket_drops_slightly_and_goes_off_when_its_flight_ends() {
        let w = WorldBuilder::new().build();
        let s = Tuning::default().rocket;
        let mut r = Rocket::launched([0.0, 50.0, 0.0], [1.0, 0.0, 0.0], &s, 0);
        for _ in 0..60 {
            assert!(r.step(&w, &[], &s, 1.0 / 60.0).is_none());
        }
        // After 1 s: about g/2 lower, about `speed` along.
        assert!(
            (r.pos[1] - (50.0 - 0.5 * s.gravity)).abs() < 0.05,
            "{:?}",
            r.pos
        );
        assert!((r.pos[0] - s.speed).abs() < 0.01);
        let (hit, ticks) = fly(&mut r, &w, &[], &s, 1.0 / 60.0);
        assert_eq!((hit.body, hit.tag, hit.centre), (None, None, hit.point));
        let t = 1.0 + ticks as f32 / 60.0;
        assert!((t - s.life).abs() < 2.0 / 60.0, "{t}");
    }

    #[test]
    fn same_launch_same_flight() {
        let w = range();
        let s = Tuning::default().rocket;
        let go = || {
            let mut r = Rocket::launched([0.3, 1.2, -1.0], [0.1, 0.05, 1.0], &s, 0);
            fly(&mut r, &w, &[], &s, 1.0 / 60.0)
        };
        assert_eq!(go(), go());
    }
}
