//! What a game gives the netcode: its deterministic step, its input and
//! state types, and the poses that are interpolated and lag-compensated.

use evani_core::rng::Pcg32;

use crate::interp::{Interpolate, PoseHistory, Sampled};
use crate::types::{NetId, PeerId, Role, Tick, TickTime, mix64};
use crate::wire::{Wire, Writer, fnv1a64};

/// A game's simulation, as the netcode runs it. The server steps the
/// authoritative state with everyone's inputs; a client steps its predicted
/// copy with its own inputs and its best guess of the others', and replays
/// ticks when a snapshot disagrees.
///
/// Rules for `step`, so the server and every client agree bit for bit:
/// * depend only on `state` and `ctx` (and constant data in `self`): no
///   wall clock, no global or thread-local state, no `HashMap` iteration;
/// * take randomness from [`StepCtx::rng`] (derived from the session seed
///   and the tick, so a replayed tick draws the same numbers);
/// * iterate entities in a fixed order (ids, sorted vectors);
/// * treat inputs as held state (buttons down, aim angles): find presses by
///   comparing with the previous input kept in `state`, because a missing
///   input is filled by repeating the last one.
pub trait NetGame: 'static {
    /// One player's input for one tick.
    type Input: Wire + Clone + Default;
    /// The whole simulated world: everything `step` reads or writes.
    type State: Wire + Clone;
    /// What is drawn for a remote entity and what a shot is tested against
    /// (position, aim, hit volume...). The same type for both is the point:
    /// players hit what they see.
    type Pose: Interpolate;

    /// Name and version of the game's protocol, e.g. `"hexagon/1"`. Peers
    /// built with a different one are refused.
    const PROTOCOL: &'static str;

    /// Advance `state` by one tick.
    fn step(&self, state: &mut Self::State, ctx: &StepCtx<'_, Self::Input, Self::Pose>);

    /// The pose of every networked entity, into `out` (cleared by the
    /// caller). Any order; the history sorts by [`NetId`].
    fn poses(&self, state: &Self::State, out: &mut Vec<(NetId, Self::Pose)>);

    /// A checksum of `state`. Client prediction compares it with the
    /// server's snapshot to skip a replay when the guess was right. The
    /// default hashes the wire encoding; override it with something cheaper
    /// for a big state.
    fn checksum(&self, state: &Self::State, scratch: &mut Writer) -> u64 {
        scratch.clear();
        state.write(scratch);
        fnv1a64(scratch.as_bytes())
    }
}

/// Where a tick's input for one peer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    /// Sent by the player for this tick.
    Fresh,
    /// The player's input for this tick did not arrive in time: the last
    /// one is repeated.
    Repeated,
    /// From a bot on the server.
    Bot,
    /// Client prediction of another player: their last known input.
    Guessed,
}

/// One peer's input for the tick being stepped.
#[derive(Debug, Clone)]
pub struct PeerInput<I> {
    pub peer: PeerId,
    pub input: I,
    /// The sim time this player was seeing remote entities at when they
    /// made this input (interpolation delay included). Lag compensation
    /// rewinds to it.
    pub view: TickTime,
    pub source: InputSource,
}

/// Everything a step may read besides the state.
pub struct StepCtx<'a, I, P> {
    /// The tick being simulated (the state is at `tick` and goes to `tick + 1`).
    pub tick: Tick,
    pub role: Role,
    /// The session seed (the server's; sent to clients when they join).
    pub seed: u64,
    /// One entry per peer, sorted by [`PeerId`].
    pub inputs: &'a [PeerInput<I>],
    /// Peers that joined since the last tick: spawn them. Only the server
    /// sees joins and leaves; clients get the result in the next snapshot.
    pub joined: &'a [PeerId],
    /// Peers that left since the last tick: remove them.
    pub left: &'a [PeerId],
    history: &'a PoseHistory<P>,
    lag_window_subs: u64,
}

impl<'a, I, P: Interpolate> StepCtx<'a, I, P> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        tick: Tick,
        role: Role,
        seed: u64,
        inputs: &'a [PeerInput<I>],
        joined: &'a [PeerId],
        left: &'a [PeerId],
        history: &'a PoseHistory<P>,
        lag_window_subs: u64,
    ) -> Self {
        Self {
            tick,
            role,
            seed,
            inputs,
            joined,
            left,
            history,
            lag_window_subs,
        }
    }

    /// A step context outside a session, for running a game's step in
    /// tools or a plain offline loop: no history, so [`StepCtx::rewind`]
    /// always says "use the live state".
    pub fn standalone(
        tick: Tick,
        role: Role,
        seed: u64,
        inputs: &'a [PeerInput<I>],
        history: &'a PoseHistory<P>,
    ) -> Self {
        Self::new(tick, role, seed, inputs, &[], &[], history, 0)
    }

    /// `peer`'s input this tick.
    pub fn input(&self, peer: PeerId) -> Option<&PeerInput<I>> {
        self.inputs
            .binary_search_by_key(&peer, |p| p.peer)
            .ok()
            .and_then(|i| self.inputs.get(i))
    }

    /// A generator for this tick and `stream` (use one stream per purpose,
    /// e.g. per weapon or per bot). The same tick, seed and stream always
    /// give the same numbers, on every peer and in every replay.
    pub fn rng(&self, stream: u64) -> Pcg32 {
        tick_rng(self.seed, self.tick, stream)
    }

    /// Lag compensation: the poses of every entity as `peer` saw them when
    /// it made this tick's input, into `out`, and the time they are from.
    /// The view time is clamped to `net.lag_comp_max_ms` before now, so a
    /// slow or lying client cannot reach further back.
    ///
    /// `None` means test against the live state instead: the peer is a bot
    /// or a guessed input, there is no history yet, or lag compensation is
    /// off (`net.lag_comp_max_ms = 0`).
    pub fn rewind(&self, peer: PeerId, out: &mut Vec<(NetId, P)>) -> Option<TickTime> {
        out.clear();
        let pi = self.input(peer)?;
        if !matches!(pi.source, InputSource::Fresh | InputSource::Repeated)
            || self.lag_window_subs == 0
        {
            return None;
        }
        let now = TickTime::at(self.tick);
        let at = pi.view.clamp(now.sub_subs(self.lag_window_subs), now);
        match self.history.sample(at, out) {
            Sampled::Empty => None,
            _ => Some(at),
        }
    }
}

/// The generator for `tick` and `stream` under `seed` (what
/// [`StepCtx::rng`] returns).
pub fn tick_rng(seed: u64, tick: Tick, stream: u64) -> Pcg32 {
    Pcg32::new(mix64(seed ^ mix64(u64::from(tick))), stream)
}

/// A server-side player that makes its own inputs. Bots read the
/// authoritative state at the start of each tick and their inputs go into
/// the same per-peer input path as a remote player's, so the step sees
/// bots and humans alike. Keep a bot deterministic: use only `state`, its
/// own fields and `rng` (seeded per bot from the session seed).
pub trait Bot<G: NetGame> {
    fn think(
        &mut self,
        game: &G,
        state: &G::State,
        me: PeerId,
        tick: Tick,
        rng: &mut Pcg32,
    ) -> G::Input;
}
