//! PROBE: how far the kart a player sees (the client's prediction) is from
//! the referee's kart at the same tick, and how far it jumps in one frame,
//! while two players steer with keys (full lock or nothing) on a link with
//! latency and jitter. `cargo test -p evani-kart-racer-referee --test
//! steer_probe -- --ignored --nocapture` prints the figures.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout)]

use std::collections::HashMap;

use evani_kart_racer_referee::{KartBrains, KartInput, NetRace, client_copy, referee};
use evani_kit_kart_racer::kart::{Kart, wrap_angle};
use evani_kit_kart_racer::race::Phase;
use evani_net::lockstep::{Brains, ClientConfig, LockstepClient};
use evani_net::transport::{LinkConditions, Loopback, Transport};
use evani_net::types::LinkId;
use evani_net::wire::Writer;

fn pose(g: &NetRace, seat: usize) -> Option<Kart> {
    let r = g.race();
    let e = *r.grid.get(seat)?;
    g.world().get::<Kart>(e).map(|k| k.clone())
}

#[derive(Default, Debug)]
pub struct Figures {
    pub late: u64,
    pub repeated: u64,
    /// Late inputs while racing, and the lead the referee reported (ticks).
    pub late_racing: u64,
    pub lead_min: i64,
    pub lead_max: i64,
    pub lead_mean: f32,
    pub key_changes: u64,
    pub rollbacks: u64,
    /// Predicted vs referee at the same tick (what was shown): position (m), yaw (deg).
    pub err_pos_max: f32,
    pub err_pos_p99: f32,
    pub err_pos_mean: f32,
    pub err_yaw_max: f32,
    pub err_yaw_p99: f32,
    /// Shown kart: one-frame jump beyond its own motion: position (m), yaw (deg).
    pub jump_pos_max: f32,
    pub jump_pos_p99: f32,
    pub jump_yaw_max: f32,
    pub jumps_over_5cm: u64,
    pub jumps_over_1deg: u64,
    pub frames: u64,
}

fn pct(v: &mut [f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * p).round() as usize]
}

/// Two human players steering with keys, `secs` of racing.
pub fn run(cond: LinkConditions, seed: u64, secs: u64, send_phase: u64, cfg: ClientConfig) -> Figures {
    let net = Loopback::new(cond, seed);
    let mut server = net.server();
    let mut r = referee(seed, &[9]).expect("referee");
    struct P {
        end: evani_net::transport::LoopbackEnd,
        id: u32,
        c: LockstepClient<NetRace>,
        brain: KartBrains,
        held: i8,
        last_sent: i8,
    }
    let mut ps: Vec<P> = (0..2)
        .map(|k| {
            let end = net.connect();
            let id = end.link().0;
            r.join(id, false).expect("seat");
            P {
                end,
                id,
                c: LockstepClient::player(client_copy().unwrap(), client_copy().unwrap(), cfg),
                brain: KartBrains::new(seed ^ ((k as u64 + 7) * 0x9E37)),
                held: 0,
                last_sent: 0,
            }
        })
        .collect();
    // The referee's kart 0 at every tick.
    let mut truth: HashMap<u64, Kart> = HashMap::new();
    // What player 0 saw at each tick (first time it was shown).
    let mut shown: HashMap<u64, Kart> = HashMap::new();
    let mut f = Figures::default();
    let (mut errs, mut yerrs, mut jumps) = (Vec::new(), Vec::new(), Vec::new());
    let mut prev: Option<(u64, Kart)> = None;
    let (mut buf, mut w, mut view) = (Vec::new(), Writer::new(), Writer::new());
    let mut racing_ticks = 0u64;
    let mut late_at_start: Option<u64> = None;
    let mut leads: Vec<i64> = Vec::new();
    let mut t = 0u64;
    while racing_ticks < secs * 60 {
        t += 1;
        assert!(t < 60 * 600, "never raced");
        net.advance();
        while let Some(from) = server.recv(&mut buf) {
            let _ = r.input(from.0, &buf);
        }
        if t.is_multiple_of(3) {
            for _ in 0..3 {
                r.step(1);
                if let Some(k) = pose(r.game(), 0) {
                    truth.insert(r.tick(), k);
                }
            }
            for p in &ps {
                r.view(Some(p.id), &mut view);
                server.send(LinkId(p.id), view.as_bytes());
            }
        }
        for (i, p) in ps.iter_mut().enumerate() {
            while p.end.recv(&mut buf).is_some() {
                let _ = p.c.on_view(&buf);
            }
            let input = if p.c.synced() && p.c.seat().is_some() {
                let seat = usize::from(p.c.seat().unwrap());
                let mut a: KartInput = p.brain.think(p.c.display(), seat, p.c.display_tick() + 1);
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
                if i == 0 && p.held != p.last_sent {
                    f.key_changes += 1;
                }
                p.last_sent = p.held;
                a
            } else {
                KartInput::default()
            };
            p.c.tick(Some(input));
            if t % 3 == send_phase {
                p.c.packet(&mut w);
                if !w.is_empty() {
                    p.end.send(LinkId::SERVER, w.as_bytes());
                }
            }
        }
        let c = &ps[0].c;
        if !c.synced() || c.display().race().phase != Phase::Racing {
            prev = None;
            continue;
        }
        racing_ticks += 1;
        late_at_start.get_or_insert(r.stats().late);
        leads.push(c.lead());
        let dt = c.display_tick();
        let Some(k) = pose(c.display(), 0) else { continue };
        shown.entry(dt).or_insert_with(|| k.clone());
        if let Some((pt, pk)) = &prev {
            let n = dt.saturating_sub(*pt) as f32;
            if n > 0.0 && n <= 3.0 {
                let step = [k.pos[0] - k.prev_pos[0], k.pos[2] - k.prev_pos[2]];
                let moved = [k.pos[0] - pk.pos[0], k.pos[2] - pk.pos[2]];
                let jp = ((moved[0] - n * step[0]).powi(2) + (moved[1] - n * step[1]).powi(2)).sqrt();
                let jy = (wrap_angle(k.yaw - pk.yaw) - n * wrap_angle(k.yaw - k.prev_yaw)).abs().to_degrees();
                // A respawn is not a steering correction.
                if jp < 20.0 {
                    f.frames += 1;
                    if jp > 0.05 {
                        f.jumps_over_5cm += 1;
                    }
                    if jy > 1.0 {
                        f.jumps_over_1deg += 1;
                    }
                    f.jump_yaw_max = f.jump_yaw_max.max(jy);
                    jumps.push(jp);
                }
            }
        }
        prev = Some((dt, k));
    }
    for (tick, s) in &shown {
        if let Some(tr) = truth.get(tick) {
            let e = ((s.pos[0] - tr.pos[0]).powi(2) + (s.pos[2] - tr.pos[2]).powi(2)).sqrt();
            // A respawn or a lap-line teleport is not a steering error.
            if e < 20.0 {
                errs.push(e);
                yerrs.push(wrap_angle(s.yaw - tr.yaw).abs().to_degrees());
            }
        }
    }
    let st = r.stats();
    f.late = st.late;
    f.repeated = st.repeated;
    f.late_racing = st.late - late_at_start.unwrap_or(0);
    f.lead_min = leads.iter().copied().min().unwrap_or(0);
    f.lead_max = leads.iter().copied().max().unwrap_or(0);
    f.lead_mean = leads.iter().sum::<i64>() as f32 / leads.len().max(1) as f32;
    f.rollbacks = ps[0].c.stats().rollbacks;
    f.err_pos_mean = errs.iter().sum::<f32>() / errs.len().max(1) as f32;
    f.err_pos_max = errs.iter().copied().fold(0.0, f32::max);
    f.err_pos_p99 = pct(&mut errs, 0.99);
    f.err_yaw_max = yerrs.iter().copied().fold(0.0, f32::max);
    f.err_yaw_p99 = pct(&mut yerrs, 0.99);
    f.jump_pos_max = jumps.iter().copied().fold(0.0, f32::max);
    f.jump_pos_p99 = pct(&mut jumps, 0.99);
    f
}

fn link(one_way_ms: u32, jitter_ms: u32, loss: u32) -> LinkConditions {
    LinkConditions {
        latency_ticks: (one_way_ms as f32 / 16.667).round() as u32,
        jitter_ticks: (jitter_ms as f32 / 16.667).round() as u32,
        loss_permille: loss,
    }
}

#[test]
#[ignore]
fn probe_steering_corrections() {
    println!("link | late inputs (racing) lead min/mean/max | held-in ticks | key changes | rollbacks | err pos mean/p99/max m | err yaw p99/max deg | jump pos p99/max m | jump yaw max deg | jumps>5cm | jumps>1deg | frames");
    for &(ms, jit, loss) in &[(30, 30, 0), (60, 30, 0), (100, 30, 0), (150, 30, 0), (100, 30, 20)] {
        for phase in [0u64, 1, 2] {
            let f = run(link(ms, jit, loss), 11 + phase, 40, phase, ClientConfig::default());
            println!(
                "{ms}+-{jit}ms loss {loss}/1000 phase {phase} | {} ({}) {}/{:.1}/{} | {} | {} | {} | {:.3}/{:.3}/{:.3} | {:.2}/{:.2} | {:.3}/{:.3} | {:.2} | {} | {} | {}",
                f.late, f.late_racing, f.lead_min, f.lead_mean, f.lead_max, f.repeated, f.key_changes, f.rollbacks, f.err_pos_mean, f.err_pos_p99, f.err_pos_max, f.err_yaw_p99, f.err_yaw_max, f.jump_pos_p99, f.jump_pos_max, f.jump_yaw_max, f.jumps_over_5cm, f.jumps_over_1deg, f.frames
            );
        }
    }
}
