//! The Sugarpeak proof over the in-page loopback: the referee plus 4 and 8
//! simulated players and a spectator, on a link with latency, jitter and
//! loss, through a whole 1-lap race, its podium and the start of the next.
//! Every client's confirmed race, carried on with the referee's own frames,
//! must equal the referee's (same hash, same results), with no desync. It
//! also measures the bytes each player sends and receives.
//!
//! The players drive with the kit's AI on their own *predicted* race (what
//! a person sees), so their inputs are as messy as real ones.
//!
//! `cargo test -p evani-kart-racer-referee --test loopback -- --nocapture`
//! prints the measurements.

// Test helpers may fail loudly (the project allows it in tests).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use evani_kart_racer_referee::{KartBrains, NetRace, SugarpeakReferee, client_copy, referee};
use evani_kit_kart_racer::race::Phase;
use evani_net::lockstep::{Brains, ClientConfig, LockstepClient, LockstepGame};
use evani_net::transport::{LinkConditions, Loopback, LoopbackEnd, Transport};
use evani_net::types::LinkId;
use evani_net::wire::{Reader, Writer};

struct Peer {
    end: LoopbackEnd,
    id: u32,
    client: LockstepClient<NetRace>,
    driver: KartBrains,
    up: u64,
    down: u64,
    up_msgs: u64,
    down_msgs: u64,
}

struct Report {
    ticks: u64,
    peers: Vec<(bool, u64, u64, u64, u64)>,
    races: u64,
    rollbacks: u64,
    desyncs: u64,
    hash_ok: u64,
    states: u64,
    view_max: usize,
}

/// Run one session: `players` humans (seats 0..players), one spectator, the
/// referee stepping every 3 ticks (a 50 ms host) and every client sending
/// every 3 ticks (20 messages a second, under a 30/s cap).
fn session(players: usize, cond: LinkConditions, seed: u64) -> (SugarpeakReferee, Vec<Peer>, Report) {
    let net = Loopback::new(cond, seed);
    let mut server = net.server();
    let mut r = referee(seed, &[1]).expect("referee");
    let mut peers: Vec<Peer> = Vec::new();
    for k in 0..=players {
        let end = net.connect();
        let id = end.link().0;
        let spectator = k == players;
        let seat = r.join(id, spectator);
        assert_eq!(seat.is_none(), spectator);
        let (a, b) = (client_copy().expect("copy"), client_copy().expect("copy"));
        let client = if spectator { LockstepClient::spectator(a, b, ClientConfig::default()) } else { LockstepClient::player(a, b, ClientConfig::default()) };
        peers.push(Peer {
            end,
            id,
            client,
            driver: KartBrains::new(seed ^ ((k as u64 + 1) * 0x9E37)),
            up: 0,
            down: 0,
            up_msgs: 0,
            down_msgs: 0,
        });
    }
    let mut buf = Vec::new();
    let mut w = Writer::new();
    let mut view = Writer::new();
    let mut view_max = 0;
    let mut t = 0u64;
    let mut seen_finish = false;
    loop {
        t += 1;
        net.advance();
        while let Some(from) = server.recv(&mut buf) {
            let _ = r.input(from.0, &buf);
        }
        if t.is_multiple_of(3) {
            r.step(3);
            for p in &mut peers {
                r.view(Some(p.id), &mut view);
                view_max = view_max.max(view.len());
                p.down += view.len() as u64;
                p.down_msgs += 1;
                server.send(LinkId(p.id), view.as_bytes());
            }
        }
        for p in &mut peers {
            while p.end.recv(&mut buf).is_some() {
                let _ = p.client.on_view(&buf);
            }
            let input = match (p.client.is_spectator(), p.client.seat()) {
                (false, Some(seat)) if p.client.synced() => {
                    let tick = p.client.display_tick() + 1;
                    Some(p.driver.think(p.client.display(), usize::from(seat), tick))
                }
                (false, _) => Some(Default::default()),
                (true, _) => None,
            };
            p.client.tick(input);
            if t % 3 == 1 {
                p.client.packet(&mut w);
                if !w.is_empty() {
                    p.up += w.len() as u64;
                    p.up_msgs += 1;
                    p.end.send(LinkId::SERVER, w.as_bytes());
                }
            }
        }
        let (phase, race_no) = {
            let g = r.game().race();
            (g.phase, g.race_no)
        };
        if phase == Phase::Finished {
            seen_finish = true;
        }
        // The podium has shown and the next race is under way.
        if seen_finish && race_no >= 3 && phase == Phase::Racing {
            break;
        }
        assert!(t < 60 * 60 * 6, "no finish in six minutes");
    }
    let report = Report {
        ticks: t,
        peers: peers.iter().map(|p| (p.client.is_spectator(), p.up, p.down, p.up_msgs, p.down_msgs)).collect(),
        races: r.game().race().race_no,
        rollbacks: peers.iter().map(|p| p.client.stats().rollbacks).sum(),
        desyncs: peers.iter().map(|p| p.client.stats().desyncs).sum(),
        hash_ok: peers.iter().map(|p| p.client.stats().hash_ok).sum(),
        states: peers.iter().map(|p| p.client.stats().states).sum(),
        view_max,
    };
    (r, peers, report)
}

/// Carry each client's confirmed race on with the referee's frames to the
/// referee's tick: it must be the referee's race exactly.
fn converged(r: &SugarpeakReferee, peers: &[Peer]) {
    let (mut rw, mut cw) = (Writer::new(), Writer::new());
    r.game().results(&mut rw);
    for (k, p) in peers.iter().enumerate() {
        let c = &p.client;
        assert!(c.synced(), "peer {k} never synced");
        let at = c.confirmed_tick();
        let mut s = Writer::new();
        c.confirmed().save(&mut s);
        let mut g = client_copy().expect("copy");
        g.load(&mut Reader::new(s.as_bytes())).expect("load");
        let mut replayed = 0;
        for f in r.history().filter(|f| f.tick > at) {
            g.step(f);
            replayed += 1;
        }
        assert!(replayed < 60, "peer {k} is {replayed} ticks behind");
        assert_eq!(g.hash(), r.hash(), "peer {k} (spectator: {}) diverged from the referee (confirmed at {at})", c.is_spectator());
        cw.clear();
        g.results(&mut cw);
        assert_eq!(cw.as_bytes(), rw.as_bytes(), "peer {k}: other results");
        assert_eq!(c.stats().desyncs, 0, "peer {k}: {:?}", c.stats());
    }
}

fn print(name: &str, rep: &Report) {
    let secs = rep.ticks as f64 / 60.0;
    let players: Vec<_> = rep.peers.iter().filter(|p| !p.0).collect();
    let up = players.iter().map(|p| p.1).sum::<u64>() as f64 / players.len() as f64 / secs;
    let down = players.iter().map(|p| p.2).sum::<u64>() as f64 / players.len() as f64 / secs;
    let spec = rep.peers.iter().find(|p| p.0).map_or(0.0, |p| p.2 as f64 / secs);
    let msgs_up = players.iter().map(|p| p.3).sum::<u64>() as f64 / players.len() as f64 / secs;
    let msgs_down = players.iter().map(|p| p.4).sum::<u64>() as f64 / players.len() as f64 / secs;
    println!(
        "{name}: {secs:.0} s simulated, races {}; per player up {up:.0} B/s ({msgs_up:.1} msg/s), down {down:.0} B/s ({msgs_down:.1} msg/s); spectator down {spec:.0} B/s; biggest view {} B; rollbacks {}, hash checks passed {}, desyncs {}, full states {}",
        rep.races, rep.view_max, rep.rollbacks, rep.hash_ok, rep.desyncs, rep.states
    );
}

fn lossy() -> LinkConditions {
    // 100 ms one way, up to 33 ms of jitter, 2 % loss.
    LinkConditions {
        latency_ticks: 6,
        jitter_ticks: 2,
        loss_permille: 20,
    }
}

#[test]
fn four_players_and_a_spectator_converge_to_the_referee() {
    let (r, peers, rep) = session(4, lossy(), 11);
    print("4 players + spectator", &rep);
    converged(&r, &peers);
    assert!(rep.hash_ok > 20, "hash checks ran: {}", rep.hash_ok);
}

#[test]
fn eight_players_and_a_spectator_converge_to_the_referee() {
    let (r, peers, rep) = session(8, lossy(), 23);
    print("8 players + spectator", &rep);
    converged(&r, &peers);
    // The general web mode sends each view as JSON {t:"data",id,part,of,b}
    // with b base64, split at 3000 characters: every message under 4 KB. A
    // view here never even needs a second part.
    let b64 = rep.view_max.div_ceil(3) * 4;
    let msg = b64.min(3000) + r#"{"t":"data","id":4294967295,"part":0,"of":1,"b":""}"#.len();
    println!("web mode: largest view {} B -> {b64} B base64 -> largest JSON message {msg} B (cap 4096), parts {}", rep.view_max, b64.div_ceil(3000));
    assert!(msg <= 4096 && b64 <= 3000);
    // Every seat human: the AI drives nobody.
    assert_eq!(r.humans().count_ones(), 8);
}

#[test]
fn a_player_who_leaves_gets_a_bot_and_a_rejoin_is_a_new_seat() {
    let mut r = referee(5, &[1]).expect("referee");
    assert_eq!(r.join(10, false), Some(0));
    assert_eq!(r.join(11, false), Some(1));
    r.step(300);
    assert!(r.leave(10));
    r.step(30);
    assert_eq!(r.humans(), 0b10);
    // A reconnect is a new join with a new random id: the lowest free seat.
    assert_eq!(r.join(99, false), Some(0));
    // Checkpoint and restore mid-race: carries on bit for bit.
    let mut w = Writer::new();
    r.save(&mut w);
    let mut copy = referee(5, &[1]).expect("referee");
    copy.load(w.as_bytes()).expect("load");
    r.step(600);
    copy.step(600);
    assert_eq!(r.hash(), copy.hash());
}

/// A copy loaded from the referee's state at many points and stepped with
/// the referee's own frames must equal the referee on every tick, through
/// finishes and restarts (the bit-for-bit save/load guard).
#[test]
fn a_loaded_copy_steps_exactly_like_the_referee_through_restarts() {
    let mut r = referee(31, &[1]).expect("referee");
    let mut copy = client_copy().expect("copy");
    let mut w = Writer::new();
    let mut loaded_at = 0;
    let mut races = 0;
    for t in 1..=60 * 150u64 {
        if t == 60 * 75 {
            // Half-way, two humans sit down (the first human gets a fresh grid).
            r.join(1, false);
            r.join(2, false);
        }
        races = races.max(r.game().race().race_no);
        if t % 700 == 1 {
            w.clear();
            r.game().save(&mut w);
            copy.load(&mut Reader::new(w.as_bytes())).expect("load");
            loaded_at = t - 1;
        }
        r.step(1);
        let f = r.history().last().expect("frame").clone();
        copy.step(&f);
        if copy.hash() != r.hash() {
            let (mut a, mut b) = (Writer::new(), Writer::new());
            r.game().save(&mut a);
            copy.save(&mut b);
            let at = a.as_bytes().iter().zip(b.as_bytes()).position(|(x, y)| x != y);
            let g = r.game().race();
            panic!("tick {t} (copy loaded at {loaded_at}): states differ at byte {at:?} of {} / {}; phase {:?} race {}", a.len(), b.len(), g.phase, g.race_no);
        }
    }
    assert!(races >= 3, "the AI races finished and restarted ({races})");
}
