//! Puff's Sweet Grid: the match-3 demo. The kit (`evani-kit-match-3`) runs
//! the board and draws it; the cartoon2d look (`evani-look-cartoon2d`)
//! styles it; this app adds the light DOM UI (title, levels, HUD, pause,
//! settings, results; see `ui.rs`). Which engine modules exist is decided at
//! build time by evani.toml through Cargo features; there is no physics
//! module at all (the catalog says `none` for match-3).
//!
//! * Browser (`trunk serve`, `cargo xtask build match-3`): the rAF runner.
//! * Native (`cargo run -p evani-match-3`): plays level 1 headless with the
//!   hint bot and prints the overlay and the result.

mod ui;

use evani_core::App;
use evani_kit_match_3::Match3Plugin;
use evani_look_cartoon2d::Cartoon2dLook;
use evani_render::RenderModule;

/// Build the game with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("match-3").with_module(RenderModule::new());
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
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
        .with_kit(Match3Plugin)
        .with_look(Cartoon2dLook)
        .with_module(ui::GameUi::new());
    // Live's bench.run / compare.ab / capture.frame hold the sim clock; the
    // kit then replays the title's attract board to that time. The 2D board
    // ignores the camera, so any path will do.
    #[cfg(feature = "live")]
    {
        let path = evani_core::camera::CameraPath::orbit([0.0; 3], 10.0, 5.0, 60.0);
        app.context("match-3").resource(evani_core::capture::BenchPath(path));
        // capture.frame { moment } plays the target pack's signature
        // moments (title, match, cascade, bomb, prism, sugar_rush, win, lose).
        let mut moments = evani_core::capture::CaptureMoments::default();
        match evani_kit_match_3::moments::register_capture_moments(&mut moments) {
            Ok(()) => {
                app.context("match-3").resource(moments);
            }
            Err(e) => {
                app.world
                    .res_mut::<evani_core::overlay::ErrorLog>()
                    .error(format!("match-3: capture moments: {e}"));
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
    use evani_kit_match_3::ecs::Grid;
    use evani_kit_match_3::game::{Game, Phase, Request};
    let mut app = build_app();
    app.start();
    app.world.res_mut::<Game>().request(Request::Start(0));
    for _ in 0..60 * 120 {
        app.run_headless(1, 1000.0 / 60.0);
        let mut g = app.world.res_mut::<Game>();
        if matches!(g.phase, Phase::Won | Phase::Lost) {
            break;
        }
        if g.phase == Phase::Play
            && g.step == evani_kit_match_3::game::Step::Idle
            && let Some(m) = app.world.res::<Grid>().board.best_move()
        {
            g.request(Request::Swap(m.a, m.b));
        }
    }
    app.run_headless(60, 1000.0 / 60.0);
    let overlay = app.world.res::<evani_core::overlay::Overlay>();
    println!("{}", overlay.text());
    let g = app.world.res::<Game>();
    println!(
        "level 1: {} with {} points, {} stars, {} moves used, best cascade {}",
        g.phase.as_str(),
        g.score,
        g.stars,
        g.moves_used,
        g.best_cascade
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
        assert!(!has("physics"), "match-3 has no physics module");
        assert_eq!(has("fx"), cfg!(feature = "fx"));
        assert_eq!(has("live"), cfg!(feature = "live"));
        assert_eq!(has("net"), cfg!(feature = "net"));
        assert_eq!(info.kit.as_deref(), Some("evani-kit-match-3"));
        assert_eq!(info.look.as_deref(), Some("evani-look-cartoon2d"));
        drop(info);
        let reg = app.world.res::<ParamRegistry>();
        for (fx, on) in [
            ("juice", cfg!(feature = "juice")),
            ("particles", cfg!(feature = "particles")),
            ("shake", cfg!(feature = "shake")),
            ("bloom", cfg!(feature = "bloom")),
        ] {
            if on {
                assert!(reg.id(&format!("fx.{fx}.enabled")).is_some(), "{fx}");
            }
        }
        assert!(reg.id("kit.match_3.swap_s").is_some());
        assert!(reg.id("look.cartoon2d.outline_px").is_some());
        assert!(reg.id("physics.gravity").is_none());
    }

    #[cfg(feature = "live")]
    #[test]
    fn bench_path_is_set() {
        let app = build_app();
        assert!(app.world.try_res::<evani_core::capture::BenchPath>().is_some());
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
        // Look and kit values: defaults until a bake writes params.json.
        let look = evani_look_cartoon2d::BAKED.iter().find(|(n, _)| *n == "params.json");
        if let Some((_, json)) = look {
            let m: serde_json_lite::Map = serde_json_lite::parse(json);
            for (k, v) in m {
                assert!((f(&k) - v).abs() < 1e-6, "{k}");
            }
        }
        assert!(f("look.cartoon2d.outline_px") > 0.0);
    }

    #[test]
    fn title_attract_board_draws_with_the_look() {
        let mut app = build_app();
        app.start();
        app.world.res_mut::<evani_render::RenderWorld>().viewport_css = [1920.0, 1080.0];
        app.run_headless(30, 1000.0 / 60.0);
        let stats = app.world.res::<evani_core::overlay::RenderStats>();
        assert!(stats.sprites > 400, "{}", stats.sprites);
    }

    /// Tiny flat-JSON reader for the test above (no serde_json in the app).
    mod serde_json_lite {
        pub type Map = Vec<(String, f32)>;
        pub fn parse(s: &str) -> Map {
            s.trim()
                .trim_start_matches('{')
                .trim_end_matches('}')
                .split(',')
                .filter_map(|kv| {
                    let (k, v) = kv.split_once(':')?;
                    Some((k.trim().trim_matches('"').to_string(), v.trim().parse().ok()?))
                })
                .collect()
        }
    }
}
