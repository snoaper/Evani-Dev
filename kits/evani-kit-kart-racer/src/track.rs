//! The track: a closed Catmull-Rom centreline through the control points in
//! `track.json`, resampled at an even spacing (about 2 m) with tangent, left
//! normal, height, distance and signed curvature per sample; the AI's racing
//! line; checkpoint gates; and the scenery layout (trees, mountains).
//!
//! Conventions: y is up; a heading `yaw` points along `(sin yaw, 0, cos yaw)`;
//! "left" of a forward vector `f` is `(f.z, 0, -f.x)`; lateral offsets are
//! positive to the left; positive curvature turns left.

use serde::Deserialize;

/// `track.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct TrackJson {
    pub name: String,
    pub half_width: f32,
    pub sample_spacing: f32,
    pub points: Vec<[f32; 3]>,
    /// Grid rows start this far behind the line (m).
    pub start_offset: f32,
    /// Item box rows as fractions of the lap.
    pub item_rows: Vec<f32>,
    pub item_boxes_per_row: u32,
    pub scenery: SceneryJson,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneryJson {
    pub seed: u64,
    pub trees: u32,
    pub tree_min_distance: f32,
    pub ground_size: f32,
    pub mountains: Vec<MountainJson>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MountainJson {
    pub pos: [f32; 3],
    pub size: [f32; 3],
    pub material: String,
}

/// One resampled point of the centreline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub pos: [f32; 3],
    /// Horizontal unit tangent `[x, z]`.
    pub tangent: [f32; 2],
    /// Distance from the start line along the centreline.
    pub s: f32,
    /// Signed curvature (1/m, positive turns left), lightly smoothed.
    pub curvature: f32,
    /// Height change per metre along the track.
    pub slope: f32,
}

impl Sample {
    pub fn left(&self) -> [f32; 2] {
        [self.tangent[1], -self.tangent[0]]
    }

    pub fn yaw(&self) -> f32 {
        self.tangent[0].atan2(self.tangent[1])
    }
}

/// Where a point is relative to the track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// Sample index at or just before the point.
    pub idx: usize,
    /// Distance along the track, `0..length`.
    pub s: f32,
    /// Signed distance from the centreline (left positive).
    pub lateral: f32,
    /// Road height under the point.
    pub height: f32,
    pub tangent: [f32; 2],
}

/// A tree of the scenery.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tree {
    pub pos: [f32; 3],
    pub scale: f32,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub name: String,
    pub half_width: f32,
    pub samples: Vec<Sample>,
    pub length: f32,
    pub spacing: f32,
    pub start_offset: f32,
    pub item_rows: Vec<f32>,
    pub item_boxes_per_row: u32,
    pub scenery: SceneryJson,
    pub trees: Vec<Tree>,
    /// Racing line: lateral offset per sample, for `line_inset`.
    pub line: Vec<f32>,
    pub line_inset: f32,
    /// Horizontal bounds `[min_x, min_z, max_x, max_z]` of the centreline.
    pub bounds: [f32; 4],
    /// Height of the flat meadow round the track (m): at most -0.3, and
    /// [`MEADOW_BELOW_ROAD`] under the lowest road, so the road's grass bank
    /// (its top 6 cm under the road) never lies level with the meadow,
    /// where the two would z-fight.
    pub meadow_y: f32,
    /// How far out each side may reach, per ring of the track meshes
    /// (`[left, right]`, m; see [`crate::trackgeo::limits`]): the rail
    /// comes in on the inside of the hairpins, and so does the wall.
    pub side_caps: Vec<[f32; 2]>,
}

/// The meadow stays at least this far (m) under the lowest point of road.
/// The bank top then stands at least 7 cm over the meadow: depth precision
/// keeps the two apart to about 170 m.
pub const MEADOW_BELOW_ROAD: f32 = 0.13;

fn catmull(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3], t: f32) -> [f32; 3] {
    let t2 = t * t;
    let t3 = t2 * t;
    let mut o = [0.0; 3];
    for i in 0..3 {
        o[i] = 0.5
            * (2.0 * p1[i]
                + (-p0[i] + p2[i]) * t
                + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * t2
                + (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * t3);
    }
    o
}

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Small deterministic hash to `0..1`.
pub fn hash01(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

impl Track {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let j: TrackJson = serde_json::from_str(json).map_err(|e| format!("track.json: {e}"))?;
        Self::build(j)
    }

    pub fn build(j: TrackJson) -> Result<Self, String> {
        let n = j.points.len();
        if n < 4 {
            return Err("track.json: need at least 4 control points".into());
        }
        if !(j.half_width > 0.5 && j.sample_spacing >= 0.5) {
            return Err("track.json: bad half_width or sample_spacing".into());
        }
        // Dense polyline through the closed spline.
        const SUB: usize = 48;
        let mut dense: Vec<[f32; 3]> = Vec::with_capacity(n * SUB + 1);
        for i in 0..n {
            let p = |k: usize| j.points[(i + k + n - 1) % n];
            for k in 0..SUB {
                dense.push(catmull(p(0), p(1), p(2), p(3), k as f32 / SUB as f32));
            }
        }
        dense.push(dense[0]);
        let mut cum = Vec::with_capacity(dense.len());
        let mut total = 0.0f32;
        cum.push(0.0);
        for w in dense.windows(2) {
            total += dist_xz(w[0], w[1]);
            cum.push(total);
        }
        if total < 50.0 {
            return Err("track.json: track shorter than 50 m".into());
        }
        let count = (total / j.sample_spacing).round().max(16.0) as usize;
        let spacing = total / count as f32;
        let mut pos = Vec::with_capacity(count);
        let mut k = 0usize;
        for i in 0..count {
            let s = i as f32 * spacing;
            while k + 1 < cum.len() - 1 && cum[k + 1] < s {
                k += 1;
            }
            let seg = (cum[k + 1] - cum[k]).max(1e-6);
            let t = ((s - cum[k]) / seg).clamp(0.0, 1.0);
            let (a, b) = (dense[k], dense[k + 1]);
            pos.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
        let mut samples: Vec<Sample> = Vec::with_capacity(count);
        for i in 0..count {
            let prev = pos[(i + count - 1) % count];
            let next = pos[(i + 1) % count];
            let (dx, dz) = (next[0] - prev[0], next[2] - prev[2]);
            let l = (dx * dx + dz * dz).sqrt().max(1e-6);
            samples.push(Sample {
                pos: pos[i],
                tangent: [dx / l, dz / l],
                s: i as f32 * spacing,
                curvature: 0.0,
                slope: (next[1] - prev[1]) / (2.0 * spacing),
            });
        }
        // Signed curvature from the heading change, smoothed over ~12 m.
        let raw: Vec<f32> = (0..count)
            .map(|i| {
                let a = samples[(i + count - 1) % count].yaw();
                let b = samples[(i + 1) % count].yaw();
                let mut d = b - a;
                while d > core::f32::consts::PI {
                    d -= core::f32::consts::TAU;
                }
                while d < -core::f32::consts::PI {
                    d += core::f32::consts::TAU;
                }
                d / (2.0 * spacing)
            })
            .collect();
        let smooth = box_smooth(&raw, 3);
        for (s, c) in samples.iter_mut().zip(smooth) {
            s.curvature = c;
        }
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let low = samples.iter().map(|s| s.pos[1]).fold(f32::MAX, f32::min);
        let meadow_y = (-0.3f32).min(low - MEADOW_BELOW_ROAD);
        for s in &samples {
            b[0] = b[0].min(s.pos[0]);
            b[1] = b[1].min(s.pos[2]);
            b[2] = b[2].max(s.pos[0]);
            b[3] = b[3].max(s.pos[2]);
        }
        let mut track = Track {
            name: j.name,
            half_width: j.half_width,
            samples,
            length: total,
            spacing,
            start_offset: j.start_offset,
            item_rows: j.item_rows,
            item_boxes_per_row: j.item_boxes_per_row,
            scenery: j.scenery,
            trees: Vec::new(),
            line: Vec::new(),
            line_inset: -1.0,
            bounds: b,
            meadow_y,
            side_caps: Vec::new(),
        };
        track.set_line_inset(0.55);
        track.place_trees();
        track.side_caps = crate::trackgeo::limits(&track, &crate::trackgeo::rings(&track));
        Ok(track)
    }

    /// The side limits `[left, right]` (m) at `s`: the tightest of the
    /// nearest ring of the track meshes and its neighbours.
    pub fn side_cap(&self, s: f32) -> [f32; 2] {
        let n = self.side_caps.len();
        if n == 0 {
            return [f32::MAX; 2];
        }
        let j = ((self.wrap_s(s) / self.length * n as f32).round() as usize) % n;
        let mut cap = [f32::MAX; 2];
        for d in [n - 1, 0, 1] {
            let l = self.side_caps[(j + d) % n];
            cap = [cap[0].min(l[0]), cap[1].min(l[1])];
        }
        cap
    }

    /// Rebuild the racing line when `inset` (fraction of the half width)
    /// changed: shift towards the inside of each corner by curvature, then
    /// smooth.
    pub fn set_line_inset(&mut self, inset: f32) {
        if (inset - self.line_inset).abs() < 1e-6 && !self.line.is_empty() {
            return;
        }
        self.line_inset = inset;
        let hw = self.half_width;
        let raw: Vec<f32> = self
            .samples
            .iter()
            .map(|s| (s.curvature / 0.03).clamp(-1.0, 1.0) * inset * hw)
            .collect();
        let once = box_smooth(&raw, 8);
        self.line = box_smooth(&once, 8);
    }

    fn place_trees(&mut self) {
        let sc = &self.scenery;
        let half = sc.ground_size * 0.5 - 8.0;
        let min_d = sc.tree_min_distance.max(self.half_width + 3.0);
        let mut trees = Vec::with_capacity(sc.trees as usize);
        let mut attempt = 0u32;
        while trees.len() < sc.trees as usize && attempt < sc.trees * 40 {
            let salt = sc.seed as u32;
            let x = (hash01(attempt, salt) * 2.0 - 1.0) * half;
            let z = (hash01(attempt, salt ^ 0x5bd1) * 2.0 - 1.0) * half;
            let scale = 0.8 + 0.7 * hash01(attempt, salt ^ 0x1234);
            attempt += 1;
            let p = [x, 0.0, z];
            let near = self.samples.iter().map(|s| dist_xz(s.pos, p)).fold(f32::MAX, f32::min);
            if near < min_d || near > min_d + 70.0 {
                continue;
            }
            trees.push(Tree { pos: p, scale });
        }
        self.trees = trees;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn wrap_idx(&self, i: isize) -> usize {
        i.rem_euclid(self.samples.len() as isize) as usize
    }

    /// `s` into `0..length`.
    pub fn wrap_s(&self, s: f32) -> f32 {
        s.rem_euclid(self.length)
    }

    /// Signed shortest difference `a - b` along the loop, in `-L/2..L/2`.
    pub fn delta_s(&self, a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(self.length);
        if d > self.length * 0.5 { d - self.length } else { d }
    }

    /// Index of the sample at or before distance `s`, and the fraction to the
    /// next one.
    pub fn index_at(&self, s: f32) -> (usize, f32) {
        let s = self.wrap_s(s);
        let f = s / self.spacing;
        let i = (f.floor() as usize).min(self.samples.len() - 1);
        (i, (f - i as f32).clamp(0.0, 1.0))
    }

    /// Interpolated centreline point at `s` plus `lateral` to the left.
    pub fn point_at(&self, s: f32, lateral: f32) -> [f32; 3] {
        let (i, t) = self.index_at(s);
        let a = &self.samples[i];
        let b = &self.samples[(i + 1) % self.samples.len()];
        let la = a.left();
        let lb = b.left();
        let l = [la[0] + (lb[0] - la[0]) * t, la[1] + (lb[1] - la[1]) * t];
        [
            a.pos[0] + (b.pos[0] - a.pos[0]) * t + l[0] * lateral,
            a.pos[1] + (b.pos[1] - a.pos[1]) * t,
            a.pos[2] + (b.pos[2] - a.pos[2]) * t + l[1] * lateral,
        ]
    }

    pub fn tangent_at(&self, s: f32) -> [f32; 2] {
        let (i, t) = self.index_at(s);
        let a = self.samples[i].tangent;
        let b = self.samples[(i + 1) % self.samples.len()].tangent;
        let v = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let l = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-6);
        [v[0] / l, v[1] / l]
    }

    /// Racing-line lateral offset at `s`.
    pub fn line_at(&self, s: f32) -> f32 {
        let (i, t) = self.index_at(s);
        let a = self.line[i];
        let b = self.line[(i + 1) % self.line.len()];
        a + (b - a) * t
    }

    /// Mean curvature over the next `dist` metres from `s`.
    pub fn curvature_ahead(&self, s: f32, dist: f32) -> f32 {
        let (i, _) = self.index_at(s);
        let n = ((dist / self.spacing).ceil() as usize).max(1);
        let mut sum = 0.0;
        for k in 0..n {
            sum += self.samples[(i + k) % self.samples.len()].curvature;
        }
        sum / n as f32
    }

    /// Locate `p` near sample `hint` (searching about ±30 m), falling back to
    /// a full search when the point is far from every nearby sample.
    pub fn locate(&self, p: [f32; 3], hint: usize) -> Located {
        let n = self.samples.len();
        let reach = ((30.0 / self.spacing) as isize).max(4);
        let mut best = (f32::MAX, hint % n);
        for k in -reach..=reach {
            let i = self.wrap_idx(hint as isize + k);
            let d = dist_xz(self.samples[i].pos, p);
            if d < best.0 {
                best = (d, i);
            }
        }
        // Walk downhill from the window's best: the hint may be stale by
        // more than the window (teleports, tests).
        for dir in [-1isize, 1] {
            let mut guard = 0;
            loop {
                let j = self.wrap_idx(best.1 as isize + dir);
                let d = dist_xz(self.samples[j].pos, p);
                if d >= best.0 || guard > n {
                    break;
                }
                best = (d, j);
                guard += 1;
            }
        }
        if best.0 > self.half_width * 4.0 + 20.0 {
            for (i, s) in self.samples.iter().enumerate() {
                let d = dist_xz(s.pos, p);
                if d < best.0 {
                    best = (d, i);
                }
            }
        }
        let i = best.1;
        // Project onto the segment after i, or before it.
        let project = |a: usize| {
            let sa = &self.samples[a];
            let sb = &self.samples[(a + 1) % n];
            let ab = [sb.pos[0] - sa.pos[0], sb.pos[2] - sa.pos[2]];
            let ap = [p[0] - sa.pos[0], p[2] - sa.pos[2]];
            let len2 = (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-6);
            (ap[0] * ab[0] + ap[1] * ab[1]) / len2
        };
        let t_next = project(i);
        let (a, t) = if t_next >= 0.0 {
            (i, t_next.min(1.0))
        } else {
            let prev = (i + n - 1) % n;
            (prev, project(prev).clamp(0.0, 1.0))
        };
        let sa = &self.samples[a];
        let sb = &self.samples[(a + 1) % n];
        let c = [
            sa.pos[0] + (sb.pos[0] - sa.pos[0]) * t,
            sa.pos[1] + (sb.pos[1] - sa.pos[1]) * t,
            sa.pos[2] + (sb.pos[2] - sa.pos[2]) * t,
        ];
        let tan = [
            sa.tangent[0] + (sb.tangent[0] - sa.tangent[0]) * t,
            sa.tangent[1] + (sb.tangent[1] - sa.tangent[1]) * t,
        ];
        let tl = (tan[0] * tan[0] + tan[1] * tan[1]).sqrt().max(1e-6);
        let tangent = [tan[0] / tl, tan[1] / tl];
        let left = [tangent[1], -tangent[0]];
        let lateral = (p[0] - c[0]) * left[0] + (p[2] - c[2]) * left[1];
        Located {
            idx: a,
            s: self.wrap_s(sa.s + t * self.spacing),
            lateral,
            height: c[1],
            tangent,
        }
    }

    /// Distance along the track of checkpoint gate `g` of `count`.
    pub fn gate_s(&self, g: u32, count: u32) -> f32 {
        self.length * (g % count.max(1)) as f32 / count.max(1) as f32
    }
}

/// Circular box filter, radius `r` samples.
fn box_smooth(v: &[f32], r: usize) -> Vec<f32> {
    let n = v.len();
    let w = (2 * r + 1) as f32;
    (0..n)
        .map(|i| (0..=2 * r).map(|k| v[(i + n + k - r) % n]).sum::<f32>() / w)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track::from_json(include_str!("../data/track.json")).unwrap()
    }

    #[test]
    fn samples_are_even_and_closed() {
        let t = track();
        assert!(t.length > 500.0 && t.length < 900.0, "{}", t.length);
        assert!((t.spacing - 2.0).abs() < 0.2);
        for w in t.samples.windows(2) {
            let d = dist_xz(w[0].pos, w[1].pos);
            assert!((d - t.spacing).abs() < 0.3, "{d}");
        }
        let last = t.samples.last().unwrap().pos;
        assert!(dist_xz(last, t.samples[0].pos) < t.spacing * 1.2);
        // The loop turns once (left or right) in total.
        let total: f32 = t.samples.iter().map(|s| s.curvature * t.spacing).sum();
        assert!((total.abs() - core::f32::consts::TAU).abs() < 0.2, "{total}");
    }

    #[test]
    fn meadow_lies_clearly_under_every_bank_top() {
        // The bank top is 6 cm under the road; the meadow must stay well
        // clear of it (geocheck found the two level where the road is low).
        let t = track();
        assert!(t.meadow_y <= -0.3);
        for s in &t.samples {
            let gap = (s.pos[1] - 0.06) - t.meadow_y;
            assert!(gap >= MEADOW_BELOW_ROAD - 0.06 - 1e-4, "bank top {gap} m over the meadow at s {}", s.s);
        }
    }

    #[test]
    fn track_never_touches_itself() {
        let t = track();
        let clear = 2.0 * t.half_width + 6.0;
        for (i, a) in t.samples.iter().enumerate() {
            for b in t.samples.iter().skip(i + 1) {
                if t.delta_s(a.s, b.s).abs() > 80.0 {
                    let d = dist_xz(a.pos, b.pos);
                    assert!(d > clear, "samples at {} and {} are {d} m apart", a.s, b.s);
                }
            }
        }
    }

    #[test]
    fn locate_round_trips() {
        let t = track();
        for k in 0..40 {
            let s = k as f32 * t.length / 40.0 + 0.7;
            let lat = (k as f32 * 0.37).sin() * 5.0;
            let p = t.point_at(s, lat);
            let l = t.locate(p, 0);
            assert!(t.delta_s(l.s, s).abs() < 0.6, "s {s} -> {}", l.s);
            assert!((l.lateral - lat).abs() < 0.6, "lat {lat} -> {}", l.lateral);
        }
    }

    #[test]
    fn racing_line_cuts_corners_inside() {
        let t = track();
        let max = t.line.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(max > 1.0 && max <= t.half_width * 0.56, "{max}");
        // Where the track bends hard left the line sits left of centre.
        let (i, s) = t
            .samples
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.curvature.abs().total_cmp(&b.1.curvature.abs()))
            .unwrap();
        assert_eq!(t.line[i].signum(), s.curvature.signum());
    }

    #[test]
    fn trees_stay_off_the_road() {
        let t = track();
        assert!(t.trees.len() > 40, "{}", t.trees.len());
        for tr in &t.trees {
            let l = t.locate(tr.pos, 0);
            assert!(l.lateral.abs() > t.half_width + 2.0);
        }
    }
}
