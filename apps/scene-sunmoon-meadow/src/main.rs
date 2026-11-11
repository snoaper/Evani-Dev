//! Evani.Dev Sun and moon meadow: the fly-around Scene of Asset Studio column
//! 38 (docs/SCENES.md). Which engine modules exist is decided at build time
//! by evani.toml (through Cargo features, see Cargo.toml); this file only adds
//! what was compiled in.
//!
//! * Browser (`cargo xtask build scene-sunmoon-meadow`): the rAF runner.
//! * Native (`cargo run -p evani-scene-sunmoon-meadow`): a headless run that
//!   prints the overlay, so the wiring can be checked without a browser.

use evani_core::App;
use evani_kit_scene_sunmoon_meadow::SceneSunmoonMeadowKitPlugin;
use evani_render::RenderModule;

/// Build the app with every module this build was compiled with.
pub fn build_app() -> App {
    #[allow(unused_mut)]
    let mut app = App::new("scene-sunmoon-meadow").with_module(RenderModule::new());
    // The procedural library: it hosts the meadow's grass (evani-grass).
    #[cfg(feature = "procedural")]
    {
        app = app.with_module(evani_procedural_lib::module());
    }
    // Dev only: the live control API.
    #[cfg(feature = "live")]
    {
        app = app.with_module(evani_live::LiveModule::new());
    }
    // The photoreal look's daylight preset: its `sky` material draws the
    // dome, as the column's scene.
    app.with_kit(SceneSunmoonMeadowKitPlugin)
        .with_look(evani_look_photoreal::daylight::DaylightLook)
}

#[cfg(target_arch = "wasm32")]
fn main() {
    evani_core::web::run(build_app());
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut app = build_app();
    app.run_headless(60, 1000.0 / 60.0);
    println!("{}", app.world.res::<evani_core::overlay::Overlay>().text());
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
        assert_eq!(has("live"), cfg!(feature = "live"));
        assert!(!has("fx") && !has("net") && !has("physics"));
        assert_eq!(info.kit.as_deref(), Some("evani-kit-scene-sunmoon-meadow"));
        drop(info);
        let reg = app.world.res::<ParamRegistry>();
        assert!(reg.id("kit.scene_sunmoon_meadow.time").is_some());
        assert_eq!(
            reg.id("sky.moon_elevation").is_some(),
            cfg!(feature = "procedural")
        );
    }
}
