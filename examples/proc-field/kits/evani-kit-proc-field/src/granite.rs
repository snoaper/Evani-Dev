//! The granite ball scene (feature `granite`, the live granite session,
//! 2026-11-04): the procedural library's granite ball in the engine's
//! realism studio (`evani_procedural::studio`), framed and lit as its
//! reference picture (best.png: three-quarter view from a little above, a
//! low warm key from the upper right behind the ball, the soft shadow going
//! left and forward), so its look can be tuned live beside the reference.
//!
//! `kit.proc_field.granite_view`: 0 the reference's framing, 1 a grazing
//! close-up of the ball's upper left (the key skims the surface there).
//! `kit.proc_field.granite_subject`: 0 the granite ball, 1 the sandstone
//! column, 2 the concrete cube: the other two share the stone or the studio,
//! and are shot before and after a granite change to show they draw the same.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The subjects, by `kit.proc_field.granite_subject`.
pub const SUBJECTS: [&str; 3] = ["geometry-uv-sphere", "geometry-cylinder", "geometry-cube-cuboid"];

#[derive(Debug, Clone, Copy)]
pub struct Granite {
    view: F32Param,
    subject: F32Param,
    /// The (view, subject) the scene was last set up for (NaN: not yet).
    set_for: (f32, f32),
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.granite_view", 0.0)
            .range(0.0, 1.0)
            .step(1.0)
            .describe("0 the reference's framing (best.png), 1 a grazing close-up of the ball's upper left"),
    )?;
    let subject = kit.param_f32(
        ParamDef::f32("kit.proc_field.granite_subject", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the granite ball, 1 the sandstone column, 2 the concrete cube (the no-change proof)"),
    )?;
    kit.resource(Granite {
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

/// The key: elevation and azimuth in degrees (azimuth 0 towards +z, the
/// camera's side; 90 towards +x, the picture's right). Live session round
/// B: solved from best.png's highlight (on the ball 60 degrees clockwise
/// from the top, 0.79 of the radius out): a low key from behind right.
pub const SUN: (f32, f32) = (16.0, 121.0);

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Granite>();
    let (view, subject) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round(), reg.f32(g.subject).round().clamp(0.0, 2.0))
    };
    if (view, subject) != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the scene's own key and fill (best.png:
        // a warm low key, a pale warm backdrop, the shade side about half the
        // lit side).
        let _ = evani_procedural::studio::apply(&mut reg);
        let (el, az) = (SUN.0.to_radians(), SUN.1.to_radians());
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        // Round B: warmer, stronger key and less fill (the shaded front
        // measured 93 against best.png's 73-83), and weaker sky reflections
        // (a pale glassy limb).
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.82, 0.6]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.4));
        set(&mut reg, "render.ambient", ParamValue::Color([0.72, 0.7, 0.67]));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.45));
        // The screen-space AO drew streaks beside a subject's foot (the cube
        // session); the soft shadow carries the contact.
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.0));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([1.3, 1.27, 1.22]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.75, 0.72, 0.68]));
        set(&mut reg, "render.exposure", ParamValue::F32(0.6));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.7, 0.66, 0.62]));
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(5.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(16.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(5.0));
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = (view, subject);
        *world.res_mut::<Granite>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(SUBJECTS[subject as usize], [0.0, 0.0, 0.0]);
    }
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = if view > 0.5 {
        // The grazing close-up: the ball's upper left fills the frame, the
        // key skimming across it from the right.
        CameraPose {
            eye: [-0.62, 0.9, 1.02],
            target: [-0.08, 0.62, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 32.0,
        }
    } else {
        // best.png: about 9 degrees above the ball's centre, a 50 mm lens
        // on the 16:9 frame (23 degrees), the 1 m ball about 0.82 of the
        // frame's height.
        let (el, dist) = (9f32.to_radians(), 3.05f32);
        CameraPose {
            eye: [0.0, 0.47 + el.sin() * dist, el.cos() * dist],
            target: [0.0, 0.47, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 23.0,
        }
    };
    // The light studio floor.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [0.88, 0.88, 0.9, 1.0];
    rw.draw_mesh(f);
}
