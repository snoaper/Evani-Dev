//! Caves (terrain-caves; the Asset Studio caves session, column 29,
//! 2026-11-08): a photoreal limestone cave passage, as a player walks in
//! from its mouth with the daylight behind them.
//!
//! The passage runs in from its mouth at z = 0 towards -z, `size` m deep:
//! an arched section (near-vertical walls, a rounded roof), wide and high at
//! the mouth, a little narrower and lower further in (`narrow`), turning
//! away towards its far end (`bend`) so the far end is out of the
//! daylight's reach, and closed there. The rock is real geometry, as bedded
//! limestone breaks: metre-scale bulges and recesses; beds some half a metre
//! thick, dipping gently, each standing out or set back by its own amount
//! with a dark parting groove between them (the ledges); each bed broken
//! across by joints into blocks, each block's face its own plane and a crack
//! along its edges; then a fine roughness. The floor is packed earth,
//! banked up with gravel and stones against the walls' feet; on it lie
//! fallen blocks, cobbles and sharp chips as real geometry (the jagged rock
//! formation's broken stones, `crate::jagged_rocks::blade`), heaped along
//! the walls (`rubble`). Near the mouth, where daylight reaches, green algae
//! and moss stain the lower walls (`moss`). Round the mouth a rock face
//! stands across the passage, so from outside the mouth reads as a hole in
//! a cliff.
//!
//! The paint is the stone recipe's variant 11 (as the jagged rock
//! formation's and the scree's stones) in a warm buff limestone, a cooler
//! grey limestone and an algae-stained one, and the ground recipe's packed
//! soil and scree gravel for the floor: a game that lists `stone` and
//! `ground` draws what the library shows.
//!
//! Light: inside, the only light is the daylight coming in at the mouth.
//! [`entrance_lights`] gives the point lights a game places at the mouth for
//! it (a row across the opening, falling off with depth as daylight does);
//! with the sky's own ambient turned down inside, the passage darkens as it
//! goes in. Detail follows the player (`focus`, m in from the mouth): round
//! it the rock's mesh is finest.

use crate::jagged_rocks::blade;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};
use evani_procedural::{Output, ParamSpec as P, Part, Values, detail, noise};

/// The library colour param's default (the old basic cave's): at it the
/// rock keeps its own limestone colours.
pub const COLOR: [f32; 3] = [0.26, 0.24, 0.21];

pub const PARAMS: [P; 9] = [
    P::num("size", 10.0, 2.0, 80.0).unit("m").describe("Depth of the cave (m)"),
    P::num("opening", 2.5, 0.5, 20.0).unit("m").describe("Height of the mouth (m)"),
    P::color("color", COLOR).describe("Rock colour (at its default the rock keeps its own limestone)"),
    P::num("width", 0.0, 0.0, 40.0).unit("m").describe("Width of the passage at the mouth (m; 0: 1.5 times the mouth's height)"),
    P::num("bend", 20.0, -90.0, 90.0).unit("deg").describe("How far the passage turns towards its far end (degrees; positive: to the right as you walk in)"),
    P::num("narrow", 0.3, 0.0, 0.7).describe("How much narrower and lower the passage gets towards its far end"),
    P::num("rubble", 1.0, 0.0, 2.0).describe("Fallen blocks, cobbles and chips on the floor, heaped along the walls' feet (0: none)"),
    P::num("moss", 0.5, 0.0, 1.0).describe("Green algae and moss on the lower walls near the mouth, where daylight reaches (0: none)"),
    P::num("focus", 3.0, 0.0, 80.0).unit("m").describe("Where a player stands (m in from the mouth): round it the rock's mesh is finest"),
];

/// How far the walls' feet go down under the floor (m), so the floor meets
/// rock wherever it ends. The asset stands on y = 0 at the walls' feet; the
/// floor lies this high.
pub const FOOT: f32 = 0.6;

/// The passage's layout, from its params: where its middle runs, how wide
/// and high it is, and the floor's height.
#[derive(Debug, Clone, Copy)]
pub struct Cave {
    /// Depth (m), half width and height at the mouth (m).
    pub len: f32,
    pub w: f32,
    pub h: f32,
    bend: f32,
    narrow: f32,
    seed: u32,
}

impl Cave {
    pub fn new(v: &Values, seed: u32) -> Self {
        let h = v.f("opening").max(0.5);
        let w = 0.5 * if v.f("width") > 0.0 { v.f("width") } else { h * 1.5 };
        Self { len: v.f("size").max(2.0), w, h, bend: v.f("bend").to_radians(), narrow: v.f("narrow").clamp(0.0, 0.9), seed }
    }

    /// How far in, 0 at the mouth to 1 at the far end.
    fn t(&self, z: f32) -> f32 {
        (-z / self.len).clamp(0.0, 1.0)
    }

    /// The far end's closing: 1 over most of the passage, down to 0.15 at
    /// the very end.
    fn close(&self, z: f32) -> f32 {
        1.0 - 0.85 * noise::smoothstep(0.88, 1.0, self.t(z))
    }

    /// The passage's middle line: x at depth `z` (m).
    pub fn centre(&self, z: f32) -> f32 {
        let t = self.t(z);
        let s = noise::smoothstep(0.3, 1.0, t);
        // The bend: the heading turns by `bend` over the far part, so the
        // offset grows as the square of the way into it; a gentle wander.
        self.len * 0.6 * self.bend.sin() * s * s + 0.18 * self.w * noise::fbm2(t * 3.0, 0.5, self.seed ^ 0x0c4e, 2, 2.0, 0.5)
    }

    /// Half the passage's width at depth `z` (m).
    pub fn half_width(&self, z: f32) -> f32 {
        let t = self.t(z);
        self.w * (1.0 - self.narrow * noise::smoothstep(0.05, 0.75, t)) * (1.0 + 0.2 * noise::fbm2(z / 6.0, 1.5, self.seed ^ 0x51d3, 2, 2.0, 0.5)) * self.close(z)
    }

    /// The roof's height over the floor at depth `z` (m).
    pub fn height(&self, z: f32) -> f32 {
        let t = self.t(z);
        self.h * (1.0 - 0.6 * self.narrow * noise::smoothstep(0.05, 0.75, t)) * (1.0 + 0.14 * noise::fbm2(z / 5.0, 2.5, self.seed ^ 0x7e11, 2, 2.0, 0.5)) * self.close(z)
    }

    /// The roof's height at depth `z` in the asset's frame (m).
    pub fn roof(&self, z: f32) -> f32 {
        self.height(z) + FOOT
    }

    /// The floor's height at (x, z): packed earth, gently uneven, banked up
    /// against the walls' feet with fallen gravel.
    pub fn floor(&self, x: f32, z: f32) -> f32 {
        let s = self.seed;
        let hw = self.half_width(z).max(0.2);
        let e = ((x - self.centre(z)).abs() / hw).min(1.4);
        let lumps = 0.5 + 0.5 * noise::fbm2(x / 0.45, z / 0.45, s ^ 0xba7d, 3, 2.0, 0.55);
        let bank = (0.22 + 0.2 * lumps) * noise::smoothstep(0.55, 1.05, e + 0.15 * noise::fbm2(x / 0.8, z / 0.8, s ^ 0xba7c, 2, 2.0, 0.5));
        FOOT + 0.07 * noise::fbm2(x / 1.6, z / 1.6, s ^ 0xf100, 3, 2.0, 0.5) + 0.025 * noise::fbm2(x / 0.35, z / 0.35, s ^ 0xf101, 2, 2.0, 0.5) + bank
    }

    /// The floor's normal at (x, z).
    fn floor_normal(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 0.05;
        let dx = (self.floor(x + e, z) - self.floor(x - e, z)) / (2.0 * e);
        let dz = (self.floor(x, z + e) - self.floor(x, z - e)) / (2.0 * e);
        geo::normalize([-dx, 1.0, -dz])
    }
}

/// One light a game places for the daylight at the mouth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaveLight {
    pub position: [f32; 3],
    /// Linear RGB.
    pub color: [f32; 3],
    /// Metres: nothing is lit beyond it.
    pub range: f32,
    pub intensity: f32,
}

/// The daylight coming in at the mouth, as point lights (in the asset's own
/// frame), together `brightness` strong (1: an overcast-bright day): six for
/// the sky seen through the opening, across it and up by its top and a
/// little outside, so the light comes in from above and behind as a sky's
/// does (the floor and the walls take it, the roof little); and two just
/// inside the mouth, high, for the opening's light on the floor further in
/// and what the sunlit ground outside throws in. They fall off with depth as the opening's
/// light does and end some three and a half mouth sizes in; the far end,
/// round the bend, gets none. A game draws
/// them every frame (evani-render `point-lights`, eight at most on the plain
/// path) and turns the sky's ambient down while the player is inside.
pub fn entrance_lights(v: &Values, seed: u32, brightness: f32) -> Vec<CaveLight> {
    let c = Cave::new(v, seed);
    let (hw, h, x0) = (c.half_width(0.0), c.height(0.0), c.centre(0.0));
    // How far the opening's light gets in: some three and a half times the
    // mouth's size (the passage's walls hide more of the opening the deeper
    // in you are, so its light dies off faster than with distance alone).
    let range = ((c.h + c.w) * 3.5).max(8.0);
    let sky = brightness * 0.5 * c.w * c.h;
    let mut out = Vec::with_capacity(8);
    for (fx, fy, z) in [(-0.6f32, 0.62f32, 1.2f32), (0.0, 0.62, 1.2), (0.6, 0.62, 1.2), (-0.45, 1.0, 2.0), (0.0, 1.0, 2.0), (0.45, 1.0, 2.0)] {
        out.push(CaveLight { position: [x0 + fx * hw, FOOT + h * fy, z], color: [1.0, 0.98, 0.95], range, intensity: sky });
    }
    // Just inside the mouth, high: the sky seen through the opening from the
    // floor further in, and the light the sunlit ground outside throws in.
    let z = -(c.h * 0.35).min(c.len * 0.2);
    for fx in [-0.35f32, 0.35] {
        out.push(CaveLight { position: [x0 + fx * hw, FOOT + c.height(z) * 0.72, z], color: [1.0, 0.96, 0.9], range: range * 0.85, intensity: sky * 0.7 });
    }
    out
}

/// sRGB bytes into colour `k` of a block (the scree's helper).
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A stone in the recipe's variant 11 (the jagged rock formation's and the
/// scree's helper): base `a`, dark grains of `b`, pale grains, cloud and
/// veins of `c`.
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
        .number(9, 0.0)
}

/// The cave's rocks: a warm buff limestone, a cooler grey limestone, an
/// algae-stained one, and the pale fresh-broken stone of the fallen blocks.
fn rocks() -> [Surface; 4] {
    [
        stone([162, 154, 134], [106, 98, 84], [204, 198, 182], [0.06, 0.07], 0.85, [0.0, 5.0], 0.85, 0.75),
        stone([154, 150, 138], [100, 96, 88], [198, 194, 182], [0.06, 0.06], 0.8, [0.0, 6.0], 0.85, 0.7),
        stone([136, 138, 108], [88, 92, 66], [172, 172, 140], [0.07, 0.05], 0.85, [0.0, 5.0], 0.9, 0.8),
        stone([170, 162, 144], [114, 106, 92], [210, 204, 190], [0.05, 0.07], 0.75, [0.0, 8.0], 0.8, 0.55),
    ]
}

/// A Voronoi cell of the point (x, y): the nearest and second nearest
/// distances, the nearest cell's hash in [0, 1) and its centre.
fn cell(x: f32, y: f32, seed: u32) -> (f32, f32, f32, [f32; 2]) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id, mut at) = (f32::MAX, f32::MAX, 0.0, [0.0; 2]);
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (xi + i, yi + j);
            let px = cx as f32 + noise::hash2(cx, cy, seed);
            let py = cy as f32 + noise::hash2(cx, cy, seed ^ 0x5bd1_e995);
            let d = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = noise::hash2(cx, cy, seed ^ 0x1b87_3593);
                at = [px, py];
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, id, at)
}

/// How far the rock stands out into the passage (m, positive inwards) at
/// world point `p`, `s` m round the arch from the right foot: big masses,
/// ledges (a terraced noise stretched along the beds, so each ledge steps
/// out over a short steep riser, never a cliff of one mesh step), the beds'
/// dark partings, sparse joints cracking across them, and a fine roughness.
fn relief(p: [f32; 3], s: f32, seed: u32) -> f32 {
    // Masses: the walls bulge and recess over 3 to 5 m.
    let mass = 0.55 * noise::fbm3p([p[0] / 3.6, p[1] / 2.8, p[2] / 3.6], seed ^ 0x1a77, 3, 0.45);
    // Ledges: a noise stretched along the beds, cut into terraces.
    let n = noise::fbm3p([p[0] / 2.2, p[1] / 0.85, p[2] / 2.2], seed ^ 0x1ed6, 2, 0.45);
    let k = 0.3;
    let t = n / k;
    let f = t - t.floor();
    let ledge = 0.55 * k * (t.floor() + noise::smoothstep(0.62, 1.0, f));
    // The beds' partings: a dark groove every 0.6 to 1.1 m up the wall,
    // dipping gently along the passage.
    let dip = p[1] + 0.05 * p[2] + 0.25 * noise::fbm3p([p[0] / 3.0, p[1] / 3.0, p[2] / 3.0], seed ^ 0x0bed, 2, 0.5);
    let bed_th = 0.85 + 0.25 * noise::fbm2(p[2] / 7.0, 0.3, seed ^ 0x7ab, 2, 2.0, 0.5);
    let bu = dip / bed_th;
    let part = (bu - bu.floor()).min(bu.ceil() - bu) * bed_th;
    let open = 0.5 + 0.5 * noise::fbm3p([p[0] / 1.5, p[1] / 1.5, p[2] / 1.5], seed ^ 0x9a17, 2, 0.5);
    let parting = 0.08 * open * (1.0 - noise::smoothstep(0.0, 0.11, part));
    // Joints: sparse cracks across the beds, about 2 m apart.
    let js = 2.2;
    let (f1, f2, _, _) = cell(s / js, p[2] / js, seed ^ 0x701d);
    let crack = 0.07 * open * (1.0 - noise::smoothstep(0.0, 0.05, (f2 - f1) * js));
    // A fine roughness (the paint carries what is finer still).
    let fine = 0.016 * noise::fbm3p([p[0] / 0.3, p[1] / 0.3, p[2] / 0.3], seed ^ 0xf1e, 2, 0.5);
    mass + ledge - parting - crack + fine
}

/// Ring positions along the passage (z, from the mouth in): finest round
/// the player's stand, coarser behind it and further in.
fn rings(c: &Cave, focus: f32, density: f32) -> Vec<f32> {
    let fine = 0.04 / density.clamp(0.3, 1.0);
    let mut zs = vec![0.0f32];
    let mut d = 0.0f32;
    while d < c.len - 1e-3 {
        let k = d - focus;
        let step = if k < -1.0 {
            fine + 0.05 * (-1.0 - k)
        } else if k <= 6.0 {
            fine
        } else {
            (fine + 0.018 * (k - 6.0)).min(0.25)
        };
        d = (d + step.min(0.3)).min(c.len);
        zs.push(-d);
    }
    zs
}

/// The walls and roof: one arched sheet from the right foot round to the
/// left, ring by ring in from the mouth, its faces towards the passage;
/// a cap closes the far end. Each vertex also carries how far round the
/// arch it is (m), for the split into rocks.
fn shell(c: &Cave, zs: &[f32], around: u32, seed: u32) -> Mesh {
    let na = around.max(16);
    let pw = 3.0f32;
    let mut m = Mesh::default();
    for &z in zs {
        let (x0, hw, h) = (c.centre(z), c.half_width(z), c.height(z));
        // The section: a superellipse arch, its feet down under the floor.
        let at = |th: f32| -> [f32; 2] {
            let (sn, cs) = th.sin_cos();
            let x = cs.signum() * cs.abs().powf(2.0 / pw) * hw;
            let y = sn.abs().powf(2.0 / pw) * (h + FOOT);
            [x, y]
        };
        let mut s = 0.0f32;
        let mut prev = at(0.0);
        for j in 0..=na {
            let th = std::f32::consts::PI * j as f32 / na as f32;
            let q = at(th);
            s += ((q[0] - prev[0]).powi(2) + (q[1] - prev[1]).powi(2)).sqrt();
            prev = q;
            // The inward normal of the section (the tangent turned a
            // quarter).
            let e = 1e-3;
            let (a, b) = (at((th - e).max(0.0)), at((th + e).min(std::f32::consts::PI)));
            let n = geo::normalize([-(b[1] - a[1]), b[0] - a[0], 0.0]);
            let base = [x0 + q[0], q[1], z];
            let mut d = relief(base, s, seed);
            // Near the floor the walls never stand back from it (the floor
            // ends inside the rock), and the passage never closes.
            if q[1] < FOOT + 0.6 {
                d = d.max(-0.15);
            }
            d = d.clamp(-0.35, 0.4 * hw.min(h));
            m.vertices.push([base[0] + n[0] * d, (base[1] + n[1] * d).max(0.0), base[2] + n[2] * d, n[0], n[1], n[2]]);
        }
    }
    let row = na + 1;
    for i in 0..zs.len().saturating_sub(1) as u32 {
        for j in 0..na {
            let a = i * row + j;
            m.indices.extend_from_slice(&[a, a + 1, a + row + 1, a, a + row + 1, a + row]);
        }
    }
    // The far end: a fan to its middle.
    if let Some(&z) = zs.last() {
        let last = (zs.len() as u32 - 1) * row;
        let mid = m.vertices.len() as u32;
        m.vertices.push([c.centre(z), FOOT + c.height(z) * 0.4, z - 0.3, 0.0, 0.0, 1.0]);
        for j in 0..na {
            m.indices.extend_from_slice(&[last + j, last + j + 1, mid]);
        }
    }
    geo::smooth_normals(&mut m);
    m
}

/// The rock face round the mouth, facing out (+z): a band of rock from the
/// mouth's outline out to `reach` m, so from outside the mouth is a hole in
/// a cliff.
fn mouth_face(c: &Cave, reach: f32, seed: u32) -> Mesh {
    let na = 64u32;
    let nr = 10u32;
    let pw = 2.6f32;
    let (x0, hw, h) = (c.centre(0.0), c.half_width(0.0), c.height(0.0));
    let mut m = Mesh::default();
    for r in 0..=nr {
        let k = r as f32 / nr as f32;
        for j in 0..=na {
            let th = std::f32::consts::PI * j as f32 / na as f32;
            let (sn, cs) = th.sin_cos();
            let grow = 1.0 + k * reach / hw.max(0.5);
            let x = cs.signum() * cs.abs().powf(2.0 / pw) * hw * grow;
            let y = sn.abs().powf(2.0 / pw) * (h + FOOT + k * reach);
            let p = [x0 + x, y, 0.15 * k * reach * (1.0 - k)];
            let d = 0.35 * noise::fbm3p([p[0] / 1.5, p[1] / 1.5, 0.0], seed ^ 0x3017, 3, 0.5) * k.sqrt();
            m.vertices.push([p[0], p[1].max(0.0), p[2] + d, 0.0, 0.0, 1.0]);
        }
    }
    let row = na + 1;
    for r in 0..nr {
        for j in 0..na {
            let a = r * row + j;
            // Faces out of the cave (+z).
            m.indices.extend_from_slice(&[a, a + row + 1, a + 1, a, a + row, a + row + 1]);
        }
    }
    geo::smooth_normals(&mut m);
    m
}

/// The floor: a grid across the passage from wall to wall (its edges inside
/// the rock), on the same rings as the walls, faces up.
fn floor(c: &Cave, zs: &[f32], across: u32) -> Mesh {
    let nx = across.max(8);
    let mut m = Mesh::default();
    for &z in zs {
        let (x0, hw) = (c.centre(z), c.half_width(z) + 0.4);
        for j in 0..=nx {
            let x = x0 - hw + 2.0 * hw * j as f32 / nx as f32;
            m.vertices.push([x, c.floor(x, z), z, 0.0, 1.0, 0.0]);
        }
    }
    let row = nx + 1;
    for i in 0..zs.len().saturating_sub(1) as u32 {
        for j in 0..nx {
            let a = i * row + j;
            m.indices.extend_from_slice(&[a, a + 1, a + row + 1, a, a + row + 1, a + row]);
        }
    }
    geo::smooth_normals(&mut m);
    m
}

/// The rotation taking +y to unit `n` (axis, angle), for [`sh::rotate`].
fn up_to(n: [f32; 3]) -> ([f32; 3], f32) {
    let ax = [n[2], 0.0, -n[0]];
    let l = (ax[0] * ax[0] + ax[2] * ax[2]).sqrt();
    if l < 1e-5 { ([1.0, 0.0, 0.0], 0.0) } else { ([ax[0] / l, 0.0, ax[2] / l], n[1].clamp(-1.0, 1.0).acos()) }
}

/// A small sharp chip: a jagged tetrahedron, its base on y = 0, `len`
/// across (the scree's chip).
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

/// A stone set on the floor at (x, z): turned `yaw`, tilted a little, laid
/// on the floor's slope and sunk `sink` m into it.
fn lay(c: &Cave, mut m: Mesh, x: f32, z: f32, yaw: f32, tilt: [f32; 2], sink: f32) -> Mesh {
    sh::rotate(&mut m, [0.0, 1.0, 0.0], yaw);
    sh::rotate(&mut m, [1.0, 0.0, 0.0], tilt[0]);
    sh::rotate(&mut m, [0.0, 0.0, 1.0], tilt[1]);
    let n = c.floor_normal(x, z);
    let (ax, a) = up_to(n);
    if a > 1e-4 {
        sh::rotate(&mut m, ax, a);
    }
    let (lo, _) = geo::bounds(&m);
    geo::transform(&mut m, [1.0; 3], [x, c.floor(x, z) - lo[1] - sink, z]);
    m
}

/// Where a stone lies across the floor: by the walls' feet (`by_wall` of
/// them) or anywhere across; x for depth `z`.
fn across(c: &Cave, z: f32, u: f32, side: f32, by_wall: bool) -> f32 {
    let hw = c.half_width(z);
    let f = if by_wall { 0.62 + 0.3 * u } else { 0.7 * (2.0 * u - 1.0) };
    c.centre(z) + if by_wall { side * f } else { f } * hw
}

/// The fallen stones: blocks along the walls and a few across the floor,
/// cobbles round the player, into the buff and the pale stone; chips of the
/// floor's own grit.
fn rubble(c: &Cave, amount: f32, focus: f32, density: f32, seed: u32) -> [Mesh; 3] {
    let mut out: [Mesh; 3] = Default::default();
    if amount <= 0.0 {
        return out;
    }
    let s = seed ^ 0x5701;
    let reach = (c.len * 0.92).max(1.0);
    // The blocks (0.3 to 1 m): in full near the player, plainer further in.
    let blocks = (amount * c.len * 0.9).round() as u32;
    for i in 0..blocks {
        let r = |k: u32| sh::rnd(s, i, k);
        let z = -(0.8 + (reach - 0.8) * r(1).powf(1.6));
        let by_wall = r(2) < 0.8;
        let side = if r(3) < 0.5 { -1.0 } else { 1.0 };
        let x = across(c, z, r(4), side, by_wall);
        let size = (0.35 + 0.95 * r(5) * r(5)) * (c.h / 4.0).clamp(0.5, 1.5);
        let near = (-z - focus).abs() < 9.0;
        let cheap = if near && density >= 0.5 { 0 } else { 1 };
        let m = blade(size * (0.45 + 0.25 * r(6)), size, size * (0.65 + 0.25 * r(7)), 0.0, 0.12, 0.3, 0.0, 0.8, s ^ i.wrapping_mul(0x2f1), density, cheap);
        let m = lay(c, m, x, z, r(8) * sh::TAU, [(r(9) - 0.5) * 0.3, (r(10) - 0.5) * 0.3], size * 0.12);
        geo::merge(&mut out[usize::from(r(11) < 0.35)], &m);
    }
    // Cobbles (0.08 to 0.35 m) within 14 m of the player.
    let lo = (focus - 2.0).max(0.4);
    let hi = (focus + 14.0).min(reach);
    let n = (amount * (hi - lo).max(0.0) * 10.0 * density.clamp(0.3, 1.0)).round() as u32;
    for i in 0..n {
        let r = |k: u32| sh::rnd(s ^ 0xc0b, i, k);
        let z = -(lo + (hi - lo) * r(1));
        let by_wall = r(2) < 0.8;
        let side = if r(3) < 0.5 { -1.0 } else { 1.0 };
        let x = across(c, z, r(4), side, by_wall);
        let size = 0.1 + 0.35 * r(5) * r(5);
        let cheap = if (-z - focus).abs() < 5.0 { 1 } else { 3 };
        let m = blade(size * (0.4 + 0.3 * r(6)), size, size * (0.6 + 0.3 * r(7)), 0.0, 0.1, 0.3, 0.0, 0.0, s ^ 0xc0b ^ i.wrapping_mul(0x3a7), density, cheap);
        let m = lay(c, m, x, z, r(8) * sh::TAU, [(r(9) - 0.5) * 0.5, (r(10) - 0.5) * 0.5], size * 0.15);
        geo::merge(&mut out[usize::from(r(11) < 0.4)], &m);
    }
    // Sharp chips (3 to 10 cm) round the player, thicker by the walls.
    let hi = (focus + 10.0).min(reach);
    let n = (amount * (hi - lo).max(0.0) * 22.0 * density.clamp(0.3, 1.0)).round() as u32;
    for i in 0..n {
        let r = |k: u32| sh::rnd(s ^ 0xc41, i, k);
        let z = -(lo + (hi - lo) * r(1));
        let by_wall = r(2) < 0.55;
        let side = if r(3) < 0.5 { -1.0 } else { 1.0 };
        let x = across(c, z, r(4), side, by_wall);
        let m = lay(c, chip(0.04 + 0.08 * r(5), s ^ i), x, z, r(8) * sh::TAU, [0.0, 0.0], 0.008);
        geo::merge(&mut out[2], &m);
    }
    out
}

/// Tint a surface's base colour by the colour param (at its default: none).
fn tinted(sf: Surface, colour: [f32; 3]) -> Surface {
    if (0..3).all(|k| (colour[k] - COLOR[k]).abs() <= 1e-3) {
        return sf;
    }
    let a = sf.colour(0);
    sf.tint([a[0] * colour[0] / COLOR[0], a[1] * colour[1] / COLOR[1], a[2] * colour[2] / COLOR[2]])
}

/// The cave (the library asset's generator).
pub fn generate(v: &Values, seed: u32) -> Output {
    let c = Cave::new(v, seed);
    let density = v.density();
    let focus = v.f("focus").clamp(0.0, c.len);
    let zs = rings(&c, focus, density);
    // Round the arch about 6 cm a step at High.
    let perim = 2.0 * (c.h + FOOT) + std::f32::consts::PI * c.w;
    let around = ((perim / 0.06) * density.clamp(0.3, 1.0)).clamp(48.0, 320.0) as u32;
    let walls = shell(&c, &zs, around, seed);
    let face = mouth_face(&c, (c.h * 0.8).max(1.5), seed);
    let across_n = ((2.0 * c.w + 0.8) / 0.07 * density.clamp(0.3, 1.0)).clamp(12.0, 120.0) as u32;
    let ground = floor(&c, &zs, across_n);
    let stones = rubble(&c, v.f("rubble").max(0.0), focus, density, seed);
    let colour = v.color("color");
    let rock = rocks();
    let moss = v.f("moss").clamp(0.0, 1.0);
    // The walls into the buff and the grey limestone in broad patches, the
    // algae on the lower walls near the mouth where daylight reaches.
    let wall_parts = detail::split_by(&walls, 3, |p, _| {
        let reach = c.h * 2.2;
        let low = 1.0 - noise::smoothstep(FOOT + 0.4, FOOT + 2.6, p[1]);
        let near = 1.0 - noise::smoothstep(reach * 0.4, reach, -p[2]);
        let m = moss * low * near * (0.55 + 0.6 * noise::fbm3p([p[0] / 0.9, p[1] / 0.6, p[2] / 0.9], seed ^ 0x6055, 3, 0.55));
        if m > 0.24 {
            return 2;
        }
        usize::from(noise::fbm3p([p[0] / 4.0, p[1] / 2.5, p[2] / 4.0], seed ^ 0x9e7, 2, 0.5) > 0.12)
    });
    let mut out = Vec::new();
    for (k, m) in wall_parts.into_iter().enumerate() {
        if !m.indices.is_empty() {
            out.push(Part::surfaced(m, tinted(rock[k].seed(seed ^ k as u32), colour)));
        }
    }
    out.push(Part::surfaced(face, tinted(rock[0].seed(seed ^ 7), colour)));
    // The floor: packed earth down the middle, fallen gravel banked against
    // the walls, their border ragged.
    let floor_parts = detail::split_by(&ground, 2, |p, _| {
        let e = (p[0] - c.centre(p[2])).abs() / c.half_width(p[2]).max(0.2);
        usize::from(e + 0.18 * noise::fbm2(p[0] / 0.7, p[2] / 0.7, seed ^ 0x6a7, 3, 2.0, 0.5) > 0.74)
    });
    // Dusty brown earth down the middle; banked against the walls a greyer
    // grit of fallen limestone. Both a grit of dark and pale grains in a
    // mottle (the scree's grit; no cells, so nothing reads as paving).
    let earth = stone([158, 142, 124], [104, 92, 80], [190, 178, 162], [0.28, 0.24], 0.75, [0.0, 10.0], 0.95, 0.55).seed(seed ^ 0xea7);
    let gravel = stone([160, 152, 138], [94, 86, 76], [200, 194, 180], [0.32, 0.3], 0.7, [0.0, 10.0], 0.92, 0.45).seed(seed ^ 0x67a);
    for (m, sf) in floor_parts.into_iter().zip([earth, gravel]) {
        if !m.indices.is_empty() {
            out.push(Part::surfaced(m, sf));
        }
    }
    let chips = stone([132, 122, 106], [80, 72, 62], [180, 172, 156], [0.1, 0.1], 0.7, [0.0, 10.0], 0.9, 0.5);
    for (m, sf) in stones.into_iter().zip([rock[0], rock[3], chips]) {
        if !m.indices.is_empty() {
            out.push(Part::surfaced(m, tinted(sf.seed(seed ^ 0x51), colour)));
        }
    }
    Output::Parts(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_procedural::Value;

    fn values(pairs: &[(&str, f32)]) -> Values {
        let mut v = Values::defaults(&PARAMS);
        for (k, x) in pairs {
            v.set(k, Value::F32(*x));
        }
        v
    }

    /// The passage is open: inside it, between floor and roof, there is room
    /// to stand all the way in, and the stand round the player is level.
    #[test]
    fn a_player_can_walk_in() {
        let c = Cave::new(&values(&[("size", 40.0), ("opening", 4.2), ("width", 5.0)]), 7);
        for k in 0..30 {
            let z = -(1.0 + k as f32);
            assert!(c.roof(z) - c.floor(c.centre(z), z) > 2.0, "roof too low at {z}");
            assert!(c.half_width(z) > 1.2, "too narrow at {z}");
        }
        assert!(c.floor_normal(c.centre(-3.0), -3.0)[1] > 0.95);
    }

    /// The scene's cave stays inside the library's per-asset triangle limit,
    /// and the default one is small.
    #[test]
    fn triangles_stay_in_budget() {
        let big = generate(&values(&[("size", 40.0), ("opening", 4.2), ("width", 5.0), ("bend", -20.0), ("focus", 2.0)]), 7);
        assert!(big.triangles() < 400_000, "{} triangles", big.triangles());
        let small = generate(&Values::defaults(&PARAMS), 7);
        assert!(small.triangles() < 200_000, "{} triangles", small.triangles());
        let (lo, _) = big.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo[1] > -1e-3, "on the ground: {lo:?}");
    }

    /// The entrance lights stand at the mouth, the sky's ones high, reaching
    /// well in but not to the far end, eight in all (the plain path's limit).
    #[test]
    fn entrance_lights_reach_in() {
        let v = values(&[("size", 40.0), ("opening", 4.2)]);
        let c = Cave::new(&v, 7);
        let l = entrance_lights(&v, 7, 1.0);
        assert_eq!(l.len(), 8);
        assert!(l.iter().all(|l| l.position[2] > -3.0 && l.position[2] < 2.5 && l.intensity > 0.0));
        assert!(l.iter().take(6).all(|l| l.range >= 15.0 && l.range < 40.0 && l.position[1] > FOOT + c.h * 0.5));
    }
}
