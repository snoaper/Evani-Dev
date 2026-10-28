//! The toy game and the harness the tests share.
//!
//! `Arena`: players move on a square with acceleration, drag, a speed cap
//! and wall bounces (f32 maths: + - * / sqrt only), push each other apart,
//! collect pickups that spawn from `StepCtx::rng`, and count "fire" presses
//! (an edge on a held button, with a lag-compensation rewind). Joins spawn
//! a player at a seeded spot, leaves remove it. Bots seek the nearest
//! pickup.
//!
//! `Harness`: one server (the referee) and any number of clients over one
//! `Loopback`, stepped in the same order `LocalSession` uses. It records the
//! server's state hash at every tick, every snapshot each client received
//! (decoded from the bytes, through a spy transport), and each client's
//! predicted state hash at every tick it stood on.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::rng::Pcg32;
use evani_net::msg::{self, Kind, SnapshotHeader};
use evani_net::wire::{Reader, Wire, Writer};
use evani_net::{
    Bot, Client, LinkConditions, LinkId, Loopback, NetConfig, NetGame, NetId, PeerId, Server,
    StepCtx, Tick, Transport,
};

pub const ARENA: f32 = 20.0;
pub const ACCEL: f32 = 0.05;
pub const DRAG: f32 = 0.96;
pub const MAX_SPEED: f32 = 0.6;
pub const RADIUS: f32 = 0.75;
pub const MAX_PICKUPS: usize = 8;
pub const FIRE: u8 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArenaInput {
    pub dx: i8,
    pub dy: i8,
    pub buttons: u8,
}
evani_net::wire_struct!(ArenaInput { dx, dy, buttons });

impl ArenaInput {
    pub fn new(dx: i8, dy: i8, buttons: u8) -> Self {
        Self { dx, dy, buttons }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub peer: u16,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub score: u32,
    pub prev_buttons: u8,
    pub shots: u32,
    /// The lag-compensation time of the last shot (0: live state).
    pub last_rewind: u64,
}
evani_net::wire_struct!(Player {
    peer,
    x,
    y,
    vx,
    vy,
    score,
    prev_buttons,
    shots,
    last_rewind
});

#[derive(Debug, Clone, PartialEq)]
pub struct ArenaState {
    pub tick: u32,
    pub players: Vec<Player>,
    pub pickups: Vec<(f32, f32)>,
    pub spawned: u32,
}
evani_net::wire_struct!(ArenaState {
    tick,
    players,
    pickups,
    spawned
});

impl ArenaState {
    pub fn new() -> Self {
        Self {
            tick: 0,
            players: Vec::new(),
            pickups: Vec::new(),
            spawned: 0,
        }
    }

    pub fn player(&self, peer: PeerId) -> Option<&Player> {
        self.players.iter().find(|p| p.peer == peer.0)
    }
}

/// Input axes are clamped to [-1, 1] inside the step: a hostile client can
/// send any `i8`, the game only ever uses a legal value.
fn axis(v: i8) -> f32 {
    f32::from(v.clamp(-1, 1))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Arena;

impl NetGame for Arena {
    type Input = ArenaInput;
    type State = ArenaState;
    type Pose = [f32; 2];
    const PROTOCOL: &'static str = "arena-test/1";

    fn step(&self, s: &mut ArenaState, ctx: &StepCtx<'_, ArenaInput, [f32; 2]>) {
        for p in ctx.left {
            s.players.retain(|pl| pl.peer != p.0);
        }
        for &p in ctx.joined {
            if s.player(p).is_none() {
                let mut r = ctx.rng(0x100 + u64::from(p.0));
                s.players.push(Player {
                    peer: p.0,
                    x: r.range_f32(-ARENA, ARENA),
                    y: r.range_f32(-ARENA, ARENA),
                    vx: 0.0,
                    vy: 0.0,
                    score: 0,
                    prev_buttons: 0,
                    shots: 0,
                    last_rewind: 0,
                });
            }
        }
        s.players.sort_by_key(|p| p.peer);

        let mut rewound = Vec::new();
        for pl in &mut s.players {
            let peer = PeerId(pl.peer);
            let inp = ctx.input(peer).map(|p| p.input).unwrap_or_default();
            pl.vx = (pl.vx + axis(inp.dx) * ACCEL) * DRAG;
            pl.vy = (pl.vy + axis(inp.dy) * ACCEL) * DRAG;
            let speed = (pl.vx * pl.vx + pl.vy * pl.vy).sqrt();
            if speed > MAX_SPEED {
                pl.vx = pl.vx / speed * MAX_SPEED;
                pl.vy = pl.vy / speed * MAX_SPEED;
            }
            pl.x += pl.vx;
            pl.y += pl.vy;
            if pl.x.abs() > ARENA {
                pl.x = pl.x.clamp(-ARENA, ARENA);
                pl.vx = -pl.vx * 0.5;
            }
            if pl.y.abs() > ARENA {
                pl.y = pl.y.clamp(-ARENA, ARENA);
                pl.vy = -pl.vy * 0.5;
            }
            if inp.buttons & FIRE != 0 && pl.prev_buttons & FIRE == 0 {
                pl.shots += 1;
                pl.last_rewind = ctx.rewind(peer, &mut rewound).map_or(0, |t| t.0);
            }
            pl.prev_buttons = inp.buttons;
        }

        // Push overlapping players apart, in id order.
        let n = s.players.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = s.players.split_at_mut(j);
                let (pa, pb) = (&mut a[i], &mut b[0]);
                let dx = pb.x - pa.x;
                let dy = pb.y - pa.y;
                let d2 = dx * dx + dy * dy;
                let min = 2.0 * RADIUS;
                if d2 < min * min && d2 > 1e-6 {
                    let d = d2.sqrt();
                    let push = (min - d) * 0.5 / d;
                    pa.x -= dx * push;
                    pa.y -= dy * push;
                    pb.x += dx * push;
                    pb.y += dy * push;
                }
            }
        }

        for pl in &mut s.players {
            let before = s.pickups.len();
            s.pickups.retain(|&(px, py)| {
                let dx = px - pl.x;
                let dy = py - pl.y;
                dx * dx + dy * dy > RADIUS * RADIUS * 4.0
            });
            pl.score += u32::try_from(before - s.pickups.len()).unwrap_or(0);
        }

        if ctx.tick.is_multiple_of(20) && s.pickups.len() < MAX_PICKUPS {
            let mut r = ctx.rng(1);
            s.pickups
                .push((r.range_f32(-ARENA, ARENA), r.range_f32(-ARENA, ARENA)));
            s.spawned += 1;
        }
        s.tick = ctx.tick + 1;
    }

    fn poses(&self, s: &ArenaState, out: &mut Vec<(NetId, [f32; 2])>) {
        for p in &s.players {
            out.push((NetId(u32::from(p.peer)), [p.x, p.y]));
        }
    }
}

/// The arena's state in named parts, for sync-test reports.
pub fn arena_parts(_g: &Arena, s: &ArenaState, out: &mut Vec<(&'static str, u64)>) {
    let mut w = Writer::new();
    s.players.write(&mut w);
    out.push(("players", evani_net::wire::fnv1a64(w.as_bytes())));
    w.clear();
    s.pickups.write(&mut w);
    out.push(("pickups", evani_net::wire::fnv1a64(w.as_bytes())));
    w.clear();
    (s.tick, s.spawned).write(&mut w);
    out.push(("counters", evani_net::wire::fnv1a64(w.as_bytes())));
}

/// Steers to the nearest pickup with a little seeded wobble and fires now
/// and then. While `quiet(tick)` holds it sends no input, so the session
/// can settle.
pub struct SeekBot {
    pub quiet: fn(Tick) -> bool,
}

impl Bot<Arena> for SeekBot {
    fn think(
        &mut self,
        _game: &Arena,
        state: &ArenaState,
        me: PeerId,
        tick: Tick,
        rng: &mut Pcg32,
    ) -> ArenaInput {
        if (self.quiet)(tick) {
            return ArenaInput::default();
        }
        let Some(p) = state.player(me) else {
            return ArenaInput::default();
        };
        let target = state.pickups.iter().copied().min_by(|a, b| {
            let da = (a.0 - p.x) * (a.0 - p.x) + (a.1 - p.y) * (a.1 - p.y);
            let db = (b.0 - p.x) * (b.0 - p.x) + (b.1 - p.y) * (b.1 - p.y);
            da.total_cmp(&db)
        });
        let (mut dx, mut dy) = match target {
            Some((tx, ty)) => (sign(tx - p.x), sign(ty - p.y)),
            None => (0, 0),
        };
        if rng.chance(0.1) {
            dx = rng.range_i32(-1, 1) as i8;
            dy = rng.range_i32(-1, 1) as i8;
        }
        let fire = if rng.chance(0.05) { FIRE } else { 0 };
        ArenaInput::new(dx, dy, fire)
    }
}

fn sign(v: f32) -> i8 {
    if v > 0.25 {
        1
    } else if v < -0.25 {
        -1
    } else {
        0
    }
}

pub fn never_quiet(_t: Tick) -> bool {
    false
}

/// The arena's state hash (the netcode's default checksum).
pub fn hash(s: &ArenaState) -> u64 {
    Arena.checksum(s, &mut Writer::new())
}

// ---- the spy transport ------------------------------------------------------

/// What a spy saw arrive at its client.
#[derive(Debug, Default)]
pub struct SpyLog {
    /// Every snapshot that decoded: its tick and its state's hash.
    pub snapshots: Vec<(Tick, u64)>,
    pub packets: u64,
}

/// Passes everything through and decodes each snapshot it delivers.
pub struct Spy {
    inner: Box<dyn Transport>,
    log: Rc<RefCell<SpyLog>>,
}

impl Spy {
    pub fn new(inner: Box<dyn Transport>) -> (Self, Rc<RefCell<SpyLog>>) {
        let log = Rc::new(RefCell::new(SpyLog::default()));
        (
            Self {
                inner,
                log: Rc::clone(&log),
            },
            log,
        )
    }
}

impl Transport for Spy {
    fn send(&mut self, to: LinkId, packet: &[u8]) {
        self.inner.send(to, packet);
    }

    fn recv(&mut self, buf: &mut Vec<u8>) -> Option<LinkId> {
        let from = self.inner.recv(buf)?;
        let mut log = self.log.borrow_mut();
        log.packets += 1;
        if let Some((tick, state)) = decode_snapshot(buf) {
            log.snapshots.push((tick, hash(&state)));
        }
        Some(from)
    }

    fn closed(&mut self, out: &mut Vec<LinkId>) {
        self.inner.closed(out);
    }
}

/// A snapshot packet's tick and state, if `bytes` is one.
pub fn decode_snapshot(bytes: &[u8]) -> Option<(Tick, ArenaState)> {
    let mut r = Reader::new(bytes);
    if Kind::read(&mut r).ok()? != Kind::Snapshot {
        return None;
    }
    let h = SnapshotHeader::decode(&mut r).ok()?;
    let mut inputs: Vec<(PeerId, ArenaInput)> = Vec::new();
    msg::read_peer_inputs(&mut r, &mut inputs).ok()?;
    let state = ArenaState::read(&mut r).ok()?;
    Some((h.tick, state))
}

// ---- the harness ------------------------------------------------------------

/// What a scripted client does each frame: its input for the tick it is
/// about to predict.
pub type Script = fn(client: usize, tick: Tick) -> ArenaInput;

pub struct Slot {
    pub client: Option<Client<Arena>>,
    pub log: Rc<RefCell<SpyLog>>,
    /// Server tick to connect at.
    pub join_at: Tick,
    /// Server tick to disconnect at (says bye).
    pub leave_at: Option<Tick>,
    /// Server tick from which this client goes silent (no poll, no step).
    pub silent_from: Option<Tick>,
    /// The predicted state's hash at each tick the client stood on (the
    /// last one recorded there), by tick.
    pub predicted: Vec<Option<u64>>,
    /// The peer this client was given.
    pub peer: Option<PeerId>,
}

pub struct Harness {
    pub game: Arena,
    pub net: Loopback,
    pub server: Server<Arena>,
    pub slots: Vec<Slot>,
    /// The server's state hash at every tick (index = tick).
    pub server_hashes: Vec<u64>,
    pub script: Script,
    pub cfg: NetConfig,
}

impl Harness {
    pub fn new(cfg: NetConfig, cond: LinkConditions, seed: u64, script: Script) -> Self {
        let net = Loopback::new(cond, seed ^ 0x6c6f_6f70);
        let state = ArenaState::new();
        let server_hashes = vec![hash(&state)];
        let server = Server::new(state, Box::new(net.server()), cfg, seed);
        Self {
            game: Arena,
            net,
            server,
            slots: Vec::new(),
            server_hashes,
            script,
            cfg,
        }
    }

    /// A client that connects at server tick `join_at`.
    pub fn add_client(&mut self, join_at: Tick) -> usize {
        self.slots.push(Slot {
            client: None,
            log: Rc::new(RefCell::new(SpyLog::default())),
            join_at,
            leave_at: None,
            silent_from: None,
            predicted: Vec::new(),
            peer: None,
        });
        self.slots.len() - 1
    }

    pub fn add_bots(&mut self, n: usize, quiet: fn(Tick) -> bool) -> Vec<PeerId> {
        (0..n)
            .filter_map(|_| self.server.add_bot(Box::new(SeekBot { quiet })))
            .collect()
    }

    /// One tick of the whole session.
    pub fn tick(&mut self) {
        let now = self.server.now();
        self.net.advance();
        let script = self.script;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.client.is_none() && now == slot.join_at {
                let (spy, log) = Spy::new(Box::new(self.net.connect()));
                let mut c = Client::new(Box::new(spy), self.cfg);
                c.connect();
                slot.client = Some(c);
                slot.log = log;
            }
            if slot.leave_at == Some(now) {
                if let Some(c) = slot.client.as_mut() {
                    c.disconnect();
                }
                slot.client = None;
            }
            if slot.silent_from.is_some_and(|s| now >= s) {
                continue;
            }
            let Some(c) = slot.client.as_mut() else {
                continue;
            };
            c.poll(&self.game);
            record(&mut slot.predicted, c);
            let input = script(i, c.now());
            c.step(&self.game, input);
            record(&mut slot.predicted, c);
            if slot.peer.is_none() {
                slot.peer = c.peer();
            }
        }
        self.server.poll();
        self.server.tick(&self.game);
        self.server_hashes.push(hash(self.server.state()));
    }

    /// Step only the server (and the link) until it reaches `tick`: clients
    /// have already sent their inputs up to their own tick.
    pub fn server_until(&mut self, tick: Tick) {
        while self.server.now() < tick {
            self.net.advance();
            self.server.poll();
            self.server.tick(&self.game);
            self.server_hashes.push(hash(self.server.state()));
        }
    }

    pub fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    pub fn client(&self, i: usize) -> Option<&Client<Arena>> {
        self.slots.get(i).and_then(|s| s.client.as_ref())
    }

    /// Every snapshot every client decoded carried exactly the server's
    /// state at that tick. Returns how many were checked.
    pub fn assert_snapshots_match(&self) -> usize {
        let mut n = 0;
        for (i, slot) in self.slots.iter().enumerate() {
            for &(tick, h) in &slot.log.borrow().snapshots {
                let want = self.server_hashes.get(tick as usize).copied();
                assert_eq!(
                    want,
                    Some(h),
                    "client {i}: the snapshot for tick {tick} is not the server's state"
                );
                n += 1;
            }
        }
        n
    }

    /// Every client's prediction for every tick in `ticks` that it stood on
    /// equals the server's state at that tick. Returns the ticks compared.
    pub fn assert_predictions_match(&self, ticks: core::ops::Range<Tick>) -> usize {
        let mut n = 0;
        for (i, slot) in self.slots.iter().enumerate() {
            for t in ticks.clone() {
                let Some(Some(p)) = slot.predicted.get(t as usize).copied() else {
                    continue;
                };
                let Some(&want) = self.server_hashes.get(t as usize) else {
                    continue;
                };
                assert_eq!(
                    p, want,
                    "client {i}: predicted state at tick {t} differs from the server's"
                );
                n += 1;
            }
        }
        n
    }
}

fn record(pred: &mut Vec<Option<u64>>, c: &Client<Arena>) {
    let Some(s) = c.predicted() else { return };
    let t = c.now() as usize;
    if pred.len() <= t {
        pred.resize(t + 1, None);
    }
    pred[t] = Some(hash(s));
}

/// A raw client end for hand-made (hostile) packets.
pub fn raw_hello(protocol: &str, wire: u16) -> Vec<u8> {
    let mut w = Writer::new();
    let mut h = msg::Hello::new(evani_net::wire::fnv1a64(protocol.as_bytes()));
    h.wire = wire;
    h.encode(&mut w);
    w.into_bytes()
}

/// An input packet with `frames` (newest first: ticks newest, newest - 1...).
pub fn raw_input(newest: Tick, frames: &[ArenaInput]) -> Vec<u8> {
    // Frames before tick 0 do not exist.
    let frames = &frames[..frames.len().min(newest as usize + 1)];
    let mut w = Writer::new();
    msg::InputHeader {
        ack_snapshot: None,
        newest,
        count: u8::try_from(frames.len()).unwrap_or(u8::MAX),
    }
    .encode(&mut w);
    let mut views = msg::ViewCodec::new();
    for (k, f) in frames.iter().enumerate() {
        let t = newest - k as u32;
        views.write(&mut w, t, evani_net::TickTime::at(t));
        f.write(&mut w);
    }
    w.into_bytes()
}

/// The default config with a short timeout for tests.
pub fn test_config() -> NetConfig {
    NetConfig {
        timeout_ticks: 120,
        ..NetConfig::default()
    }
}

/// Link conditions from milliseconds at 60 Hz.
pub fn link(latency_ms: u32, jitter_ms: u32, loss_pct: u32) -> LinkConditions {
    LinkConditions {
        latency_ticks: (latency_ms * 60 + 500) / 1000,
        jitter_ticks: (jitter_ms * 60 + 500) / 1000,
        loss_permille: loss_pct * 10,
    }
}
