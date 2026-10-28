//! `evani-kit-top-down-action-adventure`: a top-down action-adventure,
//! Mossgate Keep. A hero walks a small dungeon of single-screen rooms, swings
//! a sword (and later throws a wand bolt), fights slimes, bats, husks and
//! spitters, opens a locked door with a key, steps on a floor switch to open
//! a gate, takes hearts and gems, and beats the Warden for the Dawn Shard.
//! Drawn in original pixel art made in code ([`art`]) and coloured from the
//! active look's ramps, so it never names a look; with the `lights2d`
//! feature the torches, magic, bolts and the hero push 2D lights ([`draw`]).
//!
//! * Params (owner `kit.top-down-action-adventure`): every tunable is
//!   `kit.top_down_action_adventure.*` (a param segment must start with a
//!   letter), see [`Tune`], [`Ints`] and `kits/top-down-action-adventure/SPEC.md`.
//! * Input actions: `move_x`, `move_y`, `attack`, `item`, `pause`. On phones
//!   and tablets the [`touch`] layer sets the same actions.
//! * Data file: `top_down_action_adventure.level` (`data/level.json`).
//! * Resources: [`TopDown`] (the game), [`ecs::Objects`] (its entities),
//!   [`evani_render::SpriteSheet`] (one white cell for rectangles and text);
//!   the kit's `tda.*` frames go into [`evani_render::SpriteArt`].
//! * Systems: `top_down_action_adventure.sim` (fixed `Update`) and
//!   `top_down_action_adventure.draw` (`Frame`).

pub mod art;
pub mod draw;
pub mod ecs;
pub mod font;
pub mod game;
pub mod level;
pub mod touch;

use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::params::{F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::play::{PlayMode, PlayOutcomes};
use evani_core::plugin::{KitPlugin, PluginContext, PluginError};
use evani_core::rng::Pcg32;
use evani_core::time::Time;

use crate::game::{Controls, Game};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// The dungeon (`Mossgate Keep`).
pub const LEVEL_JSON: &str = include_str!("../data/level.json");

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

        impl Handles {
            fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                Ok(Self {
                    $($field: kit.param_f32(
                        ParamDef::f32(concat!("kit.top_down_action_adventure.", stringify!($field)), $def)
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

macro_rules! int_tunables {
    ($( $field:ident = $def:expr, $max:expr, $desc:literal; )*) => {
        /// Integer settings.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Ints {
            $(pub $field: i32,)*
        }

        impl Default for Ints {
            fn default() -> Self {
                Self { $($field: $def,)* }
            }
        }

        /// Handles of every integer setting.
        #[derive(Debug, Clone, Copy)]
        pub struct IntHandles {
            $(pub $field: I32Param,)*
        }

        impl IntHandles {
            fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                Ok(Self {
                    $($field: kit.param_i32(
                        ParamDef::i32(concat!("kit.top_down_action_adventure.", stringify!($field)), $def)
                            .range(0.0, $max)
                            .describe($desc),
                    )?,)*
                })
            }

            /// Read every integer setting.
            pub fn read(&self, reg: &ParamRegistry) -> Ints {
                Ints { $($field: reg.i32(self.$field),)* }
            }
        }
    };
}

tunables! {
    walk_speed = 70.0, [10.0, 300.0], "px/s", "Hero walking speed.";
    hero_w = 10.0, [4.0, 16.0], "px", "Hero collision box width.";
    hero_h = 10.0, [4.0, 16.0], "px", "Hero collision box height.";
    sword_time_s = 0.18, [0.05, 1.0], "s", "How long a sword swing lasts.";
    sword_cooldown_s = 0.12, [0.0, 2.0], "s", "Rest after a swing before the next.";
    sword_reach = 14.0, [4.0, 40.0], "px", "How far in front of the hero the sword reaches.";
    sword_width = 16.0, [4.0, 48.0], "px", "Width of the sword's arc across the facing direction.";
    knockback_speed = 150.0, [0.0, 600.0], "px/s", "Speed an enemy is knocked back at when hit.";
    hero_knockback_speed = 130.0, [0.0, 600.0], "px/s", "Speed the hero is knocked back at when hurt.";
    knockback_s = 0.15, [0.0, 1.0], "s", "How long a knockback lasts.";
    hurt_invuln_s = 1.0, [0.0, 5.0], "s", "Blinking safe time after the hero is hurt.";
    hit_stop_s = 0.05, [0.0, 0.3], "s", "Brief freeze on a hit that lands.";
    enter_grace_s = 0.5, [0.0, 3.0], "s", "Safe time after the hero walks into a new room.";
    bolt_speed = 170.0, [20.0, 600.0], "px/s", "Wand bolt speed.";
    bolt_cooldown_s = 0.35, [0.0, 3.0], "s", "Rest between wand bolts.";
    bolt_life_s = 1.2, [0.1, 5.0], "s", "How long a wand bolt flies.";
    slime_speed = 22.0, [0.0, 200.0], "px/s", "Slime walking speed.";
    slime_turn_s = 1.0, [0.1, 5.0], "s", "How often a slime picks a new way to wander.";
    bat_speed = 46.0, [0.0, 300.0], "px/s", "Bat flying speed.";
    bat_sight = 96.0, [0.0, 400.0], "px", "A bat swoops at a hero this close.";
    husk_speed = 24.0, [0.0, 200.0], "px/s", "Husk walking speed.";
    husk_charge_speed = 100.0, [0.0, 400.0], "px/s", "Husk charge speed.";
    husk_sight = 112.0, [0.0, 400.0], "px", "A husk winds up when the hero is this close and in line.";
    husk_windup_s = 0.45, [0.05, 3.0], "s", "Husk warning time before a charge.";
    husk_charge_s = 0.6, [0.05, 3.0], "s", "How long a husk charges.";
    husk_rest_s = 0.6, [0.0, 3.0], "s", "Husk rest after a charge (the time to strike it).";
    spitter_period_s = 2.2, [0.3, 10.0], "s", "Time between a spitter's shots.";
    spitter_windup_s = 0.4, [0.05, 3.0], "s", "Spitter warning time before a shot.";
    spit_speed = 70.0, [10.0, 400.0], "px/s", "Spat shot speed.";
    warden_speed = 28.0, [0.0, 200.0], "px/s", "Warden walking speed.";
    warden_dash_speed = 120.0, [0.0, 400.0], "px/s", "Warden dash speed.";
    warden_dash_s = 0.5, [0.05, 3.0], "s", "How long a Warden dash lasts.";
    warden_period_s = 2.0, [0.3, 10.0], "s", "Time between the Warden's attacks.";
    warden_windup_s = 0.5, [0.05, 3.0], "s", "Warden warning time before an attack.";
    warden_fan_speed = 65.0, [10.0, 400.0], "px/s", "Speed of the Warden's fanned shots.";
    drop_chance = 0.25, [0.0, 1.0], "", "Chance a slain enemy drops a heart or gem.";
    shake_px = 3.0, [0.0, 8.0], "px", "Largest screen shake, in art pixels.";
    shake_decay = 2.0, [0.1, 10.0], "1/s", "Shake trauma lost per second.";
    pop_life_s = 0.9, [0.1, 5.0], "s", "How long floating text stays up.";
    hud_height_px = 24.0, [8.0, 48.0], "px", "Height of the HUD strip, in art pixels.";
    hud_hearts_x = 6.0, [0.0, 256.0], "px", "Left of the first HUD heart, art px.";
    hud_hearts_y = 6.0, [0.0, 48.0], "px", "Top of the first row of HUD hearts, art px.";
    hud_heart_step = 8.0, [4.0, 16.0], "px", "Distance between HUD hearts, art px.";
    hud_keys_x = 84.0, [0.0, 256.0], "px", "Left of the HUD key icon and count, art px.";
    hud_gems_x = 116.0, [0.0, 256.0], "px", "Left of the HUD gem icon and count, art px.";
    hud_icons_y = 6.0, [0.0, 48.0], "px", "Top of the HUD key and gem icons, art px.";
    hud_slot_x = 154.0, [0.0, 256.0], "px", "Left of the 18 px item slot, art px.";
    hud_slot_y = 3.0, [0.0, 48.0], "px", "Top of the item slot, art px.";
    hud_map_x = 196.0, [0.0, 256.0], "px", "Left of the room map, art px.";
    hud_map_y = 3.0, [0.0, 48.0], "px", "Top of the room map, art px.";
    hud_label_y = 16.0, [0.0, 48.0], "px", "Top of the FREE ROAM label, art px.";
    shadow_alpha = 0.35, [0.0, 1.0], "", "Darkness of contact shadows and wall shade strips.";
    torch_light_radius = 72.0, [0.0, 256.0], "px", "Reach of a wall torch's light, art px.";
    torch_light_intensity = 1.0, [0.0, 4.0], "", "Brightness of a wall torch's light.";
    hero_light_radius = 48.0, [0.0, 256.0], "px", "Reach of the soft light around the hero, art px.";
    hero_light_intensity = 0.35, [0.0, 4.0], "", "Brightness of the soft light around the hero.";
    ambient_tint = 0.35, [0.0, 1.0], "", "How far the unlit shade leans to the look's ambient light colour.";
    banner_band_alpha = 1.0, [0.0, 1.0], "", "Opacity of the dark band behind title and end-screen lettering, so floor cracks never show through it.";
}

int_tunables! {
    max_hp = 6, 40.0, "Hero health at the start (2 per heart).";
    sword_damage = 1, 20.0, "Damage of a sword hit.";
    bolt_damage = 1, 20.0, "Damage of a wand bolt.";
    heal_hp = 2, 40.0, "Health a heart pickup restores.";
    container_hp = 2, 40.0, "Health a heart container adds.";
    contact_damage = 1, 20.0, "Damage from touching an enemy.";
    warden_damage = 2, 20.0, "Damage from touching or being struck by the Warden.";
    shot_damage = 1, 20.0, "Damage of a spat shot.";
    slime_hp = 2, 99.0, "Slime health.";
    bat_hp = 1, 99.0, "Bat health.";
    husk_hp = 3, 99.0, "Husk health.";
    spitter_hp = 2, 99.0, "Spitter health.";
    warden_hp = 12, 99.0, "Warden health.";
    warden_fan_count = 5, 12.0, "Shots in the Warden's fan.";
}

/// Handles the kit's systems read every tick (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct TopDownKit {
    pub tune: Handles,
    pub ints: IntHandles,
    pub actions: Actions,
}

#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub move_x: ActionId,
    pub move_y: ActionId,
    pub attack: ActionId,
    pub item: ActionId,
    pub pause: ActionId,
}

/// The kit's world resource: the game the player plays.
pub struct TopDown {
    pub game: Game,
}

/// The top-down action-adventure kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct TopDownActionAdventureKitPlugin;

fn button(kit: &mut PluginContext, name: &str, binds: &[&str], desc: &str) -> Result<ActionId, PluginError> {
    Ok(kit.action(ActionDef::button(name, binds).map_err(|e| e.to_string())?.describe(desc)))
}

fn axis(kit: &mut PluginContext, name: &str, binds: &[&str], desc: &str) -> Result<ActionId, PluginError> {
    Ok(kit.action(ActionDef::axis(name, binds).map_err(|e| e.to_string())?.describe(desc)))
}

impl KitPlugin for TopDownActionAdventureKitPlugin {
    fn name(&self) -> &'static str {
        "top-down-action-adventure"
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let tune = Handles::register(kit)?;
        let ints = IntHandles::register(kit)?;

        let move_x = axis(kit, "move_x", &["keys:KeyA/KeyD", "keys:ArrowLeft/ArrowRight", "pad:axis:0"], "Walk left and right.")?;
        let move_y = axis(kit, "move_y", &["keys:KeyW/KeyS", "keys:ArrowUp/ArrowDown", "pad:axis:1"], "Walk up and down.")?;
        let attack = button(kit, "attack", &["key:Space", "key:KeyJ", "key:KeyZ", "pad:button:0"], "Swing the sword (also starts a run).")?;
        let item = button(kit, "item", &["key:KeyK", "key:KeyX", "pad:button:1"], "Use the item (the wand, once found).")?;
        let pause = button(kit, "pause", &["key:Escape", "key:KeyP", "pad:button:9"], "Pause the game.")?;

        // On-screen controls for phones and tablets: they only set the
        // actions above (see the `touch` module).
        kit.touch_controls(touch::layout())?;

        kit.data_file("top_down_action_adventure.level", LEVEL_JSON);
        // Values tuned live and written by bake.export.
        kit.baked_files(BAKED);

        let level = Rc::new(level::Level::from_json(LEVEL_JSON)?);
        let handles = TopDownKit {
            tune,
            ints,
            actions: Actions { move_x, move_y, attack, item, pause },
        };
        let (t, n) = {
            let reg = kit.world.res::<ParamRegistry>();
            (tune.read(&reg), ints.read(&reg))
        };
        let mut game = Game::new(level, &t, &n);
        // Free roam (docs/PLAYGROUNDS.md): straight into the dungeon, no
        // goal and no game over.
        let free_roam = kit.world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
        if free_roam {
            game.set_free_roam(true, &t, &n);
        }
        kit.resource(TopDown { game });
        // The player's game objects live in the ECS (module `ecs`).
        ecs::register(kit.world);
        kit.resource(ecs::Objects::default());
        kit.resource(draw::DrawState::default());
        kit.resource(evani_render::SpriteSheet::default());
        kit.resource(handles);
        kit.system(SystemDesc::new("top_down_action_adventure.sim", Stage::Update, sim_system));
        kit.system(SystemDesc::new("top_down_action_adventure.draw", Stage::Frame, draw::draw_system));
        Ok(())
    }

    /// Free roam (docs/PLAYGROUNDS.md): no goal and no game over under
    /// `PlayMode::FreeRoam`; see [`game::Game::free_roam`].
    fn free_roam(&self) -> bool {
        true
    }
}

/// The fixed-step game update.
pub fn sim_system(world: &mut World) {
    let Some(k) = world.try_res::<TopDownKit>().map(|k| *k) else {
        return;
    };
    let dt = world.res::<Time>().fixed_dt;
    let (t, n) = {
        let reg = world.res::<ParamRegistry>();
        (k.tune.read(&reg), k.ints.read(&reg))
    };
    // A seed for a new run, drawn from the app's seeded generator (the only
    // randomness; no wall clock).
    let seed = world.res_mut::<Pcg32>().next_u64();
    let c = {
        let input = world.res::<Input>();
        let a = &k.actions;
        Controls {
            x: input.value(a.move_x).clamp(-1.0, 1.0),
            y: input.value(a.move_y).clamp(-1.0, 1.0),
            attack_pressed: input.just_pressed(a.attack),
            item_pressed: input.just_pressed(a.item),
            pause_pressed: input.just_pressed(a.pause),
            seed,
        }
    };
    // The player's game objects live in the ECS: the step starts from their
    // entities (unless the game was just reset) and writes every change back
    // (`ecs`). Both resources are taken out for the step so the sync can
    // spawn and despawn.
    let Some(mut p) = world.remove_resource::<TopDown>() else {
        return;
    };
    let mut objects = world.remove_resource::<ecs::Objects>().unwrap_or_default();
    if !p.game.dirty {
        objects.gather(world, &mut p.game);
    }
    p.game.tick(c, &t, &n, dt);
    let fails = p.game.take_fails();
    objects.sync(world, &p.game);
    p.game.dirty = false;
    world.insert_resource(p);
    world.insert_resource(objects);
    // The touch controls stay up in every phase: the title, the win and the
    // game-over screens wait for `attack`, which the touch button sets.
    evani_core::touch::set_enabled(world, true);
    // Every game over goes to PlayOutcomes (the free-roam test fails on any;
    // free roam itself has none).
    if fails.game_overs > 0
        && let Some(mut o) = world.try_res_mut::<PlayOutcomes>()
    {
        for _ in 0..fails.game_overs {
            o.game_over();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Phase;
    use evani_core::App;

    fn app() -> App {
        App::new("t").with_kit(TopDownActionAdventureKitPlugin)
    }

    fn free_app() -> App {
        App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(TopDownActionAdventureKitPlugin)
    }

    /// The kit registers its tunables, actions and level data.
    #[test]
    fn registers_tunables_actions_and_data() {
        let mut a = app();
        a.run_headless(3, 1000.0 / 60.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "kit.top_down_action_adventure.walk_speed",
            "kit.top_down_action_adventure.sword_reach",
            "kit.top_down_action_adventure.max_hp",
            "kit.top_down_action_adventure.warden_hp",
        ] {
            let id = reg.id(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(reg.def(id).map(|d| d.owner.as_str()), Some("kit.top-down-action-adventure"), "{n}");
        }
        drop(reg);
        let input = a.world.res::<Input>();
        for n in ["move_x", "move_y", "attack", "item", "pause"] {
            assert!(input.action_id(n).is_some(), "{n}");
        }
        drop(input);
        let data = a.world.res::<evani_core::plugin::DataFiles>();
        assert!(data.get("top_down_action_adventure.level").is_some());
        drop(data);
        assert_eq!(a.world.res::<TopDown>().game.phase, Phase::Title);
    }

    /// Free roam: the kit says so and starts straight in the dungeon.
    #[test]
    fn free_roam_starts_in_the_dungeon() {
        assert!(TopDownActionAdventureKitPlugin.free_roam());
        let a = free_app();
        assert_eq!(a.play_mode(), PlayMode::FreeRoam);
        let p = a.world.res::<TopDown>();
        assert!(p.game.free_roam && p.game.phase == Phase::Playing);
        drop(p);
        let play = app();
        assert!(!play.world.res::<TopDown>().game.free_roam);
        assert_eq!(play.play_mode(), PlayMode::Play);
    }

    /// Two minutes of scripted input in free roam: walk around, swing, walk
    /// through the doorways and into enemies, with no game over or error.
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{ScriptKey, free_roam_test};
        let mut app = free_app();
        let mut script = Vec::new();
        for c in 0..6 {
            let t0 = f64::from(c) * 20.0;
            script.push(ScriptKey::down(t0 + 0.2, "KeyD"));
            for j in 0..6 {
                let at = t0 + 1.0 + f64::from(j) * 1.3;
                script.push(ScriptKey::down(at, "Space"));
                script.push(ScriptKey::up(at + 0.2, "Space"));
            }
            script.push(ScriptKey::up(t0 + 9.0, "KeyD"));
            script.push(ScriptKey::down(t0 + 9.5, "KeyS"));
            script.push(ScriptKey::up(t0 + 12.5, "KeyS"));
            script.push(ScriptKey::down(t0 + 13.0, "KeyA"));
            script.push(ScriptKey::up(t0 + 18.0, "KeyA"));
        }
        let r = free_roam_test(&mut app, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(r.sim_seconds > 119.0, "{r:?}");
        let p = app.world.res::<TopDown>();
        assert_eq!(p.game.phase, Phase::Playing);
        assert!(p.game.player.hp > 0);
        assert!(p.game.tick > 6000, "the game ran {} ticks", p.game.tick);
    }
}
