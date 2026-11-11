//! Built-in procedural meshes (no asset download): cube, UV sphere, plane,
//! bevelled cube and cylinder.
//! Vertices are interleaved position + normal; triangles wind
//! counter-clockwise seen from outside (front face CCW, back faces culled).

mod store;
pub use store::{INDEX_BYTES, MeshBuf, MeshStore, MeshSync, MeshWrite, VERTEX_BYTES};

/// Which built-in mesh an instance draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshId(pub u16);

impl MeshId {
    /// Unit cube, -0.5..0.5.
    pub const CUBE: MeshId = MeshId(0);
    /// Sphere of radius 0.5.
    pub const SPHERE: MeshId = MeshId(1);
    /// 1 x 1 plane on XZ facing +Y.
    pub const PLANE: MeshId = MeshId(2);
    /// Unit cube, -0.5..0.5, with every edge chamfered (flat shaded, 44
    /// triangles): reads as a made object rather than a raw box, and the
    /// chamfers catch the toon ramp and crease outlines.
    pub const BEVEL_CUBE: MeshId = MeshId(3);
    /// Cylinder of radius 0.5 along Y, -0.5..0.5, 16 sides (smooth sides,
    /// flat caps, 64 triangles): wheels, posts, trunks.
    pub const CYLINDER: MeshId = MeshId(4);
    /// Sphere of radius 0.5 with 10 segments and 6 rings (100 triangles,
    /// a seventh of [`MeshId::SPHERE`]): crowds, clouds, small sweets and
    /// anything far away or many times on screen.
    pub const LOW_SPHERE: MeshId = MeshId(5);
    /// Cone of radius 0.5 at y = -0.5 with its tip at y = 0.5, 12 sides
    /// (smooth sides, flat base, 24 triangles): twist ends, hats, tufts.
    pub const CONE: MeshId = MeshId(6);
    /// Dome: the top half of a radius 0.5 sphere standing on y = 0 with a
    /// flat base, 12 segments and 4 rings (96 triangles): hills, gumdrops,
    /// caps; nothing hidden under the ground.
    pub const DOME: MeshId = MeshId(7);
}

/// Number of built-in meshes ([`MeshId`] 0..COUNT).
pub const BUILTIN_MESH_COUNT: usize = 8;

/// Most meshes a game can draw: the built-ins plus custom meshes in the
/// [`MeshLibrary`]. Each slot costs 20 bytes of CPU-side tables (a draw
/// range, a triangle count and an instance count) and nothing on the GPU
/// beyond the shared vertex and index buffers: no uniform, bind group or
/// shader constant depends on it.
pub const MAX_MESHES: usize = 256;

/// Custom meshes a game can register: [`MAX_MESHES`] less the built-ins
/// (248).
pub const MAX_CUSTOM_MESHES: usize = MAX_MESHES - BUILTIN_MESH_COUNT;

/// Mesh slots the renderer packs instances into: the built-ins, plus the
/// custom ones with feature `meshes` (so games without it keep the small
/// fixed arrays and none of the upload code).
pub const MESH_SLOTS: usize = if cfg!(feature = "meshes") { MAX_MESHES } else { BUILTIN_MESH_COUNT };

/// The [`MeshLibrary`] is full: a game may register at most
/// [`MAX_CUSTOM_MESHES`] custom meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshLimitError;

impl std::fmt::Display for MeshLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mesh library full: at most {MAX_CUSTOM_MESHES} custom meshes ({MAX_MESHES} slots less {BUILTIN_MESH_COUNT} built-ins); merge meshes or draw repeated shapes as models"
        )
    }
}

impl std::error::Error for MeshLimitError {}

/// Custom meshes: spline roads and walls (`spline::sweep`), generated or
/// imported meshes, streamed chunk LODs. A world resource; ids follow the
/// built-ins. The renderer uploads them (feature `meshes`) when the revision
/// changes, and only the slots that changed ([`MeshStore`]), so adding,
/// replacing or removing one mesh costs that mesh's bytes, not the library's.
///
/// A slot is live or free. [`MeshLibrary::remove`] frees a slot; the next
/// [`MeshLibrary::try_add`] reuses the lowest free slot, so a game that
/// streams meshes in and out never runs out of ids while fewer than
/// [`MAX_CUSTOM_MESHES`] are live. Ids are stable: nothing moves a live mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshLibrary {
    /// Every slot ever used; a free slot holds an empty mesh.
    meshes: Vec<Mesh>,
    alive: Vec<bool>,
    /// The library revision at which each slot last changed.
    slot_rev: Vec<u64>,
    revision: u64,
}

impl MeshLibrary {
    /// Add a mesh; `None` when [`MAX_CUSTOM_MESHES`] (248) custom meshes are
    /// in use. Use [`MeshLibrary::try_add`] for a message that says so.
    pub fn add(&mut self, m: Mesh) -> Option<MeshId> {
        self.try_add(m).ok()
    }

    /// [`MeshLibrary::add`] with the reason when the library is full. Takes
    /// the lowest free slot (one freed by [`MeshLibrary::remove`]), else the
    /// next new one.
    pub fn try_add(&mut self, m: Mesh) -> Result<MeshId, MeshLimitError> {
        let slot = match self.alive.iter().position(|a| !a) {
            Some(i) => i,
            None if self.meshes.len() < MAX_CUSTOM_MESHES => {
                self.meshes.push(Mesh::default());
                self.alive.push(false);
                self.slot_rev.push(0);
                self.meshes.len() - 1
            }
            None => return Err(MeshLimitError),
        };
        self.revision += 1;
        self.meshes[slot] = m;
        self.alive[slot] = true;
        self.slot_rev[slot] = self.revision;
        Ok(MeshId((BUILTIN_MESH_COUNT + slot) as u16))
    }

    fn live_slot(&self, id: MeshId) -> Option<usize> {
        let i = usize::from(id.0).checked_sub(BUILTIN_MESH_COUNT)?;
        self.alive.get(i).copied().unwrap_or(false).then_some(i)
    }

    /// Replace a custom mesh (false for a built-in, unknown or removed id).
    pub fn replace(&mut self, id: MeshId, m: Mesh) -> bool {
        let Some(i) = self.live_slot(id) else {
            return false;
        };
        self.revision += 1;
        self.meshes[i] = m;
        self.slot_rev[i] = self.revision;
        true
    }

    /// Remove a custom mesh and free its slot and GPU range (false for a
    /// built-in, unknown or already removed id). Instances that still draw
    /// it draw nothing until the slot is reused.
    pub fn remove(&mut self, id: MeshId) -> bool {
        let Some(i) = self.live_slot(id) else {
            return false;
        };
        self.revision += 1;
        self.meshes[i] = Mesh::default();
        self.alive[i] = false;
        self.slot_rev[i] = self.revision;
        true
    }

    /// A live custom mesh by id.
    pub fn get(&self, id: MeshId) -> Option<&Mesh> {
        self.live_slot(id).map(|i| &self.meshes[i])
    }

    /// Custom mesh slots in id order (the first is
    /// `MeshId(BUILTIN_MESH_COUNT)`). A removed mesh's slot holds an empty
    /// mesh until it is reused; see [`MeshLibrary::is_live`].
    pub fn meshes(&self) -> &[Mesh] {
        &self.meshes
    }

    /// Is `id` a custom mesh that is added and not removed?
    pub fn is_live(&self, id: MeshId) -> bool {
        self.live_slot(id).is_some()
    }

    /// Custom meshes added and not removed.
    pub fn live_count(&self) -> usize {
        self.alive.iter().filter(|a| **a).count()
    }

    /// Changes on every add, replace or remove.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The library revision at which custom slot `slot` (0 is
    /// `MeshId(BUILTIN_MESH_COUNT)`) last changed (0: never used). A
    /// consumer that keeps the value it last saw per slot finds exactly the
    /// slots to update.
    pub fn slot_revision(&self, slot: usize) -> u64 {
        self.slot_rev.get(slot).copied().unwrap_or(0)
    }
}

/// Chamfer of [`MeshId::BEVEL_CUBE`], as a fraction of its unit size.
pub const BEVEL: f32 = 0.12;
/// Sides of [`MeshId::CYLINDER`].
pub const CYLINDER_SIDES: u32 = 16;

/// CPU mesh: `[px, py, pz, nx, ny, nz]` vertices and u32 indices.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub vertices: Vec<[f32; 6]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangles(&self) -> u32 {
        (self.indices.len() / 3) as u32
    }
}

/// Every mesh the renderer draws, concatenated for the GPU: little-endian
/// vertex and index bytes, and per mesh slot `(first index, index count,
/// base vertex)` and its triangle count.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshBuffers {
    pub vb: Vec<u8>,
    pub ib: Vec<u8>,
    pub ranges: [(u32, u32, i32); MESH_SLOTS],
    pub tris: [u32; MESH_SLOTS],
}

/// The built-ins, then (feature `meshes`) the custom meshes of `lib` up to
/// [`MESH_SLOTS`]. A custom mesh with an index past its vertices is left
/// empty (drawn as nothing) rather than handed to the GPU.
pub fn mesh_buffers(lib: Option<&MeshLibrary>) -> MeshBuffers {
    let mut b = MeshBuffers {
        vb: Vec::new(),
        ib: Vec::new(),
        ranges: [(0, 0, 0); MESH_SLOTS],
        tris: [0; MESH_SLOTS],
    };
    let (mut verts, mut idx) = (0i32, 0u32);
    let mut add = |i: usize, m: &Mesh, b: &mut MeshBuffers| {
        if m.indices.iter().any(|&x| x as usize >= m.vertices.len()) {
            return;
        }
        for v in &m.vertices {
            for f in v {
                b.vb.extend_from_slice(&f.to_le_bytes());
            }
        }
        for x in &m.indices {
            b.ib.extend_from_slice(&x.to_le_bytes());
        }
        b.ranges[i] = (idx, m.indices.len() as u32, verts);
        b.tris[i] = m.triangles();
        verts += m.vertices.len() as i32;
        idx += m.indices.len() as u32;
    };
    for i in 0..BUILTIN_MESH_COUNT {
        if let Some(m) = builtin(MeshId(i as u16)) {
            add(i, &m, &mut b);
        }
    }
    if let Some(lib) = lib {
        for (j, m) in lib.meshes().iter().enumerate() {
            let i = BUILTIN_MESH_COUNT + j;
            if i < MESH_SLOTS {
                add(i, m, &mut b);
            }
        }
    }
    b
}

/// The built-in mesh for `id` (`None` past [`BUILTIN_MESH_COUNT`]).
pub fn builtin(id: MeshId) -> Option<Mesh> {
    match id {
        MeshId::CUBE => Some(cube()),
        MeshId::SPHERE => Some(sphere(24, 16)),
        MeshId::PLANE => Some(plane()),
        MeshId::BEVEL_CUBE => Some(bevel_cube(BEVEL)),
        MeshId::CYLINDER => Some(cylinder(CYLINDER_SIDES)),
        MeshId::LOW_SPHERE => Some(sphere(10, 6)),
        MeshId::CONE => Some(cone(12)),
        MeshId::DOME => Some(dome(12, 4)),
        _ => None,
    }
}

/// Triangle count of a built-in mesh (0 for unknown ids). Computed once.
pub fn triangles_of(id: MeshId) -> u32 {
    static COUNTS: std::sync::OnceLock<[u32; BUILTIN_MESH_COUNT]> = std::sync::OnceLock::new();
    let counts = COUNTS.get_or_init(|| {
        let mut c = [0; BUILTIN_MESH_COUNT];
        for (i, n) in c.iter_mut().enumerate() {
            *n = builtin(MeshId(i as u16)).map_or(0, |m| m.triangles());
        }
        c
    });
    counts.get(usize::from(id.0)).copied().unwrap_or(0)
}

pub fn cube() -> Mesh {
    let mut m = Mesh::default();
    // (normal, u axis, v axis): u x v = normal, so CCW from outside.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    for (n, u, v) in faces {
        let base = m.vertices.len() as u32;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = |i: usize| 0.5 * (n[i] + su * u[i] + sv * v[i]);
            m.vertices.push([p(0), p(1), p(2), n[0], n[1], n[2]]);
        }
        m.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

pub fn sphere(segments: u32, rings: u32) -> Mesh {
    let (segments, rings) = (segments.max(3), rings.max(2));
    let mut m = Mesh::default();
    for r in 0..=rings {
        let theta = r as f32 / rings as f32 * core::f32::consts::PI;
        let (st, ct) = theta.sin_cos();
        for s in 0..=segments {
            let phi = s as f32 / segments as f32 * core::f32::consts::TAU;
            let (sp, cp) = phi.sin_cos();
            let n = [st * cp, ct, -st * sp];
            m.vertices.push([n[0] * 0.5, n[1] * 0.5, n[2] * 0.5, n[0], n[1], n[2]]);
        }
    }
    let row = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * row + s;
            let b = a + row;
            // At the poles one triangle of the quad has zero area (two of
            // its corners are the pole): leave it out.
            if r > 0 {
                m.indices.extend_from_slice(&[a, b, a + 1]);
            }
            if r + 1 < rings {
                m.indices.extend_from_slice(&[a + 1, b, b + 1]);
            }
        }
    }
    m
}

/// A flat polygon (convex, vertices in cyclic order) as a triangle fan with
/// its own vertices, wound so it faces `n`.
fn flat_polygon(m: &mut Mesh, pts: &[[f32; 3]], n: [f32; 3]) {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    let n = [n[0] / l, n[1] / l, n[2] / l];
    let (a, b, c) = (pts[0], pts[1], pts[2]);
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let face = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let flip = face[0] * n[0] + face[1] * n[1] + face[2] * n[2] < 0.0;
    let base = m.vertices.len() as u32;
    for p in pts {
        m.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
    }
    for i in 1..pts.len() as u32 - 1 {
        if flip {
            m.indices.extend_from_slice(&[base, base + i + 1, base + i]);
        } else {
            m.indices.extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }
}

/// Unit cube with every edge chamfered by `c` (0..0.5): 6 faces, 12 edge
/// strips and 8 corner triangles, all flat shaded (44 triangles).
pub fn bevel_cube(c: f32) -> Mesh {
    let c = c.clamp(0.001, 0.49);
    let mut m = Mesh::default();
    let e = |i: usize, s: f32| {
        let mut v = [0.0f32; 3];
        v[i] = s;
        v
    };
    // The vertex of corner `s` (signs per axis) that lies on the face of
    // axis `i`: full out along `i`, pulled in by the chamfer on the others.
    let corner = |s: [f32; 3], i: usize| {
        let mut p = [s[0] * (0.5 - c), s[1] * (0.5 - c), s[2] * (0.5 - c)];
        p[i] = s[i] * 0.5;
        p
    };
    let signs = [-1.0f32, 1.0];
    for i in 0..3 {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        for si in signs {
            // Face of axis i.
            let pts: Vec<[f32; 3]> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|&(sj, sk)| {
                    let mut s = [0.0; 3];
                    (s[i], s[j], s[k]) = (si, sj, sk);
                    corner(s, i)
                })
                .collect();
            flat_polygon(&mut m, &pts, e(i, si));
            // Edge strips between face i and face j, running along k.
            for sj in signs {
                let at = |sk: f32, axis: usize| {
                    let mut s = [0.0; 3];
                    (s[i], s[j], s[k]) = (si, sj, sk);
                    corner(s, axis)
                };
                let pts = [at(-1.0, i), at(-1.0, j), at(1.0, j), at(1.0, i)];
                let n = [e(i, si), e(j, sj)];
                flat_polygon(&mut m, &pts, [n[0][0] + n[1][0], n[0][1] + n[1][1], n[0][2] + n[1][2]]);
            }
        }
    }
    for sx in signs {
        for sy in signs {
            for sz in signs {
                let s = [sx, sy, sz];
                flat_polygon(&mut m, &[corner(s, 0), corner(s, 1), corner(s, 2)], s);
            }
        }
    }
    m
}

/// Cylinder of radius 0.5 along Y from -0.5 to 0.5: smooth sides, flat
/// caps (`sides` x 4 triangles).
pub fn cylinder(sides: u32) -> Mesh {
    let sides = sides.max(3);
    let mut m = Mesh::default();
    let ring = |i: u32| {
        let a = i as f32 / sides as f32 * core::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        (c, s)
    };
    // Sides: a column of two vertices per angle (the seam repeats one).
    for i in 0..=sides {
        let (x, z) = ring(i);
        m.vertices.push([x * 0.5, -0.5, z * 0.5, x, 0.0, z]);
        m.vertices.push([x * 0.5, 0.5, z * 0.5, x, 0.0, z]);
    }
    for i in 0..sides {
        let (b0, t0, b1, t1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        // Angle grows from +X towards +Z: outward winding is b0, t0, b1.
        m.indices.extend_from_slice(&[b0, t0, b1, b1, t0, t1]);
    }
    // Caps: a centre vertex and a ring each.
    for (y, ny) in [(0.5f32, 1.0f32), (-0.5, -1.0)] {
        let centre = m.vertices.len() as u32;
        m.vertices.push([0.0, y, 0.0, 0.0, ny, 0.0]);
        for i in 0..sides {
            let (x, z) = ring(i);
            m.vertices.push([x * 0.5, y, z * 0.5, 0.0, ny, 0.0]);
        }
        for i in 0..sides {
            let a = centre + 1 + i;
            let b = centre + 1 + (i + 1) % sides;
            if ny > 0.0 {
                m.indices.extend_from_slice(&[centre, b, a]);
            } else {
                m.indices.extend_from_slice(&[centre, a, b]);
            }
        }
    }
    m
}

/// Cone of radius 0.5, base at y = -0.5, tip at y = 0.5: smooth sides (one
/// tip vertex per side so each face keeps its own normal there) and a flat
/// base (`sides` x 2 triangles).
pub fn cone(sides: u32) -> Mesh {
    let sides = sides.max(3);
    let mut m = Mesh::default();
    let ring = |a: f32| {
        let (s, c) = a.sin_cos();
        (c, s)
    };
    let step = core::f32::consts::TAU / sides as f32;
    // Side normal of a cone with radius 0.5 and height 1: (cos, 0.5, sin).
    let k = 1.0 / 1.25f32.sqrt();
    for i in 0..sides {
        let base = m.vertices.len() as u32;
        let (x0, z0) = ring(i as f32 * step);
        let (x1, z1) = ring((i + 1) as f32 * step);
        let (xm, zm) = ring((i as f32 + 0.5) * step);
        m.vertices.push([x0 * 0.5, -0.5, z0 * 0.5, x0 * k, 0.5 * k, z0 * k]);
        m.vertices.push([0.0, 0.5, 0.0, xm * k, 0.5 * k, zm * k]);
        m.vertices.push([x1 * 0.5, -0.5, z1 * 0.5, x1 * k, 0.5 * k, z1 * k]);
        m.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    let centre = m.vertices.len() as u32;
    m.vertices.push([0.0, -0.5, 0.0, 0.0, -1.0, 0.0]);
    for i in 0..sides {
        let (x, z) = ring(i as f32 * step);
        m.vertices.push([x * 0.5, -0.5, z * 0.5, 0.0, -1.0, 0.0]);
    }
    for i in 0..sides {
        m.indices.extend_from_slice(&[centre, centre + 1 + i, centre + 1 + (i + 1) % sides]);
    }
    m
}

/// The top half of a radius 0.5 sphere standing on y = 0, closed by a flat
/// base (`segments` x (2 `rings` - 1) + `segments` triangles).
pub fn dome(segments: u32, rings: u32) -> Mesh {
    let (segments, rings) = (segments.max(3), rings.max(1));
    let mut m = Mesh::default();
    for r in 0..=rings {
        let theta = r as f32 / rings as f32 * core::f32::consts::FRAC_PI_2;
        let (st, ct) = theta.sin_cos();
        for s in 0..=segments {
            let phi = s as f32 / segments as f32 * core::f32::consts::TAU;
            let (sp, cp) = phi.sin_cos();
            let n = [st * cp, ct, -st * sp];
            m.vertices.push([n[0] * 0.5, n[1] * 0.5, n[2] * 0.5, n[0], n[1], n[2]]);
        }
    }
    let row = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * row + s;
            let b = a + row;
            // The pole row's first triangle has no area: leave it out.
            if r > 0 {
                m.indices.extend_from_slice(&[a, b, a + 1]);
            }
            m.indices.extend_from_slice(&[a + 1, b, b + 1]);
        }
    }
    let centre = m.vertices.len() as u32;
    m.vertices.push([0.0, 0.0, 0.0, 0.0, -1.0, 0.0]);
    for s in 0..segments {
        let phi = s as f32 / segments as f32 * core::f32::consts::TAU;
        let (sp, cp) = phi.sin_cos();
        m.vertices.push([cp * 0.5, 0.0, -sp * 0.5, 0.0, -1.0, 0.0]);
    }
    for s in 0..segments {
        // Angle grows from +X towards -Z here: seen from below that is
        // counter-clockwise, so the base faces down.
        m.indices.extend_from_slice(&[centre, centre + 1 + (s + 1) % segments, centre + 1 + s]);
    }
    m
}

pub fn plane() -> Mesh {
    let n = [0.0, 1.0, 0.0];
    Mesh {
        vertices: [(-0.5, 0.5), (0.5, 0.5), (0.5, -0.5), (-0.5, -0.5)]
            .iter()
            .map(|&(x, z)| [x, 0.0, z, n[0], n[1], n[2]])
            .collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle faces the way its vertex normals point.
    fn outward(m: &Mesh) {
        for t in m.indices.as_chunks::<3>().0 {
            let p = |i: u32| {
                let v = m.vertices[i as usize];
                [v[0], v[1], v[2]]
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let vn = m.vertices[t[0] as usize];
            let d = n[0] * vn[3] + n[1] * vn[4] + n[2] * vn[5];
            let area = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!(area < 1e-6 || d > 0.0, "inward triangle {t:?}");
        }
    }

    #[test]
    fn builtins_are_closed_and_outward() {
        let c = cube();
        assert_eq!((c.vertices.len(), c.triangles()), (24, 12));
        outward(&c);
        let s = sphere(24, 16);
        // Two triangles per quad, less the zero-area ones at the poles.
        assert_eq!(s.triangles(), 24 * 16 * 2 - 2 * 24);
        outward(&s);
        for v in &s.vertices {
            let r = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            assert!((r - 0.5).abs() < 1e-5);
        }
        outward(&plane());
        assert_eq!(builtin(MeshId::PLANE).map(|m| m.triangles()), Some(2));
        let b = bevel_cube(BEVEL);
        assert_eq!(b.triangles(), 44);
        outward(&b);
        closed(&b);
        for v in &b.vertices {
            assert!(v[..3].iter().all(|x| x.abs() <= 0.5 + 1e-6));
            // Unit normals (flat shading per face).
            let n = (v[3] * v[3] + v[4] * v[4] + v[5] * v[5]).sqrt();
            assert!((n - 1.0).abs() < 1e-5);
        }
        let y = cylinder(CYLINDER_SIDES);
        assert_eq!(y.triangles(), CYLINDER_SIDES * 4);
        outward(&y);
        closed(&y);
        assert_eq!(triangles_of(MeshId::BEVEL_CUBE), 44);
        assert_eq!(triangles_of(MeshId::CYLINDER), 64);
        let l = builtin(MeshId::LOW_SPHERE).unwrap();
        // 10 x 6 bands less the zero-area pole triangles.
        assert_eq!(l.triangles(), 10 * 6 * 2 - 2 * 10);
        outward(&l);
        let k = cone(12);
        assert_eq!(k.triangles(), 24);
        outward(&k);
        closed(&k);
        let d = dome(12, 4);
        assert_eq!(d.triangles(), 96);
        outward(&d);
        closed(&d);
        assert!(d.vertices.iter().all(|v| v[1] >= -1e-6));
        assert_eq!(triangles_of(MeshId::DOME), 96);
        assert!(builtin(MeshId(BUILTIN_MESH_COUNT as u16)).is_none());
    }

    #[test]
    fn library_ids_follow_the_builtins() {
        let mut lib = MeshLibrary::default();
        let a = lib.add(cube()).unwrap();
        assert_eq!(a, MeshId(BUILTIN_MESH_COUNT as u16));
        assert_eq!(lib.get(a), Some(&cube()));
        assert!(lib.get(MeshId::CUBE).is_none());
        let r = lib.revision();
        assert!(lib.replace(a, plane()));
        assert!(lib.revision() > r);
        assert!(!lib.replace(MeshId::SPHERE, plane()));
        while lib.add(plane()).is_some() {}
        assert_eq!(BUILTIN_MESH_COUNT + lib.meshes().len(), MAX_MESHES);
    }

    #[test]
    fn mesh_buffers_concatenate_builtins_then_custom() {
        let base = mesh_buffers(None);
        let total_tris: u32 = (0..BUILTIN_MESH_COUNT as u16).map(|i| triangles_of(MeshId(i))).sum();
        assert_eq!(base.tris[..BUILTIN_MESH_COUNT].iter().sum::<u32>(), total_tris);
        assert_eq!(base.ib.len() as u32, total_tris * 3 * 4);
        // Ranges are back to back.
        let mut next = 0;
        for &(first, count, _) in &base.ranges[..BUILTIN_MESH_COUNT] {
            assert_eq!(first, next);
            next += count;
        }
        let mut lib = MeshLibrary::default();
        let good = lib.add(cube()).unwrap();
        let bad = lib.add(Mesh {
            vertices: vec![[0.0; 6]; 2],
            indices: vec![0, 1, 7],
        });
        let b = mesh_buffers(Some(&lib));
        let (g, x) = (usize::from(good.0), usize::from(bad.unwrap().0));
        if cfg!(feature = "meshes") {
            assert_eq!(b.ranges[g], (next, 36, (base.vb.len() / 24) as i32));
            assert_eq!(b.tris[g], 12);
            // Out-of-range indices: left empty.
            assert_eq!(b.ranges[x], (0, 0, 0));
            assert_eq!(b.vb.len(), base.vb.len() + cube().vertices.len() * 24);
        } else {
            // Without the feature only the built-ins have slots.
            assert_eq!(MESH_SLOTS, BUILTIN_MESH_COUNT);
            assert!(g >= MESH_SLOTS && x >= MESH_SLOTS);
            assert_eq!(b, base);
        }
    }

    /// Every edge (by position) is shared by exactly two triangles, in
    /// opposite directions: no holes, no flipped faces.
    fn closed(m: &Mesh) {
        use std::collections::HashMap;
        let key = |i: u32| {
            let v = m.vertices[i as usize];
            [(v[0] * 1e4).round() as i32, (v[1] * 1e4).round() as i32, (v[2] * 1e4).round() as i32]
        };
        let mut edges: HashMap<([i32; 3], [i32; 3]), i32> = HashMap::new();
        for t in m.indices.as_chunks::<3>().0 {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((key(a), key(b))).or_default() += 1;
            }
        }
        for ((a, b), n) in &edges {
            assert_eq!(*n, 1, "edge used twice the same way");
            assert_eq!(edges.get(&(*b, *a)), Some(&1), "open edge {a:?} {b:?}");
        }
    }
}
