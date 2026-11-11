//! Volumetric-looking clouds (`sky-volumetric-looking-clouds`, Asset Studio
//! column 11): a cumulus heap as a real volume, ray-marched on the GPU.
//!
//! - [`cumulus`] lays the cloud out, seeded and deterministic: separate
//!   dense heaps (the tallest tower to one side, lower heaps stepping down
//!   and forwards from it) filled with overlapping spheres ("lobes"): big
//!   core lobes inside, a skin of folds over each heap, small billows on
//!   those; vapour lobes (a thinner, lumpy cloud with gaps) under the heaps
//!   and at the shoulders; wisp lobes (faint curls) out of the ends and the
//!   lower edge. The generator's CPU mesh (bounds, tests, a host without the
//!   volume) is made of the big dense lobes.
//! - [`CloudVolume`] (hosted by the procedural module whenever the asset is
//!   compiled in, so any game that lists it gets the volume) draws every
//!   placed cloud with four game passes: a tiling billow noise (once), the
//!   lobes' smooth signed distance and the vapour's and wisps' fields baked
//!   into a volume (again when the cloud changes), the sun's and the sky's
//!   optical depth through it (again when the cloud or the sun moves), and a
//!   full-screen ray march after the opaque 3D: billowed and eroded density
//!   (short steps where a ray meets a dense surface), self-shadowing from the sun
//!   (the baked depth plus a short local probe for the billows), a two-lobe
//!   phase with three multiple-scattering octaves, sky light from above and
//!   a darker base, stopped by the scene's depth.
//!
//! Written fresh. Ideas from the public literature on real-time clouds
//! (Schneider's "Horizon: Zero Dawn" talks: Perlin-Worley billows and the
//! multiple-scattering octaves after Wrenninge; Hillaire's energy-conserving
//! step), no code copied.

use evani_core::ecs::World;
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use evani_procedural::shapes as sh;
use evani_procedural::{ProcScene, Registry};
use evani_render::game_pass::{
    self, Format, GamePass, GamePasses, GameTarget, PassBlend, PassParam, Stage,
};

/// The asset's feature.
pub const FEATURE: &str = "sky-volumetric-looking-clouds";

/// One lobe: centre (m) and radius (m).
pub type Lobe = [f32; 4];

/// A cumulus's three kinds of lobe: the dense heaps' spheres, the vapour
/// (a thinner, lumpy, partly see-through cloud between and under the heaps
/// and out at the shoulders) and the wisps (faint, frayed curls at the
/// edges).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cumulus {
    pub lobes: Vec<Lobe>,
    pub vapour: Vec<Lobe>,
    pub wisps: Vec<Lobe>,
}

/// The golden angle (radians): even directions over a dome.
const GOLDEN: f32 = 2.399_963;

/// The cloud's lobes for width `w` (m), base height `h` (m), tower height
/// `tower` (the tallest heap over the base, a fraction of `w`) and seed `s`.
/// Centred on x = z = 0; the long side runs along x, the front faces +z.
///
/// Round 3 (round 2's review: a joined, opaque dark cushion under a compact,
/// upright mass; sealed edges; no sky gaps): ten separate dense heaps step
/// down from the tower to the far side and towards the front (the tower,
/// a heap beside and one in front of it, a middle heap, a heap and a front
/// heap on the far side, big low billows in front, a shoulder and a broad
/// low heap on the tower's side), each a dome over a small belly, filled
/// with core lobes; round 5: its surface is a few big rounded heads, each
/// with its own folds and billows. There is no body under them: the
/// underside's gaps, the far end and the shoulders are vapour, lumpy with
/// gaps, and long thin curls trail out of both ends and off the far side's
/// top.
pub fn cumulus(w: f32, h: f32, tower: f32, s: u32) -> Cumulus {
    let side = if sh::rnd(s, 0, 1) < 0.5 { -1.0 } else { 1.0 };
    let t = tower;
    // (along x, depth z, top, half width, half depth, foot), fractions of w.
    let domes: [(f32, f32, f32, f32, f32, f32); 10] = [
        // The tower, right of centre.
        (
            side * sh::range(s, 0, 2, 0.2, 0.23),
            sh::range(s, 0, 4, -0.06, -0.02),
            t * sh::range(s, 0, 3, 0.97, 1.03),
            0.2,
            0.16,
            0.15,
        ),
        // The heap across from it.
        (
            -side * sh::range(s, 1, 2, 0.15, 0.19),
            sh::range(s, 1, 4, -0.04, 0.0),
            t * sh::range(s, 1, 3, 0.7, 0.75),
            0.15,
            0.13,
            0.12,
        ),
        // The far side's lower heap.
        (
            -side * sh::range(s, 2, 2, 0.4, 0.43),
            sh::range(s, 2, 4, -0.03, 0.01),
            t * sh::range(s, 2, 3, 0.4, 0.45),
            0.11,
            0.1,
            0.08,
        ),
        // The shoulder on the tower's side.
        (
            side * sh::range(s, 3, 2, 0.39, 0.42),
            sh::range(s, 3, 4, -0.04, 0.0),
            t * sh::range(s, 3, 3, 0.5, 0.56),
            0.1,
            0.1,
            0.16,
        ),
        // The far side's front heap, stepping down and forwards.
        (
            -side * sh::range(s, 4, 2, 0.2, 0.25),
            sh::range(s, 4, 4, 0.1, 0.13),
            t * sh::range(s, 4, 3, 0.44, 0.48),
            0.13,
            0.1,
            0.03,
        ),
        // A big low billow out in front, its cap lit, its underside shaded,
        // reaching the cloud's lowest point.
        (
            side * sh::range(s, 5, 2, -0.06, 0.0),
            sh::range(s, 5, 4, 0.12, 0.15),
            t * sh::range(s, 5, 3, 0.2, 0.24),
            0.1,
            0.07,
            -0.065,
        ),
        // A low front billow on the far side.
        (
            -side * sh::range(s, 6, 2, 0.34, 0.38),
            sh::range(s, 6, 4, 0.07, 0.1),
            t * sh::range(s, 6, 3, 0.28, 0.32),
            0.1,
            0.08,
            -0.05,
        ),
        // A heap in front of the tower.
        (
            side * sh::range(s, 7, 2, 0.12, 0.16),
            sh::range(s, 7, 4, 0.06, 0.09),
            t * sh::range(s, 7, 3, 0.64, 0.7),
            0.11,
            0.085,
            0.15,
        ),
        // The middle heap.
        (
            side * sh::range(s, 8, 2, -0.02, 0.02),
            sh::range(s, 8, 4, -0.01, 0.03),
            t * sh::range(s, 8, 3, 0.58, 0.64),
            0.11,
            0.11,
            0.12,
        ),
        // A broad low heap under the tower's side, in front of it.
        (
            side * sh::range(s, 9, 2, 0.28, 0.32),
            sh::range(s, 9, 4, 0.05, 0.08),
            t * sh::range(s, 9, 3, 0.3, 0.34),
            0.13,
            0.08,
            0.03,
        ),
    ];
    let mut c = Cumulus::default();
    for (ci, &(cu, cv, top, ru, rv, foot)) in domes.iter().enumerate() {
        let ci = ci as u32;
        let hh = (top - foot).max(0.05);
        let rmin = ru.min(hh).min(rv);
        let rc = rmin * 0.55;
        let hb = 0.25 * rmin;
        let step = rc * 1.1;
        // Big cores fill the dome and its belly.
        let mut n = 0u32;
        let mut ys = -hb;
        while ys <= hh {
            let hy = if ys >= 0.0 {
                (hh - rc * 0.5).max(rc * 0.5)
            } else {
                (hb - rc * 0.5).max(rc * 0.5)
            };
            let mut us = -ru;
            while us <= ru {
                let mut vs = -rv;
                while vs <= rv {
                    n += 1;
                    let e = (us / (ru - rc * 0.6).max(1e-3)).powi(2)
                        + (ys / hy).powi(2)
                        + (vs / (rv - rc * 0.6).max(1e-3)).powi(2);
                    if e <= 1.0 {
                        let j = ci * 100_000 + n;
                        c.lobes.push([
                            (cu + us + sh::range(s, j, 1, -0.2, 0.2) * rc) * w,
                            h + (foot + ys + sh::range(s, j, 2, -0.15, 0.15) * rc) * w,
                            (cv + vs + sh::range(s, j, 3, -0.2, 0.2) * rc) * w,
                            rc * sh::range(s, j, 4, 0.9, 1.1) * w,
                        ]);
                    }
                    vs += step;
                }
                us += step;
            }
            ys += step;
        }
        // Heads (round 5; round 4's review: upright sculpted columns with
        // small nodules stuck on): a few big rounded lobes over the dome's
        // upper surface, those near its sides bulging out sideways and
        // forwards and overhanging, each overlapping the next. Each head
        // carries its own folds and the upper folds small billows, so the
        // cauliflower nests (heads, folds, billows, then the march's noise)
        // and the folds flow over each head rather than evenly over a column.
        let area = sh::TAU * (ru * rv + ru * hh + rv * hh) / 3.0;
        let rh0 = rmin * 0.5;
        let nh = ((area * 0.75 / (std::f32::consts::PI * rh0 * rh0)) as u32).clamp(3, 16);
        for hi in 0..nh {
            let k = ci * 100 + hi;
            let y = (1.0 - (hi as f32 + 0.5) / nh as f32 * 0.9
                + sh::range(s, 70_000 + k, 1, -0.06, 0.06))
            .clamp(0.0, 1.0);
            let sr = (1.0 - y * y).max(0.0).sqrt();
            let a = hi as f32 * GOLDEN + sh::range(s, 70_000 + k, 2, -0.5, 0.5);
            let (dx, dy, dz) = (a.cos() * sr, y, a.sin() * sr);
            let rh = rh0 * sh::range(s, 70_000 + k, 3, 0.8, 1.25);
            let out = sh::range(s, 70_000 + k, 4, 0.74, 0.88) + 0.1 * dy * dy;
            let (hx, hy, hz) = (cu + dx * ru * out, foot + dy * hh * out, cv + dz * rv * out);
            c.lobes.push([hx * w, h + hy * w, hz * w, rh * w]);
            // Folds over the head, from its crown to a little under its
            // middle (the overhang's curve), jittered in place and size.
            let nf = 8u32;
            for f in 0..nf {
                let kf = k * 16 + f;
                let fy = (1.0 - (f as f32 + 0.5) / nf as f32 * 1.3
                    + sh::range(s, 72_000 + kf, 1, -0.06, 0.06))
                .clamp(-1.0, 1.0);
                let fs = (1.0 - fy * fy).max(0.0).sqrt();
                let fa = f as f32 * GOLDEN + sh::range(s, 72_000 + kf, 2, -0.6, 0.6);
                let (ex, ey, ez) = (fa.cos() * fs, fy, fa.sin() * fs);
                // Folds facing into the dome stay hidden: skip them.
                if ex * dx + ez * dz + ey * dy < -0.35 {
                    continue;
                }
                let rf = rh * sh::range(s, 72_000 + kf, 3, 0.36, 0.56);
                let d = rh * sh::range(s, 72_000 + kf, 4, 0.78, 0.95);
                let (px, py, pz) = (hx + ex * d, hy + ey * d, hz + ez * d);
                c.lobes.push([px * w, h + py * w, pz * w, rf * w]);
                if ey > 0.25 {
                    for q in 0..2u32 {
                        let j = kf * 2 + q;
                        let a2 = sh::range(s, 74_000 + j, 1, 0.0, sh::TAU);
                        let u2 = sh::range(s, 74_000 + j, 2, 0.2, 0.9);
                        let s2 = (1.0 - u2 * u2).sqrt();
                        let rs = rf * sh::range(s, 74_000 + j, 3, 0.36, 0.5);
                        let dd = rf * 0.85;
                        c.lobes.push([
                            (px + (ex * 0.55 + a2.cos() * s2 * 0.5) * dd) * w,
                            h + (py + (ey * 0.55 + u2 * 0.55) * dd) * w,
                            (pz + (ez * 0.55 + a2.sin() * s2 * 0.5) * dd) * w,
                            rs * w,
                        ]);
                    }
                }
            }
        }
    }
    // Vapour: a lower body under the heaps (no solid cushion: lumps with
    // gaps, lowest in the middle and on the far side, rising towards the
    // tower's end), a little under the low front lumps, and out at both
    // shoulders.
    let low =
        |u: f32| -0.04 + 0.1 * smooth(0.12, 0.42, u * side) + 0.04 * smooth(0.4, 0.55, -u * side);
    let mut n = 0u32;
    let mut u = -0.48;
    while u <= 0.37 {
        for vv in [-0.06f32, 0.06] {
            n += 1;
            if sh::rnd(s, 40_000 + n, 1) < 0.25 {
                continue;
            }
            let uu = u + sh::range(s, 40_000 + n, 4, -0.02, 0.02);
            if uu * side > 0.15 {
                continue;
            }
            c.vapour.push([
                uu * w,
                h + (low(uu) + sh::range(s, 40_000 + n, 3, 0.07, 0.15)) * w,
                (vv + sh::range(s, 40_000 + n, 5, -0.03, 0.03)) * w,
                sh::range(s, 40_000 + n, 2, 0.07, 0.1) * w,
            ]);
        }
        u += 0.07;
    }
    for k in 0..35u32 {
        // 0-4: under the low front lumps; then the tower's shoulder (odd)
        // and the far end (even, and 21-28: its airy lumps); 29-34: the
        // belly under the middle, in the heaps' shade.
        let (uu, y, vv) = if k >= 29 {
            (
                -side * sh::range(s, 41_000 + k, 1, -0.12, 0.26),
                sh::range(s, 41_000 + k, 2, 0.0, 0.08),
                sh::range(s, 41_000 + k, 3, 0.02, 0.1),
            )
        } else if k < 5 {
            (
                -side * sh::range(s, 41_000 + k, 1, -0.1, 0.38),
                sh::range(s, 41_000 + k, 2, 0.0, 0.06),
                sh::range(s, 41_000 + k, 3, 0.1, 0.16),
            )
        } else if k % 2 == 1 && k < 21 {
            (
                side * sh::range(s, 41_000 + k, 1, 0.44, 0.56),
                sh::range(s, 41_000 + k, 2, 0.08, 0.26),
                sh::range(s, 41_000 + k, 3, -0.08, 0.08),
            )
        } else {
            (
                -side * sh::range(s, 41_000 + k, 1, 0.42, 0.56),
                sh::range(s, 41_000 + k, 2, 0.02, 0.26),
                sh::range(s, 41_000 + k, 3, -0.08, 0.08),
            )
        };
        c.vapour.push([
            uu * w,
            h + y * w,
            vv * w,
            sh::range(s, 41_000 + k, 4, 0.05, 0.08) * w * if k >= 29 { 1.25 } else { 1.0 },
        ]);
    }
    // Wisps: curls, each a chain of shrinking lobes that bends as it goes.
    // Round 5 (round 4's review: fuzzy patches and upright puffs, no long
    // thin curling filaments): more of them, thinner and longer, starting
    // inside the dense edge so they thin out of it, out of both ends (some
    // lower down), and a few leaning up and outwards off the far side's top.
    for k in 0..26u32 {
        let j = 4000 + k * 32;
        // (which way out along x, start along x, start height, heading in
        // the x-up plane, lobes)
        let (sg, u0, y0, a0, m) = match k {
            0..=6 => (
                -side,
                sh::range(s, j, 1, 0.42, 0.47),
                sh::range(s, j, 2, 0.03, 0.28),
                sh::range(s, j, 3, -0.45, 0.4),
                14u32,
            ),
            7..=13 => (
                side,
                sh::range(s, j, 1, 0.42, 0.47),
                sh::range(s, j, 2, 0.08, 0.25),
                sh::range(s, j, 3, -0.45, 0.15),
                14,
            ),
            14..=16 => (
                -side,
                sh::range(s, j, 1, 0.3, 0.38),
                sh::range(s, j, 2, 0.24, 0.3),
                sh::range(s, j, 3, 0.25, 0.6),
                12,
            ),
            _ => (
                if k % 2 == 0 { -side } else { side },
                sh::range(s, j, 1, 0.36, 0.44),
                sh::range(s, j, 2, 0.0, 0.1),
                sh::range(s, j, 3, -0.4, 0.15),
                12,
            ),
        };
        let (mut pu, mut py) = (sg * u0, y0);
        let mut pv = sh::range(s, j, 4, -0.08, 0.12);
        let mut a = a0;
        let bend = sh::range(s, j, 5, -0.12, 0.12);
        let r0 = sh::range(s, j, 6, 0.014, 0.022);
        let st = r0 * 0.7;
        for i in 0..m {
            let f = i as f32 / m as f32;
            c.wisps.push([
                pu * w,
                h + py.max(-0.1) * w,
                pv * w,
                r0 * (1.0 - 0.5 * f) * w,
            ]);
            pu += sg * a.cos() * st;
            py += a.sin() * st;
            pv += sh::range(s, j + 16 + i, 7, -0.25, 0.25) * st;
            a += bend + sh::range(s, j + 16 + i, 8, -0.08, 0.08);
        }
    }
    c
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The tiling billow noise (128 cubed, made once; round 4: 64 cubed left
/// the smallest billow cells four texels wide, blurred).
pub const NOISE: &str = "procedural.cloud_noise";
/// The lobes' signed distance (r, fractions of the width) and wisp field (g).
pub const SHAPE: &str = "procedural.cloud_shape";
/// The sun's (r) and the sky's (g) optical depth through the coarse cloud.
pub const LIGHT: &str = "procedural.cloud_light";
pub const NOISE_PASS: &str = "procedural.cloud_noise_make";
pub const SHAPE_PASS: &str = "procedural.cloud_shape_make";
pub const LIGHT_PASS: &str = "procedural.cloud_light_make";
/// The ray march; its live params are `procedural.cloud_march.*`.
pub const DRAW_PASS: &str = "procedural.cloud_march";
const SHAPE_DIMS: [u32; 3] = [176, 112, 88];
const LIGHT_DIMS: [u32; 3] = [96, 64, 48];
/// Records before the lobes in each pass's `data`: the box's low corner
/// and the lobe count, its high corner and the width, the colour and the
/// base height, the dense, vapour and wisp lobes' counts (in that order
/// after the header).
const HEADER: usize = 4;

/// The shared WGSL: hashes and the cloud's box.
const COMMON_WGSL: &str = "
fn cl_hash(p: vec3<i32>) -> f32 {
    var h = (bitcast<u32>(p.x) * 73856093u) ^ (bitcast<u32>(p.y) * 19349663u) ^ (bitcast<u32>(p.z) * 83492791u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967296.0;
}
fn cl_box_uvw(p: vec3<f32>) -> vec3<f32> {
    return (p - data[0].xyz) / max(data[1].xyz - data[0].xyz, vec3<f32>(1e-4));
}
";

const NOISE_WGSL: &str = "
fn cn_wrap(c: vec3<i32>, per: i32) -> vec3<i32> {
    return ((c % vec3<i32>(per)) + vec3<i32>(per)) % vec3<i32>(per);
}
fn cn_grad_at(c: vec3<i32>, per: i32, f: vec3<f32>) -> f32 {
    let w = cn_wrap(c, per);
    let g = vec3<f32>(cl_hash(w), cl_hash(w + vec3<i32>(17, 59, 113)), cl_hash(w + vec3<i32>(131, 7, 41))) * 2.0 - 1.0;
    return dot(normalize(g + vec3<f32>(1e-4)), f);
}
// Tiling gradient noise, period `per` cells, about -0.8..0.8.
fn cn_grad(q: vec3<f32>, per: i32) -> f32 {
    let i = vec3<i32>(floor(q));
    let f = fract(q);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = mix(cn_grad_at(i, per, f), cn_grad_at(i + vec3<i32>(1, 0, 0), per, f - vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(cn_grad_at(i + vec3<i32>(0, 1, 0), per, f - vec3<f32>(0.0, 1.0, 0.0)), cn_grad_at(i + vec3<i32>(1, 1, 0), per, f - vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(cn_grad_at(i + vec3<i32>(0, 0, 1), per, f - vec3<f32>(0.0, 0.0, 1.0)), cn_grad_at(i + vec3<i32>(1, 0, 1), per, f - vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(cn_grad_at(i + vec3<i32>(0, 1, 1), per, f - vec3<f32>(0.0, 1.0, 1.0)), cn_grad_at(i + vec3<i32>(1, 1, 1), per, f - vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}
// Tiling cellular noise: distance to the nearest feature point (0..~1).
fn cn_cell(q: vec3<f32>, per: i32) -> f32 {
    let i = vec3<i32>(floor(q));
    let f = fract(q);
    var d = 9.0;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let c = vec3<i32>(x, y, z);
                let w = cn_wrap(i + c, per);
                let o = vec3<f32>(cl_hash(w + vec3<i32>(5, 3, 1)), cl_hash(w + vec3<i32>(71, 29, 13)), cl_hash(w + vec3<i32>(37, 97, 61)));
                let r = vec3<f32>(c) + o - f;
                d = min(d, dot(r, r));
            }
        }
    }
    return sqrt(d);
}
fn cn_cap(f: f32) -> f32 {
    return sqrt(max(0.0, 1.0 - f * f / 0.8));
}
fn pass_main(id: vec3<u32>) -> vec4<f32> {
    let p = (vec3<f32>(id) + 0.5) / 128.0;
    let pf = cn_grad(p * 4.0, 4) * 0.55 + cn_grad(p * 8.0, 8) * 0.3 + cn_grad(p * 16.0, 16) * 0.15;
    // Rounded caps over the cells' feature points: cauliflower billows with
    // creases between them (1 - distance would make cones).
    let w1 = cn_cap(cn_cell(p * 4.0, 4));
    let w2 = cn_cap(cn_cell(p * 8.0, 8));
    let w3 = cn_cap(cn_cell(p * 16.0, 16));
    // r: big billows (caps bent by the gradient noise); g: small billows;
    // b: smooth noise; a: fine fibres for the wisps.
    let billow = clamp(w1 * 0.7 + w2 * 0.15 + (pf * 0.5 + 0.5) * 0.25 - 0.05, 0.0, 1.0);
    let small = clamp(w2 * 0.65 + w3 * 0.35, 0.0, 1.0);
    let fib = cn_grad(p * 8.0, 8) * 0.5 + cn_grad(p * 16.0, 16) * 0.3 + cn_grad(p * 32.0, 32) * 0.2;
    return vec4<f32>(billow, small, pf * 0.6 + 0.5, fib * 0.6 + 0.5);
}
";

const SHAPE_WGSL: &str = "
// r: the dense lobes' smooth signed distance; g: the wisp field; b: the
// vapour field (1 at a lobe's centre, 0 at its edge); a: the distance to
// anything at all (the march's empty-space skip). All in cloud widths.
fn pass_main(id: vec3<u32>) -> vec4<f32> {
    let dims = vec3<f32>(textureDimensions(out_tex));
    let nd = u32(data[3].x);
    let nv = u32(data[3].y);
    let nw = u32(data[3].z);
    let w = max(data[1].w, 1e-3);
    let p = data[0].xyz + (vec3<f32>(id) + 0.5) / dims * (data[1].xyz - data[0].xyz);
    var d = 1e9;
    var any_d = 1e9;
    var wisp = 0.0;
    var vap = 0.0;
    // A narrow blend (round 3: 0.03 of the width fused the lobes into one
    // smooth, sculpted mass with no creases between the billows; round 4:
    // narrower again, crisper creases).
    let k = 0.007 * w;
    for (var i = 0u; i < nd; i++) {
        let l = data[4u + i];
        let di = length(p - l.xyz) - l.w;
        let h = clamp(0.5 + 0.5 * (di - d) / k, 0.0, 1.0);
        d = mix(di, d, h) - k * h * (1.0 - h);
    }
    any_d = d;
    for (var i = nd; i < nd + nv; i++) {
        let l = data[4u + i];
        let q = (p - l.xyz) / (l.w * vec3<f32>(1.3, 0.95, 1.1));
        vap = max(vap, 1.0 - dot(q, q));
        any_d = min(any_d, length(p - l.xyz) - l.w * 1.3);
    }
    for (var i = nd + nv; i < nd + nv + nw; i++) {
        let l = data[4u + i];
        let q = (p - l.xyz) / (l.w * vec3<f32>(1.15, 0.9, 1.0));
        wisp = max(wisp, 1.0 - dot(q, q));
        any_d = min(any_d, length(p - l.xyz) - l.w * 1.15);
    }
    return vec4<f32>(d / w, clamp(wisp, 0.0, 1.0), clamp(vap, 0.0, 1.0), any_d / w);
}
";

const LIGHT_WGSL: &str = "
fn cl_coarse(p: vec3<f32>) -> f32 {
    let uvw = cl_box_uvw(p);
    if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) { return 0.0; }
    let s = textureSampleLevel(shape, samp, uvw, 0.0);
    let yb = (p.y - data[2].w) / max(data[1].w, 1e-3);
    return clamp(0.5 - s.r / 0.03, 0.0, 1.0) * smoothstep(-0.2, -0.08, yb) + s.g * 0.05 + s.b * 0.25;
}
fn pass_main(id: vec3<u32>) -> vec4<f32> {
    let dims = vec3<f32>(textureDimensions(out_tex));
    let w = max(data[1].w, 1e-3);
    let p = data[0].xyz + (vec3<f32>(id) + 0.5) / dims * (data[1].xyz - data[0].xyz);
    let l = normalize(frame.sun.xyz + vec3<f32>(0.0, 1e-4, 0.0));
    var od = 0.0;
    var t = 0.0;
    var dt = 0.006 * w;
    for (var i = 0; i < 26; i++) {
        t += dt;
        od += cl_coarse(p + l * t) * dt / w;
        dt *= 1.14;
    }
    var odu = 0.0;
    t = 0.0;
    dt = 0.01 * w;
    for (var i = 0; i < 14; i++) {
        t += dt;
        odu += cl_coarse(p + vec3<f32>(0.0, t, 0.0)) * dt / w;
        dt *= 1.18;
    }
    return vec4<f32>(od, odu, 0.0, 1.0);
}
";

const DRAW_WGSL: &str = "
fn cd_hg(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / pow(max(1.0 + g2 - 2.0 * g * mu, 1e-4), 1.5);
}
// Two fixed rotations for the fine noise. The tile repeats every few
// billows, so the fine detail is two samples at unrelated scales and turns
// (round 1's review: a regular diagonal pattern on the shaded lobes).
fn cd_rot_a(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(0.8 * v.x - 0.36 * v.y + 0.48 * v.z, 0.6 * v.x + 0.48 * v.y - 0.64 * v.z, 0.8 * v.y + 0.6 * v.z);
}
fn cd_rot_b(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(0.36 * v.x + 0.48 * v.y - 0.8 * v.z, -0.8 * v.x + 0.6 * v.y, 0.48 * v.x + 0.64 * v.y + 0.6 * v.z);
}
// Density (x), the distance to anything in widths (y) and the soft fields
// (wisps and vapour, z) at p. `detail` 0 is the sun probes' view: no finest
// billows and a softer surface (`psoft` times), so a probe's value does not flip
// between in and out with each pixel's jittered sample (round 3: that flip
// stippled the folds).
fn cd_density(p: vec3<f32>, detail: f32) -> vec3<f32> {
    let w = max(data[1].w, 1e-3);
    let uvw = cl_box_uvw(p);
    // Outside the box (a ray entering on its very edge): one plain step, no
    // skip (a skip from here would jump the whole cloud).
    if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) { return vec3<f32>(0.0, 0.0, 0.0); }
    let s = textureSampleLevel(shape, samp, uvw, 0.0);
    if (s.a > 0.08) { return vec3<f32>(0.0, s.a, 0.0); }
    let q = (p - data[0].xyz) / w * param_scale();
    let n1 = textureSampleLevel(noise, samp_wrap, q, 0.0);
    let n2 = textureSampleLevel(noise, samp_wrap, cd_rot_a(q) * 2.71 + vec3<f32>(0.37, 0.11, 0.73), 0.0);
    let n3 = textureSampleLevel(noise, samp_wrap, cd_rot_b(q) * 4.43 + vec3<f32>(0.61, 0.29, 0.17), 0.0);
    // The fine billows from the larger of the two samples only (round 3:
    // the smallest one, finer than the march's step resolves, stippled the
    // folds).
    let fine = n2.g - 0.5;
    let fibre = (n2.a - 0.5) * 0.6 + (n3.a - 0.5) * 0.4;
    let yb = (p.y - data[2].w) / w;
    // Billows push the surface out and bite into it; at the very bottom they
    // give way to softer, frayed fibres (round 3: only the lowest lumps; the
    // low front lumps keep crisp lit tops).
    let low = 1.0 - smoothstep(-0.1, 0.06, yb);
    // Round 4: how busy the billows are varies across the cloud (a broad
    // noise), so some faces are big smooth billows and others crowded small
    // ones, not one even popcorn of equal bumps.
    let n0 = textureSampleLevel(noise, samp_wrap, q * 0.31 + vec3<f32>(0.53, 0.19, 0.41), 0.0);
    let busy = smoothstep(0.3, 0.7, n0.b);
    let disp = ((n1.r - 0.5) * param_billow() * mix(0.85, 1.1, busy)
        + (n1.g - 0.5) * param_billow() * param_small() * mix(0.55, 1.4, busy)) * (1.0 - 0.5 * low)
        + fine * param_fine() * detail * (1.0 + 1.5 * low) + fibre * param_fray() * low;
    let soft = (param_soft() * (1.0 + 3.0 * low) + param_fray() * 0.25 * low) * mix(param_psoft(), 1.0, detail);
    var d = clamp((-s.r + disp) / soft, 0.0, 1.0);
    // The base: lumps may hang a little under it, their ends frayed.
    d *= smoothstep(-0.2, -0.08, yb + (n1.b - 0.5) * 0.12);
    if (s.b > 0.002) {
        // Vapour (round 3): thinner cloud in lumps with gaps between them
        // (the big billows carve it), its own edges frayed by the fibres;
        // light gets into it and the sky shows through its thin parts.
        // Lumps all through it, not only at its edge: the big billows'
        // caps bent by the small ones and the fibres; gaps where they dip,
        // more of them towards the field's edge.
        let lump = n1.r * 0.7 + n1.g * 0.3 + fibre * 0.6 + (s.b - 0.5) * 0.4;
        let gp = param_gaps();
        let v = smoothstep(0.0, 0.3, s.b) * smoothstep(gp - 0.18, gp + 0.12, lump);
        let vb = smoothstep(-0.16, -0.03, yb + (n1.b - 0.5) * 0.2 + (n1.r - 0.5) * 0.12);
        d = max(d, v * param_vapour() * vb);
    }
    if (s.g > 0.002) {
        // Curling fibres: the fibre noise, turned, stretched a little along
        // the cloud and bent by the billows; faint (round 3: translucent).
        // Round 4: stretched along the cloud (most curls run along it), so
        // they break into filaments, not soft blobs.
        let qw = q * vec3<f32>(0.3, 1.8, 1.4) + (n1.rgb - 0.5) * 0.8;
        let f = textureSampleLevel(noise, samp_wrap, qw, 0.0).a * 0.75 + n2.a * 0.25;
        d = max(d, s.g * smoothstep(0.46, 0.72, f + (n1.r - 0.5) * 0.25) * param_wisps());
    }
    return vec3<f32>(d, s.a, s.g + s.b);
}
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    if (data[0].w < 0.5) { return vec4<f32>(0.0); }
    let bmin = data[0].xyz;
    let bmax = data[1].xyz;
    let w = max(data[1].w, 1e-3);
    let ro = frame.camera.xyz;
    let rd = view_ray(uv);
    let inv = 1.0 / select(rd, vec3<f32>(1e-6), abs(rd) < vec3<f32>(1e-6));
    let ta = (bmin - ro) * inv;
    let tb = (bmax - ro) * inv;
    let tn = min(ta, tb);
    let tf = max(ta, tb);
    let t0 = max(max(max(tn.x, tn.y), tn.z), 0.0);
    var t1 = min(min(tf.x, tf.y), tf.z);
    if (t1 <= t0) { return vec4<f32>(0.0); }
    let dz = textureLoad(depth, vec2<i32>(px), 0);
    if (dz > 0.0) { t1 = min(t1, length(world_at(uv, dz) - ro)); }
    let l = normalize(frame.sun.xyz + vec3<f32>(0.0, 1e-4, 0.0));
    let mu = dot(rd, l);
    let sunc = frame.sun_color.rgb * frame.sun.w * param_sun();
    let sky = frame.ambient.rgb * param_sky();
    let albedo = data[2].rgb;
    let sigma = param_density();
    let g = param_forward();
    // A different start per pixel: white noise (round 1's interleaved
    // gradient noise drew faint diagonal lines over the shaded lobes).
    let j = cl_hash(vec3<i32>(i32(px.x), i32(px.y), 17));
    let dt = param_step() * w;
    var t = t0 + j * dt;
    var tr = 1.0;
    var col = vec3<f32>(0.0);
    let steps = i32(param_steps());
    let rf = param_refine();
    // Round 3 (round 2's review: fine stippling over the folds): where a ray
    // meets a dense surface while the pixel is still clear, it backs up and
    // takes short steps until the surface has mostly hidden what is behind,
    // so the surface's place no longer jitters by a whole step per pixel.
    var armed = true;
    var fine = false;
    var inside = false;
    var was_empty = true;
    for (var i = 0; i < steps; i++) {
        if (t >= t1 || tr < 0.008) { break; }
        let p = ro + rd * t;
        let ds = cd_density(p, 1.0);
        if (ds.x <= 0.0) {
            // Empty: skip by the distance field, less the billows' reach, in
            // whole steps (the start's jitter stays, no banding); short
            // steps while refining.
            if (inside) {
                armed = true;
                fine = false;
                inside = false;
            }
            let skip = dt * select(1.0, max(1.0, floor((ds.y - 0.07) * w / dt)), ds.z < 0.002);
            t += select(skip, dt * rf, fine && tr > 0.3);
            was_empty = true;
            continue;
        }
        if (armed && tr > 0.3 && ds.x > select(0.5, 0.05, ds.z < 0.002)) {
            armed = false;
            fine = true;
            if (was_empty) {
                t -= dt * (1.0 - rf);
                was_empty = false;
                continue;
            }
        }
        was_empty = false;
        inside = true;
        let h = select(dt, dt * rf, fine && tr > 0.3);
        let lt = textureSampleLevel(light, samp, cl_box_uvw(p), 0.0);
        // The billows' own shadows: three short steps towards the sun (the
        // third reaches over a fold, so the one above shades the one below).
        let d1 = cd_density(p + l * (0.012 * w), 0.0).x;
        let d2 = cd_density(p + l * (0.036 * w), 0.0).x;
        let d3 = cd_density(p + l * (0.08 * w), 0.0).x;
        let od = lt.r * param_deep() + (d1 * 0.016 + d2 * 0.03 + d3 * 0.045) * param_local();
        // Powder (round 4; round 3's review: broad white tops with no folds
        // in them): the outermost skin facing the sun scatters less of it
        // towards the eye, so sunlit faces dim a little and each billow's
        // outline shows against the next; sides and the base keep their
        // light. From the probes' soft view, so no speckle.
        let pw = 1.0 - param_powder() * exp(-sigma * 2.0 * (d1 * 0.016 + d2 * 0.03));
        var sun_l = 0.0;
        var a = 1.0;
        var b = 1.0;
        var c = 1.0;
        for (var k = 0; k < 3; k++) {
            let ph = mix(cd_hg(mu, g * c), cd_hg(mu, -0.25 * c), 0.3);
            sun_l += b * ph * exp(-sigma * a * od);
            a *= 0.25;
            b *= param_multi();
            c *= 0.5;
        }
        // Sky light from above, dimmer deep inside and towards the base, and
        // the ground's bounce from below (a cloud's base is lighter than
        // the sky behind it, not black).
        let yb = clamp((p.y - data[2].w) / w / 0.5, 0.0, 1.0);
        let open = mix(0.2, 1.0, exp(-sigma * 0.12 * lt.g));
        // Round 5 (round 4's review: a flat grey sheet under the tower's
        // side, the sky light there clamped to one value): what lies just
        // above a point shades it, so overhangs, pockets and the vapour's
        // lumps each take their own value.
        let du = cd_density(p + vec3<f32>(0.0, 0.03 * w, 0.0), 0.0).x;
        let amb = sky
            * (open * mix(param_bottom(), 1.0, yb) + param_ground() * (1.0 - yb))
            * (1.0 - param_occl() * du);
        let lin = albedo * (sunc * sun_l * pw + amb);
        let ext = exp(-ds.x * sigma * h / w);
        col += tr * lin * (1.0 - ext);
        tr *= ext;
        t += h;
    }
    let alpha = 1.0 - tr;
    if (alpha <= 1e-4) { return vec4<f32>(0.0); }
    return vec4<f32>(col / alpha, alpha);
}
";

/// Each placed cloud's position, yaw and scale.
type Placed = Vec<([f32; 3], f32, f32)>;

/// Draws the placed clouds as a volume (hosted by the procedural module).
#[derive(Debug, Default)]
pub struct CloudVolume {
    /// Whether the passes are in (a build or app without the render module
    /// has none, and nothing is drawn).
    ready: bool,
    /// The param revision and placements the data was made for.
    made_at: Option<(u64, Placed)>,
    /// The asset's own params (only their changes remake the cloud).
    ids: Vec<ParamId>,
    sun: [f32; 3],
}

impl CloudVolume {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EngineModule for CloudVolume {
    fn id(&self) -> &'static str {
        "evani.module.procedural.clouds"
    }

    fn label(&self) -> &'static str {
        "procedural-clouds"
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        if ctx.world.try_res::<GamePasses>().is_none() {
            return Ok(());
        }
        game_pass::add_target(
            ctx,
            GameTarget::volume(NOISE, Format::Rgba8Unorm, [128, 128, 128]),
        )?;
        game_pass::add_target(
            ctx,
            GameTarget::volume(SHAPE, Format::Rgba16Float, SHAPE_DIMS),
        )?;
        game_pass::add_target(
            ctx,
            GameTarget::volume(LIGHT, Format::Rgba16Float, LIGHT_DIMS),
        )?;
        game_pass::add(
            ctx,
            GamePass::compute(NOISE_PASS, format!("{COMMON_WGSL}{NOISE_WGSL}"))
                .writes(NOISE)
                .once(),
        )?;
        game_pass::add(
            ctx,
            GamePass::compute(SHAPE_PASS, format!("{COMMON_WGSL}{SHAPE_WGSL}"))
                .writes(SHAPE)
                .once(),
        )?;
        game_pass::add(
            ctx,
            GamePass::compute(LIGHT_PASS, format!("{COMMON_WGSL}{LIGHT_WGSL}"))
                .reads("shape", SHAPE)
                .writes(LIGHT)
                .once(),
        )?;
        game_pass::add(
            ctx,
            GamePass::fullscreen(DRAW_PASS, format!("{COMMON_WGSL}{DRAW_WGSL}"))
                .at(Stage::AfterOpaque)
                .reads("shape", SHAPE)
                .reads("light", LIGHT)
                .reads("noise", NOISE)
                .reads_depth("depth")
                .blend(PassBlend::Alpha)
                .param(
                    PassParam::f32("density", 180.0, 1.0, 400.0)
                        .describe("How thick the cloud is: extinction per cloud width"),
                )
                .param(
                    PassParam::f32("sun", 0.52, 0.0, 4.0)
                        .describe("The sun's light on the cloud (times the scene's sun)"),
                )
                .param(
                    PassParam::f32("sky", 1.0, 0.0, 4.0)
                        .describe("The sky's light on the cloud (times the scene's ambient)"),
                )
                .param(
                    PassParam::f32("billow", 0.04, 0.0, 0.3)
                        .describe("How far the billows push and bite the surface (cloud widths)"),
                )
                .param(
                    PassParam::f32("small", 0.4, 0.0, 1.5)
                        .describe("The small billows on the big ones (times the billows' reach)"),
                )
                .param(
                    PassParam::f32("psoft", 2.5, 1.0, 10.0)
                        .describe("How much softer the surface is to the sun probes (no speckle; crisper shading when lower)"),
                )
                .param(
                    PassParam::f32("fine", 0.006, 0.0, 0.1)
                        .describe("The fine erosion at the edges (cloud widths)"),
                )
                .param(
                    PassParam::f32("scale", 4.2, 0.5, 10.0)
                        .describe("Billow noise tiles per cloud width"),
                )
                .param(
                    PassParam::f32("soft", 0.003, 0.001, 0.1)
                        .describe("How soft the surface is (cloud widths)"),
                )
                .param(
                    PassParam::f32("wisps", 0.5, 0.0, 1.0)
                        .describe("How dense the frayed wisps are"),
                )
                .param(
                    PassParam::f32("vapour", 0.18, 0.0, 1.0)
                        .describe("How dense the vapour under the heaps and at the shoulders is"),
                )
                .param(
                    PassParam::f32("gaps", 0.8, 0.3, 1.2)
                        .describe("How much of the vapour is gaps between its lumps"),
                )
                .param(
                    PassParam::f32("fray", 0.05, 0.0, 0.3)
                        .describe("How far the base frays into translucent fibres (cloud widths)"),
                )
                .param(PassParam::f32("deep", 0.43, 0.0, 2.0).describe(
                    "How much the whole cloud shades itself (the sun's depth through it)",
                ))
                .param(
                    PassParam::f32("local", 0.8, 0.0, 3.0)
                        .describe("How much each billow shades the ones under it (the creases)"),
                )
                .param(
                    PassParam::f32("powder", 0.35, 0.0, 1.0).describe(
                        "How much dimmer the sunlit outer skin is (folds in the white tops)",
                    ),
                )
                .param(
                    PassParam::f32("occl", 0.15, 0.0, 1.0)
                        .describe("How much what lies just above shades the sky light (overhangs, pockets)"),
                )
                .param(
                    PassParam::f32("forward", 0.4, 0.0, 0.9)
                        .describe("Forward scattering (the silver lining towards the sun)"),
                )
                .param(
                    PassParam::f32("multi", 0.28, 0.0, 0.95)
                        .describe("Multiple scattering: light that bounces inside the cloud"),
                )
                .param(
                    PassParam::f32("bottom", 0.7, 0.0, 1.0)
                        .describe("The sky's light left at the base (the darker underside)"),
                )
                .param(
                    PassParam::f32("ground", 0.25, 0.0, 2.0)
                        .describe("The ground's bounce light on the base (times the sky's)"),
                )
                .param(
                    PassParam::f32("step", 0.003, 0.001, 0.05)
                        .describe("The march's step inside the cloud (cloud widths)"),
                )
                .param(
                    PassParam::f32("refine", 0.33, 0.1, 1.0)
                        .describe("The short step at a dense surface (times the step; 1: none)"),
                )
                .param(
                    PassParam::f32("steps", 480.0, 16.0, 640.0).describe("Most steps per pixel"),
                ),
        )?;
        self.ready = true;
        Ok(())
    }

    fn frame(&mut self, world: &mut World) {
        if !self.ready {
            return;
        }
        if self.ids.is_empty()
            && let (Some(registry), Some(reg)) = (
                world.try_res::<Registry>(),
                world.try_res::<ParamRegistry>(),
            )
        {
            self.ids = registry
                .param_names(FEATURE)
                .iter()
                .filter_map(|n| reg.id(n))
                .collect();
        }
        let (rev, changed, sun, placed) = {
            let Some(scene) = world.try_res::<ProcScene>() else {
                return;
            };
            let reg = world.res::<ParamRegistry>();
            let sun = match reg
                .id("render.light.direction")
                .and_then(|id| reg.value(id))
            {
                Some(ParamValue::Vec3(v) | ParamValue::Color(v)) => v,
                _ => [0.0, 1.0, 0.0],
            };
            let placed: Placed = scene
                .placements
                .iter()
                .filter(|p| p.feature == FEATURE)
                .map(|p| (p.position, p.yaw, p.scale))
                .collect();
            let changed = self
                .made_at
                .as_ref()
                .is_none_or(|(r, _)| reg.any_changed_since(&self.ids, *r));
            (reg.revision(), changed, sun, placed)
        };
        let fresh = !changed && self.made_at.as_ref().is_some_and(|(_, p)| *p == placed);
        if !fresh {
            let data = self.data(world, &placed);
            if let Some(mut gp) = world.try_res_mut::<GamePasses>() {
                for pass in [SHAPE_PASS, LIGHT_PASS, DRAW_PASS] {
                    if let Some(d) = gp.data_mut(pass) {
                        d.clone_from(&data);
                    }
                }
                gp.rerun(SHAPE_PASS);
                gp.rerun(LIGHT_PASS);
            }
            self.made_at = Some((rev, placed));
            self.sun = sun;
        } else if sun != self.sun {
            if let Some(mut gp) = world.try_res_mut::<GamePasses>() {
                gp.rerun(LIGHT_PASS);
            }
            self.sun = sun;
        }
    }
}

impl CloudVolume {
    /// Every placed, switched-on cloud's lobes in world space, behind the
    /// header (box, count, width, colour, base).
    fn data(&self, world: &World, placed: &[([f32; 3], f32, f32)]) -> Vec<[f32; 4]> {
        // Dense lobes, vapour and wisps, each kind together (the shape pass
        // reads them in that order by the counts in the fourth record).
        let mut kinds: [Vec<Lobe>; 3] = Default::default();
        let (mut w, mut base, mut color) = (1.0f32, f32::MAX, [0.95, 0.947, 0.942]);
        if let (Some(registry), Some(reg)) = (
            world.try_res::<Registry>(),
            world.try_res::<ParamRegistry>(),
        ) && registry.is_on(&reg, FEATURE)
            && let Some((v, seed)) = registry.values(&reg, FEATURE)
        {
            let local = cumulus(v.f("size"), v.f("height"), v.f("tower"), seed as u32);
            color = v.color("color");
            for &(at, yaw, scale) in placed {
                let (sy, cy) = yaw.sin_cos();
                w = w.max(v.f("size") * scale);
                base = base.min(at[1] + v.f("height") * scale);
                for (out, from) in kinds
                    .iter_mut()
                    .zip([&local.lobes, &local.vapour, &local.wisps])
                {
                    for l in from {
                        let (x, z) = (l[0] * cy + l[2] * sy, -l[0] * sy + l[2] * cy);
                        out.push([
                            at[0] + x * scale,
                            at[1] + l[1] * scale,
                            at[2] + z * scale,
                            l[3] * scale,
                        ]);
                    }
                }
            }
        }
        if kinds.iter().all(Vec::is_empty) {
            return vec![[0.0; 4]; HEADER];
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        // The vapour's and the wisps' fields reach past their radius.
        for (ls, reach) in kinds.iter().zip([1.0, 1.3, 1.15]) {
            for l in ls {
                for k in 0..3 {
                    lo[k] = lo[k].min(l[k] - l[3] * reach);
                    hi[k] = hi[k].max(l[k] + l[3] * reach);
                }
            }
        }
        let m = 0.08 * w;
        let mut room = game_pass::MAX_DATA - HEADER;
        let counts = kinds.each_ref().map(|ls| {
            let n = ls.len().min(room);
            room -= n;
            n
        });
        let n: usize = counts.iter().sum();
        let mut d = Vec::with_capacity(n + HEADER);
        d.push([lo[0] - m, lo[1] - m, lo[2] - m, n as f32]);
        d.push([hi[0] + m, hi[1] + m, hi[2] + m, w]);
        d.push([color[0], color[1], color[2], base]);
        d.push([counts[0] as f32, counts[1] as f32, counts[2] as f32, 0.0]);
        for (ls, &c) in kinds.iter().zip(&counts) {
            d.extend_from_slice(&ls[..c]);
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cumulus_is_seeded_flat_based_and_tallest_at_its_tower() {
        let c = cumulus(8.0, 6.0, 0.6, 3);
        assert_eq!(c, cumulus(8.0, 6.0, 0.6, 3), "deterministic");
        assert_ne!(c, cumulus(8.0, 6.0, 0.6, 4), "the seed changes it");
        let a = &c.lobes;
        // The shape pass loops over every lobe for each of its 1.7 million
        // cells: keep the count bounded.
        let all = a.len() + c.vapour.len() + c.wisps.len();
        assert!(
            a.len() > 200 && all < 3000,
            "{} lobes, {all} in all",
            a.len()
        );
        assert!(c.vapour.len() > 20 && c.wisps.len() > 20);
        assert!(
            c.lobes
                .iter()
                .chain(&c.vapour)
                .chain(&c.wisps)
                .all(|l| l[3] > 0.0)
        );
        let top = a.iter().map(|l| l[1] + l[3]).fold(f32::MIN, f32::max);
        // The child billows ride a little over the heap's own top.
        assert!(
            (top - 6.0 - 0.6 * 8.0).abs() < 0.12 * 8.0,
            "tower top {top}"
        );
        // Nothing hangs far under the base: the dense lumps, the vapour
        // and the lowest wisps only a little under it.
        assert!(
            a.iter().all(|l| l[1] > 6.0 - 0.12 * 8.0),
            "dense heaps over the base"
        );
        assert!(
            c.vapour
                .iter()
                .chain(&c.wisps)
                .all(|l| l[1] > 6.0 - 0.12 * 8.0),
            "nothing far under the base"
        );
        // The wisps curl out past the dense heaps' ends or hang at the edges.
        let reach = a.iter().map(|l| l[0].abs() + l[3]).fold(0.0f32, f32::max);
        assert!(
            c.wisps.iter().any(|l| l[0].abs() > reach),
            "curls past the ends"
        );
    }

    /// The gate for game passes: the four cloud passes are accepted and
    /// their whole WGSL modules compile.
    #[test]
    fn the_cloud_passes_check_clean() {
        let mut a = evani_core::App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_module(crate::module());
        a.start();
        let n = game_pass::check(&a.world);
        assert!(matches!(n, Ok(k) if k >= 4), "{n:?}");
        let gp = a.world.res::<GamePasses>();
        for p in [NOISE_PASS, SHAPE_PASS, LIGHT_PASS, DRAW_PASS] {
            assert!(gp.pass(p).is_some(), "{p}");
        }
    }
}
