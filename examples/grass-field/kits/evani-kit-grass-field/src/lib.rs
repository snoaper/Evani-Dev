//! The grass proof kit (evani-grass): a dirt field with four grass patches,
//! one per kind (long meadow, uncut wheat, short lawn, muddy trampled
//! ground) with bare paths between, a walker who crosses the meadow and
//! tramples a path (then steps off it, so the path can be seen springing
//! back), and fixed camera views (`kit.grass_field.view`).
//!
//! Every tunable is a registered parameter (`kit.grass_field.*`); the grass
//! itself is tuned through `material.grass.*`.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{BoolParam, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, LookData, PluginContext, PluginError};
use evani_core::time::Time;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The camera views.
pub const VIEWS: [&str; 9] = ["meadow", "overview", "lawn", "mud", "wheat", "trample", "close", "low", "ears"];

#[derive(Debug, Clone, Copy)]
pub struct Params {
    view: I32Param,
    walker: BoolParam,
    speed: F32Param,
    orbit: F32Param,
}

/// The proof kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct GrassFieldKit;

impl KitPlugin for GrassFieldKit {
    fn name(&self) -> &'static str {
        "grass_field"
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let p = Params {
            view: kit.param_i32(ParamDef::i32("kit.grass_field.view", 0).range(0.0, (VIEWS.len() - 1) as f32).describe("Camera: 0 meadow towards the sun, 1 overview of all four kinds, 2 lawn, 3 mud, 4 wheat, 5 the trampled path, 6 close on the meadow, 7 low in the meadow, 8 in the wheat at ear height"))?,
            walker: kit.param_bool(ParamDef::bool("kit.grass_field.walker", true).describe("The walker who tramples a path through the meadow"))?,
            speed: kit.param_f32(ParamDef::f32("kit.grass_field.walk_speed", 1.3).range(0.2, 6.0).unit("m/s").describe("Walking speed"))?,
            orbit: kit.param_f32(ParamDef::f32("kit.grass_field.orbit", 0.0).range(-1.0, 1.0).unit("rad/s").describe("Turn the camera slowly round its target (0: still)"))?,
        };
        kit.resource(p);
        // The sky dome (the daylight look's `sky` material) comes with the
        // procedural materials, which need one box: a small one out of
        // sight under the field.
        let mut anchor = evani_material_proc::ProcBox::aabb([0.0, -30.0, 0.0], [1.0, -29.0, 1.0], evani_material_proc::surface::GROUND);
        anchor.bake_ao = false;
        let scene = evani_material_proc::Scene { boxes: vec![anchor], regions: Vec::new(), default_region: None };
        evani_material_proc::install(kit, scene).map_err(|e| PluginError(format!("grass_field: {e}")))?;
        kit.system(SystemDesc::new("grass_field.draw", Stage::Frame, draw));
        Ok(())
    }

    fn free_roam(&self) -> bool {
        true
    }
}

/// The four patches: meadow (north-west), wheat (north-east), lawn
/// (south-west), mud (south-east), 3 m bare paths between them.
#[cfg(feature = "grass")]
pub fn patches() -> [evani_grass::Patch; 4] {
    use evani_grass::{Kind, Patch};
    [
        Patch::new(Kind::Meadow, [-70.0, -70.0], [-1.5, -1.5]).feather(2.0),
        Patch::new(Kind::Wheat, [1.5, -70.0], [70.0, -1.5]).feather(1.2),
        Patch::new(Kind::Lawn, [-70.0, 1.5], [-1.5, 50.0]).feather(0.6),
        Patch::new(Kind::Mud, [1.5, 1.5], [70.0, 50.0]).feather(2.5),
    ]
}

/// Where the walker is at `t` seconds (x, z) and which way they face: in
/// from the path along z = -30, east through the meadow, then south off it
/// onto the bare path and on out of view, so the trodden line is left to
/// spring back.
pub fn walker_at(t: f32, speed: f32) -> ([f32; 2], [f32; 2]) {
    let legs: [([f32; 2], [f32; 2]); 3] = [([-46.0, -30.0], [-14.0, -30.0]), ([-14.0, -30.0], [-14.0, 0.0]), ([-14.0, 0.0], [-14.0, 40.0])];
    let mut d = (t * speed).max(0.0);
    for (a, b) in legs {
        let v = [b[0] - a[0], b[1] - a[1]];
        let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let dir = [v[0] / len, v[1] / len];
        if d <= len {
            return ([a[0] + dir[0] * d, a[1] + dir[1] * d], dir);
        }
        d -= len;
    }
    ([-14.0, 40.0], [0.0, 1.0])
}

/// The cameras.
pub fn camera(view: i32, t: f32, orbit: f32) -> CameraPose {
    let pose = |eye: [f32; 3], target: [f32; 3], fov: f32| CameraPose { eye, target, up: [0.0, 1.0, 0.0], fov_y_deg: fov };
    let mut c = match view {
        // Low in the meadow, looking up the field into the late sun.
        1 => pose([0.0, 9.0, 36.0], [0.0, 0.0, -12.0], 50.0),
        2 => pose([-22.0, 1.25, 30.0], [-30.0, 0.0, 16.0], 50.0),
        3 => pose([24.0, 1.5, 30.0], [32.0, 0.0, 16.0], 50.0),
        // Along the edge of the wheat, the sun across it.
        4 => pose([-0.5, 1.55, -8.0], [16.0, 0.9, -34.0], 50.0),
        // Above the walker's line through the meadow.
        5 => pose([-31.0, 5.2, -23.0], [-31.0, 0.0, -30.6], 55.0),
        6 => pose([-36.0, 0.75, -46.0], [-33.0, 0.35, -43.0], 50.0),
        7 => pose([-58.0, 0.45, -58.0], [-30.0, 0.9, -36.0], 52.0),
        8 => pose([3.2, 1.08, -6.0], [7.5, 0.82, -12.5], 48.0),
        // Across the meadow, the low sun off to the right.
        _ => pose([-58.0, 1.35, -58.0], [-39.5, 0.25, -27.0], 50.0),
    };
    if orbit != 0.0 {
        let (dx, dz) = (c.eye[0] - c.target[0], c.eye[2] - c.target[2]);
        let a = orbit * t;
        let (s, k) = a.sin_cos();
        c.eye[0] = c.target[0] + dx * k - dz * s;
        c.eye[2] = c.target[2] + dx * s + dz * k;
    }
    c
}

fn draw(world: &mut World) {
    let p = *world.res::<Params>();
    let t = world.try_res::<Time>().map_or(0.0, |t| t.render_seconds()) as f32;
    let (view, walker, speed, orbit) = {
        let r = world.res::<ParamRegistry>();
        (r.i32(p.view), r.bool(p.walker), r.f32(p.speed), r.f32(p.orbit))
    };
    #[cfg(feature = "grass")]
    let grass = world.try_res::<evani_grass::Grass>().map(|g| g.clone());
    #[cfg(feature = "grass")]
    if let Some(g) = &grass {
        if g.patches().is_empty() {
            for pt in patches() {
                g.add_patch(pt);
            }
        }
    }
    let (white, dust) = {
        let look = world.res::<LookData>();
        (RenderWorld::material(&look, "cloth"), RenderWorld::material(&look, "dirt"))
    };
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = camera(view, t, orbit);
    // The field: dry dust, the colour `material.grass.soil` fades the
    // patches' edges into.
    let mut ground = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, -10.0]);
    ground.scale = [440.0, 1.0, 440.0];
    ground.material = dust;
    rw.draw_mesh(ground);
    if !walker {
        return;
    }
    // The walker: legs swing with the stride, a long coat, a head.
    let (pos, dir) = walker_at(t, speed);
    let yaw = dir[0].atan2(dir[1]);
    let q = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], yaw);
    let side = [dir[1], -dir[0]];
    let stride = (t * speed * 2.6).sin();
    let mut put = |mesh: MeshId, at: [f32; 3], scale: [f32; 3], color: [f32; 3], rot: [f32; 4]| {
        let mut m = MeshInstance::new(mesh, at);
        m.scale = scale;
        m.rotation = rot;
        m.material = white;
        m.color = [color[0], color[1], color[2], 1.0];
        rw.draw_mesh(m);
    };
    for (k, sgn) in [(0.13f32, 1.0f32), (-0.13, -1.0)] {
        let swing = stride * sgn * 0.22;
        let foot = [pos[0] + side[0] * k + dir[0] * swing, pos[1] + side[1] * k + dir[1] * swing];
        put(MeshId::CYLINDER, [foot[0], 0.45, foot[1]], [0.15, 0.9, 0.15], [0.09, 0.08, 0.07], q);
    }
    put(MeshId::BEVEL_CUBE, [pos[0], 1.18, pos[1]], [0.46, 0.66, 0.26], [0.25, 0.20, 0.13], q);
    put(MeshId::SPHERE, [pos[0], 1.68, pos[1]], [0.24, 0.27, 0.24], [0.42, 0.30, 0.22], q);
    rw.draw_blob([pos[0], 0.0, pos[1]], 0.5);
    drop(rw);
    #[cfg(feature = "grass")]
    if let Some(g) = &grass {
        g.trample(evani_grass::Trampler { id: 1, a: [pos[0], 0.05, pos[1]], b: [pos[0], 1.8, pos[1]], radius: 0.45 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walker_crosses_the_meadow_then_leaves_it() {
        let (a, _) = walker_at(0.0, 1.3);
        assert_eq!(a, [-46.0, -30.0]);
        let (b, d) = walker_at(10.0, 1.3);
        assert!(b[0] > -46.0 && b[0] < -14.0 && d == [1.0, 0.0]);
        let (c, _) = walker_at(200.0, 1.3);
        assert!(c[1] > 0.0, "off the meadow at the end");
    }

    #[test]
    fn every_view_looks_at_the_ground() {
        for v in 0..VIEWS.len() as i32 {
            let c = camera(v, 0.0, 0.0);
            assert!(c.eye[1] > 0.2 && c.target[1] < c.eye[1] + 1.0, "{v}");
        }
    }
}
