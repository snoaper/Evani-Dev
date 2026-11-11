use evani_core::params::{ParamRegistry, ParamValue, is_valid_param_name};

use crate::placeholder::{self, Hint};
use crate::*;

const HINTS: [Hint; 10] = [Hint::Block, Hint::Ball, Hint::Blob, Hint::Post, Hint::Cone, Hint::Slab, Hint::Ring, Hint::Long, Hint::Tree, Hint::Scatter];

fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_category(Category { id: "rocks", name: "Rocks" });
    assert!(r.add(Box::new(Placeholder::new("rocks", "medium-rocks", "Medium rocks", Hint::Blob, [0.3, 0.3, 0.3]))));
    assert!(!r.add(Box::new(Placeholder::new("rocks", "medium-rocks", "Twice", Hint::Blob, [0.3; 3]))), "a duplicate is refused");
    assert!(!r.add(Box::new(Placeholder::new("nope", "x", "Unknown category", Hint::Blob, [0.3; 3]))), "an unknown category is refused");
    r
}

fn valid(m: &geo::Mesh) {
    assert!(!m.indices.is_empty());
    assert_eq!(m.indices.len() % 3, 0);
    assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
    assert!(m.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())));
}

#[test]
fn every_hint_makes_a_valid_mesh_standing_on_the_ground() {
    for h in HINTS {
        let m = placeholder::shape(h, 2.0, 7);
        valid(&m);
        let (lo, hi) = geo::bounds(&m);
        assert!(lo[1] > -1e-3, "{h:?} base at y = 0 (min {})", lo[1]);
        assert!(hi[1] > 0.05, "{h:?} has height");
    }
}

#[test]
fn params_are_live_names_under_the_asset() {
    let r = registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    r.register_params(&mut reg).unwrap();
    let names = r.param_names("rocks-medium-rocks");
    assert_eq!(names, ["procedural.rocks.medium_rocks.on", "procedural.rocks.medium_rocks.seed", "procedural.rocks.medium_rocks.size", "procedural.rocks.medium_rocks.color"]);
    for n in &names {
        assert!(is_valid_param_name(n), "{n}");
        assert!(reg.id(n).is_some(), "{n} registered");
    }
}

#[test]
fn generation_is_deterministic_and_follows_the_live_values() {
    let r = registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    let a = r.generate(&reg, "rocks-medium-rocks", REALISM).unwrap();
    let b = r.generate(&reg, "rocks-medium-rocks", REALISM).unwrap();
    assert_eq!(a, b);
    reg.set_by_name("procedural.rocks.medium_rocks.seed", ParamValue::F32(42.0)).unwrap();
    let c = r.generate(&reg, "rocks-medium-rocks", REALISM).unwrap();
    assert_ne!(a.output, c.output, "another seed, another rock");
    reg.set_by_name("procedural.rocks.medium_rocks.size", ParamValue::F32(5.0)).unwrap();
    let d = r.generate(&reg, "rocks-medium-rocks", REALISM).unwrap();
    let (_, hi) = geo::bounds(d.output.mesh().unwrap());
    assert!(hi[1] > 3.0, "the size param reaches the mesh");
}

#[test]
fn another_style_falls_back_to_realism() {
    let r = registry();
    let reg = ParamRegistry::default();
    let g = r.generate(&reg, "rocks-medium-rocks", "WK08").unwrap();
    assert_eq!((g.style, g.fallback), (REALISM, true));
    let g = r.generate(&reg, "rocks-medium-rocks", REALISM).unwrap();
    assert_eq!((g.style, g.fallback), (REALISM, false));
    assert_eq!(STYLES.len(), 24);
    assert!(style::is_style("WK24"));
}

#[test]
fn the_switch_turns_an_asset_off() {
    let r = registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    assert!(r.is_on(&reg, "rocks-medium-rocks"));
    reg.set_by_name("procedural.rocks.medium_rocks.on", ParamValue::Bool(false)).unwrap();
    assert!(!r.is_on(&reg, "rocks-medium-rocks"));
    assert!(!r.is_on(&reg, "rocks-unknown"));
}

#[test]
fn helpers_make_valid_meshes() {
    valid(&geo::height_grid([10.0, 10.0], 8, |x, z| (x * 0.3).sin() + z * 0.1));
    valid(&geo::segmented_box([1.0, 2.0, 3.0], 3));
    valid(&geo::lathe(&[[0.5, 0.0], [0.3, 1.0], [0.1, 2.0]], 12));
    let g = geo::height_grid([10.0, 10.0], 4, |_, _| 0.0);
    assert!(g.vertices.iter().all(|v| v[4] > 0.99), "a flat grid faces up");
    let f = noise::fbm2(1.3, 2.7, 9, 5, 2.0, 0.5);
    assert!((-1.0..=1.0).contains(&f));
    assert_eq!(f, noise::fbm2(1.3, 2.7, 9, 5, 2.0, 0.5));
}
