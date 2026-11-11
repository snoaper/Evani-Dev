//! The realism presets against the material catalogue: every preset in
//! `evani_procedural::surface::presets` must be its catalogue row's block
//! and PBR factors exactly (the catalogue's own encoder), so a preset never
//! drifts from the row the Material library shows.

use evani_material_recipes::catalog::Catalog;
use evani_material_recipes::gltf::gltf_material;
use evani_procedural::surface::{Surface, presets as p};

/// Every preset and its catalogue id.
const PRESETS: &[(&str, &str, Surface)] = &[
    ("CONCRETE_SMOOTH", "concrete.smooth", p::CONCRETE_SMOOTH),
    ("CONCRETE_POLISHED", "concrete.polished", p::CONCRETE_POLISHED),
    ("TIMBER_OAK", "timber.oak", p::TIMBER_OAK),
    ("WOOD_FLOOR_OAK", "wood.floor.oak", p::WOOD_FLOOR_OAK),
    ("WOOD_BEAM_OAK", "wood.beam.oak", p::WOOD_BEAM_OAK),
    ("ROCK_GRANITE_GREY", "rock.granite.grey", p::ROCK_GRANITE_GREY),
    ("ROCK_LIMESTONE", "rock.limestone", p::ROCK_LIMESTONE),
    ("ROCK_BASALT", "rock.basalt", p::ROCK_BASALT),
    ("ROCK_SANDSTONE_BUFF", "rock.sandstone.buff", p::ROCK_SANDSTONE_BUFF),
    ("ROCK_SLATE", "rock.slate", p::ROCK_SLATE),
    ("ROCK_CAVE_WALL", "rock.cave_wall", p::ROCK_CAVE_WALL),
    ("ROCK_MOSSY_BOULDER", "rock.mossy_boulder", p::ROCK_MOSSY_BOULDER),
    ("GROUND_SOIL_LOAM", "ground.soil.loam", p::GROUND_SOIL_LOAM),
    ("GROUND_SOIL_PACKED", "ground.soil.packed", p::GROUND_SOIL_PACKED),
    ("GROUND_SOIL_FOREST_FLOOR", "ground.soil.forest_floor", p::GROUND_SOIL_FOREST_FLOOR),
    ("GROUND_SOIL_CRACKED_DRY", "ground.soil.cracked_dry", p::GROUND_SOIL_CRACKED_DRY),
    ("GROUND_SAND_DESERT", "ground.sand.desert", p::GROUND_SAND_DESERT),
    ("GROUND_SAND_BEACH", "ground.sand.beach", p::GROUND_SAND_BEACH),
    ("GROUND_SAND_WET", "ground.sand.wet", p::GROUND_SAND_WET),
    ("GROUND_GRAVEL_SCREE", "ground.gravel.scree", p::GROUND_GRAVEL_SCREE),
    ("GROUND_GRAVEL_CREEK", "ground.gravel.creek", p::GROUND_GRAVEL_CREEK),
    ("GROUND_GRASS_MEADOW", "ground.grass.meadow", p::GROUND_GRASS_MEADOW),
    ("GROUND_GRASS_LAWN", "ground.grass.lawn", p::GROUND_GRASS_LAWN),
    ("GROUND_GRASS_ALPINE", "ground.grass.alpine", p::GROUND_GRASS_ALPINE),
    ("GROUND_MOSS_CARPET", "ground.moss.carpet", p::GROUND_MOSS_CARPET),
    ("GROUND_SNOW_PACKED", "ground.snow.packed", p::GROUND_SNOW_PACKED),
    ("GROUND_SNOW_FRESH", "ground.snow.fresh", p::GROUND_SNOW_FRESH),
    ("PLASTER_WHITE", "plaster.white", p::PLASTER_WHITE),
    ("METAL_STEEL_BRUSHED", "metal.steel.brushed", p::METAL_STEEL_BRUSHED),
    ("METAL_STEEL_POLISHED", "metal.steel.polished", p::METAL_STEEL_POLISHED),
    ("WALL_PAINT_MATTE", "wall.paint.matte", p::WALL_PAINT_MATTE),
    ("FINISH_PAINTED_CREAM", "finish.painted.cream", p::FINISH_PAINTED_CREAM),
    ("ROCK_CHALK", "rock.chalk", p::ROCK_CHALK),
    ("ROCK_SHALE", "rock.shale", p::ROCK_SHALE),
    ("ROCK_SANDSTONE_RED", "rock.sandstone.red", p::ROCK_SANDSTONE_RED),
    ("GROUND_SOIL_CLAY_RED", "ground.soil.clay_red", p::GROUND_SOIL_CLAY_RED),
    ("GROUND_SOIL_ASH_VOLCANIC", "ground.soil.ash_volcanic", p::GROUND_SOIL_ASH_VOLCANIC),
    ("BARK_OAK", "bark.oak", p::BARK_OAK),
    ("LEAF_OAK", "leaf.oak", p::LEAF_OAK),
    ("GLASS_WINDOW_CLEAR", "glass.window.clear", p::GLASS_WINDOW_CLEAR),
    ("METAL_ALUMINIUM", "metal.aluminium", p::METAL_ALUMINIUM),
    ("GLOW_BULB_WHITE", "glow.bulb.white", p::GLOW_BULB_WHITE),
    ("GLOW_LAMP_WARM", "glow.lamp.warm", p::GLOW_LAMP_WARM),
    ("CLOTH_KNIT_JERSEY", "cloth.knit.jersey", p::CLOTH_KNIT_JERSEY),
    ("BRICK_RED_COMMON", "brick.red.common", p::BRICK_RED_COMMON),
];

/// The row's block hex, roughness and metallic as the catalogue writes them.
fn row(cat: &Catalog, id: &str) -> (String, f32, f32) {
    let r = cat.row(id).unwrap_or_else(|| panic!("no catalogue row {id}"));
    let m = gltf_material(cat, r).unwrap_or_else(|e| panic!("{id}: {e}"));
    let hex = m["extras"]["evani_recipe"].as_str().unwrap_or_else(|| panic!("{id} has no block")).to_string();
    let pbr = &m["pbrMetallicRoughness"];
    (hex, pbr["roughnessFactor"].as_f64().unwrap_or(1.0) as f32, pbr["metallicFactor"].as_f64().unwrap_or(0.0) as f32)
}

#[test]
fn surface_presets_match_the_catalogue() {
    let cat = Catalog::bundled().expect("the bundled catalogue");
    let mut wrong = Vec::new();
    for (name, id, s) in PRESETS {
        let (hex, rough, metal) = row(&cat, id);
        if s.block_hex() != hex || (s.roughness - rough).abs() > 1e-3 || (s.metallic - metal).abs() > 1e-3 {
            wrong.push(format!("    pub const {name}: Surface = Surface::hex(\"{hex}\", {rough}, {metal}); // {id}"));
        }
    }
    assert!(wrong.is_empty(), "presets that differ from the catalogue (the right lines):\n{}", wrong.join("\n"));
}

/// Prints catalogue rows as preset lines (run with `--ignored --nocapture`
/// to add a preset): `PRESET_IDS=a,b cargo test ... print_preset_lines`.
#[test]
#[ignore]
fn print_preset_lines() {
    let cat = Catalog::bundled().expect("the bundled catalogue");
    let ids = std::env::var("PRESET_IDS").unwrap_or_default();
    for id in ids.split(',').filter(|s| !s.is_empty()) {
        let (hex, rough, metal) = row(&cat, id);
        let name = id.replace('.', "_").to_uppercase();
        println!("    pub const {name}: Surface = Surface::hex(\"{hex}\", {rough}, {metal}); // {id}");
    }
}

/// Every preview form builds closed-off parts with real triangles, its
/// base on the ground, at least one part in the finish, and slider value 0
/// (the asset's own shape) builds none.
#[test]
fn preview_forms_build_on_the_ground() {
    use evani_procedural::forms::{FORMS, form};
    assert!(form(0.0, 0.3, 1).is_none(), "0 is the asset's own shape");
    for (k, name) in FORMS.iter().enumerate().skip(1) {
        let parts = form(k as f32, 0.3, 7).unwrap_or_default();
        assert!(parts.iter().any(|(_, fin)| *fin), "{name}: no part in the finish");
        let mut lo = f32::MAX;
        for (m, _) in &parts {
            assert!(m.indices.len() >= 3 && m.indices.iter().all(|&i| (i as usize) < m.vertices.len()), "{name}: bad mesh");
            lo = m.vertices.iter().fold(lo, |l, v| l.min(v[1]));
        }
        assert!(lo.abs() < 0.03, "{name}: base at {lo}, not on the ground");
    }
}

/// Round 8: the eight realism assets' triangles at their defaults (printed
/// for the progress log; run with --nocapture), each within a light budget
/// a phone draws (the carved crack is the cube's largest share).
#[test]
fn realism_assets_stay_light() {
    use evani_procedural::{Output, Values};
    let reg = crate::registry();
    for f in ["geometry-cube-cuboid", "geometry-bevelled-box", "geometry-uv-sphere", "geometry-icosphere", "geometry-cylinder", "geometry-cone", "terrain-flat-terrain", "terrain-rolling-hills", "rocks-pebbles", "rocks-medium-rocks", "rocks-rounded-river-rocks"] {
        let Some(a) = reg.get(f) else { continue };
        let specs = evani_procedural::Registry::specs(a);
        // Round 9: tiered mesh density; the carved relief is refined less on
        // the lower tiers (Low's density here), never more than High's.
        let count = |d: f32| match a.generate(&Values::defaults(&specs).with_density(d), 1, evani_procedural::REALISM) {
            Output::Mesh(m) => m.indices.len() / 3,
            Output::Parts(p) => p.iter().map(|x| x.mesh.indices.len() / 3).sum(),
            Output::Module => 0,
        };
        let t0 = std::time::Instant::now();
        let (tris, low) = (count(1.0), count(evani_procedural::detail::Density::for_tier("low")));
        println!("{f}: {tris} triangles at High, {low} at Low ({} ms for both)", t0.elapsed().as_millis());
        assert!(tris > 0 && tris < 40_000, "{f}: {tris} triangles");
        assert!(low <= tris, "{f}: Low {low} over High {tris}");
    }
}

/// The concrete cube builds at High inside the 40k budget and quickly for
/// any seed (a slow chip refinement froze the live page once; before the
/// Asset Studio session's round 1, seeds 2 and 7 refined past 40k).
#[test]
fn concrete_cube_view_seed_builds() {
    use evani_procedural::{Output, Values};
    let reg = crate::registry();
    let Some(a) = reg.get("geometry-cube-cuboid") else { return };
    let specs = evani_procedural::Registry::specs(a);
    for seed in [1u64, 2, 3, 4, 5, 6, 7, 8] {
        let t0 = std::time::Instant::now();
        let tris = match a.generate(&Values::defaults(&specs).with_density(1.0), seed, evani_procedural::REALISM) {
            Output::Mesh(m) => m.indices.len() / 3,
            Output::Parts(p) => p.iter().map(|x| x.mesh.indices.len() / 3).sum(),
            Output::Module => 0,
        };
        let ms = t0.elapsed().as_millis();
        println!("cube seed {seed}: {tris} triangles at High in {ms} ms");
        assert!(tris > 0 && tris < 40_000, "seed {seed}: {tris} triangles");
    }
}

/// The photoreal oak (the Asset Studio oak session) is a hero tree: its own
/// budget, 60k triangles at High and 30k at Low (a phone's tier), with the
/// default single tree.
#[test]
fn the_oak_stays_in_its_budget() {
    use evani_procedural::{Output, Values};
    let reg = crate::registry();
    let Some(a) = reg.get("trees-oak-trees") else { return };
    let specs = evani_procedural::Registry::specs(a);
    let count = |d: f32| match a.generate(&Values::defaults(&specs).with_density(d), 1, evani_procedural::REALISM) {
        Output::Parts(p) => p.iter().map(|x| x.mesh.indices.len() / 3).sum(),
        _ => 0,
    };
    let (tris, low) = (count(1.0), count(evani_procedural::detail::Density::for_tier("low")));
    println!("trees-oak-trees: {tris} triangles at High, {low} at Low");
    assert!(tris > 0 && tris < 60_000, "{tris} triangles at High");
    assert!(low < 30_000 && low <= tris, "{low} triangles at Low");
}
