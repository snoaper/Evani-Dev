//! Composition from the catalogue's typed building-block links
//! (`blocks_table`, written by tools/procedural/gen_blocks.mjs from
//! catalog/procedural/blocks.json): a landscape with snow and moss is ONE
//! mesh with layered materials, and only scatter links make instances.

use evani_procedural::compose::{assemble, Join, Kind, Options};
use evani_procedural::geo;
use evani_procedural::surface::Surface;
use evani_procedural::{Output, Part};
use serde_json::Value;

use crate::blocks_table::GRAPH;

const BLOCKS: &str = include_str!("../../../catalog/procedural/blocks.json");
/// Granite grey (a catalogue row, as in evani-procedural's presets).
const GRANITE: Surface = Surface::hex("060003dd9c9c9a5c5c5ee8e6e099001a980099000000005d0000000000000000", 0.6, 0.0);

/// A stand-in massif: a 400 m square of ground rising to a 60 m dome, with
/// a rough hollowed flank, as ONE surfaced part (the landscape's ground).
fn massif() -> Output {
    let m = geo::height_grid([400.0, 400.0], 64, |x, z| {
        let r = (x * x + z * z).sqrt() / 200.0;
        60.0 * (1.0 - r.min(1.0)).powf(1.5) + 2.0 * (x * 0.07).sin() * (z * 0.05).cos()
    });
    Output::Parts(vec![Part::surfaced(m, GRANITE)])
}

fn scatter_blocks(id: &str, out: &mut Vec<&'static str>) {
    for e in GRAPH.edges(id) {
        match e.kind {
            Kind::Scatter(_) => out.push(e.block),
            Kind::Layer(_) | Kind::Base(Join::Merge | Join::Part) if e.block.starts_with("planned:") => scatter_blocks(e.block, out),
            _ => {}
        }
    }
}

#[test]
fn a_landscape_with_snow_and_moss_assembles_as_one_mesh_with_layered_materials() {
    let before = massif();
    assert_eq!(before.meshes().len(), 1);
    let a = assemble(&GRAPH, "terrain-mountains", before, &mut |_| None, &Options::default());
    // One mesh: the snow and the moss are material layers on it, not meshes.
    assert_eq!(a.meshes(), 1, "{:?}", a.missing);
    let layers = a.material_layers();
    assert_eq!(layers, vec![vec!["snow", "growth"]], "snow and moss (growth) are condition layers on the one surface");
    let drawn: Vec<&str> = a.layers.iter().filter(|l| l.drawn).map(|l| l.block).collect();
    assert_eq!(drawn, vec!["vegetation-moss-patches", "material:ground.snow.fresh"]);
    // The moss sits inside the alpine meadow's mask, the snow high and flat.
    let moss = a.layers.iter().find(|l| l.block == "vegetation-moss-patches").map(|l| l.mask.len());
    assert_eq!(moss, Some(4), "meadow (height band, slope) + moss (low, concave)");
    for l in a.layers.iter().filter(|l| l.drawn) {
        assert!(l.coverage > 0.0 && l.coverage < 0.6, "{} covers part of the ground: {}", l.block, l.coverage);
    }
    // Only scatter links make instances.
    let mut allowed = Vec::new();
    scatter_blocks("terrain-mountains", &mut allowed);
    assert!(!a.instances.is_empty());
    for i in &a.instances {
        assert!(allowed.contains(&i.block), "{} is not a scatter link", i.block);
    }
    for not in ["material:ground.snow.fresh", "vegetation-moss-patches", "sky-dynamic-sky-dome", "rocks-scree-slopes"] {
        assert!(!a.instances.iter().any(|i| i.block == not), "{not} made instances");
    }
    // Base parts draw on their own; with scatter off there are no instances and still one mesh.
    assert_eq!(a.parts, vec!["rocks-scree-slopes", "sky-dynamic-sky-dome", "sky-distance-fog"]);
    let bare = assemble(&GRAPH, "terrain-mountains", massif(), &mut |_| None, &Options { max_instances: 0, ..Options::default() });
    assert!(bare.instances.is_empty());
    assert_eq!(bare.meshes(), 1);
    assert_eq!(bare.material_layers(), layers);
    // Short grass only grows inside the alpine meadow's band (height 0.15..0.6, soft edge 0.15 of the range).
    let (lo, hi) = (0.0f32, 60.0 + 2.0);
    for g in a.instances.iter().filter(|i| i.block == "vegetation-short-grass") {
        let h = (g.position[1] - lo) / (hi - lo);
        assert!((0.05..=0.7).contains(&h), "short grass at height fraction {h}");
    }
    // Deterministic.
    assert_eq!(a, assemble(&GRAPH, "terrain-mountains", massif(), &mut |_| None, &Options::default()));
}

#[test]
fn the_engine_table_matches_the_catalogue_link_for_link() {
    let blocks: Value = serde_json::from_str(BLOCKS).unwrap();
    let needs = blocks["needs"].as_object().unwrap();
    assert_eq!(GRAPH.nodes.len(), needs.len(), "run node tools/procedural/gen_blocks.mjs");
    for (id, list) in needs {
        let edges = GRAPH.edges(id);
        let list = list.as_array().unwrap();
        assert_eq!(edges.len(), list.len(), "{id}");
        for (e, x) in edges.iter().zip(list) {
            assert_eq!(e.block, x["block"].as_str().unwrap(), "{id}");
            let kind = match e.kind {
                Kind::Layer(_) => "layer",
                Kind::Scatter(_) => "scatter",
                Kind::Base(_) => "base",
            };
            assert_eq!(kind, x["kind"].as_str().unwrap(), "{id} -> {}", e.block);
        }
    }
}

#[cfg(feature = "terrain-mountains")]
#[test]
fn the_real_mountain_takes_its_layers_without_a_new_mesh() {
    use evani_procedural::Values;
    let r = crate::registry();
    let Some(m) = r.get("terrain-mountains") else { panic!("compiled in") };
    let v = Values::defaults(&evani_procedural::Registry::specs(m));
    let out = m.generate(&v, 1, evani_procedural::style::REALISM);
    let before = out.meshes().len();
    let a = assemble(&GRAPH, "terrain-mountains", out, &mut |_| None, &Options { max_instances: 0, ..Options::default() });
    assert_eq!(a.meshes(), before, "layers add no meshes");
    assert!(a.material_layers().iter().any(|l| l.contains(&"snow") && l.contains(&"growth")));
}
