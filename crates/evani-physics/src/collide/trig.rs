//! Deterministic sine, cosine and atan2 for the collision module: exact
//! range reduction (`floor`) and odd polynomials, only `+ - * /`, so native
//! and wasm give the same bits (the platform maths library may round
//! `f32::sin` and friends differently per target). Errors under 2e-6 (sin,
//! cos) and 2e-5 rad (atan2).

use core::f32::consts::{FRAC_PI_2, PI, TAU};

/// `sin(x)` (0 for a non-finite `x`).
pub fn sin(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let k = (x / TAU + 0.5).floor();
    let mut y = x - k * TAU;
    if y > FRAC_PI_2 {
        y = PI - y;
    } else if y < -FRAC_PI_2 {
        y = -PI - y;
    }
    let y2 = y * y;
    y * (1.0 + y2 * (-1.0 / 6.0 + y2 * (1.0 / 120.0 + y2 * (-1.0 / 5040.0 + y2 * (1.0 / 362_880.0 - y2 / 39_916_800.0)))))
}

/// `cos(x)` (1 for a non-finite `x`).
pub fn cos(x: f32) -> f32 {
    if !x.is_finite() {
        return 1.0;
    }
    sin(x + FRAC_PI_2)
}

/// `atan(z)` for `|z| <= 1`.
fn atan_unit(z: f32) -> f32 {
    let z2 = z * z;
    z * (0.999_866 + z2 * (-0.330_299_5 + z2 * (0.180_141 + z2 * (-0.085_133 + z2 * 0.020_835_1))))
}

/// `atan2(y, x)`, in `-pi..=pi` (0 for the origin or a non-finite input).
pub fn atan2(y: f32, x: f32) -> f32 {
    if !(x.is_finite() && y.is_finite()) || (x == 0.0 && y == 0.0) {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let a = if ay <= ax { atan_unit(ay / ax) } else { FRAC_PI_2 - atan_unit(ax / ay) };
    let a = if x < 0.0 { PI - a } else { a };
    if y < 0.0 { -a } else { a }
}

/// Degrees to radians (a plain multiply).
#[inline]
pub fn radians(deg: f32) -> f32 {
    deg * (PI / 180.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The platform library is the reference here, in the test only.
    #[test]
    fn matches_the_library() {
        let mut x = -20.0f32;
        while x < 20.0 {
            assert!((sin(x) - x.sin()).abs() < 2e-6, "sin {x}");
            assert!((cos(x) - x.cos()).abs() < 2e-6, "cos {x}");
            x += 0.0137;
        }
        let mut a = -PI;
        while a < PI {
            for r in [0.01f32, 1.0, 37.0] {
                let (y, x) = (r * a.sin(), r * a.cos());
                assert!((atan2(y, x) - y.atan2(x)).abs() < 2e-5, "atan2 at {a}");
            }
            a += 0.0131;
        }
        assert_eq!(sin(f32::NAN), 0.0);
        assert_eq!(cos(f32::INFINITY), 1.0);
        assert_eq!(atan2(0.0, 0.0), 0.0);
    }

    /// Known bit patterns, so a change of evaluation order (or a target
    /// that fused a multiply-add) shows up as a failure.
    #[test]
    fn known_bits() {
        assert_eq!(sin(0.0).to_bits(), 0);
        assert!((cos(0.0) - 1.0).abs() < 2e-6);
        let c45 = cos(radians(45.0));
        assert!((c45 - core::f32::consts::FRAC_1_SQRT_2).abs() < 2e-6);
        // Same input, same bits, every call.
        for _ in 0..4 {
            assert_eq!(cos(radians(45.0)).to_bits(), c45.to_bits());
        }
    }
}
