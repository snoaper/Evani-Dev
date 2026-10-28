//! The named hooks this look offers the renderer (the S5-R5 contract).
//!
//! Nothing here changes evani-render. A renderer that does not know these
//! names still draws the look: its own model shader takes the look's
//! `toon_ramp`, `rim`, `tonemap` and `fog` bodies as it does for every look,
//! and the slots the look declares itself (`specular`, `surface_params`,
//! `ibl_params`, with feature `pcss` also `pcss_params`) are simply unused.
//! S5-R5 makes them count by adding the engine half: the reference is
//! `r5/look-surface.patch` in this crate's folder (ChimpDuty, written against
//! Evani.Dev f216906, not applied), plus image-based light, which the patch
//! does not have yet.

use evani_core::slots::SlotSignature;

/// Material group (group 1) bindings the lit model shader
/// ([`crate::MODEL_SURFACE_WGSL`]) reads beyond the engine's model.wgsl.
pub mod bindings {
    /// glTF metallicRoughnessTexture (G roughness, B metal), linear.
    pub const METALLIC_ROUGHNESS: u32 = 5;
    /// glTF occlusionTexture (R), linear.
    pub const OCCLUSION: u32 = 6;
    /// The look's biplanar detail map (generator [`super::DETAIL_GENERATOR`]).
    pub const DETAIL: u32 = 7;
    /// `ModelSurface` uniform, [`crate::SURFACE_WORDS`] words
    /// ([`crate::SurfaceDesc::words`]).
    pub const SURFACE: u32 = 8;
    /// The look's fur clump map (generator [`super::CLUMP_GENERATOR`]).
    pub const CLUMP: u32 = 9;
}

/// Model pipeline entry points the hook draws instead of `model_vs` /
/// `model_fs`.
pub const MODEL_VS: &str = "model_lit_vs";
pub const MODEL_FS: &str = "model_lit_fs";
/// Fur shell and fin entry points (feature `fur`; drawing them needs a pass
/// per fur part, filed for Stage 8 as `fur-shells`).
pub const FUR_ENTRY_POINTS: [&str; 4] = ["fur_shell_vs", "fur_shell_fs", "fur_fin_vs", "fur_fin_fs"];
/// The shadow lookup PCSS replaces (feature `pcss`): same bindings and
/// signature as the engine's forward_shadows.wgsl.
pub const SHADOW_FN: &str = "shadow_vis";

/// Generator names the hook asks the look for (white when absent).
pub const DETAIL_GENERATOR: &str = crate::DETAIL_GENERATOR;
pub const CLUMP_GENERATOR: &str = crate::CLUMP_GENERATOR;
/// Generator name reserved for the small environment cubemap the catalog's
/// "image-based light from a small cubemap" asks for. The look does not
/// register it yet: S5-R5 decides whether the cubemap is baked from the sky
/// at load or generated here, and then this name is the one to use.
pub const ENV_GENERATOR: &str = "surface.env";

/// Image-based light constants: (diffuse strength, specular strength,
/// roughness-to-mip scale, unused). The lit shader today lights from a
/// sky/ground hemisphere (`surface_params` indices 2 and 3); once S5-R5 binds
/// a cubemap it scales the cubemap's light by these, and keeps the hemisphere
/// when no cubemap is bound. Zero by default, so a look that never sets it
/// keeps the hemisphere.
pub const IBL_PARAMS: SlotSignature = SlotSignature {
    name: "ibl_params",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Image-based light: diffuse strength, specular strength, roughness-to-mip scale (zero = hemisphere only)",
    default_body: "    return vec4<f32>(0.0);",
};

/// The body this look sets for [`IBL_PARAMS`].
pub fn ibl_params_body() -> String {
    format!("    // {}: image-based light constants\n    return vec4<f32>(1.0, 1.0, 1.0, 0.0);", crate::BODY_TAG)
}

/// Every slot name the look declares or fills that the engine does not
/// already use (S5-R5 must splice these into the lit shader).
pub fn new_slots() -> Vec<&'static str> {
    crate::declared_slots().iter().map(|s| s.name).collect()
}
