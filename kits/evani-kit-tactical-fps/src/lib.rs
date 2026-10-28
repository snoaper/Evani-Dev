//! `evani-kit-tactical-fps`: Hexagon's match on Evani.Dev (kits/tactical-fps/SPEC.md).
//! Two teams of four, Blue and Orange, fight across a three-level hexagonal
//! fortress whose middle ring turns; concrete cover breaks under fire; a team
//! wins by holding the enemy core, by kills, or on time.
//!
//! * [`tune`]: every tunable, a param `kit.tactical_fps.<name>` with its default
//!   from `data/tuning.json`. [`arena`]: the fortress from `data/arena.json`
//!   and a seed. [`holes`], [`collapse`]: destructible concrete. [`ring`]: the
//!   turning middle level. [`body`]: movement. [`weapons`]: hitscan,
//!   projectiles and blasts. [`bot`]: the bots. [`match_rules`]: the match.
//!   [`draw`]: the drawing, the view model and the cameras; [`cut`]: the
//!   hole cut-out material.
//! * One fixed 60 Hz `Stage::Update` system steps the match
//!   ([`step_system`]); `Stage::Frame` systems fill the HUD text
//!   ([`MatchHud`]) and draw ([`draw::system`]).
//! * Actions (SPEC section 3): `move_x`, `move_y`, `look_x`, `look_y`, `aim_x`,
//!   `aim_y` (mouse deltas), `fire`, `jump`, `weapon`, `pause`, plus the
//!   touch layout on phones and tablets (a move stick and a look pad that
//!   drives the same `aim_x` / `aim_y` actions as the mouse). Mouse look goes
//!   through the engine's pointer lock ([`lock_system`]): wanted during the
//!   warm-up and the match, given back on the title, results and pause; the
//!   engine itself releases it on Esc, blur, a hidden tab, exit and errors,
//!   and the kit only asks again, so the player's next click takes it back.
//! * The look ([`draw`], [`cut`]): materials found by role name (never a
//!   named look), concrete holes cut through colour, depth and shadow by the
//!   kit material, and the held weapon drawn in the view-model pass.
//! * Free roam (docs/PLAYGROUNDS.md): no bots, score, timer or results.
//!   In Play mode the end of a match is reported in [`PlayOutcomes`].
//! * Deterministic: fixed step, the seeded [`Pcg32`], sim time only.

pub mod arena;
pub mod body;
pub mod bot;
pub mod collapse;
pub mod cut;
pub mod draw;
pub mod holes;
pub mod match_rules;
#[cfg(feature = "net")]
pub mod net;
pub mod ring;
pub mod tune;
pub mod weapons;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::params::ParamRegistry;
use evani_core::play::{PlayMode, PlayOutcomes};
use evani_core::plugin::{KitPlugin, PluginContext, PluginError};
use evani_core::rng::Pcg32;
use evani_core::scene::Scene;
use evani_core::time::Time;
use evani_core::pointer_lock::PointerLock;
use evani_core::touch::{Anchor, Button, LookPad, Stick, TouchLayout, Zone};

use crate::arena::{ARENA_JSON, Arena, ArenaDef};
use crate::match_rules::{Match, PlayerInput, WinReason};
use crate::tune::{Handles, TUNING_JSON, Tune};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Scene pushed at start.
pub const MAIN_SCENE: &str = "tactical_fps.main";

/// The seed of the title screen's arena (each match then draws its own
/// from the seeded [`Pcg32`]).
pub const TITLE_SEED: u64 = 1;

/// The kit's input actions.
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    /// Strafe (A/D, pad axis 0, the touch move stick).
    pub move_x: ActionId,
    /// Forward and back (W/S, pad axis 1; negative is forward, as a stick).
    pub move_y: ActionId,
    /// Turn (arrow keys, pad axis 2, the touch look stick).
    pub look_x: ActionId,
    /// Look up and down (arrow keys, pad axis 3; negative is up, as a stick).
    pub look_y: ActionId,
    /// Mouse turn (pixels this step).
    pub aim_x: ActionId,
    /// Mouse look up and down (pixels this step, down positive).
    pub aim_y: ActionId,
    /// Fire (held).
    pub fire: ActionId,
    /// Jump (held; also rises in a lift).
    pub jump: ActionId,
    /// Next weapon.
    pub weapon: ActionId,
    /// Pause.
    pub pause: ActionId,
}

/// Handles the kit's systems read every step (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct TacticalFps {
    /// Every tunable.
    pub handles: Handles,
    /// The input actions.
    pub actions: Actions,
    /// Free roam, read once at build.
    pub free_roam: bool,
}

/// Pause state (the `pause` action toggles it during a match).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Paused(pub bool);

/// What the HUD shows this frame (phase banner, timer, score, health,
/// weapon, ring status, core hold, kill feed); the demo app writes it into
/// its HUD element.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchHud {
    /// The HUD text, one line per item.
    pub text: String,
    /// Paused.
    pub paused: bool,
}

/// The tactical-fps kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct TacticalFpsKitPlugin;

fn action(kit: &mut PluginContext, def: Result<ActionDef, evani_core::input::BindingParseError>, what: &str) -> Result<ActionId, PluginError> {
    let def = def.map_err(|e| e.to_string())?;
    Ok(kit.action(def.describe(what)))
}

fn actions(kit: &mut PluginContext) -> Result<Actions, PluginError> {
    Ok(Actions {
        move_x: action(kit, ActionDef::axis("move_x", &["keys:KeyA/KeyD", "pad:axis:0"]), "Strafe left and right.")?,
        move_y: action(kit, ActionDef::axis("move_y", &["keys:KeyW/KeyS", "pad:axis:1"]), "Move forward and back.")?,
        look_x: action(kit, ActionDef::axis("look_x", &["keys:ArrowLeft/ArrowRight", "pad:axis:2"]), "Turn left and right.")?,
        look_y: action(kit, ActionDef::axis("look_y", &["keys:ArrowUp/ArrowDown", "pad:axis:3"]), "Look up and down.")?,
        aim_x: action(kit, ActionDef::axis("aim_x", &["mouse:dx"]), "Mouse turn.")?,
        aim_y: action(kit, ActionDef::axis("aim_y", &["mouse:dy"]), "Mouse look up and down.")?,
        fire: action(kit, ActionDef::button("fire", &["mouse:0", "key:KeyF", "pad:button:7"]), "Fire the weapon (on the title: start).")?,
        jump: action(kit, ActionDef::button("jump", &["key:Space", "pad:button:0"]), "Jump; hold in a lift shaft to rise (on the title: start).")?,
        weapon: action(kit, ActionDef::button("weapon", &["key:KeyQ", "pad:button:3"]), "Next weapon.")?,
        pause: action(kit, ActionDef::button("pause", &["key:Escape", "key:KeyP", "pad:button:9"]), "Pause the match.")?,
    })
}

/// Finger movement to mouse pixels on the look pad (touch wants more than a
/// mouse; `mouse_sens` then scales both alike).
pub const LOOK_PAD_SENSITIVITY: f32 = 2.0;

/// The touch layout (SPEC section 3): move stick on the left half, a look
/// pad (drag to look) on the right half, hold Fire, tap Jump and Weapon,
/// pause top right. Every control names an action the keys or the mouse
/// already drive: the look pad feeds `aim_x` / `aim_y`, the mouse deltas.
fn touch_layout() -> TouchLayout {
    TouchLayout::new()
        .stick(Stick::two_axis("move", "move_x", "move_y").dynamic(Zone::LEFT_HALF).at(Anchor::BottomLeft, 8.0, 16.0).label("Move"))
        .look_pad(LookPad::new("look", "aim_x", "aim_y").zone(Zone::RIGHT_HALF).sensitivity(LOOK_PAD_SENSITIVITY))
        .button(Button::hold("fire", "fire", "Fire").primary().at(Anchor::BottomRight, 4.0, 6.0))
        .button(Button::tap("jump", "jump", "Jump").at(Anchor::BottomRight, 6.0, 30.0))
        .button(Button::tap("weapon", "weapon", "Gun").at(Anchor::BottomRight, 26.0, 4.0))
        // Under the app's debug panel (top right, about 30 vmin tall on a
        // phone), which otherwise catches the tap.
        .pause_at("pause", Anchor::TopRight, 3.0, 32.0)
}

impl KitPlugin for TacticalFpsKitPlugin {
    fn name(&self) -> &'static str {
        "tactical-fps"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let handles = Handles::register(kit)?;
        let actions = actions(kit)?;
        kit.touch_controls(touch_layout())?;
        kit.pointer_lock();
        kit.data_file("tactical_fps.arena", ARENA_JSON);
        kit.data_file("tactical_fps.tuning", TUNING_JSON);
        // Values tuned live and written by bake.export (params.json,
        // curves/*.json, slots/*.expr in this crate).
        kit.baked_files(BAKED);
        // Fail at build, not mid-match, if the arena data is broken.
        ArenaDef::load()?;
        let free_roam = kit.world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
        kit.resource(TacticalFps { handles, actions, free_roam });
        kit.resource(Paused::default());
        kit.resource(MatchHud::default());
        kit.resource(draw::MomentCam::default());
        kit.resource(cut::CutState::new());
        #[cfg(not(feature = "net"))]
        kit.system(SystemDesc::new("tactical_fps.step", Stage::Update, step_system));
        // Net builds: the match runs through evani-net (src/net), and each
        // frame the local view of it is written before the HUD and drawing.
        #[cfg(feature = "net")]
        {
            kit.system(SystemDesc::new("tactical_fps.step", Stage::Update, net::step_system));
            kit.system(SystemDesc::new("tactical_fps.net_view", Stage::Frame, net::view_system).order(-1));
        }
        kit.system(SystemDesc::new("tactical_fps.hud", Stage::Frame, hud_system));
        kit.system(SystemDesc::new("tactical_fps.lock", Stage::Frame, lock_system));
        kit.system(draw::system());
        kit.scene(MAIN_SCENE, || Box::new(MainScene));
        Ok(())
    }

    fn start_scene(&self) -> Option<&'static str> {
        Some(MAIN_SCENE)
    }

    /// Free roam (docs/PLAYGROUNDS.md, SPEC 1.7): under `PlayMode::FreeRoam`
    /// the player is alone in the fortress: no bots, score, timer, win or
    /// results; a fall respawns at the base; the ring still turns and
    /// concrete still breaks.
    fn free_roam(&self) -> bool {
        true
    }
}

/// Every tunable's value now.
fn tune_now(world: &World, k: &TacticalFps) -> Tune {
    k.handles.read(&world.res::<ParamRegistry>())
}

/// A fresh match and its arena (the title, or free roam's open fortress).
fn reset(world: &mut World) -> Result<(), String> {
    let Some(k) = world.try_res::<TacticalFps>().map(|k| *k) else {
        return Ok(());
    };
    let t = tune_now(world, &k);
    let arena = Arena::new(ArenaDef::load()?, TITLE_SEED);
    let game = Match::new(k.free_roam, &arena, &t);
    world.insert_resource(arena);
    world.insert_resource(game);
    world.insert_resource(Paused::default());
    #[cfg(feature = "net")]
    net::session::start(world, k.free_roam, t)?;
    Ok(())
}

/// The player's input for one step, from the actions.
pub fn read_input(input: &Input, a: &Actions, t: &Tune, dt: f32) -> PlayerInput {
    let fire = input.down(a.fire);
    let jump = input.down(a.jump);
    // Sticks and keys turn at `look_speed`; mouse pixels at `mouse_sens`.
    // Both y axes are negative up (as a stick), so up is a negative value.
    let look = t.look_speed * dt;
    PlayerInput {
        strafe: input.value(a.move_x).clamp(-1.0, 1.0),
        forward: -input.value(a.move_y).clamp(-1.0, 1.0),
        dyaw: input.value(a.look_x).clamp(-1.0, 1.0) * look + input.value(a.aim_x) * t.mouse_sens,
        dpitch: -input.value(a.look_y).clamp(-1.0, 1.0) * look - input.value(a.aim_y) * t.mouse_sens,
        fire,
        jump,
        next_weapon: input.just_pressed(a.weapon),
        start: input.just_pressed(a.fire) || input.just_pressed(a.jump),
    }
}

/// One fixed step of the match: the player's input, the match step, and the
/// play outcomes (a match end, the player's death) in Play mode.
pub fn step_system(world: &mut World) {
    let Some(k) = world.try_res::<TacticalFps>().map(|k| *k) else {
        return;
    };
    let t = tune_now(world, &k);
    let dt = world.res::<Time>().fixed_dt;
    let (input, keys) = {
        let i = world.res::<Input>();
        (read_input(&i, &k.actions, &t, dt), PauseKeys::read(&i, &k.actions))
    };
    let (Some(mut game), Some(mut arena)) = (world.try_res_mut::<Match>(), world.try_res_mut::<Arena>()) else {
        return;
    };
    if pause_step(world, game.phase, keys) {
        return;
    }
    let was_alive = game.soldiers.first().is_some_and(|s| s.alive());
    let out = {
        let mut rng = world.res_mut::<Pcg32>();
        game.step(&input, &mut arena, &mut rng, &t, dt)
    };
    let died = was_alive && game.soldiers.first().is_some_and(|s| !s.alive());
    let free_roam = game.free_roam;
    drop(game);
    drop(arena);
    // Fail events (free roam never reaches them; `free_roam_test` checks).
    if free_roam {
        return;
    }
    if let Some(mut o) = world.try_res_mut::<PlayOutcomes>() {
        if died {
            o.life_lost();
        }
        if let Some(why) = out.ended {
            o.game_over();
            if why == WinReason::Time {
                o.timer_expired();
            }
        }
    }
}

/// The pause inputs of one fixed step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PauseKeys {
    /// The `pause` action (Esc, P, pad Start, the touch pause button) was pressed.
    pub toggle: bool,
    /// Fire was pressed (a click or a Fire tap also resumes a paused match).
    pub resume: bool,
}

impl PauseKeys {
    /// This step's pause inputs from the actions.
    pub fn read(input: &Input, a: &Actions) -> Self {
        Self { toggle: input.just_pressed(a.pause), resume: input.just_pressed(a.fire) }
    }
}

/// The pause rule, shared by the offline and the net step: during the
/// warm-up and the match `pause` flips it and Fire resumes; anywhere else it
/// is off. Returns whether the step is paused.
pub fn next_paused(paused: bool, phase: match_rules::Phase, keys: PauseKeys) -> bool {
    if !matches!(phase, match_rules::Phase::Pre | match_rules::Phase::Play) {
        return false;
    }
    if paused { !(keys.toggle || keys.resume) } else { keys.toggle }
}

/// Apply [`next_paused`] to the [`Paused`] resource. True when the step is
/// skipped: while paused, and on the step that resumes (so the click that
/// resumes never also fires).
pub(crate) fn pause_step(world: &World, phase: match_rules::Phase, keys: PauseKeys) -> bool {
    let Some(mut paused) = world.try_res_mut::<Paused>() else {
        return false;
    };
    let was = paused.0;
    paused.0 = next_paused(was, phase, keys);
    was || paused.0
}

/// A lock loss that pauses a match (the engine's rule, `PointerLock::last_loss`):
/// Esc (the browser keeps that key while locked, so the page never sees it),
/// the window losing focus, a hidden tab or an error. A refusal does not: the
/// match carries on with free look.
pub fn loss_pauses(loss: evani_core::pointer_lock::LockLoss) -> bool {
    use evani_core::pointer_lock::LockLoss;
    matches!(loss, LockLoss::Escape | LockLoss::Blur | LockLoss::Hidden | LockLoss::Error)
}

/// Fill [`MatchHud`] for this frame.
pub fn hud_system(world: &mut World) {
    let Some(k) = world.try_res::<TacticalFps>().map(|k| *k) else {
        return;
    };
    let t = tune_now(world, &k);
    let text = {
        let (Some(game), Some(arena)) = (world.try_res::<Match>(), world.try_res::<Arena>()) else {
            return;
        };
        game.hud_text(&arena, &t)
    };
    let paused = world.try_res::<Paused>().is_some_and(|p| p.0);
    if let Some(mut hud) = world.try_res_mut::<MatchHud>() {
        hud.paused = paused;
        if paused {
            hud.text = format!("Paused\n{text}");
        } else {
            hud.text = text;
        }
    }
}

/// Whether the kit wants the mouse locked: during the warm-up and the
/// match (free roam is a match that never ends), unless paused. The title
/// and the results give the mouse back so the page stays clickable.
pub fn wants_lock(phase: match_rules::Phase, paused: bool) -> bool {
    matches!(phase, match_rules::Phase::Pre | match_rules::Phase::Play) && !paused
}

/// The pointer lock, once a frame. Asks for it while [`wants_lock`] and
/// gives it back otherwise. The engine drops the lock on its own (Esc, blur,
/// a hidden tab, exit, a refusal, an error) and clears the wish; the kit
/// then only asks again, which the page grants at the player's next click,
/// never by itself. A refused lock leaves plain mouse movement working.
pub fn lock_system(world: &mut World) {
    let Some(phase) = world.try_res::<Match>().map(|g| g.phase) else {
        return;
    };
    let Some(mut lock) = world.try_res_mut::<PointerLock>() else {
        return;
    };
    // A loss mid-match (Esc, blur, a hidden tab) opens the pause screen;
    // losses anywhere else are read and dropped so they never pause later.
    let in_match = matches!(phase, match_rules::Phase::Pre | match_rules::Phase::Play);
    if lock.last_loss().is_some_and(loss_pauses) {
        lock.take_loss();
        if in_match && let Some(mut paused) = world.try_res_mut::<Paused>() {
            paused.0 = true;
        }
    }
    let want = wants_lock(phase, world.try_res::<Paused>().is_some_and(|p| p.0));
    // Any loss leaves the wish cleared (the engine's rule), so this asks
    // again while play still wants it; the page then tries once per click.
    if want && !lock.wanted() {
        lock.request();
    } else if !want && lock.wanted() {
        lock.release();
    }
}

struct MainScene;

impl Scene for MainScene {
    fn name(&self) -> &str {
        MAIN_SCENE
    }

    fn on_enter(&mut self, world: &mut World) {
        if let Err(e) = reset(world)
            && let Some(mut log) = world.try_res_mut::<evani_core::overlay::ErrorLog>() {
                log.error(format!("tactical-fps: {e}"));
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    /// Free roam: two minutes of walking (WASD), jumping (Space) and firing,
    /// with no game over, timer expiry, lost life or error, and the player
    /// still in the fortress at the end (docs/PLAYGROUNDS.md, SPEC 6).
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{ScriptKey, free_roam_test};
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(TacticalFpsKitPlugin);
        let mut script = Vec::new();
        let keys = ["KeyW", "KeyA", "KeyS", "KeyD", "Space", "KeyF"];
        for i in 0..20 {
            let at = 2.0 + f64::from(i) * 5.0;
            let key = keys[i as usize % keys.len()];
            script.push(ScriptKey::down(at, key));
            script.push(ScriptKey::up(at + 1.5, key));
        }
        let r = free_roam_test(&mut app, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(r.sim_seconds > 119.0, "{r:?}");
        let game = app.world.res::<Match>();
        assert!(game.free_roam);
        assert_eq!(game.soldiers.len(), 1, "no bots in free roam");
        assert_eq!(game.phase, match_rules::Phase::Play);
    }

    fn key(app: &mut App, code: &str, down: bool) {
        use evani_core::input::{KeyCode, RawEvent};
        let code = KeyCode::new(code).unwrap();
        app.world.res_mut::<Input>().handle(&RawEvent::Key { code, down });
    }

    fn tap(app: &mut App, code: &str) {
        key(app, code, true);
        app.run_headless(2, 1000.0 / 60.0);
        key(app, code, false);
        app.run_headless(2, 1000.0 / 60.0);
    }

    fn roaming() -> App {
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(TacticalFpsKitPlugin);
        app.run_headless(30, 1000.0 / 60.0);
        app
    }

    #[test]
    fn pause_rule() {
        use match_rules::Phase;
        let esc = PauseKeys { toggle: true, resume: false };
        let fire = PauseKeys { toggle: false, resume: true };
        let none = PauseKeys::default();
        assert!(next_paused(false, Phase::Play, esc));
        assert!(next_paused(false, Phase::Pre, esc));
        assert!(!next_paused(false, Phase::Play, fire), "Fire never pauses");
        assert!(next_paused(true, Phase::Play, none));
        assert!(!next_paused(true, Phase::Play, esc));
        assert!(!next_paused(true, Phase::Play, fire), "a click resumes");
        assert!(!next_paused(true, Phase::Title, none), "no pause off the match");
        assert!(!next_paused(false, Phase::Results, esc));
    }

    /// Esc pauses the match: the match clock stops, the pointer lock is
    /// given back and the HUD says so; Esc again resumes (the test-pass
    /// finding: Escape showed no pause screen).
    #[test]
    fn escape_pauses_and_resumes() {
        let mut app = roaming();
        tap(&mut app, "Escape");
        assert!(app.world.res::<Paused>().0);
        assert!(app.world.res::<MatchHud>().paused);
        assert!(app.world.res::<MatchHud>().text.starts_with("Paused"));
        assert!(!app.world.res::<PointerLock>().wanted(), "pause gives the mouse back");
        let t0 = app.world.res::<Match>().phase_t;
        app.run_headless(60, 1000.0 / 60.0);
        assert_eq!(app.world.res::<Match>().phase_t, t0, "the match waits while paused");
        tap(&mut app, "Escape");
        assert!(!app.world.res::<Paused>().0);
        app.run_headless(30, 1000.0 / 60.0);
        assert!(app.world.res::<Match>().phase_t > t0);
        assert!(app.world.res::<PointerLock>().wanted(), "play asks for the lock again");
        // P works too.
        tap(&mut app, "KeyP");
        assert!(app.world.res::<Paused>().0);
    }

    /// In a real browser Esc ends the pointer lock and the page never sees
    /// the key: the lost lock pauses the match instead. A refusal does not.
    #[test]
    fn losing_the_lock_pauses() {
        use evani_core::pointer_lock::LockLoss;
        let mut app = roaming();
        app.world.res_mut::<PointerLock>().on_lost(LockLoss::Refused);
        app.run_headless(2, 1000.0 / 60.0);
        assert!(!app.world.res::<Paused>().0, "free look carries on after a refusal");
        app.world.res_mut::<PointerLock>().request();
        app.world.res_mut::<PointerLock>().on_locked();
        app.world.res_mut::<PointerLock>().on_lost(LockLoss::Escape);
        app.run_headless(2, 1000.0 / 60.0);
        assert!(app.world.res::<Paused>().0);
        // A click resumes.
        tap(&mut app, "KeyF");
        assert!(!app.world.res::<Paused>().0);
    }
}
