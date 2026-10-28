//! Slipstream (drafting): a kart tucked in close behind another kart on a
//! straight gets a higher top speed for as long as it stays there.
//!
//! The rule, checked for every kart (player, second player and AI) at the
//! start of each fixed step, from one snapshot of all karts so grid order
//! does not matter:
//!
//! - another kart is ahead inside a narrow cone along this kart's heading
//!   ([`CONE_HALF_ANGLE_DEG`]), no more than [`RANGE_KART_LENGTHS`] kart
//!   lengths from its tail, heading the same way and moving;
//! - this kart is on a straight: the steering input is small
//!   ([`MAX_STEER`]) and the road ahead barely bends ([`MAX_CURVATURE`] over
//!   the next [`CURVATURE_LOOKAHEAD_M`]);
//! - it is driving normally: on the road, not drifting or spun out, and above
//!   [`MIN_SPEED`].
//!
//! While all of that holds, [`crate::kart::step`] multiplies the kart's speed
//! cap by [`SLIPSTREAM_MULTIPLIER`] (exactly +5 %). The moment one condition
//! fails (it leaves the cone, steers, or reaches a corner) the cap goes back
//! and the extra speed bleeds off the way any over-cap speed does.
//! Pure functions, no allocation.

use crate::kart::{Kart, wrap_angle};
use crate::track::Track;

/// The one tuning knob: the speed cap multiplier while slipstreaming.
pub const SLIPSTREAM_MULTIPLIER: f32 = 1.05;

/// Length of a kart (m): the models are about 3.1 m long (see `hull.rs`).
pub const KART_LENGTH_M: f32 = 3.1;
/// How far back the slipstream reaches, in kart lengths from the leader's
/// tail to the follower's nose.
pub const RANGE_KART_LENGTHS: f32 = 3.0;
/// Half-angle of the cone behind the leader, seen from the follower (deg).
pub const CONE_HALF_ANGLE_DEG: f32 = 8.0;
/// The two karts must point within this of each other (deg).
pub const MAX_HEADING_DIFF_DEG: f32 = 25.0;
/// Largest steering input (0..1) that still counts as driving straight.
pub const MAX_STEER: f32 = 0.15;
/// Largest mean track curvature ahead (1/m) that still counts as a straight
/// (the AI drivers' own "straight" threshold).
pub const MAX_CURVATURE: f32 = 0.012;
/// How far ahead the curvature is read (m).
pub const CURVATURE_LOOKAHEAD_M: f32 = 30.0;
/// Below this speed (m/s) there is no slipstream to speak of; the leader
/// must be moving at least half of it.
pub const MIN_SPEED: f32 = 12.0;

/// Why a kart is (or is not) in a slipstream this step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Slip {
    /// Drafting behind kart `.0` (grid index).
    Behind(usize),
    /// Not driving normally: off the road, drifting, spun out or slow.
    #[default]
    NotDriving,
    /// Steering more than [`MAX_STEER`].
    Turning,
    /// The road ahead bends more than [`MAX_CURVATURE`].
    Corner,
    /// Nobody close ahead inside the cone.
    NoCone,
}

impl Slip {
    pub fn active(self) -> bool {
        matches!(self, Slip::Behind(_))
    }

    /// Short word for the debug log.
    pub fn label(self) -> &'static str {
        match self {
            Slip::Behind(_) => "in cone",
            Slip::NotDriving => "not driving",
            Slip::Turning => "turning",
            Slip::Corner => "corner",
            Slip::NoCone => "left cone",
        }
    }
}

/// Whether kart `i` of `karts` (one snapshot of every kart this step) is in a
/// slipstream, given its steering input `steer`.
pub fn check(karts: &[Kart], i: usize, steer: f32, track: &Track) -> Slip {
    let Some(k) = karts.get(i) else {
        return Slip::NotDriving;
    };
    if !k.on_road || k.drift != 0.0 || k.spin > 0.0 || k.speed < MIN_SPEED {
        return Slip::NotDriving;
    }
    if steer.abs() > MAX_STEER {
        return Slip::Turning;
    }
    if track.curvature_ahead(k.s, CURVATURE_LOOKAHEAD_M).abs() > MAX_CURVATURE {
        return Slip::Corner;
    }
    match leader(karts, i) {
        Some(j) => Slip::Behind(j),
        None => Slip::NoCone,
    }
}

/// The nearest kart ahead of kart `i` whose slipstream it is in, by geometry
/// alone (cone, range, heading, leader moving).
pub fn leader(karts: &[Kart], i: usize) -> Option<usize> {
    let k = karts.get(i)?;
    let f = k.forward();
    let l = k.left();
    let tan_cone = CONE_HALF_ANGLE_DEG.to_radians().tan();
    let max_heading = MAX_HEADING_DIFF_DEG.to_radians();
    // Centre to centre: one kart length (nose to tail touching) plus the range.
    let reach = KART_LENGTH_M * (1.0 + RANGE_KART_LENGTHS);
    let mut best: Option<(usize, f32)> = None;
    for (j, o) in karts.iter().enumerate() {
        if j == i || o.speed < MIN_SPEED * 0.5 || o.spin > 0.0 {
            continue;
        }
        if wrap_angle(o.yaw - k.yaw).abs() > max_heading {
            continue;
        }
        let d = [o.pos[0] - k.pos[0], o.pos[2] - k.pos[2]];
        let along = d[0] * f[0] + d[1] * f[1];
        if along < KART_LENGTH_M * 0.9 || along > reach {
            continue;
        }
        let side = (d[0] * l[0] + d[1] * l[1]).abs();
        if side > along * tan_cone {
            continue;
        }
        if best.is_none_or(|(_, a)| along < a) {
            best = Some((j, along));
        }
    }
    best.map(|(j, _)| j)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tune;
    use crate::kart::{self, Controls};

    fn track() -> Track {
        let mut t = Track::from_json(include_str!("../data/track.json")).unwrap();
        t.set_line_inset(0.55);
        t
    }

    /// The start of the straightest 60 m on the track: the smallest largest
    /// bend along it (so an S-bend, whose mean cancels out, never wins).
    fn straight_s(t: &Track) -> f32 {
        let mut best = (f32::MAX, 0.0);
        let mut s = 0.0;
        while s < t.length {
            let mut worst = 0.0f32;
            let mut d = 0.0;
            while d < 60.0 {
                worst = worst.max(t.curvature_ahead(s + d, 2.0).abs());
                d += 2.0;
            }
            if worst < best.0 {
                best = (worst, s);
            }
            s += 2.0;
        }
        best.1
    }

    fn kart_at(t: &Track, s: f32, lateral: f32, speed: f32) -> Kart {
        let p = t.point_at(s, lateral);
        let tan = t.tangent_at(s);
        let mut k = Kart::new(0, None, p, tan[0].atan2(tan[1]));
        k.relocate(t);
        k.speed = speed;
        k
    }

    fn flat(_: f32) -> f32 {
        1.0
    }

    #[test]
    fn multiplier_is_exactly_five_percent() {
        assert_eq!(SLIPSTREAM_MULTIPLIER, 1.05);
    }

    #[test]
    fn right_behind_on_a_straight_is_in_the_slipstream() {
        let t = track();
        let s = straight_s(&t);
        let karts = [kart_at(&t, s, 0.0, 25.0), kart_at(&t, s + 6.0, 0.0, 25.0)];
        assert_eq!(check(&karts, 0, 0.0, &t), Slip::Behind(1));
        // The leader gets nothing from the kart behind it.
        assert_eq!(check(&karts, 1, 0.0, &t), Slip::NoCone);
    }

    #[test]
    fn too_far_back_or_off_to_the_side_is_not() {
        let t = track();
        let s = straight_s(&t);
        let reach = KART_LENGTH_M * (1.0 + RANGE_KART_LENGTHS);
        let far = [kart_at(&t, s, 0.0, 25.0), kart_at(&t, s + reach + 1.0, 0.0, 25.0)];
        assert_eq!(check(&far, 0, 0.0, &t), Slip::NoCone);
        let near = [kart_at(&t, s, 0.0, 25.0), kart_at(&t, s + reach - 0.5, 0.0, 25.0)];
        assert_eq!(check(&near, 0, 0.0, &t), Slip::Behind(1));
        // Side by side, a lane over: outside the narrow cone.
        let beside = [kart_at(&t, s, 0.0, 25.0), kart_at(&t, s + 6.0, 2.5, 25.0)];
        assert_eq!(check(&beside, 0, 0.0, &t), Slip::NoCone);
    }

    #[test]
    fn steering_or_a_corner_ends_it() {
        let t = track();
        let s = straight_s(&t);
        let karts = [kart_at(&t, s, 0.0, 25.0), kart_at(&t, s + 6.0, 0.0, 25.0)];
        assert_eq!(check(&karts, 0, 0.5, &t), Slip::Turning);
        // The tightest corner on the track.
        let mut corner = (0.0, 0.0);
        let mut x = 0.0;
        while x < t.length {
            let c = t.curvature_ahead(x, CURVATURE_LOOKAHEAD_M).abs();
            if c > corner.0 {
                corner = (c, x);
            }
            x += 2.0;
        }
        let karts = [kart_at(&t, corner.1, 0.0, 25.0), kart_at(&t, corner.1 + 5.0, 0.0, 25.0)];
        assert_eq!(check(&karts, 0, 0.0, &t), Slip::Corner);
    }

    #[test]
    fn the_cap_rises_by_exactly_the_multiplier_and_falls_back() {
        let t = track();
        let s = straight_s(&t);
        let tune = Tune::default();
        let gas = Controls { throttle: 1.0, ..Default::default() };
        // Held at the plain cap, then drafting: the cap and the speed it
        // settles at are 5 % higher.
        let mut k = kart_at(&t, s, 0.0, tune.max_speed);
        kart::step(&mut k, gas, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        assert!((k.cap - tune.max_speed).abs() < 1e-4, "{}", k.cap);
        let mut k = kart_at(&t, s, 0.0, tune.max_speed);
        k.slipstream = true;
        for _ in 0..90 {
            kart::step(&mut k, gas, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        }
        assert!((k.cap - tune.max_speed * SLIPSTREAM_MULTIPLIER).abs() < 1e-4, "{}", k.cap);
        assert!((k.cap_base - tune.max_speed).abs() < 1e-4);
        assert!((k.speed - tune.max_speed * 1.05).abs() < 0.01, "{}", k.speed);
        // Out of the slipstream (back at the start of the straight, at the
        // drafting speed): the cap drops back at once and the extra speed
        // bleeds off within about a second.
        let mut k = kart_at(&t, s, 0.0, tune.max_speed * SLIPSTREAM_MULTIPLIER);
        for _ in 0..60 {
            kart::step(&mut k, gas, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        }
        assert!((k.cap - tune.max_speed).abs() < 1e-4, "{}", k.cap);
        assert!(k.speed < tune.max_speed + 0.1 && k.speed >= tune.max_speed, "{}", k.speed);
    }
}
