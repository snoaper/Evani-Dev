//! Deferred change results, the auto-revert watch (docs/LIVE_API.md 5.4,
//! 6.24) and the session mirror event (10.3).

use evani_core::capture::{CaptureQueue, CaptureView, DeviceStatus};
use evani_core::ecs::World;
use evani_core::effects::{BudgetStatus, EffectState, EffectsStatus};
use evani_core::params::ParamRegistry;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use serde_json::{Value, json};

use crate::change::{Change, apply_all};
use crate::commands;
use crate::session::{AmendMode, LiveSession, TierAmend, Tx, TxStatus, now_iso};
use crate::telemetry::{change_telemetry, round2};

/// Frame-time average window for the watch (6.24).
const AVG_MS: f64 = 500.0;
/// Baseline window before a change (5.4).
const BASELINE_MS: f64 = 1000.0;

/// The auto-revert guard on one transaction.
#[derive(Debug, Clone, Copy)]
pub struct Watch {
    pub tx: u64,
    pub armed_ms: f64,
    pub frame: u64,
    pub target_ms: f32,
    pub limit_ms: f32,
    pub sustain_s: f32,
    pub window_s: f32,
    /// When the 0.5 s average first went over (and has stayed over).
    pub over_since: Option<f64>,
    pub lost_count: u32,
}

/// A mutating command's result waiting for its telemetry window.
#[derive(Debug, Clone)]
pub struct Deferred {
    pub token: u64,
    pub id: Value,
    pub result: Value,
    pub commit_ms: f64,
    pub frame: u64,
    pub telemetry_ms: f64,
    pub baseline: Option<f32>,
    /// Effects on (by name) when the change was made.
    pub fx_before: Vec<(&'static str, EffectState)>,
    /// `opts.capture`: a screenshot taken once the telemetry window is over.
    pub capture: Option<DeferredCapture>,
    /// `telemetry` and `budget` are filled (the window is over).
    pub filled: bool,
}

/// The screenshot a change result waits for (`opts.capture`).
#[derive(Debug, Clone, Copy)]
pub struct DeferredCapture {
    pub width: u32,
    /// The capture request, once sent.
    pub id: Option<u64>,
    pub since_ms: f64,
}

/// `opts.capture`: `{ "width": 64-1024 }` or `true` (480 wide).
fn capture_opt(req: &Value) -> Option<DeferredCapture> {
    let c = req.get("opts")?.get("capture")?;
    let width = match c {
        Value::Bool(true) => 480,
        Value::Object(o) => o.get("width").and_then(Value::as_u64).unwrap_or(480),
        _ => return None,
    };
    Some(DeferredCapture {
        width: width.clamp(64, 1024) as u32,
        id: None,
        since_ms: 0.0,
    })
}

fn param(world: &World, name: &str, default: f32) -> f32 {
    let reg = world.res::<ParamRegistry>();
    reg.f32_handle(name).map_or(default, |h| reg.f32(h))
}

fn now(world: &World) -> (f64, u64) {
    world
        .try_res::<Time>()
        .map_or((0.0, 0), |t| (t.now_ms, t.frame))
}

/// Target frame ms: `aq.target_ms` when set, else the active tier's.
pub fn target_ms(world: &World) -> f32 {
    let aq = param(world, "aq.target_ms", 0.0);
    if aq > 0.0 {
        aq
    } else {
        world
            .try_res::<Tiers>()
            .map_or(1000.0 / 60.0, |t| t.active().frame_budget_ms())
    }
}

fn effect_states(world: &World) -> Vec<(&'static str, EffectState)> {
    world
        .try_res::<EffectsStatus>()
        .map_or_else(Vec::new, |fx| {
            fx.effects.iter().map(|e| (e.info.name, e.state)).collect()
        })
}

/// Called by [`commands::finish_mutation`]: arm (or clear) the watch, push the
/// mirror, and send the result now (`telemetry_ms` 0) or park it.
pub fn finish(world: &World, token: u64, id: &Value, req: &Value, mut result: Value, arm: bool) {
    let (now_ms, frame) = now(world);
    let tx = result.get("tx").and_then(Value::as_u64).unwrap_or(0);
    let tx_frame = result
        .get("frame")
        .and_then(Value::as_u64)
        .unwrap_or(frame + 1);
    let (baseline, baseline_work) = {
        let s = world.res::<LiveSession>();
        (
            s.tel.mean_frame_ms(now_ms - BASELINE_MS, now_ms, 0),
            s.tel.mean_work_ms(now_ms - BASELINE_MS, now_ms, 0),
        )
    };
    // The guard judges work time (CPU + GPU) of frames not held on purpose:
    // the frame interval is vsync-bound and says nothing about headroom.
    result["watch"] = arm_watch(world, tx, arm, now_ms, tx_frame, baseline_work);
    push_mirror(world);
    let telemetry_ms = req
        .get("opts")
        .and_then(|o| o.get("telemetry_ms"))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| f64::from(param(world, "live.telemetry_ms", 1000.0)))
        .clamp(0.0, 10_000.0);
    let capture = capture_opt(req);
    if telemetry_ms <= 0.0 {
        result["telemetry"] = Value::Null;
        if capture.is_none() {
            commands::send_result(world, token, id, result);
            return;
        }
    }
    let d = Deferred {
        token,
        id: id.clone(),
        result,
        commit_ms: now_ms,
        frame: tx_frame,
        telemetry_ms,
        baseline,
        fx_before: effect_states(world),
        capture,
        filled: telemetry_ms <= 0.0,
    };
    world.res_mut::<LiveSession>().deferred.push(d);
}

fn arm_watch(
    world: &World,
    tx: u64,
    arm: bool,
    now_ms: f64,
    frame: u64,
    baseline: Option<f32>,
) -> Value {
    if !arm {
        world.res_mut::<LiveSession>().watch = None;
        return json!({ "auto_revert": "off", "reason": "history" });
    }
    let target = target_ms(world);
    let over = param(world, "live.revert.over_pct", 25.0);
    let limit = target * (1.0 + over / 100.0);
    let sustain = param(world, "live.revert.sustain_s", 3.0);
    let window = param(world, "live.revert.window_s", 10.0);
    let info = |state: &str| {
        json!({ "auto_revert": state, "target_ms": round2(target), "limit_ms": round2(limit),
                "sustain_s": sustain, "window_s": window })
    };
    if baseline.is_some_and(|b| b > limit) {
        world.res_mut::<LiveSession>().watch = None;
        let mut v = info("off");
        v["reason"] = json!("baseline_over");
        return v;
    }
    let lost_count = world.try_res::<DeviceStatus>().map_or(0, |d| d.lost_count);
    world.res_mut::<LiveSession>().watch = Some(Watch {
        tx,
        armed_ms: now_ms,
        frame,
        target_ms: target,
        limit_ms: limit,
        sustain_s: sustain,
        window_s: window,
        over_since: None,
        lost_count,
    });
    info("armed")
}

/// Per frame (after sampling): send due deferred results, run the watch.
pub fn frame(world: &World) {
    send_due(world);
    // A bench or compare run changes state on purpose; don't blame the
    // watched change for its frame times.
    if world.res::<LiveSession>().run.is_none() {
        run_watch(world);
    }
}

fn send_due(world: &World) {
    let (now_ms, frame) = now(world);
    let ready: Vec<Deferred> = {
        let mut s = world.res_mut::<LiveSession>();
        if s.deferred.is_empty() {
            return;
        }
        let (go, keep): (Vec<_>, Vec<_>) = core::mem::take(&mut s.deferred)
            .into_iter()
            .partition(|d| now_ms >= d.commit_ms + d.telemetry_ms && frame > d.frame);
        s.deferred = keep;
        go
    };
    let mut waiting = Vec::new();
    for mut d in ready {
        if !d.filled {
            d.filled = true;
            d.result["telemetry"] =
                change_telemetry(world, d.commit_ms, now_ms, d.frame, d.baseline);
            fill_budget(world, &mut d);
        }
        if let Some(c) = d.capture.as_mut()
            && !poll_capture(world, &mut d.result, c, now_ms)
        {
            waiting.push(d);
            continue;
        }
        commands::send_result(world, d.token, &d.id, d.result);
    }
    world.res_mut::<LiveSession>().deferred.extend(waiting);
}

/// Request the change result's screenshot, or fill `capture` once it is
/// there (or failed). Returns true when the result can go.
fn poll_capture(world: &World, result: &mut Value, c: &mut DeferredCapture, now_ms: f64) -> bool {
    let Some(id) = c.id else {
        let Some(mut q) = world.try_res_mut::<CaptureQueue>() else {
            result["capture"] = json!({ "error": "no capture queue in this build" });
            return true;
        };
        c.id = Some(q.request(c.width, CaptureView::Current, true));
        c.since_ms = now_ms;
        return false;
    };
    let got = world.res_mut::<CaptureQueue>().take_result(id);
    match got {
        Some(r) => {
            result["capture"] = crate::capture::capture_json(world, &r, c.width);
            true
        }
        None if now_ms - c.since_ms > crate::capture::CAPTURE_TIMEOUT_MS => {
            result["capture"] =
                json!({ "error": "the renderer did not deliver the capture within 5 s" });
            true
        }
        None => false,
    }
}

/// `budget` after the frame: points, effects the budget switched, and
/// `reason: "over_budget"` on effects that were asked on but stayed off.
pub(crate) fn fill_budget(world: &World, d: &mut Deferred) {
    if let Some(b) = world.try_res::<BudgetStatus>() {
        d.result["budget"]["points_used"] = json!(b.used);
        d.result["budget"]["points_limit"] = json!(b.total);
    }
    let after = effect_states(world);
    if after.is_empty() {
        return;
    }
    let was = |n: &str| d.fx_before.iter().find(|(m, _)| *m == n).map(|(_, s)| *s);
    let mut off = Vec::new();
    let mut on = Vec::new();
    for (name, st) in &after {
        let Some(before) = was(name) else { continue };
        if before.is_on() && *st == EffectState::OffByBudget {
            off.push(*name);
        } else if before == EffectState::OffByBudget && st.is_on() {
            on.push(*name);
        }
    }
    d.result["budget"]["switched_off"] = json!(off);
    d.result["budget"]["switched_on"] = json!(on);
    if let Some(list) = d.result.get_mut("applied").and_then(Value::as_array_mut) {
        for a in list {
            let name = a.get("name").and_then(Value::as_str);
            // effect.toggle reports the bare effect name; param.set the param.
            let fx = if a.get("kind").and_then(Value::as_str) == Some("effect") {
                name
            } else {
                name.and_then(|n| n.strip_prefix("fx."))
                    .and_then(|n| n.strip_suffix(".enabled"))
            };
            let Some(fx) = fx else { continue };
            let off_by_budget = after
                .iter()
                .any(|(n, s)| *n == fx && *s == EffectState::OffByBudget);
            if a.get("new") == Some(&Value::Bool(true)) && off_by_budget {
                a["reason"] = json!("over_budget");
            }
        }
    }
}

fn run_watch(world: &World) {
    let Some(mut w) = world.res::<LiveSession>().watch else {
        return;
    };
    let (now_ms, _) = now(world);
    let lost = world.try_res::<DeviceStatus>().map_or(0, |d| d.lost_count);
    // Mean work time (CPU + GPU) of the frames not held by a capture,
    // bench or compare run; vsync-bound frame intervals are not judged.
    let avg = world
        .res::<LiveSession>()
        .tel
        .mean_work_ms(now_ms - AVG_MS, now_ms, 0);
    if lost > w.lost_count {
        world.res_mut::<LiveSession>().watch = None;
        auto_revert(world, &w, "device_lost", avg.unwrap_or(0.0));
        return;
    }
    if avg.is_none() {
        // Every recent frame was held: nothing to judge; keep the state.
        world.res_mut::<LiveSession>().watch = Some(w);
        return;
    }
    let over = avg.is_some_and(|a| a > w.limit_ms);
    if over {
        let since = *w.over_since.get_or_insert(now_ms);
        if now_ms - since >= f64::from(w.sustain_s) * 1000.0 {
            world.res_mut::<LiveSession>().watch = None;
            auto_revert(world, &w, "frame_time", avg.unwrap_or(0.0));
            return;
        }
    } else {
        w.over_since = None;
        if now_ms - w.armed_ms >= f64::from(w.window_s) * 1000.0 {
            world.res_mut::<LiveSession>().watch = None;
            return;
        }
    }
    world.res_mut::<LiveSession>().watch = Some(w);
}

/// The changes that undo `c` when applied forward.
pub(crate) fn inverse(c: &Change) -> Change {
    match c.clone() {
        Change::Param { id, old, new } => Change::Param {
            id,
            old: new,
            new: old,
        },
        Change::Slot { id, old, new } => Change::Slot {
            id,
            old: new,
            new: old,
        },
        Change::Formula { id, old, new } => Change::Formula {
            id,
            old: new,
            new: old,
        },
        Change::Tier { old, new } => Change::Tier { old: new, new: old },
        Change::TierEffects { tier, old, new } => Change::TierEffects {
            tier,
            old: new,
            new: old,
        },
    }
}

/// Undo the watched transaction as a new `auto_revert` history entry and
/// push the `auto_revert` event.
fn auto_revert(world: &World, w: &Watch, reason: &str, observed: f32) {
    let (orig_changes, note) = {
        let s = world.res::<LiveSession>();
        let Some(t) = s.history[..s.head]
            .iter()
            .find(|t| t.tx == w.tx && t.status == TxStatus::Applied)
        else {
            return;
        };
        (t.changes.clone(), t.note.clone())
    };
    let undo: Vec<Change> = orig_changes.iter().rev().map(inverse).collect();
    if apply_all(world, &undo).is_err() {
        return;
    }
    let undone: Vec<Value> = orig_changes
        .iter()
        .map(|c| {
            let d = c.describe(world, true);
            let restored = d.get("old").or_else(|| d.get("old_hash")).cloned();
            json!({ "kind": d["kind"], "name": d["name"], "restored": restored })
        })
        .collect();
    if undo.iter().any(|c| matches!(c, Change::Tier { .. })) {
        let vals = commands::effect_switches(world);
        world.res_mut::<LiveSession>().tier_amend = Some(TierAmend {
            tx: None,
            pre: Vec::new(),
            mode: AmendMode::Reassert(vals),
        });
    }
    let (_, frame) = now(world);
    let summary = match undo.as_slice() {
        [one] => one.summary(world),
        many => format!("{} changes", many.len()),
    };
    let reverted_tx = {
        let mut s = world.res_mut::<LiveSession>();
        if let Some(t) = s.history.iter_mut().find(|t| t.tx == w.tx) {
            t.status = TxStatus::AutoReverted;
            t.revert_reason = Some(reason.to_string());
        }
        s.next_tx += 1;
        let n = s.next_tx;
        s.push(Tx {
            tx: n,
            cmd: "auto_revert".to_string(),
            note: format!("Auto-revert of {} ({reason})", w.tx),
            source: "page".to_string(),
            at: now_iso(),
            frame: frame + 1,
            changes: undo,
            summary,
            status: TxStatus::Applied,
            revert_reason: None,
            edit: None,
        });
        n
    };
    let over_pct = if w.target_ms > 0.0 {
        ((observed / w.target_ms - 1.0) * 100.0).round()
    } else {
        0.0
    };
    let ev = json!({ "type": "event", "v": 1, "event": "auto_revert", "data": {
        "tx": w.tx, "reason": reason, "reverted_tx": reverted_tx,
        "detail": { "target_ms": round2(w.target_ms), "limit_ms": round2(w.limit_ms),
                    "observed_ms": round2(observed), "over_pct": over_pct,
                    "sustain_s": w.sustain_s, "measure": "work_ms" },
        "undone": undone, "note": note } });
    world.res_mut::<LiveSession>().events.push(ev.to_string());
    push_mirror(world);
}

/// Push `session.mirror` (10.3): a compact copy of history and snapshots.
/// The page does not restore from it yet.
pub fn push_mirror(world: &World) {
    let ev = {
        let s = world.res::<LiveSession>();
        let history: Vec<Value> = s
            .history
            .iter()
            .map(|t| {
                json!({ "tx": t.tx, "cmd": t.cmd, "note": t.note, "source": t.source, "at": t.at,
                        "frame": t.frame, "summary": t.summary, "status": t.status.as_str(),
                        "revert_reason": t.revert_reason })
            })
            .collect();
        let snapshots: Vec<Value> = s
            .snapshots
            .iter()
            .map(|p| json!({ "id": p.id, "name": p.name, "tx": p.tx, "at": p.at, "note": p.note }))
            .collect();
        json!({ "type": "event", "v": 1, "event": "session.mirror", "data": {
            "head": s.head_tx(), "next_tx": s.next_tx, "history": history, "snapshots": snapshots } })
    };
    world.res_mut::<LiveSession>().events.push(ev.to_string());
}
