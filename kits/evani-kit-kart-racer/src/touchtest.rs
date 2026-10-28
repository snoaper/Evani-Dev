//! The touch layout, tested the way a thumb uses it (README "Touch
//! controls"): fingers go down on the layer's controls, the layer writes
//! actions, and the sim reads them like any other device. No key or pad is
//! pressed anywhere in this file, except where a test says so.

use evani_core::input::{Input, KeyCode, RawEvent};
use evani_core::rng::Pcg32;
use evani_core::touch::TouchControls;
use evani_core::App;

use crate::ai::{self, AiState};
use crate::race::{Phase, Race, RaceView, Request};
use crate::{ctx, KartRacerPlugin};

const FRAME_MS: f64 = 1000.0 / 60.0;
/// Control order in `touch_layout()`.
const STEER: usize = 0;
const DRIFT: usize = 1;
const ITEM: usize = 2;
const BRAKE: usize = 3;
const PAUSE: usize = 4;
/// A made-up steering ring: base centre and radius in CSS px.
const BASE: [f32; 2] = [120.0, 300.0];
const RADIUS: f32 = 60.0;

fn started(players: usize) -> App {
    let mut a = App::new("touchtest").with_kit(KartRacerPlugin);
    a.start();
    a.world.res_mut::<Race>().request(Request::Start { players });
    a.run_headless(1, FRAME_MS);
    a
}

fn with_touch<R>(a: &App, f: impl FnOnce(&mut TouchControls, &mut Input) -> R) -> R {
    let mut t = a.world.res_mut::<TouchControls>();
    let mut i = a.world.res_mut::<Input>();
    f(&mut t, &mut i)
}

/// A first touch: the layer shows, the way the page does on `pointerdown`.
fn show(a: &App) {
    with_touch(a, |t, i| t.on_touch(i));
}

fn run_until(a: &mut App, frames: usize, mut done: impl FnMut(&App) -> bool) {
    for _ in 0..frames {
        if done(a) {
            return;
        }
        a.run_headless(1, FRAME_MS);
    }
}

/// Racing, with the layer up and nothing held but the auto-throttle.
fn racing_with_touch() -> App {
    let mut a = started(1);
    show(&a);
    run_until(&mut a, 60 * 10, |a| a.world.res::<Race>().phase == Phase::Racing);
    assert_eq!(a.world.res::<Race>().phase, Phase::Racing);
    a.run_headless(2, FRAME_MS);
    a
}

#[test]
fn the_layer_belongs_to_the_race_only() {
    let mut a = App::new("touchtest").with_kit(KartRacerPlugin);
    a.start();
    show(&a);
    a.run_headless(2, FRAME_MS);
    // Title: menus are tapped on the page, so the controls are off.
    assert_eq!(a.world.res::<Race>().phase, Phase::Title);
    assert!(!evani_core::touch::active(&a.world));
    a.world.res_mut::<Race>().request(Request::Start { players: 1 });
    a.run_headless(2, FRAME_MS);
    assert_eq!(a.world.res::<Race>().phase, Phase::Countdown);
    assert!(evani_core::touch::active(&a.world), "up for the countdown");
    // Pause (a page menu with its own buttons): off, and nothing stays held.
    a.world.res_mut::<Race>().request(Request::TogglePause);
    a.run_headless(2, FRAME_MS);
    a.run_headless(1, FRAME_MS); // the switch follows the pause one tick later
    assert!(a.world.res::<Race>().paused);
    assert!(!evani_core::touch::active(&a.world));
    let throttle = a.world.res::<Input>().action_id("p1_throttle").unwrap();
    assert!(!a.world.res::<Input>().down(throttle), "auto-throttle let go with the layer");
    a.world.res_mut::<Race>().request(Request::TogglePause);
    a.run_headless(3, FRAME_MS);
    assert!(evani_core::touch::active(&a.world), "back after resume");
    assert!(a.world.res::<Input>().down(throttle));
}

#[test]
fn buttons_set_the_same_actions_the_keys_do() {
    let mut a = racing_with_touch();
    let (steer, throttle, drift, item, brake) = {
        let i = a.world.res::<Input>();
        let id = |n: &str| i.action_id(n).unwrap();
        (id("p1_steer"), id("p1_throttle"), id("p1_drift"), id("p1_item"), id("p1_brake"))
    };
    // Auto-accelerate: the throttle is down with no finger on anything.
    assert!(a.world.res::<Input>().down(throttle));
    // Drift is held while the finger is on it, like Shift.
    with_touch(&a, |t, i| t.press(i, DRIFT, 2, [0.0; 2], [0.0; 2], 0.0));
    a.run_headless(1, FRAME_MS);
    assert!(a.world.res::<Input>().down(drift));
    with_touch(&a, |t, i| t.release(i, 2));
    a.run_headless(1, FRAME_MS);
    assert!(!a.world.res::<Input>().down(drift));
    // A tap on Item is one press edge, never held.
    with_touch(&a, |t, i| {
        t.press(i, ITEM, 3, [0.0; 2], [0.0; 2], 0.0);
        t.release(i, 3);
    });
    a.world.res_mut::<Input>().update();
    assert!(a.world.res::<Input>().just_pressed(item));
    a.world.res_mut::<Input>().end_tick();
    a.run_headless(1, FRAME_MS);
    assert!(!a.world.res::<Input>().just_pressed(item) && !a.world.res::<Input>().down(item));
    // The steering stick is an axis in quantised steps; full right is +1.
    with_touch(&a, |t, i| t.press(i, STEER, 4, [BASE[0] + RADIUS, BASE[1]], BASE, RADIUS));
    a.run_headless(1, FRAME_MS);
    assert_eq!(a.world.res::<Input>().value(steer), 1.0);
    with_touch(&a, |t, i| t.drag(i, 4, [BASE[0] - RADIUS * 0.5, BASE[1]]));
    a.run_headless(1, FRAME_MS);
    let half = a.world.res::<Input>().value(steer);
    assert!((-0.8..-0.2).contains(&half), "{half}");
    with_touch(&a, |t, i| t.release(i, 4));
    // Brake is held like S.
    with_touch(&a, |t, i| t.press(i, BRAKE, 5, [0.0; 2], [0.0; 2], 0.0));
    a.run_headless(1, FRAME_MS);
    assert!(a.world.res::<Input>().down(brake));
    with_touch(&a, |t, i| t.release(i, 5));
    // The pause button is one press edge of the same `pause` action Esc drives.
    with_touch(&a, |t, i| {
        t.press(i, PAUSE, 6, [0.0; 2], [0.0; 2], 0.0);
        t.release(i, 6);
    });
    a.run_headless(2, FRAME_MS);
    assert!(a.world.res::<Race>().paused, "the pause button pauses");
}

#[test]
fn braking_under_touch_takes_the_auto_throttle_off_and_keys_are_unchanged() {
    let mut a = racing_with_touch();
    let c = ctx(&a.world).unwrap().1;
    assert_eq!((c.inputs[0].throttle, c.inputs[0].brake), (1.0, 0.0));
    with_touch(&a, |t, i| t.press(i, BRAKE, 5, [0.0; 2], [0.0; 2], 0.0));
    a.run_headless(1, FRAME_MS);
    let c = ctx(&a.world).unwrap().1;
    assert_eq!((c.inputs[0].throttle, c.inputs[0].brake), (0.0, 1.0));
    // A keyboard player (the first key hides the layer) holding W and S keeps
    // "throttle beats brake": both read as pressed, as before.
    with_touch(&a, |t, i| {
        t.release(i, 5);
        i.handle(&RawEvent::Key { code: KeyCode::new("KeyW").unwrap(), down: true });
        i.handle(&RawEvent::Key { code: KeyCode::new("KeyS").unwrap(), down: true });
        t.on_key_or_pad(i);
    });
    a.run_headless(1, FRAME_MS);
    assert!(!evani_core::touch::active(&a.world));
    let c = ctx(&a.world).unwrap().1;
    assert_eq!((c.inputs[0].throttle, c.inputs[0].brake), (1.0, 1.0));
}

/// The whole race by thumb: the steering stick only (the throttle is
/// automatic), following the kit's own racing line, to the finish line.
#[test]
fn a_whole_race_can_be_driven_by_touch_alone() {
    let mut a = started(1);
    show(&a);
    let mut brain = AiState::default();
    let mut rng = Pcg32::new(0xB07, 1);
    let mut first_lap = None;
    let mut finished = false;
    for frame in 0..60 * 400 {
        let (phase, steer, laps, done) = {
            let (_, c, _) = ctx(&a.world).unwrap();
            let r = RaceView::new(&a.world);
            let me = r.player_kart(0).unwrap();
            let k = &r.karts[me];
            let steer = if r.phase == Phase::Racing && k.finished.is_none() {
                let around = ai::surroundings(&r.karts, me, &r.track);
                ai::drive(k, &r.roster[k.spec], &mut brain, around, &r.track, &c.t, rng.next_f32(), c.dt).steer
            } else {
                0.0
            };
            (r.phase, steer, k.laps_done, r.phase == Phase::Finished)
        };
        if done {
            finished = true;
            break;
        }
        if phase == Phase::Racing {
            if laps >= 1 && first_lap.is_none() {
                first_lap = Some(frame);
            }
            with_touch(&a, |t, i| {
                let at = [BASE[0] + steer.clamp(-1.0, 1.0) * RADIUS, BASE[1]];
                if t.is_held(STEER) {
                    t.drag(i, 9, at);
                } else {
                    t.press(i, STEER, 9, at, BASE, RADIUS);
                }
            });
        }
        a.run_headless(1, FRAME_MS);
    }
    assert!(first_lap.is_some(), "no lap by touch");
    assert!(finished, "the race never ended by touch");
}
