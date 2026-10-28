//! Placement that can't float or overlap: ray casts against drawn
//! instances, snap-to-ground, and footprint-aware placement that refuses a
//! prop where another already stands (or on a reserved road).
//!
//! ```ignore
//! let mut fp = Footprints::new(0.3);
//! fp.reserve_path(&centre_line_xz, 9.0);           // keep props off the road
//! for spot in candidates {
//!     let mut tree = tree_parts(spot);              // trunk + canopy instances
//!     if !fp.try_place(&tree) { continue; }         // would overlap: skip
//!     place::snap_parts(&ground, &mut tree, 0.05);  // rest on the ground, 5 cm in
//!     statics.extend(tree);
//! }
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::mesh::{self, BUILTIN_MESH_COUNT, Mesh, MeshId, MeshLibrary};
use crate::world::MeshInstance;

type V = [f32; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, s: f32) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 1e-20 { mul(a, 1.0 / l) } else { a }
}
/// Rotate `v` by the unit quaternion `q` (x, y, z, w).
fn rotate(q: [f32; 4], v: V) -> V {
    let u = [q[0], q[1], q[2]];
    let t = mul(cross(u, v), 2.0);
    add(add(v, mul(t, q[3])), cross(u, t))
}
fn conj(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

fn builtins() -> &'static [Mesh] {
    static B: OnceLock<Vec<Mesh>> = OnceLock::new();
    B.get_or_init(|| (0..BUILTIN_MESH_COUNT as u16).filter_map(|i| mesh::builtin(MeshId(i))).collect())
}

fn mesh_of(id: MeshId, lib: Option<&MeshLibrary>) -> Option<&Mesh> {
    if usize::from(id.0) < BUILTIN_MESH_COUNT {
        builtins().get(usize::from(id.0))
    } else {
        lib.and_then(|l| l.get(id))
    }
}

/// A ray hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Distance along the (unit) ray.
    pub t: f32,
    pub point: V,
    /// World normal of the face hit.
    pub normal: V,
    /// Index of the instance hit.
    pub index: usize,
}

/// Radius around an instance's position that holds all of its mesh.
fn bound_radius(m: &MeshInstance, mesh: &Mesh) -> f32 {
    let r = mesh
        .vertices
        .iter()
        .map(|v| (v[0] * m.scale[0]).powi(2) + (v[1] * m.scale[1]).powi(2) + (v[2] * m.scale[2]).powi(2))
        .fold(0.0f32, f32::max);
    r.sqrt()
}

/// First hit of the ray `origin + t * dir` (`0 <= t <= max`) against the
/// instances' triangles (exactly what is drawn; custom meshes from `lib`).
pub fn raycast(instances: &[MeshInstance], lib: Option<&MeshLibrary>, origin: V, dir: V, max: f32) -> Option<Hit> {
    let dir = norm(dir);
    let mut best: Option<Hit> = None;
    for (index, m) in instances.iter().enumerate() {
        let Some(mesh) = mesh_of(m.mesh, lib) else { continue };
        // Bounding sphere first.
        let r = bound_radius(m, mesh);
        let oc = sub(m.position, origin);
        let along = dot(oc, dir);
        let limit = best.map_or(max, |b| b.t);
        if along + r < 0.0 || along - r > limit || dot(oc, oc) - along * along > r * r {
            continue;
        }
        // The ray in the instance's local space: t is unchanged.
        let inv = conj(m.rotation);
        let s = m.scale.map(|x| if x.abs() < 1e-12 { 1e-12 } else { x });
        let lo = rotate(inv, sub(origin, m.position));
        let ld = rotate(inv, dir);
        let lo = [lo[0] / s[0], lo[1] / s[1], lo[2] / s[2]];
        let ld = [ld[0] / s[0], ld[1] / s[1], ld[2] / s[2]];
        for t in mesh.indices.as_chunks::<3>().0 {
            let p = |i: u32| {
                let v = mesh.vertices[i as usize];
                [v[0], v[1], v[2]]
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let Some(tt) = ray_triangle(lo, ld, a, b, c) else { continue };
            if tt < 0.0 || tt > best.map_or(max, |h| h.t) {
                continue;
            }
            // Local face normal -> world (inverse-transpose of the scale).
            let n = cross(sub(b, a), sub(c, a));
            let n = norm(rotate(m.rotation, [n[0] / s[0], n[1] / s[1], n[2] / s[2]]));
            best = Some(Hit {
                t: tt,
                point: add(origin, mul(dir, tt)),
                normal: n,
                index,
            });
        }
    }
    best
}

/// MÃƒÂ¶ller-Trumbore, both faces.
fn ray_triangle(o: V, d: V, a: V, b: V, c: V) -> Option<f32> {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if !(-1e-6..=1.0 + 1e-6).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < -1e-6 || u + v > 1.0 + 1e-6 {
        return None;
    }
    Some(dot(e2, q) * inv)
}

/// The ground height under (x, z): the first surface a ray straight down
/// from `from_y` meets.
pub fn ground_height(ground: &[MeshInstance], lib: Option<&MeshLibrary>, x: f32, z: f32, from_y: f32) -> Option<f32> {
    raycast(ground, lib, [x, from_y, z], [0.0, -1.0, 0.0], f32::MAX).map(|h| h.point[1])
}

/// Lowest point of an instance below its position, and its footprint box
/// (min x, min z, max x, max z) in world space.
fn extent(m: &MeshInstance, lib: Option<&MeshLibrary>) -> Option<(f32, [f32; 4])> {
    let mesh = mesh_of(m.mesh, lib)?;
    let mut low = f32::INFINITY;
    let mut fp = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for v in &mesh.vertices {
        let w = rotate(m.rotation, [v[0] * m.scale[0], v[1] * m.scale[1], v[2] * m.scale[2]]);
        low = low.min(w[1]);
        fp = [fp[0].min(w[0]), fp[1].min(w[2]), fp[2].max(w[0]), fp[3].max(w[2])];
    }
    let p = m.position;
    Some((low, [fp[0] + p[0], fp[1] + p[2], fp[2] + p[0], fp[3] + p[2]]))
}

/// Move the parts of one object (a trunk and its canopy) up or down
/// together so its lowest point rests on the ground, `sink` metres in
/// (a little sink hides the contact line). Rays go down from the centre and
/// the four corners of the lowest part's footprint; it rests on the
/// highest ground below them, so nothing pokes through. False (unchanged)
/// when there is no ground below.
pub fn snap_parts(ground: &[MeshInstance], lib: Option<&MeshLibrary>, parts: &mut [MeshInstance], sink: f32) -> bool {
    let mut low = f32::INFINITY;
    let mut fp = [0.0; 4];
    for m in parts.iter() {
        if let Some((l, f)) = extent(m, lib) {
            let y = m.position[1] + l;
            if y < low {
                low = y;
                fp = f;
            }
        }
    }
    if !low.is_finite() {
        return false;
    }
    let (cx, cz) = ((fp[0] + fp[2]) * 0.5, (fp[1] + fp[3]) * 0.5);
    let (ix, iz) = ((fp[2] - fp[0]) * 0.35, (fp[3] - fp[1]) * 0.35);
    let from = low + 1e4;
    let top = [(0.0, 0.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
        .iter()
        .filter_map(|(a, b)| ground_height(ground, lib, cx + a * ix, cz + b * iz, from))
        .fold(f32::NEG_INFINITY, f32::max);
    if !top.is_finite() {
        return false;
    }
    let dy = top - sink - low;
    for m in parts.iter_mut() {
        m.position[1] += dy;
    }
    true
}

/// [`snap_parts`] for one instance.
pub fn snap_to_ground(ground: &[MeshInstance], lib: Option<&MeshLibrary>, m: &mut MeshInstance, sink: f32) -> bool {
    snap_parts(ground, lib, std::slice::from_mut(m), sink)
}

/// Footprints on the ground plane (circles in XZ) that placement must not
/// overlap. A uniform grid keeps lookups fast for thousands of props.
#[derive(Debug, Clone)]
pub struct Footprints {
    /// Extra clearance between footprints (m).
    pub gap: f32,
    cell: f32,
    circles: Vec<([f32; 2], f32)>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

impl Footprints {
    pub fn new(gap: f32) -> Footprints {
        Footprints {
            gap,
            cell: 4.0,
            circles: Vec::new(),
            grid: HashMap::new(),
        }
    }

    fn cells(&self, c: [f32; 2], r: f32) -> impl Iterator<Item = (i32, i32)> {
        let f = |v: f32| (v / self.cell).floor() as i32;
        let (x0, x1, z0, z1) = (f(c[0] - r), f(c[0] + r), f(c[1] - r), f(c[1] + r));
        (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
    }

    /// Is a circle free (no stored footprint within it plus the gap)?
    pub fn is_free(&self, c: [f32; 2], r: f32) -> bool {
        let reach = r + self.gap;
        // Big circles are rare: scan them all rather than many cells.
        if reach > self.cell * 16.0 {
            return self.circles.iter().all(|(p, q)| dist2(*p, c) >= (q + reach) * (q + reach));
        }
        // Stored circles sit in every cell they cover, so overlapping circles
        // always share a cell.
        for cell in self.cells(c, reach) {
            if let Some(ids) = self.grid.get(&cell) {
                for &i in ids {
                    let (p, q) = self.circles[i as usize];
                    if dist2(p, c) < (q + reach) * (q + reach) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Store a circle (no test).
    pub fn reserve(&mut self, c: [f32; 2], r: f32) {
        let id = self.circles.len() as u32;
        self.circles.push((c, r));
        let cells: Vec<(i32, i32)> = self.cells(c, r).collect();
        for cell in cells {
            self.grid.entry(cell).or_default().push(id);
        }
    }

    /// Reserve a band `half_width` either side of a polyline (a road's
    /// centre line in XZ), so props can't stand on it.
    pub fn reserve_path(&mut self, line: &[[f32; 2]], half_width: f32) {
        let step = (half_width * 0.75).max(0.25);
        for w in line.windows(2) {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let n = (l / step).ceil().max(1.0) as usize;
            for k in 0..n {
                let f = k as f32 / n as f32;
                self.reserve([w[0][0] + d[0] * f, w[0][1] + d[1] * f], half_width);
            }
        }
        if let Some(last) = line.last() {
            self.reserve(*last, half_width);
        }
    }

    /// The footprint circle of an object's parts (centre and radius in XZ).
    pub fn footprint(parts: &[MeshInstance], lib: Option<&MeshLibrary>) -> Option<([f32; 2], f32)> {
        let mut fp = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for m in parts {
            let (_, f) = extent(m, lib)?;
            fp = [fp[0].min(f[0]), fp[1].min(f[1]), fp[2].max(f[2]), fp[3].max(f[3])];
        }
        if !fp[0].is_finite() {
            return None;
        }
        let c = [(fp[0] + fp[2]) * 0.5, (fp[1] + fp[3]) * 0.5];
        let r = 0.5 * ((fp[2] - fp[0]).powi(2) + (fp[3] - fp[1]).powi(2)).sqrt();
        Some((c, r))
    }

    /// Place an object if its footprint is free: true and stored, or false
    /// (overlaps something already placed or reserved).
    pub fn try_place(&mut self, parts: &[MeshInstance], lib: Option<&MeshLibrary>) -> bool {
        let Some((c, r)) = Self::footprint(parts, lib) else { return false };
        if !self.is_free(c, r) {
            return false;
        }
        self.reserve(c, r);
        true
    }

    pub fn len(&self) -> usize {
        self.circles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.circles.is_empty()
    }
}

fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(mesh: MeshId, p: V, s: V) -> MeshInstance {
        let mut m = MeshInstance::new(mesh, p);
        m.scale = s;
        m
    }

    fn hill() -> Vec<MeshInstance> {
        // Flat ground at 0 and a box "hill" 2 m tall around x = 20.
        vec![
            inst(MeshId::CUBE, [0.0, -0.5, 0.0], [100.0, 1.0, 100.0]),
            inst(MeshId::CUBE, [20.0, 1.0, 0.0], [10.0, 2.0, 10.0]),
        ]
    }

    #[test]
    fn raycast_hits_the_nearest_face_of_rotated_and_scaled_instances() {
        let g = hill();
        let h = raycast(&g, None, [20.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0).unwrap();
        assert_eq!(h.index, 1);
        assert!((h.point[1] - 2.0).abs() < 1e-4 && (h.normal[1] - 1.0).abs() < 1e-4);
        // A box turned 45 degrees about y: its corner points at +x.
        let mut b = inst(MeshId::CUBE, [0.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let a = core::f32::consts::FRAC_PI_8;
        b.rotation = [0.0, a.sin(), 0.0, a.cos()];
        let h = raycast(&[b], None, [5.0, 0.5, 0.0], [-1.0, 0.0, 0.0], 100.0).unwrap();
        assert!((h.point[0] - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3, "{h:?}");
        assert!(raycast(&g, None, [200.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0).is_none());
        // A sphere: hits at its surface.
        let s = inst(MeshId::SPHERE, [0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let h = raycast(&[s], None, [0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0).unwrap();
        assert!((h.t - 9.0).abs() < 0.01, "{}", h.t);
    }

    #[test]
    fn snap_rests_objects_on_the_ground_with_the_sink() {
        let g = hill();
        // A trunk and canopy floating 3 m up over the hill.
        let mut tree = vec![
            inst(MeshId::CYLINDER, [20.0, 6.0, 0.0], [0.4, 2.0, 0.4]),
            inst(MeshId::SPHERE, [20.0, 7.5, 0.0], [2.0, 2.0, 2.0]),
        ];
        assert!(snap_parts(&g, None, &mut tree, 0.05));
        // Trunk bottom (y - 1) at the hill top (2) minus the sink.
        assert!((tree[0].position[1] - 1.0 - 1.95).abs() < 1e-4, "{:?}", tree[0].position);
        assert!((tree[1].position[1] - tree[0].position[1] - 1.5).abs() < 1e-5);
        // Over nothing: unchanged.
        let mut lost = inst(MeshId::CUBE, [500.0, 3.0, 0.0], [1.0; 3]);
        assert!(!snap_to_ground(&g, None, &mut lost, 0.0));
        assert_eq!(lost.position[1], 3.0);
        // A sloped stand: rests on the highest ground under its footprint.
        let mut post = inst(MeshId::CUBE, [15.5, 5.0, 0.0], [2.0, 1.0, 2.0]);
        assert!(snap_to_ground(&g, None, &mut post, 0.0));
        assert!((post.position[1] - 0.5 - 2.0).abs() < 1e-4, "{:?}", post.position);
    }

    #[test]
    fn footprints_refuse_overlaps_and_the_road() {
        let mut fp = Footprints::new(0.2);
        fp.reserve_path(&[[0.0, -50.0], [0.0, 50.0]], 7.0);
        let tree = |x: f32, z: f32| vec![inst(MeshId::CYLINDER, [x, 1.0, z], [0.4, 2.0, 0.4]), inst(MeshId::SPHERE, [x, 2.5, z], [2.0; 3])];
        assert!(!fp.try_place(&tree(5.0, 0.0), None), "on the road");
        assert!(fp.try_place(&tree(10.0, 0.0), None));
        assert!(!fp.try_place(&tree(11.5, 0.0), None), "overlaps the first tree");
        assert!(fp.try_place(&tree(13.5, 0.0), None));
        let before = fp.len();
        // Many props: the grid keeps them apart.
        let mut placed = 0;
        for i in 0..400 {
            let x = 20.0 + (i % 20) as f32 * 1.1;
            let z = -20.0 + (i / 20) as f32 * 1.1;
            placed += fp.try_place(&tree(x, z), None) as usize;
        }
        assert!(placed > 0 && placed < 400);
        assert_eq!(fp.len(), before + placed);
        for i in before..fp.len() {
            for j in before..i {
                let (a, b) = (fp.circles[i], fp.circles[j]);
                assert!(dist2(a.0, b.0).sqrt() >= a.1 + b.1 + 0.2 - 1e-4);
            }
        }
    }
}
