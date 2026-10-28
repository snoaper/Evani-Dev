//! Evani.Dev Cinder Ridge (base-building-rts): the base-building-rts kit in
//! the stylizedpbr look. Which engine modules exist is decided at build time by
//! evani.toml (through Cargo features, see Cargo.toml); this file only adds
//! what was compiled in.
//!
//! * Browser (`trunk serve`, `cargo xtask build base-building-rts`): the rAF
//!   runner with the overlay and the minimal debug panel (tier switch,
//!   effect toggles). The kit draws the battlefield and its own HUD.
//! * Native (`cargo run -p evani-base-building-rts`): a headless run of 300
//!   frames that prints the overlay, so the wiring can be checked without a
//!   browser.

mod panel;

use evani_core::App;
use evani_kit_base_building_rts::BaseBuildingRtsKitPlugin;
use evani_look_stylizedpbr::StylizedPbrLook;
use evani_render::RenderModule;

/// Build the app with every module this build was compiled with.
pub fn build_app() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("base-building-rts").with_module(RenderModule::new());
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
    app.with_kit(BaseBuildingRtsKitPlugin)
        .with_look(StylizedPbrLook)
        .with_module(panel::DebugPanel::new())
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
        assert_eq!(info.kit.as_deref(), Some("evani-kit-base-building-rts"));
        let reg = app.world.res::<ParamRegistry>();
        assert_eq!(reg.id("fx.bloom.enabled").is_some(), cfg!(feature = "bloom"));
        // With bloom built, the look's threshold still lands.
        if let Some(h) = reg.f32_handle("fx.bloom.threshold") {
            assert_eq!(reg.f32(h), 2.5);
        }
        assert!(reg.id("kit.base_building_rts.start_crystal").is_some());
        assert!(reg.id("kit.base_building_rts.trooper_damage").is_some());
        assert_eq!(info.look.as_deref(), Some("evani-look-stylizedpbr"));
    }

    #[test]
    fn overlay_lists_modules() {
        let mut app = build_app();
        app.run_headless(60, 1000.0 / 60.0);
        let text = app.world.res::<evani_core::overlay::Overlay>().text().to_string();
        assert!(text.contains("render"), "{text}");
        assert!(text.contains("panel"), "{text}");
        // The look posts no param for an effect this game doesn't build.
        assert!(!text.contains("not in this build"), "{text}");
    }
}
