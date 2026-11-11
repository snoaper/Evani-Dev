//! Scales (recipe 14): fish, snake, lizard and crocodile skin: hex-lattice
//! scales cut by a round-to-hex shape (or aligned plates, or bumps), a
//! pattern (diamonds, blotches, bands, speckles), relief and a shimmer hue by
//! view angle. Shader: `shaders/rc_scales.wgsl` (feature `recipe-scales`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The scales recipe.
pub const SCHEMA: Schema = Schema {
    code: 14,
    name: "scales",
    colours: &["colour", "pattern", "shimmer_hue"],
    variants: &["overlapping", "hex", "plates", "bumps"],
    numbers: &NUMBERS,
    extras: &["relief", "shimmer"],
    light: "flat pattern",
};

/// The numbers n0 to n7, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`). `pattern_kind` (P7-2): 0 the pattern by pattern scale,
/// 1 rings, 2 stripes, 3 beans, 4 wavy lines.
const NUMBERS: [Number; 8] = [
    num("scale_size", 0.003, 0.2, 0.03),
    num("shape", 0.0, 1.0, 0.0),
    num("relief_depth", 0.0, 1.0, 0.5),
    num("pattern_scale", 1.0, 50.0, 6.0),
    num("pattern_contrast", 0.0, 1.0, 0.4),
    num("iridescence", 0.0, 1.0, 0.0),
    num("rough", 0.0, 1.0, 0.5),
    num("pattern_kind", 0.0, 4.0, 0.0),
];
