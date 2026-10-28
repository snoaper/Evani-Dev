//! Replay proof for the race-rules split (multiplayer work, 2026-10-24):
//! three scripted races (one player, two players split-screen, the all-AI
//! title race) run through `race::tick`, hashed every tick. The hashes were
//! recorded on the code from before the split (commit 75ffe349, the live
//! game plus slipstream) by running this same file there; single-player
//! must still match them exactly.
//!
//! Uses only the race's public API, which is the same before and after.

// Test helpers may fail loudly (the project allows it in tests).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use evani_core::ecs::World;
use evani_kit_kart_racer::Tune;
use evani_kit_kart_racer::ai::roster_from_json;
use evani_kit_kart_racer::items::OddsTable;
use evani_kit_kart_racer::kart::Controls;
use evani_kit_kart_racer::race::{self, Counts, Ctx, Fns, Race, Request};
use evani_kit_kart_racer::track::Track;

fn world(t: &Tune) -> World {
    let track = Track::from_json(evani_kit_kart_racer::TRACK_JSON).expect("track");
    let odds = OddsTable::from_json(evani_kit_kart_racer::ITEMS_JSON).expect("items");
    let roster = roster_from_json(evani_kit_kart_racer::KARTS_JSON).expect("karts");
    let mut w = World::new();
    w.insert_resource(Race::new(track, odds, roster, Counts::default()));
    race::spawn_objects(&mut w, t, Counts::default());
    w
}

fn steer(x: f32) -> f32 {
    (0.25 + x * 4.0).min(1.0)
}

fn rubber(gap: f32, dist: f32, strength: f32) -> f32 {
    (gap / dist).clamp(-1.0, 1.0) * strength
}

fn fnv(h: &mut u64, s: &str) {
    for b in s.bytes() {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

fn input(p: usize, t: u32) -> Controls {
    let phase = (t / 50 + p as u32 * 3) % 7;
    Controls {
        steer: [0.0, 0.4, -0.3, 0.9, 0.0, -0.8, 0.2][phase as usize],
        throttle: if t % 400 < 380 { 1.0 } else { 0.0 },
        brake: if t % 400 >= 390 { 1.0 } else { 0.0 },
        drift: phase == 3 || phase == 5,
        item: t.is_multiple_of(97),
    }
}

/// Run `ticks` ticks of a race started with `players` (None: the title's
/// all-AI attract race) and fold every kart and the race clock into a hash.
fn replay(players: Option<usize>, ticks: u32) -> u64 {
    let t = Tune::default();
    let mut w = world(&t);
    if let Some(n) = players {
        w.res_mut::<Race>().request(Request::Start { players: n });
    }
    let fns = Fns {
        steer: &steer,
        rubber: &rubber,
    };
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for k in 0..ticks {
        let c = Ctx {
            t,
            counts: Counts::default(),
            gravity: 9.81 * 1.8,
            dt: 1.0 / 60.0,
            inputs: [input(0, k), input(1, k)],
        };
        race::tick(&mut w, &c, &fns);
        let r = w.res::<Race>();
        let karts = race::kart_list(&w, &r.grid);
        fnv(&mut h, &format!("{:?}{:?}{:?}{:?}{:?}", r.phase, r.clock.to_bits(), r.order, r.finish_order, r.events.len()));
        for kk in &karts {
            fnv(&mut h, &format!("{:?}", kk));
        }
    }
    h
}

/// Recorded on 75ffe349 (before the split).
const ONE_PLAYER: u64 = 0xc1f2_cddc_e2c6_2cd0;
const TWO_PLAYERS: u64 = 0x360d_cdc8_49b2_bd2b;
const ALL_AI: u64 = 0x1ca2_9f8a_e85e_9d6e;

#[test]
fn single_player_races_replay_exactly_as_before_the_split() {
    let got = [replay(Some(1), 3000), replay(Some(2), 3000), replay(None, 3000)];
    println!("replay hashes: one {:#018x} two {:#018x} ai {:#018x}", got[0], got[1], got[2]);
    assert_eq!(got, [ONE_PLAYER, TWO_PLAYERS, ALL_AI]);
}
