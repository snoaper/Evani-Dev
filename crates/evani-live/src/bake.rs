//! `bake.export` (docs/LIVE_API.md 6.23): turn the tuned session into source
//! files.
//!
//! The page can't write files, so the work is split in two:
//!
//! * [`export`] (page side) builds a *bundle*: every value that differs from
//!   the session's start state and was touched by an applied transaction,
//!   with its bake target, plus the CHANGES.md note lines.
//! * [`plan`] (native only, used by the bridge) merges the bundle into the
//!   files as they are on disk and returns exactly what to write. It is pure:
//!   the caller passes a reader and does the writing.

use std::collections::BTreeMap;

use evani_core::ecs::World;
use evani_core::formulas::{FormulaId, FormulaJson, Formulas};
use evani_core::params::{BakeTarget, ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::BuildInfo;
use evani_core::slots::{ShaderSlots, SlotId};
use evani_core::tiers::Tiers;
use serde_json::{Map, Value, json};

use crate::LiveError;
use crate::change::{Change, FullState};
use crate::session::{LiveSession, Tx, TxStatus};

/// Everything `include` may name.
pub const INCLUDE_ALL: &[&str] = &["tiers", "params", "slots", "curves", "policy"];

/// Who a transaction's source is, as written in CHANGES.md.
fn author(source: &str) -> &str {
    match source {
        "mcp" => "AI",
        "page" => "you, debug panel",
        "ctl" => "you, evani-ctl",
        "" => "unknown",
        s => s,
    }
}

/// An f32 as the shortest decimal that reads back to the same f32 (so
/// `0.9` is written as `0.9`, not `0.8999999761581421`).
fn f32_json(x: f32) -> Value {
    x.to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

fn param_json(reg: &ParamRegistry, id: ParamId, v: &ParamValue) -> Value {
    match *v {
        ParamValue::F32(x) => f32_json(x),
        ParamValue::Vec3(a) | ParamValue::Color(a) => {
            Value::Array(a.iter().map(|x| f32_json(*x)).collect())
        }
        _ => reg.def(id).map_or(Value::Null, |d| d.value_to_json(v)),
    }
}

fn points_json(f: &FormulaJson) -> Value {
    match f {
        FormulaJson::Curve(c) => Value::Array(
            c.points()
                .iter()
                .map(|p| Value::Array(vec![f32_json(p[0]), f32_json(p[1])]))
                .collect(),
        ),
        FormulaJson::Expr(_) => Value::Null,
    }
}

/// A value for a note line: strings bare, everything else compact JSON.
fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

fn target_str(t: BakeTarget) -> Option<&'static str> {
    match t {
        BakeTarget::Look => Some("look"),
        BakeTarget::Kit => Some("kit"),
        BakeTarget::Tier => Some("tier"),
        BakeTarget::Policy => Some("policy"),
        BakeTarget::Auto | BakeTarget::None => None,
    }
}

/// `look.basic` / `kit.example` → `look` / `kit`; anything else has no crate.
fn owner_target(owner: &str) -> Option<&'static str> {
    match owner.split('.').next() {
        Some("look") => Some("look"),
        Some("kit") => Some("kit"),
        _ => None,
    }
}

/// Every applied transaction touching each item (oldest first), for notes and authors.
#[derive(Default)]
struct Touched<'a> {
    params: BTreeMap<ParamId, Vec<&'a Tx>>,
    slots: BTreeMap<SlotId, Vec<&'a Tx>>,
    formulas: BTreeMap<FormulaId, Vec<&'a Tx>>,
    tiers: BTreeMap<String, Vec<&'a Tx>>,
}

fn touched(history: &[Tx]) -> Touched<'_> {
    let mut t = Touched::default();
    for tx in history.iter().filter(|t| t.status == TxStatus::Applied) {
        for c in &tx.changes {
            match c {
                Change::Param { id, .. } => {
                    t.params.entry(*id).or_default().push(tx);
                }
                Change::Slot { id, .. } => {
                    t.slots.entry(*id).or_default().push(tx);
                }
                Change::Formula { id, .. } => {
                    t.formulas.entry(*id).or_default().push(tx);
                }
                Change::TierEffects { tier, .. } => {
                    t.tiers.entry(tier.clone()).or_default().push(tx);
                }
                // The active tier is a runtime choice and is never baked.
                Change::Tier { .. } => {}
            }
        }
    }
    t
}

/// The notes of every transaction that changed one value, in order, each
/// with its author; repeats are dropped.
fn note_tail(txs: &[&Tx]) -> String {
    let mut pieces: Vec<String> = Vec::new();
    let mut any_note = false;
    for tx in txs {
        let note = tx.note.trim();
        let who = author(&tx.source);
        any_note |= !note.is_empty();
        let piece = if note.is_empty() {
            format!("*({who})*")
        } else {
            format!("{note} *({who})*")
        };
        if !pieces.contains(&piece) {
            pieces.push(piece);
        }
    }
    let joined = pieces.join("; ");
    if any_note {
        format!("— {joined}")
    } else {
        joined
    }
}

/// `bake.export` on the page: the bundle the bridge turns into files.
pub(crate) fn export(world: &World, params: &Value) -> Result<Value, LiveError> {
    let dry_run = match params.get("dry_run") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            return Err(LiveError::new(
                "bad_request",
                "dry_run must be true or false",
            ));
        }
    };
    let include: Vec<String> = match params.get("include") {
        None | Some(Value::Null) => INCLUDE_ALL.iter().map(|s| s.to_string()).collect(),
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for v in a {
                match v.as_str() {
                    Some(s) if INCLUDE_ALL.contains(&s) => out.push(s.to_string()),
                    _ => {
                        return Err(LiveError::new(
                            "bad_request",
                            format!("include: {v} is not one of {}", INCLUDE_ALL.join(", ")),
                        ));
                    }
                }
            }
            out
        }
        Some(_) => return Err(LiveError::new("bad_request", "include must be a list")),
    };
    let has = |k: &str| include.iter().any(|i| i == k);
    let title = match params.get("title") {
        None | Some(Value::Null) => "Live session".to_string(),
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().replace('\n', " "),
        Some(_) => {
            return Err(LiveError::new(
                "bad_request",
                "title must be a non-empty string",
            ));
        }
    };

    let session = world.res::<LiveSession>();
    let Some(start) = session.start_state.as_ref() else {
        return Err(LiveError::new(
            "bad_request",
            "the live session has not started yet",
        ));
    };
    let now = FullState::capture(world);
    let t = touched(&session.history);
    let applied: Vec<u64> = session
        .history
        .iter()
        .filter(|t| t.status == TxStatus::Applied)
        .map(|t| t.tx)
        .collect();
    let head = session.head_tx();
    let from_tx = applied.first().copied().unwrap_or(head);
    let to_tx = applied.last().copied().unwrap_or(head);

    let mut notes = Vec::new();
    let mut out_params = Vec::new();
    let mut out_slots = Vec::new();
    let mut out_curves = Vec::new();
    // tier id -> fields to merge into assets/tiers/<id>.json
    let mut tier_fields: BTreeMap<String, Map<String, Value>> = BTreeMap::new();

    {
        let reg = world.res::<ParamRegistry>();
        for (id, txs) in &t.params {
            let (Some(old), Some(new)) = (
                start.params.iter().find(|(i, _)| i == id).map(|p| p.1),
                now.params.iter().find(|(i, _)| i == id).map(|p| p.1),
            ) else {
                continue;
            };
            if old == new {
                continue;
            }
            let Some(def) = reg.def(*id) else { continue };
            let target = def.bake.resolve(&def.owner);
            let Some(target_s) = target_str(target) else {
                continue;
            };
            let wanted = match target {
                BakeTarget::Look | BakeTarget::Kit => has("params"),
                BakeTarget::Policy => has("policy"),
                BakeTarget::Tier => has("tiers"),
                _ => false,
            };
            if !wanted {
                continue;
            }
            let value = param_json(&reg, *id, &new);
            let old_v = param_json(&reg, *id, &old);
            notes.push(format!(
                "**{}** {} → {} {}",
                def.name,
                show(&old_v),
                show(&value),
                note_tail(txs)
            ));
            if target == BakeTarget::Tier {
                // tier.<id>.<field>
                let mut parts = def.name.splitn(3, '.');
                if let (Some("tier"), Some(tier), Some(field)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    tier_fields
                        .entry(tier.to_string())
                        .or_default()
                        .insert(field.to_string(), value);
                }
                continue;
            }
            out_params.push(json!({ "name": def.name, "bake": target_s, "value": value }));
        }
    }

    if has("slots")
        && let Some(slots) = world.try_res::<ShaderSlots>()
    {
        for (id, txs) in &t.slots {
            let old = start.slots.iter().find(|(i, _)| i == id).map(|s| &s.1);
            let new = now.slots.iter().find(|(i, _)| i == id).map(|s| &s.1);
            let (Some(old), Some(new)) = (old, new) else {
                continue;
            };
            if old == new {
                continue;
            }
            let Some(def) = slots.get(*id) else { continue };
            let name = def.signature.name;
            notes.push(format!("**{name}** slot rewritten {}", note_tail(txs)));
            out_slots.push(json!({ "name": name, "target": "look", "lang": "wgsl",
                                       "source": new }));
        }
    }

    if let Some(f) = world.try_res::<Formulas>() {
        for (id, txs) in &t.formulas {
            let old = start.formulas.iter().find(|(i, _)| i == id).map(|s| &s.1);
            let new = now.formulas.iter().find(|(i, _)| i == id).map(|s| &s.1);
            let (Some(old), Some(new)) = (old, new) else {
                continue;
            };
            if old == new {
                continue;
            }
            let Some(def) = f.def(*id) else { continue };
            let Some(target) = owner_target(&def.owner) else {
                continue;
            };
            match new {
                FormulaJson::Expr(src) if has("slots") => {
                    notes.push(format!(
                        "**{}** slot rewritten {}",
                        def.name,
                        note_tail(txs)
                    ));
                    out_slots.push(json!({ "name": def.name, "target": target, "lang": "expr",
                                           "source": src }));
                }
                FormulaJson::Curve(c) if has("curves") => {
                    let what = match old {
                        FormulaJson::Curve(o) if o.points().len() == c.points().len() => {
                            let n = o
                                .points()
                                .iter()
                                .zip(c.points())
                                .filter(|(a, b)| a != b)
                                .count();
                            format!("{n} point{} moved", if n == 1 { "" } else { "s" })
                        }
                        _ => format!("now {} points", c.points().len()),
                    };
                    notes.push(format!("**{}** curve: {what} {}", def.name, note_tail(txs)));
                    out_curves.push(json!({ "name": def.name, "target": target,
                                            "points": points_json(new) }));
                }
                _ => {}
            }
        }
    }

    if has("tiers") {
        for (tier, txs) in &t.tiers {
            let old = start
                .tier_effects
                .iter()
                .find(|(i, _)| i == tier)
                .map(|s| &s.1);
            let new = now
                .tier_effects
                .iter()
                .find(|(i, _)| i == tier)
                .map(|s| &s.1);
            let (Some(old), Some(new)) = (old, new) else {
                continue;
            };
            if old == new {
                continue;
            }
            notes.push(format!(
                "**tier.{tier}.effects_on** {} → {} {}",
                json!(old),
                json!(new),
                note_tail(txs)
            ));
            tier_fields
                .entry(tier.clone())
                .or_default()
                .insert("effects_on".into(), json!(new));
        }
    }
    let tiers: Vec<Value> = {
        let known = world.res::<Tiers>();
        tier_fields
            .into_iter()
            .filter(|(id, _)| known.presets().iter().any(|p| &p.id == id))
            .map(|(id, set)| json!({ "id": id, "set": set }))
            .collect()
    };

    for tx in session
        .history
        .iter()
        .filter(|t| t.status == TxStatus::AutoReverted)
    {
        let why = tx.revert_reason.as_deref().unwrap_or("frame time");
        notes.push(format!("Reverted automatically: {} ({why}).", tx.summary));
    }

    let (game, look, kit) = world.try_res::<BuildInfo>().map_or_else(
        || (String::new(), None, None),
        |b| (b.game.clone(), b.look.clone(), b.kit.clone()),
    );
    let bundle = json!({
        "title": title,
        "dry_run": dry_run,
        "include": include,
        "from_tx": from_tx,
        "to_tx": to_tx,
        "game": game,
        "look_crate": look,
        "kit_crate": kit,
        "params": out_params,
        "slots": out_slots,
        "curves": out_curves,
        "tiers": tiers,
        "notes": notes,
    });
    Ok(json!({ "bundle": bundle }))
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
