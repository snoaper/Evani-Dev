//! The textured, normal-mapped model proof (evani.toml `models = true`,
//! S3-3, streamed since S3-4): four static copies of the proof figure,
//! one at the middle of each side of the spinner grid, each facing out
//! towards the orbiting camera. One is mirrored, so mirrored instances are
//! seen too.
//!
//! The model is a [`ModelHandle`]: the scene draws a primitive placeholder
//! (a post and a ball, the model's height) until the file has streamed in,
//! then the model. The handle picks the tier's file from
//! `models/stream.json` (`proof-figure.low.glb` on Low, the shared
//! `proof-figure.glb` elsewhere) and the texture set the device can
//! sample. A native run reads the manifest and the files from disk, so the
//! tests check the shipped assets. Nothing here touches the simulation.
//! `cargo xtask new-game` does not copy this file (it is the sandbox's
//! proof, not part of the template).

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::ErrorLog;
use evani_core::plugin::LookData;
use evani_render::model::{Frame, ModelHandle, ModelInstance, Models, Variant};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The part-variant proof (S3-11): the Evani.Model kart with its body
/// painted red and its rear wheels hidden, a crate on its item slot and a
/// barrel on each exhaust. Files `models/proof-{kart,crate,barrel}.glb`.
/// Drawn only once the base and all three parts have streamed in (a
/// variant never shows half built).
pub const VARIANT_JSON: &str = r#"{
  "base": "proof-kart",
  "hide": ["wheel_rl", "wheel_rr"],
  "paint": { "body": [0.9, 0.12, 0.08] },
  "parts": [
    { "model": "proof-crate", "anchor": "item_slot", "colour": [1.0, 0.75, 0.35] },
    { "model": "proof-barrel", "anchor": "exhaust.l", "colour": [0.35, 0.55, 1.0] },
    { "model": "proof-barrel", "anchor": "exhaust.r", "colour": [0.35, 0.55, 1.0] }
  ]
}"#;
/// Where the variant stands: by a grid corner (21 m out, so the orbit never cuts it), turned
/// towards the middle. The unmodified kart stands across the corner, to compare.
pub const VARIANT_AT: [f32; 3] = [8.0, 0.0, 19.0];
/// Where the unmodified kart stands (the variant's spot mirrored across the corner).
pub const PLAIN_AT: [f32; 3] = [19.0, 0.0, 8.0];
/// Their scale (the kart is about 3 m across).
pub const VARIANT_SCALE: f32 = 1.8;

/// The model's name (files `models/proof-figure[.<tier>].glb`).
pub const ID: &str = "proof-figure";
/// Distance of each copy from the grid's centre (m): just outside the
/// grid's half-width (15.9 m with a spinner's reach).
pub const DISTANCE: f32 = 18.6;
/// Scale of each copy (the model is 2.19 m tall, so about 5.3 m).
pub const SCALE: f32 = 2.4;
/// The placeholder's height (m): the scaled model's.
pub const PLACEHOLDER_H: f32 = 2.19 * SCALE;
/// Multiplied with the warm `stone` base (0.815, 0.776, 0.701): about 0.69
/// on every channel, a neutral light grey.
pub const PLACEHOLDER_TINT: [f32; 4] = [0.85, 0.89, 0.985, 1.0];

/// The proof's state.
#[derive(Debug)]
pub struct ProofModel {
    /// The handle, once asked for.
    pub handle: Option<ModelHandle>,
    /// The failure was logged.
    logged: bool,
    /// Native runs: hand the requested files over (tests turn it off to
    /// play a network that never answers).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub deliver: bool,
    /// Frames the placeholder was drawn.
    pub placeholder_frames: u32,
    /// The part-variant recipe ([`VARIANT_JSON`]), made on the first frame.
    pub variant: Option<Variant>,
    /// The variant's unresolved names were logged.
    variant_logged: bool,
    /// Frames the variant was drawn.
    pub variant_frames: u32,
    /// The kart as it comes, drawn beside the variant.
    plain: Option<ModelHandle>,
}

impl Default for ProofModel {
    fn default() -> Self {
        Self { handle: None, logged: false, deliver: true, placeholder_frames: 0, variant: None, variant_logged: false, variant_frames: 0, plain: None }
    }
}

/// `(x, z, yaw in radians, mirrored)` of each copy; the model faces +z.
pub fn placements() -> [(f32, f32, f32, bool); 4] {
    use core::f32::consts::{FRAC_PI_2, PI};
    [
        (0.0, DISTANCE, 0.0, false),
        (DISTANCE, 0.0, FRAC_PI_2, true),
        (0.0, -DISTANCE, PI, false),
        (-DISTANCE, 0.0, -FRAC_PI_2, false),
    ]
}

/// The `Stage::Frame` system: ask once, then draw the placeholder or the
/// four copies.
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_model", Stage::Frame, draw)
}

/// Native runs: the manifest and the requested files, from disk.
#[cfg(not(target_arch = "wasm32"))]
fn native_files(world: &World, models: &mut Models, first: bool, deliver: bool) {
    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/");
    if first {
        let path = format!("{DIR}stream.json");
        let m = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| evani_render::model::stream::manifest_from_text(&t));
        if let Err(e) = &m {
            world.res_mut::<ErrorLog>().error(format!("sandbox: {path}: {e}"));
        }
        models.set_manifest(m.ok());
    }
    if !deliver {
        return;
    }
    for (id, url) in models.take_requests() {
        let file = url.strip_prefix(evani_render::model::stream::FOLDER).unwrap_or(&url);
        let bytes = std::fs::read(format!("{DIR}{file}")).map_err(|e| format!("{file}: {e}"));
        match &bytes {
            Ok(b) => models.deliver(id, Ok(b)),
            Err(e) => models.deliver(id, Err(e.clone())),
        }
    }
}

/// The variant, nothing until it can be drawn whole. A recipe name the base
/// lacks is logged once as an error.
fn draw_variant(world: &World, models: &mut Models, proof: &mut ProofModel) {
    let Some(v) = proof.variant.as_mut() else { return };
    let (s, c) = (-3.0 * core::f32::consts::FRAC_PI_4 * 0.5).sin_cos();
    let at = Frame::new(VARIANT_AT).rotation([0.0, s, 0.0, c]).scale([VARIANT_SCALE; 3]);
    if !v.draw(models, at) {
        return;
    }
    proof.variant_frames = proof.variant_frames.saturating_add(1);
    if !v.unresolved.is_empty() && !proof.variant_logged {
        proof.variant_logged = true;
        world.res_mut::<ErrorLog>().error(format!("sandbox: variant names the proof kart lacks: {}", v.unresolved.join(", ")));
    }
    // The kart as it comes, to see what the recipe changed.
    let plain = *proof.plain.get_or_insert_with(|| models.handle("proof-kart"));
    if let Some(id) = models.resolve(plain) {
        models.draw(ModelInstance::at(id, &Frame { position: PLAIN_AT, ..at }));
    }
}

fn draw(world: &mut World) {
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let Some(mut proof) = world.try_res_mut::<ProofModel>() else { return };
    let first = proof.handle.is_none();
    let h = *proof.handle.get_or_insert_with(|| models.handle(ID));
    if first {
        match Variant::from_text(&mut models, VARIANT_JSON) {
            Ok(v) => proof.variant = Some(v),
            Err(e) => world.res_mut::<ErrorLog>().error(format!("sandbox: {e}")),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    native_files(world, &mut models, first, proof.deliver);
    #[cfg(target_arch = "wasm32")]
    let _ = first;
    draw_variant(world, &mut models, &mut proof);
    if models.handle_failed(h) && !proof.logged {
        proof.logged = true;
        world.res_mut::<ErrorLog>().error(format!("sandbox: proof model {ID} not drawn (see the error above); the placeholder stays"));
    }
    match models.resolve(h) {
        Some(id) => {
            for (x, z, yaw, mirrored) in placements() {
                let (s, c) = (yaw * 0.5).sin_cos();
                let sx = if mirrored { -SCALE } else { SCALE };
                models.draw(ModelInstance::new(id, [x, 0.0, z]).rotation([0.0, s, 0.0, c]).scale([sx, SCALE, SCALE]));
            }
        }
        None => {
            proof.placeholder_frames = proof.placeholder_frames.saturating_add(1);
            // The look's pale `stone`, cooled to a neutral light grey, so the
            // stand-in reads clearly against the green ground.
            let stone = world.try_res::<LookData>().map_or(0, |l| RenderWorld::material(&l, "stone"));
            let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
            let head = PLACEHOLDER_H * 0.24;
            for (x, z, _, _) in placements() {
                let mut body = MeshInstance::new(MeshId::CYLINDER, [x, (PLACEHOLDER_H - head) * 0.5, z]);
                body.scale = [1.4, PLACEHOLDER_H - head, 1.4];
                body.material = stone;
                body.color = PLACEHOLDER_TINT;
                rw.draw_mesh(body);
                let mut ball = MeshInstance::new(MeshId::SPHERE, [x, PLACEHOLDER_H - head * 0.5, z]);
                ball.scale = [head; 3];
                ball.material = stone;
                ball.color = PLACEHOLDER_TINT;
                rw.draw_mesh(ball);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::RenderStats;

    /// The shipped proof file streams in through the handle (placeholder
    /// first), prepares with tangents and a normal map, lists only the base
    /// and normal maps with their ASTC and BC7 sets, and its four copies are
    /// counted in the frame's draws.
    #[test]
    fn proof_model_is_drawn_with_its_normal_map() {
        let mut app = crate::build_app();
        app.run_headless(4, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let proof = app.world.res::<ProofModel>();
        assert!(proof.placeholder_frames >= 1, "the placeholder is drawn first");
        let h = proof.handle.unwrap();
        assert!(app.world.res::<Models>().resolve(h).is_some(), "the model replaced the placeholder");
        let stats = app.world.res::<RenderStats>();
        assert!(stats.triangles >= 4 * 19_000, "{}", stats.triangles);

        let glb = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/proof-figure.glb")).unwrap();
        let g = evani_gltf::Gltf::from_glb(&glb).unwrap();
        let p = evani_render::model::prepare(&g).unwrap();
        assert!(p.parts.iter().all(|x| x.tangents), "every part has tangents");
        assert!(p.materials.iter().any(|m| m.maps[1].is_some()), "a normal map is sampled");
        // The lit surface pass (evani-render `look-surface`, pulled in by `lit-proof` and so by
        // `layers-proof` and `char-proof`) also lists the metallic-roughness map: 3 textures, else 2.
        if cfg!(feature = "lit-proof") {
            assert_eq!(p.textures.len(), 3, "base, normal and the lit pass's metallic-roughness");
            assert!(!p.textures[2].srgb, "the third texture is metallic-roughness (linear data)");
        } else {
            assert_eq!(p.textures.len(), 2, "base and normal only: metallic-roughness is not listed without the lit pass");
        }
        // Only base and normal ship ASTC and BC7 sets; the lit pass's metallic-roughness has none.
        if cfg!(feature = "lit-proof") {
            assert!(p.textures[2].sets.is_empty(), "metallic-roughness lists no shipped sets");
        }
        for t in &p.textures[..2] {
            let mut sets: Vec<&str> = t.sets.iter().map(|s| s.0.as_str()).collect();
            sets.sort_unstable();
            assert_eq!(sets, ["astc", "bc7"]);
            for (_, uri) in &t.sets {
                let f = format!("{}/assets/models/{uri}", env!("CARGO_MANIFEST_DIR"));
                assert!(std::path::Path::new(&f).is_file(), "{f}");
            }
        }
    }

    /// The shipped variant recipe names only what the kart has, and once the
    /// files have streamed in it draws the base and the three parts (one
    /// draw each), after drawing nothing while they were missing.
    #[test]
    fn variant_proof_draws_the_kart_and_three_parts() {
        let mut app = crate::build_app();
        app.run_headless(4, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "no unresolved name was logged");
        assert!(app.world.res::<ProofModel>().variant_frames >= 1, "the variant was drawn");
        let mut v = app.world.res_mut::<ProofModel>().variant.take().unwrap();
        assert!(v.unresolved.is_empty(), "{:?}", v.unresolved);
        let mut models = app.world.res_mut::<Models>();
        assert!(models.draws.is_empty(), "the frame consumed its draws");
        assert!(v.draw(&mut models, Frame::new(VARIANT_AT)));
        assert_eq!(models.draws.len(), 4, "the kart, the crate and two barrels");    }

    /// Models are render-only: the same run with the files delivered and
    /// with a network that never answers (placeholder throughout) leaves the
    /// simulation in the same state.
    #[test]
    fn sim_is_identical_with_and_without_streamed_models() {
        let run = |deliver: bool| {
            let mut app = crate::build_app();
            app.world.res_mut::<ProofModel>().deliver = deliver;
            app.run_headless(120, 1000.0 / 60.0);
            let streamed = {
                let proof = app.world.res::<ProofModel>();
                app.world.res::<Models>().resolve(proof.handle.unwrap()).is_some()
            };
            let t = app.world.res::<evani_core::time::Time>().sim_seconds;
            let angles = evani_kit_example::spinner_angles(&app.world, t);
            let rng = app.world.try_res::<evani_core::rng::Pcg32>().map(|r| r.clone());
            let ticks = app.world.res::<evani_core::time::Time>().tick;
            (streamed, (t, ticks, angles, rng))
        };
        let (with, a) = run(true);
        let (without, b) = run(false);
        assert!(with && !without, "one run streamed the model, the other drew the placeholder");
        assert_eq!(a, b);
    }

    /// Both karts stay whole in the 16:9 orbit's frame at every point of
    /// the turn and the bob (a 3 m-radius, 3 m-tall bound round each: the
    /// kart is about 5.5 x 3 m at its scale), and clear of the grid.
    #[test]
    fn karts_stay_whole_in_the_orbit_frame() {
        let path = evani_kit_example::draw::camera_path();
        for at in [VARIANT_AT, PLAIN_AT] {
            assert!(at[0].abs().min(at[2].abs()) - 3.0 >= 4.9 && at[0].abs().max(at[2].abs()) - 3.0 >= 15.9, "{at:?} clear of the grid and the marshals");
            for step in 0..1200 {
                let pose = path.pose(f64::from(step) * 0.05);
                for k in 0u8..16 {
                    let a = f32::from(k) * core::f32::consts::FRAC_PI_8;
                    for y in [0.0, 3.0] {
                        let p = [at[0] + 3.0 * a.cos(), y, at[2] + 3.0 * a.sin()];
                        let (x, v) = evani_kit_example::draw::ndc(&pose, p, 16.0 / 9.0);
                        assert!(x.abs() < 1.0 && v.abs() < 1.0, "kart at {at:?} leaves the frame at t {}: {p:?} -> ({x}, {v})", f64::from(step) * 0.05);
                    }
                }
            }
        }
    }
}
