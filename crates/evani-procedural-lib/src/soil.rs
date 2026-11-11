//! Dark loam (the Asset Studio dark loam session, column 69, 2026-11-10): the
//! catalogue's `ground.soil.loam` ("Soil, dark loam": crumbly dark brown with
//! small clods) shown the way a studio shows a soil, as a tipped heap on a
//! floor, so a studio shot, the Material library or a game shows the
//! material at its real scale.
//!
//! The heap is real geometry at three scales. Its body is one surface: a
//! rounded peak a little off centre, sides at a moist loam's angle of
//! repose, a thin apron at the foot that runs out under the floor, with
//! broad slumps, clods half sunk in it (blocks of three sizes, more on the
//! lower slopes) and low lumps of about two centimetres. Past the rim the
//! surface dips under the floor and only its clods and lumps stand out of
//! it, so the foot breaks up into loose bits instead of ending in a line.
//! On it and round it lie separate lumps: a few big clods broken to broad
//! flat faces, medium ones, a few hundred crumbs and grains scattered over
//! the slopes and out over the floor, and short pale bits of root and straw.
//!
//! The paint is the catalogue row itself ([`presets::GROUND_SOIL_LOAM`], a
//! game that lists the `ground` recipe draws it the same) in a few tones, as
//! the recipe's crumb (its cracks at their finest with clump over 0.8: fine
//! porous earth, a pattern in space, so it keeps its size on every slope:
//! lumps of 7 and 3 mm, crumbs, grains of 2 mm with a tone each, pores, and
//! the relief's own shade towards the light), so everything under a
//! centimetre comes from the recipe and everything over it is geometry,
//! smooth at the size of its faces (round 3: a mesh that tried to hold the
//! grain read as folded flakes). Two kinds of paint keep the soil's parts
//! apart: loose earth (clump 1) on the body, the crumbs and the grains, a
//! compact face (clump 0.85: low lumps, few pores) on the clods.
//!
//! [`heap`] is the whole heap at the origin, standing on y = 0.

use std::collections::HashMap;
use std::f32::consts::TAU;

use evani_procedural::Part;
use evani_procedural::noise::{cells2_id, fbm, hash, perlin2, perlin3, smoothstep};
use evani_procedural::surface::{Surface, presets, srgb_to_linear};
use evani_render::mesh::Mesh;

/// The heap's base radius (m) at the default size.
pub const RADIUS: f32 = 0.43;
/// Its height (m).
pub const HEIGHT: f32 = 0.345;
/// Where the peak stands off the base's centre (m: x, z), so one side is
/// longer and gentler than the other, as a tipped heap's.
pub const PEAK: [f32; 2] = [0.03, -0.01];

/// How many paints the heap uses.
const TONES: usize = 4;

/// Which paint a part takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// The heap's body and the lumps of its own colour.
    Body = 0,
    /// Drier, a little lighter clods.
    Clod = 1,
    /// Damp, darker clods and grains.
    Damp = 2,
    /// Pale bits of root and straw.
    Fleck = 3,
}

/// A colour from sRGB bytes into colour `k` of `s`.
fn col(s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.set_colour(k, [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])])
}

/// The soil's colours (sRGB bytes): the body, the drier clods, the damp
/// ones, the fibres of root and straw.
const BODY: [u8; 3] = [104, 83, 68];
const CLOD: [u8; 3] = [114, 91, 75];
const DAMP: [u8; 3] = [90, 72, 59];
const FIBRE: [u8; 3] = [160, 128, 96];

/// The paint of each part (index = [`Tone`]): the catalogue's loam, fully
/// rough and without stones, as the ground recipe's crumb (its cracks extra
/// at the finest crack scale, 50 per metre, with clump over 0.8: fine
/// porous earth, lumps of 7 and 3 mm under crumbs and 2 mm grains, dark
/// pores and hollows, the pale grains in colour B, a warm dry earth). Clump
/// at 1 is the loose earth of the body and the damp patches; the clods take
/// it at 0.85, a compact face with the grain and few pores, so a clod reads
/// apart from the earth it lies in. The fibres take the plain soil paint in
/// their own colour.
fn surfaces(seed: u32) -> [Surface; TONES] {
    let loam = presets::GROUND_SOIL_LOAM.seed(seed).number(0, 1.0).number(1, 0.0).number(2, 0.0);
    let crumb = |a: [u8; 3], b: [u8; 3], clump: f32| col(col(loam, 0, a), 1, b).extras(0x04).number(6, clump).number(7, 1.0);
    [crumb(BODY, [158, 117, 93], 1.0), crumb(CLOD, [164, 124, 99], 0.85), crumb(DAMP, [138, 102, 82], 1.0), col(loam, 0, FIBRE).extras(0).number(6, 0.3)]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] }
}

/// Seeded numbers in [0, 1), one after another.
struct Rng(u32);

impl Rng {
    fn unit(&mut self) -> f32 {
        self.0 = hash(self.0.wrapping_add(0x9e37_79b9));
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }
}

/// Add a triangle with these corner normals, wound to agree with them.
fn tri(m: &mut Mesh, p: [[f32; 3]; 3], n: [[f32; 3]; 3]) {
    let f = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    if dot(f, f) < 1e-24 {
        return;
    }
    let order = if dot(f, add(add(n[0], n[1]), n[2])) < 0.0 { [0, 2, 1] } else { [0, 1, 2] };
    for i in order {
        m.indices.push(m.vertices.len() as u32);
        m.vertices.push([p[i][0], p[i][1], p[i][2], n[i][0], n[i][1], n[i][2]]);
    }
}

/// A unit lump: an icosahedron split `splits` times, its corners on the unit
/// sphere, every face wound outwards.
fn ball(splits: u32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let p = (1.0 + 5f32.sqrt()) * 0.5;
    let mut v: Vec<[f32; 3]> = [
        [-1.0, p, 0.0],
        [1.0, p, 0.0],
        [-1.0, -p, 0.0],
        [1.0, -p, 0.0],
        [0.0, -1.0, p],
        [0.0, 1.0, p],
        [0.0, -1.0, -p],
        [0.0, 1.0, -p],
        [p, 0.0, -1.0],
        [p, 0.0, 1.0],
        [-p, 0.0, -1.0],
        [-p, 0.0, 1.0],
    ]
    .iter()
    .map(|&q| norm(q))
    .collect();
    let mut f: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..splits {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut next = Vec::with_capacity(f.len() * 4);
        for t in &f {
            let mut m = [0u32; 3];
            for (e, slot) in m.iter_mut().enumerate() {
                let (a, b) = (t[e], t[(e + 1) % 3]);
                *slot = *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    let (pa, pb) = (v[a as usize], v[b as usize]);
                    v.push(norm(add(pa, pb)));
                    (v.len() - 1) as u32
                });
            }
            next.extend([[t[0], m[0], m[2]], [t[1], m[1], m[0]], [t[2], m[2], m[1]], [m[0], m[1], m[2]]]);
        }
        f = next;
    }
    for t in &mut f {
        let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
        if dot(cross(sub(b, a), sub(c, a)), add(add(a, b), c)) < 0.0 {
            t.swap(1, 2);
        }
    }
    (v, f)
}

/// How far the base reaches from the peak along the unit direction
/// (`eu`, `ev`), in metres at the default size.
fn reach(eu: f32, ev: f32) -> f32 {
    let ae = PEAK[0] * eu + PEAK[1] * ev;
    (-ae + (ae * ae + RADIUS * RADIUS - PEAK[0] * PEAK[0] - PEAK[1] * PEAK[1]).max(0.0).sqrt()).max(1e-4)
}

/// The heap's smooth profile over how far out a point is (0 the peak, 1 the
/// base's rim), as a share of its height: a rounded top whose sides steepen
/// downwards (a tipped mound, fuller than a cone), a short thin apron at the
/// foot, under the floor past the rim.
fn profile(t: f32) -> f32 {
    let q = t / 0.92;
    let cone = 1.08 - (0.49 * q * q + 0.0064).sqrt() - 0.3754 * q * q;
    let apron = 0.09 * (1.0 - t);
    0.5 * (cone + apron + ((cone - apron) * (cone - apron) + 0.0009).sqrt())
}

/// One kind of loose lump and where it lies.
struct Scatter {
    /// How many at the default density.
    count: f32,
    /// How finely its ball is split (0: 20 faces, 1: 80, 2: 320).
    splits: usize,
    /// Its half size (m), smallest and largest.
    size: [f32; 2],
    /// How far out it lies (0 the peak, 1 the rim), nearest and furthest.
    out: [f32; 2],
    /// Over 1: more of them near, under 1: more of them far.
    bias: f32,
}

/// The loose lumps: big clods on the lower slopes and at the foot, a few on
/// the peak, medium clods, crumbs over the slopes and the floor, and grains
/// on the floor round the foot.
const SCATTER: [Scatter; 5] = [
    Scatter { count: 22.0, splits: 2, size: [0.022, 0.04], out: [0.32, 1.04], bias: 0.75 },
    Scatter { count: 6.0, splits: 1, size: [0.008, 0.016], out: [0.0, 0.14], bias: 1.0 },
    Scatter { count: 70.0, splits: 1, size: [0.009, 0.02], out: [0.15, 1.12], bias: 0.7 },
    Scatter { count: 420.0, splits: 0, size: [0.004, 0.009], out: [0.1, 1.13], bias: 0.7 },
    Scatter { count: 380.0, splits: 0, size: [0.0018, 0.0038], out: [0.96, 1.24], bias: 2.2 },
];

/// How far the grain lifts or sinks each corner of the body (m): very
/// little (round 3: at 2 mm every face of the mesh showed as a folded
/// flake; the grain is the paint's).
const GRAIN: f32 = 0.0006;
/// How far the grain moves a corner sideways, as a share of the grid's
/// step, so the mesh's corners don't line up in rows.
const SHUFFLE: f32 = 0.36;
/// How much of a face's own normal the body's corners take (0: smooth;
/// more: every face a facet). Little: the body's grain is the paint's.
const FACET: f32 = 0.06;
/// A loose lump is smooth across an edge whose two faces lie within this
/// (the cosine of 38 degrees) and keeps the edge beyond it: its fracture
/// faces are cut into its shape and stay flat.
const CREASE: f32 = 0.788;

/// A seeded number in [-0.5, 0.5) for corner `q`, channel `c`.
fn jitter(seed: u32, q: usize, c: u32) -> f32 {
    let r = hash(seed ^ (q as u32).wrapping_mul(0x9e37_79b9) ^ c.wrapping_mul(0x85eb_ca6b));
    (r >> 8) as f32 / (1u32 << 24) as f32 - 0.5
}

/// Add a triangle whose corners lean from their smooth normals `n` to the
/// face's own by `lean`.
fn facet(m: &mut Mesh, p: [[f32; 3]; 3], n: [[f32; 3]; 3], lean: f32) {
    let f = norm(cross(sub(p[1], p[0]), sub(p[2], p[0])));
    let f = if dot(f, add(add(n[0], n[1]), n[2])) < 0.0 { [-f[0], -f[1], -f[2]] } else { f };
    tri(m, p, n.map(|s| norm(add(norm(s), [f[0] * lean, f[1] * lean, f[2] * lean]))));
}

/// The heap: its size and seed.
#[derive(Debug, Clone, Copy)]
struct Heap {
    /// Its size over the default.
    k: f32,
    /// Its height over the default's at that size.
    hk: f32,
    seed: u32,
}

impl Heap {
    fn new(radius: f32, height: f32, seed: u32) -> Self {
        let k = (radius / RADIUS).max(1e-3);
        Self { k, hk: height / (HEIGHT * k), seed }
    }

    /// How far out (`u`, `v`) is (m at the default size): 0 at the peak, 1
    /// on the base's rim, more beyond; the rim wanders a little.
    fn out(&self, u: f32, v: f32) -> f32 {
        let s = self.seed;
        let wu = u + 0.045 * perlin2(u * 2.6 + 3.1, v * 2.6, s ^ 0x11);
        let wv = v + 0.045 * perlin2(u * 2.6, v * 2.6 + 7.7, s ^ 0x12);
        let (du, dv) = (wu - PEAK[0], wv - PEAK[1]);
        let d = (du * du + dv * dv).sqrt();
        if d < 1e-6 {
            return 0.0;
        }
        d / reach(du / d, dv / d)
    }

    /// Clods half sunk in the surface, three sizes: more on the lower
    /// slopes and at the foot, thinning out past the rim. Each is a block: a
    /// broad top that leans its own way, straight where it meets a
    /// neighbour, as tall as its width allows (no flank steeper than the
    /// mesh holds without long thin faces). Returns its height (m) and how
    /// far the point is on a clod of the two larger sizes (0..1), which
    /// takes the compact paint.
    fn clods(&self, u: f32, v: f32, t: f32) -> (f32, f32) {
        let place = (0.12 + 0.88 * smoothstep(0.2, 0.75, t)) * (1.0 - smoothstep(1.02, 1.38, t));
        let (mut h, mut on) = (0.0, 0.0f32);
        for (n, (freq, share)) in [(8.0f32, 0.3f32), (17.0, 0.34), (36.0, 0.4)].into_iter().enumerate() {
            let (f1, f2, id) = cells2_id(u * freq, v * freq, self.seed ^ (0x31 + n as u32));
            if id > share * place {
                continue;
            }
            let (r, r2) = ((id * 91.7).fract(), (id * 517.3).fract());
            let rad = 0.26 + 0.2 * r;
            let d = f1 / rad;
            if d < 1.0 {
                // A straight flank up to a top with an edge, not a dome.
                let flank = ((1.0 - d) / if n == 2 { 0.7 } else { 0.5 }).min((f2 - f1) / 0.28).clamp(0.0, 1.0);
                let lean = 1.0 + 0.25 * perlin2(u * freq * 1.7 + 3.0 * n as f32, v * freq * 1.7, self.seed ^ 0x37);
                h += 0.6 * rad / freq * (0.7 + 0.3 * r2) * flank * lean;
                if n < 2 {
                    on = on.max(smoothstep(0.15, 0.45, flank));
                }
            }
        }
        (h, on)
    }

    /// The breaks between the soil's aggregates (m, how far the surface
    /// sinks): the body parts along stretches of the bent borders of cells
    /// of two sizes (about 7 and 3 cm), a soft-sided gap here, a closed
    /// contact there, never a line round a whole cell; every small
    /// aggregate stands a little proud of its border.
    fn breaks(&self, u: f32, v: f32) -> f32 {
        let s = self.seed;
        let (wu, wv) = (u + 0.011 * perlin2(u * 31.0, v * 31.0, s ^ 0x51), v + 0.011 * perlin2(u * 31.0 + 9.0, v * 31.0, s ^ 0x52));
        let mut h = 0.0;
        for (n, (freq, deep, wide)) in [(15.0f32, 0.011f32, 0.24f32), (36.0, 0.006, 0.3)].into_iter().enumerate() {
            let (f1, f2, id) = cells2_id(wu * freq, wv * freq, s ^ (0x53 + n as u32));
            let open = smoothstep(-0.05, 0.25, perlin2(u * freq * 0.8 + 4.0, v * freq * 0.8, s ^ (0x55 + n as u32)));
            h += deep * (1.0 - smoothstep(0.0, wide, f2 - f1)) * open * (0.35 + 0.65 * (id * 7.3).fract());
            if n == 1 {
                h -= 0.004 * (id * 13.7).fract() * smoothstep(0.0, 0.3, f2 - f1);
            }
        }
        h
    }

    /// The crumb: a low rounded lump in six of ten cells of a 2 cm lattice,
    /// none far past the rim (the finer crumb is the paint's).
    fn crumbs(&self, u: f32, v: f32, t: f32) -> f32 {
        let (f1, f2, id) = cells2_id(u * 48.0, v * 48.0, self.seed ^ 0x41);
        if id > 0.6 * (1.0 - smoothstep(1.0, 1.22, t)) {
            return 0.0;
        }
        let (r, r2) = ((id * 91.7).fract(), (id * 517.3).fract());
        let d = f1 / (0.3 + 0.18 * r);
        if d >= 1.0 {
            return 0.0;
        }
        0.008 * (0.5 + 0.5 * r2) * (1.0 - smoothstep(0.25, 1.0, d)) * smoothstep(0.0, 0.2, f2 - f1)
    }

    /// The surface at (x, z): its height in metres over the floor (under 0:
    /// no soil stands there) and how far the point is on a clod (0..1).
    fn relief(&self, x: f32, z: f32) -> (f32, f32) {
        let (u, v) = (x / self.k, z / self.k);
        let t = self.out(u, v);
        let s = self.seed;
        let body = HEIGHT * self.hk * profile(t);
        // Slumps and lumps, fading out as the layer thins.
        let thick = smoothstep(0.0, 0.03, body);
        let big = 0.05 * fbm(u * 4.5 + 1.7, v * 4.5 - 2.3, s ^ 0x21, 3, 2.0, 0.5);
        // The lumps are read in space, at the point on the smooth heap, so
        // one is as long down a steep side as it is wide (read over the
        // floor plan it stretched into a ridge down the slope).
        let at = |f: f32, o: f32| [u * f + o, body * f, v * f];
        let mid = 0.02 * perlin3(at(21.0, 0.0), s ^ 0x22) + 0.008 * perlin3(at(43.0, 5.0), s ^ 0x23);
        // Clumps: the earth lies in lumps of about three and one and a half
        // centimetres, creased where two of them meet (a crease here, a
        // closed contact there: the creases are the zero lines of a noise).
        let clumps = 0.016 * perlin3(at(30.0, 2.0), s ^ 0x24).abs() + 0.008 * perlin3(at(70.0, 6.0), s ^ 0x25).abs();
        let (clods, on) = self.clods(u, v, t);
        ((body + (big + mid + clumps - self.breaks(u, v)) * thick + clods + self.crumbs(u, v, t)) * self.k, on)
    }

    /// The surface's height at (x, z) in metres over the floor.
    fn at(&self, x: f32, z: f32) -> f32 {
        self.relief(x, z).0
    }

    /// The point `t` out from the peak (0 the peak, 1 the rim) at heading
    /// `a`: (x, z) in metres.
    fn spot(&self, a: f32, t: f32) -> (f32, f32) {
        let (eu, ev) = (a.cos(), a.sin());
        let d = t * reach(eu, ev);
        ((PEAK[0] + eu * d) * self.k, (PEAK[1] + ev * d) * self.k)
    }

    /// The body: a grid over the heap and its foot, drawn wherever soil
    /// stands above the floor. Three paints: the loose earth, the compact
    /// one on the clods sunk in it, and the damp (darker) one in a few
    /// patches low on the heap, where the soil lies thin on the floor.
    fn surface(&self, density: f32, bins: &mut [Mesh; TONES]) {
        let cell = 0.0046 * self.k / density.clamp(0.25, 2.0).sqrt();
        let half = RADIUS * self.k * 1.42;
        let n = ((2.0 * half / cell).round() as usize).clamp(16, 420);
        let step = 2.0 * half / n as f32;
        let w = n + 1;
        // Every corner: the surface there, then the grain (lifted or sunk and
        // moved a little sideways, each on its own), and whether it is on a
        // clod.
        let s = self.seed ^ 0x6a17;
        let (ps, on): (Vec<[f32; 3]>, Vec<bool>) = (0..w * w)
            .map(|q| {
                let x = -half + ((q % w) as f32 + SHUFFLE * jitter(s, q, 1)) * step;
                let z = -half + ((q / w) as f32 + SHUFFLE * jitter(s, q, 2)) * step;
                let (y, on) = self.relief(x, z);
                ([x, y + 2.0 * GRAIN * self.k * jitter(s, q, 3), z], on > 0.5)
            })
            .unzip();
        // A corner's smooth normal, from its neighbours' heights.
        let smooth = |i: usize, j: usize| {
            let (i0, i1) = (i.saturating_sub(1), (i + 1).min(n));
            let (j0, j1) = (j.saturating_sub(1), (j + 1).min(n));
            let (sx, sz) = ((i1 - i0) as f32 * step, (j1 - j0) as f32 * step);
            norm([-(ps[j * w + i1][1] - ps[j * w + i0][1]) / sx, 1.0, -(ps[j1 * w + i][1] - ps[j0 * w + i][1]) / sz])
        };
        for j in 0..n {
            for i in 0..n {
                let (a, b, c, d) = (ps[j * w + i], ps[j * w + i + 1], ps[(j + 1) * w + i], ps[(j + 1) * w + i + 1]);
                if a[1].max(b[1]).max(c[1]).max(d[1]) <= 0.0 {
                    continue;
                }
                let (na, nb, nc, nd) = (smooth(i, j), smooth(i + 1, j), smooth(i, j + 1), smooth(i + 1, j + 1));
                // Damp: patches of the lowest soil, a third of the foot.
                let line = 0.07 * smoothstep(0.08, 0.3, perlin2(a[0] / self.k * 9.0, a[2] / self.k * 9.0, self.seed ^ 0x71)) * self.k;
                let tone = if on[j * w + i] {
                    Tone::Clod
                } else if a[1] < line {
                    Tone::Damp
                } else {
                    Tone::Body
                };
                let m = &mut bins[tone as usize];
                // Split along the diagonal that bends less.
                if (a[1] - d[1]).abs() <= (b[1] - c[1]).abs() {
                    facet(m, [a, c, d], [na, nc, nd], FACET);
                    facet(m, [a, d, b], [na, nd, nb], FACET);
                } else {
                    facet(m, [a, c, b], [na, nc, nb], FACET);
                    facet(m, [b, c, d], [nb, nc, nd], FACET);
                }
            }
        }
    }

    /// The loose lumps ([`SCATTER`]), each a ball squashed, turned and
    /// roughened, some broken to flat faces; on the heap they sit partly
    /// sunk, on the floor they rest flat.
    fn lumps(&self, density: f32, bins: &mut [Mesh; TONES]) {
        let k = self.k;
        let mut rng = Rng(hash(self.seed ^ 0x5011));
        let balls = [ball(0), ball(1), ball(2)];
        for (kind, sc) in SCATTER.iter().enumerate() {
            let count = (sc.count * density.clamp(0.25, 2.0)).round() as usize;
            let (unit, faces) = &balls[sc.splits.min(2)];
            for _ in 0..count {
                let a = rng.range(0.0, TAU);
                let t = sc.out[0] + (sc.out[1] - sc.out[0]) * rng.unit().powf(sc.bias);
                let (x, z) = self.spot(a, t);
                // Many small ones, a few large; none of them round.
                let share = rng.unit();
                let size = k * (sc.size[0] + (sc.size[1] - sc.size[0]) * share * share);
                let half = [size * rng.range(0.7, 1.4), size * rng.range(0.5, 0.95), size * rng.range(0.7, 1.4)];
                let ground = self.at(x, z);
                let on_floor = ground <= 0.0015 * k;
                // A clod stands proud of the earth it lies in (most of it
                // shows); the crumbs lie deeper.
                let sunk = if kind < 3 { rng.range(0.15, 0.6) } else { rng.range(-0.2, 0.4) };
                let cy = if on_floor { half[1] * 0.8 } else { ground + half[1] * sunk };
                let (ys, yc) = rng.range(0.0, TAU).sin_cos();
                let (ts, tc) = rng.range(-0.5, 0.5).sin_cos();
                // Broken: flat faces cut into the ball, broad ones on every
                // clod (a block with a few fracture faces), on most crumbs.
                let clod = kind < 3;
                let broken = if rng.unit() < if clod { 0.72 } else { 0.4 } { rng.range(0.55, 1.0) } else { 0.0 };
                let broken = if clod { 0.8 + 0.2 * broken } else { broken };
                let cuts: [([f32; 3], f32); 5] = std::array::from_fn(|_| {
                    let n = norm([rng.range(-1.0, 1.0), rng.range(-0.6, 1.0), rng.range(-1.0, 1.0)]);
                    (n, rng.range(0.5, 0.82) * if clod { 0.82 } else { 1.0 })
                });
                let ls = hash(self.seed ^ (rng.unit() * 16_000_000.0) as u32);
                let o = (ls % 97) as f32 * 0.37;
                let pick = rng.unit();
                // The clods take the compact paint; the crumbs and grains
                // are loose earth, some of it damp.
                let tone = if clod {
                    Tone::Clod
                } else if pick < 0.6 {
                    Tone::Body
                } else if pick < 0.8 && kind == 3 {
                    Tone::Clod
                } else {
                    Tone::Damp
                };
                let lumpy = if clod { 0.2 } else { 0.3 };
                let pts: Vec<[f32; 3]> = unit
                    .iter()
                    .map(|d| {
                        let mut r = 1.0
                            + lumpy * perlin3([d[0] * 1.7 + o, d[1] * 1.7, d[2] * 1.7], ls)
                            + 0.15 * perlin3([d[0] * 4.1, d[1] * 4.1 + o, d[2] * 4.1], ls ^ 1)
                            + 0.06 * perlin3([d[0] * 9.3, d[1] * 9.3, d[2] * 9.3 + o], ls ^ 2);
                        if broken > 0.0 {
                            for (n, off) in &cuts {
                                let c = dot(*d, *n);
                                if c > 0.05 {
                                    r = r.min(r + (off / c - r) * broken);
                                }
                            }
                        }
                        let l = [d[0] * r * half[0], d[1] * r * half[1], d[2] * r * half[2]];
                        let l = [l[0], l[1] * tc - l[2] * ts, l[1] * ts + l[2] * tc];
                        let l = [l[0] * yc + l[2] * ys, l[1], l[2] * yc - l[0] * ys];
                        let y = cy + l[1];
                        [x + l[0], if on_floor { y.max(0.0004 * k) } else { y }, z + l[2]]
                    })
                    .collect();
                // Each face's own normal (by area), and the faces round
                // every corner.
                let fnm: Vec<[f32; 3]> = faces.iter().map(|f| cross(sub(pts[f[1] as usize], pts[f[0] as usize]), sub(pts[f[2] as usize], pts[f[0] as usize]))).collect();
                let mut round: Vec<Vec<u32>> = vec![Vec::new(); pts.len()];
                for (q, f) in faces.iter().enumerate() {
                    for &i in f {
                        round[i as usize].push(q as u32);
                    }
                }
                // A corner's normal on a face: the faces round the corner
                // that lie within 38 degrees of this one, so a fracture
                // keeps its edge and the rest of the lump is smooth. A crumb
                // is smooth all round (its few faces would show as a gem's).
                let crease = if clod { CREASE } else { -2.0 };
                let m = &mut bins[tone as usize];
                for (q, f) in faces.iter().enumerate() {
                    let own = norm(fnm[q]);
                    let at = |i: u32| {
                        let mut n = [0.0f32; 3];
                        for &g in &round[i as usize] {
                            if dot(norm(fnm[g as usize]), own) > crease {
                                n = add(n, fnm[g as usize]);
                            }
                        }
                        norm(n)
                    };
                    tri(m, [pts[f[0] as usize], pts[f[1] as usize], pts[f[2] as usize]], [at(f[0]), at(f[1]), at(f[2])]);
                }
            }
        }
    }

    /// Fine roots and bits of straw, a few of them: thin strips lying on the
    /// surface, the roots long, hair thin and bent with both ends in the
    /// soil, the straws short and straight.
    fn flecks(&self, density: f32, m: &mut Mesh) {
        let k = self.k;
        let mut rng = Rng(hash(self.seed ^ 0xf1ec));
        let count = (70.0 * density.clamp(0.25, 2.0)).round() as usize;
        for _ in 0..count {
            let (mut x, mut z) = self.spot(rng.range(0.0, TAU), rng.unit().sqrt() * 1.03);
            let root = rng.unit() < 0.65;
            let (len, wid, segs) =
                if root { (k * rng.range(0.012, 0.03), k * rng.range(0.0005, 0.0008), 4usize) } else { (k * rng.range(0.004, 0.01), k * rng.range(0.0008, 0.0013), 1) };
            let mut heading = rng.range(0.0, TAU);
            let bend = if root { rng.range(-0.5, 0.5) } else { 0.0 };
            let mut last: Option<[[f32; 3]; 2]> = None;
            for i in 0..=segs {
                let (dz, dx) = heading.sin_cos();
                // A root's ends dip into the soil; its middle lies on it.
                let lift = k * if root && (i == 0 || i == segs) { -0.0006 } else { 0.0013 };
                let side = |w: f32| {
                    let (sx, sz) = (x - dz * w * wid * 0.5, z + dx * w * wid * 0.5);
                    [sx, self.at(sx, sz) + lift, sz]
                };
                let here = [side(-1.0), side(1.0)];
                if let Some(back) = last {
                    let n = norm(cross(sub(here[0], back[0]), sub(back[1], back[0])));
                    let n = if n[1] < 0.0 { [-n[0], -n[1], -n[2]] } else { n };
                    tri(m, [back[0], here[0], back[1]], [n; 3]);
                    tri(m, [here[0], here[1], back[1]], [n; 3]);
                }
                last = Some(here);
                x += dx * len / segs as f32;
                z += dz * len / segs as f32;
                heading += bend + rng.range(-0.22, 0.22);
            }
        }
    }
}

/// The heap of dark loam: `radius` at its base (m), `height` high, standing
/// on y = 0 with its base's centre at the origin. `density` thins the mesh
/// (1 = the default: about 4.6 mm between the body's corners, the whole heap
/// under 100,000 triangles; more goes over that).
pub fn heap(radius: f32, height: f32, seed: u32, density: f32) -> Vec<Part> {
    let h = Heap::new(radius, height, seed);
    let mut bins: [Mesh; TONES] = Default::default();
    h.surface(density, &mut bins);
    h.lumps(density, &mut bins);
    h.flecks(density, &mut bins[Tone::Fleck as usize]);
    bins.into_iter().zip(surfaces(seed)).filter(|(m, _)| !m.indices.is_empty()).map(|(m, s)| Part::surfaced(m, s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heap_is_whole_and_within_the_budget() {
        let parts = heap(RADIUS, HEIGHT, 69, 1.0);
        assert_eq!(parts.len(), TONES, "every tone drawn");
        let tris: u32 = parts.iter().map(|p| p.mesh.triangles()).sum();
        println!("soil heap triangles: {tris}");
        assert!(tris > 40_000 && tris < 100_000, "triangles {tris}");
        for p in &parts {
            assert!(p.mesh.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())));
            assert!(p.mesh.indices.iter().all(|&i| (i as usize) < p.mesh.vertices.len()));
            assert!(p.mesh.vertices.iter().all(|v| v[1] < HEIGHT * 1.4 && v[0].abs() < RADIUS * 1.7 && v[2].abs() < RADIUS * 1.7), "inside its box");
        }
    }

    #[test]
    fn the_heap_peaks_and_runs_out_under_the_floor() {
        let h = Heap::new(RADIUS, HEIGHT, 69);
        let top = h.at(PEAK[0], PEAK[1]);
        assert!((top - HEIGHT).abs() < 0.09, "top {top}");
        // Half way down every side the soil stands well above the floor.
        for q in 0..12 {
            let (x, z) = h.spot(q as f32 / 12.0 * TAU, 0.5);
            assert!(h.at(x, z) > 0.08, "side {q}");
        }
        // Far past the rim nothing stands.
        for q in 0..24 {
            let (x, z) = h.spot(q as f32 / 24.0 * TAU, 1.6);
            assert!(h.at(x, z) < 0.0, "beyond {q}");
        }
    }

    #[test]
    fn the_same_seed_makes_the_same_heap() {
        let (a, b) = (heap(RADIUS, HEIGHT, 5, 0.5), heap(RADIUS, HEIGHT, 5, 0.5));
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(&b) {
            assert_eq!(p.mesh.vertices, q.mesh.vertices);
            assert_eq!(p.mesh.indices, q.mesh.indices);
        }
        let c = heap(RADIUS, HEIGHT, 6, 0.5);
        assert_ne!(a[0].mesh.vertices, c[0].mesh.vertices, "another seed, another heap");
    }

    #[test]
    fn every_lump_faces_outwards() {
        for splits in 0..3 {
            let (v, f) = ball(splits);
            assert_eq!(f.len(), 20 * 4usize.pow(splits));
            for t in &f {
                let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
                assert!(dot(cross(sub(b, a), sub(c, a)), add(add(a, b), c)) > 0.0);
            }
        }
    }
}
