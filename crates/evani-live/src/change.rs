//! One reversible edit of live state ([`Change`]) and the full live state
//! ([`FullState`]) that snapshots and bake read.
//!
//! Every mutating command is planned into a list of changes first (nothing
//! touched), then applied in order. Undo applies the same list backwards
//! with `forward = false`.

use evani_core::assets::content_hash;
use evani_core::ecs::World;
use evani_core::formulas::{FormulaBody, FormulaId, FormulaJson, Formulas};
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::quality::QualityState;
use evani_core::slots::{ShaderSlots, SlotId, SlotRebuilds};
use evani_core::tiers::Tiers;
use serde_json::{Value, json};

use crate::LiveError;

/// One reversible edit.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Param {
        id: ParamId,
        old: ParamValue,
        new: ParamValue,
    },
    /// A WGSL shader-slot body.
    Slot {
        id: SlotId,
        old: String,
        new: String,
    },
    /// A CPU formula: an expression slot or a curve.
    Formula {
        id: FormulaId,
        old: FormulaJson,
        new: FormulaJson,
    },
    /// The active tier.
    Tier { old: String, new: String },
    /// A tier preset's starting effect set (`effect.toggle` scope `tier`).
    TierEffects {
        tier: String,
        old: Vec<String>,
        new: Vec<String>,
    },
}

/// Hash shown for slot and formula bodies (`hash`, `old_hash`, `new_hash`).
pub fn body_hash(text: &str) -> String {
    format!("{:016x}", content_hash(text.as_bytes()))
}

pub fn formula_text(f: &FormulaJson) -> String {
    match f {
        FormulaJson::Expr(s) => s.clone(),
        FormulaJson::Curve(c) => serde_json::to_string(c.points()).unwrap_or_default(),
    }
}

pub fn curve_points_json(f: &FormulaJson) -> Value {
    match f {
        FormulaJson::Curve(c) => json!(c.points()),
        FormulaJson::Expr(s) => json!(s),
    }
}

/// Is this formula a curve (by its default body)?
pub fn is_curve(f: &Formulas, id: FormulaId) -> bool {
    matches!(f.default_body(id), Some(FormulaBody::Curve(_)))
}

impl Change {
    /// Apply this change (`forward`) or revert it (`!forward`).
    pub fn apply(&self, world: &World, forward: bool) -> Result<(), LiveError> {
        // Adaptive quality does not judge the frames around a live change
        // (it may rebuild pipelines or resize targets).
        if let Some(mut q) = world.try_res_mut::<QualityState>() {
            q.disturb();
        }
        match self {
            Change::Param { id, old, new } => {
                let v = if forward { new } else { old };
                let mut reg = world.res_mut::<ParamRegistry>();
                reg.set(*id, *v)
                    .map(|_| ())
                    .map_err(|e| LiveError::new("internal", format!("param apply: {e}")))
            }
            Change::Slot { id, old, new } => {
                let v = if forward { new } else { old };
                let mut slots = world.res_mut::<ShaderSlots>();
                if slots.get(*id).is_some_and(|d| d.body == *v) {
                    return Ok(());
                }
                slots
                    .set_body(*id, v)
                    .map_err(|e| LiveError::new("internal", format!("slot apply: {e}")))?;
                // The GPU pipelines must follow the body (undo, redo,
                // auto-revert, restore): queue a rebuild nobody waits for.
                let name = slots.get(*id).map(|d| d.signature.name);
                if let (Some(name), Some(mut rb)) = (name, world.try_res_mut::<SlotRebuilds>()) {
                    let _ = rb.request_detached(&slots, name, v, evani_core::time::now_ms());
                }
                Ok(())
            }
            Change::Formula { id, old, new } => {
                let v = if forward { new } else { old };
                let mut f = world.res_mut::<Formulas>();
                if f.to_json_body(*id).as_ref() == Some(v) {
                    return Ok(());
                }
                f.set(*id, v.clone())
                    .map(|_| ())
                    .map_err(|e| LiveError::new("internal", format!("formula apply: {e}")))
            }
            Change::Tier { old, new } => {
                let v = if forward { new } else { old };
                if world.res_mut::<Tiers>().set_active(v) {
                    Ok(())
                } else {
                    Err(LiveError::new("internal", format!("tier {v} vanished")))
                }
            }
            Change::TierEffects { tier, old, new } => {
                let v = if forward { new } else { old };
                let mut tiers = world.res_mut::<Tiers>();
                let Some(mut p) = tiers.get(tier).cloned() else {
                    return Err(LiveError::new("internal", format!("tier {tier} vanished")));
                };
                p.effects_on = v.clone();
                tiers.upsert(p);
                Ok(())
            }
        }
    }

    /// Generic `applied[]` entry (undo, redo, restore).
    pub fn describe(&self, world: &World, forward: bool) -> Value {
        match self {
            Change::Param { id, old, new } => {
                let reg = world.res::<ParamRegistry>();
                let (o, n) = pick(forward, old, new);
                match reg.def(*id) {
                    Some(d) => json!({ "kind": param_kind(&d.name), "name": d.name,
                        "old": d.value_to_json(o), "new": d.value_to_json(n) }),
                    None => json!({ "kind": "param", "name": null }),
                }
            }
            Change::Slot { id, old, new } => {
                let slots = world.res::<ShaderSlots>();
                let name = slots.get(*id).map(|d| d.signature.name);
                let (o, n) = pick(forward, old, new);
                json!({ "kind": "slot", "name": name, "old_hash": body_hash(o), "new_hash": body_hash(n) })
            }
            Change::Formula { id, old, new } => {
                let f = world.res::<Formulas>();
                let name = f.def(*id).map(|d| d.name.clone());
                let (o, n) = pick(forward, old, new);
                if is_curve(&f, *id) {
                    json!({ "kind": "curve", "name": name, "old": curve_points_json(o), "new": curve_points_json(n) })
                } else {
                    json!({ "kind": "slot", "name": name,
                        "old_hash": body_hash(&formula_text(o)), "new_hash": body_hash(&formula_text(n)) })
                }
            }
            Change::Tier { old, new } => {
                let (o, n) = pick(forward, old, new);
                json!({ "kind": "tier", "name": "tier", "old": o, "new": n })
            }
            Change::TierEffects { tier, old, new } => {
                let (o, n) = pick(forward, old, new);
                json!({ "kind": "effect", "name": format!("tier.{tier}.effects_on"), "scope": "tier", "old": o, "new": n })
            }
        }
    }

    /// One-line summary for `history.list`.
    pub fn summary(&self, world: &World) -> String {
        let d = self.describe(world, true);
        let name = d["name"].as_str().unwrap_or("?").to_string();
        match self {
            Change::Slot { .. } => format!("{name} slot rewritten"),
            Change::Formula { .. } if d["kind"] == "slot" => format!("{name} slot rewritten"),
            Change::Formula { .. } => format!("{name} curve reshaped"),
            _ => format!("{name} {} → {}", d["old"], d["new"]),
        }
    }
}

/// `applied[].kind` for a parameter: policy params report `policy`.
pub fn param_kind(name: &str) -> &'static str {
    if name.starts_with("aq.") || name.ends_with(".value_weight") {
        "policy"
    } else if name.starts_with("fx.") && name.ends_with(".enabled") {
        "effect"
    } else {
        "param"
    }
}

/// Undo a list of already-applied changes (newest first).
pub fn revert_all(world: &World, changes: &[Change]) {
    for c in changes.iter().rev() {
        let _ = c.apply(world, false);
    }
}

/// Apply changes in order; on failure, revert the ones already applied.
pub fn apply_all(world: &World, changes: &[Change]) -> Result<(), LiveError> {
    for (i, c) in changes.iter().enumerate() {
        if let Err(e) = c.apply(world, true) {
            revert_all(world, &changes[..i]);
            return Err(e);
        }
    }
    Ok(())
}

/// The whole live state: what a snapshot holds and what bake reads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FullState {
    pub params: Vec<(ParamId, ParamValue)>,
    /// WGSL slot bodies.
    pub slots: Vec<(SlotId, String)>,
    /// Curves and expression formulas.
    pub formulas: Vec<(FormulaId, FormulaJson)>,
    pub tier: String,
    /// Every tier preset's starting effect set.
    pub tier_effects: Vec<(String, Vec<String>)>,
}

impl FullState {
    pub fn capture(world: &World) -> Self {
        let params = {
            let reg = world.res::<ParamRegistry>();
            reg.defs()
                .iter()
                .filter_map(|d| {
                    let id = reg.id(&d.name)?;
                    Some((id, reg.value(id)?))
                })
                .collect()
        };
        let slots = world.try_res::<ShaderSlots>().map_or_else(Vec::new, |s| {
            s.iter().map(|(id, d)| (id, d.body.clone())).collect()
        });
        let formulas = world.try_res::<Formulas>().map_or_else(Vec::new, |f| {
            f.ids()
                .filter_map(|id| Some((id, f.to_json_body(id)?)))
                .collect()
        });
        let tiers = world.res::<Tiers>();
        Self {
            params,
            slots,
            formulas,
            tier: tiers.active().id.clone(),
            tier_effects: tiers
                .presets()
                .iter()
                .map(|p| (p.id.clone(), p.effects_on.clone()))
                .collect(),
        }
    }

    /// The changes that turn `self` (the current state) into `target`.
    pub fn diff(&self, target: &FullState) -> Vec<Change> {
        let mut out = Vec::new();
        for (tier, want) in &target.tier_effects {
            if let Some((_, have)) = self.tier_effects.iter().find(|(t, _)| t == tier)
                && have != want
            {
                out.push(Change::TierEffects {
                    tier: tier.clone(),
                    old: have.clone(),
                    new: want.clone(),
                });
            }
        }
        if self.tier != target.tier {
            out.push(Change::Tier {
                old: self.tier.clone(),
                new: target.tier.clone(),
            });
        }
        for (id, want) in &target.params {
            if let Some((_, have)) = self.params.iter().find(|(i, _)| i == id)
                && have != want
            {
                out.push(Change::Param {
                    id: *id,
                    old: *have,
                    new: *want,
                });
            }
        }
        for (id, want) in &target.slots {
            if let Some((_, have)) = self.slots.iter().find(|(i, _)| i == id)
                && have != want
            {
                out.push(Change::Slot {
                    id: *id,
                    old: have.clone(),
                    new: want.clone(),
                });
            }
        }
        for (id, want) in &target.formulas {
            if let Some((_, have)) = self.formulas.iter().find(|(i, _)| i == id)
                && have != want
            {
                out.push(Change::Formula {
                    id: *id,
                    old: have.clone(),
                    new: want.clone(),
                });
            }
        }
        out
    }
}

fn pick<T>(forward: bool, old: T, new: T) -> (T, T) {
    if forward { (old, new) } else { (new, old) }
}
