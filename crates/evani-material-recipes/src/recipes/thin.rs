//! Thin (recipe 21): paper, wax, petal, membrane and lampshade (their light
//! through stays the nature pass's) and, batch 3, wallpaper: stripes, a
//! painted motif, fading and peeling. Shader: `shaders/rc_thin.wgsl` (feature
//! `recipe-thin`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The thin recipe.
pub const SCHEMA: Schema = Schema {
    code: 21,
    name: "thin",
    colours: &["colour", "light_through", "under"],
    variants: &["paper", "wax", "petal", "membrane", "lampshade", "wallpaper", "newsprint", "lace", "wing", "segments"],
    numbers: &NUMBERS,
    extras: &["stripes", "motif", "peel", "damask", "film"],
    light: "flat colour",
};

/// The numbers n0 to n7, in block order (the shader reads
/// n0-n3 as `na`, n4-n7 as `nb`; P7-2 added `dots`, `film` and `cross`).
const NUMBERS: [Number; 8] = [
    num("thickness", 0.0, 1.0, 0.3),
    num("pattern", 0.0, 1.0, 0.0),
    num("motif_scale", 1.0, 32.0, 1.0),
    num("peel", 0.0, 1.0, 0.0),
    num("fade", 0.0, 1.0, 0.0),
    num("dots", 0.0, 1.0, 0.0),
    num("film", 0.0, 1.0, 0.0),
    num("cross", 0.0, 1.0, 0.0),
];
