//! Mouse-look through the browser's pointer lock (feature `pointer-lock`; a
//! kit opts in with `kit.pointer_lock()`).
//!
//! The lock hides the cursor and gives raw, unbounded mouse movement: the
//! existing `mouse:dx` / `mouse:dy` bindings keep working unchanged, they
//! just stop running into the edge of the window. Look is therefore the same
//! action as ever (`look_x` bound to `mouse:dx`, the gamepad's right stick
//! and, on a phone, the touch look pad); this module only owns the lock.
//!
//! This file is the logic (no browser): what the kit wants and what the page
//! reports. The page side (listeners, `requestPointerLock`, the watchdog) is
//! `web/pointer_lock.rs`. See README "Pointer lock".
//!
//! **The lock always lets go.** It is released, immediately and from inside
//! the event handler (not at the next frame), on Esc, window blur, a hidden
//! or frozen tab, page hide and unload, a refused or failed lock, a fatal
//! engine error, a panic, and when frames stop arriving (a watchdog). Every
//! release the engine did not ask for clears the kit's wish to be locked and
//! records why ([`LockLoss`]); the kit asks again ([`PointerLock::request`])
//! when it wants the mouse back, for instance on a "Resume" click.

use crate::ecs::World;

/// Why the lock ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockLoss {
    /// The kit asked for it ([`PointerLock::release`]).
    Released,
    /// Esc, or the browser's own exit (it reserves Esc while locked).
    Escape,
    /// The window lost focus.
    Blur,
    /// The tab was hidden or frozen.
    Hidden,
    /// The page is going away.
    Unload,
    /// The browser refused the lock (no user gesture, cooldown after Esc,
    /// an embedding that forbids it).
    Refused,
    /// A fatal engine error or panic, or frames stopped arriving.
    Error,
}

/// The kit's view of the lock: a world resource, inserted by
/// [`crate::plugin::PluginContext::pointer_lock`].
#[derive(Debug, Clone, Default)]
pub struct PointerLock {
    wanted: bool,
    locked: bool,
    refused: bool,
    loss: Option<LockLoss>,
    revision: u64,
}

impl PointerLock {
    /// Ask for the lock. The browser only grants it inside a user gesture, so
    /// the page takes it at the player's next click (or key) after this.
    /// Clears an earlier refusal and loss.
    pub fn request(&mut self) {
        if !self.wanted {
            self.wanted = true;
            self.bump();
        }
        self.refused = false;
        self.loss = None;
    }

    /// Give the mouse back (menu, pause, results screen). The page exits the
    /// lock at once.
    pub fn release(&mut self) {
        if self.wanted {
            self.wanted = false;
            self.bump();
        }
    }

    /// The kit wants the lock (and it is not necessarily granted yet).
    pub fn wanted(&self) -> bool {
        self.wanted
    }

    /// The lock is held: the cursor is hidden and mouse movement is raw.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// The last request was refused. The kit falls back to free-look (mouse
    /// movement without the lock, or drag-to-look) and may ask again.
    pub fn refused(&self) -> bool {
        self.refused
    }

    /// Why the lock last ended, until [`PointerLock::request`] or
    /// [`PointerLock::take_loss`]. A kit shows its pause menu on
    /// `Some(Escape | Blur | Hidden | Refused | Error)`.
    pub fn last_loss(&self) -> Option<LockLoss> {
        self.loss
    }

    /// [`PointerLock::last_loss`], read once.
    pub fn take_loss(&mut self) -> Option<LockLoss> {
        self.loss.take()
    }

    /// Bumped on every change the page has to act on.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    // ---- the page's side --------------------------------------------------

    /// The browser granted the lock.
    pub fn on_locked(&mut self) {
        if !self.locked {
            self.locked = true;
            self.refused = false;
            self.loss = None;
            self.bump();
        }
    }

    /// The lock ended for `cause`. Anything but the kit's own release also
    /// drops the wish, so nothing grabs the mouse back on its own.
    pub fn on_lost(&mut self, cause: LockLoss) {
        if self.locked {
            self.locked = false;
            self.bump();
        }
        if cause != LockLoss::Released {
            self.wanted = false;
            self.bump();
            self.loss = Some(cause);
        }
        if cause == LockLoss::Refused {
            self.refused = true;
        }
    }
}

/// Ask for the lock from a system or scene. Does nothing unless the kit
/// declared `kit.pointer_lock()`.
pub fn request(world: &World) {
    if let Some(mut p) = world.try_res_mut::<PointerLock>() {
        p.request();
    }
}

/// Give the mouse back from a system or scene.
pub fn release(world: &World) {
    if let Some(mut p) = world.try_res_mut::<PointerLock>() {
        p.release();
    }
}

/// Whether the lock is held (false without a declared lock).
pub fn locked(world: &World) -> bool {
    world.try_res::<PointerLock>().is_some_and(|p| p.is_locked())
}
