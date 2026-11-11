//! Ceramic (recipe 23): earthenware, stoneware, porcelain, terracotta, enamel
//! and tile. Shader: `shaders/rc_ceramic.wgsl` (feature `recipe-ceramic`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The ceramic recipe.
pub const SCHEMA: Schema = Schema {
    code: 23,
    name: "ceramic",
    colours: &["body", "glaze", "speck"],
    variants: &["earthenware", "stoneware", "porcelain", "terracotta", "enamel", "tile", "subway", "hex", "mosaic", "roof"],
    numbers: &NUMBERS,
    extras: &["crackle", "glaze_depth", "motif", "lustre"],
    light: "one gloss lobe",
};

/// The numbers n0 to n9, in block order (the
/// shader reads n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 10] = [
    num("glaze_gloss", 0.0, 1.0, 0.7),
    num("glaze_depth", 0.0, 1.0, 0.4),
    num("crackle_amount", 0.0, 1.0, 0.0),
    num("crackle_scale", 1.0, 100.0, 12.0),
    num("speck_amount", 0.0, 1.0, 0.0),
    num("drip", 0.0, 1.0, 0.0),
    num("tile_zoom", 1.0, 8.0, 1.0),
    num("tile_ratio", 0.5, 4.0, 0.5),
    num("barrel", 0.0, 1.0, 0.0),
    num("lustre", 0.0, 1.0, 0.0),
];
