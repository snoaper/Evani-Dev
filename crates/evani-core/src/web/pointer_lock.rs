//! Page side of the pointer lock (feature `pointer-lock`): takes the lock at
//! the player's next gesture while the kit wants it, and lets go of it on
//! every path that must end it. See `crate::pointer_lock` for the logic.
//!
//! The rule is that the lock is never held longer than it should be, so every
//! release here runs inside the event handler that noticed the cause, never
//! "at the next frame" (a hidden or stuck page has no next frame):
//!
//! - Esc, window blur, tab hidden or frozen, page hide, `beforeunload` and
//!   `unload`: the listeners call `exitPointerLock` themselves.
//! - A refused or failed lock (`pointerlockerror`), and a lock granted after
//!   the kit stopped wanting it, are undone at once.
//! - A panic (the hook in `web/mod.rs` calls [`exit_now`]) and a fatal engine
//!   message ([`super::show_message`], an error in `set_engine_status`).
//! - A watchdog timer lets go if frames stop arriving while the lock is held.
//! - The kit's own release ([`crate::pointer_lock::PointerLock::release`]) is
//!   applied by [`LockDom::sync`] each frame.
//!
//! Listeners never touch the world: they set shared cells and queue events
//! that `sync` applies at the start of the next frame, like every other input.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use super::runner::listen;
use crate::ecs::World;
use crate::pointer_lock::{LockLoss, PointerLock};

/// Frames silent for this long (ms) while locked mean something is wrong.
const WATCHDOG_STALE_MS: f64 = 1500.0;
/// How often (ms) the watchdog looks.
const WATCHDOG_EVERY_MS: i32 = 500;

#[derive(Debug, Clone, Copy)]
enum Ev {
    Locked,
    Lost(LockLoss),
}

struct Shared {
    /// The kit wants the lock (mirrors the resource, cleared at once by any
    /// forced release so a click in between can't take it back).
    wanted: Cell<bool>,
    /// The browser holds the lock for us.
    locked: Cell<bool>,
    /// Why we asked the browser to exit, for the `pointerlockchange` that follows.
    cause: Cell<Option<LockLoss>>,
    events: RefCell<Vec<Ev>>,
    /// `performance.now()` of the last frame.
    last_frame: Cell<f64>,
}

/// The installed lock handling (one per page).
pub(crate) struct LockDom {
    shared: Rc<Shared>,
}

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

/// Let go of the lock right now, whatever the engine's state. Safe to call
/// from the panic hook and from anywhere else: it does nothing when there is
/// no lock, and it touches no engine state.
pub(crate) fn exit_now() {
    if let Some(doc) = document()
        && doc.pointer_lock_element().is_some()
    {
        doc.exit_pointer_lock();
    }
}

/// Release for `cause`: drop the wish first (so nothing re-takes the lock),
/// then ask the browser to exit if it is locked.
fn exit(shared: &Shared, cause: LockLoss) {
    shared.wanted.set(false);
    let Some(doc) = document() else { return };
    if doc.pointer_lock_element().is_some() {
        if shared.cause.get().is_none() {
            shared.cause.set(Some(cause));
        }
        doc.exit_pointer_lock();
    } else if shared.locked.replace(false) {
        // Our flag says locked but the browser already let go.
        shared.events.borrow_mut().push(Ev::Lost(cause));
    }
}

/// Take the lock if the kit wants it and this is called from a user gesture.
fn try_request(shared: &Shared) {
    if !shared.wanted.get() || shared.locked.get() {
        return;
    }
    let Some(doc) = document() else { return };
    let target = doc
        .query_selector("canvas")
        .ok()
        .flatten()
        .or_else(|| doc.body().map(web_sys::Element::from));
    if let Some(el) = target {
        el.request_pointer_lock();
    }
}

/// Build the handling if the kit declared a lock. Problems go to `errors`.
pub(crate) fn install(world: &World, errors: &mut Vec<String>) -> Option<LockDom> {
    world.try_res::<PointerLock>()?;
    let window = web_sys::window()?;
    let doc = window.document()?;
    let shared = Rc::new(Shared {
        wanted: Cell::new(false),
        locked: Cell::new(false),
        cause: Cell::new(None),
        events: RefCell::new(Vec::with_capacity(8)),
        last_frame: Cell::new(super::performance_now()),
    });
    let win: &web_sys::EventTarget = window.as_ref();
    let dt: &web_sys::EventTarget = doc.as_ref();

    // The browser's word on whether we hold the lock.
    {
        let (s, d) = (shared.clone(), doc.clone());
        let r = listen(dt, "pointerlockchange", true, move |_: web_sys::Event| {
            if d.pointer_lock_element().is_some() {
                if s.wanted.get() {
                    s.locked.set(true);
                    s.events.borrow_mut().push(Ev::Locked);
                } else {
                    // Granted after the kit stopped wanting it: undo.
                    s.cause.set(Some(LockLoss::Released));
                    d.exit_pointer_lock();
                }
            } else {
                let cause = s.cause.take();
                if s.locked.replace(false) {
                    // No cause of ours means the player or the browser ended it (Esc).
                    let cause = cause.unwrap_or(LockLoss::Escape);
                    if cause != LockLoss::Released {
                        s.wanted.set(false);
                    }
                    s.events.borrow_mut().push(Ev::Lost(cause));
                }
            }
        });
        errors.extend(r.err());
    }
    {
        let s = shared.clone();
        let r = listen(dt, "pointerlockerror", true, move |_: web_sys::Event| {
            s.wanted.set(false);
            s.locked.set(false);
            s.events.borrow_mut().push(Ev::Lost(LockLoss::Refused));
        });
        errors.extend(r.err());
    }

    // Take the lock at the next gesture while the kit wants it. Touch has no
    // cursor to hide, so only a mouse or pen press counts.
    {
        let s = shared.clone();
        let r = listen(win, "pointerdown", true, move |e: web_sys::PointerEvent| {
            if e.pointer_type() != "touch" {
                try_request(&s);
            }
        });
        errors.extend(r.err());
        let s = shared.clone();
        let r = listen(win, "keydown", true, move |e: web_sys::KeyboardEvent| {
            if e.code() == "Escape" {
                exit(&s, LockLoss::Escape);
            } else {
                try_request(&s);
            }
        });
        errors.extend(r.err());
    }

    // Every cause that must end the lock, released inside the handler.
    for (name, cause) in [
        ("blur", LockLoss::Blur),
        ("pagehide", LockLoss::Unload),
        ("beforeunload", LockLoss::Unload),
        ("unload", LockLoss::Unload),
    ] {
        let s = shared.clone();
        let r = listen(win, name, true, move |_: web_sys::Event| exit(&s, cause));
        errors.extend(r.err());
    }
    {
        let (s, d) = (shared.clone(), doc.clone());
        let r = listen(dt, "visibilitychange", true, move |_: web_sys::Event| {
            if d.hidden() {
                exit(&s, LockLoss::Hidden);
            }
        });
        errors.extend(r.err());
        let s = shared.clone();
        let r = listen(dt, "freeze", true, move |_: web_sys::Event| exit(&s, LockLoss::Hidden));
        errors.extend(r.err());
    }

    // Watchdog: if frames stop while we hold the lock (a stalled loop, an
    // error the page swallowed), let go.
    {
        let (s, d) = (shared.clone(), doc.clone());
        let cb = Closure::<dyn FnMut()>::new(move || {
            if s.locked.get()
                && !d.hidden()
                && super::performance_now() - s.last_frame.get() > WATCHDOG_STALE_MS
            {
                exit(&s, LockLoss::Error);
            }
        });
        if let Err(e) = window.set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            WATCHDOG_EVERY_MS,
        ) {
            errors.push(super::js_err(e));
        }
        // The timer lives as long as the page.
        cb.forget();
    }

    Some(LockDom { shared })
}

impl LockDom {
    /// Apply what the listeners saw, and the kit's wish. Called at the start
    /// of every frame, before the simulation steps.
    pub(crate) fn sync(&self, world: &World) {
        let s = &self.shared;
        s.last_frame.set(super::performance_now());
        let mut p = world.res_mut::<PointerLock>();
        for ev in s.events.borrow_mut().drain(..) {
            match ev {
                Ev::Locked => p.on_locked(),
                Ev::Lost(cause) => p.on_lost(cause),
            }
        }
        s.wanted.set(p.wanted());
        if !p.wanted() && s.locked.get() {
            // The kit gave the mouse back.
            exit(s, LockLoss::Released);
        }
    }
}
