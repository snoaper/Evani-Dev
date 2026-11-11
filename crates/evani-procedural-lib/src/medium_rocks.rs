//! Medium rocks (rocks-medium-rocks; the Asset Studio medium rocks session,
//! column 23, 2026-11-07: a studio group of five hand-sized to knee-high
//! rocks of different stone, each broken from bedrock rather than worn by a
//! river). Each rock is one welded mesh: a slightly boxy form (a
//! superellipsoid) cut by many fracture planes whose meeting is kept crisp
//! (refined where two faces meet), a flat bed underneath, faces that are
//! never quite flat (a broad wave and a fine roughness), then real geometry
//! for what the stone does when it breaks: layered rock (the schist, the
//! sandstone) steps back layer by layer along its bedding, the quartz and
//! basalt carry shell-like hollows with sharp rims (conchoidal breaks), and
//! every rock a few fresh chips on its upper edges ([`detail::spalls`]).
//!
//! Each rock type is one part in the stone recipe (rc_stone.wgsl: world
//! space, so it draws the same in any game that lists `stone`): granite in
//! the crystalline variant 9 (white plagioclase, salmon feldspar, smoky
//! quartz and black mica), the others in variant 11 (grain, cloud, stains,
//! foliation streaks, veins), whose vein and foliation direction is the
//! shader seed's: the seed is chosen so the schist's streaks run along its
//! length and the sandstone's pale band crosses its top (see [`vein_seed`]).
//!
//! The first five lie as the reference's group (scaled by `size`); past
//! them each further rock is set down as near the middle as it fits.
//! Written fresh; the form and the carving reuse the pebbles' method
//! (crate::pebbles, column 7), adapted to angular broken rock.

use evani_procedural::detail;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::noise;
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};

/// The library colour param's default: at it every rock keeps its own tone.
pub const COLOR: [f32; 3] = [0.3, 0.29, 0.27];

/// A rock type: its stone, its proportions and how it breaks.
#[derive(Clone, Copy)]
struct Kind {
    surface: Surface,
    /// Height and width over the length.
    flat: f32,
    wide: f32,
    /// Fracture planes cut into the form, and how far in they sit (unit
    /// radii: 0.55 deep cuts .. 0.9 shaving the corners).
    planes: u32,
    cut: f32,
    /// Edge rounding (unit radii): 0.02 a fresh break .. 0.15 weathered.
    soft: f32,
    /// Superellipsoid exponent: 2 an ellipsoid, higher boxier.
    boxy: f32,
    /// A broad wave over the faces and a finer roughness (unit radii).
    wave: f32,
    rough: f32,
    /// A flat top (a bedding or cleavage face) this far up (0: none),
    /// sloping up towards +x by `slope`.
    top: f32,
    slope: f32,
    /// Bedding layers up its height (0: massive rock) and how far each
    /// steps back or out (unit radii).
    layers: f32,
    step: f32,
    /// Fresh chips on the upper edges and shell-like hollows on the faces.
    chips: f32,
    scoops: f32,
    /// How much taller one end stands than the other (0 level; 0.3: the +x
    /// end 30% taller, the other 30% lower), a wedge.
    lean: f32,
    /// How much narrower the top is than the foot (0 none; 0.3 a peaked
    /// lump).
    taper: f32,
    /// The direction the stone's veins and foliation streaks cross, in the
    /// rock's own frame (zero: any).
    vein: [f32; 3],
}

/// sRGB bytes into colour `k` of a block.
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A stone in the recipe's variant 11: base `a`, dark grains of `b`, pale
/// grains, cloud and veins of `c`; `dark` and `pale` the grains' shares
/// (0-1), `grain` their scale (per m, 100-1000), `cloud` the mottle towards
/// `c`, `veins` their width (0: none) `per_m` apart, `relief` the grain
/// relief, `rough` the roughness, `stains` (number 8) and `foliation`
/// (number 9, the fine streaks along the veins' planes).
#[allow(clippy::too_many_arguments)]
fn stone(
    a: [u8; 3],
    b: [u8; 3],
    c: [u8; 3],
    [dark, pale]: [f32; 2],
    grain: f32,
    cloud: f32,
    [veins, per_m]: [f32; 2],
    relief: f32,
    rough: f32,
    stains: f32,
    foliation: f32,
) -> Surface {
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

/// Granite in the recipe's crystalline variant 9 (the granite ball's):
/// plagioclase `a`, dark mica and hornblende `b`, feldspar `c`; `dark`,
/// `buff` and `quartz` their shares, crystals `mm` across (1.25-5), `rough`
/// the polish.
fn granite(
    a: [u8; 3],
    b: [u8; 3],
    c: [u8; 3],
    [dark, buff, quartz]: [f32; 3],
    mm: f32,
    rough: f32,
) -> Surface {
    let s = presets::ROCK_GRANITE_GREY.variant(9).extras(0);
    // Number 3 the crystal field's scale (1-200 per m): 150, crystals 5 to
    // 12 mm, a sixtieth of the big rock as in the reference.
    srgb(srgb(srgb(s, 0, a), 1, b), 2, c)
        .number(0, rough)
        .number(1, dark / 0.5)
        .number(2, 0.7)
        .number(3, 0.75)
        .number(4, buff / 0.8)
        .number(5, 0.0)
        .number(6, quartz / 0.6)
        .number(7, 0.5)
        .number(8, (5.0 / mm.clamp(1.25, 5.0) - 1.0) / 3.0)
}

/// Sandstone in the recipe's bedded variant 10 (the sandstone column's):
/// beds of their own tones (`a` towards `b`), laminae and rusty iron lines,
/// a fine sand grain and pits; beds `cm` thick (1-4 cm: the strata follow
/// world height, so a lying rock shows them round its sides), `rough` the
/// roughness.
fn bedded(a: [u8; 3], b: [u8; 3], c: [u8; 3], cm: f32, rough: f32) -> Surface {
    let s = presets::ROCK_SANDSTONE_RED.variant(10).extras(0);
    srgb(srgb(srgb(s, 0, a), 1, b), 2, c)
        .number(0, rough)
        .number(1, 0.0)
        .number(2, ((200.0 / cm.clamp(4.0, 200.0)) - 1.0) / 49.0)
        .number(3, 0.75)
}

/// The five rock types, in the reference group's order.
fn kinds() -> [Kind; 5] {
    let base = Kind {
        surface: presets::ROCK_GRANITE_GREY,
        flat: 0.7,
        wide: 0.75,
        planes: 9,
        cut: 0.7,
        soft: 0.05,
        boxy: 2.6,
        wave: 0.05,
        rough: 0.02,
        top: 0.0,
        slope: 0.0,
        layers: 0.0,
        step: 0.0,
        chips: 0.6,
        scoops: 0.0,
        lean: 0.0,
        taper: 0.0,
        vein: [0.0; 3],
    };
    [
        // 0: grey granite, the big blocky rock at the back: coarse crystals
        // of white plagioclase, salmon feldspar, grey quartz and black mica
        // (gathered in clusters); its corners barely weathered.
        Kind {
            // (Variant 9 mixes its white plagioclase halfway to colour C and
            // its K-feldspar a third, warmed: so C is the near-white and A
            // the salmon.)
            surface: granite(
                [150, 134, 118],
                [30, 29, 28],
                [170, 167, 160],
                [0.24, 0.36, 0.22],
                5.0,
                0.74,
            ),
            flat: 0.8,
            wide: 0.8,
            planes: 12,
            cut: 0.62,
            soft: 0.05,
            boxy: 3.0,
            wave: 0.05,
            rough: 0.025,
            chips: 0.8,
            lean: 0.1,
            ..base
        },
        // 1: milky vein quartz: a peaked lump, glossy, iron-stained orange
        // along its cracks and in patches, grey where it is clearest;
        // shell-like breaks.
        Kind {
            surface: stone(
                [178, 171, 160],
                [134, 134, 136],
                [192, 126, 68],
                [0.0, 0.0],
                150.0,
                0.55,
                [0.3, 9.0],
                0.1,
                0.25,
                0.3,
                0.0,
            ),
            flat: 0.95,
            wide: 0.85,
            planes: 12,
            cut: 0.72,
            soft: 0.045,
            boxy: 2.0,
            wave: 0.04,
            rough: 0.012,
            chips: 0.6,
            scoops: 1.0,
            taper: 0.35,
            vein: [0.6, 0.2, 0.75],
            ..base
        },
        // 2: dark basalt: a fine black-grey block, crisp fracture faces, a
        // flat top, a faint brown weathering.
        Kind {
            surface: stone(
                [70, 67, 63],
                [46, 44, 42],
                [116, 108, 98],
                [0.25, 0.05],
                700.0,
                0.05,
                [0.0, 1.0],
                0.4,
                0.78,
                0.0,
                0.0,
            ),
            flat: 0.62,
            wide: 0.66,
            planes: 9,
            cut: 0.72,
            soft: 0.025,
            boxy: 3.2,
            wave: 0.04,
            rough: 0.015,
            top: 0.82,
            slope: -0.08,
            chips: 0.8,
            scoops: 0.5,
            ..base
        },
        // 3: red sandstone: a thick wedge of rusty terracotta, fine sugary
        // grain, its bedding stepped along its sides and one pale quartz
        // band across its top.
        Kind {
            surface: bedded([152, 90, 62], [132, 76, 54], [220, 198, 172], 4.0, 0.88),
            flat: 0.55,
            wide: 0.66,
            planes: 9,
            cut: 0.72,
            soft: 0.03,
            boxy: 3.0,
            wave: 0.04,
            rough: 0.015,
            top: 0.8,
            slope: 0.12,
            layers: 5.0,
            step: 0.035,
            chips: 0.7,
            lean: -0.25,
            vein: [0.7, 0.0, 0.7],
            ..base
        },
        // 4: green schist: a long slab, grey-green with pale streaks along
        // its length (its foliation), stepped layer edges down its sides.
        Kind {
            surface: stone(
                [116, 118, 100],
                [76, 82, 66],
                [172, 174, 158],
                [0.3, 0.12],
                600.0,
                0.35,
                [0.0, 1.0],
                0.4,
                0.62,
                0.4,
                0.85,
            ),
            flat: 0.42,
            wide: 0.58,
            planes: 8,
            cut: 0.75,
            soft: 0.025,
            boxy: 3.2,
            wave: 0.05,
            rough: 0.015,
            top: 0.78,
            slope: 0.18,
            layers: 7.0,
            step: 0.03,
            chips: 0.7,
            lean: 0.25,
            vein: [0.0, 0.0, 1.0],
            ..base
        },
    ]
}

/// The reference group at a typical size of 0.4 m: length (m), place x and
/// z (m; the camera looks from +z), heading (degrees; positive turns the
/// rock's +x end away from the camera).
const GROUP: [(f32, f32, f32, f32); 5] = [
    (0.56, -0.38, -0.24, 25.0),
    (0.4, 0.02, -0.02, -10.0),
    (0.54, 0.44, -0.28, -20.0),
    (0.58, -0.32, 0.27, -20.0),
    (0.64, 0.4, 0.24, 28.0),
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
/// space), so the paint's veins and streaks line up with the rock's form.
fn vein_seed(want: [f32; 3]) -> u8 {
    let want = geo::normalize(want);
    (0u32..256)
        .max_by(|a, b| {
            dot(vein_dir(*a), want)
                .abs()
                .total_cmp(&dot(vein_dir(*b), want).abs())
        })
        .unwrap_or(0) as u8
}

/// The block's form and each fracture face's radius along unit direction
/// `d`, unsoftened: `f(r)` for each.
fn faces(d: [f32; 3], k: &Kind, planes: &[([f32; 3], f32)], mut f: impl FnMut(f32)) {
    let p = k.boxy;
    f((d[0].abs().powf(p) + d[1].abs().powf(p) + d[2].abs().powf(p)).powf(-1.0 / p));
    for (n, off) in planes {
        let c = dot(*n, d);
        if c > 0.05 {
            f(off / c);
        }
    }
}

/// The rock's radius along unit direction `d` (before its half extents).
fn shape(d: [f32; 3], k: &Kind, seed: u32, planes: &[([f32; 3], f32)], rough: f32) -> f32 {
    let mut r = f32::MAX;
    faces(d, k, planes, |x| {
        r = if r == f32::MAX { x } else { smin(r, x, k.soft) }
    });
    let q = |s: f32| [d[0] * s, d[1] * s, d[2] * s];
    // Fracture faces are never quite flat: a broad wave, a finer roughness.
    r * (1.0
        + rough
            * (k.wave * noise::fbm3(q(1.3), seed, 3)
                + k.rough * noise::fbm3(q(4.2), seed ^ 0x9e37, 3)
                + 0.4 * k.rough * noise::fbm3(q(11.0), seed ^ 0x51ed, 2)))
}

/// One rock `len` long (x), its type `k`, seed `s`; on the ground round the
/// origin. `rough` scales its faces' unevenness, `wear` its chips and
/// hollows; `density` the tier's.
fn rock(len: f32, k: &Kind, s: u32, rough: f32, wear: f32, density: f32) -> Mesh {
    let h = |i: u32| sh::rnd(s, i, 77);
    let half = [
        len * 0.5,
        len * 0.5 * k.flat * (0.94 + 0.12 * h(1)),
        len * 0.5 * k.wide * (0.94 + 0.12 * h(2)),
    ];
    let mut planes: Vec<([f32; 3], f32)> = (0..k.planes)
        .map(|i| {
            let n = geo::normalize([h(10 + i) - 0.5, (h(20 + i) - 0.4) * 0.9, h(30 + i) - 0.5]);
            (n, k.cut + 0.18 * h(40 + i))
        })
        .collect();
    // The bed it lies on, and a slab's top.
    planes.push(([0.0, -1.0, 0.0], 0.78));
    if k.top > 0.0 {
        planes.push((geo::normalize([-k.slope, 1.0, 0.0]), k.top));
    }
    // Bedding: layers up the height, each stepped back or out by its own
    // amount, across the layers' wavy boundaries.
    let fol = geo::normalize([-k.slope / half[0], 1.0 / half[1], 0.0]);
    let spacing = if k.layers > 0.0 {
        2.0 * half[1] / k.layers
    } else {
        1.0
    };
    let layer = |p: [f32; 3]| -> f32 {
        dot(p, fol) / spacing
            + 0.35
                * noise::fbm3(
                    [p[0] * 6.0 / len, p[1] * 6.0 / len, p[2] * 6.0 / len],
                    s ^ 0x7a11,
                    2,
                )
    };
    let place = |d: [f32; 3], r: f32| -> [f32; 3] {
        let t = 1.0 - k.taper * d[1];
        [
            d[0] * r * half[0] * t,
            d[1] * r * half[1] * (1.0 + k.lean * d[0]),
            d[2] * r * half[2] * t,
        ]
    };
    let mut m = sh::icosphere(3);
    // Mesh detail where it shows (unit-sphere edge lengths; the base level
    // 3 is about 0.13, some 3 cm on a 0.5 m rock, enough for the flat
    // fracture faces): finer along the creases (crisp breaks) and the
    // bedding's risers, inside the group's 40k triangles at High.
    let (crease_e, riser_e) = if density < 0.6 {
        (0.08, 0.09)
    } else {
        (0.045, 0.07)
    };
    detail::refine_near(&mut m, crease_e, 8, |c, e| {
        let d = geo::normalize(c);
        if d[1] < -0.5 {
            return false;
        }
        let (mut a, mut b) = (f32::MAX, f32::MAX);
        faces(d, k, &planes, |x| {
            if x < a {
                b = a;
                a = x;
            } else if x < b {
                b = x;
            }
        });
        if b - a < 0.3 * e {
            return true;
        }
        if k.layers > 0.0 && e > riser_e {
            let u = layer(place(d, shape(d, k, s, &planes, rough)));
            let f = u - u.floor();
            return f > 0.88 - e * 0.7 || f < 0.01 + e * 0.3;
        }
        false
    });
    // The crease each vertex sits on (for the chips): how near its two
    // nearest faces are.
    let mut crease = Vec::with_capacity(m.vertices.len());
    for v in &mut m.vertices {
        let d = geo::normalize([v[0], v[1], v[2]]);
        let r = shape(d, k, s, &planes, rough);
        let (mut a, mut b) = (f32::MAX, f32::MAX);
        faces(d, k, &planes, |x| {
            if x < a {
                b = a;
                a = x;
            } else if x < b {
                b = x;
            }
        });
        crease.push(b - a);
        let p = place(d, r);
        *v = [p[0], p[1], p[2], 0.0, 1.0, 0.0];
    }
    geo::smooth_normals(&mut m);
    let mut moved = vec![false; m.vertices.len()];
    if k.layers > 0.0 && k.step > 0.0 {
        // Each layer recessed by its own amount, a short steep riser where
        // the next begins; the bedding face itself (the top) left whole.
        let depth = k.step * len * 0.5;
        let rec = |i: f32| sh::rnd(s, (i as i32 + 1000) as u32, 333);
        for (i, v) in m.vertices.iter_mut().enumerate() {
            let p = [v[0], v[1], v[2]];
            let side = ((1.0 - dot([v[3], v[4], v[5]], fol).abs()) * 1.6).clamp(0.0, 1.0);
            if side <= 0.0 {
                continue;
            }
            let u = layer(p);
            let li = u.floor();
            let f = depth
                * side
                * (rec(li) + (rec(li + 1.0) - rec(li)) * smoothstep(0.86, 1.0, u - li));
            v[0] -= v[3] * f;
            v[1] -= v[4] * f;
            v[2] -= v[5] * f;
            moved[i] = true;
        }
        geo::smooth_normals(&mut m);
    }
    let pick = |m: &Mesh, i: u32, up: f32, edge: Option<&[f32]>| -> [f32; 3] {
        // A point on the upper, visible surface (above `up` of its height),
        // on a crease when asked.
        let (lo, hi) = geo::bounds(m);
        let n = m.vertices.len();
        for t in 0..48 {
            let j = ((sh::rnd(s, i * 53 + t, 91) * n as f32) as usize).min(n - 1);
            let v = m.vertices[j];
            let on_edge = edge.is_none_or(|c| c.get(j).is_some_and(|g| *g < 0.03) || t > 40);
            if v[1] > lo[1] + (hi[1] - lo[1]) * up && v[2] > -0.2 * half[2] && on_edge {
                return [v[0], v[1], v[2]];
            }
        }
        let v = m.vertices[0];
        [v[0], v[1], v[2]]
    };
    // Shell-like hollows (conchoidal breaks): shallow, their rims sharp.
    let ns = (k.scoops * wear * (1.0 + 2.0 * h(3))).round() as u32;
    if ns > 0 {
        let scoops: Vec<([f32; 3], f32, f32)> = (0..ns)
            .map(|i| {
                let r = len * (0.08 + 0.08 * sh::rnd(s, i, 61));
                (
                    pick(&m, 100 + i, 0.3, None),
                    r,
                    r * (0.06 + 0.05 * sh::rnd(s, i, 62)),
                )
            })
            .collect();
        let fine = (len * 0.025 / density.clamp(0.25, 2.0)).max(0.003);
        detail::refine_near(&mut m, fine, 5, |c, e| {
            scoops.iter().any(|(at, r, _)| {
                let q = [c[0] - at[0], c[1] - at[1], c[2] - at[2]];
                ((q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() - r).abs() < 0.5 * e + fine
            })
        });
        crease.resize(m.vertices.len(), 1.0);
        moved.resize(m.vertices.len(), false);
        for (i, v) in m.vertices.iter_mut().enumerate() {
            for (at, r, depth) in &scoops {
                let q = [v[0] - at[0], v[1] - at[1], v[2] - at[2]];
                let t = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() / r;
                if t < 1.0 {
                    let f = (1.0 - t * t) * depth;
                    v[0] -= v[3] * f;
                    v[1] -= v[4] * f;
                    v[2] -= v[5] * f;
                    moved[i] = true;
                }
            }
        }
    }
    // Chips: fresh breaks on the upper edges.
    let nc = (k.chips * wear * (2.0 + 3.0 * h(4))).floor() as u32;
    if nc > 0 {
        let chips: Vec<detail::Chip> = (0..nc)
            .map(|i| {
                let r = len * (0.04 + 0.06 * sh::rnd(s, i, 71));
                detail::Chip {
                    at: pick(&m, 200 + i, 0.35, Some(&crease)),
                    radius: r,
                    depth: r * (0.25 + 0.2 * sh::rnd(s, i, 72)),
                    fine: (r * 0.35 / density.clamp(0.25, 2.0)).max(0.002),
                    seed: s ^ i.wrapping_mul(0x2545_f491),
                }
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

/// One rock of the group: length, rock type, and a fixed place and yaw if
/// the reference has one.
type Placed = (f32, usize, Option<([f32; 2], f32)>);

/// The group: `count` rocks of typical length `size` (m) round the origin,
/// each rock type one surfaced part; `mix` 0 makes every rock the grey
/// basalt-like stone in `colour`, `rough` scales the faces' unevenness,
/// `wear` the chips and hollows.
#[allow(clippy::too_many_arguments)]
pub fn group(
    count: u32,
    size: f32,
    colour: [f32; 3],
    rough: f32,
    mix: f32,
    wear: f32,
    seed: u32,
    density: f32,
) -> Vec<evani_procedural::Part> {
    let ks = kinds();
    let n = count.max(1);
    let scale = size / 0.4;
    let rocks: Vec<Placed> = (0..n)
        .map(|i| {
            let (len, kind, at) = match GROUP.get(i as usize) {
                Some(&(l, x, z, yaw)) => (
                    l * scale,
                    i as usize,
                    Some(([x * scale, z * scale], yaw.to_radians())),
                ),
                None => (
                    size * sh::range(seed, i, 9, 0.7, 1.4),
                    (sh::rnd(seed, i, 8) * 5.0) as usize % 5,
                    None,
                ),
            };
            let kind = if sh::rnd(seed, i, 7) < mix { kind } else { 2 };
            (len, kind, at)
        })
        .collect();
    // Past a dozen rocks each is drawn at the next tier's detail.
    let density = if n > 12 { density.min(0.5) } else { density };
    let mut placed: Vec<([f32; 2], f32)> = Vec::new();
    let mut parts: Vec<Mesh> = vec![Mesh::default(); ks.len()];
    let mut yaws: [Option<f32>; 5] = [None; 5];
    for (i, (len, kind, at)) in rocks.iter().enumerate() {
        let i = i as u32;
        let s = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
        let m = rock(*len, &ks[*kind], s, rough / 0.6, wear, density);
        let (lo, hi) = geo::bounds(&m);
        let foot = 0.5 * (hi[0] - lo[0]).max(hi[2] - lo[2]);
        let (p, yaw) = match at {
            Some((p, yaw)) => (*p, *yaw),
            None => {
                // As near the middle as it fits.
                let mut best = ([0.0f32, 0.0], f32::MAX);
                for t in 0..64u32 {
                    let a = sh::rnd(seed, i * 64 + t, 11) * sh::TAU;
                    let r =
                        size * 2.5 * sh::rnd(seed, i * 64 + t, 12).sqrt() * (n as f32 / 5.0).sqrt();
                    let p = [a.cos() * r, a.sin() * r];
                    let over: f32 = placed
                        .iter()
                        .map(|(q, f)| {
                            ((foot + f) * 0.85
                                - ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt())
                            .max(0.0)
                        })
                        .sum();
                    let cost = over * 40.0 + r;
                    if cost < best.1 {
                        best = (p, cost);
                    }
                }
                (best.0, sh::rnd(seed, i, 13) * sh::TAU)
            }
        };
        placed.push((p, foot));
        if yaws[*kind].is_none() {
            yaws[*kind] = Some(yaw);
        }
        geo::merge(
            &mut parts[*kind],
            &sh::place(&m, 1.0, yaw, 0.0, [p[0], 0.0, p[1]]),
        );
    }
    let tint = (0..3).any(|c| (colour[c] - COLOR[c]).abs() > 1e-3);
    parts
        .into_iter()
        .zip(ks.iter())
        .enumerate()
        .filter(|(_, (m, _))| !m.indices.is_empty())
        .map(|(i, (m, k))| {
            let mut sf = k.surface.seed(seed.wrapping_add(i as u32 * 0x2f6b));
            if dot(k.vein, k.vein) > 0.0 {
                // The paint's veins and streaks along the rock's own form:
                // its direction turned by the rock's heading.
                let y = yaws[i].unwrap_or(0.0);
                let (sn, cs) = y.sin_cos();
                let v = k.vein;
                sf.block[23] = vein_seed([v[0] * cs + v[2] * sn, v[1], -v[0] * sn + v[2] * cs]);
            }
            if tint {
                let a = sf.colour(0);
                sf = sf.tint([
                    a[0] * colour[0] / COLOR[0],
                    a[1] * colour[1] / COLOR[1],
                    a[2] * colour[2] / COLOR[2],
                ]);
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
    fn the_group_is_five_rocks_on_the_ground_inside_the_budget() {
        let g = group(5, 0.4, COLOR, 0.6, 1.0, 1.0, 1, 1.0);
        assert_eq!(g.len(), 5, "one part per rock type");
        for p in &g {
            let (lo, hi) = geo::bounds(&p.mesh);
            assert!(lo[1].abs() < 1e-3, "on the ground: {lo:?}");
            assert!(
                hi[1] > 0.08 && hi[1] < 0.5,
                "a medium rock's height: {hi:?}"
            );
            assert!(
                p.mesh
                    .vertices
                    .iter()
                    .all(|v| v.iter().all(|x| x.is_finite()))
            );
        }
        for (i, p) in g.iter().enumerate() {
            println!("rock {i}: {} triangles", p.mesh.indices.len() / 3);
        }
        let (high, low) = (tris(&g), tris(&group(5, 0.4, COLOR, 0.6, 1.0, 1.0, 1, 0.5)));
        println!("medium rocks: {high} triangles at High, {low} at Low");
        assert!(high < 40_000 && low <= high, "{high} / {low}");
        assert!(tris(&group(40, 0.4, COLOR, 0.6, 1.0, 1.0, 3, 1.0)) < 200_000);
    }

    #[test]
    fn the_vein_seed_lines_the_paint_up_with_the_form() {
        for want in [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.7, 0.0, 0.7]] {
            let d = vein_dir(vein_seed(want) as u32);
            assert!(dot(d, geo::normalize(want)).abs() > 0.95, "{want:?}: {d:?}");
        }
    }
}
