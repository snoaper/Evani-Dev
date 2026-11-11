//! `evani-studio`: the contracts of the Asset & Style Studio (the material
//! viewer's redesign, docs/studio/SPEC.md). Everything the parallel work
//! packages code against lives here, as data types and pure functions, so
//! the kit, the app, `cargo xtask check` and the build-time tools agree:
//!
//! - [`state::Selection`]: the record. Asset (WHAT), style (HOW) and
//!   material (OF WHAT) in one value. The drawn result is a pure function of
//!   it ([`resolve::resolve`]), so the order of the clicks that made it can't
//!   matter. Lighting and quality ([`state::View`]) sit outside it and never
//!   enter a cache key, so they never rebuild the object.
//! - [`assets::AssetCatalog`]: `catalog/assets/assets.json`, the asset
//!   catalogue. Test shapes are ordinary rows; nothing in this crate knows a
//!   "default sphere".
//! - [`materials::MaterialIndex`]: the material catalogue as the studio reads
//!   it, with the `belongs_on` field (which asset class a material is made
//!   for).
//! - [`styles::StyleCatalog`]: each art style split into a geometry stage,
//!   a surface stage and a screen pass (the `studio` block in the viewer's
//!   styles.json, plus its `render` block for the surface).
//! - [`compat`]: the compatibility rules (held as data in the catalogues).
//! - [`state::StudioState::apply`]: the auto-match rule. A deliberate asset
//!   choice is never replaced; a material made for another class is offered
//!   as "Show on a horse" instead.
//! - [`hash`]: cache keys, FNV-1a 64 over a canonical byte stream: asset (and
//!   its source), style, params and the engine version.
//! - [`geo`]: the geometry generator interface (the voxel, low-poly and
//!   deform stages implement it in `evani-style-geo`).
//! - [`check`]: catalogue cross-checks and the asset x style coverage check
//!   the engine check runs.
//!
//! No GPU, no browser, no engine crate. docs/studio/CONTRACTS.md explains
//! each contract in plain words; docs/studio/WORK_PACKAGES.md says who owns
//! what.

pub mod assets;
pub mod check;
pub mod compat;
pub mod geo;
pub mod hash;
pub mod materials;
pub mod resolve;
pub mod state;
pub mod styles;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_parts;

pub use assets::{AssetCatalog, AssetRow, AssetStatus};
pub use compat::Compat;
pub use hash::{GeometryKey, PassKey, VariantKey};
pub use materials::{MaterialIndex, MaterialInfo};
pub use resolve::{Availability, Resolved, resolve};
pub use state::{Notice, Pick, Selection, StudioState, View};
pub use styles::{Geometry, GeometryKind, Pass, StyleCatalog, StyleDef, Surface};

/// The engine version every cache key carries: a new engine invalidates
/// every cached variant.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The asset catalogue's path from the engine root.
pub const ASSETS_PATH: &str = "catalog/assets/assets.json";
/// The material catalogue's path from the engine root.
pub const MATERIALS_PATH: &str = "catalog/materials/materials.json";
/// The art styles' path from the engine root (the viewer kit's data).
pub const STYLES_PATH: &str = "kits/evani-kit-material-view/data/styles.json";

/// The shipped asset catalogue (feature `data`: the viewer's page).
#[cfg(feature = "data")]
pub const ASSETS_JSON: &str = include_str!("../../../catalog/assets/assets.json");

/// The renderer's material slots per draw set (evani-render `MAX_MATERIALS`):
/// an asset has at most this many regions and a styled mesh at most this many
/// parts, so every region can carry its own material.
pub const MAX_SLOTS: usize = 16;

/// All three catalogues, parsed, plus the source models' content hashes
/// (from the app's stream manifest; an asset without one keys as 0).
#[derive(Debug, Clone)]
pub struct Catalogs {
    pub assets: AssetCatalog,
    pub styles: StyleCatalog,
    pub materials: MaterialIndex,
    /// Asset id -> the content hash of its source model (stream.json), so a
    /// rebuilt source invalidates its cached style variants.
    pub source_hashes: std::collections::BTreeMap<String, u64>,
}

impl Catalogs {
    /// Parse the three files' text and cross-check them ([`check::problems`]
    /// must be empty).
    pub fn parse(assets_json: &str, styles_json: &str, materials_json: &str) -> Result<Self, String> {
        let c = Self {
            assets: AssetCatalog::parse(assets_json)?,
            styles: StyleCatalog::parse(styles_json)?,
            materials: MaterialIndex::parse(materials_json)?,
            source_hashes: std::collections::BTreeMap::new(),
        };
        let problems = check::problems(&c);
        match problems.first() {
            Some(p) => Err(format!("{} ({} problem(s) in all; first: fix: {})", p.what, problems.len(), p.fix)),
            None => Ok(c),
        }
    }

    /// The catalogues of the engine checkout at `root`.
    pub fn load(root: &std::path::Path) -> Result<Self, String> {
        let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
        Self::parse(&read(ASSETS_PATH)?, &read(STYLES_PATH)?, &read(MATERIALS_PATH)?)
    }
}
