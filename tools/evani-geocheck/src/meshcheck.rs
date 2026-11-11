//! Mesh validation for built-in, generated, converted and imported meshes:
//! degenerate and duplicate triangles, open and non-manifold edges,
//! inconsistent winding, inverted normals and self-intersections.

use std::collections::HashMap;

use evani_render::mesh::Mesh;

use crate::geom::*;

/// What [`validate`] found (open edges and self-intersections list at most
/// [`MAX_LISTED`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshReport {
    pub triangles: usize,
    /// Zero-area triangles (or ones repeating a vertex).
    pub degenerate: Vec<usize>,
    /// Pairs of triangles over the same three corners (either winding).
    pub duplicates: Vec<(usize, usize)>,
    /// Edges used by one triangle only (holes, cracks, borders).
    pub open_edges: usize,
    /// A few open edges, for the report.
    pub open_edge_list: Vec<(V3, V3)>,
    /// Open edges are this mesh's intended border (a plane, a ribbon).
    pub open_intended: bool,
    /// Edges shared by three or more triangles.
    pub non_manifold_edges: usize,
    /// Edges two triangles walk the same way (a flipped neighbour).
    pub flipped_edges: usize,
    /// Triangles whose winding disagrees with their vertex normals.
    pub inverted: Vec<usize>,
    /// Closed mesh with a negative volume: wound inside out.
    pub inside_out: bool,
    /// Pairs of triangles that cut through each other (not neighbours).
    pub self_intersections: Vec<(usize, usize)>,
    /// Where each kind of problem is, in the mesh's own space: the corners
    /// of the triangles and edges listed above (at most [`MAX_LISTED`] of
    /// each), so a report can frame the spot instead of the whole mesh.
    pub at: Spots,
}

/// The mesh-space points of each problem kind ([`MeshReport::at`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spots {
    pub degenerate: Vec<V3>,
    pub duplicates: Vec<V3>,
    pub non_manifold: Vec<V3>,
    pub flipped: Vec<V3>,
    pub inverted: Vec<V3>,
    pub self_intersections: Vec<V3>,
}

/// How far (m) from a problem's first point the others still count as the
/// same spot: a crop frames one place, not every place the problem occurs.
pub const SPOT_RADIUS: f64 = 0.03;

/// The box round `pts[0]` holding every point within [`SPOT_RADIUS`] of it.
pub fn spot_box(pts: &[V3]) -> Option<(V3, V3)> {
    let first = *pts.first()?;
    let near: Vec<V3> = pts.iter().copied().filter(|p| dist(*p, first) <= SPOT_RADIUS).collect();
    let b = Aabb::from_points(&near);
    Some((b.min, b.max))
}

pub const MAX_LISTED: usize = 64;

impl MeshReport {
    /// Problems in plain words (empty: the mesh is clean).
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if !self.degenerate.is_empty() {
            p.push(format!("{} degenerate (zero-area) triangles", self.degenerate.len()));
        }
        if !self.duplicates.is_empty() {
            p.push(format!("{} duplicate triangles", self.duplicates.len()));
        }
        if self.open_edges > 0 && !self.open_intended {
            p.push(format!("{} open edges (holes or cracks)", self.open_edges));
        }
        if self.non_manifold_edges > 0 {
            p.push(format!("{} non-manifold edges (shared by 3+ triangles)", self.non_manifold_edges));
        }
        if self.flipped_edges > 0 {
            p.push(format!("{} edges with a flipped neighbour (inconsistent winding)", self.flipped_edges));
        }
        if !self.inverted.is_empty() {
            p.push(format!("{} triangles wound against their normals (inverted)", self.inverted.len()));
        }
        if self.inside_out {
            p.push("the closed mesh is inside out (negative volume)".into());
        }
        if !self.self_intersections.is_empty() {
            p.push(format!("{} self-intersecting triangle pairs", self.self_intersections.len()));
        }
        p
    }

    /// Where each of [`Self::problems`] is (same order): a box in the
    /// mesh's own space round its first listed spot, `None` when the
    /// problem has no one place (an inside-out mesh).
    pub fn problem_spots(&self) -> Vec<Option<(V3, V3)>> {
        let mut p = Vec::new();
        if !self.degenerate.is_empty() {
            p.push(spot_box(&self.at.degenerate));
        }
        if !self.duplicates.is_empty() {
            p.push(spot_box(&self.at.duplicates));
        }
        if self.open_edges > 0 && !self.open_intended {
            let pts: Vec<V3> = self.open_edge_list.iter().flat_map(|e| [e.0, e.1]).collect();
            p.push(spot_box(&pts));
        }
        if self.non_manifold_edges > 0 {
            p.push(spot_box(&self.at.non_manifold));
        }
        if self.flipped_edges > 0 {
            p.push(spot_box(&self.at.flipped));
        }
        if !self.inverted.is_empty() {
            p.push(spot_box(&self.at.inverted));
        }
        if self.inside_out {
            p.push(None);
        }
        if !self.self_intersections.is_empty() {
            p.push(spot_box(&self.at.self_intersections));
        }
        p
    }

    pub fn is_clean(&self) -> bool {
        self.problems().is_empty()
    }
}

type Key = [i64; 3];

fn key(p: V3) -> Key {
    // 0.01 mm grid: welds split vertices (flat shading) by position.
    [(p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64]
}

/// Check `m`. `intended_open`: open edges are the mesh's border (a plane, a
/// road ribbon), not holes.
pub fn validate(m: &Mesh, intended_open: bool) -> MeshReport {
    let pos: Vec<V3> = m.vertices.iter().map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]).collect();
    let nrm: Vec<V3> = m.vertices.iter().map(|v| [f64::from(v[3]), f64::from(v[4]), f64::from(v[5])]).collect();
    let mut r = MeshReport {
        open_intended: intended_open,
        ..MeshReport::default()
    };
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for t in m.indices.as_chunks::<3>().0 {
        let t = [t[0] as usize, t[1] as usize, t[2] as usize];
        if t.iter().any(|&i| i >= pos.len()) {
            continue;
        }
        tris.push(t);
    }
    r.triangles = tris.len();
    let keys: Vec<[Key; 3]> = tris.iter().map(|t| [key(pos[t[0]]), key(pos[t[1]]), key(pos[t[2]])]).collect();
    let mut seen: HashMap<[Key; 3], usize> = HashMap::new();
    let mut directed: HashMap<(Key, Key), u32> = HashMap::new();
    let mut volume = 0.0;
    for (i, t) in tris.iter().enumerate() {
        let (a, b, c) = (pos[t[0]], pos[t[1]], pos[t[2]]);
        let k = keys[i];
        let n = cross(sub(b, a), sub(c, a));
        if len(n) < 1e-12 || k[0] == k[1] || k[1] == k[2] || k[0] == k[2] {
            r.degenerate.push(i);
            if r.at.degenerate.len() < MAX_LISTED * 3 {
                r.at.degenerate.extend([a, b, c]);
            }
            continue;
        }
        volume += dot(a, cross(b, c));
        let mut sorted = k;
        sorted.sort();
        match seen.get(&sorted) {
            Some(&j) => {
                r.duplicates.push((j, i));
                if r.at.duplicates.len() < MAX_LISTED * 3 {
                    r.at.duplicates.extend([a, b, c]);
                }
            }
            None => {
                seen.insert(sorted, i);
            }
        }
        let vn = add(add(nrm[t[0]], nrm[t[1]]), nrm[t[2]]);
        if len(vn) > 1e-9 && dot(vn, n) < 0.0 {
            r.inverted.push(i);
            if r.at.inverted.len() < MAX_LISTED * 3 {
                r.at.inverted.extend([a, b, c]);
            }
        }
        for (x, y) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            *directed.entry((x, y)).or_default() += 1;
        }
    }
    let mut undirected: HashMap<(Key, Key), (u32, u32)> = HashMap::new();
    for (&(x, y), &n) in &directed {
        let (lo, hi, fwd) = if x <= y { (x, y, true) } else { (y, x, false) };
        let e = undirected.entry((lo, hi)).or_default();
        if fwd {
            e.0 += n;
        } else {
            e.1 += n;
        }
    }
    let unkey = |k: Key| [k[0] as f64 * 1e-5, k[1] as f64 * 1e-5, k[2] as f64 * 1e-5];
    // Every edge of each kind, sorted by position so the listed ones (and
    // the first spot, the one a crop frames) are the same on every run:
    // the hash map's order is not.
    let (mut open, mut nonman, mut flipped) = (Vec::new(), Vec::new(), Vec::new());
    for (&(lo, hi), &(f, b)) in &undirected {
        let total = f + b;
        if total == 1 {
            open.push((lo, hi));
        } else if total > 2 {
            nonman.push((lo, hi));
        } else if f == 2 || b == 2 {
            flipped.push((lo, hi));
        }
    }
    for list in [&mut open, &mut nonman, &mut flipped] {
        list.sort_unstable();
    }
    r.open_edges = open.len();
    r.non_manifold_edges = nonman.len();
    r.flipped_edges = flipped.len();
    r.open_edge_list = open.iter().take(MAX_LISTED).map(|&(lo, hi)| (unkey(lo), unkey(hi))).collect();
    r.at.non_manifold = nonman.iter().take(MAX_LISTED).flat_map(|&(lo, hi)| [unkey(lo), unkey(hi)]).collect();
    r.at.flipped = flipped.iter().take(MAX_LISTED).flat_map(|&(lo, hi)| [unkey(lo), unkey(hi)]).collect();
    r.inside_out = r.open_edges == 0 && r.non_manifold_edges == 0 && r.triangles >= 4 && volume < -1e-12;
    r.self_intersections = self_intersections(&pos, &tris, &keys);
    for &(i, j) in &r.self_intersections {
        for t in [tris[i], tris[j]] {
            r.at.self_intersections.extend([pos[t[0]], pos[t[1]], pos[t[2]]]);
        }
    }
    r
}

/// Every edge of the mesh is used by at least two triangles (positions
/// welded): a closed surface, with an inside. Cheap (no self-intersection
/// test), for deciding whether a mesh has a volume.
pub fn is_closed(m: &Mesh) -> bool {
    let pos: Vec<V3> = m.vertices.iter().map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]).collect();
    let mut edges: HashMap<(Key, Key), u32> = HashMap::new();
    let mut any = false;
    for t in m.indices.as_chunks::<3>().0 {
        let (Some(&a), Some(&b), Some(&c)) = (pos.get(t[0] as usize), pos.get(t[1] as usize), pos.get(t[2] as usize)) else {
            continue;
        };
        let k = [key(a), key(b), key(c)];
        if len(cross(sub(b, a), sub(c, a))) < 1e-12 || k[0] == k[1] || k[1] == k[2] || k[0] == k[2] {
            continue;
        }
        any = true;
        for (x, y) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            *edges.entry(if x <= y { (x, y) } else { (y, x) }).or_default() += 1;
        }
    }
    any && edges.values().all(|&n| n >= 2)
}

/// Triangle pairs that cut through each other, skipping neighbours (pairs
/// sharing a corner). A sweep along x keeps it near-linear for real meshes.
fn self_intersections(pos: &[V3], tris: &[[usize; 3]], keys: &[[Key; 3]]) -> Vec<(usize, usize)> {
    let boxes: Vec<Aabb> = tris
        .iter()
        .map(|t| Aabb::from_points(&[pos[t[0]], pos[t[1]], pos[t[2]]]))
        .collect();
    let mut order: Vec<usize> = (0..tris.len()).collect();
    order.sort_by(|&a, &b| boxes[a].min[0].total_cmp(&boxes[b].min[0]));
    let mut out = Vec::new();
    for (oi, &i) in order.iter().enumerate() {
        for &j in &order[oi + 1..] {
            if boxes[j].min[0] > boxes[i].max[0] {
                break;
            }
            if !boxes[i].overlaps(&boxes[j], 0.0) || keys[i].iter().any(|k| keys[j].contains(k)) {
                continue;
            }
            let a = [pos[tris[i][0]], pos[tris[i][1]], pos[tris[i][2]]];
            let b = [pos[tris[j][0]], pos[tris[j][1]], pos[tris[j][2]]];
            if tri_tri(&a, &b) {
                out.push((i.min(j), i.max(j)));
                if out.len() >= MAX_LISTED {
                    return out;
                }
            }
        }
    }
    out
}

/// The open segment `p..q` crosses the inside of triangle `t` (touching an
/// edge or a corner doesn't count).
pub fn segment_hits_triangle(p: V3, q: V3, t: &[V3; 3]) -> bool {
    const E: f64 = 1e-9;
    let d = sub(q, p);
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-14 {
        return false;
    }
    let f = 1.0 / det;
    let s = sub(p, t[0]);
    let u = f * dot(s, h);
    if u <= E || u >= 1.0 - E {
        return false;
    }
    let qv = cross(s, e1);
    let v = f * dot(d, qv);
    if v <= E || u + v >= 1.0 - E {
        return false;
    }
    let tt = f * dot(e2, qv);
    tt > E && tt < 1.0 - E
}

/// Two triangles intersect: an edge of one crosses the other, or they are
/// coplanar and overlap with some area.
pub fn tri_tri(a: &[V3; 3], b: &[V3; 3]) -> bool {
    for (x, y) in [(0, 1), (1, 2), (2, 0)] {
        if segment_hits_triangle(a[x], a[y], b) || segment_hits_triangle(b[x], b[y], a) {
            return true;
        }
    }
    let na = norm(cross(sub(a[1], a[0]), sub(a[2], a[0])));
    let nb = norm(cross(sub(b[1], b[0]), sub(b[2], b[0])));
    if dot(na, nb).abs() > 1.0 - 1e-9 && (dot(na, b[0]) - dot(na, a[0])).abs() < 1e-7 {
        let (u, v) = plane_basis(na);
        let to2 = |t: &[V3; 3]| {
            let mut p: Vec<[f64; 2]> = t.iter().map(|x| [dot(*x, u), dot(*x, v)]).collect();
            if area_2d(&p) < 0.0 {
                p.reverse();
            }
            p
        };
        return area_2d(&clip_2d(&to2(a), &to2(b))) > 1e-10;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_render::mesh::{self, MeshId};

    #[test]
    fn builtins_are_clean() {
        // (The UV sphere used to emit 48 zero-area pole triangles; geocheck
        // found them and mesh::sphere now leaves them out.)
        for id in [MeshId::CUBE, MeshId::SPHERE, MeshId::BEVEL_CUBE, MeshId::CYLINDER] {
            let r = validate(&mesh::builtin(id).unwrap(), false);
            assert!(r.is_clean(), "{id:?}: {:?}", r.problems());
        }
        let plane = validate(&mesh::plane(), true);
        assert_eq!(plane.open_edges, 4);
        assert!(plane.is_clean(), "a plane's border is intended");
        assert!(!validate(&mesh::plane(), false).is_clean(), "unless the mesh should be closed");
    }

    /// Each problem comes with a box round its own spot, in step with
    /// `problems()`: a cube missing one triangle frames that hole, not the
    /// whole cube (critic round 2 of material-view: every crop was the same
    /// distant frame).
    #[test]
    fn problems_carry_their_spot() {
        let mut m = mesh::cube();
        let gone: Vec<u32> = m.indices.drain(0..3).collect();
        let r = validate(&m, false);
        let (p, spots) = (r.problems(), r.problem_spots());
        assert_eq!(p.len(), spots.len());
        let (lo, hi) = spots[0].expect("the hole has a place");
        // A spot (within SPOT_RADIUS of the first open edge), at a corner of
        // the missing triangle, not the 1 m cube.
        assert!((0..3).all(|k| hi[k] - lo[k] <= 2.0 * SPOT_RADIUS + 1e-6), "{lo:?}..{hi:?}");
        let inside = |i: &u32| {
            let v = m.vertices[*i as usize];
            let q = [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])];
            (0..3).all(|k| q[k] >= lo[k] - 1e-6 && q[k] <= hi[k] + 1e-6)
        };
        assert!(gone.iter().any(inside), "{lo:?}..{hi:?}");
        // Stable: the same mesh frames the same spot every run.
        assert_eq!(validate(&m, false).problem_spots(), spots);
    }

    #[test]
    fn finds_degenerate_duplicate_and_inverted() {
        let mut m = mesh::cube();
        // Degenerate: a triangle repeating a corner.
        m.indices.extend_from_slice(&[0, 0, 1]);
        // Duplicate: the first triangle again, reversed.
        m.indices.extend_from_slice(&[0, 2, 1]);
        let r = validate(&m, false);
        assert_eq!(r.degenerate.len(), 1);
        assert_eq!(r.duplicates.len(), 1);
        assert!(r.non_manifold_edges > 0, "the duplicate adds a third user to its edges");
        // Inverted: flip one face's two triangles (normals still point out).
        let mut m = mesh::cube();
        for t in m.indices.as_chunks_mut::<3>().0.iter_mut().take(2) {
            t.swap(1, 2);
        }
        let r = validate(&m, false);
        assert_eq!(r.inverted.len(), 2);
        assert!(r.flipped_edges > 0);
        // The whole cube inside out.
        let mut m = mesh::cube();
        for t in m.indices.as_chunks_mut::<3>().0.iter_mut() {
            t.swap(1, 2);
        }
        let r = validate(&m, false);
        assert!(r.inside_out);
        assert_eq!(r.inverted.len(), 12);
    }

    #[test]
    fn finds_holes_and_self_intersections() {
        let mut m = mesh::cube();
        m.indices.truncate(m.indices.len() - 6); // drop one face
        let r = validate(&m, false);
        assert_eq!(r.open_edges, 4);
        // Two crossing quads (an X): every triangle cuts the other quad.
        let quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], base: u32, m: &mut Mesh| {
            for p in [a, b, c, d] {
                m.vertices.push([p[0], p[1], p[2], 0.0, 0.0, 0.0]);
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        };
        let mut x = Mesh::default();
        quad([-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], 0, &mut x);
        quad([0.3, -1.0, -0.5], [0.3, 1.0, -0.5], [0.3, 1.0, 0.7], [0.3, -1.0, 0.7], 4, &mut x);
        let r = validate(&x, true);
        assert!(!r.self_intersections.is_empty());
        // Coplanar overlap counts too.
        let a = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 2.0]];
        let b = [[0.5, 0.0, 0.5], [3.0, 0.0, 0.5], [0.5, 0.0, 3.0]];
        assert!(tri_tri(&a, &b));
        let c = [[5.0, 0.0, 5.0], [6.0, 0.0, 5.0], [5.0, 0.0, 6.0]];
        assert!(!tri_tri(&a, &c));
    }
}
