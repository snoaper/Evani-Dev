//! Every combat number in one table. [`Tuning::default`] holds the starting
//! values (a fast automatic rifle, a semi-automatic pistol, a one-shot
//! rocket launcher, a frag grenade, a bot of middling skill); the live
//! `combat.*` parameters ([`crate::params`]) override any of them, and a
//! kit bakes its own values into its parameter file.

/// One gun.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponStats {
    pub name: String,
    /// Damage of a chest hit at point blank.
    pub damage: f32,
    /// Share of the damage that goes through armour (0..1).
    pub armor_pen: f32,
    pub head_mult: f32,
    pub stomach_mult: f32,
    pub leg_mult: f32,
    /// Share of the damage kept per [`crate::gun::RANGE_UNIT`] (9.525 m) of
    /// range (1 = none lost); see [`crate::gun::damage_at`].
    pub range_mod: f32,
    /// Seconds between shots.
    pub interval: f32,
    /// Keeps firing while the trigger is held (else one shot per press).
    pub auto: bool,
    pub mag: u32,
    pub reserve: u32,
    /// Seconds to reload.
    pub reload: f32,
    /// Longest shot (m).
    pub range: f32,
    /// Cone half-angle standing still with a settled gun (rad).
    pub spread: f32,
    /// Extra cone at full run speed (rad).
    pub spread_move: f32,
    /// Extra cone in the air (rad).
    pub spread_air: f32,
    /// Extra cone per shot of heat (rad).
    pub spread_fire: f32,
    /// Crouching multiplies the cone by this.
    pub crouch_mult: f32,
    /// View kick up per shot (rad), growing with heat.
    pub kick: f32,
    /// Sideways kick as a share of the up kick (random sign).
    pub kick_side: f32,
    /// Most view kick that builds up (rad).
    pub kick_max: f32,
    /// How fast the view comes back down (rad/s).
    pub recover: f32,
    /// Heat lost per second after a short pause.
    pub cool: f32,
}

/// One grenade kind.
#[derive(Debug, Clone, PartialEq)]
pub struct GrenadeStats {
    pub name: String,
    /// Damage at the centre.
    pub damage: f32,
    /// Blast radius (m); damage falls linearly to 0 there.
    pub radius: f32,
    pub armor_pen: f32,
    /// Seconds from the throw to the blast.
    pub fuse: f32,
    /// Throw speed (m/s).
    pub speed: f32,
    /// Extra upward pitch on the throw (rad).
    pub loft: f32,
    /// Gravity on the grenade (m/s^2).
    pub gravity: f32,
    /// Bounce: share of the normal speed kept (0..1).
    pub restitution: f32,
    /// Share of the sliding speed lost per bounce (0..1).
    pub friction: f32,
    /// Collision radius (m).
    pub size: f32,
    /// Grenades carried per life.
    pub carry: u32,
}

/// One rocket kind: a fast projectile with a little gravity that goes off
/// on contact (or when its flight time ends).
#[derive(Debug, Clone, PartialEq)]
pub struct RocketStats {
    pub name: String,
    /// Damage at the centre of the blast.
    pub damage: f32,
    /// Blast radius (m); damage falls linearly to 0 there.
    pub radius: f32,
    pub armor_pen: f32,
    /// Launch speed (m/s).
    pub speed: f32,
    /// Gravity on the rocket (m/s^2): a slight drop over long shots.
    pub gravity: f32,
    /// Collision radius (m).
    pub size: f32,
    /// Seconds of flight before it goes off on its own.
    pub life: f32,
}

/// The bot brain's numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct BotTune {
    /// Seconds a foe must be in sight before the first shot.
    pub react: f32,
    /// Aim error at range (rad, one axis).
    pub aim_err: f32,
    /// Error grows near: `err * (1 + near / max(d, 6))`.
    pub aim_near: f32,
    /// Seconds for the error to settle to [`BotTune::settle_min`].
    pub settle: f32,
    pub settle_min: f32,
    /// Seconds between looks for foes, plus up to `scan_jitter`.
    pub scan: f32,
    pub scan_jitter: f32,
    /// Fastest turn (rad/s).
    pub turn: f32,
    /// Fires when the aim is this close to where it wants it (rad).
    pub fire_cone: f32,
    /// Sees this far (m) within `fov` either side of its heading (rad).
    pub view: f32,
    pub fov: f32,
    /// Sound radii are scaled by this when hearing.
    pub hearing: f32,
    /// Seconds it keeps pressing a foe it lost before investigating.
    pub lost: f32,
    /// Seconds an investigation lasts at most.
    pub investigate: f32,
    /// Health under which it backs off.
    pub retreat_hp: f32,
    /// Seconds a retreat lasts.
    pub retreat_time: f32,
    /// Seconds between strafe flips while fighting.
    pub strafe: f32,
    /// Beyond this range (m) it taps bursts.
    pub burst_range: f32,
    pub burst: u32,
    /// Pause between bursts (s).
    pub burst_pause: f32,
    /// Reaches a waypoint within this (m).
    pub reach: f32,
}

/// How far each kind of noise carries (m).
#[derive(Debug, Clone, PartialEq)]
pub struct SoundTune {
    pub gunshot: f32,
    pub footstep: f32,
    pub blast: f32,
    pub bounce: f32,
}

/// The whole table.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    pub rifle: WeaponStats,
    pub pistol: WeaponStats,
    /// The rocket launcher (one rocket per magazine; the shot itself is
    /// [`Tuning::rocket`]).
    pub launcher: WeaponStats,
    pub grenade: GrenadeStats,
    pub rocket: RocketStats,
    pub bot: BotTune,
    pub sound: SoundTune,
    /// Armour at spawn and whether it has a helmet.
    pub armour: f32,
    pub helmet: bool,
    pub health: f32,
    /// Speed counted as "full run" for the move spread (m/s).
    pub run_speed: f32,
    /// Seconds before a dead player comes back.
    pub respawn: f32,
}

impl Default for Tuning {
    /// The starting values: a rifle at 600 rounds a minute whose headshot
    /// kills through a helmet, a pistol whose headshot kills only without
    /// one, a grenade that never kills a full-health body on its own, and a
    /// launcher whose direct hit kills through armour.
    fn default() -> Self {
        Tuning {
            rifle: WeaponStats {
                name: "rifle".into(),
                damage: 36.0,
                armor_pen: 0.775,
                head_mult: 4.0,
                stomach_mult: 1.25,
                leg_mult: 0.75,
                range_mod: 0.98,
                interval: 0.1,
                auto: true,
                mag: 30,
                reserve: 90,
                reload: 2.45,
                range: 160.0,
                spread: 0.0025,
                spread_move: 0.07,
                spread_air: 0.2,
                spread_fire: 0.006,
                crouch_mult: 0.75,
                kick: 0.011,
                kick_side: 0.35,
                kick_max: 0.16,
                recover: 0.45,
                cool: 9.0,
            },
            pistol: WeaponStats {
                name: "pistol".into(),
                damage: 30.0,
                armor_pen: 0.5,
                head_mult: 4.0,
                stomach_mult: 1.25,
                leg_mult: 0.75,
                range_mod: 0.79,
                interval: 0.15,
                auto: false,
                mag: 12,
                reserve: 24,
                reload: 2.2,
                range: 120.0,
                spread: 0.004,
                spread_move: 0.03,
                spread_air: 0.12,
                spread_fire: 0.012,
                crouch_mult: 0.8,
                kick: 0.02,
                kick_side: 0.25,
                kick_max: 0.1,
                recover: 0.6,
                cool: 6.0,
            },
            launcher: WeaponStats {
                name: "launcher".into(),
                damage: 0.0,
                armor_pen: 0.5,
                head_mult: 1.0,
                stomach_mult: 1.0,
                leg_mult: 1.0,
                range_mod: 1.0,
                interval: 0.9,
                auto: false,
                mag: 1,
                reserve: 3,
                reload: 2.5,
                range: 0.0,
                spread: 0.002,
                spread_move: 0.02,
                spread_air: 0.06,
                spread_fire: 0.0,
                crouch_mult: 0.8,
                kick: 0.05,
                kick_side: 0.2,
                kick_max: 0.1,
                recover: 0.35,
                cool: 4.0,
            },
            grenade: GrenadeStats {
                name: "grenade".into(),
                damage: 98.0,
                radius: 6.5,
                armor_pen: 0.5,
                fuse: 1.6,
                speed: 14.0,
                loft: 0.12,
                gravity: 8.4,
                restitution: 0.45,
                friction: 0.25,
                size: 0.06,
                carry: 1,
            },
            rocket: RocketStats {
                name: "rocket".into(),
                damage: 120.0,
                radius: 5.0,
                // Armour stops little of a rocket: a direct hit kills.
                armor_pen: 0.9,
                speed: 35.0,
                gravity: 2.5,
                size: 0.06,
                life: 4.0,
            },
            bot: BotTune {
                react: 0.45,
                aim_err: 0.035,
                aim_near: 12.0,
                settle: 3.0,
                settle_min: 0.35,
                scan: 0.35,
                scan_jitter: 0.2,
                turn: 9.0,
                fire_cone: 0.05,
                view: 70.0,
                fov: 1.3,
                hearing: 1.0,
                lost: 2.0,
                investigate: 7.0,
                retreat_hp: 25.0,
                retreat_time: 2.0,
                strafe: 0.8,
                burst_range: 18.0,
                burst: 3,
                burst_pause: 0.3,
                reach: 0.45,
            },
            sound: SoundTune {
                gunshot: 40.0,
                footstep: 9.0,
                blast: 55.0,
                bounce: 10.0,
            },
            armour: 100.0,
            helmet: true,
            health: 100.0,
            run_speed: 4.76,
            respawn: 3.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_numbers_are_sane() {
        let t = Tuning::default();
        // About 600 rounds a minute, a 30 round magazine, a one-tap rifle
        // headshot through a helmet.
        assert!((60.0 / t.rifle.interval - 600.0).abs() < 1.0);
        assert!(t.rifle.damage * t.rifle.head_mult * t.rifle.armor_pen >= 100.0);
        // A pistol headshot with no helmet kills; with one it does not.
        assert!(t.pistol.damage * t.pistol.head_mult >= 100.0);
        assert!(t.pistol.damage * t.pistol.head_mult * t.pistol.armor_pen < 100.0);
        // A grenade alone never kills a full-health body, armoured or not.
        assert!(t.grenade.damage < t.health);
        assert!((6.0..7.0).contains(&t.grenade.radius) && (1.4..2.0).contains(&t.grenade.fuse));
        // A direct rocket hit (on the body's skin, 0.2 m off its axis) kills
        // through armour, half a metre off it does not; it flies fast and
        // drops only a little; one loaded and a few spare.
        let at = |d: f32| t.rocket.damage * (1.0 - d / t.rocket.radius) * t.rocket.armor_pen;
        assert!(
            at(0.2) >= t.health && at(0.5) < t.health,
            "{} {}",
            at(0.2),
            at(0.5)
        );
        assert!(
            (30.0..=40.0).contains(&t.rocket.speed) && t.rocket.gravity < t.grenade.gravity * 0.5
        );
        assert!(t.launcher.mag == 1 && (2..=3).contains(&t.launcher.reserve) && !t.launcher.auto);
    }
}
