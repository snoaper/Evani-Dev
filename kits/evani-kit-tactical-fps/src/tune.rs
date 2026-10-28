//! Every tunable of the kit: one registered param `kit.tactical_fps.<name>`
//! each, with its default read from `data/tuning.json` (Hexagon's
//! `game/tuning.json`, snake_case, times in seconds, weapon arrays split per
//! weapon). The JSON is the only source of the defaults: a name missing from
//! it is a build error, so the file and the params never drift apart.

use evani_core::params::{F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};

/// The tuning defaults, shipped inside the wasm.
pub const TUNING_JSON: &str = include_str!("../data/tuning.json");

/// The default of `name` in the tuning file.
fn default_of(map: &serde_json::Map<String, serde_json::Value>, name: &str) -> Result<f64, PluginError> {
    map.get(name)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| PluginError(format!("data/tuning.json has no number `{name}`")))
}

macro_rules! tunables {
    (
        f32 { $( $f:ident, [$fmin:expr, $fmax:expr], $funit:literal, $fdesc:literal; )* }
        i32 { $( $i:ident, [$imin:expr, $imax:expr], $idesc:literal; )* }
    ) => {
        /// Handles of every tunable (no string lookups in the loop).
        #[derive(Debug, Clone, Copy)]
        pub struct Handles {
            $(pub $f: F32Param,)*
            $(pub $i: I32Param,)*
        }

        /// Every tunable's value for one step.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Tune {
            $(pub $f: f32,)*
            $(pub $i: i32,)*
        }

        impl Handles {
            /// Register every tunable with its default from `data/tuning.json`.
            pub fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                let map: serde_json::Map<String, serde_json::Value> =
                    serde_json::from_str(TUNING_JSON).map_err(|e| PluginError(format!("data/tuning.json: {e}")))?;
                Ok(Self {
                    $($f: kit.param_f32(
                        ParamDef::f32(concat!("kit.tactical_fps.", stringify!($f)), default_of(&map, stringify!($f))? as f32)
                            .range($fmin, $fmax)
                            .unit($funit)
                            .describe($fdesc),
                    )?,)*
                    $($i: kit.param_i32(
                        ParamDef::i32(concat!("kit.tactical_fps.", stringify!($i)), default_of(&map, stringify!($i))? as i32)
                            .range($imin, $imax)
                            .describe($idesc),
                    )?,)*
                })
            }

            /// Read every tunable.
            pub fn read(&self, reg: &ParamRegistry) -> Tune {
                Tune {
                    $($f: reg.f32(self.$f),)*
                    $($i: reg.i32(self.$i),)*
                }
            }
        }
    };
}

tunables! {
    f32 {
        hold_s, [0.5, 60.0], "s", "Time in the enemy core zone, with no defender inside, that wins the match.";
        round_s, [30.0, 3600.0], "s", "Match length; when it runs out the team with more kills wins.";
        pre_s, [0.0, 30.0], "s", "Warm-up at the bases before the match starts.";
        results_s, [1.0, 60.0], "s", "How long the results stay up before the title.";
        ring_interval_s, [10.0, 600.0], "s", "Time between turns of the middle ring.";
        ring_rotate_s, [0.5, 30.0], "s", "How long one ring turn (one sector) takes.";
        ring_warn_s, [0.0, 60.0], "s", "Warning shown before the ring turns.";
        respawn_s, [0.0, 30.0], "s", "Time from death to respawn at the base.";
        spawn_shield_s, [0.0, 10.0], "s", "No damage for this long after a respawn.";
        max_hp, [1.0, 1000.0], "hp", "Health of a soldier.";
        walk_speed, [1.0, 20.0], "m/s", "Running speed.";
        accel_ground, [1.0, 200.0], "m/s2", "Acceleration towards the wanted velocity on the ground.";
        accel_air, [0.0, 100.0], "m/s2", "Air control acceleration.";
        gravity, [1.0, 60.0], "m/s2", "Gravity on soldiers and grenades.";
        jump_speed, [0.0, 20.0], "m/s", "Upward speed of a jump.";
        lift_up, [0.5, 20.0], "m/s", "Rising speed in a lift shaft with jump held.";
        lift_down, [0.5, 20.0], "m/s", "Most falling speed in a lift shaft.";
        fall_death_y, [-200.0, -1.0], "m", "Falling below this height kills.";
        pistol_damage, [0.0, 500.0], "hp", "Pistol damage per hit.";
        sniper_damage, [0.0, 500.0], "hp", "Sniper damage per hit.";
        rocket_damage, [0.0, 500.0], "hp", "Rocket blast damage at the centre.";
        grenade_damage, [0.0, 500.0], "hp", "Grenade blast damage at the centre.";
        pistol_cooldown_s, [0.02, 5.0], "s", "Time between pistol shots.";
        sniper_cooldown_s, [0.02, 5.0], "s", "Time between sniper shots.";
        rocket_cooldown_s, [0.02, 5.0], "s", "Time between rockets.";
        grenade_cooldown_s, [0.02, 5.0], "s", "Time between grenades.";
        pistol_range, [5.0, 1000.0], "m", "Pistol ray length.";
        sniper_range, [5.0, 1000.0], "m", "Sniper ray length.";
        rocket_speed, [1.0, 200.0], "m/s", "Rocket flight speed.";
        grenade_speed, [1.0, 100.0], "m/s", "Grenade throw speed along the aim.";
        grenade_up, [0.0, 30.0], "m/s", "Extra upward speed of a grenade throw.";
        grenade_fuse_s, [0.1, 10.0], "s", "Grenade fuse.";
        grenade_bounce, [0.0, 1.0], "", "Share of speed a grenade keeps when it bounces.";
        proj_life_s, [0.5, 30.0], "s", "Longest a rocket or grenade lives.";
        blast_radius, [0.5, 20.0], "m", "Rocket and grenade blast radius (damage falls off to zero at the edge).";
        self_damage, [0.0, 1.0], "x", "Share of their own blast's damage a shooter takes.";
        pistol_recoil, [0.0, 0.3], "rad", "Aim kick each pistol shot adds.";
        sniper_recoil, [0.0, 0.5], "rad", "Aim kick each sniper shot adds.";
        recoil_decay, [0.1, 40.0], "1/s", "How fast recoil settles back (exponential rate).";
        spread_base, [0.0, 0.2], "rad", "Spread cone half-angle standing still.";
        spread_move, [0.0, 0.3], "rad", "Extra spread at full running speed.";
        spread_air, [0.0, 0.5], "rad", "Extra spread while airborne.";
        sniper_spread_mult, [0.0, 5.0], "x", "Sniper spread relative to the pistol's.";
        bot_aim_error, [0.0, 0.5], "rad", "Bots' aim error when they first see a target (it settles while they keep it in view).";
        bot_react_s, [0.0, 3.0], "s", "Bots wait this long after seeing a target before firing.";
        bot_sight, [5.0, 400.0], "m", "How far bots see.";
        pistol_hole, [0.02, 3.0], "m", "Hole radius a pistol hit leaves in concrete.";
        sniper_hole, [0.02, 3.0], "m", "Hole radius a sniper hit leaves in concrete.";
        rocket_hole, [0.1, 5.0], "m", "Crater radius of a rocket blast in concrete.";
        grenade_hole, [0.1, 5.0], "m", "Crater radius of a grenade blast in concrete.";
        hole_depth, [0.0, 1.0], "m", "How far a bullet hole's sphere centre sits inside the surface.";
        cell_min_solid, [0.0, 1.0], "x", "A cover cell with less than this share left solid is gone.";
        support_min, [0.0, 1.0], "x", "Share of a cover group's bottom that must stand on solid floor.";
        tip_margin, [0.0, 2.0], "m", "How far inside its support footprint a group's centre of mass must sit.";
        look_speed, [0.1, 10.0], "rad/s", "Turn speed from the look stick and arrow keys.";
        mouse_sens, [0.0001, 0.02], "rad/px", "Turn per pixel of mouse movement.";
        fov_deg, [40.0, 110.0], "deg", "First-person vertical field of view.";
        title_orbit_speed, [0.0, 1.0], "rad/s", "How fast the title camera circles the fortress.";
    }
    i32 {
        team_size, [0.0, 4.0], "Soldiers per team (bots fill the places no player takes).";
        kills_to_win, [1.0, 500.0], "Team kills that win the match.";
        hole_cap, [1.0, 200.0], "Holes one bucket holds before new holes merge into the nearest.";
        hole_max, [16.0, 20000.0], "Most holes kept; the oldest go first.";
    }
}

/// Weapon slots, in Hexagon's order.
pub const PISTOL: usize = 0;
/// The sniper rifle.
pub const SNIPER: usize = 1;
/// The rocket launcher.
pub const ROCKETS: usize = 2;
/// Grenades.
pub const GRENADES: usize = 3;
/// Weapon names for the HUD.
pub const WEAPON_NAMES: [&str; 4] = ["Pistol", "Sniper", "Rockets", "Grenades"];

impl Tune {
    /// Damage of weapon `w`.
    pub fn damage(&self, w: usize) -> f32 {
        [self.pistol_damage, self.sniper_damage, self.rocket_damage, self.grenade_damage][w & 3]
    }

    /// Cooldown of weapon `w` (s).
    pub fn cooldown(&self, w: usize) -> f32 {
        [self.pistol_cooldown_s, self.sniper_cooldown_s, self.rocket_cooldown_s, self.grenade_cooldown_s][w & 3]
    }

    /// Ray length of a hitscan weapon (0 for the projectile weapons).
    pub fn range(&self, w: usize) -> f32 {
        [self.pistol_range, self.sniper_range, 0.0, 0.0][w & 3]
    }

    /// Hole radius weapon `w` leaves in concrete.
    pub fn hole(&self, w: usize) -> f32 {
        [self.pistol_hole, self.sniper_hole, self.rocket_hole, self.grenade_hole][w & 3]
    }
}
