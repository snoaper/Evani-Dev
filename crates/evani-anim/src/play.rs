//! The simulation side: what gameplay may read and step. Plain integers
//! advanced once per fixed 60 Hz tick (never wall-clock time), so two runs
//! of the same inputs hold the same state on every machine. The render side
//! turns this state into weights and frame positions; nothing it computes
//! comes back here.
//!
//! Clip lengths come from kit data (ticks), not from a streamed model, so
//! the simulation never waits for a download.

/// One character's animation state: which state (the kit's own numbering,
/// usually a clip), how long it has run, and the crossfade from the last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Player {
    pub state: u16,
    /// Ticks since `state` started.
    pub ticks: u32,
    /// The state faded out of, and its ticks (it keeps running while it fades).
    pub prev: u16,
    pub prev_ticks: u32,
    /// The crossfade length and how far it has gone, in ticks.
    pub fade: u16,
    pub faded: u16,
}

impl Player {
    pub fn new(state: u16) -> Self {
        Player { state, prev: state, ..Player::default() }
    }

    /// Switch to `state` with a crossfade of `fade` ticks (0 cuts). Playing
    /// the state already playing changes nothing; [`Player::restart`] does.
    pub fn play(&mut self, state: u16, fade: u16) {
        if state != self.state {
            self.restart(state, fade);
        }
    }

    /// Start `state` from its first tick, fading from what plays now.
    pub fn restart(&mut self, state: u16, fade: u16) {
        self.prev = self.state;
        self.prev_ticks = self.ticks;
        self.state = state;
        self.ticks = 0;
        self.fade = fade;
        self.faded = 0;
    }

    /// One fixed tick.
    pub fn step(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
        self.prev_ticks = self.prev_ticks.saturating_add(1);
        if self.faded < self.fade {
            self.faded += 1;
        }
    }

    /// Whether a crossfade is still running.
    pub fn fading(&self) -> bool {
        self.faded < self.fade
    }

    /// Render-only: the current state's weight, `sub` of a tick past the
    /// last step (0 to 1); the previous state's weight is `1 - this`.
    pub fn weight(&self, sub: f32) -> f32 {
        if !self.fading() {
            return 1.0;
        }
        let sub = if sub.is_nan() { 0.0 } else { sub.clamp(0.0, 1.0) };
        ((f32::from(self.faded) + sub) / f32::from(self.fade)).min(1.0)
    }

    /// Whether the last step reached tick `mark` of the current state, a
    /// clip `len` ticks long: gameplay events at key ticks (a footstep, the
    /// frame a swing hits). A looped clip fires each time round; a mark at 0
    /// fires on each wrap, not on the first tick.
    pub fn hit(&self, mark: u32, len: u32, looped: bool) -> bool {
        if self.ticks == 0 {
            return false;
        }
        if looped && len > 0 { self.ticks % len == mark % len } else { self.ticks == mark }
    }
}

/// One clip in a 1D blend space: its place on the axis (a speed, a lean)
/// and its length in ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub clip: u16,
    pub at: f32,
    pub ticks: u32,
}

/// The two points around `x` and the weight of the second (points sorted
/// by `at`; outside the ends, the end point alone).
pub fn pick(points: &[Point], x: f32) -> (usize, usize, f32) {
    let Some(last) = points.len().checked_sub(1) else { return (0, 0, 0.0) };
    if x.is_nan() || x <= points[0].at {
        return (0, 0, 0.0);
    }
    if x >= points[last].at {
        return (last, last, 0.0);
    }
    let mut i = 0;
    while i + 1 < last && points[i + 1].at <= x {
        i += 1;
    }
    let (a, b) = (points[i].at, points[i + 1].at);
    let w = if b > a { (x - a) / (b - a) } else { 0.0 };
    (i, i + 1, w.clamp(0.0, 1.0))
}

/// A 1D blend space's phase: one shared cycle position (2^32 per cycle), so
/// a walk and a run blended together keep their feet in step. Stepped in
/// the simulation; each clip samples the same phase of its own length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Blend1d {
    pub phase: u32,
}

impl Blend1d {
    /// One fixed tick at axis position `x`: the phase moves by one tick of
    /// the blended cycle length.
    pub fn step(&mut self, points: &[Point], x: f32) {
        if points.is_empty() {
            return;
        }
        let (i, j, w) = pick(points, x);
        let (a, b) = (f64::from(points[i].ticks), f64::from(points[j].ticks));
        let len = (a + (b - a) * f64::from(w)).max(1.0);
        let inc = (4_294_967_296.0 / len).min(4_294_967_295.0) as u32;
        self.phase = self.phase.wrapping_add(inc);
    }

    /// Whether the last step crossed `mark` (a phase, 2^32 per cycle; a
    /// footstep at half a cycle is 2^31).
    pub fn crossed(&self, before: u32, mark: u32) -> bool {
        let step = self.phase.wrapping_sub(before);
        mark.wrapping_sub(before).wrapping_sub(1) < step
    }
}
