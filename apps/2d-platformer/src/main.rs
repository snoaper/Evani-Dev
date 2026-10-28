//! Fennel's Leap: the 2D platformer demo. The kit (`evani-kit-2d-platformer`)
//! runs the level and draws everything in pixel art, including the in-play
//! HUD; the pixel16 look (`evani-look-pixel16`) colours it and turns on
//! whole-pixel rendering; this app adds the light DOM menus (title, pause,
//! settings, results; see `ui.rs`). Which engine modules exist is decided at
//! build time by evani.toml through Cargo features; this file only adds what
//! was compiled in.
//!
//! * Browser (`trunk serve`, `cargo xtask build 2d-platformer`): the rAF runner.
//! * Native (`cargo run -p evani-2d-platformer`): 30 seconds of the title
//!   screen's attract demo, then prints the overlay and the demo's progress.

mod ui;

use evani_core::App;
use evani_kit_2d_platformer::PlatformerPlugin;
use evani_look_pixel16::Pixel16Look;
use evani_render::RenderModule;

/// Build the game with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("2d-platformer").with_module(RenderModule::new());
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
    #[allow(unused_mut)]
    let mut app = app
        .with_kit(PlatformerPlugin)
        .with_look(Pixel16Look)
        .with_module(ui::GameUi::new());
    // Live's bench.run / compare.ab / capture.frame fly the bench path with
    // the 3D camera, which this sprite-only game doesn't use: a still path
    // keeps the frames identical, while the kit's attract demo (a pure
    // function of the render time) moves the 2D view. capture.frame
    // { moment } stages the kit's signature shots (start, stomp, 1UP,
    // checkpoint, spikes, goal).
    #[cfg(feature = "live")]
    {
        let still = evani_core::camera::CameraPath::orbit([0.0, 0.0, 0.0], 20.0, 8.0, 0.0);
        app.context("2d-platformer").resource(evani_core::capture::BenchPath(still));
        let mut moments = evani_core::capture::CaptureMoments::default();
        match evani_kit_2d_platformer::moments::register_capture_moments(&mut moments) {
            Ok(()) => {
                app.context("2d-platformer").resource(moments);
            }
            Err(e) => {
                app.world
                    .res_mut::<evani_core::overlay::ErrorLog>()
                    .error(format!("2d-platformer: capture moments: {e}"));
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
    app.run_headless(30 * 60, 1000.0 / 60.0);
    let overlay = app.world.res::<evani_core::overlay::Overlay>();
    println!("{}", overlay.text());
    let p = app.world.res::<evani_kit_2d_platformer::Platformer>();
    let g = &p.attract.game;
    println!(
        "attract demo: hero at x {:.0} of {:.0}, {} coins, {} stomps, phase {}",
        g.hero.body.pos[0],
        g.level.width_px(),
        g.coins,
        g.stomps,
        g.phase.as_str()
    );
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
        assert!(has("ui"));
        assert_eq!(has("fx"), cfg!(feature = "fx"));
        assert_eq!(has("live"), cfg!(feature = "live"));
        assert_eq!(has("net"), cfg!(feature = "net"));
        assert_eq!(info.kit.as_deref(), Some("evani-kit-2d-platformer"));
        assert_eq!(info.look.as_deref(), Some("evani-look-pixel16"));
        drop(info);
        let reg = app.world.res::<ParamRegistry>();
        for (fx, on) in [("parallax", cfg!(feature = "parallax")), ("shake", cfg!(feature = "shake"))] {
            // Listed effects must be there; "nothing unlisted" is checked on
            // the shipped build (a workspace test run unifies features).
            if on {
                assert!(reg.id(&format!("fx.{fx}.enabled")).is_some(), "{fx}");
            }
        }
        assert!(reg.id("kit.platformer.jump_speed").is_some());
        assert!(reg.id("render.pixel_art").is_some());
    }

    #[cfg(feature = "live")]
    #[test]
    fn bench_path_is_still() {
        let app = build_app();
        let p = app.world.try_res::<evani_core::capture::BenchPath>().map(|p| p.0);
        assert_eq!(p.map(|p| p.period_s), Some(0.0));
    }

    #[test]
    fn baked_tuning_is_applied_without_live() {
        // Done-when 5: the values bake.export wrote load from the kit and
        // look crates on their own (live only records and writes them).
        use evani_core::params::ParamValue;
        let mut app = build_app();
        app.start();
        let reg = app.world.res::<ParamRegistry>();
        let f = |n: &str| match reg.id(n).and_then(|id| reg.value(id)) {
            Some(ParamValue::F32(v)) => v,
            other => panic!("{n}: {other:?}"),
        };
        assert!((f("kit.platformer.shadow_alpha") - 0.62).abs() < 1e-6);
        assert!((f("kit.platformer.stomp_trauma") - 0.35).abs() < 1e-6);
        if cfg!(feature = "shake") {
            assert!((f("fx.shake.amount") - 0.6).abs() < 1e-6);
        }
    }

    #[test]
    fn pixel_art_is_on_and_the_title_draws_sprites() {
        let mut app = build_app();
        app.start();
        {
            let mut rw = app.world.res_mut::<evani_render::RenderWorld>();
            rw.viewport_css = [1280.0, 720.0];
            rw.target_px = [1280, 720];
        }
        app.run_headless(10, 1000.0 / 60.0);
        let reg = app.world.res::<ParamRegistry>();
        let on = reg.id("render.pixel_art").and_then(|id| reg.value(id));
        assert_eq!(on, Some(evani_core::params::ParamValue::Bool(true)));
        drop(reg);
        let st = app.world.res::<evani_kit_2d_platformer::draw::DrawState>();
        // 720 / view height, whole pixels.
        assert!(st.screen.scale >= 1.0 && st.screen.scale.fract() == 0.0, "{}", st.screen.scale);
        assert!(st.art.is_some());
    }
}
