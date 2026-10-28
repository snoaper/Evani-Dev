//! Decimal number text without the standard library's float formatting.
//!
//! `format!("{:.1}", x)` on an `f32`/`f64` links core's float-to-decimal
//! code (Grisu, Dragon4 and their tables, a few KB gzip) into every game
//! that uses it once. [`fixed`] writes the same text from integer
//! arithmetic: `format!("{}", fixed(x, 1))` is byte for byte
//! `format!("{:.1}", x)`, padding, `+` and `0` flags included (tests
//! compare the two over many values). Use it for HUD numbers, CSS values
//! and log lines in code that ships (README "How data ships").

use core::fmt;

/// `x` to exactly `decimals` places, rounded the way `{:.N}` rounds (the
/// exact binary value, ties to even). See the module docs.
pub fn fixed(x: impl Into<f64>, decimals: u8) -> Fixed {
    Fixed(x.into(), decimals)
}

/// `x` like `{}` prints it, for numbers with at most `decimals` places:
/// [`fixed`] to `decimals` places, then trailing zeros and a bare `.`
/// dropped (`2.50` -> `2.5`, `3.000` -> `3`, `-0.000` -> `-0`). For a value
/// whose shortest form has more places it rounds to `decimals` places,
/// which is what CSS lengths and percentages need anyway.
///
/// Limit: it prints the stored value, not the shortest round-trip text, so
/// it matches `{}` only where that text has at most `decimals` places and
/// the number is small enough (tests: every k / 10^n up to 2,000,000 / 10^n,
/// n = 0..4, as f32 and f64). Huge f32s differ (`1e11` is `99999997952`
/// here, `100000000000` from `{}`). Use it for screen lengths, percentages
/// and similar; not for arbitrary data.
pub fn short(x: impl Into<f64>, decimals: u8) -> Short {
    Short(x.into(), decimals)
}

/// A number shown like `{}` with at most a few decimals; make one with [`short`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Short(pub f64, pub u8);

impl fmt::Display for Short {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Short(v, n) = *self;
        if !v.is_finite() {
            return fmt::Display::fmt(&Fixed(v, 0), f);
        }
        let mut digits = String::new();
        unsigned(&mut digits, v.abs(), n.min(MAX_DECIMALS));
        if digits.contains('.') {
            let keep = digits.trim_end_matches('0').trim_end_matches('.').len();
            digits.truncate(keep);
        }
        f.pad_integral(!v.is_sign_negative(), "", &digits)
    }
}

/// `x` like `{:?}` prints it (always a fraction: `1.0`, `0.85`), for
/// numbers with at most `decimals` places; see [`short`].
pub fn short_debug(x: impl Into<f64>, decimals: u8) -> String {
    let v = x.into();
    let mut s = short(v, decimals).to_string();
    if v.is_finite() && !s.contains('.') {
        s.push_str(".0");
    }
    s
}

/// `x.clamp(lo, hi)` without its panic. `f32::clamp` asserts `lo <= hi`,
/// and that assert's message prints both bounds with float `Debug`, which
/// links the float formatting code into the game wherever the compiler
/// can't prove the bounds are ordered. Same result as `f32::clamp` for
/// ordered bounds (NaN `x` stays NaN); unordered or NaN bounds give a
/// value instead of a panic.
#[inline]
pub fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    let mut v = x;
    if v < lo {
        v = lo;
    }
    if v > hi {
        v = hi;
    }
    v
}

/// A number shown with a fixed count of decimals; make one with [`fixed`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fixed(pub f64, pub u8);

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Fixed(v, n) = *self;
        if v.is_nan() {
            return f.pad_integral(true, "", "NaN");
        }
        let neg = v.is_sign_negative();
        if v.is_infinite() {
            return f.pad_integral(!neg, "", "inf");
        }
        let mut digits = String::new();
        unsigned(&mut digits, v.abs(), n.min(MAX_DECIMALS));
        // Past 17 places every further digit of an f64 is exact zeros or
        // the tail of its binary expansion; `{:.N}` beyond that is not used.
        for _ in MAX_DECIMALS..n {
            digits.push('0');
        }
        f.pad_integral(!neg, "", &digits)
    }
}

/// Decimals computed exactly; 10^17 still fits the u128 arithmetic below.
const MAX_DECIMALS: u8 = 17;

/// `v` (finite, not negative) with `n` decimals, no sign.
fn unsigned(out: &mut String, v: f64, n: u8) {
    let bits = v.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    // v = m * 2^e exactly.
    let (m, e) = if exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), exp - 1075) };
    let scale = 10u128.pow(u32::from(n));
    if e >= 0 {
        // A whole number (at least 2^52): its digits, then n zeros.
        whole_digits(out, m, e as u32);
        if n > 0 {
            out.push('.');
            for _ in 0..n {
                out.push('0');
            }
        }
        return;
    }
    // q = round(m * 10^n / 2^sh), ties to even; m * 10^n < 2^110.
    let sh = e.unsigned_abs();
    let num = u128::from(m) * scale;
    let q = if sh >= 128 {
        0 // below half of the last place: rounds to zero
    } else {
        let q = num >> sh;
        let rem = num & ((1u128 << sh) - 1);
        let half = 1u128 << (sh - 1);
        if rem > half || (rem == half && q & 1 == 1) { q + 1 } else { q }
    };
    // v < 2^53 here, so the whole part fits a u64.
    push_u64(out, (q / scale) as u64, 0);
    if n > 0 {
        out.push('.');
        push_u64(out, (q % scale) as u64, n);
    }
}

/// Decimal digits of `x`, zero-padded to at least `width`.
fn push_u64(out: &mut String, mut x: u64, width: u8) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
        if x == 0 {
            break;
        }
    }
    for _ in (buf.len() - i)..usize::from(width) {
        out.push('0');
    }
    for &d in &buf[i..] {
        out.push(char::from(d));
    }
}

/// Decimal digits of `m * 2^e` (e up to 971: up to 309 digits).
fn whole_digits(out: &mut String, m: u64, e: u32) {
    // Little-endian base-10^9 limbs.
    let mut limbs: Vec<u32> = Vec::new();
    let mut x = m;
    while x > 0 {
        limbs.push((x % 1_000_000_000) as u32);
        x /= 1_000_000_000;
    }
    for _ in 0..e {
        let mut carry = 0u64;
        for l in &mut limbs {
            let t = u64::from(*l) * 2 + carry;
            *l = (t % 1_000_000_000) as u32;
            carry = t / 1_000_000_000;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }
    }
    let mut first = true;
    for &l in limbs.iter().rev() {
        push_u64(out, u64::from(l), if first { 0 } else { 9 });
        first = false;
    }
    if first {
        out.push('0');
    }
}

/// Move `v` towards `target` by at most `step`.
#[inline]
pub fn approach(v: f32, target: f32, step: f32) -> f32 {
    if v < target {
        (v + step).min(target)
    } else {
        (v - step).max(target)
    }
}

/// Wrap an angle to `-PI..PI`.
#[inline]
pub fn wrap_angle(a: f32) -> f32 {
    let a = (a + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU);
    a - core::f32::consts::PI
}

/// Smooth `0..1` ease, `x` clamped first: `x * x * (3 - 2x)`.
#[inline]
pub fn smooth01(x: f32) -> f32 {
    let x = clamp(x, 0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_easing_helpers_match_the_kit_copies_they_replaced() {
        assert_eq!(approach(0.0, 1.0, 0.25), 0.25);
        assert_eq!(approach(0.9, 1.0, 0.25), 1.0);
        assert_eq!(approach(0.0, -1.0, 0.25), -0.25);
        assert_eq!(approach(-0.9, -1.0, 0.25), -1.0);
        for i in -40..=40 {
            let a = i as f32 * 0.37;
            let w = wrap_angle(a);
            assert!((-core::f32::consts::PI..core::f32::consts::PI).contains(&w), "{a} -> {w}");
            assert_eq!(w, (a + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU) - core::f32::consts::PI);
        }
        for i in -5..=25 {
            let x = i as f32 * 0.05;
            let c = x.clamp(0.0, 1.0);
            assert_eq!(smooth01(x), c * c * (3.0 - 2.0 * c));
        }
    }

    fn check(v: f64, n: u8) {
        assert_eq!(format!("{}", fixed(v, n)), format!("{v:.*}", usize::from(n)), "{v:e} to {n}");
        let w = v as f32;
        assert_eq!(format!("{}", fixed(w, n)), format!("{w:.*}", usize::from(n)), "f32 {w:e} to {n}");
    }

    #[test]
    fn matches_std_on_edges_and_ties() {
        let edges = [
            0.0, -0.0, 0.5, 1.5, 2.5, -0.5, 0.25, 0.125, 0.375, 1.005, 2.675, 0.05, -0.04, 0.95, 9.95, 99.5,
            999.9999, 1e-7, 1e-300, 5e-324, 123456.789, 4503599627370496.5, 9007199254740993.0, 1e21, 1e300,
            f64::MAX, f64::MIN_POSITIVE, 16.6666667, 33.333333, 60.0, 0.1, 0.2, 0.3, 0.7, 1.0 / 3.0,
        ];
        for v in edges {
            for n in 0..=6 {
                check(v, n);
                check(-v, n);
            }
        }
    }

    /// The exact shapes the shipped HUDs and pages use (lean-data): pixel
    /// CSS `{:.0}` and `{:.1}`, the saved volume `{:.2}`, the zero-padded
    /// race time `{:05.2}`, all as f32 over and past their screen ranges.
    #[test]
    fn hud_shapes_match_std() {
        for k in -40_000..=100_000i32 {
            let px = k as f32 * 0.0731;
            assert_eq!(fixed(px, 0).to_string(), format!("{px:.0}"), "{px}");
            assert_eq!(fixed(px, 1).to_string(), format!("{px:.1}"), "{px}");
        }
        for k in 0..=100_000i32 {
            let v = k as f32 / 100_000.0;
            assert_eq!(fixed(v, 2).to_string(), format!("{v:.2}"), "{v}");
            let pct = (v * 100.0).round();
            assert_eq!(fixed(pct, 0).to_string(), format!("{pct}"), "{pct}");
            let t = k as f32 * 0.000_61;
            assert_eq!(format!("{:05}", fixed(t, 2)), format!("{t:05.2}"), "{t}");
        }
    }

    #[test]
    fn matches_std_on_many_values() {
        // Deterministic spread over magnitudes: random bit patterns plus
        // values on and near n-decimal ties.
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..40_000 {
            let r = next();
            let v = f64::from_bits(r);
            if !v.is_finite() {
                continue;
            }
            check(v, (r % 5) as u8);
            // Typical game magnitudes, and exact ties at n decimals.
            let g = (r >> 11) as f64 / (1u64 << 53) as f64 * 10f64.powi((r % 9) as i32 - 2);
            for n in 0..=4u8 {
                check(g, n);
                let tie = ((g * 10f64.powi(i32::from(n))).floor() + 0.5) / 10f64.powi(i32::from(n));
                check(tie, n);
            }
        }
    }

    #[test]
    fn short_matches_display_up_to_its_decimals() {
        for n in 0..=4u8 {
            let p = 10i64.pow(u32::from(n));
            for k in (-2_000_000i64..=2_000_000).step_by(7) {
                let x = k as f32 / p as f32;
                assert_eq!(format!("{}", short(x, n)), format!("{x}"), "f32 {k}/{p}");
                let y = k as f64 / p as f64;
                assert_eq!(format!("{}", short(y, n)), format!("{y}"), "f64 {k}/{p}");
            }
        }
        for v in [0.0f32, -0.0, 1.0, -3.0, 0.5, 12.25, 100.0, 7.125] {
            assert_eq!(format!("{}", short(v, 3)), format!("{v}"));
        }
        assert_eq!(format!("{}", short(1.0f32 / 3.0, 3)), "0.333");
        // The documented limit: a huge f32 prints its stored value.
        assert_eq!(format!("{}", short(1e11f32, 0)), "99999997952");
        // CSS sizes the touch layer writes: up to a 4K screen in px.
        for px in [3839.5f32, 2160.25, 1919.875, -0.5, 0.125] {
            assert_eq!(format!("{}", short(px, 3)), format!("{px}"));
        }
        for k in -3000i32..=3000 {
            let x = k as f32 / 100.0;
            assert_eq!(short_debug(x, 2), format!("{x:?}"), "debug {k}/100");
        }
        assert_eq!(format!("{}", short(f32::INFINITY, 2)), format!("{}", f32::INFINITY));
        assert_eq!(format!("{}", short(f32::NAN, 2)), format!("{}", f32::NAN));
    }

    #[test]
    fn clamp_matches_std() {
        let vals = [-2.0f32, -0.0, 0.0, 0.25, 1.0, 3.5, f32::INFINITY, f32::NEG_INFINITY];
        for x in vals {
            for (lo, hi) in [(0.0f32, 1.0f32), (-1.0, -1.0), (-0.0, 0.0), (0.5, 10.0)] {
                assert_eq!(clamp(x, lo, hi).to_bits(), x.clamp(lo, hi).to_bits(), "{x} in {lo}..{hi}");
            }
        }
        assert!(clamp(f32::NAN, 0.0, 1.0).is_nan());
        assert_eq!(clamp(5.0, 2.0, 1.0), 1.0);
    }

    #[test]
    fn honours_width_sign_and_zero_flags() {
        for v in [0.0, 3.14151, -3.14151, 12.5, -0.001, 59.999] {
            assert_eq!(format!("{:05}", fixed(v, 2)), format!("{v:05.2}"));
            assert_eq!(format!("{:+.1}", fixed(v, 1)), format!("{v:+.1}"));
            assert_eq!(format!("{:>8}", fixed(v, 1)), format!("{v:>8.1}"));
            assert_eq!(format!("{:<8}|", fixed(v, 0)), format!("{v:<8.0}|"));
            assert_eq!(format!("{:^9}", fixed(v, 3)), format!("{v:^9.3}"));
        }
        assert_eq!(format!("{}", fixed(f64::INFINITY, 1)), format!("{:.1}", f64::INFINITY));
        assert_eq!(format!("{}", fixed(f64::NEG_INFINITY, 1)), format!("{:.1}", f64::NEG_INFINITY));
        assert_eq!(format!("{}", fixed(f64::NAN, 1)), format!("{:.1}", f64::NAN));
    }
}
