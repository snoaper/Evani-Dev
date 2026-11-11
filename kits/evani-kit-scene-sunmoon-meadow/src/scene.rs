//! The meadow, the fields, the woods, the light and (kit feature `sky`) the
//! sky dome, the sun and moon discs and the grass: Asset Studio column 38's
//! round 8 (commit f4266d5c, `examples/proc-field/.../sunmoon.rs`), its
//! fixed camera replaced by the flying one ([`crate::Flyer`]) and its fixed
//! golden hour by the clock ([`crate::sun_at`]).
//!
//! The column's view (straight ahead at the start, along -z) keeps the
//! round's exact fields, three far tree lines and the wood on the left; the
//! rest of the circle gets fields and one far tree line of the same make so
//! the flyer can turn round. The meadow's grass covers the whole fly area.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_render::{MeshId, MeshInstance, RenderWorld};

use crate::{Flyer, HEADING, SunMoonMeadow};

/// The sun's and moon's bearings to the left of the column's view, and the
/// moon's height, as measured off the column's reference.
pub const SUN_AZ: f32 = 0.11;
pub const MOON_AZ: f32 = -0.45;
pub const MOON_EL: f32 = 25.0;
/// The reference's crescent (round 2 and 4: turned 45 degrees, lit as if 30
/// degrees from the sun).
const MOON_TILT: f32 = -45.0;
const MOON_PHASE: f32 = 30.0;
/// The column's golden hour (round 8): the fields, the woods, the low warm
/// sun's light, the haze and the dusk ambient.
const FIELD: [f32; 3] = [1.55, 1.31, 0.88];
const TREES: [f32; 3] = [0.08, 0.08, 0.12];
const LIGHT: f32 = 0.5;
const LIGHT_GOLD: [f32; 3] = [1.0, 0.6, 0.32];
const HAZE: [f32; 3] = [0.06, 0.048, 0.065];
const AMBIENT: [f32; 3] = [0.22, 0.2, 0.23];
/// Full day (the sun 20 degrees up and more): the sky dome scene's
/// mid-morning sun (column 21) and the daylight look's ambient.
const LIGHT_DAY: f32 = 2.9;
const LIGHT_NOON: [f32; 3] = [1.0, 0.96, 0.9];
const AMBIENT_DAY: [f32; 3] = [0.40, 0.37, 0.33];
/// The day haze as a share of the dome's horizon colour (column 21), and a
/// plain one without the sky module.
#[cfg(feature = "sky")]
const HAZE_SHARE: f32 = 0.35;
const HAZE_DAY: [f32; 3] = [0.35, 0.42, 0.5];
/// Night: a weak cool moonlight from the moon, a dark blue ambient and haze
/// (about the dome's night horizon).
const MOON_LIGHT: f32 = 0.2;
const MOON_COLOUR: [f32; 3] = [0.55, 0.66, 1.0];
const AMBIENT_NIGHT: [f32; 3] = [0.03, 0.04, 0.07];
const HAZE_NIGHT: [f32; 3] = [0.008, 0.011, 0.024];
/// The dome's brightness and haze through the day. Round 8's sky (3.05,
/// turbidity 8.1) is tuned for a sun 1.9 degrees up; a high sun lights the
/// dome about ten times as much, so the day sky dims to 0.4 and clears to
/// turbidity 4 (a plain blue sky) as the sun climbs.
#[cfg(feature = "sky")]
const SKY_BRIGHT_DAY: f32 = 0.4;
#[cfg(feature = "sky")]
const SKY_TURB_DAY: f32 = 4.0;
/// After sunset the dome's own sun stays just under the horizon (lower, its
/// scattering model goes black) while the sky fades from round 8's
/// brightness to a moonlit navy by the time the true sun is 10 degrees down:
/// an afterglow at dusk, a dark blue night.
#[cfg(feature = "sky")]
const DOME_SUN_MIN: f32 = -1.0;
#[cfg(feature = "sky")]
const SKY_BRIGHT_NIGHT: f32 = 0.8;
/// The picture's rows the discs are smoothed over (a 1080-row canvas).
const ROWS: f32 = 1080.0;

/// The scene's colour handles and what the light was last set for.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    field: Vec3Param,
    trees: Vec3Param,
    light: F32Param,
    haze: Vec3Param,
    ambient: Vec3Param,
    moon_tilt: F32Param,
    moon_phase: F32Param,
    set_for: [f32; 16],
    #[cfg(feature = "sky")]
    grass: bool,
    #[cfg(feature = "sky")]
    grass_wait: u32,
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let p = |n: &str| format!("kit.scene_sunmoon_meadow.{n}");
    let field = kit.param_vec3(
        ParamDef::color(&p("field"), FIELD)
            .range(0.0, 4.0)
            .describe("The fields' colour (on the look's dirt material)"),
    )?;
    let trees = kit.param_vec3(
        ParamDef::color(&p("trees"), TREES)
            .range(0.0, 4.0)
            .describe("The far tree lines' colour"),
    )?;
    let light = kit.param_f32(ParamDef::f32(&p("light"), LIGHT).range(0.0, 10.0).describe(
        "The low golden-hour sun's light on the fields (it grows to the day's as the sun climbs)",
    ))?;
    let haze = kit.param_vec3(
        ParamDef::color(&p("haze"), HAZE)
            .range(0.0, 4.0)
            .describe("The golden-hour distance haze (linear, the look's fog)"),
    )?;
    let ambient = kit.param_vec3(
        ParamDef::color(&p("ambient"), AMBIENT)
            .range(0.0, 4.0)
            .describe("The golden-hour sky's ambient light on the fields and woods"),
    )?;
    let moon_tilt = kit.param_f32(
        ParamDef::f32(&p("moon_tilt"), MOON_TILT)
            .range(-180.0, 180.0)
            .unit("deg")
            .describe("Turns the crescent's lit side from the true sun direction"),
    )?;
    let moon_phase = kit.param_f32(
        ParamDef::f32(&p("moon_phase"), MOON_PHASE)
            .range(0.0, 180.0)
            .unit("deg")
            .describe("The angle between the moon and its light (wider: a thicker moon)"),
    )?;
    #[cfg(feature = "sky")]
    {
        use evani_procedural_lib::evani_material_proc as mp;
        use evani_procedural_lib::sunmoon;
        // The dome, as a game installs it: the sky alone, started with the
        // library's sun and moon recipe and round 8's disc values.
        let mut pw =
            mp::ProcWorld::sky_only().map_err(|e| PluginError(format!("sunmoon meadow: {e}")))?;
        pw.sky = sunmoon::photoreal_sky();
        pw.discs = sunmoon::photoreal();
        pw.discs.glow = 16.0;
        pw.discs.core = 0.07;
        pw.discs.disc_bright = 0.003;
        pw.discs.warm_green = 0.55;
        {
            let mut reg = kit.world.res_mut::<ParamRegistry>();
            pw.register_params(&mut reg)
                .map_err(|e| PluginError(format!("sunmoon meadow: {e}")))?;
        }
        kit.resource(pw);
        kit.system(mp::system());
    }
    kit.resource(Look {
        field,
        trees,
        light,
        haze,
        ambient,
        moon_tilt,
        moon_phase,
        set_for: [f32::NAN; 16],
        #[cfg(feature = "sky")]
        grass: false,
        #[cfg(feature = "sky")]
        grass_wait: 0,
    });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name) {
        let _ = reg.set(id, v);
    }
}

fn dir(el: f32, az: f32) -> [f32; 3] {
    let (e, a) = (el.to_radians(), az.to_radians());
    [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()]
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn wrap180(a: f32) -> f32 {
    let w = (a + 180.0).rem_euclid(360.0) - 180.0;
    if w == -180.0 { 180.0 } else { w }
}

/// The light, sky, haze and ambient for the hour: exactly the column's
/// golden hour at the start (the sun 1.9 degrees up), the day's sun as it
/// climbs, dusk as it sets and moonlight at night.
fn set_light(world: &mut World, g: &mut Look, hour: f32, fov: f32) {
    let (el, az) = crate::sun_at(hour);
    let (light, haze, ambient, field, moon_tilt, moon_phase) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.light),
            reg.vec3(g.haze),
            reg.vec3(g.ambient),
            reg.vec3(g.field),
            reg.f32(g.moon_tilt),
            reg.f32(g.moon_phase),
        )
    };
    let now = [
        hour, fov, light, haze[0], haze[1], haze[2], ambient[0], ambient[1], ambient[2], moon_tilt,
        moon_phase, field[0], 0.0, 0.0, 0.0, 0.0,
    ];
    if now == g.set_for {
        return;
    }
    g.set_for = now;
    let sun = dir(el, HEADING + az);
    // `dusk`: 1 from the sun on the horizon up, 0 at night; `day`: 0 at
    // the column's golden hour, 1 from 20 degrees up.
    let dusk = smooth(-8.0, 1.0, el);
    let day = smooth(2.0, 20.0, el);
    // The dome's sun height, brightness and haze for the hour (exactly
    // round 8's at its golden hour, where `day` is 0).
    #[cfg(feature = "sky")]
    let (dome_el, sky_bright, sky_turb) = {
        let gold = evani_procedural_lib::sunmoon::photoreal_sky();
        let lerp_log = |a: f32, b: f32, t: f32| a * (b / a).powf(t);
        if el >= DOME_SUN_MIN {
            (
                el,
                lerp_log(gold.brightness, SKY_BRIGHT_DAY, day),
                gold.turbidity + (SKY_TURB_DAY - gold.turbidity) * day,
            )
        } else {
            let night = 1.0 - smooth(-10.0, DOME_SUN_MIN, el);
            (
                DOME_SUN_MIN,
                lerp_log(gold.brightness, SKY_BRIGHT_NIGHT, night),
                gold.turbidity,
            )
        }
    };
    #[cfg(not(feature = "sky"))]
    let dome_el = el;
    #[allow(unused_mut)]
    let mut haze_day = HAZE_DAY;
    #[cfg(feature = "sky")]
    if let Some(mut s) = world
        .try_res::<evani_procedural_lib::evani_material_proc::ProcWorld>()
        .map(|p| p.sky)
    {
        s.brightness = sky_bright;
        s.turbidity = sky_turb;
        let r = s.ring(sun, 0.02, 24);
        haze_day = [r[0] * HAZE_SHARE, r[1] * HAZE_SHARE, r[2] * HAZE_SHARE];
    }
    let (ldir, lcol, lint) = if el > -2.0 {
        (
            sun,
            mix3(LIGHT_GOLD, LIGHT_NOON, day),
            (light + (LIGHT_DAY - light) * day) * smooth(-2.0, 1.9, el),
        )
    } else {
        (
            dir(MOON_EL, HEADING + MOON_AZ),
            MOON_COLOUR,
            MOON_LIGHT * (1.0 - dusk),
        )
    };
    let amb = mix3(AMBIENT_NIGHT, mix3(ambient, AMBIENT_DAY, day), dusk);
    let fog = mix3(HAZE_NIGHT, mix3(haze, haze_day, day), dusk);
    let mut reg = world.res_mut::<ParamRegistry>();
    set(&mut reg, "render.light.direction", ParamValue::Vec3(ldir));
    set(&mut reg, "render.light.color", ParamValue::Color(lcol));
    set(&mut reg, "render.light.intensity", ParamValue::F32(lint));
    set(&mut reg, "render.clear_color", ParamValue::Color(fog));
    set(&mut reg, "render.ambient", ParamValue::Color(amb));
    // The dome's own sun (it may sink below the horizon at night, where the
    // light is the moon's) and the moon, fixed over the column's view.
    set(&mut reg, "sky.follow_light", ParamValue::Bool(false));
    set(
        &mut reg,
        "sky.sun_elevation",
        ParamValue::F32(dome_el.clamp(-20.0, 90.0)),
    );
    set(
        &mut reg,
        "sky.sun_azimuth",
        ParamValue::F32(wrap180(HEADING + az)),
    );
    #[cfg(feature = "sky")]
    {
        set(&mut reg, "sky.brightness", ParamValue::F32(sky_bright));
        set(&mut reg, "sky.turbidity", ParamValue::F32(sky_turb));
    }
    set(&mut reg, "sky.moon_elevation", ParamValue::F32(MOON_EL));
    set(
        &mut reg,
        "sky.moon_azimuth",
        ParamValue::F32(wrap180(HEADING + MOON_AZ)),
    );
    set(&mut reg, "sky.moon_tilt", ParamValue::F32(moon_tilt));
    set(&mut reg, "sky.moon_phase", ParamValue::F32(moon_phase));
    // One pixel's angle, as a game tells the discs.
    set(&mut reg, "sky.disc_pixel", ParamValue::F32(fov / ROWS));
}

/// The meadow's grass handed to evani-grass once (round 5-8's meadow, its
/// patch widened over the whole fly area).
#[cfg(feature = "sky")]
fn plant_grass(world: &mut World, g: &mut Look) {
    use evani_procedural_lib::evani_grass::{Grass, Kind as GrassKind, Patch as GrassPatch};
    if g.grass {
        return;
    }
    let Some(gr) = world.try_res::<Grass>() else {
        g.grass_wait += 1;
        g.grass = g.grass_wait > 120;
        return;
    };
    gr.clear_patches();
    let (lo, hi) = ([-GRASS_HALF, -GRASS_HALF], [GRASS_HALF, GRASS_HALF]);
    let mut hs = Vec::with_capacity(RELIEF_N * RELIEF_N);
    for j in 0..RELIEF_N {
        for i in 0..RELIEF_N {
            let x = lo[0] + (hi[0] - lo[0]) * i as f32 / (RELIEF_N - 1) as f32;
            let z = lo[1] + (hi[1] - lo[1]) * j as f32 / (RELIEF_N - 1) as f32;
            let n = 0.45 * vnoise(x / 5.5, z / 4.0, 7)
                + 0.35 * vnoise(x / 2.4, z / 2.0, 11)
                + 0.2 * vnoise(x / 1.1, z / 1.1, 23);
            hs.push(RELIEF * n - SINK);
        }
    }
    let _ = gr.add_patch_on(
        GrassPatch::new(GrassKind::Meadow, lo, hi).feather(30.0),
        RELIEF_N,
        &hs,
    );
    drop(gr);
    let mut reg = world.res_mut::<ParamRegistry>();
    for (name, v) in MEADOW {
        set(&mut reg, name, v);
    }
    g.grass = true;
}

/// The camera from the flyer.
fn camera(f: &Flyer, fov: f32) -> CameraPose {
    let fw = f.forward();
    CameraPose {
        eye: f.pos,
        target: [
            f.pos[0] + fw[0] * 10.0,
            f.pos[1] + fw[1] * 10.0,
            f.pos[2] + fw[2] * 10.0,
        ],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

pub fn draw(world: &mut World) {
    let Some(k) = world.try_res::<SunMoonMeadow>().map(|k| *k) else {
        return;
    };
    let mut g = *world.res::<Look>();
    let f = *world.res::<Flyer>();
    let (hour, fov, field, trees) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(k.tune.time),
            reg.f32(k.tune.fov),
            reg.vec3(g.field),
            reg.vec3(g.trees),
        )
    };
    set_light(world, &mut g, hour, fov);
    #[cfg(feature = "sky")]
    plant_grass(world, &mut g);
    *world.res_mut::<Look>() = g;
    // Headless tests run without the renderer.
    if world.try_res::<RenderWorld>().is_none() || world.try_res::<LookData>().is_none() {
        return;
    }
    let ground = RenderWorld::material(&world.res::<LookData>(), "dirt");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = camera(&f, fov);
    // The fields out to the tree lines (inside the dome's 500 m).
    let mut m = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
    m.scale = [900.0, 1.0, 900.0];
    m.material = ground;
    m.color = [field[0], field[1], field[2], 1.0];
    rw.draw_mesh(m);
    // Field patches: the column's wedge, then the rest of the circle.
    for (i, wedge) in (0..PATCHES)
        .map(|i| (i, true))
        .chain((0..PATCHES).map(|i| (i + 50_000, false)))
    {
        let h =
            |q: u32| hash01(i.wrapping_mul(977).wrapping_add(q.wrapping_mul(7919)) ^ 0x3A5F_1E27);
        let off = if wedge {
            (h(1) - 0.5) * 80.0
        } else {
            40.0 + 280.0 * h(1)
        };
        let b = (HEADING + off).to_radians();
        let d = 30.0 + 390.0 * h(2).sqrt();
        let mut p = MeshInstance::new(
            MeshId::PLANE,
            [b.sin() * d, 0.01 + 0.002 * (i % 40) as f32, b.cos() * d],
        );
        p.scale = [d * (0.04 + 0.1 * h(3)), 1.0, d * (0.03 + 0.08 * h(4))];
        p.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], b + (h(5) - 0.5) * 3.2);
        p.material = ground;
        let v = 0.6 + 0.8 * h(6);
        let y = 0.8 + 0.45 * h(7);
        p.color = [
            field[0] * v * y,
            field[1] * v,
            field[2] * v * (1.15 - 0.35 * y),
            1.0,
        ];
        rw.draw_mesh(p);
    }
    // Field boundaries: flat dark strips of rough grass between the far
    // fields, the column's in its wedge and as many again round the rest.
    for (i, wedge) in (0..HEDGES)
        .map(|i| (i, true))
        .chain((0..HEDGES * 3).map(|i| (i + 9_000, false)))
    {
        let h = |q: u32| {
            hash01(i.wrapping_mul(7_919).wrapping_add(q.wrapping_mul(104_729)) ^ 0x51ED_270B)
        };
        let n = if wedge { HEDGES } else { HEDGES * 3 };
        let d = 110.0 + 220.0 * ((i % 9_000) as f32 + h(1)) / n as f32;
        let off = if wedge {
            (h(2) - 0.5) * 60.0
        } else {
            35.0 + 290.0 * h(2)
        };
        let b = (HEADING + off).to_radians();
        let lift = 0.4 + 0.25 * (d - 110.0) / 220.0;
        let len = d * (0.05 + 0.15 * h(3));
        let segs = 3 + (h(6) * 5.0) as u32;
        let turn = b + (h(5) - 0.5) * 1.2;
        let (ux, uz) = (turn.cos(), -turn.sin());
        for s in 0..segs {
            let q = |n: u32| {
                hash01(
                    i.wrapping_mul(131)
                        .wrapping_add(s.wrapping_mul(977))
                        .wrapping_add(n.wrapping_mul(7))
                        ^ 0x2F6B_1D03,
                )
            };
            if q(1) < 0.2 {
                continue;
            }
            let along = (s as f32 + 0.5) / segs as f32 * len - len * 0.5;
            let side = (q(2) - 0.5) * 3.0;
            let at = [
                b.sin() * d + ux * along - uz * side,
                0.03 + 0.001 * s as f32,
                b.cos() * d + uz * along + ux * side,
            ];
            let mut m = MeshInstance::new(MeshId::PLANE, at);
            m.scale = [
                len / segs as f32 * (0.7 + 0.6 * q(4)),
                1.0,
                3.0 + 4.0 * q(5),
            ];
            m.rotation =
                evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], turn + (q(6) - 0.5) * 0.25);
            m.material = ground;
            let v = 0.8 + 0.4 * q(7);
            m.color = [
                field[0] * lift * 0.85 * v,
                field[1] * lift * 0.85 * v,
                field[2] * lift * v,
                1.0,
            ];
            rw.draw_mesh(m);
        }
    }
    // The far tree lines: the column's three layers across its view, then
    // the middle layer on round the rest of the circle.
    let mut seed = 1u32;
    for (layer, (dist, height, lift, gaps)) in LAYERS.into_iter().enumerate() {
        tree_line(
            &mut rw,
            &mut seed,
            layer,
            dist,
            height,
            lift,
            gaps,
            (-34.0, 34.0),
            trees,
            ground,
        );
    }
    // The wood at the far left: a block of trees of every size, a lighter
    // row behind it in the haze (round 8).
    let (lb, ld, lh, lt) = LEFT_WOOD;
    for (row, (back, shade, count)) in [(40.0f32, 0.75f32, 70u32), (0.0, 0.3, 110)]
        .into_iter()
        .enumerate()
    {
        for i in 0..count {
            let h = |q: u32| {
                hash01(
                    i.wrapping_mul(4099)
                        .wrapping_add(q.wrapping_mul(65_537))
                        .wrapping_add(row as u32 * 7)
                        ^ 0x6B43_A9B5,
                )
            };
            let along = (i as f32 / (count - 1) as f32 - 0.5) * 2.0 * (lh + back * 0.3)
                + (h(1) - 0.5) * 2.0;
            let b = (HEADING + lb).to_radians();
            let (fx, fz) = (b.sin(), b.cos());
            let d = ld + back + (h(2) - 0.5) * 18.0;
            let at = [fx * d + fz * along, 0.0, fz * d - fx * along];
            let edge = 1.0 - (along / (lh + back * 0.3)).abs().min(1.0).powf(4.0);
            let tall = if h(4) > 0.9 {
                1.25 + 0.3 * h(5)
            } else {
                0.6 + 0.5 * h(3)
            };
            let height = lt * (0.3 + 0.7 * edge) * tall * (1.0 - back / 200.0);
            tree(
                &mut rw,
                at,
                [fz, -fx],
                height,
                0.4,
                [trees[0] * shade, trees[1] * shade, trees[2] * shade * 1.1],
                ground,
                9000 + row as u32 * 500 + i,
            );
        }
    }
    let (dist, height, lift, gaps) = LAYERS[1];
    tree_line(
        &mut rw,
        &mut seed,
        1,
        dist,
        height,
        lift,
        gaps,
        (34.0, 326.0),
        trees,
        ground,
    );
    // Sparse groups of yellow flowers in the grass ahead.
    for i in 0..FLOWER_GROUPS {
        let h = |q: u32| {
            hash01(
                i.wrapping_mul(2_654_435_761)
                    .wrapping_add(q.wrapping_mul(40_503))
                    ^ 0x7F4A_7C15,
            )
        };
        let dist = 8.0 + 50.0 * h(1).powf(1.4);
        let bear = (HEADING + (h(2) - 0.5) * 64.0).to_radians();
        let centre = [bear.sin() * dist, bear.cos() * dist];
        let spread = 0.3 + 0.6 * h(3);
        for j in 0..(4 + (h(9) * 8.0) as u32) {
            let q = |n: u32| h(200 + j * 5 + n);
            let a = q(1) * std::f32::consts::TAU;
            let r = spread * q(2);
            let mut s = MeshInstance::new(
                MeshId::LOW_SPHERE,
                [
                    centre[0] + a.cos() * r,
                    0.45 + 0.3 * q(3),
                    centre[1] + a.sin() * r,
                ],
            );
            let size = 0.03 + 0.03 * q(4);
            s.scale = [size, size * 0.7, size];
            s.material = ground;
            s.color = [field[0] * 1.6, field[1] * 1.5, field[2] * 0.25, 1.0];
            rw.draw_mesh(s);
        }
    }
}

/// One far tree line between two bearings (to the left of the column's
/// view, degrees): irregular trees in clusters with gaps (rounds 2-4).
#[allow(clippy::too_many_arguments)]
fn tree_line(
    rw: &mut RenderWorld,
    seed: &mut u32,
    layer: usize,
    dist: f32,
    height: f32,
    lift: f32,
    gaps: f32,
    span: (f32, f32),
    trees: [f32; 3],
    material: u32,
) {
    let c = [trees[0] * lift, trees[1] * lift, trees[2] * lift * 1.1];
    let mut bearing = span.0;
    while bearing < span.1 {
        *seed = seed.wrapping_add(1);
        let s = *seed;
        let h = |q: u32| {
            hash01(
                s.wrapping_mul(6007)
                    .wrapping_add(q.wrapping_mul(92_821))
                    .wrapping_add(layer as u32)
                    ^ 0x2C1B_3C6D,
            )
        };
        bearing += (height * (0.8 + 0.9 * h(1))) / dist * 57.3;
        let wave = (bearing * 0.21 + layer as f32 * 1.7).sin() * 0.5 + 0.5;
        if wave * h(2) < gaps * 0.6 && h(3) < gaps {
            continue;
        }
        let b = (HEADING + bearing).to_radians();
        let d = dist + (h(4) - 0.5) * 20.0;
        let wave2 = (bearing * 0.37 + layer as f32 * 2.3).sin() * 0.5 + 0.5;
        if wave2 < 0.22 && h(7) < 0.7 {
            continue;
        }
        let cluster = 0.45 + 0.9 * wave2;
        let tall = if h(5) > 0.88 {
            1.35 + 0.35 * h(6)
        } else {
            0.55 + 0.6 * h(6)
        };
        tree(
            rw,
            [b.sin() * d, 0.0, b.cos() * d],
            [b.cos(), -b.sin()],
            height * tall * cluster,
            1.0,
            c,
            material,
            s,
        );
    }
}

/// One tree: overlapping crowns of uneven size round its top over a trunk
/// and understorey mass (rounds 2-7).
#[allow(clippy::too_many_arguments)]
fn tree(
    rw: &mut RenderWorld,
    at: [f32; 3],
    side: [f32; 2],
    height: f32,
    wide: f32,
    colour: [f32; 3],
    material: u32,
    seed: u32,
) {
    let h = |q: u32| {
        hash01(
            seed.wrapping_mul(2_246_822_519)
                .wrapping_add(q.wrapping_mul(3_266_489_917))
                ^ 0x1B87_3593,
        )
    };
    let n = 5 + (h(0) * 6.0) as u32;
    let w = height * wide * (0.55 + 0.4 * h(1));
    for k in 0..n {
        let g = |q: u32| h(10 + k * 7 + q);
        let along = (g(1) - 0.5) * w * 1.3;
        let r = w * (0.25 + 0.3 * g(2));
        let top = height * (0.65 + 0.35 * g(3));
        let mut m = MeshInstance::new(
            MeshId::LOW_SPHERE,
            [at[0] + side[0] * along, top - r, at[2] + side[1] * along],
        );
        m.scale = [
            r * 2.0 * (0.7 + 0.6 * g(6)),
            r * 2.0 * (0.6 + 0.5 * g(4)),
            r * 2.0 * (0.7 + 0.6 * g(7)),
        ];
        m.material = material;
        let v = 0.85 + 0.3 * g(5);
        m.color = [colour[0] * v, colour[1] * v, colour[2] * v, 1.0];
        rw.draw_mesh(m);
    }
    let mut m = MeshInstance::new(MeshId::CUBE, [at[0], height * 0.3, at[2]]);
    m.scale = [w * 0.9, height * 0.6, w * 0.6];
    m.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], -side[1].atan2(side[0]));
    m.material = material;
    m.color = [colour[0], colour[1], colour[2], 1.0];
    rw.draw_mesh(m);
}

/// The far tree line's layers: (distance m, crown height m, colour lift,
/// gap share), the farther lighter in the haze.
const LAYERS: [(f32, f32, f32, f32); 3] = [
    (440.0, 1.7, 3.2, 0.15),
    (410.0, 2.1, 2.0, 0.35),
    (375.0, 2.5, 1.0, 0.6),
];
/// The wood at the far left: (bearing, distance, half length, height).
const LEFT_WOOD: (f32, f32, f32, f32) = (29.5, 300.0, 17.0, 6.2);
/// Broad field patches (each set), field boundaries and flower groups.
const PATCHES: u32 = 360;
const HEDGES: u32 = 14;
const FLOWER_GROUPS: u32 = 26;
/// The meadow's grass patch: half its side (m; the fly area is 80 m round
/// the middle, its edge thins over the last 30 m) and its height grid
/// (samples a side, the hummocks' rise and fall, how far it is sunk).
#[cfg(feature = "sky")]
const GRASS_HALF: f32 = 110.0;
#[cfg(feature = "sky")]
const RELIEF_N: usize = 129;
#[cfg(feature = "sky")]
const RELIEF: f32 = 0.8;
#[cfg(feature = "sky")]
const SINK: f32 = 0.18;

/// The meadow's grass (evani-grass's long meadow kind, round 8's values).
#[cfg(feature = "sky")]
const MEADOW: [(&str, ParamValue); 21] = [
    ("material.grass.ground", ParamValue::Bool(false)),
    ("material.grass.quality", ParamValue::F32(3.0)),
    ("material.grass.distance", ParamValue::F32(2.0)),
    ("material.grass.translucency", ParamValue::F32(0.05)),
    ("material.grass.specular", ParamValue::F32(0.0)),
    ("material.grass.meadow.height", ParamValue::F32(0.5)),
    ("material.grass.meadow.height_var", ParamValue::F32(0.8)),
    ("material.grass.meadow.clump", ParamValue::F32(1.0)),
    ("material.grass.meadow.heads", ParamValue::F32(0.0)),
    ("material.grass.meadow.width", ParamValue::F32(0.008)),
    ("material.grass.meadow.density", ParamValue::F32(1200.0)),
    ("material.grass.meadow.dryness", ParamValue::F32(0.3)),
    ("material.grass.meadow.bare", ParamValue::F32(0.2)),
    ("material.grass.meadow.droop", ParamValue::F32(0.4)),
    ("material.grass.meadow.variation", ParamValue::F32(0.1)),
    ("material.grass.round", ParamValue::F32(0.0)),
    ("material.grass.ao", ParamValue::F32(0.2)),
    (
        "material.grass.meadow.tip",
        ParamValue::Color([0.1, 0.115, 0.11]),
    ),
    (
        "material.grass.meadow.root",
        ParamValue::Color([0.024, 0.028, 0.032]),
    ),
    (
        "material.grass.meadow.dry",
        ParamValue::Color([0.23, 0.19, 0.13]),
    ),
    ("material.grass.wetness", ParamValue::F32(0.0)),
];

/// Smooth value noise in -1..1 over a unit lattice (fixed, seeded).
#[cfg(feature = "sky")]
fn vnoise(x: f32, z: f32, seed: u32) -> f32 {
    let (xf, zf) = (x.floor(), z.floor());
    let (tx, tz) = (x - xf, z - zf);
    let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
    let c = |dx: i32, dz: i32| {
        let xi = (xf as i32).wrapping_add(dx) as u32;
        let zi = (zf as i32).wrapping_add(dz) as u32;
        hash01(
            xi.wrapping_mul(73_856_093)
                ^ zi.wrapping_mul(19_349_663)
                ^ seed.wrapping_mul(83_492_791),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_hour_is_the_columns_light() {
        let (el, _) = crate::sun_at(crate::START_HOUR);
        assert!(
            smooth(-8.0, 1.0, el) > 0.999 && smooth(2.0, 20.0, el) < 1e-3,
            "{el}"
        );
        assert!((smooth(-2.0, 1.9, el) - 1.0).abs() < 0.02);
    }
}
