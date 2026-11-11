//! Where a game puts CPU assets: the [`ProcScene`] resource. Each frame
//! [`crate::ProcModule`] draws every placement whose asset is switched on,
//! regenerating an asset's mesh only when its params, seed or style change.

use crate::style::REALISM;

/// One placed asset.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The asset's feature (`terrain-rolling-hills`).
    pub feature: String,
    /// Where its base sits (m).
    pub position: [f32; 3],
    /// Turn about y (radians).
    pub yaw: f32,
    /// Uniform scale over the asset's own size.
    pub scale: f32,
}

/// The placed CPU assets and the art style they are made in.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcScene {
    pub placements: Vec<Placement>,
    /// The style every placement is generated in (`WK01`..`WK24`).
    pub style: String,
}

impl Default for ProcScene {
    fn default() -> Self {
        Self { placements: Vec::new(), style: REALISM.to_string() }
    }
}

impl ProcScene {
    /// Place `feature` with its base at `position`; returns its index.
    pub fn place(&mut self, feature: &str, position: [f32; 3]) -> usize {
        self.placements.push(Placement { feature: feature.to_string(), position, yaw: 0.0, scale: 1.0 });
        self.placements.len() - 1
    }

    pub fn clear(&mut self) {
        self.placements.clear();
    }
}
