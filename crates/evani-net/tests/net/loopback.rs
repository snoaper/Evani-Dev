//! Loopback harness tests: several clients and bots against one referee in
//! one process, with simulated latency, jitter and loss.
//!
//! Two kinds of agreement are checked:
//! * every snapshot a client decodes is exactly the referee's state at that
//!   tick (checked on every snapshot of every client);
//! * every client's *prediction* equals the referee's state once inputs
//!   have settled: the scripts alternate busy windows (inputs change, fire
//!   presses, bots roam) with quiet ones (every player and bot holds still),
//!   and from `SETTLE` ticks into each quiet window to its end, the state a
//!   client predicted for each tick must equal the referee's state at that
//!   tick, bit for bit. During busy windows a client cannot know the other
//!   players' newest inputs, so its prediction may differ until corrected;
//!   that is what rollbacks are for.

use evani_net::synctest::first_divergence;
use evani_net::{Client, ClientStatus, LinkConditions, LocalSession, NetConfig, PeerId, Tick};

use crate::support::{
    Arena, ArenaInput, ArenaState, FIRE, Harness, SeekBot, hash, link, never_quiet, test_config,
};

/// Busy for 120 ticks, quiet for 120.
const WINDOW: Tick = 240;
const BUSY: Tick = 120;
/// Ticks into a quiet window before predictions must match: two one-way
/// trips with jitter, a few snapshots, and the client's lead.
const SETTLE: Tick = 60;

fn quiet(t: Tick) -> bool {
    t % WINDOW >= BUSY
}

fn script(c: usize, t: Tick) -> ArenaInput {
    if quiet(t) {
        return ArenaInput::default();
    }
    let k = c as u32;
    let phase = (t / (11 + k) + k) % 5;
    let (dx, dy) = [(1, 0), (1, 1), (0, -1), (-1, 0), (-1, 1)][phase as usize];
    let fire = if (t + k * 5) % 30 < 2 { FIRE } else { 0 };
    ArenaInput::new(dx, dy, fire)
}

/// The settled tick ranges of the first `windows` windows.
fn settled(windows: u32) -> impl Iterator<Item = core::ops::Range<Tick>> {
    (0..windows).map(|w| w * WINDOW + BUSY + SETTLE..(w + 1) * WINDOW)
}

/// Runs `windows` windows, then brings the referee up to the clients' tick.
fn run_windows(h: &mut Harness, windows: u32) {
    h.run(windows * WINDOW);
    let lead = (0..h.slots.len())
        .filter_map(|i| h.client(i).map(Client::now))
        .max()
        .unwrap_or(0);
    h.server_until(lead);
}

fn check_converged(h: &Harness, windows: u32, clients: usize) -> (usize, usize) {
    let snaps = h.assert_snapshots_match();
    let mut preds = 0;
    for r in settled(windows) {
        preds += h.assert_predictions_match(r);
    }
    // Every client compared most of every settled window.
    let per_window = (WINDOW - BUSY - SETTLE) as usize;
    assert!(
        preds >= clients * per_window * windows as usize * 9 / 10,
        "only {preds} predicted ticks compared"
    );
    (snaps, preds)
}

#[test]
fn two_clients_on_a_perfect_link_converge() {
    let mut h = Harness::new(test_config(), LinkConditions::default(), 1, script);
    h.add_client(0);
    h.add_client(0);
    run_windows(&mut h, 2);
    let (snaps, _) = check_converged(&h, 2, 2);
    assert!(snaps > 300);
}

/// The plan's N1 case: 8 clients plus 4 bots, 100 ms latency, 30 ms
/// jitter, 5% loss each way, one client joining late.
#[test]
fn eight_clients_and_bots_converge_under_latency_jitter_and_loss() {
    let mut h = Harness::new(test_config(), link(100, 30, 5), 2, script);
    h.add_bots(4, quiet);
    for _ in 0..7 {
        h.add_client(0);
    }
    let late = h.add_client(300);
    run_windows(&mut h, 4);
    let (snaps, preds) = check_converged(&h, 4, 7);
    assert!(snaps > 7 * 250, "{snaps} snapshots");
    assert_eq!(h.server.stats().clients, 8);
    assert_eq!(h.server.stats().bots, 4);
    assert_eq!(h.server.state().players.len(), 12);
    let late_peer = h.slots[late].peer.expect("the late client joined");
    assert!(h.server.state().player(late_peer).is_some());
    let mut rollbacks = 0;
    for i in 0..h.slots.len() {
        let c = h.client(i).expect("connected");
        assert_eq!(c.status(), ClientStatus::Connected);
        rollbacks += c.stats().rollbacks;
    }
    // Other players' inputs changed under them: rollbacks did the fixing.
    assert!(rollbacks > 0);
    eprintln!(
        "8 clients + 4 bots, 100 ms / 30 ms / 5%: {snaps} snapshots and {preds} settled predicted ticks matched the referee; {rollbacks} rollbacks; loopback {:?}",
        h.net.stats()
    );
}

/// The same session twice, loss and jitter included, is the same session.
#[test]
fn a_whole_session_is_repeatable() {
    let run = || {
        let mut h = Harness::new(test_config(), link(80, 40, 10), 3, script);
        h.add_bots(2, quiet);
        h.add_client(0);
        h.add_client(5);
        h.add_client(100);
        h.run(500);
        let logs: Vec<Vec<(Tick, u64)>> = h
            .slots
            .iter()
            .map(|s| s.log.borrow().snapshots.clone())
            .collect();
        let stats: Vec<_> = (0..3)
            .filter_map(|i| h.client(i).map(Client::stats))
            .collect();
        (
            h.server_hashes,
            logs,
            stats,
            h.server.stats(),
            h.net.stats(),
        )
    };
    let a = run();
    let b = run();
    assert_eq!(first_divergence(&a.0, &b.0), None);
    assert_eq!(a.1, b.1);
    assert_eq!(a.2, b.2);
    assert_eq!(a.3, b.3);
    assert_eq!(a.4, b.4);
}

/// A lone player's prediction is exact once pacing has settled: every
/// snapshot confirms it and nothing is replayed.
#[test]
fn a_lone_player_is_never_rolled_back() {
    fn steady(_c: usize, t: Tick) -> ArenaInput {
        ArenaInput::new(if (t / 50).is_multiple_of(2) { 1 } else { -1 }, 1, 0)
    }
    let mut h = Harness::new(test_config(), link(50, 0, 0), 4, steady);
    let c = h.add_client(0);
    // The first inputs arrive late while pacing catches up (the referee
    // repeats the last one and the client is corrected); after that, none.
    h.run(200);
    let early = h.client(c).unwrap().stats();
    h.run(600);
    let s = h.client(c).unwrap().stats();
    assert!(s.confirmed > 150, "{s:?}");
    assert_eq!(
        s.rollbacks, early.rollbacks,
        "rolled back after settling: {s:?}"
    );
    h.assert_snapshots_match();
}

/// With the client's own inputs changing under latency, the server's state
/// still matches the client's predictions tick for tick (the client knows
/// its own inputs), apart from the start.
#[test]
fn own_inputs_predict_exactly_under_latency() {
    let mut h = Harness::new(test_config(), link(100, 0, 0), 5, script);
    let c = h.add_client(0);
    h.run(480);
    let now = h.client(c).unwrap().now();
    h.server_until(now);
    let n = h.assert_predictions_match(150..now);
    assert!(n > 300, "{n}");
}

/// Pacing moves the client until its inputs arrive just in time.
#[test]
fn pacing_settles_the_slack_near_the_target() {
    let cfg = test_config();
    let mut h = Harness::new(cfg, link(150, 0, 0), 6, script);
    let c = h.add_client(0);
    // Joining 150 ms behind, the client's first inputs are late until it
    // has moved ahead by the round trip.
    h.run(300);
    let early = h.server.stats().repeated_inputs;
    h.run(600);
    let s = h.client(c).unwrap().stats();
    assert!(s.lead_ups > 0, "the client had to run ahead: {s:?}");
    assert!(
        (cfg.target_slack..=cfg.target_slack + 1).contains(&s.slack),
        "slack {} not at the target {}: {s:?}",
        s.slack,
        cfg.target_slack
    );
    // Once settled, every input reaches the referee in time.
    let ss = h.server.stats();
    assert!(early > 0, "{ss:?}");
    assert_eq!(
        ss.repeated_inputs, early,
        "inputs late after settling: {ss:?}"
    );
}

/// Redundant input frames cover lost packets: at 20% loss far fewer than
/// 20% of the referee's input ticks are repeats.
#[test]
fn redundant_inputs_cover_packet_loss() {
    let mut h = Harness::new(test_config(), link(60, 0, 20), 7, script);
    for _ in 0..4 {
        h.add_client(0);
    }
    h.run(1200);
    let ss = h.server.stats();
    let input_ticks = 4 * 1200u64;
    let rate = ss.repeated_inputs as f64 / input_ticks as f64;
    assert!(
        rate < 0.10,
        "repeated {} of {input_ticks} ({:.1}%)",
        ss.repeated_inputs,
        rate * 100.0
    );
    eprintln!("20% loss: {:.2}% of input ticks repeated", rate * 100.0);
    h.assert_snapshots_match();
}

/// A link outage shorter than the prediction limit: the client keeps
/// predicting; a longer one: it stalls. Either way it converges after.
#[test]
fn clients_recover_from_an_outage() {
    for (outage, stalls) in [(10u32, false), (60, true)] {
        let mut h = Harness::new(test_config(), link(50, 10, 0), 8, script);
        h.add_bots(1, quiet);
        let c = h.add_client(0);
        h.add_client(0);
        h.run(BUSY + 10);
        h.net.set_conditions(LinkConditions {
            loss_permille: 1000,
            ..link(50, 10, 0)
        });
        h.run(outage);
        h.net.set_conditions(link(50, 10, 0));
        h.run(2 * WINDOW - BUSY - 10 - outage);
        let lead = (0..2)
            .filter_map(|i| h.client(i).map(Client::now))
            .max()
            .unwrap_or(0);
        h.server_until(lead);
        let s = h.client(c).unwrap().stats();
        assert_eq!(s.stalls > 0, stalls, "outage {outage}: {s:?}");
        assert_eq!(
            h.client(c).map(Client::status),
            Some(ClientStatus::Connected)
        );
        h.assert_snapshots_match();
        let n = h.assert_predictions_match(WINDOW + BUSY + SETTLE..2 * WINDOW);
        assert!(n > 2 * 50, "outage {outage}: {n}");
    }
}

/// Offline play through `LocalSession` (server, client and loopback in one
/// page, bots on the server) settles to the referee's state too.
#[test]
fn local_session_with_bots_settles_to_the_referee() {
    let cfg = NetConfig {
        loopback: link(40, 10, 2),
        ..test_config()
    };
    let mut s = LocalSession::new(Arena, ArenaState::new(), cfg, 9);
    let bots: Vec<PeerId> = (0..3)
        .filter_map(|_| s.add_bot(Box::new(SeekBot { quiet })))
        .collect();
    assert_eq!(bots.len(), 3);
    let mut server = vec![hash(s.authoritative())];
    let mut predicted: Vec<Option<u64>> = Vec::new();
    for _ in 0..2 * WINDOW {
        let t = s.client().now();
        s.tick(script(0, t));
        server.push(hash(s.authoritative()));
        if let Some(p) = s.predicted() {
            let at = s.client().now() as usize;
            if predicted.len() <= at {
                predicted.resize(at + 1, None);
            }
            predicted[at] = Some(hash(p));
        }
    }
    assert!(s.playing());
    let mut n = 0;
    for r in settled(1).chain(std::iter::once(WINDOW + BUSY + SETTLE..2 * WINDOW - 10)) {
        for t in r {
            if let (Some(Some(p)), Some(&want)) =
                (predicted.get(t as usize), server.get(t as usize))
            {
                assert_eq!(*p, want, "tick {t}");
                n += 1;
            }
        }
    }
    assert!(n > 80, "{n}");
    let st = s.stats();
    assert_eq!(st.server.bots, 3);
    assert_eq!(st.server.clients, 1);
    assert_eq!(st.mode, evani_net::NetMode::Loopback);
}

#[test]
fn never_quiet_bots_keep_the_session_busy() {
    // A guard on the harness itself: without quiet windows, predictions
    // of other players' bots do go wrong (so the settled checks above are
    // not passing by accident).
    let mut h = Harness::new(test_config(), link(100, 0, 0), 10, script);
    h.add_bots(3, never_quiet);
    let c = h.add_client(0);
    h.run(WINDOW + BUSY + SETTLE + 40);
    let s = h.client(c).unwrap().stats();
    assert!(s.rollbacks > 5, "{s:?}");
}
