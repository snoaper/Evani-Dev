//! Live code swapping, the engine half (dev builds only, feature
//! `hotpatch`; docs/LIVE_API.md 10).
//!
//! With the feature on, the engine calls every entry point a game has
//! through Dioxus subsecond's jump table, so no game, kit or look needs any
//! code of its own:
//!
//! * every system ([`crate::ecs::SystemDesc::new`] wraps it with
//!   [`wrap_system`]),
//! * every engine module's `frame` and every scene's `update` (hidden
//!   trait methods with a default body, see `plugin.rs` and `scene.rs`),
//! * the frame step itself (`App::frame`, from the web runner).
//!
//! A patch built by `dx serve --hot-patch` holds every function of each
//! crate that changed. Once its jump table is committed, the next call of
//! an entry point in a changed crate goes to the new code, and everything
//! that code calls inside the patch is new too. Patches are handed to
//! subsecond only at the top of a frame ([`run_before_frame`]), so a frame
//! never mixes old and new code.
//!
//! The receiver (bridge socket, patch loading, toast) lives in evani-live.
//!
//! subsecond is Copyright (c) Dioxus Labs, MIT OR Apache-2.0
//! (https://github.com/DioxusLabs/dioxus). The calling pattern follows its
//! documentation; no code is copied.
#![allow(unsafe_code)]

use std::any::TypeId;
use std::cell::Cell;

use subsecond::HotFn;

use crate::ecs::{SystemFn, World};

/// Is live code swapping compiled in? (Always true where this module exists.)
pub const ENABLED: bool = true;

thread_local! {
    static BEFORE_FRAME: Cell<Option<fn()>> = const { Cell::new(None) };
}

/// Run `f` at the top of every frame, before any system (the cutover point
/// where evani-live hands a waiting patch to subsecond).
pub fn set_before_frame(f: fn()) {
    BEFORE_FRAME.with(|c| c.set(Some(f)));
}

/// Called by the runner at the top of every frame.
pub fn run_before_frame() {
    if let Some(f) = BEFORE_FRAME.with(Cell::get) {
        f();
    }
}

/// Calls `f` through the jump table. A generic function, so each system
/// type gets its own copy in the crate that defines the system: when that
/// crate is patched, the copy (and the system body inlined into it) is in
/// the patch.
fn trampoline<F: FnMut(&mut World)>(f: &mut F, world: &mut World) {
    f(world);
}

/// Wrap a system so it runs through subsecond's jump table.
///
/// * A plain `fn(&mut World)` pointer is looked up in the table directly.
/// * Anything else (a function item, a closure) runs through
///   [`trampoline`], taken as a function pointer. A closure is never handed
///   to `HotFn` itself: subsecond treats any pointer-sized callable as a
///   function pointer, which a closure holding one reference is not.
pub fn wrap_system<F: FnMut(&mut World) + 'static>(mut f: F) -> SystemFn {
    if TypeId::of::<F>() == TypeId::of::<fn(&mut World)>() {
        // SAFETY: F is exactly `fn(&mut World)` (same TypeId), so this is a
        // plain copy of the pointer.
        let ptr: fn(&mut World) = unsafe { std::mem::transmute_copy(&f) };
        Box::new(move |w: &mut World| HotFn::current(ptr).call((w,)))
    } else {
        let tramp: fn(&mut F, &mut World) = trampoline::<F>;
        Box::new(move |w: &mut World| HotFn::current(tramp).call((&mut f, w)))
    }
}

/// One rendered frame through the jump table (the web runner's frame step).
pub fn frame(app: &mut crate::App, now_ms: f64) {
    let step: fn(&mut crate::App, f64) = crate::App::frame;
    HotFn::current(step).call((app, now_ms));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Stage, SystemDesc};

    #[derive(Default)]
    struct Count(u32);

    fn bump(w: &mut World) {
        w.res_mut::<Count>().0 += 1;
    }

    #[test]
    fn wrapped_systems_still_run_without_a_patch() {
        let mut w = World::default();
        w.insert_resource(Count::default());
        // A fn pointer, a fn item and a capturing closure (pointer-sized).
        let p: fn(&mut World) = bump;
        let mut a = wrap_system(p);
        let mut b = wrap_system(bump);
        let step = Box::new(2u32);
        let mut c = wrap_system(move |w: &mut World| w.res_mut::<Count>().0 += *step);
        a(&mut w);
        b(&mut w);
        c(&mut w);
        assert_eq!(w.res::<Count>().0, 4);
        // And through the schedule.
        let mut d = SystemDesc::new("bump", Stage::Update, bump);
        (d.run)(&mut w);
        assert_eq!(w.res::<Count>().0, 5);
    }

    #[test]
    fn before_frame_hook_runs() {
        thread_local! { static HIT: Cell<u32> = const { Cell::new(0) }; }
        fn hit() {
            HIT.with(|h| h.set(h.get() + 1));
        }
        set_before_frame(hit);
        run_before_frame();
        run_before_frame();
        assert_eq!(HIT.with(Cell::get), 2);
    }
}
