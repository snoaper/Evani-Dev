//! Seeded 3D value noise for the flow wobble: lattice values from an
//! integer hash, blended with a smooth fade. Integer maths and IEEE basic
//! operations only, so every platform gets the same numbers.

use evani_model::vec::V3;

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// A value in -1..1 for one lattice point.
fn lattice(x: i64, y: i64, z: i64, seed: u64) -> f64 {
    let mut h = mix(seed ^ 0x9e37_79b9_7f4a_7c15);
    for v in [x, y, z] {
        h = mix(h ^ (v as u64));
    }
    (h >> 11) as f64 * (2.0 / 9_007_199_254_740_992.0) - 1.0
}

fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Smooth noise in -1..1 with features about 1 unit across.
pub(crate) fn value(p: V3, seed: u64) -> f64 {
    let f = [p[0].floor(), p[1].floor(), p[2].floor()];
    let i = [f[0] as i64, f[1] as i64, f[2] as i64];
    let t = [fade(p[0] - f[0]), fade(p[1] - f[1]), fade(p[2] - f[2])];
    let mut acc = 0.0;
    for dx in 0..2 {
        for dy in 0..2 {
            for dz in 0..2 {
                let w = (if dx == 0 { 1.0 - t[0] } else { t[0] })
                    * (if dy == 0 { 1.0 - t[1] } else { t[1] })
                    * (if dz == 0 { 1.0 - t[2] } else { t[2] });
                acc += w * lattice(i[0] + dx, i[1] + dy, i[2] + dz, seed);
            }
        }
    }
    acc
}

/// Two octaves (the second at double frequency, half weight), in -1..1.
pub(crate) fn fbm(p: V3, seed: u64) -> f64 {
    (value(p, seed) + 0.5 * value([p[0] * 2.0 + 17.3, p[1] * 2.0 - 5.1, p[2] * 2.0 + 9.7], seed ^ 0x5bd1_e995)) / 1.5
}
