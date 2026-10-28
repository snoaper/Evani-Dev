//! Page hot-replace, page side (docs/LIVE_API.md 14).
//!
//! The bridge watches the game's page files and, for an edit it can swap in
//! place, sends one `page_patch`:
//! `{"type":"page_patch","v":1,"seq","from","to","files","ops":[…]}` with ops
//! - `{"op":"css","key","text"}`: the new text of the `<style
//!   data-evani-style="key">` block (an inline block or a linked sheet),
//! - `{"op":"html","id","old","html"}`: the new content of `#id`, only if
//!   its current content still equals `old` (the game hasn't changed it),
//! - `{"op":"title","html"}`: the new `<title>` content.
//!
//! [`apply`] is all or nothing: it checks the page's rev (`<meta
//! name="evani-page-rev">`) equals `from` and every op's target first, then
//! changes them and sets the rev to `to`. Nothing is reloaded; the engine,
//! game state and live values are untouched. The reply is a `page.patched`
//! event ([`report`]).
//!
//! The parsing and the all-or-nothing rules are plain Rust (tested
//! natively, against a fake page); [`web`] is the browser's [`PageDom`].

use serde_json::{Value, json};

/// One change to the open page.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchOp {
    Css { key: String, text: String },
    Html { id: String, old: String, html: String },
    Title { html: String },
}

/// A parsed `page_patch`.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub seq: u64,
    pub from: Option<String>,
    pub to: String,
    pub ops: Vec<PatchOp>,
}

fn text<'a>(v: &'a Value, k: &str, what: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or_else(|| format!("{what} needs a text \"{k}\""))
}

/// A key or id goes into a CSS selector or an id lookup: plain text only.
fn plain(s: &str, what: &str) -> Result<String, String> {
    if s.is_empty() || s.contains(['"', '\\']) || s.chars().any(char::is_control) {
        return Err(format!("{what} {s:?} isn't plain text"));
    }
    Ok(s.to_string())
}

/// Read a `page_patch` message.
pub fn parse(msg: &Value) -> Result<Patch, String> {
    if msg["type"] != "page_patch" {
        return Err("not a page_patch".into());
    }
    let seq = msg["seq"].as_u64().ok_or("page_patch needs a whole-number seq")?;
    let to = text(msg, "to", "page_patch")?.to_string();
    let from = match &msg["from"] {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return Err("page_patch \"from\" must be text or null".into()),
    };
    let list = msg["ops"].as_array().ok_or("page_patch needs an ops list")?;
    let mut ops = Vec::with_capacity(list.len());
    for o in list {
        ops.push(match o["op"].as_str() {
            Some("css") => PatchOp::Css { key: plain(text(o, "key", "css")?, "style key")?, text: text(o, "text", "css")?.to_string() },
            Some("html") => PatchOp::Html {
                id: plain(text(o, "id", "html")?, "id")?,
                old: text(o, "old", "html")?.to_string(),
                html: text(o, "html", "html")?.to_string(),
            },
            Some("title") => PatchOp::Title { html: text(o, "html", "title")?.to_string() },
            other => return Err(format!("unknown page op {other:?}")),
        });
    }
    Ok(Patch { seq, from, to, ops })
}

/// What [`apply`] needs from the page.
pub trait PageDom {
    /// The page's rev tag (`<meta name="evani-page-rev">`), if it has one.
    fn rev(&self) -> Option<String>;
    fn has_style(&self, key: &str) -> bool;
    /// `Ok` when `#id` exists and its content still equals `old`.
    fn part_is(&self, id: &str, old: &str) -> Result<(), String>;
    fn set_style(&mut self, key: &str, text: &str);
    fn set_part(&mut self, id: &str, html: &str);
    fn set_title(&mut self, html: &str);
    fn set_rev(&mut self, rev: &str);
}

/// Swap the patch in, all or nothing. Returns how many ops were applied.
pub fn apply(dom: &mut impl PageDom, p: &Patch) -> Result<usize, String> {
    let now = dom.rev();
    if now != p.from {
        return Err(format!(
            "the page is at {} but the change is from {}",
            now.as_deref().unwrap_or("no rev"),
            p.from.as_deref().unwrap_or("no rev")
        ));
    }
    for op in &p.ops {
        match op {
            PatchOp::Css { key, .. } if !dom.has_style(key) => {
                return Err(format!("the page has no style block {key}"));
            }
            PatchOp::Html { id, old, .. } => dom.part_is(id, old)?,
            _ => {}
        }
    }
    for op in &p.ops {
        match op {
            PatchOp::Css { key, text } => dom.set_style(key, text),
            PatchOp::Html { id, html, .. } => dom.set_part(id, html),
            PatchOp::Title { html } => dom.set_title(html),
        }
    }
    dom.set_rev(&p.to);
    Ok(p.ops.len())
}

/// The `page.patched` event for the bridge.
pub fn report(seq: u64, result: &Result<usize, String>, rev: Option<&str>) -> Value {
    let mut data = json!({ "seq": seq, "ok": result.is_ok(), "rev": rev });
    match result {
        Ok(n) => data["applied"] = json!(n),
        Err(e) => data["error"] = json!(e),
    }
    json!({ "type": "event", "v": 1, "event": "page.patched", "data": data })
}

/// Handle one `page_patch` message against `dom`; returns the reply event.
pub fn handle(dom: &mut impl PageDom, msg: &Value) -> Value {
    let seq = msg["seq"].as_u64().unwrap_or(0);
    let result = parse(msg).and_then(|p| apply(dom, &p));
    report(seq, &result, dom.rev().as_deref())
}

/// The browser's page.
#[cfg(target_arch = "wasm32")]
pub mod web {
    use wasm_bindgen::{JsCast, JsValue};
    use web_sys::{Document, Element, HtmlTemplateElement};

    pub struct WebDom {
        doc: Document,
    }

    impl WebDom {
        pub fn new() -> Option<Self> {
            Some(Self { doc: web_sys::window()?.document()? })
        }

        fn meta(&self) -> Option<Element> {
            self.doc.query_selector("meta[name=evani-page-rev]").ok()?
        }

        fn style(&self, key: &str) -> Option<Element> {
            self.doc.query_selector(&format!("style[data-evani-style=\"{key}\"]")).ok()?
        }
    }

    /// The page's rev tag, for the `hello`.
    pub fn page_rev() -> Option<String> {
        WebDom::new()?.meta()?.get_attribute("content")
    }

    impl super::PageDom for WebDom {
        fn rev(&self) -> Option<String> {
            self.meta()?.get_attribute("content")
        }

        fn has_style(&self, key: &str) -> bool {
            self.style(key).is_some()
        }

        fn part_is(&self, id: &str, old: &str) -> Result<(), String> {
            let el = self.doc.get_element_by_id(id).ok_or_else(|| format!("the page has no #{id}"))?;
            let changed = || format!("#{id} was changed by the game since the page loaded");
            // Parse `old` the way the page was parsed and compare node by node.
            let tpl: HtmlTemplateElement = self
                .doc
                .create_element("template")
                .map_err(|_| changed())?
                .dyn_into()
                .map_err(|_| changed())?;
            tpl.set_inner_html(old);
            let (want, have) = (tpl.content().child_nodes(), el.child_nodes());
            if want.length() != have.length() {
                return Err(changed());
            }
            for i in 0..want.length() {
                let same = match (want.item(i), have.item(i)) {
                    (Some(a), Some(b)) => a.is_equal_node(Some(&b)),
                    _ => false,
                };
                if !same {
                    return Err(changed());
                }
            }
            Ok(())
        }

        fn set_style(&mut self, key: &str, text: &str) {
            if let Some(s) = self.style(key) {
                s.set_text_content(Some(text));
            }
        }

        fn set_part(&mut self, id: &str, html: &str) {
            if let Some(el) = self.doc.get_element_by_id(id) {
                el.set_inner_html(html);
            }
        }

        fn set_title(&mut self, html: &str) {
            if let Ok(Some(t)) = self.doc.query_selector("title") {
                t.set_inner_html(html);
            }
        }

        fn set_rev(&mut self, rev: &str) {
            if let Some(m) = self.meta() {
                let _ = m.set_attribute("content", rev);
            }
            // For tests and tools: the rev and a count of swaps this page took.
            if let Some(win) = web_sys::window() {
                let n = js_sys::Reflect::get(&win, &JsValue::from_str("__evaniPagePatches"))
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let _ = js_sys::Reflect::set(&win, &JsValue::from_str("__evaniPagePatches"), &JsValue::from_f64(n + 1.0));
                let _ = js_sys::Reflect::set(&win, &JsValue::from_str("__evaniPageRev"), &JsValue::from_str(rev));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakePage {
        rev: Option<String>,
        styles: BTreeMap<String, String>,
        parts: BTreeMap<String, String>,
        title: String,
    }

    impl PageDom for FakePage {
        fn rev(&self) -> Option<String> {
            self.rev.clone()
        }
        fn has_style(&self, key: &str) -> bool {
            self.styles.contains_key(key)
        }
        fn part_is(&self, id: &str, old: &str) -> Result<(), String> {
            match self.parts.get(id) {
                None => Err(format!("the page has no #{id}")),
                Some(h) if h == old => Ok(()),
                Some(_) => Err(format!("#{id} was changed by the game since the page loaded")),
            }
        }
        fn set_style(&mut self, key: &str, text: &str) {
            self.styles.insert(key.into(), text.into());
        }
        fn set_part(&mut self, id: &str, html: &str) {
            self.parts.insert(id.into(), html.into());
        }
        fn set_title(&mut self, html: &str) {
            self.title = html.into();
        }
        fn set_rev(&mut self, rev: &str) {
            self.rev = Some(rev.into());
        }
    }

    fn page() -> FakePage {
        FakePage {
            rev: Some("s-1:0".into()),
            styles: BTreeMap::from([("hud.css".into(), ".a{}".into()), ("style:0".into(), "body{}".into())]),
            parts: BTreeMap::from([("title".into(), "<h1>Rally</h1>".into())]),
            title: "Kart".into(),
        }
    }

    fn msg(from: &str, ops: Value) -> Value {
        json!({ "type": "page_patch", "v": 1, "seq": 7, "from": from, "to": "s-1:1", "files": ["hud.css"], "ops": ops })
    }

    #[test]
    fn parses_the_three_ops() {
        let p = parse(&msg("s-1:0", json!([
            { "op": "css", "key": "hud.css", "text": ".a{color:red}" },
            { "op": "html", "id": "title", "old": "x", "html": "y" },
            { "op": "title", "html": "K &amp; co" },
        ])))
        .unwrap();
        assert_eq!((p.seq, p.from.as_deref(), p.to.as_str()), (7, Some("s-1:0"), "s-1:1"));
        assert_eq!(p.ops[0], PatchOp::Css { key: "hud.css".into(), text: ".a{color:red}".into() });
        assert_eq!(p.ops[2], PatchOp::Title { html: "K &amp; co".into() });
    }

    #[test]
    fn refuses_bad_messages_and_selector_breaking_keys() {
        for bad in [
            json!({ "type": "cmd" }),
            json!({ "type": "page_patch", "to": "x", "ops": [] }),
            msg("a", json!([{ "op": "js", "code": "x" }])),
            msg("a", json!([{ "op": "css", "key": "a\"] , script[x=\"", "text": "" }])),
            msg("a", json!([{ "op": "css", "key": "a\\", "text": "" }])),
            msg("a", json!([{ "op": "html", "id": "", "old": "", "html": "" }])),
            msg("a", json!([{ "op": "css", "key": "hud.css" }])),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_css_swap_changes_only_that_sheet_and_the_rev() {
        let mut p = page();
        let out = handle(&mut p, &msg("s-1:0", json!([{ "op": "css", "key": "hud.css", "text": ".a{color:#0ff}" }])));
        assert_eq!(out["event"], "page.patched");
        assert_eq!(out["data"], json!({ "seq": 7, "ok": true, "rev": "s-1:1", "applied": 1 }));
        assert_eq!(p.styles["hud.css"], ".a{color:#0ff}");
        assert_eq!(p.styles["style:0"], "body{}");
        assert_eq!(p.parts["title"], "<h1>Rally</h1>");
    }

    #[test]
    fn all_or_nothing() {
        // Second op's part was changed by the game: the first isn't applied either.
        let mut p = page();
        p.parts.insert("title".into(), "<h1>Rally</h1><p>3 wins</p>".into());
        let out = handle(&mut p, &msg("s-1:0", json!([
            { "op": "css", "key": "hud.css", "text": "NEW" },
            { "op": "html", "id": "title", "old": "<h1>Rally</h1>", "html": "<h1>Rally!</h1>" },
        ])));
        assert_eq!(out["data"]["ok"], false);
        assert_eq!(out["data"]["rev"], "s-1:0");
        assert!(out["data"]["error"].as_str().unwrap().contains("#title was changed"));
        assert_eq!(p.styles["hud.css"], ".a{}");
        // A missing sheet, or a page at another rev: refused, nothing changed.
        let mut p = page();
        let out = handle(&mut p, &msg("s-1:0", json!([{ "op": "css", "key": "other.css", "text": "x" }])));
        assert_eq!(out["data"]["ok"], false);
        let out = handle(&mut p, &msg("s-1:5", json!([{ "op": "css", "key": "hud.css", "text": "x" }])));
        assert!(out["data"]["error"].as_str().unwrap().contains("the page is at s-1:0"));
        assert_eq!((p.rev.as_deref(), p.styles["hud.css"].as_str()), (Some("s-1:0"), ".a{}"));
    }

    #[test]
    fn html_and_title_swap_when_untouched() {
        let mut p = page();
        let out = handle(&mut p, &msg("s-1:0", json!([
            { "op": "title", "html": "Kart!" },
            { "op": "html", "id": "title", "old": "<h1>Rally</h1>", "html": "<h1>Rally!</h1>" },
        ])));
        assert_eq!(out["data"]["applied"], 2);
        assert_eq!((p.title.as_str(), p.parts["title"].as_str()), ("Kart!", "<h1>Rally!</h1>"));
        // An empty patch still moves the rev (the bridge's "nothing visible" swap).
        let mut q = page();
        let mut m = msg("s-1:0", json!([]));
        m["to"] = json!("s-1:3");
        assert_eq!(handle(&mut q, &m)["data"]["rev"], "s-1:3");
    }
}
