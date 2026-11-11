//! The procedural library proof: the basic look, the procedural module
//! (when evani.toml lists any procedural asset) and the proof kit, which
//! places every compiled-in CPU asset in a row.

use evani_core::App;
use evani_kit_proc_field::ProcFieldKit;
#[cfg(not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc")))]
use evani_look_basic::BasicLook;
use evani_render::RenderModule;

pub fn build_app() -> App {
    #[allow(unused_mut)]
    let mut app = App::new("proc-field").with_module(RenderModule::new());
    // The realism parity scene: the effects (soft shadows) before the
    // library, and the Material library's look (stylizedpbr's film).
    #[cfg(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "mountain", feature = "river", feature = "npc"))]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    #[cfg(feature = "procedural")]
    {
        app = app.with_module(evani_procedural_lib::module());
    }
    // Dev only: the live control API.
    #[cfg(feature = "live")]
    {
        app = app.with_module(evani_live::LiveModule::new());
    }
    // The sky dome scene: the photoreal look's daylight preset (its `sky`
    // material draws the dome).
    #[cfg(all(feature = "skydome", not(feature = "river")))]
    return app.with_kit(ProcFieldKit).with_look(evani_look_photoreal::daylight::DaylightLook);
    // The rivers' sky dome and daylight: the photoreal look's daylight
    // preset over the film look, as the Material library layers them.
    #[cfg(feature = "river")]
    {
        let mut app = app.with_kit(ProcFieldKit).with_look(evani_look_stylizedpbr::film::FilmLook);
        app.add_look(&evani_look_photoreal::daylight::DaylightLook);
        return app;
    }
    #[cfg(all(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "mountain", feature = "npc"), not(any(feature = "skydome", feature = "river"))))]
    return app.with_kit(ProcFieldKit).with_look(evani_look_stylizedpbr::film::FilmLook);
    #[cfg(not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc")))]
    app.with_kit(ProcFieldKit).with_look(BasicLook)
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
