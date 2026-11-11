//! The jagged rock formation's live scene (feature `jrocks`, the Asset
//! Studio jagged rock formations session, column 24, 2026-11-07): the
//! procedural library's jagged rock formation
//! (`rocks-jagged-rock-formations`) in the engine's realism studio under its
//! reference picture's camera and light (a studio still of a 3.5 m outcrop
//! of leaning schist blades, a three-quarter view from a little above, a
//! soft key light from the upper left, soft shadows under it to the right,
//! a neutral grey sweep behind), so it can be tuned and shot beside the
//! reference. Adapted from the medium rocks' scene (column 23's
//! src/mrocks.rs), itself from the pebbles' (src/pebbles.rs), whose floor
//! pass it shares.
//!
//! `kit.proc_field.jrocks_view`: 0 the reference's view, 1 a close-up of the
//! main spire's broad face and top (the stepped sheets, the chips), 2 low
//! along the ground at the front (the rubble and the blades' feet).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct JaggedRocks {
    view: F32Param,
    yaw: F32Param,
    aim: F32Param,
    el: F32Param,
    dist: F32Param,
    fov: F32Param,
    sun: F32Param,
    floor: Vec3Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The floor's colour by default (drawn at twice it): a neutral studio grey.
const FLOOR: [f32; 3] = [0.272, 0.268, 0.284];

/// The reference view: heading (degrees from +z towards +x), aim height
/// (m), elevation (degrees), distance (m), vertical field of view
/// (degrees) and the key light's heading off the camera's (degrees).
const YAW: f32 = 0.0;
const AIM: f32 = 1.5;
const EL: f32 = 14.0;
const DIST: f32 = 10.9;
const FOV: f32 = 24.0;
const SUN: f32 = -58.0;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.jrocks_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the jagged rock formation's reference view, 1 a close-up of the main spire, 2 low along the ground at the front"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.jrocks_yaw", YAW).range(-180.0, 180.0).describe("The camera's heading round the formation (degrees from +z towards +x)"))?;
    let aim = kit.param_f32(ParamDef::f32("kit.proc_field.jrocks_aim", AIM).range(0.0, 4.0).unit("m").describe("The reference view's aim: the height the camera looks at (m)"))?;
    let el = kit.param_f32(ParamDef::f32("kit.proc_field.jrocks_el", EL).range(0.0, 80.0).describe("The reference view's elevation (degrees above the floor)"))?;
    let dist = kit.param_f32(ParamDef::f32("kit.proc_field.jrocks_dist", DIST).range(2.0, 40.0).unit("m").describe("The reference view's distance from its aim (m)"))?;
    let fov = kit.param_f32(ParamDef::f32("kit.proc_field.jrocks_fov", FOV).range(5.0, 60.0).describe("The reference view's vertical field of view (degrees)"))?;
    let sun = kit.param_f32(ParamDef::f32("kit.proc_field.jrocks_sun", SUN).range(-180.0, 180.0).describe("The key light's heading off the camera's (degrees; negative from the left)"))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.jrocks_floor", FLOOR).describe("The studio floor's colour under the formation"))?;
    kit.resource(JaggedRocks { view, yaw, aim, el, dist, fov, sun, floor, set_for: f32::NAN });
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
    let mut g = *world.res::<JaggedRocks>();
    let (view, yaw, aim, el, dist, fov, sun) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.aim), reg.f32(g.el), reg.f32(g.dist), reg.f32(g.fov), reg.f32(g.sun))
    };
    let cam = match view as u32 {
        1 => orbit(yaw - 15.0, 18.0, 4.6, [0.9, 2.4, -0.3], 24.0),
        2 => orbit(yaw + 6.0, 5.0, 7.5, [-0.4, 0.35, 0.8], 22.0),
        _ => orbit(yaw, el, dist, [0.15, aim, 0.0], fov),
    };
    // Set up each time the view, or the light's heading, changes.
    let key = view * 1000.0 + sun;
    if key != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the camera's upper left (the reference's lit faces
        // look left, its shadows fall under the formation to the right), a
        // broad neutral fill.
        let (el, az) = (52f32.to_radians(), (yaw + sun).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.99, 0.97]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.0));
        set(&mut reg, "render.ambient", ParamValue::Color([0.52, 0.53, 0.55]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(1.0));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.7, 0.7, 0.7]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.25));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.1));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.445, 0.445, 0.445]));
        // The reference's backdrop darkens towards the frame's top corners;
        // its shadows are soft and light.
        set(&mut reg, "fx.vignette.intensity", ParamValue::F32(0.22));
        set(&mut reg, "fx.vignette.radius", ParamValue::F32(0.55));
        set(&mut reg, "fx.vignette.softness", ParamValue::F32(0.9));
        set(&mut reg, "fx.vignette.roundness", ParamValue::F32(0.6));
        set(&mut reg, "fx.shadows.strength", ParamValue::F32(0.6));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(14.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(22.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(24.0));
        // A broad soft studio light: a wide sun, a wide blocker search, a
        // wrapped terminator and a strong even ambient.
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(8.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.4));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(0.04));
        set(&mut reg, "kit.proc_field.facet_floor.contact", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.far", ParamValue::F32(0.0));
        drop(reg);
        if g.set_for.is_nan()
            && let Some(mut s) = world.try_res_mut::<ProcScene>()
        {
            s.clear();
        }
        g.set_for = key;
        *world.res_mut::<JaggedRocks>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("rocks-jagged-rock-formations", [0.0, 0.0, 0.0]);
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
