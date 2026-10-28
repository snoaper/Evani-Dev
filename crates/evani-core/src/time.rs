//! Fixed-timestep loop logic and frame statistics.
//!
//! The pure logic here is driven by plain millisecond timestamps so it can be
//! unit-tested natively. On wasm the timestamps come from `performance.now()`
//! via the requestAnimationFrame driver in [`crate::web`]; `std::time::Instant`
//! is never used on wasm.

/// Result of advancing the fixed-step clock by one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepPlan {
    /// Number of fixed simulation steps to run this frame.
    pub steps: u32,
    /// Interpolation factor in `[0, 1)` between the previous and current
    /// simulation state, for rendering.
    pub alpha: f32,
    /// True when the frame delta was so large that simulation time was dropped
    /// (the max-steps clamp kicked in, e.g. after a long stall).
    pub dropped_time: bool,
}

/// Fixed-timestep accumulator (default 60 Hz) with a max-steps clamp.
#[derive(Debug, Clone)]
pub struct FixedStep {
    step_ms: f64,
    accumulator_ms: f64,
    max_steps: u32,
    /// Largest frame delta accepted before clamping (protects against tab
    /// switches and debugger pauses).
    max_frame_ms: f64,
    last_ms: Option<f64>,
}

impl FixedStep {
    /// A fixed step of `hz` simulation ticks per second.
    pub fn new(hz: u32, max_steps: u32) -> Self {
        let hz = hz.max(1);
        Self {
            step_ms: 1000.0 / f64::from(hz),
            accumulator_ms: 0.0,
            max_steps: max_steps.max(1),
            max_frame_ms: 250.0,
            last_ms: None,
        }
    }

    /// Fixed step length in seconds.
    pub fn step_seconds(&self) -> f32 {
        (self.step_ms / 1000.0) as f32
    }

    /// Fixed step length in milliseconds.
    pub fn step_ms(&self) -> f64 {
        self.step_ms
    }

    pub fn max_steps(&self) -> u32 {
        self.max_steps
    }

    /// Forget the previous timestamp and any accumulated time. Call after the
    /// tab becomes visible again so hidden time is not fast-forwarded.
    pub fn reset(&mut self) {
        self.accumulator_ms = 0.0;
        self.last_ms = None;
    }

    /// Advance using an absolute timestamp in milliseconds (e.g. the rAF
    /// timestamp or `performance.now()`). The first call only primes the clock.
    pub fn advance_to(&mut self, now_ms: f64) -> StepPlan {
        let dt = match self.last_ms {
            Some(last) => (now_ms - last).max(0.0),
            None => 0.0,
        };
        self.last_ms = Some(now_ms);
        self.advance_by(dt)
    }

    /// Advance by a frame delta in milliseconds.
    pub fn advance_by(&mut self, dt_ms: f64) -> StepPlan {
        let mut dropped_time = false;
        let dt = if dt_ms.is_finite() {
            dt_ms.max(0.0)
        } else {
            0.0
        };
        let dt = if dt > self.max_frame_ms {
            dropped_time = true;
            self.max_frame_ms
        } else {
            dt
        };
        self.accumulator_ms += dt;
        let mut steps = 0;
        while self.accumulator_ms >= self.step_ms && steps < self.max_steps {
            self.accumulator_ms -= self.step_ms;
            steps += 1;
        }
        if self.accumulator_ms >= self.step_ms {
            // Spiral-of-death guard: drop the backlog, keep the fraction.
            dropped_time = true;
            self.accumulator_ms %= self.step_ms;
        }
        let alpha = (self.accumulator_ms / self.step_ms).clamp(0.0, 0.999_999) as f32;
        StepPlan {
            steps,
            alpha,
            dropped_time,
        }
    }
}

impl Default for FixedStep {
    fn default() -> Self {
        Self::new(60, 5)
    }
}

/// Rolling frame-time statistics without allocation (fixed ring buffer).
#[derive(Debug, Clone)]
pub struct FrameStats {
    frame_ms: [f32; 64],
    cpu_ms: [f32; 64],
    len: usize,
    head: usize,
    frames: u64,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            frame_ms: [0.0; 64],
            cpu_ms: [0.0; 64],
            len: 0,
            head: 0,
            frames: 0,
        }
    }
}

impl FrameStats {
    /// Record one frame: the time since the previous frame and the CPU time
    /// spent inside the engine's frame callback.
    pub fn record(&mut self, frame_ms: f32, cpu_ms: f32) {
        self.frame_ms[self.head] = frame_ms;
        self.cpu_ms[self.head] = cpu_ms;
        self.head = (self.head + 1) % self.frame_ms.len();
        self.len = (self.len + 1).min(self.frame_ms.len());
        self.frames += 1;
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    fn mean(buf: &[f32], len: usize) -> f32 {
        if len == 0 {
            return 0.0;
        }
        buf.iter().take(len).sum::<f32>() / len as f32
    }

    /// Mean frame time in ms over the window.
    pub fn mean_frame_ms(&self) -> f32 {
        Self::mean(&self.frame_ms, self.len)
    }

    /// Mean CPU time in ms over the window.
    pub fn mean_cpu_ms(&self) -> f32 {
        Self::mean(&self.cpu_ms, self.len)
    }

    /// Frames per second derived from the mean frame time.
    pub fn fps(&self) -> f32 {
        let m = self.mean_frame_ms();
        if m > 0.0 { 1000.0 / m } else { 0.0 }
    }
}

/// Simulation and frame time, available to systems as a world resource.
#[derive(Debug, Clone, Default)]
pub struct Time {
    /// Fixed simulation ticks run so far.
    pub tick: u64,
    /// Fixed step in seconds (1/60 by default).
    pub fixed_dt: f32,
    /// Simulation time in seconds (`tick * fixed_dt`).
    pub sim_seconds: f64,
    /// Real time between the last two rendered frames, in seconds.
    pub frame_dt: f32,
    /// Interpolation alpha for rendering between the last two sim states.
    pub alpha: f32,
    /// Wall-clock timestamp of the current frame in ms (`performance.now()`).
    pub now_ms: f64,
    /// Rendered frames so far.
    pub frame: u64,
    /// CPU ms the engine spent in the previous frame callback (adaptive
    /// quality reads it; the current frame's is only known when it ends).
    pub cpu_ms: f32,
    /// Live capture only: while `Some`, the sim clock is paused (no fixed
    /// ticks run) and [`Time::render_seconds`] returns this value, so a
    /// capture or bench frame renders the scene as at exactly that sim time.
    /// Copied from `CaptureQueue::sim_time_override` each frame.
    #[cfg(feature = "capture")]
    pub held: Option<f64>,
    /// Live focus only: seconds the game has been frozen by focus (no fixed
    /// ticks). [`Time::render_seconds`] adds it, so render-time animation
    /// keeps playing over the frozen sim; back to 0 when the game resumes.
    #[cfg(feature = "focus")]
    pub focus_extra: f64,
}

impl Time {
    /// Simulation time to render at: the last tick plus the interpolation
    /// alpha. A pure function of the sim clock, so camera paths built on it
    /// are deterministic.
    pub fn render_seconds(&self) -> f64 {
        #[cfg(feature = "capture")]
        if let Some(t) = self.held {
            return t;
        }
        #[cfg(feature = "focus")]
        let extra = self.focus_extra;
        #[cfg(not(feature = "focus"))]
        let extra = 0.0;
        self.sim_seconds + f64::from(self.alpha) * f64::from(self.fixed_dt) + extra
    }
}

/// Current time in milliseconds: `performance.now()` on wasm, a monotonic
/// clock natively (native is only used for tests and tools).
pub fn now_ms() -> f64 {
    #[cfg(all(target_arch = "wasm32", not(evani_headless)))]
    {
        crate::web::performance_now()
    }
    #[cfg(not(all(target_arch = "wasm32", not(evani_headless))))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        start.elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_primes_only() {
        let mut fs = FixedStep::new(60, 5);
        let p = fs.advance_to(1000.0);
        assert_eq!(p.steps, 0);
        assert_eq!(p.alpha, 0.0);
    }

    #[test]
    fn steady_60hz_runs_one_step_per_frame() {
        let mut fs = FixedStep::new(60, 5);
        let mut total = 0;
        for _ in 0..600 {
            total += fs.advance_by(1000.0 / 60.0).steps;
        }
        // Floating error can leave the last step in the accumulator.
        assert!((599..=600).contains(&total), "{total}");
    }

    #[test]
    fn accumulates_partial_frames_and_reports_alpha() {
        let mut fs = FixedStep::new(60, 5);
        let p = fs.advance_by(8.0);
        assert_eq!(p.steps, 0);
        assert!((p.alpha - 8.0 / (1000.0 / 60.0) as f32).abs() < 1e-4);
        let p = fs.advance_by(10.0);
        assert_eq!(p.steps, 1);
        assert!(p.alpha > 0.0 && p.alpha < 1.0);
    }

    #[test]
    fn high_refresh_runs_fewer_steps_than_frames() {
        let mut fs = FixedStep::new(60, 5);
        let mut total = 0;
        for _ in 0..240 {
            total += fs.advance_by(1000.0 / 120.0).steps;
        }
        assert!((119..=120).contains(&total), "{total}");
    }

    #[test]
    fn max_steps_clamp_drops_backlog() {
        let mut fs = FixedStep::new(60, 5);
        let p = fs.advance_by(200.0); // 12 steps worth
        assert_eq!(p.steps, 5);
        assert!(p.dropped_time);
        assert!(p.alpha < 1.0);
        // Next normal frame is back to normal.
        let p = fs.advance_by(1000.0 / 60.0);
        assert!(p.steps <= 2);
    }

    #[test]
    fn huge_and_bad_deltas_are_safe() {
        let mut fs = FixedStep::new(60, 4);
        let p = fs.advance_by(1.0e9);
        assert_eq!(p.steps, 4);
        assert!(p.dropped_time);
        let p = fs.advance_by(f64::NAN);
        assert_eq!(p.steps, 0);
        let p = fs.advance_by(-50.0);
        assert_eq!(p.steps, 0);
    }

    #[test]
    fn reset_forgets_hidden_time() {
        let mut fs = FixedStep::new(60, 5);
        fs.advance_to(0.0);
        fs.advance_to(16.7);
        fs.reset();
        let p = fs.advance_to(60_000.0);
        assert_eq!(p.steps, 0);
    }

    #[test]
    fn frame_stats_mean_and_fps() {
        let mut s = FrameStats::default();
        for _ in 0..100 {
            s.record(16.0, 4.0);
        }
        assert!((s.fps() - 62.5).abs() < 0.01);
        assert!((s.mean_cpu_ms() - 4.0).abs() < 1e-5);
        assert_eq!(s.frames(), 100);
    }
}
