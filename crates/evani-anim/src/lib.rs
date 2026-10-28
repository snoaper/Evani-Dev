//! Evani.Dev animation runtime (`evani.module.anim`, STAGE3_PLAN S3-8): the
//! clip decoder for the `EVC1` block (docs/MODEL_FORMAT.md "Clips"), a
//! sampler with a cursor, bind pose, local to model space and skinning
//! matrices, three blend nodes (clip, blend of two, masked additive),
//! tick-based crossfades, 1D blend spaces with phase sync, two-bone and aim
//! IK, and attachment joints. Pure CPU scalar code, no dependency beyond
//! evani-core (for the module and its parameter), no allocation per frame.
//!
//! Two sides, kept apart:
//! - **Simulation** ([`Player`], [`Blend1d`], [`Player::hit`]): integer ticks
//!   stepped in the fixed 60 Hz update. This is all gameplay may read.
//! - **Render** ([`Clips`], [`Graph`], [`Rig`], IK): frame positions, blend
//!   weights, poses and skinning matrices made from that state each drawn
//!   frame; nothing here writes back into the simulation.
//!
//! The per-frame path: [`frame_at`] (or [`frame_at_phase`]) gives each clip
//! node its frame, [`Graph::eval`] samples and blends into a pose,
//! [`Rig::set_pose`] and [`Rig::update`] go to model space, IK
//! ([`Rig::two_bone`], [`Rig::aim`]) adjusts it, and [`Rig::skinning`] hands
//! S3-9 one flat slice of 3x4 matrices per character. Games get this crate
//! only through the `anim` module.

/// An [`Error`]: its full text natively, a short one in the wasm download
/// (the size budget, DECISIONS.md "Animation runtime"). The short text
/// names the same check, so a player's report still says what failed.
macro_rules! err {
    ($short:literal, $long:literal) => {
        crate::Error(if cfg!(target_arch = "wasm32") { concat!("anim: ", $short) } else { $long })
    };
}

mod clip;
mod graph;
mod ik;
pub mod math;
mod play;
mod rig;

#[cfg(test)]
mod tests;

pub use clip::{Clips, Cursor, RATE, ROTATION, SCALE, TICKS_PER_FRAME, TRANSLATION, Track, frame_at, frame_at_phase};
pub use graph::{ALL, Graph, Mask, Node, add_layer, blend_poses};
pub use ik::{Aim, TwoBone};
pub use math::{Mat43, Quat, Vec3, Xform};
pub use play::{Blend1d, Player, Point, pick};
pub use rig::{NONE, Rig, Skeleton, Skin};

use evani_core::params::{BakeTarget, I32Param, ParamDef};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};

/// The module marker: in a game's download exactly when it lists `anim`.
pub const MODULE_ID: &str = "evani.module.anim";

/// A decode or setup error (never a panic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

/// World resource: the engine's animation settings.
#[derive(Debug, Clone, Copy)]
pub struct AnimParams {
    /// `anim.crossfade_ticks`: the crossfade a kit uses when it has none of
    /// its own.
    pub crossfade: I32Param,
}

/// The engine module: registers the animation parameters. A game with
/// `anim` adds it (`App::with_module(AnimModule)`).
#[derive(Debug, Default)]
pub struct AnimModule;

impl EngineModule for AnimModule {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn label(&self) -> &'static str {
        "anim"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let crossfade = ctx.param_i32(
            ParamDef::i32("anim.crossfade_ticks", 12)
                .range(0.0, 120.0)
                .unit("ticks")
                .describe("Default crossfade between animation states, in 60 Hz ticks (12 = 0.2 s).")
                .owner("anim")
                .bake(BakeTarget::Kit),
        )?;
        ctx.resource(AnimParams { crossfade });
        Ok(())
    }
}
