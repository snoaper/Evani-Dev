//! Jagged rock formations (rocks-jagged-rock-formations; the Asset Studio
//! jagged rock formations session, column 24, 2026-11-07): an outcrop of
//! foliated rock (a grey schist with rusty iron-stained patches and pale
//! quartz streaks) broken into tilted blades and spires that share one
//! foliation, a tall peaked main spire leaning to one side, lower spires
//! behind it, blocky broken slabs at its foot and an apron of fallen rubble
//! and gravel in front.
//!
//! Each blade or block is one welded mesh: a slightly boxy form (a
//! superellipsoid) cut by fracture planes kept crisp where two faces meet
//! (refined along the creases), a peaked top (two or three steep planes) or
//! a sloping bedding top, faces that are never quite flat, then real
//! geometry for what foliated rock does when it breaks: its sheets step
//! back one by one along the foliation (ridges running up the blade's
//! length), and fresh chips are broken out of its upper edges
//! ([`detail::spalls`]). The blades are leaned and turned together (one
//! foliation for the whole outcrop), sunk into the ground and cut flat at
//! it, so the formation stands on the ground as one rooted mass.
//!
//! The paint is the stone recipe's variant 11 (rc_stone.wgsl: world space,
//! the same in any game that lists `stone`) in three stones: the grey
//! schist, its rusty iron-stained weathering and a pale quartz-rich grey.
//! The formation's triangles are sorted into them by a broad 3D noise, so
//! the rust lies in patches across neighbouring blades as weathering does.
//! The paint's streaks and veins run with the blades' length (the shader
//! seed is picked so its vein direction crosses the blades: [`vein_seed`]).
//!
//! The form and the carving reuse the medium rocks' method (column 23's
//! fracture-plane rock, itself from the pebbles' column 7), adapted to tall
//! foliated blades; written fresh here.

use evani_procedural::detail;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::noise;
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};

/// The library colour param's default: at it the formation keeps its own
/// grey, rust and pale stones.
pub const COLOR: [f32; 3] = [0.27, 0.25, 0.23];

/// The reference formation's height (m): the layout below is at it.
const REF_H: f32 = 3.5;

/// One blade or block of the formation at the reference's 3.5 m: its place
/// (x right, z towards the viewer, m), the height its top stands at, its
/// width along the blade's broad face and its thickness (m), its lean
/// (degrees, the top towards the broad face's +x) and its own turn off the
/// outcrop's (degrees), how peaked its top is (0 a sloping bedding top, 1 a
/// sharp point) and how much narrower its top is than its foot.
#[derive(Clone, Copy)]
struct Blade {
    x: f32,
    z: f32,
    h: f32,
    w: f32,
    t: f32,
    lean: f32,
    yaw: f32,
    peak: f32,
    taper: f32,
}

#[allow(clippy::too_many_arguments)]
const fn b(x: f32, z: f32, h: f32, w: f32, t: f32, lean: f32, yaw: f32, peak: f32, taper: f32) -> Blade {
    Blade { x, z, h, w, t, lean, yaw, peak, taper }
}

/// The spires, tallest first in the order the `spires` param keeps them.
const SPIRES: [Blade; 6] = [
    // The main spire: tall, broad, leaning right.
    b(0.7, -0.35, 3.5, 1.9, 1.4, 16.0, 0.0, 1.0, 0.28),
    // The blade in front of it on the left, with the pale streaks.
    b(-0.05, 0.1, 2.15, 1.1, 0.9, 22.0, -6.0, 1.0, 0.28),
    // The tallest of the spires on the left.
    b(-0.6, -0.95, 2.45, 0.9, 0.8, 8.0, 5.0, 1.0, 0.28),
    // The blade behind the main spire on the right.
    b(1.85, -0.9, 2.6, 1.05, 0.85, 10.0, 8.0, 1.0, 0.25),
    // A narrow spire right behind the main one.
    b(1.25, -1.15, 2.9, 0.8, 0.75, 14.0, 0.0, 1.0, 0.28),
    // A second spire on the left.
    b(-1.0, -0.7, 2.0, 0.7, 0.65, 12.0, -10.0, 1.0, 0.28),
];

/// The blocks and low blades round the spires' foot (always there).
const BLOCKS: [Blade; 15] = [
    // The core: a broad mass that roots the spires together.
    b(0.2, -0.45, 1.7, 4.2, 2.2, 4.0, 0.0, 0.1, 0.1),
    // Fillers in the slots between the spires.
    b(0.25, -0.8, 2.0, 1.2, 1.0, 10.0, 0.0, 0.6, 0.2),
    b(1.45, -0.45, 1.8, 1.0, 1.0, 12.0, 10.0, 0.5, 0.2),
    // The left shoulder's inner block, higher.
    b(-0.95, -0.15, 1.75, 1.1, 1.1, 14.0, 4.0, 0.4, 0.15),
    // A small spire between the left ones.
    b(-0.25, -0.6, 1.85, 0.65, 0.5, 18.0, 0.0, 1.0, 0.35),
    // The shoulder on the left: a big block, its top a sloping bed.
    b(-1.55, -0.3, 1.55, 1.6, 1.4, 8.0, 10.0, 0.2, 0.1),
    // The big block on the right, its top sloping.
    b(1.95, 0.25, 2.0, 1.4, 1.1, -6.0, 15.0, 0.3, 0.2),
    // The broad slab in front of the main spire, its top a tilted bed.
    b(0.75, 0.95, 1.15, 1.8, 1.0, 10.0, -15.0, 0.1, 0.1),
    // Two short blades leaning right at the front left.
    b(-1.35, 0.85, 0.95, 0.7, 0.35, 24.0, 0.0, 0.8, 0.3),
    b(-0.95, 0.95, 0.85, 0.6, 0.32, 26.0, 5.0, 0.8, 0.3),
    // Low blocks at the far left and the far right.
    b(-2.55, 0.65, 0.65, 0.9, 0.7, 4.0, 20.0, 0.2, 0.1),
    b(-2.2, 0.15, 0.8, 0.8, 0.6, 8.0, -10.0, 0.4, 0.2),
    b(2.85, 0.65, 0.6, 0.7, 0.6, 0.0, 30.0, 0.2, 0.1),
    // A fallen block at the front and one between the right block and the
    // slab.
    b(-0.35, 1.45, 0.5, 0.75, 0.55, 8.0, 20.0, 0.2, 0.1),
    b(1.6, 1.0, 0.8, 0.8, 0.6, 12.0, 0.0, 0.3, 0.2),
];

/// Slabs fallen at the front, lying tilted: place x and z, length, width
/// and thickness (m at the reference's 3.5 m), heading and how far up from
/// lying flat (degrees).
const SLABS: [(f32, f32, f32, f32, f32, f32, f32); 4] = [
    (-0.55, 1.35, 0.95, 0.7, 0.28, 20.0, 18.0),
    (0.3, 1.6, 0.8, 0.55, 0.25, -30.0, 12.0),
    (-1.9, 1.05, 0.85, 0.6, 0.3, 50.0, 22.0),
    (1.15, 1.55, 0.7, 0.5, 0.24, 75.0, 15.0),
];

/// The outcrop's own turn (degrees): its blades' broad faces look to the
/// viewer's front left.
const OUTCROP_YAW: f32 = -28.0;

/// The blades' foliation in a blade's own frame (x across its broad face, y
/// up its length, z through its thickness): sheets standing along its
/// length, a little turned towards its thickness, so their stepped edges
/// run up the broad faces and the sides.
const FOLIATION: [f32; 3] = [1.0, 0.0, 0.45];

/// sRGB bytes into colour `k` of a block.
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A stone in the recipe's variant 11 (column 23's medium rocks' helper),
/// read as foliated schist at an outcrop's scale: base `a`, dark grains of
/// `b`, pale grains, cloud and veins of `c`; `dark` and `pale` the grains'
/// shares (0-1), `grain` their scale (per m, 100-1000), `cloud` the mottle
/// towards `c` (stretched along the foliation), `veins` their width (0:
/// none) `per_m` apart (wide and close, they are the laminae's pale bands),
/// `relief` the grain relief, `rough` the roughness, `stains` (number 8) and
/// `foliation` (number 9, the streaks along the foliation's planes).
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

/// The three stones: the grey schist, its rusty weathering, a pale
/// quartz-rich grey; the laminae as close pale bands along the foliation.
fn stones() -> [Surface; 3] {
    [
        stone([100, 97, 92], [60, 58, 55], [138, 134, 126], [0.15, 0.06], 100.0, 0.85, [0.55, 18.0], 0.9, 0.8, 0.8, 1.0),
        stone([106, 97, 88], [62, 57, 52], [142, 122, 98], [0.12, 0.04], 100.0, 0.85, [0.45, 16.0], 0.9, 0.84, 0.8, 1.0),
        stone([122, 119, 113], [78, 76, 73], [176, 172, 164], [0.12, 0.1], 100.0, 0.8, [0.6, 20.0], 0.8, 0.78, 0.6, 1.0),
    ]
}

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

/// The shader seed (0-255) whose vein direction is nearest `want` (world
/// space), so the paint's veins and streaks line up with the blades.
fn vein_seed(want: [f32; 3]) -> u8 {
    let want = geo::normalize(want);
    (0u32..256).max_by(|a, b| dot(vein_dir(*a), want).abs().total_cmp(&dot(vein_dir(*b), want).abs())).unwrap_or(0) as u8
}

/// Turn `p` the way a blade is turned: its top leaned by `lean` (radians)
/// towards its +x, then the whole turned by `yaw` about the vertical.
fn turn(p: [f32; 3], lean: f32, yaw: f32) -> [f32; 3] {
    let (sl, cl) = lean.sin_cos();
    let q = [p[0] * cl + p[1] * sl, -p[0] * sl + p[1] * cl, p[2]];
    let (sy, cy) = yaw.sin_cos();
    [q[0] * cy + q[2] * sy, q[1], -q[0] * sy + q[2] * cy]
}

/// A blade's form in its own frame: its half extents, its fracture faces
/// (unit-sphere normal and distance), how boxy and how soft its creases.
struct Form {
    half: [f32; 3],
    planes: Vec<([f32; 3], f32)>,
    boxy: f32,
    soft: f32,
    wave: f32,
    rough: f32,
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

    /// The point on the form along `d` at radius `r`, in metres (the top
    /// narrowed by the taper).
    fn place(&self, d: [f32; 3], r: f32) -> [f32; 3] {
        let t = 1.0 - self.taper * (0.5 + 0.5 * d[1]);
        [d[0] * r * self.half[0] * t, d[1] * r * self.half[1], d[2] * r * self.half[2] * t]
    }
}

/// One blade, `len` long up its own y, `wide` across and `thick` through,
/// in its own frame round the origin: `peak` 0 a sloping bedding top .. 1 a
/// point, `taper` its top's narrowing; its foliation's sheets `spacing` m
/// apart, stepped up to `step` m; `wear` scales its chips, `cheap` makes a
/// light rubble stone (no refinement, no steps, no chips: 1 an icosphere
/// of 320 faces, 2 of 20, 3 of 80; the scree slopes' stones use them all).
#[allow(clippy::too_many_arguments)]
pub(crate) fn blade(len: f32, wide: f32, thick: f32, peak: f32, taper: f32, spacing: f32, step: f32, wear: f32, s: u32, density: f32, cheap: u32) -> Mesh {
    let h = |i: u32| sh::rnd(s, i, 77);
    let half = [wide * 0.5, len * 0.5, thick * 0.5];
    let mut planes: Vec<([f32; 3], f32)> = (0..if cheap > 0 { 11 } else { 13 })
        .map(|i| {
            let n = geo::normalize([h(10 + i) - 0.5, (h(20 + i) - 0.5) * 0.7, h(30 + i) - 0.5]);
            (n, 0.58 + 0.2 * h(40 + i))
        })
        .collect();
    // The bed it stands on.
    planes.push(([0.0, -1.0, 0.0], 0.85));
    if peak > 0.5 {
        // A point: two steep planes across the broad face, a third through
        // its thickness, off centre.
        let k = 0.68 + 0.15 * h(5);
        planes.push((geo::normalize([1.0, 0.75 + 0.3 * h(6), (h(7) - 0.5) * 0.4]), k + 0.08));
        planes.push((geo::normalize([-1.0, 0.65 + 0.3 * h(8), (h(9) - 0.5) * 0.4]), k));
        planes.push((geo::normalize([(h(11) - 0.5) * 0.5, 0.8, if h(12) < 0.5 { 1.0 } else { -1.0 }]), k + 0.1));
        // A broken top: two more steep cuts high up.
        for j in 0..2u32 {
            planes.push((geo::normalize([h(50 + j) - 0.5, 0.6 + 0.4 * h(52 + j), h(54 + j) - 0.5]), k + 0.04 + 0.1 * h(56 + j)));
        }
    } else {
        // A bedding top, sloping.
        planes.push((geo::normalize([(h(6) - 0.5) * 0.7, 1.0, (h(7) - 0.5) * 0.5]), 0.72 + 0.12 * h(8)));
    }
    let form = Form { half, planes, boxy: 3.0, soft: 0.012, wave: 0.035, rough: 0.018, taper };
    let fol = geo::normalize(FOLIATION);
    let layer = |p: [f32; 3]| -> f32 { dot(p, fol) / spacing + 0.3 * noise::fbm3([p[0] * 1.7, p[1] * 0.6, p[2] * 1.7], s ^ 0x7a11, 2) };
    let big = half[0].max(half[1]).max(half[2]);
    // The base mesh: finer for the big blades at High, coarse for rubble
    // and an icosahedron for the gravel.
    let mut m = sh::icosphere(match cheap {
        0 if density >= 0.6 && big > 0.45 => 3,
        0 | 1 => 2,
        3 => 1,
        _ => 0,
    });
    if cheap == 0 {
        // Mesh detail where it shows (unit-sphere edge lengths): finer along
        // the creases (crisp breaks) and the sheets' risers.
        let world = if density < 0.6 { 0.32 } else { 0.18 };
        let crease_e = (world / big).clamp(0.03, 0.2);
        let riser_e = (world * 1.4 / big).clamp(0.035, 0.12);
        detail::refine_near(&mut m, crease_e.min(riser_e), 8, |c, e| {
            let d = geo::normalize(c);
            if d[1] < -0.6 {
                return false;
            }
            if form.crease(d) < 0.3 * e && e > crease_e {
                return true;
            }
            if step > 0.0 && e > riser_e {
                let u = layer(form.place(d, form.shape(d, s)));
                let f = u - u.floor();
                return f > 0.86 - e * 0.5 || f < 0.02 + e * 0.3;
            }
            false
        });
    }
    let mut crease = Vec::with_capacity(m.vertices.len());
    for v in &mut m.vertices {
        let d = geo::normalize([v[0], v[1], v[2]]);
        crease.push(form.crease(d));
        let p = form.place(d, form.shape(d, s));
        *v = [p[0], p[1], p[2], 0.0, 1.0, 0.0];
    }
    if cheap > 0 {
        // Rubble: sharp-edged facets.
        sh::flat_shade(&mut m);
        return m;
    }
    geo::smooth_normals(&mut m);
    let mut moved = vec![false; m.vertices.len()];
    if step > 0.0 {
        // Each sheet recessed by its own amount, a short steep riser where
        // the next begins; faces along the foliation itself left whole.
        // Each sheet is broken across by joints some 0.7 m apart (a staggered
        // pattern up the blade), so its recess changes block by block.
        let joint = spacing / 0.3 * 0.7;
        let rec = |i: f32, y: f32| sh::rnd(s, ((i as i32 + 1000) as u32).wrapping_mul(97) ^ ((y / joint + 0.37 * i).floor() as i32 + 500) as u32, 333);
        for (i, v) in m.vertices.iter_mut().enumerate() {
            let side = ((1.0 - dot([v[3], v[4], v[5]], fol).abs()) * 1.6).clamp(0.0, 1.0);
            // Not on the bed underneath (it is in the ground).
            if side <= 0.0 || v[4] < -0.7 {
                continue;
            }
            let u = layer([v[0], v[1], v[2]]);
            let li = u.floor();
            let f = step * side * (rec(li, v[1]) + (rec(li + 1.0, v[1]) - rec(li, v[1])) * smoothstep(0.84, 1.0, u - li));
            v[0] -= v[3] * f;
            v[1] -= v[4] * f;
            v[2] -= v[5] * f;
            moved[i] = true;
        }
        geo::smooth_normals(&mut m);
    }
    // Chips: fresh breaks on the upper creases.
    // (Small blocks take one chip at most: their chips cost more mesh than
    // they show.)
    let nc = ((wear * (2.0 + 3.0 * h(4))).floor() as u32).min(if big > 0.6 { 3 } else { 1 });
    if nc > 0 {
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
        let unit = spacing / 0.3;
        let chips: Vec<detail::Chip> = (0..nc)
            .map(|i| {
                let r = unit * (0.06 + 0.1 * sh::rnd(s, i, 71));
                detail::Chip { at: pick(200 + i), radius: r, depth: r * (0.3 + 0.2 * sh::rnd(s, i, 72)), fine: (r * 0.6 / density.clamp(0.25, 2.0)).max(0.004), seed: s ^ i.wrapping_mul(0x2545_f491) }
            })
            .collect();
        let mv = detail::spalls(&mut m, &chips);
        detail::merge_flags(&mut moved, &mv);
    }
    if moved.iter().any(|x| *x) {
        detail::smooth_normals_where(&mut m, &moved);
    }
    m
}

/// Lean, turn and set a blade into the ground so its top stands at `top`:
/// sunk until its highest foot corner is under the ground, then cut flat
/// at the ground (what is under it is never seen).
fn stand(m: &mut Mesh, lean: f32, yaw: f32, at: [f32; 2], top: f32) {
    for v in &mut m.vertices {
        let p = turn([v[0], v[1], v[2]], lean, yaw);
        let n = turn([v[3], v[4], v[5]], lean, yaw);
        *v = [p[0], p[1], p[2], n[0], n[1], n[2]];
    }
    let (_, hi) = geo::bounds(m);
    for v in &mut m.vertices {
        v[0] += at[0];
        v[1] = (v[1] + top - hi[1]).max(0.0);
        v[2] += at[1];
    }
}

/// The formation: its tallest spire `height` m, `spires` spires (6 as the
/// reference; more are added round the main one), `rubble` scaling the
/// fallen stones and gravel in front (0: none), `rust` the share of the
/// iron-stained weathering (0-1), `wear` the chips; `colour` tints the
/// stones (at its default each keeps its own).
#[allow(clippy::too_many_arguments)]
pub fn formation(height: f32, spires: u32, colour: [f32; 3], rubble: f32, rust: f32, wear: f32, seed: u32, density: f32) -> Vec<evani_procedural::Part> {
    let k = height / REF_H;
    let mut blades: Vec<Blade> = SPIRES.iter().take(spires as usize).copied().collect();
    for i in SPIRES.len() as u32..spires {
        // More spires: round the main one, lower the further out.
        let a = sh::rnd(seed, i, 21) * sh::TAU;
        let r = 0.6 + 1.6 * sh::rnd(seed, i, 22);
        blades.push(b(0.4 + a.cos() * r, -0.4 + a.sin() * r * 0.6, sh::range(seed, i, 23, 1.4, 2.6), sh::range(seed, i, 24, 0.5, 1.0), sh::range(seed, i, 25, 0.3, 0.5), sh::range(seed, i, 26, 6.0, 22.0), sh::range(seed, i, 27, -12.0, 12.0), 1.0, 0.5));
    }
    // A shoulder beside each spire: a shorter, thicker slab of the same
    // rock overlapping its side, so the spire steps down in ledges.
    let comp: Vec<Blade> = blades
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let i = i as u32 + 40;
            let side = if sh::rnd(seed, i, 31) < 0.5 { -1.0 } else { 1.0 };
            let yaw = (OUTCROP_YAW + p.yaw).to_radians();
            let (o, d) = (side * p.w * sh::range(seed, i, 32, 0.3, 0.45), sh::range(seed, i, 33, -0.2, 0.2));
            b(p.x + yaw.cos() * o + yaw.sin() * d, p.z - yaw.sin() * o + yaw.cos() * d, p.h * sh::range(seed, i, 34, 0.5, 0.72), p.w * 0.65, p.t * 1.15, p.lean + sh::range(seed, i, 35, -5.0, 5.0), p.yaw + sh::range(seed, i, 36, -8.0, 8.0), 0.6, 0.2)
        })
        .collect();
    blades.extend(comp);
    blades.extend(BLOCKS);
    // Past the reference's spires each blade is drawn at the next tier's
    // detail (a wide range of crags stays inside the budget).
    let density = if spires > SPIRES.len() as u32 + 2 { density.min(0.5) } else { density };
    // The sheets as ledges 30 cm apart, stepped up to 11 cm (the paint
    // carries the fine laminae inside them).
    let spacing = 0.3 * k;
    let step = 0.11 * k;
    let mut all = Mesh::default();
    for (i, bl) in blades.iter().enumerate() {
        let i = i as u32;
        let s = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
        let lean = bl.lean.to_radians();
        // Long enough that, leaned and sunk, its foot is in the ground.
        let len = (bl.h * 1.1 + bl.w * lean.sin().abs()) / lean.cos().max(0.5);
        let mut m = blade(len * k, bl.w * k, bl.t * k, bl.peak, bl.taper, spacing, step, wear, s, density, 0);
        stand(&mut m, lean, (OUTCROP_YAW + bl.yaw).to_radians(), [bl.x * k, bl.z * k], bl.h * k);
        geo::merge(&mut all, &m);
    }
    for (i, &(x, z, l, w, t, yaw, up)) in SLABS.iter().enumerate() {
        let i = i as u32 + 500;
        let s = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
        let mut m = blade(l * k, w * k, t * k, 0.0, 0.0, spacing, step * 0.6, wear, s, density, 0);
        // Laid down: its length along the ground, raised `up` at one end.
        stand(&mut m, (90.0 - up).to_radians(), (OUTCROP_YAW + yaw).to_radians(), [x * k, z * k], (l * up.to_radians().sin() * 0.55 + t * 0.6) * k);
        geo::merge(&mut all, &m);
    }
    // The fallen rubble in front and round the foot, and finer gravel among
    // it: light angular stones, a quarter sunk.
    let rocks = (60.0 * rubble).round() as u32;
    let gravel = (170.0 * rubble).round() as u32;
    for i in 0..rocks + gravel {
        let fine = i >= rocks;
        let s = seed.wrapping_mul(0x2c1b_3c6d) ^ i.wrapping_mul(0x297a_2d39);
        let r = |j: u32| sh::rnd(s, i, j);
        // An arc round the front of the foot, thickest at the front left.
        // Fines all round the foot, the stones mostly in front.
        let a = if fine && r(8) < 0.3 { r(1) * sh::TAU } else { (-0.12 + 1.24 * r(1)) * std::f32::consts::PI };
        let out = if fine { 0.6 + 0.6 * r(2) } else { 0.7 + 0.45 * r(2) };
        let (x, z) = (-0.3 + a.cos() * 2.9 * out, 0.1 + a.sin() * 1.45 * out);
        let len = if fine { 0.035 + 0.065 * r(3) } else { 0.12 + 0.4 * r(3) * r(3) };
        let mut m = blade(len * k, len * k * (0.55 + 0.35 * r(4)), len * k * (0.3 + 0.25 * r(5)), 0.0, 0.0, spacing, 0.0, 0.0, s, density, if fine { 2 } else { 1 });
        sh::to_ground(&mut m);
        let tilt = (r(6) - 0.5) * 0.7;
        let mut p = sh::place(&m, 1.0, r(7) * sh::TAU, tilt, [x * k, -len * k * 0.25, z * k]);
        for v in &mut p.vertices {
            v[1] = v[1].max(0.0);
        }
        geo::merge(&mut all, &p);
    }
    // Sorted into the three stones by a broad noise: rust in patches across
    // neighbouring blades, a pale quartz-rich grey in a few.
    let rust_at = 0.66 - 0.6 * rust;
    let parts = detail::split_by(&all, 3, |c, _| {
        let q = [c[0] / k, c[1] / k, c[2] / k];
        let r = 0.7 * noise::fbm3([q[0] * 1.6, q[1] * 1.2, q[2] * 1.6], seed ^ 0xa511, 3) + 0.45 * noise::fbm3([q[0] * 5.0, q[1] * 5.0, q[2] * 5.0], seed ^ 0x5a17, 2);
        if r > rust_at {
            return 1;
        }
        let p = noise::fbm3([q[0] * 0.8 + 7.1, q[1] * 0.6, q[2] * 0.8], seed ^ 0x3c3c, 3);
        if p > 0.38 { 2 } else { 0 }
    });
    // The paint's veins cross the blades (their streaks run up them).
    let across = turn(geo::normalize(FOLIATION), 15f32.to_radians(), OUTCROP_YAW.to_radians());
    let vein = vein_seed(across);
    let tint = (0..3).any(|c| (colour[c] - COLOR[c]).abs() > 1e-3);
    parts
        .into_iter()
        .zip(stones())
        .enumerate()
        .filter(|(_, (m, _))| !m.indices.is_empty())
        .map(|(i, (m, sf))| {
            let mut sf = sf.seed(seed.wrapping_add(i as u32 * 0x2f6b));
            sf.block[23] = vein;
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
    fn the_formation_stands_on_the_ground_inside_its_budget() {
        let t0 = std::time::Instant::now();
        let g = formation(3.5, 6, COLOR, 1.0, 0.5, 1.0, 1, 1.0);
        let ms = t0.elapsed().as_millis();
        assert_eq!(g.len(), 3, "grey, rust and pale stone");
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
        assert!(hi[1] > 3.3 && hi[1] < 3.6, "the main spire's height: {hi:?}");
        assert!(hi[0] - lo[0] > 4.5, "a broad outcrop: {lo:?} {hi:?}");
        let (high, low) = (tris(&g), tris(&formation(3.5, 6, COLOR, 1.0, 0.5, 1.0, 1, 0.5)));
        println!("jagged rock formation: {high} triangles at High, {low} at Low ({ms} ms at High)");
        assert!(high < 150_000 && low <= high, "{high} / {low}");
        assert!(tris(&formation(3.5, 20, COLOR, 2.0, 1.0, 2.0, 3, 1.0)) < 300_000);
    }

    #[test]
    fn the_vein_seed_crosses_the_blades() {
        let across = turn(geo::normalize(FOLIATION), 15f32.to_radians(), OUTCROP_YAW.to_radians());
        let d = vein_dir(vein_seed(across) as u32);
        assert!(dot(d, across).abs() > 0.9, "{across:?}: {d:?}");
    }
}
