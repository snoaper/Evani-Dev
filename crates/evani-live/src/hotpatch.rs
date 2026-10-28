//! Live code swapping, the page half (dev builds only, feature `hotpatch`;
//! docs/LIVE_API.md 10). Browser only.
//!
//! The bridge runs `dx serve --hot-patch`, and when a saved Rust file
//! compiles into a patch it sends `code_patch` with subsecond's jump table
//! over the existing `/engine` socket, the patch wasm served from its own
//! origin. Here:
//!
//! 1. The patch is fetched and compiled first, every error caught, and its
//!    imports are checked against the running module. Nothing about the
//!    game changes until all of that passed.
//! 2. At the top of the next frame (evani-core's before-frame hook) it is
//!    handed to `subsecond::apply_patch`, which instantiates it and commits
//!    the jump table in a later task, so the switch lands between frames.
//!    Every system, module frame and scene update already runs through the
//!    jump table (evani-core `hotpatch`), so no game needs code of its own.
//! 3. The commit handler reports `code.patched` and shows a small toast.
//!
//! A patch that fails to load is reported (`code.failed`) and the old code
//! keeps running. A panic once a patch is in use stops the wasm (panics
//! abort on wasm), so the page reports `code.crashed` and reloads; the
//! bridge then serves the last patch that worked (and never the one that
//! crashed), at most twice a minute.
//!
//! The `/_dioxus` message shapes and the patch-loading steps follow
//! dioxus-devtools and subsecond 0.7.10 (Copyright (c) Dioxus Labs,
//! MIT OR Apache-2.0, https://github.com/DioxusLabs/dioxus); this is a
//! re-write without the dioxus dependencies.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use js_sys::{Array, Object, Reflect, WebAssembly};
use serde_json::{Value, json};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::ws::Link;

/// Extra table slots on top of the bridge's exact count (dx main's own
/// count can be short; spare slots are only leaked).
const IFUNC_HEADROOM: u64 = 16;
/// Auto-reloads after a crash allowed per minute (sessionStorage).
const MAX_CRASH_RELOADS: u32 = 2;
const TOAST_ID: &str = "evani-code-toast";

/// A patch ready to hand to subsecond.
struct Ready {
    seq: u64,
    table: subsecond::JumpTable,
    functions: usize,
    started_at: f64,
    build_ms: f64,
    blob_url: String,
}

/// The patch subsecond is loading now.
struct Applying {
    seq: u64,
    functions: usize,
    started_at: f64,
    build_ms: f64,
    blob_url: String,
}

#[derive(Default)]
struct State {
    link: Option<Rc<Link>>,
    /// The newest `code_patch` seq seen; older ones still preparing are dropped.
    newest: u64,
    ready: Option<Ready>,
    applying: Option<Applying>,
    /// The seq of the patch the page runs (0: the build as served).
    active: u64,
    applied: u32,
    failed: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn now_epoch() -> f64 {
    js_sys::Date::now()
}

fn log(msg: &str) {
    web_sys::console::info_1(&JsValue::from_str(&format!("[Evani.dev code] {msg}")));
}

/// `window.__evani_code`: counters for tests and the tab's own page view.
fn page_state(k: &str, v: &JsValue) {
    let Some(win) = web_sys::window() else { return };
    let key = JsValue::from_str("__evani_code");
    let o = Reflect::get(&win, &key)
        .ok()
        .filter(JsValue::is_object)
        .unwrap_or_else(|| {
            let o: JsValue = Object::new().into();
            let _ = Reflect::set(&win, &key, &o);
            o
        });
    let _ = Reflect::set(&o, &JsValue::from_str(k), v);
    if k == "last" {
        let hk = JsValue::from_str("history");
        let hist = Reflect::get(&o, &hk)
            .ok()
            .and_then(|h| h.dyn_into::<Array>().ok())
            .unwrap_or_else(|| {
                let a = Array::new();
                let _ = Reflect::set(&o, &hk, &a);
                a
            });
        hist.push(v);
    }
}

fn record(v: &Value) {
    if let Ok(js) = js_sys::JSON::parse(&v.to_string()) {
        page_state("last", &js);
    }
    STATE.with(|s| {
        if let Ok(s) = s.try_borrow() {
            page_state("applied", &JsValue::from(s.applied));
            page_state("failed", &JsValue::from(s.failed));
            page_state("active", &JsValue::from(s.active as f64));
        }
    });
}

/// Send an event to the bridge (`{"type":"event","event":…}`).
fn send_event(event: &str, data: Value) {
    let env = json!({ "type": "event", "v": 1, "event": event, "data": data });
    let link = STATE.with(|s| s.try_borrow().ok().and_then(|s| s.link.clone()));
    if let Some(l) = link {
        l.try_send_text(&env.to_string());
    }
}

// ---- toast ------------------------------------------------------------------------

thread_local! {
    static TOAST_TIMER: RefCell<Option<i32>> = const { RefCell::new(None) };
}

/// Show the dev toast. `error` keeps it up (click to close) and shows the
/// text as preformatted lines; otherwise it fades after a few seconds.
fn toast(text: &str, error: bool) {
    let Some(win) = web_sys::window() else { return };
    let Some(doc) = win.document() else { return };
    let el = match doc.get_element_by_id(TOAST_ID) {
        Some(e) => e,
        None => {
            let Ok(e) = doc.create_element("pre") else { return };
            e.set_id(TOAST_ID);
            let hide = Closure::<dyn FnMut()>::new(|| {
                if let Some(e) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id(TOAST_ID))
                {
                    let _ = e.set_attribute("data-shown", "0");
                    if let Some(h) = e.dyn_ref::<web_sys::HtmlElement>() {
                        let _ = h.style().set_property("display", "none");
                    }
                }
            });
            if let Some(h) = e.dyn_ref::<web_sys::HtmlElement>() {
                h.set_onclick(Some(hide.as_ref().unchecked_ref()));
            }
            hide.forget();
            if let Some(b) = doc.body() {
                let _ = b.append_child(&e);
            }
            e
        }
    };
    let (bg, fg, border) = if error {
        ("#2a1416", "#ffd6d6", "#d9534f")
    } else {
        ("#10261a", "#d8f5e3", "#3fb950")
    };
    let style = format!(
        "position:fixed;left:12px;bottom:12px;z-index:2147483000;max-width:min(46rem,90vw);\
         max-height:40vh;overflow:auto;margin:0;padding:8px 12px;border-radius:8px;\
         border:1px solid {border};background:{bg};color:{fg};\
         font:12px/1.45 ui-monospace,Consolas,monospace;white-space:pre-wrap;\
         box-shadow:0 4px 16px rgba(0,0,0,.4);cursor:pointer;pointer-events:auto"
    );
    let _ = el.set_attribute("style", &style);
    let _ = el.set_attribute("data-kind", if error { "error" } else { "ok" });
    let _ = el.set_attribute("data-shown", "1");
    el.set_text_content(Some(text));
    TOAST_TIMER.with(|t| {
        if let Some(id) = t.borrow_mut().take() {
            win.clear_timeout_with_handle(id);
        }
        if !error {
            let cb = Closure::once_into_js(|| {
                if let Some(e) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id(TOAST_ID))
                    && let Some(h) = e.dyn_ref::<web_sys::HtmlElement>()
                {
                    let _ = e.set_attribute("data-shown", "0");
                    let _ = h.style().set_property("display", "none");
                }
            });
            if let Ok(id) = win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 4000) {
                *t.borrow_mut() = Some(id);
            }
        }
    });
}

// ---- install ------------------------------------------------------------------------

/// Hook the engine's frame top, subsecond's commit and the panic hook.
/// Called once from `LiveModule::build` on the page.
pub fn install(link: Option<Rc<Link>>) {
    STATE.with(|s| s.borrow_mut().link = link);
    evani_core::hotpatch::set_before_frame(before_frame);
    subsecond::register_handler(Arc::new(on_commit));
    // evani-core's own hook first (console and overlay), then ours on top.
    evani_core::web::init();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loader = info.location().is_some_and(|l| l.file().contains("subsecond"));
        let (applying, active) = STATE.with(|s| {
            s.try_borrow()
                .map_or((false, 0), |s| (s.applying.is_some(), s.active))
        });
        if loader && applying {
            // subsecond's loader task failed; the game itself is fine.
            web_sys::console::warn_1(&JsValue::from_str(&format!("[Evani.dev code] patch did not load: {info}")));
            patch_failed(&format!("The patch did not load: {info}"));
            return;
        }
        prev(info);
        if active > 0 || applying {
            crashed(&info.to_string(), active);
        }
    }));
    page_state("hot", &JsValue::TRUE);
    record(&json!({ "state": "ready" }));
}

/// The `code` part of the page's `hello`: lets the bridge know this page
/// can take patches and which one it runs.
pub fn hello() -> Value {
    let active = STATE.with(|s| s.try_borrow().map_or(0, |s| s.active));
    json!({ "hot": true, "seq": active })
}

// ---- messages from the bridge -----------------------------------------------------------

/// `code_patch`, `code_patching`, `code_error`, `code_reload` (LIVE_API.md 10.4).
pub fn on_message(msg: &Value) {
    match msg["type"].as_str() {
        Some("code_patching") => toast("Code change saved: building the patch…", false),
        Some("code_error") => {
            let m = msg["message"].as_str().unwrap_or("The code change did not compile.");
            toast(&format!("Code change not applied: it doesn't compile yet. The game keeps running.\n\n{m}"), true);
            record(&json!({ "state": "error", "message": m }));
        }
        Some("code_reload") => {
            let reason = msg["reason"].as_str().unwrap_or("This change needs a fresh start.");
            toast(&format!("Reloading: {reason}"), false);
            record(&json!({ "state": "reload", "reason": reason }));
            reload_soon(600);
        }
        Some("code_patch") => receive(msg),
        _ => {}
    }
}

fn receive(msg: &Value) {
    let seq = msg["seq"].as_u64().unwrap_or(0);
    let table: subsecond::JumpTable = match serde_json::from_value(msg["table"].clone()) {
        Ok(t) => t,
        Err(e) => {
            patch_failed(&format!("bad jump table: {e}"));
            return;
        }
    };
    let stale = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if seq <= s.active || seq < s.newest {
            return true;
        }
        s.newest = seq;
        false
    });
    if stale {
        return;
    }
    let started_at = msg["started_at"].as_f64().unwrap_or_else(now_epoch);
    let build_ms = msg["build_ms"].as_f64().unwrap_or(0.0);
    let functions = msg["functions"].as_u64().map_or(table.map.len(), |n| n as usize);
    log(&format!("patch {seq} received ({functions} functions)"));
    wasm_bindgen_futures::spawn_local(async move {
        match prepare(table).await {
            Ok((table, blob_url)) => STATE.with(|s| {
                let mut s = s.borrow_mut();
                if s.newest != seq {
                    let _ = web_sys::Url::revoke_object_url(&blob_url);
                    return;
                }
                if let Some(old) = s.ready.take() {
                    let _ = web_sys::Url::revoke_object_url(&old.blob_url);
                }
                s.ready = Some(Ready { seq, table, functions, started_at, build_ms, blob_url });
            }),
            Err(e) => patch_failed(&e),
        }
    });
}

fn js_text(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())))
        .unwrap_or_else(|| format!("{e:?}"))
}

/// Fetch and compile the patch and check its imports, catching every error,
/// then keep its bytes in a blob URL so subsecond's own fetch can't fail.
async fn prepare(mut table: subsecond::JumpTable) -> Result<(subsecond::JumpTable, String), String> {
    let win = web_sys::window().ok_or("no window")?;
    let url = table.lib.to_string_lossy().replace('\\', "/");
    let resp: web_sys::Response = JsFuture::from(win.fetch_with_str(&url))
        .await
        .map_err(|e| format!("fetching {url}: {}", js_text(&e)))?
        .dyn_into()
        .map_err(|_| "fetch did not return a Response".to_string())?;
    if !resp.ok() {
        return Err(format!("fetching {url}: HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(|e| js_text(&e))?)
        .await
        .map_err(|e| format!("reading {url}: {}", js_text(&e)))?;
    let module: WebAssembly::Module = JsFuture::from(WebAssembly::compile(&buf))
        .await
        .map_err(|e| format!("the patch does not compile in the browser: {}", js_text(&e)))?
        .dyn_into()
        .map_err(|_| "compile did not return a Module".to_string())?;
    // Every import must be one the running module exports (or one of the
    // two bases subsecond adds); otherwise instantiation would fail.
    let exports: JsValue = wasm_bindgen::exports();
    for imp in WebAssembly::Module::imports(&module).iter() {
        let m = Reflect::get(&imp, &"module".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        let n = Reflect::get(&imp, &"name".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        let known = m == "env"
            && (n == "__memory_base"
                || n == "__table_base"
                || Reflect::has(&exports, &JsValue::from_str(&n)).unwrap_or(false));
        if !known {
            return Err(format!(
                "the patch needs {m}.{n}, which the running build doesn't have; a full rebuild is needed"
            ));
        }
    }
    let parts = Array::of1(&buf);
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/wasm");
    let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &opts).map_err(|e| js_text(&e))?;
    let blob_url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| js_text(&e))?;
    // subsecond only loads paths ending in `.wasm`; a blob URL's fragment
    // is ignored when it is fetched.
    table.lib = format!("{blob_url}#patch.wasm").into();
    table.ifunc_count += IFUNC_HEADROOM;
    Ok((table, blob_url))
}

/// Engine before-frame hook: hand a prepared patch to subsecond.
pub(crate) fn before_frame() {
    let ready = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.applying.is_some() {
            return None;
        }
        s.ready.take()
    });
    let Some(r) = ready else { return };
    STATE.with(|s| {
        s.borrow_mut().applying = Some(Applying {
            seq: r.seq,
            functions: r.functions,
            started_at: r.started_at,
            build_ms: r.build_ms,
            blob_url: r.blob_url.clone(),
        });
    });
    log(&format!("applying patch {} ({} functions)", r.seq, r.functions));
    // SAFETY: the table comes from our own bridge for this exact build
    // (dx's fat build that served this page), its module compiled and its
    // imports all resolve. Signature or layout changes make dx ask for a
    // full rebuild instead of a patch.
    if let Err(e) = unsafe { subsecond::apply_patch(r.table) } {
        patch_failed(&e.to_string());
    }
}

/// subsecond's commit handler: the new jump table is live.
fn on_commit() {
    let done = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let a = s.applying.take()?;
        s.active = a.seq;
        s.applied += 1;
        Some(a)
    });
    let Some(a) = done else { return };
    let _ = web_sys::Url::revoke_object_url(&a.blob_url);
    let ms = (now_epoch() - a.started_at).max(0.0);
    let secs = ms / 1000.0;
    let s = if a.functions == 1 { "" } else { "s" };
    toast(&format!("Code updated: {} function{s}, {secs:.1} s, no reload", a.functions), false);
    log(&format!("patch {} live ({} functions, {secs:.1} s from save)", a.seq, a.functions));
    let data = json!({
        "seq": a.seq, "functions": a.functions, "ms": ms.round(), "build_ms": a.build_ms,
    });
    record(&json!({ "state": "applied", "seq": a.seq, "functions": a.functions, "ms": ms.round() }));
    send_event("code.patched", data);
}

fn patch_failed(message: &str) {
    let seq = STATE.with(|s| {
        s.try_borrow_mut().map_or(0, |mut s| {
            s.failed += 1;
            let a = s.applying.take();
            if let Some(a) = &a {
                let _ = web_sys::Url::revoke_object_url(&a.blob_url);
            }
            a.map_or(s.newest, |a| a.seq)
        })
    });
    log(&format!("patch {seq} not applied: {message}"));
    toast(&format!("Code change not applied; the game keeps running the old code.\n\n{message}"), true);
    record(&json!({ "state": "failed", "seq": seq, "message": message }));
    send_event("code.failed", json!({ "seq": seq, "message": message }));
}

/// A panic with patched code in use: report, then reload (rate limited).
fn crashed(message: &str, seq: u64) {
    send_event("code.crashed", json!({ "seq": seq, "message": message }));
    toast(&format!("The game stopped in changed code. Reloading with the last code that worked.\n\n{message}"), true);
    let Some(win) = web_sys::window() else { return };
    let store = win.session_storage().ok().flatten();
    let now = now_epoch();
    let mut recent: Vec<f64> = store
        .as_ref()
        .and_then(|st| st.get_item("evani-code-crash-reloads").ok().flatten())
        .map(|t| t.split(',').filter_map(|x| x.parse().ok()).filter(|t: &f64| now - t < 60_000.0).collect())
        .unwrap_or_default();
    if recent.len() as u32 >= MAX_CRASH_RELOADS {
        log("crashed again within a minute: not reloading by itself");
        return;
    }
    recent.push(now);
    if let Some(st) = &store {
        let text: Vec<String> = recent.iter().map(|t| format!("{t:.0}")).collect();
        let _ = st.set_item("evani-code-crash-reloads", &text.join(","));
    }
    reload_soon(1200);
}

fn reload_soon(ms: i32) {
    let Some(win) = web_sys::window() else { return };
    let cb = Closure::once_into_js(|| {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    });
    let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), ms);
}
