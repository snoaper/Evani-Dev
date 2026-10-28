//! A uniform hash grid for nearby points and triangles, and the two exact
//! queries the generator needs: closest point on a triangle and a segment
//! against a triangle. Lookups only: results never depend on the map's
//! iteration order (candidates are sorted before use).

use std::collections::HashMap;

use evani_model::vec::{V3, add, cross, dot, mul, sub};

pub(crate) struct Grid {
    cell: f64,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

/// Cells spanned per axis are capped so one huge triangle cannot stall a
/// build; a triangle that big is still found from every cell near its
/// corners and centre.
const MAX_SPAN: i64 = 48;

impl Grid {
    pub fn new(cell: f64) -> Self {
        Grid { cell: cell.max(1e-6), cells: HashMap::new() }
    }

    fn key(&self, p: V3) -> [i64; 3] {
        [(p[0] / self.cell).floor() as i64, (p[1] / self.cell).floor() as i64, (p[2] / self.cell).floor() as i64]
    }

    pub fn insert_point(&mut self, p: V3, id: u32) {
        self.cells.entry(self.key(p)).or_default().push(id);
    }

    pub fn insert_box(&mut self, lo: V3, hi: V3, id: u32) {
        let (a, b) = (self.key(lo), self.key(hi));
        if (0..3).any(|k| b[k] - a[k] > MAX_SPAN) {
            // Too big to list in every cell: list it near its corners and centre.
            for p in [lo, hi, mul(add(lo, hi), 0.5)] {
                self.insert_point(p, id);
            }
            return;
        }
        for x in a[0]..=b[0] {
            for y in a[1]..=b[1] {
                for z in a[2]..=b[2] {
                    self.cells.entry([x, y, z]).or_default().push(id);
                }
            }
        }
    }

    /// Ids listed in cells within `r` of `p` (sorted, no repeats).
    pub fn near(&self, p: V3, r: f64, out: &mut Vec<u32>) {
        self.in_box(sub(p, [r; 3]), add(p, [r; 3]), out);
    }

    /// Ids listed in cells overlapping the box (sorted, no repeats).
    pub fn in_box(&self, lo: V3, hi: V3, out: &mut Vec<u32>) {
        out.clear();
        let (a, b) = (self.key(lo), self.key(hi));
        let span = |k: usize| (b[k] - a[k]).clamp(0, MAX_SPAN);
        for x in a[0]..=a[0] + span(0) {
            for y in a[1]..=a[1] + span(1) {
                for z in a[2]..=a[2] + span(2) {
                    if let Some(v) = self.cells.get(&[x, y, z]) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// The point of triangle `abc` closest to `p` (by the triangle's Voronoi
/// regions: three corners, three edges, the face).
pub(crate) fn closest_on_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let den = d1 - d3;
        return if den > 0.0 { add(a, mul(ab, d1 / den)) } else { a };
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let den = d2 - d6;
        return if den > 0.0 { add(a, mul(ac, d2 / den)) } else { a };
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let den = (d4 - d3) + (d5 - d6);
        return if den > 0.0 { add(b, mul(sub(c, b), (d4 - d3) / den)) } else { b };
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-300 {
        return a;
    }
    let v = vb / sum;
    let w = vc / sum;
    add(a, add(mul(ab, v), mul(ac, w)))
}

/// Where the ray `o + t d` (t in 0..=t_max) first crosses triangle `abc`,
/// from either side; `None` when it misses.
pub(crate) fn ray_hits(o: V3, d: V3, t_max: f64, a: V3, b: V3, c: V3) -> Option<f64> {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let pv = cross(d, e2);
    let det = dot(e1, pv);
    if det.abs() < 1e-18 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = sub(o, a);
    let u = dot(tv, pv) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = cross(tv, e1);
    let v = dot(d, qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, qv) * inv;
    (t >= 0.0 && t <= t_max).then_some(t)
}
