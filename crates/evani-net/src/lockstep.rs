//! Deterministic input-only netcode with a referee (feature `lockstep`).
//!
//! Every peer runs the same fixed-step simulation from the same input
//! stream. Players send only their inputs; the **referee** (the host's
//! server, running the game headless) puts every seat's input for a tick
//! into one [`Frame`] (late input = the seat's last input; bot seats get
//! their input from the referee's own [`Brains`]), steps its own copy and
//! relays the confirmed frames. Results come from the referee's run.
//!
//! * [`Referee`]: seats, input queues, bots, frame history, state hashes,
//!   checkpoints ([`Referee::save`] / [`Referee::load`]) and per-recipient
//!   views ([`Referee::view`]: `&self`, so a host may call it any number of
//!   times between steps).
//! * [`LockstepClient`] in player mode: applies confirmed frames to its
//!   *confirmed* copy (bit for bit the referee's), compares the referee's
//!   periodic 32-bit state hash, and predicts ahead from it with its own
//!   inputs so the player's kart (or ship, or hero) reacts at once; a
//!   newly confirmed frame re-runs the prediction (a rollback). A mismatch
//!   or a gap asks for the full state, which the next view carries.
//! * [`LockstepClient`] in spectator mode: every spectator view is complete
//!   on its own (a state a few ticks back plus the frames since), so a
//!   spectator who skips views, joins late or switches who it follows is
//!   always one view from a correct picture. Spectators send nothing.
//! * [`sync_test`]: run a game twice from the same frames and compare hashes
//!   every tick (the determinism guard; GGRS's sync-test idea).
//!
//! Ideas, not code, from Gaffer on Games' "Deterministic Lockstep" (redundant
//! inputs with acks, a playout buffer, periodic state hashes) and from GGRS
//! (MIT OR Apache-2.0: the save/load/advance shape, the sync test, spectators
//! and input prediction); DECISIONS.md "Multiplayer: lockstep with a
//! referee" has the links. Nothing here knows any host or platform: a peer is
//! an opaque number the host hands over per join.
//!
//! Wire format (all integers LEB128 varints unless noted):
//!
//! ```text
//! client -> referee (input packet)
//!   u8 WIRE, varint PROTOCOL, u8 kind = 1
//!   varint ack      newest tick the client holds as confirmed (0 = none)
//!   u8 flags        bit 0: send me the full state
//!   varint first    tick of the first input (0 when count = 0)
//!   varint count    at most 64
//!   count x input   the game's input encoding, ticks first, first+1, ...
//!
//! referee -> client (view)
//!   u8 WIRE, varint PROTOCOL, u8 kind = 2 (player) or 3 (spectator)
//!   varint tick     the referee's newest tick
//!   u8 seat         the recipient's seat, 255 = none
//!   varint in_ack   newest tick of the recipient's input the referee holds
//!   zigzag lead     in_ack - tick: how early the recipient's inputs arrive
//!   u8 flags        bit 0: state follows, bit 1: hash follows
//!   [state]         varint at, varint len, len bytes (the game's save),
//!                   then the frame at `at`, whole
//!   [hash]          varint at, u32 little-endian
//!   varint first, varint count, frames (first whole, then each against
//!                   the one before: changed humans mask, changed seats)
//! ```

use std::collections::VecDeque;

use crate::wire::{Reader, WireError, Writer, fnv1a64};

/// Version of the lockstep packets.
pub const WIRE: u8 = 1;
/// Most seats a game may have (the humans mask is 32 bits).
pub const MAX_SLOTS: usize = 32;
/// "No seat" in a view (a spectator, or a player on a full session).
pub const NO_SEAT: u8 = 0xFF;

const KIND_INPUT: u8 = 1;
const KIND_PLAYER: u8 = 2;
const KIND_SPECTATOR: u8 = 3;
const MAX_INPUTS_PER_PACKET: usize = 64;

/// A game the lockstep netcode can run: a fixed-step simulation stepped by
/// one [`Frame`] per tick, with exact save and load.
///
/// Rules (the same as every Evani.Dev simulation): no clock, no outside
/// randomness (seed a `Pcg32` from the session seed and keep it in the
/// state), no `HashMap` iteration order, no reading anything but the frame.
pub trait LockstepGame: 'static {
    /// One seat's input for one tick.
    type Input: Copy + Default + PartialEq + core::fmt::Debug;
    /// Seats in the game (players and bots), at most [`MAX_SLOTS`].
    const SLOTS: usize;
    /// Hash of the game's rules and input encoding: peers with a different
    /// one are refused.
    const PROTOCOL: u32;
    fn write_input(input: &Self::Input, w: &mut Writer);
    fn read_input(r: &mut Reader<'_>) -> Result<Self::Input, WireError>;
    /// Clamp a received input to its legal range (the referee calls it on
    /// every input; never trust a client).
    fn sanitize(input: Self::Input) -> Self::Input {
        input
    }
    /// The input a seat repeats on a tick its input is late for (and the
    /// one a client predicts for others): default the same input; a game
    /// drops one-shot presses here (an item button) so a late packet never
    /// fires one twice.
    fn held(input: Self::Input) -> Self::Input {
        input
    }
    /// Advance one tick.
    fn step(&mut self, frame: &Frame<Self::Input>);
    /// The whole simulation state, exactly.
    fn save(&self, w: &mut Writer);
    /// Replace the state with one [`LockstepGame::save`] wrote.
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), WireError>;
    /// The game's results so far (finishing order, times, scores), for the
    /// host: what the referee's own run decided. Default: none.
    fn results(&self, _w: &mut Writer) {}
    /// A 32-bit hash of the state (default: of its save).
    fn hash(&self) -> u32 {
        let mut w = Writer::new();
        self.save(&mut w);
        fold(fnv1a64(w.as_bytes()))
    }
}

/// The referee's bots: inputs for seats no human holds, from the referee's
/// own run. Their memory is theirs (not part of the replicated state), so
/// clients never run them.
pub trait Brains<G: LockstepGame> {
    fn think(&mut self, game: &G, seat: usize, tick: u64) -> G::Input;
    fn save(&self, _w: &mut Writer) {}
    fn load(&mut self, _r: &mut Reader<'_>) -> Result<(), WireError> {
        Ok(())
    }
}

/// Bots that hold still (every input default).
#[derive(Debug, Default, Clone, Copy)]
pub struct IdleBrains;

impl<G: LockstepGame> Brains<G> for IdleBrains {
    fn think(&mut self, _game: &G, _seat: usize, _tick: u64) -> G::Input {
        G::Input::default()
    }
}

/// Every seat's input for one tick, and which seats humans hold.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame<I> {
    pub tick: u64,
    /// Bit `s` set: a human holds seat `s` this tick.
    pub humans: u32,
    pub inputs: Vec<I>,
}

impl<I: Copy + Default> Frame<I> {
    pub fn empty(tick: u64, seats: usize) -> Self {
        Self {
            tick,
            humans: 0,
            inputs: vec![I::default(); seats],
        }
    }

    pub fn is_human(&self, seat: usize) -> bool {
        seat < 32 && self.humans & (1 << seat) != 0
    }
}

/// Fold a 64-bit hash to 32 bits.
pub fn fold(h: u64) -> u32 {
    (h ^ (h >> 32)) as u32
}

fn write_frame<G: LockstepGame>(w: &mut Writer, f: &Frame<G::Input>, prev: Option<&Frame<G::Input>>) {
    let def = G::Input::default();
    let prev_humans = prev.map_or(0, |p| p.humans);
    let mut mask: u64 = 0;
    for (s, i) in f.inputs.iter().enumerate().take(MAX_SLOTS) {
        let before = prev.and_then(|p| p.inputs.get(s)).unwrap_or(&def);
        if i != before {
            mask |= 1 << s;
        }
    }
    let changed = f.humans != prev_humans;
    w.u8(u8::from(changed));
    if changed {
        w.varint(u64::from(f.humans));
    }
    w.varint(mask);
    for (s, i) in f.inputs.iter().enumerate().take(MAX_SLOTS) {
        if mask & (1 << s) != 0 {
            G::write_input(i, w);
        }
    }
}

fn read_frame<G: LockstepGame>(r: &mut Reader<'_>, tick: u64, prev: Option<&Frame<G::Input>>) -> Result<Frame<G::Input>, WireError> {
    let mut f = match prev {
        Some(p) => Frame {
            tick,
            humans: p.humans,
            inputs: p.inputs.clone(),
        },
        None => Frame::empty(tick, G::SLOTS),
    };
    let flags = r.u8()?;
    if flags > 1 {
        return Err(WireError::Invalid);
    }
    if flags == 1 {
        f.humans = r.varint_u32()?;
    }
    let mask = r.varint()?;
    if G::SLOTS < 64 && mask >> G::SLOTS != 0 {
        return Err(WireError::Invalid);
    }
    for s in 0..G::SLOTS.min(MAX_SLOTS) {
        if mask & (1 << s) != 0 {
            f.inputs[s] = G::read_input(r)?;
        }
    }
    Ok(f)
}

fn write_frames<'a, G: LockstepGame>(w: &mut Writer, frames: impl Iterator<Item = &'a Frame<G::Input>>) {
    let frames: Vec<&Frame<G::Input>> = frames.collect();
    w.varint(frames.first().map_or(0, |f| f.tick));
    w.varint(frames.len() as u64);
    let mut prev: Option<&Frame<G::Input>> = None;
    for f in frames {
        write_frame::<G>(w, f, prev);
        prev = Some(f);
    }
}

fn read_frames<G: LockstepGame>(r: &mut Reader<'_>) -> Result<Vec<Frame<G::Input>>, WireError> {
    let first = r.varint()?;
    let n = r.list_len(2)?;
    let mut out: Vec<Frame<G::Input>> = Vec::with_capacity(n);
    for k in 0..n {
        let f = read_frame::<G>(r, first + k as u64, out.last())?;
        out.push(f);
    }
    Ok(out)
}

fn header<G: LockstepGame>(w: &mut Writer, kind: u8) {
    w.u8(WIRE);
    w.varint(u64::from(G::PROTOCOL));
    w.u8(kind);
}

fn read_header<G: LockstepGame>(r: &mut Reader<'_>) -> Result<u8, PacketError> {
    if r.u8()? != WIRE {
        return Err(PacketError::Version);
    }
    if r.varint_u32()? != G::PROTOCOL {
        return Err(PacketError::Protocol);
    }
    Ok(r.u8()?)
}

/// Why a packet was refused (it is dropped and counted; nothing changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    /// Another wire version.
    Version,
    /// Another game, or another version of its rules.
    Protocol,
    /// Not a packet of this kind, or from someone not seated.
    Unexpected,
    /// Over the size cap.
    TooBig,
    Wire(WireError),
}

impl From<WireError> for PacketError {
    fn from(e: WireError) -> Self {
        PacketError::Wire(e)
    }
}

impl core::fmt::Display for PacketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PacketError::Version => f.write_str("another wire version"),
            PacketError::Protocol => f.write_str("another game or rules version"),
            PacketError::Unexpected => f.write_str("unexpected packet"),
            PacketError::TooBig => f.write_str("packet over the size cap"),
            PacketError::Wire(e) => write!(f, "{e}"),
        }
    }
}

// ---- the referee -----------------------------------------------------------------

/// The referee's settings (ticks are the game's fixed 1/60 s steps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefereeConfig {
    /// Confirmed frames kept for views (a player further behind gets the
    /// full state).
    pub history: u32,
    /// A state hash every this many ticks (0: none).
    pub hash_every: u32,
    /// Inputs at most this far ahead of the referee are kept.
    pub max_ahead: u32,
    /// A player view carries at most this many frames; further behind, the
    /// full state instead.
    pub max_view_frames: u32,
    /// A spectator base state every this many ticks.
    pub spectator_every: u32,
    /// A spectator view starts from a base at least this many ticks old, so
    /// spectators play smoothly from a buffer.
    pub spectator_lag: u32,
    /// Input packets over this size are refused.
    pub max_packet: usize,
}

impl Default for RefereeConfig {
    fn default() -> Self {
        Self {
            history: 240,
            hash_every: 30,
            max_ahead: 90,
            max_view_frames: 120,
            spectator_every: 3,
            spectator_lag: 9,
            max_packet: 2048,
        }
    }
}

/// Referee totals (tests, host logs, the overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefereeStats {
    pub ticks: u64,
    pub packets: u64,
    pub refused: u64,
    /// Inputs that came after their tick was confirmed (the seat repeated
    /// its last input instead).
    pub late: u64,
    /// Ticks a human seat had no input for (repeated its last).
    pub repeated: u64,
}

#[derive(Debug, Clone)]
struct Peer<I> {
    id: u32,
    seat: Option<u8>,
    /// Inputs for future ticks, ascending.
    pending: VecDeque<(u64, I)>,
    last: I,
    ack: u64,
    want_full: bool,
    newest: u64,
}

/// The authority: seats, inputs, bots, the confirmed frames and views.
pub struct Referee<G: LockstepGame, B: Brains<G>> {
    game: G,
    brains: B,
    cfg: RefereeConfig,
    tick: u64,
    peers: Vec<Peer<G::Input>>,
    frames: VecDeque<Frame<G::Input>>,
    hashes: VecDeque<(u64, u32)>,
    bases: VecDeque<(u64, Vec<u8>)>,
    last: Frame<G::Input>,
    stats: RefereeStats,
}

impl<G: LockstepGame, B: Brains<G>> Referee<G, B> {
    pub fn new(game: G, brains: B, cfg: RefereeConfig) -> Self {
        let mut base = Writer::new();
        game.save(&mut base);
        let mut r = Self {
            game,
            brains,
            cfg,
            tick: 0,
            peers: Vec::new(),
            frames: VecDeque::new(),
            hashes: VecDeque::new(),
            bases: VecDeque::new(),
            last: Frame::empty(0, G::SLOTS),
            stats: RefereeStats::default(),
        };
        r.bases.push_back((0, base.into_bytes()));
        r
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn game(&self) -> &G {
        &self.game
    }

    pub fn stats(&self) -> RefereeStats {
        self.stats
    }

    pub fn config(&self) -> RefereeConfig {
        self.cfg
    }

    /// The current state hash.
    pub fn hash(&self) -> u32 {
        self.game.hash()
    }

    /// The confirmed frames still kept, oldest first (tests and host logs
    /// replay them).
    pub fn history(&self) -> impl Iterator<Item = &Frame<G::Input>> {
        self.frames.iter()
    }

    /// Seats humans hold now (bit per seat).
    pub fn humans(&self) -> u32 {
        self.peers
            .iter()
            .filter_map(|p| p.seat)
            .fold(0, |m, s| m | (1u32 << s))
    }

    /// The seat `peer` holds, if any.
    pub fn seat_of(&self, peer: u32) -> Option<u8> {
        self.find(peer).and_then(|i| self.peers[i].seat)
    }

    fn find(&self, peer: u32) -> Option<usize> {
        self.peers.binary_search_by_key(&peer, |p| p.id).ok()
    }

    /// A peer joins (from the host's roster). A player takes the lowest free
    /// seat (a bot's); with every seat held by a human it watches instead.
    /// Returns its seat, `None` for a spectator. Joining twice keeps the
    /// first seat.
    pub fn join(&mut self, peer: u32, spectator: bool) -> Option<u8> {
        if let Some(i) = self.find(peer) {
            return self.peers[i].seat;
        }
        let seat = if spectator {
            None
        } else {
            let taken = self.humans();
            (0..G::SLOTS.min(MAX_SLOTS)).find(|&s| taken & (1 << s) == 0).map(|s| s as u8)
        };
        // A human taking over a bot's seat starts from the bot's last input.
        let last = seat.map_or_else(G::Input::default, |s| self.last.inputs.get(usize::from(s)).copied().unwrap_or_default());
        let p = Peer {
            id: peer,
            seat,
            pending: VecDeque::new(),
            last,
            ack: 0,
            want_full: true,
            newest: 0,
        };
        let at = self.peers.partition_point(|q| q.id < peer);
        self.peers.insert(at, p);
        seat
    }

    /// A peer leaves (the host's roster is the truth): its seat goes back to
    /// a bot. False if it was not here.
    pub fn leave(&mut self, peer: u32) -> bool {
        match self.find(peer) {
            Some(i) => {
                self.peers.remove(i);
                true
            }
            None => false,
        }
    }

    /// One packet from `peer`, exactly as its client sent it.
    pub fn input(&mut self, peer: u32, packet: &[u8]) -> Result<(), PacketError> {
        self.stats.packets += 1;
        let r = self.read_input(peer, packet);
        if r.is_err() {
            self.stats.refused += 1;
        }
        r
    }

    fn read_input(&mut self, peer: u32, packet: &[u8]) -> Result<(), PacketError> {
        if packet.len() > self.cfg.max_packet {
            return Err(PacketError::TooBig);
        }
        let i = self.find(peer).ok_or(PacketError::Unexpected)?;
        let mut r = Reader::new(packet);
        if read_header::<G>(&mut r)? != KIND_INPUT {
            return Err(PacketError::Unexpected);
        }
        let ack = r.varint()?;
        let flags = r.u8()?;
        let first = r.varint()?;
        let n = r.list_len(1)?;
        if n > MAX_INPUTS_PER_PACKET || flags > 1 {
            return Err(PacketError::Wire(WireError::Invalid));
        }
        let mut inputs = Vec::with_capacity(n);
        for _ in 0..n {
            inputs.push(G::sanitize(G::read_input(&mut r)?));
        }
        if !r.is_empty() {
            return Err(PacketError::Wire(WireError::Invalid));
        }
        let (now, ahead) = (self.tick, u64::from(self.cfg.max_ahead));
        let p = &mut self.peers[i];
        p.ack = p.ack.max(ack.min(now));
        p.want_full = flags & 1 != 0;
        if p.seat.is_none() {
            // A spectator's (or an unseated player's) inputs never reach the game.
            return Ok(());
        }
        for (k, input) in inputs.into_iter().enumerate() {
            let t = first.saturating_add(k as u64);
            if t <= now {
                if t > p.newest {
                    self.stats.late += 1;
                }
                continue;
            }
            if t > now + ahead {
                break;
            }
            p.newest = p.newest.max(t);
            match p.pending.binary_search_by_key(&t, |e| e.0) {
                Ok(at) => p.pending[at].1 = input,
                Err(at) => p.pending.insert(at, (t, input)),
            }
        }
        Ok(())
    }

    /// Advance `ticks` whole ticks (a host with a 50 ms step calls
    /// `step(3)`).
    pub fn step(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step_one();
        }
    }

    fn step_one(&mut self) {
        let t = self.tick + 1;
        let humans = self.humans();
        let mut inputs = vec![G::Input::default(); G::SLOTS];
        for p in &mut self.peers {
            let Some(seat) = p.seat else { continue };
            while p.pending.front().is_some_and(|e| e.0 < t) {
                p.pending.pop_front();
            }
            let input = match p.pending.front() {
                Some(&(at, i)) if at == t => {
                    p.pending.pop_front();
                    i
                }
                _ => {
                    self.stats.repeated += 1;
                    G::held(p.last)
                }
            };
            p.last = input;
            if let Some(slot) = inputs.get_mut(usize::from(seat)) {
                *slot = input;
            }
        }
        for (s, slot) in inputs.iter_mut().enumerate() {
            if humans & (1 << s) == 0 {
                *slot = G::sanitize(self.brains.think(&self.game, s, t));
            }
        }
        let frame = Frame { tick: t, humans, inputs };
        self.game.step(&frame);
        self.tick = t;
        self.stats.ticks += 1;
        self.last = frame.clone();
        self.frames.push_back(frame);
        while self.frames.len() > self.cfg.history.max(1) as usize {
            self.frames.pop_front();
        }
        if self.cfg.hash_every > 0 && t.is_multiple_of(u64::from(self.cfg.hash_every)) {
            self.hashes.push_back((t, self.game.hash()));
            while self.hashes.len() > 8 {
                self.hashes.pop_front();
            }
        }
        if self.cfg.spectator_every > 0 && t.is_multiple_of(u64::from(self.cfg.spectator_every)) {
            let mut w = Writer::new();
            self.game.save(&mut w);
            self.bases.push_back((t, w.into_bytes()));
            let keep = u64::from(self.cfg.spectator_lag + 2 * self.cfg.spectator_every);
            while self.bases.len() > 1 && self.bases.front().is_some_and(|b| b.0 + keep < t) {
                self.bases.pop_front();
            }
        }
    }

    fn frame_at(&self, tick: u64) -> Option<&Frame<G::Input>> {
        let first = self.frames.front()?.tick;
        let i = usize::try_from(tick.checked_sub(first)?).ok()?;
        self.frames.get(i)
    }

    fn frame_or_last(&self, tick: u64) -> Frame<G::Input> {
        self.frame_at(tick).cloned().unwrap_or_else(|| Frame {
            tick,
            ..self.last.clone()
        })
    }

    /// The view for `peer` (a player's or a spectator's), or with `None` the
    /// spectator view every spectator gets. Never changes the referee.
    pub fn view(&self, peer: Option<u32>, out: &mut Writer) {
        out.clear();
        let p = peer.and_then(|id| self.find(id)).map(|i| &self.peers[i]);
        match p {
            Some(p) if p.seat.is_some() => self.player_view(p, out),
            _ => self.spectator_view(out),
        }
    }

    fn player_view(&self, p: &Peer<G::Input>, out: &mut Writer) {
        header::<G>(out, KIND_PLAYER);
        out.varint(self.tick);
        out.u8(p.seat.unwrap_or(NO_SEAT));
        let in_ack = p.newest.max(p.ack.min(self.tick));
        out.varint(in_ack);
        out.zigzag(p.newest as i64 - self.tick as i64);
        let oldest = self.frames.front().map_or(self.tick + 1, |f| f.tick);
        let behind = self.tick.saturating_sub(p.ack);
        let full = p.want_full || p.ack == 0 || p.ack + 1 < oldest || behind > u64::from(self.cfg.max_view_frames);
        let hash = self.hashes.back().copied();
        out.u8(u8::from(full) | (u8::from(hash.is_some()) << 1));
        if full {
            let mut s = Writer::new();
            self.game.save(&mut s);
            out.varint(self.tick);
            out.varint(s.len() as u64);
            out.bytes(s.as_bytes());
            write_frame::<G>(out, &self.frame_or_last(self.tick), None);
        }
        if let Some((at, h)) = hash {
            out.varint(at);
            out.bytes(&h.to_le_bytes());
        }
        if full {
            write_frames::<G>(out, core::iter::empty());
        } else {
            write_frames::<G>(out, self.frames.iter().filter(|f| f.tick > p.ack));
        }
    }

    fn spectator_view(&self, out: &mut Writer) {
        header::<G>(out, KIND_SPECTATOR);
        out.varint(self.tick);
        out.u8(NO_SEAT);
        out.varint(0);
        out.zigzag(0);
        let lag = u64::from(self.cfg.spectator_lag);
        let oldest = self.frames.front().map_or(self.tick, |f| f.tick.saturating_sub(1));
        let base = self
            .bases
            .iter()
            .rev()
            .find(|b| b.0 + lag <= self.tick && b.0 >= oldest)
            .or_else(|| self.bases.iter().find(|b| b.0 >= oldest))
            .or(self.bases.back());
        let hash = self.hashes.back().copied();
        out.u8(1 | (u8::from(hash.is_some()) << 1));
        let (at, bytes) = match base {
            Some((at, b)) => (*at, b.as_slice()),
            None => (self.tick, &[][..]),
        };
        out.varint(at);
        out.varint(bytes.len() as u64);
        out.bytes(bytes);
        write_frame::<G>(out, &self.frame_or_last(at), None);
        if let Some((t, h)) = hash {
            out.varint(t);
            out.bytes(&h.to_le_bytes());
        }
        write_frames::<G>(out, self.frames.iter().filter(|f| f.tick > at));
    }

    /// The whole referee as bytes: tick, game, bots, seats and their queued
    /// inputs, history, hashes and spectator bases. [`Referee::load`] carries
    /// on bit for bit (a host that restarts or moves a session).
    pub fn save(&self, w: &mut Writer) {
        w.u8(WIRE);
        w.varint(u64::from(G::PROTOCOL));
        w.varint(self.tick);
        let mut g = Writer::new();
        self.game.save(&mut g);
        w.varint(g.len() as u64);
        w.bytes(g.as_bytes());
        let mut b = Writer::new();
        self.brains.save(&mut b);
        w.varint(b.len() as u64);
        w.bytes(b.as_bytes());
        w.varint(self.peers.len() as u64);
        for p in &self.peers {
            w.varint(u64::from(p.id));
            w.u8(p.seat.unwrap_or(NO_SEAT));
            G::write_input(&p.last, w);
            w.varint(p.ack);
            w.u8(u8::from(p.want_full));
            w.varint(p.newest);
            w.varint(p.pending.len() as u64);
            for (t, i) in &p.pending {
                w.varint(*t);
                G::write_input(i, w);
            }
        }
        write_frame::<G>(w, &self.last, None);
        write_frames::<G>(w, self.frames.iter());
        w.varint(self.hashes.len() as u64);
        for (t, h) in &self.hashes {
            w.varint(*t);
            w.bytes(&h.to_le_bytes());
        }
        w.varint(self.bases.len() as u64);
        for (t, s) in &self.bases {
            w.varint(*t);
            w.varint(s.len() as u64);
            w.bytes(s);
        }
        let s = self.stats;
        for v in [s.ticks, s.packets, s.refused, s.late, s.repeated] {
            w.varint(v);
        }
    }

    /// Replace everything with a [`Referee::save`].
    pub fn load(&mut self, data: &[u8]) -> Result<(), PacketError> {
        let mut r = Reader::new(data);
        if r.u8()? != WIRE {
            return Err(PacketError::Version);
        }
        if r.varint_u32()? != G::PROTOCOL {
            return Err(PacketError::Protocol);
        }
        let tick = r.varint()?;
        let n = r.list_len(1)?;
        let g = r.take(n)?;
        self.game.load(&mut Reader::new(g))?;
        let n = r.list_len(1)?;
        let b = r.take(n)?;
        self.brains.load(&mut Reader::new(b))?;
        let n = r.list_len(4)?;
        let mut peers = Vec::with_capacity(n);
        for _ in 0..n {
            let id = r.varint_u32()?;
            let seat = match r.u8()? {
                NO_SEAT => None,
                s if usize::from(s) < G::SLOTS => Some(s),
                _ => return Err(PacketError::Wire(WireError::Invalid)),
            };
            let last = G::read_input(&mut r)?;
            let ack = r.varint()?;
            let want_full = r.u8()? != 0;
            let newest = r.varint()?;
            let k = r.list_len(1)?;
            let mut pending = VecDeque::with_capacity(k);
            for _ in 0..k {
                let t = r.varint()?;
                pending.push_back((t, G::read_input(&mut r)?));
            }
            peers.push(Peer {
                id,
                seat,
                pending,
                last,
                ack,
                want_full,
                newest,
            });
        }
        let last = read_frame::<G>(&mut r, tick, None)?;
        let frames = read_frames::<G>(&mut r)?;
        let n = r.list_len(2)?;
        let mut hashes = VecDeque::with_capacity(n);
        for _ in 0..n {
            let t = r.varint()?;
            let b = r.take(4)?;
            hashes.push_back((t, u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        }
        let n = r.list_len(2)?;
        let mut bases = VecDeque::with_capacity(n);
        for _ in 0..n {
            let t = r.varint()?;
            let k = r.list_len(1)?;
            bases.push_back((t, r.take(k)?.to_vec()));
        }
        let mut st = [0u64; 5];
        for v in &mut st {
            *v = r.varint()?;
        }
        self.tick = tick;
        self.peers = peers;
        self.last = last;
        self.frames = frames.into();
        self.hashes = hashes;
        self.bases = bases;
        self.stats = RefereeStats {
            ticks: st[0],
            packets: st[1],
            refused: st[2],
            late: st[3],
            repeated: st[4],
        };
        Ok(())
    }
}

// ---- clients ---------------------------------------------------------------------

/// A client's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConfig {
    /// A player's input applies this many ticks after it is read (0: at
    /// once; prediction keeps the player's own reaction instant either way;
    /// more trims corrections of others at the cost of feel).
    pub input_delay: u32,
    /// Keep inputs arriving this many ticks early at the referee (at
    /// least); below it the client runs a tick ahead.
    pub lead_min: i64,
    /// ... and at most this many; above it the client holds a tick.
    pub lead_max: i64,
    /// Never predict further than this past the confirmed tick.
    pub max_predict: u32,
    /// A spectator plays this many ticks behind the newest frame.
    pub spectator_buffer: u32,
    /// Most inputs in one packet (the newest ones; older ones are acked).
    pub max_packet_inputs: usize,
    /// Most game steps one [`LockstepClient::tick`] runs to catch up: a
    /// player's prediction replay after a correction, a spectator's playout
    /// of a backlog. More waits for the next ticks, so a slow host spreads
    /// the work over several frames instead of stalling on one. 0 (the
    /// default) is no cap, as the client always ran; otherwise 2 to 240
    /// (`catchup_cap` keeps it there; one step a tick would only hold a
    /// backlog in place) (`net.max_catchup_steps`).
    pub max_catchup_steps: u32,
}

/// `net.input_delay_ticks`: default and largest value.
pub const INPUT_DELAY_DEFAULT: u32 = 0;
pub const INPUT_DELAY_MAX: u32 = 6;
/// `net.max_catchup_steps`: 0 is no cap (the default, what the client always
/// did); a cap is at least 2 steps (one a tick only holds a backlog in place)
/// and at most 240 (four seconds).
pub const MAX_CATCHUP_DEFAULT: u32 = 0;
pub const MAX_CATCHUP_MIN: u32 = 2;
pub const MAX_CATCHUP_MAX: u32 = 240;

/// A catch-up cap as used: 0 stays "none", anything else is kept between the
/// limits.
pub fn catchup_cap(steps: u32) -> u32 {
    if steps == 0 { 0 } else { steps.clamp(MAX_CATCHUP_MIN, MAX_CATCHUP_MAX) }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            input_delay: INPUT_DELAY_DEFAULT,
            lead_min: 2,
            lead_max: 9,
            max_predict: 45,
            spectator_buffer: 6,
            max_packet_inputs: 32,
            max_catchup_steps: MAX_CATCHUP_DEFAULT,
        }
    }
}

impl ClientConfig {
    /// The defaults with `net.input_delay_ticks` and `net.max_catchup_steps`
    /// from the parameter registry (the defaults alone when the net module
    /// is not built in). Read the delay when a session starts; the catch-up
    /// cap can be re-read every tick ([`LockstepClient::set_max_catchup_steps`]).
    pub fn from_world(world: &evani_core::ecs::World) -> Self {
        let mut c = Self::default();
        if let (Some(p), Some(reg)) = (world.try_res::<crate::NetParams>(), world.try_res::<evani_core::params::ParamRegistry>()) {
            c.input_delay = u32::try_from(reg.i32(p.input_delay_ticks)).unwrap_or(0).min(INPUT_DELAY_MAX);
            c.max_catchup_steps = catchup_cap(u32::try_from(reg.i32(p.max_catchup_steps)).unwrap_or(0));
        }
        c
    }
}

/// Client totals (tests, the overlay, `NetStats`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClientStats {
    pub views: u64,
    pub refused: u64,
    /// Full states loaded (join, desync, gap).
    pub states: u64,
    /// Hash checks passed and failed.
    pub hash_ok: u64,
    pub desyncs: u64,
    /// Confirmed frames applied.
    pub frames: u64,
    /// Predictions re-run from a newly confirmed state (rollbacks).
    pub rollbacks: u64,
    pub predicted_steps: u64,
    /// Pacing nudges: ran a tick ahead / held a tick.
    pub sped_up: u64,
    pub held: u64,
    /// The most game steps one `tick` has run for catch-up (replay or
    /// playout), and how many times the cap held steps back for later.
    pub peak_catchup: u32,
    pub catchup_capped: u64,
}

/// A view's full state: its tick, the game's save, the frame at that tick.
type ViewState<'a, I> = (u64, &'a [u8], Frame<I>);

/// A player or spectator.
pub struct LockstepClient<G: LockstepGame> {
    conf: G,
    pred: G,
    cfg: ClientConfig,
    spectator: bool,
    synced: bool,
    conf_tick: u64,
    last: Frame<G::Input>,
    /// Spectator: confirmed frames waiting to be played.
    queue: VecDeque<Frame<G::Input>>,
    pred_tick: u64,
    pred_dirty: bool,
    local: u64,
    seat: Option<u8>,
    mine: VecDeque<(u64, G::Input)>,
    my_last: G::Input,
    in_ack: u64,
    lead: i64,
    since_adjust: u32,
    want_full: bool,
    newest: u64,
    stats: ClientStats,
    scratch: Writer,
}

impl<G: LockstepGame> LockstepClient<G> {
    /// A player. `confirmed` and `predicted` are two fresh copies of the game
    /// (any state; the first view brings the real one).
    pub fn player(confirmed: G, predicted: G, cfg: ClientConfig) -> Self {
        Self::new(confirmed, predicted, cfg, false)
    }

    /// A spectator (sends nothing).
    pub fn spectator(confirmed: G, predicted: G, cfg: ClientConfig) -> Self {
        Self::new(confirmed, predicted, cfg, true)
    }

    fn new(conf: G, pred: G, cfg: ClientConfig, spectator: bool) -> Self {
        Self {
            conf,
            pred,
            cfg,
            spectator,
            synced: false,
            conf_tick: 0,
            last: Frame::empty(0, G::SLOTS),
            queue: VecDeque::new(),
            pred_tick: 0,
            pred_dirty: true,
            local: 0,
            seat: None,
            mine: VecDeque::new(),
            my_last: G::Input::default(),
            in_ack: 0,
            lead: 0,
            since_adjust: 0,
            want_full: true,
            newest: 0,
            stats: ClientStats::default(),
            scratch: Writer::new(),
        }
    }

    pub fn is_spectator(&self) -> bool {
        self.spectator
    }

    /// A state has arrived and the client is playing.
    pub fn synced(&self) -> bool {
        self.synced
    }

    /// This player's seat (None: watching, or not seated yet).
    pub fn seat(&self) -> Option<u8> {
        self.seat
    }

    /// The tick the confirmed state is at.
    pub fn confirmed_tick(&self) -> u64 {
        self.conf_tick
    }

    /// The tick on screen (predicted for a player, played for a spectator).
    pub fn display_tick(&self) -> u64 {
        if self.spectator { self.conf_tick } else { self.pred_tick }
    }

    /// The newest tick the referee has told us about.
    pub fn newest_tick(&self) -> u64 {
        self.newest
    }

    /// The confirmed state: bit for bit the referee's at
    /// [`LockstepClient::confirmed_tick`].
    pub fn confirmed(&self) -> &G {
        &self.conf
    }

    /// What to draw: the prediction (a player) or the played state (a
    /// spectator).
    pub fn display(&self) -> &G {
        if self.spectator { &self.conf } else { &self.pred }
    }

    /// The frame of the confirmed tick (who holds which seat).
    pub fn last_frame(&self) -> &Frame<G::Input> {
        &self.last
    }

    pub fn stats(&self) -> ClientStats {
        self.stats
    }

    /// Change the catch-up cap while playing (`net.max_catchup_steps`; kept
    /// between its limits).
    pub fn set_max_catchup_steps(&mut self, steps: u32) {
        self.cfg.max_catchup_steps = catchup_cap(steps);
    }

    /// The referee's last pacing figure (how early our inputs arrive).
    pub fn lead(&self) -> i64 {
        self.lead
    }

    /// One view from the referee. A refused view changes nothing.
    pub fn on_view(&mut self, packet: &[u8]) -> Result<(), PacketError> {
        let r = self.read_view(packet);
        match r {
            Ok(()) => self.stats.views += 1,
            Err(_) => self.stats.refused += 1,
        }
        r
    }

    fn read_view(&mut self, packet: &[u8]) -> Result<(), PacketError> {
        let mut r = Reader::new(packet);
        let kind = read_header::<G>(&mut r)?;
        let want = if self.spectator { KIND_SPECTATOR } else { KIND_PLAYER };
        if kind != want && !(kind == KIND_SPECTATOR && !self.spectator) {
            return Err(PacketError::Unexpected);
        }
        let tick = r.varint()?;
        let seat = r.u8()?;
        let in_ack = r.varint()?;
        let lead = r.zigzag()?;
        let flags = r.u8()?;
        if flags > 3 {
            return Err(PacketError::Wire(WireError::Invalid));
        }
        let state = if flags & 1 != 0 {
            let at = r.varint()?;
            let n = r.list_len(1)?;
            let bytes = r.take(n)?;
            let f = read_frame::<G>(&mut r, at, None)?;
            Some((at, bytes, f))
        } else {
            None
        };
        let hash = if flags & 2 != 0 {
            let at = r.varint()?;
            let b = r.take(4)?;
            Some((at, u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
        } else {
            None
        };
        let frames = read_frames::<G>(&mut r)?;
        if !r.is_empty() {
            return Err(PacketError::Wire(WireError::Invalid));
        }
        // Decoded whole: now apply.
        self.newest = self.newest.max(tick);
        if kind == KIND_SPECTATOR && !self.spectator {
            // A player whose session is full watches until a seat frees up.
            self.seat = None;
            self.apply_spectator(state, hash, frames)?;
            return Ok(());
        }
        if self.spectator {
            return self.apply_spectator(state, hash, frames);
        }
        self.seat = (seat != NO_SEAT).then_some(seat);
        self.in_ack = self.in_ack.max(in_ack);
        self.lead = lead;
        if let Some((at, bytes, f)) = state {
            // Only take a state that moves us forward or repairs a desync.
            if !self.synced || self.want_full || at > self.conf_tick {
                self.conf.load(&mut Reader::new(bytes))?;
                self.conf_tick = at;
                self.last = f;
                self.stats.states += 1;
                self.pred_dirty = true;
                self.want_full = false;
                if !self.synced {
                    self.synced = true;
                    self.local = at + 6;
                    self.since_adjust = 0;
                }
                self.check_hash(hash);
            }
        }
        if !self.synced {
            return Ok(());
        }
        for f in frames {
            if f.tick <= self.conf_tick {
                continue;
            }
            if f.tick != self.conf_tick + 1 {
                // A gap (the referee dropped history we needed): ask for it all.
                self.want_full = true;
                break;
            }
            self.conf.step(&f);
            self.conf_tick = f.tick;
            self.last = f;
            self.stats.frames += 1;
            self.pred_dirty = true;
            self.check_hash(hash);
        }
        // Inputs the referee has (or whose ticks are confirmed) need no resend.
        let done = self.in_ack.max(self.conf_tick);
        while self.mine.front().is_some_and(|e| e.0 <= done) {
            self.mine.pop_front();
        }
        Ok(())
    }

    fn check_hash(&mut self, hash: Option<(u64, u32)>) {
        if let Some((at, h)) = hash
            && at == self.conf_tick
        {
            if self.conf.hash() == h {
                self.stats.hash_ok += 1;
            } else {
                self.stats.desyncs += 1;
                self.want_full = true;
            }
        }
    }

    fn apply_spectator(&mut self, state: Option<ViewState<'_, G::Input>>, hash: Option<(u64, u32)>, frames: Vec<Frame<G::Input>>) -> Result<(), PacketError> {
        let Some((at, bytes, base_frame)) = state else {
            return Err(PacketError::Wire(WireError::Invalid));
        };
        let end = frames.last().map_or(at, |f| f.tick);
        let have = self.queue.back().map_or(self.conf_tick, |f| f.tick);
        // Keep playing from our own state when the view continues it (we
        // hold every tick up to its base); otherwise (first view, fell
        // behind, or the session restarted) start from its base.
        if !self.synced || have < at || have > end + 60 {
            self.conf.load(&mut Reader::new(bytes))?;
            self.conf_tick = at;
            self.last = base_frame;
            self.queue.clear();
            self.synced = true;
            self.stats.states += 1;
            self.check_hash(hash);
        }
        let have = self.queue.back().map_or(self.conf_tick, |f| f.tick);
        for f in frames {
            if f.tick == self.queue.back().map_or(have, |b| b.tick) + 1 {
                self.queue.push_back(f);
            }
        }
        self.newest = self.newest.max(end);
        Ok(())
    }

    /// One local fixed tick. A player passes its input (read this tick); a
    /// spectator passes `None`. Advances the local clock (with pacing) and
    /// the picture.
    pub fn tick(&mut self, input: Option<G::Input>) {
        if self.spectator {
            self.play_spectator();
            return;
        }
        if !self.synced {
            return;
        }
        let input = G::sanitize(input.unwrap_or_default());
        let mut advance = 1u32;
        self.since_adjust += 1;
        if self.since_adjust >= 6 {
            if self.lead < self.cfg.lead_min {
                advance = 2;
                self.stats.sped_up += 1;
                self.since_adjust = 0;
            } else if self.lead > self.cfg.lead_max {
                advance = 0;
                self.stats.held += 1;
                self.since_adjust = 0;
            }
        }
        // Far behind the referee (a stall, a background tab): jump to it.
        if self.local + 30 < self.newest {
            self.local = self.newest + 4;
            self.pred_dirty = true;
        }
        for _ in 0..advance {
            self.local += 1;
            let at = self.local + u64::from(self.cfg.input_delay);
            if self.mine.back().is_none_or(|e| e.0 < at) {
                self.mine.push_back((at, input));
            }
            while self.mine.len() > 256 {
                self.mine.pop_front();
            }
        }
        self.my_last = input;
        self.predict();
    }

    fn mine_at(&self, t: u64) -> Option<G::Input> {
        self.mine.binary_search_by_key(&t, |e| e.0).ok().map(|i| self.mine[i].1)
    }

    fn predict(&mut self) {
        if self.pred_dirty {
            self.scratch.clear();
            self.conf.save(&mut self.scratch);
            if self.pred.load(&mut Reader::new(self.scratch.as_bytes())).is_err() {
                return;
            }
            self.pred_tick = self.conf_tick;
            self.pred_dirty = false;
            self.stats.rollbacks += 1;
        }
        let limit = self.conf_tick + u64::from(self.cfg.max_predict);
        let target = self.local.min(limit);
        let mut budget = match catchup_cap(self.cfg.max_catchup_steps) {
            0 => u32::MAX,
            n => n,
        };
        let mut ran = 0u32;
        let mut frame = Frame {
            tick: 0,
            humans: self.last.humans,
            inputs: self.last.inputs.iter().map(|&i| G::held(i)).collect(),
        };
        while self.pred_tick < target {
            if budget == 0 {
                // The rest waits for the next ticks.
                self.stats.catchup_capped += 1;
                break;
            }
            budget -= 1;
            ran += 1;
            let t = self.pred_tick + 1;
            frame.tick = t;
            if let Some(s) = self.seat.map(usize::from)
                && let Some(slot) = frame.inputs.get_mut(s)
            {
                *slot = self.mine_at(t).unwrap_or(*slot);
            }
            self.pred.step(&frame);
            self.pred_tick = t;
            self.stats.predicted_steps += 1;
        }
        self.stats.peak_catchup = self.stats.peak_catchup.max(ran);
    }

    fn play_spectator(&mut self) {
        let buffer = self.cfg.spectator_buffer as usize;
        let steps = if self.queue.len() > buffer * 3 {
            self.queue.len() - buffer
        } else if self.queue.len() > buffer * 2 {
            2
        } else {
            usize::from(!self.queue.is_empty())
        };
        let cap = match catchup_cap(self.cfg.max_catchup_steps) {
            0 => usize::MAX,
            n => n as usize,
        };
        if steps > cap {
            self.stats.catchup_capped += 1;
        }
        let steps = steps.min(cap);
        let ran = u32::try_from(steps.min(self.queue.len())).unwrap_or(u32::MAX);
        self.stats.peak_catchup = self.stats.peak_catchup.max(ran);
        for _ in 0..steps {
            let Some(f) = self.queue.pop_front() else { break };
            self.conf.step(&f);
            self.conf_tick = f.tick;
            self.last = f;
            self.stats.frames += 1;
        }
    }

    /// The input packet to send now (a spectator, or a player before its
    /// first view, gets an empty `out`). Carries every input the referee has
    /// not acked yet (up to the cap), so a lost packet costs nothing.
    pub fn packet(&mut self, out: &mut Writer) {
        out.clear();
        if self.spectator {
            return;
        }
        header::<G>(out, KIND_INPUT);
        out.varint(self.conf_tick);
        out.u8(u8::from(self.want_full || !self.synced));
        let done = self.in_ack.max(self.conf_tick);
        let pending: Vec<&(u64, G::Input)> = self.mine.iter().filter(|e| e.0 > done).collect();
        let skip = pending.len().saturating_sub(self.cfg.max_packet_inputs.min(MAX_INPUTS_PER_PACKET));
        let send = &pending[skip..];
        // Contiguous run only (they always are; a hole would mislabel ticks).
        let mut run = 0;
        for (k, e) in send.iter().enumerate() {
            if k == 0 || e.0 == send[k - 1].0 + 1 {
                run = k + 1;
            } else {
                break;
            }
        }
        out.varint(send.first().map_or(0, |e| e.0));
        out.varint(run as u64);
        for e in &send[..run] {
            G::write_input(&e.1, out);
        }
    }
}

// ---- the determinism guard -------------------------------------------------------

/// Run two copies of a game through the same frames and compare their
/// hashes after every tick, and a save/load round trip on the way; `Err`
/// names the first tick that differs. `frames(tick)` gives the frame for
/// each tick from 1 to `ticks`.
pub fn sync_test<G: LockstepGame>(a: &mut G, b: &mut G, ticks: u64, mut frames: impl FnMut(u64) -> Frame<G::Input>) -> Result<(), String> {
    let mut w = Writer::new();
    a.save(&mut w);
    b.load(&mut Reader::new(w.as_bytes())).map_err(|e| format!("load: {e}"))?;
    for t in 1..=ticks {
        let f = frames(t);
        a.step(&f);
        b.step(&f);
        let (ha, hb) = (a.hash(), b.hash());
        if ha != hb {
            return Err(format!("tick {t}: hash {ha:08x} != {hb:08x}"));
        }
        if t.is_multiple_of(97) {
            // A round trip through save and load must not change anything.
            w.clear();
            b.save(&mut w);
            b.load(&mut Reader::new(w.as_bytes())).map_err(|e| format!("tick {t}: load: {e}"))?;
            if b.hash() != ha {
                return Err(format!("tick {t}: save/load changed the state"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{LinkConditions, Loopback, LoopbackEnd, Transport};
    use crate::types::LinkId;
    use evani_core::rng::Pcg32;

    /// A toy game: each seat moves a dot by its input; a shared random walk.
    #[derive(Debug, Clone)]
    struct Dots<const N: usize = 4> {
        pos: [i64; N],
        humans: u32,
        rng: (u64, u64),
        t: u64,
    }

    impl<const N: usize> LockstepGame for Dots<N> {
        type Input = i8;
        const SLOTS: usize = N;
        const PROTOCOL: u32 = 0xD075;
        fn write_input(i: &i8, w: &mut Writer) {
            w.u8(*i as u8);
        }
        fn read_input(r: &mut Reader<'_>) -> Result<i8, WireError> {
            Ok(r.u8()? as i8)
        }
        fn sanitize(i: i8) -> i8 {
            i.clamp(-3, 3)
        }
        fn step(&mut self, f: &Frame<i8>) {
            let mut rng = Pcg32::from_raw(self.rng.0, self.rng.1 | 1);
            for s in 0..N {
                self.pos[s] += i64::from(f.inputs[s]) * 10 + i64::from(rng.below(3)) - 1;
            }
            self.rng = rng.to_raw();
            self.humans = f.humans;
            self.t = f.tick;
        }
        fn save(&self, w: &mut Writer) {
            for p in self.pos {
                w.zigzag(p);
            }
            w.varint(u64::from(self.humans));
            w.u64_fixed(self.rng.0);
            w.u64_fixed(self.rng.1);
            w.varint(self.t);
        }
        fn load(&mut self, r: &mut Reader<'_>) -> Result<(), WireError> {
            for p in &mut self.pos {
                *p = r.zigzag()?;
            }
            self.humans = r.varint_u32()?;
            self.rng = (r.u64_fixed()?, r.u64_fixed()?);
            self.t = r.varint()?;
            Ok(())
        }
    }

    fn fresh<const N: usize>() -> Dots<N> {
        Dots {
            pos: [0; N],
            humans: 0,
            rng: Pcg32::new(7, 1).to_raw(),
            t: 0,
        }
    }

    fn dots() -> Dots {
        fresh::<4>()
    }

    struct Wobble;
    impl<const N: usize> Brains<Dots<N>> for Wobble {
        fn think(&mut self, g: &Dots<N>, seat: usize, tick: u64) -> i8 {
            if (tick / 20 + seat as u64).is_multiple_of(2) && g.pos[seat] < 1000 { 1 } else { -1 }
        }
    }

    struct Net<const N: usize = 4> {
        net: Loopback,
        server: LoopbackEnd,
        links: Vec<(LoopbackEnd, u32)>,
        referee: Referee<Dots<N>, Wobble>,
        clients: Vec<LockstepClient<Dots<N>>>,
    }

    fn net<const N: usize>(players: usize, spectators: usize, cond: LinkConditions) -> Net<N> {
        net_with(players, spectators, cond, ClientConfig::default())
    }

    fn net_with<const N: usize>(players: usize, spectators: usize, cond: LinkConditions, cfg: ClientConfig) -> Net<N> {
        let net = Loopback::new(cond, 99);
        let server = net.server();
        let mut referee = Referee::new(fresh::<N>(), Wobble, RefereeConfig::default());
        let mut links = Vec::new();
        let mut clients = Vec::new();
        for k in 0..players + spectators {
            let end = net.connect();
            let id = end.link().0;
            let spec = k >= players;
            referee.join(id, spec);
            links.push((end, id));
            clients.push(if spec {
                LockstepClient::spectator(fresh::<N>(), fresh::<N>(), cfg)
            } else {
                LockstepClient::player(fresh::<N>(), fresh::<N>(), cfg)
            });
        }
        Net {
            net,
            server,
            links,
            referee,
            clients,
        }
    }

    /// One 1/60 s tick of everything: the referee steps every third tick (a
    /// 50 ms host) and sends views; clients tick and send every other tick.
    fn run<const N: usize>(n: &mut Net<N>, ticks: u64, inputs: impl Fn(usize, u64) -> i8) {
        run_with(n, ticks, inputs, |_, _| false, |_, _, _| {});
    }

    /// [`run`] with a client frozen while `frozen(client, tick)` (it hears and
    /// sends nothing, as in a hidden tab) and `observe(client, stats before,
    /// client after)` called after each of its ticks.
    fn run_with<const N: usize>(
        n: &mut Net<N>,
        ticks: u64,
        inputs: impl Fn(usize, u64) -> i8,
        frozen: impl Fn(usize, u64) -> bool,
        mut observe: impl FnMut(usize, ClientStats, &LockstepClient<Dots<N>>),
    ) {
        let mut buf = Vec::new();
        let mut w = Writer::new();
        for t in 0..ticks {
            n.net.advance();
            while let Some(from) = n.server.recv(&mut buf) {
                let _ = n.referee.input(from.0, &buf);
            }
            if t.is_multiple_of(3) {
                n.referee.step(3);
                for (_, id) in &n.links {
                    let mut v = Writer::new();
                    n.referee.view(Some(*id), &mut v);
                    n.server.send(LinkId(*id), v.as_bytes());
                }
            }
            for (k, (end, _)) in n.links.iter_mut().enumerate() {
                if frozen(k, t) {
                    continue;
                }
                while end.recv(&mut buf).is_some() {
                    let _ = n.clients[k].on_view(&buf);
                }
                let c = &mut n.clients[k];
                let input = (!c.is_spectator()).then(|| inputs(k, c.display_tick()));
                let before = c.stats();
                c.tick(input);
                observe(k, before, c);
                if t.is_multiple_of(2) {
                    c.packet(&mut w);
                    if !w.is_empty() {
                        end.send(LinkId::SERVER, w.as_bytes());
                    }
                }
            }
        }
    }

    /// Re-run the referee's own frames from the start state: every client's
    /// confirmed state equals the referee's at that tick.
    fn converged<const N: usize>(n: &Net<N>) {
        for (k, c) in n.clients.iter().enumerate() {
            assert!(c.synced(), "client {k} never synced");
            let mut g = fresh::<N>();
            // Replay the referee's history up to the client's tick.
            let mut w = Writer::new();
            n.referee.save(&mut w);
            let at = c.confirmed_tick();
            assert!(at + 60 > n.referee.tick(), "client {k} at {at}, referee at {}", n.referee.tick());
            let first = n.referee.frames.front().map(|f| f.tick).unwrap_or(0);
            assert!(at >= first, "client {k} behind the history");
            // Start from a spectator-style base: the referee's state now, then
            // compare by stepping the client's copy forward with the history.
            let mut cw = Writer::new();
            c.confirmed().save(&mut cw);
            g.load(&mut Reader::new(cw.as_bytes())).unwrap();
            for f in n.referee.frames.iter().filter(|f| f.tick > at) {
                g.step(f);
            }
            assert_eq!(g.hash(), n.referee.hash(), "client {k} diverged (confirmed at {at})");
        }
    }

    #[test]
    fn four_and_eight_style_sessions_converge_under_loss_and_jitter() {
        let cond = LinkConditions {
            latency_ticks: 6,
            jitter_ticks: 2,
            loss_permille: 50,
        };
        let mut n = net::<4>(3, 1, cond);
        run(&mut n, 1200, |k, t| if (t / 30 + k as u64).is_multiple_of(3) { 2 } else { -1 });
        converged(&n);
        for c in &n.clients {
            assert_eq!(c.stats().desyncs, 0);
            if !c.is_spectator() {
                assert!(c.stats().hash_ok > 10, "{:?}", c.stats());
            }
        }
        // Three humans, one bot seat.
        assert_eq!(n.referee.humans().count_ones(), 3);
    }

    #[test]
    fn a_full_session_seats_the_extra_player_as_a_spectator() {
        let mut n = net::<4>(5, 0, LinkConditions::default());
        assert_eq!(n.referee.humans().count_ones(), 4);
        let extra = n.links[4].1;
        assert_eq!(n.referee.seat_of(extra), None);
        run(&mut n, 120, |_, _| 1);
        assert_eq!(n.clients[4].seat(), None);
        assert!(n.clients[4].synced());
        // The extra player's inputs never reach the game.
        let before = n.referee.hash();
        let mut w = Writer::new();
        let mut c = LockstepClient::<Dots>::player(dots(), dots(), ClientConfig::default());
        c.synced = true;
        c.mine.push_back((n.referee.tick() + 1, 3));
        c.packet(&mut w);
        assert!(n.referee.input(extra, w.as_bytes()).is_ok());
        assert_eq!(n.referee.hash(), before);
    }

    #[test]
    fn leaving_hands_the_seat_to_a_bot_and_joining_takes_it_back() {
        let mut n = net::<4>(2, 0, LinkConditions::default());
        run(&mut n, 90, |_, _| 3);
        let id = n.links[1].1;
        assert!(n.referee.leave(id));
        assert_eq!(n.referee.humans(), 0b01);
        n.referee.step(3);
        assert_eq!(n.referee.join(id, false), Some(1));
        assert_eq!(n.referee.humans(), 0b11);
    }

    #[test]
    fn save_and_load_continue_bit_for_bit() {
        let mut n = net::<4>(2, 1, LinkConditions {
            latency_ticks: 3,
            jitter_ticks: 1,
            loss_permille: 0,
        });
        run(&mut n, 300, |k, _| k as i8);
        let mut w = Writer::new();
        n.referee.save(&mut w);
        let mut copy = Referee::new(dots(), Wobble, RefereeConfig::default());
        copy.load(w.as_bytes()).unwrap();
        assert_eq!(copy.hash(), n.referee.hash());
        n.referee.step(90);
        copy.step(90);
        assert_eq!(copy.hash(), n.referee.hash());
        let (mut a, mut b) = (Writer::new(), Writer::new());
        n.referee.save(&mut a);
        copy.save(&mut b);
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn hostile_packets_are_refused_and_change_nothing() {
        let mut r = Referee::new(dots(), Wobble, RefereeConfig::default());
        r.join(1, false);
        r.step(3);
        let h = r.hash();
        assert_eq!(r.input(1, &[9, 9, 9]), Err(PacketError::Version));
        assert_eq!(r.input(2, &[]), Err(PacketError::Unexpected));
        assert!(matches!(r.input(1, &vec![1u8; 5000]), Err(PacketError::TooBig)));
        let mut w = Writer::new();
        header::<Dots>(&mut w, KIND_INPUT);
        w.varint(0);
        w.u8(0);
        w.varint(4);
        w.varint(1000); // claims 1000 inputs
        assert!(r.input(1, w.as_bytes()).is_err());
        // Out-of-range inputs are clamped by sanitize.
        let mut w = Writer::new();
        header::<Dots>(&mut w, KIND_INPUT);
        w.varint(0);
        w.u8(0);
        w.varint(4);
        w.varint(1);
        w.u8(100);
        assert!(r.input(1, w.as_bytes()).is_ok());
        r.step(1);
        assert_eq!(r.last.inputs[0], 3);
        assert_eq!(r.stats().refused, 4);
        assert_ne!(r.hash(), h);
    }

    #[test]
    fn a_late_spectator_sees_the_same_state() {
        let mut n = net::<4>(2, 0, LinkConditions::default());
        run(&mut n, 200, |_, _| 1);
        let end = n.net.connect();
        let id = end.link().0;
        n.referee.join(id, true);
        n.links.push((end, id));
        n.clients.push(LockstepClient::spectator(dots(), dots(), ClientConfig::default()));
        run(&mut n, 200, |_, _| -1);
        converged(&n);
        let spec = n.clients.last().unwrap();
        assert!(spec.confirmed_tick() + 30 > n.referee.tick());
    }

    /// 100 ms each way, 30 ms of jitter, 2% of packets lost (at 60 Hz).
    fn rough_link() -> LinkConditions {
        LinkConditions {
            latency_ticks: 6,
            jitter_ticks: 2,
            loss_permille: 20,
        }
    }

    fn wiggle(k: usize, t: u64) -> i8 {
        if (t / 30 + k as u64).is_multiple_of(3) { 2 } else { -1 }
    }

    #[test]
    fn eight_players_and_a_watcher_converge_at_input_delays_0_2_and_4() {
        for delay in [0u32, 2, 4] {
            let cfg = ClientConfig {
                input_delay: delay,
                ..ClientConfig::default()
            };
            let mut n = net_with::<8>(8, 1, rough_link(), cfg);
            run(&mut n, 1800, wiggle);
            converged(&n);
            assert_eq!(n.referee.humans().count_ones(), 8, "delay {delay}");
            for (k, c) in n.clients.iter().enumerate() {
                assert_eq!(c.stats().desyncs, 0, "delay {delay}, client {k}");
                if c.is_spectator() {
                    continue;
                }
                assert!(c.stats().hash_ok > 10, "delay {delay}, client {k}: {:?}", c.stats());
                // The input read now is scheduled `delay` ticks ahead.
                let last = c.mine.back().map(|e| e.0);
                assert_eq!(last, Some(c.local + u64::from(delay)), "delay {delay}, client {k}");
            }
        }
    }

    /// Player 0 and the watcher freeze for five seconds (a hidden tab) while
    /// the others play on, then wake to a backlog.
    fn stall_scenario(cap: u32) -> (Net, u32, ClientStats) {
        let cfg = ClientConfig {
            max_catchup_steps: cap,
            ..ClientConfig::default()
        };
        let mut n = net_with::<4>(3, 1, rough_link(), cfg);
        let mut worst = 0u32;
        run_with(&mut n, 1500, wiggle, |k, t| (k == 0 || k == 3) && (300..600).contains(&t), |_, before, c| {
            let s = c.stats();
            let steps = if c.is_spectator() { s.frames - before.frames } else { s.predicted_steps - before.predicted_steps };
            worst = worst.max(u32::try_from(steps).unwrap_or(u32::MAX));
        });
        let both = n.clients[0].stats();
        (n, worst, both)
    }

    #[test]
    fn a_client_far_behind_never_runs_more_than_the_cap_in_one_frame_and_still_converges() {
        // Uncapped, the wake-up replays far more than 4 steps in one frame
        // (so the cap below has something to hold back).
        let (free, free_worst, _) = stall_scenario(0);
        assert!(free_worst > 8, "the stall did not produce a backlog: {free_worst}");
        converged(&free);

        for cap in [2u32, 4, 8] {
            let (n, worst, player) = stall_scenario(cap);
            assert!(worst <= cap, "cap {cap}: a frame ran {worst} steps");
            assert!(player.catchup_capped > 0, "cap {cap}: never engaged: {player:?}");
            converged(&n);
            for (k, c) in n.clients.iter().enumerate() {
                assert!(c.stats().peak_catchup <= cap, "cap {cap}, client {k}: {:?}", c.stats());
                assert_eq!(c.stats().desyncs, 0, "cap {cap}, client {k}");
            }
            // Back in step: the picture is within a few ticks of the referee.
            let player = &n.clients[0];
            assert!(player.display_tick() + 12 > n.referee.tick(), "cap {cap}: display at {}", player.display_tick());
        }
    }

    #[test]
    fn the_default_catch_up_cap_changes_nothing() {
        assert_eq!(ClientConfig::default().input_delay, 0);
        assert_eq!(ClientConfig::default().max_catchup_steps, 0);
        // No cap: through a five-second stall the client still steps a whole
        // backlog in one frame, as it always did, and the cap never engages.
        let (n, worst, _) = stall_scenario(ClientConfig::default().max_catchup_steps);
        assert!(worst > 240, "{worst}");
        for (k, c) in n.clients.iter().enumerate() {
            assert_eq!(c.stats().catchup_capped, 0, "client {k}");
        }
        converged(&n);
        // The limits: 0 stays off, 1 is raised to 2, large values come down.
        assert_eq!([0, 1, 2, 100, 240, 9999].map(catchup_cap), [0, 2, 2, 100, 240, 240]);
        let mut c = LockstepClient::<Dots>::player(dots(), dots(), ClientConfig::default());
        c.set_max_catchup_steps(1);
        assert_eq!(c.cfg.max_catchup_steps, 2);
        c.set_max_catchup_steps(0);
        assert_eq!(c.cfg.max_catchup_steps, 0);
    }

    #[test]
    fn the_sync_test_catches_a_hidden_input() {
        let (mut a, mut b) = (dots(), dots());
        assert!(sync_test(&mut a, &mut b, 500, |t| Frame {
            tick: t,
            humans: 1,
            inputs: vec![(t % 5) as i8 - 2; 4],
        })
        .is_ok());
    }
}
