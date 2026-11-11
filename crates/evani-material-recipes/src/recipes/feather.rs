//! Feather (recipe 13): plumage as rows of feathers with a shaft, barb
//! stripes, barring, speckles or an eye, and an iridescent hue by view angle;
//! P7-2 adds loose plumes, scalloped edges and the single quill.
//! Shader: `shaders/rc_feather.wgsl` (feature `recipe-feather`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The feather recipe.
pub const SCHEMA: Schema = Schema {
    code: 13,
    name: "feather",
    colours: &["colour", "barring", "iridescent_hue"],
    variants: &["plain", "barred", "speckled", "eyed", "plume", "scalloped", "quill"],
    numbers: &NUMBERS,
    extras: &["iridescence", "edge_relief"],
    light: "flat barred colour",
};

/// The numbers n0 to n5, in block order (the shader reads n0-n3 as `na`,
/// n4-n5 as `nb`).
const NUMBERS: [Number; 6] = [
    num("feather_size", 0.005, 0.3, 0.05),
    num("barring", 0.0, 1.0, 0.3),
    num("barb_scale", 10.0, 400.0, 100.0),
    num("iridescence", 0.0, 1.0, 0.0),
    num("rough", 0.0, 1.0, 0.6),
    num("edge_relief", 0.0, 1.0, 0.3),
];
