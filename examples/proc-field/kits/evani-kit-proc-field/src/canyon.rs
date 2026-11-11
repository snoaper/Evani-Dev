//! The canyons' live scene (feature `canyon`, the Asset Studio canyons
//! session, column 12, 2026-11-06): the procedural library's canyons
//! (`terrain-canyons`) in the engine's realism studio under their reference
//! picture's camera and light (a cut block of red-rock canyon country seen
//! from its front corner, three-quarter from above, on a neutral grey sweep
//! under a soft key from the upper left).
//!
//! `kit.proc_field.canyon_view`: 0 the reference's view (the block's front
//! corner towards the camera, 30 degrees down, a 21 degree lens), 1 a
//! close-up of the main canyon's walls (the cliff bands, benches and
//! talus), 2 low along the canyon floor (the wash, the fallen blocks).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Canyon {
    view: F32Param,
    yaw: F32Param,
    el: F32Param,
    dist: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    floor: Vec3Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The floor's colour by default (drawn at twice it): the reference's
/// neutral studio grey (bluer than grey: the look's grade warms it).
const FLOOR: [f32; 3] = [0.3, 0.323, 0.365];

/// The reference view: heading (the block's +x +z corner is its front),
/// elevation (degrees), distance (m), aim and lens.
const YAW: f32 = 45.0;
const EL: f32 = 34.0;
const DIST: f32 = 65.8;
const AIM: [f32; 3] = [0.55, -1.6, -0.55];
const FOV: f32 = 21.0;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.canyon_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the canyons' reference view (the reference's camera and light), 1 a close-up of the main canyon's walls, 2 low along the canyon floor"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.canyon_yaw", YAW).range(-180.0, 180.0).describe("The reference view's heading round the block (degrees from +z towards +x)"))?;
    let el = kit.param_f32(ParamDef::f32("kit.proc_field.canyon_el", EL).range(0.0, 89.0).describe("The reference view's elevation (degrees above the floor)"))?;
    let dist = kit.param_f32(ParamDef::f32("kit.proc_field.canyon_dist", DIST).range(2.0, 400.0).unit("m").describe("The reference view's distance from its aim (m)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.canyon_sun_az", -60.0).range(-180.0, 180.0).describe("The key light's bearing from the camera's heading (degrees; negative: from the camera's left)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.canyon_sun_el", 50.0).range(5.0, 89.0).describe("The key light's elevation (degrees)"))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.canyon_floor", FLOOR).describe("The studio floor's colour under the canyons"))?;
    kit.resource(Canyon { view, yaw, el, dist, sun_az, sun_el, floor, set_for: f32::NAN });
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
    let mut g = *world.res::<Canyon>();
    let (view, yaw, el, dist, sun_az, sun_el) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.el), reg.f32(g.dist), reg.f32(g.sun_az), reg.f32(g.sun_el))
    };
    let cam = match view as u32 {
        // The main canyon's walls where it bends right of the middle.
        1 => orbit(yaw - 25.0, 26.0, 20.0, [3.6, 2.2, -0.4], 30.0),
        // Low along the floor towards the front corner's mouth.
        2 => orbit(yaw + 20.0, 9.0, 12.0, [7.2, 1.0, 5.4], 34.0),
        _ => orbit(yaw, el, dist, AIM, FOV),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the camera's upper left (the reference's west
        // walls are lit, the block's shadow falls to the right), a broad
        // neutral fill; soft, light shadows, no rim.
        let (e, az) = (sun_el.to_radians(), (yaw + sun_az).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.97, 0.93]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.7));
        set(&mut reg, "render.ambient", ParamValue::Color([0.4, 0.385, 0.37]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.9));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.7, 0.7, 0.7]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.25));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.15));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.66, 0.712, 0.81]));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(5.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.45));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist + 20.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist + 90.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view == 0.0 { dist + 30.0 } else { 40.0 }));
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
        *world.res_mut::<Canyon>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("terrain-canyons", [0.0, 0.0, 0.0]);
    }
    let rgb = world.res::<ParamRegistry>().vec3(g.floor);
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [800.0, 1.0, 800.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
}
