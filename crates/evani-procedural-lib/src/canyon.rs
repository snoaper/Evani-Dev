//! Canyons (`terrain-canyons`, Asset Studio column 12, 2026-11-06): a cut
//! block of red-rock canyon country. A plateau of sandy, scrubby slickrock
//! is cut by a winding main canyon (open at the block's back and front
//! edges) and four blind side canyons; their walls fall in stepped strata,
//! each layer a talus slope under a sheer cliff band capped by a bench, the
//! cliff lines crenulated into spurs, alcoves and buttresses; a dry wash of
//! pale gravel winds along the floors; the block's own edges erode in the
//! same steps down to a thin base of dark rock on the floor.
//!
//! Everything is real geometry in catalogue materials (red sandstone in
//! banded tints by height, red clay talus, desert sand and slickrock on the
//! flats, creek gravel in the wash, scrub and fallen blocks as small
//! meshes), so a game that lists the ground and stone recipes draws the
//! same asset the library shows. The layout is designed at the default 30 m
//! and scales with `size`; the walls' height is `depth`.
//!
//! The plan is a height grid whose square border is bent onto an irregular
//! outline (a rounded square with wandering, chipped edges), so the base's
//! cut sides follow the grid's own border with no seam.

use std::collections::HashMap;

use evani_procedural::surface::presets as sp;
use evani_procedural::{Output, ParamSpec as P, Part, Surface, Values, geo, noise, shapes as sh};

type Mesh = evani_render::mesh::Mesh;

/// The design size (m): every plan coordinate below is at this size.
const DESIGN: f32 = 30.0;

/// The default sandstone colour (linear): the canyon walls' red.
pub const SANDSTONE: [f32; 3] = [0.14, 0.068, 0.048];

pub const PARAMS: [P; 9] = [
    P::num("size", 30.0, 4.0, 400.0).unit("m").describe("Width of the block of canyon country (m)"),
    P::num("depth", 3.6, 0.5, 100.0).unit("m").describe("Canyon depth: the plateau over the main canyon's floor (m)"),
    P::num("width", 1.0, 0.1, 60.0).unit("m").describe("Width of the main canyon's floor (m)"),
    P::color("color", SANDSTONE).describe("Sandstone colour (the walls' red)"),
    P::num("rock", 0.75, 0.0, 1.0).describe("Bare rock: how much of the plateau and the benches is bare slickrock rather than sand (0: all sand)"),
    P::num("snow", 0.0, 0.0, 1.0).describe("Snow: how far down from the plateau it lies on the flats (0: none)"),
    P::num("steps", 4.0, 1.0, 8.0).step(1.0).describe("Cliff bands in the canyon walls"),
    P::num("scrub", 1.0, 0.0, 1.0).describe("Desert scrub and fallen blocks on the plateau, benches and floors (0: bare)"),
    P::num("edge", 0.5, 0.0, 5.0).unit("m").describe("Height of the dark rock base at the block's cut edge (m; 0: none, for a canyon set into a world)"),
];

/// One canyon: its centre line in the view plan (x right, z towards the
/// viewer; design metres), its floor's half-width (times the `width`
/// param over 1 m), its walls' plan width, and how far (a share of the
/// depth) its floor rises towards its end (side canyons hang).
struct Arm {
    line: &'static [[f32; 2]],
    half: f32,
    wall: f32,
    rise: f32,
}

/// The canyons, at the design size, in the view plan: the main canyon from
/// the back edge to the front corner, and six blind side canyons.
const ARMS: [Arm; 7] = [
    Arm {
        line: &[[1.2, -19.0], [0.6, -15.5], [-1.4, -13.2], [-3.2, -8.2], [-2.0, -3.2], [0.8, -0.6], [3.4, 2.4], [3.9, 5.6], [2.4, 9.6], [2.6, 13.4], [2.0, 19.0]],
        half: 0.8,
        wall: 4.4,
        rise: 0.0,
    },
    Arm { line: &[[-1.4, -13.2], [1.6, -12.0], [4.8, -11.6], [7.6, -10.0], [9.4, -7.4]], half: 0.5, wall: 3.9, rise: 0.34 },
    Arm { line: &[[3.4, 2.4], [5.6, 0.4], [8.0, -0.6], [9.8, -2.6]], half: 0.45, wall: 3.6, rise: 0.38 },
    Arm { line: &[[-3.2, -8.2], [-5.0, -10.4], [-7.6, -11.6], [-9.0, -10.6]], half: 0.45, wall: 3.6, rise: 0.36 },
    Arm { line: &[[-2.0, -3.2], [-4.6, -2.6], [-6.8, -0.8], [-7.6, 1.2]], half: 0.42, wall: 3.4, rise: 0.44 },
    Arm { line: &[[7.6, -10.0], [9.4, -12.2], [11.2, -12.4]], half: 0.32, wall: 2.6, rise: 0.55 },
    Arm { line: &[[2.4, 9.6], [0.2, 8.4], [-2.0, 8.9], [-3.8, 7.6]], half: 0.36, wall: 3.0, rise: 0.5 },
];

/// The block's outer walls: their plan width, and the base rock's rim
/// inside the cut edge (design m).
const EDGE_WALL: f32 = 1.9;
const EDGE_RIM: f32 = 0.15;

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// The distance from `p` to the polyline `line`, how far along it the
/// nearest point lies (0 at its start, 1 at its end) and its length.
fn line_dist(line: &[[f32; 2]], p: [f32; 2]) -> (f32, f32, f32) {
    let total: f32 = line.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum();
    let (mut best, mut at, mut run) = (f32::MAX, 0.0, 0.0);
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len2 = (d[0] * d[0] + d[1] * d[1]).max(1e-6);
        let q = [p[0] - a[0], p[1] - a[1]];
        let t = ((q[0] * d[0] + q[1] * d[1]) / len2).clamp(0.0, 1.0);
        let e = [q[0] - d[0] * t, q[1] - d[1] * t];
        let dist = (e[0] * e[0] + e[1] * e[1]).sqrt();
        if dist < best {
            best = dist;
            at = (run + t * len2.sqrt()) / total.max(1e-6);
        }
        run += len2.sqrt();
    }
    (best, at, total)
}

/// The land: the outline, the canyons and the strata, in world metres.
struct Land {
    /// World metres per design metre.
    k: f32,
    half: f32,
    depth: f32,
    width: f32,
    edge: f32,
    steps: u32,
    seed: u32,
    /// Each layer's top as a share of the walls' height (from the floor).
    tops: Vec<f32>,
    /// The outline's corners (world m, counter-clockwise from -x).
    corners: Vec<[f32; 2]>,
}

impl Land {
    fn new(v: &Values, seed: u32) -> Self {
        let size = v.f("size").max(1.0);
        let steps = v.f("steps").round().clamp(1.0, 8.0) as u32;
        // Layers of their own thickness: the caprock and the inner gorge's
        // band thicker, the middle beds by turns thin and thick.
        let mut tops = Vec::with_capacity(steps as usize);
        let mut acc = 0.0;
        for i in 0..steps {
            let edge = if i == 0 || i + 1 == steps { 1.25 } else { 1.0 };
            acc += edge * (0.7 + 0.6 * sh::rnd(seed, i, 0x5a));
            tops.push(acc);
        }
        let total = acc.max(1e-3);
        for t in &mut tops {
            *t /= total;
        }
        // The outline: corners round a rounded square, each its own
        // distance out and a little off its even angle, so the runs between
        // them are straight but no two alike.
        let half = size * 0.5;
        let count = 17u32;
        let corners = (0..count)
            .map(|i| {
                let a = -std::f32::consts::PI + (i as f32 + 0.5 + 0.55 * (sh::rnd(seed, i, 0x0d) - 0.5)) * std::f32::consts::TAU / count as f32;
                let (c, s) = (a.cos(), a.sin());
                let n = 3.2;
                let sq = (c.abs().powf(n) + s.abs().powf(n)).powf(-1.0 / n);
                let r = half * 0.98 * sq * (1.0 + 0.09 * (sh::rnd(seed, i, 0x0e) - 0.5) + 0.04 * noise::fbm(c * 1.3 + 7.0, s * 1.3 + 3.0, seed ^ 0x01, 2, 2.0, 0.5));
                [r * c, r * s]
            })
            .collect();
        Self { k: size / DESIGN, half, depth: v.f("depth").max(0.05), width: v.f("width").max(0.01), edge: v.f("edge").max(0.0), steps, seed, tops, corners }
    }

    /// The main canyon floor's height (m over the base).
    fn floor(&self) -> f32 {
        self.edge + 0.22 * self.k
    }

    /// The plateau's height (m over the base).
    fn top(&self) -> f32 {
        self.floor() + self.depth
    }

    /// Design-metre noise at the view-plan point `q` (design m): about
    /// -1..1, features `1/f` design metres across.
    fn n(&self, q: [f32; 2], f: f32, o: u32) -> f32 {
        noise::fbm(q[0] * f, q[1] * f, self.seed ^ o, 3, 2.0, 0.5)
    }

    /// The outline's radius (world m) towards plan angle `a` (asset frame):
    /// a broken slab, a polygon of straight runs and blunt corners round a
    /// rounded square, its edges chipped.
    fn outline(&self, a: f32) -> f32 {
        let (c, s) = (a.cos(), a.sin());
        let corner = |i: usize| self.corners[i % self.corners.len()];
        let n = self.corners.len();
        let mut r = self.half;
        for i in 0..n {
            let (p, q) = (corner(i), corner(i + 1));
            let e = [q[0] - p[0], q[1] - p[1]];
            let den = c * e[1] - s * e[0];
            if den.abs() < 1e-6 {
                continue;
            }
            // Where the ray meets this run's line, if inside the run.
            let t = (c * p[1] - s * p[0]) / den;
            let along = -t;
            let rr = (p[0] * e[1] - p[1] * e[0]) / den;
            if rr > 0.0 && (0.0..=1.0).contains(&along) {
                r = rr;
                break;
            }
        }
        let ring = |f: f32, o: u32| noise::fbm(c * f + 7.0, s * f + 3.0, self.seed ^ o, 3, 2.0, 0.5);
        let chips = noise::ridged2(c * 11.0 + 1.0, s * 11.0 + 5.0, self.seed ^ 0x0c, 2);
        r * (1.0 + 0.012 * ring(6.0, 0x02) + 0.006 * ring(18.0, 0x03) - 0.016 * chips)
    }

    /// The grid point (x, z) of the square bent onto the outline.
    fn bend(&self, x: f32, z: f32) -> [f32; 2] {
        let r = (x * x + z * z).sqrt();
        if r < 1e-6 {
            return [x, z];
        }
        let a = z.atan2(x);
        let sq = self.half / a.cos().abs().max(a.sin().abs()).max(1e-3);
        let f = self.outline(a) / sq;
        [x * f, z * f]
    }

    /// The view plan (design m) of world point (x, z): turned 45 degrees so
    /// the block's +x +z corner is the front.
    fn view(&self, x: f32, z: f32) -> [f32; 2] {
        let s = std::f32::consts::FRAC_1_SQRT_2 / self.k;
        [(x - z) * s, (x + z) * s]
    }

    /// At world point (x, z): the share of the walls' height before the
    /// steps (0 the main floor, 1 the plateau; under 0 the base's rim at
    /// the cut edge), how far past the nearest rim the point lies (design
    /// m, plateau only), and its distance from the lowest canyon's centre
    /// line over that floor's half-width (the wash runs along it).
    fn level(&self, x: f32, z: f32) -> (f32, f32, f32) {
        let q = self.view(x, z);
        // The walls wander: spurs and alcoves at three scales, so the cliff
        // lines are crenulated, not smooth curves.
        let w = [q[0] + 0.7 * self.n(q, 0.22, 0x11) + 0.22 * self.n(q, 0.9, 0x13), q[1] + 0.7 * self.n(q, 0.22, 0x12) + 0.22 * self.n(q, 0.9, 0x14)];
        // Gullies: narrow ravines eaten back into the walls (a ridged
        // noise's creases), and buttresses between them.
        let gully = (1.0 - noise::perlin2(q[0] * 0.3, q[1] * 0.3, self.seed ^ 0x1a).abs()).powi(6);
        let crenel = 0.18 * noise::ridged2(q[0] * 1.5, q[1] * 1.5, self.seed ^ 0x15, 2) + 0.07 * self.n(q, 3.5, 0x16) - 0.6 * gully;
        let wf = self.width;
        let (mut lvl, mut past, mut across) = (f32::MAX, f32::MAX, f32::MAX);
        for (i, arm) in ARMS.iter().enumerate() {
            let (d, t, len) = line_dist(arm.line, w);
            let half = arm.half * wf * (0.8 + 0.35 * self.n([t * 9.0, i as f32 * 3.1], 1.0, 0x17).abs());
            let wall = arm.wall * (0.85 + 0.3 * self.n([t * 5.0, i as f32 * 2.3], 1.0, 0x18));
            let s = (d + crenel - self.rill((t * len + i as f32 * 13.0) * 1.7, d * 0.4) - half) / wall;
            // A side canyon's floor rises towards its end.
            let lift = arm.rise * smooth(0.15, 1.0, t);
            let l = lift + (1.0 - lift) * s.clamp(0.0, 1.0);
            if l < lvl {
                lvl = l;
                across = d / half.max(1e-3);
            }
            past = past.min(d + crenel - half - wall);
        }
        // The block's own edges erode in the same steps, down to the rim.
        let (r, a) = ((x * x + z * z).sqrt(), z.atan2(x));
        let out = (self.outline(a) - r) / self.k;
        // (Round the outline the rills' along-the-wall axis is the angle, on
        // a circle so they meet themselves with no seam.)
        let ring = 1.7 * self.half / self.k;
        let inward = out + 0.35 * self.n(q, 0.5, 0x19) + crenel - self.rill(a.cos() * ring + 0.3 * out + 101.0, a.sin() * ring - 0.3 * out);
        let s = (inward - EDGE_RIM) / (EDGE_WALL * (0.75 + 0.6 * smooth(-0.4, 0.5, self.n(q, 0.12, 0x1b))));
        let l = s.clamp(0.0, 1.0) - 0.06 * (1.0 - smooth(0.0, 0.3, s.max(0.0)));
        if l < lvl {
            lvl = l;
            across = f32::MAX;
        }
        (lvl, past.min(inward - EDGE_RIM - EDGE_WALL), across)
    }

    /// Rills (design m, to take off a wall's distance): narrow runnels down
    /// the walls between blunt ribs, at a point (`u`, `v`) of a plane
    /// stretched down the slope (the caller scales `u` along the wall about
    /// four times faster than `v` across it), so the slopes are fluted by
    /// real gullies, not smooth.
    fn rill(&self, u: f32, v: f32) -> f32 {
        let r = noise::ridged2(u, v, self.seed ^ 0x1c, 2);
        let f = noise::ridged2(u * 2.4 + 3.0, v * 2.2, self.seed ^ 0x1d, 1);
        0.36 * r * r * r + 0.1 * f * f
    }

    /// The stepped profile: share `y` of the walls' height (0..1) to the
    /// share drawn, layer by layer a concave talus slope, a sheer cliff band
    /// and a bench at the layer's top; each layer's split wanders with `q`.
    fn stepped(&self, y: f32, q: [f32; 2]) -> f32 {
        self.profile(y, q, true)
    }

    /// [`Land::stepped`], with or without the talus' thin ledges (the
    /// materials follow the profile without them, so a ledge's tread stays
    /// talus, not a patch of sand).
    fn profile(&self, y: f32, q: [f32; 2], ledges: bool) -> f32 {
        if y <= 0.0 {
            return y;
        }
        if y >= 1.0 {
            return 1.0;
        }
        let mut lo = 0.0;
        for (i, &hi) in self.tops.iter().enumerate() {
            if y <= hi || i + 1 == self.tops.len() {
                let f = ((y - lo) / (hi - lo).max(1e-4)).clamp(0.0, 1.0);
                let i = i as u32;
                // Where the talus ends and how wide the cliff is, as shares
                // of the layer's plan width.
                let a = (0.38 + 0.14 * sh::rnd(self.seed, i, 0x21) + 0.12 * self.n([q[0] + i as f32 * 7.0, q[1]], 0.35, 0x22)).clamp(0.2, 0.62);
                let c = (0.2 + 0.1 * sh::rnd(self.seed, i, 0x23) + 0.06 * self.n([q[0], q[1] + i as f32 * 5.0], 0.5, 0x24)).clamp(0.12, 0.34);
                let talus = 0.3 + 0.08 * sh::rnd(self.seed, i, 0x25);
                let g = if f < a {
                    // The talus over the softer beds: thin ledges, each a
                    // short steep riser and a narrow tread.
                    let u = f / a;
                    let m = 3.0;
                    let uq = if ledges { ((u * m).floor() + smooth(0.5, 1.0, (u * m).fract())) / m } else { u };
                    talus * (0.4 * u + 0.6 * uq).powf(1.7)
                } else if f < a + c {
                    let t = (f - a) / c;
                    talus + (0.96 - talus) * t.powf(1.25)
                } else {
                    // The bench, falling a little towards the canyon.
                    1.0 - 0.04 * (1.0 - (f - a - c) / (1.0 - a - c).max(1e-3))
                };
                return lo + (hi - lo) * g;
            }
            lo = hi;
        }
        y
    }

    /// The ground's height (m over the base) at world point (x, z).
    fn height(&self, x: f32, z: f32) -> f32 {
        self.heights(x, z).0
    }

    /// The ground's height and its clean part (the stepped landform without
    /// the rubble, ledges and wash: the slopes the materials follow).
    fn heights(&self, x: f32, z: f32) -> (f32, f32) {
        let (k, q) = (self.k, self.view(x, z));
        let (lvl, past, across) = self.level(x, z);
        let rim_edge = self.edge;
        let floor = self.floor();
        // The base rim at the cut edge, under the main floor's level.
        if lvl < 0.0 {
            let h = rim_edge + (floor - rim_edge) * (1.0 + lvl / 0.06).clamp(0.0, 1.0);
            return (h, h);
        }
        let clean = floor + self.depth * self.profile(lvl, q, false);
        let mut h = floor + self.depth * self.stepped(lvl, q);
        if lvl >= 1.0 {
            // The plateau: gently swelling slickrock in thin ledges, a few
            // low knolls, rising away from the rims.
            let inner = smooth(0.0, 1.4, past);
            let swell = 0.11 * self.n(q, 0.16, 0x31) + 0.05 * self.n(q, 0.7, 0x32) + 0.3 * smooth(0.25, 0.8, self.n(q, 0.11, 0x33));
            let ledge = 0.06;
            let raw = swell.max(0.0) * inner * k;
            let t = raw / (ledge * k);
            let stepped = (t.floor() + smooth(0.75, 1.0, t.fract())) * ledge * k;
            h += 0.55 * stepped + 0.45 * raw + 0.012 * self.n(q, 4.0, 0x34) * k;
        } else if lvl < 1e-3 {
            // The floor: a dry wash winding down its middle (braided round
            // gravel bars), a little rise towards the walls.
            let off = 0.35 * self.n(q, 0.5, 0x41);
            let main = (-((across - 0.55 * off.abs()) / 0.42).powi(2)).exp();
            let braid = 0.5 * (-((across - 0.7 - off) / 0.2).powi(2)).exp() * smooth(0.1, 0.4, self.n(q, 0.8, 0x44));
            h += (-0.05 * main.max(braid) + 0.02 * smooth(0.6, 1.0, across) + 0.01 * self.n(q, 2.5, 0x42)) * k;
        } else {
            // Rough talus and ledges: small rubble lumps on the slopes.
            h += (0.035 * self.n(q, 1.6, 0x43) + 0.02 * self.n(q, 4.5, 0x45)) * k * (1.0 - smooth(0.85, 1.0, lvl));
        }
        (h, clean)
    }

    /// The layer index and its share (0..1) of height `y` (m over the base).
    fn layer_at(&self, y: f32) -> (usize, f32) {
        let f = ((y - self.floor()) / self.depth).clamp(0.0, 1.0);
        let mut lo = 0.0;
        for (i, &hi) in self.tops.iter().enumerate() {
            if f <= hi {
                return (i, (f - lo) / (hi - lo).max(1e-4));
            }
            lo = hi;
        }
        (self.tops.len().saturating_sub(1), 1.0)
    }
}

/// Split `m` along the zero line of `f` (per vertex: position and normal):
/// (where f < 0, where f >= 0). Triangles crossing the line are cut at it,
/// their new corners interpolated (shared along each cut edge), so the
/// border runs smooth instead of following the triangles. (The shoreline's
/// cut, crate::shore, copied.)
fn cut(m: &Mesh, f: impl Fn(&[f32; 6]) -> f32) -> (Mesh, Mesh) {
    let none = vec![0.0; m.vertices.len()];
    let ((lo, _), (hi, _)) = cut_s(m, &none, |v, _| f(v));
    (lo, hi)
}

/// A mesh and one scalar per vertex.
type Carried = (Mesh, Vec<f32>);

/// [`cut`] carrying a scalar `s` per vertex (interpolated on the cut
/// edges), which `f` may read: each side's mesh with its own scalars.
fn cut_s(m: &Mesh, s: &[f32], f: impl Fn(&[f32; 6], f32) -> f32) -> (Carried, Carried) {
    let vals: Vec<f32> = m.vertices.iter().zip(s).map(|(v, s)| f(v, *s)).collect();
    #[derive(Default)]
    struct Side {
        b: geo::MeshBuilder,
        s: Vec<f32>,
        old: HashMap<u32, u32>,
        mid: HashMap<(u32, u32), u32>,
    }
    impl Side {
        fn old(&mut self, m: &Mesh, s: &[f32], i: u32) -> u32 {
            let (b, out) = (&mut self.b, &mut self.s);
            *self.old.entry(i).or_insert_with(|| {
                let v = m.vertices[i as usize];
                out.push(s[i as usize]);
                b.vertex([v[0], v[1], v[2]], [v[3], v[4], v[5]])
            })
        }
        fn mid(&mut self, m: &Mesh, s: &[f32], vals: &[f32], a: u32, c: u32) -> u32 {
            let (b, out) = (&mut self.b, &mut self.s);
            let (a, c) = (a.min(c), a.max(c));
            *self.mid.entry((a, c)).or_insert_with(|| {
                let (va, vc) = (m.vertices[a as usize], m.vertices[c as usize]);
                let (fa, fc) = (vals[a as usize], vals[c as usize]);
                let t = (fa / (fa - fc)).clamp(0.0, 1.0);
                let l = |i: usize| va[i] + (vc[i] - va[i]) * t;
                out.push(s[a as usize] + (s[c as usize] - s[a as usize]) * t);
                b.vertex([l(0), l(1), l(2)], geo::normalize([l(3), l(4), l(5)]))
            })
        }
    }
    let (mut lo, mut hi) = (Side::default(), Side::default());
    for t in m.indices.as_chunks::<3>().0 {
        let neg: Vec<bool> = t.iter().map(|&i| vals[i as usize] < 0.0).collect();
        let count = neg.iter().filter(|x| **x).count();
        if count == 0 || count == 3 {
            let side = if count == 3 { &mut lo } else { &mut hi };
            let ids = [side.old(m, s, t[0]), side.old(m, s, t[1]), side.old(m, s, t[2])];
            side.b.tri(ids[0], ids[1], ids[2]);
            continue;
        }
        let lone = (0..3).find(|&i| neg[i] == (count == 1)).unwrap_or(0);
        let (a, b, c) = (t[lone], t[(lone + 1) % 3], t[(lone + 2) % 3]);
        let (one, two) = if count == 1 { (&mut lo, &mut hi) } else { (&mut hi, &mut lo) };
        let (ab1, ac1, a1) = (one.mid(m, s, &vals, a, b), one.mid(m, s, &vals, a, c), one.old(m, s, a));
        one.b.tri(a1, ab1, ac1);
        let (ab2, ac2, b2, c2) = (two.mid(m, s, &vals, a, b), two.mid(m, s, &vals, a, c), two.old(m, s, b), two.old(m, s, c));
        two.b.tri(ab2, b2, c2);
        two.b.tri(ab2, c2, ac2);
    }
    ((lo.b.finish(), lo.s), (hi.b.finish(), hi.s))
}

/// The ground: a grid of `n` cells a side bent onto the outline, each cell
/// split along its flatter diagonal (so cliff lines run clean, not in a
/// saw-tooth), its border loop (vertex ids round the square) and each
/// vertex's steepness (the clean landform's slope, rise over run, softened
/// a little: the materials follow the cliffs and benches, not every rubble
/// bump, so their borders stay smooth and cheap).
fn ground(l: &Land, n: u32) -> (Mesh, Vec<u32>, Vec<f32>) {
    let mut b = geo::MeshBuilder::new();
    let size = l.half * 2.0;
    let row = n + 1;
    let mut clean = Vec::with_capacity((row * row) as usize);
    for j in 0..=n {
        for i in 0..=n {
            let x = (i as f32 / n as f32 - 0.5) * size;
            let z = (j as f32 / n as f32 - 0.5) * size;
            let p = l.bend(x, z);
            let (h, c) = l.heights(p[0], p[1]);
            clean.push(c);
            b.vertex([p[0], h, p[1]], [0.0, 1.0, 0.0]);
        }
    }
    let r = row as usize;
    let at = |i: usize, j: usize| j * r + i;
    let mut steep = vec![0.0f32; r * r];
    {
        let v = &b.mesh.vertices;
        let d = |k0: usize, k1: usize| {
            let run = ((v[k1][0] - v[k0][0]).powi(2) + (v[k1][2] - v[k0][2]).powi(2)).sqrt().max(1e-5);
            (clean[k1] - clean[k0]) / run
        };
        for j in 0..r {
            for i in 0..r {
                let (i0, i1, j0, j1) = (i.saturating_sub(1), (i + 1).min(r - 1), j.saturating_sub(1), (j + 1).min(r - 1));
                let (gx, gz) = (d(at(i0, j), at(i1, j)), d(at(i, j0), at(i, j1)));
                steep[at(i, j)] = (gx * gx + gz * gz).sqrt();
            }
        }
    }
    // Soften: half the vertex, half its four neighbours.
    let soft: Vec<f32> = (0..r * r)
        .map(|k| {
            let (i, j) = (k % r, k / r);
            let nb = [at(i.saturating_sub(1), j), at((i + 1).min(r - 1), j), at(i, j.saturating_sub(1)), at(i, (j + 1).min(r - 1))];
            0.5 * steep[k] + 0.125 * nb.iter().map(|&q| steep[q]).sum::<f32>()
        })
        .collect();
    let y = |b: &geo::MeshBuilder, i: u32| b.mesh.vertices[i as usize][1];
    for j in 0..n {
        for i in 0..n {
            let a = j * row + i;
            let (c0, c1, c2, c3) = (a, a + row, a + row + 1, a + 1);
            if (y(&b, c0) - y(&b, c2)).abs() <= (y(&b, c1) - y(&b, c3)).abs() {
                b.quad(c0, c1, c2, c3);
            } else {
                b.tri(c0, c1, c3);
                b.tri(c1, c2, c3);
            }
        }
    }
    let mut m = b.finish();
    geo::smooth_normals(&mut m);
    // The border, counter-clockwise from above round the square.
    let mut border = Vec::with_capacity(4 * n as usize);
    border.extend(0..n);
    border.extend((0..n).map(|j| j * row + n));
    border.extend((0..n).map(|i| n * row + n - i));
    border.extend((0..n).map(|j| (n - j) * row));
    (m, border, soft)
}

/// The base's cut sides: from the ground's border down to the floor,
/// facing out, in irregular courses (each a little proud of the one under
/// it, rough at two scales: real ledges, not paint).
fn sides(l: &Land, m: &Mesh, border: &[u32], rows: u32) -> Mesh {
    let mut b = geo::MeshBuilder::new();
    let row = rows + 1;
    let k = l.k;
    for (j, &i) in border.iter().enumerate() {
        let v = m.vertices[i as usize];
        let r = (v[0] * v[0] + v[2] * v[2]).sqrt().max(1e-4);
        let out = [v[0] / r, 0.0, v[2] / r];
        for s in 0..=rows {
            let y = v[1] * s as f32 / rows as f32;
            let a = j as f32 * 0.37;
            let course = (y / (0.11 * k) + 0.6 * noise::fbm(a * 0.2, y / k * 0.5, l.seed ^ 0x61, 2, 2.0, 0.5)).fract();
            let off = ((course - 0.5) * 0.02 + 0.012 * noise::fbm(a * 1.3, y / k * 4.0, l.seed ^ 0x62, 2, 2.0, 0.5)) * k * smooth(0.0, 0.04 * k, v[1] - y);
            b.vertex([v[0] + out[0] * off, y, v[2] + out[2] * off], out);
        }
    }
    let count = border.len() as u32;
    for j in 0..count {
        let (a, c) = (j * row, ((j + 1) % count) * row);
        for s in 0..rows {
            b.quad(a + s, c + s, c + s + 1, a + s + 1);
        }
    }
    let mut out = b.finish();
    sh::fix_winding(&mut out);
    geo::smooth_normals(&mut out);
    out
}

/// The walls' banded tints: share `f` of the walls' height (0 the floor, 1
/// the rim) to a sandstone colour, `base` the colour param (the red).
fn band(base: [f32; 3], f: f32) -> [f32; 3] {
    let stops: [(f32, [f32; 3]); 6] = [
        (0.0, [0.72, 0.74, 0.86]),
        (0.25, [0.95, 0.88, 0.88]),
        (0.5, [1.15, 1.02, 0.95]),
        (0.68, [1.3, 1.3, 1.25]),
        (0.86, [2.0, 2.1, 1.9]),
        (1.0, [1.55, 1.6, 1.55]),
    ];
    let mut out = stops[0].1;
    for w in stops.windows(2) {
        if f >= w[0].0 {
            let t = ((f - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0);
            out = std::array::from_fn(|c| w[0].1[c] + (w[1].1[c] - w[0].1[c]) * t);
        }
    }
    // (Less saturated than the plain red: the walls' rock reads brown-orange
    // under the sky, not salmon.)
    mul(base, mul(out, WALL_TINT))
}

/// The walls' and talus' shift off the colour param's red (more green and
/// blue: weathered, dusty rock).
const WALL_TINT: [f32; 3] = [1.0, 1.1, 1.2];

fn push(parts: &mut Vec<Part>, m: Mesh, s: Surface) {
    if !m.indices.is_empty() {
        parts.push(Part::surfaced(m, s));
    }
}

/// A catalogue surface with all three of its colours set: `a` and its
/// colours B and C as `a` times `b` and `c` (a plain tint scales B and C by
/// A's change, which turned the red sandstone's cream bed and the red clay's
/// grey stones pale pink and blue-grey under a darker red).
fn toned(s: Surface, a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Surface {
    s.set_colour(0, a).set_colour(1, mul(a, b)).set_colour(2, mul(a, c))
}

/// Red sandstone in uneven beds (the stone recipe's bedded variant: beds of
/// their own tones, iron lines, laminae, sand grains and pits), colour `rgb`.
fn sandstone(rgb: [f32; 3], seed: u32, scale: f32) -> Surface {
    toned(sp::ROCK_SANDSTONE_RED.variant(10), rgb, [1.32, 1.36, 1.4], [1.6, 1.7, 1.85]).number(2, 0.3).number(3, 0.9).seed(seed).scale(scale)
}

/// Ground (the ground recipe: A the soil, B its stones, C its sand tint).
fn soil(s: Surface, a: [f32; 3], b: [f32; 3], c: [f32; 3], seed: u32, scale: f32) -> Surface {
    toned(s, a, b, c).number(3, 0.0).seed(seed).scale(scale)
}

/// The asset.
pub fn generate(v: &Values, seed: u32) -> Output {
    let l = Land::new(v, seed);
    let k = l.k;
    let density = v.density();
    let cells = ((110.0 + 170.0 * density.min(1.0)) as u32).clamp(90, 280);
    let (ground, border, steep) = ground(&l, cells);
    let red = v.color("color");
    let mut parts: Vec<Part> = Vec::new();

    // The base's cut sides: dark, weathered rock in courses.
    if l.edge > 0.0 {
        let base = sides(&l, &ground, &border, 6);
        push(&mut parts, base, sandstone(mul(red, [0.45, 0.58, 0.68]), seed ^ 0x70, 1.2));
    }

    // By slope: sheer cliff bands, talus slopes, and the flats (plateau,
    // benches, floors), each border wandering a little.
    // (Rise over run on the clean landform: cliffs past about 58 degrees,
    // talus past about 30; the rubble's bumps keep their own shading.)
    let wob = |p: &[f32; 6], o: u32| 0.12 * noise::fbm(p[0] / k * 0.9, p[2] / k * 0.9, seed ^ o, 2, 2.0, 0.5);
    let ((rest, rest_s), (cliff, _)) = cut_s(&ground, &steep, |p, s| s - 1.6 - 2.0 * wob(p, 0x81));
    let ((flat, _), (talus, _)) = cut_s(&rest, &rest_s, |p, s| s - 0.58 - wob(p, 0x82));

    // The cliffs in banded strata: one band per layer, each its own tone of
    // the red sandstone (pale orange and buff near the rim, red in the
    // middle, dark maroon at the foot), the upper two layers with a pale
    // hard cap bed; the stone's own beds band each layer finely. (The layer
    // tops run along the benches, so cutting there splits few triangles;
    // each cap cut runs the length of every cliff, so only the two that
    // show most.)
    let mut cliffs = cliff;
    let n = l.steps as usize;
    for i in 0..n {
        let cut_at = l.floor() + l.depth * l.tops[i];
        let (below, above) = cut(&cliffs, |p| p[1] - cut_at - 0.05 * k * noise::fbm(p[0] / k * 0.5, p[2] / k * 0.5, seed ^ 0x83, 2, 2.0, 0.5));
        cliffs = above;
        let lo = if i == 0 { l.floor() } else { l.floor() + l.depth * l.tops[i - 1] };
        let f_mid = (0.5 * (lo + cut_at) - l.floor()) / l.depth;
        let f_top = (cut_at - l.floor()) / l.depth;
        if i + 2 >= n {
            let cap_at = lo + (cut_at - lo) * (0.78 + 0.08 * sh::rnd(seed, i as u32, 0x84));
            let (body, cap) = cut(&below, |p| p[1] - cap_at - 0.03 * k * noise::fbm(p[0] / k * 0.9, p[2] / k * 0.9, seed ^ 0x85, 2, 2.0, 0.5));
            push(&mut parts, body, sandstone(band(red, f_mid), seed ^ (0x90 + i as u32), 1.4));
            push(&mut parts, cap, sandstone(mul(band(red, f_top), [1.35, 1.4, 1.32]), seed ^ (0xa0 + i as u32), 1.4));
        } else {
            push(&mut parts, below, sandstone(band(red, f_mid), seed ^ (0x90 + i as u32), 1.4));
        }
    }
    // Whatever cliff is left over the last layer (the plateau's knolls).
    push(&mut parts, cliffs, sandstone(band(red, 1.0), seed ^ 0xaf, 1.4));

    // Talus: red clay and rubble, paler under the pale beds.
    let (talus_lo, talus_hi) = cut(&talus, |p| p[1] - l.floor() - l.depth * 0.55 - 0.2 * k * noise::fbm(p[0] / k * 0.6, p[2] / k * 0.6, seed ^ 0x86, 2, 2.0, 0.5));
    let rubble = |a: [f32; 3], o: u32| soil(sp::GROUND_SOIL_CLAY_RED, a, [1.25, 1.3, 1.35], [1.15, 1.25, 1.3], seed ^ o, 0.6).number(1, 0.3).number(2, 0.55);
    push(&mut parts, talus_lo, rubble(mul(red, [1.0, 1.08, 1.12]), 0xb0));
    push(&mut parts, talus_hi, rubble(mul(red, [1.2, 1.36, 1.42]), 0xb1));

    // The flats: the base's rim at the cut edge, the canyon floors (the
    // wash's gravel, the red silt beside it), the benches and the plateau
    // (sand over slickrock, the bare-rock share from `rock`; snow from the
    // top down with `snow`).
    let (rim, flat) = cut(&flat, |p| p[1] - (l.floor() - 0.07 * k));
    push(&mut parts, rim, rubble(mul(red, [0.7, 0.8, 0.88]), 0xb2));
    let floor_top = l.floor() + 0.06 * l.depth;
    let (floors, high) = cut(&flat, |p| p[1] - floor_top);
    let (wash, silt) = cut(&floors, |p| {
        let q = l.view(p[0], p[2]);
        p[1] - l.floor() + 0.018 * k + 0.01 * k * l.n(q, 3.0, 0x87)
    });
    push(&mut parts, wash, soil(sp::GROUND_GRAVEL_CREEK, mul(red, [1.5, 2.15, 2.6]), [1.22, 1.22, 1.2], [1.1, 1.1, 1.1], seed ^ 0xc0, 0.35).number(1, 0.1));
    push(&mut parts, silt, soil(sp::GROUND_SOIL_CLAY_RED, mul(red, [1.2, 1.45, 1.7]), [1.3, 1.35, 1.4], [1.2, 1.3, 1.35], seed ^ 0xc1, 0.6));
    let snow = v.f("snow").clamp(0.0, 1.0);
    let (bare, snowy) = if snow > 0.0 {
        let line = l.top() - snow * l.depth * 1.05;
        cut(&high, |p| p[1] - line - 0.05 * k * noise::fbm(p[0] / k, p[2] / k, seed ^ 0x88, 2, 2.0, 0.5))
    } else {
        (high, Mesh::default())
    };
    push(&mut parts, snowy, sp::GROUND_SNOW_PACKED.seed(seed ^ 0xc2));
    // Sand: buff-tan, with darker red-brown grit and pale grains; the
    // slickrock a little redder and darker, in small patches.
    let rock = v.f("rock").clamp(0.0, 1.0);
    let (sand, slick) = cut(&bare, |p| {
        let q = l.view(p[0], p[2]);
        l.n(q, 0.8, 0x89) + 0.45 * l.n(q, 2.2, 0x8a) - (0.75 - 0.6 * rock)
    });
    // (The ground recipe's soil variant: its sand variant always draws
    // ripples, and the plateau's sand is wind-packed, not rippled.)
    let tan = mul(red, [2.0, 2.55, 2.6]);
    push(&mut parts, sand, soil(sp::GROUND_SOIL_PACKED.variant(0), tan, [0.62, 0.56, 0.56], [1.12, 1.12, 1.1], seed ^ 0xc3, 0.5).number(1, 0.1).number(2, 0.3).number(6, 0.55));
    // Slickrock: the stone recipe's granular sandstone with no strata (beds
    // by height drew contour stripes on the flats).
    push(&mut parts, slick, toned(sp::ROCK_SANDSTONE_RED.variant(0), mul(red, [1.75, 2.05, 2.0]), [1.15, 1.15, 1.12], [1.3, 1.3, 1.25]).number(1, 0.0).number(4, 0.0).seed(seed ^ 0xc4).scale(1.0));

    // Scrub: small dark bushes in clumps on the flats; fallen blocks on the
    // talus and the floors (real geometry).
    let scrub = v.f("scrub").clamp(0.0, 1.0);
    let mut bushes: [Mesh; 2] = Default::default();
    let mut blocks: [Mesh; 2] = Default::default();
    let n_bush = (1600.0 * scrub * density.clamp(0.4, 1.0)) as u32;
    let n_block = (420.0 * scrub * density.clamp(0.4, 1.0)) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    let span = l.half * 0.985;
    while made < n_bush + n_block && i < (n_bush + n_block) * 12 {
        i += 1;
        let p = l.bend(sh::range(seed, i, 51, -span, span), sh::range(seed, i, 52, -span, span));
        let (x, z) = (p[0], p[1]);
        let e = 0.04 * k;
        let h = l.height(x, z);
        // Nothing on the base's rim at the cut edge.
        if h < l.floor() - 0.02 * k {
            continue;
        }
        let gx = (l.height(x + e, z) - l.height(x - e, z)) / (2.0 * e);
        let gz = (l.height(x, z + e) - l.height(x, z - e)) / (2.0 * e);
        let slope = (gx * gx + gz * gz).sqrt();
        let q = l.view(x, z);
        let block = made >= n_bush;
        if block {
            // Blocks under the cliffs: on talus and the floors.
            if !(0.15..1.6).contains(&slope) && sh::rnd(seed, i, 53) > 0.15 {
                continue;
            }
            let r = k * (0.03 + 0.09 * sh::rnd(seed, i, 54).powi(3));
            let mut m = sh::blob([r, r * sh::range(seed, i, 55, 0.45, 0.8), r * sh::range(seed, i, 56, 0.6, 1.0)], [0.0, 0.0, 0.0], 0.25, seed.wrapping_add(i * 29), 0);
            sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 57) * sh::TAU);
            geo::transform(&mut m, [1.0; 3], [x, h - r * 0.25, z]);
            let (layer, _) = l.layer_at(h + 0.3 * k);
            geo::merge(&mut blocks[usize::from(layer * 2 >= l.steps as usize)], &m);
        } else {
            if slope > 0.35 {
                continue;
            }
            let clump = smooth(-0.1, 0.5, l.n(q, 0.8, 0x91) + 0.4 * l.n(q, 3.0, 0x92));
            if sh::rnd(seed, i, 53) > 0.06 + 0.94 * clump {
                continue;
            }
            let r = k * (0.022 + 0.04 * sh::rnd(seed, i, 54).powi(2));
            let mut m = sh::blob([r, r * sh::range(seed, i, 55, 0.5, 0.8), r], [0.0, 0.0, 0.0], 0.35, seed.wrapping_add(i * 31), 0);
            geo::transform(&mut m, [1.0; 3], [x, h + r * 0.2, z]);
            geo::merge(&mut bushes[usize::from(sh::rnd(seed, i, 58) < 0.35)], &m);
        }
        made += 1;
    }
    let shrub = |rgb: [f32; 3], o: u32| sp::GROUND_SOIL_PACKED.tint(rgb).seed(seed ^ o).scale(0.12);
    for (j, (m, c)) in bushes.into_iter().zip([[0.065, 0.066, 0.042], [0.105, 0.1, 0.072]]).enumerate() {
        push(&mut parts, m, shrub(c, 0xd0 + j as u32));
    }
    for (j, m) in blocks.into_iter().enumerate() {
        push(&mut parts, m, sandstone(mul(band(red, 0.25 + 0.4 * j as f32), [0.9, 0.9, 0.9]), seed ^ (0xd4 + j as u32), 0.5));
    }
    Output::Parts(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(d: f32) -> Output {
        let specs: Vec<P> = PARAMS.to_vec();
        generate(&Values::defaults(&specs).with_density(d), 7)
    }

    #[test]
    fn the_canyon_stands_on_the_floor_within_its_budget() {
        let (hi, lo) = (make(1.0), make(evani_procedural::detail::Density::for_tier("low")));
        let (t_hi, t_lo) = (hi.triangles(), lo.triangles());
        println!("terrain-canyons: {t_hi} triangles at High, {t_lo} at Low");
        if let Output::Parts(ps) = &hi {
            let each: Vec<usize> = ps.iter().map(|p| p.mesh.indices.len() / 3).collect();
            println!("terrain-canyons parts at High: {each:?}");
        }
        assert!(hi.is_valid() && t_hi < 370_000 && t_lo <= t_hi, "{t_hi} / {t_lo}");
        let (lo_b, hi_b) = hi.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo_b[1] > -1e-3, "base at {}", lo_b[1]);
        assert!(hi_b[1] - lo_b[1] > 3.0, "the plateau stands over the base");
    }

    #[test]
    fn the_walls_step_in_cliffs_and_benches() {
        let l = Land::new(&Values::defaults(&PARAMS), 7);
        // Going up the walls the profile only rises, and each layer has a
        // cliff (a steep stretch) and a bench (a flat one).
        let mut prev = -1.0;
        let (mut steep, mut flat) = (0, 0);
        for i in 0..=400 {
            let y = l.stepped(i as f32 / 400.0, [0.0, 0.0]);
            assert!(y >= prev - 1e-5, "monotone at {i}");
            if y - prev > 0.004 {
                steep += 1;
            } else if y - prev < 0.0008 {
                flat += 1;
            }
            prev = y;
        }
        assert!(steep > 20 && flat > 40, "cliffs {steep}, benches {flat}");
    }
}
