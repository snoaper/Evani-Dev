//! Closest-point and intersection primitives between points, segments,
//! rays and triangles. Every routine visits its candidates in a fixed order
//! and keeps the first strictly smaller one, so ties resolve the same way
//! on every run.

use crate::collide::math::{V3, add, clamp01, cross, dot, len_sq, mad, scale, sub};

/// Relative tolerance of the point-in-triangle test, so a ray or segment
/// that hits exactly on the shared edge of two triangles is never lost
/// through a crack between them.
const INSIDE_EPS: f32 = 1e-6;

/// The closest pair between a segment and a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Closest {
    /// Squared distance between the two points.
    pub dist_sq: f32,
    /// The point on the segment.
    pub on_seg: V3,
    /// The point on the triangle.
    pub on_tri: V3,
}

/// Parameter in `[0, 1]` and point of the closest point to `p` on the
/// segment `a..b`.
pub fn point_segment(p: V3, a: V3, b: V3) -> (f32, V3) {
    let ab = sub(b, a);
    let l = len_sq(ab);
    if l <= 1e-20 {
        return (0.0, a);
    }
    let t = clamp01(dot(sub(p, a), ab) / l);
    (t, mad(a, ab, t))
}

/// Whether `q`, a point in the triangle's plane, lies inside the triangle
/// `a b c` with (unnormalised) normal `n`.
fn inside(q: V3, a: V3, b: V3, c: V3, n: V3) -> bool {
    let tol = -INSIDE_EPS * len_sq(n);
    dot(cross(sub(b, a), sub(q, a)), n) >= tol && dot(cross(sub(c, b), sub(q, b)), n) >= tol && dot(cross(sub(a, c), sub(q, c)), n) >= tol
}

/// The closest point to `p` on the triangle `a b c` (a degenerate triangle
/// is treated as its edges).
pub fn point_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let n = cross(sub(b, a), sub(c, a));
    let nn = len_sq(n);
    if nn > 1e-20 {
        let q = mad(p, n, -dot(sub(p, a), n) / nn);
        if inside(q, a, b, c, n) {
            return q;
        }
    }
    // Outside the face: the closest point is on the boundary.
    let mut best = point_segment(p, a, b).1;
    let mut best_d = len_sq(sub(p, best));
    for (u, v) in [(b, c), (c, a)] {
        let q = point_segment(p, u, v).1;
        let d = len_sq(sub(p, q));
        if d < best_d {
            best = q;
            best_d = d;
        }
    }
    best
}

/// The closest points between segments `p1..q1` and `p2..q2`, as
/// `(point on the first, point on the second)`. Parallel and degenerate
/// segments are handled (any closest pair is returned, picked the same way
/// every time).
pub fn segment_segment(p1: V3, q1: V3, p2: V3, q2: V3) -> (V3, V3) {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = len_sq(d1);
    let e = len_sq(d2);
    let f = dot(d2, r);
    const EPS: f32 = 1e-20;
    let (s, t);
    if a <= EPS && e <= EPS {
        return (p1, p2);
    }
    if a <= EPS {
        s = 0.0;
        t = clamp01(f / e);
    } else {
        let c = dot(d1, r);
        if e <= EPS {
            t = 0.0;
            s = clamp01(-c / a);
        } else {
            // Minimise |r + d1 s - d2 t|^2: solve the 2x2 system, clamp s,
            // then t from s, then s again if t had to be clamped.
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let s0 = if denom > EPS * a * e { clamp01((b * f - c * e) / denom) } else { 0.0 };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = clamp01(-c / a);
            } else if t0 > 1.0 {
                t = 1.0;
                s = clamp01((b - c) / a);
            } else {
                t = t0;
                s = s0;
            }
        }
    }
    (mad(p1, d1, s), mad(p2, d2, t))
}

/// The closest pair between the segment `p..q` and the triangle `a b c`.
/// A segment that pierces the triangle gives distance 0 at the piercing
/// point.
pub fn segment_triangle(p: V3, q: V3, a: V3, b: V3, c: V3) -> Closest {
    let n = cross(sub(b, a), sub(c, a));
    let nn = len_sq(n);
    if nn > 1e-20 {
        let dp = dot(sub(p, a), n);
        let dq = dot(sub(q, a), n);
        if ((dp <= 0.0 && dq >= 0.0) || (dp >= 0.0 && dq <= 0.0)) && dp != dq {
            let t = dp / (dp - dq);
            let x = mad(p, sub(q, p), t);
            if inside(x, a, b, c, n) {
                return Closest { dist_sq: 0.0, on_seg: x, on_tri: x };
            }
        }
    }
    // Endpoints against the face, then the segment against each edge.
    let tp = point_triangle(p, a, b, c);
    let mut best = Closest { dist_sq: len_sq(sub(p, tp)), on_seg: p, on_tri: tp };
    let tq = point_triangle(q, a, b, c);
    let dq = len_sq(sub(q, tq));
    if dq < best.dist_sq {
        best = Closest { dist_sq: dq, on_seg: q, on_tri: tq };
    }
    for (u, v) in [(a, b), (b, c), (c, a)] {
        let (s, e) = segment_segment(p, q, u, v);
        let d = len_sq(sub(s, e));
        if d < best.dist_sq {
            best = Closest { dist_sq: d, on_seg: s, on_tri: e };
        }
    }
    best
}

/// Distance `t` along the ray `o + d t` (`d` need not be unit) to the
/// two-sided triangle `a b c`, for `t` in `[0, t_max]`.
pub fn ray_triangle(o: V3, d: V3, t_max: f32, a: V3, b: V3, c: V3) -> Option<f32> {
    let n = cross(sub(b, a), sub(c, a));
    let denom = dot(d, n);
    if denom.abs() <= 1e-12 * len_sq(n).sqrt() * len_sq(d).sqrt() {
        return None;
    }
    let t = dot(sub(a, o), n) / denom;
    if !(0.0..=t_max).contains(&t) {
        return None;
    }
    let x = add(o, scale(d, t));
    if inside(x, a, b, c, n) { Some(t) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: V3 = [0.0, 0.0, 0.0];
    const B: V3 = [2.0, 0.0, 0.0];
    const C: V3 = [0.0, 0.0, 2.0];

    fn close(a: V3, b: V3) -> bool {
        len_sq(sub(a, b)) < 1e-10
    }

    #[test]
    fn point_triangle_regions() {
        // Above the face: straight down.
        assert!(close(point_triangle([0.5, 3.0, 0.5], A, B, C), [0.5, 0.0, 0.5]));
        // Beyond a vertex.
        assert!(close(point_triangle([-1.0, 1.0, -1.0], A, B, C), A));
        // Beyond the hypotenuse.
        assert!(close(point_triangle([2.0, 0.0, 2.0], A, B, C), [1.0, 0.0, 1.0]));
    }

    #[test]
    fn segment_segment_cases() {
        // Crossing skew lines.
        let (s, t) = segment_segment([0.0, 1.0, -1.0], [0.0, 1.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        assert!(close(s, [0.0, 1.0, 0.0]) && close(t, [0.0, 0.0, 0.0]));
        // Parallel: some pair at the right distance.
        let (s, t) = segment_segment([0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [0.5, 0.0, 0.0], [3.0, 0.0, 0.0]);
        assert!((len_sq(sub(s, t)) - 1.0).abs() < 1e-6);
        // Degenerate (points).
        let (s, t) = segment_segment([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        assert!(close(s, [0.0; 3]) && close(t, [1.0, 0.0, 0.0]));
    }

    #[test]
    fn segment_triangle_pierce_and_near() {
        let pierce = segment_triangle([0.5, -1.0, 0.5], [0.5, 1.0, 0.5], A, B, C);
        assert_eq!(pierce.dist_sq, 0.0);
        assert!(close(pierce.on_tri, [0.5, 0.0, 0.5]));
        // A vertical segment hovering over the face: its low end.
        let over = segment_triangle([0.5, 0.5, 0.5], [0.5, 2.0, 0.5], A, B, C);
        assert!((over.dist_sq - 0.25).abs() < 1e-6);
        // A horizontal segment passing beside the hypotenuse edge.
        let edge = segment_triangle([2.0, 0.0, 0.0 + 2.0], [3.0, 0.0, 1.0 + 2.0], A, B, C);
        assert!(edge.dist_sq > 0.0);
        assert!(close(edge.on_tri, [1.0, 0.0, 1.0]));
    }

    #[test]
    fn ray_triangle_two_sided() {
        assert_eq!(ray_triangle([0.5, 2.0, 0.5], [0.0, -1.0, 0.0], 10.0, A, B, C), Some(2.0));
        assert_eq!(ray_triangle([0.5, -2.0, 0.5], [0.0, 1.0, 0.0], 10.0, A, B, C), Some(2.0));
        assert_eq!(ray_triangle([0.5, 2.0, 0.5], [0.0, -1.0, 0.0], 1.0, A, B, C), None);
        assert_eq!(ray_triangle([3.0, 2.0, 3.0], [0.0, -1.0, 0.0], 10.0, A, B, C), None);
        // Parallel to the plane.
        assert_eq!(ray_triangle([0.5, 1.0, 0.5], [1.0, 0.0, 0.0], 10.0, A, B, C), None);
    }
}
