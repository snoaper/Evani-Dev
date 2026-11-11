//! The geometry stage's interface. One source model per asset goes in
//! ([`SourceMesh`]: the rest pose, unskinned, a region index per vertex);
//! a styled mesh comes out ([`StyledMesh`]: at most 16 parts, one region
//! each). The generators (voxel, low-poly, papercraft, lumps, ridges,
//! proportion) implement [`GeometryGenerator`] in `evani-style-geo`, shared
//! by the viewer at runtime and `cargo xtask style-bake` / `style-matrix` at
//! build time. The output never depends on the material, the light or the
//! quality: it is cached by [`crate::hash::GeometryKey`] alone, and the
//! surface step colours it per region afterwards (a voxel's colour is the
//! material's pattern sampled at [`StyledPart::samples`]).

use crate::MAX_SLOTS;
use crate::assets::AssetRow;
use crate::styles::{Geometry, GeometryKind};

/// The version of each generator's output. A generator bumps its number
/// whenever its output changes, so every cached variant made by the old one
/// is invalidated (it enters [`crate::hash::GeometryKey`]). 0 = no
/// generator yet: [`crate::check::gaps`] lists the styles waiting for one.
/// Only the package that owns a generator edits its line.
pub fn generator_version(kind: GeometryKind) -> u32 {
    match kind {
        GeometryKind::None => 1,
        // WP3 (evani-style-geo): the conservative voxeliser, the low-poly
        // decimator, papercraft, lumps and ridges.
        GeometryKind::Voxel => 1,
        GeometryKind::Lowpoly => 1,
        GeometryKind::Papercraft => 1,
        GeometryKind::Lumpy => 1,
        GeometryKind::Ridges => 1,
        // Later than B0: needs the skeleton (ARCHITECTURE.md section 2).
        GeometryKind::Proportion => 0,
    }
}

/// An asset's one source model, as the geometry stage reads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Per vertex: the index of its region in the asset row's `regions`.
    pub regions: Vec<u8>,
}

impl SourceMesh {
    /// Lengths agree, indices are in range, every region index names one of
    /// the asset's `region_count` regions, every position is finite.
    pub fn validate(&self, region_count: usize) -> Result<(), String> {
        let n = self.positions.len();
        if n == 0 || self.normals.len() != n || self.regions.len() != n {
            return Err(format!("source: {n} positions, {} normals, {} region ids: they must match and not be empty", self.normals.len(), self.regions.len()));
        }
        if self.indices.is_empty() || !self.indices.len().is_multiple_of(3) || self.indices.iter().any(|&i| i as usize >= n) {
            return Err("source: indices must be whole triangles within the vertices".into());
        }
        if self.regions.iter().any(|&r| usize::from(r) >= region_count) {
            return Err(format!("source: a vertex names a region past the asset's {region_count}"));
        }
        if self.positions.iter().flatten().any(|x| !x.is_finite()) {
            return Err("source: a position isn't finite".into());
        }
        Ok(())
    }
}

/// One part of a styled mesh: one region's triangles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyledPart {
    /// Index into the asset row's `regions`.
    pub region: u8,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Per vertex, where the surface step samples the material's pattern (a
    /// voxel's cell centre, so a cube is one colour). Empty: at the vertex.
    pub samples: Vec<[f32; 3]>,
}

/// The geometry stage's output.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyledMesh {
    pub parts: Vec<StyledPart>,
}

impl StyledMesh {
    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// At most [`MAX_SLOTS`] parts, each well formed, regions in range.
    pub fn validate(&self, region_count: usize) -> Result<(), String> {
        if self.parts.is_empty() || self.parts.len() > MAX_SLOTS {
            return Err(format!("styled mesh: {} parts, must be 1 to {MAX_SLOTS}", self.parts.len()));
        }
        for p in &self.parts {
            let n = p.positions.len();
            let ok = n > 0
                && p.normals.len() == n
                && (p.samples.is_empty() || p.samples.len() == n)
                && !p.indices.is_empty()
                && p.indices.len().is_multiple_of(3)
                && p.indices.iter().all(|&i| (i as usize) < n)
                && usize::from(p.region) < region_count;
            if !ok {
                return Err(format!("styled mesh: part of region {} is malformed", p.region));
            }
        }
        Ok(())
    }
}

/// A geometry stage generator.
pub trait GeometryGenerator {
    fn kind(&self) -> GeometryKind;

    /// Build `geometry` (controls already applied) from `src` for `asset`.
    /// Deterministic: the same input gives the same output, byte for byte.
    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String>;
}

/// Geometry stage `none`: the source cut into one part per region.
#[derive(Debug, Clone, Copy, Default)]
pub struct Passthrough;

impl GeometryGenerator for Passthrough {
    fn kind(&self) -> GeometryKind {
        GeometryKind::None
    }

    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String> {
        if *geometry != Geometry::None {
            return Err(format!("passthrough can't build `{}`", geometry.kind().name()));
        }
        src.validate(asset.regions.len())?;
        let mut parts: Vec<StyledPart> = Vec::new();
        let mut remap: Vec<Option<(usize, u32)>> = vec![None; src.positions.len()];
        let (tris, _) = src.indices.as_chunks::<3>();
        for tri in tris {
            // A triangle belongs to the region of its first vertex.
            let region = src.regions[tri[0] as usize];
            let pi = match parts.iter().position(|p| p.region == region) {
                Some(i) => i,
                None => {
                    parts.push(StyledPart { region, ..StyledPart::default() });
                    parts.len() - 1
                }
            };
            for &v in tri {
                let vi = v as usize;
                let local = match remap[vi] {
                    Some((p, l)) if p == pi => l,
                    _ => {
                        let part = &mut parts[pi];
                        let l = part.positions.len() as u32;
                        part.positions.push(src.positions[vi]);
                        part.normals.push(src.normals[vi]);
                        remap[vi] = Some((pi, l));
                        l
                    }
                };
                parts[pi].indices.push(local);
            }
        }
        let out = StyledMesh { parts };
        out.validate(asset.regions.len())?;
        Ok(out)
    }
}
