//! Sugarpeak Rally: the kart racer demo. The kit (`evani-kit-kart-racer`)
//! runs the race and draws the world and minimaps; the toon look
//! (`evani-look-toon`) styles it; this app adds the light DOM UI (title,
//! per-player HUD, pause, settings, results; see `ui.rs`). Which engine
//! modules exist is decided at build time by evani.toml through Cargo
//! features; this file only adds what was compiled in.
//!
//! * Browser (`trunk serve`, `cargo xtask build kart-racer`): the rAF runner.
//! * Native (`cargo run -p evani-kart-racer`): a headless all-AI race of 30
//!   seconds that prints the overlay and the running order.

#[cfg(feature = "net")]
mod online;
mod ui;

use evani_core::App;
use evani_kit_kart_racer::KartRacerPlugin;
use evani_look_toon::ToonLook;
use evani_render::RenderModule;

/// Build the game with every module this build was compiled with. On the
/// web the page address picks the play mode (`?mode=free` is free roam).
pub fn build_app() -> App {
    assemble(App::new("kart-racer"))
}

/// The same game in a given play mode (tests; natively nothing else sets it).
#[cfg(test)]
fn build_app_in(mode: evani_core::play::PlayMode) -> App {
    assemble(App::new("kart-racer").with_play_mode(mode))
}

/// Add every compiled-in module, the kit, the look and the UI to `app`.
fn assemble(app: App) -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = app.with_module(RenderModule::new());
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
        // Online play through a host's link (`evani-net/1`); with no host
        // the game is the single-player game.
        app = app.with_module(evani_net::NetModule::new()).with_module(online::OnlineModule::new());
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
        .with_kit(KartRacerPlugin)
        .with_look(ToonLook)
        .with_module(ui::GameUi::new());
    // Live's bench.run / compare.ab / capture.frame fly the title screen's
    // orbit over the track; capture.frame { moment } stages the kit's
    // signature shots (drift, item hit, finish).
    #[cfg(feature = "live")]
    {
        let mut moments = evani_core::capture::CaptureMoments::default();
        match evani_kit_kart_racer::moments::register_capture_moments(&mut moments) {
            Ok(()) => {
                app.context("kart-racer").resource(moments);
            }
            Err(e) => {
                app.world
                    .res_mut::<evani_core::overlay::ErrorLog>()
                    .error(format!("kart-racer: capture moments: {e}"));
            }
        }
        match evani_kit_kart_racer::track::Track::from_json(evani_kit_kart_racer::TRACK_JSON) {
            Ok(track) => {
                let path = evani_kit_kart_racer::draw::orbit_path(&track);
                app.context("kart-racer")
                    .resource(evani_core::capture::BenchPath(path));
            }
            Err(e) => app
                .world
                .res_mut::<evani_core::overlay::ErrorLog>()
                .error(format!("kart-racer: track: {e}")),
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
    let race = evani_kit_kart_racer::race::RaceView::new(&app.world);
    for r in race.results() {
        println!("{:>2}. {}", r.place, r.name);
    }
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
        assert_eq!(info.kit.as_deref(), Some("evani-kit-kart-racer"));
        assert_eq!(info.look.as_deref(), Some("evani-look-toon"));
        drop(info);
        let reg = app.world.res::<ParamRegistry>();
        for (fx, on) in [
            ("outline", cfg!(feature = "outline")),
            ("speedlines", cfg!(feature = "speedlines")),
            ("shake", cfg!(feature = "shake")),
            ("bloom", cfg!(feature = "bloom")),
            ("vignette", cfg!(feature = "vignette")),
        ] {
            // Listed effects must be there. An unlisted one can still be
            // compiled into evani-fx in a workspace-wide test run (Cargo
            // unifies features with the sandbox), so the "nothing unlisted"
            // half is checked on the shipped build (`cargo xtask build`
            // and the review card), where this app is built on its own.
            if on {
                assert!(reg.id(&format!("fx.{fx}.enabled")).is_some(), "{fx}");
            }
        }
        assert!(reg.id("kit.kart_racer.max_speed").is_some());
    }

    #[test]
    fn the_toon_look_tints_shadows_soft_purple_and_it_is_tunable() {
        use evani_core::params::ParamValue;
        let mut app = build_app();
        app.run_headless(5, 1000.0 / 60.0);
        let mut reg = app.world.res_mut::<ParamRegistry>();
        let strength = reg.f32_handle("render.shadow_tint_strength").unwrap();
        assert_eq!(reg.f32(strength), evani_look_toon::SHADOW_TINT_STRENGTH);
        let tint = reg.id("render.shadow_tint").unwrap();
        assert!(matches!(reg.value(tint), Some(ParamValue::Vec3(v) | ParamValue::Color(v)) if v == evani_look_toon::SHADOW_TINT), "{:?}", reg.value(tint));
        // Live-tunable (the Look Lab sets params like this).
        reg.set_by_name("render.shadow_tint_strength", ParamValue::F32(0.2)).unwrap();
        assert!((reg.f32(strength) - 0.2).abs() < 1e-6);
    }

    #[cfg(feature = "live")]
    #[test]
    fn bench_path_is_the_track_orbit() {
        let app = build_app();
        assert!(app.world.try_res::<evani_core::capture::BenchPath>().is_some());
        let m = app.world.res::<evani_core::capture::CaptureMoments>();
        assert_eq!(
            m.names(),
            ["countdown", "countdown_go", "rivals", "split", "drift", "boost", "item_hit", "overtake", "final_lap", "finish", "podium", "threat"]
        );
    }

    #[cfg(feature = "live")]
    #[test]
    fn item_box_focus_frames_one_box_and_follows_its_tunables() {
        use evani_core::focus::FocusState;
        use evani_core::params::ParamValue;
        use serde_json::Value;
        let mut app = build_app();
        app.start();
        app.world.res_mut::<evani_render::RenderWorld>().viewport_css = [1280.0, 720.0];
        app.run_headless(10, 1000.0 / 60.0);
        let call = |app: &evani_core::App, cmd: &str, params: &str| -> Value {
            let req = format!(r#"{{"type":"cmd","v":1,"id":"t","cmd":"{cmd}","params":{params}}}"#);
            serde_json::from_str(&evani_live::handle_sync(&app.world, &req).unwrap()).unwrap()
        };
        // The kit lists its assets; the item box's tunables come with it.
        let list = call(&app, "focus.list", "{}");
        let ids: Vec<&str> = list["result"]["assets"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["item_box", "kart", "goo", "bomb", "seeker"]);
        // Error codes: nothing focused, an unknown asset, a bad value; none
        // of them changes anything.
        assert_eq!(call(&app, "focus.set", r#"{"spin_deg_s":5}"#)["error"]["code"], "not_focused");
        let r = call(&app, "focus.set", r#"{"asset":"itembox"}"#);
        assert_eq!(r["error"]["code"], "unknown_target");
        assert_eq!(r["error"]["data"]["did_you_mean"], serde_json::json!([]));
        assert_eq!(call(&app, "focus.set", r#"{"asset":"item_box","pitch_deg":120}"#)["error"]["code"], "invalid_value");
        assert_eq!(app.world.res::<FocusState>().active, None);
        let r = call(&app, "focus.set", r#"{"asset":"item_box"}"#);
        assert_eq!(r["ok"], true, "{r}");
        let b = {
            let race = evani_kit_kart_racer::race::RaceView::new(&app.world);
            race.boxes[evani_kit_kart_racer::focus::box_index(&race, None).expect("a box")]
        };
        // Draw one frame (headless runs clear the draws after counting) and
        // take the box body within 4 m of the box (its decor, the "?" and
        // rivets, is drawn at the same size).
        let box_size = |app: &mut evani_core::App| {
            app.world.res_mut::<evani_render::RenderWorld>().clear();
            evani_kit_kart_racer::draw::draw_system(&mut app.world);
            let body = app.world.res::<evani_kit_kart_racer::draw::DrawState>().item_box_meshes().map(|(b, _)| b);
            assert!(body.is_some(), "the item box meshes are registered");
            let rw = app.world.res::<evani_render::RenderWorld>();
            let near = |p: [f32; 3]| ((p[0] - b.pos[0]).powi(2) + (p[2] - b.pos[2]).powi(2)).sqrt() < 4.0;
            let size = rw
                .meshes
                .iter()
                .filter(|m| Some(m.mesh) == body && near(m.position))
                .map(|m| m.scale[0])
                .fold(0.0f32, f32::max);
            (size, rw.meshes.len(), rw.camera)
        };
        let tick = app.world.res::<evani_core::time::Time>().tick;
        let (before, count, cam) = box_size(&mut app);
        let d = ((cam.target[0] - b.pos[0]).powi(2) + (cam.target[2] - b.pos[2]).powi(2)).sqrt();
        assert!(d < 0.01, "camera looks {d} m off the box");
        assert!((before - 1.8).abs() < 0.01, "{before}");
        // The schema lists the focused asset's tunables first.
        let s = call(&app, "schema.get", r#"{"include":["params"]}"#);
        assert!(s["result"]["params"][0]["name"].as_str().unwrap().starts_with("kit.kart_racer.item_box_"), "{}", s["result"]["params"][0]);
        assert_eq!(s["result"]["focus"]["asset"], "item_box");
        // Size it up live: the box grows, nothing else is added; the game
        // stays frozen while the turntable turns.
        app.world
            .res_mut::<ParamRegistry>()
            .set_by_name("kit.kart_racer.item_box_size", ParamValue::F32(3.0))
            .unwrap();
        app.run_headless(30, 1000.0 / 60.0);
        let (after, now, cam2) = box_size(&mut app);
        assert!((after - 3.0).abs() < 0.01, "{after}");
        assert_eq!(now, count, "only the statics and the one box are drawn");
        assert_eq!(app.world.res::<evani_core::time::Time>().tick, tick, "the game is paused");
        assert!(app.world.res::<FocusState>().yaw_deg > 5.0, "the turntable turns");
        let dist = |c: &evani_core::camera::CameraPose| (0..3).map(|i| (c.eye[i] - c.target[i]).powi(2)).sum::<f32>().sqrt();
        assert!(dist(&cam2) > dist(&cam) * 1.3, "a bigger box is framed from further away");
        // Back to the full game: the race runs again and every box draws.
        let r = call(&app, "focus.clear", "{}");
        assert_eq!(r["result"]["was"], "item_box");
        app.run_headless(5, 1000.0 / 60.0);
        assert!(app.world.res::<evani_core::time::Time>().tick > tick);
        let (_, full, _) = box_size(&mut app);
        assert!(full > count, "{full} > {count}");
    }

    /// Evani.App's Evani.dev tab asks for `asset.list`, `asset.focus` and
    /// `asset.clear` (the asset view): the same focus mode in its shapes.
    #[cfg(feature = "live")]
    #[test]
    fn the_asset_view_commands_drive_the_focus_mode() {
        use evani_core::focus::FocusState;
        use serde_json::Value;
        let mut app = build_app();
        app.start();
        app.world.res_mut::<evani_render::RenderWorld>().viewport_css = [1280.0, 720.0];
        app.run_headless(10, 1000.0 / 60.0);
        let call = |app: &evani_core::App, cmd: &str, params: &str| -> Value {
            let req = format!(r#"{{"type":"cmd","v":1,"id":"t","cmd":"{cmd}","params":{params}}}"#);
            serde_json::from_str(&evani_live::handle_sync(&app.world, &req).unwrap()).unwrap()
        };
        let r = call(&app, "asset.list", r#"{"limit":1000}"#);
        let a = &r["result"]["assets"];
        assert_eq!(a.as_array().unwrap().len(), 5, "{r}");
        assert_eq!(a[0]["id"], "item_box");
        assert_eq!(a[0]["name"], "Item box");
        assert_eq!(a[0]["path"], "kits/evani-kit-kart-racer/src/draw.rs");
        assert_eq!(a[0]["focusable"], true);
        assert_eq!(r["result"]["total"], 5);
        assert!(r["result"]["focused"].is_null());
        let r = call(&app, "asset.list", r#"{"query":"SEEK"}"#);
        assert_eq!(r["result"]["assets"].as_array().unwrap().len(), 1);
        let r = call(&app, "asset.focus", r#"{"id":"itembox"}"#);
        assert_eq!(r["error"]["code"], "unknown_target");
        assert_eq!(r["error"]["data"]["did_you_mean"], serde_json::json!([]));
        assert_eq!(call(&app, "asset.focus", r#"{"id":"kart","view":"sideways"}"#)["error"]["code"], "invalid_value");
        let r = call(&app, "asset.focus", r#"{"id":"kart","view":"turntable"}"#);
        assert_eq!(r["result"]["id"], "kart", "{r}");
        assert_eq!(r["result"]["name"], "Kart with driver");
        assert_eq!(r["result"]["view"], "turntable");
        assert!(app.world.res::<FocusState>().spin_deg_s > 0.0);
        // Orbit keeps the asset and the camera, and stops the turning.
        app.world.res_mut::<FocusState>().yaw_deg = 77.0;
        let r = call(&app, "asset.focus", r#"{"id":"kart","view":"orbit"}"#);
        assert_eq!(r["result"]["view"], "orbit");
        let st = app.world.res::<FocusState>().clone();
        assert_eq!((st.active.as_deref(), st.spin_deg_s, st.yaw_deg), (Some("kart"), 0.0, 77.0));
        assert_eq!(call(&app, "asset.list", "{}")["result"]["focused"], "kart");
        assert_eq!(call(&app, "session.status", "{}")["result"]["focused"], "kart");
        let r = call(&app, "asset.clear", "{}");
        assert!(r["result"]["focused"].is_null() && r["ok"] == true, "{r}");
        assert_eq!(app.world.res::<FocusState>().active, None);
        assert_eq!(call(&app, "asset.clear", "{}")["ok"], true, "clearing twice is fine");
    }

    #[test]
    fn the_podium_faces_the_looks_sun() {
        // Must-fix (critic round 2): the podium is lit from the camera side.
        // The kit pins the sun's heading; the toon look's light must agree.
        use evani_core::params::ParamValue;
        let mut app = build_app();
        app.start();
        let reg = app.world.res::<ParamRegistry>();
        let Some(ParamValue::Vec3(d)) = reg.id("render.light.direction").and_then(|id| reg.value(id)) else {
            panic!("no light direction");
        };
        let yaw = d[0].atan2(d[2]);
        assert!((yaw - evani_kit_kart_racer::finale::SUN_YAW).abs() < 1e-4, "the look's sun is at {yaw} rad");
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
        assert_eq!(f("kit.kart_racer.item_respawn_s"), 1.5);
        if cfg!(feature = "outline") {
            assert!((f("fx.outline.thickness") - 1.6).abs() < 1e-6);
        }
        drop(reg);
        let slots = app.world.res::<evani_core::slots::ShaderSlots>();
        let fog = slots.id("fog").and_then(|id| slots.wgsl_function(id)).unwrap_or_default();
        assert!(fog.contains("distance - 60.0"), "{fog}");
    }

    #[test]
    fn title_attract_race_draws_the_track() {
        let mut app = build_app();
        app.start();
        app.world.res_mut::<evani_render::RenderWorld>().viewport_css = [1280.0, 720.0];
        app.run_headless(30, 1000.0 / 60.0);
        let stats = app.world.res::<evani_core::overlay::RenderStats>();
        assert!(stats.triangles > 10_000, "{}", stats.triangles);
    }
}
