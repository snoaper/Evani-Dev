//! The concrete garden cone scene (feature `cone`, the live cone session,
//! 2026-11-04): the procedural library's cast-concrete cone in the engine's
//! realism studio (`evani_procedural::studio`), framed and lit as its
//! reference picture (best.png: a low camera about 0.57 m up and 3.2 m off,
//! a 26 degree lens, a warm low key from the right and a little in front,
//! the soft shadow going left and back), so its look can be tuned live
//! beside the reference.
//!
//! `kit.proc_field.cone_view`: 0 the reference's framing, 1 a grazing
//! close-up of the cone's upper half (the key skims across its front).
//! `kit.proc_field.cone_subject`: 0 the cone, 1 the concrete cube, 2 the
//! sandstone column, 3 the granite ball: the other three share the concrete
//! shader, the studio or the stone, and are shot before and after a cone
//! change to show they draw the same. They keep the session's first scene
//! (its light, floor and camera) exactly, so their pictures stay comparable;
//! the cone's own staging (round 2: a concrete floor fading into haze, a
//! warm key on neutral concrete, softer shadows) is the cone's alone.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::{AssetInfo, FnAsset, Output, Part, ProcScene, Registry, Values};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The subjects, by `kit.proc_field.cone_subject`.
pub const SUBJECTS: [&str; 4] = ["geometry-cone", "geometry-cube-cuboid", "geometry-cylinder", "geometry-uv-sphere"];

/// The cone scene's own floor (dev only, registered by this scene at run
/// time, never in the library): a cast-concrete slab with the cone's cloudy
/// patches. (The registry takes an asset only in a category it knows.)
const FLOOR: AssetInfo = AssetInfo::cpu("geometry", "cone-scene-floor", "Cone scene floor");

#[derive(Debug, Clone, Copy)]
pub struct Cone {
    view: F32Param,
    subject: F32Param,
    /// The (view, subject) the scene was last set up for (NaN: not yet).
    set_for: (f32, f32),
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.cone_view", 0.0)
            .range(0.0, 1.0)
            .step(1.0)
            .describe("0 the reference's framing (best.png), 1 a grazing close-up of the cone's upper half"),
    )?;
    let subject = kit.param_f32(
        ParamDef::f32("kit.proc_field.cone_subject", 0.0)
            .range(0.0, 3.0)
            .step(1.0)
            .describe("0 the concrete cone, 1 the concrete cube, 2 the sandstone column, 3 the granite ball (the no-change proof)"),
    )?;
    kit.resource(Cone {
        view,
        subject,
        set_for: (f32::NAN, f32::NAN),
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

/// The key, elevation and azimuth in degrees (azimuth 0 towards +z, the
/// camera's side; 90 towards +x, the picture's right): best.png's shadow
/// runs off the left edge and a little back, so the sun is low, on the right
/// and a little in front (round 1 measured its front at 133 of 255 against
/// our 161 at azimuth 62: more from the side; round 8, with best.png's
/// camera re-solved, the body's mean over its middle 128 against our 118
/// at 72 and 131 at 60). The close-up's key is lower and more from the
/// side, so it skims the cone's front.
pub const SUN: (f32, f32) = (24.0, 68.0);
pub const SUN_CLOSE: (f32, f32) = (16.0, 84.0);
/// The session's first scene (round 0), kept for the other subjects.
const SUN_R0: (f32, f32) = (24.0, 62.0);

/// How much larger the floor is drawn than it is built.
const FLOOR_SCALE: f32 = 4.0;

/// The floor slab: 30 m square, lying on the ground under the cone's foot
/// (no casting lift lines, no pores: a floor seen edge-on).
fn floor(_v: &Values, s: u32) -> Output {
    use evani_procedural::surface::{cast_concrete, finish};
    // A block's base is at its `at`, and a generated asset is pressed up to
    // y 0: a slab 0.2 mm thick lying on the ground, the cone's foot on it.
    // Built at a quarter size and placed four times larger, so its patches
    // are four times the cone's (0.5-2 m, round from above, the review's
    // floor) and its fine mottle a floor's.
    let m = evani_procedural::shapes::block([30.0 / FLOOR_SCALE, 0.0002, 30.0 / FLOOR_SCALE], [0.0, 0.0, 0.0]);
    // Round 9: the cone's foot band no longer reaches the floor; its tint
    // takes the darkening the band gave it in round 8.
    let a = [0.37 * 0.79, 0.362 * 0.76, 0.35 * 0.71];
    let sf = cast_concrete(finish(0.0))
        .tint(a)
        // The cone's patches (bit 5) without its sand tooth (bit 6).
        .extras(32 | 64)
        .set_colour(1, [a[0] * 0.88, a[1] * 0.88, a[2] * 0.885])
        .set_colour(2, [a[0] * 1.1, a[1] * 1.1, a[2] * 1.095])
        .number(1, 0.0)
        .number(2, 0.0)
        // The grain's relief at an eighth (slot 4's upper half, the
        // concrete's relief control): drawn four times larger it read as
        // sandpaper.
        .number(4, 0.5 + 0.12 / 8.0)
        .number(5, 0.0)
        .number(7, 0.0)
        .number(8, 0.6)
        .number(9, 0.5)
        .seed(s ^ 0x51f0);
    Output::Parts(vec![Part::surfaced(m, sf)])
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Cone>();
    let (view, subject) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round(), reg.f32(g.subject).round().clamp(0.0, 3.0))
    };
    let cone = subject < 0.5;
    if let Some(mut r) = world.try_res_mut::<Registry>()
        && r.get(&FLOOR.feature()).is_none()
    {
        r.add(Box::new(FnAsset { info: FLOOR, params: &[], make: floor }));
    }
    if (view, subject) != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the scene's own key and fill.
        let _ = evani_procedural::studio::apply(&mut reg);
        // Adaptive quality off: on a busy machine it sheds the shadows and
        // the render scale, so the same view would not draw the same.
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        let sun = if view > 0.5 {
            SUN_CLOSE
        } else if cone {
            SUN
        } else {
            SUN_R0
        };
        let (el, az) = (sun.0.to_radians(), sun.1.to_radians());
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        // The screen-space AO drew streaks beside a subject's foot (the cube
        // session); the soft shadow carries the contact.
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.0));
        set(&mut reg, "render.exposure", ParamValue::F32(if cone { 0.47 } else { 0.6 }));
        if cone {
            // Round 2 (the review): neutral grey concrete, the warmth in the
            // key alone (about 3,800 K); the shade side lifted by the bright
            // floor's bounce; the floor fading into a warm haze with no
            // horizon line; the soft shadow's search widened so its edge
            // blurs as it runs away from the foot.
            set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.86, 0.72]));
            // Tones measured against best.png (lit, front centre and shade
            // patches: 169, 119, 70 of 255 against its 162, 122, 70).
            set(&mut reg, "render.light.intensity", ParamValue::F32(2.3));
            set(&mut reg, "render.ambient", ParamValue::Color([0.79, 0.74, 0.68]));
            set(&mut reg, "render.sky.horizon", ParamValue::Color([1.3, 1.26, 1.2]));
            set(&mut reg, "render.sky.ground", ParamValue::Color([0.92, 0.8, 0.67]));
            set(&mut reg, "render.clear_color", ParamValue::Color([0.74, 0.66, 0.58]));
            set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(3.6));
            set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(10.0));
            set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(24.0));
            set(&mut reg, "fx.shadows.distance", ParamValue::F32(7.0));
        } else {
            set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.86, 0.7]));
            set(&mut reg, "render.light.intensity", ParamValue::F32(2.0));
            set(&mut reg, "render.ambient", ParamValue::Color([0.95, 0.9, 0.85]));
            set(&mut reg, "render.sky.horizon", ParamValue::Color([1.3, 1.25, 1.18]));
            set(&mut reg, "render.sky.ground", ParamValue::Color([0.78, 0.72, 0.66]));
            set(&mut reg, "render.clear_color", ParamValue::Color([0.72, 0.66, 0.6]));
            set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(5.0));
            set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(16.0));
            set(&mut reg, "fx.shadows.distance", ParamValue::F32(6.0));
        }
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = (view, subject);
        *world.res_mut::<Cone>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(SUBJECTS[subject as usize], [0.0, 0.0, 0.0]);
        if cone {
            let i = s.place(&FLOOR.feature(), [0.0, 0.0, 0.0]);
            s.placements[i].scale = FLOOR_SCALE;
        }
    }
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    // Round 2 measured best.png's left edge 10 px right of ours; round 4
    // measured its right edge too (the flanks now 1 cm slimmer): the camera
    // 5.6 mm left puts both edges on the reference's.
    let dx = if cone { -0.005 } else { 0.0 };
    rw.camera = if view > 0.5 {
        // The grazing close-up: the cone's upper half, the tip near the top
        // of the frame, the key skimming across its front from the right.
        CameraPose {
            eye: [-0.3, 0.98, 1.72],
            target: [0.0, 0.84, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 27.0,
        }
    } else {
        // best.png. The cone's own camera, round 5: re-solved from its
        // crown and its collar's sides, front top and front bottom (the
        // collar's ellipse read as seen from too low): 0.65 m up, 2.3 m off
        // the axis, pitched 3 degrees down, a 36 degree lens on the 16:9
        // frame. The other subjects keep the session's first camera (0.57 m
        // up, 3.22 m off, 0.85 degrees down, 26.2 degrees).
        let (d, y0, pitch, fov) = if cone { (2.3f32, 0.65f32, 3.0f32, 35.95) } else { (3.22, 0.57, 0.85, 26.2) };
        CameraPose {
            eye: [dx, y0, d],
            target: [dx, y0 - d * pitch.to_radians().tan(), 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: fov,
        }
    };
    // The light, warm studio floor (under the cone's concrete slab).
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.022, 0.0]);
    if !cone {
        f.position[1] = -0.002;
    }
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [0.92, 0.88, 0.84, 1.0];
    rw.draw_mesh(f);
}
