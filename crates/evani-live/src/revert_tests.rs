//! Telemetry, deferred change results, auto-revert and subscriptions on a
//! native app driven by a fake frame-time series (`run_headless` sets
//! `Time.frame_dt` to the step it is given).

use evani_core::App;
use evani_core::capture::DeviceStatus;
use evani_core::overlay::RenderStats;
use evani_core::params::ParamRegistry;
use evani_core::quality::QualityState;
use serde_json::{Value, json};

use crate::telemetry::RING;
use crate::{LiveModule, LiveSession, submit, take_events, take_replies};

const FAST: f64 = 16.0;
const PARAM: &str = "core.audio.master";

struct H {
    a: App,
    token: u64,
    events: Vec<Value>,
}

impl H {
    fn new() -> Self {
        let mut a = App::new("sandbox")
            .with_kit(evani_kit_example::ExampleKitPlugin)
            .with_look(evani_look_basic::BasicLook)
            .with_module(evani_fx::FxModule::compiled())
            .with_module(LiveModule::new());
        a.run_headless(2, FAST);
        if !a.world.has_resource::<DeviceStatus>() {
            a.world.insert_resource(DeviceStatus::default());
        }
        Self {
            a,
            token: 0,
            events: Vec::new(),
        }
    }

    fn now(&self) -> f64 {
        self.a.world.res::<evani_core::time::Time>().now_ms
    }

    /// Run frames of `dt` ms whose GPU work fills the frame (so they are as
    /// slow as their interval), collecting events.
    fn run(&mut self, frames: u32, dt: f64) {
        self.run_work(frames, dt, dt);
    }

    /// Run frames of `dt` ms with `work` ms of GPU work each (a light frame
    /// on a slow vsync has `work` well under `dt`).
    fn run_work(&mut self, frames: u32, dt: f64, work: f64) {
        for _ in 0..frames {
            self.a.world.res_mut::<RenderStats>().gpu_ms = Some(work as f32);
            self.a.run_headless(1, dt);
            self.collect();
        }
    }

    fn collect(&mut self) {
        for e in take_events(&self.a.world) {
            self.events.push(serde_json::from_str(&e).unwrap());
        }
    }

    fn submit(&mut self, cmd: &str, params: Value, opts: Value) -> u64 {
        self.token += 1;
        let req = json!({ "type": "cmd", "v": 1, "id": format!("r-{}", self.token), "cmd": cmd,
                          "params": params, "note": "Try louder", "source": "mcp", "opts": opts });
        submit(&self.a.world, self.token, &req.to_string());
        self.token
    }

    /// Run frames of `dt` until `token` answers; returns (reply, ms waited).
    fn wait(&mut self, token: u64, dt: f64, max_frames: u32) -> (Value, f64) {
        let start = self.now();
        for _ in 0..max_frames {
            self.run(1, dt);
            let mut replies = take_replies(&self.a.world);
            if let Some(i) = replies.iter().position(|(t, _)| *t == token) {
                let (_, text) = replies.swap_remove(i);
                return (serde_json::from_str(&text).unwrap(), self.now() - start);
            }
        }
        panic!("no reply to token {token}");
    }

    fn set(&mut self, v: f64, opts: Value, dt: f64) -> Value {
        let t = self.submit("param.set", json!({ "values": { PARAM: v } }), opts);
        let (r, _) = self.wait(t, dt, 400);
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }

    fn value(&self) -> f64 {
        let reg = self.a.world.res::<ParamRegistry>();
        f64::from(reg.f32(reg.f32_handle(PARAM).unwrap()))
    }

    fn events_named(&self, name: &str) -> Vec<Value> {
        self.events
            .iter()
            .filter(|e| e["event"] == name)
            .cloned()
            .collect()
    }
}

#[test]
fn slow_frames_after_a_change_auto_revert_after_sustain() {
    let mut h = H::new();
    h.run(90, FAST);
    let before = h.value();
    let res = h.set(0.3, json!({ "telemetry_ms": 0 }), FAST);
    let w = &res["watch"];
    assert_eq!(w["auto_revert"], "armed", "{res}");
    assert_eq!(w["sustain_s"], 3.0);
    assert_eq!(w["window_s"], 10.0);
    let target = w["target_ms"].as_f64().unwrap();
    assert!((w["limit_ms"].as_f64().unwrap() - target * 1.25).abs() < 0.02);
    let tx = res["tx"].as_u64().unwrap();
    assert!((h.value() - 0.3).abs() < 1e-6);
    h.run(10, FAST);

    let slow = target * 2.4;
    let start = h.now();
    let mut reverted_at = None;
    for _ in 0..400 {
        h.run(1, slow);
        if !h.events_named("auto_revert").is_empty() {
            reverted_at = Some(h.now() - start);
            break;
        }
    }
    let after = reverted_at.expect("no auto_revert");
    assert!(
        (3000.0..=3500.0).contains(&after),
        "reverted after {after} ms"
    );
    assert!((h.value() - before).abs() < 1e-6, "value restored");

    let ev = &h.events_named("auto_revert")[0];
    assert_eq!(ev["type"], "event");
    assert_eq!(ev["v"], 1);
    let d = &ev["data"];
    assert_eq!(d["tx"], tx);
    assert_eq!(d["reason"], "frame_time");
    assert_eq!(d["reverted_tx"], tx + 1);
    assert_eq!(d["note"], "Try louder");
    assert_eq!(d["detail"]["sustain_s"], 3.0);
    assert!(
        d["detail"]["observed_ms"].as_f64().unwrap() > d["detail"]["limit_ms"].as_f64().unwrap()
    );
    assert!(d["detail"]["over_pct"].as_f64().unwrap() > 25.0);
    assert_eq!(d["undone"][0]["kind"], "param");
    assert_eq!(d["undone"][0]["name"], PARAM);
    assert!((d["undone"][0]["restored"].as_f64().unwrap() - before).abs() < 1e-6);

    let s = h.a.world.res::<LiveSession>();
    let orig = s.history.iter().find(|t| t.tx == tx).unwrap();
    assert_eq!(orig.status.as_str(), "auto_reverted");
    assert_eq!(orig.revert_reason.as_deref(), Some("frame_time"));
    let rev = s.history.last().unwrap();
    assert_eq!(rev.tx, tx + 1);
    assert_eq!(rev.cmd, "auto_revert");
    assert!(s.watch.is_none());
    drop(s);
    // Mirror after the change and after the revert.
    let mirrors = h.events_named("session.mirror");
    assert!(mirrors.len() >= 2);
    assert_eq!(mirrors.last().unwrap()["data"]["head"], tx + 1);
}

#[test]
fn slow_vsync_frames_with_light_work_do_not_revert() {
    let mut h = H::new();
    h.run(90, FAST);
    let target = crate::revert::target_ms(&h.a.world) as f64;
    let res = h.set(0.3, json!({ "telemetry_ms": 0 }), FAST);
    assert_eq!(res["watch"]["auto_revert"], "armed", "{res}");
    // A 30 Hz display: long intervals, but only 4 ms of work a frame.
    h.run_work(400, target * 2.4, 4.0);
    assert!(h.events_named("auto_revert").is_empty());
    assert!((h.value() - 0.3).abs() < 1e-6);
}

#[test]
fn held_frames_are_not_judged() {
    let mut h = H::new();
    h.run(90, FAST);
    let target = crate::revert::target_ms(&h.a.world) as f64;
    let res = h.set(0.3, json!({ "telemetry_ms": 0 }), FAST);
    assert_eq!(res["watch"]["auto_revert"], "armed", "{res}");
    // A capture stalls frames on purpose: they are marked held.
    for _ in 0..300 {
        h.a.world.res_mut::<QualityState>().held = true;
        h.run(1, target * 2.4);
    }
    assert!(h.events_named("auto_revert").is_empty());
    assert!((h.value() - 0.3).abs() < 1e-6);
    let held = h.a.world.res::<LiveSession>().tel.latest().unwrap().held;
    assert!(held);
}

#[test]
fn already_slow_before_a_change_is_not_armed() {
    let mut h = H::new();
    h.run(30, FAST);
    let target = crate::revert::target_ms(&h.a.world) as f64;
    let slow = target * 2.4;
    h.run(40, slow);
    let res = h.set(0.3, json!({ "telemetry_ms": 0 }), slow);
    assert_eq!(res["watch"]["auto_revert"], "off", "{res}");
    assert_eq!(res["watch"]["reason"], "baseline_over");
    h.run(300, slow);
    assert!(h.events_named("auto_revert").is_empty());
    assert!((h.value() - 0.3).abs() < 1e-6);
}

#[test]
fn device_lost_after_a_change_reverts() {
    let mut h = H::new();
    h.run(70, FAST);
    let before = h.value();
    let res = h.set(0.3, json!({ "telemetry_ms": 0 }), FAST);
    assert_eq!(res["watch"]["auto_revert"], "armed");
    h.run(5, FAST);
    h.a.world.res_mut::<DeviceStatus>().lost_count += 1;
    h.run(2, FAST);
    let evs = h.events_named("auto_revert");
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["data"]["reason"], "device_lost");
    assert!((h.value() - before).abs() < 1e-6);
}

#[test]
fn mutating_result_waits_for_telemetry_window() {
    let mut h = H::new();
    h.run(70, FAST);
    let t = h.submit("param.set", json!({ "values": { PARAM: 0.4 } }), json!({}));
    let (r, waited) = h.wait(t, FAST, 200);
    assert_eq!(r["ok"], true);
    assert!((1000.0..=1100.0).contains(&waited), "waited {waited} ms");
    let tel = &r["result"]["telemetry"];
    assert!(tel["frames"].as_u64().unwrap() >= 55, "{tel}");
    assert!((tel["frame_ms"]["avg"].as_f64().unwrap() - FAST).abs() < 0.01);
    assert!((tel["baseline_frame_ms"].as_f64().unwrap() - FAST).abs() < 0.01);
    assert!(tel.get("gpu_timing").is_some());
    assert!(tel.get("cpu_ms").is_some());
    assert!(tel.get("draw_calls").is_some());
    assert!(r["result"]["budget"]["switched_off"].is_array());

    let t = h.submit(
        "param.set",
        json!({ "values": { PARAM: 0.5 } }),
        json!({ "telemetry_ms": 0 }),
    );
    let (r, waited) = h.wait(t, FAST, 5);
    assert!(waited <= FAST + 0.1, "waited {waited} ms");
    assert!(r["result"]["telemetry"].is_null());

    // An explicit window.
    let t = h.submit(
        "param.set",
        json!({ "values": { PARAM: 0.6 } }),
        json!({ "telemetry_ms": 250 }),
    );
    let (_, waited) = h.wait(t, FAST, 100);
    assert!((250.0..=300.0).contains(&waited), "waited {waited} ms");
}

#[test]
fn subscribe_pushes_at_rate_and_unsubscribe_stops() {
    let mut h = H::new();
    h.run(10, FAST);
    let t = h.submit("telemetry.subscribe", json!({ "rate_hz": 4 }), json!({}));
    let (r, _) = h.wait(t, FAST, 5);
    assert_eq!(r["result"]["sub"], "t-1", "{r}");
    assert_eq!(r["result"]["rate_hz"], 4.0);
    h.events.clear();
    h.run(125, FAST); // 2 s
    let evs = h.events_named("telemetry");
    assert!((7..=9).contains(&evs.len()), "{} events", evs.len());
    let e = &evs[0];
    assert_eq!(e["sub"], "t-1");
    let d = &e["data"];
    for k in [
        "t",
        "frame",
        "tier",
        "fps",
        "frame_ms",
        "cpu_ms",
        "gpu_ms",
        "draw_calls",
        "triangles",
        "sprites",
        "budget",
        "render_scale",
    ] {
        assert!(d.get(k).is_some(), "missing {k} in {d}");
    }
    assert!((d["fps"].as_f64().unwrap() - 62.5).abs() < 0.1);
    assert!(d["budget"].get("limit").is_some());

    // Field filter.
    let t = h.submit(
        "telemetry.subscribe",
        json!({ "rate_hz": 10, "fields": ["frame_ms"] }),
        json!({}),
    );
    let (r, _) = h.wait(t, FAST, 5);
    assert_eq!(r["result"]["sub"], "t-2");
    let bad = h.submit("telemetry.subscribe", json!({ "rate_hz": 100 }), json!({}));
    let (r, _) = h.wait(bad, FAST, 5);
    assert_eq!(r["ok"], false);

    for sub in ["t-1", "t-2"] {
        let t = h.submit("telemetry.unsubscribe", json!({ "sub": sub }), json!({}));
        let (r, _) = h.wait(t, FAST, 5);
        assert_eq!(r["result"]["stopped"], true);
    }
    h.events.clear();
    h.run(60, FAST);
    assert!(h.events_named("telemetry").is_empty());
}

#[test]
fn sampling_uses_a_fixed_ring() {
    let mut h = H::new();
    let (cap, scratch) = {
        let s = h.a.world.res::<LiveSession>();
        (s.tel.capacity(), s.tel.scratch_capacity())
    };
    assert_eq!(cap, RING);
    h.run(RING as u32 + 300, FAST);
    let s = h.a.world.res::<LiveSession>();
    assert_eq!(s.tel.capacity(), cap);
    assert_eq!(s.tel.len(), RING);
    assert_eq!(s.tel.scratch_capacity(), scratch);
    // One sample per frame, newest last.
    let frames: Vec<u64> = s.tel.iter().map(|x| x.frame).collect();
    assert!(frames.windows(2).all(|w| w[1] == w[0] + 1));
}

#[test]
fn effect_toggle_reports_over_budget_when_the_budget_keeps_it_off() {
    use evani_core::effects::{EffectState, EffectsStatus};

    let mut h = H::new();
    h.run(2, FAST);
    {
        let mut fx = h.a.world.res_mut::<EffectsStatus>();
        let bloom = fx
            .effects
            .iter_mut()
            .find(|e| e.info.name == "bloom")
            .unwrap();
        bloom.state = EffectState::OffByBudget;
    }
    let mut d = crate::revert::Deferred {
        token: 1,
        id: json!("x"),
        result: json!({
            "applied": [
                { "kind": "effect", "name": "bloom", "scope": "session", "old": false, "new": true },
                { "kind": "param", "name": "fx.bloom.enabled", "old": false, "new": true },
                { "kind": "effect", "name": "bloom", "scope": "session", "old": true, "new": false }
            ],
            "budget": {}
        }),
        commit_ms: 0.0,
        frame: 0,
        telemetry_ms: 0.0,
        baseline: None,
        fx_before: vec![("bloom", EffectState::OffByRequest)],
        capture: None,
        filled: false,
    };
    crate::revert::fill_budget(&h.a.world, &mut d);
    let applied = d.result["applied"].as_array().unwrap();
    assert_eq!(applied[0]["reason"], "over_budget", "{applied:?}");
    assert_eq!(applied[1]["reason"], "over_budget", "{applied:?}");
    // Turning it off is not "asked on but stayed off".
    assert!(applied[2].get("reason").is_none(), "{applied:?}");
}
