//! WP4, wasm only: the viewport's own overlay, laid over the part of the
//! view the page's panels leave open ([`FreeArea`]), so the page (WP6) can
//! put its bars and panels round it without covering it:
//!
//! - a camera rail (Orbit, Pan, Zoom, Reset): drag on a tool to turn, slide
//!   or zoom the view (the view itself still orbits on a drag, pans on a
//!   right or two-finger drag and zooms on the wheel or a pinch); a click
//!   on Zoom zooms in, Reset goes back to the asset's framing;
//! - the combination card ("Voxel + Brick + Horse": style, material,
//!   asset), and in comparison one small label under each record;
//! - one plain state line: being built, loading, making the style's shape
//!   (the plain asset shown meanwhile), the style not built yet;
//! - the view modes (Render view, Material only, Wireframe), Compare and
//!   Fullscreen.
//!
//! Sized by its own box (CSS container queries on `#studio-vp`): labels
//! give way to icons on a narrow viewport, long names end in "…" with the
//! full text in a tooltip.

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;
use evani_kit_material_view::studio::{Selection, Studio, StudioCommand};
use evani_kit_material_view::{Command, Viewer};
use evani_render::RenderWorld;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{Document, Element, Event, HtmlElement, PointerEvent};

use super::scene::{DISPLAY, SCENE, SceneParams, StudioScene};
use super::scene_build::Display;
use crate::scenes::styles::FreeArea;

const CSS: &str = r#"
#studio-vp{position:fixed;left:0;top:0;width:100vw;height:100vh;pointer-events:none;z-index:4;container-type:size;container-name:svp;font:13px/1.3 system-ui,-apple-system,"Segoe UI",sans-serif;color:#f4efe8}
#studio-vp[hidden],#studio-vp [hidden]{display:none!important}
#studio-vp button{pointer-events:auto;font:inherit;color:inherit;background:rgba(28,26,30,.82);border:1px solid rgba(255,255,255,.14);border-radius:10px;cursor:pointer;display:flex;align-items:center;gap:6px;white-space:nowrap;touch-action:none}
#studio-vp button:hover{background:rgba(52,48,56,.9)}
#studio-vp button[aria-pressed="true"]{border-color:#e9b949;box-shadow:0 0 0 1px #e9b949 inset}
#studio-vp button:disabled{cursor:default}#studio-vp button:disabled>*{opacity:.4}
#studio-vp .ic{font-size:16px;line-height:1;width:18px;text-align:center}
.svp-rail{position:absolute;left:10px;top:12px;display:flex;flex-direction:column;gap:2px;padding:4px;background:rgba(28,26,30,.86);border:1px solid rgba(255,255,255,.12);border-radius:12px;pointer-events:auto}
#studio-vp .svp-rail button{flex-direction:column;gap:3px;padding:8px 6px;min-width:52px;font-size:11.5px;background:transparent;border-color:transparent}
#studio-vp .svp-rail button:hover{background:rgba(255,255,255,.08)}
.svp-rail [data-svp="orbit"] .ic{color:#e9b949}
.svp-card{position:absolute;right:10px;top:10px;max-width:min(340px,56cqw);display:flex;align-items:center;gap:10px;background:rgba(28,26,30,.84);border:1px solid rgba(255,255,255,.12);border-radius:12px;padding:8px 14px 8px 8px;pointer-events:auto}
.svp-card .ball{flex:none;width:52px;height:52px;border-radius:50%;overflow:hidden;background:radial-gradient(circle at 45% 40%,color-mix(in srgb,var(--sw,#888) 70%,#555) 0,color-mix(in srgb,var(--sw,#888) 35%,#111) 100%)}
.svp-card .ball .bc{display:block;width:100%;height:100%;border-radius:50%}
.svp-card .bp{position:relative;display:block;width:100%;height:100%;isolation:isolate}
.svp-card .bp img{display:block;width:100%;height:100%;object-fit:contain;filter:grayscale(1) brightness(1.35) contrast(1.9);pointer-events:none}
.svp-card .bp::after{content:"";position:absolute;inset:4%;border-radius:50%;pointer-events:none;background:radial-gradient(circle at 34% 28%,rgba(255,255,255,.7) 0,rgba(255,255,255,0) 26%),radial-gradient(circle at 50% 50%,rgba(0,0,0,0) 55%,rgba(0,0,0,.38) 100%)}
.svp-card .bp i{position:absolute;inset:0;background:var(--sw);mix-blend-mode:multiply;-webkit-mask-size:contain;mask-size:contain;-webkit-mask-repeat:no-repeat;mask-repeat:no-repeat;-webkit-mask-position:center;mask-position:center}
.svp-card .tx{min-width:0;display:flex;flex-direction:column}
.svp-card b{display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;font-size:14px;line-height:1.25;overflow:hidden;margin-bottom:2px}
.svp-card span{display:block;opacity:.75;font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.svp-state{position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);max-width:min(420px,80cqw);text-align:center;background:rgba(28,26,30,.86);border:1px solid rgba(233,185,73,.55);border-radius:12px;padding:10px 16px;font-size:14px;pointer-events:auto}
.svp-state.side{top:auto;bottom:62px;transform:translateX(-50%)}
.svp-state[hidden]{display:none}
.svp-state .spin{display:inline-block;width:10px;height:10px;margin-right:8px;border:2px solid #e9b949;border-right-color:transparent;border-radius:50%;animation:svp-spin 1s linear infinite;vertical-align:-1px}
@keyframes svp-spin{to{transform:rotate(360deg)}}
.svp-bottom{position:absolute;left:10px;right:10px;bottom:10px;display:flex;align-items:center;justify-content:center;padding-right:46px;pointer-events:none}
.svp-bar{display:flex;gap:6px;min-width:0;max-width:100%}
.svp-bar button{padding:7px 11px;min-width:0;min-height:36px}
.svp-bar [data-svp="mode-material"] .ic{color:#e9b949}
.svp-bottom>[data-svp="full"]{position:absolute;right:0;bottom:0;padding:7px 8px;min-height:36px}
:root:not([data-evani-preparing="0"]) .svp-bottom,:root[data-evani-stand-in] .svp-bottom{bottom:52px}
.svp-bar button .tx{overflow:hidden;text-overflow:ellipsis}
.svp-tags{position:absolute;inset:0}
.svp-tags span{position:absolute;transform:translateX(-50%);max-width:30cqw;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;background:rgba(28,26,30,.78);border-radius:8px;padding:3px 8px;font-size:12px}
.svp-tags span.on{box-shadow:0 0 0 1px #e9b949 inset}
@container svp (max-width:470px){.svp-bar .tx{display:none}.svp-bar button{padding:8px}.svp-card{max-width:58cqw}.svp-card .ball{width:40px;height:40px}}
@container svp (max-width:430px){.svp-rail button span.tx{display:none}#studio-vp .svp-rail button{min-width:40px;padding:8px 6px}.svp-card span,.svp-card .ball{display:none}.svp-card{padding:6px 10px}}
@container svp (max-height:360px){.svp-rail{flex-direction:row}}
"#;

const HTML: &str = r#"
<div class="svp-tags"></div>
<div class="svp-rail" role="toolbar" aria-label="Camera">
  <button data-svp="orbit" title="Orbit: drag on this button (or on the view) to turn round the asset"><span class="ic">⟳</span><span class="tx">Orbit</span></button>
  <button data-svp="pan" title="Pan: drag on this button (or right-drag the view) to slide the view"><span class="ic">✥</span><span class="tx">Pan</span></button>
  <button data-svp="zoom" title="Zoom: drag up or down on this button, click to zoom in, or use the wheel"><span class="ic">⌕</span><span class="tx">Zoom</span></button>
  <button data-svp="reset" title="Reset: back to the asset's own framing"><span class="ic">⌂</span><span class="tx">Reset</span></button>
</div>
<div class="svp-card" title=""><i class="ball"></i><div class="tx"><b></b></div></div>
<div class="svp-state" role="status" aria-live="polite" hidden></div>
<div class="svp-bottom"><div class="svp-bar" role="toolbar" aria-label="View">
  <button data-svp="compare" title="Compare: the record before this one and this one, side by side"><span class="ic">▦</span></button>
  <button data-svp="mode-render" title="Render view: the asset in its style"><span class="ic">◐</span><span class="tx">Render view</span></button>
  <button data-svp="mode-material" title="Material only: the asset's own shape in its materials, without the style"><span class="ic">●</span><span class="tx">Material only</span></button>
  <button data-svp="mode-wireframe" title="Wireframe: the drawn shape's edges"><span class="ic">⬡</span><span class="tx">Wireframe</span></button>
  <button data-svp="add" title="Add this record to the comparison (up to 4)" hidden><span class="ic">＋</span><span class="tx">Add</span></button>
</div><button data-svp="full" title="Fullscreen" aria-label="Fullscreen"><span class="ic">⛶</span></button></div>
"#;

/// What the overlay's listeners collected since the last frame.
#[derive(Debug, Default)]
struct Input {
    orbit: [f32; 2],
    pan: [f32; 2],
    zoom: f32,
    clicks: Vec<String>,
    /// The tool held and where the pointer last was, and whether it moved.
    held: Option<(String, [f32; 2], bool)>,
}

type Shared = Rc<RefCell<Input>>;

struct Dom {
    root: HtmlElement,
    card: Element,
    state: Element,
    tags: Element,
    shown: String,
}

fn q(root: &Element, sel: &str) -> Result<Element, String> {
    root.query_selector(sel).ok().flatten().ok_or_else(|| format!("studio viewport: no {sel}"))
}

fn make(doc: &Document, input: &Shared) -> Result<Dom, String> {
    let body = doc.body().ok_or("studio viewport: no body")?;
    let style = doc.create_element("style").map_err(|_| "style")?;
    style.set_text_content(Some(CSS));
    body.append_child(&style).map_err(|_| "append style")?;
    let root: HtmlElement = doc.create_element("div").map_err(|_| "div")?.dyn_into().map_err(|_| "div")?;
    root.set_id("studio-vp");
    root.set_hidden(true);
    root.set_inner_html(HTML);
    body.append_child(&root).map_err(|_| "append")?;
    // Presses, drags and clicks on the tools.
    let down = {
        let input = input.clone();
        Closure::<dyn FnMut(Event)>::new(move |e: Event| {
            let Some(btn) = e.target().and_then(|t| t.dyn_into::<Element>().ok()).and_then(|el| el.closest("[data-svp]").ok().flatten()) else { return };
            let Some(id) = btn.get_attribute("data-svp") else { return };
            let Ok(pe) = e.dyn_into::<PointerEvent>() else { return };
            if matches!(id.as_str(), "orbit" | "pan" | "zoom") {
                let _ = btn.set_pointer_capture(pe.pointer_id());
                pe.prevent_default();
            }
            input.borrow_mut().held = Some((id, [pe.client_x() as f32, pe.client_y() as f32], false));
        })
    };
    let moved = {
        let input = input.clone();
        Closure::<dyn FnMut(Event)>::new(move |e: Event| {
            let Ok(pe) = e.dyn_into::<PointerEvent>() else { return };
            let mut i = input.borrow_mut();
            let Some((id, at, _)) = i.held.clone() else { return };
            let now = [pe.client_x() as f32, pe.client_y() as f32];
            let d = [now[0] - at[0], now[1] - at[1]];
            if d == [0.0, 0.0] {
                return;
            }
            match id.as_str() {
                "orbit" => i.orbit = [i.orbit[0] + d[0], i.orbit[1] + d[1]],
                "pan" => i.pan = [i.pan[0] + d[0], i.pan[1] + d[1]],
                "zoom" => i.zoom += d[1],
                _ => return,
            }
            i.held = Some((id, now, true));
        })
    };
    let up = {
        let input = input.clone();
        Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
            let mut i = input.borrow_mut();
            if let Some((id, _, dragged)) = i.held.take()
                && !dragged
            {
                i.clicks.push(id);
            }
        })
    };
    for (ev, f) in [("pointerdown", &down), ("pointermove", &moved), ("pointerup", &up)] {
        root.add_event_listener_with_callback(ev, f.as_ref().unchecked_ref()).map_err(|_| "listener")?;
    }
    let cancel = {
        let input = input.clone();
        Closure::<dyn FnMut(Event)>::new(move |_e: Event| input.borrow_mut().held = None)
    };
    root.add_event_listener_with_callback("pointercancel", cancel.as_ref().unchecked_ref()).map_err(|_| "listener")?;
    down.forget();
    moved.forget();
    up.forget();
    cancel.forget();
    Ok(Dom { card: q(&root, ".svp-card")?, state: q(&root, ".svp-state")?, tags: q(&root, ".svp-tags")?, root, shown: String::new() })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn install(ctx: &mut PluginContext) {
    let input: Shared = Rc::new(RefCell::new(Input::default()));
    let mut dom: Option<Dom> = None;
    let mut failed = false;
    // The record shown before the current one (Compare's first record).
    let mut last: (Option<Selection>, Option<Selection>) = (None, None);
    ctx.system(SystemDesc::new("material-view.studio.viewport", Stage::Frame, move |world: &mut World| {
        if dom.is_none() && !failed {
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
            match make(&doc, &input) {
                Ok(d) => dom = Some(d),
                Err(e) => {
                    failed = true;
                    world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: {e}"));
                }
            }
        }
        let Some(d) = dom.as_mut() else { return };
        frame(world, d, &input, &mut last);
    }));
}

fn frame(world: &mut World, d: &mut Dom, input: &Shared, last: &mut (Option<Selection>, Option<Selection>)) {
    let showing = world.try_res::<Viewer>().is_some_and(|v| v.scene == SCENE);
    d.root.set_hidden(!showing);
    if !showing {
        input.borrow_mut().held = None;
        return;
    }
    // The open part of the view, as the page measured it.
    if let Some(a) = world.try_res::<FreeArea>().map(|a| *a).filter(|a| a.view[0] > 0.0 && a.rect[2] - a.rect[0] > 60.0) {
        let s = d.root.style();
        let _ = s.set_property("left", &format!("{}px", a.rect[0]));
        let _ = s.set_property("top", &format!("{}px", a.rect[1]));
        let _ = s.set_property("width", &format!("{}px", a.rect[2] - a.rect[0]));
        let _ = s.set_property("height", &format!("{}px", a.rect[3] - a.rect[1]));
    }
    let sel = world.res::<Studio>().state.selection.clone();
    if last.1.as_ref() != Some(&sel) {
        last.0 = last.1.take();
        last.1 = Some(sel.clone());
    }
    // 1. The tools.
    let (orbit, pan, zoom, clicks) = {
        let mut i = input.borrow_mut();
        (std::mem::take(&mut i.orbit), std::mem::take(&mut i.pan), std::mem::take(&mut i.zoom), std::mem::take(&mut i.clicks))
    };
    let view_h = world.res::<RenderWorld>().viewport_css[1].max(1.0);
    if let Some(mut v) = world.try_res_mut::<Viewer>() {
        if orbit != [0.0, 0.0] {
            v.push(Command::Orbit { daz: -orbit[0] / view_h * 180.0, del: orbit[1] / view_h * 120.0 });
        }
        if pan != [0.0, 0.0] {
            v.push(Command::Pan { dx: pan[0] / view_h, dy: pan[1] / view_h });
        }
        if zoom != 0.0 {
            v.push(Command::Zoom((zoom * 0.006).exp()));
        }
        for c in &clicks {
            match c.as_str() {
                "zoom" => v.push(Command::Zoom(0.8)),
                "reset" => v.push(Command::Reset),
                _ => {}
            }
        }
    }
    let compare = world.res::<Studio>().state.compare.clone();
    for c in &clicks {
        let mode = match c.as_str() {
            "mode-render" => Some(Display::Render),
            "mode-material" => Some(Display::Material),
            "mode-wireframe" => Some(Display::Wireframe),
            _ => None,
        };
        if let Some(m) = mode {
            let mut reg = world.res_mut::<ParamRegistry>();
            if let Some(id) = reg.id(DISPLAY) {
                let _ = reg.set(id, ParamValue::Enum(m.index()));
            }
        }
        let cmd = match c.as_str() {
            "compare" if compare.len() >= 2 => Some(StudioCommand::Compare(Vec::new())),
            "compare" => last.0.clone().map(|before| StudioCommand::Compare(vec![before, sel.clone()])),
            "add" if compare.len() >= 2 && compare.len() < super::scene::MAX_VIEWS && !compare.contains(&sel) => {
                let mut list = compare.clone();
                list.push(sel.clone());
                Some(StudioCommand::Compare(list))
            }
            "full" => {
                fullscreen();
                None
            }
            _ => None,
        };
        if let Some(cmd) = cmd {
            world.res_mut::<Studio>().push(cmd);
        }
    }
    // 2. What it shows (the DOM is only touched when this changes).
    let display = world.try_res::<SceneParams>().map(|p| Display::from_index(world.res::<ParamRegistry>().enum_index(p.display))).unwrap_or_default();
    let st = world.res::<StudioScene>();
    let cam = world.res::<RenderWorld>().camera;
    let vp = world.res::<RenderWorld>().viewport_css;
    let comparing = st.views.len() > 1;
    let mut sig = format!("{display:?}|{}|{}|{}", compare.len(), last.0.is_some(), comparing);
    let cur = st.current(&sel);
    if let Some(v) = cur {
        sig.push_str(&format!("|{}|{:?}", v.title(), v.status()));
    }
    // Comparison labels under each record (moved every frame: the camera turns).
    let mut tags = String::new();
    if comparing {
        let origin = d.root.get_bounding_client_rect();
        for v in &st.views {
            let foot = [(v.bounds[0][0] + v.bounds[1][0]) * 0.5, 0.0, (v.bounds[0][2] + v.bounds[1][2]) * 0.5];
            if let Some(p) = crate::scenes::fit::project(&cam, vp, foot) {
                let on = if v.selection == sel { " on" } else { "" };
                let line = match v.status() {
                    Some(s) => format!("{} · {s}", v.title()),
                    None => v.title(),
                };
                tags.push_str(&format!(
                    r#"<span class="{on}" style="left:{:.0}px;top:{:.0}px" title="{}">{}</span>"#,
                    p[0] as f64 - origin.left(),
                    (p[1] as f64 - origin.top() + 8.0).min(origin.height() - 70.0),
                    esc(&line),
                    esc(&line)
                ));
            }
        }
    }
    sig.push_str(&tags);
    if sig == d.shown {
        return;
    }
    d.shown = sig;
    if let Some(v) = cur {
        let (sw, layers) = world.try_res::<evani_kit_material_view::studio::StudioData>().map_or_else(|| ("#888".to_string(), None), |d| (super::page_view::swatch(&d.0, &v.selection.material), super::page_view::ball_layers(&d.0, &v.selection.material)));
        let card = format!(
            r#"<i class="ball{}" style="--sw:{sw}">{}</i><div class="tx"><b>{}</b><span>Style: {}</span><span>Material: {}</span><span>Asset: {}</span></div>"#,
            if layers.is_some() { "" } else { " plain" },
            layers.unwrap_or_default(),
            esc(&v.title()),
            esc(&v.names[1]),
            esc(&v.names[2]),
            esc(&v.names[0])
        );
        d.card.set_inner_html(&card);
        let _ = d.card.set_attribute("title", &format!("{} — style {}, material {}, asset {}", v.title(), v.names[1], v.names[2], v.names[0]));
        match (v.status(), comparing) {
            (Some(s), false) => {
                let busy = matches!(v.state, super::scene::ViewState::Loading | super::scene::ViewState::Generating);
                let spin = if busy { r#"<span class="spin"></span>"# } else { "" };
                d.state.set_inner_html(&format!("{spin}{}", esc(&s)));
                let _ = d.state.set_attribute("title", v.reason().unwrap_or(&s));
                // A plain asset is on screen: keep the line clear of it.
                let side = v.model.is_some();
                let _ = d.state.class_list().toggle_with_force("side", side);
                let _ = d.state.remove_attribute("hidden");
            }
            _ => {
                let _ = d.state.set_attribute("hidden", "");
            }
        }
    }
    d.tags.set_inner_html(&tags);
    for (id, on) in [("mode-render", display == Display::Render), ("mode-material", display == Display::Material), ("mode-wireframe", display == Display::Wireframe), ("compare", compare.len() >= 2)] {
        if let Ok(b) = q(&d.root, &format!("[data-svp=\"{id}\"]")) {
            let _ = b.set_attribute("aria-pressed", if on { "true" } else { "false" });
        }
    }
    if let Ok(b) = q(&d.root, "[data-svp=\"compare\"]") {
        let off = compare.len() < 2 && last.0.is_none();
        let _ = if off { b.set_attribute("disabled", "") } else { b.remove_attribute("disabled") };
        let _ = b.set_attribute("title", if off { "Compare: pick another style, material or asset first, then compare it with this one" } else if compare.len() >= 2 { "Leave the comparison" } else { "Compare: the record before this one and this one, side by side" });
    }
    if let Ok(b) = q(&d.root, "[data-svp=\"add\"]") {
        let show = compare.len() >= 2 && compare.len() < super::scene::MAX_VIEWS && !compare.contains(&sel);
        let _ = if show { b.remove_attribute("hidden") } else { b.set_attribute("hidden", "") };
    }
}

/// Fullscreen on, or off again.
fn fullscreen() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if doc.fullscreen_element().is_some() {
        doc.exit_fullscreen();
    } else if let Some(el) = doc.document_element() {
        let _ = el.request_fullscreen();
    }
}
