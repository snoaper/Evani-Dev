//! Foliage (recipe 20, batch 2 package B2-P1): leaf and frond cards with
//! veins, needle sprays with berry specks, grass and straw strands, turf and
//! fallen litter seen from above (world space), and moss cushions; dry edges,
//! a pale or rusty underside, a waxy gloss and the sun shining through. It is
//! a recipe-stage recipe, so rain, snow and the other condition layers run on
//! it; the nature module's placed back-light stays available beside it (the
//! adapter still writes the nature kind). Shader: `shaders/rc_foliage.wgsl`
//! (feature `recipe-foliage`).
//!
//! Two numbers mean a second thing on some variants: `thickness` is the blade
//! width for turf and litter (above 0.66 a turf blade is a three-leaflet
//! clover), and `bare` is the share of berry specks (colour 2) on needles.
//! `fan` (material-view round 4) turns a leaf's veins from a midrib with
//! side veins (0) to five main veins fanning from the stalk (1, maple and
//! plane leaves); a stored 0 keeps every older block as it was. `patches`
//! (procedural realism round 6) lays dry-grass and worn-soil patches of a
//! few metres over turf, more soil on slopes, smooth in the shader; 0 (every
//! older block) draws none.
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The foliage recipe.
pub const SCHEMA: Schema = Schema {
    code: 20,
    name: "foliage",
    colours: &["colour_1", "colour_2", "back_light"],
    variants: &["leaf", "needle", "grass", "straw", "turf", "moss", "litter"],
    numbers: &NUMBERS,
    extras: &["back_light", "veins", "bare"],
    light: "flat, no back-light",
};

/// The numbers n0 to n8, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`, n8 as `nc.x`, n9 as `nc.y`). n0 and n1 kept their batch-1 slots.
const NUMBERS: [Number; 10] = [
    num("variation", 0.0, 1.0, 0.3),
    num("thickness", 0.0, 1.0, 0.3),
    num("vein", 0.0, 1.0, 0.5),
    num("gloss", 0.0, 1.0, 0.2),
    num("edge_dry", 0.0, 1.0, 0.0),
    num("blade_scale", 2.0, 200.0, 30.0),
    num("bare", 0.0, 1.0, 0.0),
    num("underside", 0.0, 1.0, 0.0),
    num("fan", 0.0, 1.0, 0.0),
    num("patches", 0.0, 1.0, 0.0),
];
