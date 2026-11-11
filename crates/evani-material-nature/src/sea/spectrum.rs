//! The spectral sea's waves: a wind sea and a swell from the JONSWAP
//! spectrum with directional spreading, sampled once into a fixed table of
//! wave components in *normalised* units, so that every live setting (wind
//! speed, fetch, swell, height, directions) is a few per-frame numbers and
//! never a rebuild.
//!
//! Why normalised: in `x = omega / omega_p` the JONSWAP shape (gamma 3.3) and
//! the spreading (a function of `x` only) are the same for every wind speed
//! and fetch; only the peak wave number `k_p = omega_p^2 / g` and the height
//! scale `sqrt(alpha) g / omega_p^2` change. So the table holds, per
//! component, `x^2` (its wave number over `k_p`), the cosine and sine of its
//! heading off the mean, its amplitude for unit height and its phase; the
//! shader turns them into metres and seconds with the frame's numbers.
//!
//! Maths: Tessendorf, "Simulating Ocean Water" (the sum of waves, deep-water
//! dispersion); Hasselmann et al. 1973 (JONSWAP); the fetch-limited alpha
//! and peak and the Donelan-Banner style spreading as Ultra-Water
//! (Humpalumps, MIT) uses them. Ideas only: our own code, and a direct sum
//! of components instead of an FFT (DECISIONS.md "Spectral sea").
//!
//! The components are drawn with the engine's seeded [`Pcg32`], so the table
//! is the same on every machine; `sea_table_wgsl` writes it as WGSL and a
//! test keeps `shaders/sea_table.wgsl` equal to it.

use evani_core::rng::Pcg32;

/// Gravity (m/s^2).
pub const G: f32 = 9.81;
/// Peak enhancement of JONSWAP.
pub const GAMMA: f64 = 3.3;
/// The table's parts: for each quality level a set of components the
/// march displaces the surface by (wind sea from the peak down to waves a
/// quarter of its length, then swell), and one set of shorter wind-sea
/// detail components that only tilt the surface (sorted shortest last, so a
/// level that reads fewer drops the finest).
/// Per level (0 Low, 1 Medium, 2 High, 3 Ultra): wind and swell components
/// of its height set.
pub const GEO: [(usize, usize); 4] = [(6, 2), (6, 2), (8, 3), (10, 4)];
/// Where each level's height set starts (Low shares Medium's).
pub const GEO_BASE: [usize; 4] = [25, 25, 14, 0];
/// The detail set: where it starts, its size, and how much of it each
/// level reads.
pub const DET_BASE: usize = 33;
pub const DET: usize = 64;
pub const DET_N: [usize; 4] = [12, 20, 28, 40];
/// Every component.
pub const COMPONENTS: usize = DET_BASE + DET;
/// The height sets' reach in `x = omega / omega_p` (wind; swell) and the
/// detail set's.
pub const GEO_X: (f64, f64) = (0.72, 4.0);
pub const SWELL_X: (f64, f64) = (0.82, 2.6);
pub const DET_X: (f64, f64) = (4.0, 12.0);
/// The table's seed (fixed: the sea is the same on every run).
pub const SEED: u64 = 0x05EA_5EED;
/// Seconds the sea takes to repeat: every frequency is a whole multiple of
/// `2 pi / LOOP_S`, so phases stay small (f32-safe) and the loop is seamless.
pub const LOOP_S: f64 = 300.0;
/// The swell's own wind (m/s) and fetch (m): a long, old swell.
pub const SWELL_WIND: f32 = 7.0;
pub const SWELL_FETCH: f32 = 900_000.0;

/// One component in normalised units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Comp {
    /// `(omega / omega_p)^2 = k / k_p`.
    pub xx: f32,
    /// Heading off the mean heading: cosine and sine.
    pub cos: f32,
    pub sin: f32,
    /// Amplitude for a height scale of 1 (metres when multiplied by the
    /// spectrum's `sqrt(alpha) g / omega_p^2`).
    pub amp: f32,
    /// Phase at time 0, in turns (0..1).
    pub phase: f32,
}

/// Fetch-limited JONSWAP constants for wind `u` (m/s) over fetch `f` (m):
/// `(alpha, omega_p)` (Ultra-Water's forms: alpha = 0.076 (g F / U^2)^-0.22,
/// omega_p = 22 (U F / g^2)^-0.33).
pub fn jonswap(u: f32, f: f32) -> (f32, f32) {
    let u = u.max(0.5);
    let f = f.max(100.0);
    let alpha = 0.076 * (G * f / (u * u)).powf(-0.22);
    let wp = 22.0 * (u * f / (G * G)).powf(-0.33);
    (alpha, wp)
}

/// The peak wave number and the height scale of a spectrum (wind `u` m/s,
/// fetch `f` m): `k_p = omega_p^2 / g` and `sqrt(alpha) g / omega_p^2`.
pub fn scales(u: f32, f: f32) -> (f32, f32) {
    let (alpha, wp) = jonswap(u, f);
    (wp * wp / G, alpha.sqrt() * G / (wp * wp))
}

/// JONSWAP in `x = omega / omega_p`, for alpha g^2 omega_p^-4 = 1.
fn shape(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let s = if x <= 1.0 { 0.07 } else { 0.09 };
    let r = (-(x - 1.0) * (x - 1.0) / (2.0 * s * s)).exp();
    x.powi(-5) * (-1.25 * x.powi(-4)).exp() * GAMMA.powf(r)
}

/// Spreading exponent at `x` (Donelan-Banner style), plus the swell's
/// sharpening, and `sharp` of it for the wind sea.
fn spread_s(x: f64, swell: bool, sharp: f64) -> f64 {
    let mut s = if x > 1.0 { 9.77 * x.powf(-2.5) } else { 6.97 * x.powi(5) };
    if swell {
        s += 16.0 * x.min(20.0).tanh();
    } else {
        s += sharp * 16.0 * x.min(20.0).tanh();
    }
    s.max(0.05)
}

/// The (unnormalised) heading density at `x`, angle `t` off the mean: the
/// spreading lobe, for the wind sea blended 85 % with a cos^2 lobe.
fn spread(x: f64, t: f64, swell: bool, sharp: f64) -> f64 {
    let s = spread_s(x, swell, sharp);
    let lobe = (t * 0.5).cos().abs().powf(2.0 * s);
    if swell {
        return lobe;
    }
    let cos2 = if t.abs() < core::f64::consts::FRAC_PI_2 { t.cos() * t.cos() } else { 0.0 };
    // Both lobes normalised before the blend.
    let n_lobe = lobe_norm(s);
    0.15 * cos2 * (2.0 / core::f64::consts::PI) + 0.85 * lobe * n_lobe
}

/// 1 / integral of |cos(t/2)|^(2 s) over -pi..pi (numerically).
fn lobe_norm(s: f64) -> f64 {
    let n = 720;
    let dt = 2.0 * core::f64::consts::PI / n as f64;
    let mut sum = 0.0;
    for i in 0..n {
        let t = -core::f64::consts::PI + (i as f64 + 0.5) * dt;
        sum += (t * 0.5).cos().abs().powf(2.0 * s) * dt;
    }
    1.0 / sum.max(1e-12)
}

/// A heading off the mean (radians) at quantile `u` of the density at `x`.
fn heading(x: f64, u: f64, swell: bool, sharp: f64) -> f64 {
    let n = 1440;
    let dt = 2.0 * core::f64::consts::PI / n as f64;
    let mut cdf = Vec::with_capacity(n + 1);
    cdf.push(0.0);
    let mut sum = 0.0;
    for i in 0..n {
        let t = -core::f64::consts::PI + (i as f64 + 0.5) * dt;
        sum += spread(x, t, swell, sharp) * dt;
        cdf.push(sum);
    }
    let want = u.clamp(0.0, 1.0) * sum;
    let i = cdf.partition_point(|c| *c < want).clamp(1, n);
    let (a, b) = (cdf[i - 1], cdf[i]);
    let f = if b > a { (want - a) / (b - a) } else { 0.5 };
    -core::f64::consts::PI + (i as f64 - 1.0 + f) * dt
}

/// Energy of `shape` between `a` and `b` (Simpson, 32 steps).
fn energy(a: f64, b: f64) -> f64 {
    let n = 32;
    let h = (b - a) / n as f64;
    let mut s = shape(a) + shape(b);
    for i in 1..n {
        s += shape(a + i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0
}

/// Stratum edges over `x0..x1` with equal shares of `w(x)` in log x.
fn strata(n: usize, x0: f64, x1: f64, w: impl Fn(f64) -> f64) -> Vec<f64> {
    let steps = 2000;
    let (l0, l1) = (x0.ln(), x1.ln());
    let dl = (l1 - l0) / steps as f64;
    let mut cdf = vec![0.0];
    for i in 0..steps {
        let x = (l0 + (i as f64 + 0.5) * dl).exp();
        let last = cdf.last().copied().unwrap_or(0.0);
        cdf.push(last + w(x).max(1e-12) * dl);
    }
    let total = cdf.last().copied().unwrap_or(1.0);
    let mut edges = vec![x0];
    for k in 1..n {
        let want = total * k as f64 / n as f64;
        let i = cdf.partition_point(|c| *c < want).clamp(1, steps);
        let f = (want - cdf[i - 1]) / (cdf[i] - cdf[i - 1]).max(1e-18);
        edges.push((l0 + (i as f64 - 1.0 + f) * dl).exp());
    }
    edges.push(x1);
    edges
}

/// `n` components over `x0..x1`: strata with equal shares of `w` (in log
/// x), each component carrying its stratum's exact energy (a^2 / 2 = E)
/// at the stratum's energy middle; headings drawn by stratified quantiles
/// in a shuffled order so heading and length are independent. Sorted by
/// length, longest first.
fn draw(n: usize, x0: f64, x1: f64, swell: bool, sharp: f64, w: impl Fn(f64) -> f64, rng: &mut Pcg32) -> Vec<Comp> {
    let edges = strata(n, x0, x1, w);
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.below(i as u32 + 1) as usize;
        order.swap(i, j);
    }
    let mut out = Vec::with_capacity(n);
    for (i, &q) in order.iter().enumerate() {
        let (a, b) = (edges[i], edges[i + 1]);
        let e = energy(a, b);
        // The stratum's energy middle, nudged a little either way.
        let target = e * (0.5 + 0.3 * (f64::from(rng.next_f32()) - 0.5));
        let mut x = a;
        let k = 64;
        let mut acc = 0.0;
        for j in 0..k {
            let (xa, xb) = (a + (b - a) * j as f64 / k as f64, a + (b - a) * (j + 1) as f64 / k as f64);
            acc += energy(xa, xb);
            x = xb;
            if acc >= target {
                break;
            }
        }
        let amp = (2.0 * e).sqrt();
        let u = (q as f64 + 0.2 + 0.6 * f64::from(rng.next_f32())) / n as f64;
        let t = heading(x, u, swell, sharp);
        out.push(Comp { xx: (x * x) as f32, cos: t.cos() as f32, sin: t.sin() as f32, amp: amp as f32, phase: rng.next_f32() });
    }
    out.sort_by(|a, b| a.xx.total_cmp(&b.xx));
    out
}

/// The table: the height sets of Ultra, High and Medium (each its wind sea
/// then its swell), then the detail set. The height sets are spaced by the
/// square root of the energy, so the peak gets most of them and every
/// length down to a quarter of the peak's still gets some; the detail set
/// evenly in log length.
pub fn table() -> Vec<Comp> {
    let mut rng = Pcg32::seeded(SEED);
    let sqrt_e = |x: f64| shape(x).sqrt();
    let mut t = Vec::with_capacity(COMPONENTS);
    for (w, s) in [GEO[3], GEO[2], GEO[1]] {
        t.extend(draw(w, GEO_X.0, GEO_X.1, false, 0.0, sqrt_e, &mut rng));
        t.extend(draw(s, SWELL_X.0, SWELL_X.1, true, 0.0, sqrt_e, &mut rng));
    }
    t.extend(draw(DET, DET_X.0, DET_X.1, false, 0.0, |_| 1.0, &mut rng));
    t
}

/// Whether component `i` of the table is swell.
pub fn is_swell(i: usize) -> bool {
    if i >= DET_BASE {
        return false;
    }
    for level in [3, 2, 1] {
        let (b, (w, s)) = (GEO_BASE[level], GEO[level]);
        if (b..b + w + s).contains(&i) {
            return i >= b + w;
        }
    }
    false
}

/// The table as WGSL (`shaders/sea_table.wgsl`): its layout constants and
/// module-scope `const` arrays of `vec4(xx, cos, sin, amp)` and of phases.
pub fn sea_table_wgsl(t: &[Comp]) -> String {
    let mut s = String::from(
        "// evani.shader.sea_table (generated by evani-material-nature src/sea/spectrum.rs:
         // `sea_table_wgsl`; a test keeps this file equal to it). The spectral sea's
         // components in normalised units: (k / k_p, heading cos, heading sin, amplitude
         // for unit height). Height sets of Ultra, High and Medium (wind sea, then
         // swell, longest first), then the detail set.
",
    );
    let arr = |v: [usize; 4]| v.map(|x| format!("{x}u")).join(", ");
    s.push_str(&format!("const SEA_GEO_BASE = array<u32, 4>({});
", arr(GEO_BASE)));
    s.push_str(&format!("const SEA_GEO_W = array<u32, 4>({});
", arr(GEO.map(|g| g.0))));
    s.push_str(&format!("const SEA_GEO_S = array<u32, 4>({});
", arr(GEO.map(|g| g.1))));
    s.push_str(&format!("const SEA_DET_BASE: u32 = {DET_BASE}u;
"));
    s.push_str(&format!("const SEA_DET_N = array<u32, 4>({});
", arr(DET_N)));
    s.push_str(&format!("const SEA_C = array<vec4<f32>, {}>(
", t.len()));
    for c in t {
        s.push_str(&format!("    vec4<f32>({:.6}, {:.6}, {:.6}, {:.6}),
", c.xx, c.cos, c.sin, c.amp));
    }
    s.push_str(");
");
    s.push_str(&format!("const SEA_P = array<f32, {}>(
", t.len()));
    for c in t.chunks(8) {
        let row: Vec<String> = c.iter().map(|c| format!("{:.6}", c.phase)).collect();
        s.push_str(&format!("    {},
", row.join(", ")));
    }
    s.push_str(");
");
    s
}

/// A component as the shader reads it (the WGSL text's rounding), so the
/// CPU mirror evaluates exactly the numbers the GPU does.
pub fn as_shader(t: &[Comp]) -> Vec<Comp> {
    let r = |v: f32| format!("{v:.6}").parse::<f32>().unwrap_or(v);
    t.iter().map(|c| Comp { xx: r(c.xx), cos: r(c.cos), sin: r(c.sin), amp: r(c.amp), phase: r(c.phase) }).collect()
}

// ---- the field sea's table (wave textures; `material.nature.water.wave_field`) ----

/// The field sea's wind components (spread over the whole wind sea, from
/// below the peak to the short chop), then its swell components.
pub const FIELD_WIND: usize = 864;
pub const FIELD_SWELL: usize = 32;
pub const FIELD_N: usize = FIELD_WIND + FIELD_SWELL;
/// The wind set's reach in `x = omega / omega_p` (k / k_p up to 2304: in a
/// 24 m/s storm 0.16 m, in a 9 m/s breeze 3 cm; what the finest band can't
/// hold is left to the roughness).
pub const FIELD_X: (f64, f64) = (0.72, 48.0);
/// How much of the swell's sharpening the wind sea's headings get: 0.25^2, as
/// Ultra-Water's `windSwell` (MIT, Copyright 2026 Humpalumps). Without it short
/// waves run nearly every way, and the sum reads as round dimples, not crests.
const FIELD_SHARP: f64 = 0.0625;
/// Its own seed (the classic table above is untouched).
pub const FIELD_SEED: u64 = 0x0F1E_D5EA;

/// The field table: `FIELD_WIND` wind components over `FIELD_X`, spaced by
/// the square root of the energy near the peak and evenly by wave length ratio
/// down the tail (so every band gets about the same three dozen: the bands of
/// 2 to 16 m, which draw most of what the eye reads as chop, are as busy as the
/// rest), then `FIELD_SWELL` swell components; each carries its stratum's
/// exact energy, so the sum's variance is the spectrum's. Sorted by length
/// within each set, longest first.
pub fn field_table() -> Vec<Comp> {
    let mut rng = Pcg32::seeded(FIELD_SEED);
    let peak = shape(1.0).sqrt();
    let mut t = draw(FIELD_WIND, FIELD_X.0, FIELD_X.1, false, FIELD_SHARP, |x| (shape(x).sqrt() / peak).max(1.0 / x), &mut rng);
    t.extend(draw(FIELD_SWELL, SWELL_X.0, SWELL_X.1, true, 0.0, |x| shape(x).sqrt(), &mut rng));
    t
}

/// The field table as WGSL (`shaders/sea_field_table.wgsl`, read by the
/// wave-texture draw), with the field sea's layout numbers.
pub fn field_table_wgsl(t: &[Comp]) -> String {
    let mut s = String::from(
        "// evani.shader.sea_field_table (generated by evani-material-nature src/sea/spectrum.rs:
// `field_table_wgsl`; a test keeps this file equal to it). The field sea's
// components in normalised units: (k / k_p, heading cos, heading sin,
// amplitude for unit height), wind sea then swell, and their phases.
",
    );
    s.push_str(&format!("const FIELD_NW: u32 = {FIELD_WIND}u;\nconst FIELD_N: u32 = {FIELD_N}u;\n"));
    s.push_str(&format!("const FIELD_C = array<vec4<f32>, {}>(\n", t.len()));
    for c in t {
        s.push_str(&format!("    vec4<f32>({:.6}, {:.6}, {:.6}, {:.6}),\n", c.xx, c.cos, c.sin, c.amp));
    }
    s.push_str(");\n");
    s.push_str(&format!("const FIELD_P = array<f32, {}>(\n", t.len()));
    for c in t.chunks(8) {
        let row: Vec<String> = c.iter().map(|c| format!("{:.6}", c.phase)).collect();
        s.push_str(&format!("    {},\n", row.join(", ")));
    }
    s.push_str(");\n");
    s
}
