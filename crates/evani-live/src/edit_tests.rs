//! Edit transactions (docs/LIVE_API.md 6.31): a compound request ("turn the
//! lights down, turn the map blue, reverse the rotation") split into three
//! atomic edits, staged by three clients (parallel agents) in the same frame,
//! in both apply modes; conflicts, failures, resolve and bake.
//!
//! The three tunables come from a test-only module, not from a game.

use evani_core::App;
use evani_core::params::{ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use serde_json::{Value, json};

use crate::session::TxStatus;
use crate::{LiveModule, LiveSession, submit, take_events, take_replies};

const LIGHT: &str = "test.light.intensity";
const SKY: &str = "test.sky.color";
const SPIN: &str = "test.spin.speed";

/// The light, the sky/map colour and the spin, as a game would register them.
struct TestScene;

impl EngineModule for TestScene {
    fn id(&self) -> &'static str {
        "evani.module.test_scene"
    }
    fn label(&self) -> &'static str {
        "test_scene"
    }
    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        ctx.param_f32(ParamDef::f32(LIGHT, 1.0).range(0.0, 4.0))?;
        ctx.param_vec3(ParamDef::color(SKY, [0.9, 0.6, 0.3]))?;
        ctx.param_f32(ParamDef::f32(SPIN, 6.0).range(-20.0, 20.0))?;
        Ok(())
    }
}

struct T {
    a: App,
    token: u64,
}

impl T {
    fn new() -> Self {
        let mut a = App::new("sandbox")
            .with_kit(evani_kit_example::ExampleKitPlugin)
            .with_look(evani_look_basic::BasicLook)
            .with_module(evani_fx::FxModule::compiled())
            .with_module(TestScene)
            .with_module(LiveModule::new());
        a.run_headless(2, 16.0);
        Self { a, token: 0 }
    }

    fn req(&mut self, cmd: &str, params: Value, note: &str) -> u64 {
        self.token += 1;
        let req = json!({ "type": "cmd", "v": 1, "id": format!("t-{}", self.token), "cmd": cmd,
                          "params": params, "note": note, "source": "mcp",
                          "opts": { "telemetry_ms": 0 } });
        submit(&self.a.world, self.token, &req.to_string());
        self.token
    }

    /// Run frames until every token answered; the results by token order.
    fn wait(&mut self, tokens: &[u64]) -> Vec<Value> {
        let mut got: Vec<Option<Value>> = vec![None; tokens.len()];
        for _ in 0..60 {
            self.a.run_headless(1, 16.0);
            for (t, text) in take_replies(&self.a.world) {
                if let Some(i) = tokens.iter().position(|x| *x == t) {
                    got[i] = Some(serde_json::from_str(&text).unwrap());
                }
            }
            if got.iter().all(Option::is_some) {
                self.a.run_headless(2, 16.0);
                return got.into_iter().map(Option::unwrap).collect();
            }
        }
        panic!("no reply to {tokens:?}: got {got:?}");
    }

    fn ok(&mut self, cmd: &str, params: Value) -> Value {
        let t = self.req(cmd, params.clone(), &format!("test {cmd}"));
        let r = self.wait(&[t]).remove(0);
        assert_eq!(r["ok"], true, "{cmd} {params} -> {r}");
        r["result"].clone()
    }

    fn err(&mut self, cmd: &str, params: Value) -> Value {
        let t = self.req(cmd, params.clone(), &format!("test {cmd}"));
        let r = self.wait(&[t]).remove(0);
        assert_eq!(r["ok"], false, "{cmd} {params} should fail -> {r}");
        r["error"].clone()
    }

    /// Three agents each stage one edit in the same frame.
    fn stage_three(&mut self, txn: &str) -> Vec<Value> {
        let tokens = [
            self.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "e1", "cmd": "param.set",
                        "params": { "values": { LIGHT: 0.4 } } }),
                "turn the lights down",
            ),
            self.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "e2", "cmd": "param.set",
                        "params": { "values": { SKY: [0.1, 0.25, 0.9] } } }),
                "turn the map blue",
            ),
            self.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "e3", "cmd": "param.set",
                        "params": { "values": { SPIN: -6.0 } } }),
                "reverse the rotation",
            ),
        ];
        self.wait(&tokens)
            .into_iter()
            .map(|r| {
                assert_eq!(r["ok"], true, "{r}");
                r["result"].clone()
            })
            .collect()
    }

    fn value(&self, name: &str) -> ParamValue {
        let reg = self.a.world.res::<ParamRegistry>();
        reg.id(name).and_then(|i| reg.value(i)).unwrap()
    }

    fn f(&self, name: &str) -> f32 {
        match self.value(name) {
            ParamValue::F32(x) => x,
            v => panic!("{name} is {v:?}"),
        }
    }

    fn sky(&self) -> [f32; 3] {
        match self.value(SKY) {
            ParamValue::Color(c) | ParamValue::Vec3(c) => c,
            v => panic!("{SKY} is {v:?}"),
        }
    }

    fn originals(&self) -> bool {
        self.f(LIGHT) == 1.0 && self.sky() == [0.9, 0.6, 0.3] && self.f(SPIN) == 6.0
    }

    fn all_three(&self) -> bool {
        (self.f(LIGHT) - 0.4).abs() < 1e-6
            && self.sky() == [0.1, 0.25, 0.9]
            && (self.f(SPIN) + 6.0).abs() < 1e-6
    }

    fn begin(&mut self, mode: &str) -> String {
        let r = self.ok(
            "edit.begin",
            json!({ "mode": mode,
                    "request": "turn the lights down, turn the blue map, and reverse the rotation",
                    "edits": ["turn the lights down", "turn the map blue", "reverse the rotation"] }),
        );
        r["txn"].as_str().unwrap().to_string()
    }

    fn status(&mut self, txn: &str) -> Value {
        self.ok("edit.status", json!({ "txn": txn }))
    }

    fn edit_state(&mut self, txn: &str, edit: &str) -> String {
        let s = self.status(txn);
        s["edits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["edit"] == edit)
            .map(|e| e["state"].as_str().unwrap().to_string())
            .unwrap()
    }
}

/// Decomposition input: `edit.begin` takes the list of atomic edits and
/// hands back the plan, one pending edit per item, with ids.
#[test]
fn begin_returns_the_plan_of_atomic_edits() {
    let mut t = T::new();
    let r = t.ok(
        "edit.begin",
        json!({ "request": "turn the lights down, turn the blue map, and reverse the rotation",
                "edits": ["turn the lights down", { "id": "sky", "what": "turn the map blue" }, "reverse the rotation"] }),
    );
    assert_eq!(r["mode"], "stream", "stream is the default");
    assert_eq!(r["state"], "open");
    let ids: Vec<&str> = r["edits"].as_array().unwrap().iter().map(|e| e["edit"].as_str().unwrap()).collect();
    assert_eq!(ids, ["e1", "sky", "e2"]);
    assert!(r["edits"].as_array().unwrap().iter().all(|e| e["state"] == "pending"));
    assert_eq!(r["edits"][1]["what"], "turn the map blue");
    assert_eq!(r["counts"]["pending"], 3);
    assert!(r["how"].as_str().unwrap().contains("one agent per edit"));
    // Bad plans are refused whole.
    t.err("edit.begin", json!({ "edits": [{ "id": "a" }, { "id": "a" }] }));
    t.err("edit.begin", json!({ "mode": "later" }));
    // The default mode follows live.edit.mode.
    t.ok("param.set", json!({ "values": { "live.edit.mode": "together" } }));
    let r = t.ok("edit.begin", json!({}));
    assert_eq!(r["mode"], "together");
    let s = t.ok("edit.status", json!({}));
    assert_eq!(s["default_mode"], "together");
}

/// Stream: three agents stage in the same frame; each goes live at once as
/// its own history entry with its own note.
#[test]
fn three_agents_stream_all_apply() {
    let mut t = T::new();
    assert!(t.originals());
    let txn = t.begin("stream");
    let res = t.stage_three(&txn);
    for (r, e) in res.iter().zip(["e1", "e2", "e3"]) {
        assert_eq!(r["state"], "applied", "{r}");
        assert_eq!(r["edit"], e);
        assert_eq!(r["txn"], txn.as_str());
    }
    assert!(t.all_three());
    let c = t.ok("edit.commit", json!({ "txn": txn }));
    assert_eq!(c["state"], "closed");
    assert_eq!(c["counts"]["applied"], 3);
    let h = t.ok("history.list", json!({}));
    let mine: Vec<&Value> = h["entries"]
        .as_array()
        .or_else(|| h["history"].as_array())
        .unwrap()
        .iter()
        .filter(|e| e["txn"] == txn.as_str())
        .collect();
    assert_eq!(mine.len(), 3, "{h}");
    let notes: Vec<&str> = mine.iter().map(|e| e["note"].as_str().unwrap()).collect();
    for n in ["turn the lights down", "turn the map blue", "reverse the rotation"] {
        assert!(notes.contains(&n), "{notes:?}");
    }
    // Each edit undoes on its own: undo the last one only.
    t.ok("history.undo", json!({}));
    assert!((t.f(LIGHT) - 0.4).abs() < 1e-6);
    assert_eq!(t.sky(), [0.1, 0.25, 0.9]);
    assert_eq!(t.f(SPIN), 6.0);
}

/// Together: nothing shows until commit, then all three land on one frame
/// (the same history frame), each still its own history entry.
#[test]
fn three_agents_together_land_on_one_frame() {
    let mut t = T::new();
    let txn = t.begin("together");
    let res = t.stage_three(&txn);
    assert!(res.iter().all(|r| r["state"] == "staged"), "{res:?}");
    for _ in 0..5 {
        t.a.run_headless(1, 16.0);
        assert!(t.originals(), "a staged edit showed before commit");
    }
    let tok = t.req("edit.commit", json!({ "txn": txn }), "all three at once");
    // The frame the commit is processed on: before it nothing, after it all three.
    let mut replies = Vec::new();
    for _ in 0..10 {
        let before = t.originals();
        t.a.run_headless(1, 16.0);
        replies.extend(take_replies(&t.a.world));
        if replies.iter().any(|(x, _)| *x == tok) {
            assert!(before);
            assert!(t.all_three(), "not every edit landed in the commit frame");
            break;
        }
        assert!(t.originals(), "part of the commit showed early");
    }
    let (_, text) = replies.into_iter().find(|(x, _)| *x == tok).unwrap();
    let c: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(c["ok"], true, "{c}");
    let c = &c["result"];
    assert_eq!(c["state"], "closed");
    assert_eq!(c["counts"]["applied"], 3);
    assert_eq!(c["txs"].as_array().unwrap().len(), 3);
    assert_eq!(c["applied"].as_array().unwrap().len(), 3);
    let s = t.a.world.res::<LiveSession>();
    let frames: Vec<u64> = s
        .history
        .iter()
        .filter(|x| x.edit.as_ref().is_some_and(|(id, _)| *id == txn))
        .map(|x| x.frame)
        .collect();
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|f| *f == frames[0]), "{frames:?}");
    // A plain-parameter commit lands in one tick: no canvas hold needed.
    assert!(s.edit_hold.is_none());
}

/// Two edits to the same parameter conflict, found the same way whatever
/// the arrival order and in both modes; neither stays applied until resolved.
#[test]
fn same_parameter_clash_is_a_conflict_in_both_orders_and_modes() {
    for mode in ["stream", "together"] {
        for order in [["down", "up"], ["up", "down"]] {
            let mut t = T::new();
            let txn = t.begin(mode);
            let value = |w: &str| if w == "down" { 0.3 } else { 1.8 };
            let tokens: Vec<u64> = order
                .iter()
                .map(|w| {
                    t.req(
                        "edit.stage",
                        json!({ "txn": txn, "edit": w, "cmd": "param.set",
                                "params": { "values": { LIGHT: value(w) } } }),
                        &format!("lights {w}"),
                    )
                })
                .collect();
            let other = t.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "sky", "cmd": "param.set",
                        "params": { "values": { SKY: [0.1, 0.25, 0.9] } } }),
                "turn the map blue",
            );
            let r = t.wait(&[tokens[0], tokens[1], other]);
            let second = &r[1]["result"];
            assert_eq!(second["state"], "conflict", "{mode} {order:?}: {second}");
            assert_eq!(second["conflict"]["with"], json!([order[0]]));
            assert_eq!(second["conflict"]["targets"], json!([LIGHT]));
            assert!(second["conflict"]["ask"].as_str().unwrap().contains("Ask the user"));
            let ev: Vec<String> = take_events(&t.a.world);
            assert!(ev.iter().any(|e| e.contains("edit.conflict")), "{mode}: {ev:?}");
            for w in order {
                assert_eq!(t.edit_state(&txn, w), "conflict", "{mode} {order:?} {w}");
            }
            assert_eq!(t.f(LIGHT), 1.0, "{mode} {order:?}: a clashing edit stayed applied");
            // The independent edit is not held up.
            let c = t.ok("edit.commit", json!({ "txn": txn }));
            assert_eq!(c["state"], "waiting", "{c}");
            assert_eq!(t.sky(), [0.1, 0.25, 0.9]);
            assert_eq!(t.f(LIGHT), 1.0);
            if mode == "stream" {
                // The first edit was live and is now held back as its own entry.
                let s = t.a.world.res::<LiveSession>();
                let held = s.history.iter().find(|x| x.edit.as_ref().is_some_and(|(_, e)| e == order[0]) && x.cmd == "param.set").unwrap();
                assert_eq!(held.status, TxStatus::AutoReverted);
                assert_eq!(held.revert_reason.as_deref(), Some("conflict"));
                assert!(s.history.iter().any(|x| x.cmd == "edit.hold_back"));
            }
            // The user picks "up".
            let r = t.ok("edit.resolve", json!({ "txn": txn, "keep": "up" }));
            assert!((t.f(LIGHT) - 1.8).abs() < 1e-6, "{mode} {order:?}: {r}");
            assert_eq!(t.edit_state(&txn, "up"), "applied");
            assert_eq!(t.edit_state(&txn, "down"), "dropped");
            assert_eq!(t.status(&txn)["state"], "closed");
        }
    }
}

/// Resolve with drop: none of the clashing edits applies.
#[test]
fn resolve_drop_applies_neither() {
    for mode in ["stream", "together"] {
        let mut t = T::new();
        let txn = t.begin(mode);
        let a = t.req(
            "edit.stage",
            json!({ "txn": txn, "edit": "a", "cmd": "param.set", "params": { "values": { SPIN: 10.0 } } }),
            "spin faster",
        );
        let b = t.req(
            "edit.stage",
            json!({ "txn": txn, "edit": "b", "cmd": "batch",
                    "params": { "cmds": [{ "cmd": "param.set", "params": { "values": { SPIN: -3.0, LIGHT: 2.0 } } }] } }),
            "reverse and brighten",
        );
        let r = t.wait(&[a, b]);
        assert_eq!(r[1]["result"]["state"], "conflict", "{mode}");
        t.ok("edit.commit", json!({ "txn": txn }));
        let r = t.ok("edit.resolve", json!({ "txn": txn, "drop": "a" }));
        assert_eq!(r["state"], "closed", "{r}");
        assert_eq!(t.f(SPIN), 6.0, "{mode}");
        assert_eq!(t.f(LIGHT), 1.0, "{mode}");
        assert_eq!(t.edit_state(&txn, "a"), "dropped");
        assert_eq!(t.edit_state(&txn, "b"), "dropped");
        // Nothing left to resolve.
        t.err("edit.resolve", json!({ "txn": txn, "keep": "b" }));
    }
}

/// A bad name and an out-of-range value fail on their own edit, with the
/// reason; the other edits still apply.
#[test]
fn a_failed_edit_never_blocks_the_others() {
    for mode in ["stream", "together"] {
        let mut t = T::new();
        let txn = t.begin(mode);
        let tokens = [
            t.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "e1", "cmd": "param.set", "params": { "values": { "test.light.intensty": 0.4 } } }),
                "turn the lights down",
            ),
            t.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "e2", "cmd": "param.set", "params": { "values": { SKY: [0.1, 0.25, 0.9] } } }),
                "turn the map blue",
            ),
            t.req(
                "edit.stage",
                json!({ "txn": txn, "edit": "e3", "cmd": "param.set", "params": { "values": { SPIN: -60.0 } } }),
                "reverse the rotation",
            ),
        ];
        let r = t.wait(&tokens);
        assert!(r.iter().all(|x| x["ok"] == true), "{r:?}");
        let bad_name = &r[0]["result"];
        assert_eq!(bad_name["state"], "failed", "{mode}: {bad_name}");
        assert!(bad_name["error"]["code"].is_string());
        let range = &r[2]["result"];
        assert_eq!(range["state"], "failed", "{mode}: {range}");
        assert_eq!(range["error"]["code"], "out_of_range");
        assert_eq!(range["error"]["data"]["name"], SPIN);
        assert_eq!(range["error"]["data"]["nearest"], -20.0);
        assert_eq!(range["error"]["data"]["min"], -20.0);
        assert_eq!(range["error"]["data"]["max"], 20.0);
        let c = t.ok("edit.commit", json!({ "txn": txn }));
        assert_eq!(c["counts"]["applied"], 1, "{mode}: {c}");
        assert_eq!(c["counts"]["failed"], 2);
        assert_eq!(t.sky(), [0.1, 0.25, 0.9]);
        assert_eq!(t.f(LIGHT), 1.0);
        assert_eq!(t.f(SPIN), 6.0);
        // Staged again with clamp (in a new transaction: the commit closed
        // that one), the out-of-range edit goes through at the nearest value.
        let tx2 = t.begin(mode);
        let r = t.ok(
            "edit.stage",
            json!({ "txn": tx2, "edit": "e3", "cmd": "param.set", "params": { "values": { SPIN: -60.0 } }, "clamp": true }),
        );
        if mode == "together" {
            assert_eq!(r["state"], "staged");
            t.ok("edit.commit", json!({ "txn": tx2 }));
        } else {
            assert_eq!(r["state"], "applied");
        }
        assert_eq!(t.f(SPIN), -20.0, "{mode}");
        // param.set itself still clamps as before.
        let r = t.ok("param.set", json!({ "values": { SPIN: 99.0 } }));
        assert_eq!(r["applied"][0]["clamped"], true);
    }
}

/// The edits are ordinary history: bake.export plans them as it does any
/// change, with their notes; a held-back edit is not baked.
#[test]
fn bake_works_on_the_result() {
    let mut t = T::new();
    let txn = t.begin("stream");
    t.stage_three(&txn);
    let a = t.req(
        "edit.stage",
        json!({ "txn": txn, "edit": "e4", "cmd": "param.set", "params": { "values": { LIGHT: 2.5 } } }),
        "lights up",
    );
    let r = t.wait(&[a]).remove(0);
    assert_eq!(r["result"]["state"], "conflict");
    t.ok("edit.resolve", json!({ "txn": txn, "keep": "e1" }));
    t.ok("edit.commit", json!({ "txn": txn }));
    assert!(t.all_three());
    let b = t.ok("bake.export", json!({ "dry_run": true }));
    let params = b["bundle"]["params"].as_array().unwrap_or_else(|| panic!("no params in {b}"));
    let find = |n: &str| params.iter().find(|p| p["name"] == n).unwrap_or_else(|| panic!("{n} not baked: {b}"));
    assert!((find(LIGHT)["value"].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert!((find(SPIN)["value"].as_f64().unwrap() + 6.0).abs() < 1e-6);
    find(SKY);
    let notes = b.to_string();
    assert!(notes.contains("reverse the rotation"), "{b}");
}

/// Cancel drops staged edits; the transaction takes no more.
#[test]
fn cancel_drops_staged_edits() {
    let mut t = T::new();
    let txn = t.begin("together");
    t.stage_three(&txn);
    let r = t.ok("edit.cancel", json!({ "txn": txn }));
    assert_eq!(r["state"], "cancelled");
    assert_eq!(r["counts"]["cancelled"], 3);
    t.err("edit.commit", json!({ "txn": txn }));
    assert!(t.originals());
    // A stage outside an open transaction, or without a note, is refused.
    t.err(
        "edit.stage",
        json!({ "txn": txn, "cmd": "param.set", "params": { "values": { LIGHT: 0.2 } } }),
    );
    let other = t.begin("stream");
    let tok = t.req(
        "edit.stage",
        json!({ "txn": other, "cmd": "param.set", "params": { "values": { LIGHT: 0.2 } } }),
        "",
    );
    assert_eq!(t.wait(&[tok])[0]["error"]["code"], "note_required");
    t.err(
        "edit.stage",
        json!({ "txn": other, "cmd": "edit.stage", "params": {} }),
    );
}

/// Undoing the hold-back of a conflict that still waits for the user is refused (it would put
/// the edit back on screen while the transaction still reports `conflict`); once the conflict
/// is resolved, undo works as usual.
#[test]
fn undo_of_an_open_hold_back_is_refused_until_resolved() {
    let mut t = T::new();
    let txn = t.begin("stream");
    let a = t.req(
        "edit.stage",
        json!({ "txn": txn, "edit": "down", "cmd": "param.set", "params": { "values": { LIGHT: 0.3 } } }),
        "lights down",
    );
    let b = t.req(
        "edit.stage",
        json!({ "txn": txn, "edit": "up", "cmd": "param.set", "params": { "values": { LIGHT: 1.8 } } }),
        "lights up",
    );
    let r = t.wait(&[a, b]);
    assert_eq!(r[1]["result"]["state"], "conflict", "{}", r[1]);
    assert_eq!(t.f(LIGHT), 1.0);
    let e = t.err("history.undo", json!({}));
    assert_eq!(e["code"], "invalid_value", "{e}");
    assert!(e["message"].as_str().unwrap().contains("edit_resolve"), "{e}");
    assert_eq!(t.f(LIGHT), 1.0, "a refused undo changed the light");
    assert_eq!(t.edit_state(&txn, "down"), "conflict");
    t.ok("edit.resolve", json!({ "txn": txn, "keep": "down" }));
    assert!((t.f(LIGHT) - 0.3).abs() < 1e-6);
    t.ok("history.undo", json!({}));
    assert_eq!(t.f(LIGHT), 1.0, "undo after resolve works");
}

/// At most MAX_TXNS transactions are kept: finished ones make room, but an open one is never
/// dropped under its agents; when every kept one is open, edit.begin answers `limit`.
#[test]
fn open_transactions_are_never_evicted() {
    let mut t = T::new();
    let first = t.begin("stream");
    for _ in 1..crate::edits::MAX_TXNS {
        t.begin("stream");
    }
    let e = t.err("edit.begin", json!({ "edits": ["one more"] }));
    assert_eq!(e["code"], "limit", "{e}");
    assert_eq!(t.status(&first)["state"], "open", "the oldest open transaction was dropped");
    t.ok("edit.cancel", json!({ "txn": first }));
    let r = t.ok("edit.begin", json!({ "edits": ["one more"] }));
    assert!(r["txn"].is_string(), "{r}");
    t.err("edit.status", json!({ "txn": first }));
}
