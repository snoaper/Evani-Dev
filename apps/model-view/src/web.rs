//! The page side of the model viewer (wasm only, docs/EVANI_MODEL_TAB.md 4.1
//! and 4.4). One frame system, run before the kit's own:
//!
//! * the first frame reads the page address (`#model=..&v=..&tier=..&cam=..`)
//!   and starts the viewer;
//! * every file the viewer asks for is fetched from the bridge in the
//!   background and handed back on a later frame (the old model keeps
//!   drawing meanwhile);
//! * the viewer's log lines go to the console (the page's log line);
//! * the address is kept up to date with `history.replaceState`;
//! * the panel shows the model, each tier's triangle count and the fallback
//!   text when there is no bridge or no model;
//! * ordinary pointer events orbit and zoom (drag, wheel, double-click to
//!   frame). No pointer lock, no keyboard lock, no fullscreen; a drag ends
//!   on pointer up, window blur and when the tab is hidden.

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::App;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::Overlay;
use evani_core::time::Time;
use evani_kit_model_view::view::TIERS;
use evani_kit_model_view::{Address, PointerInput, Status, ViewLog, Viewer};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Document, Element, Event, PointerEvent, WheelEvent};

/// A click on the panel.
enum UiCmd {
    Tier(String),
    Model(String),
}

/// What the browser's callbacks hand to the frame system.
#[derive(Default)]
struct Shared {
    done: Vec<(u64, Result<Vec<u8>, String>)>,
    pointer: PointerInput,
    /// The pointer holding the drag, and where it was last.
    drag: Option<(i32, [f32; 2])>,
    cmds: Vec<UiCmd>,
    /// Model ids from `/model/v1/models`, newest first.
    models: Option<Vec<String>>,
}

type SharedRc = Rc<RefCell<Shared>>;

/// Add the page system to the app.
pub fn install(app: App) -> App {
    let shared: SharedRc = Rc::new(RefCell::new(Shared::default()));
    let mut dom = match Dom::create(&shared) {
        Ok(d) => Some(d),
        Err(e) => {
            web_sys::console::error_1(&JsValue::from_str(&format!("model-view: page setup failed: {e}")));
            None
        }
    };
    let mut started = false;
    app.with_system(
        SystemDesc::new("model_view.page", Stage::Frame, move |world: &mut World| {
            page_frame(world, &shared, &mut started, dom.as_mut());
        })
        .order(-10),
    )
}

fn page_frame(world: &mut World, shared: &SharedRc, started: &mut bool, dom: Option<&mut Dom>) {
    let now_ms = world.res::<Time>().now_ms;
    if !*started {
        // The engine's stats overlay would cover the panel (tiers and their
        // triangle counts); like the games, the page starts with it hidden.
        world.res_mut::<Overlay>().enabled = false;
    }
    {
        let mut v = world.res_mut::<Viewer>();
        let mut sh = shared.borrow_mut();
        if !*started {
            *started = true;
            let hash = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
            v.start(&Address::parse(&hash), now_ms);
            fetch_models(shared.clone());
        }
        // Panel clicks wait while a load (perhaps the tab's) is in flight,
        // so a click never supersedes a command the tab is waiting on.
        if !v.loading() {
            let cmds = std::mem::take(&mut sh.cmds);
            if let Some(cmd) = cmds.into_iter().last() {
                let (model, version, tier) = match (&cmd, &v.shown) {
                    (UiCmd::Tier(t), Some(s)) => (Some(s.model.clone()), Some(s.version), t.clone()),
                    (UiCmd::Tier(_), None) => (None, None, String::new()),
                    (UiCmd::Model(m), s) => (Some(m.clone()), None, s.as_ref().map_or_else(|| "high".to_string(), |s| s.tier.clone())),
                };
                if model.is_some() {
                    let keep = matches!(cmd, UiCmd::Tier(_));
                    let _ = v.begin(model, version, tier, keep, None, now_ms);
                }
            }
        }
        let p = std::mem::take(&mut sh.pointer);
        v.pointer.drag[0] += p.drag[0];
        v.pointer.drag[1] += p.drag[1];
        v.pointer.wheel += p.wheel;
        v.pointer.frame |= p.frame;
        for (id, url) in v.fetches.want.drain(..) {
            let sh = shared.clone();
            spawn_local(async move {
                let got = fetch_bytes(&url).await;
                sh.borrow_mut().done.push((id, got));
            });
        }
        v.fetches.done.append(&mut sh.done);
    }
    for line in world.res_mut::<ViewLog>().0.drain(..) {
        web_sys::console::log_1(&JsValue::from_str(&line));
    }
    if let Some(dom) = dom {
        let v = world.res::<Viewer>();
        let sh = shared.borrow();
        dom.sync(&v, &sh);
    }
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let win = web_sys::window().ok_or("no window")?;
    let resp = JsFuture::from(win.fetch_with_str(url)).await.map_err(|_| "network error".to_string())?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a response".to_string())?;
    if !resp.ok() {
        return Err(format!("http {}", resp.status()));
    }
    let body = resp.array_buffer().map_err(|_| "no body".to_string())?;
    let buf = JsFuture::from(body).await.map_err(|_| "the body did not arrive".to_string())?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// The model list for the panel's model buttons.
fn fetch_models(shared: SharedRc) {
    spawn_local(async move {
        let Ok(bytes) = fetch_bytes("/model/v1/models").await else { return };
        let Ok(list) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return };
        let ids = list
            .as_array()
            .map(|a| a.iter().filter_map(|m| m.get("model").and_then(|x| x.as_str()).map(str::to_string)).collect())
            .unwrap_or_default();
        shared.borrow_mut().models = Some(ids);
    });
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `12345` → `12,345`.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn by_id(doc: &Document, id: &str) -> Result<Element, String> {
    doc.get_element_by_id(id).ok_or_else(|| format!("index.html has no #{id}"))
}

/// The page's panel and fallback, written only when what they show changes.
struct Dom {
    fallback: Element,
    fallback_line: Element,
    panel: Element,
    shown: Element,
    tiers: Element,
    models: Element,
    seen_fallback: Option<Option<String>>,
    seen_shown: String,
    seen_tiers: String,
    seen_models: String,
    seen_address: String,
}

impl Dom {
    fn create(shared: &SharedRc) -> Result<Self, String> {
        let win = web_sys::window().ok_or("no window")?;
        let doc = win.document().ok_or("no document")?;
        let dom = Dom {
            fallback: by_id(&doc, "fallback")?,
            fallback_line: by_id(&doc, "fallback-line")?,
            panel: by_id(&doc, "panel")?,
            shown: by_id(&doc, "shown")?,
            tiers: by_id(&doc, "tiers")?,
            models: by_id(&doc, "models")?,
            seen_fallback: None,
            seen_shown: String::new(),
            seen_tiers: String::new(),
            seen_models: String::new(),
            seen_address: String::new(),
        };
        // Panel buttons: one delegated click listener.
        {
            let sh = shared.clone();
            let click = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
                let Some(el) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
                let Ok(Some(btn)) = el.closest("button") else { return };
                if let Some(t) = btn.get_attribute("data-tier") {
                    sh.borrow_mut().cmds.push(UiCmd::Tier(t));
                } else if let Some(m) = btn.get_attribute("data-model") {
                    sh.borrow_mut().cmds.push(UiCmd::Model(m));
                }
            });
            dom.panel.add_event_listener_with_callback("click", click.as_ref().unchecked_ref()).map_err(|_| "panel click listener")?;
            click.forget();
        }
        let canvas = by_id(&doc, "evani-canvas")?;
        listen_pointer(&canvas, shared)?;
        // A drag ends when the window loses focus or the tab is hidden.
        let end = |sh: SharedRc| {
            Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                sh.borrow_mut().drag = None;
            })
        };
        let blur = end(shared.clone());
        win.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref()).map_err(|_| "blur listener")?;
        blur.forget();
        let hide = end(shared.clone());
        doc.add_event_listener_with_callback("visibilitychange", hide.as_ref().unchecked_ref()).map_err(|_| "visibility listener")?;
        hide.forget();
        Ok(dom)
    }

    fn sync(&mut self, v: &Viewer, sh: &Shared) {
        // The fallback panel: shown only when there is nothing to draw.
        let fallback = match &v.status {
            Status::NoModels(line) | Status::Error(line) if v.shown.is_none() => Some(line.clone()),
            _ => None,
        };
        if self.seen_fallback.as_ref() != Some(&fallback) {
            match &fallback {
                Some(line) => {
                    self.fallback_line.set_text_content(Some(line));
                    self.fallback.set_class_name("on");
                }
                None => self.fallback.set_class_name(""),
            }
            self.seen_fallback = Some(fallback);
        }
        // What is shown, and each tier's triangle count from the metadata.
        let (shown, tiers) = match &v.shown {
            Some(s) => {
                let tiers_meta = s.meta.get("tiers");
                let mut buttons = String::new();
                for t in TIERS {
                    let Some(tm) = tiers_meta.and_then(|m| m.get(t)) else { continue };
                    let tris = tm.get("tris").and_then(|x| x.as_u64()).map_or_else(|| "?".to_string(), thousands);
                    let on = if t == s.tier { " class=\"on\"" } else { "" };
                    buttons.push_str(&format!("<button data-tier=\"{t}\"{on}><b>{t}</b><span>{tris} triangles</span></button>"));
                }
                let busy = if v.loading() { " &middot; loading&hellip;" } else { "" };
                let line = format!(
                    "<b>{}</b> v{} &middot; {} &middot; {} triangles drawn, {} colour{}{busy}",
                    escape(&s.model),
                    s.version,
                    escape(&s.tier),
                    thousands(u64::from(s.decoded.tris)),
                    s.decoded.groups.len(),
                    if s.decoded.groups.len() == 1 { "" } else { "s" }
                );
                (line, buttons)
            }
            None if v.loading() => ("Loading&hellip;".to_string(), String::new()),
            None => (String::new(), String::new()),
        };
        if shown != self.seen_shown {
            self.shown.set_inner_html(&shown);
            self.seen_shown = shown;
        }
        if tiers != self.seen_tiers {
            self.tiers.set_inner_html(&tiers);
            self.seen_tiers = tiers;
        }
        // Model buttons (only when there is more than one model).
        let current = v.shown.as_ref().map(|s| s.model.as_str());
        let models = match &sh.models {
            Some(ids) if ids.len() > 1 => ids
                .iter()
                .map(|m| {
                    let on = if Some(m.as_str()) == current { " class=\"on\"" } else { "" };
                    format!("<button data-model=\"{0}\"{on}>{0}</button>", escape(m))
                })
                .collect::<String>(),
            _ => String::new(),
        };
        if models != self.seen_models {
            self.models.set_inner_html(&models);
            self.seen_models = models;
        }
        // The page's address holds the view (4.4).
        let addr = v.address();
        if addr != self.seen_address {
            if let Some(h) = web_sys::window().and_then(|w| w.history().ok()) {
                let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&addr));
            }
            self.seen_address = addr;
        }
    }
}

/// Drag to orbit, wheel to zoom, double-click to frame.
fn listen_pointer(canvas: &Element, shared: &SharedRc) -> Result<(), String> {
    let on = |name: &str, f: Closure<dyn FnMut(Event)>| -> Result<(), String> {
        canvas.add_event_listener_with_callback(name, f.as_ref().unchecked_ref()).map_err(|_| format!("{name} listener"))?;
        f.forget();
        Ok(())
    };
    let sh = shared.clone();
    let c = canvas.clone();
    on(
        "pointerdown",
        Closure::new(move |e: Event| {
            let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
            if p.button() != 0 {
                return;
            }
            let _ = c.set_pointer_capture(p.pointer_id());
            sh.borrow_mut().drag = Some((p.pointer_id(), [p.client_x() as f32, p.client_y() as f32]));
        }),
    )?;
    let sh = shared.clone();
    on(
        "pointermove",
        Closure::new(move |e: Event| {
            let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
            let mut s = sh.borrow_mut();
            let Some((id, last)) = s.drag else { return };
            if id != p.pointer_id() {
                return;
            }
            let at = [p.client_x() as f32, p.client_y() as f32];
            s.pointer.drag[0] += at[0] - last[0];
            s.pointer.drag[1] += at[1] - last[1];
            s.drag = Some((id, at));
        }),
    )?;
    for name in ["pointerup", "pointercancel", "lostpointercapture"] {
        let sh = shared.clone();
        on(
            name,
            Closure::new(move |_e: Event| {
                sh.borrow_mut().drag = None;
            }),
        )?;
    }
    let sh = shared.clone();
    on(
        "wheel",
        Closure::new(move |e: Event| {
            let Some(w) = e.dyn_ref::<WheelEvent>() else { return };
            e.prevent_default();
            // deltaMode 1 is lines, 2 pages; turn both into pixels.
            let scale = match w.delta_mode() {
                1 => 16.0,
                2 => 400.0,
                _ => 1.0,
            };
            sh.borrow_mut().pointer.wheel += w.delta_y() as f32 * scale;
        }),
    )?;
    let sh = shared.clone();
    on(
        "dblclick",
        Closure::new(move |_e: Event| {
            sh.borrow_mut().pointer.frame = true;
        }),
    )?;
    Ok(())
}
