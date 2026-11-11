//! Fire (recipe 28): flame, embers, lava, smoke and steam from scrolling noise, flickering with sim time. Shader: `shaders/rc_fire.wgsl` (feature `recipe-fire`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The fire recipe.
pub const SCHEMA: Schema = Schema {
    code: 28,
    name: "fire",
    colours: &["core", "edge", "smoke"],
    variants: &["flame", "embers", "lava", "smoke", "steam"],
    numbers: &NUMBERS,
    extras: &["flicker"],
    light: "static glowing card",
};

/// The numbers n0 to n5, in block order (the shader reads
/// n0-n3 as `na`, n4-n7 as `nb`).
const NUMBERS: [Number; 6] = [
    num("intensity", 0.0, 4.0, 1.0),
    num("flicker_speed", 0.0, 20.0, 6.0),
    num("flicker_amount", 0.0, 1.0, 0.4),
    num("softness", 0.0, 1.0, 0.5),
    num("scroll_speed", 0.0, 5.0, 1.0),
    num("embers", 0.0, 1.0, 0.0),
];
