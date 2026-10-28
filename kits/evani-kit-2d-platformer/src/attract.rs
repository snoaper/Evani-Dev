//! The title screen's attract demo: the bot ([`crate::bot`]) plays the level
//! once when the kit starts and its input is recorded, with a snapshot of
//! the game every [`SNAP`] ticks. Playback is a pure function of the render
//! clock, so `capture.frame` at a held sim time always shows the same frame:
//! review times 0, 20 and 40 s map to the first jump's apex, a stomp near
//! the middle of the run and the castle gate; the ticks between are played
//! at whatever even speed joins them (about 0.8x to 1.4x), then the castle
//! is held for [`HOLD_S`] and the demo loops.
//!
//! The demo keeps the tuning it was recorded with, so live param changes
//! never desync a replay (it is re-recorded when the kit is rebuilt).

use std::rc::Rc;

use crate::bot::{self, Run, Sim};
use crate::game::{Controls, Event, Game, Phase};
use crate::level::Level;
use crate::{Ints, Tune};

/// Ticks between snapshots.
pub const SNAP: usize = 120;
/// Review times (s) of the three key moments.
pub const KEY_TIMES: [f64; 3] = [0.0, 20.0, 40.0];
/// Seconds the demo stays at the castle before it loops.
pub const HOLD_S: f64 = 4.0;
/// Ticks after the stomp the key frame may show: the one in this window
/// where the hero is highest (the puff and score pop are up, the squashed
/// enemy still shows and the hero has bounced clear of them).
const AFTER_STOMP: (usize, usize) = (4, 26);
/// Ticks after reaching the goal that the last key frame shows (the hero
/// is in the gate).
const AFTER_GOAL: usize = 40;

/// The title screen's demo run.
pub struct Attract {
    t: Tune,
    n: Ints,
    gravity: f32,
    dt: f32,
    controls: Vec<Controls>,
    snaps: Vec<Game>,
    /// Ticks shown at [`KEY_TIMES`].
    pub keys: [usize; 3],
    /// Every event of the recording with the tick that raised it (capture
    /// moments find their shots here).
    pub events: Vec<(usize, Event)>,
    /// The playback state: `game` has had `at` ticks applied.
    pub game: Game,
    at: usize,
    /// Whether the bot reached the castle (if not, the demo just loops the
    /// recording from the start).
    pub reached_goal: bool,
}

impl Attract {
    /// Record the demo with the tuning `t`, `n` and gravity `g`.
    pub fn new(level: Rc<Level>, t: &Tune, n: &Ints, g: f32) -> Self {
        let dt = 1.0 / 60.0;
        let mut game = Game::new(level, t, n);
        game.reset(Phase::Playing, t, n);
        let start = game.clone();
        let mut snaps = Vec::new();
        let sim = Sim { t, n, gravity: g, dt };
        let tail = (HOLD_S * 60.0) as usize + AFTER_GOAL + 10;
        let run = bot::play(&mut game, sim, 60 * 90, tail, |i, g| {
            if i % SNAP == 0 {
                snaps.push(g.clone());
            }
        });
        let keys = key_ticks(&run, &start, sim);
        let reached_goal = run.won_at.is_some();
        let mut game = start;
        game.events.clear();
        Self {
            t: *t,
            n: *n,
            gravity: g,
            dt,
            controls: run.controls,
            snaps,
            keys,
            events: run.events,
            game,
            at: 0,
            reached_goal,
        }
    }

    /// Ticks in the recording.
    pub fn len(&self) -> usize {
        self.controls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }

    /// The integer settings the demo was recorded with.
    pub fn ints(&self) -> Ints {
        self.n
    }

    /// Length of one loop in seconds.
    pub fn period(&self) -> f64 {
        KEY_TIMES[2] + HOLD_S
    }

    /// The recording tick (fractional) to show at render time `secs`.
    pub fn tick_at(&self, secs: f64) -> f64 {
        let last = self.len().saturating_sub(1) as f64;
        let s = secs.rem_euclid(self.period());
        let k = self.keys.map(|k| k as f64);
        let tick = if s < KEY_TIMES[1] {
            k[0] + (k[1] - k[0]) * (s - KEY_TIMES[0]) / (KEY_TIMES[1] - KEY_TIMES[0])
        } else if s < KEY_TIMES[2] {
            k[1] + (k[2] - k[1]) * (s - KEY_TIMES[1]) / (KEY_TIMES[2] - KEY_TIMES[1])
        } else {
            // Hold at the gate: real time, then stop at the end.
            k[2] + (s - KEY_TIMES[2]) * 60.0
        };
        tick.clamp(0.0, last.max(0.0))
    }

    /// Bring [`Attract::game`] to render time `secs`; returns the
    /// interpolation alpha between its last two ticks. Events are dropped
    /// (the demo is silent).
    pub fn seek(&mut self, secs: f64) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let want = self.tick_at(secs);
        // `game` after `at` ticks shows tick `at - 1 + alpha`.
        let target = (want.floor() as usize + 1).min(self.len());
        if target < self.at || target > self.at + SNAP {
            let s = (target.saturating_sub(1) / SNAP).min(self.snaps.len().saturating_sub(1));
            if let Some(g) = self.snaps.get(s) {
                self.game = g.clone();
                self.at = s * SNAP;
            }
        }
        while self.at < target {
            let c = self.controls[self.at];
            self.game.tick(c, &self.t, &self.n, self.gravity, self.dt);
            self.at += 1;
        }
        self.game.events.clear();
        (want - want.floor()) as f32
    }

    /// The recording's game after `tick` ticks (from the nearest snapshot),
    /// events cleared; `None` past the end.
    pub fn game_at(&self, tick: usize) -> Option<Game> {
        if tick > self.len() {
            return None;
        }
        let s = (tick / SNAP).min(self.snaps.len().checked_sub(1)?);
        let mut g = self.snaps[s].clone();
        self.replay(&mut g, s * SNAP, tick - s * SNAP);
        g.events.clear();
        Some(g)
    }

    /// Play `n` recorded ticks from tick `from` on `g` (which may have
    /// been changed, e.g. a coin count) with the recording's tuning.
    pub fn replay(&self, g: &mut Game, from: usize, n: usize) {
        self.replay_with(g, from, n, |c, _| c);
    }

    /// [`Attract::replay`] with each recorded input changed by `f` (given
    /// the input and its tick) first: a capture moment shows what a player
    /// could have done there.
    pub fn replay_with(&self, g: &mut Game, from: usize, n: usize, f: impl Fn(Controls, usize) -> Controls) {
        for (i, c) in self.controls.iter().enumerate().skip(from).take(n) {
            g.tick(f(*c, i), &self.t, &self.n, self.gravity, self.dt);
        }
    }
}

/// The three key ticks: the first jump's apex, the stomp closest to the
/// middle of the run, the castle gate. Falls back to evenly spaced ticks
/// when the run has no jump, stomp or goal.
fn key_ticks(run: &Run, start: &Game, sim: Sim) -> [usize; 3] {
    let len = run.controls.len().max(3);
    let goal = run.won_at.map_or(len - 1, |w| (w + AFTER_GOAL).min(len - 1));
    // Apex: replay to the first jump and on until the hero stops rising.
    let mut apex = 0;
    if let Some(&(jt, _)) = run.events.iter().find(|(_, e)| *e == Event::Jump) {
        let mut g = start.clone();
        for (i, c) in run.controls.iter().enumerate() {
            g.tick(*c, sim.t, sim.n, sim.gravity, sim.dt);
            g.events.clear();
            if i > jt && g.hero.body.vel[1] >= 0.0 {
                apex = i;
                break;
            }
        }
    }
    let mid = (apex + goal) / 2;
    let stomp = run
        .events
        .iter()
        .filter(|(i, e)| *e == Event::Stomp && *i > apex && *i < goal)
        .map(|(i, _)| *i)
        .min_by_key(|i| i.abs_diff(mid))
        .map_or(mid, |s| highest_after(run, start, sim, s));
    [apex, stomp.max(apex + 1), goal.max(stomp + 1).min(len - 1)]
}

/// The tick in the [`AFTER_STOMP`] window after `stomp` where the hero is highest.
fn highest_after(run: &Run, start: &Game, sim: Sim, stomp: usize) -> usize {
    let (from, to) = (stomp + AFTER_STOMP.0, (stomp + AFTER_STOMP.1).min(run.controls.len().saturating_sub(1)));
    let mut g = start.clone();
    let mut best = (f32::INFINITY, from);
    for (i, c) in run.controls.iter().enumerate().take(to + 1) {
        g.tick(*c, sim.t, sim.n, sim.gravity, sim.dt);
        g.events.clear();
        if i >= from && g.hero.body.pos[1] < best.0 {
            best = (g.hero.body.pos[1], i);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LEVEL_JSON;

    fn attract() -> Attract {
        let (t, n) = (Tune::default(), Ints::default());
        let level = Rc::new(Level::from_json(LEVEL_JSON).unwrap());
        Attract::new(level, &t, &n, 980.0)
    }

    #[test]
    fn key_times_show_the_apex_a_stomp_and_the_castle() {
        let mut a = attract();
        assert!(a.reached_goal);
        let [k0, k1, k2] = a.keys;
        assert!(k0 < k1 && k1 < k2, "{:?}", a.keys);
        // 0 s: in the air at the top of the first jump.
        a.seek(0.0);
        assert!(!a.game.hero.body.grounded);
        assert!(a.game.hero.body.vel[1].abs() < 40.0, "{:?}", a.game.hero.body.vel);
        // 20 s: just after a stomp (a squashed enemy, the hero bounced up
        // clear of it and its score pop).
        a.seek(20.0);
        let squashed = a.game.enemies.iter().find(|e| e.squashed >= 0.0 && e.squashed < 0.45);
        let squashed = squashed.map(|e| e.body.pos[1]).unwrap_or_else(|| panic!("no fresh stomp at 20 s"));
        assert!(a.game.hero.body.pos[1] < squashed - 16.0, "hero {:?} enemy feet {squashed}", a.game.hero.body.pos);
        assert!(a.game.stomps >= 1);
        // 40 s: at the castle gate.
        a.seek(40.0);
        assert_eq!(a.game.phase, Phase::Won);
        assert!((a.game.hero.body.pos[0] - a.game.level.goal[0]).abs() < 12.0);
    }

    #[test]
    fn playback_is_a_pure_function_of_time() {
        let mut a = attract();
        let mut b = attract();
        // Different seek histories land on the same state.
        for s in [3.0, 31.0, 7.5, 44.0, 12.25] {
            a.seek(s);
        }
        a.seek(12.25);
        b.seek(12.25);
        assert_eq!(a.game.hero, b.game.hero);
        assert_eq!(a.game.enemies, b.game.enemies);
        // Loops.
        let p = a.period();
        a.seek(5.0 + p);
        b.seek(5.0);
        assert_eq!(a.game.hero, b.game.hero);
    }
}
