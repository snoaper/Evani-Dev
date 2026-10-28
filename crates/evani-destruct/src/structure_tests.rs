use super::*;

/// An 8 x 6 m concrete deck at y = 3 with a metal rim, the ground at y = 0,
/// a 4 x 1 x 3 wall (x 1..5, z 1..2) and a 1 x 1 x 3 pillar (x 5..6, z 3..4) on it.
fn deck() -> Structure {
    let (nx, nz) = (16, 12);
    let mut f = FloorGrid::filled(nx, nz, 0.5, FloorKind::Concrete);
    for a in 0..nx {
        for b in 0..nz {
            if a == 0 || b == 0 || a == nx - 1 || b == nz - 1 {
                f.set(a, b, FloorKind::Metal);
            }
        }
    }
    let mut cells = Vec::new();
    for u in 1..5 {
        for y in 0..3 {
            cells.push([u, 1, y]);
        }
    }
    for y in 0..3 {
        cells.push([5, 3, y]);
    }
    Structure::new(
        StructureDesc {
            origin: [0.0, 3.0, 0.0],
            slab: 0.5,
            floor: Some(f),
            cells,
            ground_y: Some(0.0),
            hub: [4.0, 3.0],
        },
        &Tuning::default(),
    )
}

fn run(s: &mut Structure, secs: f32, rng: &mut Pcg32, ev: &mut Vec<DestructEvent>) {
    let t = Tuning::default();
    for _ in 0..(secs * 60.0) as u32 {
        s.step(1.0 / 60.0, &t, rng, ev);
    }
}

#[test]
fn cell_mass_counts_its_27_samples() {
    let mut s = deck();
    assert_eq!(s.cell_mass(0), 1.0);
    let c = s.cell_corner(0);
    s.shoot([c[0] + 0.5, c[1] + 0.5, c[2] + 0.5], 0.2, 0);
    assert!((s.cell_mass(0) - 26.0 / 27.0).abs() < 1e-6);
}

#[test]
fn an_untouched_structure_stands() {
    let mut s = deck();
    let mut rng = Pcg32::seeded(1);
    let mut ev = Vec::new();
    s.shoot([7.5, 3.0, 5.5], 0.12, 0); // a bullet hole far from everything
    run(&mut s, 1.0, &mut rng, &mut ev);
    assert!(ev.is_empty(), "{ev:?}");
    assert_eq!(s.standing(), 15);
}

#[test]
fn a_wall_tips_and_falls_when_its_footing_is_blasted() {
    let mut s = deck();
    let mut rng = Pcg32::seeded(2);
    let mut ev = Vec::new();
    for x in [1.5, 2.5, 3.5] {
        s.blast([x, 3.0, 1.5], 0.6, 0);
    }
    run(&mut s, 0.1, &mut rng, &mut ev);
    let fall = ev.iter().find_map(|e| match e {
        DestructEvent::Fall { cells, .. } => Some(*cells),
        _ => None,
    });
    assert_eq!(fall, Some(12), "{ev:?}");
    assert_eq!(s.standing(), 3, "only the pillar stands");
    // It turns on its ledge as it goes (it slumps into the trench about 0.4 s in).
    run(&mut s, 0.15, &mut rng, &mut ev);
    assert!(s.falls().first().is_some_and(|f| f.tilt() > 0.05));
    run(&mut s, 3.0, &mut rng, &mut ev);
    assert!(
        ev.iter().any(|e| matches!(e, DestructEvent::Land { .. })),
        "{ev:?}"
    );
    assert!(s.falls().is_empty());
}

/// Cut a ring of holes round the 3 x 2.5 m patch x 4..7, z 2.5..5 (the
/// pillar stands inside it). `keep` leaves the floor on the ring's +x side
/// whole where it says so.
fn ring(s: &mut Structure, keep: impl Fn(f32) -> bool) {
    for k in 0..=15 {
        let a = 4.0 + 0.2 * k as f32;
        s.blast([a, 2.75, 2.5], 0.45, 0);
        s.blast([a, 2.75, 5.0], 0.45, 0);
    }
    for k in 0..=10 {
        let b = 2.5 + 0.25 * k as f32;
        s.blast([4.0, 2.75, b], 0.45, 0);
        if !keep(b) {
            s.blast([7.0, 2.75, b], 0.45, 0);
        }
    }
}

/// Cut the patch free all round.
fn sever(s: &mut Structure) {
    ring(s, |_| false);
}

#[test]
fn a_floor_island_drops_with_the_pillar_riding_it() {
    let mut s = deck();
    let mut rng = Pcg32::seeded(3);
    let mut ev = Vec::new();
    let before = s.floor_left();
    sever(&mut s);
    run(&mut s, 0.1, &mut rng, &mut ev);
    let fall = ev.iter().find_map(|e| match e {
        DestructEvent::Fall { cells, samples, .. } => Some((*cells, *samples)),
        _ => None,
    });
    let Some((cells, samples)) = fall else {
        panic!("no fall: {ev:?}")
    };
    assert_eq!(cells, 3, "the pillar rides the slab");
    assert!(samples >= 4, "{samples}");
    assert!(s.floor_left() < before);
    assert_eq!(s.standing(), 12, "the wall still stands");
    run(&mut s, 2.0, &mut rng, &mut ev);
    assert!(
        ev.iter().any(|e| matches!(e, DestructEvent::Land { .. })),
        "{ev:?}"
    );
}

#[test]
fn a_thin_sliver_carries_nothing() {
    let mut s = deck();
    let mut rng = Pcg32::seeded(4);
    let mut ev = Vec::new();
    // Cut round the patch but leave a join one sample (0.5 m) wide on its
    // +x side: the floor row at z = 3.75 still reaches the metal rim.
    ring(&mut s, |b| (3.3..4.2).contains(&b));
    assert!(
        s.solid([7.25, 2.75, 3.75]) && s.solid([6.75, 2.75, 3.75]),
        "the join is whole"
    );
    assert!(
        !s.solid([6.75, 2.75, 3.25]) && !s.solid([6.75, 2.75, 4.25]),
        "and one sample wide"
    );
    // With joins down to 0.4 m counted, the same cut holds.
    let mut held = s.clone();
    let loose = Tuning {
        floor_sliver_min: 0.4,
        ..Tuning::default()
    };
    for _ in 0..6 {
        held.step(1.0 / 60.0, &loose, &mut rng, &mut ev);
    }
    assert!(ev.is_empty(), "{ev:?}");
    run(&mut s, 0.1, &mut rng, &mut ev);
    assert!(
        ev.iter()
            .any(|e| matches!(e, DestructEvent::Fall { samples, .. } if *samples > 0)),
        "{ev:?}"
    );
}

#[test]
fn same_hits_same_collapse() {
    let go = || {
        let mut s = deck();
        let mut rng = Pcg32::seeded(5);
        let mut ev = Vec::new();
        for x in [1.5, 2.5, 3.5] {
            s.blast([x, 3.0, 1.5], 0.6, 0);
        }
        sever(&mut s);
        run(&mut s, 3.0, &mut rng, &mut ev);
        (ev, s.holes().holes().to_vec())
    };
    assert_eq!(go(), go());
}

#[test]
fn fall_max_makes_the_rest_crumble() {
    let mut s = deck();
    let mut rng = Pcg32::seeded(6);
    let mut ev = Vec::new();
    let t = Tuning {
        fall_max: 1,
        ..Tuning::default()
    };
    for x in [1.5, 2.5, 3.5] {
        s.blast([x, 3.0, 1.5], 0.6, 0);
    }
    sever(&mut s);
    s.step(1.0 / 60.0, &t, &mut rng, &mut ev);
    assert!(
        ev.iter()
            .any(|e| matches!(e, DestructEvent::Crumble { .. })),
        "{ev:?}"
    );
    assert_eq!(s.falls().len(), 1);
}
