//! Masonry (recipe 7): running bond, stack, cobble, setts, ashlar and flagstone. Shader:
//! `shaders/rc_masonry.wgsl` (feature `recipe-masonry`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The masonry recipe.
pub const SCHEMA: Schema = Schema {
    code: 7,
    name: "masonry",
    colours: &["unit", "mortar", "unit_tint"],
    variants: &["running bond", "stack", "cobble", "setts", "ashlar", "flagstone", "herringbone", "basket", "rubble", "drystone", "shingle"],
    numbers: &NUMBERS,
    extras: &["parallax", "unit_variation", "painted", "glaze", "flecks"],
    light: "flat pattern",
};

/// The numbers n0 to n8, in block order (the
/// shader reads n0-n3 as `na`, n4-n7 as `nb`, n8 as `nc.x`). `joint_cover`
/// (Asset Studio column 37, the townhouses): 0, the default, fades joints
/// under a pixel wide to their average; 1 draws each pixel's share of joint,
/// so far walls keep their mortar lines.
const NUMBERS: [Number; 9] = [
    num("unit_width", 0.05, 1.0, 0.22),
    num("unit_height", 0.03, 1.0, 0.07),
    num("joint_width", 0.002, 0.05, 0.01),
    num("unevenness", 0.0, 1.0, 0.2),
    num("rough", 0.0, 1.0, 0.8),
    num("variation", 0.0, 1.0, 0.4),
    num("joint_depth", 0.0, 1.0, 0.5),
    num("fleck", 0.0, 1.0, 0.0),
    num("joint_cover", 0.0, 1.0, 0.0),
];
