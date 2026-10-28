//! The example kit's scene, as the sandbox draws it: fills [`RenderWorld`]
//! once per rendered frame. It lives in the kit so native tools (the
//! geocheck's sandbox adapter, the review dump) draw exactly what the game does.
//!
//! * The example kit's spinners ([`crate::place`]): a 20 x 20
//!   grid of bevelled cubes and ringed orbs, about one in seven glowing
//!   (the look's `glow` material, so bloom has something to catch). Each
//!   rests on the ground on its lowest point while it turns. With the
//!   shadow map on they cast real shadows; without it (Low) each stands on a
//!   dark shadow disc.
//! * A ground plane at y = 0 that reaches the horizon (2 km square, so no
//!   scripted camera sees the end of the world).
//! * The middle distance, all original filler in the look's palette: a loose
//!   ring of low stone terraces 36 to 54 m out, standing columns with stone
//!   caps 64 to 110 m out (one in three crowned with a glowing orb) and low
//!   hills 320 to 700 m out, softened by the look's distance haze.
//! * Drifting atlas sprites over the 3D scene, in palette colours: 110 at
//!   1920 x 1080 and the same density on smaller canvases ([`sprite_count`]).
//! * A slow orbit camera from [`CameraPath`], a pure function of sim time,
//!   framed so the whole grid and a band of sky are in view all the way
//!   round. On a narrower screen ([`frame_for_aspect`]) the field of view
//!   widens and the camera rises and looks down more steeply, so the sky
//!   stays a 14 to 17 % band and the whole grid fills the frame across.
//!
//! Everything is computed from the spinners, the entity order and
//! `Time::render_seconds` (spinner poses through [`crate::SpinClock`]), so
//! the picture is the same for a given sim time, however long the game ran
//! before: a capture at a set sim time draws the same frame every time.

use std::sync::OnceLock;

use evani_core::camera::CameraPath;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::effects::EffectsStatus;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use crate::Spinner;
use crate::place::{self, hash};
use evani_render::atlas::cell;
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, RenderWorld, Sprite};

/// Ground plane edge length (world units). Wide enough that the ground
/// reaches over 600 m from every orbit camera (the review's world-edge
/// check), and ends near the renderer's 1000 m far plane.
pub const GROUND: f32 = 2000.0;
/// Height of the ground plane.
pub const GROUND_Y: f32 = 0.0;
/// Number of sprites in the screen-space layer.
pub const SPRITES: u32 = 110;
/// Vertical field of view of the orbit on a 16:9 screen (degrees).
pub const FOV_Y: f32 = 42.0;
/// Shadow discs: how much darker than the ground (linear multiplier).
const DISC_SHADE: [f32; 4] = [0.42, 0.45, 0.52, 1.0];

/// The camera path the sandbox (and later bench, capture, compare.ab) uses:
/// 42 m out, 11 m up, one turn a minute. From there the whole 20 x 20 grid
/// stays in frame with a margin at every point of the orbit, and the
/// horizon sits 14 to 17 % below the top edge (a band of sky) even at the
/// bob's highest point. Narrow screens frame it with [`frame_for_aspect`].
pub fn camera_path() -> CameraPath {
    let mut p = CameraPath::orbit([0.0, 0.0, 0.0], 42.0, 11.0, 60.0).with_bob(0.5, 7.0);
    p.fov_y_deg = FOV_Y;
    p
}

/// Widest vertical field of view a narrow screen gets (degrees).
const FOV_Y_MAX: f32 = 90.0;
/// Share of the frame height that is sky above the horizon on a narrow
/// screen (the look bible's 14 to 17 % band).
const SKY_BAND: f32 = 0.155;
/// On a narrow screen: the grid's widest point stays inside this share of
/// the half-width (|x| in NDC) all the way round the orbit...
const FIT_X: f32 = 0.93;
/// ...and its nearest corner stays above this height (NDC y), clear of the
/// HUD panel bottom left.
const FIT_Y_LOW: f32 = -0.74;
/// The grid's half-extent for framing: 20 x 20 spinners and a spinner's
/// reach (m).
const GRID_HALF: f32 = 19.0 * place::SPACING * 0.5 + 0.7;

/// The vertical field of view for a canvas of `aspect` (width / height):
/// [`FOV_Y`] at 16:9 and wider, wider on a narrower screen so it sees as
/// far across as a 16:9 one would, up to [`FOV_Y_MAX`].
pub fn fov_for_aspect(aspect: f32) -> f32 {
    const WIDE: f32 = 16.0 / 9.0;
    if aspect.is_nan() || aspect <= 0.0 || aspect >= WIDE {
        return FOV_Y;
    }
    let half = (FOV_Y * 0.5).to_radians().tan() * WIDE / aspect;
    (2.0 * half.atan().to_degrees()).min(FOV_Y_MAX)
}

/// How a narrow screen frames the orbit: the camera looks down at `pitch`
/// (degrees) at a point `aim` metres beyond the grid's centre, from
/// `distance` metres away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NarrowFrame {
    pub fov_y_deg: f32,
    pub pitch_deg: f32,
    pub aim: f32,
    pub distance: f32,
}

impl NarrowFrame {
    /// The pose at orbit azimuth `dir` (unit, from the centre towards the
    /// camera, on the ground), lifted by `bob` metres.
    fn pose(&self, dir: [f32; 2], bob: f32) -> evani_core::camera::CameraPose {
        let p = self.pitch_deg.to_radians();
        let t = [-dir[0] * self.aim, GROUND_Y, -dir[1] * self.aim];
        let (h, v) = (self.distance * p.cos(), self.distance * p.sin());
        evani_core::camera::CameraPose {
            eye: [t[0] + dir[0] * h, t[1] + v + bob, t[2] + dir[1] * h],
            target: t,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: self.fov_y_deg,
        }
    }
}

/// Where the grid's corners land at `aspect` with `f`, worst case round the
/// orbit: (largest |x|, lowest y) in NDC. The square grid repeats every 90
/// degrees and mirrors, so a sweep of 0 to 45 covers the whole orbit.
fn grid_extent(f: &NarrowFrame, aspect: f32) -> (f32, f32) {
    let (mut wx, mut wy) = (0.0f32, 1.0f32);
    for step in 0..=18 {
        let a = (step as f32 * 2.5).to_radians();
        let pose = f.pose([a.sin(), a.cos()], 0.0);
        for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            for y in [0.0, 1.0] {
                let (sx, sy) = ndc(&pose, [x * GRID_HALF, y, z * GRID_HALF], aspect);
                wx = wx.max(sx.abs());
                wy = wy.min(sy);
            }
        }
    }
    (wx, wy)
}

/// A world point in NDC (x, y in -1..1 on screen) for a pose and aspect.
pub fn ndc(pose: &evani_core::camera::CameraPose, p: [f32; 3], aspect: f32) -> (f32, f32) {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm = |a: [f32; 3]| {
        let l = dot(a, a).sqrt().max(1e-6);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let f = norm(sub(pose.target, pose.eye));
    let r = norm([-f[2], 0.0, f[0]]);
    let u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
    let d = sub(p, pose.eye);
    let z = dot(d, f).max(1e-4);
    let t = (pose.fov_y_deg * 0.5).to_radians().tan();
    (dot(d, r) / z / (t * aspect), dot(d, u) / z / t)
}

/// The narrow-screen framing for `aspect`: the field of view from
/// [`fov_for_aspect`]; the pitch that puts the horizon [`SKY_BAND`] below
/// the top edge; then the closest distance at which the whole grid fits
/// across ([`FIT_X`]) and the furthest aim beyond the centre that keeps its
/// near corner above [`FIT_Y_LOW`], so the grid fills the frame from just
/// above the HUD panel. A portrait phone looks down steeply from high up;
/// the frame's bottom edge then meets the ground inside the terrace ring,
/// so no near terrace is in view.
///
/// From that height the ground visibly ends a little below the true horizon
/// (at its edge or at the far plane), which would widen the sky band by a
/// few per cent, so the pitch also tilts down by that dip.
pub fn narrow_frame(aspect: f32) -> NarrowFrame {
    let fov = fov_for_aspect(aspect);
    let band = ((1.0 - 2.0 * SKY_BAND) * (fov * 0.5).to_radians().tan()).atan().to_degrees();
    let mut f = narrow_fit(aspect, fov, band);
    for _ in 0..4 {
        f = narrow_fit(aspect, fov, band + ground_dip(&f));
    }
    f
}

/// How far (degrees) below the true horizon the ground visibly ends from
/// framing `f`, averaged over the orbit: straight ahead the ground runs to
/// its square edge (nearest facing a side, furthest facing a corner) or to
/// the far plane, whichever is closer.
fn ground_dip(f: &NarrowFrame) -> f32 {
    let p = f.pitch_deg.to_radians();
    let eye_y = f.distance * p.sin();
    let behind = f.distance * p.cos() - f.aim;
    let far = (evani_render::shaders::FAR - eye_y * p.sin()) / p.cos();
    let dip = |edge: f32| (eye_y / (edge + behind).min(far)).atan().to_degrees();
    0.5 * (dip(GROUND * 0.5) + dip(GROUND * 0.5 * core::f32::consts::SQRT_2))
}

/// [`narrow_frame`] at a fixed `pitch`: the closest distance that fits the
/// grid across, then the furthest aim that keeps its near corner up.
fn narrow_fit(aspect: f32, fov: f32, pitch: f32) -> NarrowFrame {
    let fit = |aim: f32| {
        let (mut lo, mut hi) = (5.0f32, 400.0f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            let f = NarrowFrame { fov_y_deg: fov, pitch_deg: pitch, aim, distance: mid };
            if grid_extent(&f, aspect).0 <= FIT_X {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let f = NarrowFrame { fov_y_deg: fov, pitch_deg: pitch, aim, distance: hi };
        (f, grid_extent(&f, aspect).1)
    };
    let (f0, low0) = fit(0.0);
    if low0 < FIT_Y_LOW {
        return f0;
    }
    let (mut lo, mut hi) = (0.0f32, 80.0f32);
    let mut best = f0;
    for _ in 0..16 {
        let mid = 0.5 * (lo + hi);
        let (f, low) = fit(mid);
        if low >= FIT_Y_LOW {
            best = f;
            lo = mid;
        } else {
            hi = mid;
        }
    }
    best
}

thread_local! {
    /// The last narrow framing worked out: (aspect bits, framing). A canvas
    /// keeps its shape for many frames, so it is solved once per resize.
    static NARROW: std::cell::Cell<Option<(u32, NarrowFrame)>> = const { std::cell::Cell::new(None) };
}

/// The orbit pose framed for a canvas of `aspect` (width / height). At 16:9
/// and wider it is the path's pose. Narrower, it is [`narrow_frame`] at the
/// same point of the orbit and the same height bob: the field of view
/// widens, the camera rises and looks down more steeply so the sky stays a
/// 14 to 17 % band, and it pulls back just far enough that the whole grid
/// fits across, filling the frame down to just above the HUD panel.
pub fn frame_for_aspect(
    pose: evani_core::camera::CameraPose,
    aspect: f32,
) -> evani_core::camera::CameraPose {
    const WIDE: f32 = 16.0 / 9.0;
    if aspect.is_nan() || aspect <= 0.0 || aspect >= WIDE {
        return pose;
    }
    let path = camera_path();
    let key = aspect.to_bits();
    let f = match NARROW.with(|c| c.get()) {
        Some((k, f)) if k == key => f,
        _ => {
            let f = narrow_frame(aspect);
            NARROW.with(|c| c.set(Some((key, f))));
            f
        }
    };
    let (dx, dz) = (pose.eye[0] - path.center[0], pose.eye[2] - path.center[2]);
    let l = dx.hypot(dz).max(1e-6);
    let bob = pose.eye[1] - path.center[1] - path.height;
    f.pose([dx / l, dz / l], bob)
}

/// How many sprites a canvas of `w` x `h` CSS pixels draws: [`SPRITES`] at
/// 1920 x 1080 and the same density on smaller canvases (at least 24), so a
/// phone or a small window is not crowded with them.
pub fn sprite_count(w: f32, h: f32) -> u32 {
    let share = (w * h / (1920.0 * 1080.0)).clamp(0.0, 1.0);
    ((SPRITES as f32 * share).round() as u32).clamp(24, SPRITES)
}

/// The `Stage::Frame` system that draws the scene.
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.scene", Stage::Frame, draw_scene)
}

/// Cheap integer hash to a float in 0..1 (per-sprite and per-prop
/// constants, no RNG state).
fn hash01(i: u32, salt: u32) -> f32 {
    (hash(i, salt) >> 8) as f32 / (1u32 << 24) as f32
}

/// Which look material a scenery piece uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Pale stone: terrace tops, column caps and bases.
    Stone,
    /// Dark slate: terraces and column shafts.
    Slate,
    /// The glowing orbs on some columns.
    Glow,
    /// Hills: the ground material, tinted.
    Hill,
}

/// One piece of the middle distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    pub mesh: MeshId,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub role: Role,
    pub color: [f32; 4],
}

fn yaw(a: f32) -> [f32; 4] {
    quat_axis_angle([0.0, 1.0, 0.0], a)
}

/// Footprints already used: `(x, z, radius)`.
struct Taken(Vec<[f32; 3]>);

impl Taken {
    fn free(&self, x: f32, z: f32, r: f32, gap: f32) -> bool {
        self.0
            .iter()
            .all(|t| (t[0] - x).hypot(t[1] - z) >= t[2] + r + gap)
    }
}

/// The middle distance: terraces, columns and hills, built once from fixed
/// hashes (the same scene every run). Every piece stands on the ground or on
/// the piece under it, and no two footprints overlap.
pub fn scenery() -> &'static [Prop] {
    static SCENERY: OnceLock<Vec<Prop>> = OnceLock::new();
    SCENERY.get_or_init(build_scenery)
}

fn build_scenery() -> Vec<Prop> {
    use core::f32::consts::TAU;
    let mut out = Vec::new();
    let mut taken = Taken(Vec::new());
    let prop = |mesh, position, rotation, scale, role, color| Prop {
        mesh,
        position,
        rotation,
        scale,
        role,
        color,
    };
    let shade = |v: f32| [v, v, v, 1.0];

    // Low stone terraces, a loose ring 36 to 54 m out: long, low slabs laid
    // round the grid, some with a smaller slab on top.
    const TERRACES: u32 = 30;
    for k in 0..TERRACES {
        let (h1, h2, h3, h4) = (hash01(k, 21), hash01(k, 22), hash01(k, 23), hash01(k, 24));
        let a = (k as f32 + 0.6 * (h1 - 0.5)) / TERRACES as f32 * TAU;
        let r = 36.0 + 18.0 * h2;
        let (x, z) = (r * a.sin(), r * a.cos());
        let (w, d) = (2.4 + 3.2 * h3, 1.6 + 2.0 * h4);
        let h = 0.3 + 0.9 * hash01(k, 25);
        let foot = 0.5 * w.hypot(d);
        if !taken.free(x, z, foot, 1.0) {
            continue;
        }
        taken.0.push([x, z, foot]);
        let rot = yaw(a + 0.4 * (hash01(k, 26) - 0.5));
        let role = if hash(k, 27).is_multiple_of(3) { Role::Stone } else { Role::Slate };
        let tone = 0.9 + 0.2 * hash01(k, 28);
        out.push(prop(MeshId::BEVEL_CUBE, [x, GROUND_Y + h * 0.5, z], rot, [w, h, d], role, shade(tone)));
        if hash01(k, 29) < 0.45 {
            let (w2, d2, h2b) = (w * 0.55, d * 0.6, 0.25 + 0.35 * hash01(k, 30));
            let rot2 = yaw(a + 0.35 + 0.3 * hash01(k, 31));
            out.push(prop(
                MeshId::BEVEL_CUBE,
                [x, GROUND_Y + h + h2b * 0.5, z],
                rot2,
                [w2, h2b, d2],
                if role == Role::Stone { Role::Slate } else { Role::Stone },
                shade(tone),
            ));
        }
    }

    // Standing columns 64 to 110 m out: a stone base, a slate shaft, a stone
    // cap; one in three carries a glowing orb that blooms far off.
    const COLUMNS: u32 = 22;
    for k in 0..COLUMNS {
        let (h1, h2, h3) = (hash01(k, 41), hash01(k, 42), hash01(k, 43));
        let a = (k as f32 + 0.7 * (h1 - 0.5)) / COLUMNS as f32 * TAU + 0.11;
        let r = 64.0 + 46.0 * h2;
        let (x, z) = (r * a.sin(), r * a.cos());
        let w = 1.4 + 0.9 * h3;
        let tall = 4.0 + 10.0 * hash01(k, 44);
        let foot = w * 1.7 * 0.71;
        if !taken.free(x, z, foot, 3.0) {
            continue;
        }
        taken.0.push([x, z, foot]);
        let rot = yaw(TAU * hash01(k, 45));
        let base_h = 0.45;
        let cap_h = 0.5;
        out.push(prop(MeshId::BEVEL_CUBE, [x, GROUND_Y + base_h * 0.5, z], rot, [w * 1.7, base_h, w * 1.7], Role::Stone, shade(0.95)));
        out.push(prop(MeshId::BEVEL_CUBE, [x, GROUND_Y + base_h + tall * 0.5, z], rot, [w, tall, w], Role::Slate, shade(1.0)));
        let top = GROUND_Y + base_h + tall;
        out.push(prop(MeshId::BEVEL_CUBE, [x, top + cap_h * 0.5, z], rot, [w * 1.45, cap_h, w * 1.45], Role::Stone, shade(1.0)));
        if k % 3 == 1 {
            let d = w * 0.95;
            out.push(prop(MeshId::SPHERE, [x, top + cap_h + d * 0.5, z], [0.0, 0.0, 0.0, 1.0], [d; 3], Role::Glow, shade(1.0)));
        }
    }

    // Low hills 320 to 700 m out, in the ground's green leaning blue with
    // distance; the haze fades them into the sky.
    const HILLS: u32 = 18;
    for k in 0..HILLS {
        let (h1, h2, h3, h4) = (hash01(k, 61), hash01(k, 62), hash01(k, 63), hash01(k, 64));
        let a = (k as f32 + 0.8 * (h1 - 0.5)) / HILLS as f32 * TAU;
        let r = 320.0 + 380.0 * h2;
        let (x, z) = (r * a.sin(), r * a.cos());
        let wide = 90.0 + 130.0 * h3;
        let high = 7.0 + 13.0 * h4;
        if !taken.free(x, z, wide * 0.5, 4.0) {
            continue;
        }
        taken.0.push([x, z, wide * 0.5]);
        let far = (r - 320.0) / 380.0;
        let color = [0.78 - 0.1 * far, 0.86 - 0.06 * far, 0.95 + 0.1 * far, 1.0];
        out.push(prop(
            MeshId::DOME,
            [x, GROUND_Y, z],
            yaw(TAU * hash01(k, 65)),
            [wide, high * 2.0, wide * (0.7 + 0.5 * hash01(k, 66))],
            Role::Hill,
            color,
        ));
    }
    out
}

/// Materials the scene uses, looked up once per frame.
struct Mats {
    glow: u32,
    ground: u32,
    accent: u32,
    stone: u32,
    slate: u32,
}

/// Whether this frame draws a shadow map (the `shadows` effect is on and the
/// tier gives a map size); without one, things stand on shadow discs.
fn shadow_map_on(world: &World) -> bool {
    world.try_res::<Tiers>().is_some_and(|t| t.active().shadow_map.is_some())
        && world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on("shadows"))
}

pub fn draw_scene(world: &mut World) {
    let t = world.res::<Time>().render_seconds();
    let discs = !shadow_map_on(world);
    let m = {
        let look = world.res::<LookData>();
        let accent = RenderWorld::material(&look, "accent");
        let has = |n: &str| look.materials.iter().any(|x| x.name == n);
        Mats {
            glow: RenderWorld::material(&look, "glow"),
            ground: RenderWorld::material(&look, "ground"),
            accent,
            stone: if has("stone") { RenderWorld::material(&look, "stone") } else { accent },
            slate: if has("slate") { RenderWorld::material(&look, "slate") } else { accent },
        }
    };
    // The geometry check's still world: only the static scene is drawn.
    let still = world
        .try_res::<crate::ExampleKit>()
        .map(|k| k.static_world)
        .is_some_and(|p| world.res::<ParamRegistry>().bool(p));
    // Every pose is a pure function of the render time.
    let angles = crate::spinner_angles(world, t);
    let side = place::side(angles.len());
    let mut rw = world.res_mut::<RenderWorld>();
    let mut cam = camera_path().pose(t);
    let [w, h] = rw.viewport_css;
    if w > 0.0 && h > 0.0 {
        cam = frame_for_aspect(cam, w / h);
    }
    rw.camera = cam;

    // Ground plane.
    let mut g = MeshInstance::new(MeshId::PLANE, [0.0, GROUND_Y, 0.0]);
    g.scale = [GROUND, 1.0, GROUND];
    g.material = m.ground;
    rw.draw_mesh(g);

    // The middle distance.
    for p in scenery() {
        let mut inst = MeshInstance::new(p.mesh, p.position);
        inst.rotation = p.rotation;
        inst.scale = p.scale;
        inst.color = p.color;
        inst.material = match p.role {
            Role::Stone => m.stone,
            Role::Slate => m.slate,
            Role::Glow => m.glow,
            Role::Hill => m.ground,
        };
        rw.draw_mesh(inst);
    }

    // Spinner grid, centred on the origin, each resting on the ground.
    let mut i = 0u32;
    world.query::<&Spinner>().for_each(|_, s| {
        if still {
            return;
        }
        let now = Spinner { angle: angles.get(i as usize).copied().unwrap_or(s.angle), ..*s };
        let p = place::parts(i, side, &now, GROUND_Y);
        let (mat, color) = if p.glows {
            (m.glow, [1.0; 4])
        } else {
            let k = hash01(i, 3);
            (m.accent, [0.75 + 0.25 * k, 0.85 + 0.15 * (1.0 - k), 1.0, 1.0])
        };
        let mut body = p.body;
        body.material = mat;
        body.color = color;
        rw.draw_mesh(body);
        if let Some(mut ring) = p.ring {
            // The ring in pale stone round a blue ball; round a glowing ball
            // it glows too.
            if p.glows {
                ring.material = m.glow;
            } else {
                ring.material = m.stone;
                ring.color = [1.0; 4];
            }
            rw.draw_mesh(ring);
        }
        if discs {
            let mut d = p.disc;
            d.material = m.ground;
            d.color = DISC_SHADE;
            rw.draw_decal(d);
        }
        i += 1;
    });

    // Sprites drift across the canvas and wrap; positions in CSS pixels.
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let shapes = [
        cell::CIRCLE,
        cell::RING,
        cell::STAR,
        cell::DIAMOND,
        cell::PLUS,
        cell::GLOW,
        cell::RING,
        cell::CIRCLE,
        cell::GLOW,
        cell::DIAMOND,
        cell::CHECKER,
    ];
    // Palette colours (linear): spinner blue, glow orange, palette light,
    // ground mint, a cooler cyan.
    let colours: [[f32; 3]; 5] = [
        [0.30, 0.56, 0.90],
        [1.00, 0.46, 0.12],
        [0.95, 0.92, 0.88],
        [0.42, 0.80, 0.60],
        [0.36, 0.78, 0.95],
    ];
    let tf = (t % 3600.0) as f32;
    for k in 0..sprite_count(w, h) {
        let (a, b, c, d) = (hash01(k, 1), hash01(k, 2), hash01(k, 5), hash01(k, 7));
        let speed = 16.0 + 44.0 * c;
        let size = 7.0 + 15.0 * d;
        let x = (a * (w + 2.0 * size) + speed * tf).rem_euclid(w + 2.0 * size) - size;
        let y = (b * h + 16.0 * (tf * (0.3 + c) + k as f32).sin()).rem_euclid(h);
        let shape = shapes[(k as usize) % shapes.len()];
        let color = if shape == cell::CHECKER {
            [1.0, 1.0, 1.0, 0.7]
        } else {
            let p = colours[(hash(k, 9) % colours.len() as u32) as usize];
            let v = 0.85 + 0.15 * a;
            [p[0] * v, p[1] * v, p[2] * v, 0.6]
        };
        rw.draw_sprite(Sprite {
            pos: [x, y],
            size: [size, size],
            rotation: tf * (c - 0.5) * 2.0,
            cell: shape,
            color,
            layer: (k % 3) as i16,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_in_unit_range() {
        for i in 0..1000 {
            let v = hash01(i, 9);
            assert!((0.0..1.0).contains(&v), "{v}");
        }
    }

    #[test]
    fn fov_widens_only_on_narrow_screens() {
        assert_eq!(fov_for_aspect(16.0 / 9.0), FOV_Y);
        assert_eq!(fov_for_aspect(21.0 / 9.0), FOV_Y);
        assert_eq!(fov_for_aspect(0.0), FOV_Y);
        let phone = fov_for_aspect(390.0 / 844.0);
        assert!(phone > FOV_Y && phone <= FOV_Y_MAX, "{phone}");
        let tablet = fov_for_aspect(4.0 / 3.0);
        assert!(tablet > FOV_Y && tablet < phone, "{tablet}");
    }

    /// Project a world point with a pose at 16:9: (x, y) in -1..1 when on
    /// screen, and the depth along the view.
    fn project(pose: &evani_core::camera::CameraPose, p: [f32; 3]) -> (f32, f32, f32) {
        project_at(pose, p, 16.0 / 9.0)
    }

    /// [`project`] on a canvas of `aspect` (width / height).
    fn project_at(pose: &evani_core::camera::CameraPose, p: [f32; 3], aspect: f32) -> (f32, f32, f32) {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let norm = |a: [f32; 3]| {
            let l = dot(a, a).sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        };
        let f = norm(sub(pose.target, pose.eye));
        let r = norm([-f[2], 0.0, f[0]]);
        let u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
        let d = sub(p, pose.eye);
        let z = dot(d, f);
        let t = (pose.fov_y_deg * 0.5).to_radians().tan();
        (dot(d, r) / z / (t * aspect), dot(d, u) / z / t, z)
    }

    #[test]
    fn whole_grid_and_a_sky_band_stay_in_frame_all_the_way_round() {
        let path = camera_path();
        let half = 19.0 * place::SPACING * 0.5 + 0.7;
        for step in 0..240 {
            let pose = path.pose(step as f64 * 0.25);
            for (x, z) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                for y in [0.0, 1.0] {
                    let (sx, sy, _) = project(&pose, [x, y, z]);
                    assert!(sx.abs() < 0.95 && sy.abs() < 0.95, "t={} corner ({x},{z}) at ({sx:.2},{sy:.2})", step as f64 * 0.25);
                }
            }
            // The horizon: a point far out at eye height, straight ahead.
            let f = [pose.target[0] - pose.eye[0], 0.0, pose.target[2] - pose.eye[2]];
            let l = f[0].hypot(f[2]);
            let far = [pose.eye[0] + f[0] / l * 5000.0, pose.eye[1], pose.eye[2] + f[2] / l * 5000.0];
            let (_, sy, _) = project(&pose, far);
            // At least 10 % of the frame height is sky above the horizon.
            assert!(sy < 0.8, "t={}: horizon at {sy:.2}", step as f64 * 0.25);
        }
    }

    #[test]
    fn the_whole_grid_fits_across_a_portrait_phone_all_the_way_round() {
        let path = camera_path();
        let half = 19.0 * place::SPACING * 0.5 + 0.7;
        for aspect in [390.0 / 844.0, 9.0 / 16.0, 3.0 / 4.0, 4.0 / 3.0] {
            for step in 0..240 {
                let t = step as f64 * 0.25;
                let pose = frame_for_aspect(path.pose(t), aspect);
                assert!(pose.fov_y_deg <= FOV_Y_MAX + 1e-3);
                // Clear of the columns (from 64 m out).
                let r = pose.eye[0].hypot(pose.eye[2]);
                assert!(r < 58.0, "aspect {aspect}: eye {r:.1} m out");
                // The grid fits across, and its near corner stays above the
                // HUD panel (bottom left, about the lowest 9 % of a phone).
                for (x, z) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                    for y in [0.0, 1.0] {
                        let (sx, sy, _) = project_at(&pose, [x, y, z], aspect);
                        assert!(
                            sx.abs() < 0.97 && sy.abs() < 0.95 && sy > -0.8,
                            "aspect {aspect} t={t} corner ({x},{z}) at ({sx:.2},{sy:.2})"
                        );
                    }
                }
                // A 14 to 17 % band of sky along the top, measured to where
                // the ground visibly ends straight ahead: its square edge or
                // the far plane, whichever is closer.
                let f = [pose.target[0] - pose.eye[0], 0.0, pose.target[2] - pose.eye[2]];
                let l = f[0].hypot(f[2]);
                let fwd = [f[0] / l, f[2] / l];
                let ground_at = |s: f32| [pose.eye[0] + fwd[0] * s, GROUND_Y, pose.eye[2] + fwd[1] * s];
                let edge = [0, 1]
                    .iter()
                    .filter(|&&k| fwd[k].abs() > 1e-6)
                    .map(|&k| {
                        let e = if k == 0 { pose.eye[0] } else { pose.eye[2] };
                        (GROUND * 0.5 * fwd[k].signum() - e) / fwd[k]
                    })
                    .fold(f32::INFINITY, f32::min);
                let (_, _, z0) = project_at(&pose, ground_at(0.0), aspect);
                let (_, _, z1) = project_at(&pose, ground_at(1.0), aspect);
                let s_far = (evani_render::shaders::FAR - z0) / (z1 - z0);
                let (_, hy, _) = project_at(&pose, ground_at(edge.min(s_far)), aspect);
                let sky = (1.0 - hy) * 0.5;
                if aspect < 1.0 {
                    assert!((0.14..=0.17).contains(&sky), "aspect {aspect} t={t}: sky {sky:.3}");
                }
                // No terrace in the bottom band (under the panel or cut by
                // the bottom edge), and none cut by a side edge in the lowest
                // quarter of the frame (near slabs that read as clutter).
                for p in scenery().iter().filter(|p| p.mesh == MeshId::BEVEL_CUBE && p.position[0].hypot(p.position[2]) < 56.0) {
                    let foot = 0.5 * p.scale[0].hypot(p.scale[2]);
                    let top = p.position[1] + p.scale[1] * 0.5;
                    let mut inside = false;
                    let mut outside = false;
                    for k in 0..8 {
                        let a = k as f32 * core::f32::consts::FRAC_PI_4;
                        for y in [GROUND_Y, top] {
                            let q = [p.position[0] + foot * a.sin(), y, p.position[2] + foot * a.cos()];
                            let (sx, sy, z) = project_at(&pose, q, aspect);
                            if z < 0.1 || sy > 1.0 || sy < -1.0 {
                                continue;
                            }
                            assert!(
                                !(sx.abs() < 1.0 && sy < -0.7),
                                "aspect {aspect} t={t}: terrace at {:?} in the bottom band ({sx:.2},{sy:.2})",
                                p.position
                            );
                            if sy < -0.5 {
                                if sx.abs() < 1.0 { inside = true } else { outside = true }
                            }
                        }
                    }
                    assert!(!(inside && outside) || aspect > 1.0, "aspect {aspect} t={t}: terrace at {:?} cut by a side edge", p.position);
                }
            }
        }
        // 16:9 and wider keep the path's own pose.
        let p = path.pose(3.0);
        assert_eq!(frame_for_aspect(p, 16.0 / 9.0), p);
    }

    #[test]
    fn scenery_stands_on_the_ground_without_overlaps() {
        let s = scenery();
        assert!(s.len() > 40, "{}", s.len());
        for p in s {
            let bottom = match p.mesh {
                MeshId::DOME => p.position[1],
                _ => p.position[1] - p.scale[1] * 0.5,
            };
            assert!(bottom >= GROUND_Y - 1e-4, "{p:?} below the ground");
            let r = p.position[0].hypot(p.position[2]);
            // Clear of the grid and of the orbit camera's path.
            assert!(r > 28.0, "{p:?} too close to the grid");
            if p.mesh != MeshId::DOME && (r - 42.0).abs() < 12.0 {
                assert!(p.position[1] + p.scale[1] * 0.5 < 4.0, "{p:?} near the camera path is too tall");
            }
        }
    }
}
