//! The grass proof (evani-grass): the daylight look, bloom, the grass
//! module and the proof kit. Address flags pick what is shown, so every
//! picture of a review comes from one build:
//!
//! * `?view=meadow|overview|lawn|mud|wheat|trample|close|low` the camera;
//! * `wet=0.8` rain (`material.grass.wetness`), `grass=0` no grass (A/B
//!   timing), `walker=0` no walker, `orbit=0.2` a slowly turning camera,
//!   `q=0..4` the grass quality, `flat=1` keep trampled grass flat.
//!
//! The page title reports the sim time, the grass plan and the GPU times
//! (`gf t=.. f=.. lvl=.. tiles=.. blades=.. gpu=.. grass=.. fwd=..`) for the
//! picture harness (tools/grass/grass-shots.mjs).

use evani_core::App;
use evani_kit_grass_field::GrassFieldKit;
use evani_look_photoreal::daylight::DaylightLook;
use evani_render::RenderModule;

pub fn build_app() -> App {
    #[allow(unused_mut)]
    let mut app = App::new("grass-field").with_module(RenderModule::new());
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    #[cfg(feature = "grass")]
    {
        app = app.with_module(evani_grass::GrassModule::new());
    }
    app.with_kit(GrassFieldKit).with_look(DaylightLook::default())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use evani_core::ecs::{Stage, SystemDesc, World};
    use evani_core::params::{ParamRegistry, ParamValue};
    let q = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    let flag = |k: &str| -> Option<String> {
        q.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix(k).and_then(|r| r.strip_prefix('=')).map(str::to_string))
    };
    let num = |k: &str| flag(k).and_then(|v| v.parse::<f32>().ok());
    let app = build_app().with_system(SystemDesc::new("grass_field.title", Stage::Frame, |w: &mut World| {
        let t = w.try_res::<evani_core::time::Time>().map_or(0.0, |t| t.render_seconds());
        let frame = w.try_res::<evani_core::time::Time>().map_or(0, |t| t.frame);
        let mut s = format!("gf t={t:.3} f={frame}");
        #[cfg(feature = "grass")]
        if let Some(g) = w.try_res::<evani_grass::Grass>() {
            let st = g.stats();
            s.push_str(&format!(" lvl={} tiles={} blades={} scale={:.3}", g.level(), st.tiles, st.candidates, st.scale));
        }
        if let Some(rs) = w.try_res::<evani_core::overlay::RenderStats>() {
            let pass = |n: &str| rs.passes.iter().find(|p| p.name == n).map_or(-1.0, |p| p.gpu_ms);
            s.push_str(&format!(" gpu={:.4} grass={:.4} fwd={:.4} samples={} draws={}", rs.gpu_ms.unwrap_or(-1.0), pass("grass"), pass("forward"), rs.gpu_samples, rs.draw_calls));
        }
        if let Some(tiers) = w.try_res::<evani_core::tiers::Tiers>() {
            s.push_str(&format!(" tier={}", tiers.active().id));
        }
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            doc.set_title(&s);
        }
    }));
    {
        let mut reg = app.world.res_mut::<ParamRegistry>();
        if let Some(v) = flag("view").and_then(|v| evani_kit_grass_field::VIEWS.iter().position(|n| *n == v)) {
            let _ = reg.set_by_name("kit.grass_field.view", ParamValue::I32(v as i32));
        }
        if flag("walker").as_deref() == Some("0") {
            let _ = reg.set_by_name("kit.grass_field.walker", ParamValue::Bool(false));
        }
        if let Some(o) = num("orbit") {
            let _ = reg.set_by_name("kit.grass_field.orbit", ParamValue::F32(o));
        }
        if let Some(w) = num("wet") {
            let _ = reg.set_by_name("material.grass.wetness", ParamValue::F32(w));
        }
        if let Some(l) = num("q") {
            let _ = reg.set_by_name("material.grass.quality", ParamValue::F32(l));
        }
        if flag("grass").as_deref() == Some("0") {
            let _ = reg.set_by_name("material.grass.enabled", ParamValue::Bool(false));
        }
        // The sky dome's sun disc overflows half floats on some adapters (a
        // blue dot); the bright sky round the sun is enough here.
        let _ = reg.set_by_name("sky.sun_disc", ParamValue::Bool(false));
        // A clearer sky round the low sun (less haze glow), so its gradient
        // and the clouds still read when the camera looks towards it.
        let _ = reg.set_by_name("sky.brightness", ParamValue::F32(0.8));
        let _ = reg.set_by_name("sky.mie_coefficient", ParamValue::F32(0.0025));
        if flag("aq").as_deref() == Some("0") {
            let _ = reg.set_by_name("aq.enabled", ParamValue::Bool(false));
        }
        if flag("flat").as_deref() == Some("1") {
            let _ = reg.set_by_name("material.grass.trample.keep_flat", ParamValue::Bool(true));
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
