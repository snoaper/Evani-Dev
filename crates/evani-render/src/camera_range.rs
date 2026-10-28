//! Per-game camera near and far planes (feature `camera-range`, ChimpDuty
//! intake). Without it the world camera clips at the fixed
//! [`crate::shaders::NEAR`] 0.1 m and [`crate::shaders::FAR`] 1000 m.
//!
//! With it two live params set the planes, and every place that projects
//! the world camera reads them: the forward, model and particle frame
//! uniforms ([`crate::shaders::view_proj`]), frustum culling, the GTAO depth
//! linearisation and, through evani-fx's own `camera-range` feature (which
//! turns this one on), decals' and the outline's depth reconstruction and
//! the fog's far-edge fade.
//!
//! * `render.camera.near` (m, default 0.1, 0.005..10): a first-person game
//!   sets it to 0.01 to 0.03 so a wall the player stands against never
//!   clips away.
//! * `render.camera.far` (m, default 1000, 10..100000): a big outdoor map
//!   raises it; a small indoor one lowers it for depth precision.
//!
//! Depth is reversed (1 at near, 0 at far, [`crate::depth`]) in a 32-bit
//! float buffer, so a near plane 10x closer costs almost no precision far
//! away. The far plane is kept at least 10x the near plane.
//!
//! The values are read once a frame ([`sync`], at the start of the 3D path)
//! into a per-thread cell, so `view_proj` keeps its signature and every
//! caller gets the same planes in the same frame.

use core::cell::Cell;

use evani_core::camera::CameraPose;
use evani_core::params::{ParamDef, ParamError, ParamRegistry};
use evani_core::plugin::PluginContext;

use crate::math::{self, Mat4};
use crate::shaders::{FAR, NEAR};

/// The near plane param (metres).
pub const NEAR_PARAM: &str = "render.camera.near";
/// The far plane param (metres).
pub const FAR_PARAM: &str = "render.camera.far";
/// Range of `render.camera.near`.
pub const NEAR_RANGE: (f32, f32) = (0.005, 10.0);
/// Range of `render.camera.far`.
pub const FAR_RANGE: (f32, f32) = (10.0, 100_000.0);
/// The far plane is kept at least this many times the near plane.
pub const MIN_RATIO: f32 = 10.0;

thread_local! {
    static RANGE: Cell<(f32, f32)> = const { Cell::new((NEAR, FAR)) };
}

/// Register the two params (render module build).
pub(crate) fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    ctx.param_f32(
        ParamDef::f32(NEAR_PARAM, NEAR)
            .range(NEAR_RANGE.0, NEAR_RANGE.1)
            .step(0.005)
            .unit("m")
            .describe("Camera near clip plane (first-person games: 0.01 to 0.03, so walls never clip when the player stands against them)")
            .gpu(),
    )?;
    ctx.param_f32(
        ParamDef::f32(FAR_PARAM, FAR)
            .range(FAR_RANGE.0, FAR_RANGE.1)
            .step(10.0)
            .unit("m")
            .describe("Camera far clip plane (the draw distance; fog fades out by it)")
            .gpu(),
    )?;
    Ok(())
}

/// The planes `reg` asks for, made safe: finite, in range, far at least
/// [`MIN_RATIO`] x near. A missing param keeps the fixed default.
pub fn from_params(reg: &ParamRegistry) -> (f32, f32) {
    let get = |name: &str, d: f32| reg.f32_handle(name).map_or(d, |h| reg.f32(h));
    resolve(get(NEAR_PARAM, NEAR), get(FAR_PARAM, FAR))
}

/// `near` and `far` made safe (see [`from_params`]).
pub fn resolve(near: f32, far: f32) -> (f32, f32) {
    let near = if near.is_finite() { evani_core::num::clamp(near, NEAR_RANGE.0, NEAR_RANGE.1) } else { NEAR };
    let far = if far.is_finite() { evani_core::num::clamp(far, FAR_RANGE.0, FAR_RANGE.1) } else { FAR };
    (near, far.max(near * MIN_RATIO))
}

/// Read this frame's planes from the params (the renderer calls it at the
/// start of the 3D path; tools that project the camera themselves may too).
pub fn sync(reg: &ParamRegistry) {
    set(from_params(reg));
}

/// Set this thread's planes directly (tests and tools).
pub fn set(range: (f32, f32)) {
    let (n, f) = resolve(range.0, range.1);
    RANGE.with(|c| c.set((n, f)));
}

/// This frame's (near, far) planes in metres.
pub fn current() -> (f32, f32) {
    RANGE.with(Cell::get)
}

/// Projection * view for `camera` with this frame's planes (what
/// [`crate::shaders::view_proj`] returns with the feature on).
pub fn view_proj(camera: &CameraPose, aspect: f32) -> Mat4 {
    let (n, f) = current();
    let proj = math::perspective(camera.fov_y_deg.to_radians(), aspect, n, f);
    let view = math::look_at(camera.eye, camera.target, camera.up);
    math::mul(&proj, &view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depth::{self, NEAR_DEPTH};
    use evani_core::params::ParamValue;

    fn depth_at(vp: &Mat4, z: f32) -> f32 {
        math::transform_point(vp, [0.0, 0.0, -z])[2]
    }

    #[test]
    fn resolve_keeps_planes_sane() {
        assert_eq!(resolve(0.1, 1000.0), (0.1, 1000.0));
        assert_eq!(resolve(0.0, 1000.0).0, NEAR_RANGE.0);
        assert_eq!(resolve(f32::NAN, f32::INFINITY), (NEAR, FAR));
        // Far never closer than 10x near.
        assert_eq!(resolve(5.0, 10.0), (5.0, 50.0));
    }

    #[test]
    fn near_plane_follows_the_param() {
        let mut reg = ParamRegistry::new();
        reg.register(ParamDef::f32(NEAR_PARAM, NEAR).range(NEAR_RANGE.0, NEAR_RANGE.1)).unwrap();
        reg.register(ParamDef::f32(FAR_PARAM, FAR).range(FAR_RANGE.0, FAR_RANGE.1)).unwrap();
        reg.set_by_name(NEAR_PARAM, ParamValue::F32(0.01)).unwrap();
        reg.set_by_name(FAR_PARAM, ParamValue::F32(5000.0)).unwrap();
        sync(&reg);
        assert_eq!(current(), (0.01, 5000.0));
        let cam = CameraPose { eye: [0.0; 3], target: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0], fov_y_deg: 70.0 };
        let vp = view_proj(&cam, 1.0);
        // A point 2 cm ahead is inside the view (the fixed 0.1 m would clip it).
        let d = depth_at(&vp, 0.02);
        assert!(d > 0.0 && d < 1.0, "{d}");
        assert!((depth_at(&vp, 0.01) - NEAR_DEPTH).abs() < 1e-4);
        // The far plane lands on the far depth, and the distance comes back.
        assert!(depth_at(&vp, 5000.0).abs() < 1e-4);
        let back = depth::distance(depth_at(&vp, 250.0), 0.01, 5000.0);
        assert!((back - 250.0).abs() / 250.0 < 1e-3, "{back}");
        // The shared view_proj gives the same matrix with the feature on.
        assert_eq!(crate::shaders::view_proj(&cam, 1.0), vp);
        set((NEAR, FAR));
    }

    #[test]
    fn defaults_match_the_fixed_planes() {
        let reg = ParamRegistry::new();
        assert_eq!(from_params(&reg), (NEAR, FAR));
    }
}
