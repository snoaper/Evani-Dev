//! `mcp`: an MCP server on stdio (docs/LIVE_API.md 11). Each tool is one
//! command sent to the running bridge over `/ctl` with the token from
//! `<root>/.evani/live/session.json`. Logs go to stderr only (stdout is the
//! MCP channel).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::proto::{self, MAX_MESSAGE};
use crate::session;
use crate::tools::{self, Kind, ToolSpec};

pub const NOT_RUNNING: &str =
    "The live server isn't running. Open the Evani.dev tab with this engine's folder, or run `cargo xtask live`.";

const INSTRUCTIONS: &str = "Drive the running Evani.Dev game live. Call live_status first, then schema_get to see every \
tunable. Give every change a short note saying why. Check capture_frame after visual changes. Bake (bake_export) \
only when asked, dry_run first. To choose a value, don't loop param_set + capture_frame + telemetry_sample per try: \
run one sweep_run (a list of values, or a small grid) and read its table of costs and look measures, then apply the \
chosen values in one param_set (several names) or one batch. Use compare_ab to measure a change against the current \
state before keeping it. sweep_run and compare_ab try values off-screen: the game's view holds its last frame \
(marked \"measuring\") and only what you apply afterwards shows, so the person watching never sees the tries. While \
bench_run, compare_ab or sweep_run runs, changes answer busy with until_ms; wait and \
retry. When one request asks for several live changes (\"turn the lights down, make the sky blue, reverse the \
spin\"), don't do them one after another: split it into independent atomic edits, call edit_begin with that list, \
then run one sub-agent per edit in parallel (no fixed cap: 3, 6 or 20 edits means 3, 6 or 20 agents), each calling \
edit_stage for its own edit with its own note, and call edit_commit when all are back. Mode stream (the default) \
shows each edit as soon as it is staged; together shows them all on one frame at commit. Each edit answers applied, \
failed (with why; the others carry on) or conflict: two edits change the same thing, neither stays applied, and you \
must ask the user which one wins, then call edit_resolve. Never pick the winner yourself. evani_check (no game \
needed) is the project's gate: run it before handing over and fix every ERROR until it says clean.";

/// Kept events per kind, so a long silence can't grow memory.
const MAX_EVENTS: usize = 50;
const MAX_SAMPLES: usize = 2000;

pub async fn run(root: PathBuf) -> Result<(), String> {
    let server = LiveMcp::new(root);
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| format!("MCP start: {e}"))?;
    running.waiting().await.map_err(|e| format!("MCP: {e}"))?;
    Ok(())
}

/// What the `/ctl` reader collects between tool calls.
#[derive(Default)]
struct Inbox {
    /// Broadcast events (auto_revert, device loss…) since the last tool result.
    events: Vec<Value>,
    /// Telemetry events by subscription id.
    telemetry: HashMap<String, Vec<Value>>,
}

struct Conn {
    /// The token it was opened with (a restarted bridge has a new one).
    token: String,
    out: mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    alive: Arc<AtomicBool>,
}

pub struct LiveMcp {
    root: PathBuf,
    /// The open `/ctl` connection and the session file it came from (one
    /// at a time; a tool naming another session reconnects, LIVE_API.md 2.6).
    conn: tokio::sync::Mutex<Option<(PathBuf, Conn)>>,
    inbox: Arc<Mutex<Inbox>>,
    next: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl LiveMcp {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            conn: tokio::sync::Mutex::new(None),
            inbox: Arc::new(Mutex::new(Inbox::default())),
            next: AtomicU64::new(0),
        }
    }

    fn next_id(&self) -> String {
        format!("m-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Open `/ctl` with the token from the session file `file`.
    async fn connect(&self, file: &Path) -> Result<Conn, String> {
        let s = session::read_at(file).map_err(|_| NOT_RUNNING.to_string())?;
        let mut req = format!("{}/ctl", s.ws)
            .into_client_request()
            .map_err(|e| format!("session.json has a bad ws address: {e}"))?;
        let h = req.headers_mut();
        h.insert(
            "Authorization",
            format!("Bearer {}", s.token)
                .parse()
                .map_err(|_| "session.json has a bad token".to_string())?,
        );
        h.insert("X-Evani-Client", tokio_tungstenite::tungstenite::http::HeaderValue::from_static("mcp"));
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let connecting = tokio_tungstenite::connect_async_with_config(req, Some(config), false);
        let (ws, _) = match tokio::time::timeout(Duration::from_secs(5), connecting).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) if e.to_string().contains("401") => {
                return Err(format!(
                    "unauthorized: the live server refused the token in {}; it may have just restarted, try again.",
                    file.display()
                ));
            }
            _ => return Err(NOT_RUNNING.to_string()),
        };
        let (mut sink, mut stream) = ws.split();
        let (out, mut rx) = mpsc::unbounded_channel::<String>();
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>> = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if sink.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let (p, a, inbox) = (pending.clone(), alive.clone(), self.inbox.clone());
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(t) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                match v["type"].as_str() {
                    Some("result") => {
                        let tx = v["id"].as_str().and_then(|id| lock(&p).remove(id));
                        if let Some(tx) = tx {
                            let _ = tx.send(v);
                        }
                    }
                    Some("event") => {
                        let mut ib = lock(&inbox);
                        if let Some(sub) = v["sub"].as_str() {
                            let list = ib.telemetry.entry(sub.to_string()).or_default();
                            if list.len() < MAX_SAMPLES {
                                list.push(v["data"].clone());
                            }
                        } else {
                            if ib.events.len() >= MAX_EVENTS {
                                ib.events.remove(0);
                            }
                            ib.events.push(json!({ "event": v["event"], "data": v["data"] }));
                        }
                    }
                    _ => {}
                }
            }
            a.store(false, Ordering::Relaxed);
            lock(&p).clear();
        });
        Ok(Conn { token: s.token, out, pending, alive })
    }

    /// Send one command envelope to the session `sel` names (default the
    /// most recent) and wait for its result envelope.
    async fn call(&self, env: Value, sel: Option<&str>) -> Result<Value, String> {
        let limit = proto::limit_for(&env, proto::BASE_TIMEOUT) + Duration::from_secs(5);
        let id = env["id"].as_str().unwrap_or("").to_string();
        let file = session::resolve(&self.root, sel)?;
        let rx = {
            let mut guard = self.conn.lock().await;
            // Reconnect when the connection dropped, another session is
            // asked for, or the file now names another bridge (a newer
            // session took over `session.json`).
            let token = session::read_at(&file).ok().map(|s| s.token);
            let stale = guard.as_ref().is_none_or(|(f, c)| {
                *f != file || !c.alive.load(Ordering::Relaxed) || token.as_deref().is_some_and(|t| t != c.token)
            });
            if stale {
                *guard = None;
                *guard = Some((file.clone(), self.connect(&file).await?));
            }
            let Some((_, c)) = guard.as_ref() else {
                return Err(NOT_RUNNING.to_string());
            };
            let (tx, rx) = oneshot::channel();
            lock(&c.pending).insert(id.clone(), tx);
            if c.out.send(env.to_string()).is_err() {
                *guard = None;
                return Err(NOT_RUNNING.to_string());
            }
            rx
        };
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Err("The connection to the live server dropped before it answered.".into()),
            Err(_) => {
                if let Some((_, c)) = self.conn.lock().await.as_ref() {
                    lock(&c.pending).remove(&id);
                }
                Err("The live server didn't answer in time.".into())
            }
        }
    }

    fn take_events(&self) -> Vec<Value> {
        std::mem::take(&mut lock(&self.inbox).events)
    }

    async fn run_tool(&self, spec: &ToolSpec, args: &Map<String, Value>) -> CallToolResult {
        let sel = session_arg(args);
        let sel = sel.as_deref();
        let res = match spec.kind {
            Kind::Local => return local_check(self.root.clone(), args).await,
            Kind::Sessions => return sessions_result(&self.root),
            Kind::Sample => self.telemetry_sample(args, sel).await,
            _ => self.call(spec.envelope(&self.next_id(), args), sel).await,
        };
        let events = self.take_events();
        match res {
            Ok(mut env) if spec.cmd == "sweep.run" => {
                if let Err(e) = save_sweep_thumbs(&self.root, args, &mut env) {
                    env = proto::error(&env["id"], "internal", &format!("sweep thumbnails: {e}"), None);
                }
                sweep_result(&env, events)
            }
            Ok(env) => tool_result(&env, events),
            Err(msg) => {
                let mut r = CallToolResult::error(vec![ContentBlock::text(msg)]);
                r.content.push(events_block(events));
                r
            }
        }
    }

    /// `telemetry_sample`: subscribe, wait, unsubscribe, average (11).
    async fn telemetry_sample(&self, args: &Map<String, Value>, sel: Option<&str>) -> Result<Value, String> {
        let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(2.0).clamp(0.5, 30.0);
        let rate = args.get("rate_hz").and_then(Value::as_f64).unwrap_or(4.0).clamp(0.5, 60.0);
        let mut params = json!({ "rate_hz": rate });
        if let Some(f) = args.get("fields") {
            params["fields"] = f.clone();
        }
        let id = self.next_id();
        let sub_env = json!({ "type": "cmd", "v": 1, "id": id, "cmd": "telemetry.subscribe", "params": params });
        let res = self.call(sub_env, sel).await?;
        if res["ok"] != true {
            return Ok(res);
        }
        let Some(sub) = res["result"]["sub"].as_str().map(String::from) else {
            return Ok(proto::error(&res["id"], "internal", "telemetry.subscribe returned no sub", None));
        };
        tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
        let unsub = json!({ "type": "cmd", "v": 1, "id": self.next_id(), "cmd": "telemetry.unsubscribe", "params": { "sub": sub } });
        let _ = self.call(unsub, sel).await;
        let samples = lock(&self.inbox).telemetry.remove(&sub).unwrap_or_default();
        let frame_ms_max = samples
            .iter()
            .filter_map(|s| s["frame_ms"]["max"].as_f64())
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
        Ok(json!({ "type": "result", "v": 1, "id": res["id"], "ok": true, "result": {
            "seconds": seconds, "rate_hz": rate, "samples": samples.len(),
            "average": average(&samples), "frame_ms_max": frame_ms_max,
            "last": samples.last().cloned().unwrap_or(Value::Null),
        }}))
    }
}

/// A tool's `session` argument: a string, or a bare port number.
fn session_arg(args: &Map<String, Value>) -> Option<String> {
    match args.get(tools::SESSION_PROP)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `live_sessions`: every session file, newest first, and which one a tool
/// without `session` uses.
pub fn sessions_result(root: &Path) -> CallToolResult {
    let default_file = session::resolve(root, None).ok();
    let main = session::read_at(&session::path(root)).ok();
    let list: Vec<Value> = session::list(root)
        .into_iter()
        .map(|i| {
            // `session.json` is a copy, so match the default by token too.
            let is_default = default_file.as_ref().is_some_and(|f| *f == i.file)
                || (default_file.as_ref() == Some(&session::path(root))
                    && main.as_ref().is_some_and(|m| session::read_at(&i.file).is_ok_and(|s| s.token == m.token)));
            json!({
                "port": i.port, "game": i.game, "kind": i.kind, "started": i.started, "pid": i.pid,
                "alive": i.alive, "default": is_default, "url": format!("http://localhost:{}/?live=1", i.port),
            })
        })
        .collect();
    let text = json!({
        "sessions": list,
        "default": std::env::var("EVANI_LIVE_SESSION").ok().filter(|s| !s.trim().is_empty())
            .map_or_else(|| "most recent (session.json)".to_string(), |s| format!("EVANI_LIVE_SESSION={s}")),
        "how": "Pass session: \"<port>\" or \"<game>\" on any live tool to drive that session.",
    });
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&text).unwrap_or_else(|_| text.to_string()),
    )])
}

fn events_block(events: Vec<Value>) -> ContentBlock {
    ContentBlock::text(json!({ "events_since_last_call": events }).to_string())
}

/// The MCP result for a command's result envelope: the `result` as JSON text
/// with every `*png_base64` swapped for a marker and sent as an image.
/// `evani_check`: the check gate (evani_guide::check), off the async threads.
pub async fn local_check(root: PathBuf, args: &Map<String, Value>) -> CallToolResult {
    let opts = evani_guide::check::Options {
        apps: args.get("app").and_then(Value::as_str).map(|a| vec![a.to_string()]),
        full: args.get("full").and_then(Value::as_bool).unwrap_or(false),
        ..Default::default()
    };
    match tokio::task::spawn_blocking(move || evani_guide::check::run(&root, &opts)).await {
        Ok(report) => {
            let text = report.text(false, evani_guide::check::Severity::Note);
            if report.passes(false) {
                CallToolResult::success(vec![ContentBlock::text(text)])
            } else {
                CallToolResult::error(vec![ContentBlock::text(text)])
            }
        }
        Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("check: {e}"))]),
    }
}

pub fn tool_result(env: &Value, events: Vec<Value>) -> CallToolResult {
    if env["ok"] == true {
        let mut result = env["result"].clone();
        let mut images = Vec::new();
        pull_images(&mut result, &mut images);
        let mut content = vec![ContentBlock::text(
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
        )];
        for png in images {
            content.push(ContentBlock::image(png, "image/png"));
        }
        content.push(events_block(events));
        CallToolResult::success(content)
    } else {
        let e = &env["error"];
        let mut text = format!(
            "{}: {}",
            e["code"].as_str().unwrap_or("internal"),
            e["message"].as_str().unwrap_or("unknown error")
        );
        if let Some(d) = e.get("data") {
            text.push('\n');
            text.push_str(&d.to_string());
        }
        CallToolResult::error(vec![ContentBlock::text(text), events_block(events)])
    }
}

/// `sweep_run`: the table as compact JSON, one line per row (pretty
/// printing would put every number on its own line).
pub fn sweep_result(env: &Value, events: Vec<Value>) -> CallToolResult {
    if env["ok"] != true {
        return tool_result(env, events);
    }
    let mut result = env["result"].clone();
    // Inline thumbnails (thumbs.inline) go as image content.
    let mut images = Vec::new();
    pull_images(&mut result, &mut images);
    let rows = result.as_object_mut().and_then(|m| m.remove("rows"));
    let mut text = result.to_string();
    if let Some(Value::Array(rows)) = rows {
        let body: Vec<String> = rows.iter().map(Value::to_string).collect();
        text.pop();
        if !text.ends_with('{') {
            text.push(',');
        }
        text.push_str(&format!("\"rows\":[\n{}\n]}}", body.join(",\n")));
    }
    let mut content = vec![ContentBlock::text(text)];
    for png in images {
        content.push(ContentBlock::image(png, "image/png"));
    }
    content.push(events_block(events));
    CallToolResult::success(content)
}

/// The folder for a sweep's thumbnails: `thumbs.dir` (relative, inside
/// the root) or `.evani/live/sweeps/<run>`.
fn thumbs_dir(root: &Path, dir: Option<&str>, run: &str) -> Result<PathBuf, String> {
    let rel = match dir {
        Some(d) => PathBuf::from(d),
        None => {
            let run: String = run.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            Path::new(".evani/live/sweeps").join(if run.is_empty() { "sweep".into() } else { run })
        }
    };
    let inside = rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if rel.as_os_str().is_empty() || !inside {
        return Err(format!("thumbs.dir must be a folder inside the project root, not {}", rel.display()));
    }
    Ok(root.join(rel))
}

/// `sweep_run` thumbnails go to files: each `thumbs[i].png_base64` is
/// written as `row-N.png` and replaced by its `path` (unless the call asked
/// for `thumbs.inline`).
pub fn save_sweep_thumbs(root: &Path, args: &Map<String, Value>, env: &mut Value) -> Result<(), String> {
    let opts = args.get("thumbs");
    if env["ok"] != true || opts.and_then(|t| t.get("inline")).and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let run = env["result"]["run"].as_str().unwrap_or("sweep").to_string();
    let list = env.get_mut("result").and_then(|r| r.get_mut("thumbs")).and_then(Value::as_array_mut);
    let Some(list) = list.filter(|l| !l.is_empty()) else {
        return Ok(());
    };
    let dir = thumbs_dir(root, opts.and_then(|t| t.get("dir")).and_then(Value::as_str), &run)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for t in list.iter_mut() {
        let Some(Value::String(b64)) = t.as_object_mut().and_then(|m| m.remove("png_base64")) else {
            continue;
        };
        let png = base64_decode(&b64).ok_or("a thumbnail is not valid base64")?;
        let file = dir.join(format!("row-{}.png", t["row"].as_u64().unwrap_or(0)));
        std::fs::write(&file, png).map_err(|e| format!("{}: {e}", file.display()))?;
        t["path"] = json!(file.display().to_string());
    }
    Ok(())
}

/// Standard base64 (padding optional) to bytes; `None` if malformed.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        Some(u32::from(v))
    };
    let s = s.trim_end_matches('=').as_bytes();
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Some(out)
}

fn pull_images(v: &mut Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                if k.ends_with("png_base64") && val.is_string() {
                    if let Value::String(s) = std::mem::take(val) {
                        out.push(s);
                        *val = Value::String(format!("(image {}, attached as image content)", out.len()));
                    }
                } else {
                    pull_images(val, out);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| pull_images(x, out)),
        _ => {}
    }
}

/// Mean of every numeric leaf across samples; other leaves keep the last value.
pub fn average(samples: &[Value]) -> Value {
    let Some(last) = samples.last() else { return Value::Null };
    match last {
        Value::Number(_) => {
            let nums: Vec<f64> = samples.iter().filter_map(Value::as_f64).collect();
            if nums.is_empty() {
                return Value::Null;
            }
            let mean = nums.iter().sum::<f64>() / nums.len() as f64;
            json!((mean * 1000.0).round() / 1000.0)
        }
        Value::Object(_) => {
            let mut keys: Vec<&String> = Vec::new();
            for s in samples {
                if let Value::Object(m) = s {
                    for k in m.keys() {
                        if !keys.contains(&k) {
                            keys.push(k);
                        }
                    }
                }
            }
            let mut out = Map::new();
            for k in keys {
                let col: Vec<Value> = samples.iter().filter_map(|s| s.get(k.as_str()).cloned()).collect();
                out.insert(k.clone(), average(&col));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

impl ServerHandler for LiveMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("evani-live", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = tools::TOOLS
            .iter()
            .map(|t| Tool::new(t.name, t.desc, Arc::new(t.schema())))
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(spec) = tools::find(&request.name) else {
            return Err(ErrorData::invalid_params(format!("no tool named {}", request.name), None));
        };
        let args = request.arguments.unwrap_or_default();
        Ok(self.run_tool(spec, &args).await.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(r: &CallToolResult) -> Vec<String> {
        r.content
            .iter()
            .filter_map(|c| serde_json::to_value(c).ok())
            .filter_map(|v| v["text"].as_str().map(String::from))
            .collect()
    }

    #[test]
    fn results_errors_and_images() {
        let ok = json!({"type":"result","v":1,"id":"m-1","ok":true,
            "result":{"tx":3,"capture":{"png_base64":"iVBOR","width":480}}});
        let r = tool_result(&ok, vec![json!({"event":"auto_revert"})]);
        assert_eq!(r.is_error, Some(false));
        let all = serde_json::to_value(&r).unwrap();
        let content = all["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert!(content[0]["text"].as_str().unwrap().contains("(image 1"));
        assert!(!content[0]["text"].as_str().unwrap().contains("iVBOR"));
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["data"], "iVBOR");
        assert_eq!(content[1]["mimeType"], "image/png");
        assert!(content[2]["text"].as_str().unwrap().contains("auto_revert"));

        let planned = json!({"type":"result","v":1,"id":"m-2","ok":false,
            "error":{"code":"unknown_command","message":"bake.export is not implemented in this build yet","data":{"planned":true}}});
        let r = tool_result(&planned, vec![]);
        assert_eq!(r.is_error, Some(true));
        let t = texts(&r);
        assert!(t[0].starts_with("unknown_command: bake.export"));
        assert!(t[0].contains("\"planned\":true"));
        assert_eq!(t[1], r#"{"events_since_last_call":[]}"#);
    }

    #[test]
    fn averages() {
        let s = vec![
            json!({"fps":60,"frame_ms":{"avg":16.0,"max":17.0},"tier":"medium"}),
            json!({"fps":58,"frame_ms":{"avg":17.0,"max":20.0},"tier":"low"}),
        ];
        let a = average(&s);
        assert_eq!(a["fps"], 59.0);
        assert_eq!(a["frame_ms"]["avg"], 16.5);
        assert_eq!(a["tier"], "low");
        assert_eq!(average(&[]), Value::Null);
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..20u8 {
            let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37).wrapping_add(n)).collect();
            assert_eq!(base64_decode(&evani_live::img::base64(&data)), Some(data), "{n} bytes");
        }
        assert_eq!(base64_decode("aGk"), Some(b"hi".to_vec()), "padding optional");
        assert_eq!(base64_decode("a$=="), None);
        assert_eq!(base64_decode("abcde"), None);
    }

    fn sweep_env(thumbs: bool) -> Value {
        let png = evani_live::img::base64(&evani_live::img::encode_png(&evani_live::img::Rgb {
            w: 2,
            h: 1,
            px: vec![255, 0, 0, 0, 0, 255],
        }));
        let mut r = json!({"type":"result","v":1,"id":"m-1","ok":true,"result":{"run":"sw-3",
            "columns":["fx.bloom.intensity","work_ms","clipped"],"rows":[[0.6,1.2,0.01],[0.2,1.1,0.0]]}});
        if thumbs {
            r["result"]["thumbs"] = json!([{"row":0,"png_base64":png,"width":2,"height":1},
                                           {"row":1,"png_base64":png,"width":2,"height":1}]);
        }
        r
    }

    #[test]
    fn sweep_thumbnails_become_files_and_the_table_stays_compact() {
        let root = std::env::temp_dir().join(format!("evani-live-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut env = sweep_env(true);
        save_sweep_thumbs(&root, &Map::new(), &mut env).unwrap();
        let t = &env["result"]["thumbs"];
        assert_eq!(t[0].get("png_base64"), None);
        let p = PathBuf::from(t[1]["path"].as_str().unwrap());
        assert_eq!(p, root.join(".evani/live/sweeps/sw-3").join("row-1.png"));
        assert!(std::fs::read(&p).unwrap().starts_with(b"\x89PNG"));
        let r = sweep_result(&env, vec![]);
        let text = &texts(&r)[0];
        assert!(text.contains("\"rows\":[\n[0.6,1.2,0.01],\n[0.2,1.1,0.0]\n]}"), "{text}");
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["rows"][1][1], 1.1);
        assert_eq!(parsed["columns"][2], "clipped");
        assert_eq!(serde_json::to_value(&r).unwrap()["content"].as_array().unwrap().len(), 2, "no images");

        // A chosen folder inside the root; nothing outside it.
        let mut env = sweep_env(true);
        let args = json!({"thumbs":{"dir":"shots/sweep"}});
        save_sweep_thumbs(&root, args.as_object().unwrap(), &mut env).unwrap();
        assert!(root.join("shots/sweep/row-0.png").is_file());
        for bad in ["../out", "/abs", "C:/abs", ""] {
            let mut env = sweep_env(true);
            let args = json!({"thumbs":{"dir":bad}});
            assert!(save_sweep_thumbs(&root, args.as_object().unwrap(), &mut env).is_err(), "{bad}");
        }
        // inline: base64 stays and goes as image content.
        let mut env = sweep_env(true);
        let args = json!({"thumbs":{"inline":true}});
        save_sweep_thumbs(&root, args.as_object().unwrap(), &mut env).unwrap();
        let r = serde_json::to_value(sweep_result(&env, vec![])).unwrap();
        let content = r["content"].as_array().unwrap();
        assert_eq!(content.len(), 4);
        assert_eq!(content[1]["type"], "image");
        assert!(!content[0]["text"].as_str().unwrap().contains("iVBOR"));
        // No thumbs, errors: unchanged.
        let mut env = sweep_env(false);
        save_sweep_thumbs(&root, &Map::new(), &mut env).unwrap();
        assert_eq!(env, sweep_env(false));
        let err = json!({"type":"result","v":1,"id":"m-2","ok":false,
            "error":{"code":"timeout","message":"no frame","data":{"row":2}}});
        let r = sweep_result(&err, vec![]);
        assert_eq!(r.is_error, Some(true));
        assert!(texts(&r)[0].contains("\"row\":2"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn no_bridge_gives_the_documented_message() {
        let dir = std::env::temp_dir().join(format!("evani-live-mcp-{}", std::process::id()));
        let m = LiveMcp::new(dir);
        let spec = tools::find("schema_get").unwrap();
        let r = m.run_tool(spec, &Map::new()).await;
        assert_eq!(r.is_error, Some(true));
        assert_eq!(texts(&r)[0], NOT_RUNNING);
        // Naming a session that isn't there says so, and lists what is.
        let args = json!({"session": 8131});
        let r = m.run_tool(spec, args.as_object().unwrap()).await;
        assert_eq!(r.is_error, Some(true));
        assert!(texts(&r)[0].starts_with("no live session on port 8131; none are running"), "{:?}", texts(&r));
        let r = m.run_tool(tools::find("live_sessions").unwrap(), &Map::new()).await;
        assert_eq!(r.is_error, Some(false));
        let v: Value = serde_json::from_str(&texts(&r)[0]).unwrap();
        assert_eq!(v["sessions"], json!([]));
    }
}
