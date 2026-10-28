//! Command processing: envelope checks, read commands, planning mutating
//! commands into [`Change`]s, committing them as one transaction, history
//! and snapshots. See docs/LIVE_API.md sections 5 and 6.

use evani_core::curve::{Curve, MAX_POINTS};
use evani_core::effects::{BudgetStatus, EffectsStatus};
use evani_core::formulas::{FormulaError, FormulaJson, Formulas};
use evani_core::params::{ParamError, ParamId, ParamRegistry, ParamValue};
use evani_core::slots::{ShaderSlots, SlotCompileResult, SlotId, SlotRebuilds, check_body};
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use serde_json::{Map, Value, json};

use crate::change::{
    Change, FullState, apply_all, body_hash, curve_points_json, formula_text, is_curve,
};
use crate::session::{
    AmendMode, LiveSession, NO_RENDERER_FRAMES, Parked, PendingSlot, SLOT_TIMEOUT_MS, Snapshot,
    TierAmend, Tx, TxStatus, now_iso,
};
use crate::{LiveError, MUTATING, param_error};

/// A WGSL slot body that must compile before the plan can commit.
#[derive(Debug, Clone, PartialEq)]
pub struct WgslReq {
    /// `batch` part index (0 outside a batch).
    pub part: usize,
    pub slot: SlotId,
    pub name: String,
    pub old: String,
    pub new: String,
}

/// A validated mutating command: what it will change, nothing applied yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub changes: Vec<Change>,
    /// `applied[]` entries of the change result.
    pub applied: Vec<Value>,
    /// Slot compiles to wait for (renderer) before committing.
    pub wgsl: Vec<WgslReq>,
    /// `compile` of an expression slot (set when one was involved).
    pub compile: Option<Value>,
}

impl Plan {
    fn extend(&mut self, other: Plan) {
        self.changes.extend(other.changes);
        self.applied.extend(other.applied);
        self.wgsl.extend(other.wgsl);
        if other.compile.is_some() {
            self.compile = other.compile;
        }
    }
}

fn reply_envelope(id: &Value, res: Result<Value, LiveError>) -> String {
    match res {
        Ok(result) => json!({ "type": "result", "v": 1, "id": id, "ok": true, "result": result }),
        Err(e) => {
            let mut err = json!({ "code": e.code, "message": e.message });
            if let Some(d) = e.data {
                err["data"] = d;
            }
            json!({ "type": "result", "v": 1, "id": id, "ok": false, "error": err })
        }
    }
    .to_string()
}

pub(crate) fn reply(world: &World, token: u64, id: &Value, res: Result<Value, LiveError>) {
    let text = reply_envelope(id, res);
    world.res_mut::<LiveSession>().reply(token, text);
}

use evani_core::ecs::World;

/// Check the envelope; returns `(cmd, params)`.
fn check_envelope(req: &Value) -> Result<(&str, Value), LiveError> {
    if req.get("type").and_then(Value::as_str) != Some("cmd") {
        return Err(LiveError::new("bad_request", "type must be \"cmd\""));
    }
    if req.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(LiveError::new(
            "version_mismatch",
            "only protocol version 1 is supported",
        ));
    }
    match req.get("id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() && id.len() <= 64 => {}
        _ => {
            return Err(LiveError::new(
                "bad_request",
                "id must be a string of 1-64 chars",
            ));
        }
    }
    let Some(cmd) = req.get("cmd").and_then(Value::as_str) else {
        return Err(LiveError::new("bad_request", "missing cmd"));
    };
    let params = req
        .get("params")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    if !params.is_object() {
        return Err(LiveError::new("bad_request", "params must be an object"));
    }
    if MUTATING.contains(&cmd) {
        let note = req.get("note").and_then(Value::as_str).unwrap_or("");
        if note.trim().is_empty() || note.len() > 500 {
            return Err(LiveError::new(
                "note_required",
                "mutating commands need a note of 1-500 chars saying why",
            ));
        }
    }
    Ok((cmd, params))
}

/// Process one request (called by [`crate::tick`] in arrival order).
pub(crate) fn process(world: &World, token: u64, text: &str) {
    let req: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let err = LiveError::new("bad_request", format!("not JSON: {e}"));
            return reply(world, token, &Value::Null, Err(err));
        }
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let (cmd, params) = match check_envelope(&req) {
        Ok(c) => c,
        Err(e) => return reply(world, token, &id, Err(e)),
    };
    if let Some(e) = crate::capture::busy(world, cmd) {
        return reply(world, token, &id, Err(e));
    }
    let res = match cmd {
        "schema.get" => Ok(crate::schema_filtered(world, &params)),
        "param.get" => crate::param_get(world, &params),
        "session.status" => Ok(crate::session_status(world)),
        "slot.get" => slot_get(world, &params),
        "curve.get" => curve_get(world, &params),
        "policy.get" => Ok(policy_get(world)),
        "history.list" => Ok(history_list(world, &params)),
        "bake.export" => crate::bake::export(world, &params),
        "snapshot.save" => snapshot_save(world, &req, &params),
        "snapshot.list" => Ok(snapshot_list(world)),
        "focus.list" => Ok(crate::focus::list(world)),
        "focus.set" => crate::focus::set(world, &params),
        "focus.clear" => Ok(crate::focus::clear(world)),
        "asset.list" => crate::focus::asset_list(world, &params),
        "asset.focus" => crate::focus::asset_focus(world, &params),
        "asset.clear" => Ok(crate::focus::asset_clear(world)),
        // Drop-to-convert (LIVE_API.md 16): the page loads and compares;
        // the bridge converts and reports.
        #[cfg(feature = "models")]
        "model.load" => match crate::convert::load(world, token, &id, &params) {
            Ok(Some(now)) => Ok(now),
            Ok(None) => return,
            Err(e) => Err(e),
        },
        #[cfg(feature = "models")]
        "model.compare" => crate::convert::compare(world, &params),
        #[cfg(not(feature = "models"))]
        "model.load" | "model.compare" => Err(LiveError::new(
            "not_compiled_in",
            "this game is built without models: add models = true to [modules] in evani.toml",
        )),
        "model.convert" | "model.report" => Err(LiveError::new(
            "unknown_command",
            "model.convert and model.report run in the live bridge: send them through it",
        )),
        "telemetry.subscribe" => crate::telemetry::subscribe(world, &params),
        "telemetry.unsubscribe" => crate::telemetry::unsubscribe(world, &params),
        "debug.geometry" => match crate::geometry::command(world, token, &id, &params) {
            Ok(Some(now)) => Ok(now),
            Ok(None) => return,
            Err(e) => Err(e),
        },
        c if crate::edits::COMMANDS.contains(&c) => {
            return crate::edits::command(world, token, &id, &req, c, &params);
        }
        "history.undo" => return history_step(world, token, &id, &req, &params, false),
        "history.redo" => return history_step(world, token, &id, &req, &params, true),
        c if MUTATING.contains(&c) => {
            match plan(world, c, &params) {
                Ok(p) if p.wgsl.is_empty() => commit(world, token, &id, &req, p, None),
                Ok(p) => park(world, token, &id, &req, p),
                Err(e) => reply(world, token, &id, Err(e)),
            }
            return;
        }
        c if crate::capture::RUN_COMMANDS.contains(&c) => {
            match crate::capture::start(world, token, &id, &req, c, &params) {
                Ok(()) => return,
                Err(e) => Err(e),
            }
        }
        c => match world.try_res::<crate::AppCommands>().map(|a| *a) {
            Some(app) if c.starts_with(app.prefix) => {
                let call = crate::AppCall { token, id: id.clone(), cmd: c.to_string(), params };
                match (app.handle)(world, &call) {
                    Some(res) => res,
                    None => return,
                }
            }
            _ => Err(LiveError::new("unknown_command", format!("no command {c}"))),
        },
    };
    reply(world, token, &id, res);
}

// ---- planning ---------------------------------------------------------------------

/// Validate a mutating command and build its changes without applying them.
pub fn plan(world: &World, cmd: &str, params: &Value) -> Result<Plan, LiveError> {
    match cmd {
        "param.set" => {
            let values = params
                .get("values")
                .and_then(Value::as_object)
                .ok_or_else(|| LiveError::new("bad_request", "params.values must be an object"))?;
            let entries: Vec<(String, Value, Option<String>)> = values
                .iter()
                .map(|(k, v)| (k.clone(), v.clone(), None))
                .collect();
            plan_params(world, &entries)
        }
        "policy.set" => plan_policy(world, params),
        "slot.set" => plan_slot(world, params, 0),
        "curve.set" => plan_curves(world, params),
        "effect.toggle" => plan_effect(world, params),
        "tier.set" => plan_tier(world, params),
        "snapshot.restore" => plan_restore(world, params),
        "batch" => plan_batch(world, params),
        c => Err(LiveError::new(
            "unknown_command",
            format!("{c} is not a mutating command"),
        )),
    }
}

fn plan_params(
    world: &World,
    entries: &[(String, Value, Option<String>)],
) -> Result<Plan, LiveError> {
    let reg = world.res::<ParamRegistry>();
    let mut p = Plan::default();
    for (name, requested, field) in entries {
        let id = reg
            .id(name)
            .ok_or_else(|| param_error(&reg, ParamError::Unknown(name.clone())))?;
        let Some(def) = reg.def(id) else {
            return Err(LiveError::new(
                "internal",
                format!("{name} has no definition"),
            ));
        };
        let v = def
            .value_from_json(requested)
            .map_err(|e| param_error(&reg, e))?;
        let (new, clamped) = def.clamp(v).map_err(|e| param_error(&reg, e))?;
        let old = reg
            .value(id)
            .ok_or_else(|| LiveError::new("internal", format!("{name} has no value")))?;
        let mut a = json!({
            "kind": crate::change::param_kind(name),
            "name": name,
            "old": def.value_to_json(&old),
            "new": def.value_to_json(&new),
            "requested": requested,
            "clamped": clamped,
        });
        if let Some(f) = field {
            a["field"] = json!(f);
        }
        p.applied.push(a);
        p.changes.push(Change::Param { id, old, new });
    }
    Ok(p)
}

fn plan_policy(world: &World, params: &Value) -> Result<Plan, LiveError> {
    let set = params
        .get("set")
        .and_then(Value::as_object)
        .ok_or_else(|| LiveError::new("bad_request", "params.set must be an object"))?;
    let mut entries = Vec::new();
    for (k, v) in set {
        if k == "weights" {
            let w = v
                .as_object()
                .ok_or_else(|| LiveError::new("invalid_value", "weights must be an object"))?;
            for (effect, weight) in w {
                entries.push((
                    format!("fx.{effect}.value_weight"),
                    weight.clone(),
                    Some(format!("weights.{effect}")),
                ));
            }
        } else if k == "degrade_order" && !v.is_string() {
            return Err(LiveError::new(
                "invalid_value",
                "degrade_order must be one of the named orders; explicit effect lists are not supported yet",
            )
            .with(json!({ "options": evani_core::quality::DEGRADE_ORDERS })));
        } else {
            entries.push((format!("aq.{k}"), v.clone(), Some(k.clone())));
        }
    }
    plan_params(world, &entries)
}

/// Expression-error position → `(line, col, source_line)`, 1-based, within
/// the submitted body.
pub fn line_col(src: &str, pos: usize) -> (u32, u32, String) {
    let pos = pos.min(src.len());
    let mut line_start = 0;
    let mut line = 1u32;
    for (i, ch) in src.char_indices() {
        if i >= pos {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = src.get(line_start..pos).map_or(0, |s| s.chars().count()) as u32 + 1;
    let text = src[line_start..].lines().next().unwrap_or("").to_string();
    (line, col, text)
}

pub(crate) fn compile_error(message: &str, messages: Vec<Value>, ms: f64) -> LiveError {
    LiveError::new("compile_error", message.to_string())
        .with(json!({ "ok": false, "ms": ms, "kept": "previous", "messages": messages }))
}

pub(crate) fn plan_slot(world: &World, params: &Value, part: usize) -> Result<Plan, LiveError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.name must be a string"))?;
    let source = params
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.source must be a string"))?;
    let mut p = Plan::default();
    // A WGSL shader slot?
    if let Some(slots) = world.try_res::<ShaderSlots>()
        && let Some(id) = slots.id(name)
    {
        let old = slots.get(id).map(|d| d.body.clone()).unwrap_or_default();
        if let Err(e) = check_body(source) {
            let m = json!({ "severity": "error", "line": 0, "col": 0, "len": 0,
                            "message": e.to_string(), "source_line": null });
            return Err(compile_error(&e.to_string(), vec![m], 0.0));
        }
        p.applied.push(json!({ "kind": "slot", "name": name,
            "old_hash": body_hash(&old), "new_hash": body_hash(source) }));
        if old != source {
            p.changes.push(Change::Slot {
                id,
                old: old.clone(),
                new: source.to_string(),
            });
            p.wgsl.push(WgslReq {
                part,
                slot: id,
                name: name.to_string(),
                old,
                new: source.to_string(),
            });
        } else {
            p.compile =
                Some(json!({ "ok": true, "ms": 0, "messages": [], "pipelines_rebuilt": [] }));
        }
        return Ok(p);
    }
    // An expression formula?
    let f = world.try_res::<Formulas>();
    let fid = f.as_ref().and_then(|f| f.id(name));
    let (Some(f), Some(id)) = (f, fid) else {
        return Err(unknown_slot(world, name));
    };
    if is_curve(&f, id) {
        return Err(LiveError::new(
            "invalid_value",
            format!("{name} is a curve; use curve.set"),
        ));
    }
    let old = f
        .to_json_body(id)
        .ok_or_else(|| LiveError::new("internal", format!("{name} has no body")))?;
    let new = FormulaJson::Expr(source.to_string());
    // Compile on a copy: exactly what apply will do, without touching it.
    match f.clone().set(id, new.clone()) {
        Ok(_) => {}
        Err(FormulaError::Expr(e)) => {
            let (line, col, text) = line_col(source, e.pos);
            let m = json!({ "severity": "error", "line": line, "col": col, "len": 1,
                            "message": e.message, "source_line": text });
            return Err(compile_error(&e.message, vec![m], 0.0));
        }
        Err(e) => return Err(LiveError::new("invalid_value", e.to_string())),
    }
    p.applied.push(json!({ "kind": "slot", "name": name,
        "old_hash": body_hash(&formula_text(&old)), "new_hash": body_hash(source) }));
    p.compile = Some(json!({ "ok": true, "ms": 0, "messages": [], "pipelines_rebuilt": [] }));
    if old != new {
        p.changes.push(Change::Formula { id, old, new });
    }
    Ok(p)
}

fn slot_names(world: &World) -> Vec<String> {
    let mut names: Vec<String> = world.try_res::<ShaderSlots>().map_or_else(Vec::new, |s| {
        s.iter()
            .map(|(_, d)| d.signature.name.to_string())
            .collect()
    });
    if let Some(f) = world.try_res::<Formulas>() {
        names.extend(
            f.ids()
                .filter(|&id| !is_curve(&f, id))
                .filter_map(|id| f.def(id).map(|d| d.name.clone())),
        );
    }
    names
}

fn curve_names(world: &World) -> Vec<String> {
    world.try_res::<Formulas>().map_or_else(Vec::new, |f| {
        f.ids()
            .filter(|&id| is_curve(&f, id))
            .filter_map(|id| f.def(id).map(|d| d.name.clone()))
            .collect()
    })
}

fn unknown_slot(world: &World, name: &str) -> LiveError {
    LiveError::new("unknown_target", format!("No slot named {name}"))
        .with(json!({ "did_you_mean": close_names(name, &slot_names(world)) }))
}

fn close_names(name: &str, names: &[String]) -> Vec<String> {
    let parts: Vec<&str> = name.split('.').collect();
    for keep in (1..parts.len()).rev() {
        let prefix = format!("{}.", parts[..keep].join("."));
        let hits: Vec<String> = names
            .iter()
            .filter(|n| n.starts_with(&prefix))
            .take(3)
            .cloned()
            .collect();
        if !hits.is_empty() {
            return hits;
        }
    }
    names.iter().take(5).cloned().collect()
}

fn plan_curves(world: &World, params: &Value) -> Result<Plan, LiveError> {
    let curves = params
        .get("curves")
        .and_then(Value::as_object)
        .ok_or_else(|| LiveError::new("bad_request", "params.curves must be an object"))?;
    let f = world.res::<Formulas>();
    let mut p = Plan::default();
    for (name, pts) in curves {
        let id = f.id(name).filter(|&id| is_curve(&f, id)).ok_or_else(|| {
            LiveError::new("unknown_target", format!("No curve named {name}"))
                .with(json!({ "did_you_mean": close_names(name, &curve_names(world)) }))
        })?;
        let bad = |m: String| LiveError::new("invalid_value", format!("{name}: {m}"));
        let arr = pts
            .as_array()
            .ok_or_else(|| bad("points must be an array of [x, y]".into()))?;
        if arr.len() < 2 || arr.len() > MAX_POINTS {
            return Err(bad(format!(
                "needs 2 to {MAX_POINTS} points, got {}",
                arr.len()
            )));
        }
        let mut points = Vec::with_capacity(arr.len());
        for pt in arr {
            let xy = pt
                .as_array()
                .filter(|a| a.len() == 2)
                .and_then(|a| Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32]))
                .ok_or_else(|| bad("each point must be [x, y] numbers".into()))?;
            if !xy[0].is_finite() || !xy[1].is_finite() {
                return Err(bad("points must be finite".into()));
            }
            points.push(xy);
        }
        if points.windows(2).any(|w| w[1][0] <= w[0][0]) {
            return Err(bad("x values must be strictly increasing".into()));
        }
        let def = f.def(id).cloned();
        let (xr, yr) = def.map_or((None, None), |d| (d.x_range, d.y_range));
        let mut clamped = false;
        for pt in &mut points {
            for (axis, r) in [(0, xr), (1, yr)] {
                if let Some([lo, hi]) = r {
                    let c = pt[axis].clamp(lo, hi);
                    if c != pt[axis] {
                        clamped = true;
                        pt[axis] = c;
                    }
                }
            }
        }
        if points.windows(2).any(|w| w[1][0] <= w[0][0]) {
            return Err(bad(
                "two points share an x value after clamping to the x range".into(),
            ));
        }
        let curve = Curve::new(&points).map_err(|e| bad(e.to_string()))?;
        let new = FormulaJson::Curve(curve);
        let old = f
            .to_json_body(id)
            .ok_or_else(|| LiveError::new("internal", format!("{name} has no body")))?;
        p.applied.push(
            json!({ "kind": "curve", "name": name, "old": curve_points_json(&old),
            "new": curve_points_json(&new), "requested": pts, "clamped": clamped }),
        );
        if old != new {
            p.changes.push(Change::Formula { id, old, new });
        }
    }
    Ok(p)
}

fn plan_effect(world: &World, params: &Value) -> Result<Plan, LiveError> {
    let name = params
        .get("effect")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.effect must be a string"))?;
    let on = params
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| LiveError::new("bad_request", "params.on must be true or false"))?;
    let scope = params
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("session");
    if scope != "session" && scope != "tier" {
        return Err(LiveError::new(
            "invalid_value",
            format!("unknown scope {scope}"),
        ));
    }
    let (enabled, info) = {
        let fx = world.try_res::<EffectsStatus>();
        let Some(e) = fx.as_ref().and_then(|f| f.get(name)).copied() else {
            return Err(LiveError::new(
                "not_compiled_in",
                format!("effect {name} is not in this build"),
            ));
        };
        (e.enabled, e.info)
    };
    let tiers = world.res::<Tiers>();
    if on && tiers.active().stereo && info.vr_comfort_off {
        return Err(
            LiveError::new("invalid_value", format!("{name} is comfort-off in VR"))
                .with(json!({ "reason": "vr_comfort" })),
        );
    }
    let Some(h) = enabled else {
        return Err(LiveError::new(
            "internal",
            format!("{name} has no enabled param"),
        ));
    };
    let mut p = Plan::default();
    if scope == "tier" {
        let t = tiers.active();
        let old = t.effects_on.clone();
        let mut new: Vec<String> = old.iter().filter(|e| e.as_str() != name).cloned().collect();
        if on {
            new.push(name.to_string());
        }
        if new != old {
            p.changes.push(Change::TierEffects {
                tier: t.id.clone(),
                old,
                new,
            });
        }
    }
    drop(tiers);
    let old = world.res::<ParamRegistry>().bool(h);
    if old != on {
        p.changes.push(Change::Param {
            id: h.id(),
            old: ParamValue::Bool(old),
            new: ParamValue::Bool(on),
        });
    }
    p.applied
        .push(json!({ "kind": "effect", "name": name, "scope": scope, "old": old, "new": on }));
    Ok(p)
}

fn plan_tier(world: &World, params: &Value) -> Result<Plan, LiveError> {
    let id = params
        .get("tier")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.tier must be a string"))?;
    let tiers = world.res::<Tiers>();
    let selectable = || -> Vec<String> { tiers.selectable().map(|(_, p)| p.id.clone()).collect() };
    match tiers.get(id) {
        None => {
            return Err(
                LiveError::new("unknown_target", format!("No tier named {id}"))
                    .with(json!({ "did_you_mean": selectable() })),
            );
        }
        // The stereo (VR) preset only runs inside an immersive WebXR session.
        Some(p) if !tiers.is_selectable(p) => {
            return Err(LiveError::new(
                "needs_xr_session",
                format!("Tier {id} runs only inside an immersive WebXR session (Enter VR)"),
            )
            .with(json!({ "selectable": selectable() })));
        }
        Some(_) => {}
    }
    let old = tiers.active().id.clone();
    let mut p = Plan::default();
    p.applied
        .push(json!({ "kind": "tier", "name": "tier", "old": old, "new": id }));
    if old != id {
        p.changes.push(Change::Tier {
            old,
            new: id.to_string(),
        });
    }
    Ok(p)
}

fn plan_restore(world: &World, params: &Value) -> Result<Plan, LiveError> {
    let sid = params
        .get("snapshot")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.snapshot must be a string"))?;
    let target = {
        let s = world.res::<LiveSession>();
        match s.snapshots.iter().find(|x| x.id == sid || x.name == sid) {
            Some(x) => x.state.clone(),
            None => {
                let ids: Vec<&str> = s.snapshots.iter().map(|x| x.id.as_str()).collect();
                return Err(
                    LiveError::new("unknown_target", format!("No snapshot {sid}"))
                        .with(json!({ "did_you_mean": ids })),
                );
            }
        }
    };
    let changes = FullState::capture(world).diff(&target);
    let applied = changes.iter().map(|c| c.describe(world, true)).collect();
    Ok(Plan {
        changes,
        applied,
        wgsl: Vec::new(),
        compile: None,
    })
}

fn plan_batch(world: &World, params: &Value) -> Result<Plan, LiveError> {
    let cmds = params
        .get("cmds")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| LiveError::new("bad_request", "params.cmds must be a non-empty array"))?;
    let mut all = Plan::default();
    for (i, part) in cmds.iter().enumerate() {
        let with_index = |mut e: LiveError| {
            let mut d = e.data.take().unwrap_or_else(|| json!({}));
            if !d.is_object() {
                d = json!({ "detail": d });
            }
            d["index"] = json!(i);
            e.data = Some(d);
            e
        };
        let cmd = part.get("cmd").and_then(Value::as_str).unwrap_or("");
        if !MUTATING.contains(&cmd) || cmd == "batch" {
            return Err(with_index(LiveError::new(
                "invalid_value",
                format!(
                    "batch part {i}: only mutating commands (not batch) are allowed, got {cmd:?}"
                ),
            )));
        }
        let empty = json!({});
        let p = part.get("params").unwrap_or(&empty);
        let planned = if cmd == "slot.set" {
            plan_slot(world, p, i)
        } else {
            plan(world, cmd, p)
        };
        all.extend(planned.map_err(with_index)?);
    }
    Ok(all)
}

// ---- committing -------------------------------------------------------------------

pub(crate) fn frame_now(world: &World) -> (u64, f64) {
    world
        .try_res::<Time>()
        .map_or((0, 0.0), |t| (t.frame, t.now_ms))
}

fn change_result(
    world: &World,
    tx: u64,
    frame: u64,
    applied: Vec<Value>,
    compile: Option<Value>,
) -> Value {
    let budget = *world.res::<BudgetStatus>();
    let tier = world.res::<Tiers>().active().id.clone();
    json!({
        "tx": tx,
        "frame": frame,
        "applied": applied,
        "compile": compile,
        "budget": { "tier": tier, "points_used": budget.used, "points_limit": budget.total,
                    "switched_off": [], "switched_on": [] },
        "telemetry": null,
        "capture": null,
    })
}

fn tier_info(world: &World) -> Value {
    let tiers = world.res::<Tiers>();
    let t = tiers.active();
    json!({ "active": t.id, "render_scale": t.render_scale_or_native(),
            "dpr_cap": t.dpr_cap, "points_limit": t.points })
}

/// The single place a mutating command's reply leaves. b2 hooks here:
/// defer the reply by `opts.telemetry_ms` of telemetry, fill `telemetry`
/// (and `capture`), and arm the auto-revert watch for `tx` when `arm`.
pub fn finish_mutation(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    result: Value,
    arm: bool,
) {
    crate::revert::finish(world, token, id, req, result, arm);
}

/// Send any reply (run results and their errors).
pub(crate) fn send_reply(world: &World, token: u64, id: &Value, res: Result<Value, LiveError>) {
    reply(world, token, id, res);
}

/// Send a finished change result.
pub(crate) fn send_result(world: &World, token: u64, id: &Value, result: Value) {
    reply(world, token, id, Ok(result));
}

/// Apply a plan and record it as one transaction. A staged edit (`req`
/// carries `edit`, 6.31) records its outcome in its edit transaction and
/// adds `txn`, `edit` and `state` to the result.
pub(crate) fn commit(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    plan: Plan,
    compile: Option<Value>,
) {
    match commit_tx(world, req, plan, compile) {
        Ok(result) => finish_mutation(world, token, id, req, result, true),
        Err(e) => crate::edits::fail_or_reply(world, token, id, req, e),
    }
}

/// Apply a plan, push its history entry and build its change result
/// (telemetry and watch not filled yet). Nothing is applied on `Err`.
pub(crate) fn commit_tx(
    world: &World,
    req: &Value,
    plan: Plan,
    compile: Option<Value>,
) -> Result<Value, LiveError> {
    let tier_change = plan
        .changes
        .iter()
        .any(|c| matches!(c, Change::Tier { .. }));
    let pre = if tier_change {
        effect_switches(world)
    } else {
        Vec::new()
    };
    apply_all(world, &plan.changes)?;
    let (frame, _) = frame_now(world);
    let frame = frame + 1;
    let summary = match plan.changes.as_slice() {
        [] => "no change".to_string(),
        [one] => one.summary(world),
        many => format!("{} changes", many.len()),
    };
    let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
    let has_tier = tier_change || cmd == "tier.set";
    let mode = if cmd == "snapshot.restore" {
        AmendMode::Reassert(effect_switches(world))
    } else {
        let ids: Vec<ParamId> = pre.iter().map(|(i, _)| *i).collect();
        AmendMode::Defaults {
            explicit: plan
                .changes
                .iter()
                .filter_map(|c| match c {
                    Change::Param { id, new, .. } if ids.contains(id) => Some((*id, *new)),
                    _ => None,
                })
                .collect(),
        }
    };
    let tx = {
        let mut s = world.res_mut::<LiveSession>();
        s.next_tx += 1;
        let tx = s.next_tx;
        s.push(Tx {
            tx,
            cmd: cmd.to_string(),
            note: req
                .get("note")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            source: req
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("page")
                .to_string(),
            at: now_iso(),
            frame,
            changes: plan.changes,
            summary,
            status: TxStatus::Applied,
            revert_reason: None,
            edit: crate::edits::edit_of(req),
        });
        if tier_change {
            s.tier_amend = Some(TierAmend {
                tx: Some(tx),
                pre,
                mode,
            });
        }
        tx
    };
    let mut result = change_result(world, tx, frame, plan.applied, compile.or(plan.compile));
    if has_tier {
        result["tier"] = tier_info(world);
    }
    crate::edits::on_committed(world, req, tx, &mut result);
    Ok(result)
}

/// Every effect's `fx.<name>.enabled` value.
pub(crate) fn effect_switches(world: &World) -> Vec<(ParamId, ParamValue)> {
    let Some(fx) = world.try_res::<EffectsStatus>() else {
        return Vec::new();
    };
    let reg = world.res::<ParamRegistry>();
    fx.effects
        .iter()
        .filter_map(|e| e.enabled)
        .map(|h| (h.id(), ParamValue::Bool(reg.bool(h))))
        .collect()
}

/// Settle effect switches after a tier change (the tick after it, once the
/// effects module has applied the new tier's defaults). Returns true when
/// it ran.
pub(crate) fn run_tier_amend(world: &World) -> bool {
    let Some(a) = world.res_mut::<LiveSession>().tier_amend.take() else {
        return false;
    };
    let set = |vals: &[(ParamId, ParamValue)]| {
        let mut reg = world.res_mut::<ParamRegistry>();
        for (id, v) in vals {
            let _ = reg.set(*id, *v);
        }
    };
    match &a.mode {
        AmendMode::Defaults { explicit } => set(explicit),
        AmendMode::Reassert(vals) => set(vals),
    }
    let Some(txn) = a.tx else { return true };
    let post = effect_switches(world);
    let ids: Vec<ParamId> = a.pre.iter().map(|(i, _)| *i).collect();
    let mut s = world.res_mut::<LiveSession>();
    if let Some(t) = s.history.iter_mut().find(|t| t.tx == txn) {
        t.changes
            .retain(|c| !matches!(c, Change::Param { id, .. } if ids.contains(id)));
        for (id, old) in &a.pre {
            if let Some((_, new)) = post.iter().find(|(i, _)| i == id)
                && new != old
            {
                t.changes.push(Change::Param {
                    id: *id,
                    old: *old,
                    new: *new,
                });
            }
        }
    }
    true
}

/// Queue the plan's slot compiles and park the command until they finish.
pub(crate) fn park(world: &World, token: u64, id: &Value, req: &Value, plan: Plan) {
    let (_, now) = frame_now(world);
    let mut pending = Vec::new();
    {
        let slots = world.res::<ShaderSlots>();
        let mut rb = world.res_mut::<SlotRebuilds>();
        for w in &plan.wgsl {
            match rb.request(&slots, &w.name, &w.new, now) {
                Ok(handle) => pending.push(PendingSlot {
                    handle,
                    part: w.part,
                    slot: w.slot,
                    old: w.old.clone(),
                    new: w.new.clone(),
                    result: None,
                }),
                Err(e) => {
                    drop(rb);
                    drop(slots);
                    let m = json!({ "severity": "error", "line": 0, "col": 0, "len": 0,
                                    "message": e.to_string(), "source_line": null });
                    return crate::edits::fail_or_reply(
                        world,
                        token,
                        id,
                        req,
                        compile_error(&e.to_string(), vec![m], 0.0),
                    );
                }
            }
        }
    }
    world.res_mut::<LiveSession>().parked = Some(Parked {
        token,
        id: id.clone(),
        req: req.clone(),
        plan,
        slots: pending,
        frames: 0,
        started_ms: now,
    });
}

pub(crate) fn structural_ok(ms: f64) -> SlotCompileResult {
    SlotCompileResult {
        ok: true,
        ms,
        kept: None,
        messages: Vec::new(),
        pipelines_rebuilt: Vec::new(),
    }
}

pub(crate) fn message_json(m: &evani_core::slots::SlotMessage) -> Value {
    json!({ "severity": m.severity, "line": m.line, "col": m.column, "len": m.length,
            "message": m.text, "source_line": m.source_line })
}

/// Poll a parked command. Returns true when it is finished (or none).
pub(crate) fn poll_parked(world: &World) -> bool {
    let Some(mut parked) = world.res_mut::<LiveSession>().parked.take() else {
        return true;
    };
    parked.frames += 1;
    let (_, now) = frame_now(world);
    let mut self_completed = false;
    {
        let mut rb = world.res_mut::<SlotRebuilds>();
        if parked.frames >= NO_RENDERER_FRAMES && rb.has_pending() {
            // Nobody took the requests: no renderer drives slots here.
            for r in rb.take_pending() {
                rb.complete(r.handle, structural_ok((now - r.requested_ms).max(0.0)));
            }
            self_completed = true;
        }
        for s in parked.slots.iter_mut().filter(|s| s.result.is_none()) {
            s.result = rb.take_result(s.handle);
        }
    }
    if parked.slots.iter().any(|s| s.result.is_none()) {
        if now - parked.started_ms > SLOT_TIMEOUT_MS {
            restore_bodies(world, &parked.slots);
            let e = LiveError::new("timeout", "the slot compile did not finish within 8 s")
                .with(json!({ "kept": "previous" }));
            crate::edits::fail_or_reply(world, parked.token, &parked.id, &parked.req, e);
            return true;
        }
        world.res_mut::<LiveSession>().parked = Some(parked);
        return false;
    }
    let ms = parked
        .slots
        .iter()
        .filter_map(|s| s.result.as_ref().map(|r| r.ms))
        .fold(0.0, f64::max);
    let mut messages = Vec::new();
    let mut rebuilt = Vec::new();
    let mut failed: Option<usize> = None;
    for s in &parked.slots {
        if let Some(r) = &s.result {
            messages.extend(r.messages.iter().map(message_json));
            rebuilt.extend(r.pipelines_rebuilt.iter().cloned());
            if !r.ok && failed.is_none() {
                failed = Some(s.part);
            }
        }
    }
    if let Some(part) = failed {
        restore_bodies(world, &parked.slots);
        let first = messages
            .iter()
            .find(|m| m["severity"] == "error")
            .and_then(|m| m["message"].as_str())
            .unwrap_or("slot did not compile")
            .to_string();
        let mut e = compile_error(&first, messages, ms);
        if parked.req.get("cmd").and_then(Value::as_str) == Some("batch")
            && let Some(d) = e.data.as_mut()
        {
            d["index"] = json!(part);
        }
        crate::edits::fail_or_reply(world, parked.token, &parked.id, &parked.req, e);
        return true;
    }
    let mut compile =
        json!({ "ok": true, "ms": ms, "messages": messages, "pipelines_rebuilt": rebuilt });
    if self_completed {
        compile["checked"] = json!("structural");
    }
    commit(
        world,
        parked.token,
        &parked.id,
        &parked.req,
        parked.plan,
        Some(compile),
    );
    true
}

/// Put back the old body of any slot the renderer already swapped in.
pub(crate) fn restore_bodies(world: &World, slots: &[PendingSlot]) {
    let mut ss = world.res_mut::<ShaderSlots>();
    for s in slots {
        if ss.get(s.slot).is_some_and(|d| d.body == s.new) {
            let _ = ss.set_body(s.slot, &s.old);
        }
    }
}

// ---- history ----------------------------------------------------------------------

fn history_step(world: &World, token: u64, id: &Value, req: &Value, params: &Value, redo: bool) {
    let steps = match params.get("steps") {
        None => 1,
        Some(v) => match v.as_u64() {
            Some(n @ 1..=50) => n as usize,
            _ => {
                let e = LiveError::new("invalid_value", "steps must be an integer 1-50");
                return reply(world, token, id, Err(e));
            }
        },
    };
    if !redo
        && let Some(e) = crate::edits::undo_blocked(world, steps)
    {
        return reply(world, token, id, Err(e));
    }
    let mut applied = Vec::new();
    let mut touched = 0u64;
    let mut tier_touched = false;
    let mut done = 0;
    for _ in 0..steps {
        let next = {
            let s = world.res::<LiveSession>();
            if redo {
                s.history
                    .get(s.head)
                    .map(|t| (s.head, t.changes.clone(), t.tx))
            } else {
                s.head
                    .checked_sub(1)
                    .and_then(|i| s.history.get(i).map(|t| (i, t.changes.clone(), t.tx)))
            }
        };
        let Some((i, changes, txn)) = next else { break };
        tier_touched |= changes.iter().any(|c| matches!(c, Change::Tier { .. }));
        let res = if redo {
            apply_all(world, &changes)
        } else {
            let mut r = Ok(());
            for (k, c) in changes.iter().enumerate().rev() {
                if let Err(e) = c.apply(world, false) {
                    // Put this transaction back the way it was.
                    let _ = apply_all(world, &changes[k + 1..]);
                    r = Err(e);
                    break;
                }
            }
            r
        };
        if let Err(e) = res {
            return reply(world, token, id, Err(e));
        }
        if redo {
            applied.extend(changes.iter().map(|c| c.describe(world, true)));
        } else {
            applied.extend(changes.iter().rev().map(|c| c.describe(world, false)));
        }
        let mut s = world.res_mut::<LiveSession>();
        if let Some(t) = s.history.get_mut(i) {
            t.status = if redo {
                TxStatus::Applied
            } else {
                TxStatus::Undone
            };
        }
        s.head = if redo { i + 1 } else { i };
        if redo || done == 0 {
            touched = txn;
        }
        done += 1;
    }
    if done == 0 {
        let what = if redo { "redo" } else { "undo" };
        return reply(
            world,
            token,
            id,
            Err(LiveError::new(
                "history_empty",
                format!("nothing to {what}"),
            )),
        );
    }
    if tier_touched {
        // The effects module resets effect switches for the tier we just
        // moved to; put the history's values back after it has.
        let vals = effect_switches(world);
        world.res_mut::<LiveSession>().tier_amend = Some(TierAmend {
            tx: None,
            pre: Vec::new(),
            mode: AmendMode::Reassert(vals),
        });
    }
    let (frame, _) = frame_now(world);
    let mut result = change_result(world, touched, frame + 1, applied, None);
    {
        let s = world.res::<LiveSession>();
        result["history"] =
            json!({ "head": s.head_tx(), "can_undo": s.can_undo(), "can_redo": s.can_redo() });
    }
    result["tier"] = tier_info(world);
    finish_mutation(world, token, id, req, result, false);
}

fn history_list(world: &World, params: &Value) -> Value {
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 500) as usize;
    let s = world.res::<LiveSession>();
    let entries: Vec<Value> = s
        .history
        .iter()
        .rev()
        .take(limit)
        .map(|t| {
            let mut e = json!({ "tx": t.tx, "at": t.at, "cmd": t.cmd, "source": t.source,
                "note": t.note, "summary": t.summary, "state": t.status.as_str(), "frame": t.frame });
            if let Some(r) = &t.revert_reason {
                e["revert_reason"] = json!(r);
            }
            if let Some((txn, edit)) = &t.edit {
                e["txn"] = json!(txn);
                e["edit"] = json!(edit);
            }
            e
        })
        .collect();
    json!({ "head": s.head_tx(), "can_undo": s.can_undo(), "can_redo": s.can_redo(), "entries": entries })
}

// ---- snapshots --------------------------------------------------------------------

fn snapshot_save(world: &World, req: &Value, params: &Value) -> Result<Value, LiveError> {
    let state = FullState::capture(world);
    let start = world
        .res::<LiveSession>()
        .start_state
        .clone()
        .unwrap_or_else(|| state.clone());
    let diff = start.diff(&state);
    let mut params_changed = 0;
    let mut slots_changed = Vec::new();
    let mut curves_changed = Vec::new();
    for c in &diff {
        match c {
            Change::Param { .. } => params_changed += 1,
            Change::Slot { .. } | Change::Formula { .. } => {
                let d = c.describe(world, true);
                let name = d["name"].clone();
                if d["kind"] == "curve" {
                    curves_changed.push(name)
                } else {
                    slots_changed.push(name)
                }
            }
            _ => {}
        }
    }
    let effects: Map<String, Value> =
        world
            .try_res::<EffectsStatus>()
            .map_or_else(Map::new, |fx| {
                let reg = world.res::<ParamRegistry>();
                fx.effects
                    .iter()
                    .map(|e| {
                        (
                            e.info.name.to_string(),
                            json!(e.enabled.map_or(e.state.is_on(), |h| reg.bool(h))),
                        )
                    })
                    .collect()
            });
    let at = now_iso();
    let mut s = world.res_mut::<LiveSession>();
    s.next_snapshot += 1;
    let sid = format!("snap-{}", s.next_snapshot);
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&sid)
        .to_string();
    let tx = s.head_tx();
    let tier = state.tier.clone();
    s.snapshots.push(Snapshot {
        id: sid.clone(),
        name: name.clone(),
        tx,
        at: at.clone(),
        note: req
            .get("note")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        state,
    });
    Ok(json!({ "snapshot": sid, "name": name, "tx": tx, "at": at,
        "summary": { "params_changed": params_changed, "slots_changed": slots_changed,
                     "curves_changed": curves_changed, "effects": effects, "tier": tier } }))
}

fn snapshot_list(world: &World) -> Value {
    let s = world.res::<LiveSession>();
    let list: Vec<Value> = s
        .snapshots
        .iter()
        .map(
            |x| json!({ "snapshot": x.id, "name": x.name, "tx": x.tx, "at": x.at, "note": x.note }),
        )
        .collect();
    json!({ "snapshots": list })
}

// ---- reads ------------------------------------------------------------------------

fn slot_get(world: &World, params: &Value) -> Result<Value, LiveError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.name must be a string"))?;
    let with_prelude = params
        .get("with_prelude")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(slots) = world.try_res::<ShaderSlots>()
        && let Some(d) = slots.id(name).and_then(|id| slots.get(id))
    {
        let mut r = json!({ "name": name, "lang": "wgsl", "signature": crate::wgsl_signature(d),
            "source": d.body, "default_source": d.default_body, "hash": body_hash(&d.body),
            "is_default": d.body == d.default_body, "owner": d.owner });
        if with_prelude {
            let full = ShaderSlots::assemble(&d.signature, "");
            r["prelude"] = json!(full.lines().next().unwrap_or(""));
        }
        return Ok(r);
    }
    let f = world.try_res::<Formulas>();
    let found = f
        .as_ref()
        .and_then(|f| f.id(name).filter(|&id| !is_curve(f, id)).map(|id| (f, id)));
    let Some((f, id)) = found else {
        return Err(unknown_slot(world, name));
    };
    let src = f
        .to_json_body(id)
        .map(|b| formula_text(&b))
        .unwrap_or_default();
    let def_src = match f.default_body(id) {
        Some(evani_core::formulas::FormulaBody::Expr(e)) => e.source().to_string(),
        _ => String::new(),
    };
    let d = f.def(id);
    Ok(
        json!({ "name": name, "lang": "expr", "signature": crate::expr_signature(d),
        "vars": d.map(|d| d.vars.clone()), "source": src, "default_source": def_src,
        "hash": body_hash(&src), "is_default": src == def_src, "owner": d.map(|d| d.owner.clone()) }),
    )
}

fn range_json(r: Option<[f32; 2]>) -> Value {
    r.map_or(
        json!({ "min": null, "max": null }),
        |[a, b]| json!({ "min": a, "max": b }),
    )
}

fn curve_get(world: &World, params: &Value) -> Result<Value, LiveError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("bad_request", "params.name must be a string"))?;
    let f = world.try_res::<Formulas>();
    let found = f
        .as_ref()
        .and_then(|f| f.id(name).filter(|&id| is_curve(f, id)).map(|id| (f, id)));
    let Some((f, id)) = found else {
        return Err(
            LiveError::new("unknown_target", format!("No curve named {name}"))
                .with(json!({ "did_you_mean": close_names(name, &curve_names(world)) })),
        );
    };
    let d = f.def(id);
    Ok(json!({ "name": name,
        "points": f.to_json_body(id).map(|b| curve_points_json(&b)),
        "x": range_json(d.and_then(|d| d.x_range)), "y": range_json(d.and_then(|d| d.y_range)),
        "max_points": MAX_POINTS }))
}

fn policy_get(world: &World) -> Value {
    let reg = world.res::<ParamRegistry>();
    let mut out = Map::new();
    let mut weights = Map::new();
    for d in reg.defs() {
        let Some(v) = reg.id(&d.name).and_then(|id| reg.value(id)) else {
            continue;
        };
        if let Some(field) = d.name.strip_prefix("aq.") {
            out.insert(field.to_string(), d.value_to_json(&v));
        } else if let Some(effect) = d
            .name
            .strip_suffix(".value_weight")
            .and_then(|n| n.strip_prefix("fx."))
        {
            weights.insert(effect.to_string(), d.value_to_json(&v));
        }
    }
    if out
        .get("target_ms")
        .and_then(Value::as_f64)
        .is_none_or(|t| t <= 0.0)
    {
        out.insert(
            "target_ms".into(),
            json!(world.res::<Tiers>().active().frame_budget_ms()),
        );
    }
    out.insert("weights".into(), Value::Object(weights));
    Value::Object(out)
}
