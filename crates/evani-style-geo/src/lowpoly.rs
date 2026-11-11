//! Low-poly and papercraft geometry (WP3, docs/studio/WORK_PACKAGES.md).
//! Our own code; the idea is quadric edge collapse (the classic error
//! quadrics, no library):
//!
//! 1. The source is welded by position into one mesh; each face keeps its
//!    region.
//! 2. Every vertex sums the area-weighted plane quadrics of its faces (thin
//!    regions weighted [`THIN_WEIGHT`] times, so legs and ears keep their
//!    shape longest), plus a plane square to every region border and open
//!    edge, so borders keep their line.
//! 3. Edges collapse cheapest first (one end onto the other, so no new
//!    points are invented) until about `target_tris` remain, never: moving a
//!    locked vertex (each piece of each region keeps the two ends of its
//!    long axis, so a leg keeps its hoof end and an ear its tip), moving a
//!    border vertex off its border, breaking the surface (the link test),
//!    flipping a face, or thinning a region piece (an eye) below
//!    [`MIN_PIECE_FACES`].
//! 4. Out: one part per region; `facets` gives every triangle its own
//!    vertices and face normal (the faceted look), else smooth normals.
//!
//! Papercraft is the same simplification, then faces gather into flat
//! panels (neighbours in one region within [`PANEL_ANGLE`] of the panel's
//! first face and `cluster` of its centre) that share one normal: paper
//! sheets with sharp folds between them.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use evani_studio::assets::AssetRow;
use evani_studio::geo::{GeometryGenerator, SourceMesh, StyledMesh, StyledPart};
use evani_studio::styles::{Geometry, GeometryKind};

use crate::math::{V3, add, cross, dot, len, normalize, scale, sub};
use crate::voxel::tri_region;

/// Candidate collapses, cheapest first: (cost bits, from, to, from's
/// version, to's version); a stale version drops the entry when it surfaces.
type CollapseHeap = BinaryHeap<Reverse<(u64, u32, u32, u32, u32)>>;

/// How much more a thin region's surface error counts.
pub const THIN_WEIGHT: f64 = 6.0;
/// How strongly region borders and open edges hold their line.
pub const BORDER_WEIGHT: f64 = 4.0;
/// A collapse may turn no face more than this far (cosine).
const FLIP_COS: f32 = 0.15;
/// The fewest faces a region piece keeps (an eye stays an eye).
pub const MIN_PIECE_FACES: usize = 8;
/// Papercraft: faces within this angle of a panel's first face join it.
pub const PANEL_ANGLE: f32 = 30.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct LowPoly;

impl GeometryGenerator for LowPoly {
    fn kind(&self) -> GeometryKind {
        GeometryKind::Lowpoly
    }

    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String> {
        let Geometry::Lowpoly { target_tris, facets, .. } = *geometry else {
            return Err(format!("low-poly can't build `{}`", geometry.kind().name()));
        };
        src.validate(asset.regions.len())?;
        let out = simplify(src, asset, target_tris as usize).to_mesh(facets, None);
        out.validate(asset.regions.len())?;
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Papercraft;

impl GeometryGenerator for Papercraft {
    fn kind(&self) -> GeometryKind {
        GeometryKind::Papercraft
    }

    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String> {
        let Geometry::Papercraft { cluster, target_tris } = *geometry else {
            return Err(format!("papercraft can't build `{}`", geometry.kind().name()));
        };
        src.validate(asset.regions.len())?;
        let out = simplify(src, asset, target_tris as usize).to_mesh(true, Some(cluster));
        out.validate(asset.regions.len())?;
        Ok(out)
    }
}

/// A simplified mesh: welded positions, faces with their regions.
#[derive(Debug, Clone, PartialEq)]
pub struct Simplified {
    pub positions: Vec<V3>,
    pub faces: Vec<[u32; 3]>,
    pub regions: Vec<u8>,
}

impl Simplified {
    /// The shipped form (a few KB): magic `EVL1`, the bounds (6 f32 LE),
    /// vertex and face counts (u16 LE), positions quantised to u16 within
    /// the bounds, faces (3 u16 each), then a region byte per face. The
    /// viewer re-facets it on load ([`Simplified::to_mesh`]).
    pub fn encode(&self) -> Vec<u8> {
        let (lo, hi) = crate::math::bounds(&self.positions);
        let mut out = b"EVL1".to_vec();
        for v in lo.iter().chain(&hi) {
            out.extend(v.to_le_bytes());
        }
        out.extend((self.positions.len().min(65535) as u16).to_le_bytes());
        out.extend((self.faces.len().min(65535) as u16).to_le_bytes());
        for p in &self.positions {
            for k in 0..3 {
                let span = (hi[k] - lo[k]).max(1e-9);
                out.extend((((p[k] - lo[k]) / span * 65535.0).round().clamp(0.0, 65535.0) as u16).to_le_bytes());
            }
        }
        for f in &self.faces {
            for &v in f {
                out.extend((v.min(65535) as u16).to_le_bytes());
            }
        }
        out.extend(&self.regions);
        out
    }

    /// Read [`Simplified::encode`]'s bytes back.
    pub fn decode(b: &[u8]) -> Result<Self, String> {
        let bad = || "low-poly bake: bad or truncated data".to_string();
        if b.get(0..4) != Some(b"EVL1") {
            return Err(bad());
        }
        let f = |o: usize| -> Result<f32, String> { Ok(f32::from_le_bytes(b.get(o..o + 4).ok_or_else(bad)?.try_into().map_err(|_| bad())?)) };
        let u = |o: usize| -> Result<u16, String> { Ok(u16::from_le_bytes(b.get(o..o + 2).ok_or_else(bad)?.try_into().map_err(|_| bad())?)) };
        let lo = [f(4)?, f(8)?, f(12)?];
        let hi = [f(16)?, f(20)?, f(24)?];
        let (nv, nf) = (usize::from(u(28)?), usize::from(u(30)?));
        let mut o = 32;
        let mut positions = Vec::with_capacity(nv);
        for _ in 0..nv {
            let mut p = [0.0; 3];
            for k in 0..3 {
                p[k] = lo[k] + f32::from(u(o)?) / 65535.0 * (hi[k] - lo[k]);
                o += 2;
            }
            positions.push(p);
        }
        let mut faces = Vec::with_capacity(nf);
        for _ in 0..nf {
            let face = [u32::from(u(o)?), u32::from(u(o + 2)?), u32::from(u(o + 4)?)];
            if face.iter().any(|&v| v as usize >= nv) {
                return Err(bad());
            }
            faces.push(face);
            o += 6;
        }
        let regions = b.get(o..o + nf).ok_or_else(bad)?.to_vec();
        if b.len() != o + nf {
            return Err(bad());
        }
        Ok(Self { positions, faces, regions })
    }

    /// The drawn mesh: flat facets (`facets`), smooth, or papercraft panels
    /// (`panels`: the `cluster` reach).
    pub fn to_mesh(&self, facets: bool, panels: Option<f32>) -> StyledMesh {
        match panels {
            Some(c) => self.faceted(Some(&self.panels(c))),
            None if facets => self.faceted(None),
            None => self.smooth(),
        }
    }

    fn normal(&self, f: usize) -> V3 {
        let [a, b, c] = self.faces[f].map(|v| self.positions[v as usize]);
        normalize(cross(sub(b, a), sub(c, a)), [0.0, 1.0, 0.0])
    }

    fn centroid(&self, f: usize) -> V3 {
        let [a, b, c] = self.faces[f].map(|v| self.positions[v as usize]);
        scale(add(add(a, b), c), 1.0 / 3.0)
    }

    /// Un-indexed triangles, one part per region (in region order), each
    /// with its face normal, or its panel's normal when given.
    fn faceted(&self, panel_normal: Option<&[V3]>) -> StyledMesh {
        let mut parts: Vec<StyledPart> = Vec::new();
        let mut order: Vec<u8> = self.regions.clone();
        order.sort_unstable();
        order.dedup();
        for r in order {
            let mut p = StyledPart { region: r, ..StyledPart::default() };
            for (f, face) in self.faces.iter().enumerate() {
                if self.regions[f] != r {
                    continue;
                }
                let n = panel_normal.map_or_else(|| self.normal(f), |pn| pn[f]);
                for &v in face {
                    p.indices.push(p.positions.len() as u32);
                    p.positions.push(self.positions[v as usize]);
                    p.normals.push(n);
                }
            }
            parts.push(p);
        }
        StyledMesh { parts }
    }

    /// Indexed parts with area-weighted smooth normals (per part, so region
    /// borders stay crisp).
    fn smooth(&self) -> StyledMesh {
        let mut parts: Vec<StyledPart> = Vec::new();
        let mut order: Vec<u8> = self.regions.clone();
        order.sort_unstable();
        order.dedup();
        for r in order {
            let mut p = StyledPart { region: r, ..StyledPart::default() };
            let mut local = vec![u32::MAX; self.positions.len()];
            let mut acc: Vec<V3> = Vec::new();
            for (f, face) in self.faces.iter().enumerate() {
                if self.regions[f] != r {
                    continue;
                }
                let [a, b, c] = face.map(|v| self.positions[v as usize]);
                let an = cross(sub(b, a), sub(c, a));
                for &v in face {
                    let v = v as usize;
                    if local[v] == u32::MAX {
                        local[v] = p.positions.len() as u32;
                        p.positions.push(self.positions[v]);
                        acc.push([0.0; 3]);
                    }
                    let l = local[v] as usize;
                    acc[l] = add(acc[l], an);
                    p.indices.push(local[v]);
                }
            }
            p.normals = acc.iter().map(|n| normalize(*n, [0.0, 1.0, 0.0])).collect();
            parts.push(p);
        }
        StyledMesh { parts }
    }

    /// Papercraft panels: each face's shared panel normal.
    fn panels(&self, cluster: f32) -> Vec<V3> {
        let nf = self.faces.len();
        let mut by_vertex: Vec<Vec<u32>> = vec![Vec::new(); self.positions.len()];
        for (f, face) in self.faces.iter().enumerate() {
            for &v in face {
                by_vertex[v as usize].push(f as u32);
            }
        }
        let cos = PANEL_ANGLE.to_radians().cos();
        let reach = if cluster > 0.0 { cluster } else { f32::MAX };
        let mut panel = vec![u32::MAX; nf];
        let mut normals = vec![[0.0f32; 3]; nf];
        let mut next = 0u32;
        for seed in 0..nf {
            if panel[seed] != u32::MAX {
                continue;
            }
            let (sn, sc, sr) = (self.normal(seed), self.centroid(seed), self.regions[seed]);
            let mut members = vec![seed];
            panel[seed] = next;
            let mut k = 0;
            while k < members.len() {
                let f = members[k];
                k += 1;
                // Neighbours across an edge (two shared corners).
                for &v in &self.faces[f] {
                    for &g in &by_vertex[v as usize] {
                        let g = g as usize;
                        if panel[g] != u32::MAX || self.regions[g] != sr {
                            continue;
                        }
                        let shared = self.faces[g].iter().filter(|x| self.faces[f].contains(x)).count();
                        if shared < 2 || dot(self.normal(g), sn) < cos || len(sub(self.centroid(g), sc)) > reach {
                            continue;
                        }
                        panel[g] = next;
                        members.push(g);
                    }
                }
            }
            let mut n = [0.0; 3];
            for &f in &members {
                let [a, b, c] = self.faces[f].map(|v| self.positions[v as usize]);
                n = add(n, cross(sub(b, a), sub(c, a)));
            }
            let n = normalize(n, sn);
            for &f in &members {
                normals[f] = n;
            }
            next += 1;
        }
        normals
    }
}

/// A symmetric 4x4 error quadric (10 terms), in f64.
#[derive(Debug, Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    fn plane(n: V3, d: f32, w: f64) -> Self {
        let [a, b, c] = n.map(f64::from);
        let d = f64::from(d);
        Quadric([a * a * w, a * b * w, a * c * w, a * d * w, b * b * w, b * c * w, b * d * w, c * c * w, c * d * w, d * d * w])
    }

    fn add(&mut self, o: &Quadric) {
        for (a, b) in self.0.iter_mut().zip(o.0) {
            *a += b;
        }
    }

    fn error(&self, p: V3) -> f64 {
        let [x, y, z] = p.map(f64::from);
        let q = &self.0;
        (q[0] * x * x + 2.0 * q[1] * x * y + 2.0 * q[2] * x * z + 2.0 * q[3] * x + q[4] * y * y + 2.0 * q[5] * y * z + 2.0 * q[6] * y + q[7] * z * z + 2.0 * q[8] * z + q[9]).max(0.0)
    }
}

/// Simplify `src` to about `target` triangles (module docs).
pub fn simplify(src: &SourceMesh, asset: &AssetRow, target: usize) -> Simplified {
    // 1. Weld by position (sorted keys: deterministic).
    let mut keys: Vec<([i32; 3], u32)> = src.positions.iter().enumerate().map(|(i, p)| (p.map(|x| (x * 1e5).round() as i32), i as u32)).collect();
    keys.sort_unstable();
    let mut weld = vec![0u32; src.positions.len()];
    let mut positions: Vec<V3> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if i == 0 || keys[i - 1].0 != k.0 {
            positions.push(src.positions[k.1 as usize]);
        }
        weld[k.1 as usize] = positions.len() as u32 - 1;
    }
    let nv = positions.len();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    let mut regions: Vec<u8> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for t in src.indices.as_chunks::<3>().0 {
        let f = t.map(|v| weld[v as usize]);
        if f[0] == f[1] || f[1] == f[2] || f[0] == f[2] {
            continue;
        }
        let mut key = f;
        key.sort_unstable();
        if !seen.insert(key) {
            continue;
        }
        faces.push(f);
        regions.push(tri_region(src, *t));
    }
    let thin: Vec<bool> = asset.regions.iter().map(|r| r.thin).collect();
    // Per vertex: its faces, the regions round it, its quadric.
    let mut vf: Vec<Vec<u32>> = vec![Vec::new(); nv];
    let mut vreg = vec![0u32; nv];
    for (f, face) in faces.iter().enumerate() {
        for &v in face {
            vf[v as usize].push(f as u32);
            vreg[v as usize] |= 1 << regions[f];
        }
    }
    let mut q = vec![Quadric::default(); nv];
    for (f, face) in faces.iter().enumerate() {
        let [a, b, c] = face.map(|v| positions[v as usize]);
        let an = cross(sub(b, a), sub(c, a));
        let area = f64::from(len(an)) * 0.5;
        let n = normalize(an, [0.0, 1.0, 0.0]);
        let w = area * if thin.get(usize::from(regions[f])).copied().unwrap_or(false) { THIN_WEIGHT } else { 1.0 };
        let k = Quadric::plane(n, -dot(n, a), w);
        for &v in face {
            q[v as usize].add(&k);
        }
    }
    // Border and open edges: a plane square to the face through the edge.
    let mut edges: Vec<(u32, u32, u32)> = Vec::new();
    for (f, face) in faces.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (face[k], face[(k + 1) % 3]);
            edges.push((a.min(b), a.max(b), f as u32));
        }
    }
    edges.sort_unstable();
    let mut open = vec![false; nv];
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() && edges[j].0 == edges[i].0 && edges[j].1 == edges[i].1 {
            j += 1;
        }
        let group = &edges[i..j];
        let border = group.len() != 2 || regions[group[0].2 as usize] != regions[group[1].2 as usize];
        if border {
            let (a, b) = (group[0].0, group[0].1);
            if group.len() != 2 {
                open[a as usize] = true;
                open[b as usize] = true;
            }
            for &(_, _, f) in group {
                let fnorm = {
                    let [p, r, s] = faces[f as usize].map(|v| positions[v as usize]);
                    normalize(cross(sub(r, p), sub(s, p)), [0.0, 1.0, 0.0])
                };
                let (pa, pb) = (positions[a as usize], positions[b as usize]);
                let e = sub(pb, pa);
                let m = normalize(cross(e, fnorm), [1.0, 0.0, 0.0]);
                let k = Quadric::plane(m, -dot(m, pa), f64::from(dot(e, e)) * BORDER_WEIGHT);
                q[a as usize].add(&k);
                q[b as usize].add(&k);
            }
        }
        i = j;
    }
    // Locks: the two ends of each region piece's long axis, and the
    // whole mesh's extremes on each axis.
    let mut locked = vec![false; nv];
    let piece = lock_ends(&positions, &faces, &regions, &mut locked);
    let mut piece_live = vec![0usize; faces.len()];
    for &p in &piece {
        piece_live[p as usize] += 1;
    }
    let mut alive_f = vec![true; faces.len()];
    let mut alive_v = vec![true; nv];
    let mut version = vec![0u32; nv];
    let mut live = faces.len();
    let mut heap: CollapseHeap = BinaryHeap::new();
    let cost = |q: &[Quadric], from: u32, to: u32, positions: &[V3]| -> f64 {
        let mut s = q[from as usize];
        s.add(&q[to as usize]);
        s.error(positions[to as usize])
    };
    let push = |heap: &mut CollapseHeap, q: &[Quadric], positions: &[V3], version: &[u32], from: u32, to: u32| {
        let c = cost(q, from, to, positions);
        heap.push(Reverse((c.to_bits(), from, to, version[from as usize], version[to as usize])));
    };
    for &(a, b, _) in &edges {
        // Each direction once (edges repeat per face; the version check
        // drops the repeats when they surface).
        push(&mut heap, &q, &positions, &version, a, b);
        push(&mut heap, &q, &positions, &version, b, a);
    }
    while live > target {
        let Some(Reverse((_, from, to, vf_ver, vt_ver))) = heap.pop() else { break };
        let (fu, tu) = (from as usize, to as usize);
        if !alive_v[fu] || !alive_v[tu] || version[fu] != vf_ver || version[tu] != vt_ver {
            continue;
        }
        if locked[fu] || vreg[fu] & !vreg[tu] != 0 || (open[fu] && !open[tu]) {
            continue;
        }
        // The edge must still exist, and the link test: the corners the two
        // ends share are exactly the third corners of the faces on the edge.
        let on_edge: Vec<u32> = vf[fu].iter().copied().filter(|&f| alive_f[f as usize] && faces[f as usize].contains(&to)).collect();
        // Never undershoot the target (a collapse takes two faces, one on
        // an open edge): a budget of 300 means at least 300.
        if on_edge.is_empty() || live - on_edge.len() < target {
            continue;
        }
        let (rf, rt) = (ring(&vf, &alive_f, &faces, fu), ring(&vf, &alive_f, &faces, tu));
        let common = rf.iter().filter(|x| rt.binary_search(x).is_ok()).count();
        if common != on_edge.len() {
            continue;
        }
        // No region piece (an eye, an ear) may thin out below its floor.
        let starves = on_edge.iter().any(|&f| {
            let p = piece[f as usize] as usize;
            piece_live[p].saturating_sub(on_edge.iter().filter(|&&g| piece[g as usize] as usize == p).count()) < MIN_PIECE_FACES
        });
        if starves {
            continue;
        }
        // No face may flip or fold to nothing.
        let target_p = positions[tu];
        let flips = vf[fu].iter().any(|&f| {
            let f = f as usize;
            if !alive_f[f] || faces[f].contains(&to) {
                return false;
            }
            let [a, b, c] = faces[f].map(|v| positions[v as usize]);
            let old = cross(sub(b, a), sub(c, a));
            let moved = faces[f].map(|v| if v == from { target_p } else { positions[v as usize] });
            let new = cross(sub(moved[1], moved[0]), sub(moved[2], moved[0]));
            let (lo, ln) = (len(old), len(new));
            ln < 1e-9 || dot(old, new) < FLIP_COS * lo * ln
        });
        if flips {
            continue;
        }
        // Collapse `from` onto `to`.
        for &f in &on_edge {
            alive_f[f as usize] = false;
            piece_live[piece[f as usize] as usize] -= 1;
            live -= 1;
        }
        let moved: Vec<u32> = vf[fu].iter().copied().filter(|&f| alive_f[f as usize]).collect();
        for &f in &moved {
            for v in &mut faces[f as usize] {
                if *v == from {
                    *v = to;
                }
            }
        }
        vf[tu].extend(moved);
        vf[tu].retain(|&f| alive_f[f as usize]);
        vf[tu].sort_unstable();
        vf[tu].dedup();
        vf[fu].clear();
        alive_v[fu] = false;
        let qf = q[fu];
        q[tu].add(&qf);
        open[tu] |= open[fu];
        // Only the edges at `to` changed cost (its quadric grew): its new
        // version drops their old entries, the rest stay valid (and every
        // pop re-checks the link and flip tests anyway).
        version[tu] += 1;
        for n in ring(&vf, &alive_f, &faces, tu) {
            push(&mut heap, &q, &positions, &version, n, to);
            push(&mut heap, &q, &positions, &version, to, n);
        }
    }
    // Compact.
    let mut remap = vec![u32::MAX; nv];
    let mut out = Simplified { positions: Vec::new(), faces: Vec::new(), regions: Vec::new() };
    for (f, face) in faces.iter().enumerate() {
        if !alive_f[f] {
            continue;
        }
        let nf = face.map(|v| {
            let v = v as usize;
            if remap[v] == u32::MAX {
                remap[v] = out.positions.len() as u32;
                out.positions.push(positions[v]);
            }
            remap[v]
        });
        out.faces.push(nf);
        out.regions.push(regions[f]);
    }
    out
}

/// The live neighbours of vertex `v`, sorted.
fn ring(vf: &[Vec<u32>], alive_f: &[bool], faces: &[[u32; 3]], v: usize) -> Vec<u32> {
    let mut r: Vec<u32> = vf[v].iter().filter(|&&f| alive_f[f as usize]).flat_map(|&f| faces[f as usize]).filter(|&x| x as usize != v).collect();
    r.sort_unstable();
    r.dedup();
    r
}

/// Lock the ends of each region piece's long axis and the mesh's extremes;
/// returns each face's piece (the index of the piece's first face).
fn lock_ends(positions: &[V3], faces: &[[u32; 3]], regions: &[u8], locked: &mut [bool]) -> Vec<u32> {
    // Pieces: faces of one region connected through shared vertices.
    let nf = faces.len();
    let mut by_vertex: Vec<Vec<u32>> = vec![Vec::new(); positions.len()];
    for (f, face) in faces.iter().enumerate() {
        for &v in face {
            by_vertex[v as usize].push(f as u32);
        }
    }
    let mut piece = vec![u32::MAX; nf];
    for seed in 0..nf {
        if piece[seed] != u32::MAX {
            continue;
        }
        let r = regions[seed];
        piece[seed] = seed as u32;
        let mut members = vec![seed];
        let mut k = 0;
        while k < members.len() {
            let f = members[k];
            k += 1;
            for &v in &faces[f] {
                for &g in &by_vertex[v as usize] {
                    let g = g as usize;
                    if piece[g] == u32::MAX && regions[g] == r {
                        piece[g] = seed as u32;
                        members.push(g);
                    }
                }
            }
        }
        let verts: Vec<u32> = {
            let mut v: Vec<u32> = members.iter().flat_map(|&f| faces[f]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        if verts.len() < 4 {
            continue;
        }
        let pts: Vec<V3> = verts.iter().map(|&v| positions[v as usize]).collect();
        let axis = long_axis(&pts);
        let (mut lo, mut hi) = ((f32::MAX, 0u32), (f32::MIN, 0u32));
        for &v in &verts {
            let d = dot(positions[v as usize], axis);
            if d < lo.0 {
                lo = (d, v);
            }
            if d > hi.0 {
                hi = (d, v);
            }
        }
        locked[lo.1 as usize] = true;
        locked[hi.1 as usize] = true;
    }
    for k in 0..3 {
        let lo = (0..positions.len()).min_by(|&a, &b| positions[a][k].total_cmp(&positions[b][k]).then(a.cmp(&b)));
        let hi = (0..positions.len()).max_by(|&a, &b| positions[a][k].total_cmp(&positions[b][k]).then(b.cmp(&a)));
        for v in [lo, hi].into_iter().flatten() {
            locked[v] = true;
        }
    }
    piece
}

/// The principal (longest) axis of `pts`.
fn long_axis(pts: &[V3]) -> V3 {
    let n = pts.len() as f64;
    let mean = pts.iter().fold([0.0f64; 3], |m, p| [m[0] + f64::from(p[0]) / n, m[1] + f64::from(p[1]) / n, m[2] + f64::from(p[2]) / n]);
    let mut cov = [[0.0f64; 3]; 3];
    for p in pts {
        let d = [f64::from(p[0]) - mean[0], f64::from(p[1]) - mean[1], f64::from(p[2]) - mean[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j] / n;
            }
        }
    }
    crate::math::eigen_sym(cov).1[0].map(|x| x as f32)
}
