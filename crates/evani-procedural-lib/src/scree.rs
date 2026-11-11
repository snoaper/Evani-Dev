//! Scree slopes (rocks-scree-slopes; the Asset Studio scree slopes session,
//! column 27, 2026-11-08): a photoreal talus slope at the foot of a rock
//! wall, as a player stands on it.
//!
//! A wall of foliated schist stands at the slope's head: one continuous
//! broken face of beds stepped back one by one, cut by joints into blocks
//! with deep fissures between them, each block's face a planar break of its
//! own, the beds' partings dark lines, the rock grey with rust in patches.
//! Rock ribs run down from it between the chutes. The scree lies at its
//! angle of repose (34 degrees at the default), steepest under the wall and
//! easing towards its toe, heaped into cones under the chutes, and runs out
//! past the toe into a hummocky apron where the biggest blocks come to rest.
//! The slope's foot may turn round (`curve`) so the talus lines a cirque's
//! bowl. On the gravel bed lie stones of every size as real geometry:
//! angular slabs and blocks lying flat along the slope, sunk among each
//! other, small sharp chips between them, the big blocks further down (fall
//! sorting); each a broken stone of the jagged rock formation's kind
//! (`crate::jagged_rocks::blade`: fracture faces, stepped foliation and
//! fresh chips on the big ones). Mats of dwarf shrub hold on where the scree
//! is still.
//!
//! The stones are four rocks in the stone recipe's variant 11 (as the jagged
//! rock formation's): a grey schist, a dark schist with white quartz veins,
//! a grey stone with rusty lichen and iron patches and a pale quartzite; the
//! bed is the ground recipe's scree gravel. A game that lists `stone` and
//! `ground` draws what the library shows.
//!
//! Detail follows the player (`focus_x`, `focus_z`, `focus_radius`, as the
//! mountains' forest): round that point the ground's mesh is finer and the
//! stones lie clast on clast down to chips; further out the bed's paint
//! carries the fines and only cobbles and blocks stand on it. The asset is
//! built in the slope's own frame (the slope falls towards +x, its foot runs
//! along z, the toe at y = 0) and then bent round by `curve`; the focus is
//! in that frame ([`Slope::warp`] gives where a point of it lands).

use crate::jagged_rocks::blade;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};
use evani_procedural::{Output, ParamSpec as P, Part, Values, detail, noise};

/// The library colour param's default: at it every stone keeps its own.
pub const COLOR: [f32; 3] = [0.3, 0.29, 0.27];

pub const PARAMS: [P; 14] = [
    P::num("size", 120.0, 6.0, 400.0).unit("m").describe("Width of the slope along its foot (m)"),
    P::num("length", 48.0, 4.0, 200.0).unit("m").describe("How far the scree runs out from the wall's foot to its toe (m, level)"),
    P::num("slope", 0.68, 0.2, 1.0).describe("Steepness as rise per metre (0.68: scree's angle of repose, 34 degrees)"),
    P::num("cliff", 14.0, 0.0, 60.0).unit("m").describe("Height of the rock wall the scree has fallen from (m; 0: none)"),
    P::num("stones", 1.0, 0.0, 2.0).describe("How thickly stones and blocks lie on the gravel (0: bare gravel)"),
    P::num("block", 1.6, 0.3, 4.0).unit("m").describe("Largest fallen block (m)"),
    P::color("color", COLOR).describe("Rock colour (at its default each stone keeps its own)"),
    P::num("rust", 0.35, 0.0, 1.0).describe("Share of rusty, lichen-stained stones"),
    P::num("moss", 0.4, 0.0, 1.0).describe("Mats of dwarf shrub on the stiller scree (0: none)"),
    P::num("focus_x", 0.0, -200.0, 200.0).unit("m").describe("Where a player stands (x across the slope, m from the middle, in the slope's own frame): round it every stone is drawn"),
    P::num("focus_z", 0.0, -200.0, 200.0).unit("m").describe("Where a player stands (z along the slope's foot, m from the middle)"),
    P::num("focus_radius", 24.0, 4.0, 80.0).unit("m").describe("How far round the player's point the ground is fine and the stones lie close"),
    P::num("curve", 0.0, 0.0, 180.0).unit("deg").describe("How far the slope's foot turns round over its width (degrees): 0 straight, more lines a cirque's bowl"),
    P::num("apron", 20.0, 0.0, 150.0).unit("m").describe("A hummocky debris apron past the toe, where the biggest blocks come to rest (m)"),
];

/// The library's triangle limit for one asset (src/tests.rs), less a margin.
const BUDGET: usize = 385_000;

/// The slope's layout, from its params.
#[derive(Clone)]
pub struct Slope {
    /// Width along the foot, run from head to toe (m), rise per metre.
    pub size: f32,
    pub length: f32,
    pub k: f32,
    /// The wall's height (m) and how far it stands back from the head (m).
    pub cliff: f32,
    depth: f32,
    /// The apron past the toe (m).
    pub apron: f32,
    /// Chutes between the ribs: how many, and the seed.
    chutes: u32,
    seed: u32,
    /// The bend: radians per metre along the foot (0: straight) and the
    /// radius of the bowl's middle from x = 0 (m).
    kappa: f32,
    r0: f32,
    /// The ground's lattice lines across (x) and along (z) the slope, finer
    /// round the player.
    xs: Vec<f32>,
    zs: Vec<f32>,
}

/// Lattice lines from `lo` to `hi`: `fine` m apart within `half` of `at`,
/// `coarse` m elsewhere.
fn lines(lo: f32, hi: f32, at: f32, half: f32, fine: f32, coarse: f32) -> Vec<f32> {
    let mut v = vec![lo];
    let mut x = lo;
    while x < hi - 1e-3 {
        let step = if (x - at).abs() < half { fine } else { coarse };
        x = (x + step).min(hi);
        v.push(x);
    }
    v
}

impl Slope {
    pub fn new(v: &Values, seed: u32) -> Self {
        let (size, length, k, cliff) = (v.f("size"), v.f("length"), v.f("slope").clamp(0.2, 1.0), v.f("cliff").max(0.0));
        let apron = v.f("apron").max(0.0);
        let chutes = ((size / 34.0).round() as u32).clamp(1, 12);
        let depth = if cliff > 0.0 { cliff * 0.3 + 2.5 } else { 0.0 };
        // The bend: the bowl's middle lies downhill, past the apron.
        let curve = v.f("curve").clamp(0.0, 180.0).to_radians();
        let (kappa, r0) = if curve > 1e-3 {
            let r0 = (size / curve).max(length * 0.5 + apron + 10.0);
            (1.0 / r0, r0)
        } else {
            (0.0, 0.0)
        };
        // The ground's lattice: fine round the player (0.5 m across the
        // slope, 0.7 m along it at High), coarse elsewhere, at most about
        // 28 000 cells.
        let (x0, x1) = (-length * 0.5 - depth - 4.0, length * 0.5 + apron);
        let (z0, z1) = (-size * 0.5, size * 0.5);
        let (fx, fz, fr) = (v.f("focus_x"), v.f("focus_z"), v.f("focus_radius").max(1.0));
        let low = if v.density() >= 0.6 { 1.0 } else { 1.8 };
        let mut f = low;
        let (xs, zs) = loop {
            let xs = lines(x0, x1, fx, fr, 0.5 * f, 1.6 * f);
            let zs = lines(z0, z1, fz, fr * 1.2, 0.7 * f, 2.2 * f);
            if (xs.len() * zs.len()) as f32 <= 28_000.0 || f > 20.0 {
                break (xs, zs);
            }
            f *= 1.15;
        };
        Self { size, length, k, cliff, depth, apron, chutes, seed, kappa, r0, xs, zs }
    }

    /// The wall's height over the head at z: an uneven crest (three
    /// quarters to one and a quarter of `cliff`), tapering away over the
    /// last eighth of the foot at each end.
    pub fn wall_h(&self, z: f32) -> f32 {
        let end = self.size * 0.5 - z.abs();
        let t = (end / (self.size * 0.12).max(1.0)).clamp(0.0, 1.0);
        let taper = t * t * (3.0 - 2.0 * t);
        self.cliff * taper * (0.75 + 0.5 * (noise::fbm2(z / 30.0, 0.37, self.seed ^ 0x0c4e, 3, 2.0, 0.5) + 0.5).clamp(0.0, 1.0))
    }

    /// The scree's head (x) and its height there.
    pub fn head(&self) -> (f32, f32) {
        (-self.length * 0.5, 0.875 * self.length * self.k)
    }

    /// The x a rock rib between chutes `i - 1` and `i` comes down at its
    /// head (z), and how far down the slope (share of the run) it reaches.
    fn rib(&self, i: u32) -> (f32, f32) {
        let w = self.size / self.chutes as f32;
        let z = -self.size * 0.5 + w * i as f32 + w * 0.18 * (sh::rnd(self.seed, i, 401) - 0.5);
        (z, 0.12 + 0.22 * sh::rnd(self.seed, i, 402))
    }

    /// The scree bed's height at (x, z) in the slope's frame: a concave
    /// profile (steep under the wall, easing to the toe), cones under the
    /// chutes, hummocks; past the toe the apron's hummocks and low moraine
    /// ridges, settling to the ground at its edge; behind the head, the
    /// ground under the wall and over it.
    pub fn bed(&self, x: f32, z: f32) -> f32 {
        let (hx, top) = self.head();
        let s = self.seed;
        let toe = self.length * 0.5;
        let t = ((x - hx) / self.length).clamp(0.0, 1.0);
        // Cones: each chute heaps its scree into a cone, narrow and high under
        // the wall, spreading and merging towards the toe.
        let w = self.size / self.chutes as f32;
        let amp = (0.012 * self.length).min(w * 0.03);
        let mut cones = 0.0;
        for i in 0..self.chutes {
            let zc = -self.size * 0.5 + w * (i as f32 + 0.5) + w * 0.2 * (sh::rnd(s, i, 403) - 0.5);
            let half = w * (0.35 + 0.45 * t) * (0.8 + 0.4 * sh::rnd(s, i, 404));
            let wander = w * 0.12 * noise::fbm2(x / 25.0, i as f32 * 3.1, s ^ 0x41, 2, 2.0, 0.5);
            let u = ((z - zc - wander) / half).abs();
            if u < 1.0 {
                cones += amp * (0.6 + 0.8 * sh::rnd(s, i, 405)) * (1.0 - u * u) * (1.0 - u * u) * (1.0 - t * 0.7);
            }
        }
        // Hummocks and the bed's own unevenness (lobes a few metres across,
        // a finer ripple of settled stones), lifted so they stay above the
        // ground at the toe.
        let common = 0.4 + cones + 0.35 * noise::fbm2(x / 7.0, z / 7.0, s ^ 0x5c1e, 3, 2.0, 0.5) + 0.06 * noise::fbm2(x / 2.2, z / 2.2, s ^ 0x77a1, 2, 2.0, 0.5);
        let h = if x < hx - 1.5 && self.cliff > 0.0 {
            // Under the wall's face, then the ground over it.
            let b = ((hx - 1.5 - x) / self.depth).clamp(0.0, 1.0);
            top + common + self.wall_h(z) * b * b * (3.0 - 2.0 * b) + 0.6 * noise::fbm2(x / 5.0, z / 5.0, s ^ 0x3e9, 2, 2.0, 0.5) * b
        } else if x <= toe {
            self.length * self.k * (1.15 * (1.0 - t) - 0.275 * (1.0 - t * t)) + common
        } else {
            // The apron: the hummocks run on, low moraine ridges along the
            // toe, all settling to the ground at its edge.
            let a = ((x - toe) / self.apron.max(1e-3)).clamp(0.0, 1.0);
            let ridge = 0.7 * noise::ridged2(x / 14.0, z / 30.0, s ^ 0x6d1, 3) * (a * 4.0).min(1.0);
            (common + ridge) * (1.0 - a * a * (3.0 - 2.0 * a))
        };
        h.max(0.0)
    }

    /// The ground's height at (x, z) as drawn: the bed at the lattice's
    /// corners, laid in the same triangles as [`Slope::grid`], so a stone set
    /// on it sits exactly on the drawn ground.
    pub fn ground(&self, x: f32, z: f32) -> f32 {
        let cell = |v: &[f32], p: f32| -> (usize, f32) {
            let i = v.partition_point(|q| *q <= p).saturating_sub(1).min(v.len().saturating_sub(2));
            let (a, b) = (v[i], v[i + 1]);
            (i, ((p - a) / (b - a).max(1e-6)).clamp(0.0, 1.0))
        };
        if self.xs.len() < 2 || self.zs.len() < 2 {
            return self.bed(x, z);
        }
        let ((i, u), (j, v)) = (cell(&self.xs, x), cell(&self.zs, z));
        let (xa, xb, za, zb) = (self.xs[i], self.xs[i + 1], self.zs[j], self.zs[j + 1]);
        let (ha, hb, hc, hd) = (self.bed(xa, za), self.bed(xb, za), self.bed(xb, zb), self.bed(xa, zb));
        if v >= u { ha + (hc - hd) * u + (hd - ha) * v } else { ha + (hb - ha) * u + (hc - hb) * v }
    }

    /// The ground as a mesh: the lattice's cells, each split along its
    /// (i, j)-(i + 1, j + 1) diagonal; smooth normals.
    pub fn grid(&self) -> Mesh {
        let (nx, nz) = (self.xs.len() as u32, self.zs.len() as u32);
        let mut m = Mesh::default();
        m.vertices.reserve((nx * nz) as usize);
        for z in &self.zs {
            for x in &self.xs {
                m.vertices.push([*x, self.bed(*x, *z), *z, 0.0, 1.0, 0.0]);
            }
        }
        for j in 0..nz.saturating_sub(1) {
            for i in 0..nx.saturating_sub(1) {
                let a = j * nx + i;
                let (b, c, d) = (a + 1, a + nx + 1, a + nx);
                m.indices.extend_from_slice(&[a, d, c, a, c, b]);
            }
        }
        geo::smooth_normals(&mut m);
        m
    }

    /// The bed's normal at (x, z).
    pub fn normal(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 0.25;
        let dx = (self.bed(x + e, z) - self.bed(x - e, z)) / (2.0 * e);
        let dz = (self.bed(x, z + e) - self.bed(x, z - e)) / (2.0 * e);
        geo::normalize([-dx, 1.0, -dz])
    }

    /// Whether (x, z) is on the scree or its apron (not on the wall).
    fn on_scree(&self, x: f32, z: f32) -> bool {
        x > -self.length * 0.5 + 0.4 && x < self.length * 0.5 + self.apron * 0.85 && z.abs() < self.size * 0.5
    }

    /// Share of the run at x (0 the head, 1 the toe and the apron).
    fn t(&self, x: f32) -> f32 {
        ((x + self.length * 0.5) / self.length).clamp(0.0, 1.0)
    }

    /// How far the slope has turned at z (radians; 0 without a curve).
    pub fn turn(&self, z: f32) -> f32 {
        z * self.kappa
    }

    /// Where a point of the slope's frame lands once bent round by `curve`
    /// (the bowl's middle downhill, at +x).
    pub fn warp(&self, p: [f32; 3]) -> [f32; 3] {
        if self.kappa <= 0.0 {
            return p;
        }
        let th = p[2] * self.kappa;
        let r = self.r0 - p[0];
        [self.r0 - r * th.cos(), p[1], r * th.sin()]
    }

    /// Bend a whole mesh (positions and normals) round by `curve`.
    fn bend(&self, m: &mut Mesh) {
        if self.kappa <= 0.0 {
            return;
        }
        for v in &mut m.vertices {
            let th = v[2] * self.kappa;
            let p = self.warp([v[0], v[1], v[2]]);
            let (s, c) = th.sin_cos();
            *v = [p[0], p[1], p[2], v[3] * c + v[5] * s, v[4], -v[3] * s + v[5] * c];
        }
    }
}

/// sRGB bytes into colour `k` of a block.
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A stone in the recipe's variant 11 (the jagged rock formation's helper):
/// base `a`, dark grains of `b`, pale grains, cloud and veins of `c`.
#[allow(clippy::too_many_arguments)]
fn stone(a: [u8; 3], b: [u8; 3], c: [u8; 3], [dark, pale]: [f32; 2], cloud: f32, [veins, per_m]: [f32; 2], rough: f32, stains: f32) -> Surface {
    let s = presets::ROCK_GRANITE_GREY.variant(11).extras(0);
    srgb(srgb(srgb(s, 0, a), 1, b), 2, c)
        .number(0, rough)
        .number(1, veins)
        .number(2, (per_m - 1.0) / 49.0)
        .number(3, 0.0)
        .number(4, dark)
        .number(5, pale)
        .number(6, cloud)
        .number(7, 0.9)
        .number(8, stains)
        .number(9, 0.35)
}

/// The four rocks: grey schist, dark schist with white quartz veins, a grey
/// stone mottled with rusty lichen and iron patches, pale quartzite. Round
/// 2: finer grain shares (the stipple), the rust only in its patches.
fn stones() -> [Surface; 4] {
    [
        stone([112, 108, 101], [66, 64, 62], [152, 147, 138], [0.07, 0.04], 0.85, [0.45, 10.0], 0.8, 0.6),
        stone([80, 82, 86], [48, 50, 53], [214, 210, 200], [0.06, 0.03], 0.5, [0.8, 4.0], 0.75, 0.35),
        stone([116, 108, 98], [68, 62, 55], [166, 116, 70], [0.07, 0.04], 0.62, [0.3, 10.0], 0.85, 0.8),
        stone([176, 168, 154], [126, 120, 110], [214, 208, 196], [0.05, 0.06], 0.7, [0.5, 16.0], 0.75, 0.5),
    ]
}

/// The rotation taking +y to unit `n` (axis, angle), for [`sh::rotate`].
fn up_to(n: [f32; 3]) -> ([f32; 3], f32) {
    let ax = [n[2], 0.0, -n[0]];
    let l = (ax[0] * ax[0] + ax[2] * ax[2]).sqrt();
    if l < 1e-5 { ([1.0, 0.0, 0.0], 0.0) } else { ([ax[0] / l, 0.0, ax[2] / l], n[1].clamp(-1.0, 1.0).acos()) }
}

/// A blade (its length up its own y, its thickness along z) turned to lie
/// down: length along x, thickness up, width along z (a turn of the axes,
/// so the faces keep their winding).
fn lie_down(mut m: Mesh) -> Mesh {
    for v in &mut m.vertices {
        *v = [v[1], v[2], v[0], v[4], v[5], v[3]];
    }
    m
}

/// A small sharp chip: a jagged tetrahedron, its base on y = 0, `len` across
/// and about two fifths of that high; flat faces.
fn chip(len: f32, s: u32) -> Mesh {
    let r = |k: u32| sh::rnd(s, 0, k);
    let a0 = r(1) * sh::TAU;
    let p = |k: u32, da: f32| {
        let a = a0 + da + (r(10 + k) - 0.5) * 0.6;
        let q = len * (0.4 + 0.2 * r(20 + k));
        [a.cos() * q, -len * 0.08, a.sin() * q]
    };
    let v = [p(0, 0.0), p(1, 2.1), p(2, 4.2), [len * (r(3) - 0.5) * 0.4, len * (0.3 + 0.2 * r(4)), len * (r(5) - 0.5) * 0.4]];
    let c = [(v[0][0] + v[1][0] + v[2][0] + v[3][0]) * 0.25, (v[0][1] + v[1][1] + v[2][1] + v[3][1]) * 0.25, (v[0][2] + v[1][2] + v[2][2] + v[3][2]) * 0.25];
    let mut m = Mesh::default();
    for f in [[0usize, 1, 2], [0, 1, 3], [1, 2, 3], [2, 0, 3]] {
        let (a, b, cc) = (v[f[0]], v[f[1]], v[f[2]]);
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]];
        let mut n = geo::normalize([e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]);
        let out = [a[0] - c[0], a[1] - c[1], a[2] - c[2]];
        let tri = if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
            [a, cc, b]
        } else {
            [a, b, cc]
        };
        let base = m.vertices.len() as u32;
        for q in tri {
            m.vertices.push([q[0], q[1], q[2], n[0], n[1], n[2]]);
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    m
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A broken block of schist (round 3, for the blocks and the ribs): a
/// polyhedron of fracture planes, two bedding faces a little off parallel,
/// five to seven steep breaks round it and two to four cuts across its
/// corners, so its faces are flat, its edges crisp and its outline uneven.
/// With `full` its edges are refined, its thin sheets step back one by one
/// along the breaks (layers on its sides) and fresh chips come off its upper
/// corners. Thickness up y, length along x, width along z, round its middle;
/// flat faces. `sub`: the base sphere's subdivisions.
fn slab(len: f32, wide: f32, thick: f32, s: u32, sub: u32, full: bool, wear: f32) -> Mesh {
    let r = |k: u32| sh::rnd(s, 7, k);
    let half = [len * 0.5, thick * 0.5, wide * 0.5];
    let support = |n: [f32; 3]| n[0].abs() * half[0] + n[1].abs() * half[1] + n[2].abs() * half[2];
    // (normal, offset, a side break)
    let mut planes: Vec<([f32; 3], f32, bool)> = Vec::new();
    for (k, up) in [(1u32, 1.0f32), (4, -1.0)] {
        let n = geo::normalize([(r(k) - 0.5) * 0.3, up, (r(k + 1) - 0.5) * 0.3]);
        planes.push((n, support(n) * (0.9 + 0.12 * r(k + 2)), false));
    }
    let sides = 5 + (r(7) * 3.0) as u32;
    let a0 = r(8) * sh::TAU;
    for i in 0..sides {
        let a = a0 + i as f32 / sides as f32 * sh::TAU + (r(10 + i) - 0.5) * 0.7;
        let n = geo::normalize([a.cos(), (r(20 + i) - 0.5) * 0.8, a.sin()]);
        planes.push((n, support(n) * (0.8 + 0.18 * r(30 + i)), true));
    }
    for i in 0..(2 + (r(9) * 3.0) as u32) {
        let a = r(40 + i) * sh::TAU;
        let n = geo::normalize([a.cos(), if r(50 + i) < 0.7 { 0.7 + 0.6 * r(45 + i) } else { -0.8 }, a.sin()]);
        planes.push((n, support(n) * (0.68 + 0.16 * r(55 + i)), true));
    }
    // The nearest and the second nearest plane along unit d, and which.
    let hit = |d: [f32; 3]| -> (f32, f32, usize) {
        let (mut a, mut b, mut ia) = (f32::MAX, f32::MAX, 0usize);
        for (i, (n, off, _)) in planes.iter().enumerate() {
            let c = dot3(*n, d);
            if c > 1e-3 {
                let t = off / c;
                if t < a {
                    b = a;
                    a = t;
                    ia = i;
                } else if t < b {
                    b = t;
                }
            }
        }
        (a.min(half[0] + half[1] + half[2]), b, ia)
    };
    let lt = thick / (3.0 + 3.0 * r(60));
    let mut m = sh::icosphere(sub);
    if full {
        // Finer along the creases and where a sheet's edge crosses a break.
        detail::refine_near(&mut m, 0.05, 4, |c, e| {
            if e <= 0.05 {
                return false;
            }
            let d = geo::normalize(c);
            let (a, b, ia) = hit(d);
            if b - a < 0.6 * e * a {
                return true;
            }
            if planes[ia].2 {
                let f = (d[1] * a + half[1]) / lt;
                let f = f - f.floor();
                return f.min(1.0 - f) * lt < 0.7 * e * a;
            }
            false
        });
    }
    let step = 0.06 * thick + 0.008;
    let mut crease = Vec::with_capacity(m.vertices.len());
    for v in &mut m.vertices {
        let d = geo::normalize([v[0], v[1], v[2]]);
        let (a, b, ia) = hit(d);
        let mut p = [d[0] * a, d[1] * a, d[2] * a];
        if full && planes[ia].2 {
            // The sheet this point is in, set back by its own amount.
            let li = ((p[1] + half[1]) / lt).floor();
            let rec = step * sh::rnd(s, (li as i32 + 64) as u32, 61);
            let n = planes[ia].0;
            p = [p[0] - n[0] * rec, p[1] - n[1] * rec, p[2] - n[2] * rec];
        }
        crease.push((b - a) / a.max(1e-4));
        *v = [p[0], p[1], p[2], d[0], d[1], d[2]];
    }
    if full && wear > 0.0 {
        // Fresh chips off two of its upper corners.
        let n = m.vertices.len();
        let mut chips = Vec::new();
        for i in 0..2u32 {
            for t in 0..64u32 {
                let j = ((sh::rnd(s, i * 71 + t, 91) * n as f32) as usize).min(n - 1);
                let v = m.vertices[j];
                if v[1] > half[1] * 0.2 && crease.get(j).is_some_and(|c| *c < 0.04) {
                    let rr = (len.min(wide) * (0.08 + 0.08 * sh::rnd(s, i, 92))) * wear.min(1.5);
                    chips.push(detail::Chip { at: [v[0], v[1], v[2]], radius: rr, depth: rr * 0.4, fine: (rr * 0.4).max(0.004), seed: s ^ i.wrapping_mul(0x2545_f491) });
                    break;
                }
            }
        }
        if !chips.is_empty() {
            let _ = detail::spalls(&mut m, &chips);
        }
    }
    sh::flat_shade(&mut m);
    m
}

/// A stone lying on the bed at (x, z): `m` (its thickness up its own y, round
/// its middle) turned `yaw` off the fall line, tilted a little, then set on
/// the bed's slope with `rise` of it over the drawn ground.
fn lay(sl: &Slope, mut m: Mesh, x: f32, z: f32, yaw: f32, tilt: [f32; 2], rise: f32) -> Mesh {
    sh::rotate(&mut m, [0.0, 1.0, 0.0], yaw);
    sh::rotate(&mut m, [1.0, 0.0, 0.0], tilt[0]);
    sh::rotate(&mut m, [0.0, 0.0, 1.0], tilt[1]);
    let n = sl.normal(x, z);
    let (ax, a) = up_to(n);
    if a > 1e-4 {
        sh::rotate(&mut m, ax, a);
    }
    let y = sl.ground(x, z);
    geo::transform(&mut m, [1.0; 3], [x + n[0] * rise, y + n[1] * rise, z + n[2] * rise]);
    m
}

/// The stone mesh levels: 0 a full broken stone (fracture faces, stepped
/// foliation, chips), 1 an icosphere's 320 faces cut flat, 3 its 80, 2 its
/// 20, 5 a four-faced chip.
const CHIP: u32 = 5;

/// One stone class: its length range (m), how many per square metre, where
/// (a disc round the focus, or the whole scree), its mesh level and seed.
struct Class {
    lo: f32,
    hi: f32,
    per_m2: f32,
    /// Density by distance from the focus: full inside `full`, none past
    /// `none` (m); `none` 0: the whole scree and apron.
    full: f32,
    none: f32,
    /// Fading in from none at `fade_in[0]` to full at `fade_in[1]` (m; both
    /// 0: no fade in), for a class that takes over from a nearer one.
    fade_in: [f32; 2],
    /// The talus at the wall's foot: thickest just under the wall.
    foot: bool,
    /// The mesh level by distance from the focus: the first whose radius
    /// (m) the stone is inside, else the last.
    levels: [(f32, u32); 3],
    key: u32,
    /// Its share of the stones' triangle budget (round 2: shares, so the
    /// near stones can't leave the far ones none).
    share: f32,
}

/// Every stone of class `c` into the four rocks' meshes.
#[allow(clippy::too_many_arguments)]
fn scatter(sl: &Slope, c: &Class, focus: [f32; 2], rust: f32, wear: f32, density: f32, out: &mut [Mesh; 4], budget: &mut usize) {
    let s = sl.seed ^ c.key.wrapping_mul(0x9e37_79b9);
    let run = if c.foot { 15.0f32.min(sl.length) } else { sl.length + sl.apron * 0.85 };
    let area = if c.none > 0.0 { std::f32::consts::PI * c.none * c.none } else { sl.size * run };
    let n = (c.per_m2 * area).round() as u32;
    let smooth = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a).max(1e-4)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let rust_p = 0.08 + 0.42 * rust;
    let big = c.lo > 0.45;
    for i in 0..n {
        let r = |k: u32| sh::rnd(s, i, k);
        let (x, z) = if c.none > 0.0 {
            // From the focus outwards, so a class cut short by its budget
            // loses its outermost, thinnest stones.
            let (rr, a) = (c.none * ((i as f32 + r(1)) / n as f32).sqrt(), r(2) * sh::TAU);
            (focus[0] + rr * a.cos(), focus[1] + rr * a.sin())
        } else {
            (-sl.length * 0.5 + r(1) * run, (r(2) - 0.5) * sl.size)
        };
        if !sl.on_scree(x, z) {
            continue;
        }
        let t = sl.t(x);
        let past = x > sl.length * 0.5;
        // Fall sorting: the blocks further down and out on the apron, the
        // small stones thinner there.
        let keep = if big {
            (0.4 + 1.2 * t).min(1.0) * if past { 1.0 } else { 0.8 }
        } else if past {
            0.3
        } else {
            0.8 + 0.2 * t
        };
        let d = ((x - focus[0]).powi(2) + (z - focus[1]).powi(2)).sqrt();
        let mut keep = keep;
        if c.none > 0.0 {
            keep *= 1.0 - smooth(c.full, c.none, d);
        }
        if c.fade_in[1] > 0.0 {
            keep *= smooth(c.fade_in[0], c.fade_in[1], d);
        }
        if c.foot {
            keep = (-(x - sl.head().0 - 0.5).max(0.0) / 4.0).exp();
        }
        if r(3) > keep {
            continue;
        }
        let u = r(4).powf(1.6);
        let len = c.lo * (c.hi / c.lo).powf(u) * (0.8 + 0.3 * t);
        let level = c.levels.iter().find(|(r, _)| d < *r).map_or(c.levels[2].1, |l| l.1);
        let cost = match level {
            0 => 2600,
            1 => 320,
            3 => 80,
            CHIP => 4,
            _ => 20,
        };
        if *budget < cost {
            break;
        }
        let ss = s ^ i.wrapping_mul(0x85eb_ca6b);
        // Round 3: the blocks are broken slabs of schist, in two rocks.
        if big {
            let (wide, thick) = (len * (0.55 + 0.35 * r(5)), len * (0.3 + 0.25 * r(6)));
            let m = match level {
                0 => slab(len, wide, thick, ss, 3, true, wear),
                1 => slab(len, wide, thick, ss, 2, false, wear),
                _ => slab(len, wide, thick, ss, 1, false, wear),
            };
            let k0 = if r(13) > 0.85 { 3 } else { 0 };
            let accent = if r(15) < 0.35 + 0.4 * rust { 2 } else { 3 };
            let parts = detail::split_by(&m, 2, |q, _| usize::from(noise::fbm3([q[0] / (len * 0.4), q[1] / (len * 0.4), q[2] / (len * 0.4)], ss ^ 0x51, 2) > 0.22));
            let yaw = (r(8) - 0.5) * if r(9) < 0.6 { 1.1 } else { sh::TAU };
            let tilt = [(r(10) - 0.5) * 0.5, (r(11) - 0.5) * 0.4];
            let rise = thick * 0.5 * (0.2 + 0.45 * r(12));
            for (pi, pm) in parts.into_iter().enumerate() {
                let pm = lay(sl, pm, x, z, yaw, tilt, rise);
                *budget = budget.saturating_sub(pm.indices.len() / 3);
                geo::merge(&mut out[if pi == 0 { k0 } else { accent }], &pm);
            }
            continue;
        }
        // Slabs and chunky broken stones; a chip.
        let (m, half) = if level == CHIP {
            (chip(len, ss), len * 0.2)
        } else {
            let slab = r(14) < 0.55;
            let (wide, thick) = (len * (0.55 + 0.35 * r(5)), len * if slab { 0.25 + 0.15 * r(6) } else { 0.45 + 0.25 * r(6) });
            let peak = if level == 0 { 0.15 + 0.2 * r(7) } else { 0.0 };
            let m = blade(len, wide, thick, peak, 0.1, (len * 0.22).max(0.05), if level == 0 { len * 0.05 } else { 0.0 }, wear, ss, density, level);
            (lie_down(m), thick * 0.5)
        };
        // Long axes mostly down the fall line, tilted off the bed as they
        // lean on each other, sunk among their neighbours.
        let yaw = (r(8) - 0.5) * if r(9) < 0.6 { 1.1 } else { sh::TAU };
        let tilt = [(r(10) - 0.5) * 0.6, (r(11) - 0.5) * 0.45];
        let rise = if level == CHIP { len * 0.05 } else { half * (0.15 + 0.5 * r(12)) };
        let m = lay(sl, m, x, z, yaw, tilt, rise);
        *budget = budget.saturating_sub(m.indices.len() / 3);
        // Its rock: rust in patches (the lichen and iron on the stiller
        // scree), the dark veined and pale stones scattered through.
        let patch = noise::fbm2(x / 9.0, z / 9.0, sl.seed ^ 0x2a57, 2, 2.0, 0.5);
        let q = r(13);
        let k = if q < rust_p * (0.5 + 1.2 * patch.max(0.0) + 0.3) {
            2
        } else if q > 0.86 {
            3
        } else if q > 0.64 {
            1
        } else {
            0
        };
        geo::merge(&mut out[k], &m);
    }
}

/// The wall's face: one continuous surface of foliated schist over the
/// slope's head. Its beds (about 0.9 m, dipping along the foot) step back
/// one by one; joints every few metres cut them into blocks, each set back
/// by its own amount with a planar break of its own and a deep fissure at
/// the joint; a dark parting under each bed; the top turns back into the
/// ground over the wall. Built in rows up the face and columns along the
/// foot (finer round the player); flat faces, so the breaks stay crisp.
fn wall_face(sl: &Slope, focus: [f32; 2], radius: f32) -> Mesh {
    let s = sl.seed ^ 0xface_0001;
    let (hx, top) = sl.head();
    let k = (sl.cliff / 14.0).max(0.3);
    let ybot = top - 2.0;
    let bed_h = 0.9 * k;
    let dip = 0.27;
    let zs = lines(-sl.size * 0.5, sl.size * 0.5, focus[1], radius * 1.3, 0.3, 1.3);
    // Rows up the face (about 0.35 m at its tallest), up to each column's
    // own crest (round 3: an uneven crest, the ends tapering away), then
    // three back over its top.
    let rows = (((sl.cliff * 1.25 + 2.5) / (0.35 * k.min(1.5))).ceil() as usize).max(2);
    // The joints along the foot: 2-5 m apart, wandering a little with height.
    let joint = |z: f32, y: f32| -> (f32, f32) {
        let q = z / (3.2 * k) + 0.45 * noise::fbm2(y / 6.0, z / 9.0, s ^ 0x10, 2, 2.0, 0.5) + 0.3 * (z / (7.3 * k)).sin();
        let ji = q.floor();
        let f = q - ji;
        (ji, f.min(1.0 - f) * 3.2 * k)
    };
    let face_x = |y: f32, z: f32, ytop: f32| -> f32 {
        let v = ((y - ybot) / (ytop - ybot).max(0.1)).clamp(0.0, 1.0);
        // Buttresses and bays a few metres out and in along the foot.
        let lean = -v * sl.depth * 0.5 + 2.0 * k * noise::fbm2(z / 16.0, v * 0.6, s ^ 0x0b75, 2, 2.0, 0.5);
        let b0 = (y + dip * z) / bed_h + 0.6 * noise::fbm2(z / 22.0, y / 30.0, s ^ 0x22, 2, 2.0, 0.5);
        // Uneven beds: thick and thin ones in turn.
        let b = b0 + 0.28 * (b0 * 1.9).sin() + 0.15 * (b0 * 0.7 + 1.3).sin();
        let bi = b.floor();
        let bf = b - bi;
        let (ji, dj) = joint(z, y);
        let id = ((bi as i32 + 4096) as u32).wrapping_mul(7919) ^ ((ji as i32 + 4096) as u32).wrapping_mul(104_729);
        // Each block set back by its own amount, its face a tilted break.
        let rec = 2.0 * k * sh::rnd(s, id, 1).powf(1.5) * sh::rnd(s, id, 2).sqrt();
        let tilt = (sh::rnd(s, id, 3) - 0.5) * 0.5 * (bf - 0.5) * bed_h + (sh::rnd(s, id, 4) - 0.5) * 0.25 * (dj - 1.0);
        // A dark parting at the bed's foot, a deep fissure at the joint.
        let parting = 0.22 * k * (1.0 - (bf / 0.1).min(1.0));
        let fissure = 0.9 * k * (1.0 - (dj / (0.35 * k)).min(1.0)).powf(1.5);
        let rough = 0.05 * noise::fbm2(y * 2.5, z * 2.5, s ^ id, 2, 2.0, 0.5);
        hx + 0.6 + lean - rec + tilt - parting - fissure + rough
    };
    let mut m = Mesh::default();
    let back = sl.depth * 0.5 + 2.0;
    for z in &zs {
        let ytop = top + sl.wall_h(*z) + 0.5;
        for j in 0..rows {
            let y = ybot + (ytop - ybot) * j as f32 / (rows - 1) as f32;
            let x = face_x(y, *z, ytop);
            m.vertices.push([x, y, *z, 1.0, 0.0, 0.0]);
            if j + 1 == rows {
                // Over the top: three rows back into the ground over the wall.
                for q in 1..=3 {
                    let f = q as f32 / 3.0;
                    let yy = ytop + 0.4 * noise::fbm2(z / 4.0, f * 3.0, s ^ 0x77, 2, 2.0, 0.5) - 0.3 * f;
                    m.vertices.push([x - back * f, yy, *z, 0.0, 1.0, 0.0]);
                }
            }
        }
    }
    let per = (rows + 3) as u32;
    for i in 0..zs.len().saturating_sub(1) as u32 {
        for j in 0..per - 1 {
            let a = i * per + j;
            let (b, c, d) = (a + per, a + per + 1, a + 1);
            // Seen from +x (the slope): (a, d, b) and (b, d, c).
            m.indices.extend_from_slice(&[a, d, b, b, d, c]);
        }
    }
    sh::flat_shade(&mut m);
    m
}

/// The rock ribs: a few broken blades down from the wall between the
/// chutes, standing out of the scree (full near the focus, plainer further).
fn ribs(sl: &Slope, focus: [f32; 2], radius: f32, wear: f32, out: &mut [Mesh; 4], budget: &mut usize) {
    if sl.cliff <= 0.0 {
        return;
    }
    let s = sl.seed ^ 0x0c11_ff00;
    let hx = sl.head().0;
    let floor = budget.saturating_sub(*budget / 10);
    for j in 1..sl.chutes.max(2) {
        let (zr, reach) = sl.rib(j);
        if zr.abs() > sl.size * 0.5 - 2.0 {
            continue;
        }
        let n = 4 + (reach * 8.0) as u32;
        for q in 0..n {
            let id = 1000 + j * 50 + q;
            let r = |k: u32| sh::rnd(s, id, k);
            let f = q as f32 / n as f32;
            let x = hx + 0.5 + f * reach * sl.length;
            let zz = zr + 2.2 * (r(1) - 0.5);
            let h = (sl.cliff * 0.16 * (1.0 - f * 0.8) + 0.7) * (0.7 + 0.5 * r(2));
            let near = ((x - focus[0]).powi(2) + (zz - focus[1]).powi(2)).sqrt() < radius * 0.9;
            let full = near && *budget > floor + 8_000;
            let (len, wide, thick) = (h * (1.6 + 0.8 * r(3)), h * (1.1 + 0.6 * r(4)), h * (0.6 + 0.4 * r(5)));
            let m = slab(len, wide, thick, s ^ id.wrapping_mul(0x2c1b_3c6d), if full { 3 } else { 2 }, full, wear);
            // Bedded along the slope, half sunk in the scree.
            let m = lay(sl, m, x, zz, (r(8) - 0.5) * 1.2, [(r(7) - 0.5) * 0.4, (r(9) - 0.5) * 0.3], thick * (0.1 + 0.2 * r(10)));
            *budget = budget.saturating_sub(m.indices.len() / 3);
            geo::merge(&mut out[if r(6) < 0.3 { 2 } else if r(6) > 0.88 { 3 } else { 0 }], &m);
        }
    }
}

/// Mats of dwarf shrub where the scree is still: low, lumpy, longer down
/// the slope than across.
fn mats(sl: &Slope, moss: f32, focus: [f32; 2], radius: f32) -> Mesh {
    let mut all = Mesh::default();
    let s = sl.seed ^ 0x3a7_5eed;
    let n = (moss * sl.size * sl.length / 260.0).round().min(60.0) as u32;
    let base = sh::icosphere(2);
    for i in 0..n {
        let r = |k: u32| sh::rnd(s, i, k);
        // Half of them round the player, where they show.
        let (x, z) = if r(0) < 0.5 {
            let (rr, a) = (radius * 1.5 * r(1).sqrt(), r(2) * sh::TAU);
            (focus[0] + rr * a.cos(), focus[1] + rr * a.sin())
        } else {
            ((r(1) - 0.5) * sl.length, (r(2) - 0.5) * sl.size)
        };
        if !sl.on_scree(x, z) || sl.t(x) < 0.12 {
            continue;
        }
        let len = 0.7 + 2.2 * r(3);
        let mut m = base.clone();
        for v in &mut m.vertices {
            let bump = 1.0 + 0.35 * noise::fbm3([v[0] * 3.0, v[1] * 3.0, v[2] * 3.0], s ^ i, 2);
            *v = [v[0] * len * 0.5 * bump, v[1] * 0.16 * bump.max(0.6), v[2] * len * 0.22 * (0.8 + 0.6 * r(4)) * bump, v[3], v[4], v[5]];
        }
        geo::smooth_normals(&mut m);
        sh::rotate(&mut m, [0.0, 1.0, 0.0], (r(5) - 0.5) * 0.7);
        let nn = sl.normal(x, z);
        let (ax, a) = up_to(nn);
        if a > 1e-4 {
            sh::rotate(&mut m, ax, a);
        }
        geo::transform(&mut m, [1.0; 3], [x, sl.ground(x, z) + 0.04, z]);
        geo::merge(&mut all, &m);
    }
    all
}

/// The scree slope (the library asset's generator).
pub fn generate(v: &Values, seed: u32) -> Output {
    let sl = Slope::new(v, seed);
    let density = v.density();
    let stones_k = v.f("stones").max(0.0);
    let focus = [v.f("focus_x"), v.f("focus_z")];
    let radius = v.f("focus_radius").max(1.0);
    let mut budget = (BUDGET as f32 * density.clamp(0.3, 1.0)) as usize;
    // The bed: the ground's lattice over the scree, the apron, under the
    // wall and over it.
    let grid = sl.grid();
    budget = budget.saturating_sub(grid.indices.len() / 3);
    let mut rocks: [Mesh; 4] = Default::default();
    let wear = 1.0;
    let mut face = if sl.cliff > 0.0 { wall_face(&sl, focus, radius) } else { Mesh::default() };
    budget = budget.saturating_sub(face.indices.len() / 3);
    ribs(&sl, focus, radius, wear, &mut rocks, &mut budget);
    let mut mats = mats(&sl, v.f("moss").clamp(0.0, 1.0), focus, radius);
    budget = budget.saturating_sub(mats.indices.len() / 3);
    if stones_k > 0.0 {
        let big = v.f("block").max(0.35);
        let cob = (big * 0.3).max(0.3);
        let r = radius;
        let classes = [
            // The blocks over the whole scree and its apron: in full close
            // by, plainer further off.
            Class { lo: cob, hi: big, per_m2: 0.012 * stones_k, full: 0.0, none: 0.0, fade_in: [0.0; 2], foot: false, levels: [(r * 0.3, 0), (r * 2.0, 1), (0.0, 3)], key: 1, share: 0.12 },
            // The talus heaped at the wall's foot.
            Class { lo: cob, hi: big * 0.8, per_m2: 0.3 * stones_k, full: 0.0, none: 0.0, fade_in: [0.0; 2], foot: true, levels: [(r * 0.3, 0), (r * 2.0, 1), (0.0, 3)], key: 7, share: 0.05 },
            // Round the player, clast on clast: slabs and chunks, finer close,
            // thinning smoothly outwards.
            Class { lo: 0.1, hi: cob * 1.4, per_m2: 16.0 * stones_k, full: r * 0.18, none: r * 0.5, fade_in: [0.0; 2], foot: false, levels: [(r * 0.12, 3), (0.0, 2), (0.0, 2)], key: 2, share: 0.36 },
            // Sharp chips between them, close by.
            Class { lo: 0.03, hi: 0.1, per_m2: 34.0 * stones_k, full: r * 0.15, none: r * 0.42, fade_in: [0.0; 2], foot: false, levels: [(0.0, CHIP); 3], key: 4, share: 0.06 },
            // Cobbles further out, taking over as the near stones thin.
            Class { lo: 0.15, hi: cob * 1.5, per_m2: 3.5 * stones_k, full: r, none: r * 1.6, fade_in: [r * 0.2, r * 0.5], foot: false, levels: [(0.0, 2); 3], key: 5, share: 0.13 },
            // Out to four times the focus radius, many stones as four-faced
            // chips (a few pixels across from the player, they need no more).
            Class { lo: 0.3, hi: 0.9, per_m2: 2.0 * stones_k, full: r * 3.0, none: r * 4.5, fade_in: [r * 0.6, r * 1.1], foot: false, levels: [(0.0, CHIP); 3], key: 6, share: 0.23 },
            // A few plain cobbles over the rest of the scree and the apron.
            Class { lo: 0.2, hi: cob * 1.6, per_m2: 0.04 * stones_k, full: 0.0, none: 0.0, fade_in: [0.0; 2], foot: false, levels: [(0.0, 2); 3], key: 3, share: 0.05 },
        ];
        // Each class its share of what is left for stones, what one leaves
        // passed on to the next.
        let pool = budget as f32;
        let mut spare = 0usize;
        for c in &classes {
            let give = ((pool * c.share) as usize + spare).min(budget);
            let mut local = give;
            scatter(&sl, c, focus, v.f("rust"), wear, density, &mut rocks, &mut local);
            budget -= give - local;
            spare = local;
        }
    }
    // Bent round the bowl, nothing under the ground.
    for m in rocks.iter_mut().chain([&mut face, &mut mats]) {
        sl.bend(m);
        for p in &mut m.vertices {
            p[1] = p[1].max(0.0);
        }
    }
    let colour = v.color("color");
    let tint = (0..3).any(|c| (colour[c] - COLOR[c]).abs() > 1e-3);
    let tinted = |sf: Surface| -> Surface {
        if tint {
            let a = sf.colour(0);
            sf.tint([a[0] * colour[0] / COLOR[0], a[1] * colour[1] / COLOR[1], a[2] * colour[2] / COLOR[2]])
        } else {
            sf
        }
    };
    let rock = stones();
    // The wall's face in the four rocks: rust in broad patches, the dark
    // veined schist in a few beds, the pale stone in streaks, grey elsewhere.
    let rust_at = 0.42 - 0.3 * v.f("rust").clamp(0.0, 1.0);
    let face_parts = detail::split_by(&face, 4, |c, _| {
        let r = 0.7 * noise::fbm3([c[0] / 7.0, c[1] / 5.0, c[2] / 7.0], seed ^ 0xa511, 3) + 0.35 * noise::fbm3([c[0] / 1.6, c[1] / 1.6, c[2] / 1.6], seed ^ 0x5a17, 2);
        if r > rust_at {
            return 2;
        }
        let d = noise::fbm3([c[0] / 12.0, c[1] / 2.2, c[2] / 12.0], seed ^ 0x3c3c, 2);
        if d > 0.4 {
            1
        } else if d < -0.45 {
            3
        } else {
            0
        }
    });
    let mut out = Vec::new();
    // The bed (round 2): round the player, where the stones lie close, a grit
    // of dark and pale grains in a grey-tan mottle (no cells, so nothing reads
    // as paving between the stones); further out the ground recipe's scree
    // gravel at a stone's size, so the far slope keeps its texture. Their
    // border is ragged. Split in the slope's frame, before the bend.
    let grit = stone([122, 113, 101], [66, 60, 54], [170, 162, 150], [0.34, 0.3], 0.7, [0.0, 10.0], 0.92, 0.45);
    let bed_parts = detail::split_by(&grid, 2, |c, _| {
        let d = ((c[0] - focus[0]).powi(2) + (c[2] - focus[1]).powi(2)).sqrt();
        let edge = radius * (0.55 + 0.25 * noise::fbm2(c[0] / 4.0, c[2] / 4.0, seed ^ 0x6e, 2, 2.0, 0.5));
        usize::from(d > edge)
    });
    let mut bed_parts = bed_parts.into_iter();
    for (m, sf) in [(bed_parts.next(), grit.seed(seed ^ 0x9f)), (bed_parts.next(), presets::GROUND_GRAVEL_SCREE.tint([0.11, 0.1, 0.085]).number(1, 0.2).seed(seed).scale(0.22))] {
        if let Some(mut m) = m.filter(|m| !m.indices.is_empty()) {
            sl.bend(&mut m);
            for p in &mut m.vertices {
                p[1] = p[1].max(0.0);
            }
            out.push(Part::surfaced(m, tinted(sf)));
        }
    }
    for (i, ((m, f), sf)) in rocks.into_iter().zip(face_parts).zip(rock).enumerate() {
        let mut m = m;
        geo::merge(&mut m, &f);
        if !m.indices.is_empty() {
            out.push(Part::surfaced(m, tinted(sf.seed(seed.wrapping_add(i as u32 * 0x2f6b)))));
        }
    }
    if !mats.indices.is_empty() {
        out.push(Part::surfaced(mats, presets::GROUND_GRASS_ALPINE.tint([0.035, 0.06, 0.02]).seed(seed ^ 0x3a)));
    }
    Output::Parts(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_procedural::Value;

    fn values() -> Values {
        Values::defaults(&PARAMS)
    }

    fn tris(o: &Output) -> usize {
        match o {
            Output::Parts(p) => p.iter().map(|x| x.mesh.indices.len() / 3).sum(),
            _ => 0,
        }
    }

    fn check(o: &Output) {
        let (lo, _) = o.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo[1] > -1e-3, "on the ground: {lo:?}");
        let Output::Parts(p) = o else { panic!("parts") };
        assert!(p.iter().all(|x| x.mesh.vertices.iter().all(|q| q.iter().all(|c| c.is_finite()))));
        assert!(tris(o) < 400_000, "{}", tris(o));
    }

    #[test]
    fn the_scree_lies_on_the_ground_inside_its_budget() {
        let v = values();
        let t0 = std::time::Instant::now();
        let o = generate(&v, 7);
        let ms = t0.elapsed().as_millis();
        let Output::Parts(p) = &o else { panic!("parts") };
        assert!(p.len() >= 5, "bed, four rocks and mats: {}", p.len());
        check(&o);
        let (_, hi) = o.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        let sl = Slope::new(&v, 7);
        assert!(hi[1] > sl.head().1 + sl.cliff * 0.8, "the wall stands over the head: {hi:?}");
        println!("scree slope: {} triangles at High ({ms} ms)", tris(&o));
        // A player's big cirque (the proof game's scene), and a wide focus.
        let mut big = v.clone();
        for (n, x) in [("size", 300.0), ("length", 90.0), ("curve", 120.0), ("apron", 40.0), ("focus_x", -41.5), ("focus_z", 104.0), ("focus_radius", 26.0)] {
            big.set(n, Value::F32(x));
        }
        let o = generate(&big, 3);
        check(&o);
        println!("scree cirque: {} triangles at High", tris(&o));
        let mut wide = v.clone();
        wide.set("focus_radius", Value::F32(80.0));
        check(&generate(&wide, 3));
    }

    #[test]
    fn the_slope_lies_at_its_angle_of_repose() {
        let sl = Slope::new(&values(), 1);
        // Steeper under the wall than at the toe, about 34 degrees on the
        // upper half.
        let (hx, _) = sl.head();
        let up = (sl.bed(hx + 4.0, 3.0) - sl.bed(hx + 14.0, 3.0)) / 10.0;
        let low = (sl.bed(sl.length * 0.5 - 12.0, 3.0) - sl.bed(sl.length * 0.5 - 2.0, 3.0)) / 10.0;
        assert!(up > 0.62 && up < 0.85, "{up}");
        assert!(low < up && low > 0.25, "{low}");
    }

    #[test]
    fn the_ground_is_the_drawn_lattice_and_the_bend_keeps_it() {
        let mut v = values();
        v.set("curve", Value::F32(90.0));
        let sl = Slope::new(&v, 2);
        let g = sl.grid();
        // Every lattice corner is on the ground's surface.
        for q in g.vertices.iter().step_by(97) {
            assert!((sl.ground(q[0], q[2]) - q[1]).abs() < 1e-3, "{q:?}");
        }
        // The bend keeps the middle line in place and the distances along it.
        let a = sl.warp([0.0, 1.0, 0.0]);
        assert!(a[0].abs() < 1e-4 && a[2].abs() < 1e-4);
        let (p, q) = (sl.warp([0.0, 0.0, 10.0]), sl.warp([0.0, 0.0, 11.0]));
        let d = ((p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        assert!((d - 1.0).abs() < 0.01, "{d}");
    }
}

