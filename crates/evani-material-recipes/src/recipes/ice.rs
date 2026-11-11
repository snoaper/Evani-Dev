//! Ice (recipe 27): clear and blue ice, snow, frost and icicles, as a surface
//! with no refraction: a blue-white ramp by view angle, cracks inside the ice
//! (a second deeper layer with parallax at High), a frost cover, sparkle specks
//! and a wrap scatter. Shader: `shaders/rc_ice.wgsl` (feature `recipe-ice`).
//!
//! Fresh powder (Asset Studio column 76) has no number or extra of its own:
//! the frost variant with clarity 0, frost 1 and the `inner_cracks` extra at
//! a crack amount over 0.8 breaks the cover up into loose crystals (soft
//! lumps, packed crystals that each lie their own way, cool hollows, the
//! relief's own shade and glints; the crack scale is its lumps per metre).
//! No catalogue row has that, so every row draws as it did;
//! `evani_procedural_lib::snow::surface` is that paint.
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The ice recipe.
pub const SCHEMA: Schema = Schema {
    code: 27,
    name: "ice",
    colours: &["colour", "deep", "rim"],
    variants: &["clear", "blue", "frost", "icicle"],
    numbers: &NUMBERS,
    extras: &["inner_cracks", "sparkle", "scatter", "ridges"],
    light: "white with a blue rim",
};

/// The numbers n0 to n5, in block order (the shader reads n0-n3 as `na`,
/// n4-n5 as `nb`).
const NUMBERS: [Number; 6] = [
    num("clarity", 0.0, 1.0, 0.6),
    num("crack_amount", 0.0, 1.0, 0.3),
    num("crack_scale", 1.0, 50.0, 8.0),
    num("sparkle", 0.0, 1.0, 0.3),
    num("scatter", 0.0, 1.0, 0.3),
    num("frost", 0.0, 1.0, 0.0),
];
