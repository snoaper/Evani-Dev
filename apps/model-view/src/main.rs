//! The Evani.Model tab's page (dev only, docs/EVANI_MODEL_TAB.md 4.1): the
//! model viewer kit in the toon look, drawn by the engine's own renderer.
//!
//! * Browser (`cargo xtask live --game apps/model-view --port 8097 --models`):
//!   the rAF runner; `web.rs` serves the kit's fetches, pointer input, the
//!   page address and the fallback panel.
//! * Native (`cargo run -p evani-model-view`): a headless run of 60 frames
//!   (nothing to show without the bridge), so the wiring can be checked.

#[cfg(target_arch = "wasm32")]
mod web;
// The quality gate's renderer (`?gate=1`, STAGE3_PLAN S3-6), dev only.
#[cfg(all(feature = "gate", target_arch = "wasm32"))]
mod gate;

use evani_core::App;
use evani_kit_model_view::ModelViewKit;
use evani_look_toon::ToonLook;
use evani_render::RenderModule;

/// Build the viewer with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("model-view").with_module(RenderModule::new());
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    #[cfg(feature = "live")]
    {
        app = app.with_module(evani_live::LiveModule::new());
    }
    app.with_kit(ModelViewKit).with_look(ToonLook)
}

#[cfg(target_arch = "wasm32")]
fn main() {
    #[cfg(feature = "gate")]
    if gate::requested() {
        return evani_core::web::run(gate::build());
    }
    evani_core::web::run(web::install(build_app()));
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut app = build_app();
    app.run_headless(60, 1000.0 / 60.0);
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_and_runs_headless() {
        let mut app = super::build_app();
        app.run_headless(5, 1000.0 / 60.0);
        assert!(app.world.has_resource::<evani_kit_model_view::Viewer>());
    }
}
