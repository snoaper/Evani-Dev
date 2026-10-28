//! Drop-to-convert, the page's controls (S3-14, docs/LIVE_API.md 16).
//! Browser only, dev builds of model games only (`models` feature).
//!
//! Two ways in, both ending at the same place:
//!
//! * a drop zone along the bottom of the page: a `.glb`, a `.blend` or a
//!   `.zip` holding one `.gltf` and its buffers, dropped anywhere on the
//!   window, converts; a file chosen through the zone's "Convert a model"
//!   button does the same (S3-0: a drop on the game did nothing in the
//!   Evani.dev tab, so the button is the way that always works);
//! * the before/after slider (`model.compare { at }`), the split box
//!   (`model.compare { split }`), the tier switch and the report panel that
//!   appear once a conversion is on show, and `window.__evaniConvert` for
//!   the tab and for tests.
//!
//! Every file goes through the bridge's checks here first, so a wrong file
//! is refused with a sentence the user reads instead of a failed request
//! ([`crate::convert::accept_drop`], native and tested there):
//!
//! 1. [`crate::convert::accept_name`]: a plain name (no path, no drive, no
//!    control character, not hidden), at most [`crate::convert::MAX_NAME`]
//!    bytes, with a `.glb`, `.blend` or `.zip` extension.
//! 2. its size, before a byte is read: over [`crate::convert::UPLOAD_CAP`]
//!    is refused.
//! 3. its first bytes, by the bridge's own signature rule, so a renamed
//!    picture is "not a model file" here rather than a 415 later.
//!
//! Nothing about a name reaches a path: the body goes to
//! [`crate::convert::upload_url`], the bridge names the job's folder by the
//! content hash and the job by the checked id, and the page only posts and
//! fetches through the bridge's token (its cookie). The page writes no file.
//!
//! Flow: post the bytes, hear `convert_progress` and `convert_done` on the
//! live socket, then `model.load` the finished job into the split view and
//! fill the report panel. A load that fails leaves the game as it was.

use std::cell::RefCell;

use js_sys::{Function, Promise, Reflect};
use serde_json::{Value, json};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::LiveError;
use crate::convert::{HEAD_BYTES, accept_drop, job_from_done, page_code, report_lines, sentence, upload_url};

const ZONE_ID: &str = "evani-convert-zone";
const PANEL_ID: &str = "evani-convert-report";

// ---- page state ------------------------------------------------------------------------

#[derive(Default)]
struct State {
    /// The last sentence shown in the zone (the tab and tests read it).
    message: String,
    /// The job the bridge accepted (`{job, hash, id}`), until it ends.
    job: Option<Value>,
    /// The conversion on show (`model.load`'s answer), if any.
    view: Option<Value>,
    busy: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn log(msg: &str) {
    web_sys::console::info_1(&JsValue::from_str(&format!("[Evani.dev convert] {msg}")));
}

/// `window.__evaniConvert` and an `evani-convert` event: the tab and tests
/// read what the zone is doing.
fn publish() {
    let Some(win) = web_sys::window() else { return };
    let st = STATE.with(|s| {
        let s = s.borrow();
        json!({ "message": s.message, "busy": s.busy, "job": s.job, "view": s.view, "zone": ZONE_ID })
    });
    let text = st.to_string();
    if let Ok(v) = js_sys::JSON::parse(&text) {
        let _ = Reflect::set(&win, &JsValue::from_str("__evaniConvert"), &v);
        if let Ok(ev) = web_sys::Event::new("evani-convert") {
            let _ = win.dispatch_event(&ev);
        }
    }
}

/// Set the zone's sentence and republish.
fn say(msg: impl Into<String>) {
    let msg = msg.into();
    STATE.with(|s| s.borrow_mut().message = msg.clone());
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(el) = doc.get_element_by_id(&format!("{ZONE_ID}-msg"))
    {
        el.set_text_content(Some(&msg));
    }
    log(&msg);
    publish();
}

/// Set `busy`, then republish.
fn settle(busy: bool) {
    STATE.with(|s| s.borrow_mut().busy = busy);
    publish();
}

// ---- the bridge calls ------------------------------------------------------------------

fn live_call(cmd: &str, params: &Value) -> Option<Promise> {
    let win = web_sys::window()?;
    let evani = Reflect::get(&win, &JsValue::from_str("evani")).ok()?;
    let live = Reflect::get(&evani, &JsValue::from_str("live")).ok()?;
    let call = Reflect::get(&live, &JsValue::from_str("call")).ok()?.dyn_into::<Function>().ok()?;
    let p = call.call2(&JsValue::NULL, &JsValue::from_str(cmd), &js_to(params)).ok()?;
    p.dyn_into::<Promise>().ok()
}

fn js_to(v: &Value) -> JsValue {
    js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL)
}

fn from_js(v: &JsValue) -> Value {
    js_sys::JSON::stringify(v).ok().and_then(|s| serde_json::from_str(&String::from(s)).ok()).unwrap_or(Value::Null)
}

fn js_err(e: &JsValue) -> LiveError {
    let field = |k: &str| Reflect::get(e, &JsValue::from_str(k)).ok();
    // `LiveError::code` is `&'static str`: the JS side's code string maps
    // onto the known list, anything else reads as `internal`.
    let code = field("code").and_then(|c| c.as_string()).map_or("internal", |c| page_code(&c));
    let msg = field("message").and_then(|m| m.as_string()).unwrap_or_else(|| "the page could not call the bridge".into());
    let data = field("data").filter(|d| d.is_object()).map(|d| from_js(&d)).unwrap_or(Value::Null);
    LiveError::new(code, msg).with(data)
}

/// Show or hide an element by id (`display`).
fn display(doc: &web_sys::Document, id: &str, how: &str) {
    if let Some(h) = doc.get_element_by_id(id).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
        let _ = h.style().set_property("display", how);
    }
}

/// Fill the panel, the slider, the split box and the tier row from a
/// `model.load`/`model.compare` view, and show them.
fn show_view(view: &Value) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        let on = view["shown"].is_object();
        if let Some(panel) = doc.get_element_by_id(PANEL_ID) {
            let report = view.get("report").filter(|r| r.is_object()).cloned().unwrap_or(Value::Null);
            panel.set_text_content(Some(&report_lines(&report).join("\n")));
        }
        display(&doc, PANEL_ID, if on { "block" } else { "none" });
        display(&doc, &format!("{ZONE_ID}-view"), if on { "flex" } else { "none" });
        if let Some(at) = view["at"].as_f64()
            && let Some(s) = doc.get_element_by_id(&format!("{ZONE_ID}-at")).and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            s.set_value_as_number(at);
        }
        if let Some(split) = view["split"].as_bool()
            && let Some(c) = doc.get_element_by_id(&format!("{ZONE_ID}-split")).and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            c.set_checked(split);
        }
        if let Some(row) = doc.get_element_by_id(&format!("{ZONE_ID}-tiers")) {
            row.set_inner_html("");
            let active = view["tier"].as_str().unwrap_or_default().to_string();
            for tier in view["tiers"].as_array().into_iter().flatten().filter_map(|t| t["tier"].as_str()) {
                let Ok(b) = doc.create_element("button") else { continue };
                b.set_text_content(Some(tier));
                let _ = b.set_attribute("data-tier", tier);
                let _ = b.set_attribute("data-active", if tier == active { "1" } else { "0" });
                let _ = b.set_attribute("style", "margin:0;padding:2px 8px;border-radius:6px;border:1px solid #3c4043;background:#1c1f22;color:#e8eaed;font:12px system-ui,sans-serif;cursor:pointer");
                let want = tier.to_string();
                let on = Closure::<dyn FnMut()>::new(move || compare(&json!({ "tier": want })));
                if let Some(h) = b.dyn_ref::<web_sys::HtmlElement>() {
                    h.set_onclick(Some(on.as_ref().unchecked_ref()));
                }
                on.forget();
                let _ = row.append_child(&b);
            }
        }
    }
}

/// `model.compare { at?, tier?, split? }`: move the line or switch the tier.
fn compare(params: &Value) {
    let Some(p) = live_call("model.compare", params) else {
        return say("The page has no live link to the bridge.");
    };
    wasm_bindgen_futures::spawn_local(async move {
        match JsFuture::from(p).await {
            Ok(v) => {
                let view = from_js(&v);
                STATE.with(|s| s.borrow_mut().view = Some(view.clone()));
                show_view(&view);
                publish();
            }
            Err(e) => say(sentence(&js_err(&e))),
        }
    });
}

// ---- the upload ------------------------------------------------------------------------

/// Post a file's bytes to the bridge (its cookie carries the token).
async fn post(file: &web_sys::File, name: &str) -> Result<Value, String> {
    let Some(win) = web_sys::window() else { return Err("the page has no window".into()) };
    let blob = file.slice().map_err(|_| "the file could not be read".to_string())?;
    let body = JsFuture::from(blob.array_buffer()).await.map_err(|_| "the file could not be read".to_string())?;
    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&body);
    let url = upload_url(name);
    let resp = JsFuture::from(win.fetch_with_str_and_init(&url, &opts)).await.map_err(|_| "the page could not reach the bridge".to_string())?;
    let Some(resp) = resp.dyn_into::<web_sys::Response>().ok() else { return Err("the bridge sent no answer".into()) };
    let text = JsFuture::from(resp.text().map_err(|_| "the bridge sent no answer".to_string())?).await.ok().and_then(|t| t.as_string());
    let body = text.and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    if resp.ok() {
        Ok(body)
    } else {
        let msg = body["error"]["message"].as_str().map_or_else(|| format!("the bridge refused the file (HTTP {})", resp.status()), str::to_string);
        Err(msg)
    }
}

/// Take a dropped (or chosen) `File` through the checks, then the bridge.
fn take(file: web_sys::File) {
    let name = file.name();
    let size = file.size();
    settle(true);
    say(format!("Checking {name}…"));
    wasm_bindgen_futures::spawn_local(async move {
        // The first bytes, for the type check.
        let head = match file.slice_with_f64_and_f64(0.0, HEAD_BYTES as f64) {
            Ok(b) => JsFuture::from(b.array_buffer()).await.ok(),
            Err(_) => None,
        };
        let head: Vec<u8> = head.map(|b| js_sys::Uint8Array::new(&b).to_vec()).unwrap_or_default();
        let checked = match accept_drop(&name, size, &head) {
            Ok(n) => n,
            Err(e) => {
                settle(false);
                return say(sentence(&e));
            }
        };
        say(format!("Sending {checked} ({} MiB)…", (size / (1024.0 * 1024.0)).max(0.1)));
        match post(&file, &checked).await {
            Ok(job) => {
                STATE.with(|s| s.borrow_mut().job = Some(job));
                settle(true);
                say("Converting… this can take a few minutes.");
            }
            Err(msg) => {
                settle(false);
                say(format!("{checked}: {msg}"));
            }
        }
    });
}

// ---- events from the bridge ------------------------------------------------------------

/// A `convert_progress` event: its message in the zone.
fn on_progress(data: &Value) {
    let msg = data["message"].as_str().or_else(|| data["stage"].as_str()).unwrap_or("converting").to_string();
    say(msg);
}

/// A `convert_done` event: show the model, or the reason it failed.
fn on_done(data: &Value) {
    match job_from_done(data) {
        Ok(job) => {
            STATE.with(|s| s.borrow_mut().job = Some(job.clone()));
            settle(true);
            say("Converted. Loading it into the view…");
            let Some(p) = live_call("model.load", &json!({ "job": job })) else {
                settle(false);
                return say("Converted, but the page has no live link to show it.");
            };
            wasm_bindgen_futures::spawn_local(async move {
                match JsFuture::from(p).await {
                    Ok(v) => {
                        let view = from_js(&v);
                        STATE.with(|s| s.borrow_mut().view = Some(view.clone()));
                        show_view(&view);
                        settle(false);
                        say("Showing the conversion: the original is left of the line.");
                    }
                    Err(e) => {
                        settle(false);
                        say(sentence(&js_err(&e)));
                    }
                }
            });
        }
        Err(e) => {
            settle(false);
            let log = data["log"].as_str().unwrap_or_default();
            let tail: Vec<&str> = log.lines().rev().take(6).collect();
            let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
            say(if tail.is_empty() { sentence(&e) } else { format!("{}\n{tail}", sentence(&e)) });
        }
    }
}

// ---- the DOM ---------------------------------------------------------------------------

fn el(doc: &web_sys::Document, tag: &str) -> Option<web_sys::Element> {
    doc.create_element(tag).ok()
}

/// Add the drop zone, its button and its panel to the page. Called once,
/// after the live link is up; the browser's own drop default is stopped so a
/// model dropped anywhere on the game lands in the zone.
pub fn install() {
    let Some(win) = web_sys::window() else { return };
    let Some(doc) = win.document() else { return };
    let Some(body) = doc.body() else { return };
    if doc.get_element_by_id(ZONE_ID).is_some() {
        return;
    }
    let Some(zone) = el(&doc, "div") else { return };
    zone.set_id(ZONE_ID);
    let _ = zone.set_attribute(
        "style",
        "position:fixed;left:12px;right:12px;bottom:12px;z-index:2147482000;display:flex;gap:8px;\
         align-items:center;padding:8px 12px;border-radius:10px;border:1px dashed #3c4043;\
         background:rgba(20,22,24,.92);color:#e8eaed;font:12px/1.4 system-ui,sans-serif;\
         pointer-events:auto;box-sizing:border-box",
    );
    let _ = zone.set_attribute("data-drop", "idle");
    let Some(hint) = el(&doc, "span") else { return };
    hint.set_text_content(Some("Drop a model here (.glb, .blend, or a .zip with a .gltf)"));
    let _ = hint.set_attribute("id", &format!("{ZONE_ID}-hint"));
    let _ = hint.set_attribute("style", "flex:0 0 auto;color:#c6cbd1");
    let Some(msg) = el(&doc, "span") else { return };
    msg.set_id(&format!("{ZONE_ID}-msg"));
    let _ = msg.set_attribute("style", "flex:1;min-width:0;overflow:hidden;white-space:pre-wrap;color:#9aa0a6");
    let Some(row) = el(&doc, "span") else { return };
    row.set_id(&format!("{ZONE_ID}-tiers"));
    let _ = row.set_attribute("style", "display:flex;gap:4px;flex:0 0 auto");
    let Some(pick) = el(&doc, "input") else { return };
    pick.set_id(&format!("{ZONE_ID}-file"));
    let _ = pick.set_attribute("type", "file");
    let _ = pick.set_attribute("accept", ".glb,.blend,.zip");
    let _ = pick.set_attribute("style", "display:none");
    let Some(button) = el(&doc, "button") else { return };
    button.set_id(&format!("{ZONE_ID}-pick"));
    button.set_text_content(Some("Convert a model"));
    let _ = button.set_attribute("style", "padding:4px 10px;border-radius:6px;border:1px solid #3c4043;background:#1c1f22;color:#e8eaed;font:12px system-ui,sans-serif;cursor:pointer");
    // The before/after controls, shown once a conversion is on show: the
    // slider moves the line (`at`: 0 all converted, 1 all original), the box
    // turns the split off to see the converted model whole.
    let Some(view) = el(&doc, "span") else { return };
    view.set_id(&format!("{ZONE_ID}-view"));
    let _ = view.set_attribute("style", "display:none;gap:6px;align-items:center;flex:0 0 auto;color:#c6cbd1");
    let Some(left) = el(&doc, "span") else { return };
    left.set_text_content(Some("converted"));
    let Some(slider) = el(&doc, "input") else { return };
    slider.set_id(&format!("{ZONE_ID}-at"));
    for (k, v) in [("type", "range"), ("min", "0"), ("max", "1"), ("step", "0.01"), ("value", "0.5")] {
        let _ = slider.set_attribute(k, v);
    }
    let _ = slider.set_attribute("aria-label", "Before and after: move right to see more of the original");
    let _ = slider.set_attribute("style", "width:9rem;margin:0");
    let Some(right) = el(&doc, "span") else { return };
    right.set_text_content(Some("original"));
    let Some(split_label) = el(&doc, "label") else { return };
    let _ = split_label.set_attribute("style", "display:flex;gap:4px;align-items:center;cursor:pointer");
    let Some(split_box) = el(&doc, "input") else { return };
    split_box.set_id(&format!("{ZONE_ID}-split"));
    let _ = split_box.set_attribute("type", "checkbox");
    let _ = split_box.set_attribute("checked", "");
    let Some(split_text) = el(&doc, "span") else { return };
    split_text.set_text_content(Some("Split"));
    let _ = split_label.append_child(&split_box);
    let _ = split_label.append_child(&split_text);
    for n in [&left, &slider, &right, &split_label] {
        let _ = view.append_child(n);
    }
    for n in [&hint, &msg, &view, &row, &pick, &button] {
        let _ = zone.append_child(n);
    }
    let on_at = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        let Some(s) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        let v = s.value_as_number();
        if v.is_finite() {
            compare(&json!({ "at": v.clamp(0.0, 1.0) }));
        }
    });
    let _ = slider.add_event_listener_with_callback("input", on_at.as_ref().unchecked_ref());
    on_at.forget();
    let on_split = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        let Some(c) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        compare(&json!({ "split": c.checked() }));
    });
    let _ = split_box.add_event_listener_with_callback("change", on_split.as_ref().unchecked_ref());
    on_split.forget();
    let Some(panel) = el(&doc, "pre") else { return };
    panel.set_id(PANEL_ID);
    let _ = panel.set_attribute(
        "style",
        "position:fixed;right:12px;bottom:64px;z-index:2147482000;max-width:min(30rem,60vw);max-height:30vh;\
         overflow:auto;margin:0;padding:8px 12px;border-radius:8px;border:1px solid #3c4043;\
         background:rgba(20,22,24,.92);color:#e8eaed;font:12px/1.45 ui-monospace,Consolas,monospace;\
         white-space:pre-wrap;display:none",
    );
    let _ = body.append_child(&zone);
    let _ = body.append_child(&panel);

    // A file dropped anywhere on the page goes to the zone.
    let on_over = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
        if let Some(t) = e.data_transfer() {
            t.set_drop_effect("copy");
        }
        if let Some(z) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(ZONE_ID)) {
            let _ = z.set_attribute("data-drop", "over");
        }
    });
    let on_leave = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
        if let Some(z) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(ZONE_ID)) {
            let _ = z.set_attribute("data-drop", "idle");
        }
    });
    let on_drop = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| {
        e.prevent_default();
        if let Some(z) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(ZONE_ID)) {
            let _ = z.set_attribute("data-drop", "idle");
        }
        // One file at a time (the bridge converts one at a time too).
        let Some(f) = e.data_transfer().and_then(|t| t.files()).and_then(|l| l.get(0)) else {
            return say("Drop one model file at a time.");
        };
        take(f);
    });
    let _ = win.add_event_listener_with_callback("dragover", on_over.as_ref().unchecked_ref());
    let _ = win.add_event_listener_with_callback("dragleave", on_leave.as_ref().unchecked_ref());
    let _ = win.add_event_listener_with_callback("drop", on_drop.as_ref().unchecked_ref());
    on_over.forget();
    on_leave.forget();
    on_drop.forget();

    // The button: the same checks, through the browser's own chooser.
    let on_click = Closure::<dyn FnMut()>::new(|| {
        if let Some(p) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(&format!("{ZONE_ID}-file")))
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        {
            p.click();
        }
    });
    if let Some(h) = button.dyn_ref::<web_sys::HtmlElement>() {
        h.set_onclick(Some(on_click.as_ref().unchecked_ref()));
    }
    on_click.forget();
    let on_pick = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        let Some(input) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        if let Some(f) = input.files().and_then(|l| l.get(0)) {
            take(f);
        }
    });
    if let Some(input) = pick.dyn_ref::<web_sys::HtmlInputElement>() {
        input.set_onchange(Some(on_pick.as_ref().unchecked_ref()));
    }
    on_pick.forget();

    // The bridge's own events reach the page through `evani.live.on`.
    listen("convert_progress", on_progress);
    listen("convert_done", on_done);
    settle(false);
    say("Drop a model to convert it.");
}

/// `evani.live.on(event, fn)` with `fn` taking the event's data.
fn listen(event: &str, f: impl Fn(&Value) + 'static) {
    let Some(win) = web_sys::window() else { return };
    let Ok(evani) = Reflect::get(&win, &JsValue::from_str("evani")) else { return };
    let Ok(live) = Reflect::get(&evani, &JsValue::from_str("live")) else { return };
    let Ok(on) = Reflect::get(&live, &JsValue::from_str("on")).ok().and_then(|o| o.dyn_into::<Function>().ok()).ok_or(()) else { return };
    let cb = Closure::<dyn FnMut(JsValue)>::new(move |data: JsValue| f(&from_js(&data)));
    let _ = on.call2(&JsValue::NULL, &JsValue::from_str(event), cb.as_ref().unchecked_ref());
    cb.forget();
}
