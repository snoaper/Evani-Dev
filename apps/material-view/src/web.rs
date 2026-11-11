//! The page side of the material viewer (wasm only). One frame system, run
//! before the kit's own (`.order(-10)`), so what the page asks for is applied
//! the same frame:
//!
//! * browser callbacks only push into [`Shared`]; the frame system drains it
//!   into [`Viewer::push`], the param registry (`set` / `reset`) and
//!   [`Tiers::try_set_active`];
//! * the tuning panel (panel.rs) is rebuilt only when the open material
//!   changes; when the registry's revision moves (a live edit, an AI, a
//!   preset) only the values are written, never the input being dragged;
//! * the saved values (A) are the material's values the frame after it
//!   opens (or when Save A is pressed); a group's reset puts its rows back
//!   to A;
//! * compare: with the camera settled, A is set for a few frames, the
//!   canvas is copied (`drawImage`) into an overlay canvas, B comes back;
//!   the overlay is clipped at `kit.material_view.split`, a vertical line in
//!   landscape and a horizontal one in portrait. If the copy reads back
//!   empty, compare falls back to holding a button to see A, and says so;
//! * gestures: one pointer orbits, the right button or two pointers pan,
//!   a pinch or the wheel zooms, a double tap resets. No pointer lock, no
//!   keyboard lock, no fullscreen; a drag ends on pointer up, window blur and
//!   when the tab is hidden.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::App;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::Overlay;
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::tiers::Tiers;
use evani_core::time::Time;
#[cfg(feature = "material-styles")]
use evani_core::plugin::LookData;
use evani_kit_material_view::library::{PanelRow, panel_rows};
#[cfg(feature = "material-styles")]
use evani_kit_material_view::styles::{StyleCommand, StyleView, StylesData, finish as style_finish};
use evani_kit_material_view::{Command, MaterialViewKit, Viewer, ViewerData};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, Document, Element, Event, HtmlCanvasElement, HtmlElement, HtmlInputElement, PointerEvent, WheelEvent};

use crate::panel;
#[cfg(feature = "material-catalogue")]
use evani_kit_material_view::catalogue::{self as kcat, CatalogueView, Filter as CatFilter, LibraryData};
#[cfg(feature = "material-styles")]
use crate::scenes::styles::FreeArea;

/// Frames the camera, light and tier must hold still before compare copies
/// the view again.
const SETTLE_FRAMES: u32 = 10;
/// Empty copies in a row before compare falls back to hold-to-flip.
const COPY_TRIES: u32 = 3;
/// Frames A stays set before the copy (a family may read its params in the
/// fixed update, one frame behind).
const A_FRAMES: u32 = 3;
/// Two taps this close (ms, CSS px) are a double tap.
const DOUBLE_TAP_MS: f64 = 350.0;
const TAP_SLOP_PX: f32 = 10.0;
/// Phones and narrow windows: the drawer and the panel are bottom sheets,
/// one at a time.
const NARROW_PX: f64 = 760.0;
/// Windows this short or shorter (a phone held sideways, 844x390) keep the
/// drawer and the panel closed too (hud-layout.md; critic round 2: both
/// opened cut off on a phone in landscape).
const SHORT_PX: f64 = 520.0;

/// The window is too narrow or too short for the side panels.
fn compact_size(w: f64, h: f64) -> bool {
    w < NARROW_PX || h <= SHORT_PX
}

/// A request from the page.
enum Ui {
    Cmd(Command),
    /// Set a numeric param (or one vec3 component).
    Set { name: String, comp: Option<usize>, v: f32 },
    Toggle(String),
    Choose(String, u32),
    Colour(String, [f32; 3]),
    /// Put a group's rows back to A.
    GroupReset(String),
    Tier(String),
    SaveA,
    /// An art styles command (the style bar, chips, shapes, resets).
    #[cfg(feature = "material-styles")]
    Style(StyleCommand),
    /// Open style `i`, switching to the styles scene if needed.
    #[cfg(feature = "material-styles")]
    StyleOpen(usize),
    /// Open a catalogue row (`none` closes it), switching to the styles
    /// scene: the same `kit.material_view.cat.open` an AI sets.
    #[cfg(feature = "material-catalogue")]
    CatOpen(String),
    /// A library filter chip (`panel::catalogue::apply_click`).
    #[cfg(feature = "material-catalogue")]
    CatFilter(String),
    /// The library's search box.
    #[cfg(feature = "material-catalogue")]
    CatQuery(String),
}

#[derive(Debug, Clone, Copy)]
struct Ptr {
    id: i32,
    at: [f32; 2],
    button: i16,
}

/// What the browser's callbacks hand to the frame system.
#[derive(Default)]
struct Shared {
    ui: Vec<Ui>,
    pointers: Vec<Ptr>,
    /// Accumulated since the last frame, CSS px.
    orbit: [f32; 2],
    pan: [f32; 2],
    /// Pinch, as the sum of ln(distance ratio).
    pinch: f32,
    /// Wheel, px (down = out).
    wheel: f32,
    reset: bool,
    /// Where and when a single-pointer press began, and whether it moved.
    press: Option<(f64, [f32; 2], bool)>,
    last_tap: Option<(f64, [f32; 2])>,
    /// A single tap's client position (the studio picks the part under it).
    tap: Option<[f32; 2]>,
    /// The sun dial's drag, px.
    sun: [f32; 2],
    sun_drag: Option<(i32, [f32; 2])>,
    /// The compare line, dragged (0..1).
    split: Option<f32>,
    split_drag: Option<i32>,
    /// The param whose input is being dragged (its value is not rewritten).
    dragging: Option<String>,
    /// Hold-to-flip (compare's fallback): A while held.
    hold: bool,
    /// The reference panel: shown, its docked width (CSS px, 0 = not set)
    /// and the pointer dragging its handle.
    #[cfg(feature = "material-styles")]
    refs_open: bool,
    #[cfg(feature = "material-styles")]
    refs_w: f32,
    #[cfg(feature = "material-styles")]
    refs_drag: Option<i32>,
}

type SharedRc = Rc<RefCell<Shared>>;

/// Compare's copy of the view.
enum Cap {
    Off,
    /// Waiting for the view to hold still.
    Settling(u32),
    /// A is set; the copy is taken when the count runs out. Holds B.
    ShowingA(u32, Vec<(ParamId, ParamValue)>),
    /// The copy is queued to run right after this frame is drawn (a WebGPU
    /// canvas can only be read in the task that drew it); its result lands
    /// in the cell. Holds B.
    Copying(Vec<(ParamId, ParamValue)>, Rc<Cell<Option<bool>>>),
    Ready,
}

/// The frame system's own state.
struct State {
    started: bool,
    material: String,
    rows: Vec<PanelRow>,
    /// The saved values (A) of the open material's params.
    a: Vec<(ParamId, ParamValue)>,
    a_pending: bool,
    reg_rev: u64,
    ui_key: String,
    last_now: f64,
    frame_ms: f64,
    ft_shown_at: f64,
    tier_seen: usize,
    cap: Cap,
    view_key: String,
    /// B while the fallback's hold shows A.
    held: Option<Vec<(ParamId, ParamValue)>>,
    split_seen: String,
    /// The canvas could not be copied: compare is hold-to-flip.
    fallback: bool,
    /// Copies that came back empty in a row.
    copy_fails: u32,
    compare_shown: bool,
    #[cfg(feature = "material-styles")]
    sty: StyleUi,
    #[cfg(feature = "material-catalogue")]
    cat: CatUi,
}

/// The library tab's state: the search and filters, and what its parts last
/// showed.
#[cfg(feature = "material-catalogue")]
#[derive(Default)]
struct CatUi {
    query: String,
    filter: CatFilter,
    key: String,
    opened: u64,
    shape: u64,
}

/// What the art styles' page parts last showed (each is rebuilt only when
/// its own input changes).
#[cfg(feature = "material-styles")]
#[derive(Default)]
struct StyleUi {
    started: bool,
    on_scene: Option<bool>,
    style: Option<usize>,
    rev: u64,
    head: String,
    mats: String,
    shapes: String,
    refs_key: String,
    refs_w: String,
    layout: String,
    dock_h: i32,
    saved_w: f32,
}

/// Add the page system to the app.
pub fn install(app: App) -> App {
    let shared: SharedRc = Rc::new(RefCell::new(Shared::default()));
    let mut dom = match Dom::create(&shared) {
        Ok(d) => Some(d),
        Err(e) => {
            web_sys::console::error_1(&JsValue::from_str(&format!("material-view: page setup failed: {e}")));
            None
        }
    };
    let mut st = State {
        started: false,
        material: String::new(),
        rows: Vec::new(),
        a: Vec::new(),
        a_pending: false,
        reg_rev: 0,
        ui_key: String::new(),
        last_now: 0.0,
        frame_ms: 16.7,
        ft_shown_at: 0.0,
        tier_seen: usize::MAX,
        cap: Cap::Off,
        view_key: String::new(),
        held: None,
        split_seen: String::new(),
        fallback: false,
        copy_fails: 0,
        compare_shown: false,
        #[cfg(feature = "material-styles")]
        sty: StyleUi::default(),
        #[cfg(feature = "material-catalogue")]
        cat: CatUi::default(),
    };
    app.with_system(
        SystemDesc::new("material_view.page", Stage::Frame, move |world: &mut World| {
            if let Some(dom) = dom.as_mut() {
                page_frame(world, &shared, &mut st, dom);
            }
        })
        .order(-10),
    )
}

/// Fit the page to the room each part really has (critic round 1: panels
/// overlapping on a phone). Going narrow closes the drawer and the panel,
/// so the view is never opened covered by a sheet; `--bottom` is the room
/// the bottom stack (the bar, the scene row, the dev strip) takes, measured,
/// so the side panels stop above it however the bar wraps.
fn fit_page(dom: &Dom) {
    thread_local! {
        static FIT: std::cell::Cell<(i8, i32)> = const { std::cell::Cell::new((-1, -1)) };
    }
    let Some(win) = web_sys::window() else { return };
    let num = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let (w, h) = (num(win.inner_width()), num(win.inner_height()));
    let narrow = compact_size(w, h);
    let (was, bottom) = FIT.with(std::cell::Cell::get);
    if was >= 0 && (was == 1) != narrow && narrow {
        let _ = dom.body.class_list().remove_2("drawer-open", "panel-open");
    }
    let top = ["mv-bar", "mv-scenes", "evani-convert-zone"]
        .iter()
        .filter_map(|id| dom.doc.get_element_by_id(id))
        .map(|e| e.get_bounding_client_rect())
        .filter(|r| r.height() > 2.0 && r.width() > 2.0)
        .map(|r| r.top())
        .fold(h, f64::min);
    let px = ((h - top).max(0.0) + 8.0).round() as i32;
    if px != bottom {
        let _ = dom.body.style().set_property("--bottom", &format!("{px}px"));
    }
    FIT.with(|f| f.set((i8::from(narrow), px)));
}

fn page_frame(world: &mut World, shared: &SharedRc, st: &mut State, dom: &mut Dom) {
    fit_page(dom);
    // A capture moment's drawer and panel (closed on a phone whatever it asks).
    if let Some((drawer, panel)) = world.try_res_mut::<Viewer>().and_then(|mut v| v.panels.take()) {
        let (w, h) = web_sys::window().map_or((0.0, 0.0), |win| {
            let num = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
            (num(win.inner_width()), num(win.inner_height()))
        });
        let compact = compact_size(w, h);
        dom.body_class("drawer-open", drawer && !compact);
        dom.body_class("panel-open", panel && !compact);
    }
    // A held proof moment: the library tab searched for its row, the drawer
    // open (on a phone too: the tile and its reason are what it shows).
    #[cfg(feature = "material-catalogue")]
    if let Some(q) = world.try_res_mut::<Viewer>().and_then(|mut v| v.lib_search.take()) {
        if let Some(inp) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("mv-cat-q")).and_then(|e| e.dyn_into::<HtmlInputElement>().ok()) {
            inp.set_value(&q);
        }
        st.cat.query = q;
        dom.body_class("tab-rooms", false);
        dom.body_class("tab-lib", true);
        dom.body_class("drawer-open", true);
        dom.body_class("panel-open", false);
    }
    let Some(data) = world.try_res::<ViewerData>().map(|d| d.0.clone()) else { return };
    let Some(k) = world.try_res::<MaterialViewKit>().map(|k| *k) else { return };
    let now = world.res::<Time>().now_ms;
    if !st.started {
        st.started = true;
        // The engine's stats overlay would cover the drawer; like the other
        // games, the page starts with it hidden.
        world.res_mut::<Overlay>().enabled = false;
        dom.set_html(&dom.lights, &panel::lights_html(&data, &world.res::<Viewer>()));
    }
    let capturing = matches!(st.cap, Cap::ShowingA(..)) || st.held.is_some();
    // 1. What the page asked for.
    let (orbit_speed, zoom_speed) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(k.orbit_speed), reg.f32(k.zoom_speed))
    };
    let view_h = dom.view_height().max(1.0);
    let mut cmds = Vec::new();
    let mut ui = Vec::new();
    let (split, hold) = {
        let mut sh = shared.borrow_mut();
        // The page's requests wait (a few frames) while compare holds A, so
        // B comes back intact.
        if !capturing {
            ui = std::mem::take(&mut sh.ui);
        }
        let o = std::mem::take(&mut sh.orbit);
        if o != [0.0, 0.0] {
            cmds.push(Command::Orbit { daz: o[0] / view_h * orbit_speed, del: o[1] / view_h * orbit_speed });
        }
        let p = std::mem::take(&mut sh.pan);
        if p != [0.0, 0.0] {
            cmds.push(Command::Pan { dx: p[0] / view_h, dy: p[1] / view_h });
        }
        // One wheel notch (100 px) or one pinch "step" moves by zoom_speed.
        let z = std::mem::take(&mut sh.wheel) / 100.0 * zoom_speed - std::mem::take(&mut sh.pinch);
        if z != 0.0 {
            cmds.push(Command::Zoom(z.exp()));
        }
        if std::mem::take(&mut sh.reset) {
            cmds.push(Command::Reset);
        }
        // A tap on the studio's view: where, as a share of the canvas.
        #[cfg(feature = "material-studio")]
        if let Some(at) = sh.tap.take() {
            let r = dom.canvas.get_bounding_client_rect();
            if r.width() > 0.0 && r.height() > 0.0 {
                let share = [((f64::from(at[0]) - r.left()) / r.width()) as f32, ((f64::from(at[1]) - r.top()) / r.height()) as f32];
                if let Some(mut t) = world.try_res_mut::<crate::studio::pick::StudioTap>() {
                    t.0 = Some(share);
                }
            }
        }
        #[cfg(not(feature = "material-studio"))]
        let _ = sh.tap.take();
        let s = std::mem::take(&mut sh.sun);
        if s != [0.0, 0.0] {
            cmds.push(Command::Sun { daz: s[0] * 0.6, del: -s[1] * 0.3 });
        }
        (sh.split.take(), sh.hold)
    };
    #[cfg(feature = "material-styles")]
    let mut style_cmds: Vec<StyleCommand> = Vec::new();
    #[cfg(feature = "material-styles")]
    let mut open_styles = false;
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        if let Some(s) = split {
            let _ = reg.set(k.split.id(), ParamValue::F32(s.clamp(0.0, 1.0)));
        }
        for u in ui {
            match u {
                Ui::Cmd(c) => cmds.push(c),
                Ui::Set { name, comp, v } => set_number(&mut reg, &name, comp, v),
                Ui::Toggle(name) => {
                    if let Some(ParamValue::Bool(b)) = reg.id(&name).and_then(|id| reg.value(id)) {
                        let _ = reg.set_by_name(&name, ParamValue::Bool(!b));
                    }
                }
                Ui::Choose(name, i) => {
                    let _ = reg.set_by_name(&name, ParamValue::Enum(i));
                }
                Ui::Colour(name, c) => {
                    let _ = reg.set_by_name(&name, ParamValue::Color(c.map(|x| x.clamp(0.0, 1.0))));
                }
                Ui::GroupReset(g) => {
                    for r in st.rows.iter().filter(|r| r.group == g.as_str()) {
                        match st.a.iter().find(|(id, _)| *id == r.id) {
                            Some((id, v)) => {
                                let _ = reg.set(*id, *v);
                            }
                            None => {
                                let _ = reg.reset(r.id);
                            }
                        }
                    }
                }
                Ui::Tier(t) => {
                    if let Err(e) = world.res_mut::<Tiers>().try_set_active(&t) {
                        web_sys::console::warn_1(&JsValue::from_str(&format!("material-view: tier {t}: {e:?}")));
                    }
                }
                Ui::SaveA => {
                    st.a = snapshot(&reg, &st.rows);
                    st.view_key.clear();
                }
                #[cfg(feature = "material-styles")]
                Ui::Style(c) => style_cmds.push(c),
                #[cfg(feature = "material-styles")]
                Ui::StyleOpen(i) => {
                    style_cmds.push(StyleCommand::Style(i));
                    open_styles = true;
                }
                #[cfg(feature = "material-catalogue")]
                Ui::CatOpen(id) => {
                    let _ = reg.set_json(kcat::OPEN, &serde_json::Value::String(id.clone()));
                    open_styles |= id != "none";
                }
                #[cfg(feature = "material-catalogue")]
                Ui::CatFilter(key) => panel::catalogue::apply_click(&mut st.cat.filter, &key),
                #[cfg(feature = "material-catalogue")]
                Ui::CatQuery(q) => st.cat.query = q,
            }
        }
    }
    // The art styles' commands go to the style row's own queue; a style
    // picked on another scene opens the styles scene too.
    #[cfg(feature = "material-styles")]
    {
        if open_styles && world.res::<Viewer>().scene != "styles" {
            cmds.push(Command::Scene("styles".to_string()));
        }
        if !style_cmds.is_empty()
            && let Some(mut sv) = world.try_res_mut::<StyleView>()
        {
            for c in style_cmds {
                sv.push(c);
            }
        }
    }
    {
        let mut v = world.res_mut::<Viewer>();
        for c in cmds {
            v.push(c);
        }
    }
    // 2. The tuning panel: rebuilt when the open material changes; A is
    // taken now (the material's preset was set when it opened last frame).
    #[allow(unused_mut)]
    let mut material = world.res::<Viewer>().material.clone();
    // A library material open (on the styles scene): the panel is its switches.
    #[cfg(feature = "material-catalogue")]
    let cat_open = world
        .try_res::<CatalogueView>()
        .and_then(|v| v.open.map(|i| (i, v.opened)))
        .filter(|_| world.res::<Viewer>().scene == "styles");
    #[cfg(feature = "material-catalogue")]
    if let Some((i, opened)) = cat_open {
        material = format!("catalogue:{i}:{opened}");
    }
    if material != st.material || st.a_pending {
        if material != st.material {
            restore_held(world, st);
            st.material = material.clone();
            st.rows.clear();
            #[cfg(feature = "material-catalogue")]
            let lib = world.try_res::<LibraryData>().map(|l| l.0.clone());
            #[cfg(feature = "material-catalogue")]
            let cat_panel = cat_open.zip(lib).map(|((i, _), lib)| {
                let reg = world.res::<ParamRegistry>();
                st.rows = panel::catalogue::panel_rows(&reg, &lib, i);
                let values: Vec<ParamValue> = st.rows.iter().map(|r| reg.value(r.id).unwrap_or(r.def.default)).collect();
                let t = world.res::<Tiers>();
                let tiers: Vec<(String, String)> = panel::TIER_CHIPS.iter().filter_map(|id| t.get(id).map(|p| (p.id.clone(), p.label.clone()))).collect();
                panel::catalogue::panel_html(&lib, i, &st.rows, &values, &tiers)
            });
            #[cfg(not(feature = "material-catalogue"))]
            let cat_panel: Option<String> = None;
            if let Some(html) = cat_panel {
                dom.set_html(&dom.rows, &html);
                st.tier_seen = usize::MAX;
            } else if let Some((fam, mat)) = data.material(&material) {
                let reg = world.res::<ParamRegistry>();
                st.rows = panel_rows(&reg, &data.library.groups, fam, Some(mat));
                let values: Vec<ParamValue> = st.rows.iter().map(|r| reg.value(r.id).unwrap_or(r.def.default)).collect();
                let prefixes = panel::label_prefixes(fam, &mat.panel);
                let tiers: Vec<(String, String)> = {
                    let t = world.res::<Tiers>();
                    panel::TIER_CHIPS.iter().filter_map(|id| t.get(id).map(|p| (p.id.clone(), p.label.clone()))).collect()
                };
                // The art styles reset to their style's defaults, not to A.
                #[cfg(feature = "material-styles")]
                let style_fam = fam.id == "styles";
                #[cfg(not(feature = "material-styles"))]
                let style_fam = false;
                #[allow(unused_mut)]
                let mut html = panel::panel_html(&data, fam, &st.rows, &values, &prefixes, &tiers, style_fam);
                #[cfg(feature = "material-styles")]
                if style_fam {
                    html.push_str(panel::styles::ABOUT_HTML);
                }
                dom.set_html(&dom.rows, &html);
                st.tier_seen = usize::MAX;
            } else {
                dom.set_html(&dom.rows, "<p class=\"none\">No material is open. Pick one in the browser.</p>");
            }
        }
        st.a = snapshot(&world.res::<ParamRegistry>(), &st.rows);
        st.a_pending = false;
        st.view_key.clear();
        st.reg_rev = world.res::<ParamRegistry>().revision();
    }
    // 3. Values follow the registry (a live edit moves the sliders).
    let rev = world.res::<ParamRegistry>().revision();
    if rev != st.reg_rev && !capturing {
        st.reg_rev = rev;
        let dragging = shared.borrow().dragging.clone();
        dom.write_values(&world.res::<ParamRegistry>(), &st.rows, dragging.as_deref());
    }
    // 4. The scene row, the browser, the lights and the HUD, when they change.
    {
        let v = world.res::<Viewer>();
        let key = format!("{}|{}|{}|{}|{}|{}|{}|{}", v.material, v.scene, v.light, v.spin, v.compare, v.paused, v.families_on.len(), st.material);
        if key != st.ui_key {
            st.ui_key = key;
            dom.set_html(&dom.scenes, &panel::scenes_html(&data, &v));
            dom.set_html(&dom.lights, &panel::lights_html(&data, &v));
            dom.set_html(&dom.fams, &panel::families_html(&data, &v));
            dom.set_html(&dom.rooms, &panel::rooms_html(&data, &v));
            #[allow(unused_mut)]
            let (mut mname, mut fname) = data.material(&v.material).map_or(("No material".to_string(), String::new()), |(f, m)| (m.name.clone(), f.name.clone()));
            #[cfg(feature = "material-catalogue")]
            if let Some(((i, _), lib)) = cat_open.zip(world.try_res::<LibraryData>().map(|l| l.0.clone()))
                && let Some(r) = lib.cat.rows.get(i)
            {
                mname = r.name.clone();
                fname = format!("Library ({})", r.recipe);
            }
            let scene = data.scene(&v.scene).map_or("", |s| s.name.as_str());
            let light = data.lights.presets.get(v.light).map_or("", |p| p.name.as_str());
            dom.set_html(&dom.hud, &panel::hud_html(&mname, &[&fname, scene, light], v.paused));
            dom.body_class("spin-on", v.spin);
            dom.body_class("compare", v.compare);
        }
    }
    #[cfg(feature = "material-catalogue")]
    catalogue_frame(world, &mut st.cat, &mut st.sty, dom);
    #[cfg(feature = "material-styles")]
    styles_frame(world, shared, &mut st.sty, dom);
    // 5. Quality: the tier chips and the frame time.
    {
        let t = world.res::<Tiers>();
        if t.active_index() != st.tier_seen {
            st.tier_seen = t.active_index();
            let active = t.active().id.clone();
            for id in panel::TIER_CHIPS {
                dom.class_by_id(&format!("mv-tier-{id}"), "on", *id == active);
            }
            let note = if panel::TIER_CHIPS.contains(&active.as_str()) { String::new() } else { format!("(now {})", t.active().label) };
            dom.text_by_id("mv-tier-now", &note);
            st.view_key.clear();
        }
    }
    if st.last_now > 0.0 {
        let dt = (now - st.last_now).clamp(0.0, 1000.0);
        st.frame_ms = st.frame_ms * 0.92 + dt * 0.08;
    }
    st.last_now = now;
    if now - st.ft_shown_at > 250.0 {
        st.ft_shown_at = now;
        let fps = if st.frame_ms > 0.0 { 1000.0 / st.frame_ms } else { 0.0 };
        dom.text_by_id("mv-ft", &format!("{:.1} ms ({fps:.0} fps)", st.frame_ms));
    }
    // 6. Compare.
    let split_now = world.res::<ParamRegistry>().f32(k.split);
    compare_frame(world, st, dom, split_now, hold);
}

/// The library tab: the filter chips, the tree and the list follow the
/// search, the filters and the open row; each is written only when its
/// input changes. Opening a row, or a new shape for it, also refreshes the
/// dock's strips.
#[cfg(feature = "material-catalogue")]
fn catalogue_frame(world: &mut World, cu: &mut CatUi, su: &mut StyleUi, dom: &Dom) {
    let Some(cd) = dom.cat.as_ref() else { return };
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.0.clone()) else { return };
    let (open, opened) = world.try_res::<CatalogueView>().map_or((None, 0), |v| (v.open, v.opened));
    let shape = {
        let reg = world.res::<ParamRegistry>();
        match reg.id(kcat::SHAPE).and_then(|id| reg.value(id)) {
            Some(ParamValue::Enum(k)) => u64::from(k),
            _ => 0,
        }
    };
    if opened != cu.opened || shape != cu.shape {
        cu.opened = opened;
        cu.shape = shape;
        // The dock's chips and shapes show the library material (or the
        // style's own again).
        su.rev = u64::MAX;
    }
    let key = format!("{}|{:?}|{open:?}", cu.query, cu.filter);
    if key == cu.key {
        return;
    }
    cu.key = key;
    let hits = lib.search(&cu.query, &cu.filter);
    let no_cat = CatFilter { category: None, subcategory: None, ..cu.filter.clone() };
    let tree_rows: Vec<usize> = lib.search(&cu.query, &no_cat).iter().map(|h| h.row).collect();
    dom.set_html(&cd.filters, &panel::catalogue::filters_html(&cu.filter));
    dom.set_html(&cd.tree, &panel::catalogue::tree_html(&lib, &tree_rows, &cu.filter));
    dom.set_html(&cd.list, &panel::catalogue::list_html(&lib, &hits, open));
}

/// The art styles' page parts. The header, chips and shapes follow the
/// style row's revision (an edit from the panel or over evani-live bumps
/// it); the bar only moves its highlight; the reference panel follows the
/// style, its toggle and the layout. Each part is written only when its
/// markup changes.
#[cfg(feature = "material-styles")]
fn styles_frame(world: &mut World, shared: &SharedRc, su: &mut StyleUi, dom: &Dom) {
    use panel::styles as ps;
    let Some(sd) = dom.sty.as_ref() else { return };
    let Some(styles) = world.try_res::<StylesData>().map(|s| s.0.clone()) else { return };
    let Some((rev, style)) = world.try_res::<StyleView>().map(|v| (v.revision, v.style)) else { return };
    let mut changed = false;
    if !su.started {
        su.started = true;
        dom.body_class("styles-on", true);
        dom.set_html(&sd.bar, &ps::bar_html(&styles, style));
        su.style = Some(style);
        su.saved_w = shared.borrow().refs_w;
        changed = true;
    }
    let on = world.res::<Viewer>().scene == "styles";
    if su.on_scene != Some(on) {
        su.on_scene = Some(on);
        dom.body_class("on-styles", on);
        changed = true;
    }
    if su.style != Some(style) {
        if let Some(old) = su.style {
            dom.class_by_id(&format!("mv-st-{old}"), "on", false);
        }
        dom.class_by_id(&format!("mv-st-{style}"), "on", true);
        su.style = Some(style);
        sd.centre(style);
    }
    sd.edges();
    if rev != su.rev {
        su.rev = rev;
        let v = world.res::<StyleView>();
        let look = world.try_res::<LookData>();
        if let Some(s) = styles.styles.get(v.style) {
            let fins: Vec<ps::MatFinish> = (0..s.materials.len()).map(|m| style_finish(&v, &styles, look.as_deref(), v.style, m)).collect();
            #[allow(unused_mut)]
            let (mut mats, mut shapes) = (ps::mats_html(s, &fins, v.selected), ps::shapes_html(&v.shape, v.show_all));
            #[cfg(feature = "material-catalogue")]
            if let (Some(i), Some(lib)) = (world.try_res::<CatalogueView>().and_then(|c| c.open), world.try_res::<LibraryData>().map(|l| l.0.clone())) {
                mats = panel::catalogue::dock_chip_html(&lib, i);
                let reg = world.res::<ParamRegistry>();
                let k = match reg.id(kcat::SHAPE).and_then(|id| reg.value(id)) {
                    Some(ParamValue::Enum(k)) => k as usize,
                    _ => 0,
                };
                shapes = panel::catalogue::shapes_html(k);
            }
            for (el, last, html) in [
                (&sd.head, &mut su.head, ps::head_html(s, &fins)),
                (&sd.mats, &mut su.mats, mats),
                (&sd.shapes, &mut su.shapes, shapes),
            ] {
                if *last != html {
                    dom.set_html(el, &html);
                    *last = html;
                    changed = true;
                }
            }
        }
    }
    // The reference panel: docked on the right on wide screens, a sheet on
    // phones; closed, its pictures are dropped.
    let (open, w, dragging) = {
        let s = shared.borrow();
        (s.refs_open, s.refs_w, s.refs_drag.is_some())
    };
    let (win_w, win_h) = win_size();
    let narrow = win_w < NARROW_PX;
    let layout = format!("{win_w}x{win_h}");
    if layout != su.layout {
        su.layout = layout;
        changed = true;
    }
    let key = format!("{open}|{narrow}|{style}");
    if key != su.refs_key {
        su.refs_key = key;
        let _ = sd.refs_body.class_list().remove_1("no-refs");
        match styles.styles.get(style).filter(|_| open) {
            Some(s) => dom.set_html(&sd.refs_body, &ps::refs_html(&styles, s)),
            None => dom.set_html(&sd.refs_body, ""),
        }
        dom.body_class("refs-open", open);
        dom.body_class("refs-dock", open && !narrow);
        dom.body_class("refs-sheet", open && narrow);
        changed = true;
    }
    let max_w = (win_w * ps::REFS_MAX_SHARE).min(win_w - ps::REFS_KEEP_VIEW).max(ps::REFS_MIN_W);
    let cw = if w > 0.0 { f64::from(w) } else { win_w * 0.3 }.clamp(ps::REFS_MIN_W, max_w).round();
    let px = if open && !narrow { format!("{cw}px") } else { "0px".to_string() };
    if px != su.refs_w {
        let _ = dom.body.style().set_property("--refs-w", &px);
        su.refs_w = px;
        changed = true;
    }
    if open && !narrow && !dragging && (cw as f32 - su.saved_w).abs() >= 1.0 {
        su.saved_w = cw as f32;
        shared.borrow_mut().refs_w = cw as f32;
        storage_set(ps::REFS_W_KEY, &format!("{cw}"));
    }
    // The dock's height moves the HUD, the drawer and the panel below it.
    if changed {
        let h = sd.dock.get_bounding_client_rect().height().round() as i32;
        if h != su.dock_h {
            su.dock_h = h;
            let _ = dom.body.style().set_property("--dock-h", &format!("{h}px"));
        }
    }
    // The scenes fit their subject into what the bars and panels leave open
    // (the art styles its shapes, the others their camera: scenes::fit).
    if let Some(open) = free_area(&dom.doc, &dom.canvas)
        && let Some(mut cur) = world.try_res_mut::<FreeArea>()
        && *cur != open
    {
        *cur = open;
    }
}

/// The page parts that can cover the styles scene (a closed one has no box),
/// and the studio page's panels (studio/page.rs: the view is the gap they leave).
#[cfg(feature = "material-styles")]
const COVERS: &[&str] = &["mv-styles", "mv-hud", "mv-drawer", "mv-panel", "mv-bar", "mv-scenes", "evani-convert-zone", "st-top", "st-note", "st-left", "st-right", "st-insp", "st-side", "st-vars", "st-tabs", "st-mode", "st-plist", "st-ppanel"];

/// The biggest part of the canvas the page's bars and panels leave open,
/// CSS px inside the canvas: each cover, biggest first, trims the open box
/// from the side that keeps the most of it.
#[cfg(feature = "material-styles")]
fn free_area(doc: &Document, canvas: &HtmlCanvasElement) -> Option<FreeArea> {
    let c = canvas.get_bounding_client_rect();
    let (cw, ch) = (c.width(), c.height());
    if cw < 1.0 || ch < 1.0 {
        return None;
    }
    let area = |b: &[f64; 4]| (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let mut covers: Vec<[f64; 4]> = COVERS
        .iter()
        .filter_map(|id| doc.get_element_by_id(id))
        // A panel hidden for a picture of the scene alone (proc-shots
        // --bare) covers nothing: the view refits to the whole canvas.
        .filter(|e| e.dyn_ref::<web_sys::HtmlElement>().is_none_or(|h| h.style().get_property_value("visibility").ok().as_deref() != Some("hidden")))
        .map(|e| {
            let r = e.get_bounding_client_rect();
            [r.left() - c.left(), r.top() - c.top(), r.right() - c.left(), r.bottom() - c.top()]
        })
        .filter(|b| area(b) > 0.0)
        .collect();
    covers.sort_by(|a, b| area(b).total_cmp(&area(a)));
    let mut f = [0.0, 0.0, cw, ch];
    for b in covers {
        let i = [b[0].max(f[0]), b[1].max(f[1]), b[2].min(f[2]), b[3].min(f[3])];
        if i[2] <= i[0] || i[3] <= i[1] {
            continue;
        }
        let cuts = [[i[2], f[1], f[2], f[3]], [f[0], i[3], f[2], f[3]], [f[0], f[1], i[0], f[3]], [f[0], f[1], f[2], i[1]]];
        f = cuts.into_iter().max_by(|a, b| area(a).total_cmp(&area(b))).unwrap_or(f);
    }
    Some(FreeArea { rect: f.map(|x| x.round() as f32), view: [cw.round() as f32, ch.round() as f32] })
}

/// The window's size, CSS px.
#[cfg(feature = "material-styles")]
fn win_size() -> (f64, f64) {
    let Some(win) = web_sys::window() else { return (1.0, 1.0) };
    let get = |v: Result<JsValue, JsValue>| v.ok().and_then(|x| x.as_f64()).unwrap_or(1.0).max(1.0);
    (get(win.inner_width()), get(win.inner_height()))
}

/// `localStorage` through plain JS calls (the page has no Storage binding);
/// a browser that refuses storage just forgets.
#[cfg(feature = "material-styles")]
fn storage_call(method: &str, args: &[&str]) -> Option<JsValue> {
    let win = web_sys::window()?;
    let ls = js_sys::Reflect::get(&win, &JsValue::from_str("localStorage")).ok().filter(|v| v.is_object())?;
    let f: js_sys::Function = js_sys::Reflect::get(&ls, &JsValue::from_str(method)).ok()?.dyn_into().ok()?;
    let a = js_sys::Array::new();
    for x in args {
        a.push(&JsValue::from_str(x));
    }
    f.apply(&ls, &a).ok()
}

#[cfg(feature = "material-styles")]
fn storage_get(key: &str) -> Option<String> {
    storage_call("getItem", &[key])?.as_string()
}

#[cfg(feature = "material-styles")]
fn storage_set(key: &str, value: &str) {
    let _ = storage_call("setItem", &[key, value]);
}

/// Set a numeric param, or one component of a vec3, from a slider.
fn set_number(reg: &mut ParamRegistry, name: &str, comp: Option<usize>, x: f32) {
    let Some(id) = reg.id(name) else { return };
    let Some(cur) = reg.value(id) else { return };
    let v = match (cur, comp) {
        (ParamValue::F32(_), _) => ParamValue::F32(x),
        (ParamValue::I32(_), _) => ParamValue::I32(x.round() as i32),
        (ParamValue::Vec3(mut c), Some(i)) if i < 3 => {
            c[i] = x;
            ParamValue::Vec3(c)
        }
        _ => return,
    };
    let _ = reg.set(id, v);
}

/// The current values of the panel's rows.
fn snapshot(reg: &ParamRegistry, rows: &[PanelRow]) -> Vec<(ParamId, ParamValue)> {
    rows.iter().filter_map(|r| reg.value(r.id).map(|v| (r.id, v))).collect()
}

/// Set saved values (A) over the non-quality rows; returns what was there (B).
fn set_a(world: &mut World, st: &State) -> Vec<(ParamId, ParamValue)> {
    let mut reg = world.res_mut::<ParamRegistry>();
    let tuning: Vec<PanelRow> = st.rows.iter().filter(|r| r.group != "quality").cloned().collect();
    let b = snapshot(&reg, &tuning);
    for (id, v) in &st.a {
        if tuning.iter().any(|r| r.id == *id) {
            let _ = reg.set(*id, *v);
        }
    }
    b
}

fn restore(world: &mut World, b: &[(ParamId, ParamValue)]) {
    let mut reg = world.res_mut::<ParamRegistry>();
    for (id, v) in b {
        let _ = reg.set(*id, *v);
    }
}

fn restore_held(world: &mut World, st: &mut State) {
    if let Cap::ShowingA(_, b) | Cap::Copying(b, _) = std::mem::replace(&mut st.cap, Cap::Off) {
        restore(world, &b);
    }
    if let Some(b) = st.held.take() {
        restore(world, &b);
    }
}

fn compare_frame(world: &mut World, st: &mut State, dom: &mut Dom, split: f32, hold: bool) {
    let on = world.res::<Viewer>().compare;
    if !on {
        if st.compare_shown {
            st.compare_shown = false;
            restore_held(world, st);
            dom.body_class("cap-ready", false);
            dom.body_class("cap-hold", false);
        }
        return;
    }
    if !st.compare_shown {
        st.compare_shown = true;
        st.view_key.clear();
        st.split_seen.clear();
        dom.body_class("cap-hold", st.fallback);
    }
    // The compare line follows the param (vertical in landscape).
    let portrait = dom.portrait();
    let pct = (split.clamp(0.0, 1.0) * 100.0).round();
    let key = format!("{portrait}{pct}");
    if key != st.split_seen {
        st.split_seen = key;
        dom.place_split(portrait, pct);
    }
    // Hold-to-flip when the canvas cannot be copied.
    if st.fallback {
        if hold && st.held.is_none() {
            st.held = Some(set_a(world, st));
        } else if !hold && let Some(b) = st.held.take() {
            restore(world, &b);
        }
        return;
    }
    let view = {
        let v = world.res::<Viewer>();
        let t = world.res::<Tiers>();
        format!("{:?}|{}|{}|{}|{}|{}|{}", v.orbit, v.light, v.sun_az, v.sun_el, v.material, t.active_index(), dom.canvas_size_key())
    };
    if view != st.view_key {
        // The view moved: the copy is stale until it holds still again.
        st.view_key = view;
        if let Cap::ShowingA(_, b) | Cap::Copying(b, _) = std::mem::replace(&mut st.cap, Cap::Settling(0)) {
            restore(world, &b);
        }
        dom.body_class("cap-ready", false);
        return;
    }
    st.cap = match std::mem::replace(&mut st.cap, Cap::Off) {
        Cap::Off => Cap::Settling(0),
        Cap::Settling(n) if n + 1 >= SETTLE_FRAMES => Cap::ShowingA(A_FRAMES, set_a(world, st)),
        Cap::Settling(n) => Cap::Settling(n + 1),
        Cap::ShowingA(n, b) if n > 1 => Cap::ShowingA(n - 1, b),
        // This frame is drawn with A too: copy it straight after the draw.
        Cap::ShowingA(_, b) => Cap::Copying(b, dom.queue_copy()),
        Cap::Copying(b, done) => match done.get() {
            // The copy has not run (no frame was drawn): wait.
            None => Cap::Copying(b, done),
            Some(ok) => {
                restore(world, &b);
                if ok {
                    st.copy_fails = 0;
                    dom.body_class("cap-ready", true);
                    Cap::Ready
                } else if st.copy_fails + 1 < COPY_TRIES {
                    st.copy_fails += 1;
                    Cap::Settling(0)
                } else {
                    web_sys::console::warn_1(&JsValue::from_str(
                        "material-view: the view could not be copied for the split; compare falls back to holding A",
                    ));
                    st.fallback = true;
                    dom.body_class("cap-hold", true);
                    Cap::Ready
                }
            }
        },
        Cap::Ready => Cap::Ready,
    };
}

/// Copy the view into compare's A canvas; false when it came back empty.
fn copy_view(canvas: &HtmlCanvasElement, shot: &HtmlCanvasElement, ctx: &CanvasRenderingContext2d) -> bool {
    let (w, h) = (canvas.width(), canvas.height());
    if w == 0 || h == 0 {
        return false;
    }
    if shot.width() != w || shot.height() != h {
        shot.set_width(w);
        shot.set_height(h);
    }
    ctx.clear_rect(0.0, 0.0, f64::from(w), f64::from(h));
    if ctx.draw_image_with_html_canvas_element(canvas, 0.0, 0.0).is_err() {
        return false;
    }
    // An opaque view has alpha everywhere; an unreadable one has none.
    [(0.5, 0.5), (0.25, 0.25), (0.75, 0.75), (0.25, 0.75), (0.75, 0.25)].iter().any(|(fx, fy)| {
        let (x, y) = ((f64::from(w) * fx) as i32, (f64::from(h) * fy) as i32);
        ctx.get_image_data(x, y, 1, 1).is_ok_and(|d| d.data().0.get(3).is_some_and(|a| *a > 0))
    })
}

fn by_id(doc: &Document, id: &str) -> Result<Element, String> {
    doc.get_element_by_id(id).ok_or_else(|| format!("index.html has no #{id}"))
}

/// The page's elements.
struct Dom {
    doc: Document,
    body: HtmlElement,
    canvas: HtmlCanvasElement,
    shot: HtmlCanvasElement,
    shot_ctx: CanvasRenderingContext2d,
    hud: Element,
    rows: Element,
    scenes: Element,
    lights: Element,
    fams: Element,
    rooms: Element,
    split: HtmlElement,
    #[cfg(feature = "material-styles")]
    sty: Option<StyleDom>,
    #[cfg(feature = "material-catalogue")]
    cat: Option<CatDom>,
}

/// The library tab's parts, added to the drawer only by builds with the
/// catalogue.
#[cfg(feature = "material-catalogue")]
struct CatDom {
    filters: Element,
    tree: Element,
    list: Element,
}

#[cfg(feature = "material-catalogue")]
impl CatDom {
    fn create(doc: &Document, body: &HtmlElement, shared: &SharedRc) -> Result<Self, String> {
        use panel::catalogue as pc;
        let css = doc.create_element("style").map_err(|_| "create style")?;
        css.set_text_content(Some(pc::CSS));
        let head: web_sys::Node = match doc.query_selector("head").ok().flatten() {
            Some(h) => h.into(),
            None => body.clone().into(),
        };
        head.append_child(&css).map_err(|_| "add the catalogue's CSS")?;
        let tabs = doc.query_selector("#mv-drawer .tabs").ok().flatten().ok_or("index.html has no drawer tabs")?;
        tabs.insert_adjacent_html("afterbegin", pc::TAB_HTML).map_err(|_| "add the Library tab")?;
        let scroll = doc.query_selector("#mv-drawer .scroll").ok().flatten().ok_or("index.html has no drawer body")?;
        scroll.insert_adjacent_html("afterbegin", pc::LIB_HTML).map_err(|_| "add the library")?;
        let _ = body.class_list().add_2("cat-on", "tab-lib");
        let q = by_id(doc, "mv-cat-q")?;
        let sh = shared.clone();
        let input = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
            if let Some(inp) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                sh.borrow_mut().ui.push(Ui::CatQuery(inp.value()));
            }
        });
        q.add_event_listener_with_callback("input", input.as_ref().unchecked_ref()).map_err(|_| "search listener")?;
        input.forget();
        Ok(CatDom { filters: by_id(doc, "mv-cat-filters")?, tree: by_id(doc, "mv-cat-tree")?, list: by_id(doc, "mv-cat-list")? })
    }
}

/// Copy `text` to the clipboard (where the browser allows it).
#[cfg(feature = "material-catalogue")]
fn copy_text(text: &str) -> bool {
    let Some(win) = web_sys::window() else { return false };
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &JsValue::from_str(k)).ok().filter(|v| !v.is_undefined());
    let Some(clip) = get(&win.into(), "navigator").and_then(|n| get(&n, "clipboard")) else { return false };
    let Some(f) = get(&clip, "writeText").and_then(|f| f.dyn_into::<js_sys::Function>().ok()) else { return false };
    f.call1(&clip, &JsValue::from_str(text)).is_ok()
}

/// The art styles' dock and reference panel, added to the page only by
/// builds with the family.
#[cfg(feature = "material-styles")]
struct StyleDom {
    dock: Element,
    bar: Element,
    head: Element,
    mats: Element,
    shapes: Element,
    refs_body: Element,
    /// The tuning panel's scrolling body.
    rows: Element,
    /// The scene bar, which scrolls sideways on a phone.
    scenes: Element,
}

#[cfg(feature = "material-styles")]
impl StyleDom {
    fn create(doc: &Document, body: &HtmlElement, shared: &SharedRc) -> Result<Self, String> {
        use panel::styles as ps;
        let make = |tag: &str, id: &str, html: &str| -> Result<Element, String> {
            let el = doc.create_element(tag).map_err(|_| format!("create {tag}"))?;
            if !id.is_empty() {
                el.set_id(id);
            }
            el.set_inner_html(html);
            Ok(el)
        };
        let css = make("style", "", "")?;
        css.set_text_content(Some(ps::CSS));
        let head: web_sys::Node = match doc.query_selector("head").ok().flatten() {
            Some(h) => h.into(),
            None => body.clone().into(),
        };
        head.append_child(&css).map_err(|_| "add the styles' CSS")?;
        let dock = make("div", "mv-styles", ps::DOCK_HTML)?;
        let refs = make("aside", "mv-refs", ps::REFS_HTML)?;
        let _ = refs.set_attribute("aria-label", "Reference pictures");
        body.append_child(&dock).map_err(|_| "add the style dock")?;
        body.append_child(&refs).map_err(|_| "add the reference panel")?;
        let sd = StyleDom {
            bar: by_id(doc, "mv-stylebar")?,
            head: by_id(doc, "mv-stylehead")?,
            mats: by_id(doc, "mv-smats")?,
            shapes: by_id(doc, "mv-shapes")?,
            refs_body: by_id(doc, "mv-refs-body")?,
            rows: by_id(doc, "mv-rows")?,
            scenes: by_id(doc, "mv-scenes")?,
            dock,
        };
        // The docked panel's width from last time.
        if let Some(w) = storage_get(ps::REFS_W_KEY).and_then(|w| w.parse::<f32>().ok()).filter(|w| w.is_finite() && *w > 0.0) {
            shared.borrow_mut().refs_w = w;
        }
        // The handle: the panel's left edge follows the pointer.
        listen_drag(&by_id(doc, "mv-refs-grip")?, shared, |s, id, _| s.refs_drag = Some(id), |s, id, at| {
            if s.refs_drag == Some(id) {
                s.refs_w = (win_size().0 as f32 - at[0]).max(1.0);
            }
        })?;
        // On a phone one tap anywhere on the sheet closes it.
        let sh = shared.clone();
        let tap = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
            if narrow() {
                sh.borrow_mut().refs_open = false;
            }
        });
        refs.add_event_listener_with_callback("click", tap.as_ref().unchecked_ref()).map_err(|_| "refs click listener")?;
        tap.forget();
        // A picture that can't load is hidden; when none can, the panel
        // says the pictures are only on the dev server (load errors don't
        // bubble, so this listens in the capture phase).
        let rb = sd.refs_body.clone();
        let err = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
            let Some(img) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
            if img.tag_name() != "IMG" {
                return;
            }
            let _ = img.class_list().add_1("bad");
            if rb.query_selector("img:not(.bad)").ok().flatten().is_none() {
                let _ = rb.class_list().add_1("no-refs");
            }
        });
        sd.refs_body
            .add_event_listener_with_callback_and_bool("error", err.as_ref().unchecked_ref(), true)
            .map_err(|_| "refs error listener")?;
        err.forget();
        Ok(sd)
    }

    /// Scroll the bar so style `i` is in the middle.
    fn centre(&self, i: usize) {
        let Some(btn) = self.bar.query_selector(&format!("#mv-st-{i}")).ok().flatten().and_then(|b| b.dyn_into::<HtmlElement>().ok()) else {
            return;
        };
        let Some(bar) = self.bar.dyn_ref::<HtmlElement>() else { return };
        let x = btn.offset_left() - bar.offset_left() - (bar.client_width() - btn.offset_width()) / 2;
        bar.set_scroll_left(f64::from(x.max(0)));
    }

    /// Mark each strip's row (styles, materials, shapes) `fl` / `fr` while
    /// items are hidden off its left / right edge: the page fades that
    /// edge, so a phone shows there is more to swipe to, and lights the
    /// style row's arrow.
    fn edges(&self) {
        for strip in [&self.bar, &self.mats, &self.shapes] {
            let (Some(s), Some(row)) = (strip.dyn_ref::<HtmlElement>(), strip.parent_element()) else { continue };
            let x = s.scroll_left();
            let max = f64::from(s.scroll_width() - s.client_width());
            let cl = row.class_list();
            let _ = cl.toggle_with_force("fl", x > 1.0);
            let _ = cl.toggle_with_force("fr", x < max - 1.0);
        }
        // The scene bar fades its own edges the same way.
        if let Some(s) = self.scenes.dyn_ref::<HtmlElement>() {
            let x = s.scroll_left();
            let max = f64::from(s.scroll_width() - s.client_width());
            let cl = s.class_list();
            let _ = cl.toggle_with_force("fl", x > 1.0);
            let _ = cl.toggle_with_force("fr", x < max - 1.0);
        }
        // The tuning panel: `more-up` / `more-down` while rows are hidden
        // above / below, so the page fades that edge and says there is more.
        if let (Some(r), Some(panel)) = (self.rows.dyn_ref::<HtmlElement>(), self.rows.parent_element()) {
            let y = r.scroll_top();
            let max = f64::from(r.scroll_height() - r.client_height());
            let cl = panel.class_list();
            let _ = cl.toggle_with_force("more-up", y > 1.0);
            let _ = cl.toggle_with_force("more-down", y < max - 1.0);
        }
    }
}

impl Dom {
    fn create(shared: &SharedRc) -> Result<Self, String> {
        let win = web_sys::window().ok_or("no window")?;
        let doc = win.document().ok_or("no document")?;
        let body = doc.body().ok_or("no body")?;
        let canvas: HtmlCanvasElement = by_id(&doc, "evani-canvas")?.dyn_into().map_err(|_| "#evani-canvas is not a canvas")?;
        let shot: HtmlCanvasElement = by_id(&doc, "mv-a")?.dyn_into().map_err(|_| "#mv-a is not a canvas")?;
        let shot_ctx: CanvasRenderingContext2d = shot
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|c| c.dyn_into().ok())
            .ok_or("#mv-a has no 2d context")?;
        let split: HtmlElement = by_id(&doc, "mv-split")?.dyn_into().map_err(|_| "#mv-split")?;
        let dom = Dom {
            hud: by_id(&doc, "mv-hud")?,
            rows: by_id(&doc, "mv-rows")?,
            scenes: by_id(&doc, "mv-scenes")?,
            lights: by_id(&doc, "mv-lights")?,
            fams: by_id(&doc, "mv-fams")?,
            rooms: by_id(&doc, "mv-rooms")?,
            doc: doc.clone(),
            body: body.clone(),
            canvas,
            shot,
            shot_ctx,
            split,
            #[cfg(feature = "material-styles")]
            sty: None,
            #[cfg(feature = "material-catalogue")]
            cat: None,
        };
        #[cfg(feature = "material-styles")]
        let dom = Dom { sty: Some(StyleDom::create(&doc, &body, shared)?), ..dom };
        #[cfg(feature = "material-catalogue")]
        let dom = Dom { cat: Some(CatDom::create(&doc, &body, shared)?), ..dom };
        // Wide, tall enough windows open with the drawer and the panel showing.
        let num = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        if !compact_size(num(win.inner_width()), num(win.inner_height())) {
            let _ = body.class_list().add_2("drawer-open", "panel-open");
        }
        listen_clicks(&body, shared)?;
        listen_panel(&dom.rows, shared)?;
        listen_canvas(&dom.canvas, shared)?;
        listen_drag(&by_id(&doc, "mv-sun")?, shared, |s, id, at| s.sun_drag = Some((id, at)), |s, id, at| {
            if let Some((d, last)) = s.sun_drag
                && d == id
            {
                s.sun[0] += at[0] - last[0];
                s.sun[1] += at[1] - last[1];
                s.sun_drag = Some((id, at));
            }
        })?;
        listen_drag(&dom.split, shared, |s, id, _| s.split_drag = Some(id), |s, id, at| {
            if s.split_drag == Some(id) {
                s.split = Some(split_at(at));
            }
        })?;
        let hold_btn = by_id(&doc, "mv-hold")?;
        listen_drag(&hold_btn, shared, |s, _, _| s.hold = true, |_, _, _| {})?;
        // Every drag ends on pointer up, window blur and a hidden tab.
        let end = |sh: SharedRc| {
            Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                let mut s = sh.borrow_mut();
                s.pointers.clear();
                s.press = None;
                s.sun_drag = None;
                s.split_drag = None;
                s.dragging = None;
                s.hold = false;
                #[cfg(feature = "material-styles")]
                {
                    s.refs_drag = None;
                }
            })
        };
        for (target, name) in [(win.as_ref() as &web_sys::EventTarget, "blur"), (doc.as_ref(), "visibilitychange")] {
            let f = end(shared.clone());
            target.add_event_listener_with_callback(name, f.as_ref().unchecked_ref()).map_err(|_| format!("{name} listener"))?;
            f.forget();
        }
        for name in ["pointerup", "pointercancel"] {
            let sh = shared.clone();
            let f = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
                let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
                let mut s = sh.borrow_mut();
                if s.sun_drag.is_some_and(|(id, _)| id == p.pointer_id()) {
                    s.sun_drag = None;
                }
                if s.split_drag == Some(p.pointer_id()) {
                    s.split_drag = None;
                }
                #[cfg(feature = "material-styles")]
                if s.refs_drag == Some(p.pointer_id()) {
                    s.refs_drag = None;
                }
                s.dragging = None;
                s.hold = false;
            });
            win.add_event_listener_with_callback(name, f.as_ref().unchecked_ref()).map_err(|_| format!("{name} listener"))?;
            f.forget();
        }
        Ok(dom)
    }

    fn set_html(&self, el: &Element, html: &str) {
        el.set_inner_html(html);
    }

    fn body_class(&self, class: &str, on: bool) {
        let _ = self.body.class_list().toggle_with_force(class, on);
    }

    fn class_by_id(&self, id: &str, class: &str, on: bool) {
        if let Some(el) = self.doc.get_element_by_id(id) {
            let _ = el.class_list().toggle_with_force(class, on);
        }
    }

    fn text_by_id(&self, id: &str, text: &str) {
        if let Some(el) = self.doc.get_element_by_id(id) {
            el.set_text_content(Some(text));
        }
    }

    fn view_height(&self) -> f32 {
        self.canvas.client_height() as f32
    }

    fn portrait(&self) -> bool {
        self.canvas.client_height() > self.canvas.client_width()
    }

    fn canvas_size_key(&self) -> String {
        format!("{}x{}", self.canvas.width(), self.canvas.height())
    }

    /// Clip the A copy at the split and move the line there.
    fn place_split(&self, portrait: bool, pct: f32) {
        let clip = if portrait { format!("inset(0 0 {}% 0)", 100.0 - pct) } else { format!("inset(0 {}% 0 0)", 100.0 - pct) };
        let _ = self.shot.style().set_property("clip-path", &clip);
        let s = self.split.style();
        if portrait {
            let _ = s.set_property("left", "0");
            let _ = s.set_property("top", &format!("{pct}%"));
        } else {
            let _ = s.set_property("top", "0");
            // Along the view, which is narrower while a panel is docked on
            // the right (`--refs-w`, set by the art styles' references).
            let _ = s.set_property("left", &format!("calc((100vw - var(--refs-w, 0px)) * {})", pct / 100.0));
        }
        self.body_class("portrait", portrait);
    }

    /// Copy the view (the last frame drawn) into the A canvas. False when
    /// the copy came back empty (a browser that cannot read a WebGPU canvas).
    /// Queue the view copy as a microtask: it runs once this frame's
    /// callback (the systems, then the draw) returns, before the browser
    /// presents and clears the WebGPU canvas. The result lands in the cell.
    fn queue_copy(&self) -> Rc<Cell<Option<bool>>> {
        let done = Rc::new(Cell::new(None));
        let (canvas, shot, ctx, out) = (self.canvas.clone(), self.shot.clone(), self.shot_ctx.clone(), done.clone());
        let f = Closure::<dyn FnMut(JsValue)>::once(move |_: JsValue| out.set(Some(copy_view(&canvas, &shot, &ctx))));
        let _ = js_sys::Promise::resolve(&JsValue::UNDEFINED).then(&f);
        // The promise calls it once, later: JS owns it from here (a one-shot
        // closure; at most COPY_TRIES per compare).
        f.forget();
        done
    }

    /// Write the registry's values into the rows (not the one being dragged).
    fn write_values(&self, reg: &ParamRegistry, rows: &[PanelRow], dragging: Option<&str>) {
        for (i, r) in rows.iter().enumerate() {
            let Some(v) = reg.value(r.id) else { continue };
            self.text_by_id(&format!("mvv-{i}"), &panel::fmt_value(&r.def, &v));
            if dragging == Some(r.def.name.as_str()) {
                continue;
            }
            let input = |id: String| self.doc.get_element_by_id(&id).and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
            match v {
                ParamValue::F32(x) => {
                    if let Some(el) = input(format!("mvp-{i}")) {
                        el.set_value_as_number(f64::from(x));
                    }
                }
                ParamValue::I32(x) => {
                    if let Some(el) = input(format!("mvp-{i}")) {
                        el.set_value_as_number(f64::from(x));
                    }
                }
                ParamValue::Vec3(c) => {
                    for (k, x) in c.iter().enumerate() {
                        if let Some(el) = input(format!("mvp-{i}-{k}")) {
                            el.set_value_as_number(f64::from(*x));
                        }
                    }
                }
                ParamValue::Color(c) => {
                    if let Some(el) = input(format!("mvp-{i}")) {
                        el.set_value(&panel::linear_to_hex(c));
                    }
                }
                ParamValue::Bool(b) => {
                    if let Some(el) = self.doc.get_element_by_id(&format!("mvp-{i}")) {
                        let _ = el.class_list().toggle_with_force("on", b);
                        el.set_text_content(Some(if b { "On" } else { "Off" }));
                    }
                }
                ParamValue::Enum(sel) => {
                    for k in 0..r.def.options.len() {
                        self.class_by_id(&format!("mvp-{i}-{k}"), "on", k as u32 == sel);
                    }
                }
            }
        }
    }
}

/// Where a pointer at `at` puts the compare line (0..1 across the view, or
/// down it in portrait).
fn split_at(at: [f32; 2]) -> f32 {
    let Some(canvas) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("evani-canvas")) else { return 0.5 };
    let w = canvas.client_width().max(1) as f32;
    let h = canvas.client_height().max(1) as f32;
    if h > w { at[1] / h } else { at[0] / w }
}

fn attr(el: &Element, name: &str) -> Option<String> {
    el.get_attribute(name)
}

/// One delegated click listener for every button on the page.
fn listen_clicks(body: &HtmlElement, shared: &SharedRc) -> Result<(), String> {
    let sh = shared.clone();
    let b = body.clone();
    let click = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
        let Some(el) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
        let Ok(Some(btn)) = el.closest("button") else { return };
        let mut s = sh.borrow_mut();
        #[cfg(feature = "material-styles")]
        if style_click(&btn, &mut s) {
            return;
        }
        let push = |s: &mut Shared, u: Ui| s.ui.push(u);
        #[cfg(feature = "material-catalogue")]
        {
            if let Some(id) = attr(&btn, "data-cat-open") {
                push(&mut s, Ui::CatOpen(id));
                // On a phone the drawer steps aside so the material shows.
                if narrow() {
                    let _ = b.class_list().remove_1("drawer-open");
                }
                return;
            }
            if attr(&btn, "data-cat-close").is_some() {
                push(&mut s, Ui::CatOpen("none".into()));
                return;
            }
            if let Some(id) = attr(&btn, "data-cat-copy") {
                let done = copy_text(&id);
                btn.set_text_content(Some(if done { "Copied" } else { "Select the id below" }));
                return;
            }
            if let Some(k) = attr(&btn, "data-cf") {
                push(&mut s, Ui::CatFilter(k));
                return;
            }
            // A style's own material chip closes the library material.
            if attr(&btn, "data-smat").is_some() {
                push(&mut s, Ui::CatOpen("none".into()));
            }
        }
        if let Some(id) = attr(&btn, "data-scene") {
            push(&mut s, Ui::Cmd(Command::Scene(id)));
        } else if let Some(id) = attr(&btn, "data-mat") {
            push(&mut s, Ui::Cmd(Command::Material(id)));
            // On a phone the sheet steps aside so the material shows.
            if narrow() {
                let _ = b.class_list().remove_1("drawer-open");
            }
        } else if let Some(i) = attr(&btn, "data-light").and_then(|i| i.parse().ok()) {
            push(&mut s, Ui::Cmd(Command::Light(i)));
        } else if let Some(name) = attr(&btn, "data-bool") {
            push(&mut s, Ui::Toggle(name));
        } else if let (Some(name), Some(i)) = (attr(&btn, "data-enum"), attr(&btn, "data-i").and_then(|i| i.parse().ok())) {
            push(&mut s, Ui::Choose(name, i));
        } else if let Some(g) = attr(&btn, "data-reset") {
            push(&mut s, Ui::GroupReset(g));
        } else if let Some(t) = attr(&btn, "data-tier") {
            push(&mut s, Ui::Tier(t));
        } else if let Some(t) = attr(&btn, "data-tab") {
            let _ = b.class_list().toggle_with_force("tab-rooms", t == "rooms");
            let _ = b.class_list().toggle_with_force("tab-lib", t == "lib");
        } else if let Some(c) = attr(&btn, "data-cmd") {
            match c.as_str() {
                "spin" => push(&mut s, Ui::Cmd(Command::Turntable(None))),
                "compare" => push(&mut s, Ui::Cmd(Command::Compare(None))),
                "reset" => push(&mut s, Ui::Cmd(Command::Reset)),
                "save-a" => push(&mut s, Ui::SaveA),
                "browse" | "tune" => {
                    let (mine, other) = if c == "browse" { ("drawer-open", "panel-open") } else { ("panel-open", "drawer-open") };
                    let list = b.class_list();
                    let open = !list.contains(mine);
                    let _ = list.toggle_with_force(mine, open);
                    if open && narrow() {
                        let _ = list.remove_1(other);
                    }
                }
                _ => {}
            }
        }
    });
    body.add_event_listener_with_callback("click", click.as_ref().unchecked_ref()).map_err(|_| "click listener")?;
    click.forget();
    Ok(())
}

/// The art styles' buttons and the reference panel's toggle; false when
/// the button is not one of them.
#[cfg(feature = "material-styles")]
fn style_click(btn: &Element, s: &mut Shared) -> bool {
    let num = |name: &str| attr(btn, name).and_then(|v| v.parse::<usize>().ok());
    let u = if let Some(i) = num("data-style") {
        Ui::StyleOpen(i)
    } else if let Some(i) = num("data-smat") {
        Ui::Style(StyleCommand::Select(i))
    } else if let Some(id) = attr(btn, "data-shape") {
        Ui::Style(StyleCommand::Shape(id))
    } else if let Some(v) = attr(btn, "data-showall") {
        Ui::Style(StyleCommand::ShowAll(Some(v == "1")))
    } else if let Some(v) = attr(btn, "data-sreset") {
        Ui::Style(if v == "all" { StyleCommand::ResetAll } else { StyleCommand::Reset })
    } else {
        match attr(btn, "data-cmd").as_deref() {
            Some("refs") => s.refs_open = !s.refs_open,
            Some("refs-close") => s.refs_open = false,
            Some(c @ ("st-prev" | "st-next")) => {
                // Page the style strip by most of its width.
                let bar = btn.owner_document().and_then(|d| d.get_element_by_id("mv-stylebar"));
                if let Some(bar) = bar.and_then(|b| b.dyn_into::<HtmlElement>().ok()) {
                    let page = f64::from(bar.client_width()) * 0.8;
                    bar.set_scroll_left(bar.scroll_left() + if c == "st-prev" { -page } else { page });
                }
            }
            _ => return false,
        }
        return true;
    };
    s.ui.push(u);
    true
}

fn narrow() -> bool {
    web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|w| w.as_f64()).is_some_and(|w| w < NARROW_PX)
}

/// Slider and colour input changes, and which slider is held.
fn listen_panel(rows: &Element, shared: &SharedRc) -> Result<(), String> {
    let on = |name: &str, f: Closure<dyn FnMut(Event)>| -> Result<(), String> {
        rows.add_event_listener_with_callback(name, f.as_ref().unchecked_ref()).map_err(|_| format!("{name} listener"))?;
        f.forget();
        Ok(())
    };
    let sh = shared.clone();
    on(
        "input",
        Closure::new(move |e: Event| {
            let Some(inp) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) else { return };
            let Some(name) = inp.get_attribute("data-p") else { return };
            let mut s = sh.borrow_mut();
            if inp.type_() == "color" {
                if let Some(c) = panel::hex_to_linear(&inp.value()) {
                    s.ui.push(Ui::Colour(name, c));
                }
            } else {
                let x = inp.value_as_number();
                if x.is_finite() {
                    let comp = inp.get_attribute("data-c").and_then(|c| c.parse().ok());
                    s.ui.push(Ui::Set { name, comp, v: x as f32 });
                }
            }
        }),
    )?;
    let sh = shared.clone();
    on(
        "pointerdown",
        Closure::new(move |e: Event| {
            let name = e.target().and_then(|t| t.dyn_into::<Element>().ok()).and_then(|el| el.get_attribute("data-p"));
            sh.borrow_mut().dragging = name;
        }),
    )?;
    let sh = shared.clone();
    on(
        "change",
        Closure::new(move |_e: Event| {
            sh.borrow_mut().dragging = None;
        }),
    )?;
    Ok(())
}

/// A small drag handle (the sun dial, the split line, the hold button):
/// `down` on press with the pointer captured, `moved` while it moves.
fn listen_drag(el: &Element, shared: &SharedRc, down: fn(&mut Shared, i32, [f32; 2]), moved: fn(&mut Shared, i32, [f32; 2])) -> Result<(), String> {
    let sh = shared.clone();
    let target = el.clone();
    let press = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
        let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
        e.prevent_default();
        let _ = target.set_pointer_capture(p.pointer_id());
        down(&mut sh.borrow_mut(), p.pointer_id(), [p.client_x() as f32, p.client_y() as f32]);
    });
    el.add_event_listener_with_callback("pointerdown", press.as_ref().unchecked_ref()).map_err(|_| "drag listener")?;
    press.forget();
    let sh = shared.clone();
    let mv = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
        let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
        moved(&mut sh.borrow_mut(), p.pointer_id(), [p.client_x() as f32, p.client_y() as f32]);
    });
    el.add_event_listener_with_callback("pointermove", mv.as_ref().unchecked_ref()).map_err(|_| "drag listener")?;
    mv.forget();
    Ok(())
}

/// The view's gestures: one pointer orbits, the right button or two
/// pointers pan, a pinch or the wheel zooms, a double tap resets.
fn listen_canvas(canvas: &HtmlCanvasElement, shared: &SharedRc) -> Result<(), String> {
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
            if p.button() != 0 && p.button() != 2 {
                return;
            }
            let _ = c.set_pointer_capture(p.pointer_id());
            let at = [p.client_x() as f32, p.client_y() as f32];
            let mut s = sh.borrow_mut();
            s.pointers.retain(|q| q.id != p.pointer_id());
            s.pointers.push(Ptr { id: p.pointer_id(), at, button: p.button() });
            s.press = if s.pointers.len() == 1 && p.button() == 0 { Some((e.time_stamp(), at, false)) } else { None };
        }),
    )?;
    let sh = shared.clone();
    on(
        "pointermove",
        Closure::new(move |e: Event| {
            let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
            let mut s = sh.borrow_mut();
            let Some(i) = s.pointers.iter().position(|q| q.id == p.pointer_id()) else { return };
            let at = [p.client_x() as f32, p.client_y() as f32];
            let before = s.pointers.clone();
            s.pointers[i].at = at;
            let d = [at[0] - before[i].at[0], at[1] - before[i].at[1]];
            if let Some((t, start, moved)) = s.press
                && !moved
                && ((at[0] - start[0]).abs() > TAP_SLOP_PX || (at[1] - start[1]).abs() > TAP_SLOP_PX)
            {
                s.press = Some((t, start, true));
            }
            match before.len() {
                1 if before[0].button == 2 => {
                    s.pan[0] += d[0];
                    s.pan[1] += d[1];
                }
                1 => {
                    s.orbit[0] += d[0];
                    s.orbit[1] += d[1];
                }
                n if n >= 2 && i < 2 => {
                    // The first two fingers: the midpoint pans, the spread zooms.
                    let (a0, b0) = (before[0].at, before[1].at);
                    let (a1, b1) = (s.pointers[0].at, s.pointers[1].at);
                    s.pan[0] += ((a1[0] + b1[0]) - (a0[0] + b0[0])) * 0.5;
                    s.pan[1] += ((a1[1] + b1[1]) - (a0[1] + b0[1])) * 0.5;
                    let dist = |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt().max(1.0);
                    s.pinch += (dist(a1, b1) / dist(a0, b0)).ln();
                }
                _ => {}
            }
        }),
    )?;
    for name in ["pointerup", "pointercancel", "lostpointercapture"] {
        let sh = shared.clone();
        let is_up = name == "pointerup";
        on(
            name,
            Closure::new(move |e: Event| {
                let Some(p) = e.dyn_ref::<PointerEvent>() else { return };
                let mut s = sh.borrow_mut();
                let was = s.pointers.len();
                s.pointers.retain(|q| q.id != p.pointer_id());
                if s.pointers.len() == was {
                    return;
                }
                // A short press that did not move is a tap; two close taps reset.
                let now = e.time_stamp();
                if is_up
                    && was == 1
                    && let Some((t, at, false)) = s.press
                    && now - t < 300.0
                {
                    match s.last_tap {
                        Some((lt, la)) if now - lt < DOUBLE_TAP_MS && (at[0] - la[0]).abs() < 30.0 && (at[1] - la[1]).abs() < 30.0 => {
                            s.reset = true;
                            s.last_tap = None;
                        }
                        _ => {
                            s.last_tap = Some((now, at));
                            s.tap = Some(at);
                        }
                    }
                }
                s.press = None;
            }),
        )?;
    }
    let sh = shared.clone();
    on(
        "wheel",
        Closure::new(move |e: Event| {
            let Some(w) = e.dyn_ref::<WheelEvent>() else { return };
            e.prevent_default();
            let scale = match w.delta_mode() {
                1 => 33.0,
                2 => 400.0,
                _ => 1.0,
            };
            sh.borrow_mut().wheel += (w.delta_y() * scale) as f32;
        }),
    )?;
    on(
        "contextmenu",
        Closure::new(move |e: Event| {
            e.prevent_default();
        }),
    )?;
    Ok(())
}
