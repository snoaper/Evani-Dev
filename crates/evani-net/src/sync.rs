//! The networking sync layer (feature `sync`): what a game draws online.
//!
//! An online game should never draw raw server state. This layer sits
//! between the game's netcode (any transport, any snapshot format, or the
//! engine's own [`Client`](crate::Client)) and its drawing:
//!
//! * The **locally controlled entity** is predicted ([`Predictor`]): every
//!   fixed tick the game's own step runs with this tick's input at once (the
//!   car turns on the frame the key goes down), and the input is kept in an
//!   [`InputHistory`] under its sequence number. When an authoritative
//!   snapshot says "I have applied your inputs up to `ack`, here is your
//!   state", the predictor rewinds to that state and replays every input
//!   after `ack`. The server stays the authority; the client predicts only
//!   its own entity.
//! * A **correction** never shows as a jump: the difference between where
//!   the entity was drawn and where it now belongs becomes a render offset
//!   ([`ErrorSmoother`]) that decays over a few frames (`net.sync.smooth_rate`).
//!   Only an error over `net.sync.snap_threshold` (a respawn, a teleport)
//!   cuts at once.
//! * **Remote entities** are drawn from a [`SnapshotBuffer`]: snapshots are
//!   kept by server time and sampled a little in the past (the render delay,
//!   `net.interp_delay_ms`, default 100 ms, growing with the jitter the
//!   buffer measures, `net.sync.jitter_scale`), so they move continuously
//!   between snapshots. When snapshots run out (loss, a stall) the buffer
//!   extrapolates along the last motion for at most
//!   `net.sync.extrapolate_max_ms`, then holds; when the late snapshot
//!   arrives the difference is eased away like a correction.
//!
//! Everything here is drawing and prediction bookkeeping: the game's
//! simulation stays its own fixed 60 Hz step with its seeded generator. The
//! render clock the buffer runs on is passed in by the game (tick count times
//! the step plus the frame's alpha), never read from a wall clock.
//!
//! Ideas, not code, from Gabriel Gambetta's "Fast-Paced Multiplayer" series
//! (prediction with sequence numbers and server reconciliation, entity
//! interpolation) and Glenn Fiedler's "Snapshot Interpolation" and
//! "State Synchronization" (an interpolation buffer sized to the send rate
//! and jitter, visual error smoothing); DECISIONS.md "Netcode sync layer".

use std::collections::VecDeque;

use crate::interp::{Interpolate, lerp_angle};
use crate::types::{NetId, Tick};

/// Pose arithmetic the smoothing needs: differences, shifts and a size.
///
/// Implemented for `f32`, `[f32; N]` (a position: the size is its length),
/// [`Angle`] (wraps the short way round) and pairs. A pair's size is its
/// **first** part's, so put the position first (`([f32; 3], Angle)`): the
/// snap threshold is then in metres.
pub trait Offset: Clone {
    /// The offset that changes nothing.
    fn zero() -> Self;
    /// `a - b`.
    fn delta(a: &Self, b: &Self) -> Self;
    /// `a + d`.
    fn shift(a: &Self, d: &Self) -> Self;
    /// `d * k`.
    fn scaled(d: &Self, k: f32) -> Self;
    /// How big an offset is (for the snap threshold).
    fn magnitude(d: &Self) -> f32;
}

impl Offset for f32 {
    fn zero() -> Self {
        0.0
    }
    fn delta(a: &Self, b: &Self) -> Self {
        a - b
    }
    fn shift(a: &Self, d: &Self) -> Self {
        a + d
    }
    fn scaled(d: &Self, k: f32) -> Self {
        d * k
    }
    fn magnitude(d: &Self) -> f32 {
        d.abs()
    }
}

impl<const N: usize> Offset for [f32; N] {
    fn zero() -> Self {
        [0.0; N]
    }
    fn delta(a: &Self, b: &Self) -> Self {
        core::array::from_fn(|i| a[i] - b[i])
    }
    fn shift(a: &Self, d: &Self) -> Self {
        core::array::from_fn(|i| a[i] + d[i])
    }
    fn scaled(d: &Self, k: f32) -> Self {
        core::array::from_fn(|i| d[i] * k)
    }
    fn magnitude(d: &Self) -> f32 {
        d.iter().map(|v| v * v).sum::<f32>().sqrt()
    }
}

/// An angle in radians that blends and corrects the short way round.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Angle(pub f32);

fn wrap_pi(a: f32) -> f32 {
    use core::f32::consts::{PI, TAU};
    let mut d = a % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    d
}

impl Interpolate for Angle {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        Angle(lerp_angle(a.0, b.0, t))
    }
}

impl Offset for Angle {
    fn zero() -> Self {
        Angle(0.0)
    }
    fn delta(a: &Self, b: &Self) -> Self {
        Angle(wrap_pi(a.0 - b.0))
    }
    fn shift(a: &Self, d: &Self) -> Self {
        Angle(a.0 + d.0)
    }
    fn scaled(d: &Self, k: f32) -> Self {
        Angle(d.0 * k)
    }
    fn magnitude(d: &Self) -> f32 {
        wrap_pi(d.0).abs()
    }
}

impl<A: Offset, B: Offset> Offset for (A, B) {
    fn zero() -> Self {
        (A::zero(), B::zero())
    }
    fn delta(a: &Self, b: &Self) -> Self {
        (A::delta(&a.0, &b.0), B::delta(&a.1, &b.1))
    }
    fn shift(a: &Self, d: &Self) -> Self {
        (A::shift(&a.0, &d.0), B::shift(&a.1, &d.1))
    }
    fn scaled(d: &Self, k: f32) -> Self {
        (A::scaled(&d.0, k), B::scaled(&d.1, k))
    }
    fn magnitude(d: &Self) -> f32 {
        A::magnitude(&d.0)
    }
}

/// Default `net.sync.smooth_rate`: a correction decays by `e^-12` a second
/// (half gone in about 58 ms, 95 % in 250 ms: a few frames).
pub const SMOOTH_RATE_DEFAULT: f32 = 12.0;
/// Default `net.sync.snap_threshold` (pose units, metres for a position):
/// errors larger than this cut instead of blending.
pub const SNAP_THRESHOLD_DEFAULT: f32 = 3.0;
/// Default render delay for remote entities (`net.interp_delay_ms`).
pub const INTERP_DELAY_DEFAULT_S: f64 = 0.100;
/// Most the render delay grows to with jitter.
pub const INTERP_DELAY_MAX_S: f64 = 0.500;
/// Default `net.sync.extrapolate_max_ms`.
pub const EXTRAPOLATE_MAX_DEFAULT_S: f64 = 0.200;
/// Default `net.sync.jitter_scale`: the delay covers the send interval plus
/// this many times the measured jitter (0 keeps the delay fixed).
pub const JITTER_SCALE_DEFAULT: f32 = 2.0;
/// Inputs the predictor keeps unacknowledged (4.3 s at 60 Hz).
pub const INPUT_HISTORY_DEFAULT: usize = 256;

/// The sync layer's settings. [`SyncConfig::from_world`] reads them from the
/// registered `net.*` parameters, so the live tools and the Look Lab tune
/// them while the game runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncConfig {
    /// How fast a correction's render offset decays, per second (0: no
    /// smoothing, corrections show at once).
    pub smooth_rate: f32,
    /// Errors larger than this snap instead of blending.
    pub snap_threshold: f32,
    /// The least render delay for remote entities, seconds.
    pub interp_delay_s: f64,
    /// The most the delay grows to with jitter, seconds.
    pub interp_delay_max_s: f64,
    /// How far past the newest snapshot remote entities are extrapolated,
    /// seconds (0: they hold).
    pub extrapolate_max_s: f64,
    /// Jitter allowance in the render delay (0: the delay stays fixed).
    pub jitter_scale: f32,
    /// Unacknowledged inputs kept for replay.
    pub input_history: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            smooth_rate: SMOOTH_RATE_DEFAULT,
            snap_threshold: SNAP_THRESHOLD_DEFAULT,
            interp_delay_s: INTERP_DELAY_DEFAULT_S,
            interp_delay_max_s: INTERP_DELAY_MAX_S,
            extrapolate_max_s: EXTRAPOLATE_MAX_DEFAULT_S,
            jitter_scale: JITTER_SCALE_DEFAULT,
            input_history: INPUT_HISTORY_DEFAULT,
        }
    }
}

impl SyncConfig {
    /// The defaults with `net.interp_delay_ms`, `net.sync.smooth_rate`,
    /// `net.sync.snap_threshold`, `net.sync.extrapolate_max_ms` and
    /// `net.sync.jitter_scale` from the parameter registry (the defaults
    /// alone when the net module is not built in). Cheap: read it every
    /// frame and pass it to [`Predictor::set_config`] and
    /// [`SnapshotBuffer::set_config`] so live changes apply at once.
    pub fn from_world(world: &evani_core::ecs::World) -> Self {
        let mut c = Self::default();
        if let (Some(p), Some(reg)) = (
            world.try_res::<crate::NetParams>(),
            world.try_res::<evani_core::params::ParamRegistry>(),
        ) {
            c.interp_delay_s = f64::from(reg.f32(p.interp_delay_ms).max(0.0)) / 1000.0;
            c.smooth_rate = reg.f32(p.sync_smooth_rate).max(0.0);
            c.snap_threshold = reg.f32(p.sync_snap_threshold).max(0.0);
            c.extrapolate_max_s = f64::from(reg.f32(p.sync_extrapolate_max_ms).max(0.0)) / 1000.0;
            c.jitter_scale = reg.f32(p.sync_jitter_scale).max(0.0);
        }
        c.interp_delay_max_s = c.interp_delay_max_s.max(c.interp_delay_s);
        c
    }
}

/// The client's own inputs by sequence number, oldest first: what is
/// replayed after a correction and resent until the server acknowledges it.
#[derive(Debug, Clone)]
pub struct InputHistory<I> {
    buf: VecDeque<(Tick, I)>,
    cap: usize,
    dropped: u64,
}

impl<I: Clone> InputHistory<I> {
    /// Keeps at most `cap` (at least 1) unacknowledged inputs.
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap.clamp(1, 1024)),
            cap: cap.max(1),
            dropped: 0,
        }
    }

    /// Record the input for sequence `seq`. Sequences only grow: an older or
    /// repeated one is ignored. When full the oldest is dropped (counted in
    /// [`InputHistory::dropped`]; a replay then starts from a later input).
    pub fn push(&mut self, seq: Tick, input: I) {
        if self.buf.back().is_some_and(|b| seq <= b.0) {
            return;
        }
        while self.buf.len() >= self.cap {
            self.buf.pop_front();
            self.dropped += 1;
        }
        self.buf.push_back((seq, input));
    }

    /// The server has applied every input up to and including `seq`.
    pub fn ack(&mut self, seq: Tick) {
        while self.buf.front().is_some_and(|f| f.0 <= seq) {
            self.buf.pop_front();
        }
    }

    /// Unacknowledged inputs, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &(Tick, I)> + ExactSizeIterator {
        self.buf.iter()
    }

    /// The newest `n` inputs, oldest first (to send with redundancy).
    pub fn newest(&self, n: usize) -> impl Iterator<Item = &(Tick, I)> {
        self.buf.iter().skip(self.buf.len().saturating_sub(n))
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn oldest_seq(&self) -> Option<Tick> {
        self.buf.front().map(|f| f.0)
    }

    pub fn newest_seq(&self) -> Option<Tick> {
        self.buf.back().map(|f| f.0)
    }

    /// Inputs lost to the cap so far.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn set_capacity(&mut self, cap: usize) {
        self.cap = cap.max(1);
        while self.buf.len() > self.cap {
            self.buf.pop_front();
            self.dropped += 1;
        }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

/// A render offset that eases a correction away.
///
/// [`ErrorSmoother::add`] takes the error (where the thing was drawn minus
/// where it now belongs); the draw adds [`ErrorSmoother::offset`] to the true
/// pose, and [`ErrorSmoother::decay`] shrinks it by `e^(-rate * dt)` each
/// frame. An error (or a running total) over the snap threshold clears it:
/// the draw cuts.
#[derive(Debug, Clone)]
pub struct ErrorSmoother<P> {
    offset: P,
    active: bool,
    rate: f32,
    snap: f32,
}

/// Offsets smaller than this are dropped (pose units).
const SETTLED: f32 = 1e-4;

impl<P: Offset> ErrorSmoother<P> {
    pub fn new(rate: f32, snap_threshold: f32) -> Self {
        Self {
            offset: P::zero(),
            active: false,
            rate,
            snap: snap_threshold,
        }
    }

    pub fn set(&mut self, rate: f32, snap_threshold: f32) {
        self.rate = rate;
        self.snap = snap_threshold;
    }

    /// Fold in a correction. Returns `true` when it snapped (the error, or
    /// the offset with it, is over the threshold, or smoothing is off).
    pub fn add(&mut self, error: &P) -> bool {
        let next = if self.active {
            P::shift(&self.offset, error)
        } else {
            error.clone()
        };
        if self.rate <= 0.0
            || P::magnitude(error) > self.snap
            || P::magnitude(&next) > self.snap
        {
            self.clear();
            return true;
        }
        self.offset = next;
        self.active = P::magnitude(&self.offset) > SETTLED;
        false
    }

    /// One frame of `dt` seconds.
    pub fn decay(&mut self, dt: f32) {
        if !self.active {
            return;
        }
        if self.rate <= 0.0 {
            self.clear();
            return;
        }
        let k = (-self.rate * dt.max(0.0)).exp();
        self.offset = P::scaled(&self.offset, k);
        if P::magnitude(&self.offset) <= SETTLED {
            self.clear();
        }
    }

    /// The offset to add to the true pose this frame.
    pub fn offset(&self) -> P {
        if self.active {
            self.offset.clone()
        } else {
            P::zero()
        }
    }

    /// `pose` with the offset added.
    pub fn apply(&self, pose: &P) -> P {
        if self.active {
            P::shift(pose, &self.offset)
        } else {
            pose.clone()
        }
    }

    /// How big the offset still is.
    pub fn remaining(&self) -> f32 {
        if self.active {
            P::magnitude(&self.offset)
        } else {
            0.0
        }
    }

    pub fn is_settled(&self) -> bool {
        !self.active
    }

    pub fn clear(&mut self) {
        self.offset = P::zero();
        self.active = false;
    }
}

/// The game side of prediction: its fixed-step simulation of the locally
/// controlled entity, and the pose drawn from it. The same `step` runs on
/// the server, so with the same inputs prediction and server agree; the
/// server's extra knowledge (collisions with others, hits) is what a
/// reconcile corrects.
pub trait SyncSim {
    /// Everything the step reads and writes for the local entity.
    type State: Clone;
    type Input: Clone;
    /// What is drawn (position first, see [`Offset`]).
    type Pose: Interpolate + Offset;
    /// One fixed tick.
    fn step(&self, state: &mut Self::State, input: &Self::Input);
    fn pose(&self, state: &Self::State) -> Self::Pose;
}

/// What one [`Predictor::reconcile`] did.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Correction {
    /// The size of the error between the old prediction and the corrected one.
    pub error: f32,
    /// The error cut instead of blending.
    pub snapped: bool,
    /// Inputs replayed on top of the server's state.
    pub replayed: u32,
    /// The snapshot was older than one already applied, and ignored.
    pub stale: bool,
}

/// Predictor totals, for tests, telemetry and the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PredictStats {
    pub predicted: u64,
    pub reconciles: u64,
    pub replayed: u64,
    /// Reconciles that moved the prediction (error over 1e-4).
    pub corrections: u64,
    pub snaps: u64,
    pub stale: u64,
    /// Largest error seen.
    pub max_error: f32,
}

/// Client-side prediction of the locally controlled entity, with server
/// reconciliation and error smoothing.
///
/// Per fixed tick: [`Predictor::predict`] with this tick's input (it steps at
/// once and returns the input's sequence number; send the input with it, and
/// the last few again, [`Predictor::inputs`]). Per authoritative snapshot:
/// [`Predictor::reconcile`] with the last sequence the server applied and the
/// entity's server state. Per drawn frame: [`Predictor::render_pose`].
#[derive(Debug, Clone)]
pub struct Predictor<S: SyncSim> {
    state: S::State,
    next_seq: Tick,
    inputs: InputHistory<S::Input>,
    prev: S::Pose,
    cur: S::Pose,
    smooth: ErrorSmoother<S::Pose>,
    acked: Option<Tick>,
    cfg: SyncConfig,
    stats: PredictStats,
}

impl<S: SyncSim> Predictor<S> {
    /// Start from `state` (the entity's state when the session began); the
    /// first input gets sequence `first_seq`.
    pub fn new(sim: &S, state: S::State, first_seq: Tick, cfg: SyncConfig) -> Self {
        let pose = sim.pose(&state);
        Self {
            state,
            next_seq: first_seq,
            inputs: InputHistory::new(cfg.input_history),
            prev: pose.clone(),
            cur: pose,
            smooth: ErrorSmoother::new(cfg.smooth_rate, cfg.snap_threshold),
            acked: None,
            cfg,
            stats: PredictStats::default(),
        }
    }

    pub fn config(&self) -> &SyncConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: SyncConfig) {
        if cfg.input_history != self.cfg.input_history {
            self.inputs.set_capacity(cfg.input_history);
        }
        self.smooth.set(cfg.smooth_rate, cfg.snap_threshold);
        self.cfg = cfg;
    }

    /// One fixed tick: store `input` under the next sequence number and step
    /// the predicted state with it now, so the entity answers the input on
    /// this very frame. Returns the sequence number.
    pub fn predict(&mut self, sim: &S, input: S::Input) -> Tick {
        let seq = self.next_seq;
        sim.step(&mut self.state, &input);
        self.inputs.push(seq, input);
        self.prev = core::mem::replace(&mut self.cur, sim.pose(&self.state));
        self.next_seq = seq.wrapping_add(1);
        self.stats.predicted += 1;
        seq
    }

    /// An authoritative snapshot: the server has applied this client's
    /// inputs up to `ack` (`None`: none yet) and its state for the entity is
    /// `server`. Rewinds to it, replays every input after `ack`, and turns the
    /// difference into a render offset that decays (or a cut, over the snap
    /// threshold). A snapshot acknowledging less than one already applied
    /// (reordered) is ignored.
    pub fn reconcile(&mut self, sim: &S, ack: Option<Tick>, server: S::State) -> Correction {
        if let Some(old) = self.acked
            && ack.is_none_or(|n| n < old)
        {
            self.stats.stale += 1;
            return Correction {
                stale: true,
                ..Correction::default()
            };
        }
        if let Some(a) = ack {
            self.inputs.ack(a);
            self.acked = Some(a);
        }
        self.state = server;
        let mut replayed = 0u32;
        for (_, input) in self.inputs.iter() {
            sim.step(&mut self.state, input);
            replayed = replayed.saturating_add(1);
        }
        let fresh = sim.pose(&self.state);
        let err = S::Pose::delta(&self.cur, &fresh);
        let size = S::Pose::magnitude(&err);
        let snapped = if size > SETTLED {
            let snapped = self.smooth.add(&err);
            if snapped {
                // Cut: no blend across the jump either.
                self.prev = fresh.clone();
            } else {
                // Shift the tick blend with the correction, so the drawn pose
                // is unchanged this frame and only the offset carries it.
                self.prev = S::Pose::shift(&self.prev, &S::Pose::scaled(&err, -1.0));
            }
            self.stats.corrections += 1;
            self.stats.max_error = self.stats.max_error.max(size);
            snapped
        } else {
            false
        };
        if snapped {
            self.stats.snaps += 1;
        }
        self.cur = fresh;
        self.stats.reconciles += 1;
        self.stats.replayed += u64::from(replayed);
        Correction {
            error: size,
            snapped,
            replayed,
            stale: false,
        }
    }

    /// The pose to draw this frame: the predicted pose between the last two
    /// ticks at `alpha` (the engine's render alpha), plus what is left of any
    /// correction. Call once per drawn frame with the frame's `dt` seconds.
    pub fn render_pose(&mut self, alpha: f32, dt: f32) -> S::Pose {
        self.smooth.decay(dt);
        let p = S::Pose::interpolate(&self.prev, &self.cur, alpha.clamp(0.0, 1.0));
        self.smooth.apply(&p)
    }

    /// The pose to draw without advancing the smoothing (a second camera, a
    /// minimap).
    pub fn peek_pose(&self, alpha: f32) -> S::Pose {
        let p = S::Pose::interpolate(&self.prev, &self.cur, alpha.clamp(0.0, 1.0));
        self.smooth.apply(&p)
    }

    /// The predicted state (gameplay reads on the client: the HUD's speed).
    pub fn state(&self) -> &S::State {
        &self.state
    }

    /// The predicted pose at the newest tick, without smoothing.
    pub fn pose(&self) -> &S::Pose {
        &self.cur
    }

    /// The sequence the next input will get.
    pub fn next_seq(&self) -> Tick {
        self.next_seq
    }

    /// The newest sequence the server has acknowledged.
    pub fn acked(&self) -> Option<Tick> {
        self.acked
    }

    /// Unacknowledged inputs, to send (with [`InputHistory::newest`] for
    /// redundancy).
    pub fn inputs(&self) -> &InputHistory<S::Input> {
        &self.inputs
    }

    /// The render offset still being eased away.
    pub fn smoothing(&self) -> &ErrorSmoother<S::Pose> {
        &self.smooth
    }

    pub fn stats(&self) -> PredictStats {
        self.stats
    }

    /// Start over from `state` (a respawn the client itself knows of, a new
    /// round): no smoothing, unacknowledged inputs kept.
    pub fn reset(&mut self, sim: &S, state: S::State) {
        let pose = sim.pose(&state);
        self.state = state;
        self.prev = pose.clone();
        self.cur = pose;
        self.smooth.clear();
    }
}

/// How a remote entity was placed by [`SnapshotBuffer::sample`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncSampled {
    /// Nothing buffered yet.
    Empty,
    /// The render time is before the oldest snapshot: its poses.
    Early,
    /// Blended between two snapshots.
    Interpolated,
    /// Past the newest snapshot, extrapolated along the last motion.
    Extrapolated,
    /// Past the newest snapshot by more than the extrapolation cap: held at
    /// the cap.
    Capped,
}

impl SyncSampled {
    pub fn has_poses(self) -> bool {
        self != SyncSampled::Empty
    }
}

/// Snapshot buffer totals.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BufferStats {
    pub snapshots: u64,
    /// Snapshots that arrived after the render time had passed them.
    pub late: u64,
    /// Older or duplicate snapshots dropped.
    pub dropped: u64,
    pub interpolated_frames: u64,
    pub extrapolated_frames: u64,
    pub capped_frames: u64,
    /// Render clock cuts (the server clock jumped: a pause, a reconnect).
    pub clock_resyncs: u64,
    /// Remote corrections cut over the snap threshold.
    pub snaps: u64,
    /// Current render delay, seconds.
    pub delay_s: f64,
    /// Measured arrival jitter, seconds.
    pub jitter_s: f64,
}

struct Frame<P> {
    t: f64,
    poses: Vec<(NetId, P)>,
}

struct Drawn<P> {
    id: NetId,
    /// The raw (unsmoothed) pose last drawn, at the last render time.
    raw: P,
    smooth: ErrorSmoother<P>,
    seen: bool,
}

/// Render clock: how far it may run fast or slow to reach its target (10 %).
const CLOCK_SLEW: f64 = 0.10;
/// Render clock: a gap this big cuts instead of slewing.
const CLOCK_CUT_S: f64 = 0.25;
/// Weights of the clock and jitter estimators.
const CLOCK_GAIN: f64 = 0.05;
const JITTER_GAIN: f64 = 1.0 / 16.0;
const INTERVAL_GAIN: f64 = 0.1;

/// Snapshot interpolation for remote entities, with an adaptive render delay
/// and bounded extrapolation.
///
/// Per snapshot: [`SnapshotBuffer::push`] with its server time, the local
/// time it arrived and the entities' poses. Per drawn frame:
/// [`SnapshotBuffer::advance`] with the local time and the frame's `dt`, then
/// [`SnapshotBuffer::sample`]. Times are seconds on two clocks: the server's
/// (its tick times the step) and the client's own (its tick count times the
/// step plus the frame's alpha); the buffer learns the offset between them
/// and the arrival jitter itself.
pub struct SnapshotBuffer<P> {
    frames: VecDeque<Frame<P>>,
    cap: usize,
    spare: Vec<Vec<(NetId, P)>>,
    /// Estimated `server time - local arrival time` of a typical snapshot.
    clock: Option<f64>,
    jitter: f64,
    interval: Option<f64>,
    delay: f64,
    render_t: Option<f64>,
    last_t: Option<f64>,
    dirty: bool,
    drawn: Vec<Drawn<P>>,
    cfg: SyncConfig,
    stats: BufferStats,
}

impl<P: Interpolate + Offset> SnapshotBuffer<P> {
    /// Keeps the newest `cap` snapshots (at least 2; 32 is over a second at
    /// 20 Hz).
    pub fn new(cap: usize, cfg: SyncConfig) -> Self {
        Self {
            frames: VecDeque::with_capacity(cap.clamp(2, 256)),
            cap: cap.max(2),
            spare: Vec::new(),
            clock: None,
            jitter: 0.0,
            interval: None,
            delay: cfg.interp_delay_s,
            render_t: None,
            last_t: None,
            dirty: false,
            drawn: Vec::new(),
            cfg,
            stats: BufferStats::default(),
        }
    }

    pub fn config(&self) -> &SyncConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: SyncConfig) {
        for d in &mut self.drawn {
            d.smooth.set(cfg.smooth_rate, cfg.snap_threshold);
        }
        self.cfg = cfg;
    }

    pub fn stats(&self) -> BufferStats {
        let mut s = self.stats;
        s.delay_s = self.delay;
        s.jitter_s = self.jitter;
        s
    }

    /// The server time remote entities are drawn at (after
    /// [`SnapshotBuffer::advance`]).
    pub fn render_time(&self) -> Option<f64> {
        self.render_t
    }

    /// The render delay now in use, seconds.
    pub fn delay(&self) -> f64 {
        self.delay
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn newest_time(&self) -> Option<f64> {
        self.frames.back().map(|f| f.t)
    }

    /// Forget everything (a new session).
    pub fn clear(&mut self) {
        while let Some(f) = self.frames.pop_front() {
            self.recycle(f.poses);
        }
        self.clock = None;
        self.jitter = 0.0;
        self.interval = None;
        self.delay = self.cfg.interp_delay_s;
        self.render_t = None;
        self.last_t = None;
        self.drawn.clear();
    }

    fn recycle(&mut self, mut v: Vec<(NetId, P)>) {
        v.clear();
        if self.spare.len() < 4 {
            self.spare.push(v);
        }
    }

    /// The delay the jitter and send rate ask for.
    fn target_delay(&self) -> f64 {
        let need = self.interval.unwrap_or(0.0) + f64::from(self.cfg.jitter_scale) * self.jitter;
        let base = self.cfg.interp_delay_s;
        if self.cfg.jitter_scale <= 0.0 {
            return base;
        }
        need.max(base).min(self.cfg.interp_delay_max_s.max(base))
    }

    /// A snapshot taken at server time `server_t` arrived at local time
    /// `local_t`, with these entity poses (any order). Reordered snapshots
    /// are slotted in by time; one at a time already buffered replaces it.
    pub fn push(&mut self, server_t: f64, local_t: f64, poses: &[(NetId, P)]) {
        self.stats.snapshots += 1;
        if self.frames.front().is_some_and(|f| server_t < f.t) && self.frames.len() >= self.cap {
            self.stats.dropped += 1;
            return;
        }
        // Clock and jitter: RFC 3550's interarrival jitter on the offset.
        let o = server_t - local_t;
        match self.clock {
            None => self.clock = Some(o),
            Some(c) => {
                let d = o - c;
                self.jitter += (d.abs() - self.jitter) * JITTER_GAIN;
                self.clock = Some(c + d * CLOCK_GAIN);
            }
        }
        if let Some(newest) = self.newest_time()
            && server_t > newest
        {
            let gap = server_t - newest;
            self.interval = Some(match self.interval {
                None => gap,
                Some(i) => i + (gap - i) * INTERVAL_GAIN,
            });
        }
        if self.render_t.is_some_and(|r| server_t <= r) {
            self.stats.late += 1;
        }
        let at = self.frames.partition_point(|f| f.t < server_t);
        if self.frames.get(at).is_some_and(|f| f.t == server_t) {
            let mut v = self.spare.pop().unwrap_or_default();
            v.extend_from_slice(poses);
            v.sort_by_key(|p| p.0);
            if let Some(f) = self.frames.get_mut(at) {
                let old = core::mem::replace(&mut f.poses, v);
                self.recycle(old);
            }
            self.dirty = true;
            return;
        }
        let mut v = self.spare.pop().unwrap_or_default();
        v.extend_from_slice(poses);
        v.sort_by_key(|p| p.0);
        self.frames.insert(at, Frame { t: server_t, poses: v });
        while self.frames.len() > self.cap {
            if let Some(f) = self.frames.pop_front() {
                self.recycle(f.poses);
            }
        }
        self.dirty = true;
    }

    /// One drawn frame at local time `local_t`, `dt` seconds after the last.
    /// The render clock runs at the frame rate and is slewed (at most 10 %)
    /// toward `local_t + clock offset - delay`, so it never runs backwards
    /// or jumps; the delay itself moves toward what the jitter asks for at
    /// the same slew. A gap over 250 ms (a pause, a reconnect) cuts.
    pub fn advance(&mut self, local_t: f64, dt: f64) {
        let dt = dt.max(0.0);
        let target_delay = self.target_delay();
        let step = CLOCK_SLEW * dt;
        self.delay += (target_delay - self.delay).clamp(-step, step);
        let Some(clock) = self.clock else { return };
        let target = local_t + clock - self.delay;
        self.last_t = self.render_t;
        self.render_t = Some(match self.render_t {
            None => target,
            Some(r) => {
                let next = r + dt;
                let err = target - next;
                if err.abs() > CLOCK_CUT_S {
                    self.stats.clock_resyncs += 1;
                    target
                } else {
                    next + err.clamp(-step, step)
                }
            }
        });
        for d in &mut self.drawn {
            d.smooth.decay(dt as f32);
        }
        // Frames no render time can reach again: keep one before it.
        if let Some(r) = self.render_t {
            while self.frames.len() > 2 && self.frames.get(1).is_some_and(|f| f.t <= r) {
                if let Some(f) = self.frames.pop_front() {
                    self.recycle(f.poses);
                }
            }
        }
    }

    /// Where entity `id` is at server time `t`, raw (no smoothing).
    fn raw_at(&self, id: NetId, t: f64) -> Option<(P, SyncSampled)> {
        let idx = self.frames.partition_point(|f| f.t <= t);
        let find = |f: &Frame<P>| {
            f.poses
                .binary_search_by_key(&id, |p| p.0)
                .ok()
                .and_then(|i| f.poses.get(i))
                .map(|p| p.1.clone())
        };
        if idx == 0 {
            let f = self.frames.front()?;
            return find(f).map(|p| (p, SyncSampled::Early));
        }
        let a = self.frames.get(idx - 1)?;
        let pa = find(a)?;
        if let Some(b) = self.frames.get(idx) {
            return Some(match find(b) {
                Some(pb) => {
                    let span = (b.t - a.t).max(1e-9);
                    let k = ((t - a.t) / span).clamp(0.0, 1.0) as f32;
                    (P::interpolate(&pa, &pb, k), SyncSampled::Interpolated)
                }
                // Gone by the newer snapshot: its last pose until then.
                None => (pa, SyncSampled::Interpolated),
            });
        }
        // Past the newest snapshot.
        let over = t - a.t;
        let cap = self.cfg.extrapolate_max_s.max(0.0);
        let mode = if over > cap { SyncSampled::Capped } else { SyncSampled::Extrapolated };
        // Extrapolate along the motion from the newest earlier snapshot that
        // has this entity.
        let prev = self.frames.iter().rev().skip(1).find_map(|f| find(f).map(|p| (f.t, p)));
        let Some((tp, pp)) = prev else {
            return Some((pa, mode));
        };
        let span = (a.t - tp).max(1e-9);
        let k = (over.min(cap) / span) as f32;
        let vel = P::delta(&pa, &pp);
        Some((P::shift(&pa, &P::scaled(&vel, k)), mode))
    }

    /// Every remote entity's pose for this frame into `out` (cleared first),
    /// sorted by [`NetId`], smoothed. Entities in the snapshot bracketing the
    /// render time are drawn; one that has left is dropped once the render
    /// time passes its last snapshot. Returns how the frame was placed (the
    /// newest-snapshot case decides between extrapolated and capped).
    pub fn sample(&mut self, out: &mut Vec<(NetId, P)>) -> SyncSampled {
        out.clear();
        let Some(t) = self.render_t else {
            return if self.frames.is_empty() { SyncSampled::Empty } else { SyncSampled::Early };
        };
        if self.frames.is_empty() {
            return SyncSampled::Empty;
        }
        // The frame whose entities are drawn: the one at or before t.
        let idx = self.frames.partition_point(|f| f.t <= t).saturating_sub(1);
        let mut ids: Vec<NetId> = Vec::new();
        if let Some(f) = self.frames.get(idx) {
            ids.extend(f.poses.iter().map(|p| p.0));
        }
        for d in &mut self.drawn {
            d.seen = false;
        }
        let dirty = core::mem::take(&mut self.dirty);
        let last_t = self.last_t;
        let mut mode = SyncSampled::Interpolated;
        for id in ids {
            let Some((raw, m)) = self.raw_at(id, t) else { continue };
            if m != SyncSampled::Interpolated {
                mode = m;
            }
            // A snapshot that arrived since the last frame may change where
            // the entity was at the last render time (it was extrapolated, or
            // a reordered snapshot slotted in): ease that difference away.
            let then = if dirty { last_t.and_then(|lt| self.raw_at(id, lt)).map(|r| r.0) } else { None };
            let pos = self.drawn.binary_search_by_key(&id, |d| d.id);
            let d = match pos {
                Ok(i) => self.drawn.get_mut(i),
                Err(i) => {
                    self.drawn.insert(
                        i,
                        Drawn {
                            id,
                            raw: raw.clone(),
                            smooth: ErrorSmoother::new(self.cfg.smooth_rate, self.cfg.snap_threshold),
                            seen: false,
                        },
                    );
                    self.drawn.get_mut(i)
                }
            };
            let Some(d) = d else { continue };
            if let Some(then) = then {
                let err = P::delta(&d.raw, &then);
                if P::magnitude(&err) > SETTLED && d.smooth.add(&err) {
                    self.stats.snaps += 1;
                }
            }
            d.raw = raw.clone();
            d.seen = true;
            out.push((id, d.smooth.apply(&raw)));
        }
        self.drawn.retain(|d| d.seen);
        match mode {
            SyncSampled::Interpolated => self.stats.interpolated_frames += 1,
            SyncSampled::Extrapolated => self.stats.extrapolated_frames += 1,
            SyncSampled::Capped => self.stats.capped_frames += 1,
            _ => {}
        }
        mode
    }
}

/// Server or client seconds at a fixed tick rate (`tick / hz`).
pub fn tick_seconds(tick: Tick, hz: u32) -> f64 {
    f64::from(tick) / f64::from(hz.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1D mover: velocity from the input, position integrates.
    struct Mover;
    #[derive(Clone, Debug, PartialEq)]
    struct St {
        x: f32,
    }
    impl SyncSim for Mover {
        type State = St;
        type Input = f32;
        type Pose = f32;
        fn step(&self, s: &mut St, v: &f32) {
            s.x += v / 60.0;
        }
        fn pose(&self, s: &St) -> f32 {
            s.x
        }
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn input_history_keeps_unacked_in_order_and_caps() {
        let mut h = InputHistory::new(4);
        for s in 0..6u32 {
            h.push(s, s as f32);
        }
        assert_eq!(h.len(), 4);
        assert_eq!(h.dropped(), 2);
        assert_eq!(h.oldest_seq(), Some(2));
        h.push(3, 99.0); // older than newest: ignored
        assert_eq!(h.newest_seq(), Some(5));
        h.ack(3);
        assert_eq!(h.iter().map(|p| p.0).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(h.newest(1).map(|p| p.0).collect::<Vec<_>>(), vec![5]);
    }

    #[test]
    fn predict_answers_the_input_on_the_same_tick() {
        let mut p = Predictor::new(&Mover, St { x: 0.0 }, 0, SyncConfig::default());
        let before = p.render_pose(1.0, 1.0 / 60.0);
        let seq = p.predict(&Mover, 60.0);
        assert_eq!(seq, 0);
        let after = p.render_pose(1.0, 1.0 / 60.0);
        assert!(approx(after - before, 1.0), "moved {}", after - before);
        assert_eq!(p.next_seq(), 1);
    }

    /// Reconciliation replay: the server's state at the ack plus the
    /// unacknowledged inputs replayed equals a straight prediction when the
    /// server agrees, and lands on the corrected path when it does not.
    #[test]
    fn reconcile_rewinds_to_the_server_and_replays_unacked_inputs() {
        let mut p = Predictor::new(&Mover, St { x: 0.0 }, 10, SyncConfig::default());
        let inputs = [6.0, 12.0, -6.0, 30.0, 0.0, 18.0];
        for v in inputs {
            p.predict(&Mover, v);
        }
        let straight = p.state().x;
        // The server applied seq 10..=12 (first three inputs) and agrees.
        let mut srv = St { x: 0.0 };
        for v in &inputs[..3] {
            Mover.step(&mut srv, v);
        }
        let c = p.reconcile(&Mover, Some(12), srv.clone());
        assert_eq!(c.replayed, 3);
        assert!(!c.stale && !c.snapped);
        assert!(approx(c.error, 0.0));
        assert!(approx(p.state().x, straight));
        assert_eq!(p.inputs().len(), 3);
        // Now the server was bumped 0.5 ahead at seq 13.
        let mut srv2 = srv;
        Mover.step(&mut srv2, &inputs[3]);
        srv2.x += 0.5;
        let c = p.reconcile(&Mover, Some(13), srv2.clone());
        assert_eq!(c.replayed, 2);
        let mut want = srv2;
        for v in &inputs[4..] {
            Mover.step(&mut want, v);
        }
        assert!(approx(p.state().x, want.x));
        assert!(approx(c.error, 0.5));
        assert!(!c.snapped);
        // A reordered older snapshot changes nothing.
        let c = p.reconcile(&Mover, Some(11), St { x: -100.0 });
        assert!(c.stale);
        assert!(approx(p.state().x, want.x));
    }

    /// A correction does not move the drawn pose on its frame; the offset
    /// then decays by e^(-rate dt) a frame and settles.
    #[test]
    fn error_smoothing_decays_and_settles() {
        let cfg = SyncConfig::default();
        let mut p = Predictor::new(&Mover, St { x: 0.0 }, 0, cfg);
        p.predict(&Mover, 0.0);
        let dt = 1.0 / 60.0;
        let drawn = p.peek_pose(1.0);
        let c = p.reconcile(&Mover, Some(0), St { x: 1.0 });
        assert!(approx(c.error, 1.0));
        assert!(approx(p.peek_pose(1.0), drawn), "no jump on the correction frame");
        let mut last = p.smoothing().remaining();
        assert!(approx(last, 1.0));
        let k = (-cfg.smooth_rate * dt).exp();
        for _ in 0..10 {
            p.render_pose(1.0, dt);
            let now = p.smoothing().remaining();
            assert!(approx(now, last * k), "{now} vs {}", last * k);
            last = now;
        }
        for _ in 0..120 {
            p.render_pose(1.0, dt);
        }
        assert!(p.smoothing().is_settled());
        assert!(approx(p.render_pose(1.0, dt), 1.0));
    }

    #[test]
    fn errors_over_the_snap_threshold_cut() {
        let cfg = SyncConfig { snap_threshold: 2.0, ..SyncConfig::default() };
        let mut p = Predictor::new(&Mover, St { x: 0.0 }, 0, cfg);
        p.predict(&Mover, 0.0);
        let c = p.reconcile(&Mover, Some(0), St { x: 5.0 });
        assert!(c.snapped);
        assert!(p.smoothing().is_settled());
        assert!(approx(p.render_pose(0.0, 0.016), 5.0), "no blend from the old tick");
        assert_eq!(p.stats().snaps, 1);
        // Small errors adding up past the threshold cut too.
        let mut s = ErrorSmoother::<f32>::new(10.0, 1.0);
        assert!(!s.add(&0.6));
        assert!(s.add(&0.6));
        assert!(s.is_settled());
        // Rate 0 turns smoothing off.
        let mut s = ErrorSmoother::<f32>::new(0.0, 1.0);
        assert!(s.add(&0.1));
    }

    #[test]
    fn angles_correct_the_short_way() {
        use core::f32::consts::PI;
        let d = Angle::delta(&Angle(PI - 0.1), &Angle(-PI + 0.1));
        assert!(approx(d.0, -0.2), "{}", d.0);
        let pose = ([1.0f32, 0.0], Angle(0.0));
        let off = <([f32; 2], Angle)>::delta(&pose, &([0.0, 0.0], Angle(3.0)));
        assert!(approx(<([f32; 2], Angle)>::magnitude(&off), 1.0), "position decides");
    }

    fn buf() -> SnapshotBuffer<f32> {
        SnapshotBuffer::new(32, SyncConfig { jitter_scale: 0.0, ..SyncConfig::default() })
    }

    /// Interpolation between two snapshots: halfway in time is halfway in
    /// pose, and the render time sits the delay behind the server.
    #[test]
    fn interpolates_between_two_snapshots() {
        let mut b = buf();
        let id = NetId(7);
        // Snapshots at server 1.00 and 1.05 arrive with no delay.
        b.push(1.00, 1.00, &[(id, 0.0)]);
        b.push(1.05, 1.05, &[(id, 10.0)]);
        // Render at local 1.125: server 1.125 - 0.100 = 1.025, halfway.
        b.advance(1.125, 0.0);
        let mut out = Vec::new();
        assert_eq!(b.sample(&mut out), SyncSampled::Interpolated);
        assert_eq!(out.len(), 1);
        let (oid, x) = out[0];
        assert_eq!(oid, id);
        assert!(approx(x, 5.0), "{x}");
        assert!((b.render_time().unwrap_or(0.0) - 1.025).abs() < 1e-9);
    }

    /// Past the newest snapshot the entity moves on along its last motion,
    /// but no further than the cap; then it holds.
    #[test]
    fn extrapolation_is_capped() {
        let mut b = buf();
        let id = NetId(1);
        b.push(0.0, 0.0, &[(id, 0.0)]);
        b.push(0.05, 0.05, &[(id, 1.0)]); // 20 units a second
        let mut out = Vec::new();
        // Render time 0.05 + 0.1 = 0.15: 0.1 s past the newest.
        b.advance(0.25, 0.0);
        assert_eq!(b.sample(&mut out), SyncSampled::Extrapolated);
        assert!(approx(out[0].1, 3.0), "{}", out[0].1);
        // 0.5 s past: capped at 0.2 s of motion.
        let mut b2 = buf();
        b2.push(0.0, 0.0, &[(id, 0.0)]);
        b2.push(0.05, 0.05, &[(id, 1.0)]);
        b2.advance(0.65, 0.0);
        assert_eq!(b2.sample(&mut out), SyncSampled::Capped);
        assert!(approx(out[0].1, 5.0), "{}", out[0].1);
        // Cap 0: holds the newest pose.
        let mut b3 = SnapshotBuffer::new(
            8,
            SyncConfig { extrapolate_max_s: 0.0, jitter_scale: 0.0, ..SyncConfig::default() },
        );
        b3.push(0.0, 0.0, &[(id, 0.0)]);
        b3.push(0.05, 0.05, &[(id, 1.0)]);
        b3.advance(0.3, 0.0);
        b3.sample(&mut out);
        assert!(approx(out[0].1, 1.0));
    }

    /// After extrapolating, a late snapshot that disagrees does not jump the
    /// entity: the difference is eased away.
    #[test]
    fn late_snapshot_after_extrapolation_is_eased_in() {
        let mut b = buf();
        let id = NetId(1);
        let mut out = Vec::new();
        b.push(0.0, 0.0, &[(id, 0.0)]);
        b.push(0.05, 0.05, &[(id, 1.0)]);
        let dt = 1.0 / 60.0;
        let mut t = 0.15;
        b.advance(t, 0.0);
        b.sample(&mut out);
        let mut last = out[0].1;
        // It stopped at 1.0 for real; the snapshot at 0.10 arrives late.
        for _ in 0..3 {
            t += dt;
            b.advance(t, dt);
            b.sample(&mut out);
            last = out[0].1;
        }
        b.push(0.10, t, &[(id, 1.0)]);
        b.push(0.15, t, &[(id, 1.0)]);
        t += dt;
        b.advance(t, dt);
        b.sample(&mut out);
        let step = (out[0].1 - last).abs();
        assert!(step < 0.4, "jumped {step}");
        for _ in 0..120 {
            t += dt;
            b.advance(t, dt);
            b.sample(&mut out);
        }
        assert!(approx(out[0].1, 1.0), "{}", out[0].1);
    }

    /// The render delay grows with measured jitter and stays within its
    /// limits; with jitter_scale 0 it stays at the base delay.
    #[test]
    fn render_delay_adapts_to_jitter() {
        let mut b = SnapshotBuffer::<f32>::new(64, SyncConfig::default());
        let id = NetId(1);
        let mut local = 0.0;
        for i in 0..200u32 {
            let s = f64::from(i) * 0.05;
            // Arrival delay alternates 20 ms and 120 ms: heavy jitter.
            let late = if i % 2 == 0 { 0.02 } else { 0.12 };
            b.push(s, s + late, &[(id, i as f32)]);
            local = s + late;
            for _ in 0..3 {
                local += 1.0 / 60.0;
                b.advance(local, 1.0 / 60.0);
            }
        }
        let _ = local;
        let st = b.stats();
        assert!(st.jitter_s > 0.03, "jitter {}", st.jitter_s);
        assert!(st.delay_s > INTERP_DELAY_DEFAULT_S + 0.02, "delay {}", st.delay_s);
        assert!(st.delay_s <= INTERP_DELAY_MAX_S + 1e-9);
        let mut f = buf();
        for i in 0..50u32 {
            let s = f64::from(i) * 0.05;
            f.push(s, s + if i % 2 == 0 { 0.0 } else { 0.1 }, &[(id, 0.0)]);
            f.advance(s, 0.05);
        }
        assert!((f.delay() - INTERP_DELAY_DEFAULT_S).abs() < 1e-9);
    }

    #[test]
    fn reordered_and_departed_entities() {
        let mut b = buf();
        let (a, c) = (NetId(1), NetId(2));
        b.push(0.10, 0.10, &[(a, 2.0)]);
        b.push(0.05, 0.10, &[(c, 9.0), (a, 1.0)]); // reordered, both
        b.push(0.0, 0.10, &[(a, 0.0), (c, 9.0)]);
        b.advance(0.175, 0.0); // render about 0.07 (the late arrivals moved the clock)
        let r = b.render_time().expect("render time");
        assert!(r > 0.05 && r < 0.10, "{r}");
        let mut out = Vec::new();
        b.sample(&mut out);
        // `a` blends 1 -> 2; `c` left at 0.10 and keeps its pose until then.
        assert_eq!(out.len(), 2);
        let want = 1.0 + ((r - 0.05) / 0.05) as f32;
        assert!(approx(out[0].1, want), "{} vs {want}", out[0].1);
        assert!(approx(out[1].1, 9.0));
        b.advance(0.215, 0.04); // render 0.115: past `c`'s last snapshot
        b.sample(&mut out);
        assert_eq!(out.iter().map(|p| p.0).collect::<Vec<_>>(), vec![a]);
    }

    #[test]
    fn params_reach_the_config() {
        use evani_core::App;
        use evani_core::params::{ParamRegistry, ParamValue};
        let mut app = App::new("t").with_module(crate::NetModule::new());
        app.run_headless(1, 16.0);
        assert_eq!(SyncConfig::from_world(&app.world), SyncConfig::default());
        {
            let mut reg = app.world.res_mut::<ParamRegistry>();
            for (name, v) in [
                ("net.interp_delay_ms", 150.0),
                ("net.sync.smooth_rate", 20.0),
                ("net.sync.snap_threshold", 1.5),
                ("net.sync.extrapolate_max_ms", 80.0),
                ("net.sync.jitter_scale", 0.0),
            ] {
                let id = reg.id(name).expect("registered");
                assert!(reg.set(id, ParamValue::F32(v)).is_ok(), "{name}");
            }
        }
        let c = SyncConfig::from_world(&app.world);
        assert!((c.interp_delay_s - 0.15).abs() < 1e-6);
        assert_eq!((c.smooth_rate, c.snap_threshold, c.jitter_scale), (20.0, 1.5, 0.0));
        assert!((c.extrapolate_max_s - 0.08).abs() < 1e-6);
    }
}
