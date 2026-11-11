//! The fresh powder snow's live scene (feature `snowpowder`, the Asset
//! Studio fresh powder session, column 76, 2026-11-11): the procedural
//! library's heap of fresh powder (`evani_procedural_lib::snow::heap`, the
//! catalogue's `ground.snow.fresh` as real geometry under the ice recipe's
//! powder) in the engine's realism studio under its reference picture's
//! camera and light (a studio still: a low, lobed mound of loose white
//! powder whose foot frays into tufts, a three-quarter view from a little
//! above the floor through a long lens, a soft key light from the upper
//! right behind it, a neutral grey sweep), so it can be tuned and shot
//! beside the reference. Adapted from the dark loam's scene (src/loam.rs,
//! column 69), whose floor pass it shares; the heap is drawn as one model
//! the scene builds itself (the material has no library asset to place).
//!
//! `kit.proc_field.snowpowder_view`: 0 the reference's view, 1 a close-up
//! of the fringe on the left, 2 low along the floor at the heap's foot.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_procedural_lib::snow;
use evani_render::model::{ModelId, ModelInstance, Models};
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct SnowPowder {
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
    backdrop: Vec3Param,
    contact: F32Param,
    light: F32Param,
    fill: F32Param,
    exposure: F32Param,
    wrap: F32Param,
    seed: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
    /// The heap's model once built, and the seed it was built for.
    model: Option<ModelId>,
    built_for: f32,
}

/// The floor's colour by default (drawn at twice it): the reference's
/// neutral grey, a shade cool.
const FLOOR: [f32; 3] = [0.48, 0.5, 0.55];
/// The backdrop behind the floor's far edge.
const BACKDROP: [f32; 3] = [0.63, 0.66, 0.74];
/// The reference view: heading (degrees from +z towards +x), the heap's
/// turn (degrees), aim height (m), elevation (degrees), distance (m),
/// vertical field of view (degrees: a long lens).
const YAW: f32 = 0.0;
const TURN: f32 = 0.0;
const AIM: f32 = 0.03;
const EL: f32 = 27.0;
const DIST: f32 = 1.55;
const FOV: f32 = 14.0;
/// The key light's heading off the camera's (degrees; positive from the
/// right) and elevation, its size (degrees) and strength, and the fill.
const SUN: f32 = 110.0;
const SUN_EL: f32 = 55.0;
const SOFT: f32 = 18.0;
const LIGHT: f32 = 1.9;
const FILL: f32 = 0.42;
const CONTACT: f32 = 0.2;
const EXPOSURE: f32 = 0.265;
const WRAP: f32 = 0.6;
/// The heap's seed.
const SEED: f32 = 76.0;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let f = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(name, v).range(lo, hi).describe(what);
    let view = kit.param_f32(
        f("kit.proc_field.snowpowder_view", 0.0, 0.0, 2.0, "0 the fresh powder's reference view, 1 a close-up of the fringe on the left, 2 low along the floor").step(1.0),
    )?;
    let yaw = kit.param_f32(f("kit.proc_field.snowpowder_yaw", YAW, -180.0, 180.0, "The camera's heading round the heap (degrees from +z towards +x)"))?;
    let turn = kit.param_f32(f("kit.proc_field.snowpowder_turn", TURN, -180.0, 180.0, "The heap's turn on the floor (degrees)"))?;
    let aim = kit.param_f32(f("kit.proc_field.snowpowder_aim", AIM, -0.2, 0.5, "The reference view's aim: the height the camera looks at (m)"))?;
    let el = kit.param_f32(f("kit.proc_field.snowpowder_el", EL, 0.0, 85.0, "The reference view's elevation (degrees above the floor)"))?;
    let dist = kit.param_f32(f("kit.proc_field.snowpowder_dist", DIST, 0.3, 12.0, "The reference view's distance from its aim (m)"))?;
    let fov = kit.param_f32(f("kit.proc_field.snowpowder_fov", FOV, 4.0, 60.0, "The reference view's vertical field of view (degrees)"))?;
    let sun = kit.param_f32(f("kit.proc_field.snowpowder_sun", SUN, -180.0, 180.0, "The key light's heading off the camera's (degrees; positive from the right)"))?;
    let sun_el = kit.param_f32(f("kit.proc_field.snowpowder_sun_el", SUN_EL, 10.0, 85.0, "The key light's elevation (degrees)"))?;
    let soft = kit.param_f32(f("kit.proc_field.snowpowder_soft", SOFT, 1.0, 20.0, "The key light's size: how soft the heap's shadow is (degrees)"))?;
    let light = kit.param_f32(f("kit.proc_field.snowpowder_light", LIGHT, 0.2, 6.0, "The key light's strength"))?;
    let fill = kit.param_f32(f("kit.proc_field.snowpowder_fill", FILL, 0.0, 2.0, "The studio's even fill light"))?;
    let exposure = kit.param_f32(f("kit.proc_field.snowpowder_exposure", EXPOSURE, 0.05, 4.0, "The camera's exposure"))?;
    let wrap = kit.param_f32(f("kit.proc_field.snowpowder_wrap", WRAP, 0.0, 1.0, "How far the key light wraps round into the shade (the look's wrap)"))?;
    let contact = kit.param_f32(f("kit.proc_field.snowpowder_contact", CONTACT, 0.0, 1.0, "How dark the floor is right under the heap's foot (soft contact; 0: none)"))?;
    let seed = kit.param_f32(f("kit.proc_field.snowpowder_seed", SEED, 0.0, 9999.0, "The heap's seed").step(1.0))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.snowpowder_floor", FLOOR).describe("The studio floor's colour under the heap"))?;
    let backdrop = kit.param_vec3(ParamDef::color("kit.proc_field.snowpowder_backdrop", BACKDROP).describe("The studio backdrop's colour behind the floor"))?;
    kit.resource(SnowPowder {
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
        backdrop,
        contact,
        light,
        fill,
        exposure,
        wrap,
        seed,
        set_for: f32::NAN,
        model: None,
        built_for: f32::NAN,
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

/// The heap's model: built from the library for the seed and handed to the
/// models (again when the seed changes).
fn model(world: &mut World, seed: f32) -> Option<ModelId> {
    let mut g = *world.res::<SnowPowder>();
    if g.model.is_none() || g.built_for != seed {
        let parts = snow::heap(snow::RADIUS, snow::HEIGHT, seed as u32, 1.0);
        let surfaced: Vec<_> = parts.iter().filter_map(|p| p.surface.as_ref().map(|s| (&p.mesh, s, p.uvs.as_slice()))).collect();
        let bytes = evani_procedural::surface::glb_given(&surfaced)?;
        let mut models = world.try_res_mut::<Models>()?;
        g.model = Some(models.add_glb(&format!("procedural/snow-fresh-powder-{}.glb", seed as u32), &bytes));
        drop(models);
        g.built_for = seed;
        *world.res_mut::<SnowPowder>() = g;
    }
    g.model
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<SnowPowder>();
    let (view, yaw, turn, aim, el, dist, fov, sun, sun_el, soft, light, fill, exposure, wrap, seed, backdrop) = {
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
            reg.f32(g.exposure),
            reg.f32(g.wrap),
            reg.f32(g.seed).round(),
            reg.vec3(g.backdrop),
        )
    };
    let cam = match view as u32 {
        1 => orbit(yaw - 28.0, 26.0, 0.62, [-0.19, 0.02, 0.1], 16.0),
        2 => orbit(yaw + 22.0, 7.0, 0.8, [0.08, 0.03, 0.2], 18.0),
        _ => orbit(yaw, el, dist, [0.0, aim, 0.0], fov),
    };
    let key = view * 1000.0
        + sun
        + sun_el * 0.0031
        + soft * 0.37
        + light * 7.7
        + fill * 3.3
        + dist * 0.001
        + yaw * 0.013
        + exposure * 11.3
        + wrap * 5.1
        + backdrop[0] * 17.0
        + backdrop[1] * 19.0
        + backdrop[2] * 23.0;
    if key != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from behind the heap on the right, a broad neutral fill.
        let (e, az) = (sun_el.to_radians(), (yaw + sun).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 1.0, 1.0]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(light));
        set(&mut reg, "render.ambient", ParamValue::Color([fill, fill, fill * 1.03]));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.7));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.45, 0.45, 0.45]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.04));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.3));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        // Loose powder mirrors nothing of the room.
        set(&mut reg, "render.ibl.specular", ParamValue::F32(0.0));
        set(&mut reg, "render.exposure", ParamValue::F32(exposure));
        set(&mut reg, "render.clear_color", ParamValue::Color(backdrop));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist - 0.3));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist + 1.3));
        // The reference's backdrop darkens a little to its corners.
        set(&mut reg, "fx.vignette.intensity", ParamValue::F32(0.14));
        set(&mut reg, "fx.vignette.radius", ParamValue::F32(0.62));
        set(&mut reg, "fx.vignette.softness", ParamValue::F32(0.9));
        set(&mut reg, "fx.vignette.roundness", ParamValue::F32(0.6));
        // A small shadow box: the heap is half a metre across.
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(2.0));
        set(&mut reg, "fx.shadows.bias", ParamValue::F32(0.0001));
        set(&mut reg, "fx.shadows.normal_offset", ParamValue::F32(0.003));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(soft));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(wrap));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(0.03));
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
        *world.res_mut::<SnowPowder>() = g;
    }
    if let Some(id) = model(world, seed)
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
            let r = snow::RADIUS * 0.8;
            rw.draw_blob([a.cos() * r, -0.03, a.sin() * r], snow::RADIUS * 0.42);
        }
    }
}
