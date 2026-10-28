//! The load-time bake: each relief pattern rendered once on the CPU into an
//! N x N tile (nothing is downloaded), with a mip chain so distant surfaces
//! stay calm (the kit data has no sampler, so the shader picks the level
//! from the pixel footprint).
//!
//! Every texel is two floats, each an exact integer under 2^24 (so no NaN
//! or rounding can creep in through the f32 storage):
//!
//! * A: the colour, square-root encoded, `r + 256 g + 65536 b` (8 bits each).
//! * B: the slope along u and v (7 bits each, `63` = flat, scaled by the
//!   surface's slope scale), the height (6 bits) and the roughness (4 bits):
//!   `su + 128 sv + 16384 h + 1048576 r`.
//!
//! Slopes are the physical height's derivative (m per m) by wrapped central
//! differences, so the tile's edges match its opposite edges.

use super::pattern::Pattern;

/// The smallest and largest bake size (texels a side).
pub const MIN_SIZE: u32 = 16;
pub const MAX_SIZE: u32 = 256;
/// Mips stop at this size.
pub const MIN_LEVEL: u32 = 4;

/// One baked surface.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceBake {
    /// Level 0's size (texels a side, a power of two).
    pub n: u32,
    /// Mip levels (level k is `n >> k` a side).
    pub levels: u32,
    /// Every level, largest first, row by row: `[A, B]` per texel.
    pub texels: Vec<[f32; 2]>,
    /// The slope that encodes as the largest code (m per m).
    pub slope_scale: f32,
    /// The tile's mean colour (linear) and roughness, for the flat tier and
    /// past the last mip.
    pub mean_color: [f32; 3],
    pub mean_rough: f32,
    /// The tile (m) and relief depth (m) baked.
    pub tile: [f32; 2],
    pub depth: f32,
}

/// Decoded texel values (what the shader reads).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Texel {
    pub color: [f32; 3],
    pub slope: [f32; 2],
    pub height: f32,
    pub rough: f32,
}

/// `size` as the bake will use it: a power of two in range.
pub fn bake_size(size: u32) -> u32 {
    size.clamp(MIN_SIZE, MAX_SIZE).next_power_of_two().min(MAX_SIZE)
}

/// Mip levels for level-0 size `n`.
pub fn levels_for(n: u32) -> u32 {
    let mut l = 1;
    while (n >> l) >= MIN_LEVEL {
        l += 1;
    }
    l
}

/// Texels in levels `0..l` of an `n` tile.
pub fn level_offset(n: u32, l: u32) -> usize {
    (0..l).map(|k| ((n >> k) as usize).pow(2)).sum()
}

/// Every texel of every level.
pub fn texel_count(n: u32) -> usize {
    level_offset(n, levels_for(n))
}

fn q(v: f32, max: u32) -> u32 {
    (v * max as f32 + 0.5).floor().clamp(0.0, max as f32) as u32
}

/// Encode one texel.
pub fn encode(t: &Texel, slope_scale: f32) -> [f32; 2] {
    let c = |x: f32| q(x.max(0.0).sqrt(), 255);
    let a = c(t.color[0]) | (c(t.color[1]) << 8) | (c(t.color[2]) << 16);
    let s = |x: f32| {
        let k = (x / slope_scale).clamp(-1.0, 1.0);
        ((k * 63.0 + 63.0 + 0.5).floor() as u32).min(126)
    };
    let b = s(t.slope[0]) | (s(t.slope[1]) << 7) | (q(t.height, 63) << 14) | (q(t.rough, 15) << 20);
    [a as f32, b as f32]
}

/// Decode one texel (the shader's `rl_decode`).
pub fn decode(e: [f32; 2], slope_scale: f32) -> Texel {
    let (a, b) = (e[0] as u32, e[1] as u32);
    let c = |k: u32| {
        let x = ((a >> k) & 255) as f32 / 255.0;
        x * x
    };
    let s = |k: u32| (((b >> k) & 127) as f32 - 63.0) / 63.0 * slope_scale;
    Texel {
        color: [c(0), c(8), c(16)],
        slope: [s(0), s(7)],
        height: ((b >> 14) & 63) as f32 / 63.0,
        rough: ((b >> 20) & 15) as f32 / 15.0,
    }
}

/// Bake `pattern` at `size` texels a side (rounded to a power of two in
/// [`MIN_SIZE`]..=[`MAX_SIZE`]). Deterministic: the same pattern and size
/// give the same bits everywhere.
pub fn bake(pattern: &Pattern, size: u32) -> SurfaceBake {
    let n = bake_size(size);
    let nu = n as usize;
    let tile = pattern.tile();
    let depth = pattern.depth();
    let (du, dv) = (tile[0] / n as f32, tile[1] / n as f32);
    let mut lvl: Vec<Texel> = Vec::with_capacity(nu * nu);
    for j in 0..nu {
        for i in 0..nu {
            let s = pattern.sample((i as f32 + 0.5) * du, (j as f32 + 0.5) * dv);
            lvl.push(Texel {
                color: s.color,
                slope: [0.0; 2],
                height: s.height,
                rough: s.rough,
            });
        }
    }
    // Wrapped central differences of the physical height.
    let h: Vec<f32> = lvl.iter().map(|t| t.height).collect();
    let at = |i: usize, j: usize| h[(j % nu) * nu + (i % nu)];
    let mut smax = 1e-4f32;
    for j in 0..nu {
        for i in 0..nu {
            let su = depth * (at(i + 1, j) - at(i + nu - 1, j)) / (2.0 * du);
            let sv = depth * (at(i, j + 1) - at(i, j + nu - 1)) / (2.0 * dv);
            smax = smax.max(su.abs()).max(sv.abs());
            lvl[j * nu + i].slope = [su, sv];
        }
    }
    let levels = levels_for(n);
    let mut all: Vec<Vec<Texel>> = vec![lvl];
    for k in 1..levels {
        let prev = &all[(k - 1) as usize];
        let pn = (n >> (k - 1)) as usize;
        let m = pn / 2;
        let mut next = Vec::with_capacity(m * m);
        for j in 0..m {
            for i in 0..m {
                let t = [
                    prev[2 * j * pn + 2 * i],
                    prev[2 * j * pn + 2 * i + 1],
                    prev[(2 * j + 1) * pn + 2 * i],
                    prev[(2 * j + 1) * pn + 2 * i + 1],
                ];
                let avg = |f: &dyn Fn(&Texel) -> f32| (f(&t[0]) + f(&t[1]) + f(&t[2]) + f(&t[3])) * 0.25;
                next.push(Texel {
                    color: [avg(&|x| x.color[0]), avg(&|x| x.color[1]), avg(&|x| x.color[2])],
                    slope: [avg(&|x| x.slope[0]), avg(&|x| x.slope[1])],
                    height: avg(&|x| x.height),
                    rough: avg(&|x| x.rough),
                });
            }
        }
        all.push(next);
    }
    let top = &all[0];
    let inv = 1.0 / top.len() as f32;
    let mut mean_color = [0.0f32; 3];
    let mut mean_rough = 0.0f32;
    for t in top {
        for (m, c) in mean_color.iter_mut().zip(t.color) {
            *m += c * inv;
        }
        mean_rough += t.rough * inv;
    }
    let texels = all.iter().flatten().map(|t| encode(t, smax)).collect();
    SurfaceBake {
        n,
        levels,
        texels,
        slope_scale: smax,
        mean_color,
        mean_rough,
        tile,
        depth,
    }
}

impl SurfaceBake {
    /// The decoded texel `(i, j)` of `level` (wrapped).
    pub fn texel(&self, level: u32, i: i64, j: i64) -> Texel {
        let level = level.min(self.levels - 1);
        let m = i64::from(self.n >> level);
        let idx = level_offset(self.n, level) + (j.rem_euclid(m) * m + i.rem_euclid(m)) as usize;
        decode(self.texels[idx], self.slope_scale)
    }
}
