//! A lookahead bot that plays the level: every few ticks it tries a small
//! set of plans (wait, walk, run, jumps of several lengths and delays, a
//! step back) on clones of the [`Game`], about a second ahead, rejects any
//! plan that loses a life (or leaves the hero falling to one), and commits
//! the first ticks of the best: furthest right, with a bonus for coins and
//! stomps and a big one for the castle. The attract demo records one run.

use crate::game::{Controls, Event, Game, Phase};
use crate::{Ints, Tune};

/// Ticks between decisions (the committed part of a plan).
pub const DECIDE: usize = 6;
/// Ticks each plan is simulated.
pub const HORIZON: usize = 72;
/// Extra ticks a plan that ends airborne is followed (no input) to see
/// where it lands.
const SETTLE: usize = 45;

/// One plan: move left for `back` ticks, then towards `dir` until tick
/// `stop`, then stand still; jump held from tick `delay` for `hold`
/// ticks. Plans end standing still so a plan that is kept for longer than
/// it was tested never runs off a ledge.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Plan {
    back: usize,
    dir: f32,
    stop: usize,
    jump: Option<(usize, usize)>,
    run: bool,
}

impl Plan {
    const WAIT: Plan = Plan { back: 0, dir: 0.0, stop: 0, jump: None, run: false };

    fn right(stop: usize, run: bool) -> Plan {
        Plan { back: 0, dir: 1.0, stop, jump: None, run }
    }

    fn jump(back: usize, dir: f32, delay: usize, hold: usize, stop: usize) -> Plan {
        Plan { back, dir, stop, jump: Some((delay, hold)), run: true }
    }

    /// (x, jump held, run held) at tick `i`.
    fn input(self, i: usize) -> (f32, bool, bool) {
        let x = if i < self.back {
            -1.0
        } else if i < self.stop {
            self.dir
        } else {
            0.0
        };
        let jump = self.jump.is_some_and(|(d, h)| i >= d && i < d + h);
        (x, jump, self.run)
    }
}

fn plans() -> Vec<Plan> {
    let mut v = vec![Plan::WAIT, Plan::right(HORIZON, false)];
    for stop in [8, 16, 32, 48, HORIZON] {
        v.push(Plan::right(stop, true));
    }
    for delay in [0, 6, 12, 24, 36] {
        for hold in [4, 12, 24] {
            v.push(Plan::jump(0, 1.0, delay, hold, delay + 36));
        }
    }
    v.push(Plan::jump(0, 1.0, 0, 24, HORIZON));
    v.push(Plan::jump(0, 1.0, 12, 24, HORIZON));
    // Run-ups: back off, then jump on the way forward.
    for back in [10, 20] {
        for d in [10, 18] {
            v.push(Plan::jump(back, 1.0, back + d, 24, back + d + 36));
        }
    }
    v.push(Plan::jump(0, 0.0, 0, 24, 0));
    v.push(Plan::jump(0, -1.0, 0, 12, 24));
    v
}

/// The bot's input state between ticks (for `jump_pressed`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Bot {
    jump_was: bool,
    /// The plan being followed and how far into it.
    current: Option<(Plan, usize)>,
}

/// Simulation constants a bot run uses.
#[derive(Debug, Clone, Copy)]
pub struct Sim<'a> {
    pub t: &'a Tune,
    pub n: &'a Ints,
    pub gravity: f32,
    pub dt: f32,
}

fn controls(x: f32, jump: bool, run: bool, jump_was: bool) -> Controls {
    Controls { x, jump, jump_pressed: jump && !jump_was, run, down: false }
}

/// How good a finished plan is; `None` when it costs a life.
fn score(start: &Game, g: &Game, max_x: f32, ticks: usize) -> Option<f32> {
    if g.lives < start.lives || !g.hero.alive() || g.phase == Phase::Lost {
        return None;
    }
    if g.phase == Phase::Won {
        return Some(1.0e6 - ticks as f32);
    }
    let coins = (g.coins - start.coins) as f32;
    let stomps = (g.stomps - start.stomps) as f32;
    Some(g.hero.body.pos[0] + 0.25 * max_x + coins * 10.0 + stomps * 60.0)
}

/// Simulate `plan` from `start`; its score, or `None` if it dies.
fn evaluate(start: &Game, plan: Plan, off: usize, jump_was: bool, sim: Sim) -> Option<f32> {
    let mut g = start.clone();
    let mut was = jump_was;
    let mut max_x = g.hero.body.pos[0];
    for i in 0..HORIZON {
        let (x, jump, run) = plan.input(off + i);
        g.tick(controls(x, jump, run, was), sim.t, sim.n, sim.gravity, sim.dt);
        was = jump;
        g.events.clear();
        if !g.hero.alive() || g.lives < start.lives {
            return None;
        }
        if g.phase == Phase::Won {
            return score(start, &g, max_x, i);
        }
        max_x = max_x.max(g.hero.body.pos[0]);
    }
    // Ending in the air: follow it down (no input) to see where it lands.
    for _ in 0..SETTLE {
        if g.hero.body.grounded || g.phase != Phase::Playing {
            break;
        }
        g.tick(controls(0.0, false, false, was), sim.t, sim.n, sim.gravity, sim.dt);
        was = false;
        g.events.clear();
        if !g.hero.alive() {
            return None;
        }
    }
    score(start, &g, max_x, HORIZON)
}

impl Bot {
    /// Choose the next [`DECIDE`] ticks of input for `g`.
    pub fn decide(&mut self, g: &Game, sim: Sim) -> Vec<Controls> {
        // Keeping the current plan wins ties (no dithering between plans
        // that score the same).
        let mut best: Option<(f32, Plan, usize)> = None;
        if let Some((p, off)) = self.current
            && let Some(s) = evaluate(g, p, off, self.jump_was, sim)
        {
            best = Some((s + 0.5, p, off));
        }
        for p in plans() {
            if let Some(s) = evaluate(g, p, 0, self.jump_was, sim)
                && best.is_none_or(|(b, _, _)| s > b)
            {
                best = Some((s, p, 0));
            }
        }
        // Every plan dies: keep running right (the run is lost anyway).
        let (plan, off) = best.map_or((Plan::right(HORIZON, true), 0), |(_, p, o)| (p, o));
        self.current = Some((plan, off + DECIDE));
        (0..DECIDE)
            .map(|i| {
                let (x, jump, run) = plan.input(off + i);
                let c = controls(x, jump, run, self.jump_was);
                self.jump_was = jump;
                c
            })
            .collect()
    }
}

/// A recorded bot run.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Input per tick from the start of the level.
    pub controls: Vec<Controls>,
    /// Tick of every event the run produced.
    pub events: Vec<(usize, Event)>,
    /// Tick the castle was reached, if it was.
    pub won_at: Option<usize>,
}

/// Play `g` (a fresh run in [`Phase::Playing`]) to the castle, at most
/// `max_ticks`, then `tail` more ticks of no input. `on_tick` sees the game
/// before each tick (the attract demo takes its snapshots there).
pub fn play(g: &mut Game, sim: Sim, max_ticks: usize, tail: usize, mut on_tick: impl FnMut(usize, &Game)) -> Run {
    let mut bot = Bot::default();
    let mut run = Run { controls: Vec::new(), events: Vec::new(), won_at: None };
    let mut pending: Vec<Controls> = Vec::new();
    let mut tick = 0usize;
    let mut after = 0usize;
    while tick < max_ticks + tail {
        let c = if run.won_at.is_some() || g.phase != Phase::Playing {
            after += 1;
            if after > tail {
                break;
            }
            Controls::default()
        } else {
            if pending.is_empty() {
                pending = bot.decide(g, sim);
                pending.reverse();
            }
            pending.pop().unwrap_or_default()
        };
        on_tick(tick, g);
        g.tick(c, sim.t, sim.n, sim.gravity, sim.dt);
        run.controls.push(c);
        for e in g.events.drain(..) {
            run.events.push((tick, e));
            if e == Event::Win {
                run.won_at = Some(tick);
            }
        }
        tick += 1;
        if run.won_at.is_none() && g.phase == Phase::Playing && tick >= max_ticks {
            break;
        }
    }
    run
}

/// For the review checks: play every plan from `g` (as [`Bot::decide`]
/// would, plus the landing) and, for each plan that keeps the life, the
/// coins it takes as `(coin index, jump)`: `jump` counts take-offs since the
/// branch began, `None` for a coin taken on the ground.
#[cfg(test)]
pub(crate) fn branch_coins(g: &Game, sim: Sim) -> Vec<Vec<(usize, Option<usize>)>> {
    let mut out = Vec::new();
    for plan in plans() {
        let mut b = g.clone();
        let mut was = false;
        let mut jumps = 0usize;
        let mut taken = Vec::new();
        let mut ok = true;
        for i in 0..HORIZON + SETTLE {
            let (x, jump, run) = if i < HORIZON { plan.input(i) } else { (0.0, false, false) };
            if i >= HORIZON && b.hero.body.grounded {
                break;
            }
            let grounded = b.hero.body.grounded;
            let before = b.coins_taken.clone();
            b.tick(controls(x, jump, run, was), sim.t, sim.n, sim.gravity, sim.dt);
            was = jump;
            b.events.clear();
            if !b.hero.alive() || b.lives < g.lives || b.phase == Phase::Lost {
                ok = false;
                break;
            }
            if grounded && !b.hero.body.grounded {
                jumps += 1;
            }
            for (k, (now, then)) in b.coins_taken.iter().zip(&before).enumerate() {
                if *now && !*then {
                    taken.push((k, (!b.hero.body.grounded).then_some(jumps)));
                }
            }
            if b.phase != Phase::Playing {
                break;
            }
        }
        if ok {
            out.push(taken);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LEVEL_JSON;
    use crate::level::Level;
    use std::rc::Rc;

    #[test]
    fn reaches_the_castle_without_losing_a_life() {
        let (t, n) = (Tune::default(), Ints::default());
        let level = Rc::new(Level::from_json(LEVEL_JSON).unwrap());
        let mut g = Game::new(level, &t, &n);
        g.reset(Phase::Playing, &t, &n);
        let sim = Sim { t: &t, n: &n, gravity: 980.0, dt: 1.0 / 60.0 };
        let started = std::time::Instant::now();
        let run = play(&mut g, sim, 60 * 120, 30, |i, g| {
            if i % 120 == 0 && std::env::var("BOT_TRACE").is_ok() {
                let e: Vec<_> = g.enemies.iter().map(|e| (e.body.pos[0] as i32, e.body.pos[1] as i32, e.alive())).collect();
                eprintln!("{i}: hero {:?} grounded {} {:?}", g.hero.body.pos, g.hero.body.grounded, &e[..2]);
            }
        });
        let took = started.elapsed();
        let won = run.won_at.unwrap_or_else(|| {
            panic!("stuck at x = {} after {} ticks, lives {}", g.hero.body.pos[0], run.controls.len(), g.lives)
        });
        assert_eq!(g.phase, Phase::Won);
        assert!(!run.events.iter().any(|(_, e)| *e == Event::Hurt), "lost a life");
        assert!(run.events.iter().any(|(_, e)| *e == Event::Stomp), "never stomped");
        assert!(run.events.iter().any(|(_, e)| *e == Event::Coin), "no coins");
        eprintln!("bot: castle at tick {won} ({:.1} s), planned in {took:?}", won as f32 / 60.0);
    }
}

