//! The page side (wasm only): shows the table and sets `window.__bench` for
//! the runner. Failures here are logged to the console and swallowed: the
//! bench itself keeps going.

use wasm_bindgen::JsValue;

use crate::check::format_table_judging;
use crate::run::Report;

const TABLE_ID: &str = "evani-bench-table";

fn log_error(msg: &str) {
    evani_core::web::console_log(true, &format!("bench page: {msg}"));
}

/// Set `window.__bench = { done, error, mode, progress, results }` and redraw
/// the table. Called whenever the report changes (not every frame).
pub fn publish(report: &Report) {
    let Some(window) = web_sys::window() else {
        return;
    };
    match serde_json::to_string(report) {
        Ok(json) => match js_sys::JSON::parse(&json) {
            Ok(value) => {
                if let Err(e) = js_sys::Reflect::set(&window, &JsValue::from_str("__bench"), &value) {
                    log_error(&evani_core::web::js_err(e));
                }
            }
            Err(e) => log_error(&evani_core::web::js_err(e)),
        },
        Err(e) => log_error(&e.to_string()),
    }
    show_table(&window, report);
}

fn table_element(window: &web_sys::Window) -> Option<web_sys::Element> {
    let doc = window.document()?;
    if let Some(el) = doc.get_element_by_id(TABLE_ID) {
        return Some(el);
    }
    let el = doc.create_element("pre").ok()?;
    el.set_id(TABLE_ID);
    // Above the engine's stats overlay (z-index 1000), below its full-screen
    // messages (2000), and pinned to the top so the verdict lines stay in view
    // on a small screen; a long tail of pass details is cut off instead. On a
    // narrow screen the font shrinks so a table row up to its result column
    // (about 125 characters) still fits the width. Opaque, so the overlay
    // underneath doesn't show through the numbers.
    let _ = el.set_attribute(
        "style",
        "position:fixed;left:8px;top:8px;max-height:calc(100vh - 16px);max-width:calc(100vw - 16px);\
         overflow:hidden;box-sizing:border-box;margin:0;padding:8px 10px;z-index:1001;\
         font:12px/1.4 ui-monospace,Consolas,monospace;font-size:clamp(5px, calc((100vw - 36px) / 76), 12px);\
         white-space:pre;background:#000;color:#d8f0e8;border-radius:6px;pointer-events:none",
    );
    doc.body()?.append_child(&el).ok()?;
    Some(el)
}

fn show_table(window: &web_sys::Window, report: &Report) {
    let Some(el) = table_element(window) else {
        return;
    };
    let mut text = format!(
        "Evani.Dev bench ({})  {}\n",
        report.mode,
        if report.done { "done" } else { report.progress.as_str() }
    );
    if let Some(e) = &report.error {
        text.push_str(&format!("ERROR: {e}\n"));
    }
    if let Some(pick) = &report.device_pick {
        text.push_str(&crate::check::device_line(pick, &report.results));
        text.push('\n');
    }
    text.push_str(&format_table_judging(&report.results, report.device_pick.as_deref()));
    if report.results.iter().any(|r| r.note.starts_with("stress scene")) {
        text.push_str(&crate::check::format_stress_table(&report.results));
    }
    el.set_text_content(Some(&text));
}
