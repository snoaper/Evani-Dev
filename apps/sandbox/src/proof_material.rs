//! The procedural materials proof (sandbox feature `material-proof`; never
//! in evani.toml or `default`, so the default and ship builds are
//! unchanged): a small walled yard west of the spinner grid built from
//! evani-material-proc boxes (plaster, sandstone and concrete walls with
//! stone and timber trims, paving and brick floors, wooden and painted
//! metal crates, steps, a platform, a ramp and stencil decals, all with
//! baked AO), under the photoreal look's daylight preset and the Preetham
//! sky dome. The camera is held on the yard (from [`SUNWARD_AT`] seconds it
//! turns towards the sun), so captures see the same views every run.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::plugin::PluginContext;
use evani_core::time::Time;
use evani_material_proc::{ProcBox, Scene, V3, surface};
use evani_render::RenderWorld;

/// The yard's centre on the ground (west of the grid, inside the terraces).
pub const AT: V3 = [-28.0, 0.0, 0.0];
/// The floor's top: just above the kit's ground plane (y = 0), so the two
/// never share a plane.
pub const FLOOR_Y: f32 = 0.02;
/// The wall height (m).
pub const WALL_H: f32 = 3.5;

/// The held camera: in the sunlit corner, looking into the yard's back
/// corner, the walls' sunlit faces and the deep blue sky opposite the sun.
pub const CAMERA: CameraPose = CameraPose {
    eye: [AT[0] + 6.2, 2.3, AT[2] + 7.4],
    target: [AT[0] - 1.6, 1.25, AT[2] - 2.2],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 55.0,
};
/// From [`SUNWARD_AT`]: from the back corner towards the low sun, over the
/// crates and the platform: the sky's glow and the long shadows.
pub const SUNWARD: CameraPose = CameraPose {
    eye: [AT[0] - 3.6, 1.7, AT[2] - 3.0],
    target: [AT[0] + 4.0, 1.3, AT[2] + 3.5],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 60.0,
};
/// When the camera turns towards the sun (sim seconds).
pub const SUNWARD_AT: f64 = 8.0;

/// An axis-aligned box from corners relative to [`AT`].
fn b(min: V3, max: V3, s: u32) -> ProcBox {
    ProcBox::aabb(
        [AT[0] + min[0], AT[1] + min[1], AT[2] + min[2]],
        [AT[0] + max[0], AT[1] + max[1], AT[2] + max[2]],
        s,
    )
}

/// A stencil decal 1 cm proud of a wall (never occludes or bakes).
fn decal(min: V3, max: V3, s: u32) -> ProcBox {
    let mut d = b(min, max, s);
    d.decal = true;
    d.occluder = false;
    d.bake_ao = false;
    d
}

/// The yard (10 x 10 m, walls on its west and north sides).
pub fn scene() -> Scene {
    let (f, h) = (FLOOR_Y, WALL_H);
    let mut boxes = Vec::new();
    // Floors: flagstones and brick paving inside, sandy ground on the
    // open south side. World patterns, so the halves join without seams.
    for (min, max, s) in [
        ([-5.0, f - 0.3, -5.0], [0.0, f, 5.0], surface::PAVING),
        ([0.0, f - 0.3, -5.0], [5.0, f, 5.0], surface::BRICK_PAVING),
        ([-5.5, f - 0.3, 5.0], [5.0, f - 0.01, 7.0], surface::GROUND),
    ] {
        let mut fl = b(min, max, s);
        fl.world_pattern = true;
        boxes.push(fl);
    }
    // The north wall in three materials, the west wall in concrete.
    for (min, max, s) in [
        ([-5.5, f, -5.5], [-1.7, h, -5.0], surface::PLASTER),
        ([-1.7, f, -5.5], [1.6, h, -5.0], surface::SANDSTONE),
        ([1.6, f, -5.5], [5.0, h, -5.0], surface::CONCRETE),
        ([-5.5, f, -5.0], [-5.0, h, 5.0], surface::CONCRETE),
    ] {
        let mut w = b(min, max, s);
        w.world_pattern = true;
        boxes.push(w);
    }
    // Trims: a stone coping on the north wall, a timber beam on the west.
    boxes.push(b([-5.6, h, -5.6], [5.1, h + 0.18, -4.92], surface::TRIM_STONE));
    boxes.push(b([-5.6, h, -4.92], [-4.92, h + 0.22, 5.0], surface::TRIM_WOOD));
    // Crates: two wooden (one turned), one painted steel.
    boxes.push(b([-4.4, f, -4.4], [-3.2, f + 1.2, -3.2], surface::WOOD_CRATE));
    boxes.push(b([-4.2, f + 1.2, -4.2], [-3.4, f + 2.0, -3.4], surface::WOOD_CRATE));
    let mut turned = b([-2.6, f, -1.6], [-1.6, f + 1.0, -0.6], surface::WOOD_CRATE);
    turned.rotation = yaw(0.35);
    turned.seed = 7;
    boxes.push(turned);
    boxes.push(b([0.2, f, -4.8], [1.6, f + 1.2, -3.8], surface::METAL_CRATE));
    // A raised platform with two stone steps up its west side and a ramp
    // down its south side.
    let top = f + 0.6;
    boxes.push(b([2.6, f, -2.4], [5.0, top, 0.6], surface::PLATFORM));
    boxes.push(b([2.1, f, -1.6], [2.6, f + 0.4, -0.2], surface::STONE_STEP));
    boxes.push(b([1.6, f, -1.6], [2.1, f + 0.2, -0.2], surface::STONE_STEP));
    boxes.push(ramp(top));
    // Stencils: an A on the plaster, an arrow on the west wall.
    boxes.push(decal([-4.4, 1.2, -5.0], [-2.8, 2.8, -4.99], surface::SIGN_A));
    boxes.push(decal([-5.0, 1.0, 1.2], [-4.99, 1.6, 3.4], surface::ARROW));
    Scene { boxes, regions: Vec::new(), default_region: None }
}

/// A turn of `a` radians about +y.
fn yaw(a: f32) -> [f32; 4] {
    let (s, c) = (a * 0.5).sin_cos();
    [0.0, s, 0.0, c]
}

/// A concrete ramp from the floor up to the platform's south edge (a slab
/// tilted about x; its ends sink into the floor and the platform).
fn ramp(top: f32) -> ProcBox {
    let (z0, z1) = (0.6, 3.0);
    let rise = top - FLOOR_Y;
    let run = z1 - z0;
    let tilt = (rise / run).atan();
    let len = rise.hypot(run);
    let t = 0.08;
    let mut r = b([3.0, 0.0, 0.0], [4.6, 0.0, 0.0], surface::RAMP);
    r.half = [0.8, t, len * 0.5];
    r.centre = [AT[0] + 3.8, FLOOR_Y + rise * 0.5 - t, AT[2] + (z0 + z1) * 0.5];
    let (s, c) = (tilt * 0.5).sin_cos();
    r.rotation = [s, 0.0, 0.0, c];
    r
}

/// Bake the yard and add its draw system, after a camera hold (so the sky
/// dome centres on this frame's eye).
pub fn install(ctx: &mut PluginContext) {
    ctx.system(SystemDesc::new("sandbox.proof_material.camera", Stage::Frame, hold_camera));
    if let Err(e) = evani_material_proc::install(ctx, scene()) {
        ctx.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("sandbox: material proof: {e}"));
    }
}

fn hold_camera(world: &mut World) {
    let t = world.try_res::<Time>().map_or(0.0, |t| t.sim_seconds);
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = if t < SUNWARD_AT { CAMERA } else { SUNWARD };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_yard_bakes_and_draws_through_the_daylight_materials() {
        let pw = evani_material_proc::ProcWorld::new(scene()).unwrap();
        assert!(pw.texels > 0 && pw.rays > 0);
        let mut app = crate::build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let look = app.world.res::<evani_core::plugin::LookData>();
        for m in ["world", "decal", "sky"] {
            assert!(look.materials.iter().any(|d| d.name == m), "no `{m}` material");
        }
        let reg = app.world.res::<evani_core::params::ParamRegistry>();
        assert!(reg.id("material.plaster.color").is_some());
        assert!(reg.id("sky.follow_light").is_some());
        assert_eq!(app.world.res::<evani_core::overlay::ErrorLog>().error_count(), 0);
    }
}
