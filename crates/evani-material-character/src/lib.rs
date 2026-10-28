//! Evani.Dev character materials: wet eyes, soft skin and fur/hair shine for
//! close-up models, on top of the lit surface pass (evani-render
//! `look-surface`). Turned on only through evani-render's `char-materials`
//! feature; a game without it links none of this.
//!
//! * [`record`]: the glTF material tag (`extras.evani_kind`: `eye`, `skin`,
//!   `hair`) and per-material values, read at model load into the surface
//!   uniform.
//! * [`lut`]: the pre-integrated skin lookup, generated at load and fitted
//!   for the GPU.
//! * [`shader`]: `shaders/character.wgsl` and the patch that wires it into
//!   the lit fragment stage.
//! * [`params`]: the live params (`material.eye.*`, `material.skin.*`,
//!   `material.hair.*`, `material.character.quality`), the quality by tier
//!   and the `char_params` slot body they write.
//! * [`register`]: declares the slot, registers the params and keeps the
//!   slot body in step with them and with the tier.

pub mod lut;
pub mod params;
pub mod record;
pub mod shader;

use evani_core::plugin::{PluginContext, PluginError};

pub use params::register;

/// The params' owner (shown by live tools).
pub const OWNER: &str = "material-character";

/// Register everything (called by evani-render's plugin when the
/// `char-materials` feature is on).
pub fn install(ctx: &mut PluginContext) -> Result<(), PluginError> {
    register(ctx)
}
