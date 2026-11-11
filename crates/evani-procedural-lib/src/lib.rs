//! evani-procedural-lib: the Evani.Dev procedural library's assets, by
//! category (catalog/procedural/library.json; docs/PROCEDURAL.md).
//!
//! Every category and every asset is its own Cargo feature, and a game
//! turns on only what its evani.toml lists (`[modules] procedural =
//! ["terrain", "geometry-cube-cuboid"]`), so nothing else is compiled into
//! its download. [`registry`] holds exactly the compiled-in assets;
//! [`module`] is the engine module that registers their live params, draws
//! the placed CPU assets and hosts the wrapped engine modules.
//!
//! - Built generators: [`geometry`] (cube / cuboid), [`terrain`] (rolling
//!   hills).
//! - Wrapped as they are ([`wrapped`]): the storm sea and ocean (spectral
//!   sea), lakes, ponds, rivers and glass (the nature pass), the sky dome,
//!   short grass, long grass and wheat (evani-grass).
//! - Every other listed asset: a placeholder made from its name
//!   (generated.rs, written by tools/procedural/gen_library.py), replaced
//!   one by one by category workers ([`template`] is the asset to copy).

/// One basic generator ([`evani_procedural::FnAsset`]) registered behind
/// its own Cargo feature: `basic!(r, "<feature>", "<category>", "<id>",
/// "<name>", "<material>", [params], |v: &Values, seed: u32| -> Output)`.
/// The function is a plain (non-capturing) closure, so each asset is its own
/// code, compiled only with its feature.
macro_rules! basic {
    ($r:expr, $feat:literal, $cat:literal, $id:literal, $name:literal, $mat:literal, [$($p:expr),* $(,)?], $make:expr) => {
        #[cfg(feature = $feat)]
        {
            const PARAMS: &[evani_procedural::ParamSpec] = &[$($p),*];
            $r.add(Box::new(evani_procedural::FnAsset { info: evani_procedural::AssetInfo::cpu($cat, $id, $name).material($mat), params: PARAMS, make: $make }));
        }
    };
}

pub mod geometry;
mod generated;
#[cfg(feature = "cat-effects")]
pub mod effects;
#[cfg(feature = "cat-textures")]
pub mod textures;
#[cfg(feature = "cat-agriculture")]
pub mod agriculture;
#[cfg(feature = "cat-sky")]
pub mod sky;
#[cfg(feature = "sky-volumetric-looking-clouds")]
pub mod clouds;
// The distance fog's depth-based height fog (Asset Studio column 77).
#[cfg(feature = "sky-distance-fog")]
pub mod fog;
// The dynamic sky dome's photoreal recipe (Asset Studio column 21).
#[cfg(feature = "sky-dynamic-sky-dome")]
pub mod skydome;
// The sun and moon discs' photoreal recipe (Asset Studio column 38).
#[cfg(feature = "sky-sun-and-moon-discs")]
pub mod sunmoon;
#[cfg(feature = "cat-water")]
pub mod water;
#[cfg(feature = "water-shorelines")]
pub mod shore;
// The river valley (the Asset Studio rivers session, column 20).
#[cfg(feature = "water-rivers")]
pub mod river;
#[cfg(feature = "cat-trees")]
pub mod trees;
// The photoreal oak (the Asset Studio oak session); the rivers' bank oaks
// are grown by it too.
#[cfg(any(feature = "trees-oak-trees", feature = "water-rivers"))]
pub mod oak;
#[cfg(feature = "cat-vegetation")]
pub mod vegetation;
#[cfg(feature = "cat-rocks")]
pub mod rocks;
// The pebbles' stones and their group (the Asset Studio pebbles session).
#[cfg(feature = "rocks-pebbles")]
pub mod pebbles;
// The medium rocks' group of broken rock (the Asset Studio medium rocks
// session, column 23).
#[cfg(feature = "rocks-medium-rocks")]
pub mod medium_rocks;
// The jagged rock formation's blades, spires and rubble (the Asset Studio
// jagged rock formations session, column 24). The scree slopes' stones are
// its blades too (column 27): built for either, its formation only for its
// own asset.
#[cfg(any(feature = "rocks-jagged-rock-formations", feature = "rocks-scree-slopes", feature = "terrain-caves"))]
#[cfg_attr(not(feature = "rocks-jagged-rock-formations"), allow(dead_code))]
pub mod jagged_rocks;
// The caves' limestone passage (the Asset Studio caves session, column 29);
// its fallen stones are the jagged rock formation's broken stones.
#[cfg(feature = "terrain-caves")]
pub mod caves;
// The scree slopes' talus under its rock wall (the Asset Studio scree slopes
// session, column 27).
#[cfg(feature = "rocks-scree-slopes")]
pub mod scree;
// The rounded river rocks' group of water-polished stones (the Asset Studio
// rounded river rocks session, column 25).
#[cfg(feature = "rocks-rounded-river-rocks")]
pub mod river_rocks;
// The rock piles (the Asset Studio rock piles session, column 26).
#[cfg(feature = "rocks-rock-piles")]
pub mod rock_piles;
// The dark loam soil heap (the Asset Studio soil session, column 69).
#[cfg(feature = "material-soil-dark-loam")]
pub mod soil;
// The fresh powder snow heap (the Asset Studio fresh powder session, column
// 76).
#[cfg(feature = "material-snow-fresh-powder")]
pub mod snow;
#[cfg(feature = "cat-roads")]
pub mod roads;
#[cfg(feature = "cat-buildings")]
pub mod buildings;
#[cfg(feature = "cat-architecture")]
pub mod architecture;
#[cfg(feature = "cat-furniture")]
pub mod furniture;
#[cfg(feature = "cat-props")]
pub mod props;
#[cfg(feature = "cat-industrial")]
pub mod industrial;
#[cfg(feature = "cat-city")]
pub mod city;
#[cfg(feature = "cat-vehicles")]
pub mod vehicles;
#[cfg(feature = "cat-aircraft")]
pub mod aircraft;
#[cfg(feature = "cat-boats")]
pub mod boats;
#[cfg(feature = "cat-humans")]
pub mod humans;
#[cfg(feature = "cat-clothing")]
pub mod clothing;
#[cfg(feature = "cat-animals")]
pub mod animals;
#[cfg(feature = "cat-weapons")]
pub mod weapons;
pub mod template;
pub mod terrain;
// The red-rock canyons (the Asset Studio canyons session, column 12).
#[cfg(feature = "terrain-canyons")]
pub mod canyon;
// The photoreal mountain massif (the Asset Studio mountains session, column 19).
#[cfg(feature = "terrain-mountains")]
pub mod mountain;
// The photoreal office building (the Asset Studio office buildings session,
// column 13).
#[cfg(feature = "buildings-office-buildings")]
pub mod office;
// The photoreal terrace of townhouses (the Asset Studio townhouses session,
// column 37).
#[cfg(feature = "buildings-townhouses")]
pub mod townhouse;
// The photoreal NPC line-up (the Asset Studio full NPC body variations
// session, column 22).
#[cfg(feature = "humans-full-npc-body-variations")]
pub mod npc;
pub mod wrapped;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod realism_tests;
// The building-block graph (catalog/procedural/blocks.json): ids, whys, no cycles.
#[cfg(test)]
mod blocks_tests;
// The same graph, typed, as engine data (feature `compose`; written by
// tools/procedural/gen_blocks.mjs).
#[cfg(any(test, feature = "compose"))]
#[rustfmt::skip]
pub mod blocks_table;
#[cfg(test)]
mod compose_tests;

/// The world resource that assembles each placed asset from its typed
/// building-block links ([`evani_procedural::compose`]): add it with
/// `ctx.resource(evani_procedural_lib::composition())`.
#[cfg(feature = "compose")]
pub fn composition() -> evani_procedural::compose::Composition {
    evani_procedural::compose::Composition { graph: &blocks_table::GRAPH }
}

pub use evani_procedural as base;
use evani_procedural::{ProcModule, Registry};

#[cfg(feature = "host-grass")]
pub use evani_grass;
#[cfg(feature = "host-nature")]
pub use evani_material_nature;
#[cfg(feature = "host-sky")]
pub use evani_material_proc;

/// Every category and asset compiled into this build.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    generated::categories(&mut r);
    #[cfg(feature = "geometry-cube-cuboid")]
    r.add(Box::new(geometry::CubeCuboid));
    #[cfg(feature = "terrain-rolling-hills")]
    r.add(Box::new(terrain::RollingHills));
    // Batch A and B, the basic generators (one feature each).
    #[cfg(feature = "cat-geometry")]
    geometry::register(&mut r);
    #[cfg(feature = "cat-terrain")]
    terrain::register(&mut r);
    #[cfg(feature = "cat-rocks")]
    rocks::register(&mut r);
    #[cfg(feature = "cat-vegetation")]
    vegetation::register(&mut r);
    #[cfg(feature = "cat-trees")]
    trees::register(&mut r);
    #[cfg(feature = "cat-water")]
    water::register(&mut r);
    #[cfg(feature = "cat-sky")]
    sky::register(&mut r);
    #[cfg(feature = "cat-agriculture")]
    agriculture::register(&mut r);
    #[cfg(feature = "cat-textures")]
    textures::register(&mut r);
    #[cfg(feature = "cat-effects")]
    effects::register(&mut r);
    #[cfg(feature = "cat-roads")]
    roads::register(&mut r);
    #[cfg(feature = "cat-buildings")]
    buildings::register(&mut r);
    #[cfg(feature = "cat-architecture")]
    architecture::register(&mut r);
    #[cfg(feature = "cat-furniture")]
    furniture::register(&mut r);
    #[cfg(feature = "cat-props")]
    props::register(&mut r);
    #[cfg(feature = "cat-industrial")]
    industrial::register(&mut r);
    #[cfg(feature = "cat-city")]
    city::register(&mut r);
    #[cfg(feature = "cat-vehicles")]
    vehicles::register(&mut r);
    #[cfg(feature = "cat-aircraft")]
    aircraft::register(&mut r);
    #[cfg(feature = "cat-boats")]
    boats::register(&mut r);
    #[cfg(feature = "cat-humans")]
    humans::register(&mut r);
    #[cfg(feature = "cat-clothing")]
    clothing::register(&mut r);
    #[cfg(feature = "cat-animals")]
    animals::register(&mut r);
    #[cfg(feature = "cat-weapons")]
    weapons::register(&mut r);
    wrapped::register(&mut r);
    generated::placeholders(&mut r);
    r
}

/// The procedural module over [`registry`], hosting the wrapped modules
/// this build needs (the grass and the nature pass; the sky dome is the
/// kit's to install). Add it after the render module; an app that adds a
/// wrapped module itself says so with `.hosted_elsewhere(&["nature"])`.
pub fn module() -> ProcModule {
    #[allow(unused_mut)]
    let mut m = ProcModule::new(registry());
    #[cfg(feature = "host-nature")]
    {
        m = m.host("nature", Box::new(evani_material_nature::NatureModule::new()));
    }
    // The shoreline (a CPU asset) draws its water with the nature pass.
    #[cfg(feature = "water-shorelines")]
    {
        m = m.host_always("nature", Box::new(evani_material_nature::NatureModule::new()));
    }
    // So do the rivers (a CPU valley), and their banks' long grass is
    // evani-grass's.
    #[cfg(feature = "water-rivers")]
    {
        m = m.host_always("nature", Box::new(evani_material_nature::NatureModule::new()));
        m = m.host_always("grass", Box::new(evani_grass::GrassModule::new()));
    }
    // The mountains' meadows round a player: evani-grass (opt-in).
    #[cfg(feature = "mountain-grass")]
    {
        m = m.host_always("grass", Box::new(evani_grass::GrassModule::new()));
    }
    #[cfg(feature = "host-grass")]
    {
        m = m.host("grass", Box::new(evani_grass::GrassModule::new()));
    }
    // The volumetric clouds draw themselves: a ray-marched volume.
    #[cfg(feature = "sky-volumetric-looking-clouds")]
    {
        m = m.host_drawing("clouds", &[clouds::FEATURE], Box::new(clouds::CloudVolume::new()));
    }
    // So does the distance fog: one full-screen pass over the scene's depth.
    #[cfg(feature = "sky-distance-fog")]
    {
        m = m.host_drawing("fog", &[fog::FEATURE], Box::new(fog::DistanceFog::new()));
    }
    m
}
