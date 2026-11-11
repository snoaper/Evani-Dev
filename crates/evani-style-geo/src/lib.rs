//! `evani-style-geo`: the studio's geometry stage (docs/studio/CONTRACTS.md
//! "Geometry"). Every style's shape comes from the asset's one source model
//! through a [`GeometryGenerator`]; [`generator`] is the table the viewer,
//! `cargo xtask style-bake` and `cargo xtask style-matrix` all use.
//!
//! File ownership (docs/studio/WORK_PACKAGES.md):
//! - `source.rs`: WP2 (the asset loader): a model file to a [`SourceMesh`];
//!   `code.rs`: WP2 (for WP8): the code-built sources (wall, roof, ground
//!   patch, sphere, cube).
//! - `voxel.rs`, `lowpoly.rs`, `deform.rs`: WP3 (the generators), with
//!   their helpers `math.rs`, `rest.rs` (a tier at its bind pose),
//!   `proxy.rs` (solid mane and tail from the tier's anchors), `standin.rs`
//!   (the horse's source until WP2's reader lands), `view.rs` (the
//!   rasteriser), `shapecheck.rs` (the recognisability check) and
//!   `cache.rs` (the runtime job queue and memory LRU the viewer runs).
//! - `proportion.rs`: later than B0 (needs the skeleton).
//! - this file: the table only. A generator's `generator_version` in
//!   evani-studio turns non-zero in the same change that makes it work; the
//!   test below holds every working generator to the contract.

pub mod cache;
pub mod code;
pub mod deform;
pub mod lowpoly;
mod math;
pub mod proportion;
pub mod proxy;
pub mod regioncolour;
pub mod rest;
pub mod shapecheck;
pub mod source;
pub mod standin;
pub mod view;
pub mod voxel;

pub use evani_studio::geo::{GeometryGenerator, Passthrough, SourceMesh, StyledMesh, StyledPart, generator_version};
pub use evani_studio::assets::AssetRow;
pub use evani_studio::hash::GeometryKey;
pub use evani_studio::styles::{Geometry, GeometryKind};

/// The generator for each geometry kind.
pub fn generator(kind: GeometryKind) -> &'static dyn GeometryGenerator {
    match kind {
        GeometryKind::None => &Passthrough,
        GeometryKind::Voxel => &voxel::Voxel,
        GeometryKind::Lowpoly => &lowpoly::LowPoly,
        GeometryKind::Papercraft => &lowpoly::Papercraft,
        GeometryKind::Lumpy => &deform::Lumpy,
        GeometryKind::Ridges => &deform::Ridges,
        GeometryKind::Proportion => &proportion::Proportion,
    }
}

/// The message a generator not built yet answers with: the cell shows
/// "generating", never another object.
pub fn not_built(kind: GeometryKind) -> String {
    format!("the `{}` geometry generator isn't built yet (docs/studio/WORK_PACKAGES.md)", kind.name())
}

#[cfg(test)]
mod shape_tests;

#[cfg(test)]
mod tests {
    use evani_studio::Catalogs;

    use super::*;

    /// A small closed test source: a box cut into the horse's coat (top)
    /// and leg (bottom) regions.
    fn source() -> SourceMesh {
        let p = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]];
        let n: Vec<[f32; 3]> = p.iter().map(|v: &[f32; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / l, v[1] / l, v[2] / l]
        }).collect();
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        let regions = p.iter().map(|v| if v[1] > 0.0 { 0 } else { 1 }).collect();
        SourceMesh { positions: p.to_vec(), normals: n, indices, regions }
    }

    /// The table names a generator of the right kind for every kind, and
    /// every generator that claims to work (version > 0) builds a valid
    /// mesh from a test source in every style that uses it,
    /// deterministically; the others say they aren't built.
    #[test]
    fn every_working_generator_keeps_the_contract() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let c = Catalogs::load(root).unwrap();
        let horse = c.assets.asset("animal.horse").unwrap();
        let src = source();
        for kind in GeometryKind::ALL {
            assert_eq!(generator(kind).kind(), kind);
        }
        for s in &c.styles.styles {
            let g = generator(s.geometry.kind());
            let out = g.generate(&src, horse, &s.geometry);
            if generator_version(s.geometry.kind()) == 0 {
                assert_eq!(out.unwrap_err(), not_built(s.geometry.kind()), "{}", s.id);
                continue;
            }
            let a = out.unwrap_or_else(|e| panic!("{}: {e}", s.id));
            a.validate(horse.regions.len()).unwrap();
            assert_eq!(g.generate(&src, horse, &s.geometry).unwrap(), a, "{}: deterministic", s.id);
        }
    }
}
