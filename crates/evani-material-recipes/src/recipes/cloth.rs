//! Cloth (recipe 10): weaves (plain, 2/1 twill, open, herringbone), knit,
//! felt, carpet pile, satin and a rug's flatweave; coloured yarns (the sett:
//! gingham, plaid, tartan, pinstripe, ticking, chambray, houndstooth,
//! windowpane), slubs and woven motifs; and since B4-P3b its own sheen and
//! fuzz after lighting, so a game that lists cloth needs no layers family.
//! Shader: `shaders/rc_cloth.wgsl` (feature `recipe-cloth`).
//!
//! The schema row: what the shader reads from the block. A test holds it
//! equal to `catalog/materials/recipes.json`.

use super::{Number, Schema, num};

/// The cloth recipe.
pub const SCHEMA: Schema = Schema {
    code: 10,
    name: "cloth",
    colours: &["colour", "sheen", "motif"],
    variants: &["plain", "twill", "knit", "pile", "satin", "flatweave", "felt", "open", "herringbone"],
    numbers: &NUMBERS,
    extras: &["sheen", "weave_relief", "fuzz_rim", "motif", "sett", "slubs"],
    light: "the weave, sett and slubs in colour, with the sheen and fuzz",
};

/// The sett kinds, by `sett_kind` (B4-P3a): which stripes the warp and weft
/// yarns carry. Chambray, pinstripe and ticking colour the warp only.
pub const SETT_KINDS: [&str; 8] = ["chambray", "gingham", "pinstripe", "ticking", "plaid", "tartan", "houndstooth", "windowpane"];

/// The numbers n0 to n9, in block order (the shader reads n0-n3 as `na`,
/// n4-n7 as `nb`, n8-n9 as `nc`). The last three (B4-P3a) default to their
/// lowest value, so every earlier cloth block keeps its 32 bytes.
const NUMBERS: [Number; 10] = [
    num("weave_scale", 1.0, 400.0, 120.0),
    num("sheen", 0.0, 1.0, 0.3),
    num("sheen_rough", 0.0, 1.0, 0.5),
    num("fuzz", 0.0, 1.0, 0.2),
    num("rough", 0.0, 1.0, 0.8),
    num("motif_scale", 1.0, 32.0, 1.0),
    num("motif_contrast", 0.0, 1.0, 0.0),
    num("sett_scale", 1.0, 64.0, 1.0),
    num("sett_kind", 0.0, 7.0, 0.0),
    num("slub", 0.0, 1.0, 0.0),
];
