//! The caves' live scene (feature `cave`, the Asset Studio caves session,
//! column 29, 2026-11-08): the procedural library's limestone cave passage
//! (`terrain-caves`) as a game shows it, from a player standing just inside
//! its mouth and looking deeper in, under the reference's camera and light:
//! a standing eye (1.7 m), level, a 70 degree view, the horizon near the
//! middle, and daylight from the mouth behind the player only.
//!
//! A scene, not a studio: the only light is the daylight coming in at the
//! mouth, the library's own [`entrance_lights`] (a row of point lights
//! across the opening, drawn every frame as a game does), the sky's ambient
//! turned right down inside, and the haze fading into the dark further in.
//! It rides in the mountains scene's slot, as the hills' and the scree's:
//! `cave` turns `mountain` on for its modules (the look, the stone and
//! ground recipes).
//!
//! `kit.proc_field.cave_view`: 0 the reference view, 1 the near left wall
//! close up, 2 low along the floor, 3 looking back out at the mouth.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{PluginContext, PluginError};
use evani_procedural::{ProcScene, Registry};
use evani_procedural_lib::caves::{Cave, entrance_lights};
use evani_render::RenderWorld;
use evani_render::light::Light;

const KEY: &str = "terrain-caves";
const PREFIX: &str = "procedural.terrain.caves";

/// The passage as the reference shows it: 40 m deep, its mouth 4.2 m high
/// and 5 m wide, turning a little to the left far in.
const DEPTH: f32 = 40.0;
const OPENING: f32 = 4.2;
const WIDTH: f32 = 5.0;
const BEND: f32 = -20.0;

#[derive(Debug, Clone, Copy)]
pub struct CaveScene {
    view: F32Param,
    stand: F32Param,
    heading: F32Param,
    pitch: F32Param,
    brightness: F32Param,
    ambient: F32Param,
    exposure: F32Param,
    haze_near: F32Param,
    haze_far: F32Param,
    /// The stand and light the scene was last set up for (NaN: not yet).
    set_for: f32,
    lit_for: [f32; 6],
    /// The player's eye and heading (degrees from -z towards +x).
    eye: [f32; 3],
    face: f32,
    /// The entrance lights: position, colour, range, intensity.
    lights: [[f32; 8]; 8],
    count: usize,
    /// Frames to wait after the cave's params change before placing it, so
    /// the library makes it once, with every param already set.
    wait: u32,
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let p = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(&format!("kit.proc_field.cave_{name}"), v).range(lo, hi).describe(what);
    let view = kit.param_f32(p("view", 0.0, 0.0, 3.0, "0 the reference view (a player just inside the mouth, looking in), 1 the near left wall close up, 2 low along the floor, 3 looking back out at the mouth").step(1.0))?;
    let stand = kit.param_f32(p("stand", 2.2, 0.5, 30.0, "How far in from the mouth the player stands (m)"))?;
    let heading = kit.param_f32(p("heading", 0.0, -60.0, 60.0, "The player's heading off the passage's line (degrees; positive: to the right)"))?;
    let pitch = kit.param_f32(p("pitch", 0.0, -30.0, 30.0, "How far the player looks down (degrees; 0: level, the horizon in the middle)"))?;
    let brightness = kit.param_f32(p("brightness", 1.7, 0.0, 6.0, "The daylight coming in at the mouth (the entrance lights' strength)"))?;
    let ambient = kit.param_f32(p("ambient", 0.01, 0.0, 0.5, "The sky's ambient left inside the cave"))?;
    let exposure = kit.param_f32(p("exposure", 2.1, 0.05, 6.0, "The picture's exposure"))?;
    let haze_near = kit.param_f32(p("haze_near", 4.0, 0.0, 200.0, "Where the dark haze deeper in begins (m from the eye)"))?;
    let haze_far = kit.param_f32(p("haze_far", 30.0, 1.0, 1000.0, "Where the dark haze has hidden the rock (m from the eye)"))?;
    kit.resource(CaveScene {
        view,
        stand,
        heading,
        pitch,
        brightness,
        ambient,
        exposure,
        haze_near,
        haze_far,
        set_for: f32::NAN,
        lit_for: [f32::NAN; 6],
        eye: [0.0, 2.3, -2.0],
        face: 0.0,
        lights: [[0.0; 8]; 8],
        count: 0,
        wait: 0,
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

/// A camera at `eye` looking `pitch` degrees down on heading `yaw` (degrees
/// from -z towards +x), a vertical field of view `fov`.
fn look(eye: [f32; 3], yaw: f32, pitch: f32, fov: f32) -> CameraPose {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let d = [y.sin() * p.cos(), -p.sin(), -y.cos() * p.cos()];
    CameraPose { eye, target: [eye[0] + d[0] * 100.0, eye[1] + d[1] * 100.0, eye[2] + d[2] * 100.0], up: [0.0, 1.0, 0.0], fov_y_deg: fov }
}

/// The cave as the scene sets it.
fn cave(world: &World) -> Option<(evani_procedural::Values, u64)> {
    let r = world.try_res::<Registry>()?;
    let reg = world.try_res::<ParamRegistry>()?;
    r.values(&reg, KEY)
}

/// A 70 degree horizontal view at the reference's 16:9 (1672 x 941) as a
/// vertical field of view.
const FOV_Y: f32 = 43.0;

pub fn draw(world: &mut World) {
    let mut g = *world.res::<CaveScene>();
    let (view, stand, hd, pitch, bright, amb, exposure, near, far) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 3.0), reg.f32(g.stand), reg.f32(g.heading), reg.f32(g.pitch), reg.f32(g.brightness), reg.f32(g.ambient), reg.f32(g.exposure), reg.f32(g.haze_near), reg.f32(g.haze_far))
    };
    if stand != g.set_for || g.count == 0 {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        for (n, v) in [("size", DEPTH), ("opening", OPENING), ("width", WIDTH), ("bend", BEND), ("narrow", 0.35), ("rubble", 1.0), ("moss", 0.6), ("focus", stand)] {
            set(&mut reg, &format!("{PREFIX}.{n}"), ParamValue::F32(v));
        }
        drop(reg);
        // Until the library can say where the cave lies, try again next frame.
        let Some((v, seed)) = cave(world) else { return };
        let c = Cave::new(&v, seed as u32);
        let z = -stand;
        let x = c.centre(z);
        g.eye = [x, c.floor(x, z) + 1.7, z];
        // Facing down the passage: towards its middle 12 m further in.
        let ahead = c.centre(z - 12.0) - x;
        g.face = ahead.atan2(12.0).to_degrees();
        let ls = entrance_lights(&v, seed as u32, 1.0);
        g.count = ls.len().min(8);
        for (k, l) in ls.iter().take(8).enumerate() {
            g.lights[k] = [l.position[0], l.position[1], l.position[2], l.color[0], l.color[1], l.color[2], l.range, l.intensity];
        }
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.wait = 3;
        g.set_for = stand;
    }
    let lit = [view, amb, exposure, near, far, bright];
    if lit != g.lit_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // Inside: no sun reaches in; the sky's light only as a faint ambient,
        // the daylight from the mouth comes from the entrance lights.
        set(&mut reg, "render.light.direction", ParamValue::Vec3([0.0, 0.6, 0.8]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.96, 0.9]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(0.0));
        set(&mut reg, "render.ambient", ParamValue::Color([amb, amb * 0.96, amb * 0.9]));
        set(&mut reg, "render.shadow_tint", ParamValue::Color([1.0, 1.0, 1.0]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "render.sky.studio", ParamValue::F32(0.0));
        // The engine's sky light as the ambient (not the film's fixed sky and
        // ground colours), so the ambient above is all there is inside.
        set(&mut reg, "look.stylizedpbr.studio", ParamValue::F32(1.0));
        let a = amb.max(1e-4);
        set(&mut reg, "render.sky.zenith", ParamValue::Color([a, a * 0.97, a * 0.92]));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([a, a * 0.97, a * 0.92]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([a * 0.8, a * 0.72, a * 0.6]));
        set(&mut reg, "render.sky.sun_glow", ParamValue::F32(0.0));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.8));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.15));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.4));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.3));
        set(&mut reg, "render.exposure", ParamValue::F32(exposure));
        // The dark the passage fades into further in.
        set(&mut reg, "render.clear_color", ParamValue::Color([0.0, 0.0, 0.0]));
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(near));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(far.max(near + 1.0)));
        set(&mut reg, "render.camera.near", ParamValue::F32(0.05));
        set(&mut reg, "render.camera.far", ParamValue::F32(400.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(60.0));
        drop(reg);
        g.lit_for = lit;
    }
    if g.wait > 0 {
        g.wait -= 1;
    } else if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(KEY, [0.0, 0.0, 0.0]);
    }
    *world.res_mut::<CaveScene>() = g;
    let face = g.face + hd;
    let cam = match view as u32 {
        // The near left wall close up.
        1 => look(g.eye, face - 42.0, 4.0, 28.0),
        // Low along the floor.
        2 => look([g.eye[0], g.eye[1] - 1.2, g.eye[2]], face + 4.0, 6.0, FOV_Y),
        // Looking back out at the mouth.
        3 => look([g.eye[0], g.eye[1], g.eye[2] - 4.0], face + 180.0, 0.0, FOV_Y),
        _ => look(g.eye, face, pitch, FOV_Y),
    };
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // The daylight at the mouth, every frame, as a game draws it.
    for l in g.lights.iter().take(g.count) {
        rw.draw_light(Light::new([l[0], l[1], l[2]], [l[3], l[4], l[5]], l[6], l[7] * bright));
    }
}
