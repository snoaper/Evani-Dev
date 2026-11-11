//! The dark loam's live scene (feature `loam`, the Asset Studio dark loam
//! session, column 69, 2026-11-10): the procedural library's heap of dark
//! loam (`evani_procedural_lib::soil::heap`, the catalogue's
//! `ground.soil.loam` as real geometry) in the engine's realism studio under
//! its reference picture's camera and light (a studio still: a conical heap
//! of crumbly dark brown soil with clods on its lower slopes and loose
//! crumbs round its foot, a three-quarter view from a little above the
//! floor through a long lens, a soft key light from the upper left, a light
//! neutral grey sweep), so it can be tuned and shot beside the reference.
//! Adapted from the pine bark's scene (src/barkpine.rs, column 40), whose
//! floor pass it shares; the heap is drawn as one model the scene builds
//! itself (the material has no library asset to place).
//!
//! `kit.proc_field.loam_view`: 0 the reference's view, 1 a close-up of the
//! clods on the lower left slope, 2 low along the floor at the heap's foot.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_procedural_lib::soil;
use evani_render::model::{ModelId, ModelInstance, Models};
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Loam {
    view: F32Param,
    yaw: F32Param,
    turn: F32Param,
    aim: F32Param,
    el: F32Param,
    dist: F32Param,
    fov: F32Param,
    sun: F32Param,
    sun_el: F32Param,
    soft: F32Param,
    floor: Vec3Param,
    contact: F32Param,
    light: F32Param,
    fill: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
    /// The heap's model once built.
    model: Option<ModelId>,
}

/// The floor's colour by default (drawn at twice it): the reference's light
/// neutral grey.
const FLOOR: [f32; 3] = [0.452, 0.466, 0.52];
/// The reference view: heading (degrees from +z towards +x), the heap's
/// turn (degrees), aim height (m), elevation (degrees), distance (m),
/// vertical field of view (degrees: a long lens).
const YAW: f32 = 0.0;
const TURN: f32 = 0.0;
const AIM: f32 = 0.085;
const EL: f32 = 24.0;
const DIST: f32 = 3.5;
const FOV: f32 = 12.8;
/// The key light's heading off the camera's (degrees; negative from the
/// left) and elevation, its size (degrees) and strength, and the fill.
const SUN: f32 = -72.0;
const SUN_EL: f32 = 45.0;
const SOFT: f32 = 9.0;
const LIGHT: f32 = 2.15;
const FILL: f32 = 0.37;
const CONTACT: f32 = 0.5;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let f = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(name, v).range(lo, hi).describe(what);
    let view =
        kit.param_f32(f("kit.proc_field.loam_view", 0.0, 0.0, 2.0, "0 the dark loam's reference view, 1 a close-up of the clods on the lower slope, 2 low along the floor").step(1.0))?;
    let yaw = kit.param_f32(f("kit.proc_field.loam_yaw", YAW, -180.0, 180.0, "The camera's heading round the heap (degrees from +z towards +x)"))?;
    let turn = kit.param_f32(f("kit.proc_field.loam_turn", TURN, -180.0, 180.0, "The heap's turn on the floor (degrees)"))?;
    let aim = kit.param_f32(f("kit.proc_field.loam_aim", AIM, -0.2, 0.5, "The reference view's aim: the height the camera looks at (m)"))?;
    let el = kit.param_f32(f("kit.proc_field.loam_el", EL, 0.0, 85.0, "The reference view's elevation (degrees above the floor)"))?;
    let dist = kit.param_f32(f("kit.proc_field.loam_dist", DIST, 0.3, 12.0, "The reference view's distance from its aim (m)"))?;
    let fov = kit.param_f32(f("kit.proc_field.loam_fov", FOV, 4.0, 60.0, "The reference view's vertical field of view (degrees)"))?;
    let sun = kit.param_f32(f("kit.proc_field.loam_sun", SUN, -180.0, 180.0, "The key light's heading off the camera's (degrees; negative from the left)"))?;
    let sun_el = kit.param_f32(f("kit.proc_field.loam_sun_el", SUN_EL, 10.0, 85.0, "The key light's elevation (degrees)"))?;
    let soft = kit.param_f32(f("kit.proc_field.loam_soft", SOFT, 1.0, 20.0, "The key light's size: how soft the heap's shadow is (degrees)"))?;
    let light = kit.param_f32(f("kit.proc_field.loam_light", LIGHT, 0.2, 6.0, "The key light's strength"))?;
    let fill = kit.param_f32(f("kit.proc_field.loam_fill", FILL, 0.0, 2.0, "The studio's even fill light"))?;
    let contact = kit.param_f32(f("kit.proc_field.loam_contact", CONTACT, 0.0, 1.0, "How dark the floor is right under the heap's foot (soft contact; 0: none)"))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.loam_floor", FLOOR).describe("The studio floor's colour under the heap"))?;
    kit.resource(Loam {
        view,
        yaw,
        turn,
        aim,
        el,
        dist,
        fov,
        sun,
        sun_el,
        soft,
        floor,
        contact,
        light,
        fill,
        set_for: f32::NAN,
        model: None,
    });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && let Some(v) = reg.value(id).and_then(|like| evani_procedural::studio::coerce(like, v))
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

/// The heap's model: built once from the library and handed to the models.
fn model(world: &mut World) -> Option<ModelId> {
    let mut g = *world.res::<Loam>();
    if g.model.is_none() {
        let parts = soil::heap(soil::RADIUS, soil::HEIGHT, 69, 1.0);
        let surfaced: Vec<_> = parts.iter().filter_map(|p| p.surface.as_ref().map(|s| (&p.mesh, s, p.uvs.as_slice()))).collect();
        let bytes = evani_procedural::surface::glb_given(&surfaced)?;
        let mut models = world.try_res_mut::<Models>()?;
        g.model = Some(models.add_glb("procedural/soil-dark-loam.glb", &bytes));
        drop(models);
        *world.res_mut::<Loam>() = g;
    }
    g.model
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Loam>();
    let (view, yaw, turn, aim, el, dist, fov, sun, sun_el, soft, light, fill) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round().clamp(0.0, 2.0),
            reg.f32(g.yaw),
            reg.f32(g.turn),
            reg.f32(g.aim),
            reg.f32(g.el),
            reg.f32(g.dist),
            reg.f32(g.fov),
            reg.f32(g.sun),
            reg.f32(g.sun_el),
            reg.f32(g.soft),
            reg.f32(g.light),
            reg.f32(g.fill),
        )
    };
    let cam = match view as u32 {
        1 => orbit(yaw - 24.0, 24.0, 1.1, [-0.2, 0.09, 0.2], 16.0),
        2 => orbit(yaw + 20.0, 6.0, 1.3, [0.12, 0.05, 0.3], 18.0),
        _ => orbit(yaw, el, dist, [0.0, aim, 0.0], fov),
    };
    let key = view * 1000.0 + sun + sun_el * 0.0031 + soft * 0.37 + light * 7.7 + fill * 3.3 + dist * 0.001 + yaw * 0.013;
    if key != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the camera's upper left, a broad neutral fill.
        let (e, az) = (sun_el.to_radians(), (yaw + sun).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 1.0, 1.0]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(light));
        set(&mut reg, "render.ambient", ParamValue::Color([fill, fill, fill * 1.025]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.9));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.3, 0.3, 0.3]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.04));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.3));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        // A crumbly soil mirrors nothing of the room.
        set(&mut reg, "render.ibl.specular", ParamValue::F32(0.0));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.69, 0.69, 0.7]));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist + 0.6));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist + 3.0));
        // The reference's backdrop darkens a little to its corners.
        set(&mut reg, "fx.vignette.intensity", ParamValue::F32(0.12));
        set(&mut reg, "fx.vignette.radius", ParamValue::F32(0.62));
        set(&mut reg, "fx.vignette.softness", ParamValue::F32(0.9));
        set(&mut reg, "fx.vignette.roundness", ParamValue::F32(0.6));
        // A small shadow box: the heap is under a metre across.
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(3.0));
        set(&mut reg, "fx.shadows.bias", ParamValue::F32(0.0001));
        set(&mut reg, "fx.shadows.normal_offset", ParamValue::F32(0.004));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(soft));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.35));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(0.04));
        set(&mut reg, "kit.proc_field.facet_floor.contact", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.far", ParamValue::F32(0.0));
        drop(reg);
        // Nothing of the library is placed: the heap is drawn below.
        if g.set_for.is_nan()
            && let Some(mut s) = world.try_res_mut::<ProcScene>()
        {
            s.clear();
        }
        g.set_for = key;
        *world.res_mut::<Loam>() = g;
    }
    if let Some(id) = model(world)
        && let Some(mut models) = world.try_res_mut::<Models>()
    {
        let q = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], turn.to_radians());
        models.draw(ModelInstance::new(id, [0.0, 0.0, 0.0]).rotation(q));
    }
    let (rgb, contact) = {
        let reg = world.res::<ParamRegistry>();
        (reg.vec3(g.floor), reg.f32(g.contact))
    };
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
    if contact > 0.0 {
        // Soft contact round the heap's foot, where it thins to the floor.
        rw.blob_strength = contact;
        let n = 12;
        for i in 0..n {
            let a = (i as f32 / n as f32 * std::f32::consts::TAU) + turn.to_radians();
            let r = soil::RADIUS * 0.8;
            rw.draw_blob([a.cos() * r, -0.03, a.sin() * r], soil::RADIUS * 0.42);
        }
    }
}
