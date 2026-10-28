//! Transaction, history, snapshot, batch and slot tests on a native app with
//! the example kit, the basic look and the compiled effects.

use evani_core::App;
use evani_core::formulas::{FormulaJson, Formulas};
use evani_core::slots::{
    SLOT_HEADER_LINES, ShaderSlots, SlotCompileResult, SlotMessage, SlotRebuilds,
};
use serde_json::{Value, json};

use crate::{FullState, LiveModule, LiveSession, submit, take_replies};

const CURVE: &str = "kit.example.spin_falloff";
const EXPR: &str = "kit.example.boost_scale";

fn app() -> App {
    let mut a = App::new("sandbox")
        .with_kit(evani_kit_example::ExampleKitPlugin)
        .with_look(evani_look_basic::BasicLook)
        .with_module(evani_fx::FxModule::compiled())
        .with_module(LiveModule::new());
    a.run_headless(2, 16.0);
    a
}

/// A stand-in renderer: parse the spliced slot with naga, swap the body in
/// on success, report errors in body coordinates.
fn fake_renderer(a: &App) {
    let reqs = a.world.res_mut::<SlotRebuilds>().take_pending();
    for r in reqs {
        let sig = a.world.res::<ShaderSlots>().get(r.slot).unwrap().signature;
        let src = ShaderSlots::assemble(&sig, &r.body);
        let result = match naga::front::wgsl::parse_str(&src) {
            Ok(_) => {
                a.world
                    .res_mut::<ShaderSlots>()
                    .set_body(r.slot, &r.body)
                    .unwrap();
                SlotCompileResult {
                    ok: true,
                    ms: 1.0,
                    kept: None,
                    messages: vec![],
                    pipelines_rebuilt: vec![format!("post.{}", sig.name)],
                }
            }
            Err(e) => {
                let loc = e.location(&src);
                let line = loc.map_or(0, |l| {
                    l.line_number.saturating_sub(SLOT_HEADER_LINES as u32)
                });
                SlotCompileResult {
                    ok: false,
                    ms: 1.0,
                    kept: Some("previous"),
                    messages: vec![SlotMessage {
                        severity: "error",
                        text: e.message().to_string(),
                        line,
                        column: loc.map_or(0, |l| l.line_position),
                        length: loc.map_or(0, |l| l.length),
                        source_line: r
                            .body
                            .lines()
                            .nth(line.saturating_sub(1) as usize)
                            .map(str::to_string),
                    }],
                    pipelines_rebuilt: vec![],
                }
            }
        };
        a.world.res_mut::<SlotRebuilds>().complete(r.handle, result);
    }
}

struct T {
    a: App,
    token: u64,
    renderer: bool,
}

impl T {
    fn new() -> Self {
        Self {
            a: app(),
            token: 0,
            renderer: false,
        }
    }

    /// Send one command, run frames until it answers, then let it settle.
    fn send(&mut self, cmd: &str, params: Value) -> Value {
        self.token += 1;
        let req = json!({ "type": "cmd", "v": 1, "id": format!("t-{}", self.token), "cmd": cmd,
                          "params": params, "note": format!("test {cmd}"), "source": "mcp",
                          "opts": { "telemetry_ms": 0 } });
        submit(&self.a.world, self.token, &req.to_string());
        for _ in 0..40 {
            self.a.run_headless(1, 16.0);
            if self.renderer {
                fake_renderer(&self.a);
            }
            let mut replies = take_replies(&self.a.world);
            if let Some(i) = replies.iter().position(|(t, _)| *t == self.token) {
                let (_, text) = replies.swap_remove(i);
                // Two more frames: the effects module settles a tier change.
                self.a.run_headless(2, 16.0);
                return serde_json::from_str(&text).unwrap();
            }
        }
        panic!("no reply to {cmd}");
    }

    fn ok(&mut self, cmd: &str, params: Value) -> Value {
        let r = self.send(cmd, params.clone());
        assert_eq!(r["ok"], true, "{cmd} {params} -> {r}");
        r["result"].clone()
    }

    fn err(&mut self, cmd: &str, params: Value) -> Value {
        let r = self.send(cmd, params.clone());
        assert_eq!(r["ok"], false, "{cmd} {params} should fail -> {r}");
        r["error"].clone()
    }

    fn state(&self) -> FullState {
        FullState::capture(&self.a.world)
    }

    fn formula(&self, name: &str) -> FormulaJson {
        let f = self.a.world.res::<Formulas>();
        f.to_json_body(f.id(name).unwrap()).unwrap()
    }

    fn history_len(&self) -> usize {
        self.a.world.res::<LiveSession>().history.len()
    }
}

#[test]
fn undo_redo_across_mixed_changes_restores_every_value() {
    let mut t = T::new();
    let mut states = vec![t.state()];
    let cmds: Vec<(&str, Value)> = vec![
        (
            "param.set",
            json!({ "values": { "core.audio.master": 0.5 } }),
        ),
        (
            "policy.set",
            json!({ "set": { "hysteresis_pct": 15, "weights": { "bloom": 0.4 } } }),
        ),
        (
            "slot.set",
            json!({ "name": EXPR, "source": "mix(1, boost * 2, held)" }),
        ),
        (
            "curve.set",
            json!({ "curves": { CURVE: [[0.5, 1.0], [1.0, 2.0], [1.5, 0.5]] } }),
        ),
        ("effect.toggle", json!({ "effect": "bloom", "on": false })),
        ("tier.set", json!({ "tier": "low" })),
        (
            "effect.toggle",
            json!({ "effect": "vignette", "on": false, "scope": "tier" }),
        ),
        (
            "param.set",
            json!({ "values": { "core.audio.master": 0.25 } }),
        ),
        ("tier.set", json!({ "tier": "high" })),
        ("effect.toggle", json!({ "effect": "bloom", "on": false })),
        (
            "slot.set",
            json!({ "name": "tonemap", "source": "return color / (vec3<f32>(1.0) + color);" }),
        ),
        (
            "curve.set",
            json!({ "curves": { CURVE: [[0.5, 0.0], [1.5, 3.0]] } }),
        ),
        (
            "policy.set",
            json!({ "set": { "render_scale_floor": 0.85 } }),
        ),
        (
            "slot.set",
            json!({ "name": EXPR, "source": "held + boost" }),
        ),
        (
            "batch",
            json!({ "cmds": [
            { "cmd": "param.set", "params": { "values": { "core.audio.master": 0.9 } } },
            { "cmd": "curve.set", "params": { "curves": { CURVE: [[0.6, 1.0], [1.4, 1.0]] } } },
            { "cmd": "effect.toggle", "params": { "effect": "vignette", "on": true } } ] }),
        ),
        ("tier.set", json!({ "tier": "medium" })),
        (
            "effect.toggle",
            json!({ "effect": "bloom", "on": false, "scope": "tier" }),
        ),
        (
            "param.set",
            json!({ "values": { "core.audio.master": 0.1 } }),
        ),
        (
            "policy.set",
            json!({ "set": { "weights": { "vignette": 0.9 } } }),
        ),
        (
            "batch",
            json!({ "cmds": [
            { "cmd": "tier.set", "params": { "tier": "low" } },
            { "cmd": "effect.toggle", "params": { "effect": "bloom", "on": true } } ] }),
        ),
        (
            "slot.set",
            json!({ "name": "tonemap", "source": "return color;" }),
        ),
    ];
    for (i, (cmd, p)) in cmds.iter().enumerate() {
        let r = t.ok(cmd, p.clone());
        assert_eq!(r["tx"], (i + 1) as u64, "{cmd}");
        assert!(r["applied"].is_array());
        let s = t.state();
        assert_ne!(
            s,
            *states.last().unwrap(),
            "step {i} {cmd} changed nothing: {r}"
        );
        states.push(s);
    }
    let n = cmds.len();
    assert!(n >= 20);
    let list = t.ok("history.list", json!({ "limit": 50 }));
    assert_eq!(list["head"], n as u64);
    assert_eq!(list["entries"].as_array().unwrap().len(), n);
    assert_eq!(list["entries"][0]["source"], "mcp");

    for k in (0..n).rev() {
        let r = t.ok("history.undo", json!({}));
        assert_eq!(r["tx"], (k + 1) as u64);
        assert_eq!(r["watch"]["auto_revert"], "off");
        assert_eq!(r["watch"]["reason"], "history");
        assert_eq!(r["history"]["can_redo"], true);
        assert_eq!(
            t.state(),
            states[k],
            "after undoing tx {} ({})",
            k + 1,
            cmds[k].0
        );
    }
    assert_eq!(t.err("history.undo", json!({}))["code"], "history_empty");
    for k in 1..=n {
        let r = t.ok("history.redo", json!({}));
        assert_eq!(r["tx"], k as u64);
        assert_eq!(
            t.state(),
            states[k],
            "after redoing tx {k} ({})",
            cmds[k - 1].0
        );
    }
    assert_eq!(t.err("history.redo", json!({}))["code"], "history_empty");

    // Undo 3, then a new change drops the redo tail.
    let r = t.ok("history.undo", json!({ "steps": 3 }));
    assert_eq!(r["history"]["head"], (n - 3) as u64);
    assert_eq!(t.state(), states[n - 3]);
    let r = t.ok(
        "param.set",
        json!({ "values": { "core.audio.master": 0.33 } }),
    );
    assert_eq!(r["tx"], (n + 1) as u64);
    assert_eq!(t.history_len(), n - 2);
    assert_eq!(t.err("history.redo", json!({}))["code"], "history_empty");
    let list = t.ok("history.list", json!({ "limit": 1 }));
    assert_eq!(list["entries"][0]["tx"], (n + 1) as u64);
    assert_eq!(list["head"], (n + 1) as u64);
    assert_eq!(list["can_redo"], false);
    assert_eq!(
        t.err("history.undo", json!({ "steps": 51 }))["code"],
        "invalid_value"
    );
}

#[test]
fn expression_slot_error_keeps_old_formula_and_reports_line_col() {
    let mut t = T::new();
    let before = t.formula(EXPR);
    let e = t.err(
        "slot.set",
        json!({ "name": EXPR, "source": "mix(1,\n  boost +* 2, held)" }),
    );
    assert_eq!(e["code"], "compile_error");
    let d = &e["data"];
    assert_eq!(d["ok"], false);
    assert_eq!(d["kept"], "previous");
    let m = &d["messages"][0];
    assert_eq!(m["severity"], "error");
    assert_eq!(m["line"], 2);
    assert!(m["col"].as_u64().unwrap() >= 3, "{m}");
    assert_eq!(m["source_line"], "  boost +* 2, held)");
    assert_eq!(t.formula(EXPR), before);
    assert_eq!(t.history_len(), 0);

    // Unknown names and curve/slot mix-ups.
    assert_eq!(
        t.err("slot.set", json!({ "name": "nope", "source": "1" }))["code"],
        "unknown_target"
    );
    assert_eq!(
        t.err("slot.set", json!({ "name": CURVE, "source": "1" }))["code"],
        "invalid_value"
    );

    // A good one, then slot.get sees it.
    let r = t.ok(
        "slot.set",
        json!({ "name": EXPR, "source": "held * boost" }),
    );
    assert_eq!(r["applied"][0]["kind"], "slot");
    assert_eq!(r["compile"]["ok"], true);
    let g = t.ok("slot.get", json!({ "name": EXPR }));
    assert_eq!(g["lang"], "expr");
    assert_eq!(g["source"], "held * boost");
    assert_eq!(g["is_default"], false);
}

#[test]
fn invalid_curves_keep_the_old_curve() {
    let mut t = T::new();
    let before = t.formula(CURVE);
    for pts in [
        json!([[1.0, 1.0]]),
        json!([[1.0, 1.0], [0.8, 2.0]]),
        json!([[1.0, 1.0], [1.0, 2.0]]),
        json!(
            (0..65)
                .map(|i| [0.5 + i as f64 * 0.01, 1.0])
                .collect::<Vec<_>>()
        ),
        json!([[0.1, 1.0], [0.2, 1.0]]), // both clamp to x = 0.5
        json!([[0.6, "a"], [0.9, 1.0]]),
    ] {
        let e = t.err("curve.set", json!({ "curves": { CURVE: pts } }));
        assert_eq!(e["code"], "invalid_value", "{pts}");
        assert_eq!(t.formula(CURVE), before);
    }
    assert_eq!(
        t.err(
            "curve.set",
            json!({ "curves": { "kit.nope": [[0, 0], [1, 1]] } })
        )["code"],
        "unknown_target"
    );
    assert_eq!(t.history_len(), 0);

    // Clamped into x 0.5-1.5, y 0-3.
    let r = t.ok(
        "curve.set",
        json!({ "curves": { CURVE: [[0.0, 5.0], [2.0, 1.0]] } }),
    );
    assert_eq!(r["applied"][0]["clamped"], true);
    assert_eq!(r["applied"][0]["new"], json!([[0.5, 3.0], [1.5, 1.0]]));
    let g = t.ok("curve.get", json!({ "name": CURVE }));
    assert_eq!(g["points"], json!([[0.5, 3.0], [1.5, 1.0]]));
    assert_eq!(g["x"]["min"], 0.5);
    assert_eq!(g["max_points"], 64);
}

#[test]
fn wgsl_slot_compiles_through_renderer_and_errors_map_to_body_lines() {
    let mut t = T::new();
    t.renderer = true;
    let old = t.ok("slot.get", json!({ "name": "tonemap" }))["source"].clone();

    let good = "let x = color * 1.5;\nreturn x / (vec3<f32>(1.0) + x);";
    let r = t.ok("slot.set", json!({ "name": "tonemap", "source": good }));
    assert_eq!(r["compile"]["ok"], true);
    assert_eq!(r["compile"]["pipelines_rebuilt"], json!(["post.tonemap"]));
    assert!(r["compile"].get("checked").is_none());
    assert_eq!(r["applied"][0]["kind"], "slot");
    assert_eq!(
        t.ok("slot.get", json!({ "name": "tonemap" }))["source"],
        good
    );

    let bad = "let x = color * 1.5;\nreturn x / (vec3<f32>(1.0) + bb);";
    let e = t.err("slot.set", json!({ "name": "tonemap", "source": bad }));
    assert_eq!(e["code"], "compile_error");
    assert_eq!(e["data"]["kept"], "previous");
    let m = &e["data"]["messages"][0];
    assert_eq!(m["line"], 2, "{m}");
    assert_eq!(m["source_line"], "return x / (vec3<f32>(1.0) + bb);");
    assert_eq!(
        t.ok("slot.get", json!({ "name": "tonemap" }))["source"],
        good
    );
    assert_eq!(t.history_len(), 1);

    // A batch whose slot part fails changes nothing and names the part.
    let master = t.ok("param.get", json!({ "names": ["core.audio.master"] }))["values"].clone();
    let e = t.err(
        "batch",
        json!({ "cmds": [
        { "cmd": "param.set", "params": { "values": { "core.audio.master": 0.2 } } },
        { "cmd": "slot.set", "params": { "name": "tonemap", "source": bad } } ] }),
    );
    assert_eq!(e["code"], "compile_error");
    assert_eq!(e["data"]["index"], 1);
    assert_eq!(
        t.ok("param.get", json!({ "names": ["core.audio.master"] }))["values"],
        master
    );
    assert_eq!(t.history_len(), 1);

    // Undo puts the old body straight back (no compile to wait for) and
    // queues a rebuild so the GPU pipelines follow it.
    t.renderer = false; // keep what undo queues where this test can see it
    t.a.world.res_mut::<evani_core::slots::SlotRebuilds>().take_pending();
    t.ok("history.undo", json!({}));
    assert_eq!(
        t.ok("slot.get", json!({ "name": "tonemap" }))["source"],
        old
    );
    let queued = t.a.world.res_mut::<evani_core::slots::SlotRebuilds>().take_pending();
    assert!(
        queued.iter().any(|r| Some(r.body.as_str()) == old.as_str()),
        "undo must rebuild the pipelines with the old body: {queued:?}"
    );
}

#[test]
fn wgsl_slot_without_renderer_gets_structural_ok() {
    let mut t = T::new();
    let body = "return color * 0.5;";
    let f0 = t.a.world.res::<evani_core::time::Time>().frame;
    let r = t.ok("slot.set", json!({ "name": "tonemap", "source": body }));
    assert_eq!(r["compile"]["ok"], true);
    assert_eq!(r["compile"]["checked"], "structural");
    assert!(r["frame"].as_u64().unwrap() >= f0 + 3);
    assert_eq!(
        t.ok("slot.get", json!({ "name": "tonemap" }))["source"],
        body
    );
    // Structural errors fail at once.
    assert_eq!(
        t.err(
            "slot.set",
            json!({ "name": "tonemap", "source": "return color; }" })
        )["code"],
        "compile_error"
    );
}

#[test]
fn batch_is_atomic_and_reports_the_failing_part() {
    let mut t = T::new();
    let s0 = t.state();
    let e = t.err(
        "batch",
        json!({ "cmds": [
        { "cmd": "param.set", "params": { "values": { "core.audio.master": 0.1 } } },
        { "cmd": "curve.set", "params": { "curves": { CURVE: [[1.0, 1.0]] } } } ] }),
    );
    assert_eq!(e["code"], "invalid_value");
    assert_eq!(e["data"]["index"], 1);
    let e = t.err(
        "batch",
        json!({ "cmds": [
        { "cmd": "tier.set", "params": { "tier": "low" } },
        { "cmd": "param.set", "params": { "values": { "fx.blom.intensity": 1.0 } } } ] }),
    );
    assert_eq!(e["code"], "unknown_target");
    assert_eq!(e["data"]["index"], 1);
    assert!(e["data"]["did_you_mean"].is_array());
    let e = t.err(
        "batch",
        json!({ "cmds": [ { "cmd": "schema.get", "params": {} } ] }),
    );
    assert_eq!(e["data"]["index"], 0);
    assert_eq!(t.state(), s0);
    assert_eq!(t.history_len(), 0);

    let r = t.ok(
        "batch",
        json!({ "cmds": [
        { "cmd": "param.set", "params": { "values": { "core.audio.master": 0.1 } } },
        { "cmd": "policy.set", "params": { "set": { "hysteresis_pct": 99 } } } ] }),
    );
    assert_eq!(r["tx"], 1);
    assert_eq!(r["applied"].as_array().unwrap().len(), 2);
    assert_eq!(r["applied"][1]["kind"], "policy");
    assert_eq!(r["applied"][1]["clamped"], true);
    assert_eq!(t.history_len(), 1);
}

#[test]
fn policy_get_set_and_schema_shape() {
    let mut t = T::new();
    let p = t.ok("policy.get", json!({}));
    assert!(p["target_ms"].as_f64().unwrap() > 0.0);
    assert_eq!(p["degrade_order"], "value_per_point");
    assert!(p["weights"]["bloom"].is_number());
    assert_eq!(
        t.err(
            "policy.set",
            json!({ "set": { "degrade_order": ["bloom"] } })
        )["code"],
        "invalid_value"
    );
    assert_eq!(
        t.err("policy.set", json!({ "set": { "nope": 1 } }))["code"],
        "unknown_target"
    );
    assert_eq!(
        t.err(
            "policy.set",
            json!({ "set": { "weights": { "sparkle": 1 } } })
        )["code"],
        "unknown_target"
    );
    t.ok(
        "policy.set",
        json!({ "set": { "target_ms": 20, "weights": { "vignette": 0.1 } } }),
    );
    let p = t.ok("policy.get", json!({}));
    assert_eq!(p["target_ms"], 20.0);
    assert!((p["weights"]["vignette"].as_f64().unwrap() - 0.1).abs() < 1e-6);

    let s = t.ok("schema.get", json!({}));
    assert!(s.get("formulas").is_none());
    let slots = s["slots"].as_array().unwrap();
    assert!(
        slots
            .iter()
            .any(|x| x["name"] == "tonemap" && x["lang"] == "wgsl")
    );
    assert!(
        slots
            .iter()
            .any(|x| x["name"] == EXPR && x["lang"] == "expr")
    );
    let curves = s["curves"].as_array().unwrap();
    assert!(
        curves
            .iter()
            .any(|x| x["name"] == CURVE && x["x"]["max"] == 1.5)
    );
    assert!(
        s["policy"]["params"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "aq.target_ms")
    );
    assert!(
        s["policy"]["weights"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "fx.bloom.value_weight")
    );
    let only = t.ok("schema.get", json!({ "include": ["curves"] }));
    assert!(only.get("params").is_none() && only["curves"].is_array());
}

#[test]
fn snapshot_save_restore_round_trip() {
    let mut t = T::new();
    t.ok(
        "param.set",
        json!({ "values": { "core.audio.master": 0.4 } }),
    );
    t.ok(
        "curve.set",
        json!({ "curves": { CURVE: [[0.5, 2.0], [1.5, 2.5]] } }),
    );
    let saved_state = t.state();
    let saved = t.ok("snapshot.save", json!({ "name": "warm" }));
    assert_eq!(saved["snapshot"], "snap-1");
    assert_eq!(saved["tx"], 2);
    assert_eq!(saved["summary"]["params_changed"], 1);
    assert_eq!(saved["summary"]["curves_changed"], json!([CURVE]));

    t.ok(
        "param.set",
        json!({ "values": { "core.audio.master": 0.9 } }),
    );
    t.ok("slot.set", json!({ "name": EXPR, "source": "held" }));
    t.ok("tier.set", json!({ "tier": "low" }));
    t.ok(
        "effect.toggle",
        json!({ "effect": "vignette", "on": false, "scope": "tier" }),
    );
    let changed = t.state();
    assert_ne!(changed, saved_state);

    let list = t.ok("snapshot.list", json!({}));
    assert_eq!(list["snapshots"][0]["name"], "warm");
    let r = t.ok("snapshot.restore", json!({ "snapshot": "warm" }));
    assert!(r["applied"].as_array().unwrap().len() >= 4);
    assert_eq!(t.state(), saved_state);
    assert_eq!(t.history_len(), 7);
    t.ok("history.undo", json!({}));
    assert_eq!(t.state(), changed);
    t.ok("history.redo", json!({}));
    assert_eq!(t.state(), saved_state);
    assert_eq!(
        t.err("snapshot.restore", json!({ "snapshot": "nope" }))["code"],
        "unknown_target"
    );
}

#[test]
fn vr_tier_needs_an_xr_session() {
    let mut t = T::new();
    let e = t.err("tier.set", json!({ "tier": "vr" }));
    assert_eq!(e["code"], "needs_xr_session", "{e}");
    let ids: Vec<&str> = e["data"]["selectable"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(!ids.contains(&"vr"), "{ids:?}");
    assert!(ids.contains(&"low") && ids.contains(&"ultra"), "{ids:?}");
    assert_eq!(t.history_len(), 0);
    // Inside an immersive session the preset is offered.
    t.a
        .world
        .res_mut::<evani_core::tiers::Tiers>()
        .set_xr_session(true);
    t.ok("tier.set", json!({ "tier": "vr" }));
}
