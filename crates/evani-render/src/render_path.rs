//! Which render path a game gets on a tier, and how much of the catalog's
//! description of it (`catalog/tiers.json` `render_paths`) is built.
//!
//! Pure data: a few hundred bytes of tables, no GPU, no allocation until
//! [`RenderPathInfo::line`] formats a report line. The same table lives in
//! `assets/render_paths.json`, which `cargo xtask check` reads to stop a game
//! that targets a path that isn't built; the test `table_matches_assets_json`
//! keeps the two equal.

use evani_core::effects::StyleClasses;
use evani_core::tiers::TierPreset;

/// The family of render path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Instanced atlas sprites (the `2d` style class).
    Sprites,
    /// The low-resolution pseudo-3D pass (the `pseudo-3d` style class).
    Pseudo3d,
    /// The 3D forward path (the `3d` style class). Compiled only when a
    /// crate of the game turns on evani-render's `forward` feature
    /// ([`crate::FORWARD_PATH`]); 2D games leave it off.
    Forward,
}

impl PathKind {
    /// The style class name this path serves: `2d`, `pseudo-3d` or `3d`.
    pub const fn style(self) -> &'static str {
        match self {
            PathKind::Sprites => "2d",
            PathKind::Pseudo3d => "pseudo-3d",
            PathKind::Forward => "3d",
        }
    }
}

/// What a game of one style gets on one tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPathInfo {
    pub kind: PathKind,
    /// The tier id this describes.
    pub tier: &'static str,
    /// MSAA samples of the forward pass (1: off).
    pub msaa: u32,
    /// Shadow-map size in texels, if the tier has a shadow map.
    pub shadow_map: Option<u32>,
    /// Internal render scale in thousandths (1000: native).
    pub post_scale_milli: u32,
    /// The path's base pass exists, so a game of this style can run here.
    pub runs: bool,
    /// Nothing of the catalog's description is missing.
    pub built: bool,
    /// Ids of the parts not built yet (see `assets/render_paths.json` `deferred`).
    pub missing: &'static [&'static str],
}

impl RenderPathInfo {
    /// One report line, e.g. `3d/high: Forward, msaa 4, shadow 2048, scale 1.00, runs; missing point-lights, ...`.
    pub fn line(&self) -> String {
        let shadow = self.shadow_map.map_or("none".to_string(), |s| s.to_string());
        let state = if !self.runs {
            "NOT BUILT"
        } else if self.built {
            "built"
        } else {
            "runs"
        };
        let mut s = format!(
            "{}/{}: {:?}, msaa {}, shadow {}, scale {}.{:02}, {}",
            self.kind.style(),
            self.tier,
            self.kind,
            self.msaa,
            shadow,
            self.post_scale_milli / 1000,
            self.post_scale_milli % 1000 / 10,
            state
        );
        if !self.missing.is_empty() {
            s.push_str("; missing ");
            s.push_str(&self.missing.join(", "));
        }
        s
    }
}

const NONE: &[&str] = &[];

/// (runs, missing) per tier in the order lite, low, medium, high, ultra, vr.
type Row = [(bool, &'static [&'static str]); 6];

const SPRITES: Row = [
    (true, NONE),
    (true, NONE),
    (true, NONE),
    (true, &["2d-normal-lighting"]),
    (true, &["2d-normal-lighting"]),
    (false, &["vr-stereo"]),
];
const PSEUDO_3D: Row = [
    (false, &["pseudo-3d-pass"]),
    (false, &["pseudo-3d-pass"]),
    (false, &["pseudo-3d-pass"]),
    (false, &["pseudo-3d-pass"]),
    (false, &["pseudo-3d-pass"]),
    (false, &["pseudo-3d-pass", "vr-stereo"]),
];
const FORWARD: Row = [
    (true, NONE),
    (true, NONE),
    (true, &["cpu-frustum-culling", "point-lights"]),
    (true, &["cpu-frustum-culling", "point-lights", "shadow-cascades-2", "clustered-forward", "image-based-lighting"]),
    (
        true,
        &[
            "gpu-driven-culling",
            "point-lights",
            "shadow-cascades-4",
            "clustered-forward",
            "image-based-lighting",
            "volumetric-passes",
            "temporal-upscale",
        ],
    ),
    (false, &["vr-stereo"]),
];

const TIERS: [&str; 6] = ["lite", "low", "medium", "high", "ultra", "vr"];

/// The table row for a style: 3D wins over pseudo-3D wins over 2D when a
/// look claims several classes.
fn row(styles: StyleClasses) -> (PathKind, &'static Row) {
    if styles.contains(StyleClasses::THREE_D) {
        (PathKind::Forward, &FORWARD)
    } else if styles.contains(StyleClasses::PSEUDO_3D) {
        (PathKind::Pseudo3d, &PSEUDO_3D)
    } else {
        (PathKind::Sprites, &SPRITES)
    }
}

/// The render path a game of `styles` gets on `tier`. A custom tier id that
/// is not one of the six reads as the VR row if it is stereo, else Low.
pub fn render_path(tier: &TierPreset, styles: StyleClasses) -> RenderPathInfo {
    let (kind, rows) = row(styles);
    let i = TIERS.iter().position(|t| *t == tier.id).unwrap_or(if tier.stereo { 5 } else { 1 });
    let (runs, missing) = rows[i];
    RenderPathInfo {
        kind,
        tier: TIERS[i],
        msaa: tier.msaa,
        shadow_map: tier.shadow_map,
        post_scale_milli: (tier.render_scale_or_native() * 1000.0).round() as u32,
        runs,
        built: runs && missing.is_empty(),
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::tiers::BUILTIN_TIER_JSON;
    use serde_json::Value;

    fn presets() -> Vec<TierPreset> {
        BUILTIN_TIER_JSON.iter().map(|(_, j)| TierPreset::from_json(j).unwrap()).collect()
    }

    fn json(path: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn render_path_matches_catalog_for_every_tier_and_style() {
        let cat = json("catalog/tiers.json");
        let order: Vec<&str> = cat["render_path_order"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(order, TIERS, "tier order differs from the catalog's render_path_order");
        for (style, kind, class) in [
            ("2d", PathKind::Sprites, StyleClasses::TWO_D),
            ("pseudo-3d", PathKind::Pseudo3d, StyleClasses::PSEUDO_3D),
            ("3d", PathKind::Forward, StyleClasses::THREE_D),
        ] {
            let desc = cat["render_paths"][style].as_array().unwrap();
            assert_eq!(desc.len(), TIERS.len(), "{style}: the catalog describes one path per tier");
            for p in presets() {
                let info = render_path(&p, class);
                assert_eq!(info.kind, kind, "{style}/{}", p.id);
                assert_eq!(info.tier, p.id, "{style}");
                assert_eq!(info.msaa, p.msaa);
                assert_eq!(info.shadow_map, p.shadow_map);
                assert_eq!(info.built, info.runs && info.missing.is_empty());
                assert!(info.runs || !info.missing.is_empty(), "{style}/{}: not built but nothing named", p.id);
                assert!(info.line().starts_with(&format!("{style}/{}", p.id)), "{}", info.line());
            }
        }
        // The 2D and 3D paths the kits use today run on every non-VR tier; pseudo-3D runs nowhere.
        for p in presets().iter().filter(|p| !p.stereo) {
            assert!(render_path(p, StyleClasses::TWO_D).runs && render_path(p, StyleClasses::THREE_D).runs, "{}", p.id);
            assert!(!render_path(p, StyleClasses::PSEUDO_3D).runs, "{}", p.id);
        }
    }

    #[test]
    fn table_matches_assets_json() {
        let j = json("assets/render_paths.json");
        let deferred = j["deferred"].as_object().unwrap();
        for (style, rows) in [("2d", &SPRITES), ("pseudo-3d", &PSEUDO_3D), ("3d", &FORWARD)] {
            for (i, tier) in TIERS.iter().enumerate() {
                let e = &j["paths"][style][*tier];
                assert_eq!(e["runs"].as_bool().unwrap(), rows[i].0, "{style}/{tier} runs");
                let missing: Vec<&str> = e["missing"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
                assert_eq!(missing, rows[i].1, "{style}/{tier} missing");
                for m in missing {
                    assert!(deferred.contains_key(m), "{m} has no entry under `deferred`");
                }
            }
        }
    }

    #[test]
    fn a_look_with_several_classes_takes_the_richest() {
        let low = &presets()[0];
        assert_eq!(render_path(low, StyleClasses::ALL).kind, PathKind::Forward);
        assert_eq!(render_path(low, StyleClasses::PSEUDO_3D.union(StyleClasses::TWO_D)).kind, PathKind::Pseudo3d);
    }
}
