//! One small shader per material (feature `material-split`, on with
//! `material-recipes`): every model material draws with its own pipeline,
//! [`Key::ModelSplit`]`(pass, msaa, recipe, kind)`: the forward module with
//! only that recipe in the recipe stage, only that character kind's shading,
//! only that pass's see-through code, and then only the code the pipeline's
//! entry points reach (evani-material-recipes `split`). There is no
//! all-in-one model module: one with every recipe took 21-28 s to compile on
//! an RTX 4060 Ti, a material's own 0.6-3.7 s.
//!
//! Pipelines are lazy: a draw asks for its material's key, the background
//! queue builds it (light jobs, several at once, each off the main thread),
//! and until it is in the material draws with the plain stand-in (the
//! forward module cut down to `model_vs` + `model_fs`, about 11 KB), or, for
//! a see-through one, opaque, as before. Live slot edits rebuild the
//! installed keys that use the slot (`Pipelines::split_keys`), each from the
//! variant cache when it was built before.

use std::sync::Mutex;

use evani_material_recipes::split::{GLASS, OPAQUE, shake};

use super::Key;
use crate::splice::Spliced;

#[cfg(not(feature = "material-recipes"))]
compile_error!("`material-split` is turned on by `material-recipes`; don't list it on its own");
#[cfg(feature = "relief")]
compile_error!("catalogue material recipes (`material-split`) don't draw on the relief stage");

/// The skinned pass (the opaque module through the skinned vertex stage).
pub(super) const SKIN: u8 = crate::model::recipes::SKINNED;

type Cached = ((u8, bool, u8, u8), Option<&'static str>);

/// The template of one material's pipeline, made once per key and kept for
/// the session (`None`: the pass is not built, the recipe is not compiled in
/// or an anchor is missing; the material then keeps the stand-in, and the
/// overlay log says once that the stage fell back). The skinned pass shares
/// the opaque text.
pub(super) fn template(see: u8, ms: bool, code: u8, kind: u8) -> Option<&'static str> {
    static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());
    let see = if see == SKIN { OPAQUE } else { see };
    let ms = ms && see != GLASS;
    let mut c = CACHE.lock().ok()?;
    if let Some(e) = c.iter().find(|e| e.0 == (see, ms, code, kind)) {
        return e.1;
    }
    let msaa = if ms { super::MSAA_SAMPLES } else { 1 };
    let t = crate::model::recipes::patched_material(msaa, code, see, kind).map(|t| &*Box::leak(t.into_boxed_str()));
    // Recipe 0 always has a stage; if even it fails an anchor is missing.
    if code == 0 && see == OPAQUE {
        crate::model::recipes::note_stage(t.is_some());
    }
    c.push(((see, ms, code, kind), t));
    t
}

/// The stand-ins' template: the engine's forward module (the patches only
/// touch the lit stage, which a stand-in never reaches).
pub(super) fn lite_template() -> &'static str {
    crate::shaders::FORWARD.source
}

/// The entry points a key's pipeline uses (`None`: not a model key).
fn entries(key: Key) -> Option<&'static [&'static str]> {
    match key {
        Key::ModelSplit(see, ..) => Some(crate::model::recipes::material_entries(see)),
        Key::ModelLite(_) => Some(&["model_vs", super::super::models::LITE_FS]),
        #[cfg(feature = "skinning")]
        Key::SkinLite(_) => Some(&["model_skin_vs", super::super::models::LITE_FS]),
        _ => None,
    }
}

/// The spliced source of a material pipeline or a stand-in, cut down to
/// what its entry points reach (other keys unchanged). Slot spans no longer
/// line up after the cut, so they are dropped (a slot error in a material
/// pipeline is reported without its slot line).
pub(super) fn shaken(key: Key, sp: Spliced) -> Spliced {
    let Some(e) = entries(key) else {
        return sp;
    };
    Spliced { source: shake(&sp.source, e), spans: Vec::new() }
}

/// A material key's label part: `opaque`, `blend`, `glass` or `skin`.
pub(super) fn pass_name(see: u8) -> &'static str {
    match see {
        OPAQUE => "opaque",
        GLASS => "glass",
        SKIN => "skin",
        _ => "blend",
    }
}

/// Whether `key`'s text is in this build (its template exists).
pub(super) fn has_text(key: Key) -> bool {
    match key {
        Key::ModelSplit(see, ms, code, kind) => template(see, ms, code, kind).is_some(),
        _ => false,
    }
}
