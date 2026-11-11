//! Surfaces (`u`, `v` in [0, 1] to a point) and bands on them.
//!
//! Kinds: `lines` (a hull-style surface from a half-breadth formula between a
//! bottom and a top line), `loft` (through section polylines), `sweep` (a
//! profile along a path), `lathe` (a profile revolved about y), `ruled`
//! (straight between two paths) and `formula` (x, y, z from formulas of u
//! and v). A [`Band`] is the region between two lines `v = a(u)` and
//! `v = b(u)` over `u0..u1`; it splits into strips (strakes, courses), and a
//! strip becomes a board: the surface is the board's face and the board
//! extends behind it (against the normal) by its thickness.

use crate::curve::{Frame, Path};
use crate::formula::Formula;
use crate::vec::*;

/// A formula over two leading slots plus fixed extra slots (parameters).
#[derive(Clone, Debug, PartialEq)]
pub struct Fx {
    pub f: Formula,
    pub env: Vec<f64>,
}

impl Fx {
    pub fn eval(&self, a: f64, b: f64) -> f64 {
        let mut v = Vec::with_capacity(self.env.len() + 2);
        v.push(a);
        v.push(b);
        v.extend_from_slice(&self.env);
        self.f.eval(&v)
    }
    pub fn constant(x: f64) -> Fx {
        Fx { f: Formula::constant(x), env: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    /// `x = (u - 0.5) * length`, station `t = 2u - 1` in [-1, 1]; height
    /// `y` from `bottom(t)` to `top(t)` by `v`; `z = side * half_breadth(t, y)`.
    Lines { length: f64, half_breadth: Fx, bottom: Fx, top: Fx, side: f64 },
    /// Through section polylines (equal point counts): `u` along a section,
    /// `v` across the sections (centripetal Catmull-Rom between them).
    Loft { sections: Vec<Vec<V3>> },
    /// A profile polyline (`u`) along frames (`v`).
    Sweep { frames: Vec<Frame>, profile: Vec<[f64; 2]> },
    /// A profile of (radius, height) points (`v`) revolved about y from angle
    /// `a0` to `a1` (radians, `u`), centred on `centre`.
    Lathe { profile: Vec<[f64; 2]>, a0: f64, a1: f64, centre: V3 },
    /// Straight lines from `a(u)` to `b(u)` (both by arc-length fraction).
    Ruled { a: Path, b: Path },
    /// `x, y, z` from formulas of (u, v).
    Formula { x: Fx, y: Fx, z: Fx },
}

fn poly2(p: &[[f64; 2]], t: f64) -> [f64; 2] {
    if p.len() < 2 {
        return p.first().copied().unwrap_or([0.0; 2]);
    }
    let x = t.clamp(0.0, 1.0) * (p.len() - 1) as f64;
    let i = (x.floor() as usize).min(p.len() - 2);
    let f = x - i as f64;
    [p[i][0] + (p[i + 1][0] - p[i][0]) * f, p[i][1] + (p[i + 1][1] - p[i][1]) * f]
}

impl Surface {
    pub fn point(&self, u: f64, v: f64) -> V3 {
        let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        match self {
            Surface::Lines { length, half_breadth, bottom, top, side } => {
                let t = 2.0 * u - 1.0;
                let (y0, y1) = (bottom.eval(t, 0.0), top.eval(t, 0.0));
                let y = y0 + (y1 - y0) * v;
                [(u - 0.5) * length, y, side * half_breadth.eval(t, y).max(0.0)]
            }
            Surface::Loft { sections } => {
                let along: Vec<V3> = sections.iter().map(|s| crate::curve::Curve::Polyline(s.clone()).point(u)).collect();
                if along.len() < 3 {
                    crate::curve::Curve::Polyline(along).point(v)
                } else {
                    crate::curve::Curve::CatmullRom(along).point(v)
                }
            }
            Surface::Sweep { frames, profile } => {
                if frames.is_empty() {
                    return [0.0; 3];
                }
                let x = v * (frames.len() - 1) as f64;
                let i = (x.floor() as usize).min(frames.len().saturating_sub(2));
                let f = x - i as f64;
                let q = poly2(profile, u);
                let a = frames[i].local(q[0], q[1], 0.0);
                match frames.get(i + 1) {
                    Some(b) => lerp(a, b.local(q[0], q[1], 0.0), f),
                    None => a,
                }
            }
            Surface::Lathe { profile, a0, a1, centre } => {
                let q = poly2(profile, v);
                let (s, c) = crate::dmath::sin_cos(a0 + (a1 - a0) * u);
                add(*centre, [q[0] * c, q[1], -q[0] * s])
            }
            Surface::Ruled { a, b } => lerp(a.at(a.length() * u), b.at(b.length() * u), v),
            Surface::Formula { x, y, z } => [x.eval(u, v), y.eval(u, v), z.eval(u, v)],
        }
    }
    /// `dP/du x dP/dv`, normalised (central differences, one-sided at the edges).
    pub fn normal(&self, u: f64, v: f64) -> V3 {
        let h = 1e-4;
        let (ua, ub) = ((u - h).max(0.0), (u + h).min(1.0));
        let (va, vb) = ((v - h).max(0.0), (v + h).min(1.0));
        let du = sub(self.point(ub, v), self.point(ua, v));
        let dv = sub(self.point(u, vb), self.point(u, va));
        norm_or(cross(du, dv), [0.0, 1.0, 0.0])
    }
    /// Length of `dP/dv` per unit `v` at (u, v).
    pub fn v_speed(&self, u: f64, v: f64) -> f64 {
        let h = 1e-4;
        let (va, vb) = ((v - h).max(0.0), (v + h).min(1.0));
        dist(self.point(u, vb), self.point(u, va)) / (vb - va).max(1e-12)
    }
    /// Length of `dP/du` per unit `u` at (u, v).
    pub fn u_speed(&self, u: f64, v: f64) -> f64 {
        let h = 1e-4;
        let (ua, ub) = ((u - h).max(0.0), (u + h).min(1.0));
        dist(self.point(ub, v), self.point(ua, v)) / (ub - ua).max(1e-12)
    }
}

/// A region of a surface: `u` from `u0` to `u1`, `v` from `a(u)` to `b(u)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub u0: f64,
    pub u1: f64,
    /// Lower and upper line as `v` per `u`, sampled (u, v) pairs; constant
    /// lines are one pair.
    pub a: Vec<[f64; 2]>,
    pub b: Vec<[f64; 2]>,
}

fn line_at(l: &[[f64; 2]], u: f64) -> f64 {
    match l {
        [] => 0.0,
        [only] => only[1],
        _ => {
            let i = l.partition_point(|p| p[0] <= u).clamp(1, l.len() - 1);
            let (p, q) = (l[i - 1], l[i]);
            let f = if (q[0] - p[0]).abs() > 1e-300 { ((u - p[0]) / (q[0] - p[0])).clamp(0.0, 1.0) } else { 0.0 };
            p[1] + (q[1] - p[1]) * f
        }
    }
}

impl Band {
    /// The whole surface.
    pub fn full() -> Band {
        Band { u0: 0.0, u1: 1.0, a: vec![[0.0, 0.0]], b: vec![[0.0, 1.0]] }
    }
    pub fn rect(u0: f64, u1: f64, v0: f64, v1: f64) -> Band {
        Band { u0, u1, a: vec![[0.0, v0]], b: vec![[0.0, v1]] }
    }
    pub fn v_range(&self, u: f64) -> (f64, f64) {
        (line_at(&self.a, u), line_at(&self.b, u))
    }
    /// Split into `n` strips of equal share across (`v`).
    pub fn strips(&self, n: usize) -> Vec<Band> {
        let n = n.max(1);
        let us: Vec<f64> = if self.a.len() <= 1 && self.b.len() <= 1 {
            vec![self.u0]
        } else {
            let mut u: Vec<f64> = self.a.iter().chain(&self.b).map(|p| p[0]).collect();
            u.sort_by(f64::total_cmp);
            u.dedup();
            u
        };
        (0..n).map(|k| {
            let f0 = k as f64 / n as f64;
            let f1 = (k + 1) as f64 / n as f64;
            let line = |f: f64| us.iter().map(|&u| {
                let (a, b) = self.v_range(u);
                [u, a + (b - a) * f]
            }).collect();
            Band { u0: self.u0, u1: self.u1, a: line(f0), b: line(f1) }
        }).collect()
    }
    /// Cut along `u` into `n` equal lengths.
    pub fn lengths(&self, n: usize) -> Vec<Band> {
        let n = n.max(1);
        (0..n).map(|k| Band {
            u0: self.u0 + (self.u1 - self.u0) * k as f64 / n as f64,
            u1: self.u0 + (self.u1 - self.u0) * (k + 1) as f64 / n as f64,
            a: self.a.clone(),
            b: self.b.clone(),
        }).collect()
    }
}

/// How a strip becomes a board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Board {
    pub thickness: f64,
    /// Total gap between neighbouring boards across (m); half on each edge.
    pub gap: f64,
    /// Gap at each end (m).
    pub end_gap: f64,
    /// Rows along and columns across the board.
    pub rows: usize,
    pub cols: usize,
    /// Proud offset along the normal (m), for lapped strakes and wear.
    pub proud: f64,
    /// Chamfer on the two long face edges (m across and m deep; 0: square
    /// edges). Each chamfer is its own flat strip, so it catches the light.
    pub bevel: f64,
}

/// The board on `strip` of `s`: its face on the surface (moved `proud`
/// along the normal), its back `thickness` behind it.
pub fn board(s: &Surface, strip: &Band, b: &Board) -> crate::mesh::Mesh {
    let (top, bot) = board_grids(s, strip, b, 0.0);
    if b.bevel <= 0.0 {
        return crate::mesh::slab(&top, &bot);
    }
    // Chamfered: the face is inset by `bevel` across, and each long edge
    // gets a strip down to `bevel` below the face. The back keeps its
    // columns (the outer ones doubled, so it gains no triangles).
    let (face, _) = board_grids(s, strip, b, b.bevel);
    let cols = face.first().map_or(0, Vec::len);
    let depth = b.bevel.min(0.9 * b.thickness);
    let mut t2 = Vec::with_capacity(top.len());
    let mut b2 = Vec::with_capacity(top.len());
    for (r, row) in face.iter().enumerate() {
        let (lo, hi) = (top[r][0], top[r][top[r].len() - 1]);
        let (nl, nh) = (unit(sub(lo, bot[r][0])), unit(sub(hi, bot[r][bot[r].len() - 1])));
        let mut tr = Vec::with_capacity(cols + 2);
        tr.push(sub(lo, mul(nl, depth)));
        tr.extend_from_slice(row);
        tr.push(sub(hi, mul(nh, depth)));
        t2.push(tr);
        let mut br = Vec::with_capacity(cols + 2);
        br.push(bot[r][0]);
        br.extend_from_slice(&bot[r]);
        br.push(bot[r][bot[r].len() - 1]);
        b2.push(br);
    }
    crate::mesh::slab_creased(&t2, &b2, &[1, cols])
}

fn unit(v: V3) -> V3 {
    norm_or(v, [0.0, 1.0, 0.0])
}

/// A board's face and back grids, the face inset `inset` (m) across at
/// each long edge.
fn board_grids(s: &Surface, strip: &Band, b: &Board, inset: f64) -> (Vec<Vec<V3>>, Vec<Vec<V3>>) {
    let rows = b.rows.max(1);
    let cols = b.cols.max(1);
    let mid_v = |u: f64| {
        let (a, bb) = strip.v_range(u);
        0.5 * (a + bb)
    };
    // The end gap as a share of u at the strip's middle line.
    let um = 0.5 * (strip.u0 + strip.u1);
    let du_end = (b.end_gap / s.u_speed(um, mid_v(um)).max(1e-9)).min(0.45 * (strip.u1 - strip.u0));
    let (u0, u1) = (strip.u0 + du_end, strip.u1 - du_end);
    let mut top = Vec::with_capacity(rows + 1);
    let mut bot = Vec::with_capacity(rows + 1);
    for i in 0..=rows {
        let u = u0 + (u1 - u0) * i as f64 / rows as f64;
        let (va, vb) = strip.v_range(u);
        let dv = ((0.5 * b.gap + inset) / s.v_speed(u, 0.5 * (va + vb)).max(1e-9)).min(0.45 * (vb - va).abs());
        let (va, vb) = if vb >= va { (va + dv, vb - dv) } else { (va - dv, vb + dv) };
        let mut tr = Vec::with_capacity(cols + 1);
        let mut br = Vec::with_capacity(cols + 1);
        for j in 0..=cols {
            let v = va + (vb - va) * j as f64 / cols as f64;
            let p = s.point(u, v);
            let n = s.normal(u, v);
            tr.push(add(p, mul(n, b.proud)));
            br.push(sub(p, mul(n, b.thickness - b.proud)));
        }
        top.push(tr);
        bot.push(br);
    }
    (top, bot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_and_boards() {
        let names: Vec<String> = ["t", "y", "beam"].iter().map(|s| s.to_string()).collect();
        let hb = Fx { f: Formula::parse("beam / 2 * (1 - t * t * 0.6) * (0.6 + 0.4 * y)", &names).unwrap(), env: vec![3.0] };
        let s = Surface::Lines { length: 6.0, half_breadth: hb, bottom: Fx::constant(0.0), top: Fx::constant(1.0), side: 1.0 };
        let p = s.point(0.5, 1.0);
        assert!(dist(p, [0.0, 1.0, 1.5]) < 1e-12, "{p:?}");
        let strips = Band::full().strips(4);
        assert_eq!(strips.len(), 4);
        let b = Board { thickness: 0.03, gap: 0.004, end_gap: 0.0, rows: 12, cols: 2, proud: 0.0, bevel: 0.0 };
        for st in &strips {
            let m = board(&s, st, &b);
            assert!(m.volume() > 0.0);
            assert_eq!(m.tris.len(), 2 * (12 * 2 * 2) + 2 * 12 * 2 + 2 * 2 * 2);
            // Chamfered: two more strips on the face, one more triangle per
            // end at each edge, the back unchanged; still a closed solid
            // (slightly smaller) with a flat chamfer strip.
            let c = board(&s, st, &Board { bevel: 0.005, ..b });
            assert_eq!(c.tris.len(), m.tris.len() + 2 * 12 * 2 + 2 * 2);
            assert!(c.volume() > 0.0 && c.volume() < m.volume());
            assert!(crate::check::piece(&c).is_empty(), "{:?}", crate::check::piece(&c));
        }
        let lathe = Surface::Lathe { profile: vec![[1.0, 0.0], [1.0, 2.0]], a0: 0.0, a1: core::f64::consts::PI, centre: [0.0; 3] };
        assert!(dist(lathe.point(1.0, 1.0), [-1.0, 2.0, 0.0]) < 1e-9);
        let loft = Surface::Loft { sections: vec![vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]], vec![[0.0, 1.0, 2.0], [1.0, 1.0, 2.0]]] };
        assert!(dist(loft.point(1.0, 0.5), [1.0, 0.0, 1.0]) < 1e-9);
        let ruled = Surface::Ruled { a: Path::from_points(vec![[0.0; 3], [2.0, 0.0, 0.0]]), b: Path::from_points(vec![[0.0, 1.0, 0.0], [2.0, 1.0, 0.0]]) };
        assert!(dist(ruled.point(0.5, 0.5), [1.0, 0.5, 0.0]) < 1e-12);
        let n = ruled.normal(0.5, 0.5);
        assert!(dist(n, [0.0, 0.0, 1.0]) < 1e-9, "{n:?}");
    }
}
