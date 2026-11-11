//! Pebbles (rocks-pebbles; the Asset Studio pebbles session, 2026-11-05,
//! best.png: a studio group of ten water-worn stones of mixed rock, from a
//! 5 cm pebble to a 28 cm cobble). Each stone is one welded mesh: a rounded,
//! slightly boxy form (a superellipsoid) cut by a few planes (the faces of
//! the block it wore down from: softened on river-worn stones, crisp breaks
//! on the angular ones, their meeting refined), an egg's taper and one
//! flatter side, gentle bumps from seeded noise, then real wear carved into
//! it: shallow rounded dents (smooth hollows a river leaves), a few fresh
//! chips (flat fracture facets, [`detail::spalls`]) and on the angular
//! cobble a ledge, one fracture face stepped down where a slab came off.
//! Each rock is one part in the stone recipe (rc_stone.wgsl: world space in
//! 3D, so it works in any game that lists the stone recipe): the water-worn
//! variant 11 (sandy 1-3 mm specks, a cloudy tone, stains, veins, a soft
//! relief) or, for the granites, the crystalline variant 9 (interlocking
//! crystals), each rock its own colours and shares.
//!
//! The first ten stones lie as the reference's group (scaled by `size`);
//! past them each is set down as near the middle as it fits without passing
//! into another, so a dozen read as a pile and two hundred as a scatter.

use evani_procedural::detail;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::noise;
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};

/// The library colour param's default: at it every rock keeps its own tone.
pub const COLOR: [f32; 3] = [0.36, 0.33, 0.3];

/// A rock type: its stone, how flat, angular and boxy it wears, how many
/// dents and chips it shows, and whether one fracture face steps down in a
/// ledge.
#[derive(Clone, Copy)]
struct Kind {
    surface: Surface,
    /// Height over the long side.
    flat: f32,
    /// Width over the long side.
    wide: f32,
    /// 0 water-worn round .. 1 angular (its block's faces barely softened).
    angular: f32,
    /// Superellipsoid exponent: 2 an ellipsoid, higher boxier.
    boxy: f32,
    dents: f32,
    chips: f32,
    ledge: bool,
    /// How much taller one end stands than the other (0 level; 0.3: the
    /// +x end 30% taller, the other 30% lower), a boulder's slope.
    lean: f32,
}

/// A step in one fracture face: plane `plane` sits `depth` lower (in the
/// unit form's radii) where the direction passes `at` along `along`.
struct Ledge {
    plane: usize,
    along: [f32; 3],
    at: f32,
    depth: f32,
}

/// sRGB bytes into colour `k` of a block.
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A water-worn stone (the stone recipe's variant 11): base `a`, dark specks
/// of `b`, pale specks, mottle and veins of `c`; `dark` and `pale` the
/// specks' shares (0-1), `speck` their scale (per m, 100-1000), `mottle` the
/// cloud towards `c`, `veins` their width (0: none) `per_m` apart, `pits`
/// the relief, `rough` the roughness; stains 3-8 cm across at a middle
/// strength (number 8) and no foliation (number 9; slate's).
#[allow(clippy::too_many_arguments)]
fn worn(a: [u8; 3], b: [u8; 3], c: [u8; 3], dark: f32, pale: f32, speck: f32, mottle: f32, veins: f32, per_m: f32, pits: f32, rough: f32) -> Surface {
    let s = presets::ROCK_GRANITE_GREY.variant(11).extras(0);
    srgb(srgb(srgb(s, 0, a), 1, b), 2, c)
        .number(0, rough)
        .number(1, veins)
        .number(2, (per_m - 1.0) / 49.0)
        .number(3, (speck - 100.0) / 900.0)
        .number(4, dark)
        .number(5, pale)
        .number(6, mottle)
        .number(7, pits)
        .number(8, 0.5)
        .number(9, 0.0)
}

/// A granite pebble (the stone recipe's crystalline variant 9, the granite
/// ball's: interlocking crystals of four minerals, the dark ones gathered in
/// clusters): base `a` (plagioclase towards `c`), dark minerals of `b`,
/// feldspar of `c`; `dark`, `buff` and `quartz` their shares (as the ball's
/// sliders), crystals `mm` across (1.25-5: number 8 shrinks the ball's
/// smallest), `rough` the polish.
fn granite(a: [u8; 3], b: [u8; 3], c: [u8; 3], [dark, buff, quartz]: [f32; 3], mm: f32, rough: f32) -> Surface {
    let s = presets::ROCK_GRANITE_GREY.variant(9).extras(0);
    srgb(srgb(srgb(s, 0, a), 1, b), 2, c)
        .number(0, rough)
        .number(1, dark / 0.5)
        .number(2, 0.7)
        .number(3, 1.0)
        .number(4, buff / 0.8)
        .number(5, 0.0)
        .number(6, quartz / 0.6)
        .number(7, 0.4)
        .number(8, (5.0 / mm.clamp(1.25, 5.0) - 1.0) / 3.0)
}

/// The ten rock types, in the reference group's order (largest first).
fn kinds() -> [Kind; 10] {
    let k = |surface, flat, wide, angular, boxy, dents, chips, ledge| Kind { surface, flat, wide, angular, boxy, dents, chips, ledge, lean: 0.0 };
    [
        // 0: grey granite gneiss, the big sub-angular cobble: a dense
        // grey grain, buff streaks along a faint foliation, rough; one
        // fracture face stepped.
        // Column 7 round 2: a long mass sloping up to the right (the
        // review: a tall squared block), its faces softer, no ledge (it
        // read as a deep bite), darker.
        Kind { lean: 0.32, ..k(worn([123, 118, 112], [76, 74, 70], [186, 174, 150], 0.4, 0.05, 650.0, 1.0, 0.0, 1.0, 0.7, 0.82).number(8, 0.85).number(9, 0.35), 0.7, 0.62, 0.78, 2.3, 0.6, 1.0, false) },
        // 1: brown sandstone, a warm umber egg, finely crinkled.
        k(worn([104, 94, 85], [70, 63, 56], [150, 138, 120], 0.5, 0.05, 1000.0, 0.35, 0.0, 1.0, 0.8, 0.85), 0.74, 0.74, 0.1, 2.2, 0.4, 0.2, false),
        // 2: dark slate, foliated, thin wandering pale quartz veins;
        // sub-angular, its faces crisp.
        k(worn([75, 75, 76], [52, 52, 53], [146, 148, 146], 0.35, 0.04, 1000.0, 0.3, 0.12, 28.0, 0.35, 0.8).number(9, 0.5), 0.72, 0.7, 0.6, 2.6, 0.4, 0.8, false),
        // 3: pale salt-and-pepper granite, round.
        k(granite([126, 122, 118], [118, 114, 110], [176, 168, 160], [0.13, 0.25, 0.15], 2.6, 0.62), 0.72, 0.76, 0.05, 2.4, 0.4, 0.2, false),
        // 4: red-brown sandstone, finely crinkled.
        k(worn([120, 100, 87], [86, 71, 62], [164, 142, 126], 0.45, 0.04, 1000.0, 0.35, 0.0, 1.0, 0.6, 0.85), 0.68, 0.74, 0.15, 2.2, 0.5, 0.3, false),
        // 5: flat grey greywacke with one thin pale quartz band.
        k(worn([108, 106, 102], [80, 79, 76], [186, 184, 176], 0.35, 0.03, 900.0, 0.25, 0.2, 5.0, 0.35, 0.8).number(10, 0.5).number(11, 0.45), 0.5, 0.66, 0.0, 2.3, 0.3, 0.1, false),
        // 6: ochre sandstone.
        k(worn([121, 100, 83], [84, 70, 59], [160, 142, 120], 0.45, 0.04, 1000.0, 0.35, 0.0, 1.0, 0.5, 0.85), 0.6, 0.72, 0.05, 2.1, 0.4, 0.1, false),
        // 7: small pink-grey granite.
        k(granite([122, 117, 113], [114, 109, 105], [174, 164, 156], [0.14, 0.3, 0.2], 2.4, 0.62), 0.6, 0.72, 0.0, 2.2, 0.2, 0.1, false),
        // 8: small dark green-grey, pale yellow flecks.
        k(worn([105, 104, 96], [78, 78, 73], [165, 155, 120], 0.25, 0.06, 800.0, 0.1, 0.0, 1.0, 0.3, 0.75), 0.7, 0.74, 0.0, 2.2, 0.2, 0.0, false),
        // 9: small dark grey basalt, fine.
        k(worn([72, 73, 72], [55, 56, 55], [116, 118, 114], 0.25, 0.05, 800.0, 0.1, 0.0, 1.0, 0.3, 0.7), 0.58, 0.72, 0.0, 2.2, 0.2, 0.0, false),
    ]
}

/// The reference group at a typical size of 7 cm: length (m), place x and
/// z (m; the camera looks from +z), heading (degrees).
const GROUP: [(f32, f32, f32, f32); 10] = [
    (0.335, -0.112, -0.15, 4.0),
    (0.21, 0.1, -0.105, 12.0),
    (0.172, 0.205, 0.0, -8.0),
    (0.135, -0.24, 0.0, 14.0),
    (0.172, -0.035, 0.03, -6.0),
    (0.141, -0.12, 0.135, 6.0),
    (0.104, 0.115, 0.1, -10.0),
    (0.084, 0.012, 0.13, 2.0),
    (0.055, -0.225, 0.11, 22.0),
    (0.061, 0.205, 0.115, -16.0),
];

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

/// The block's form and each face's radius along unit direction `d` (the
/// ledge's face stepped down past its line), unsoftened: `f(r)` for each.
fn faces(d: [f32; 3], k: &Kind, planes: &[([f32; 3], f32)], ledge: Option<&Ledge>, mut f: impl FnMut(f32)) {
    let p = k.boxy;
    f((d[0].abs().powf(p) + d[1].abs().powf(p) + d[2].abs().powf(p)).powf(-1.0 / p));
    for (i, (n, off)) in planes.iter().enumerate() {
        let c = dot(*n, d);
        if c > 0.05 {
            let step = ledge.filter(|l| l.plane == i).map_or(0.0, |l| l.depth * smoothstep(-0.012, 0.012, dot(d, l.along) - l.at));
            f((off - step) / c);
        }
    }
}

/// How sharp the block's edges stay: water rounds them; angular stones keep
/// a crisp break (column 7 round 1: soft blobs).
fn softness(k: &Kind) -> f32 {
    0.018 + 0.3 * (1.0 - k.angular).powf(1.5)
}

/// The stone's radius along unit direction `d` (before its half extents).
fn shape(d: [f32; 3], k: &Kind, seed: u32, planes: &[([f32; 3], f32)], ledge: Option<&Ledge>) -> f32 {
    let soft = softness(k);
    let mut r = f32::MAX;
    faces(d, k, planes, ledge, |x| r = if r == f32::MAX { x } else { smin(r, x, soft) });
    let q = |s: f32| [d[0] * s, d[1] * s, d[2] * s];
    // A broad unevenness; fracture faces stay nearly flat, a fine roughness
    // on them.
    r * (1.0 + 0.06 * noise::fbm3(q(1.1), seed, 3) + (0.004 + 0.02 * k.angular) * noise::fbm3(q(3.2), seed ^ 0x9e37, 3) + 0.006 * k.angular * noise::fbm3(q(9.0), seed ^ 0x51ed, 2))
}

/// One stone `len` long (x), its rock `k`, seed `s`; on the ground, round
/// the origin. `wear` scales its dents, chips and ledge; `round` softens
/// every stone towards water-worn; `density` the tier's.
fn pebble(len: f32, k: &Kind, s: u32, wear: f32, round: f32, density: f32, sub: u32) -> Mesh {
    let h = |i: u32| sh::rnd(s, i, 77);
    let mut kk = *k;
    kk.angular = (k.angular * (1.0 - round)).clamp(0.0, 1.0);
    let half = [len * 0.5, len * 0.5 * k.flat * (0.94 + 0.12 * h(1)), len * 0.5 * k.wide * (0.94 + 0.12 * h(2))];
    let planes: Vec<([f32; 3], f32)> = (0..(3 + (kk.angular * 9.0) as u32))
        .map(|i| {
            let n = geo::normalize([h(10 + i) - 0.5, (h(20 + i) - 0.5) * 0.8, h(30 + i) - 0.5]);
            (n, 0.62 + 0.16 * h(40 + i) + 0.3 * (1.0 - kk.angular))
        })
        .collect();
    // A ledge: the face that looks most up and out steps down along a line
    // across it (a fresh break that took a slab off).
    let ledge = (k.ledge && wear > 0.0 && kk.angular > 0.5)
        .then(|| {
            let (i, n) = planes.iter().enumerate().max_by(|a, b| (a.1.0[1] + 0.6 * a.1.0[2]).total_cmp(&(b.1.0[1] + 0.6 * b.1.0[2])))?;
            let a = h(60) * sh::TAU;
            let t = [a.cos(), 0.3 * a.sin(), a.sin()];
            let c = dot(t, n.0);
            let along = geo::normalize([t[0] - c * n.0[0], t[1] - c * n.0[1], t[2] - c * n.0[2]]);
            Some(Ledge { plane: i, along, at: 0.1 * (h(61) - 0.5), depth: 0.07 * wear.min(1.5) })
        })
        .flatten();
    let mut m = sh::icosphere(sub);
    if kk.angular > 0.5 {
        // Crisp edges need small triangles: refine where two faces (or the
        // ledge's riser) meet, before the form is applied.
        let fine = if density < 0.6 { 0.04 } else { 0.02 };
        detail::refine_near(&mut m, fine, 3, |c, e| {
            let d = geo::normalize(c);
            let (mut a, mut b) = (f32::MAX, f32::MAX);
            faces(d, &kk, &planes, ledge.as_ref(), |x| {
                if x < a {
                    b = a;
                    a = x;
                } else if x < b {
                    b = x;
                }
            });
            b - a < 0.5 * e || ledge.as_ref().is_some_and(|l| (dot(d, l.along) - l.at).abs() < 0.012 + e && dot(d, planes[l.plane].0) > 0.3)
        });
    }
    // An egg's taper along its length and one side flatter, so no two read
    // as matching ovals.
    let taper = (h(5) - 0.5) * 0.36 * (1.0 - 0.5 * kk.angular);
    let (flat_side, side) = (0.2 * h(6) * (1.0 - kk.angular), if h(7) < 0.5 { 1.0 } else { -1.0 });
    for v in &mut m.vertices {
        let d = geo::normalize([v[0], v[1], v[2]]);
        let r = shape(d, &kk, s, &planes, ledge.as_ref());
        let f = 1.0 + taper * d[0];
        let z = (d[2] * side).max(0.0);
        *v = [d[0] * r * half[0], d[1] * r * half[1] * f * (1.0 + k.lean * d[0]), d[2] * r * half[2] * f * (1.0 - flat_side * z * z), 0.0, 1.0, 0.0];
    }
    // A flatter underside, as it settles.
    let (lo, _) = geo::bounds(&m);
    for v in &mut m.vertices {
        let floor = lo[1] * 0.8;
        if v[1] < floor {
            v[1] = floor + (v[1] - floor) * 0.3;
        }
    }
    geo::smooth_normals(&mut m);
    let mut moved = vec![false; m.vertices.len()];
    let pick = |m: &Mesh, i: u32, up: f32| -> [f32; 3] {
        // A point on the upper, visible surface (above `up` of its height).
        let (lo, hi) = geo::bounds(m);
        let n = m.vertices.len();
        for t in 0..24 {
            let j = ((sh::rnd(s, i * 31 + t, 91) * n as f32) as usize).min(n - 1);
            let v = m.vertices[j];
            if v[1] > lo[1] + (hi[1] - lo[1]) * up {
                return [v[0], v[1], v[2]];
            }
        }
        let v = m.vertices[0];
        [v[0], v[1], v[2]]
    };
    // Dents: smooth, shallow hollows (a river's knocks, worn round again).
    let nd = (k.dents * wear * (2.0 + 4.0 * h(3))).round() as u32;
    if nd > 0 {
        let dents: Vec<([f32; 3], f32, f32)> = (0..nd)
            .map(|i| {
                let r = len * (0.06 + 0.1 * sh::rnd(s, i, 61));
                (pick(&m, 100 + i, 0.25), r, r * (0.05 + 0.08 * sh::rnd(s, i, 62)))
            })
            .collect();
        let fine = (len * 0.03 / density.clamp(0.25, 2.0)).max(0.0012);
        detail::refine_near(&mut m, fine, 6, |c, e| {
            dents.iter().any(|(at, r, _)| {
                let q = [c[0] - at[0], c[1] - at[1], c[2] - at[2]];
                (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() < r * 1.2 + 0.5 * e
            })
        });
        moved.resize(m.vertices.len(), false);
        for (i, v) in m.vertices.iter_mut().enumerate() {
            for (at, r, depth) in &dents {
                let q = [v[0] - at[0], v[1] - at[1], v[2] - at[2]];
                let t = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() / r;
                if t < 1.0 {
                    let f = (1.0 - t * t).powi(2) * depth;
                    v[0] -= v[3] * f;
                    v[1] -= v[4] * f;
                    v[2] -= v[5] * f;
                    moved[i] = true;
                }
            }
        }
    }
    // Chips: a few fresh breaks, flat fracture facets on the upper ridges.
    let nc = (k.chips * wear * (1.0 + 3.0 * h(4))).floor() as u32;
    if nc > 0 {
        let chips: Vec<detail::Chip> = (0..nc)
            .map(|i| {
                let r = len * (0.06 + 0.08 * sh::rnd(s, i, 71));
                detail::Chip { at: pick(&m, 200 + i, 0.4), radius: r, depth: r * (0.2 + 0.15 * sh::rnd(s, i, 72)), fine: (r * 0.35 / density.clamp(0.25, 2.0)).max(0.0015), seed: s ^ i.wrapping_mul(0x2545_f491) }
            })
            .collect();
        let mv = detail::spalls(&mut m, &chips);
        detail::merge_flags(&mut moved, &mv);
    }
    if moved.iter().any(|x| *x) {
        detail::smooth_normals_where(&mut m, &moved);
    }
    sh::to_ground(&mut m);
    m
}

/// One stone of the group: length, rock type, and a fixed place and yaw if the reference has one.
type Stone = (f32, usize, Option<([f32; 2], f32)>);

/// The group: `count` stones of typical size `size` (m) packed round the
/// origin within about `spread` (m), each rock type one surfaced part.
#[allow(clippy::too_many_arguments)]
pub fn group(count: u32, spread: f32, size: f32, colour: [f32; 3], mix: f32, wear: f32, round: f32, seed: u32, density: f32) -> Vec<evani_procedural::Part> {
    let ks = kinds();
    let n = count.max(1);
    let scale = size / 0.07;
    // The reference's ten, then a seeded spread round the typical size.
    let stones: Vec<Stone> = (0..n)
        .map(|i| {
            let (len, kind, at) = match GROUP.get(i as usize) {
                Some(&(l, x, z, yaw)) => (l * scale, i as usize, Some(([x * scale, z * scale], yaw.to_radians()))),
                None => (size * sh::range(seed, i, 9, 0.6, 1.6), (sh::rnd(seed, i, 8) * 10.0) as usize % 10, None),
            };
            let kind = if sh::rnd(seed, i, 7) < mix { kind } else { 5 };
            (len, kind, at)
        })
        .collect();
    // Mesh detail by size, inside the triangle budget (40k at High; the
    // carved wear adds its refinement on top of these).
    let level = |l: f32| -> u32 {
        // Only the two big cobbles need the finest form; the angular
        // stones' creases are refined where they are.
        let base = if l > 0.19 * scale { 4 } else { 3 };
        if density < 0.6 { base - 1 } else { base }
    };
    let mut subs: Vec<u32> = stones.iter().map(|(l, _, _)| level(*l)).collect();
    let tris = |subs: &[u32]| subs.iter().map(|s| 20usize << (2 * s)).sum::<usize>();
    let cap = if density < 0.6 { 10_000 } else { 30_000 };
    // Over the cap, the smallest of the finest stones loses a level first.
    while tris(&subs) > cap {
        let Some(j) = (0..subs.len()).max_by_key(|&j| subs[j]) else { break };
        if subs[j] <= 1 {
            break;
        }
        subs[j] -= 1;
    }
    let mut placed: Vec<([f32; 2], f32)> = Vec::new();
    let mut parts: Vec<Mesh> = vec![Mesh::default(); ks.len()];
    for (i, ((len, kind, at), sub)) in stones.iter().zip(subs).enumerate() {
        let i = i as u32;
        let s = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
        let m = pebble(*len, &ks[*kind], s, wear, round, density, sub);
        let (lo, hi) = geo::bounds(&m);
        let foot = 0.5 * (hi[0] - lo[0]).max(hi[2] - lo[2]);
        let (p, yaw) = match at {
            Some((p, yaw)) => (*p, *yaw),
            None => {
                // As near the middle as it fits (a little nestling allowed).
                let mut best = ([0.0f32, 0.0], f32::MAX);
                for t in 0..64u32 {
                    let a = sh::rnd(seed, i * 64 + t, 11) * sh::TAU;
                    let r = spread * sh::rnd(seed, i * 64 + t, 12).sqrt();
                    let p = [a.cos() * r, a.sin() * r];
                    let over: f32 = placed.iter().map(|(q, f)| ((foot + f) * 0.8 - ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()).max(0.0)).sum();
                    let cost = over * 40.0 + r;
                    if cost < best.1 {
                        best = (p, cost);
                    }
                }
                (best.0, sh::rnd(seed, i, 13) * sh::TAU)
            }
        };
        placed.push((p, foot));
        geo::merge(&mut parts[*kind], &sh::place(&m, 1.0, yaw, 0.0, [p[0], 0.0, p[1]]));
    }
    let tint = (0..3).any(|c| (colour[c] - COLOR[c]).abs() > 1e-3);
    parts
        .into_iter()
        .zip(ks.iter())
        .enumerate()
        .filter(|(_, (m, _))| !m.indices.is_empty())
        .map(|(i, (m, k))| {
            // Each rock its own pattern (and vein direction).
            let mut sf = k.surface.seed(seed.wrapping_add(i as u32 * 0x2f6b));
            if i == 5 {
                // The greywacke's quartz band crosses its face diagonally,
                // as best.png's (column 7 round 2: a seeded direction ran it
                // nearly upright at the stone's edge): shader seed 11 sets
                // the band's plane (rc_stone.wgsl, variant 11).
                sf.block[23] = 11;
            }
            if tint {
                let a = sf.colour(0);
                sf = sf.tint([a[0] * colour[0] / COLOR[0], a[1] * colour[1] / COLOR[1], a[2] * colour[2] / COLOR[2]]);
            }
            evani_procedural::Part::surfaced(m, sf)
        })
        .collect()
}
