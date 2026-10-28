//! The quality gate's two picture scores (STAGE3_PLAN S3-6, decisions 3
//! and 4), both on an original and a converted render of the same view:
//!
//! * **SSIMULACRA2** (the pinned `ssimulacra2` crate, converter only): 100 is
//!   identical, about 90 is "visually lossless" at 1:1, 70 "high quality".
//! * **Worst pixel**, our own: a per-pixel perceptual difference after the
//!   idea behind FLIP (a colour term made stronger where edges differ),
//!   then its 99.9th percentile, so one bad region fails a view even when
//!   the average is fine. Written from scratch here; no outside code.
//!
//! Per pixel, on both pictures lightly blurred (3x3 binomial, about what the
//! eye merges at 1:1) in linear light and taken to CIELAB (D65):
//! * colour `e_c = min(1, dE76 / 50)`;
//! * edges `e_f = sqrt(min(1, |G_a - G_b|))`, `G` the Sobel gradient
//!   magnitude of `L*/100` (a step from black to white is 1);
//! * `e = e_c^(1 - e_f)` (`0` when `e_c` is `0`): equal edges leave the
//!   colour difference as it is, a lost or added edge pushes it toward 1.

/// An RGBA8 picture (sRGB, top row first).
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Rgba {
    pub fn new(w: usize, h: usize, px: Vec<u8>) -> Result<Rgba, String> {
        if px.len() != w * h * 4 {
            return Err(format!("a {w}x{h} picture needs {} bytes, got {}", w * h * 4, px.len()));
        }
        Ok(Rgba { w, h, px })
    }

    /// The part inside `[x, y, w, h]` (clamped to the picture).
    pub fn crop(&self, r: [usize; 4]) -> Rgba {
        let x0 = r[0].min(self.w);
        let y0 = r[1].min(self.h);
        let x1 = (r[0] + r[2]).min(self.w);
        let y1 = (r[1] + r[3]).min(self.h);
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity(w * h * 4);
        for y in y0..y1 {
            px.extend_from_slice(&self.px[(y * self.w + x0) * 4..(y * self.w + x1) * 4]);
        }
        Rgba { w, h, px }
    }
}

/// sRGB 8-bit to linear light.
fn lut() -> [f32; 256] {
    let mut t = [0f32; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let c = i as f32 / 255.0;
        *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    }
    t
}

fn linear(img: &Rgba) -> Vec<[f32; 3]> {
    let t = lut();
    img.px.as_chunks::<4>().0.iter().map(|p| [t[p[0] as usize], t[p[1] as usize], t[p[2] as usize]]).collect()
}

/// SSIMULACRA2 of `b` against the original `a` (same size, at least 8x8).
pub fn ssimulacra2(a: &Rgba, b: &Rgba) -> Result<f64, String> {
    if (a.w, a.h) != (b.w, b.h) {
        return Err(format!("pictures differ in size ({}x{} and {}x{})", a.w, a.h, b.w, b.h));
    }
    if a.px == b.px {
        // The crate's answer for identical input is 100 up to float noise; say it exactly.
        return Ok(100.0);
    }
    let la = ssimulacra2::LinearRgb::new(linear(a), a.w, a.h).map_err(|e| format!("ssimulacra2: {e:?}"))?;
    let lb = ssimulacra2::LinearRgb::new(linear(b), b.w, b.h).map_err(|e| format!("ssimulacra2: {e:?}"))?;
    ssimulacra2::compute_frame_ssimulacra2(la, lb).map_err(|e| format!("ssimulacra2: {e}"))
}

/// 3x3 binomial blur of linear RGB (edges clamp).
fn blur(src: &[[f32; 3]], w: usize, h: usize) -> Vec<[f32; 3]> {
    const K: [f32; 3] = [0.25, 0.5, 0.25];
    let mut tmp = vec![[0f32; 3]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = [0f32; 3];
            for (k, wk) in K.iter().enumerate() {
                let xx = (x + k).saturating_sub(1).min(w - 1);
                let p = src[y * w + xx];
                for c in 0..3 {
                    s[c] += wk * p[c];
                }
            }
            tmp[y * w + x] = s;
        }
    }
    let mut out = vec![[0f32; 3]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = [0f32; 3];
            for (k, wk) in K.iter().enumerate() {
                let yy = (y + k).saturating_sub(1).min(h - 1);
                let p = tmp[yy * w + x];
                for c in 0..3 {
                    s[c] += wk * p[c];
                }
            }
            out[y * w + x] = s;
        }
    }
    out
}

/// Linear sRGB to CIELAB (D65 white).
pub fn lab(p: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = p;
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Sobel gradient magnitude of `l` (0 to 1), kernels scaled so a black to
/// white step reads 1.
fn sobel(l: &[f32], w: usize, h: usize) -> Vec<f32> {
    let at = |x: isize, y: isize| l[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
    let mut out = vec![0f32; l.len()];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let gx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1));
            let gy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1));
            out[y as usize * w + x as usize] = (gx * gx + gy * gy).sqrt() / 4.0;
        }
    }
    out
}

/// The per-pixel difference map of `b` against the original `a` (0 to 1).
pub fn error_map(a: &Rgba, b: &Rgba) -> Result<Vec<f32>, String> {
    if (a.w, a.h) != (b.w, b.h) {
        return Err(format!("pictures differ in size ({}x{} and {}x{})", a.w, a.h, b.w, b.h));
    }
    let (w, h) = (a.w, a.h);
    if w == 0 || h == 0 {
        return Ok(Vec::new());
    }
    let la: Vec<[f32; 3]> = blur(&linear(a), w, h).into_iter().map(lab).collect();
    let lb: Vec<[f32; 3]> = blur(&linear(b), w, h).into_iter().map(lab).collect();
    let ga = sobel(&la.iter().map(|p| p[0] / 100.0).collect::<Vec<_>>(), w, h);
    let gb = sobel(&lb.iter().map(|p| p[0] / 100.0).collect::<Vec<_>>(), w, h);
    Ok((0..w * h)
        .map(|i| {
            let d = ((la[i][0] - lb[i][0]).powi(2) + (la[i][1] - lb[i][1]).powi(2) + (la[i][2] - lb[i][2]).powi(2)).sqrt();
            let ec = (d / 50.0).min(1.0);
            if ec <= 0.0 {
                return 0.0;
            }
            let ef = (ga[i] - gb[i]).abs().min(1.0).sqrt();
            ec.powf(1.0 - ef)
        })
        .collect())
}

/// The `q` quantile (0 to 1) of `v` (nearest rank).
pub fn quantile(v: &[f32], q: f64) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    let k = ((q * s.len() as f64).ceil() as usize).clamp(1, s.len()) - 1;
    let (_, x, _) = s.select_nth_unstable_by(k, f32::total_cmp);
    *x
}

/// Both scores of one view.
#[derive(Clone, Debug, PartialEq)]
pub struct Scores {
    pub ssimulacra2: f64,
    /// 99.9th percentile of [`error_map`].
    pub worst_pixel: f64,
    /// Where the difference is largest (in the scored picture).
    pub worst_at: [usize; 2],
}

/// Score `b` against the original `a`.
pub fn score(a: &Rgba, b: &Rgba) -> Result<(Scores, Vec<f32>), String> {
    let s2 = if a.w >= 8 && a.h >= 8 { ssimulacra2(a, b)? } else if a.px == b.px { 100.0 } else { 0.0 };
    let map = error_map(a, b)?;
    let wp = quantile(&map, 0.999);
    let i = map.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1)).map_or(0, |m| m.0);
    let at = if a.w > 0 { [i % a.w, i / a.w] } else { [0, 0] };
    Ok((Scores { ssimulacra2: s2, worst_pixel: f64::from(wp), worst_at: at }, map))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, c: [u8; 3]) -> Rgba {
        Rgba::new(w, h, (0..w * h).flat_map(|_| [c[0], c[1], c[2], 255]).collect()).unwrap()
    }

    #[test]
    fn identical_is_perfect_and_a_change_is_not() {
        let mut a = flat(32, 32, [120, 80, 40]);
        for y in 8..24 {
            for x in 8..24 {
                a.px[(y * 32 + x) * 4] = 250;
            }
        }
        let (s, _) = score(&a, &a).unwrap();
        assert_eq!((s.ssimulacra2, s.worst_pixel), (100.0, 0.0));
        let b = flat(32, 32, [120, 80, 40]);
        let (t, map) = score(&a, &b).unwrap();
        assert!(t.ssimulacra2 < 90.0, "{t:?}");
        assert!(t.worst_pixel > 0.35, "{t:?}");
        assert!(map[16 * 32 + 16] > 0.5);
        assert_eq!(map[0], 0.0);
    }

    #[test]
    fn quantile_is_nearest_rank() {
        let v: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(quantile(&v, 0.999), 998.0);
        assert_eq!(quantile(&v, 1.0), 999.0);
        assert_eq!(quantile(&[], 0.5), 0.0);
    }

    #[test]
    fn crop_clamps() {
        let a = flat(10, 6, [1, 2, 3]);
        let c = a.crop([8, 4, 5, 5]);
        assert_eq!((c.w, c.h, c.px.len()), (2, 2, 16));
    }
}
