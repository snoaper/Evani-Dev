//! Paint (recipe 2): the recipe stage draws paint's extras (metal flake, colour-shift film and carbon twill); the clear coat is the layers family's. Shader: `shaders/rc_paint.wgsl` (feature `recipe-paint`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The paint recipe.
pub const SCHEMA: Schema = Schema {
    code: 2,
    name: "paint",
    colours: &["base", "under_layer", "flake"],
    variants: &["solid", "metallic", "pearl", "colour-shift", "matte", "carbon"],
    numbers: &NUMBERS,
    extras: &["clearcoat", "flake", "film", "twill", "studs"],
    light: "one gloss lobe, no flakes",
};

/// The numbers n0 to n7, in block order (the shader reads
/// n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 9] = [
    num("rough", 0.0, 1.0, 0.35),
    num("coat", 0.0, 1.0, 0.8),
    num("coat_rough", 0.0, 1.0, 0.05),
    num("flake_amount", 0.0, 1.0, 0.0),
    num("flake_size", 0.2, 5.0, 1.0),
    num("film", 0.0, 1.0, 0.0),
    num("film_shift", 0.0, 1.0, 0.5),
    num("twill_scale", 1.0, 64.0, 16.0),
    num("stud_scale", 1.0, 20.0, 1.0),
];
