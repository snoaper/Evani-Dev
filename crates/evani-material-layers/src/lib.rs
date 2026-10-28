//! Layered materials a model file delivers (Evani.Dev, evani-render feature
//! `material-layers`): the glTF `KHR_materials_sheen`, `KHR_materials_clearcoat`,
//! `KHR_materials_anisotropy` and `KHR_materials_iridescence` extensions,
//! read at `cargo xtask convert` into `extras.evani_layers` (glTF extras
//! survive the converter's clean; extensions it doesn't know don't), then at
//! model load into one 16-float record per material ([`Layers::words`])
//! with a flag bit per layer that the material really uses.
//!
//! * Cloth: Charlie sheen distribution with the Neubelt visibility term,
//!   velvet, an optional subsurface colour (`extras.evani_cloth`), and a
//!   64x64 woven/knit detail map made in code at load ([`weave`]), never
//!   shipped as a file.
//! * Clear coat: a second, smooth specular lobe on the geometric normal
//!   (car paint, lacquer, varnish) that dims the layers under it.
//! * Anisotropy: brushed metal and satin; stretched GGX on Medium and up,
//!   a Kajiya-Kay band on Lite and Low.
//! * Iridescence: thin-film interference (soap, beetle shells, oil),
//!   Medium and up only.
//!
//! Layer order is three.js's: base, then sheen, then clear coat. Every
//! strength is a live parameter (`material.layers.*`, [`params`]) and the
//! tier picks the cheap or the full path ([`params::Quality`]).
//!
//! Formulas ported, never copied as code, from: Filament (Google,
//! Apache-2.0: Charlie D, Neubelt V, clear coat with Kelemen V, anisotropic
//! GGX, cloth subsurface wrap); three.js MeshPhysicalMaterial (MIT:
//! `evalIridescence` / `evalSensitivity` after Belcour and Barla 2017,
//! `IBLSheenBRDF`, the sheen energy term `1 - 0.157 * max3(sheen)`);
//! glTF-Sample-Viewer (Khronos, Apache-2.0: the glTF anisotropy roughness
//! split and the bent reflection normal); Bevy bevy_pbr (MIT/Apache-2.0:
//! one function per layer, compiled in only when used). See
//! THIRD_PARTY_NOTICES.md.

pub mod params;
pub mod parse;
pub mod weave;

pub use params::{Quality, register};
pub use parse::{Layers, Weave};

/// Floats in a material's layer record (4 vec4, the shader's `LayerMat`).
pub const LAYER_WORDS: usize = 16;
/// Floats in the shared per-frame layer uniform (2 vec4, `LayerFrame`).
pub const FRAME_WORDS: usize = 8;

/// The word holding the flag bits.
pub const FLAGS_WORD: usize = 11;
/// Flag bits (word [`FLAGS_WORD`], stored as a float the shader reads back
/// as an integer).
pub const SHEEN: u32 = 1;
pub const CLEARCOAT: u32 = 2;
pub const ANISOTROPY: u32 = 4;
pub const IRIDESCENCE: u32 = 8;
pub const WEAVE_PLAIN: u32 = 16;
pub const WEAVE_KNIT: u32 = 32;

/// The layer BRDF functions (pure WGSL, prefixed `ly_`), appended to the
/// lit model shader by evani-render's `material-layers` feature.
pub const BRDF_WGSL: &str = include_str!("../shaders/layers_brdf.wgsl");

#[cfg(test)]
mod wgsl_tests;
