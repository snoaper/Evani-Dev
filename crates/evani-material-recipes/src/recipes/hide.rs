//! Hide (recipe 15): skin as a surface (wrinkle normals, wetness, a wrap
//! scatter, blotches, bristles, thick plates). Shader: `shaders/rc_hide.wgsl` (feature
//! `recipe-hide`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The hide recipe.
pub const SCHEMA: Schema = Schema {
    code: 15,
    name: "hide",
    colours: &["colour", "wrinkle", "scatter"],
    variants: &["plain", "wrinkled", "warty", "slick", "plates"],
    numbers: &NUMBERS,
    extras: &["wrinkle_relief", "scatter", "bristles"],
    light: "lit colour, wrinkles painted",
};

/// The numbers n0 to n8, in block order. The last three default to their lowest value, so a block that does not set them is byte-for-byte what it was before they existed.
const NUMBERS: [Number; 9] = [
    num("wrinkle_depth", 0.0, 1.0, 0.5),
    num("wrinkle_scale", 1.0, 50.0, 10.0),
    num("wetness", 0.0, 1.0, 0.0),
    num("scatter", 0.0, 1.0, 0.2),
    num("rough", 0.0, 1.0, 0.6),
    num("spots", 0.0, 1.0, 0.0),
    num("bristles", 0.0, 1.0, 0.0),
    num("plate_scale", 1.0, 20.0, 1.0),
    num("bristle_scale", 10.0, 200.0, 10.0),
];
