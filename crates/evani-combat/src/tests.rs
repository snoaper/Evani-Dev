//! Crate-level tests: angles, the live `combat.*` parameters, and a whole
//! seeded fight replayed bit for bit.

use super::*;
use evani_core::App;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::rng::Pcg32;
use evani_physics::collide::{StaticWorld, WorldBuilder};

#[test]
fn yaw_pitch_round_trips() {
    for &(y, p) in &[(0.3f32, 0.2f32), (-2.0, -0.4), (3.0, 1.0)] {
        let (y2, p2) = yaw_pitch_of(look_dir(y, p));
        assert!((wrap(y2 - y)).abs() < 1e-4 && (p2 - p).abs() < 1e-4);
    }
}

#[test]
fn params_register_and_drive_the_table_live() {
    let mut a = App::new("t").with_module(CombatModule::new());
    a.run_headless(1, 16.0);
    let p = *a.world.res::<CombatParams>();
    {
        let reg = a.world.res::<ParamRegistry>();
        let ours: Vec<&str> = reg
            .defs()
            .iter()
            .filter(|d| d.name.starts_with("combat."))
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(ours.len(), 114, "{ours:?}");
        assert!(
            reg.defs()
                .iter()
                .filter(|d| d.name.starts_with("combat."))
                .all(|d| d.owner == "combat")
        );
        for n in [
            "combat.rifle.damage",
            "combat.pistol.auto",
            "combat.launcher.mag",
            "combat.grenade.carry",
            "combat.rocket.life",
            "combat.bot.react",
            "combat.bot.burst",
            "combat.sound.gunshot",
            "combat.health",
            "combat.helmet",
        ] {
            assert!(reg.id(n).is_some(), "{n}");
        }
        // The defaults are the default table.
        assert_eq!(p.tuning(&reg), Tuning::default());
    }
    // A live change (as `param_set` makes it) shows in the next read.
    {
        let mut reg = a.world.res_mut::<ParamRegistry>();
        for (n, v) in [
            ("combat.rifle.damage", ParamValue::F32(50.0)),
            ("combat.bot.react", ParamValue::F32(0.75)),
            ("combat.pistol.mag", ParamValue::I32(7)),
            ("combat.helmet", ParamValue::Bool(false)),
        ] {
            let id = reg.id(n);
            assert!(id.is_some_and(|id| reg.set(id, v).is_ok()), "{n}");
        }
    }
    let reg = a.world.res::<ParamRegistry>();
    let mut t = Tuning::default();
    p.apply(&reg, &mut t);
    assert_eq!(t.rifle.damage, 50.0);
    assert_eq!(t.bot.react, 0.75);
    assert_eq!(t.pistol.mag, 7);
    assert!(!t.helmet);
    // Names are left alone.
    assert_eq!(t.rifle.name, Tuning::default().rifle.name);
    let mut s = String::new();
    evani_core::plugin::EngineModule::status(&CombatModule::new(), &a.world, &mut s);
    assert!(s.contains("bots"), "{s}");
}

struct Straight;
impl Router for Straight {
    fn route(&self, _from: V3, to: V3) -> Option<Vec<V3>> {
        Some(vec![to])
    }
}

fn arena() -> StaticWorld {
    let mut b = WorldBuilder::new();
    b.aabb([-30.0, -0.2, -30.0], [30.0, 0.0, 30.0], 0);
    // A low crate beside the line of fire: in the world, never in the way.
    b.aabb([2.0, 0.0, 5.0], [3.0, 1.0, 6.0], 1);
    b.build()
}

/// What a fight left behind: every shot's direction bits, the target's
/// health bits, and the step the target died on.
#[derive(Debug, PartialEq)]
struct Record {
    shots: Vec<[u32; 3]>,
    hp: u32,
    armour: u32,
    died: Option<u32>,
}

/// A bot with a rifle against a still target 14 m ahead, `steps` fixed
/// 60 Hz steps, seeded by `seed`.
fn fight(seed: u64, steps: u32) -> Record {
    let world = arena();
    let t = Tuning::default();
    let dt = 1.0 / 60.0;
    let mut brain = Brain::new(t.bot.clone(), seed);
    brain.set_orders(Orders {
        goal: [0.0, 0.0, 0.0],
        watch: None,
    });
    let mut gun = Gun::new(t.rifle.clone());
    let mut rng = Pcg32::seeded(seed ^ 0x5eed);
    let mut target = Health::new(t.health, t.armour, t.helmet);
    let mut foes = [
        Foe {
            body: Body::new([0.0, 0.0, 0.0], 1.8, 1.6),
            team: 0,
        },
        Foe {
            body: Body::new([0.5, 0.0, 14.0], 1.8, 1.6),
            team: 1,
        },
    ];
    let (mut yaw, mut pitch) = (0.6f32, 0.0f32);
    let mut rec = Record {
        shots: Vec::new(),
        hp: 0,
        armour: 0,
        died: None,
    };
    for step in 0..steps {
        let back = gun.tick(dt);
        yaw = wrap(yaw + back[0]);
        pitch += back[1];
        let me = foes[0].body;
        let inp = BotIn {
            world: &world,
            me: 0,
            team: 0,
            feet: me.feet,
            eye: [me.feet[0], me.eye, me.feet[2]],
            yaw,
            pitch,
            hp: t.health,
            mag: gun.mag,
            reloading: gun.reloading(),
            mag_frac: gun.mag as f32 / gun.stats.mag as f32,
            interval: gun.stats.interval,
            others: &foes,
            sounds: &[],
            hurt_by: None,
            dt,
        };
        let out = brain.think(&inp, &Straight);
        yaw = wrap(yaw + out.dyaw);
        pitch += out.dpitch;
        if out.reload {
            gun.start_reload();
        }
        if let Some(shot) = gun.fire(
            out.fire,
            yaw,
            pitch,
            Stance::default(),
            t.run_speed,
            &mut rng,
        ) {
            yaw = wrap(yaw + shot.kick[0]);
            pitch += shot.kick[1];
            rec.shots.push(shot.dir.map(f32::to_bits));
            let bodies = foes.map(|f| f.body);
            if let Some(Hit::Body {
                index: 1,
                t: d,
                zone,
                ..
            }) = trace(&world, inp.eye, shot.dir, gun.stats.range, &bodies, Some(0))
            {
                let dmg = damage_at(&gun.stats, d) * zone.mult(&gun.stats);
                if target.take(dmg, gun.stats.armor_pen, zone).killed {
                    rec.died = Some(step);
                    foes[1].body.alive = false;
                }
            }
        }
    }
    rec.hp = target.hp.to_bits();
    rec.armour = target.armour.to_bits();
    rec
}

#[test]
fn a_seeded_fight_replays_bit_for_bit_and_the_bot_wins() {
    let a = fight(7, 600);
    let b = fight(7, 600);
    assert_eq!(a, b, "same seed, same fight");
    assert!(!a.shots.is_empty(), "the bot fired");
    let died = a.died.unwrap_or(u32::MAX);
    assert!(died < 600, "the target died in ten seconds: {a:?}");
    // The first shot waits for the reaction delay.
    assert!(died as f32 / 60.0 >= Tuning::default().bot.react, "{died}");
    // Another seed is another fight (aim error and spread differ).
    let c = fight(8, 600);
    assert_ne!(a.shots, c.shots);
}
