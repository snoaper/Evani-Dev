//! The track's wall geometry the race rules need (no drawing): ring
//! spacing, how far each side may reach on the inside of a bend, and where
//! the rail stands. [`crate::trackmesh`] builds the meshes from the same
//! numbers and re-exports these, so the rules build without the graphics.

use crate::track::Track;

/// About this far between rings (m); the ring count is a multiple of four
/// so the red and white pieces alternate all the way round.
pub const RING_STEP: f32 = 2.0;
/// Kerb width beside the road edge (m).
pub const KERB_W: f32 = 0.9;
/// Rail centre past the verge's outer edge and its width (m).
pub const RAIL_OUT: f32 = 0.3;
pub const RAIL_W: f32 = 0.5;
/// Inside a bend, nothing reaches past this share of the bend's radius.
pub const FOLD: f32 = 0.9;
/// Rings either side a tight bend's limit spreads to (a smooth envelope).
const ENVELOPE: usize = 8;

/// Ring positions along the track (m from the start line), evenly spaced.
pub fn rings(track: &Track) -> Vec<f32> {
    let n = ((track.length / RING_STEP / 4.0).round() as usize).max(2) * 4;
    (0..n).map(|j| j as f32 * track.length / n as f32).collect()
}

/// Shortest signed angle `b - a` (radians).
pub(crate) fn wrap(mut d: f32) -> f32 {
    while d > core::f32::consts::PI {
        d -= core::f32::consts::TAU;
    }
    while d < -core::f32::consts::PI {
        d += core::f32::consts::TAU;
    }
    d
}

/// How far out each side may reach at each ring: `[left, right]` (m,
/// `f32::MAX` on the outside of a bend). From the heading change between
/// neighbouring rings, then the smallest within [`ENVELOPE`] rings.
pub fn limits(track: &Track, ss: &[f32]) -> Vec<[f32; 2]> {
    let n = ss.len();
    let ds = track.length / n as f32;
    let yaw = |s: f32| {
        let t = track.tangent_at(s);
        t[0].atan2(t[1])
    };
    let raw: Vec<[f32; 2]> = ss
        .iter()
        .map(|&s| {
            // Positive turns left (as Sample::curvature): the inside is left.
            let k = wrap(yaw(s + ds) - yaw(s - ds)) / (2.0 * ds);
            let r = FOLD / k.abs().max(1e-6);
            if k > 0.0 { [r, f32::MAX] } else { [f32::MAX, r] }
        })
        .collect();
    (0..n)
        .map(|j| {
            let mut m = [f32::MAX; 2];
            for d in 0..=2 * ENVELOPE {
                let r = raw[(j + n + d - ENVELOPE) % n];
                m = [m[0].min(r[0]), m[1].min(r[1])];
            }
            m
        })
        .collect()
}

/// Rail centre (lateral distance, m) on a side with limit `cap`: the
/// nominal place past the verge, or closer in on the inside of a tight bend
/// (always on the shoulder).
pub fn rail_lat(track: &Track, verge: f32, cap: f32) -> f32 {
    let hw = track.half_width;
    (hw + verge + RAIL_OUT).min(cap - 0.9).max(hw + KERB_W + RAIL_W)
}
