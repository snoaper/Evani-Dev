//! requestAnimationFrame runner and DOM listeners.
//!
//! Event listeners never touch the app while a frame runs: input becomes
//! [`RawEvent`]s pushed into a shared queue that the next frame drains
//! (the queue keeps its capacity, so no per-frame allocation). Visibility
//! and reduced-motion changes are applied directly when the app is free,
//! otherwise at the next frame.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use super::{AudioUnlocker, overlay_element, overlay_style};
use crate::app::App;
use crate::input::{Input, KeyCode, MAX_PADS, PAD_AXES, PAD_BUTTONS, PointerKind, RawEvent};
use crate::overlay::{ErrorLog, Overlay, Platform};

type RawQueue = Rc<RefCell<Vec<RawEvent>>>;
type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

struct Runner {
    app: App,
    raw: RawQueue,
    pads_seen: Rc<Cell<bool>>,
    pending_hidden: Rc<Cell<Option<bool>>>,
    pending_reduced_motion: Rc<Cell<Option<bool>>>,
    overlay_el: Option<web_sys::Element>,
    overlay_rev: u64,
    overlay_shown: bool,
    next_download_poll: f64,
    started: bool,
    #[cfg(feature = "touch")]
    touch: Option<super::touch::TouchDom>, #[cfg(feature = "pointer-lock")] lock: Option<super::pointer_lock::LockDom>,
}

impl Runner {
    fn frame(&mut self, now: f64) {
        if let Some(h) = self.pending_hidden.take() {
            self.app.set_hidden(h);
        }
        if let Some(r) = self.pending_reduced_motion.take() {
            self.app.world.res_mut::<Platform>().reduced_motion = r;
        } #[cfg(feature = "pointer-lock")] if let Some(l) = &self.lock { l.sync(&self.app.world); }
        #[cfg(feature = "touch")]
        if let Some(t) = &self.touch {
            t.observe(&self.app.world, &self.raw.borrow());
        }
        {
            let mut input = self.app.world.res_mut::<Input>();
            let mut q = self.raw.borrow_mut();
            for ev in q.iter() {
                input.handle(ev);
            }
            q.clear();
            if self.pads_seen.get() {
                poll_pads(&mut input);
            }
        }
        #[cfg(feature = "touch")]
        if let Some(t) = &self.touch {
            if self.pads_seen.get() {
                t.observe_pads(&self.app.world);
            }
            t.drain(&self.app.world);
        }
        // Live code swapping (dev builds only): a patch waiting since the
        // last frame is handed over here, before any system runs, and the
        // frame step itself goes through the jump table.
        #[cfg(feature = "hotpatch")]
        {
            crate::hotpatch::run_before_frame();
            crate::hotpatch::frame(&mut self.app, now);
        }
        #[cfg(not(feature = "hotpatch"))]
        self.app.frame(now);
        if !self.started {
            self.started = true;
            super::set_engine_status(true, None);
        }
        if now >= self.next_download_poll {
            self.next_download_poll = now + 1000.0;
            if let Some(b) = super::downloaded_bytes() {
                self.app.world.res_mut::<Platform>().download_bytes = Some(b);
            }
        }
        self.sync_overlay();
        #[cfg(feature = "touch")]
        if let Some(t) = &mut self.touch {
            t.sync(&self.app.world);
        }
    }

    fn sync_overlay(&mut self) {
        let Some(el) = &self.overlay_el else { return };
        let o = self.app.world.res::<Overlay>();
        if o.enabled != self.overlay_shown {
            self.overlay_shown = o.enabled;
            let _ = el.set_attribute("style", &overlay_style(o.enabled, false));
        }
        if o.enabled && o.revision() != self.overlay_rev {
            self.overlay_rev = o.revision();
            el.set_text_content(Some(o.text()));
        }
    }
}

/// Fixed-size scratch for one pad (no allocation per poll).
fn poll_pads(input: &mut Input) {
    let Some(window) = web_sys::window() else { return };
    let Ok(pads) = window.navigator().get_gamepads() else {
        return;
    };
    let mut buttons = [0f32; PAD_BUTTONS];
    let mut axes = [0f32; PAD_AXES];
    for i in 0..MAX_PADS {
        let pad = pads.get(i as u32).dyn_into::<web_sys::Gamepad>().ok();
        match pad {
            Some(p) if p.connected() => {
                let bs = p.buttons();
                for (b, slot) in buttons.iter_mut().enumerate() {
                    *slot = bs
                        .get(b as u32)
                        .dyn_into::<web_sys::GamepadButton>()
                        .map_or(0.0, |x| x.value() as f32);
                }
                let ax = p.axes();
                for (a, slot) in axes.iter_mut().enumerate() {
                    *slot = ax.get(a as u32).as_f64().unwrap_or(0.0) as f32;
                }
                input.set_pad(i, true, &buttons, &axes);
            }
            _ => input.set_pad(i, false, &[], &[]),
        }
    }
}

fn is_form_target(e: &web_sys::Event) -> bool {
    e.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "SELECT" | "TEXTAREA" | "BUTTON"))
}

pub(super) fn listen<E: wasm_bindgen::convert::FromWasmAbi + 'static>(
    target: &web_sys::EventTarget,
    name: &str,
    passive: bool,
    f: impl FnMut(E) + 'static,
) -> Result<(), String> {
    let cb = Closure::<dyn FnMut(E)>::new(f);
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(passive);
    target
        .add_event_listener_with_callback_and_add_event_listener_options(
            name,
            cb.as_ref().unchecked_ref(),
            &opts,
        )
        .map_err(super::js_err)?;
    // Listeners live as long as the page.
    cb.forget();
    Ok(())
}

fn pointer_kind(s: &str) -> PointerKind {
    match s {
        "touch" => PointerKind::Touch,
        "pen" => PointerKind::Pen,
        _ => PointerKind::Mouse,
    }
}

/// Start the game in the browser: panic hook, overlay, input listeners,
/// visibility/blur/reduced-motion handling and the requestAnimationFrame
/// loop. Never returns an error: problems go to the overlay.
pub fn run(mut app: App) {
    super::init();
    super::set_engine_status(false, None);
    let Some(window) = web_sys::window() else {
        super::console_log(true, "Evani.Dev: no window");
        return;
    };
    let document = window.document();

    // Platform facts.
    {
        let mut p = app.world.res_mut::<Platform>();
        p.device_pixel_ratio = window.device_pixel_ratio() as f32;
        p.webgpu = Some(super::webgpu_exposed());
        p.hidden = document.as_ref().is_some_and(web_sys::Document::hidden);
    }
    let mql = window
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten();
    if let Some(m) = &mql {
        app.world.res_mut::<Platform>().reduced_motion = m.matches();
    }
    if app.world.res::<Platform>().hidden {
        app.set_hidden(true);
    }

    let unlocker = app.world.try_res::<AudioUnlocker>().map(|u| u.clone());
    let raw: RawQueue = Rc::new(RefCell::new(Vec::with_capacity(64)));
    let pads_seen = Rc::new(Cell::new(false));
    let pending_hidden = Rc::new(Cell::new(None));
    let pending_reduced_motion = Rc::new(Cell::new(None));

    let mut errors: Vec<String> = Vec::new();
    // On-screen touch controls, when the kit declared some.
    #[cfg(feature = "touch")]
    let touch = super::touch::install(&app.world, &mut errors); #[cfg(feature = "pointer-lock")] let lock = super::pointer_lock::install(&app.world, &mut errors); // pointer lock, when the kit declared one
    let overlay_el = overlay_element();
    let overlay_shown = true;
    let runner = Rc::new(RefCell::new(Runner {
        app,
        raw: raw.clone(),
        pads_seen: pads_seen.clone(),
        pending_hidden: pending_hidden.clone(),
        pending_reduced_motion: pending_reduced_motion.clone(),
        overlay_el,
        overlay_rev: u64::MAX,
        overlay_shown,
        next_download_poll: 0.0,
        started: false,
        #[cfg(feature = "touch")]
        touch, #[cfg(feature = "pointer-lock")] lock,
    }));

    let win: &web_sys::EventTarget = window.as_ref();
    let unlock = move || {
        if let Some(u) = &unlocker {
            u.unlock();
        }
    };

    // Keyboard.
    {
        let (q, unlock) = (raw.clone(), unlock.clone());
        let r = listen(win, "keydown", false, move |e: web_sys::KeyboardEvent| {
            unlock();
            if let Some(code) = KeyCode::new(&e.code()) {
                if !is_form_target(&e)
                    && matches!(
                        code.as_str(),
                        "Space" | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Tab"
                    )
                {
                    e.prevent_default();
                }
                if !e.repeat() {
                    q.borrow_mut().push(RawEvent::Key { code, down: true });
                }
            }
        });
        errors.extend(r.err());
        let q = raw.clone();
        let r = listen(win, "keyup", false, move |e: web_sys::KeyboardEvent| {
            if let Some(code) = KeyCode::new(&e.code()) {
                q.borrow_mut().push(RawEvent::Key { code, down: false });
            }
        });
        errors.extend(r.err());
    }
    // Mouse buttons, movement, wheel.
    for (name, down) in [("mousedown", true), ("mouseup", false)] {
        let q = raw.clone();
        let r = listen(win, name, true, move |e: web_sys::MouseEvent| {
            q.borrow_mut().push(RawEvent::MouseButton {
                button: e.button().clamp(0, 7) as u8,
                down,
            });
        });
        errors.extend(r.err());
    }
    {
        let q = raw.clone();
        let r = listen(win, "mousemove", true, move |e: web_sys::MouseEvent| {
            q.borrow_mut().push(RawEvent::MouseMove {
                x: e.client_x() as f32,
                y: e.client_y() as f32,
                dx: e.movement_x() as f32,
                dy: e.movement_y() as f32,
            });
        });
        errors.extend(r.err());
        let q = raw.clone();
        let r = listen(win, "wheel", true, move |e: web_sys::WheelEvent| {
            q.borrow_mut().push(RawEvent::Wheel {
                dy: e.delta_y() as f32,
            });
        });
        errors.extend(r.err());
    }
    // Pointer and touch.
    for name in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
        let (q, unlock) = (raw.clone(), unlock.clone());
        let r = listen(win, name, true, move |e: web_sys::PointerEvent| {
            let kind = pointer_kind(&e.pointer_type());
            let id = e.pointer_id();
            let mut q = q.borrow_mut();
            match name {
                "pointercancel" => q.push(RawEvent::PointerGone { id }),
                _ => {
                    if name == "pointerdown" {
                        unlock();
                    }
                    let down = match name {
                        "pointerdown" => true,
                        "pointerup" => false,
                        _ => e.buttons() != 0,
                    };
                    q.push(RawEvent::Pointer {
                        id,
                        x: e.client_x() as f32,
                        y: e.client_y() as f32,
                        down,
                        primary: e.is_primary(),
                        kind,
                    });
                    if name == "pointerup" && kind == PointerKind::Touch {
                        q.push(RawEvent::PointerGone { id });
                    }
                }
            }
        });
        errors.extend(r.err());
    }
    // Touch end also counts as a gesture for audio on iOS.
    {
        let unlock = unlock.clone();
        let r = listen(win, "touchend", true, move |_: web_sys::Event| unlock());
        errors.extend(r.err());
    }
    // Focus loss: release everything.
    {
        let q = raw.clone();
        let r = listen(win, "blur", true, move |_: web_sys::Event| {
            q.borrow_mut().push(RawEvent::Blur);
        });
        errors.extend(r.err());
    }
    // Gamepads: only poll once one has been seen.
    {
        let seen = pads_seen.clone();
        let r = listen(win, "gamepadconnected", true, move |_: web_sys::Event| {
            seen.set(true);
        });
        errors.extend(r.err());
    }
    // Visibility: pause when hidden.
    if let Some(doc) = &document {
        let (rn, pending, d) = (runner.clone(), pending_hidden.clone(), doc.clone());
        let r = listen(doc.as_ref(), "visibilitychange", true, move |_: web_sys::Event| {
            let hidden = d.hidden();
            match rn.try_borrow_mut() {
                Ok(mut r) => r.app.set_hidden(hidden),
                Err(_) => pending.set(Some(hidden)),
            }
        });
        errors.extend(r.err());
    }
    // prefers-reduced-motion changes.
    if let Some(m) = &mql {
        let (rn, pending, m2) = (runner.clone(), pending_reduced_motion.clone(), m.clone());
        let r = listen(m.as_ref(), "change", true, move |_: web_sys::Event| {
            let v = m2.matches();
            match rn.try_borrow_mut() {
                Ok(r) => r.app.world.res_mut::<Platform>().reduced_motion = v,
                Err(_) => pending.set(Some(v)),
            }
        });
        errors.extend(r.err());
    }

    if !errors.is_empty()
        && let Ok(r) = runner.try_borrow()
    {
        let mut log = r.app.world.res_mut::<ErrorLog>();
        for e in errors {
            log.error(format!("input listener: {e}"));
        }
    }

    // requestAnimationFrame loop.
    let cb: FrameCallback = Rc::new(RefCell::new(None));
    let cb2 = cb.clone();
    *cb2.borrow_mut() = Some(Closure::new(move |t: f64| {
        if let Ok(mut r) = runner.try_borrow_mut() {
            r.frame(t);
        }
        request_frame(&cb);
    }));
    request_frame(&cb2);
}

fn request_frame(cb: &FrameCallback) {
    let Some(window) = web_sys::window() else { return };
    if let Some(f) = cb.borrow().as_ref()
        && let Err(e) = window.request_animation_frame(f.as_ref().unchecked_ref())
    {
        super::console_log(true, &format!("requestAnimationFrame: {}", super::js_err(e)));
    }
}
