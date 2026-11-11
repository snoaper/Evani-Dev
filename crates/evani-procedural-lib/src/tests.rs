//! The registry, the manifest and the Cargo features agree, and the built
//! assets generate. Run with the categories on (`--features geometry,...`
//! or `--all-features`); with fewer features each check covers what is on.

use evani_core::params::{ParamRegistry, ParamValue, is_valid_param_name};
#[allow(unused_imports)]
use evani_procedural::{Category, Output, Path, REALISM, Registry, geo};
use serde_json::Value;

const MANIFEST: &str = include_str!("../../../catalog/procedural/library.json");
const CARGO: &str = include_str!("../Cargo.toml");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

fn assets(m: &Value) -> Vec<(String, String, String)> {
    m["categories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["assets"].as_array().unwrap().iter().map(move |a| (c["id"].as_str().unwrap().to_string(), a["feature"].as_str().unwrap().to_string(), a["status"].as_str().unwrap().to_string())))
        .collect()
}

#[test]
fn the_manifest_lists_24_categories_and_360_assets_each_with_a_source() {
    let m = manifest();
    let cats = m["categories"].as_array().unwrap();
    assert_eq!(cats.len(), 24);
    assert_eq!(assets(&m).len(), 357);
    assert_eq!(m["count"].as_u64(), Some(357));
    for c in cats {
        for a in c["assets"].as_array().unwrap() {
            let f = a["feature"].as_str().unwrap();
            assert!(matches!(a["status"].as_str(), Some("todo" | "placeholder" | "basic" | "done")), "{f}: status");
            assert!(a["source"]["url"].as_str().is_some_and(|u| u.starts_with("http")) || a["flags"].is_array(), "{f}: a source link or a flag");
            assert!(is_valid_param_name(a["param_prefix"].as_str().unwrap()), "{f}: param prefix");
            assert!(a["styles"]["native"].as_array().is_some_and(|s| s.iter().any(|x| x == REALISM)), "{f}: built for Realism first");
            if (a["status"] == "done" || a["status"] == "basic") && a["path"] == "cpu" {
                assert!(a["inspired_by"].as_str().is_some_and(|s| s.contains("MIT")), "{f}: a built asset names its MIT source");
            }
        }
    }
}

#[test]
fn every_manifest_entry_has_its_cargo_feature() {
    let cargo: toml_lite::Features = toml_lite::features(CARGO);
    let m = manifest();
    for c in m["categories"].as_array().unwrap() {
        let id = c["id"].as_str().unwrap();
        let members = cargo.get(id).unwrap_or_else(|| panic!("category feature {id}"));
        for a in c["assets"].as_array().unwrap() {
            let f = a["feature"].as_str().unwrap();
            assert!(members.iter().any(|x| x == f), "{id} turns on {f}");
            assert!(cargo.get(f).is_some_and(|d| d.iter().any(|x| *x == format!("cat-{id}"))), "{f} turns on cat-{id}");
        }
    }
}

#[test]
fn the_registry_holds_exactly_what_is_compiled_in() {
    let r = evani_procedural_lib_registry();
    let m = manifest();
    let listed = assets(&m);
    for f in r.features() {
        assert!(listed.iter().any(|(_, x, _)| *x == f), "{f} is in the manifest");
    }
    // Each asset compiled in is the manifest's: built (done) or a placeholder.
    for (_, f, status) in &listed {
        if let Some(a) = r.get(f) {
            let placeholder = a.params().iter().any(|p| p.describe == "Size of the stand-in (m)");
            assert_eq!(placeholder, status != "done" && status != "basic", "{f}: status {status}");
        }
    }
    for c in r.categories() {
        assert!(r.in_category(c.id).count() > 0, "{} has an asset on", c.id);
    }
}

fn evani_procedural_lib_registry() -> Registry {
    crate::registry()
}

#[test]
fn every_compiled_in_asset_registers_and_generates() {
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    for a in r.assets() {
        let f = a.info().feature();
        for n in r.param_names(&f) {
            assert!(reg.id(&n).is_some(), "{n}");
        }
        let g = r.generate(&reg, &f, REALISM).unwrap();
        match (a.info().path, &g.output) {
            (Path::Cpu, o @ (Output::Mesh(_) | Output::Parts(_))) => {
                assert!(o.is_valid(), "{f}: a valid mesh");
                // Standing on the ground, of a sane size and cost.
                let (lo, hi) = o.bounds().unwrap();
                assert!(lo[1] > -1e-3, "{f}: on the ground (lowest {})", lo[1]);
                assert!((0..3).any(|k| hi[k] - lo[k] > 0.01), "{f}: has a size");
                assert!(o.triangles() < 400_000, "{f}: {} triangles", o.triangles());
            }
            (Path::Gpu { .. }, Output::Module) => {}
            (p, o) => panic!("{f}: path {p:?} gave {o:?}"),
        }
    }
}

#[cfg(feature = "terrain-rolling-hills")]
#[test]
fn rolling_hills_follow_their_params_and_seed() {
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    let f = "terrain-rolling-hills";
    let a = r.generate(&reg, f, REALISM).unwrap().output;
    let (lo, hi) = a.bounds().unwrap();
    // Round 9: the tallest hill is its 3 m over the lowest ground, which
    // stands on the 0.3 m soil sides.
    assert!(lo[1] >= 0.0 && hi[1] > 2.5 && hi[1] <= 3.0 + 0.3 + 0.05, "hills between the ground and 3 m over the soil: {lo:?} {hi:?}");
    // Realism: meadow grass and at least one other ground cover.
    let Output::Parts(ps) = &a else { panic!("parts") };
    assert!(ps.len() >= 2 && ps.iter().all(|p| p.surface.is_some()), "{} covers", ps.len());
    assert!((hi[0] - lo[0] - 24.0).abs() < 1e-3, "24 m wide");
    reg.set_by_name("procedural.terrain.rolling_hills.seed", ParamValue::F32(7.0)).unwrap();
    let b = r.generate(&reg, f, REALISM).unwrap().output;
    assert_ne!(a, b, "the seed changes the hills");
    reg.set_by_name("procedural.terrain.rolling_hills.height", ParamValue::F32(12.0)).unwrap();
    let c = r.generate(&reg, f, "WK08").unwrap();
    assert!(c.fallback && c.style == REALISM, "voxel falls back to realism for now");
    assert!(c.output.bounds().unwrap().1[1] > 3.0, "taller hills");
}

/// Asset Studio column 28: hills a player walks in. With `horizon` the land
/// runs on past the square (one grid, its middle the square's own points, so
/// no seam), with `scars` worn soil and the limestone in it lie on the
/// steeper banks, and the player's ground is the drawn one; both stay inside
/// a game's budget.
#[cfg(feature = "terrain-rolling-hills")]
#[test]
fn rolling_hills_run_to_the_horizon_with_scars_at_a_walkers_scale() {
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    let f = "terrain-rolling-hills";
    let plain = r.generate(&reg, f, REALISM).unwrap().output;
    for (n, v) in [("size", 400.0), ("height", 34.0), ("feature_size", 200.0), ("octaves", 5.0), ("resolution", 200.0), ("edge", 0.0), ("outcrops", 1.0), ("horizon", 9000.0), ("scars", 0.55)] {
        reg.set_by_name(&format!("procedural.terrain.rolling_hills.{n}"), ParamValue::F32(v)).unwrap();
    }
    let a = r.generate(&reg, f, REALISM).unwrap().output;
    let (lo, hi) = a.bounds().unwrap();
    assert!(lo[0] < -8900.0 && hi[0] > 8900.0 && lo[2] < -8900.0 && hi[2] > 8900.0, "out to the horizon: {lo:?} {hi:?}");
    let Output::Parts(tp) = &a else { panic!("parts") };
    let per: Vec<usize> = tp.iter().map(|p| p.mesh.indices.len() / 3).collect();
    // The mountains' budget (400k) is the ceiling for a walker-scale land.
    assert!(a.triangles() < 300_000, "{} triangles ({per:?})", a.triangles());
    let Output::Parts(ps) = &a else { panic!("parts") };
    // Scar soil, the turf's covers and the limestone in the scars.
    assert!(ps.len() >= 3, "{} parts", ps.len());
    assert_ne!(plain, a);
    // The land's own function, its grid and a game's ground agree on the
    // square's points.
    let (v, seed) = r.values(&reg, f).unwrap();
    let l = crate::terrain::Hills::new(&v, seed);
    let (size, n, h) = crate::terrain::hills_ground(&v, seed);
    assert!((size - 400.0).abs() < 1e-3 && n == 201 && h.len() == n * n, "{size} {n}");
    for (i, j) in [(0usize, 0usize), (100, 37), (200, 200), (13, 180)] {
        let (x, z) = ((i as f32 / 200.0 - 0.5) * size, (j as f32 / 200.0 - 0.5) * size);
        assert!((l.height(x, z) - h[j * n + i]).abs() < 1e-3 && (l.ground(x, z) - h[j * n + i]).abs() < 1e-3, "at {i},{j}");
    }
    // Some of the square's steeper ground is scarred, most of it is turf.
    let scarred = (0..400).filter(|k| {
        let (x, z) = ((k % 20) as f32 * 19.0 - 180.0, (k / 20) as f32 * 19.0 - 180.0);
        l.scar_at(x, z)
    });
    let s = scarred.count();
    assert!((1..80).contains(&s), "{s} of 400 points scarred");
}

#[cfg(feature = "geometry-cube-cuboid")]
#[test]
fn the_cuboid_is_the_size_its_params_say() {
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    for (n, v) in [("width", 2.0), ("height", 3.0), ("depth", 0.5), ("segments", 4.0)] {
        reg.set_by_name(&format!("procedural.geometry.cube_cuboid.{n}"), ParamValue::F32(v)).unwrap();
    }
    reg.set_by_name("procedural.geometry.cube_cuboid.wear", ParamValue::F32(0.0)).unwrap();
    let out = r.generate(&reg, "geometry-cube-cuboid", REALISM).unwrap().output;
    let (lo, hi) = out.bounds().unwrap();
    let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    assert!(d.iter().zip([2.0, 3.0, 0.5]).all(|(a, b)| (a - b).abs() < 1e-4), "{d:?}");
    assert!(lo[1].abs() < 1e-6, "base on the ground");
    // Realism: one part in a catalogue finish (cast concrete), its grid
    // 10 + 2 x 4 steps a face, welded into one surface.
    let Output::Parts(ps) = &out else { panic!("parts") };
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].mesh.triangles(), 6 * 18 * 18 * 2, "18 x 18 steps a face");
    assert_eq!(ps[0].surface.map(|s| s.block[0]), Some(evani_procedural::surface::presets::CONCRETE_SMOOTH.block[0]));
}

#[test]
fn the_template_builds() {
    let mut r = Registry::new();
    r.add_category(Category { id: "template", name: "Template" });
    assert!(r.add(Box::new(crate::template::Template)));
    let reg = ParamRegistry::default();
    let g = r.generate(&reg, "template-box-stack", REALISM).unwrap();
    assert!(g.output.mesh().is_some_and(|m| m.triangles() == 3 * 12));
}

/// Just enough TOML for `[features]` lists (the test has no toml crate).
mod toml_lite {
    pub type Features = std::collections::HashMap<String, Vec<String>>;

    pub fn features(text: &str) -> Features {
        let mut out = Features::new();
        let mut on = false;
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('[') && !l.starts_with("[\"") {
                on = l == "[features]";
                continue;
            }
            if !on || l.starts_with('#') || l.is_empty() {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                let items = v.trim().trim_start_matches('[').trim_end_matches(']').split(',').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect();
                out.insert(k.trim().to_string(), items);
            }
        }
        out
    }
}

/// Every basic generator (batch A): the same values give the same output,
/// and every slider (and every colour of a multi-part asset) reaches what
/// it makes, through the live params.
#[test]
fn every_basic_generator_follows_its_live_params() {
    use evani_procedural::{SpecKind, placeholder::is_placeholder};
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    let m = manifest();
    let basic: Vec<String> = assets(&m).into_iter().filter(|(_, _, s)| s == "basic").map(|(_, f, _)| f).collect();
    for f in basic.iter().filter(|f| r.get(f).is_some()) {
        let a = r.get(f).unwrap();
        assert!(!is_placeholder(a) && a.info().path == Path::Cpu, "{f}: a real CPU generator");
        let base = r.generate(&reg, f, REALISM).unwrap().output;
        assert_eq!(base, r.generate(&reg, f, REALISM).unwrap().output, "{f}: deterministic");
        let prefix = a.info().prefix();
        for p in a.params() {
            let name = format!("{prefix}.{}", p.name);
            let id = reg.id(&name).unwrap();
            let was = reg.value(id).unwrap();
            let moved = match p.kind {
                SpecKind::Num { default, min, max, .. } => ParamValue::F32(if (max - default).abs() > (default - min).abs() { max } else { min }),
                SpecKind::Color(c) => match base {
                    // A single mesh takes its colour live, not in the mesh.
                    Output::Mesh(_) => continue,
                    _ => ParamValue::Color([1.0 - c[0], 1.0 - c[1], 1.0 - c[2]]),
                },
            };
            reg.set(id, moved).unwrap();
            let out = r.generate(&reg, f, REALISM).unwrap().output;
            assert!(out.is_valid(), "{name} at its limit: a valid mesh");
            assert_ne!(out, base, "{name} changes the asset");
            reg.set(id, was).unwrap();
        }
    }
}

/// Batch B, the built world: the 8 categories every built-world asset lives in.
const BUILT_WORLD: [&str; 8] = ["geometry", "roads", "buildings", "architecture", "furniture", "props", "industrial", "city"];

/// No stand-in is left in the built world: all 120 assets are real
/// generators in the manifest, and every one compiled into this build is.
#[test]
fn the_built_world_has_no_stand_ins_left() {
    use evani_procedural::placeholder::is_placeholder;
    let m = manifest();
    let (mut real, mut stand_ins) = (0, Vec::new());
    for c in m["categories"].as_array().unwrap().iter().filter(|c| BUILT_WORLD.contains(&c["id"].as_str().unwrap())) {
        for a in c["assets"].as_array().unwrap() {
            match a["status"].as_str() {
                Some("basic" | "done") => real += 1,
                _ => stand_ins.push(a["feature"].as_str().unwrap().to_string()),
            }
            if a["status"] == "basic" {
                assert!(a["inspired_by"].as_str().is_some_and(|s| s.contains("MIT")), "{}: an MIT inspiration note", a["feature"]);
                assert_eq!(a["styles"]["native"], serde_json::json!([REALISM]), "{}: Realism only", a["feature"]);
            }
        }
    }
    assert!(stand_ins.is_empty(), "stand-ins left: {stand_ins:?}");
    assert_eq!(real, 120, "8 categories of 15");
    let r = crate::registry();
    for a in r.assets().filter(|a| BUILT_WORLD.contains(&a.info().category)) {
        assert!(!is_placeholder(a), "{}: compiled in as a stand-in", a.info().feature());
    }
}

/// Every built-world asset compiled in switches off and on through its live
/// `.on` param, has a seed and two to four sliders, and each slider moved
/// to its far end changes what it makes (every_basic_generator_follows_its_live_params
/// checks the same for each colour).
#[test]
fn every_built_world_asset_switches_and_its_sliders_change_it() {
    use evani_procedural::SpecKind;
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    let mut checked = 0;
    for a in r.assets().filter(|a| BUILT_WORLD.contains(&a.info().category)) {
        let f = a.info().feature();
        let prefix = a.info().prefix();
        let on = format!("{prefix}.on");
        assert!(r.is_on(&reg, &f), "{f}: on by default");
        reg.set_by_name(&on, ParamValue::Bool(false)).unwrap();
        assert!(!r.is_on(&reg, &f), "{f}: switched off");
        reg.set_by_name(&on, ParamValue::Bool(true)).unwrap();
        assert!(r.is_on(&reg, &f), "{f}: back on");
        assert!(reg.id(&format!("{prefix}.seed")).is_some(), "{f}: a seed");
        let sliders: Vec<_> = a.params().iter().filter(|p| matches!(p.kind, SpecKind::Num { .. })).collect();
        if f == "geometry-cube-cuboid" {
            continue;
        }
        // The realism preview's form picker is the viewer's, not a shape
        // slider; nor are the granite ball's stone sliders (the live granite
        // session: they tune its material, and each still has to change it).
        const GRANITE: [&str; 7] = ["rough", "crystal", "dark", "sizes", "buff", "quartz", "inner"];
        // Nor are the faceted ball's concrete and joint-tone sliders (the
        // live faceted-ball session: its material, as the cube's).
        const FACET: [&str; 14] = ["seam_tone", "patches", "concrete", "view", "rough", "mottle", "aggregate", "pores", "weathering", "blowholes", "grain", "stains", "streaks", "pattern"];
        // Nor are the oak block's wood sliders (the live oak session: they
        // tune its solid oak; the pith's two also move its checks).
        const OAK: [&str; 10] = ["ring_mm", "warp", "contrast", "knots", "relief", "rays", "streaks", "rough", "pith_y", "pith_z"];
        // Nor are the garden cone's concrete sliders (the live cone session).
        const CONE: [&str; 9] = ["rough", "mottle", "aggregate", "pores", "weathering", "blowholes", "blotches", "coverage", "view"];
        let shape_sliders = sliders
            .iter()
            .filter(|p| {
                p.name != "preview" && !(f == "geometry-uv-sphere" && GRANITE.contains(&p.name)) && !(f == "geometry-icosphere" && FACET.contains(&p.name)) && !(f == "geometry-bevelled-box" && OAK.contains(&p.name)) && !(f == "geometry-cone" && CONE.contains(&p.name))
            })
            .count();
        assert!((2..=6).contains(&shape_sliders), "{f}: {shape_sliders} sliders");
        let base = r.generate(&reg, &f, REALISM).unwrap().output;
        for p in sliders {
            let SpecKind::Num { default, min, max, .. } = p.kind else { continue };
            let name = format!("{prefix}.{}", p.name);
            let far = if (max - default).abs() > (default - min).abs() { max } else { min };
            reg.set_by_name(&name, ParamValue::F32(far)).unwrap();
            let moved = r.generate(&reg, &f, REALISM).unwrap();
            assert!(moved.output.is_valid() && moved.output != base, "{name}: changes {f}");
            // Another style falls back to Realism, the only one built.
            let other = r.generate(&reg, &f, "WK08").unwrap();
            assert!(other.fallback && other.style == REALISM && other.output == moved.output, "{f}: falls back to Realism");
            reg.set_by_name(&name, ParamValue::F32(default)).unwrap();
        }
        checked += 1;
    }
    if BUILT_WORLD.iter().all(|c| r.categories().iter().any(|k| k.id == *c)) {
        assert_eq!(checked, 119, "every built-world asset but the reference cuboid");
    }
}

/// Batch C, vehicles and creatures: the last 7 categories.
const BATCH_C: [&str; 7] = ["vehicles", "aircraft", "boats", "humans", "clothing", "animals", "weapons"];

/// No stand-in is left anywhere: all 357 assets in the manifest are built
/// (`basic` or `done`, wrapped modules included), none is a placeholder,
/// every basic one names its MIT inspiration and builds Realism only, and
/// no asset compiled into this build is a stand-in.
#[test]
fn the_library_has_no_stand_ins_left() {
    use evani_procedural::placeholder::is_placeholder;
    let m = manifest();
    let (mut built, mut stand_ins) = (0, Vec::new());
    for c in m["categories"].as_array().unwrap() {
        for a in c["assets"].as_array().unwrap() {
            match a["status"].as_str() {
                Some("basic" | "done") => built += 1,
                _ => stand_ins.push(a["feature"].as_str().unwrap().to_string()),
            }
            if a["status"] == "basic" {
                assert!(a["inspired_by"].as_str().is_some_and(|s| s.contains("MIT")), "{}: an MIT inspiration note", a["feature"]);
                assert_eq!(a["styles"]["native"], serde_json::json!([REALISM]), "{}: Realism only", a["feature"]);
            }
        }
    }
    assert!(stand_ins.is_empty(), "{} stand-ins left: {stand_ins:?}", stand_ins.len());
    assert_eq!(built, 357);
    let r = crate::registry();
    let compiled: Vec<String> = r.assets().filter(|a| is_placeholder(*a)).map(|a| a.info().feature()).collect();
    assert!(compiled.is_empty(), "compiled in as stand-ins: {compiled:?}");
    assert!(!include_str!("generated.rs").contains("Placeholder::new("), "the generated table has no placeholder left");
}

/// Every batch-C asset compiled in switches off and on through its live
/// `.on` param, has a seed and two to four sliders, each slider at its far
/// end changes what it makes, and another style falls back to Realism.
#[test]
fn every_batch_c_asset_switches_and_its_sliders_change_it() {
    use evani_procedural::SpecKind;
    let r = crate::registry();
    let mut reg = ParamRegistry::default();
    r.register_params(&mut reg).unwrap();
    let mut checked = 0;
    for a in r.assets().filter(|a| BATCH_C.contains(&a.info().category)) {
        let f = a.info().feature();
        let prefix = a.info().prefix();
        let on = format!("{prefix}.on");
        assert!(r.is_on(&reg, &f), "{f}: on by default");
        reg.set_by_name(&on, ParamValue::Bool(false)).unwrap();
        assert!(!r.is_on(&reg, &f), "{f}: switched off");
        reg.set_by_name(&on, ParamValue::Bool(true)).unwrap();
        assert!(r.is_on(&reg, &f), "{f}: back on");
        assert!(reg.id(&format!("{prefix}.seed")).is_some(), "{f}: a seed");
        let sliders: Vec<_> = a.params().iter().filter(|p| matches!(p.kind, SpecKind::Num { .. })).collect();
        assert!((2..=6).contains(&sliders.len()), "{f}: {} sliders", sliders.len());
        let base = r.generate(&reg, &f, REALISM).unwrap().output;
        for p in sliders {
            let SpecKind::Num { default, min, max, .. } = p.kind else { continue };
            let name = format!("{prefix}.{}", p.name);
            let far = if (max - default).abs() > (default - min).abs() { max } else { min };
            reg.set_by_name(&name, ParamValue::F32(far)).unwrap();
            let moved = r.generate(&reg, &f, REALISM).unwrap();
            assert!(moved.output.is_valid() && moved.output != base, "{name}: changes {f}");
            let other = r.generate(&reg, &f, "WK08").unwrap();
            assert!(other.fallback && other.style == REALISM && other.output == moved.output, "{f}: falls back to Realism");
            reg.set_by_name(&name, ParamValue::F32(default)).unwrap();
        }
        checked += 1;
    }
    if BATCH_C.iter().all(|c| r.categories().iter().any(|k| k.id == *c)) {
        assert_eq!(checked, 105, "every batch-C asset");
    }
}
