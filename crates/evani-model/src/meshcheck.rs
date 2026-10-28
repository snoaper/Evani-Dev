//! Our own mesh checker (decision D17, option A): plain arrays in, a sorted
//! list of issues out. Standard library only, no engine crates.
//!
//! Checks: degenerate and duplicate triangles, open and non-manifold edges,
//! inconsistent orientation (flipped neighbours), non-manifold vertices,
//! optional flipped normals against reference directions (the field's
//! gradient), inward-facing closed meshes, and self-intersections. Candidate
//! pairs for the intersection test come from a uniform spatial grid sized
//! from the mean edge length, so the cost grows with the triangle count, not
//! its square. Edges are gathered in sorted vectors (no hash maps), so the
//! report is identical on every machine.

use crate::vec::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Degenerate,
    Duplicate,
    OpenEdge,
    NonManifoldEdge,
    FlippedNeighbour,
    NonManifoldVertex,
    FlippedNormal,
    InsideOut,
    SelfIntersection,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Degenerate => "degenerate",
            Kind::Duplicate => "duplicate",
            Kind::OpenEdge => "open_edge",
            Kind::NonManifoldEdge => "non_manifold_edge",
            Kind::FlippedNeighbour => "flipped_neighbour",
            Kind::NonManifoldVertex => "non_manifold_vertex",
            Kind::FlippedNormal => "flipped_normal",
            Kind::InsideOut => "inside_out",
            Kind::SelfIntersection => "self_intersection",
        }
    }
}

/// One problem: its kind and the triangles (or, for vertices, the vertex) involved.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Issue {
    pub kind: Kind,
    pub items: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub triangles: usize,
    pub vertices: usize,
    /// Signed volume (positive for an outward-facing closed mesh).
    pub volume: f64,
    /// Connected pieces (by shared edges).
    pub components: usize,
}

impl Report {
    pub fn count(&self, k: Kind) -> usize {
        self.issues.iter().filter(|i| i.kind == k).count()
    }
    pub fn watertight(&self) -> bool {
        self.count(Kind::OpenEdge) == 0
    }
    pub fn manifold(&self) -> bool {
        self.count(Kind::NonManifoldEdge) == 0 && self.count(Kind::NonManifoldVertex) == 0
    }
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
    /// Counts per kind, in kind order.
    pub fn summary(&self) -> Vec<(&'static str, usize)> {
        let mut out: Vec<(Kind, usize)> = Vec::new();
        for i in &self.issues {
            match out.last_mut() {
                Some((k, n)) if *k == i.kind => *n += 1,
                _ => out.push((i.kind, 1)),
            }
        }
        out.into_iter().map(|(k, n)| (k.name(), n)).collect()
    }
}

pub struct Options {
    /// Per-vertex reference directions (for example the field gradient);
    /// a triangle facing against all three of its corners' is flagged as flipped.
    pub ref_normals: Option<Vec<V3>>,
    /// The mesh must be closed and outward-facing.
    pub expect_closed: bool,
    pub self_intersections: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { ref_normals: None, expect_closed: true, self_intersections: true }
    }
}

fn tri_normal(p: &[V3], t: [u32; 3]) -> V3 {
    let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
    cross(sub(b, a), sub(c, a))
}

pub fn check(pos: &[V3], tris: &[[u32; 3]], opt: &Options) -> Report {
    let mut issues = Vec::new();
    let nv = pos.len();
    let mut bbox = Aabb::EMPTY;
    for p in pos {
        bbox.add_point(*p);
    }
    let diag = if bbox.is_empty() { 1.0 } else { len(bbox.size()).max(1e-9) };
    let area_eps = diag * diag * 1e-14;

    // Degenerate and out-of-range triangles.
    let mut valid = vec![true; tris.len()];
    for (i, t) in tris.iter().enumerate() {
        let bad_index = t.iter().any(|&v| v as usize >= nv);
        if bad_index || t[0] == t[1] || t[1] == t[2] || t[0] == t[2] || len(tri_normal(pos, *t)) * 0.5 <= area_eps {
            issues.push(Issue { kind: Kind::Degenerate, items: vec![i as u32] });
            valid[i] = false;
        }
    }

    // Duplicates: same vertex set.
    let mut keyed: Vec<([u32; 3], u32)> = tris
        .iter()
        .enumerate()
        .filter(|(i, _)| valid[*i])
        .map(|(i, t)| {
            let mut k = *t;
            k.sort_unstable();
            (k, i as u32)
        })
        .collect();
    keyed.sort_unstable();
    for w in keyed.windows(2) {
        if w[0].0 == w[1].0 {
            issues.push(Issue { kind: Kind::Duplicate, items: vec![w[0].1, w[1].1] });
        }
    }

    // Edges: (lo, hi, triangle, forward?) sorted.
    let mut edges: Vec<(u32, u32, u32, bool)> = Vec::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        if !valid[i] {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.push((a.min(b), a.max(b), i as u32, a < b));
        }
    }
    edges.sort_unstable();
    // Union-find over triangles for components.
    let mut parent: Vec<u32> = (0..tris.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    let mut i = 0;
    while i < edges.len() {
        let mut j = i;
        while j < edges.len() && edges[j].0 == edges[i].0 && edges[j].1 == edges[i].1 {
            j += 1;
        }
        let group = &edges[i..j];
        match group.len() {
            1 => {
                if opt.expect_closed {
                    issues.push(Issue { kind: Kind::OpenEdge, items: vec![group[0].2] });
                }
            }
            2 => {
                if group[0].3 == group[1].3 {
                    issues.push(Issue { kind: Kind::FlippedNeighbour, items: vec![group[0].2, group[1].2] });
                }
            }
            _ => {
                let mut t: Vec<u32> = group.iter().map(|e| e.2).collect();
                t.sort_unstable();
                issues.push(Issue { kind: Kind::NonManifoldEdge, items: t });
            }
        }
        for e in &group[1..] {
            let (a, b) = (find(&mut parent, group[0].2), find(&mut parent, e.2));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
        i = j;
    }
    let mut roots: Vec<u32> = (0..tris.len() as u32).filter(|&t| valid[t as usize]).map(|t| find(&mut parent, t)).collect();
    roots.sort_unstable();
    roots.dedup();

    // Non-manifold vertices: the triangles around a vertex must form one fan.
    let mut corner: Vec<(u32, u32)> = Vec::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        if valid[i] {
            for v in t {
                corner.push((*v, i as u32));
            }
        }
    }
    corner.sort_unstable();
    let mut k = 0;
    while k < corner.len() {
        let mut m = k;
        while m < corner.len() && corner[m].0 == corner[k].0 {
            m += 1;
        }
        let v = corner[k].0;
        let fan: Vec<u32> = corner[k..m].iter().map(|c| c.1).collect();
        if fan.len() > 1 && fan_pieces(v, &fan, tris) > 1 {
            issues.push(Issue { kind: Kind::NonManifoldVertex, items: vec![v] });
        }
        k = m;
    }

    // Flipped against reference normals.
    if let Some(rn) = &opt.ref_normals {
        for (i, t) in tris.iter().enumerate() {
            if !valid[i] || rn.len() < nv {
                continue;
            }
            let n = tri_normal(pos, *t);
            // Flipped when it faces against every corner's reference: at a
            // sharp edge one corner's reference may belong to the other face.
            // A corner triangle across a sharp corner can be perpendicular
            // to all three, so only a clear reversal counts.
            let n = norm_or(n, [0.0; 3]);
            if t.iter().all(|&v| dot(n, norm_or(rn[v as usize], [0.0; 3])) < -0.2) {
                issues.push(Issue { kind: Kind::FlippedNormal, items: vec![i as u32] });
            }
        }
    }

    // Signed volume.
    let mut vol = 0.0;
    for (i, t) in tris.iter().enumerate() {
        if valid[i] {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            vol += dot(a, cross(b, c)) / 6.0;
        }
    }
    if opt.expect_closed && !tris.is_empty() && vol <= 0.0 {
        issues.push(Issue { kind: Kind::InsideOut, items: vec![] });
    }

    if opt.self_intersections {
        for (a, b) in self_intersections(pos, tris, &valid) {
            issues.push(Issue { kind: Kind::SelfIntersection, items: vec![a, b] });
        }
    }

    issues.sort();
    Report { issues, triangles: tris.len(), vertices: nv, volume: vol, components: roots.len() }
}

/// Number of edge-connected pieces in the fan of triangles around `v`.
fn fan_pieces(v: u32, fan: &[u32], tris: &[[u32; 3]]) -> usize {
    let n = fan.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    // Each fan triangle has two other vertices; triangles sharing one are joined.
    let mut others: Vec<(u32, usize)> = Vec::with_capacity(n * 2);
    for (i, &t) in fan.iter().enumerate() {
        for &w in &tris[t as usize] {
            if w != v {
                others.push((w, i));
            }
        }
    }
    others.sort_unstable();
    for w in others.windows(2) {
        if w[0].0 == w[1].0 {
            let (a, b) = (find(&mut parent, w[0].1), find(&mut parent, w[1].1));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut roots: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    roots.sort_unstable();
    roots.dedup();
    roots.len()
}

/// Pairs of triangles that cross each other (sorted, each pair once).
pub fn self_intersections(pos: &[V3], tris: &[[u32; 3]], valid: &[bool]) -> Vec<(u32, u32)> {
    let n = tris.len();
    if n < 2 {
        return Vec::new();
    }
    let mut boxes = Vec::with_capacity(n);
    let mut total = Aabb::EMPTY;
    let mut edge_sum = 0.0;
    let mut cnt = 0usize;
    for (i, t) in tris.iter().enumerate() {
        let mut b = Aabb::EMPTY;
        if valid[i] {
            for k in 0..3 {
                b.add_point(pos[t[k] as usize]);
                edge_sum += dist(pos[t[k] as usize], pos[t[(k + 1) % 3] as usize]);
                cnt += 1;
            }
            total = total.union(&b);
        }
        boxes.push(b);
    }
    if total.is_empty() {
        return Vec::new();
    }
    let cell = (edge_sum / cnt.max(1) as f64 * 2.0).max(1e-9);
    let size = total.size();
    let dims = [
        ((size[0] / cell) as usize + 1).min(512),
        ((size[1] / cell) as usize + 1).min(512),
        ((size[2] / cell) as usize + 1).min(512),
    ];
    let cs = [size[0] / dims[0] as f64, size[1] / dims[1] as f64, size[2] / dims[2] as f64];
    let idx = |p: f64, a: usize| -> usize {
        if cs[a] <= 0.0 {
            return 0;
        }
        (((p - total.min[a]) / cs[a]).floor().max(0.0) as usize).min(dims[a] - 1)
    };
    // (cell id, triangle) for every cell a triangle's box touches.
    let mut reg: Vec<(u64, u32)> = Vec::new();
    for (i, b) in boxes.iter().enumerate() {
        if !valid[i] {
            continue;
        }
        let lo = [idx(b.min[0], 0), idx(b.min[1], 1), idx(b.min[2], 2)];
        let hi = [idx(b.max[0], 0), idx(b.max[1], 1), idx(b.max[2], 2)];
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let id = ((z * dims[1] + y) * dims[0] + x) as u64;
                    reg.push((id, i as u32));
                }
            }
        }
    }
    reg.sort_unstable();
    let mut pairs = Vec::new();
    let mut s = 0;
    while s < reg.len() {
        let mut e = s;
        while e < reg.len() && reg[e].0 == reg[s].0 {
            e += 1;
        }
        let cellid = reg[s].0;
        for a in s..e {
            for b in (a + 1)..e {
                let (ta, tb) = (reg[a].1, reg[b].1);
                let (ba, bb) = (&boxes[ta as usize], &boxes[tb as usize]);
                if !ba.overlaps(bb) {
                    continue;
                }
                // Test each pair only in the cell holding the low corner of
                // the overlap of their boxes.
                let o = ba.intersect(bb);
                let home = ((idx(o.min[2], 2) * dims[1] + idx(o.min[1], 1)) * dims[0] + idx(o.min[0], 0)) as u64;
                if home != cellid {
                    continue;
                }
                if tris_cross(pos, tris[ta as usize], tris[tb as usize]) {
                    pairs.push((ta.min(tb), ta.max(tb)));
                }
            }
        }
        s = e;
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// Do two triangles cross, beyond any vertices or edge they share?
fn tris_cross(pos: &[V3], a: [u32; 3], b: [u32; 3]) -> bool {
    let shared: Vec<u32> = a.iter().copied().filter(|v| b.contains(v)).collect();
    let pa = [pos[a[0] as usize], pos[a[1] as usize], pos[a[2] as usize]];
    let pb = [pos[b[0] as usize], pos[b[1] as usize], pos[b[2] as usize]];
    match shared.len() {
        0 => tri_tri(pa, pb),
        1 => {
            // The edge of each triangle opposite the shared vertex must not
            // pierce the other triangle, and vice versa.
            let s = shared[0];
            let opp = |t: [u32; 3], p: [V3; 3]| -> (V3, V3) {
                let i = t.iter().position(|&v| v == s).unwrap_or(0);
                (p[(i + 1) % 3], p[(i + 2) % 3])
            };
            let (a1, a2) = opp(a, pa);
            let (b1, b2) = opp(b, pb);
            seg_tri(a1, a2, pb) || seg_tri(b1, b2, pa) || coplanar_fold(pa, pb)
        }
        2 => coplanar_fold(pa, pb),
        _ => false,
    }
}

/// Two triangles sharing an edge folded flat onto each other.
fn coplanar_fold(pa: [V3; 3], pb: [V3; 3]) -> bool {
    let na = norm_or(cross(sub(pa[1], pa[0]), sub(pa[2], pa[0])), [0.0; 3]);
    let nb = norm_or(cross(sub(pb[1], pb[0]), sub(pb[2], pb[0])), [0.0; 3]);
    dot(na, nb) < -0.9999
}

/// Segment p-q strictly crosses the triangle's interior.
fn seg_tri(p: V3, q: V3, t: [V3; 3]) -> bool {
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    let nl = len(n);
    if nl == 0.0 {
        return false;
    }
    let scale = nl.sqrt();
    let eps = 1e-12 * scale * scale;
    let dp = dot(n, sub(p, t[0]));
    let dq = dot(n, sub(q, t[0]));
    if (dp > eps && dq > eps) || (dp < -eps && dq < -eps) || (dp.abs() <= eps && dq.abs() <= eps) {
        return false;
    }
    let s = dp / (dp - dq);
    if s <= 1e-9 || s >= 1.0 - 1e-9 {
        return false;
    }
    let x = add(p, mul(sub(q, p), s));
    // Inside test with a small inward tolerance (touching edges are not crossings).
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        if dot(cross(sub(b, a), sub(x, a)), n) <= 1e-9 * nl * scale {
            return false;
        }
    }
    true
}

/// Triangle-triangle intersection by the interval method on the line where
/// the two planes meet; coplanar pairs by edge crossings and containment.
fn tri_tri(a: [V3; 3], b: [V3; 3]) -> bool {
    let nb = cross(sub(b[1], b[0]), sub(b[2], b[0]));
    let na = cross(sub(a[1], a[0]), sub(a[2], a[0]));
    let (lna, lnb) = (len(na), len(nb));
    if lna == 0.0 || lnb == 0.0 {
        return false;
    }
    let scale = lna.sqrt().max(lnb.sqrt());
    let eps = 1e-12 * scale * scale * scale;
    let da: [f64; 3] = core::array::from_fn(|i| dot(nb, sub(a[i], b[0])));
    let db: [f64; 3] = core::array::from_fn(|i| dot(na, sub(b[i], a[0])));
    let side = |d: &[f64; 3]| -> i32 {
        let pos = d.iter().all(|&x| x > eps);
        let neg = d.iter().all(|&x| x < -eps);
        if pos || neg { 1 } else if d.iter().all(|&x| x.abs() <= eps) { 0 } else { -1 }
    };
    let (sa, sb) = (side(&da), side(&db));
    if sa == 1 || sb == 1 {
        return false;
    }
    if sa == 0 || sb == 0 {
        return coplanar_overlap(a, b, na);
    }
    let dir = cross(na, nb);
    let ia = interval(a, da, dir);
    let ib = interval(b, db, dir);
    let tol = 1e-9 * scale;
    ia.0 < ib.1 - tol && ib.0 < ia.1 - tol
}

/// Where triangle `t` crosses the other plane, projected on `dir`.
fn interval(t: [V3; 3], d: [f64; 3], dir: V3) -> (f64, f64) {
    let mut pts = Vec::with_capacity(3);
    for k in 0..3 {
        let (i, j) = (k, (k + 1) % 3);
        if d[i] == 0.0 {
            pts.push(dot(t[i], dir));
        }
        if (d[i] > 0.0 && d[j] < 0.0) || (d[i] < 0.0 && d[j] > 0.0) {
            let s = d[i] / (d[i] - d[j]);
            pts.push(dot(add(t[i], mul(sub(t[j], t[i]), s)), dir));
        }
    }
    let lo = pts.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = pts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

fn coplanar_overlap(a: [V3; 3], b: [V3; 3], n: V3) -> bool {
    // Drop the axis where the normal is largest.
    let ax = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() {
        0
    } else if n[1].abs() >= n[2].abs() {
        1
    } else {
        2
    };
    let (u, v) = match ax {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let p2 = |p: V3| [p[u], p[v]];
    let a2 = [p2(a[0]), p2(a[1]), p2(a[2])];
    let b2 = [p2(b[0]), p2(b[1]), p2(b[2])];
    for i in 0..3 {
        for j in 0..3 {
            if seg2_cross(a2[i], a2[(i + 1) % 3], b2[j], b2[(j + 1) % 3]) {
                return true;
            }
        }
    }
    inside2(a2[0], b2) || inside2(b2[0], a2)
}

fn orient2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn seg2_cross(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> bool {
    let (d1, d2) = (orient2(r, s, p), orient2(r, s, q));
    let (d3, d4) = (orient2(p, q, r), orient2(p, q, s));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn inside2(p: [f64; 2], t: [[f64; 2]; 3]) -> bool {
    let (a, b, c) = (orient2(t[0], t[1], p), orient2(t[1], t[2], p), orient2(t[2], t[0], p));
    (a > 0.0 && b > 0.0 && c > 0.0) || (a < 0.0 && b < 0.0 && c < 0.0)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A closed, outward-facing box mesh.
    pub fn cube(c: V3, h: f64, base: u32) -> (Vec<V3>, Vec<[u32; 3]>) {
        let mut p = Vec::new();
        for i in 0..8 {
            let s = [if i & 1 == 0 { -h } else { h }, if i & 2 == 0 { -h } else { h }, if i & 4 == 0 { -h } else { h }];
            p.push(add(c, s));
        }
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut t = Vec::new();
        for q in quads {
            t.push([q[0] + base, q[1] + base, q[2] + base]);
            t.push([q[0] + base, q[2] + base, q[3] + base]);
        }
        (p, t)
    }

    fn kinds(r: &Report) -> Vec<&'static str> {
        r.summary().into_iter().map(|(k, _)| k).collect()
    }

    #[test]
    fn a_closed_cube_passes() {
        let (p, t) = cube([0.0; 3], 0.5, 0);
        let r = check(&p, &t, &Options::default());
        assert!(r.ok(), "{:?}", r.issues);
        assert!((r.volume - 1.0).abs() < 1e-12);
        assert_eq!(r.components, 1);
    }

    #[test]
    fn every_issue_kind_is_found() {
        let (p, t) = cube([0.0; 3], 0.5, 0);
        // Open: drop a triangle.
        let r = check(&p, &t[1..], &Options::default());
        assert!(kinds(&r).contains(&"open_edge"));
        // Flipped neighbour: reverse one triangle.
        let mut f = t.clone();
        f[0] = [f[0][0], f[0][2], f[0][1]];
        assert!(kinds(&check(&p, &f, &Options::default())).contains(&"flipped_neighbour"));
        // Inside out: reverse all.
        let inv: Vec<[u32; 3]> = t.iter().map(|x| [x[0], x[2], x[1]]).collect();
        assert!(kinds(&check(&p, &inv, &Options::default())).contains(&"inside_out"));
        // Duplicate and degenerate.
        let mut d = t.clone();
        d.push(t[3]);
        d.push([0, 0, 1]);
        let k = kinds(&check(&p, &d, &Options::default()));
        assert!(k.contains(&"duplicate") && k.contains(&"degenerate"));
        // Non-manifold edge: a fin on an existing edge.
        let mut pf = p.clone();
        pf.push([0.0, 2.0, 0.0]);
        let mut nm = t.clone();
        nm.push([0, 2, 8]);
        nm.push([2, 0, 8]);
        assert!(kinds(&check(&pf, &nm, &Options::default())).contains(&"non_manifold_edge"));
        // Non-manifold vertex: two cubes touching at one corner.
        let (p2, t2) = cube([1.0, 1.0, 1.0], 0.5, 8);
        let mut pp = p.clone();
        pp.extend(p2);
        let mut tt = t.clone();
        // Weld cube 2's corner 0 onto cube 1's corner 7.
        tt.extend(t2.iter().map(|x| x.map(|v| if v == 8 { 7 } else { v })));
        assert!(kinds(&check(&pp, &tt, &Options::default())).contains(&"non_manifold_vertex"));
        // Flipped normal against a reference.
        let refn: Vec<V3> = p.iter().map(|q| neg(*q)).collect();
        let r = check(&p, &t, &Options { ref_normals: Some(refn), ..Options::default() });
        assert_eq!(r.count(Kind::FlippedNormal), 12);
    }

    #[test]
    fn self_intersections_are_found_and_touching_is_not() {
        let (mut p, mut t) = cube([0.0; 3], 0.5, 0);
        let (p2, t2) = cube([0.6, 0.3, 0.2], 0.5, 8);
        p.extend(p2);
        t.extend(t2);
        let r = check(&p, &t, &Options::default());
        assert!(r.count(Kind::SelfIntersection) > 0);
        assert_eq!(r.components, 2);
        // Two separate cubes: no crossing.
        let (mut p, mut t) = cube([0.0; 3], 0.5, 0);
        let (p2, t2) = cube([1.2, 0.0, 0.0], 0.5, 8);
        p.extend(p2);
        t.extend(t2);
        assert!(check(&p, &t, &Options::default()).ok());
    }

    #[test]
    fn crossing_through_a_shared_vertex_is_found() {
        // Two triangles sharing vertex 0, the second piercing the first.
        let p = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.5, 0.5, -1.0], [0.5, 0.5, 1.0]];
        let t = vec![[0, 1, 2], [0, 3, 4]];
        let r = check(&p, &t, &Options { expect_closed: false, ..Options::default() });
        assert_eq!(r.count(Kind::SelfIntersection), 1);
    }

    #[test]
    fn grid_scales_to_many_triangles() {
        // 500 separate cubes in a row: no pairs, fast.
        let (mut p, mut t) = (Vec::new(), Vec::new());
        for i in 0..500 {
            let (pi, ti) = cube([i as f64 * 1.5, 0.0, 0.0], 0.5, (i * 8) as u32);
            p.extend(pi);
            t.extend(ti);
        }
        let r = check(&p, &t, &Options::default());
        assert!(r.ok());
        assert_eq!(r.components, 500);
    }
}
