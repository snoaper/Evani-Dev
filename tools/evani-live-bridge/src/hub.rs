//! Routing between controllers (`/ctl`, `POST /live/v1/cmd`), the page
//! (`/engine`) and the Evani.dev tab (`/evani-dev`). The hub holds no render
//! state: the page is the source of truth (docs/LIVE_API.md 1).
//!
//! Each command gets a bridge-wide id before it goes to the page (controllers
//! choose their own ids, which may collide); the page's result is matched
//! back by that id and returned with the controller's original id.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::lookguard::LookGuard;
use crate::proto::{self, MUTATING};

/// Outgoing text frames for one socket.
pub type Outbox = mpsc::UnboundedSender<String>;

/// Build state of the game, shown in `/live/status` (3.1).
#[derive(Debug, Clone, PartialEq)]
pub struct BuildState {
    /// `idle | building | error`.
    pub phase: &'static str,
    pub error: Option<String>,
    pub build_ms: u64,
}

impl Default for BuildState {
    fn default() -> Self {
        Self {
            phase: "building",
            error: None,
            build_ms: 0,
        }
    }
}

/// Fixed facts about this bridge.
#[derive(Debug, Clone)]
pub struct HubInfo {
    pub port: u16,
    pub root: String,
    pub game: String,
    pub session: String,
}

struct Pending {
    reply: oneshot::Sender<Value>,
    conn: u64,
}

#[derive(Default)]
struct Inner {
    next: u64,
    engine: Option<(u64, Outbox)>,
    pending: HashMap<String, Pending>,
    ctls: HashMap<u64, Outbox>,
    devs: HashMap<u64, Outbox>,
    /// Telemetry subscription id â†’ the connection that owns it.
    subs: HashMap<String, u64>,
    /// The page's `hello` (game, schema_version, tier).
    page_hello: Option<Value>,
    tier: Option<String>,
    /// Latest `session.mirror` from the page (10.3; kept, and forwarded to every /ctl client).
    mirror: Option<Value>,
    build: BuildState,
    /// The open page's `page_rev` (its `<meta name="evani-page-rev">`, 14.2).
    page_rev: Option<String>,
    /// `page_patch` messages waiting for the page's `page.patched`, by seq.
    page_patches: HashMap<u64, oneshot::Sender<Value>>,
    /// Live code swapping (10.4).
    code: CodeState,
    /// What the page last said about itself (`page_state`, 3.5). `None`
    /// until the first one (or for a page from before 3.5).
    page: Option<PageState>,
    /// Model mode: `/ctl` connections that sent `view.watch` (EVANI_MODEL_TAB.md 5.2).
    watchers: std::collections::HashSet<u64>,
}

/// No `page_state` for this long counts as a stalled page (the page sends
/// one every second while its scripts run).
const PAGE_SILENT: Duration = Duration::from_millis(2500);
/// How often a waiting command checks whether the page has paused.
const PAUSE_POLL: Duration = Duration::from_millis(100);
/// A code patch the page hasn't applied (or failed) this long after it was
/// sent, while the page kept running frames, is stuck (10.4).
const PATCH_STALL: Duration = Duration::from_secs(15);
/// Frames the page must have run since a patch was sent before it counts
/// as stuck (a parked page that runs no frames isn't stuck).
const PATCH_STALL_FRAMES: u64 = 60;
/// What every paused answer tells the caller to do.
pub const PAUSED_FIX: &str = "Show the Evani.dev tab in Evani.App (bring its window back if it is minimised or covered); \
the game runs again and live commands answer. Ask the user to do that if you can't.";

/// The page's own report (`page_state`, LIVE_API.md 3.5): its frame
/// counter, `document.visibilityState` and Evani.App's host state.
#[derive(Debug, Clone)]
struct PageState {
    /// When the last `page_state` (of any reason) arrived.
    at: std::time::Instant,
    frames: u64,
    /// The frame counter at the last heartbeat.
    beat_frames: Option<u64>,
    /// Frames ran between the last two heartbeats (or since the last one).
    moving: bool,
    visibility: String,
    /// `window.__evaniHost` (`{v, shown, paused}`), or null outside Evani.App.
    host: Value,
}

impl PageState {
    /// Why the page might not run frames: Evani.App's `paused` text, or a
    /// hidden document.
    fn reason(&self) -> Option<String> {
        if let Some(p) = self.host["paused"].as_str().map(str::trim).filter(|p| !p.is_empty()) {
            return Some(p.chars().take(60).collect());
        }
        (self.visibility == "hidden").then(|| "paused (tab hidden)".to_string())
    }

    /// Paused: there is a reason AND the page is stalled (silent past
    /// [`PAGE_SILENT`], or its frame counter stood still between beats).
    /// A page Evani.App calls paused that still runs frames answers, so it
    /// isn't paused here.
    fn paused(&self, now: std::time::Instant) -> Option<String> {
        let reason = self.reason()?;
        let silent = now.saturating_duration_since(self.at) > PAGE_SILENT;
        (silent || !self.moving).then_some(reason)
    }

    fn json(&self, now: std::time::Instant) -> Value {
        let paused = self.paused(now);
        json!({
            "state": if paused.is_some() { "paused" } else { "running" },
            "reason": paused,
            "visibility": self.visibility,
            "host": self.host,
            "frames": self.frames,
            "frames_moving": self.moving,
            "last_heard_ms": now.saturating_duration_since(self.at).as_millis() as u64,
        })
    }
}

/// `Hub::ask_page`'s error: a code, a sentence and `error.data`.
pub type PageErr = (&'static str, String, Option<Value>);

/// `error.data` of a paused answer, built when the pause is found: why, what
/// to do, and the page state.
fn paused_data(g: &Inner, why: &str) -> Value {
    let page = g.page.as_ref().map_or(Value::Null, |p| p.json(std::time::Instant::now()));
    json!({ "reason": why, "fix": PAUSED_FIX, "page": page })
}

/// A code patch sent to the page and not yet reported applied or failed.
#[derive(Debug, Clone, Copy)]
struct PatchWait {
    seq: u64,
    at: std::time::Instant,
    /// The page's frame counter when it was sent (`None`: the page sends no
    /// `page_state`, so the stall watchdog stays off).
    frames: Option<u64>,
}

/// How many good code patches are kept (only the newest is ever re-sent).
const CODE_KEEP: usize = 8;
/// How many code events `/live/status` lists.
const CODE_HISTORY: usize = 20;

/// Live code swapping as the bridge sees it (LIVE_API.md 10.4).
#[derive(Default)]
struct CodeState {
    /// `{"mode":"hot"|"trunk", "reason":…}` for `/live/status`.
    mode: Value,
    next_seq: u64,
    /// `code_patch` messages not known to be bad, oldest first. Each is
    /// cumulative from the build the page loaded, so the newest one alone
    /// brings a freshly loaded page up to date.
    good: Vec<(u64, Value)>,
    /// Patches the page couldn't load or crashed in: never sent again.
    bad: Vec<u64>,
    /// The latest events (`patched`, `error`, `failed`, `crashed`, `reload`, `stalled`).
    history: std::collections::VecDeque<Value>,
    /// The patch the page is loading (stall watchdog).
    waiting: Option<PatchWait>,
    /// Patches that stalled once (a second stall marks them bad).
    stalled: Vec<u64>,
}

impl CodeState {
    fn remember(&mut self, v: Value) {
        if self.history.len() == CODE_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(v);
    }

    fn mark_bad(&mut self, seq: u64) {
        if seq > 0 && !self.bad.contains(&seq) {
            self.bad.push(seq);
        }
        self.good.retain(|(s, _)| *s != seq);
    }
}

impl Inner {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// The page went away: every command waiting on it fails (their senders
    /// drop) and its subscriptions are gone.
    fn drop_engine(&mut self) {
        self.engine = None;
        self.pending.clear();
        self.subs.clear();
        self.page_hello = None;
        self.page_rev = None;
        self.page_patches.clear();
        self.page = None;
        self.code.waiting = None;
    }

    /// The page's paused reason, if it is paused now.
    fn paused(&self) -> Option<String> {
        self.page.as_ref()?.paused(std::time::Instant::now())
    }

    /// A `code_patch` went to the page: start the stall watchdog.
    fn patch_sent(&mut self, seq: u64) {
        let frames = self.page.as_ref().map(|p| p.frames);
        self.code.waiting = Some(PatchWait { seq, at: std::time::Instant::now(), frames });
    }
}

pub struct Hub {
    pub info: HubInfo,
    base_timeout: Duration,
    inner: Mutex<Inner>,
    /// The look guard (docs/LIVE_API.md 15): `look.*` commands, the bake
    /// gate and the approval book.
    pub lookguard: LookGuard,
    /// Model mode (`--models`, EVANI_MODEL_TAB.md 4.2): the Evani.Model
    /// workspace. `None`: the bridge behaves exactly as without the flag.
    pub models: Option<crate::models::Workspace>,
    /// Drop-to-convert (16): the one-at-a-time evani-convert job slot.
    pub convert: crate::convert::Converter,
    /// `serve --app-dir`: the game's app folder (outside the engine). The
    /// bridge then only builds and serves it and relays the page's own live
    /// commands; its own features that read the engine's apps/ layout answer
    /// "not available for --app-dir yet" ([`OUTSIDE_LATER`]).
    pub outside: Option<String>,
}

/// What `serve --app-dir` doesn't do yet (Evani.Dev's own live/dev work for
/// games outside the engine): the look guard, drop-to-convert, model mode.
/// Live code swapping and page hot-replace are off from the start.
pub const OUTSIDE_LATER: &str = "is not available for --app-dir yet: a game outside the engine folder gets the build, the page and its live commands (capture, tier, params); the look guard, model conversion and live code swapping are in-tree only for now";

impl Hub {
    pub fn new(info: HubInfo) -> Self {
        Self::with_timeout(info, proto::BASE_TIMEOUT)
    }

    pub fn with_timeout(info: HubInfo, base_timeout: Duration) -> Self {
        let lookguard = LookGuard::new(std::path::Path::new(&info.root), &info.session, &info.game);
        let convert = crate::convert::Converter::new(std::path::Path::new(&info.root));
        Self {
            info,
            base_timeout,
            inner: Mutex::new(Inner::default()),
            lookguard,
            models: None,
            convert,
            outside: None,
        }
    }

    /// `serve --app-dir`: the game in `app_dir`, outside the engine.
    pub fn with_outside(mut self, app_dir: String) -> Self {
        self.outside = Some(app_dir);
        self
    }

    /// Turn on model mode with this workspace.
    pub fn with_models(mut self, ws: crate::models::Workspace) -> Self {
        self.models = Some(ws);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding the lock can't leave Inner half-updated in a
        // way that matters (maps of channels), so keep going.
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // ---- connections ------------------------------------------------------------

    /// A page authenticated on `/engine`. A newer page replaces the older one
    /// (its outbox is dropped, which closes that socket).
    pub fn attach_engine(&self, out: Outbox) -> u64 {
        let mut g = self.lock();
        let id = g.id();
        if g.engine.is_some() {
            g.drop_engine();
        }
        g.engine = Some((id, out));
        id
    }

    pub fn detach_engine(&self, id: u64) {
        let mut g = self.lock();
        if g.engine.as_ref().is_some_and(|(e, _)| *e == id) {
            g.drop_engine();
        }
    }

    #[cfg(test)]
    pub fn engine_connected(&self) -> bool {
        self.lock().engine.is_some()
    }

    pub fn attach_ctl(&self, out: Outbox) -> u64 {
        let mut g = self.lock();
        let id = g.id();
        g.ctls.insert(id, out);
        id
    }

    /// A controller left: end the telemetry subscriptions it owned.
    pub fn detach_ctl(&self, conn: u64) {
        let mut g = self.lock();
        g.ctls.remove(&conn);
        g.watchers.remove(&conn);
        g.pending.retain(|_, p| p.conn != conn);
        let owned: Vec<String> = g
            .subs
            .iter()
            .filter(|(_, c)| **c == conn)
            .map(|(s, _)| s.clone())
            .collect();
        for sub in owned {
            g.subs.remove(&sub);
            let n = g.id();
            if let Some((_, eng)) = &g.engine {
                let msg = json!({ "type": "cmd", "v": 1, "id": format!("bx{n}"),
                    "cmd": "telemetry.unsubscribe", "params": { "sub": sub }, "source": "bridge" });
                let _ = eng.send(msg.to_string());
            }
        }
    }

    pub fn attach_dev(&self, out: Outbox) -> u64 {
        let mut g = self.lock();
        let id = g.id();
        g.devs.insert(id, out);
        id
    }

    pub fn detach_dev(&self, conn: u64) {
        self.lock().devs.remove(&conn);
    }

    /// A fresh id for an HTTP request (it gets no outbox).
    pub fn http_conn(&self) -> u64 {
        self.lock().id()
    }

    // ---- status -------------------------------------------------------------------

    pub fn set_build(&self, b: BuildState) {
        self.lock().build = b;
    }

    pub fn build(&self) -> BuildState {
        self.lock().build.clone()
    }

    /// `GET /live/status` (3.1). Never contains the token.
    pub fn status(&self) -> Value {
        let look_guard = self.lookguard.status();
        let models = self.models.as_ref().map(crate::models::Workspace::status);
        let g = self.lock();
        let tier = g
            .tier
            .clone()
            .or_else(|| g.page_hello.as_ref().and_then(|h| h["tier"].as_str().map(String::from)));
        let mut status = json!({
            "type": "live_hello",
            "build": 0,
            "url": null,
            "phase": g.build.phase,
            "error": g.build.error,
            "build_ms": g.build.build_ms,
            "relay": format!("ws://localhost:{}", self.info.port),
            "engine": proto::ENGINE,
            "protocol": proto::PROTOCOL,
            "root": self.info.root,
            "game": self.info.game,
            "engine_connected": g.engine.is_some(),
            "tier": tier,
            // 3.5: running, paused (with why) or unknown (no page_state yet).
            "page": match (&g.engine, &g.page) {
                (None, _) => json!({ "state": "none" }),
                (Some(_), None) => json!({ "state": "unknown" }),
                (Some(_), Some(p)) => p.json(std::time::Instant::now()),
            },
            "code": self.code_status(&g),
            "look_guard": look_guard,
        });
        // `serve --app-dir` only; absent for a game in apps/.
        if let Some(dir) = &self.outside {
            status["app_dir"] = json!(dir);
        }
        // Model mode only (EVANI_MODEL_TAB.md 4.2); absent otherwise.
        if let Some(m) = models {
            status["models"] = m;
        }
        status
    }

    /// `/live/status` `code` (10.4): how code changes reach the page.
    fn code_status(&self, g: &Inner) -> Value {
        let mut v = if g.code.mode.is_object() { g.code.mode.clone() } else { json!({ "mode": "trunk" }) };
        v["seq"] = json!(g.code.good.last().map_or(0, |(s, _)| *s));
        v["history"] = Value::Array(g.code.history.iter().cloned().collect());
        v
    }

    /// The `hello` sent to every `/ctl` client on connect (3.3).
    pub fn ctl_hello(&self) -> Value {
        let g = self.lock();
        let schema_version = g.page_hello.as_ref().map_or(Value::Null, |h| h["schema_version"].clone());
        json!({
            "type": "hello", "v": 1, "protocol": proto::PROTOCOL, "engine": proto::ENGINE,
            "game": self.info.game, "engine_connected": g.engine.is_some(),
            "schema_version": schema_version, "session": self.info.session,
        })
    }

    pub fn mirror(&self) -> Option<Value> {
        self.lock().mirror.clone()
    }

    // ---- commands -------------------------------------------------------------------

    /// Run one command from a controller and return its result envelope (with
    /// the controller's own id). Never hangs past the command's limit.
    pub async fn command(&self, conn: u64, source: &str, req: Value) -> Value {
        let env = match proto::check_envelope(&req) {
            Ok(e) => e,
            Err(fail) => return fail,
        };
        let original = Value::String(env.id.clone());
        if self.outside.is_some() && (env.cmd.starts_with("look.") || env.cmd.starts_with("model.") || env.cmd == "view.watch") {
            return proto::error(&original, "not_available", &format!("`{}` {OUTSIDE_LATER}", env.cmd), None);
        }
        // The look guard's commands are the bridge's own (14); the page
        // never sees them.
        if env.cmd.starts_with("look.") {
            return self.lookguard.command(self, source, &env.cmd, &req, &original).await;
        }
        // Drop-to-convert (16): the bridge runs these; `model.load` and
        // `model.compare` go on to the page.
        if env.cmd == "model.convert" || env.cmd == "model.report" {
            return self.convert_command(&env.cmd, &req["params"], req["note"].as_str(), &original).await;
        }
        // The page loads by URL: give it the job record (files, master).
        let mut req = req;
        if env.cmd == "model.load" && !req["params"]["job"].is_object() {
            let conv = self.convert.clone();
            let hash = req["params"]["hash"].as_str().map(String::from);
            match tokio::task::spawn_blocking(move || conv.report(hash.as_deref())).await {
                Ok(Ok(v)) => req["params"]["job"] = v["job"].clone(),
                Ok(Err(r)) => return proto::error(&original, r.code, &r.message, None),
                Err(e) => return proto::error(&original, "internal", &format!("model.load stopped: {e}"), None),
            }
        }
        // Model mode (EVANI_MODEL_TAB.md 5.2, 5.3): `view.watch` and
        // `model.*` are the bridge's own and work with no page.
        if let Some(ws) = &self.models {
            if env.cmd == "view.watch" {
                self.lock().watchers.insert(conn);
                return json!({ "type": "result", "v": 1, "id": original, "ok": true, "result": { "watching": true } });
            }
            if let Some(r) = ws.command(&env.cmd, &req["params"]) {
                return match r {
                    Ok(result) => json!({ "type": "result", "v": 1, "id": original, "ok": true, "result": result }),
                    Err(e) => proto::error(&original, e.code, &e.message, None),
                };
            }
        }
        let mut res = match self.ask_page(conn, source, req).await {
            Ok(v) => v,
            Err((code, message, data)) => return proto::error(&original, code, &message, data),
        };
        res["id"] = original.clone();
        if env.cmd == "bake.export" && res["ok"] == true {
            // The look check runs inside the bake itself (15.9).
            res = self.lookguard.bake(self, source, &original, res).await;
        }
        self.after_result(conn, source, &env.cmd, &res);
        res
    }

    /// Send one command to the page and wait for its result envelope (whose
    /// id is the bridge id). Err: a code, a sentence and `error.data` for
    /// the caller (a paused answer carries why and what to do, 3.5).
    pub async fn ask_page(&self, conn: u64, source: &str, mut req: Value) -> Result<Value, PageErr> {
        let cmd = req["cmd"].as_str().unwrap_or("?").to_string();
        let limit = proto::limit_for(&req, self.base_timeout);
        let gone = || ("no_engine", "The game page isn't connected to the live server.".to_string(), None);
        let (tx, mut rx) = oneshot::channel();
        let bridge_id = {
            let mut g = self.lock();
            let n = g.id();
            let Some((_, eng)) = &g.engine else {
                return Err(gone());
            };
            // A paused page answers nothing until it runs again (3.5): say
            // so at once instead of waiting out the limit, and don't queue
            // the command in it.
            if let Some(why) = g.paused() {
                let data = paused_data(&g, &why);
                return Err(("paused", format!(
                    "{why}: the game in the Evani.dev tab isn't running, so it can't answer {cmd} \
                     (not sent). Show the Evani.dev tab, then try again."
                ), Some(data)));
            }
            let bridge_id = format!("b{n}");
            req["id"] = Value::String(bridge_id.clone());
            req["source"] = Value::String(source.to_string());
            if eng.send(req.to_string()).is_err() {
                g.drop_engine();
                return Err(gone());
            }
            g.pending.insert(bridge_id.clone(), Pending { reply: tx, conn });
            bridge_id
        };
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            tokio::select! {
                r = &mut rx => {
                    return match r {
                        Ok(v) => Ok(v),
                        Err(_) => Err(("no_engine", "The game page disconnected before answering.".into(), None)),
                    };
                }
                _ = tokio::time::sleep_until(deadline) => {
                    self.lock().pending.remove(&bridge_id);
                    return Err(("timeout", format!("The page didn't answer {cmd} within {:.0} s.", limit.as_secs_f64()), None));
                }
                _ = tokio::time::sleep(PAUSE_POLL) => {
                    let mut g = self.lock();
                    if let Some(why) = g.paused() {
                        g.pending.remove(&bridge_id);
                        let data = paused_data(&g, &why);
                        return Err(("paused", format!(
                            "{why}: the game in the Evani.dev tab stopped running before it answered {cmd}; \
                             it was already sent, so it may still run when the game runs again. \
                             Show the Evani.dev tab, then check with live_status."
                        ), Some(data)));
                    }
                }
            }
        }
    }

    /// `page_state` from the page (3.5): keep it, and run the code-patch
    /// stall watchdog on it.
    fn on_page_state(&self, g: &mut Inner, msg: &Value) {
        let now = std::time::Instant::now();
        let frames = msg["frames"].as_u64().unwrap_or(0);
        let beat = msg["reason"].as_str() == Some("beat");
        let prev = g.page.take();
        let beat_frames = prev.as_ref().and_then(|p| p.beat_frames);
        let moving = match (beat, beat_frames) {
            // Between two beats a second apart: did any frame run?
            (true, Some(b)) => frames > b,
            // The first beat, or a change report: any progress counts, and
            // otherwise the last beat's verdict stands.
            (_, b) => b.is_none_or(|b| frames > b) || prev.as_ref().is_some_and(|p| p.moving),
        };
        g.page = Some(PageState {
            at: now,
            frames,
            beat_frames: if beat { Some(frames) } else { beat_frames },
            moving,
            visibility: msg["visibility"].as_str().unwrap_or("visible").to_string(),
            host: if msg["host"].is_object() { msg["host"].clone() } else { Value::Null },
        });
        // Stall watchdog (10.4): a patch the page hasn't taken in 15 s while
        // it kept running frames is stuck; reload the page so it loads the
        // patch afresh, and after a second stall drop the patch.
        let Some(w) = g.code.waiting.as_mut() else { return };
        let Some(sent_frames) = w.frames else {
            w.frames = Some(frames);
            w.at = now;
            return;
        };
        if now.saturating_duration_since(w.at) < PATCH_STALL || frames < sent_frames + PATCH_STALL_FRAMES {
            return;
        }
        let seq = w.seq;
        g.code.waiting = None;
        let again = g.code.stalled.contains(&seq);
        let reason = if again {
            g.code.mark_bad(seq);
            "A code change got stuck loading twice, so the game restarts without it (the last code that worked)."
        } else {
            g.code.stalled.push(seq);
            "A code change got stuck loading, so the game restarts to load it afresh."
        };
        self.stalled_reload(g, seq, reason);
    }

    /// Reload the page after a stuck patch, keeping the good patches (the
    /// reloaded page gets the newest one again with its `hello`).
    fn stalled_reload(&self, g: &mut Inner, seq: u64, reason: &str) {
        let at = now_ms();
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(json!({ "type": "code_reload", "at": at, "reason": reason }).to_string());
        }
        let ev = json!({ "type": "event", "v": 1, "event": "code.stalled", "data": { "at": at, "seq": seq, "reason": reason } });
        broadcast(&g.ctls, &ev.to_string());
        let news = json!({ "type": "live_swap_failed", "at": at, "build": 0, "kind": "code",
                           "message": "needs a fresh start", "reason": tab_text(reason) });
        broadcast(&g.devs, &news.to_string());
        g.code.remember(json!({ "state": "stalled", "at": at, "seq": seq, "message": reason }));
    }

    /// A look-guard event for every controller and the Evani.dev tab (15.7).
    pub fn look_event(&self, event: &str, data: Value) {
        let msg = json!({ "type": "event", "v": 1, "event": event, "data": data }).to_string();
        let g = self.lock();
        broadcast(&g.ctls, &msg);
        broadcast(&g.devs, &msg);
    }

    /// `model.convert {path, id?}` (its `note` goes into the job record) and
    /// `model.report {hash?}` (16).
    async fn convert_command(&self, cmd: &str, params: &Value, note: Option<&str>, original: &Value) -> Value {
        let conv = self.convert.clone();
        let root = std::path::PathBuf::from(&self.info.root);
        let hash = params["hash"].as_str().map(String::from);
        let id = params["id"].as_str().map(String::from);
        let path = params["path"].as_str().map(String::from);
        let note = note.map(String::from);
        let is_convert = cmd == "model.convert";
        if is_convert && path.is_none() {
            return proto::error(original, "bad_request", "model.convert needs params.path (a .glb, .gltf, .zip or .blend file)", None);
        }
        let answer = tokio::task::spawn_blocking(move || {
            if is_convert {
                let p = std::path::PathBuf::from(path.unwrap_or_default());
                let p = if p.is_absolute() { p } else { root.join(p) };
                conv.start_path(&p, id.as_deref(), note.as_deref())
            } else {
                conv.report(hash.as_deref())
            }
        })
        .await;
        match answer {
            Ok(Ok(result)) => json!({ "type": "result", "v": 1, "id": original, "ok": true, "result": result }),
            Ok(Err(r)) => proto::error(original, r.code, &r.message, None),
            Err(e) => proto::error(original, "internal", &format!("{cmd} stopped: {e}"), None),
        }
    }

    /// A convert job's event (`convert_progress`, `convert_done`, 16) for
    /// every controller, the Evani.dev tab and the page; a finished job
    /// also makes one news line for the tab.
    pub fn convert_event(&self, event: &str, data: Value) {
        let msg = json!({ "type": "event", "v": 1, "event": event, "data": data }).to_string();
        let g = self.lock();
        broadcast(&g.ctls, &msg);
        broadcast(&g.devs, &msg);
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(msg);
        }
        if event == "convert_done" {
            let id = data["id"].as_str().unwrap_or("model");
            let message = match (data["ok"] == true, data["converted"] == true) {
                (true, _) => format!("Converted {id}: every check passed"),
                (false, true) => format!("Converted {id}, with problems (see its report)"),
                (false, false) => format!("{id} didn't convert (see its report)"),
            };
            let news = json!({ "type": "live_status", "at": now_ms(), "state": "applied", "kind": "convert", "message": message });
            broadcast(&g.devs, &news.to_string());
        }
    }

    /// Bookkeeping on a successful result: subscription owners, the active
    /// tier, and the one-line news for the Evani.dev tab (3.4).
    fn after_result(&self, conn: u64, source: &str, cmd: &str, res: &Value) {
        if res["ok"] != true {
            return;
        }
        let result = &res["result"];
        let mut g = self.lock();
        match cmd {
            "telemetry.subscribe" => {
                if let Some(sub) = result["sub"].as_str() {
                    g.subs.insert(sub.to_string(), conn);
                }
            }
            "telemetry.unsubscribe" => {
                if let Some(sub) = result["sub"].as_str() {
                    g.subs.remove(sub);
                }
            }
            _ => {}
        }
        if let Some(t) = result["tier"]["active"].as_str() {
            g.tier = Some(t.to_string());
        }
        if MUTATING.contains(&cmd) {
            let news = json!({ "type": "live_status", "at": now_ms(), "state": "applied", "kind": "live",
                               "message": change_line(cmd, result, source) });
            broadcast(&g.devs, &news.to_string());
        }
    }

    /// A text frame from the page.
    pub fn on_engine_message(&self, engine: u64, text: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let mut g = self.lock();
        if !g.engine.as_ref().is_some_and(|(e, _)| *e == engine) {
            return;
        }
        match msg["type"].as_str() {
            Some("result") => {
                if let Some(id) = msg["id"].as_str()
                    && let Some(p) = g.pending.remove(id)
                {
                    let _ = p.reply.send(msg);
                }
            }
            Some("hello") => {
                if let Some(t) = msg["tier"].as_str() {
                    g.tier = Some(t.to_string());
                }
                g.page_rev = msg["page_rev"].as_str().map(String::from);
                // A page that takes code patches but runs older code (a
                // reload, a new tab): bring it up to the newest good patch.
                if msg["code"]["hot"] == true {
                    let has = msg["code"]["seq"].as_u64().unwrap_or(0);
                    let mut sent = None;
                    if let Some((seq, patch)) = g.code.good.last()
                        && *seq > has
                        && let Some((_, eng)) = &g.engine
                    {
                        let _ = eng.send(patch.to_string());
                        sent = Some(*seq);
                    }
                    if let Some(seq) = sent {
                        g.patch_sent(seq);
                    }
                }
                g.page_hello = Some(msg);
            }
            Some("page_state") => self.on_page_state(&mut g, &msg),
            Some("event") => self.route_event(&mut g, msg),
            _ => {}
        }
    }

    fn route_event(&self, g: &mut Inner, msg: Value) {
        let event = msg["event"].as_str().unwrap_or("");
        if event == "session.mirror" {
            g.mirror = Some(msg["data"].clone());
            // Always-on for /ctl clients (no subscription needed), so the tab
            // sees each committed change within a frame instead of polling.
            broadcast(&g.ctls, &msg.to_string());
            return;
        }
        if event == "page.patched" {
            let d = &msg["data"];
            if d["ok"] == true
                && let Some(rev) = d["rev"].as_str()
            {
                g.page_rev = Some(rev.to_string());
            }
            if let Some(seq) = d["seq"].as_u64()
                && let Some(reply) = g.page_patches.remove(&seq)
            {
                let _ = reply.send(d.clone());
            }
            broadcast(&g.ctls, &msg.to_string());
            return;
        }
        if let Some(kind) = event.strip_prefix("code.") {
            self.code_event(g, kind, &msg["data"]);
            broadcast(&g.ctls, &msg.to_string());
            return;
        }
        if let Some(sub) = msg["sub"].as_str() {
            if let Some(conn) = g.subs.get(sub)
                && let Some(out) = g.ctls.get(conn)
            {
                let _ = out.send(msg.to_string());
            }
            return;
        }
        if event == "auto_revert" {
            let d = &msg["data"];
            let message = format!(
                "Change #{} undone: {}",
                d["tx"].as_u64().unwrap_or(0),
                match d["reason"].as_str() {
                    Some("device_lost") => "the GPU device was lost".to_string(),
                    _ => format!(
                        "frame time stayed {}% over the {} ms target for {} s",
                        d["detail"]["over_pct"], d["detail"]["target_ms"], d["detail"]["sustain_s"]
                    ),
                }
            );
            let news = json!({ "type": "live_status", "at": now_ms(), "state": "reverted", "kind": "live", "message": message });
            broadcast(&g.devs, &news.to_string());
        }
        // Model mode: the page's `view_*` events go only to `/ctl` clients
        // that sent `view.watch` (EVANI_MODEL_TAB.md 5.1).
        if self.models.is_some() && event.starts_with("view_") {
            let text = msg.to_string();
            for c in &g.watchers {
                if let Some(out) = g.ctls.get(c) {
                    let _ = out.send(text.clone());
                }
            }
            return;
        }
        broadcast(&g.ctls, &msg.to_string());
    }

    /// Model mode: the workspace watcher's events (`model_version`,
    /// `model_status`) go to every `/ctl` client (5.4).
    pub fn model_event(&self, event: &str, data: Value) {
        let msg = json!({ "type": "event", "v": 1, "event": event, "data": data }).to_string();
        broadcast(&self.lock().ctls, &msg);
    }

    /// A text frame from the Evani.dev tab: only `chat_reply` goes on (to the page).
    pub fn on_dev_message(&self, text: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(text) else {
            return;
        };
        if msg["type"].as_str() != Some("chat_reply") {
            return;
        }
        let g = self.lock();
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(msg.to_string());
        }
    }

    /// News for the Evani.dev tab (build state changes).
    pub fn dev_news(&self, msg: &Value) {
        let g = self.lock();
        broadcast(&g.devs, &msg.to_string());
    }

    // ---- page hot-replace (14) ----------------------------------------------------

    /// The open page's rev tag: `None` when no page is connected,
    /// `Some(None)` when it is but didn't say (a page from before 14).
    pub fn page_rev(&self) -> Option<Option<String>> {
        let g = self.lock();
        g.engine.as_ref().map(|_| g.page_rev.clone())
    }

    /// Send a `page_patch` to the page; the receiver gets the `data` of its
    /// `page.patched` with the same seq. `None` when no page is connected.
    pub fn send_page_patch(&self, seq: u64, msg: &Value) -> Option<oneshot::Receiver<Value>> {
        let mut g = self.lock();
        let (_, eng) = g.engine.as_ref()?;
        if eng.send(msg.to_string()).is_err() {
            g.drop_engine();
            return None;
        }
        let (tx, rx) = oneshot::channel();
        g.page_patches.insert(seq, tx);
        Some(rx)
    }

    // ---- live code swapping (10.4) ------------------------------------------------------

    /// How code changes reach the page, for `/live/status` `code`.
    pub fn set_code_mode(&self, mode: Value) {
        self.lock().code.mode = mode;
    }

    /// The builder started a patch for a saved change.
    pub fn code_patching(&self, at: u64, files: &[String]) {
        let g = self.lock();
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(json!({ "type": "code_patching", "at": at, "files": files }).to_string());
        }
        let ev = json!({ "type": "event", "v": 1, "event": "code.patching", "data": { "at": at, "files": files } });
        broadcast(&g.ctls, &ev.to_string());
    }

    /// A patch is ready: number it, keep it and send it to the page.
    /// `table` is subsecond's jump table with `lib` already pointing at
    /// this bridge (`/wasm/<file>`). Returns its seq.
    pub fn code_patch(&self, table: Value, functions: usize, build_ms: u64, started_at: u64, files: &[String]) -> u64 {
        let mut g = self.lock();
        g.code.next_seq += 1;
        let seq = g.code.next_seq;
        let msg = json!({ "type": "code_patch", "seq": seq, "table": table, "functions": functions,
                          "build_ms": build_ms, "started_at": started_at, "files": files });
        g.code.good.push((seq, msg.clone()));
        if g.code.good.len() > CODE_KEEP {
            g.code.good.remove(0);
        }
        let sent = g.engine.as_ref().is_some_and(|(_, eng)| eng.send(msg.to_string()).is_ok());
        if sent {
            g.patch_sent(seq);
        }
        seq
    }

    /// A code change didn't compile: the page keeps running the old code.
    /// `message` is the compiler's text, `summary` one line for the tab.
    pub fn code_error(&self, message: &str, summary: &str) {
        let mut g = self.lock();
        let at = now_ms();
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(json!({ "type": "code_error", "at": at, "message": message }).to_string());
        }
        let ev = json!({ "type": "event", "v": 1, "event": "code.error", "data": { "at": at, "message": message } });
        broadcast(&g.ctls, &ev.to_string());
        let news = json!({ "type": "live_status", "at": at, "state": "error", "kind": "code", "message": tab_text(summary) });
        broadcast(&g.devs, &news.to_string());
        g.code.remember(json!({ "state": "error", "at": at, "message": summary }));
    }

    /// The builder made a whole new build (a change too big to patch): the
    /// page reloads, and the patches made for the old build are dropped.
    pub fn code_reload(&self, reason: &str) {
        let mut g = self.lock();
        let at = now_ms();
        g.code.good.clear();
        g.code.waiting = None;
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(json!({ "type": "code_reload", "at": at, "reason": reason }).to_string());
        }
        let ev = json!({ "type": "event", "v": 1, "event": "code.reload", "data": { "at": at, "reason": reason } });
        broadcast(&g.ctls, &ev.to_string());
        let news = json!({ "type": "live_swap_failed", "at": at, "build": 0, "kind": "code",
                           "message": "needs a fresh start", "reason": tab_text(reason) });
        broadcast(&g.devs, &news.to_string());
        g.code.remember(json!({ "state": "reload", "at": at, "message": reason }));
    }

    /// `code.patched` / `code.failed` / `code.crashed` from the page.
    fn code_event(&self, g: &mut Inner, kind: &str, d: &Value) {
        let at = now_ms();
        let seq = d["seq"].as_u64().unwrap_or(0);
        if matches!(kind, "patched" | "failed" | "crashed") && g.code.waiting.is_some_and(|w| seq >= w.seq || seq == 0) {
            g.code.waiting = None;
        }
        match kind {
            "patched" => {
                let functions = d["functions"].as_u64().unwrap_or(0);
                let secs = d["ms"].as_f64().unwrap_or(0.0) / 1000.0;
                let s = if functions == 1 { "" } else { "s" };
                let message = format!("Code updated: {functions} function{s}, {secs:.1} s, no reload");
                let news = json!({ "type": "live_status", "at": at, "state": "applied", "kind": "code", "message": message });
                broadcast(&g.devs, &news.to_string());
                g.code.remember(json!({ "state": "patched", "at": at, "seq": seq, "functions": functions,
                                        "ms": d["ms"], "message": message }));
            }
            "failed" | "crashed" => {
                g.code.mark_bad(seq);
                let detail = d["message"].as_str().unwrap_or("");
                let reason = if kind == "failed" {
                    format!("The code change could not be loaded; the game kept the old code. {detail}")
                } else {
                    format!("The game stopped in the changed code and reloaded with the last code that worked. {detail}")
                };
                let news = json!({ "type": "live_swap_failed", "at": at, "build": 0, "kind": "code",
                                   "message": "needs a fresh start", "reason": tab_text(&reason) });
                broadcast(&g.devs, &news.to_string());
                g.code.remember(json!({ "state": kind, "at": at, "seq": seq, "message": tab_text(&reason) }));
            }
            _ => {}
        }
    }

    /// A one-way message to the page (e.g. `live_status` `page_changed`).
    pub fn to_engine(&self, msg: &Value) {
        let g = self.lock();
        if let Some((_, eng)) = &g.engine {
            let _ = eng.send(msg.to_string());
        }
    }
}

/// The tab shows `message` up to 300 characters (3.4): one trimmed line.
fn tab_text(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= 300 {
        return one;
    }
    let mut out: String = one.chars().take(299).collect();
    out.push('…');
    out
}

fn broadcast(to: &HashMap<u64, Outbox>, text: &str) {
    for out in to.values() {
        let _ = out.send(text.to_string());
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// One toolbar line for a committed change, e.g.
/// `fx.bloom.threshold 1 â†’ 1.5 (AI)`. Never says "error" or "failed to build"
/// (3.4: the tab would read it as a broken build).
pub fn change_line(cmd: &str, result: &Value, source: &str) -> String {
    let who = match source {
        "mcp" => "AI",
        "page" => "you",
        _ => "script",
    };
    // Edit transactions (6.31): a staged, held or not-applied edit says so.
    if let Some(txn) = result["txn"].as_str() {
        let edit = result["edit"].as_str().unwrap_or(txn);
        match result["state"].as_str() {
            Some("staged") => return format!("{edit} staged in {txn}, shows on commit ({who})"),
            Some("failed") => return format!("{edit} in {txn} not applied ({who})"),
            Some("conflict") => {
                return format!("{edit} in {txn} clashes with another edit; waiting for your choice ({who})");
            }
            _ if result["applied"].as_array().is_none_or(Vec::is_empty) => {
                return format!("{txn} {} ({who})", result["state"].as_str().unwrap_or("done"));
            }
            _ => {}
        }
    }
    let applied = result["applied"].as_array().map_or(&[][..], Vec::as_slice);
    let short = |v: &Value| match v {
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |f| format!("{}", (f * 1000.0).round() / 1000.0)),
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    };
    let first = applied.first().map(|a| {
        let name = a["name"].as_str().unwrap_or("?");
        if a.get("old").is_some() || a.get("new").is_some() {
            format!("{name} {} â†’ {}", short(&a["old"]), short(&a["new"]))
        } else {
            format!("{name} changed")
        }
    });
    let line = match (first, applied.len()) {
        (Some(f), 1) => f,
        (Some(f), n) => format!("{f} and {} more", n - 1),
        (None, _) => format!("{cmd} done"),
    };
    format!("{line} ({who})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn hub(timeout_ms: u64) -> Arc<Hub> {
        Arc::new(Hub::with_timeout(
            HubInfo { port: 8096, root: "C:\\x".into(), game: "sandbox".into(), session: "s-1".into() },
            Duration::from_millis(timeout_ms),
        ))
    }

    fn cmd(id: &str, name: &str) -> Value {
        json!({"type":"cmd","v":1,"id":id,"cmd":name,"params":{},"opts":{"telemetry_ms":0}})
    }

    /// A fake page: answers every command by echoing its cmd and source.
    fn fake_page(h: &Arc<Hub>) -> u64 {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let id = h.attach_engine(tx);
        let h2 = h.clone();
        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                let m: Value = serde_json::from_str(&text).unwrap();
                if m["type"] != "cmd" || m["cmd"] == "slow" {
                    continue;
                }
                let reply = if m["cmd"] == "param.set" {
                    json!({"type":"result","v":1,"id":m["id"],"ok":true,
                           "result":{"applied":[{"kind":"param","name":"fx.bloom.threshold","old":1.0,"new":1.5}]}})
                } else if m["cmd"] == "telemetry.subscribe" {
                    json!({"type":"result","v":1,"id":m["id"],"ok":true,"result":{"sub":"t-1","rate_hz":4}})
                } else {
                    json!({"type":"result","v":1,"id":m["id"],"ok":true,
                           "result":{"cmd":m["cmd"],"source":m["source"],"bridge_id":m["id"]}})
                };
                h2.on_engine_message(id, &reply.to_string());
            }
        });
        id
    }

    #[tokio::test]
    async fn view_events_reach_only_watchers_in_model_mode() {
        let ws = crate::models::Workspace::new(std::env::temp_dir().join("evani-no-such-models"), ".evani/model".into());
        let h = Arc::new(
            Hub::with_timeout(HubInfo { port: 8097, root: "C:\\x".into(), game: "model-view".into(), session: "s-1".into() }, Duration::from_millis(500))
                .with_models(ws),
        );
        let (eng_tx, _eng_rx) = mpsc::unbounded_channel::<String>();
        let eng = h.attach_engine(eng_tx);
        let (a_tx, mut a_rx) = mpsc::unbounded_channel::<String>();
        let (b_tx, mut b_rx) = mpsc::unbounded_channel::<String>();
        let a = h.attach_ctl(a_tx);
        let _b = h.attach_ctl(b_tx);
        let r = h.command(a, "ctl", cmd("w", "view.watch")).await;
        assert_eq!(r["result"]["watching"], true);
        let ev = json!({ "type": "event", "v": 1, "event": "view_loaded", "data": { "model": "kart-06b3" } });
        h.on_engine_message(eng, &ev.to_string());
        assert!(a_rx.try_recv().unwrap().contains("view_loaded"));
        assert!(b_rx.try_recv().is_err(), "a client that did not watch gets no view_* events");
        // The bridge's own events go to everyone.
        h.model_event("model_version", json!({ "model": "kart-06b3", "version": 2 }));
        assert!(a_rx.try_recv().unwrap().contains("model_version"));
        assert!(b_rx.try_recv().unwrap().contains("model_version"));
        // A watcher that leaves is forgotten.
        h.detach_ctl(a);
        assert!(h.lock().watchers.is_empty());
    }

    #[tokio::test]
    async fn an_outside_game_gets_its_page_commands_and_a_clear_no_for_the_rest() {
        let h = Arc::new(
            Hub::with_timeout(HubInfo { port: 8099, root: "C:\\e".into(), game: "sea".into(), session: "s-1".into() }, Duration::from_millis(2000))
                .with_outside("C:\\g\\app".into()),
        );
        fake_page(&h);
        // The page's own live commands go through as for any game.
        let r = h.command(1, "ctl", cmd("c-1", "capture.frame")).await;
        assert_eq!(r["result"]["cmd"], "capture.frame");
        // The bridge's in-tree features answer "not available", never a hang or an odd failure.
        for c in ["look.status", "model.convert", "model.load", "view.watch"] {
            let r = h.command(1, "ctl", cmd("c-2", c)).await;
            assert_eq!(r["error"]["code"], "not_available", "{c}: {r}");
            assert!(r["error"]["message"].as_str().unwrap().contains("not available for --app-dir yet"), "{c}: {r}");
            assert_eq!(r["id"], "c-2");
        }
        assert_eq!(h.status()["app_dir"], "C:\\g\\app");
        assert!(hub(100).status().get("app_dir").is_none(), "absent for a game in apps/");
    }

    #[tokio::test]
    async fn no_page_fails_fast() {
        let h = hub(5000);
        let t = std::time::Instant::now();
        let r = h.command(1, "ctl", cmd("c-1", "schema.get")).await;
        assert_eq!(r["error"]["code"], "no_engine");
        assert_eq!(r["id"], "c-1");
        assert!(t.elapsed() < Duration::from_millis(500));
    }

    #[tokio::test]
    async fn results_route_back_with_original_ids() {
        let h = hub(5000);
        fake_page(&h);
        let (a, b) = tokio::join!(
            h.command(1, "mcp", cmd("same", "schema.get")),
            h.command(2, "ctl", cmd("same", "param.get"))
        );
        assert_eq!(a["id"], "same");
        assert_eq!(b["id"], "same");
        assert_eq!(a["result"]["cmd"], "schema.get");
        assert_eq!(b["result"]["cmd"], "param.get");
        assert_eq!(a["result"]["source"], "mcp");
        assert_eq!(b["result"]["source"], "ctl");
        // The page saw bridge ids, never the controllers' ids.
        assert_ne!(a["result"]["bridge_id"], b["result"]["bridge_id"]);
        assert!(a["result"]["bridge_id"].as_str().unwrap().starts_with('b'));
    }

    #[tokio::test]
    async fn bad_envelopes_never_reach_the_page() {
        let h = hub(5000);
        fake_page(&h);
        let r = h.command(1, "ctl", json!({"type":"cmd","v":2,"id":"x","cmd":"schema.get"})).await;
        assert_eq!(r["error"]["code"], "version_mismatch");
        let r = h.command(1, "ctl", json!({"type":"cmd","v":1,"cmd":"schema.get"})).await;
        assert_eq!(r["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn slow_page_times_out_and_late_answer_is_dropped() {
        let h = hub(50);
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let eng = h.attach_engine(tx);
        let r = h.command(1, "ctl", cmd("c-9", "slow")).await;
        assert_eq!(r["error"]["code"], "timeout");
        assert_eq!(r["id"], "c-9");
        // The page answers late: ignored, nothing pending.
        let sent: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        h.on_engine_message(eng, &json!({"type":"result","v":1,"id":sent["id"],"ok":true,"result":{}}).to_string());
        assert!(h.lock().pending.is_empty());
    }

    #[tokio::test]
    async fn page_leaving_fails_waiting_commands() {
        let h = hub(5000);
        let (tx, _rx) = mpsc::unbounded_channel::<String>();
        let eng = h.attach_engine(tx);
        let h2 = h.clone();
        let waiting = tokio::spawn(async move { h2.command(1, "ctl", cmd("c-2", "schema.get")).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        h.detach_engine(eng);
        let r = waiting.await.unwrap();
        assert_eq!(r["error"]["code"], "no_engine");
        assert!(!h.engine_connected());
    }

    #[tokio::test]
    async fn newer_page_replaces_older() {
        let h = hub(5000);
        let (tx1, mut rx1) = mpsc::unbounded_channel::<String>();
        let first = h.attach_engine(tx1);
        let second = fake_page(&h);
        assert_ne!(first, second);
        // The old page's outbox is closed, so its socket task ends.
        assert!(rx1.recv().await.is_none());
        // Messages from the old page are ignored.
        h.on_engine_message(first, r#"{"type":"hello","v":1,"tier":"low"}"#);
        assert_eq!(h.status()["tier"], Value::Null);
        let r = h.command(1, "ctl", cmd("c", "schema.get")).await;
        assert_eq!(r["ok"], true);
    }

    #[tokio::test]
    async fn events_go_to_subscription_owner_or_everyone() {
        let h = hub(5000);
        let eng = fake_page(&h);
        let (t1, mut r1) = mpsc::unbounded_channel::<String>();
        let (t2, mut r2) = mpsc::unbounded_channel::<String>();
        let (td, mut rd) = mpsc::unbounded_channel::<String>();
        let c1 = h.attach_ctl(t1);
        let _c2 = h.attach_ctl(t2);
        h.attach_dev(td);
        let r = h.command(c1, "ctl", cmd("s", "telemetry.subscribe")).await;
        assert_eq!(r["result"]["sub"], "t-1");
        h.on_engine_message(eng, r#"{"type":"event","v":1,"event":"telemetry","sub":"t-1","data":{"fps":60}}"#);
        assert!(r1.try_recv().unwrap().contains("\"fps\":60"));
        assert!(r2.try_recv().is_err());
        // Broadcast events reach every controller; auto_revert also makes tab news.
        h.on_engine_message(
            eng,
            r#"{"type":"event","v":1,"event":"auto_revert","data":{"tx":17,"reason":"frame_time","detail":{"over_pct":41,"target_ms":16.7,"sustain_s":3}}}"#,
        );
        assert!(r1.try_recv().unwrap().contains("auto_revert"));
        assert!(r2.try_recv().unwrap().contains("auto_revert"));
        let news: Value = serde_json::from_str(&rd.try_recv().unwrap()).unwrap();
        assert_eq!(news["state"], "reverted");
        assert_eq!(news["message"], "Change #17 undone: frame time stayed 41% over the 16.7 ms target for 3 s");
        // The session mirror is kept and forwarded to every controller, no subscription needed.
        h.on_engine_message(eng, r#"{"type":"event","v":1,"event":"session.mirror","data":{"head":3}}"#);
        for r in [&mut r1, &mut r2] {
            let m: Value = serde_json::from_str(&r.try_recv().unwrap()).unwrap();
            assert_eq!(m["event"], "session.mirror");
            assert_eq!(m["data"]["head"], 3);
        }
        assert_eq!(h.mirror().unwrap()["head"], 3);
    }

    #[tokio::test]
    async fn committed_changes_make_tab_news() {
        let h = hub(5000);
        fake_page(&h);
        let (td, mut rd) = mpsc::unbounded_channel::<String>();
        h.attach_dev(td);
        let mut req = cmd("p", "param.set");
        req["note"] = json!("why");
        let r = h.command(1, "mcp", req).await;
        assert_eq!(r["ok"], true);
        let news: Value = serde_json::from_str(&rd.try_recv().unwrap()).unwrap();
        assert_eq!(news["type"], "live_status");
        assert_eq!(news["state"], "applied");
        assert_eq!(news["kind"], "live");
        assert_eq!(news["message"], "fx.bloom.threshold 1 â†’ 1.5 (AI)");
        // Reads make no news.
        h.command(1, "mcp", cmd("g", "param.get")).await;
        assert!(rd.try_recv().is_err());
    }

    #[tokio::test]
    async fn tab_channel_only_forwards_chat_reply() {
        let h = hub(5000);
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        h.attach_engine(tx);
        h.on_dev_message(r#"{"type":"cmd","v":1,"id":"x","cmd":"param.set"}"#);
        h.on_dev_message("not json");
        assert!(rx.try_recv().is_err());
        h.on_dev_message(r#"{"type":"chat_reply","text":"hi","from":"evani-dev"}"#);
        assert!(rx.try_recv().unwrap().contains("chat_reply"));
    }

    #[tokio::test]
    async fn status_shape() {
        let h = hub(5000);
        let s = h.status();
        for k in ["type", "build", "url", "phase", "error", "build_ms", "relay", "engine", "protocol", "root", "game", "engine_connected", "tier", "look_guard"] {
            assert!(s.get(k).is_some(), "missing {k}");
        }
        assert_eq!(s["type"], "live_hello");
        assert_eq!(s["relay"], "ws://localhost:8096");
        assert_eq!(s["engine_connected"], false);
        let eng = fake_page(&h);
        h.on_engine_message(eng, r#"{"type":"hello","v":1,"schema_version":"abcd1234","tier":"medium"}"#);
        let s = h.status();
        assert_eq!(s["engine_connected"], true);
        assert_eq!(s["tier"], "medium");
        assert_eq!(h.ctl_hello()["schema_version"], "abcd1234");
    }

    #[test]
    fn change_lines() {
        let r = json!({"applied":[{"name":"fx.bloom.intensity","old":0.6,"new":0.9},{"name":"x","old":1,"new":2}]});
        assert_eq!(change_line("param.set", &r, "page"), "fx.bloom.intensity 0.6 â†’ 0.9 and 1 more (you)");
        assert_eq!(change_line("batch", &json!({}), "ctl"), "batch done (script)");
        // Edit transactions (6.31).
        let staged = json!({"txn":"edit-1","edit":"e2","state":"staged","applied":[{"name":"x","old":1,"new":2}]});
        assert_eq!(change_line("edit.stage", &staged, "mcp"), "e2 staged in edit-1, shows on commit (AI)");
        let clash = json!({"txn":"edit-1","edit":"e3","state":"conflict"});
        assert!(change_line("edit.stage", &clash, "mcp").contains("waiting for your choice"));
        let failed = json!({"txn":"edit-1","edit":"e1","state":"failed","error":{"code":"out_of_range"}});
        let line = change_line("edit.stage", &failed, "mcp");
        assert_eq!(line, "e1 in edit-1 not applied (AI)");
        assert!(!line.contains("error") && !line.contains("failed"));
        let applied = json!({"txn":"edit-1","edit":"e1","state":"applied","applied":[{"name":"x","old":1,"new":2}]});
        assert!(change_line("edit.stage", &applied, "mcp").starts_with("x 1 "));
        let closed = json!({"txn":"edit-1","state":"closed","applied":[]});
        assert_eq!(change_line("edit.commit", &closed, "mcp"), "edit-1 closed (AI)");
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(t) = rx.try_recv() {
            out.push(serde_json::from_str(&t).unwrap());
        }
        out
    }

    /// Live code swapping (10.4): patches go to the page, its answers make
    /// tab news, a bad patch is never re-sent, a reloaded page gets the
    /// newest good one, and a full rebuild drops them all.
    #[test]
    fn code_patches_flow_and_bad_ones_are_dropped() {
        let h = hub(5000);
        let (dev_tx, mut dev) = mpsc::unbounded_channel::<String>();
        h.attach_dev(dev_tx);
        let (ctl_tx, mut ctl) = mpsc::unbounded_channel::<String>();
        h.attach_ctl(ctl_tx);
        let (eng_tx, mut eng) = mpsc::unbounded_channel::<String>();
        let page = h.attach_engine(eng_tx);
        h.set_code_mode(json!({"mode": "hot", "builder": "dx"}));

        h.code_patching(5, &["lib.rs".into()]);
        assert_eq!(drain(&mut eng)[0]["type"], "code_patching");
        assert_eq!(drain(&mut ctl)[0]["event"], "code.patching");
        let s1 = h.code_patch(json!({"lib": "/wasm/p1.wasm"}), 412, 800, 5, &[]);
        let s2 = h.code_patch(json!({"lib": "/wasm/p2.wasm"}), 413, 700, 9, &[]);
        assert_eq!((s1, s2), (1, 2));
        let sent = drain(&mut eng);
        assert_eq!((sent[0]["type"].as_str(), sent[1]["seq"].as_u64()), (Some("code_patch"), Some(2)));

        h.on_engine_message(page, r#"{"type":"event","v":1,"event":"code.patched","data":{"seq":2,"functions":413,"ms":3600}}"#);
        let news = drain(&mut dev);
        assert_eq!(news[0]["state"], "applied");
        assert_eq!(news[0]["kind"], "code");
        assert_eq!(news[0]["message"], "Code updated: 413 functions, 3.6 s, no reload");
        assert_eq!(drain(&mut ctl)[0]["event"], "code.patched");
        assert_eq!(h.status()["code"]["seq"], 2);

        // Patch 2 crashed the page: it reloads and says hello; it gets patch 1.
        h.on_engine_message(page, r#"{"type":"event","v":1,"event":"code.crashed","data":{"seq":2,"message":"boom"}}"#);
        assert_eq!(drain(&mut dev)[0]["type"], "live_swap_failed");
        let (eng_tx, mut eng) = mpsc::unbounded_channel::<String>();
        let page = h.attach_engine(eng_tx);
        h.on_engine_message(page, r#"{"type":"hello","v":1,"code":{"hot":true,"seq":0}}"#);
        let sent = drain(&mut eng);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["seq"], 1);
        // A page already on patch 1, or one without the code module, gets nothing.
        h.on_engine_message(page, r#"{"type":"hello","v":1,"code":{"hot":true,"seq":1}}"#);
        h.on_engine_message(page, r#"{"type":"hello","v":1}"#);
        assert!(drain(&mut eng).is_empty());

        // A compile error: page, controllers and tab hear it; nothing is dropped.
        h.code_error("error[E0308]: mismatched types\n --> lib.rs:580", "error[E0308]: mismatched types (lib.rs:580)");
        assert_eq!(drain(&mut eng)[0]["type"], "code_error");
        let news = drain(&mut dev);
        assert_eq!((news[0]["state"].as_str(), news[0]["kind"].as_str()), (Some("error"), Some("code")));
        assert_eq!(h.status()["code"]["seq"], 1);

        // A full rebuild drops the old build's patches.
        h.code_reload("Cargo.toml changed");
        assert_eq!(drain(&mut eng)[0]["type"], "code_reload");
        assert_eq!(drain(&mut dev)[0]["message"], "needs a fresh start");
        h.on_engine_message(page, r#"{"type":"hello","v":1,"code":{"hot":true,"seq":0}}"#);
        assert!(drain(&mut eng).is_empty());
        let st = h.status();
        assert_eq!(st["code"]["mode"], "hot");
        assert_eq!(st["code"]["seq"], 0);
        let states: Vec<&str> = st["code"]["history"].as_array().unwrap().iter().map(|e| e["state"].as_str().unwrap()).collect();
        assert_eq!(states, ["patched", "crashed", "error", "reload"]);
        assert_eq!(tab_text(&"x ".repeat(400)).chars().count(), 300);
    }

    fn state(h: &Hub, page: u64, reason: &str, frames: u64, visibility: &str, host: Value) {
        let m = json!({"type":"page_state","v":1,"reason":reason,"frames":frames,"visibility":visibility,"host":host});
        h.on_engine_message(page, &m.to_string());
    }

    #[tokio::test]
    async fn paused_page_answers_at_once() {
        let h = hub(5000);
        let page = fake_page(&h);
        assert_eq!(h.status()["page"]["state"], "unknown");
        // Visible and running: commands go through.
        state(&h, page, "beat", 10, "visible", Value::Null);
        state(&h, page, "beat", 70, "visible", Value::Null);
        assert_eq!(h.status()["page"]["state"], "running");
        assert_eq!(h.command(1, "mcp", cmd("a", "schema.get")).await["ok"], true);
        // Hidden but still running frames (Evani.App's parked page): answers.
        state(&h, page, "visibility", 75, "hidden", Value::Null);
        assert_eq!(h.status()["page"]["state"], "running");
        state(&h, page, "beat", 75, "hidden", Value::Null);
        assert_eq!(h.command(1, "mcp", cmd("b", "schema.get")).await["ok"], true);
        // Hidden and the frame counter stood still for a beat: paused, at once,
        // and the command is never sent.
        state(&h, page, "beat", 75, "hidden", Value::Null);
        let t = std::time::Instant::now();
        let r = h.command(1, "mcp", cmd("c", "session.status")).await;
        assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
        assert_eq!(r["id"], "c");
        assert_eq!(r["error"]["code"], "paused");
        assert!(r["error"]["message"].as_str().unwrap().starts_with("paused (tab hidden): "), "{r}");
        assert_eq!(r["error"]["data"]["reason"], "paused (tab hidden)");
        assert!(r["error"]["data"]["fix"].as_str().unwrap().contains("Show the Evani.dev tab"));
        let st = h.status();
        assert_eq!((st["page"]["state"].as_str(), st["page"]["reason"].as_str()), (Some("paused"), Some("paused (tab hidden)")));
        // Evani.App's own reason wins; showing the tab again clears it.
        state(&h, page, "host", 75, "hidden", json!({"v":1,"shown":false,"paused":"paused (window minimised)"}));
        assert_eq!(h.command(1, "mcp", cmd("d", "x")).await["error"]["data"]["reason"], "paused (window minimised)");
        state(&h, page, "host", 76, "visible", json!({"v":1,"shown":true,"paused":null}));
        assert_eq!(h.command(1, "mcp", cmd("e", "schema.get")).await["ok"], true);
    }

    #[tokio::test]
    async fn waiting_command_ends_when_the_page_pauses() {
        let h = hub(5000);
        let page = fake_page(&h);
        state(&h, page, "beat", 10, "visible", Value::Null);
        let h2 = h.clone();
        let waiting = tokio::spawn(async move { h2.command(1, "mcp", cmd("s", "slow")).await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        state(&h, page, "visibility", 10, "hidden", Value::Null);
        state(&h, page, "beat", 10, "hidden", Value::Null);
        let t = std::time::Instant::now();
        let r = waiting.await.unwrap();
        assert!(t.elapsed() < Duration::from_millis(400), "{:?}", t.elapsed());
        assert_eq!(r["error"]["code"], "paused");
        assert!(r["error"]["message"].as_str().unwrap().contains("already sent"));
    }

    #[test]
    fn silent_hidden_page_counts_as_paused() {
        let now = std::time::Instant::now();
        let mut p = PageState { at: now, frames: 5, beat_frames: Some(1), moving: true,
                                visibility: "hidden".into(), host: Value::Null };
        assert_eq!(p.paused(now), None, "still running frames");
        assert_eq!(p.paused(now + Duration::from_secs(3)).as_deref(), Some("paused (tab hidden)"), "silent past 2.5 s");
        p.visibility = "visible".into();
        assert_eq!(p.paused(now + Duration::from_secs(30)), None, "silent but visible: a long task, not a pause");
        p.moving = false;
        assert_eq!(p.paused(now), None, "no reason, no pause");
    }

    #[test]
    fn stuck_patch_reloads_then_is_dropped() {
        let h = hub(5000);
        let (eng_tx, mut eng) = mpsc::unbounded_channel::<String>();
        let page = h.attach_engine(eng_tx);
        let (dev_tx, mut dev) = mpsc::unbounded_channel::<String>();
        h.attach_dev(dev_tx);
        state(&h, page, "beat", 100, "visible", Value::Null);
        let seq = h.code_patch(json!({"lib":"/wasm/p.wasm"}), 3, 900, now_ms(), &[]);
        assert_eq!(drain(&mut eng)[0]["type"], "code_patch");
        let age = |h: &Hub| {
            if let Some(w) = h.lock().code.waiting.as_mut() {
                w.at -= PATCH_STALL + Duration::from_secs(1);
            }
        };
        // Old enough but no frames ran (a parked page): not stuck.
        age(&h);
        state(&h, page, "beat", 120, "visible", Value::Null);
        assert!(drain(&mut eng).is_empty());
        // Frames ran and the patch never landed: reload, keep the patch.
        state(&h, page, "beat", 400, "visible", Value::Null);
        let sent = drain(&mut eng);
        assert_eq!(sent[0]["type"], "code_reload");
        assert_eq!(drain(&mut dev)[0]["type"], "live_swap_failed");
        h.on_engine_message(page, r#"{"type":"hello","v":1,"code":{"hot":true,"seq":0}}"#);
        assert_eq!(drain(&mut eng)[0]["seq"], seq, "the reloaded page gets it again");
        // Stuck a second time: dropped, and the next page doesn't get it.
        state(&h, page, "beat", 400, "visible", Value::Null);
        age(&h);
        state(&h, page, "beat", 900, "visible", Value::Null);
        assert_eq!(drain(&mut eng)[0]["type"], "code_reload");
        h.on_engine_message(page, r#"{"type":"hello","v":1,"code":{"hot":true,"seq":0}}"#);
        assert!(drain(&mut eng).is_empty());
        let states: Vec<String> = h.status()["code"]["history"].as_array().unwrap().iter()
            .map(|e| e["state"].as_str().unwrap().to_string()).collect();
        assert_eq!(states, ["stalled", "stalled"]);
        // A patch the page reports applied stops the watchdog.
        let seq2 = h.code_patch(json!({"lib":"/wasm/q.wasm"}), 1, 500, now_ms(), &[]);
        h.on_engine_message(page, &json!({"type":"event","v":1,"event":"code.patched","data":{"seq":seq2,"functions":1,"ms":800}}).to_string());
        assert!(h.lock().code.waiting.is_none());
    }
}
