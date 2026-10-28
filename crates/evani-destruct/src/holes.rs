//! [`HoleField`]: the damage model. Every hit cuts a hole sphere into the
//! breakable material; a hole removes everything inside it, across cells and
//! floor. The hole list is the single source of truth: collision, support,
//! debris and drawing all ask the same [`HoleField::holed`].
//!
//! Holes are indexed in a square grid of buckets over the XZ plane (Hexagon:
//! 2 m buckets), so `holed` scans one bucket's short list. Positions are
//! quantised (centimetres, radius in millimetres) as Hexagon does, so every
//! machine tests the same numbers.
//!
//! Damage memory is capped (per bucket and in total) but a hit is never
//! dropped: a hit inside a hole adds nothing; in a crowded bucket the hit
//! grows the nearby hole whose enclosing sphere is smallest; when full the
//! field recycles: holes lying inside another go first, then neighbouring
//! pairs are joined into one enclosing hole (least new volume first), and
//! only when no pair can join do the oldest go.

use core::f32::consts::PI;

/// One hole sphere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub r: f32,
    /// 0..63: picks the look of the break (debris spin, crater variation).
    pub seed: u8,
}

/// Recycling counters: rounds, holes dropped for lying inside another, pairs
/// joined, oldest dropped, hits stored over the cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recycled {
    pub rounds: u32,
    pub inside: u32,
    pub joined: u32,
    pub oldest: u32,
    pub forced: u32,
}

/// The hole list and its bucket index.
#[derive(Debug, Clone)]
pub struct HoleField {
    holes: Vec<Hole>,
    buckets: Vec<Vec<u32>>,
    cells: usize,
    cell_size: f32,
    centre: [f32; 2],
    per_cell_max: usize,
    max_holes: usize,
    max_radius: f32,
    version: u64,
    /// What recycling has done so far.
    pub recycled: Recycled,
}

fn q2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn q3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

/// A merged radius, padded so rounding its centre still covers both holes.
fn pad(r: f32) -> f32 {
    ((r + 0.01) * 1000.0).ceil() / 1000.0
}

fn sphere_v(r: f32) -> f32 {
    4.0 / 3.0 * PI * r * r * r
}

/// Volume of the union of two spheres of radii `a`, `b` whose centres are `d` apart.
fn union_v(a: f32, b: f32, d: f32) -> f32 {
    if d >= a + b {
        return sphere_v(a) + sphere_v(b);
    }
    if d + a.min(b) <= a.max(b) {
        return sphere_v(a.max(b));
    }
    let s = a + b - d;
    sphere_v(a) + sphere_v(b)
        - PI * s * s * (d * d + 2.0 * d * (a + b) - 3.0 * (a - b) * (a - b)) / (12.0 * d)
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

impl Hole {
    fn centre(&self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

/// A join candidate in [`HoleField::make_room`].
#[derive(Debug, Clone, Copy)]
struct Cand {
    extra: f32,
    i: usize,
    j: usize,
    c: [f32; 3],
    r: f32,
    contained: bool,
}

impl HoleField {
    /// A field of `cells x cells` buckets of `cell_size` metres centred on
    /// `centre` (x, z). Points outside use the edge buckets.
    pub fn new(
        centre: [f32; 2],
        cells: usize,
        cell_size: f32,
        per_cell_max: usize,
        max_holes: usize,
        max_radius: f32,
    ) -> Self {
        let cells = cells.max(1);
        Self {
            holes: Vec::new(),
            buckets: vec![Vec::new(); cells * cells],
            cells,
            cell_size: if cell_size > 0.0 { cell_size } else { 2.0 },
            centre,
            per_cell_max: per_cell_max.max(2),
            max_holes: max_holes.max(4),
            max_radius: max_radius.clamp(0.01, 4.095),
            version: 0,
            recycled: Recycled::default(),
        }
    }

    /// Every hole, oldest first.
    pub fn holes(&self) -> &[Hole] {
        &self.holes
    }

    pub fn len(&self) -> usize {
        self.holes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.holes.is_empty()
    }

    /// Bumped on every change (support is re-checked when it moves).
    pub fn version(&self) -> u64 {
        self.version
    }

    fn ci(&self, v: f32) -> usize {
        let i = (v / self.cell_size + self.cells as f32 * 0.5).floor();
        if i.is_nan() || i < 0.0 {
            0
        } else {
            (i as usize).min(self.cells - 1)
        }
    }

    fn bx(&self, x: f32) -> usize {
        self.ci(x - self.centre[0])
    }

    fn bz(&self, z: f32) -> usize {
        self.ci(z - self.centre[1])
    }

    fn bucket(&self, a: usize, b: usize) -> usize {
        a * self.cells + b
    }

    fn home(&self, h: &Hole) -> usize {
        self.bucket(self.bx(h.x), self.bz(h.z))
    }

    /// Is `p` inside a hole?
    pub fn holed(&self, p: [f32; 3]) -> bool {
        let Some(list) = self.buckets.get(self.bucket(self.bx(p[0]), self.bz(p[2]))) else {
            return false;
        };
        list.iter().any(|&i| {
            self.holes.get(i as usize).is_some_and(|h| {
                let (dx, dy, dz) = (p[0] - h.x, p[1] - h.y, p[2] - h.z);
                dx * dx + dy * dy + dz * dz < h.r * h.r
            })
        })
    }

    /// Remove every hole.
    pub fn clear(&mut self) {
        self.holes.clear();
        for b in &mut self.buckets {
            b.clear();
        }
        self.version += 1;
    }

    /// Replace the list (e.g. from the network) and rebuild the index.
    pub fn load(&mut self, holes: &[Hole]) {
        self.holes.clear();
        self.holes.extend_from_slice(holes);
        self.index();
        self.version += 1;
    }

    fn index(&mut self) {
        for b in &mut self.buckets {
            b.clear();
        }
        for i in 0..self.holes.len() {
            let h = self.holes[i];
            let (a0, a1, b0, b1) = (
                self.bx(h.x - h.r),
                self.bx(h.x + h.r),
                self.bz(h.z - h.r),
                self.bz(h.z + h.r),
            );
            for a in a0..=a1 {
                for b in b0..=b1 {
                    let k = self.bucket(a, b);
                    self.buckets[k].push(i as u32);
                }
            }
        }
    }

    /// Can hole `j` grow to centre `(x, z)`, radius `r` without a bucket it
    /// newly reaches going over the per-bucket cap?
    fn grows(&self, j: usize, x: f32, z: f32, r: f32) -> bool {
        let h = self.holes[j];
        let (o0, o1, p0, p1) = (
            self.bx(h.x - h.r),
            self.bx(h.x + h.r),
            self.bz(h.z - h.r),
            self.bz(h.z + h.r),
        );
        for a in self.bx(x - r)..=self.bx(x + r) {
            for b in self.bz(z - r)..=self.bz(z + r) {
                if a >= o0 && a <= o1 && b >= p0 && b <= p1 {
                    continue;
                }
                if self.buckets[self.bucket(a, b)].len() >= self.per_cell_max {
                    return false;
                }
            }
        }
        true
    }

    /// Cut a hole sphere of radius `r` centred on `p`. Returns false when it
    /// adds nothing (inside an existing hole, or a bad radius). When a bucket
    /// it reaches is full, the hit grows the nearest hole or the field
    /// recycles, so damage only ever grows.
    pub fn add_hole(&mut self, p: [f32; 3], r: f32, seed: u8) -> bool {
        if r.is_nan() || r <= 0.0 || !p.iter().all(|v| v.is_finite()) {
            return false;
        }
        let (x, y, z) = (q2(p[0]), q2(p[1]), q2(p[2]));
        let r = q3(r.min(self.max_radius));
        let seed = seed & 63;
        let rmax = self.max_radius;
        let (i0, i1, k0, k1) = (
            self.bx(x - r),
            self.bx(x + r),
            self.bz(z - r),
            self.bz(z + r),
        );
        let mut crowded: Vec<usize> = Vec::new();
        for tries in 0..=6u32 {
            let list_full = self.holes.len() >= self.max_holes;
            if list_full && tries < 3 {
                self.make_room(None);
                continue;
            }
            let mut best: Option<(usize, [f32; 3])> = None;
            let mut br = f32::MAX;
            let mut full = list_full;
            crowded.clear();
            for a in i0..=i1 {
                for b in k0..=k1 {
                    let bk = self.bucket(a, b);
                    let list = &self.buckets[bk];
                    if list.len() >= self.per_cell_max {
                        full = true;
                        if !crowded.contains(&bk) {
                            crowded.push(bk);
                        }
                    }
                    for &i in list {
                        let h = self.holes[i as usize];
                        let d = dist([x, y, z], h.centre());
                        if d + r <= h.r {
                            return false;
                        }
                        let (mut c, mut rr) = ([x, y, z], r);
                        if d + h.r > r {
                            rr = (d + r + h.r) * 0.5;
                            let f = (rr - h.r) / d;
                            c = [
                                h.x + (x - h.x) * f,
                                h.y + (y - h.y) * f,
                                h.z + (z - h.z) * f,
                            ];
                        }
                        if rr < br && rr <= rmax && self.grows(i as usize, c[0], c[2], rr + 0.02) {
                            br = rr;
                            best = Some((i as usize, c));
                        }
                    }
                }
            }
            if !full || tries >= 6 {
                if full {
                    self.recycled.forced += 1;
                }
                self.holes.push(Hole { x, y, z, r, seed });
                let n = (self.holes.len() - 1) as u32;
                for a in i0..=i1 {
                    for b in k0..=k1 {
                        let k = self.bucket(a, b);
                        self.buckets[k].push(n);
                    }
                }
                self.version += 1;
                return true;
            }
            if let Some((i, c)) = best
                && tries == 0
            {
                self.holes[i] = Hole {
                    x: q2(c[0]),
                    y: q2(c[1]),
                    z: q2(c[2]),
                    r: pad(br),
                    seed,
                };
                self.index();
                self.version += 1;
                return true;
            }
            let crowd = core::mem::take(&mut crowded);
            self.make_room(if crowd.is_empty() { None } else { Some(&crowd) });
            crowded = crowd;
        }
        false
    }

    /// Free room when the list (or the `crowded` buckets) is full.
    fn make_room(&mut self, crowded: Option<&[usize]>) {
        self.recycled.rounds += 1;
        let n = self.holes.len();
        let rmax = self.max_radius;
        let room_for = (self.max_holes / 16).max(8);
        let want = if crowded.is_some() {
            (room_for >> 2).max(2)
        } else {
            room_for
        };
        let mut cand: Vec<Cand> = Vec::new();
        for i in 0..n {
            let hi = self.holes[i];
            let bi = self.home(&hi);
            if crowded.is_some_and(|c| !c.contains(&bi)) {
                continue;
            }
            for &j in &self.buckets[bi] {
                let j = j as usize;
                if j == i {
                    continue;
                }
                let hj = self.holes[j];
                if j < i && crowded.is_none() && self.buckets[self.home(&hj)].contains(&(i as u32))
                {
                    continue;
                }
                let d = dist(hi.centre(), hj.centre());
                if d + hj.r <= hi.r {
                    cand.push(Cand {
                        extra: 0.0,
                        i,
                        j,
                        c: hi.centre(),
                        r: hi.r,
                        contained: true,
                    });
                } else if d + hi.r <= hj.r {
                    cand.push(Cand {
                        extra: 0.0,
                        i: j,
                        j: i,
                        c: hj.centre(),
                        r: hj.r,
                        contained: true,
                    });
                } else {
                    let r = (d + hi.r + hj.r) * 0.5;
                    if r > rmax - 0.02 {
                        continue;
                    }
                    let f = (r - hi.r) / d;
                    let c = [
                        hi.x + (hj.x - hi.x) * f,
                        hi.y + (hj.y - hi.y) * f,
                        hi.z + (hj.z - hi.z) * f,
                    ];
                    cand.push(Cand {
                        extra: sphere_v(r) - union_v(hi.r, hj.r, d),
                        i,
                        j,
                        c,
                        r,
                        contained: false,
                    });
                }
            }
        }
        cand.sort_by(|a, b| {
            a.extra
                .total_cmp(&b.extra)
                .then(a.i.cmp(&b.i))
                .then(a.j.cmp(&b.j))
        });
        cand.truncate(want * 3);
        let mut cnt: Vec<i64> = self.buckets.iter().map(|b| b.len() as i64).collect();
        let mut used = vec![false; n];
        let mut dead = vec![false; n];
        let mut merged: Vec<Option<Hole>> = vec![None; n];
        let mut freed = 0usize;
        for c in &cand {
            if freed >= want {
                break;
            }
            let (i, j) = (c.i, c.j);
            if used[i] || used[j] {
                continue;
            }
            let (hi, hj) = (self.holes[i], self.holes[j]);
            if c.contained {
                used[i] = true;
                used[j] = true;
                dead[j] = true;
                freed += 1;
                self.recycled.inside += 1;
                self.bump(&mut cnt, hj.x, hj.z, hj.r, -1);
                continue;
            }
            let (cx, cy, cz, r) = (q2(c.c[0]), q2(c.c[1]), q2(c.c[2]), pad(c.r));
            self.bump(&mut cnt, hi.x, hi.z, hi.r, -1);
            self.bump(&mut cnt, hj.x, hj.z, hj.r, -1);
            self.bump(&mut cnt, cx, cz, r, 1);
            if self.over(&cnt, cx, cz, r) {
                self.bump(&mut cnt, hi.x, hi.z, hi.r, 1);
                self.bump(&mut cnt, hj.x, hj.z, hj.r, 1);
                self.bump(&mut cnt, cx, cz, r, -1);
                continue;
            }
            used[i] = true;
            used[j] = true;
            dead[j] = true;
            freed += 1;
            self.recycled.joined += 1;
            merged[i] = Some(Hole {
                x: cx,
                y: cy,
                z: cz,
                r,
                seed: hi.seed,
            });
        }
        if freed == 0 {
            // No pair to join: the oldest go (of the crowded buckets, if that is what is full).
            let mut old = vec![crowded.is_none(); n];
            if let Some(c) = crowded {
                for &b in c {
                    for &i in &self.buckets[b] {
                        if let Some(o) = old.get_mut(i as usize) {
                            *o = true;
                        }
                    }
                }
            }
            let mut k = 0;
            for i in 0..n {
                if k >= (want >> 2).max(1) {
                    break;
                }
                if old[i] {
                    dead[i] = true;
                    k += 1;
                    self.recycled.oldest += 1;
                }
            }
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if dead[i] {
                continue;
            }
            out.push(merged[i].unwrap_or(self.holes[i]));
        }
        self.holes = out;
        self.index();
        self.version += 1;
    }

    fn bump(&self, cnt: &mut [i64], x: f32, z: f32, r: f32, by: i64) {
        for a in self.bx(x - r)..=self.bx(x + r) {
            for b in self.bz(z - r)..=self.bz(z + r) {
                cnt[self.bucket(a, b)] += by;
            }
        }
    }

    fn over(&self, cnt: &[i64], x: f32, z: f32, r: f32) -> bool {
        for a in self.bx(x - r)..=self.bx(x + r) {
            for b in self.bz(z - r)..=self.bz(z + r) {
                if cnt[self.bucket(a, b)] > self.per_cell_max as i64 {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(cap: usize, max: usize) -> HoleField {
        HoleField::new([0.0, 0.0], 8, 2.0, cap, max, 4.0)
    }

    #[test]
    fn a_hole_removes_what_is_inside_it() {
        let mut f = field(24, 1800);
        assert!(f.add_hole([1.0, 1.0, 1.0], 0.5, 3));
        assert!(f.holed([1.0, 1.2, 1.0]));
        assert!(!f.holed([1.0, 1.6, 1.0]));
        assert!(!f.holed([3.0, 1.0, 1.0]));
        // A hole spanning a bucket edge is found from both sides.
        assert!(f.add_hole([2.0, 0.0, 0.0], 0.4, 0));
        assert!(f.holed([1.8, 0.0, 0.0]) && f.holed([2.2, 0.0, 0.0]));
    }

    #[test]
    fn a_hit_inside_a_hole_adds_nothing() {
        let mut f = field(24, 1800);
        assert!(f.add_hole([0.0, 0.0, 0.0], 1.0, 0));
        let v = f.version();
        assert!(!f.add_hole([0.2, 0.0, 0.0], 0.3, 0));
        assert_eq!(f.len(), 1);
        assert_eq!(f.version(), v);
    }

    #[test]
    fn positions_are_quantised() {
        let mut f = field(24, 1800);
        f.add_hole([0.123_456, 1.0, -0.987_6], 0.123_456_7, 99);
        let h = f.holes()[0];
        assert_eq!((h.x, h.z, h.r, h.seed), (0.12, -0.99, 0.123, 99 & 63));
    }

    #[test]
    fn a_crowded_bucket_grows_the_nearest_hole_and_keeps_the_hit() {
        let mut f = field(4, 1800);
        for k in 0..4 {
            assert!(f.add_hole([0.3 + 0.4 * k as f32, 0.0, 0.5], 0.1, 0));
        }
        assert_eq!(f.len(), 4);
        let hit = [0.5, 0.0, 0.5];
        assert!(f.add_hole(hit, 0.1, 0));
        assert_eq!(f.len(), 4, "merged into a neighbour, not appended");
        assert!(f.holed(hit), "the hit is never lost");
        assert!(
            f.holed([0.3, 0.0, 0.5]),
            "the grown hole still covers the old one"
        );
    }

    #[test]
    fn a_full_field_recycles_and_never_drops_the_newest_hit() {
        let mut f = HoleField::new([0.0, 0.0], 32, 2.0, 24, 64, 4.0);
        let mut rng = evani_core::rng::Pcg32::seeded(7);
        for _ in 0..400 {
            let p = [
                rng.range_f32(-20.0, 20.0),
                rng.range_f32(0.0, 3.0),
                rng.range_f32(-20.0, 20.0),
            ];
            let r = rng.range_f32(0.05, 0.6);
            f.add_hole(p, r, 0);
            assert!(f.holed([q2(p[0]), q2(p[1]), q2(p[2])]), "hit at {p:?} lost");
            assert!(f.len() <= 64 + 8, "{} holes", f.len());
        }
        assert!(f.recycled.rounds > 0);
        assert!(f.recycled.joined + f.recycled.inside + f.recycled.oldest > 0);
    }

    #[test]
    fn contained_holes_go_first() {
        let mut f = field(24, 1800);
        f.add_hole([0.0, 0.0, 0.0], 0.2, 0);
        f.add_hole([0.05, 0.0, 0.0], 1.0, 0); // swallows the first
        assert_eq!(f.len(), 2);
        f.make_room(None);
        assert_eq!(f.len(), 1);
        assert_eq!(f.recycled.inside, 1);
        assert!(f.holed([0.0, 0.0, 0.0]));
    }

    #[test]
    fn union_volume_is_sane() {
        let (a, b) = (1.0, 0.5);
        assert!((union_v(a, b, 10.0) - (sphere_v(a) + sphere_v(b))).abs() < 1e-4);
        assert!((union_v(a, b, 0.2) - sphere_v(a)).abs() < 1e-4);
        let mid = union_v(a, b, 1.0);
        assert!(mid > sphere_v(a) && mid < sphere_v(a) + sphere_v(b));
    }
}
