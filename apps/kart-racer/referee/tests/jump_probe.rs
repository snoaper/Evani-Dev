//! PROBE: how far every kart a player sees jumps in one frame online (its
//! own kart, the other human's and the AI racers'), beyond its own motion,
//! on a link like a ChimpVibe session (the host steps the referee 3 ticks
//! every 50 ms and sends each player a view; 100-150 ms each way, jitter,
//! loss). Two players steer with keys. `cargo test -p
//! evani-kart-racer-referee --test jump_probe -- --ignored --nocapture`
//! prints the figures with the game's drawing smoother off (`raw`, the
//! picture before the fix) and on (`smooth`, what the game draws now).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout)]

use std::collections::HashMap;

use evani_kart_racer_referee::{KartBrains, KartInput, NetRace, SEATS, client_copy, referee};
use evani_kit_kart_racer::kart::Kart;
use evani_kit_kart_racer::net::{GUESS_HUMANS_AFTER, NetSmooth};
use evani_kit_kart_racer::race::Phase;
use evani_net::lockstep::{Brains, ClientConfig, LockstepClient};
use evani_net::transport::{LinkConditions, Loopback, Transport};
use evani_net::types::LinkId;
use evani_net::wire::Writer;

fn karts(g: &NetRace) -> Vec<Kart> {
    let r = g.race();
    r.grid.iter().filter_map(|&e| g.world().get::<Kart>(e).map(|k| k.clone())).collect()
}

/// A snap: a one-frame jump beyond the kart's own motion of more than this.
pub const SNAP_M: f32 = 0.10;
/// A big snap.
pub const BIG_M: f32 = 0.50;

#[derive(Default, Debug, Clone)]
pub struct Group {
    pub frames: u64,
    pub max: f32,
    pub p99: f32,
    pub snaps: u64,
    pub big: u64,
    jumps: Vec<f32>,
}

impl Group {
    fn add(&mut self, j: f32) {
        self.frames += 1;
        self.max = self.max.max(j);
        if j > SNAP_M {
            self.snaps += 1;
        }
        if j > BIG_M {
            self.big += 1;
        }
        self.jumps.push(j);
    }

    fn finish(&mut self) {
        let v = &mut self.jumps;
        v.sort_by(|a, b| a.total_cmp(b));
        self.p99 = v.get(((v.len().max(1) - 1) as f32 * 0.99).round() as usize).copied().unwrap_or(0.0);
    }
}

#[derive(Default, Debug)]
pub struct Figures {
    pub own: Group,
    pub human: Group,
    pub ai: Group,
    /// Frames whose shown tick moved by 0 or 2+ (the client's pacing nudges
    /// and catch-ups).
    pub pacing: u64,
    /// Game steps one frame re-ran (the rollback replay): mean and most.
    pub replay_mean: f32,
    pub replay_max: u64,
    /// How far the shown tick ran past the confirmed one (ticks).
    pub horizon_mean: f32,
    pub horizon_max: u64,
    /// Ticks between views reaching player 0: mean and longest (a late
    /// view shows as a long gap).
    pub view_gap_mean: f32,
    pub view_gap_max: u64,
    /// The shown kart vs the referee's at the same tick, mean (m): own kart,
    /// AI karts.
    pub own_err_mean: f32,
    pub ai_err_mean: f32,
    /// Teleports (respawns) not counted as snaps.
    pub cuts: u64,
    /// Full states player 0 loaded (join, desync, gap) and its desyncs.
    pub states: u64,
    pub desyncs: u64,
}

pub fn link(one_way_ms: u32, jitter_ms: u32, loss: u32) -> LinkConditions {
    LinkConditions {
        latency_ticks: (one_way_ms as f32 / 16.667).round() as u32,
        jitter_ticks: (jitter_ms as f32 / 16.667).round() as u32,
        loss_permille: loss,
    }
}

/// `smooth`: draw through the game's [`NetSmooth`] (as the game does now)
/// or straight from the prediction (as before). `guess`: the prediction
/// guesses the AI racers' inputs with their own driving (now) or holds
/// their last ones (before).
pub fn run(cond: LinkConditions, seed: u64, secs: u64, smooth: bool, guess: bool, humans_after: Option<u64>) -> Figures {
    let net = Loopback::new(cond, seed);
    let mut server = net.server();
    let mut r = referee(seed, &[9]).expect("referee");
    struct P {
        end: evani_net::transport::LoopbackEnd,
        id: u32,
        c: LockstepClient<NetRace>,
        brain: KartBrains,
        held: i8,
    }
    let mut ps: Vec<P> = (0..2)
        .map(|k| {
            let end = net.connect();
            let id = end.link().0;
            r.join(id, false).expect("seat");
            P {
                end,
                id,
                c: LockstepClient::player(client_copy().unwrap(), predicted(guess, humans_after), ClientConfig::default()),
                brain: KartBrains::new(seed ^ ((k as u64 + 7) * 0x9E37)),
                held: 0,
            }
        })
        .collect();
    let mut truth: HashMap<u64, Vec<Kart>> = HashMap::new();
    // What player 0 drew at each tick (first time it was drawn).
    let mut drawn: HashMap<u64, Vec<[f32; 3]>> = HashMap::new();
    let mut sm = NetSmooth::default();
    let mut f = Figures::default();
    let (mut buf, mut w, mut view) = (Vec::new(), Writer::new(), Writer::new());
    let mut prev: Option<(u64, Vec<[f32; 3]>)> = None;
    let mut racing = 0u64;
    let (mut hsum, mut rsum, mut gsum, mut gn) = (0u64, 0u64, 0u64, 0u64);
    let mut last_view = 0u64;
    let (mut ai_err, mut ai_n, mut own_err, mut own_n) = (0.0f32, 0u64, 0.0f32, 0u64);
    let mut t = 0u64;
    while racing < secs * 60 {
        t += 1;
        assert!(t < 60 * 600, "never raced");
        net.advance();
        while let Some(from) = server.recv(&mut buf) {
            let _ = r.input(from.0, &buf);
        }
        if t.is_multiple_of(3) {
            for _ in 0..3 {
                r.step(1);
                truth.insert(r.tick(), karts(r.game()));
            }
            for p in &ps {
                r.view(Some(p.id), &mut view);
                server.send(LinkId(p.id), view.as_bytes());
            }
        }
        let steps_before = ps[0].c.stats().predicted_steps;
        for (i, p) in ps.iter_mut().enumerate() {
            while p.end.recv(&mut buf).is_some() {
                let _ = p.c.on_view(&buf);
                if i == 0 {
                    if last_view > 0 {
                        let g = t - last_view;
                        gsum += g;
                        gn += 1;
                        f.view_gap_max = f.view_gap_max.max(g);
                    }
                    last_view = t;
                }
            }
            let input = if p.c.synced() && p.c.seat().is_some() {
                let seat = usize::from(p.c.seat().unwrap());
                let mut a: KartInput = p.brain.think(p.c.display(), seat, p.c.display_tick() + 1);
                // A person does not drive the AI's line: wander across the
                // road (a few metres, over a few seconds), so a guess from
                // the AI's line is not handed the answer.
                let ph = t as f32 / 60.0 * std::f32::consts::TAU / (3.0 + i as f32);
                a.steer = (f32::from(a.steer) + 45.0 * ph.sin()).clamp(-127.0, 127.0) as i8;
                // A keyboard: full lock or nothing, with some hold.
                if a.steer > 45 {
                    p.held = 127;
                } else if a.steer < -45 {
                    p.held = -127;
                } else if a.steer.abs() < 13 {
                    p.held = 0;
                }
                a.steer = p.held;
                a.throttle = 255;
                a
            } else {
                KartInput::default()
            };
            p.c.tick(Some(input));
            if t % 3 == 1 {
                p.c.packet(&mut w);
                if !w.is_empty() {
                    p.end.send(LinkId::SERVER, w.as_bytes());
                }
            }
        }
        let c = &ps[0].c;
        if !c.synced() || c.display().race().phase != Phase::Racing {
            prev = None;
            sm.reset();
            continue;
        }
        racing += 1;
        let dt = c.display_tick();
        let raw = karts(c.display());
        let mut shown = raw.clone();
        if smooth {
            sm.apply(&mut shown, 1.0 / 60.0);
        }
        let replay = c.stats().predicted_steps - steps_before;
        rsum += replay;
        f.replay_max = f.replay_max.max(replay);
        let h = dt.saturating_sub(c.confirmed_tick());
        hsum += h;
        f.horizon_max = f.horizon_max.max(h);
        drawn.entry(dt).or_insert_with(|| shown.iter().map(|k| k.pos).collect());
        if let Some((pt, pp)) = &prev {
            if dt != pt + 1 {
                f.pacing += 1;
            }
            for s in 0..SEATS.min(shown.len()) {
                // The kart's own motion in one frame: one tick of the guess.
                let step = [raw[s].pos[0] - raw[s].prev_pos[0], raw[s].pos[2] - raw[s].prev_pos[2]];
                let moved = [shown[s].pos[0] - pp[s][0], shown[s].pos[2] - pp[s][2]];
                let j = ((moved[0] - step[0]).powi(2) + (moved[1] - step[1]).powi(2)).sqrt();
                if j > 10.0 {
                    f.cuts += 1;
                    continue;
                }
                match s {
                    0 => f.own.add(j),
                    1 => f.human.add(j),
                    _ => f.ai.add(j),
                }
            }
        }
        prev = Some((dt, shown.iter().map(|k| k.pos).collect()));
    }
    for (tick, d) in &drawn {
        let Some(tr) = truth.get(tick) else { continue };
        for (s, p) in d.iter().enumerate() {
            let e = ((p[0] - tr[s].pos[0]).powi(2) + (p[2] - tr[s].pos[2]).powi(2)).sqrt();
            // A respawn is not a guessing error.
            if e < 10.0 {
                if s == 0 {
                    own_err += e;
                    own_n += 1;
                } else if s >= 2 {
                    ai_err += e;
                    ai_n += 1;
                }
            }
        }
    }
    f.states = ps[0].c.stats().states;
    f.desyncs = ps[0].c.stats().desyncs;
    f.own.finish();
    f.human.finish();
    f.ai.finish();
    f.horizon_mean = hsum as f32 / racing.max(1) as f32;
    f.replay_mean = rsum as f32 / racing.max(1) as f32;
    f.view_gap_mean = gsum as f32 / gn.max(1) as f32;
    f.ai_err_mean = ai_err / ai_n.max(1) as f32;
    f.own_err_mean = own_err / own_n.max(1) as f32;
    f
}

fn predicted(guess: bool, humans_after: Option<u64>) -> NetRace {
    let mut g = client_copy().unwrap();
    g.set_guess_bots(guess);
    g.set_guess_humans(humans_after);
    g
}

fn g(x: &Group) -> String {
    format!("{:.3}/{:.3} {}/{}", x.p99, x.max, x.snaps, x.big)
}

#[test]
#[ignore]
fn probe_kart_jumps() {
    println!("link | mode | own: p99/max m, snaps>10cm/>50cm | other human | AI karts | frames per kart group (own) | pacing frames | replay mean/max steps | horizon mean/max ticks | view gap mean/max ticks | shown vs referee own/AI mean m | cuts");
    for &(ms, jit, loss) in &[(100, 30, 0), (125, 40, 10), (150, 30, 20)] {
        for seed in [21u64, 22] {
            for (mode, smooth, guess, ha) in [("before", false, false, None), ("smoothing only", true, false, None), ("guessing only", false, true, GUESS_HUMANS_AFTER), ("after", true, true, GUESS_HUMANS_AFTER)] {
                let f = run(link(ms, jit, loss), seed, 40, smooth, guess, ha);
                println!(
                    "{ms}+-{jit}ms loss {loss}/1000 seed {seed} | {} | {} | {} | {} | {} | {} | {:.1}/{} | {:.1}/{} | {:.2}/{} | {:.3}/{:.3} | {} | states {} desyncs {}",
                    mode,
                    g(&f.own),
                    g(&f.human),
                    g(&f.ai),
                    f.own.frames,
                    f.pacing,
                    f.replay_mean,
                    f.replay_max,
                    f.horizon_mean,
                    f.horizon_max,
                    f.view_gap_mean,
                    f.view_gap_max,
                    f.own_err_mean,
                    f.ai_err_mean,
                    f.cuts,
                    f.states,
                    f.desyncs
                );
            }
        }
    }
}

/// The fix holds: on a 125 ms link with jitter and loss, no kart the player
/// sees (its own, the other human's, the AI's) jumps more than [`BIG_M`] in
/// a frame beyond its own motion, and 99 % of frames stay under 20 cm.
#[test]
fn karts_do_not_snap_on_a_laggy_link() {
    let f = run(link(125, 40, 10), 21, 20, true, true, GUESS_HUMANS_AFTER);
    for (name, x) in [("own", &f.own), ("human", &f.human), ("ai", &f.ai)] {
        assert!(x.frames > 1000, "{name}: {} frames", x.frames);
        assert_eq!(x.big, 0, "{name}: max {} m", x.max);
        assert!(x.p99 < 0.2, "{name}: p99 {} m", x.p99);
    }
}
