//! The studio page's procedural mode on the page (wasm only; the markup is
//! [`proc_html`]'s, the state [`ProcState`]'s). page.rs mounts it, hands it
//! the page's clicks and inputs first and calls [`frame`] once a frame.
//!
//! - The mode switch at the top opens the `procedural` scene ("Procedural")
//!   or the studio's own ("Materials"); the mode is the scene shown, so a
//!   live agent or the legacy scene row switch it the same way.
//! - In procedural mode the materials panels are hidden and the effect
//!   cards and the effect panel shown ([`proc_html::panels`]).
//! - Each part is rebuilt only when what it shows changes; the sliders'
//!   values, readouts, "changed" marks and the "Use in a game" text follow
//!   the registry's revision without a rebuild (never the input in use).
//! - A card's picture is the live view itself: once the open item's view
//!   has held still for a second its middle is copied into the card (and
//!   kept in localStorage for the next visit). A view that reads back empty
//!   keeps the drawn placeholder.
//! - The Library tab ("Library") is the procedural scene with the left
//!   column listing the approved assets from evani.app ([`lh`]):
//!   `./evani-library.json` and `./library-contents.json`, fetched at start
//!   and on Refresh (nothing beyond the page's own server: Evani.App serves
//!   both from its checked cache). The last good list is kept in
//!   localStorage for when the next one doesn't load. A reference picture is
//!   fetched from `./evani-library/blobs/<sha256>` and shown only once its
//!   SHA-256 (SubtleCrypto) matches the list's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_kit_material_view::{Command, Viewer};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, Document, Element, HtmlCanvasElement, HtmlElement, HtmlInputElement};

use super::lib_data::{self as ld, Contents, Fetched, Saved, Shown};
use super::lib_html::{self as lh, LibView, Pic};
use super::proc_data as pd;
use super::proc_html::{self as ph, ProcUi};
use super::proc_view::{PROC_SCENE, ProcCommand, ProcState};

/// The studio's own scene (the "Materials" side).
const STUDIO_SCENE: &str = "studio";
/// Frames the view must hold still before a card picture is taken.
const SETTLE_FRAMES: u32 = 60;
/// A card picture's size (device px).
const PIC: (u32, u32) = (224, 140);
// v2 (2026-11-02): the Ocean card became the spectral sea; pictures taken
// of the older water are dropped, not shown.
const THUMB_KEY: &str = "evani.studio.proc.pic2.";
/// The Library tab's last good list (localStorage): its text and when.
const SAVED_KEY: &str = "evani.library.saved1.text";
const SAVED_AT_KEY: &str = "evani.library.saved1.at";

/// The parts the procedural mode adds to the page.
pub struct ProcDom {
    doc: Document,
    root: HtmlElement,
    mode: Element,
    list: Element,
    panel: Element,
    pic: HtmlCanvasElement,
    pic_ctx: CanvasRenderingContext2d,
}

/// The mode's page state.
#[derive(Default)]
pub struct ProcPage {
    keys: [String; 3],
    proc_was: Option<bool>,
    ui: ProcUi,
    values_at: u64,
    /// Card pictures by item (data: URLs).
    pics: HashMap<String, String>,
    pics_rev: u64,
    loaded: bool,
    /// What the view shows (item, preset, values) and for how many frames.
    still: (String, u32),
    /// What the open item's picture was taken of.
    taken: HashMap<String, String>,
    lib: LibPage,
}

/// What a fetch hands back: the list and the build's contents list.
type Answer = Rc<RefCell<Option<(Fetched, Fetched)>>>;
/// Checked pictures: the SHA-256 wanted, and the one worked out with the
/// picture's object URL (none: not served or not hashable).
type PicAnswers = Rc<RefCell<Vec<(String, Option<(String, String)>)>>>;

/// The Library tab's page state.
#[derive(Default)]
struct LibPage {
    /// The tab is open (the procedural scene with the library's list).
    on: bool,
    started: bool,
    loading: bool,
    answer: Answer,
    shown: Option<Shown>,
    contents: Option<Contents>,
    /// Moves whenever what the tab shows changes.
    rev: u64,
    pics: HashMap<String, Pic>,
    asked: HashSet<String>,
    pic_answers: PicAnswers,
}

/// `fetch(url)` of a page file, never cached: `{status, text}`, or status 0
/// and the reason when it couldn't be read.
const FETCH_JS: &str = "return fetch(url,{cache:'no-store'}).then(function(r){if(!r.ok)return {status:r.status};\
return r.text().then(function(t){return {status:r.status,text:t}})}).catch(function(e){return {status:0,text:String(e&&e.message||e)}});";

/// A picture's bytes, their SHA-256 (hex) and an object URL of them; null
/// when it isn't served or can't be hashed (then it is never shown).
const PIC_JS: &str = "return fetch(url,{cache:'no-store'}).then(function(r){if(!r.ok)return null;return r.arrayBuffer().then(function(b){\
return crypto.subtle.digest('SHA-256',b).then(function(d){var h=Array.prototype.map.call(new Uint8Array(d),function(x){return(x<16?'0':'')+x.toString(16)}).join('');\
return {hex:h,url:URL.createObjectURL(new Blob([b],{type:type}))}})})}).catch(function(){return null});";

/// Drop an object URL made for a picture that failed its check.
const REVOKE_JS: &str = "try{URL.revokeObjectURL(url)}catch(e){}";

fn field(o: &JsValue, k: &str) -> Option<JsValue> {
    js_sys::Reflect::get(o, &JsValue::from_str(k)).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

async fn fetch_text(url: &str) -> Fetched {
    let asked = js_sys::Function::new_with_args("url", FETCH_JS).call1(&JsValue::NULL, &JsValue::from_str(url)).ok().and_then(|p| p.dyn_into::<js_sys::Promise>().ok());
    let Some(p) = asked else { return Fetched::Failed("this browser couldn't ask for it".into()) };
    let Ok(r) = wasm_bindgen_futures::JsFuture::from(p).await else { return Fetched::Failed("this browser couldn't ask for it".into()) };
    let status = field(&r, "status").and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    let text = field(&r, "text").and_then(|v| v.as_string()).unwrap_or_default();
    match status {
        200..=299 => Fetched::Text(text),
        404 => Fetched::Missing,
        0 => Fetched::Failed("the window couldn't reach Evani.App".into()),
        n => Fetched::Failed(format!("Evani.App answered with error {n}")),
    }
}

impl LibPage {
    /// Ask for the list and the contents list (again).
    fn fetch(&mut self) {
        self.started = true;
        self.loading = true;
        self.rev += 1;
        let answer = self.answer.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let list = fetch_text(ld::MANIFEST_FILE).await;
            let contents = fetch_text(ld::CONTENTS_FILE).await;
            *answer.borrow_mut() = Some((list, contents));
        });
    }

    /// Take in what arrived: the list (or the saved copy), the build's
    /// contents and the checked pictures.
    fn poll(&mut self) {
        let got = self.answer.borrow_mut().take();
        if let Some((list, contents)) = got {
            let text = storage("getItem", &[SAVED_KEY]).and_then(|v| v.as_string());
            let at = storage("getItem", &[SAVED_AT_KEY]).and_then(|v| v.as_string());
            let saved = text.zip(at).map(|(text, saved_at)| Saved { text, saved_at });
            let (shown, save) = ld::resolve(&list, saved.as_ref());
            if save
                && let Fetched::Text(t) = &list
                && storage("setItem", &[SAVED_KEY, t]).is_some()
            {
                let _ = storage("setItem", &[SAVED_AT_KEY, &String::from(js_sys::Date::new_0().to_iso_string())]);
            }
            if let (Shown::Nothing { why, .. }, Fetched::Text(_)) = (&shown, &list) {
                web_sys::console::warn_1(&JsValue::from_str(&format!("material-view: the library list wasn't used: {why}")));
            }
            self.contents = match contents {
                Fetched::Text(t) => match ld::read_contents(&t) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        web_sys::console::warn_1(&JsValue::from_str(&format!("material-view: {e}")));
                        None
                    }
                },
                _ => None,
            };
            self.shown = Some(shown);
            self.loading = false;
            self.rev += 1;
            self.ask_pictures();
        }
        let done: Vec<_> = std::mem::take(&mut *self.pic_answers.borrow_mut());
        for (want, got) in done {
            match ld::check_picture(&want, got.as_ref().map(|(h, _)| h.as_str())) {
                ld::PictureCheck::Good => {
                    if let Some((_, url)) = got {
                        self.pics.insert(want, Pic::Good(url));
                    }
                }
                ld::PictureCheck::Mismatch => {
                    if let Some((_, url)) = got {
                        let _ = js_sys::Function::new_with_args("url", REVOKE_JS).call1(&JsValue::NULL, &JsValue::from_str(&url));
                    }
                    web_sys::console::warn_1(&JsValue::from_str(&format!("material-view: library picture {want} didn't match its check")));
                    self.pics.insert(want, Pic::Mismatch);
                }
                ld::PictureCheck::Absent => continue,
            }
            self.rev += 1;
        }
    }

    /// Fetch and check each listed reference picture once.
    fn ask_pictures(&mut self) {
        let Some(Shown::List { listing, .. }) = &self.shown else { return };
        for p in listing.assets.iter().filter_map(|a| a.reference.as_ref()) {
            if !self.asked.insert(p.sha256.clone()) {
                continue;
            }
            let url = format!("{}{}", ld::PICTURE_DIR, p.sha256);
            let asked = js_sys::Function::new_with_args("url, type", PIC_JS).call2(&JsValue::NULL, &JsValue::from_str(&url), &JsValue::from_str(&p.kind));
            let Some(promise) = asked.ok().and_then(|x| x.dyn_into::<js_sys::Promise>().ok()) else { continue };
            let (answers, want) = (self.pic_answers.clone(), p.sha256.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let r = wasm_bindgen_futures::JsFuture::from(promise).await.ok();
                let got = r.as_ref().and_then(|r| Some((field(r, "hex")?.as_string()?, field(r, "url")?.as_string()?)));
                answers.borrow_mut().push((want, got));
            });
        }
    }
}

fn el(doc: &Document, tag: &str, id: &str, class: &str) -> Result<Element, String> {
    let e = doc.create_element(tag).map_err(|_| "studio page: procedural element")?;
    e.set_id(id);
    e.set_class_name(class);
    Ok(e)
}

/// Add the mode switch (top of `#st-top`), the cards and the panel.
pub fn mount(doc: &Document, root: &HtmlElement) -> Result<ProcDom, String> {
    let style = doc.create_element("style").map_err(|_| "studio page: style")?;
    style.set_text_content(Some(&format!("{}{}", ph::CSS, lh::CSS)));
    doc.body().ok_or("studio page: no body")?.append_child(&style).map_err(|_| "studio page: append style")?;
    let top = root.query_selector("#st-top").ok().flatten().ok_or("studio page: no #st-top")?;
    let mode = el(doc, "div", "st-mode", "st-panel")?;
    let _ = mode.set_attribute("role", "tablist");
    let _ = mode.set_attribute("aria-label", "Library section");
    top.prepend_with_node_1(&mode).map_err(|_| "studio page: mode")?;
    let list = el(doc, "aside", "st-plist", "st-panel")?;
    let _ = list.set_attribute("aria-label", "Procedural effects");
    let panel = el(doc, "aside", "st-ppanel", "st-panel")?;
    let _ = panel.set_attribute("aria-label", "Effect settings");
    for e in [&list, &panel] {
        e.set_attribute("hidden", "").map_err(|_| "studio page: hidden")?;
        root.append_child(e).map_err(|_| "studio page: append")?;
    }
    let pic: HtmlCanvasElement = doc.create_element("canvas").map_err(|_| "studio page: canvas")?.dyn_into().map_err(|_| "studio page: canvas")?;
    pic.set_width(PIC.0);
    pic.set_height(PIC.1);
    let pic_ctx: CanvasRenderingContext2d = pic.get_context("2d").ok().flatten().and_then(|c| c.dyn_into().ok()).ok_or("studio page: 2d")?;
    Ok(ProcDom { doc: doc.clone(), root: root.clone(), mode, list, panel, pic, pic_ctx })
}

fn set_param(world: &mut World, name: &str, v: ParamValue) {
    let mut reg = world.res_mut::<ParamRegistry>();
    if let Some(id) = reg.id(name) {
        let _ = reg.set(id, v);
    }
}

fn copy(text: &str) -> bool {
    let Some(win) = web_sys::window() else { return false };
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &JsValue::from_str(k)).ok().filter(|v| !v.is_undefined() && !v.is_null());
    let Some(clip) = get(&win, "navigator").and_then(|n| get(&n, "clipboard")) else { return false };
    let Some(f) = get(&clip, "writeText").and_then(|f| f.dyn_into::<js_sys::Function>().ok()) else { return false };
    f.call1(&clip, &JsValue::from_str(text)).is_ok()
}

/// One of the mode's controls (true: it was one; page.rs then skips it).
/// Returns a message for the page's notice line when there is one.
pub fn handle(world: &mut World, page: &mut ProcPage, act: &str, v: &str, input: &str) -> Option<Option<String>> {
    let push = |world: &mut World, c: ProcCommand| {
        if let Some(mut s) = world.try_res_mut::<ProcState>() {
            s.push(c);
        }
    };
    match act {
        "mode" => {
            page.lib.on = v == "lib";
            let scene = if v == "proc" || v == "lib" { PROC_SCENE } else { STUDIO_SCENE };
            if let Some(mut vw) = world.try_res_mut::<Viewer>()
                && vw.scene != scene
            {
                vw.push(Command::Scene(scene.into()));
            }
        }
        "proc-item" | "lib-open" => push(world, ProcCommand::Open(v.into())),
        "lib-refresh" => {
            page.lib.fetch();
            return Some(Some("Asking Evani.App for the latest library list".into()));
        }
        "proc-preset" => push(world, ProcCommand::Preset(v.into())),
        "proc-reset" => push(world, ProcCommand::Reset(v.into())),
        "proc-reset-all" => push(world, ProcCommand::ResetAll),
        "proc-on" => push(world, ProcCommand::Toggle(v.into())),
        "proc-num" => {
            if let Ok(x) = input.parse::<f32>() {
                set_param(world, v, ParamValue::F32(x));
            }
        }
        "proc-col" => {
            if let Some(c) = pd::from_hex(input) {
                set_param(world, v, ParamValue::Color(c));
            }
        }
        "proc-quality" => {
            let q = world.try_res::<ProcState>().and_then(|s| s.open_item().and_then(|it| it.quality.clone()));
            if let (Some(q), Ok(x)) = (q, v.parse::<f32>()) {
                set_param(world, &q, ParamValue::F32(x));
            }
        }
        "proc-adv" => page.ui.advanced = !page.ui.advanced,
        "proc-game" => page.ui.game = !page.ui.game,
        "proc-copy" => {
            let text = {
                let st = world.try_res::<ProcState>();
                let reg = world.res::<ParamRegistry>();
                st.and_then(|s| s.open_item().map(|it| ph::game_text(it, &reg)))
            };
            let ok = text.is_some_and(|t| copy(&t));
            return Some(Some(if ok { "Copied: the module line and the params".into() } else { "This browser refused the clipboard".into() }));
        }
        _ => return None,
    }
    Some(None)
}

fn put(el: &Element, last: &mut String, key: String, html: impl FnOnce() -> String) {
    if *last != key {
        *last = key;
        el.set_inner_html(&html());
    }
}

fn storage(method: &str, args: &[&str]) -> Option<JsValue> {
    let win = web_sys::window()?;
    let ls = js_sys::Reflect::get(&win, &JsValue::from_str("localStorage")).ok().filter(JsValue::is_object)?;
    let f: js_sys::Function = js_sys::Reflect::get(&ls, &JsValue::from_str(method)).ok()?.dyn_into().ok()?;
    let a = js_sys::Array::new();
    for x in args {
        a.push(&JsValue::from_str(x));
    }
    f.apply(&ls, &a).ok()
}

/// The middle of the live view as a small JPEG, or `None` when the view
/// reads back empty (a browser that won't copy a WebGPU canvas).
fn take_picture(d: &ProcDom) -> Option<String> {
    let canvas: HtmlCanvasElement = d.doc.get_element_by_id("evani-canvas")?.dyn_into().ok()?;
    let view = d.root.query_selector("#st-view").ok().flatten()?.get_bounding_client_rect();
    let c = canvas.get_bounding_client_rect();
    if c.width() < 1.0 || view.width() < 8.0 || view.height() < 8.0 {
        return None;
    }
    let (sx, sy) = (f64::from(canvas.width()) / c.width(), f64::from(canvas.height()) / c.height());
    // The view's middle at the card's shape.
    let aspect = f64::from(PIC.0) / f64::from(PIC.1);
    let (mut w, mut h) = (view.width() * 0.8, view.height() * 0.8);
    if w / h > aspect {
        w = h * aspect;
    } else {
        h = w / aspect;
    }
    let x = view.left() - c.left() + (view.width() - w) * 0.5;
    let y = view.top() - c.top() + (view.height() - h) * 0.5;
    let ctx = &d.pic_ctx;
    ctx.clear_rect(0.0, 0.0, f64::from(PIC.0), f64::from(PIC.1));
    ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(&canvas, x * sx, y * sy, w * sx, h * sy, 0.0, 0.0, f64::from(PIC.0), f64::from(PIC.1)).ok()?;
    let seen = [(0.5, 0.5), (0.2, 0.2), (0.8, 0.8)].iter().any(|(fx, fy)| {
        let (px, py) = ((f64::from(PIC.0) * fx) as i32, (f64::from(PIC.1) * fy) as i32);
        ctx.get_image_data(px, py, 1, 1).is_ok_and(|im| im.data().0.get(3).is_some_and(|a| *a > 0))
    });
    if !seen {
        return None;
    }
    d.pic.to_data_url_with_type_and_encoder_options("image/jpeg", &JsValue::from_f64(0.82)).ok()
}

/// The mode's frame: the switch, the parts' visibility, and in procedural
/// mode the cards and the panel. Returns whether procedural mode is on.
pub fn frame(world: &mut World, d: &ProcDom, page: &mut ProcPage) -> bool {
    let proc = world.try_res::<Viewer>().is_some_and(|v| v.scene == PROC_SCENE);
    let Some(st) = world.try_res::<ProcState>().map(|s| s.clone()) else { return false };
    // The library's list is fetched once at start (and on Refresh).
    if !page.lib.started {
        page.lib.fetch();
    }
    page.lib.poll();
    let lib_on = page.lib.on;
    put(&d.mode, &mut page.keys[0], format!("{proc}|{lib_on}"), || ph::mode_html(proc, lib_on));
    if page.proc_was != Some(proc) {
        page.proc_was = Some(proc);
        for (id, show) in ph::panels(proc) {
            // The notice line and the more-materials sheet show themselves
            // when they have something; here they are only ever hidden.
            if show && (id == "st-note" || id == "st-morepop") {
                continue;
            }
            if let Some(e) = d.root.query_selector(&format!("#{id}")).ok().flatten() {
                let _ = if show { e.remove_attribute("hidden") } else { e.set_attribute("hidden", "") };
            }
        }
        page.keys[1].clear();
        page.keys[2].clear();
        page.values_at = 0;
    }
    if !proc {
        return false;
    }
    if !page.loaded {
        page.loaded = true;
        for it in st.lib.ready() {
            if let Some(src) = storage("getItem", &[&format!("{THUMB_KEY}{}", it.id)]).and_then(|v| v.as_string()).filter(|s| s.starts_with("data:image/")) {
                page.pics.insert(it.id.clone(), src);
            }
        }
        page.pics_rev += 1;
    }
    // The frame's GPU time and tier on the page root (`data-gpu-ms`,
    // `data-gpu-n`, `data-tier`), for the shot and timing tools.
    if let Some(s) = world.try_res::<evani_core::overlay::RenderStats>()
        && d.root.get_attribute("data-gpu-n").as_deref() != Some(s.gpu_samples.to_string().as_str())
    {
        let _ = d.root.set_attribute("data-gpu-n", &s.gpu_samples.to_string());
        let _ = d.root.set_attribute("data-gpu-ms", &s.gpu_ms.map_or_else(|| s.gpu_note.to_string(), |g| format!("{g:.3}")));
        let tier = world.try_res::<evani_core::tiers::Tiers>().map_or_else(String::new, |t| t.active().id.clone());
        let _ = d.root.set_attribute("data-tier", &tier);
        // The shadow box's half width (m), so a shot can show whether the
        // library view fitted it to the object.
        let reg = world.res::<ParamRegistry>();
        if let Some(id) = reg.id("fx.shadows.distance")
            && let Some(evani_core::params::ParamValue::F32(x)) = reg.value(id)
        {
            let _ = d.root.set_attribute("data-shadow-half", &format!("{x:.2}"));
        }
    }
    let reg_rev = world.res::<ParamRegistry>().revision();
    // The card picture: the open item once its view holds still.
    let preset = st.open_preset().unwrap_or_default().to_string();
    let (orbit, turning) = world.try_res::<Viewer>().map_or((String::new(), false), |v| (format!("{:?}", v.orbit), v.spin));
    // The item's own values (the registry's revision also moves for
    // things the view doesn't show).
    let vals = st.open_item().map(|it| format!("{:?}", pd::read(&world.res::<ParamRegistry>(), &it.params()))).unwrap_or_default();
    let now = format!("{}|{preset}|{vals}|{orbit}", st.item);
    if page.still.0 == now {
        page.still.1 = page.still.1.saturating_add(1);
    } else {
        page.still = (now.clone(), 0);
    }
    if page.still.1 == SETTLE_FRAMES && !turning && page.taken.get(&st.item) != Some(&now) {
        page.taken.insert(st.item.clone(), now);
        if let Some(src) = take_picture(d) {
            let _ = storage("setItem", &[&format!("{THUMB_KEY}{}", st.item), &src]);
            page.pics.insert(st.item.clone(), src);
            page.pics_rev += 1;
        }
    }
    let pics = &page.pics;
    let mut presets: Vec<String> = st.preset.iter().map(|(k, v)| format!("{k}={v}")).collect();
    presets.sort();
    // The library's switches (a live agent may flip one with param_set).
    let offs: Vec<&str> = {
        let reg = world.res::<ParamRegistry>();
        st.lib.ready().filter(|i| !super::proc_view::is_on(&reg, i.on_param.as_ref())).map(|i| i.id.as_str()).collect()
    };
    if lib_on {
        let l = &page.lib;
        put(&d.list, &mut page.keys[1], format!("lib|{}|{}|{}", st.item, page.pics_rev, l.rev), || {
            let v = LibView { loading: l.loading, open: &st.item, pics: Some(&l.pics), thumbs: Some(pics) };
            lh::section_html(l.shown.as_ref(), l.contents.as_ref(), &st.lib, &v)
        });
    } else {
        let reg = world.res::<ParamRegistry>();
        put(&d.list, &mut page.keys[1], format!("{}|{}|{presets:?}|{}|{offs:?}", st.item, page.pics_rev, st.lib.items.len()), || ph::list_html_with(&st.lib, &st, &|id| pics.get(id).cloned(), Some(&reg)));
    }
    let quality = st.open_item().and_then(|it| ph::quality_now(it, &world.res::<ParamRegistry>()));
    let open_off = offs.contains(&st.item.as_str());
    let rows_key = format!("{}|{preset}|{}|{}|{quality:?}|{open_off}", st.item, page.ui.advanced, page.ui.game);
    let rebuilt = page.keys[2] != rows_key;
    {
        let reg = world.res::<ParamRegistry>();
        put(&d.panel, &mut page.keys[2], rows_key, || ph::panel_html(&st, &reg, &page.ui));
        if rebuilt || page.values_at != reg_rev {
            page.values_at = reg_rev;
            values(d, &st, &reg, page.ui.game);
        }
    }
    true
}

/// Write the current values into the panel (never into the input in use).
fn values(d: &ProcDom, st: &ProcState, reg: &ParamRegistry, game: bool) {
    let Some(it) = st.open_item() else { return };
    let base = st.baseline(reg);
    let names: Vec<String> = it.headline.iter().chain(&it.advanced).cloned().collect();
    let focused = d.doc.active_element();
    for r in ph::rows(reg, &names, &base) {
        let q = |sel: String| d.panel.query_selector(&sel).ok().flatten();
        if let Some(row) = q(format!("[data-row=\"{}\"]", r.name)) {
            let _ = row.class_list().toggle_with_force("chg", r.changed);
        }
        let (txt, val) = match &r.kind {
            ph::RowKind::Num { value, step, unit, .. } => {
                let u = if unit.is_empty() || unit == "x" { String::new() } else { format!(" {unit}") };
                (format!("{}{u}", ph::readout(*value, *step)), value.to_string())
            }
            ph::RowKind::Colour { hex } => (hex.clone(), hex.clone()),
        };
        if let Some(o) = q(format!("[data-out=\"{}\"]", r.name))
            && o.text_content().as_deref() != Some(txt.as_str())
        {
            o.set_text_content(Some(&txt));
        }
        if let Some(i) = q(format!("input[data-v=\"{}\"]", r.name))
            && focused.as_ref() != Some(&i)
            && let Ok(i) = i.dyn_into::<HtmlInputElement>()
        {
            let differs = match &r.kind {
                ph::RowKind::Num { value, .. } => (i.value_as_number() as f32 - value).abs() > 1e-5,
                ph::RowKind::Colour { hex } => i.value() != *hex,
            };
            if differs {
                i.set_value(&val);
            }
        }
    }
    if game && let Some(pre) = d.panel.query_selector("[data-game]").ok().flatten() {
        let t = ph::game_text(it, reg);
        if pre.text_content().as_deref() != Some(t.as_str()) {
            pre.set_text_content(Some(&t));
        }
    }
}
