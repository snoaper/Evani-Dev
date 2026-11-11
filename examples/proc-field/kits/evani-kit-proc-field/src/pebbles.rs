//! The pebbles' live scene (feature `pebbles`, the Asset Studio pebbles
//! session, 2026-11-05): the procedural library's pebbles (`rocks-pebbles`)
//! in the engine's realism studio under their reference picture's camera
//! and light (best.png: a studio group of a dozen mixed stones, a
//! three-quarter view from a little above, a soft key light from the upper
//! left, soft shadows going right, a neutral grey sweep behind), so they can
//! be tuned and shot beside the reference.
//!
//! `kit.proc_field.pebbles_view`: 0 the reference's view, 1 a close-up of
//! the big cobble and its neighbours (the dents, chips and crack), 2 low
//! along the ground (where the stones meet the floor).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Pebbles {
    view: F32Param,
    yaw: F32Param,
    aim: F32Param,
    el: F32Param,
    floor: Vec3Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The floor's colour by default (drawn at twice it): a neutral studio grey.
const FLOOR: [f32; 3] = [0.235, 0.242, 0.26];

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.pebbles_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the pebbles' reference view (best.png's camera and light), 1 a close-up of the big cobble, 2 low along the ground"),
    )?;
    let yaw = kit.param_f32(
        ParamDef::f32("kit.proc_field.pebbles_yaw", YAW)
            .range(-180.0, 180.0)
            .describe("The camera's heading round the group (degrees from +z towards +x)"),
    )?;
    let aim = kit.param_f32(
        ParamDef::f32("kit.proc_field.pebbles_aim", AIM)
            .range(0.0, 0.3)
            .unit("m")
            .describe("The reference view's aim: the height the camera looks at (m)"),
    )?;
    let el = kit.param_f32(
        ParamDef::f32("kit.proc_field.pebbles_el", EL)
            .range(0.0, 60.0)
            .describe("The reference view's elevation (degrees above the floor)"),
    )?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.pebbles_floor", FLOOR).describe("The studio floor's colour under the pebbles"))?;
    kit.resource(Pebbles { view, yaw, aim, el, floor, set_for: f32::NAN });
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

/// The default heading: the library lays its first ten stones out as
/// best.png's group seen from +z (the big cobble at the back left).
const YAW: f32 = 0.0;

/// The reference view's aim height (m) and elevation (degrees): the group
/// sits where best.png's does (column 7 round 1: 1.6 cm lower aim than the
/// first try, whose stones sat 30-80 px low).
const AIM: f32 = 0.104;
const EL: f32 = 14.0;

/// A camera `dist` off at `el` degrees up, heading `yaw`, on `target`.
fn orbit(yaw: f32, el: f32, dist: f32, target: [f32; 3], fov: f32) -> CameraPose {
    let (y, e) = (yaw.to_radians(), el.to_radians());
    CameraPose {
        eye: [target[0] + dist * e.cos() * y.sin(), target[1] + dist * e.sin(), target[2] + dist * e.cos() * y.cos()],
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Pebbles>();
    let (view, yaw, aim, el) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.aim), reg.f32(g.el))
    };
    // best.png: about 14 degrees up through a long lens (20 degrees tall),
    // the group's foot a fifth of the frame below its centre.
    let cam = match view as u32 {
        1 => orbit(yaw - 15.0, 22.0, 0.8, [-0.06, 0.08, -0.04], 20.0),
        2 => orbit(yaw + 10.0, 5.0, 1.0, [0.0, 0.05, 0.0], 22.0),
        _ => orbit(yaw, el, 1.39, [0.0, aim, 0.0], 20.0),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the camera's upper left (best.png's shadows fall
        // right and a little back), a broad neutral fill.
        let (el, az) = (44f32.to_radians(), (yaw - 62.0).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 1.0, 1.0]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.1));
        set(&mut reg, "render.ambient", ParamValue::Color([0.92, 0.93, 0.96]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.7, 0.7, 0.7]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.3));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.15));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.52, 0.515, 0.515]));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(2.4));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(6.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(3.0));
        // A broad soft studio light (column 7 round 2: hard, near-black
        // shadows under the big cobble and beside the slate): an 8 degree
        // sun at little more than half its strength, a wide blocker search,
        // a wrapped terminator and a strong even ambient.
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(8.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.75));
        // No stylised rim: it lit a pale strip along the big cobble's top.
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(0.05));
        set(&mut reg, "kit.proc_field.facet_floor.contact", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.far", ParamValue::F32(0.0));
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = view;
        *world.res_mut::<Pebbles>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("rocks-pebbles", [0.0, 0.0, 0.0]);
    }
    let rgb = world.res::<ParamRegistry>().vec3(g.floor);
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
}
