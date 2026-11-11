//! Organic (recipe 22): flesh, bread, fruit, slime, mushroom, kernels and pastry with wetness, scatter, crumb and marbling. Shader: `shaders/rc_organic.wgsl` (feature `recipe-organic`); flesh grain, kernels and the pastry lattice are its food part (`shaders/rc_organic_food.wgsl`, feature `recipe-organic-food`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The organic recipe.
pub const SCHEMA: Schema = Schema {
    code: 22,
    name: "organic",
    colours: &["colour_1", "colour_2"],
    variants: &["flesh", "bread", "fruit", "slime", "mushroom", "kernels", "pastry"],
    numbers: &NUMBERS,
    extras: &["scatter", "clear"],
    light: "flat colour",
};

/// The numbers n0 to n5, in block order (the shader reads
/// n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 6] = [
    num("wetness", 0.0, 1.0, 0.3),
    num("scatter", 0.0, 1.0, 0.3),
    num("crumb_amount", 0.0, 1.0, 0.0),
    num("crumb_scale", 1.0, 100.0, 20.0),
    num("rough", 0.0, 1.0, 0.6),
    num("marbling", 0.0, 1.0, 0.0),
];
