//! `evani-kit-2d-platformer`: a side-on 2D platformer. A kinematic hero with
//! coyote time, jump buffering and variable jump height; tile collision with
//! slopes, one-way platforms and step-up; moving platforms; patrolling
//! beetles and jumping hoppers to stomp; coins, checkpoints, lives, score
//! and a timer; a tile-map level loaded from JSON; indexed pixel art built
//! into a sprite sheet at start (recoloured by the look's palettes); a 2D
//! camera with parallax and whole-pixel shake; a pixel HUD; synthesised
//! sounds. Sprites only: no meshes and no shaders of its own.
//!
//! * Params (owner `kit.2d-platformer`): every tunable is
//!   `kit.platformer.*` (a param segment must start with a letter), see
//!   [`Tune`], [`Ints`] and SPEC.md section 1.
//! * Input actions: `move`, `jump`, `run`, `down`, `pause`. On phones and
//!   tablets the [`touch`] layer sets the same actions (README "Touch controls").
//! * Data files: `platformer.level`, `platformer.art`.
//! * Resources: [`Platformer`] (the game and the attract demo; the app
//!   sends [`game::Request`]s), [`hud::PlatformerHud`] (what menus show),
//!   [`evani_render::SpriteSheet`] (the art).
//! * Systems: `platformer.sim` (fixed `Update`), `platformer.draw`
//!   (`Frame`: camera, parallax, tiles, sprites, shadows, HUD, shake,
//!   sounds).

pub mod art;
pub mod attract;
pub mod body;
pub mod bot;
#[cfg(test)]
mod checks;
#[cfg(test)]
mod review;
pub mod draw;
pub mod ecs;
pub mod font;
pub mod game;
pub mod hud;
pub mod level;
pub mod moments;
pub mod sfx;
pub mod touch;

use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::play::{PlayMode, PlayOutcomes};
use evani_core::params::{F32Param, I32Param, ParamDef, ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::{KitPlugin, PluginContext, PluginError};
use evani_core::time::Time;

use crate::game::{Controls, Game, Request};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// The level (`Bramble Hills`).
pub const LEVEL_JSON: &str = include_str!("../data/level.json");
/// Indexed sprites and default colour ramps.
pub const ART_JSON: &str = include_str!("../data/art.json");

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
                $(if name == concat!("kit.platformer.", stringify!($field)) {
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
                        ParamDef::f32(concat!("kit.platformer.", stringify!($field)), $def)
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
    walk_speed = 90.0, [10.0, 400.0], "px/s", "Top walking speed.";
    run_speed = 150.0, [10.0, 500.0], "px/s", "Top speed with run held.";
    ground_accel = 700.0, [10.0, 5000.0], "px/s2", "Acceleration on the ground.";
    air_accel = 450.0, [10.0, 5000.0], "px/s2", "Acceleration in the air.";
    ground_friction = 900.0, [0.0, 5000.0], "px/s2", "Slow-down on the ground with no input.";
    air_friction = 120.0, [0.0, 5000.0], "px/s2", "Slow-down in the air with no input.";
    turn_accel = 1400.0, [10.0, 8000.0], "px/s2", "Acceleration when turning against the motion.";
    gravity = 980.0, [50.0, 4000.0], "px/s2", "Gravity when the physics module has none (scaled by physics.gravity / 9.81 when it does).";
    gravity_scale = 1.0, [0.1, 5.0], "x", "Gravity multiplier.";
    fall_gravity_mult = 1.6, [1.0, 4.0], "x", "Extra gravity on the way down.";
    max_fall = 330.0, [50.0, 1500.0], "px/s", "Fastest fall.";
    jump_speed = 330.0, [50.0, 1200.0], "px/s", "Jump take-off speed.";
    run_jump_bonus = 25.0, [0.0, 300.0], "px/s", "Extra take-off speed from a running jump.";
    jump_cut = 0.45, [0.0, 1.0], "x", "Rising speed kept when jump is released early (variable height).";
    coyote_s = 0.09, [0.0, 0.5], "s", "A jump still works this long after walking off a ledge.";
    buffer_s = 0.12, [0.0, 0.5], "s", "A jump pressed this long before landing fires on landing.";
    stomp_bounce = 260.0, [0.0, 1200.0], "px/s", "Bounce off a stomped enemy (jump held: jump_speed).";
    invuln_s = 1.5, [0.0, 10.0], "s", "Blinking safe time after a respawn.";
    hero_w = 10.0, [4.0, 16.0], "px", "Hero collision box width.";
    hero_h = 14.0, [4.0, 30.0], "px", "Hero collision box height.";
    slope_snap = 6.0, [0.0, 16.0], "px", "How far a grounded body snaps down to stay on a slope.";
    drop_through_s = 0.2, [0.0, 1.0], "s", "Down + jump ignores one-way platforms this long.";
    platform_speed = 40.0, [1.0, 400.0], "px/s", "Moving platform speed when the level data gives none.";
    beetle_speed = 28.0, [0.0, 200.0], "px/s", "Bramble beetle walking speed.";
    hopper_period_s = 1.6, [0.2, 10.0], "s", "Puddle hopper time between hops.";
    hopper_jump_speed = 260.0, [0.0, 1000.0], "px/s", "Puddle hopper jump speed.";
    hopper_hop_speed = 50.0, [0.0, 400.0], "px/s", "Puddle hopper sideways hop speed.";
    hopper_sight = 96.0, [0.0, 600.0], "px", "Puddle hoppers hop towards a hero this close.";
    stomp_window = 6.0, [0.0, 16.0], "px", "Feet within this of an enemy's top count as a stomp.";
    wake_distance = 336.0, [16.0, 2000.0], "px", "Enemies sleep until this close to the camera (more than half view_width_max_px, so they wake before they come into view).";
    time_limit_s = 0.0, [0.0, 999.0], "s", "Level time limit; 0 uses the level data.";
    cam_deadzone_x = 24.0, [0.0, 200.0], "px", "Camera dead zone half width.";
    cam_deadzone_y = 72.0, [0.0, 200.0], "px", "Vertical dead zone: the hero can rise this far above the ground it last stood on before the camera follows (a normal jump stays inside).";
    cam_ground_frac = 0.72, [0.4, 0.95], "", "Where the ground the hero last stood on sits, as a fraction of the view height from the top (0.72: lower third, sky above).";
    cam_lookahead = 32.0, [0.0, 200.0], "px", "Camera look-ahead in the direction of movement.";
    cam_smooth = 4.0, [0.1, 60.0], "1/s", "Camera look-ahead easing.";
    cam_smooth_y = 5.0, [0.1, 60.0], "1/s", "Camera easing to a new ground line (landing higher or lower).";
    view_height_px = 224.0, [120.0, 480.0], "px", "Fewest art pixels of height the screen shows (a smaller screen may show less); smooth looks show exactly this.";
    view_height_max_px = 288.0, [120.0, 720.0], "px", "Most art pixels of height: the whole-pixel scale is the smallest that keeps the view at or under this (and at or over view_height_px).";
    view_width_max_px = 640.0, [256.0, 2000.0], "px", "Widest view in art pixels; a wider screen frames it with bars at the sides (keep it under 2 x wake_distance).";
    cycle_fps = 6.0, [0.0, 30.0], "1/s", "Palette cycling speed for water and torch fire.";
    stomp_trauma = 0.25, [0.0, 1.0], "", "Shake trauma from a stomp.";
    hurt_trauma = 0.5, [0.0, 1.0], "", "Shake trauma from losing a life.";
    goal_trauma = 0.3, [0.0, 1.0], "", "Shake trauma on reaching the castle.";
    shake_decay = 1.6, [0.1, 10.0], "1/s", "Trauma lost per second.";
    shadow_alpha = 0.45, [0.0, 1.0], "", "Drop shadow strength under the hero and enemies.";
}

/// Integer settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ints {
    pub lives: i32,
    pub coins_per_life: i32,
    pub score_coin: i32,
    pub score_stomp: i32,
    pub score_time: i32,
}

impl Default for Ints {
    fn default() -> Self {
        Self {
            lives: 3,
            coins_per_life: 50,
            score_coin: 10,
            score_stomp: 100,
            score_time: 5,
        }
    }
}

/// Handles the kit's systems read every tick (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct PlatformerKit {
    pub tune: Handles,
    pub lives: I32Param,
    pub coins_per_life: I32Param,
    pub score_coin: I32Param,
    pub score_stomp: I32Param,
    pub score_time: I32Param,
    pub actions: Actions,
    /// `physics.gravity` when the physics module is in the build.
    pub physics_gravity: Option<ParamId>,
}

#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub move_x: ActionId,
    pub jump: ActionId,
    pub run: ActionId,
    pub down: ActionId,
    pub pause: ActionId,
}

impl PlatformerKit {
    pub fn ints(&self, reg: &ParamRegistry) -> Ints {
        Ints {
            lives: reg.i32(self.lives),
            coins_per_life: reg.i32(self.coins_per_life),
            score_coin: reg.i32(self.score_coin),
            score_stomp: reg.i32(self.score_stomp),
            score_time: reg.i32(self.score_time),
        }
    }

    /// Gravity in px/s²: the kit's own, scaled by `physics.gravity / 9.81`
    /// when the physics module registered it, times `gravity_scale`.
    pub fn gravity(&self, reg: &ParamRegistry, t: &Tune) -> f32 {
        let earth = match self.physics_gravity.and_then(|id| reg.value(id)) {
            Some(ParamValue::Vec3(v)) if v[1].abs() > 0.01 => v[1].abs() / 9.81,
            _ => 1.0,
        };
        t.gravity * earth * t.gravity_scale
    }
}

/// The kit's world resource: the game the player plays and the attract
/// demo the title screen shows.
pub struct Platformer {
    pub game: Game,
    pub attract: attract::Attract,
    /// A staged capture moment (live builds only): what the player's game
    /// shows until the player acts.
    #[cfg(feature = "showcase")]
    pub show: Option<Show>,
}

/// What a staged capture moment does with the player's game (the
/// `showcase` feature, which only the app's `live` turns on: a shipped game
/// has none of it).
#[cfg(feature = "showcase")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    /// The staged frame stays as it is: the sim does not step and the
    /// draw's animation clock is the game's own tick, so every capture of
    /// the moment (card, showcase, phone, any tier) is the same frame.
    Still,
    /// The run video: each step plays the attract recording's next input,
    /// from recording tick `at`, on the player's game, until the recording
    /// ends.
    Run { at: usize },
}

/// One showcase step: true when it stepped (or held) the player's game in
/// place of the normal update. Any player input ends the show.
#[cfg(feature = "showcase")]
fn showcase_step(p: &mut Platformer, acted: bool) -> bool {
    if acted {
        p.show = None;
    }
    match p.show {
        None => false,
        Some(Show::Still) => true,
        Some(Show::Run { at }) if at < p.attract.len() => {
            p.attract.replay(&mut p.game, at, 1);
            p.show = Some(Show::Run { at: at + 1 });
            true
        }
        Some(Show::Run { .. }) => {
            p.show = None;
            false
        }
    }
}

/// The 2D platformer kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlatformerPlugin;

fn int(kit: &mut PluginContext, name: &str, def: i32, max: f32, desc: &str) -> Result<I32Param, PluginError> {
    Ok(kit.param_i32(ParamDef::i32(name, def).range(0.0, max).describe(desc))?)
}

fn button(kit: &mut PluginContext, name: &str, binds: &[&str], desc: &str) -> Result<ActionId, PluginError> {
    Ok(kit.action(ActionDef::button(name, binds).map_err(|e| e.to_string())?.describe(desc)))
}

impl KitPlugin for PlatformerPlugin {
    fn name(&self) -> &'static str {
        "2d-platformer"
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let tune = Handles::register(kit)?;
        let d = Ints::default();
        let lives = int(kit, "kit.platformer.lives", d.lives, 99.0, "Lives at the start of a run.")?;
        let coins_per_life = int(kit, "kit.platformer.coins_per_life", d.coins_per_life, 999.0, "Coins for an extra life.")?;
        let score_coin = int(kit, "kit.platformer.score_coin", d.score_coin, 10_000.0, "Points per coin.")?;
        let score_stomp = int(kit, "kit.platformer.score_stomp", d.score_stomp, 10_000.0, "Points per stomped enemy.")?;
        let score_time = int(kit, "kit.platformer.score_time", d.score_time, 1000.0, "Points per second left at the goal.")?;

        let move_x = kit.action(
            ActionDef::axis("move", &["keys:KeyA/KeyD", "keys:ArrowLeft/ArrowRight", "pad:axis:0"])
                .map_err(|e| e.to_string())?
                .describe("Walk left and right."),
        );
        let jump = button(
            kit,
            "jump",
            &["key:Space", "key:KeyW", "key:ArrowUp", "key:KeyK", "pad:button:0"],
            "Jump (hold for higher; with down on a plank: drop through).",
        )?;
        let run = button(kit, "run", &["key:ShiftLeft", "key:KeyJ", "pad:button:2"], "Hold to run.")?;
        let down = button(kit, "down", &["key:KeyS", "key:ArrowDown", "pad:button:13"], "Down (with jump: drop through a plank).")?;
        let pause = button(kit, "pause", &["key:Escape", "key:KeyP", "pad:button:9"], "Pause the game.")?;

        // On-screen controls for phones and tablets: they only set the
        // actions above (see the `touch` module); up while a run is played.
        kit.touch_controls(touch::layout())?;

        kit.data_file("platformer.level", LEVEL_JSON);
        kit.data_file("platformer.art", ART_JSON);
        // Values tuned live and written by bake.export.
        kit.baked_files(BAKED);

        let level = Rc::new(level::Level::from_json(LEVEL_JSON)?);
        // Fail the build early on bad art data.
        art::ArtData::from_json(ART_JSON)?;
        let physics_gravity = kit.world.res::<ParamRegistry>().id("physics.gravity");
        let handles = PlatformerKit {
            tune,
            lives,
            coins_per_life,
            score_coin,
            score_stomp,
            score_time,
            actions: Actions { move_x, jump, run, down, pause },
            physics_gravity,
        };
        let (t, n, g) = {
            let reg = kit.world.res::<ParamRegistry>();
            let t = tune.read(&reg);
            (t, handles.ints(&reg), handles.gravity(&reg, &t))
        };
        let mut game = Game::new(Rc::clone(&level), &t, &n);
        // Free roam (docs/PLAYGROUNDS.md): straight into the level, no
        // lives, timer or goal. The attract demo stays a Play-mode demo.
        let free_roam = kit.world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
        if free_roam {
            game.set_free_roam(true, &t, &n);
        }
        let attract = attract::Attract::new(level, &t, &n, g);
        kit.resource(Platformer {
            game,
            attract,
            #[cfg(feature = "showcase")]
            show: None,
        });
        // The player's game objects live in the ECS (module `ecs`).
        ecs::register(kit.world);
        kit.resource(ecs::Objects::default());
        kit.resource(hud::PlatformerHud::default());
        kit.resource(draw::DrawState::default());
        kit.resource(evani_render::SpriteSheet::default());
        kit.resource(handles);
        kit.system(SystemDesc::new("platformer.sim", Stage::Update, sim_system));
        kit.system(SystemDesc::new("platformer.draw", Stage::Frame, draw::draw_system));
        Ok(())
    }

    /// Free roam (docs/PLAYGROUNDS.md): no lives, deaths, timer or level
    /// goal under `PlayMode::FreeRoam`; see [`game::Game::free_roam`].
    fn free_roam(&self) -> bool {
        true
    }
}

/// The fixed-step game update.
pub fn sim_system(world: &mut World) {
    let Some(k) = world.try_res::<PlatformerKit>().map(|k| *k) else {
        return;
    };
    let dt = world.res::<Time>().fixed_dt;
    let (t, n, g) = {
        let reg = world.res::<ParamRegistry>();
        let t = k.tune.read(&reg);
        (t, k.ints(&reg), k.gravity(&reg, &t))
    };
    let (c, pause) = {
        let input = world.res::<Input>();
        let a = &k.actions;
        (
            Controls {
                x: input.value(a.move_x).clamp(-1.0, 1.0),
                jump: input.down(a.jump),
                jump_pressed: input.just_pressed(a.jump),
                run: input.down(a.run),
                down: input.down(a.down),
            },
            input.just_pressed(a.pause),
        )
    };
    // The player's game objects live in the ECS: the step starts from their
    // entities (unless the game was replaced as a whole since the last step)
    // and writes every change back to them (`ecs`). Both resources are taken
    // out for the step so the sync can spawn and despawn.
    let Some(mut p) = world.remove_resource::<Platformer>() else {
        return;
    };
    let mut objects = world.remove_resource::<ecs::Objects>().unwrap_or_default();
    objects.gather(world, &mut p.game);
    // A staged capture moment holds or plays the game until the player acts.
    #[cfg(feature = "showcase")]
    let shown = showcase_step(&mut p, pause || c.jump || c.jump_pressed || c.run || c.down || c.x != 0.0);
    #[cfg(not(feature = "showcase"))]
    let shown = false;
    if !shown {
        if pause {
            p.game.request(Request::TogglePause);
        }
        p.game.tick(c, &t, &n, g, dt);
    }
    let fails = p.game.take_fails();
    objects.sync(world, &mut p.game);
    // The touch controls belong to a run in progress: off on the title, the
    // pause and settings menus and the results, which are tapped on the page.
    let playing = touch::controls_on(p.game.phase);
    world.insert_resource(p);
    world.insert_resource(objects);
    evani_core::touch::set_enabled(world, playing);
    // Every game over, expired timer and lost life goes to PlayOutcomes (the
    // free-roam test fails on any; free roam itself has none).
    if fails.any()
        && let Some(mut o) = world.try_res_mut::<PlayOutcomes>()
    {
        for _ in 0..fails.game_overs {
            o.game_over();
        }
        for _ in 0..fails.timers_expired {
            o.timer_expired();
        }
        for _ in 0..fails.lives_lost {
            o.life_lost();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Phase;
    use evani_core::App;

    fn app() -> App {
        App::new("t").with_kit(PlatformerPlugin)
    }

    #[test]
    fn registers_tunables_actions_and_data() {
        let mut a = app();
        a.run_headless(3, 1000.0 / 60.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "kit.platformer.jump_speed",
            "kit.platformer.coyote_s",
            "kit.platformer.buffer_s",
            "kit.platformer.lives",
            "kit.platformer.cam_lookahead",
        ] {
            let id = reg.id(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(reg.def(id).unwrap().owner, "kit.2d-platformer", "{n}");
        }
        drop(reg);
        let input = a.world.res::<Input>();
        for n in ["move", "jump", "run", "down", "pause"] {
            assert!(input.action_id(n).is_some(), "{n}");
        }
        drop(input);
        let data = a.world.res::<evani_core::plugin::DataFiles>();
        assert!(data.get("platformer.level").is_some());
        assert!(data.get("platformer.art").is_some());
        drop(data);
        assert_eq!(a.world.res::<Platformer>().game.phase, Phase::Title);
    }

    fn free_app() -> App {
        App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(PlatformerPlugin)
    }

    /// Free roam (docs/PLAYGROUNDS.md): the kit says so, starts straight in
    /// the level, and a Play-mode app still starts at the title.
    #[test]
    fn free_roam_starts_in_the_level() {
        assert!(PlatformerPlugin.free_roam());
        let a = free_app();
        assert_eq!(a.play_mode(), PlayMode::FreeRoam);
        let p = a.world.res::<Platformer>();
        assert!(p.game.free_roam && p.game.phase == Phase::Playing);
        // The attract demo behind the title stays a Play-mode demo.
        assert!(!p.attract.game.free_roam);
        drop(p);
        let play = app();
        assert!(!play.world.res::<Platformer>().game.free_roam);
        assert_eq!(play.play_mode(), PlayMode::Play);
    }

    /// Two minutes of scripted input in free roam: run, jump, run into the
    /// first beetle, run into the 10-tile pit (again and again), walk back,
    /// all with no game over, expired timer, lost life or error.
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{free_roam_test, ScriptKey};
        let mut app = free_app();
        let mut script = Vec::new();
        // Cycles of 20 s: walk right into the beetle, jump now and then,
        // run on to the pit and fall in; later walk back left and run again.
        for c in 0..6 {
            let t0 = f64::from(c) * 20.0;
            script.push(ScriptKey::down(t0 + 0.2, "KeyD"));
            for j in 0..4 {
                let at = t0 + 1.5 + f64::from(j) * 1.7;
                script.push(ScriptKey::down(at, "Space"));
                script.push(ScriptKey::up(at + 0.4, "Space"));
            }
            script.push(ScriptKey::down(t0 + 8.0, "ShiftLeft"));
            script.push(ScriptKey::up(t0 + 12.0, "ShiftLeft"));
            script.push(ScriptKey::up(t0 + 12.0, "KeyD"));
            script.push(ScriptKey::down(t0 + 12.5, "KeyA"));
            script.push(ScriptKey::up(t0 + 16.5, "KeyA"));
        }
        let r = free_roam_test(&mut app, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(r.sim_seconds > 119.0, "{r:?}");
        let p = app.world.res::<Platformer>();
        let g = &p.game;
        assert!(g.free_respawns >= 1, "the script never fell into the pit");
        assert_eq!(g.lives, Ints::default().lives, "no life lost");
        assert_eq!(g.phase, Phase::Playing);
        assert!(g.hero.alive());
        assert!((0.0..g.level.width_px()).contains(&g.hero.body.pos[0]), "{:?}", g.hero.body.pos);
        assert!(g.hero.body.pos[1] < g.level.height_px(), "{:?}", g.hero.body.pos);
        assert!(g.tick > 6000, "the game ran {} ticks", g.tick);
    }

    /// In Play mode the same moments are reported, which is what lets the
    /// free-roam test catch a regression.
    #[test]
    fn play_mode_reports_outcomes() {
        use evani_core::play::PlayOutcomes;
        let mut a = app();
        a.start();
        {
            let mut p = a.world.res_mut::<Platformer>();
            p.game.request(Request::Start);
        }
        a.run_headless(2, 1000.0 / 60.0);
        {
            let mut p = a.world.res_mut::<Platformer>();
            p.game.lives = 1;
            p.game.time_left = 0.05;
        }
        a.run_headless(180, 1000.0 / 60.0);
        let o = *a.world.res::<PlayOutcomes>();
        assert_eq!((o.game_overs, o.timers_expired, o.lives_lost), (1, 1, 1), "{o:?}");
        assert_eq!(a.world.res::<Platformer>().game.phase, Phase::Lost);
    }

    #[test]
    fn tune_defaults_match_the_registry() {
        let mut a = app();
        a.start();
        let k = *a.world.res::<PlatformerKit>();
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
    fn param_changes_reach_the_sim() {
        let mut a = app();
        a.start();
        a.world.res_mut::<Platformer>().game.request(Request::Start);
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("kit.platformer.walk_speed", ParamValue::F32(20.0))
            .unwrap();
        a.world.res_mut::<Input>().handle(&evani_core::input::RawEvent::Key {
            code: evani_core::input::KeyCode::new("KeyD").unwrap(),
            down: true,
        });
        a.run_headless(60, 1000.0 / 60.0);
        let p = a.world.res::<Platformer>();
        let v = p.game.hero.body.vel[0];
        assert!(v > 0.0 && v <= 20.0 + 1e-3, "{v}");
    }
}
