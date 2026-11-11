//! Bark (recipe 5): furrowed, plated, smooth and peeling bark. Shader:
//! `shaders/rc_bark.wgsl` (feature `recipe-bark`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The bark recipe.
pub const SCHEMA: Schema = Schema {
    code: 5,
    name: "bark",
    colours: &["bark", "furrow", "patch"],
    variants: &["furrowed", "plated", "smooth", "peeling", "fibrous", "ringed", "banded"],
    numbers: &NUMBERS,
    extras: &["parallax", "marks", "embers"],
    light: "flat furrow stripes",
};

/// The numbers n0 to n7, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`). B2 added n6 (`cord_twist`, default at the bottom of its range);
/// the Asset Studio oak added n7 (`far_detail`: 0, every bark before it, fades
/// furrows under about 15 px to their average; 1 draws them down to about
/// 4 px, so a trunk seen across a field stays finely fissured).
const NUMBERS: [Number; 8] = [
    num("rough", 0.0, 1.0, 0.85),
    num("furrow_depth", 0.0, 1.0, 0.6),
    num("furrow_scale", 1.0, 50.0, 8.0),
    num("plate_size", 0.02, 1.0, 0.2),
    num("stretch", 0.5, 4.0, 1.0),
    num("peel", 0.0, 1.0, 0.0),
    num("cord_twist", 0.0, 1.0, 0.0),
    num("far_detail", 0.0, 1.0, 0.0),
];
