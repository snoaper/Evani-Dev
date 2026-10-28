use evani_model::dmath;
use evani_model::mesh::Mesh;
use evani_model::vec::{V3, add, dot, len, mul, sub};

use crate::*;

/// A UV sphere with outward normals; every triangle in part `part`.
fn sphere(c: V3, r: f64, rings: usize, segs: usize, part: u32) -> Mesh {
    let mut m = Mesh::default();
    for i in 0..=rings {
        let (st, ct) = dmath::sin_cos(std::f64::consts::PI * i as f64 / rings as f64);
        for j in 0..segs {
            let (sp, cp) = dmath::sin_cos(2.0 * std::f64::consts::PI * j as f64 / segs as f64);
            let n = [st * cp, ct, st * sp];
            m.pos.push(add(c, mul(n, r)));
            m.nrm.push(n);
        }
    }
    let id = |i: usize, j: usize| (i * segs + j % segs) as u32;
    for i in 0..rings {
        for j in 0..segs {
            let (a, b, c2, d) = (id(i, j), id(i, j + 1), id(i + 1, j), id(i + 1, j + 1));
            m.tris.push([a, b, d]);
            m.tris.push([a, d, c2]);
            m.tri_part.push(part);
            m.tri_part.push(part);
        }
    }
    // Outward winding check: flip if the first sizeable face points inwards.
    let t = m.tris[segs * 2 + 1];
    let f = evani_model::vec::cross(sub(m.pos[t[1] as usize], m.pos[t[0] as usize]), sub(m.pos[t[2] as usize], m.pos[t[0] as usize]));
    if dot(f, sub(m.pos[t[0] as usize], c)) < 0.0 {
        for t in &mut m.tris {
            t.swap(1, 2);
        }
    }
    m
}

/// An open band (a sleeve) round the y axis from `y0` to `y1`.
fn band(r: f64, y0: f64, y1: f64, segs: usize, part: u32) -> Mesh {
    let mut m = Mesh::default();
    for y in [y0, y1] {
        for j in 0..segs {
            let (s, c) = dmath::sin_cos(2.0 * std::f64::consts::PI * j as f64 / segs as f64);
            m.pos.push([r * c, y, r * s]);
            m.nrm.push([c, 0.0, s]);
        }
    }
    for j in 0..segs {
        let (a, b) = (j as u32, ((j + 1) % segs) as u32);
        let (c, d) = (a + segs as u32, b + segs as u32);
        m.tris.push([a, c, d]);
        m.tris.push([a, d, b]);
        m.tri_part.push(part);
        m.tri_part.push(part);
    }
    m
}

fn names(list: &'static [&'static str]) -> impl Fn(u32) -> String {
    move |p| list.get(p as usize).copied().unwrap_or("").to_string()
}

/// A furry ball of radius 0.3 m with a gear band round its middle.
fn ball_with_sleeve() -> FurSurface {
    let mut s = FurSurface::new();
    s.add_mesh(&sphere([0.0; 3], 0.3, 40, 64, 0), "body", [0.0; 3], false, &names(&["fur"]));
    s.add_mesh(&band(0.303, -0.06, 0.06, 64, 1), "body", [0.0; 3], false, &names(&["fur", "shirt"]));
    s
}

fn quiet() -> LockParams {
    let mut p = LockParams::default();
    if let Err(e) = p.set("kit.fur_locks.length_jitter", 0.0) {
        panic!("{e}");
    }
    p
}

fn bits(f: &FurLocks) -> Vec<u64> {
    let m = &f.solid.mesh;
    let mut v: Vec<u64> = m.pos.iter().chain(m.nrm.iter()).chain(f.strand.iter()).flat_map(|p| p.map(f64::to_bits)).collect();
    v.extend(f.uv.iter().flat_map(|u| u.map(f64::to_bits)));
    v.extend(m.tris.iter().flat_map(|t| t.map(u64::from)));
    v.extend(m.tri_part.iter().map(|&p| u64::from(p)));
    v
}

#[test]
fn same_seed_same_bytes_new_seed_new_locks() {
    let s = ball_with_sleeve();
    let p = LockParams::default();
    let a = fur_locks(&s, &p).unwrap();
    let b = fur_locks(&s, &p).unwrap();
    assert!(a.stats.locks > 1000, "{:?}", a.stats);
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(a.locks, b.locks);
    let mut q = p.clone();
    q.set("kit.fur_locks.seed", 7.0).unwrap();
    let c = fur_locks(&s, &q).unwrap();
    assert_ne!(bits(&a), bits(&c));
}

#[test]
fn never_over_the_triangle_cap() {
    let mut big = FurSurface::new();
    big.add_mesh(&sphere([0.0; 3], 0.6, 48, 96, 0), "body", [0.0; 3], false, &names(&["fur"]));
    for (segs, per) in [(3.0, 6), (4.0, 8), (5.0, 10)] {
        let mut p = LockParams::default();
        p.set("kit.fur_locks.segments", segs).unwrap();
        let f = fur_locks(&big, &p).unwrap();
        let cap = MAX_LOCKS.min(MAX_TRIS / per);
        assert_eq!(f.stats.wanted, cap);
        assert!(f.stats.locks <= cap && f.stats.locks * 10 >= cap * 9, "{segs}: {:?}", f.stats);
        assert_eq!(f.stats.triangles, f.stats.locks * per);
        assert!(f.stats.triangles <= MAX_TRIS);
        assert_eq!(f.uv.len(), f.solid.mesh.pos.len());
        assert_eq!(f.strand.len(), f.solid.mesh.pos.len());
        assert_eq!(f.solid.mesh.tri_part.len(), f.solid.mesh.tris.len());
    }
    // The default is 1,750 locks of 8 triangles: exactly the cap.
    let f = fur_locks(&big, &LockParams::default()).unwrap();
    assert_eq!(f.stats.locks, 1_750);
    assert_eq!(f.stats.triangles, 14_000);
}

#[test]
fn the_solid_is_fur_locks_and_well_formed() {
    let s = ball_with_sleeve();
    let f = fur_locks(&s, &LockParams::default()).unwrap();
    assert_eq!(f.solid.solid, "fur_locks");
    assert_eq!(f.solid.instances.len(), 1);
    assert!(!f.solid.instances[0].mirror);
    assert_eq!(f.solid.report.triangles, f.stats.triangles);
    // One sheet per lock, no degenerate triangles.
    assert_eq!(f.solid.report.components, f.stats.locks);
    assert_eq!(f.solid.report.count(evani_model::meshcheck::Kind::Degenerate), 0, "{:?}", f.solid.report.summary());
    for n in &f.solid.mesh.nrm {
        assert!((len(*n) - 1.0).abs() < 1e-9);
    }
    // Every lock rises off its root's skin, root to tip (4 segments: 10
    // vertices a lock, the tip pair last).
    let per = 2 * (4 + 1);
    for (i, l) in f.locks.iter().enumerate() {
        let tip = f.solid.mesh.pos[i * per + per - 1];
        assert!(dot(sub(tip, l.root), l.normal) > 0.0, "lock {i}");
    }
    for u in &f.uv {
        assert!((0.0..=1.0).contains(&u[0]) && (0.0..=1.0).contains(&u[1]));
    }
}

#[test]
fn gear_doubles_the_length_where_fur_meets_it_and_covers_fur_under_it() {
    let s = ball_with_sleeve();
    let p = quiet();
    let f = fur_locks(&s, &p).unwrap();
    let l0 = 0.040;
    assert!(f.stats.covered > 0, "{:?}", f.stats);
    assert!(f.stats.near_gear > 20, "{:?}", f.stats);
    // Locks above the sleeve droop onto it and are stopped short of it:
    // no lock vertex ends up inside the sleeve's radius within its height.
    assert!(f.stats.clipped > 0, "{:?}", f.stats);
    for p in &f.solid.mesh.pos {
        let r = (p[0] * p[0] + p[2] * p[2]).sqrt();
        assert!(!(p[1].abs() < 0.055 && r > 0.304), "lock vertex through the sleeve at {p:?}");
    }
    let mut longest: f64 = 0.0;
    for l in &f.locks {
        // No roots under the sleeve (it covers |y| < 0.06 on the ball).
        assert!(l.root[1].abs() > 0.045, "root under the sleeve at {:?}", l.root);
        assert!((l.length - l0 * (1.0 + l.gear)).abs() < 1e-12, "{l:?}");
        if l.root[1].abs() > 0.06 + 0.031 {
            assert_eq!(l.gear, 0.0);
            assert!((l.length - l0).abs() < 1e-12);
        }
        longest = longest.max(l.length);
    }
    // Roots are scattered ~14 mm apart, so the nearest one sits a few mm
    // off the cuff: its mask is high but not quite 1 (the formula above
    // reaches exactly 2x at contact).
    assert!(longest > 1.5 * l0, "longest {longest}");
    // A boost of 1 turns the mask off.
    let mut q = quiet();
    q.set("kit.fur_locks.gear_boost", 1.0).unwrap();
    let g = fur_locks(&s, &q).unwrap();
    assert!(g.locks.iter().all(|l| (l.length - l0).abs() < 1e-12));
}

#[test]
fn length_per_region_and_per_solid() {
    let mut s = FurSurface::new();
    s.add_mesh(&sphere([0.0; 3], 0.2, 24, 48, 0), "body", [0.0; 3], false, &names(&["fur"]));
    s.add_mesh(&sphere([0.0, 0.6, 0.0], 0.12, 24, 48, 0), "head", [0.0; 3], false, &names(&["face_fur"]));
    s.add_mesh(&sphere([0.6, 0.0, 0.0], 0.15, 24, 48, 0), "arm", [0.0; 3], false, &names(&["fur"]));
    s.add_mesh(&sphere([-0.6, 0.0, 0.0], 0.1, 16, 32, 0), "cheek", [0.0; 3], false, &names(&["face"]));
    let mut p = quiet();
    p.set_region(RegionFur::new("arm.fur", 75.0));
    let f = fur_locks(&s, &p).unwrap();
    let mut seen = [0usize; 3];
    for l in &f.locks {
        let solid = s.solid_of(l.tri as usize);
        let want = match solid {
            "body" => 0.040,
            "head" => 0.018,
            "arm" => 0.075,
            other => panic!("lock on {other}"),
        };
        seen[["body", "head", "arm"].iter().position(|x| *x == solid).unwrap_or(0)] += 1;
        // Bare skin is not gear: no boost anywhere here.
        assert_eq!(l.gear, 0.0);
        assert!((l.length - want).abs() < 1e-12, "{solid}: {}", l.length);
    }
    assert!(seen.iter().all(|&n| n > 20), "{seen:?}");
    // A density of 0 grows nothing there.
    p.set_region(RegionFur::new("face_fur", 18.0).density(0.0));
    let g = fur_locks(&s, &p).unwrap();
    assert!(g.locks.iter().all(|l| s.solid_of(l.tri as usize) != "head"));
}

#[test]
fn droop_lowers_tips_and_clumping_gathers_them() {
    let s = ball_with_sleeve();
    let tips = |p: &LockParams| -> (f64, f64) {
        let f = fur_locks(&s, p).unwrap();
        // Two vertices per point, segments + 1 points.
        let per = 2 * (4 + 1);
        let tip = |i: usize| mul(add(f.solid.mesh.pos[i * per + per - 1], f.solid.mesh.pos[i * per + per - 2]), 0.5);
        let mut drop = 0.0;
        let mut gap = 0.0;
        let mut n: f64 = 0.0;
        for (i, l) in f.locks.iter().enumerate() {
            drop += l.root[1] - tip(i)[1];
            if l.leader as usize != i {
                gap += len(sub(tip(i), tip(l.leader as usize)));
                n += 1.0;
            }
        }
        (drop / f.locks.len() as f64, gap / n.max(1.0))
    };
    let mut flat = quiet();
    flat.set("kit.fur_locks.droop", 0.0).unwrap();
    flat.set("kit.fur_locks.clump", 0.0).unwrap();
    let mut heavy = flat.clone();
    heavy.set("kit.fur_locks.droop", 1.5).unwrap();
    heavy.set("kit.fur_locks.clump", 0.9).unwrap();
    let (d0, g0) = tips(&flat);
    let (d1, g1) = tips(&heavy);
    assert!(d1 > d0 + 0.004, "droop {d0} -> {d1}");
    assert!(g1 < 0.6 * g0, "clump gap {g0} -> {g1}");
}

#[test]
fn bad_input_is_an_error_and_no_fur_is_empty() {
    let mut s = ball_with_sleeve();
    let p = LockParams::default();
    let mut broken = s.clone();
    broken.tri_region.pop();
    assert!(fur_locks(&broken, &p).is_err());
    let mut broken = s.clone();
    broken.tris[0][0] = 1_000_000;
    assert!(fur_locks(&broken, &p).is_err());
    let mut broken = s.clone();
    broken.pos[0][0] = f64::NAN;
    assert!(fur_locks(&broken, &p).is_err());
    let mut bad = p.clone();
    bad.set_region(RegionFur::new("fur", -1.0));
    assert!(fur_locks(&s, &bad).is_err());
    // No fur regions at all: nothing grown, no error.
    s.regions = vec!["shirt".into(), "shirt2".into()];
    let f = fur_locks(&s, &p).unwrap();
    assert_eq!(f.stats.locks, 0);
    assert!(f.solid.mesh.tris.is_empty());
    let mut zero = p.clone();
    zero.set("kit.fur_locks.count", 0.0).unwrap();
    assert_eq!(fur_locks(&ball_with_sleeve(), &zero).unwrap().stats.locks, 0);
}

/// The frontier marshal (a character with colour regions and a skeleton),
/// compiled at its low tier by Evani.Model, with its hair as fur.
fn marshal_low() -> evani_model::compile::TierMesh {
    let src = include_str!("../../../tests/model_bench/characters/frontier_marshal.evmodel");
    let j = evani_model::intent::parse_text(src).unwrap_or_else(|e| panic!("{e:?}"));
    let mut tiers = evani_model::compile::compile(&j, Some(&["low".to_string()]), 2).unwrap_or_else(|e| panic!("{e:?}"));
    tiers.remove(0)
}

fn hair_params() -> LockParams {
    let mut p = quiet();
    p.set_region(RegionFur::new("hair", 30.0));
    if let Err(e) = p.set("kit.fur_locks.count", 400.0) {
        panic!("{e}");
    }
    p
}

#[test]
fn from_meshed_solids_keeps_regions_parts_and_mirrors() {
    let tm = marshal_low();
    let s = FurSurface::from_solids(&tm.model, &tm.solids);
    assert!(s.regions.iter().any(|r| r == "hair") && s.regions.iter().any(|r| r == "leather"), "{:?}", s.regions);
    assert_eq!(s.tri_part.len(), s.tris.len());
    let f = fur_locks(&s, &hair_params()).unwrap();
    assert!(f.stats.locks > 50, "{:?}", f.stats);
    // Hair meets the hat and the collar: gear.
    assert!(f.stats.near_gear > 0, "{:?}", f.stats);
    // Every lock keeps the part it grew from: a hair part.
    for &p in &f.solid.mesh.tri_part {
        assert_eq!(tm.model.parts[p as usize].region.as_deref(), Some("hair"), "lock on part {}", tm.model.parts[p as usize].name);
    }
    // A mirrored copy lands on the other side of x = 0, wound outwards.
    let mut m = FurSurface::new();
    let ball = sphere([0.2, 0.0, 0.0], 0.1, 12, 24, 0);
    m.add_mesh(&ball, "arm.l", [0.0; 3], false, &names(&["fur"]));
    m.add_mesh(&ball, "arm.r", [0.0; 3], true, &names(&["fur"]));
    let half = m.pos.len() / 2;
    assert_eq!(m.pos[half][0], -m.pos[0][0]);
    assert_eq!(m.solids, vec!["arm.l".to_string(), "arm.r".to_string()]);
    let t = m.tris[m.tris.len() - 3];
    let fnrm = evani_model::vec::cross(sub(m.pos[t[1] as usize], m.pos[t[0] as usize]), sub(m.pos[t[2] as usize], m.pos[t[0] as usize]));
    assert!(dot(fnrm, sub(m.pos[t[0] as usize], [-0.2, 0.0, 0.0])) > 0.0);
}

#[test]
fn the_bake_writes_fur_locks_into_the_glb_and_repeats_byte_for_byte() {
    let tm = marshal_low();
    let p = hair_params();
    let (baked, f) = crate::bake::bake_tier(&tm, &p).unwrap();
    assert_eq!(baked.solids.len(), tm.solids.len() + 1);
    assert_eq!(baked.tris, tm.tris + f.stats.triangles);
    assert!(f.stats.triangles <= MAX_TRIS);
    assert!(crate::bake::bake_tier(&baked, &p).is_err(), "a second bake must be refused");
    let col = evani_model::collide::build_collision(&baked.model);
    let mut meta = serde_json::Map::new();
    meta.insert("fur_locks".into(), crate::bake::record(&f.stats, &p));
    let glb = evani_model::gltf::write_tier(&baked, &col, None, &meta);
    let rb = evani_model::gltf::read(&glb).unwrap();
    let fur: Vec<_> = rb.instances.iter().filter(|i| i.0 == SOLID).collect();
    assert_eq!(fur.len(), 1);
    assert_eq!(fur[0].3.len(), f.stats.triangles);
    assert_eq!(rb.json["extras"]["evani"]["fur_locks"]["locks"], serde_json::json!(f.stats.locks));
    // Skinned: the locks carry weights like the rest of the body.
    let fi = rb.instances.iter().position(|i| i.0 == SOLID).unwrap();
    assert_eq!(rb.influences[fi].len(), fur[0].1.len());
    // The whole bake from text, twice: the same bytes.
    let src = include_str!("../../../tests/model_bench/characters/frontier_marshal.evmodel");
    let a = crate::bake::bake_glb(src, "low", &p, 2).unwrap();
    let b = crate::bake::bake_glb(src, "low", &p, 2).unwrap();
    assert!(a.glb == b.glb);
    assert_eq!(a.stats, f.stats);
    assert!(crate::bake::bake_glb(src, "nope", &p, 2).is_err());
}
