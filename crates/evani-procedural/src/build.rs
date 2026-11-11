//! Shared pieces for the built world (batch B, "1 out of 10"): polygons
//! extruded into slabs and prisms, profiles swept along paths and revolved
//! round the y axis (with hard edges where the profile turns sharply),
//! square bars and round rods between two points, gable and hip roofs,
//! bevelled boxes, window grids on a facade, gears, stairs and points
//! along a path. Plain maths over [`crate::geo`] and [`crate::shapes`]: no
//! tables, no third-party code. Ideas from three.js (MIT): ExtrudeGeometry
//! (a shape's caps by ear clipping and its sides), LatheGeometry (a profile
//! turned round an axis), TubeGeometry (frames along a path) and
//! RoundedBoxGeometry (a box whose corners are pushed onto spheres);
//! written fresh here.

use crate::geo::{self, Mesh, MeshBuilder, normalize};
use crate::shapes::{self as sh, TAU, fix_winding};

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Twice the polygon's signed area (positive: counter-clockwise).
pub fn area2(poly: &[[f32; 2]]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        a[0] * b[1] - b[0] * a[1]
    }).sum()
}

/// A simple polygon's triangles by ear clipping (index triples, counter-
/// clockwise in the polygon's own plane whatever its winding).
pub fn triangulate(poly: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    if area2(poly) < 0.0 {
        idx.reverse();
    }
    let cross2 = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let inside = |p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]| cross2(a, b, p) >= 0.0 && cross2(b, c, p) >= 0.0 && cross2(c, a, p) >= 0.0;
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < 10_000 {
        guard += 1;
        let n = idx.len();
        let mut cut = None;
        for i in 0..n {
            let (ia, ib, ic) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross2(a, b, c) <= 0.0 {
                continue;
            }
            let blocked = idx.iter().any(|&j| j != ia && j != ib && j != ic && inside(poly[j], a, b, c));
            if !blocked {
                cut = Some(i);
                out.push([ia, ib, ic]);
                break;
            }
        }
        match cut {
            Some(i) => {
                idx.remove(i);
            }
            // Degenerate (collinear) leftovers: drop the flattest point.
            None => {
                let flat = |i: usize| cross2(poly[idx[(i + n - 1) % n]], poly[idx[i]], poly[idx[(i + 1) % n]]).abs();
                let i = (0..n).min_by(|&a, &b| flat(a).total_cmp(&flat(b))).unwrap_or(0);
                idx.remove(i);
            }
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// Extrude polygon `poly` (u, v) from w = `w0` to `w1`, the point (u, v, w)
/// placed at u·`ax[0]` + v·`ax[1]` + w·`ax[2]` (orthonormal axes): capped,
/// flat sides.
pub fn extrude_axes(poly: &[[f32; 2]], w0: f32, w1: f32, ax: [[f32; 3]; 3]) -> Mesh {
    let at = |u: f32, v: f32, w: f32| add(add(mul(ax[0], u), mul(ax[1], v)), mul(ax[2], w));
    let mut b = MeshBuilder::new();
    let ccw = area2(poly) >= 0.0;
    for (w, n) in [(w0, mul(ax[2], -1.0)), (w1, ax[2])] {
        let base = b.mesh.vertices.len() as u32;
        for p in poly {
            b.vertex(at(p[0], p[1], w), n);
        }
        for t in triangulate(poly) {
            b.tri(base + t[0] as u32, base + t[1] as u32, base + t[2] as u32);
        }
    }
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let (nx, ny) = if ccw { (dy, -dx) } else { (-dy, dx) };
        let nn = normalize(add(mul(ax[0], nx), mul(ax[1], ny)));
        let a = b.vertex(at(p[0], p[1], w0), nn);
        b.vertex(at(q[0], q[1], w0), nn);
        b.vertex(at(q[0], q[1], w1), nn);
        b.vertex(at(p[0], p[1], w1), nn);
        b.quad(a, a + 1, a + 2, a + 3);
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    m
}

/// Extrude polygon `poly` (x, y) `depth` along z, centred on z = 0.
pub fn extrude(poly: &[[f32; 2]], depth: f32) -> Mesh {
    extrude_axes(poly, -depth * 0.5, depth * 0.5, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
}

/// Extrude polygon `poly` (x, y) `depth` along x, centred on x = 0: its
/// x becomes z (a gable or a profile seen from the side).
pub fn extrude_x(poly: &[[f32; 2]], depth: f32) -> Mesh {
    extrude_axes(poly, -depth * 0.5, depth * 0.5, [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]])
}

/// A floor plan `plan` (x, z) raised from y = `y0` by `h`.
pub fn prism(plan: &[[f32; 2]], y0: f32, h: f32) -> Mesh {
    extrude_axes(plan, y0, y0 + h, [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]])
}

/// A rectangle `w` by `h` round the origin (counter-clockwise).
pub fn rect(w: f32, h: f32) -> Vec<[f32; 2]> {
    let (a, b) = (w * 0.5, h * 0.5);
    vec![[-a, -b], [a, -b], [a, b], [-a, b]]
}

/// A circle of radius `r`, `n` points (counter-clockwise).
pub fn circle(r: f32, n: u32) -> Vec<[f32; 2]> {
    let n = n.clamp(3, 256);
    (0..n).map(|i| {
        let a = i as f32 / n as f32 * TAU;
        [r * a.cos(), r * a.sin()]
    }).collect()
}

/// A regular star or polygon: `points` tips at radius `r`, `inner`·`r`
/// between them (1: a plain polygon of `points` sides).
pub fn star(points: u32, r: f32, inner: f32) -> Vec<[f32; 2]> {
    let n = points.clamp(3, 64);
    if (inner - 1.0).abs() < 1e-4 {
        return circle(r, n);
    }
    (0..n * 2).map(|i| {
        let a = i as f32 / (n * 2) as f32 * TAU + TAU * 0.25;
        let k = if i % 2 == 0 { r } else { r * inner };
        [k * a.cos(), k * a.sin()]
    }).collect()
}

/// A spur gear's outline: `teeth` teeth round radius `r`, `depth` deep.
pub fn gear(teeth: u32, r: f32, depth: f32) -> Vec<[f32; 2]> {
    let n = teeth.clamp(4, 120);
    let mut out = Vec::with_capacity(n as usize * 4);
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        let s = TAU / n as f32;
        for (f, k) in [(0.0, r - depth), (0.18, r), (0.5, r), (0.68, r - depth)] {
            let b = a + s * f;
            out.push([k * b.cos(), k * b.sin()]);
        }
    }
    out
}

/// Per-edge normals of a 2-D profile and, at each point, the normal on
/// either side: the edges' average where they meet gently (under about 53
/// degrees), each its own where the profile turns sharply.
fn edge_normals(profile: &[[f32; 2]], closed: bool) -> Vec<([f32; 2], [f32; 2], [f32; 2])> {
    let n = profile.len();
    let edges = if closed { n } else { n.saturating_sub(1) };
    let ccw = !closed || area2(profile) >= 0.0;
    let en: Vec<[f32; 2]> = (0..edges).map(|i| {
        let (p, q) = (profile[i], profile[(i + 1) % n]);
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-9);
        if ccw { [dy / l, -dx / l] } else { [-dy / l, dx / l] }
    }).collect();
    let smooth = |a: [f32; 2], b: [f32; 2]| {
        if a[0] * b[0] + a[1] * b[1] > 0.6 {
            let s = [a[0] + b[0], a[1] + b[1]];
            let l = (s[0] * s[0] + s[1] * s[1]).sqrt().max(1e-9);
            [s[0] / l, s[1] / l]
        } else {
            a
        }
    };
    (0..edges).map(|i| {
        let prev = if i > 0 { Some(en[i - 1]) } else if closed { Some(en[edges - 1]) } else { None };
        let next = if i + 1 < edges { Some(en[i + 1]) } else if closed { Some(en[0]) } else { None };
        let start = prev.map_or(en[i], |p| smooth(en[i], p));
        let end = next.map_or(en[i], |q| smooth(en[i], q));
        (en[i], start, end)
    }).collect()
}

/// Turn profile `(radius, y)` round the y axis in `sides` steps; the solid
/// lies on the profile's left as it is walked (walk up the outside, down
/// the inside). Sharp turns of the profile stay hard edges; a point on the
/// axis closes the end.
pub fn revolve(profile: &[[f32; 2]], sides: u32) -> Mesh {
    let s = sides.clamp(3, 256);
    let mut b = MeshBuilder::new();
    for (i, (_, n0, n1)) in edge_normals(profile, false).into_iter().enumerate() {
        let (p, q) = (profile[i], profile[i + 1]);
        let base = b.mesh.vertices.len() as u32;
        for k in 0..=s {
            let a = k as f32 / s as f32 * TAU;
            let (c, sn) = (a.cos(), -a.sin());
            b.vertex([p[0] * c, p[1], p[0] * sn], normalize([n0[0] * c, n0[1], n0[0] * sn]));
            b.vertex([q[0] * c, q[1], q[0] * sn], normalize([n1[0] * c, n1[1], n1[0] * sn]));
        }
        for k in 0..s {
            let a = base + k * 2;
            b.quad(a, a + 1, a + 3, a + 2);
        }
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    m
}

/// The frame at each point of `path`: (tangent, right, up), `up` as near
/// world up as the path allows. A path whose ends meet is treated as a
/// loop.
fn frames(path: &[[f32; 3]]) -> Vec<([f32; 3], [f32; 3], [f32; 3])> {
    let n = path.len();
    let looped = n > 2 && len(sub(path[0], path[n - 1])) < 1e-4;
    (0..n).map(|i| {
        let (a, b) = if looped && (i == 0 || i == n - 1) {
            (path[n - 2], path[1])
        } else {
            (path[i.saturating_sub(1)], path[(i + 1).min(n - 1)])
        };
        let t = normalize(sub(b, a));
        let r = cross(t, [0.0, 1.0, 0.0]);
        let r = if len(r) < 1e-4 { [1.0, 0.0, 0.0] } else { normalize(r) };
        (t, r, cross(r, t))
    }).collect()
}

/// Sweep a closed 2-D `profile` (x to the path's right, y up) along
/// `path`; hard edges where the profile turns sharply, ends capped
/// (`caps`) unless the path is a loop.
pub fn sweep(path: &[[f32; 3]], profile: &[[f32; 2]], caps: bool) -> Mesh {
    let n = path.len();
    if n < 2 || profile.len() < 3 {
        return Mesh::default();
    }
    let fr = frames(path);
    let at = |i: usize, p: [f32; 2]| add(path[i], add(mul(fr[i].1, p[0]), mul(fr[i].2, p[1])));
    let dir = |i: usize, d: [f32; 2]| normalize(add(mul(fr[i].1, d[0]), mul(fr[i].2, d[1])));
    let mut b = MeshBuilder::new();
    let pn = profile.len();
    for (e, (_, n0, n1)) in edge_normals(profile, true).into_iter().enumerate() {
        let (p, q) = (profile[e], profile[(e + 1) % pn]);
        let base = b.mesh.vertices.len() as u32;
        for i in 0..n {
            b.vertex(at(i, p), dir(i, n0));
            b.vertex(at(i, q), dir(i, n1));
        }
        for i in 0..(n - 1) as u32 {
            let a = base + i * 2;
            b.quad(a, a + 1, a + 3, a + 2);
        }
    }
    let looped = n > 2 && len(sub(path[0], path[n - 1])) < 1e-4;
    if caps && !looped {
        for (i, k) in [(0usize, -1.0f32), (n - 1, 1.0)] {
            let nn = mul(fr[i].0, k);
            let base = b.mesh.vertices.len() as u32;
            for p in profile {
                b.vertex(at(i, *p), nn);
            }
            for t in triangulate(profile) {
                b.tri(base + t[0] as u32, base + t[1] as u32, base + t[2] as u32);
            }
        }
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    m
}

/// A square-section bar `w` by `h` (across, up) from `a` to `b`: posts,
/// legs, rails, beams, struts.
pub fn bar(a: [f32; 3], b: [f32; 3], w: f32, h: f32) -> Mesh {
    sweep(&[a, b], &rect(w, h), true)
}

/// A round rod of radius `r` from `a` to `b`, `sides` round, capped.
pub fn rod(a: [f32; 3], b: [f32; 3], r: f32, sides: u32) -> Mesh {
    sweep(&[a, b], &circle(r, sides), true)
}

/// A post `w` square from the ground to `h` at (x, z).
pub fn post(x: f32, z: f32, w: f32, h: f32) -> Mesh {
    sh::block([w, h, w], [x, 0.0, z])
}

/// A box `size` whose centre is `c`.
pub fn cbox(size: [f32; 3], c: [f32; 3]) -> Mesh {
    sh::block(size, [c[0], c[1] - size[1] * 0.5, c[2]])
}

/// A box `size` (bottom centre at `at`) with its edges rounded by `r` (m),
/// `seg` steps across each face: corners pushed onto spheres.
pub fn rounded_box(size: [f32; 3], r: f32, seg: u32, at: [f32; 3]) -> Mesh {
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let r = r.clamp(0.0, h[0].min(h[1]).min(h[2]));
    let mut m = geo::segmented_box(size, seg.clamp(1, 24));
    for v in &mut m.vertices {
        let p = [v[0], v[1] - h[1], v[2]];
        // Bunch the grid towards the edges, where the rounding is.
        let p: [f32; 3] = std::array::from_fn(|k| {
            let u = p[k] / h[k].max(1e-6);
            let u = u.signum() * (1.0 - (1.0 - u.abs()).powf(1.6));
            u * h[k]
        });
        let inner: [f32; 3] = std::array::from_fn(|k| p[k].clamp(-(h[k] - r), h[k] - r));
        let d = sub(p, inner);
        let nrm = if len(d) > 1e-6 { normalize(d) } else { [v[3], v[4], v[5]] };
        let q = if r > 0.0 && len(d) > 1e-6 { add(inner, mul(nrm, r)) } else { p };
        *v = [q[0] + at[0], q[1] + h[1] + at[1], q[2] + at[2], nrm[0], nrm[1], nrm[2]];
    }
    m
}

/// A gable roof over a `w` (x) by `d` (z) footprint whose eaves sit at
/// height `y`: ridge along x, `h` high, overhanging by `o`, as a solid
/// prism.
pub fn gable(w: f32, d: f32, h: f32, y: f32, o: f32) -> Mesh {
    let (hd, slope) = (d * 0.5 + o, h / (d * 0.5).max(1e-3));
    let mut m = extrude_x(&[[-hd, -o * slope], [hd, -o * slope], [0.0, h]], w + o * 2.0);
    geo::transform(&mut m, [1.0; 3], [0.0, y, 0.0]);
    m
}

/// A hip (pyramid) roof over `w` by `d` from height `y`, `h` high,
/// overhanging by `o`.
pub fn hip(w: f32, d: f32, h: f32, y: f32, o: f32) -> Mesh {
    let mut m = sh::frustum(std::f32::consts::SQRT_2 * 0.5, 0.0, 1.0, 4);
    sh::rotate(&mut m, [0.0, 1.0, 0.0], TAU / 8.0);
    geo::transform(&mut m, [w + o * 2.0, h, d + o * 2.0], [0.0, y, 0.0]);
    sh::flat_shade(&mut m);
    m
}

/// Window panes on all four walls of a `w` (x) by `d` (z) block: `floors`
/// rows from `y0`, each `fh` high, `per_w` panes along x (and as many
/// along z as fit), each `pane` (width, height), 3 cm proud of the wall.
pub fn windows(w: f32, d: f32, y0: f32, floors: u32, fh: f32, per_w: u32, pane: [f32; 2]) -> Mesh {
    let (pw, ph) = (pane[0], pane[1]);
    let mut m = Mesh::default();
    let per_d = ((per_w as f32) * d / w.max(1e-3)).round().max(1.0) as u32;
    let t = 0.06;
    for f in 0..floors {
        let cy = y0 + (f as f32 + 0.55) * fh;
        for (len, n, k) in [(w, per_w, 0u32), (d, per_d, 1)] {
            for i in 0..n {
                let s = ((i as f32 + 0.5) / n as f32 - 0.5) * len;
                for side in [-1.0f32, 1.0] {
                    let c = if k == 0 { [s, cy, side * d * 0.5] } else { [side * w * 0.5, cy, s] };
                    let size = if k == 0 { [pw, ph, t] } else { [t, ph, pw] };
                    geo::merge(&mut m, &cbox(size, c));
                }
            }
        }
    }
    m
}

/// A straight flight of `n` steps rising `rise` each and `run` deep, `w`
/// wide, climbing along +z from z = 0: solid steps down to the ground.
pub fn stairs(n: u32, w: f32, rise: f32, run: f32) -> Mesh {
    let mut m = Mesh::default();
    for i in 0..n.max(1) {
        let h = rise * (i + 1) as f32;
        geo::merge(&mut m, &sh::block([w, h, run], [0.0, 0.0, (i as f32 + 0.5) * run]));
    }
    m
}

/// An S-curve path along x, `length` long, `n` points, bending
/// sideways (z) by up to `curve` / 2 (m) each way, lifted to `y`.
pub fn s_path(length: f32, curve: f32, y: f32, n: u32) -> Vec<[f32; 3]> {
    let n = n.max(2);
    (0..n).map(|i| {
        let t = i as f32 / (n - 1) as f32;
        [(t - 0.5) * length, y, curve * 0.5 * (t * TAU).sin()]
    }).collect()
}

/// The point at fraction `t` (0..1) of `path`'s length, with its heading
/// (the yaw for [`crate::shapes::place`] that turns +x along the path)
/// and its right (the horizontal across it).
pub fn along(path: &[[f32; 3]], t: f32) -> ([f32; 3], f32, [f32; 3]) {
    let total: f32 = path.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
    let mut goal = t.clamp(0.0, 1.0) * total;
    let last = path.len().saturating_sub(2);
    for (i, w) in path.windows(2).enumerate() {
        let l = len(sub(w[1], w[0]));
        if goal <= l || i == last {
            let k = (goal / l.max(1e-6)).min(1.0);
            let p = add(w[0], mul(sub(w[1], w[0]), k));
            let d = normalize(sub(w[1], w[0]));
            let right = normalize(cross(d, [0.0, 1.0, 0.0]));
            return (p, (-d[2]).atan2(d[0]), right);
        }
        goal -= l;
    }
    (path.first().copied().unwrap_or([0.0; 3]), 0.0, [0.0, 0.0, 1.0])
}

/// The path's length (m).
pub fn path_len(path: &[[f32; 3]]) -> f32 {
    path.windows(2).map(|w| len(sub(w[1], w[0]))).sum()
}

/// A rope or wire hanging between `a` and `b`, sagging `sag` (m) at its
/// middle: `n` points.
pub fn catenary(a: [f32; 3], b: [f32; 3], sag: f32, n: u32) -> Vec<[f32; 3]> {
    let n = n.max(2);
    (0..n).map(|i| {
        let t = i as f32 / (n - 1) as f32;
        let p = add(a, mul(sub(b, a), t));
        [p[0], p[1] - sag * 4.0 * t * (1.0 - t), p[2]]
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_l_shape_is_cut_into_four_triangles() {
        let l = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
        assert_eq!(triangulate(&l).len(), 4);
        let m = prism(&l, 0.0, 1.0);
        let (lo, hi) = geo::bounds(&m);
        assert_eq!((lo[1], hi[1]), (0.0, 1.0));
        assert_eq!(m.triangles(), 4 * 2 + 6 * 2);
    }

    #[test]
    fn a_bar_and_a_rod_reach_their_ends() {
        for m in [bar([0.0, 0.0, 0.0], [0.0, 2.0, 0.0], 0.1, 0.1), rod([0.0, 1.0, 0.0], [3.0, 1.0, 0.0], 0.05, 8)] {
            let (lo, hi) = geo::bounds(&m);
            assert!(hi[0] - lo[0] > 0.09 && hi[1] - lo[1] > 0.09, "{lo:?} {hi:?}");
        }
    }

    #[test]
    fn a_revolved_cup_and_a_rounded_box_keep_their_size() {
        let cup = revolve(&[[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [0.45, 1.0], [0.45, 0.05], [0.0, 0.05]], 24);
        let (lo, hi) = geo::bounds(&cup);
        assert!((hi[0] - 0.5).abs() < 1e-3 && (lo[1]).abs() < 1e-6 && (hi[1] - 1.0).abs() < 1e-6);
        let b = rounded_box([1.0, 2.0, 1.0], 0.2, 6, [0.0; 3]);
        let (lo, hi) = geo::bounds(&b);
        assert!((hi[1] - 2.0).abs() < 1e-3 && lo[1].abs() < 1e-3 && (hi[0] - 0.5).abs() < 1e-3);
    }
}
