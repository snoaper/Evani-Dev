//! Relief surfaces (feature `relief`): brick, cobble, tiles and wood planks
//! with real relief (a bent normal from the height, and a roughness) through
//! evani-render's opt-in kit-surface hook, plus wet, puddle and moss layers.
//!
//! * [`pattern`]: the four patterns, written once in Rust, periodic and
//!   bit-exact (integer hashes, no transcendental maths).
//! * [`bake`]: a deterministic load-time bake of each pattern into an
//!   N x N tile with mips (nothing is downloaded), packed two exact-integer
//!   floats per texel.
//! * [`params`]: the live params, `material.relief.*`.
//! * [`system`]: the [`Relief`] resource and its frame system, which runs
//!   after `material_proc.draw`, appends the relief block to the world's
//!   `kit_data` and swaps the kit code for one that defines `kit_surface`.
//!
//! Using it: give box faces (or regions) the [`surface`] ids below; call
//! [`install`] after `evani_material_proc::install`. Box faces map planar
//! on the face's pattern coordinates; with [`ReliefConfig::free_surface`]
//! every other mesh drawn with the world material (rocks, columns) gets that
//! surface triplanar.
//!
//! Tiers (`Tiers::active().id`): `lite` paints each relief surface its flat
//! mean colour (a 16 x 16 bake for the means only), `low` the baked colour
//! with no relief (bake capped at 128), every other tier the full relief.

pub mod bake;
pub mod params;
pub mod pattern;
pub mod system;
#[cfg(test)]
mod tests;

pub use bake::{SurfaceBake, Texel};
pub use params::ReliefParams;
pub use pattern::{Brick, Cobble, Pattern, Planks, Tiles};
pub use system::{Relief, install, system};

/// The relief surface ids (box faces and regions). The plain world shader
/// paints them grey; with this module on they get their relief.
pub mod surface {
    pub const BRICK: u32 = 32;
    pub const COBBLE: u32 = 33;
    pub const TILES: u32 = 34;
    pub const PLANKS: u32 = 35;
    /// The first relief id, and how many there are.
    pub const FIRST: u32 = 32;
    pub const COUNT: u32 = 4;
}

/// The block's first float: a mark the shader checks before reading.
pub const MAGIC: f32 = 7177.0;
/// Records before the per-surface records.
pub const HEADER_RECS: usize = 4;
/// Records per surface.
pub const SURF_RECS: usize = 4;

/// The weather layers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layers {
    /// 0 dry .. 1 soaked: darker, glossier, joints fill with water.
    pub wetness: f32,
    /// 0 none .. 1 most of the ground: puddles in the low spots of upward
    /// faces.
    pub puddles: f32,
    /// The puddle pattern's size (m).
    pub puddle_scale: f32,
    /// 0 none .. 1 overgrown: moss in joints and on top faces.
    pub moss: f32,
    pub moss_color: [f32; 3],
    /// The moss patches' size (m).
    pub moss_scale: f32,
}

impl Default for Layers {
    fn default() -> Self {
        Layers {
            wetness: 0.0,
            puddles: 0.0,
            puddle_scale: 2.5,
            moss: 0.0,
            moss_color: [0.07, 0.11, 0.03],
            moss_scale: 0.6,
        }
    }
}

/// Everything the relief paints with.
#[derive(Debug, Clone, PartialEq)]
pub struct ReliefConfig {
    pub brick: Brick,
    pub cobble: Cobble,
    pub tiles: Tiles,
    pub planks: Planks,
    /// Each surface's colour (linear, brick, cobble, tiles, planks); the
    /// baked colour is multiplied by colour / the pattern's base colour.
    pub colors: [[f32; 3]; 4],
    /// The bent normal's strength (0 = flat, 1 = as baked).
    pub height_strength: f32,
    pub layers: Layers,
    /// Texels a side of each baked tile (16..256, a power of two).
    pub bake_size: u32,
    /// The relief surface id painted triplanar on world meshes that are
    /// not a box face (None: they stay grey).
    pub free_surface: Option<u32>,
    /// Debug view (feature `relief-debug` only): 0 off, 1 puddle mask,
    /// 2 moss mask, 3 roughness.
    pub debug: u32,
}

impl Default for ReliefConfig {
    fn default() -> Self {
        ReliefConfig {
            brick: Brick::default(),
            cobble: Cobble::default(),
            tiles: Tiles::default(),
            planks: Planks::default(),
            colors: [
                pattern::BRICK_COLOR,
                pattern::COBBLE_COLOR,
                pattern::TILES_COLOR,
                pattern::WOOD_COLOR,
            ],
            height_strength: 1.0,
            layers: Layers::default(),
            bake_size: 256,
            free_surface: None,
            debug: 0,
        }
    }
}

impl ReliefConfig {
    /// The four patterns, in surface order.
    pub fn patterns(&self) -> [Pattern; 4] {
        [
            Pattern::Brick(self.brick),
            Pattern::Cobble(self.cobble),
            Pattern::Tiles(self.tiles),
            Pattern::Planks(self.planks),
        ]
    }
}

/// The shader mode for a tier: 0 flat colour, 1 baked colour, 2 full.
pub fn tier_mode(tier: &str) -> u32 {
    match tier {
        "lite" => 0,
        "low" => 1,
        _ => 2,
    }
}

/// The bake size a tier uses for a configured `size`.
pub fn tier_bake_size(size: u32, tier: &str) -> u32 {
    match tier_mode(tier) {
        0 => bake::MIN_SIZE,
        1 => bake::bake_size(size).min(128),
        _ => bake::bake_size(size),
    }
}

/// The header and per-surface records (everything but the texels).
pub fn header(cfg: &ReliefConfig, bakes: &[SurfaceBake], mode: u32) -> Vec<[f32; 4]> {
    let l = &cfg.layers;
    let free = cfg
        .free_surface
        .and_then(|s| s.checked_sub(surface::FIRST))
        .filter(|k| (*k as usize) < bakes.len())
        .map_or(-1.0, |k| k as f32);
    let mut d = vec![
        [MAGIC, bakes.len() as f32, mode as f32, cfg.height_strength],
        [l.wetness, l.puddles, l.moss, l.puddle_scale],
        [l.moss_color[0], l.moss_color[1], l.moss_color[2], l.moss_scale],
        [free, cfg.debug as f32, 0.0, 0.0],
    ];
    let mut first = 4 * (HEADER_RECS + SURF_RECS * bakes.len());
    for (k, b) in bakes.iter().enumerate() {
        let base = cfg.patterns()[k.min(3)].base_color();
        let col = cfg.colors.get(k).copied().unwrap_or(base);
        let mul: [f32; 3] = std::array::from_fn(|i| col[i] / base[i].max(1e-4));
        d.push([first as f32, b.n as f32, b.levels as f32, b.slope_scale]);
        d.push([b.tile[0], b.tile[1], b.depth, 0.0]);
        d.push([b.mean_color[0], b.mean_color[1], b.mean_color[2], b.mean_rough]);
        d.push([mul[0], mul[1], mul[2], 0.0]);
        first += 2 * b.texels.len();
    }
    d
}

/// The whole block: header, surface records, then every surface's texels
/// (floats from the block start; each texel's two floats share a record
/// half, so a surface starts on an even float).
pub fn pack(cfg: &ReliefConfig, bakes: &[SurfaceBake], mode: u32) -> Vec<[f32; 4]> {
    let mut d = header(cfg, bakes, mode);
    let floats: usize = bakes.iter().map(|b| 2 * b.texels.len()).sum();
    let start = d.len();
    d.resize(start + floats.div_ceil(4), [0.0; 4]);
    let mut f = 4 * start;
    for b in bakes {
        for t in &b.texels {
            d[f / 4][f % 4] = t[0];
            d[f / 4][f % 4 + 1] = t[1];
            f += 2;
        }
    }
    d
}

/// The relief part of the kit code (appended after the surfaces and sky).
pub const WGSL: &str = include_str!("../../shaders/relief.wgsl");
/// Its entry: `kit_surface` and the plain `kit_material`.
pub const ENTRY: &str = include_str!("../../shaders/relief_entry.wgsl");
/// The debug views, spliced at the `//$DEBUG` mark with feature
/// `relief-debug` (otherwise the mark is dropped).
pub const DEBUG: &str = include_str!("../../shaders/relief_debug.wgsl");
/// The first line of the plain entry, which the relief entry replaces.
pub const PLAIN_ENTRY_MARK: &str = "// ---- entry ---";

/// The kit code with relief: the plain code (`shader::wgsl`) without its
/// entry, then the relief part reading the block at record `base`, then
/// the relief entry. `None` if the plain entry can't be found.
pub fn wgsl(world_slot: u32, decal_slot: u32, sky_slot: u32, base: usize) -> Option<String> {
    let plain = crate::shader::wgsl(world_slot, decal_slot, sky_slot);
    let cut = plain.rfind(PLAIN_ENTRY_MARK)?;
    #[cfg(feature = "sky")]
    let sky_call = "mp_sky(n)";
    #[cfg(not(feature = "sky"))]
    let sky_call = "vec3<f32>(1.0)";
    let mut s = String::with_capacity(cut + WGSL.len() + ENTRY.len() + 64);
    s.push_str(&plain[..cut]);
    s.push_str(&format!("const MP_RELIEF: u32 = {base}u;\n"));
    s.push_str(&WGSL.replace("//$DEBUG", if cfg!(feature = "relief-debug") { DEBUG } else { "" }));
    s.push('\n');
    s.push_str(&ENTRY.replace("$SKY_CALL", sky_call));
    Some(s)
}
