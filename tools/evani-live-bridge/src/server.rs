//! `serve`: HTTP and WebSocket on one localhost port (docs/LIVE_API.md 3).

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::auth::{self, Door};
use crate::hub::Hub;
use crate::proto::{self, MAX_MESSAGE};

/// WebSocket close code for a missing or wrong token (4.2).
pub const CLOSE_UNAUTHORIZED: u16 = 4401;
/// Close code sent to a page replaced by a newer one.
pub const CLOSE_REPLACED: u16 = 4000;
/// How long a page has to send `auth` after connecting.
const AUTH_WAIT: Duration = Duration::from_secs(5);

pub struct App {
    pub hub: Arc<Hub>,
    pub token: String,
    /// The build being served: Trunk's dist, or dx's once its first build
    /// is up (live code swapping, LIVE_API.md 10.4).
    dist: std::sync::RwLock<PathBuf>,
    /// Page hot-replace (LIVE_API.md 15): `/` is composed from the app's
    /// current page files. `None` serves the build's page as it is.
    pub page: Option<Arc<crate::page::PageHot>>,
    /// Hot mode (10.4): dx's dist and the baked files its build embeds;
    /// `/` from that dist carries the files changed since (baked.rs).
    baked: std::sync::RwLock<Option<(PathBuf, Arc<crate::baked::Snapshot>)>>,
    /// `serve --app-dir`: every file served must resolve (symlinks and
    /// junctions followed) inside the build folder.
    contained: bool,
}

impl App {
    pub fn new(hub: Arc<Hub>, token: String, dist: PathBuf, page: Option<Arc<crate::page::PageHot>>) -> Self {
        Self { hub, token, dist: std::sync::RwLock::new(dist), page, baked: std::sync::RwLock::new(None), contained: false }
    }

    /// Serve only files that resolve inside the build folder (a game outside
    /// the engine, `serve --app-dir`).
    pub fn contained(mut self) -> Self {
        self.contained = true;
        self
    }

    /// The baked files the build in `dist` embeds (set when dx starts).
    pub fn set_baked(&self, dist: PathBuf, built: crate::baked::Snapshot) {
        *self.baked.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((dist, Arc::new(built)));
    }

    /// The baked files changed since the served build was made (none
    /// outside hot mode).
    async fn baked_changes(&self) -> serde_json::Map<String, Value> {
        let built = {
            let g = self.baked.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*g {
                Some((dist, built)) if *dist == self.dist() => built.clone(),
                _ => return serde_json::Map::new(),
            }
        };
        let root = PathBuf::from(&self.hub.info.root);
        tokio::task::spawn_blocking(move || crate::baked::changed(&built, &crate::baked::snapshot(&root)))
            .await
            .unwrap_or_default()
    }

    pub fn dist(&self) -> PathBuf {
        self.dist.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn set_dist(&self, dist: PathBuf) {
        *self.dist.write().unwrap_or_else(std::sync::PoisonError::into_inner) = dist;
    }
}

type Shared = Arc<App>;

pub fn router(app: Shared) -> Router {
    let mut r = Router::new()
        .route("/live/status", get(status))
        .route("/live/v1/cmd", any(http_cmd))
        .route("/engine", any(ws_engine))
        .route("/ctl", any(ws_ctl))
        .route("/evani-dev", any(ws_dev))
        .route("/live/v1/convert", any(convert_post))
        .route("/live/v1/convert/{*rest}", any(convert_get));
    // Convert jobs report through the hub (weak: the hub owns the slot).
    let weak = Arc::downgrade(&app.hub);
    app.hub.convert.set_sink(Arc::new(move |event: &str, data: Value| {
        if let Some(h) = weak.upgrade() {
            h.convert_event(event, data);
        }
    }));
    // Model mode only (EVANI_MODEL_TAB.md 4.3); without `--models` these
    // paths fall through to the game's files exactly as before.
    if app.hub.models.is_some() {
        r = r.route("/model/v1/{*rest}", any(model_route));
    }
    r.fallback(files)
        .layer(middleware::from_fn_with_state(app.clone(), host_check))
        .with_state(app)
}

// ---- errors -----------------------------------------------------------------------

fn fail(status: StatusCode, code: &str, message: &str) -> Response {
    let body = proto::error(&Value::Null, code, message, None);
    (status, [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

fn header_str(h: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// Every request: `Host` must be `localhost:P` or `127.0.0.1:P`, else 421.
async fn host_check(State(app): State<Shared>, req: Request, next: Next) -> Response {
    let host = header_str(req.headers(), header::HOST);
    if !auth::host_ok(host, app.hub.info.port) {
        return fail(StatusCode::MISDIRECTED_REQUEST, "forbidden_origin", "Host not allowed");
    }
    next.run(req).await
}

#[allow(clippy::result_large_err)] // Response is big; these run once per request
fn origin_check(door: Door, headers: &HeaderMap, port: u16) -> Result<(), Response> {
    if auth::origin_ok(door, header_str(headers, header::ORIGIN), port) {
        Ok(())
    } else {
        Err(fail(StatusCode::FORBIDDEN, "forbidden_origin", "Origin not allowed"))
    }
}

#[allow(clippy::result_large_err)]
fn token_check(headers: &HeaderMap, token: &str) -> Result<(), Response> {
    if auth::authorized(header_str(headers, header::AUTHORIZATION), token) {
        Ok(())
    } else {
        Err(fail(StatusCode::UNAUTHORIZED, "unauthorized", "Missing or wrong token"))
    }
}

#[allow(clippy::result_large_err)]
fn upgrade_of(ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>) -> Result<WebSocketUpgrade, Response> {
    ws.map(|w| w.max_message_size(MAX_MESSAGE).max_frame_size(MAX_MESSAGE))
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "bad_request", "WebSocket upgrade expected"))
}

// ---- HTTP -----------------------------------------------------------------------

async fn status(State(app): State<Shared>) -> Response {
    let mut r = json_response(&app.hub.status());
    r.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    r
}

/// `POST /live/v1/cmd`: one command, or a JSON array of commands run in order.
async fn http_cmd(State(app): State<Shared>, method: Method, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(r) = origin_check(Door::HttpCmd, &headers, app.hub.info.port) {
        return r;
    }
    if let Err(r) = token_check(&headers, &app.token) {
        return r;
    }
    if method != Method::POST {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "bad_request", "POST a command");
    }
    if body.len() > MAX_MESSAGE {
        return fail(StatusCode::PAYLOAD_TOO_LARGE, "bad_request", "Body over 8 MiB");
    }
    let Ok(req) = serde_json::from_slice::<Value>(&body) else {
        return fail(StatusCode::BAD_REQUEST, "bad_request", "Body is not JSON");
    };
    let conn = app.hub.http_conn();
    let out = match req {
        Value::Array(items) => {
            let mut results = Vec::with_capacity(items.len());
            for item in items {
                results.push(app.hub.command(conn, "http", item).await);
            }
            Value::Array(results)
        }
        one => app.hub.command(conn, "http", one).await,
    };
    json_response(&out)
}

/// The game's files; `index.html` gets the token. While the game builds (or
/// if its build failed) `/` is a holding page that reloads when it's ready.
async fn files(State(app): State<Shared>, method: Method, req: Request) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "bad_request", "GET only");
    }
    let path = req.uri().path();
    let is_index = path == "/" || path == "/index.html";
    let build = app.hub.build();
    if build.phase != "idle" {
        return if is_index {
            html(holding_page(build.phase, build.error.as_deref()))
        } else {
            (StatusCode::SERVICE_UNAVAILABLE, "The game is building").into_response()
        };
    }
    let Some(file) = safe_join(&app.dist(), if is_index { "index.html" } else { path }) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let file = if app.contained {
        match evani_guide::app_dir::contained(&app.dist(), &file) {
            Some(f) => f,
            None => return (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    } else {
        file
    };
    // A dx build (live code swapping) has no copy of what the page's Trunk
    // `copy-dir` / `copy-file` links carry (models, data): serve those from
    // the app folder. Trunk's own dist has them, so it is read first.
    let read = match tokio::fs::read(&file).await {
        Ok(b) => Ok(b),
        Err(e) => match app.page.as_ref().and_then(|p| p.static_file(path)) {
            Some(src) => tokio::fs::read(&src).await,
            None => Err(e),
        },
    };
    let Ok(bytes) = read else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    // The content type follows the file asked for, not where it was found.
    if is_index {
        let built = String::from_utf8_lossy(&bytes);
        let page = match &app.page {
            Some(p) => p.served(&built),
            None => built.into_owned(),
        };
        let page = crate::baked::inject(&page, &app.baked_changes().await);
        let mut r = html(inject_token(&page, &app.token));
        // The renderer fetches converted models by plain URL (no header),
        // so the page gets the token as a cookie for the convert routes only.
        if let Ok(v) = header::HeaderValue::from_str(&convert_cookie(app.hub.info.port, &app.token)) {
            r.headers_mut().insert(header::SET_COOKIE, v);
        }
        return r;
    }
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(&file))
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ---- drop-to-convert (16) ---------------------------------------------------------

fn cookie_name(port: u16) -> String {
    format!("evani_live_{port}")
}

/// The convert cookie: the dev token, sent back only to the convert routes.
fn convert_cookie(port: u16, token: &str) -> String {
    format!("{}={token}; HttpOnly; SameSite=Strict; Path=/live/v1/convert", cookie_name(port))
}

/// The convert routes take the token as `Authorization: Bearer` or as the
/// page's cookie.
fn convert_token_ok(headers: &HeaderMap, app: &App) -> bool {
    auth::cookie(header_str(headers, header::COOKIE), &cookie_name(app.hub.info.port)).is_some_and(|t| auth::token_eq(t, &app.token))
        || auth::authorized(header_str(headers, header::AUTHORIZATION), &app.token)
}

/// `Sec-Fetch-Site` on a convert route: absent (an agent or curl),
/// `same-origin` (the page) or `none` (typed in the address bar). Cookies
/// ignore the port, so another localhost page is `same-site`: refused, like
/// `cross-site`.
fn fetch_site_ok(v: Option<&str>) -> bool {
    matches!(v.map(str::trim), None | Some("same-origin") | Some("none"))
}

/// Why a convert request is refused (403 `forbidden_origin`, 401
/// `unauthorized`), or `None` to let it in.
fn convert_refused(headers: &HeaderMap, app: &App) -> Option<Response> {
    if !fetch_site_ok(header_str(headers, header::HeaderName::from_static("sec-fetch-site"))) {
        return Some(fail(StatusCode::FORBIDDEN, "forbidden_origin", "Only the game's own page may use the convert routes"));
    }
    if let Err(r) = origin_check(Door::Convert, headers, app.hub.info.port) {
        return Some(r);
    }
    if !convert_token_ok(headers, app) {
        return Some(fail(StatusCode::UNAUTHORIZED, "unauthorized", "Missing or wrong token"));
    }
    None
}

/// Every convert answer, refusals included: never sniffed as another type,
/// never readable by another origin's page.
fn hardened(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    h.insert(header::HeaderName::from_static("cross-origin-resource-policy"), header::HeaderValue::from_static("same-origin"));
    r
}

fn refusal(r: &crate::convert::Refusal) -> Response {
    let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_REQUEST);
    fail(status, r.code, &r.message)
}

/// `POST /live/v1/convert?id=<id>`: the raw body is a `.glb`, a zipped
/// `.gltf` or a `.blend`. Answers 202 `{job, hash, id}` at once; the job's
/// `convert_progress` and `convert_done` events follow (16).
async fn convert_post(State(app): State<Shared>, method: Method, req: Request) -> Response {
    hardened(convert_upload(app, method, req).await)
}

async fn convert_upload(app: Shared, method: Method, req: Request) -> Response {
    let headers = req.headers().clone();
    if let Some(r) = convert_refused(&headers, &app) {
        return r;
    }
    if method != Method::POST {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "bad_request", "POST a model file");
    }
    let cap = crate::convert::MAX_UPLOAD;
    let too_big = || fail(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &format!("The upload is over {} MiB.", cap / (1024 * 1024)));
    // Refused before a byte of the body is read.
    let declared = header_str(&headers, header::CONTENT_LENGTH).and_then(|v| v.trim().parse::<u64>().ok());
    if declared.is_some_and(|n| n > cap as u64) {
        return too_big();
    }
    if let Some(job) = app.hub.convert.running() {
        return fail(StatusCode::CONFLICT, "busy", &format!("A conversion is already running ({job}); try again when it is done."));
    }
    let id = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("id=")))
        .map(String::from);
    // Also capped while reading (a chunked body has no Content-Length).
    let Ok(body) = axum::body::to_bytes(req.into_body(), cap).await else {
        return too_big();
    };
    let conv = app.hub.convert.clone();
    match tokio::task::spawn_blocking(move || conv.start_upload(&body, id.as_deref())).await {
        Ok(Ok(v)) => {
            let mut r = json_response(&v);
            *r.status_mut() = StatusCode::ACCEPTED;
            r
        }
        Ok(Err(e)) => refusal(&e),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, "internal", &format!("the upload stopped: {e}")),
    }
}

/// `GET /live/v1/convert/<hash>/<file>`: a converted file (tiers, the
/// master, `textures/`, the report and its sheets).
async fn convert_get(State(app): State<Shared>, method: Method, headers: HeaderMap, req: Request) -> Response {
    hardened(convert_file(app, method, headers, req).await)
}

async fn convert_file(app: Shared, method: Method, headers: HeaderMap, req: Request) -> Response {
    if let Some(r) = convert_refused(&headers, &app) {
        return r;
    }
    if method != Method::GET && method != Method::HEAD {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "bad_request", "GET only");
    }
    let rest = req.uri().path().trim_start_matches("/live/v1/convert/");
    let Some(file) = app.hub.convert.file(rest) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let Ok(bytes) = tokio::fs::read(&file).await else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let ct = match file.extension().and_then(|e| e.to_str()) {
        Some("glb") => "model/gltf-binary",
        Some("ktx2") => "image/ktx2",
        Some("md") => "text/markdown; charset=utf-8",
        _ => content_type(&file),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `GET /model/v1/...` (EVANI_MODEL_TAB.md 4.3): the model list, a model's
/// history, a request's status, and the files a version lists. No token
/// (they only read dev files); the Host check stops DNS rebinding, and a
/// foreign `Origin` is refused (403).
async fn model_route(State(app): State<Shared>, method: Method, headers: HeaderMap, req: Request) -> Response {
    if let Err(r) = origin_check(Door::Ctl, &headers, app.hub.info.port) {
        return r;
    }
    if method != Method::GET && method != Method::HEAD {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "bad_request", "GET only");
    }
    let Some(ws) = app.hub.models.clone() else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let path = req.uri().path().trim_start_matches("/model/v1/").to_string();
    let parts: Vec<String> = path.split('/').map(str::to_string).collect();
    let answer = tokio::task::spawn_blocking(move || model_answer(&ws, &parts)).await;
    let no_store = |mut r: Response| {
        r.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
        r
    };
    match answer {
        Ok(ModelAnswer::Json(v)) => no_store(json_response(&v)),
        Ok(ModelAnswer::Error(code, message)) => no_store(fail(StatusCode::NOT_FOUND, code, &message)),
        Ok(ModelAnswer::File(file)) => match tokio::fs::read(&file).await {
            Ok(bytes) => Response::builder()
                .header(header::CONTENT_TYPE, if file.extension().is_some_and(|e| e == "glb") { "model/gltf-binary" } else { content_type(&file) })
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
            Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
        Ok(ModelAnswer::NotFound) | Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

enum ModelAnswer {
    Json(Value),
    Error(&'static str, String),
    File(PathBuf),
    NotFound,
}

fn model_answer(ws: &crate::models::Workspace, parts: &[String]) -> ModelAnswer {
    match parts {
        [m] if m == "models" => ModelAnswer::Json(ws.list()),
        [r, id] if r == "requests" => ModelAnswer::Json(ws.request(id)),
        [m, h] if h == "history" => match ws.history(m) {
            Ok(v) => ModelAnswer::Json(v),
            Err(e) => ModelAnswer::Error(e.code, e.message),
        },
        [m, v, f] => ws.version_file(m, v, f).map_or(ModelAnswer::NotFound, ModelAnswer::File),
        _ => ModelAnswer::NotFound,
    }
}

fn json_response(v: &Value) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], v.to_string()).into_response()
}

fn html(text: String) -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")],
        text,
    )
        .into_response()
}

/// A path under `dist` from a URL path, refusing anything that climbs out.
pub fn safe_join(dist: &Path, url_path: &str) -> Option<PathBuf> {
    let rel = url_path.trim_start_matches('/');
    let rel = rel.split(['?', '#']).next().unwrap_or("");
    let mut out = dist.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (out != dist).then_some(out)
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "wgsl" | "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Put `<meta name="evani-live-token">` first in `<head>` (4.3).
pub fn inject_token(index: &str, token: &str) -> String {
    let meta = format!("<meta name=\"evani-live-token\" content=\"{token}\">");
    let lower = index.to_ascii_lowercase();
    match lower.find("<head") {
        Some(start) => match lower[start..].find('>') {
            Some(end) => {
                let at = start + end + 1;
                format!("{}\n  {meta}{}", &index[..at], &index[at..])
            }
            None => format!("{meta}{index}"),
        },
        None => format!("{meta}{index}"),
    }
}

fn holding_page(phase: &str, error: Option<&str>) -> String {
    let (title, body) = if phase == "error" {
        ("The game didn't build", error.unwrap_or("See the live server's output."))
    } else {
        ("Building the gameâ€¦", "This page reloads by itself when it's ready.")
    };
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>Evani.Dev live</title>
<style>html,body{{margin:0;height:100%;background:#0d0f14;color:#d6dae3;font:15px system-ui,sans-serif}}
main{{max-width:52rem;margin:18vh auto;padding:0 1.5rem}}h1{{font-weight:600;font-size:1.4rem}}
pre{{white-space:pre-wrap;color:#e7a0a0;font-size:12px}}</style></head>
<body><main><h1>{title}</h1><pre>{body}</pre></main>
<script>
setInterval(async () => {{
  try {{
    const s = await (await fetch('/live/status', {{ cache: 'no-store' }})).json();
    if (s.phase === 'idle') location.reload();
  }} catch (_) {{}}
}}, 1000);
</script></body></html>"#,
        title = title,
        body = esc(body),
    )
}

// ---- WebSockets -------------------------------------------------------------------

/// Send everything from `rx` to the socket; close it when `rx` ends.
async fn pump_out(
    mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
    mut rx: mpsc::UnboundedReceiver<String>,
    close_code: Option<u16>,
) {
    while let Some(text) = rx.recv().await {
        if sink.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    let frame = close_code.map(|code| CloseFrame { code, reason: "replaced by a newer page".into() });
    let _ = sink.send(Message::Close(frame)).await;
}

async fn ws_engine(
    State(app): State<Shared>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    if let Err(r) = origin_check(Door::Engine, &headers, app.hub.info.port) {
        return r;
    }
    let ws = match upgrade_of(ws) {
        Ok(w) => w,
        Err(r) => return r,
    };
    ws.on_upgrade(move |socket| engine_session(app, socket))
}

/// The page: first message must be `{"type":"auth","v":1,"token":â€¦}` (4.2).
async fn engine_session(app: Shared, mut socket: WebSocket) {
    let first = tokio::time::timeout(AUTH_WAIT, socket.recv()).await;
    let ok = match first {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<Value>(t.as_str()).is_ok_and(|m| {
            m["type"] == "auth" && m["token"].as_str().is_some_and(|tok| auth::token_eq(tok, &app.token))
        }),
        _ => false,
    };
    if !ok {
        let _ = socket
            .send(Message::Close(Some(CloseFrame { code: CLOSE_UNAUTHORIZED, reason: "unauthorized".into() })))
            .await;
        return;
    }
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel();
    let id = app.hub.attach_engine(tx.clone());
    // Offer the last session mirror back (10.3); the page may ignore it.
    if let Some(m) = app.hub.mirror() {
        let _ = tx.send(json!({ "type": "event", "v": 1, "event": "session.mirror", "data": m }).to_string());
    }
    drop(tx);
    let writer = tokio::spawn(pump_out(sink, rx, Some(CLOSE_REPLACED)));
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Text(t) => app.hub.on_engine_message(id, t.as_str()),
            Message::Close(_) => break,
            _ => {}
        }
    }
    app.hub.detach_engine(id);
    writer.abort();
}

async fn ws_ctl(
    State(app): State<Shared>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    if let Err(r) = origin_check(Door::Ctl, &headers, app.hub.info.port) {
        return r;
    }
    if let Err(r) = token_check(&headers, &app.token) {
        return r;
    }
    let ws = match upgrade_of(ws) {
        Ok(w) => w,
        Err(r) => return r,
    };
    let is_mcp = header_str(&headers, header::HeaderName::from_static("x-evani-client")) == Some("mcp");
    ws.on_upgrade(move |socket| ctl_session(app, socket, if is_mcp { "mcp" } else { "ctl" }))
}

/// A controller: gets `hello`, sends commands, gets its results and events.
async fn ctl_session(app: Shared, socket: WebSocket, source: &'static str) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let _ = tx.send(app.hub.ctl_hello().to_string());
    let conn = app.hub.attach_ctl(tx.clone());
    let writer = tokio::spawn(pump_out(sink, rx, None));
    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t.as_str().to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(req) = serde_json::from_str::<Value>(&text) else {
            let _ = tx.send(proto::error(&Value::Null, "bad_request", "not JSON", None).to_string());
            continue;
        };
        let (hub, tx) = (app.hub.clone(), tx.clone());
        tokio::spawn(async move {
            let res = hub.command(conn, source, req).await;
            let _ = tx.send(res.to_string());
        });
    }
    app.hub.detach_ctl(conn);
    drop(tx);
    let _ = writer.await;
}

async fn ws_dev(
    State(app): State<Shared>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    if let Err(r) = origin_check(Door::EvaniDev, &headers, app.hub.info.port) {
        return r;
    }
    let ws = match upgrade_of(ws) {
        Ok(w) => w,
        Err(r) => return r,
    };
    ws.on_upgrade(move |socket| dev_session(app, socket))
}

/// The Evani.dev tab: news only; it may send `chat_reply`, nothing else counts.
async fn dev_session(app: Shared, socket: WebSocket) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let conn = app.hub.attach_dev(tx);
    let writer = tokio::spawn(pump_out(sink, rx, None));
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Text(t) => app.hub.on_dev_message(t.as_str()),
            Message::Close(_) => break,
            _ => {}
        }
    }
    app.hub.detach_dev(conn);
    writer.abort();
}

// ---- the approve listener (15.7) ------------------------------------------------------

/// The look guard's approve listener, on its own 127.0.0.1 port: only
/// `POST /look-guard/v1/approve`. Not proxied by `/live/v1/cmd` or `/ctl`,
/// not an MCP tool, and no dev token opens it: each approval carries an HMAC
/// with the secret only Evani.App holds plus Evani.App's Ed25519 signature.
pub fn approve_router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/look-guard/v1/approve", any(approve))
        .fallback(|| async { fail(StatusCode::NOT_FOUND, "not_found", "Only POST /look-guard/v1/approve is served here") })
        .layer(DefaultBodyLimit::max(crate::lookguard::MAX_APPROVE_BODY))
        .with_state(hub)
}

/// The MAC from `Authorization: Evani-Approve <hex>`.
fn approve_mac(header: Option<&str>) -> Option<String> {
    let (scheme, rest) = header?.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("evani-approve") {
        return None;
    }
    let mac = rest.trim();
    (mac.len() == 64 && mac.bytes().all(|b| b.is_ascii_hexdigit())).then(|| mac.to_ascii_lowercase())
}

async fn approve(State(hub): State<Arc<Hub>>, method: Method, headers: HeaderMap, body: Bytes) -> Response {
    let port = hub.lookguard.approve_port().unwrap_or(0);
    if port == 0 || !auth::host_ok(header_str(&headers, header::HOST), port) {
        hub.lookguard.refused_at_door("forbidden_origin", "Host not allowed");
        return fail(StatusCode::MISDIRECTED_REQUEST, "forbidden_origin", "Host not allowed");
    }
    // Evani.App is a native client: no browser page may approve.
    if headers.contains_key(header::ORIGIN) {
        hub.lookguard.refused_at_door("forbidden_origin", "a browser Origin was sent");
        return fail(StatusCode::FORBIDDEN, "forbidden_origin", "Origin not allowed");
    }
    if method != Method::POST {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "bad_request", "POST an approval");
    }
    let Ok(v) = serde_json::from_slice::<Value>(&body) else {
        hub.lookguard.refused_at_door("bad_request", "body is not JSON");
        return fail(StatusCode::BAD_REQUEST, "bad_request", "Body is not JSON");
    };
    // A missing or malformed header is checked (and audited) like a wrong MAC.
    let mac = approve_mac(header_str(&headers, header::AUTHORIZATION)).unwrap_or_default();
    let h = hub.clone();
    let answer = tokio::task::spawn_blocking(move || h.lookguard.approve(&h, &mac, &v)).await;
    match answer {
        Ok(Ok(out)) => json_response(&out),
        Ok(Err(r)) => {
            let status = match r.code {
                "bad_request" => StatusCode::BAD_REQUEST,
                "bad_auth" | "bad_signature" => StatusCode::UNAUTHORIZED,
                "approvals_disabled" => StatusCode::SERVICE_UNAVAILABLE,
                "io_error" => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::CONFLICT,
            };
            fail(status, r.code, &r.message)
        }
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, "internal", &format!("the approval stopped: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{BuildState, HubInfo};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn token_goes_into_head() {
        let out = inject_token("<!DOCTYPE html>\n<html><HEAD>\n<title>x</title></HEAD></html>", "tok");
        assert!(out.contains("<HEAD>\n  <meta name=\"evani-live-token\" content=\"tok\">\n<title>"), "{out}");
        assert!(inject_token("<p>no head</p>", "t").starts_with("<meta name=\"evani-live-token\""));
    }

    #[test]
    fn paths_stay_inside_dist() {
        let d = Path::new("/d");
        assert_eq!(safe_join(d, "/app.js"), Some(d.join("app.js")));
        assert_eq!(safe_join(d, "/a/./b.wasm"), Some(d.join("a").join("b.wasm")));
        assert_eq!(safe_join(d, "/../secret"), None);
        assert_eq!(safe_join(d, "/a/../../x"), None);
        assert_eq!(safe_join(d, "/"), None);
    }

    /// A real server on a free port, game already "built" into a temp dist.
    async fn start() -> (u16, Arc<App>) {
        start_with(None).await
    }

    /// As [`start`], in model mode over `models` when given.
    async fn start_with(models: Option<crate::models::Workspace>) -> (u16, Arc<App>) {
        start_in("C:\\r", models).await
    }

    /// As [`start_with`], with the engine root at `root` (convert tests
    /// write under `<root>/.evani/convert`).
    async fn start_in(root: &str, models: Option<crate::models::Workspace>) -> (u16, Arc<App>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let dist = std::env::temp_dir().join(format!("evani-live-srv-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<html><head><title>g</title></head></html>").unwrap();
        let mut hub = Hub::new(HubInfo { port, root: root.into(), game: "sandbox".into(), session: "s".into() });
        if let Some(ws) = models {
            hub = hub.with_models(ws);
        }
        let hub = Arc::new(hub);
        hub.set_build(BuildState { phase: "idle", error: None, build_ms: 1 });
        let app = Arc::new(App::new(hub, TOKEN.into(), dist, None));
        let r = router(app.clone());
        tokio::spawn(async move { axum::serve(listener, r).await });
        (port, app)
    }

    /// Raw HTTP/1.1 so the test controls every header (Host included).
    async fn raw(port: u16, request: String) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(3), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    fn get(path: &str, host: &str, extra: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n")
    }

    fn post(host: &str, extra: &str, body: &str) -> String {
        format!(
            "POST /live/v1/cmd HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// `serve --app-dir`: files resolve inside the build folder or are not served.
    #[tokio::test]
    async fn an_outside_game_is_served_only_from_inside_its_build() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = std::env::temp_dir().join(format!("evani-live-contained-{}-{port}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dist = base.join("game").join("target").join("evani-live").join("sea").join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<html><head><title>g</title></head></html>").unwrap();
        std::fs::write(dist.join("sea.js"), "export {}").unwrap();
        std::fs::write(base.join("secret.txt"), "not for the page").unwrap();
        // A junction (Windows) or symlink inside the build that leads out of it.
        let link = dist.join("out");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&base).output().is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&base, &link).is_ok();
        assert!(made && link.join("secret.txt").is_file(), "could not make the test link");
        let hub = Arc::new(Hub::new(HubInfo { port, root: "C:\\e".into(), game: "sea".into(), session: "s".into() }).with_outside("C:\\g\\app".into()));
        hub.set_build(BuildState { phase: "idle", error: None, build_ms: 1 });
        let app = Arc::new(App::new(hub, TOKEN.into(), dist.clone(), None).contained());
        let r = router(app);
        tokio::spawn(async move { axum::serve(listener, r).await });
        let host = format!("localhost:{port}");
        assert!(raw(port, get("/", &host, "")).await.contains(TOKEN), "the page");
        assert!(raw(port, get("/sea.js", &host, "")).await.starts_with("HTTP/1.1 200"));
        for p in ["/out/secret.txt", "/../../../../../secret.txt", "/%2e%2e/%2e%2e/secret.txt", "/out/"] {
            let r = raw(port, get(p, &host, "")).await;
            assert!(!r.contains("not for the page") && !r.starts_with("HTTP/1.1 200"), "{p}: {r}");
        }
        #[cfg(windows)]
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn http_host_origin_and_token_rules() {
        let (port, _app) = start().await;
        let host = format!("localhost:{port}");
        // Status: open, right shape, no token in it.
        let r = raw(port, get("/live/status", &host, "")).await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert!(r.contains("\"type\":\"live_hello\"") && !r.contains(TOKEN));
        // DNS rebinding: wrong Host â†’ 421.
        let r = raw(port, get("/live/status", &format!("evil.example:{port}"), "")).await;
        assert!(r.starts_with("HTTP/1.1 421"), "{r}");
        assert!(r.contains("forbidden_origin"));
        // The page gets the token injected.
        let r = raw(port, get("/", &host, "")).await;
        assert!(r.contains(&format!("<meta name=\"evani-live-token\" content=\"{TOKEN}\">")), "{r}");
        // POST without token â†’ 401 unauthorized; wrong token too.
        let body = r#"{"type":"cmd","v":1,"id":"c","cmd":"schema.get"}"#;
        let r = raw(port, post(&host, "", body)).await;
        assert!(r.starts_with("HTTP/1.1 401") && r.contains("\"unauthorized\""), "{r}");
        let r = raw(port, post(&host, "Authorization: Bearer nope\r\n", body)).await;
        assert!(r.starts_with("HTTP/1.1 401"), "{r}");
        // With an Origin â†’ 403, even with the token.
        let auth = format!("Authorization: Bearer {TOKEN}\r\n");
        let r = raw(port, post(&host, &format!("{auth}Origin: http://localhost:{port}\r\n"), body)).await;
        assert!(r.starts_with("HTTP/1.1 403") && r.contains("forbidden_origin"), "{r}");
        // With the token, no page connected â†’ a no_engine result.
        let r = raw(port, post(&host, &auth, body)).await;
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("\"no_engine\""), "{r}");
    }

    /// The body after the headers.
    fn body_of(r: &str) -> &str {
        r.split_once("\r\n\r\n").map_or("", |b| b.1)
    }

    fn status_line(r: &str) -> &str {
        r.lines().next().unwrap_or("")
    }

    #[tokio::test]
    async fn game_bridge_unchanged_without_models_flag() {
        let (port, _app) = start().await;
        let host = format!("localhost:{port}");
        // /live/status has no `models` field.
        let r = raw(port, get("/live/status", &host, "")).await;
        let status: Value = serde_json::from_str(body_of(&r)).unwrap();
        assert!(status.get("models").is_none(), "{status}");
        // /model/v1/... is just another missing game file, byte for byte.
        let plain = raw(port, get("/no/such/file", &host, "")).await;
        for p in ["/model/v1/models", "/model/v1/kart-06b3/history", "/model/v1/kart-06b3/v1/kart.low.glb"] {
            let r = raw(port, get(p, &host, "")).await;
            assert_eq!((status_line(&r), body_of(&r)), (status_line(&plain), body_of(&plain)), "{p}");
        }
        // view.watch and model.* go to the page like any other command.
        let auth = format!("Authorization: Bearer {TOKEN}\r\n");
        let other = raw(port, post(&host, &auth, r#"{"type":"cmd","v":1,"id":"c","cmd":"schema.get"}"#)).await;
        for cmd in ["view.watch", "model.list", "model.history"] {
            let r = raw(port, post(&host, &auth, &format!(r#"{{"type":"cmd","v":1,"id":"c","cmd":"{cmd}"}}"#))).await;
            assert_eq!(body_of(&r), body_of(&other), "{cmd}");
        }
    }

    #[tokio::test]
    async fn model_mode_routes_and_commands() {
        let d = std::env::temp_dir().join(format!("evani-live-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let v1 = d.join("kart-06b3").join("v1");
        std::fs::create_dir_all(&v1).unwrap();
        std::fs::write(v1.join("kart.low.glb"), b"glTF-bytes").unwrap();
        std::fs::write(v1.join("notes.txt"), b"unlisted").unwrap();
        let vj = json!({ "format": 1, "model": "kart-06b3", "version": 1, "tiers": { "low": { "file": "kart.low.glb", "tris": 2830 } } });
        std::fs::write(v1.join("version.json"), vj.to_string()).unwrap();
        let (port, _app) = start_with(Some(crate::models::Workspace::new(d.clone(), ".evani/model".into()))).await;
        let host = format!("localhost:{port}");
        let r = raw(port, get("/live/status", &host, "")).await;
        let status: Value = serde_json::from_str(body_of(&r)).unwrap();
        assert_eq!(status["models"], json!({ "folder": ".evani/model", "count": 1, "latest": "kart-06b3" }));
        let r = raw(port, get("/model/v1/models", &host, "")).await;
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("\"kart-06b3\""), "{r}");
        let r = raw(port, get("/model/v1/kart-06b3/history", &host, "")).await;
        assert!(r.contains("\"tris\":2830"), "{r}");
        let r = raw(port, get("/model/v1/kart-06b3/v1/kart.low.glb", &host, "")).await;
        assert!(r.starts_with("HTTP/1.1 200") && body_of(&r) == "glTF-bytes", "{r}");
        // The page's own origin may read; a foreign one is refused.
        let r = raw(port, get("/model/v1/models", &host, &format!("Origin: http://localhost:{port}\r\n"))).await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        let r = raw(port, get("/model/v1/models", &host, "Origin: http://evil.example\r\n")).await;
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
        for p in ["/model/v1/kart-06b3/v1/notes.txt", "/model/v1/kart-06b3/v1/version.json", "/model/v1/kart-06b3/v1/../v1/kart.low.glb",
                  "/model/v1/kart-06b3/v1/%2e%2e%2fv1%2fkart.low.glb", "/model/v1/nope/history", "/model/v1/kart-06b3/v2/kart.low.glb"] {
            let r = raw(port, get(p, &host, "")).await;
            assert!(r.starts_with("HTTP/1.1 404"), "{p}: {r}");
        }
        let r = raw(port, get("/model/v1/requests/r-1", &host, "")).await;
        assert!(body_of(&r).contains("\"idle\""), "{r}");
        // model.* and view.watch are answered by the bridge with no page.
        let auth = format!("Authorization: Bearer {TOKEN}\r\n");
        let r = raw(port, post(&host, &auth, r#"{"type":"cmd","v":1,"id":"c","cmd":"model.list"}"#)).await;
        assert!(r.contains("\"ok\":true") && r.contains("\"kart-06b3\""), "{r}");
        let r = raw(port, post(&host, &auth, r#"{"type":"cmd","v":1,"id":"c","cmd":"model.history","params":{"model":"nope"}}"#)).await;
        assert!(r.contains("\"unknown_model\""), "{r}");
        let r = raw(port, post(&host, &auth, r#"{"type":"cmd","v":1,"id":"c","cmd":"view.watch"}"#)).await;
        assert!(r.contains("\"watching\":true"), "{r}");
        // view.load still goes to the page (none here).
        let r = raw(port, post(&host, &auth, r#"{"type":"cmd","v":1,"id":"c","cmd":"view.load","params":{"model":"kart-06b3"}}"#)).await;
        assert!(r.contains("\"no_engine\""), "{r}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn websocket_doors() {
        let (port, app) = start().await;
        let url = |p: &str| format!("ws://127.0.0.1:{port}{p}");
        // /ctl without the token: refused at the upgrade with 401.
        let e = tokio_tungstenite::connect_async(url("/ctl")).await.err().unwrap();
        assert!(e.to_string().contains("401"), "{e}");
        // /ctl from a foreign origin: 403 even with the token.
        let mut req = url("/ctl").into_client_request().unwrap();
        req.headers_mut().insert("Authorization", format!("Bearer {TOKEN}").parse().unwrap());
        req.headers_mut().insert("Origin", "http://evil.example".parse().unwrap());
        let e = tokio_tungstenite::connect_async(req).await.err().unwrap();
        assert!(e.to_string().contains("403"), "{e}");
        // /evani-dev: no token needed.
        let (mut dev, _) = tokio_tungstenite::connect_async(url("/evani-dev")).await.unwrap();
        // /engine from a non-page origin: 403.
        let e = tokio_tungstenite::connect_async(url("/engine")).await.err().unwrap();
        assert!(e.to_string().contains("403"), "{e}");
        // /engine with a wrong token: closed with 4401.
        let mut req = url("/engine").into_client_request().unwrap();
        req.headers_mut().insert("Origin", format!("http://localhost:{port}").parse().unwrap());
        let (mut bad, _) = tokio_tungstenite::connect_async(req.clone()).await.unwrap();
        bad.send(tokio_tungstenite::tungstenite::Message::text(r#"{"type":"auth","v":1,"token":"nope"}"#))
            .await
            .unwrap();
        match bad.next().await {
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(Some(f)))) => {
                assert_eq!(f.code, CloseCode::from(CLOSE_UNAUTHORIZED));
            }
            other => panic!("expected close 4401, got {other:?}"),
        }
        // The real page: auth, then it answers a command from /ctl.
        let (mut page, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        page.send(tokio_tungstenite::tungstenite::Message::text(
            json!({"type":"auth","v":1,"token":TOKEN}).to_string(),
        ))
        .await
        .unwrap();
        page.send(tokio_tungstenite::tungstenite::Message::text(
            r#"{"type":"hello","v":1,"schema_version":"cafe0001","tier":"medium"}"#,
        ))
        .await
        .unwrap();
        for _ in 0..50 {
            if app.hub.engine_connected() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut req = url("/ctl").into_client_request().unwrap();
        req.headers_mut().insert("Authorization", format!("Bearer {TOKEN}").parse().unwrap());
        req.headers_mut().insert("X-Evani-Client", "mcp".parse().unwrap());
        let (mut ctl, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        let hello: Value = serde_json::from_str(ctl.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["engine_connected"], true);
        assert_eq!(hello["schema_version"], "cafe0001");
        ctl.send(tokio_tungstenite::tungstenite::Message::text(
            r#"{"type":"cmd","v":1,"id":"c-42","cmd":"param.set","params":{"values":{"fx.bloom.threshold":1.5}},"note":"test"}"#,
        ))
        .await
        .unwrap();
        // The page receives it with a bridge id and source "mcp", answers.
        let got: Value = serde_json::from_str(page.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(got["cmd"], "param.set");
        assert_eq!(got["source"], "mcp");
        assert_ne!(got["id"], "c-42");
        page.send(tokio_tungstenite::tungstenite::Message::text(
            json!({"type":"result","v":1,"id":got["id"],"ok":true,
                   "result":{"applied":[{"kind":"param","name":"fx.bloom.threshold","old":1.0,"new":1.5}]}})
            .to_string(),
        ))
        .await
        .unwrap();
        let res: Value = serde_json::from_str(ctl.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(res["id"], "c-42");
        assert_eq!(res["ok"], true);
        // The tab heard about it.
        let news: Value = serde_json::from_str(dev.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(news["message"], "fx.bloom.threshold 1 â†’ 1.5 (AI)");
        // A chat_reply from the tab reaches the page.
        dev.send(tokio_tungstenite::tungstenite::Message::text(r#"{"type":"chat_reply","text":"done","from":"evani-dev"}"#))
            .await
            .unwrap();
        let got: Value = serde_json::from_str(page.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(got["type"], "chat_reply");
    }

    fn approve_req(method: &str, path: &str, host: &str, extra: &str, body: &str) -> String {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn approve_listener_rules() {
        use crate::lookguard::tests as lg;
        let fx = lg::fixture("http", 8096, true);
        lg::approved_guard(&fx);
        let r = lg::send(&fx.hub, "mcp", "look.limits.propose", json!({ "band_widen": { "fraction": 0.4 } }), "wider").await;
        let proposal = r["result"].clone();
        assert_eq!(proposal["status"], "pending", "{r}");
        let (mac, body) = lg::approval_for(&proposal, "limits");
        let body = body.to_string();
        let auth = format!("Authorization: Evani-Approve {mac}\r\n");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        fx.hub.lookguard.set_approve_port(port);
        assert_eq!(fx.hub.status()["look_guard"]["approve_port"], port);
        let r = approve_router(fx.hub.clone());
        tokio::spawn(async move { axum::serve(listener, r).await });
        let path = "/look-guard/v1/approve";
        let host = format!("127.0.0.1:{port}");

        // DNS rebinding and browsers: refused before anything is read.
        let r = raw(port, approve_req("POST", path, &format!("evil.example:{port}"), &auth, &body)).await;
        assert!(r.starts_with("HTTP/1.1 421"), "{r}");
        let r = raw(port, approve_req("POST", path, &host, &format!("{auth}Origin: http://localhost:{port}\r\n"), &body)).await;
        assert!(r.starts_with("HTTP/1.1 403") && r.contains("forbidden_origin"), "{r}");
        let r = raw(port, approve_req("GET", path, &host, &auth, "")).await;
        assert!(r.starts_with("HTTP/1.1 405"), "{r}");
        // Nothing else lives here (no live commands, no proxy).
        let r = raw(port, approve_req("POST", "/live/v1/cmd", &host, &auth, "{}")).await;
        assert!(r.starts_with("HTTP/1.1 404"), "{r}");
        // No or wrong MAC: 401; not JSON: 400; too big: 413.
        let r = raw(port, approve_req("POST", path, &host, "", &body)).await;
        assert!(r.starts_with("HTTP/1.1 401") && r.contains("bad_auth"), "{r}");
        let r = raw(port, approve_req("POST", path, &host, &format!("Authorization: Evani-Approve {}\r\n", "0".repeat(64)), &body)).await;
        assert!(r.starts_with("HTTP/1.1 401"), "{r}");
        let r = raw(port, approve_req("POST", path, &host, &auth, "nope")).await;
        assert!(r.starts_with("HTTP/1.1 400"), "{r}");
        let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(20 * 1024));
        let r = raw(port, approve_req("POST", path, &host, &auth, &big)).await;
        assert!(r.starts_with("HTTP/1.1 413"), "{r}");
        // Evani.App signs the record, then approves: 200, and the limits are live.
        lg::sign_record(&fx, proposal["hash"].as_str().unwrap(), 2);
        let r = raw(port, approve_req("POST", path, &format!("localhost:{port}"), &auth, &body)).await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        let json_body: Value = serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(json_body["verify"]["ok"], true, "{r}");
        assert_eq!(json_body["verify"]["seq"], 2);
        // Once only.
        let r = raw(port, approve_req("POST", path, &host, &auth, &body)).await;
        assert!(r.starts_with("HTTP/1.1 409") && r.contains("already_used"), "{r}");

        // The main live server doesn't serve approvals at all.
        let (main_port, _app) = start().await;
        let r = raw(main_port, approve_req("POST", path, &format!("localhost:{main_port}"), &auth, &body)).await;
        assert!(r.starts_with("HTTP/1.1 405"), "{r}");
    }

    // ---- drop-to-convert (16) ----------------------------------------------------------

    /// A server on a temp root whose converter is `runner` (nothing real
    /// is spawned): the root, the port and the app.
    async fn convert_server(name: &str, runner: crate::convert::Runner) -> (PathBuf, u16, Arc<App>) {
        let root = std::env::temp_dir().join(format!("evani-live-conv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (port, app) = start_in(&root.to_string_lossy(), None).await;
        app.hub.convert.set_runner(runner);
        (root, port, app)
    }

    /// A runner that writes what evani-convert writes, after `gate` opens.
    fn fake_runner(calls: Arc<std::sync::atomic::AtomicUsize>, gate: Option<Arc<tokio::sync::Semaphore>>) -> crate::convert::Runner {
        Arc::new(move |spec: crate::convert::RunSpec| {
            let (calls, gate) = (calls.clone(), gate.clone());
            Box::pin(async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(g) = gate {
                    let _ = g.acquire().await.map_err(|e| e.to_string())?;
                }
                assert!(spec.input.is_file(), "the input copy exists while the job runs");
                let id = &spec.id;
                std::fs::write(spec.out.join(format!("{id}.master.glb")), b"glTF-master").map_err(|e| e.to_string())?;
                std::fs::write(spec.out.join(format!("{id}.low.glb")), b"glTF-low").map_err(|e| e.to_string())?;
                let report = json!({ "id": id, "master": format!("{id}.master.glb"),
                    "tiers": [{ "tier": "low", "file": format!("{id}.low.glb"), "bytes": { "raw": 8, "gzip": 20 }, "draw_calls": 1, "lods": [{ "triangles": 12 }] }],
                    "package": { "raw": 8, "gzip": 20, "cap": 10485760, "over_cap": false } });
                std::fs::write(spec.out.join(format!("{id}.report.json")), report.to_string()).map_err(|e| e.to_string())?;
                Ok(0)
            }) as crate::convert::RunFuture
        })
    }

    fn upload(port: u16, extra: &str, query: &str, body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "POST /live/v1/convert{query} HTTP/1.1\r\nHost: localhost:{port}\r\n{extra}Content-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    async fn raw_bytes(port: u16, request: Vec<u8>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(&request).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Wait until the slot is free again (the fake job has finished).
    async fn until_idle(app: &App) {
        for _ in 0..300 {
            if app.hub.convert.running().is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the convert job never finished");
    }

    #[tokio::test]
    async fn convert_upload_needs_the_token() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (root, port, app) = convert_server("token", fake_runner(calls.clone(), None)).await;
        let glb = b"glTF\x02\0\0\0 a model";
        let bearer = format!("Authorization: Bearer {TOKEN}\r\n");
        // No token, a wrong token, a wrong cookie: 401, nothing converted.
        for extra in ["".to_string(), "Authorization: Bearer nope\r\n".into(), format!("Cookie: evani_live_{port}=nope\r\n"), format!("Cookie: evani_live_1={TOKEN}\r\n")] {
            let r = raw_bytes(port, upload(port, &extra, "?id=crate", glb)).await;
            assert!(r.starts_with("HTTP/1.1 401") && r.contains("unauthorized"), "{extra}: {r}");
        }
        // A foreign page: 403 even with the token.
        let r = raw_bytes(port, upload(port, &format!("{bearer}Origin: http://evil.example\r\n"), "", glb)).await;
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        // The page's index.html carries the token as an HttpOnly cookie for
        // the convert routes only.
        let r = raw(port, get("/", &format!("localhost:{port}"), "")).await;
        let set = format!("set-cookie: evani_live_{port}={TOKEN}; HttpOnly; SameSite=Strict; Path=/live/v1/convert");
        assert!(r.to_ascii_lowercase().contains(&set.to_ascii_lowercase()), "{r}");
        // With the cookie and the page's own origin: accepted (202).
        let page = format!("Cookie: evani_live_{port}={TOKEN}\r\nOrigin: http://localhost:{port}\r\n");
        let r = raw_bytes(port, upload(port, &page, "?id=crate", glb)).await;
        assert!(r.starts_with("HTTP/1.1 202"), "{r}");
        let accepted: Value = serde_json::from_str(body_of(&r)).unwrap();
        let hash = accepted["hash"].as_str().unwrap().to_string();
        assert_eq!(accepted["id"], "crate");
        until_idle(&app).await;
        // The input copy is gone; the results stay.
        let folder = root.join(".evani").join("convert").join(&hash);
        assert!(!folder.join("in").exists(), "the uploaded copy is removed");
        assert!(folder.join("out").join("crate.low.glb").is_file());
        // Files are served only with the token.
        let file = format!("/live/v1/convert/{hash}/crate.low.glb");
        let r = raw(port, get(&file, &format!("localhost:{port}"), "")).await;
        assert!(r.starts_with("HTTP/1.1 401"), "{r}");
        let r = raw(port, get(&file, &format!("localhost:{port}"), &format!("Cookie: evani_live_{port}={TOKEN}\r\n"))).await;
        assert!(r.starts_with("HTTP/1.1 200") && body_of(&r) == "glTF-low" && r.contains("model/gltf-binary"), "{r}");
        for bad in [format!("/live/v1/convert/{hash}/../job.json"), format!("/live/v1/convert/{hash}/%2e%2e/job.json"), "/live/v1/convert/../../Cargo.toml".into(), format!("/live/v1/convert/{hash}/")] {
            let r = raw(port, get(&bad, &format!("localhost:{port}"), &bearer)).await;
            assert!(r.starts_with("HTTP/1.1 404") || r.starts_with("HTTP/1.1 400"), "{bad}: {r}");
        }
        // model.report answers with the job and its report.
        let r = raw(port, post(&format!("localhost:{port}"), &bearer, r#"{"type":"cmd","v":1,"id":"c","cmd":"model.report"}"#)).await;
        let rep: Value = serde_json::from_str(body_of(&r)).unwrap();
        assert_eq!(rep["ok"], true, "{rep}");
        assert_eq!(rep["result"]["job"]["hash"], hash.as_str());
        assert_eq!(rep["result"]["job"]["report"]["tiers"][0]["triangles"], 12);
        assert_eq!(rep["result"]["job"]["master"], "crate.master.glb");
        // A bad id is refused; something that isn't a model too.
        let r = raw_bytes(port, upload(port, &bearer, "?id=..%2Fx", glb)).await;
        assert!(r.starts_with("HTTP/1.1 400"), "{r}");
        let r = raw_bytes(port, upload(port, &bearer, "", b"<html>nope</html>")).await;
        assert!(r.starts_with("HTTP/1.1 415"), "{r}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Both protective headers, on any convert answer.
    fn hardened_answer(r: &str) -> bool {
        let head = r.split("\r\n\r\n").next().unwrap_or("").to_ascii_lowercase();
        head.contains("x-content-type-options: nosniff") && head.contains("cross-origin-resource-policy: same-origin")
    }

    #[tokio::test]
    async fn convert_routes_refuse_other_sites() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (root, port, app) = convert_server("site", fake_runner(calls.clone(), None)).await;
        let glb = b"glTF\x02\0\0\0 a site model";
        let host = format!("localhost:{port}");
        let bearer = format!("Authorization: Bearer {TOKEN}\r\n");
        let cookie = format!("Cookie: evani_live_{port}={TOKEN}\r\n");
        // Another localhost page (same-site: cookies ignore the port) or
        // another site: 403 even with the cookie or the token.
        for site in ["same-site", "cross-site", "something-else"] {
            for auth in [&bearer, &cookie] {
                let extra = format!("{auth}Sec-Fetch-Site: {site}\r\n");
                let r = raw_bytes(port, upload(port, &extra, "?id=site", glb)).await;
                assert!(r.starts_with("HTTP/1.1 403") && r.contains("forbidden_origin") && hardened_answer(&r), "{site}: {r}");
                let r = raw(port, get("/live/v1/convert/0123456789abcdef0123456789abcdef/site.low.glb", &host, &extra)).await;
                assert!(r.starts_with("HTTP/1.1 403") && hardened_answer(&r), "{site}: {r}");
            }
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing was converted");
        // A token refusal carries the headers too.
        let r = raw_bytes(port, upload(port, "Sec-Fetch-Site: same-origin\r\n", "?id=site", glb)).await;
        assert!(r.starts_with("HTTP/1.1 401") && hardened_answer(&r), "{r}");
        // The page itself (same-origin): accepted.
        let page = format!("{cookie}Origin: http://localhost:{port}\r\nSec-Fetch-Site: same-origin\r\n");
        let r = raw_bytes(port, upload(port, &page, "?id=site", glb)).await;
        assert!(r.starts_with("HTTP/1.1 202") && hardened_answer(&r), "{r}");
        let hash = serde_json::from_str::<Value>(body_of(&r)).unwrap()["hash"].as_str().unwrap().to_string();
        until_idle(&app).await;
        // A file opened from the address bar (none) or by an agent (no header).
        let file = format!("/live/v1/convert/{hash}/site.low.glb");
        for extra in [format!("{cookie}Sec-Fetch-Site: none\r\n"), bearer.clone()] {
            let r = raw(port, get(&file, &host, &extra)).await;
            assert!(r.starts_with("HTTP/1.1 200") && body_of(&r) == "glTF-low" && hardened_answer(&r), "{extra}: {r}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn oversize_upload_is_refused() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (root, port, _app) = convert_server("big", fake_runner(calls.clone(), None)).await;
        // Declared over the cap: refused from the headers alone (no body sent).
        let head = format!(
            "POST /live/v1/convert HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            crate::convert::MAX_UPLOAD + 1
        );
        let r = raw_bytes(port, head.into_bytes()).await;
        assert!(r.starts_with("HTTP/1.1 413") && r.contains("too_large"), "{r}");
        // A chunked body that grows past the cap is cut off while reading.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let head = format!("POST /live/v1/convert HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: Bearer {TOKEN}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
        s.write_all(head.as_bytes()).await.unwrap();
        let chunk = vec![b'g'; 1024 * 1024];
        let mut sent = 0usize;
        while sent <= crate::convert::MAX_UPLOAD {
            let mut c = format!("{:x}\r\n", chunk.len()).into_bytes();
            c.extend_from_slice(&chunk);
            c.extend_from_slice(b"\r\n");
            if s.write_all(&c).await.is_err() {
                break; // the server stopped reading: refused
            }
            sent += chunk.len();
        }
        let _ = s.write_all(b"0\r\n\r\n").await;
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
        let r = String::from_utf8_lossy(&out);
        assert!(r.is_empty() || r.starts_with("HTTP/1.1 413"), "{r}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing was converted");
        assert!(!root.join(".evani").join("convert").exists(), "nothing was written");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn one_conversion_at_a_time() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let (root, port, app) = convert_server("one", fake_runner(calls.clone(), Some(gate.clone()))).await;
        // The tab hears progress and the result.
        let (mut dev, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/evani-dev")).await.unwrap();
        let bearer = format!("Authorization: Bearer {TOKEN}\r\n");
        let r = raw_bytes(port, upload(port, &bearer, "?id=first", b"glTF\x02 one")).await;
        assert!(r.starts_with("HTTP/1.1 202"), "{r}");
        // While it runs, a second upload and a model.convert are busy.
        let r = raw_bytes(port, upload(port, &bearer, "?id=second", b"glTF\x02 two")).await;
        assert!(r.starts_with("HTTP/1.1 409") && r.contains("\"busy\""), "{r}");
        let local = root.join("local.glb");
        std::fs::write(&local, b"glTF\x02 three").unwrap();
        let cmd = json!({"type":"cmd","v":1,"id":"c","cmd":"model.convert","params":{"path": local.to_string_lossy()}}).to_string();
        let r = raw(port, post(&format!("localhost:{port}"), &bearer, &cmd)).await;
        assert!(r.contains("\"busy\""), "{r}");
        gate.add_permits(1);
        until_idle(&app).await;
        let mut seen = Vec::new();
        while let Ok(Some(Ok(m))) = tokio::time::timeout(Duration::from_millis(500), dev.next()).await {
            let v: Value = serde_json::from_str(m.to_text().unwrap()).unwrap();
            seen.push(if v["type"] == "event" { v["event"].as_str().unwrap().to_string() } else { format!("{}:{}", v["type"].as_str().unwrap_or(""), v["state"].as_str().unwrap_or("")) });
            if v["event"] == "convert_done" {
                assert_eq!(v["data"]["ok"], true, "{v}");
                assert_eq!(v["data"]["id"], "first");
                assert!(v["data"]["base_url"].as_str().unwrap().starts_with("/live/v1/convert/"));
            }
        }
        assert!(seen.contains(&"convert_progress".to_string()) && seen.contains(&"convert_done".to_string()), "{seen:?}");
        assert!(seen.contains(&"live_status:applied".to_string()), "{seen:?}");
        assert!(!seen.iter().any(|s| s.ends_with(":error")), "{seen:?}");
        // Free again: the local file converts (model.convert answers at once).
        gate.add_permits(1);
        let r = raw(port, post(&format!("localhost:{port}"), &bearer, &cmd)).await;
        assert!(r.contains("\"ok\":true") && r.contains("\"id\":\"local\""), "{r}");
        until_idle(&app).await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn zip_slip_upload_is_refused() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (root, port, app) = convert_server("slip", fake_runner(calls.clone(), None)).await;
        let bearer = format!("Authorization: Bearer {TOKEN}\r\n");
        let zip = crate::convert::tests::zip_of(&[("model.gltf", b"{}"), ("../../../evil.txt", b"pwned")]);
        let r = raw_bytes(port, upload(port, &bearer, "?id=slip", &zip)).await;
        assert!(r.starts_with("HTTP/1.1 400") && r.contains("unsafe_zip"), "{r}");
        // A .gltf whose buffer points outside the zip: refused too.
        let zip = crate::convert::tests::zip_of(&[("model.gltf", br#"{"buffers":[{"uri":"../../secret.bin"}]}"#)]);
        let r = raw_bytes(port, upload(port, &bearer, "?id=slip", &zip)).await;
        assert!(r.starts_with("HTTP/1.1 400") && r.contains("unsafe_zip"), "{r}");
        // Nothing escaped, nothing ran, the unpack folder is gone, the slot is free.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        for p in [root.join("evil.txt"), root.join(".evani").join("evil.txt"), root.parent().unwrap().join("evil.txt")] {
            assert!(!p.exists(), "{}", p.display());
        }
        assert!(app.hub.convert.running().is_none());
        // A good zip converts.
        let zip = crate::convert::tests::zip_of(&[("m/model.gltf", br#"{"buffers":[{"uri":"model.bin"}]}"#), ("m/model.bin", b"bin")]);
        let r = raw_bytes(port, upload(port, &bearer, "?id=zipped", &zip)).await;
        assert!(r.starts_with("HTTP/1.1 202"), "{r}");
        until_idle(&app).await;
        let hash = serde_json::from_str::<Value>(body_of(&r)).unwrap()["hash"].as_str().unwrap().to_string();
        let folder = root.join(".evani").join("convert").join(hash);
        assert!(!folder.join("src").exists(), "the unpacked zip is removed");
        assert!(folder.join("out").join("zipped.report.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
