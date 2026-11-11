//! Plastic (recipe 3): plain, rubber, foam and tread plastic with a translucent wrap, specks and tyre tread. Shader: `shaders/rc_plastic.wgsl` (feature `recipe-plastic`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The plastic recipe.
pub const SCHEMA: Schema = Schema {
    code: 3,
    name: "plastic",
    colours: &["colour", "speck"],
    variants: &["plain", "rubber", "foam", "tread", "sheet"],
    numbers: &NUMBERS,
    extras: &["translucency", "specks", "tread"],
    light: "plain lit colour",
};

/// The numbers n0 to n7, in block order (the shader reads
/// n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 8] = [
    num("rough", 0.0, 1.0, 0.45),
    num("translucency", 0.0, 1.0, 0.0),
    num("speck_amount", 0.0, 1.0, 0.0),
    num("speck_scale", 1.0, 200.0, 60.0),
    num("tread_scale", 1.0, 64.0, 12.0),
    num("tread_depth", 0.0, 1.0, 0.0),
    num("mottle", 0.0, 1.0, 0.0),
    num("seam_scale", 0.0, 16.0, 0.0),
];
