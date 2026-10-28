//! Sync test: a determinism guard any [`NetGame`] can run in its own tests.
//!
//! The idea comes from GGRS's sync-test session (gschup/ggrs, MIT OR
//! Apache-2.0; idea only, no code and no dependency): step the simulation
//! more than once from the same seed and inputs and compare a state hash
//! after every tick, so the first tick that is not repeatable is found on
//! the developer's machine instead of as a desync in a match.
//!
//! [`SyncTest::run`] steps four copies of the state side by side, with the
//! same inputs each tick:
//!
//! * **reference**: the state stepped as the server steps it;
//! * [`Check::Repeat`]: a second, independent copy stepped the same way
//!   (anything outside the state that the step reads, such as a counter in
//!   the game value, a `HashMap` order or a clock, shows up here);
//! * [`Check::WireRoundTrip`]: a copy that is encoded and decoded before
//!   every step, as a client receives it in a snapshot (a field the
//!   [`Wire`] impl forgets, or reads back differently, shows up here);
//! * [`Check::Rollback`]: every tick, the state from
//!   [`SyncTest::check_distance`] ticks ago is replayed to now with
//!   [`Role::Resim`], as client prediction does after a correction (a step
//!   that changes the state differently on a replay, or hidden state that a
//!   replay does not rewind, shows up here).
//!
//! The first difference stops the run with a [`Divergence`]: the tick, the
//! check, both hashes, the first byte where the encoded states differ and,
//! when the game names the parts of its state ([`SyncTest::parts`]), which
//! part it is.
//!
//! Lag compensation is not exercised: the step gets an empty pose history,
//! so [`StepCtx::rewind`] answers "use the live state".
//!
//! ```ignore
//! let report = SyncTest::new(&game, 7, 600)
//!     .run(&initial, |tick, state, out| my_inputs(tick, state, out))
//!     .unwrap_or_else(|d| panic!("{d}"));
//! ```

use core::fmt;
use std::collections::VecDeque;

use crate::game::{NetGame, PeerInput, StepCtx};
use crate::interp::PoseHistory;
use crate::types::{PeerId, Role, Tick};
use crate::wire::{Reader, Wire, Writer};

/// What the step sees on one tick: every peer's input (any order; sorted by
/// the test), and the peers that joined and left before it.
#[derive(Debug, Clone)]
pub struct TickInputs<I> {
    pub inputs: Vec<PeerInput<I>>,
    pub joined: Vec<PeerId>,
    pub left: Vec<PeerId>,
}

impl<I> Default for TickInputs<I> {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            joined: Vec::new(),
            left: Vec::new(),
        }
    }
}

impl<I> TickInputs<I> {
    pub fn clear(&mut self) {
        self.inputs.clear();
        self.joined.clear();
        self.left.clear();
    }
}

/// Which comparison found a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Two independent runs with the same inputs.
    Repeat,
    /// A run whose state goes through the wire before every step, or a
    /// state whose encoding does not read back to the same bytes.
    WireRoundTrip,
    /// A replay of the last ticks from a saved state, as after a rollback.
    Rollback,
}

impl Check {
    pub fn label(self) -> &'static str {
        match self {
            Check::Repeat => "repeat run",
            Check::WireRoundTrip => "wire round trip",
            Check::Rollback => "rollback replay",
        }
    }

    fn fix(self) -> &'static str {
        match self {
            Check::Repeat => {
                "the step reads something besides the state and its StepCtx: a clock, a global, a counter or cache in the game value, or the order of a HashMap; take randomness from StepCtx::rng and keep every changing value in the state"
            }
            Check::WireRoundTrip => {
                "the state's Wire impl does not write (or read back) every field the step uses; list every field in wire_struct! or the hand-written impl, in the same order"
            }
            Check::Rollback => {
                "a replayed tick (Role::Resim) changes the state differently from the first run, or the step keeps state outside G::State that a rollback cannot rewind; effects may check Role::first_time, the state may not"
            }
        }
    }
}

/// The first tick a sync test found not repeatable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The tick whose step gave different states (they differ at `tick + 1`).
    pub tick: Tick,
    pub check: Check,
    /// The reference run's hash and the checked run's.
    pub expected: u64,
    pub got: u64,
    /// The first byte where the two states' encodings differ (`None` when
    /// the encodings are equal and only a custom checksum differs).
    pub first_byte: Option<usize>,
    /// The first part of the state that differs, when the game names them.
    pub part: Option<&'static str>,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sync test: tick {} is not repeatable ({}): state hash {:#018x} expected {:#018x}",
            self.tick,
            self.check.label(),
            self.got,
            self.expected
        )?;
        if let Some(b) = self.first_byte {
            write!(f, "; encodings first differ at byte {b}")?;
        }
        if let Some(p) = self.part {
            write!(f, "; part `{p}`")?;
        }
        write!(f, ". fix: {}", self.check.fix())
    }
}

/// A finished, clean sync test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Ticks stepped.
    pub ticks: Tick,
    /// The reference run's hash after every tick (`hashes[t]` is the state
    /// at `t + 1`). Compare two reports with [`first_divergence`] to check a
    /// change kept the simulation the same.
    pub hashes: Vec<u64>,
    /// Ticks replayed by the rollback check.
    pub replayed: u64,
}

impl SyncReport {
    /// The hash of the final state.
    pub fn final_hash(&self) -> Option<u64> {
        self.hashes.last().copied()
    }
}

/// The first index where two per-tick hash lists differ, or where the
/// shorter one ends. `None` when they are equal.
pub fn first_divergence(a: &[u64], b: &[u64]) -> Option<usize> {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => Some(i),
        None if a.len() != b.len() => Some(a.len().min(b.len())),
        None => None,
    }
}

/// Names and hashes the parts of a state, for a [`Divergence`] that says
/// which part differs (e.g. `("players", hash)`, `("pickups", hash)`).
pub type PartsFn<G> = fn(&G, &<G as NetGame>::State, &mut Vec<(&'static str, u64)>);

/// A sync test over a game: see the module docs.
pub struct SyncTest<'g, G: NetGame> {
    game: &'g G,
    seed: u64,
    ticks: Tick,
    check_distance: u32,
    parts: Option<PartsFn<G>>,
}

/// One copy of the state under test.
struct Run<S> {
    state: S,
    role: Role,
}

impl<'g, G: NetGame> SyncTest<'g, G> {
    /// `ticks` ticks of `game` under session `seed`, with a rollback replay
    /// of 8 ticks.
    pub fn new(game: &'g G, seed: u64, ticks: Tick) -> Self {
        Self {
            game,
            seed,
            ticks,
            check_distance: 8,
            parts: None,
        }
    }

    /// How many ticks each rollback replay goes back (0 turns the check
    /// off). Client prediction replays about the round trip in ticks.
    pub fn check_distance(mut self, ticks: u32) -> Self {
        self.check_distance = ticks;
        self
    }

    /// Name the parts of the state, so a divergence says which one differs.
    pub fn parts(mut self, f: PartsFn<G>) -> Self {
        self.parts = Some(f);
        self
    }

    /// Run from `initial`. `inputs` fills each tick's inputs, joins and
    /// leaves; it gets the reference state at that tick (so scripted bots
    /// can react to it) and must depend only on the tick and that state.
    pub fn run(
        &self,
        initial: &G::State,
        mut inputs: impl FnMut(Tick, &G::State, &mut TickInputs<G::Input>),
    ) -> Result<SyncReport, Divergence> {
        let history: PoseHistory<G::Pose> = PoseHistory::new(1);
        let mut reference = Run {
            state: initial.clone(),
            role: Role::Server,
        };
        let mut repeat = Run {
            state: initial.clone(),
            role: Role::Server,
        };
        let mut wire = Run {
            state: initial.clone(),
            role: Role::Server,
        };
        let distance = usize::try_from(self.check_distance).unwrap_or(usize::MAX);
        // The reference state before each of the last `distance` ticks, and
        // those ticks' inputs.
        let mut saved: VecDeque<(G::State, TickInputs<G::Input>)> = VecDeque::new();
        let mut scratch = Writer::with_capacity(1024);
        let mut a = Writer::with_capacity(1024);
        let mut b = Writer::with_capacity(1024);
        let mut report = SyncReport {
            ticks: 0,
            hashes: Vec::with_capacity(usize::try_from(self.ticks).unwrap_or(0)),
            replayed: 0,
        };

        for t in 0..self.ticks {
            let mut tick_in = TickInputs::default();
            inputs(t, &reference.state, &mut tick_in);
            tick_in.inputs.sort_by_key(|p| p.peer);
            tick_in.joined.sort();
            tick_in.left.sort();

            if distance > 0 {
                saved.push_back((reference.state.clone(), tick_in.clone()));
                while saved.len() > distance {
                    saved.pop_front();
                }
            }

            // The wire copy arrives the way a snapshot does.
            a.clear();
            wire.state.write(&mut a);
            match G::State::read(&mut Reader::new(a.as_bytes())) {
                Ok(s) => wire.state = s,
                Err(_) => {
                    return Err(self.diverged(
                        t,
                        Check::WireRoundTrip,
                        &reference.state,
                        &wire.state,
                        &mut a,
                        &mut b,
                        &mut scratch,
                    ));
                }
            }

            self.step(&mut reference, t, &tick_in, &history);
            self.step(&mut repeat, t, &tick_in, &history);
            self.step(&mut wire, t, &tick_in, &history);

            let want = self.game.checksum(&reference.state, &mut scratch);
            report.hashes.push(want);
            report.ticks = t.saturating_add(1);

            // The reference state must read back to the same bytes.
            a.clear();
            reference.state.write(&mut a);
            let back = G::State::read(&mut Reader::new(a.as_bytes()));
            let same_bytes = back.as_ref().is_ok_and(|s| {
                b.clear();
                s.write(&mut b);
                a.as_bytes() == b.as_bytes()
            });
            if !same_bytes {
                let got = back.map_or(!want, |s| self.game.checksum(&s, &mut scratch));
                return Err(Divergence {
                    tick: t,
                    check: Check::WireRoundTrip,
                    expected: want,
                    got,
                    first_byte: first_byte(a.as_bytes(), b.as_bytes()),
                    part: None,
                });
            }

            for (check, run) in [(Check::Repeat, &repeat), (Check::WireRoundTrip, &wire)] {
                if self.game.checksum(&run.state, &mut scratch) != want {
                    return Err(self.diverged(
                        t,
                        check,
                        &reference.state,
                        &run.state,
                        &mut a,
                        &mut b,
                        &mut scratch,
                    ));
                }
            }

            if distance > 0 && saved.len() == distance {
                let Some((start, _)) = saved.front() else {
                    continue;
                };
                let first = t.saturating_sub(self.check_distance.saturating_sub(1));
                let mut replay = Run {
                    state: start.clone(),
                    role: Role::Resim,
                };
                for (k, (_, tick_in)) in saved.iter().enumerate() {
                    let tk = first.saturating_add(u32::try_from(k).unwrap_or(u32::MAX));
                    self.step(&mut replay, tk, tick_in, &history);
                }
                report.replayed += u64::from(self.check_distance);
                if self.game.checksum(&replay.state, &mut scratch) != want {
                    return Err(self.diverged(
                        t,
                        Check::Rollback,
                        &reference.state,
                        &replay.state,
                        &mut a,
                        &mut b,
                        &mut scratch,
                    ));
                }
            }
        }
        Ok(report)
    }

    fn step(
        &self,
        run: &mut Run<G::State>,
        t: Tick,
        tick_in: &TickInputs<G::Input>,
        history: &PoseHistory<G::Pose>,
    ) {
        let ctx = StepCtx::new(
            t,
            run.role,
            self.seed,
            &tick_in.inputs,
            &tick_in.joined,
            &tick_in.left,
            history,
            0,
        );
        self.game.step(&mut run.state, &ctx);
    }

    #[allow(clippy::too_many_arguments)]
    fn diverged(
        &self,
        tick: Tick,
        check: Check,
        want: &G::State,
        got: &G::State,
        a: &mut Writer,
        b: &mut Writer,
        scratch: &mut Writer,
    ) -> Divergence {
        a.clear();
        want.write(a);
        b.clear();
        got.write(b);
        let part = self.parts.and_then(|parts| {
            let mut pw = Vec::new();
            let mut pg = Vec::new();
            parts(self.game, want, &mut pw);
            parts(self.game, got, &mut pg);
            pw.iter().zip(&pg).find(|(x, y)| x != y).map(|(x, _)| x.0)
        });
        Divergence {
            tick,
            check,
            expected: self.game.checksum(want, scratch),
            got: self.game.checksum(got, scratch),
            first_byte: first_byte(a.as_bytes(), b.as_bytes()),
            part,
        }
    }
}

fn first_byte(a: &[u8], b: &[u8]) -> Option<usize> {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => Some(i),
        None if a.len() != b.len() => Some(a.len().min(b.len())),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_divergence_finds_the_index() {
        assert_eq!(first_divergence(&[1, 2, 3], &[1, 2, 3]), None);
        assert_eq!(first_divergence(&[1, 2, 3], &[1, 9, 3]), Some(1));
        assert_eq!(first_divergence(&[1, 2], &[1, 2, 3]), Some(2));
        assert_eq!(first_byte(b"abc", b"abd"), Some(2));
        assert_eq!(first_byte(b"ab", b"ab"), None);
    }
}
