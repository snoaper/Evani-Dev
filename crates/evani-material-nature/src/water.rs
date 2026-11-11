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

/// The wave params `waves` reads (the shader's table rows 4, 18 and 20).
/// [`Waves::default`] is the material's default: the ocean upgrade off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waves {
    /// `water.wave_scale` (1/m).
    pub scale: f32,
    /// `water.wave_speed`.
    pub speed: f32,
    /// `water.wave_strength`.
    pub strength: f32,
    /// `water.swell_direction` (degrees).
    pub swell_direction: f32,
    /// `water.swell` (0..1).
    pub swell: f32,
    /// `water.flow_speed` (m/s).
    pub flow_speed: f32,
    /// `water.flow_direction` (degrees, 0 = +x, 90 = +z).
    pub flow_direction: f32,
}

impl Default for Waves {
    fn default() -> Self {
        Waves {
            scale: 0.9,
            speed: 1.0,
            strength: 0.12,
            swell_direction: 0.0,
            swell: 0.0,
            flow_speed: 0.0,
            flow_direction: 0.0,
        }
    }
}

/// The shader's `turn2`: `v` turned by `a` radians (x towards z).
pub fn turn2(v: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// The shader's `water_drift`: how far the flow has carried the surface (m)
/// at sim time `t`.
pub fn drift(w: &Waves, t: f32) -> [f32; 2] {
    let a = w.flow_direction.to_radians();
    let k = w.flow_speed * t;
    [a.cos() * k, a.sin() * k]
}

/// The four wave directions after the swell: turned by `swell_direction`
/// and gathered round the first by `swell` (the shader's `water_waves`).
pub fn wave_dirs(w: &Waves) -> [[f32; 2]; 4] {
    const SPREAD: [f32; 4] = [0.0, 0.38, -0.52, 0.85];
    let turn = w.swell_direction.to_radians();
    let mut out = DIRS;
    for (k, d) in out.iter_mut().enumerate() {
        if w.swell > 0.0 {
            let b = turn2(DIRS[0], SPREAD[k]);
            let m = [d[0] + (b[0] - d[0]) * w.swell, d[1] + (b[1] - d[1]) * w.swell];
            let l = (m[0] * m[0] + m[1] * m[1]).sqrt().max(1e-6);
            *d = [m[0] / l, m[1] / l];
        }
        *d = turn2(*d, turn);
    }
    out
}

const DIRS: [[f32; 2]; 4] = [[0.8, 0.6], [-0.39, 0.92], [0.97, -0.24], [-0.71, -0.7]];
const FREQS: [f32; 4] = [1.0, 1.73, 2.61, 3.9];

/// The shader's `water_waves` at world `p` (x, z; the flow drift is taken
/// off here, as `water` does before calling it): (dh/dx, dh/dz, crest),
/// crest -1 (every trough at once) to 1 (every crest at once). With the
/// upgrade off ([`Waves::default`] values) the slope is [`slope`]'s, bit
/// for bit.
pub fn waves(p: [f32; 2], t: f32, w: &Waves) -> [f32; 3] {
    let dr = drift(w, t);
    let p = [p[0] - dr[0], p[1] - dr[1]];
    let scale = w.scale.max(0.01);
    let speed = w.speed;
    let mut g = [0.0f32; 2];
    let mut crest = 0.0f32;
    for (d, fr) in wave_dirs(w).iter().zip(FREQS) {
        let f = fr * scale;
        let ph = (d[0] * p[0] + d[1] * p[1]) * f + t * speed * (9.81 * f).sqrt();
        // The same order of operations as `slope` (bit for bit when off).
        let c = ph.cos() * 0.55 / fr;
        g[0] += d[0] * c;
        g[1] += d[1] * c;
        crest += ph.sin() * 0.55 / fr;
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
    [g[0] * w.strength, g[1] * w.strength, crest / 1.219_67]
}

/// Whether a view ray from `eye` along `ray` (any length; `scene_t` is the
/// scene's distance in the same units, `f32::INFINITY` for the sky) crosses
/// a water rectangle at height `centre[1]` (half size `half` in x and z,
/// turned `turn` radians about +y) before its scene point: the shader's
/// `water` hit test, which `seen_through` in `shaders/nature_common.wgsl`
/// uses to keep refraction on pixels seen through the same water.
pub fn ray_crosses_rect(eye: [f32; 3], ray: [f32; 3], scene_t: f32, centre: [f32; 3], half: [f32; 2], turn: f32) -> bool {
    if eye[1] <= centre[1] || ray[1].abs() < 1e-6 {
        return false;
    }
    let tw = (centre[1] - eye[1]) / ray[1];
    if tw <= 0.0 || tw >= scene_t {
        return false;
    }
    let (dx, dz) = (eye[0] + ray[0] * tw - centre[0], eye[2] + ray[2] * tw - centre[2]);
    // The shader's rot_y(v, -turn): x' = c x + s z, z' = -s x + c z with
    // s = sin(-turn), c = cos(-turn).
    let (s, c) = (-turn).sin_cos();
    let (lx, lz) = (c * dx + s * dz, -s * dx + c * dz);
    lx.abs() <= half[0] && lz.abs() <= half[1]
}
