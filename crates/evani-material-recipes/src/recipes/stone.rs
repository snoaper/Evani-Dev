//! Stone (recipe 6): granular, veined, layered and ore stone in world space.
//! Shader: `shaders/rc_stone.wgsl` (feature `recipe-stone`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The stone recipe.
pub const SCHEMA: Schema = Schema {
    code: 6,
    name: "stone",
    colours: &["colour_1", "colour_2", "vein"],
    variants: &["granular", "veined", "layered", "ore", "porous", "ropy", "clastic", "flowstone", "terrazzo", "crystalline", "bedded", "water-worn"],
    numbers: &NUMBERS,
    extras: &["parallax", "glints", "translucency", "tiles"],
    light: "flat noise",
};

/// The numbers n0 to n9, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`, n8-n9 as `nc`). B2 added n7 (`pits`), batch 3 n8 and n9
/// (`tile_size`, `chequer`).
const NUMBERS: [Number; 10] = [
    num("rough", 0.0, 1.0, 0.75),
    num("vein_amount", 0.0, 1.0, 0.2),
    num("vein_scale", 1.0, 50.0, 6.0),
    num("grain_scale", 1.0, 200.0, 40.0),
    num("layering", 0.0, 1.0, 0.0),
    num("glints", 0.0, 1.0, 0.0),
    num("edge_translucency", 0.0, 1.0, 0.0),
    num("pits", 0.0, 1.0, 0.0),
    num("tile_size", 0.1, 2.0, 0.1),
    num("chequer", 0.0, 1.0, 0.0),
];
