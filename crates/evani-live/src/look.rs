//! Look-check measures of a capture, for `sweep.run` (docs/LIVE_API.md 6.29).
//!
//! The same definitions as the look guard's statistics, version 1
//! (`tools/evani-lookguard/src/stats.rs` on the `look-guard` branch, which
//! is a native tool and not a dependency of the engine): computed on the
//! final sRGB capture (8-bit RGB). If the guard's formulas change, change
//! these with them; the tests pin the same reference values.
//!
//! - `mean_luma`: mean Rec.709 luma `0.2126 R + 0.7152 G + 0.0722 B` of the
//!   sRGB-encoded values, 0–1.
//! - `log_avg`: log-average luminance of the linearised image,
//!   `exp(mean(ln(1e-4 + Y_lin)))`, 0–1.
//! - `luma_std`: standard deviation of luma, 0–1.
//! - `luma_range`: luma p95 − p5 (nearest rank on luma rounded to 0–255), 0–1.
//! - `saturation`: mean HSV saturation over pixels with V > 0.1, 0–1.
//! - `colorfulness`: Hasler & Süsstrunk (2003) on 0–255 values,
//!   `σ_rgyb + 0.3 μ_rgyb` with `rg = R − G`, `yb = (R + G) / 2 − B`.
//! - `clipped`: fraction of pixels with max(R, G, B) ≥ 250.
//! - `crushed`: fraction of pixels with luma ≤ 5 (on 0–255).
//! - `bloom`: fraction of pixels whose luma with bloom on is more than
//!   3/255 above the same pixel with bloom off (needs the pair).
//!
//! Dev only, like the rest of evani-live.

use crate::img::Rgb;

/// The measure names, in table order.
pub const NAMES: [&str; 9] = [
    "mean_luma",
    "log_avg",
    "luma_std",
    "luma_range",
    "saturation",
    "colorfulness",
    "clipped",
    "crushed",
    "bloom",
];

/// The measures of one capture (or the mean over several).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Look {
    pub mean_luma: f64,
    pub log_avg: f64,
    pub luma_std: f64,
    pub luma_range: f64,
    pub saturation: f64,
    pub colorfulness: f64,
    pub clipped: f64,
    pub crushed: f64,
    /// `None` without a bloom-off capture of the same view.
    pub bloom: Option<f64>,
}

impl Look {
    /// The values in [`NAMES`] order.
    pub fn values(&self) -> [Option<f64>; 9] {
        [
            Some(self.mean_luma),
            Some(self.log_avg),
            Some(self.luma_std),
            Some(self.luma_range),
            Some(self.saturation),
            Some(self.colorfulness),
            Some(self.clipped),
            Some(self.crushed),
            self.bloom,
        ]
    }

    /// The mean of several captures' measures (`bloom` only when every one
    /// has it). `None` for an empty list.
    pub fn mean(list: &[Look]) -> Option<Look> {
        if list.is_empty() {
            return None;
        }
        let n = list.len() as f64;
        let avg = |f: fn(&Look) -> f64| list.iter().map(f).sum::<f64>() / n;
        let bloom = list
            .iter()
            .map(|l| l.bloom)
            .collect::<Option<Vec<f64>>>()
            .map(|v| v.iter().sum::<f64>() / n);
        Some(Look {
            mean_luma: avg(|l| l.mean_luma),
            log_avg: avg(|l| l.log_avg),
            luma_std: avg(|l| l.luma_std),
            luma_range: avg(|l| l.luma_range),
            saturation: avg(|l| l.saturation),
            colorfulness: avg(|l| l.colorfulness),
            clipped: avg(|l| l.clipped),
            crushed: avg(|l| l.crushed),
            bloom,
        })
    }
}

fn luma255(p: &[u8; 3]) -> f64 {
    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The measures of one capture; `nobloom` is the same view with bloom off
/// (same size), for `bloom`.
pub fn compute(img: &Rgb, nobloom: Option<&Rgb>) -> Result<Look, String> {
    let n = (img.w as usize) * (img.h as usize);
    if n == 0 || img.px.len() != n * 3 {
        return Err("empty or malformed image".into());
    }
    let lin: Vec<f64> = (0..=255u8).map(srgb_to_linear).collect();
    let nf = n as f64;
    let (mut sum_l, mut sum_l2, mut sum_log) = (0.0, 0.0, 0.0);
    let mut hist = [0u64; 256];
    let (mut sat_sum, mut sat_n) = (0.0, 0u64);
    let (mut rg_s, mut rg_s2, mut yb_s, mut yb_s2) = (0.0, 0.0, 0.0, 0.0);
    let (mut clipped, mut crushed) = (0u64, 0u64);
    for p in img.px.as_chunks::<3>().0 {
        let l = luma255(p);
        sum_l += l;
        sum_l2 += l * l;
        hist[l.round().clamp(0.0, 255.0) as usize] += 1;
        let y_lin = 0.2126 * lin[usize::from(p[0])]
            + 0.7152 * lin[usize::from(p[1])]
            + 0.0722 * lin[usize::from(p[2])];
        sum_log += (1e-4 + y_lin).ln();
        let mx = p[0].max(p[1]).max(p[2]);
        let mn = p[0].min(p[1]).min(p[2]);
        if f64::from(mx) / 255.0 > 0.1 {
            sat_sum += f64::from(mx - mn) / f64::from(mx);
            sat_n += 1;
        }
        let (r, g, b) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        let rg = r - g;
        let yb = 0.5 * (r + g) - b;
        rg_s += rg;
        rg_s2 += rg * rg;
        yb_s += yb;
        yb_s2 += yb * yb;
        if mx >= 250 {
            clipped += 1;
        }
        // + 1e-9: the luma of an exact grey is its level, up to rounding.
        if l <= 5.0 + 1e-9 {
            crushed += 1;
        }
    }
    let mean = sum_l / nf;
    let var = (sum_l2 / nf - mean * mean).max(0.0);
    let rank = |q: f64| -> f64 {
        // Nearest rank: the smallest value with at least ceil(q n) pixels at or below it.
        let k = ((q * nf).ceil() as u64).max(1);
        let mut acc = 0;
        for (i, c) in hist.iter().enumerate() {
            acc += c;
            if acc >= k {
                return i as f64;
            }
        }
        255.0
    };
    let (rg_m, yb_m) = (rg_s / nf, yb_s / nf);
    let rg_var = (rg_s2 / nf - rg_m * rg_m).max(0.0);
    let yb_var = (yb_s2 / nf - yb_m * yb_m).max(0.0);
    let bloom = match nobloom {
        None => None,
        Some(off) => {
            if (off.w, off.h) != (img.w, img.h) || off.px.len() != img.px.len() {
                return Err(format!(
                    "the bloom-off capture is {}x{}, the capture is {}x{}",
                    off.w, off.h, img.w, img.h
                ));
            }
            let lit = img
                .px
                .as_chunks::<3>()
                .0
                .iter()
                .zip(off.px.as_chunks::<3>().0)
                .filter(|(a, b)| luma255(a) - luma255(b) > 3.0 + 1e-9)
                .count();
            Some(lit as f64 / nf)
        }
    };
    Ok(Look {
        mean_luma: mean / 255.0,
        log_avg: (sum_log / nf).exp(),
        luma_std: var.sqrt() / 255.0,
        luma_range: (rank(0.95) - rank(0.05)) / 255.0,
        saturation: if sat_n == 0 {
            0.0
        } else {
            sat_sum / sat_n as f64
        },
        colorfulness: (rg_var + yb_var).sqrt() + 0.3 * (rg_m * rg_m + yb_m * yb_m).sqrt(),
        clipped: clipped as f64 / nf,
        crushed: crushed as f64 / nf,
        bloom,
    })
}

#[cfg(test)]
mod tests {
    //! The look guard's own test cases (stats.rs), so both stay in step.
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 3]) -> Rgb {
        Rgb {
            w,
            h,
            px: c.iter().copied().cycle().take((w * h * 3) as usize).collect(),
        }
    }

    fn bands(w: u32, h: u32, colours: &[[u8; 3]]) -> Rgb {
        let mut px = Vec::new();
        for y in 0..h {
            let c = colours[(y as usize * colours.len()) / h as usize];
            for _ in 0..w {
                px.extend_from_slice(&c);
            }
        }
        Rgb { w, h, px }
    }

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn brightness() {
        let s = compute(&solid(4, 4, [128, 128, 128]), None).unwrap();
        assert!(close(s.mean_luma, 128.0 / 255.0, 1e-9));
        assert!(close(s.log_avg, 0.2159 + 1e-4, 1e-3), "{}", s.log_avg);
        let black = compute(&solid(4, 4, [0, 0, 0]), None).unwrap();
        assert!(close(black.log_avg, 1e-4, 1e-9));
        assert!(close(black.mean_luma, 0.0, 1e-12));
        let g = compute(&solid(2, 2, [0, 255, 0]), None).unwrap();
        let b = compute(&solid(2, 2, [0, 0, 255]), None).unwrap();
        assert!(close(g.mean_luma, 0.7152, 1e-9) && close(b.mean_luma, 0.0722, 1e-9));
    }

    #[test]
    fn contrast() {
        let flat = compute(&solid(10, 10, [90, 90, 90]), None).unwrap();
        assert!(close(flat.luma_std, 0.0, 1e-9) && close(flat.luma_range, 0.0, 1e-12));
        let bw = compute(&bands(10, 10, &[[0, 0, 0], [255, 255, 255]]), None).unwrap();
        assert!(close(bw.luma_std, 0.5, 1e-9), "{}", bw.luma_std);
        assert!(close(bw.luma_range, 1.0, 1e-12));
        let mut colours = vec![[100, 100, 100]; 40];
        colours[0] = [0, 0, 0];
        colours[39] = [255, 255, 255];
        let t = compute(&bands(5, 40, &colours), None).unwrap();
        assert!(close(t.luma_range, 0.0, 1e-12), "{}", t.luma_range);
    }

    #[test]
    fn saturation_and_colorfulness() {
        let grey = compute(&solid(4, 4, [120, 120, 120]), None).unwrap();
        assert!(close(grey.saturation, 0.0, 1e-12) && close(grey.colorfulness, 0.0, 1e-9));
        let red = compute(&solid(4, 4, [200, 0, 0]), None).unwrap();
        assert!(close(red.saturation, 1.0, 1e-12));
        assert!(close(red.colorfulness, 0.3 * (200.0f64 * 200.0 + 100.0 * 100.0).sqrt(), 1e-9));
        let half = compute(&solid(4, 4, [200, 100, 100]), None).unwrap();
        assert!(close(half.saturation, 0.5, 1e-12));
        let dark = compute(&bands(4, 4, &[[20, 0, 0], [200, 100, 100]]), None).unwrap();
        assert!(close(dark.saturation, 0.5, 1e-12));
    }

    #[test]
    fn clipped_and_crushed() {
        let s = compute(
            &bands(10, 10, &[[255, 10, 10], [250, 0, 0], [249, 249, 249], [5, 5, 5], [6, 6, 6]]),
            None,
        )
        .unwrap();
        assert!(close(s.clipped, 0.4, 1e-12), "{}", s.clipped);
        assert!(close(s.crushed, 0.2, 1e-12), "{}", s.crushed);
    }

    #[test]
    fn bloom_coverage() {
        let off = solid(10, 10, [50, 50, 50]);
        let mut on = off.clone();
        for i in 0..30 {
            for c in 0..3 {
                on.px[i * 3 + c] = 60;
            }
        }
        for i in 30..40 {
            for c in 0..3 {
                on.px[i * 3 + c] = 53;
            }
        }
        let s = compute(&on, Some(&off)).unwrap();
        assert!(close(s.bloom.unwrap(), 0.3, 1e-12));
        assert_eq!(compute(&on, None).unwrap().bloom, None);
        assert!(compute(&on, Some(&solid(5, 5, [0, 0, 0]))).is_err());
    }

    /// A synthetic scene with every measure non-trivial, checked against
    /// numbers the look guard's `stats::compute` gives for the same image
    /// (stats.rs from the look-guard branch run on this image, printed with
    /// 9 decimals).
    #[test]
    fn matches_the_look_guard_on_a_synthetic_scene() {
        let img = bands(
            8,
            10,
            &[[255, 255, 255], [250, 240, 200], [180, 60, 40], [30, 120, 200], [3, 3, 3]],
        );
        let off = bands(
            8,
            10,
            &[[240, 240, 240], [250, 240, 200], [170, 55, 38], [30, 120, 200], [3, 3, 3]],
        );
        let s = compute(&img, Some(&off)).unwrap();
        let want = crate::look::tests::GUARD_SYNTHETIC;
        for (i, v) in s.values().iter().enumerate() {
            let v = v.unwrap();
            assert!(close(v, want[i], 2e-6), "{}: {v} vs {}", NAMES[i], want[i]);
        }
    }

    /// The look guard's values for the synthetic scene above, in [`NAMES`] order.
    pub(super) const GUARD_SYNTHETIC: [f64; 9] = [
        0.539_567_059,
        0.115_507_786,
        0.376_365_489,
        0.988_235_294,
        0.456_944_444,
        98.660_064_409,
        0.4,
        0.2,
        0.4,
    ];

    #[test]
    fn mean_of_captures() {
        let a = compute(&solid(2, 2, [0, 0, 0]), Some(&solid(2, 2, [0, 0, 0]))).unwrap();
        let b = compute(&solid(2, 2, [255, 255, 255]), None).unwrap();
        let m = Look::mean(&[a, b]).unwrap();
        assert!(close(m.mean_luma, 0.5, 1e-9));
        assert!(close(m.clipped, 0.5, 1e-12) && close(m.crushed, 0.5, 1e-12));
        assert_eq!(m.bloom, None, "bloom only when every capture has it");
        assert_eq!(Look::mean(&[]), None);
    }
}
