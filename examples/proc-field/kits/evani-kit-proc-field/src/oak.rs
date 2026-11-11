//! The live oak block (feature `oak`, procedural realism, the oak's own
//! live session): the procedural library's solid-oak bevelled box in the
//! engine's realism studio, lit and framed as its reference picture
//! (`.evani/refs/geometry-bevelled-box/best.png`), so its wood can be tuned
//! live (`procedural.geometry.bevelled_box.*`, the look and light params)
//! and held against the reference on its dev page
//! (`art-source/style-refs/oak-block/`, served by the live bridge).
//!
//! `kit.proc_field.oak_view`: 0 the reference's framing, 1 a grazing
//! close-up along the long face and its top arris, 2 the end grain. The
//! light, backdrop and floor stay the same in every view.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The asset the scene shows.
pub const ASSET: &str = "geometry-bevelled-box";

/// The key light's elevation and azimuth (degrees; azimuth from +z towards
/// +x): low, from the camera's right and in front, so the long face is the
/// brightest (as the reference's: a little brighter than the top), the end
/// face in shade and the long shadow falls to the left. Tuned live against
/// the reference by measurement (top 190 against 186, long face 204 against
/// 197, end 91 against 103, shadow 116 against 101, sRGB red).
pub const SUN: (f32, f32) = (18.0, 45.0);

#[derive(Debug, Clone, Copy)]
pub struct Oak {
    view: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.oak_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("The oak block's view: 0 the reference's framing, 1 a grazing close-up along the long face, 2 the end grain"),
    )?;
    kit.resource(Oak {
        view,
        set_for: f32::NAN,
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

/// The camera for each view (the block is 1 m long on x, 0.8 m high and
/// deep, its base centred on the origin).
pub fn pose(view: f32) -> CameraPose {
    if view > 1.5 {
        // The end grain, from a little in front and above (looking down,
        // as the reference's camera, so no horizon sits behind it).
        CameraPose {
            eye: [-1.95, 1.08, 0.48],
            target: [-0.5, 0.4, 0.02],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 34.0,
        }
    } else if view > 0.5 {
        // Grazing along the long face towards its top arris.
        CameraPose {
            eye: [0.95, 0.98, 0.78],
            target: [-0.05, 0.62, 0.36],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 34.0,
        }
    } else {
        // The reference's framing: the end face on the left, the long face
        // on the right and nearer square on, from a little above (its
        // outline within about 16 px of the reference's on every side).
        CameraPose {
            eye: [-1.222, 1.351, 2.067],
            target: [-0.016, 0.327, -0.021],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 28.0,
        }
    }
}

pub fn draw(world: &mut World) {
    let mut o = *world.res::<Oak>();
    let view = world.res::<ParamRegistry>().f32(o.view).round();
    if view != o.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the reference's light.
        let _ = evani_procedural::studio::apply(&mut reg);
        let (el, az) = (SUN.0.to_radians(), SUN.1.to_radians());
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        set(
            &mut reg,
            "render.light.color",
            ParamValue::Color([1.0, 0.93, 0.84]),
        );
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.6));
        set(
            &mut reg,
            "render.ambient",
            ParamValue::Color([0.46, 0.44, 0.42]),
        );
        // No screen-space AO: it smeared ghost streaks out from the block's
        // outline onto the floor (round 2's review; the concrete cube's
        // session saw the same); the soft shadow carries the contact.
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.0));
        set(
            &mut reg,
            "render.sky.horizon",
            ParamValue::Color([1.3, 1.28, 1.25]),
        );
        set(
            &mut reg,
            "render.sky.ground",
            ParamValue::Color([0.72, 0.7, 0.67]),
        );
        set(&mut reg, "render.exposure", ParamValue::F32(0.66));
        // Oiled oak reflects little of the studio: at the grazing views the
        // top and end read whitish with the studio's full reflections.
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.6));
        // The reference's warm grey floor runs out of frame: the backdrop
        // matches it.
        set(
            &mut reg,
            "render.clear_color",
            ParamValue::Color([0.66, 0.62, 0.58]),
        );
        let d = pose(0.0);
        let dist = (0..3)
            .map(|k| (d.eye[k] - d.target[k]).powi(2))
            .sum::<f32>()
            .sqrt();
        set(
            &mut reg,
            "look.stylizedpbr.haze_near",
            ParamValue::F32(dist * 1.7),
        );
        set(
            &mut reg,
            "look.stylizedpbr.haze_far",
            ParamValue::F32(dist * 5.5),
        );
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(5.0));
        drop(reg);
        o.set_for = view;
        *world.res_mut::<Oak>() = o;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(ASSET, [0.0, 0.0, 0.0]);
    }
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = pose(view);
    // The reference's floor: a smooth, warm light grey.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [0.86, 0.83, 0.81, 1.0];
    rw.draw_mesh(f);
}
