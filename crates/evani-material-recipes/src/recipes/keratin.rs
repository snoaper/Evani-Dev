//! Keratin (recipe 19): horn, bone, antler, teeth, shell and beetle casing,
//! hoof, turtle scutes and armadillo bands (growth rings, edge translucency, a
//! nacre hue shift, stripes); P7-2: snail spirals and round spots. Shader:
//! `shaders/rc_keratin.wgsl` (feature `recipe-keratin`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The keratin recipe.
pub const SCHEMA: Schema = Schema {
    code: 19,
    name: "keratin",
    colours: &["colour", "ring_edge"],
    variants: &["horn", "bone", "antler", "tooth", "shell", "beetle", "hoof", "scutes", "banded", "spiral"],
    numbers: &NUMBERS,
    extras: &["edge_translucency", "nacre", "spots"],
    light: "flat glossy colour",
};

/// The numbers n0 to n6, in block order (the shader reads n0-n3 as `na`,
/// n4 to n6 as `nb.x` to `nb.z`; P7-2 added `spot_scale`).
const NUMBERS: [Number; 7] = [
    num("translucency", 0.0, 1.0, 0.2),
    num("rings", 0.0, 1.0, 0.3),
    num("ring_scale", 1.0, 100.0, 20.0),
    num("rough", 0.0, 1.0, 0.5),
    num("nacre", 0.0, 1.0, 0.0),
    num("bands", 0.0, 16.0, 0.0),
    num("spot_scale", 1.0, 40.0, 1.0),
];
