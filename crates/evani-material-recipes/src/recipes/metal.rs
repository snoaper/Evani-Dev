//! Metal (recipe 1): plain, brushed, hammered and cast metal with edge polish
//! and engraving. Shader: `shaders/rc_metal.wgsl` (feature `recipe-metal`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The metal recipe.
pub const SCHEMA: Schema = Schema {
    code: 1,
    name: "metal",
    colours: &["base", "edge_polish", "oxide"],
    variants: &["plain", "brushed", "hammered", "cast", "diamond plate", "embossed", "grate", "chain link", "mesh", "damascus", "case colours", "spangle", "sequins"],
    numbers: &NUMBERS,
    extras: &["grain", "engraving", "relief", "studs", "seams", "spots", "temper"],
    light: "plain metal-rough",
};

/// The numbers n0 to n7, in block order (the
/// shader reads n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 10] = [
    num("rough", 0.0, 1.0, 0.3),
    num("metal", 0.0, 1.0, 1.0),
    num("aniso", 0.0, 1.0, 0.0),
    num("grain_scale", 1.0, 200.0, 40.0),
    num("grain_strength", 0.0, 1.0, 0.3),
    num("edge_polish", 0.0, 1.0, 0.5),
    num("engrave_depth", 0.0, 1.0, 0.0),
    num("engrave_scale", 1.0, 50.0, 8.0),
    num("layout_scale", 1.0, 20.0, 1.0),
    num("spot_amount", 0.0, 1.0, 0.0),
];
