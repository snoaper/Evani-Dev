//! Gem (recipe 25): cut stones, raw crystals and amber, faked without any
//! refraction: faceted normals from a cellular split of the normal, a rainbow
//! by view angle, a Fresnel rim, a sparkle per facet and an emissive core.
//! Shader: `shaders/rc_gem.wgsl` (feature `recipe-gem`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The gem recipe.
pub const SCHEMA: Schema = Schema {
    code: 25,
    name: "gem",
    colours: &["colour", "inclusion", "core_glow"],
    variants: &["brilliant", "step", "cabochon", "raw crystal", "amber"],
    numbers: &NUMBERS,
    extras: &["sparkle", "inner_glow", "clear"],
    light: "tinted gloss with one sparkle",
};

/// The numbers n0 to n5, in block order (the shader reads n0-n3 as `na`,
/// n4-n5 as `nb`).
const NUMBERS: [Number; 6] = [
    num("dispersion", 0.0, 1.0, 0.3),
    num("sparkle", 0.0, 1.0, 0.6),
    num("facet_scale", 1.0, 30.0, 6.0),
    num("inclusions", 0.0, 1.0, 0.1),
    num("clarity", 0.0, 1.0, 0.8),
    num("core_glow", 0.0, 1.0, 0.2),
];
