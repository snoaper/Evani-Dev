//! Small f64 vector, quaternion and box helpers for the deterministic CPU path.
//!
//! Only IEEE 754 basic operations (add, sub, mul, div, sqrt) are used here,
//! each in a fixed order, so results are bit-identical on every platform.
//! Rust never fuses a multiply and an add on its own, and we never call
//! `mul_add`.

pub type V3 = [f64; 3];

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
pub fn hadamard(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
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
pub fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
#[inline]
pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
/// Unit vector, or `fallback` for a (near) zero vector.
#[inline]
pub fn norm_or(a: V3, fallback: V3) -> V3 {
    let l = len(a);
    if l > 1e-300 { mul(a, 1.0 / l) } else { fallback }
}
#[inline]
pub fn dist(a: V3, b: V3) -> f64 {
    len(sub(a, b))
}
#[inline]
pub fn vmin(a: V3, b: V3) -> V3 {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])]
}
#[inline]
pub fn vmax(a: V3, b: V3) -> V3 {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]
}

/// Unit quaternion `[x, y, z, w]`.
pub type Quat = [f64; 4];
pub const QID: Quat = [0.0, 0.0, 0.0, 1.0];

/// Rotation of `angle` radians about the unit `axis` (deterministic sin/cos).
pub fn quat_axis(axis: V3, angle: f64) -> Quat {
    let a = norm_or(axis, [0.0, 1.0, 0.0]);
    let (s, c) = (crate::dmath::sin(angle * 0.5), crate::dmath::cos(angle * 0.5));
    [a[0] * s, a[1] * s, a[2] * s, c]
}
/// `a * b`: apply `b` first, then `a`.
pub fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub fn qconj(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}
/// Rotate `v` by the unit quaternion `q`.
pub fn qrot(q: Quat, v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let t = mul(cross(u, v), 2.0);
    add(add(v, mul(t, q[3])), cross(u, t))
}
/// Euler rotation: yaw about y, then pitch about x, then roll about z
/// (applied to the part in the order roll, pitch, yaw). Angles in radians.
pub fn quat_ypr(yaw: f64, pitch: f64, roll: f64) -> Quat {
    let qy = quat_axis([0.0, 1.0, 0.0], yaw);
    let qx = quat_axis([1.0, 0.0, 0.0], pitch);
    let qz = quat_axis([0.0, 0.0, 1.0], roll);
    qmul(qy, qmul(qx, qz))
}

/// Axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}
impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] };
    pub fn new(min: V3, max: V3) -> Self {
        Aabb { min, max }
    }
    pub fn centred(c: V3, half: V3) -> Self {
        Aabb { min: sub(c, half), max: add(c, half) }
    }
    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: vmin(self.min, o.min), max: vmax(self.max, o.max) }
    }
    pub fn intersect(&self, o: &Aabb) -> Aabb {
        Aabb { min: vmax(self.min, o.min), max: vmin(self.max, o.max) }
    }
    pub fn grow(&self, m: f64) -> Aabb {
        Aabb { min: sub(self.min, [m; 3]), max: add(self.max, [m; 3]) }
    }
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }
    pub fn size(&self) -> V3 {
        sub(self.max, self.min)
    }
    pub fn centre(&self) -> V3 {
        mul(add(self.min, self.max), 0.5)
    }
    pub fn add_point(&mut self, p: V3) {
        self.min = vmin(self.min, p);
        self.max = vmax(self.max, p);
    }
    /// Distance from `p` to the box (0 inside).
    pub fn dist(&self, p: V3) -> f64 {
        let mut s = 0.0;
        for i in 0..3 {
            let d = (self.min[i] - p[i]).max(p[i] - self.max[i]).max(0.0);
            s += d * d;
        }
        s.sqrt()
    }
    pub fn contains(&self, p: V3) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
    pub fn overlaps(&self, o: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] && o.min[i] <= self.max[i])
    }
    /// Box of this box after the rigid motion (rotation `q`, then `t`).
    pub fn transformed(&self, q: Quat, t: V3) -> Aabb {
        if self.is_empty() {
            return *self;
        }
        let c = self.centre();
        let h = mul(self.size(), 0.5);
        let nc = add(qrot(q, c), t);
        let ax = [qrot(q, [1.0, 0.0, 0.0]), qrot(q, [0.0, 1.0, 0.0]), qrot(q, [0.0, 0.0, 1.0])];
        let mut nh = [0.0; 3];
        for (i, nhi) in nh.iter_mut().enumerate() {
            *nhi = ax[0][i].abs() * h[0] + ax[1][i].abs() * h[1] + ax[2][i].abs() * h[2];
        }
        Aabb::centred(nc, nh)
    }
}
