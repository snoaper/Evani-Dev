//! Cartoon proportions for Rubber-hose and Storybook (later than the proof
//! batch B0, ARCHITECTURE.md section 2; still required for every asset by the
//! end): head and limb scale driven by the skeleton, so it needs the source's
//! bones (an extension of [`SourceMesh`] decided by the package that builds
//! it).
//!
//! Until then it answers "not built" and the studio shows "generating".

use evani_studio::assets::AssetRow;
use evani_studio::geo::{GeometryGenerator, SourceMesh, StyledMesh};
use evani_studio::styles::{Geometry, GeometryKind};

#[derive(Debug, Clone, Copy, Default)]
pub struct Proportion;

impl GeometryGenerator for Proportion {
    fn kind(&self) -> GeometryKind {
        GeometryKind::Proportion
    }

    fn generate(&self, _src: &SourceMesh, _asset: &AssetRow, _geometry: &Geometry) -> Result<StyledMesh, String> {
        Err(crate::not_built(self.kind()))
    }
}
