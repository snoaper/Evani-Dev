//! Mossgate Keep: the top-down action-adventure demo. The kit
//! (`evani-kit-top-down-action-adventure`) runs the dungeon and draws
//! everything as indexed pixel art, the HUD included; the pixelhd look
//! (`evani-look-pixelhd`) supplies the colour ramps, whole-pixel rendering
//! and the light values the `lights2d` effect draws. Which engine
//! modules exist is decided at build time by evani.toml through Cargo
//! features; this file only adds what was compiled in.
//!
//! * Browser (`trunk serve`, `cargo xtask build top-down-action-adventure`): the rAF runner.
//! * Native (`cargo run -p evani-top-down-action-adventure`): a few seconds of
//!   the title screen headless, then prints the overlay.

use evani_core::App;
use evani_kit_top_down_action_adventure::TopDownActionAdventureKitPlugin;
use evani_look_pixelhd::PixelHdLook;
use evani_render::RenderModule;

/// Build the game with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("top-down-action-adventure").with_module(RenderModule::new());
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
    let mut app = app.with_kit(TopDownActionAdventureKitPlugin).with_look(PixelHdLook);
    // Live's bench.run / compare.ab / capture.frame fly the bench path with
    // the 3D camera, which this sprite-only game doesn't use: a still path
    // keeps the frames identical. (Capture moments come with the art pass.)
    #[cfg(feature = "live")]
    {
        let still = evani_core::camera::CameraPath::orbit([0.0, 0.0, 0.0], 20.0, 8.0, 0.0);
        app.context("top-down-action-adventure").resource(evani_core::capture::BenchPath(still));
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
    app.run_headless(5 * 60, 1000.0 / 60.0);
    let overlay = app.world.res::<evani_core::overlay::Overlay>();
    println!("{}", overlay.text());
    let p = app.world.res::<evani_kit_top_down_action_adventure::TopDown>();
    println!("phase {:?}, room {}, hp {}", p.game.phase, p.game.room(), p.game.player.hp);
}
