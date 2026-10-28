//! Evani.Dev sandbox: the test game. Which engine modules exist is decided at
//! build time by evani.toml (through Cargo features, see Cargo.toml); this file
//! only adds what was compiled in.
//!
//! * Browser (`trunk serve`, `cargo xtask build sandbox`): the rAF runner with
//!   the overlay and the minimal debug panel (tier switch, effect toggles).
//! * Native (`cargo run -p evani-sandbox`): a headless run of 300 frames that
//!   prints the overlay, so the wiring can be checked without a browser.

mod panel; #[cfg(feature = "text-proof")] mod textpanel; #[cfg(feature = "ui")] mod hudpanel; #[cfg(feature = "relief")] mod proof_relief;
#[cfg(feature = "models")] // proof-model:line (new-game drops it)
mod proof_model; #[cfg(feature = "skin-proof")] mod proof_skin; // proof-model:line
mod scene; #[cfg(feature = "pseudo3d-proof")] mod proof_road; #[cfg(all(test, feature = "importance"))] mod importance_tests; #[cfg(feature = "collide-proof")] mod proof_collide; #[cfg(feature = "combat-proof")] mod proof_combat; #[cfg(feature = "render-limits-before")] mod proof_limits; #[cfg(feature = "material-proof")] mod proof_material; #[cfg(feature = "destruct-proof")] mod proof_destruct; #[cfg(feature = "layers-before")] mod proof_layers; #[cfg(feature = "char-proof")] mod proof_character; #[cfg(feature = "nature-proof")] mod proof_nature; #[cfg(feature = "playmat-proof")] mod proof_playmat;

use evani_core::App;
use evani_kit_example::ExampleKitPlugin;
use evani_look_basic::BasicLook;
use evani_render::RenderModule;

/// Build the sandbox app with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("sandbox").with_module(RenderModule::new()); #[cfg(all(feature = "collide-proof", not(any(feature = "physics-simple", feature = "physics-rigid"))))] { app = app.with_module(evani_physics::PhysicsModule::new()); } #[cfg(all(feature = "combat-proof", not(any(feature = "collide-proof", feature = "physics-simple", feature = "physics-rigid"))))] { app = app.with_module(evani_physics::PhysicsModule::new()); } #[cfg(feature = "combat-proof")] { app = app.with_module(evani_combat::CombatModule::new()); } #[cfg(feature = "nature-proof")] { app = app.with_module(evani_material_nature::NatureModule::new()); }
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    #[cfg(any(feature = "physics-simple", feature = "physics-rigid"))]
    {
        app = app.with_module(evani_physics::PhysicsModule::new());
    }
    #[cfg(feature = "net")]
    {
        app = app.with_module(evani_net::NetModule::new());
    }
    #[cfg(feature = "xr")]
    {
        app = app.with_module(evani_xr::XrModule::new());
    }
    #[cfg(feature = "live")]
    {
        app = app.with_module(evani_live::LiveModule::new());
    }
    let mut app = app
        .with_kit(ExampleKitPlugin)
        .with_look(BasicLook)
        .with_module(panel::DebugPanel::new()); #[cfg(feature = "toon")] app.add_look(&evani_look_toon::ToonLook); #[cfg(feature = "lit-proof")] app.add_look(&evani_look_stylizedpbr::film::FilmLook); #[cfg(feature = "render-limits-before")] app.add_look(&proof_limits::ProofLook); #[cfg(feature = "material-proof")] app.add_look(&evani_look_photoreal::daylight::DaylightLook); #[cfg(feature = "destruct")] app.add_module(Box::new(evani_destruct::DestructModule::new())); #[cfg(feature = "playmat-proof")] app.add_look(&evani_look_photoreal::daylight::DaylightLook);
    // The scene: a 20 x 20 spinner grid (see scene.rs) plus sprites.
    let mut game = app.context("sandbox");
    game.baked_params(r#"{"kit.example.count":400}"#);
    game.system(scene::system()); #[cfg(feature = "text-proof")] game.system(textpanel::system()); #[cfg(all(feature = "ui", not(feature = "text-proof")))] game.system(hudpanel::system()); #[cfg(feature = "pseudo3d-proof")] game.system(proof_road::system()); #[cfg(feature = "collide-proof")] { game.resource(proof_collide::ProofCollide::default()); game.system(proof_collide::walk_system()); game.system(proof_collide::draw_system()); } #[cfg(feature = "combat-proof")] { game.resource(proof_combat::ProofCombat::default()); game.system(proof_combat::fight_system()); game.system(proof_combat::draw_system()); } #[cfg(feature = "destruct-proof")] game.system(proof_destruct::system());
    // proof-model:begin (the textured model proof; new-game drops it)
    #[cfg(feature = "models")]
    {
        game.resource(proof_model::ProofModel::default());
        game.system(proof_model::system());
    } #[cfg(feature = "relief")] { game.resource(proof_relief::ProofRelief::default()); game.system(proof_relief::system()); } #[cfg(feature = "skin-proof")] { game.resource(proof_skin::ProofSkin::default()); game.system(proof_skin::system()); } #[cfg(feature = "render-limits-before")] game.system(proof_limits::system()); #[cfg(feature = "material-proof")] proof_material::install(&mut game); #[cfg(feature = "layers-before")] game.system(proof_layers::system()); #[cfg(feature = "char-proof")] { game.resource(proof_character::ProofCharacter::default()); game.system(proof_character::system()); } #[cfg(feature = "nature-proof")] proof_nature::install(&mut game); #[cfg(feature = "playmat-proof")] proof_playmat::install(&mut game);
    // proof-model:end
    // Live's bench.run / compare.ab / capture.frame fly the scene's own path.
    #[cfg(feature = "live")]
    game.resource(evani_core::capture::BenchPath(scene::camera_path()));
    // capture.frame { moment } stages the example kit's signature moments.
    #[cfg(feature = "live")]
    {
        let mut moments = evani_core::capture::CaptureMoments::default();
        match scene::moments::register(&mut moments) {
            Ok(()) => {
                app.context("sandbox").resource(moments);
            }
            Err(e) => {
                app.world
                    .res_mut::<evani_core::overlay::ErrorLog>()
                    .error(format!("sandbox: capture moments: {e}"));
            }
        }
    }
    app
}

#[cfg(target_arch = "wasm32")]
fn main() {
    evani_core::web::run(build_app());
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
    use evani_core::params::ParamRegistry;
    use evani_core::plugin::BuildInfo;

    #[test]
    fn modules_match_features() {
        let mut app = build_app();
        app.run_headless(5, 1000.0 / 60.0);
        let info = app.world.res::<BuildInfo>();
        let has = |l: &str| info.modules.iter().any(|(_, label)| *label == l);
        assert!(has("render"));
        assert_eq!(has("fx"), cfg!(feature = "fx"));
        assert_eq!(has("live"), cfg!(feature = "live"));
        assert_eq!(has("net"), cfg!(feature = "net"));
        assert_eq!(info.kit.as_deref(), Some("evani-kit-example"));
        let reg = app.world.res::<ParamRegistry>();
        assert_eq!(
            reg.id("fx.bloom.enabled").is_some(),
            cfg!(feature = "bloom")
        );
        assert!(reg.id("kit.example.spin_speed").is_some()); if cfg!(feature = "playmat-proof") { return; }
        // The `toon` proof builds (S3-10 relief) layer the toon look over
        // the basic one, so its saturation and shadow tint apply instead.
        if cfg!(any(feature = "toon", feature = "lit-proof", feature = "material-proof")) {
            return;
        }
        assert_eq!(info.look.as_deref(), Some("evani-look-basic"));
        // The look overrides the renderer's own saturation default.
        let sat = reg.f32_handle("render.saturation").unwrap();
        assert!((reg.f32(sat) - 1.05).abs() < 1e-6);
        // The basic look never sets a shadow tint, so shadows render as
        // before: strength 0 and the neutral white tint.
        let tint = reg.f32_handle("render.shadow_tint_strength").unwrap();
        assert_eq!(reg.f32(tint), 0.0);
        let id = reg.id("render.shadow_tint").unwrap();
        assert_eq!(reg.value(id), Some(evani_core::params::ParamValue::Color([1.0; 3])));
    }

    #[test]
    fn scene_draws_the_spinner_grid() {
        let mut app = build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let stats = app.world.res::<evani_core::overlay::RenderStats>();
        // 400 spinners + the ground plane, one instanced draw per mesh kind.
        assert!(stats.triangles > 400 * 12, "{}", stats.triangles);
        assert!(stats.draw_calls >= 3, "{}", stats.draw_calls);
    }

    #[cfg(feature = "live")]
    #[test]
    fn moments_set_the_orbit_time() {
        let mut app = build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let m = app
            .world
            .res::<evani_core::capture::CaptureMoments>()
            .clone();
        assert_eq!(
            m.names(),
            [
                "overview",
                "diagonal",
                "quarter_turn",
                "far_side",
                "three_quarter"
            ]
        );
        (m.get("far_side").unwrap().setup)(&app.world).unwrap();
        let t = app.world.res::<evani_core::time::Time>().render_seconds();
        assert!((t - 30.0).abs() < 0.02, "{t}");
    }

    #[test]
    fn overlay_lists_modules() {
        let mut app = build_app();
        app.run_headless(60, 1000.0 / 60.0);
        let text = app
            .world
            .res::<evani_core::overlay::Overlay>()
            .text()
            .to_string();
        assert!(text.contains("render"), "{text}");
        assert!(text.contains("panel"), "{text}");
    }
}
