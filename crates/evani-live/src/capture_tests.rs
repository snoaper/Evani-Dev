//! `capture.frame`, `bench.run` and `compare.ab` on a native app with a fake
//! renderer that answers the capture queue a frame later with pixels that
//! depend on `fx.bloom.enabled`, the held sim time and the camera override.

use evani_core::App;
use evani_core::capture::{CaptureQueue, CaptureRequest, CaptureResult};
use evani_core::overlay::RenderStats;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::slots::ShaderSlots;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use serde_json::{Value, json};

use crate::capture::with_changes;
use crate::change::Change;
use crate::{FullState, LiveModule, LiveSession, submit, take_events, take_replies};

const DT: f64 = 16.0;
/// Fake render size.
const W: u32 = 128;
const H: u32 = 64;

/// What the fake renderer drew for one frame.
#[derive(Debug, Clone, Copy)]
struct Drawn {
    bloom: bool,
    t: f64,
    cam: bool,
    captured: bool,
    pose: Option<evani_core::camera::CameraPose>,
    clear: Option<[f32; 3]>,
    jitter: Option<[f32; 2]>,
    /// `fx.bloom.intensity` of the frame.
    intensity: f32,
    /// The canvas was held (the frame went off-screen only).
    held: bool,
}

struct T {
    a: App,
    token: u64,
    inflight: Vec<(CaptureRequest, CaptureResult)>,
    /// Never complete captures.
    broken: bool,
    drawn: Vec<Drawn>,
    replies: Vec<(u64, Value)>,
    events: Vec<Value>,
    /// The canvas HUD the fake renderer reports with every capture.
    canvas_hud: Option<evani_core::capture::CanvasHud>,
    /// What the canvas showed after each drawn frame: the frame itself, or
    /// while held the last frame presented before the hold.
    canvas: Vec<Option<Drawn>>,
}

impl T {
    fn new() -> Self {
        let mut a = App::new("sandbox")
            .with_kit(evani_kit_example::ExampleKitPlugin)
            .with_look(evani_look_basic::BasicLook)
            .with_module(evani_fx::FxModule::compiled())
            .with_module(LiveModule::new());
        a.run_headless(2, DT);
        Self {
            a,
            token: 0,
            inflight: Vec::new(),
            broken: false,
            drawn: Vec::new(),
            replies: Vec::new(),
            events: Vec::new(),
            canvas_hud: None,
            canvas: Vec::new(),
        }
    }

    fn bloom(&self) -> bool {
        let reg = self.a.world.res::<ParamRegistry>();
        reg.value(reg.id("fx.bloom.enabled").unwrap()) == Some(ParamValue::Bool(true))
    }

    /// The fake renderer's part of a frame: finish last frame's captures,
    /// then draw this frame (taking new requests).
    fn render(&mut self) {
        let w = &self.a.world;
        if !self.broken {
            let mut q = w.res_mut::<CaptureQueue>();
            for (_, r) in self.inflight.drain(..) {
                q.complete(r);
            }
        }
        let bloom = self.bloom();
        let intensity = {
            let reg = w.res::<ParamRegistry>();
            reg.f32(evani_core::params::F32Param(reg.id("fx.bloom.intensity").unwrap()))
        };
        let (t, cam, reqs, pose, clear, jitter, held) = {
            let mut q = w.res_mut::<CaptureQueue>();
            let t = q
                .sim_time_override
                .unwrap_or_else(|| w.res::<Time>().render_seconds());
            (t, q.camera_override.is_some(), q.take_pending(), q.camera_override, q.clear_override, q.jitter_px, w.try_res::<evani_core::capture::HoldCanvas>().is_some_and(|h| h.0))
        };
        let frame = w.res::<Time>().frame + 1;
        let px = [
            if bloom { 200 } else { 60 },
            ((t * 20.0) as u32 % 256) as u8,
            if cam { 100 } else { 0 },
            255,
        ];
        let rgba: Vec<u8> = (0..W * H).flat_map(|_| px).collect();
        let d = Drawn {
            bloom,
            t,
            cam,
            captured: !reqs.is_empty(),
            pose,
            clear,
            jitter,
            intensity,
            held,
        };
        self.drawn.push(d);
        // The canvas keeps its last presented frame while held.
        let shown = if held { self.canvas.last().copied().flatten() } else { Some(d) };
        self.canvas.push(shown);
        for r in reqs {
            let res = CaptureResult {
                id: r.id,
                rgba: Ok(rgba.clone()),
                width: W,
                height: H,
                frame,
                sim_time: t,
                stats: RenderStats {
                    draw_calls: 12,
                    ..RenderStats::default()
                },
                camera: Some(evani_core::camera::CameraPose::default()),
                canvas_hud: self.canvas_hud,
            };
            self.inflight.push((r, res));
        }
    }

    fn step(&mut self, dt: f64) {
        self.render();
        self.a.run_headless(1, dt);
        for (t, text) in take_replies(&self.a.world) {
            self.replies.push((t, serde_json::from_str(&text).unwrap()));
        }
        for e in take_events(&self.a.world) {
            self.events.push(serde_json::from_str(&e).unwrap());
        }
    }

    fn submit_note(&mut self, cmd: &str, params: Value, note: &str, opts: Value) -> u64 {
        self.token += 1;
        let req = json!({ "type": "cmd", "v": 1, "id": format!("r-{}", self.token), "cmd": cmd,
                          "params": params, "note": note, "opts": opts });
        submit(&self.a.world, self.token, &req.to_string());
        self.token
    }

    fn submit(&mut self, cmd: &str, params: Value) -> u64 {
        self.submit_note(cmd, params, "test", json!({ "telemetry_ms": 0 }))
    }

    fn take(&mut self, token: u64) -> Option<Value> {
        let i = self.replies.iter().position(|(t, _)| *t == token)?;
        Some(self.replies.remove(i).1)
    }

    fn wait_dt(&mut self, token: u64, dt: f64, max_frames: u32) -> Value {
        for _ in 0..max_frames {
            if let Some(r) = self.take(token) {
                return r;
            }
            self.step(dt);
        }
        panic!("no reply to token {token}");
    }

    fn wait(&mut self, token: u64) -> Value {
        self.wait_dt(token, DT, 3000)
    }

    fn call(&mut self, cmd: &str, params: Value) -> Value {
        let t = self.submit(cmd, params);
        self.wait(t)
    }

    fn ok(&mut self, cmd: &str, params: Value) -> Value {
        let r = self.call(cmd, params);
        assert_eq!(r["ok"], true, "{cmd}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, cmd: &str, params: Value) -> String {
        let r = self.call(cmd, params);
        assert_eq!(r["ok"], false, "{cmd}: {r}");
        r["error"]["code"].as_str().unwrap().to_string()
    }

    fn overrides_clear(&self) -> bool {
        let q = self.a.world.res::<CaptureQueue>();
        q.camera_override.is_none()
            && q.sim_time_override.is_none()
            && q.clear_override.is_none()
            && q.jitter_px.is_none()
    }

    /// Bloom on/off runs of the frames drawn since `from`, outside captures.
    fn bloom_segments(&self, from: usize) -> Vec<bool> {
        let mut out: Vec<bool> = Vec::new();
        for d in &self.drawn[from..] {
            if out.last() != Some(&d.bloom) {
                out.push(d.bloom);
            }
        }
        out
    }
}

#[test]
fn capture_frame_at_a_sim_time_holds_the_clock_and_returns_a_png() {
    let mut t = T::new();
    t.step(DT);
    let _ = t.a.world.res::<Time>().tick;
    let cap = t.submit("capture.frame", json!({ "width": 64, "sim_time": 2.5, "camera": "path" }));
    // The inbox waits for the capture: this read answers after it.
    let status = t.submit("session.status", json!({}));
    for _ in 0..3 {
        t.step(DT);
    }
    assert!(t.take(cap).is_none(), "a held capture takes a few frames");
    assert!(t.take(status).is_none(), "the inbox is paused during a capture");
    // The clock is held by now.
    let tick0 = t.a.world.res::<Time>().tick;
    let r = t.wait(cap);
    assert_eq!(r["ok"], true, "{r}");
    let res = &r["result"];
    assert!(res["png_base64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    assert_eq!(res["width"], 64);
    assert_eq!(res["height"], 32);
    assert_eq!(res["sim_time"], 2.5);
    assert_eq!(res["stats"]["draw_calls"], 12);
    assert!(res["stats"]["tier"].is_string());
    let shot = t.drawn.iter().find(|d| d.captured).copied().unwrap();
    assert_eq!(shot.t, 2.5);
    assert!(shot.cam, "the path camera was used");
    assert_eq!(t.a.world.res::<Time>().tick, tick0, "no sim ticks while held");
    let s = t.wait(status);
    assert_eq!(s["result"]["busy"], Value::Null);
    assert!(t.overrides_clear());
    for _ in 0..10 {
        t.step(DT);
    }
    assert!(t.a.world.res::<Time>().tick > tick0, "the sim resumes");
}

/// A moment's setup marks the world so the test can see it ran.
#[derive(Debug, Default)]
struct MomentRan(u32);

fn moment_setup(w: &evani_core::ecs::World) -> Result<(), String> {
    match w.try_res_mut::<MomentRan>() {
        Some(mut m) => {
            m.0 += 1;
            Ok(())
        }
        None => Err("no marker".into()),
    }
}

#[test]
fn capture_frame_moment_runs_its_setup_holds_the_clock_and_is_listed() {
    let mut t = T::new();
    t.a.world.insert_resource(MomentRan::default());
    let mut m = evani_core::capture::CaptureMoments::default();
    m.add("drift", "A drift with sparks", moment_setup).unwrap();
    t.a.world.insert_resource(m);
    t.step(DT);
    let s = t.ok("session.status", json!({}));
    assert_eq!(s["moments"], json!([{ "name": "drift", "label": "A drift with sparks" }]));
    let cap = t.submit("capture.frame", json!({ "width": 64, "moment": "drift" }));
    for _ in 0..3 {
        t.step(DT);
    }
    let tick0 = t.a.world.res::<Time>().tick;
    let r = t.wait(cap);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["moment"]["name"], "drift");
    assert_eq!(r["result"]["moment"]["label"], "A drift with sparks");
    // The reply says which camera drew the frame (the world-edge check needs it).
    assert_eq!(r["result"]["camera"]["fov_y_deg"], 50.0);
    assert_eq!(t.a.world.res::<MomentRan>().0, 1);
    assert_eq!(t.a.world.res::<Time>().tick, tick0, "no sim ticks while held");
    let shot = t.drawn.iter().find(|d| d.captured).copied().unwrap();
    assert!(!shot.cam, "the game camera frames a moment");
    assert!(t.overrides_clear());
    assert_eq!(t.err("capture.frame", json!({ "moment": "nope" })), "unknown_target");
    assert_eq!(t.err("capture.frame", json!({ "moment": "drift", "camera": "path" })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "moment": 3 })), "invalid_value");
}

/// `hold_after` keeps the clock where a moment was shot, so a page screenshot
/// taken after the reply shows the same frame however long it takes; the next
/// run puts the clock back as it was before.
#[test]
fn capture_frame_hold_after_keeps_the_clock_held_until_the_next_run() {
    let mut t = T::new();
    t.a.world.insert_resource(MomentRan::default());
    let mut m = evani_core::capture::CaptureMoments::default();
    m.add("drift", "A drift with sparks", moment_setup).unwrap();
    t.a.world.insert_resource(m);
    t.step(DT);
    assert_eq!(t.err("capture.frame", json!({ "hold_after": true })), "invalid_value", "nothing held to keep");
    assert_eq!(t.err("capture.frame", json!({ "moment": "drift", "hold_after": 1 })), "invalid_value");
    assert!(t.overrides_clear(), "a refused request holds nothing");
    let cap = t.submit("capture.frame", json!({ "width": 64, "moment": "drift", "hold_after": true }));
    let r = t.wait(cap);
    assert_eq!(r["ok"], true, "{r}");
    let held = r["result"]["sim_time"].as_f64().unwrap();
    let tick0 = t.a.world.res::<Time>().tick;
    for _ in 0..30 {
        t.step(DT);
    }
    assert_eq!(t.a.world.res::<Time>().tick, tick0, "still held after the reply");
    assert!(!t.overrides_clear());
    assert!((t.a.world.res::<Time>().render_seconds() - held).abs() < 0.01, "the frame shown is the frame shot");
    // Another held shot keeps it; a plain one ends it.
    let cap = t.submit("capture.frame", json!({ "width": 64, "moment": "drift", "hold_after": true }));
    let r = t.wait(cap);
    assert_eq!(r["ok"], true, "{r}");
    assert!(!t.overrides_clear());
    let cap = t.submit("capture.frame", json!({ "width": 64, "moment": "drift" }));
    let r = t.wait(cap);
    assert_eq!(r["ok"], true, "{r}");
    assert!(t.overrides_clear(), "the clock is back to running");
    for _ in 0..10 {
        t.step(DT);
    }
    assert!(t.a.world.res::<Time>().tick > tick0, "the sim resumes");
    assert_eq!(t.a.world.res::<MomentRan>().0, 3);
}

#[test]
fn capture_frame_reports_the_canvas_hud_of_the_captured_frame() {
    let mut t = T::new();
    t.step(DT);
    let r = t.ok("capture.frame", json!({ "width": 64 }));
    assert!(r.get("canvas_hud").is_none(), "no canvas HUD declared: no field");
    t.canvas_hud = Some(evani_core::capture::CanvasHud { sprites: 14, rect_css: [8.0, 6.0, 300.0, 20.0], viewport_css: [640.0, 360.0] });
    let r = t.ok("capture.frame", json!({ "width": 64 }));
    assert_eq!(r["canvas_hud"], json!({ "sprites": 14, "rect_css": [8.0, 6.0, 300.0, 20.0], "viewport_css": [640.0, 360.0] }));
}

#[test]
fn capture_frame_without_a_time_takes_the_next_frame() {
    let mut t = T::new();
    let r = t.ok("capture.frame", json!({}));
    assert_eq!(r["width"], W, "never wider than the render");
    let shot = t.drawn.iter().find(|d| d.captured).copied().unwrap();
    assert!(!shot.cam);
    assert!(t.overrides_clear());
}

#[test]
fn capture_times_out_when_the_renderer_never_answers() {
    let mut t = T::new();
    t.broken = true;
    let tok = t.submit("capture.frame", json!({ "sim_time": 1.0 }));
    let r = t.wait_dt(tok, 50.0, 400);
    assert_eq!(r["error"]["code"], "timeout", "{r}");
    assert!(t.overrides_clear());
    assert!(t.a.world.res::<LiveSession>().run.is_none());
    // The inbox runs again.
    t.ok("session.status", json!({}));
}

#[test]
fn compare_bloom_on_vs_off_abba_restores_and_differs() {
    let mut t = T::new();
    assert!(t.bloom());
    let s0 = FullState::capture(&t.a.world);
    let from = t.drawn.len();
    let r = t.ok(
        "compare.ab",
        json!({ "b": { "values": { "fx.bloom.enabled": false } }, "seconds": 1,
                "warmup_s": 0.1, "capture_width": 64 }),
    );
    assert_eq!(r["order"], "ABBA");
    assert_eq!(r["a"]["snapshot"], "current");
    assert_eq!(r["b"]["snapshot"], Value::Null);
    assert_eq!(r["b"]["changes"][0]["name"], "fx.bloom.enabled");
    // Each side runs twice, 60 frames a run.
    assert_eq!(r["a"]["frames"], 120);
    assert_eq!(r["b"]["frames"], 120);
    assert_eq!(r["a"]["frame_ms"]["avg"], 16.0);
    assert_eq!(r["delta"]["frame_ms_avg"], 0.0);
    assert_eq!(r["gpu_timing"], "unavailable");
    let caps = r["captures"].as_array().unwrap();
    assert_eq!(caps.len(), 1, "default capture_at is the middle");
    assert_eq!(caps[0]["t"], 0.5);
    assert_eq!(caps[0]["width"], 64);
    assert!(caps[0]["a_png_base64"].as_str().unwrap().starts_with("iVBOR"));
    assert_ne!(caps[0]["a_png_base64"], caps[0]["b_png_base64"]);
    assert_eq!(caps[0]["diff"]["changed_pct"], 100.0);
    assert_eq!(caps[0]["diff"]["max"], 140);
    assert!(r["diff"]["psnr_db"].as_f64().unwrap() > 0.0);
    assert_eq!(r["kept"], Value::Null);
    // Both captures at the same sim time, on the path camera.
    let shots: Vec<Drawn> = t.drawn[from..].iter().filter(|d| d.captured).copied().collect();
    assert_eq!(shots.len(), 2);
    assert!(shots.iter().all(|d| d.t == 0.5 && d.cam));
    assert_eq!((shots[0].bloom, shots[1].bloom), (true, false));
    // A, B, B, A; then back to the start state.
    assert_eq!(t.bloom_segments(from), vec![true, false, true]);
    assert_eq!(FullState::capture(&t.a.world), s0, "state restored exactly");
    assert_eq!(t.a.world.res::<LiveSession>().history.len(), 0);
    assert!(t.overrides_clear());
    let progress: Vec<&Value> = t.events.iter().filter(|e| e["event"] == "bench.progress").collect();
    assert!(!progress.is_empty());
    assert!(progress.iter().any(|e| e["data"]["side"] == "b"));
}

#[test]
fn compare_ab_order_runs_each_side_once() {
    let mut t = T::new();
    let from = t.drawn.len();
    let r = t.ok(
        "compare.ab",
        json!({ "a": "current", "b": { "values": { "fx.bloom.enabled": false } },
                "seconds": 1, "warmup_s": 0, "order": "AB", "capture_at": [] }),
    );
    assert_eq!(r["a"]["frames"], 60);
    assert_eq!(r["b"]["frames"], 60);
    assert_eq!(r["captures"], json!([]));
    assert_eq!(r["diff"], Value::Null);
    assert_eq!(t.bloom_segments(from), vec![true, false, true]);
    assert!(t.bloom());
}

#[test]
fn compare_identical_sides_show_no_difference() {
    let mut t = T::new();
    let r = t.ok(
        "compare.ab",
        json!({ "b": "current", "seconds": 1, "warmup_s": 0, "order": "AB", "capture_width": 64 }),
    );
    let d = &r["captures"][0]["diff"];
    assert_eq!(d["mean_abs"], 0.0);
    assert_eq!(d["changed_pct"], 0.0);
    assert_eq!(d["psnr_db"], Value::Null, "identical images have no PSNR");
    assert_eq!(r["captures"][0]["a_png_base64"], r["captures"][0]["b_png_base64"]);
}

#[test]
fn compare_keep_ends_at_b_as_one_transaction() {
    let mut t = T::new();
    let b = json!({ "values": { "fx.bloom.enabled": false } });
    let tok = t.submit_note("compare.ab", json!({ "b": b, "keep": true }), "", json!({}));
    assert_eq!(t.wait(tok)["error"]["code"], "note_required");
    let tok = t.submit_note(
        "compare.ab",
        json!({ "b": b, "keep": true, "seconds": 1, "warmup_s": 0, "order": "AB" }),
        "Bloom costs more than it adds",
        json!({}),
    );
    let r = t.wait(tok);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["kept"]["tx"], 1);
    assert!(!t.bloom(), "kept B");
    let h = t.ok("history.list", json!({}));
    assert_eq!(h["entries"][0]["cmd"], "compare.ab");
    assert_eq!(h["entries"][0]["note"], "Bloom costs more than it adds");
    // adaptive quality is not part of what was kept
    let aq = {
        let reg = t.a.world.res::<ParamRegistry>();
        reg.value(reg.id("aq.enabled").unwrap())
    };
    let s0_aq = t.a.world.res::<LiveSession>().start_state.as_ref().and_then(|s| {
        let reg = t.a.world.res::<ParamRegistry>();
        let id = reg.id("aq.enabled")?;
        s.params.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
    });
    assert_eq!(aq, s0_aq);
    t.ok("history.undo", json!({}));
    assert!(t.bloom());
}

#[test]
fn compare_with_a_slot_side_rebuilds_and_restores_the_body() {
    let mut t = T::new();
    let body = |t: &T| {
        let s = t.a.world.res::<ShaderSlots>();
        s.get(s.id("tonemap").unwrap()).unwrap().body.clone()
    };
    let before = body(&t);
    let b = json!({ "cmds": [ { "cmd": "slot.set",
        "params": { "name": "tonemap", "source": "return color;" } } ] });
    let r = t.ok("compare.ab", json!({ "b": b, "seconds": 1, "warmup_s": 0, "order": "AB" }));
    assert_eq!(r["b"]["changes"][0]["kind"], "slot");
    assert_eq!(body(&t), before);
}

#[test]
fn bench_is_busy_for_changes_and_restores_the_tier() {
    let mut t = T::new();
    let tier0 = t.a.world.res::<Tiers>().active().id.clone();
    let s0 = FullState::capture(&t.a.world);
    let bench = t.submit(
        "bench.run",
        json!({ "seconds": 1, "warmup_s": 0, "tier": "low", "capture_at": [0.5], "capture_width": 64 }),
    );
    for _ in 0..5 {
        t.step(DT);
    }
    let set = t.submit("param.set", json!({ "values": { "core.audio.master": 0.3 } }));
    let undo = t.submit("history.undo", json!({}));
    let status = t.submit("session.status", json!({}));
    let read = t.submit("param.get", json!({ "prefix": "core.audio." }));
    t.step(DT);
    let e = t.take(set).unwrap();
    assert_eq!(e["error"]["code"], "busy", "{e}");
    assert!(e["error"]["data"]["until_ms"].as_f64().unwrap() > 0.0);
    assert_eq!(t.take(undo).unwrap()["error"]["code"], "busy");
    let s = t.take(status).unwrap();
    assert_eq!(s["result"]["busy"]["run"], "b-1");
    assert_eq!(s["result"]["busy"]["cmd"], "bench.run");
    assert_eq!(t.take(read).unwrap()["ok"], true, "reads still work");
    assert_eq!(t.a.world.res::<Tiers>().active().id, "low");

    let r = t.wait(bench);
    assert_eq!(r["ok"], true, "{r}");
    let r = &r["result"];
    assert_eq!(r["run"], "b-1");
    assert_eq!(r["tier"], "low");
    assert_eq!(r["frames"], 60);
    assert_eq!(r["frame_ms"]["p99"], 16.0);
    assert_eq!(r["pass"], true, "{r}");
    assert_eq!(r["failures"], json!([]));
    assert_eq!(r["captures"][0]["t"], 0.5);
    assert_eq!(r["captures"][0]["width"], 64);
    assert_eq!(t.a.world.res::<Tiers>().active().id, tier0);
    t.step(DT);
    assert_eq!(FullState::capture(&t.a.world), s0, "effect switches and tier restored");
    assert!(t.overrides_clear());
    t.ok("param.set", json!({ "values": { "core.audio.master": 0.3 } }));
}

#[test]
fn bench_lists_broken_limits() {
    let mut t = T::new();
    let tok = t.submit("bench.run", json!({ "seconds": 1, "warmup_s": 0, "tier": "low" }));
    let r = t.wait_dt(tok, 50.0, 400);
    assert_eq!(r["ok"], true, "{r}");
    let r = &r["result"];
    assert_eq!(r["pass"], false);
    let f = r["failures"].as_array().unwrap();
    assert!(f.contains(&json!({ "limit": "frame_ms_avg", "max": 18.33, "seen": 50.0 })), "{f:?}");
    assert!(f.contains(&json!({ "limit": "frame_ms_p99", "max": 33.33, "seen": 50.0 })), "{f:?}");
}

#[test]
fn run_params_are_checked() {
    let mut t = T::new();
    assert_eq!(t.err("capture.frame", json!({ "width": 9999 })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "camera": "drone" })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "camera": { "eye": [0, 1], "target": [0, 0, 0] } })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "camera": { "eye": [1, 1, 1], "target": [1, 1, 1] } })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "sky": [2, 0, 0] })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "jitter_px": [9, 0] })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "geometry": "yes" })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "width": 4097 })), "invalid_value");
    assert_eq!(t.err("bench.run", json!({ "capture_width": 2048 })), "invalid_value");
    assert_eq!(t.err("compare.ab", json!({})), "bad_request");
    assert_eq!(t.err("compare.ab", json!({ "b": "snap-9" })), "unknown_target");
    assert_eq!(t.err("compare.ab", json!({ "b": { "values": { "nope.x": 1 } } })), "unknown_target");
    assert_eq!(t.err("compare.ab", json!({ "b": "current", "order": "BA" })), "invalid_value");
    assert_eq!(t.err("bench.run", json!({ "tier": "potato" })), "unknown_target");
    assert_eq!(t.err("bench.run", json!({ "seconds": 0.5 })), "invalid_value");
    assert_eq!(t.err("bench.run", json!({ "seconds": 2, "capture_at": [3.0] })), "invalid_value");
    assert!(t.a.world.res::<LiveSession>().run.is_none());
}

#[test]
fn change_result_carries_a_capture_when_asked() {
    let mut t = T::new();
    let tok = t.submit_note(
        "param.set",
        json!({ "values": { "core.audio.master": 0.4 } }),
        "quieter",
        json!({ "telemetry_ms": 0, "capture": { "width": 64 } }),
    );
    let r = t.wait(tok);
    assert_eq!(r["ok"], true, "{r}");
    let c = &r["result"]["capture"];
    assert!(c["png_base64"].as_str().unwrap().starts_with("iVBOR"), "{c}");
    assert_eq!(c["width"], 64);
    assert_eq!(c["height"], 32);
    assert_eq!(r["result"]["telemetry"], Value::Null);
}

#[test]
fn with_changes_applies_new_values_without_touching_the_world() {
    let t = T::new();
    let s0 = FullState::capture(&t.a.world);
    let reg = t.a.world.res::<ParamRegistry>();
    let id = reg.id("fx.bloom.enabled").unwrap();
    drop(reg);
    let changes = vec![
        Change::Param {
            id,
            old: ParamValue::Bool(true),
            new: ParamValue::Bool(false),
        },
        Change::Tier {
            old: s0.tier.clone(),
            new: "low".into(),
        },
    ];
    let b = with_changes(&s0, &changes);
    assert_eq!(b.tier, "low");
    assert_eq!(
        b.params.iter().find(|(i, _)| *i == id).map(|(_, v)| *v),
        Some(ParamValue::Bool(false))
    );
    assert_eq!(s0.diff(&b).len(), 2);
    assert_eq!(FullState::capture(&t.a.world), s0);
}

#[test]
fn capture_frame_geocheck_options_apply_for_the_shot_and_are_restored() {
    let mut t = T::new();
    t.step(DT);
    let r = t.ok(
        "capture.frame",
        json!({ "width": 4096, "camera": { "eye": [10, 5, 10], "target": [0, 1, 0], "fov_y_deg": 40 },
                "sky": true, "jitter_px": [0.5, 0], "geometry": true }),
    );
    assert_eq!(r["width"], W, "a wide request gives the full render");
    assert_eq!(r["sky"], json!([1.0, 0.0, 1.0]));
    assert_eq!(r["jitter_px"], json!([0.5, 0.0]));
    assert!(r["geometry"]["counts"].is_object(), "{r}");
    let shot = t.drawn.iter().find(|d| d.captured).copied().unwrap();
    let pose = shot.pose.expect("the fixed camera was used");
    assert_eq!(pose.eye, [10.0, 5.0, 10.0]);
    assert_eq!(pose.target, [0.0, 1.0, 0.0]);
    assert_eq!(pose.fov_y_deg, 40.0);
    assert_eq!(shot.clear, Some([1.0, 0.0, 1.0]));
    assert_eq!(shot.jitter, Some([0.5, 0.0]));
    assert!(t.overrides_clear(), "everything is given back");
    assert!(!t.a.world.res::<crate::geometry::GeometryView>().on, "the view was off before");
    // A custom sky colour, nothing else.
    let r = t.ok("capture.frame", json!({ "sky": [0.0, 1.0, 0.0] }));
    assert_eq!(r["sky"], json!([0.0, 1.0, 0.0]));
    let shot = t.drawn.iter().rev().find(|d| d.captured).copied().unwrap();
    assert_eq!(shot.clear, Some([0.0, 1.0, 0.0]));
    assert_eq!(shot.jitter, None);
    assert!(shot.pose.is_none());
    assert!(t.overrides_clear());
}

// ---- sweep.run ------------------------------------------------------------------------

fn approx(v: &Value, want: f64) -> bool {
    v.as_f64().is_some_and(|x| (x - want).abs() < 1e-5)
}

#[test]
fn sweep_returns_one_table_and_restores_everything() {
    let mut t = T::new();
    let s0 = FullState::capture(&t.a.world);
    let from = t.drawn.len();
    let r = t.ok(
        "sweep.run",
        json!({ "param": "fx.bloom.intensity", "values": [0.2, 9.0], "frames_per_value": 30,
                "warmup_s": 0, "thumbs": { "w": 32 } }),
    );
    assert_eq!(r["run"], "sw-1");
    assert_eq!(r["path"], "bench");
    assert_eq!(r["frames_per_value"], 30);
    assert_eq!(r["restored"], true);
    assert_eq!(r["base_row"], 0, "the current value comes first");
    assert_eq!(r["gpu_timing"], "unavailable");
    assert_eq!(r["look_at"], json!([0.083, 0.25, 0.417]));
    let cols: Vec<&str> = r["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert_eq!(cols[..4], ["fx.bloom.intensity", "work_ms", "cpu_ms", "gpu_ms"]);
    assert_eq!(cols[cols.len() - 9..], crate::look::NAMES, "every look measure, bloom last");
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(row.as_array().unwrap().len(), cols.len(), "{row}");
    }
    assert!(approx(&rows[0][0], 0.3) && approx(&rows[1][0], 0.2) && approx(&rows[2][0], 4.0), "{rows:?}");
    assert_eq!(r["clamped"], json!([{ "row": 2, "name": "fx.bloom.intensity", "requested": 9.0, "applied": 4.0 }]));
    let col = |name: &str| cols.iter().position(|c| *c == name).unwrap();
    for row in rows {
        assert!(row[col("work_ms")].is_number(), "{row}");
        assert_eq!(row[col("gpu_ms")], Value::Null, "no GPU timing in the fake");
        // The fake draws bloom-on frames 140 levels redder than bloom-off ones.
        assert_eq!(row[col("bloom")], 1.0, "{row}");
        assert_eq!(row[col("clipped")], 0.0);
        assert!(row[col("mean_luma")].as_f64().unwrap() > 0.0);
    }
    let thumbs = r["thumbs"].as_array().unwrap();
    assert_eq!(thumbs.len(), 3);
    assert_eq!(thumbs[1]["row"], 1);
    assert_eq!(thumbs[1]["width"], 32);
    assert_eq!(thumbs[1]["height"], 16);
    assert!(thumbs[1]["png_base64"].as_str().unwrap().starts_with("iVBOR"));
    // One shared bloom-off reference set, then three look shots per row.
    let shots: Vec<Drawn> = t.drawn[from..].iter().filter(|d| d.captured).copied().collect();
    assert_eq!(shots.len(), 3 + 3 * 3, "{shots:?}");
    assert!(shots.iter().all(|d| d.cam), "every shot on the path camera");
    assert_eq!(shots.iter().filter(|d| !d.bloom).count(), 3);
    t.step(DT);
    assert_eq!(FullState::capture(&t.a.world), s0, "every value restored");
    assert_eq!(t.a.world.res::<LiveSession>().history.len(), 0, "no history entries");
    assert!(t.overrides_clear());
    assert!(t.a.world.res::<LiveSession>().run.is_none());
}

#[test]
fn sweep_grid_without_baseline_or_bloom() {
    let mut t = T::new();
    let s0 = FullState::capture(&t.a.world);
    let r = t.ok(
        "sweep.run",
        json!({ "grid": { "fx.bloom.threshold": [0.5, 2.0], "fx.bloom.intensity": [0.1, 0.3] },
                "frames_per_value": 12, "warmup_s": 0, "baseline": false, "bloom": false,
                "look_at": [0.1] }),
    );
    let cols = r["columns"].as_array().unwrap();
    assert_eq!(cols[0], "fx.bloom.intensity");
    assert_eq!(cols[1], "fx.bloom.threshold");
    assert!(!cols.contains(&json!("bloom")), "bloom: false drops the coverage column");
    assert!(cols.contains(&json!("clipped")));
    assert_eq!(r["base_row"], Value::Null);
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!((rows[1][0].clone(), rows[1][1].clone()), (json!(0.1), json!(2.0)), "{rows:?}");
    assert_eq!(r.get("thumbs"), None);
    // Without look times, costs only.
    let r = t.ok(
        "sweep.run",
        json!({ "param": "core.audio.master", "values": [0.2], "frames_per_value": 10,
                "warmup_s": 0, "look_at": [] }),
    );
    assert_eq!(r["columns"], json!(["core.audio.master", "work_ms", "cpu_ms", "gpu_ms"]));
    t.step(DT);
    assert_eq!(FullState::capture(&t.a.world), s0);
}

#[test]
fn sweep_is_busy_for_changes() {
    let mut t = T::new();
    let sweep = t.submit(
        "sweep.run",
        json!({ "param": "fx.bloom.intensity", "values": [0.2, 0.4], "frames_per_value": 30, "warmup_s": 0 }),
    );
    for _ in 0..5 {
        t.step(DT);
    }
    let set = t.submit("param.set", json!({ "values": { "core.audio.master": 0.3 } }));
    let other = t.submit("bench.run", json!({ "seconds": 1 }));
    let status = t.submit("session.status", json!({}));
    t.step(DT);
    let e = t.take(set).unwrap();
    assert_eq!(e["error"]["code"], "busy", "{e}");
    assert!(e["error"]["data"]["until_ms"].as_f64().unwrap() > 0.0);
    assert_eq!(t.take(other).unwrap()["error"]["code"], "busy");
    let s = t.take(status).unwrap();
    assert_eq!(s["result"]["busy"]["cmd"], "sweep.run");
    assert_eq!(t.wait(sweep)["ok"], true);
    t.ok("param.set", json!({ "values": { "core.audio.master": 0.3 } }));
}

#[test]
fn sweep_failure_names_the_row_and_still_restores() {
    let mut t = T::new();
    let s0 = FullState::capture(&t.a.world);
    t.broken = true;
    let tok = t.submit(
        "sweep.run",
        json!({ "param": "fx.bloom.intensity", "values": [0.2, 0.4], "frames_per_value": 12,
                "warmup_s": 0, "bloom": false }),
    );
    let r = t.wait_dt(tok, 50.0, 400);
    assert_eq!(r["error"]["code"], "timeout", "{r}");
    assert_eq!(r["error"]["data"]["row"], 0, "{r}");
    t.broken = false;
    t.step(DT);
    assert_eq!(FullState::capture(&t.a.world), s0, "restored after the failure");
    assert!(t.overrides_clear());
    assert!(t.a.world.res::<LiveSession>().run.is_none());
    t.ok("session.status", json!({}));
}

#[test]
fn sweep_params_are_checked() {
    let mut t = T::new();
    let r = t.call("sweep.run", json!({ "param": "nope.x", "values": [1] }));
    assert_eq!(r["error"]["code"], "unknown_target", "{r}");
    assert_eq!(r["error"]["data"]["row"], 0);
    let r = t.call("sweep.run", json!({ "grid": { "fx.bloom.intensity": [0.1, "loud"] } }));
    assert_eq!(r["error"]["code"], "invalid_value", "{r}");
    assert_eq!(r["error"]["data"]["row"], 1);
    assert_eq!(t.err("sweep.run", json!({ "param": "fx.bloom.intensity", "values": [1], "path": "orbit" })), "unknown_target");
    assert_eq!(t.err("sweep.run", json!({ "param": "aq.enabled", "values": [false] })), "invalid_value");
    assert_eq!(t.err("sweep.run", json!({ "param": "fx.bloom.intensity", "values": vec![1; 33] })), "invalid_value");
    assert_eq!(t.err("sweep.run", json!({ "param": "fx.bloom.intensity", "values": [1], "thumbs": { "w": 5 } })), "invalid_value");
    assert_eq!(t.err("sweep.run", json!({ "param": "fx.bloom.intensity", "values": [1], "look_at": [99] })), "invalid_value");
    assert_eq!(t.err("sweep.run", json!({ "param": "fx.bloom.intensity", "values": [1], "tier": "potato" })), "unknown_target");
    assert_eq!(t.err("sweep.run", json!({ "param": "fx.bloom.intensity", "values": [1], "frames_per_value": 2 })), "invalid_value");
    assert_eq!(t.err("sweep.run", json!({ "values": [1] })), "bad_request");
    assert!(t.a.world.res::<LiveSession>().run.is_none());
    assert_eq!(t.a.world.res::<LiveSession>().history.len(), 0);
}

#[test]
fn capture_frame_with_temporary_values_and_tier_restores_after_the_shot() {
    let mut t = T::new();
    t.step(DT);
    assert!(t.bloom(), "the sandbox starts with bloom on");
    let tier0 = t.a.world.res::<Tiers>().active().id.clone();
    let from = t.drawn.len();
    let r = t.ok(
        "capture.frame",
        json!({ "width": 64, "sim_time": 1.0, "values": { "fx.bloom.enabled": false }, "tier": tier0 }),
    );
    let shot = t.drawn[from..].iter().find(|d| d.captured).copied().unwrap();
    assert!(!shot.bloom, "the shot was taken with bloom off");
    assert_eq!(r["tier"], json!(tier0));
    assert_eq!(r["values"][0]["name"], "fx.bloom.enabled", "{r}");
    assert!(t.bloom(), "bloom is back on after the shot");
    assert!(t.overrides_clear());
    // Without values or tier the reply has neither.
    let r = t.ok("capture.frame", json!({ "width": 64 }));
    assert!(r.get("values").is_none() && r.get("tier").is_none());
    // Bad values are refused before anything changes.
    assert_eq!(t.err("capture.frame", json!({ "values": 3 })), "invalid_value");
    assert_eq!(t.err("capture.frame", json!({ "tier": "nope" })), "unknown_target");
    assert!(t.bloom());
}

#[test]
fn capture_frame_with_temporary_values_restores_when_the_shot_fails() {
    let mut t = T::new();
    t.step(DT);
    t.broken = true;
    let tok = t.submit("capture.frame", json!({ "sim_time": 1.0, "values": { "fx.bloom.enabled": false } }));
    let r = t.wait_dt(tok, 50.0, 400);
    assert_eq!(r["error"]["code"], "timeout", "{r}");
    assert!(t.bloom(), "bloom restored after the failed shot");
    assert!(t.a.world.res::<LiveSession>().run.is_none());
    assert!(t.overrides_clear());
}

// ---- the canvas hold during trial runs (docs/LIVE_API.md 6.30) ----------------------

impl T {
    fn held(&self) -> bool {
        self.a.world.res::<evani_core::capture::HoldCanvas>().0
    }

    /// What the canvas showed for every frame drawn since `from`.
    fn canvas_since(&self, from: usize) -> Vec<Drawn> {
        self.canvas[from..].iter().map(|c| c.expect("the canvas shows a frame")).collect()
    }
}

#[test]
fn sweep_holds_the_canvas_on_the_last_frame_and_releases_it_after() {
    let mut t = T::new();
    t.step(DT);
    let before = t.canvas.last().copied().flatten().unwrap();
    assert!(!before.held && !before.cam);
    let from = t.drawn.len();
    let tok = t.submit(
        "sweep.run",
        json!({ "param": "fx.bloom.intensity", "values": [0.05, 4.0], "frames_per_value": 20, "warmup_s": 0 }),
    );
    let mut held_frames = 0;
    while t.take(tok).is_none() {
        t.step(DT);
        // The reply comes the frame the run finishes: from then on, not held.
        held_frames += usize::from(t.held());
        assert!(held_frames < 3000, "the sweep never ended");
    }
    let drawn = &t.drawn[from..];
    // The trial values (and the path camera) were drawn, off-screen only.
    assert!(drawn.iter().any(|d| d.intensity == 4.0 && d.held));
    assert!(drawn.iter().any(|d| d.intensity == 0.05 && d.held));
    assert!(drawn.iter().filter(|d| d.cam).all(|d| d.held), "every path-camera frame held");
    assert!(drawn.iter().filter(|d| d.captured).all(|d| d.held), "every capture held");
    // The canvas only ever showed the frame from before the sweep.
    for c in t.canvas_since(from) {
        assert!(c.intensity == before.intensity && c.bloom == before.bloom && !c.cam, "the canvas showed a trial frame: {c:?}");
    }
    assert!(!t.held(), "released when the run ended");
    // Released: the next frame is presented (the restored state).
    let n = t.drawn.len();
    t.step(DT);
    assert!(!t.drawn[n].held);
    assert_eq!(t.canvas.last().copied().flatten().map(|c| c.intensity), Some(before.intensity));
    // The chosen value, applied with param.set, shows on the very next frame.
    t.ok("param.set", json!({ "values": { "fx.bloom.intensity": 1.5 } }));
    t.step(DT);
    let now = t.canvas.last().copied().flatten().unwrap();
    assert!(!now.held);
    assert_eq!(now.intensity, 1.5);
}

#[test]
fn compare_holds_the_canvas_and_shows_b_at_once_when_kept() {
    let mut t = T::new();
    t.step(DT);
    let from = t.drawn.len();
    let r = t.call(
        "compare.ab",
        json!({ "b": { "values": { "fx.bloom.enabled": false } }, "seconds": 1, "warmup_s": 0,
                "capture_width": 64 }),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert!(t.drawn[from..].iter().any(|d| !d.bloom && d.held), "B was drawn off-screen");
    assert!(t.canvas_since(from).iter().all(|c| c.bloom && !c.cam), "the canvas never showed B");
    assert!(!t.held());
    // keep: true ends on B, and B shows on the first frame after the run.
    let from = t.drawn.len();
    let tok = t.submit_note(
        "compare.ab",
        json!({ "b": { "values": { "fx.bloom.enabled": false } }, "seconds": 1, "warmup_s": 0, "keep": true }),
        "keep B",
        json!({ "telemetry_ms": 0 }),
    );
    let r = t.wait(tok);
    assert_eq!(r["ok"], true, "{r}");
    let during = t.canvas_since(from);
    assert!(during.iter().all(|c| c.bloom), "B (bloom off) never showed during the run");
    t.step(DT);
    let now = t.canvas.last().copied().flatten().unwrap();
    assert!(!now.held && !now.bloom, "the kept B shows at once: {now:?}");
}

#[test]
fn a_failed_sweep_or_shot_releases_the_canvas() {
    let mut t = T::new();
    t.step(DT);
    t.broken = true;
    let tok = t.submit(
        "sweep.run",
        json!({ "param": "fx.bloom.intensity", "values": [4.0], "frames_per_value": 12, "warmup_s": 0, "bloom": false }),
    );
    t.step(DT);
    assert!(t.held(), "held while the sweep runs");
    let r = t.wait_dt(tok, 50.0, 400);
    assert_eq!(r["error"]["code"], "timeout", "{r}");
    assert!(!t.held(), "released by the failure");
    t.broken = false;
    t.step(DT);
    let now = t.canvas.last().copied().flatten().unwrap();
    assert!(!now.held);
    assert_eq!(now.intensity, 0.3, "the restored value shows");
    // A failed shot with temporary values releases too.
    t.broken = true;
    let tok = t.submit("capture.frame", json!({ "values": { "fx.bloom.enabled": false } }));
    t.step(DT);
    assert!(t.held());
    let r = t.wait_dt(tok, 50.0, 400);
    assert_eq!(r["error"]["code"], "timeout", "{r}");
    assert!(!t.held());
    t.broken = false;
    t.step(DT);
    assert!(t.canvas.last().copied().flatten().is_some_and(|c| !c.held && c.bloom));
}

#[test]
fn only_trial_shots_hold_the_canvas() {
    let mut t = T::new();
    t.step(DT);
    // The next frame as the player sees it: never held.
    let from = t.drawn.len();
    t.ok("capture.frame", json!({ "width": 64 }));
    assert!(t.drawn[from..].iter().all(|d| !d.held));
    // Temporary values: held, and the canvas never shows them.
    let from = t.drawn.len();
    t.ok("capture.frame", json!({ "width": 64, "values": { "fx.bloom.enabled": false } }));
    assert!(t.drawn[from..].iter().any(|d| d.held && !d.bloom));
    assert!(t.canvas_since(from).iter().all(|c| c.bloom));
    // Another time or camera: held.
    let from = t.drawn.len();
    t.ok("capture.frame", json!({ "width": 64, "sim_time": 1.0, "camera": "path" }));
    assert!(t.drawn[from..].iter().filter(|d| d.captured).all(|d| d.held));
    assert!(t.canvas_since(from).iter().all(|c| !c.cam));
    assert!(!t.held());
    // A bench measures the game as shown: not held (as before).
    let from = t.drawn.len();
    t.ok("bench.run", json!({ "seconds": 1, "warmup_s": 0 }));
    assert!(t.drawn[from..].iter().all(|d| !d.held));
    // Plain changes show at once.
    t.ok("param.set", json!({ "values": { "fx.bloom.enabled": false } }));
    t.step(DT);
    assert!(t.canvas.last().copied().flatten().is_some_and(|c| !c.held && !c.bloom));
}
