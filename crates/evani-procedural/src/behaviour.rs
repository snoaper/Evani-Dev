//! Behaviour fields (feature `behaviour`, off by default): what a library
//! asset you can't photograph does to the world, so a standard test scene
//! can show it and measure it (catalog/procedural/ref_classes.json, class
//! `behaviour`). Only Evani.commons' render app turns the feature on
//! (docs/EVANI_COMMONS.md "Test scenes and measures"); no game's download
//! carries it.
//!
//! An asset registers plain functions of its own live values, its seed, a
//! point and the sim time. The test scene's proxies (grass, trees, a flag,
//! floating leaves) have fixed physics of their own and move only by the
//! field; the scene's probes measure the proxies' positions, never a value
//! the asset reports about itself.

use crate::spec::Values;

/// Wind velocity (m/s, world axes, y up) at point `p` and sim time `t` (s).
pub type WindFn = fn(values: &Values, seed: u64, p: [f32; 3], t: f32) -> [f32; 3];

/// Water surface velocity (m/s, `[x, z]`) at point `p` and sim time `t` (s)
/// in the channel `c`.
pub type FlowFn = fn(values: &Values, seed: u64, c: &Channel, p: [f32; 3], t: f32) -> [f32; 2];

/// The current test scene's channel: it runs along +x (downstream), its
/// centre line at z = 0, banks at z = +-`half_width`, with round rocks.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub half_width: f32,
    /// Rocks: `[x, z]` centre and radius (m).
    pub rocks: Vec<([f32; 2], f32)>,
}

/// The fields this build's assets registered, by asset feature.
#[derive(Debug, Default, Clone)]
pub struct Behaviours {
    wind: Vec<(&'static str, WindFn)>,
    flow: Vec<(&'static str, FlowFn)>,
}

impl Behaviours {
    /// The asset `feature`'s wind (a second one for the same asset replaces it).
    pub fn add_wind(&mut self, feature: &'static str, f: WindFn) {
        self.wind.retain(|(k, _)| *k != feature);
        self.wind.push((feature, f));
    }

    /// The asset `feature`'s water surface flow.
    pub fn add_flow(&mut self, feature: &'static str, f: FlowFn) {
        self.flow.retain(|(k, _)| *k != feature);
        self.flow.push((feature, f));
    }

    pub fn wind(&self, feature: &str) -> Option<WindFn> {
        self.wind.iter().find(|(k, _)| *k == feature).map(|(_, f)| *f)
    }

    pub fn flow(&self, feature: &str) -> Option<FlowFn> {
        self.flow.iter().find(|(k, _)| *k == feature).map(|(_, f)| *f)
    }

    /// Every asset with a wind, in the order they were added.
    pub fn wind_features(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.wind.iter().map(|(k, _)| *k)
    }

    /// Every asset with a flow, in the order they were added.
    pub fn flow_features(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.flow.iter().map(|(k, _)| *k)
    }
}
