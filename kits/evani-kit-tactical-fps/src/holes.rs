//! Holes in the concrete, ported from Hexagon's `rules.ts` (`indexHoles`,
//! `holed`, `addHole`, `grows`, `blastHoles`).
//!
//! Every level keeps a list of hole spheres in its own frame (ring-local on
//! the middle level), indexed in a coarse grid of buckets. A hole removes
//! all concrete inside its sphere. The list is the single source of truth:
//! [`crate::arena::Arena::probe`] asks [`Holes::holed`], and the drawing
//! shows the same spheres.
//!
//! Difference from Hexagon: its `makeRoom` (recycling holes that lie inside
//! others, joining pairs, then dropping the oldest) is replaced by dropping
//! the oldest holes, in the crowded buckets first.

use crate::arena::{Arena, sector_axis};

// The hole list over evani-net (src/net): a child module, so a read can
// rebuild the private bucket index.
#[cfg(feature = "net")]
mod net;

/// Bucket size (m).
pub const HB: f32 = 2.0;
/// Buckets per side per level (centred on the axis).
pub const HN: usize = 46;
/// The biggest hole radius.
pub const HOLE_RMAX: f32 = 4.0;

/// Bucket index of a level-frame coordinate.
pub fn hbi(v: f32) -> usize {
    ((v / HB + (HN / 2) as f32).floor()).clamp(0.0, (HN - 1) as f32) as usize
}

fn q2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn q3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

/// One hole sphere in its level's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    /// Level.
    pub level: usize,
    /// Centre (level frame).
    pub p: [f32; 3],
    /// Radius.
    pub r: f32,
    /// Seed (for the drawing's rim variation).
    pub seed: u32,
}

/// The hole list and its bucket index.
#[derive(Debug, Clone, Default)]
pub struct Holes {
    /// Holes, oldest first.
    pub list: Vec<Hole>,
    buckets: Vec<Vec<u32>>,
    /// Bumped on every change (the collapse check and the drawing watch it).
    pub version: u64,
}

impl Holes {
    /// No holes.
    pub fn new() -> Self {
        Holes { list: Vec::new(), buckets: vec![Vec::new(); 3 * HN * HN], version: 0 }
    }

    /// Remove every hole.
    pub fn clear(&mut self) {
        self.list.clear();
        self.buckets.iter_mut().for_each(Vec::clear);
        self.version += 1;
    }

    fn bucket(&self, level: usize, a: usize, b: usize) -> &[u32] {
        self.buckets.get((level * HN + a) * HN + b).map_or(&[], Vec::as_slice)
    }

    /// How many holes bucket `(a, b)` of `level` lists.
    pub fn bucket_len(&self, level: usize, a: usize, b: usize) -> usize {
        self.bucket(level, a, b).len()
    }

    fn bucket_range(x: f32, z: f32, r: f32) -> (usize, usize, usize, usize) {
        (hbi(x - r), hbi(x + r), hbi(z - r), hbi(z + r))
    }

    /// Rebuild the bucket index from the hole list.
    fn reindex(&mut self) {
        self.buckets.iter_mut().for_each(Vec::clear);
        for (i, h) in self.list.iter().enumerate() {
            let (i0, i1, k0, k1) = Self::bucket_range(h.p[0], h.p[2], h.r);
            for a in i0..=i1 {
                for b in k0..=k1 {
                    if let Some(l) = self.buckets.get_mut((h.level * HN + a) * HN + b) {
                        l.push(i as u32);
                    }
                }
            }
        }
    }

    /// Is a point (in `level`'s frame) inside one of its holes? The one hole test everything uses.
    pub fn holed(&self, level: usize, x: f32, y: f32, z: f32) -> bool {
        self.bucket(level, hbi(x), hbi(z)).iter().any(|&i| {
            self.list.get(i as usize).is_some_and(|h| {
                let (dx, dy, dz) = (x - h.p[0], y - h.p[1], z - h.p[2]);
                dx * dx + dy * dy + dz * dz < h.r * h.r
            })
        })
    }

    /// Can hole `j` grow to centre `(x, z)` and radius `r` without a bucket
    /// it newly reaches going over `cap`?
    fn grows(&self, level: usize, j: usize, x: f32, z: f32, r: f32, cap: usize) -> bool {
        let Some(h) = self.list.get(j) else {
            return false;
        };
        let (o0, o1, p0, p1) = Self::bucket_range(h.p[0], h.p[2], h.r);
        let (i0, i1, k0, k1) = Self::bucket_range(x, z, r);
        for a in i0..=i1 {
            for b in k0..=k1 {
                if (o0..=o1).contains(&a) && (p0..=p1).contains(&b) {
                    continue;
                }
                if self.bucket(level, a, b).len() >= cap {
                    return false;
                }
            }
        }
        true
    }

    /// Drop the `n` oldest holes (those listed in `only` buckets first, when given).
    fn drop_oldest(&mut self, n: usize, only: Option<&[usize]>) {
        let mut kill = vec![false; self.list.len()];
        if let Some(bks) = only {
            let mut ids: Vec<u32> = bks.iter().filter_map(|&bk| self.buckets.get(bk)).flatten().copied().collect();
            ids.sort_unstable();
            ids.dedup();
            for i in ids.into_iter().take(n) {
                if let Some(k) = kill.get_mut(i as usize) {
                    *k = true;
                }
            }
        } else {
            kill.iter_mut().take(n).for_each(|k| *k = true);
        }
        let mut i = 0;
        self.list.retain(|_| {
            let keep = !kill.get(i).copied().unwrap_or(false);
            i += 1;
            keep
        });
        self.reindex();
        self.version += 1;
    }

    /// Add a hole sphere to `level` (coordinates in its frame, quantised so
    /// every side tests the same numbers). A hole inside an existing one adds
    /// nothing (false). When a bucket it reaches already lists `cap` holes,
    /// the new hole is merged into the nearby hole whose enclosing sphere is
    /// smallest, so damage only ever grows. Over `max` holes the oldest go.
    #[allow(clippy::too_many_arguments)]
    pub fn add(&mut self, level: usize, x: f32, y: f32, z: f32, r: f32, seed: u32, cap: usize, max: usize) -> bool {
        let (x, y, z, r) = (q2(x), q2(y), q2(z), q3(r.min(HOLE_RMAX)));
        let (i0, i1, k0, k1) = Self::bucket_range(x, z, r);
        let cap = cap.max(1);
        let max = max.max(16);
        for tries in 0.. {
            if self.list.len() >= max && tries < 3 {
                // The list is full: make room first, so this hit lands as itself.
                self.drop_oldest((max / 16).max(8), None);
                continue;
            }
            let mut best: Option<(usize, [f32; 3], f32)> = None;
            let mut full = self.list.len() >= max;
            let mut crowded: Vec<usize> = Vec::new();
            for a in i0..=i1 {
                for b in k0..=k1 {
                    let bk = (level * HN + a) * HN + b;
                    let ids = self.bucket(level, a, b);
                    if ids.len() >= cap {
                        full = true;
                        crowded.push(bk);
                    }
                    for &i in ids {
                        let Some(h) = self.list.get(i as usize) else { continue };
                        let (dx, dy, dz) = (x - h.p[0], y - h.p[1], z - h.p[2]);
                        let d = (dx * dx + dy * dy + dz * dz).sqrt();
                        if d + r <= h.r {
                            return false;
                        }
                        let (mut c, mut big_r) = ([x, y, z], r);
                        if d + h.r > r {
                            big_r = (d + r + h.r) / 2.0;
                            let f = (big_r - h.r) / d.max(1e-6);
                            c = [h.p[0] + dx * f, h.p[1] + dy * f, h.p[2] + dz * f];
                        }
                        let better = best.is_none_or(|(_, _, br)| big_r < br);
                        if better && big_r <= HOLE_RMAX && self.grows(level, i as usize, c[0], c[2], big_r + 0.02, cap) {
                            best = Some((i as usize, c, big_r));
                        }
                    }
                }
            }
            if !full || tries >= 6 {
                // After a few rounds of making room the hit is stored regardless: damage is never dropped.
                self.list.push(Hole { level, p: [x, y, z], r, seed });
                let id = (self.list.len() - 1) as u32;
                for a in i0..=i1 {
                    for b in k0..=k1 {
                        if let Some(l) = self.buckets.get_mut((level * HN + a) * HN + b) {
                            l.push(id);
                        }
                    }
                }
                self.version += 1;
                return true;
            }
            if let (Some((j, c, br)), 0) = (best, tries) {
                // A crowded spot: the hit grows the hole nearest it. Rounding the
                // merged centre may move it by up to about 9 mm: pad the radius.
                if let Some(h) = self.list.get_mut(j) {
                    *h = Hole { level, p: [q2(c[0]), q2(c[1]), q2(c[2])], r: ((br + 0.01) * 1000.0).ceil() / 1000.0, seed };
                }
                self.reindex();
                self.version += 1;
                return true;
            }
            // Nothing to merge into: free a slot in the crowded buckets and try again.
            self.drop_oldest(1, (!crowded.is_empty()).then_some(crowded.as_slice()));
        }
        false
    }
}

impl Arena {
    /// Concrete height range of `level` (slab underside to the top of the tallest cover).
    pub fn conc_lo(&self, level: usize) -> f32 {
        self.def.fy[level] - self.def.slab
    }

    /// Top of the tallest cover on `level`.
    pub fn conc_hi(&self, level: usize) -> f32 {
        self.def.fy[level] + 3.0
    }

    /// A blast sphere at world point `c`: a hole in every level whose concrete
    /// it reaches. Metal is never touched. Returns how many concrete pieces it reached.
    pub fn blast_holes(&mut self, c: [f32; 3], r: f32, seed: u32, cap: usize, max: usize) -> usize {
        let [x, y, z] = c;
        let mut n = 0;
        for level in 0..3 {
            if y + r <= self.conc_lo(level) || y - r >= self.conc_hi(level) {
                continue;
            }
            let (mx, mz) = self.to_level(level, x, z);
            let m = self
                .pieces
                .iter()
                .filter(|p| p.level == level)
                .filter(|p| {
                    let (ck, sk) = sector_axis(p.sector);
                    let (u, v) = (mx * ck + mz * sk, -mx * sk + mz * ck);
                    let b = &p.bounds;
                    let du = (b[0] - u).max(0.0).max(u - b[1]);
                    let dv = (b[2] - v).max(0.0).max(v - b[3]);
                    let dy = (b[4] - y).max(0.0).max(y - b[5]);
                    du * du + dv * dv + dy * dy < r * r
                })
                .count();
            if m > 0 && self.holes.add(level, mx, y, mz, r, seed, cap, max) {
                n += m;
            }
        }
        n
    }

    /// A hitscan hole where a shot met concrete at world point `p`, pushed
    /// `depth` into the surface along the shot direction `d`.
    #[allow(clippy::too_many_arguments)]
    pub fn shot_hole(&mut self, p: [f32; 3], d: [f32; 3], r: f32, depth: f32, seed: u32, cap: usize, max: usize) -> bool {
        let c = [p[0] + d[0] * depth, p[1] + d[1] * depth, p[2] + d[2] * depth];
        let level = self.def.level_of(c[1]);
        let (mx, mz) = self.to_level(level, c[0], c[2]);
        self.holes.add(level, mx, c[1], mz, r, seed, cap, max)
    }
}
