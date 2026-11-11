//! Art styles as a first-class input. Every asset is generated for a style,
//! the way the material library pairs a material with a style: the same 24
//! world-kit styles (`WK01`..`WK24`, kits/evani-kit-material-view/data/
//! styles.json). Each asset lists the styles it builds natively
//! ([`crate::ProcAsset::styles`]); any other style falls back to Cinematic
//! Realism ([`REALISM`]), and the registry says so ([`Generated::fallback`]),
//! so a style is added to an asset later without changing any interface.

/// Cinematic Realism, the style every asset is built for first.
pub const REALISM: &str = "WK24";

/// The 24 world-kit styles, in the style bar's order.
pub const STYLES: [&str; 24] = [
    "WK01", "WK02", "WK03", "WK04", "WK05", "WK06", "WK07", "WK08", "WK09", "WK10", "WK11", "WK12", "WK13", "WK14", "WK15", "WK16", "WK17", "WK18", "WK19", "WK20", "WK21", "WK22", "WK23", "WK24",
];

/// Whether `id` is one of the 24 styles.
pub fn is_style(id: &str) -> bool {
    STYLES.contains(&id)
}

/// The style an asset builds `wanted` in: `wanted` when it is native to the
/// asset, else Realism.
pub fn resolve<'a>(native: &[&'a str], wanted: &'a str) -> (&'a str, bool) {
    if native.contains(&wanted) { (wanted, false) } else { (REALISM, wanted != REALISM) }
}

/// A generation with the style it was made in.
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
    pub output: crate::Output,
    /// The style it was made in.
    pub style: &'static str,
    /// The wanted style is not native to the asset: made in Realism.
    pub fallback: bool,
}
