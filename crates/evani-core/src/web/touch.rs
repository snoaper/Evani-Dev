//! Page side of the touch layer (feature `touch`): builds the controls a
//! kit declared as plain elements over the canvas, turns Pointer Events
//! into calls on [`TouchControls`] and draws its state back.
//!
//! Listeners only queue [`TouchEv`]s; the runner applies them at the start
//! of the next frame, like every other input, so the sim sees one snapshot
//! per frame. Never requests fullscreen or pointer lock. (The pointer lock is its own feature, `pointer-lock`, and never starts from a touch.)

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;

use super::runner::listen;
use crate::ecs::World;
use crate::input::{Input, PointerKind, RawEvent};
use crate::touch::{Anchor, Control, Place, StickMode, TouchControls}; #[cfg(feature = "touch-look")] mod look;

#[derive(Debug, Clone, Copy)]
enum TouchEv {
    Press { index: u16, pointer: i32, at: [f32; 2], base: [f32; 2], radius: f32 },
    Move { pointer: i32, at: [f32; 2] },
    Release { pointer: i32 },
    ReleaseAll,
}

type TouchQueue = Rc<RefCell<Vec<TouchEv>>>;

/// Shared page rules: no scrolling, selection, long-press menu or tap
/// highlight on the controls; sizes in vmin with CSS-pixel floors.
const CSS: &str = "html,body{overscroll-behavior:none}\
#evani-touch{position:fixed;inset:0;z-index:850;pointer-events:none;display:none;\
-webkit-user-select:none;user-select:none;-webkit-touch-callout:none;-webkit-tap-highlight-color:transparent}\
#evani-touch.on{display:block}\
#evani-touch .et-c{pointer-events:auto;touch-action:none}\
#evani-touch .et-b,#evani-touch .et-r{position:absolute;box-sizing:border-box;border-radius:50%;\
border:2px solid rgba(255,255,255,.7);background:rgba(16,16,28,.35)}\
#evani-touch .et-b{display:flex;align-items:center;justify-content:center;color:#fff;\
font:700 clamp(13px,3.2vmin,20px)/1 system-ui,sans-serif;text-shadow:0 1px 2px #000}\
#evani-touch .et-b.down{background:rgba(255,255,255,.62);border-color:#fff;color:#111;text-shadow:none;\ntransform:scale(.92);box-shadow:0 0 0 4px rgba(255,255,255,.45)}\
#evani-touch .et-r{width:clamp(96px,30vmin,176px);height:clamp(96px,30vmin,176px);opacity:.6}\
#evani-touch .et-r.down{opacity:1}\
#evani-touch .et-r::after{content:attr(data-label);position:absolute;bottom:100%;left:0;right:0;\
text-align:center;color:#fff;font:700 13px/1.6 system-ui,sans-serif;text-shadow:0 0 3px #000,0 1px 2px #000}\
#evani-touch .et-k{position:absolute;left:30%;top:30%;width:40%;height:40%;border-radius:50%;\
background:rgba(255,255,255,.65);pointer-events:none}\
#evani-touch .et-z{position:absolute}";

/// `left/right` + `top/bottom` for a place, inside the safe area.
fn place_css(p: Place) -> String {
    let (h, v) = match p.anchor {
        Anchor::TopLeft => ("left", "top"),
        Anchor::TopRight => ("right", "top"),
        Anchor::BottomLeft => ("left", "bottom"),
        Anchor::BottomRight => ("right", "bottom"),
    };
    format!(
        "{h}:calc(max(8px,env(safe-area-inset-{h})) + {}vmin);{v}:calc(max(8px,env(safe-area-inset-{v})) + {}vmin);",
        css(p.x),
        css(p.y)
    )
}

/// A CSS number: `{}` text up to 3 decimals, without shipping float
/// formatting (crate::num::short).
fn css(x: f32) -> crate::num::Short {
    crate::num::short(x, 3)
}

struct Part {
    /// The button, or a stick's ring.
    el: web_sys::HtmlElement,
    class: &'static str,
    style: String,
    knob: Option<web_sys::HtmlElement>,
    dynamic: bool,
    held: bool,
    knob_drawn: Option<([f32; 2], [f32; 2])>,
}

/// The installed layer (one per page).
pub(crate) struct TouchDom {
    root: web_sys::Element,
    queue: TouchQueue,
    parts: Vec<Part>,
    drawn_rev: u64,
    drawn_active: bool,
}

fn html(doc: &web_sys::Document, tag: &str, class: &str) -> Option<web_sys::HtmlElement> {
    let el = doc.create_element(tag).ok()?;
    let _ = el.set_attribute("class", class);
    el.dyn_into::<web_sys::HtmlElement>().ok()
}

fn xy(e: &web_sys::PointerEvent) -> [f32; 2] {
    [e.client_x() as f32, e.client_y() as f32]
}

/// Press/move/release listeners on one touch target (a button, a fixed
/// stick's ring, or a dynamic stick's zone). Each finger is captured, so
/// sliding off still ends with a release; cancel and lost capture release
/// too, so nothing sticks. Events stop here: the game's own pointer
/// handling never sees a touch on a control.
fn wire(target: &web_sys::HtmlElement, index: u16, ring: Option<(web_sys::HtmlElement, bool)>, q: &TouchQueue, errors: &mut Vec<String>) {
    let t: &web_sys::EventTarget = target.as_ref();
    {
        let (q, el) = (q.clone(), target.clone());
        let r = listen(t, "pointerdown", false, move |e: web_sys::PointerEvent| {
            e.prevent_default();
            e.stop_propagation();
            let pointer = e.pointer_id();
            let _ = el.set_pointer_capture(pointer);
            let at = xy(&e);
            let (base, radius) = match &ring {
                Some((r, true)) => (at, r.offset_width() as f32 / 2.0),
                Some((r, false)) => {
                    let half = r.offset_width() as f32 / 2.0;
                    ([r.offset_left() as f32 + half, r.offset_top() as f32 + half], half)
                }
                None => ([0.0; 2], 0.0),
            };
            q.borrow_mut().push(TouchEv::Press { index, pointer, at, base, radius });
        });
        errors.extend(r.err());
    }
    {
        let q = q.clone();
        let r = listen(t, "pointermove", false, move |e: web_sys::PointerEvent| {
            e.stop_propagation();
            q.borrow_mut().push(TouchEv::Move { pointer: e.pointer_id(), at: xy(&e) });
        });
        errors.extend(r.err());
    }
    for name in ["pointerup", "pointercancel", "lostpointercapture"] {
        let q = q.clone();
        let r = listen(t, name, false, move |e: web_sys::PointerEvent| {
            e.stop_propagation();
            q.borrow_mut().push(TouchEv::Release { pointer: e.pointer_id() });
        });
        errors.extend(r.err());
    }
}

/// Build the layer if the kit declared one. Problems go to `errors`.
pub(crate) fn install(world: &World, errors: &mut Vec<String>) -> Option<TouchDom> {
    let layout = world.try_res::<TouchControls>()?.layout().clone();
    let window = web_sys::window()?;
    let doc = window.document()?;
    let body = doc.body()?;
    // Notches: the page must let content under the safe-area insets.
    if let Ok(Some(meta)) = doc.query_selector("meta[name=viewport]")
        && let Some(content) = meta.get_attribute("content")
        && !content.contains("viewport-fit")
    {
        let _ = meta.set_attribute("content", &format!("{content}, viewport-fit=cover"));
    }
    let style = doc.create_element("style").ok()?;
    style.set_text_content(Some(CSS));
    let _ = body.append_child(&style);
    let root = doc.create_element("div").ok()?;
    root.set_id("evani-touch");
    let q: TouchQueue = Rc::new(RefCell::new(Vec::with_capacity(32)));
    {
        let r = listen(root.as_ref(), "contextmenu", false, |e: web_sys::Event| e.prevent_default());
        errors.extend(r.err());
    }
    let mut parts = Vec::with_capacity(layout.controls.len());
    let mut zones = Vec::new();
    for (i, c) in layout.controls.iter().enumerate() {
        let index = i as u16;
        match c {
            Control::Button(b) => {
                let el = html(&doc, "div", "et-c et-b")?;
                let _ = el.set_attribute("data-id", &b.id);
                let _ = el.set_attribute("role", "button");
                let _ = el.set_attribute("aria-label", &b.label);
                el.set_text_content(Some(&b.label));
                let css = b.size.css();
                let style = format!("{}width:{css};height:{css}", place_css(b.place));
                let _ = el.set_attribute("style", &style);
                wire(&el, index, None, &q, errors);
                let _ = root.append_child(&el);
                parts.push(Part { el, class: "et-c et-b", style, knob: None, dynamic: false, held: false, knob_drawn: None });
            }
            Control::Stick(s) => {
                let dynamic = s.mode == StickMode::Dynamic;
                let class = if dynamic { "et-r" } else { "et-c et-r" };
                let ring = html(&doc, "div", class)?;
                let _ = ring.set_attribute("data-id", &s.id);
                let _ = ring.set_attribute("data-label", &s.label);
                let knob = html(&doc, "div", "et-k")?;
                let _ = ring.append_child(&knob);
                let style = place_css(s.place);
                let _ = ring.set_attribute("style", &style);
                if dynamic {
                    let z = html(&doc, "div", "et-c et-z")?;
                    let _ = z.set_attribute("data-id", &format!("{}-zone", s.id));
                    let zn = s.zone;
                    let _ = z.set_attribute(
                        "style",
                        &format!(
                            "left:{}%;top:{}%;width:{}%;height:{}%",
                            css(zn.x0 * 100.0),
                            css(zn.y0 * 100.0),
                            css((zn.x1 - zn.x0) * 100.0),
                            css((zn.y1 - zn.y0) * 100.0)
                        ),
                    );
                    wire(&z, index, Some((ring.clone(), true)), &q, errors);
                    zones.push(z);
                } else {
                    wire(&ring, index, Some((ring.clone(), false)), &q, errors);
                }
                parts.push(Part { el: ring, class, style, knob: Some(knob), dynamic, held: false, knob_drawn: None });
            } #[cfg(feature = "touch-look")] Control::LookPad(l) => look::install(&doc, l, index, &q, errors, &mut zones, &mut parts)?,
        }
    }
    // Zones go under the buttons so a button inside a zone still wins.
    for z in zones.iter().rev() {
        let _ = root.insert_before(z, root.first_child().as_ref());
    }
    for p in &parts {
        if p.knob.is_some() {
            let _ = root.append_child(&p.el);
        }
    }
    let _ = body.append_child(&root);
    // A hidden tab lets go of everything.
    {
        let (q, d) = (q.clone(), doc.clone());
        let r = listen(doc.as_ref(), "visibilitychange", true, move |_: web_sys::Event| {
            if d.hidden() {
                q.borrow_mut().push(TouchEv::ReleaseAll);
            }
        });
        errors.extend(r.err());
    }
    // Start state: a coarse pointer (phone, tablet) shows the controls.
    let coarse = window
        .match_media("(pointer: coarse)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches());
    {
        let mut t = world.res_mut::<TouchControls>();
        let mut input = world.res_mut::<Input>();
        t.set_coarse(&mut input, coarse);
    }
    let mut dom = TouchDom { root, queue: q, parts, drawn_rev: 0, drawn_active: false };
    dom.sync(world);
    Some(dom)
}

impl TouchDom {
    /// The touch-only rule from the frame's raw events: a touch going down
    /// shows the layer; a key (Esc included) hides it and lets go; blur
    /// lets go. Called before `Input` handles the same events.
    pub(crate) fn observe(&self, world: &World, raw: &[RawEvent]) {
        let mut t = world.res_mut::<TouchControls>();
        let mut input = world.res_mut::<Input>();
        for ev in raw {
            match ev {
                RawEvent::Pointer { kind: PointerKind::Touch, down: true, .. } => t.on_touch(&mut input),
                RawEvent::Key { down: true, .. } => t.on_key_or_pad(&mut input),
                RawEvent::Blur => t.release_all(&mut input),
                _ => {}
            }
        }
    }

    /// A gamepad in use hides the layer (polled pads only).
    pub(crate) fn observe_pads(&self, world: &World) {
        let mut input = world.res_mut::<Input>();
        if TouchControls::pad_in_use(&input) {
            world.res_mut::<TouchControls>().on_key_or_pad(&mut input);
        }
    }

    /// Apply the queued finger events.
    pub(crate) fn drain(&self, world: &World) {
        let mut q = self.queue.borrow_mut();
        if q.is_empty() {
            return;
        }
        let mut t = world.res_mut::<TouchControls>();
        let mut input = world.res_mut::<Input>();
        for ev in q.drain(..) {
            match ev {
                TouchEv::Press { index, pointer, at, base, radius } => {
                    t.press(&mut input, usize::from(index), pointer, at, base, radius);
                }
                TouchEv::Move { pointer, at } => t.drag(&mut input, pointer, at),
                TouchEv::Release { pointer } => t.release(&mut input, pointer),
                TouchEv::ReleaseAll => t.release_all(&mut input),
            }
        }
    }

    /// Draw: shown or not, held buttons, stick bases and knobs.
    pub(crate) fn sync(&mut self, world: &World) {
        let t = world.res::<TouchControls>();
        let active = t.active();
        if active != self.drawn_active {
            self.drawn_active = active;
            let _ = self.root.set_attribute("class", if active { "on" } else { "" });
        }
        if t.revision() == self.drawn_rev {
            return;
        }
        self.drawn_rev = t.revision();
        for (i, p) in self.parts.iter_mut().enumerate() {
            let held = t.is_held(i);
            if held != p.held {
                p.held = held;
                let class = if held { format!("{} down", p.class) } else { p.class.to_string() };
                let _ = p.el.set_attribute("class", &class);
            }
            let Some(knob) = &p.knob else { continue };
            let draw = t.stick_draw(i);
            if draw == p.knob_drawn {
                continue;
            }
            p.knob_drawn = draw;
            match draw {
                Some((base, k)) => {
                    let r = p.el.offset_width() as f32 / 2.0;
                    let _ = knob.set_attribute(
                        "style",
                        &format!("transform:translate({}px,{}px)", css(k[0] * r), css(k[1] * r)),
                    );
                    if p.dynamic {
                        let _ = p.el.set_attribute(
                            "style",
                            &format!(
                                "left:{}px;top:{}px;right:auto;bottom:auto",
                                css(base[0] - r),
                                css(base[1] - r)
                            ),
                        );
                    }
                }
                None => {
                    let _ = knob.remove_attribute("style");
                    let _ = p.el.set_attribute("style", &p.style);
                }
            }
        }
    }
}
