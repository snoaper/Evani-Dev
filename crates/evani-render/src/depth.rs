//! The renderer's depth convention, in one place: reversed-Z.
//!
//! The camera passes map the near plane to depth 1 and the far plane to
//! depth 0 ([`crate::math::perspective`]), clear the depth target to
//! [`CLEAR`] (0, the far plane) and keep a fragment whose depth is greater
//! than the stored one (`COMPARE`, `Greater`). A perspective divide bunches
//! far depths together; a 32-bit float buffer is finest near 0, so putting
//! the far plane at 0 spreads the precision almost evenly over distance and
//! two surfaces a millimetre apart stay separate 150 m away (Nathan Reed,
//! "Depth Precision Visualized", 2015; NVIDIA, "Depth Precision"). The
//! z-fighting test in `tools/evani-geocheck` (`DepthSetup::engine`) reads
//! this module and the projection, so its depth model follows the renderer.
//!
//! Kits, effects and tools never hard-code the direction: they use these
//! constants and helpers. The forward shader is direction-free (a decal's
//! instance pad is already signed by [`toward_camera`]) and the outline
//! effect linearises depth with [`inverse_distance`].
//!
//! Shadow maps keep their own conventional depth (0 at the light's near
//! plane, 1 at its far plane, cleared to 1, drawn with `Less` and sampled
//! with a `LessEqual` comparison): their projection is orthographic, so
//! depth is linear in distance and reversing it gains nothing.

/// Near plane at depth 1, far plane at depth 0.
pub const REVERSED: bool = true;
/// Depth of a point on the near plane.
pub const NEAR_DEPTH: f32 = 1.0;
/// Depth of a point on the far plane.
pub const FAR_DEPTH: f32 = 0.0;
/// What the camera passes clear depth to: the far plane.
pub const CLEAR: f32 = FAR_DEPTH;
/// Sign of a depth change that moves a surface toward the camera.
pub const TOWARD_CAMERA: f32 = 1.0;

/// The camera passes' depth test: a fragment nearer than the stored depth
/// passes (wgpu only exists in the browser build).
#[cfg(target_arch = "wasm32")]
pub const COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::Greater;

/// Is depth `a` nearer the camera than depth `b`?
pub fn is_nearer(a: f32, b: f32) -> bool {
    a > b
}

/// The nearer of two depths (the MSAA depth resolve keeps this sample).
pub fn nearer(a: f32, b: f32) -> f32 {
    a.max(b)
}

/// A depth offset of `amount` (>= 0) toward the camera, signed for this
/// convention: what goes in a decal's instance pad, which the forward shader
/// adds to clip z times w.
pub fn toward_camera(amount: f32) -> f32 {
    TOWARD_CAMERA * amount
}

/// `[c0, c1]` with `1 / distance = c0 + c1 * depth` for a perspective
/// projection with these clip planes (inverse view distance is affine in
/// depth). Depth readers linearise with it instead of their own formula.
pub fn inverse_distance(near: f32, far: f32) -> [f32; 2] {
    let near = near.max(1e-6);
    let far = far.max(near * 2.0);
    // depth = near (far - z) / (z (far - near))
    [1.0 / far, (far - near) / (near * far)]
}

/// View distance (m along the camera axis) of a stored depth value.
pub fn distance(depth: f32, near: f32, far: f32) -> f32 {
    let [c0, c1] = inverse_distance(near, far);
    1.0 / (c0 + c1 * depth).max(1e-12)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math;
    use crate::shaders::{FAR, NEAR};

    /// Depth of a point straight ahead at view distance `z`.
    fn depth_at(z: f32) -> f32 {
        let proj = math::perspective(1.0, 1.0, NEAR, FAR);
        math::transform_point(&proj, [0.0, 0.0, -z])[2]
    }

    #[test]
    fn the_projection_follows_the_convention() {
        assert!((depth_at(NEAR) - NEAR_DEPTH).abs() < 1e-6);
        assert!((depth_at(FAR) - FAR_DEPTH).abs() < 1e-6);
        assert_eq!(CLEAR, FAR_DEPTH);
        assert!(is_nearer(depth_at(10.0), depth_at(10.5)));
        assert!(!is_nearer(depth_at(10.5), depth_at(10.0)));
        assert_eq!(nearer(depth_at(3.0), depth_at(4.0)), depth_at(3.0));
        assert_eq!(nearer(depth_at(4.0), CLEAR), depth_at(4.0));
        // Toward the camera is the direction of a nearer depth.
        assert!(is_nearer(depth_at(20.0) + toward_camera(1e-6), depth_at(20.0)));
        assert_eq!(REVERSED, NEAR_DEPTH > FAR_DEPTH);
    }

    #[test]
    fn depth_linearises_back_to_the_distance() {
        for z in [NEAR, 0.5, 3.0, 25.0, 150.0, 600.0, FAR] {
            let back = distance(depth_at(z), NEAR, FAR);
            assert!((back - z).abs() <= z * 1e-4, "{z} -> {back}");
        }
        assert!((distance(CLEAR, NEAR, FAR) - FAR).abs() < 1e-2);
    }

    #[test]
    fn far_depth_keeps_its_precision() {
        // One f32 step of the stored depth is worth well under a millimetre
        // of distance at 150 m (it was about 1.5 m with depth 0 at near).
        let z = 150.0f32;
        let d = depth_at(z);
        let next = f32::from_bits(d.to_bits() + 1);
        let [c0, c1] = inverse_distance(NEAR, FAR).map(f64::from);
        let dist = |d: f32| 1.0 / (c0 + c1 * f64::from(d));
        let dz = dist(d) - dist(next);
        assert!(dz > 0.0 && dz < 1e-3, "{dz}");
        assert!((dist(d) - f64::from(z)).abs() < 0.05, "{}", dist(d));
    }
}
