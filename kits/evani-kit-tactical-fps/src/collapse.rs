//! Concrete never floats. Ported from Hexagon's `rules.ts` (`cellMat`,
//! `nearHole`, `unsupported`, `topple`'s support test, `hull`, `outside`)
//! and the cover half of `runtime.ts`'s `collapse`.
//!
//! Cover cells with at least `cell_min_solid` of their concrete left carry
//! load and join into connected groups (cells with only a little left go
//! with the group that touches them). A group stands while the floor under
//! its bottom cells still holds at least `support_min` of its footprint and
//! its centre of mass lies over the held region (within `tip_margin`).
//! Otherwise its cells are gone at once (they no longer block movement,
//! shots or sight), as in Hexagon's final state.
//!
//! Left out (later pieces): the tipping fall animation (`stepFall`), the
//! landing craters, floor islands that drop away as slabs (`floorDrops`),
//! and the debris.

use crate::arena::{Arena, NV, to_world};
use crate::holes::{HN, hbi};
use crate::tune::Tune;

/// How far under a cell's bottom its support is tested.
const BELOW: f32 = 0.05;
/// Support samples under a cell's bottom face: an `SG x SG` grid.
const SG: usize = 4;
/// The 6 grid neighbours.
const N6: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];
/// The 3 sample fractions per axis inside a cell.
const MS: [f32; 3] = [1.0 / 6.0, 0.5, 5.0 / 6.0];

impl Arena {
    /// Could a hole reach the 1 m square at sector-local `(u0, v0)`?
    fn near_hole(&self, level: usize, k: usize, u0: f32, v0: f32) -> bool {
        let (mut a0, mut a1, mut b0, mut b1) = (HN, 0, HN, 0);
        for i in 0..4 {
            let (u, v) = (u0 + (i & 1) as f32, v0 + (i >> 1) as f32);
            let (x, z) = to_world(k, u, v);
            let (a, b) = (hbi(x), hbi(z));
            a0 = a0.min(a);
            a1 = a1.max(a);
            b0 = b0.min(b);
            b1 = b1.max(b);
        }
        (a0..=a1).any(|a| (b0..=b1).any(|b| self.holes.bucket_len(level, a, b) > 0))
    }

    /// How much of cover cell `c` is still concrete, 0..1 (0 once it has fallen).
    pub fn cell_mat(&self, c: usize) -> f32 {
        let (Some(cell), Some(&gone)) = (self.cells.get(c), self.gone.get(c)) else {
            return 0.0;
        };
        if gone {
            return 0.0;
        }
        let (level, k) = (cell.level, cell.sector);
        let u0 = self.def.a_in + f32::from(cell.iu);
        let v0 = f32::from(cell.iv) - (NV / 2) as f32;
        let y0 = self.def.fy[level] + f32::from(cell.iy);
        if !self.near_hole(level, k, u0, v0) {
            return 1.0;
        }
        let mut n = 0;
        for a in MS {
            for b in MS {
                for c in MS {
                    let (x, z) = to_world(k, u0 + a, v0 + c);
                    if !self.holes.holed(level, x, y0 + b, z) {
                        n += 1;
                    }
                }
            }
        }
        n as f32 / 27.0
    }

    /// The grid neighbours of cell `c`.
    fn neighbours(&self, c: usize) -> impl Iterator<Item = usize> + '_ {
        let cell = self.cells.get(c).copied();
        N6.iter().filter_map(move |&(du, dv, dy)| {
            let cell = cell?;
            let iu = i32::from(cell.iu) + du;
            let iv = i32::from(cell.iv) + dv;
            let iy = i32::from(cell.iy) + dy;
            if iu < 0 || iv < 0 || iy < 0 {
                return None;
            }
            self.cell_at(cell.level, cell.sector, iu as usize, iv as usize, iy as usize)
        })
    }

    /// Cover on `level` that is no longer held up: the cells of every
    /// connected group that falls.
    pub fn unsupported(&self, level: usize, t: &Tune) -> Vec<Vec<usize>> {
        let n = self.cells.len();
        let mut m = vec![0.0_f32; n];
        for (c, cell) in self.cells.iter().enumerate() {
            if cell.level == level {
                m[c] = self.cell_mat(c);
            }
        }
        let mut grp: Vec<Option<usize>> = vec![None; n];
        let mut groups: Vec<Vec<usize>> = Vec::new();
        // Grow groups from seeds that pass `keep` through neighbours that pass `join`.
        let grow = |seed_ok: &dyn Fn(f32) -> bool, join_ok: &dyn Fn(f32) -> bool, grp: &mut Vec<Option<usize>>, groups: &mut Vec<Vec<usize>>| {
            for c in 0..n {
                if !seed_ok(m[c]) || grp[c].is_some() {
                    continue;
                }
                let gi = groups.len();
                grp[c] = Some(gi);
                let mut g = vec![c];
                let mut h = 0;
                while h < g.len() {
                    let at = g[h];
                    for e in self.neighbours(at) {
                        if join_ok(m[e]) && grp[e].is_none() {
                            grp[e] = Some(gi);
                            g.push(e);
                        }
                    }
                    h += 1;
                }
                groups.push(g);
            }
        };
        // Cells that still carry load, in connected groups.
        let strong = |v: f32| v >= t.cell_min_solid;
        grow(&strong, &strong, &mut grp, &mut groups);
        // Cells with only a little concrete left go with the group that reaches them first.
        let mut q: Vec<usize> = groups.iter().flatten().copied().collect();
        let mut h = 0;
        while h < q.len() {
            let at = q[h];
            if let Some(gi) = grp[at] {
                let next: Vec<usize> = self.neighbours(at).filter(|&e| m[e] > 0.0 && grp[e].is_none()).collect();
                for e in next {
                    grp[e] = Some(gi);
                    if let Some(g) = groups.get_mut(gi) {
                        g.push(e);
                    }
                    q.push(e);
                }
            }
            h += 1;
        }
        // The rest form groups of their own.
        let some = |v: f32| v > 0.0;
        grow(&some, &some, &mut grp, &mut groups);
        groups.into_iter().filter(|g| self.falls(level, g, &m, t)).collect()
    }

    /// Does group `g` (on `level`, `m` = concrete left per cell) fall? The support test of Hexagon's `topple`.
    fn falls(&self, level: usize, g: &[usize], m: &[f32], t: &Tune) -> bool {
        let Some(first) = g.iter().min().and_then(|&c| self.cells.get(c)) else {
            return false;
        };
        let k = first.sector;
        let (mut mass, mut cu, mut cv) = (0.0_f32, 0.0_f32, 0.0_f32);
        let mut cols: Vec<usize> = Vec::new();
        for &c in g {
            let Some(cell) = self.cells.get(c) else { continue };
            let wt = m[c];
            mass += wt;
            cu += wt * (self.def.a_in + f32::from(cell.iu) + 0.5);
            cv += wt * (f32::from(cell.iv) - (NV / 2) as f32 + 0.5);
            cols.push(usize::from(cell.iu) * NV + usize::from(cell.iv));
        }
        if mass <= 0.0 {
            return true;
        }
        cu /= mass;
        cv /= mass;
        cols.sort_unstable();
        cols.dedup();
        // Held points of the footprint: the cell's foot is concrete there and the floor under it is solid.
        let mut pts: Vec<[f32; 2]> = Vec::new();
        for &c in g {
            let Some(cell) = self.cells.get(c) else { continue };
            if cell.iy != 0 || m[c] < t.cell_min_solid {
                continue;
            }
            let u0 = self.def.a_in + f32::from(cell.iu);
            let v0 = f32::from(cell.iv) - (NV / 2) as f32;
            for i in 0..SG {
                for j in 0..SG {
                    let u = u0 + (i as f32 + 0.5) / SG as f32;
                    let v = v0 + (j as f32 + 0.5) / SG as f32;
                    let (x, z) = to_world(k, u, v);
                    let fy = self.def.fy[level];
                    if self.holes.holed(level, x, fy + 0.1, z) || !self.probe_level(level, x, fy - BELOW, z).is_solid() {
                        continue;
                    }
                    pts.push([u, v]);
                }
            }
        }
        if pts.is_empty() {
            return true;
        }
        let area = pts.len() as f32 / (SG * SG) as f32;
        let out = outside(&hull(&pts), cu, cv);
        !(area >= t.support_min * cols.len() as f32 && out <= t.tip_margin)
    }

    /// Run the collapse check if the holes changed since `seen`: every
    /// unsupported cover group's cells become gone. Returns how many cells fell.
    pub fn collapse(&mut self, seen: &mut u64, t: &Tune) -> usize {
        if *seen == self.holes.version {
            return 0;
        }
        *seen = self.holes.version;
        let mut fell = 0;
        for level in 0..3 {
            for g in self.unsupported(level, t) {
                for c in g {
                    if let Some(gone) = self.gone.get_mut(c) {
                        *gone = true;
                        fell += 1;
                    }
                }
            }
        }
        fell
    }
}

/// Cross product of `oa` and `ob`.
fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Convex hull, counter-clockwise (monotone chain; collinear points dropped).
fn hull(p: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut s = p.to_vec();
    s.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    if s.len() < 3 {
        return s;
    }
    let mut lo: Vec<[f32; 2]> = Vec::new();
    for &q in &s {
        while lo.len() >= 2 && cross(lo[lo.len() - 2], lo[lo.len() - 1], q) <= 0.0 {
            lo.pop();
        }
        lo.push(q);
    }
    let mut up: Vec<[f32; 2]> = Vec::new();
    for &q in s.iter().rev() {
        while up.len() >= 2 && cross(up[up.len() - 2], up[up.len() - 1], q) <= 0.0 {
            up.pop();
        }
        up.push(q);
    }
    lo.pop();
    up.pop();
    lo.extend(up);
    lo
}

/// How far `(x, y)` lies outside a convex polygon (ccw; 1 or 2 points allowed).
fn outside(h: &[[f32; 2]], x: f32, y: f32) -> f32 {
    let n = h.len();
    if n >= 3 {
        let inside = (0..n).all(|i| {
            let (a, b) = (h[i], h[(i + 1) % n]);
            (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]) >= -1e-9
        });
        if inside {
            return 0.0;
        }
    }
    let mut best = 1e9_f32;
    for i in 0..n {
        let (a, b) = (h[i], h[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 { (((x - a[0]) * dx + (y - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (x - (a[0] + dx * t)).hypot(y - (a[1] + dy * t));
        best = best.min(d);
    }
    best
}
