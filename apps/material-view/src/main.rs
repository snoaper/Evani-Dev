//! Evani.Dev material-view: a game for viewing and tuning materials, and the
//! in-engine home of Evani.Materials. Which material families exist is
//! decided at build time by evani.toml (`[modules] materials`, each the
//! feature `material-<family>`, see Cargo.toml); this file only adds what was
//! compiled in.
//!
//! * The kit (kits/evani-kit-material-view) holds the data, the viewer's
//!   state and commands, the orbit camera, the keys and the touch controls.
//! * `scenes/` draws each family's scenes, one module per family.
//! * `web.rs` (wasm only) is the page: the scene row, the browser drawer,
//!   the tuning panel built from the registered params, quality, lights and
//!   compare.
//! * Native (`cargo run -p evani-material-view`): a headless run of 300
//!   frames that prints the overlay, so the wiring can be checked without a
//!   browser.

mod panel;
#[cfg(feature = "material-catalogue")]
mod masks;
mod scenes;
#[cfg(feature = "material-styles")]
mod shapes;
#[cfg(feature = "material-studio")]
mod studio;
#[cfg(feature = "material-styles")]
mod treat;
#[cfg(target_arch = "wasm32")]
mod web;

use evani_core::App;
use evani_core::play::PlayMode;
use evani_kit_material_view::{MaterialViewKitPlugin, Viewer};
use evani_look_stylizedpbr::film::FilmLook;
use evani_render::RenderModule;

/// The material families compiled into this build.
pub const FAMILIES: &[&str] = &[
    #[cfg(feature = "material-styles")]
    "styles",
    #[cfg(feature = "material-layers")]
    "layers",
    #[cfg(feature = "material-characters")]
    "characters",
    #[cfg(feature = "material-nature")]
    "nature",
    #[cfg(feature = "material-playground")]
    "playground",
    #[cfg(feature = "material-catalogue")]
    "catalogue",
    #[cfg(feature = "material-studio")]
    "studio",
];

/// Build the material-view app with every module this build was compiled with.
pub fn build_app() -> App {
    build_app_in(PlayMode::default())
}

/// The same in a given play mode (a viewer plays the same in both).
pub fn build_app_in(mode: PlayMode) -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("material-view").with_play_mode(mode).with_module(RenderModule::new());
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    #[cfg(feature = "material-nature")]
    {
        app = app.with_module(evani_material_nature::NatureModule::new());
    }
    // The procedural library (evani.toml `procedural = [..]`): its params,
    // the placed assets and the grass it hosts; the nature pass is the
    // viewer's own when the nature family is on.
    #[cfg(feature = "procedural")]
    {
        let m = evani_procedural_lib::module();
        #[cfg(feature = "material-nature")]
        let m = m.hosted_elsewhere(&["nature"]);
        app = app.with_module(m);
    }
    #[cfg(feature = "live")]
    {
        app = app.with_module(evani_live::LiveModule::new());
    }
    #[allow(unused_mut)]
    let mut app = app.with_kit(MaterialViewKitPlugin).with_look(FilmLook);
    // The playground surfaces' world and decal slots and the sky dome come
    // from the photoreal look's daylight preset, layered over the film look.
    #[cfg(feature = "material-playground")]
    {
        app.add_look(&evani_look_photoreal::daylight::DaylightLook);
        // The daylight preset vetoes bloom (its dusty daylight has nothing
        // to glow), and as the later look its veto won: the viewer drew no
        // bloom on any tier, so the night lamp's shade and every glowing
        // material lit nothing round them (critic round 2). The film look
        // is the viewer's look and asks for bloom; its wish is put back
        // last, and the tier still decides where bloom starts.
        app.world.res_mut::<evani_core::plugin::LookData>().post_effects.push(("bloom".to_string(), true));
    }
    add_stage_materials(&mut app);
    for f in FAMILIES {
        Viewer::enable(&mut app.world, f);
    }
    let mut game = app.context("material-view");
    // The art styles' params, data and sync system (the kit's); a bad
    // styles file is logged and the family shows nothing.
    #[cfg(feature = "material-styles")]
    if let Err(e) = evani_kit_material_view::styles::install(&mut game) {
        game.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: styles: {}", e.0));
    }
    // The catalogue's library, switches and sync system (the kit's).
    #[cfg(feature = "material-catalogue")]
    if let Err(e) = evani_kit_material_view::catalogue::install(&mut game) {
        game.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: catalogue: {}", e.0));
    }
    // The Asset & Style Studio's record, catalogues and command system (the
    // kit's), then the app's studio parts (src/studio/).
    #[cfg(feature = "material-studio")]
    if let Err(e) = evani_kit_material_view::studio::install(&mut game) {
        game.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: studio: {}", e.0));
    }
    #[cfg(feature = "material-studio")]
    studio::install(&mut game);
    scenes::install(&mut game);
    #[cfg(feature = "material-studio")]
    studio::install_late(&mut game);
    #[cfg(feature = "live")]
    live_review(&mut game);
    app
}

/// Live builds only (never a shipped download): the target pack's capture
/// moments, a camera path at the scenes' size for the bench and the
/// world-edge renders, and every streamed model asked for at start, so a
/// moment's four frames never show a placeholder.
#[cfg(feature = "live")]
fn live_review(game: &mut evani_core::plugin::PluginContext) {
    let mut moments = evani_core::capture::CaptureMoments::default();
    match evani_kit_material_view::moments::register_capture_moments(&mut moments) {
        Ok(()) => game.resource(moments),
        Err(e) => game.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: capture moments: {e}")),
    }
    // Round the stage at the scenes' own distance and height.
    game.resource(evani_core::capture::BenchPath(evani_core::camera::CameraPath::orbit([0.0, 0.5, 0.0], 5.5, 2.2, 40.0)));
    #[cfg(feature = "models")]
    {
        let mut asked = false;
        game.system(evani_core::ecs::SystemDesc::new("material_view.warm_models", evani_core::ecs::Stage::Frame, move |w: &mut evani_core::ecs::World| {
            if !asked {
                asked = warm_models(w);
            }
        }));
    }
}

/// Every model a scene streams (`assets/models`, by id).
#[cfg(all(feature = "live", feature = "models"))]
pub const STREAMED_MODELS: [&str; 7] = ["char-head", "layer-velvet", "layer-coat", "layer-brushed", "layer-bubble", "monkey", "teapot"];

/// Ask for every streamed model (the handles are shared with the scenes,
/// which ask for the same ids); true once asked.
#[cfg(all(feature = "live", feature = "models"))]
fn warm_models(world: &evani_core::ecs::World) -> bool {
    let Some(mut models) = world.try_res_mut::<evani_render::model::Models>() else { return false };
    for id in STREAMED_MODELS {
        let _ = models.handle(id);
    }
    true
}

/// The renderer's material slots (evani-render `MAX_MATERIALS`): a slot past
/// the last draws as the last.
const RENDER_SLOTS: usize = 16;

/// The stage surfaces the scenes need that the looks lack (from the kit's
/// lights.json). Without them a missing name falls back to the look's first
/// material (fur), which tinted every floor brown. The film and daylight
/// looks together bring 27 materials, so they go in just below the
/// renderer's 16th slot, not at the end where they would draw as slot 15;
/// the slots below them keep their numbers, and everything here finds its
/// material by name each frame.
fn add_stage_materials(app: &mut App) {
    use evani_core::plugin::{LookData, MaterialDesc};
    let Some(data) = app.world.try_res::<evani_kit_material_view::ViewerData>().map(|d| d.0.clone()) else {
        return;
    };
    let mut look = app.world.res_mut::<LookData>();
    let missing: Vec<MaterialDesc> = data
        .lights
        .stage_materials
        .iter()
        .filter(|s| look.material(&s.name).is_none())
        .map(|s| {
            let mut m = MaterialDesc::new(&s.name, s.base_color);
            m.emissive = s.emissive;
            m.roughness = 0.85;
            m
        })
        .collect();
    let at = look.materials.len().min(RENDER_SLOTS.saturating_sub(missing.len()));
    look.materials.splice(at..at, missing);
}

#[cfg(target_arch = "wasm32")]
fn main() {
    evani_core::web::run(web::install(build_app()));
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut app = build_app();
    app.run_headless(300, 1000.0 / 60.0);
    let overlay = app.world.res::<evani_core::overlay::Overlay>();
    println!("{}", overlay.text());
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::ErrorLog;
    use evani_core::params::ParamRegistry;
    use evani_core::plugin::BuildInfo;
    use evani_kit_material_view::{Command, ViewerData};

    #[test]
    fn modules_match_features() {
        let mut app = build_app();
        app.run_headless(5, 1000.0 / 60.0);
        let info = app.world.res::<BuildInfo>();
        let has = |l: &str| info.modules.iter().any(|(_, label)| *label == l);
        assert!(has("render"));
        assert_eq!(has("fx"), cfg!(feature = "fx"));
        assert_eq!(has("live"), cfg!(feature = "live"));
        assert_eq!(info.kit.as_deref(), Some("evani-kit-material-view"));
        let reg = app.world.res::<ParamRegistry>();
        assert_eq!(reg.id("fx.bloom.enabled").is_some(), cfg!(feature = "bloom"));
        // Bloom starts on where the tier starts it (the daylight layer's
        // veto must not switch it off for the whole viewer).
        if let Some(id) = reg.id("fx.bloom.enabled") {
            let tier_has = app.world.res::<evani_core::tiers::Tiers>().active().effects_on.iter().any(|e| e == "bloom");
            assert_eq!(reg.value(id), Some(evani_core::params::ParamValue::Bool(tier_has)), "bloom follows the tier");
        }
        assert!(reg.id("kit.material_view.orbit_speed").is_some());
        // Each family's params exist exactly when the family is built.
        assert_eq!(reg.id("material.layers.sheen_strength").is_some(), cfg!(feature = "material-layers"));
        assert_eq!(reg.id("material.character.quality").is_some(), cfg!(feature = "material-characters"));
        assert_eq!(reg.id("material.nature.water.clarity").is_some(), cfg!(feature = "material-nature"));
        assert_eq!(reg.id("material.relief.wetness").is_some(), cfg!(feature = "material-playground"));
    }

    /// Every material name the scenes ask for is its own slot (a missing
    /// name falls back to slot 0, the look's fur), within the renderer's 16
    /// (a later slot draws as the 16th).
    #[test]
    fn stage_materials_have_their_own_slots() {
        use evani_core::plugin::LookData;
        let app = build_app();
        let look = app.world.res::<LookData>();
        let mut names = vec!["ground", "accent", "glow", "stone"];
        if cfg!(feature = "material-playground") {
            names.extend(["world", "decal", "sky"]);
        }
        for name in names {
            let at = look.materials.iter().position(|m| m.name == name);
            assert!(matches!(at, Some(i) if i > 0 && i < RENDER_SLOTS), "{name} at {at:?}");
        }
        let glow = look.material("glow").map_or(0.0, |m| m.emissive[0]);
        assert!(glow > 1.0, "the bulb glows: {glow}");
    }

    /// No effect the film look asks for is switched off by a layered look
    /// (the daylight preset vetoed bloom for the whole viewer once).
    #[test]
    fn no_layered_look_vetoes_the_film_looks_effects() {
        use evani_core::plugin::LookData;
        let app = build_app();
        let vetoed = app.world.res::<LookData>().vetoed_effects().iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert!(vetoed.is_empty(), "vetoed by a later look: {vetoed:?}");
    }

    #[test]
    fn the_viewer_knows_the_compiled_families() {
        let app = build_app();
        let v = app.world.res::<Viewer>();
        assert_eq!(v.families_on, FAMILIES);
    }

    /// Every scene this build has opens, draws and logs no error.
    #[test]
    fn every_scene_opens_and_draws() {
        let mut app = build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let data = app.world.res::<ViewerData>().0.clone();
        for s in &data.library.scenes {
            if !FAMILIES.contains(&s.family.as_str()) {
                continue;
            }
            app.world.res_mut::<Viewer>().push(Command::Scene(s.id.clone()));
            app.run_headless(6, 1000.0 / 60.0);
            assert_eq!(app.world.res::<Viewer>().scene, s.id, "{} opened", s.id);
            let stats = app.world.res::<evani_core::overlay::RenderStats>();
            assert!(stats.draw_calls >= 1, "{}: {} draws", s.id, stats.draw_calls);
            drop(stats);
            assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{}: {:?}", s.id, app.world.res::<ErrorLog>());
        }
    }

    /// Every capture moment opens its scene with its light, the turntable as
    /// it says and the home camera (zoomed for the close-up), draws, and logs
    /// no error, whatever was open before.
    #[cfg(feature = "live")]
    #[test]
    fn every_capture_moment_stages_its_scene() {
        use evani_kit_material_view::moments::MOMENTS;
        let mut app = build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let list = app.world.res::<evani_core::capture::CaptureMoments>().list().to_vec();
        #[cfg(feature = "material-catalogue")]
        let proofs = evani_kit_material_view::moments::PROOFS.len() + evani_kit_material_view::moments::HELD.len();
        #[cfg(not(feature = "material-catalogue"))]
        let proofs = 0;
        assert_eq!(list.len(), MOMENTS.len() + proofs);
        let data = app.world.res::<ViewerData>().0.clone();
        for (cm, m) in list.iter().zip(MOMENTS.iter()) {
            assert_eq!(cm.name, m.id);
            let family = data.scene(m.scene).map(|s| s.family.clone()).unwrap_or_default();
            if !FAMILIES.contains(&family.as_str()) {
                assert!((cm.setup)(&app.world).is_err(), "{}: a family this build lacks says so", m.id);
                continue;
            }
            // Something else open first: the moment must not depend on it.
            app.world.res_mut::<Viewer>().push(Command::NextScene);
            app.run_headless(2, 1000.0 / 60.0);
            (cm.setup)(&app.world).unwrap_or_else(|e| panic!("{}: {e}", m.id));
            app.run_headless(4, 1000.0 / 60.0);
            let v = app.world.res::<Viewer>().clone();
            assert_eq!(v.scene, m.scene, "{}", m.id);
            assert_eq!((v.light, v.spin, v.compare), (m.light, m.spin, false), "{}", m.id);
            // Only the head close-up starts from the scene's close camera; the
            // head moment shows the whole bust on its plinth (critic round 8).
            let close = data.scene(m.scene).and_then(|s| s.close).filter(|_| m.id == "head_close");
            let base = close.map_or(v.home.dist, |c| c.dist);
            assert!((v.orbit.dist - (base * m.zoom).clamp(v.home.min_dist, v.home.max_dist)).abs() < 1e-4, "{}", m.id);
            assert!(app.world.res::<evani_core::overlay::RenderStats>().draw_calls >= 1, "{}: draws", m.id);
            assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{}: {:?}", m.id, app.world.res::<ErrorLog>());
        }
        // The proofs: each opens its catalogue row in the art styles scene,
        // the panels closed.
        #[cfg(feature = "material-catalogue")]
        for (cm, (id, row)) in list[MOMENTS.len()..].iter().zip(evani_kit_material_view::moments::PROOFS) {
            assert_eq!(cm.name, id);
            (cm.setup)(&app.world).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(app.world.res::<Viewer>().panels, Some((false, false)), "{id}");
            app.run_headless(4, 1000.0 / 60.0);
            assert_eq!(app.world.res::<Viewer>().scene, "styles", "{id}");
            let lib = app.world.res::<evani_kit_material_view::catalogue::LibraryData>().0.clone();
            let open = app.world.res::<evani_kit_material_view::catalogue::CatalogueView>().open;
            assert_eq!(open.map(|i| lib.cat.rows[i].id.clone()).as_deref(), Some(row), "{id}");
            assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{id}: {:?}", app.world.res::<ErrorLog>());
        }
        // The held proofs: the art styles scene, no row open (a planned row
        // cannot), the library asked open and searched for the row.
        #[cfg(feature = "material-catalogue")]
        for (cm, (id, _, search)) in list[MOMENTS.len() + evani_kit_material_view::moments::PROOFS.len()..].iter().zip(evani_kit_material_view::moments::HELD) {
            assert_eq!(cm.name, id);
            (cm.setup)(&app.world).unwrap_or_else(|e| panic!("{id}: {e}"));
            let v = app.world.res::<Viewer>().clone();
            assert_eq!((v.panels, v.lib_search.as_deref()), (Some((true, false)), Some(search)), "{id}");
            app.run_headless(4, 1000.0 / 60.0);
            assert_eq!(app.world.res::<Viewer>().scene, "styles", "{id}");
            assert!(app.world.res::<evani_kit_material_view::catalogue::CatalogueView>().open.is_none(), "{id}");
            assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{id}: {:?}", app.world.res::<ErrorLog>());
        }
    }

    /// The geometry check's scene dump (`cargo xtask geocheck material-view`
    /// runs this with `EVANI_GEOCHECK_DUMP` set to a folder; otherwise it
    /// does nothing): every scene of the target pack's moments, staged as the
    /// moment does, with what it draws (meshes, decals and each model's
    /// highest-detail triangles in model space), its camera and four views
    /// round its home orbit, written to `<folder>/dump.json`.
    #[cfg(all(feature = "live", feature = "models", feature = "material-styles"))]
    #[test]
    fn geocheck_dump() {
        use evani_kit_material_view::moments::MOMENTS;
        use evani_render::RenderWorld;
        use evani_render::model::Models;
        use serde_json::{Value, json};
        let Some(dir) = std::env::var_os("EVANI_GEOCHECK_DUMP") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let materials: Vec<String> = app.world.res::<evani_core::plugin::LookData>().materials.iter().map(|m| m.name.clone()).collect();
        let list = app.world.res::<evani_core::capture::CaptureMoments>().list().to_vec();
        let pose = |p: &evani_core::camera::CameraPose| json!({ "eye": p.eye, "target": p.target, "up": p.up, "fov_y_deg": p.fov_y_deg });
        // The draw lists are read by a system last in the frame (the renderer
        // empties them once the frame is drawn), on the frame asked for.
        type Snap = (Vec<Value>, usize, Vec<(evani_render::model::ModelId, [f32; 3], [f32; 4], [f32; 3])>, evani_core::camera::CameraPose);
        let want = std::rc::Rc::new(std::cell::Cell::new(false));
        let snap: std::rc::Rc<std::cell::RefCell<Option<Snap>>> = std::rc::Rc::new(std::cell::RefCell::new(None));
        {
            let (want, snap) = (want.clone(), snap.clone());
            app.context("geocheck").system(
                evani_core::ecs::SystemDesc::new("geocheck.snapshot", evani_core::ecs::Stage::Frame, move |w: &mut evani_core::ecs::World| {
                    if !want.get() {
                        return;
                    }
                    want.set(false);
                    let (Some(rw), Some(models)) = (w.try_res::<RenderWorld>(), w.try_res::<Models>()) else { return };
                    let inst = |i: &evani_render::MeshInstance| json!([i.mesh.0, i.position, i.rotation, i.scale, i.material, i.color]);
                    let mut meshes: Vec<Value> = rw.meshes.iter().map(inst).collect();
                    let decal_from = meshes.len();
                    meshes.extend(rw.decals.iter().map(inst));
                    let draws = models.draws.iter().map(|d| (d.model, d.position, d.rotation, d.scale)).collect();
                    *snap.borrow_mut() = Some((meshes, decal_from, draws, rw.camera));
                })
                .order(i32::MAX),
            );
        }
        let mut shapes = serde_json::Map::new();
        let mut scenes = Vec::new();
        for (cm, m) in list.iter().zip(MOMENTS.iter()) {
            if m.spin {
                continue;
            }
            (cm.setup)(&app.world).unwrap_or_else(|e| panic!("{}: {e}", m.id));
            // Frames for the streamed models to arrive (fed from the game's
            // own folder, as the browser streams them) and build.
            for k in 0..41 {
                {
                    let mut models = app.world.res_mut::<Models>();
                    scenes::native_stream(&app.world, &mut models);
                }
                want.set(k == 40);
                app.run_headless(1, 1000.0 / 60.0);
            }
            let (meshes, decal_from, draws, camera) = snap.borrow_mut().take().unwrap_or_else(|| panic!("{}: no snapshot", m.id));
            assert!(!meshes.is_empty() || !draws.is_empty(), "{}: the scene drew nothing", m.id);
            let models = app.world.res::<Models>();
            let mut drawn = Vec::new();
            for (id, position, rotation, scale) in draws {
                let url = models.url(id).unwrap_or("").to_string();
                let key = format!("{}#{}", url, id.0);
                if !shapes.contains_key(&key) {
                    let bytes = models.glb_bytes(id).map(<[u8]>::to_vec).or_else(|| {
                        let file = url.rsplit('/').next().unwrap_or("");
                        std::fs::read(format!("{}/assets/models/{file}", env!("CARGO_MANIFEST_DIR"))).ok()
                    });
                    let Some(bytes) = bytes else { continue };
                    shapes.insert(key.clone(), lod0_triangles(&bytes));
                }
                drawn.push(json!({ "shape": key, "position": position, "rotation": rotation, "scale": scale }));
            }
            drop(models);
            let v = app.world.res::<Viewer>();
            let views: Vec<Value> = (0..4).map(|k| pose(&v.home.pose(k as f32 * 90.0))).collect();
            scenes.push(json!({
                "id": m.id, "scene": m.scene, "camera": pose(&camera), "views": views,
                "meshes": meshes, "decal_from": decal_from, "models": drawn,
            }));
        }
        let out = json!({ "app": "material-view", "materials": materials, "scenes": scenes, "shapes": shapes });
        std::fs::write(dir.join("dump.json"), serde_json::to_string(&out).unwrap()).unwrap();
    }

    /// A model's highest-detail triangles (the nodes under its first LOD,
    /// or every mesh node), in model space, one part per mesh node:
    /// `{"parts": [{"positions": [...], "indices": [...]}]}`.
    #[cfg(all(feature = "live", feature = "models", feature = "material-styles"))]
    fn lod0_triangles(glb: &[u8]) -> serde_json::Value {
        let Ok(g) = evani_gltf::Gltf::from_glb(glb) else { return serde_json::json!({ "parts": [] }) };
        let json: serde_json::Value = evani_gltf::split_glb(glb).ok().and_then(|(j, _)| serde_json::from_slice(j).ok()).unwrap_or_default();
        let lod0 = json.pointer("/extras/evani/lods/0/node").and_then(|n| n.as_str()).and_then(|n| g.node_by_name(n));
        let under = |mut n: usize| loop {
            if Some(n) == lod0 {
                return true;
            }
            match g.parent(n) {
                Some(p) => n = p,
                None => return lod0.is_none(),
            }
        };
        let mats = g.world_matrices();
        let mut parts = Vec::new();
        for (ni, node) in g.nodes.iter().enumerate() {
            let Some(mi) = node.mesh else { continue };
            if !under(ni) {
                continue;
            }
            // One part per mesh node (skin, eyes, hair are separate pieces),
            // named by its first material.
            let (mut pos, mut idx): (Vec<f32>, Vec<u32>) = (Vec::new(), Vec::new());
            let material = g.meshes[mi].primitives.first().and_then(|p| p.material).and_then(|m| g.materials.get(m)).map_or(String::new(), |m| m.name.clone());
            for prim in &g.meshes[mi].primitives {
                let Some(pa) = prim.attribute("POSITION") else { continue };
                let p = g.read_f32(pa).unwrap_or_default();
                let base = (pos.len() / 3) as u32;
                for c in p.as_chunks::<3>().0 {
                    pos.extend(evani_gltf::transform_point(&mats[ni], [c[0], c[1], c[2]]));
                }
                let n = (p.len() / 3) as u32;
                let ix = prim.indices.and_then(|i| g.read_u32(i).ok()).unwrap_or_else(|| (0..n).collect());
                idx.extend(ix.into_iter().map(|i| i + base));
            }
            parts.push(serde_json::json!({ "positions": pos, "indices": idx, "material": material }));
        }
        serde_json::json!({ "parts": parts })
    }

    #[test]
    fn overlay_lists_modules() {
        let mut app = build_app();
        app.run_headless(60, 1000.0 / 60.0);
        let text = app.world.res::<evani_core::overlay::Overlay>().text().to_string();
        assert!(text.contains("render"), "{text}");
    }

    /// A viewer has no fail state: free roam plays the same
    /// (docs/PLAYGROUNDS.md). Half a minute of keys through every scene and
    /// light with every family this build has.
    #[test]
    fn free_roam_through_every_scene() {
        use evani_core::play::{ScriptKey, free_roam_test};
        let mut app = build_app_in(evani_core::play::PlayMode::FreeRoam);
        let mut script = Vec::new();
        for i in 0..8 {
            let at = 1.0 + f64::from(i) * 3.5;
            for (k, key) in ["KeyN", "ArrowLeft", "KeyL", "KeyT", "KeyR"].iter().enumerate() {
                let t = at + k as f64 * 0.6;
                script.push(ScriptKey::down(t, key));
                script.push(ScriptKey::up(t + 0.3, key));
            }
        }
        let r = free_roam_test(&mut app, 30.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
    }

    /// A tier change rebuilds no shader (docs/KIT_PROCESS.md item 30): every
    /// tier switch keeps the slot bodies (the character constants are data).
    #[test]
    fn tier_switches_rebuild_no_shader() {
        let mut app = build_app();
        app.start();
        evani_core::tier_switch::tier_switch_test(&mut app, 3).unwrap();
    }
}
