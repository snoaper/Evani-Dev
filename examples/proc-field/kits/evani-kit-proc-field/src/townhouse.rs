//! The townhouses' live scene (feature `townhouse`, the Asset Studio
//! townhouses session, column 37, 2026-11-08): the procedural library's
//! terrace of townhouses (`buildings-townhouses`) in the engine's realism
//! studio under its reference picture's camera and light (five houses under
//! one slate roof, seen from the front left at a little over standing eye
//! height through a longish lens, the left gable in half shade, on a
//! seamless neutral grey sweep under a soft key from the front left).
//!
//! The camera was solved from the reference's converging lines: the front's
//! eaves and foot meet far to the right (about 4200 px right of the middle
//! of a 1774 px picture), the gable's to the left (about 1580 px), so the
//! lens is about 19.6 degrees high, the front about 31.5 degrees off square;
//! the near corner's 8.6 m eaves span 590 px (about 37.5 m off along the
//! view), the far corner's 410 px (about 54 m); the horizon at 2.1 m on the
//! wall, a little under the ground floor's sills.
//!
//! `kit.proc_field.townhouse_view`: 0 the reference's view, 1 a close-up of
//! the first house's door, steps and ground floor, 2 the roof line (slates,
//! a chimney, the gutter).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct Townhouse {
    view: F32Param,
    yaw: F32Param,
    pitch: F32Param,
    eye: Vec3Param,
    fov: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    floor: Vec3Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The floor's colour by default (drawn at twice it): the reference's
/// neutral studio grey.
const FLOOR: [f32; 3] = [0.315, 0.318, 0.345];

/// The reference view: the eye (m; the terrace's front faces +z, its first
/// house at -x), the heading (degrees from -z towards +x), the pitch
/// (degrees up) and the lens (degrees high).
/// Round 2: solved again from seven corners (both ends' foot and eaves, the
/// left gable's back corners and apex) for the terrace's real 9.5 m depth
/// and 33 degree pitch (round 1's solve assumed 8 m and 36 degrees, so the
/// left corner sat 50 px right of the reference's and the front came out
/// narrower); 8 px rms.
const EYE: [f32; 3] = [-26.14, 2.3, 43.29];
const YAW: f32 = 29.78;
const PITCH: f32 = 5.1;
const FOV: f32 = 19.12;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.townhouse_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the townhouses' reference view (the reference's camera and light), 1 the first house's door and steps, 2 the roof line and a chimney"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.townhouse_yaw", YAW).range(-180.0, 180.0).describe("The reference view's heading (degrees from -z towards +x)"))?;
    let pitch = kit.param_f32(ParamDef::f32("kit.proc_field.townhouse_pitch", PITCH).range(-30.0, 60.0).describe("The reference view's pitch (degrees up)"))?;
    let eye = kit.param_vec3(ParamDef::vec3("kit.proc_field.townhouse_eye", EYE).describe("The reference view's eye (m)"))?;
    let fov = kit.param_f32(ParamDef::f32("kit.proc_field.townhouse_fov", FOV).range(5.0, 90.0).describe("The reference view's lens (degrees high)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.townhouse_sun_az", -12.0).range(-180.0, 180.0).describe("The key light's bearing from the front's normal (degrees; negative: from the terrace's left end)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.townhouse_sun_el", 42.0).range(5.0, 89.0).describe("The key light's elevation (degrees)"))?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.townhouse_floor", FLOOR).describe("The studio floor's colour under the terrace"))?;
    kit.resource(Townhouse { view, yaw, pitch, eye, fov, sun_az, sun_el, floor, set_for: f32::NAN });
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

/// A camera at `eye` heading `yaw` degrees (from -z towards +x), `pitch` up.
fn look(eye: [f32; 3], yaw: f32, pitch: f32, fov: f32) -> CameraPose {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let d = [y.sin() * p.cos(), p.sin(), -y.cos() * p.cos()];
    CameraPose { eye, target: [eye[0] + d[0] * 40.0, eye[1] + d[1] * 40.0, eye[2] + d[2] * 40.0], up: [0.0, 1.0, 0.0], fov_y_deg: fov }
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Townhouse>();
    let (view, yaw, pitch, eye, fov, sun_az, sun_el) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.pitch), reg.vec3(g.eye), reg.f32(g.fov), reg.f32(g.sun_az), reg.f32(g.sun_el))
    };
    let cam = match view as u32 {
        // The first house's door, steps and ground floor windows.
        1 => look([-15.6, 2.4, 15.5], yaw - 6.0, 1.0, 32.0),
        // The roof line: the slates, the second chimney, the gutter.
        2 => look([-13.0, 9.5, 14.0], yaw - 2.0, 8.0, 34.0),
        _ => look(eye, yaw, pitch, fov),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the front, a little from the left end (12 degrees
        // round from the front's normal, 42 up; round 1's 20 lit the gable
        // too brightly once the bricks were lighter; just right of the camera):
        // the fronts lit, the left gable in half shade (about 0.75 of the
        // front, as in the reference: gable brick 86 vs the front's 115),
        // the step and sill shadows falling right and down; a neutral fill.
        // (The direction points at the light, as the other studio scenes.)
        let (e, az) = (sun_el.to_radians(), sun_az.to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.98, 0.95]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.1));
        set(&mut reg, "render.ambient", ParamValue::Color([0.45, 0.45, 0.45]));
        // (Round 2 tried a 2.2 m occlusion radius for contact shade on the
        // floor: it drew dark blotches and dashed lines on the walls.)
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.8));
        set(&mut reg, "render.ao.radius", ParamValue::F32(1.0));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.8, 0.8, 0.8]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.72, 0.72, 0.72]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.3));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.15));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.7));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.8, 0.795, 0.795]));
        // Round 2: a broader key (softer shadow edges), so the cornices'
        // and sills' shadow lines don't break into dashes at this distance.
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(9.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.4));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(70.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(160.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view == 0.0 { 75.0 } else { 45.0 }));
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        // Round 2: a faint broad mottle and the far floor a shade darker
        // (the reference's floor isn't one flat grey).
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(0.05));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(0.05));
        set(&mut reg, "kit.proc_field.facet_floor.contact", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.far", ParamValue::F32(0.04));
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = view;
        *world.res_mut::<Townhouse>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("buildings-townhouses", [0.0, 0.0, 0.0]);
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
