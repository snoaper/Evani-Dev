//! Fresh powder snow (the Asset Studio fresh powder session, column 76,
//! 2026-11-11): the catalogue's `ground.snow.fresh` ("Snow, fresh powder":
//! soft bright white with blue shadows and sparkle) shown the way a studio
//! shows a loose material, as a low heap on a floor, so a studio shot, the
//! Material library or a game shows the material at its real scale. Adapted
//! from the dark loam's heap (`soil.rs`, column 69).
//!
//! The heap is real geometry down to a millimetre. Its body is one surface:
//! a low mound with its top a little off centre, drifts and round-topped
//! lobes of three sizes that roll into one another (a shallow round hollow
//! where two meet, never a crease), a lobed outline, and a foot that ends
//! in a rounded wall the best part of a centimetre high with gaps where
//! the floor shows (never a thin sheet). Loose snow stands off it as
//! clumps: a soft round lump of snow (twenty faces, uneven, never a
//! chip) with small crystals (soft double pyramids of one to three
//! millimetres) standing off it and thin needles out of it, all taking
//! the snow's own soft normal. Clumps of half a centimetre to two stand
//! along the foot's line, over its edge, two deep, in groups with gaps
//! between, so the snow ends in a ragged frill with shade under it;
//! small tufts of crystals lie in a band just above it, and crumbs
//! (loose flocks of a few crystals) on the floor (round 3; round 2's
//! flat flakes read as ice shards or grass, its sharp-crested lobes as
//! flat facets).
//!
//! The paint is the catalogue row itself ([`presets::GROUND_SNOW_FRESH`], a
//! game that lists the `ice` recipe draws it the same) as the recipe's
//! powder ([`surface`]: the frost cover at its fullest on opaque snow with
//! the inner cracks at their fullest is loose powder: clumps of every size
//! from a centimetre down, a crisp grain of crystals, a cool glow in its
//! shade and pinpoint glints), so everything under a few millimetres comes
//! from the recipe and keeps its size on every slope.
//!
//! [`heap`] is the whole heap at the origin, standing on y = 0;
//! [`heap_with`] takes the loose snow's amounts ([`Fluff`]).

use std::f32::consts::TAU;

use evani_procedural::Part;
use evani_procedural::noise::{fbm, hash, perlin2, perlin3, smoothstep};
use evani_procedural::surface::{Surface, presets, srgb_to_linear};
use evani_render::mesh::Mesh;

/// The heap's base radius (m) at the default size.
pub const RADIUS: f32 = 0.27;
/// Its height (m).
pub const HEIGHT: f32 = 0.118;
/// Where the top stands off the base's centre (m: x, z), so one side is
/// longer and gentler than the other.
pub const PEAK: [f32; 2] = [-0.045, -0.035];

/// How many paints the heap uses.
const TONES: usize = 2;

/// Which paint a part takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// The heap's body.
    Body = 0,
    /// The loose snow: the clumps along the foot, the tufts of crystals
    /// just above it and the crumbs on the floor (the same snow, its
    /// pattern laid another way).
    Loose = 1,
}

/// The powder's own colour (sRGB bytes): a white a shade cool.
pub const SNOW: [u8; 3] = [250, 250, 252];
/// Its cool shade (sRGB bytes): the creases between its clumps and the
/// faces turned from the light, where the light has come through the snow.
pub const SHADE: [u8; 3] = [205, 216, 240];
/// Its crystals where they catch the light (sRGB bytes).
pub const CRYSTAL: [u8; 3] = [255, 255, 255];

/// The powder's lumps per metre (the recipe's crack scale): its largest
/// clumps are a fifth of a lump across (5 mm), the rest every size down to
/// a grain under a millimetre.
pub const CLUMPS: f32 = 40.0;

/// The loose snow's amounts: the heap's live sliders, each 1 at the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fluff {
    /// The nap: small tufts of crystals on the body in a band just above
    /// its foot.
    pub nap: f32,
    /// The lace: clumps of crystals along the foot and just past it.
    pub lace: f32,
    /// A clump's greatest width over the default's (20 mm), and a tuft's
    /// crystals' length (2.6 mm).
    pub long: f32,
    /// How far past the foot the lace reaches over the default's (about a
    /// centimetre and a half).
    pub reach: f32,
    /// The crumbs on the floor round the foot.
    pub clumps: f32,
}

impl Default for Fluff {
    fn default() -> Self {
        Self { nap: 1.0, lace: 1.0, long: 1.0, reach: 1.0, clumps: 1.0 }
    }
}

/// How many tufts lie in the nap at the default density, how many clumps
/// stand in the lace, and how many crumbs lie on the floor.
const NAP: f32 = 120.0;
const LACE: f32 = 640.0;
const LOOSE: f32 = 220.0;
/// A crystal's greatest length (m) in the nap, and a clump's greatest width
/// in the lace.
const NAP_LONG: f32 = 0.0026;
const LACE_LONG: f32 = 0.02;
/// The twelve corners of a clump's lump (unit) and its twenty faces.
const LUMP: [[f32; 3]; 12] = [
    [-0.525_731, 0.850_651, 0.0],
    [0.525_731, 0.850_651, 0.0],
    [-0.525_731, -0.850_651, 0.0],
    [0.525_731, -0.850_651, 0.0],
    [0.0, -0.525_731, 0.850_651],
    [0.0, 0.525_731, 0.850_651],
    [0.0, -0.525_731, -0.850_651],
    [0.0, 0.525_731, -0.850_651],
    [0.850_651, 0.0, -0.525_731],
    [0.850_651, 0.0, 0.525_731],
    [-0.850_651, 0.0, -0.525_731],
    [-0.850_651, 0.0, 0.525_731],
];
const LUMP_FACES: [[usize; 3]; 20] = [
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
/// The radius (m) of the rounded wall the snow ends in at its foot.
const EDGE: f32 = 0.006;
/// Snow thinner than this (m) does not lie: the heap has no thin apron.
const THIN: f32 = 0.0025;

/// A colour from sRGB bytes into colour `k` of `s`.
fn col(s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.set_colour(k, [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])])
}

/// The fresh powder as a paint for any mesh (a drift, a roof, a field): the
/// catalogue's `ground.snow.fresh` drawn as the ice recipe's powder (the
/// frost variant, frost at 1 on opaque snow, the inner cracks extra with
/// the crack amount at 1; the crack scale is the lumps per metre), with
/// its sparkle and scatter. A game lists the `ice` recipe to draw it.
pub fn surface(seed: u32) -> Surface {
    let s = presets::GROUND_SNOW_FRESH.seed(seed).extras(0x07).number(0, 0.0).number(1, 1.0).number(2, (CLUMPS - 1.0) / 49.0).number(5, 1.0);
    col(col(col(s, 0, SNOW), 1, SHADE), 2, CRYSTAL)
}

/// The paint of each part (index = [`Tone`]).
fn surfaces(seed: u32) -> [Surface; TONES] {
    [surface(seed), surface(seed ^ 0x5f)]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
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

/// Add a triangle of a closed lump round `c`, wound to face away from it,
/// with these corner normals.
fn tri_from(m: &mut Mesh, c: [f32; 3], p: [[f32; 3]; 3], n: [[f32; 3]; 3]) {
    let f = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    if dot(f, f) < 1e-26 {
        return;
    }
    let mid = scale(add(add(p[0], p[1]), p[2]), 1.0 / 3.0);
    let order = if dot(f, sub(mid, c)) < 0.0 { [0, 2, 1] } else { [0, 1, 2] };
    for i in order {
        m.indices.push(m.vertices.len() as u32);
        m.vertices.push([p[i][0], p[i][1], p[i][2], n[i][0], n[i][1], n[i][2]]);
    }
}

/// How far the base reaches from the top along the unit direction
/// (`eu`, `ev`), in metres at the default size.
fn reach(eu: f32, ev: f32) -> f32 {
    let ae = PEAK[0] * eu + PEAK[1] * ev;
    (-ae + (ae * ae + RADIUS * RADIUS - PEAK[0] * PEAK[0] - PEAK[1] * PEAK[1]).max(0.0).sqrt()).max(1e-4)
}

/// The heap's smooth profile over how far out a point is (0 the top, 1 the
/// base's rim), as a share of its height: a rounded top, sides at a loose
/// powder's angle, falling away under the floor past the rim.
fn profile(t: f32) -> f32 {
    let bell = if t < 1.0 { 0.5 * (1.0 + (std::f32::consts::PI * t).cos()) } else { 0.0 };
    0.74 * bell + 0.26 * (1.0 - t.powf(2.4))
}

/// How far a corner of the body moves sideways, as a share of the grid's
/// step, so the mesh's corners don't line up in rows.
const SHUFFLE: f32 = 0.3;

/// A seeded number in [-0.5, 0.5) for corner `q`, channel `c`.
fn jitter(seed: u32, q: usize, c: u32) -> f32 {
    let r = hash(seed ^ (q as u32).wrapping_mul(0x9e37_79b9) ^ c.wrapping_mul(0x85eb_ca6b));
    (r >> 8) as f32 / (1u32 << 24) as f32 - 0.5
}

/// The snow's edge stands: loose snow of thickness `whole` (m; under 0: none)
/// ends in a rounded wall [`EDGE`] high, twice as steep as the layer under
/// it at its foot, never in a thin sheet.
fn wall(whole: f32) -> f32 {
    if whole > 0.0 { whole + EDGE * (1.0 - (-whole / EDGE).exp()) } else { whole }
}

/// A number in [-1, 1) on each axis.
fn rand3(rng: &mut Rng) -> [f32; 3] {
    [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)]
}

/// What the loose snow is made of: crystals. A crystal is a soft double
/// pyramid with three sides, one to four millimetres long; its corners'
/// normals lean to the snow's own soft normal, so it shades as the snow it
/// stands on (a little darker underneath) and never as a facet.
struct Frost {
    /// The way the snow under the crystals faces (unit).
    lean: [f32; 3],
    /// The lowest a crystal may reach (m).
    floor: f32,
}

impl Frost {
    /// One crystal round `c`: `len` long along the unit `axis`, `wide`
    /// across its middle, turned any way about the axis, its three sides
    /// uneven.
    fn crystal(&self, m: &mut Mesh, rng: &mut Rng, c: [f32; 3], axis: [f32; 3], len: f32, wide: f32) {
        let t = norm(cross(axis, if axis[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        let b = cross(axis, t);
        let lift = |q: [f32; 3]| [q[0], q[1].max(self.floor), q[2]];
        let turn = rng.range(0.0, TAU);
        let mut ring = [[0.0f32; 3]; 3];
        for (i, r) in ring.iter_mut().enumerate() {
            let a = turn + i as f32 * (TAU / 3.0) + rng.range(-0.35, 0.35);
            let out = add(scale(t, a.cos()), scale(b, a.sin()));
            *r = lift(add(add(c, scale(axis, len * rng.range(-0.2, 0.2))), scale(out, 0.5 * wide * rng.range(0.7, 1.3))));
        }
        let tips = [lift(add(c, scale(axis, 0.5 * len))), lift(sub(c, scale(axis, 0.5 * len)))];
        let soft = |q: [f32; 3]| norm(add(scale(norm(sub(q, c)), 0.5), self.lean));
        for i in 0..3 {
            let (r0, r1) = (ring[i], ring[(i + 1) % 3]);
            for tip in tips {
                tri_from(m, c, [tip, r0, r1], [soft(tip), soft(r0), soft(r1)]);
            }
        }
    }

    /// A tuft at `c` on snow that faces `n`: `count` crystals of about `r`
    /// lying any way round it, each rising a little off the snow, a fifth
    /// of it sunk in.
    fn tuft(&self, m: &mut Mesh, rng: &mut Rng, c: [f32; 3], n: [f32; 3], r: f32, count: usize) {
        let t = norm(cross(n, if n[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        let b = cross(n, t);
        for _ in 0..count {
            let a = rng.range(0.0, TAU);
            let axis = norm(add(add(scale(t, a.cos()), scale(b, a.sin())), scale(n, rng.range(0.1, 0.9))));
            let len = r * rng.range(0.7, 1.3);
            let wide = len * rng.range(0.5, 0.8);
            self.crystal(m, rng, add(c, scale(axis, 0.3 * len)), axis, len, wide);
        }
    }

    /// A needle from `root`: a thin three-sided spike `len` long along the
    /// unit `axis`, `wide` across its root (which stands inside a clump's
    /// lump, so it has no end face).
    fn needle(&self, m: &mut Mesh, rng: &mut Rng, root: [f32; 3], axis: [f32; 3], len: f32, wide: f32) {
        let t = norm(cross(axis, if axis[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        let b = cross(axis, t);
        let lift = |q: [f32; 3]| [q[0], q[1].max(self.floor), q[2]];
        let turn = rng.range(0.0, TAU);
        let mut ring = [[0.0f32; 3]; 3];
        for (i, r) in ring.iter_mut().enumerate() {
            let a = turn + i as f32 * (TAU / 3.0);
            *r = lift(add(root, scale(add(scale(t, a.cos()), scale(b, a.sin())), 0.5 * wide)));
        }
        let tip = lift(add(root, scale(axis, len)));
        let mid = add(root, scale(axis, 0.3 * len));
        let soft = |q: [f32; 3]| norm(add(scale(norm(sub(q, mid)), 0.5), self.lean));
        for i in 0..3 {
            let (r0, r1) = (ring[i], ring[(i + 1) % 3]);
            tri_from(m, mid, [tip, r0, r1], [soft(tip), soft(r0), soft(r1)]);
        }
    }

    /// A clump round `c`, `r` in radius (`mm` a millimetre at the heap's
    /// size). `lump` set (the foot's clumps): a soft round lump of snow
    /// three fifths as wide, twenty faces round twelve uneven corners,
    /// lower than wide, with small crystals standing off it (the larger
    /// the clump the more) and thin needles out of it to its radius, so
    /// it has body, a ragged outline and shade under it. Without (the
    /// floor's crumbs): a loose flock of a few crystals and nothing else.
    fn clump(&self, m: &mut Mesh, rng: &mut Rng, c: [f32; 3], r: f32, mm: f32, lump: bool) {
        let size = r / mm.max(1e-9);
        let core = 0.62 * r;
        let out = |rng: &mut Rng| {
            let j = rand3(rng);
            norm([j[0], j[1] + 0.35, j[2]])
        };
        if lump {
            let lift = |q: [f32; 3]| [q[0], q[1].max(self.floor), q[2]];
            let soft = |q: [f32; 3]| norm(add(scale(norm(sub(q, c)), 0.8), self.lean));
            let mut ps = [[0.0f32; 3]; 12];
            for (q, v) in ps.iter_mut().zip(LUMP) {
                let k = core * rng.range(0.78, 1.22);
                *q = lift([c[0] + v[0] * k, c[1] + 0.8 * v[1] * k, c[2] + v[2] * k]);
            }
            for f in LUMP_FACES {
                let (p0, p1, p2) = (ps[f[0]], ps[f[1]], ps[f[2]]);
                tri_from(m, c, [p0, p1, p2], [soft(p0), soft(p1), soft(p2)]);
            }
        }
        for _ in 0..(if lump { 4.0 + 0.25 * size * size } else { 3.0 + 1.5 * size }) as usize {
            let d = out(rng);
            // On the lump's skin, or anywhere in the flock (closer
            // together towards its middle).
            let at = add(c, scale(d, if lump { core * rng.range(0.85, 1.2) } else { 0.8 * r * rng.unit().sqrt() }));
            let axis = norm(add(d, scale(rand3(rng), 0.8)));
            let len = (mm * rng.range(1.3, 3.0)).min(0.8 * r);
            let wide = len * rng.range(0.4, 0.7);
            self.crystal(m, rng, at, axis, len, wide);
        }
        if lump {
            for _ in 0..(3.0 + 0.8 * size) as usize {
                let d = out(rng);
                let axis = norm(add(d, scale(rand3(rng), 0.35)));
                let len = r * rng.range(0.7, 1.15);
                self.needle(m, rng, add(c, scale(d, 0.5 * core)), axis, len, (0.14 * len).max(0.45 * mm));
            }
        }
    }
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

    /// How far out (`u`, `v`) is (m at the default size): 0 at the top, 1
    /// on the base's rim, more beyond; the rim runs in lobes of a hand's
    /// width and of a few fingers'.
    fn out(&self, u: f32, v: f32) -> f32 {
        let s = self.seed;
        let wu = u + 0.034 * perlin2(u * 5.2 + 3.1, v * 5.2, s ^ 0x11) + 0.011 * perlin2(u * 15.0, v * 15.0 + 1.9, s ^ 0x13);
        let wv = v + 0.034 * perlin2(u * 5.2, v * 5.2 + 7.7, s ^ 0x12) + 0.011 * perlin2(u * 15.0 + 4.4, v * 15.0, s ^ 0x14);
        let (du, dv) = (wu - PEAK[0], wv - PEAK[1]);
        let d = (du * du + dv * dv).sqrt();
        if d < 1e-6 {
            return 0.0;
        }
        d / reach(du / d, dv / d)
    }

    /// The foot's ragged edge: how much nearer the top a point near the rim
    /// counts (as a share of the reach; under 0: further out). The snow
    /// ends in rounded tufts of one to four centimetres across, with
    /// notches cut in between them (the zero lines of two noises), and a
    /// ripple of a few millimetres on both.
    fn rag(&self, u: f32, v: f32, t: f32) -> f32 {
        let s = self.seed;
        // Only the foot: a notch higher up is a pit that fills with shade.
        let near = smoothstep(0.8, 1.0, t);
        let wide = 1.4 * perlin2(u * 24.0 + 6.1, v * 24.0, s ^ 0x33).abs() - 0.6;
        let fine = 1.4 * perlin2(u * 52.0, v * 52.0, s ^ 0x31).abs() - 0.7;
        near * (0.1 * wide + 0.085 * fine + 0.02 * perlin2(u * 105.0 + 3.3, v * 105.0, s ^ 0x32))
    }

    /// The surface at (x, z): its height in metres over the floor (under 0:
    /// no snow stands there).
    fn at(&self, x: f32, z: f32) -> f32 {
        let (u, v) = (x / self.k, z / self.k);
        let t = self.out(u, v);
        let t = t - self.rag(u, v, t);
        let s = self.seed;
        let body = HEIGHT * self.hk * profile(t);
        // Drifts and lobes fade out as the layer thins.
        let thick = smoothstep(0.0, 0.035, body);
        let big = 0.03 * fbm(u * 4.6 + 1.7, v * 4.6 - 2.3, s ^ 0x21, 2, 2.0, 0.5);
        // The lobes are read in space, at the point on the smooth heap, so
        // one is as long down a steep side as it is wide. A lobe is round
        // on top; where it meets the next there is a shallow round hollow
        // (the zero lines of a noise, rounded right off: a crease there
        // shades as a flat facet and throws a hard shadow).
        let at = |f: f32, o: f32| [u * f + o, body * f, v * f];
        let lobe = |f: f32, o: f32, seed: u32| {
            let p = perlin3(at(f, o), seed);
            (p * p + 0.05).sqrt() - 0.32
        };
        let lobes = 0.056 * lobe(9.0, 0.0, s ^ 0x22) + 0.011 * lobe(23.0, 5.0, s ^ 0x23) + 0.002 * lobe(56.0, 2.0, s ^ 0x24);
        // Thin, the snow lies in tufts: round mounds a centimetre or two
        // across and half of one high, with the floor showing along the
        // creases between them; thinner than [`THIN`] it does not lie at
        // all. Only where it is thin: a tuft's hollow in thicker snow is
        // a pit that fills with shade and reads as a grey smudge.
        let tuft = perlin2(u * 45.0 + 1.1, v * 45.0, s ^ 0x26);
        let tufts = 0.015 * ((tuft * tuft + 0.003).sqrt() - 0.21);
        let whole = body + (big + lobes) * thick + tufts * (1.0 - smoothstep(0.0, 0.014, body));
        wall(whole - THIN) * self.k
    }

    /// The snow's soft normal at (x, z): its slope over a centimetre.
    fn soft(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 0.005 * self.k;
        norm([self.at(x - e, z) - self.at(x + e, z), 2.0 * e, self.at(x, z - e) - self.at(x, z + e)])
    }

    /// The way out from the heap at (x, z): downhill along the floor (unit).
    fn outward(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 0.004 * self.k;
        let (dx, dz) = (self.at(x + e, z) - self.at(x - e, z), self.at(x, z + e) - self.at(x, z - e));
        let l = (dx * dx + dz * dz).sqrt();
        if l > 1e-6 { [-dx / l, 0.0, -dz / l] } else { norm([x - PEAK[0] * self.k, 0.0, z - PEAK[1] * self.k]) }
    }

    /// The point `t` out from the top (0 the top, 1 the rim) at heading
    /// `a`: (x, z) in metres.
    fn spot(&self, a: f32, t: f32) -> (f32, f32) {
        let (eu, ev) = (a.cos(), a.sin());
        let d = t * reach(eu, ev);
        ((PEAK[0] + eu * d) * self.k, (PEAK[1] + ev * d) * self.k)
    }

    /// The body: a grid over the heap and its foot, drawn wherever snow
    /// stands above the floor, smooth across its faces. A corner's normal is
    /// half its own slope and half the slope over a few centimetres round
    /// it, so a lobe's side is never a hard shade line: light comes through
    /// loose snow. Each corner stands up to half a millimetre off the
    /// smooth surface (its normal keeps the smooth one's), so the heap's
    /// outline is never a clean line.
    fn body(&self, density: f32, m: &mut Mesh) {
        let cell = 0.0048 * self.k / density.clamp(0.25, 2.0).sqrt();
        let half = RADIUS * self.k * 1.42;
        let n = ((2.0 * half / cell).round() as usize).clamp(16, 420);
        let step = 2.0 * half / n as f32;
        let w = n + 1;
        let s = self.seed ^ 0x6a17;
        let smooth: Vec<[f32; 3]> = (0..w * w)
            .map(|q| {
                let x = -half + ((q % w) as f32 + SHUFFLE * jitter(s, q, 1)) * step;
                let z = -half + ((q / w) as f32 + SHUFFLE * jitter(s, q, 2)) * step;
                [x, self.at(x, z), z]
            })
            .collect();
        let ps: Vec<[f32; 3]> = smooth.iter().enumerate().map(|(q, p)| [p[0], p[1] + 0.001 * self.k * jitter(s, q, 3) * smoothstep(0.001, 0.006, p[1] / self.k), p[2]]).collect();
        // A corner's normal from the heights `r` corners away. Under the
        // floor a corner counts as just under it, so the edge's wall never
        // tips the normals of the snow above it flat.
        let y = |i: usize, j: usize| smooth[j * w + i][1].max(-0.002 * self.k);
        let slope = |i: usize, j: usize, r: usize| {
            let (i0, i1) = (i.saturating_sub(r), (i + r).min(n));
            let (j0, j1) = (j.saturating_sub(r), (j + r).min(n));
            let (sx, sz) = ((i1 - i0) as f32 * step, (j1 - j0) as f32 * step);
            norm([-(y(i1, j) - y(i0, j)) / sx, 1.0, -(y(i, j1) - y(i, j0)) / sz])
        };
        let wide = ((0.016 * self.k / step).round() as usize).max(2);
        let soft = |i: usize, j: usize| norm(add(slope(i, j, 1), slope(i, j, wide)));
        for j in 0..n {
            for i in 0..n {
                let (a, b, c, d) = (ps[j * w + i], ps[j * w + i + 1], ps[(j + 1) * w + i], ps[(j + 1) * w + i + 1]);
                if a[1].max(b[1]).max(c[1]).max(d[1]) <= 0.0 {
                    continue;
                }
                let (na, nb, nc, nd) = (soft(i, j), soft(i + 1, j), soft(i, j + 1), soft(i + 1, j + 1));
                // Split along the diagonal that bends less.
                if (a[1] - d[1]).abs() <= (b[1] - c[1]).abs() {
                    tri(m, [a, c, d], [na, nc, nd]);
                    tri(m, [a, d, b], [na, nd, nb]);
                } else {
                    tri(m, [a, c, b], [na, nc, nb]);
                    tri(m, [b, c, d], [nb, nc, nd]);
                }
            }
        }
    }

    /// The nap ([`NAP`] tufts): tufts of two to four crystals lying on the
    /// body any way round in a band just above its foot, where the frill
    /// of clumps thins out into the heap's skin (higher up a crystal
    /// standing alone on the snow reads as a speck of dirt).
    fn nap(&self, density: f32, fl: &Fluff, m: &mut Mesh) {
        let k = self.k;
        let mut rng = Rng(hash(self.seed ^ 0x7a91));
        let want = (NAP * fl.nap.clamp(0.0, 4.0) * density.clamp(0.25, 2.0)).round() as usize;
        let half = RADIUS * k * 1.3;
        let (mut made, mut tries) = (0usize, 0usize);
        while made < want && tries < want * 30 {
            tries += 1;
            let (x, z) = (rng.range(-half, half), rng.range(-half, half));
            let y = self.at(x, z);
            // The foot is the lace's, the heap above the band the snow's own.
            if y < 0.008 * k || rng.unit() > 1.0 - smoothstep(0.012 * k, 0.034 * k, y) {
                continue;
            }
            made += 1;
            let n = self.soft(x, z);
            let frost = Frost { lean: n, floor: 0.0002 * k };
            let (r, count) = (NAP_LONG * fl.long * k * rng.range(0.5, 1.0), 2 + (rng.unit() * 3.0) as usize);
            frost.tuft(m, &mut rng, [x, y, z], n, r, count);
        }
    }

    /// The lace ([`LACE`] clumps): on the foot's line and a finger's width
    /// either side of it, two deep, clumps of snow half a centimetre to
    /// two across, a few large and most small, the largest on the line,
    /// standing in groups with gaps between them, so the snow's edge is
    /// under a ragged frill that overhangs the floor, with shade under
    /// it, and never shows as a line.
    fn lace(&self, density: f32, fl: &Fluff, m: &mut Mesh) {
        let k = self.k;
        let mut rng = Rng(hash(self.seed ^ 0xc257));
        let want = (LACE * fl.lace.clamp(0.0, 4.0) * density.clamp(0.25, 2.0)).round() as usize;
        // How far under the floor the surface has run where the lace ends,
        // and how high on the foot it starts.
        let far = 0.004 * fl.reach.clamp(0.1, 4.0) * k;
        let top = 0.012 * k;
        let (mut made, mut tries) = (0usize, 0usize);
        while made < want && tries < want * 80 {
            tries += 1;
            // As many to a finger's width of rim on the heap's long side
            // as on its short one.
            let a = rng.range(0.0, TAU);
            if rng.unit() * (RADIUS + PEAK[0].hypot(PEAK[1])) > reach(a.cos(), a.sin()) {
                continue;
            }
            let (x, z) = self.spot(a, rng.range(0.7, 1.32));
            let g = self.at(x, z);
            if g > top || g < -far {
                continue;
            }
            let group = 0.45 + 0.55 * smoothstep(-0.15, 0.2, perlin2(x / k * 70.0, z / k * 70.0, self.seed ^ 0x41));
            let keep = if g < 0.0 { (1.0 + g / far).powf(2.5) } else { 1.0 - 0.85 * smoothstep(0.003 * k, 0.01 * k, g) };
            if rng.unit() > keep * group {
                continue;
            }
            made += 1;
            let out = self.outward(x, z);
            let share = rng.unit();
            let r = 0.5 * LACE_LONG * fl.long * k * (0.35 + 0.65 * share * share) * (0.6 + 0.4 * keep);
            let frost = Frost { lean: norm([out[0] * 0.3, 0.95, out[2] * 0.3]), floor: 0.0002 * k };
            // Sunk a little into what it stands on, leaning out over the floor.
            let c = [x + out[0] * 0.3 * r, g.max(0.0) + 0.45 * r, z + out[2] * 0.3 * r];
            frost.clump(m, &mut rng, c, r, 0.001 * k, true);
        }
    }

    /// The crumbs ([`LOOSE`]): on the floor a finger's width or two from
    /// the foot, most of them near it, each a small clump of a few
    /// crystals; white as the heap, lit from above as it is.
    fn loose(&self, density: f32, fl: &Fluff, m: &mut Mesh) {
        let k = self.k;
        let mut rng = Rng(hash(self.seed ^ 0x5011));
        let want = (LOOSE * fl.clumps.clamp(0.0, 4.0) * density.clamp(0.25, 2.0)).round() as usize;
        let far = 0.012 * k;
        let (mut made, mut tries) = (0usize, 0usize);
        while made < want && tries < want * 80 {
            tries += 1;
            let (x, z) = self.spot(rng.range(0.0, TAU), rng.range(0.95, 1.36));
            let g = self.at(x, z);
            if !(-far..-0.001 * k).contains(&g) || rng.unit() > (1.0 + g / far).powf(2.2) {
                continue;
            }
            made += 1;
            // Many small crumbs, a few larger.
            let share = rng.unit();
            let r = k * (0.0012 + 0.0026 * share * share);
            let frost = Frost { lean: [0.0, 1.0, 0.0], floor: 0.0002 * k };
            frost.clump(m, &mut rng, [x, 0.0003 * k + 0.5 * r, z], r, 0.001 * k, false);
        }
    }
}

/// The heap of fresh powder: `radius` at its base (m), `height` high,
/// standing on y = 0 with its base's centre at the origin. `density` thins
/// the mesh and the loose snow (1 = the default: about 5 mm between the
/// body's corners, the whole heap under 100,000 triangles; more goes over
/// that).
pub fn heap(radius: f32, height: f32, seed: u32, density: f32) -> Vec<Part> {
    heap_with(radius, height, seed, density, Fluff::default())
}

/// [`heap`] with the loose snow's amounts set (`Fluff::default()`: as
/// [`heap`]; over 1 on the nap or the lace goes over 100,000 triangles).
pub fn heap_with(radius: f32, height: f32, seed: u32, density: f32, fluff: Fluff) -> Vec<Part> {
    let h = Heap::new(radius, height, seed);
    let mut bins: [Mesh; TONES] = Default::default();
    h.body(density, &mut bins[Tone::Body as usize]);
    let loose = &mut bins[Tone::Loose as usize];
    h.nap(density, &fluff, loose);
    h.lace(density, &fluff, loose);
    h.loose(density, &fluff, loose);
    bins.into_iter().zip(surfaces(seed)).filter(|(m, _)| !m.indices.is_empty()).map(|(m, s)| Part::surfaced(m, s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heap_is_whole_and_within_the_budget() {
        let parts = heap(RADIUS, HEIGHT, 76, 1.0);
        assert_eq!(parts.len(), TONES, "every tone drawn");
        let tris: Vec<u32> = parts.iter().map(|p| p.mesh.triangles()).collect();
        let all: u32 = tris.iter().sum();
        println!("snow heap triangles: {all} (body {}, loose snow {})", tris[0], tris[1]);
        assert!(all > 60_000 && all < 100_000, "triangles {all}");
        for p in &parts {
            assert!(p.mesh.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())));
            assert!(p.mesh.indices.iter().all(|&i| (i as usize) < p.mesh.vertices.len()));
            assert!(p.mesh.vertices.iter().all(|v| v[1] < HEIGHT * 1.5 && v[0].abs() < RADIUS * 1.7 && v[2].abs() < RADIUS * 1.7), "inside its box");
        }
    }

    #[test]
    fn the_loose_snow_clears_the_floor_and_follows_its_sliders() {
        let parts = heap(RADIUS, HEIGHT, 76, 1.0);
        let loose = &parts[Tone::Loose as usize].mesh;
        assert!(loose.vertices.iter().all(|v| v[1] > 0.0), "no flake under the floor");
        // The loose snow's crystals take soft normals that look up, never a
        // facet's own.
        assert!(loose.vertices.iter().all(|v| (v[3] * v[3] + v[4] * v[4] + v[5] * v[5] - 1.0).abs() < 1e-3));
        let up = loose.vertices.iter().filter(|v| v[4] > 0.0).count();
        assert!(up * 100 >= loose.vertices.len() * 97, "up {up} of {}", loose.vertices.len());
        // No loose snow at all: the body alone.
        let bare = heap_with(RADIUS, HEIGHT, 76, 1.0, Fluff { nap: 0.0, lace: 0.0, clumps: 0.0, ..Fluff::default() });
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].mesh.vertices, parts[Tone::Body as usize].mesh.vertices);
        // Half the lace: fewer clumps.
        let thin = heap_with(RADIUS, HEIGHT, 76, 1.0, Fluff { lace: 0.5, ..Fluff::default() });
        assert!(thin[Tone::Loose as usize].mesh.triangles() < loose.triangles());
    }

    #[test]
    fn the_foot_ends_in_a_wall_not_a_sheet() {
        // A millimetre of snow stands nearly two high, thick snow stands a
        // wall's height higher, and nothing stands where there is none.
        assert!(wall(0.001) > 0.0018, "thin {}", wall(0.001));
        assert!((wall(0.05) - 0.05 - EDGE).abs() < 1e-4);
        assert_eq!(wall(-0.003), -0.003);
        assert!(wall(0.0) == 0.0 && wall(0.002) > wall(0.001));
    }

    #[test]
    fn the_heap_peaks_and_runs_out_under_the_floor() {
        let h = Heap::new(RADIUS, HEIGHT, 76);
        let top = h.at(PEAK[0], PEAK[1]);
        assert!((top - HEIGHT).abs() < 0.05, "top {top}");
        // Half way down every side the snow stands well above the floor.
        for q in 0..12 {
            let (x, z) = h.spot(q as f32 / 12.0 * TAU, 0.5);
            assert!(h.at(x, z) > 0.025, "side {q}");
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
    fn every_crystal_is_closed_and_faces_outwards() {
        let mut m = Mesh::default();
        let mut rng = Rng(7);
        let frost = Frost { lean: [0.0, 1.0, 0.0], floor: -1.0 };
        let c = [0.01, 0.02, 0.03];
        frost.crystal(&mut m, &mut rng, c, norm([0.3, 0.8, -0.2]), 0.003, 0.0012);
        assert_eq!(m.indices.len(), 18, "six faces");
        for t in m.indices.chunks(3) {
            let p = |i: u32| {
                let v = m.vertices[i as usize];
                [v[0], v[1], v[2]]
            };
            let (a, b, d) = (p(t[0]), p(t[1]), p(t[2]));
            let mid = scale(add(add(a, b), d), 1.0 / 3.0);
            assert!(dot(cross(sub(b, a), sub(d, a)), sub(mid, c)) > 0.0);
        }
        // A clump of half a centimetre is its lump, 10 crystals and 7
        // needles, all within a little over its radius of its middle; a
        // crumb (no lump, no needles) is lighter.
        let mut f = Mesh::default();
        frost.clump(&mut f, &mut rng, c, 0.005, 0.001, true);
        assert!(f.triangles() > 20 + 8 * 6 && f.triangles() <= 20 + 10 * 6 + 7 * 3, "triangles {}", f.triangles());
        assert!(f.vertices.iter().all(|v| ((v[0] - c[0]).powi(2) + (v[1] - c[1]).powi(2) + (v[2] - c[2]).powi(2)).sqrt() < 0.008));
        let mut g = Mesh::default();
        frost.clump(&mut g, &mut rng, c, 0.005, 0.001, false);
        assert!(g.triangles() < f.triangles());
    }

    #[test]
    fn the_paint_is_the_catalogue_row_as_powder() {
        let s = surface(76);
        // The ice recipe's frost variant, opaque, fully frosted, the inner
        // cracks at their fullest: the recipe's powder.
        assert_eq!(s.block[0], presets::GROUND_SNOW_FRESH.block[0]);
        assert_eq!(s.block[1], 2);
        assert_eq!(s.block[2] & 0x07, 0x07);
        assert_eq!(s.number_of(0), 0.0);
        assert_eq!(s.number_of(1), 1.0);
        assert_eq!(s.number_of(5), 1.0);
    }
}
