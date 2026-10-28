//! The static-collision proof (sandbox feature `collide-proof`; never in the
//! default build): a small triangle level built with
//! `evani_physics::collide::WorldBuilder` (a slab, three stairs up to a
//! platform, a ramp down, a wall that meets a back wall) and an upright
//! capsule that walks a scripted waypoint path through it with the
//! character mover, every fixed step, at the live `physics.collide.*`
//! values and the `physics.gravity` value.
//!
//! The walk: up the stairs (steps), across the platform, down the ramp
//! (ground snap), along the slab, then diagonally into the wall, sliding
//! along it into the corner with the back wall, where it stays pressed.
//! The capsule's colour is the tag a raycast straight down returns (slab
//! white, stairs orange, platform yellow, ramp blue, airborne red), so the
//! tagged raycast shows too. The lap restarts every [`LAP_TICKS`].
//! The level stands outside the spinner grid on the -z side and the camera
//! is held on it.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_physics::PhysicsParams;
use evani_physics::collide::trig;
use evani_physics::collide::{CollideParams, StaticWorld, V3, WorldBuilder};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// Where the level's origin stands: outside the grid on the -z side.
pub const AT: V3 = [0.0, 0.0, -36.0];
/// The held camera: above and in front of the level, looking at its middle.
pub const CAMERA: CameraPose = CameraPose {
    eye: [-1.0, 8.5, -24.5],
    target: [0.3, 0.5, -36.3],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 50.0,
};

/// Surface tags (the `u32` every triangle carries).
pub const TAG_SLAB: u32 = 1;
pub const TAG_STAIRS: u32 = 2;
pub const TAG_PLATFORM: u32 = 3;
pub const TAG_RAMP: u32 = 4;
pub const TAG_WALL: u32 = 5;

/// The level's boxes in local metres: `(min, max, tag, mesh, colour)`.
const BOXES: [(V3, V3, u32, MeshId, [f32; 4]); 7] = [
    ([-7.0, 0.0, -4.5], [7.0, 0.1, 4.5], TAG_SLAB, MeshId::CUBE, [0.80, 0.82, 0.86, 1.0]),
    ([-3.0, 0.1, -3.5], [-2.5, 0.35, -1.5], TAG_STAIRS, MeshId::BEVEL_CUBE, [0.95, 0.72, 0.48, 1.0]),
    ([-2.5, 0.1, -3.5], [-2.0, 0.6, -1.5], TAG_STAIRS, MeshId::BEVEL_CUBE, [0.95, 0.72, 0.48, 1.0]),
    ([-2.0, 0.1, -3.5], [-1.5, 0.85, -1.5], TAG_STAIRS, MeshId::BEVEL_CUBE, [0.95, 0.72, 0.48, 1.0]),
    ([-1.5, 0.1, -3.5], [2.0, 1.1, -1.5], TAG_PLATFORM, MeshId::CUBE, [0.92, 0.88, 0.62, 1.0]),
    ([-1.6, 0.1, 0.6], [-1.0, 1.6, 4.1], TAG_WALL, MeshId::CUBE, [0.70, 0.72, 0.80, 1.0]),
    ([-7.0, 0.1, 4.1], [7.0, 1.2, 4.5], TAG_WALL, MeshId::CUBE, [0.70, 0.72, 0.80, 1.0]),
];
/// The ramp: from the platform's edge (local x 2, top 1.1) down to the slab
/// (x 5, top 0.1), 2 m wide; its low end runs on into the slab.
const RAMP_TOP: [f32; 2] = [2.0, 1.1];
const RAMP_BOTTOM: [f32; 2] = [5.0, 0.1];
const RAMP_Z: f32 = -2.5;
const RAMP_HALF_WIDTH: f32 = 1.0;
const RAMP_HALF_THICK: f32 = 0.1;
const RAMP_BURY: f32 = 0.3;
const RAMP_COLOUR: [f32; 4] = [0.58, 0.78, 0.95, 1.0];

/// The capsule's start (feet, local).
pub const START: V3 = [-5.5, 0.1, -2.5];
/// The waypoints (local x, z). The last one is behind the wall, so the
/// capsule never reaches it: it slides along the wall into the corner.
pub const WAYPOINTS: [[f32; 2]; 3] = [[5.8, -2.5], [5.8, 0.0], [-5.0, 4.4]];
/// How close (m, flat) counts as reaching a waypoint.
const REACH: f32 = 0.15;
/// Ticks standing still before each lap's walk (1.5 s at 60 Hz).
pub const HOLD_TICKS: u32 = 90;
/// Ticks per lap (12 s at 60 Hz); then the capsule starts again.
pub const LAP_TICKS: u32 = 720;

/// Local level metres to world space. Local +z (the back wall's side) runs
/// away from the held camera, so the walls never hide the walk.
fn local(p: V3) -> V3 {
    [p[0] + AT[0], p[1] + AT[1], AT[2] - p[2]]
}

/// The ramp as an oriented box: `(centre, half sizes, axes, angle)` in
/// world space, angle about +z (radians, negative: it descends along +x).
fn ramp() -> (V3, V3, [V3; 3], f32) {
    let (dx, dy) = (RAMP_BOTTOM[0] - RAMP_TOP[0], RAMP_BOTTOM[1] - RAMP_TOP[1]);
    let len = (dx * dx + dy * dy).sqrt();
    let angle = trig::atan2(dy, dx);
    let (s, c) = (trig::sin(angle), trig::cos(angle));
    let ax = [c, s, 0.0];
    let ay = [-s, c, 0.0];
    let half_len = (len + RAMP_BURY) * 0.5;
    let mid = [RAMP_TOP[0] + ax[0] * half_len, RAMP_TOP[1] + ax[1] * half_len];
    let centre = local([mid[0] - ay[0] * RAMP_HALF_THICK, mid[1] - ay[1] * RAMP_HALF_THICK, RAMP_Z]);
    (centre, [half_len, RAMP_HALF_THICK, RAMP_HALF_WIDTH], [ax, ay, [0.0, 0.0, 1.0]], angle)
}

/// The level's collision world (world space).
pub fn level() -> StaticWorld {
    let mut b = WorldBuilder::new();
    for (min, max, tag, _, _) in BOXES {
        b.aabb(local(min), local(max), tag);
    }
    let (centre, half, axes, _) = ramp();
    b.oriented_box(centre, half, axes, TAG_RAMP);
    b.build()
}

/// The proof's state (a world resource).
#[derive(Debug, Clone)]
pub struct ProofCollide {
    pub world: StaticWorld,
    /// Feet of the capsule (world space).
    pub feet: V3,
    pub vy: f32,
    pub grounded: bool,
    /// Ticks into the current lap.
    pub tick: u32,
    pub laps: u32,
    /// The waypoint being walked to.
    pub next: usize,
    /// The tag a raycast straight down hits (0: none within 0.3 m).
    pub ground_tag: u32,
    /// What this lap has done: the tags stood on (bit per tag), steps
    /// climbed, highest feet (world y), touched a wall.
    pub tags_seen: u32,
    pub steps: u32,
    pub max_y: f32,
    pub hit_wall: bool,
}

impl Default for ProofCollide {
    fn default() -> Self {
        Self {
            world: level(),
            feet: local(START),
            vy: 0.0,
            grounded: true,
            tick: 0,
            laps: 0,
            next: 0,
            ground_tag: TAG_SLAB,
            tags_seen: 0,
            steps: 0,
            max_y: local(START)[1],
            hit_wall: false,
        }
    }
}

impl ProofCollide {
    fn restart(&mut self) {
        self.feet = local(START);
        self.vy = 0.0;
        self.grounded = true;
        self.tick = 0;
        self.laps += 1;
        self.next = 0;
        self.tags_seen = 0;
        self.steps = 0;
        self.max_y = self.feet[1];
        self.hit_wall = false;
    }
}

/// The fixed-step walker (`Stage::Update`).
pub fn walk_system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_collide.walk", Stage::Update, walk)
}

/// The `Stage::Frame` drawing (after the kit's, so its camera wins).
pub fn draw_system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_collide.draw", Stage::Frame, draw)
}

fn walk(world: &mut World) {
    let Some(params) = world.try_res::<CollideParams>().map(|p| *p) else { return };
    let gravity = world.try_res::<PhysicsParams>().map(|p| p.gravity);
    let dt = world.try_res::<evani_core::time::Time>().map_or(1.0 / 60.0, |t| t.fixed_dt);
    let (cfg, height, speed, g) = {
        let reg = world.res::<ParamRegistry>();
        (params.config(&reg), params.height(&reg), params.speed(&reg), gravity.map_or(-9.81, |h| reg.vec3(h)[1]))
    };
    let Some(mut p) = world.try_res_mut::<ProofCollide>() else { return };
    if p.tick >= LAP_TICKS {
        p.restart();
    }
    p.tick += 1;
    let mut flat = [0.0, 0.0];
    if p.tick > HOLD_TICKS {
        while let Some(w) = WAYPOINTS.get(p.next) {
            let to = [w[0] + AT[0] - p.feet[0], AT[2] - w[1] - p.feet[2]];
            let d = (to[0] * to[0] + to[1] * to[1]).sqrt();
            if d < REACH && p.next + 1 < WAYPOINTS.len() {
                p.next += 1;
                continue;
            }
            if d > 1e-4 {
                let step = if speed * dt < d { speed * dt } else { d };
                flat = [to[0] / d * step, to[1] / d * step];
            }
            break;
        }
    }
    p.vy = if p.grounded { 0.0 } else { p.vy + g * dt };
    let motion = [flat[0], p.vy * dt, flat[1]];
    let r = p.world.move_character(p.feet, height, motion, &cfg, p.grounded);
    p.feet = r.feet;
    p.grounded = r.grounded;
    p.steps += u32::from(r.stepped);
    p.hit_wall |= r.hit_wall;
    if p.feet[1] > p.max_y {
        p.max_y = p.feet[1];
    }
    let down = p.world.raycast([p.feet[0], p.feet[1] + 0.1, p.feet[2]], [0.0, -1.0, 0.0], 0.4);
    p.ground_tag = down.map_or(0, |h| h.tag);
    if p.ground_tag < 32 {
        p.tags_seen |= 1 << p.ground_tag;
    }
}

fn tag_colour(tag: u32) -> [f32; 4] {
    match tag {
        TAG_SLAB => [1.0, 1.0, 1.0, 1.0],
        TAG_STAIRS => [1.0, 0.55, 0.15, 1.0],
        TAG_PLATFORM => [1.0, 0.9, 0.2, 1.0],
        TAG_RAMP => [0.25, 0.6, 1.0, 1.0],
        _ => [1.0, 0.2, 0.2, 1.0],
    }
}

fn draw(world: &mut World) {
    let (stone, slate) = world
        .try_res::<LookData>()
        .map_or((0, 0), |l| (RenderWorld::material(&l, "stone"), RenderWorld::material(&l, "slate")));
    let (height, radius) = match (world.try_res::<CollideParams>(), world.try_res::<ParamRegistry>()) {
        (Some(p), Some(reg)) => (p.height(&reg), reg.f32(p.radius)),
        _ => (1.8, 0.3),
    };
    let Some(p) = world.try_res::<ProofCollide>().map(|p| (p.feet, p.ground_tag)) else { return };
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    for (min, max, tag, mesh, colour) in BOXES {
        let (a, b) = (local(min), local(max));
        let mut m = MeshInstance::new(mesh, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]);
        m.scale = [(b[0] - a[0]).abs(), (b[1] - a[1]).abs(), (b[2] - a[2]).abs()];
        m.material = if tag == TAG_WALL { slate } else { stone };
        m.color = colour;
        rw.draw_mesh(m);
    }
    let (centre, half, _, angle) = ramp();
    let mut m = MeshInstance::new(MeshId::CUBE, centre);
    m.rotation = [0.0, 0.0, trig::sin(angle * 0.5), trig::cos(angle * 0.5)];
    m.scale = [half[0] * 2.0, half[1] * 2.0, half[2] * 2.0];
    m.material = stone;
    m.color = RAMP_COLOUR;
    rw.draw_mesh(m);
    // The capsule: a cylinder between two spheres, coloured by its ground tag.
    let (feet, tag) = p;
    let r = if radius > 0.0 { radius } else { 0.3 };
    let body = if height > 2.0 * r { height - 2.0 * r } else { 0.0 };
    let colour = tag_colour(tag);
    let mut c = MeshInstance::new(MeshId::CYLINDER, [feet[0], feet[1] + r + body * 0.5, feet[2]]);
    c.scale = [2.0 * r, body, 2.0 * r];
    c.material = stone;
    c.color = colour;
    rw.draw_mesh(c);
    for y in [feet[1] + r, feet[1] + r + body] {
        let mut s = MeshInstance::new(MeshId::SPHERE, [feet[0], y, feet[2]]);
        s.scale = [2.0 * r; 3];
        s.material = stone;
        s.color = colour;
        rw.draw_mesh(s);
    }
    rw.camera = CAMERA;
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::ErrorLog;

    fn run(frames: u32) -> crate::App {
        let mut app = crate::build_app();
        app.run_headless(frames, 1000.0 / 60.0);
        app
    }

    /// The capsule climbs the stairs, crosses the platform, walks down the
    /// ramp, slides along the wall into the corner and stays pressed there,
    /// never inside a wall; the downward raycast names every surface it
    /// stood on; two runs give the same bits.
    #[test]
    fn capsule_walks_the_level() {
        let app = run(600);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let p = app.world.res::<ProofCollide>().clone();
        assert_eq!(p.laps, 0);
        assert!(p.tick > 500, "the walk ran: tick {}", p.tick);
        assert!(p.steps >= 3, "stepped up the three stairs: {}", p.steps);
        assert!(p.max_y > local([0.0, 1.09, 0.0])[1], "reached the platform: {}", p.max_y);
        for tag in [TAG_SLAB, TAG_STAIRS, TAG_PLATFORM, TAG_RAMP] {
            assert!(p.tags_seen & (1 << tag) != 0, "stood on tag {tag}: {:b}", p.tags_seen);
        }
        assert!(p.hit_wall && p.grounded);
        assert_eq!(p.ground_tag, TAG_SLAB);
        assert_eq!(p.next, WAYPOINTS.len() - 1, "on the last leg");
        // In the corner: against the wall's +x face and the back wall's -z
        // face, a radius (and the skin) away from each.
        let x = p.feet[0] - AT[0];
        let z = AT[2] - p.feet[2];
        assert!((x - (-1.0 + 0.3)).abs() < 0.02, "against the wall: x {x}");
        assert!((z - (4.1 - 0.3)).abs() < 0.02, "against the back wall: z {z}");
        assert!((p.feet[1] - local([0.0, 0.1, 0.0])[1]).abs() < 0.01, "on the slab: y {}", p.feet[1]);
        assert_eq!(app.world.res::<RenderWorld>().camera.eye, CAMERA.eye);

        let again = run(600);
        let q = again.world.res::<ProofCollide>().clone();
        assert_eq!(p.feet.map(f32::to_bits), q.feet.map(f32::to_bits), "deterministic");
    }

    /// The lap restarts at the start after [`LAP_TICKS`].
    #[test]
    fn lap_restarts() {
        let app = run(LAP_TICKS + 10);
        let p = app.world.res::<ProofCollide>();
        assert_eq!(p.laps, 1);
        assert!(p.tick <= HOLD_TICKS && p.feet == local(START));
    }
}
