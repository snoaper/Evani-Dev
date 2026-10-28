//! The cloth detail map, made in code at load (no weave file ships): one
//! 64x64 RGBA8 tile, linear data, seamless. R and G are a plain weave
//! (brightness, thread height): warp and weft threads with a rounded
//! cross-section that pass over and under each other along a cosine. B and
//! A are a knit (brightness, height): rows of V-shaped stitches whose legs
//! reach up into the row above. Brightness is centred on 0.5 (the tile's
//! mean), so a cloth keeps its average colour and only gains texture.
//! Same inputs, same bytes.

use evani_core::plugin::{GenRequest, GeneratedAsset};
use evani_core::rng::Pcg32;

/// Edge of the tile the renderer makes.
pub const SIZE: u32 = 64;
/// Threads (plain) and stitches (knit) per tile edge.
pub const THREADS: u32 = 8;
/// The generator name for looks that lay a tiling detail map
/// (`surface.detail`-compatible: R brightness, G height).
pub const DETAIL_GENERATOR: &str = "surface.weave";

/// A small, well-mixed hash in -0.5..0.5 for per-thread variation.
fn vary(i: u32, axis: u32, seed: u32) -> f32 {
    let mut h = i.wrapping_mul(0x9e37_79b1)
        ^ axis.wrapping_mul(0x85eb_ca77)
        ^ seed.wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0 - 0.5
}

/// Rounded thread cross-section: 1 on the crown, 0 at the edges.
fn round(u: f32) -> f32 {
    let x = 2.0 * u - 1.0;
    (1.0 - x * x).max(0.0).sqrt()
}

/// One texel of a plain weave at cell coordinates (`fx`, `fy`, in threads;
/// `threads` is even, so the over-under pattern tiles): (brightness, height).
fn plain_at(fx: f32, fy: f32, _cells: u32, seed: u32) -> (f32, f32) {
    let (ix, iy) = (fx.floor(), fy.floor());
    let (ux, uy) = (fx - ix, fy - iy);
    let sx = if (ix as i64) % 2 == 0 { 1.0 } else { -1.0 };
    let sy = if (iy as i64) % 2 == 0 { 1.0 } else { -1.0 };
    let pi = std::f32::consts::PI;
    // Warp thread `ix` runs along y and is on top over weft rows where
    // ix + iy is even; the weft is on top where it is odd.
    let z_warp = sx * (pi * (fy - 0.5)).cos();
    let z_weft = -sy * (pi * (fx - 0.5)).cos();
    let warp = (0.5 + 0.3 * z_warp) * round(ux);
    let weft = (0.5 + 0.3 * z_weft) * round(uy);
    let (top, v) = if warp >= weft {
        (warp, vary(ix as u32, 0, seed))
    } else {
        (weft, vary(iy as u32, 1, seed))
    };
    // Fine fibres twisted round each thread.
    let twist = if warp >= weft {
        (fy * 2.0 * pi * 3.0 + ux * pi).sin()
    } else {
        (fx * 2.0 * pi * 3.0 + uy * pi).sin()
    };
    let height = top.clamp(0.0, 1.0);
    let bright = 0.18 + 0.7 * height + 0.12 * v + 0.03 * twist * height;
    (bright, height)
}

/// Distance from `p` to the segment `a`-`b`.
fn seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (ex, ey) = (p[0] - a[0] - dx * t, p[1] - a[1] - dy * t);
    (ex * ex + ey * ey).sqrt()
}

/// One texel of a knit at stitch coordinates (`fx`, `fy`; y grows down
/// the tile): (brightness, height). Each stitch is a V of two slanted legs
/// meeting at the bottom centre; the legs reach a little into the rows
/// above and below so neighbouring stitches interlock.
fn knit_at(fx: f32, fy: f32, cells: u32, seed: u32) -> (f32, f32) {
    let (ix, iy) = (fx.floor(), fy.floor());
    let (ux, uy) = (fx - ix, fy - iy);
    let mut best = 0.0f32;
    let mut best_v = 0.0;
    for dy in [-1.0f32, 0.0, 1.0] {
        let p = [ux, uy - dy];
        for (leg, a, b) in [
            (0u32, [0.08, -0.25], [0.5, 1.05]),
            (1u32, [0.92, -0.25], [0.5, 1.05]),
        ] {
            let d = seg(p, a, b);
            let h = round((0.5 + d / 0.46).min(1.0)) * if d < 0.23 { 1.0 } else { 0.0 };
            if h > best {
                best = h;
                best_v = vary(
                    (iy - dy).rem_euclid(cells as f32) as u32 * 1024 + ix as u32,
                    2 + leg,
                    seed,
                );
            }
        }
    }
    let bright = 0.15 + 0.75 * best + 0.1 * best_v;
    (bright, best)
}

fn to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `size` x `size` samples of `f` over `cells` cells per edge, the
/// brightness shifted so its mean is 0.5.
fn bake(
    size: u32,
    cells: u32,
    seed: u32,
    f: fn(f32, f32, u32, u32) -> (f32, f32),
) -> Vec<(f32, f32)> {
    let size = size.max(8);
    let per = size as f32 / cells as f32;
    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            out.push(f(
                (x as f32 + 0.5) / per,
                (y as f32 + 0.5) / per,
                cells,
                seed,
            ));
        }
    }
    let mean = out.iter().map(|s| s.0).sum::<f32>() / out.len() as f32;
    for s in &mut out {
        s.0 = s.0 - mean + 0.5;
    }
    out
}

/// The renderer's tile: 64x64 RGBA8, RG plain weave, BA knit.
pub fn weave_map(seed: u32) -> Vec<u8> {
    let plain = bake(SIZE, THREADS, seed, plain_at);
    let knit = bake(SIZE, THREADS, seed ^ 0x5bd1, knit_at);
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for (p, k) in plain.iter().zip(&knit) {
        out.extend_from_slice(&[to_u8(p.0), to_u8(p.1), to_u8(k.0), to_u8(k.1)]);
    }
    out
}

/// A `surface.detail`-style map (R brightness, G height, B 128, A 255) of
/// a plain weave, or a knit when `knit`, at any even size.
pub fn detail_map(size: u32, knit: bool, seed: u32) -> Vec<u8> {
    let size = size.clamp(16, 1024) & !1;
    let cells = (size / 8).clamp(2, 64) & !1;
    let s = if knit {
        bake(size, cells, seed, knit_at)
    } else {
        bake(size, cells, seed, plain_at)
    };
    let mut out = Vec::with_capacity(s.len() * 4);
    for (b, h) in s {
        out.extend_from_slice(&[to_u8(b), to_u8(h), 128, 255]);
    }
    out
}

/// The generator a look registers under [`DETAIL_GENERATOR`]: a plain
/// weave, or a knit when the request's `knit` param is above 0.5.
pub fn detail_generator(req: &GenRequest, _rng: &mut Pcg32) -> GeneratedAsset {
    let size = req.size.clamp(16, 1024) & !1;
    let knit = req.params.get("knit").is_some_and(|&k| k > 0.5);
    GeneratedAsset::Texture {
        width: size,
        height: size,
        rgba8: detail_map(size, knit, req.seed as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_same_bytes() {
        assert_eq!(weave_map(3), weave_map(3));
        assert_ne!(
            weave_map(3),
            weave_map(4),
            "the seed changes the thread variation"
        );
        assert_eq!(weave_map(0).len(), (SIZE * SIZE * 4) as usize);
        assert_eq!(detail_map(128, true, 9), detail_map(128, true, 9));
    }

    /// Mean of channel `c` over the tile, 0..1.
    fn mean(map: &[u8], c: usize) -> f32 {
        map.chunks(4).map(|p| p[c] as f32 / 255.0).sum::<f32>() / (map.len() / 4) as f32
    }

    #[test]
    fn brightness_is_centred_and_both_patterns_have_relief() {
        let m = weave_map(1);
        for c in [0, 2] {
            assert!(
                (mean(&m, c) - 0.5).abs() < 0.02,
                "channel {c} mean {}",
                mean(&m, c)
            );
        }
        for c in [1, 3] {
            let (lo, hi) = m
                .chunks(4)
                .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
            assert!(
                lo < 100 && hi > 150 && hi - lo > 100,
                "channel {c}: height from {lo} to {hi}"
            );
        }
        assert_ne!(
            m.chunks(4).map(|p| p[1]).collect::<Vec<_>>(),
            m.chunks(4).map(|p| p[3]).collect::<Vec<_>>(),
            "plain and knit differ"
        );
    }

    /// Heights across the wrap seam differ no more than across any
    /// neighbouring pair inside the tile (the tile repeats seamlessly).
    #[test]
    fn tiles_seamlessly() {
        let m = weave_map(5);
        let at =
            |x: u32, y: u32, c: usize| m[((y % SIZE) * SIZE + (x % SIZE)) as usize * 4 + c] as i32;
        for c in [1, 3] {
            let mut inner = 0;
            let mut seam = 0;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let d = (at(x, y, c) - at(x + 1, y, c))
                        .abs()
                        .max((at(x, y, c) - at(x, y + 1, c)).abs());
                    if x == SIZE - 1 || y == SIZE - 1 {
                        seam = seam.max(d);
                    } else {
                        inner = inner.max(d);
                    }
                }
            }
            assert!(
                seam <= inner,
                "channel {c}: seam step {seam}, inner step {inner}"
            );
        }
    }

    #[test]
    fn plain_weave_alternates_over_and_under() {
        // At a cell centre the thread on top is the warp where ix + iy is
        // even and the weft where it is odd: the heights there match, the
        // brightness differs only by the per-thread variation.
        let (_, h00) = plain_at(0.5, 0.5, 8, 0);
        let (_, h10) = plain_at(1.5, 0.5, 8, 0);
        assert!(
            h00 > 0.7 && h10 > 0.7,
            "both cells show a thread crown: {h00} {h10}"
        );
        // Between threads (cell corners) it is low.
        let (_, gap) = plain_at(1.0, 1.0, 8, 0);
        assert!(gap < 0.1, "gap {gap}");
    }

    #[test]
    fn generator_matches_the_detail_layout() {
        let mut rng = Pcg32::new(1, 1);
        let mut req = GenRequest {
            name: DETAIL_GENERATOR.into(),
            size: 64,
            seed: 2,
            params: Default::default(),
        };
        let GeneratedAsset::Texture {
            width,
            height,
            rgba8,
        } = detail_generator(&req, &mut rng)
        else {
            panic!("a texture")
        };
        assert_eq!((width, height, rgba8.len()), (64, 64, 64 * 64 * 4));
        assert!(rgba8.chunks(4).all(|p| p[2] == 128 && p[3] == 255));
        req.params.insert("knit".into(), 1.0);
        let GeneratedAsset::Texture { rgba8: knit, .. } = detail_generator(&req, &mut rng) else {
            panic!("a texture")
        };
        assert_ne!(knit, rgba8);
    }
}
