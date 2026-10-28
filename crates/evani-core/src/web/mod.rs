//! Browser glue (wasm32 only): the requestAnimationFrame runner, DOM input,
//! Gamepad API polling, Web Audio, localStorage, fetch, the overlay element,
//! the panic hook and the full-screen message.
//!
//! Rule: no `unwrap`/`expect` on browser calls. Failures are logged to the
//! overlay ([`crate::overlay::ErrorLog`]) or the console and the game keeps
//! running with the feature missing.

mod audio;
mod runner; #[cfg(feature = "pointer-lock")] mod pointer_lock;
#[cfg(feature = "touch")]
mod touch;

use core::future::Future;
use core::pin::Pin;
use std::sync::Once;

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::assets::AssetSource;
use crate::save::{KeyValueStore, SaveError};

pub use audio::{AudioUnlocker, WebAudio};
pub use runner::run;

/// Id of the overlay `<pre>` element (created if the page has none).
pub const OVERLAY_ID: &str = "evani-overlay";
/// Id of the full-screen message element.
pub const MESSAGE_ID: &str = "evani-message";

thread_local! {
    static PERF: Option<web_sys::Performance> =
        web_sys::window().and_then(|w| w.performance());
}

/// `performance.now()` in ms (0 if unavailable).
pub fn performance_now() -> f64 {
    PERF.with(|p| p.as_ref().map_or(0.0, web_sys::Performance::now))
}

/// Log to the browser console.
pub fn console_log(error: bool, msg: &str) {
    let v = JsValue::from_str(msg);
    if error {
        web_sys::console::error_1(&v);
    } else {
        web_sys::console::log_1(&v);
    }
}

/// Short text for a JS error value.
pub fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| {
            e.dyn_ref::<js_sys::Error>()
                .map(|e| String::from(e.message()))
        })
        .unwrap_or_else(|| format!("{e:?}"))
}

static PANIC_HOOK: Once = Once::new();

/// Install the panic hook: console message with stack (console_error_panic_hook)
/// plus the panic text in the overlay element, so a crash is visible on the
/// page rather than a frozen frame. Idempotent.
pub fn init() {
    PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            console_error_panic_hook::hook(info); #[cfg(feature = "pointer-lock")] pointer_lock::exit_now(); // a crashed page must never keep the mouse captured
            if let Some(el) = overlay_element() {
                let msg = format!("Evani.Dev stopped: {info}");
                el.set_text_content(Some(&msg));
                let _ = el.set_attribute("style", &overlay_style(true, true));
            }
        }));
    });
}

pub(crate) fn overlay_style(visible: bool, error: bool) -> String {
    format!(
        "position:fixed;left:0;top:0;margin:0;padding:6px 8px;z-index:1000;\
         font:12px/1.35 ui-monospace,Consolas,monospace;white-space:pre;\
         pointer-events:none;border-bottom-right-radius:6px;\
         background:rgba(0,0,0,0.6);color:{};display:{}",
        if error { "#ff8080" } else { "#d8f0e8" },
        if visible { "block" } else { "none" }
    )
}

/// The overlay `<pre>`, created on first use.
pub(crate) fn overlay_element() -> Option<web_sys::Element> {
    let doc = web_sys::window()?.document()?;
    if let Some(el) = doc.get_element_by_id(OVERLAY_ID) {
        return Some(el);
    }
    let el = doc.create_element("pre").ok()?;
    el.set_id(OVERLAY_ID);
    let _ = el.set_attribute("style", &overlay_style(true, false));
    doc.body()?.append_child(&el).ok()?;
    Some(el)
}

/// Show a full-screen message (e.g. "WebGPU is not available") instead of a
/// blank page. Safe to call more than once; the latest message wins.
pub fn show_message(title: &str, body: &str) {
    set_engine_status(false, Some(title));
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        console_log(true, title);
        return;
    };
    let el = match doc.get_element_by_id(MESSAGE_ID) {
        Some(el) => el,
        None => {
            let Ok(el) = doc.create_element("div") else {
                return;
            };
            el.set_id(MESSAGE_ID);
            let _ = el.set_attribute(
                "style",
                "position:fixed;inset:0;z-index:2000;display:flex;flex-direction:column;\
                 align-items:center;justify-content:center;gap:12px;padding:24px;\
                 text-align:center;background:#101418;color:#e8eef2;\
                 font:16px/1.5 system-ui,sans-serif",
            );
            if let Some(b) = doc.body() {
                let _ = b.append_child(&el);
            }
            el
        }
    };
    el.set_inner_html("");
    for (tag, text, style) in [
        ("h1", title, "margin:0;font-size:24px"),
        ("p", body, "margin:0;max-width:36em;opacity:0.85"),
    ] {
        if let Ok(e) = doc.create_element(tag) {
            e.set_text_content(Some(text));
            let _ = e.set_attribute("style", style);
            let _ = el.append_child(&e);
        }
    }
}

/// Set `window.__engine = { started, error }` for Evani.App's build checker
/// (docs/LIVE_API.md section 7). `error` replaces any previous error only
/// when `Some`.
pub fn set_engine_status(started: bool, error: Option<&str>) { #[cfg(feature = "pointer-lock")] if error.is_some() { pointer_lock::exit_now(); } // a fatal error never leaves the mouse captured
    let Some(window) = web_sys::window() else { return };
    let key = JsValue::from_str("__engine");
    let obj = match js_sys::Reflect::get(&window, &key) {
        Ok(v) if v.is_object() => v,
        _ => {
            let o: JsValue = js_sys::Object::new().into();
            let _ = js_sys::Reflect::set(&o, &JsValue::from_str("error"), &JsValue::NULL);
            let _ = js_sys::Reflect::set(&window, &key, &o);
            o
        }
    };
    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("started"), &JsValue::from_bool(started));
    if let Some(e) = error {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("error"), &JsValue::from_str(e));
    }
}

/// True if `navigator.gpu` exists (WebGPU exposed by the browser).
pub fn webgpu_exposed() -> bool {
    web_sys::window()
        .map(|w| w.navigator())
        .and_then(|n| js_sys::Reflect::get(&n, &JsValue::from_str("gpu")).ok())
        .is_some_and(|g| !g.is_undefined() && !g.is_null())
}

// ---- localStorage ------------------------------------------------------------------

/// `localStorage` as a [`KeyValueStore`].
pub struct LocalStore {
    storage: web_sys::Storage,
}

impl LocalStore {
    /// `None` if storage is blocked (private mode, sandboxed iframe).
    pub fn open() -> Option<Self> {
        let storage = web_sys::window()?.local_storage().ok()??;
        Some(Self { storage })
    }
}

impl KeyValueStore for LocalStore {
    fn get(&self, key: &str) -> Result<Option<String>, SaveError> {
        self.storage
            .get_item(key)
            .map_err(|e| SaveError::Unavailable(js_err(e)))
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), SaveError> {
        self.storage
            .set_item(key, value)
            .map_err(|e| SaveError::Unavailable(js_err(e)))
    }
    fn remove(&mut self, key: &str) -> Result<(), SaveError> {
        self.storage
            .remove_item(key)
            .map_err(|e| SaveError::Unavailable(js_err(e)))
    }
}

// ---- fetch -------------------------------------------------------------------------

/// `fetch()` as an [`AssetSource`].
#[derive(Debug, Clone, Copy, Default)]
pub struct FetchSource;

impl AssetSource for FetchSource {
    fn fetch(&self, url: &str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>>>> {
        let url = url.to_string();
        Box::pin(async move {
            let window = web_sys::window().ok_or("no window")?;
            let resp = JsFuture::from(window.fetch_with_str(&url))
                .await
                .map_err(js_err)?;
            let resp: web_sys::Response = resp.dyn_into().map_err(js_err)?;
            if !resp.ok() {
                return Err(format!("{url}: HTTP {}", resp.status()));
            }
            let buf = JsFuture::from(resp.array_buffer().map_err(js_err)?)
                .await
                .map_err(js_err)?;
            Ok(js_sys::Uint8Array::new(&buf).to_vec())
        })
    }
}

/// Bytes downloaded by this page so far (document plus every resource),
/// from Resource Timing. Uses `encodedBodySize` so cached loads still count
/// what the build weighs.
pub fn downloaded_bytes() -> Option<u64> {
    let perf = web_sys::window()?.performance()?;
    let mut total = 0f64;
    for kind in ["navigation", "resource"] {
        let entries = perf.get_entries_by_type(kind);
        for e in entries.iter() {
            if let Some(r) = e.dyn_ref::<web_sys::PerformanceResourceTiming>() {
                total += r.encoded_body_size().max(r.transfer_size());
            }
        }
    }
    Some(total as u64)
}
