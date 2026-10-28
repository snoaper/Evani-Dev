//! The pre-warm hold (feature `prewarm`; the rule is in [`crate::prewarm`]).
//! Called by [`Gpu::frame`] once the engine pipelines exist: while it
//! answers true the frame is skipped, as before they existed.

use core::sync::atomic::{AtomicU32, Ordering};

use evani_core::ecs::World;

use super::Gpu;
use crate::prewarm::{Prewarm, Step};

/// The last "submitted work done" ask and the last answer (ask numbers).
static ASKED: AtomicU32 = AtomicU32::new(0);
static ANSWERED: AtomicU32 = AtomicU32::new(0);

impl Gpu {
    /// Whether this frame is still held for the warm-up.
    pub(super) fn prewarm_hold(&mut self, world: &mut World) -> bool {
        if !world.has_resource::<Prewarm>() {
            world.insert_resource(Prewarm::default());
        }
        if world.res::<Prewarm>().done() {
            return false;
        }
        // Kit code set in the first frames is built now, inside the hold
        // (the draw path would only see it once the game is on screen).
        #[cfg(feature = "kit-material")]
        if self.kit.prepare(&self.device, &self.queue, &self.layouts, world) {
            self.pipes.relaunch_kit(&self.device, &self.layouts, self.formats, world);
        }
        let busy = self.pipes.in_flight();
        let asked = ASKED.load(Ordering::Relaxed);
        let confirmed = asked != 0 && ANSWERED.load(Ordering::Relaxed) == asked;
        let step = world.res_mut::<Prewarm>().step(evani_core::time::now_ms(), busy, confirmed);
        match step {
            Step::Draw => false,
            Step::Hold => true,
            Step::Ask => {
                let n = asked.wrapping_add(1).max(1);
                ASKED.store(n, Ordering::Relaxed);
                // Answered after the GPU process has worked through every
                // command sent so far, pipeline builds included.
                self.queue.on_submitted_work_done(move || ANSWERED.store(n, Ordering::Relaxed));
                true
            }
        }
    }
}
