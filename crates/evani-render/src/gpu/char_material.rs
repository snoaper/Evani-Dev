//! The model pipelines' WGSL with the character materials wired in
//! (feature `char-materials`): evani-material-character's `patch_lit` of the
//! forward module, made once per sample count and kept for the session, so
//! start-up, slot rebuilds (the `char_params` slot) and fallbacks all
//! compile the same text. Without the lit pass (the `relief` stage wins),
//! nothing is patched and the models draw as before.
//!
//! With `material-layers` on too, the model pipelines draw the layered stage
//! (`model_layered_fs`, gpu/models.rs `LIT_FS`), so that stage is the one
//! patched (`patch_layered`): character kinds take the character shading
//! with their layer record ignored, every other material keeps its layers.
//! DECISIONS.md "Join 6".

use std::sync::OnceLock;

use super::{Key, MSAA_SAMPLES};

/// The patch for the stage the model pipelines draw.
#[cfg(not(all(feature = "material-layers", not(feature = "relief"))))]
const PATCH: fn(&str, u32) -> Option<String> = evani_material_character::shader::patch_lit;
/// Both families on: the layered stage.
#[cfg(all(feature = "material-layers", not(feature = "relief")))]
const PATCH: fn(&str, u32) -> Option<String> = evani_material_character::shader::patch_layered;

/// The patched forward module, without and with MSAA.
static PATCHED: [OnceLock<Option<String>>; 2] = [OnceLock::new(), OnceLock::new()];

/// The template for `key`: the patched module for the model pipelines,
/// `None` (the engine's own) for everything else.
pub(super) fn template(key: Key) -> Option<&'static str> {
    let ms = match key {
        Key::Model(ms) => ms,
        #[cfg(feature = "skinning")]
        Key::Skin(ms) => ms,
        _ => return None,
    };
    PATCHED[usize::from(ms)]
        .get_or_init(|| PATCH(crate::shaders::FORWARD.source, if ms { MSAA_SAMPLES } else { 1 }))
        .as_deref()
}
