//! Evani.Dev material catalogue recipes.
//!
//! P1 of the batch 1 build plan (`docs/materials/BATCH1_BUILD_PLAN.md`): the
//! 32-byte material block ([`block`]) and, behind the dev-only `catalog` feature,
//! the reader for `catalog/materials/*.json` ([`catalog`]). There is no shader
//! here yet. No game lists this crate; it is opt-in, so a game that doesn't use it
//! doesn't compile or download any of it.
//!
//! P2a adds [`shader`]: the recipe stage WGSL (`shaders/core.wgsl`, the
//! dispatch and condition pass generated from the compiled recipes and
//! layers) and the load-time patch into the model lit stage. Text only; the
//! engine wiring is P2b.
//!
//! P2b adds [`params`] (feature `engine`): the live switches and the words of
//! the shared per-frame uniform; evani-render's `material-recipes` binds the
//! block and that uniform and applies the patch.
//!
//! P3 adds the eight condition layers (features `cond-<layer>`,
//! `shaders/cond_<layer>.wgsl`) and [`conditions`] (feature `engine`): their
//! `material.cond.*` switches, the amount rule and the strongest-N mask.
//!
//! P4 adds the first recipes (features `recipe-metal`, `recipe-wood`,
//! `recipe-stone`, `recipe-ceramic`, `shaders/rc_<name>.wgsl`) and [`recipes`]:
//! their schema rows, and the guide for adding a recipe.
//!
//! P5 adds bark, masonry, plaster and ground (features `recipe-bark`,
//! `recipe-masonry`, `recipe-plaster`, `recipe-ground`).
//!
//! P6 adds leather, fur, hide and keratin (features `recipe-leather`,
//! `recipe-fur`, `recipe-hide`, `recipe-keratin`).
//!
//! P7 adds feather and scales (features `recipe-feather`, `recipe-scales`).
//!
//! P8 adds gem and ice (features `recipe-gem`, `recipe-ice`).
//!
//! P9 adds paint's extras, plastic, organic, fire and glow (features
//! `recipe-paint`, `recipe-plastic`, `recipe-organic`, `recipe-fire`, `recipe-glow`).
//!
//! Batch 2's B2-P1 adds foliage (feature `recipe-foliage`): leaves, needles,
//! grass, straw, turf, moss and litter, now in the recipe stage.
//!
//! P10a adds [`gltf`] (feature `gltf`, dev and tools only): a catalogue row as
//! the glTF material games and the viewer draw, with each family's fields, and
//! paint's clear coat drawn by the recipe stage from the block.
//!
//! Batch 3 B3-P1 adds the pattern layouts every recipe can call (feature
//! `layout`, `shaders/rc_layout.wgsl`: bonds, herringbone, basket, chequer,
//! hex, shingles, panels, studs, lines and motifs) and [`layout`], their cell
//! maths on the CPU under tests. Masonry's bonds moved there; wood's nail
//! heads, stone's tiles, metal's and paint's rivets and ceramic's motifs use it.
//! The style detail step adds [`style`] (feature `style`, dev viewers only): a
//! recipe's pattern kept, simplified, inside a non-realistic art style's flat
//! colours, driven by the reserved word of the per-frame uniform.

//!
//! Batch 3 B3-P2 adds the cut-out (feature `cutout`, `shaders/cut.wgsl` and
//! `shaders/cut_stage.wgsl`): holes for grates, chain-link, wire mesh and
//! lattice, from the block alone, shared by the colour and shadow passes; and
//! [`cut`], which blocks are cut and the coverage maths on the CPU.

pub mod blend;
pub mod block;
#[cfg(feature = "catalog")]
pub mod catalog;
#[cfg(feature = "engine")]
pub mod conditions;
pub mod cut;
#[cfg(feature = "gltf")]
pub mod gltf;
pub mod layout;
#[cfg(feature = "engine")]
pub mod params;
pub mod recipes;
pub mod shader;
#[cfg(feature = "split")]
pub mod split;
#[cfg(feature = "style")]
pub mod style;

pub use block::{Block, Fields, BLOCK_BYTES};
