//! The character materials proof (feature `char-proof`; it turns on
//! `lit-proof` and evani-render's `char-materials`): a close-up head
//! (`tests/fixtures/models/char-head.glb`, made by `char-head.mjs`) with a
//! wet eye, soft skin, a hair cap and a fur collar, held in front of its own
//! camera out past the spinner grid. The file is compiled in
//! (`include_bytes!`), so the sandbox's assets, its stream.json and its
//! default build stay as they are. `cargo xtask new-game` does not copy
//! this file.
//!
//! The page address picks the view and the shading, so before and after
//! shots come from one build:
//! * `?char=eyes`: the eyes, close; `?char=side`: the hair and the collar
//!   from behind one shoulder; otherwise the face, three-quarter on;
//! * `cm=0`: `material.character.enabled` off (the plain lit pass: before);
//! * `cq=<0..3>`: `material.character.quality` (Lite 0 to High 3; else by
//!   tier).
//!
//! Render-only: nothing flows back into the simulation.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::ErrorLog;
use evani_core::params::ParamRegistry;
use evani_render::RenderWorld;
use evani_render::model::{ModelId, ModelInstance, ModelState, Models};

const HEAD: &[u8] = include_bytes!("../../../tests/fixtures/models/char-head.glb");

/// Where the head's centre is: high, beyond the grid and the other proofs,
/// facing +z (towards the camera, with the sun over the camera's right
/// shoulder).
pub const AT: [f32; 3] = [0.0, 1.6, 34.0];

/// The view the page asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Face,
    Eyes,
    Side,
}

/// What the page address asks for: the view, the `enabled` switch and a
/// quality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ask {
    pub view: View,
    pub enabled: Option<f32>,
    pub quality: Option<f32>,
}

/// The value of query key `k` in `href`, if any.
fn query<'a>(href: &'a str, k: &str) -> Option<&'a str> {
    let q = href.split_once('?')?.1.split('#').next()?;
    q.split('&').find_map(|kv| kv.split_once('=').filter(|(a, _)| *a == k).map(|(_, v)| v))
}

/// Read a page address.
pub fn ask_from(href: &str) -> Ask {
    let view = match query(href, "char") {
        Some("eyes") => View::Eyes,
        Some("side") => View::Side,
        _ => View::Face,
    };
    let num = |k| query(href, k).and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite());
    Ask { view, enabled: num("cm").map(|v| v.clamp(0.0, 1.0)), quality: num("cq").map(|v| v.clamp(-1.0, 3.0).round()) }
}

#[cfg(target_arch = "wasm32")]
fn page() -> Ask {
    ask_from(&web_sys::window().and_then(|w| w.document()).and_then(|d| d.url().ok()).unwrap_or_default())
}

#[cfg(not(target_arch = "wasm32"))]
fn page() -> Ask {
    ask_from("")
}

/// The proof's state.
#[derive(Default)]
pub struct ProofCharacter {
    pub model: Option<ModelId>,
    pub ask: Option<Ask>,
    applied: bool,
    logged: bool,
}

/// The `Stage::Frame` system; registered after the scene's, so its camera
/// wins.
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_character", Stage::Frame, draw)
}

/// The camera for each view (1280 x 720 and up: the whole head, hair and
/// collar on the face view; both eyes across the frame on the eyes view).
/// Every camera stays over 0.15 m from the nearest part of the head, past
/// the default 0.1 m near plane (the eyes view once sat 8 cm from the nose
/// and cut a hole in it), so the proof needs no camera-range feature.
pub fn camera(view: View) -> CameraPose {
    let [x, y, z] = AT;
    match view {
        View::Face => CameraPose { eye: [x + 0.18, y + 0.03, z + 0.6], target: [x, y - 0.01, z], up: [0.0, 1.0, 0.0], fov_y_deg: 30.0 },
        View::Eyes => CameraPose { eye: [x + 0.03, y + 0.03, z + 0.3], target: [x, y + 0.018, z + 0.08], up: [0.0, 1.0, 0.0], fov_y_deg: 16.0 },
        View::Side => CameraPose { eye: [x + 0.38, y + 0.04, z - 0.12], target: [x, y - 0.02, z], up: [0.0, 1.0, 0.0], fov_y_deg: 36.0 },
    }
}

fn apply(reg: &mut ParamRegistry, ask: Ask) {
    for (name, v) in [("material.character.enabled", ask.enabled), ("material.character.quality", ask.quality)] {
        if let (Some(v), Some(h)) = (v, reg.f32_handle(name)) {
            let _ = reg.set_f32(h, v);
        }
    }
}

fn draw(world: &mut World) {
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let Some(mut proof) = world.try_res_mut::<ProofCharacter>() else { return };
    let ask = *proof.ask.get_or_insert_with(page);
    let id = *proof.model.get_or_insert_with(|| models.add_glb("models/char-head.glb", HEAD));
    if let (Some(ModelState::Failed(e)), false) = (models.state(id), proof.logged) {
        proof.logged = true;
        world.res_mut::<ErrorLog>().error(format!("sandbox: character proof: {e}"));
    }
    models.draw(ModelInstance::new(id, AT));
    let first = !proof.applied;
    proof.applied = true;
    drop(models);
    drop(proof);
    if first && let Some(mut reg) = world.try_res_mut::<ParamRegistry>() {
        apply(&mut reg, ask);
    }
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = camera(ask.view);
        // The kit's drifting screen sprites would differ between captures.
        rw.sprites.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_address_picks_the_view_and_the_shading() {
        let a = ask_from("http://127.0.0.1:8290/?char=eyes&cm=0&cq=0");
        assert_eq!(a, Ask { view: View::Eyes, enabled: Some(0.0), quality: Some(0.0) });
        assert_eq!(ask_from("http://127.0.0.1:8290/?char=side").view, View::Side);
        let d = ask_from("http://127.0.0.1:8290/");
        assert_eq!(d, Ask { view: View::Face, enabled: None, quality: None });
        assert_eq!(ask_from("/?cq=9&cm=x").quality, Some(3.0));
        assert_eq!(ask_from("/?cq=9&cm=x").enabled, None);
    }

    /// The head loads and draws with no error, the proof's camera wins, and
    /// its materials are the four character kinds.
    #[test]
    fn proof_draws_the_head() {
        let mut app = crate::build_app();
        app.run_headless(4, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>());
        let id = app.world.res::<ProofCharacter>().model.unwrap();
        assert!(app.world.res::<Models>().ready(id));
        // The proof registered last holds the frame (DECISIONS "Join 6").
        if !cfg!(any(feature = "nature-proof", feature = "playmat-proof")) {
            assert_eq!(app.world.res::<RenderWorld>().camera, camera(View::Face));
        }
        let g = evani_gltf::Gltf::from_glb(HEAD).unwrap();
        let kinds: Vec<_> = g.materials.iter().map(|m| m.extras.get("evani_kind").and_then(|k| k.as_str()).unwrap_or("")).collect();
        // The base cap under the hair locks is untagged: the plain lit pass.
        assert_eq!(kinds, ["skin", "eye", "hair", "fur", ""]);
        // The params the page address drives exist in this build.
        let reg = app.world.res::<ParamRegistry>();
        assert!(reg.f32_handle("material.character.enabled").is_some() && reg.f32_handle("material.character.quality").is_some());
    }
}
