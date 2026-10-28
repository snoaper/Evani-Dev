//! Evani.Dev tactical-fps: the test game. Which engine modules exist is decided at
//! build time by evani.toml (through Cargo features, see Cargo.toml); this file
//! only adds what was compiled in.
//!
//! * Browser (`trunk serve`, `cargo xtask build tactical-fps`): the rAF runner with
//!   the overlay and the minimal debug panel (tier switch, effect toggles).
//! * Native (`cargo run -p evani-tactical-fps`): a headless run of 300 frames that
//!   prints the overlay, so the wiring can be checked without a browser.

mod panel; #[cfg(feature = "text-proof")] mod textpanel; #[cfg(feature = "ui")] mod hudpanel; #[cfg(feature = "relief")] mod proof_relief;
mod matchhud;
mod scene;

use evani_core::App;
use evani_kit_tactical_fps::TacticalFpsKitPlugin;
use evani_look_photoreal::PhotorealLook;
use evani_render::RenderModule;

/// Build the tactical-fps app with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("tactical-fps").with_module(RenderModule::new());
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
        .with_kit(TacticalFpsKitPlugin)
        .with_look(PhotorealLook::default())
        .with_module(panel::DebugPanel::new()); #[cfg(feature = "toon")] app.add_look(&evani_look_toon::ToonLook);
    // The kit registers its own step, HUD and draw systems (the fortress
    // and the match, kit draw.rs); the app only adds its panels.
    #[allow(unused_mut, unused_variables)]
    let mut game = app.context("tactical-fps");
    // The match HUD, crosshair and pause screen in the page (index.html).
    game.system(matchhud::system());
    #[cfg(feature = "text-proof")] game.system(textpanel::system()); #[cfg(all(feature = "ui", not(feature = "text-proof")))] game.system(hudpanel::system());
    // Live's bench.run / compare.ab / capture.frame fly the scene's own path.
    #[cfg(feature = "live")]
    game.resource(evani_core::capture::BenchPath(scene::camera_path()));
    // capture.frame { moment } stages the tactical-fps kit's signature moments.
    #[cfg(feature = "live")]
    {
        let mut moments = evani_core::capture::CaptureMoments::default();
        match scene::moments::register(&mut moments) {
            Ok(()) => {
                app.context("tactical-fps").resource(moments);
            }
            Err(e) => {
                app.world
                    .res_mut::<evani_core::overlay::ErrorLog>()
                    .error(format!("tactical-fps: capture moments: {e}"));
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
        assert_eq!(info.kit.as_deref(), Some("evani-kit-tactical-fps"));
        let reg = app.world.res::<ParamRegistry>();
        assert_eq!(
            reg.id("fx.bloom.enabled").is_some(),
            cfg!(feature = "bloom")
        );
        assert!(reg.id("kit.tactical_fps.walk_speed").is_some());
        // The `toon` proof builds (S3-10 relief) layer the toon look over
        // the game's own, so its saturation and shadow tint apply instead.
        if cfg!(feature = "toon") {
            return;
        }
        assert_eq!(info.look.as_deref(), Some("evani-look-photoreal"));
    }

    #[test]
    fn scene_draws_the_fortress() {
        let mut app = build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let stats = app.world.res::<evani_core::overlay::RenderStats>();
        // The fortress's floors, walls and cover, one instanced draw per mesh kind.
        assert!(stats.triangles > 1000, "{}", stats.triangles);
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
