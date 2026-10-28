//! Tests across the collision module: tagged ray hits, filters, line of
//! sight, sphere casts, the live parameters and a whole-session
//! determinism check. Each file also has its own unit tests.

use super::*;
use crate::PhysicsModule;
use evani_core::App;
use evani_core::params::{ParamRegistry, ParamValue};

const FLOOR: u32 = 1;
const WALL: u32 = 2;
const GLASS: u32 = 3;
const CRATE: u32 = 4;
const ROCK: u32 = 5;

/// A small level: a floor, a wall, a glass pane, a turned crate and a
/// tri-mesh rock (a four-sided pyramid).
fn level() -> StaticWorld {
    let mut b = WorldBuilder::new();
    b.aabb([-20.0, -0.5, -20.0], [20.0, 0.0, 20.0], FLOOR);
    b.aabb([8.0, 0.0, -20.0], [8.5, 3.0, 20.0], WALL);
    b.aabb([4.0, 0.0, -1.0], [4.05, 2.5, 1.0], GLASS);
    let a = trig::radians(30.0);
    let (c, s) = (trig::cos(a), trig::sin(a));
    b.oriented_box([0.0, 0.5, 5.0], [0.5, 0.5, 0.5], [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]], CRATE);
    let pos = [[-6.0, 0.0, -1.0], [-4.0, 0.0, -1.0], [-4.0, 0.0, 1.0], [-6.0, 0.0, 1.0], [-5.0, 1.5, 0.0]];
    let idx = [0, 4, 1, 1, 4, 2, 2, 4, 3, 3, 4, 0];
    assert!(b.mesh(&pos, &idx, ROCK).is_ok());
    b.build()
}

#[test]
fn ray_hits_carry_the_surface_tag() {
    let w = level();
    let eye = [0.0, 1.5, 0.0];
    let at = |d: V3| w.raycast(eye, d, 100.0).map(|h| h.tag);
    assert_eq!(at([0.0, -1.0, 0.0]), Some(FLOOR));
    assert_eq!(at([1.0, 0.0, 0.0]), Some(GLASS));
    assert_eq!(at([-1.0, -0.2, 0.0]), Some(ROCK));
    assert_eq!(at([0.0, -0.2, 1.0]), Some(CRATE));
    assert_eq!(at([0.0, 1.0, 0.0]), None, "open sky");
    // Through the glass to the wall, with the hit point on the wall's face.
    let h = w.raycast_filtered(eye, [1.0, 0.0, 0.0], 100.0, |t| t != GLASS);
    let h = h.map(|h| (h.tag, h.point, h.normal));
    assert_eq!(h, Some((WALL, [8.0, 1.5, 0.0], [-1.0, 0.0, 0.0])));
    // The rock's slanted face: a normal that faces the ray and leans up.
    let r = w.raycast(eye, [-1.0, -0.2, 0.0], 100.0);
    assert!(r.is_some_and(|r| r.normal[0] > 0.5 && r.normal[1] > 0.3), "{r:?}");
}

#[test]
fn line_of_sight_and_through_glass() {
    let w = level();
    let a = [0.0, 1.5, 0.0];
    assert!(w.line_of_sight(a, [3.9, 1.5, 0.0]), "short of the glass");
    assert!(!w.line_of_sight(a, [6.0, 1.5, 0.0]), "the glass blocks");
    assert!(w.line_of_sight_filtered(a, [6.0, 1.5, 0.0], |t| t != GLASS), "seen through the glass");
    assert!(!w.line_of_sight_filtered(a, [9.0, 1.5, 0.0], |t| t != GLASS), "the wall still blocks");
    assert!(w.line_of_sight(a, [6.0, 3.5, 0.0]), "over the pane");
    assert!(w.line_of_sight(a, a), "a point sees itself");
}

#[test]
fn sphere_cast_stops_at_the_first_tag() {
    let w = level();
    // Thrown along +x at head height: stops a radius short of the glass.
    let h = w.cast_sphere([0.0, 1.5, 0.0], 0.1, [20.0, 0.0, 0.0]);
    assert!(h.is_some_and(|h| h.tag == GLASS && (h.distance - 3.9).abs() < 1e-3), "{h:?}");
    // Falling: lands on the floor, the contact normal straight up.
    let h = w.cast_sphere([2.0, 3.0, 2.0], 0.25, [0.0, -10.0, 0.0]);
    assert!(h.is_some_and(|h| h.tag == FLOOR && (h.distance - 2.75).abs() < 1e-3 && h.normal[1] > 0.999), "{h:?}");
    // Onto the rock's slanted face from above.
    let h = w.cast_sphere([-5.0, 4.0, 0.0], 0.2, [0.0, -5.0, 0.0]);
    assert!(h.is_some_and(|h| h.tag == ROCK), "{h:?}");
    // A clear path: no hit.
    assert_eq!(w.cast_sphere([0.0, 1.5, -3.0], 0.1, [3.0, 0.0, 0.0]), None);
}

#[test]
fn mover_walks_the_level() {
    // The wall stops it and the floor holds it.
    let w = level();
    let cfg = CharacterConfig::default();
    let mut feet = [5.0, 0.0, 3.0];
    let mut r = w.move_character(feet, 1.8, [0.0; 3], &cfg, true);
    for _ in 0..90 {
        r = w.move_character(feet, 1.8, [3.0 / 60.0, 0.0, 0.0], &cfg, true);
        feet = r.feet;
    }
    assert!(r.grounded && r.hit_wall && !r.stepped, "{r:?}");
    assert!((feet[0] - (8.0 - cfg.radius - cfg.skin)).abs() < 2e-3, "{feet:?}");
    // The 1 m crate is too tall to step: walking at it, the body slides
    // round it on the floor and never climbs it or sinks into it.
    let mut feet = [0.0, 0.0, 3.0];
    for _ in 0..90 {
        let r = w.move_character(feet, 1.8, [0.0, 0.0, 3.0 / 60.0], &cfg, true);
        feet = r.feet;
        assert!(r.grounded && feet[1].abs() < 1e-3 && !r.stepped, "{r:?}");
        assert!(!w.overlaps(&cfg.capsule(feet, 1.8)), "{feet:?}");
    }
}

#[test]
fn params_register_and_drive_the_config_live() {
    let mut a = App::new("t").with_module(PhysicsModule::new());
    a.run_headless(1, 16.0);
    let p = *a.world.res::<CollideParams>();
    let names = [
        "physics.collide.radius",
        "physics.collide.height",
        "physics.collide.step_height",
        "physics.collide.max_slope",
        "physics.collide.skin",
        "physics.collide.snap_distance",
        "physics.collide.max_slides",
        "physics.collide.depenetrate_passes",
        "physics.collide.speed",
    ];
    {
        let reg = a.world.res::<ParamRegistry>();
        for n in names {
            assert!(reg.id(n).is_some(), "{n}");
        }
        // The defaults are the generic body.
        assert_eq!(p.config(&reg), CharacterConfig::default());
        assert_eq!((p.height(&reg), p.speed(&reg)), (1.8, 4.5));
    }
    // A live change (as `param_set` makes it) shows in the next config.
    {
        let mut reg = a.world.res_mut::<ParamRegistry>();
        for (n, v) in [("physics.collide.step_height", ParamValue::F32(0.5)), ("physics.collide.max_slope", ParamValue::F32(30.0))] {
            let id = reg.id(n);
            assert!(id.is_some_and(|id| reg.set(id, v).is_ok()), "{n}");
        }
    }
    let reg = a.world.res::<ParamRegistry>();
    let c = p.config(&reg);
    assert_eq!(c.step_height, 0.5);
    assert!((c.min_ground_normal_y - 0.866_025_4).abs() < 2e-6);
    // The overlay says the feature is in.
    let mut s = String::new();
    evani_core::plugin::EngineModule::status(&PhysicsModule::new(), &a.world, &mut s);
    assert!(s.contains("collide"), "{s}");
}

/// FNV-1a over the bits of a session's results.
fn fold(h: &mut u64, x: f32) {
    for byte in x.to_bits().to_le_bytes() {
        *h ^= u64::from(byte);
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
}

/// A scripted session: a fan of rays, a fan of sphere casts and a walk of
/// 300 fixed steps with gravity, steering by the deterministic trig.
fn session() -> u64 {
    let w = level();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for k in 0..48 {
        let a = trig::radians(k as f32 * 7.5);
        let d = [trig::cos(a), -0.1, trig::sin(a)];
        if let Some(r) = w.raycast([0.3, 1.2, -0.2], d, 50.0) {
            fold(&mut h, r.t);
            r.point.iter().chain(r.normal.iter()).for_each(|&x| fold(&mut h, x));
        }
        if let Some(c) = w.cast_sphere([0.3, 1.2, -0.2], 0.15, [d[0] * 30.0, d[1] * 30.0, d[2] * 30.0]) {
            fold(&mut h, c.fraction);
            c.normal.iter().for_each(|&x| fold(&mut h, x));
        }
    }
    let cfg = CharacterConfig::default();
    let (mut feet, mut vy, mut grounded) = ([0.0, 0.0, -3.0], 0.0f32, true);
    for step in 0..300 {
        let yaw = trig::radians(step as f32 * 1.7);
        vy = if grounded { if step % 90 == 45 { 4.0 } else { 0.0 } } else { vy - 9.81 / 60.0 };
        let m = [trig::cos(yaw) * 4.5 / 60.0, vy / 60.0, trig::sin(yaw) * 4.5 / 60.0];
        let r = w.move_character(feet, 1.8, m, &cfg, grounded && vy <= 0.0);
        feet = r.feet;
        grounded = r.grounded;
        feet.iter().for_each(|&x| fold(&mut h, x));
    }
    h
}

#[test]
fn whole_session_is_bit_identical() {
    let first = session();
    for _ in 0..3 {
        assert_eq!(session(), first);
    }
    assert_eq!(level(), level(), "the build is deterministic");
}
