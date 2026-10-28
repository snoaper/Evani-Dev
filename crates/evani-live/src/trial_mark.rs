//! Wasm, dev builds only: while a trial run holds the canvas
//! ([`evani_core::capture::HoldCanvas`], docs/LIVE_API.md
//! 6.30), mark the page `<html data-evani-trial="run id">` and show a small
//! "measuring…" tag in the bottom-right corner (clear of the HUDs that sit
//! bottom-left), so a frozen view reads as "the tools are measuring", not
//! as a hang. Runs every frame; touches the
//! DOM only on change.

use evani_core::ecs::World;
use wasm_bindgen::JsCast;

use crate::LiveSession;

const TAG_ID: &str = "evani-trial-mark";
const TAG_STYLE: &str = "position:fixed;right:8px;bottom:8px;z-index:2147483646;pointer-events:none;\
font:11px/1.4 system-ui,sans-serif;color:#fff;background:rgba(0,0,0,.45);padding:2px 7px;\
border-radius:9px;opacity:.8;user-select:none";

#[derive(Default)]
pub(crate) struct TrialMark {
    /// The run id the page shows as held, if any.
    shown: Option<String>,
}

impl TrialMark {
    pub(crate) fn frame(&mut self, world: &World) {
        // A trial run shows its run id and "measuring…"; a `together` edit
        // commit settling over frames (6.31) its transaction id and "applying…".
        let now = if crate::capture::canvas_held(world) {
            world
                .try_res::<LiveSession>()
                .and_then(|s| {
                    s.run.as_ref().map(|r| r.id.clone()).or_else(|| {
                        s.edit_hold.as_ref().map(|h| h.txn.clone())
                    })
                })
                .or_else(|| Some(String::new()))
        } else {
            None
        };
        if now == self.shown {
            return;
        }
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Some(html) = doc.document_element() else {
            return;
        };
        match &now {
            Some(id) => {
                let _ = html.set_attribute("data-evani-trial", id);
                let text = if id.starts_with("edit-") {
                    "applying\u{2026}"
                } else {
                    "measuring\u{2026}"
                };
                if let Some(tag) = doc.get_element_by_id(TAG_ID) {
                    tag.set_text_content(Some(text));
                } else if let Ok(tag) = doc.create_element("div") {
                    tag.set_id(TAG_ID);
                    tag.set_text_content(Some(text));
                    let _ = tag.set_attribute("aria-hidden", "true");
                    if let Some(h) = tag.dyn_ref::<web_sys::HtmlElement>() {
                        h.style().set_css_text(TAG_STYLE);
                    }
                    if let Some(body) = doc.body() {
                        let _ = body.append_child(&tag);
                    }
                }
            }
            None => {
                let _ = html.remove_attribute("data-evani-trial");
                if let Some(tag) = doc.get_element_by_id(TAG_ID) {
                    tag.remove();
                }
            }
        }
        self.shown = now;
    }
}
