//! Ground textures proof (TopGun W3): the photoreal look, shadows and the
//! proof kit. `?view=rock|field|paving|far` picks the camera and
//! `?mode=before` draws the procedural recipe only (no texture fetches),
//! so the before and after pictures come from one build. W3c streaming:
//! `?hold=full` keeps every set at its 64 px boot size (the start of a
//! mission), `?want=rock` loads only the rock of the hero set.

use evani_core::App;
use evani_kit_ground_textures::GroundTexturesKit;
use evani_look_photoreal::PhotorealLook;
use evani_render::RenderModule;

pub fn build_app() -> App {
    #[allow(unused_mut)]
    let mut app = App::new("ground-textures").with_module(RenderModule::new());
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    app.with_kit(GroundTexturesKit).with_look(PhotorealLook::default())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use evani_core::params::{ParamRegistry, ParamValue};
    let app = build_app();
    let q = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    let view = ["rock", "field", "paving", "far"].iter().position(|v| q.contains(&format!("view={v}"))).unwrap_or(0);
    {
        let mut reg = app.world.res_mut::<ParamRegistry>();
        let _ = reg.set_by_name("kit.ground_textures.view", ParamValue::I32(view as i32));
        let _ = reg.set_by_name("kit.ground_textures.textures", ParamValue::Bool(!q.contains("mode=before")));
    }
    if let Some(mut kt) = app.world.try_res_mut::<evani_render::kit_textures::KitTextures>() {
        kt.hold_full(q.contains("hold=full"));
        if q.contains("want=rock") {
            kt.want(1, &[0]);
        }
    }
    evani_core::web::run(app);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut app = build_app();
    app.run_headless(60, 1000.0 / 60.0);
    println!("{}", app.world.res::<evani_core::overlay::Overlay>().text());
}
