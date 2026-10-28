//! A kinematic character mover on the static world: an upright capsule
//! that slides along walls and creases, climbs steps up to a set height,
//! walks up and down walkable slopes, treats steeper slopes as walls, stops
//! under ceilings and snaps down onto the ground it walks over (stairs and
//! ramps going down), so it never bounces off them.
//!
//! The game owns velocity, gravity and input; each fixed step it hands this
//! mover the step's motion and gets back where the body ended and what it
//! touched.

use crate::collide::cast::Capsule;
use crate::collide::math::{V3, add, cross, dot, fmax, len, len_xz, normalize, reject, scale, sub};
use crate::collide::world::StaticWorld;

/// How the character collides. [`CharacterConfig::default`] is a generic
/// adult-sized body (0.3 m radius, 0.35 m step, 45 degree slopes); a game
/// with the engine's live parameters reads the tuned one with
/// [`crate::collide::params::CollideParams::config`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterConfig {
    /// Capsule radius (m).
    pub radius: f32,
    /// Highest ledge it steps straight up onto (m).
    pub step_height: f32,
    /// Lowest ground normal `y` it can stand on (the cosine of the steepest
    /// walkable slope).
    pub min_ground_normal_y: f32,
    /// Gap kept from surfaces after a move (m).
    pub skin: f32,
    /// How far down it looks for ground to stay on while walking (m).
    pub snap_distance: f32,
    /// Most slide iterations per move.
    pub max_slides: usize,
    /// Most push-out passes when a move starts inside something.
    pub depenetrate_passes: usize,
}

impl Default for CharacterConfig {
    fn default() -> Self {
        CharacterConfig::new(0.3, 0.35, 45.0)
    }
}

impl CharacterConfig {
    /// A config with the slope limit given in degrees (its cosine from the
    /// module's deterministic [`crate::collide::trig::cos`]); skin 5 mm,
    /// ground snap as deep as the step, 4 slides and 4 push-out passes.
    pub fn new(radius: f32, step_height: f32, max_slope_deg: f32) -> Self {
        CharacterConfig {
            radius,
            step_height,
            min_ground_normal_y: slope_cos(max_slope_deg),
            skin: 0.005,
            snap_distance: step_height,
            max_slides: 4,
            depenetrate_passes: 4,
        }
    }

    /// The capsule standing at `feet`, `height` tall.
    pub fn capsule(&self, feet: V3, height: f32) -> Capsule {
        Capsule::upright(feet, self.radius, height)
    }

    fn walkable(&self, n: V3) -> bool {
        n[1] >= self.min_ground_normal_y
    }
}

/// What one move did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveResult {
    /// Where the feet ended.
    pub feet: V3,
    /// Standing on walkable ground at the end.
    pub grounded: bool,
    /// The ground's normal (up when not grounded).
    pub ground_normal: V3,
    /// Touched a wall (or a slope too steep to walk).
    pub hit_wall: bool,
    /// Touched a ceiling.
    pub hit_ceiling: bool,
    /// Climbed a step.
    pub stepped: bool,
    /// Pushed out of an overlap first (m).
    pub depenetrated: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct SlideInfo {
    ground: Option<V3>,
    wall: bool,
    ceiling: bool,
}

const UP: V3 = [0.0, 1.0, 0.0];

/// The cosine of a slope limit in degrees (clamped to 0..=90), from the
/// deterministic [`crate::collide::trig::cos`].
pub fn slope_cos(max_slope_deg: f32) -> f32 {
    let d = if max_slope_deg > 90.0 {
        90.0
    } else if max_slope_deg >= 0.0 {
        max_slope_deg
    } else {
        0.0
    };
    fmax(crate::collide::trig::cos(crate::collide::trig::radians(d)), 0.0)
}

impl StaticWorld {
    /// Move a character standing at `feet` (`height` tall) by `motion`.
    /// `stick` (it was grounded and is not jumping) allows the step-up and
    /// keeps it on the ground it walks over.
    pub fn move_character(&self, feet: V3, height: f32, motion: V3, cfg: &CharacterConfig, stick: bool) -> MoveResult {
        // Out of any overlap first (spawned in a wall, a crouch released
        // under something): the push is reported, never hidden.
        let push = self.depenetrate(&cfg.capsule(feet, height), cfg.depenetrate_passes);
        let start = add(feet, push);
        let (mut end, mut info) = self.slide(start, height, motion, cfg);
        let mut stepped = false;

        // Step up: a grounded body blocked by a wall tries again lifted by
        // the step height, then sets down; it keeps that path if it got
        // further and landed on walkable ground.
        let horiz = [motion[0], 0.0, motion[2]];
        if stick && info.wall && cfg.step_height > 0.0 && len_xz(horiz) > 1e-6 {
            let hdir = scale(horiz, 1.0 / len_xz(horiz));
            let gained = dot(sub(end, start), hdir);
            let cap = cfg.capsule(start, height);
            let rise = match self.cast_capsule(&cap, [0.0, cfg.step_height, 0.0]) {
                Some(h) => fmax(h.distance - cfg.skin, 0.0),
                None => cfg.step_height,
            };
            if rise > cfg.skin {
                let lifted = add(start, [0.0, rise, 0.0]);
                let (over, over_info) = self.slide(lifted, height, horiz, cfg);
                let drop = rise + cfg.skin * 2.0;
                if let Some(h) = self.cast_capsule(&cfg.capsule(over, height), [0.0, -drop, 0.0])
                    && let Some((gn, surface)) = self.ground_under(&h, over, cfg)
                {
                    let landed = add(over, [0.0, -fmax(h.distance - cfg.skin, 0.0), 0.0]);
                    let gained2 = dot(sub(landed, start), hdir);
                    // The height of the surface stepped onto, not of the
                    // capsule (which may still rest on the rim's edge).
                    let climbed = surface - start[1];
                    if gained2 > gained + 1e-3 && climbed <= cfg.step_height + cfg.skin {
                        end = landed;
                        info = SlideInfo { ground: Some(gn), wall: over_info.wall, ceiling: info.ceiling || over_info.ceiling };
                        stepped = true;
                    }
                }
            }
        }

        // Ground snap: keep a walking body on the ground below it (down
        // stairs and ramps) instead of launching off every edge.
        if stick
            && !stepped
            && motion[1] <= 0.0
            && cfg.snap_distance > 0.0
            && let Some(h) = self.cast_capsule(&cfg.capsule(end, height), [0.0, -cfg.snap_distance, 0.0])
            && let Some((gn, _)) = self.ground_under(&h, end, cfg)
        {
            end = add(end, [0.0, -fmax(h.distance - cfg.skin, 0.0), 0.0]);
            info.ground = Some(gn);
        }

        // Final probe: standing on walkable ground within a few skins.
        if info.ground.is_none()
            && let Some(h) = self.cast_capsule(&cfg.capsule(end, height), [0.0, -cfg.skin * 3.0, 0.0])
            && let Some((gn, _)) = self.ground_under(&h, end, cfg)
        {
            info.ground = Some(gn);
        }

        MoveResult {
            feet: end,
            grounded: info.ground.is_some(),
            ground_normal: info.ground.unwrap_or(UP),
            hit_wall: info.wall,
            hit_ceiling: info.ceiling,
            stepped,
            depenetrated: len(push),
        }
    }

    /// The walkable ground under a downward cast's contact, as its normal
    /// and surface height, or `None`. A capsule resting on the rim of a step touches its edge with
    /// a slanted normal; the surface itself is read by a short ray straight
    /// down just inside the rim (away from the capsule's axis: the face that
    /// holds a sphere up on an edge lies on the far side of the edge).
    fn ground_under(&self, h: &crate::collide::cast::CastHit, feet: V3, cfg: &CharacterConfig) -> Option<(V3, f32)> {
        if cfg.walkable(h.normal) {
            return Some((h.normal, h.point[1]));
        }
        if h.normal[1] <= 0.0 || h.point[1] > feet[1] + cfg.radius {
            return None;
        }
        let away = normalize([h.point[0] - feet[0], 0.0, h.point[2] - feet[2]]).unwrap_or([0.0; 3]);
        let probe = add(add(h.point, scale(away, 0.01)), [0.0, 0.05, 0.0]);
        let r = self.raycast(probe, [0.0, -1.0, 0.0], 0.1)?;
        if cfg.walkable(r.normal) { Some((r.normal, r.point[1])) } else { None }
    }

    /// Slide the capsule along `motion`, projecting what is left of the
    /// motion onto each surface it meets (and along the crease of two).
    fn slide(&self, feet: V3, height: f32, motion: V3, cfg: &CharacterConfig) -> (V3, SlideInfo) {
        let mut pos = feet;
        let mut rem = motion;
        let mut info = SlideInfo::default();
        let mut planes: [V3; 3] = [[0.0; 3]; 3];
        let mut np = 0usize;
        for _ in 0..cfg.max_slides {
            let l = len(rem);
            if l < 1e-7 {
                break;
            }
            let Some(h) = self.cast_capsule(&cfg.capsule(pos, height), rem) else {
                pos = add(pos, rem);
                break;
            };
            let dir = scale(rem, 1.0 / l);
            let travel = fmax(h.distance - cfg.skin, 0.0);
            pos = add(pos, scale(dir, travel));
            let walkable = cfg.walkable(h.normal);
            if walkable {
                info.ground = Some(h.normal);
            } else if h.normal[1] < -0.3 {
                info.ceiling = true;
            } else {
                info.wall = true;
            }
            // A slope too steep to stand on acts as an upright wall, so the
            // slide can never climb it.
            let n = if !walkable && h.normal[1] > 0.0 { normalize([h.normal[0], 0.0, h.normal[2]]).unwrap_or(h.normal) } else { h.normal };
            rem = reject(scale(dir, l - travel), n);
            // Pushing back into an earlier surface: follow the crease, or stop
            // in a corner.
            for &p in &planes[..np] {
                if dot(rem, p) < -1e-6 {
                    rem = match normalize(cross(p, n)) {
                        Some(crease) => scale(crease, dot(rem, crease)),
                        None => [0.0; 3],
                    };
                    if planes[..np].iter().any(|&q| dot(rem, q) < -1e-6) {
                        rem = [0.0; 3];
                    }
                    break;
                }
            }
            if np < planes.len() {
                planes[np] = n;
                np += 1;
            } else {
                planes.rotate_left(1);
                planes[2] = n;
            }
        }
        (pos, info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::trig;
    use crate::collide::world::WorldBuilder;

    /// A generic adult-sized body: 0.3 m radius, 1.8 m tall, 0.35 m step.
    fn cfg() -> CharacterConfig {
        CharacterConfig::default()
    }
    const H: f32 = 1.8;
    const GRAVITY: f32 = 9.81;

    fn room() -> WorldBuilder {
        let mut b = WorldBuilder::new();
        b.aabb([-20.0, -1.0, -20.0], [20.0, 0.0, 20.0], 0); // floor
        b.aabb([5.0, 0.0, -20.0], [6.0, 4.0, 20.0], 1); // wall at x = 5
        b
    }

    /// Run `steps` fixed steps of horizontal velocity `v` with gravity.
    fn walk(w: &StaticWorld, mut feet: V3, v: V3, steps: usize) -> (V3, MoveResult) {
        let c = cfg();
        let dt = 1.0 / 60.0;
        let mut vy = 0.0f32;
        let mut grounded = true;
        let mut last = w.move_character(feet, H, [0.0; 3], &c, grounded);
        for _ in 0..steps {
            vy = if grounded { 0.0 } else { vy - GRAVITY * dt };
            let r = w.move_character(feet, H, [v[0] * dt, vy * dt, v[2] * dt], &c, grounded);
            feet = r.feet;
            grounded = r.grounded;
            last = r;
        }
        (feet, last)
    }

    #[test]
    fn default_is_generic_and_slope_cos_is_deterministic() {
        let c = cfg();
        assert_eq!((c.radius, c.step_height, c.max_slides), (0.3, 0.35, 4));
        assert!((c.min_ground_normal_y - core::f32::consts::FRAC_1_SQRT_2).abs() < 2e-6);
        assert_eq!(slope_cos(45.0).to_bits(), c.min_ground_normal_y.to_bits());
        assert!((slope_cos(0.0) - 1.0).abs() < 2e-6);
        assert!(slope_cos(90.0).abs() < 1e-6 && slope_cos(120.0) == slope_cos(90.0));
    }

    #[test]
    fn walks_on_floor_and_stops_at_wall() {
        let w = room().build();
        let (feet, r) = walk(&w, [0.0, 0.0, 0.0], [4.5, 0.0, 0.0], 120);
        assert!(r.grounded && r.hit_wall);
        assert!((feet[0] - (5.0 - cfg().radius - cfg().skin)).abs() < 2e-3, "{feet:?}");
        assert!(feet[1].abs() < 1e-3);
    }

    #[test]
    fn slides_along_wall_at_an_angle() {
        let w = room().build();
        // Diagonal into the wall: keeps the z part of the motion.
        let (feet, _) = walk(&w, [4.0, 0.0, 0.0], [3.0, 0.0, 3.0], 60);
        assert!(feet[0] < 5.0 - cfg().radius && feet[0] > 4.6, "{feet:?}");
        assert!(feet[2] > 2.9, "{feet:?}");
    }

    #[test]
    fn steps_up_low_ledge_but_not_high_one() {
        let mut b = room();
        b.aabb([1.0, 0.0, -2.0], [3.0, 0.3, 2.0], 2); // 0.30 m step (under 0.35)
        b.aabb([1.0, 0.0, 4.0], [3.0, 0.45, 8.0], 3); // 0.45 m ledge (over it)
        let w = b.build();
        let (feet, r) = walk(&w, [0.0, 0.0, 0.0], [3.0, 0.0, 0.0], 40);
        assert!(feet[0] > 1.5, "climbed the step: {feet:?}");
        assert!((feet[1] - 0.3).abs() < 0.02, "{feet:?}");
        assert!(r.grounded);
        let (feet, r) = walk(&w, [0.0, 0.0, 6.0], [3.0, 0.0, 0.0], 40);
        assert!(feet[0] < 1.0, "blocked by the ledge: {feet:?}");
        assert!(r.hit_wall && feet[1].abs() < 1e-3);
    }

    #[test]
    fn climbs_stairs_and_snaps_down_them() {
        let mut b = room();
        // Five 0.25 m steps, each 0.6 m deep, up along +x from x = 0.
        for k in 0..5 {
            let x0 = k as f32 * 0.6;
            b.aabb([x0, 0.0, -2.0], [4.5, (k + 1) as f32 * 0.25, 2.0], 4);
        }
        let w = b.build();
        let (feet, r) = walk(&w, [-1.0, 0.0, 0.0], [3.0, 0.0, 0.0], 90);
        assert!(r.grounded);
        assert!((feet[1] - 1.25).abs() < 0.02, "at the top: {feet:?}");
        // And back down without leaving the ground.
        let c = cfg();
        let mut f = feet;
        let mut airborne = 0;
        for _ in 0..90 {
            let r = w.move_character(f, H, [-3.0 / 60.0, 0.0, 0.0], &c, true);
            f = r.feet;
            if !r.grounded {
                airborne += 1;
            }
        }
        assert!(f[1].abs() < 1e-2, "at the bottom: {f:?}");
        assert_eq!(airborne, 0);
    }

    /// A slab tilted about z, its top surface rising along +x from x = 0
    /// (the deterministic sine and cosine, as a game would build it).
    fn ramp(b: &mut WorldBuilder, deg: f32, tag: u32) {
        let a = trig::radians(deg);
        let (len, thick) = (6.0, 0.4);
        let ax = [trig::cos(a), trig::sin(a), 0.0];
        let ay = [-trig::sin(a), trig::cos(a), 0.0];
        let az = [0.0, 0.0, 1.0];
        let centre = add(scale(ax, len * 0.5), scale(ay, -thick * 0.5));
        b.oriented_box(centre, [len * 0.5, thick * 0.5, 2.0], [ax, ay, az], tag);
    }

    #[test]
    fn walks_up_gentle_ramp_not_steep_one() {
        let mut b = room();
        ramp(&mut b, 20.0, 5);
        let w = b.build();
        let (feet, r) = walk(&w, [-1.0, 0.0, 0.0], [3.0, 0.0, 0.0], 80);
        assert!(r.grounded, "{r:?}");
        assert!(feet[1] > 0.8, "went up the ramp: {feet:?}");
        assert!(r.ground_normal[1] < 0.99, "stands on the slope: {r:?}");

        let mut b = room();
        ramp(&mut b, 60.0, 5);
        let w = b.build();
        let (feet, r) = walk(&w, [-1.0, 0.0, 0.0], [3.0, 0.0, 0.0], 80);
        assert!(feet[1] < 0.2, "the steep slope is a wall: {feet:?}");
        assert!(r.hit_wall, "{r:?}");
    }

    #[test]
    fn ceiling_stops_a_jump() {
        let mut b = room();
        b.aabb([-2.0, 2.0, -2.0], [2.0, 2.5, 2.0], 6);
        let w = b.build();
        let c = cfg();
        let r = w.move_character([0.0, 0.0, 0.0], H, [0.0, 3.0, 0.0], &c, false);
        assert!(r.hit_ceiling);
        assert!((r.feet[1] + H - 2.0).abs() < 0.01, "{r:?}");
    }

    #[test]
    fn falls_and_lands() {
        let w = room().build();
        let c = cfg();
        let mut f = [0.0, 3.0, 0.0];
        let mut vy = 0.0f32;
        let mut grounded = false;
        for _ in 0..120 {
            vy = if grounded { 0.0 } else { vy - GRAVITY / 60.0 };
            let r = w.move_character(f, H, [0.0, vy / 60.0, 0.0], &c, grounded);
            f = r.feet;
            grounded = r.grounded;
        }
        assert!(grounded && f[1].abs() < 0.01, "{f:?}");
    }

    #[test]
    fn walking_off_a_ledge_snaps_only_within_reach() {
        // A 0.2 m drop is within the snap distance: the body stays grounded.
        // A 1 m drop is not: it leaves the ground and the game's gravity
        // takes over.
        let mut b = WorldBuilder::new();
        b.aabb([-5.0, -1.0, -2.0], [0.0, 0.0, 2.0], 0);
        b.aabb([0.0, -1.0, -2.0], [5.0, -0.2, 2.0], 0);
        b.aabb([-5.0, -1.0, 4.0], [0.0, 0.0, 8.0], 0);
        b.aabb([0.0, -3.0, 4.0], [5.0, -1.0, 8.0], 0);
        let w = b.build();
        let c = cfg();
        let mut f = [-1.0, 0.0, 0.0];
        for _ in 0..60 {
            let r = w.move_character(f, H, [3.0 / 60.0, 0.0, 0.0], &c, true);
            assert!(r.grounded, "small drop keeps the ground: {r:?}");
            f = r.feet;
        }
        assert!((f[1] + 0.2).abs() < 1e-2, "{f:?}");
        let mut f = [-1.0, 0.0, 6.0];
        let mut left_ground = false;
        for _ in 0..60 {
            let r = w.move_character(f, H, [3.0 / 60.0, 0.0, 0.0], &c, true);
            f = r.feet;
            left_ground |= !r.grounded;
        }
        assert!(left_ground && f[1] > -0.5, "a big drop is not snapped: {f:?}");
    }

    #[test]
    fn spawned_inside_a_wall_is_pushed_out() {
        let w = room().build();
        let c = cfg();
        let r = w.move_character([5.1, 0.0, 0.0], H, [0.0; 3], &c, true);
        assert!(r.depenetrated > 0.0);
        assert!(!w.overlaps(&c.capsule(r.feet, H)));
    }

    #[test]
    fn moves_are_deterministic() {
        let mut b = room();
        ramp(&mut b, 25.0, 5);
        let w = b.build();
        let a = walk(&w, [-1.0, 0.0, 0.3], [2.7, 0.0, 1.1], 200);
        let b2 = walk(&w, [-1.0, 0.0, 0.3], [2.7, 0.0, 1.1], 200);
        assert_eq!(a, b2);
        assert_eq!(a.0.map(f32::to_bits), b2.0.map(f32::to_bits));
    }
}
