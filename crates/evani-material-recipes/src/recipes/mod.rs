//! The recipes the recipe stage draws, one module per recipe (batch 1 build
//! plan section 3; P4 builds metal, wood, stone and ceramic, P5 bark, masonry,
//! plaster and ground, P6 leather, fur, hide and keratin, P7 feather and scales, P8 gem and ice, P9 paint extras, plastic, organic, fire and glow; batch 2's B2-P1 foliage). Each module holds
//! its schema row: the names, ranges and defaults of what its shader reads
//! from the block, so tools (the viewer's sliders, P14) and tests know the
//! slots without parsing JSON. A test holds every row equal to
//! `catalog/materials/recipes.json`. The data is plain constants, so a game
//! that never reads it downloads none of it.
//!
//! # How to add a recipe (P4 to P9 followed this)
//!
//! 1. **Shader**: `shaders/rc_<name>.wgsl`, first line `// evani.shader.rc.<name>`,
//!    then a comment block listing its numbers (`b.na` = n0-n3, `b.nb` = n4-n7,
//!    `b.nc` = n8-n9, each unorm: map with `rc_range(x, min, max)` using the
//!    ranges in `recipes.json`), colours, variants (`b.variant` is the index),
//!    extras (`rc_extra(b, RC_X_<NAME>, k)`, `k` the extra's bit index) and what
//!    each level draws. It defines
//!    `fn rc_<name>(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut`;
//!    helpers are named `rc_<name>_<what>` so recipes never clash.
//! 2. **Levels** (`q`, its quality group's level, plan 2.5): `q == 0u` (lite)
//!    returns the light version early, through `rc_out`; low (1) draws the
//!    pattern with every detail faded to its average by `rc_aa` /
//!    `rc_aa_world` / `rc_fade` once a cell is under 1.5 px, and no normal
//!    edits; medium (2) adds normals within 25 m (`rc_near`, a finite
//!    difference of the recipe's height function, `rc_tuv` for uv patterns or
//!    `rc_tw` for world ones, then `rc_bump`); high (3) adds glints, sparkle and
//!    2-step parallax near the camera; ultra (4) parallax everywhere. A level
//!    with nothing new draws as the one below.
//! 3. **Colour** comes from `b.col_*` times `i.tex` and `i.vcol` (`rc_out` does
//!    this), never from `i.base`. Pattern space is uv times a scale, or world
//!    space for ground, stone and masonry (`rc_proj` up to medium, `rc_proj2`
//!    blended from high). No `dpdx`, `dpdy` or `fwidth`: every derivative is
//!    in `MatFrame`. No clock: animation reads `rc_time()`. Post-light terms go
//!    in `o.emit`, `o.sheen` (rgb, w = rim power) and `o.glint`.
//! 4. **Feature**: `recipe-<name> = []` in `Cargo.toml`, one
//!    `#[cfg(feature = "recipe-<name>")]` row in `shader::COMPILED`, and the same
//!    row (uncfg'd) in `shader::ALL_RECIPES` (both through `stripped!`, so
//!    comments never ship). The dispatch, the `RC_X_<NAME>` constant and the
//!    `material.recipe.<name>.<extra>` live switches follow by themselves.
//! 5. **Schema**: `src/recipes/<name>.rs` with its `SCHEMA` (copy one of these),
//!    added to [`BUILT`]; `tests/recipes.rs` then checks it against
//!    `recipes.json` and the extras table.
//! 6. **Tests**: `crates/evani-render/tests/recipe_wgsl.rs` validates every
//!    `ALL_RECIPES` entry alone, all together and with all eight condition
//!    layers (it reads the array, so nothing to add). Run it with
//!    `--features material-layers,char-materials,material-recipes`.
//! 7. **Proof and numbers**: add the recipe's catalogue rows to the sandbox
//!    proof (`apps/sandbox/src/proof_recipes.rs`, feature list in
//!    `recipes-proof`), give the rows you show real `params` in
//!    `catalog/materials/materials.json`, capture High and Low, and measure the
//!    gzip size (standalone stripped WGSL, plus the sandbox build pair).
//! 8. **Layouts** (batch 3, B3-P1): units laid out in a pattern (bonds,
//!    herringbone, basket, chequer, hex, shingles, studs, rivets, stripes,
//!    motifs) come from `shaders/rc_layout.wgsl` (`rc_lay_*`, mirrored in
//!    [`crate::layout`]): call it and add `"layout"` to the recipe's feature
//!    (`recipe-<name> = ["layout"]`). A new extra defaults off in
//!    `recipes.json` and a new number to its stored 0, so existing blocks
//!    stay byte-identical until a row opts in.

pub mod bark;
pub mod ceramic;
pub mod cloth;
pub mod feather;
pub mod fire;
pub mod foliage;
pub mod fur;
pub mod gem;
pub mod glass;
pub mod glow;
pub mod ground;
pub mod hide;
pub mod ice;
pub mod keratin;
pub mod leather;
pub mod masonry;
pub mod metal;
pub mod organic;
pub mod paint;
pub mod plaster;
pub mod plastic;
pub mod scales;
pub mod stone;
pub mod thin;
pub mod wood;

/// One number of a recipe: its name and the range its u8 maps onto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Number {
    /// The number's name in `recipes.json`.
    pub name: &'static str,
    /// The value a stored 0 means.
    pub min: f32,
    /// The value a stored 255 means.
    pub max: f32,
    /// What a row that doesn't set it gets.
    pub default: f32,
}

/// A [`Number`], for the schema tables.
pub const fn num(name: &'static str, min: f32, max: f32, default: f32) -> Number {
    Number { name, min, max, default }
}

/// What a recipe's shader reads from the block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Schema {
    /// The recipe code (block byte 0).
    pub code: u8,
    /// The recipe id.
    pub name: &'static str,
    /// Colour slots A, B and C, as many as the recipe uses (1 to 3).
    pub colours: &'static [&'static str],
    /// The variants, by index (block byte 1).
    pub variants: &'static [&'static str],
    /// The numbers n0.., in block order.
    pub numbers: &'static [Number],
    /// The extras, by bit (block byte 2).
    pub extras: &'static [&'static str],
    /// What the light version draws.
    pub light: &'static str,
}

impl Schema {
    /// The index of number `name` (its slot n`i`), if the recipe has it.
    pub fn number(&self, name: &str) -> Option<usize> {
        self.numbers.iter().position(|n| n.name == name)
    }
}

/// Every built recipe's schema, in code order (the same recipes as
/// `shader::ALL_RECIPES`).
pub const BUILT: [&Schema; 25] = [
    &metal::SCHEMA,
    &paint::SCHEMA,
    &plastic::SCHEMA,
    &wood::SCHEMA,
    &bark::SCHEMA,
    &stone::SCHEMA,
    &masonry::SCHEMA,
    &plaster::SCHEMA,
    &ground::SCHEMA,
    &cloth::SCHEMA,
    &leather::SCHEMA,
    &fur::SCHEMA,
    &feather::SCHEMA,
    &scales::SCHEMA,
    &hide::SCHEMA,
    &keratin::SCHEMA,
    &foliage::SCHEMA,
    &thin::SCHEMA,
    &organic::SCHEMA,
    &ceramic::SCHEMA,
    &glass::SCHEMA,
    &gem::SCHEMA,
    &ice::SCHEMA,
    &fire::SCHEMA,
    &glow::SCHEMA,
];

/// The schema of recipe `code`, if it is built.
pub fn schema(code: u8) -> Option<&'static Schema> {
    BUILT.iter().copied().find(|s| s.code == code)
}
