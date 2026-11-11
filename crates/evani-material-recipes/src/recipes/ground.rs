//! Ground (recipe 9): soil, sand, gravel, mud and road. Shader:
//! `shaders/rc_ground.wgsl` (feature `recipe-ground`).
//!
//! Crumb (Asset Studio column 69, dark loam): on the soil variant, the
//! `cracks` extra with `clump` over 0.8 breaks the cracked plates up into
//! fine porous earth, a pattern in space (lumps, crumbs, toned grains and
//! dark pores from `crack_scale` down; `clump` at 1 lies loose, just over
//! 0.8 is a clod's compact face). No new number or extra: every catalogue
//! row draws as before (the cracked soils' clump is 0.75 at most).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The ground recipe.
pub const SCHEMA: Schema = Schema {
    code: 9,
    name: "ground",
    colours: &["soil", "stone", "sand_tint"],
    variants: &["soil", "sand", "gravel", "mud", "road", "crushed"],
    numbers: &NUMBERS,
    extras: &["parallax_pebbles", "sparkle", "cracks", "furrows", "cover"],
    light: "flat noise",
};

/// The numbers n0 to n9, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`, n8-n9 as `nc`). B2 added n7 to n9; their defaults sit at the
/// bottom of their ranges, so every earlier row encodes to the same block.
const NUMBERS: [Number; 10] = [
    num("rough", 0.0, 1.0, 0.9),
    num("stone_size", 0.005, 0.5, 0.05),
    num("stone_amount", 0.0, 1.0, 0.2),
    num("ripple_amount", 0.0, 1.0, 0.0),
    num("ripple_scale", 1.0, 50.0, 8.0),
    num("sparkle", 0.0, 1.0, 0.0),
    num("clump", 0.0, 1.0, 0.3),
    num("crack_scale", 1.0, 50.0, 1.0),
    num("furrow_scale", 1.0, 50.0, 1.0),
    num("cover", 0.0, 1.0, 0.0),
];
