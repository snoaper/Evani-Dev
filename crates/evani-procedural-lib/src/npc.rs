//! Full NPC body variations (the Asset Studio full NPC body variations
//! session, column 22, 2026-11-07): a line-up of barefoot adults of
//! different builds in grey tank tops and shorts, each turned three
//! quarters to the right.
//!
//! Each person is one smooth sculpted body: a signed distance field of
//! anatomical forms (ribcage, pectorals or breasts, belly, pelvis, glutes,
//! deltoids, biceps, triceps, forearms, quadriceps, hamstrings, calves,
//! the skull and face, hands with fingers, feet with toes) blended with
//! smooth unions, so shoulders run into arms and calves into ankles as a
//! body does. The field is meshed by surface nets (only near the surface,
//! the head, hands and feet at twice the density) with normals from the
//! field's gradient. Garments are the body's own field grown outward and
//! cut by hem, neckline and armhole shapes (a racer tank, a men's tank, a
//! crew tee, bike shorts and boxer shorts), hair and beards the head's
//! field grown and cut at a hairline, curls a little noise on it.
//!
//! The builds are six archetypes (a petite woman, a tall muscular man, a
//! tall slim woman, a heavy bearded man, a curvy woman, a tall lean man in
//! a tee), shuffled and varied by the seed as `variety` grows. All shapes
//! are written fresh; the smooth union and round cone distances are the
//! well-known closed forms (Inigo Quilez's articles, MIT-style public
//! formulas), the mesher the classic naive surface nets idea (Gibson 1998).

use evani_procedural::geo::{self, MeshBuilder};
use evani_procedural::surface::presets as sp;
use evani_procedural::{Part, Values, shapes as sh};

type Mesh = geo::Mesh;
type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-12 {
        mul(a, 1.0 / l)
    } else {
        [0.0, 1.0, 0.0]
    }
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn lerp(a: V3, b: V3, t: f32) -> V3 {
    add(a, mul(sub(b, a), t))
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    lerp(a, b, t)
}
fn srgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    let f = |c: u8| {
        let s = c as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(r), f(g), f(b)]
}

/// Smooth minimum (polynomial, blend width `k`): exactly `min` once the two
/// differ by `k` or more.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}
fn smax(a: f32, b: f32, k: f32) -> f32 {
    -smin(-a, -b, k)
}

// ---- the field's shapes ---------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Shape {
    /// An ellipsoid round `c`, half sizes `r` along its own axes `ax`.
    Ell { c: V3, r: V3, ax: [V3; 3] },
    /// A round cone from `a` (radius `ra`) to `b` (radius `rb`).
    Cone { a: V3, b: V3, ra: f32, rb: f32 },
    /// An eyelid: the ball round `c` of radius `r`, kept where
    /// `dot(p - c, n) < d` (the lid's margin is that plane's arc on the ball).
    Lid { c: V3, r: f32, n: V3, d: f32 },
}

fn round_cone(p: V3, a: V3, b: V3, r1: f32, r2: f32) -> f32 {
    let ba = sub(b, a);
    let l2 = dot(ba, ba);
    if l2 < 1e-12 {
        return len(sub(p, a)) - r1.max(r2);
    }
    let rr = r1 - r2;
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = sub(p, a);
    let y = dot(pa, ba);
    let z = y - l2;
    let xv = sub(mul(pa, l2), mul(ba, y));
    let x2 = dot(xv, xv);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        return (x2 + z2).sqrt() * il2 - r2;
    }
    if y.signum() * a2 * y2 < k {
        return (x2 + y2).sqrt() * il2 - r1;
    }
    ((x2 * a2 * il2).max(0.0).sqrt() + y * rr) * il2 - r1
}

impl Shape {
    fn dist(&self, p: V3) -> f32 {
        match *self {
            Shape::Ell { c, r, ax } => {
                let d = sub(p, c);
                let q = [
                    dot(d, ax[0]) / r[0],
                    dot(d, ax[1]) / r[1],
                    dot(d, ax[2]) / r[2],
                ];
                let k0 = len(q);
                let k1 = len([q[0] / r[0], q[1] / r[1], q[2] / r[2]]);
                let approx = if k1 < 1e-9 {
                    -r[0].min(r[1]).min(r[2])
                } else {
                    k0 * (k0 - 1.0) / k1
                };
                // The estimate undercuts a flat ellipsoid's true distance;
                // never below the bounding sphere's (true for any point, and
                // it keeps the bounds' skip in `eval` exact).
                approx.max(len(d) - r[0].max(r[1]).max(r[2]))
            }
            Shape::Cone { a, b, ra, rb } => round_cone(p, a, b, ra, rb),
            Shape::Lid { c, r, n, d } => {
                let v = sub(p, c);
                (len(v) - r).max(dot(v, n) - d)
            }
        }
    }

    fn bounds(&self) -> (V3, V3) {
        match *self {
            Shape::Ell { c, r, .. } => {
                let m = r[0].max(r[1]).max(r[2]);
                (sub(c, [m; 3]), add(c, [m; 3]))
            }
            Shape::Lid { c, r, .. } => (sub(c, [r; 3]), add(c, [r; 3])),
            Shape::Cone { a, b, ra, rb } => {
                let lo = [
                    (a[0] - ra).min(b[0] - rb),
                    (a[1] - ra).min(b[1] - rb),
                    (a[2] - ra).min(b[2] - rb),
                ];
                let hi = [
                    (a[0] + ra).max(b[0] + rb),
                    (a[1] + ra).max(b[1] + rb),
                    (a[2] + ra).max(b[2] + rb),
                ];
                (lo, hi)
            }
        }
    }
}

/// Which part of the body a form belongs to (garments and hair read only
/// some groups).
mod grp {
    pub const TRUNK: u16 = 1;
    pub const NECK: u16 = 2;
    pub const ARM_UP: u16 = 4;
    pub const ARM_LO: u16 = 8;
    pub const HAND: u16 = 16;
    pub const PELVIS: u16 = 32;
    pub const THIGH: u16 = 64;
    pub const SHIN: u16 = 128;
    pub const FOOT: u16 = 256;
    pub const HEAD: u16 = 512;
    /// The right side's limbs (left: without it).
    pub const RIGHT: u16 = 1 << 15;
    pub const ALL: u16 = 0x7fff;
    /// The limbs: each side blends with the trunk, never with the other.
    pub const SIDED: u16 = ARM_UP | ARM_LO | HAND | THIGH | SHIN | FOOT;
}

#[derive(Debug, Clone, Copy)]
struct Prim {
    s: Shape,
    k: f32,
    cut: bool,
    g: u16,
    lo: V3,
    hi: V3,
    /// A turn about a vertical axis (cos, sin, the axis's x and z): the
    /// head's forms turn with the head; the query point is turned back.
    rot: [f32; 4],
}

/// No turn.
const NO_TURN: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

/// `p` turned back by the turn `r` (into the turned form's own frame).
fn unturn(p: V3, r: [f32; 4]) -> V3 {
    let (dx, dz) = (p[0] - r[2], p[2] - r[3]);
    [r[2] + dx * r[0] - dz * r[1], p[1], r[3] + dx * r[1] + dz * r[0]]
}

/// `p` turned by the turn `r` (out of the turned form's frame).
fn turn_by(p: V3, r: [f32; 4]) -> V3 {
    let (dx, dz) = (p[0] - r[2], p[2] - r[3]);
    [r[2] + dx * r[0] + dz * r[1], p[1], r[3] - dx * r[1] + dz * r[0]]
}

/// Distance from `p` to the box `lo..hi` (0 inside).
fn box_dist(p: V3, lo: V3, hi: V3) -> f32 {
    let d = [
        (lo[0] - p[0]).max(p[0] - hi[0]).max(0.0),
        (lo[1] - p[1]).max(p[1] - hi[1]).max(0.0),
        (lo[2] - p[2]).max(p[2] - hi[2]).max(0.0),
    ];
    len(d)
}

/// The field of the forms in `groups` (and of the right or left side only
/// when `side` is 1 or -1), every blend widened by `kmul`. The trunk's
/// forms come first; each side's limbs then blend into a copy of the
/// trunk's field, so the two legs (and an arm and the other side) never
/// melt into each other.
fn eval(ps: &[Prim], p: V3, groups: u16, side: i32, kmul: f32) -> f32 {
    let mut centre = 1.0e3f32;
    let mut sides = [f32::NAN; 2];
    for q in ps {
        if q.g & groups == 0 {
            continue;
        }
        let right = q.g & grp::RIGHT != 0;
        if side != 0 && (side > 0) != right {
            continue;
        }
        let d = if q.g & grp::SIDED != 0 {
            let s = &mut sides[right as usize];
            if s.is_nan() {
                *s = centre;
            }
            s
        } else {
            &mut centre
        };
        let k = q.k * kmul;
        let pp = if q.rot[1] != 0.0 { unturn(p, q.rot) } else { p };
        let lb = box_dist(pp, q.lo, q.hi);
        if q.cut {
            if lb >= k - *d {
                continue;
            }
            *d = smax(*d, -q.s.dist(pp), k);
        } else {
            if lb >= *d + k {
                continue;
            }
            *d = smin(*d, q.s.dist(pp), k);
        }
    }
    centre.min(sides[0]).min(sides[1])
}

/// Axes (rows) with y along `dir` and z towards `fwd`.
fn frame(dir: V3, fwd: V3) -> [V3; 3] {
    let ey = norm(dir);
    let mut ez = sub(fwd, mul(ey, dot(fwd, ey)));
    if len(ez) < 1e-6 {
        ez = cross([1.0, 0.0, 0.0], ey);
    }
    let ez = norm(ez);
    let ex = cross(ey, ez);
    [ex, ey, ez]
}

const AXES: [V3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Collects the forms of one person, every size a share of its height.
struct Forms {
    ps: Vec<Prim>,
    h: f32,
    g: u16,
    rot: [f32; 4],
}

impl Forms {
    fn push(&mut self, s: Shape, k: f32, cut: bool) {
        let (lo, hi) = s.bounds();
        self.ps.push(Prim {
            s,
            k: k * self.h,
            cut,
            g: self.g,
            lo,
            hi,
            rot: self.rot,
        });
    }
    /// An upright ellipsoid (shares of the height).
    fn ell(&mut self, c: V3, r: V3, k: f32) {
        let (c, r) = (mul(c, self.h), mul(r, self.h));
        if r[0] > 1e-5 && r[1] > 1e-5 && r[2] > 1e-5 {
            self.push(Shape::Ell { c, r, ax: AXES }, k, false);
        }
    }
    /// An ellipsoid on axes `ax` (shares of the height).
    fn ell_ax(&mut self, c: V3, r: V3, ax: [V3; 3], k: f32) {
        let (c, r) = (mul(c, self.h), mul(r, self.h));
        if r[0] > 1e-5 && r[1] > 1e-5 && r[2] > 1e-5 {
            self.push(Shape::Ell { c, r, ax }, k, false);
        }
    }
    /// An eyelid round the eye `c` (shares): the ball of radius `r` kept
    /// where `dot(p - c, n) < d` (shares).
    fn lid(&mut self, c: V3, r: f32, n: V3, d: f32, k: f32) {
        let h = self.h;
        let n = norm(n);
        self.push(
            Shape::Lid {
                c: mul(c, h),
                r: r * h,
                n,
                d: d * h,
            },
            k,
            false,
        );
    }
    fn cut_ell(&mut self, c: V3, r: V3, ax: [V3; 3], k: f32) {
        let (c, r) = (mul(c, self.h), mul(r, self.h));
        self.push(Shape::Ell { c, r, ax }, k, true);
    }
    fn cone(&mut self, a: V3, b: V3, ra: f32, rb: f32, k: f32) {
        let h = self.h;
        self.push(
            Shape::Cone {
                a: mul(a, h),
                b: mul(b, h),
                ra: ra * h,
                rb: rb * h,
            },
            k,
            false,
        );
    }
    /// A muscle: an ellipsoid a share `t` of the way along the bone `(a, b)`,
    /// moved by `off` (shares), half sizes (across, along, depth) on the
    /// bone's axes with depth towards `fwd`.
    fn along(&mut self, (a, b): (V3, V3), t: f32, off: V3, r: V3, fwd: V3, k: f32) {
        let ax = frame(sub(b, a), fwd);
        self.ell_ax(add(lerp(a, b, t), off), r, ax, k);
    }
}

// ---- the people ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hair {
    /// Pulled back into a small bun at the back of the crown.
    Bun,
    /// Close-cropped.
    Crop,
    /// Pulled back into a loose low bun.
    LooseBun,
    /// Short and curly all over.
    Curly,
    /// Curly, gathered into a high bun.
    TopBun,
    /// Medium length, swept back in waves.
    Swept,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Top {
    /// A women's racer tank: thin straps, a deep scoop.
    Racer,
    /// A men's tank: wider straps, deep armholes.
    Tank,
    /// A crew-neck tee with short sleeves.
    Tee,
}

#[derive(Debug, Clone, Copy)]
struct Person {
    h: f32,
    /// 0 male .. 1 female forms.
    fem: f32,
    /// Muscle 0 .. 1.
    mus: f32,
    /// Body fat 0 .. 1.
    fat: f32,
    bust: f32,
    skin: [f32; 3],
    hair: Hair,
    hair_rgb: [f32; 3],
    /// Beard thickness (share of the height; 0 none).
    beard: f32,
    top: Top,
    /// Where the shorts' legs end (share of the height).
    hem: f32,
    /// Where the top's hem lies and the shorts' waistband's top (shares).
    top_hem: f32,
    rise: f32,
    /// The shoulders' half width beyond the build's own (share).
    shoulders: f32,
    /// The head's turn to the right beyond the body's (degrees).
    look: f32,
    /// The top's strap width at the shoulder (share of the height; 0 a tee).
    strap: f32,
    /// How low the top's front neckline reaches (share of the height).
    scoop: f32,
    jaw: f32,
    nose: f32,
    lips: f32,
    iris: [f32; 3],
}

/// The six builds of the reference line-up, left to right.
fn archetypes() -> [Person; 6] {
    let black = srgb(18, 14, 12);
    [
        Person {
            h: 1.6,
            fem: 1.0,
            mus: 0.35,
            fat: 0.12,
            bust: 0.45,
            skin: srgb(233, 195, 169),
            hair: Hair::Bun,
            hair_rgb: black,
            beard: 0.0,
            top: Top::Racer,
            hem: 0.445,
            top_hem: 0.586,
            rise: 0.6,
            shoulders: 0.0,
            look: 12.0,
            strap: 0.009,
            scoop: 0.768,
            jaw: 0.92,
            nose: 0.75,
            lips: 0.9,
            iris: srgb(40, 26, 18),
        },
        Person {
            h: 1.91,
            fem: 0.0,
            mus: 1.0,
            fat: 0.05,
            bust: 0.0,
            skin: srgb(175, 135, 114),
            hair: Hair::Crop,
            hair_rgb: srgb(14, 11, 10),
            beard: 0.0035,
            top: Top::Tank,
            hem: 0.41,
            top_hem: 0.54,
            rise: 0.582,
            shoulders: -0.006,
            look: 14.0,
            strap: 0.026,
            scoop: 0.786,
            jaw: 1.1,
            nose: 1.05,
            lips: 1.15,
            iris: srgb(30, 20, 14),
        },
        Person {
            h: 1.78,
            fem: 1.0,
            mus: 0.1,
            fat: 0.05,
            bust: 0.42,
            skin: srgb(219, 183, 165),
            hair: Hair::LooseBun,
            hair_rgb: srgb(196, 164, 116),
            beard: 0.0,
            top: Top::Racer,
            hem: 0.442,
            top_hem: 0.552,
            rise: 0.592,
            shoulders: 0.0,
            look: 20.0,
            strap: 0.008,
            scoop: 0.764,
            jaw: 0.9,
            nose: 0.95,
            lips: 0.95,
            iris: srgb(70, 96, 112),
        },
        Person {
            h: 1.76,
            fem: 0.0,
            mus: 0.35,
            fat: 0.95,
            bust: 0.0,
            skin: srgb(220, 183, 167),
            hair: Hair::Curly,
            hair_rgb: srgb(74, 52, 36),
            beard: 0.0078,
            top: Top::Tank,
            hem: 0.395,
            top_hem: 0.52,
            rise: 0.575,
            shoulders: -0.004,
            look: 14.0,
            strap: 0.012,
            scoop: 0.752,
            jaw: 1.18,
            nose: 1.0,
            lips: 1.0,
            iris: srgb(60, 48, 34),
        },
        Person {
            h: 1.66,
            fem: 1.0,
            mus: 0.45,
            fat: 0.42,
            bust: 1.0,
            skin: srgb(182, 137, 114),
            hair: Hair::TopBun,
            hair_rgb: black,
            beard: 0.0,
            top: Top::Racer,
            hem: 0.425,
            top_hem: 0.522,
            rise: 0.585,
            shoulders: 0.007,
            look: 10.0,
            strap: 0.011,
            scoop: 0.756,
            jaw: 0.98,
            nose: 0.9,
            lips: 1.1,
            iris: srgb(34, 22, 14),
        },
        Person {
            h: 1.86,
            fem: 0.0,
            mus: 0.6,
            fat: 0.04,
            bust: 0.0,
            skin: srgb(225, 186, 164),
            hair: Hair::Swept,
            hair_rgb: srgb(42, 30, 24),
            beard: 0.0,
            top: Top::Tee,
            hem: 0.405,
            top_hem: 0.535,
            rise: 0.578,
            shoulders: -0.005,
            look: 16.0,
            strap: 0.0,
            scoop: 0.83,
            jaw: 1.0,
            nose: 1.0,
            lips: 0.95,
            iris: srgb(48, 34, 24),
        },
    ]
}

/// One person's joints (shares of the height), standing on y = 0, facing
/// +z, its left +x.
#[derive(Debug, Clone, Copy)]
struct Joints {
    sh: [V3; 2],
    el: [V3; 2],
    wr: [V3; 2],
    /// The forearm's direction (down the hand).
    fo: [V3; 2],
    hip: [V3; 2],
    kn: [V3; 2],
    an: [V3; 2],
    /// The foot's forward direction.
    toe: [V3; 2],
    head: V3,
    sw: f32,
}

fn joints(p: &Person) -> Joints {
    let (fem, m, f) = (p.fem, p.mus, p.fat);
    // Round 5: the shoulders set lower and in (the reference's silhouettes,
    // measured row by row: the neck runs clear down to about 0.84 of the
    // height and the shoulders' width there was 1.25 to 1.4 times too much).
    let sw = 0.096 - 0.012 * fem + 0.006 * m + 0.004 * m * (1.0 - fem) + 0.006 * f + p.shoulders;
    let hw = 0.049 + 0.006 * fem + 0.008 * f;
    let out = (10.0 + 8.0 * f + 2.0 * m).to_radians();
    let mut j = Joints {
        sh: [[0.0; 3]; 2],
        el: [[0.0; 3]; 2],
        wr: [[0.0; 3]; 2],
        fo: [[0.0; 3]; 2],
        hip: [[0.0; 3]; 2],
        kn: [[0.0; 3]; 2],
        an: [[0.0; 3]; 2],
        toe: [[0.0; 3]; 2],
        head: [0.0, 0.935, 0.004],
        sw,
    };
    for (i, s) in [(0usize, 1.0f32), (1, -1.0)] {
        let sh_ = [s * sw, 0.802 - 0.006 * fem - 0.012 * f, -0.01];
        let up = norm([s * out.sin(), -out.cos(), 0.05]);
        let el = add(sh_, mul(up, 0.18));
        let fo = norm([s * (out * 0.75).sin(), -1.0, 0.2]);
        let wr = add(el, mul(fo, 0.148));
        j.sh[i] = sh_;
        j.el[i] = el;
        j.wr[i] = wr;
        j.fo[i] = fo;
        j.hip[i] = [s * hw, 0.515, 0.0];
        j.kn[i] = [s * (hw + 0.008), 0.285, 0.008];
        j.an[i] = [s * (hw + 0.022), 0.046, -0.004];
        let a = 9f32.to_radians();
        j.toe[i] = [s * a.sin(), 0.0, a.cos()];
    }
    j
}

/// Every form of one body.
fn body_forms(p: &Person, j: &Joints) -> Vec<Prim> {
    use grp::*;
    let (fem, m, f) = (p.fem, p.mus, p.fat);
    let male = 1.0 - fem;
    let mut b = Forms {
        ps: Vec::with_capacity(180),
        h: p.h,
        g: TRUNK,
        rot: NO_TURN,
    };
    // ---- trunk
    b.ell(
        [0.0, 0.705, -0.008],
        [
            0.082 + 0.01 * m + 0.03 * f - 0.008 * fem,
            0.1,
            0.057 + 0.006 * m + 0.028 * f - 0.004 * fem,
        ],
        0.03,
    );
    b.ell(
        [0.0, 0.718, -0.034],
        [
            0.082 - 0.006 * fem + 0.016 * m * male + 0.02 * f,
            0.082,
            0.044 + 0.008 * f,
        ],
        0.03,
    );
    // Pectorals (men and the muscular), breasts (women).
    let pec = male * (0.6 + 0.6 * m) + fem * 0.3;
    for s in [1.0f32, -1.0] {
        b.ell(
            [s * 0.043, 0.736 - 0.012 * f, 0.026 + 0.008 * m + 0.016 * f],
            [
                0.044 + 0.008 * m + 0.008 * f,
                0.032 + 0.004 * m + 0.01 * f,
                (0.016 + 0.01 * m + 0.02 * f) * pec,
            ],
            0.018 + 0.014 * f,
        );
        if fem * p.bust > 0.05 {
            let bu = p.bust;
            let ax = frame([s * 0.15, 1.0, -0.25], [s * 0.25, 0.0, 1.0]);
            b.ell_ax(
                [s * 0.047, 0.708 - 0.006 * bu, 0.04 + 0.008 * bu + 0.01 * f],
                [0.034 + 0.008 * bu, 0.032 + 0.006 * bu, 0.022 + 0.014 * bu],
                ax,
                0.016,
            );
        }
        // The trapezius: a thin sheet sloping from the back of the neck
        // down to the shoulder (round 5: lower and thinner, so the neck
        // stands clear above it as in the reference; thick and high only
        // on the muscular).
        b.cone(
            [0.0, 0.846 + 0.008 * m * male - 0.012 * f, -0.03],
            [s * (j.sw - 0.016), 0.808 - 0.012 * f, -0.02],
            0.019 + 0.01 * m * male + 0.004 * f,
            0.017 + 0.004 * m,
            0.014,
        );
        // A muscular man's trapezius stands up the sides of his neck.
        if m * male > 0.2 {
            let t = m * male;
            b.ell(
                [s * 0.042, 0.83, -0.026],
                [0.032 * t, 0.02 * t, 0.024 * t],
                0.016,
            );
        }
    }
    // Waist and abdomen; the belly with body fat.
    b.ell(
        [0.0, 0.615, 0.0],
        [
            0.068 + 0.004 * m + 0.036 * f - 0.006 * fem,
            0.075,
            0.052 + 0.034 * f,
        ],
        0.032,
    );
    if f > 0.15 {
        b.ell(
            [0.0, 0.575 - 0.016 * f, 0.03 + 0.048 * f],
            [0.062 + 0.042 * f, 0.062 + 0.04 * f, 0.03 + 0.05 * f],
            0.045,
        );
        // Love handles.
        for s in [1.0f32, -1.0] {
            b.ell(
                [s * (0.07 + 0.03 * f), 0.585, -0.005],
                [0.03 * f, 0.04 * f, 0.045 * f],
                0.03,
            );
        }
    }
    // ---- pelvis and glutes
    b.g = PELVIS;
    b.ell(
        [0.0, 0.525, -0.004],
        [
            0.086 + 0.018 * fem + 0.022 * f,
            0.065,
            0.058 + 0.012 * f + 0.006 * fem,
        ],
        0.03,
    );
    b.ell(
        [0.0, 0.5, 0.028],
        [0.06 + 0.01 * f, 0.042, 0.03 + 0.012 * f],
        0.03,
    );
    b.ell([0.0, 0.476, -0.002], [0.034, 0.03, 0.04], 0.025);
    for s in [1.0f32, -1.0] {
        b.ell(
            [s * 0.042, 0.494, -0.036 - 0.004 * fem],
            [
                0.048 + 0.008 * fem + 0.01 * f,
                0.052 + 0.006 * fem,
                0.032 + 0.008 * fem + 0.012 * f,
            ],
            0.022,
        );
        if fem > 0.0 {
            b.ell(
                [s * (0.08 + 0.012 * f), 0.49, -0.006],
                [0.022 * fem + 0.012 * f, 0.06, 0.042],
                0.03,
            );
        }
    }
    // ---- neck
    b.g = NECK;
    // Round 5: slimmer (the reference's necks measure about 0.058 of the
    // height across on the women, 0.06 to 0.075 on the men) with a tighter
    // blend, leaning forward a little from the shoulders to the skull.
    b.cone(
        [0.0, 0.8, -0.022],
        [0.0, 0.9, -0.004],
        0.03 + 0.004 * male + 0.007 * m * male + 0.012 * f,
        0.027 + 0.003 * male + 0.004 * m * male + 0.008 * f,
        0.012,
    );
    // The sternocleidomastoids from behind the ears down to the notch
    // between the collarbones: set into the neck, a few millimetres proud
    // (a soft V at the front of the neck, not cords).
    for s in [1.0f32, -1.0] {
        b.cone(
            [s * 0.021, 0.902, -0.004],
            [s * 0.008, 0.814, 0.004],
            0.006 + 0.002 * m * male,
            0.0055,
            0.012,
        );
    }
    // The upper chest under the collarbones, so the neck's base runs down
    // into the chest without a hollow.
    b.g = TRUNK;
    b.ell(
        [0.0, 0.778, 0.0],
        [0.058 + 0.01 * m * male + 0.01 * f, 0.03, 0.032 + 0.006 * f],
        0.02,
    );
    b.g = NECK;
    // ---- head (a centre form: before the limbs)
    // Round 5: the head turned a little further right than the body, as
    // every head in the reference is (looking past the camera).
    b.rot = head_turn(p);
    head_forms(&mut b, p, j.head);
    b.rot = NO_TURN;
    // ---- legs
    let fwd = [0.0, 0.0, 1.0];
    for (i, s) in [(0usize, 1.0f32), (1, -1.0)] {
        let side = if s < 0.0 { RIGHT } else { 0 };
        let (hp, kn, an) = (j.hip[i], j.kn[i], j.an[i]);
        // Round 5: every leg form re-sized against the reference's
        // silhouettes (knees 0.055 to 0.07 of the height across on the slim
        // builds, 0.08 on the heavy man; the round-4 knees measured 0.088
        // on all six), with tighter blends: the old wide blends swelled
        // every joint into a soft bulge.
        b.g = THIGH | side;
        b.cone(
            add(hp, [s * 0.012, 0.008, 0.0]),
            kn,
            0.05 + 0.007 * fem + 0.02 * f + 0.004 * m,
            0.026 + 0.006 * f + 0.003 * m,
            0.02,
        );
        b.along(
            (hp, kn),
            0.5,
            [s * 0.004, 0.0, 0.013],
            [
                (0.04 + 0.008 * m + 0.008 * f) * (1.0 - 0.12 * fem),
                0.115,
                (0.034 + 0.008 * m + 0.006 * f) * (1.0 - 0.12 * fem),
            ],
            fwd,
            0.02,
        );
        // The vastus medialis: the teardrop above the knee's inner side.
        b.along(
            (hp, kn),
            0.78,
            [-s * 0.014, 0.0, 0.008],
            [0.017 + 0.006 * m, 0.036, 0.015 + 0.004 * m],
            fwd,
            0.012,
        );
        b.along(
            (hp, kn),
            0.25,
            [-s * 0.012, 0.0, -0.002],
            [0.029 + 0.012 * f, 0.085, 0.038 + 0.006 * f],
            fwd,
            0.022,
        );
        b.along(
            (hp, kn),
            0.45,
            [0.0, 0.0, -0.02],
            [0.036 + 0.004 * m, 0.11, 0.03 + 0.004 * m + 0.006 * f],
            fwd,
            0.025,
        );
        // The knee: the condyles (wider than deep), the kneecap in front and
        // the tendon below it; a flat plane each side, not a ball.
        b.ell(
            add(kn, [0.0, 0.0, 0.002]),
            [0.02 + 0.012 * f + 0.002 * m, 0.022, 0.019 + 0.008 * f],
            0.014,
        );
        b.ell(add(kn, [0.0, 0.006, 0.019]), [0.0095, 0.012, 0.0065], 0.008);
        b.cone(
            add(kn, [0.0, -0.006, 0.019]),
            add(kn, [0.0, -0.032, 0.016]),
            0.005,
            0.0055,
            0.008,
        );
        b.g = SHIN | side;
        b.cone(kn, an, 0.021 + 0.005 * f + 0.002 * m, 0.0135 + 0.002 * f, 0.016);
        // The calf: the gastrocnemius's two heads (the inner one fuller and
        // lower), the soleus under them, sized by the build.
        b.along(
            (kn, an),
            0.28,
            [s * 0.006, 0.0, -0.017],
            [
                0.0175 + 0.009 * m + 0.022 * f,
                0.075,
                0.0195 + 0.008 * m + 0.014 * f,
            ],
            fwd,
            0.016,
        );
        b.along(
            (kn, an),
            0.33,
            [-s * 0.007, 0.0, -0.015],
            [
                0.0185 + 0.009 * m + 0.022 * f,
                0.07,
                0.0195 + 0.008 * m + 0.014 * f,
            ],
            fwd,
            0.016,
        );
        b.along(
            (kn, an),
            0.55,
            [0.0, 0.0, -0.01],
            [0.0185 + 0.004 * m + 0.014 * f, 0.09, 0.0165 + 0.008 * f],
            fwd,
            0.016,
        );
        b.ell(an, [0.0145, 0.018, 0.017], 0.012);
        // The ankle bones (the inner one higher and further forward) and
        // the Achilles tendon from the calf down to the heel.
        b.ell(
            add(an, [-s * 0.0105, 0.003, 0.002]),
            [0.0065, 0.008, 0.0065],
            0.008,
        );
        b.ell(
            add(an, [s * 0.0115, -0.003, -0.003]),
            [0.006, 0.008, 0.006],
            0.008,
        );
        b.cone(
            add(lerp(kn, an, 0.62), [0.0, 0.0, -0.022]),
            add(an, [0.0, -0.01, -0.024]),
            0.009,
            0.0065,
            0.014,
        );
        // ---- foot: heel, sole, ball and five toes.
        b.g = FOOT | side;
        let d = j.toe[i];
        let lat = [d[2] * s, 0.0, -d[0] * s];
        let ax = [lat, [0.0, 1.0, 0.0], d];
        let base = [an[0], 0.0, an[2]];
        // Round 5: the foot 0.152 of the height from heel to big toe (it
        // measured 0.179: long flat paddles), a high instep along the inner
        // side and the arch lifted off the floor there.
        b.ell_ax(
            add(base, [0.0, 0.02, -0.017]),
            [0.017, 0.02, 0.021],
            ax,
            0.01,
        );
        b.ell_ax(
            add(add(base, mul(d, 0.036)), [0.0, 0.021, 0.0]),
            [0.022, 0.019, 0.048],
            ax,
            0.012,
        );
        b.ell_ax(
            add(add(base, add(mul(d, 0.03), mul(lat, -0.006))), [0.0, 0.029, 0.0]),
            [0.013, 0.014, 0.034],
            ax,
            0.01,
        );
        b.ell_ax(
            add(
                add(base, add(mul(d, 0.079), mul(lat, -0.002))),
                [0.0, 0.0135, 0.0],
            ),
            [0.0265, 0.0135, 0.02],
            ax,
            0.01,
        );
        let toes: [(f32, f32, f32); 5] = [
            (-0.0145, 0.026, 0.0066),
            (-0.003, 0.021, 0.0044),
            (0.0062, 0.018, 0.0041),
            (0.0142, 0.0155, 0.0038),
            (0.0215, 0.012, 0.0036),
        ];
        for (k, (o, l, r)) in toes.into_iter().enumerate() {
            let back = if k == 0 { 0.0 } else { 0.0035 * k as f32 };
            let a = add(
                add(base, add(mul(d, 0.088 - back), mul(lat, o))),
                [0.0, r + 0.002, 0.0],
            );
            let tip = add(add(a, mul(d, l)), mul(lat, o * 0.12));
            b.cone(a, [tip[0], r * 0.85, tip[2]], r, r * 0.85, 0.004);
        }
        // ---- arm
        let (sh_, el, wr, fo) = (j.sh[i], j.el[i], j.wr[i], j.fo[i]);
        b.g = ARM_UP | side;
        let slim = 1.0 - 0.18 * fem;
        // Round 5: the arm re-sized against the reference (its upper arms
        // measure 0.044 to 0.057 of the height across, the round-4 arms 0.06
        // to 0.07 with a cap-like deltoid): a deltoid that tapers to its
        // insertion halfway down the upper arm, slimmer biceps and triceps,
        // an elbow as wide as the arm (no pinch), tighter blends.
        b.along(
            (sh_, el),
            0.25,
            [s * 0.004, 0.0, 0.0],
            [
                (0.02 + 0.013 * m + 0.012 * f) * slim,
                0.058 + 0.006 * m,
                (0.025 + 0.01 * m + 0.01 * f) * slim,
            ],
            fwd,
            0.022,
        );
        b.cone(
            sh_,
            el,
            (0.025 + 0.006 * m + 0.016 * f) * slim.sqrt(),
            0.0175 + 0.002 * m + 0.005 * f,
            0.016,
        );
        b.along(
            (sh_, el),
            0.55,
            [0.0, 0.0, 0.009],
            [
                (0.017 + 0.012 * m + 0.008 * f) * slim,
                0.055,
                (0.016 + 0.012 * m + 0.008 * f) * slim,
            ],
            fwd,
            0.012,
        );
        b.along(
            (sh_, el),
            0.42,
            [s * 0.002, 0.0, -0.011],
            [
                (0.019 + 0.01 * m + 0.014 * f) * slim.sqrt(),
                0.065,
                (0.017 + 0.009 * m + 0.012 * f) * slim.sqrt(),
            ],
            fwd,
            0.012,
        );
        b.ell(el, [0.0155 + 0.002 * m, 0.016, 0.014], 0.014);
        b.ell(add(el, [0.0, 0.004, -0.011]), [0.007, 0.011, 0.006], 0.008);
        b.g = ARM_LO | side;
        b.cone(
            el,
            wr,
            (0.018 + 0.006 * m + 0.006 * f) * slim.sqrt(),
            0.0135 + 0.001 * m + 0.002 * f,
            0.014,
        );
        b.along(
            (el, wr),
            0.27,
            [s * 0.004, 0.0, 0.004],
            [
                (0.02 + 0.009 * m + 0.006 * f) * slim,
                0.06,
                (0.017 + 0.007 * m + 0.004 * f) * slim,
            ],
            fwd,
            0.014,
        );
        // ---- hand: the palm faces the thigh.
        b.g = HAND | side;
        let hax = frame(fo, [0.0, 0.0, 1.0]);
        let (ex, ey, ez) = (hax[0], hax[1], hax[2]);
        let inward = mul(ex, s);
        b.ell_ax(wr, [0.011, 0.012, 0.016], hax, 0.01);
        b.ell_ax(add(wr, mul(ey, 0.031)), [0.0085, 0.031, 0.023], hax, 0.008);
        b.ell_ax(
            add(
                add(add(wr, mul(ey, 0.022)), mul(ez, 0.016)),
                mul(inward, 0.004),
            ),
            [0.011, 0.02, 0.01],
            hax,
            0.008,
        );
        // Fingers: index (front) to little, curling a little towards the palm.
        let fingers: [(f32, f32); 4] = [
            (0.0155, 0.042),
            (0.0052, 0.047),
            (-0.0052, 0.044),
            (-0.0148, 0.035),
        ];
        for (k, (zo, l)) in fingers.into_iter().enumerate() {
            let r0 = 0.0047 - 0.0003 * k as f32;
            let mut at = add(add(wr, mul(ey, 0.06)), mul(ez, zo));
            let mut dir = norm(add(ey, mul(ez, zo * 1.2)));
            let mut r = r0;
            for (seg, (share, bend)) in [(0.45f32, 0.16f32), (0.3, 0.3), (0.25, 0.22)]
                .into_iter()
                .enumerate()
            {
                dir = norm(add(mul(dir, bend.cos()), mul(inward, bend.sin())));
                let next = add(at, mul(dir, l * share));
                let rn = r * if seg == 2 { 0.86 } else { 0.92 };
                b.cone(at, next, r, rn, 0.0025);
                at = next;
                r = rn;
            }
        }
        // The thumb, down and forward along the index finger.
        let mut at = add(
            add(add(wr, mul(ey, 0.016)), mul(ez, 0.02)),
            mul(inward, 0.006),
        );
        let mut dir = norm(add(add(ey, mul(ez, 0.55)), mul(inward, 0.25)));
        for (share, r0, r1) in [(0.026f32, 0.0062f32, 0.0056f32), (0.022, 0.0056, 0.005)] {
            let next = add(at, mul(dir, share));
            b.cone(at, next, r0, r1, 0.003);
            at = next;
            dir = norm(add(dir, mul(inward, 0.25)));
        }
    }
    b.ps
}

/// The eye's centre from the head's centre (shares; x mirrored per side)
/// and the eyeball's radius: set back in the orbit, the cornea about level
/// with the cheek below it and well behind the brow and the nose's root.
const EYE: V3 = [0.0178, -0.0015, 0.0405];
const EYE_R: f32 = 0.0068;

/// The head's turn beyond the body's (about a vertical axis just behind
/// the head's centre, metres).
fn head_turn(p: &Person) -> [f32; 4] {
    let a = p.look.to_radians();
    let hc = joints(p).head;
    [a.cos(), a.sin(), 0.0, (hc[2] - 0.01) * p.h]
}

/// `m` turned with the head.
fn turned(m: &Mesh, r: [f32; 4]) -> Mesh {
    let mut c = m.clone();
    for v in &mut c.vertices {
        let q = turn_by([v[0], v[1], v[2]], r);
        let n = turn_by([v[3] + r[2], v[4], v[5] + r[3]], r);
        *v = [q[0], q[1], q[2], n[0] - r[2], n[1], n[2] - r[3]];
    }
    c
}

/// The head and face (offsets from the head's centre `hc`, shares).
fn head_forms(b: &mut Forms, p: &Person, hc: V3) {
    b.g = grp::HEAD;
    let at = |o: V3| add(hc, o);
    let jaw = p.jaw;
    let male = 1.0 - p.fem;
    // The cranium, the forehead and the face's planes, blended wide so
    // cheek, jaw and chin run into each other.
    b.ell(at([0.0, 0.012, -0.012]), [0.0435, 0.053, 0.055], 0.01);
    b.ell(at([0.0, 0.02, 0.018]), [0.037, 0.034, 0.031], 0.014);
    b.ell(at([0.0, -0.018, 0.022]), [0.031, 0.03, 0.03], 0.014);
    // Round 5: the lower face narrowed to real breadths (the jaw's angles
    // were 0.083 of the height apart, a real jaw about 0.06; the cheekbones
    // 0.087, real about 0.074), so the face tapers from the cheekbones
    // down the jaw line to a defined chin instead of a box.
    b.ell(at([0.0, -0.035, 0.013]), [0.0285 * jaw, 0.025, 0.035], 0.014);
    b.ell(at([0.0, -0.0545, 0.04]), [0.0125 * jaw, 0.0115, 0.0115], 0.012);
    // Under the chin, curving into the neck.
    b.ell(at([0.0, -0.0525, 0.018]), [0.018 * jaw, 0.009, 0.02], 0.012);
    for s in [1.0f32, -1.0] {
        // The jaw's angles, below and in front of the ears; the jaw line
        // runs from them to the chin as one edge.
        b.ell(
            at([s * 0.0225 * jaw, -0.043, -0.008]),
            [0.0095, 0.012, 0.016],
            0.014,
        );
        b.cone(
            at([s * 0.0225 * jaw, -0.047, -0.004]),
            at([s * 0.0085, -0.058, 0.034]),
            0.0045,
            0.0045,
            0.012,
        );
        // Cheekbones (the zygomatic arch running back to the ear), the
        // cheeks' soft fullness under them, and the fat pad beside the
        // mouth that makes the fold from the nose's wing to the mouth's
        // corner.
        b.ell(at([s * 0.0245, -0.006, 0.031]), [0.0125, 0.0095, 0.011], 0.014);
        b.cone(
            at([s * 0.03, -0.004, 0.024]),
            at([s * 0.038, -0.004, -0.004]),
            0.0045,
            0.004,
            0.008,
        );
        b.ell(at([s * 0.02, -0.021, 0.027]), [0.011, 0.014, 0.012], 0.014);
        b.ell(at([s * 0.0135, -0.026, 0.039]), [0.0055, 0.009, 0.005], 0.01);
        // The temple: a shallow hollow above the arch.
        b.cut_ell(at([s * 0.043, 0.012, 0.012]), [0.004, 0.012, 0.012], AXES, 0.01);
        // Ears: thin and close to the head, set back at its middle depth.
        let eax = frame([0.0, 1.0, -0.22], [s * 0.35, 0.0, 1.0]);
        b.ell_ax(
            at([s * 0.0425, -0.004, -0.013]),
            [0.0042, 0.0175, 0.0105],
            eax,
            0.005,
        );
    }
    // The brow ridge, overhanging the eyes (stronger on men), curving
    // back at its ends.
    b.ell(
        at([0.0, 0.0145, 0.041]),
        [0.032, 0.0082, 0.0084 + 0.0016 * male],
        0.012,
    );
    // The orbits: the skin in front of each eye cut away, then thin lids
    // wrapping the eyeball (an arched upper margin, a flatter lower one),
    // a crease where the upper lid meets the orbit under the brow.
    let open = 0.0002 * p.fem;
    for s in [1.0f32, -1.0] {
        let e = [s * EYE[0], EYE[1], EYE[2]];
        b.cut_ell(
            at(add(e, [s * 0.0005, 0.0004, 0.0055])),
            [0.0092, 0.0042, 0.0048],
            AXES,
            0.0028,
        );
        let r = EYE_R + 0.0009;
        let (yu, ku) = (0.0021 + open, 0.4f32);
        let (yl, kl) = (-0.0034 - open, 0.25f32);
        let nu = [0.0, -1.0, ku];
        b.lid(at(e), r, nu, (ku * r - yu) / len(nu), 0.0015);
        let nl = [0.0, 1.0, kl];
        b.lid(at(e), r, nl, (yl + kl * r) / len(nl), 0.0015);
    }
    // Nose: the bridge to the tip, and its wings.
    let nz = 0.0615 + 0.005 * (p.nose - 1.0);
    b.cone(
        at([0.0, 0.006, 0.0505]),
        at([0.0, -0.0175, nz - 0.001]),
        0.0039,
        0.0049 * p.nose.sqrt(),
        0.006,
    );
    // The tip: a soft ball a little proud of the bridge's line, the
    // columella under it running back to the lip.
    b.ell(
        at([0.0, -0.0185, nz + 0.0004]),
        [0.0048 * p.nose.sqrt(), 0.0049, 0.0046],
        0.004,
    );
    b.ell(at([0.0, -0.0232, nz - 0.0042]), [0.0024, 0.0022, 0.0045], 0.003);
    for s in [1.0f32, -1.0] {
        b.ell(
            at([s * 0.0062 * p.nose, -0.0222, nz - 0.0098]),
            [0.0042, 0.0042, 0.0048],
            0.004,
        );
    }
    // Lips (set back, modest) and the line between them.
    let lw = 0.0138 * p.lips;
    for (c, r) in lip_forms(p) {
        b.ell(at(c), r, 0.006);
    }
    // The philtrum's two ridges from under the nose to the upper lip's
    // peaks, and the hollow between the lower lip and the chin.
    for s in [1.0f32, -1.0] {
        b.cone(
            at([s * 0.0024, -0.0245, nz - 0.0062]),
            at([s * 0.0036, -0.0312, 0.0532]),
            0.0013,
            0.0015,
            0.003,
        );
    }
    b.cut_ell(at([0.0, -0.0462, 0.0525]), [0.0085, 0.0022, 0.0035], AXES, 0.004);
    b.cut_ell(
        at([0.0, -0.0367, 0.0545]),
        [lw * 1.02, 0.0007, 0.0048],
        AXES,
        0.0015,
    );
    if p.fat > 0.3 {
        let f = p.fat;
        b.ell(
            at([0.0, -0.064, 0.026]),
            [0.026 * f, 0.013 * f, 0.022 * f],
            0.014,
        );
        for s in [1.0f32, -1.0] {
            b.ell(
                at([s * 0.026, -0.03, 0.03]),
                [0.012 * f, 0.013 * f, 0.012 * f],
                0.012,
            );
        }
    }
}

/// The upper and the lower lip (centre from the head's centre, half
/// sizes; shares): the head's forms and the lips' own colour use the same.
fn lip_forms(p: &Person) -> [(V3, V3); 2] {
    let lw = 0.0138 * p.lips;
    [
        ([0.0, -0.0334, 0.0494], [lw, 0.0035 * p.lips, 0.0049]),
        ([0.0, -0.0401, 0.0486], [lw * 0.86, 0.0042 * p.lips, 0.005]),
    ]
}

/// The triangles of `m` whose centre `take` accepts, and the rest (each
/// with only its own vertices).
fn split(m: &Mesh, take: &dyn Fn(V3) -> bool) -> (Mesh, Mesh) {
    let (mut a, mut b) = (Mesh::default(), Mesh::default());
    let (mut ma, mut mb) = (
        vec![u32::MAX; m.vertices.len()],
        vec![u32::MAX; m.vertices.len()],
    );
    for t in m.indices.chunks(3) {
        let v = |i: u32| m.vertices[i as usize];
        let c = mul(
            add(add(
                [v(t[0])[0], v(t[0])[1], v(t[0])[2]],
                [v(t[1])[0], v(t[1])[1], v(t[1])[2]],
            ), [v(t[2])[0], v(t[2])[1], v(t[2])[2]]),
            1.0 / 3.0,
        );
        let (out, map) = if take(c) { (&mut a, &mut ma) } else { (&mut b, &mut mb) };
        for &i in t {
            if map[i as usize] == u32::MAX {
                map[i as usize] = out.vertices.len() as u32;
                out.vertices.push(m.vertices[i as usize]);
            }
            out.indices.push(map[i as usize]);
        }
    }
    (a, b)
}

// ---- the mesher ------------------------------------------------------------------

/// Mesh `f = 0` (negative inside) over `lo..hi` at cell size `h` with
/// surface nets, only in cells whose centre `keep` accepts. Samples are
/// taken only near the surface (blocks of 4 cells whose coarse corners lie
/// near it, and their neighbours).
fn mesh_field(f: &dyn Fn(V3) -> f32, lo: V3, hi: V3, h: f32, keep: &dyn Fn(V3) -> bool) -> Mesh {
    const B: usize = 4;
    let cells = |a: usize| (((hi[a] - lo[a]) / h).ceil().max(1.0) as usize).div_ceil(B) * B;
    let n = [cells(0), cells(1), cells(2)];
    let nb = [n[0] / B, n[1] / B, n[2] / B];
    let ns = [n[0] + 1, n[1] + 1, n[2] + 1];
    let pos = |i: usize, j: usize, k: usize| {
        [
            lo[0] + i as f32 * h,
            lo[1] + j as f32 * h,
            lo[2] + k as f32 * h,
        ]
    };
    let sidx = |i: usize, j: usize, k: usize| (i * ns[1] + j) * ns[2] + k;
    // Coarse corners and the blocks near the surface.
    let cs = [nb[0] + 1, nb[1] + 1, nb[2] + 1];
    let mut coarse = vec![0.0f32; cs[0] * cs[1] * cs[2]];
    for i in 0..cs[0] {
        for j in 0..cs[1] {
            for k in 0..cs[2] {
                coarse[(i * cs[1] + j) * cs[2] + k] = f(pos(i * B, j * B, k * B));
            }
        }
    }
    let thr = 1.6 * B as f32 * h;
    let bidx = |i: usize, j: usize, k: usize| (i * nb[1] + j) * nb[2] + k;
    let mut near = vec![false; nb[0] * nb[1] * nb[2]];
    for i in 0..nb[0] {
        for j in 0..nb[1] {
            for k in 0..nb[2] {
                let mut lo_d = f32::MAX;
                let (mut neg, mut pos_) = (false, false);
                for c in 0..8 {
                    let v = coarse
                        [((i + (c & 1)) * cs[1] + j + ((c >> 1) & 1)) * cs[2] + k + ((c >> 2) & 1)];
                    lo_d = lo_d.min(v.abs());
                    neg |= v < 0.0;
                    pos_ |= v >= 0.0;
                }
                near[bidx(i, j, k)] = lo_d < thr || (neg && pos_);
            }
        }
    }
    let mut active = near.clone();
    for i in 0..nb[0] {
        for j in 0..nb[1] {
            for k in 0..nb[2] {
                if !near[bidx(i, j, k)] {
                    continue;
                }
                for di in i.saturating_sub(1)..=(i + 1).min(nb[0] - 1) {
                    for dj in j.saturating_sub(1)..=(j + 1).min(nb[1] - 1) {
                        for dk in k.saturating_sub(1)..=(k + 1).min(nb[2] - 1) {
                            active[bidx(di, dj, dk)] = true;
                        }
                    }
                }
            }
        }
    }
    // Fine samples in the active blocks.
    let mut d = vec![f32::NAN; ns[0] * ns[1] * ns[2]];
    for bi in 0..nb[0] {
        for bj in 0..nb[1] {
            for bk in 0..nb[2] {
                if !active[bidx(bi, bj, bk)] {
                    continue;
                }
                for i in bi * B..=bi * B + B {
                    for j in bj * B..=bj * B + B {
                        for k in bk * B..=bk * B + B {
                            let s = sidx(i, j, k);
                            if d[s].is_nan() {
                                d[s] = f(pos(i, j, k));
                            }
                        }
                    }
                }
            }
        }
    }
    // One vertex per cell the surface crosses, moved onto the surface.
    let cidx = |i: usize, j: usize, k: usize| (i * n[1] + j) * n[2] + k;
    let mut vid = vec![u32::MAX; n[0] * n[1] * n[2]];
    let mut mb = MeshBuilder::new();
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let e = h * 0.3;
    let grad = |p: V3| {
        [
            f(add(p, [e, 0.0, 0.0])) - f(sub(p, [e, 0.0, 0.0])),
            f(add(p, [0.0, e, 0.0])) - f(sub(p, [0.0, e, 0.0])),
            f(add(p, [0.0, 0.0, e])) - f(sub(p, [0.0, 0.0, e])),
        ]
    };
    for bi in 0..nb[0] {
        for bj in 0..nb[1] {
            for bk in 0..nb[2] {
                if !active[bidx(bi, bj, bk)] {
                    continue;
                }
                for i in bi * B..bi * B + B {
                    for j in bj * B..bj * B + B {
                        for k in bk * B..bk * B + B {
                            let mut c = [0.0f32; 8];
                            let (mut neg, mut pos_) = (false, false);
                            for (q, v) in c.iter_mut().enumerate() {
                                *v = d[sidx(i + (q & 1), j + ((q >> 1) & 1), k + ((q >> 2) & 1))];
                                neg |= *v < 0.0;
                                pos_ |= *v >= 0.0;
                            }
                            if !(neg && pos_) {
                                continue;
                            }
                            let base = pos(i, j, k);
                            let centre = add(base, [h * 0.5; 3]);
                            if !keep(centre) {
                                continue;
                            }
                            let mut acc = [0.0f32; 3];
                            let mut cnt = 0.0f32;
                            for (a, z) in EDGES {
                                let (da, dz) = (c[a], c[z]);
                                if (da < 0.0) == (dz < 0.0) {
                                    continue;
                                }
                                let t = da / (da - dz);
                                let pa =
                                    [(a & 1) as f32, ((a >> 1) & 1) as f32, ((a >> 2) & 1) as f32];
                                let pz =
                                    [(z & 1) as f32, ((z >> 1) & 1) as f32, ((z >> 2) & 1) as f32];
                                acc = add(acc, lerp(pa, pz, t));
                                cnt += 1.0;
                            }
                            let mut p = add(base, mul(acc, h / cnt.max(1.0)));
                            // One Newton step onto the surface, kept near the cell.
                            let g = grad(p);
                            let g2 = dot(g, g);
                            if g2 > 1e-12 {
                                let v = f(p);
                                let step = mul(g, v * 2.0 * e / g2);
                                p = sub(p, step);
                                for a in 0..3 {
                                    p[a] = p[a].clamp(base[a] - h * 0.5, base[a] + h * 1.5);
                                }
                            }
                            let nrm = norm(g);
                            vid[cidx(i, j, k)] = mb.vertex(p, nrm);
                        }
                    }
                }
            }
        }
    }
    // Quads round every sampled edge the surface crosses.
    let v_at = |i: usize, j: usize, k: usize| vid[cidx(i, j, k)];
    let quad = |a: u32, b: u32, c: u32, dd: u32, flip: bool, mb: &mut MeshBuilder| {
        if a == u32::MAX || b == u32::MAX || c == u32::MAX || dd == u32::MAX {
            return;
        }
        if flip {
            mb.quad(a, dd, c, b)
        } else {
            mb.quad(a, b, c, dd)
        }
    };
    for i in 0..n[0] {
        for j in 0..n[1] {
            for k in 0..n[2] {
                let d0 = d[sidx(i, j, k)];
                if d0.is_nan() {
                    continue;
                }
                let inside = d0 < 0.0;
                // Along x: cells (i, j-1..j, k-1..k).
                if j > 0 && k > 0 {
                    let d1 = d[sidx(i + 1, j, k)];
                    if !d1.is_nan() && (d1 < 0.0) != inside {
                        quad(
                            v_at(i, j - 1, k - 1),
                            v_at(i, j, k - 1),
                            v_at(i, j, k),
                            v_at(i, j - 1, k),
                            !inside,
                            &mut mb,
                        );
                    }
                }
                // Along y: cells (i-1..i, j, k-1..k).
                if i > 0 && k > 0 {
                    let d1 = d[sidx(i, j + 1, k)];
                    if !d1.is_nan() && (d1 < 0.0) != inside {
                        quad(
                            v_at(i - 1, j, k - 1),
                            v_at(i - 1, j, k),
                            v_at(i, j, k),
                            v_at(i, j, k - 1),
                            !inside,
                            &mut mb,
                        );
                    }
                }
                // Along z: cells (i-1..i, j-1..j, k).
                if i > 0 && j > 0 {
                    let d1 = d[sidx(i, j, k + 1)];
                    if !d1.is_nan() && (d1 < 0.0) != inside {
                        quad(
                            v_at(i - 1, j - 1, k),
                            v_at(i, j - 1, k),
                            v_at(i, j, k),
                            v_at(i - 1, j, k),
                            !inside,
                            &mut mb,
                        );
                    }
                }
            }
        }
    }
    mb.finish()
}

// ---- noise for curls and strands -----------------------------------------------

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth value noise in -1..1.
fn vnoise(p: V3, seed: u32) -> f32 {
    let fl = [p[0].floor(), p[1].floor(), p[2].floor()];
    let t = sub(p, fl);
    let s = [
        t[0] * t[0] * (3.0 - 2.0 * t[0]),
        t[1] * t[1] * (3.0 - 2.0 * t[1]),
        t[2] * t[2] * (3.0 - 2.0 * t[2]),
    ];
    let (x, y, z) = (fl[0] as i32, fl[1] as i32, fl[2] as i32);
    let mut acc = 0.0;
    for c in 0..8i32 {
        let (dx, dy, dz) = (c & 1, (c >> 1) & 1, (c >> 2) & 1);
        let w = (if dx == 1 { s[0] } else { 1.0 - s[0] })
            * (if dy == 1 { s[1] } else { 1.0 - s[1] })
            * (if dz == 1 { s[2] } else { 1.0 - s[2] });
        acc += w * hash3(x + dx, y + dy, z + dz, seed);
    }
    acc * 2.0 - 1.0
}

/// 0 below `a`, 1 above `b`, smooth between.
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Cloth folds (metres, outward) at `q` on a person `h` tall: gravity
/// folds running down (stronger lower down, below `from` of the height),
/// of depth `amp`, and tension wrinkles running across (`across` of their
/// depth) round the waist.
fn cloth_folds(q: V3, h: f32, seed: u32, from: f32, amp: f32, across: f32) -> f32 {
    let w = h * 0.03;
    let down = ((h * from - q[1]) / (h * 0.12)).clamp(0.0, 1.0);
    let hang = vnoise([q[0] / w, q[1] / (w * 7.0), q[2] / w], seed);
    let waist = (-((q[1] - h * 0.6) / (h * 0.04)).powi(2)).exp();
    let tension = vnoise([q[0] / (w * 5.0), q[1] / (w * 0.7), q[2] / (w * 5.0)], seed ^ 0x77);
    amp * (down * hang + across * waist * tension * 0.6)
}

/// Tension folds (metres, outward) radiating from the point `c` (x and y,
/// shares of the height `h`) as on cloth pulled from there: `n` ridges round
/// it, `amp` deep, from `r0` out to `r1` (shares), their angles bent by noise
/// so that no two folds or sides match.
#[allow(clippy::too_many_arguments)]
fn radial_folds(q: V3, h: f32, c: [f32; 2], r0: f32, r1: f32, n: f32, amp: f32, seed: u32) -> f32 {
    let (dx, dy) = (q[0] / h - c[0], q[1] / h - c[1]);
    let r = (dx * dx + dy * dy).sqrt();
    let env = smooth(r0, r0 + 0.015, r) * (1.0 - smooth(r1 * 0.55, r1, r));
    if env <= 0.0 {
        return 0.0;
    }
    let w = h * 0.06;
    let bend = 0.6 * vnoise([q[0] / w, q[1] / w, q[2] / w], seed);
    let ridge = 0.5 + 0.5 * (dy.atan2(dx) * n + bend * n * 0.5).cos();
    // Each ridge its own depth, fading out along its length.
    let depth = 0.6 + 0.4 * vnoise([q[0] / (w * 2.0), q[1] / (w * 2.0), q[2] / (w * 2.0)], seed ^ 0x2b);
    amp * env * depth * ridge * ridge
}

// ---- one person's parts ----------------------------------------------------------

/// A box in shares of the height.
#[derive(Debug, Clone, Copy)]
struct Bx {
    lo: V3,
    hi: V3,
}

impl Bx {
    fn grow(self, g: f32) -> Self {
        Bx {
            lo: sub(self.lo, [g; 3]),
            hi: add(self.hi, [g; 3]),
        }
    }
    fn holds(&self, p: V3) -> bool {
        (0..3).all(|a| p[a] >= self.lo[a] && p[a] <= self.hi[a])
    }
}

/// The meshes of one person in its own frame (metres, standing on y = 0,
/// facing +z): skin, eyes (whites, irises), brows, hair, beard, top,
/// shorts.
struct Built {
    skin: Mesh,
    /// The lips' red (split from the skin).
    lips: Mesh,
    /// The upper lashes' dark line.
    lashes: Mesh,
    whites: Mesh,
    irises: Mesh,
    brows: Mesh,
    hair: Mesh,
    beard: Mesh,
    top: Mesh,
    shorts: Mesh,
}

fn build_person(p: &Person, cell: f32, seed: u32) -> Built {
    use grp::*;
    let hh = p.h;
    let j = joints(p);
    let forms = body_forms(p, &j);
    let fs = &forms;
    let body = |q: V3| eval(fs, q, ALL, 0, 1.0);
    let hc = mul(j.head, hh);
    let m = |s: f32| s * hh;
    // Region boxes (metres): the head, the hands and the feet are meshed
    // at twice the density.
    let reach = 0.3;
    let whole = Bx {
        lo: [m(-reach), -0.01, m(-0.13)],
        hi: [m(reach), m(1.045), m(0.21)],
    };
    let head_box = Bx {
        lo: [hc[0] - m(0.08), m(0.857), hc[2] - m(0.105)],
        hi: [hc[0] + m(0.08), m(1.04), hc[2] + m(0.1)],
    };
    let mut hand_box = [head_box; 2];
    for (i, b) in hand_box.iter_mut().enumerate() {
        let w = mul(j.wr[i], hh);
        let tip = add(w, mul(j.fo[i], m(0.11)));
        let c = lerp(w, tip, 0.5);
        *b = Bx {
            lo: [c[0] - m(0.04), tip[1] - m(0.02), c[2] - m(0.045)],
            hi: [c[0] + m(0.04), w[1] + m(0.006), c[2] + m(0.045)],
        };
    }
    let feet_box = Bx {
        lo: [m(-0.2), -0.01, m(-0.07)],
        hi: [m(0.2), m(0.062), m(0.2)],
    };
    let fine = [head_box, hand_box[0], hand_box[1], feet_box];
    let hf = cell * 0.5;
    let shrink = cell * 1.5;
    // Round 5: the face (eyes, nose, mouth) at twice the head's density
    // again, so the lids, the nose's wings and the lips' edges are formed,
    // not stepped. Its box is in the head's own (turned) frame.
    let hr = head_turn(p);
    let face_box = Bx {
        lo: add(hc, [m(-0.034), m(-0.052), m(0.02)]),
        hi: add(hc, [m(0.034), m(0.022), m(0.08)]),
    };
    let hq = hf * 0.5;
    let in_face = |q: V3, g: f32| face_box.grow(g).holds(unturn(q, hr));
    let coarse_keep = |q: V3| !fine.iter().any(|b| b.grow(-shrink).holds(q));
    let mut skin = mesh_field(&body, whole.lo, whole.hi, cell, &coarse_keep);
    for (k, b) in fine.into_iter().enumerate() {
        let g = b.grow(hf * 2.0);
        geo::merge(
            &mut skin,
            &mesh_field(&body, g.lo, g.hi, hf, &|q| {
                b.holds(q) && (k > 0 || !in_face(q, -hf * 1.5))
            }),
        );
    }
    {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for c in 0..8 {
            let v = [
                if c & 1 == 0 { face_box.lo[0] } else { face_box.hi[0] },
                if c & 2 == 0 { face_box.lo[1] } else { face_box.hi[1] },
                if c & 4 == 0 { face_box.lo[2] } else { face_box.hi[2] },
            ];
            let w = turn_by(v, hr);
            for (a, wa) in w.iter().enumerate() {
                lo[a] = lo[a].min(wa - hq * 2.0);
                hi[a] = hi[a].max(wa + hq * 2.0);
            }
        }
        geo::merge(
            &mut skin,
            &mesh_field(&body, lo, hi, hq, &|q| in_face(q, 0.0)),
        );
    }

    // The lips' red: the skin's triangles on the front of the lips' forms.
    let (lips, skin) = {
        let lf = lip_forms(p).map(|(c, r)| Shape::Ell {
            c: add(hc, mul(c, hh)),
            r: mul(r, hh),
            ax: AXES,
        });
        split(&skin, &|c| {
            let c = unturn(c, hr);
            let d = lf[0].dist(c).min(lf[1].dist(c));
            d < m(0.0008) && (c[2] - hc[2]) > m(0.0455) && (c[0] - hc[0]).abs() < m(0.0134 * p.lips)
        })
    };

    // ---- eyes and brows (small closed shapes)
    // The eyes, brows, lashes, hair and beard are made in the head's own
    // frame (as if it faced the body's way) and then turned with it.
    let head = |q: V3| eval(fs, turn_by(q, hr), HEAD, 0, 1.0);
    let (mut whites, mut irises, mut brows, mut lashes) = (
        Mesh::default(),
        Mesh::default(),
        Mesh::default(),
        Mesh::default(),
    );
    let male = 1.0 - p.fem;
    for s in [1.0f32, -1.0] {
        let e = add(hc, [m(s * EYE[0]), m(EYE[1]), m(EYE[2])]);
        let r = m(EYE_R);
        geo::merge(&mut whites, &ball([r; 3], e, AXES));
        let fwd = norm([s * 0.05, -0.02, 1.0]);
        let iax = frame(cross(fwd, [0.0, 1.0, 0.0]), fwd);
        geo::merge(
            &mut irises,
            &ball(
                [m(0.0012), m(0.0036), m(0.0036)],
                add(e, mul(fwd, r * 0.93)),
                [iax[2], iax[0], iax[1]],
            ),
        );
        // The upper lashes: a thin dark line along the upper lid's margin
        // (the lid's ball cut by its tilted plane), from corner to corner.
        let rl = EYE_R + 0.0009;
        for k in 0..6 {
            let phi = (-0.95 + 1.9 * (k as f32 + 0.5) / 6.0) * s;
            let z = rl * phi.cos();
            let y = 0.4 * z - (0.4 * rl - (0.0021 + 0.0002 * p.fem)) + 0.0003;
            let rho = (rl * rl - y * y).max(0.0).sqrt() + 0.0003;
            let lo = [rho * phi.sin(), y, rho * phi.cos()];
            let c = add(e, mul(lo, hh));
            let out = norm(lo);
            let along = norm([phi.cos(), 0.0, -phi.sin()]);
            let ax = frame(out, along);
            geo::merge(
                &mut lashes,
                &ball(
                    [m(0.0005), m(0.0007), m(0.0021)],
                    c,
                    [cross(ax[1], ax[2]), ax[1], ax[2]],
                ),
            );
        }
        // The brow: short flat hairs' patches laid on the skin along an
        // arch over the eye (thick at the inner end, thin at the tail),
        // each found on the head's surface and sunk into it.
        let segs = 6;
        for k in 0..segs {
            let t = (k as f32 + 0.5) / segs as f32;
            let bx = 0.0085 + 0.0225 * t;
            let by = 0.0086 + 0.0024 * (std::f32::consts::PI * t * 0.9).sin() - 0.0022 * t;
            let (x, y) = (hc[0] + m(s * bx), hc[1] + m(by));
            // March in from the front onto the surface.
            let (mut zo, mut zi) = (hc[2] + m(0.075), hc[2] + m(0.02));
            for _ in 0..24 {
                let zm = 0.5 * (zo + zi);
                if head([x, y, zm]) > 0.0 {
                    zo = zm;
                } else {
                    zi = zm;
                }
            }
            let at = [x, y, 0.5 * (zo + zi)];
            let gd = m(0.0015);
            let n = norm([
                head(add(at, [gd, 0.0, 0.0])) - head(sub(at, [gd, 0.0, 0.0])),
                head(add(at, [0.0, gd, 0.0])) - head(sub(at, [0.0, gd, 0.0])),
                head(add(at, [0.0, 0.0, gd])) - head(sub(at, [0.0, 0.0, gd])),
            ]);
            let along = norm([s, 0.12 - 0.35 * t, 0.0]);
            let ax = frame(n, along);
            let th = (0.0023 - 0.0013 * t) * (1.0 + 0.3 * male);
            geo::merge(
                &mut brows,
                &ball(
                    [m(th * 0.5), m(0.0007), m(0.0029)],
                    sub(at, mul(n, m(0.0003))),
                    [cross(ax[1], ax[2]), ax[1], ax[2]],
                ),
            );
        }
    }

    // ---- hair
    let hs = seed ^ 0x51ed;
    // A curly top bun: a cluster of round curls on the crown (not one
    // smooth knob), each its own size.
    let curls: Vec<(V3, f32)> = if p.hair == Hair::TopBun {
        let c0 = [0.0, 0.066, -0.028];
        (0..11u32)
            .filter_map(|k| {
                let a = k as f32 * 2.399_963;
                let z = 1.0 - 2.0 * (k as f32 + 0.5) / 11.0;
                let rr = (1.0 - z * z).max(0.0).sqrt();
                let dir = [rr * a.cos(), z, rr * a.sin()];
                // None pointing down into the head.
                if dir[1] < -0.55 {
                    return None;
                }
                let j = 0.8 + 0.4 * sh::rnd(hs, k, 3);
                let c = add(c0, mul(dir, 0.0165 * j));
                let r = 0.0105 + 0.0035 * sh::rnd(hs, k, 4);
                Some((add(hc, mul(c, hh)), m(r)))
            })
            .collect()
    } else {
        Vec::new()
    };
    let hair_field = |q: V3| -> f32 {
        let l = mul(sub(q, hc), 1.0 / hh);
        let th = l[0].atan2(l[2]);
        let (c1, c2) = (th.cos(), (2.0 * th).cos());
        let (front, back) = match p.hair {
            Hair::Crop => (0.045, -0.028),
            Hair::Swept => (0.043, -0.056),
            Hair::Curly => (0.043, -0.046),
            _ => (0.041, -0.03),
        };
        // The hairline: high on the forehead, above the ears, low at the nape.
        let side = 0.012;
        let c0 = (front + back) * 0.25 + side * 0.5;
        let cc2 = c0 - side;
        let cc1 = (front - back) * 0.5;
        let line = c0 + cc1 * c1 + cc2 * c2;
        let mask = line - l[1];
        let top = ((l[1] + 0.02) / 0.07).clamp(0.0, 1.0);
        let (t, amp, freq) = match p.hair {
            Hair::Bun => (0.0032 + 0.0018 * top, 0.0008, 90.0),
            Hair::Crop => (0.0028 + 0.0025 * top, 0.0006, 160.0),
            Hair::LooseBun => (0.0045 + 0.003 * top, 0.0016, 70.0),
            Hair::Curly => (0.009 + 0.01 * top, 0.0035, 60.0),
            Hair::TopBun => (0.0045 + 0.0025 * top, 0.002, 80.0),
            Hair::Swept => (0.0045 + 0.013 * top * top, 0.003, 40.0),
        };
        // Thinning to a fringe of short hairs at the hairline (no brim,
        // no cap's edge), full about 2.5 cm above it.
        let ramp = match p.hair {
            Hair::Curly | Hair::Swept => 0.014 + 0.016 * c1.max(0.0),
            _ => 0.014,
        };
        let tp = ((-mask) / ramp).clamp(0.0, 1.0);
        let taper = 0.06 + 0.94 * tp * tp * (3.0 - 2.0 * tp);
        // Strands: pulled-back and swept hair runs front to back, curls
        // and crops have no direction.
        let flow = match p.hair {
            Hair::Curly | Hair::Crop => [1.0, 1.0, 1.0],
            _ => [1.0, 1.0, 0.08],
        };
        let lq = [
            l[0] * freq * flow[0],
            l[1] * freq * flow[1],
            l[2] * freq * flow[2],
        ];
        // Broad clumps, then ridged strands and finer strands along the flow.
        let clump = vnoise(mul(lq, 0.35), hs ^ 5);
        let strand = 1.0 - vnoise(mul(lq, 2.2), hs ^ 9).abs();
        let fine = 1.0 - vnoise(mul(lq, 5.0), hs ^ 11).abs();
        let mut n = (0.3 * clump + 0.75 * strand * strand + 0.45 * fine * fine - 0.48) * amp * taper;
        match p.hair {
            Hair::Curly => {
                // Curls: rounded lumps at two sizes that break the outline
                // (fuller on top, close at the sides and the nape).
                // (round 4: the big lumps halved and finer, the curls
                // tighter, so the outline breaks into curls, not blobs).
                let big = vnoise(mul(l, 38.0), hs ^ 21);
                let curl = 1.0 - vnoise(mul(l, 95.0), hs ^ 23).abs();
                let tight = 1.0 - vnoise(mul(l, 210.0), hs ^ 24).abs();
                n += (0.0022 * big + 0.0036 * curl * curl + 0.0012 * tight * tight - 0.0016) * taper * (0.35 + 0.65 * top);
            }
            Hair::Swept => {
                // Waves across the flow and separate locks along it.
                let wave = (l[2] * 105.0 + 2.5 * vnoise(mul(l, 25.0), hs ^ 25)).sin();
                let locks = vnoise([l[0] * 70.0, l[1] * 70.0, l[2] * 9.0], hs ^ 27);
                n += (0.0022 * wave + 0.003 * locks) * taper * (0.3 + 0.7 * top);
            }
            Hair::TopBun | Hair::LooseBun => {
                // Wavy texture under the pull.
                n += 0.0012 * (1.0 - vnoise(mul(l, 95.0), hs ^ 29).abs()) * taper;
            }
            _ => {}
        }
        let mut d = smax(head(q) - m(t * taper + n), m(mask), m(0.003));
        // Buns: round, sitting on the head (a small blend, no cone).
        let bun = match p.hair {
            Hair::Bun => Some(([0.0, 0.017, -0.068], [0.021, 0.019, 0.017], 0.0015, 0.004)),
            Hair::LooseBun => Some(([0.0, 0.002, -0.072], [0.025, 0.021, 0.02], 0.003, 0.006)),
            _ => None,
        };
        if let Some((c, r, a, k)) = bun {
            let sb = Shape::Ell {
                c: add(hc, mul(c, hh)),
                r: mul(r, hh),
                ax: AXES,
            };
            let nb = (vnoise(mul(l, 70.0), hs ^ 7) + 0.5 * vnoise(mul(l, 160.0), hs ^ 17)) * a;
            d = smin(d, sb.dist(q) - m(nb), m(k));
        }
        if !curls.is_empty() {
            let mut bd = f32::MAX;
            for (c, r) in &curls {
                bd = smin(bd, len(sub(q, *c)) - r, m(0.003));
            }
            let ring = 1.0 - vnoise(mul(l, 120.0), hs ^ 31).abs();
            d = smin(d, bd - m(0.0016 * ring * ring), m(0.004));
        }
        if p.hair == Hair::Swept {
            // Volume swept up and back off the forehead.
            let sb = Shape::Ell {
                c: add(hc, mul([0.0, 0.05, 0.01], hh)),
                r: mul([0.036, 0.0095, 0.045], hh),
                ax: AXES,
            };
            d = smin(d, smax(sb.dist(q) - m(n * 0.6), m(mask), m(0.004)), m(0.012));
        }
        d
    };
    let hb = head_box;
    let hair = turned(&mesh_field(&hair_field, hb.lo, hb.hi, hf, &|_| true), hr);

    // ---- beard
    let beard = if p.beard > 0.0 {
        let face = |q: V3| eval(fs, turn_by(q, hr), HEAD | NECK, 0, 1.0);
        let mouth = Shape::Ell {
            c: add(hc, mul([0.0, -0.0368, 0.0555], hh)),
            r: mul([0.016, 0.0064, 0.018], hh),
            ax: AXES,
        };
        let bfield = |q: V3| -> f32 {
            let l = mul(sub(q, hc), 1.0 / hh);
            let th = l[0].atan2(l[2]).abs();
            let u = (th / std::f32::consts::FRAC_PI_2).min(1.3);
            // Ragged borders (no cut edge), the upper one over the cheeks,
            // the lower one following the jaw back to the neck (higher at
            // the sides than under the chin).
            let rag = 0.003 * vnoise(mul(l, 95.0), hs ^ 41);
            let line = -0.03 + 0.032 * u.powf(2.2) + rag;
            let bottom = -0.069 + 0.016 * u.min(1.0) + rag;
            let mask = (l[1] - line).max(-0.01 - l[2]).max(bottom - l[1]);
            // Short at every edge, growing out from the skin; full on the
            // chin, closer on the cheeks and under the jaw.
            let tp = ((-mask) / 0.012).clamp(0.0, 1.0);
            let grow = 0.08 + 0.92 * tp * tp * (3.0 - 2.0 * tp);
            let full = p.beard
                * (1.0 - 0.4 * u.min(1.0))
                * (1.0 - 0.35 * (1.0 - smooth(-0.066, -0.05, l[1])));
            // Clumps of hair hanging down, each its own length, and fine
            // strands along them.
            let clump = vnoise([l[0] * 150.0, l[1] * 48.0, l[2] * 150.0], hs ^ 3);
            let fine = 1.0 - vnoise([l[0] * 420.0, l[1] * 130.0, l[2] * 420.0], hs ^ 13).abs();
            let n = (0.55 * clump + 0.45 * fine * fine - 0.2) * p.beard * 0.5;
            let d = smax(face(q) - m((full + n) * grow), m(mask), m(0.0025));
            smax(d, -mouth.dist(q), m(0.003))
        };
        let bb = Bx {
            lo: [hc[0] - m(0.06), m(0.84), hc[2] - m(0.03)],
            hi: [hc[0] + m(0.06), hc[1] + m(0.01), hc[2] + m(0.085)],
        };
        turned(&mesh_field(&bfield, bb.lo, bb.hi, hf, &|_| true), hr)
    } else {
        Mesh::default()
    };

    // ---- garments: the body's field grown a little (fitted jersey lies a
    // few millimetres off the skin and bridges its hollows), cut by hems
    // and openings. The shorts first: the top's hem lies over them.
    let cs = seed ^ 0xc1a7;
    let waist = p.rise;
    let sw_ = m(0.0026);
    let crotch = [0.0, 0.472];
    // Round 5: each person's own hem, measured on the reference (the
    // petite woman's tank ends at 0.586 of her height, the curvy woman's
    // at 0.522; one height for all left most of them wrong).
    let top_hem = p.top_hem;
    // The shorts' outer face before their hems: a waistband (thicker),
    // cuffs at the legs, the centre seam as a fine groove; folds radiating
    // from the crotch over the fronts of the thighs, and compression folds
    // round the hips.
    let shorts_shell = |q: V3| -> f32 {
        let below_waist = m(waist) - q[1];
        let above_hem = q[1] - m(p.hem);
        let waistband = 1.0 - smooth(m(0.018), m(0.021), below_waist);
        let cuff = 1.0 - smooth(m(0.011), m(0.014), above_hem);
        let centre = (-(q[0] / m(0.003)).powi(2)).exp();
        let front = smooth(-m(0.01), m(0.02), q[2]);
        let folds = cloth_folds(q, hh, cs ^ 0x55, 0.5, m(0.0012), 1.4).max(-m(0.0006))
            + front * radial_folds(q, hh, crotch, 0.012, 0.085, 7.0, m(0.0022), cs ^ 0x91);
        let thick = sw_ + m(0.0014) * waistband + m(0.0009) * cuff - m(0.0004) * centre;
        // Under the top they lie thin and plain (no waistband or folds to
        // push the top out into a ledge), reaching their full thickness a
        // little below its hem, so the hem lies on them as a thin edge.
        let under = smooth(m(top_hem - 0.022), m(top_hem - 0.004), q[1]);
        let out = (thick + folds) * (1.0 - under) + m(0.001) * under;
        eval(fs, q, TRUNK | PELVIS | THIGH, 0, 1.25) - out
    };
    let shorts_field = |q: V3| -> f32 {
        let mask = (q[1] - m(waist)).max(m(p.hem) - q[1]);
        smax(shorts_shell(q), mask, m(0.0025))
    };
    let sb = Bx {
        lo: [m(-0.24), m(p.hem - 0.03), m(-0.16)],
        hi: [m(0.24), m(waist + 0.03), m(0.24)],
    };
    let shorts = mesh_field(&shorts_field, sb.lo, sb.hi, cell * 0.6, &|_| true);
    // Just over the shorts (below their waistband's top), where the top's
    // hem lies on them (rarely outside the top's own face).
    let over_shorts = |q: V3| (shorts_shell(q) - m(0.0014)).max(q[1] - m(waist));

    let neck_y = 0.822;
    let tee = p.top == Top::Tee;
    // Each strap centred over the shoulder, its width the person's own.
    let xs = if p.fem > 0.5 { 0.058 } else { 0.062 };
    let (strap_in, strap_out) = (xs - p.strap * 0.5, xs + p.strap * 0.5);
    let arm_drop = match p.top {
        // A racer's armhole ends at the armpit (deeper bared the chest's
        // side between the cloth and the arm as a skin lump).
        // Round 5: a little shallower, so the cloth covers the far side's
        // armpit (a skin nub showed there between the strap and the arm).
        Top::Racer => 0.058,
        Top::Tank => 0.095 + 0.015 * p.fat,
        Top::Tee => 0.0,
    };
    let sw = j.sw;
    let cut = |c: V3, r: V3| Shape::Ell {
        c: mul(c, hh),
        r: mul(r, hh),
        ax: AXES,
    };
    // A U-shaped front neckline down to the person's own depth (the tanks;
    // the tee's crew neck is a ring round the neck's base, below).
    let neck_front = {
        let yc = neck_y + 0.024;
        cut([0.0, yc, 0.07], [strap_in, yc - p.scoop, 0.085])
    };
    let neck_back = cut([0.0, neck_y + 0.03, -0.075], [strap_in, 0.05, 0.075]);
    // The crew neck: open inside a ring a little wider than the neck's base
    // (round the neck's own axis) above a line lower in front than behind,
    // so the collar sits on the trapezius and just above the collarbones.
    let neck_r = 0.03 + 0.004 * (1.0 - p.fem) + 0.007 * p.mus * (1.0 - p.fem) + 0.012 * p.fat;
    let crew = |q: V3| -> f32 {
        let l = mul(q, 1.0 / hh);
        let za = -0.022 + (l[1] - 0.8) / 0.1 * 0.018;
        let rad = (l[0] * l[0] + (l[2] - za) * (l[2] - za)).sqrt();
        // Low at the front's middle, curving up round the sides to meet the
        // trapezius, a little higher again at the back (no square corners).
        let c = (l[2] - za) / rad.max(1e-4);
        let line = 0.846 - 0.036 * c.max(0.0).powf(1.5) - 0.006 * (-c).max(0.0);
        m((neck_r + 0.0075 - rad).min(l[1] - line))
    };
    let arm_l = cut(
        [sw + 0.006, 0.778 - 0.012 * p.fat, -0.006],
        [sw + 0.006 - strap_out, arm_drop, 0.13],
    );
    let arm_r = cut(
        [-(sw + 0.006), 0.778 - 0.012 * p.fat, -0.006],
        [sw + 0.006 - strap_out, arm_drop, 0.13],
    );
    let sleeve_hem = [
        lerp(mul(j.sh[0], hh), mul(j.el[0], hh), 0.42),
        lerp(mul(j.sh[1], hh), mul(j.el[1], hh), 0.42),
    ];
    let sleeve_dir = [norm(sub(j.el[0], j.sh[0])), norm(sub(j.el[1], j.sh[1]))];
    // Across the bust the cloth spans from breast to breast (no moulding
    // into the cleavage).
    let bridge = if p.fem > 0.5 && p.bust > 0.05 {
        let zf = 0.062 + 0.022 * p.bust + 0.01 * p.fat;
        Some(cut(
            [0.0, 0.699 - 0.006 * p.bust, 0.03],
            [0.07, 0.026, zf - 0.035],
        ))
    } else {
        None
    };
    // A loose men's tank or tee hangs straight down from the chest over
    // the stomach (the field is the least of the body's field here and at
    // every step up to the chest); below `drape_low` it follows the body
    // again, tucking under a belly. Round 5: only on a heavy build (the
    // reference's muscular and lean men wear fitted tops); every other top
    // is fitted.
    let drape_top = if p.fat > 0.5 { 0.72 } else { 0.0 };
    let drape_low = m(0.6);
    let drape_step = m(0.01);
    let off = m(0.0022);
    let fold_amp = if p.fem > 0.5 { 0.0032 } else { 0.0044 };
    let top_field = |q: V3| -> f32 {
        // The trunk and the pelvis (not the thighs: the hem follows the
        // hips by lying on the shorts, not flaring over the legs).
        let mut shell = eval(fs, q, TRUNK | PELVIS, 0, 1.8);
        let mut y = m(drape_top);
        let stop = q[1].max(drape_low);
        while y > stop {
            shell = shell.min(eval(fs, [q[0], y, q[2]], TRUNK | PELVIS, 0, 1.8));
            y -= drape_step;
        }
        if let Some(b) = &bridge {
            shell = smin(shell, b.dist(q), m(0.02));
        }
        // The skin's own blend from each arm into the chest and the back
        // (and on the tee the shoulders, the sleeves and the neck's base):
        // the cloth covers it, so no skin pushes through at an armhole's
        // edge or between a sleeve and the body.
        let upper = if tee {
            TRUNK | PELVIS | NECK | ARM_UP
        } else {
            TRUNK | PELVIS | ARM_UP
        };
        shell = shell.min(eval(fs, q, upper, 0, 1.05));
        // The openings (positive: no cloth; negative inside the cloth): the
        // cloth lies a little closer to the skin at their edges.
        let mut open = if tee {
            crew(q)
        } else {
            (-neck_front.dist(q))
                .max(-neck_back.dist(q))
                .max(-arm_l.dist(q))
                .max(-arm_r.dist(q))
        };
        // Where an arm takes over from the trunk the cloth stops (a tank's
        // armhole below its cut; the tee's sleeve below its hem).
        let tr = eval(fs, q, TRUNK | PELVIS | NECK, 0, 1.0);
        let mut on_arm = -1.0f32;
        for (i, sd) in [(0usize, -1i32), (1, 1)] {
            let own = tr - eval(fs, q, ARM_UP | ARM_LO | HAND, sd, 1.0) - m(0.004);
            on_arm = on_arm.max(own);
            let c = if tee {
                own.min(dot(sub(q, sleeve_hem[i]), sleeve_dir[i]))
            } else {
                own
            };
            open = open.max(c);
        }
        let hug = (-open / m(0.02)).clamp(0.0, 1.0);
        // The tee's collar: a ribbed band a little proud of the body.
        let collar = if tee {
            m(0.0011) * (1.0 - smooth(m(0.008), m(0.011), -crew(q)))
        } else {
            0.0
        };
        // Its sleeves stand a little off the arm below the shoulder.
        let loose = if tee {
            m(0.0004) * smooth(-m(0.01), m(0.004), on_arm)
        } else {
            0.0
        };
        // A thin hem, its edge a little wavy.
        let wave = m(0.002) * vnoise([q[0] / m(0.05), 0.0, q[2] / m(0.05)], cs);
        let above = q[1] - m(top_hem) - wave;
        // Folds: gravity folds lower down, tension folds radiating from
        // each armpit across the chest and the back (each side its own),
        // compression at the waist; mostly outward (inward would meet the
        // skin).
        let mut folds = cloth_folds(q, hh, cs, 0.7, m(fold_amp), 0.8).max(-m(0.0009));
        for (i, s) in [(0u32, 1.0f32), (1, -1.0)] {
            folds += radial_folds(
                q,
                hh,
                [s * (sw - 0.012), 0.705 - 0.01 * p.fat],
                0.008,
                0.1 + 0.03 * p.fat,
                9.0,
                m(0.0036),
                cs ^ (0x3c + i),
            );
        }
        // Soft, uneven folds gathered round the waist above the hem (the
        // cloth's slack settling on the shorts): wavy ridges a few
        // centimetres apart, deeper on one side than the other, more on a
        // loose men's top and over a belly.
        let wy = (q[1] / hh - top_hem) / 0.075;
        if wy > -0.1 && wy < 1.2 {
            let env = smooth(-0.05, 0.12, wy) * (1.0 - smooth(0.55, 1.15, wy));
            let side = 0.35 + 0.65 * (0.5 + 0.5 * vnoise([q[0] / m(0.08), 0.0, q[2] / m(0.08)], cs ^ 0x5a));
            let warp = vnoise([q[0] / m(0.045), q[1] / m(0.06), q[2] / m(0.045)], cs ^ 0x5b);
            let ph = (q[1] + 0.25 * (q[0] + q[2])) / m(0.0042) + 2.4 * warp;
            let ripple = 0.5 + 0.5 * ph.sin();
            let amp = (0.0016 + 0.0014 * (1.0 - p.fem) + 0.0016 * p.fat) * env * side;
            folds += m(amp) * ripple * ripple;
        }
        let hem_edge = m(0.0004) * (1.0 - smooth(0.0, m(0.006), above));
        let shell = shell - off * (0.75 + 0.25 * hug) - hem_edge - folds - collar - loose;
        // Over the shorts the top lies on them.
        let shell = if q[1] < m(waist + 0.02) {
            smin(shell, over_shorts(q), m(0.006))
        } else {
            shell
        };
        let mask = (-above).max(open);
        smax(shell, mask, m(0.002))
    };
    let top_box = Bx {
        lo: [m(-0.26), m(top_hem - 0.03), m(-0.15)],
        // Above the straps on a heavy build's shoulders.
        hi: [m(0.26), m(0.93), m(0.24)],
    };
    let top = mesh_field(&top_field, top_box.lo, top_box.hi, cell * 0.6, &|_| true);

    Built {
        skin,
        lips,
        whites: turned(&whites, hr),
        irises: turned(&irises, hr),
        brows: turned(&brows, hr),
        lashes: turned(&lashes, hr),
        hair,
        beard,
        top,
        shorts,
    }
}

/// A small closed ellipsoid on axes `ax` (for eyes and brows).
fn ball(r: V3, c: V3, ax: [V3; 3]) -> Mesh {
    let mut m = sh::icosphere(2);
    for v in &mut m.vertices {
        let l = [v[0] * r[0], v[1] * r[1], v[2] * r[2]];
        let w = add(
            add(add(c, mul(ax[0], l[0])), mul(ax[1], l[1])),
            mul(ax[2], l[2]),
        );
        let nl = norm([v[3] / r[0], v[4] / r[1], v[5] / r[2]]);
        let nw = norm(add(
            add(mul(ax[0], nl[0]), mul(ax[1], nl[1])),
            mul(ax[2], nl[2]),
        ));
        *v = [w[0], w[1], w[2], nw[0], nw[1], nw[2]];
    }
    m
}

/// The person `i` of the line-up for `seed`: one of the six builds, varied
/// by `variety`.
fn person(i: u32, seed: u32, variety: f32) -> Person {
    let base = archetypes();
    // No variety keeps the reference's order; more turns the seed's start.
    let k = (i as usize + (sh::rnd(seed, 0, 99) * 6.0 * variety).floor() as usize) % 6;
    let mut p = base[k];
    let v = variety;
    let r = |c: u32, a: f32, b: f32| v * sh::range(seed, i, c, a, b);
    p.h = (p.h * (1.0 + r(1, -0.05, 0.05))).clamp(1.45, 2.05);
    p.mus = (p.mus + r(2, -0.25, 0.25)).clamp(0.0, 1.0);
    p.fat = (p.fat + r(3, -0.2, 0.25)).clamp(0.0, 1.0);
    p.bust = (p.bust + r(4, -0.3, 0.3)).clamp(0.0, 1.2);
    p.jaw = (p.jaw + r(5, -0.08, 0.08)).clamp(0.8, 1.3);
    p.nose = (p.nose + r(6, -0.15, 0.15)).clamp(0.6, 1.3);
    // A darker or lighter skin within the archetype's family.
    let dk = r(7, -0.12, 0.12);
    p.skin = mix3(
        p.skin,
        if dk < 0.0 {
            [p.skin[0] * 0.6, p.skin[1] * 0.55, p.skin[2] * 0.5]
        } else {
            [p.skin[0] * 1.2, p.skin[1] * 1.22, p.skin[2] * 1.25]
        },
        dk.abs() * 2.0,
    );
    p
}

/// The line-up: `count` people `spacing` apart along x, each turned
/// `turn` degrees to the right, its clothes in `cloth`.
pub fn lineup(v: &Values, seed: u32) -> evani_procedural::Output {
    let n = v.f("count").round().clamp(1.0, 9.0) as u32;
    let variety = v.f("variety").clamp(0.0, 1.0);
    let spacing = v.f("spacing");
    let turn = v.f("turn").to_radians();
    // The mesh's cell: about 9 mm on a 1.75 m person at detail 1 (coarser
    // under unit tests, which build every asset unoptimised many times).
    let detail = v.f("detail").clamp(0.3, 2.0);
    let test_scale = if cfg!(test) { 5.0 } else { 1.0 };
    let cloth = v.color("color");
    let mut parts = Vec::new();
    for i in 0..n {
        let p = person(i, seed, variety);
        let cell = p.h * 0.0052 / detail * test_scale;
        let b = build_person(&p, cell, seed.wrapping_add(i * 7919));
        let x = (i as f32 - (n - 1) as f32 * 0.5) * spacing;
        let place = |m: &Mesh| sh::place(m, 1.0, turn, 0.0, [x, 0.0, 0.0]);
        // The cloth a touch lighter or darker per person (heather greys).
        let shade = 1.0 + 0.06 * (sh::rnd(seed, i, 40) - 0.5);
        let c = [cloth[0] * shade, cloth[1] * shade, cloth[2] * shade];
        let beard_rgb = mix3(p.hair_rgb, [0.03, 0.025, 0.02], 0.2);
        let shorts_rgb = [c[0] * 0.92, c[1] * 0.92, c[2] * 0.93];
        // The lips a little darker and redder than the skin round them.
        let lip = [p.skin[0] * 0.78, p.skin[1] * 0.52, p.skin[2] * 0.52];
        for (m, rgb, base, mat) in [
            (&b.skin, skin_shown(p.skin), SKIN, "skin"),
            (&b.lips, skin_shown(lip), SKIN, "skin"),
            (&b.whites, [0.26, 0.245, 0.23], GLASS, "glass"),
            (&b.irises, p.iris, GLASS, "glass"),
            (&b.brows, mix3(p.hair_rgb, [0.03, 0.022, 0.016], 0.45), LEATHER, "leather"),
            // Lashes dark on everyone (fair hair has dark-rooted lashes).
            (&b.lashes, [0.022, 0.016, 0.012], LEATHER, "leather"),
            (&b.hair, p.hair_rgb, LEATHER, "leather"),
            (&b.beard, beard_rgb, LEATHER, "leather"),
        ] {
            if !m.indices.is_empty() {
                parts.push(Part::new(place(m), on(rgb, base)).material(mat));
            }
        }
        // The cotton: the catalogue's jersey knit with its heather of short
        // lighter and darker fibres (the knit's slubs), painted by the cloth
        // recipe where a game lists it, else flat.
        let knit = |rgb: [f32; 3], k: u32| {
            sp::CLOTH_KNIT_JERSEY
                .tint([rgb[0] * KNIT_GAIN, rgb[1] * KNIT_GAIN, rgb[2] * KNIT_GAIN * 0.94])
                .number(9, 0.45)
                .extras(0b10_0111)
                .seed(seed.wrapping_add(i * 31 + k))
                .scale(KNIT_M)
        };
        for (m, s) in [(&b.top, knit(c, 1)), (&b.shorts, knit(shorts_rgb, 2))] {
            if !m.indices.is_empty() {
                parts.push(Part::surfaced(place(m), s));
            }
        }
    }
    evani_procedural::Output::Parts(parts)
}

/// Metres per uv unit of the jersey (70 weave steps per unit, about 9 mm
/// each, so its heather (the knit's slubs: number 9, extras bit 5) reads at
/// the reference's scale as short fibres a few millimetres long).
const KNIT_M: f32 = 0.6;

/// The knit path draws darker than the flat colour the param names (round
/// 2 measured the tank at 76 of 255 against the reference's 113 at gain
/// 1, 101 at 1.5): its gain.
const KNIT_GAIN: f32 = 1.84;

/// A forward part draws in its colour times its look material's base
/// colour; these are the realism (film) look's bases for the materials the
/// line-up's flat parts use (skin's soft sheen, leather's sheen for hair,
/// glass's wet gloss for eyes), so `on` makes the colour given the colour
/// that shows.
const SKIN: [f32; 3] = [0.78, 0.38, 0.22];
const LEATHER: [f32; 3] = [0.52, 0.2, 0.06];
const GLASS: [f32; 3] = [0.9, 0.9, 0.88];

/// A skin swatch as the body shows it under the studio's key: darker and
/// less saturated than the swatch (round 1 measured the lit thigh too bright
/// and too warm against the reference's).
fn skin_shown(rgb: [f32; 3]) -> [f32; 3] {
    let l = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    mul(mix3([l; 3], rgb, 0.84), 0.9)
}

fn on(rgb: [f32; 3], base: [f32; 3]) -> [f32; 3] {
    [rgb[0] / base[0], rgb[1] / base[1], rgb[2] / base[2]]
}

#[cfg(test)]
mod npc_tests {
    use super::*;

    #[test]
    fn a_body_is_one_closed_surface_standing_on_the_ground() {
        let p = archetypes()[1];
        let b = build_person(&p, p.h * 0.016, 1);
        assert!(
            b.skin.indices.len() > 3000,
            "a full body of triangles: {}",
            b.skin.indices.len()
        );
        let (lo, hi) = geo::bounds(&b.skin);
        assert!(
            lo[1] > -0.02 && lo[1] < 0.03,
            "the soles on the ground: {}",
            lo[1]
        );
        assert!(
            (hi[1] - p.h).abs() < 0.05,
            "as tall as asked: {} vs {}",
            hi[1],
            p.h
        );
        assert!(
            !b.top.indices.is_empty() && !b.shorts.indices.is_empty() && !b.hair.indices.is_empty(),
            "clothes and hair"
        );
    }

    /// Edges used by exactly one triangle (a hole's rim), with their heights.
    fn open_edges(m: &Mesh) -> Vec<f32> {
        let mut count = std::collections::HashMap::new();
        for t in m.indices.chunks(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *count.entry((a.min(b), a.max(b))).or_insert(0u32) += 1;
            }
        }
        count
            .into_iter()
            .filter(|(_, c)| *c == 1)
            .map(|((a, _), _)| m.vertices[a as usize][1])
            .collect()
    }

    #[test]
    fn garments_and_hair_are_closed() {
        for (k, p) in archetypes().iter().enumerate() {
            let b = build_person(p, p.h * 0.0052 * 2.0, 3);
            for (name, m) in [("top", &b.top), ("shorts", &b.shorts), ("hair", &b.hair)] {
                let mut open = open_edges(m);
                open.sort_by(f32::total_cmp);
                assert!(
                    open.is_empty(),
                    "person {k} {name}: {} open edges at heights {:?}",
                    open.len(),
                    &open[..open.len().min(12)]
                );
            }
        }
    }

    /// The garment field down the front of each body has no ledge (a flat
    /// ellipsoid's distance once undercut its bounds, so the bounds' skip
    /// switched it on in a step: a ridge across the women's tops).
    #[test]
    fn the_trunk_field_has_no_ledges_down_the_front() {
        for (k, p) in archetypes().iter().enumerate() {
            let f = body_forms(p, &joints(p));
            let mut last = f32::NAN;
            // Steps of 0.9 mm: the real slopes move less than a millimetre.
            for i in 0..500 {
                let y = p.h * (0.52 + 0.0005 * i as f32);
                let (mut lo, mut hi) = (0.0f32, 0.5f32);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if eval(&f, [0.0, y, mid], grp::TRUNK | grp::PELVIS, 0, 1.8) < p.h * 0.0068 {
                        lo = mid
                    } else {
                        hi = mid
                    }
                }
                assert!(
                    last.is_nan() || (lo - last).abs() < 0.0012,
                    "person {k}: a {:.1} mm ledge at {:.3} of the height",
                    (lo - last).abs() * 1000.0,
                    y / p.h
                );
                last = lo;
            }
        }
    }

    /// A dev measure (run by hand: `cargo test ... silhouette_widths --
    /// --ignored --nocapture`): each build's silhouette as the reference
    /// camera sees it (turned `turn` to the right, looked at from the
    /// front), the runs of body across x at landmark heights, in
    /// thousandths of its height, to compare with the same runs measured
    /// on the reference picture.
    #[test]
    #[ignore]
    fn silhouette_widths() {
        let turn = 38f32.to_radians();
        for (k, p) in archetypes().iter().enumerate() {
            let b = build_person(p, p.h * 0.0052 * 1.5, 3);
            let mut all = Mesh::default();
            for m in [&b.skin, &b.top, &b.shorts, &b.hair] {
                geo::merge(&mut all, &sh::place(m, 1.0, turn, 0.0, [0.0; 3]));
            }
            let mut line = format!("fig{} h={:.2}", k + 1, p.h);
            for s in [
                0.88f32, 0.87, 0.86, 0.85, 0.84, 0.83, 0.82, 0.80, 0.78, 0.75, 0.73, 0.70, 0.68,
                0.65, 0.62, 0.57, 0.53, 0.50, 0.47, 0.44, 0.40, 0.35, 0.29, 0.25, 0.21, 0.17,
                0.13, 0.09,
            ] {
                let y = s * p.h;
                let bin = p.h * 0.002;
                let mut occ = std::collections::BTreeSet::new();
                for v in &all.vertices {
                    if (v[1] - y).abs() < p.h * 0.004 {
                        occ.insert((v[0] / bin).floor() as i32);
                    }
                }
                let mut runs: Vec<(i32, i32)> = Vec::new();
                for x in occ {
                    match runs.last_mut() {
                        Some(r) if x - r.1 <= 3 => r.1 = x,
                        _ => runs.push((x, x)),
                    }
                }
                let all_w = runs.last().map_or(0, |l| l.1) - runs.first().map_or(0, |f| f.0);
                line += &format!("\n  {s:.2} W{:<4.0}", (all_w + 1) as f32 * 2.0);
                for (a, c) in runs {
                    line += &format!("[{:.0} w{:.0}] ", a as f32 * 2.0, (c - a + 1) as f32 * 2.0);
                }
            }
            println!("{line}");
            // A shaded orthographic picture of the build as the camera sees
            // it (PPM, 420 px per metre), when `NPC_SIL_OUT` names a folder.
            if let Ok(dir) = std::env::var("NPC_SIL_OUT") {
              let hcw = mul(joints(p).head, p.h);
              let hx = hcw[2] * turn.sin();
              // The whole body, then the head close (2200 px per metre).
              for (name, sc, w, hgt, cx, ytop) in [
                  ("fig", 420.0f32, 460usize, (p.h * 1.08 * 420.0) as usize, 0.0f32, p.h * 1.08),
                  ("head", 2200.0, 520, 560, hx, hcw[1] + 0.14),
              ] {
                let mut z = vec![f32::MIN; w * hgt];
                let mut px = vec![[128u8, 128, 130]; w * hgt];
                let light = norm([-0.55, 0.6, 0.6]);
                let parts: [(&Mesh, [f32; 3]); 10] = [
                    (&b.skin, p.skin),
                    (&b.top, [0.42, 0.42, 0.43]),
                    (&b.shorts, [0.39, 0.39, 0.4]),
                    (&b.hair, mul(p.hair_rgb, 3.0)),
                    (&b.beard, mul(p.hair_rgb, 3.0)),
                    (&b.brows, mix3(p.hair_rgb, [0.03, 0.022, 0.016], 0.45)),
                    (&b.lashes, [0.02, 0.015, 0.01]),
                    (&b.whites, [0.85, 0.85, 0.82]),
                    (&b.irises, [0.1, 0.07, 0.05]),
                    (&b.lips, [p.skin[0] * 0.78, p.skin[1] * 0.52, p.skin[2] * 0.52]),
                ];
                for (m, rgb) in parts {
                    let pm = sh::place(m, 1.0, turn, 0.0, [0.0; 3]);
                    let v = &pm.vertices;
                    for t in pm.indices.chunks(3) {
                        let q = [v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]];
                        let s2 = |a: [f32; 6]| [(a[0] - cx) * sc + w as f32 * 0.5, (ytop - a[1]) * sc];
                        let (a, bb, c) = (s2(q[0]), s2(q[1]), s2(q[2]));
                        let area = (bb[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (bb[1] - a[1]);
                        if area.abs() < 1e-9 {
                            continue;
                        }
                        let x0 = a[0].min(bb[0]).min(c[0]).floor().max(0.0) as usize;
                        let x1 = (a[0].max(bb[0]).max(c[0]).ceil() as usize).min(w - 1);
                        let y0 = a[1].min(bb[1]).min(c[1]).floor().max(0.0) as usize;
                        let y1 = (a[1].max(bb[1]).max(c[1]).ceil() as usize).min(hgt - 1);
                        for yy in y0..=y1 {
                            for xx in x0..=x1 {
                                let (fx, fy) = (xx as f32 + 0.5, yy as f32 + 0.5);
                                let w0 = ((bb[0] - fx) * (c[1] - fy) - (c[0] - fx) * (bb[1] - fy)) / area;
                                let w1 = ((c[0] - fx) * (a[1] - fy) - (a[0] - fx) * (c[1] - fy)) / area;
                                let w2 = 1.0 - w0 - w1;
                                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                                    continue;
                                }
                                let zz = w0 * q[0][2] + w1 * q[1][2] + w2 * q[2][2];
                                let k = yy * w + xx;
                                if zz <= z[k] {
                                    continue;
                                }
                                z[k] = zz;
                                let n = norm([
                                    w0 * q[0][3] + w1 * q[1][3] + w2 * q[2][3],
                                    w0 * q[0][4] + w1 * q[1][4] + w2 * q[2][4],
                                    w0 * q[0][5] + w1 * q[1][5] + w2 * q[2][5],
                                ]);
                                let l = 0.38 + 0.62 * dot(n, light).max(0.0);
                                let g = |c: f32| ((c * l).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                                px[k] = [g(rgb[0]), g(rgb[1]), g(rgb[2])];
                            }
                        }
                    }
                }
                let mut out = format!("P6\n{w} {hgt}\n255\n").into_bytes();
                for c in &px {
                    out.extend_from_slice(c);
                }
                let _ = std::fs::write(format!("{dir}/{name}{}.ppm", k + 1), out);
              }
            }
        }
    }

    #[test]
    fn the_field_is_negative_inside_the_chest_and_positive_outside() {
        let p = archetypes()[0];
        let j = joints(&p);
        let f = body_forms(&p, &j);
        assert!(eval(&f, [0.0, p.h * 0.7, 0.0], grp::ALL, 0, 1.0) < 0.0);
        assert!(eval(&f, [0.0, p.h * 0.7, p.h * 0.3], grp::ALL, 0, 1.0) > 0.0);
    }
}
