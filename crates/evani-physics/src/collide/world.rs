//! The static collision world: triangles (from meshes, boxes and oriented
//! boxes) with a user tag each, and a bounding volume hierarchy over them.
//!
//! The BVH is built once by median splits along the longest axis of the
//! triangle centres; ties sort by triangle index, so the same input always
//! gives the same tree. Queries walk it with an explicit stack and return
//! candidates in ascending triangle order.

use crate::collide::aabb::Aabb;
use crate::collide::closest::ray_triangle;
use crate::collide::math::{V3, add, cross, len_sq, mad, normalize, scale, sub};

/// Triangles per BVH leaf at most.
const LEAF: usize = 4;

/// One collision triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tri {
    /// Corners.
    pub p: [V3; 3],
    /// Unit face normal (`(b - a) x (c - a)`, right-handed winding).
    pub n: V3,
    /// The caller's tag (a material or brush id; returned with hits).
    pub tag: u32,
}

/// A BVH node: an inner node holds its two children, a leaf a run of
/// triangle indices in [`StaticWorld::order`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Node {
    bounds: Aabb,
    /// Leaf: first index into `order`; inner: index of the left child (the
    /// right child is `left + 1`).
    first: u32,
    /// Leaf: triangle count (> 0); inner: 0.
    count: u32,
}

/// A ray hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// Distance along the (unit) ray direction.
    pub t: f32,
    /// Hit point.
    pub point: V3,
    /// Face normal, turned to face the ray.
    pub normal: V3,
    /// Triangle index.
    pub tri: u32,
    /// The triangle's tag.
    pub tag: u32,
}

/// Builds a [`StaticWorld`].
#[derive(Debug, Default, Clone)]
pub struct WorldBuilder {
    tris: Vec<Tri>,
}

/// Errors building a world.
#[derive(Debug, Clone, PartialEq)]
pub struct CollideError(pub String);

impl core::fmt::Display for CollideError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CollideError {}

impl WorldBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one triangle. Degenerate (zero-area) and non-finite triangles
    /// are skipped; the return says whether it was added.
    pub fn triangle(&mut self, a: V3, b: V3, c: V3, tag: u32) -> bool {
        if !(a.iter().chain(b.iter()).chain(c.iter()).all(|v| v.is_finite())) {
            return false;
        }
        let Some(n) = normalize(cross(sub(b, a), sub(c, a))) else {
            return false;
        };
        self.tris.push(Tri { p: [a, b, c], n, tag });
        true
    }

    /// Add an indexed mesh (`indices` in threes). Fails on an index out of
    /// range or a count that is not a multiple of three.
    pub fn mesh(&mut self, positions: &[V3], indices: &[u32], tag: u32) -> Result<(), CollideError> {
        if !indices.len().is_multiple_of(3) {
            return Err(CollideError(format!("mesh index count {} is not a multiple of 3", indices.len())));
        }
        let get = |i: u32| positions.get(i as usize).copied().ok_or_else(|| CollideError(format!("mesh index {i} out of range")));
        for t in indices.as_chunks::<3>().0 {
            let (a, b, c) = (get(t[0])?, get(t[1])?, get(t[2])?);
            self.triangle(a, b, c, tag);
        }
        Ok(())
    }

    /// Add an axis-aligned box (12 outward-facing triangles).
    pub fn aabb(&mut self, min: V3, max: V3, tag: u32) {
        let c = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
        let h = [(max[0] - min[0]).abs() * 0.5, (max[1] - min[1]).abs() * 0.5, (max[2] - min[2]).abs() * 0.5];
        self.oriented_box(c, h, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], tag);
    }

    /// Add an oriented box: centre, half sizes along its three unit axes
    /// (a right-handed orthonormal frame), 12 outward-facing triangles.
    pub fn oriented_box(&mut self, centre: V3, half: V3, axes: [V3; 3], tag: u32) {
        let corner = |sx: f32, sy: f32, sz: f32| {
            let p = mad(centre, axes[0], sx * half[0]);
            let p = mad(p, axes[1], sy * half[1]);
            mad(p, axes[2], sz * half[2])
        };
        // Corners indexed by bits (x, y, z) = (1, 2, 4).
        let v: Vec<V3> = (0..8)
            .map(|i| {
                let s = |bit: usize| if i & bit != 0 { 1.0 } else { -1.0 };
                corner(s(1), s(2), s(4))
            })
            .collect();
        // Each face as a quad wound counter-clockwise seen from outside.
        const FACES: [[usize; 4]; 6] = [
            [1, 3, 7, 5], // +x
            [0, 4, 6, 2], // -x
            [2, 6, 7, 3], // +y
            [0, 1, 5, 4], // -y
            [4, 5, 7, 6], // +z
            [0, 2, 3, 1], // -z
        ];
        for f in FACES {
            self.triangle(v[f[0]], v[f[1]], v[f[2]], tag);
            self.triangle(v[f[0]], v[f[2]], v[f[3]], tag);
        }
    }

    /// Triangles added so far.
    pub fn len(&self) -> usize {
        self.tris.len()
    }

    /// No triangles yet.
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Build the world and its BVH.
    pub fn build(self) -> StaticWorld {
        StaticWorld::from_tris(self.tris)
    }
}

/// The static world: triangles and their BVH.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StaticWorld {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
    /// Triangle indices in leaf order.
    order: Vec<u32>,
}

impl StaticWorld {
    fn from_tris(tris: Vec<Tri>) -> Self {
        let mut w = StaticWorld { tris, nodes: Vec::new(), order: Vec::new() };
        if w.tris.is_empty() {
            return w;
        }
        let boxes: Vec<Aabb> = w.tris.iter().map(|t| Aabb::of_tri(t.p[0], t.p[1], t.p[2])).collect();
        let centres: Vec<V3> = boxes.iter().map(Aabb::centre).collect();
        w.order = (0..w.tris.len() as u32).collect();
        w.nodes.push(Node { bounds: Aabb::EMPTY, first: 0, count: 0 });
        // Explicit work stack of (node, start, end) over `order`.
        let mut work = vec![(0usize, 0usize, w.order.len())];
        while let Some((node, start, end)) = work.pop() {
            let bounds = w.order[start..end].iter().fold(Aabb::EMPTY, |b, &i| b.union(boxes[i as usize]));
            if end - start <= LEAF {
                w.nodes[node] = Node { bounds, first: start as u32, count: (end - start) as u32 };
                continue;
            }
            let cb = w.order[start..end].iter().fold(Aabb::EMPTY, |b, &i| {
                let c = centres[i as usize];
                b.union(Aabb { min: c, max: c })
            });
            let size = cb.size();
            let axis = if size[0] >= size[1] && size[0] >= size[2] {
                0
            } else if size[1] >= size[2] {
                1
            } else {
                2
            };
            w.order[start..end].sort_by(|&a, &b| centres[a as usize][axis].total_cmp(&centres[b as usize][axis]).then(a.cmp(&b)));
            let mid = start + (end - start) / 2;
            let left = w.nodes.len();
            w.nodes.push(Node { bounds: Aabb::EMPTY, first: 0, count: 0 });
            w.nodes.push(Node { bounds: Aabb::EMPTY, first: 0, count: 0 });
            w.nodes[node] = Node { bounds, first: left as u32, count: 0 };
            // Right pushed first so the left subtree is finished first (the
            // order only affects the build sequence, not the result).
            work.push((left + 1, mid, end));
            work.push((left, start, mid));
        }
        w
    }

    /// All triangles.
    pub fn tris(&self) -> &[Tri] {
        &self.tris
    }

    /// Triangle `i`.
    pub fn tri(&self, i: u32) -> Option<&Tri> {
        self.tris.get(i as usize)
    }

    /// BVH node count (0 for an empty world).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The bounds of everything (`Aabb::EMPTY` for an empty world).
    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map_or(Aabb::EMPTY, |n| n.bounds)
    }

    /// Indices of every triangle whose box overlaps `q`, ascending, into `out`
    /// (cleared first).
    pub fn query_aabb(&self, q: &Aabb, out: &mut Vec<u32>) {
        out.clear();
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let n = self.nodes[i as usize];
            if !n.bounds.overlaps(q) {
                continue;
            }
            if n.count > 0 {
                for &t in &self.order[n.first as usize..(n.first + n.count) as usize] {
                    let tri = &self.tris[t as usize];
                    if Aabb::of_tri(tri.p[0], tri.p[1], tri.p[2]).overlaps(q) {
                        out.push(t);
                    }
                }
            } else {
                stack.push(n.first + 1);
                stack.push(n.first);
            }
        }
        out.sort_unstable();
    }

    /// The nearest hit of the ray from `origin` along `dir` (any length; it
    /// is normalised) within `max_dist`. Equal distances resolve to the lower
    /// triangle index.
    pub fn raycast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<RayHit> {
        self.raycast_filtered(origin, dir, max_dist, |_| true)
    }

    /// [`StaticWorld::raycast`] that only considers triangles whose tag
    /// passes `keep` (glass a bullet passes, a team's own spawn wall...).
    pub fn raycast_filtered(&self, origin: V3, dir: V3, max_dist: f32, keep: impl Fn(u32) -> bool) -> Option<RayHit> {
        let d = normalize(dir)?;
        if self.nodes.is_empty() || max_dist.is_nan() || max_dist <= 0.0 {
            return None;
        }
        let mut best: Option<(f32, u32)> = None;
        let mut limit = max_dist;
        let mut stack: Vec<(u32, f32)> = vec![(0, 0.0)];
        while let Some((i, enter)) = stack.pop() {
            if enter > limit {
                continue;
            }
            let n = self.nodes[i as usize];
            if n.count > 0 {
                for &t in &self.order[n.first as usize..(n.first + n.count) as usize] {
                    let tri = &self.tris[t as usize];
                    if !keep(tri.tag) {
                        continue;
                    }
                    if let Some(h) = ray_triangle(origin, d, limit, tri.p[0], tri.p[1], tri.p[2]) {
                        let better = match best {
                            None => true,
                            Some((bt, bi)) => h < bt || (h == bt && t < bi),
                        };
                        if better {
                            best = Some((h, t));
                            limit = h;
                        }
                    }
                }
                continue;
            }
            // Visit the nearer child first (pushed last).
            let (l, r) = (n.first, n.first + 1);
            let el = self.nodes[l as usize].bounds.ray_enter(origin, d, limit);
            let er = self.nodes[r as usize].bounds.ray_enter(origin, d, limit);
            match (el, er) {
                (Some(a), Some(b)) => {
                    if a <= b {
                        stack.push((r, b));
                        stack.push((l, a));
                    } else {
                        stack.push((l, a));
                        stack.push((r, b));
                    }
                }
                (Some(a), None) => stack.push((l, a)),
                (None, Some(b)) => stack.push((r, b)),
                (None, None) => {}
            }
        }
        let (t, i) = best?;
        let tri = &self.tris[i as usize];
        let normal = if crate::collide::math::dot(tri.n, d) > 0.0 { scale(tri.n, -1.0) } else { tri.n };
        Some(RayHit { t, point: add(origin, scale(d, t)), normal, tri: i, tag: tri.tag })
    }

    /// Whether nothing blocks the straight line from `a` to `b` (hitscan and
    /// sight checks).
    pub fn line_of_sight(&self, a: V3, b: V3) -> bool {
        let d = sub(b, a);
        let l = len_sq(d).sqrt();
        if l <= 1e-6 {
            return true;
        }
        self.raycast(a, d, l).is_none()
    }

    /// [`StaticWorld::line_of_sight`] that looks through every triangle whose
    /// tag fails `blocks` (sight through glass or a fence, say).
    pub fn line_of_sight_filtered(&self, a: V3, b: V3, blocks: impl Fn(u32) -> bool) -> bool {
        let d = sub(b, a);
        let l = len_sq(d).sqrt();
        if l <= 1e-6 {
            return true;
        }
        self.raycast_filtered(a, d, l, blocks).is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_world(n: i32) -> StaticWorld {
        let mut b = WorldBuilder::new();
        for i in 0..n {
            for j in 0..n {
                let (x, z) = (i as f32 * 3.0, j as f32 * 3.0);
                b.aabb([x, 0.0, z], [x + 1.0, 2.0, z + 1.0], (i * n + j) as u32);
            }
        }
        b.build()
    }

    #[test]
    fn box_has_twelve_outward_triangles() {
        let mut b = WorldBuilder::new();
        b.aabb([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0], 7);
        let w = b.build();
        assert_eq!(w.tris().len(), 12);
        for t in w.tris() {
            let c = scale(add(add(t.p[0], t.p[1]), t.p[2]), 1.0 / 3.0);
            // Outward: the normal points away from the box centre.
            assert!(crate::collide::math::dot(t.n, c) > 0.0, "{t:?}");
            assert_eq!(t.tag, 7);
        }
    }

    #[test]
    fn degenerate_and_bad_meshes() {
        let mut b = WorldBuilder::new();
        assert!(!b.triangle([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], 0));
        assert!(!b.triangle([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0));
        assert!(b.mesh(&[[0.0; 3]], &[0, 0], 0).is_err());
        assert!(b.mesh(&[[0.0; 3]], &[0, 0, 5], 0).is_err());
        assert!(b.is_empty());
        let w = b.build();
        assert!(w.raycast([0.0; 3], [1.0, 0.0, 0.0], 10.0).is_none());
        assert!(w.line_of_sight([0.0; 3], [5.0, 0.0, 0.0]));
    }

    #[test]
    fn raycast_matches_brute_force() {
        let w = grid_world(8);
        assert!(w.node_count() > 1);
        // A fan of rays from above and from the side: the BVH answer equals
        // testing every triangle.
        for k in 0..64 {
            let a = k as f32 * 0.1;
            let o = [-2.0, 1.0 + (k % 3) as f32 * 0.3, -2.0];
            let d = [crate::collide::trig::cos(a) + 1.2, -0.05, crate::collide::trig::sin(a) + 1.2];
            let got = w.raycast(o, d, 100.0);
            let dn = normalize(d).unwrap();
            let mut best: Option<(f32, u32)> = None;
            for (i, t) in w.tris().iter().enumerate() {
                if let Some(h) = ray_triangle(o, dn, 100.0, t.p[0], t.p[1], t.p[2])
                    && best.is_none_or(|(bt, bi)| h < bt || (h == bt && (i as u32) < bi))
                {
                    best = Some((h, i as u32));
                }
            }
            assert_eq!(got.map(|h| (h.t, h.tri)), best, "ray {k}");
        }
    }

    #[test]
    fn query_returns_sorted_overlaps() {
        let w = grid_world(5);
        let mut out = Vec::new();
        let q = Aabb::new([-0.5, 0.5, -0.5], [3.5, 1.0, 0.5]);
        w.query_aabb(&q, &mut out);
        // Exactly the triangles whose own box overlaps, as brute force finds.
        let brute: Vec<u32> =
            (0..w.tris().len() as u32).filter(|&i| w.tri(i).is_some_and(|t| Aabb::of_tri(t.p[0], t.p[1], t.p[2]).overlaps(&q))).collect();
        assert_eq!(out, brute);
        assert_eq!(out.len(), 10);
        assert!(out.windows(2).all(|p| p[0] < p[1]));
        let tags: Vec<u32> = out.iter().map(|&i| w.tri(i).unwrap().tag).collect();
        assert!(tags.contains(&0) && tags.contains(&5));
    }

    #[test]
    fn hit_normal_faces_the_ray_and_sight() {
        let mut b = WorldBuilder::new();
        b.aabb([2.0, 0.0, -1.0], [3.0, 3.0, 1.0], 1);
        let w = b.build();
        let h = w.raycast([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 50.0).unwrap();
        assert!((h.t - 2.0).abs() < 1e-6);
        assert_eq!(h.normal, [-1.0, 0.0, 0.0]);
        assert!(!w.line_of_sight([0.0, 1.0, 0.0], [5.0, 1.0, 0.0]));
        assert!(w.line_of_sight([0.0, 4.0, 0.0], [5.0, 4.0, 0.0]));
        // The filter skips the wall.
        assert!(w.raycast_filtered([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 50.0, |tag| tag != 1).is_none());
    }

    #[test]
    fn build_is_deterministic() {
        let a = grid_world(6);
        let b = grid_world(6);
        assert_eq!(a, b);
    }
}
