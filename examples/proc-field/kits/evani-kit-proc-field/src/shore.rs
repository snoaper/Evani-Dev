//! The shoreline's live scene (feature `shore`, the Asset Studio shorelines
//! session, 2026-11-05): the procedural library's shoreline
//! (`water-shorelines`) in the engine's realism studio under its reference
//! picture's camera and light (best.png: a cut block of coast, a sandy cove
//! between grassy dunes and a granite headland, clear turquoise water, seen
//! three-quarter from above on a neutral grey sweep under a soft key).
//!
//! The library's water goes to the nature pass the way a game hands it
//! over: `shore::water` gives the water rectangle for the placement and
//! `shore::apply_water_look` its look.
//!
//! `kit.proc_field.shore_view`: 0 the reference's view (its camera solved
//! with best.png's top margin: 33 degrees down, 24 degrees round from the
//! front face, a 19 degree lens), 1 a close-up of the beach's front
//! corner (granite, pebbles, grass), 2 low along the waterline (the foam,
//! the clear shallows over the sand).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::{ProcScene, Registry};
use evani_procedural_lib::evani_material_nature::NatureSurfaces;
use evani_procedural_lib::shore;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Shore {
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

/// The floor's colour by default (drawn at twice it): best.png's warm
/// neutral studio grey.
const FLOOR: [f32; 3] = [0.262, 0.255, 0.28];

/// The reference view: heading, elevation (degrees), distance (m), aim and
/// lens. Round 1 solved them from best.png's visible corners (10 px rms),
/// which put the block's far corner at the frame's top and cut off the
/// headland; round 2 frames the whole block with best.png's top margin
/// (about 80 px over the headland), its corners within about 75 px rms.
const YAW: f32 = 23.5;
const EL: f32 = 33.0;
const DIST: f32 = 34.0;
const AIM: [f32; 3] = [-0.01, 1.15, 1.6];
const FOV: f32 = 19.1;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.shore_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the shoreline's reference view (best.png's camera and light), 1 a close-up of the beach's front corner, 2 low along the waterline"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.shore_yaw", YAW).range(-180.0, 180.0).describe("The reference view's heading round the block (degrees from +z towards +x)"))?;
    let el = kit.param_f32(ParamDef::f32("kit.proc_field.shore_el", EL).range(0.0, 89.0).describe("The reference view's elevation (degrees above the floor)"))?;
    let dist = kit.param_f32(ParamDef::f32("kit.proc_field.shore_dist", DIST).range(2.0, 200.0).unit("m").describe("The reference view's distance from its aim (m)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.shore_sun_az", -55.0).range(-180.0, 180.0).describe("The key light's bearing from the camera's heading (degrees; negative: from the camera's left)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.shore_sun_el", 52.0).range(5.0, 89.0).describe("The key light's elevation (degrees)"))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.shore_floor", FLOOR).describe("The studio floor's colour under the shoreline"))?;
    kit.resource(Shore { view, yaw, el, dist, sun_az, sun_el, floor, set_for: f32::NAN });
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
    let mut g = *world.res::<Shore>();
    let (view, yaw, el, dist, sun_az, sun_el) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.el), reg.f32(g.dist), reg.f32(g.sun_az), reg.f32(g.sun_el))
    };
    let cam = match view as u32 {
        1 => orbit(yaw - 20.0, 30.0, 9.5, [-3.6, 1.0, 4.4], 28.0),
        2 => orbit(yaw + 45.0, 13.0, 9.0, [-1.8, 0.85, 1.6], 32.0),
        _ => orbit(yaw, el, dist, AIM, FOV),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the camera's upper left (best.png's rocks are lit
        // on their left and top faces, the water's cut face is in shade),
        // a broad neutral fill; soft, light shadows (a wide light, a wide
        // blocker search, light wrapped round the forms), no rim.
        let (e, az) = (sun_el.to_radians(), (yaw + sun_az).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.98, 0.95]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.9));
        set(&mut reg, "render.ambient", ParamValue::Color([0.74, 0.745, 0.77]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.7, 0.7, 0.7]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.3));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.15));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.63, 0.61, 0.66]));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(7.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.6));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist + 12.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist + 50.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(10.0));
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(0.05));
        set(&mut reg, "kit.proc_field.facet_floor.contact", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.far", ParamValue::F32(0.0));
        // The library's water look for its shoreline.
        let _ = shore::apply_water_look(&mut reg);
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = view;
        *world.res_mut::<Shore>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("water-shorelines", [0.0, 0.0, 0.0]);
    }
    // The water: the nature pass draws the library's rectangle for the
    // placement (kept in step with its live size and depth).
    let water = match (world.try_res::<Registry>(), world.try_res::<ParamRegistry>()) {
        (Some(r), Some(reg)) => shore::water(&r, &reg, [0.0, 0.0, 0.0], 0.0, 1.0),
        _ => None,
    };
    if let (Some(w), Some(ns)) = (water, world.try_res::<NatureSurfaces>()) {
        ns.set(&[w]);
    }
    let rgb = world.res::<ParamRegistry>().vec3(g.floor);
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [600.0, 1.0, 600.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
}
