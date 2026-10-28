//! A player's side of a session: client-side prediction with rollback, and
//! the interpolation history remote entities are drawn from.
//!
//! Prediction: the client runs ahead of the server by about the one-way
//! delay plus [`NetConfig::target_slack`] ticks, so each input reaches the
//! server just before the server needs it. Every tick it stores its input,
//! steps its own copy of the state with that input (and the other players'
//! last known inputs as a guess), and sends the input with the last few
//! repeated.
//!
//! Reconcile: when a snapshot for tick `s` arrives, the inputs before `s`
//! are done with. If the predicted state at `s` has the snapshot's checksum,
//! the guess was right and nothing changes. Otherwise the client takes the
//! snapshot's state and replays its stored inputs from `s` up to its own
//! tick with [`Role::Resim`] (effects check [`Role::first_time`]).
//!
//! Pacing: each snapshot says how early this client's inputs arrive (the
//! slack). Under the target the client steps two ticks in one frame, over
//! it the client skips a frame, one tick at a time, then waits until the
//! server has seen the change before judging again.

use std::collections::VecDeque;

use crate::config::{ClientStats, NetConfig};
use crate::game::{InputSource, NetGame, PeerInput, StepCtx};
use crate::interp::{PoseHistory, Sampled};
use crate::msg::{
    self, Hello, InputHeader, Kind, MAX_INPUT_FRAMES, RefuseReason, SnapshotHeader, ViewCodec,
    Welcome,
};
use crate::transport::Transport;
use crate::types::{LinkId, NetId, PeerId, Role, Tick, TickTime, alpha_to_sub};
use crate::wire::{Reader, Wire, WireError, Writer, fnv1a64};

/// Ticks between hellos while connecting.
const HELLO_EVERY: u32 = 30;

/// Most ticks one pacing correction moves the client.
const MAX_LEAD_STEP: i32 = 32;

/// Where the client is in the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientStatus {
    /// Not connected; call [`Client::connect`].
    Idle,
    /// Hello sent, waiting for the server.
    Connecting,
    /// Welcomed: predicting once the first snapshot arrives.
    Connected,
    /// The server said no.
    Refused(RefuseReason),
    /// The server dropped this client or closed, or [`Client::disconnect`].
    Closed,
}

/// Slack-driven pacing: how many ticks the client steps each frame so its
/// inputs keep reaching the server [`NetConfig::target_slack`] ticks early.
/// Not generic, so it is compiled once for every game.
#[derive(Debug, Clone, Copy, Default)]
struct Pacing {
    /// Ticks still to gain (+) or give up (-) for the current correction.
    lead: i32,
    /// No new correction until the server acks an input this new.
    adjust_after: Tick,
    /// A correction was running when this frame started.
    correcting: bool,
}

impl Pacing {
    /// Start a correction from the server's slack report, if the last one is
    /// done and the server has seen its effect.
    fn plan(&mut self, slack: i32, target: i32, ack_input: Option<Tick>) {
        if self.lead != 0 {
            return;
        }
        let Some(ack) = ack_input else { return };
        if ack < self.adjust_after {
            return;
        }
        if slack < target {
            self.lead = target.saturating_sub(slack).min(MAX_LEAD_STEP);
        } else if slack > target.saturating_add(1) {
            self.lead = -slack.saturating_sub(target).min(MAX_LEAD_STEP);
        }
    }

    /// Ticks to step this frame: 2 while gaining, 0 while giving up, else 1.
    fn frame_steps(&mut self, stats: &mut ClientStats) -> u32 {
        self.correcting = self.lead != 0;
        if self.lead > 0 {
            self.lead -= 1;
            stats.lead_ups += 1;
            2
        } else if self.lead < 0 {
            self.lead += 1;
            stats.lead_downs += 1;
            0
        } else {
            1
        }
    }

    /// After the frame's steps, with the client now at `tick`: once a
    /// correction is finished, judge the next only on inputs sent after it.
    fn frame_done(&mut self, tick: Tick, snapshot_every: u32) {
        if self.correcting && self.lead == 0 {
            self.adjust_after = tick.saturating_add(snapshot_every);
        }
        self.correcting = false;
    }
}

/// The client side of a session, over any [`Transport`].
pub struct Client<G: NetGame> {
    transport: Box<dyn Transport>,
    cfg: NetConfig,
    status: ClientStatus,
    protocol: u64,
    hello_wait: u32,
    peer: Option<PeerId>,
    seed: u64,
    snapshot_every: u32,
    /// The next tick to predict (the predicted state is at this tick).
    tick: Tick,
    predicted: Option<G::State>,
    /// The newest snapshot accepted.
    snap_tick: Option<Tick>,
    /// This client's inputs from the newest snapshot's tick on, by tick
    /// (consecutive), with the view time each was made at.
    pending: VecDeque<(Tick, G::Input, TickTime)>,
    /// Checksums of predicted states, by tick.
    checksums: VecDeque<(Tick, u64)>,
    /// Every peer's last input as of the newest snapshot (the guess for
    /// the other players), sorted by peer.
    others: Vec<(PeerId, G::Input)>,
    history: PoseHistory<G::Pose>,
    /// The server's sim time as this client sees it now: jumps to each new
    /// snapshot and moves on one tick per [`Client::step`].
    server_time: TickTime,
    /// The view time of the last drawn frame (stamped on inputs).
    view: Option<TickTime>,
    ack_input: Option<Tick>,
    pacing: Pacing,
    /// The newest snapshot from this poll, reconciled once at its end.
    latest: Option<(Tick, G::State)>,
    stats: ClientStats,
    recv: Vec<u8>,
    out: Writer,
    scratch: Writer,
    inputs: Vec<PeerInput<G::Input>>,
    poses: Vec<(NetId, G::Pose)>,
    snap_inputs: Vec<(PeerId, G::Input)>,
}

impl<G: NetGame> Client<G> {
    pub fn new(transport: Box<dyn Transport>, cfg: NetConfig) -> Self {
        Self {
            transport,
            cfg,
            status: ClientStatus::Idle,
            protocol: fnv1a64(G::PROTOCOL.as_bytes()),
            hello_wait: 0,
            peer: None,
            seed: 0,
            snapshot_every: cfg.snapshot_every.max(1),
            tick: 0,
            predicted: None,
            snap_tick: None,
            pending: VecDeque::new(),
            checksums: VecDeque::new(),
            others: Vec::new(),
            history: PoseHistory::new(cfg.pose_history),
            server_time: TickTime::default(),
            view: None,
            ack_input: None,
            pacing: Pacing::default(),
            latest: None,
            stats: ClientStats::default(),
            recv: Vec::new(),
            out: Writer::with_capacity(256),
            scratch: Writer::with_capacity(1024),
            inputs: Vec::new(),
            poses: Vec::new(),
            snap_inputs: Vec::new(),
        }
    }

    /// Say hello (repeated every half second until answered).
    pub fn connect(&mut self) {
        self.status = ClientStatus::Connecting;
        self.send_hello();
    }

    /// Leave the session.
    pub fn disconnect(&mut self) {
        if self.status == ClientStatus::Connected || self.status == ClientStatus::Connecting {
            self.out.clear();
            Kind::ClientBye.write(&mut self.out);
            self.transport.send(LinkId::SERVER, self.out.as_bytes());
        }
        self.status = ClientStatus::Closed;
    }

    pub fn status(&self) -> ClientStatus {
        self.status
    }

    /// This client's peer id, once welcomed.
    pub fn peer(&self) -> Option<PeerId> {
        self.peer
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The next tick to predict.
    pub fn now(&self) -> Tick {
        self.tick
    }

    /// The newest snapshot's tick.
    pub fn snapshot_tick(&self) -> Option<Tick> {
        self.snap_tick
    }

    /// The predicted state (draw the local player from it). `None` until
    /// the first snapshot.
    pub fn predicted(&self) -> Option<&G::State> {
        self.predicted.as_ref()
    }

    /// The snapshot pose frames remote entities are drawn from.
    pub fn history(&self) -> &PoseHistory<G::Pose> {
        &self.history
    }

    pub fn config(&self) -> &NetConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: NetConfig) {
        self.history.set_capacity(cfg.pose_history);
        self.cfg = cfg;
    }

    pub fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    pub fn stats(&self) -> ClientStats {
        let mut s = self.stats;
        s.tick = self.tick;
        s.snapshot_tick = self.snap_tick.unwrap_or(0);
        s
    }

    /// The sim time remote entities are drawn at for a frame with render
    /// `alpha`: the server's time less the interpolation delay. The value
    /// is kept and stamped on the next input, so the server's lag
    /// compensation rewinds to exactly this frame.
    pub fn frame_view(&mut self, alpha: f32) -> TickTime {
        let v = self.view_at(alpha);
        self.view = Some(v);
        v
    }

    fn view_at(&self, alpha: f32) -> TickTime {
        self.server_time
            .add_subs(u64::from(alpha_to_sub(alpha)))
            .sub_subs(self.cfg.interp_delay_subs)
    }

    /// Every networked entity's pose for this frame, interpolated between
    /// snapshots, into `out`. The local player's own entity is in it too:
    /// draw that one from [`Client::predicted`] instead.
    pub fn remote_poses(&mut self, alpha: f32, out: &mut Vec<(NetId, G::Pose)>) -> Sampled {
        let v = self.frame_view(alpha);
        self.history.sample(v, out)
    }

    fn send_hello(&mut self) {
        self.out.clear();
        Hello::new(self.protocol).encode(&mut self.out);
        self.stats.bytes_out += self.out.len() as u64;
        self.transport.send(LinkId::SERVER, self.out.as_bytes());
        self.hello_wait = HELLO_EVERY;
    }

    /// Read everything that has arrived and reconcile with the newest
    /// snapshot. Call once per tick before [`Client::step`].
    pub fn poll(&mut self, game: &G) {
        let mut buf = core::mem::take(&mut self.recv);
        while self.transport.recv(&mut buf).is_some() {
            self.stats.bytes_in += buf.len() as u64;
            if self.handle(game, &buf).is_err() {
                self.stats.bad_packets += 1;
            }
        }
        self.recv = buf;
        if let Some((tick, state)) = self.latest.take() {
            self.reconcile(game, tick, state);
        }
    }

    fn handle(&mut self, game: &G, data: &[u8]) -> Result<(), WireError> {
        let mut r = Reader::new(data);
        match Kind::read(&mut r)? {
            Kind::Welcome => {
                let w = Welcome::decode(&mut r)?;
                if self.status == ClientStatus::Connecting {
                    self.status = ClientStatus::Connected;
                    self.peer = Some(w.peer);
                    self.seed = w.seed;
                    self.snapshot_every = w.snapshot_every.max(1);
                    self.tick = w.tick;
                    self.server_time = TickTime::at(w.tick);
                }
            }
            Kind::Snapshot => self.on_snapshot(game, &mut r)?,
            Kind::Refuse => {
                let why = RefuseReason::decode(&mut r)?;
                if self.status == ClientStatus::Connecting {
                    self.status = ClientStatus::Refused(why);
                }
            }
            Kind::ServerBye => {
                if self.status == ClientStatus::Connected || self.status == ClientStatus::Connecting
                {
                    self.status = ClientStatus::Closed;
                }
            }
            _ => return Err(WireError::Invalid),
        }
        Ok(())
    }

    fn on_snapshot(&mut self, game: &G, r: &mut Reader<'_>) -> Result<(), WireError> {
        if self.status != ClientStatus::Connected {
            return Ok(());
        }
        let h = SnapshotHeader::decode(r)?;
        if h.baseline.is_some() {
            // A delta body against an earlier snapshot: reserved, not read by
            // this version (counted as a bad packet).
            return Err(WireError::Invalid);
        }
        if self.snap_tick.is_some_and(|s| h.tick <= s) {
            // Older than one we have (reordered) or a duplicate.
            return Ok(());
        }
        msg::read_peer_inputs(r, &mut self.snap_inputs)?;
        let state = G::State::read(r)?;

        self.snap_tick = Some(h.tick);
        core::mem::swap(&mut self.others, &mut self.snap_inputs);
        if let Some(a) = h.ack_input {
            self.ack_input = Some(self.ack_input.map_or(a, |b| b.max(a)));
        }
        self.stats.snapshots += 1;
        self.stats.slack = h.slack;
        self.poses.clear();
        game.poses(&state, &mut self.poses);
        self.history.push(h.tick, &self.poses);
        self.server_time = self.server_time.max(TickTime::at(h.tick));
        self.pacing
            .plan(h.slack, self.cfg.target_slack, self.ack_input);
        self.latest = Some((h.tick, state));
        Ok(())
    }

    fn reconcile(&mut self, game: &G, s: Tick, snap: G::State) {
        while self.pending.front().is_some_and(|p| p.0 < s) {
            self.pending.pop_front();
        }

        // Nothing predicted yet, or the server is ahead of us: start over
        // from the snapshot.
        if self.predicted.is_none() || s > self.tick {
            self.predicted = Some(snap);
            self.tick = s;
            self.pending.clear();
            self.checksums.clear();
            self.stats.resyncs += 1;
            return;
        }

        if self.cfg.verify_prediction {
            let guess = self.checksums.iter().find(|c| c.0 == s).map(|c| c.1);
            while self.checksums.front().is_some_and(|c| c.0 <= s) {
                self.checksums.pop_front();
            }
            if guess.is_some_and(|g| g == game.checksum(&snap, &mut self.scratch)) {
                self.stats.confirmed += 1;
                return;
            }
        }

        // Wrong guess: take the server's state and replay our inputs.
        self.checksums.clear();
        let Some(me) = self.peer else { return };
        let mut state = snap;
        let mut last = self.pending.front().map(|p| (p.1.clone(), p.2));
        for t in s..self.tick {
            let found = input_at(&self.pending, t).map(|p| (p.1.clone(), p.2));
            if found.is_some() {
                last = found;
            }
            let (input, view) = last
                .clone()
                .unwrap_or_else(|| (G::Input::default(), TickTime::at(t)));
            predict_step(
                game,
                &mut state,
                Prediction {
                    tick: t,
                    role: Role::Resim,
                    seed: self.seed,
                    me,
                    local: &input,
                    view,
                    others: &self.others,
                    history: &self.history,
                    lag_window_subs: self.cfg.lag_comp_max_subs,
                },
                &mut self.inputs,
            );
            if self.cfg.verify_prediction {
                self.checksums.push_back((
                    t.saturating_add(1),
                    game.checksum(&state, &mut self.scratch),
                ));
            }
        }
        self.predicted = Some(state);
        self.stats.rollbacks += 1;
        self.stats.resim_ticks += u64::from(self.tick.saturating_sub(s));
    }

    /// One frame of play with this frame's `input`: usually one predicted
    /// tick, two when catching up, none when easing off or stalled.
    /// Returns the ticks stepped. Call once per fixed tick, after
    /// [`Client::poll`].
    pub fn step(&mut self, game: &G, input: G::Input) -> u32 {
        match self.status {
            ClientStatus::Connecting => {
                self.hello_wait = self.hello_wait.saturating_sub(1);
                if self.hello_wait == 0 {
                    self.send_hello();
                }
                return 0;
            }
            ClientStatus::Connected => {}
            _ => return 0,
        }
        self.server_time = self.server_time.add_ticks(1);
        let Some(me) = self.peer else { return 0 };
        if self.predicted.is_none() {
            // Waiting for the first snapshot.
            return 0;
        }

        let n = self.pacing.frame_steps(&mut self.stats);

        let view = self.view.unwrap_or_else(|| self.view_at(0.0));
        let mut stepped = 0;
        for _ in 0..n {
            let past = self
                .tick
                .saturating_sub(self.snap_tick.unwrap_or(self.tick));
            if past >= self.cfg.max_predict_ticks {
                self.stats.stalls += 1;
                break;
            }
            let t = self.tick;
            let Some(state) = self.predicted.as_mut() else {
                break;
            };
            predict_step(
                game,
                state,
                Prediction {
                    tick: t,
                    role: Role::Predict,
                    seed: self.seed,
                    me,
                    local: &input,
                    view,
                    others: &self.others,
                    history: &self.history,
                    lag_window_subs: self.cfg.lag_comp_max_subs,
                },
                &mut self.inputs,
            );
            if self.cfg.verify_prediction {
                self.checksums
                    .push_back((t.saturating_add(1), game.checksum(state, &mut self.scratch)));
            }
            self.pending.push_back((t, input.clone(), view));
            self.tick = t.saturating_add(1);
            stepped += 1;
        }
        self.pacing.frame_done(self.tick, self.snapshot_every);
        self.send_inputs();
        stepped
    }

    /// Send the newest inputs, repeating the last few and every one the
    /// server has not acknowledged (up to [`MAX_INPUT_FRAMES`]).
    fn send_inputs(&mut self) {
        let Some(newest) = self.pending.back().map(|p| p.0) else {
            return;
        };
        let unacked = self
            .ack_input
            .map_or(u32::MAX, |a| newest.saturating_sub(a));
        let cap = MAX_INPUT_FRAMES as u32;
        let want = self
            .cfg
            .input_redundancy
            .max(1)
            .max(unacked.min(cap))
            .min(cap);
        let count = usize::try_from(want)
            .unwrap_or(MAX_INPUT_FRAMES)
            .min(self.pending.len());
        // Only a consecutive run, newest first, can be sent.
        let mut run = 0usize;
        for (k, p) in self.pending.iter().rev().take(count).enumerate() {
            let k = u32::try_from(k).unwrap_or(u32::MAX);
            if newest.checked_sub(k) != Some(p.0) {
                break;
            }
            run += 1;
        }
        self.out.clear();
        InputHeader {
            ack_snapshot: self.snap_tick,
            newest,
            count: u8::try_from(run).unwrap_or(u8::MAX),
        }
        .encode(&mut self.out);
        let mut views = ViewCodec::new();
        for (t, input, view) in self.pending.iter().rev().take(run) {
            views.write(&mut self.out, *t, *view);
            input.write(&mut self.out);
        }
        self.stats.bytes_out += self.out.len() as u64;
        self.transport.send(LinkId::SERVER, self.out.as_bytes());
    }
}

/// The stored input for tick `t` (the run is consecutive, so it is found
/// by offset).
fn input_at<I>(pending: &VecDeque<(Tick, I, TickTime)>, t: Tick) -> Option<&(Tick, I, TickTime)> {
    let first = pending.front()?.0;
    let i = usize::try_from(t.checked_sub(first)?).ok()?;
    pending.get(i).filter(|p| p.0 == t)
}

/// One predicted tick's inputs and context.
struct Prediction<'a, G: NetGame> {
    tick: Tick,
    role: Role,
    seed: u64,
    me: PeerId,
    local: &'a G::Input,
    view: TickTime,
    others: &'a [(PeerId, G::Input)],
    history: &'a PoseHistory<G::Pose>,
    lag_window_subs: u64,
}

/// Step `state` one tick on the client: the local input as given, every
/// other peer's last known input as a guess.
fn predict_step<G: NetGame>(
    game: &G,
    state: &mut G::State,
    p: Prediction<'_, G>,
    inputs: &mut Vec<PeerInput<G::Input>>,
) {
    inputs.clear();
    let local = PeerInput {
        peer: p.me,
        input: p.local.clone(),
        view: p.view,
        source: InputSource::Fresh,
    };
    let mut local = Some(local);
    for (peer, input) in p.others {
        if *peer == p.me {
            continue;
        }
        if *peer > p.me {
            inputs.extend(local.take());
        }
        inputs.push(PeerInput {
            peer: *peer,
            input: input.clone(),
            view: TickTime::at(p.tick),
            source: InputSource::Guessed,
        });
    }
    if let Some(l) = local.take() {
        inputs.push(l);
    }
    let ctx = StepCtx::new(
        p.tick,
        p.role,
        p.seed,
        inputs,
        &[],
        &[],
        p.history,
        p.lag_window_subs,
    );
    game.step(state, &ctx);
}
