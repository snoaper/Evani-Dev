//! The typed building-block graph (catalog/procedural/blocks.json) as engine data: written by
//! tools/procedural/gen_blocks.mjs, never by hand (`--check` fails when it is stale). Read through
//! evani_procedural::compose (docs/PROCEDURAL.md "Composition from typed links").

use evani_procedural::compose::{Edge, Graph, Join, Kind, Layer, Mask, Node, Scatter, Term};

/// Every block with links, sorted by id.
pub static GRAPH: Graph = Graph {
    nodes: &[
        Node {
            id: "planned:alpine-meadow",
            edges: &[
                Edge { block: "vegetation-short-grass", kind: Kind::Scatter(Scatter { density: 4.0, scale: [0.8, 1.2], lod: 40.0, impostor: 0.0, slope: [0.0, 40.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "vegetation-shrubs", kind: Kind::Scatter(Scatter { density: 0.05, scale: [0.7, 1.3], lod: 60.0, impostor: 250.0, slope: [0.0, 40.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "vegetation-moss-patches", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Height, min: None, max: Some(0.4), scale: 1.0 }, Term { by: Mask::Curvature, min: None, max: Some(-0.1), scale: 1.0 }], blend: 0.8, falloff: 0.2, condition: Some("growth") }) },
            ],
        },
        Node {
            id: "planned:barren-ground",
            edges: &[
                Edge { block: "material:ground.soil.packed", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-small-stones", kind: Kind::Scatter(Scatter { density: 0.4, scale: [0.6, 1.4], lod: 25.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:cave-floor",
            edges: &[
                Edge { block: "material:ground.soil.packed", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-small-stones", kind: Kind::Scatter(Scatter { density: 0.4, scale: [0.6, 1.4], lod: 25.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:dry-wash",
            edges: &[
                Edge { block: "material:ground.sand.desert", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-pebbles", kind: Kind::Scatter(Scatter { density: 0.4, scale: [0.6, 1.4], lod: 25.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:dune-grass",
            edges: &[
                Edge { block: "vegetation-long-grass", kind: Kind::Scatter(Scatter { density: 3.0, scale: [0.8, 1.3], lod: 40.0, impostor: 0.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "material:ground.sand.beach", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "planned:gravel-bar",
            edges: &[
                Edge { block: "material:ground.sand.river", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-pebbles", kind: Kind::Scatter(Scatter { density: 0.4, scale: [0.6, 1.4], lod: 25.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:lakeshore",
            edges: &[
                Edge { block: "rocks-rounded-river-rocks", kind: Kind::Scatter(Scatter { density: 0.1, scale: [0.6, 1.3], lod: 60.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "vegetation-long-grass", kind: Kind::Scatter(Scatter { density: 3.0, scale: [0.8, 1.3], lod: 40.0, impostor: 0.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:marsh-ground",
            edges: &[
                Edge { block: "material:ground.mud.swamp", kind: Kind::Base(Join::Surface) },
                Edge { block: "material:ground.grass.marsh", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Noise, min: Some(0.35), max: None, scale: 0.5 }], blend: 0.9, falloff: 0.2, condition: None }) },
            ],
        },
        Node {
            id: "planned:meadow",
            edges: &[
                Edge { block: "vegetation-long-grass", kind: Kind::Scatter(Scatter { density: 3.0, scale: [0.8, 1.3], lod: 40.0, impostor: 0.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "vegetation-wildflowers", kind: Kind::Scatter(Scatter { density: 0.3, scale: [0.8, 1.2], lod: 30.0, impostor: 0.0, slope: [0.0, 30.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:riverbank",
            edges: &[
                Edge { block: "material:ground.mud.riverbank", kind: Kind::Base(Join::Surface) },
                Edge { block: "vegetation-long-grass", kind: Kind::Scatter(Scatter { density: 3.0, scale: [0.8, 1.3], lod: 40.0, impostor: 0.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "planned:stony-scar",
            edges: &[
                Edge { block: "material:ground.soil.loam", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-medium-rocks", kind: Kind::Scatter(Scatter { density: 0.02, scale: [0.7, 1.4], lod: 80.0, impostor: 250.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: false }) },
            ],
        },
        Node {
            id: "planned:turf",
            edges: &[
                Edge { block: "vegetation-short-grass", kind: Kind::Scatter(Scatter { density: 4.0, scale: [0.8, 1.2], lod: 40.0, impostor: 0.0, slope: [0.0, 40.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "material:ground.soil.loam", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "rocks-jagged-rock-formations",
            edges: &[
                Edge { block: "rocks-pebbles", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "rocks-medium-rocks",
            edges: &[
                Edge { block: "rocks-pebbles", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "rocks-rounded-river-rocks",
            edges: &[
                Edge { block: "rocks-pebbles", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "rocks-scree-slopes",
            edges: &[
                Edge { block: "rocks-medium-rocks", kind: Kind::Scatter(Scatter { density: 0.02, scale: [0.7, 1.4], lod: 80.0, impostor: 250.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: false }) },
            ],
        },
        Node {
            id: "sky-dynamic-sky-dome",
            edges: &[
                Edge { block: "sky-volumetric-looking-clouds", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-sun-and-moon-discs", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-beaches",
            edges: &[
                Edge { block: "material:ground.sand.beach", kind: Kind::Base(Join::Surface) },
                Edge { block: "water-oceans", kind: Kind::Base(Join::Part) },
                Edge { block: "water-wave-foam", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-canyons",
            edges: &[
                Edge { block: "material:rock.sandstone.red", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-scree-slopes", kind: Kind::Base(Join::Part) },
                Edge { block: "rocks-medium-rocks", kind: Kind::Scatter(Scatter { density: 0.02, scale: [0.7, 1.4], lod: 80.0, impostor: 250.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "planned:dry-wash", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Height, min: None, max: Some(0.1), scale: 1.0 }, Term { by: Mask::Curvature, min: None, max: Some(-0.2), scale: 1.0 }], blend: 1.0, falloff: 0.1, condition: None }) },
                Edge { block: "vegetation-shrubs", kind: Kind::Scatter(Scatter { density: 0.05, scale: [0.7, 1.3], lod: 60.0, impostor: 250.0, slope: [0.0, 40.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-caves",
            edges: &[
                Edge { block: "material:rock.limestone", kind: Kind::Base(Join::Surface) },
                Edge { block: "rocks-jagged-rock-formations", kind: Kind::Scatter(Scatter { density: 0.01, scale: [0.7, 1.3], lod: 120.0, impostor: 400.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "planned:cave-floor", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Slope, min: None, max: Some(25.0), scale: 1.0 }], blend: 1.0, falloff: 0.1, condition: None }) },
            ],
        },
        Node {
            id: "terrain-cliffs",
            edges: &[
                Edge { block: "rocks-rock-walls", kind: Kind::Base(Join::Merge) },
                Edge { block: "rocks-scree-slopes", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-craters",
            edges: &[
                Edge { block: "planned:barren-ground", kind: Kind::Base(Join::Merge) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-flat-terrain",
            edges: &[
                Edge { block: "planned:turf", kind: Kind::Base(Join::Merge) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-islands",
            edges: &[
                Edge { block: "terrain-beaches", kind: Kind::Base(Join::Merge) },
                Edge { block: "water-oceans", kind: Kind::Base(Join::Part) },
                Edge { block: "trees-tree-canopies", kind: Kind::Scatter(Scatter { density: 0.005, scale: [0.8, 1.2], lod: 150.0, impostor: 800.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-mountain-ranges",
            edges: &[
                Edge { block: "terrain-mountains", kind: Kind::Base(Join::Merge) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-mountains",
            edges: &[
                Edge { block: "rocks-medium-rocks", kind: Kind::Scatter(Scatter { density: 0.02, scale: [0.7, 1.4], lod: 80.0, impostor: 250.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "rocks-scree-slopes", kind: Kind::Base(Join::Part) },
                Edge { block: "trees-dense-forest-clusters", kind: Kind::Scatter(Scatter { density: 0.002, scale: [0.8, 1.2], lod: 200.0, impostor: 1200.0, slope: [0.0, 35.0], height: [0.0, 0.6], avoid_water: true }) },
                Edge { block: "planned:alpine-meadow", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Height, min: Some(0.15), max: Some(0.6), scale: 1.0 }, Term { by: Mask::Slope, min: None, max: Some(30.0), scale: 1.0 }], blend: 1.0, falloff: 0.15, condition: None }) },
                Edge { block: "material:ground.snow.fresh", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Height, min: Some(0.6), max: None, scale: 1.0 }, Term { by: Mask::Slope, min: None, max: Some(35.0), scale: 1.0 }], blend: 0.9, falloff: 0.15, condition: Some("snow") }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-plateaus",
            edges: &[
                Edge { block: "rocks-rock-walls", kind: Kind::Base(Join::Merge) },
                Edge { block: "terrain-flat-terrain", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-ravines",
            edges: &[
                Edge { block: "rocks-rock-walls", kind: Kind::Base(Join::Merge) },
                Edge { block: "water-streams", kind: Kind::Base(Join::Part) },
                Edge { block: "vegetation-shrubs", kind: Kind::Scatter(Scatter { density: 0.05, scale: [0.7, 1.3], lod: 60.0, impostor: 250.0, slope: [0.0, 40.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-rolling-hills",
            edges: &[
                Edge { block: "planned:turf", kind: Kind::Base(Join::Merge) },
                Edge { block: "planned:stony-scar", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Slope, min: Some(30.0), max: None, scale: 1.0 }], blend: 1.0, falloff: 0.1, condition: None }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-sand-dunes",
            edges: &[
                Edge { block: "material:ground.sand.desert", kind: Kind::Base(Join::Surface) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-terraced-landscapes",
            edges: &[
                Edge { block: "architecture-walls", kind: Kind::Base(Join::Part) },
                Edge { block: "agriculture-crop-rows", kind: Kind::Scatter(Scatter { density: 0.05, scale: [0.9, 1.1], lod: 80.0, impostor: 300.0, slope: [0.0, 15.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "terrain-valleys",
            edges: &[
                Edge { block: "water-streams", kind: Kind::Base(Join::Part) },
                Edge { block: "planned:meadow", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Slope, min: None, max: Some(20.0), scale: 1.0 }, Term { by: Mask::Height, min: None, max: Some(0.5), scale: 1.0 }], blend: 1.0, falloff: 0.15, condition: None }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "trees-dense-forest-clusters",
            edges: &[
                Edge { block: "trees-pine-trees", kind: Kind::Scatter(Scatter { density: 0.05, scale: [0.8, 1.25], lod: 120.0, impostor: 600.0, slope: [0.0, 35.0], height: [0.0, 1.0], avoid_water: true }) },
            ],
        },
        Node {
            id: "trees-oak-trees",
            edges: &[
                Edge { block: "material:bark.oak", kind: Kind::Base(Join::Surface) },
                Edge { block: "material:leaf.oak", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "trees-pine-trees",
            edges: &[
                Edge { block: "material:bark.pine", kind: Kind::Base(Join::Surface) },
                Edge { block: "material:needle.pine", kind: Kind::Base(Join::Surface) },
            ],
        },
        Node {
            id: "water-lakes",
            edges: &[
                Edge { block: "planned:lakeshore", kind: Kind::Base(Join::Merge) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-marshes",
            edges: &[
                Edge { block: "planned:marsh-ground", kind: Kind::Base(Join::Merge) },
                Edge { block: "vegetation-reeds", kind: Kind::Scatter(Scatter { density: 1.0, scale: [0.8, 1.2], lod: 40.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 0.2], avoid_water: false }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-oceans",
            edges: &[
                Edge { block: "water-ocean-waves", kind: Kind::Base(Join::Merge) },
                Edge { block: "water-wave-foam", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-ponds",
            edges: &[
                Edge { block: "planned:lakeshore", kind: Kind::Base(Join::Merge) },
                Edge { block: "vegetation-reeds", kind: Kind::Scatter(Scatter { density: 1.0, scale: [0.8, 1.2], lod: 40.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 0.2], avoid_water: false }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-rapids",
            edges: &[
                Edge { block: "rocks-rounded-river-rocks", kind: Kind::Scatter(Scatter { density: 0.1, scale: [0.6, 1.3], lod: 60.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "water-wave-foam", kind: Kind::Base(Join::Part) },
                Edge { block: "planned:riverbank", kind: Kind::Base(Join::Merge) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-rivers",
            edges: &[
                Edge { block: "planned:riverbank", kind: Kind::Base(Join::Merge) },
                Edge { block: "planned:gravel-bar", kind: Kind::Base(Join::Merge) },
                Edge { block: "rocks-rounded-river-rocks", kind: Kind::Scatter(Scatter { density: 0.1, scale: [0.6, 1.3], lod: 60.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "trees-oak-trees", kind: Kind::Scatter(Scatter { density: 0.002, scale: [0.8, 1.2], lod: 120.0, impostor: 600.0, slope: [0.0, 30.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "trees-hedgerows", kind: Kind::Scatter(Scatter { density: 0.002, scale: [0.9, 1.1], lod: 120.0, impostor: 500.0, slope: [0.0, 20.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "planned:meadow", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Slope, min: None, max: Some(20.0), scale: 1.0 }, Term { by: Mask::Height, min: None, max: Some(0.5), scale: 1.0 }], blend: 1.0, falloff: 0.15, condition: None }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-distance-fog", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-shorelines",
            edges: &[
                Edge { block: "material:ground.sand.beach", kind: Kind::Base(Join::Surface) },
                Edge { block: "planned:dune-grass", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Height, min: Some(0.3), max: None, scale: 1.0 }, Term { by: Mask::Noise, min: Some(0.4), max: None, scale: 0.2 }], blend: 1.0, falloff: 0.2, condition: None }) },
                Edge { block: "rocks-medium-rocks", kind: Kind::Scatter(Scatter { density: 0.02, scale: [0.7, 1.4], lod: 80.0, impostor: 250.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "rocks-pebbles", kind: Kind::Scatter(Scatter { density: 0.4, scale: [0.6, 1.4], lod: 25.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
                Edge { block: "water-wave-foam", kind: Kind::Base(Join::Part) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-streams",
            edges: &[
                Edge { block: "planned:riverbank", kind: Kind::Base(Join::Merge) },
                Edge { block: "rocks-rounded-river-rocks", kind: Kind::Scatter(Scatter { density: 0.1, scale: [0.6, 1.3], lod: 60.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-tidal-pools",
            edges: &[
                Edge { block: "rocks-medium-rocks", kind: Kind::Scatter(Scatter { density: 0.02, scale: [0.7, 1.4], lod: 80.0, impostor: 250.0, slope: [0.0, 45.0], height: [0.0, 1.0], avoid_water: false }) },
                Edge { block: "material:ground.mud.tidal", kind: Kind::Layer(Layer { mask: &[Term { by: Mask::Height, min: None, max: Some(0.3), scale: 1.0 }], blend: 1.0, falloff: 0.1, condition: Some("mud") }) },
                Edge { block: "sky-dynamic-sky-dome", kind: Kind::Base(Join::Part) },
            ],
        },
        Node {
            id: "water-waterfalls",
            edges: &[
                Edge { block: "rocks-rock-walls", kind: Kind::Base(Join::Merge) },
                Edge { block: "effects-water-splashes", kind: Kind::Base(Join::Part) },
                Edge { block: "rocks-rounded-river-rocks", kind: Kind::Scatter(Scatter { density: 0.1, scale: [0.6, 1.3], lod: 60.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
            ],
        },
    ],
};
