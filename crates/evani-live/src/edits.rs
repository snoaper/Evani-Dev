//! Edit transactions (docs/LIVE_API.md 6.31): a compound live-edit request
//! ("turn the lights down, make the sky blue, reverse the spin") split by
//! the calling AI into independent atomic edits, each staged by its own
//! parallel agent into one transaction.
//!
//! * `edit.begin` opens a transaction (mode `stream` or `together`, default
//!   the `live.edit.mode` parameter) with an optional list of planned edits.
//! * `edit.stage` plans one edit (any mutating command) the moment it
//!   arrives. In `stream` mode it goes live at once as its own history
//!   entry; in `together` mode it waits for `edit.commit`, which applies
//!   every staged edit on one frame (holding the canvas while anything
//!   settles over frames: slot compiles, a tier switch).
//! * Two edits whose targets overlap (the same parameter, slot, curve, the
//!   tier, or the tier and an effect switch) are a `conflict`, found the
//!   moment the second is staged, whatever the arrival order: neither stays
//!   applied (a streamed one is held back as a new history entry) until
//!   `edit.resolve` says which wins. Nothing is resolved silently.
//! * A failed edit (unknown name, wrong type, out of range) is reported for
//!   that edit only and never blocks the others.
//!
//! Every applied edit is an ordinary history entry with its own note (undo,
//! auto-revert and `bake.export` work on it as on any change) tagged with
//! its transaction and edit id. Dev builds only, like the rest of evani-live.

use std::collections::BTreeSet;

use evani_core::formulas::Formulas;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::slots::{ShaderSlots, SlotRebuilds};
use serde_json::{Value, json};

use crate::change::{Change, apply_all};
use crate::commands::{self, Plan, frame_now};
use crate::session::{
    AmendMode, LiveSession, NO_RENDERER_FRAMES, PendingSlot, SLOT_TIMEOUT_MS, TierAmend, Tx,
    TxStatus, now_iso,
};
use crate::{LiveError, MUTATING};
use evani_core::ecs::World;

/// The edit-transaction commands.
pub const COMMANDS: &[&str] = &[
    "edit.begin",
    "edit.stage",
    "edit.commit",
    "edit.resolve",
    "edit.status",
    "edit.cancel",
];

/// The parameter holding the default apply mode (`stream` | `together`).
pub const MODE_PARAM: &str = "live.edit.mode";

/// Transactions kept per session (finished ones drop off first).
pub const MAX_TXNS: usize = 32;

/// Edits per transaction.
pub const MAX_EDITS: usize = 64;

/// What the AI is told to do on a conflict.
const ASK: &str = "These edits change the same thing. Ask the user which one wins, then call \
edit_resolve with keep (the winner) or drop (none of them).";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Each staged edit goes live at once.
    Stream,
    /// Staged edits go live together on commit.
    Together,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Stream => "stream",
            Mode::Together => "together",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    /// Taking edits.
    Open,
    /// Committed, but a conflict still waits for `edit.resolve`.
    Waiting,
    /// Committed and nothing left to decide.
    Closed,
    Cancelled,
}

impl TxnState {
    pub fn as_str(self) -> &'static str {
        match self {
            TxnState::Open => "open",
            TxnState::Waiting => "waiting",
            TxnState::Closed => "closed",
            TxnState::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditState {
    /// Planned in `edit.begin`, not staged yet.
    Pending,
    /// Checked, waiting for `edit.commit` (`together`).
    Staged,
    Applied,
    Failed,
    /// Overlaps another edit; waits for `edit.resolve`.
    Conflict,
    /// Lost a conflict (or dropped with it).
    Dropped,
    /// Never applied: the transaction was cancelled.
    Cancelled,
}

impl EditState {
    pub fn as_str(self) -> &'static str {
        match self {
            EditState::Pending => "pending",
            EditState::Staged => "staged",
            EditState::Applied => "applied",
            EditState::Failed => "failed",
            EditState::Conflict => "conflict",
            EditState::Dropped => "dropped",
            EditState::Cancelled => "cancelled",
        }
    }
}

/// One atomic edit of a transaction.
#[derive(Debug, Clone)]
pub struct Edit {
    pub id: String,
    /// What it is for, in the user's words (from the plan), if given.
    pub what: String,
    pub state: EditState,
    pub cmd: String,
    pub params: Value,
    pub note: String,
    pub source: String,
    /// Clamp out-of-range values instead of failing (`clamp: true`).
    pub clamp: bool,
    /// What it changes: `param:<name>`, `slot:<name>`, `formula:<name>`,
    /// `tier`, `tier_effects:<tier>`.
    pub targets: Vec<String>,
    /// Its history entry once applied.
    pub tx: Option<u64>,
    /// The entry that held it back on a conflict.
    pub held_back_tx: Option<u64>,
    /// `applied[]` of its change result (or the staged preview).
    pub applied: Value,
    pub error: Option<Value>,
    /// The edits it conflicts with.
    pub conflict_with: Vec<String>,
}

impl Edit {
    fn planned(id: String, what: String) -> Self {
        Self {
            id,
            what,
            state: EditState::Pending,
            cmd: String::new(),
            params: Value::Null,
            note: String::new(),
            source: String::new(),
            clamp: false,
            targets: Vec::new(),
            tx: None,
            held_back_tx: None,
            applied: Value::Null,
            error: None,
            conflict_with: Vec::new(),
        }
    }

    fn json(&self) -> Value {
        let mut e = json!({ "edit": self.id, "state": self.state.as_str() });
        if !self.what.is_empty() {
            e["what"] = json!(self.what);
        }
        if !self.cmd.is_empty() {
            e["cmd"] = json!(self.cmd);
            e["note"] = json!(self.note);
            e["targets"] = json!(self.targets.iter().map(|t| target_name(t)).collect::<Vec<_>>());
        }
        if let Some(tx) = self.tx {
            e["tx"] = json!(tx);
        }
        if let Some(tx) = self.held_back_tx {
            e["held_back_tx"] = json!(tx);
        }
        if !self.applied.is_null() {
            e["applied"] = self.applied.clone();
        }
        if let Some(err) = &self.error {
            e["error"] = err.clone();
        }
        if !self.conflict_with.is_empty() {
            e["conflict_with"] = json!(self.conflict_with);
        }
        e
    }
}

/// One edit transaction.
#[derive(Debug, Clone)]
pub struct EditTxn {
    pub id: String,
    pub mode: Mode,
    pub state: TxnState,
    /// The user's request, as the AI passed it.
    pub request: String,
    pub edits: Vec<Edit>,
    /// Last auto-assigned edit number.
    pub next_edit: u32,
    pub at: String,
}

impl EditTxn {
    fn find(&self, edit: &str) -> Option<usize> {
        self.edits.iter().position(|e| e.id == edit)
    }

    fn has_conflict(&self) -> bool {
        self.edits.iter().any(|e| e.state == EditState::Conflict)
    }

    /// The state after a commit: waiting while a conflict is open.
    fn settled_state(&self) -> TxnState {
        if self.has_conflict() {
            TxnState::Waiting
        } else {
            TxnState::Closed
        }
    }

    pub fn json(&self) -> Value {
        let mut counts = serde_json::Map::new();
        for e in &self.edits {
            let n = counts
                .get(e.state.as_str())
                .and_then(Value::as_u64)
                .unwrap_or(0);
            counts.insert(e.state.as_str().into(), json!(n + 1));
        }
        let mut v = json!({ "txn": self.id, "mode": self.mode.as_str(), "state": self.state.as_str(),
            "request": self.request, "at": self.at,
            "edits": self.edits.iter().map(Edit::json).collect::<Vec<_>>(), "counts": counts });
        if self.has_conflict() {
            v["ask"] = json!(ASK);
        }
        v
    }
}

/// A `together` commit waiting for its WGSL slot compiles.
#[derive(Debug, Clone)]
pub struct PendingCommit {
    pub token: u64,
    pub id: Value,
    pub req: Value,
    pub txn: String,
    /// Edits ready to apply, by id, with their plans.
    pub ready: Vec<(String, Plan)>,
    /// The compiles, by edit id.
    pub slots: Vec<(String, PendingSlot)>,
    pub frames: u32,
    pub started_ms: f64,
}

/// The canvas hold of a `together` commit: released on the first tick after
/// `frame` once nothing is settling (slot compiles, a tier switch).
#[derive(Debug, Clone)]
pub struct EditHold {
    pub txn: String,
    pub frame: u64,
}

// ---- hooks used by commands.rs ------------------------------------------------------

/// Undoing `steps` entries would undo an `edit.hold_back` whose conflict is still waiting for
/// the user: that would put the held-back edit live again while its transaction still reports
/// `conflict`, so it is refused until `edit_resolve` settles the conflict.
pub(crate) fn undo_blocked(world: &World, steps: usize) -> Option<LiveError> {
    let s = world.res::<LiveSession>();
    let from = s.head.saturating_sub(steps);
    s.history[from..s.head].iter().rev().find_map(|t| {
        if t.cmd != "edit.hold_back" {
            return None;
        }
        let (txn, edit) = t.edit.as_ref()?;
        let tr = s.edit_txns.iter().find(|x| &x.id == txn)?;
        let e = &tr.edits[tr.find(edit)?];
        (e.state == EditState::Conflict).then(|| {
            LiveError::new(
                "invalid_value",
                format!(
                    "history entry {} holds back edit {edit} of {txn} until the user decides the \
                     conflict; ask the user, then call edit_resolve (keep or drop) instead of undoing it",
                    t.tx
                ),
            )
        })
    })
}

/// `(txn, edit)` of a staged edit's command (its `req.edit`).
pub(crate) fn edit_of(req: &Value) -> Option<(String, String)> {
    let e = req.get("edit")?;
    Some((e.get("txn")?.as_str()?.to_string(), e.get("id")?.as_str()?.to_string()))
}

/// Record an applied edit and tag its change result.
pub(crate) fn on_committed(world: &World, req: &Value, tx: u64, result: &mut Value) {
    let Some((txn, edit)) = edit_of(req) else {
        return;
    };
    let mut s = world.res_mut::<LiveSession>();
    let Some(t) = s.edit_txns.iter_mut().find(|t| t.id == txn) else {
        return;
    };
    let mode = t.mode;
    if let Some(i) = t.find(&edit) {
        let e = &mut t.edits[i];
        e.state = EditState::Applied;
        e.tx = Some(tx);
        e.error = None;
        e.applied = result.get("applied").cloned().unwrap_or(Value::Null);
    }
    result["txn"] = json!(txn);
    result["edit"] = json!(edit);
    result["state"] = json!("applied");
    result["mode"] = json!(mode.as_str());
}

/// A failed command: for a staged edit, record the failure on that edit
/// and answer `ok` with `state: "failed"` (the transaction carries on);
/// otherwise the usual error reply.
pub(crate) fn fail_or_reply(world: &World, token: u64, id: &Value, req: &Value, e: LiveError) {
    let Some((txn, edit)) = edit_of(req) else {
        return commands::reply(world, token, id, Err(e));
    };
    let err = mark_failed(world, &txn, &edit, &e);
    commands::reply(
        world,
        token,
        id,
        Ok(json!({ "txn": txn, "edit": edit, "state": "failed", "error": err })),
    );
}

fn error_json(e: &LiveError) -> Value {
    let mut err = json!({ "code": e.code, "message": e.message });
    if let Some(d) = &e.data {
        err["data"] = d.clone();
    }
    err
}

fn mark_failed(world: &World, txn: &str, edit: &str, e: &LiveError) -> Value {
    let err = error_json(e);
    let mut s = world.res_mut::<LiveSession>();
    if let Some(t) = s.edit_txns.iter_mut().find(|t| t.id == txn)
        && let Some(i) = t.find(edit)
    {
        let x = &mut t.edits[i];
        x.state = EditState::Failed;
        x.error = Some(err.clone());
        x.targets.clear();
        x.tx = None;
    }
    err
}

/// Release a `together` commit's canvas hold once it has settled.
pub(crate) fn settle_hold(world: &World) {
    let Some(mut s) = world.try_res_mut::<LiveSession>() else {
        return;
    };
    let Some(h) = &s.edit_hold else { return };
    let frame = h.frame;
    drop(s);
    let (now, _) = frame_now(world);
    s = world.res_mut::<LiveSession>();
    if s.edit_commit.is_none() && s.tier_amend.is_none() && now > frame {
        s.edit_hold = None;
    }
}

// ---- commands -----------------------------------------------------------------------

/// Run one `edit.*` command.
pub(crate) fn command(world: &World, token: u64, id: &Value, req: &Value, cmd: &str, params: &Value) {
    let res = match cmd {
        "edit.begin" => begin(world, req, params),
        "edit.stage" => return stage(world, token, id, req, params),
        "edit.commit" => return commit(world, token, id, req, params),
        "edit.resolve" => return resolve(world, token, id, req, params),
        "edit.status" => status(world, params),
        "edit.cancel" => cancel(world, params),
        c => Err(LiveError::new("unknown_command", format!("no command {c}"))),
    };
    commands::reply(world, token, id, res);
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The index of transaction `params.txn`.
fn txn_index(world: &World, params: &Value) -> Result<usize, LiveError> {
    let Some(txn) = str_param(params, "txn") else {
        return Err(LiveError::new("bad_request", "params.txn must be a string"));
    };
    let s = world.res::<LiveSession>();
    s.edit_txns.iter().position(|t| t.id == txn).ok_or_else(|| {
        let open: Vec<&str> = s
            .edit_txns
            .iter()
            .filter(|t| t.state == TxnState::Open)
            .map(|t| t.id.as_str())
            .collect();
        LiveError::new("unknown_target", format!("No edit transaction {txn}"))
            .with(json!({ "did_you_mean": open }))
    })
}

fn default_mode(world: &World) -> Mode {
    let reg = world.res::<ParamRegistry>();
    match reg.id(MODE_PARAM).and_then(|i| reg.value(i)) {
        Some(ParamValue::Enum(1)) => Mode::Together,
        _ => Mode::Stream,
    }
}

fn begin(world: &World, req: &Value, params: &Value) -> Result<Value, LiveError> {
    let mode = match str_param(params, "mode") {
        None => default_mode(world),
        Some("stream") => Mode::Stream,
        Some("together") => Mode::Together,
        Some(m) => {
            return Err(LiveError::new(
                "invalid_value",
                format!("mode must be stream or together, got {m:?}"),
            ));
        }
    };
    let request = str_param(params, "request")
        .or_else(|| req.get("note").and_then(Value::as_str))
        .unwrap_or("")
        .chars()
        .take(500)
        .collect::<String>();
    let mut edits: Vec<Edit> = Vec::new();
    let mut next = 0u32;
    if let Some(list) = params.get("edits") {
        let list = list
            .as_array()
            .ok_or_else(|| LiveError::new("bad_request", "params.edits must be an array"))?;
        if list.len() > MAX_EDITS {
            return Err(LiveError::new(
                "limit",
                format!("at most {MAX_EDITS} edits per transaction"),
            ));
        }
        for (i, e) in list.iter().enumerate() {
            let (eid, what) = match e {
                Value::String(w) => (None, w.clone()),
                Value::Object(o) => (
                    o.get("id").and_then(Value::as_str).map(str::to_string),
                    o.get("what")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                ),
                _ => {
                    return Err(LiveError::new(
                        "bad_request",
                        format!("edits[{i}] must be a string or {{id, what}}"),
                    ));
                }
            };
            let eid = match eid {
                Some(x) if valid_id(&x) => x,
                Some(x) => {
                    return Err(LiveError::new(
                        "invalid_value",
                        format!("edit id {x:?}: 1-64 letters, digits, - _ ."),
                    ));
                }
                None => {
                    next += 1;
                    format!("e{next}")
                }
            };
            if edits.iter().any(|x| x.id == eid) {
                return Err(LiveError::new(
                    "invalid_value",
                    format!("edit id {eid} is listed twice"),
                ));
            }
            edits.push(Edit::planned(eid, what.chars().take(300).collect()));
        }
    }
    let mut s = world.res_mut::<LiveSession>();
    // Keep at most MAX_TXNS: only finished ones (closed, cancelled) are dropped; an open or
    // waiting one is never removed under its agents.
    while s.edit_txns.len() >= MAX_TXNS {
        let Some(i) = s
            .edit_txns
            .iter()
            .position(|t| matches!(t.state, TxnState::Closed | TxnState::Cancelled))
        else {
            return Err(LiveError::new(
                "limit",
                format!(
                    "{MAX_TXNS} edit transactions are still open or waiting; commit, resolve or \
                     cancel one (edit_status lists them) before starting another"
                ),
            ));
        };
        s.edit_txns.remove(i);
    }
    s.next_edit_txn += 1;
    let txn = EditTxn {
        id: format!("edit-{}", s.next_edit_txn),
        mode,
        state: TxnState::Open,
        request,
        edits,
        next_edit: next,
        at: now_iso(),
    };
    let mut v = txn.json();
    v["how"] = json!(match mode {
        Mode::Stream =>
            "Stage each edit with edit_stage (one agent per edit, all at once); each shows as soon as it is staged. Call edit_commit when every agent is done.",
        Mode::Together =>
            "Stage each edit with edit_stage (one agent per edit, all at once); nothing shows until edit_commit, which applies them all on one frame.",
    });
    s.edit_txns.push(txn);
    Ok(v)
}

/// Plan an edit; out-of-range values fail (`out_of_range`) unless `clamp`.
fn plan_edit(world: &World, cmd: &str, params: &Value, clamp: bool) -> Result<Plan, LiveError> {
    let p = commands::plan(world, cmd, params)?;
    if !clamp
        && let Some(a) = p
            .applied
            .iter()
            .find(|a| a.get("clamped") == Some(&Value::Bool(true)))
    {
        let name = a["name"].as_str().unwrap_or("?").to_string();
        let mut data = json!({ "name": name, "requested": a["requested"], "nearest": a["new"] });
        let reg = world.res::<ParamRegistry>();
        if let Some(d) = reg.id(&name).and_then(|i| reg.def(i)) {
            data["min"] = json!(d.min);
            data["max"] = json!(d.max);
        }
        return Err(LiveError::new(
            "out_of_range",
            format!(
                "{name}: {} is outside its range (nearest allowed {}); stage it again within range, or with clamp: true",
                a["requested"], a["new"]
            ),
        )
        .with(data));
    }
    Ok(p)
}

/// What a plan changes (see [`Edit::targets`]).
fn targets_of(world: &World, plan: &Plan) -> Vec<String> {
    let mut t = BTreeSet::new();
    for c in &plan.changes {
        match c {
            Change::Param { id, .. } => {
                if let Some(d) = world.res::<ParamRegistry>().def(*id) {
                    t.insert(format!("param:{}", d.name));
                }
            }
            Change::Slot { id, .. } => {
                if let Some(d) = world.res::<ShaderSlots>().get(*id) {
                    t.insert(format!("slot:{}", d.signature.name));
                }
            }
            Change::Formula { id, .. } => {
                if let Some(d) = world.res::<Formulas>().def(*id) {
                    t.insert(format!("formula:{}", d.name));
                }
            }
            Change::Tier { .. } => {
                t.insert("tier".to_string());
            }
            Change::TierEffects { tier, .. } => {
                t.insert(format!("tier_effects:{tier}"));
            }
        }
    }
    // Switching to the tier already active changes nothing, but still claims it.
    if plan.applied.iter().any(|a| a["kind"] == "tier") {
        t.insert("tier".to_string());
    }
    t.into_iter().collect()
}

/// The target without its kind (`param:fx.bloom.intensity` → the name).
fn target_name(t: &str) -> &str {
    t.split_once(':').map_or(t, |(_, n)| n)
}

/// Do two targets overlap? The same target, or the active tier against an
/// effect switch or a tier preset's effect set (a tier switch resets both).
pub fn overlap(a: &str, b: &str) -> bool {
    let tier_vs = |t: &str, o: &str| {
        t == "tier"
            && (o.starts_with("tier_effects:")
                || (o.starts_with("param:fx.") && o.ends_with(".enabled")))
    };
    a == b || tier_vs(a, b) || tier_vs(b, a)
}

/// Is history entry `tx` applied (not undone or reverted)?
fn tx_live(s: &LiveSession, tx: u64) -> bool {
    s.history[..s.head]
        .iter()
        .any(|t| t.tx == tx && t.status == TxStatus::Applied)
}

/// The command a staged edit runs as (its own note and source, tagged).
fn edit_req(req: &Value, txn: &str, e: &Edit) -> Value {
    let mut r = json!({ "type": "cmd", "v": 1, "id": req.get("id").cloned().unwrap_or(Value::Null),
        "cmd": e.cmd, "params": e.params, "note": e.note, "source": e.source,
        "edit": { "txn": txn, "id": e.id } });
    if let Some(o) = req.get("opts") {
        r["opts"] = o.clone();
    }
    r
}

fn stage(world: &World, token: u64, id: &Value, req: &Value, params: &Value) {
    match stage_inner(world, token, id, req, params) {
        Ok(()) => {}
        Err(e) => commands::reply(world, token, id, Err(e)),
    }
}

fn stage_inner(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    params: &Value,
) -> Result<(), LiveError> {
    let ti = txn_index(world, params)?;
    let note = req.get("note").and_then(Value::as_str).unwrap_or("");
    if note.trim().is_empty() || note.len() > 500 {
        return Err(LiveError::new(
            "note_required",
            "edit_stage needs a note of 1-500 chars saying why (it goes into history and CHANGES.md)",
        ));
    }
    let cmd = str_param(params, "cmd")
        .ok_or_else(|| LiveError::new("bad_request", "params.cmd must be a mutating command"))?;
    if !MUTATING.contains(&cmd) {
        return Err(LiveError::new(
            "invalid_value",
            format!("an edit is one mutating command ({}), got {cmd:?}", MUTATING.join(", ")),
        ));
    }
    let inner = params.get("params").cloned().unwrap_or_else(|| json!({}));
    if !inner.is_object() {
        return Err(LiveError::new("bad_request", "params.params must be an object"));
    }
    let clamp = params.get("clamp").and_then(Value::as_bool).unwrap_or(false);
    let source = req
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("page")
        .to_string();
    // Find or add the edit.
    let (txn_id, mode, ei) = {
        let mut s = world.res_mut::<LiveSession>();
        let t = &mut s.edit_txns[ti];
        if t.state != TxnState::Open {
            return Err(LiveError::new(
                "invalid_value",
                format!(
                    "edit transaction {} is {}; start a new one with edit_begin",
                    t.id,
                    t.state.as_str()
                ),
            ));
        }
        let eid = match str_param(params, "edit") {
            Some(x) if valid_id(x) => x.to_string(),
            Some(x) => {
                return Err(LiveError::new(
                    "invalid_value",
                    format!("edit id {x:?}: 1-64 letters, digits, - _ ."),
                ));
            }
            None => loop {
                t.next_edit += 1;
                let x = format!("e{}", t.next_edit);
                if t.find(&x).is_none() {
                    break x;
                }
            },
        };
        let ei = match t.find(&eid) {
            Some(i) => {
                let st = t.edits[i].state;
                let again = matches!(st, EditState::Pending | EditState::Failed)
                    || (st == EditState::Staged && t.mode == Mode::Together);
                if !again {
                    return Err(LiveError::new(
                        "invalid_value",
                        format!(
                            "edit {eid} is already {}; stage a new edit id (undo it with history_undo if needed)",
                            st.as_str()
                        ),
                    ));
                }
                i
            }
            None => {
                if t.edits.len() >= MAX_EDITS {
                    return Err(LiveError::new(
                        "limit",
                        format!("at most {MAX_EDITS} edits per transaction"),
                    ));
                }
                t.edits.push(Edit::planned(eid, String::new()));
                t.edits.len() - 1
            }
        };
        let e = &mut t.edits[ei];
        e.cmd = cmd.to_string();
        e.params = inner.clone();
        e.note = note.to_string();
        e.source = source;
        e.clamp = clamp;
        e.error = None;
        e.targets.clear();
        e.conflict_with.clear();
        (t.id.clone(), t.mode, ei)
    };
    let edit_id = world.res::<LiveSession>().edit_txns[ti].edits[ei].id.clone();
    let plan = match plan_edit(world, cmd, &inner, clamp) {
        Ok(p) => p,
        Err(e) => {
            let err = mark_failed(world, &txn_id, &edit_id, &e);
            commands::reply(
                world,
                token,
                id,
                Ok(json!({ "txn": txn_id, "edit": edit_id, "state": "failed", "error": err })),
            );
            return Ok(());
        }
    };
    let targets = targets_of(world, &plan);
    // Overlaps with the transaction's other live, staged or conflicting edits.
    let others: Vec<usize> = {
        let s = world.res::<LiveSession>();
        let t = &s.edit_txns[ti];
        t.edits
            .iter()
            .enumerate()
            .filter(|(i, o)| {
                *i != ei
                    && match o.state {
                        EditState::Staged | EditState::Conflict => true,
                        EditState::Applied => o.tx.is_some_and(|tx| tx_live(&s, tx)),
                        _ => false,
                    }
                    && o.targets.iter().any(|a| targets.iter().any(|b| overlap(a, b)))
            })
            .map(|(i, _)| i)
            .collect()
    };
    {
        let mut s = world.res_mut::<LiveSession>();
        let e = &mut s.edit_txns[ti].edits[ei];
        e.targets = targets.clone();
        e.applied = Value::Array(plan.applied.clone());
    }
    if !others.is_empty() {
        conflict(world, token, id, ti, ei, &others);
        return Ok(());
    }
    if mode == Mode::Together {
        {
            let mut s = world.res_mut::<LiveSession>();
            s.edit_txns[ti].edits[ei].state = EditState::Staged;
        }
        commands::reply(
            world,
            token,
            id,
            Ok(json!({ "txn": txn_id, "edit": edit_id, "state": "staged", "mode": mode.as_str(),
                       "applied": plan.applied, "targets": targets.iter().map(|t| target_name(t)).collect::<Vec<_>>() })),
        );
        return Ok(());
    }
    apply_now(world, token, id, req, ti, ei, plan);
    Ok(())
}

/// Apply edit `ei` now through the ordinary commit (or slot-compile) path.
fn apply_now(world: &World, token: u64, id: &Value, req: &Value, ti: usize, ei: usize, plan: Plan) {
    let r = {
        let s = world.res::<LiveSession>();
        let t = &s.edit_txns[ti];
        edit_req(req, &t.id, &t.edits[ei])
    };
    if plan.wgsl.is_empty() {
        commands::commit(world, token, id, &r, plan, None);
    } else {
        commands::park(world, token, id, &r, plan);
    }
}

/// Edit `ei` overlaps `others`: every one of them becomes a conflict, and
/// any that is live is held back, so none stays applied until resolved.
fn conflict(world: &World, token: u64, id: &Value, ti: usize, ei: usize, others: &[usize]) {
    let (txn_id, edit_id, names, live) = {
        let s = world.res::<LiveSession>();
        let t = &s.edit_txns[ti];
        let me = &t.edits[ei];
        let mut names = BTreeSet::new();
        for &o in others {
            for a in &t.edits[o].targets {
                for b in &me.targets {
                    if overlap(a, b) {
                        names.insert(target_name(a).to_string());
                        names.insert(target_name(b).to_string());
                    }
                }
            }
        }
        let live: Vec<(String, u64)> = others
            .iter()
            .filter_map(|&o| {
                let x = &t.edits[o];
                (x.state == EditState::Applied)
                    .then_some(x.tx)
                    .flatten()
                    .filter(|tx| tx_live(&s, *tx))
                    .map(|tx| (x.id.clone(), tx))
            })
            .collect();
        (t.id.clone(), me.id.clone(), names.into_iter().collect::<Vec<_>>(), live)
    };
    let with: Vec<String> = {
        let s = world.res::<LiveSession>();
        others.iter().map(|&o| s.edit_txns[ti].edits[o].id.clone()).collect()
    };
    let mut rolled = Vec::new();
    for (other, tx) in &live {
        if let Some(by) = hold_back(world, &txn_id, other, *tx, &edit_id, &names) {
            rolled.push(json!({ "edit": other, "tx": tx, "by_tx": by }));
            let mut s = world.res_mut::<LiveSession>();
            if let Some(i) = s.edit_txns[ti].find(other) {
                s.edit_txns[ti].edits[i].held_back_tx = Some(by);
            }
        }
    }
    {
        let mut s = world.res_mut::<LiveSession>();
        let t = &mut s.edit_txns[ti];
        for &o in others {
            let x = &mut t.edits[o];
            x.state = EditState::Conflict;
            if !x.conflict_with.contains(&edit_id) {
                x.conflict_with.push(edit_id.clone());
            }
        }
        let me = &mut t.edits[ei];
        me.state = EditState::Conflict;
        me.conflict_with = with.clone();
    }
    let mut all_edits = vec![edit_id.clone()];
    all_edits.extend(with.iter().cloned());
    let data = json!({ "txn": txn_id, "edits": all_edits, "targets": names, "rolled_back": rolled });
    let ev = json!({ "type": "event", "v": 1, "event": "edit.conflict", "data": data });
    world.res_mut::<LiveSession>().events.push(ev.to_string());
    commands::reply(
        world,
        token,
        id,
        Ok(json!({ "txn": txn_id, "edit": edit_id, "state": "conflict",
                   "conflict": { "with": with, "targets": names, "rolled_back": rolled, "ask": ASK } })),
    );
}

/// Undo applied edit `edit` (history entry `tx`) as a new history entry,
/// marking the original `auto_reverted` with reason `conflict` (as an
/// auto-revert does, so undo, history and bake treat it the same way).
fn hold_back(
    world: &World,
    txn: &str,
    edit: &str,
    tx: u64,
    by_edit: &str,
    names: &[String],
) -> Option<u64> {
    let orig = {
        let s = world.res::<LiveSession>();
        s.history[..s.head]
            .iter()
            .find(|t| t.tx == tx && t.status == TxStatus::Applied)
            .map(|t| t.changes.clone())?
    };
    let undo: Vec<Change> = orig.iter().rev().map(crate::revert::inverse).collect();
    apply_all(world, &undo).ok()?;
    if undo.iter().any(|c| matches!(c, Change::Tier { .. })) {
        let vals = commands::effect_switches(world);
        world.res_mut::<LiveSession>().tier_amend = Some(TierAmend {
            tx: None,
            pre: Vec::new(),
            mode: AmendMode::Reassert(vals),
        });
    }
    let (frame, _) = frame_now(world);
    let summary = match undo.as_slice() {
        [one] => one.summary(world),
        many => format!("{} changes", many.len()),
    };
    let by = {
        let mut s = world.res_mut::<LiveSession>();
        if let Some(t) = s.history.iter_mut().find(|t| t.tx == tx) {
            t.status = TxStatus::AutoReverted;
            t.revert_reason = Some("conflict".to_string());
        }
        if s.watch.is_some_and(|w| w.tx == tx) {
            s.watch = None;
        }
        s.next_tx += 1;
        let n = s.next_tx;
        s.push(Tx {
            tx: n,
            cmd: "edit.hold_back".to_string(),
            note: format!(
                "Held back {edit}: it and {by_edit} both change {}; the user decides which wins",
                names.join(", ")
            ),
            source: "page".to_string(),
            at: now_iso(),
            frame: frame + 1,
            changes: undo,
            summary,
            status: TxStatus::Applied,
            revert_reason: None,
            edit: Some((txn.to_string(), edit.to_string())),
        });
        n
    };
    crate::revert::push_mirror(world);
    Some(by)
}

fn commit(world: &World, token: u64, id: &Value, req: &Value, params: &Value) {
    let ti = match txn_index(world, params) {
        Ok(i) => i,
        Err(e) => return commands::reply(world, token, id, Err(e)),
    };
    let (state, mode, txn_id) = {
        let s = world.res::<LiveSession>();
        let t = &s.edit_txns[ti];
        (t.state, t.mode, t.id.clone())
    };
    match state {
        TxnState::Open => {}
        TxnState::Cancelled => {
            let e = LiveError::new(
                "invalid_value",
                format!("edit transaction {txn_id} was cancelled"),
            );
            return commands::reply(world, token, id, Err(e));
        }
        // Committed already: answer with where it stands.
        _ => {
            let v = world.res::<LiveSession>().edit_txns[ti].json();
            return commands::reply(world, token, id, Ok(v));
        }
    }
    if mode == Mode::Stream {
        let v = {
            let mut s = world.res_mut::<LiveSession>();
            let t = &mut s.edit_txns[ti];
            t.state = t.settled_state();
            t.json()
        };
        return commands::reply(world, token, id, Ok(v));
    }
    // Together: plan every staged edit again against the state now.
    let staged: Vec<(String, String, Value, bool)> = {
        let s = world.res::<LiveSession>();
        s.edit_txns[ti]
            .edits
            .iter()
            .filter(|e| e.state == EditState::Staged)
            .map(|e| (e.id.clone(), e.cmd.clone(), e.params.clone(), e.clamp))
            .collect()
    };
    let mut ready = Vec::new();
    for (eid, cmd, p, clamp) in staged {
        match plan_edit(world, &cmd, &p, clamp) {
            Ok(plan) => ready.push((eid, plan)),
            Err(e) => {
                mark_failed(world, &txn_id, &eid, &e);
            }
        }
    }
    if ready.iter().any(|(_, p)| !p.wgsl.is_empty()) {
        return start_pending(world, token, id, req, txn_id, ready);
    }
    let ready = ready.into_iter().map(|(e, p)| (e, p, None)).collect();
    finish_commit(world, token, id, req, &txn_id, ready);
}

/// Request the commit's slot compiles and hold the canvas until they are
/// done and everything is applied.
fn start_pending(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    txn: String,
    ready: Vec<(String, Plan)>,
) {
    let (_, now) = frame_now(world);
    let mut slots = Vec::new();
    let mut ok = Vec::new();
    for (eid, plan) in ready {
        let mut mine = Vec::new();
        let mut failed = None;
        {
            let ss = world.res::<ShaderSlots>();
            let mut rb = world.res_mut::<SlotRebuilds>();
            for w in &plan.wgsl {
                match rb.request(&ss, &w.name, &w.new, now) {
                    Ok(handle) => mine.push((
                        eid.clone(),
                        PendingSlot {
                            handle,
                            part: w.part,
                            slot: w.slot,
                            old: w.old.clone(),
                            new: w.new.clone(),
                            result: None,
                        },
                    )),
                    Err(e) => {
                        failed = Some(e.to_string());
                        break;
                    }
                }
            }
        }
        match failed {
            Some(msg) => {
                let m = json!({ "severity": "error", "line": 0, "col": 0, "len": 0,
                                "message": msg, "source_line": null });
                let e = commands::compile_error(&msg, vec![m], 0.0);
                mark_failed(world, &txn, &eid, &e);
            }
            None => {
                slots.extend(mine);
                ok.push((eid, plan));
            }
        }
    }
    let mut s = world.res_mut::<LiveSession>();
    s.edit_hold = Some(EditHold {
        txn: txn.clone(),
        frame: u64::MAX,
    });
    s.edit_commit = Some(PendingCommit {
        token,
        id: id.clone(),
        req: req.clone(),
        txn,
        ready: ok,
        slots,
        frames: 0,
        started_ms: now,
    });
    drop(s);
    // Hold from the very next drawn frame.
    crate::capture::sync_hold(world);
}

/// Poll a `together` commit waiting for compiles. Returns true when there
/// is none (or it just finished), so the inbox can go on.
pub(crate) fn poll_commit(world: &World) -> bool {
    let Some(mut pc) = world.res_mut::<LiveSession>().edit_commit.take() else {
        return true;
    };
    pc.frames += 1;
    let (_, now) = frame_now(world);
    let mut self_completed = false;
    {
        let mut rb = world.res_mut::<SlotRebuilds>();
        if pc.frames >= NO_RENDERER_FRAMES && rb.has_pending() {
            // No renderer drives slots here: structural check only.
            for r in rb.take_pending() {
                rb.complete(
                    r.handle,
                    commands::structural_ok((now - r.requested_ms).max(0.0)),
                );
            }
            self_completed = true;
        }
        for (_, s) in pc.slots.iter_mut().filter(|(_, s)| s.result.is_none()) {
            s.result = rb.take_result(s.handle);
        }
    }
    let waiting = pc.slots.iter().any(|(_, s)| s.result.is_none());
    if waiting && now - pc.started_ms <= SLOT_TIMEOUT_MS {
        world.res_mut::<LiveSession>().edit_commit = Some(pc);
        return false;
    }
    // Each edit: applied if all its compiles passed, else failed alone.
    let mut ready = Vec::new();
    for (eid, plan) in core::mem::take(&mut pc.ready) {
        let mine: Vec<&PendingSlot> = pc
            .slots
            .iter()
            .filter(|(e, _)| *e == eid)
            .map(|(_, s)| s)
            .collect();
        let mut messages = Vec::new();
        let mut rebuilt = Vec::new();
        let mut ms: f64 = 0.0;
        let mut err: Option<LiveError> = None;
        for s in &mine {
            match &s.result {
                None => {
                    err = Some(
                        LiveError::new("timeout", "the slot compile did not finish within 8 s")
                            .with(json!({ "kept": "previous" })),
                    );
                }
                Some(r) => {
                    ms = ms.max(r.ms);
                    messages.extend(r.messages.iter().map(commands::message_json));
                    rebuilt.extend(r.pipelines_rebuilt.iter().cloned());
                    if !r.ok && err.is_none() {
                        let first = r
                            .messages
                            .iter()
                            .find(|m| m.severity == "error")
                            .map_or("slot did not compile".to_string(), |m| m.text.clone());
                        err = Some(commands::compile_error(&first, Vec::new(), r.ms));
                    }
                }
            }
        }
        match err {
            Some(mut e) => {
                if e.code == "compile_error" {
                    e = commands::compile_error(&e.message, messages, ms);
                }
                let owned: Vec<PendingSlot> = mine.into_iter().cloned().collect();
                commands::restore_bodies(world, &owned);
                mark_failed(world, &pc.txn, &eid, &e);
            }
            None => {
                let mut compile =
                    json!({ "ok": true, "ms": ms, "messages": messages, "pipelines_rebuilt": rebuilt });
                if self_completed {
                    compile["checked"] = json!("structural");
                }
                let compile = (!mine.is_empty()).then_some(compile);
                ready.push((eid, plan, compile));
            }
        }
    }
    finish_commit(world, pc.token, &pc.id, &pc.req, &pc.txn, ready);
    true
}

/// Apply every ready edit of a `together` commit in this one tick (each its
/// own history entry), then send one change result covering all of them.
fn finish_commit(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    txn: &str,
    ready: Vec<(String, Plan, Option<Value>)>,
) {
    let tier = ready
        .iter()
        .any(|(_, p, _)| p.changes.iter().any(|c| matches!(c, Change::Tier { .. })));
    let mut applied_all = Vec::new();
    let mut txs = Vec::new();
    let mut last: Option<Value> = None;
    for (eid, plan, compile) in ready {
        let r = {
            let s = world.res::<LiveSession>();
            let Some(t) = s.edit_txns.iter().find(|t| t.id == txn) else {
                continue;
            };
            let Some(i) = t.find(&eid) else { continue };
            edit_req(req, txn, &t.edits[i])
        };
        match commands::commit_tx(world, &r, plan, compile) {
            Ok(res) => {
                if let Some(list) = res.get("applied").and_then(Value::as_array) {
                    for a in list {
                        let mut a = a.clone();
                        a["edit"] = json!(eid);
                        applied_all.push(a);
                    }
                }
                txs.push(res["tx"].clone());
                last = Some(res);
            }
            Err(e) => {
                mark_failed(world, txn, &eid, &e);
            }
        }
    }
    let (frame, _) = frame_now(world);
    let summary = {
        let mut s = world.res_mut::<LiveSession>();
        if tier || s.edit_hold.is_some() {
            s.edit_hold = Some(EditHold {
                txn: txn.to_string(),
                frame,
            });
        }
        match s.edit_txns.iter_mut().find(|t| t.id == txn) {
            Some(t) => {
                t.state = t.settled_state();
                t.json()
            }
            None => json!({ "txn": txn }),
        }
    };
    crate::capture::sync_hold(world);
    match last {
        Some(mut res) => {
            if let Some(o) = res.as_object_mut() {
                o.remove("edit");
            }
            for k in ["txn", "mode", "state", "edits", "counts", "request", "ask"] {
                if let Some(v) = summary.get(k) {
                    res[k] = v.clone();
                }
            }
            res["applied"] = Value::Array(applied_all);
            res["txs"] = Value::Array(txs);
            commands::finish_mutation(world, token, id, req, res, true);
        }
        None => commands::reply(world, token, id, Ok(summary)),
    }
}

/// The conflict group of edit `ei`: every conflicting edit linked to it.
fn group(t: &EditTxn, ei: usize) -> Vec<usize> {
    let mut seen = vec![ei];
    let mut i = 0;
    while i < seen.len() {
        let e = &t.edits[seen[i]];
        for other in &e.conflict_with {
            if let Some(j) = t.find(other)
                && t.edits[j].state == EditState::Conflict
                && !seen.contains(&j)
            {
                seen.push(j);
            }
        }
        // Links are kept both ways, but follow back-links too.
        for (j, x) in t.edits.iter().enumerate() {
            if x.state == EditState::Conflict && x.conflict_with.contains(&e.id) && !seen.contains(&j)
            {
                seen.push(j);
            }
        }
        i += 1;
    }
    seen
}

fn resolve(world: &World, token: u64, id: &Value, req: &Value, params: &Value) {
    if let Err(e) = resolve_inner(world, token, id, req, params) {
        commands::reply(world, token, id, Err(e));
    }
}

fn resolve_inner(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    params: &Value,
) -> Result<(), LiveError> {
    let ti = txn_index(world, params)?;
    let (keep, which) = match (str_param(params, "keep"), str_param(params, "drop")) {
        (Some(k), None) => (true, k.to_string()),
        (None, Some(d)) => (false, d.to_string()),
        _ => {
            return Err(LiveError::new(
                "bad_request",
                "give keep (the edit that wins) or drop (an edit whose whole conflict is dropped)",
            ));
        }
    };
    let (winner, plan_input) = {
        let mut s = world.res_mut::<LiveSession>();
        let t = &mut s.edit_txns[ti];
        if t.state == TxnState::Cancelled || t.state == TxnState::Closed {
            return Err(LiveError::new(
                "invalid_value",
                format!("edit transaction {} is {}", t.id, t.state.as_str()),
            ));
        }
        let Some(ei) = t.find(&which) else {
            let ids: Vec<String> = t.edits.iter().map(|e| e.id.clone()).collect();
            return Err(
                LiveError::new("unknown_target", format!("No edit {which} in {}", t.id))
                    .with(json!({ "did_you_mean": ids })),
            );
        };
        if t.edits[ei].state != EditState::Conflict {
            return Err(LiveError::new(
                "invalid_value",
                format!("edit {which} is {}, not in a conflict", t.edits[ei].state.as_str()),
            ));
        }
        let g = group(t, ei);
        let losers: Vec<String> = g
            .iter()
            .filter(|&&j| !keep || j != ei)
            .map(|&j| t.edits[j].id.clone())
            .collect();
        for &j in &g {
            if !keep || j != ei {
                t.edits[j].state = EditState::Dropped;
            }
        }
        if !keep {
            if t.state == TxnState::Waiting {
                t.state = t.settled_state();
            }
            (None, None)
        } else {
            let e = &mut t.edits[ei];
            e.conflict_with.clear();
            if !losers.is_empty() {
                e.note = format!("{} (the user chose this over {})", e.note, losers.join(", "));
                e.note.truncate(500);
            }
            let input = (e.cmd.clone(), e.params.clone(), e.clamp);
            if t.state == TxnState::Open && t.mode == Mode::Together {
                e.state = EditState::Staged;
                (Some(ei), None)
            } else {
                // Streaming, or committed already: it goes live now (no longer in a
                // conflict, so the transaction can settle).
                e.state = EditState::Staged;
                if t.state == TxnState::Waiting {
                    t.state = t.settled_state();
                }
                (Some(ei), Some(input))
            }
        }
    };
    let Some(ei) = winner else {
        let v = world.res::<LiveSession>().edit_txns[ti].json();
        commands::reply(world, token, id, Ok(v));
        return Ok(());
    };
    let Some((cmd, p, clamp)) = plan_input else {
        let v = world.res::<LiveSession>().edit_txns[ti].json();
        commands::reply(world, token, id, Ok(v));
        return Ok(());
    };
    let (txn_id, edit_id) = {
        let s = world.res::<LiveSession>();
        let t = &s.edit_txns[ti];
        (t.id.clone(), t.edits[ei].id.clone())
    };
    match plan_edit(world, &cmd, &p, clamp) {
        Ok(plan) => {
            {
                let targets = targets_of(world, &plan);
                let mut s = world.res_mut::<LiveSession>();
                s.edit_txns[ti].edits[ei].targets = targets;
            }
            apply_now(world, token, id, req, ti, ei, plan);
        }
        Err(e) => {
            let err = mark_failed(world, &txn_id, &edit_id, &e);
            commands::reply(
                world,
                token,
                id,
                Ok(json!({ "txn": txn_id, "edit": edit_id, "state": "failed", "error": err })),
            );
        }
    }
    Ok(())
}

fn status(world: &World, params: &Value) -> Result<Value, LiveError> {
    if params.get("txn").is_some() {
        let ti = txn_index(world, params)?;
        return Ok(world.res::<LiveSession>().edit_txns[ti].json());
    }
    let s = world.res::<LiveSession>();
    let list: Vec<Value> = s
        .edit_txns
        .iter()
        .rev()
        .map(|t| {
            let mut v = t.json();
            if let Some(o) = v.as_object_mut() {
                o.remove("edits");
            }
            v
        })
        .collect();
    Ok(json!({ "default_mode": default_mode(world).as_str(), "txns": list }))
}

fn cancel(world: &World, params: &Value) -> Result<Value, LiveError> {
    let ti = txn_index(world, params)?;
    let mut s = world.res_mut::<LiveSession>();
    let t = &mut s.edit_txns[ti];
    if t.state != TxnState::Open && t.state != TxnState::Waiting {
        return Err(LiveError::new(
            "invalid_value",
            format!("edit transaction {} is {}", t.id, t.state.as_str()),
        ));
    }
    for e in &mut t.edits {
        if matches!(
            e.state,
            EditState::Pending | EditState::Staged | EditState::Conflict
        ) {
            e.state = EditState::Cancelled;
        }
    }
    t.state = TxnState::Cancelled;
    let mut v = t.json();
    v["note"] = json!(
        "Staged and conflicting edits were dropped; applied edits stay (undo them with history_undo)."
    );
    Ok(v)
}
