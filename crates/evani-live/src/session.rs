//! The live session: inbox, versioned history, snapshots and replies.
//!
//! [`LiveSession`] is a world resource inserted by [`crate::LiveModule`].
//! Requests arrive with [`crate::submit`] and are processed in arrival order
//! by [`crate::tick`] at a frame boundary. Mutating commands become one
//! [`Tx`] each; history is linear with a head (undo moves it back, a new
//! transaction after an undo drops the redo tail).

use std::collections::VecDeque;

use evani_core::params::{ParamId, ParamValue};
use evani_core::slots::{RebuildHandle, SlotCompileResult, SlotId};
use serde_json::Value;

use crate::change::{Change, FullState};

/// Transactions kept per session (docs: at least 200).
pub const HISTORY_CAP: usize = 500;

/// Frames a WGSL rebuild may sit untaken before live assumes no renderer is
/// driving slots and completes it itself (structural check only).
pub const NO_RENDERER_FRAMES: u32 = 3;

/// A WGSL rebuild that takes longer than this fails with `timeout`.
pub const SLOT_TIMEOUT_MS: f64 = 8_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStatus {
    Applied,
    Undone,
    AutoReverted,
}

impl TxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TxStatus::Applied => "applied",
            TxStatus::Undone => "undone",
            TxStatus::AutoReverted => "auto_reverted",
        }
    }
}

/// One versioned transaction.
#[derive(Debug, Clone)]
pub struct Tx {
    pub tx: u64,
    pub cmd: String,
    pub note: String,
    /// Who sent it: `mcp | ctl | http | page` (stamped by the bridge).
    pub source: String,
    /// Wall-clock time, ISO 8601 UTC.
    pub at: String,
    /// First frame rendered with it.
    pub frame: u64,
    pub changes: Vec<Change>,
    pub summary: String,
    pub status: TxStatus,
    /// Set by auto-revert (`frame_time`, `device_lost`) or when an edit
    /// transaction holds an applied edit back on a conflict (`conflict`).
    pub revert_reason: Option<String>,
    /// The edit transaction and edit this change belongs to (`edit.stage`,
    /// docs/LIVE_API.md 6.31), if any.
    pub edit: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    /// History head when saved.
    pub tx: u64,
    pub at: String,
    pub note: String,
    pub state: FullState,
}

/// One WGSL rebuild a parked command waits for.
#[derive(Debug, Clone)]
pub struct PendingSlot {
    pub handle: RebuildHandle,
    /// Index of the `batch` part (0 outside a batch).
    pub part: usize,
    pub slot: SlotId,
    pub old: String,
    pub new: String,
    pub result: Option<SlotCompileResult>,
}

/// A mutating command waiting for slot compiles. The inbox is paused while
/// one is parked so commands still apply in arrival order.
#[derive(Debug, Clone)]
pub struct Parked {
    pub token: u64,
    pub id: Value,
    pub req: Value,
    pub plan: crate::commands::Plan,
    pub slots: Vec<PendingSlot>,
    pub frames: u32,
    pub started_ms: f64,
}

/// How the effect switches settle after a tier change (the effects module
/// resets `fx.*.enabled` from the new tier's preset on the next frame).
#[derive(Debug, Clone)]
pub enum AmendMode {
    /// `tier.set` (or a batch with one): keep the new tier's defaults, then
    /// re-apply the transaction's own effect switches on top.
    Defaults {
        explicit: Vec<(ParamId, ParamValue)>,
    },
    /// Undo/redo/restore: put these exact values back.
    Reassert(Vec<(ParamId, ParamValue)>),
}

/// Run on the tick after a tier change. With `tx`, the transaction's effect
/// switch changes are rewritten to `pre` → settled values so undo is exact.
#[derive(Debug, Clone)]
pub struct TierAmend {
    pub tx: Option<u64>,
    pub pre: Vec<(ParamId, ParamValue)>,
    pub mode: AmendMode,
}

/// World resource: the live session.
#[derive(Debug, Default)]
pub struct LiveSession {
    pub inbox: VecDeque<(u64, String)>,
    /// Oldest first. Entries `[0, head)` are applied, `[head, len)` undone.
    pub history: Vec<Tx>,
    pub head: usize,
    /// Last transaction number handed out.
    pub next_tx: u64,
    pub snapshots: Vec<Snapshot>,
    pub next_snapshot: u64,
    /// The live state when the session started (bake diffs against it).
    pub start_state: Option<FullState>,
    /// Finished replies (result envelopes) by submit token.
    pub replies: Vec<(u64, String)>,
    /// Outgoing event envelopes (`{"type":"event",...}`).
    pub events: Vec<String>,
    pub parked: Option<Parked>,
    /// Pending effect-switch settle after a tier change.
    pub tier_amend: Option<TierAmend>,
    /// Requests answered so far.
    pub handled: u64,
    /// Per-frame telemetry ring and subscriptions.
    pub tel: crate::telemetry::Telemetry,
    /// The auto-revert guard on the newest change.
    pub watch: Option<crate::revert::Watch>,
    /// Change results waiting for their telemetry window.
    pub deferred: Vec<crate::revert::Deferred>,
    /// The active `capture.frame` / `bench.run` / `compare.ab` run.
    pub run: Option<crate::capture::Run>,
    /// Last run number handed out.
    pub next_run: u64,
    /// Edit transactions (6.31), oldest first.
    pub edit_txns: Vec<crate::edits::EditTxn>,
    /// Last edit transaction number handed out.
    pub next_edit_txn: u64,
    /// A `together` commit waiting for its slot compiles (pauses the inbox).
    pub edit_commit: Option<crate::edits::PendingCommit>,
    /// The canvas hold of a `together` commit that settles over frames.
    pub edit_hold: Option<crate::edits::EditHold>,
    /// A `capture.frame { hold_after }` left the clock held where it shot:
    /// the camera and clock overrides from before it, put back when the
    /// next capture, bench, compare or sweep run ends (6.14).
    pub kept_hold: Option<(Option<evani_core::camera::CameraPose>, Option<f64>)>,
}

impl LiveSession {
    /// The tx number at the history head (0 = nothing applied).
    pub fn head_tx(&self) -> u64 {
        self.head
            .checked_sub(1)
            .and_then(|i| self.history.get(i))
            .map_or(0, |t| t.tx)
    }

    pub fn can_undo(&self) -> bool {
        self.head > 0
    }

    pub fn can_redo(&self) -> bool {
        self.head < self.history.len()
    }

    /// Record an applied transaction: drops the redo tail, caps the length.
    pub fn push(&mut self, tx: Tx) {
        self.history.truncate(self.head);
        self.history.push(tx);
        if self.history.len() > HISTORY_CAP {
            let drop = self.history.len() - HISTORY_CAP;
            self.history.drain(..drop);
        }
        self.head = self.history.len();
    }

    pub fn reply(&mut self, token: u64, text: String) {
        self.handled += 1;
        self.replies.push((token, text));
    }
}

/// Wall-clock now as ISO 8601 UTC (`2026-10-13T14:31:02Z`).
pub fn now_iso() -> String {
    let secs = now_unix_secs();
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

#[cfg(target_arch = "wasm32")]
fn now_unix_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

#[cfg(not(target_arch = "wasm32"))]
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_739), (2026, 10, 13));
    }
}
