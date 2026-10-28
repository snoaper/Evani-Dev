//! Small f64 vector maths (the checks work in f64 so thin seams and 2 cm
//! paint lines are measured exactly, whatever the f32 scene says).

pub type V3 = [f64; 3];

#[inline]
pub fn v3(a: [f32; 3]) -> V3 {
    [f64::from(a[0]), f64::from(a[1]), f64::from(a[2])]
}
#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l < 1e-300 { [0.0; 3] } else { mul(a, 1.0 / l) }
}
#[inline]
pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(a, mul(sub(b, a), t))
}
#[inline]
pub fn dist(a: V3, b: V3) -> f64 {
    len(sub(a, b))
}

/// Rotate `v` by the unit quaternion `q` = `[x, y, z, w]`.
pub fn rotate(q: [f64; 4], v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let w = q[3];
    let t = mul(cross(u, v), 2.0);
    add(add(v, mul(t, w)), cross(u, t))
}

/// Normalised quaternion in f64 (identity for a zero quaternion).
pub fn quat(q: [f32; 4]) -> [f64; 4] {
    let q = [f64::from(q[0]), f64::from(q[1]), f64::from(q[2]), f64::from(q[3])];
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l < 1e-12 { [0.0, 0.0, 0.0, 1.0] } else { [q[0] / l, q[1] / l, q[2] / l, q[3] / l] }
}

/// Axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };

    pub fn from_points<'a>(pts: impl IntoIterator<Item = &'a V3>) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in pts {
            b.add(*p);
        }
        b
    }
    pub fn add(&mut self, p: V3) {
        for (i, x) in p.iter().enumerate() {
            self.min[i] = self.min[i].min(*x);
            self.max[i] = self.max[i].max(*x);
        }
    }
    pub fn union(&self, o: &Aabb) -> Aabb {
        let mut b = *self;
        b.add(o.min);
        b.add(o.max);
        b
    }
    pub fn is_empty(&self) -> bool {
        (0..3).any(|i| self.min[i] > self.max[i])
    }
    /// Boxes overlap (or come within `margin` of each other).
    pub fn overlaps(&self, o: &Aabb, margin: f64) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] + margin && o.min[i] <= self.max[i] + margin)
    }
    pub fn intersect(&self, o: &Aabb) -> Aabb {
        let mut b = *self;
        for i in 0..3 {
            b.min[i] = b.min[i].max(o.min[i]);
            b.max[i] = b.max[i].min(o.max[i]);
        }
        b
    }
    pub fn extent(&self) -> V3 {
        if self.is_empty() { [0.0; 3] } else { sub(self.max, self.min) }
    }
    pub fn center(&self) -> V3 {
        mul(add(self.min, self.max), 0.5)
    }
    pub fn max_extent(&self) -> f64 {
        let e = self.extent();
        e[0].max(e[1]).max(e[2])
    }
    pub fn min_extent(&self) -> f64 {
        let e = self.extent();
        e[0].min(e[1]).min(e[2])
    }
    pub fn contains(&self, p: V3, margin: f64) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] - margin && p[i] <= self.max[i] + margin)
    }
    pub fn grow(&self, m: f64) -> Aabb {
        Aabb {
            min: sub(self.min, [m; 3]),
            max: add(self.max, [m; 3]),
        }
    }
}

/// An orthonormal basis `(u, v)` of the plane with normal `n`.
pub fn plane_basis(n: V3) -> (V3, V3) {
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = norm(cross(n, a));
    let v = cross(n, u);
    (u, v)
}

/// 2D convex hull (counter-clockwise, no collinear points), Andrew's monotone chain.
pub fn hull_2d(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = pts.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12);
    if p.len() < 3 {
        return p;
    }
    let cr = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = Vec::with_capacity(p.len() * 2);
    for &q in &p {
        while h.len() >= 2 && cr(h[h.len() - 2], h[h.len() - 1], q) <= 1e-14 {
            h.pop();
        }
        h.push(q);
    }
    let lower = h.len() + 1;
    for &q in p.iter().rev().skip(1) {
        while h.len() >= lower && cr(h[h.len() - 2], h[h.len() - 1], q) <= 1e-14 {
            h.pop();
        }
        h.push(q);
    }
    h.pop();
    h
}

/// Signed area of a 2D polygon (positive counter-clockwise).
pub fn area_2d(p: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (x, y) = (p[i], p[(i + 1) % p.len()]);
        a += x[0] * y[1] - y[0] * x[1];
    }
    0.5 * a
}

/// Intersection of two convex counter-clockwise 2D polygons
/// (Sutherland-Hodgman). Empty when they don't overlap.
pub fn clip_2d(subject: &[[f64; 2]], clip: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = subject.to_vec();
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let side = |p: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_quarter_turn_about_y() {
        let h = std::f64::consts::FRAC_PI_4;
        let q = [0.0, h.sin(), 0.0, h.cos()];
        let r = rotate(q, [1.0, 0.0, 0.0]);
        assert!(dist(r, [0.0, 0.0, -1.0]) < 1e-12, "{r:?}");
    }

    #[test]
    fn hull_and_clip_2d() {
        let sq = |x: f64, y: f64, s: f64| vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]];
        let h = hull_2d(&[[0.0, 0.0], [1.0, 0.0], [0.5, 0.5], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(h.len(), 4);
        assert!((area_2d(&h) - 1.0).abs() < 1e-12);
        let i = clip_2d(&sq(0.0, 0.0, 2.0), &sq(1.0, 1.0, 2.0));
        assert!((area_2d(&i) - 1.0).abs() < 1e-12);
        assert!(clip_2d(&sq(0.0, 0.0, 1.0), &sq(3.0, 3.0, 1.0)).len() < 3);
    }

    #[test]
    fn aabb_ops() {
        let a = Aabb::from_points(&[[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]]);
        let b = Aabb::from_points(&[[0.5, 1.0, 1.0], [4.0, 4.0, 4.0]]);
        assert!(a.overlaps(&b, 0.0));
        assert_eq!(a.intersect(&b).extent(), [0.5, 1.0, 2.0]);
        assert_eq!(a.max_extent(), 3.0);
        assert!(!a.overlaps(&Aabb::from_points(&[[2.0, 0.0, 0.0], [3.0, 1.0, 1.0]]), 0.5));
    }
}
