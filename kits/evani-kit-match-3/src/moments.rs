//! Scripted capture moments for live's `capture.frame { moment }`: the
//! review's showcase shots of the target pack's signature moments
//! (`kits/match-3/target/moments.md`). Each moment plays the game in the
//! world itself (the real game, so the DOM HUD and the result cards show it
//! too) from a fresh, seeded level with the best or the weakest bot, and
//! stops on the first tick that shows what the moment asks for. Levels are
//! seeded and the bot is deterministic, so a moment is the same frame every
//! time, whatever ran before.
//!
//! A moment's setup only gets `&World`, and pieces are entities, so the
//! setup asks for the moment ([`DrawState::stage`]) and the draw system
//! plays it on its next frame with `&mut World` ([`play`]). Live waits a few
//! frames after the setup before it captures, and holds the sim clock, so
//! the capture shows exactly the state [`play`] left. While a staged moment
//! is held, the title's attract replay (`crate::Showcase`) stays off.
//!
//! Only the `showcase` feature (the app's `live`) builds this, so a shipped
//! game never has it.

use core::cell::{Cell, RefCell};

use evani_core::capture::CaptureMoments;
use evani_core::ecs::World;
use evani_core::formulas::Formulas;
use evani_core::params::ParamRegistry;
use evani_core::particles::{Emit, ParticleQueue};
use evani_core::time::Time;

use crate::board::Kind;
use crate::draw::DrawState;
use crate::ecs::{Blast, Floats, Grid, Motion, Popping};
use crate::game::{self, Fns, Game, Phase, PlayerInput, Request, Step, default_match_score};
use crate::{Match3Kit, fx_flags};

/// The signature moments, in the order the review shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    Title,
    Match,
    Cascade,
    Bomb,
    Prism,
    SugarRush,
    Win,
    Lose,
    /// Not a target-pack moment: the review's run video. Level 1 from its
    /// first move, then the best bot keeps playing in the running game
    /// ([`DrawState::autoplay`]) so the video shows real play.
    Play,
    /// Not a target-pack moment: a board with no move left reshuffles and
    /// says so (the playbook's "No dead boards").
    Shuffle,
}

/// Every moment with its id (as in the target pack; the last two are the
/// kit's own) and what it shows.
pub const MOMENTS: [(Moment, &str, &str); 10] = [
    (Moment::Title, "title", "Title screen over the attract board, Puff beside it, a match popping"),
    (Moment::Match, "match", "Level 1, first move: a line of three pops with its score"),
    (Moment::Cascade, "cascade", "A cascade: pieces falling, the callout off the board beside Puff"),
    (Moment::Bomb, "bomb", "A Pop Bomb goes off: shockwave rings, sparkles and shake"),
    (Moment::Prism, "prism", "A Prism clears a colour: beams to every piece of it"),
    (Moment::SugarRush, "sugar_rush", "Level won: left-over moves fire as Streaks (Sugar Rush)"),
    (Moment::Win, "win", "The win card with stars and score over the cleared board"),
    (Moment::Lose, "lose", "Out of moves: the lose card, Puff sad"),
    (Moment::Play, "play", "Level 1 played by the best bot from its first move (the run video)"),
    (Moment::Shuffle, "shuffle", "A board with no move left reshuffles: pieces trade places, the Shuffle! callout"),
];

/// Register the moments in review order.
pub fn register_capture_moments(m: &mut CaptureMoments) -> Result<(), String> {
    for (_, name, label) in MOMENTS {
        m.add(name, label, setup_of(name)?)?;
    }
    Ok(())
}

fn setup_of(name: &str) -> Result<evani_core::capture::MomentSetup, String> {
    Ok(match name {
        "title" => |w: &World| ask(w, Moment::Title),
        "match" => |w: &World| ask(w, Moment::Match),
        "cascade" => |w: &World| ask(w, Moment::Cascade),
        "bomb" => |w: &World| ask(w, Moment::Bomb),
        "prism" => |w: &World| ask(w, Moment::Prism),
        "sugar_rush" => |w: &World| ask(w, Moment::SugarRush),
        "win" => |w: &World| ask(w, Moment::Win),
        "lose" => |w: &World| ask(w, Moment::Lose),
        "play" => |w: &World| ask(w, Moment::Play),
        "shuffle" => |w: &World| ask(w, Moment::Shuffle),
        other => return Err(format!("no capture moment {other}")),
    })
}

/// The setup: check the moment can be played here and ask the draw system
/// to play it.
fn ask(world: &World, m: Moment) -> Result<(), String> {
    let free_roam = world.try_res::<Game>().ok_or("the match-3 kit is not loaded")?.free_roam;
    if free_roam && matches!(m, Moment::SugarRush | Moment::Win | Moment::Lose) {
        return Err("free roam has no Sugar Rush, win or loss: open the game without ?mode=free".into());
    }
    let mut ds = world.try_res_mut::<DrawState>().ok_or("the match-3 draw state is missing")?;
    ds.stage = Some(m);
    Ok(())
}

/// Which bot plays when the board waits for a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bot {
    Best,
    Worst,
}

/// Ticks one level may take (a bot game ends well inside it).
const MAX_TICKS: u32 = 60 * 60 * 5;

/// How many of the last replayed ticks keep their sparkles for the capture
/// (the longest one lives 2.6 s, 156 ticks).
const KEEP_TICKS: u32 = 180;

/// Take this tick's emits out of the queue (the replay runs many ticks in
/// one frame, so they would all land at age 0), stamped with the replay tick
/// they were made in, and forget those too old to still be alive.
fn collect_emits(world: &World, n: u32, kept: &mut Vec<(u32, Emit)>) {
    let mut got = Vec::new();
    world.res_mut::<ParticleQueue>().drain_into(&mut got);
    kept.extend(got.iter().map(|s| (n, s.emit)));
    kept.retain(|(at, _)| n - at < KEEP_TICKS);
}

/// Play moment `m` on the game in `world`, leaving the first tick that
/// shows it.
pub fn play(world: &mut World, m: Moment) -> Result<(), String> {
    let k = world.try_res::<Match3Kit>().map(|k| *k).ok_or("the match-3 kit is not loaded")?;
    let (t, rules) = {
        let reg = world.res::<ParamRegistry>();
        (k.tune.read(&reg), k.rules(&reg))
    };
    let fx = fx_flags(world);
    let dt = world.res::<Time>().fixed_dt;
    let score = move |w: &World, p: f32, c: f32, s: f32| {
        w.try_res::<Formulas>()
            .map_or(default_match_score(p, c, s), |f| f.eval(k.match_score, &[p, c, s]))
    };
    let fns = Fns { match_score: &score };
    let result_at = t.result_delay_s + 0.5;
    // Score texts and callouts are juice: without it only the pieces show.
    let juice = fx.juice;
    // Play `level` (or the title board) with `bot` until `done` holds.
    let kept: RefCell<Vec<(u32, Emit)>> = RefCell::new(Vec::new());
    let last = Cell::new(0u32);
    let find = |world: &mut World, start: Request, bot: Option<Bot>, done: &dyn Fn(&World) -> bool| -> bool {
        kept.borrow_mut().clear();
        {
            let mut g = world.res_mut::<Game>();
            g.rules = rules;
            g.paused = false;
            g.request(start);
        }
        for n in 0..MAX_TICKS {
            {
                let mut g = world.res_mut::<Game>();
                if g.phase == Phase::Play
                    && g.step == Step::Idle
                    && let Some(b) = bot
                {
                    let grid = world.res::<Grid>();
                    let mv = match b {
                        Bot::Best => grid.board.best_move(),
                        Bot::Worst => grid.board.worst_move(),
                    };
                    if let Some(mv) = mv {
                        g.request(Request::Swap(mv.a, mv.b));
                    }
                }
            }
            game::tick(world, &PlayerInput::default(), &t, &fns, &fx, dt);
            world.res_mut::<Game>().events.clear();
            collect_emits(world, n, &mut kept.borrow_mut());
            last.set(n);
            if done(world) {
                return true;
            }
            let g = world.res::<Game>();
            let ended = matches!(g.phase, Phase::Won | Phase::Lost) && g.phase_t > result_at + 1.0;
            if ended {
                return false;
            }
        }
        false
    };
    let levels = world.res::<Game>().levels.len();
    let found = match m {
        Moment::Title => find(world, Request::Title, None, &popping),
        Moment::Match => find(world, Request::Start(0), Some(Bot::Best), &|w| popping(w) && (!juice || score_text(w))),
        Moment::Cascade => {
            (0..levels).any(|l| find(world, Request::Start(l), Some(Bot::Best), &|w| cascade(w, 2, juice)))
                || find(world, Request::Start(0), Some(Bot::Best), &|w| cascade(w, 1, juice))
        }
        Moment::Bomb => (0..levels).any(|l| find(world, Request::Start(l), Some(Bot::Best), &|w| blasting(w, Kind::Bomb))),
        Moment::Prism => (0..levels).any(|l| find(world, Request::Start(l), Some(Bot::Best), &|w| blasting(w, Kind::Prism))),
        Moment::SugarRush => find(world, Request::Start(0), Some(Bot::Best), &|w| {
            w.res::<Game>().phase == Phase::Rush && (blasting(w, Kind::StreakH) || blasting(w, Kind::StreakV))
        }),
        Moment::Win => find(world, Request::Start(0), Some(Bot::Best), &|w| {
            let g = w.res::<Game>();
            g.phase == Phase::Won && g.phase_t >= result_at
        }),
        Moment::Lose => find(world, Request::Start(0), Some(Bot::Worst), &|w| {
            let g = w.res::<Game>();
            g.phase == Phase::Lost && g.phase_t >= result_at
        }),
        Moment::Play => {
            let ok = find(world, Request::Start(0), None, &|w| {
                let g = w.res::<Game>();
                g.phase == Phase::Play && g.step == Step::Idle
            });
            // The running game plays on from here (`crate::sim_system`).
            world.res_mut::<DrawState>().autoplay = ok;
            ok
        }
        Moment::Shuffle => {
            find(world, Request::Start(0), None, &|w| {
                let g = w.res::<Game>();
                g.phase == Phase::Play && g.step == Step::Idle
            }) && {
                dead_board(world)?;
                // The board settles again: no move left, so it reshuffles.
                world.res_mut::<Game>().step = Step::Fall;
                let half = t.shuffle_s * 0.5;
                // (Carry on as is: unpaused, the same level.)
                find(world, Request::Pause(false), None, &|w| matches!(w.res::<Game>().step, Step::Shuffle { t } if t >= half))
            }
        }
    };
    if found {
        // The sparkles the replay made show at the age they have by now:
        // what was alive before the moment is gone.
        let mut q = world.res_mut::<ParticleQueue>();
        q.clear();
        for (at, e) in kept.borrow().iter() {
            q.emit_ago(last.get() - at, *e);
        }
        drop(q);
        // The draw animates at the moment's own time while it is held.
        world.res_mut::<DrawState>().moment_t = Some(f64::from(last.get() + 1) * f64::from(dt));
        Ok(())
    } else { Err(format!("the bot never reached the {m:?} moment")) }
}

/// Recolour the settled board in place (every piece keeps its entity) to a
/// pattern with no move left: rows of two colours alternating, the next row
/// two other colours.
fn dead_board(world: &mut World) -> Result<(), String> {
    let mut grid = world.res_mut::<Grid>();
    if grid.board.colors < 4 {
        return Err("the shuffle moment needs a level with four colours or more".into());
    }
    for i in 0..grid.board.len() {
        let (x, y) = grid.board.xy(i);
        let c = if y % 2 == 0 { (x % 2) as u8 } else { 2 + (x % 2) as u8 };
        if let Some(p) = grid.board.cells[i].as_mut() {
            p.kind = Kind::Normal;
            p.color = c;
        }
        let piece = grid.board.cells[i];
        if let (Some(e), Some(p)) = (grid.slots.get(i).copied().flatten(), piece)
            && let Some(mut pc) = world.get_mut::<crate::board::Piece>(e)
        {
            *pc = p;
        }
    }
    if grid.board.has_move() || !grid.board.groups().is_empty() {
        return Err("the dead-board pattern still has a move".into());
    }
    Ok(())
}

/// A match is part way through its pop.
fn popping(w: &World) -> bool {
    w.res::<Grid>()
        .pops
        .iter()
        .any(|&e| w.get::<Popping>(e).is_some_and(|p| p.t > 0.06 && p.t < 0.16))
}

/// A score text is up and readable.
fn score_text(w: &World) -> bool {
    w.res::<Floats>().items.iter().any(|f| f.style == 0 && f.age < f.life * 0.6)
}

/// Cascade round `n` or later: its callout is up (and not fading; with
/// juice) while pieces fall.
fn cascade(w: &World, n: u32, juice: bool) -> bool {
    if w.res::<Game>().cascade < n || w.res::<Game>().step != Step::Fall {
        return false;
    }
    let callout = w.res::<Floats>().items.iter().any(|f| f.style >= 1 && f.age > 0.15 && f.age < f.life * 0.6);
    let mut falling = false;
    w.query::<&Motion>().for_each(|_, m| falling |= m.off[1] < -0.3);
    (callout || !juice) && falling
}

/// A special of `kind` has just gone off (its rings and beams are bright).
fn blasting(w: &World, kind: Kind) -> bool {
    w.res::<Grid>()
        .blasts
        .iter()
        .any(|&e| w.get::<Blast>(e).is_some_and(|b| b.kind == kind && b.t > 0.05 && b.t < 0.2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Match3Plugin;
    use crate::draw_system;
    use crate::hud::Match3Hud;
    use evani_core::App;
    use evani_render::RenderWorld;

    fn app() -> App {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Match3Plugin);
        a.start();
        a.world.res_mut::<RenderWorld>().viewport_css = [1920.0, 1080.0];
        a
    }

    /// Run moment `name` as live does: the setup, then a held draw frame.
    fn stage(a: &mut App, m: &CaptureMoments, name: &str) {
        let f = m.get(name).unwrap_or_else(|| panic!("no moment {name}")).setup;
        f(&a.world).unwrap_or_else(|e| panic!("{name}: {e}"));
        a.world.res_mut::<Time>().held = Some(12.5);
        draw_system(&mut a.world);
        assert!(a.world.res::<DrawState>().stage.is_none(), "{name}: not played");
    }

    #[test]
    fn every_moment_stages_its_shot() {
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        assert_eq!(m.names(), MOMENTS.map(|x| x.1));
        let mut a = app();
        for (moment, name, _) in MOMENTS {
            stage(&mut a, &m, name);
            let w = &a.world;
            let juice = fx_flags(w).juice;
            assert_eq!(w.res::<evani_core::overlay::ErrorLog>().error_count(), 0, "{name}: not found");
            let g = w.res::<Game>();
            let hud = w.res::<Match3Hud>();
            assert!(g.events.is_empty(), "{name}: queued sounds");
            match moment {
                Moment::Title => assert!(g.phase == Phase::Title && popping(w)),
                Moment::Match => assert!(g.phase == Phase::Play && g.cascade == 0 && popping(w) && (!juice || score_text(w))),
                Moment::Cascade => {
                    assert!(cascade(w, 2, juice), "{name}: cascade {}", g.cascade);
                    assert!(!juice || hud.floats.iter().any(|f| f.style >= 1), "{name}: no callout in the HUD");
                }
                Moment::Bomb => assert_eq!(hud.blasting & 2, 2, "{name}"),
                Moment::Prism => assert_eq!(hud.blasting & 4, 4, "{name}"),
                Moment::SugarRush => assert!(g.phase == Phase::Rush && hud.blasting & 1 == 1),
                Moment::Win => assert!(g.phase == Phase::Won && hud.result.ready && hud.result.won && hud.result.stars >= 1),
                Moment::Lose => assert!(g.phase == Phase::Lost && hud.result.ready && !hud.result.won),
                Moment::Play => assert!(g.phase == Phase::Play && g.step == Step::Idle && w.res::<DrawState>().autoplay, "{name}"),
                Moment::Shuffle => {
                    assert!(matches!(g.step, Step::Shuffle { .. }), "{name}: {:?}", g.step);
                    assert!(hud.floats.iter().any(|f| f.style == 1 && f.text == game::SHUFFLE_CALLOUT), "{name}: no Shuffle! callout: {:?}", hud.floats);
                }
            }
            // Only the play moment leaves the bot playing.
            assert_eq!(w.res::<DrawState>().autoplay, moment == Moment::Play, "{name}");
        }
        // The play moment's bot plays on in the running game: several
        // moves in ten seconds, and it stops when another moment is staged.
        stage(&mut a, &m, "play");
        a.world.res_mut::<Time>().held = None;
        for _ in 0..600 {
            crate::sim_system(&mut a.world);
        }
        let moves = a.world.res::<Game>().moves_used;
        assert!(moves >= 4, "the bot made {moves} moves in 10 s");
        stage(&mut a, &m, "match");
        assert!(!a.world.res::<DrawState>().autoplay);
        // The held title moment shows the staged board, not the attract
        // replay; the replay comes back once the clock runs again.
        stage(&mut a, &m, "title");
        assert!(a.world.res::<DrawState>().staged && a.world.res::<DrawState>().showcase.is_none());
        a.world.res_mut::<Time>().held = None;
        draw_system(&mut a.world);
        assert!(!a.world.res::<DrawState>().staged);
        a.world.res_mut::<Time>().held = Some(20.0);
        draw_system(&mut a.world);
        assert!(a.world.res::<DrawState>().showcase.is_some(), "the attract replay is back");
        // The same moment twice is the same frame, whatever ran before.
        let frame = |a: &mut App, name: &str| {
            stage(a, &m, name);
            a.world.res_mut::<RenderWorld>().sprites.clear();
            draw_system(&mut a.world);
            a.world.res::<RenderWorld>().sprites.clone()
        };
        let one = frame(&mut a, "bomb");
        let _ = frame(&mut a, "lose");
        assert_eq!(one, frame(&mut a, "bomb"));
        let mut b = app();
        assert_eq!(one, frame(&mut b, "bomb"));
    }

    /// Live holds the clock wherever the session's clock happened to be when
    /// it staged the moment (it differs run to run); the picture must not.
    #[test]
    fn a_moment_is_the_same_picture_whatever_time_the_clock_is_held_at() {
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        let shot = |held: f64, name: &str| {
            let mut a = app();
            let f = m.get(name).unwrap().setup;
            f(&a.world).unwrap();
            a.world.res_mut::<Time>().held = Some(held);
            draw_system(&mut a.world);
            // A few more held frames, as live waits before it captures.
            for _ in 0..3 {
                a.world.res_mut::<RenderWorld>().sprites.clear();
                draw_system(&mut a.world);
            }
            a.world.res::<RenderWorld>().sprites.clone()
        };
        for name in ["title", "bomb", "win"] {
            let one = shot(9.25, name);
            assert!(!one.is_empty());
            assert_eq!(one, shot(31.79, name), "{name}");
        }
    }

    /// The fixed picture of a moment shows Puff's face, never a blink.
    #[test]
    fn a_staged_moment_never_catches_puff_mid_blink() {
        use crate::draw::{BLINK_LEN, BLINK_PERIOD, moment_clock};
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        let mut checked = 0;
        for name in m.names() {
            let mut a = app();
            stage(&mut a, &m, name);
            let Some(mt) = a.world.res::<DrawState>().moment_t else { continue };
            let t = moment_clock(mt);
            assert!(t % BLINK_PERIOD >= BLINK_LEN, "{name} at {t}");
            assert!(t - mt <= BLINK_LEN + 1.0 / 60.0, "{name}");
            checked += 1;
        }
        assert!(checked >= 5, "only {checked} moments have a replay time");
        // A replay time inside a blink steps just past it.
        assert!((moment_clock(BLINK_PERIOD * 2.0 + 0.05) - (BLINK_PERIOD * 2.0 + BLINK_LEN + 1.0 / 60.0)).abs() < 1e-9);
        assert_eq!(moment_clock(1.0), 1.0);
    }

    #[test]
    fn free_roam_refuses_the_end_moments() {
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        let a = app();
        a.world.res_mut::<Game>().free_roam = true;
        for name in ["sugar_rush", "win", "lose"] {
            let f = m.get(name).unwrap().setup;
            assert!(f(&a.world).is_err(), "{name}");
        }
        let f = m.get("cascade").unwrap().setup;
        assert!(f(&a.world).is_ok());
    }
}
