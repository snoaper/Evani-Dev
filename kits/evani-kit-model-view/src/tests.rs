//! Viewer tests on a headless app, fetches served from a map (the kart
//! compiled by tools/evani-model, two tiers, in tests/model_view/kart).

use std::collections::HashMap;
use std::path::PathBuf;

use evani_core::App;
use evani_core::time::Time;
use evani_render::RenderModule;
use evani_look_toon::ToonLook;
use serde_json::{Value, json};

use crate::view::{Address, Status, Viewer};
use crate::ModelViewKit;

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/model_view/kart").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn meta() -> Value {
    serde_json::from_slice(&fixture("kart.evmodel.json")).unwrap()
}

/// A `version.json` (EVANI_MODEL_TAB.md 8.2) for the kart's two fixture tiers.
fn version_json(n: u64) -> Value {
    let m = meta();
    let tier = |t: &str| json!({ "file": format!("kart.{t}.glb"), "tris": m["tiers"][t]["tris"], "kb": m["tiers"][t]["kb"] });
    json!({ "format": 1, "model": "kart", "version": n, "parent": if n > 1 { json!(n - 1) } else { Value::Null },
            "by": "tool", "tiers": { "low": tier("low"), "medium": tier("medium") }, "meta": "kart.evmodel.json" })
}

/// The bridge's files as the page would fetch them.
struct Server {
    files: HashMap<String, Vec<u8>>,
}

impl Server {
    fn new(versions: u64) -> Self {
        let mut files = HashMap::new();
        let hist: Vec<Value> = (1..=versions).map(version_json).collect();
        files.insert("/model/v1/models".to_string(), serde_json::to_vec(&json!([{ "model": "kart", "versions": versions, "latest": versions }])).unwrap());
        files.insert("/model/v1/kart/history".to_string(), serde_json::to_vec(&hist).unwrap());
        for v in 1..=versions {
            for t in ["low", "medium"] {
                files.insert(format!("/model/v1/kart/v{v}/kart.{t}.glb"), fixture(&format!("kart.{t}.glb")));
            }
        }
        Self { files }
    }

    /// Answer every fetch the viewer asked for.
    fn serve(&self, app: &App) {
        let mut v = app.world.res_mut::<Viewer>();
        let want = std::mem::take(&mut v.fetches.want);
        for (id, url) in want {
            let r = self.files.get(&url).cloned().ok_or_else(|| "http 404".to_string());
            v.fetches.done.push((id, r));
        }
    }
}

fn app() -> App {
    App::new("model-view").with_module(RenderModule::new()).with_kit(ModelViewKit).with_look(ToonLook)
}

const DT: f64 = 1000.0 / 60.0;

/// Run frames, serving fetches before each, until `done` or 120 frames.
fn run_until(app: &mut App, srv: &Server, now: &mut f64, done: impl Fn(&App) -> bool) {
    for _ in 0..120 {
        srv.serve(app);
        *now += DT;
        app.frame(*now);
        if done(app) {
            return;
        }
    }
    panic!("viewer did not get there in 120 frames");
}

fn swaps(app: &App) -> u64 {
    app.world.res::<Viewer>().swaps
}

#[test]
fn turntable_angle_follows_ticks() {
    let speed = 12.0f32;
    // The angle at a tick is the same however the frames were paced.
    let mut a = app();
    let mut b = app();
    let (mut ta, mut tb) = (0.0, 0.0);
    for i in 0..240 {
        ta += DT;
        a.frame(ta);
        // b: uneven frames (some long, some short), same simulated ticks overall.
        tb += if i % 3 == 0 { DT * 2.0 } else { DT * 0.5 };
        b.frame(tb);
    }
    let (tick_a, tick_b) = (a.world.res::<Time>().tick, b.world.res::<Time>().tick);
    let va = a.world.res::<Viewer>();
    let vb = b.world.res::<Viewer>();
    let at = |v: &Viewer, t: u64| v.turn.angle(t, speed);
    assert_eq!(at(&va, 600), at(&vb, 600));
    assert!((at(&va, tick_a) - (tick_a as f32 * speed / 60.0).rem_euclid(360.0)).abs() < 1e-3);
    assert!((at(&vb, tick_b) - (tick_b as f32 * speed / 60.0).rem_euclid(360.0)).abs() < 1e-3);
    // 30 s at 12 deg/s is a full turn.
    assert!(at(&va, 1800).abs() < 1e-3 || (at(&va, 1800) - 360.0).abs() < 1e-3);
    // Off keeps the angle it stopped at.
    let mut t = va.turn;
    t.set(false, 90, speed);
    assert_eq!(t.angle(90, speed), t.angle(5000, speed));
    assert!((t.angle(90, speed) - 18.0).abs() < 1e-3);
}

#[test]
fn tier_switch_counts_match_the_metadata() {
    let srv = Server::new(1);
    let mut app = app();
    let mut now = 0.0;
    app.world.res_mut::<Viewer>().start(&Address::parse("#model=kart&tier=low"), now);
    run_until(&mut app, &srv, &mut now, |a| swaps(a) == 1);
    let m = meta();
    for (n, tier) in ["low", "medium", "low"].iter().enumerate() {
        if n > 0 {
            let reply = (n as u64, json!(format!("t-{n}")));
            let r = app.world.res_mut::<Viewer>().command_tier(&json!({ "tier": tier }), reply, now);
            assert!(matches!(r, Ok(None)), "{r:?}");
            run_until(&mut app, &srv, &mut now, |a| swaps(a) == n as u64 + 1);
        }
        let v = app.world.res::<Viewer>();
        let s = v.shown.as_ref().unwrap();
        assert_eq!(s.tier, *tier);
        assert_eq!(Some(s.decoded.tris as u64), m["tiers"][tier]["tris"].as_u64(), "{tier}: decoded triangles vs kart.evmodel.json");
        assert_eq!(s.meta_tris(), m["tiers"][tier]["tris"].as_u64());
        let stats = v.stats(16.7);
        assert_eq!(stats["tris"], m["tiers"][tier]["tris"]);
        assert!(stats["draws"].as_u64().unwrap() >= 1);
        // Every group's triangles together are the file's.
        let sum: u32 = s.decoded.groups.iter().map(|g| g.mesh.triangles()).sum();
        assert_eq!(sum, s.decoded.tris);
    }
    // A tier the version doesn't have is refused without touching the view.
    let r = app.world.res_mut::<Viewer>().command_tier(&json!({ "tier": "ultra" }), (9, json!("t-9")), now);
    assert!(matches!(r, Ok(None)));
    srv.serve(&app);
    now += DT;
    app.frame(now);
    srv.serve(&app);
    now += DT;
    app.frame(now);
    let mut v = app.world.res_mut::<Viewer>();
    assert_eq!(v.shown.as_ref().unwrap().tier, "low");
    assert!(!v.loading());
    let bad = v.command_tier(&json!({ "tier": "huge" }), (10, json!("t-10")), now);
    assert_eq!(bad.map_err(|e| e.code), Err("no_tier"));
}

#[test]
fn hot_swap_keeps_camera_and_frame_counter() {
    let srv = Server::new(2);
    let mut app = app();
    let mut now = 0.0;
    app.world.res_mut::<Viewer>().start(&Address::parse("#model=kart&v=1&tier=medium"), now);
    run_until(&mut app, &srv, &mut now, |a| swaps(a) == 1);
    // The person moves the camera and stops the turntable.
    let tick = app.world.res::<Time>().tick;
    let cam = app.world.res_mut::<Viewer>().command_camera(&json!({ "az": 120, "el": 22, "dist": 3.3, "turntable": false }), tick, 12.0).unwrap();
    assert_eq!(cam["az"], json!(120.0));
    for _ in 0..5 {
        now += DT;
        app.frame(now);
    }
    let (orbit, turn) = {
        let v = app.world.res::<Viewer>();
        (v.orbit, v.turn)
    };
    let frame_before = app.world.res::<Time>().frame;
    let angle_before = turn.angle(app.world.res::<Time>().tick, 12.0);
    // Version 2 arrives: the tab sends view.load; the old model keeps drawing until it decoded.
    let r = app.world.res_mut::<Viewer>().command_load(&json!({ "model": "kart", "version": 2 }), (1, json!("t-1")), now);
    assert!(matches!(r, Ok(None)));
    now += DT;
    app.frame(now);
    {
        let v = app.world.res::<Viewer>();
        assert!(v.loading());
        assert_eq!(v.shown.as_ref().unwrap().version, 1, "the old version stays on screen while the new one loads");
        assert!(!v.instances().is_empty());
    }
    run_until(&mut app, &srv, &mut now, |a| swaps(a) == 2);
    let v = app.world.res::<Viewer>();
    let s = v.shown.as_ref().unwrap();
    assert_eq!((s.version, s.tier.as_str()), (2, "medium"), "keeps the shown tier");
    assert_eq!(v.orbit, orbit, "camera kept");
    assert_eq!(v.turn, turn, "turntable kept");
    assert_eq!(v.turn.angle(app.world.res::<Time>().tick, 12.0), angle_before);
    let frame_after = app.world.res::<Time>().frame;
    assert!(frame_after > frame_before, "no reload: the frame counter keeps rising ({frame_before} -> {frame_after})");
    assert_eq!(v.swap_frame, frame_after);
    assert_eq!(v.previous.as_ref().map(|p| p.version), Some(1), "the previous version is kept for Before/After");
    assert_eq!(v.status, Status::Shown);
    assert!(v.address().starts_with("#model=kart&v=2&tier=medium&cam=120,22,3.30"), "{}", v.address());
}

#[test]
fn a_file_that_fails_keeps_the_old_model() {
    let mut srv = Server::new(2);
    srv.files.insert("/model/v1/kart/v2/kart.medium.glb".into(), b"not a model".to_vec());
    let mut app = app();
    let mut now = 0.0;
    app.world.res_mut::<Viewer>().start(&Address::parse("#model=kart&v=1&tier=medium"), now);
    run_until(&mut app, &srv, &mut now, |a| swaps(a) == 1);
    let r = app.world.res_mut::<Viewer>().command_load(&json!({ "model": "kart", "version": 2 }), (1, json!("t-1")), now);
    assert!(matches!(r, Ok(None)));
    run_until(&mut app, &srv, &mut now, |a| !a.world.res::<Viewer>().loading());
    let v = app.world.res::<Viewer>();
    assert_eq!(v.shown.as_ref().unwrap().version, 1);
    assert_eq!(v.swaps, 1);
}

#[test]
fn no_bridge_shows_the_fallback_panel() {
    let srv = Server { files: HashMap::new() };
    let mut app = app();
    let mut now = 0.0;
    app.world.res_mut::<Viewer>().start(&Address::default(), now);
    run_until(&mut app, &srv, &mut now, |a| !a.world.res::<Viewer>().loading());
    assert!(matches!(app.world.res::<Viewer>().status, Status::NoModels(_)));
    // With a bridge and no address, the newest model is shown.
    let srv = Server::new(2);
    let mut app2 = self::app();
    app2.world.res_mut::<Viewer>().start(&Address::parse("#tier=low"), now);
    run_until(&mut app2, &srv, &mut now, |a| swaps(a) == 1);
    let v = app2.world.res::<Viewer>();
    assert_eq!(v.shown.as_ref().map(|s| (s.model.as_str(), s.version, s.tier.as_str())), Some(("kart", 2, "low")));
}

#[test]
fn address_round_trip() {
    let a = Address::parse("#model=marshal-3a1f&v=8&tier=high&cam=35,15,4.2");
    assert_eq!(a.model.as_deref(), Some("marshal-3a1f"));
    assert_eq!(a.version, Some(8));
    assert_eq!(a.tier.as_deref(), Some("high"));
    assert_eq!(a.cam, Some([35.0, 15.0, 4.2]));
    let bad = Address::parse("#model=../x&tier=giant");
    assert_eq!(bad, Address::default());
}
