//! Glass (recipe 24, batch 3 build plan 3.3): window, bottle and crystal
//! glass, stained glass glowing when backlit, leaded diamond panes, wired
//! safety glass and glass blocks, drawn see-through by the blend pass (B3-P3;
//! frosted, wavy and block glass bend and blur what is behind). Shader:
//! `shaders/rc_glass.wgsl` (feature `recipe-glass`). The nature family stays
//! its `also`: a game that lists it keeps its placed panes' real refraction.
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The glass recipe.
pub const SCHEMA: Schema = Schema {
    code: 24,
    name: "glass",
    colours: &["tint", "came"],
    variants: &["window", "bottle", "crystal", "stained", "leaded", "wired", "block"],
    numbers: &NUMBERS,
    extras: &["backlight"],
    light: "flat tinted see-through",
};

/// The numbers n0 to n5, in block order (the shader reads n0-n3 as `na`,
/// n4 and n5 as `nb.x` and `nb.y`). The first three are batch 1's, unchanged.
const NUMBERS: [Number; 6] = [
    num("thickness", 0.0, 1.0, 0.3),
    num("frost", 0.0, 1.0, 0.0),
    num("waviness", 0.0, 1.0, 0.0),
    num("pane_scale", 1.0, 40.0, 6.0),
    num("backlight", 0.0, 1.0, 0.5),
    num("swirl", 0.0, 1.0, 0.0),
];
