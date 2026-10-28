//! A CPU mirror of the shader's water waves (`water_slope` in
//! `shaders/nature_common.wgsl`), bit for bit the same integer hashes, so
//! a test can pin that the waves depend only on position and sim time.

/// PCG hash (Jarzynski and Olano, JCGT 2020).
pub fn pcg(x: u32) -> u32 {
    let s = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// Hash of an integer cell to 0..1.
pub fn hash2(x: i32, y: i32) -> f32 {
    let h = pcg((x as u32) ^ pcg(y as u32));
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Value noise with its gradient: (value, d/dx, d/dy).
pub fn vnoise(p: [f32; 2]) -> [f32; 3] {
    let fl = [p[0].floor(), p[1].floor()];
    let (ix, iy) = (fl[0] as i32, fl[1] as i32);
    let f = [p[0] - fl[0], p[1] - fl[1]];
    let u = f.map(|f| f * f * f * (f * (f * 6.0 - 15.0) + 10.0));
    let du = f.map(|f| 30.0 * f * f * (f * (f - 2.0) + 1.0));
    let a = hash2(ix, iy);
    let b = hash2(ix + 1, iy);
    let c = hash2(ix, iy + 1);
    let d = hash2(ix + 1, iy + 1);
    let k = a - b - c + d;
    let v = a + (b - a) * u[0] + (c - a) * u[1] + k * u[0] * u[1];
    [v, du[0] * (b - a + k * u[1]), du[1] * (c - a + k * u[0])]
}

/// Slope (dh/dx, dh/dz) of the water at world `p` (x, z) and sim time `t`
/// for `material.nature.water.wave_scale`, `wave_speed`, `wave_strength`.
pub fn slope(p: [f32; 2], t: f32, scale: f32, speed: f32, strength: f32) -> [f32; 2] {
    const DIRS: [[f32; 2]; 4] = [[0.8, 0.6], [-0.39, 0.92], [0.97, -0.24], [-0.71, -0.7]];
    const FREQS: [f32; 4] = [1.0, 1.73, 2.61, 3.9];
    let scale = scale.max(0.01);
    let mut g = [0.0f32; 2];
    for (d, fr) in DIRS.iter().zip(FREQS) {
        let f = fr * scale;
        let ph = (d[0] * p[0] + d[1] * p[1]) * f + t * speed * (9.81 * f).sqrt();
        let a = ph.cos() * 0.55 / fr;
        g[0] += d[0] * a;
        g[1] += d[1] * a;
    }
    let n1 = vnoise([
        p[0] * scale * 4.3 + t * speed * 0.31,
        p[1] * scale * 4.3 + t * speed * 0.17,
    ]);
    let n2 = vnoise([
        p[0] * scale * 9.1 - t * speed * 0.23,
        p[1] * scale * 9.1 + t * speed * 0.29,
    ]);
    g[0] += n1[1] * 0.18 + n2[1] * 0.07;
    g[1] += n1[2] * 0.18 + n2[2] * 0.07;
    [g[0] * strength, g[1] * strength]
}
