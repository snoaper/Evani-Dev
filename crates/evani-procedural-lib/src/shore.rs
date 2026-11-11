//! Shorelines (`water-shorelines`, Asset Studio column 10, 2026-11-05/06):
//! a cut block of coast. A sandy cove curves between a grassy dune ridge and
//! a granite headland; the beach runs down through a damp and a wet band
//! into clear water over a sand bed with sunken stones; granite masses break
//! the beach's front corner; pebbles and shells lie along an irregular
//! strandline; marram grass grows on the dunes in wind-bent tufts with low
//! mats and bare sand between; the cut sides show weathered strata.
//!
//! Everything solid is real geometry in catalogue materials (dry, damp and
//! wet sand, the bed, the strata of the cut sides, grey, ochre-weathered and
//! wet granite with real joints between its blocks and carved cracks,
//! pebbles, shells, grass), so a game gets the same asset the library shows.
//! The water is the engine's nature water (evani-material-nature, a
//! `WaterRect` at the asset's water level: depth colour, Fresnel, sun glint,
//! waves, and the coast upgrade's lace foam, swash fronts, caustics and
//! ripples); the host hands it to the nature pass with [`water`] and sets
//! its look with [`WATER_LOOK`] (the proof game's `shore` scene does both).
//! The water's cut faces at the block's sides are solid glassy faces.
//!
//! The layout is designed at the default 12 m and scales with `size`.

use std::collections::HashMap;

use evani_procedural::surface::presets as sp;
use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};

type Mesh = evani_render::mesh::Mesh;

/// The design size (m): every plan coordinate below is at this size.
const DESIGN: f32 = 12.0;

pub const PARAMS: [P; 9] = [
    P::num("size", 12.0, 4.0, 80.0).unit("m").describe("Width of the square block of coast (m)"),
    P::num("depth", 1.15, 0.1, 4.0).unit("m").describe("Water depth at the far side of the bay (m)"),
    P::num("dunes", 1.2, 0.0, 6.0).unit("m").describe("Height of the dune ridge over the beach (m)"),
    P::num("rocks", 1.0, 0.0, 1.0).describe("Granite: the headland, the beach's corner and the stones under the water (0: none)"),
    P::num("grass", 1.0, 0.0, 1.0).describe("Marram grass and low plants on the dunes (0: bare sand)"),
    P::num("pebbles", 1.0, 0.0, 1.0).describe("Pebbles and shells along the strandline (0: none)"),
    P::num("edge", 0.15, 0.0, 3.0).unit("m").describe("Rock under the deepest sea bed, shown as the block's cut sides (m; 0: none, for a coast set into a world)"),
    P::color("color", SAND).describe("Dry sand colour"),
    P::color("water", WATER_SIDE).describe("Colour of the water's cut faces at the block's sides"),
];

/// Warm dry beach sand, its overall (mean) colour (linear).
pub const SAND: [f32; 3] = [0.275, 0.217, 0.157];
/// Wet sand at the water's edge (linear, mean).
const WET: [f32; 3] = [0.12, 0.092, 0.068];
/// Weathered grey granite (linear).
const GRANITE: [f32; 3] = [0.2, 0.19, 0.175];
/// Granite under ochre lichen and iron staining (linear).
const OCHRE: [f32; 3] = [0.27, 0.17, 0.07];
/// Wet granite at the waterline (linear).
const WET_ROCK: [f32; 3] = [0.07, 0.066, 0.055];
/// The water's cut faces (linear deep teal).
pub const WATER_SIDE: [f32; 3] = [0.012, 0.06, 0.066];

/// A sand surface whose overall colour is `rgb`: the sand presets' colour
/// A is their darkest grain and B the body, so A is set to keep the body
/// at `rgb` (tinting A to the body colour lit the paler grains past white).
fn sand_tint(s: evani_procedural::Surface, rgb: [f32; 3]) -> evani_procedural::Surface {
    let (a, b) = (s.colour(0), s.colour(1));
    s.tint(std::array::from_fn(|c| rgb[c] * a[c] / b[c].max(1e-3)))
}

/// [`sand_tint`] with a wider grain: darker dark grains, paler pale ones
/// (best.png's sand is coarse and speckled, not an even tone).
fn gritty(s: evani_procedural::Surface, rgb: [f32; 3]) -> evani_procedural::Surface {
    sand_tint(s, rgb).set_colour(0, mul(rgb, [0.42, 0.4, 0.38])).set_colour(2, mul(rgb, [1.45, 1.45, 1.42]))
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// The waterline in plan (x out to sea, z towards the front), at the design
/// size: the cove's sand edge from behind the headland round to the front.
const WATERLINE: [[f32; 2]; 12] = [
    [1.6, -6.4], [1.0, -4.6], [0.4, -2.7], [-1.2, -1.7], [-1.75, -0.8], [-2.0, 0.7],
    [-2.5, 1.7], [-2.9, 2.7], [-3.0, 3.5], [-2.4, 4.6], [-1.2, 5.3], [0.4, 6.4],
];
/// The dune ridge's crest in plan, along the block's land edges.
const CREST: [[f32; 2]; 7] = [[-6.6, 5.9], [-6.0, 4.6], [-5.85, 1.2], [-4.7, -1.2], [-3.2, -3.4], [-2.2, -4.5], [-1.9, -6.6]];
/// The headland's mound (plan centre, radius, height over the beach).
const HEADLAND: ([f32; 2], f32, f32) = ([-1.0, -4.1], 2.7, 0.6);

/// The asset's water level over its base (m).
pub fn water_level(v: &Values) -> f32 {
    v.f("edge").max(0.0) + v.f("depth").max(0.05)
}

/// The signed distance (m) from `p` to the polyline `line` (scaled by
/// `k`): positive on the +x side of its run (the polyline runs towards +z).
fn side(line: &[[f32; 2]], k: f32, p: [f32; 2]) -> f32 {
    let mut best = (f32::MAX, 1.0f32);
    for w in line.windows(2) {
        let (a, b) = ([w[0][0] * k, w[0][1] * k], [w[1][0] * k, w[1][1] * k]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let q = [p[0] - a[0], p[1] - a[1]];
        let t = ((q[0] * d[0] + q[1] * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(1e-6)).clamp(0.0, 1.0);
        let e = [q[0] - d[0] * t, q[1] - d[1] * t];
        let dist = (e[0] * e[0] + e[1] * e[1]).sqrt();
        if dist < best.0 {
            let s = d[1] * q[0] - d[0] * q[1];
            best = (dist, if s >= 0.0 { 1.0 } else { -1.0 });
        }
    }
    best.0 * best.1
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The ground's height (m over the asset's base) at plan point (x, z): the
/// sea bed, the beach, the dunes and the headland's mound.
struct Ground {
    k: f32,
    wl: f32,
    depth: f32,
    dunes: f32,
    seed: u32,
}

impl Ground {
    fn new(v: &Values, seed: u32) -> Self {
        Self { k: v.f("size").max(1.0) / DESIGN, wl: water_level(v), depth: v.f("depth").max(0.05), dunes: v.f("dunes").max(0.0), seed }
    }

    /// Plan distances in design metres: (from the waterline, + at sea; from
    /// the crest, + outwards past it, - towards the beach: the crest runs
    /// towards -z, so its +x side is the negative one).
    fn plan(&self, x: f32, z: f32) -> (f32, f32) {
        let k = self.k;
        (side(&WATERLINE, k, [x, z]) / k, side(&CREST, k, [x, z]) / k)
    }

    /// Design-metre noise at (x, z) (world m): about -1..1, features `1/f`
    /// design metres across.
    fn n(&self, x: f32, z: f32, f: f32, o: u32) -> f32 {
        noise::fbm(x / self.k * f, z / self.k * f, self.seed ^ o, 3, 2.0, 0.5)
    }

    fn height(&self, x: f32, z: f32) -> f32 {
        let k = self.k;
        let (s, t) = self.plan(x, z);
        let (dx, dz) = (x / k, z / k);
        let n = |f: f32, o: u32| self.n(x, z, f, o);
        // Sea bed: shelving away from the waterline, soft irregular lumps
        // and sand waves (no regular ripples). The beach: rising from the
        // waterline over a berm, the upper beach trampled and scoured.
        let base = if s > 0.0 {
            let d = self.depth * (0.8 * smooth(0.0, 8.5, s) + 0.2 * (1.0 - (-s / 1.2).exp())) * (1.0 + 0.1 * n(0.4, 11));
            let lumps = 0.02 * n(1.4, 12) + 0.008 * n(4.2, 15);
            self.wl - d + lumps * smooth(0.3, 1.5, s) * k
        } else {
            let u = -s;
            let rise = 0.085 * u.min(2.6) + 0.035 * (u - 2.6).max(0.0) + 0.04 * smooth(0.8, 1.6, u);
            let pit = 1.0 - smooth(0.0, 0.32, noise::cells2(dx * 2.6, dz * 2.6, self.seed ^ 0x16).0);
            let trod = smooth(0.05, 0.4, n(0.7, 17));
            let rough = (0.006 * n(1.8, 13) - 0.007 * pit * trod) * smooth(1.0, 2.2, u);
            self.wl + (rise + rough) * k + 0.02 * n(0.9, 14) * k * smooth(0.3, 1.5, u)
        };
        // The dune ridge: highest along the crest, falling to its foot on
        // the beach, in irregular hummocks; only on land.
        let ridge = if t > 0.0 { 1.0 - smooth(0.15, 1.9, t) } else { 1.0 - 0.1 * (-t).min(2.0) };
        let land = smooth(-0.8, -2.0, s);
        let hummock = 0.78 + 0.42 * n(0.33, 21) + 0.22 * n(1.0, 22) + 0.1 * n(2.7, 23);
        let dune = self.dunes * ridge * land * hummock.max(0.3);
        // The headland's mound under its granite (also under the sea).
        let (c, r, h) = HEADLAND;
        let dr = ((dx - c[0]).powi(2) + (dz - c[1]).powi(2)).sqrt() / r;
        let mound = if dr < 1.0 { h * (1.0 - dr * dr).powi(2) * (self.dunes / 1.2).clamp(0.3, 2.0) } else { 0.0 };
        base + (dune + mound * k).max(0.0)
    }

    /// The wet band's top over the water level (m): where the last wave
    /// soaked the sand, a gently wandering line.
    fn wet_top(&self, x: f32, z: f32) -> f32 {
        (0.022 + 0.008 * self.n(x, z, 1.3, 31)) * self.k
    }

    /// The damp band's top over the water level (m): the highest reach of
    /// the swash, scalloped into lobes.
    fn damp_top(&self, x: f32, z: f32) -> f32 {
        let lobe = noise::cells2(x / self.k * 1.5, z / self.k * 1.5, self.seed ^ 0x23).0;
        (0.05 + 0.016 * self.n(x, z, 0.7, 32) + 0.016 * lobe) * self.k
    }
}

/// Split `m` along the zero line of `f` (per vertex: position and normal):
/// (where f < 0, where f >= 0). Triangles crossing the line are cut at it, their new corners
/// interpolated (shared along each cut edge), so the border runs smooth
/// instead of following the triangles.
fn cut(m: &Mesh, f: impl Fn(&[f32; 6]) -> f32) -> (Mesh, Mesh) {
    let vals: Vec<f32> = m.vertices.iter().map(&f).collect();
    #[derive(Default)]
    struct Side {
        b: geo::MeshBuilder,
        old: HashMap<u32, u32>,
        mid: HashMap<(u32, u32), u32>,
    }
    impl Side {
        fn old(&mut self, m: &Mesh, i: u32) -> u32 {
            let b = &mut self.b;
            *self.old.entry(i).or_insert_with(|| {
                let v = m.vertices[i as usize];
                b.vertex([v[0], v[1], v[2]], [v[3], v[4], v[5]])
            })
        }
        fn mid(&mut self, m: &Mesh, vals: &[f32], a: u32, c: u32) -> u32 {
            let b = &mut self.b;
            let (a, c) = (a.min(c), a.max(c));
            *self.mid.entry((a, c)).or_insert_with(|| {
                let (va, vc) = (m.vertices[a as usize], m.vertices[c as usize]);
                let (fa, fc) = (vals[a as usize], vals[c as usize]);
                let t = (fa / (fa - fc)).clamp(0.0, 1.0);
                let l = |i: usize| va[i] + (vc[i] - va[i]) * t;
                b.vertex([l(0), l(1), l(2)], geo::normalize([l(3), l(4), l(5)]))
            })
        }
    }
    let (mut lo, mut hi) = (Side::default(), Side::default());
    for t in m.indices.as_chunks::<3>().0 {
        let neg: Vec<bool> = t.iter().map(|&i| vals[i as usize] < 0.0).collect();
        let count = neg.iter().filter(|x| **x).count();
        if count == 0 || count == 3 {
            let s = if count == 3 { &mut lo } else { &mut hi };
            let ids = [s.old(m, t[0]), s.old(m, t[1]), s.old(m, t[2])];
            s.b.tri(ids[0], ids[1], ids[2]);
            continue;
        }
        // The corner alone on its side first, the winding kept.
        let lone = (0..3).find(|&i| neg[i] == (count == 1)).unwrap_or(0);
        let (a, b, c) = (t[lone], t[(lone + 1) % 3], t[(lone + 2) % 3]);
        let (one, two) = if count == 1 { (&mut lo, &mut hi) } else { (&mut hi, &mut lo) };
        let (ab1, ac1, a1) = (one.mid(m, &vals, a, b), one.mid(m, &vals, a, c), one.old(m, a));
        one.b.tri(a1, ab1, ac1);
        let (ab2, ac2, b2, c2) = (two.mid(m, &vals, a, b), two.mid(m, &vals, a, c), two.old(m, b), two.old(m, c));
        two.b.tri(ab2, b2, c2);
        two.b.tri(ab2, c2, ac2);
    }
    (lo.b.finish(), hi.b.finish())
}

/// A granite block about `size` (x, y, z, m) with its base at y = 0: a
/// rounded box (granite weathers to blocks with worn arrises) with a gently
/// domed top, broken across two or three corners and edges by flat fracture
/// faces, its faces dented and grained at two scales, and with a joint
/// crack carved through it when `crack` (real geometry, not paint).
fn granite_block(size: [f32; 3], seed: u32, sub: u32, crack: bool) -> Mesh {
    let mut m = sh::icosphere(sub);
    let r = |k: u32| sh::rnd(seed, 7, k);
    // Fracture faces across corners and edges: planes facing a corner or
    // an edge of the box, cutting a share of it away.
    let planes: Vec<([f32; 3], f32)> = (0..3u32)
        .take(2 + (r(1) * 2.0) as usize)
        .map(|i| {
            let sx = if sh::rnd(seed, i, 2) < 0.5 { -1.0 } else { 1.0 };
            let sz = if sh::rnd(seed, i, 3) < 0.5 { -1.0 } else { 1.0 };
            let up = sh::range(seed, i, 4, -0.2, 1.0);
            let n = geo::normalize([sx * sh::range(seed, i, 5, 0.3, 1.0), up, sz * sh::range(seed, i, 6, 0.3, 1.0)]);
            (n, sh::range(seed, i, 8, 0.62, 0.88))
        })
        .collect();
    // The crack: a joint parallel to one of the block's faces.
    let along_x = r(5) < 0.5;
    let co = sh::range(seed, 9, 7, -0.3, 0.3);
    let (hx, hy, hz) = (size[0] * 0.5, size[1] * 0.5, size[2] * 0.5);
    let scale = hx.max(hy).max(hz);
    for v in &mut m.vertices {
        let d = [v[0], v[1], v[2]];
        let q = (d[0].abs().powi(10) + d[1].abs().powi(10) + d[2].abs().powi(10)).powf(0.1).max(1e-4);
        let mut p = [d[0] / q, d[1] / q, d[2] / q];
        for (n, o) in &planes {
            let e = p[0] * n[0] + p[1] * n[1] + p[2] * n[2] - o;
            if e > 0.0 {
                p = [p[0] - n[0] * e, p[1] - n[1] * e, p[2] - n[2] * e];
            }
        }
        // A domed, worn top; it settles on a flatter underside.
        if p[1] > 0.0 {
            p[1] *= 1.0 - 0.06 * (p[0] * p[0] + p[2] * p[2]);
        } else if p[1] < -0.6 {
            p[1] = -0.6 + (p[1] + 0.6) * 0.25;
        }
        let mut w = [p[0] * hx, p[1] * hy, p[2] * hz];
        // Weathering in metres: broad dents, then a fine grain relief.
        let wp = [w[0] * 1.3 + (seed % 997) as f32 * 0.37, w[1] * 1.3, w[2] * 1.3];
        let dent = 0.035 * noise::fbm3(wp, seed, 3) + 0.01 * noise::fbm3([wp[0] * 5.0, wp[1] * 5.0, wp[2] * 5.0], seed ^ 0x5a, 2);
        let len = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt().max(1e-4);
        let mut g = 1.0 + dent * scale.min(1.2) / len;
        if crack {
            let e = if along_x { (p[0] - co).abs() * hx } else { (p[2] - co).abs() * hz };
            let width = 0.06;
            if e < width {
                let f = 1.0 - e / width;
                g -= 0.14 * f * f * scale.min(1.0) / len.max(0.3);
            }
        }
        w = [w[0] * g, w[1] * g, w[2] * g];
        *v = [w[0], w[1], w[2], 0.0, 1.0, 0.0];
    }
    geo::smooth_normals(&mut m);
    sh::to_ground(&mut m);
    m
}

/// One planned granite block: plan position, size and yaw (design m and
/// radians), how far it sinks into the ground (share of its height), the
/// height it rests on when stacked (m over the ground, design), and
/// whether a joint crack runs through it.
struct Block {
    at: [f32; 2],
    size: [f32; 3],
    yaw: f32,
    sink: f32,
    rest: f32,
    crack: bool,
}

/// A jointed granite mass: blocks on a grid turned to the mass's joint
/// heading (`yaw`, degrees), `cell` apart (x, z, design m), up to `h` high
/// in the middle and lower towards the edges; some cells left open so the
/// outline is ragged, and (with `tier`) smaller blocks stacked on the
/// middle ones.
struct Mass {
    at: [f32; 2],
    yaw: f32,
    grid: [u32; 2],
    cell: [f32; 2],
    h: f32,
    tier: bool,
}

/// Every granite block of the layout, the most important first (`rocks`
/// keeps a share of them).
fn blocks(seed: u32, rocks: f32) -> Vec<Block> {
    let masses = [
        // The headland's tor, the ridge running into the sea, its sea end,
        // the back of the headland.
        Mass { at: [-1.1, -4.4], yaw: 18.0, grid: [3, 3], cell: [1.25, 1.05], h: 1.1, tier: true },
        Mass { at: [1.5, -3.75], yaw: 26.0, grid: [3, 2], cell: [1.05, 0.95], h: 0.85, tier: true },
        Mass { at: [3.4, -4.6], yaw: 34.0, grid: [2, 2], cell: [0.85, 0.75], h: 1.5, tier: false },
        Mass { at: [0.4, -5.5], yaw: 22.0, grid: [2, 1], cell: [1.0, 0.85], h: 0.95, tier: false },
        // The beach's front corner and the low blocks running out of it.
        Mass { at: [-4.7, 5.0], yaw: -12.0, grid: [3, 2], cell: [1.0, 0.9], h: 0.85, tier: true },
        Mass { at: [-2.55, 5.6], yaw: 6.0, grid: [3, 1], cell: [0.72, 0.62], h: 0.45, tier: false },
    ];
    let mut out = Vec::new();
    let mut id = 0u32;
    for (mi, m) in masses.iter().enumerate() {
        let (s, c) = m.yaw.to_radians().sin_cos();
        let (gx, gz) = (m.grid[0] as f32, m.grid[1] as f32);
        for j in 0..m.grid[1] {
            for i in 0..m.grid[0] {
                id += 1;
                let u = if gx > 1.0 { (i as f32 - (gx - 1.0) * 0.5) / ((gx - 1.0) * 0.5) } else { 0.0 };
                let w = if gz > 1.0 { (j as f32 - (gz - 1.0) * 0.5) / ((gz - 1.0) * 0.5) } else { 0.0 };
                let rr = ((u * u + w * w) * 0.5).sqrt();
                if rr > 0.1 && sh::rnd(seed, id, 70) < 0.08 + 0.2 * rr {
                    continue;
                }
                let lx = (i as f32 - (gx - 1.0) * 0.5) * m.cell[0] + sh::range(seed, id, 71, -0.06, 0.06);
                let lz = (j as f32 - (gz - 1.0) * 0.5) * m.cell[1] + sh::range(seed, id, 72, -0.06, 0.06);
                let at = [m.at[0] + lx * c - lz * s, m.at[1] + lx * s + lz * c];
                let h = m.h * (1.0 - 0.42 * rr * rr) * sh::range(seed, id, 73, 0.72, 1.1);
                let size = [m.cell[0] * sh::range(seed, id, 74, 0.9, 0.99), h, m.cell[1] * sh::range(seed, id, 75, 0.9, 0.99)];
                let yaw = (m.yaw + sh::range(seed, id, 76, -7.0, 7.0)).to_radians();
                let crack = size[0].max(size[2]) > 0.8 && sh::rnd(seed, id, 77) < 0.55;
                out.push(Block { at, size, yaw, sink: 0.28, rest: 0.0, crack });
                if m.tier && rr < 0.75 && sh::rnd(seed, id, 78) < 0.6 {
                    let k2 = sh::range(seed, id, 79, 0.72, 0.92);
                    let off = [sh::range(seed, id, 80, -0.12, 0.12), sh::range(seed, id, 81, -0.12, 0.12)];
                    out.push(Block {
                        at: [at[0] + off[0], at[1] + off[1]],
                        size: [size[0] * k2, h * sh::range(seed, id, 82, 0.45, 0.7), size[2] * k2],
                        yaw: yaw + sh::range(seed, id, 83, -0.1, 0.1),
                        sink: 0.05,
                        rest: h * 0.66,
                        crack: false,
                    });
                }
            }
        }
        // A loose block or two fallen off each of the first masses.
        if mi < 2 || mi == 4 {
            id += 1;
            let a = sh::rnd(seed, id, 84) * sh::TAU;
            let d = 0.5 * (m.grid[0] as f32 * m.cell[0]).max(m.grid[1] as f32 * m.cell[1]) + 0.45;
            let s0 = sh::range(seed, id, 85, 0.35, 0.6);
            out.push(Block { at: [m.at[0] + a.cos() * d, m.at[1] + a.sin() * d], size: [s0, s0 * 0.7, s0 * 0.85], yaw: a, sink: 0.25, rest: 0.0, crack: false });
        }
    }
    // Stones under the water (the bay's dark shapes), bedded into the sand.
    let sunk: [([f32; 2], [f32; 3]); 7] = [
        ([-0.6, 4.5], [1.3, 0.32, 0.8]),
        ([0.2, 4.9], [0.8, 0.26, 0.6]),
        ([-0.1, 2.8], [0.9, 0.26, 0.7]),
        ([2.8, 1.4], [1.2, 0.3, 0.9]),
        ([1.2, 3.0], [0.7, 0.22, 0.6]),
        ([4.0, 4.6], [1.1, 0.3, 0.8]),
        ([4.6, -1.0], [1.0, 0.28, 0.8]),
    ];
    for (i, (at, size)) in sunk.into_iter().enumerate() {
        out.push(Block { at, size, yaw: sh::rnd(seed, 900 + i as u32, 86) * sh::TAU, sink: 0.5, rest: 0.0, crack: false });
    }
    let keep = (out.len() as f32 * rocks.clamp(0.0, 1.0)).round() as usize;
    out.truncate(keep);
    out
}

/// A blade `len` long and `w` wide at its foot, rising at `a0` (radians
/// from upright) and bending over to `a1` at its tip, towards +x, in
/// `segs` pieces; both faces, its normals bent skywards.
fn arc_blade(w: f32, len: f32, a0: f32, a1: f32, segs: u32) -> Mesh {
    let mut b = geo::MeshBuilder::new();
    let n = segs.max(1);
    let step = len / n as f32;
    let mut p = [0.0f32, 0.0, 0.0];
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let a = a0 + (a1 - a0) * t.powf(1.5);
        let half = 0.5 * w * (1.0 - t * 0.9);
        let nrm = [-a.cos(), a.sin(), 0.0];
        b.vertex([p[0], p[1], p[2] - half], nrm);
        b.vertex([p[0], p[1], p[2] + half], nrm);
        p = [p[0] + a.sin() * step, p[1] + a.cos() * step, p[2]];
    }
    for i in 0..n {
        let a = i * 2;
        b.quad(a, a + 1, a + 3, a + 2);
    }
    let mut m = b.finish();
    sh::fix_winding(&mut m);
    sh::double_sided(&mut m);
    sh::foliage_normals(&mut m);
    m
}

/// The block's cut sides from the floor up to the ground, on the four
/// edges of the plot: rock in irregular, wavering strata, each layer's hard
/// top standing out over its worn foot (real ledges and recesses), rough at
/// two scales. Cut along smooth wandering lines into a dark base rock, a
/// buff band, a grey grit and the sand on top.
fn strata(g: &Ground, half: f32, n: u32, rows: u32, seed: u32) -> [Mesh; 4] {
    let k = g.k;
    let layer = |a: f32, y: f32| y / (0.15 * k) + 1.1 * noise::fbm(a / k * 0.3, y / k * 0.4, seed ^ 0x81, 2, 2.0, 0.5) + 0.3 * noise::fbm(a / k * 1.2, 0.5, seed ^ 0x82, 2, 2.0, 0.5);
    let mut b = geo::MeshBuilder::new();
    let edges: [([f32; 2], [f32; 2], [f32; 3]); 4] = [
        ([-half, -half], [1.0, 0.0], [0.0, 0.0, -1.0]),
        ([-half, half], [1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-half, -half], [0.0, 1.0], [-1.0, 0.0, 0.0]),
        ([half, -half], [0.0, 1.0], [1.0, 0.0, 0.0]),
    ];
    let row = rows + 1;
    for (o, d, nrm) in edges {
        let base = b.mesh.vertices.len() as u32;
        for i in 0..=n {
            let a = 2.0 * half * i as f32 / n as f32;
            let (x, z) = (o[0] + d[0] * a, o[1] + d[1] * a);
            let top = g.height(x, z);
            for r in 0..=rows {
                let y = top * r as f32 / rows as f32;
                let l = layer(a, y);
                let f = l - l.floor();
                let hard = noise::hash(l.floor() as i32 as u32 ^ seed) as f32 / u32::MAX as f32;
                let ledge = (f - 0.45) * (0.016 + 0.05 * hard);
                let rough = 0.02 * noise::fbm(a / k * 2.4, y / k * 4.0, seed ^ 0x83, 3, 2.0, 0.5);
                let off = (ledge + rough) * k * smooth(0.0, 0.05 * k, top - y);
                b.vertex([x + nrm[0] * off, y, z + nrm[2] * off], nrm);
            }
        }
        for i in 0..n {
            for r in 0..rows {
                let a = base + i * row + r;
                b.quad(a, a + row, a + row + 1, a + 1);
            }
        }
    }
    let mut m = b.finish();
    sh::fix_winding(&mut m);
    geo::smooth_normals(&mut m);
    // Where along its side a point is (m from the side's start).
    let along = move |v: &[f32; 6]| if (v[2].abs() - half).abs() < (v[0].abs() - half).abs() { v[0] + half } else { v[2] + half };
    let wave = move |v: &[f32; 6], f: f32, o: u32| noise::fbm(along(v) / k * f, 0.5, seed ^ o, 2, 2.0, 0.5);
    let (sand, rock) = cut(&m, |v| g.height(v[0].clamp(-half, half), v[2].clamp(-half, half)) - v[1] - (0.07 + 0.035 * wave(v, 0.8, 0x84)) * k);
    let (dark, upper) = cut(&rock, |v| v[1] - (0.3 + 0.12 * wave(v, 0.35, 0x85)) * k);
    // Above the dark base, grey-brown and buff beds by turns, each its own
    // thickness, wavering along the side.
    let (grey, buff) = cut(&upper, |v| (std::f32::consts::TAU * (v[1] / (0.34 * k) + 0.6 * wave(v, 0.3, 0x86))).sin() + 0.3 * wave(v, 1.1, 0x87));
    [dark, buff, grey, sand]
}

/// The water's cut faces: a wall from the ground up to `wl` along each side
/// of the plot where the ground is under it, facing out; split at
/// `wl - band` into its deep body and the lighter layer under the surface.
fn water_sides(g: &Ground, half: f32, cells: u32, wl: f32, band: f32) -> (Mesh, Mesh) {
    let mut deep = geo::MeshBuilder::new();
    let mut top = geo::MeshBuilder::new();
    let n = cells.max(1);
    let step = 2.0 * half / n as f32;
    let edges: [([f32; 2], [f32; 2], [f32; 3]); 4] = [
        ([-half, -half], [1.0, 0.0], [0.0, 0.0, -1.0]),
        ([-half, half], [1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-half, -half], [0.0, 1.0], [-1.0, 0.0, 0.0]),
        ([half, -half], [0.0, 1.0], [1.0, 0.0, 0.0]),
    ];
    let split = wl - band;
    for (o, d, nrm) in edges {
        for i in 0..n {
            let p0 = [o[0] + d[0] * step * i as f32, o[1] + d[1] * step * i as f32];
            let p1 = [p0[0] + d[0] * step, p0[1] + d[1] * step];
            let (h0, h1) = (g.height(p0[0], p0[1]), g.height(p1[0], p1[1]));
            if h0 >= wl && h1 >= wl {
                continue;
            }
            let wall = |b: &mut geo::MeshBuilder, lo: [f32; 2], hi: f32| {
                let (t0, t1) = (lo[0].min(hi), lo[1].min(hi));
                if t0 >= hi && t1 >= hi {
                    return;
                }
                let q = [b.vertex([p0[0], t0, p0[1]], nrm), b.vertex([p0[0], hi, p0[1]], nrm), b.vertex([p1[0], hi, p1[1]], nrm), b.vertex([p1[0], t1, p1[1]], nrm)];
                b.quad(q[0], q[1], q[2], q[3]);
            };
            wall(&mut deep, [h0, h1], split);
            wall(&mut top, [h0.max(split), h1.max(split)], wl);
        }
    }
    let fin = |b: geo::MeshBuilder| {
        let mut m = b.finish();
        sh::fix_winding(&mut m);
        sh::double_sided(&mut m);
        m
    };
    (fin(deep), fin(top))
}

/// The asset: the ground (dry, damp and wet sand, the sea bed), its cut
/// sides, the water's cut faces, the granite, the pebbles and shells, the
/// grass and low plants.
pub fn generate(v: &Values, seed: u32) -> Output {
    let g = Ground::new(v, seed);
    let (size, k, wl) = (v.f("size").max(1.0), g.k, g.wl);
    let half = size * 0.5;
    let density = v.density();
    let cells = ((80.0 + 80.0 * density.min(1.0)) as u32).clamp(48, 160);
    let ground = geo::height_grid([size; 2], cells, |x, z| g.height(x, z));

    // The sand, cut along smooth lines: the bed under the water, the wet
    // band at its edge, the damp band up to the swash's scalloped reach,
    // the dry beach and dunes above. No ripples: the dry sand is coarse
    // and gritty (the recipe's grit and clumps), the wet sand dark and
    // glossy.
    let sand = v.color("color");
    let (bed, rest) = cut(&ground, |p| p[1] - (wl - 0.004 * k));
    let (wet, rest) = cut(&rest, |p| p[1] - (wl + g.wet_top(p[0], p[2])));
    let (damp, dry) = cut(&rest, |p| p[1] - (wl + g.damp_top(p[0], p[2])));
    let dry_s = gritty(sp::GROUND_SAND_BEACH, sand).number(1, 0.05).number(2, 0.5).number(3, 0.0).number(6, 0.3).seed(seed).scale(0.5);
    let damp_s = gritty(sp::GROUND_SAND_BEACH, mul(sand, [0.74, 0.72, 0.69])).number(1, 0.035).number(2, 0.45).number(3, 0.0).number(6, 0.3).seed(seed ^ 0x51).scale(0.5);
    let wet_s = sand_tint(sp::GROUND_SAND_WET, WET).number(0, 0.7).number(2, 0.25).number(3, 0.0).seed(seed ^ 0x3c).scale(0.5);
    let bed_s = gritty(sp::GROUND_SAND_BEACH, mul(sand, [0.84, 0.84, 0.8])).number(1, 0.035).number(2, 0.4).number(3, 0.04).number(6, 0.3).seed(seed ^ 0x7d).scale(0.5);
    let mut parts: Vec<Part> = Vec::new();
    for (m, s) in [(bed, bed_s), (wet, wet_s), (damp, damp_s), (dry, dry_s)] {
        if !m.indices.is_empty() {
            parts.push(Part::surfaced(m, s));
        }
    }
    if v.f("edge") > 0.0 {
        // The cut sides in weathered strata.
        let tints: [evani_procedural::Surface; 4] = [
            sp::ROCK_SANDSTONE_BUFF.tint([0.05, 0.046, 0.04]).seed(seed).scale(0.4),
            sp::ROCK_SANDSTONE_BUFF.tint([0.115, 0.095, 0.07]).seed(seed ^ 0x2).scale(0.4),
            sp::ROCK_SANDSTONE_BUFF.tint([0.075, 0.07, 0.062]).seed(seed ^ 0x3).scale(0.4),
            gritty(sp::GROUND_SAND_BEACH, mul(sand, [0.72, 0.7, 0.66])).number(3, 0.0).seed(seed ^ 0x4).scale(0.5),
        ];
        let layers = strata(&g, half, (cells / 2).max(24), 40, seed);
        for (m, s) in layers.into_iter().zip(tints) {
            if !m.indices.is_empty() {
                parts.push(Part::surfaced(m, s));
            }
        }
    }
    // The water's cut faces: solid deep teal, lighter under the surface.
    let (deep_face, top_face) = water_sides(&g, half, cells, wl, 0.08 * k);
    let c = v.color("water");
    for (m, rgb) in [(deep_face, c), (top_face, [c[0] * 2.2 + 0.01, c[1] * 1.9, c[2] * 1.8])] {
        if !m.indices.is_empty() {
            parts.push(Part::new(m, rgb).material("glass"));
        }
    }

    // Granite: jointed masses (the gaps between blocks are the joints),
    // ochre-weathered over its tops and sunlit faces, dark and wet at the
    // waterline.
    let mut rock = Mesh::default();
    let big_sub = if density >= 0.75 { 4 } else { 3 };
    for (i, b) in blocks(seed, v.f("rocks")).iter().enumerate() {
        let i = i as u32;
        let s = [b.size[0] * k, b.size[1] * k, b.size[2] * k];
        let sub = if b.sink >= 0.5 || s[0].max(s[2]) <= 0.45 * k {
            (big_sub - 1).min(2)
        } else if s[0].max(s[2]) > 1.15 * k {
            big_sub
        } else {
            big_sub - 1
        };
        let mut m = granite_block(s, seed.wrapping_add(i * 977), sub, b.crack);
        sh::rotate(&mut m, [0.0, 1.0, 0.0], b.yaw);
        let (x, z) = (b.at[0] * k, b.at[1] * k);
        // Keep the block inside the plot (its far side at most at the edge).
        let r = 0.5 * s[0].max(s[2]);
        let (x, z) = (x.clamp(-half + r * 0.6, half - r * 0.6), z.clamp(-half + r * 0.6, half - r * 0.6));
        let mut y = g.height(x, z) - s[1] * b.sink + b.rest * k;
        if b.sink >= 0.5 {
            y = y.min(wl - 0.2 * k - s[1]);
        }
        geo::transform(&mut m, [1.0; 3], [x, y, z]);
        geo::merge(&mut rock, &m);
    }
    if !rock.indices.is_empty() {
        // Cut along smooth lines: the wet foot, then crusty lichen patches
        // (more on tops and the faces turned to the sun).
        let (wet_rock, dry_rock) = cut(&rock, |v| v[1] - wl - (0.1 + 0.03 * noise::fbm(v[0] / k * 2.0, v[2] / k * 2.0, seed ^ 0x6f, 2, 2.0, 0.5)) * k);
        let (lichen, bare) = cut(&dry_rock, |v| {
            let p = [v[0] / k * 1.6, v[1] / k * 1.6, v[2] / k * 1.6];
            0.46 - (0.7 * noise::fbm3(p, seed ^ 0x6d, 3) + 0.35 * noise::fbm3([p[0] * 3.5, p[1] * 3.5, p[2] * 3.5], seed ^ 0x6e, 2) + 0.35 * v[4] - 0.12 * v[5])
        });
        let kinds = [bare, lichen, wet_rock];
        let looks = [
            sp::ROCK_GRANITE_GREY.tint(GRANITE).seed(seed ^ 0x6a).scale(1.6),
            sp::ROCK_GRANITE_GREY.tint(OCHRE).seed(seed ^ 0x6b).scale(1.6),
            sp::ROCK_GRANITE_GREY.tint(WET_ROCK).number(0, 0.35).seed(seed ^ 0x6c).scale(1.6),
        ];
        for (m, s) in kinds.into_iter().zip(looks) {
            if !m.indices.is_empty() {
                parts.push(Part::surfaced(m, s));
            }
        }
    }

    // Pebbles and shells: an irregular strandline (wandering up and down
    // the beach, gathered in drifts), the dune foot, a few in the wet and
    // under the shallows; mostly small with a few big ones; grey granite,
    // dark basalt, buff and rusty sandstone, white quartz, slate.
    let tints: [[f32; 3]; 6] = [[0.25, 0.24, 0.225], [0.05, 0.05, 0.048], [0.36, 0.28, 0.18], [0.26, 0.14, 0.07], [0.52, 0.5, 0.46], [0.11, 0.12, 0.13]];
    let weights = [0.3, 0.45, 0.65, 0.75, 0.87, 1.0];
    let mut peb: [Mesh; 6] = Default::default();
    let mut shells: [Mesh; 2] = Default::default();
    let pebbles = v.f("pebbles").clamp(0.0, 1.0);
    let n = (420.0 * pebbles) as u32;
    let n_shell = (170.0 * pebbles) as u32;
    let (mut placed, mut shelled) = (0u32, 0u32);
    let mut i = 0u32;
    while (placed < n || shelled < n_shell) && i < (n + n_shell) * 14 {
        i += 1;
        let (x, z) = (sh::range(seed, i, 41, -half * 0.98, half * 0.98), sh::range(seed, i, 42, -half * 0.98, half * 0.98));
        let (s, _) = g.plan(x, z);
        let u = -s;
        let u0 = 1.35 + 0.4 * g.n(x, z, 0.5, 61);
        let strand = (-((u - u0) / 0.3).powi(2)).exp();
        let drift = smooth(-0.25, 0.4, g.n(x, z, 1.6, 62));
        // The dune foot, a little higher up.
        let foot = 0.3 * (-((u - 2.3) / 0.4).powi(2)).exp();
        let shell = placed >= n || (shelled < n_shell && sh::rnd(seed, i, 40) < 0.3);
        let want = if s > 0.9 {
            0.0
        } else if s > -0.2 {
            if shell { 0.0 } else { 0.035 }
        } else if shell {
            strand * (0.3 + 0.7 * drift)
        } else {
            (strand * (0.2 + 1.0 * drift) + foot * drift + 0.025).min(1.0)
        };
        if sh::rnd(seed, i, 43) > want {
            continue;
        }
        let y = g.height(x, z);
        if shell {
            let r = k * sh::range(seed, i, 44, 0.006, 0.016);
            let mut m = sh::blob([r, r * 0.3, r * 0.8], [0.0, 0.0, 0.0], 0.15, seed.wrapping_add(i * 37), 0);
            sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 48) * sh::TAU);
            geo::transform(&mut m, [1.0; 3], [x, y + r * 0.1, z]);
            geo::merge(&mut shells[usize::from(sh::rnd(seed, i, 49) < 0.3)], &m);
            shelled += 1;
            continue;
        }
        let q = sh::rnd(seed, i, 44);
        let r = k * (0.008 + 0.032 * q * q + if sh::rnd(seed, i, 45) < 0.06 { 0.035 } else { 0.0 });
        let sub = if r > 0.022 * k { 1 } else { 0 };
        let mut m = sh::blob([r, r * sh::range(seed, i, 46, 0.4, 0.7), r * sh::range(seed, i, 47, 0.6, 1.0)], [0.0, 0.0, 0.0], 0.2, seed.wrapping_add(i * 31), sub);
        sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 48) * sh::TAU);
        geo::transform(&mut m, [1.0; 3], [x, y - r * 0.15, z]);
        let pick = sh::rnd(seed, i, 49);
        let c = weights.iter().position(|w| pick < *w).unwrap_or(0);
        geo::merge(&mut peb[c], &m);
        placed += 1;
    }
    for (j, (m, c)) in peb.into_iter().zip(tints).enumerate() {
        if !m.indices.is_empty() {
            parts.push(Part::surfaced(m, sp::ROCK_GRANITE_GREY.tint(c).seed(seed ^ (0x99 + j as u32)).scale(0.3)));
        }
    }
    for (m, c) in shells.into_iter().zip([[0.6, 0.55, 0.46], [0.55, 0.38, 0.33]]) {
        if !m.indices.is_empty() {
            parts.push(Part::surfaced(m, sp::ROCK_GRANITE_GREY.tint(c).number(0, 0.4).seed(seed ^ 0x9f).scale(0.2)));
        }
    }

    // Marram grass: tufts of long blades bent by the wind (most lean the
    // same way, arching over; dead straw ones further), green, olive,
    // straw and pale in every tuft, thinning down the dune face and broken
    // by bare sand; low creeping mats at the dune foot and between tufts.
    // Drawn in a catalogue surface (fine mottling along the blades) so no
    // look recolours them.
    let colours: [[f32; 3]; 6] = [[0.13, 0.12, 0.035], [0.07, 0.095, 0.022], [0.23, 0.18, 0.08], [0.32, 0.27, 0.16], [0.035, 0.065, 0.016], [0.09, 0.07, 0.03]];
    let mut grass: [Mesh; 6] = Default::default();
    let gamount = v.f("grass").clamp(0.0, 1.0);
    let bdens = density.clamp(0.4, 1.0);
    let wind = 205f32.to_radians();
    let tufts = (160.0 * gamount) as u32;
    let mut made = 0u32;
    let mut i = 0u32;
    while made < tufts && i < tufts * 40 {
        i += 1;
        let (x, z) = (sh::range(seed, i, 51, -half * 0.97, half * 0.97), sh::range(seed, i, 52, -half * 0.97, half * 0.97));
        let (s, _) = g.plan(x, z);
        if s > -1.3 {
            continue;
        }
        let cover = smooth(-0.3, 0.3, g.n(x, z, 0.55, 0x71));
        let base = 0.95 * smooth(1.3, 2.6, -s);
        if sh::rnd(seed, i, 53) > base * (0.12 + 0.88 * cover) {
            continue;
        }
        let h = k * sh::range(seed, i, 55, 0.4, 0.95) * (0.65 + 0.35 * cover);
        let blades = ((30.0 + 42.0 * sh::rnd(seed, i, 56) * (0.5 + 0.5 * cover)) * bdens) as u32;
        let y = g.height(x, z);
        for b in 0..blades {
            let id = i * 96 + b;
            let pick = sh::rnd(seed, id, 61);
            let c = if pick < 0.36 { 0 } else if pick < 0.52 { 1 } else if pick < 0.84 { 2 } else { 3 };
            let dead = c >= 2;
            let spread = if sh::rnd(seed, id, 62) < 0.25 { 2.6 } else { 1.1 };
            let yaw = wind + (sh::rnd(seed, id, 57) - 0.5) * spread;
            let len = h * sh::range(seed, id, 58, 0.55, 1.2) * if dead { 1.15 } else { 1.0 };
            let a0 = sh::range(seed, id, 59, 0.04, 0.38) + if dead { 0.2 } else { 0.0 };
            let a1 = a0 + sh::range(seed, id, 60, 0.25, 1.05) + if dead { sh::range(seed, id, 63, 0.3, 0.9) } else { 0.0 };
            let mut m = arc_blade(0.016 * k * sh::range(seed, id, 64, 0.8, 1.3), len, a0, a1, 3);
            sh::rotate(&mut m, [0.0, 1.0, 0.0], yaw);
            let rr = h * 0.18 * sh::rnd(seed, id, 65).sqrt();
            let ra = sh::rnd(seed, id, 66) * sh::TAU;
            geo::transform(&mut m, [1.0; 3], [x + ra.cos() * rr, y - 0.01 * k, z + ra.sin() * rr]);
            geo::merge(&mut grass[c], &m);
        }
        made += 1;
    }
    // Low mats: creeping plants in patches of small spreading leaves.
    let mats = (50.0 * gamount) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < mats && i < mats * 40 {
        i += 1;
        let (x, z) = (sh::range(seed, i, 91, -half * 0.95, half * 0.95), sh::range(seed, i, 92, -half * 0.95, half * 0.95));
        let (s, _) = g.plan(x, z);
        if !(-3.2..=-1.4).contains(&s) {
            continue;
        }
        if sh::rnd(seed, i, 93) > 0.55 * smooth(-0.4, 0.2, g.n(x, z, 0.9, 0x72)) {
            continue;
        }
        let rad = k * sh::range(seed, i, 94, 0.15, 0.45);
        let leaves = ((30.0 + 90.0 * rad / k) * bdens) as u32;
        let c = if sh::rnd(seed, i, 95) < 0.7 { 4 } else { 5 };
        for b in 0..leaves {
            let id = i * 160 + b;
            let mut m = arc_blade(0.022 * k, k * sh::range(seed, id, 96, 0.04, 0.1), sh::range(seed, id, 97, 0.7, 1.15), sh::range(seed, id, 98, 1.25, 1.65), 1);
            sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, id, 99) * sh::TAU);
            let rr = rad * sh::rnd(seed, id, 100).sqrt();
            let ra = sh::rnd(seed, id, 101) * sh::TAU;
            let (px, pz) = (x + ra.cos() * rr, z + ra.sin() * rr);
            geo::transform(&mut m, [1.0; 3], [px, g.height(px, pz) - 0.004 * k, pz]);
            geo::merge(&mut grass[c], &m);
        }
        made += 1;
    }
    for (j, (m, c)) in grass.into_iter().zip(colours).enumerate() {
        if !m.indices.is_empty() {
            parts.push(Part::surfaced(m, sp::GROUND_SOIL_PACKED.tint(c).seed(seed ^ (0x4400 + j as u32)).scale(0.12)));
        }
    }
    Output::Parts(parts)
}

/// The water's look for the nature pass (`material.nature.water.*`): clear
/// coastal water, the sand showing through the shallows, teal further out,
/// a dark blue-teal in the deep that hides the bed; foam as lace with swash
/// fronts running in, the sun's caustics on the shallow bed, a swell from
/// the sea with fine ripples across it and broken crests. A host applies it
/// once with [`apply_water_look`].
pub const WATER_LOOK: [(&str, Look); 24] = [
    ("material.nature.water.deep_color", Look::C([0.36, 0.66, 0.72])),
    ("material.nature.water.clarity", Look::F(0.75)),
    ("material.nature.water.scatter_color", Look::C([0.012, 0.04, 0.065])),
    ("material.nature.water.foam_width", Look::F(0.22)),
    ("material.nature.water.foam", Look::F(1.5)),
    ("material.nature.water.wave_scale", Look::F(1.1)),
    ("material.nature.water.wave_speed", Look::F(0.6)),
    ("material.nature.water.wave_strength", Look::F(0.16)),
    ("material.nature.water.glint", Look::F(1.2)),
    ("material.nature.water.refraction", Look::F(1.0)),
    ("material.nature.water.reflection", Look::F(0.22)),
    ("material.nature.water.roughness", Look::F(0.07)),
    ("material.nature.water.swell", Look::F(0.45)),
    ("material.nature.water.swell_direction", Look::F(150.0)),
    ("material.nature.water.choppiness", Look::F(0.6)),
    ("material.nature.water.whitecap_amount", Look::F(0.12)),
    ("material.nature.water.whitecap_threshold", Look::F(0.8)),
    ("material.nature.water.coast_lace", Look::F(0.8)),
    ("material.nature.water.coast_swash", Look::F(1.6)),
    ("material.nature.water.coast_swash_spacing", Look::F(0.06)),
    ("material.nature.water.coast_caustics", Look::F(0.8)),
    ("material.nature.water.coast_caustic_scale", Look::F(3.2)),
    ("material.nature.water.coast_ripples", Look::F(0.55)),
    ("material.nature.water.coast_murk", Look::F(0.9)),
];

/// A water-look value.
#[derive(Debug, Clone, Copy)]
pub enum Look {
    F(f32),
    C([f32; 3]),
}

/// Set [`WATER_LOOK`] in the param registry (names the build lacks are
/// skipped and returned).
pub fn apply_water_look(reg: &mut evani_core::params::ParamRegistry) -> Vec<&'static str> {
    use evani_core::params::ParamValue;
    let mut missing = Vec::new();
    for (name, v) in WATER_LOOK {
        let val = match v {
            Look::F(x) => ParamValue::F32(x),
            Look::C(c) => ParamValue::Color(c),
        };
        match reg.id(name) {
            Some(id) => {
                let _ = reg.set(id, val);
            }
            None => missing.push(name),
        }
    }
    missing
}

/// The asset's water for the nature pass: a water rectangle over the whole
/// plot at its water level (the sand and rocks hide it where they stand
/// above it), moved like the placement (`at`, `yaw`, `scale`). `None` when
/// the asset isn't in the registry.
#[cfg(feature = "host-nature")]
pub fn water(registry: &evani_procedural::Registry, reg: &evani_core::params::ParamRegistry, at: [f32; 3], yaw: f32, scale: f32) -> Option<evani_material_nature::Surface> {
    use evani_material_nature::{Kind, Surface as W};
    let (v, _) = registry.values(reg, "water-shorelines")?;
    let half = v.f("size").max(1.0) * 0.5;
    Some(W::new(Kind::WaterRect, [0.0, water_level(&v), 0.0], [half, 0.0, half]).placed(at, yaw, scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_procedural::Output;

    fn values() -> Values {
        Values::defaults(&PARAMS)
    }

    fn tris(o: &Output) -> usize {
        match o {
            Output::Parts(p) => p.iter().map(|x| x.mesh.indices.len() / 3).sum(),
            Output::Mesh(m) => m.indices.len() / 3,
            Output::Module => 0,
        }
    }

    /// It builds every part (four sands, four strata, two water faces,
    /// three granites, pebbles, shells, grass and mats), within a budget at
    /// High and lighter on Low, the same each run.
    #[test]
    fn shoreline_builds_light_and_repeatable() {
        let high = generate(&values().with_density(1.0), 3);
        let low = generate(&values().with_density(evani_procedural::detail::Density::for_tier("low")), 3);
        let Output::Parts(parts) = &high else { panic!("parts") };
        assert!(parts.len() >= 20, "{} parts", parts.len());
        assert!(parts.iter().filter(|p| p.surface.is_some()).count() >= 18);
        assert!(parts.iter().any(|p| p.material == "glass" && p.surface.is_none()));
        let (h, l) = (tris(&high), tris(&low));
        println!("shoreline: {h} triangles at High, {l} at Low");
        let each: Vec<usize> = parts.iter().map(|p| p.mesh.indices.len() / 3).collect();
        println!("per part: {each:?}");
        assert!(h < 300_000 && l <= h, "{h} / {l}");
        assert_eq!(h, tris(&generate(&values().with_density(1.0), 3)));
    }

    /// The bay is under the water level, the beach and dunes over it, the
    /// dune crest well over the beach.
    #[test]
    fn sea_under_the_water_and_dunes_over_it() {
        let v = values();
        let g = Ground::new(&v, 1);
        let wl = water_level(&v);
        assert!(g.height(4.5, 2.0) < wl - 0.2, "sea {}", g.height(4.5, 2.0));
        assert!(g.height(-4.0, 2.5) > wl, "beach {}", g.height(-4.0, 2.5));
        assert!(g.height(-5.8, 1.2) > wl + 0.5, "crest {}", g.height(-5.8, 1.2));
        assert!((side(&WATERLINE, 1.0, [-2.0, 0.7])).abs() < 1e-3);
    }

    /// The cut keeps every bit of the surface (the two sides' areas add up
    /// to the whole) and puts each side where its function says.
    #[test]
    fn the_cut_splits_along_the_line() {
        let m = geo::height_grid([2.0, 2.0], 16, |x, z| 0.3 * x + 0.1 * z);
        let area = |m: &Mesh| -> f32 {
            m.indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let p = |i: u32| {
                        let v = m.vertices[i as usize];
                        [v[0], v[1], v[2]]
                    };
                    let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                    let (e, f) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                    let x = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
                    0.5 * (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt()
                })
                .sum()
        };
        let line = |p: &[f32; 6]| p[0] - 0.13 + 0.2 * p[2] * p[2];
        let (lo, hi) = cut(&m, line);
        assert!((area(&lo) + area(&hi) - area(&m)).abs() < 1e-3 * area(&m));
        assert!(lo.vertices.iter().all(|v| line(v) < 2e-3));
        assert!(hi.vertices.iter().all(|v| line(v) > -2e-3));
    }

    /// The water rectangle covers the plot at the water level, moved like
    /// the placement.
    #[test]
    fn water_rect_at_the_water_level() {
        let reg_lib = crate::registry();
        let mut reg = evani_core::params::ParamRegistry::new();
        reg_lib.register_params(&mut reg).expect("params");
        let w = water(&reg_lib, &reg, [10.0, 1.0, 0.0], 0.0, 2.0).expect("registered");
        assert_eq!(w.kind, evani_material_nature::Kind::WaterRect);
        assert!((w.centre[1] - (1.0 + 2.0 * water_level(&values()))).abs() < 1e-4);
        assert!((w.half[0] - 12.0).abs() < 1e-4 && (w.centre[0] - 10.0).abs() < 1e-4);
    }
}
