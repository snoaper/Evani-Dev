//! Shader pre-warm (feature `prewarm`, ChimpDuty intake limit 4).
//!
//! Without it the canvas starts drawing as soon as the engine's base
//! pipelines are built. Everything else is still being made while the game
//! runs: the 4x MSAA variants, the model, skin and effect pipelines, and, the
//! big one, a kit's own material code (`KitMaterial::set_wgsl` on its first
//! frame rebuilds the forward pipelines and makes the cut-out shadow
//! pipeline). The browser builds a pipeline in its GPU process in the order
//! the commands arrive, so the frame that follows a late build waits for it:
//! a stall of a few hundred milliseconds just after the game appears.
//!
//! With it the canvas keeps its loading state a little longer: the render
//! module keeps returning "not ready" (the frame is skipped, the game's
//! systems still run) while it
//!
//! 1. builds the kit material code the kit has set so far (so code set in
//!    the first frames is compiled during loading, not after),
//! 2. waits until no pipeline build has been in flight for
//!    [`Prewarm::quiet_frames`] frames, after at least [`Prewarm::min_frames`],
//! 3. then asks the GPU queue once for "submitted work done" and waits for
//!    the answer, which comes back only after the GPU process has finished
//!    every pipeline it was asked for.
//!
//! [`Prewarm::max_ms`] caps the wait (a build that never finishes cannot hold
//! the game back). The game may insert its own [`Prewarm`] resource before
//! the first frame to change the numbers; the status fields tell a test or a
//! HUD how long the warm-up took. Games without the feature are byte for byte
//! what they were.

/// The pre-warm settings and status (a world resource; the render module
/// makes the default one on the first frame if the game has none).
#[derive(Debug, Clone, PartialEq)]
pub struct Prewarm {
    /// Frames the hold lasts at least (the kit's first frames run in it).
    pub min_frames: u32,
    /// Frames in a row with no pipeline build in flight before the GPU is
    /// asked to confirm.
    pub quiet_frames: u32,
    /// The longest the hold may last (ms from its first frame).
    pub max_ms: f64,
    /// Status: frames held so far.
    pub held_frames: u32,
    /// Status: when the hold started (ms, `evani_core::time::now_ms`).
    pub started_ms: Option<f64>,
    /// Status: how long the hold lasted (ms), once it is over.
    pub held_ms: Option<f64>,
    /// Status: the hold ended at `max_ms` rather than on a quiet GPU.
    pub timed_out: bool,
    quiet: u32,
    asked: bool,
}

impl Default for Prewarm {
    fn default() -> Self {
        Self {
            min_frames: 8,
            quiet_frames: 3,
            max_ms: 6000.0,
            held_frames: 0,
            started_ms: None,
            held_ms: None,
            timed_out: false,
            quiet: 0,
            asked: false,
        }
    }
}

/// What the render module does this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Keep holding (skip the frame).
    Hold,
    /// Hold, and ask the GPU queue to confirm it is idle now.
    Ask,
    /// The warm-up is over: draw.
    Draw,
}

impl Prewarm {
    /// Whether the warm-up is over.
    pub fn done(&self) -> bool {
        self.held_ms.is_some()
    }

    /// One frame of the gate. `busy`: a pipeline build is in flight;
    /// `confirmed`: the GPU answered the last ask.
    pub fn step(&mut self, now_ms: f64, busy: bool, confirmed: bool) -> Step {
        if self.done() {
            return Step::Draw;
        }
        let start = *self.started_ms.get_or_insert(now_ms);
        self.held_frames += 1;
        if now_ms - start >= self.max_ms {
            self.timed_out = true;
            return self.finish(now_ms);
        }
        if busy {
            // A new build after the ask: ask again once it is quiet.
            self.quiet = 0;
            self.asked = false;
            return Step::Hold;
        }
        self.quiet += 1;
        if self.held_frames < self.min_frames || self.quiet < self.quiet_frames {
            return Step::Hold;
        }
        if !self.asked {
            self.asked = true;
            return Step::Ask;
        }
        if confirmed {
            return self.finish(now_ms);
        }
        Step::Hold
    }

    fn finish(&mut self, now_ms: f64) -> Step {
        self.held_ms = Some(now_ms - self.started_ms.unwrap_or(now_ms));
        Step::Draw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &mut Prewarm, frames: &[(bool, bool)]) -> Vec<Step> {
        frames.iter().enumerate().map(|(i, &(busy, ok))| p.step(i as f64 * 16.0, busy, ok)).collect()
    }

    #[test]
    fn holds_for_the_minimum_then_asks_then_draws_on_the_answer() {
        let mut p = Prewarm::default();
        let steps = run(&mut p, &[(false, false); 12]);
        assert!(steps[..7].iter().all(|s| *s == Step::Hold));
        assert_eq!(steps[7], Step::Ask);
        assert!(steps[8..].iter().all(|s| *s == Step::Hold), "no answer yet");
        assert!(!p.done());
        assert_eq!(p.step(500.0, false, true), Step::Draw);
        assert!(p.done() && !p.timed_out);
        assert_eq!(p.held_ms, Some(500.0));
        // Over for good.
        assert_eq!(p.step(600.0, true, false), Step::Draw);
    }

    #[test]
    fn a_build_in_flight_resets_the_quiet_count_and_the_ask() {
        let mut p = Prewarm { min_frames: 1, quiet_frames: 2, ..Prewarm::default() };
        assert_eq!(p.step(0.0, false, false), Step::Hold);
        assert_eq!(p.step(16.0, false, false), Step::Ask);
        // The kit sets its code: a rebuild starts after the ask.
        assert_eq!(p.step(32.0, true, true), Step::Hold);
        assert_eq!(p.step(48.0, false, true), Step::Hold);
        // A stale answer does not count: a new ask goes out first.
        assert_eq!(p.step(64.0, false, true), Step::Ask);
        assert_eq!(p.step(80.0, false, true), Step::Draw);
    }

    #[test]
    fn the_cap_ends_a_hold_that_never_goes_quiet() {
        let mut p = Prewarm { max_ms: 100.0, ..Prewarm::default() };
        let steps = run(&mut p, &[(true, false); 8]);
        assert_eq!(steps[6], Step::Hold);
        assert_eq!(steps[7], Step::Draw);
        assert!(p.timed_out);
        assert_eq!(p.held_ms, Some(112.0));
    }
}
