//! `evani-kit-match-3`: a match-3 puzzle. Swap validation, cascades with
//! chain reactions, special pieces and their combos, level goals with move
//! limits and star scores, boards generated with no starting matches, hints
//! and reshuffles; mouse, touch, keyboard and gamepad on one code path. No
//! physics module.
//!
//! * Params (owner `kit.match-3`): every tunable is `kit.match_3.*` (see
//!   [`Tune`] and SPEC.md), plus `kit.match_3.bomb_radius` and
//!   `kit.match_3.gen_attempts`.
//! * Formula: `kit.match_3.match_score(pieces, cascade, special)`.
//! * Input actions: `up`, `down`, `left`, `right`, `pick`, `cancel`,
//!   `pause`; the primary pointer (mouse or touch) drives swipes and taps.
//! * Data file: `match_3.levels` (`data/levels.json`).
//! * Entities ([`ecs`]): every piece on the board (`match_3.piece`,
//!   `match_3.cell`, `match_3.motion`), popping pieces (`match_3.popping`),
//!   blasts (`match_3.blast`) and Puff (`match_3.puff`).
//! * Resources: [`game::Game`] (the session: level, phase, moves, score;
//!   the app sends it [`game::Request`]s), [`ecs::Grid`] (the board as the
//!   rules see it, a view of the piece entities), the [`ecs::FxStream`] and
//!   [`ecs::Floats`] pools, and [`hud::Match3Hud`] (what a HUD shows).
//! * Free roam (`PlayMode::FreeRoam`): moves cost nothing and there are no
//!   goals, Sugar Rush, win or loss; the board keeps refilling. A finished
//!   level reports a game over in `PlayOutcomes`.
//! * Systems: `match_3.sim` (fixed `Update`) and `match_3.draw` (`Frame`:
//!   layout, sprites, shake, sounds, HUD).
//!
//! The kit reads its colours from the look's `pieces` and `ui` palettes and
//! its style numbers from `look.<look>.*` params, with flat defaults, so it
//! runs with any look.

pub mod board;
#[cfg(test)]
mod determinism;
#[cfg(test)]
mod review;
pub mod draw;
pub mod ecs;
pub mod game;
pub mod hud;
pub mod layout;
pub mod level;
#[cfg(feature = "showcase")]
pub mod moments;
pub mod sfx;
pub mod sparks;

use evani_core::audio::AudioMixer;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::formulas::{FormulaId, FormulaJson, Formulas};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::overlay::ErrorLog;
use evani_core::play::PlayMode;
use evani_core::params::{F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, LookData, PluginContext, PluginError};

use evani_core::time::Time;
use evani_render::RenderWorld;

use crate::board::Rules;
use crate::ecs::Grid;
use crate::draw::{DrawFx, DrawState, Frame, Style, fx_on, param_f32};
use crate::game::{Event, Fns, FxFlags, Game, Phase, PlayerInput, Request, default_match_score};
use crate::hud::Match3Hud;
use crate::layout::{CellLimits, layout};
use crate::sfx::{Sfx, Sounds};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// The demo's levels.
pub const LEVELS_JSON: &str = include_str!("../data/levels.json");

macro_rules! tunables {
    ($( $field:ident = $def:expr, [$min:expr, $max:expr], $unit:literal, $desc:literal; )*) => {
        /// Handles of every `f32` tunable.
        #[derive(Debug, Clone, Copy)]
        pub struct Handles {
            $(pub $field: F32Param,)*
        }

        /// Values of every `f32` tunable for one tick (defaults = SPEC.md).
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Tune {
            $(pub $field: f32,)*
        }

        impl Default for Tune {
            fn default() -> Self {
                Self { $($field: $def,)* }
            }
        }

        #[cfg(test)]
        impl Tune {
            /// Set a tunable by its full param name; false if unknown.
            fn set_by_name(&mut self, name: &str, v: f32) -> bool {
                $(if name == concat!("kit.match_3.", stringify!($field)) {
                    self.$field = v;
                    return true;
                })*
                false
            }
        }

        impl Handles {
            fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                Ok(Self {
                    $($field: kit.param_f32(
                        ParamDef::f32(concat!("kit.match_3.", stringify!($field)), $def)
                            .range($min, $max)
                            .unit($unit)
                            .describe($desc),
                    )?,)*
                })
            }

            /// Read every tunable.
            pub fn read(&self, reg: &ParamRegistry) -> Tune {
                Tune { $($field: reg.f32(self.$field),)* }
            }
        }
    };
}

tunables! {
    swipe_threshold = 0.35, [0.1, 1.0], "cells", "How far a drag must go to swap with the neighbour.";
    swap_s = 0.16, [0.03, 1.0], "s", "Swap animation time.";
    invalid_slide = 0.45, [0.0, 1.0], "cells", "How far a swap with no match slides before bouncing back.";
    clear_s = 0.24, [0.05, 1.5], "s", "Pop time of a cleared piece.";
    chain_delay_s = 0.035, [0.0, 0.3], "s", "Delay per cell as a blast sweeps outwards.";
    fall_gravity = 55.0, [1.0, 400.0], "cells/s2", "Falling pieces' acceleration.";
    fall_speed = 16.0, [1.0, 80.0], "cells/s", "Falling pieces' top speed.";
    land_squash_s = 0.16, [0.0, 1.0], "s", "Landing squash time.";
    shuffle_s = 0.6, [0.1, 3.0], "s", "Time pieces take to fly to their shuffled cells.";
    hint_delay_s = 5.0, [0.5, 30.0], "s", "Idle time before the hint shows.";
    intro_s = 1.4, [0.0, 10.0], "s", "Level intro card time.";
    attract_move_s = 1.6, [0.2, 10.0], "s", "Time between moves on the title's attract board.";
    rush_step_s = 0.25, [0.05, 2.0], "s", "Time between Sugar Rush Streaks after a win.";
    blast_score = 40.0, [0.0, 1000.0], "pts", "Score per piece cleared by a special.";
    trauma_streak = 0.25, [0.0, 1.0], "", "Screen-shake trauma when a Streak fires.";
    trauma_bomb = 0.4, [0.0, 1.0], "", "Screen-shake trauma when a Pop Bomb fires.";
    trauma_prism = 0.6, [0.0, 1.0], "", "Screen-shake trauma when a Prism fires.";
    trauma_cascade = 0.08, [0.0, 1.0], "", "Screen-shake trauma per cascade round.";
    float_s = 0.9, [0.1, 5.0], "s", "Floating score text life (at most the pop time, so it is gone before pieces fall through it).";
    callout_s = 0.8, [0.1, 5.0], "s", "Combo callout life (it fades out over its last 30 %).";
    min_cell_px = 28.0, [8.0, 200.0], "px", "Smallest cell size.";
    max_cell_px = 96.0, [16.0, 400.0], "px", "Largest cell size.";
    shake_px_per_unit = 40.0, [0.0, 200.0], "px/m", "Pixels of board shake per unit of fx.shake.amount.";
    result_delay_s = 0.8, [0.0, 5.0], "s", "Pause after the last move before the result screen.";
}

/// Input actions.
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    /// Up, down, left, right.
    pub dirs: [ActionId; 4],
    pub pick: ActionId,
    pub cancel: ActionId,
    pub pause: ActionId,
}

/// Handles the kit's systems read every tick (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct Match3Kit {
    pub tune: Handles,
    pub bomb_radius: I32Param,
    pub gen_attempts: I32Param,
    pub match_score: FormulaId,
    pub actions: Actions,
}

impl Match3Kit {
    pub fn rules(&self, reg: &ParamRegistry) -> Rules {
        Rules {
            bomb_radius: reg.i32(self.bomb_radius).clamp(0, 4),
            gen_attempts: reg.i32(self.gen_attempts).clamp(1, 1000) as u32,
        }
    }
}

/// The match-3 kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct Match3Plugin;

fn button(kit: &mut PluginContext, name: &str, binds: &[&str], desc: &str) -> Result<ActionId, PluginError> {
    Ok(kit.action(ActionDef::button(name, binds).map_err(|e| e.to_string())?.describe(desc)))
}

impl KitPlugin for Match3Plugin {
    fn name(&self) -> &'static str {
        "match-3"
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let tune = Handles::register(kit)?;
        let bomb_radius = kit.param_i32(
            ParamDef::i32("kit.match_3.bomb_radius", 1)
                .range(0.0, 4.0)
                .describe("Pop Bomb radius in cells (1 = 3x3)."),
        )?;
        let gen_attempts = kit.param_i32(
            ParamDef::i32("kit.match_3.gen_attempts", 50)
                .range(1.0, 1000.0)
                .describe("Board generation tries before the fallback pattern."),
        )?;
        let match_score = kit.formula(
            "kit.match_3.match_score",
            "Score for one match from its piece count, the cascade depth (0 = the swap itself) and whether it made a special (0 or 1).",
            &["pieces", "cascade", "special"],
            FormulaJson::Expr("pieces * 60 * (1 + cascade * 0.5) + special * 120".to_string()),
        )?;
        let dirs = [
            button(kit, "up", &["key:ArrowUp", "key:KeyW", "pad:button:12"], "Move the cursor up, or swap the picked piece up.")?,
            button(kit, "down", &["key:ArrowDown", "key:KeyS", "pad:button:13"], "Move the cursor down, or swap down.")?,
            button(kit, "left", &["key:ArrowLeft", "key:KeyA", "pad:button:14"], "Move the cursor left, or swap left.")?,
            button(kit, "right", &["key:ArrowRight", "key:KeyD", "pad:button:15"], "Move the cursor right, or swap right.")?,
        ];
        let pick = button(kit, "pick", &["key:Space", "key:Enter", "pad:button:0"], "Pick up the piece under the cursor.")?;
        let cancel = button(kit, "cancel", &["key:Backspace", "pad:button:1"], "Put the picked piece down.")?;
        let pause = button(kit, "pause", &["key:Escape", "key:KeyP", "pad:button:9"], "Pause the game.")?;

        kit.data_file("match_3.levels", LEVELS_JSON);
        // Values tuned live and written by bake.export (params.json,
        // slots/*.expr in this crate).
        kit.baked_files(BAKED);

        let levels = level::levels_from_json(LEVELS_JSON)?;
        let handles = Match3Kit {
            tune,
            bomb_radius,
            gen_attempts,
            match_score,
            actions: Actions {
                dirs,
                pick,
                cancel,
                pause,
            },
        };
        let rules = handles.rules(&kit.world.res::<ParamRegistry>());
        let free_roam = kit.world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
        game::init(kit.world, levels, rules, free_roam);
        kit.resource(Match3Hud::default());
        kit.resource(DrawState::default());
        kit.resource(handles);
        kit.system(SystemDesc::new("match_3.sim", Stage::Update, sim_system));
        kit.system(SystemDesc::new("match_3.draw", Stage::Frame, draw_system));
        Ok(())
    }

    /// Free roam (docs/PLAYGROUNDS.md): under `PlayMode::FreeRoam` moves
    /// cost nothing and there are no goals, Sugar Rush, win or loss; the
    /// board keeps refilling. A loss in play mode is reported in
    /// `PlayOutcomes`.
    fn free_roam(&self) -> bool {
        true
    }
}

/// Effect switches and settings for this tick.
fn fx_flags(world: &World) -> FxFlags {
    let reg = world.res::<ParamRegistry>();
    let d = FxFlags::default();
    FxFlags {
        juice: fx_on(world, "juice"),
        particles: fx_on(world, "particles"),
        particle_density: param_f32(&reg, "fx.particles.density", d.particle_density),
        particle_gravity: param_f32(&reg, "fx.particles.gravity", d.particle_gravity),
        shake_decay: param_f32(&reg, "fx.shake.decay", d.shake_decay),
        colors: world.try_res::<LookData>().map_or(d.colors, |l| draw::pieces_of(&l)),
    }
}

/// The fixed-step game update.
pub fn sim_system(world: &mut World) {
    let Some(k) = world.try_res::<Match3Kit>().map(|k| *k) else {
        return;
    };
    let dt = world.res::<Time>().fixed_dt;
    let (t, rules) = {
        let reg = world.res::<ParamRegistry>();
        (k.tune.read(&reg), k.rules(&reg))
    };
    let (input, pause) = {
        let i = world.res::<Input>();
        let a = &k.actions;
        (
            PlayerInput {
                pointer: i.primary_pointer(),
                dirs: [
                    i.just_pressed(a.dirs[0]),
                    i.just_pressed(a.dirs[1]),
                    i.just_pressed(a.dirs[2]),
                    i.just_pressed(a.dirs[3]),
                ],
                pick: i.just_pressed(a.pick),
                cancel: i.just_pressed(a.cancel),
            },
            i.just_pressed(a.pause),
        )
    };
    let fx = fx_flags(world);
    {
        let Some(mut g) = world.try_res_mut::<Game>() else {
            return;
        };
        g.rules = rules;
        if pause && matches!(g.phase, Phase::Intro | Phase::Play | Phase::Rush) {
            g.request(Request::TogglePause);
        }
    }
    let score = |w: &World, p: f32, c: f32, s: f32| {
        w.try_res::<Formulas>()
            .map_or(default_match_score(p, c, s), |f| f.eval(k.match_score, &[p, c, s]))
    };
    let fns = Fns { match_score: &score };
    #[cfg(feature = "showcase")]
    autoplay(world, &input, pause);
    game::tick(world, &input, &t, &fns, &fx, dt);
    if let Some(mut g) = world.try_res_mut::<Game>()
        && g.events.len() > 256
    {
        g.events.clear();
    }
}

/// Live captures: while [`DrawState::autoplay`] is on, the best bot makes a
/// move whenever the board has been still for a moment, so the review's run
/// video shows swaps, cascades and specials. It stops for good when the
/// level ends or the player presses anything.
#[cfg(feature = "showcase")]
fn autoplay(world: &mut World, input: &PlayerInput, pause: bool) {
    /// How long the board rests between the bot's moves (s).
    const THINK_S: f32 = 0.45;
    let Some(mut ds) = world.try_res_mut::<DrawState>() else {
        return;
    };
    if !ds.autoplay {
        return;
    }
    let touched = input.pointer.pressed || input.pointer.down || input.pick || input.cancel || pause || input.dirs.iter().any(|d| *d);
    let (Some(mut g), Some(grid)) = (world.try_res_mut::<Game>(), world.try_res::<crate::ecs::Grid>()) else {
        return;
    };
    if touched || g.paused || matches!(g.phase, Phase::Won | Phase::Lost | Phase::Title) {
        ds.autoplay = false;
        return;
    }
    if g.phase == Phase::Play
        && g.step == game::Step::Idle
        && g.idle_t >= THINK_S
        && g.selected.is_none()
        && let Some(mv) = grid.board.best_move()
    {
        g.request(Request::Swap(mv.a, mv.b));
    }
}

/// Sound for a game event.
fn sound_of(e: Event) -> Option<(Sfx, f32)> {
    Some(match e {
        Event::Select => (Sfx::Select, 0.5),
        Event::Swap => (Sfx::Swap, 0.6),
        Event::Invalid => (Sfx::Invalid, 0.7),
        Event::Pop(n) => (Sfx::Pop(n.min(4) as u8), 0.8),
        Event::Streak => (Sfx::Streak, 0.7),
        Event::Bomb => (Sfx::Bomb, 0.9),
        Event::Prism => (Sfx::Prism, 0.9),
        Event::Special => (Sfx::Special, 0.7),
        Event::Land => (Sfx::Land, 0.35),
        Event::Frost => (Sfx::Frost, 0.6),
        Event::Shuffle => (Sfx::Shuffle, 0.7),
        Event::Star(_) => (Sfx::Star, 0.8),
        Event::GoalDone => (Sfx::Goal, 0.8),
        Event::Win => (Sfx::Win, 0.9),
        Event::Lose => (Sfx::Lose, 0.9),
        Event::LevelStart => (Sfx::Start, 0.7),
    })
}

/// Live captures: the attract board replayed to the held sim time in a world
/// of its own, so a capture or bench frame at time `t` always shows the same
/// moment.
#[cfg(feature = "showcase")]
#[derive(Debug)]
pub struct Showcase {
    /// The replayed game.
    pub sim: game::SimWorld,
    /// The replay time the held time maps to (s).
    pub at: f64,
    /// Fixed ticks `at` stands for.
    at_ticks: u32,
    /// Fixed ticks the replay has run (more than `at_ticks` once it ran on
    /// to a popping match).
    ran: u32,
}

#[cfg(feature = "showcase")]
impl Showcase {
    fn new(world: &World, l: crate::layout::Layout) -> Option<Self> {
        let g = world.try_res::<Game>()?;
        let sim = game::SimWorld::new(g.levels.clone(), g.rules);
        sim.game_mut().layout = l;
        Some(Self {
            sim,
            at: 0.0,
            at_ticks: 0,
            ran: 0,
        })
    }

    /// A match is popping (part way through its pop).
    fn popping(&self) -> bool {
        let grid = self.sim.grid();
        grid.pops
            .iter()
            .any(|&e| self.sim.world.get::<ecs::Popping>(e).is_some_and(|p| p.t > 0.06 && p.t < 0.16))
    }
}

/// Bring the capture replay to the held time; false without one.
#[cfg(feature = "showcase")]
fn showcase_step(world: &World, held: f64, fx: &FxFlags, l: crate::layout::Layout) -> bool {
    let Some(k) = world.try_res::<Match3Kit>().map(|k| *k) else {
        return false;
    };
    let t = k.tune.read(&world.res::<ParamRegistry>());
    let formulas = world.res::<Formulas>();
    let score = |_: &World, p: f32, c: f32, s: f32| formulas.eval(k.match_score, &[p, c, s]);
    let fns = Fns { match_score: &score };
    let mut ds = world.res_mut::<DrawState>();
    // The board drops in and makes its first move about 2.5 s in, so the
    // replay leads the held time by 2 s, then shows the first moment a
    // match is popping (up to 4 s further): every capture shows play.
    let target = held + SHOWCASE_LEAD_S;
    let dt = 1.0 / 60.0;
    if ds.showcase.as_ref().is_none_or(|s| s.at > target + 1e-6) {
        ds.showcase = Showcase::new(world, l);
    }
    let Some(s) = ds.showcase.as_mut() else {
        return false;
    };
    while s.at + dt <= target + 1e-6 {
        s.at += dt;
        s.at_ticks += 1;
    }
    if s.ran > s.at_ticks {
        // The replay ran on past this moment: play it again from the start.
        let Some(again) = Showcase::new(world, l) else {
            return false;
        };
        s.sim = again.sim;
        s.ran = 0;
    }
    s.sim.game_mut().layout = l;
    let input = PlayerInput::default();
    while s.ran < s.at_ticks {
        s.sim.tick(&input, &t, &fns, fx, dt as f32);
        s.sim.game_mut().events.clear();
        s.ran += 1;
    }
    for _ in 0..240 {
        if s.popping() {
            break;
        }
        s.sim.tick(&input, &t, &fns, fx, dt as f32);
        s.sim.game_mut().events.clear();
        s.ran += 1;
    }
    true
}

/// How far the capture replay runs ahead of the held time (s).
#[cfg(feature = "showcase")]
pub const SHOWCASE_LEAD_S: f64 = 2.0;

/// Layout, sprites, shake, sounds and the HUD, once per rendered frame.
pub fn draw_system(world: &mut World) {
    let Some(k) = world.try_res::<Match3Kit>().map(|k| *k) else {
        return;
    };
    let tune = k.tune.read(&world.res::<ParamRegistry>());
    // First frame: the look's style and the sounds.
    {
        let mut ds = world.res_mut::<DrawState>();
        let rev = world.res::<ParamRegistry>().revision();
        if ds.style.is_none() || ds.style_rev != rev {
            ds.style_rev = rev;
            let reg = world.res::<ParamRegistry>();
            ds.style = Some(match world.try_res::<LookData>() {
                Some(look) => Style::from_look(&look, &reg),
                None => Style::default(),
            });
        }
        if ds.sounds.is_none() {
            ds.sounds = Some(Sounds::register(&mut world.res_mut::<AudioMixer>()));
        }
    }
    // No canvas yet (or headless without the render module): a 720p layout.
    let vp = world
        .try_res::<RenderWorld>()
        .map(|rw| rw.viewport_css)
        .filter(|v| v[0] > 1.0 && v[1] > 1.0)
        .unwrap_or([1280.0, 720.0]);
    let lim = CellLimits {
        min_px: tune.min_cell_px,
        max_px: tune.max_cell_px.max(tune.min_cell_px),
    };
    let fx = fx_flags(world);
    let (juice, shake_on) = (fx.juice, fx_on(world, "shake"));
    let reg = world.res::<ParamRegistry>();
    let pop_scale = param_f32(&reg, "fx.juice.pop_scale", 1.3);
    let shake_amount = param_f32(&reg, "fx.shake.amount", 0.35);
    let shake_freq = param_f32(&reg, "fx.shake.frequency", 18.0);
    drop(reg);
    let l = {
        let (Some(mut game), Some(grid)) = (world.try_res_mut::<Game>(), world.try_res::<Grid>()) else {
            return;
        };
        let l = layout(vp[0], vp[1], grid.board.w, grid.board.h, lim);
        game.layout = l;
        l
    };
    // A scripted capture moment asked for by live (`moments`): play it on
    // the game now, and keep the attract replay off while it is held.
    #[cfg(feature = "showcase")]
    {
        if world.res::<Time>().held.is_none() {
            world.res_mut::<DrawState>().staged = false;
        }
        let stage = world.res_mut::<DrawState>().stage.take();
        if let Some(m) = stage {
            world.res_mut::<DrawState>().autoplay = false;
            world.res_mut::<DrawState>().moment_t = None;
            if let Err(e) = moments::play(world, m) {
                world.res_mut::<ErrorLog>().error(format!("match-3: capture moment: {e}"));
            }
            world.res_mut::<DrawState>().staged = true;
        }
    }
    #[cfg(feature = "showcase")]
    let replay = {
        let held = world.res::<Time>().held;
        let title = world.res::<Game>().phase == Phase::Title && !world.res::<DrawState>().staged;
        match held.filter(|_| title) {
            Some(h) => showcase_step(world, h, &fx, l),
            None => false,
        }
    };
    // The animation clock (Puff, the background drift, breathing pieces,
    // shake). A staged moment draws at its own replay time, not at the held
    // render time, which is wherever the session's clock happened to be when
    // live staged it: the same moment is the same picture on every run.
    let t = world.res::<Time>().render_seconds();
    #[cfg(feature = "showcase")]
    let t = {
        let ds = world.res::<DrawState>();
        match ds.moment_t {
            Some(mt) if ds.staged => draw::moment_clock(mt),
            _ => t,
        }
    };
    let ds = world.res::<DrawState>();
    // The pieces' sprites: the look's smooth frames by name, found again each
    // frame (the sheet is added when the render module starts), or the
    // built-in shapes for any frame the look doesn't supply.
    let style = match world.try_res::<evani_render::SpriteArt>() {
        Some(art) => ds.style.unwrap_or_default().with_art(&art),
        None => ds.style.unwrap_or_default(),
    };
    // The game to show: the capture replay, or the game itself.
    #[cfg(feature = "showcase")]
    let src: &World = match ds.showcase.as_ref() {
        Some(s) if replay => &s.sim.world,
        _ => world,
    };
    #[cfg(not(feature = "showcase"))]
    let src: &World = world;
    let trauma = src.try_res::<Game>().map_or(0.0, |g| g.trauma);
    let shake = if shake_on { draw::shake_offset(trauma, t as f32, shake_amount * tune.shake_px_per_unit, shake_freq) } else { [0.0; 2] };
    let frame = Frame {
        layout: &l,
        style: &style,
        fx: DrawFx {
            juice,
            pop_scale,
            shake,
        },
        t: t as f32,
        tune: &tune,
    };
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        draw::draw_game(&mut rw, src, &frame);
    }
    world.res_mut::<Match3Hud>().update(src, &style, shake, tune.result_delay_s);
    // Sounds for this frame's events (the game's own, never the replay's).
    let Some(mut game) = world.try_res_mut::<Game>() else {
        return;
    };
    if game.events.is_empty() {
        return;
    }
    if let Some(sounds) = ds.sounds.as_ref().filter(|s| !s.is_empty()) {
        let mut mixer = world.res_mut::<AudioMixer>();
        // At most one of each sound per frame (a cascade lands many pieces).
        let mut played: Vec<Sfx> = Vec::with_capacity(8);
        for &e in &game.events {
            if let Some((s, gain)) = sound_of(e)
                && !played.contains(&s)
            {
                played.push(s);
                sounds.play(&mut mixer, s, gain);
            }
        }
    } else {
        world.res_mut::<ErrorLog>().note("match-3: no sounds registered".to_string());
    }
    game.events.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::input::{PointerKind, RawEvent};
    use evani_core::params::ParamValue;

    fn app() -> App {
        App::new("t").with_kit(Match3Plugin)
    }

    #[test]
    fn registers_tunables_actions_and_data() {
        let mut a = app();
        a.run_headless(3, 1000.0 / 60.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "kit.match_3.swap_s",
            "kit.match_3.swipe_threshold",
            "kit.match_3.fall_gravity",
            "kit.match_3.bomb_radius",
            "kit.match_3.trauma_prism",
        ] {
            let id = reg.id(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(reg.def(id).unwrap().owner, "kit.match-3", "{n}");
        }
        drop(reg);
        let f = a.world.res::<Formulas>();
        let s = f.id("kit.match_3.match_score").unwrap();
        assert_eq!(f.eval(s, &[3.0, 0.0, 0.0]), 180.0);
        assert_eq!(f.eval(s, &[4.0, 2.0, 1.0]), 4.0 * 60.0 * 2.0 + 120.0);
        drop(f);
        let input = a.world.res::<Input>();
        for n in ["up", "down", "left", "right", "pick", "cancel", "pause"] {
            assert!(input.action_id(n).is_some(), "{n}");
        }
        drop(input);
        assert!(a.world.res::<evani_core::plugin::DataFiles>().get("match_3.levels").is_some());
        assert_eq!(a.world.res::<Game>().phase, Phase::Title);
    }

    #[test]
    fn tune_defaults_match_the_registry() {
        let mut a = app();
        a.start();
        let k = *a.world.res::<Match3Kit>();
        let reg = a.world.res::<ParamRegistry>();
        let mut want = Tune::default();
        if let Some((_, json)) = BAKED.iter().find(|(n, _)| *n == "params.json") {
            let baked: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json).unwrap();
            for (name, v) in baked {
                if let Some(v) = v.as_f64() {
                    want.set_by_name(&name, v as f32);
                }
            }
        }
        assert_eq!(k.tune.read(&reg), want);
    }

    #[test]
    fn formula_matches_the_game_default() {
        let mut a = app();
        a.start();
        let f = a.world.res::<Formulas>();
        let s = f.id("kit.match_3.match_score").unwrap();
        for (p, c, sp) in [(3.0, 0.0, 0.0), (5.0, 1.0, 1.0), (7.0, 4.0, 0.0)] {
            assert_eq!(f.eval(s, &[p, c, sp]), game::default_match_score(p, c, sp));
        }
    }

    #[test]
    fn a_touch_swipe_through_the_engine_makes_a_move() {
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(Match3Plugin);
        a.start();
        a.world.res_mut::<RenderWorld>().viewport_css = [390.0, 844.0];
        a.world.res_mut::<Game>().request(Request::Start(0));
        a.run_headless(60 * 3, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Game>().phase, Phase::Play);
        let best = a.world.res::<Match3Hud>().best.expect("a move");
        let moves = a.world.res::<Game>().moves_left;
        let ev = |x: f32, y: f32, down: bool| RawEvent::Pointer {
            id: 1,
            x,
            y,
            down,
            primary: true,
            kind: PointerKind::Touch,
        };
        a.world.res_mut::<Input>().handle(&ev(best[0], best[1], true));
        a.run_headless(2, 1000.0 / 60.0);
        a.world.res_mut::<Input>().handle(&ev(best[2], best[3], true));
        a.run_headless(2, 1000.0 / 60.0);
        a.world.res_mut::<Input>().handle(&ev(best[2], best[3], false));
        a.world.res_mut::<Input>().handle(&RawEvent::PointerGone { id: 1 });
        a.run_headless(2, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Game>().moves_left, moves - 1);
        // The frame drew a board and Puff.
        let stats = a.world.res::<evani_core::overlay::RenderStats>();
        assert!(stats.sprites > 300, "{}", stats.sprites);
    }

    #[test]
    fn runs_with_the_basic_look() {
        // Done-when 4: no cartoon2d look needed; flat default style.
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(Match3Plugin)
            .with_look(evani_look_basic::BasicLook);
        a.start();
        a.world.res_mut::<RenderWorld>().viewport_css = [1280.0, 720.0];
        a.world.res_mut::<Game>().request(Request::Start(1));
        a.run_headless(60, 1000.0 / 60.0);
        let stats = a.world.res::<evani_core::overlay::RenderStats>();
        assert!(stats.sprites > 200, "{}", stats.sprites);
        drop(stats);
        let ds = a.world.res::<DrawState>();
        let st = ds.style.unwrap();
        assert_eq!(st.outline_px, 0.0, "basic has no outline style");
        assert!(!st.tile_from_look);
        assert_eq!(a.world.res::<Match3Hud>().level_name, "Frosting Falls");
    }

    #[test]
    fn param_changes_reach_the_sim() {
        let mut a = app();
        a.start();
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("kit.match_3.intro_s", ParamValue::F32(5.0))
            .unwrap();
        a.world.res_mut::<Game>().request(Request::Start(0));
        a.run_headless(60 * 3, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Game>().phase, Phase::Intro, "a 5 s intro is still showing at 3 s");
    }

    #[test]
    fn pause_action_pauses_play_only() {
        let mut a = app();
        a.start();
        let esc = |down| RawEvent::Key {
            code: evani_core::input::KeyCode::new("Escape").unwrap(),
            down,
        };
        a.world.res_mut::<Input>().handle(&esc(true));
        a.run_headless(2, 1000.0 / 60.0);
        assert!(!a.world.res::<Game>().paused, "no pause on the title");
        a.world.res_mut::<Input>().handle(&esc(false));
        a.world.res_mut::<Game>().request(Request::Start(0));
        a.run_headless(10, 1000.0 / 60.0);
        a.world.res_mut::<Input>().handle(&esc(true));
        a.run_headless(2, 1000.0 / 60.0);
        assert!(a.world.res::<Game>().paused);
        assert!(a.world.res::<Match3Hud>().paused);
    }

    /// Run until the board waits for a move in play (at most `max` frames).
    fn until_idle(a: &mut App, max: u32) -> bool {
        for _ in 0..max {
            a.run_headless(1, 1000.0 / 60.0);
            let g = a.world.res::<Game>();
            if g.phase == Phase::Play && g.step == game::Step::Idle {
                return true;
            }
        }
        false
    }

    /// Free roam (docs/PLAYGROUNDS.md): two minutes of keyboard play (the
    /// cursor, picks and swaps), then a bot playing more moves than the
    /// level allows, with no game over, Sugar Rush, win or loss, and the
    /// board still full.
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{PlayOutcomes, ScriptKey, free_roam_test};
        let mut a = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(Match3Plugin);
        a.start();
        assert!(a.world.res::<Game>().free_roam);
        a.world.res_mut::<Game>().request(Request::Start(0));
        let mut script = Vec::new();
        let keys = ["ArrowRight", "ArrowDown", "Space", "ArrowLeft", "ArrowUp", "Space", "ArrowDown", "Enter"];
        for i in 0..120 {
            let at = 3.0 + f64::from(i) * 0.9;
            let key = keys[i as usize % keys.len()];
            script.push(ScriptKey::down(at, key));
            script.push(ScriptKey::up(at + 0.2, key));
        }
        let r = free_roam_test(&mut a, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(a.world.res::<Game>().moves_used > 0, "the keys made moves");
        // A bot plays the weakest move, far past the level's move limit.
        let total = a.world.res::<Game>().current().moves;
        let used = a.world.res::<Game>().moves_used;
        for _ in 0..total + 5 {
            assert!(until_idle(&mut a, 60 * 30), "the board settles");
            let m = a.world.res::<Grid>().board.worst_move().expect("a move");
            a.world.res_mut::<Game>().request(Request::Swap(m.a, m.b));
        }
        assert!(until_idle(&mut a, 60 * 30));
        let g = a.world.res::<Game>();
        assert_eq!(g.phase, Phase::Play, "no win, loss or Sugar Rush");
        assert_eq!(g.moves_used, used + total + 5);
        assert_eq!(g.moves_left, total, "moves cost nothing");
        assert_eq!(*a.world.res::<PlayOutcomes>(), PlayOutcomes::default());
        let hud = a.world.res::<Match3Hud>();
        assert!(hud.free_roam && hud.goals.is_empty());
        let grid = a.world.res::<Grid>();
        let n = grid.board.cells.iter().flatten().count();
        assert!(n > 0 && (0..grid.board.len()).all(|i| !grid.board.mask[i] || grid.board.cells[i].is_some()));
        assert_eq!(a.world.count::<ecs::Cell>(), n, "every piece is an entity");
    }

    /// Play mode keeps its move limit, and a lost level is a game over.
    #[test]
    fn play_mode_loss_is_a_game_over() {
        use evani_core::play::PlayOutcomes;
        let mut a = app();
        a.start();
        assert!(!a.world.res::<Game>().free_roam);
        a.world.res_mut::<Game>().request(Request::Start(0));
        for _ in 0..200 {
            if !until_idle(&mut a, 60 * 30) {
                break;
            }
            let m = a.world.res::<Grid>().board.worst_move().expect("a move");
            a.world.res_mut::<Game>().request(Request::Swap(m.a, m.b));
        }
        let g = a.world.res::<Game>();
        assert_eq!(g.phase, Phase::Lost);
        assert_eq!(g.moves_left, 0);
        assert_eq!(a.world.res::<PlayOutcomes>().game_overs, 1);
    }
}

#[cfg(all(test, feature = "showcase"))]
mod showcase_tests {
    use super::*;
    use evani_core::App;

    #[test]
    fn held_captures_replay_the_attract_board_to_a_popping_match() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Match3Plugin);
        a.start();
        a.world.res_mut::<RenderWorld>().viewport_css = [1280.0, 720.0];
        for held in [0.0, 20.0, 40.0] {
            a.world.res_mut::<Time>().held = Some(held);
            draw_system(&mut a.world);
            let ds = a.world.res::<DrawState>();
            let s = ds.showcase.as_ref().unwrap();
            assert!((s.at - (held + SHOWCASE_LEAD_S)).abs() < 0.02, "{}", s.at);
            assert!(s.sim.game().phase == Phase::Title);
            assert!(s.sim.world.entity_count() > 60, "the replay's pieces are entities");
            drop(ds);
            // The drawn frame has popping pieces (more sprites than a still board).
            assert!(a.world.res::<Match3Hud>().score > 0 || held == 0.0);
        }
        // Same held time, same frame.
        a.world.res_mut::<Time>().held = Some(20.0);
        a.world.res_mut::<RenderWorld>().sprites.clear();
        draw_system(&mut a.world);
        let one = a.world.res::<RenderWorld>().sprites.clone();
        a.world.res_mut::<RenderWorld>().sprites.clear();
        draw_system(&mut a.world);
        assert_eq!(one, a.world.res::<RenderWorld>().sprites);
        // Straight to 20 s in a fresh game: the same frame as after 0 and 40.
        let mut b = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Match3Plugin);
        b.start();
        b.world.res_mut::<RenderWorld>().viewport_css = [1280.0, 720.0];
        b.world.res_mut::<Time>().held = Some(20.0);
        b.world.res_mut::<RenderWorld>().sprites.clear();
        draw_system(&mut b.world);
        assert_eq!(one, b.world.res::<RenderWorld>().sprites);
    }
}
