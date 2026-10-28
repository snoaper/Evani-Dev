//! Look statistics, version 1 ([`crate::guardset::STAT_VERSION`]).
//!
//! All computed on the final sRGB capture (8-bit RGB, alpha ignored).
//! Changing any formula here changes what "passes", so it must bump
//! `STAT_VERSION`, which changes every look's guard hash and needs Chris's
//! approval again.
//!
//! - `mean_luma`: mean Rec.709 luma `0.2126 R + 0.7152 G + 0.0722 B` of the
//!   sRGB-encoded values, 0–1.
//! - `log_avg`: log-average luminance of the linearised image,
//!   `exp(mean(ln(1e-4 + Y_lin)))`, 0–1 (Reinhard's "key").
//! - `luma_std`: standard deviation of luma, 0–1.
//! - `luma_range`: luma p95 − p5 (nearest rank on luma rounded to 0–255), 0–1.
//! - `saturation`: mean HSV saturation over pixels with V > 0.1, 0–1.
//! - `colorfulness`: Hasler & Süsstrunk (2003) on 0–255 values,
//!   `σ_rgyb + 0.3 μ_rgyb` with `rg = R − G`, `yb = (R + G) / 2 − B`.
//! - `clipped`: fraction of pixels with max(R, G, B) ≥ 250.
//! - `crushed`: fraction of pixels with luma ≤ 5 (on 0–255).
//! - `bloom`: fraction of pixels whose luma with bloom on is more than
//!   3/255 above the same pixel with bloom off (needs the pair).

use serde_json::{Map, Value, json};

use crate::png::Rgb;

/// How a stat is compared with its baseline (Layer A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `|value − baseline| ≤ tol`.
    Abs,
    /// `|value − baseline| ≤ max(tol × |baseline|, min)`.
    Rel,
    /// One-sided: `value ≤ baseline + tol` (more is worse, less is fine).
    Up,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Abs => "abs",
            Mode::Rel => "rel",
            Mode::Up => "up",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "abs" => Some(Mode::Abs),
            "rel" => Some(Mode::Rel),
            "up" => Some(Mode::Up),
            _ => None,
        }
    }
}

/// One stat's fixed description.
#[derive(Debug, Clone, Copy)]
pub struct StatDef {
    pub id: &'static str,
    /// Plain words for the breach sentence.
    pub label: &'static str,
    /// Shown as a percentage (a fraction 0–1) or as a plain number.
    pub percent: bool,
    /// Default Layer A tolerance (placeholder; tuned on real baselines).
    pub mode: Mode,
    pub tol: f64,
    /// Smallest allowed difference for `Rel` stats near zero.
    pub tol_min: f64,
    /// Layer B: the smallest widening of the band on each side.
    pub band_floor: f64,
}

/// Every stat, in report order.
pub const STATS: [StatDef; 9] = [
    StatDef { id: "mean_luma", label: "average brightness", percent: false, mode: Mode::Abs, tol: 0.04, tol_min: 0.0, band_floor: 0.02 },
    StatDef { id: "log_avg", label: "log-average brightness", percent: false, mode: Mode::Rel, tol: 0.20, tol_min: 0.005, band_floor: 0.01 },
    StatDef { id: "luma_std", label: "contrast (brightness spread)", percent: false, mode: Mode::Rel, tol: 0.15, tol_min: 0.01, band_floor: 0.01 },
    StatDef { id: "luma_range", label: "contrast range (p5 to p95)", percent: false, mode: Mode::Rel, tol: 0.15, tol_min: 0.02, band_floor: 0.02 },
    StatDef { id: "saturation", label: "saturation", percent: false, mode: Mode::Rel, tol: 0.15, tol_min: 0.02, band_floor: 0.02 },
    StatDef { id: "colorfulness", label: "colorfulness", percent: false, mode: Mode::Abs, tol: 10.0, tol_min: 0.0, band_floor: 3.0 },
    StatDef { id: "clipped", label: "highlights clipped", percent: true, mode: Mode::Up, tol: 0.01, tol_min: 0.0, band_floor: 0.005 },
    StatDef { id: "crushed", label: "shadows crushed", percent: true, mode: Mode::Up, tol: 0.02, tol_min: 0.0, band_floor: 0.01 },
    StatDef { id: "bloom", label: "bloom coverage", percent: true, mode: Mode::Abs, tol: 0.05, tol_min: 0.0, band_floor: 0.02 },
];

pub fn def(id: &str) -> Option<&'static StatDef> {
    STATS.iter().find(|d| d.id == id)
}

/// A value as shown in sentences: `4.1%` or `0.452`.
pub fn fmt(d: &StatDef, v: f64) -> String {
    if d.percent {
        format!("{:.1}%", v * 100.0)
    } else if d.id == "colorfulness" {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
    }
}

/// The stats of one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub mean_luma: f64,
    pub log_avg: f64,
    pub luma_std: f64,
    pub luma_range: f64,
    pub saturation: f64,
    pub colorfulness: f64,
    pub clipped: f64,
    pub crushed: f64,
    /// `None` without a bloom-off capture.
    pub bloom: Option<f64>,
}

impl Stats {
    pub fn get(&self, id: &str) -> Option<f64> {
        match id {
            "mean_luma" => Some(self.mean_luma),
            "log_avg" => Some(self.log_avg),
            "luma_std" => Some(self.luma_std),
            "luma_range" => Some(self.luma_range),
            "saturation" => Some(self.saturation),
            "colorfulness" => Some(self.colorfulness),
            "clipped" => Some(self.clipped),
            "crushed" => Some(self.crushed),
            "bloom" => self.bloom,
            _ => None,
        }
    }

    /// As a JSON object, every value rounded to 6 decimals so the file is
    /// stable and readable.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        for d in &STATS {
            if let Some(v) = self.get(d.id) {
                m.insert(d.id.into(), json!(round6(v)));
            }
        }
        Value::Object(m)
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let g = |k: &str| v[k].as_f64().ok_or_else(|| format!("stats have no number {k}"));
        Ok(Stats {
            mean_luma: g("mean_luma")?,
            log_avg: g("log_avg")?,
            luma_std: g("luma_std")?,
            luma_range: g("luma_range")?,
            saturation: g("saturation")?,
            colorfulness: g("colorfulness")?,
            clipped: g("clipped")?,
            crushed: g("crushed")?,
            bloom: v["bloom"].as_f64(),
        })
    }
}

pub fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn luma255(p: &[u8]) -> f64 {
    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The stats of one capture; `nobloom` is the same moment with bloom off
/// (same size), for `bloom`.
pub fn compute(img: &Rgb, nobloom: Option<&Rgb>) -> Result<Stats, String> {
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
        let y_lin = 0.2126 * lin[usize::from(p[0])] + 0.7152 * lin[usize::from(p[1])] + 0.0722 * lin[usize::from(p[2])];
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
                .filter(|&(a, b)| luma255(a) - luma255(b) > 3.0 + 1e-9)
                .count();
            Some(lit as f64 / nf)
        }
    };
    Ok(Stats {
        mean_luma: mean / 255.0,
        log_avg: (sum_log / nf).exp(),
        luma_std: var.sqrt() / 255.0,
        luma_range: (rank(0.95) - rank(0.05)) / 255.0,
        saturation: if sat_n == 0 { 0.0 } else { sat_sum / sat_n as f64 },
        colorfulness: (rg_var + yb_var).sqrt() + 0.3 * (rg_m * rg_m + yb_m * yb_m).sqrt(),
        clipped: clipped as f64 / nf,
        crushed: crushed as f64 / nf,
        bloom,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn solid(w: u32, h: u32, c: [u8; 3]) -> Rgb {
        Rgb { w, h, px: c.iter().copied().cycle().take((w * h * 3) as usize).collect() }
    }

    /// Fill rows top-down with colours, `rows[i]` covering an equal share.
    pub fn bands(w: u32, h: u32, colours: &[[u8; 3]]) -> Rgb {
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
        // sRGB 128 is about 0.2159 linear.
        assert!(close(s.log_avg, 0.2159 + 1e-4, 1e-3), "{}", s.log_avg);
        let black = compute(&solid(4, 4, [0, 0, 0]), None).unwrap();
        assert!(close(black.log_avg, 1e-4, 1e-9));
        assert!(close(black.mean_luma, 0.0, 1e-12));
        // Green weighs more than blue in luma.
        let g = compute(&solid(2, 2, [0, 255, 0]), None).unwrap();
        let b = compute(&solid(2, 2, [0, 0, 255]), None).unwrap();
        assert!(close(g.mean_luma, 0.7152, 1e-9) && close(b.mean_luma, 0.0722, 1e-9));
    }

    #[test]
    fn contrast() {
        let flat = compute(&solid(10, 10, [90, 90, 90]), None).unwrap();
        assert!(close(flat.luma_std, 0.0, 1e-9) && close(flat.luma_range, 0.0, 1e-12));
        // Half black, half white: std 0.5, p95 − p5 = 1.
        let bw = compute(&bands(10, 10, &[[0, 0, 0], [255, 255, 255]]), None).unwrap();
        assert!(close(bw.luma_std, 0.5, 1e-9), "{}", bw.luma_std);
        assert!(close(bw.luma_range, 1.0, 1e-12));
        // 40 rows: 1 dark, 38 mid, 1 bright (2.5% each): the 5% tails are cut off.
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
        // rg = 200, yb = 100, no spread: C = 0.3 * sqrt(200² + 100²).
        assert!(close(red.colorfulness, 0.3 * (200.0f64 * 200.0 + 100.0 * 100.0).sqrt(), 1e-9));
        let half = compute(&solid(4, 4, [200, 100, 100]), None).unwrap();
        assert!(close(half.saturation, 0.5, 1e-12));
        // Pixels with V ≤ 0.1 don't count towards saturation.
        let dark = compute(&bands(4, 4, &[[20, 0, 0], [200, 100, 100]]), None).unwrap();
        assert!(close(dark.saturation, 0.5, 1e-12));
    }

    #[test]
    fn clipped_and_crushed() {
        let s = compute(&bands(10, 10, &[[255, 10, 10], [250, 0, 0], [249, 249, 249], [5, 5, 5], [6, 6, 6]]), None).unwrap();
        // Each band is 20%; two bands have max(R, G, B) ≥ 250.
        assert!(close(s.clipped, 0.4, 1e-12), "{}", s.clipped);
        // [250,0,0] has luma 53; [5,5,5] has luma 5 (crushed), [6,6,6] not.
        assert!(close(s.crushed, 0.2, 1e-12), "{}", s.crushed);
    }

    #[test]
    fn bloom_coverage() {
        let off = solid(10, 10, [50, 50, 50]);
        let mut on = off.clone();
        // 30 pixels brighter by 10, 10 pixels brighter by exactly 3 (not counted).
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

    #[test]
    fn json_round_trip() {
        let s = compute(&bands(8, 8, &[[10, 200, 30], [250, 250, 250]]), Some(&solid(8, 8, [0, 0, 0]))).unwrap();
        let back = Stats::from_json(&s.to_json()).unwrap();
        for d in &STATS {
            assert!(close(back.get(d.id).unwrap(), s.get(d.id).unwrap(), 1e-6), "{}", d.id);
        }
        assert_eq!(fmt(def("clipped").unwrap(), 0.041), "4.1%");
        assert_eq!(fmt(def("mean_luma").unwrap(), 0.4523), "0.452");
    }
}
