//! The collision module's live tunables, `physics.collide.*`: the
//! character mover's body size, step, slope limit, skin, ground snap and
//! iteration counts, plus the walking speed a game's own controller drives.
//!
//! [`crate::PhysicsModule`] registers them when the `collide` feature is on
//! and puts a [`CollideParams`] resource in the world. A game reads the
//! current values every fixed step with [`CollideParams::config`] (and
//! [`CollideParams::height`], [`CollideParams::speed`]), so a change from the
//! live tools (`param_set`, a sweep) applies on the next step.

use evani_core::params::{BakeTarget, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};

use crate::collide::character::{CharacterConfig, slope_cos};
use crate::collide::math::fmax;

/// Handles of the `physics.collide.*` parameters (a world resource).
#[derive(Debug, Clone, Copy)]
pub struct CollideParams {
    /// `physics.collide.radius`: capsule radius (m).
    pub radius: F32Param,
    /// `physics.collide.height`: capsule height, feet to head (m).
    pub height: F32Param,
    /// `physics.collide.step_height`: highest ledge stepped straight onto (m).
    pub step_height: F32Param,
    /// `physics.collide.max_slope`: steepest walkable slope (degrees).
    pub max_slope: F32Param,
    /// `physics.collide.skin`: gap kept from surfaces (m).
    pub skin: F32Param,
    /// `physics.collide.snap_distance`: how far down a walking body looks
    /// for ground to stay on (m).
    pub snap_distance: F32Param,
    /// `physics.collide.max_slides`: slide iterations per move.
    pub max_slides: I32Param,
    /// `physics.collide.depenetrate_passes`: push-out passes per move.
    pub depenetrate_passes: I32Param,
    /// `physics.collide.speed`: walking speed for the game's controller
    /// (m/s). The mover itself takes a motion, never a speed.
    pub speed: F32Param,
}

fn def(name: &str, default: f32, min: f32, max: f32, unit: &str, what: &str) -> ParamDef {
    ParamDef::f32(name, default).range(min, max).unit(unit).describe(what).owner("physics").bake(BakeTarget::Kit)
}

fn idef(name: &str, default: i32, min: f32, max: f32, unit: &str, what: &str) -> ParamDef {
    ParamDef::i32(name, default).range(min, max).unit(unit).describe(what).owner("physics").bake(BakeTarget::Kit)
}

impl CollideParams {
    /// Register every `physics.collide.*` parameter; the defaults are
    /// [`CharacterConfig::default`] with a 1.8 m body walking at 4.5 m/s.
    pub fn register(ctx: &mut PluginContext) -> Result<Self, PluginError> {
        let d = CharacterConfig::default();
        Ok(CollideParams {
            radius: ctx.param_f32(def("physics.collide.radius", d.radius, 0.05, 2.0, "m", "Character capsule radius."))?,
            height: ctx.param_f32(def("physics.collide.height", 1.8, 0.2, 5.0, "m", "Character capsule height, feet to head."))?,
            step_height: ctx.param_f32(def("physics.collide.step_height", d.step_height, 0.0, 1.5, "m", "Highest ledge the mover steps straight onto."))?,
            max_slope: ctx.param_f32(def("physics.collide.max_slope", 45.0, 0.0, 89.0, "deg", "Steepest walkable slope; steeper ones act as walls."))?,
            skin: ctx.param_f32(def("physics.collide.skin", d.skin, 0.0005, 0.05, "m", "Gap the mover keeps from surfaces."))?,
            snap_distance: ctx.param_f32(def(
                "physics.collide.snap_distance",
                d.snap_distance,
                0.0,
                1.5,
                "m",
                "How far down a walking body looks for ground to stay on (stairs and ramps going down).",
            ))?,
            max_slides: ctx.param_i32(idef("physics.collide.max_slides", d.max_slides as i32, 1.0, 8.0, "slides", "Slide iterations per move."))?,
            depenetrate_passes: ctx.param_i32(idef(
                "physics.collide.depenetrate_passes",
                d.depenetrate_passes as i32,
                0.0,
                8.0,
                "passes",
                "Push-out passes when a move starts inside something.",
            ))?,
            speed: ctx.param_f32(def("physics.collide.speed", 4.5, 0.0, 20.0, "m/s", "Walking speed a game's controller drives the mover at."))?,
        })
    }

    /// The mover config from the current values.
    pub fn config(&self, reg: &ParamRegistry) -> CharacterConfig {
        let count = |h: I32Param| if reg.i32(h) > 0 { reg.i32(h) as usize } else { 0 };
        CharacterConfig {
            radius: fmax(reg.f32(self.radius), 0.0),
            step_height: fmax(reg.f32(self.step_height), 0.0),
            min_ground_normal_y: slope_cos(reg.f32(self.max_slope)),
            skin: fmax(reg.f32(self.skin), 0.0),
            snap_distance: fmax(reg.f32(self.snap_distance), 0.0),
            max_slides: count(self.max_slides),
            depenetrate_passes: count(self.depenetrate_passes),
        }
    }

    /// The capsule height (m).
    pub fn height(&self, reg: &ParamRegistry) -> f32 {
        reg.f32(self.height)
    }

    /// The walking speed (m/s).
    pub fn speed(&self, reg: &ParamRegistry) -> f32 {
        reg.f32(self.speed)
    }
}

/// Register the parameters and add the [`CollideParams`] resource (called
/// from [`crate::PhysicsModule`]'s build when the feature is on).
pub fn register(ctx: &mut PluginContext) -> Result<(), PluginError> {
    let p = CollideParams::register(ctx)?;
    ctx.resource(p);
    Ok(())
}
