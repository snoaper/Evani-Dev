//! The realism parity scene (feature `wall`, procedural realism round 8):
//! the procedural library's cast-concrete cube, in the engine's realism
//! studio (`evani_procedural::studio`), as a game builds it, so its picture
//! can be compared with the Material library's (docs/PROCEDURAL.md
//! "Applying a realism material in a game"; tools/procedural/parity.mjs).
//!
//! `kit.proc_field.view`: 0 the parity view (the library's default cube
//! under the library's own camera, a 16:9 view), 1 the wall scene (the same
//! concrete as a 3 m wall, 2.2 m high and 0.3 m thick, from a person's
//! height). The game sets only what a game sets: the studio preset, its own
//! sun, a floor, a backdrop colour and a haze, and its camera.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The asset the scene shows.
pub const ASSET: &str = "geometry-cube-cuboid";

#[derive(Debug, Clone, Copy)]
pub struct Wall {
    view: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe(
                "0 the parity view (the library's cube and camera), 1 the concrete as a wall, 2 the owner's studio picture (camera, light, ground and backdrop as his reference 4)",
            ),
    )?;
    kit.resource(Wall {
        view,
        set_for: f32::NAN,
    });
    Ok(())
}

/// The library's camera for a box `lo`..`hi` (apps/material-view
/// proc_data.rs `fit_cam`, the cube's framing: 17 degrees up, 0.84 of a
/// 16:9 view, a 20 degree lens, from azimuth 30).
fn library_pose(lo: [f32; 3], hi: [f32; 3]) -> CameraPose {
    let (el, az, fill, fov, aspect) = (
        17f32.to_radians(),
        30f32.to_radians(),
        0.84f32,
        20.0f32,
        16.0f32 / 9.0,
    );
    let target: [f32; 3] =
        std::array::from_fn(|k| lo[k] + (hi[k] - lo[k]) * if k == 1 { 0.45 } else { 0.5 });
    let back = [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()];
    let right = [az.cos(), 0.0, -az.sin()];
    let up = [-el.sin() * az.sin(), el.cos(), -el.sin() * az.cos()];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let tan_v = (fov.to_radians() * 0.5).tan() * fill;
    let tan_h = tan_v * aspect;
    let mut dist = 0.4f32;
    for i in 0..8 {
        let c = [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let rel: [f32; 3] = std::array::from_fn(|k| c[k] - target[k]);
        let z = dot(rel, back);
        dist = dist
            .max(z + dot(rel, right).abs() / tan_h)
            .max(z + dot(rel, up).abs() / tan_v);
    }
    CameraPose {
        eye: std::array::from_fn(|k| target[k] + back[k] * dist),
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
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

pub fn draw(world: &mut World) {
    let mut w = *world.res::<Wall>();
    let view = world.res::<ParamRegistry>().f32(w.view).round();
    if view != w.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the game's own sun (the library's).
        let _ = evani_procedural::studio::apply(&mut reg);
        // The wall view's sun comes across its face from the camera's side.
        let sun = if view > 1.5 {
            // Reference 4: a soft key from the front left, the shadow going
            // back to the right.
            (38.0f32, -20.0f32)
        } else if view > 0.5 {
            (28.0f32, 40.0f32)
        } else {
            evani_procedural::studio::SUN
        };
        let (el, az) = (sun.0.to_radians(), sun.1.to_radians());
        // View 2's key: warm, as best.png's (the live session, 2026-11-04;
        // reference 4's was a neutral, slightly cool studio key). The
        // screen-space AO drew vertical streaks on the floor beside the
        // cube's foot; the soft shadow carries the contact here.
        if view > 1.5 {
            set(
                &mut reg,
                "render.light.color",
                ParamValue::Color([1.0, 0.88, 0.72]),
            );
            set(&mut reg, "render.light.intensity", ParamValue::F32(1.9));
            // The shaded face's fill (measured against best.png: lit about
            // 150, shade about 76 of 255, a 2:1 ratio).
            set(&mut reg, "render.ambient", ParamValue::Color([1.0, 0.97, 0.93]));
            set(&mut reg, "render.ao.strength", ParamValue::F32(0.0));
            // A brighter studio horizon and floor bounce: best.png's shaded
            // side is about 70% of its lit side (ours was 40%), and a little
            // more exposure.
            set(&mut reg, "render.sky.horizon", ParamValue::Color([1.35, 1.33, 1.3]));
            set(&mut reg, "render.sky.ground", ParamValue::Color([0.75, 0.73, 0.7]));
            set(&mut reg, "render.exposure", ParamValue::F32(0.58));
            // The seed whose chips land on the arrises this camera sees,
            // inside the 40k-triangle budget (seed 7's chips read best but
            // refine to 42k).
            set(&mut reg, "procedural.geometry.cube_cuboid.seed", ParamValue::F32(3.0));
        }
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        // A backdrop and a haze like the library's (a game picks its own).
        set(
            &mut reg,
            "render.clear_color",
            ParamValue::Color(if view > 1.5 {
                [0.66, 0.64, 0.61]
            } else {
                [0.8, 0.74, 0.63]
            }),
        );
        let size = if view > 0.5 && view < 1.5 {
            [3.0, 2.2, 0.3]
        } else {
            [1.0, 1.0, 1.0]
        };
        for (k, n) in ["width", "height", "depth"].iter().enumerate() {
            set(
                &mut reg,
                &format!("procedural.geometry.cube_cuboid.{n}"),
                ParamValue::F32(size[k]),
            );
        }
        let d = library_pose([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]);
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
        // The shadow box fitted to the subject, as the library fits it.
        set(
            &mut reg,
            "fx.shadows.distance",
            ParamValue::F32(if view > 0.5 { 6.0 } else { 4.0 }),
        );
        drop(reg);
        w.set_for = view;
        *world.res_mut::<Wall>() = w;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(ASSET, [0.0, 0.0, 0.0]);
    }
    // The look's stone (base 0.42, 0.40, 0.36, rough 0.9), scaled to the
    // library floor's albedo (0.36, 0.35, 0.33).
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = if view > 1.5 {
        // Reference 4: low (about 5 degrees), 45 degrees round, a 30 degree
        // lens, the cube in the lower middle of the frame.
        CameraPose {
            eye: [2.55, 1.05, 2.55],
            target: [0.0, 0.72, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 30.0,
        }
    } else if view > 0.5 {
        // A person's eye, about 4.5 m off the wall's face, a little to one side.
        CameraPose {
            eye: [2.2, 1.6, 4.6],
            target: [0.1, 1.05, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 35.0,
        }
    } else {
        library_pose([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5])
    };
    // The library's light floor.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = if view > 1.5 {
        [0.86, 0.88, 0.93, 1.0]
    } else {
        [0.857, 0.875, 0.917, 1.0]
    };
    rw.draw_mesh(f);
}
