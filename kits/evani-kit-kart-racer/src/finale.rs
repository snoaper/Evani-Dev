//! The end of the race: the finish celebration (confetti of wrapped sweets,
//! lit gate lights, a camera that eases from the chase view into a slow
//! orbit round the player) and the podium scene (three blocks, a candy-cane
//! arch, the top three drivers in their kart colours, a fixed three-quarter
//! low camera). Pure functions of the race clock, so a held capture shows
//! the same frame every time; the draw system calls [`draw`] and the
//! camera functions, the race flow only needs [`CELEBRATE_S`],
//! [`ROLL_OUT_BRAKE`] and [`podium_blocks`].
//!
//! Built only from the engine's primitives (bevel cubes, cylinders,
//! spheres, cones) in the look's materials, so the toon bands and ink
//! outlines apply as everywhere else.

use evani_core::num::smooth01;
use evani_core::camera::CameraPose;
use evani_render::math::{quat_axis_angle, quat_mul};
use evani_render::{MeshId, RenderWorld};

use crate::draw::{Mat, add, inst, role, rot, yaw_q};
pub use crate::finish::{CELEBRATE_S, ROLL_OUT_BRAKE, Stage, stage};
use crate::race::RaceView;
use crate::track::{Track, hash01};

// ---- timeline ---------------------------------------------------------------------


/// Seconds the chase view takes to ease into the orbit.
pub const ORBIT_BLEND_S: f32 = 1.5;
/// Seconds the orbit takes to reach its full turning rate.
pub const ORBIT_RAMP_S: f32 = 1.2;
/// Turning rate of the slow orbit (rad/s).
pub const ORBIT_RATE: f32 = 0.9;
/// Orbit radius (m), camera height over the kart's ground (m) and the
/// height of the look-at point (m; above the kart, so Pip sits low in the
/// frame under the banner and the big place).
pub const ORBIT_RADIUS: f32 = 9.5;
pub const ORBIT_HEIGHT: f32 = 3.2;
pub const ORBIT_LOOK_Y: f32 = 2.4;

/// How far round the kart the orbit has turned `since` seconds after the
/// line (rad): it starts at rest and eases up to [`ORBIT_RATE`].
pub fn orbit_angle(since: f32) -> f32 {
    let s = since.max(0.0);
    if s < ORBIT_RAMP_S {
        ORBIT_RATE * s * s / (2.0 * ORBIT_RAMP_S)
    } else {
        ORBIT_RATE * (s - ORBIT_RAMP_S * 0.5)
    }
}

/// The chase camera's inputs (the kart's anchor point, the camera's smoothed
/// heading and the kit's chase tunables).
#[derive(Debug, Clone, Copy)]
pub struct Chase {
    pub anchor: [f32; 3],
    pub yaw: f32,
    pub distance: f32,
    pub height: f32,
    pub look_ahead: f32,
}

impl Chase {
    /// The plain chase pose: `distance` behind, `height` up, looking
    /// `look_ahead` ahead.
    pub fn pose(&self) -> ([f32; 3], [f32; 3]) {
        let f = [self.yaw.sin(), 0.0, self.yaw.cos()];
        let a = self.anchor;
        (
            [a[0] - f[0] * self.distance, a[1] + self.height, a[2] - f[2] * self.distance],
            [a[0] + f[0] * self.look_ahead, a[1] + 1.0, a[2] + f[2] * self.look_ahead],
        )
    }
}

/// The camera `since` seconds after the player crossed the line: the chase
/// pose at 0, easing (over [`ORBIT_BLEND_S`]) into a slow orbit round the
/// kart. No jump: at 0 it is exactly [`Chase::pose`].
pub fn finish_camera(c: &Chase, since: f32) -> ([f32; 3], [f32; 3]) {
    let e = smooth01(since / ORBIT_BLEND_S);
    let lerp = |a: f32, b: f32| a + (b - a) * e;
    let r = lerp(c.distance, ORBIT_RADIUS);
    let h = lerp(c.height, ORBIT_HEIGHT);
    // The chase eye is behind the kart (-forward); the orbit turns it round.
    let phi = orbit_angle(since);
    let back = [-c.yaw.sin(), -c.yaw.cos()];
    let (s, co) = phi.sin_cos();
    // The same turn as `draw::rot`.
    let turned = [back[0] * co + back[1] * s, -back[0] * s + back[1] * co];
    let a = c.anchor;
    let eye = [a[0] + turned[0] * r, a[1] + h, a[2] + turned[1] * r];
    let look = lerp(c.look_ahead, 0.0);
    let f = [c.yaw.sin(), c.yaw.cos()];
    let target = [a[0] + f[0] * look, a[1] + lerp(1.0, ORBIT_LOOK_Y), a[2] + f[1] * look];
    (eye, target)
}

// ---- stages -----------------------------------------------------------------------

// ---- the podium site --------------------------------------------------------------

/// Where the podium stands and which way it faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    /// The middle of the podium's footprint on the meadow.
    pub pos: [f32; 3],
    /// Heading of the podium's front (the way the drivers look), as
    /// `atan2(x, z)`.
    pub yaw: f32,
}

/// Distance of the camera in front of the podium (m).
pub const CAM_DISTANCE: f32 = 17.0;
/// Turn of the camera away from straight in front (rad): three-quarter.
pub const CAM_AZIMUTH: f32 = 0.5;
/// Camera height over the meadow (m): a slight low angle.
pub const CAM_HEIGHT: f32 = 1.5;
/// Height of the point the camera looks at over the meadow (m).
pub const CAM_LOOK_Y: f32 = 3.3;
/// Vertical field of view of the podium camera (deg).
pub const CAM_FOV: f32 = 52.0;
/// Where the podium sits on the screen (normalised device x, y) beside the
/// results panel: left of centre on a wide screen, high on a tall one.
pub fn podium_screen_pos(aspect: f32) -> [f32; 2] {
    if aspect >= 1.2 { [-0.42, 0.0] } else { [0.0, 0.42] }
}
/// Props this close to the podium (m), or to the line from it to the
/// camera, are left out of the layout.
pub const CLEAR_RADIUS: f32 = 13.0;

/// Heading of the sun over the map (`atan2(x, z)`): the kit's look (toon)
/// lights the world from `render.light.direction` [0.62, 0.85, 0.28]. The
/// podium faces it, so the drivers and the gold, silver and bronze blocks
/// are lit from the camera's side (the camera stands [`CAM_AZIMUTH`] off
/// the podium's front), never backlit.
pub const SUN_YAW: f32 = 1.146_601_9;

/// The podium's place: on the meadow inside the circuit, just past the
/// start line, facing the sun (see [`SUN_YAW`]).
pub fn podium_site(track: &Track, verge: f32) -> Site {
    let s = 24.0;
    let side = infield_side(track, s);
    let lat = side * (track.half_width + verge + 30.0);
    let p = track.point_at(s, lat);
    Site {
        pos: [p[0], track.meadow_y, p[2]],
        yaw: SUN_YAW,
    }
}

/// +1 (left) or -1 (right) of the track at `s`: the side that faces the
/// middle of the circuit.
fn infield_side(track: &Track, s: f32) -> f32 {
    let b = track.bounds;
    let c = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
    let d = |lat: f32| {
        let p = track.point_at(s, lat);
        (p[0] - c[0]).powi(2) + (p[2] - c[1]).powi(2)
    };
    if d(10.0) < d(-10.0) { 1.0 } else { -1.0 }
}

/// Whether a prop standing at `p` would stand in the podium's way: on its
/// footprint, or in the camera's view of it.
pub fn blocks_podium(site: &Site, p: [f32; 3]) -> bool {
    let near = |a: [f32; 3], r: f32| (a[0] - p[0]).hypot(a[2] - p[2]) < r;
    let eye = camera_eye(site);
    near(site.pos, CLEAR_RADIUS) || near(eye, 4.0) || near([(site.pos[0] + eye[0]) * 0.5, 0.0, (site.pos[2] + eye[2]) * 0.5], CLEAR_RADIUS * 0.55)
}

/// Where the podium camera stands.
pub(crate) fn camera_eye(site: &Site) -> [f32; 3] {
    let o = rot(site.yaw + CAM_AZIMUTH, [0.0, 0.0, CAM_DISTANCE]);
    [site.pos[0] + o[0], site.pos[1] + CAM_HEIGHT, site.pos[2] + o[2]]
}

/// The fixed podium camera. `aspect` is the canvas's width over its height:
/// the view is shifted so the podium sits in [`podium_screen_pos`], clear of
/// the results panel.
pub fn podium_camera(site: &Site, aspect: f32) -> CameraPose {
    let eye = camera_eye(site);
    let target = [site.pos[0], site.pos[1] + CAM_LOOK_Y, site.pos[2]];
    shifted_pose(eye, target, CAM_FOV, aspect)
}

/// A pose looking from `eye` at `target`, shifted so the target sits in
/// [`podium_screen_pos`] (clear of the results panel).
fn shifted_pose(eye: [f32; 3], target: [f32; 3], fov: f32, aspect: f32) -> CameraPose {
    let f = norm3([target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]]);
    // Right = forward x up; looking `shift` further right puts the podium
    // that far left of the centre.
    let right = norm3([-f[2], 0.0, f[0]]);
    let th = (fov.to_radians() * 0.5).tan();
    let [px, py] = podium_screen_pos(aspect);
    let k_right = -px * th * aspect;
    let k_up = -py * th;
    let g = norm3([f[0] + right[0] * k_right, f[1] + k_up, f[2] + right[2] * k_right]);
    CameraPose {
        eye,
        target: [eye[0] + g[0] * 30.0, eye[1] + g[1] * 30.0, eye[2] + g[2] * 30.0],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

/// The finale close-up's tunables (`kit.kart_racer.finale_cam_*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloseUp {
    /// Seconds the wide shot holds before the move starts.
    pub hold_s: f32,
    /// Seconds the move takes.
    pub move_s: f32,
    /// Camera distance in front of the podium (m).
    pub distance: f32,
    /// Turn away from straight in front (rad).
    pub azimuth: f32,
    /// Camera height over the meadow (m).
    pub height: f32,
    /// Height of the look-at point over the meadow (m).
    pub look_y: f32,
    /// Vertical field of view (deg).
    pub fov: f32,
}

/// The podium camera `since` seconds after the podium scene appeared: the
/// wide shot ([`podium_camera`]) for `hold_s`, then an eased move into the
/// winners close-up. Pure in `since`, so a held frame is the same every time;
/// at the start it is exactly [`podium_camera`] (no jump). Under free roam it
/// stays on the wide shot (there is no finale to frame).
pub fn podium_closeup_camera(site: &Site, aspect: f32, since: f32, c: &CloseUp, free_roam: bool) -> CameraPose {
    let e = smooth01((since - c.hold_s) / c.move_s.max(1e-3));
    if free_roam || e <= 0.0 {
        return podium_camera(site, aspect);
    }
    let o = rot(site.yaw + c.azimuth, [0.0, 0.0, c.distance]);
    let close_eye = [site.pos[0] + o[0], site.pos[1] + c.height, site.pos[2] + o[2]];
    let close_target = [site.pos[0], site.pos[1] + c.look_y, site.pos[2]];
    let wide_eye = camera_eye(site);
    let wide_target = [site.pos[0], site.pos[1] + CAM_LOOK_Y, site.pos[2]];
    let l = |a: [f32; 3], b: [f32; 3]| [a[0] + (b[0] - a[0]) * e, a[1] + (b[1] - a[1]) * e, a[2] + (b[2] - a[2]) * e];
    shifted_pose(l(wide_eye, close_eye), l(wide_target, close_target), CAM_FOV + (c.fov - CAM_FOV) * e, aspect)
}

fn norm3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

// ---- the blocks and who stands on them --------------------------------------------

/// One podium block: the place it is for (0 = first), its centre's offset
/// across the podium (m; negative is the viewer's left) and its height (m).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub place: usize,
    pub x: f32,
    pub height: f32,
}

/// Width and depth of a block (m).
pub const BLOCK_W: f32 = 3.2;
pub const BLOCK_D: f32 = 3.0;

/// The three blocks: second on the viewer's left, first in the middle and
/// taller than both, third on the right.
pub fn podium_blocks() -> [Block; 3] {
    [
        Block { place: 1, x: -3.5, height: 1.5 },
        Block { place: 0, x: 0.0, height: 2.2 },
        Block { place: 2, x: 3.5, height: 1.0 },
    ]
}

/// The kart indices of the top three in place order (`None` for a field
/// smaller than the podium), from the race order (kart index by place).
pub fn podium_order(order: &[usize]) -> [Option<usize>; 3] {
    [order.first().copied(), order.get(1).copied(), order.get(2).copied()]
}

// ---- confetti ---------------------------------------------------------------------

/// Sweets in the pool for a tier's per-frame triangle limit: each is a body
/// and two twists (60 triangles), so even the largest pool is a few
/// thousand triangles and far under every tier's particle cap (2 000 on Low).
pub fn confetti_count(max_triangles: u32) -> usize {
    match max_triangles {
        0..=149_999 => 90,
        150_000..=999_999 => 140,
        1_000_000..=3_999_999 => 240,
        _ => 360,
    }
}

/// Triangles one sweet draws (a cube body and two cone twists).
pub const SWEET_TRIANGLES: u32 = 12 + 24 + 24;
/// Seconds over which the pool is launched.
pub const EMIT_S: f32 = 2.0;
/// Air drag (1/s) and terminal fall speed (m/s) of a sweet.
const DRAG: f32 = 1.2;
const TERMINAL: f32 = 3.0;
/// Seconds a landed sweet stays when the pool repeats.
const LANDED_S: f32 = 0.6;

/// Wrapper colours (linear, as the scenery's candy): pink, mint, lilac,
/// lemon, sky, cherry.
const WRAPPERS: [[f32; 3]; 6] = [
    [1.0, 0.08, 0.37],
    [0.35, 0.85, 0.62],
    [0.55, 0.3, 1.0],
    [1.0, 0.7, 0.08],
    [0.25, 0.78, 1.0],
    [0.88, 0.08, 0.15],
];

/// Where a sweet comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Launch {
    /// Cannons at the gate's two posts (centre of the line, forward unit
    /// vector on the ground, lateral unit vector, distance of the posts
    /// from the middle) and a rain over `rain` (centre, radius, height).
    pub line: [f32; 3],
    pub forward: [f32; 2],
    pub posts: f32,
    pub rain: [f32; 3],
    pub rain_radius: f32,
    pub rain_height: f32,
    pub ground: f32,
    /// Whether the pool launches again once a sweet has landed.
    pub repeat: bool,
}

/// One sweet in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweet {
    pub pos: [f32; 3],
    pub q: [f32; 4],
    pub color: [f32; 3],
    pub landed: bool,
}

/// Sweet `i` of a pool of `n`, `since` seconds after the first launch.
/// `None` before its launch (and, when the pool repeats, after it has
/// landed). Two in three come from the gate's cannons, the rest rain over
/// `rain`. A closed form of the launch, so a held frame is the same every
/// time and nothing is allocated.
pub fn sweet(i: usize, n: usize, since: f32, l: &Launch) -> Option<Sweet> {
    let h = |salt: u32| hash01(i as u32, salt);
    let delay = (i as f32 + h(1) * 0.5) / n.max(1) as f32 * EMIT_S;
    let mut age = since - delay;
    if age < 0.0 {
        return None;
    }
    let (origin, vel);
    let tang = l.forward;
    let lateral = [tang[1], -tang[0]];
    match i % 3 {
        0 | 1 => {
            // A cannon at each post, shooting up, in and along the road.
            let side = if i.is_multiple_of(3) { 1.0 } else { -1.0 };
            origin = [l.line[0] + lateral[0] * side * l.posts, l.ground + 1.0, l.line[2] + lateral[1] * side * l.posts];
            let up = 15.0 + 6.0 * h(2);
            let along = 8.0 + 14.0 * h(3);
            let inward = -side * (2.0 + 6.0 * h(4));
            vel = [tang[0] * along + lateral[0] * inward, up, tang[1] * along + lateral[1] * inward];
        }
        _ => {
            // Rain: a drift of sweets let go over the finish area.
            let a = h(5) * core::f32::consts::TAU;
            let rr = l.rain_radius * h(6).sqrt();
            origin = [l.rain[0] + a.cos() * rr, l.ground + l.rain_height * (0.8 + 0.4 * h(7)), l.rain[2] + a.sin() * rr];
            vel = [(h(8) - 0.5) * 2.0, 0.0, (h(9) - 0.5) * 2.0];
        }
    }
    let fall = |t: f32| origin[1] - TERMINAL * t + (vel[1] + TERMINAL) * (1.0 - (-DRAG * t).exp()) / DRAG;
    let rest = |t: f32| {
        let k = (1.0 - (-DRAG * t).exp()) / DRAG;
        [origin[0] + vel[0] * k, origin[2] + vel[2] * k]
    };
    // When it lands: after the peak the height only falls, so bisect there.
    let floor = l.ground + 0.1;
    let peak = if vel[1] > 0.0 { ((vel[1] + TERMINAL) / TERMINAL).ln() / DRAG } else { 0.0 };
    let (mut lo, mut hi) = (peak, peak + 40.0);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if fall(mid) > floor {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let land = hi;
    if l.repeat {
        let period = EMIT_S + land + LANDED_S;
        age = age.rem_euclid(period);
        if age > land + LANDED_S {
            return None;
        }
    }
    let landed = age >= land;
    let t = age.min(land);
    let xz = rest(t);
    // A little flutter while falling.
    let flutter = if landed { 0.0 } else { 0.35 * (age * 5.0 + h(10) * core::f32::consts::TAU).sin() };
    let pos = [xz[0] + flutter * 0.5, if landed { floor } else { fall(t) }, xz[1] + flutter * 0.3];
    let axis = [h(11) - 0.5, h(12) + 0.2, h(13) - 0.5];
    let spin = (3.0 + 6.0 * h(14)) * if landed { land } else { age };
    let q = if landed { yaw_q(h(15) * core::f32::consts::TAU) } else { quat_axis_angle(axis, spin) };
    let color = WRAPPERS[(h(16) * WRAPPERS.len() as f32) as usize % WRAPPERS.len()];
    Some(Sweet { pos, q, color, landed })
}

/// Draw one wrapped sweet: a cube body and a cone twist at each end.
pub fn draw_sweet(s: &Sweet, mats: &[Mat; 16], rw: &mut RenderWorld) {
    rw.draw_mesh(inst(MeshId::CUBE, s.pos, s.q, [0.36, 0.22, 0.22], mats[role::KART_BODY], s.color));
    for sign in [1.0f32, -1.0] {
        let tip = quat_axis_angle([0.0, 0.0, 1.0], -sign * core::f32::consts::FRAC_PI_2);
        let at = add(s.pos, quat_rotate(s.q, [sign * 0.27, 0.0, 0.0]));
        rw.draw_mesh(inst(MeshId::CONE, at, quat_mul(s.q, tip), [0.22, 0.2, 0.22], mats[role::KERB_WHITE], [1.0; 3]));
    }
}

fn quat_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    // v' = v + 2w (u x v) + 2 u x (u x v)
    let u = [q[0], q[1], q[2]];
    let c = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = c(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let w = c(u, t);
    [v[0] + q[3] * t[0] + w[0], v[1] + q[3] * t[1] + w[1], v[2] + q[3] * t[2] + w[2]]
}

/// The gate's cannons and the rain over the stopping place, for a track.
pub fn finish_launch(track: &Track) -> Launch {
    let line = track.point_at(0.0, 0.0);
    let tan = track.tangent_at(0.0);
    let stop = track.point_at(14.0, 0.0);
    Launch {
        line,
        forward: tan,
        posts: track.half_width + crate::draw::GANTRY_POST_OUT + 0.6,
        rain: stop,
        rain_radius: 8.0,
        rain_height: 9.0,
        ground: line[1],
        repeat: false,
    }
}

/// Rain over the podium that goes on while the results show.
pub fn podium_launch(site: &Site) -> Launch {
    Launch {
        line: site.pos,
        forward: [0.0, 1.0],
        posts: 0.0,
        rain: site.pos,
        rain_radius: 9.0,
        rain_height: 11.0,
        ground: site.pos[1],
        repeat: true,
    }
}

// ---- lights -----------------------------------------------------------------------

/// Whether gate light `i` shows gold (rather than green) `since` seconds
/// after the line: the three flash in turn, never dark.
pub fn light_is_gold(i: usize, since: f32) -> bool {
    (((since * 3.0).max(0.0) as usize) + i) % 2 == 1
}

// ---- drawing ----------------------------------------------------------------------

/// A bevel cube / cylinder part in a podium figure's frame (x left, y up, z
/// forward, scaled by `s`).
struct Frame {
    pos: [f32; 3],
    yaw: f32,
    s: f32,
}

impl Frame {
    #[allow(clippy::too_many_arguments)]
    fn part(&self, rw: &mut RenderWorld, mesh: MeshId, o: [f32; 3], lq: [f32; 4], size: [f32; 3], m: Mat, c: [f32; 3]) {
        let at = add(self.pos, rot(self.yaw, [o[0] * self.s, o[1] * self.s, o[2] * self.s]));
        rw.draw_mesh(inst(mesh, at, quat_mul(yaw_q(self.yaw), lq), [size[0] * self.s, size[1] * self.s, size[2] * self.s], m, c));
    }
}

const NO_ROT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const INK: [f32; 3] = [0.024, 0.006, 0.051];
const GOLD: [f32; 3] = [1.0, 0.7, 0.08];
const SILVER: [f32; 3] = [0.62, 0.72, 0.95];
const BRONZE: [f32; 3] = [1.0, 0.33, 0.04];

/// The seven-segment bars of digits 1 to 3 (top, top right, bottom right,
/// bottom, bottom left, top left, middle).
fn digit_segments(n: usize) -> &'static [usize] {
    match n {
        1 => &[1, 2],
        2 => &[0, 1, 6, 4, 3],
        _ => &[0, 1, 6, 2, 3],
    }
}

/// Everything the end of the race adds to the frame: the confetti and the
/// lit gate for a finished human (the gate lights are drawn by the start
/// light code), and on the podium stage the podium with its drivers and the
/// rain of sweets. `budget` is the tier's per-frame triangle limit.
pub fn draw(race: &RaceView, site: &Site, mats: &[Mat; 16], budget: u32, time: f32, shadows: bool, rw: &mut RenderWorld) {
    let n = confetti_count(budget);
    match stage(race.phase, race.phase_time, race.players) {
        Stage::Race => {
            for p in 0..race.players.min(2) {
                if let Some(since) = since_finish(race, p) {
                    burst(race, since, n / race.players.clamp(1, 2), mats, rw);
                }
            }
        }
        Stage::Celebrate => {
            for p in 0..race.players.min(2) {
                if let Some(since) = since_finish(race, p) {
                    burst(race, since, n / race.players.clamp(1, 2), mats, rw);
                }
            }
        }
        Stage::Podium => {
            let t = (race.phase_time - CELEBRATE_S).max(0.0);
            draw_podium(race, site, mats, t, shadows, rw);
            let l = podium_launch(site);
            for i in 0..n {
                if let Some(s) = sweet(i, n, t + EMIT_S + 0.5, &l) {
                    draw_sweet(&s, mats, rw);
                }
            }
        }
    }
    let _ = time;
}

/// Seconds since local player `p` crossed the line, if they have.
pub fn since_finish(race: &RaceView, p: usize) -> Option<f32> {
    let k = &race.karts[race.player_kart(p)?];
    k.finished.map(|f| (race.clock - f).max(0.0))
}

fn burst(race: &RaceView, since: f32, n: usize, mats: &[Mat; 16], rw: &mut RenderWorld) {
    let l = finish_launch(&race.track);
    for i in 0..n {
        if let Some(s) = sweet(i, n, since, &l) {
            draw_sweet(&s, mats, rw);
        }
    }
}

/// The podium: blocks with numerals, a candy-cane arch behind them and the
/// top three drivers cheering on their blocks.
fn draw_podium(race: &RaceView, site: &Site, mats: &[Mat; 16], t: f32, shadows: bool, rw: &mut RenderWorld) {
    let f = Frame { pos: site.pos, yaw: site.yaw, s: 1.0 };
    let bc = MeshId::BEVEL_CUBE;
    let body = mats[role::KART_BODY];
    let order = podium_order(&race.order);
    if !shadows {
        // One soft disc under the whole podium where there is no shadow map.
        rw.draw_blob(add(site.pos, [0.0, 0.0, 0.0]), 6.0);
    }
    for b in podium_blocks() {
        let tint = [GOLD, SILVER, BRONZE][b.place];
        let lighter = [0.35 + tint[0] * 0.65, 0.35 + tint[1] * 0.65, 0.35 + tint[2] * 0.65];
        f.part(rw, bc, [b.x, b.height * 0.5, 0.0], NO_ROT, [BLOCK_W, b.height, BLOCK_D], body, tint);
        f.part(rw, bc, [b.x, b.height + 0.06, 0.0], NO_ROT, [BLOCK_W + 0.2, 0.14, BLOCK_D + 0.2], body, lighter);
        // The numeral on the front face.
        let dh = 0.42 * b.height + 0.1;
        let (w, th) = (dh * 0.55, dh * 0.16);
        let seg = |rw: &mut RenderWorld, k: usize| {
            let (o, size) = match k {
                0 => ([0.0, dh * 0.5, 0.0], [w, th, 0.1]),
                1 => ([w * 0.5, dh * 0.25, 0.0], [th, dh * 0.5, 0.1]),
                2 => ([w * 0.5, -dh * 0.25, 0.0], [th, dh * 0.5, 0.1]),
                3 => ([0.0, -dh * 0.5, 0.0], [w, th, 0.1]),
                4 => ([-w * 0.5, -dh * 0.25, 0.0], [th, dh * 0.5, 0.1]),
                5 => ([-w * 0.5, dh * 0.25, 0.0], [th, dh * 0.5, 0.1]),
                _ => ([0.0, 0.0, 0.0], [w, th, 0.1]),
            };
            f.part(rw, bc, [b.x + o[0], b.height * 0.5 + o[1], BLOCK_D * 0.5 + 0.04], NO_ROT, size, body, INK);
        };
        for &k in digit_segments(b.place + 1) {
            seg(rw, k);
        }
        // The driver on top.
        if let Some(Some(kart)) = order.get(b.place) {
            let spec = &race.roster[race.karts[*kart].spec];
            let hop = if b.place == 0 { 0.28 * (t * 7.0).sin().abs() } else { 0.07 * (t * 5.0 + b.place as f32 * 2.0).sin().abs() };
            let top = add(site.pos, rot(site.yaw, [b.x, b.height + 0.13 + hop, 0.0]));
            if !shadows {
                rw.draw_blob([top[0], site.pos[1] + b.height + 0.13, top[2]], 1.1);
            }
            draw_driver(top, site.yaw, spec.color, spec.suit, t + b.place as f32 * 0.37, b.place == 0, mats, rw);
        }
    }
    // A candy-cane arch behind the blocks.
    let (rx, ry, base, z) = (5.9f32, 3.7f32, 3.0f32, -2.2f32);
    let red = mats[role::KERB_RED];
    let white = mats[role::KERB_WHITE];
    for x in [rx, -rx] {
        for k in 0..4 {
            let m = if k % 2 == 0 { red } else { white };
            f.part(rw, MeshId::CYLINDER, [x, 0.375 + 0.75 * k as f32, z], NO_ROT, [0.56, 0.75, 0.56], m, [1.0; 3]);
        }
    }
    let segs = 20;
    let point = |k: usize| {
        let a = core::f32::consts::PI * k as f32 / segs as f32;
        [rx * a.cos(), base + ry * a.sin()]
    };
    for k in 0..segs {
        let (a, b) = (point(k), point(k + 1));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        let lq = quat_axis_angle([0.0, 0.0, 1.0], (-dx).atan2(dy));
        let m = if k % 2 == 0 { red } else { white };
        f.part(rw, MeshId::CYLINDER, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, z], lq, [0.56, len * 1.08, 0.56], m, [1.0; 3]);
    }
}

/// One driver standing on a block, cheering: round legs and an egg-shaped
/// torso in the kart colour, arms up in the contrasting suit, a round helmet in the
/// kart colour lightened with a visor, a stripe and a crest, so the head
/// reads as a volume with bands.
#[allow(clippy::too_many_arguments)]
fn draw_driver(pos: [f32; 3], yaw: f32, col: [f32; 3], suit: Option<[f32; 3]>, t: f32, winner: bool, mats: &[Mat; 16], rw: &mut RenderWorld) {
    let f = Frame { pos, yaw, s: 1.2 };
    let body = mats[role::KART_BODY];
    let dark = mats[role::WHEEL];
    let bc = MeshId::BEVEL_CUBE;
    let lighter = [0.35 + col[0] * 0.65, 0.35 + col[1] * 0.65, 0.35 + col[2] * 0.65];
    let deeper = [col[0] * 0.72, col[1] * 0.72, col[2] * 0.72];
    let suit = suit.unwrap_or([0.25 + col[0] * 0.5, 0.25 + col[1] * 0.5, 0.25 + col[2] * 0.5]);
    // Round legs on rounded boots, a short belted hip and an egg-shaped
    // torso: soft toy volumes, not boxes.
    for x in [0.21f32, -0.21] {
        f.part(rw, MeshId::CYLINDER, [x, 0.42, 0.0], NO_ROT, [0.3, 0.62, 0.3], body, deeper);
        f.part(rw, MeshId::LOW_SPHERE, [x, 0.1, 0.07], NO_ROT, [0.36, 0.22, 0.52], dark, [1.0; 3]);
    }
    f.part(rw, MeshId::CYLINDER, [0.0, 0.76, 0.0], NO_ROT, [0.66, 0.2, 0.46], body, suit);
    f.part(rw, MeshId::SPHERE, [0.0, 1.08, 0.0], NO_ROT, [0.82, 0.84, 0.58], body, col);
    // Arms up and out, waving; the winner punches straight up.
    for (j, side) in [1.0f32, -1.0].into_iter().enumerate() {
        let wave = 0.22 * (t * 6.0 + j as f32 * 2.2).sin();
        let a = if winner { 0.18 + wave * 0.5 } else { 0.55 + wave };
        let arm_q = quat_axis_angle([0.0, 0.0, 1.0], -side * a);
        let shoulder = [side * 0.5, 1.28];
        let (sa, ca) = (a.sin(), a.cos());
        f.part(rw, MeshId::CYLINDER, [shoulder[0] + side * sa * 0.34, shoulder[1] + ca * 0.34, 0.0], arm_q, [0.2, 0.68, 0.2], body, suit);
        f.part(rw, MeshId::LOW_SPHERE, [shoulder[0] + side * sa * 0.74, shoulder[1] + ca * 0.74, 0.0], NO_ROT, [0.28, 0.28, 0.28], body, [0.95; 3]);
    }
    f.part(rw, MeshId::SPHERE, [0.0, 1.84, 0.0], NO_ROT, [0.92, 0.88, 0.96], body, lighter);
    f.part(rw, bc, [0.0, 1.84, 0.36], NO_ROT, [0.66, 0.26, 0.26], body, [0.1, 0.13, 0.26]);
    f.part(rw, MeshId::LOW_SPHERE, [0.0, 1.86, -0.02], NO_ROT, [0.24, 0.92, 0.98], body, suit);
    f.part(rw, MeshId::LOW_SPHERE, [0.0, 2.34, -0.04], NO_ROT, [0.28, 0.28, 0.28], body, suit);
}

#[cfg(test)]
mod tests {
    use crate::race::Phase;
    use super::*;
    use crate::race::RaceView;
    use evani_render::math::{look_at, mul, perspective, transform_point};

    fn track() -> Track {
        Track::from_json(include_str!("../data/track.json")).unwrap()
    }

    /// Screen position (x, y in -1..1) and depth of a point in a pose.
    fn project(p: &CameraPose, aspect: f32, q: [f32; 3]) -> [f32; 3] {
        let v = look_at(p.eye, p.target, p.up);
        let pr = perspective(p.fov_y_deg.to_radians(), aspect, 0.1, 1000.0);
        transform_point(&mul(&pr, &v), q)
    }

    #[test]
    fn the_podium_is_second_first_third_left_to_right_with_first_tallest() {
        let blocks = podium_blocks();
        let site = podium_site(&track(), 7.0);
        let cam = podium_camera(&site, 16.0 / 9.0);
        let x = |b: &Block| {
            let o = rot(site.yaw, [b.x, b.height, 0.0]);
            project(&cam, 16.0 / 9.0, [site.pos[0] + o[0], site.pos[1] + o[1], site.pos[2] + o[2]])[0]
        };
        let by_place = |p: usize| *blocks.iter().find(|b| b.place == p).unwrap();
        let (first, second, third) = (by_place(0), by_place(1), by_place(2));
        assert!(x(&second) < x(&first) && x(&first) < x(&third), "second is on the viewer's left");
        assert!(first.height > second.height && second.height > third.height);
        let mut places: Vec<_> = blocks.iter().map(|b| b.place).collect();
        places.sort();
        assert_eq!(places, [0, 1, 2]);
    }

    #[test]
    fn the_top_three_stand_on_the_blocks_in_place_order() {
        assert_eq!(podium_order(&[5, 2, 7, 0, 1]), [Some(5), Some(2), Some(7)]);
        assert_eq!(podium_order(&[4]), [Some(4), None, None]);
        assert_eq!(podium_order(&[]), [None, None, None]);
    }

    #[test]
    fn the_podium_sits_left_of_the_results_panel_and_is_fully_in_frame() {
        let site = podium_site(&track(), 7.0);
        for (w, h) in [(1920.0f32, 1080.0f32), (844.0, 390.0)] {
            let aspect = w / h;
            let cam = podium_camera(&site, aspect);
            // The arch's top and feet, and the outer blocks' sides.
            let pts: [[f32; 3]; 5] = [[0.0, 7.0, -2.2], [-5.9, 0.0, -2.2], [5.9, 0.0, -2.2], [-5.1, 0.0, 0.0], [5.1, 0.0, 0.0]];
            for q in pts {
                let o = rot(site.yaw, q);
                let s = project(&cam, aspect, [site.pos[0] + o[0], site.pos[1] + q[1], site.pos[2] + o[2]]);
                // The panel takes the right 42% (x > 0.16 in -1..1).
                assert!(s[0] > -0.98 && s[0] < 0.1, "{q:?} at x {} on {w}x{h}", s[0]);
                assert!(s[1] > -1.0 && s[1] < 1.0, "{q:?} at y {} on {w}x{h}", s[1]);
                assert!(s[2] > 0.0 && s[2] < 1.0);
            }
        }
    }

    #[test]
    fn the_stage_follows_the_phase_and_the_clock() {
        assert_eq!(stage(Phase::Racing, 99.0, 1), Stage::Race);
        assert_eq!(stage(Phase::Countdown, 0.0, 1), Stage::Race);
        assert_eq!(stage(Phase::Finished, 0.0, 1), Stage::Celebrate);
        assert_eq!(stage(Phase::Finished, CELEBRATE_S - 0.01, 1), Stage::Celebrate);
        assert_eq!(stage(Phase::Finished, CELEBRATE_S, 1), Stage::Podium);
        assert_eq!(stage(Phase::Finished, 0.0, 0), Stage::Podium, "an all-AI race has no celebration");
    }

    #[test]
    fn the_camera_leaves_the_chase_view_without_a_jump_and_then_orbits() {
        let c = Chase { anchor: [10.0, 0.5, -4.0], yaw: 0.7, distance: 7.0, height: 3.0, look_ahead: 4.0 };
        let (e0, t0) = finish_camera(&c, 0.0);
        let (ce, ct) = c.pose();
        for k in 0..3 {
            assert!((e0[k] - ce[k]).abs() < 1e-4 && (t0[k] - ct[k]).abs() < 1e-4, "starts as the chase view");
        }
        // No frame-to-frame jump at 60 Hz over the whole celebration.
        let mut prev = (e0, t0);
        let mut s = 0.0;
        while s < CELEBRATE_S {
            s += 1.0 / 60.0;
            let cur = finish_camera(&c, s);
            let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            assert!(d(cur.0, prev.0) < 0.35 && d(cur.1, prev.1) < 0.15, "jump at {s}");
            prev = cur;
        }
        // Settled: orbiting at the orbit radius and height, looking at a point above the kart.
        let (e, t) = finish_camera(&c, ORBIT_BLEND_S + 1.0);
        let r = (e[0] - c.anchor[0]).hypot(e[2] - c.anchor[2]);
        assert!((r - ORBIT_RADIUS).abs() < 1e-3, "radius {r}");
        assert!((e[1] - c.anchor[1] - ORBIT_HEIGHT).abs() < 1e-3);
        assert!((t[1] - c.anchor[1] - ORBIT_LOOK_Y).abs() < 1e-3);
        // It keeps turning, slowly: under a lap in 8 s.
        assert!(orbit_angle(8.0) < 1.2 * core::f32::consts::TAU && orbit_angle(3.0) > orbit_angle(2.0));
        // And swings round to the front of the kart by the end of the celebration.
        assert!(orbit_angle(CELEBRATE_S) > 2.5);
    }

    #[test]
    fn confetti_is_a_fixed_pool_scaled_by_tier_and_inside_the_particle_caps() {
        assert!(confetti_count(100_000) < confetti_count(400_000));
        assert!(confetti_count(400_000) < confetti_count(2_000_000));
        assert!(confetti_count(2_000_000) < confetti_count(6_000_000));
        // Particle caps of catalog/tiers.json: 2 000 / 10 000 / 50 000 / 250 000.
        for (tris, cap) in [(100_000u32, 2_000usize), (400_000, 10_000), (2_000_000, 50_000), (6_000_000, 250_000)] {
            assert!(confetti_count(tris) <= cap);
        }
        // Low's sweets are a small part of its triangle limit.
        assert!(confetti_count(100_000) as u32 * SWEET_TRIANGLES < 6_000);
    }

    #[test]
    fn sweets_launch_from_the_gate_fall_and_land_on_the_ground() {
        let l = finish_launch(&track());
        let n = 90;
        // Nothing in the air before the line; the first launches at once.
        assert!(sweet(n - 1, n, 0.0, &l).is_none());
        let mut seen = 0;
        for i in 0..n {
            for k in 0..80 {
                let since = k as f32 * 0.1;
                if let Some(s) = sweet(i, n, since, &l) {
                    seen += 1;
                    assert!(s.pos[1] >= l.ground, "under the ground: {s:?}");
                    assert!(s.pos[1] < l.ground + 30.0);
                }
            }
            // Every one has landed after the whole burst.
            let last = sweet(i, n, 12.0, &l).expect("a landed sweet stays");
            assert!(last.landed && (last.pos[1] - l.ground - 0.1).abs() < 0.05);
        }
        assert!(seen > n * 20);
        // Deterministic.
        assert_eq!(sweet(7, n, 2.3, &l), sweet(7, n, 2.3, &l));
    }

    #[test]
    fn the_podium_rain_repeats_and_never_holds_more_than_the_pool() {
        let site = podium_site(&track(), 7.0);
        let l = podium_launch(&site);
        let n = 90;
        for k in 0..400 {
            let since = k as f32 * 0.1;
            let live = (0..n).filter(|&i| sweet(i, n, since, &l).is_some()).count();
            assert!(live <= n);
            if since > 6.0 {
                assert!(live > 0, "the rain goes on at {since}");
            }
        }
    }

    #[test]
    fn the_gate_lights_flash_in_turn_and_are_never_dark() {
        for k in 0..20 {
            let since = k as f32 * 0.1;
            let gold: Vec<_> = (0..3).map(|i| light_is_gold(i, since)).collect();
            assert!(gold.contains(&true) && gold.contains(&false), "{gold:?} at {since}");
        }
    }

    /// The triangles one frame of the finish and the podium add, on every tier.
    #[test]
    fn the_end_of_the_race_stays_inside_the_tier_limits() {
        use crate::KartRacerPlugin;
        use evani_core::App;
        let mats = crate::draw::resolve_materials(&evani_core::plugin::LookData::default());
        let tris = |rw: &RenderWorld| -> u32 { rw.meshes.iter().map(|m| evani_render::mesh::triangles_of(m.mesh)).sum() };
        assert_eq!(
            evani_render::mesh::triangles_of(MeshId::CUBE) + 2 * evani_render::mesh::triangles_of(MeshId::CONE),
            SWEET_TRIANGLES
        );
        type Moment = fn(&evani_core::ecs::World) -> Result<(), String>;
        for (moment, name) in [(crate::moments::finish as Moment, "finish"), (crate::moments::podium as Moment, "podium")] {
            let mut a = App::new("t").with_kit(KartRacerPlugin);
            a.run_headless(2, 1000.0 / 60.0);
            moment(&a.world).unwrap();
            let race = RaceView::new(&a.world);
            let site = podium_site(&race.track, 7.0);
            // Tier triangle limits (catalog/tiers.json) and particle caps.
            for (limit, cap) in [(100_000u32, 2_000u32), (400_000, 10_000), (2_000_000, 50_000), (6_000_000, 250_000)] {
                for shadows in [false, true] {
                    let mut rw = RenderWorld::default();
                    for k in [0.0f32, 1.0, 3.0] {
                        rw.clear();
                        draw(&race, &site, &mats, limit, k, shadows, &mut rw);
                        let t = tris(&rw);
                        assert!(t < limit / 4, "{name} on {limit}: {t} triangles");
                        assert!(rw.meshes.len() as u32 <= cap, "{name} on {limit}: {} instances", rw.meshes.len());
                    }
                }
            }
        }
    }

    #[test]
    fn the_site_is_on_open_meadow_clear_of_the_road_and_the_hills() {
        let t = track();
        let site = podium_site(&t, 7.0);
        let eye = camera_eye(&site);
        let c = crate::scenery::centre(&t);
        // Inside the circuit, off the road's banks and the mountains.
        let d = |p: [f32; 3]| t.locate(p, 0).lateral.abs();
        assert!(d(site.pos) > t.half_width + 7.0 + 12.0, "the podium is {} m off the road", d(site.pos));
        assert!(d(eye) > t.half_width + 7.0 + 6.0, "the camera is {} m off the road", d(eye));
        assert!(crate::scenery::meadow_clear(&t, 7.0, &[], site.pos, CLEAR_RADIUS));
        assert!(crate::scenery::meadow_clear(&t, 7.0, &[], eye, 3.0));
        let to_c = (site.pos[0] - c[0]).hypot(site.pos[2] - c[2]);
        assert!(to_c < 120.0, "inside the circuit");
    }

    #[test]
    fn the_podium_and_its_camera_face_the_sun() {
        let site = podium_site(&track(), 7.0);
        let sun = [0.62f32, 0.85, 0.28];
        assert!((sun[0].atan2(sun[2]) - SUN_YAW).abs() < 1e-5);
        // The drivers' front and the camera's side of the podium face the
        // sun's heading: the sun is behind the camera's shoulder.
        let flat = (sun[0] * sun[0] + sun[2] * sun[2]).sqrt();
        let lit = |yaw: f32| (yaw.sin() * sun[0] + yaw.cos() * sun[2]) / flat;
        assert!(lit(site.yaw) > 0.99, "{}", lit(site.yaw));
        assert!(lit(site.yaw + CAM_AZIMUTH) > 0.8, "{}", lit(site.yaw + CAM_AZIMUTH));
    }
}
