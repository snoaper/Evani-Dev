//! Deterministic power and tangent for combat: exact range reduction (the
//! float's own exponent bits, `floor`) and short polynomials, only
//! `+ - * /` and comparisons, so native and wasm give the same bits (the
//! platform maths library may round `f32::powf` and `f32::tan` differently
//! per target). Relative error under 1e-6 for `exp2` and `log2` inputs a
//! game uses (falloff bases in `0..=1`, exponents up to a few hundred).

use evani_physics::collide::trig::{cos, sin};

const LN2: f32 = core::f32::consts::LN_2;
const LOG2_E: f32 = core::f32::consts::LOG2_E;
const SQRT2: f32 = core::f32::consts::SQRT_2;

/// `2^y` (0 under the smallest float, infinity over the largest; NaN stays
/// NaN).
pub fn exp2(y: f32) -> f32 {
    if y.is_nan() {
        return f32::NAN;
    }
    if y >= 128.0 {
        return f32::INFINITY;
    }
    if y < -149.0 {
        return 0.0;
    }
    // y = n + f, f in [-0.5, 0.5]: 2^f = e^(f ln 2), |f ln 2| <= 0.347.
    let n = (y + 0.5).floor();
    let x = (y - n) * LN2;
    let p = 1.0
        + x * (1.0
            + x * (0.5
                + x * (1.0 / 6.0
                    + x * (1.0 / 24.0 + x * (1.0 / 120.0 + x * (1.0 / 720.0 + x / 5040.0))))));
    let n = n as i32;
    if n >= -126 {
        p * pow2i(n)
    } else {
        // Into the subnormals: two exact scalings.
        p * pow2i(n + 64) * pow2i(-64)
    }
}

/// `2^n` for `n` in `-126..=127`, built from the exponent bits (exact).
fn pow2i(n: i32) -> f32 {
    let e = (n + 127).clamp(1, 254) as u32;
    f32::from_bits(e << 23)
}

/// `log2(x)` for `x > 0` (negative infinity at 0, NaN below 0 or for NaN,
/// infinity for infinity).
pub fn log2(x: f32) -> f32 {
    if x.is_nan() || x < 0.0 {
        return f32::NAN;
    }
    if x == 0.0 {
        return f32::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f32::INFINITY;
    }
    // Normalise subnormals first, then split x = m * 2^e with m in
    // [sqrt(1/2), sqrt(2)).
    let (x, bias) = if x < f32::MIN_POSITIVE {
        (x * pow2i(64), -64)
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let mut e = ((bits >> 23) & 0xff) as i32 - 127 + bias;
    let mut m = f32::from_bits((bits & 0x007f_ffff) | (127 << 23));
    if m > SQRT2 {
        m *= 0.5;
        e += 1;
    }
    // ln m = 2 atanh(s), s = (m - 1) / (m + 1), |s| <= 0.1716.
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let ln_m = 2.0 * s * (1.0 + s2 * (1.0 / 3.0 + s2 * (1.0 / 5.0 + s2 * (1.0 / 7.0 + s2 / 9.0))));
    e as f32 + ln_m * LOG2_E
}

/// `base^exponent` for `base >= 0` (what range falloff needs). `x^0` is 1,
/// `1^y` is 1, `0^y` is 0 for `y > 0` and infinity for `y < 0`; a negative
/// or non-finite base, or a non-finite exponent, gives NaN.
pub fn powf(base: f32, exponent: f32) -> f32 {
    if exponent == 0.0 || base == 1.0 {
        return 1.0;
    }
    if !base.is_finite() || !exponent.is_finite() || base < 0.0 {
        return f32::NAN;
    }
    if base == 0.0 {
        return if exponent > 0.0 { 0.0 } else { f32::INFINITY };
    }
    exp2(exponent * log2(base))
}

/// `tan(x)` from the deterministic sine and cosine (a large finite value
/// near the poles, 0 for a non-finite `x`).
pub fn tan(x: f32) -> f32 {
    let c = cos(x);
    let s = sin(x);
    if c.abs() < 1e-30 {
        return if s < 0.0 { -1e30 } else { 1e30 };
    }
    s / c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The platform library is the reference here, in the test only.
    #[test]
    fn pow_log_exp_match_the_library() {
        // Normal results only: a subnormal has too few bits for a relative bound.
        let mut y = -125.0f32;
        while y < 127.0 {
            let (a, b) = (exp2(y), y.exp2());
            assert!((a - b).abs() <= b * 2e-6, "exp2 {y}: {a} vs {b}");
            y += 0.173;
        }
        let mut x = 1e-30f32;
        while x < 1e30 {
            let (a, b) = (log2(x), x.log2());
            assert!((a - b).abs() < 2e-5, "log2 {x}: {a} vs {b}");
            x *= 1.37;
        }
        for base in [0.5f32, 0.79, 0.85, 0.98, 1.0, 1.3, 2.0] {
            for e in [0.0f32, 0.1, 1.0, 2.5, 10.5, 16.8, 40.0, -3.0] {
                let (a, b) = (powf(base, e), base.powf(e));
                assert!(
                    (a - b).abs() <= b.abs() * 1e-5 + 1e-30,
                    "{base}^{e}: {a} vs {b}"
                );
            }
        }
        assert_eq!(powf(0.0, 2.0), 0.0);
        assert_eq!(powf(0.0, -1.0), f32::INFINITY);
        assert!(powf(-2.0, 2.0).is_nan());
        assert_eq!(powf(0.3, 0.0), 1.0);
        let mut t = -1.5f32;
        while t < 1.5 {
            assert!(
                (tan(t) - t.tan()).abs() <= t.tan().abs() * 1e-4 + 2e-6,
                "tan {t}"
            );
            t += 0.0113;
        }
    }

    /// Known bits, so a change of evaluation order shows up as a failure.
    #[test]
    fn exact_cases() {
        assert_eq!(exp2(0.0).to_bits(), 1.0f32.to_bits());
        assert_eq!(exp2(10.0), 1024.0);
        assert_eq!(exp2(-3.0), 0.125);
        assert_eq!(log2(8.0), 3.0);
        assert_eq!(log2(1.0), 0.0);
        assert_eq!(powf(2.0, 3.0), 8.0);
        let k = powf(0.98, 1.7);
        for _ in 0..4 {
            assert_eq!(powf(0.98, 1.7).to_bits(), k.to_bits());
        }
    }
}
