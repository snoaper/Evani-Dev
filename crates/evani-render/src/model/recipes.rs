//! Catalogue material recipes on textured models (feature `material-recipes`
//! without `relief`; the crate `evani-material-recipes` holds the 32-byte
//! block, the recipe stage WGSL, its patch and the live switches). A
//! material's `extras.evani_recipe` (the block as 64 hex characters, what
//! `cargo xtask convert --materials` writes) is read into the eight words of
//! its binding 12. A material without one, or with a bad one, reads as no
//! block (all zero), and the recipe stage then returns the lit pass's own
//! values: the model draws exactly as it would without the feature.
//!
//! [`patched_material`] is the text one material's pipelines compile (the GPU
//! side keeps one per recipe, character kind, pass and sample count);
//! [`BlockEdits`] is how a kit rewrites one material's block live (the
//! viewer's sliders) without rebuilding anything.

use core::sync::atomic::{AtomicU8, Ordering};

use evani_gltf::Material;
use evani_material_recipes::block::Block;

use super::ModelId;

/// The words of a material's block (binding 12, 2 x `vec4<u32>`).
pub const BLOCK_WORDS: usize = 8;
/// No block: recipe 0, the lit pass unchanged.
pub const NO_BLOCK: [u32; BLOCK_WORDS] = [0; BLOCK_WORDS];
/// The glTF material extras key of the block (64 hex characters).
pub const RECIPE_KEY: &str = "evani_recipe";

/// The block words of material `m` (`None`, no block or a malformed one: no
/// block).
pub fn material(m: Option<&Material>) -> [u32; BLOCK_WORDS] {
    m.and_then(|m| m.extras.get(RECIPE_KEY))
        .and_then(serde_json::Value::as_str)
        .and_then(|s| Block::from_hex(s).ok())
        .map_or(NO_BLOCK, |b| b.to_words())
}

/// The stage the model pipelines draw: `model_layered_fs` with
/// `material-layers`, else `model_lit_fs` (as a `fn` head, the patch's anchor).
#[cfg(not(feature = "material-layers"))]
const ENTRY: &str = "fn model_lit_fs(";
/// With `material-layers` on, the layered stage.
#[cfg(feature = "material-layers")]
const ENTRY: &str = "fn model_layered_fs(";

/// [`ENTRY`]'s name: the fragment stage every material pipeline draws.
const DRAWN_FS: &str = if cfg!(feature = "material-layers") { "model_layered_fs" } else { "model_lit_fs" };

/// The skinned pass of a material pipeline (gpu/split.rs `Key::ModelSplit`):
/// the opaque module through the skinned vertex stage.
pub const SKINNED: u8 = 3;

/// The entry points one material pipeline compiles, for pass `see`
/// (`OPAQUE`, `BLEND`, `GLASS` or [`SKINNED`]): exactly one vertex stage
/// (`model_skin_vs` for the skinned pass, `model_vs` for every other) and
/// the drawn fragment stage. The GPU side cuts each module to these
/// (gpu/split.rs) and evani-render's recipe shader test sizes each module by
/// them, so every size the test reports is a module the engine builds (it
/// once cut each module to both vertex stages with `skinning` on, 1.1 to
/// 1.5 KB the engine never compiles, and failed the budget on that).
pub fn material_entries(see: u8) -> &'static [&'static str; 2] {
    const STATIC: [&str; 2] = ["model_vs", DRAWN_FS];
    const SKIN: [&str; 2] = ["model_skin_vs", DRAWN_FS];
    if see == SKINNED { &SKIN } else { &STATIC }
}

/// The character patch the build applies first.
#[cfg(all(feature = "char-materials", not(feature = "material-layers")))]
const CHAR_PATCH: fn(&str, u32) -> Option<String> = evani_material_character::shader::patch_lit;
/// Both families on: the character patch of the layered stage.
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const CHAR_PATCH: fn(&str, u32) -> Option<String> = evani_material_character::shader::patch_layered;

/// One material's module (before the slot bodies are spliced in and the
/// unreached code is cut): the engine's forward module through the
/// character patch first when `char-materials` is on (keeping only `kind`'s
/// character shading), then through the recipe patch with recipe `code`
/// alone in the stage, for pass `see` (evani-material-recipes `split`:
/// `OPAQUE`, `BLEND` or `GLASS`, which is always one sample). `None` when an
/// anchor is missing, the recipe is not compiled in, or the build does not
/// draw that pass. There is no all-in-one module with every recipe.
pub fn patched_material(msaa: u32, code: u8, see: u8, kind: u8) -> Option<String> {
    let msaa = if see == evani_material_recipes::split::GLASS { 1 } else { msaa };
    let src = crate::shaders::FORWARD.source;
    #[cfg(feature = "char-materials")]
    let charred = CHAR_PATCH(src, msaa);
    #[cfg(feature = "char-materials")]
    let src = charred.as_deref().unwrap_or(src);
    let t = evani_material_recipes::split::patch_one(src, ENTRY, msaa, code, see)?;
    Some(evani_material_recipes::split::char_only(&t, kind))
}

/// The extras key of a see-through material's sort priority (batch 3
/// B3-P3): an integer, lower drawn first (default 0), for the odd case the
/// far-to-near order gets wrong.
pub const SORT_KEY: &str = "evani_sort";

/// A blended material's words (batch 3 B3-P3, `material-recipes-blend`): a
/// material whose glTF alphaMode is BLEND gets its mode in the free word 15
/// (its block's: over, add or refract; a block that names none, or no block,
/// draws over) and its sort priority in word 19. Any other material keeps 0
/// in both and draws opaque, as before.
#[cfg(feature = "material-recipes-blend")]
pub fn blend_words(m: Option<&Material>, block: &[u32; BLOCK_WORDS], w: &mut [f32; super::MATERIAL_WORDS]) {
    use evani_material_recipes::blend::{OVER, mode_of_words};
    let Some(m) = m.filter(|m| m.alpha_mode == evani_gltf::AlphaMode::Blend) else {
        return;
    };
    w[15] = f32::from(mode_of_words(block).max(OVER));
    w[19] = m.extras.get(SORT_KEY).and_then(serde_json::Value::as_f64).unwrap_or(0.0).clamp(-1000.0, 1000.0) as f32;
}

/// 0 not built yet, 1 the recipe stage is in, 2 it fell back.
static STAGE: AtomicU8 = AtomicU8::new(0);

/// Whether the model pipelines carry the recipe stage: `None` before the
/// first model pipeline is built, `Some(false)` if an anchor was missing and
/// they fell back to the plain lit stage.
pub fn stage_state() -> Option<bool> {
    match STAGE.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Record how the stage was built (the GPU side, once).
pub fn note_stage(patched: bool) {
    let _ = STAGE.compare_exchange(
        0,
        if patched { 1 } else { 2 },
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
}

/// Live block rewrites, a world resource: each queued block replaces one
/// material's binding 12 at the next frame (a model that isn't on the GPU
/// yet ignores it). The pipelines are never rebuilt.
#[derive(Debug, Default)]
pub struct BlockEdits {
    pending: Vec<(ModelId, usize, [u32; BLOCK_WORDS])>,
}

impl BlockEdits {
    /// Queue `block` for material `material` of model `model`.
    pub fn set_block(&mut self, model: ModelId, material: usize, block: &Block) {
        self.pending.push((model, material, block.to_words()));
    }

    /// Take the queued edits, oldest first.
    pub fn take(&mut self) -> Vec<(ModelId, usize, [u32; BLOCK_WORDS])> {
        core::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mat(extras: serde_json::Value) -> Material {
        Material {
            name: String::new(),
            base_color_factor: [1.0; 4],
            base_color_texture: None,
            metallic_factor: 0.0,
            roughness_factor: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            emissive_factor: [0.0; 3],
            alpha_mode: evani_gltf::AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            extras,
        }
    }

    #[test]
    fn plain_materials_carry_no_block() {
        assert_eq!(material(None), NO_BLOCK);
        assert_eq!(material(Some(&mat(json!({})))), NO_BLOCK);
        assert_eq!(
            material(Some(&mat(json!({"evani_recipe": "zz"})))),
            NO_BLOCK,
            "malformed hex"
        );
        assert_eq!(
            material(Some(&mat(json!({"evani_recipe": 7})))),
            NO_BLOCK,
            "not a string"
        );
    }

    #[test]
    fn the_hex_block_reaches_the_words() {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8 * 7 + 1;
        }
        let b = Block(bytes);
        let w = material(Some(&mat(
            json!({"evani_recipe": b.to_hex(), "evani_material": "wood.walnut.stock"}),
        )));
        assert_eq!(w, b.to_words());
        assert_eq!(
            w[0] & 0xff,
            1,
            "byte 0, the recipe code, is the low byte of word 0"
        );
    }

    #[test]
    fn edits_queue_in_order() {
        let mut e = BlockEdits::default();
        e.set_block(ModelId(3), 1, &Block([1; 32]));
        e.set_block(ModelId(3), 0, &Block([2; 32]));
        let t = e.take();
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].0, t[0].1), (ModelId(3), 1));
        assert!(e.take().is_empty());
    }
}
