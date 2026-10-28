//! Scripted capture moments for live's `capture.frame { moment }` (Chris's
//! review, point 7): the level start, a stomp, the 1UP, a checkpoint, the
//! spike pit, the first pit and the castle gate. Each one is a state of the title screen's
//! attract recording (the bot's run, [`crate::attract`]) found from its
//! events, copied into the player's game and shown exactly at that tick (no
//! interpolation, no queued sounds). The run is deterministic, so every
//! capture shows the same frame.
//!
//! The kit has no power-up item; its reward moment is the extra life: the
//! coin count is set one short of `coins_per_life` just before a coin in an
//! arc, so that coin raises "1UP".
//!
//! The app registers them only in builds with live, so a shipped game never
//! references this code.

use evani_core::capture::CaptureMoments;
use evani_core::ecs::World;

use crate::Platformer;
use crate::attract::Attract;
use crate::game::{Controls, Event, Game, Phase};
use crate::level::{TILE, Tile};

/// Ticks after the checkpoint flag is touched (the flag is up, its
/// sparkle still shows).
const AFTER_CHECKPOINT: usize = 10;
/// Ticks after the 1UP coin: the "1UP" has risen over Fennel's head and
/// the rest of the coin arc is still ahead of him.
const AFTER_ONE_UP: usize = 6;
/// Most ticks between two coins of one arc.
const ARC_GAP: usize = 8;
/// How far (art px) Fennel's middle may be from the stomped beetle's in
/// the stomp shot: about half his body, so he is still above it.
pub const STOMP_DX: f32 = 10.0;

/// Register the seven moments in the order a review shows them.
pub fn register_capture_moments(m: &mut CaptureMoments) -> Result<(), String> {
    m.add("start", "Level start: Fennel at the signpost on Bramble Hills", start)?;
    m.add("stomp", "Fennel bounces off a stomped beetle, the points rising clear", stomp)?;
    m.add("power_up", "An extra life: the 50th coin, taken in mid-air, raises 1UP", power_up)?;
    m.add("checkpoint", "A checkpoint flag just raised", checkpoint)?;
    m.add("spikes", "Fennel passes over the spike pit by the stone pillar", spikes)?;
    m.add("pit", "Fennel leaps over the first bottomless pit: it reads as a gap, not as ground", pit)?;
    m.add("goal", "Fennel walks into the castle gate: level clear", goal)?;
    // The review's run video (`[review] video = "run"`): not a still.
    #[cfg(feature = "showcase")]
    m.add("run", "The run video: Fennel runs, jumps, stomps and takes coins (the attract recording, played live)", run)?;
    Ok(())
}

/// The span of the recording the run video is chosen for (ticks): the
/// review records 10 s, less a second for the screencast to start.
pub const RUN_WINDOW: usize = 9 * 60;

/// The recording tick the run video starts at: the start (on a half-second
/// step) of the [`RUN_WINDOW`] with the most going on (stomps, a
/// checkpoint, coins, jumps) and nothing that hurts the hero; the earliest
/// such span when several tie.
pub fn run_start(a: &Attract) -> usize {
    let score = |from: usize| {
        let mut n = [0usize; 4];
        for (_, e) in a.events.iter().filter(|(t, _)| *t >= from && *t < from + RUN_WINDOW) {
            match e {
                Event::Stomp => n[0] += 1,
                Event::Checkpoint => n[1] += 1,
                Event::Coin => n[2] += 1,
                Event::Jump => n[3] += 1,
                Event::Hurt | Event::Lose | Event::Respawn => return 0,
                _ => {}
            }
        }
        n[0].min(2) * 6 + n[1].min(1) * 5 + n[2].min(8) + n[3].min(6)
    };
    let last = a.len().saturating_sub(RUN_WINDOW);
    (0..=last).step_by(30).max_by_key(|&f| (score(f), core::cmp::Reverse(f))).unwrap_or(0)
}

/// The run video: the recording from [`run_start`] played on the player's
/// game, one recorded input per step, until the player acts.
#[cfg(feature = "showcase")]
pub fn run(world: &World) -> Result<(), String> {
    let from = run_start(&world.try_res::<Platformer>().ok_or("the 2D platformer kit is not loaded")?.attract);
    stage(world, |a| at(a, from))?;
    world.res_mut::<Platformer>().show = Some(crate::Show::Run { at: from });
    Ok(())
}

/// Put `pick`'s state of the recording into the player's game, shown
/// exactly as it is.
fn stage(world: &World, pick: impl FnOnce(&Attract) -> Result<Game, String>) -> Result<(), String> {
    let mut p = world.try_res_mut::<Platformer>().ok_or("the 2D platformer kit is not loaded")?;
    if p.attract.is_empty() {
        return Err("the attract demo has no recording".into());
    }
    let mut g = pick(&p.attract)?;
    hold(&mut g);
    p.game = g;
    // Held until the player acts (live builds; see `crate::Show`).
    #[cfg(feature = "showcase")]
    {
        p.show = Some(crate::Show::Still);
    }
    Ok(())
}

/// Show `g` at its current tick: previous positions equal the current ones
/// and no events wait to play.
pub fn hold(g: &mut Game) {
    g.hero.prev = g.hero.body.pos;
    for e in &mut g.enemies {
        e.prev = e.body.pos;
    }
    g.cam.prev = g.cam.focus;
    g.plats_prev.clone_from(&g.plats);
    g.events.clear();
}

fn at(a: &Attract, tick: usize) -> Result<Game, String> {
    a.game_at(tick).ok_or_else(|| format!("tick {tick} is past the recording"))
}

/// Ticks of event `e` in the recording.
fn ticks(a: &Attract, e: Event) -> impl Iterator<Item = usize> + '_ {
    a.events.iter().filter(move |(_, x)| *x == e).map(|(t, _)| *t)
}

pub fn start(world: &World) -> Result<(), String> {
    stage(world, |a| at(a, 0))
}

/// The stomp nearest the middle of the run (the attract demo's), played as
/// a player who holds jump through it for the full bounce and lets go of
/// the stick: Fennel at the highest point of the bounce where he is still
/// above the flattened beetle (within [`STOMP_DX`] of it).
pub fn stomp(world: &World) -> Result<(), String> {
    stage(world, |a| {
        let s = ticks(a, Event::Stomp).filter(|&s| s <= a.keys[1]).last().ok_or("the recording stomps no enemy")?;
        let mut g = at(a, s)?;
        let mut beetle: Option<usize> = None;
        let mut best: Option<(f32, Game)> = None;
        for i in s..(s + 40).min(a.len()) {
            a.replay_with(&mut g, i, 1, |c, k| Controls { jump: true, x: if k > s { 0.0 } else { c.x }, ..c });
            let e = *beetle.get_or_insert_with(|| g.enemies.iter().position(|e| e.squashed >= 0.0 && e.squashed < 0.1).unwrap_or(usize::MAX));
            let Some(e) = g.enemies.get(e) else {
                return Err(format!("no enemy squashed at tick {s}"));
            };
            let h = g.hero.body.pos;
            if (h[0] - e.body.pos[0]).abs() <= STOMP_DX && best.as_ref().is_none_or(|(y, _)| h[1] < *y) {
                best = Some((h[1], g.clone()));
            }
            if g.hero.body.vel[1] >= 0.0 {
                break;
            }
        }
        best.map(|(_, g)| g).ok_or_else(|| format!("Fennel is never above the beetle after the stomp at tick {s}"))
    })
}

/// The extra life: the first coin of the longest arc of coins taken in
/// mid-air (the earliest of equals), with the count set one short of an
/// extra life just before it, shown [`AFTER_ONE_UP`] ticks later with the
/// rest of the arc ahead.
pub fn power_up(world: &World) -> Result<(), String> {
    stage(world, |a| {
        let coins: Vec<usize> = ticks(a, Event::Coin).collect();
        let mut best: Option<(usize, usize)> = None;
        let mut i = 0;
        while i < coins.len() {
            let mut j = i;
            while j + 1 < coins.len() && coins[j + 1] - coins[j] <= ARC_GAP {
                j += 1;
            }
            let (t, n) = (coins[i], j - i + 1);
            if n >= 3 && at(a, t).is_ok_and(|g| !g.hero.body.grounded) && best.is_none_or(|(_, b)| n > b) {
                best = Some((t, n));
            }
            i = j + 1;
        }
        let (t, _) = best.ok_or("the recording takes no arc of coins in a jump")?;
        let mut g = at(a, t)?;
        g.coins = (a.ints().coins_per_life.max(1) - 1) as u32;
        a.replay(&mut g, t, AFTER_ONE_UP);
        Ok(g)
    })
}

pub fn checkpoint(world: &World) -> Result<(), String> {
    stage(world, |a| {
        let t = ticks(a, Event::Checkpoint).next().ok_or("the recording reaches no checkpoint")?;
        at(a, t + AFTER_CHECKPOINT)
    })
}

/// The tick where the hero is closest above a spike tile.
pub fn spikes(world: &World) -> Result<(), String> {
    stage(world, |a| {
        let mut g = at(a, 0)?;
        let l = g.level.clone();
        let spikes: Vec<[f32; 2]> = (0..l.h as i32)
            .flat_map(|ty| (0..l.w as i32).map(move |tx| (tx, ty)))
            .filter(|&(tx, ty)| l.tile(tx, ty) == Tile::Spikes)
            .map(|(tx, ty)| [(tx as f32 + 0.5) * TILE, ty as f32 * TILE])
            .collect();
        if spikes.is_empty() {
            return Err("the level has no spikes".into());
        }
        let mut best: Option<(f32, usize)> = None;
        for t in 0..a.len() {
            if g.phase != Phase::Playing {
                break;
            }
            if g.hero.alive() {
                let p = g.hero.body.pos;
                for s in &spikes {
                    let (dx, dy) = ((p[0] - s[0]).abs(), s[1] - p[1]);
                    if dx < TILE && dy >= 0.0 && best.is_none_or(|(b, _)| dy < b) {
                        best = Some((dy, t));
                    }
                }
            }
            a.replay(&mut g, t, 1);
        }
        let (_, t) = best.ok_or("the recording never passes over the spikes")?;
        at(a, t)
    })
}

/// The first bottomless pit (the first run of columns whose bottom row is
/// empty): its first column, and its width in columns.
pub fn first_pit(l: &crate::level::Level) -> Option<(i32, i32)> {
    let open = |tx: i32| l.tile(tx, l.h as i32 - 1) == Tile::Empty;
    let first = (0..l.w as i32).find(|&tx| open(tx))?;
    let width = (first..l.w as i32).take_while(|&tx| open(tx)).count() as i32;
    Some((first, width))
}

/// The tick where the hero is nearest the middle of the first bottomless
/// pit, so both lips and the whole gap are in frame.
pub fn pit(world: &World) -> Result<(), String> {
    stage(world, |a| {
        let mut g = at(a, 0)?;
        let (first, width) = first_pit(&g.level).ok_or("the level has no bottomless pit")?;
        let mid = (first as f32 + width as f32 * 0.5) * TILE;
        let mut best: Option<(f32, usize)> = None;
        for t in 0..a.len() {
            if g.phase != Phase::Playing {
                break;
            }
            let d = (g.hero.body.pos[0] - mid).abs();
            if g.hero.alive() && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, t));
            }
            a.replay(&mut g, t, 1);
        }
        let (_, t) = best.ok_or("the recording never reaches the pit")?;
        at(a, t)
    })
}

/// The attract demo's last key frame: in the castle gate.
pub fn goal(world: &World) -> Result<(), String> {
    stage(world, |a| at(a, a.keys[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformerPlugin;
    use crate::game::PopKind;
    use evani_core::App;

    fn app() -> App {
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_look(evani_look_pixel16::Pixel16Look)
            .with_kit(PlatformerPlugin);
        a.start();
        a
    }

    #[test]
    fn every_moment_stages_its_shot() {
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        let a = app();
        let game = |a: &App| a.world.res::<Platformer>().game.clone();
        for name in ["start", "stomp", "power_up", "checkpoint", "spikes", "pit", "goal"] {
            let f = m.get(name).unwrap_or_else(|| panic!("no moment {name}")).setup;
            f(&a.world).unwrap_or_else(|e| panic!("{name}: {e}"));
            let g = game(&a);
            assert!(g.hero.alive(), "{name}: hero down");
            assert_eq!(g.hero.prev, g.hero.body.pos, "{name}: not held");
            assert!(g.events.is_empty());
            match name {
                "start" => assert!(g.coins == 0 && g.tick == 0 && g.phase == Phase::Playing),
                "stomp" => {
                    // High above the flattened beetle, not past it.
                    let e = g.enemies.iter().find(|e| e.squashed >= 0.0 && e.squashed < 0.45).expect("no fresh stomp");
                    let (h, b) = (g.hero.body.pos, e.body.pos);
                    assert!((h[0] - b[0]).abs() <= STOMP_DX, "hero {h:?} beside the beetle {b:?}");
                    assert!(b[1] - h[1] >= 28.0, "hero {h:?} low over the beetle {b:?}");
                    assert!(g.hero.body.vel[1] < 0.0, "still rising");
                }
                "power_up" => {
                    assert!(g.pops.iter().any(|p| p.kind == PopKind::OneUp), "no 1UP");
                    assert!(g.lives > 3);
                    assert!(!g.hero.body.grounded);
                    // At least three coins of the arc are still ahead.
                    let h = g.hero.body.pos;
                    let ahead = g.level.coins.iter().zip(&g.coins_taken).filter(|(c, taken)| !**taken && c[0] > h[0] && c[0] - h[0] < 64.0 && (c[1] - h[1]).abs() < 48.0).count();
                    assert!(ahead >= 3, "{ahead} coins ahead of {h:?}");
                }
                "checkpoint" => assert!(g.flags.iter().any(|f| *f)),
                "spikes" => {
                    let p = g.hero.body.pos;
                    let near = (0..4).any(|k| [-TILE, 0.0, TILE].iter().any(|dx| g.level.tile_at(p[0] + dx, p[1] + k as f32 * TILE + 1.0) == Tile::Spikes));
                    assert!(near, "no spikes just below {p:?}");
                }
                "pit" => {
                    let (first, width) = first_pit(&g.level).unwrap();
                    let x = g.hero.body.pos[0] / TILE;
                    assert!(x > first as f32 && x < (first + width) as f32, "hero at column {x} not over the pit {first}+{width}");
                }
                "goal" => assert_eq!(g.phase, Phase::Won),
                _ => unreachable!(),
            }
        }
        // The same moment twice is the same frame.
        let f = m.get("power_up").unwrap().setup;
        f(&a.world).unwrap();
        let one = game(&a);
        f(&a.world).unwrap();
        assert_eq!(one.hero, game(&a).hero);
        assert_eq!(one.pops, game(&a).pops);
    }

    /// Live builds: a staged moment holds still until the player acts, and
    /// the run video plays real play (jumps, coins, a stomp, a checkpoint)
    /// without a hit.
    #[cfg(feature = "showcase")]
    #[test]
    fn a_staged_moment_holds_and_the_run_video_plays() {
        use crate::Show;
        use evani_core::input::{Input, KeyCode, RawEvent};
        let key = |a: &mut App, down: bool| {
            a.world.res_mut::<Input>().handle(&RawEvent::Key { code: KeyCode::new("KeyD").unwrap(), down });
        };
        let step = |a: &mut App| a.run_headless(1, 1000.0 / 60.0);
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        let mut a = app();
        let game = |a: &App| a.world.res::<Platformer>().game.clone();
        (m.get("stomp").unwrap().setup)(&a.world).unwrap();
        let held = game(&a);
        for _ in 0..30 {
            step(&mut a);
        }
        assert_eq!(game(&a).hero, held.hero, "the stomp shot moved on");
        assert_eq!(game(&a).tick, held.tick);
        // The player acts: the game plays on from the shot.
        key(&mut a, true);
        for _ in 0..5 {
            step(&mut a);
        }
        assert_eq!(a.world.res::<Platformer>().show, None);
        assert_ne!(game(&a).hero, held.hero);
        key(&mut a, false);
        step(&mut a);

        (m.get("run").unwrap().setup)(&a.world).unwrap();
        let from = run_start(&a.world.res::<Platformer>().attract);
        let g0 = game(&a);
        let (mut jumps, mut grounded) = (0, g0.hero.body.grounded);
        let at = |a: &App| match a.world.res::<Platformer>().show {
            Some(Show::Run { at }) => at,
            other => panic!("the run video stopped: {other:?}"),
        };
        while at(&a) < from + RUN_WINDOW {
            step(&mut a);
            let g = game(&a);
            jumps += usize::from(grounded && !g.hero.body.grounded && g.hero.body.vel[1] < 0.0);
            grounded = g.hero.body.grounded;
            assert_eq!(g.lives, g0.lives, "a hit in the run video");
        }
        let g = game(&a);
        let ran = g.hero.body.pos[0] - g0.hero.body.pos[0];
        assert!(ran > 30.0 * TILE, "ran only {ran} px");
        let checkpoint = g.flags.iter().filter(|f| **f).count() > g0.flags.iter().filter(|f| **f).count();
        assert!(
            g.stomps > g0.stomps && g.coins >= g0.coins + 5 && jumps >= 4 && checkpoint,
            "stomps {} coins {} jumps {jumps} checkpoint {checkpoint}",
            g.stomps - g0.stomps,
            g.coins - g0.coins
        );
    }
}
