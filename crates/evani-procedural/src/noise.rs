//! Seeded noise: an integer hash, value noise and fractal sums (fBm).
//! Pure integer hashing, so every device gives the same field.

/// A 32-bit integer hash (a PCG-style permute; no tables).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// A lattice point's value in [0, 1).
pub fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let h = hash((x as u32).wrapping_mul(0x9e37_79b9) ^ hash((y as u32).wrapping_add(seed.wrapping_mul(0x85eb_ca6b))));
    (h >> 8) as f32 / (1u32 << 24) as f32
}

pub fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let h = hash((x as u32).wrapping_mul(0x9e37_79b9) ^ hash((y as u32) ^ hash((z as u32).wrapping_add(seed.wrapping_mul(0x85eb_ca6b)))));
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth 2D value noise in [0, 1).
pub fn value2(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (smooth(x - x.floor()), smooth(y - y.floor()));
    let a = lerp(hash2(xi, yi, seed), hash2(xi + 1, yi, seed), tx);
    let b = lerp(hash2(xi, yi + 1, seed), hash2(xi + 1, yi + 1, seed), tx);
    lerp(a, b, ty)
}

/// Smooth 3D value noise in [0, 1).
pub fn value3(p: [f32; 3], seed: u32) -> f32 {
    let i = [p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32];
    let t = [smooth(p[0] - p[0].floor()), smooth(p[1] - p[1].floor()), smooth(p[2] - p[2].floor())];
    let c = |dx: i32, dy: i32, dz: i32| hash3(i[0] + dx, i[1] + dy, i[2] + dz, seed);
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), t[0]);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), t[0]);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), t[0]);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), t[0]);
    lerp(lerp(x00, x10, t[1]), lerp(x01, x11, t[1]), t[2])
}

/// Fractal value noise (fBm) in about [-1, 1]: `octaves` layers, each
/// `lacunarity` times finer and `gain` times weaker.
pub fn fbm2(x: f32, y: f32, seed: u32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves.clamp(1, 12) {
        sum += amp * (value2(x * freq, y * freq, seed.wrapping_add(o * 1013)) * 2.0 - 1.0);
        norm += amp;
        amp *= gain;
        freq *= lacunarity;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// Fractal 3D value noise in about [-1, 1]: `octaves` layers, each twice as
/// fine and half as strong.
pub fn fbm3(p: [f32; 3], seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves.clamp(1, 12) {
        sum += amp * (value3([p[0] * freq, p[1] * freq, p[2] * freq], seed.wrapping_add(o * 1013)) * 2.0 - 1.0);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// Ridged fractal noise in [0, 1]: sharp crests where the noise crosses
/// its middle (mountain ridges, cracks).
pub fn ridged2(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves.clamp(1, 12) {
        let n = 1.0 - (value2(x * freq, y * freq, seed.wrapping_add(o * 977)) * 2.0 - 1.0).abs();
        sum += amp * n * n;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// Cell noise: the distances from (x, y) to the nearest and second nearest
/// of one seeded point per unit cell (stones, cobbles, cracks, scales).
pub fn cells2(x: f32, y: f32, seed: u32) -> (f32, f32) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2) = (f32::MAX, f32::MAX);
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (xi + i, yi + j);
            let px = cx as f32 + hash2(cx, cy, seed);
            let py = cy as f32 + hash2(cx, cy, seed ^ 0x5bd1_e995);
            let d = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2)
}

// ---- realism base: gradient noise, warps, eroded fBm, cell ids -------------

/// A lattice point's unit gradient (one of 16 directions).
fn grad2(x: i32, y: i32, seed: u32) -> [f32; 2] {
    let a = hash2(x, y, seed ^ 0x2c1b_3c6d) * std::f32::consts::TAU;
    [a.cos(), a.sin()]
}

fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Smooth 2D gradient (Perlin-style) noise in about [-1, 1]: no lattice
/// blockiness, the base of every realism field.
pub fn perlin2(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let d = |i: i32, j: i32| {
        let g = grad2(xi + i, yi + j, seed);
        g[0] * (fx - i as f32) + g[1] * (fy - j as f32)
    };
    let (u, v) = (quintic(fx), quintic(fy));
    let a = lerp(d(0, 0), d(1, 0), u);
    let b = lerp(d(0, 1), d(1, 1), u);
    (lerp(a, b, v) * 1.414).clamp(-1.0, 1.0)
}

/// Smooth 3D gradient noise in about [-1, 1] (surface detail on any shape).
pub fn perlin3(p: [f32; 3], seed: u32) -> f32 {
    let i = [p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32];
    let f = [p[0] - p[0].floor(), p[1] - p[1].floor(), p[2] - p[2].floor()];
    let d = |a: i32, b: i32, c: i32| {
        let h1 = hash3(i[0] + a, i[1] + b, i[2] + c, seed ^ 0x51ed_270b) * std::f32::consts::TAU;
        let z = hash3(i[0] + a, i[1] + b, i[2] + c, seed ^ 0x2f6b_8a13) * 2.0 - 1.0;
        let r = (1.0 - z * z).max(0.0).sqrt();
        r * h1.cos() * (f[0] - a as f32) + r * h1.sin() * (f[1] - b as f32) + z * (f[2] - c as f32)
    };
    let (u, v, w) = (quintic(f[0]), quintic(f[1]), quintic(f[2]));
    let x00 = lerp(d(0, 0, 0), d(1, 0, 0), u);
    let x10 = lerp(d(0, 1, 0), d(1, 1, 0), u);
    let x01 = lerp(d(0, 0, 1), d(1, 0, 1), u);
    let x11 = lerp(d(0, 1, 1), d(1, 1, 1), u);
    (lerp(lerp(x00, x10, v), lerp(x01, x11, v), w) * 1.6).clamp(-1.0, 1.0)
}

/// Fractal gradient noise in about [-1, 1]; each octave is rotated so the
/// lattice never lines up (no grid-aligned streaks in terrain).
pub fn fbm(x: f32, y: f32, seed: u32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    let (mut px, mut py) = (x, y);
    for o in 0..octaves.clamp(1, 12) {
        sum += amp * perlin2(px, py, seed.wrapping_add(o * 7919));
        norm += amp;
        amp *= gain;
        // Rotate by about 37 degrees and scale.
        let (rx, ry) = (0.8 * px - 0.6 * py, 0.6 * px + 0.8 * py);
        px = rx * lacunarity + 17.3;
        py = ry * lacunarity - 9.1;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// 3D fractal gradient noise in about [-1, 1].
pub fn fbm3p(p: [f32; 3], seed: u32, octaves: u32, gain: f32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves.clamp(1, 12) {
        sum += amp * perlin3(q, seed.wrapping_add(o * 7919));
        norm += amp;
        amp *= gain;
        q = [q[0] * 2.03 + 3.1, q[1] * 2.03 - 1.7, q[2] * 2.03 + 5.3];
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// Domain warp: (x, y) moved by a low-frequency noise field `strength`
/// units, so ridges and coasts meander instead of following the lattice.
pub fn warp(x: f32, y: f32, seed: u32, scale: f32, strength: f32) -> (f32, f32) {
    let wx = fbm(x * scale, y * scale, seed ^ 0x68e3_1da4, 3, 2.0, 0.5);
    let wy = fbm(x * scale + 5.2, y * scale + 1.3, seed ^ 0xb529_7a4d, 3, 2.0, 0.5);
    (x + wx * strength, y + wy * strength)
}

/// Value noise and its two derivatives (d/dx, d/dy): the base of
/// [`eroded`].
pub fn value2d(x: f32, y: f32, seed: u32) -> (f32, f32, f32) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (du, dv) = (6.0 * fx * (1.0 - fx), 6.0 * fy * (1.0 - fy));
    let a = hash2(xi, yi, seed) * 2.0 - 1.0;
    let b = hash2(xi + 1, yi, seed) * 2.0 - 1.0;
    let c = hash2(xi, yi + 1, seed) * 2.0 - 1.0;
    let d = hash2(xi + 1, yi + 1, seed) * 2.0 - 1.0;
    let k = a - b - c + d;
    (a + (b - a) * u + (c - a) * v + k * u * v, du * ((b - a) + k * v), dv * ((c - a) + k * u))
}

/// Eroded fractal noise in about [-1, 1]: each octave is damped where the
/// slope summed so far is steep, so peaks stay sharp and rugged while
/// valleys and lowlands come out smooth, like weathered land (the
/// derivative-damped fBm idea, written fresh).
pub fn eroded(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    let (mut dx, mut dy) = (0.0f32, 0.0f32);
    let (mut px, mut py) = (x, y);
    for o in 0..octaves.clamp(1, 12) {
        let (n, nx, ny) = value2d(px, py, seed.wrapping_add(o * 4073));
        dx += nx * amp;
        dy += ny * amp;
        sum += amp * n / (1.0 + dx * dx + dy * dy);
        norm += amp;
        amp *= 0.5;
        let (rx, ry) = (0.8 * px - 0.6 * py, 0.6 * px + 0.8 * py);
        px = rx * 2.0 + 11.7;
        py = ry * 2.0 - 4.9;
    }
    if norm > 0.0 { (sum / norm * 1.6).clamp(-1.0, 1.0) } else { 0.0 }
}

/// Cell noise with the nearest cell's id: the distances to the nearest and
/// second nearest seeded point and a hash of the nearest cell in [0, 1)
/// (per-stone colour, per-plate tilt, per-crack depth).
pub fn cells2_id(x: f32, y: f32, seed: u32) -> (f32, f32, f32) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id) = (f32::MAX, f32::MAX, 0.0);
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (xi + i, yi + j);
            let px = cx as f32 + hash2(cx, cy, seed);
            let py = cy as f32 + hash2(cx, cy, seed ^ 0x5bd1_e995);
            let d = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = hash2(cx, cy, seed ^ 0x1b87_3593);
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, id)
}

/// A smooth step from 0 at `a` to 1 at `b`.
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The recipe stage's own noise on the CPU (round 9, shared CPU/GPU noise):
/// an exact port of `rc_pcg`, `rc_hash2`, `rc_u01`, `rc_vnoise2` and
/// `rc_fbm2` from evani-material-recipes' `shaders/core.wgsl` (the same u32
/// arithmetic, the same quintic fade and lattice turn), so a generator can
/// carve geometry where a material paints its pattern: the sandstone's soft
/// beds weather back exactly where the shader draws them. The shader's seed
/// is the block's byte 23 ([`crate::surface::Surface::shader_seed`]).
pub mod gpu {
    pub fn pcg(v: u32) -> u32 {
        let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
        (w >> 22) ^ w
    }

    pub fn hash2(c: [i32; 2], seed: u32) -> u32 {
        pcg((c[1] as u32) ^ pcg((c[0] as u32) ^ pcg(seed)))
    }

    pub fn u01(h: u32) -> f32 {
        (h >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    fn fade5(t: f32) -> f32 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    /// `rc_vnoise2`: value noise in [0, 1].
    pub fn vnoise2(p: [f32; 2], seed: u32) -> f32 {
        let fl = [p[0].floor(), p[1].floor()];
        let i = [fl[0] as i32, fl[1] as i32];
        let u = [fade5(p[0] - fl[0]), fade5(p[1] - fl[1])];
        let a = u01(hash2(i, seed));
        let b = u01(hash2([i[0] + 1, i[1]], seed));
        let c = u01(hash2([i[0], i[1] + 1], seed));
        let d = u01(hash2([i[0] + 1, i[1] + 1], seed));
        let mix = |x: f32, y: f32, t: f32| x + (y - x) * t;
        mix(mix(a, b, u[0]), mix(c, d, u[0]), u[1])
    }

    /// `rc_fbm2`: octaves clamped to 1..6, normalised to [0, 1].
    pub fn fbm2(p: [f32; 2], seed: u32, octaves: u32) -> f32 {
        let mut q = p;
        let (mut amp, mut sum, mut norm) = (0.5f32, 0.0f32, 0.0f32);
        for k in 0..octaves.clamp(1, 6) {
            sum += vnoise2(q, seed.wrapping_add(k * 101)) * amp;
            norm += amp;
            q = [q[0] * 1.6 - q[1] * 1.2, q[0] * 1.2 + q[1] * 1.6];
            amp *= 0.5;
        }
        sum / norm
    }

    /// `rc_proj`: the plane facing the dominant axis of `n`.
    pub fn proj(p: [f32; 3], n: [f32; 3]) -> [f32; 2] {
        let a = [n[0].abs(), n[1].abs(), n[2].abs()];
        if a[1] >= a[0] && a[1] >= a[2] {
            [p[0], p[2]]
        } else if a[0] >= a[2] {
            [p[2], p[1]]
        } else {
            [p[0], p[1]]
        }
    }

    /// `rc_proj2`: the two planes nearest `n` and the second's weight.
    pub fn proj2(p: [f32; 3], n: [f32; 3]) -> ([f32; 2], [f32; 2], f32) {
        let a = [n[0].abs(), n[1].abs(), n[2].abs()];
        if a[1] >= a[0] && a[1] >= a[2] {
            let b = if a[0] >= a[2] { [p[2], p[1]] } else { [p[0], p[1]] };
            ([p[0], p[2]], b, a[0].max(a[2]) / (a[1] + a[0].max(a[2]) + 1e-6))
        } else if a[0] >= a[2] {
            let b = if a[1] >= a[2] { [p[0], p[2]] } else { [p[0], p[1]] };
            ([p[2], p[1]], b, a[1].max(a[2]) / (a[0] + a[1].max(a[2]) + 1e-6))
        } else {
            let b = if a[1] >= a[0] { [p[0], p[2]] } else { [p[2], p[1]] };
            ([p[0], p[1]], b, a[0].max(a[1]) / (a[2] + a[0].max(a[1]) + 1e-6))
        }
    }

    #[cfg(test)]
    mod tests {
        /// The port's integer hash is the shader's: values worked by hand
        /// from rc_pcg's three steps (wrapping u32).
        #[test]
        fn the_cpu_hash_is_the_shaders() {
            // s = 0 * 747796405 + 2891336453 = 2891336453; s >> 28 = 10;
            // (s >> 14) ^ s = 2891512868...; checked against the formula.
            let s: u32 = 2_891_336_453;
            let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
            assert_eq!(super::pcg(0), (w >> 22) ^ w);
            let n = super::vnoise2([3.25, -1.5], 7);
            assert!((0.0..=1.0).contains(&n));
            assert_eq!(super::vnoise2([2.0, 5.0], 9), super::u01(super::hash2([2, 5], 9)), "lattice points are the hash");
            let f = super::fbm2([0.3, 0.7], 11, 3);
            assert!((0.0..=1.0).contains(&f));
        }
    }
}
