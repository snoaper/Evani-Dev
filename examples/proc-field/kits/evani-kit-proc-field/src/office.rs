//! The office building's live scene (feature `office`, the Asset Studio
//! office buildings session, column 13, 2026-11-06): the procedural
//! library's office building (`buildings-office-buildings`) in the engine's
//! realism studio under its reference picture's camera and light (a
//! stone-clad mid-rise office block, a glass tower between two lower wings,
//! seen from its front-left corner at about a third of its height, on a
//! neutral grey sweep under a soft, high key).
//!
//! `kit.proc_field.office_view`: 0 the reference's view (the left wing's
//! corner towards the camera, a long lens), 1 a close-up of the entrance
//! (the canopy, the lobby and the left wing's stone grid), 2 low along the
//! left wing's planters to the tower's corner.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Office {
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
/// neutral studio grey.
const FLOOR: [f32; 3] = [0.3, 0.31, 0.33];

/// The reference view: heading (from +z towards +x; the building's front
/// faces +z, its left side -x), elevation (degrees), distance (m), aim and
/// lens.
const YAW: f32 = -42.0;
const EL: f32 = 4.0;
const DIST: f32 = 98.0;
const AIM: [f32; 3] = [-2.0, 15.0, 2.0];
const FOV: f32 = 25.0;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.office_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the office building's reference view (the reference's camera and light), 1 a close-up of the entrance and the stone grid, 2 low along the planters"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.office_yaw", YAW).range(-180.0, 180.0).describe("The reference view's heading round the building (degrees from +z towards +x)"))?;
    let el = kit.param_f32(ParamDef::f32("kit.proc_field.office_el", EL).range(-10.0, 89.0).describe("The reference view's elevation (degrees above level)"))?;
    let dist = kit.param_f32(ParamDef::f32("kit.proc_field.office_dist", DIST).range(5.0, 600.0).unit("m").describe("The reference view's distance from its aim (m)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.office_sun_az", 20.0).range(-180.0, 180.0).describe("The key light's bearing from the camera's heading (degrees; negative: from the camera's left)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.office_sun_el", 50.0).range(5.0, 89.0).describe("The key light's elevation (degrees)"))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.office_floor", FLOOR).describe("The studio floor's colour under the office building"))?;
    kit.resource(Office { view, yaw, el, dist, sun_az, sun_el, floor, set_for: f32::NAN });
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
    let mut g = *world.res::<Office>();
    let (view, yaw, el, dist, sun_az, sun_el) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.el), reg.f32(g.dist), reg.f32(g.sun_az), reg.f32(g.sun_el))
    };
    let cam = match view as u32 {
        // The entrance: the canopy, the lobby and the left wing's grid.
        1 => orbit(yaw + 8.0, 6.0, 34.0, [-3.0, 5.5, 11.0], 32.0),
        // Low along the left wing's front planters to the tower's corner.
        2 => orbit(yaw - 30.0, 3.0, 16.0, [-12.0, 2.0, 10.5], 40.0),
        _ => orbit(yaw, el, dist, AIM, FOV),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft, high key from just right of the camera (the reference's
        // front faces are a little brighter than its left sides, its soft
        // shadow falls behind and to the right), a broad neutral fill.
        let (e, az) = (sun_el.to_radians(), (yaw + sun_az).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.98, 0.95]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.4));
        set(&mut reg, "render.ambient", ParamValue::Color([0.42, 0.42, 0.42]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.8));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.7, 0.7, 0.7]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.3));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.15));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.7));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.7, 0.7, 0.72]));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(6.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.4));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist + 40.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist + 160.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view == 0.0 { dist + 60.0 } else { 90.0 }));
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
        *world.res_mut::<Office>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("buildings-office-buildings", [0.0, 0.0, 0.0]);
    }
    let rgb = world.res::<ParamRegistry>().vec3(g.floor);
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [1200.0, 1.0, 1200.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
}
