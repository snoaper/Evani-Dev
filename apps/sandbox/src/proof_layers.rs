//! The layered materials proof (sandbox features `layers-before` and
//! `layers-proof`; never in evani.toml, so the default and ship builds are
//! unchanged). Four original models made from formulas, each carrying one
//! glTF extension that `cargo xtask convert` turned into
//! `extras.evani_layers`:
//!
//! * `layer-velvet`: a round velvet pouf, `KHR_materials_sheen` plus
//!   `extras.evani_cloth` (plain weave, cloth subsurface colour);
//! * `layer-coat`: a red ball under `KHR_materials_clearcoat`;
//! * `layer-brushed`: a steel disc with radial tangents and
//!   `KHR_materials_anisotropy`, turned to catch the sun;
//! * `layer-bubble`: a dark ball under `KHR_materials_iridescence` (a soap
//!   film: ior 1.33, 400 nm). The lit pass is opaque, so it is a film on a
//!   dark ball, not a see-through bubble.
//!
//! The same scene builds twice: `layers-before` on the lit surface pass as
//! it was (the extras are read by nothing), `layers-proof` with evani-render's
//! `material-layers`, for true before and after pictures. A low sun from the
//! right grazes the pouf's rounded edge. The scene shows when the page address
//! asks for it (`?ly`); without it the sandbox is as it was. The page title
//! carries `ly ...` for the headless harness.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::LookData;
use evani_render::model::{ModelHandle, ModelInstance, Models};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The four models (files `models/<id>[.low].glb`).
pub const IDS: [&str; 4] = ["layer-velvet", "layer-coat", "layer-brushed", "layer-bubble"];
/// Where each stands (the pouf is 0.46 m tall, the disc's middle at 0.55 m).
pub const AT: [[f32; 3]; 4] = [[-2.0, 0.0, 0.0], [-0.55, 0.0, 0.0], [0.8, 0.55, 0.0], [2.05, 0.0, 0.0]];
/// The low sun: from the right, a little towards the camera, about 14 degrees up.
pub const SUN: [f32; 3] = [0.9, 0.25, 0.35];
/// The proof camera.
pub const CAMERA: CameraPose = CameraPose { eye: [0.0, 1.7, 5.0], target: [0.0, 0.85, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 42.0 };

/// The proof's state (a world resource).
#[derive(Debug, Default, Clone)]
pub struct ProofLayers {
    /// The scene takes over the frame (`?ly` in the page address; tests set it).
    pub active: bool,
    pub frames: u32,
    pub handles: Option<[ModelHandle; 4]>,
    /// Models drawn this frame (4 once all have streamed in).
    pub drawn: u32,
}

/// The `Stage::Frame` system (registered after the kit and the model proof,
/// so the scene replaces what they drew).
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_layers", Stage::Frame, run)
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

/// How far the disc tilts up past the half vector (radians, 20 degrees).
/// Facing the half vector exactly puts every point of a flat disc at the
/// lobe's peak (a flat white plate, brushed or not); a little off it, the
/// radial brushing lights only where its tangent crosses the half vector,
/// the bright bow-tie streak of turned metal.
pub const DISC_TILT: f32 = 0.35;

/// The disc's turn: its face (+z) along the half vector between the camera
/// and the sun, tilted up by `DISC_TILT` so the brushed streak crosses its
/// middle.
pub fn disc_rotation() -> [f32; 4] {
    let p = AT[2];
    let v = norm([CAMERA.eye[0] - p[0], CAMERA.eye[1] - p[1], CAMERA.eye[2] - p[2]]);
    let l = norm(SUN);
    let h = norm([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    let yaw = h[0].atan2(h[2]);
    let pitch = -h[1].clamp(-1.0, 1.0).asin() - DISC_TILT;
    let (sy, cy) = (yaw * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    qmul([0.0, sy, 0.0, cy], [sp, 0.0, 0.0, cp])
}

fn material(world: &World, name: &str) -> u32 {
    world.try_res::<LookData>().map_or(0, |l| RenderWorld::material(&l, name))
}

fn mesh(id: MeshId, at: [f32; 3], scale: [f32; 3], material: u32, color: [f32; 4]) -> MeshInstance {
    let mut m = MeshInstance::new(id, at);
    m.scale = scale;
    m.material = material;
    m.color = color;
    m
}

/// The page asks for the scene (`?ly` or `&ly`, with or without a value).
pub fn asked(url: Option<&str>) -> bool {
    url.and_then(|u| u.split_once('?')).is_some_and(|(_, q)| {
        q.split('#').next().unwrap_or(q).split('&').any(|kv| kv.split('=').next() == Some("ly"))
    })
}

/// The page address (wasm; `None` natively).
fn page_url() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.document()?.url().ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

fn run(world: &mut World) {
    if world.try_res::<ProofLayers>().is_none() {
        world.insert_resource(ProofLayers { active: asked(page_url().as_deref()), ..ProofLayers::default() });
    }
    let Some(mut p) = world.try_res::<ProofLayers>().filter(|p| p.active).map(|p| p.clone()) else { return };
    if p.frames == 0 {
        // A param the build lacks is simply not there.
        let _ = world.res_mut::<ParamRegistry>().set_by_name("render.light.direction", ParamValue::Vec3(SUN));
    }
    p.frames = p.frames.saturating_add(1);
    let stone = material(world, "stone");
    let ground = material(world, "ground");
    {
        // Take over the frame: the proof's camera, nothing the kit or the
        // model proof drew.
        let mut rw = world.res_mut::<RenderWorld>();
        rw.meshes.clear();
        rw.blobs.clear();
        rw.camera = CAMERA;
        rw.draw_mesh(mesh(MeshId::PLANE, [0.0, 0.0, 0.0], [30.0, 1.0, 30.0], ground, [1.0; 4]));
        // The disc's stand.
        rw.draw_mesh(mesh(MeshId::BEVEL_CUBE, [AT[2][0], 0.025, 0.0], [0.5, 0.05, 0.3], stone, [0.3, 0.3, 0.32, 1.0]));
    }
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    models.draws.clear();
    let hs = *p.handles.get_or_insert_with(|| IDS.map(|id| models.handle(id)));
    p.drawn = 0;
    let mut waiting = Vec::new();
    for (i, h) in hs.iter().enumerate() {
        let Some(id) = models.resolve(*h) else {
            waiting.push(i);
            continue;
        };
        let inst = ModelInstance::new(id, AT[i]);
        models.draw(if i == 2 { inst.rotation(disc_rotation()) } else { inst });
        p.drawn += 1;
    }
    drop(models);
    if !waiting.is_empty() {
        // Placeholders while a model streams in: a pale ball its size.
        let mut rw = world.res_mut::<RenderWorld>();
        for i in waiting {
            rw.draw_mesh(mesh(MeshId::SPHERE, [AT[i][0], 0.45, AT[i][2]], [0.8; 3], stone, [0.7, 0.7, 0.7, 1.0]));
        }
    }
    if p.frames % 10 == 0 {
        let title = format!(
            "ly build={} frames={} drawn={}",
            if cfg!(feature = "layers-proof") { "after" } else { "before" },
            p.frames,
            p.drawn
        );
        #[cfg(target_arch = "wasm32")]
        if let Some(d) = web_sys::window().and_then(|w| w.document()) {
            d.set_title(&title);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = title;
    }
    if let Some(mut r) = world.try_res_mut::<ProofLayers>() {
        *r = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::ErrorLog;

    fn glb(id: &str) -> evani_gltf::Gltf {
        let f = format!("{}/assets/models/{id}.glb", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&f).unwrap_or_else(|e| panic!("{f}: {e}"));
        evani_gltf::Gltf::from_glb(&bytes).unwrap()
    }

    /// The scene streams its four models in, draws all four under the low
    /// sun, and logs nothing.
    #[test]
    fn the_layers_scene_draws_its_four_models() {
        let mut app = crate::build_app();
        app.world.insert_resource(ProofLayers { active: true, ..ProofLayers::default() });
        app.run_headless(8, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let p = app.world.res::<ProofLayers>();
        assert_eq!(p.drawn, 4, "all four models streamed in and were drawn");
        // The proof registered last holds the frame (DECISIONS "Join 6").
        if !cfg!(any(feature = "char-proof", feature = "nature-proof", feature = "playmat-proof")) {
            let reg = app.world.res::<ParamRegistry>();
            assert_eq!(reg.id("render.light.direction").and_then(|id| reg.value(id)), Some(ParamValue::Vec3(SUN)));
            assert_eq!(app.world.res::<RenderWorld>().camera.eye, CAMERA.eye);
        }
    }

    /// Each shipped file carries its layer extras (the KHR extensions are
    /// gone) and keeps its tangents for the anisotropy.
    #[test]
    fn the_shipped_models_carry_their_layers() {
        let want = ["sheen", "clearcoat", "anisotropy", "iridescence"];
        for (id, layer) in IDS.iter().zip(want) {
            for file in [id.to_string(), format!("{id}.low")] {
                let g = glb(&file);
                let m = &g.materials[0];
                assert!(m.extras["evani_layers"][layer].is_object(), "{file}: {}", m.extras);
                let p = evani_render::model::prepare(&g).unwrap();
                assert!(p.parts.iter().all(|x| x.tangents), "{file}: tangents kept");
            }
        }
        assert_eq!(glb("layer-velvet").materials[0].extras["evani_cloth"]["weave"], "plain");
    }

    /// With `material-layers`, model load turns the extras into each
    /// material's layer record and the live strengths are registered.
    #[cfg(feature = "layers-proof")]
    #[test]
    fn the_layer_records_reach_the_renderer() {
        use evani_material_layers::{ANISOTROPY, CLEARCOAT, FLAGS_WORD, IRIDESCENCE, SHEEN, WEAVE_PLAIN};
        let want = [SHEEN | WEAVE_PLAIN, CLEARCOAT, ANISOTROPY, IRIDESCENCE];
        for (id, flags) in IDS.iter().zip(want) {
            let p = evani_render::model::prepare(&glb(id)).unwrap();
            assert_eq!(p.materials[0].layers[FLAGS_WORD] as u32, flags, "{id}");
        }
        let app = crate::build_app();
        let reg = app.world.res::<ParamRegistry>();
        for name in evani_material_layers::params::NAMES {
            assert!(reg.id(name).is_some(), "{name} is registered");
        }
    }

    #[test]
    fn the_address_turns_the_scene_on() {
        assert!(!asked(None));
        assert!(!asked(Some("http://x/")));
        assert!(!asked(Some("http://x/?lyx=1#ly")));
        assert!(asked(Some("http://x/?ly")));
        assert!(asked(Some("http://x/?a=1&ly=1#top")));
    }

    #[test]
    fn the_disc_faces_between_the_camera_and_the_sun() {
        let q = disc_rotation();
        // Rotate +z by q.
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let n = [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)];
        assert!(n[0] > 0.3 && n[1] > 0.1 && n[2] > 0.6, "{n:?}");
        // Tilted up past the half vector, not on it.
        assert!(n[1] > 0.5, "{n:?}");
    }
}
