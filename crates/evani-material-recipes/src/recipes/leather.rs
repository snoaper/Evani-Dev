//! Leather (recipe 11): pebbled grain, F2-F1 creases, suede nap, tooling,
//! embossed scales and crazed cracks.
//! Shader: `shaders/rc_leather.wgsl` (feature `recipe-leather`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The leather recipe.
pub const SCHEMA: Schema = Schema {
    code: 11,
    name: "leather",
    colours: &["colour", "crease"],
    variants: &["smooth", "suede", "rawhide", "tooled", "embossed", "tufted"],
    numbers: &NUMBERS,
    extras: &["crease_relief", "nap", "crazed"],
    light: "flat with soft sheen",
};

/// The numbers n0 to n7, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`; until P8-4 it read craze scale from `nc.x`, an empty slot).
const NUMBERS: [Number; 8] = [
    num("rough", 0.0, 1.0, 0.55),
    num("grain_scale", 1.0, 200.0, 60.0),
    num("grain_depth", 0.0, 1.0, 0.4),
    num("crease_amount", 0.0, 1.0, 0.3),
    num("nap", 0.0, 1.0, 0.0),
    num("wear_lighten", 0.0, 1.0, 0.0),
    num("tooling", 0.0, 1.0, 0.0),
    num("craze_scale", 1.0, 50.0, 1.0),
];
