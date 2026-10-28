//! A thrown grenade: a small sphere swept through the world each substep.
//!
//! On contact it reflects `v -= (1 + e)(v.n)n` and loses a share of its
//! sliding speed; gentle contacts (rolling, resting) lose their normal speed
//! instead of bouncing and slow with a rolling drag. On a floor and slow it
//! comes to rest. When the fuse ends it reports an explosion.

use evani_core::num::clamp;
use evani_physics::collide::math::{dot, fmax, fmin, len, mad};
use evani_physics::collide::{StaticWorld, V3};

use crate::look_dir;
use crate::tune::GrenadeStats;

/// Longest physics substep (s).
const SUBSTEP: f32 = 1.0 / 120.0;
/// Normal speed under which a contact rolls instead of bouncing (m/s).
const BOUNCE_MIN: f32 = 1.2;
/// Rolling drag on a floor (1/s).
const ROLL_DRAG: f32 = 2.5;
/// Speed under which it rests on a floor (m/s).
const REST_SPEED: f32 = 0.35;
/// Gap kept off a surface after contact (m).
const SKIN: f32 = 0.002;

/// Something a grenade did this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GrenadeEvent {
    /// Hit a surface hard enough to be heard.
    Bounce { point: V3, speed: f32, tag: u32 },
    /// The fuse ran out here.
    Explode { pos: V3 },
}

/// A grenade in the air or on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grenade {
    pub pos: V3,
    /// Position before the last step (for drawing between steps).
    pub prev: V3,
    pub vel: V3,
    /// Seconds left.
    pub fuse: f32,
    pub resting: bool,
    /// Who threw it (the game's index).
    pub owner: usize,
    /// Tumble angle for drawing (rad).
    pub spin: f32,
    pub bounces: u32,
}

impl Grenade {
    /// Thrown from `from` looking along `yaw`/`pitch`, with the thrower's
    /// velocity added.
    pub fn thrown(
        from: V3,
        yaw: f32,
        pitch: f32,
        thrower_vel: V3,
        stats: &GrenadeStats,
        owner: usize,
    ) -> Self {
        let d = look_dir(yaw, fmin(pitch + stats.loft, 1.5));
        let vel = mad(thrower_vel, d, stats.speed);
        Grenade {
            pos: from,
            prev: from,
            vel,
            fuse: stats.fuse,
            resting: false,
            owner,
            spin: 0.0,
            bounces: 0,
        }
    }

    /// Advance `dt`; events go to `out`. Returns true when it exploded (the
    /// caller then removes it).
    pub fn step(
        &mut self,
        world: &StaticWorld,
        stats: &GrenadeStats,
        dt: f32,
        out: &mut Vec<GrenadeEvent>,
    ) -> bool {
        self.prev = self.pos;
        self.fuse -= dt;
        if self.fuse <= 0.0 {
            out.push(GrenadeEvent::Explode { pos: self.pos });
            return true;
        }
        if self.resting || dt <= 0.0 {
            return false;
        }
        let n = fmax((dt / SUBSTEP).ceil(), 1.0) as u32;
        let h = dt / n as f32;
        for _ in 0..n {
            self.substep(world, stats, h, out);
            if self.resting {
                break;
            }
        }
        self.spin += len(self.vel) * dt * 6.0;
        false
    }

    fn substep(
        &mut self,
        world: &StaticWorld,
        stats: &GrenadeStats,
        h: f32,
        out: &mut Vec<GrenadeEvent>,
    ) {
        self.vel[1] -= stats.gravity * h;
        let mut left = h;
        for _ in 0..4 {
            let motion = [self.vel[0] * left, self.vel[1] * left, self.vel[2] * left];
            if len(motion) < 1e-7 {
                return;
            }
            let Some(hit) = world.cast_sphere(self.pos, stats.size, motion) else {
                self.pos = mad(self.pos, motion, 1.0);
                return;
            };
            let nrm = hit.normal;
            self.pos = mad(mad(self.pos, motion, hit.fraction), nrm, SKIN);
            left *= 1.0 - hit.fraction;
            let vn = dot(self.vel, nrm);
            if vn < 0.0 {
                let tangent = mad(self.vel, nrm, -vn);
                if -vn > BOUNCE_MIN {
                    // v - (1 + e)(v.n)n, with the sliding part slowed.
                    let keep = 1.0 - clamp(stats.friction, 0.0, 1.0);
                    self.vel = mad(
                        [tangent[0] * keep, tangent[1] * keep, tangent[2] * keep],
                        nrm,
                        -vn * stats.restitution,
                    );
                    self.bounces += 1;
                    out.push(GrenadeEvent::Bounce {
                        point: hit.point,
                        speed: -vn,
                        tag: hit.tag,
                    });
                } else {
                    let k = fmax(1.0 - ROLL_DRAG * h, 0.0);
                    self.vel = [tangent[0] * k, tangent[1] * k, tangent[2] * k];
                }
            }
            if nrm[1] > 0.7 && len(self.vel) < REST_SPEED {
                self.vel = [0.0; 3];
                self.resting = true;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tune::Tuning;
    use evani_physics::collide::WorldBuilder;

    #[test]
    fn grenade_bounces_off_a_wall_and_rests() {
        let mut b = WorldBuilder::new();
        b.aabb([-20.0, -0.2, -20.0], [20.0, 0.0, 20.0], 1);
        b.aabb([-5.0, 0.0, 3.0], [5.0, 3.0, 3.3], 2);
        let w = b.build();
        let mut stats = Tuning::default().grenade;
        stats.fuse = 30.0;
        let mut g = Grenade {
            pos: [0.0, 1.0, 0.0],
            prev: [0.0, 1.0, 0.0],
            vel: [0.0, 1.0, 12.0],
            fuse: 30.0,
            resting: false,
            owner: 0,
            spin: 0.0,
            bounces: 0,
        };
        let mut ev = Vec::new();
        let mut max_z: f32 = 0.0;
        let mut ticks = 0;
        while !g.resting && ticks < 1200 {
            assert!(!g.step(&w, &stats, 1.0 / 60.0, &mut ev));
            max_z = max_z.max(g.pos[2]);
            ticks += 1;
        }
        let wall_hit = ev
            .iter()
            .any(|e| matches!(e, GrenadeEvent::Bounce { tag: 2, .. }));
        assert!(wall_hit, "bounced off the wall: {ev:?}");
        assert!(
            max_z <= 3.0 - stats.size + 0.01,
            "never passed into the wall: {max_z}"
        );
        assert!(g.resting, "came to rest within 20 s");
        assert!(g.pos[2] < 2.9, "came back off the wall: {:?}", g.pos);
        assert!(
            (g.pos[1] - stats.size).abs() < 0.02,
            "rests on the floor: {:?}",
            g.pos
        );
        // And it goes off when the fuse ends.
        g.fuse = 0.01;
        assert!(g.step(&w, &stats, 1.0 / 60.0, &mut ev));
        assert!(matches!(ev.last(), Some(GrenadeEvent::Explode { .. })));
    }

    #[test]
    fn thrown_follows_the_look_and_carries_the_throwers_speed() {
        let s = Tuning::default().grenade;
        let g = Grenade::thrown([0.0, 1.2, 0.0], 0.0, 0.0, [1.0, 0.0, 0.0], &s, 3);
        assert!(g.vel[2] > 13.0 && g.vel[1] > 1.0 && (g.vel[0] - 1.0).abs() < 1e-5 && g.owner == 3);
    }
}
