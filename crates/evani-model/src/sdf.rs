//! The SDF back end: a tree of field nodes evaluated on the deterministic CPU
//! path with exact gradients (docs/model_research/05-sdf-backend.md).
//!
//! Every node returns a signed distance or a safe bound (never an
//! overestimate: domain operations that stretch space divide by their
//! Lipschitz factor), its gradient by forward-mode derivatives through the
//! node, and the id of the part that owns the nearest surface. Booleans are
//! min and max, so they never fail. Each node also knows a guaranteed
//! bounding box, used for culling.

use crate::dmath;
use crate::vec::*;

/// One field sample: distance (or bound), gradient, owning part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub d: f64,
    pub g: V3,
    pub part: u32,
}

impl Sample {
    fn new(d: f64, g: V3) -> Self {
        Sample { d, g, part: u32::MAX }
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    /// Sphere of radius `r` at the origin (exact).
    Sphere { r: f64 },
    /// Box of half sizes `half` with edges rounded by `r` (exact).
    RoundBox { half: V3, r: f64 },
    /// Cylinder along y: radius `r`, half height `half_h`, edges rounded by `round` (exact).
    Cylinder { r: f64, half_h: f64, round: f64 },
    /// Segment `a`-`b` swept by a sphere of radius `r` (exact).
    Capsule { a: V3, b: V3, r: f64 },
    /// Segment swept by a sphere whose radius goes from `ra` to `rb` (exact outside).
    RoundCone { a: V3, b: V3, ra: f64, rb: f64 },
    /// Torus around y: ring radius `major`, tube radius `minor` (exact).
    Torus { major: f64, minor: f64 },
    /// Ellipsoid with semi-axes `radii` (bound; exact zero set).
    Ellipsoid { radii: V3 },
    /// Half-space `n . p <= off` (exact; `n` unit).
    Plane { n: V3, off: f64 },
    /// Closed 2D profile (radius, height) revolved around y (exact).
    Lathe { profile: Vec<[f64; 2]> },
    /// Hard union.
    Union(Vec<Node>),
    /// Smooth union with blend width `k` metres (quadratic kernel).
    SmoothUnion { k: f64, children: Vec<Node> },
    /// Hard intersection.
    Intersect(Vec<Node>),
    /// `a` minus `b`, smooth when `k > 0`.
    Subtract { a: Box<Node>, b: Box<Node>, k: f64 },
    /// Rigid motion: the child is rotated by `q` then moved by `t`.
    Transform { q: Quat, t: V3, child: Box<Node> },
    /// Mirror across the plane through the origin normal to `axis`.
    Mirror { axis: usize, child: Box<Node> },
    /// The child reflected across the plane normal to `axis` (a mirror image, not a copy).
    Flip { axis: usize, child: Box<Node> },
    /// Inflate the child by `r` (rounding).
    Round { r: f64, child: Box<Node> },
    /// Hollow shell of thickness `t` around the child's surface.
    Shell { t: f64, child: Box<Node> },
    /// Twist about y: `rate` radians per metre.
    Twist { rate: f64, child: Box<Node> },
    /// Bend the child's x axis into an arc of curvature `k` (1/m) towards +y.
    Bend { k: f64, child: Box<Node> },
    /// Scale x and z by `1 + slope * y` (a taper along y).
    Taper { slope: f64, child: Box<Node> },
    /// `count` copies along `axis`, `spacing` apart, starting at the origin.
    Repeat { axis: usize, spacing: f64, count: u32, child: Box<Node> },
    /// Add `amp` times our value noise of frequency `freq` (cycles per metre).
    Noise { amp: f64, freq: f64, seed: u32, child: Box<Node> },
    /// Tag the child's surface with a part id.
    Label { part: u32, child: Box<Node> },
    /// The child with its bounding box and stretch factor computed once (`prepare`).
    Prepared { b: Aabb, lip: f64, child: Box<Node> },
}

fn sgn(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

impl Node {
    pub fn boxed(self) -> Box<Node> {
        Box::new(self)
    }
    pub fn at(self, q: Quat, t: V3) -> Node {
        Node::Transform { q, t, child: Box::new(self) }
    }
    pub fn label(self, part: u32) -> Node {
        Node::Label { part, child: Box::new(self) }
    }

    /// Distance (or safe bound), gradient and owning part at `p`.
    pub fn eval(&self, p: V3) -> Sample {
        match self {
            Node::Sphere { r } => {
                let l = len(p);
                Sample::new(l - r, norm_or(p, [0.0, 1.0, 0.0]))
            }
            Node::RoundBox { half, r } => round_box(p, *half, *r),
            Node::Cylinder { r, half_h, round } => cylinder(p, *r, *half_h, *round),
            Node::Capsule { a, b, r } => {
                let (ab, ap) = (sub(*b, *a), sub(p, *a));
                let den = dot(ab, ab);
                let t = if den > 0.0 { (dot(ap, ab) / den).clamp(0.0, 1.0) } else { 0.0 };
                let v = sub(ap, mul(ab, t));
                Sample::new(len(v) - r, norm_or(v, [0.0, 1.0, 0.0]))
            }
            Node::RoundCone { a, b, ra, rb } => round_cone(p, *a, *b, *ra, *rb),
            Node::Torus { major, minor } => {
                let rho = (p[0] * p[0] + p[2] * p[2]).sqrt();
                let (qx, qy) = (rho - major, p[1]);
                let l = (qx * qx + qy * qy).sqrt();
                let (ux, uz) = if rho > 1e-300 { (p[0] / rho, p[2] / rho) } else { (1.0, 0.0) };
                let (nx, ny) = if l > 1e-300 { (qx / l, qy / l) } else { (1.0, 0.0) };
                Sample::new(l - minor, [nx * ux, ny, nx * uz])
            }
            Node::Ellipsoid { radii } => {
                let q = [p[0] / radii[0], p[1] / radii[1], p[2] / radii[2]];
                let s = len(q);
                let rmin = radii[0].min(radii[1]).min(radii[2]);
                // (s - 1) * rmin has Lipschitz constant <= 1 and the exact zero set.
                let gq = [q[0] / radii[0], q[1] / radii[1], q[2] / radii[2]];
                Sample::new((s - 1.0) * rmin, norm_or(gq, [0.0, 1.0, 0.0]))
            }
            Node::Plane { n, off } => Sample::new(dot(*n, p) - off, *n),
            Node::Lathe { profile } => lathe(p, profile),
            Node::Union(ch) => {
                let mut best: Option<Sample> = None;
                for c in ch {
                    if let Some(b) = best
                        && b.d > 0.0
                        && c.bounds().dist(p) >= b.d
                    {
                        continue;
                    }
                    let s = c.eval(p);
                    if best.is_none_or(|b| s.d < b.d) {
                        best = Some(s);
                    }
                }
                best.unwrap_or(Sample::new(f64::INFINITY, [0.0, 1.0, 0.0]))
            }
            Node::SmoothUnion { k, children } => {
                let mut best: Option<Sample> = None;
                for c in children {
                    if let Some(b) = best
                        && b.d > 0.0
                        && c.bounds().dist(p) >= b.d + k
                    {
                        continue;
                    }
                    let s = c.eval(p);
                    best = Some(match best {
                        None => s,
                        Some(b) => smin(b, s, *k),
                    });
                }
                best.unwrap_or(Sample::new(f64::INFINITY, [0.0, 1.0, 0.0]))
            }
            Node::Intersect(ch) => {
                let mut best: Option<Sample> = None;
                for c in ch {
                    let s = c.eval(p);
                    if best.is_none_or(|b| s.d > b.d) {
                        best = Some(s);
                    }
                }
                best.unwrap_or(Sample::new(f64::INFINITY, [0.0, 1.0, 0.0]))
            }
            Node::Subtract { a, b, k } => {
                let sa = a.eval(p);
                let mut sb = b.eval(p);
                sb.d = -sb.d;
                sb.g = neg(sb.g);
                sb.part = sa.part;
                if *k > 0.0 { smax(sa, sb, *k) } else if sb.d > sa.d { sb } else { sa }
            }
            Node::Transform { q, t, child } => {
                let lp = qrot(qconj(*q), sub(p, *t));
                let mut s = child.eval(lp);
                s.g = qrot(*q, s.g);
                s
            }
            Node::Mirror { axis, child } => {
                let mut lp = p;
                let flip = lp[*axis] < 0.0;
                if flip {
                    lp[*axis] = -lp[*axis];
                }
                let mut s = child.eval(lp);
                if flip {
                    s.g[*axis] = -s.g[*axis];
                }
                s
            }
            Node::Flip { axis, child } => {
                let mut lp = p;
                lp[*axis] = -lp[*axis];
                let mut s = child.eval(lp);
                s.g[*axis] = -s.g[*axis];
                s
            }
            Node::Round { r, child } => {
                let mut s = child.eval(p);
                s.d -= r;
                s
            }
            Node::Shell { t, child } => {
                let mut s = child.eval(p);
                let sg = sgn(s.d);
                s.d = s.d.abs() - t;
                s.g = mul(s.g, sg);
                s
            }
            Node::Twist { .. } | Node::Bend { .. } | Node::Taper { .. } => self.warp_clamped(p, self.bounds(), self.lipschitz_local()),
            Node::Prepared { b, lip, child } => match child.as_ref() {
                Node::Twist { .. } | Node::Bend { .. } | Node::Taper { .. } => child.warp_clamped(p, *b, *lip),
                Node::Noise { amp, freq, seed, child: inner } => {
                    let s = inner.eval(p);
                    let (nv, ng) = value_noise(mul(p, *freq), *seed);
                    let g = add(s.g, mul(ng, amp * freq));
                    Sample { d: (s.d + amp * nv) / lip, g, part: s.part }
                }
                _ => child.eval(p),
            },
            Node::Repeat { axis, spacing, count, child } => {
                let n = (*count).max(1) as f64;
                let c = (p[*axis] / spacing).round().clamp(0.0, n - 1.0);
                let mut best: Option<Sample> = None;
                for off in [-1.0, 0.0, 1.0] {
                    let i = c + off;
                    if i < 0.0 || i > n - 1.0 {
                        continue;
                    }
                    let mut lp = p;
                    lp[*axis] -= i * spacing;
                    let s = child.eval(lp);
                    if best.is_none_or(|b| s.d < b.d) {
                        best = Some(s);
                    }
                }
                best.unwrap_or(Sample::new(f64::INFINITY, [0.0, 1.0, 0.0]))
            }
            Node::Noise { amp, freq, seed, child } => {
                let s = child.eval(p);
                let (nv, ng) = value_noise(mul(p, *freq), *seed);
                let l = self.lipschitz_local();
                let g = add(s.g, mul(ng, amp * freq));
                Sample { d: (s.d + amp * nv) / l, g, part: s.part }
            }
            Node::Label { part, child } => {
                let mut s = child.eval(p);
                s.part = *part;
                s
            }
        }
    }

    /// A stretching domain operation evaluated at the nearest point q of its
    /// box `b`, returning max(|p - q|, f(q)): both are lower bounds of the
    /// true distance (projection onto a convex box never moves a point
    /// further from anything inside it) and both are 1-Lipschitz.
    fn warp_clamped(&self, p: V3, b: Aabb, lip: f64) -> Sample {
        let b = b.grow(DOMAIN_MARGIN);
        let q = [p[0].clamp(b.min[0], b.max[0]), p[1].clamp(b.min[1], b.max[1]), p[2].clamp(b.min[2], b.max[2])];
        let mut s = self.warp_eval(q, lip);
        let out = dist(p, q);
        if out > 0.0 && out > s.d {
            return Sample { d: out, g: norm_or(sub(p, q), s.g), part: s.part };
        }
        // f(clamp(p)) does not change along clamped axes.
        for i in 0..3 {
            if p[i] != q[i] {
                s.g[i] = 0.0;
            }
        }
        s
    }

    /// Wrap every node with its bounding box and stretch factor computed once,
    /// so evaluation never recomputes them (the evaluator's hot path).
    pub fn prepare(self) -> Node {
        let n = match self {
            Node::Prepared { .. } => return self,
            Node::Union(ch) => Node::Union(ch.into_iter().map(Node::prepare).collect()),
            Node::SmoothUnion { k, children } => Node::SmoothUnion { k, children: children.into_iter().map(Node::prepare).collect() },
            Node::Intersect(ch) => Node::Intersect(ch.into_iter().map(Node::prepare).collect()),
            Node::Subtract { a, b, k } => Node::Subtract { a: a.prepare().boxed(), b: b.prepare().boxed(), k },
            Node::Transform { q, t, child } => Node::Transform { q, t, child: child.prepare().boxed() },
            Node::Mirror { axis, child } => Node::Mirror { axis, child: child.prepare().boxed() },
            Node::Flip { axis, child } => Node::Flip { axis, child: child.prepare().boxed() },
            Node::Round { r, child } => Node::Round { r, child: child.prepare().boxed() },
            Node::Shell { t, child } => Node::Shell { t, child: child.prepare().boxed() },
            Node::Twist { rate, child } => Node::Twist { rate, child: child.prepare().boxed() },
            Node::Bend { k, child } => Node::Bend { k, child: child.prepare().boxed() },
            Node::Taper { slope, child } => Node::Taper { slope, child: child.prepare().boxed() },
            Node::Repeat { axis, spacing, count, child } => Node::Repeat { axis, spacing, count, child: child.prepare().boxed() },
            Node::Noise { amp, freq, seed, child } => Node::Noise { amp, freq, seed, child: child.prepare().boxed() },
            Node::Label { part, child } => Node::Label { part, child: child.prepare().boxed() },
            leaf => leaf,
        };
        let b = n.bounds();
        let lip = n.lipschitz_local();
        Node::Prepared { b, lip, child: n.boxed() }
    }

    /// The stretching domain operations at a point inside their box.
    fn warp_eval(&self, p: V3, lip: f64) -> Sample {
        match self {
            Node::Twist { rate, child } => {
                let th = rate * p[1];
                let (sn, cs) = dmath::sin_cos(th);
                let lp = [cs * p[0] + sn * p[2], p[1], -sn * p[0] + cs * p[2]];
                let s = child.eval(lp);
                let g = s.g;
                let gx = cs * g[0] - sn * g[2];
                let gy = rate * (-sn * p[0] + cs * p[2]) * g[0] + g[1] + rate * (-cs * p[0] - sn * p[2]) * g[2];
                let gz = sn * g[0] + cs * g[2];
                let l = lip;
                Sample { d: s.d / l, g: [gx, gy, gz], part: s.part }
            }
            Node::Bend { k, child } => {
                let r = 1.0 / k;
                let v = [p[0], p[1] - r, p[2]];
                let rho2 = v[0] * v[0] + v[1] * v[1];
                let rho = rho2.sqrt();
                let phi = dmath::atan2(v[0], -v[1]);
                let lp = [r * phi, r - rho, p[2]];
                let s = child.eval(lp);
                let g = s.g;
                let (dpx, dpy) = if rho2 > 1e-300 { (-v[1] / rho2, v[0] / rho2) } else { (0.0, 0.0) };
                let (drx, dry) = if rho > 1e-300 { (v[0] / rho, v[1] / rho) } else { (0.0, 0.0) };
                let gx = r * g[0] * dpx - g[1] * drx;
                let gy = r * g[0] * dpy - g[1] * dry;
                let l = lip;
                Sample { d: s.d / l, g: [gx, gy, g[2]], part: s.part }
            }
            Node::Taper { slope, child } => {
                let sc = 1.0 + slope * p[1];
                let lp = [p[0] / sc, p[1], p[2] / sc];
                let s = child.eval(lp);
                let g = s.g;
                let dy = -slope / (sc * sc);
                let gx = g[0] / sc;
                let gz = g[2] / sc;
                let gy = g[1] + dy * (p[0] * g[0] + p[2] * g[2]);
                let l = lip;
                Sample { d: s.d / l, g: [gx, gy, gz], part: s.part }
            }
            _ => self.eval(p),
        }
    }

    /// Just the distance (or bound).
    pub fn dist(&self, p: V3) -> f64 {
        self.eval(p).d
    }

    /// Stretch factor of this node's own domain operation (1 for the rest).
    fn lipschitz_local(&self) -> f64 {
        match self {
            // The stretch is bounded over the node's own (grown) box only;
            // `eval` never applies the map outside that box.
            Node::Twist { rate, .. } => {
                let b = self.bounds().grow(DOMAIN_MARGIN);
                // The map is a rotation times a shear of size m = rate * rho,
                // whose largest singular value is (m + sqrt(m^2 + 4)) / 2.
                let m = rate.abs() * radial_max(&b);
                (m + (m * m + 4.0).sqrt()) * 0.5
            }
            Node::Bend { k, .. } => {
                let b = self.bounds().grow(DOMAIN_MARGIN);
                let r = 1.0 / k.abs();
                // Arc-length stretch r / rho, largest at the smallest radius in the box.
                let rho_low = (r - b.max[1]).max(r * 0.02);
                (r / rho_low).max(1.0)
            }
            Node::Taper { slope, .. } => {
                let b = self.bounds().grow(DOMAIN_MARGIN);
                let s_min = (1.0 + slope * b.min[1]).min(1.0 + slope * b.max[1]).max(0.05);
                let rho = radial_max(&b);
                // Frobenius norm of the Jacobian: a safe bound on its spectral norm.
                let a = 1.0 / s_min;
                let c = rho * slope.abs() / (s_min * s_min);
                (2.0 * a * a + 1.0 + c * c).sqrt()
            }
            Node::Noise { amp, freq, .. } => 1.0 + NOISE_LIPSCHITZ * amp * freq,
            _ => 1.0,
        }
    }

    /// A guaranteed bounding box of the surface (and of the inside).
    pub fn bounds(&self) -> Aabb {
        match self {
            Node::Sphere { r } => Aabb::centred([0.0; 3], [*r; 3]),
            Node::RoundBox { half, .. } => Aabb::centred([0.0; 3], *half),
            Node::Cylinder { r, half_h, .. } => Aabb::centred([0.0; 3], [*r, *half_h, *r]),
            Node::Capsule { a, b, r } => Aabb::new(vmin(*a, *b), vmax(*a, *b)).grow(*r),
            Node::RoundCone { a, b, ra, rb } => {
                Aabb::new(vmin(*a, *b), vmax(*a, *b)).grow(ra.max(*rb))
            }
            Node::Torus { major, minor } => Aabb::centred([0.0; 3], [major + minor, *minor, major + minor]),
            Node::Ellipsoid { radii } => Aabb::centred([0.0; 3], *radii),
            Node::Plane { .. } => Aabb::centred([0.0; 3], [1e6; 3]),
            Node::Lathe { profile } => {
                let (mut rmax, mut ymin, mut ymax) = (0.0f64, f64::INFINITY, f64::NEG_INFINITY);
                for q in profile {
                    rmax = rmax.max(q[0].abs());
                    ymin = ymin.min(q[1]);
                    ymax = ymax.max(q[1]);
                }
                Aabb::new([-rmax, ymin, -rmax], [rmax, ymax, rmax])
            }
            Node::Union(ch) => ch.iter().fold(Aabb::EMPTY, |a, c| a.union(&c.bounds())),
            Node::SmoothUnion { k, children } => {
                children.iter().fold(Aabb::EMPTY, |a, c| a.union(&c.bounds())).grow(k * 0.25)
            }
            Node::Intersect(ch) => {
                ch.iter().fold(Aabb::centred([0.0; 3], [1e6; 3]), |a, c| a.intersect(&c.bounds()))
            }
            Node::Subtract { a, k, .. } => a.bounds().grow(k * 0.25),
            Node::Transform { q, t, child } => child.bounds().transformed(*q, *t),
            Node::Mirror { axis, child } => {
                let b = child.bounds();
                let m = b.min[*axis].abs().max(b.max[*axis].abs());
                let mut out = b;
                out.min[*axis] = -m;
                out.max[*axis] = m;
                // The mirrored field shows the child's x >= 0 half on both sides.
                out
            }
            Node::Flip { axis, child } => {
                let b = child.bounds();
                let mut out = b;
                out.min[*axis] = -b.max[*axis];
                out.max[*axis] = -b.min[*axis];
                out
            }
            Node::Round { r, child } => child.bounds().grow(*r),
            Node::Shell { t, child } => child.bounds().grow(*t),
            Node::Twist { child, .. } => {
                let b = child.bounds();
                let rho = radial_max(&b);
                Aabb::new([-rho, b.min[1], -rho], [rho, b.max[1], rho])
            }
            Node::Bend { k, child } => {
                // World point = (0, r) + rho (sin phi, -cos phi), phi = x' / r, rho = r - y'.
                let b = child.bounds();
                let r = 1.0 / k;
                let rho_max = r - b.min[1];
                let rho_min = (r - b.max[1]).max(0.0);
                let phi = b.min[0].abs().max(b.max[0].abs()) / r;
                if phi >= core::f64::consts::FRAC_PI_2 {
                    Aabb::new([-rho_max, r - rho_max, b.min[2]], [rho_max, r + rho_max, b.max[2]])
                } else {
                    let (sp, cp) = dmath::sin_cos(phi);
                    let xh = rho_max * sp;
                    Aabb::new([-xh, r - rho_max, b.min[2]], [xh, r - rho_min * cp, b.max[2]]).grow(1e-9)
                }
            }
            Node::Taper { slope, child } => {
                let b = child.bounds();
                let s = (1.0 + slope * b.min[1]).abs().max((1.0 + slope * b.max[1]).abs());
                let rx = b.min[0].abs().max(b.max[0].abs()) * s;
                let rz = b.min[2].abs().max(b.max[2].abs()) * s;
                Aabb::new([-rx, b.min[1], -rz], [rx, b.max[1], rz])
            }
            Node::Repeat { axis, spacing, count, child } => {
                let b = child.bounds();
                let mut out = b;
                out.max[*axis] += spacing * ((*count).max(1) - 1) as f64;
                out
            }
            Node::Noise { amp, child, .. } => child.bounds().grow(amp.abs()),
            Node::Label { child, .. } => child.bounds(),
            Node::Prepared { b, .. } => *b,
        }
    }
}

fn radial_max(b: &Aabb) -> f64 {
    let x = b.min[0].abs().max(b.max[0].abs());
    let z = b.min[2].abs().max(b.max[2].abs());
    (x * x + z * z).sqrt()
}

/// Quadratic smooth minimum and its gradient.
fn smin(a: Sample, b: Sample, k: f64) -> Sample {
    if k <= 0.0 {
        return if b.d < a.d { b } else { a };
    }
    let diff = (a.d - b.d).abs();
    let h = ((k - diff) / k).max(0.0);
    let (lo, hi) = if a.d <= b.d { (a, b) } else { (b, a) };
    let d = lo.d - h * h * k * 0.25;
    let w = h * 0.5;
    let g = add(mul(lo.g, 1.0 - w), mul(hi.g, w));
    Sample { d, g, part: lo.part }
}

/// Smooth maximum: -smin(-a, -b).
fn smax(a: Sample, b: Sample, k: f64) -> Sample {
    let na = Sample { d: -a.d, g: neg(a.g), part: a.part };
    let nb = Sample { d: -b.d, g: neg(b.g), part: b.part };
    let s = smin(na, nb, k);
    Sample { d: -s.d, g: neg(s.g), part: a.part }
}

fn round_box(p: V3, half: V3, r: f64) -> Sample {
    let r = r.min(half[0]).min(half[1]).min(half[2]).max(0.0);
    let q = [p[0].abs() - (half[0] - r), p[1].abs() - (half[1] - r), p[2].abs() - (half[2] - r)];
    let qp = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
    let outside = len(qp);
    if outside > 0.0 {
        let g = [qp[0] / outside * sgn(p[0]), qp[1] / outside * sgn(p[1]), qp[2] / outside * sgn(p[2])];
        Sample::new(outside - r, g)
    } else {
        // Inside the core box: nearest face along the largest component.
        let (mut ax, mut m) = (0, q[0]);
        for (i, qi) in q.iter().enumerate().skip(1) {
            if *qi > m {
                m = *qi;
                ax = i;
            }
        }
        let mut g = [0.0; 3];
        g[ax] = sgn(p[ax]);
        Sample::new(m - r, g)
    }
}

fn cylinder(p: V3, r: f64, half_h: f64, round: f64) -> Sample {
    let round = round.min(r).min(half_h).max(0.0);
    let rho = (p[0] * p[0] + p[2] * p[2]).sqrt();
    let (ux, uz) = if rho > 1e-300 { (p[0] / rho, p[2] / rho) } else { (1.0, 0.0) };
    let qx = rho - (r - round);
    let qy = p[1].abs() - (half_h - round);
    let (px, py) = (qx.max(0.0), qy.max(0.0));
    let outside = (px * px + py * py).sqrt();
    if outside > 0.0 {
        let (gx, gy) = (px / outside, py / outside * sgn(p[1]));
        Sample::new(outside - round, [gx * ux, gy, gx * uz])
    } else if qx > qy {
        Sample::new(qx - round, [ux, 0.0, uz])
    } else {
        Sample::new(qy - round, [0.0, sgn(p[1]), 0.0])
    }
}

/// Convex hull of two spheres (a "round cone").
fn round_cone(p: V3, a: V3, b: V3, ra: f64, rb: f64) -> Sample {
    let axis = sub(b, a);
    let l = len(axis);
    if l <= (ra - rb).abs() {
        // One sphere contains the other.
        let (c, r) = if ra >= rb { (a, ra) } else { (b, rb) };
        let v = sub(p, c);
        return Sample::new(len(v) - r, norm_or(v, [0.0, 1.0, 0.0]));
    }
    let ax = mul(axis, 1.0 / l);
    let ap = sub(p, a);
    let u = dot(ap, ax);
    let radial = sub(ap, mul(ax, u));
    let v = len(radial);
    let rdir = norm_or(radial, any_perp(ax));
    let sa = (ra - rb) / l;
    let ca = (1.0 - sa * sa).sqrt();
    // In the (u, v) plane: side normal n = (sa, ca), side tangent t = (ca, -sa).
    let s = u * ca - v * sa;
    if s < 0.0 {
        let w = ap;
        return Sample::new(len(w) - ra, norm_or(w, rdir));
    }
    if s > l * ca {
        let w = sub(p, b);
        return Sample::new(len(w) - rb, norm_or(w, rdir));
    }
    let d = u * sa + v * ca - ra;
    Sample::new(d, add(mul(ax, sa), mul(rdir, ca)))
}

fn any_perp(a: V3) -> V3 {
    let t = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    norm_or(cross(a, t), [0.0, 0.0, 1.0])
}

/// Signed distance to a closed 2D polygon (radius, height), revolved around y.
fn lathe(p: V3, profile: &[[f64; 2]]) -> Sample {
    let rho = (p[0] * p[0] + p[2] * p[2]).sqrt();
    let (ux, uz) = if rho > 1e-300 { (p[0] / rho, p[2] / rho) } else { (1.0, 0.0) };
    let q = [rho, p[1]];
    let n = profile.len();
    if n < 3 {
        return Sample::new(f64::INFINITY, [0.0, 1.0, 0.0]);
    }
    let mut best = f64::INFINITY;
    let mut bv = [0.0, 0.0];
    let mut inside = false;
    for i in 0..n {
        let a = profile[i];
        let b = profile[(i + 1) % n];
        // Crossing number for the sign (every edge counts).
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if q[0] < x {
                inside = !inside;
            }
        }
        // An edge lying on the axis is not a surface once revolved.
        if a[0] == 0.0 && b[0] == 0.0 {
            continue;
        }
        let e = [b[0] - a[0], b[1] - a[1]];
        let w = [q[0] - a[0], q[1] - a[1]];
        let den = e[0] * e[0] + e[1] * e[1];
        let t = if den > 0.0 { ((w[0] * e[0] + w[1] * e[1]) / den).clamp(0.0, 1.0) } else { 0.0 };
        let v = [w[0] - e[0] * t, w[1] - e[1] * t];
        let d2 = v[0] * v[0] + v[1] * v[1];
        if d2 < best {
            best = d2;
            bv = v;
        }
    }
    let d = best.sqrt();
    let s = if inside { -1.0 } else { 1.0 };
    let (gx, gy) = if d > 1e-300 { (bv[0] / d * s, bv[1] / d * s) } else { (1.0, 0.0) };
    Sample::new(d * s, [gx * ux, gy, gx * uz])
}

/// Margin around a stretching node's box inside which its map is applied.
const DOMAIN_MARGIN: f64 = 1e-3;

/// Bound on the gradient length of `value_noise` (per unit of its input).
pub const NOISE_LIPSCHITZ: f64 = 6.5;

fn lattice(ix: i64, iy: i64, iz: i64, seed: u32) -> f64 {
    // Integer mixing of the lattice coordinates (our own constants).
    let mut h = (ix as u64).wrapping_mul(0x9E37_79B1_85EB_CA87)
        ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (iz as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ (seed as u64).wrapping_mul(0x27D4_EB2F_1656_67C5);
    h ^= h >> 31;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 29;
    ((h >> 11) as f64) * (2.0 / 9_007_199_254_740_992.0) - 1.0
}

/// Value noise in [-1, 1] with a quintic fade, and its gradient.
pub fn value_noise(p: V3, seed: u32) -> (f64, V3) {
    let f = [p[0].floor(), p[1].floor(), p[2].floor()];
    let t = sub(p, f);
    let fade = |x: f64| x * x * x * (x * (x * 6.0 - 15.0) + 10.0);
    let dfade = |x: f64| 30.0 * x * x * (x * (x - 2.0) + 1.0);
    let u = [fade(t[0]), fade(t[1]), fade(t[2])];
    let du = [dfade(t[0]), dfade(t[1]), dfade(t[2])];
    let (ix, iy, iz) = (f[0] as i64, f[1] as i64, f[2] as i64);
    let c = |dx: i64, dy: i64, dz: i64| lattice(ix + dx, iy + dy, iz + dz, seed);
    let (c000, c100, c010, c110) = (c(0, 0, 0), c(1, 0, 0), c(0, 1, 0), c(1, 1, 0));
    let (c001, c101, c011, c111) = (c(0, 0, 1), c(1, 0, 1), c(0, 1, 1), c(1, 1, 1));
    let x00 = c000 + (c100 - c000) * u[0];
    let x10 = c010 + (c110 - c010) * u[0];
    let x01 = c001 + (c101 - c001) * u[0];
    let x11 = c011 + (c111 - c011) * u[0];
    let y0 = x00 + (x10 - x00) * u[1];
    let y1 = x01 + (x11 - x01) * u[1];
    let v = y0 + (y1 - y0) * u[2];
    // d/dx
    let dx00 = c100 - c000;
    let dx10 = c110 - c010;
    let dx01 = c101 - c001;
    let dx11 = c111 - c011;
    let dy0x = dx00 + (dx10 - dx00) * u[1];
    let dy1x = dx01 + (dx11 - dx01) * u[1];
    let gx = (dy0x + (dy1x - dy0x) * u[2]) * du[0];
    let gy = ((x10 - x00) + ((x11 - x01) - (x10 - x00)) * u[2]) * du[1];
    let gz = (y1 - y0) * du[2];
    (v, [gx, gy, gz])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fd_grad(n: &Node, p: V3) -> V3 {
        let h = 1e-6;
        let mut g = [0.0; 3];
        for i in 0..3 {
            let (mut a, mut b) = (p, p);
            a[i] += h;
            b[i] -= h;
            g[i] = (n.dist(a) - n.dist(b)) / (2.0 * h);
        }
        g
    }

    fn pts() -> Vec<V3> {
        let mut r = crate::rng::Pcg32::new(7, 1);
        (0..400).map(|_| [r.range(-1.5, 1.5), r.range(-1.5, 1.5), r.range(-1.5, 1.5)]).collect()
    }

    fn shapes() -> Vec<(&'static str, Node)> {
        vec![
            ("sphere", Node::Sphere { r: 0.7 }),
            ("round_box", Node::RoundBox { half: [0.6, 0.3, 0.4], r: 0.1 }),
            ("cylinder", Node::Cylinder { r: 0.5, half_h: 0.3, round: 0.05 }),
            ("capsule", Node::Capsule { a: [-0.4, 0.0, 0.1], b: [0.5, 0.3, -0.2], r: 0.2 }),
            ("round_cone", Node::RoundCone { a: [0.0, -0.5, 0.0], b: [0.1, 0.6, 0.0], ra: 0.4, rb: 0.15 }),
            ("torus", Node::Torus { major: 0.6, minor: 0.15 }),
            ("lathe", Node::Lathe { profile: vec![[0.0, -0.5], [0.4, -0.5], [0.5, 0.0], [0.35, 0.5], [0.0, 0.5]] }),
            (
                "transform",
                Node::RoundBox { half: [0.5, 0.2, 0.3], r: 0.05 }.at(quat_ypr(0.3, 0.2, -0.4), [0.1, 0.2, -0.1]),
            ),
        ]
    }

    /// Exact primitives: sampled distance equals the true distance, checked
    /// by the Lipschitz property both ways on a dense set: |d(p) - d(q)| <= |p - q|
    /// and the gradient is a unit vector matching finite differences.
    #[test]
    fn exact_primitives_have_unit_gradients_matching_finite_differences() {
        for (name, n) in shapes() {
            for p in pts() {
                let s = n.eval(p);
                let fd = fd_grad(&n, p);
                // Skip points on a medial ridge where the gradient jumps.
                if (len(fd) - 1.0).abs() > 1e-3 {
                    continue;
                }
                assert!((len(s.g) - 1.0).abs() < 1e-9, "{name} gradient length at {p:?}");
                assert!(len(sub(s.g, fd)) < 1e-4, "{name} gradient {:?} vs fd {fd:?} at {p:?}", s.g);
            }
        }
    }

    #[test]
    fn exact_distances_for_simple_primitives() {
        let b = Node::RoundBox { half: [1.0, 0.5, 0.25], r: 0.0 };
        assert!((b.dist([2.0, 0.0, 0.0]) - 1.0).abs() < 1e-15);
        assert!((b.dist([2.0, 1.5, 0.0]) - 2f64.sqrt()).abs() < 1e-15);
        assert!((b.dist([0.0, 0.0, 0.0]) + 0.25).abs() < 1e-15);
        let c = Node::Cylinder { r: 0.5, half_h: 1.0, round: 0.0 };
        assert!((c.dist([1.5, 0.0, 0.0]) - 1.0).abs() < 1e-15);
        assert!((c.dist([0.0, 2.0, 0.0]) - 1.0).abs() < 1e-15);
        let t = Node::Torus { major: 1.0, minor: 0.25 };
        assert!((t.dist([1.0, 0.0, 0.0]) + 0.25).abs() < 1e-15);
        let l = Node::Lathe { profile: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 2.0], [0.0, 2.0]] };
        assert!((l.dist([0.0, 1.0, 1.5]) - 0.5).abs() < 1e-15);
        assert!((l.dist([0.0, 1.0, 0.5]) + 0.5).abs() < 1e-15);
    }

    /// Sampled Lipschitz property: no node, including the stretching domain
    /// operations, ever changes faster than distance.
    #[test]
    fn lipschitz_bound_never_violated() {
        let base = Node::RoundBox { half: [0.5, 0.6, 0.2], r: 0.05 };
        let nodes = vec![
            Node::Twist { rate: 1.5, child: base.clone().boxed() },
            Node::Bend { k: 0.8, child: base.clone().boxed() },
            Node::Taper { slope: 0.5, child: base.clone().boxed() },
            Node::Noise { amp: 0.05, freq: 3.0, seed: 9, child: Node::Sphere { r: 0.6 }.boxed() },
            Node::Ellipsoid { radii: [0.8, 0.3, 0.5] },
            Node::SmoothUnion { k: 0.2, children: vec![base.clone(), Node::Sphere { r: 0.4 }.at(QID, [0.5, 0.3, 0.0])] },
            Node::Repeat { axis: 0, spacing: 0.4, count: 4, child: Node::Sphere { r: 0.15 }.boxed() },
            Node::Mirror { axis: 0, child: Node::Sphere { r: 0.3 }.at(QID, [0.5, 0.0, 0.0]).boxed() },
        ];
        let p = pts();
        for n in &nodes {
            for i in 0..p.len() {
                for j in (i + 1)..p.len().min(i + 40) {
                    let (a, b) = (p[i], p[j]);
                    let dd = (n.dist(a) - n.dist(b)).abs();
                    assert!(dd <= dist(a, b) * (1.0 + 1e-9), "{n:?} violates Lipschitz");
                }
            }
        }
    }

    #[test]
    fn domain_op_gradients_match_finite_differences() {
        let base = Node::RoundBox { half: [0.5, 0.6, 0.2], r: 0.05 };
        let nodes = vec![
            Node::Twist { rate: 1.5, child: base.clone().boxed() },
            Node::Bend { k: 0.8, child: base.clone().boxed() },
            Node::Taper { slope: 0.5, child: base.clone().boxed() },
            Node::Noise { amp: 0.05, freq: 3.0, seed: 9, child: Node::Sphere { r: 0.6 }.boxed() },
        ];
        for n in &nodes {
            let mut checked = 0;
            for p in pts() {
                let s = n.eval(p);
                let fd = fd_grad(n, p);
                // Gradients are of the unscaled field; compare directions.
                let (a, b) = (norm_or(s.g, [0.0; 3]), norm_or(fd, [0.0; 3]));
                if len(fd) < 1e-6 {
                    continue;
                }
                if len(sub(a, b)) < 1e-3 {
                    checked += 1;
                } else {
                    // Allowed only next to a gradient discontinuity of the child.
                    let h = 1e-3;
                    let jump = len(sub(norm_or(fd_grad(n, add(p, [h, 0.0, 0.0])), [0.0; 3]), b));
                    assert!(jump > 1e-3, "{n:?} gradient {a:?} vs fd {b:?} at {p:?}");
                }
            }
            assert!(checked > 300, "{n:?}: only {checked} points checked");
        }
    }

    #[test]
    fn booleans_and_labels() {
        let a = Node::Sphere { r: 0.5 }.label(1);
        let b = Node::Sphere { r: 0.5 }.at(QID, [0.8, 0.0, 0.0]).label(2);
        let u = Node::Union(vec![a.clone(), b.clone()]);
        assert_eq!(u.eval([-0.6, 0.0, 0.0]).part, 1);
        assert_eq!(u.eval([1.4, 0.0, 0.0]).part, 2);
        let s = Node::SmoothUnion { k: 0.3, children: vec![a.clone(), b.clone()] };
        assert!(s.dist([0.4, 0.45, 0.0]) < u.dist([0.4, 0.45, 0.0]));
        let d = Node::Subtract { a: a.clone().boxed(), b: b.clone().boxed(), k: 0.0 };
        assert!(d.dist([0.45, 0.0, 0.0]) > 0.0);
        assert!(d.dist([-0.45, 0.0, 0.0]) < 0.0);
        let i = Node::Intersect(vec![a, b]);
        assert!(i.dist([0.4, 0.0, 0.0]) < 0.0);
        assert!(i.dist([-0.4, 0.0, 0.0]) > 0.0);
    }

    #[test]
    fn bounds_contain_the_surface() {
        for (name, n) in shapes() {
            let b = n.bounds();
            for p in pts() {
                if n.dist(p) <= 0.0 {
                    assert!(b.grow(1e-9).contains(p), "{name}: inside point {p:?} outside bounds {b:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod prepared_tests {
    use super::*;

    #[test]
    fn prepared_fields_give_identical_samples() {
        let base = Node::RoundBox { half: [0.5, 0.6, 0.2], r: 0.05 };
        let tree = Node::SmoothUnion {
            k: 0.1,
            children: vec![
                Node::Twist { rate: 1.5, child: base.clone().boxed() }.label(1),
                Node::Bend { k: 0.8, child: base.clone().boxed() }.at(QID, [0.3, 0.0, 0.0]).label(2),
                Node::Noise { amp: 0.05, freq: 3.0, seed: 9, child: Node::Sphere { r: 0.6 }.boxed() }.label(3),
                Node::Mirror { axis: 0, child: Node::Taper { slope: 0.5, child: base.boxed() }.at(QID, [0.6, 0.0, 0.0]).boxed() },
            ],
        };
        let prepared = tree.clone().prepare();
        let mut r = crate::rng::Pcg32::new(3, 3);
        for _ in 0..2000 {
            let p = [r.range(-2.0, 2.0), r.range(-2.0, 2.0), r.range(-2.0, 2.0)];
            assert_eq!(tree.eval(p), prepared.eval(p));
        }
    }
}
