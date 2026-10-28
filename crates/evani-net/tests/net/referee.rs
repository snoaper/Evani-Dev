//! The server as referee: deterministic steps, joins and leaves, bots,
//! refusals, timeouts, and hostile input packets dropped or made harmless.

use evani_net::msg::{Kind, RefuseReason};
use evani_net::wire::{Reader, Wire, Writer};
use evani_net::{
    Client, ClientStatus, InputSource, LinkConditions, LinkId, Loopback, NetConfig, NetGame,
    PeerId, PeerInput, Server, StepCtx, TickTime, Transport, tick_rng,
};

use crate::support::{
    ARENA, Arena, ArenaInput, ArenaState, FIRE, Harness, SeekBot, hash, never_quiet, raw_hello,
    raw_input, test_config,
};

fn idle_script(_c: usize, _t: u32) -> ArenaInput {
    ArenaInput::default()
}

fn busy_script(c: usize, t: u32) -> ArenaInput {
    let phase = (t / 17 + c as u32) % 4;
    let (dx, dy) = [(1, 0), (0, 1), (-1, 0), (0, -1)][phase as usize];
    let fire = if (t + c as u32 * 7) % 40 < 3 { FIRE } else { 0 };
    ArenaInput::new(dx, dy, fire)
}

#[test]
fn tick_rng_is_a_pure_function_of_seed_tick_and_stream() {
    let draw = |seed, tick, stream| {
        let mut r = tick_rng(seed, tick, stream);
        (0..8).map(|_| r.next_u32()).collect::<Vec<u32>>()
    };
    let (a, b) = (draw(7, 100, 1), draw(7, 100, 1));
    assert_eq!(a, b);
    assert_ne!(
        tick_rng(7, 100, 1).next_u32(),
        tick_rng(7, 101, 1).next_u32()
    );
    assert_ne!(
        tick_rng(7, 100, 1).next_u32(),
        tick_rng(7, 100, 2).next_u32()
    );
    assert_ne!(
        tick_rng(7, 100, 1).next_u32(),
        tick_rng(8, 100, 1).next_u32()
    );
}

#[test]
fn step_context_finds_inputs_by_peer() {
    let inputs = vec![
        PeerInput {
            peer: PeerId(1),
            input: ArenaInput::new(1, 0, 0),
            view: TickTime::at(0),
            source: InputSource::Fresh,
        },
        PeerInput {
            peer: PeerId(4),
            input: ArenaInput::new(0, 1, 0),
            view: TickTime::at(0),
            source: InputSource::Bot,
        },
    ];
    let history = evani_net::PoseHistory::new(4);
    let ctx: StepCtx<'_, ArenaInput, [f32; 2]> =
        StepCtx::standalone(3, evani_net::Role::Server, 9, &inputs, &history);
    assert_eq!(
        ctx.input(PeerId(4)).map(|p| p.input),
        Some(ArenaInput::new(0, 1, 0))
    );
    assert!(ctx.input(PeerId(2)).is_none());
    let mut out = Vec::new();
    // No history: lag compensation says "use the live state".
    assert!(ctx.rewind(PeerId(1), &mut out).is_none());
}

/// The same seed, bots and joins give the same server run, tick for tick;
/// another seed gives another run.
#[test]
fn the_referee_is_deterministic() {
    let run = |seed: u64| {
        let mut h = Harness::new(test_config(), LinkConditions::default(), seed, busy_script);
        h.add_bots(4, never_quiet);
        h.add_client(0);
        h.add_client(40);
        h.run(600);
        (h.server_hashes, h.server.stats())
    };
    let (a, sa) = run(11);
    let (b, sb) = run(11);
    assert_eq!(evani_net::synctest::first_divergence(&a, &b), None);
    assert_eq!(sa, sb);
    let (c, _) = run(12);
    assert_ne!(a.last(), c.last(), "the seed changes the run");
}

/// A server with only bots, stepped twice from scratch, agrees on every
/// tick, and its bots really play (they move and score).
#[test]
fn bots_play_deterministically_on_the_referee() {
    let run = || {
        let net = Loopback::new(LinkConditions::default(), 1);
        let mut s: Server<Arena> = Server::new(
            ArenaState::new(),
            Box::new(net.server()),
            NetConfig::default(),
            77,
        );
        let ids: Vec<PeerId> = (0..6)
            .filter_map(|_| s.add_bot(Box::new(SeekBot { quiet: never_quiet })))
            .collect();
        assert_eq!(ids.len(), 6);
        let mut hashes = Vec::new();
        for _ in 0..1200 {
            net.advance();
            s.poll();
            s.tick(&Arena);
            hashes.push(hash(s.state()));
        }
        (hashes, s.state().clone(), ids)
    };
    let (a, state, ids) = run();
    let (b, _, _) = run();
    assert_eq!(a, b);
    assert_eq!(state.players.len(), 6);
    let total: u32 = state.players.iter().map(|p| p.score).sum();
    assert!(total >= 5, "bots should collect pickups (got {total})");
    assert!(
        state
            .players
            .iter()
            .all(|p| p.x.abs() <= ARENA && p.y.abs() <= ARENA)
    );
    for id in ids {
        assert!(state.player(id).is_some());
    }
}

#[test]
fn joins_and_leaves_reach_the_game_once() {
    let mut h = Harness::new(test_config(), LinkConditions::default(), 5, busy_script);
    let bots = h.add_bots(2, never_quiet);
    let c = h.add_client(10);
    h.slots[c].leave_at = Some(200);
    h.run(150);
    let me = h.slots[c].peer.expect("welcomed");
    assert!(
        h.server.state().player(me).is_some(),
        "the client's player spawned"
    );
    assert_eq!(h.server.stats().clients, 1);
    assert_eq!(h.server.state().players.len(), 3);
    h.run(60);
    assert!(
        h.server.state().player(me).is_none(),
        "the player left with its client"
    );
    assert_eq!(h.server.stats().clients, 0);
    assert_eq!(h.server.stats().leaves, 1);
    // Removing a bot takes it out of the game on the next tick.
    assert!(h.server.remove_peer(bots[0]));
    assert!(!h.server.remove_peer(bots[0]), "only once");
    h.run(1);
    assert!(h.server.state().player(bots[0]).is_none());
    assert_eq!(h.server.state().players.len(), 1);
    // A bot added and removed before any tick never existed for the game.
    let ghost = h
        .server
        .add_bot(Box::new(SeekBot { quiet: never_quiet }))
        .unwrap();
    assert!(h.server.remove_peer(ghost));
    h.run(1);
    assert!(h.server.state().player(ghost).is_none());
    assert_eq!(h.server.state().players.len(), 1);
}

/// A freed id is not handed straight back out in the same tick: the game
/// never sees one id leave and join in one step.
#[test]
fn ids_are_not_reused_within_a_tick() {
    let net = Loopback::new(LinkConditions::default(), 1);
    let cfg = NetConfig {
        max_peers: 2,
        ..NetConfig::default()
    };
    let mut s: Server<Arena> = Server::new(ArenaState::new(), Box::new(net.server()), cfg, 1);
    let a = s.add_bot(Box::new(SeekBot { quiet: never_quiet })).unwrap();
    let b = s.add_bot(Box::new(SeekBot { quiet: never_quiet })).unwrap();
    assert!(
        s.add_bot(Box::new(SeekBot { quiet: never_quiet }))
            .is_none(),
        "full"
    );
    s.tick(&Arena);
    assert!(s.remove_peer(a));
    let c = s.add_bot(Box::new(SeekBot { quiet: never_quiet })).unwrap();
    assert_ne!(c, a);
    assert_ne!(c, b);
    s.tick(&Arena);
    let ids: Vec<u16> = s.state().players.iter().map(|p| p.peer).collect();
    assert_eq!(ids, vec![b.0, c.0]);
}

#[test]
fn a_full_server_refuses() {
    let cfg = NetConfig {
        max_peers: 2,
        ..test_config()
    };
    let mut h = Harness::new(cfg, LinkConditions::default(), 3, idle_script);
    h.add_bots(2, never_quiet);
    let c = h.add_client(0);
    h.run(10);
    assert_eq!(
        h.client(c).map(Client::status),
        Some(ClientStatus::Refused(RefuseReason::Full))
    );
    assert_eq!(h.server.stats().refused, 1);
    assert_eq!(h.server.stats().clients, 0);
}

/// A game built with another protocol name, or another wire version, is
/// turned away at the handshake.
#[test]
fn another_protocol_is_refused() {
    #[derive(Clone, Copy)]
    struct Other;
    impl NetGame for Other {
        type Input = ArenaInput;
        type State = ArenaState;
        type Pose = [f32; 2];
        const PROTOCOL: &'static str = "arena-test/2";
        fn step(&self, s: &mut ArenaState, ctx: &StepCtx<'_, ArenaInput, [f32; 2]>) {
            Arena.step(s, ctx);
        }
        fn poses(&self, s: &ArenaState, out: &mut Vec<(evani_net::NetId, [f32; 2])>) {
            Arena.poses(s, out);
        }
    }
    let net = Loopback::new(LinkConditions::default(), 1);
    let mut server: Server<Arena> = Server::new(
        ArenaState::new(),
        Box::new(net.server()),
        NetConfig::default(),
        1,
    );
    let mut other: Client<Other> = Client::new(Box::new(net.connect()), NetConfig::default());
    other.connect();
    let mut raw = net.connect();
    raw.send(LinkId::SERVER, &raw_hello(Arena::PROTOCOL, 99));
    for _ in 0..5 {
        net.advance();
        other.poll(&Other);
        other.step(&Other, ArenaInput::default());
        server.poll();
        server.tick(&Arena);
    }
    assert_eq!(
        other.status(),
        ClientStatus::Refused(RefuseReason::Protocol)
    );
    let mut buf = Vec::new();
    assert!(raw.recv(&mut buf).is_some());
    let mut r = Reader::new(&buf);
    assert_eq!(Kind::read(&mut r), Ok(Kind::Refuse));
    assert_eq!(RefuseReason::decode(&mut r), Ok(RefuseReason::Protocol));
    assert_eq!(server.stats().refused, 2);
    assert_eq!(server.stats().clients, 0);
}

/// A client that goes silent is dropped after the timeout and told so; one
/// that closes its link is dropped at once.
#[test]
fn silent_and_closed_clients_are_dropped() {
    let mut h = Harness::new(test_config(), LinkConditions::default(), 9, busy_script);
    let quiet = h.add_client(0);
    let gone = h.add_client(0);
    let stays = h.add_client(0);
    h.slots[quiet].silent_from = Some(60);
    h.run(60);
    assert_eq!(h.server.stats().clients, 3);
    // Dropping the client end closes its link.
    h.slots[gone].client = None;
    h.run(2);
    assert_eq!(h.server.stats().clients, 2);
    let timeout = test_config().timeout_ticks;
    h.run(timeout + 5);
    assert_eq!(h.server.stats().clients, 1);
    assert_eq!(h.server.stats().timeouts, 1);
    assert_eq!(
        h.client(stays).map(Client::status),
        Some(ClientStatus::Connected)
    );
    // The silent client learns it was dropped when it listens again.
    h.slots[quiet].silent_from = None;
    h.run(2);
    assert_eq!(
        h.client(quiet).map(Client::status),
        Some(ClientStatus::Closed)
    );
}

/// Runs a raw (hand-made) client next to the server; returns the server.
struct Raw {
    net: Loopback,
    server: Server<Arena>,
    end: evani_net::LoopbackEnd,
    peer: PeerId,
}

impl Raw {
    fn new() -> Self {
        let net = Loopback::new(LinkConditions::default(), 1);
        let server: Server<Arena> =
            Server::new(ArenaState::new(), Box::new(net.server()), test_config(), 4);
        let end = net.connect();
        let mut raw = Self {
            net,
            server,
            end,
            peer: PeerId(0),
        };
        raw.end.send(
            LinkId::SERVER,
            &raw_hello(Arena::PROTOCOL, evani_net::msg::WIRE_VERSION),
        );
        raw.tick();
        let Some(peer) = raw.server.peer_ids().next() else {
            panic!("the raw client did not join");
        };
        raw.peer = peer;
        raw
    }

    fn send(&mut self, bytes: &[u8]) {
        self.end.send(LinkId::SERVER, bytes);
    }

    fn tick(&mut self) {
        self.net.advance();
        self.server.poll();
        self.server.tick(&Arena);
        let mut buf = Vec::new();
        while self.end.recv(&mut buf).is_some() {}
    }

    fn me(&self) -> Option<&crate::support::Player> {
        self.server.state().player(self.peer)
    }
}

#[test]
fn garbage_and_wrong_kinds_are_counted_and_dropped() {
    let mut raw = Raw::new();
    let before = raw.me().cloned();
    raw.send(&[]);
    raw.send(&[0xff, 1, 2, 3]);
    raw.send(&[Kind::Snapshot as u8, 0, 0, 0, 0]);
    raw.send(&[Kind::Welcome as u8, 1, 0]);
    raw.send(&[Kind::Input as u8, 0, 5, 200]); // more frames than allowed
    raw.send(&[Kind::Hello as u8, 1]); // truncated hello
    raw.tick();
    assert_eq!(raw.server.stats().bad_packets, 6);
    assert_eq!(
        raw.server.stats().clients,
        1,
        "still connected: bad packets are dropped, not fatal"
    );
    // No input arrived, so the player kept still (its last input repeated).
    let after = raw.me().cloned();
    assert_eq!(
        after.as_ref().map(|p| (p.x, p.y, p.vx, p.vy, p.shots)),
        before.as_ref().map(|p| (p.x, p.y, p.vx, p.vy, p.shots))
    );
}

/// An input whose axis does not fit the input type is refused (it does not
/// decode); one that decodes but is out of the game's range is clamped by
/// the game's own step.
#[test]
fn out_of_range_inputs_never_move_a_player_further() {
    let mut raw = Raw::new();
    let now = raw.server.now();
    let mut w = Writer::new();
    evani_net::msg::InputHeader {
        ack_snapshot: None,
        newest: now,
        count: 1,
    }
    .encode(&mut w);
    w.varint(0); // view: at the tick
    w.zigzag(5000); // dx: not an i8
    w.zigzag(0);
    w.u8(0);
    raw.send(w.as_bytes());
    raw.tick();
    assert_eq!(raw.server.stats().bad_packets, 1);

    // Legal i8s far outside [-1, 1] act like 1 and -1.
    let mut fast = Raw::new();
    let mut fair = Raw::new();
    for _ in 0..60 {
        let t = fast.server.now();
        fast.send(&raw_input(t, &[ArenaInput::new(i8::MAX, i8::MIN, 0)]));
        fair.send(&raw_input(t, &[ArenaInput::new(1, -1, 0)]));
        fast.tick();
        fair.tick();
    }
    assert_eq!(fast.me(), fair.me());
    assert_eq!(
        fast.server.stats().repeated_inputs,
        1,
        "only the join tick had no input"
    );
}

/// Inputs too far ahead are not stored (no ack, no memory); inputs for
/// ticks already simulated are ignored; the first copy of a tick's input
/// wins, so a client cannot rewrite one already sent.
#[test]
fn inputs_outside_the_window_or_resent_are_ignored() {
    let ahead = test_config().max_input_ahead;
    let mut raw = Raw::new();
    let now = raw.server.now();
    raw.send(&raw_input(now + ahead + 5, &[ArenaInput::new(1, 0, 0); 4]));
    raw.send(&raw_input(
        now.saturating_sub(1),
        &[ArenaInput::new(0, 1, 0)],
    ));
    raw.tick();
    let me = raw.me().unwrap().clone();
    assert_eq!(
        (me.vx, me.vy),
        (0.0, 0.0),
        "neither the future nor the past input was used"
    );
    assert_eq!(raw.server.last_applied(raw.peer), None);

    let t = raw.server.now();
    raw.send(&raw_input(t, &[ArenaInput::new(1, 0, 0)]));
    raw.send(&raw_input(t, &[ArenaInput::new(-1, 0, 0)]));
    raw.tick();
    assert_eq!(
        raw.server.last_input(raw.peer),
        Some(&ArenaInput::new(1, 0, 0))
    );
    assert_eq!(raw.server.last_applied(raw.peer), Some(t));
}

/// A flood of copies of the same packet changes nothing: the referee ends
/// in exactly the state one copy gives.
#[test]
fn a_flood_of_inputs_is_harmless() {
    let mut one = Raw::new();
    let mut flood = Raw::new();
    for k in 0..30u32 {
        let t = one.server.now();
        let frames: Vec<ArenaInput> = (0..8)
            .map(|i| ArenaInput::new(((k + i) % 3) as i8 - 1, 1, 0))
            .collect();
        let p = raw_input(t + 3, &frames);
        one.send(&p);
        for _ in 0..500 {
            flood.send(&p);
        }
        one.tick();
        flood.tick();
    }
    assert_eq!(hash(one.server.state()), hash(flood.server.state()));
    assert_eq!(flood.server.stats().bad_packets, 0);
}

/// Input before a hello, from a link the server never welcomed, is ignored.
#[test]
fn input_from_a_stranger_is_ignored() {
    let net = Loopback::new(LinkConditions::default(), 1);
    let mut server: Server<Arena> =
        Server::new(ArenaState::new(), Box::new(net.server()), test_config(), 4);
    server.add_bot(Box::new(SeekBot { quiet: never_quiet }));
    let mut twin: Server<Arena> = Server::new(
        ArenaState::new(),
        Box::new(Loopback::new(LinkConditions::default(), 1).server()),
        test_config(),
        4,
    );
    twin.add_bot(Box::new(SeekBot { quiet: never_quiet }));
    let mut stranger = net.connect();
    for _ in 0..30 {
        stranger.send(
            LinkId::SERVER,
            &raw_input(server.now(), &[ArenaInput::new(1, 1, FIRE)]),
        );
        net.advance();
        server.poll();
        server.tick(&Arena);
        twin.tick(&Arena);
    }
    assert_eq!(server.stats().clients, 0);
    assert_eq!(hash(server.state()), hash(twin.state()));
}

/// Lag compensation never reaches further back than its window, however old
/// the view a client claims.
#[test]
fn a_lying_view_time_is_clamped_to_the_lag_window() {
    let mut raw = Raw::new();
    raw.server.add_bot(Box::new(SeekBot { quiet: never_quiet }));
    for _ in 0..60 {
        raw.tick();
    }
    let t = raw.server.now();
    let mut w = Writer::new();
    evani_net::msg::InputHeader {
        ack_snapshot: None,
        newest: t,
        count: 1,
    }
    .encode(&mut w);
    w.varint(TickTime::at(t).0); // "I was looking at tick 0"
    ArenaInput::new(0, 0, FIRE).write(&mut w);
    raw.send(w.as_bytes());
    raw.tick();
    let me = raw.me().unwrap();
    assert_eq!(me.shots, 1);
    let window = test_config().lag_comp_max_subs;
    assert!(
        me.last_rewind >= TickTime::at(t).0 - window,
        "rewound to {} (window starts {})",
        me.last_rewind,
        TickTime::at(t).0 - window
    );
    assert!(me.last_rewind <= TickTime::at(t).0);
}
