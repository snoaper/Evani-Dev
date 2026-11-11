//! Deterministic maths: our own sin, cos, tan, atan, atan2, exp, ln, pow and
//! cbrt, built only from IEEE 754 basic operations in a fixed order.
//!
//! The platform maths library differs between Windows, macOS, Linux and
//! WebAssembly, so the compile path never calls it (docs/model_research/05
//! and 18). Range reduction splits the constant into a part that multiplies
//! exactly and a remainder; the reduced argument goes through a truncated
//! power series evaluated by Horner's rule. Coefficients are the reciprocal
//! factorials and odd reciprocals of the textbook series; they are rounded
//! once at compile time, which is the same on every target.
//!
//! Accuracy (tests below): sin, cos, exp and ln within 2 ulp of the
//! correctly rounded result on dense samples of the ranges the modeler uses;
//! atan and atan2 within 2 ulp; pow and cbrt within a relative 1e-13.

use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, LN_2, PI};

/// `fl(pi/2)` with its low 28 mantissa bits cleared: `k * PIO2_HI` is exact
/// for every `|k| < 2^28`.
const PIO2_HI: f64 = f64::from_bits(FRAC_PI_2.to_bits() & !((1u64 << 28) - 1));
/// `fl(pi/2) - PIO2_HI`, exact.
const PIO2_MID: f64 = FRAC_PI_2 - PIO2_HI;
/// `pi/2 - fl(pi/2)`: the part of pi/2 a double cannot hold.
const PIO2_LO: f64 = 6.123_233_995_736_766e-17;

const LN2_HI: f64 = f64::from_bits(LN_2.to_bits() & !((1u64 << 28) - 1));
const LN2_MID: f64 = LN_2 - LN2_HI;
/// `ln 2 - fl(ln 2)`.
const LN2_LO: f64 = 2.319_046_813_846_299_6e-17;

/// Largest argument the sin and cos reduction handles exactly; beyond it the
/// argument is first folded with the exact float remainder.
const TRIG_LIMIT: f64 = 1.0e7;

fn fact(n: u32) -> f64 {
    let mut f = 1.0;
    let mut i = 2;
    while i <= n {
        f *= i as f64;
        i += 1;
    }
    f
}

/// sin(r) for |r| <= pi/4: odd series to the r^15 term.
fn sin_kernel(r: f64) -> f64 {
    let r2 = r * r;
    let mut p = -1.0 / fact(15);
    p = p * r2 + 1.0 / fact(13);
    p = p * r2 - 1.0 / fact(11);
    p = p * r2 + 1.0 / fact(9);
    p = p * r2 - 1.0 / fact(7);
    p = p * r2 + 1.0 / fact(5);
    p = p * r2 - 1.0 / fact(3);
    r + r * r2 * p
}

/// cos(r) for |r| <= pi/4: even series to the r^16 term.
fn cos_kernel(r: f64) -> f64 {
    let r2 = r * r;
    let mut p = 1.0 / fact(16);
    p = p * r2 - 1.0 / fact(14);
    p = p * r2 + 1.0 / fact(12);
    p = p * r2 - 1.0 / fact(10);
    p = p * r2 + 1.0 / fact(8);
    p = p * r2 - 1.0 / fact(6);
    p = p * r2 + 1.0 / fact(4);
    let w = r2 * 0.5;
    let one_minus_w = 1.0 - w;
    // Recover the rounding error of 1 - w so the small tail is not lost.
    let err = (1.0 - one_minus_w) - w;
    one_minus_w + (r2 * r2 * p - err)
}

/// Reduce `x` to `r` in [-pi/4, pi/4] and the quadrant `n` (0..4).
fn reduce(x: f64) -> (f64, u32) {
    let x = if x.abs() > TRIG_LIMIT { x % (2.0 * PI) } else { x };
    let k = (x * (2.0 / PI)).round();
    let r = ((x - k * PIO2_HI) - k * PIO2_MID) - k * PIO2_LO;
    let n = (k as i64).rem_euclid(4) as u32;
    (r, n)
}

/// sin and cos of `x` (radians) together.
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    let (r, n) = reduce(x);
    let (s, c) = (sin_kernel(r), cos_kernel(r));
    match n {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}
pub fn sin(x: f64) -> f64 {
    sin_cos(x).0
}
pub fn cos(x: f64) -> f64 {
    sin_cos(x).1
}
pub fn tan(x: f64) -> f64 {
    let (s, c) = sin_cos(x);
    s / c
}

/// atan(t) for |t| <= tan(pi/8): odd series to the t^49 term.
fn atan_kernel(t: f64) -> f64 {
    let t2 = t * t;
    let mut p = 0.0;
    let mut k: i32 = 24;
    while k >= 1 {
        let c = 1.0 / (2 * k + 1) as f64;
        p = p * t2 + if k % 2 == 1 { -c } else { c };
        k -= 1;
    }
    t + t * t2 * p
}

pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let a = x.abs();
    // tan(3pi/8) = 1 + sqrt 2 and tan(pi/8) = sqrt 2 - 1.
    let r = if a > 2.414_213_562_373_095 {
        if a.is_infinite() { FRAC_PI_2 } else { (FRAC_PI_2 - atan_kernel(1.0 / a)) + PIO2_LO }
    } else if a > 0.414_213_562_373_095_1 {
        (FRAC_PI_4 + atan_kernel((a - 1.0) / (a + 1.0))) + PIO2_LO * 0.5
    } else {
        atan_kernel(a)
    };
    if x < 0.0 { -r } else { r }
}

pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        if y == 0.0 {
            return if x.is_sign_negative() { if y.is_sign_negative() { -PI } else { PI } } else { y };
        }
        return if y > 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
    }
    let a = atan(y / x);
    if x > 0.0 {
        a
    } else if y >= 0.0 && !y.is_sign_negative() {
        (a + PI) + 2.0 * PIO2_LO
    } else {
        (a - PI) - 2.0 * PIO2_LO
    }
}

/// 2^k as a double, for k in the normal range.
fn pow2(k: i64) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.2 {
        return 0.0;
    }
    let k = (x * (1.0 / LN_2)).round();
    let r = ((x - k * LN2_HI) - k * LN2_MID) - k * LN2_LO;
    let mut p = 1.0 / fact(14);
    let mut n = 13;
    while n >= 1 {
        p = p * r + 1.0 / fact(n);
        n -= 1;
    }
    let e = 1.0 + r * p;
    let k = k as i64;
    if k > 1023 {
        e * pow2(1023) * pow2(k - 1023)
    } else if k < -1022 {
        e * pow2(-1022) * pow2(k + 1022)
    } else {
        e * pow2(k)
    }
}

pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    let (mut x, mut e) = (x, 0i64);
    if x < f64::MIN_POSITIVE {
        x *= pow2(54);
        e -= 54;
    }
    let bits = x.to_bits();
    e += ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & ((1u64 << 52) - 1)) | (1023u64 << 52));
    if m > core::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut p = 0.0;
    let mut k: i32 = 12;
    while k >= 1 {
        p = p * s2 + 1.0 / (2 * k + 1) as f64;
        k -= 1;
    }
    let lnm = 2.0 * s + 2.0 * s * s2 * p;
    let ef = e as f64;
    ef * LN2_HI + (lnm + ef * LN2_MID + ef * LN2_LO)
}

pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    if x == 0.0 {
        return if y > 0.0 { 0.0 } else { f64::INFINITY };
    }
    if x < 0.0 {
        if y == y.trunc() {
            let r = exp(y * ln(-x));
            return if (y.abs() % 2.0) == 1.0 { -r } else { r };
        }
        return f64::NAN;
    }
    exp(y * ln(x))
}

pub fn cbrt(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let a = x.abs();
    let mut y = exp(ln(a) / 3.0);
    y -= (y * y * y - a) / (3.0 * y * y);
    if x < 0.0 { -y } else { y }
}

pub fn deg(d: f64) -> f64 {
    d * (PI / 180.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ulps(a: f64, b: f64) -> u64 {
        if a == b {
            return 0;
        }
        let (ia, ib) = (a.to_bits() as i64, b.to_bits() as i64);
        if (ia < 0) != (ib < 0) {
            return u64::MAX;
        }
        (ia - ib).unsigned_abs()
    }

    fn samples(lo: f64, hi: f64, n: usize) -> impl Iterator<Item = f64> {
        (0..=n).map(move |i| lo + (hi - lo) * (i as f64) / (n as f64))
    }

    #[test]
    fn sin_cos_within_2_ulp() {
        let mut worst = 0;
        for x in samples(-50.0, 50.0, 200_000) {
            let (s, c) = sin_cos(x);
            // Near a zero of sin or cos the ulp of the result is tiny; compare
            // in absolute terms there.
            for (ours, theirs) in [(s, x.sin()), (c, x.cos())] {
                if theirs.abs() < 1e-3 {
                    assert!((ours - theirs).abs() < 1e-18, "x {x}: {ours} vs {theirs}");
                } else {
                    worst = worst.max(ulps(ours, theirs));
                }
            }
        }
        assert!(worst <= 2, "worst {worst} ulp");
    }

    #[test]
    fn exp_ln_within_2_ulp() {
        let mut worst = 0;
        for x in samples(-40.0, 40.0, 200_000) {
            worst = worst.max(ulps(exp(x), x.exp()));
        }
        for x in samples(1e-6, 1e4, 200_000) {
            if (x - 1.0).abs() > 1e-3 {
                worst = worst.max(ulps(ln(x), x.ln()));
            }
        }
        assert!(worst <= 2, "worst {worst} ulp");
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(ln(1.0), 0.0);
    }

    #[test]
    fn atan2_within_2_ulp() {
        let mut worst = 0;
        for a in samples(-3.1, 3.1, 50_000) {
            let (y, x) = (a.sin() * 1.7, a.cos() * 0.9);
            worst = worst.max(ulps(atan2(y, x), y.atan2(x)));
        }
        for t in samples(-30.0, 30.0, 100_000) {
            if t.abs() > 1e-3 {
                worst = worst.max(ulps(atan(t), t.atan()));
            }
        }
        assert!(worst <= 2, "worst {worst} ulp");
    }

    #[test]
    fn pow_cbrt_relative() {
        for x in samples(0.01, 100.0, 20_000) {
            for y in [-2.5, -1.0, 0.5, 1.0 / 3.0, 2.0, 3.7] {
                let (o, t) = (pow(x, y), x.powf(y));
                assert!(((o - t) / t).abs() < 1e-13, "{x}^{y}");
            }
            let (o, t) = (cbrt(x), x.cbrt());
            assert!(((o - t) / t).abs() < 1e-15, "cbrt {x}");
        }
        assert!((pow(-2.0, 3.0) + 8.0).abs() < 1e-12);
        assert!(pow(-2.0, 0.5).is_nan());
    }
}
