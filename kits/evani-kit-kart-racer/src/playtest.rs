//! The playtest bot (docs/KIT_PROCESS.md, "Kit cooperation"): whole races
//! played through a virtual gamepad, the way a person plays, headless.
//!
//! The bot reads the race like a player (where the kart is, the road ahead,
//! the karts around it, the item boxes and the item it holds), decides, and
//! presses pad buttons: stick axis 0 steers, button 7 is the throttle, 6 the
//! brake, 5 drift and 2 the item (released between uses, so each use is a
//! fresh press). Nothing in the sim is touched: only the pad.
//!
//! * On the grid it revs inside the start-boost window, like a player
//!   watching the lights.
//! * It follows the racing line with the kit's own AI driver
//!   ([`ai::drive`]) for Pip, but drifts like a keen player: into every
//!   corner that can hold a drift, steering the drift along the line, and
//!   lets go for the boost as the corner opens ([`drift_line`]). The AI
//!   drivers drift only the tightest corners, so drifting well is what wins,
//!   which is the genre's core skill (playbook, "Drift and boost").
//! * With an empty slot it aims for an item box that will be back by the
//!   time it arrives, and it uses items at once (a bomb when a rival is in
//!   range ahead).
//!
//! Every run has a frame cap: a race that has not ended by then (a
//! softlock) fails the test.

use evani_core::input::Input;
use evani_core::rng::Pcg32;
use evani_core::App;

use crate::ai::{self, AiState};
use crate::items::{Item, ItemBox, Slot};
use crate::kart::Kart;
use crate::race::{Phase, Race, RaceView, Request};
use crate::track::Track;
use crate::{ctx, KartRacerPlugin, Tune};

/// 60 frames a second.
const FRAME_MS: f64 = 1000.0 / 60.0;
/// About 400 s of play: far past a normal race (about 80 to 130 s).
const MAX_FRAMES: usize = 60 * 400;
/// The race numbers (item and AI noise streams) the tests play.
const RACES: [u64; 3] = [1, 2, 3];

/// How the bot plays.
#[derive(Debug, Clone, Copy)]
struct Style {
    /// Most throttle it presses while racing (the pad button's pressure).
    throttle: f32,
    /// Drifts through corners.
    drift: bool,
    /// Goes for item boxes and uses items.
    items: bool,
    /// Revs on the grid inside the start-boost window.
    good_start: bool,
}

const RACER: Style = Style {
    throttle: 1.0,
    drift: true,
    items: true,
    good_start: true,
};

/// Part throttle, no drifting, no items, a slow start: a run that loses.
const TIMID: Style = Style {
    throttle: 0.6,
    drift: false,
    items: false,
    good_start: false,
};

/// Corner curvature (1/m, over the next 30 m) where the bot starts a drift:
/// gentle enough that most corners count, tight enough for the drift's
/// slowest turn rate to follow the road.
const DRIFT_START: f32 = 0.009;
/// Yaw-rate gain on the heading error while drifting.
const DRIFT_GAIN: f32 = 4.0;

/// How a run ended.
#[derive(Debug)]
struct Outcome {
    /// 0-based finishing place of the player (None: did not finish).
    place: Option<usize>,
    /// The race reached its end (`Phase::Finished`).
    ended: bool,
    frames: usize,
    good_start: bool,
    clock: f32,
    /// The player's finish time minus the best rival's (seconds; negative: ahead).
    gap: Option<f32>,
}

/// One race with one local player driven by the bot. `race_no` picks the
/// race's random streams (item draws, AI noise), as a new race would.
fn play(style: Style, race_no: u64) -> Outcome {
    play_with(style, race_no, &[])
}

/// [`play`] with some tunables changed first (full param names), the way a
/// live `param.set` would; the tuning table below uses it.
fn play_with(style: Style, race_no: u64, overrides: &[(&str, f32)]) -> Outcome {
    let mut a = App::new("playtest").with_kit(KartRacerPlugin);
    a.start();
    for (name, v) in overrides {
        a.world
            .res_mut::<evani_core::params::ParamRegistry>()
            .set_by_name(name, evani_core::params::ParamValue::F32(*v))
            .expect("a registered kit tunable");
    }
    // The Start request's reset counts the race up from here.
    a.world.res_mut::<Race>().race_no = race_no;
    a.world.res_mut::<Race>().request(Request::Start { players: 1 });
    a.run_headless(1, FRAME_MS);
    let mut brain = AiState::default();
    let mut rng = Pcg32::new(0xB07, race_no);
    let mut item_down = false;
    let mut good_start = false;
    for frame in 0..MAX_FRAMES {
        let (_, c, _) = ctx(&a.world).expect("kit built");
        // Read the race and decide this frame's pad state.
        let (steer, throttle, brake, drift, item, ended, me) = {
            let r = RaceView::new(&a.world);
            let me = r.player_kart(0).expect("one local player");
            let k = &r.karts[me];
            good_start |= r.phase == Phase::Racing && k.boost > 0.0 && r.clock < 0.2;
            match r.phase {
                Phase::Countdown => {
                    let rev = style.good_start && r.countdown < c.t.start_boost_window_s * 0.6;
                    (0.0, if rev { 1.0 } else { 0.0 }, 0.0, false, false, false, me)
                }
                Phase::Racing if k.finished.is_none() => {
                    let around = ai::surroundings(&r.karts, me, &r.track);
                    let ctl = ai::drive(k, &r.roster[k.spec], &mut brain, around, &r.track, &c.t, rng.next_f32(), c.dt);
                    let (mut steer, mut drift) = if style.drift {
                        drift_line(k, ctl.steer, &r.track, &c.t)
                    } else {
                        (ctl.steer, false)
                    };
                    // With an empty slot, aim for a box that will be there
                    // when the kart arrives (the pack ahead takes some).
                    if style.items
                        && k.slot == Slot::Empty
                        && k.drift == 0.0
                        && let Some(err) = box_heading(k, &r.boxes)
                    {
                        steer = (-err * c.t.ai_steer_gain).clamp(-1.0, 1.0);
                        drift = false;
                    }
                    let item = style.items
                        && match k.slot {
                            Slot::Holding(Item::ZoomPop) => k.on_road,
                            Slot::Holding(Item::BoingBomb) => around.ahead.is_some_and(|d| d < 35.0),
                            Slot::Holding(_) => true,
                            _ => false,
                        };
                    (steer, ctl.throttle.min(style.throttle), ctl.brake, drift, item, false, me)
                }
                Phase::Finished => (0.0, 0.0, 0.0, false, false, true, me),
                _ => (0.0, 0.0, 0.0, false, false, false, me),
            }
        };
        if ended {
            let gap = {
                let v = RaceView::new(&a.world);
                let mine = v.karts[me].finished;
                let best = v.karts.iter().enumerate().filter(|(i, _)| *i != me).filter_map(|(_, k)| k.finished).fold(f32::INFINITY, f32::min);
                mine.filter(|_| best.is_finite()).map(|m| m - best)
            };
            let r = a.world.res::<Race>();
            return Outcome {
                place: r.finish_order.iter().position(|&i| i == me),
                ended: true,
                frames: frame,
                good_start,
                clock: r.clock,
                gap,
            };
        }
        // A fresh press for each item use: release first if still held.
        let press_item = item && !item_down;
        item_down = press_item;
        let mut buttons = [0.0f32; 16];
        buttons[7] = throttle;
        buttons[6] = brake;
        buttons[5] = if drift { 1.0 } else { 0.0 };
        buttons[2] = if press_item { 1.0 } else { 0.0 };
        a.world.res_mut::<Input>().set_pad(0, true, &buttons, &[steer, 0.0, 0.0, 0.0]);
        a.run_headless(1, FRAME_MS);
    }
    let clock = a.world.res::<Race>().clock;
    Outcome {
        place: None,
        ended: false,
        frames: MAX_FRAMES,
        good_start,
        clock,
        gap: None,
    }
}

/// A keen player's drift: dive into any corner that can hold a drift, steer
/// the drift along the racing line, and let go for the boost when the
/// corner opens up, the kart nears the road edge, or the charge is full.
/// Returns (steer, drift button).
fn drift_line(k: &Kart, steer: f32, track: &Track, t: &Tune) -> (f32, bool) {
    let curve = track.curvature_ahead(k.s + 6.0, 30.0);
    if k.drift == 0.0 {
        let go = curve.abs() > DRIFT_START && k.speed > t.drift_min_speed + 3.0 && k.hop <= 0.0;
        // A drift starts towards the steering side: steer into the corner.
        return if go { (-curve.signum() * steer.abs().max(0.5), true) } else { (steer, false) };
    }
    let d = k.drift;
    let ahead = t.ai_lookahead + k.speed.max(0.0) * 0.12;
    let ts = k.s + ahead;
    let p = track.point_at(ts, track.line_at(ts));
    let err = wrap((p[0] - k.pos[0]).atan2(p[2] - k.pos[2]) - k.yaw);
    // The yaw rate wanted, as a share of the drift's own turn rate; steering
    // tightens (towards 1.8) or widens (towards 0.2) the drift.
    let shape = err * DRIFT_GAIN / (t.drift_turn * d);
    let s = (-d * (shape.clamp(0.2, 1.8) - 1.0) / t.drift_steer.max(0.1)).clamp(-1.0, 1.0);
    let opens = shape < 0.25;
    let edge = k.lateral.abs() > track.half_width * 0.8;
    let release = k.charge >= t.drift_tier3 || (opens && (k.tier(t) > 0 || curve.signum() != d)) || edge;
    (s, !release)
}

/// Heading error (radians, + is left) to the best item box ahead: 4 to 40 m
/// in front, within 35 degrees, and back from respawn by the time the kart
/// gets there.
fn box_heading(k: &Kart, boxes: &[ItemBox]) -> Option<f32> {
    let fwd = [k.yaw.sin(), k.yaw.cos()];
    boxes
        .iter()
        .filter_map(|b| {
            let d = [b.pos[0] - k.pos[0], b.pos[2] - k.pos[2]];
            let along = d[0] * fwd[0] + d[1] * fwd[1];
            let arrive = along / k.speed.max(1.0);
            let err = wrap(d[0].atan2(d[1]) - k.yaw);
            ((4.0..40.0).contains(&along) && err.abs() < 0.6 && b.hidden < arrive - 0.1).then_some(err)
        })
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

fn wrap(a: f32) -> f32 {
    (a + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU) - core::f32::consts::PI
}

/// The bot plays full three-lap races to a podium finish (1st to 3rd), with
/// a good start, on three different races.
#[test]
fn playtest_win_podium_finish() {
    for n in RACES {
        let o = play(RACER, n);
        assert!(o.ended, "softlock: the race did not end in {MAX_FRAMES} frames ({o:?})");
        assert!(o.good_start, "the bot missed the start boost: {o:?}");
        let place = o.place.expect("the player finished");
        assert!(place < 3, "the bot finished {} ({o:?})", place + 1);
        eprintln!("playtest_win: race {n}: place {} after {:.1} s, {} frames", place + 1, o.clock, o.frames);
    }
}

/// A timid run (part throttle, no drifting, no items, no start boost)
/// finishes off the podium: losing is possible, and the race still ends.
/// An unfinished player at the race's end would count as a lose too.
#[test]
fn playtest_lose_off_the_podium() {
    for n in RACES {
        let o = play(TIMID, n);
        assert!(o.ended, "softlock: the race did not end in {MAX_FRAMES} frames ({o:?})");
        assert!(!o.good_start, "{o:?}");
        assert!(o.place.is_none_or(|p| p >= 3), "the timid bot reached the podium: {o:?}");
        eprintln!("playtest_lose: race {n}: place {:?} after {:.1} s, {} frames", o.place.map(|p| p + 1), o.clock, o.frames);
    }
}

/// The handling tuning table (kit 001, Stage 2 gate). Run it by name:
/// `TUNE_PARAM=drift_tier1 TUNE_VALUES=0.6,0.8,1.0 cargo test -p
/// evani-kit-kart-racer --release tune_table -- --ignored --nocapture`.
/// For each value the three bot styles (a drifting racer, plain driving with
/// no drifts, and a timid driver) play the three races; the table gives the
/// mean race time and mean place of each, so a handling value can be chosen
/// from what drifting, plain driving and timid driving earn. Not a check: it
/// does nothing without `TUNE_PARAM`.
#[test]
#[ignore = "a tuning table, not a check: run it by name"]
fn tune_table() {
    let Ok(param) = std::env::var("TUNE_PARAM") else { return };
    let values: Vec<f32> = std::env::var("TUNE_VALUES")
        .expect("TUNE_VALUES")
        .split(',')
        .map(|v| v.trim().parse().expect("a number"))
        .collect();
    let name = format!("kit.kart_racer.{param}");
    const PLAIN: Style = Style { throttle: 1.0, drift: false, items: true, good_start: true };
    eprintln!("| {param} | racer s | racer place | racer gap s | plain s | plain place | timid s | timid place | start boost |");
    eprintln!("|---|---|---|---|---|---|---|---|---|");
    for v in values {
        let mut cells = Vec::new();
        let mut start = 0;
        for style in [RACER, PLAIN, TIMID] {
            let (mut t, mut p, mut g, mut n) = (0.0f32, 0.0f32, 0.0f32, 0);
            for race in RACES {
                let o = play_with(style, race, &[(&name, v)]);
                t += o.clock;
                p += o.place.map_or(8.0, |p| p as f32 + 1.0);
                g += o.gap.unwrap_or(f32::NAN);
                n += 1;
                if style.good_start && o.good_start {
                    start += 1;
                }
            }
            cells.push(if style.drift { format!("{:.1} | {:.2} | {:+.1}", t / n as f32, p / n as f32, g / n as f32) } else { format!("{:.1} | {:.2}", t / n as f32, p / n as f32) });
        }
        eprintln!("| {v} | {} | {start}/6 |", cells.join(" | "));
    }
}
