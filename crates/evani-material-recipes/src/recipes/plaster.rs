//! Plaster (recipe 8): smooth, stucco, concrete and limewash. Shader:
//! `shaders/rc_plaster.wgsl` (feature `recipe-plaster`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The plaster recipe.
pub const SCHEMA: Schema = Schema {
    code: 8,
    name: "plaster",
    colours: &["colour", "crack_stain", "lath"],
    variants: &["smooth", "stucco", "concrete", "limewash", "polished", "cast"],
    numbers: &NUMBERS,
    extras: &["cracks", "trowel_relief", "chunks", "aggregate", "boards"],
    light: "flat colour with noise",
};

/// The numbers n0 to n9, in block order (the
/// shader reads n0-n3 as `na`, n4-n7 as `nb`, n8-n9 as `nc`). n8 and n9 are
/// cast concrete's stains and streaks (variant 5, the live concrete cube);
/// 0 by default, so every other row draws as before.
const NUMBERS: [Number; 10] = [
    num("rough", 0.0, 1.0, 0.85),
    num("trowel_amount", 0.0, 1.0, 0.3),
    num("trowel_scale", 1.0, 50.0, 6.0),
    num("crack_amount", 0.0, 1.0, 0.1),
    num("crack_scale", 1.0, 50.0, 4.0),
    num("aggregate", 0.0, 1.0, 0.1),
    num("chunk_amount", 0.0, 1.0, 0.0),
    num("aggregate_size", 0.005, 0.05, 0.005),
    num("stains", 0.0, 1.0, 0.0),
    num("streaks", 0.0, 1.0, 0.0),
];
