//! The pre-integrated skin lookup, generated in code at load (nothing is
//! downloaded): how much light a curved patch of skin returns for each
//! N.L and curvature, with light scattered under the skin wrapping past the
//! shadow edge, red furthest.
//!
//! The idea is Penner and Borshukov's pre-integrated skin shading (GPU Pro
//! 2, 2011): integrate the diffusion profile round a circle of radius r,
//!
//! `D(theta, r) = int cos+(theta + x) R(2 r sin(x / 2)) dx / int R(2 r sin(x / 2)) dx`
//!
//! with R the six-Gaussian skin profile of d'Eon and Luebke (GPU Gems 3,
//! ch. 14, 2007; the published variances and weights below). Ideas and the
//! published profile only; no code from either source (or from
//! iryoku/separable-sss) is used.
//!
//! The table is [`SIZE`] x [`SIZE`]: columns are N.L from -1 to 1, rows are
//! curvature 1/r from 0 to [`C_MAX`] per mm on a square-root axis (row j is
//! `C_MAX * (j / 63)^2`, so the low curvatures of a face get most rows).
//! The model pipeline has no free texture binding, so the GPU gets the table
//! as a fit: every [`FIT_STEP`]th row, per colour channel, as the blurred
//! ramp `a * g(N.L; s)` ([`ramp`]), 8 rows x 3 channels x 2 numbers, spliced
//! into the `char_params` slot. The shader blends between rows by curvature.

/// Rows and columns of the table.
pub const SIZE: usize = 64;
/// The highest curvature in the table, 1/mm (a 1 mm radius).
pub const C_MAX: f32 = 1.0;
/// Rows the GPU gets (every [`FIT_STEP`]th table row, 0 to 63).
pub const FIT_ROWS: usize = 8;
/// Table rows between two fitted rows.
pub const FIT_STEP: usize = 9;
/// Samples round the circle per row.
const STEPS: usize = 256;

/// d'Eon and Luebke's skin profile: (variance in mm^2, RGB weights).
pub const PROFILE: [(f32, [f32; 3]); 6] = [
    (0.0064, [0.233, 0.455, 0.649]),
    (0.0484, [0.100, 0.336, 0.344]),
    (0.187, [0.118, 0.198, 0.0]),
    (0.567, [0.113, 0.007, 0.007]),
    (1.99, [0.358, 0.004, 0.0]),
    (7.41, [0.078, 0.0, 0.0]),
];

/// N.L of table column `col`.
pub fn ndl_at(col: usize) -> f32 {
    -1.0 + 2.0 * col as f32 / (SIZE - 1) as f32
}

/// Curvature (1/mm) of table row `row`.
pub fn curvature_at(row: usize) -> f32 {
    let t = row as f32 / (SIZE - 1) as f32;
    C_MAX * t * t
}

/// The profile at distance `d` mm, per channel.
fn profile(d: f32) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (v, w) in PROFILE {
        let g = (-d * d / (2.0 * v)).exp() / (2.0 * core::f32::consts::PI * v);
        for c in 0..3 {
            out[c] += w[c] * g;
        }
    }
    out
}

/// One table row: D for every column at curvature `c` (1/mm).
pub fn row(c: f32) -> [[f32; 3]; SIZE] {
    let mut out = [[0.0; 3]; SIZE];
    if c <= 0.0 {
        for (col, o) in out.iter_mut().enumerate() {
            *o = [ndl_at(col).max(0.0); 3];
        }
        return out;
    }
    let r = 1.0 / c;
    let dx = 2.0 * core::f32::consts::PI / STEPS as f32;
    let mut k = [[0.0f32; 3]; STEPS];
    let mut cs = [(0.0f32, 0.0f32); STEPS];
    let mut norm = [0.0f32; 3];
    for s in 0..STEPS {
        let x = -core::f32::consts::PI + (s as f32 + 0.5) * dx;
        k[s] = profile(2.0 * r * (x * 0.5).sin().abs());
        cs[s] = (x.cos(), x.sin());
        for ch in 0..3 {
            norm[ch] += k[s][ch];
        }
    }
    for (col, o) in out.iter_mut().enumerate() {
        let ct = ndl_at(col).clamp(-1.0, 1.0);
        let st = (1.0 - ct * ct).max(0.0).sqrt();
        let mut acc = [0.0f32; 3];
        for s in 0..STEPS {
            // cos(theta + x), theta = acos(N.L)
            let lit = (ct * cs[s].0 - st * cs[s].1).max(0.0);
            for ch in 0..3 {
                acc[ch] += lit * k[s][ch];
            }
        }
        for ch in 0..3 {
            o[ch] = if norm[ch] > 0.0 { acc[ch] / norm[ch] } else { ct.max(0.0) };
        }
    }
    out
}

/// The whole table, row-major (row = curvature).
pub fn generate() -> Vec<[[f32; 3]; SIZE]> {
    (0..SIZE).map(|j| row(curvature_at(j))).collect()
}

/// The blurred ramp the GPU evaluates: a ramp max(x, 0) smoothed over a
/// width `s` (the expected value of max(x + n, 0) for a bell-shaped n, with
/// the logistic stand-in for the normal CDF). Same formula as
/// `char_skin_ramp` in shaders/character.wgsl.
pub fn ramp(x: f32, s: f32) -> f32 {
    if s < 1e-4 {
        return x.max(0.0);
    }
    let z = x / s;
    let cdf = 1.0 / (1.0 + (-1.702 * z).exp());
    let pdf = 0.398_942_3 * (-0.5 * z * z).exp();
    x * cdf + s * pdf
}

/// One fitted row: per channel (width s, amplitude a) and the RMS error.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitRow {
    pub width: [f32; 3],
    pub amp: [f32; 3],
    pub rms: [f32; 3],
}

/// Least-squares fit of `a * ramp(x, s)` to one channel of one row.
fn fit_channel(row: &[[f32; 3]; SIZE], ch: usize) -> (f32, f32, f32) {
    let mut best = (0.0, 1.0, f32::MAX);
    for i in 0..=200 {
        // Widths from 0 (a hard ramp) to 3 on a log axis.
        let s = if i == 0 { 0.0 } else { 1e-3 * (3000.0f32).powf((i - 1) as f32 / 199.0) };
        let (mut dg, mut gg) = (0.0f32, 0.0f32);
        for (col, d) in row.iter().enumerate() {
            let g = ramp(ndl_at(col), s);
            dg += d[ch] * g;
            gg += g * g;
        }
        let a = if gg > 0.0 { (dg / gg).clamp(0.0, 2.0) } else { 1.0 };
        let mut e = 0.0f32;
        for (col, d) in row.iter().enumerate() {
            let r = d[ch] - a * ramp(ndl_at(col), s);
            e += r * r;
        }
        if e < best.2 {
            best = (s, a, e);
        }
    }
    (best.0, best.1, (best.2 / SIZE as f32).sqrt())
}

/// The rows the GPU gets: table rows 0, 9, ... 63, fitted.
pub fn fit(table: &[[[f32; 3]; SIZE]]) -> [FitRow; FIT_ROWS] {
    let mut out = [FitRow { width: [0.0; 3], amp: [1.0; 3], rms: [0.0; 3] }; FIT_ROWS];
    for (k, o) in out.iter_mut().enumerate() {
        let Some(r) = table.get(k * FIT_STEP) else { continue };
        for ch in 0..3 {
            let (s, a, e) = fit_channel(r, ch);
            o.width[ch] = s;
            o.amp[ch] = a;
            o.rms[ch] = e;
        }
    }
    out
}

/// Generate the table and fit it (what the plugin runs once at load).
pub fn fitted() -> [FitRow; FIT_ROWS] {
    fit(&generate())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes() {
        assert_eq!(ndl_at(0), -1.0);
        assert_eq!(ndl_at(SIZE - 1), 1.0);
        assert_eq!(curvature_at(0), 0.0);
        assert_eq!(curvature_at(SIZE - 1), C_MAX);
        assert_eq!((FIT_ROWS - 1) * FIT_STEP, SIZE - 1, "the fitted rows span the table");
        let w: f32 = PROFILE.iter().map(|p| p.1[0]).sum();
        assert!((w - 1.0).abs() < 0.01, "red weights sum to 1: {w}");
    }

    #[test]
    fn flat_skin_is_plain_lambert() {
        let t = generate();
        for (col, v) in t[0].iter().enumerate() {
            assert_eq!(*v, [ndl_at(col).max(0.0); 3]);
        }
    }

    #[test]
    fn rows_are_bounded_and_rise_with_ndl() {
        let t = generate();
        assert_eq!(t.len(), SIZE);
        for (j, r) in t.iter().enumerate() {
            for col in 0..SIZE {
                for (ch, &v) in r[col].iter().enumerate() {
                    assert!((0.0..=1.0001).contains(&v), "row {j} col {col} ch {ch}: {v}");
                    if col > 0 {
                        assert!(v + 1e-5 >= r[col - 1][ch], "row {j} col {col} ch {ch} falls");
                    }
                }
            }
        }
    }

    #[test]
    fn red_wraps_furthest_and_more_with_curvature() {
        let t = generate();
        let term = SIZE / 2; // N.L just above 0
        let mut last = 0.0;
        for r in t.iter().skip(1) {
            let [red, green, blue] = r[term];
            assert!(red + 1e-4 >= green && green + 1e-4 >= blue, "{red} {green} {blue}");
            assert!(red >= last - 1e-3, "red at the terminator grows with curvature: {red} after {last}");
            last = red;
        }
        // Light reaches past the shadow edge on tight curves, red only.
        let shadow = t[SIZE - 1][SIZE / 2 - 8]; // N.L about -0.25
        assert!(shadow[0] > 0.02 && shadow[2] < shadow[0] * 0.5, "{shadow:?}");
        // At a face's curvature (r 20 mm) it is a soft edge, not a glow.
        let face = row(0.05);
        assert!(face[SIZE / 2][0] > face[SIZE / 2][2] && face[SIZE / 2][0] < 0.2, "{:?}", face[SIZE / 2]);
    }

    #[test]
    fn the_fit_follows_the_table() {
        let t = generate();
        let f = fit(&t);
        assert_eq!(f[0].width, [0.0; 3], "flat row: a hard ramp");
        for (k, row) in f.iter().enumerate() {
            for ch in 0..3 {
                assert!(row.rms[ch] < 0.03, "row {k} ch {ch}: rms {}", row.rms[ch]);
                assert!((0.5..=1.5).contains(&row.amp[ch]), "row {k} ch {ch}: amp {}", row.amp[ch]);
            }
            if k > 0 {
                assert!(row.width[0] >= row.width[2], "red blurs wider than blue: {row:?}");
                assert!(row.width[0] >= f[k - 1].width[0], "wider with curvature");
            }
        }
    }

    #[test]
    fn ramp_limits() {
        assert_eq!(ramp(0.5, 0.0), 0.5);
        assert_eq!(ramp(-0.5, 0.0), 0.0);
        assert!((ramp(1.0, 0.01) - 1.0).abs() < 1e-3);
        assert!(ramp(-1.0, 0.01) < 1e-3);
        assert!(ramp(0.0, 0.3) > 0.1, "the edge is lifted");
    }
}
