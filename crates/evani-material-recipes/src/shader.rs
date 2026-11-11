//! The recipe stage as WGSL text, and the patch that wires it into the model
//! lit stage (batch 1 build plan 2.3, package P2a).
//!
//! [`patch_lit`] and [`patch_layered`] take the forward module the model
//! pipelines compile (the engine's `FORWARD.source`, or that text already
//! through the character patch) and rewrite five lines of `model_lit_fs` or
//! `model_layered_fs` by exact text, so the stage calls `mat_frame` (every
//! screen derivative, before the discard), `mat_surface` (the recipe and the
//! condition layers, after `occ` is set and before any lighting) and
//! `mat_extra` (the post-light term, before fog). Then they append
//! [`stage_wgsl`]. If a line is missing the patch returns `None` and the
//! engine compiles the unpatched stage: plain PBR, never a broken pipeline.
//! No WGSL file that games use is edited.
//!
//! What gets appended is assembled from the recipes and condition layers that
//! are compiled in ([`COMPILED`], [`LAYERS`]): each recipe package adds one
//! `#[cfg(feature = "recipe-<name>")]` row and its `shaders/rc_<name>.wgsl`,
//! each layer package one `#[cfg(feature = "cond-<layer>")]` row, so a recipe
//! nobody lists is never in the download.

/// The core WGSL, with comments: block unpack, hash and noise, Voronoi, hex
/// cells, the AA fade, pattern space, `MatFrame`, `MatIn`, `MatOut` and the
/// three calls the patch wires in.
pub const CORE_WGSL: &str = include_str!("../shaders/core.wgsl");

/// [`CORE_WGSL`] without its comments, stripped at compile time (as the
/// engine strips its own WGSL), so the comments never reach a download.
pub const CORE_STRIPPED: &str = utf8(&CORE_BYTES);
const CORE_BYTES: [u8; wgsl_strip(CORE_WGSL.as_bytes(), &mut [])] = wgsl_stripped(CORE_WGSL.as_bytes());

const fn starts_at(s: &[u8], i: usize, p: &[u8]) -> bool {
    if i + p.len() > s.len() {
        return false;
    }
    let mut k = 0;
    while k < p.len() {
        if s[i + k] != p[k] {
            return false;
        }
        k += 1;
    }
    true
}

/// A `//` comment that isn't a `//@` or `// evani.` marker starts at `i`.
const fn comment_at(s: &[u8], i: usize) -> bool {
    starts_at(s, i, b"//") && !starts_at(s, i, b"//@") && !starts_at(s, i, b"// evani.")
}

/// A space, tab or carriage return.
const fn blank(b: u8) -> bool {
    b == b' ' || b == 9 || b == 13
}

/// A letter, digit, `_` or `.` (part of a word or number).
const fn wordy(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// A character that can be half of a two-character operator.
const fn opish(b: u8) -> bool {
    matches!(b, b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^' | b'%')
}

/// [`strip`] at compile time, and tighter: `s` without `//` comments, line
/// indentation, trailing spaces, carriage returns, or blank and comment-only
/// lines (WGSL has no strings, so `//` always starts a comment; `//@` and
/// `// evani.` marker lines stay as they are). Inside a code line a run of
/// spaces is dropped unless it separates two words or two operator
/// characters (`a b`, `= -x`, `> =`), so tokens never merge. Writes what fits
/// into `out` and returns the stripped length. Recipe packages strip their
/// `rc_<name>.wgsl` with it too ([`wgsl_stripped`]).
pub const fn wgsl_strip(s: &[u8], out: &mut [u8]) -> usize {
    let (mut i, mut n) = (0, 0);
    while i < s.len() {
        while i < s.len() && blank(s[i]) {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        if s[i] == 10 || comment_at(s, i) {
            while i < s.len() && s[i] != 10 {
                i += 1;
            }
            i += 1;
            continue;
        }
        let marker = starts_at(s, i, b"//");
        let mut keep = n;
        let mut prev = 0u8;
        while i < s.len() && s[i] != 10 && !comment_at(s, i) {
            if blank(s[i]) && !marker {
                let mut j = i;
                while j < s.len() && blank(s[j]) {
                    j += 1;
                }
                let next = if j < s.len() { s[j] } else { 10 };
                if !((wordy(prev) && wordy(next)) || (opish(prev) && opish(next))) {
                    i = j;
                    continue;
                }
            }
            if n < out.len() {
                out[n] = s[i];
            }
            n += 1;
            if !blank(s[i]) {
                keep = n;
                prev = s[i];
            }
            i += 1;
        }
        n = keep;
        while i < s.len() && s[i] != 10 {
            i += 1;
        }
        if n < out.len() {
            out[n] = 10;
        }
        n += 1;
        i += 1;
    }
    n
}

/// The bytes of [`wgsl_strip`] as an array of its exact length.
pub const fn wgsl_stripped<const N: usize>(s: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let n = wgsl_strip(s, &mut out);
    assert!(n == N, "wgsl_strip: length changed");
    out
}

/// Stripped bytes back to text, at compile time.
pub const fn utf8(b: &'static [u8]) -> &'static str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("wgsl_strip: not UTF-8"),
    }
}

/// The binding of the material's 32-byte block (`@group(1)`, a uniform).
pub const BLOCK_BINDING: u32 = 12;
/// The binding of the shared per-frame recipe uniform (`@group(1)`).
pub const FRAME_BINDING: u32 = 13;
/// The size of the per-frame recipe uniform in bytes (layout in core.wgsl,
/// `RecFrame`): 64 bytes of amounts, levels, time, off and deny bits, then
/// 64 bytes of the condition layers' look numbers (P3). The WGSL struct, the
/// words `params::frame_words` writes and the GPU buffer all follow this; a
/// test holds the struct's size equal to it.
pub const FRAME_BYTES: usize = 128;

/// The quality groups, in the order of their 4-bit level in the per-frame
/// uniform (`material.quality.<group>`).
pub const GROUPS: [&str; 8] = ["detail", "fur", "water", "glass", "skin", "foliage", "conditions", "reflections"];

/// A recipe the recipe stage draws (backend `recipes`, plus `paint`, whose
/// flake and twill extras are drawn here): its code, name, quality group
/// (index into [`GROUPS`]) and how many extras bits it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageRecipe {
    /// The recipe code (block byte 0).
    pub code: u8,
    /// The recipe id in `recipes.json`.
    pub name: &'static str,
    /// Its quality group, an index into [`GROUPS`].
    pub group: u8,
    /// How many extras bits it has (block byte 2).
    pub extras: u8,
}

const fn sr(code: u8, name: &'static str, group: u8, extras: u8) -> StageRecipe {
    StageRecipe { code, name, group, extras }
}

/// Every recipe-stage recipe, in code order, whatever is compiled in. Their
/// extras take consecutive global bits in this order ([`extra_base`]), so the
/// per-frame deny bits mean the same in every build. A test holds this table
/// equal to `catalog/materials/recipes.json`.
pub const STAGE: [StageRecipe; 25] = [
    sr(1, "metal", 0, 7),
    sr(2, "paint", 0, 5),
    sr(3, "plastic", 0, 3),
    sr(4, "wood", 0, 5),
    sr(5, "bark", 0, 3),
    sr(6, "stone", 0, 4),
    sr(7, "masonry", 0, 5),
    sr(8, "plaster", 0, 5),
    sr(9, "ground", 0, 5),
    sr(10, "cloth", 0, 6),
    sr(11, "leather", 0, 3),
    sr(12, "fur", 1, 2),
    sr(13, "feather", 0, 2),
    sr(14, "scales", 0, 2),
    sr(15, "hide", 4, 3),
    sr(19, "keratin", 0, 3),
    sr(20, "foliage", 5, 3),
    sr(21, "thin", 0, 5),
    sr(22, "organic", 0, 2),
    sr(23, "ceramic", 0, 4),
    sr(24, "glass", 3, 1),
    sr(25, "gem", 3, 3),
    sr(27, "ice", 3, 4),
    sr(28, "fire", 0, 1),
    sr(29, "glow", 0, 2),
];

/// The stage row of recipe `code`, if the recipe stage draws it.
pub fn stage_recipe(code: u8) -> Option<&'static StageRecipe> {
    STAGE.iter().find(|r| r.code == code)
}

/// The first global extras bit of recipe `code` (its extras bit `k` is global
/// bit `extra_base + k`), or `None` for a recipe the stage doesn't draw.
pub fn extra_base(code: u8) -> Option<u32> {
    let mut base = 0u32;
    for r in &STAGE {
        if r.code == code {
            return Some(base);
        }
        base += u32::from(r.extras);
    }
    None
}

/// The extras of each [`STAGE`] row, by name, in bit order (`recipes.json`;
/// a test holds the two equal). `material.recipe.<recipe>.<extra>` names.
pub const EXTRA_NAMES: [&[&str]; 25] = [
    &["grain", "engraving", "relief", "studs", "seams", "spots", "temper"],
    &["clearcoat", "flake", "film", "twill", "studs"],
    &["translucency", "specks", "tread"],
    &["grain_relief", "plank_gaps", "studs", "painted", "adze"],
    &["parallax", "marks", "embers"],
    &["parallax", "glints", "translucency", "tiles"],
    &["parallax", "unit_variation", "painted", "glaze", "flecks"],
    &["cracks", "trowel_relief", "chunks", "aggregate", "boards"],
    &["parallax_pebbles", "sparkle", "cracks", "furrows", "cover"],
    &["sheen", "weave_relief", "fuzz_rim", "motif", "sett", "slubs"],
    &["crease_relief", "nap", "crazed"],
    &["shells", "pattern"],
    &["iridescence", "edge_relief"],
    &["relief", "shimmer"],
    &["wrinkle_relief", "scatter", "bristles"],
    &["edge_translucency", "nacre", "spots"],
    &["back_light", "veins", "bare"],
    &["stripes", "motif", "peel", "damask", "film"],
    &["scatter", "clear"],
    &["crackle", "glaze_depth", "motif", "lustre"],
    &["backlight"],
    &["sparkle", "inner_glow", "clear"],
    &["inner_cracks", "sparkle", "scatter", "ridges"],
    &["flicker"],
    &["pulse", "hologram"],
];

/// The `material.recipe.<recipe>.<extra>` name of global extras bit `bit`.
pub fn extra_name(bit: u32) -> Option<String> {
    let mut base = 0u32;
    for (row, names) in STAGE.iter().zip(EXTRA_NAMES) {
        let n = u32::from(row.extras);
        if bit < base + n {
            let k = (bit - base) as usize;
            return names.get(k).map(|e| format!("material.recipe.{}.{e}", row.name));
        }
        base += n;
    }
    None
}

/// One compiled recipe: its code and its WGSL, which defines
/// `fn rc_<name>(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut`.
#[derive(Debug, Clone, Copy)]
pub struct RecipeSrc {
    /// The recipe code; must have a [`STAGE`] row.
    pub code: u8,
    /// The recipe's WGSL.
    pub wgsl: &'static str,
}

/// One compiled condition layer: its bit (0 wet .. 7 blood, the block's
/// condition mask) and its WGSL, which defines
/// `fn cond_<layer>(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut`
/// (`a` its amount; `b` the block, for the colours wear and oxide show).
#[derive(Debug, Clone, Copy)]
pub struct LayerSrc {
    /// The layer's bit in the condition mask.
    pub bit: u8,
    /// The layer's WGSL.
    pub wgsl: &'static str,
}

/// A WGSL file of this crate, stripped at compile time ([`wgsl_strip`]), so
/// its comments never reach a download.
macro_rules! stripped {
    ($path:literal) => {{
        const SRC: &[u8] = include_str!($path).as_bytes();
        const BYTES: [u8; wgsl_strip(SRC, &mut [])] = wgsl_stripped(SRC);
        utf8(&BYTES)
    }};
    // A recipe and one of its parts, as one text.
    ($path:literal, $part:literal) => {{
        const SRC: &[u8] = concat!(include_str!($path), "
", include_str!($part)).as_bytes();
        const BYTES: [u8; wgsl_strip(SRC, &mut [])] = wgsl_stripped(SRC);
        utf8(&BYTES)
    }};
}

/// The helpers every recipe shares (`rc_out`, `rc_near`, `rc_tuv`, `rc_tw`,
/// `rc_bump`), stripped. Appended once after the core when any recipe is
/// compiled, so a game with no recipe never downloads them.
pub const RC_SHARED: &str = stripped!("../shaders/rc_shared.wgsl");

/// The pattern layouts recipes share (batch 3 build plan 3.1, B3-P1:
/// `shaders/rc_layout.wgsl`, the `rc_lay_` functions), stripped, when the
/// `layout` feature is on (the recipes that draw a layout turn it on); empty
/// otherwise, so a build with no patterned recipe carries none of it.
#[cfg(feature = "layout")]
pub const RC_LAYOUT: &str = stripped!("../shaders/rc_layout.wgsl");
/// The pattern layouts: none in this build (no recipe that draws one).
#[cfg(not(feature = "layout"))]
pub const RC_LAYOUT: &str = "";

/// [`RC_LAYOUT`] whatever the features, for [`stage_wgsl_with`] (tests and
/// tools, which assemble any set of recipes). Never reached from
/// [`stage_wgsl`], [`patch_lit`] or [`patch_layered`], so a game never
/// downloads it through here.
pub const ALL_LAYOUT: &str = stripped!("../shaders/rc_layout.wgsl");

/// A recipe's WGSL calls the pattern layouts.
pub fn uses_layout(r: &RecipeSrc) -> bool {
    r.wgsl.contains("rc_lay_")
}

/// The recipes compiled into this build, one
/// `#[cfg(feature = "recipe-<name>")]` row each, in code order (see
/// [`crate::recipes`] for how a recipe package adds one).
pub const COMPILED: &[RecipeSrc] = &[
    #[cfg(feature = "recipe-metal")]
    RecipeSrc { code: 1, wgsl: stripped!("../shaders/rc_metal.wgsl") },
    #[cfg(feature = "recipe-paint")]
    RecipeSrc { code: 2, wgsl: stripped!("../shaders/rc_paint.wgsl") },
    #[cfg(feature = "recipe-plastic")]
    RecipeSrc { code: 3, wgsl: stripped!("../shaders/rc_plastic.wgsl") },
    // Wood's batch 3 layouts are their own part (`recipe-wood-layouts`);
    // without it a stand-in draws them as planks, grain and planks unchanged.
    #[cfg(all(feature = "recipe-wood", feature = "recipe-wood-layouts"))]
    RecipeSrc { code: 4, wgsl: stripped!("../shaders/rc_wood.wgsl", "../shaders/rc_wood_layouts.wgsl") },
    #[cfg(all(feature = "recipe-wood", not(feature = "recipe-wood-layouts")))]
    RecipeSrc { code: 4, wgsl: stripped!("../shaders/rc_wood.wgsl", "../shaders/rc_wood_planks.wgsl") },
    #[cfg(feature = "recipe-bark")]
    RecipeSrc { code: 5, wgsl: stripped!("../shaders/rc_bark.wgsl") },
    #[cfg(feature = "recipe-stone")]
    RecipeSrc { code: 6, wgsl: stripped!("../shaders/rc_stone.wgsl") },
    #[cfg(feature = "recipe-masonry")]
    RecipeSrc { code: 7, wgsl: stripped!("../shaders/rc_masonry.wgsl") },
    #[cfg(feature = "recipe-plaster")]
    RecipeSrc { code: 8, wgsl: stripped!("../shaders/rc_plaster.wgsl", "../shaders/rc_plaster_concrete.wgsl") },
    #[cfg(feature = "recipe-ground")]
    RecipeSrc { code: 9, wgsl: stripped!("../shaders/rc_ground.wgsl") },
    #[cfg(feature = "recipe-cloth")]
    RecipeSrc { code: 10, wgsl: stripped!("../shaders/rc_cloth.wgsl") },
    #[cfg(feature = "recipe-leather")]
    RecipeSrc { code: 11, wgsl: stripped!("../shaders/rc_leather.wgsl") },
    #[cfg(feature = "recipe-fur")]
    RecipeSrc { code: 12, wgsl: stripped!("../shaders/rc_fur.wgsl") },
    #[cfg(feature = "recipe-feather")]
    RecipeSrc { code: 13, wgsl: stripped!("../shaders/rc_feather.wgsl") },
    #[cfg(feature = "recipe-scales")]
    RecipeSrc { code: 14, wgsl: stripped!("../shaders/rc_scales.wgsl") },
    #[cfg(feature = "recipe-hide")]
    RecipeSrc { code: 15, wgsl: stripped!("../shaders/rc_hide.wgsl") },
    #[cfg(feature = "recipe-keratin")]
    RecipeSrc { code: 19, wgsl: stripped!("../shaders/rc_keratin.wgsl") },
    #[cfg(feature = "recipe-foliage")]
    RecipeSrc { code: 20, wgsl: stripped!("../shaders/rc_foliage.wgsl") },
    #[cfg(feature = "recipe-thin")]
    RecipeSrc { code: 21, wgsl: stripped!("../shaders/rc_thin.wgsl") },
    // Organic's food detail (flesh grain, kernels, pastry) is its own part
    // (`recipe-organic-food`); without it a stand-in draws them plain.
    #[cfg(all(feature = "recipe-organic", feature = "recipe-organic-food"))]
    RecipeSrc { code: 22, wgsl: stripped!("../shaders/rc_organic.wgsl", "../shaders/rc_organic_food.wgsl") },
    #[cfg(all(feature = "recipe-organic", not(feature = "recipe-organic-food")))]
    RecipeSrc { code: 22, wgsl: stripped!("../shaders/rc_organic.wgsl", "../shaders/rc_organic_plain.wgsl") },
    #[cfg(feature = "recipe-ceramic")]
    RecipeSrc { code: 23, wgsl: stripped!("../shaders/rc_ceramic.wgsl") },
    #[cfg(feature = "recipe-glass")]
    RecipeSrc { code: 24, wgsl: stripped!("../shaders/rc_glass.wgsl") },
    #[cfg(feature = "recipe-gem")]
    RecipeSrc { code: 25, wgsl: stripped!("../shaders/rc_gem.wgsl") },
    #[cfg(feature = "recipe-ice")]
    RecipeSrc { code: 27, wgsl: stripped!("../shaders/rc_ice.wgsl") },
    #[cfg(feature = "recipe-fire")]
    RecipeSrc { code: 28, wgsl: stripped!("../shaders/rc_fire.wgsl") },
    #[cfg(feature = "recipe-glow")]
    RecipeSrc { code: 29, wgsl: stripped!("../shaders/rc_glow.wgsl") },
];

/// Every built recipe's WGSL, stripped, whatever is compiled in (tests and
/// tools: the feature-mix validation and the size report). Grows with each
/// recipe package.
pub const ALL_RECIPES: [RecipeSrc; 25] = [
    RecipeSrc { code: 1, wgsl: stripped!("../shaders/rc_metal.wgsl") },
    RecipeSrc { code: 2, wgsl: stripped!("../shaders/rc_paint.wgsl") },
    RecipeSrc { code: 3, wgsl: stripped!("../shaders/rc_plastic.wgsl") },
    RecipeSrc { code: 4, wgsl: stripped!("../shaders/rc_wood.wgsl", "../shaders/rc_wood_layouts.wgsl") },
    RecipeSrc { code: 5, wgsl: stripped!("../shaders/rc_bark.wgsl") },
    RecipeSrc { code: 6, wgsl: stripped!("../shaders/rc_stone.wgsl") },
    RecipeSrc { code: 7, wgsl: stripped!("../shaders/rc_masonry.wgsl") },
    RecipeSrc { code: 8, wgsl: stripped!("../shaders/rc_plaster.wgsl", "../shaders/rc_plaster_concrete.wgsl") },
    RecipeSrc { code: 9, wgsl: stripped!("../shaders/rc_ground.wgsl") },
    RecipeSrc { code: 10, wgsl: stripped!("../shaders/rc_cloth.wgsl") },
    RecipeSrc { code: 11, wgsl: stripped!("../shaders/rc_leather.wgsl") },
    RecipeSrc { code: 12, wgsl: stripped!("../shaders/rc_fur.wgsl") },
    RecipeSrc { code: 13, wgsl: stripped!("../shaders/rc_feather.wgsl") },
    RecipeSrc { code: 14, wgsl: stripped!("../shaders/rc_scales.wgsl") },
    RecipeSrc { code: 15, wgsl: stripped!("../shaders/rc_hide.wgsl") },
    RecipeSrc { code: 19, wgsl: stripped!("../shaders/rc_keratin.wgsl") },
    RecipeSrc { code: 20, wgsl: stripped!("../shaders/rc_foliage.wgsl") },
    RecipeSrc { code: 21, wgsl: stripped!("../shaders/rc_thin.wgsl") },
    RecipeSrc { code: 22, wgsl: stripped!("../shaders/rc_organic.wgsl", "../shaders/rc_organic_food.wgsl") },
    RecipeSrc { code: 23, wgsl: stripped!("../shaders/rc_ceramic.wgsl") },
    RecipeSrc { code: 24, wgsl: stripped!("../shaders/rc_glass.wgsl") },
    RecipeSrc { code: 25, wgsl: stripped!("../shaders/rc_gem.wgsl") },
    RecipeSrc { code: 27, wgsl: stripped!("../shaders/rc_ice.wgsl") },
    RecipeSrc { code: 28, wgsl: stripped!("../shaders/rc_fire.wgsl") },
    RecipeSrc { code: 29, wgsl: stripped!("../shaders/rc_glow.wgsl") },
];

/// Wood without its layouts part (grain and planks, the layouts drawn as
/// planks, cork flat), stripped, whatever is compiled in (tests and tools:
/// the size report and the check that it builds and draws grain and planks
/// as [`ALL_RECIPES`]' wood does).
pub const WOOD_PLANKS: RecipeSrc = RecipeSrc { code: 4, wgsl: stripped!("../shaders/rc_wood.wgsl", "../shaders/rc_wood_planks.wgsl") };

/// Organic without its food part (flesh without its grain, kernels and
/// pastry plain), stripped, whatever is compiled in (tests and tools: the
/// size report and the byte budget, which is the recipe's own size).
pub const ORGANIC_PLAIN: RecipeSrc = RecipeSrc { code: 22, wgsl: stripped!("../shaders/rc_organic.wgsl", "../shaders/rc_organic_plain.wgsl") };

/// The condition layers compiled into this build (P3), one
/// `#[cfg(feature = "cond-<layer>")]` row each, in bit order.
pub const LAYERS: &[LayerSrc] = &[
    #[cfg(feature = "cond-wet")]
    LayerSrc { bit: 0, wgsl: stripped!("../shaders/cond_wet.wgsl") },
    #[cfg(feature = "cond-mud")]
    LayerSrc { bit: 1, wgsl: stripped!("../shaders/cond_mud.wgsl") },
    #[cfg(feature = "cond-dust")]
    LayerSrc { bit: 2, wgsl: stripped!("../shaders/cond_dust.wgsl") },
    #[cfg(feature = "cond-snow")]
    LayerSrc { bit: 3, wgsl: stripped!("../shaders/cond_snow.wgsl") },
    #[cfg(feature = "cond-wear")]
    LayerSrc { bit: 4, wgsl: stripped!("../shaders/cond_wear.wgsl") },
    #[cfg(feature = "cond-oxide")]
    LayerSrc { bit: 5, wgsl: stripped!("../shaders/cond_oxide.wgsl") },
    #[cfg(feature = "cond-growth")]
    LayerSrc { bit: 6, wgsl: stripped!("../shaders/cond_growth.wgsl") },
    #[cfg(feature = "cond-blood")]
    LayerSrc { bit: 7, wgsl: stripped!("../shaders/cond_blood.wgsl") },
];

/// Every layer's WGSL, stripped, whatever is compiled in (tests and tools:
/// the feature-mix validation and the size report).
pub const ALL_LAYERS: [LayerSrc; 8] = [
    LayerSrc { bit: 0, wgsl: stripped!("../shaders/cond_wet.wgsl") },
    LayerSrc { bit: 1, wgsl: stripped!("../shaders/cond_mud.wgsl") },
    LayerSrc { bit: 2, wgsl: stripped!("../shaders/cond_dust.wgsl") },
    LayerSrc { bit: 3, wgsl: stripped!("../shaders/cond_snow.wgsl") },
    LayerSrc { bit: 4, wgsl: stripped!("../shaders/cond_wear.wgsl") },
    LayerSrc { bit: 5, wgsl: stripped!("../shaders/cond_oxide.wgsl") },
    LayerSrc { bit: 6, wgsl: stripped!("../shaders/cond_growth.wgsl") },
    LayerSrc { bit: 7, wgsl: stripped!("../shaders/cond_blood.wgsl") },
];

/// The condition mask bits of the layers compiled into this build.
pub fn compiled_layers() -> u8 {
    LAYERS.iter().fold(0, |m, l| m | (1u8 << (l.bit & 7)))
}

/// The recipe stage WGSL for this build: the core, the shared recipe helpers
/// (when any recipe is compiled), every compiled recipe and layer, the
/// dispatch and the condition pass, comments stripped.
pub fn stage_wgsl() -> String {
    stage_text(COMPILED, LAYERS, RC_LAYOUT, CUT_BUILT, if BLEND_BUILT { 1 } else { 0 })
}

/// [`stage_wgsl`] for a given set of recipes and layers (tests, tools).
/// A recipe without a [`STAGE`] row is left out of the dispatch. The
/// pattern layouts follow the set: in when one of its recipes calls them.
pub fn stage_wgsl_with(recipes: &[RecipeSrc], layers: &[LayerSrc]) -> String {
    stage_text(recipes, layers, ALL_LAYOUT, false, 0)
}

/// [`stage_wgsl_with`] plus the cut-out ([`CUT_WGSL`], [`CUT_STAGE`]),
/// whatever the features (tests, tools).
pub fn stage_wgsl_cut_with(recipes: &[RecipeSrc], layers: &[LayerSrc]) -> String {
    stage_text(recipes, layers, ALL_LAYOUT, !recipes.is_empty(), 0)
}

/// [`stage_wgsl_with`] plus the see-through pass ([`BLEND_WGSL`]; `refract`
/// false: the stub, the blend pipeline's text; true: [`REFRACT_WGSL`], the
/// glass pipeline's), whatever the features (tests, tools).
pub fn stage_wgsl_blend_with(recipes: &[RecipeSrc], layers: &[LayerSrc], refract: bool) -> String {
    stage_text(recipes, layers, ALL_LAYOUT, false, if recipes.is_empty() { 0 } else if refract { 2 } else { 1 })
}

/// The stage text, with `layout` (the stripped layouts, or empty) appended
/// after the shared helpers when a recipe of the set calls it, with `cut`
/// the cut-out's two texts after the recipes (they call the shared helpers),
/// and with `blend` 1 or 2 the see-through pass's text and its refraction
/// (1 the stub, 2 the copy's sampler; B3-P3).
pub(crate) fn stage_text(recipes: &[RecipeSrc], layers: &[LayerSrc], layout: &str, cut: bool, blend: u8) -> String {
    stage_text_style(recipes, layers, layout, cut, blend, true)
}

/// [`stage_text`] with the style detail step only when `style` (and the
/// `style` feature) is on: `cargo xtask check` sizes each game's stage as
/// that game builds it.
pub(crate) fn stage_text_style(recipes: &[RecipeSrc], layers: &[LayerSrc], layout: &str, cut: bool, blend: u8, style: bool) -> String {
    #[cfg(feature = "style")]
    let (mut s, style) = match crate::style::hooked(CORE_STRIPPED).filter(|_| style) {
        Some(h) => (h, true),
        None => (CORE_STRIPPED.to_string(), false),
    };
    #[cfg(not(feature = "style"))]
    let _ = style;
    #[cfg(not(feature = "style"))]
    let mut s = CORE_STRIPPED.to_string();
    if !recipes.is_empty() {
        s.push('\n');
        s.push_str(RC_SHARED);
    }
    if !layout.is_empty() && recipes.iter().any(uses_layout) {
        s.push('\n');
        s.push_str(layout);
    }
    for r in recipes {
        s.push('\n');
        s.push_str(&strip(r.wgsl));
    }
    for l in layers {
        s.push('\n');
        s.push_str(&strip(l.wgsl));
    }
    if cut {
        // The oak leaf spray (cut-out kind 7) only where foliage is drawn.
        // Every other material's text is unchanged.
        let leaf = recipes.iter().any(|r| r.code == FOLIAGE_CODE);
        s.push('\n');
        if leaf {
            s.push_str(&leaf_cut(CUT_WGSL));
            s.push('\n');
            s.push_str(CUT_LEAF);
            s.push('\n');
            s.push_str(&leaf_cut_stage(CUT_STAGE));
            s.push('\n');
            s.push_str(CUT_LEAF_STAGE);
        } else {
            s.push_str(CUT_WGSL);
            s.push('\n');
            s.push_str(CUT_STAGE);
        }
    }
    if blend > 0 {
        s.push('\n');
        s.push_str(BLEND_WGSL);
        s.push('\n');
        s.push_str(if blend == 2 { REFRACT_WGSL } else { REFRACT_STUB });
    }
    s.push('\n');
    s.push_str(&dispatch_wgsl(recipes));
    s.push_str(post_wgsl(recipes));
    s.push('\n');
    s.push_str(&conditions_wgsl(layers));
    #[cfg(feature = "style")]
    if style {
        s.push('\n');
        s.push_str(crate::style::STYLE_WGSL);
    }
    s
}

/// `rc_dispatch`: one `case` per compiled recipe, calling its `rc_<name>`
/// with the level of its quality group; any other code (and 0, no recipe)
/// returns `o`, the lit stage's own values. Also one `RC_X_<NAME>` constant
/// per compiled recipe, its first global extras bit.
pub fn dispatch_wgsl(recipes: &[RecipeSrc]) -> String {
    let mut consts = String::new();
    let mut cases = String::new();
    for r in recipes {
        let (Some(row), Some(base)) = (stage_recipe(r.code), extra_base(r.code)) else {
            continue;
        };
        consts.push_str(&format!("const RC_X_{}: u32 = {base}u;\n", row.name.to_uppercase()));
        cases.push_str(&format!(
            "        case {}u: {{\n            r = rc_{}(b, f, i, rc_q({}u));\n        }}\n",
            row.code, row.name, row.group
        ));
    }
    format!(
        "{consts}fn rc_dispatch(b: RecBlock, f: MatFrame, i: MatIn, o: MatOut) -> MatOut {{\n    var r = o;\n    switch b.recipe {{\n{cases}        default: {{\n        }}\n    }}\n    return r;\n}}\n"
    )
}

/// `rc_post`, the last post-light step `mat_extra` calls: cloth's sheen and
/// fuzz (`rc_cloth_post`, B4-P3b) when cloth is compiled, then paint's clear
/// coat (`rc_paint_coat`, P10a) when paint is, else the colour unchanged, so
/// a game with neither carries one empty function. The cloth texts exist only
/// in builds with `recipe-cloth`, so a game without cloth carries none of
/// them.
pub fn post_wgsl(recipes: &[RecipeSrc]) -> &'static str {
    #[cfg(feature = "recipe-cloth")]
    if recipes.iter().any(|r| r.code == 10) {
        return if recipes.iter().any(|r| r.code == 2) {
            "fn rc_post(c: vec3<f32>, o: MatOut, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {
    return rc_paint_coat(rc_cloth_post(c, o, v, l, sun, vis), o, g, v, l, sun, vis);
}
"
        } else {
            "fn rc_post(c: vec3<f32>, o: MatOut, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {
    return rc_cloth_post(c, o, v, l, sun, vis);
}
"
        };
    }
    if recipes.iter().any(|r| r.code == 2) {
        "fn rc_post(c: vec3<f32>, o: MatOut, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {\n    return rc_paint_coat(c, o, g, v, l, sun, vis);\n}\n"
    } else {
        "fn rc_post(c: vec3<f32>, o: MatOut, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {\n    return c;\n}\n"
    }
}

/// Whether this build's `rc_post` calls cloth's sheen and fuzz when cloth is
/// in the set (`recipe-cloth`, B4-P3b); tests elsewhere check against it.
pub const CLOTH_POST: bool = cfg!(feature = "recipe-cloth");

/// The condition layer names by bit, as `cond_<name>` functions are named.
pub const LAYER_NAMES: [&str; 8] = ["wet", "mud", "dust", "snow", "wear", "oxide", "growth", "blood"];

/// `mat_conditions`: after the recipe, each compiled layer whose bit is in
/// the material's condition mask runs with its amount (`rc_amount`), all of
/// them on one shared noise sample `nz`. With no layer compiled, it returns
/// the recipe's output unchanged.
pub fn conditions_wgsl(layers: &[LayerSrc]) -> String {
    let mut calls = String::new();
    for l in layers {
        let Some(name) = LAYER_NAMES.get(usize::from(l.bit)) else {
            continue;
        };
        calls.push_str(&format!(
            "    if (((b.cond_mask >> {bit}u) & 1u) == 1u) {{\n        o = cond_{name}(b, rc_amount(b, {bit}u), o, i, f, nz);\n    }}\n",
            bit = l.bit
        ));
    }
    if calls.is_empty() {
        return "fn mat_conditions(b: RecBlock, f: MatFrame, i: MatIn, o0: MatOut) -> MatOut {\n    return o0;\n}\n".to_string();
    }
    format!(
        "fn mat_conditions(b: RecBlock, f: MatFrame, i: MatIn, o0: MatOut) -> MatOut {{\n    var o = o0;\n    if (b.cond_mask == 0u || (rc_frame.misc.z & 2u) != 0u) {{\n        return o;\n    }}\n    let nz = rc_vnoise3(i.world * 1.7, 0u);\n{calls}    return o;\n}}\n"
    )
}

/// One line edit: the first alternative whose text is found (after the
/// previous edit) is replaced. Alternatives let one edit hold on both the
/// plain and the character-patched or layered text.
type Edit = &'static [(&'static str, &'static str)];

/// The lines of the model lit stage the patch rewrites, in order. They hold
/// in `model_lit_fs` and `model_layered_fs`, with or without the character
/// patch (which turns `let base` into `var base` and appends to the
/// `occ_tex` line, and touches none of the others).
pub const EDITS: [Edit; 5] = [
    // 1. Before the discard, in uniform control flow: every derivative.
    &[(
        "let occ_tex = textureSample(occlusion_map, map_sampler, uv).r;",
        "let occ_tex = textureSample(occlusion_map, map_sampler, uv).r; let rf = mat_frame(in.world, in.local, uv, normalize(in.normal));",
    )],
    // 2. The colour becomes writable (already so when layered or char-patched).
    &[
        ("let base = tex * model_mat.base", "var base = tex * model_mat.base"),
        ("var base = tex * model_mat.base", "var base = tex * model_mat.base"),
    ],
    // 3. So does metalness.
    &[("let metal = clamp(surface.pbr.x * mr.b, 0.0, 1.0);", "var metal = clamp(surface.pbr.x * mr.b, 0.0, 1.0);")],
    // 4. After occ, before any lighting: the recipe and its conditions.
    &[(
        "let occ = mix(1.0, occ_tex, clamp(surface.pbr.z, 0.0, 1.0));",
        "var occ = mix(1.0, occ_tex, clamp(surface.pbr.z, 0.0, 1.0)); let ro = mat_surface(rf, MatIn(base, tex, in.color.rgb, n, g, in.world, in.local, in.lnorm, uv, rough, metal, occ)); base = ro.base; rough = mat_saa(rf, ro.rough); metal = ro.metal; n = ro.n; occ = ro.occ;",
    )],
    // 5. Before fog: emission, rim sheen, glints and paint's clear coat.
    &[(
        "let f = clamp(slot_fog(dist, in.world.y), 0.0, 1.0);",
        "c = mat_extra(c, ro, n, g, v, l, sun_rgb, vis); let f = clamp(slot_fog(dist, in.world.y), 0.0, 1.0);",
    )],
];

/// The cut-out's two line edits (B3-P2, feature `cutout`), applied after
/// [`EDITS`]: after the recipe (inside edit 4's own text) the holes, the wire
/// normal and the crossing shade ([`CUT_STAGE`]'s `mat_cut`, which discards);
/// on the return, a kept pixel the holes only partly cover is marked with
/// alpha 0, so the tonemap pass blends its 2x2 pattern (the forward pass's
/// screen-door fade mark). Both hold in every stage the patch serves, the
/// character patch's `char_out` return included.
pub const CUT_EDITS: [Edit; 2] = [
    &[("occ = ro.occ;", "occ = ro.occ; let rk = mat_cut(rf, uv, in.pos.xy, n); n = rk.n; occ = occ * rk.k; let rcut = rk.c;")],
    &[("mix(c, frame.fog_color.rgb, f), 1.0)", "mix(c, frame.fog_color.rgb, f), select(1.0, 0.0, rcut < 0.999))")],
];

/// The cut-out's pattern maths (`shaders/cut.wgsl`: the four patterns, their
/// coverage and the 2x2 threshold), stripped. It needs only `RecRaw`, so the
/// colour stage and evani-render's model shadow pass share it. A game's code
/// reaches it only with the cut-out on (feature `cutout`).
pub const CUT_WGSL: &str = stripped!("../shaders/cut.wgsl");

/// The colour stage's `mat_cut` (`shaders/cut_stage.wgsl`), stripped.
pub const CUT_STAGE: &str = stripped!("../shaders/cut_stage.wgsl");

/// The foliage recipe's code: only its material shader carries the oak leaf
/// spray's cut ([`CUT_LEAF`], [`CUT_LEAF_STAGE`]).
pub const FOLIAGE_CODE: u8 = 20;

/// Cut-out kind 7, the procedural oak's leaf sprays (`shaders/cut_leaf.wgsl`,
/// the Asset Studio oak session), stripped: in the foliage material's shader
/// and the shadow pass only, with [`CUT_WGSL`] spliced to reach it
/// ([`leaf_cut`]), so no other material's recipe stage changes by a byte.
pub const CUT_LEAF: &str = stripped!("../shaders/cut_leaf.wgsl");

/// The colour pass's shading of the leaf spray (`shaders/cut_leaf_stage.wgsl`),
/// stripped; foliage's shader only, [`CUT_STAGE`] spliced to call it
/// ([`leaf_cut_stage`]).
pub const CUT_LEAF_STAGE: &str = stripped!("../shaders/cut_leaf_stage.wgsl");

/// A source line as `stripped!` writes it (no line end).
fn stripped_line(s: &str) -> String {
    let mut buf = vec![0u8; s.len() + 2];
    let n = wgsl_strip(s.as_bytes(), &mut buf).min(buf.len());
    String::from_utf8_lossy(&buf[..n]).trim_end().to_string()
}

/// `text` with `insert` after the first `anchor` (a stripped source line),
/// or `None` when the anchor is missing.
fn splice_after(text: &str, anchor: &str, insert: &str) -> Option<String> {
    let a = stripped_line(anchor);
    let at = text.find(&a)? + a.len();
    Some(format!("{}{insert}{}", &text[..at], &text[at..]))
}

/// The cut-out's pattern text ([`CUT_WGSL`]) with the oak leaf spray spliced
/// in: `rc_cut_kind` names foliage's leaf with extras bit 3 kind 7,
/// `rc_cut_geo` hands kind 7 to `rc_cut_leaf` ([`CUT_LEAF`]), and a motif
/// fades to at least `g.top` of its cell when tiny (the spray's share; lace
/// leaves `top` at 0, so its 0.2 stays). Unchanged if an anchor is missing
/// (then leaves draw uncut; a test holds every anchor).
///
/// Extras bit 4 on the leaf (the rivers' crowns, Asset Studio column 20)
/// fills a far card: once its leaves are under a few pixels the card draws
/// as one solid, lobed leaf mass (a disc of 0.36 of the card with five and
/// three lobes, turned per card; its edge anti-aliased over a pixel), where
/// an unflagged card keeps a fifth of its pixels, blended with what lies
/// behind (whole crowns vanished to bare branches from 300 m). Round 10 kept
/// a share of each far card instead, but the screen-door pattern is the same
/// 2x2 for every card, so overlapping cards kept the same pixels and every
/// crown showed a dotted grid from the air. `g.top` 2 marks the far mass and
/// `g.p.y` carries its distance (both unused by kind 7 otherwise). No block
/// sets the bit unless asked, so every other leaf draws as before.
/// `rc_cut_cov`'s far fade with the leaf spray's share (`g.top`, at least
/// the lace's 0.2) and, for a far-filled card (`g.top` 2 or 3), its lobed
/// mass (`g.p.y`). On a tree's own sprays (`g.top` 2) the mass takes over
/// only once the leaves are under about a pixel and a half, so a crown 60 m
/// from the player keeps its leaves; on a card crown (`g.top` 3: a card
/// whose uv cell is past 255, the rivers' stands, hedges and hill woods) it
/// holds until the leaves are 5 to 10 pixels, so those few big cards read
/// as overlapping leaf clumps at any distance, never as see-through lace.
/// Every other motif fades as before.
const LEAF_FADE: &str = "let m = mix(select(max(0.2, g.top), clamp(0.5 - g.p.y / px, 0.0, 1.0), g.top > 1.5), clamp(0.5 - g.m / px, 0.0, 1.0), smoothstep(select(2.0, select(0.9, 5.0, g.top > 2.5), g.top > 1.5), select(4.0, select(1.8, 10.0, g.top > 2.5), g.top > 1.5), g.mc / px));";

pub fn leaf_cut(cut: &str) -> String {
    let go = || {
        let s = splice_after(cut, "fn rc_cut_kind(w: RecRaw) -> u32 {", "\nif((w.w0.x&0x8ffffu)==0x80014u){\nreturn 7u;\n}")?;
        let s = splice_after(&s, "fn rc_cut_geo(w: RecRaw, kind: u32, uv: vec2<f32>) -> RcCutG {", "\nif(kind==7u){\nvar g7=rc_cut_leaf(uv);\nif((w.w0.x&0x100000u)!=0u){\nlet c7=floor(uv);\nlet f7=uv-c7-vec2<f32>(.5,.47);\nlet a7=atan2(f7.y,f7.x)+rc_cut_h(c7,31u)*6.28;\ng7.p.y=length(f7)-.36*(1.+.16*sin(a7*5.)+.1*sin(a7*3.+2.));\ng7.top=select(2.,3.,c7.x>255.);\n}\nreturn g7;\n}")?;
        let fade = stripped_line("let m = mix(0.2, clamp(0.5 - g.m / px, 0.0, 1.0), smoothstep(2.0, 4.0, g.mc / px));");
        s.contains(&fade).then(|| s.replacen(&fade, &stripped_line(LEAF_FADE), 1))
    };
    go().unwrap_or_else(|| cut.to_string())
}

/// The colour stage's `mat_cut` ([`CUT_STAGE`]) with kind 7 shaded by
/// `rc_cut_leaf_n` ([`CUT_LEAF_STAGE`]) just before it returns. Unchanged if
/// the anchor is missing.
pub fn leaf_cut_stage(stage: &str) -> String {
    let r = stripped_line("return o;");
    match stage.rfind(&r) {
        Some(at) => format!("{}if(kind==7u){{\no=rc_cut_leaf_n(f,g,n,o);\n}}\n{}", &stage[..at], &stage[at..]),
        None => stage.to_string(),
    }
}

/// Whether this build cuts holes: the `cutout` feature with a recipe to cut.
pub const CUT_BUILT: bool = cfg!(feature = "cutout") && !COMPILED.is_empty();

/// The see-through pass's edit (B3-P3, feature `blend`), applied after
/// [`EDITS`] and the cut-out's: the recipe's colour, where it lands before any
/// lighting, is premultiplied by its alpha in the blend pipeline
/// ([`BLEND_WGSL`]'s `mat_premul`). The stage's final return is wrapped in
/// `mat_blend` as well ([`wrap_return`]).
pub const BLEND_EDITS: [Edit; 1] = [&[("base = ro.base;", "base = mat_premul(ro.base);")]];

/// The see-through pass's stage (`shaders/blend.wgsl`: the `REC_BLEND`
/// override, `mat_premul`, `mat_blend`), stripped. A game's code reaches it
/// only with the see-through pass on (feature `blend`).
pub const BLEND_WGSL: &str = stripped!("../shaders/blend.wgsl");

/// `mat_refract` without the copy (`shaders/refract_stub.wgsl`): refract-mode
/// glass drawn as over, in the opaque and blend pipelines' module.
pub const REFRACT_STUB: &str = stripped!("../shaders/refract_stub.wgsl");

/// `mat_refract` with the copy of the opaque colour (`shaders/refract.wgsl`:
/// group 2's texture and sampler), for the glass pipeline's module only.
pub const REFRACT_WGSL: &str = stripped!("../shaders/refract.wgsl");

/// The see-through shadows (`shaders/blend_shadow.wgsl`: a stipple on the
/// cut-out's depth pipeline, its density from the block), stripped; it needs
/// only `RecRaw`, like [`CUT_WGSL`]. evani-render appends it to the
/// hole-aware depth module with `material-recipes-blend`.
pub const BLEND_SHADOW_WGSL: &str = stripped!("../shaders/blend_shadow.wgsl");

/// The hole-aware depth module's tail (evani-render's `model_cut_shadows`):
/// with `cut` the cut-out's pattern maths ([`CUT_WGSL`]), with `blend` the
/// see-through stipple ([`BLEND_SHADOW_WGSL`]), and `rc_shadow_drop`, which
/// asks the stipple first (`rc_blend_drop`, -1 for a block that is not
/// see-through) and then the holes; whichever of the two is not built gets a
/// stand-in. evani-render passes its features as constants, so a game carries
/// only its own texts.
pub fn shadow_tail(cut: bool, blend: bool) -> String {
    let mut s = String::new();
    if cut {
        // The oak leaf spray's holes cast in the shadows only when the build
        // draws foliage.
        if COMPILED.iter().any(|r| r.code == FOLIAGE_CODE) {
            s.push_str(&leaf_cut(CUT_WGSL));
            s.push('\n');
            s.push_str(CUT_LEAF);
        } else {
            s.push_str(CUT_WGSL);
        }
        s.push('\n');
    }
    s.push_str(if blend { BLEND_SHADOW_WGSL } else { "fn rc_blend_drop(w:RecRaw,p:vec2<f32>)->i32{\nreturn -1;\n}\n" });
    s.push('\n');
    s.push_str(if cut {
        "fn rc_shadow_drop(w:RecRaw,uv:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,p:vec2<f32>)->bool{\nlet s=rc_blend_drop(w,p);\nif(s>=0){\nreturn s==1;\n}\nlet kind=rc_cut_kind(w);\nif(kind==0u||(cut_frame[2].z&4u)!=0u){\nreturn false;\n}\nreturn rc_cut_cov(rc_cut_geo(w,kind,uv),dx,dy,rc_cut_mpx(cut_frame[2].z))<rc_cut_th(p);\n}\n"
    } else {
        "fn rc_shadow_drop(w:RecRaw,uv:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,p:vec2<f32>)->bool{\nreturn rc_blend_drop(w,p)==1;\n}\n"
    });
    s
}

/// Whether this build draws see-through materials: the `blend` feature with
/// a recipe compiled.
pub const BLEND_BUILT: bool = cfg!(feature = "blend") && !COMPILED.is_empty();

/// The colour of the stage's final return (the body's last `return`: the
/// `vec4<f32>(mix(c, frame.fog_color.rgb, f), ..)` it returns, or that is the
/// first argument of the character patch's `char_out`, whose result is the
/// coverage struct) wrapped as `mat_blend(<colour>, ro, f, in.pos, g,
/// in.world)`; `None` when there is no such return. Holds in every stage the
/// patch serves (the cut-out's edge mark included).
pub fn wrap_return(body: &mut String) -> Option<()> {
    // The only place the body mixes in the fog colour is its final return.
    let start = body.find("vec4<f32>(mix(c, frame.fog_color.rgb, f)")?;
    let mut depth = 0i32;
    let mut end = None;
    for (k, ch) in body.bytes().enumerate().skip(start) {
        match ch {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(k + 1);
                    break;
                }
            }
            b';' => return None,
            _ => {}
        }
    }
    body.insert_str(end?, ", ro, f, in.pos, g, in.world)");
    body.insert_str(start, "mat_blend(");
    Some(())
}

/// Every anchor text of [`EDITS`] (each alternative), for the anchor tests.
pub fn anchors() -> impl Iterator<Item = &'static str> {
    EDITS.iter().flat_map(|e| e.iter().map(|&(find, _)| find))
}

/// `s` without `//` comments, keeping `//@` slot markers and `// evani.`
/// names (as the engine strips its own WGSL; WGSL has no strings).
pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        match line.find("//") {
            Some(i) if !line[i..].starts_with("//@") && !line[i..].starts_with("// evani.") => {
                out.push_str(line[..i].trim_end());
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ => out.push_str(line),
        }
    }
    out
}

/// The forward module `template` with `model_lit_fs` wired to the recipe
/// stage, for a pipeline with `msaa` samples (1 or 4), or `None` when a line
/// to patch is missing. `template` may already carry the character patch.
pub fn patch_lit(template: &str, msaa: u32) -> Option<String> {
    patch_entry(template, "fn model_lit_fs(", msaa, &stage_wgsl(), CUT_BUILT, BLEND_BUILT)
}

/// The glass pipeline's module (B3-P3, feature `refract`): [`patch_lit`] with
/// the copy's `mat_refract` instead of the stub, one sample.
#[cfg(feature = "refract")]
pub fn patch_lit_refract(template: &str) -> Option<String> {
    patch_entry(template, "fn model_lit_fs(", 1, &stage_text(COMPILED, LAYERS, RC_LAYOUT, CUT_BUILT, 2), CUT_BUILT, true)
}

/// [`patch_layered`] for the glass pipeline (B3-P3, feature `refract`).
#[cfg(feature = "refract")]
pub fn patch_layered_refract(template: &str) -> Option<String> {
    patch_entry(template, "fn model_layered_fs(", 1, &stage_text(COMPILED, LAYERS, RC_LAYOUT, CUT_BUILT, 2), CUT_BUILT, true)
}

/// [`patch_lit_with`] with the see-through pass (`refract` the glass
/// pipeline's text), whatever the features (tests, tools).
pub fn patch_lit_blend_with(template: &str, msaa: u32, recipes: &[RecipeSrc], layers: &[LayerSrc], refract: bool) -> Option<String> {
    patch_entry(template, "fn model_lit_fs(", msaa, &stage_wgsl_blend_with(recipes, layers, refract), false, !recipes.is_empty())
}

/// [`patch_layered_with`] with the see-through pass (tests, tools).
pub fn patch_layered_blend_with(template: &str, msaa: u32, recipes: &[RecipeSrc], layers: &[LayerSrc], refract: bool) -> Option<String> {
    patch_entry(template, "fn model_layered_fs(", msaa, &stage_wgsl_blend_with(recipes, layers, refract), false, !recipes.is_empty())
}

/// [`patch_lit`] with a given set of recipes and layers (tests, tools).
pub fn patch_lit_with(template: &str, msaa: u32, recipes: &[RecipeSrc], layers: &[LayerSrc]) -> Option<String> {
    patch_entry(template, "fn model_lit_fs(", msaa, &stage_wgsl_with(recipes, layers), false, false)
}

/// [`patch_lit_with`] with the cut-out as well, whatever the features
/// (tests, tools).
pub fn patch_lit_cut_with(template: &str, msaa: u32, recipes: &[RecipeSrc], layers: &[LayerSrc]) -> Option<String> {
    patch_entry(template, "fn model_lit_fs(", msaa, &stage_wgsl_cut_with(recipes, layers), !recipes.is_empty(), false)
}

/// [`patch_lit`] for a module whose model pipelines draw the layered stage
/// `model_layered_fs` (evani-render `material-layers` on): that body gets
/// the edits, `model_lit_fs` stays unpatched and unused. The recipe edits the
/// inputs and the layered stage adds its coat and sheen lobes on top.
pub fn patch_layered(template: &str, msaa: u32) -> Option<String> {
    patch_entry(template, "fn model_layered_fs(", msaa, &stage_wgsl(), CUT_BUILT, BLEND_BUILT)
}

/// [`patch_layered`] with a given set of recipes and layers (tests, tools).
pub fn patch_layered_with(template: &str, msaa: u32, recipes: &[RecipeSrc], layers: &[LayerSrc]) -> Option<String> {
    patch_entry(template, "fn model_layered_fs(", msaa, &stage_wgsl_with(recipes, layers), false, false)
}

/// [`patch_layered_with`] with the cut-out as well (tests, tools).
pub fn patch_layered_cut_with(template: &str, msaa: u32, recipes: &[RecipeSrc], layers: &[LayerSrc]) -> Option<String> {
    patch_entry(template, "fn model_layered_fs(", msaa, &stage_wgsl_cut_with(recipes, layers), !recipes.is_empty(), false)
}

/// `edits`, in order, from the start of `body`; `None` if one is missing.
fn apply_edits(body: &mut String, edits: &[Edit]) -> Option<()> {
    let mut from = 0;
    for alts in edits {
        let (i, find, with) = alts.iter().find_map(|&(find, with)| body[from..].find(find).map(|i| (from + i, find, with)))?;
        body.replace_range(i..i + find.len(), with);
        from = i + with.len();
    }
    Some(())
}

/// [`EDITS`], in order, inside the body of `entry` in `template` (up to its
/// closing brace at the start of a line), with `cut` then [`CUT_EDITS`], with
/// `blend` then [`BLEND_EDITS`] and [`wrap_return`], then `stage`.
pub(crate) fn patch_entry(template: &str, entry: &str, msaa: u32, stage: &str, cut: bool, blend: bool) -> Option<String> {
    let at = template.find(entry)?;
    let end = at + template[at..].find("\n}")? + 2;
    let mut body = template[at..end].to_string();
    apply_edits(&mut body, &EDITS)?;
    let (pre, local) = with_local(&template[..at]);
    if !local {
        // No object-local position in this module: the world one stands in.
        body = body.replace("in.world, in.local, in.lnorm, uv,", "in.world, in.world, vec3<f32>(0.0), uv,").replace("mat_frame(in.world, in.local, uv,", "mat_frame(in.world, in.world, uv,");
    }
    if cut {
        apply_edits(&mut body, &CUT_EDITS)?;
    }
    if blend {
        apply_edits(&mut body, &BLEND_EDITS)?;
        wrap_return(&mut body)?;
    }
    Some(format!(
        "{}{body}{}\nconst REC_MSAA: u32 = {}u;\n{}",
        pre,
        &template[end..],
        msaa.max(1),
        stage
    ))
}

/// The module text before the stage's entry with the object-local position
/// and normal added to the vertex output (procedural realism round 9,
/// CORE_REQUESTS `object-local-position`): `ModelOut` gains `local` and
/// `lnorm` (locations 7 and 8) and every vertex shader that builds the
/// position from the mesh's own (`let local = vec4<f32>(in.pos_offset...`)
/// sets them. `false` (the text as it was) where there is no such struct or
/// vertex shader: the recipe stage then reads the world position.
fn with_local(pre: &str) -> (String, bool) {
    const FIELD: &str = "@location(6) sun: vec4<f32>,";
    const SET: &str = "o.world = world;";
    let Some(fi) = pre.find(FIELD) else { return (pre.to_string(), false) };
    let mut out = String::with_capacity(pre.len() + 256);
    out.push_str(&pre[..fi + FIELD.len()]);
    out.push_str(" @location(7) local: vec3<f32>, @location(8) lnorm: vec3<f32>,");
    let mut rest = &pre[fi + FIELD.len()..];
    let mut any = false;
    while let Some(k) = rest.find(SET) {
        let fn_at = rest[..k].rfind("fn ").unwrap_or(0);
        out.push_str(&rest[..k + SET.len()]);
        if rest[fn_at..k].contains("let local = vec4<f32>(in.pos_offset") {
            out.push_str(" o.local = local.xyz; o.lnorm = in.normal.xyz;");
            any = true;
        }
        rest = &rest[k + SET.len()..];
    }
    out.push_str(rest);
    if any { (out, true) } else { (pre.to_string(), false) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round 9: the forward module's model stage carries the object-local
    /// position into the recipe stage.
    #[test]
    fn the_object_local_position_reaches_the_recipe_stage() {
        let m = concat!(include_str!("../../evani-render/shaders/model.wgsl"), include_str!("../../evani-render/shaders/model_lit.wgsl"));
        let p = patch_lit(m, 1).expect("patched");
        assert!(p.contains("@location(7) local: vec3<f32>"));
        assert!(p.contains("o.local = local.xyz;"));
        assert!(p.contains("MatIn(base, tex, in.color.rgb, n, g, in.world, in.local, in.lnorm, uv,"));
        assert!(p.contains("mat_frame(in.world, in.local, uv,"));
        let q = patch_lit(include_str!("../../evani-render/shaders/model_lit.wgsl"), 1).expect("patched");
        assert!(q.contains("MatIn(base, tex, in.color.rgb, n, g, in.world, in.world, vec3<f32>(0.0), uv,"), "no vertex stage: the world position stands in");
    }

    const LIT: &str = include_str!("../../evani-render/shaders/model_lit.wgsl");
    const LAYERED: &str = include_str!("../../evani-render/shaders/model_lit_layers.wgsl");

    #[test]
    fn every_line_is_in_the_lit_stage_in_order() {
        let p = patch_lit(LIT, 4).expect("all lines found");
        assert!(p.contains("const REC_MSAA: u32 = 4u;"));
        assert!(p.contains("// evani.shader.recipes"));
        assert!(!p.contains("Evani.Dev; the noise"), "comments are stripped");
        assert_eq!(p.matches("mat_surface(").count(), 2, "one call, one definition");
        assert_eq!(p.matches("mat_frame(").count(), 2);
        assert_eq!(p.matches("mat_extra(").count(), 2);
        assert!(p.contains("var base = tex * model_mat.base"));
        assert!(p.contains("var metal = "));
        assert!(patch_lit(LIT, 1).is_some_and(|s| s.contains("const REC_MSAA: u32 = 1u;")));
    }

    #[test]
    fn a_missing_line_means_no_patch() {
        assert!(patch_lit("fn model_lit_fs() {\n}\n", 1).is_none());
        assert!(patch_lit("fn model_fs() {}", 1).is_none());
        for a in anchors() {
            if a.starts_with("var base") {
                continue; // the plain stage only has the `let base` form
            }
            assert!(LIT.contains(a), "{a}");
            assert!(patch_lit(&LIT.replace(a, "let other = 0.0;"), 1).is_none(), "{a}");
        }
    }

    /// The edits stay inside the stage they patch.
    #[test]
    fn the_layered_stage_patches_and_the_plain_one_stays() {
        let both = format!("{LIT}\n{LAYERED}");
        let p = patch_layered(&both, 4).expect("all lines found in model_layered_fs");
        let (plain, layered) = p.split_at(p.find("fn model_layered_fs(").expect("layered stage"));
        assert!(!plain.contains("let ro = mat_surface("), "model_lit_fs is not patched");
        assert!(layered.contains("let ro = mat_surface("));
        assert!(patch_layered(LIT, 1).is_none(), "no layered stage: nothing to patch");
        let lit_only = patch_lit(&both, 4).expect("plain stage");
        let (plain, layered) = lit_only.split_at(lit_only.find("fn model_layered_fs(").expect("layered stage"));
        assert!(plain.contains("let ro = mat_surface("));
        assert!(!layered.contains("let ro = mat_surface("));
    }

    /// Recipe 0, and any code without a compiled recipe, keeps the lit
    /// stage's own values: the dispatch defaults to `o`, mat_surface returns
    /// `mat_identity` for 0, and the identity copies every input.
    #[test]
    fn recipe_zero_is_the_identity() {
        let d = dispatch_wgsl(&[]);
        assert!(d.contains("var r = o;") && d.contains("default: {") && d.ends_with("return r;\n}\n"));
        assert!(!d.contains("case "));
        let core = CORE_STRIPPED;
        // The compile-time strip is the runtime one with indentation, blank
        // lines, trailing spaces and the spaces beside punctuation gone.
        let tight = |s: &str| s.replace(' ', "");
        let squash = |s: &str| s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        assert_eq!(tight(core), tight(&(squash(&strip(CORE_WGSL)) + "\n")));
        assert_eq!(core.matches("//").count(), core.matches("// evani.").count(), "only markers stay");
        assert!(core.starts_with("// evani.shader.recipes\n"), "marker lines keep their spaces");
        assert!(!core.contains("\n ") && !core.contains("\n\n") && !core.contains('\r'));
        assert!(tight(core).contains(&tight("if (b.recipe == 0u || (rc_frame.misc.z & 1u) != 0u) {\nreturn o;")));
        assert!(tight(core).contains(&tight("return MatOut(i.base, i.rough, i.metal, i.n, i.occ, vec3<f32>(0.0), vec4<f32>(0.0), vec3<f32>(0.0));")));
        assert_eq!(conditions_wgsl(&[]), "fn mat_conditions(b: RecBlock, f: MatFrame, i: MatIn, o0: MatOut) -> MatOut {\n    return o0;\n}\n");
    }

    #[test]
    fn dispatch_and_conditions_follow_the_compiled_rows() {
        let d = dispatch_wgsl(&[RecipeSrc { code: 12, wgsl: "" }, RecipeSrc { code: 16, wgsl: "" }]);
        assert!(d.contains("const RC_X_FUR: u32 = 51u;"));
        assert!(d.contains("case 12u: {\n            r = rc_fur(b, f, i, rc_q(1u));"));
        assert!(!d.contains("16u"), "skin is a family recipe, not a stage case");
        let c = conditions_wgsl(&[LayerSrc { bit: 0, wgsl: "" }, LayerSrc { bit: 7, wgsl: "" }]);
        assert!(c.contains("o = cond_wet(b, rc_amount(b, 0u), o, i, f, nz);"));
        assert!(c.contains("o = cond_blood(b, rc_amount(b, 7u), o, i, f, nz);"));
    }

    /// B4-P3b: with cloth compiled, `rc_post` runs cloth's sheen and fuzz
    /// before paint's coat; without cloth in the set it is the old text.
    #[test]
    fn cloth_sheen_runs_after_lighting_exactly_with_cloth() {
        let cloth = RecipeSrc { code: 10, wgsl: "" };
        let paint = RecipeSrc { code: 2, wgsl: "" };
        let wood = RecipeSrc { code: 4, wgsl: "" };
        assert!(!post_wgsl(&[wood]).contains("rc_cloth_post"));
        assert!(post_wgsl(&[paint]).contains("return rc_paint_coat(c, o"));
        assert_eq!(post_wgsl(&[cloth]).contains("return rc_cloth_post(c, o, v, l, sun, vis);"), CLOTH_POST);
        assert_eq!(post_wgsl(&[paint, cloth]).contains("return rc_paint_coat(rc_cloth_post(c, o, v, l, sun, vis), o"), CLOTH_POST);
        // The shader side: rc_cloth leaves o.sheen.w below -0.5 (its code
        // for the post step), every other recipe's rim power is above 0.
        assert!(ALL_RECIPES.iter().any(|r| r.code == 10 && r.wgsl.contains("fn rc_cloth_post(c:vec3<f32>,o:MatOut,v:vec3<f32>,l:vec3<f32>,sun:vec3<f32>,vis:f32)->vec3<f32>")));
    }

    /// The oak leaf spray (cut-out kind 7) splices into the cut-out's texts
    /// at every anchor, and a stage without foliage keeps them as they were.
    #[test]
    fn the_leaf_cut_splices_only_into_foliage() {
        let c = leaf_cut(CUT_WGSL);
        assert!(c.contains("if((w.w0.x&0x8ffffu)==0x80014u){\nreturn 7u;\n}"), "rc_cut_kind's kind 7");
        assert!(c.contains("if(kind==7u){\nvar g7=rc_cut_leaf(uv);\nif((w.w0.x&0x100000u)!=0u){"), "rc_cut_geo's call, with the far fill");
        assert!(c.contains("g7.top=select(2.,3.,c7.x>255.);"), "the far mass");
        assert!(c.contains(&stripped_line(LEAF_FADE)), "the far fade");
        let st = leaf_cut_stage(CUT_STAGE);
        assert!(st.contains("o=rc_cut_leaf_n(f,g,n,o);"), "mat_cut's shading");
        assert!(CUT_LEAF.contains("fn rc_cut_leaf(") && CUT_LEAF_STAGE.contains("fn rc_cut_leaf_n("));
        let wood = [RecipeSrc { code: 4, wgsl: "" }];
        let s = stage_text_style(&wood, &[], "", true, 0, false);
        assert!(s.contains(CUT_WGSL) && !s.contains("rc_cut_leaf"), "no other material carries it");
        let leaf = [RecipeSrc { code: FOLIAGE_CODE, wgsl: "" }];
        assert!(stage_text_style(&leaf, &[], "", true, 0, false).contains("fn rc_cut_leaf_n("));
    }

    #[test]
    fn extras_bits_are_consecutive_and_fit() {
        assert_eq!(extra_base(1), Some(0));
        assert_eq!(extra_base(2), Some(7));
        assert_eq!(extra_base(16), None);
        let total: u32 = STAGE.iter().map(|r| u32::from(r.extras)).sum();
        assert_eq!(total, 88); // batch 1's 45, batch 2's 9 (3 ground, 2 bark, 1 ice, 3 foliage), batch 3's 23 (glass's backlight), leather crazed (B4-P2), hide bristles (B5-P2), cloth sett and slubs (B4-P3a), keratin spots and thin film (P7-2), metal temper and ceramic lustre (P8-4), thin damask, organic and gem clear (material-view round 4)
        assert!(total <= 128, "the deny bits are one vec4<u32>");
        assert!(STAGE.windows(2).all(|w| w[0].code < w[1].code));
        assert!(STAGE.iter().all(|r| usize::from(r.group) < GROUPS.len() && r.extras <= 8));
    }

    #[test]
    fn extras_names_follow_the_stage_table() {
        for (row, names) in STAGE.iter().zip(EXTRA_NAMES) {
            assert_eq!(names.len(), usize::from(row.extras), "{}", row.name);
        }
        assert_eq!(extra_name(0).as_deref(), Some("material.recipe.metal.grain"));
        assert_eq!(extra_name(3).as_deref(), Some("material.recipe.metal.studs"));
        assert_eq!(extra_name(6).as_deref(), Some("material.recipe.metal.temper"));
        assert_eq!(extra_name(8).as_deref(), Some("material.recipe.paint.flake"));
        assert_eq!(extra_name(46).as_deref(), Some("material.recipe.cloth.sett"));
        assert_eq!(extra_name(47).as_deref(), Some("material.recipe.cloth.slubs"));
        assert_eq!(extra_name(50).as_deref(), Some("material.recipe.leather.crazed"));
        assert_eq!(extra_name(59).as_deref(), Some("material.recipe.hide.bristles"));
        assert_eq!(extra_name(62).as_deref(), Some("material.recipe.keratin.spots"));
        assert_eq!(extra_name(64).as_deref(), Some("material.recipe.foliage.veins"));
        assert_eq!(extra_name(69).as_deref(), Some("material.recipe.thin.damask"));
        assert_eq!(extra_name(70).as_deref(), Some("material.recipe.thin.film"));
        assert_eq!(extra_name(72).as_deref(), Some("material.recipe.organic.clear"));
        assert_eq!(extra_name(76).as_deref(), Some("material.recipe.ceramic.lustre"));
        assert_eq!(extra_name(77).as_deref(), Some("material.recipe.glass.backlight"));
        assert_eq!(extra_name(80).as_deref(), Some("material.recipe.gem.clear"));
        assert_eq!(extra_name(87).as_deref(), Some("material.recipe.glow.hologram"));
        assert_eq!(extra_name(88), None);
    }

    /// No two extras share a global bit, whatever a later package adds: every
    /// (recipe, extra) lands on its own bit, every bit below the total names
    /// exactly one extra, and every `rc_extra` read in a recipe shader names
    /// that recipe's own base with a slot inside its count (leather's crazed
    /// and hide's bristles shifted every later bit by one each).
    #[test]
    fn no_two_extras_share_a_bit() {
        let mut owner = std::collections::BTreeMap::new();
        for (row, names) in STAGE.iter().zip(EXTRA_NAMES) {
            let base = extra_base(row.code).expect("every stage row has a base");
            let mut seen = std::collections::BTreeSet::new();
            for (k, e) in names.iter().enumerate() {
                assert!(seen.insert(*e), "{}.{e} is listed twice", row.name);
                let bit = base + k as u32;
                let name = format!("material.recipe.{}.{e}", row.name);
                assert_eq!(extra_name(bit).as_deref(), Some(name.as_str()));
                if let Some(prev) = owner.insert(bit, name.clone()) {
                    panic!("bit {bit} is both {prev} and {name}");
                }
            }
        }
        let total: u32 = STAGE.iter().map(|r| u32::from(r.extras)).sum();
        assert_eq!(owner.keys().copied().collect::<Vec<_>>(), (0..total).collect::<Vec<_>>(), "no gaps, no overlaps");

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let mut reads = 0;
        for row in &STAGE {
            let src = std::fs::read_to_string(dir.join(format!("rc_{}.wgsl", row.name))).expect("each stage recipe has a shader");
            let own = format!("RC_X_{}", row.name.to_uppercase());
            for (at, _) in src.match_indices("rc_extra(b, ") {
                let call = &src[at + "rc_extra(b, ".len()..];
                let (base, rest) = call.split_once(", ").expect("rc_extra(b, BASE, Ku)");
                assert_eq!(base, own, "rc_{}.wgsl reads another recipe's extras", row.name);
                let k: u8 = rest[..rest.find('u').expect("slot ends in u")].parse().expect("a literal slot");
                assert!(k < row.extras, "rc_{}.wgsl reads slot {k} of {}", row.name, row.extras);
                reads += 1;
            }
        }
        assert!(reads >= total, "the scan found the reads ({reads})");
    }

    /// The tight strip drops spaces beside punctuation but never lets two
    /// tokens merge.
    #[test]
    fn the_tight_strip_keeps_tokens_apart() {
        const SRC: &str = "fn f(a: f32) -> vec3<f32> {\n    let x = a - -b * c; // gone\n    let t: vec2<f32> = y;\n    return vec3<f32>(x >= 1.0, a & &b, 0.5);\n}\n// evani.k v\n";
        const B: [u8; wgsl_strip(SRC.as_bytes(), &mut [])] = wgsl_stripped(SRC.as_bytes());
        assert_eq!(
            utf8(&B),
            "fn f(a:f32)->vec3<f32>{\nlet x=a- -b*c;\nlet t:vec2<f32> =y;\nreturn vec3<f32>(x>=1.0,a& &b,0.5);\n}\n// evani.k v\n"
        );
    }

    /// B3-P1: the pattern layouts reach a stage only with a recipe that calls
    /// them (once, after the shared helpers), ship stripped, and take no
    /// screen derivative.
    #[test]
    fn the_layouts_follow_the_recipes_that_use_them() {
        assert!(ALL_LAYOUT.starts_with("// evani.shader.recipes.layout\n"));
        assert_eq!(ALL_LAYOUT.matches("//").count(), 1, "only the marker survives the strip");
        for bad in ["dpdx", "dpdy", "fwidth", "textureSample", "model_mat", "rc_frame"] {
            assert!(!ALL_LAYOUT.contains(bad), "the layouts use {bad}");
        }
        assert_eq!(cfg!(feature = "layout"), !RC_LAYOUT.is_empty());
        let user = RecipeSrc { code: 7, wgsl: "fn rc_masonry() { rc_lay_bond(); }" };
        let other = RecipeSrc { code: 5, wgsl: "fn rc_bark() {}" };
        let with = stage_wgsl_with(&[other, user], &[]);
        assert_eq!(with.matches("fn rc_lay_bond(").count(), 1);
        assert!(with.find("fn rc_out(").unwrap() < with.find("fn rc_lay_bond(").unwrap(), "after the shared helpers");
        assert!(!stage_wgsl_with(&[other], &[]).contains("rc_lay_"), "no layout without a recipe that draws one");
        assert!(!stage_wgsl_with(&[], &[]).contains("rc_lay_"));
        assert_eq!(stage_wgsl().contains("fn rc_lay_"), COMPILED.iter().any(uses_layout));
    }

    #[test]
    fn strip_keeps_markers_and_code() {
        assert_eq!(strip("a // x\n//@slot t\n// evani.k\nb\n// gone\n"), "a\n//@slot t\n// evani.k\nb\n\n");
    }
}

/// Guard (critic round 3 of material-view: crystal read as an opaque white
/// ball and hide as smooth veins on Low, against a cut stone and a deep
/// crackle on Ultra). What makes them read as themselves starts at Low (level
/// 1): the gem's facet normals, dispersion and rim, the hide's wrinkle
/// normals and scatter. Only extra octaves and sparkle stay higher.
#[cfg(test)]
mod low_parity_tests {
    #[test]
    fn low_keeps_what_makes_gem_and_hide_read() {
        let gem = include_str!("../shaders/rc_gem.wgsl").replace("\r\n", "\n");
        assert!(gem.contains("if (q >= 1u) {\n        // A rainbow that turns"), "gem dispersion from Low");
        assert!(gem.contains("if (q >= 1u) {\n        o.sheen = vec4<f32>(mix(b.col_a, hue, b.na.x) * 0.5, 3.0);"), "gem rim and facet normals from Low");
        let hide = include_str!("../shaders/rc_hide.wgsl").replace("\r\n", "\n");
        assert!(hide.contains("if (q >= 1u) {\n        if (rc_extra(b, RC_X_HIDE, 1u)) {"), "hide wrinkle normals from Low");
    }
}
