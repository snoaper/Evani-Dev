//! The layered finishes' scenes (feature `material-layers`): four original
//! models made from formulas, each carrying one glTF extension that
//! `cargo xtask convert` turned into `extras.evani_layers`:
//!
//! * `layer-velvet`: a round velvet pouf (`KHR_materials_sheen` plus
//!   `extras.evani_cloth`), the cushion scene;
//! * `layer-coat`: the shader ball (a ball with a recessed band on a foot,
//!   `tools/models/layer_test_shapes.py coat-ball`) under
//!   `KHR_materials_clearcoat`;
//! * `layer-brushed`: a steel disc with radial tangents and
//!   `KHR_materials_anisotropy`, on a stand;
//! * `layer-bubble`: the same shader ball, dark, under
//!   `KHR_materials_iridescence` (a soap film on a dark ball: the lit pass is
//!   opaque).
//!
//! The shader ball scene shows one model at the origin, picked by the
//! material's `show` (coat, bubble or brushed); the cushion scene shows the
//! velvet pouf. Each draws its own floor and a contact shadow. A pale ball
//! stands in while a model streams in.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;
use evani_kit_material_view::{Viewer, ViewerData};
use evani_render::model::{ModelHandle, ModelInstance, Models};
use evani_render::{MeshId, RenderWorld};

use super::common::{material, mesh, showing};

/// The four models (files `models/<id>[.low].glb`).
pub const IDS: [&str; 4] = ["layer-velvet", "layer-coat", "layer-brushed", "layer-bubble"];
/// The scenes this family draws.
pub const SCENES: [&str; 2] = ["ball", "cloth"];
/// The disc's middle above the floor (it stands on a low block).
pub const DISC_Y: f32 = 0.55;
/// How far the disc tilts up past the half vector (radians, 20 degrees), so
/// the brushed streak crosses its middle instead of lighting it all.
pub const DISC_TILT: f32 = 0.35;

/// The scene's state (a world resource).
#[derive(Debug, Default, Clone)]
pub struct LayersScene {
    pub handles: Option<[ModelHandle; 4]>,
    /// The model drawn this frame (index into [`IDS`]), once streamed in.
    pub drawn: Option<usize>,
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(LayersScene::default());
    ctx.system(SystemDesc::new("material-view.scene.layers", Stage::Frame, run));
}

/// Which model a material shows (`show` in the library).
pub fn model_for(show: Option<&str>) -> usize {
    match show {
        Some("velvet") => 0,
        Some("brushed") => 2,
        Some("bubble") => 3,
        _ => 1,
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// `a * b` (x, y, z, w).
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The disc's turn: its face (+z) along the half vector between the home
/// camera and the sun, tilted up by [`DISC_TILT`]. The disc holds still
/// while the camera orbits, so the streak slides across it.
pub fn disc_rotation(home: &CameraPose, sun: [f32; 3]) -> [f32; 4] {
    let p = [0.0, DISC_Y, 0.0];
    let v = norm([home.eye[0] - p[0], home.eye[1] - p[1], home.eye[2] - p[2]]);
    let l = norm(sun);
    let h = norm([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    let yaw = h[0].atan2(h[2]);
    let pitch = -h[1].clamp(-1.0, 1.0).asin() - DISC_TILT;
    let (sy, cy) = (yaw * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    qmul([0.0, sy, 0.0, cy], [sp, 0.0, 0.0, cp])
}

fn sun(world: &World) -> [f32; 3] {
    let reg = world.res::<ParamRegistry>();
    match reg.id("render.light.direction").and_then(|id| reg.value(id)) {
        Some(ParamValue::Vec3(d)) => d,
        _ => [0.45, 0.8, 0.3],
    }
}

fn run(world: &mut World) {
    if !showing(world, &SCENES) {
        if let Some(mut s) = world.try_res_mut::<LayersScene>() {
            s.drawn = None;
        }
        return;
    }
    let Some(data) = world.try_res::<ViewerData>().map(|d| d.0.clone()) else { return };
    let Some((mat, home)) = world.try_res::<Viewer>().map(|v| (v.material.clone(), v.home.pose(0.0))) else { return };
    let which = model_for(data.material(&mat).and_then(|(_, m)| m.show.as_deref()));
    let (ground, stone) = (material(world, "ground"), material(world, "stone"));
    let sun_dir = sun(world);
    let rot = (which == 2).then(|| disc_rotation(&home, sun_dir));
    {
        let mut rw = world.res_mut::<RenderWorld>();
        rw.draw_mesh(super::common::floor(ground, super::common::FLOOR_SAND));
        // The pouf (0) is 0.45 m round, the shader ball's foot about 0.3 m,
        // the disc's stand 0.25 m. A blob is dark to 0.45 of its radius and
        // gone at its rim, so it reaches well past the footprint (critic
        // round 3: at 0.52 the pouf's disc lay almost wholly under the pouf).
        let r = match which {
            0 => 0.8,
            2 => 0.32,
            _ => 0.5,
        };
        if which == 1 {
            // The shader ball's sphere (0.7 m round, its centre 0.4 m up)
            // throws its own round shadow away from the sun, past the foot:
            // a tight contact disc under the foot, then soft discs where the
            // sphere's middle and upper part throw their shadow, so Low shows
            // the shadow to the side as Ultra's shadow map does (critic
            // rounds 10 and 11: a wide disc under the foot read as a halo and
            // hid the cast). Tiers with a shadow map skip blobs.
            super::common::contact(&mut rw, [0.0, 0.0, 0.0], 0.3, 0.75);
            let s = sun_dir;
            // (The studio sun is front right and high, so the shadow falls
            // back left, mostly behind the sphere; what shows is the shadow
            // of the sphere's left limb, beside the shadow's middle line, so
            // the discs span its width: left, middle and right at three
            // heights, dark cores reaching past the sphere's outline as
            // Ultra's shadow does.)
            let flat = (s[0] * s[0] + s[2] * s[2]).sqrt().max(1e-3);
            let side = [s[2] / flat, -s[0] / flat];
            // (Three heights close together: at two, the discs parted into
            // two stacked lobes.)
            for h in [0.33f32, 0.44, 0.55] {
                let k = h / s[1].max(0.3);
                for w in [-0.22f32, 0.0, 0.22] {
                    super::common::contact(&mut rw, [-s[0] * k + side[0] * w, 0.0, -s[2] * k + side[1] * w], 0.27, 0.0);
                }
            }
        } else {
            super::common::contact(&mut rw, [0.0, 0.0, 0.0], r, if which == 0 { 0.5 } else { 1.0 });
        }
        if rot.is_some() {
            // The disc's stand.
            rw.draw_mesh(mesh(MeshId::BEVEL_CUBE, [0.0, 0.025, 0.0], [0.5, 0.05, 0.3], stone, [0.3, 0.3, 0.32, 1.0]));
        }
    }
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let Some(mut st) = world.try_res_mut::<LayersScene>() else { return };
    // Native runs (tests, the headless check): the manifest and the
    // requested files from the game's own folder; the browser streams them.
    #[cfg(not(target_arch = "wasm32"))]
    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/");
    #[cfg(not(target_arch = "wasm32"))]
    if st.handles.is_none() {
        let m = std::fs::read_to_string(format!("{DIR}stream.json"))
            .map_err(|e| e.to_string())
            .and_then(|t| evani_render::model::stream::manifest_from_text(&t));
        models.set_manifest(m.ok());
    }
    let hs = *st.handles.get_or_insert_with(|| IDS.map(|id| models.handle(id)));
    #[cfg(not(target_arch = "wasm32"))]
    for (id, url) in models.take_requests() {
        let file = url.strip_prefix(evani_render::model::stream::FOLDER).unwrap_or(&url).to_string();
        match std::fs::read(format!("{DIR}{file}")) {
            Ok(b) => models.deliver(id, Ok(&b)),
            Err(e) => models.deliver(id, Err(format!("{file}: {e}"))),
        }
    }
    let at = if which == 2 { [0.0, DISC_Y, 0.0] } else { [0.0; 3] };
    st.drawn = None;
    if let Some(id) = models.resolve(hs[which]) {
        let inst = ModelInstance::new(id, at);
        models.draw(match rot {
            Some(q) => inst.rotation(q),
            None => inst,
        });
        st.drawn = Some(which);
        return;
    }
    drop((models, st));
    // While the model streams in: a pale ball its size.
    world.res_mut::<RenderWorld>().draw_mesh(mesh(MeshId::SPHERE, [0.0, 0.45, 0.0], [0.8; 3], stone, [0.7, 0.7, 0.7, 1.0]));
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::ErrorLog;
    use evani_kit_material_view::Command;

    fn glb(id: &str) -> evani_gltf::Gltf {
        let f = format!("{}/assets/models/{id}.glb", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&f).unwrap_or_else(|e| panic!("{f}: {e}"));
        evani_gltf::Gltf::from_glb(&bytes).unwrap()
    }

    /// Each material of the family draws its own model, and nothing else
    /// of the family draws in another family's scene.
    #[test]
    fn each_material_draws_its_model() {
        let mut app = crate::build_app();
        app.run_headless(3, 1000.0 / 60.0);
        for (m, want) in [("layers.clearcoat", 1), ("layers.soap_film", 3), ("layers.brushed", 2), ("layers.velvet", 0), ("layers.woven", 0)] {
            app.world.res_mut::<Viewer>().push(Command::Material(m.into()));
            app.run_headless(8, 1000.0 / 60.0);
            assert_eq!(app.world.res::<LayersScene>().drawn, Some(want), "{m}");
        }
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        if crate::FAMILIES.contains(&"characters") {
            app.world.res_mut::<Viewer>().push(Command::Scene("head".into()));
            app.run_headless(2, 1000.0 / 60.0);
            assert_eq!(app.world.res::<LayersScene>().drawn, None);
        }
    }

    /// Each shipped file carries its layer extras and keeps its tangents.
    #[test]
    fn the_shipped_models_carry_their_layers() {
        let want = ["sheen", "clearcoat", "anisotropy", "iridescence"];
        for (id, layer) in IDS.iter().zip(want) {
            for file in [id.to_string(), format!("{id}.low")] {
                let g = glb(&file);
                assert!(g.materials[0].extras["evani_layers"][layer].is_object(), "{file}");
                let p = evani_render::model::prepare(&g).unwrap();
                assert!(p.parts.iter().all(|x| x.tangents), "{file}: tangents kept");
            }
        }
        // The live strengths the panel shows are registered.
        let app = crate::build_app();
        let reg = app.world.res::<ParamRegistry>();
        for name in evani_material_layers::params::NAMES {
            assert!(reg.id(name).is_some(), "{name} is registered");
        }
    }

    #[test]
    fn the_disc_faces_between_the_camera_and_the_sun() {
        let home = CameraPose { eye: [0.0, 1.4, 2.6], target: [0.0, DISC_Y, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 35.0 };
        let q = disc_rotation(&home, [0.9, 0.25, 0.35]);
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let n = [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)];
        assert!(n[0] > 0.2 && n[1] > 0.3 && n[2] > 0.4, "{n:?}");
    }
}
