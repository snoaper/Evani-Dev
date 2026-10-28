//! The authority: it owns the true state, steps it once per tick with every
//! peer's input, and sends each client a snapshot every
//! [`NetConfig::snapshot_every`] ticks.
//!
//! Inputs: each client sends its inputs ahead of time, tagged with the tick
//! they are for. The server queues them per peer and, at each tick, uses
//! the one for that tick ([`InputSource::Fresh`]) or repeats the peer's last
//! ([`InputSource::Repeated`]) when it is late or lost. Bots are peers too:
//! they think on the state at the start of the tick and their inputs go into
//! the same per-peer list ([`InputSource::Bot`]).
//!
//! Lag compensation: at every snapshot tick the server records every
//! entity's pose in a [`PoseHistory`], the same frames clients interpolate
//! between. [`StepCtx::rewind`] samples it at the time the shooter was
//! looking at, so a hit is tested against what the shooter saw.

use std::collections::VecDeque;

use evani_core::rng::Pcg32;

use crate::config::{NetConfig, ServerStats};
use crate::game::{Bot, InputSource, NetGame, PeerInput, StepCtx};
use crate::interp::PoseHistory;
use crate::msg::{
    self, Hello, InputHeader, Kind, RefuseReason, SnapshotHeader, ViewCodec, WIRE_VERSION, Welcome,
};
use crate::transport::Transport;
use crate::types::{LinkId, NetId, PeerId, Role, Tick, TickTime, mix64};
use crate::wire::{Reader, Wire, WireError, Writer, fnv1a64};

/// A remote player's connection state.
struct Remote<I> {
    link: LinkId,
    /// Inputs received for ticks not simulated yet, sorted by tick, one per
    /// tick (the first copy to arrive wins).
    queue: VecDeque<(Tick, I, TickTime)>,
    /// The newest input tick received (sent back as the ack).
    newest: Option<Tick>,
    /// The smallest slack since the last snapshot to this peer.
    window_slack: Option<i32>,
    /// The slack in the last snapshot (repeated when no input came since).
    reported_slack: i32,
    last_heard: Tick,
}

struct BotSlot<G: NetGame> {
    bot: Box<dyn Bot<G>>,
    rng: Pcg32,
    next: Option<G::Input>,
}

enum Driver<G: NetGame> {
    Remote(Remote<G::Input>),
    Bot(BotSlot<G>),
}

struct Peer<G: NetGame> {
    id: PeerId,
    driver: Driver<G>,
    last_input: G::Input,
    last_view: TickTime,
    last_applied: Option<Tick>,
}

/// The authoritative side of a session, over any [`Transport`].
pub struct Server<G: NetGame> {
    transport: Box<dyn Transport>,
    cfg: NetConfig,
    seed: u64,
    protocol: u64,
    tick: Tick,
    state: G::State,
    /// Sorted by [`PeerId`].
    peers: Vec<Peer<G>>,
    next_peer: u16,
    joined: Vec<PeerId>,
    left: Vec<PeerId>,
    history: PoseHistory<G::Pose>,
    stats: ServerStats,
    inputs: Vec<PeerInput<G::Input>>,
    poses: Vec<(NetId, G::Pose)>,
    recv: Vec<u8>,
    closed: Vec<LinkId>,
    out: Writer,
    state_buf: Writer,
    inputs_buf: Writer,
}

impl<G: NetGame> Server<G> {
    /// A server starting at tick 0 with `state`. `seed` is the session seed
    /// every peer's [`StepCtx::rng`] derives from.
    pub fn new(state: G::State, transport: Box<dyn Transport>, cfg: NetConfig, seed: u64) -> Self {
        Self {
            transport,
            cfg,
            seed,
            protocol: fnv1a64(G::PROTOCOL.as_bytes()),
            tick: 0,
            state,
            peers: Vec::new(),
            next_peer: 1,
            joined: Vec::new(),
            left: Vec::new(),
            history: PoseHistory::new(cfg.pose_history),
            stats: ServerStats::default(),
            inputs: Vec::new(),
            poses: Vec::new(),
            recv: Vec::new(),
            closed: Vec::new(),
            out: Writer::with_capacity(256),
            state_buf: Writer::with_capacity(1024),
            inputs_buf: Writer::with_capacity(128),
        }
    }

    /// The next tick to simulate (the state is at this tick).
    pub fn now(&self) -> Tick {
        self.tick
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn state(&self) -> &G::State {
        &self.state
    }

    /// The true state, for setup between ticks (loading a level, a round
    /// reset). Clients get the change in the next snapshot.
    pub fn state_mut(&mut self) -> &mut G::State {
        &mut self.state
    }

    pub fn config(&self) -> &NetConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: NetConfig) {
        self.history.set_capacity(cfg.pose_history);
        self.cfg = cfg;
    }

    /// The pose frames lag compensation rewinds through.
    pub fn history(&self) -> &PoseHistory<G::Pose> {
        &self.history
    }

    pub fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    /// Every peer, in [`PeerId`] order.
    pub fn peer_ids(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.iter().map(|p| p.id)
    }

    pub fn is_bot(&self, peer: PeerId) -> bool {
        self.peer(peer)
            .is_some_and(|p| matches!(p.driver, Driver::Bot(_)))
    }

    /// The input `peer` used on the last tick.
    pub fn last_input(&self, peer: PeerId) -> Option<&G::Input> {
        self.peer(peer).map(|p| &p.last_input)
    }

    /// The last tick `peer` had its own input for (not a repeat).
    pub fn last_applied(&self, peer: PeerId) -> Option<Tick> {
        self.peer(peer).and_then(|p| p.last_applied)
    }

    pub fn stats(&self) -> ServerStats {
        let mut s = self.stats;
        s.tick = self.tick;
        s.clients = 0;
        s.bots = 0;
        for p in &self.peers {
            match p.driver {
                Driver::Remote(_) => s.clients += 1,
                Driver::Bot(_) => s.bots += 1,
            }
        }
        s
    }

    fn peer(&self, id: PeerId) -> Option<&Peer<G>> {
        self.peers
            .binary_search_by_key(&id, |p| p.id)
            .ok()
            .and_then(|i| self.peers.get(i))
    }

    fn find_link(&self, link: LinkId) -> Option<usize> {
        self.peers
            .iter()
            .position(|p| matches!(&p.driver, Driver::Remote(r) if r.link == link))
    }

    /// A free id: the next in turn, skipping ids in use or leaving this
    /// tick (so a game never sees one id leave and join in one step).
    fn alloc_id(&mut self) -> Option<PeerId> {
        if self.peers.len() >= self.cfg.max_peers {
            return None;
        }
        for _ in 0..=u32::from(u16::MAX) {
            let id = PeerId(self.next_peer.max(1));
            self.next_peer = id.0.wrapping_add(1);
            let used =
                self.peers.binary_search_by_key(&id, |p| p.id).is_ok() || self.left.contains(&id);
            if !used {
                return Some(id);
            }
        }
        None
    }

    fn insert(&mut self, peer: Peer<G>) {
        let at = self.peers.partition_point(|p| p.id < peer.id);
        self.joined.push(peer.id);
        self.stats.joins += 1;
        self.peers.insert(at, peer);
    }

    /// Add a server-side bot. `None` when the session is full.
    pub fn add_bot(&mut self, bot: Box<dyn Bot<G>>) -> Option<PeerId> {
        let id = self.alloc_id()?;
        let rng = Pcg32::new(
            mix64(self.seed ^ u64::from(id.0)),
            0x626f_7400 | u64::from(id.0),
        );
        self.insert(Peer {
            id,
            driver: Driver::Bot(BotSlot {
                bot,
                rng,
                next: None,
            }),
            last_input: G::Input::default(),
            last_view: TickTime::at(self.tick),
            last_applied: None,
        });
        Some(id)
    }

    /// Remove a bot or kick a player (who is told with a `Bye`). The game
    /// sees it in [`StepCtx::left`] on the next tick.
    pub fn remove_peer(&mut self, peer: PeerId) -> bool {
        match self.peers.binary_search_by_key(&peer, |p| p.id) {
            Ok(i) => {
                self.drop_at(i, true);
                true
            }
            Err(_) => false,
        }
    }

    fn drop_at(&mut self, i: usize, bye: bool) {
        if i >= self.peers.len() {
            return;
        }
        let peer = self.peers.remove(i);
        if let (Driver::Remote(r), true) = (&peer.driver, bye) {
            self.out.clear();
            Kind::ServerBye.write(&mut self.out);
            self.transport.send(r.link, self.out.as_bytes());
        }
        let id = peer.id;
        let before = self.joined.len();
        self.joined.retain(|&j| j != id);
        // Joined and left before the game saw it: it never existed.
        if self.joined.len() == before {
            self.left.push(id);
        }
        self.stats.leaves += 1;
    }

    /// Read everything that has arrived: joins, inputs, leaves. Call once
    /// per tick before [`Server::tick`].
    pub fn poll(&mut self) {
        let mut closed = core::mem::take(&mut self.closed);
        closed.clear();
        self.transport.closed(&mut closed);
        for link in closed.drain(..) {
            if let Some(i) = self.find_link(link) {
                self.drop_at(i, false);
            }
        }
        self.closed = closed;

        let mut buf = core::mem::take(&mut self.recv);
        while let Some(from) = self.transport.recv(&mut buf) {
            self.stats.bytes_in += buf.len() as u64;
            if self.handle(from, &buf).is_err() {
                self.stats.bad_packets += 1;
            }
        }
        self.recv = buf;
    }

    fn handle(&mut self, from: LinkId, data: &[u8]) -> Result<(), WireError> {
        let mut r = Reader::new(data);
        match Kind::read(&mut r)? {
            Kind::Hello => {
                let h = Hello::decode(&mut r)?;
                self.on_hello(from, h);
            }
            Kind::Input => self.on_input(from, &mut r)?,
            Kind::ClientBye => {
                if let Some(i) = self.find_link(from) {
                    self.drop_at(i, false);
                }
            }
            _ => return Err(WireError::Invalid),
        }
        Ok(())
    }

    fn on_hello(&mut self, from: LinkId, h: Hello) {
        if let Some(id) = self
            .find_link(from)
            .and_then(|i| self.peers.get(i))
            .map(|p| p.id)
        {
            // Already in: the welcome was lost, send it again.
            self.send_welcome(from, id);
            return;
        }
        if h.wire != WIRE_VERSION || h.protocol != self.protocol {
            self.refuse(from, RefuseReason::Protocol);
            return;
        }
        let Some(id) = self.alloc_id() else {
            self.refuse(from, RefuseReason::Full);
            return;
        };
        self.insert(Peer {
            id,
            driver: Driver::Remote(Remote {
                link: from,
                queue: VecDeque::new(),
                newest: None,
                window_slack: None,
                reported_slack: 0,
                last_heard: self.tick,
            }),
            last_input: G::Input::default(),
            last_view: TickTime::at(self.tick),
            last_applied: None,
        });
        self.send_welcome(from, id);
    }

    fn send_welcome(&mut self, link: LinkId, peer: PeerId) {
        self.out.clear();
        Welcome {
            peer,
            tick: self.tick,
            seed: self.seed,
            snapshot_every: self.cfg.snapshot_every.max(1),
        }
        .encode(&mut self.out);
        self.stats.bytes_out += self.out.len() as u64;
        self.transport.send(link, self.out.as_bytes());
    }

    fn refuse(&mut self, link: LinkId, why: RefuseReason) {
        self.stats.refused += 1;
        self.out.clear();
        why.encode(&mut self.out);
        self.transport.send(link, self.out.as_bytes());
    }

    fn on_input(&mut self, from: LinkId, r: &mut Reader<'_>) -> Result<(), WireError> {
        // Unknown link (timed out, or never said hello): ignore.
        let Some(i) = self.find_link(from) else {
            return Ok(());
        };
        let h = InputHeader::decode(r)?;
        let now = self.tick;
        let horizon = now.saturating_add(self.cfg.max_input_ahead.max(1));
        let Some(Driver::Remote(rem)) = self.peers.get_mut(i).map(|p| &mut p.driver) else {
            return Ok(());
        };
        rem.last_heard = now;
        // How early this packet's newest input is; a lying client only
        // changes its own pacing.
        let ahead = i64::from(self.cfg.max_input_ahead);
        let slack = (i64::from(h.newest) - i64::from(now)).clamp(-ahead, ahead) as i32;
        rem.window_slack = Some(rem.window_slack.map_or(slack, |s| s.min(slack)));
        if h.newest < horizon {
            rem.newest = Some(rem.newest.map_or(h.newest, |n| n.max(h.newest)));
        }
        let mut views = ViewCodec::new();
        for k in 0..h.count {
            let Some(t) = h.frame_tick(k) else { break };
            let view = views.read(r, t)?;
            let input = G::Input::read(r)?;
            if t >= now
                && t < horizon
                && let Err(at) = rem.queue.binary_search_by_key(&t, |e| e.0)
            {
                rem.queue.insert(at, (t, input, view));
            }
        }
        Ok(())
    }

    /// Simulate one tick: bots think, every peer's input for this tick is
    /// gathered (or repeated), the game steps, and on a snapshot tick the
    /// poses are recorded and every client is sent the new state.
    pub fn tick(&mut self, game: &G) {
        let now = self.tick;

        let timeout = self.cfg.timeout_ticks;
        if timeout > 0 {
            let mut i = 0;
            while let Some(p) = self.peers.get(i) {
                let stale = matches!(&p.driver, Driver::Remote(r) if now.saturating_sub(r.last_heard) > timeout);
                if stale {
                    self.stats.timeouts += 1;
                    self.drop_at(i, true);
                } else {
                    i += 1;
                }
            }
        }

        for peer in &mut self.peers {
            if let Driver::Bot(b) = &mut peer.driver {
                b.next = Some(b.bot.think(game, &self.state, peer.id, now, &mut b.rng));
            }
        }

        self.inputs.clear();
        for peer in &mut self.peers {
            let (input, view, source) = match &mut peer.driver {
                Driver::Bot(b) => {
                    let input = b.next.take().unwrap_or_else(|| peer.last_input.clone());
                    (input, TickTime::at(now), InputSource::Bot)
                }
                Driver::Remote(r) => {
                    while r.queue.front().is_some_and(|e| e.0 < now) {
                        r.queue.pop_front();
                    }
                    let fresh = if r.queue.front().is_some_and(|e| e.0 == now) {
                        r.queue.pop_front()
                    } else {
                        None
                    };
                    match fresh {
                        Some((_, input, view)) => (input, view, InputSource::Fresh),
                        None => {
                            self.stats.repeated_inputs += 1;
                            // The player kept watching while the input was
                            // missing: their view moves on with time.
                            (
                                peer.last_input.clone(),
                                peer.last_view.add_ticks(1),
                                InputSource::Repeated,
                            )
                        }
                    }
                }
            };
            if source == InputSource::Fresh {
                peer.last_applied = Some(now);
            }
            peer.last_input = input.clone();
            peer.last_view = view;
            self.inputs.push(PeerInput {
                peer: peer.id,
                input,
                view,
                source,
            });
        }

        let ctx = StepCtx::new(
            now,
            Role::Server,
            self.seed,
            &self.inputs,
            &self.joined,
            &self.left,
            &self.history,
            self.cfg.lag_comp_max_subs,
        );
        game.step(&mut self.state, &ctx);
        self.joined.clear();
        self.left.clear();
        self.tick = now.saturating_add(1);

        if self.tick.is_multiple_of(self.cfg.snapshot_every.max(1)) {
            self.snapshot(game);
        }
    }

    fn snapshot(&mut self, game: &G) {
        let tick = self.tick;
        self.poses.clear();
        game.poses(&self.state, &mut self.poses);
        self.history.push(tick, &self.poses);
        if !self
            .peers
            .iter()
            .any(|p| matches!(p.driver, Driver::Remote(_)))
        {
            return;
        }
        // The state and the input list are the same for every client:
        // encode them once.
        self.state_buf.clear();
        self.state.write(&mut self.state_buf);
        self.stats.state_bytes = u32::try_from(self.state_buf.len()).unwrap_or(u32::MAX);
        self.inputs_buf.clear();
        msg::write_peer_inputs(
            &mut self.inputs_buf,
            self.peers.iter().map(|p| (p.id, &p.last_input)),
        );
        for peer in &mut self.peers {
            let Driver::Remote(r) = &mut peer.driver else {
                continue;
            };
            if let Some(s) = r.window_slack.take() {
                r.reported_slack = s;
            }
            self.out.clear();
            SnapshotHeader {
                tick,
                ack_input: r.newest,
                slack: r.reported_slack,
                baseline: None,
            }
            .encode(&mut self.out);
            self.out.bytes(self.inputs_buf.as_bytes());
            self.out.bytes(self.state_buf.as_bytes());
            self.stats.bytes_out += self.out.len() as u64;
            self.transport.send(r.link, self.out.as_bytes());
        }
    }

    /// Tell every client the session is over.
    pub fn shutdown(&mut self) {
        while !self.peers.is_empty() {
            self.drop_at(0, true);
        }
    }
}
