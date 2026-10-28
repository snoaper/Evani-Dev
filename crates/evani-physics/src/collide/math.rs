//! Small vector helpers on `[f32; 3]` (the engine's own vector type in
//! `evani-render`), so the module needs no maths crate.
//!
//! Only `+ - * /`, `sqrt` (correctly rounded by IEEE 754 on every target)
//! and plain comparisons: no platform maths library, and no `f32::min`,
//! `max` or `hypot`, whose signed-zero and rounding behaviour may differ
//! between native and wasm. So every result has the same bits everywhere.

/// A 3D vector or point.
pub type V3 = [f32; 3];

/// `a + b`.
#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`.
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a * s`.
#[inline]
pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `a + b * s`.
#[inline]
pub fn mad(a: V3, b: V3, s: f32) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

/// Dot product.
#[inline]
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Cross product.
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Squared length.
#[inline]
pub fn len_sq(a: V3) -> f32 {
    dot(a, a)
}

/// Length.
#[inline]
pub fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// Unit vector, or `None` for a (near) zero vector.
#[inline]
pub fn normalize(a: V3) -> Option<V3> {
    let l = len(a);
    if l > 1e-12 { Some(scale(a, 1.0 / l)) } else { None }
}

/// Component-wise minimum.
#[inline]
pub fn min(a: V3, b: V3) -> V3 {
    [fmin(a[0], b[0]), fmin(a[1], b[1]), fmin(a[2], b[2])]
}

/// Component-wise maximum.
#[inline]
pub fn max(a: V3, b: V3) -> V3 {
    [fmax(a[0], b[0]), fmax(a[1], b[1]), fmax(a[2], b[2])]
}

/// Linear interpolation `a + (b - a) * t`.
#[inline]
pub fn lerp(a: V3, b: V3, t: f32) -> V3 {
    mad(a, sub(b, a), t)
}

/// `v` with its component along unit `n` removed.
#[inline]
pub fn reject(v: V3, n: V3) -> V3 {
    mad(v, n, -dot(v, n))
}

/// Horizontal length (x and z).
#[inline]
pub fn len_xz(a: V3) -> f32 {
    (a[0] * a[0] + a[2] * a[2]).sqrt()
}

/// The smaller of `a` and `b` by plain comparisons, the same on every
/// target. Like `f32::min` a NaN is ignored (the other value is returned);
/// of two equal values (`-0.0` and `0.0`) it returns `b`.
#[inline]
pub fn fmin(a: f32, b: f32) -> f32 {
    if a < b || b.is_nan() { a } else { b }
}

/// The larger of `a` and `b` by plain comparisons, the same on every
/// target. Like `f32::max` a NaN is ignored (the other value is returned);
/// of two equal values (`-0.0` and `0.0`) it returns `b`.
#[inline]
pub fn fmax(a: f32, b: f32) -> f32 {
    if a > b || b.is_nan() { a } else { b }
}

/// `x` clamped to `0..=1` by plain comparisons (NaN gives 0). Unlike
/// `f32::clamp` it has no assert, so no float formatting ships.
#[inline]
pub fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}
