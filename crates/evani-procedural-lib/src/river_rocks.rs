//! Rounded river rocks (rocks-rounded-river-rocks; the Asset Studio rounded
//! river rocks session, column 25, 2026-11-07): the reference's studio group
//! of five water-polished stones of different rock, from a 29 cm cobble to a
//! 56 cm one: a big dark grey greywacke crossed by cream and white quartz
//! veins (back right), a pale speckled granite egg leaning up to the right
//! (back left), a rounded tan sandstone with fine lamination rings (middle
//! front), a small dark gneiss with thin white veins (front left) and a
//! small pale speckled stone (front right).
//!
//! Each stone is one welded mesh, adapted from the pebbles' (crate::pebbles,
//! column 7): a superellipsoid (boxier for the sandstone's rounded
//! trapezoid), a few broad worn facets melted into it (a river rounds a
//! block's faces but leaves them a little flatter than an egg), an egg's
//! taper, one end standing taller (`lean`), a long axis tilted up off the
//! ground (`tilt`, the granite egg), broad seeded unevenness, a flatter
//! underside it settles on, then real wear: a few smooth, shallow dents
//! (old knocks the river has polished round again). Each rock type is one
//! part in the stone recipe (rc_stone.wgsl, world space, so it works in any
//! game that lists `stone`): the water-worn variant 11 for all five (the
//! veins and streaks turned to run along each stone's form, `vein_seed`, as
//! the medium rocks'; the granite as white and dark grey grains gathered in
//! patches: the crystalline variant 9 read as black pepper on white, rounds
//! 2 and 3).
//!
//! The first five lie as the reference's group (scaled by `size`); past
//! them each is set down as near the middle as it fits, so a dozen read as a
//! riverbed cluster and forty as a scatter.

use evani_procedural::detail;
use evani_procedural::geo::{self, Mesh};
use evani_procedural::noise;
use evani_procedural::shapes as sh;
use evani_procedural::surface::{Surface, presets};

/// The library colour param's default: at it every rock keeps its own tone.
pub const COLOR: [f32; 3] = [0.34, 0.32, 0.3];

/// A rock type: its stone and its water-worn form.
#[derive(Clone, Copy)]
struct Kind {
    surface: Surface,
    /// Height over the long side.
    flat: f32,
    /// Depth (width) over the long side.
    wide: f32,
    /// Superellipsoid exponent: 2 an ellipsoid, higher boxier.
    boxy: f32,
    /// Egg taper along the long side (+: the +x end fuller).
    taper: f32,
    /// How much taller the +x end stands than the other (0 level).
    lean: f32,
    /// The long axis tilted up towards +x off the ground (radians).
    tilt: f32,
    /// Broad worn facets melted into the form (0 none .. 1 clear).
    facets: f32,
    /// Smooth shallow dents (0 none).
    dents: f32,
    /// The direction the paint's veins and streaks cross, in the stone's
    /// own frame before its heading (zero: the shader seed's own).
    vein: [f32; 3],
}

/// sRGB bytes into colour `k` of a block.
fn srgb(mut s: Surface, k: usize, c: [u8; 3]) -> Surface {
    s.block[4 + 3 * k..7 + 3 * k].copy_from_slice(&c);
    s
}

/// A water-worn stone (the stone recipe's variant 11, as the pebbles'):
/// base `a`, dark grains of `b`, pale grains, cloud and veins of `c`; `dark`
/// and `pale` the grains' shares (0-1), `speck` their scale (per m,
/// 100-1000), `mottle` the cloud towards `c`, `veins` their width (0: none)
/// `per_m` apart, `pits` the relief, `rough` the roughness, `stains` 3-8 cm
/// blotches, `foliation` streaks along the vein direction.
#[allow(clippy::too_many_arguments)]
fn worn(a: [u8; 3], b: [u8; 3], c: [u8; 3], [dark, pale, speck, mottle]: [f32; 4], [veins, per_m]: [f32; 2], [pits, rough, stains, foliation]: [f32; 4]) -> Surface {
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
        .number(8, stains)
        .number(9, foliation)
}

/// The five rock types, in the reference group's order (largest first).
fn kinds() -> [Kind; 5] {
    [
        // 0: the big dark grey greywacke, an egg sloping down to the right
        // from a crown left of centre (round 1's review: the high point too
        // far right). Round 2: darker, a quiet fine grain (the review: an
        // angular confetti), fine aligned streaks along its veins, the veins
        // broader, softer-edged and closer to the stone's tone (a diffuse
        // cream band and fainter strands, not clean painted lines).
        Kind {
            surface: worn([98, 99, 99], [68, 69, 70], [156, 150, 136], [0.4, 0.03, 450.0, 0.22], [0.5, 5.0], [0.45, 0.72, 0.12, 0.95]),
            flat: 0.58,
            wide: 0.78,
            boxy: 2.2,
            taper: -0.12,
            lean: -0.1,
            // Round 3: the whole egg tilted up to the left, so its crown sits
            // left of centre on a smooth oval (a stronger lean read as a
            // wedge).
            tilt: -0.2,
            facets: 0.5,
            dents: 0.5,
            vein: [0.8, 0.3, -0.5],
        },
        // 1: the pale granite egg, its fuller end up to the right; round 2:
        // lower and narrower, softer greys (the review: a high-contrast
        // speckle), its pale and dark grains gathered in patches (the
        // crystalline variant 9 read as black pepper on white here).
        Kind {
            surface: worn([140, 137, 132], [92, 91, 90], [186, 182, 174], [0.45, 0.32, 600.0, 0.45], [0.0, 1.0], [0.5, 0.8, 0.35, 0.0]),
            flat: 0.62,
            wide: 0.72,
            boxy: 2.2,
            taper: 0.16,
            lean: 0.18,
            tilt: 0.1,
            facets: 0.4,
            dents: 0.5,
            vein: [0.0; 3],
        },
        // 2: the tan sandstone, a rounded trapezoid with a higher crown;
        // round 2: greyer and darker (the review: too yellow and bright), a
        // subdued grain, fine laminations as near-level planes, so they ring
        // its crown like contour lines.
        Kind {
            surface: worn([156, 143, 125], [124, 112, 96], [184, 172, 154], [0.38, 0.15, 520.0, 0.25], [0.0, 1.0], [0.5, 0.85, 0.25, 1.0]),
            flat: 0.64,
            wide: 0.76,
            boxy: 2.5,
            taper: -0.14,
            lean: 0.0,
            tilt: 0.0,
            facets: 0.8,
            dents: 0.5,
            vein: [0.25, 0.9, 0.15],
        },
        // 3: the small dark gneiss, a plump dome; round 2: neutral (the
        // review: too blue), many thin white veins and strong aligned
        // streaks on the slant.
        Kind {
            surface: worn([128, 128, 127], [88, 88, 88], [176, 176, 172], [0.38, 0.0, 560.0, 0.22], [0.2, 16.0], [0.4, 0.75, 0.15, 1.0]),
            flat: 0.55,
            wide: 0.76,
            boxy: 2.4,
            taper: 0.18,
            lean: 0.14,
            tilt: 0.0,
            facets: 0.95,
            dents: 0.3,
            // Round 3 (round 2's review: clean looping lines): planes that
            // hold the reference camera's line of sight, so from it each
            // vein runs as a straight diagonal stroke from lower left to
            // upper right, as the reference's, not a ring round the dome.
            vein: [0.73, -0.66, 0.17],
        },
        // 4: the small pale speckled stone, grey-beige with dark and rusty
        // specks; round 2: an egg tapering to its right (the review: too
        // round), a quieter speckle.
        Kind {
            surface: worn([148, 142, 131], [114, 106, 95], [190, 184, 172], [0.32, 0.12, 650.0, 0.3], [0.0, 1.0], [0.5, 0.8, 0.45, 0.0]),
            flat: 0.56,
            wide: 0.78,
            boxy: 2.2,
            taper: -0.15,
            lean: -0.08,
            tilt: 0.0,
            facets: 0.25,
            dents: 0.3,
            vein: [0.0; 3],
        },
    ]
}

/// The reference group at a typical size of 0.4 m: length (m), place x and
/// z (m; the camera looks from +z), heading (degrees, +: the +x end turned
/// away from the camera).
const GROUP: [(f32, f32, f32, f32); 5] = [
    (0.59, 0.21, -0.24, -12.0),
    (0.44, -0.28, -0.12, 28.0),
    (0.39, -0.03, 0.05, 8.0),
    (0.3, -0.28, 0.21, -6.0),
    (0.29, 0.24, 0.19, 14.0),
];

/// Each reference stone's footprint at typical size `size` (m): its centre
/// (x, z), half length, half width and heading (radians), for a scene's
/// contact shadows (the proof game's `rrocks` scene, round 2).
pub fn footprints(size: f32) -> Vec<([f32; 2], f32, f32, f32)> {
    let scale = size / 0.4;
    GROUP
        .iter()
        .zip(kinds().iter())
        .map(|(&(l, x, z, yaw), k)| {
            let len = l * scale;
            ([x * scale, z * scale], len * 0.5, len * 0.5 * k.wide, yaw.to_radians())
        })
        .collect()
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The stone shader's PCG hash (core.wgsl `rc_pcg`).
fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// The variant 11 vein direction for shader seed `seed` (rc_stone.wgsl
/// `rc_stone_ww`).
fn vein_dir(seed: u32) -> [f32; 3] {
    let h = pcg(seed + 97);
    let f = |x: u32| (x & 255) as f32 / 127.5 - 0.99;
    geo::normalize([f(h), f(h >> 8) * 0.45, f(h >> 16)])
}

/// The shader seed (0-255) whose vein direction is nearest `want` (world
/// space), so the paint's veins and streaks cross the stone as planned
/// (column 23's medium rocks).
fn vein_seed(want: [f32; 3]) -> u8 {
    let want = geo::normalize(want);
    (0u32..256)
        .max_by(|a, b| dot(vein_dir(*a), want).abs().total_cmp(&dot(vein_dir(*b), want).abs()))
        .unwrap_or(0) as u8
}

/// The worn form's radius along unit direction `d` (before its half
/// extents): the superellipsoid with its broad facets melted in, and a
/// gentle unevenness.
fn shape(d: [f32; 3], k: &Kind, seed: u32, planes: &[([f32; 3], f32)]) -> f32 {
    let p = k.boxy;
    let mut r = (d[0].abs().powf(p) + d[1].abs().powf(p) + d[2].abs().powf(p)).powf(-1.0 / p);
    for (n, off) in planes {
        let c = dot(*n, d);
        if c > 0.05 {
            // A river rounds the facets' edges broadly.
            r = smin(r, off / c, 0.22);
        }
    }
    let q = |s: f32| [d[0] * s, d[1] * s, d[2] * s];
    // Round 2 (round 1's review: outlines too regular and inflated): a
    // stronger broad unevenness, so no two read as matching ovals.
    r * (1.0 + 0.062 * noise::fbm3(q(1.1), seed, 3) + 0.01 * noise::fbm3(q(3.4), seed ^ 0x9e37, 2))
}

/// One stone `len` long (x), its rock `k`, seed `s`; on the ground, round
/// the origin. `wear` scales its dents; `density` the tier's.
fn stone(len: f32, k: &Kind, s: u32, wear: f32, density: f32, sub: u32) -> Mesh {
    let h = |i: u32| sh::rnd(s, i, 77);
    let half = [len * 0.5, len * 0.5 * k.flat * (0.96 + 0.08 * h(1)), len * 0.5 * k.wide * (0.96 + 0.08 * h(2))];
    // Two to four broad worn facets, kept off the underside.
    let planes: Vec<([f32; 3], f32)> = (0..(2 + (h(3) * 2.99) as u32))
        .map(|i| {
            let n = geo::normalize([h(10 + i) - 0.5, (h(20 + i) - 0.2) * 0.8, h(30 + i) - 0.5]);
            (n, 0.97 - 0.12 * k.facets * (0.6 + 0.4 * h(40 + i)))
        })
        .collect();
    let mut m = sh::icosphere(sub);
    let (st, ct) = k.tilt.sin_cos();
    for v in &mut m.vertices {
        let d = geo::normalize([v[0], v[1], v[2]]);
        let r = shape(d, k, s, &planes);
        let f = 1.0 + k.taper * d[0];
        let p = [d[0] * r * half[0], d[1] * r * half[1] * f * (1.0 + k.lean * d[0]), d[2] * r * half[2] * f];
        // The long axis tilted up towards +x.
        *v = [p[0] * ct - p[1] * st, p[0] * st + p[1] * ct, p[2], 0.0, 1.0, 0.0];
    }
    // A flatter underside, as it settles in the bed: below `floor` the
    // form is pressed to 30% of its depth, rolled in smoothly (a softplus,
    // so the side meets the underside with no crease at the silhouette).
    let (lo, hi) = geo::bounds(&m);
    let floor = lo[1] + (hi[1] - lo[1]) * 0.035;
    let w = (hi[1] - lo[1]) * 0.02;
    for v in &mut m.vertices {
        let d = (floor - v[1]) / w;
        v[1] += 0.7 * w * if d > 20.0 { d } else { d.exp().ln_1p() };
    }
    geo::smooth_normals(&mut m);
    // Dents: smooth, shallow hollows on the upper, visible surface.
    let nd = (k.dents * wear * (1.0 + 3.0 * h(4))).round() as u32;
    if nd > 0 {
        let (lo, hi) = geo::bounds(&m);
        let n = m.vertices.len();
        let dents: Vec<([f32; 3], f32, f32)> = (0..nd)
            .filter_map(|i| {
                (0..24u32).find_map(|t| {
                    let j = ((sh::rnd(s, i * 31 + t, 91) * n as f32) as usize).min(n - 1);
                    let v = m.vertices[j];
                    (v[1] > lo[1] + (hi[1] - lo[1]) * 0.35).then(|| {
                        let r = len * (0.05 + 0.07 * sh::rnd(s, i, 61));
                        ([v[0], v[1], v[2]], r, r * (0.04 + 0.05 * sh::rnd(s, i, 62)))
                    })
                })
            })
            .collect();
        let fine = (len * 0.025 / density.clamp(0.25, 2.0)).max(0.002);
        detail::refine_near(&mut m, fine, 4, |c, e| {
            dents.iter().any(|(at, r, _)| {
                let q = [c[0] - at[0], c[1] - at[1], c[2] - at[2]];
                (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() < r * 1.2 + 0.5 * e
            })
        });
        let mut moved = vec![false; m.vertices.len()];
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
        if moved.iter().any(|x| *x) {
            detail::smooth_normals_where(&mut m, &moved);
        }
    }
    sh::to_ground(&mut m);
    m
}

/// One stone of the group: length, rock type, and a fixed place and heading
/// if the reference has one.
type Placed = (f32, usize, Option<([f32; 2], f32)>);

/// The group: `count` stones of typical length `size` (m) round the origin,
/// each rock type one surfaced part; `mix` 0 makes every stone the dark
/// greywacke in `colour`, `wear` scales the dents.
#[allow(clippy::too_many_arguments)]
pub fn group(count: u32, size: f32, colour: [f32; 3], mix: f32, wear: f32, seed: u32, density: f32) -> Vec<evani_procedural::Part> {
    let ks = kinds();
    let n = count.max(1);
    let scale = size / 0.4;
    let stones: Vec<Placed> = (0..n)
        .map(|i| {
            let (len, kind, at) = match GROUP.get(i as usize) {
                Some(&(l, x, z, yaw)) => (l * scale, i as usize, Some(([x * scale, z * scale], yaw.to_radians()))),
                None => (size * sh::range(seed, i, 9, 0.65, 1.3), (sh::rnd(seed, i, 8) * 5.0) as usize % 5, None),
            };
            let kind = if sh::rnd(seed, i, 7) < mix { kind } else { 0 };
            (len, kind, at)
        })
        .collect();
    // Smooth stones need a fine mesh for a round silhouette; past a dozen
    // each is drawn at the next tier's detail.
    let low = density < 0.6 || n > 12;
    let sub = if low { 3 } else { 4 };
    let density = if n > 12 { density.min(0.5) } else { density };
    let mut placed: Vec<([f32; 2], f32)> = Vec::new();
    let mut parts: Vec<Mesh> = vec![Mesh::default(); ks.len()];
    let mut yaws: [Option<f32>; 5] = [None; 5];
    for (i, (len, kind, at)) in stones.iter().enumerate() {
        let i = i as u32;
        let s = seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b);
        let m = stone(*len, &ks[*kind], s, wear, density, sub);
        let (lo, hi) = geo::bounds(&m);
        let foot = 0.5 * (hi[0] - lo[0]).max(hi[2] - lo[2]);
        let (p, yaw) = match at {
            Some((p, yaw)) => (*p, *yaw),
            None => {
                // As near the middle as it fits (a little nestling allowed).
                let mut best = ([0.0f32, 0.0], f32::MAX);
                for t in 0..64u32 {
                    let a = sh::rnd(seed, i * 64 + t, 11) * sh::TAU;
                    let r = size * 2.2 * sh::rnd(seed, i * 64 + t, 12).sqrt() * (n as f32 / 5.0).sqrt();
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
        if yaws[*kind].is_none() {
            yaws[*kind] = Some(yaw);
        }
        geo::merge(&mut parts[*kind], &sh::place(&m, 1.0, yaw, 0.0, [p[0], 0.0, p[1]]));
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
                // The veins and streaks cross the stone as planned: the
                // direction turned by the stone's heading.
                let (sn, cs) = yaws[i].unwrap_or(0.0).sin_cos();
                let v = k.vein;
                sf.block[23] = vein_seed([v[0] * cs + v[2] * sn, v[1], -v[0] * sn + v[2] * cs]);
            }
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
    fn the_group_is_five_smooth_stones_on_the_ground_inside_the_budget() {
        let g = group(5, 0.4, COLOR, 1.0, 1.0, 1, 1.0);
        assert_eq!(g.len(), 5, "one part per rock type");
        for p in &g {
            let (lo, hi) = geo::bounds(&p.mesh);
            assert!(lo[1].abs() < 1e-3, "on the ground: {lo:?}");
            assert!(hi[1] > 0.08 && hi[1] < 0.4, "a river rock's height: {hi:?}");
            assert!(p.mesh.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())));
        }
        let (high, low) = (tris(&g), tris(&group(5, 0.4, COLOR, 1.0, 1.0, 1, 0.5)));
        println!("rounded river rocks: {high} triangles at High, {low} at Low");
        assert!(high < 40_000 && low <= high, "{high} / {low}");
        assert!(tris(&group(40, 0.4, COLOR, 1.0, 1.0, 3, 1.0)) < 200_000);
    }

    #[test]
    fn the_vein_seed_lines_the_paint_up_with_the_form() {
        for want in [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.7, 0.0, 0.7]] {
            let d = vein_dir(vein_seed(want) as u32);
            assert!(dot(d, geo::normalize(want)).abs() > 0.95, "{want:?}: {d:?}");
        }
    }
}
