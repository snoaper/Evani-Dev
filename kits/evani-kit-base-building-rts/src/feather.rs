//! The fog's soft edge (draw only, never gameplay). The player's fog is
//! turned into a smooth field: per cell, the share of seen (and of visible)
//! cells in a small box round it, read between cell centres bilinearly. Its
//! contours are smooth curves, not the cells' staircase, so:
//!
//! - the line between seen ground and fog is drawn as a gradient of thin
//!   ground strips (a few per cell) from the ground's colour to the fog's;
//! - the ground and the ridges beyond the map edge shade smoothly along the
//!   edge instead of switching from fog to lit at one edge cell.

use crate::grid::{Fog, Grid, UNEXPLORED, VISIBLE};

/// Fog steps a strip's shade is rounded to (equal neighbours merge).
const STEPS: f32 = 32.0;

/// Ease 0..1 so the gradient starts and ends without a crease.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The fog as a smooth field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FogField {
    w: i32,
    h: i32,
    cell: f32,
    half: [f32; 2],
    /// Share of seen (not unexplored) cells in the box round each cell.
    seen: Vec<f32>,
    /// Share of visible cells in the box round each cell.
    vis: Vec<f32>,
}

impl FogField {
    /// Box of `2r + 1` cells a side; cells beyond the map copy the nearest
    /// edge cell. `r = 0` keeps the cells as they are.
    pub fn build(g: &Grid, fog: &Fog, r: i32) -> Self {
        let r = r.clamp(0, 8);
        let (w, h) = (g.w.max(1), g.h.max(1));
        let n = (w * h) as usize;
        let at = |x: i32, y: i32| fog.at(g, [x.clamp(0, w - 1), y.clamp(0, h - 1)]);
        // A box blur of `r`, then a second of 1: the second rounds the
        // first's creases, so the strips step evenly across the band.
        let pass = |src: &[f32], r: i32| -> Vec<f32> {
            let k = ((2 * r + 1) * (2 * r + 1)) as f32;
            let px = |x: i32, y: i32| src[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
            let mut rows = vec![0.0f32; n];
            for y in 0..h {
                for x in 0..w {
                    rows[(y * w + x) as usize] = (-r..=r).map(|d| px(x + d, y)).sum();
                }
            }
            let mut out = vec![0.0f32; n];
            for y in 0..h {
                for x in 0..w {
                    let s: f32 = (-r..=r).map(|d| rows[((y + d).clamp(0, h - 1) * w + x) as usize]).sum();
                    out[(y * w + x) as usize] = s / k;
                }
            }
            out
        };
        let blur = |val: &dyn Fn(u8) -> f32| -> Vec<f32> {
            let raw: Vec<f32> = (0..n as i32).map(|i| val(at(i % w, i / w))).collect();
            if r == 0 {
                return raw;
            }
            pass(&pass(&raw, r), 1)
        };
        let seen = blur(&|f| if f == UNEXPLORED { 0.0 } else { 1.0 });
        let vis = blur(&|f| if f == VISIBLE { 1.0 } else { 0.0 });
        Self { w, h, cell: g.cell, half: g.half(), seen, vis }
    }

    /// How lit the seen ground at `p` looks: 1 in sight now, 0 only
    /// remembered (explored).
    pub fn lit(&self, p: [f32; 2]) -> f32 {
        let s = self.sample(&self.seen, p);
        if s <= 1e-4 {
            return 0.0;
        }
        ease(self.sample(&self.vis, p) / s)
    }

    /// The share of visible ground round cell `c` (its own value).
    fn vis_at(&self, c: [i32; 2]) -> f32 {
        self.get(&self.vis, c[0], c[1])
    }

    fn get(&self, v: &[f32], x: i32, y: i32) -> f32 {
        v.get((y.clamp(0, self.h - 1) * self.w + x.clamp(0, self.w - 1)) as usize).copied().unwrap_or(1.0)
    }

    /// A field read bilinearly between cell centres at `p` (`[x, z]`).
    fn sample(&self, v: &[f32], p: [f32; 2]) -> f32 {
        if v.is_empty() || self.cell <= 0.0 {
            return 1.0;
        }
        let fx = (p[0] + self.half[0]) / self.cell - 0.5;
        let fz = (p[1] + self.half[1]) / self.cell - 0.5;
        let (x0, z0) = (fx.floor(), fz.floor());
        let (tx, tz) = (fx - x0, fz - z0);
        let (x0, z0) = (x0 as i32, z0 as i32);
        let a = self.get(v, x0, z0) + (self.get(v, x0 + 1, z0) - self.get(v, x0, z0)) * tx;
        let b = self.get(v, x0, z0 + 1) + (self.get(v, x0 + 1, z0 + 1) - self.get(v, x0, z0 + 1)) * tx;
        a + (b - a) * tz
    }

    /// How fogged the ground at `p` looks: 0 seen, 1 fog.
    pub fn fogginess(&self, p: [f32; 2]) -> f32 {
        ease(1.0 - self.sample(&self.seen, p))
    }

    /// The share of seen ground round cell `c` (its own value, no blending).
    fn seen_at(&self, c: [i32; 2]) -> f32 {
        self.get(&self.seen, c[0], c[1])
    }

    /// How scenery beyond the map edge near `p` shows: `(dim, fade)`, the
    /// colour scaled by `dim` then blended towards the fog colour by
    /// `fade`. Read on the edge (`p` clamped onto the map), averaged
    /// `reach` metres along it, so neighbours shade alike.
    pub fn edge_look(&self, p: [f32; 2], reach: f32, look: &EdgeLook) -> (f32, f32) {
        let k = self.cell * 0.5;
        let on = [p[0].clamp(-self.half[0] + k, self.half[0] - k), p[1].clamp(-self.half[1] + k, self.half[1] - k)];
        // Along the edge: x for the north and south sides, z for east and west.
        let beyond_x = p[0].abs() > self.half[0];
        let beyond_z = p[1].abs() > self.half[1];
        let axis = match (beyond_x, beyond_z) {
            (true, false) => Some(1),
            (false, true) => Some(0),
            _ => None,
        };
        let (mut s, mut v, mut wsum) = (0.0, 0.0, 0.0);
        let n = if axis.is_some() { 4 } else { 0 };
        for i in -n..=n {
            let off = if n > 0 { reach.max(0.0) * i as f32 / n as f32 } else { 0.0 };
            let mut q = on;
            if let Some(a) = axis {
                let lim = self.half[a] - k;
                q[a] = (q[a] + off).clamp(-lim, lim);
            }
            let wt = 1.0 - (i as f32).abs() / (n as f32 + 1.0);
            s += self.sample(&self.seen, q) * wt;
            v += self.sample(&self.vis, q) * wt;
            wsum += wt;
        }
        let (s, v) = if wsum > 0.0 { (s / wsum, v / wsum) } else { (1.0, 1.0) };
        let lit = if s > 1e-4 { (v / s).clamp(0.0, 1.0) } else { 0.0 };
        let q = ease(1.0 - s);
        let seen_dim = look.explored_dim + (1.0 - look.explored_dim) * lit;
        let seen_fade = look.explored_to_fog * (1.0 - lit);
        (seen_dim + (1.0 - seen_dim) * q, seen_fade + (look.unexplored_to_fog - seen_fade) * q)
    }
}

/// The fog look scenery beyond the edge takes from the edge it borders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeLook {
    pub explored_dim: f32,
    pub explored_to_fog: f32,
    /// Fade when the edge is unexplored (1 = flat fog colour).
    pub unexplored_to_fog: f32,
}

/// One strip of the fog's gradient on cell `cell`: `x0..x0 + len` along x,
/// centred on `z`, `depth` across, fogged by `f` (0 seen .. 1 fog) and lit
/// by `lit` (0 remembered .. 1 in sight now).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    pub cell: [i32; 2],
    pub x0: f32,
    pub len: f32,
    pub z: f32,
    pub depth: f32,
    pub f: f32,
    pub lit: f32,
}

/// The gradient strips (`n` rows a cell, merged along x where equal), for
/// both lines: seen ground to fog, and ground in sight to ground only
/// remembered. A cell gets none where its slab already shows the right
/// shade (no line near it).
pub fn strips(g: &Grid, fog: &Fog, field: &FogField, n: i32) -> Vec<Strip> {
    let n = n.clamp(1, 8);
    let s = g.cell / n as f32;
    let h = g.half();
    let quant = |f: f32| (f * STEPS).round() / STEPS;
    let mut out: Vec<Strip> = Vec::new();
    for y in 0..g.h {
        for x in 0..g.w {
            let (mut lo, mut hi, mut vlo, mut vhi) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let v = field.seen_at([x + dx, y + dy]);
                    lo = lo.min(v);
                    hi = hi.max(v);
                    let v = field.vis_at([x + dx, y + dy]);
                    vlo = vlo.min(v);
                    vhi = vhi.max(v);
                }
            }
            let state = fog.at(g, [x, y]);
            let fogged = state == UNEXPLORED;
            // The slab's own shade: fog on unexplored cells; seen ground,
            // lit where in sight, elsewhere.
            let slab = (if fogged { 1.0 } else { 0.0 }, if state == VISIBLE { 1.0 } else { 0.0 });
            let flat_seen = lo >= 1.0 - 1e-4 && (vhi <= 1e-4 || vlo >= 1.0 - 1e-4);
            if (fogged && hi <= 1e-4) || (!fogged && flat_seen) {
                continue;
            }
            let shade = |px: f32, z: f32| {
                let f = quant(field.fogginess([px, z]));
                // Under full fog the lit shade never shows: keep it equal
                // so the run merges and matches the slab.
                let lit = if f >= 1.0 { slab.1 } else { quant(field.lit([px, z])) };
                (f, lit)
            };
            let x_at = |i: i32| x as f32 * g.cell - h[0] + (i as f32 + 0.5) * s;
            // Strips of the row before on this cell: one equal strip right
            // above a new one grows to cover it (fewer pieces to draw).
            let mut prev = out.len()..out.len();
            for j in 0..n {
                let z = y as f32 * g.cell - h[1] + (j as f32 + 0.5) * s;
                let mut i = 0;
                while i < n {
                    let v = shade(x_at(i), z);
                    let mut e = i;
                    while e + 1 < n && shade(x_at(e + 1), z) == v {
                        e += 1;
                    }
                    let same = if fogged { v.0 >= 1.0 } else { v == slab };
                    if !same {
                        let x0 = x as f32 * g.cell - h[0] + i as f32 * s;
                        let len = (e - i + 1) as f32 * s;
                        let top = z - s * 0.5;
                        let up = out[prev.clone()].iter().position(|p| p.x0 == x0 && p.len == len && p.f == v.0 && p.lit == v.1 && (p.z + p.depth * 0.5 - top).abs() < s * 0.01);
                        match up.map(|k| prev.start + k) {
                            Some(k) => {
                                out[k].z += s * 0.5;
                                out[k].depth += s;
                            }
                            None => out.push(Strip { cell: [x, y], x0, len, z, depth: s, f: v.0, lit: v.1 }),
                        }
                    }
                    i = e + 1;
                }
                // Every strip on this cell so far may grow into the next row
                // (only those whose bottom meets it can).
                prev = prev.start..out.len();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MapData, MAP_JSON};
    use crate::grid::EXPLORED;

    fn grid() -> Grid {
        Grid::new(&MapData::from_json(MAP_JSON).unwrap())
    }

    /// The west half seen, the east half fog.
    fn half_seen(g: &Grid) -> Fog {
        let mut fog = Fog::new(g);
        for y in 0..g.h {
            for x in 0..g.w / 2 {
                fog.cells[(y * g.w + x) as usize] = EXPLORED;
            }
        }
        fog
    }

    /// Across a straight fog line the shade climbs steadily from seen to
    /// fog over a few metres: no jump bigger than a small step anywhere.
    #[test]
    fn the_fog_line_is_a_gradient() {
        let g = grid();
        let fog = half_seen(&g);
        let field = FogField::build(&g, &fog, 2);
        let (mut last, mut biggest) = (field.fogginess([-20.0, 3.0]), 0.0f32);
        assert!(last < 1e-3, "deep in seen ground: {last}");
        let mut x = -20.0;
        while x < 20.0 {
            x += 0.1;
            let f = field.fogginess([x, 3.0]);
            assert!(f >= last - 1e-5, "fog never lifts going east: {last} -> {f} at {x}");
            biggest = biggest.max(f - last);
            last = f;
        }
        assert!(last > 1.0 - 1e-3, "deep in fog: {last}");
        assert!(biggest < 0.05, "largest step {biggest}");
        let mid = field.fogginess([0.0, 3.0]);
        assert!((mid - 0.5).abs() < 0.05, "half fogged on the line: {mid}");
    }

    /// The strips cover only the band either side of the fog line, and the
    /// shade of the strips on one row climbs eastward.
    #[test]
    fn strips_lie_on_the_fog_line() {
        let g = grid();
        let fog = half_seen(&g);
        let field = FogField::build(&g, &fog, 2);
        let strips = strips(&g, &fog, &field, 3);
        assert!(!strips.is_empty());
        for s in &strips {
            assert!(s.x0 >= -8.0 - 1e-3 && s.x0 + s.len <= 8.0 + 1e-3, "{s:?}");
            assert!(s.f > 0.0 && s.f < 1.0, "{s:?}");
        }
        let z = strips.iter().find(|s| s.cell[1] == 10).map(|s| s.z).unwrap();
        let mut row: Vec<_> = strips.iter().filter(|s| (s.z - z).abs() < 1e-4).collect();
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        assert!(row.len() > 3, "{row:?}");
        assert!(row.windows(2).all(|w| w[0].f < w[1].f), "{row:?}");
    }

    /// The line between ground in sight and ground only remembered is a
    /// gradient too: strips appear on it, with no fog in them, and their
    /// lit shade falls steadily going east (out of sight).
    #[test]
    fn the_sight_line_is_a_gradient() {
        let g = grid();
        let mut fog = Fog::new(&g);
        for (i, c) in fog.cells.iter_mut().enumerate() {
            *c = if (i as i32 % g.w) < g.w / 2 { VISIBLE } else { EXPLORED };
        }
        let field = FogField::build(&g, &fog, 2);
        let (mut last, mut biggest) = (field.lit([-20.0, 3.0]), 0.0f32);
        assert!(last > 1.0 - 1e-3, "deep in sight: {last}");
        let mut x = -20.0;
        while x < 20.0 {
            x += 0.1;
            let l = field.lit([x, 3.0]);
            assert!(l <= last + 1e-5, "sight never returns going east: {last} -> {l} at {x}");
            biggest = biggest.max(last - l);
            last = l;
        }
        assert!(last < 1e-3, "deep in memory: {last}");
        assert!(biggest < 0.05, "largest step {biggest}");
        let strips = strips(&g, &fog, &field, 4);
        assert!(!strips.is_empty());
        assert!(strips.iter().all(|s| s.f == 0.0 && s.lit > 0.0 && s.lit < 1.0), "{strips:?}");
    }

    /// Merged strips never overlap, and every point a strip covers shows
    /// that point's own fog shade (merging never stretches a shade over a
    /// row that differs). Diagonal lines, so shades vary both ways.
    #[test]
    fn merged_strips_keep_every_shade() {
        let g = grid();
        let mut fog = Fog::new(&g);
        for (i, c) in fog.cells.iter_mut().enumerate() {
            let (x, y) = (i as i32 % g.w, i as i32 / g.w);
            *c = if x + y < g.w / 2 { VISIBLE } else if x + y < g.w { EXPLORED } else { UNEXPLORED };
        }
        let field = FogField::build(&g, &fog, 2);
        let n = 4;
        let all = strips(&g, &fog, &field, n);
        assert!(all.iter().any(|s| s.depth > g.cell / n as f32 * 1.5), "some strips merged across rows");
        let s = g.cell / n as f32;
        let h = g.half();
        let quant = |f: f32| (f * STEPS).round() / STEPS;
        for y in 0..g.h * n {
            for x in 0..g.w * n {
                let p = [(x as f32 + 0.5) * s - h[0], (y as f32 + 0.5) * s - h[1]];
                let on: Vec<_> = all.iter().filter(|t| p[0] > t.x0 && p[0] < t.x0 + t.len && (p[1] - t.z).abs() < t.depth * 0.5).collect();
                assert!(on.len() <= 1, "{p:?} {on:?}");
                if let Some(t) = on.first() {
                    assert_eq!(t.f, quant(field.fogginess(p)), "{p:?} {t:?}");
                }
            }
        }
    }

    /// Scenery beyond the edge shades smoothly along it: moving along the
    /// south edge across the fog line its fade changes in small steps.
    #[test]
    fn edge_look_changes_smoothly_along_the_edge() {
        let g = grid();
        let fog = half_seen(&g);
        let field = FogField::build(&g, &fog, 2);
        let look = EdgeLook { explored_dim: 0.5, explored_to_fog: 0.2, unexplored_to_fog: 0.85 };
        let z = g.half()[1] + 10.0;
        let (mut last, mut biggest) = (field.edge_look([-30.0, z], 5.0, &look), 0.0f32);
        assert!((last.0 - 0.5).abs() < 1e-3 && (last.1 - 0.2).abs() < 1e-3, "seen edge: {last:?}");
        let mut x = -30.0;
        while x < 30.0 {
            x += 0.5;
            let l = field.edge_look([x, z], 5.0, &look);
            biggest = biggest.max((l.1 - last.1).abs()).max((l.0 - last.0).abs());
            last = l;
        }
        assert!((last.1 - 0.85).abs() < 1e-3, "fog edge: {last:?}");
        assert!(biggest < 0.06, "largest step {biggest}");
    }
}
