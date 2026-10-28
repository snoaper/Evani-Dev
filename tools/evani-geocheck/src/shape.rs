//! World-space shapes of drawn instances: convex polytopes (the built-in
//! meshes are all convex) with exact f64 clipping for overlap volumes, ray
//! casts and point tests. A non-convex mesh keeps its triangles for ray
//! casts and coplanar-face tests, and when it is a closed swept mesh
//! (spline roads, kerbs, walls: `evani_render::spline::sweep`) it is also
//! split into one convex piece per sweep segment ([`crate::sweep`]), so it
//! has a volume and an inside like any other solid. Any other non-convex
//! mesh (an imported model) is a surface only: no volume, and
//! [`Shapes::notes`] says so.

use std::collections::{HashMap, HashSet};

use evani_render::mesh::{self, BUILTIN_MESH_COUNT, Mesh, MeshId, MeshLibrary};
use evani_render::world::MeshInstance;

use crate::geom::*;

/// A planar face: outward normal `n`, plane `n . x = d`, and its polygon
/// (counter-clockwise seen from outside).
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub n: V3,
    pub d: f64,
    pub pts: Vec<V3>,
    /// Box of the polygon (a quick reject for face-pair tests).
    pub bb: Aabb,
}

impl Face {
    pub fn from_pts(pts: Vec<V3>) -> Option<Face> {
        // Newell's normal: robust for any planar polygon.
        let mut n = [0.0; 3];
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        if len(n) < 1e-14 {
            return None;
        }
        let n = norm(n);
        let d = dot(n, pts[0]);
        let bb = Aabb::from_points(&pts);
        Some(Face { n, d, pts, bb })
    }

    pub fn area(&self) -> f64 {
        let mut a = [0.0; 3];
        for i in 0..self.pts.len() {
            a = add(a, cross(self.pts[i], self.pts[(i + 1) % self.pts.len()]));
        }
        0.5 * dot(a, self.n).abs()
    }

    pub fn centroid(&self) -> V3 {
        let mut c = [0.0; 3];
        for p in &self.pts {
            c = add(c, *p);
        }
        mul(c, 1.0 / self.pts.len().max(1) as f64)
    }

    /// The polygon in the plane basis of `n` (counter-clockwise about `n`
    /// when the face faces `n`).
    pub fn to_2d(&self, u: V3, v: V3) -> Vec<[f64; 2]> {
        self.pts.iter().map(|p| [dot(*p, u), dot(*p, v)]).collect()
    }

    /// `p` (on the face's plane) lies inside the polygon (within `eps`).
    pub fn contains_on_plane(&self, p: V3, eps: f64) -> bool {
        for i in 0..self.pts.len() {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % self.pts.len()]);
            if dot(cross(sub(b, a), sub(p, a)), self.n) < -eps * len(sub(b, a)) {
                return false;
            }
        }
        true
    }
}

/// A local-space mesh reduced to what the checks need.
#[derive(Debug, Clone)]
pub struct LocalShape {
    /// Merged planar faces (convex mesh) or triangles (otherwise).
    pub faces: Vec<Vec<V3>>,
    pub convex: bool,
    /// Has a volume: closed and convex, or closed and split into `pieces`.
    pub solid: bool,
    /// Watertight: every edge is shared by two triangles (convex or not,
    /// split or not). From outside, its back faces are hidden by its front.
    pub closed: bool,
    /// Convex pieces (outward polygons) of a concave solid; empty for a
    /// convex mesh (which is its own single piece).
    pub pieces: Vec<Vec<Vec<V3>>>,
    /// What the checks can do with a concave mesh, in words (`None` for a
    /// convex mesh or an open surface).
    pub note: Option<String>,
    /// The note is a limit: the mesh is closed and concave, but could not be
    /// split, so it has no volume in the overlap and inside tests.
    pub limited: bool,
}

/// Group a mesh's triangles into planar faces; the mesh is convex when every
/// vertex lies behind every face plane.
pub fn local_shape(m: &Mesh) -> LocalShape {
    let pos: Vec<V3> = m.vertices.iter().map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]).collect();
    // Tolerances relative to the mesh size: tiny f32 triangles (a sphere's
    // poles) have tilted normals or no real area.
    let size = Aabb::from_points(&pos).max_extent().max(1e-9);
    let mut groups: Vec<(V3, f64, Vec<V3>)> = Vec::new();
    let mut tris: Vec<Vec<V3>> = Vec::new();
    for t in m.indices.as_chunks::<3>().0 {
        let (Some(&a), Some(&b), Some(&c)) = (pos.get(t[0] as usize), pos.get(t[1] as usize), pos.get(t[2] as usize)) else {
            continue;
        };
        let n = cross(sub(b, a), sub(c, a));
        if len(n) < 1e-8 * size * size {
            continue;
        }
        let n = norm(n);
        let d = dot(n, a);
        tris.push(vec![a, b, c]);
        match groups.iter_mut().find(|g| dot(g.0, n) > 1.0 - 1e-9 && (g.1 - d).abs() < 1e-7) {
            Some(g) => g.2.extend_from_slice(&[a, b, c]),
            None => groups.push((n, d, vec![a, b, c])),
        }
    }
    let convex = !groups.is_empty() && groups.iter().all(|(n, d, _)| pos.iter().all(|p| dot(*n, *p) <= d + 1e-5 * size));
    if !convex {
        // Concave. A closed sweep splits into convex segments; any other
        // closed concave mesh stays a surface (and says so).
        let mut out = LocalShape {
            faces: tris,
            convex: false,
            solid: false,
            closed: crate::meshcheck::is_closed(m),
            pieces: Vec::new(),
            note: None,
            limited: false,
        };
        if crate::meshcheck::is_closed(m) {
            match crate::sweep::decompose(m) {
                Ok(d) => {
                    out.solid = !d.pieces.is_empty();
                    out.note = Some(if d.bent == 0 {
                        format!("swept mesh, split into {} convex segments for the overlap volume", d.pieces.len())
                    } else {
                        format!(
                            "swept mesh, {} convex segments checked for overlap volume; {} of {} segments are bent too tightly to be convex and are checked as surface only",
                            d.pieces.len(),
                            d.bent,
                            d.segments
                        )
                    });
                    out.limited = d.bent > 0;
                    out.pieces = d.pieces;
                }
                Err(why) => {
                    out.note = Some(format!(
                        "closed but concave and not a swept mesh ({why}): checked as a surface (ray casts, coplanar faces, mesh validation) with no overlap volume or inside test; build it from convex parts or with evani_render::spline::sweep to get those"
                    ));
                    out.limited = true;
                }
            }
        }
        return out;
    }
    let mut faces = Vec::with_capacity(groups.len());
    for (n, _, pts) in &groups {
        let (u, v) = plane_basis(*n);
        let d = dot(*n, pts[0]);
        let h = hull_2d(&pts.iter().map(|p| [dot(*p, u), dot(*p, v)]).collect::<Vec<_>>());
        if h.len() < 3 {
            continue;
        }
        // A flat group with gaps (a flat mesh of stripes that interlocks with
        // another, a ring) is its triangles, not its hull: the hull would
        // cover what the mesh leaves open.
        let tri_area: f64 = pts.as_chunks::<3>().0.iter().map(|t| 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])))).sum();
        if area_2d(&h).abs() > tri_area * (1.0 + 1e-4) + 1e-9 * size * size {
            faces.extend(pts.as_chunks::<3>().0.iter().map(|t| t.to_vec()));
        } else {
            faces.push(h.iter().map(|q| add(add(mul(u, q[0]), mul(v, q[1])), mul(*n, d))).collect());
        }
    }
    // Closed: every edge of the original mesh is shared by two triangles.
    let closed = crate::meshcheck::is_closed(m);
    let solid = closed && faces.len() >= 4;
    LocalShape {
        faces,
        convex,
        solid,
        closed,
        pieces: Vec::new(),
        note: None,
        limited: false,
    }
}

/// A drawn instance in world space.
#[derive(Debug, Clone)]
pub struct Shape {
    /// The surface: merged polygons of a convex mesh, triangles otherwise.
    pub faces: Vec<Face>,
    pub aabb: Aabb,
    /// Has a volume and an inside: a closed convex polytope, or a closed
    /// concave mesh split into convex `pieces`.
    pub solid: bool,
    /// Watertight (convex or concave, split into pieces or not) and not
    /// flattened by a zero scale: seen from outside, every back face lies
    /// behind one of its own front faces, so it can't show.
    pub closed: bool,
    pub convex: bool,
    /// The convex pieces of a concave solid (empty: `faces` is the one piece).
    pub pieces: Vec<Piece>,
}

/// One convex part of a concave solid.
#[derive(Debug, Clone)]
pub struct Piece {
    pub faces: Vec<Face>,
    pub aabb: Aabb,
}

/// Local shapes of every mesh, built once.
#[derive(Debug, Default)]
pub struct Shapes {
    cache: HashMap<u16, LocalShape>,
    custom: HashMap<u16, Mesh>,
    open_ok: HashSet<u16>,
}

impl Shapes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a non-built-in mesh (custom, generated or imported).
    pub fn add_mesh(&mut self, id: MeshId, m: Mesh) {
        self.cache.remove(&id.0);
        self.custom.insert(id.0, m);
    }

    /// Register every mesh of the app's [`MeshLibrary`] under its id (the
    /// instances that draw them are otherwise unknown shapes).
    pub fn add_library(&mut self, lib: &MeshLibrary) {
        for (n, m) in lib.meshes().iter().enumerate() {
            self.add_mesh(MeshId((BUILTIN_MESH_COUNT + n) as u16), m.clone());
        }
    }

    /// Say that mesh `id` has open edges on purpose (a one-sided ribbon, a
    /// plane): mesh validation then does not report them. Custom meshes are
    /// expected to be closed unless the adapter says otherwise.
    pub fn allow_open(&mut self, id: MeshId) {
        self.open_ok.insert(id.0);
    }

    /// Open edges are intended for this mesh (the plane, or [`Shapes::allow_open`]).
    pub fn open_intended(&self, id: MeshId) -> bool {
        id == MeshId::PLANE || self.open_ok.contains(&id.0)
    }

    /// What the checks can and cannot do with each of the custom meshes in
    /// `used`, in words, for the report: a swept road is split into convex
    /// segments; a closed concave model is a surface only. Meshes that are
    /// plain convex solids or open surfaces need no note.
    pub fn notes(&mut self, used: &[MeshId]) -> Vec<String> {
        let mut out = Vec::new();
        for id in used {
            if !self.custom.contains_key(&id.0) {
                continue;
            }
            let tris = self.custom.get(&id.0).map_or(0, Mesh::triangles);
            if let Some(note) = self.local(*id).and_then(|l| l.note.clone()) {
                out.push(format!("Custom mesh {} ({tris} triangles): {note}.", id.0));
            }
        }
        out
    }

    pub fn mesh(&self, id: MeshId) -> Option<Mesh> {
        self.custom.get(&id.0).cloned().or_else(|| mesh::builtin(id))
    }

    pub fn local(&mut self, id: MeshId) -> Option<&LocalShape> {
        if !self.cache.contains_key(&id.0) {
            let m = self.mesh(id)?;
            self.cache.insert(id.0, local_shape(&m));
        }
        self.cache.get(&id.0)
    }

    /// The world-space shape of `inst` (`None` for an unknown mesh).
    pub fn shape(&mut self, inst: &MeshInstance) -> Option<Shape> {
        let q = quat(inst.rotation);
        let s = v3(inst.scale);
        let p = v3(inst.position);
        let flat = s.iter().any(|x| x.abs() < 1e-6);
        let mirrored = s[0] * s[1] * s[2] < 0.0;
        let local = self.local(inst.mesh)?;
        let xf = |v: &V3| add(rotate(q, [v[0] * s[0], v[1] * s[1], v[2] * s[2]]), p);
        let mut faces = Vec::with_capacity(local.faces.len());
        let mut aabb = Aabb::EMPTY;
        for f in &local.faces {
            let mut pts: Vec<V3> = f.iter().map(xf).collect();
            if mirrored {
                pts.reverse();
            }
            for q in &pts {
                aabb.add(*q);
            }
            if let Some(face) = Face::from_pts(pts) {
                faces.push(face);
            }
        }
        let mut pieces = Vec::with_capacity(local.pieces.len());
        if !flat {
            for polys in &local.pieces {
                let mut faces = Vec::with_capacity(polys.len());
                let mut bb = Aabb::EMPTY;
                for f in polys {
                    let mut pts: Vec<V3> = f.iter().map(xf).collect();
                    if mirrored {
                        pts.reverse();
                    }
                    for q in &pts {
                        bb.add(*q);
                    }
                    faces.extend(Face::from_pts(pts));
                }
                if faces.len() >= 4 {
                    pieces.push(Piece { faces, aabb: bb });
                }
            }
        }
        Some(Shape {
            faces,
            aabb,
            solid: local.solid && !flat && (local.convex || !pieces.is_empty()),
            closed: local.closed && !flat,
            convex: local.convex,
            pieces,
        })
    }
}

/// Entry distance of a ray into the convex polytope bounded by `faces`
/// (0 when it starts inside), within `tmax`.
fn ray_convex(faces: &[Face], o: V3, dir: V3, tmax: f64) -> Option<f64> {
    let (mut tn, mut tf) = (0.0f64, tmax);
    for f in faces {
        let denom = dot(f.n, dir);
        let dist = f.d - dot(f.n, o);
        if denom.abs() < 1e-15 {
            if dist < 0.0 {
                return None;
            }
            continue;
        }
        let t = dist / denom;
        if denom < 0.0 {
            tn = tn.max(t);
        } else {
            tf = tf.min(t);
        }
        if tn > tf {
            return None;
        }
    }
    Some(tn)
}

/// Volume of the intersection of two convex polytopes and its box.
fn overlap_convex(fa: &[Face], fb: &[Face]) -> Option<(f64, Aabb)> {
    // Clip the one with fewer faces by the other's planes.
    let (a, b) = if fa.len() <= fb.len() { (fa, fb) } else { (fb, fa) };
    let mut polys: Vec<Vec<V3>> = a.iter().map(|f| f.pts.clone()).collect();
    for f in b {
        polys = clip_polytope(&polys, f.n, f.d);
        if polys.len() < 4 {
            return None;
        }
    }
    let v = polys_volume(polys.iter().map(Vec::as_slice));
    if v <= 0.0 {
        return None;
    }
    Some((v, Aabb::from_points(polys.iter().flatten())))
}

impl Shape {
    /// The convex parts: the shape itself when it is convex, else its pieces.
    fn parts(&self) -> impl Iterator<Item = (&[Face], &Aabb)> {
        std::iter::once((self.faces.as_slice(), &self.aabb))
            .filter(|_| self.pieces.is_empty())
            .chain(self.pieces.iter().map(|p| (p.faces.as_slice(), &p.aabb)))
    }

    /// Volume (0 unless solid).
    pub fn volume(&self) -> f64 {
        if !self.solid {
            return 0.0;
        }
        self.parts().map(|(fs, _)| polys_volume(fs.iter().map(|f| f.pts.as_slice()))).sum()
    }

    /// Point strictly inside, at least `margin` from every face of one of its
    /// convex parts (so within `margin` of the joint between two segments of a
    /// sweep it counts as outside).
    pub fn contains(&self, p: V3, margin: f64) -> bool {
        self.solid
            && self.aabb.contains(p, 0.0)
            && self.parts().any(|(fs, bb)| bb.contains(p, 0.0) && fs.iter().all(|f| dot(f.n, p) - f.d < -margin))
    }

    /// First hit along `o + t * dir` (`dir` unit), `0 <= t <= tmax`. A ray
    /// starting inside a solid hits at 0.
    pub fn ray(&self, o: V3, dir: V3, tmax: f64) -> Option<f64> {
        if self.solid {
            let mut best: Option<f64> = None;
            for (fs, _) in self.parts() {
                if let Some(t) = ray_convex(fs, o, dir, best.map_or(tmax, |b| b.min(tmax)))
                    && best.is_none_or(|b| t < b)
                {
                    best = Some(t);
                    if t == 0.0 {
                        break;
                    }
                }
            }
            return best;
        }
        self.ray_surface(o, dir, tmax)
    }

    /// First crossing of the surface (the triangles or polygons, solid or
    /// not) along `o + t * dir`, `0 <= t <= tmax`: a ray starting inside a
    /// solid finds the wall it will leave through, not 0.
    pub fn ray_surface(&self, o: V3, dir: V3, tmax: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        for f in &self.faces {
            let denom = dot(f.n, dir);
            if denom.abs() < 1e-15 {
                continue;
            }
            let t = (f.d - dot(f.n, o)) / denom;
            if t < 0.0 || t > tmax || best.is_some_and(|b| t >= b) {
                continue;
            }
            if f.contains_on_plane(add(o, mul(dir, t)), 1e-9) {
                best = Some(t);
            }
        }
        best
    }

    /// Volume of the intersection with `o` and its bounding box (both solid).
    /// A concave solid is the sum over its convex pieces.
    pub fn overlap(&self, o: &Shape) -> Option<(f64, Aabb)> {
        if !self.solid || !o.solid || !self.aabb.overlaps(&o.aabb, 0.0) {
            return None;
        }
        let mut total = 0.0;
        let mut bb = Aabb::EMPTY;
        for (fa, ba) in self.parts() {
            for (fb, bb2) in o.parts() {
                if !ba.overlaps(bb2, 0.0) {
                    continue;
                }
                if let Some((v, b)) = overlap_convex(fa, fb) {
                    total += v;
                    bb = bb.union(&b);
                }
            }
        }
        (total > 0.0).then_some((total, bb))
    }
}

/// Volume enclosed by outward-wound polygons (divergence theorem).
pub fn polys_volume<'a>(polys: impl Iterator<Item = &'a [V3]>) -> f64 {
    let mut v = 0.0;
    for p in polys {
        for i in 1..p.len().saturating_sub(1) {
            v += dot(p[0], cross(p[i], p[i + 1]));
        }
    }
    v / 6.0
}

/// Keep the part of a convex polytope (outward polygons) with `n . x <= d`,
/// closing the cut with a cap.
pub fn clip_polytope(polys: &[Vec<V3>], n: V3, d: f64) -> Vec<Vec<V3>> {
    const EPS: f64 = 1e-10;
    // Nothing beyond the plane: unchanged (a face lying on the plane must
    // not grow a duplicate cap).
    if polys.iter().flatten().all(|p| dot(n, *p) - d <= EPS) {
        return polys.to_vec();
    }
    let mut out = Vec::with_capacity(polys.len() + 1);
    let mut cap: Vec<V3> = Vec::new();
    for poly in polys {
        let mut np = Vec::with_capacity(poly.len() + 1);
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (sp, sq) = (dot(n, p) - d, dot(n, q) - d);
            if sp <= EPS {
                np.push(p);
                if sp.abs() <= EPS {
                    cap.push(p);
                }
            }
            if (sp < -EPS && sq > EPS) || (sp > EPS && sq < -EPS) {
                let x = lerp(p, q, sp / (sp - sq));
                np.push(x);
                cap.push(x);
            }
        }
        if np.len() >= 3 {
            out.push(np);
        }
    }
    if cap.len() >= 3 {
        let (u, v) = plane_basis(n);
        let h = hull_2d(&cap.iter().map(|p| [dot(*p, u), dot(*p, v)]).collect::<Vec<_>>());
        if h.len() >= 3 && area_2d(&h) > 1e-14 {
            out.push(h.iter().map(|q| add(add(mul(u, q[0]), mul(v, q[1])), mul(n, d))).collect());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_render::math::quat_axis_angle;

    fn cube(p: [f32; 3], s: [f32; 3]) -> MeshInstance {
        let mut m = MeshInstance::new(MeshId::CUBE, p);
        m.scale = s;
        m
    }

    #[test]
    fn builtins_are_convex_solids_and_plane_is_open() {
        let mut sh = Shapes::new();
        for id in [MeshId::CUBE, MeshId::SPHERE, MeshId::BEVEL_CUBE, MeshId::CYLINDER] {
            let l = sh.local(id).unwrap();
            assert!(l.convex && l.solid, "{id:?} convex {} solid {} faces {}", l.convex, l.solid, l.faces.len());
        }
        let cube = sh.local(MeshId::CUBE).unwrap();
        assert_eq!(cube.faces.len(), 6);
        let plane = sh.local(MeshId::PLANE).unwrap();
        assert!(plane.convex && !plane.solid);
        assert_eq!(plane.faces.len(), 1);
    }

    #[test]
    fn a_flat_mesh_with_gaps_keeps_its_triangles_not_its_hull() {
        // Two 1 m squares side by side with a 1 m gap between them (the
        // light stripes of a striped lawn): the hull would cover the gap.
        let n = [0.0, 1.0, 0.0];
        let quad = |x: f32| [[x, 0.0, 0.0], [x, 0.0, 1.0], [x + 1.0, 0.0, 1.0], [x + 1.0, 0.0, 0.0]];
        let mut m = Mesh::default();
        for x in [0.0f32, 2.0] {
            let b = m.vertices.len() as u32;
            m.vertices.extend(quad(x).iter().map(|p| [p[0], p[1], p[2], n[0], n[1], n[2]]));
            m.indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let l = local_shape(&m);
        assert_eq!(l.faces.len(), 4, "its four triangles");
        let area: f64 = l.faces.iter().map(|f| 0.5 * len(cross(sub(f[1], f[0]), sub(f[2], f[0])))).sum();
        assert!((area - 2.0).abs() < 1e-9);
        // One square alone is still one merged face.
        m.indices.truncate(6);
        assert_eq!(local_shape(&m).faces.len(), 1);
    }

    #[test]
    fn volumes_and_overlap_volumes_are_exact() {
        let mut sh = Shapes::new();
        let a = sh.shape(&cube([0.0; 3], [2.0, 2.0, 2.0])).unwrap();
        assert!((a.volume() - 8.0).abs() < 1e-9);
        // Shifted by 1.5 m: a 0.5 x 2 x 2 slab overlaps.
        let b = sh.shape(&cube([1.5, 0.0, 0.0], [2.0, 2.0, 2.0])).unwrap();
        let (v, bb) = a.overlap(&b).unwrap();
        assert!((v - 2.0).abs() < 1e-9, "{v}");
        assert!((bb.extent()[0] - 0.5).abs() < 1e-9);
        // Touching faces: no volume.
        let c = sh.shape(&cube([2.0, 0.0, 0.0], [2.0, 2.0, 2.0])).unwrap();
        assert!(a.overlap(&c).is_none());
        // A unit cube turned 45 degrees about Y, centred 1.5 m away: its
        // corner pokes h = 1 - (1.5 - sqrt2/2) into `a`, a triangular prism
        // of area h^2 and height 1.
        let mut r = cube([1.5, 0.0, 0.0], [1.0, 1.0, 1.0]);
        r.rotation = quat_axis_angle([0.0, 1.0, 0.0], std::f32::consts::FRAC_PI_4);
        let r = sh.shape(&r).unwrap();
        let (v1, _) = a.overlap(&r).unwrap();
        let (v2, _) = r.overlap(&a).unwrap();
        assert!((v1 - v2).abs() < 1e-9);
        let h = 1.0 - (1.5 - std::f64::consts::FRAC_1_SQRT_2);
        assert!((v1 - h * h).abs() < 1e-5, "{v1} vs {}", h * h);
        // Sphere radius 0.5 inside a cube: its own volume (polytope ~ 4/3 pi r^3).
        let s = sh.shape(&MeshInstance::new(MeshId::SPHERE, [0.0; 3])).unwrap();
        let (vs, _) = a.overlap(&s).unwrap();
        // (Half-space form vs face polygons of an f32 sphere: equal to ~1e-6.)
        assert!((vs - s.volume()).abs() < 1e-4 * s.volume(), "{vs} vs {}", s.volume());
        assert!((s.volume() - 4.0 / 3.0 * std::f64::consts::PI * 0.125).abs() < 0.02);
    }

    #[test]
    fn rays_hit_solids_and_planes() {
        let mut sh = Shapes::new();
        let a = sh.shape(&cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0])).unwrap();
        assert!((a.ray([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0).unwrap() - 4.0).abs() < 1e-12);
        assert!(a.ray([0.0, 5.0, 0.0], [0.0, 1.0, 0.0], 100.0).is_none());
        assert!(a.ray([3.0, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0).is_none());
        assert_eq!(a.ray([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], 100.0), Some(0.0));
        assert!(a.ray([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 3.0).is_none());
        let mut p = MeshInstance::new(MeshId::PLANE, [0.0, -2.0, 0.0]);
        p.scale = [10.0, 1.0, 10.0];
        let p = sh.shape(&p).unwrap();
        assert!(!p.solid);
        assert!((p.ray([1.0, 5.0, 1.0], [0.0, -1.0, 0.0], 100.0).unwrap() - 7.0).abs() < 1e-12);
        assert!(p.ray([6.0, 5.0, 1.0], [0.0, -1.0, 0.0], 100.0).is_none());
        assert!(a.contains([0.5, 0.5, 0.5], 0.1));
        assert!(!a.contains([0.95, 0.5, 0.5], 0.1));
    }

    #[test]
    fn mirrored_and_flat_instances() {
        let mut sh = Shapes::new();
        let m = sh.shape(&cube([0.0; 3], [-1.0, 1.0, 1.0])).unwrap();
        assert!((m.volume() - 1.0).abs() < 1e-9, "a mirrored cube keeps a positive volume");
        let f = sh.shape(&cube([0.0; 3], [1.0, 0.0, 1.0])).unwrap();
        assert!(!f.solid);
    }
}
