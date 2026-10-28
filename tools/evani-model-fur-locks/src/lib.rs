//! `evani-model-fur-locks`: procedural fur locks for Evani.Model, a
//! build-time tool (development only). Ported from ChimpDuty's gap module.
//!
//! A code-built ape reads as a toy while its coat is one smooth skin: the
//! outline never breaks. Locks fix the outline. A lock is a tapered ribbon
//! (2 x `segments` triangles: 6, 8 or 10) that grows from a fur triangle,
//! leaves the skin at a small lift, lies along a flow field, droops under
//! gravity, curls a little sideways and pulls its tip towards the leading
//! lock of its clump, so neighbouring locks gather into shaggy points.
//! Written from scratch for Evani.Model; the shells-and-fins and
//! clumping-and-curling papers (Lengyel and NVIDIA; Silva et al.) and
//! Scheuermann's hair-card talk were read for ideas only.
//!
//! * [`params`]: the registered tunables (`kit.fur_locks.*`) and the
//!   per-region table (length, density, width, flow per colour region, or
//!   per solid and region such as `head.fur`). Always built.
//! * `surface` (feature `locks`): [`FurSurface`], the input. Any triangle
//!   mesh with a colour region and a solid per triangle; built from meshed
//!   solids with [`FurSurface::from_solids`], or by hand with
//!   [`FurSurface::add_mesh`].
//! * `locks` (feature `locks`): [`fur_locks`], the generator. Returns
//!   [`FurLocks`]: one [`evani_model::compile::SolidMesh`] named
//!   [`SOLID`] (`fur_locks`) plus per-vertex ribbon coordinates and strand
//!   directions for a hair shader, the locks themselves and counts.
//! * [`bake`] (feature `locks`): grow the locks over a compiled tier and
//!   write them into its `.glb` as the solid `fur_locks`; the `fur-locks`
//!   command line does it from an `.evmodel`. The locks reach a game only
//!   as triangles in its model file: 0 runtime bytes.
//!
//! Which triangles are what: a triangle whose region has an entry in
//! [`LockParams::regions`] is fur and grows locks; a region named in
//! [`LockParams::bare`] (face skin, eyes, mouth) is neither; every other
//! region is gear. Locks rooted within `kit.fur_locks.gear_band_mm` of gear
//! grow up to `kit.fur_locks.gear_boost` (2x) longer, so fur spills over
//! cuffs, collars and straps; roots with gear right above them (fur under a
//! sleeve) are skipped.
//!
//! The solid never holds more than [`MAX_TRIS`] (14,000) triangles: at most
//! [`MAX_LOCKS`] (1,750) locks of 8 triangles, fewer at 10. Output is
//! byte-identical for the same input on every platform: one seeded
//! `Pcg32`, a fixed visiting order, Evani.Model's own `dmath` for every
//! sine and cosine, and no threads.

pub mod params;

#[cfg(feature = "locks")]
pub mod bake;
#[cfg(feature = "locks")]
mod grid;
#[cfg(feature = "locks")]
mod locks;
#[cfg(feature = "locks")]
mod noise;
#[cfg(feature = "locks")]
pub mod surface;
#[cfg(all(test, feature = "locks"))]
mod tests;

use std::fmt;

#[cfg(feature = "locks")]
pub use locks::{FurLocks, Lock, Stats, fur_locks};
pub use params::{Flow, LockParams, MAX_LOCKS, MAX_TRIS, RegionFur};
#[cfg(feature = "locks")]
pub use surface::FurSurface;

/// The name of the solid the locks are emitted as.
pub const SOLID: &str = "fur_locks";

/// Why locks could not be generated.
#[derive(Clone, Debug, PartialEq)]
pub struct LockError(pub String);

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LockError {}

pub(crate) fn lerr(msg: impl Into<String>) -> LockError {
    LockError(msg.into())
}
