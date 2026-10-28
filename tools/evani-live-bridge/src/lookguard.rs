//! The look guard on the live server (docs/LIVE_API.md 15).
//!
//! * `look.status`, `look.check`, `look.limits.propose` and
//!   `look.baseline.propose` are answered here, not by the page. The AI may
//!   read, check and *propose*; `look.approve*` is refused (`forbidden`).
//! * Every real (non-dry-run) `bake.export` passes [`LookGuard::bake`]: the
//!   look's guard set must match Chris's signed approval record, and the
//!   look's captures must be inside its limits, or carry Chris's one-time
//!   approval for exactly this bake. Only then are files written.
//! * Approvals arrive on their own listener (`--approve-port`, see
//!   `server::approve_router`), checked by [`LookGuard::approve`]: an HMAC
//!   with the secret Evani.App handed over on the first stdin line, plus an
//!   Ed25519 signature by the key pinned in Evani.App's data folder.
//!
//! Every proposal, approval, refusal, check and bake goes to the audit log
//! (`.evani/look-guard/audit.jsonl`).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use evani_lookguard::approvals::{self, ApproveBody, Approvals, Keys, Kind, Pending, Proposal, Refusal};
use evani_lookguard::limits::{self, Breach, Limits, Moments};
use evani_lookguard::shots::{self, Shot};
use evani_lookguard::verify::{self, Failure, Rollback, Trust, Verdict, Verified};
use evani_lookguard::{audit, guardset, record, store};
use serde_json::{Value, json};

use crate::bake;
use crate::hub::Hub;
use crate::proto;

/// Why approvals are off until Evani.App starts the engine.
pub const OFF_REASON: &str = "Restart the engine from Evani.App to approve";
/// The first stdin line is read up to this many bytes.
pub const MAX_STDIN_LINE: u64 = 4096;
/// Largest approve body.
pub const MAX_APPROVE_BODY: usize = 16 * 1024;

/// A failed look command: code, plain sentence, optional data.
#[derive(Debug, Clone)]
pub struct Fail {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

fn fail(code: &str, message: impl Into<String>) -> Fail {
    Fail { code: code.into(), message: message.into(), data: None }
}

struct State {
    book: Approvals,
    /// Evani.App's look-approvals folder (`None`: no home for it).
    store_dir: Option<PathBuf>,
    /// From the stdin line.
    project_id: Option<String>,
    /// From the stdin line; used only when `endpoint.json` names none.
    seq_port: Option<u16>,
    approve_port: Option<u16>,
    stdin_seen: bool,
}

/// The look guard's state on one live server.
pub struct LookGuard {
    root: PathBuf,
    session: String,
    game: String,
    default_look: Option<String>,
    st: Mutex<State>,
    /// One capture sequence at a time: each shot changes and restores the
    /// page's state, so two interleaved sequences would see each other.
    capturing: tokio::sync::Mutex<()>,
}

/// A look's guard set as read from disk.
struct GuardSet {
    files: Vec<(String, Vec<u8>)>,
    hash: String,
    moments: Moments,
    limits: Limits,
}

/// What a bake that may go ahead carries into its manifest.
struct Pass {
    v: Verified,
    check: &'static str,
    approval_id: Option<String>,
    bake_hash: Option<String>,
    captures_hash: Option<String>,
    bundle_hash: String,
    key_warning: Option<String>,
}

/// `look = "basic"` in the game's `evani.toml`, as the crate name.
pub fn look_from_manifest(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix("look") else { continue };
        let Some(v) = rest.trim_start().strip_prefix('=') else { continue };
        let v = v.trim().trim_matches('"');
        let id = if v.starts_with("evani-look-") { v.to_string() } else { format!("evani-look-{v}") };
        return guardset::is_look_id(&id).then_some(id);
    }
    None
}

/// The pinned key's id, if Evani.App's folder has one.
fn pinned_key(dir: Option<&Path>) -> (Option<[u8; 32]>, Option<String>) {
    match dir.map(store::read_pubkey) {
        Some(Ok(k)) => (k, None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    }
}

/// A port from the stdin line's `approvals_endpoint`: a number, `{"port": N}`
/// or `http://127.0.0.1:N[/…]` / `http://localhost:N[/…]`.
fn endpoint_port(v: &Value) -> Option<u16> {
    let n = match v {
        Value::Number(n) => n.as_u64(),
        Value::Object(_) => v["port"].as_u64(),
        Value::String(s) => {
            let rest = s.strip_prefix("http://127.0.0.1:").or_else(|| s.strip_prefix("http://localhost:"))?;
            rest.split('/').next()?.parse::<u64>().ok()
        }
        _ => None,
    }?;
    u16::try_from(n).ok().filter(|p| *p != 0)
}

fn verified_json(v: &Verified) -> Value {
    json!({
        "hash": v.hash, "seq": v.seq, "key_id": v.key_id, "test_key": v.test_key,
        "rollback": match &v.rollback {
            Rollback::Checked { current } => json!({ "checked": true, "current": current }),
            Rollback::Offline(why) => json!({ "checked": false, "why": why }),
        },
        "line": v.line(),
    })
}

fn manifest_rel(look: &str) -> String {
    format!("looks/{look}/bake-manifest.json")
}

/// A warning when the key that verified this look is not the one recorded
/// in its last bake manifest.
pub fn key_warning(root: &Path, look: &str, key_id: &str) -> Option<String> {
    let m: Value = serde_json::from_str(&bake::read(root, &manifest_rel(look))?).ok()?;
    let old = m["key_id"].as_str()?;
    (old != key_id).then(|| {
        format!(
            "Evani.App's approvals key changed since the last bake of {look} (key {old} then, {key_id} now). \
             If Evani.App didn't replace its key on purpose, someone swapped the pinned key: check before trusting this bake."
        )
    })
}

impl LookGuard {
    pub fn new(root: &Path, session: &str, game: &str) -> Self {
        let default_look = std::fs::read_to_string(root.join("apps").join(game).join("evani.toml"))
            .ok()
            .and_then(|t| look_from_manifest(&t));
        LookGuard {
            root: root.to_path_buf(),
            session: session.to_string(),
            game: game.to_string(),
            default_look,
            st: Mutex::new(State {
                book: Approvals::new(session.to_string(), Err(OFF_REASON.into())),
                // Tests never read the real Evani.App folder; they set their own.
                store_dir: if cfg!(test) { None } else { store::data_dir() },
                project_id: None,
                seq_port: None,
                approve_port: None,
                stdin_seen: false,
            }),
            capturing: tokio::sync::Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Use another look-approvals folder (tests; never from a command).
    #[cfg(test)]
    pub fn set_store_dir(&self, dir: Option<PathBuf>) {
        self.lock().store_dir = dir;
    }

    pub fn set_approve_port(&self, port: u16) {
        self.lock().approve_port = Some(port);
    }

    pub fn approve_port(&self) -> Option<u16> {
        self.lock().approve_port
    }

    /// Trust for the record check: Evani.App's folder, plus what the stdin
    /// line said.
    fn trust(&self) -> Trust {
        let (dir, project_id, stdin_port) = {
            let st = self.lock();
            (st.store_dir.clone(), st.project_id.clone(), st.seq_port)
        };
        let (pubkey, pubkey_error) = pinned_key(dir.as_deref());
        let seq_port = dir.as_deref().and_then(store::endpoint_port).or(stdin_port);
        Trust { data_dir: dir, pubkey, pubkey_error, project_id, seq_port }
    }

    /// Evani.App's first stdin line: `{"approve_secret", "approvals_pubkey",
    /// "project_id", "approvals_endpoint"}`. Only the first line counts.
    /// Returns one line for the server log (never the secret).
    pub fn on_stdin_line(&self, line: &str) -> String {
        let dir = {
            let mut st = self.lock();
            if st.stdin_seen {
                return "look guard: ignoring a second stdin line".into();
            }
            st.stdin_seen = true;
            st.store_dir.clone()
        };
        let parsed: Result<Value, _> = serde_json::from_str(line.trim());
        let (keys, project_id, port) = match &parsed {
            Ok(v) => {
                let keys = match (v["approve_secret"].as_str(), v["approvals_pubkey"].as_str()) {
                    (Some(s), Some(k)) => Keys::new(s, k, pinned_key(dir.as_deref()).0),
                    _ => Err("the engine's first stdin line has no approve_secret or approvals_pubkey".into()),
                };
                let pid = v["project_id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128 && !s.contains(['/', '\\', ':']) && !s.contains(".."))
                    .map(String::from);
                (keys, pid, endpoint_port(&v["approvals_endpoint"]))
            }
            Err(_) => (Err("the engine's first stdin line is not JSON".to_string()), None, None),
        };
        let keys = keys.map_err(|e| format!("{e}. {OFF_REASON}"));
        let msg = match &keys {
            Ok(k) => format!("look guard: approvals on (key {})", k.key_id()),
            Err(e) => format!("look guard: approvals off: {e}"),
        };
        let mut st = self.lock();
        st.book.set_keys(keys);
        st.project_id = project_id;
        st.seq_port = port;
        msg
    }

    /// `look_guard` in `/live/status`.
    pub fn status(&self) -> Value {
        let dir = self.lock().store_dir.clone();
        let pinned = pinned_key(dir.as_deref()).0.map(|k| record::key_id(&k));
        let st = self.lock();
        let pending = st.book.pending(evani_lookguard::now_ms()).len();
        match st.book.status() {
            Ok(key_id) => json!({ "approvals": "enabled", "reason": null, "approve_port": st.approve_port,
                                  "key_id": key_id, "pending": pending }),
            Err(why) => json!({ "approvals": "disabled", "reason": why, "approve_port": st.approve_port,
                                "key_id": pinned, "pending": pending }),
        }
    }

    fn audit(&self, event: &str, data: Value) {
        if let Err(e) = audit::append(&self.root, event, data, evani_lookguard::now_ms()) {
            eprintln!("evani-live-bridge: look guard audit log: {e}");
        }
    }

    fn look_of(&self, params: &Value) -> Result<String, Fail> {
        let look = match params.get("look") {
            Some(Value::String(s)) if s.starts_with("evani-look-") => s.clone(),
            Some(Value::String(s)) => format!("evani-look-{s}"),
            None | Some(Value::Null) => self.default_look.clone().ok_or_else(|| {
                fail("bad_request", "Say which look (e.g. evani-look-basic); this game's evani.toml names none.")
            })?,
            Some(_) => return Err(fail("bad_request", "look must be a look crate name, e.g. evani-look-basic")),
        };
        if !guardset::is_look_id(&look) {
            return Err(fail("bad_request", format!("{look} is not a look crate name (evani-look-<name>)")));
        }
        Ok(look)
    }

    fn load_guard(&self, look: &str) -> Result<Option<GuardSet>, Fail> {
        let dir = guardset::guard_dir(&self.root, look);
        if !dir.exists() {
            return Ok(None);
        }
        let bad = |e: String| fail("look_unapproved", format!("The guard set of {look} can't be read: {e}."));
        let files = guardset::read_files(&dir).map_err(bad)?;
        let hash = guardset::hash_files(look, &files).map_err(bad)?.hash;
        let text = |name: &str| {
            files
                .iter()
                .find(|(r, _)| r == name)
                .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default()
        };
        let moments = Moments::parse(&text("moments.json")).map_err(bad)?;
        let limits = Limits::parse(&text("limits.json")).map_err(bad)?;
        Ok(Some(GuardSet { files, hash, moments, limits }))
    }

    async fn verify(&self, look: &str) -> Result<Verdict, Failure> {
        let (root, look, trust) = (self.root.clone(), look.to_string(), self.trust());
        tokio::task::spawn_blocking(move || verify::verify_look(&root, &look, &trust))
            .await
            .unwrap_or_else(|e| Err(Failure { code: "internal", message: format!("the look check stopped: {e}") }))
    }

    /// One `capture.frame` on the page.
    async fn shoot(&self, hub: &Hub, params: Value) -> Result<Value, Fail> {
        let req = json!({ "type": "cmd", "v": 1, "id": "look-guard", "cmd": "capture.frame", "params": params,
                          "opts": { "telemetry_ms": 0 } });
        let res = hub.ask_page(hub.http_conn(), "bridge", req).await.map_err(|(code, message, data)| Fail { code: code.into(), message, data })?;
        if res["ok"] == true {
            return Ok(res["result"].clone());
        }
        let e = &res["error"];
        Err(Fail {
            code: e["code"].as_str().unwrap_or("internal").to_string(),
            message: e["message"].as_str().unwrap_or("capture failed").to_string(),
            data: None,
        })
    }

    /// Capture version 1: every moment with the look as it is, then with
    /// bloom off (a game without bloom: its bloom-off shot is the same image).
    async fn capture_all(&self, hub: &Hub, moments: &Moments) -> Result<Vec<Shot>, Fail> {
        let _one = self.capturing.lock().await;
        let mut out = Vec::new();
        for m in &moments.moments {
            let ctx = |f: Fail| Fail { message: format!("capturing moment '{}': {}", m.name, f.message), ..f };
            let on = self.shoot(hub, shots::capture_params(moments, m, false)).await.map_err(ctx)?;
            let off = match self.shoot(hub, shots::capture_params(moments, m, true)).await {
                Ok(v) => v,
                Err(f) if f.code == "unknown_target" => on.clone(),
                Err(f) => return Err(ctx(f)),
            };
            out.push(Shot::from_results(&m.name, &on, &off).map_err(|e| fail("capture_failed", e))?);
        }
        Ok(out)
    }

    fn propose(&self, hub: &Hub, p: Proposal) -> Result<Pending, Fail> {
        let now = evani_lookguard::now_ms();
        let (pending, approvals, port) = {
            let mut st = self.lock();
            let pending = st.book.propose(p, now).map_err(|e| fail("internal", e))?;
            (pending, st.book.status(), st.approve_port)
        };
        let mut data = pending.event(&self.session);
        data["approvePort"] = json!(port);
        data["approvals"] = json!(if approvals.is_ok() { "enabled" } else { "disabled" });
        hub.look_event("look_guard.approval_requested", data);
        self.audit(
            "proposed",
            json!({ "approvalId": pending.id, "kind": pending.kind.as_str(), "look": pending.look, "hash": pending.hash,
                    "oldHash": pending.old_hash, "origin": pending.origin, "note": pending.note,
                    "looser": pending.rows.iter().any(|r| r["looser"] == true), "rows": pending.rows.len() }),
        );
        Ok(pending)
    }

    fn waiting(&self, p: &Pending) -> Value {
        let approvals = self.status();
        let mut message = "Waiting for Chris to approve it in Evani.App; nothing changes until then.".to_string();
        if approvals["approvals"] == "disabled" {
            message = format!(
                "Recorded, but approvals are off ({}), so it can't be approved in this session.",
                approvals["reason"].as_str().unwrap_or(OFF_REASON)
            );
        }
        json!({ "approvalId": p.id, "kind": p.kind.as_str(), "look": p.look, "hash": p.hash, "oldHash": p.old_hash,
                "expiry": p.expiry_ms, "rows": p.rows, "looser": p.rows.iter().any(|r| r["looser"] == true),
                "status": "pending", "approvals": approvals, "message": message })
    }

    // ---- commands -----------------------------------------------------------------

    /// A `look.*` command (the result envelope, with the caller's id).
    pub async fn command(&self, hub: &Hub, source: &str, cmd: &str, req: &Value, id: &Value) -> Value {
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
        let note = req["note"].as_str().unwrap_or("").to_string();
        let r = match cmd {
            "look.status" => self.cmd_status(&params).await,
            "look.check" => self.cmd_check(hub, source, &params).await,
            "look.limits.propose" => self.cmd_limits_propose(hub, source, &params, &note),
            "look.baseline.propose" => self.cmd_baseline_propose(hub, source, &params, &note).await,
            c if c.starts_with("look.approve") => {
                self.audit("approve_refused", json!({ "code": "forbidden", "origin": source, "via": c }));
                Err(fail(
                    "forbidden",
                    "Only Chris approves look limits and flagged bakes, in Evani.App. Propose the change and wait for him.",
                ))
            }
            other => Err(fail("unknown_command", format!("unknown command {other}"))),
        };
        match r {
            Ok(result) => json!({ "type": "result", "v": 1, "id": id, "ok": true, "result": result }),
            Err(f) => proto::error(id, &f.code, &f.message, f.data),
        }
    }

    async fn cmd_status(&self, params: &Value) -> Result<Value, Fail> {
        let look = self.look_of(params)?;
        let verdict = self.verify(&look).await;
        let (guard, verified, problem, key_id) = match &verdict {
            Ok(Verdict::Unguarded) => ("unguarded", Value::Null, Value::Null, None),
            Ok(Verdict::Verified(v)) => ("verified", verified_json(v), Value::Null, Some(v.key_id.clone())),
            Err(f) => ("failed", Value::Null, json!({ "code": f.code, "message": f.message }), None),
        };
        let set = self.load_guard(&look).ok().flatten();
        let now = evani_lookguard::now_ms();
        let pending: Vec<Value> =
            self.lock().book.pending(now).into_iter().filter(|p| p.look == look).map(|p| p.event(&self.session)).collect();
        let last: Option<Value> = bake::read(&self.root, &manifest_rel(&look)).and_then(|t| serde_json::from_str(&t).ok());
        let key_id = key_id.or_else(|| self.status()["key_id"].as_str().map(String::from));
        let warning = key_id.as_deref().and_then(|k| key_warning(&self.root, &look, k));
        let audit_text = std::fs::read_to_string(audit::path(&self.root)).unwrap_or_default();
        let chain = match audit::verify_chain(&audit_text) {
            Ok(n) => json!({ "entries": n, "chain": "ok" }),
            Err(e) => json!({ "entries": audit_text.lines().count(), "chain": format!("broken: {e}") }),
        };
        Ok(json!({
            "look": look, "game": self.game, "guard": guard, "verified": verified, "problem": problem,
            "guard_hash": set.as_ref().map(|s| s.hash.clone()),
            "moments": set.as_ref().map(|s| s.moments.moments.iter().map(|m| m.name.clone()).collect::<Vec<_>>()),
            "tier": set.as_ref().map(|s| s.moments.tier.clone()),
            "width": set.as_ref().map(|s| s.moments.width),
            "approvals": self.status(), "pending": pending,
            "last_bake": last, "key_warning": warning, "audit": chain,
        }))
    }

    async fn cmd_check(&self, hub: &Hub, source: &str, params: &Value) -> Result<Value, Fail> {
        let look = self.look_of(params)?;
        let Some(g) = self.load_guard(&look)? else {
            return Err(fail(
                "no_limits",
                format!("{look} has no look limits yet. Propose a baseline with look_baseline_propose; Chris approves it in Evani.App."),
            ));
        };
        let verdict = self.verify(&look).await;
        let shots = self.capture_all(hub, &g.moments).await?;
        let stats = shots::stats_of(&shots).map_err(|e| fail("capture_failed", e))?;
        let breaches = limits::check(&g.limits, &stats);
        let captures_hash = shots::captures_hash(&shots);
        let (guard, problem) = match &verdict {
            Ok(Verdict::Verified(_)) => ("verified", Value::Null),
            Ok(Verdict::Unguarded) => ("unguarded", Value::Null),
            Err(f) => ("failed", json!({ "code": f.code, "message": f.message })),
        };
        self.audit(
            "check",
            json!({ "look": look, "origin": source, "guardHash": g.hash, "guard": guard, "capturesHash": captures_hash,
                    "passed": breaches.is_empty(), "flags": breaches.iter().map(Breach::id).collect::<Vec<_>>() }),
        );
        Ok(json!({
            "look": look, "passed": breaches.is_empty(), "guard": guard, "problem": problem,
            "guard_hash": g.hash, "captures_hash": captures_hash,
            "sentences": breaches.iter().map(|b| b.sentence.clone()).collect::<Vec<_>>(),
            "breaches": breaches.iter().map(Breach::to_json).collect::<Vec<_>>(),
            "moments": stats.iter().map(|(k, s)| (k.clone(), s.to_json())).collect::<serde_json::Map<_, _>>(),
        }))
    }

    fn cmd_limits_propose(&self, hub: &Hub, source: &str, params: &Value, note: &str) -> Result<Value, Fail> {
        let look = self.look_of(params)?;
        if note.trim().is_empty() {
            return Err(fail("bad_request", "A limits proposal needs a note saying why (Chris sees it)."));
        }
        let Some(g) = self.load_guard(&look)? else {
            return Err(fail(
                "no_limits",
                format!("{look} has no limits to change yet. Propose a baseline with look_baseline_propose first."),
            ));
        };
        let mut patch = serde_json::Map::new();
        for k in ["tolerances", "band_widen"] {
            if let Some(v) = params.get(k) {
                patch.insert(k.into(), v.clone());
            }
        }
        if patch.is_empty() {
            return Err(fail("bad_request", "Say what to change: tolerances and/or band_widen."));
        }
        let new_limits = g.limits.with_settings(&Value::Object(patch)).map_err(|e| fail("bad_request", e))?;
        let mut files = g.files.clone();
        for f in &mut files {
            if f.0 == "limits.json" {
                f.1 = new_limits.file_text().into_bytes();
            }
        }
        let hash = guardset::hash_files(&look, &files).map_err(|e| fail("internal", e))?.hash;
        if hash == g.hash {
            return Err(fail("bad_request", "That changes nothing in the limits."));
        }
        let rows: Vec<Value> =
            limits::diff(Some((&g.moments, &g.limits)), (&g.moments, &new_limits)).iter().map(|r| r.to_json()).collect();
        let p = self.propose(
            hub,
            Proposal {
                kind: Kind::Limits,
                look,
                hash,
                old_hash: Some(g.hash),
                bind: None,
                rows,
                files,
                origin: source.into(),
                note: note.into(),
            },
        )?;
        Ok(self.waiting(&p))
    }

    async fn cmd_baseline_propose(&self, hub: &Hub, source: &str, params: &Value, note: &str) -> Result<Value, Fail> {
        let look = self.look_of(params)?;
        if note.trim().is_empty() {
            return Err(fail("bad_request", "A baseline proposal needs a note saying why (Chris sees it)."));
        }
        let old = self.load_guard(&look)?;
        let moments = match params.get("moments").filter(|v| !v.is_null()) {
            Some(Value::Object(o)) => {
                let mut o = o.clone();
                o.entry("format").or_insert(json!(limits::MOMENTS_FORMAT));
                o.entry("game").or_insert(json!(self.game));
                o.entry("tier").or_insert(json!("medium"));
                o.entry("width").or_insert(json!(320));
                Moments::from_json(&Value::Object(o)).map_err(|e| fail("bad_request", e))?
            }
            Some(_) => return Err(fail("bad_request", "moments must be an object like moments.json")),
            None => match &old {
                Some(g) => g.moments.clone(),
                None => {
                    return Err(fail(
                        "bad_request",
                        format!("{look} has no moments.json yet: give moments, e.g. {{\"tier\":\"medium\",\"width\":320,\"moments\":[{{\"name\":\"start\",\"capture\":{{\"sim_time\":2,\"camera\":\"path\"}}}}]}}"),
                    ));
                }
            },
        };
        if moments.game != self.game {
            return Err(fail(
                "bad_request",
                format!("The moments are for game {}, but this live server runs {}.", moments.game, self.game),
            ));
        }
        let shots = self.capture_all(hub, &moments).await?;
        let stats = shots::stats_of(&shots).map_err(|e| fail("capture_failed", e))?;
        let new_limits = Limits::generate(&look, stats, old.as_ref().map(|g| &g.limits));
        let files = shots::staged_files(&moments, &new_limits, &shots);
        let hash = guardset::hash_files(&look, &files).map_err(|e| fail("internal", e))?.hash;
        if old.as_ref().is_some_and(|g| g.hash == hash) {
            return Err(fail("bad_request", "The new captures are identical to the approved baseline; nothing to approve."));
        }
        let rows: Vec<Value> = limits::diff(old.as_ref().map(|g| (&g.moments, &g.limits)), (&moments, &new_limits))
            .iter()
            .map(|r| r.to_json())
            .collect();
        let p = self.propose(
            hub,
            Proposal {
                kind: Kind::Limits,
                look,
                hash,
                old_hash: old.map(|g| g.hash),
                bind: None,
                rows,
                files,
                origin: source.into(),
                note: note.into(),
            },
        )?;
        Ok(self.waiting(&p))
    }

    // ---- the bake gate ------------------------------------------------------------

    /// The page's OK `bake.export` result → the 6.23 result, after the
    /// look check (15.9). Dry runs write nothing and pass straight through.
    pub async fn bake(&self, hub: &Hub, source: &str, id: &Value, res: Value) -> Value {
        let heading = evani_live::bake::heading_time(&evani_live::session::now_iso());
        let bundle = res["result"]["bundle"].clone();
        if !bundle.is_object() || bundle["dry_run"] == true {
            return bake::finish(&self.root, id, res, &heading);
        }
        let Some(look) = bundle["look_crate"].as_str().filter(|l| guardset::is_look_id(l)).map(String::from) else {
            return proto::error(id, "bad_request", "The bake bundle names no look crate, so the look check can't run.", None);
        };
        match self.gate(hub, source, &look, &bundle).await {
            Err(f) => {
                if f.code != "look_flagged" {
                    self.audit("bake_refused", json!({ "look": look, "code": f.code, "message": f.message, "origin": source }));
                }
                proto::error(id, &f.code, &f.message, f.data)
            }
            Ok(pass) => {
                let out = bake::finish(&self.root, id, res, &heading);
                if out["ok"] != true {
                    return out;
                }
                self.after_bake(&look, source, pass, out)
            }
        }
    }

    async fn gate(&self, hub: &Hub, source: &str, look: &str, bundle: &Value) -> Result<Pass, Fail> {
        let v = match self.verify(look).await {
            Ok(Verdict::Verified(v)) => v,
            Ok(Verdict::Unguarded) => {
                return Err(Fail {
                    code: "look_unapproved".into(),
                    message: format!(
                        "{look} has no approved look limits yet, so this bake can't be checked. Propose a baseline with look_baseline_propose and have Chris approve it in Evani.App, then bake."
                    ),
                    data: Some(json!({ "look": look, "reason": "unguarded" })),
                });
            }
            Err(f) => {
                return Err(Fail {
                    code: "look_unapproved".into(),
                    message: f.message,
                    data: Some(json!({ "look": look, "reason": f.code })),
                });
            }
        };
        let bundle_hash = shots::bundle_hash(bundle);
        let bind = approvals::bind_key(&v.hash, &bundle_hash);
        let key_warning = key_warning(&self.root, look, &v.key_id);
        let grant = self.lock().book.take_grant(look, &bind, evani_lookguard::now_ms());
        if let Some(g) = grant {
            return Ok(Pass {
                v,
                check: "approved",
                approval_id: Some(g.approval_id),
                bake_hash: Some(g.hash),
                captures_hash: None,
                bundle_hash,
                key_warning,
            });
        }
        let g = self.load_guard(look)?.ok_or_else(|| fail("look_unapproved", format!("The guard set of {look} disappeared.")))?;
        if g.hash != v.hash {
            return Err(fail("look_unapproved", format!("The guard set of {look} changed while it was being checked; bake again.")));
        }
        let shots = self.capture_all(hub, &g.moments).await?;
        let stats = shots::stats_of(&shots).map_err(|e| fail("capture_failed", e))?;
        let breaches = limits::check(&g.limits, &stats);
        let captures_hash = shots::captures_hash(&shots);
        if breaches.is_empty() {
            return Ok(Pass {
                v,
                check: "passed",
                approval_id: None,
                bake_hash: None,
                captures_hash: Some(captures_hash),
                bundle_hash,
                key_warning,
            });
        }
        let bake_hash = limits::bake_hash(look, &v.hash, &captures_hash, &bundle_hash, &breaches);
        let sentences: Vec<String> = breaches.iter().map(|b| b.sentence.clone()).collect();
        let title = bundle["title"].as_str().unwrap_or("").trim();
        let p = self.propose(
            hub,
            Proposal {
                kind: Kind::Bake,
                look: look.into(),
                hash: bake_hash.clone(),
                old_hash: Some(v.hash.clone()),
                bind: Some(bind),
                rows: breaches.iter().map(Breach::to_json).collect(),
                files: Vec::new(),
                origin: source.into(),
                note: if title.is_empty() { "bake".into() } else { format!("bake: {title}") },
            },
        )?;
        self.audit(
            "bake_flagged",
            json!({ "look": look, "approvalId": p.id, "hash": bake_hash, "guardHash": v.hash, "capturesHash": captures_hash,
                    "bundleHash": bundle_hash, "flags": breaches.iter().map(Breach::id).collect::<Vec<_>>(), "origin": source }),
        );
        let approvals = self.status();
        let mut message = format!(
            "The look check flagged {} on {look}: {}. Chris must approve this bake in Evani.App; then run the same bake again within 5 minutes.",
            if sentences.len() == 1 { "1 thing".to_string() } else { format!("{} things", sentences.len()) },
            sentences.join("; ")
        );
        if approvals["approvals"] == "disabled" {
            message.push_str(&format!(
                " (Approvals are off in this session: {}.)",
                approvals["reason"].as_str().unwrap_or(OFF_REASON).trim_end_matches('.')
            ));
        }
        Err(Fail {
            code: "look_flagged".into(),
            message,
            data: Some(json!({ "approvalId": p.id, "look": look, "sentences": sentences,
                               "breaches": p.rows, "expiry": p.expiry_ms, "hash": bake_hash, "approvals": approvals })),
        })
    }

    fn after_bake(&self, look: &str, source: &str, pass: Pass, mut out: Value) -> Value {
        let now = evani_lookguard::now_ms();
        let written: Vec<Value> = out["result"]["written"]
            .as_array()
            .map(|a| a.iter().filter_map(|w| w["path"].as_str().map(|p| json!(p))).collect())
            .unwrap_or_default();
        let rollback = match &pass.v.rollback {
            Rollback::Checked { .. } => "checked".to_string(),
            Rollback::Offline(why) => format!("offline: {why}"),
        };
        let manifest = json!({
            "format": "evani-look-bake-manifest/1",
            "look": look, "game": self.game, "baked_at": evani_lookguard::iso_utc_ms(now),
            "guard_hash": pass.v.hash, "seq": pass.v.seq, "key_id": pass.v.key_id, "test_key": pass.v.test_key,
            "rollback": rollback, "check": pass.check, "approval_id": pass.approval_id, "bake_hash": pass.bake_hash,
            "bundle_hash": pass.bundle_hash, "captures_hash": pass.captures_hash, "written": written,
        });
        let mut warnings: Vec<String> = Vec::new();
        let rel = manifest_rel(look);
        if let Err(e) = bake::write(&self.root, &rel, &limits::pretty(&manifest)) {
            warnings.push(format!("the bake manifest wasn't written: {e}"));
        }
        if let Some(cm) = out["result"]["changes_md"].as_str() {
            let line = match pass.check {
                "approved" => format!(
                    "\nLook check flagged this bake; Chris approved it (approval {}, limits {}…, key {}).\n",
                    pass.approval_id.as_deref().unwrap_or("?"),
                    &pass.v.hash[..12],
                    pass.v.key_id
                ),
                _ => format!(
                    "\nLook check passed against Chris's approved limits {}… (seq {}, key {}).\n",
                    &pass.v.hash[..12],
                    pass.v.seq,
                    pass.v.key_id
                ),
            };
            let mut text = bake::read(&self.root, cm).unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&line);
            if let Err(e) = bake::write(&self.root, cm, &text) {
                warnings.push(format!("CHANGES.md note not written: {e}"));
            }
        }
        if pass.v.test_key {
            warnings.push("the approval record is signed with the public TEST key, so it proves nothing".into());
        }
        if let Some(w) = &pass.key_warning {
            warnings.push(w.clone());
        }
        self.audit(
            "baked",
            json!({ "look": look, "check": pass.check, "approvalId": pass.approval_id, "guardHash": pass.v.hash,
                    "seq": pass.v.seq, "keyId": pass.v.key_id, "bundleHash": pass.bundle_hash,
                    "capturesHash": pass.captures_hash, "written": written, "origin": source }),
        );
        out["result"]["look_guard"] = json!({
            "look": look, "check": pass.check, "approval_id": pass.approval_id, "guard_hash": pass.v.hash,
            "seq": pass.v.seq, "key_id": pass.v.key_id, "test_key": pass.v.test_key, "rollback": rollback,
            "manifest": rel, "key_warning": pass.key_warning, "warnings": warnings,
        });
        out
    }

    // ---- approvals ------------------------------------------------------------------

    /// `POST /look-guard/v1/approve` (15.7), after the server's Host and
    /// Origin checks. `mac` is the header's hex.
    pub fn approve(&self, hub: &Hub, mac: &str, body: &Value) -> Result<Value, Refusal> {
        let refused = |r: Refusal| {
            self.audit("approve_refused", json!({ "code": r.code, "approvalId": body["approvalId"], "via": "approve" }));
            r
        };
        let parsed = ApproveBody::parse(body).map_err(refused)?;
        let root = self.root.clone();
        let now = evani_lookguard::now_ms();
        let result = self.lock().book.approve(&parsed, mac, now, |look| {
            guardset::guard_hash(&root, look).map(|g| g.map(|g| g.hash))
        });
        let p = result.map_err(refused)?;
        let mut out = json!({ "ok": true, "approvalId": p.id, "kind": p.kind.as_str(), "look": p.look, "hash": p.hash });
        let mut data = json!({ "approvalId": p.id, "kind": p.kind.as_str(), "look": p.look, "hash": p.hash });
        if p.kind == Kind::Limits {
            let (written, removed) = self.write_guard(&p).map_err(|e| {
                self.audit("approve_failed", json!({ "approvalId": p.id, "look": p.look, "error": e }));
                Refusal { code: "io_error", message: e }
            })?;
            let now_hash = guardset::guard_hash(&self.root, &p.look).ok().flatten().map(|g| g.hash);
            let verdict = verify::verify_look(&self.root, &p.look, &self.trust());
            out["written"] = json!(written);
            out["removed"] = json!(removed);
            out["guard_hash"] = json!(now_hash);
            out["verify"] = match verdict {
                Ok(Verdict::Verified(v)) => json!({ "ok": true, "seq": v.seq, "key_id": v.key_id }),
                Ok(Verdict::Unguarded) => json!({ "ok": false, "code": "unguarded" }),
                Err(f) => json!({ "ok": false, "code": f.code, "message": f.message }),
            };
            data["written"] = json!(written.len());
            data["removed"] = json!(removed.len());
        } else {
            out["message"] = json!("Approved: the same bake, run again within 5 minutes, goes ahead once.");
        }
        self.audit(
            "approved",
            json!({ "approvalId": p.id, "kind": p.kind.as_str(), "look": p.look, "hash": p.hash, "oldHash": p.old_hash,
                    "origin": p.origin, "via": "approve" }),
        );
        hub.look_event("look_guard.approved", data);
        Ok(out)
    }

    /// An approve request the listener turned away before reading it (Host,
    /// Origin, method), for the audit log.
    pub fn refused_at_door(&self, code: &str, why: &str) {
        self.audit("approve_refused", json!({ "code": code, "via": "approve", "why": why }));
    }

    /// Write an approved limits proposal's files to the guard folder and
    /// remove baseline captures that are no longer in the set.
    fn write_guard(&self, p: &Pending) -> Result<(Vec<String>, Vec<String>), String> {
        if !guardset::is_look_id(&p.look) {
            return Err(format!("bad look {}", p.look));
        }
        if p.files.iter().any(|(rel, _)| !guardset::counts(rel)) {
            return Err("the staged files are not a guard set".into());
        }
        if guardset::hash_files(&p.look, &p.files)?.hash != p.hash {
            return Err("the staged files don't hash to the approved hash".into());
        }
        let base = format!("looks/{}/guard", p.look);
        let mut written = Vec::new();
        for (rel, bytes) in &p.files {
            let path = format!("{base}/{rel}");
            bake::write_bytes(&self.root, &path, bytes)?;
            written.push(path);
        }
        let mut removed = Vec::new();
        if let Ok(rd) = std::fs::read_dir(guardset::guard_dir(&self.root, &p.look).join("baseline")) {
            for e in rd.flatten() {
                let Some(name) = e.file_name().to_str().map(String::from) else { continue };
                let rel = format!("baseline/{name}");
                if guardset::counts(&rel) && !p.files.iter().any(|(r, _)| *r == rel) {
                    let path = format!("{base}/{rel}");
                    bake::remove(&self.root, &path)?;
                    removed.push(path);
                }
            }
        }
        Ok((written, removed))
    }
}

#[cfg(test)]
#[path = "lookguard_tests.rs"]
pub(crate) mod tests;
