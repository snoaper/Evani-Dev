//! The dynamic sky dome's scene (feature `skydome`, the Asset Studio sky
//! dome session, column 21, 2026-11-07): the procedural library's sky dome
//! (`sky-dynamic-sky-dome`, evani-material-proc's Preetham dome with its
//! clouds, started with the library's photoreal recipe,
//! `evani_procedural_lib::skydome`) seen the way its reference is: from a
//! standing player's eye (1.7 m), level in heading, a 90 degree horizontal
//! view tilted up so the horizon runs along the bottom tenth, a thin strip
//! of open meadow and distant low hills under the whole sky, mid-morning,
//! the sun out of frame to the upper left.
//!
//! The dome is installed the way a game does it (`ProcWorld::sky_only`, the
//! module's own draw system) under the photoreal look's daylight preset
//! (its `sky` material); the scene light follows the dome's sun, so the
//! meadow and hills are lit from where the glow is.
//!
//! `kit.proc_field.skydome_view`: 0 the reference's view, 1 looking high
//! overhead (the deep blue zenith and the clouds' bases), 2 turned towards
//! the sun's side (its glow over the hills).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural_lib::evani_material_proc as mp;
use evani_procedural_lib::skydome;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct SkyDome {
    view: F32Param,
    pitch: F32Param,
    fov: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    meadow: Vec3Param,
    hills: Vec3Param,
    far_hills: Vec3Param,
    fields: Vec3Param,
    haze: F32Param,
    /// The (view, sun bearing, sun elevation, haze) the light and the haze
    /// colour were last set for (NaN: not yet).
    set_for: [f32; 4],
}

/// The reference's camera: eye height (m), the tilt up that puts the
/// horizon at nine tenths of the frame's height (24 degrees, from the
/// horizon's row in the 1672 x 941 reference), and the vertical field of
/// view of a 90 degree horizontal one at 16:9.
const EYE: f32 = 1.7;
const PITCH: f32 = 24.1;
const FOV_Y: f32 = 58.7;
/// The view 0 heading (degrees from +z towards +x): looking along -z.
const HEADING: f32 = 180.0;
/// The sun: bearing to the left of the view (degrees) and elevation, outside
/// the frame's upper left (its corner is 37 degrees up, 56 to the left); the
/// glow's spread on the reference's left side fits best with it here.
const SUN_AZ: f32 = 75.0;
const SUN_EL: f32 = 28.0;
/// The meadow's sunlit yellow-green and the hills' (near: green under haze;
/// far: blue-green-grey), linear (round 2: the meadow brighter and
/// greener, the hills darker and greener so they read through the haze,
/// towards the reference's measured colours: its bottom strip 116/122/54,
/// the band under the skyline 94/105/85).
const MEADOW: [f32; 3] = [1.45, 2.3, 0.45];
const HILLS: [f32; 3] = [0.08, 0.3, 0.12];
const FAR_HILLS: [f32; 3] = [0.08, 0.24, 0.34];
/// The farmland past the meadow (round 2): the land drops away beyond the
/// meadow's edge, `MEADOW_HALF` m out, into fields and woods under the
/// haze, the reference's darker blue-green band under the skyline.
const FIELDS: [f32; 3] = [0.22, 0.62, 0.16];
const MEADOW_HALF: f32 = 80.0;
/// The distance haze's tint on the dome's horizon colour (round 2).
const HAZE_TINT: [f32; 3] = [0.82, 1.0, 1.04];
/// Weed clumps and darker patches over the meadow (seeded, fixed): how many.
const CLUMPS: u32 = 420;
/// Grass tufts in the meadow ahead (seeded, fixed; round 2): how many, and
/// how many of them carry a pale seed head.
const TUFTS: u32 = 7000;
const SEED_EVERY: u32 = 4;
/// The hills' crowns, a share of round 1's (round 2: the skyline at 89-91
/// percent of the frame's height, as the reference's, not 86-88).
const HILL_SCALE: f32 = 0.38;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.skydome_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the sky dome's reference view (a player's eye, the horizon low), 1 high overhead, 2 towards the sun's side"),
    )?;
    let pitch = kit.param_f32(ParamDef::f32("kit.proc_field.skydome_pitch", PITCH).range(0.0, 89.0).unit("deg").describe("The reference view's tilt up (degrees; 24 puts the horizon at nine tenths of the frame)"))?;
    let fov = kit.param_f32(ParamDef::f32("kit.proc_field.skydome_fov_y", FOV_Y).range(20.0, 120.0).unit("deg").describe("The vertical field of view (58.7: 90 degrees across at 16:9)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.skydome_sun_az", SUN_AZ).range(-180.0, 180.0).unit("deg").describe("The sun's bearing to the left of the reference view (degrees)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.skydome_sun_el", SUN_EL).range(1.0, 89.0).unit("deg").describe("The sun's elevation (degrees)"))?;
    let meadow = kit.param_vec3(ParamDef::color("kit.proc_field.skydome_meadow", MEADOW).range(0.0, 4.0).describe("The meadow's colour (on the look's dust material)"))?;
    let hills = kit.param_vec3(ParamDef::color("kit.proc_field.skydome_hills", HILLS).range(0.0, 4.0).describe("The nearer hills' colour"))?;
    let far_hills = kit.param_vec3(ParamDef::color("kit.proc_field.skydome_far_hills", FAR_HILLS).range(0.0, 4.0).describe("The farthest hills' colour"))?;
    let fields = kit.param_vec3(ParamDef::color("kit.proc_field.skydome_fields", FIELDS).range(0.0, 4.0).describe("The farmland's colour past the meadow's edge"))?;
    // Round 2: 0.35 (0.8 before), so the hills and fields keep their
    // blue-green through the haze instead of a pale grey band.
    let haze = kit.param_f32(ParamDef::f32("kit.proc_field.skydome_haze", 0.35).range(0.0, 3.0).describe("The distance haze's colour as a share of the dome's own horizon colour"))?;
    // The dome, as a game installs it: the sky alone, started with the
    // library's photoreal recipe.
    let mut pw = mp::ProcWorld::sky_only().map_err(|e| PluginError(format!("skydome: {e}")))?;
    pw.sky = skydome::photoreal();
    {
        let mut reg = kit.world.res_mut::<ParamRegistry>();
        pw.register_params(&mut reg).map_err(|e| PluginError(format!("skydome: {e}")))?;
    }
    kit.resource(pw);
    kit.system(mp::system());
    kit.resource(SkyDome { view, pitch, fov, sun_az, sun_el, meadow, hills, far_hills, fields, haze, set_for: [f32::NAN; 4] });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name) {
        let _ = reg.set(id, v);
    }
}

/// The camera at the player's eye, heading `heading` and tilted up `pitch`
/// degrees.
fn eye_camera(heading: f32, pitch: f32, fov_y: f32) -> CameraPose {
    let (h, p) = (heading.to_radians(), pitch.to_radians());
    let eye = [0.0, EYE, 0.0];
    CameraPose {
        eye,
        target: [eye[0] + h.sin() * p.cos() * 10.0, eye[1] + p.sin() * 10.0, eye[2] + h.cos() * p.cos() * 10.0],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov_y,
    }
}

/// The low hills round the horizon: `(bearing from the view's heading in
/// degrees, distance m, half width m, height m, far)`. Their crowns rise
/// half a degree to two degrees over the skyline in overlapping rows, the
/// farthest bluest, so the skyline undulates as the reference's does.
const HILL_ROWS: [(f32, f32, f32, f32, bool); 26] = [
    (-66.0, 440.0, 140.0, 11.0, true),
    (-50.0, 445.0, 120.0, 15.0, true),
    (-36.0, 440.0, 100.0, 9.0, true),
    (-22.0, 445.0, 150.0, 13.0, true),
    (-6.0, 440.0, 90.0, 8.0, true),
    (6.0, 445.0, 130.0, 12.0, true),
    (20.0, 440.0, 110.0, 16.0, true),
    (33.0, 445.0, 90.0, 10.0, true),
    (46.0, 440.0, 140.0, 14.0, true),
    (60.0, 445.0, 120.0, 9.0, true),
    (72.0, 440.0, 150.0, 13.0, true),
    (-58.0, 360.0, 70.0, 6.0, false),
    (-44.0, 380.0, 90.0, 8.5, false),
    (-30.0, 350.0, 60.0, 4.5, false),
    (-15.0, 370.0, 80.0, 7.0, false),
    (-2.0, 355.0, 55.0, 4.0, false),
    (12.0, 375.0, 85.0, 6.5, false),
    (26.0, 360.0, 60.0, 5.0, false),
    (39.0, 380.0, 95.0, 8.0, false),
    (53.0, 355.0, 70.0, 5.5, false),
    (66.0, 370.0, 80.0, 7.0, false),
    (-40.0, 300.0, 50.0, 2.6, false),
    (-12.0, 290.0, 60.0, 2.2, false),
    (16.0, 305.0, 45.0, 2.8, false),
    (44.0, 295.0, 55.0, 2.4, false),
    (-70.0, 300.0, 50.0, 2.5, false),
];

pub fn draw(world: &mut World) {
    let mut g = *world.res::<SkyDome>();
    let (view, pitch, fov, sun_az, sun_el, haze, meadow, hills, far_hills, fields) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round().clamp(0.0, 2.0),
            reg.f32(g.pitch),
            reg.f32(g.fov),
            reg.f32(g.sun_az),
            reg.f32(g.sun_el),
            reg.f32(g.haze),
            reg.vec3(g.meadow),
            reg.vec3(g.hills),
            reg.vec3(g.far_hills),
            reg.vec3(g.fields),
        )
    };
    let cam = match view as u32 {
        1 => eye_camera(HEADING, 62.0, fov),
        2 => eye_camera(HEADING + 50.0, 22.0, fov),
        _ => eye_camera(HEADING, pitch, fov),
    };
    let now = [view, sun_az, sun_el, haze];
    if now != g.set_for {
        let sun = mp::sky::sun_direction(sun_el, HEADING + sun_az);
        let sky = world.try_res::<mp::ProcWorld>().map(|p| p.sky);
        let mut reg = world.res_mut::<ParamRegistry>();
        // Mid-morning sun, near white; the dome's sun follows it.
        set(&mut reg, "render.light.direction", ParamValue::Vec3(sun));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.96, 0.9]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.9));
        set(&mut reg, "sky.follow_light", ParamValue::Bool(true));
        // The distance haze fades the hills to the dome's own horizon
        // colour (HDR, as the dome draws it before exposure).
        // Round 2: leaned from the ring's lavender towards the reference's
        // blue-green haze over the hills.
        if let Some(s) = sky {
            let r = s.ring(sun, 0.02, 24);
            let c = [r[0] * haze * HAZE_TINT[0], r[1] * haze * HAZE_TINT[1], r[2] * haze * HAZE_TINT[2]];
            set(&mut reg, "render.clear_color", ParamValue::Color(c));
        }
        drop(reg);
        g.set_for = now;
        *world.res_mut::<SkyDome>() = g;
    }
    let ground = RenderWorld::material(&world.res::<LookData>(), "dirt");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // The meadow the player stands in, out to its edge; past it the land
    // drops half a metre to the farmland, out to the hills (inside the
    // look's 450 m haze reach).
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
    f.scale = [MEADOW_HALF * 2.0, 1.0, MEADOW_HALF * 2.0];
    f.material = ground;
    f.color = [meadow[0], meadow[1], meadow[2], 1.0];
    rw.draw_mesh(f);
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.6, 0.0]);
    f.scale = [880.0, 1.0, 880.0];
    f.material = ground;
    f.color = [fields[0] * 0.8, fields[1] * 0.8, fields[2] * 0.85, 1.0];
    rw.draw_mesh(f);
    // The fields: a patchwork in rings past the meadow's edge, one patch
    // per 10 degree cell (inside its cell, so none overlap), fresh or
    // yellower grass, a few darker woods, between them the farmland's
    // own green.
    for (r, (r0, r1)) in FIELD_RINGS.into_iter().enumerate() {
        for cell in 0..15u32 {
            let h = |q: u32| hash01((r as u32).wrapping_mul(977).wrapping_add(cell.wrapping_mul(7919)).wrapping_add(q.wrapping_mul(104_729)) ^ 0x3A5F_1E27);
            if h(1) < 0.15 {
                continue;
            }
            let b = (HEADING - 70.0 + 10.0 * cell as f32).to_radians();
            let mid = 0.5 * (r0 + r1);
            let mut p = MeshInstance::new(MeshId::PLANE, [b.sin() * mid, -0.1, b.cos() * mid]);
            p.scale = [r0 * 0.17 * (0.7 + 0.3 * h(2)), 1.0, (r1 - r0) * (0.55 + 0.35 * h(3))];
            p.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], b);
            p.material = ground;
            let kind = h(4);
            let c = if kind > 0.78 {
                [0.55, 0.62, 0.85]
            } else if kind > 0.5 {
                [1.3, 1.18, 0.9]
            } else if kind > 0.25 {
                [1.1, 1.22, 1.05]
            } else {
                [0.85, 0.95, 1.0]
            };
            let v = 0.85 + 0.3 * h(5);
            p.color = [fields[0] * c[0] * v, fields[1] * c[1] * v, fields[2] * c[2] * v, 1.0];
            rw.draw_mesh(p);
        }
    }
    // A broken hedgerow just past the meadow's edge: low dark bushes with
    // gaps, so the edge is not a ruled line.
    for i in 0..HEDGES {
        let h = |q: u32| hash01(i.wrapping_mul(4099).wrapping_add(q.wrapping_mul(65_537)) ^ 0x6B43_A9B5);
        if h(1) < 0.35 {
            continue;
        }
        let x = -100.0 + 200.0 * (i as f32 + h(2)) / HEDGES as f32;
        let mut m = MeshInstance::new(MeshId::SPHERE, [x, -0.3, -(MEADOW_HALF + 3.0 + 5.0 * h(3))]);
        m.scale = [3.0 + 9.0 * h(4), 1.0 + 1.6 * h(5), 2.5 + 2.0 * h(6)];
        m.material = ground;
        let v = 0.75 + 0.4 * h(7);
        m.color = [0.1 * v, 0.24 * v, 0.08 * v, 1.0];
        rw.draw_mesh(m);
    }
    // The hills: flattened domes sunk into the ground, so only a low crown
    // shows over the skyline; each its own shade of green (fields and
    // woods under the haze), the far rows bluer.
    for (k, (bearing, dist, half, height, far)) in HILL_ROWS.into_iter().enumerate() {
        let height = height * HILL_SCALE;
        let b = (HEADING + bearing).to_radians();
        let at = [b.sin() * dist, -height * 0.6, b.cos() * dist];
        let mut m = MeshInstance::new(MeshId::SPHERE, at);
        m.scale = [half * 2.0, height * 3.2, half * 1.2];
        m.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], b);
        m.material = ground;
        let c = if far { far_hills } else { hills };
        let h = |q: u32| hash01((k as u32).wrapping_mul(31).wrapping_add(q.wrapping_mul(7717)) ^ 0x51ED_270B);
        let v = 0.8 + 0.4 * h(1);
        m.color = [c[0] * v * (0.9 + 0.25 * h(2)), c[1] * v, c[2] * v * (0.9 + 0.2 * h(3)), 1.0];
        rw.draw_mesh(m);
    }
    // Tree lines and hedges at the foot of the hills: long, low, dark
    // green strips that break the skyline's band into layers.
    for i in 0..TREE_LINES {
        let h = |q: u32| hash01(i.wrapping_mul(6007).wrapping_add(q.wrapping_mul(92_821)) ^ 0x2C1B_3C6D);
        let b = (HEADING + (h(1) - 0.5) * 150.0).to_radians();
        let dist = 190.0 + 150.0 * h(2);
        let mut m = MeshInstance::new(MeshId::SPHERE, [b.sin() * dist, -0.2, b.cos() * dist]);
        m.scale = [18.0 + 40.0 * h(3), 2.2 + 2.6 * h(4), 3.0];
        m.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], b);
        m.material = ground;
        let v = 0.75 + 0.35 * h(5);
        m.color = [0.16 * v, 0.34 * v, 0.12 * v, 1.0];
        rw.draw_mesh(m);
    }
    // Weed clumps and tufts in the meadow ahead (a fixed seeded scatter in
    // the view's wedge, 16 to 76 m out, denser near): low domes a little
    // darker or yellower than the grass round them, barely raised.
    for i in 0..CLUMPS {
        let h = |k: u32| hash01(i.wrapping_mul(7919).wrapping_add(k.wrapping_mul(104_729)));
        let dist = 16.0 + 60.0 * h(1) * h(1);
        let b = (HEADING + (h(2) - 0.5) * 120.0).to_radians();
        let w = 0.5 + 2.2 * h(3);
        let mut m = MeshInstance::new(MeshId::SPHERE, [b.sin() * dist, -0.04, b.cos() * dist]);
        m.scale = [w, 0.1 + 0.18 * h(4), w * (0.6 + 0.6 * h(5))];
        m.material = ground;
        let k = 0.62 + 0.3 * h(6);
        let y = h(7);
        m.color = [meadow[0] * k * (0.8 + 0.5 * y), meadow[1] * k, meadow[2] * k * (0.7 + 0.6 * y), 1.0];
        rw.draw_mesh(m);
    }
    // Grass at the player's scale (round 2): tufts of blades (thin
    // leaning cones, fresh green to a little straw) all over the meadow
    // ahead, denser near, a pale seed head on every fourth.
    for i in 0..TUFTS {
        let h = |k: u32| hash01(i.wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)) ^ 0x7F4A_7C15);
        let dist = 16.0 + 60.0 * h(1).powf(1.7);
        let b = (HEADING + (h(2) - 0.5) * 110.0).to_radians();
        let at = [b.sin() * dist, 0.0, b.cos() * dist];
        let tall = 0.25 + 0.45 * h(3);
        let wide = 0.08 + 0.18 * h(4);
        let mut m = MeshInstance::new(MeshId::CONE, at);
        m.scale = [wide, tall, wide * (0.6 + 0.8 * h(5))];
        let lean = h(6) * std::f32::consts::TAU;
        m.rotation = evani_render::math::quat_axis_angle([lean.cos(), 0.0, lean.sin()], 0.35 * h(8));
        m.material = ground;
        // Fresh green, yellow-green or dry straw (as shares of the
        // meadow's colour), lighter or darker.
        let dry = h(9);
        let v = 0.8 + 0.45 * h(10);
        let c = if dry > 0.9 {
            [1.25, 0.98, 1.4]
        } else if dry > 0.45 {
            [1.05, 1.04, 1.07]
        } else {
            [0.75, 0.92, 0.95]
        };
        m.color = [meadow[0] * c[0] * v, meadow[1] * c[1] * v, meadow[2] * c[2] * v, 1.0];
        rw.draw_mesh(m);
        if i % SEED_EVERY == 0 {
            let mut s = MeshInstance::new(MeshId::LOW_SPHERE, [at[0], tall * 0.95, at[2]]);
            let r = 0.05 + 0.05 * h(11);
            s.scale = [r, r * 1.6, r];
            s.material = ground;
            s.color = [1.8, 1.8, 1.15, 1.0];
            rw.draw_mesh(s);
        }
    }
}

/// Tree lines along the foot of the far hills (round 2).
const TREE_LINES: u32 = 18;
/// The farmland's rings of field patches past the meadow, (inner, outer) m
/// (round 2).
const FIELD_RINGS: [(f32, f32); 4] = [(84.0, 120.0), (120.0, 170.0), (170.0, 240.0), (240.0, 330.0)];
/// Bushes in the hedgerow along the meadow's edge (round 2).
const HEDGES: u32 = 26;

/// A fixed hash of `n` to 0..1 (the clumps' scatter; no runtime randomness).
fn hash01(n: u32) -> f32 {
    let mut x = n ^ 0x9E37_79B9;
    x = (x ^ (x >> 16)).wrapping_mul(0x7FEB_352D);
    x = (x ^ (x >> 15)).wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / 16_777_216.0
}
