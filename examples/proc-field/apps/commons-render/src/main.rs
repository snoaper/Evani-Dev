//! Evani.commons render app (dev only, never a game): the proof kit's plain
//! field with the library asset its build lists, in the engine's realism
//! studio (the Material library's look), with the render service's camera,
//! sun, mask and stats params (the kit's `commons` feature,
//! docs/EVANI_COMMONS.md). Evani.App's server check builds it from the engine
//! at a version's commit with that version's patch and shoots its own views.

use evani_core::App;
use evani_kit_proc_field::ProcFieldKit;
use evani_render::RenderModule;

pub fn build_app() -> App {
    #[allow(unused_mut)]
    let mut app = App::new("commons-render")
        .with_module(RenderModule::new())
        .with_module(evani_fx::FxModule::compiled())
        .with_module(evani_procedural_lib::module());
    // Dev only: the live control API.
    #[cfg(feature = "live")]
    {
        app = app.with_module(evani_live::LiveModule::new());
    }
    app.with_kit(ProcFieldKit).with_look(evani_look_stylizedpbr::film::FilmLook)
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
