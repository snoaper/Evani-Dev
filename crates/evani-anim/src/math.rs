//! Small scalar maths for poses: vectors and quaternions as plain arrays,
//! one local transform type and a 3x4 affine matrix. Only `+ - * /` and
//! `sqrt` (IEEE on every platform, wasm included): no `sin`, `cos` or `acos`
//! from a platform maths library, so the same input gives the same bits on
//! every machine.

/// `[x, y, z]`.
pub type Vec3 = [f32; 3];
/// A quaternion `[x, y, z, w]`.
pub type Quat = [f32; 4];
/// An affine matrix as its top three rows, row-major: `[m00 m01 m02 tx, m10
/// m11 m12 ty, m20 m21 m22 tz]`, 48 bytes. In WGSL these are the three
/// columns of a `mat3x4<f32>` used as `vec4(p, 1.0) * m`.
pub type Mat43 = [f32; 12];

/// The identity rotation.
pub const QID: Quat = [0.0, 0.0, 0.0, 1.0];
/// The identity matrix.
pub const MID: Mat43 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

const EPS: f32 = 1e-12;

/// A local transform: translation, rotation, scale (`T * R * S`, glTF's order).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub t: Vec3,
    pub r: Quat,
    pub s: Vec3,
}

impl Xform {
    pub const IDENTITY: Xform = Xform { t: [0.0; 3], r: QID, s: [1.0; 3] };

    /// `T * R * S` as a matrix (the same terms as glTF's node matrix).
    pub fn matrix(&self) -> Mat43 {
        let [x, y, z, w] = self.r;
        let [sx, sy, sz] = self.s;
        let [tx, ty, tz] = self.t;
        [
            (1.0 - 2.0 * (y * y + z * z)) * sx,
            (2.0 * (x * y - z * w)) * sy,
            (2.0 * (x * z + y * w)) * sz,
            tx,
            (2.0 * (x * y + z * w)) * sx,
            (1.0 - 2.0 * (x * x + z * z)) * sy,
            (2.0 * (y * z - x * w)) * sz,
            ty,
            (2.0 * (x * z - y * w)) * sx,
            (2.0 * (y * z + x * w)) * sy,
            (1.0 - 2.0 * (x * x + y * y)) * sz,
            tz,
        ]
    }
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    core::array::from_fn(|i| a[i] + b[i])
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    core::array::from_fn(|i| a[i] - b[i])
}

pub fn scale(a: Vec3, k: f32) -> Vec3 {
    a.map(|v| v * k)
}

pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

/// `a` at unit length, or `None` when it has (almost) no length.
pub fn unit(a: Vec3) -> Option<Vec3> {
    let l = length(a);
    if l > 1e-6 && l.is_finite() { Some(scale(a, 1.0 / l)) } else { None }
}

/// Some unit vector at right angles to `v` (which need not be unit length).
pub fn perpendicular(v: Vec3) -> Vec3 {
    let other = if v[0].abs() < 0.9 * length(v) { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    unit(cross(v, other)).unwrap_or([0.0, 0.0, 1.0])
}

/// `q` at unit length (the identity for a zero or broken quaternion). The
/// same steps as the reference reader's `normalize`, so samples match it
/// bit for bit.
pub fn qnorm(q: Quat) -> Quat {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 0.0 && l.is_finite() { q.map(|c| c / l) } else { QID }
}

/// `a * b` (apply `b`, then `a`).
pub fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The inverse of a unit quaternion.
pub fn conj(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

/// `v` rotated by the unit quaternion `q`.
pub fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let u = [q[0], q[1], q[2]];
    let t = scale(cross(u, v), 2.0);
    add(add(v, scale(t, q[3])), cross(u, t))
}

/// Normalised lerp on the shorter arc: the second rotation is negated when
/// the dot product is below 0. The same steps as the reference reader.
pub fn nlerp(a: Quat, b: Quat, u: f32) -> Quat {
    qnorm(mix4(a, b, u, true))
}

/// `a + (b - a) * u`, with `b` negated first when `shortest` and the dot
/// product is below 0: [`lerp4`] and the inside of [`nlerp`] in one body
/// (the sampler uses both; `b * 1.0` is `b` exactly, so the bits match).
pub(crate) fn mix4(a: [f32; 4], b: [f32; 4], u: f32, shortest: bool) -> [f32; 4] {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let s = if shortest && dot < 0.0 { -1.0 } else { 1.0 };
    core::array::from_fn(|i| a[i] + (b[i] * s - a[i]) * u)
}

/// `a + (b - a) * u`, per component.
pub fn lerp4(a: [f32; 4], b: [f32; 4], u: f32) -> [f32; 4] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
}

pub fn lerp3(a: Vec3, b: Vec3, u: f32) -> Vec3 {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
}

/// The shortest rotation that turns direction `u` into direction `v`
/// (neither needs unit length). Opposite directions turn half way round an
/// axis at right angles to `u`: `about` when it is one, else any.
pub fn from_to(u: Vec3, v: Vec3, about: Vec3) -> Quat {
    let k = (dot(u, u) * dot(v, v)).sqrt();
    if k.is_nan() || k <= EPS {
        return QID;
    }
    let d = dot(u, v);
    if d + k <= 1e-6 * k {
        let a = unit(sub(about, scale(u, dot(about, u) / dot(u, u)))).unwrap_or_else(|| perpendicular(u));
        return [a[0], a[1], a[2], 0.0];
    }
    let c = cross(u, v);
    qnorm([c[0], c[1], c[2], d + k])
}

/// `a * b` for affine matrices (apply `b`, then `a`).
pub fn mmul(a: &Mat43, b: &Mat43) -> Mat43 {
    let mut r = [0.0; 12];
    for i in 0..3 {
        let (a0, a1, a2, a3) = (a[i * 4], a[i * 4 + 1], a[i * 4 + 2], a[i * 4 + 3]);
        for j in 0..4 {
            r[i * 4 + j] = a0 * b[j] + a1 * b[4 + j] + a2 * b[8 + j];
        }
        r[i * 4 + 3] += a3;
    }
    r
}

/// A point transformed by `m`.
pub fn mpoint(m: &Mat43, p: Vec3) -> Vec3 {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3],
        m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7],
        m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11],
    ]
}

/// The translation of `m`.
pub fn mpos(m: &Mat43) -> Vec3 {
    [m[3], m[7], m[11]]
}

/// A glTF column-major 4x4 (an inverse bind matrix) as a [`Mat43`]; its
/// bottom row is dropped (affine).
pub fn from_cols(c: &[f32; 16]) -> Mat43 {
    [c[0], c[4], c[8], c[12], c[1], c[5], c[9], c[13], c[2], c[6], c[10], c[14]]
}
