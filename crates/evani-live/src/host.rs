//! Page state for the live bridge and render pacing while parked
//! (docs/LIVE_API.md 3.5). Browser only, dev builds only.
//!
//! * Evani.App's host state: `window.__evaniHost` (`{v, shown, paused}`),
//!   read at start and on every `evani-host` event; the document's
//!   `visibilityState` on every `visibilitychange`.
//! * `page_state` to the bridge at once on every change and as a 1 s beat
//!   from a timer, with the frame counter, so the bridge can tell a paused
//!   page (no beat, or frames standing still) from a busy one and answer
//!   live commands at once instead of waiting out their limit.
//! * While Evani.App says the tab is parked (`shown: false`), the renderer
//!   draws about 5 frames a second ([`RenderPace`]); the simulation keeps
//!   its fixed 60 Hz step. Back to full rate at once on `shown: true`, and
//!   while a bench, compare or sweep run is measuring. Without
//!   `__evaniHost` nothing changes.
//! * A code patch that is ready while no frame runs is applied from the
//!   beat (a timer callback always runs between two frames).

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::capture::RenderPace;
use evani_core::ecs::World;
use js_sys::Reflect;
use serde_json::{Value, json};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use super::ws::Link;

/// Least time between two drawn frames while parked (about 5 fps).
pub const PARKED_INTERVAL_MS: f64 = 200.0;
/// The heartbeat period.
const BEAT_MS: i32 = 1000;

#[derive(Default)]
struct State {
    link: Option<Rc<Link>>,
    /// Frames the page ran (`LiveModule::frame`).
    frames: u64,
    /// `frames` at the last beat.
    beat_frames: u64,
    /// `window.__evaniHost` as JSON (null outside Evani.App).
    host: Value,
    /// The last `page_state` sent (minus its reason), to skip repeats.
    last_sent: Option<Value>,
    /// The renderer's counts, copied each frame for `__evani_host_state`.
    rendered: u64,
    skipped: u64,
    interval_ms: f64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Read the host state, listen for changes and start the beat. Called once
/// from `LiveModule::build`.
pub fn install(link: Option<Rc<Link>>) {
    let Some(win) = web_sys::window() else { return };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.link = link;
        s.host = read_host(&win, None);
    });
    let on_host = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        let Some(win) = web_sys::window() else { return };
        let detail = Reflect::get(&e, &JsValue::from_str("detail")).ok();
        let host = read_host(&win, detail.as_ref());
        STATE.with(|s| s.borrow_mut().host = host);
        send("host");
    });
    let _ = win.add_event_listener_with_callback("evani-host", on_host.as_ref().unchecked_ref());
    on_host.forget();
    let on_vis = Closure::<dyn FnMut()>::new(|| send("visibility"));
    if let Some(doc) = win.document() {
        let _ = doc.add_event_listener_with_callback("visibilitychange", on_vis.as_ref().unchecked_ref());
    }
    on_vis.forget();
    let on_beat = Closure::<dyn FnMut()>::new(beat);
    let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(on_beat.as_ref().unchecked_ref(), BEAT_MS);
    on_beat.forget();
    publish();
}

/// `window.__evaniHost` (or the event's `detail`) as JSON; null when absent.
fn read_host(win: &web_sys::Window, detail: Option<&JsValue>) -> Value {
    let from = |v: &JsValue| -> Option<Value> {
        if !v.is_object() {
            return None;
        }
        let text = js_sys::JSON::stringify(v).ok()?.as_string()?;
        serde_json::from_str(&text).ok()
    };
    Reflect::get(win, &JsValue::from_str("__evaniHost"))
        .ok()
        .and_then(|v| from(&v))
        .or_else(|| detail.and_then(from))
        .unwrap_or(Value::Null)
}

fn visibility() -> String {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| Reflect::get(&d, &JsValue::from_str("visibilityState")).ok())
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "visible".to_string())
}

/// Is Evani.App showing the tab? `None` outside Evani.App.
fn shown() -> Option<bool> {
    STATE.with(|s| s.borrow().host["shown"].as_bool())
}

/// Send `page_state` now (`reason`: `hello`, `beat`, `visibility`, `host`).
/// Changes are sent even when nothing differs from the last report only
/// for `beat` and `hello`.
pub fn send(reason: &str) {
    let msg = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let body = json!({
            "frames": s.frames, "visibility": visibility(), "host": s.host,
            "rendered": s.rendered, "skipped": s.skipped, "render_interval_ms": s.interval_ms,
        });
        let changed = s.last_sent.as_ref().is_none_or(|l| l["visibility"] != body["visibility"] || l["host"] != body["host"]);
        if !changed && !matches!(reason, "beat" | "hello") {
            return None;
        }
        s.last_sent = Some(body.clone());
        let link = s.link.clone()?;
        let mut msg = body;
        msg["type"] = json!("page_state");
        msg["v"] = json!(1);
        msg["reason"] = json!(reason);
        Some((link, msg.to_string()))
    });
    if let Some((link, text)) = msg {
        link.send_text(&text);
    }
    publish();
}

/// The 1 s beat: report, and apply a ready code patch if no frame ran.
fn beat() {
    let idle = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let idle = s.frames == s.beat_frames;
        s.beat_frames = s.frames;
        idle
    });
    send("beat");
    #[cfg(feature = "hotpatch")]
    if idle {
        crate::hotpatch::before_frame();
    }
    #[cfg(not(feature = "hotpatch"))]
    let _ = idle;
}

/// `window.__evani_host_state`: what the page reports, for tests and
/// measuring (`frames`, `rendered`, `skipped`, `render_interval_ms`,
/// `shown`, `visibility`).
fn publish() {
    let Some(win) = web_sys::window() else { return };
    let o = js_sys::Object::new();
    STATE.with(|s| {
        let s = s.borrow();
        let set = |k: &str, v: JsValue| {
            let _ = Reflect::set(&o, &JsValue::from_str(k), &v);
        };
        set("frames", JsValue::from(s.frames as f64));
        set("rendered", JsValue::from(s.rendered as f64));
        set("skipped", JsValue::from(s.skipped as f64));
        set("render_interval_ms", JsValue::from(s.interval_ms));
        set("shown", s.host["shown"].as_bool().map_or(JsValue::NULL, JsValue::from));
    });
    let _ = Reflect::set(&o, &JsValue::from_str("visibility"), &JsValue::from_str(&visibility()));
    let _ = Reflect::set(&win, &JsValue::from_str("__evani_host_state"), &o);
}

/// Every frame (`LiveModule::frame`): count it and set the render pace.
/// `measuring`: a bench, compare or sweep run is on (always full rate).
pub fn frame(world: &mut World, measuring: bool) {
    let interval = if !measuring && shown() == Some(false) { PARKED_INTERVAL_MS } else { 0.0 };
    let (rendered, skipped) = {
        if world.try_res::<RenderPace>().is_none() {
            world.insert_resource(RenderPace::default());
        }
        let mut p = world.res_mut::<RenderPace>();
        if p.min_interval_ms != interval {
            p.min_interval_ms = interval;
            // Back to full rate: the next frame draws at once.
            p.last_ms = 0.0;
        }
        (p.rendered, p.skipped)
    };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.frames += 1;
        s.rendered = rendered;
        s.skipped = skipped;
        s.interval_ms = interval;
    });
}
