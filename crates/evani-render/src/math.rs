//! The little matrix math the renderer needs (column-major `[f32; 16]`,
//! right-handed, WebGPU depth range 0..1). Hand-rolled to keep wasm small.

pub type Mat4 = [f32; 16];

pub const IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Unit vector (zero stays zero).
pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0; 3] }
}

/// Right-handed perspective with reversed depth ([`crate::depth`]): depth 1
/// at `near`, 0 at `far` (a finite far plane, so `FAR` still clips as it
/// always has).
pub fn perspective(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_rad * 0.5).tan();
    let r = near / (far - near);
    [
        f / aspect.max(1e-6), 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, r, -1.0,
        0.0, 0.0, r * far, 0.0,
    ]
}

/// Right-handed orthographic projection of the view-space box
/// `[l, r] x [b, t]` between distances `near` and `far` along -z, depth 0
/// (near) .. 1 (far). Only the shadow map uses it, and it keeps this
/// conventional direction (see [`crate::depth`]).
pub fn ortho(l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Mat4 {
    let w = (r - l).max(1e-6);
    let h = (t - b).max(1e-6);
    let d = (far - near).max(1e-6);
    [
        2.0 / w, 0.0, 0.0, 0.0,
        0.0, 2.0 / h, 0.0, 0.0,
        0.0, 0.0, -1.0 / d, 0.0,
        -(r + l) / w, -(t + b) / h, -near / d, 1.0,
    ]
}

/// Right-handed view matrix.
pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        s[0], u[0], -f[0], 0.0,
        s[1], u[1], -f[1], 0.0,
        s[2], u[2], -f[2], 0.0,
        -dot(s, eye), -dot(u, eye), dot(f, eye), 1.0,
    ]
}

/// `a * b`.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

/// `m * [p, 1]` with the perspective divide.
pub fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    let v = |r: usize| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r];
    let w = v(3);
    [v(0) / w, v(1) / w, v(2) / w]
}

/// Quaternion `[x, y, z, w]` for a rotation of `angle` radians about `axis`.
pub fn quat_axis_angle(axis: [f32; 3], angle: f32) -> [f32; 4] {
    let a = normalize(axis);
    let (s, c) = (angle * 0.5).sin_cos();
    [a[0] * s, a[1] * s, a[2] * s, c]
}

/// `a * b` (apply `b`, then `a`).
pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn camera_projects_target_to_centre() {
        let view = look_at([0.0, 5.0, 10.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let proj = perspective(1.0, 16.0 / 9.0, 0.1, 100.0);
        let vp = mul(&proj, &view);
        let c = transform_point(&vp, [0.0, 0.0, 0.0]);
        assert!(c[0].abs() < 1e-5 && c[1].abs() < 1e-5);
        assert!(c[2] > 0.0 && c[2] < 1.0, "{c:?}");
        // Reversed depth: the near plane maps to 1, far to 0.
        let dist = 125f32.sqrt();
        let fwd = normalize([0.0, -5.0, -10.0]);
        let at = |d: f32| [fwd[0] * d, 5.0 + fwd[1] * d, 10.0 + fwd[2] * d];
        assert!((transform_point(&vp, at(0.1))[2] - crate::depth::NEAR_DEPTH).abs() < 1e-4);
        assert!((transform_point(&vp, at(100.0))[2] - crate::depth::FAR_DEPTH).abs() < 1e-4);
        assert!(transform_point(&vp, at(dist))[2] > 0.0);
        assert!(transform_point(&vp, at(dist))[2] > transform_point(&vp, at(dist + 1.0))[2]);
        assert_eq!(mul(&IDENTITY, &vp), vp);
    }

    #[test]
    fn quaternions_compose() {
        let q = quat_axis_angle([0.0, 1.0, 0.0], core::f32::consts::FRAC_PI_2);
        let qq = quat_mul(q, q);
        let half = quat_axis_angle([0.0, 1.0, 0.0], core::f32::consts::PI);
        assert!(close([qq[0], qq[1], qq[2]], [half[0], half[1], half[2]]));
        assert!((qq[3] - half[3]).abs() < 1e-5);
    }
}
