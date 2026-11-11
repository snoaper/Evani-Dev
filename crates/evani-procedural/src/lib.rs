//! evani-procedural: the shared base of the Evani.Dev procedural library
//! (docs/PROCEDURAL.md).
//!
//! * [`ProcAsset`]: one asset: its [`AssetInfo`] (category, id, name, CPU
//!   or GPU [`Path`]), its params ([`ParamSpec`]), the art styles it builds
//!   natively, and `generate(values, seed, style)`.
//! * [`Registry`]: the categories and assets compiled into this build;
//!   evani-procedural-lib fills it, one Cargo feature per asset and one per
//!   category, so nothing a game does not list is in its download.
//! * Live params: every asset gets `procedural.<category>.<asset>.on`,
//!   `.seed` (CPU assets) and one param per spec, registered by
//!   [`ProcModule`] in the engine's param registry, so the Look Lab, the
//!   library viewer and AI agents (`param_set`) drive the same values.
//! * [`ProcScene`]: where a game places CPU assets; [`ProcModule`] draws
//!   them (meshes regenerated only when their params, seed or style change)
//!   and hosts the engine modules wrapped assets stand for.
//! * [`geo`], [`noise`], [`shapes`]: shared mesh, noise and shape helpers
//!   (rocks, tubes, blades, crowns, trunks, scatter, puffs, streaks);
//!   [`FnAsset`]: an asset made by one function; [`placeholder`]:
//!   the name-based stand-in every not-yet-built asset gets.
//! * [`build`]: the built world's shared pieces (extrude, sweep, revolve,
//!   bars, rods, roofs, windows, stairs, gears, points along a path).
//! * [`style`]: the 24 art styles; Realism first, others fall back to it.
//! * Realism: [`surface`] (a catalogue material on a part: its 32-byte
//!   block, uv layout and the in-memory model the module draws it with,
//!   feature `surface`), [`detail`] (surface relief, weathering and masks on
//!   meshes) and [`noise`]'s gradient, warped and eroded fields.
//! * [`compose`]: an asset assembled from its typed building-block links
//!   (layer, scatter, base; catalog/procedural/blocks.json): layers as
//!   condition layers on its own parts, scatters as placed copies.

pub mod asset;
// Composition from typed building-block links (docs/PROCEDURAL.md).
pub mod compose;
// What an asset you can't photograph does to the world (Evani.commons'
// test scenes only).
#[cfg(feature = "behaviour")]
pub mod behaviour;
pub mod build;
pub mod craft;
pub mod geo;
pub mod land;
pub mod module;
pub mod noise;
pub mod placeholder;
pub mod registry;
pub mod scene;
pub mod shapes;
pub mod spec;
pub mod style;
pub mod surface;
pub mod detail;
pub mod forms;
#[cfg(feature = "surface")]
pub mod studio;

#[cfg(test)]
mod tests;

pub use asset::{AssetInfo, FnAsset, Output, Part, Path, ProcAsset};
pub use module::{Host, ProcModule};
pub use placeholder::{Hint, Placeholder};
pub use registry::{Category, Registry};
pub use scene::{Placement, ProcScene};
pub use spec::{ParamSpec, SpecKind, Value, Values};
pub use style::{Generated, REALISM, STYLES};
pub use surface::{Surface, Uv};
