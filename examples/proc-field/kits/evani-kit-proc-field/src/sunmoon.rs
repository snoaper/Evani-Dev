//! The sun and moon discs' scene (feature `sunmoon`, the Asset Studio sun
//! and moon session, column 38, 2026-11-08): the procedural library's sun
//! and moon discs (`sky-sun-and-moon-discs`: evani-material-proc's sky dome
//! with its `sky-discs` part, started with the library's photoreal recipe,
//! `evani_procedural_lib::sunmoon`) seen the way its reference is: from a
//! standing player's eye (1.7 m), a 60 degree horizontal view tilted up so
//! the horizon runs along the bottom eighth, a dark strip of open fields
//! and a far tree line, golden hour just before sunset: the sun a small
//! bright disc low over the horizon straight ahead with its glow, a pale
//! thin crescent moon high above it, both at their true size.
//!
//! The dome is installed the way a game does it (`ProcWorld::sky_only`, the
//! module's own draw system) under the photoreal look's daylight preset;
//! the scene light follows the dome's sun, so the fields are backlit. The
//! scene tells the discs its pixel's angle (`sky.disc_pixel`, the vertical
//! view over the picture's rows), as a game would.
//!
//! `kit.proc_field.sunmoon_view`: 0 the reference's view, 1 a long lens on
//! the moon (its crescent), 2 a long lens on the sun (its disc and glow).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural_lib::evani_grass::{Grass, Kind as GrassKind, Patch as GrassPatch};
use evani_procedural_lib::evani_material_proc as mp;
use evani_procedural_lib::sunmoon;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct SunMoon {
    view: F32Param,
    pitch: F32Param,
    fov: F32Param,
    rows: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    moon_az: F32Param,
    moon_el: F32Param,
    moon_tilt: F32Param,
    moon_phase: F32Param,
    moon_size: F32Param,
    tree_scale: F32Param,
    field: Vec3Param,
    trees: Vec3Param,
    light: F32Param,
    haze: Vec3Param,
    ambient: Vec3Param,
    /// The (view, fov, rows, sun bearing, sun elevation, moon bearing, moon
    /// elevation, moon tilt, moon phase, light, haze rgb, ambient rgb, moon
    /// size) the light, discs and haze were last set for (NaN: not yet).
    set_for: [f32; 17],
    /// Whether the meadow ahead was handed to evani-grass (round 5), and
    /// how many frames it has waited for the module.
    grass: bool,
    grass_wait: u32,
}

/// The reference's camera (1672 x 941): eye height (m), the tilt up that
/// puts the horizon at row 830 (88 percent down: 13.9 degrees), and the
/// vertical field of view of a 60 degree horizontal one at that aspect.
const EYE: f32 = 1.7;
const PITCH: f32 = 13.9;
const FOV_Y: f32 = 36.0;
/// The picture's rows (the discs are smoothed and sampled over one pixel).
const ROWS: f32 = 941.0;
/// The view 0 heading (degrees from +z towards +x): looking along -z.
const HEADING: f32 = 180.0;
/// The sun and moon, measured off the reference (bearings to the left of
/// the view, degrees): the sun's bright disc centred at (832.6, 778.1),
/// 1.9 degrees up and a tenth of a degree left of the view's middle; the
/// moon's disc round (845.5, 184.5), 25 degrees up, half a degree right.
const SUN_AZ: f32 = 0.11;
const SUN_EL: f32 = 1.9;
const MOON_AZ: f32 = -0.31;
const MOON_EL: f32 = 25.14;
/// The reference's crescent is lit on its lower right (its horns at about
/// one and seven o'clock) and thicker than a real moon 23 degrees from the
/// sun: the true sun lies straight below it, so the lit side is turned 45
/// degrees towards the right and lit as if 36 degrees from the sun (round
/// 2, chosen from 60/50, 45/40 and 75/60 on the long lens; the recipe's
/// own defaults are 0, the physical moon).
const MOON_TILT: f32 = -36.0;
/// Round 4: 30 (was 36), the reference's crescent is a little thinner.
/// Column 73: the crescent measured again on the reference with a fitted
/// circle (its lit limb towards 52 degrees below the right, centred at
/// 843.2, 183.2): the lit side turned 36 degrees and lit as if 22 degrees
/// from the sun (a thinner arc), the moon 0.14 degrees further left and up.
const MOON_PHASE: f32 = 22.0;
/// The moon's apparent diameter (degrees; column 73): this evening's moon
/// stands near its closest (the real moon's runs from 0.49 to 0.56), as the
/// reference's wide crescent.
const MOON_SIZE: f32 = 0.56;
/// The tree line's height, a share of the layers' own (round 2: the
/// reference's far woods rise only 5 to 8 pixels over the fields).
const TREE_SCALE: f32 = 1.0;
/// The fields' and tree line's colours (linear, before the backlight): a
/// dark olive-brown meadow (round 2: browner, the far field's red minus
/// blue was half the reference's; round 3: a little lighter, the near
/// clumps now darker and greener; round 4: a fifth lighter, the meadow's
/// own leaf masses darker than the blades) and near-black woods, as the
/// reference's.
const FIELD: [f32; 3] = [1.55, 1.31, 0.88];
const TREES: [f32; 3] = [0.08, 0.08, 0.12];
/// The scene light's strength (the low sun's, warm).
const LIGHT: f32 = 0.5;
/// The distance haze's colour (linear HDR, the look's fog colour): the
/// reference's far fields and woods are a hazy purple-grey, not black.
const HAZE: [f32; 3] = [0.06, 0.048, 0.065];
/// The dusk ambient: the light of the open sky overhead, dim, on the
/// backlit fields and woods (round 2: less blue, see `FIELD`).
const AMBIENT: [f32; 3] = [0.22, 0.2, 0.23];

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.sunmoon_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the sun and moon's reference view (a player's eye, the horizon low), 1 a long lens on the moon, 2 a long lens on the sun"),
    )?;
    let pitch = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_pitch", PITCH).range(0.0, 89.0).unit("deg").describe("The reference view's tilt up (degrees; 13.9 puts the horizon 88 percent down the frame)"))?;
    let fov = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_fov_y", FOV_Y).range(1.0, 120.0).unit("deg").describe("The vertical field of view (36: 60 degrees across at 1672 x 941)"))?;
    let rows = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_rows", ROWS).range(100.0, 4000.0).describe("The picture's rows: the discs are smoothed and sampled over one pixel"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_sun_az", SUN_AZ).range(-180.0, 180.0).unit("deg").describe("The sun's bearing to the left of the reference view (degrees)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_sun_el", SUN_EL).range(-5.0, 89.0).unit("deg").describe("The sun's elevation (degrees)"))?;
    let moon_az = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_moon_az", MOON_AZ).range(-180.0, 180.0).unit("deg").describe("The moon's bearing to the left of the reference view (degrees)"))?;
    let moon_el = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_moon_el", MOON_EL).range(-5.0, 89.0).unit("deg").describe("The moon's elevation (degrees)"))?;
    let moon_tilt = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_moon_tilt", MOON_TILT).range(-180.0, 180.0).unit("deg").describe("Turns the crescent's lit side from the true sun direction (degrees)"))?;
    let moon_phase = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_moon_phase", MOON_PHASE).range(0.0, 180.0).unit("deg").describe("The angle between the moon and its light (degrees; 0 the true one; wider, a thicker crescent)"))?;
    let moon_size = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_moon_size", MOON_SIZE).range(0.05, 10.0).unit("deg").describe("The moon's apparent diameter (degrees; the real moon's runs from 0.49 to 0.56)"))?;
    let tree_scale = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_tree_scale", TREE_SCALE).range(0.1, 4.0).describe("The far tree line's height, a share of its layers' own"))?;
    let field = kit.param_vec3(ParamDef::color("kit.proc_field.sunmoon_field", FIELD).range(0.0, 4.0).describe("The fields' colour (on the look's dirt material)"))?;
    let trees = kit.param_vec3(ParamDef::color("kit.proc_field.sunmoon_trees", TREES).range(0.0, 4.0).describe("The far tree line's colour"))?;
    let light = kit.param_f32(ParamDef::f32("kit.proc_field.sunmoon_light", LIGHT).range(0.0, 10.0).describe("The low sun's light on the fields"))?;
    let haze = kit.param_vec3(ParamDef::color("kit.proc_field.sunmoon_haze", HAZE).range(0.0, 4.0).describe("The distance haze's colour (linear, the look's fog)"))?;
    let ambient = kit.param_vec3(ParamDef::color("kit.proc_field.sunmoon_ambient", AMBIENT).range(0.0, 4.0).describe("The dusk sky's ambient light on the fields and woods"))?;
    // The dome, as a game installs it: the sky alone, started with the
    // library's sun and moon recipe.
    let mut pw = mp::ProcWorld::sky_only().map_err(|e| PluginError(format!("sunmoon: {e}")))?;
    pw.sky = sunmoon::photoreal_sky();
    pw.discs = sunmoon::photoreal();
    {
        let mut reg = kit.world.res_mut::<ParamRegistry>();
        pw.register_params(&mut reg).map_err(|e| PluginError(format!("sunmoon: {e}")))?;
    }
    kit.resource(pw);
    kit.system(mp::system());
    kit.resource(SunMoon { view, pitch, fov, rows, sun_az, sun_el, moon_az, moon_el, moon_tilt, moon_phase, moon_size, tree_scale, field, trees, light, haze, ambient, set_for: [f32::NAN; 17], grass: false, grass_wait: 0 });
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

/// The far tree line in three layers (round 2): `(distance m, crown height
/// m, colour lift, gap share)`. Each layer is a run of irregular trees,
/// every tree a heap of overlapping crowns of different sizes so its top
/// edge is broken; the farther layers lower and lighter in the haze, the
/// nearest broken by gaps. Low, as the reference's: a quarter to a third
/// of a degree over the fields, a darker band with a few taller trees.
/// Every layer stays inside the look's 450 m haze.
/// Round 3: clusters with small gaps in every layer, the farther layers
/// lighter in the haze (the reference's far woods separate in value).
const LAYERS: [(f32, f32, f32, f32); 3] = [(440.0, 1.7, 2.3, 0.15), (410.0, 2.1, 1.6, 0.35), (375.0, 2.5, 1.0, 0.6)];
/// The wood at the far left edge, a compact block as the reference's
/// (from the frame's edge to 26 degrees left, about a degree tall):
/// (bearing, distance, half length, height).
const LEFT_WOOD: (f32, f32, f32, f32) = (29.5, 300.0, 17.0, 6.2);

/// One tree: overlapping crowns round its top, `n` of them, sizes and
/// heights uneven (a ragged top edge, no single dome); `wide` scales its
/// crowns' width.
#[allow(clippy::too_many_arguments)]
fn tree(rw: &mut RenderWorld, at: [f32; 3], side: [f32; 2], height: f32, wide: f32, colour: [f32; 3], material: u32, seed: u32) {
    let h = |q: u32| hash01(seed.wrapping_mul(2_246_822_519).wrapping_add(q.wrapping_mul(3_266_489_917)) ^ 0x1B87_3593);
    let n = 5 + (h(0) * 6.0) as u32;
    let w = height * wide * (0.55 + 0.4 * h(1));
    for k in 0..n {
        let g = |q: u32| h(10 + k * 7 + q);
        let along = (g(1) - 0.5) * w * 1.3;
        let r = w * (0.25 + 0.3 * g(2));
        let top = height * (0.65 + 0.35 * g(3));
        let mut m = MeshInstance::new(MeshId::LOW_SPHERE, [at[0] + side[0] * along, top - r, at[2] + side[1] * along]);
        m.scale = [r * 2.0, r * 2.0 * (0.8 + 0.4 * g(4)), r * 2.0];
        m.material = material;
        let v = 0.85 + 0.3 * g(5);
        m.color = [colour[0] * v, colour[1] * v, colour[2] * v, 1.0];
        rw.draw_mesh(m);
    }
    // The trunk-and-understorey mass under the crowns, down to the ground.
    let mut m = MeshInstance::new(MeshId::CUBE, [at[0], height * 0.3, at[2]]);
    m.scale = [w * 0.9, height * 0.6, w * 0.6];
    m.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], -side[1].atan2(side[0]));
    m.material = material;
    m.color = [colour[0], colour[1], colour[2], 1.0];
    rw.draw_mesh(m);
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<SunMoon>();
    let (view, pitch, fov, rows, sun_az, sun_el, moon_az, moon_el, moon_tilt, moon_phase, moon_size, tree_scale, light, haze, ambient, field, trees) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round().clamp(0.0, 2.0),
            reg.f32(g.pitch),
            reg.f32(g.fov),
            reg.f32(g.rows),
            reg.f32(g.sun_az),
            reg.f32(g.sun_el),
            reg.f32(g.moon_az),
            reg.f32(g.moon_el),
            reg.f32(g.moon_tilt),
            reg.f32(g.moon_phase),
            reg.f32(g.moon_size),
            reg.f32(g.tree_scale),
            reg.f32(g.light),
            reg.vec3(g.haze),
            reg.vec3(g.ambient),
            reg.vec3(g.field),
            reg.vec3(g.trees),
        )
    };
    // The long lenses look straight at each disc (2.5 degrees tall).
    let (cam, fov_now) = match view as u32 {
        1 => (eye_camera(HEADING + moon_az, moon_el, 2.5), 2.5),
        2 => (eye_camera(HEADING + sun_az, sun_el, 2.5), 2.5),
        _ => (eye_camera(HEADING, pitch, fov), fov),
    };
    let now = [view, fov_now, rows, sun_az, sun_el, moon_az, moon_el, moon_tilt, moon_phase, light, haze[0], haze[1], haze[2], ambient[0], ambient[1], ambient[2], moon_size];
    if now != g.set_for {
        let sun = mp::sky::sun_direction(sun_el, HEADING + sun_az);
        let mut reg = world.res_mut::<ParamRegistry>();
        // The low sun, warm and weak; the dome's sun follows it.
        set(&mut reg, "render.light.direction", ParamValue::Vec3(sun));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.6, 0.32]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(light));
        set(&mut reg, "sky.follow_light", ParamValue::Bool(true));
        set(&mut reg, "sky.moon_elevation", ParamValue::F32(moon_el));
        set(&mut reg, "sky.moon_azimuth", ParamValue::F32(wrap180(HEADING + moon_az)));
        set(&mut reg, "sky.moon_tilt", ParamValue::F32(moon_tilt));
        set(&mut reg, "sky.moon_phase", ParamValue::F32(moon_phase));
        set(&mut reg, "sky.moon_size", ParamValue::F32(moon_size));
        // The recipe's grade (column 73): the saturation its sky is fitted
        // under, as a game with the photoreal look sets it.
        sunmoon::apply_photoreal_grade(&mut reg);
        // One pixel's angle in this view, as a game tells the discs.
        set(&mut reg, "sky.disc_pixel", ParamValue::F32(fov_now / rows.max(1.0)));
        // The distance haze and the dusk sky's ambient.
        set(&mut reg, "render.clear_color", ParamValue::Color(haze));
        set(&mut reg, "render.ambient", ParamValue::Color(ambient));
        drop(reg);
        g.set_for = now;
        *world.res_mut::<SunMoon>() = g;
    }
    // The meadow ahead (round 5): real grass from evani-grass, handed over
    // once as a game does (the library's long grass brings the module into
    // the build). A knee-high late-summer meadow of dark olive blades with
    // dry, straw-tipped ones in patches and seed heads, in clumps, out to
    // where the far fields take over; no carpet of its own (the fields are
    // the ground).
    if !g.grass {
        if let Some(gr) = world.try_res::<Grass>() {
            gr.clear_patches();
            // Round 6: the meadow stands on hummocks and hollows a metre to
            // five across (a height grid under the patch; the fields' flat
            // ground stays at 0), so it grows in raised dark masses with
            // broad gaps where the hollows sink its blades below the
            // ground, instead of one even carpet.
            let (lo, hi) = ([-50.0f32, -75.0f32], [50.0f32, -0.3f32]);
            let mut hs = Vec::with_capacity(RELIEF_N * RELIEF_N);
            for j in 0..RELIEF_N {
                for i in 0..RELIEF_N {
                    let x = lo[0] + (hi[0] - lo[0]) * i as f32 / (RELIEF_N - 1) as f32;
                    let z = lo[1] + (hi[1] - lo[1]) * j as f32 / (RELIEF_N - 1) as f32;
                    let n = 0.45 * vnoise(x / 5.5, z / 4.0, 7) + 0.35 * vnoise(x / 2.4, z / 2.0, 11) + 0.2 * vnoise(x / 1.1, z / 1.1, 23);
                    hs.push(RELIEF * n - SINK);
                }
            }
            let _ = gr.add_patch_on(GrassPatch::new(GrassKind::Meadow, lo, hi).feather(12.0), RELIEF_N, &hs);
            drop(gr);
            let mut reg = world.res_mut::<ParamRegistry>();
            for (name, v) in MEADOW {
                set(&mut reg, name, v);
            }
            drop(reg);
            g.grass = true;
        } else {
            g.grass_wait += 1;
            g.grass = g.grass_wait > 120;
        }
        *world.res_mut::<SunMoon>() = g;
    }
    let ground = RenderWorld::material(&world.res::<LookData>(), "dirt");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // The fields out to the tree line (inside the dome's 500 m).
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
    f.scale = [900.0, 1.0, 900.0];
    f.material = ground;
    f.color = [field[0], field[1], field[2], 1.0];
    rw.draw_mesh(f);
    // Field patches: broad strips a little lighter, darker or yellower
    // (seeded, fixed), more and stronger far out (round 2: the far field's
    // texture was half the reference's), so the dark strip is not one flat
    // colour.
    for i in 0..PATCHES {
        let h = |q: u32| hash01(i.wrapping_mul(977).wrapping_add(q.wrapping_mul(7919)) ^ 0x3A5F_1E27);
        let b = (HEADING + (h(1) - 0.5) * 80.0).to_radians();
        let d = 30.0 + 390.0 * h(2).sqrt();
        let mut p = MeshInstance::new(MeshId::PLANE, [b.sin() * d, 0.01 + 0.002 * (i % 40) as f32, b.cos() * d]);
        p.scale = [d * (0.06 + 0.2 * h(3)), 1.0, d * (0.03 + 0.08 * h(4))];
        p.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], b + (h(5) - 0.5) * 0.8);
        p.material = ground;
        // No patch darker than about half the field (round 4: a darker one
        // read as a black bar near the skyline).
        let v = 0.42 + 1.18 * h(6);
        let y = 0.8 + 0.45 * h(7);
        p.color = [field[0] * v * y, field[1] * v, field[2] * v * (1.15 - 0.35 * y), 1.0];
        rw.draw_mesh(p);
    }
    // Field boundaries between the far fields (round 4): long low dark
    // strips at 110 to 330 m, under the eye's height so they stay below
    // the skyline, so the far field reads as fields at different
    // distances; the farther ones lighter in the haze.
    for i in 0..HEDGES {
        let h = |q: u32| hash01(i.wrapping_mul(7_919).wrapping_add(q.wrapping_mul(104_729)) ^ 0x51ED_270B);
        let d = 110.0 + 220.0 * (i as f32 + h(1)) / HEDGES as f32;
        let b = (HEADING + (h(2) - 0.5) * 60.0).to_radians();
        let tall = 0.4 + 0.5 * h(4);
        let lift = 0.4 + 0.25 * (d - 110.0) / 220.0;
        // Round 5: each boundary a run of uneven segments (heights, lengths,
        // small kinks and gaps), not one straight bar.
        let len = d * (0.05 + 0.15 * h(3));
        let segs = 3 + (h(6) * 5.0) as u32;
        let turn = b + (h(5) - 0.5) * 0.3;
        let (ux, uz) = (turn.cos(), -turn.sin());
        for k in 0..segs {
            let q = |n: u32| hash01(i.wrapping_mul(131).wrapping_add(k.wrapping_mul(977)).wrapping_add(n.wrapping_mul(7)) ^ 0x2F6B_1D03);
            if q(1) < 0.2 {
                continue;
            }
            let along = (k as f32 + 0.5) / segs as f32 * len - len * 0.5;
            let side = (q(2) - 0.5) * 3.0;
            let t = tall * (0.5 + 0.7 * q(3));
            let at = [b.sin() * d + ux * along - uz * side, t * 0.5, b.cos() * d + uz * along + ux * side];
            // Round 6: flat on the ground (a darker strip of rough grass),
            // so no rounded outline shows against the far fields.
            let at = [at[0], 0.03 + 0.001 * k as f32, at[2]];
            let _ = t;
            let mut m = MeshInstance::new(MeshId::PLANE, at);
            m.scale = [len / segs as f32 * (0.7 + 0.6 * q(4)), 1.0, 3.0 + 4.0 * q(5)];
            m.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], turn + (q(6) - 0.5) * 0.25);
            m.material = ground;
            let v = 0.8 + 0.4 * q(7);
            m.color = [field[0] * lift * 0.85 * v, field[1] * lift * 0.85 * v, field[2] * lift * v, 1.0];
            rw.draw_mesh(m);
        }
        let _ = ground;
    }
    // The far tree line: three layers of irregular trees across the view.
    let mut seed = 1u32;
    for (layer, (dist, height, lift, gaps)) in LAYERS.into_iter().enumerate() {
        let height = height * tree_scale;
        let c = [trees[0] * lift, trees[1] * lift, trees[2] * lift * 1.1];
        let mut bearing = -34.0f32;
        while bearing < 34.0 {
            seed = seed.wrapping_add(1);
            let h = |q: u32| hash01(seed.wrapping_mul(6007).wrapping_add(q.wrapping_mul(92_821)).wrapping_add(layer as u32) ^ 0x2C1B_3C6D);
            let spacing = (height * (0.8 + 0.9 * h(1))) / dist * 57.3;
            bearing += spacing;
            // Gaps come in runs: a slow wave along the line decides them.
            let wave = (bearing * 0.21 + layer as f32 * 1.7).sin() * 0.5 + 0.5;
            if wave * h(2) < gaps * 0.6 && h(3) < gaps {
                continue;
            }
            let b = (HEADING + bearing).to_radians();
            let d = dist + (h(4) - 0.5) * 20.0;
            let at = [b.sin() * d, 0.0, b.cos() * d];
            // Clusters rise and fall along the line (a slow wave), now and
            // then a taller tree stands out.
            // Round 4: deeper troughs between the groups, and a gap where a
            // trough is low.
            let wave2 = (bearing * 0.37 + layer as f32 * 2.3).sin() * 0.5 + 0.5;
            if wave2 < 0.22 && h(7) < 0.7 {
                continue;
            }
            let cluster = 0.45 + 0.9 * wave2;
            let tall = if h(5) > 0.88 { 1.35 + 0.35 * h(6) } else { 0.55 + 0.6 * h(6) };
            tree(&mut rw, at, [b.cos(), -b.sin()], height * tall * cluster, 1.0, c, ground, seed);
        }
    }
    // The wood at the far left: a block of trees of every size with small
    // crowns, its top edge rising and falling, flat-topped and ending
    // steeply.
    let (lb, ld, lh, lt) = LEFT_WOOD;
    for k in 0..56u32 {
        let h = |q: u32| hash01(k.wrapping_mul(4099).wrapping_add(q.wrapping_mul(65_537)) ^ 0x6B43_A9B5);
        let along = (k as f32 / 55.0 - 0.5) * 2.0 * lh + (h(1) - 0.5) * 2.0;
        let b = (HEADING + lb).to_radians();
        let (fx, fz) = (b.sin(), b.cos());
        let d = ld + (h(2) - 0.5) * 14.0;
        let at = [fx * d + fz * along, 0.0, fz * d - fx * along];
        let edge = 1.0 - (along / lh).abs().min(1.0).powf(6.0);
        let height = lt * (0.35 + 0.65 * edge) * (0.75 + 0.4 * h(3));
        // Nearer and denser than the far line: near black, as the
        // reference's (its pixels about 8 of 255).
        tree(&mut rw, at, [fz, -fx], height, 0.55, [trees[0] * 0.3, trees[1] * 0.3, trees[2] * 0.3], ground, 9000 + k);
    }
    // Sparse groups of yellow flowers standing in the grass (the reference's
    // yellow plants), a few in the foreground.
    for i in 0..FLOWER_GROUPS {
        let h = |k: u32| hash01(i.wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)) ^ 0x7F4A_7C15);
        let dist = 8.0 + 50.0 * h(1).powf(1.4);
        let bear = (HEADING + (h(2) - 0.5) * 64.0).to_radians();
        let centre = [bear.sin() * dist, bear.cos() * dist];
        let spread = 0.3 + 0.6 * h(3);
        for j in 0..(4 + (h(9) * 8.0) as u32) {
            let k = |q: u32| h(200 + j * 5 + q);
            let a = k(1) * std::f32::consts::TAU;
            let r = spread * k(2);
            let mut s = MeshInstance::new(MeshId::LOW_SPHERE, [centre[0] + a.cos() * r, 0.45 + 0.3 * k(3), centre[1] + a.sin() * r]);
            let size = 0.03 + 0.03 * k(4);
            s.scale = [size, size * 0.7, size];
            s.material = ground;
            s.color = [field[0] * 1.6, field[1] * 1.5, field[2] * 0.25, 1.0];
            rw.draw_mesh(s);
        }
    }
}

/// Broad field patches.
const PATCHES: u32 = 220;
/// The meadow's height grid: samples per side (the grass module's most),
/// how far its hummocks rise and fall (m) and how far it is sunk, so the
/// deeper hollows hide their blades under the fields' flat ground.
const RELIEF_N: usize = 129;
const RELIEF: f32 = 0.8;
const SINK: f32 = 0.18;
/// Groups of yellow flowers in the meadow ahead.
const FLOWER_GROUPS: u32 = 26;

/// The meadow's grass (evani-grass's long meadow kind), set once when the
/// patch is handed over; live params, so a sweep can change them after.
const MEADOW: [(&str, ParamValue); 20] = [
    ("material.grass.ground", ParamValue::Bool(false)),
    ("material.grass.quality", ParamValue::F32(3.0)),
    ("material.grass.distance", ParamValue::F32(2.0)),
    // Backlit by a sun two degrees up: little glow through the blades and
    // no highlight, as the reference's dark meadow (the module's defaults,
    // 1 and 1, light it like a field at noon).
    ("material.grass.translucency", ParamValue::F32(0.05)),
    ("material.grass.specular", ParamValue::F32(0.0)),
    ("material.grass.meadow.height", ParamValue::F32(0.5)),
    ("material.grass.meadow.height_var", ParamValue::F32(0.8)),
    ("material.grass.meadow.clump", ParamValue::F32(1.0)),
    ("material.grass.meadow.heads", ParamValue::F32(0.0)),
    ("material.grass.meadow.width", ParamValue::F32(0.03)),
    ("material.grass.meadow.density", ParamValue::F32(140.0)),
    ("material.grass.meadow.dryness", ParamValue::F32(0.3)),
    ("material.grass.meadow.bare", ParamValue::F32(0.2)),
    ("material.grass.meadow.droop", ParamValue::F32(0.4)),
    ("material.grass.meadow.variation", ParamValue::F32(0.35)),
    ("material.grass.round", ParamValue::F32(0.3)),
    // Round 6: lit mostly by the dusk sky (the scene's sun 0.5, its sky
    // light up), so the blades' own olive-brown, wider blades a pixel
    // across at 40 m and no bright seed heads: no fine sparkle.
    ("material.grass.meadow.tip", ParamValue::Color([0.1, 0.115, 0.11])),
    ("material.grass.meadow.root", ParamValue::Color([0.024, 0.028, 0.032])),
    ("material.grass.meadow.dry", ParamValue::Color([0.23, 0.19, 0.13])),
    ("material.grass.wetness", ParamValue::F32(0.0)),
];
/// Field boundaries between the far fields.
const HEDGES: u32 = 14;

fn wrap180(a: f32) -> f32 {
    let w = (a + 180.0).rem_euclid(360.0) - 180.0;
    if w == -180.0 { 180.0 } else { w }
}

/// Smooth value noise in -1..1 over a unit lattice (fixed, seeded).
fn vnoise(x: f32, z: f32, seed: u32) -> f32 {
    let (xf, zf) = (x.floor(), z.floor());
    let (tx, tz) = (x - xf, z - zf);
    let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
    let c = |dx: i32, dz: i32| {
        let xi = (xf as i32).wrapping_add(dx) as u32;
        let zi = (zf as i32).wrapping_add(dz) as u32;
        hash01(xi.wrapping_mul(73_856_093) ^ zi.wrapping_mul(19_349_663) ^ seed.wrapping_mul(83_492_791))
    };
    let a = c(0, 0) + (c(1, 0) - c(0, 0)) * sx;
    let b = c(0, 1) + (c(1, 1) - c(0, 1)) * sx;
    (a + (b - a) * sz) * 2.0 - 1.0
}

/// A fixed hash of `n` to 0..1 (the scatter; no runtime randomness).
fn hash01(n: u32) -> f32 {
    let mut x = n ^ 0x9E37_79B9;
    x = (x ^ (x >> 16)).wrapping_mul(0x7FEB_352D);
    x = (x ^ (x >> 15)).wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / 16_777_216.0
}
