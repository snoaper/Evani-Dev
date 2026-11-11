//! Curves, arc-length paths and frames.
//!
//! A [`Curve`] maps `t` in [0, 1] to a point. A [`Path`] samples a curve
//! densely once and answers by arc length (`at`, `resample`), and gives
//! frames along it: with an up direction, or by parallel transport (the
//! double-reflection method of Wang, Juttler, Zheng and Liu, 2008: a
//! published method, written here from the maths).

use crate::dmath;
use crate::vec::*;

/// A curve through space, `t` in [0, 1].
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// Straight segments through the points, `t` by point index.
    Polyline(Vec<V3>),
    /// Centripetal Catmull-Rom through every point (alpha 0.5: no cusps or
    /// self-loops within a segment), ends extended by reflection.
    CatmullRom(Vec<V3>),
    /// Cubic Bezier segments: `p0 c0 c1 p1 c2 c3 p2 ...` (3n + 1 points).
    Bezier(Vec<V3>),
    /// Circular arc about `centre` in the plane of the unit axes `u` and `v`,
    /// from angle `a0` to `a1` (radians, from `u` towards `v`).
    Arc { centre: V3, radius: f64, a0: f64, a1: f64, u: V3, v: V3 },
    /// Helix about the y axis through `centre`: `turns` turns, rising `pitch`
    /// per turn, starting at angle `a0`.
    Helix { centre: V3, radius: f64, pitch: f64, turns: f64, a0: f64 },
    /// A hanging rope between `a` and `b`, its lowest point `sag` below the
    /// chord's middle (a true catenary, `y = c cosh(x / c)`, solved for `c`).
    Catenary { a: V3, b: V3, sag: f64 },
}

fn cosh(x: f64) -> f64 {
    let e = dmath::exp(x);
    0.5 * (e + 1.0 / e)
}

/// The catenary constant `c` whose span `l` sags by `sag`: `c (cosh(l / 2c) - 1) = sag`.
fn catenary_c(l: f64, sag: f64) -> f64 {
    let f = |c: f64| c * (cosh(l / (2.0 * c)) - 1.0) - sag;
    // f falls as c grows; bisect in log space between a tight and a slack rope.
    let (mut lo, mut hi) = (l * 1e-3, l * 1e4);
    for _ in 0..200 {
        let mid = (lo * hi).sqrt();
        if f(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo * hi).sqrt()
}

/// Centripetal Catmull-Rom point on the segment p1..p2 at local `s` in [0, 1]
/// (Barry and Goldman's pyramid form).
fn catmull(p0: V3, p1: V3, p2: V3, p3: V3, s: f64) -> V3 {
    let knot = |a: V3, b: V3| dist(a, b).sqrt().max(1e-9);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * s;
    let l = |a: V3, b: V3, ta: f64, tb: f64| lerp(a, b, (t - ta) / (tb - ta));
    let a1 = l(p0, p1, t0, t1);
    let a2 = l(p1, p2, t1, t2);
    let a3 = l(p2, p3, t2, t3);
    let b1 = l(a1, a2, t0, t2);
    let b2 = l(a2, a3, t1, t3);
    l(b1, b2, t1, t2)
}

impl Curve {
    /// The point at `t` (clamped to [0, 1]).
    pub fn point(&self, t: f64) -> V3 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Polyline(p) | Curve::CatmullRom(p) if p.len() < 2 => p.first().copied().unwrap_or([0.0; 3]),
            Curve::Polyline(p) => {
                let x = t * (p.len() - 1) as f64;
                let i = (x.floor() as usize).min(p.len() - 2);
                lerp(p[i], p[i + 1], x - i as f64)
            }
            Curve::CatmullRom(p) => {
                let n = p.len();
                let x = t * (n - 1) as f64;
                let i = (x.floor() as usize).min(n - 2);
                let before = if i == 0 { sub(mul(p[0], 2.0), p[1]) } else { p[i - 1] };
                let after = if i + 2 >= n { sub(mul(p[n - 1], 2.0), p[n - 2]) } else { p[i + 2] };
                catmull(before, p[i], p[i + 1], after, x - i as f64)
            }
            Curve::Bezier(p) => {
                let segs = (p.len().saturating_sub(1)) / 3;
                if segs == 0 {
                    return p.first().copied().unwrap_or([0.0; 3]);
                }
                let x = t * segs as f64;
                let i = (x.floor() as usize).min(segs - 1);
                let s = x - i as f64;
                let (a, b, c, d) = (p[3 * i], p[3 * i + 1], p[3 * i + 2], p[3 * i + 3]);
                let r = 1.0 - s;
                add(add(mul(a, r * r * r), mul(b, 3.0 * r * r * s)), add(mul(c, 3.0 * r * s * s), mul(d, s * s * s)))
            }
            Curve::Arc { centre, radius, a0, a1, u, v } => {
                let a = a0 + (a1 - a0) * t;
                let (s, c) = dmath::sin_cos(a);
                add(*centre, add(mul(*u, radius * c), mul(*v, radius * s)))
            }
            Curve::Helix { centre, radius, pitch, turns, a0 } => {
                let a = a0 + core::f64::consts::TAU * turns * t;
                let (s, c) = dmath::sin_cos(a);
                add(*centre, [radius * c, pitch * turns * t, radius * s])
            }
            Curve::Catenary { a, b, sag } => {
                let chord = lerp(*a, *b, t);
                let l = dist([a[0], 0.0, a[2]], [b[0], 0.0, b[2]]).max(dist(*a, *b) * 0.5);
                if *sag <= 1e-9 || l <= 1e-9 {
                    return chord;
                }
                let c = catenary_c(l, *sag);
                let drop = c * (cosh(l / (2.0 * c)) - cosh((t - 0.5) * l / c));
                sub(chord, [0.0, drop, 0.0])
            }
        }
    }
    /// A sample count that follows the curve well enough for an arc-length
    /// table (dense; resample for output).
    fn table_samples(&self) -> usize {
        match self {
            Curve::Polyline(p) => p.len().max(2) - 1,
            Curve::CatmullRom(p) => (p.len().max(2) - 1) * 32,
            Curve::Bezier(p) => (p.len().max(4) / 3) * 32,
            _ => 256,
        }
    }
}

/// A point with its orientation along a path: tangent `t`, normal `n` and
/// binormal `b = n x t`, so (`b`, `n`, `t`) is a right-handed basis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub p: V3,
    pub t: V3,
    pub n: V3,
    pub b: V3,
}

impl Frame {
    /// A point in the frame's local axes (`x` along `b`, `y` along `n`,
    /// `z` along `t`).
    pub fn local(&self, x: f64, y: f64, z: f64) -> V3 {
        add(self.p, add(add(mul(self.b, x), mul(self.n, y)), mul(self.t, z)))
    }
    /// The frame's rotation as a quaternion (local z to `t`, local y to `n`).
    pub fn quat(&self) -> Quat {
        mat_to_quat([self.b, self.n, self.t])
    }
}

/// Quaternion of the rotation whose columns are the images of x, y and z.
pub fn mat_to_quat(c: [V3; 3]) -> Quat {
    let (m00, m11, m22) = (c[0][0], c[1][1], c[2][2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(c[1][2] - c[2][1]) / s, (c[2][0] - c[0][2]) / s, (c[0][1] - c[1][0]) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (c[1][0] + c[0][1]) / s, (c[2][0] + c[0][2]) / s, (c[1][2] - c[2][1]) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(c[1][0] + c[0][1]) / s, 0.25 * s, (c[2][1] + c[1][2]) / s, (c[2][0] - c[0][2]) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(c[2][0] + c[0][2]) / s, (c[2][1] + c[1][2]) / s, 0.25 * s, (c[0][1] - c[1][0]) / s]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 1e-300 { [q[0] / l, q[1] / l, q[2] / l, q[3] / l] } else { QID }
}

/// A curve sampled densely with its cumulative arc length.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pts: Vec<V3>,
    cum: Vec<f64>,
}

impl Path {
    pub fn new(c: &Curve) -> Path {
        let n = c.table_samples().max(1);
        Path::from_points((0..=n).map(|i| c.point(i as f64 / n as f64)).collect())
    }
    pub fn from_points(pts: Vec<V3>) -> Path {
        let mut cum = Vec::with_capacity(pts.len());
        let mut s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                s += dist(pts[i - 1], *p);
            }
            cum.push(s);
        }
        Path { pts, cum }
    }
    pub fn length(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }
    /// The point at arc length `s` (clamped).
    pub fn at(&self, s: f64) -> V3 {
        if self.pts.len() < 2 {
            return self.pts.first().copied().unwrap_or([0.0; 3]);
        }
        let s = s.clamp(0.0, self.length());
        let i = match self.cum.binary_search_by(|x| x.total_cmp(&s)) {
            Ok(i) => i.min(self.pts.len() - 2),
            Err(i) => i.saturating_sub(1).min(self.pts.len() - 2),
        };
        let seg = self.cum[i + 1] - self.cum[i];
        let f = if seg > 1e-300 { (s - self.cum[i]) / seg } else { 0.0 };
        lerp(self.pts[i], self.pts[i + 1], f)
    }
    /// The unit tangent at arc length `s` (central difference on the table).
    pub fn tangent(&self, s: f64) -> V3 {
        let h = (self.length() * 1e-4).max(1e-7);
        norm_or(sub(self.at(s + h), self.at(s - h)), [0.0, 0.0, 1.0])
    }
    /// `n + 1` points at equal arc length, ends included.
    pub fn resample(&self, n: usize) -> Vec<V3> {
        let n = n.max(1);
        (0..=n).map(|i| self.at(self.length() * i as f64 / n as f64)).collect()
    }
    /// `n + 1` frames at equal arc length. With `up`, the normal is `up`
    /// made perpendicular to the tangent; without, the first normal is the
    /// axis least aligned with the tangent and later ones follow by parallel
    /// transport (no twist).
    pub fn frames(&self, n: usize, up: Option<V3>) -> Vec<Frame> {
        let n = n.max(1);
        let len = self.length();
        let ss: Vec<f64> = (0..=n).map(|i| len * i as f64 / n as f64).collect();
        let pts: Vec<V3> = ss.iter().map(|s| self.at(*s)).collect();
        let tans: Vec<V3> = ss.iter().map(|s| self.tangent(*s)).collect();
        let mut out = Vec::with_capacity(n + 1);
        let perp = |t: V3, u: V3| -> Option<V3> {
            let v = sub(u, mul(t, dot(u, t)));
            (len_of(v) > 1e-9).then(|| norm_or(v, [0.0, 1.0, 0.0]))
        };
        let first = up.and_then(|u| perp(tans[0], u)).unwrap_or_else(|| {
            let t = tans[0];
            let ax = if t[0].abs() <= t[1].abs() && t[0].abs() <= t[2].abs() {
                [1.0, 0.0, 0.0]
            } else if t[1].abs() <= t[2].abs() {
                [0.0, 1.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            };
            perp(t, ax).unwrap_or([0.0, 1.0, 0.0])
        });
        let mut nrm = first;
        for i in 0..=n {
            let t = tans[i];
            if i > 0 {
                nrm = match up.and_then(|u| perp(t, u)) {
                    Some(v) => v,
                    None => {
                        // Double reflection from frame i-1 to frame i.
                        let v1 = sub(pts[i], pts[i - 1]);
                        let c1 = dot(v1, v1);
                        if c1 < 1e-300 {
                            nrm
                        } else {
                            let rl = sub(nrm, mul(v1, 2.0 / c1 * dot(v1, nrm)));
                            let tl = sub(tans[i - 1], mul(v1, 2.0 / c1 * dot(v1, tans[i - 1])));
                            let v2 = sub(t, tl);
                            let c2 = dot(v2, v2);
                            let r = if c2 < 1e-300 { rl } else { sub(rl, mul(v2, 2.0 / c2 * dot(v2, rl))) };
                            perp(t, r).unwrap_or(nrm)
                        }
                    }
                };
            }
            out.push(Frame { p: pts[i], t, n: nrm, b: norm_or(cross(nrm, t), [1.0, 0.0, 0.0]) });
        }
        out
    }
}

fn len_of(a: V3) -> f64 {
    len(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_hit_their_ends_and_lengths() {
        let q = Curve::Arc { centre: [0.0; 3], radius: 2.0, a0: 0.0, a1: core::f64::consts::PI, u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
        let p = Path::new(&q);
        assert!((p.length() - 2.0 * core::f64::consts::PI).abs() < 1e-3, "{}", p.length());
        assert!(dist(q.point(1.0), [-2.0, 0.0, 0.0]) < 1e-12);
        let cr = Curve::CatmullRom(vec![[0.0; 3], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 0.0]]);
        assert!(dist(cr.point(0.0), [0.0; 3]) < 1e-12 && dist(cr.point(1.0), [3.0, 1.0, 0.0]) < 1e-12);
        assert!(dist(cr.point(1.0 / 3.0), [1.0, 1.0, 0.0]) < 1e-9);
        let bz = Curve::Bezier(vec![[0.0; 3], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
        assert!(dist(bz.point(0.5), [0.5, 0.75, 0.0]) < 1e-12);
        // Catenary: ends fixed, middle sags by `sag`.
        let c = Curve::Catenary { a: [0.0, 1.0, 0.0], b: [2.0, 1.0, 0.0], sag: 0.3 };
        assert!(dist(c.point(0.0), [0.0, 1.0, 0.0]) < 1e-9 && dist(c.point(1.0), [2.0, 1.0, 0.0]) < 1e-9);
        assert!((c.point(0.5)[1] - 0.7).abs() < 1e-6, "{:?}", c.point(0.5));
        let h = Curve::Helix { centre: [0.0; 3], radius: 1.0, pitch: 0.5, turns: 2.0, a0: 0.0 };
        assert!(dist(h.point(1.0), [1.0, 1.0, 0.0]) < 1e-9);
    }

    #[test]
    fn frames_are_orthonormal_and_transport_without_twist() {
        let h = Path::new(&Curve::Helix { centre: [0.0; 3], radius: 1.0, pitch: 0.4, turns: 1.5, a0: 0.0 });
        for up in [None, Some([0.0, 1.0, 0.0])] {
            for f in h.frames(40, up) {
                assert!((len(f.t) - 1.0).abs() < 1e-9 && (len(f.n) - 1.0).abs() < 1e-9 && (len(f.b) - 1.0).abs() < 1e-9);
                assert!(dot(f.t, f.n).abs() < 1e-9 && dot(f.t, f.b).abs() < 1e-9 && dot(f.n, f.b).abs() < 1e-9);
                let q = f.quat();
                assert!(dist(qrot(q, [0.0, 0.0, 1.0]), f.t) < 1e-9 && dist(qrot(q, [0.0, 1.0, 0.0]), f.n) < 1e-9);
            }
        }
        // A straight line keeps its first normal.
        let line = Path::from_points(vec![[0.0; 3], [0.0, 0.0, 5.0]]);
        let fs = line.frames(10, None);
        assert!(fs.iter().all(|f| dist(f.n, fs[0].n) < 1e-12));
    }
}
