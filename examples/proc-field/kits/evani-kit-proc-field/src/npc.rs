//! The NPC body variations' live scene (feature `npc`, the Asset Studio
//! full NPC body variations session, column 22, 2026-11-07): the procedural
//! library's line-up of people (`humans-full-npc-body-variations`) in the
//! engine's realism studio under its reference picture's camera and light
//! (six adults of different builds in grey tank tops and shorts, barefoot,
//! standing in a row turned three-quarters to the right, on a seamless grey
//! sweep under a soft key from the front left).
//!
//! `kit.proc_field.npc_view`: 0 the reference's view (the whole row, a long
//! lens at chest height), 1 a close-up of the two middle people's heads and
//! shoulders, 2 the row's feet low along the floor.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Npc {
    view: F32Param,
    yaw: F32Param,
    el: F32Param,
    dist: F32Param,
    fov: F32Param,
    aim_y: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    floor: Vec3Param,
    wall_gain: F32Param,
    wall_z: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The floor's colour by default (drawn at twice it): the reference's
/// neutral studio grey, lighter near the camera than the backdrop.
const FLOOR: [f32; 3] = [0.315, 0.328, 0.363];

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the line-up's reference view (the reference's camera and light), 1 the two middle people's heads and shoulders, 2 the feet low along the floor"),
    )?;
    let yaw = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_yaw", 0.0)
            .range(-180.0, 180.0)
            .describe("The reference view's heading round the row (degrees from +z towards +x)"),
    )?;
    let el = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_el", 1.5)
            .range(-10.0, 89.0)
            .describe("The reference view's elevation (degrees above level)"),
    )?;
    let dist = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_dist", 14.0)
            .range(2.0, 60.0)
            .unit("m")
            .describe("The reference view's distance from its aim (m)"),
    )?;
    let fov = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_fov", 9.05)
            .range(2.0, 60.0)
            .describe("The reference view's vertical field of view (degrees)"),
    )?;
    let aim_y = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_aim_y", 0.93)
            .range(0.0, 2.0)
            .unit("m")
            .describe("The reference view's aim height (m)"),
    )?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.npc_sun_az", -38.0).range(-180.0, 180.0).describe("The key light's bearing from the camera's heading (degrees; negative: from the camera's left)"))?;
    let sun_el = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_sun_el", 66.0)
            .range(5.0, 89.0)
            .describe("The key light's elevation (degrees)"),
    )?;
    let floor = kit.param_vec3(
        ParamDef::color("kit.proc_field.npc_floor", FLOOR)
            .describe("The studio floor's colour under the line-up"),
    )?;
    let wall_gain = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_wall_gain", 1.55)
            .range(0.2, 4.0)
            .describe("The studio wall's brightness against the floor's colour (it faces the camera, so the key light reaches it at a slant)"),
    )?;
    let wall_z = kit.param_f32(
        ParamDef::f32("kit.proc_field.npc_wall_z", 1.4)
            .range(0.5, 10.0)
            .unit("m")
            .describe("How far behind the row the studio wall stands (m)"),
    )?;
    kit.resource(Npc {
        view,
        yaw,
        el,
        dist,
        fov,
        aim_y,
        sun_az,
        sun_el,
        floor,
        wall_gain,
        wall_z,
        set_for: f32::NAN,
    });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && let Some(v) = reg
            .value(id)
            .and_then(|like| evani_procedural::studio::coerce(like, v))
    {
        let _ = reg.set(id, v);
    }
}

/// A piecewise-linear profile through `(x, value)` points (sorted by x).
fn profile(pts: &[(f32, f32)], x: f32) -> f32 {
    let mut prev = pts[0];
    if x <= prev.0 {
        return prev.1;
    }
    for &(px, pv) in &pts[1..] {
        if x <= px {
            let t = (x - prev.0) / (px - prev.0).max(1e-6);
            return prev.1 + (pv - prev.1) * t;
        }
        prev = (px, pv);
    }
    prev.1
}

/// The studio wall's shade up its height (m; 1 its brightest), measured
/// on the reference row by row: darker towards the top of the frame, a
/// soft dark cove where it meets the floor.
const WALL: [(f32, f32); 12] = [
    (0.0, 0.7),
    (0.07, 0.72),
    (0.12, 0.85),
    (0.2, 1.0),
    (0.8, 1.0),
    (1.1, 0.93),
    (1.43, 0.9),
    (1.6, 0.9),
    (1.76, 0.8),
    (1.92, 0.72),
    (2.09, 0.66),
    (3.0, 0.55),
];

/// The floor's shade from the wall's foot (its z behind the row, m) to the
/// camera: dark at the cove, lighter behind the feet, brightest in front.
const FLOOR_SHADE: [(f32, f32); 7] = [
    (-1.4, 0.57),
    (-0.9, 0.6),
    (-0.55, 0.7),
    (-0.3, 0.84),
    (0.0, 0.9),
    (1.0, 0.96),
    (2.0, 1.0),
];

/// A camera `dist` off at `el` degrees up, heading `yaw`, on `target`.
fn orbit(yaw: f32, el: f32, dist: f32, target: [f32; 3], fov: f32) -> CameraPose {
    let (y, e) = (yaw.to_radians(), el.to_radians());
    CameraPose {
        eye: [
            target[0] + dist * e.cos() * y.sin(),
            target[1] + dist * e.sin(),
            target[2] + dist * e.cos() * y.cos(),
        ],
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Npc>();
    let (view, yaw, el, dist, fov, aim_y, sun_az, sun_el) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round().clamp(0.0, 2.0),
            reg.f32(g.yaw),
            reg.f32(g.el),
            reg.f32(g.dist),
            reg.f32(g.fov),
            reg.f32(g.aim_y),
            reg.f32(g.sun_az),
            reg.f32(g.sun_el),
        )
    };
    let cam = match view as u32 {
        // The two middle people's heads and shoulders.
        1 => orbit(yaw - 6.0, 2.0, 6.0, [0.0, 1.5, 0.0], 9.0),
        // Low along the floor at the row's feet.
        2 => orbit(yaw + 10.0, 4.0, 7.0, [0.0, 0.18, 0.0], 12.0),
        _ => orbit(yaw, el, dist, [0.0, aim_y, 0.0], fov),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the front left and above (the reference's faces
        // and the fronts of the bodies are lit, the right sides turn a
        // little darker, faint soft shadows fall behind and to the right),
        // a broad neutral fill.
        let (e, az) = (sun_el.to_radians(), (yaw + sun_az).to_radians());
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]),
        );
        set(
            &mut reg,
            "render.light.color",
            ParamValue::Color([1.0, 0.98, 0.96]),
        );
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.15));
        // A gentle fill: hands, feet and faces turned from the key stay
        // readable (less occlusion darkening them).
        set(
            &mut reg,
            "render.ambient",
            ParamValue::Color([0.63, 0.63, 0.64]),
        );
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.6));
        set(
            &mut reg,
            "render.sky.horizon",
            ParamValue::Color([0.8, 0.8, 0.8]),
        );
        set(
            &mut reg,
            "render.sky.ground",
            ParamValue::Color([0.7, 0.7, 0.7]),
        );
        set(
            &mut reg,
            "render.shadow_tint_strength",
            ParamValue::F32(0.0),
        );
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.3));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.3));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.8));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(
            &mut reg,
            "render.clear_color",
            ParamValue::Color([0.66, 0.66, 0.665]),
        );
        // The softest the look allows (its ranges' tops: 8 degrees, 24
        // texels; round 3 asked 34 and 32, which the ranges cut to these).
        set(
            &mut reg,
            "look.stylizedpbr.pcss_light_deg",
            ParamValue::F32(8.0),
        );
        set(
            &mut reg,
            "look.stylizedpbr.pcss_search",
            ParamValue::F32(24.0),
        );
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.45));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(
            &mut reg,
            "look.stylizedpbr.haze_near",
            ParamValue::F32(dist + 4.0),
        );
        set(
            &mut reg,
            "look.stylizedpbr.haze_far",
            ParamValue::F32(dist + 30.0),
        );
        // The shadow map's half width round the aim: just the row and its
        // shadows (wider blurs the bust's and hems' shadow edges to specks).
        // Round 4: a little wider, so the same 24-texel search spans a
        // broader penumbra (the row's cast shadows were hard-edged wedges).
        // Round 5: wider again (a coarser map blurs the edges further) and
        // lighter: the reference's cast shadows are faint, wide and soft,
        // with the dark kept at the feet.
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(7.0));
        set(&mut reg, "fx.shadows.strength", ParamValue::F32(0.44));
        // The wall now carries the studio's gradient; only a faint fall-off
        // at the frame's corners.
        set(&mut reg, "fx.vignette.intensity", ParamValue::F32(0.06));
        set(&mut reg, "fx.vignette.radius", ParamValue::F32(0.62));
        set(&mut reg, "fx.vignette.softness", ParamValue::F32(0.9));
        set(&mut reg, "fx.vignette.roundness", ParamValue::F32(0.5));
        set(
            &mut reg,
            "kit.proc_field.facet_floor.on",
            ParamValue::Bool(true),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.broad",
            ParamValue::F32(0.0),
        );
        // A restrained sweep: a faint grain (its broad noise streaks into
        // bands at this grazing angle, so it stays off).
        set(
            &mut reg,
            "kit.proc_field.facet_floor.fine",
            ParamValue::F32(0.02),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.contact",
            ParamValue::F32(0.0),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.far",
            ParamValue::F32(0.0),
        );
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = view;
        *world.res_mut::<Npc>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("humans-full-npc-body-variations", [0.0, 0.0, 0.0]);
    }
    let (rgb, gain, wz) = {
        let reg = world.res::<ParamRegistry>();
        (reg.vec3(g.floor), reg.f32(g.wall_gain), reg.f32(g.wall_z))
    };
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.006, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
    // Round 5: the studio sweep as the reference shows it: a wall `wz`
    // behind the row (thin strips, each its own shade of the measured
    // profile, so the gradient has no visible steps) and the floor
    // shaded from the wall's dark cove to the bright front.
    let up = quat_axis_angle([1.0, 0.0, 0.0], std::f32::consts::FRAC_PI_2);
    let strip = 0.025f32;
    for k in 0..120 {
        let y = (k as f32 + 0.5) * strip;
        let sh = profile(&WALL, y) * gain * 2.0;
        let mut w = MeshInstance::new(MeshId::PLANE, [0.0, y, -wz]);
        w.rotation = up;
        w.scale = [9.0, 1.0, strip * 1.04];
        w.material = floor;
        w.color = [rgb[0] * sh, rgb[1] * sh, rgb[2] * sh, 1.0];
        rw.draw_mesh(w);
    }
    let fs = 0.04f32;
    let n = ((wz + 2.6) / fs).ceil() as i32;
    for k in 0..n {
        let z = -wz + (k as f32 + 0.5) * fs;
        let sh = profile(&FLOOR_SHADE, z) * 2.0;
        let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.0005, z]);
        f.scale = [9.0, 1.0, fs * 1.04];
        f.material = floor;
        f.color = [rgb[0] * sh, rgb[1] * sh, rgb[2] * sh, 1.0];
        rw.draw_mesh(f);
    }
}
