//! Entity interpolation and the pose history that lag compensation rewinds.
//!
//! One structure serves both: a [`PoseHistory`] holds the poses of every
//! networked entity at the snapshot ticks. A client draws remote entities by
//! sampling its history a little in the past (`net.interp_delay_ms`); the
//! server keeps the same frames and, when a shot arrives, samples them at the
//! time the shooter was looking at. Both sides blend the same frames with the
//! same integer-derived weights, so what a player saw is what the server
//! tests against.

use std::collections::VecDeque;

use crate::types::{NetId, Tick, TickTime};

/// How a pose blends between two snapshots. `t` is in `[0, 1]`.
pub trait Interpolate: Clone {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self;
}

impl Interpolate for f32 {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl<const N: usize> Interpolate for [f32; N] {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
    }
}

impl<A: Interpolate, B: Interpolate> Interpolate for (A, B) {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        (A::interpolate(&a.0, &b.0, t), B::interpolate(&a.1, &b.1, t))
    }
}

/// Blend two angles in radians the short way round.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    use core::f32::consts::{PI, TAU};
    let mut d = (b - a) % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    a + d * t
}

/// For discrete fields (a weapon id, a crouch flag): `a` until `b` is reached.
pub fn hold<T: Clone>(a: &T, b: &T, t: f32) -> T {
    if t < 1.0 { a.clone() } else { b.clone() }
}

/// What [`PoseHistory::sample`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampled {
    /// Nothing recorded yet; the output is empty.
    Empty,
    /// The time was before the oldest frame: the oldest poses.
    Oldest,
    /// The time was at or after the newest frame: the newest poses, held
    /// (no extrapolation).
    Newest,
    /// Blended between the two frames around the time.
    Between,
}

impl Sampled {
    pub fn has_poses(self) -> bool {
        self != Sampled::Empty
    }
}

/// Poses of every networked entity at a run of ticks, oldest first.
#[derive(Debug, Clone)]
pub struct PoseHistory<P> {
    frames: VecDeque<(Tick, Vec<(NetId, P)>)>,
    cap: usize,
    spare: Vec<Vec<(NetId, P)>>,
}

impl<P: Interpolate> PoseHistory<P> {
    /// Keeps the newest `cap` frames (at least 2).
    pub fn new(cap: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(cap.max(2)),
            cap: cap.max(2),
            spare: Vec::new(),
        }
    }

    pub fn set_capacity(&mut self, cap: usize) {
        self.cap = cap.max(2);
        while self.frames.len() > self.cap {
            self.drop_oldest();
        }
    }

    pub fn clear(&mut self) {
        while !self.frames.is_empty() {
            self.drop_oldest();
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn newest_tick(&self) -> Option<Tick> {
        self.frames.back().map(|f| f.0)
    }

    pub fn oldest_tick(&self) -> Option<Tick> {
        self.frames.front().map(|f| f.0)
    }

    /// The poses recorded at exactly `tick`.
    pub fn frame(&self, tick: Tick) -> Option<&[(NetId, P)]> {
        self.frames
            .iter()
            .find(|f| f.0 == tick)
            .map(|f| f.1.as_slice())
    }

    fn drop_oldest(&mut self) {
        if let Some((_, mut v)) = self.frames.pop_front() {
            v.clear();
            if self.spare.len() < 4 {
                self.spare.push(v);
            }
        }
    }

    /// Record the poses at `tick`, sorted by [`NetId`] (they are sorted here
    /// if not). A tick older than the newest frame is ignored (a late,
    /// reordered packet); the same tick replaces that frame.
    pub fn push(&mut self, tick: Tick, poses: &[(NetId, P)]) {
        if let Some(newest) = self.newest_tick() {
            if tick < newest {
                return;
            }
            if tick == newest {
                if let Some(back) = self.frames.back_mut() {
                    back.1.clear();
                    back.1.extend_from_slice(poses);
                    back.1.sort_by_key(|p| p.0);
                }
                return;
            }
        }
        while self.frames.len() >= self.cap {
            self.drop_oldest();
        }
        let mut v = self.spare.pop().unwrap_or_default();
        v.extend_from_slice(poses);
        v.sort_by_key(|p| p.0);
        self.frames.push_back((tick, v));
    }

    /// The poses at `at` into `out` (cleared first), sorted by [`NetId`].
    /// Between two frames, an entity in both is blended; one only in the
    /// older frame (gone by the newer) keeps its last pose; one only in the
    /// newer frame (not there yet) is left out.
    pub fn sample(&self, at: TickTime, out: &mut Vec<(NetId, P)>) -> Sampled {
        out.clear();
        let idx = self.frames.partition_point(|f| TickTime::at(f.0) <= at);
        if self.frames.is_empty() {
            return Sampled::Empty;
        }
        if idx == 0 {
            if let Some(f) = self.frames.front() {
                out.extend_from_slice(&f.1);
            }
            return Sampled::Oldest;
        }
        let (Some(a), Some(b)) = (self.frames.get(idx - 1), self.frames.get(idx)) else {
            if let Some(f) = self.frames.back() {
                out.extend_from_slice(&f.1);
            }
            return Sampled::Newest;
        };
        let ta = TickTime::at(a.0).0;
        let tb = TickTime::at(b.0).0;
        let span = tb.saturating_sub(ta).max(1);
        let t = (at.0.saturating_sub(ta).min(span) as f64 / span as f64) as f32;
        let (pa, pb) = (&a.1, &b.1);
        let (mut i, mut j) = (0, 0);
        while i < pa.len() {
            let Some((ida, posea)) = pa.get(i) else { break };
            match pb.get(j) {
                Some((idb, poseb)) if idb == ida => {
                    out.push((*ida, P::interpolate(posea, poseb, t)));
                    i += 1;
                    j += 1;
                }
                Some((idb, _)) if idb < ida => j += 1,
                _ => {
                    out.push((*ida, posea.clone()));
                    i += 1;
                }
            }
        }
        Sampled::Between
    }
}
