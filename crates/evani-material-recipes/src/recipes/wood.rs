//! Wood (recipe 4): flat-sawn, quarter-sawn and end grain, and staggered planks
//! with gaps; solid wood (variants 15 and 16: the rings of a log's volume, procedural
//! realism round 8). Shader: `shaders/rc_wood.wgsl` (feature `recipe-wood`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The wood recipe.
pub const SCHEMA: Schema = Schema {
    code: 4,
    name: "wood",
    colours: &["early_grain", "late_grain", "knot"],
    variants: &["flat-sawn", "quarter-sawn", "end grain", "planks", "clapboard", "board and batten", "logs", "shingle", "parquet", "panels", "culm", "granular", "lattice", "charred", "marquetry", "solid", "solid (end grain)"],
    numbers: &NUMBERS,
    extras: &["grain_relief", "plank_gaps", "studs", "painted", "adze"],
    light: "flat two-tone grain",
};

/// The numbers n0 to n7, in block order (the
/// shader reads n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 10] = [
    num("rough", 0.0, 1.0, 0.6),
    num("grain_scale", 1.0, 100.0, 20.0),
    num("ring_warp", 0.0, 1.0, 0.3),
    num("ring_contrast", 0.0, 1.0, 0.5),
    num("knot_amount", 0.0, 1.0, 0.2),
    num("plank_width", 0.05, 1.0, 0.2),
    num("gap_depth", 0.0, 1.0, 0.3),
    num("fleck", 0.0, 1.0, 0.2),
    num("paint_peel", 0.0, 1.0, 0.0),
    num("opening", 0.0, 0.9, 0.0),
];
