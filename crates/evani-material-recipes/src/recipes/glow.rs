//! Glow (recipe 29): lamps, neon, screens, runes, holograms and energy, emitted from colour A with a pulse. Shader: `shaders/rc_glow.wgsl` (feature `recipe-glow`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The glow recipe.
pub const SCHEMA: Schema = Schema {
    code: 29,
    name: "glow",
    colours: &["colour", "rim_scanline"],
    variants: &["lamp", "neon", "screen", "rune", "hologram", "energy"],
    numbers: &NUMBERS,
    extras: &["pulse", "hologram"],
    light: "plain emissive",
};

/// The numbers n0 to n6, in block order (the shader reads
/// n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 7] = [
    num("intensity", 0.0, 8.0, 1.0),
    num("pulse_speed", 0.0, 10.0, 1.0),
    num("pulse_amount", 0.0, 1.0, 0.0),
    num("scanline_density", 0.0, 400.0, 0.0),
    num("scanline_speed", 0.0, 10.0, 1.0),
    num("flicker", 0.0, 1.0, 0.0),
    num("rim", 0.0, 1.0, 0.0),
];
