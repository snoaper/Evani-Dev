//! Fur (recipe 12): a coat painted from base to tip along the flow angle, a
//! pattern (stripes, spots, patches, rosettes, dapple), points and a
//! Kajiya-Kay-style sheen rim; no shells. Batch 5 (B5-P1) adds roan, merle,
//! mottle, crimp and reticulated and the number `mix`. Shader: `shaders/rc_fur.wgsl`
//! (feature `recipe-fur`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The fur recipe.
pub const SCHEMA: Schema = Schema {
    code: 12,
    name: "fur",
    colours: &["base", "tip", "pattern"],
    variants: &["solid", "stripes", "spots", "patches", "rosettes", "dapple", "roan", "merle", "mottle", "crimp", "reticulated"],
    numbers: &NUMBERS,
    extras: &["shells", "pattern"],
    light: "painted coat + sheen rim (batch 1 draws this on every tier)",
};

/// The numbers n0 to n8, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`, n8 and n9 as `nc`).
const NUMBERS: [Number; 10] = [
    num("length", 0.0, 1.0, 0.3),
    num("density", 0.0, 1.0, 0.7),
    num("sheen", 0.0, 1.0, 0.3),
    num("sheen_shift", 0.0, 1.0, 0.5),
    num("pattern_scale", 1.0, 50.0, 8.0),
    num("pattern_contrast", 0.0, 1.0, 0.5),
    num("flow_angle", 0.0, 360.0, 0.0),
    num("clump", 0.0, 1.0, 0.2),
    num("points", 0.0, 1.0, 0.0),
    num("mix", 0.0, 1.0, 0.0),
];
