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
            continue;
        }
        volume += dot(a, cross(b, c));
        let mut sorted = k;
        sorted.sort();
        match seen.get(&sorted) {
            Some(&j) => r.duplicates.push((j, i)),
            None => {
                seen.insert(sorted, i);
            }
        }
        let vn = add(add(nrm[t[0]], nrm[t[1]]), nrm[t[2]]);
        if len(vn) > 1e-9 && dot(vn, n) < 0.0 {
            r.inverted.push(i);
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
    for (&(lo, hi), &(f, b)) in &undirected {
        let total = f + b;
        if total == 1 {
            r.open_edges += 1;
            if r.open_edge_list.len() < MAX_LISTED {
                r.open_edge_list.push((unkey(lo), unkey(hi)));
            }
        } else if total > 2 {
            r.non_manifold_edges += 1;
        } else if f == 2 || b == 2 {
            r.flipped_edges += 1;
        }
    }
    r.inside_out = r.open_edges == 0 && r.non_manifold_edges == 0 && r.triangles >= 4 && volume < -1e-12;
    r.self_intersections = self_intersections(&pos, &tris, &keys);
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
