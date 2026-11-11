//! The oak trees scene (feature `oaktree`, the Asset Studio oak session,
//! column 8): the procedural library's photoreal oaks, a group of three
//! (`procedural.trees.oak_trees.grove`), in the engine's realism studio, lit
//! and framed as their reference picture
//! (`.evani/refs/trees-oak-trees/photoreal/best.png`): a neutral grey
//! studio, a large soft key light from the front left with its shadows
//! filled, the camera level at about half the tree's height, all three
//! crowns in the frame.
//!
//! `kit.proc_field.oaktree_view`: 0 the reference's framing, 1 the trunk
//! and root flare close up, 2 a leaf spray close up. The light, backdrop and
//! floor stay the same in every view.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The asset the scene shows.
pub const ASSET: &str = "trees-oak-trees";

/// The key light's elevation and azimuth (degrees; azimuth from +z towards
/// +x): from in front and a little to the camera's left, high enough that
/// the shadows pool behind the trees and the floor in front stays light,
/// low enough to light the trunks under the crowns as the reference's
/// studio does.
pub const SUN: (f32, f32) = (50.0, -10.0);

#[derive(Debug, Clone, Copy)]
pub struct OakTree {
    view: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.oaktree_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("The oak trees' view: 0 the reference's framing, 1 the trunk close up, 2 a leaf spray close up"),
    )?;
    kit.resource(OakTree { view, set_for: f32::NAN });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && let Some(v) = reg.value(id).and_then(|like| evani_procedural::studio::coerce(like, v))
    {
        let _ = reg.set(id, v);
    }
}

/// The camera for each view (the first oak's base at the origin, 13 m tall
/// by default; the group's others behind it).
pub fn pose(view: f32) -> CameraPose {
    if view > 1.5 {
        // Leaf sprays at the crown's lower edge, from below and in front.
        CameraPose { eye: [-3.5, 4.2, 5.5], target: [-4.5, 5.6, 1.5], up: [0.0, 1.0, 0.0], fov_y_deg: 34.0 }
    } else if view > 0.5 {
        // The trunk and its root flare.
        CameraPose { eye: [-1.2, 2.2, 7.5], target: [0.0, 2.0, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 36.0 }
    } else {
        // The reference's framing: level, at about half the tree's height.
        CameraPose { eye: [-1.25, 6.45, 30.2], target: [-1.25, 6.85, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 30.0 }
    }
}

pub fn draw(world: &mut World) {
    let mut o = *world.res::<OakTree>();
    let view = world.res::<ParamRegistry>().f32(o.view).round();
    if view != o.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the reference's light.
        let _ = evani_procedural::studio::apply(&mut reg);
        // A studio picture holds every effect: adaptive quality may not shed
        // the shadows on a busy machine (as the bench and the model-view gate
        // hold it). Dev scene only.
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        let (el, az) = (SUN.0.to_radians(), SUN.1.to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.97, 0.93]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(2.6));
        set(&mut reg, "render.ambient", ParamValue::Color([0.5, 0.5, 0.5]));
        // A large soft key (a studio softbox): wide penumbrae, and the
        // shadows filled so the floor under the crowns stays light.
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(20.0));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(30.0));
        set(&mut reg, "fx.shadows.strength", ParamValue::F32(0.62));
        // A little screen-space occlusion darkens the crowns' insides.
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.7));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([1.15, 1.15, 1.15]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.72, 0.72, 0.72]));
        set(&mut reg, "render.exposure", ParamValue::F32(0.7));
        // The studio's seamless grey: the backdrop as light as the floor
        // where it fades into it.
        set(&mut reg, "render.clear_color", ParamValue::Color([0.52, 0.52, 0.52]));
        // Rough bark reflects little of the studio.
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.2));
        let d = pose(0.0);
        let dist = (0..3).map(|k| (d.eye[k] - d.target[k]).powi(2)).sum::<f32>().sqrt();
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist * 1.6));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist * 4.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(70.0));
        // The reference's group of three.
        set(&mut reg, "procedural.trees.oak_trees.grove", ParamValue::F32(3.0));
        drop(reg);
        o.set_for = view;
        *world.res_mut::<OakTree>() = o;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(ASSET, [0.0, 0.0, 0.0]);
    }
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = pose(view);
    // The reference's floor: a smooth neutral light grey.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [600.0, 1.0, 600.0];
    f.material = floor;
    f.color = [0.57, 0.58, 0.62, 1.0];
    rw.draw_mesh(f);
}
