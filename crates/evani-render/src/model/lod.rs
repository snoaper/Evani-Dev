//! Level of detail inside one tier file (feature `models`, S3-4).
//!
//! A tier file may hold several levels (`extras.evani.lods`, most detailed
//! first, each with `min_screen`: the smallest projected height, as a
//! fraction of the screen height, at which the level is still used). Each
//! frame every drawn instance gets the level for its projected size, with
//! hysteresis: a level changes only once the size is clearly past the
//! threshold ([`BAND`] either side), so an instance hovering at a
//! threshold never flickers between two levels.
//!
//! Render only: the level never reaches the simulation.

use evani_core::camera::CameraPose;

/// Hysteresis either side of a threshold (fraction of it): finer only
/// above `min * (1 + BAND)`, coarser only below `min * (1 - BAND)`.
pub const BAND: f32 = 0.15;

/// Projected height of a sphere of `radius` whose centre is `dist` away,
/// as a fraction of the screen height, through a vertical field of view of
/// `fov_y_deg`. Inside the sphere it is at least 1 (fills the screen).
pub fn screen_fraction(radius: f32, dist: f32, fov_y_deg: f32) -> f32 {
    let t = (fov_y_deg.to_radians() * 0.5).tan().max(1e-4);
    radius / (dist.max(radius).max(1e-4) * t)
}

/// The level for projected size `s`, given last frame's level `prev` and
/// the levels' `min_screen` thresholds (most detailed first; an empty list
/// is one level).
pub fn choose(prev: Option<u8>, s: f32, mins: &[f32]) -> u8 {
    let n = mins.len();
    if n < 2 {
        return 0;
    }
    let raw = mins.iter().position(|&m| s >= m).unwrap_or(n - 1);
    let mut k = prev.map_or(raw, |p| usize::from(p).min(n - 1));
    if prev.is_some() {
        while k > 0 && s >= mins[k - 1] * (1.0 + BAND) {
            k -= 1;
        }
        while k + 1 < n && s < mins[k] * (1.0 - BAND) {
            k += 1;
        }
    }
    k as u8
}

/// The largest projected size of a sphere (`centre`, `radius`, world
/// space) over the cameras (one per view).
pub fn largest_fraction(centre: [f32; 3], radius: f32, cameras: &[CameraPose]) -> f32 {
    let mut best = 0.0f32;
    for c in cameras {
        let d = [centre[0] - c.eye[0], centre[1] - c.eye[1], centre[2] - c.eye[2]];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        best = best.max(screen_fraction(radius, dist, c.fov_y_deg));
    }
    best
}

/// `q` (x, y, z, w) applied to `v`.
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // v + 2w (u x v) + 2 u x (u x v), u = (x, y, z).
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINS: [f32; 3] = [0.3, 0.1, 0.0];

    /// An instance hovering a few percent either side of a threshold keeps
    /// its level; one that clearly crosses it changes once, and only once.
    #[test]
    fn lod_hysteresis_does_not_flicker() {
        for &t in &MINS[..2] {
            for start in [t * 1.3, t * 0.7] {
                let mut lvl = choose(None, start, &MINS);
                let first = lvl;
                for i in 0..600 {
                    // +-10 % around the threshold: inside the band.
                    let s = t * (1.0 + 0.1 * ((i as f32) * 0.37).sin());
                    lvl = choose(Some(lvl), s, &MINS);
                    assert_eq!(lvl, first, "flicker at threshold {t}, frame {i}, size {s}");
                }
            }
        }
        // A camera flying away and back: each level is entered once each way.
        let mut lvl = choose(None, 1.0, &MINS);
        let mut changes = 0;
        let mut seen = vec![lvl];
        for i in 0..2000 {
            let x = i as f32 / 1000.0; // 0..2
            let s = if x < 1.0 { 1.0 - 0.99 * x } else { 0.01 + 0.99 * (x - 1.0) };
            // A little noise on top: never enough to flip a level back.
            let s = s * (1.0 + 0.05 * ((i as f32) * 1.7).sin());
            let next = choose(Some(lvl), s, &MINS);
            if next != lvl {
                changes += 1;
                seen.push(next);
            }
            lvl = next;
        }
        assert_eq!(seen, [0, 1, 2, 1, 0], "levels went {seen:?}");
        assert_eq!(changes, 4);
    }

    #[test]
    fn one_level_files_always_draw_level_zero() {
        assert_eq!(choose(None, 0.001, &[]), 0);
        assert_eq!(choose(Some(3), 0.5, &[0.0]), 0);
    }

    #[test]
    fn first_choice_has_no_hysteresis_and_sizes_shrink_with_distance() {
        assert_eq!(choose(None, 0.31, &MINS), 0);
        assert_eq!(choose(None, 0.29, &MINS), 1);
        assert_eq!(choose(None, 0.05, &MINS), 2);
        let near = screen_fraction(1.0, 5.0, 50.0);
        let far = screen_fraction(1.0, 50.0, 50.0);
        assert!(near > far * 9.0 && near < far * 11.0, "{near} {far}");
        assert!(screen_fraction(1.0, 0.1, 50.0) >= 1.0);
        let r = rotate([0.0, (0.25f32 * core::f32::consts::PI).sin(), 0.0, (0.25f32 * core::f32::consts::PI).cos()], [1.0, 0.0, 0.0]);
        assert!((r[0]).abs() < 1e-5 && (r[2] + 1.0).abs() < 1e-5, "{r:?}");
    }
}
