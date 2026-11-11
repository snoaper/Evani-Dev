//! Rock piles (rocks-rock-piles; the Asset Studio rock piles session,
//! column 26, 2026-11-08): a heap of freshly broken rock, a big leaning
//! block of grey schist at the back, broad slabs and blocks of other stone
//! (a grey-green schist, a rusty iron-stained block, a pale quartzite with a
//! white quartz band, a tan block, a blue-grey stone) leaning on it and on
//! each other, smaller blocks wedged between them, stones and gravel spilled
//! round the foot.
//!
//! Each rock is one welded mesh: a slightly boxy form (a superellipsoid) cut
//! by fracture planes whose meeting is kept crisp (refined where two faces
//! meet), a flat bed underneath, faces that are never quite flat (a broad
//! wave and a fine roughness), the schists' sheets stepped back along their
//! foliation, and fresh chips broken out of the upper edges as real geometry
//! ([`detail::spalls`]). The form, the steps and the chips are the medium
//! rocks' (crate::medium_rocks, column 23), adapted here.
//!
//! The pile is built the way a heap forms: the rocks are dropped one after
//! another, biggest first, each turned and tilted as it lies, onto a height
//! field of the ground and the rocks already down, and each settles a few
//! millimetres into what it lands on, so every rock rests on the ground or
//! on the rocks under it, never hovers ([`Field`]).
//!
//! The paint is the stone recipe's variant 11 (rc_stone.wgsl: world space,
//! the same in any game that lists `stone`) in six stones. The grey rocks'
//! triangles are sorted into the grey and a rusty iron-stained stone by a
//! broad 3D noise, so the rust lies in patches across them as weathering
//! does (the jagged rock formations' method, crate::jagged_rocks, column 24).

use evani_procedural::detail;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::noise;
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};

/// The library colour param's default: at it every stone keeps its own tone.
pub const COLOR: [f32; 3] = [0.3, 0.29, 0.27];

/// The reference pile's height (m): the layout below is at it.
const REF_H: f32 = 1.2;

/// The stones (the parts, in this order).
const GREY: usize = 0;
const RUST: usize = 1;
const PALE: usize = 2;
const GREEN: usize = 3;
const TAN: usize = 4;
const BLUE: usize = 5;
const STONES: usize = 6;

/// sRGB bytes into colour `k` of a block.
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A stone in the recipe's variant 11 (column 23's helper): base `a`, dark
/// grains of `b`, pale grains, cloud and veins of `c`; `dark` and `pale` the
/// grains' shares (0-1), `grain` their scale (per m, 100-1000), `cloud` the
/// mottle towards `c`, `veins` their width (0: none) `per_m` apart, `relief`
/// the grain relief, `rough` the roughness, `stains` (number 8) and
/// `foliation` (number 9, the streaks along the veins' planes).
#[allow(clippy::too_many_arguments)]
fn stone(a: [u8; 3], b: [u8; 3], c: [u8; 3], [dark, pale]: [f32; 2], grain: f32, cloud: f32, [veins, per_m]: [f32; 2], relief: f32, rough: f32, stains: f32, foliation: f32) -> Surface {
    let s = presets::ROCK_GRANITE_GREY.variant(11).extras(0);
    srgb(srgb(srgb(s, 0, a), 1, b), 2, c)
        .number(0, rough)
        .number(1, veins)
        .number(2, (per_m - 1.0) / 49.0)
        .number(3, (grain - 100.0) / 900.0)
        .number(4, dark)
        .number(5, pale)
        .number(6, cloud)
        .number(7, relief)
        .number(8, stains)
        .number(9, foliation)
}

/// The six stones, with the world direction their veins and streaks cross.
fn stones() -> [(Surface, [f32; 3]); STONES] {
    [
        // Grey schist: mid grey with an olive cast, cream-white quartz
        // streaks along its foliation, dark mica flecks.
        (stone([118, 111, 99], [62, 60, 54], [182, 172, 152], [0.12, 0.08], 300.0, 0.65, [0.09, 6.0], 1.0, 0.8, 0.8, 0.55), [0.8, 0.35, 0.3]),
        // Its rusty iron-stained weathering: orange-brown over grey-brown.
        (stone([126, 110, 94], [74, 66, 58], [176, 122, 78], [0.1, 0.06], 300.0, 0.8, [0.08, 7.0], 1.0, 0.84, 0.9, 0.5), [0.8, 0.35, 0.3]),
        // Pale quartzite: cream and light grey, a band of milky quartz.
        (stone([148, 138, 120], [100, 96, 88], [196, 188, 172], [0.08, 0.06], 260.0, 0.6, [0.3, 4.0], 0.8, 0.66, 0.7, 0.2), [0.7, 0.6, 0.4]),
        // Grey-green schist: grey-olive with yellow-olive flecks, pale streaks.
        (stone([114, 113, 100], [66, 68, 58], [170, 166, 144], [0.13, 0.09], 320.0, 0.6, [0.09, 8.0], 1.0, 0.78, 0.7, 0.6), [0.9, 0.2, 0.4]),
        // Tan: an ochre-tan quartzite, browner in its pits.
        (stone([146, 124, 96], [90, 76, 60], [180, 156, 118], [0.11, 0.08], 380.0, 0.55, [0.0, 1.0], 0.9, 0.84, 0.7, 0.3), [0.6, 0.3, 0.7]),
        // Blue-grey: a fine dark blue-grey stone.
        (stone([104, 108, 114], [60, 62, 68], [142, 144, 148], [0.3, 0.14], 500.0, 0.35, [0.0, 1.0], 0.8, 0.8, 0.6, 0.0), [1.0, 0.0, 0.0]),
    ]
}

/// One rock of the pile at the reference's 1.2 m: its stone, its size (m:
/// length along x, height, width along z), where it is dropped (x right, z
/// towards the viewer, m), its heading, tilt (its top towards the viewer)
/// and roll (its +x end up), in degrees, how far it may slide out down the
/// heap from where it is dropped before it comes to rest (m), its share of rusty weathering, its form (how deep its fracture faces
/// cut, how much taller its -x end stands than its +x end, how much
/// narrower its top) and its sheets.
#[derive(Clone, Copy)]
struct Rock {
    stone: usize,
    size: [f32; 3],
    at: [f32; 2],
    yaw: f32,
    tilt: f32,
    roll: f32,
    slide: f32,
    rust: f32,
    cut: f32,
    lean: f32,
    taper: f32,
    layers: f32,
}

#[allow(clippy::too_many_arguments)]
const fn r(stone: usize, size: [f32; 3], at: [f32; 2], yaw: f32, tilt: f32, roll: f32, slide: f32, rust: f32, cut: f32, lean: f32, taper: f32, layers: f32) -> Rock {
    Rock { stone, size, at, yaw, tilt, roll, slide, rust, cut, lean, taper, layers }
}

/// The reference pile, in the order the rocks are dropped (the big leaning
/// block first; each later one lands on the ground or on those before it).
const PILE: [Rock; 23] = [
    // The big grey schist block at the back: broad, leaning right, its top a
    // broken slope down to the right, rust down its left side.
    r(GREY, [1.45, 1.32, 0.8], [-0.1, -0.32], 6.0, 0.0, -5.0, 0.0, 0.5, 0.5, 0.35, 0.18, 6.0),
    // A block at its left foot.
    r(GREY, [0.24, 0.28, 0.22], [-0.8, -0.2], 30.0, 0.0, 0.0, 0.3, 0.2, 0.55, 0.0, 0.1, 0.0),
    // A block in front of the big one's right half that the rusty block
    // and the pale one lie on (mostly hidden behind them).
    r(GREY, [0.44, 0.36, 0.38], [0.32, -0.02], 20.0, 0.0, 0.0, 0.3, 0.3, 0.55, 0.0, 0.1, 0.0),
    // The tan block at the right.
    r(TAN, [0.34, 0.28, 0.3], [0.8, 0.0], 22.0, 6.0, 0.0, 0.3, 0.0, 0.55, 0.15, 0.15, 0.0),
    // The rusty block, lying on the hidden one and the tan one, against
    // the big block, its top sloping right.
    r(RUST, [0.74, 0.34, 0.48], [0.42, -0.06], -20.0, 14.0, -12.0, 0.3, 0.0, 0.52, 0.1, 0.1, 4.0),
    // The tan wedge between the big block and the rusty one.
    r(TAN, [0.2, 0.22, 0.17], [0.0, 0.08], 35.0, 0.0, 10.0, 0.3, 0.0, 0.55, 0.2, 0.25, 0.0),
    // The broad grey-green slab at the front left, its back up on the
    // block at the big one's foot, its top towards the viewer.
    r(GREEN, [0.9, 0.45, 0.6], [-0.44, 0.12], -14.0, 20.0, -5.0, 0.3, 0.2, 0.52, -0.15, 0.1, 5.0),
    // Blocks in front, between the slab and the pale block.
    r(BLUE, [0.24, 0.16, 0.2], [-0.12, 0.38], -20.0, 0.0, 0.0, 0.3, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(BLUE, [0.3, 0.19, 0.23], [0.0, 0.54], 10.0, 10.0, 0.0, 0.3, 0.0, 0.55, 0.1, 0.1, 0.0),
    r(GREY, [0.2, 0.16, 0.17], [-0.06, 0.38], 40.0, 0.0, 8.0, 0.2, 0.1, 0.55, 0.0, 0.15, 0.0),
    // The pale quartzite block at the front, leaning back on the rusty one,
    // iron-stained along its cracks.
    r(PALE, [0.6, 0.42, 0.4], [0.26, 0.32], -10.0, -16.0, 8.0, 0.3, 0.45, 0.52, 0.1, 0.12, 0.0),
    // A grey block at the right front.
    r(GREY, [0.22, 0.17, 0.18], [0.66, 0.26], -30.0, 0.0, 0.0, 0.3, 0.0, 0.55, 0.0, 0.1, 0.0),
    // Stones spilled round the foot.
    r(GREY, [0.15, 0.08, 0.11], [-0.9, -0.02], 20.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.1, 0.1, 0.0),
    r(TAN, [0.11, 0.06, 0.08], [-0.94, 0.2], -30.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(GREY, [0.08, 0.06, 0.07], [0.92, 0.1], 50.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(PALE, [0.16, 0.06, 0.11], [0.44, 0.54], -15.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(TAN, [0.12, 0.08, 0.09], [0.6, 0.42], 25.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.1, 0.15, 0.0),
    r(GREY, [0.07, 0.05, 0.06], [0.76, 0.36], 0.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(BLUE, [0.09, 0.06, 0.07], [-0.34, 0.52], 70.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(BLUE, [0.06, 0.05, 0.05], [0.12, 0.68], 10.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(TAN, [0.07, 0.05, 0.05], [0.88, 0.24], -40.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(GREY, [0.06, 0.04, 0.05], [-0.66, 0.38], 15.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
    r(GREEN, [0.09, 0.05, 0.07], [-0.58, 0.48], -25.0, 0.0, 0.0, 0.2, 0.0, 0.55, 0.0, 0.1, 0.0),
];

/// The heap's middle (m at the reference's size): rocks slide out from it.
const MIDDLE: [f32; 2] = [-0.1, -0.35];

/// The rocks the `count` param counts at its default: the reference's.
pub const COUNT: f32 = PILE.len() as f32;

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The stone shader's PCG hash (core.wgsl `rc_pcg`).
fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// The variant 11 vein direction for shader seed `seed` (rc_stone.wgsl
/// `rc_stone_ww`: kept off the vertical).
fn vein_dir(seed: u32) -> [f32; 3] {
    let h = pcg(seed + 97);
    let f = |x: u32| (x & 255) as f32 / 127.5 - 0.99;
    geo::normalize([f(h), f(h >> 8) * 0.45, f(h >> 16)])
}

/// The shader seed (0-255) whose vein direction is nearest `want`.
fn vein_seed(want: [f32; 3]) -> u8 {
    let want = geo::normalize(want);
    (0u32..256).max_by(|a, b| dot(vein_dir(*a), want).abs().total_cmp(&dot(vein_dir(*b), want).abs())).unwrap_or(0) as u8
}

/// A rock's form in its own frame: its half extents, its fracture faces
/// (unit-sphere normal and distance), how boxy and how soft its creases.
struct Form {
    half: [f32; 3],
    planes: Vec<([f32; 3], f32)>,
    boxy: f32,
    soft: f32,
    wave: f32,
    rough: f32,
    lean: f32,
    taper: f32,
}

impl Form {
    /// The superellipsoid's and each fracture face's radius along unit
    /// direction `d`, unsoftened: `f(r)` for each.
    fn faces(&self, d: [f32; 3], mut f: impl FnMut(f32)) {
        let p = self.boxy;
        f((d[0].abs().powf(p) + d[1].abs().powf(p) + d[2].abs().powf(p)).powf(-1.0 / p));
        for (n, off) in &self.planes {
            let c = dot(*n, d);
            if c > 0.05 {
                f(off / c);
            }
        }
    }

    /// The gap between the two nearest faces along `d` (small on a crease).
    fn crease(&self, d: [f32; 3]) -> f32 {
        let (mut a, mut b) = (f32::MAX, f32::MAX);
        self.faces(d, |x| {
            if x < a {
                b = a;
                a = x;
            } else if x < b {
                b = x;
            }
        });
        b - a
    }

    /// The radius along unit direction `d`.
    fn shape(&self, d: [f32; 3], seed: u32) -> f32 {
        let mut r = f32::MAX;
        self.faces(d, |x| r = if r == f32::MAX { x } else { smin(r, x, self.soft) });
        let q = |s: f32| [d[0] * s, d[1] * s, d[2] * s];
        // Fracture faces are never quite flat: a broad wave, a finer roughness.
        r * (1.0 + self.wave * noise::fbm3(q(1.3), seed, 3) + self.rough * noise::fbm3(q(4.2), seed ^ 0x9e37, 3) + 0.4 * self.rough * noise::fbm3(q(11.0), seed ^ 0x51ed, 2))
    }

    /// The point on the form along `d` at radius `r`, in metres: the top
    /// narrowed by the taper, the -x end raised by the lean.
    fn place(&self, d: [f32; 3], r: f32) -> [f32; 3] {
        let t = 1.0 - self.taper * (0.5 + 0.5 * d[1]);
        // (The lean raises the top only: the bed stays flat.)
        let lean = if d[1] > 0.0 { 1.0 - self.lean * d[0] } else { 1.0 };
        [d[0] * r * self.half[0] * t, d[1] * r * self.half[1] * lean, d[2] * r * self.half[2] * t]
    }
}

/// One rock `size` (length, height, width; m) in its own frame round the
/// origin: fracture faces `cut` deep (unit radii: .55 deep .. .9 shaving
/// the corners), its -x end `lean` taller, its top `taper` narrower,
/// `layers` sheets up its height stepped back (0: massive), `wear` scaling
/// its chips; `cheap` makes gravel (a few flat facets, no refinement).
#[allow(clippy::too_many_arguments)]
fn rock(size: [f32; 3], cut: f32, lean: f32, taper: f32, layers: f32, wear: f32, s: u32, density: f32, cheap: bool) -> Mesh {
    let h = |i: u32| sh::rnd(s, i, 77);
    let half = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let big = half[0].max(half[1]).max(half[2]);
    let mut planes: Vec<([f32; 3], f32)> = (0..if cheap { 9 } else { 14 })
        .map(|i| {
            let n = geo::normalize([h(10 + i) - 0.5, (h(20 + i) - 0.35) * 0.9, h(30 + i) - 0.5]);
            (n, cut + 0.2 * h(40 + i))
        })
        .collect();
    // Broken sides: four rough fracture faces round it, so it is a block.
    for (i, n) in [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]].iter().enumerate() {
        let i = i as u32;
        let j = |a: u32| (h(60 + a + 4 * i) - 0.5) * 0.5;
        planes.push((geo::normalize([n[0] + j(0), j(1) * 0.8, n[2] + j(2)]), 0.74 + 0.1 * h(80 + i)));
    }
    // The bed it lies on, and a broken top across it.
    planes.push(([0.0, -1.0, 0.0], 0.8));
    planes.push((geo::normalize([(h(6) - 0.5) * 0.8, 1.0, (h(7) - 0.5) * 0.6]), 0.74 + 0.12 * h(8)));
    let form = Form { half, planes, boxy: 2.4, soft: if cheap { 0.01 } else { 0.016 }, wave: 0.04, rough: 0.022, lean, taper };
    let mut m = sh::icosphere(match () {
        _ if cheap => 1,
        _ if big > 0.12 && density >= 0.6 => 3,
        _ if big > 0.05 => 2,
        _ => 1,
    });
    // Sheets up the height, each stepped back by its own amount.
    let fol = geo::normalize([0.25, 1.0, 0.1]);
    let spacing = if layers > 0.0 { size[1] / layers } else { 1.0 };
    let layer = |p: [f32; 3]| -> f32 { dot(p, fol) / spacing + 0.35 * noise::fbm3([p[0] * 6.0 / size[0], p[1] * 6.0 / size[0], p[2] * 6.0 / size[0]], s ^ 0x7a11, 2) };
    if !cheap && big > 0.05 {
        // Mesh detail where it shows (unit-sphere edge lengths): finer along
        // the creases (crisp breaks) and the sheets' risers.
        let world = if density < 0.6 { 0.05 } else { 0.028 };
        let crease_e = (world / big).clamp(0.035, 0.14);
        let riser_e = (world * 1.5 / big).clamp(0.05, 0.14);
        detail::refine_near(&mut m, crease_e.min(riser_e), 8, |c, e| {
            let d = geo::normalize(c);
            if d[1] < -0.55 {
                return false;
            }
            if form.crease(d) < 0.3 * e && e > crease_e {
                return true;
            }
            if layers > 0.0 && e > riser_e {
                let u = layer(form.place(d, form.shape(d, s)));
                let f = u - u.floor();
                return f > 0.86 - e * 0.6 || f < 0.02 + e * 0.3;
            }
            false
        });
    }
    if !cheap && big > 0.05 {
        // The broken faces' own small facets (a few millimetres of real
        // relief) need a mesh fine enough to carry them: about 3.5 cm at
        // High over every face that shows (not the bed underneath).
        let fine = (if density < 0.6 { 0.065 } else { 0.035 }) / big;
        detail::refine_near(&mut m, fine.clamp(0.02, 0.14), 6, |c, _| geo::normalize(c)[1] > -0.5);
    }
    let mut crease = Vec::with_capacity(m.vertices.len());
    for v in &mut m.vertices {
        let d = geo::normalize([v[0], v[1], v[2]]);
        crease.push(form.crease(d));
        let p = form.place(d, form.shape(d, s));
        *v = [p[0], p[1], p[2], 0.0, 1.0, 0.0];
    }
    if cheap {
        // Gravel: sharp-edged facets.
        sh::flat_shade(&mut m);
        sh::to_ground(&mut m);
        return m;
    }
    geo::smooth_normals(&mut m);
    let mut moved = vec![false; m.vertices.len()];
    if layers > 0.0 {
        // Each sheet recessed by its own amount, a short steep riser where
        // the next begins; the faces along the sheets left whole.
        let depth = 0.035 * size[1].min(size[2]);
        let rec = |i: f32| sh::rnd(s, (i as i32 + 1000) as u32, 333);
        for (i, v) in m.vertices.iter_mut().enumerate() {
            let side = ((1.0 - dot([v[3], v[4], v[5]], fol).abs()) * 1.6).clamp(0.0, 1.0);
            if side <= 0.0 || v[4] < -0.7 {
                continue;
            }
            let u = layer([v[0], v[1], v[2]]);
            let li = u.floor();
            let f = depth * side * (rec(li) + (rec(li + 1.0) - rec(li)) * smoothstep(0.84, 1.0, u - li));
            v[0] -= v[3] * f;
            v[1] -= v[4] * f;
            v[2] -= v[5] * f;
            moved[i] = true;
        }
        geo::smooth_normals(&mut m);
    }
    // Chips: fresh breaks on the upper creases (one on a small stone).
    let nc = ((wear * (2.0 + 3.0 * h(4))).floor() as u32).min(if big > 0.2 { 4 } else { 1 });
    if nc > 0 && big > 0.05 {
        let (lo, hi) = geo::bounds(&m);
        let n = m.vertices.len();
        let pick = |i: u32| -> [f32; 3] {
            for t in 0..48 {
                let j = ((sh::rnd(s, i * 53 + t, 91) * n as f32) as usize).min(n - 1);
                let v = m.vertices[j];
                let on_edge = crease.get(j).is_some_and(|g| *g < 0.03) || t > 40;
                if v[1] > lo[1] + (hi[1] - lo[1]) * 0.3 && on_edge {
                    return [v[0], v[1], v[2]];
                }
            }
            let v = m.vertices[0];
            [v[0], v[1], v[2]]
        };
        let chips: Vec<detail::Chip> = (0..nc)
            .map(|i| {
                let r = half[0].min(half[1]).min(half[2]) * (0.12 + 0.13 * sh::rnd(s, i, 71));
                detail::Chip { at: pick(200 + i), radius: r, depth: r * (0.2 + 0.15 * sh::rnd(s, i, 72)), fine: (r * 0.35 / density.clamp(0.25, 2.0)).max(0.003), seed: s ^ i.wrapping_mul(0x2545_f491) }
            })
            .collect();
        let mv = detail::spalls(&mut m, &chips);
        detail::merge_flags(&mut moved, &mv);
    }
    if moved.iter().any(|x| *x) {
        detail::smooth_normals_where(&mut m, &moved);
    }
    if big > 0.05 {
        // Micro relief, last, over the form's own smooth normals (the
        // chips' floors too): ridged noise (sharp creases between small
        // facets, 4-8 cm across) and a finer roughness, a few millimetres
        // deep, fading out at the creases so the breaks stay crisp.
        let amp = 0.012 * big.min(0.5) / 0.5 + 0.002;
        for (i, v) in m.vertices.iter_mut().enumerate() {
            let g = crease.get(i).copied().unwrap_or(0.1);
            let p = [v[0] * 16.0, v[1] * 16.0, v[2] * 16.0];
            let ridged = (1.0 - noise::fbm3p(p, s ^ 0x3b1d, 3, 0.5).abs() * 2.2).clamp(0.0, 1.0);
            let f = [v[0] * 45.0, v[1] * 45.0, v[2] * 45.0];
            let d = amp * (0.7 * (ridged - 0.5) + 0.3 * noise::fbm3(f, s ^ 0x77a3, 2)) * smoothstep(0.0, 0.04, g);
            v[0] += v[3] * d;
            v[1] += v[4] * d;
            v[2] += v[5] * d;
        }
        geo::smooth_normals(&mut m);
    }
    sh::to_ground(&mut m);
    m
}

/// The top of the ground and the rocks already down, on a square grid round
/// the pile: what a dropped rock lands on.
struct Field {
    n: usize,
    lo: f32,
    cell: f32,
    h: Vec<f32>,
}

impl Field {
    fn new(half: f32, n: usize) -> Self {
        Field { n, lo: -half, cell: 2.0 * half / n as f32, h: vec![0.0; n * n] }
    }

    fn index(&self, x: f32, z: f32) -> Option<usize> {
        let (i, j) = ((x - self.lo) / self.cell, (z - self.lo) / self.cell);
        if i < 0.0 || j < 0.0 || i >= self.n as f32 || j >= self.n as f32 {
            return None;
        }
        Some(j as usize * self.n + i as usize)
    }

    /// The surface's height at `x`, `z` (the ground outside the grid).
    fn at(&self, x: f32, z: f32) -> f32 {
        self.index(x, z).map_or(0.0, |k| self.h[k])
    }

    /// Raise the surface to the top of `m`'s triangles.
    fn add(&mut self, m: &Mesh) {
        for t in m.indices.as_chunks::<3>().0 {
            let p: [[f32; 3]; 3] = std::array::from_fn(|i| {
                let v = m.vertices[t[i] as usize];
                [v[0], v[1], v[2]]
            });
            let (x0, x1) = (p[0][0].min(p[1][0]).min(p[2][0]), p[0][0].max(p[1][0]).max(p[2][0]));
            let (z0, z1) = (p[0][2].min(p[1][2]).min(p[2][2]), p[0][2].max(p[1][2]).max(p[2][2]));
            let ci = |x: f32| (((x - self.lo) / self.cell).floor() as i64).clamp(0, self.n as i64 - 1) as usize;
            let det = (p[1][2] - p[2][2]) * (p[0][0] - p[2][0]) + (p[2][0] - p[1][0]) * (p[0][2] - p[2][2]);
            for j in ci(z0)..=ci(z1) {
                for i in ci(x0)..=ci(x1) {
                    let (x, z) = (self.lo + (i as f32 + 0.5) * self.cell, self.lo + (j as f32 + 0.5) * self.cell);
                    let y = if det.abs() < 1e-12 {
                        p[0][1].max(p[1][1]).max(p[2][1])
                    } else {
                        let a = ((p[1][2] - p[2][2]) * (x - p[2][0]) + (p[2][0] - p[1][0]) * (z - p[2][2])) / det;
                        let b = ((p[2][2] - p[0][2]) * (x - p[2][0]) + (p[0][0] - p[2][0]) * (z - p[2][2])) / det;
                        let c = 1.0 - a - b;
                        // Cells the triangle covers, and the cell under each
                        // of its corners (thin slivers).
                        let near = |q: [f32; 3]| (q[0] - x).abs() <= self.cell * 0.5 && (q[2] - z).abs() <= self.cell * 0.5;
                        if a >= -1e-4 && b >= -1e-4 && c >= -1e-4 {
                            a * p[0][1] + b * p[1][1] + c * p[2][1]
                        } else if let Some(q) = p.iter().find(|q| near(**q)) {
                            q[1]
                        } else {
                            continue;
                        }
                    };
                    let k = j * self.n + i;
                    if y > self.h[k] {
                        self.h[k] = y;
                    }
                }
            }
        }
    }

    /// How far `m` must be lifted to rest on the surface.
    fn lift(&self, m: &Mesh) -> f32 {
        m.vertices.iter().map(|v| self.at(v[0], v[2]) - v[1]).fold(f32::MIN, f32::max)
    }

    /// Drop `m` straight down onto the surface: lifted until it rests on
    /// it, then let settle `sink` m into it (a heap's rocks bed into what
    /// they land on). Returns how far it was lifted.
    fn drop(&self, m: &mut Mesh, sink: f32) -> f32 {
        let lift = self.lift(m) - sink;
        for v in &mut m.vertices {
            v[1] += lift;
        }
        lift
    }
}

/// Turn a rock as it lies: its +x end raised by `roll`, its top towards the
/// viewer by `tilt`, then turned by `yaw` (its +x end away from the viewer),
/// all in radians.
fn orient(m: &mut Mesh, yaw: f32, tilt: f32, roll: f32) {
    if roll != 0.0 {
        sh::rotate(m, [0.0, 0.0, 1.0], roll);
    }
    if tilt != 0.0 {
        sh::rotate(m, [1.0, 0.0, 0.0], tilt);
    }
    if yaw != 0.0 {
        sh::rotate(m, [0.0, 1.0, 0.0], yaw);
    }
}

/// The pile: `height` m tall (the reference's 1.2 m by default), `count`
/// rocks (the reference's 23; more are dropped round it, fewer drops the
/// smallest), `gravel` scaling the fine stones round its foot (0: none),
/// `rust` the share of rusty weathering on the grey rocks, `wear` the chips;
/// `colour` tints the stones (at its default each keeps its own).
#[allow(clippy::too_many_arguments)]
pub fn pile(height: f32, count: u32, colour: [f32; 3], gravel: f32, rust: f32, wear: f32, seed: u32, density: f32) -> Vec<evani_procedural::Part> {
    let k = height / REF_H;
    let mut rocks: Vec<Rock> = PILE.iter().take(count as usize).copied().collect();
    for i in PILE.len() as u32..count {
        // More rocks: dropped round the pile, smaller further out.
        let a = sh::rnd(seed, i, 21) * sh::TAU;
        let d = 0.3 + 0.75 * sh::rnd(seed, i, 22).sqrt();
        let l = sh::range(seed, i, 23, 0.08, 0.4) * (1.2 - 0.6 * d);
        let st = [GREY, GREY, RUST, PALE, GREEN, TAN, BLUE][(sh::rnd(seed, i, 24) * 7.0) as usize % 7];
        rocks.push(r(st, [l, l * sh::range(seed, i, 25, 0.5, 0.8), l * sh::range(seed, i, 26, 0.6, 0.9)], [a.cos() * d * 1.0, a.sin() * d * 0.65], sh::range(seed, i, 27, -180.0, 180.0), sh::range(seed, i, 28, -15.0, 15.0), sh::range(seed, i, 29, -15.0, 15.0), 0.3, 0.15, 0.55, 0.1, 0.1, 0.0));
    }
    // Past three dozen rocks each is drawn at the next tier's detail.
    let density = if count > 36 { density.min(0.5) } else { density };
    let mut field = Field::new(1.6 * k, 320);
    let mut parts: Vec<Mesh> = vec![Mesh::default(); STONES];
    for (i, rk) in rocks.iter().enumerate() {
        let i = i as u32;
        let s = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
        let size = [rk.size[0] * k, rk.size[1] * k, rk.size[2] * k];
        let mut m = rock(size, rk.cut, rk.lean, rk.taper, rk.layers, wear, s, density, false);
        orient(&mut m, rk.yaw.to_radians(), rk.tilt.to_radians(), rk.roll.to_radians());
        let (lo, _) = geo::bounds(&m);
        geo::transform(&mut m, [1.0; 3], [rk.at[0] * k, -lo[1], rk.at[1] * k]);
        // Dropped where it is meant to lie, it may land on a steep face of
        // the rocks behind it: it slides out from the heap's middle, as a
        // rock rolls down a pile, while what it rests on is steeper than
        // the heap's angle of repose (about 35 degrees), so it comes to rest
        // on something that holds it.
        let out = geo::normalize([rk.at[0] - MIDDLE[0], 0.0, rk.at[1] - MIDDLE[1] + 1e-3]);
        let step = 0.01 * k;
        let mut lift = field.lift(&m);
        for _ in 0..(rk.slide / 0.01).round() as u32 {
            geo::transform(&mut m, [1.0; 3], [out[0] * step, 0.0, out[2] * step]);
            let next = field.lift(&m);
            if lift - next < 0.7 * step {
                geo::transform(&mut m, [1.0; 3], [-out[0] * step, 0.0, -out[2] * step]);
                break;
            }
            lift = next;
        }
        // Settled into what it lands on: a rock on the ground beds in by an
        // eighth of its height (no daylight under its rounded foot), one on
        // other rocks by a twentieth.
        let on_ground = field.lift(&m) < 0.02 * k;
        field.drop(&mut m, if on_ground { 0.12 * size[1] } else { 0.05 * size[1].min(size[2]) });
        // What is under the ground is never seen: cut flat at it.
        for v in &mut m.vertices {
            v[1] = v[1].max(0.0);
        }
        field.add(&m);
        if rk.rust > 0.0 && rk.stone != RUST {
            // Rusty weathering in patches across the rock: the noise below
            // spreads about a quarter either side of zero, so the threshold
            // sets the share (the rock's own share times the rust param;
            // 0.25 rusts about a fifth of it); the rusty stone's own grey-
            // brown mottle carries the patch's edge into the grey.
            let at = 0.42 - rust.clamp(0.0, 1.0) * rk.rust;
            let split = detail::split_by(&m, 2, |c, _| {
                let q = [c[0] / k, c[1] / k, c[2] / k];
                let n = 0.6 * noise::fbm3([q[0] * 2.2, q[1] * 1.6, q[2] * 2.2], seed ^ 0xa511, 3) + 0.5 * noise::fbm3([q[0] * 10.0, q[1] * 10.0, q[2] * 10.0], seed ^ 0x5a17, 2);
                usize::from(n > at)
            });
            let mut it = split.into_iter();
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                geo::merge(&mut parts[rk.stone], &a);
                geo::merge(&mut parts[RUST], &b);
            }
        } else {
            geo::merge(&mut parts[rk.stone], &m);
        }
    }
    // Gravel: fine angular stones spilled round the foot, mostly in front,
    // each dropped onto the ground or the rocks' feet.
    let n = (70.0 * gravel).round() as u32;
    for i in 0..n {
        let s = seed.wrapping_mul(0x2c1b_3c6d) ^ i.wrapping_mul(0x297a_2d39);
        let q = |j: u32| sh::rnd(s, i, j);
        let a = if q(8) < 0.25 { q(1) * sh::TAU } else { (-0.1 + 1.2 * q(1)) * std::f32::consts::PI };
        let out = 0.7 + 0.4 * q(2) * q(2);
        let (x, z) = (a.cos() * 0.98 * out, -0.05 + a.sin() * 0.62 * out);
        let len = (0.012 + 0.03 * q(3) * q(3)) * k;
        let mut m = rock([len, len * (0.5 + 0.3 * q(4)), len * (0.6 + 0.3 * q(5))], 0.7, 0.0, 0.1, 0.0, 0.0, s, density, true);
        orient(&mut m, q(7) * sh::TAU, (q(6) - 0.5) * 0.6, 0.0);
        let (lo, _) = geo::bounds(&m);
        geo::transform(&mut m, [1.0; 3], [x * k, -lo[1], z * k]);
        field.drop(&mut m, len * 0.2);
        for v in &mut m.vertices {
            v[1] = v[1].max(0.0);
        }
        let st = [GREY, GREY, BLUE, TAN, PALE][(q(9) * 5.0) as usize % 5];
        geo::merge(&mut parts[st], &m);
    }
    // The heap stands exactly `height` tall: the rocks' forms and the way
    // they land set its top, so the whole is scaled to it.
    let top = parts.iter().filter(|m| !m.indices.is_empty()).map(|m| geo::bounds(m).1[1]).fold(0.0f32, f32::max);
    if top > 0.0 {
        let f = height / top;
        for m in &mut parts {
            geo::transform(m, [f; 3], [0.0; 3]);
        }
    }
    let tint = (0..3).any(|c| (colour[c] - COLOR[c]).abs() > 1e-3);
    parts
        .into_iter()
        .zip(stones())
        .enumerate()
        .filter(|(_, (m, _))| !m.indices.is_empty())
        .map(|(i, (m, (sf, vein)))| {
            let mut sf = sf.seed(seed.wrapping_add(i as u32 * 0x2f6b));
            sf.block[23] = vein_seed(vein);
            if tint {
                let a = sf.colour(0);
                sf = sf.tint([a[0] * colour[0] / COLOR[0], a[1] * colour[1] / COLOR[1], a[2] * colour[2] / COLOR[2]]);
            }
            evani_procedural::Part::surfaced(m, sf)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tris(p: &[evani_procedural::Part]) -> usize {
        p.iter().map(|x| x.mesh.indices.len() / 3).sum()
    }

    #[test]
    fn the_pile_rests_on_the_ground_inside_its_budget() {
        let t0 = std::time::Instant::now();
        let g = pile(1.2, PILE.len() as u32, COLOR, 1.0, 0.5, 1.0, 1, 1.0);
        let ms = t0.elapsed().as_millis();
        assert_eq!(g.len(), STONES, "one part per stone");
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &g {
            let (l, h) = geo::bounds(&p.mesh);
            for c in 0..3 {
                lo[c] = lo[c].min(l[c]);
                hi[c] = hi[c].max(h[c]);
            }
            assert!(p.mesh.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())));
        }
        assert!(lo[1].abs() < 1e-4, "on the ground: {lo:?}");
        assert!(hi[1] > 1.0 && hi[1] < 1.45, "the pile's height: {hi:?}");
        assert!(hi[0] - lo[0] > 1.6, "a broad pile: {lo:?} {hi:?}");
        let (high, low) = (tris(&g), tris(&pile(1.2, PILE.len() as u32, COLOR, 1.0, 0.5, 1.0, 1, 0.5)));
        println!("rock pile: {high} triangles at High, {low} at Low ({ms} ms at High), top {hi:?}, foot {lo:?}");
        assert!(high < 120_000 && low <= high, "{high} / {low}");
        assert!(tris(&pile(1.2, 80, COLOR, 2.0, 1.0, 2.0, 3, 1.0)) < 300_000);
    }

    #[test]
    fn every_rock_rests_on_something() {
        // Each dropped rock touches the surface under it: its lift leaves
        // it settled, never hovering above the ground or the rocks.
        let mut f = Field::new(1.0, 64);
        let mut a = rock([0.5, 0.4, 0.4], 0.66, 0.0, 0.1, 0.0, 0.0, 7, 0.5, false);
        f.drop(&mut a, 0.0);
        assert!(geo::bounds(&a).0[1].abs() < 1e-4, "on the ground");
        f.add(&a);
        let mut b = rock([0.3, 0.2, 0.25], 0.66, 0.0, 0.1, 0.0, 0.0, 9, 0.5, false);
        geo::transform(&mut b, [1.0; 3], [0.05, 0.0, 0.0]);
        let lift = f.drop(&mut b, 0.0);
        assert!(lift > 0.2, "it landed on the first rock: lifted {lift}");
        let gap = b.vertices.iter().map(|v| v[1] - f.at(v[0], v[2])).fold(f32::MAX, f32::min);
        assert!(gap.abs() < 1e-3, "touching, not hovering: {gap}");
    }

    #[test]
    fn the_vein_seed_lines_the_paint_up() {
        for (_, want) in stones() {
            let d = vein_dir(vein_seed(want) as u32);
            assert!(dot(d, geo::normalize(want)).abs() > 0.85, "{want:?}: {d:?}");
        }
    }
}
