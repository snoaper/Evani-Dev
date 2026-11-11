//! Mesh helpers every asset shares: a builder, height grids, segmented
//! boxes, lathes, transforms, merging and normals. Output is the engine's
//! CPU mesh (`[px, py, pz, nx, ny, nz]`, u32 indices, counter-clockwise
//! from outside), so it goes straight into the mesh library.

pub use evani_render::mesh::Mesh;

/// Push vertices and triangles.
#[derive(Debug, Clone, Default)]
pub struct MeshBuilder {
    pub mesh: Mesh,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex(&mut self, p: [f32; 3], n: [f32; 3]) -> u32 {
        self.mesh.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
        (self.mesh.vertices.len() - 1) as u32
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.mesh.indices.extend_from_slice(&[a, b, c]);
    }

    /// Two triangles `a b c`, `a c d` (counter-clockwise from outside).
    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }

    pub fn finish(self) -> Mesh {
        self.mesh
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] }
}

/// Recompute smooth normals from the triangles (area-weighted).
pub fn smooth_normals(m: &mut Mesh) {
    let mut acc = vec![[0.0f32; 3]; m.vertices.len()];
    for t in m.indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (m.vertices.get(t[0] as usize), m.vertices.get(t[1] as usize), m.vertices.get(t[2] as usize)) else { continue };
        let n = cross(sub([b[0], b[1], b[2]], [a[0], a[1], a[2]]), sub([c[0], c[1], c[2]], [a[0], a[1], a[2]]));
        for &i in t {
            if let Some(s) = acc.get_mut(i as usize) {
                s[0] += n[0];
                s[1] += n[1];
                s[2] += n[2];
            }
        }
    }
    for (v, n) in m.vertices.iter_mut().zip(acc) {
        let n = normalize(n);
        v[3] = n[0];
        v[4] = n[1];
        v[5] = n[2];
    }
}

/// A height grid `size` wide (x, z), centred on the origin, `cells` cells a
/// side, with `height(x, z)`; smooth normals.
pub fn height_grid(size: [f32; 2], cells: u32, height: impl Fn(f32, f32) -> f32) -> Mesh {
    let n = cells.clamp(1, 512);
    let mut b = MeshBuilder::new();
    for j in 0..=n {
        for i in 0..=n {
            let x = (i as f32 / n as f32 - 0.5) * size[0];
            let z = (j as f32 / n as f32 - 0.5) * size[1];
            b.vertex([x, height(x, z), z], [0.0, 1.0, 0.0]);
        }
    }
    let row = n + 1;
    for j in 0..n {
        for i in 0..n {
            let a = j * row + i;
            // Counter-clockwise seen from above (+y).
            b.quad(a, a + row, a + row + 1, a + 1);
        }
    }
    let mut m = b.finish();
    smooth_normals(&mut m);
    m
}

/// A box `size` (x, y, z), its base on y = 0, each face split into
/// `segments` × `segments` quads (flat normals).
pub fn segmented_box(size: [f32; 3], segments: u32) -> Mesh {
    let s = segments.clamp(1, 64);
    let mut b = MeshBuilder::new();
    // (normal, u axis, v axis): u x v = normal.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    for (n, u, v) in faces {
        let base = b.mesh.vertices.len() as u32;
        for j in 0..=s {
            for i in 0..=s {
                let (su, sv) = (i as f32 / s as f32 * 2.0 - 1.0, j as f32 / s as f32 * 2.0 - 1.0);
                let p = |k: usize| 0.5 * (n[k] + su * u[k] + sv * v[k]) * size[k];
                b.vertex([p(0), p(1) + 0.5 * size[1], p(2)], n);
            }
        }
        let row = s + 1;
        for j in 0..s {
            for i in 0..s {
                let a = base + j * row + i;
                b.quad(a, a + 1, a + row + 1, a + row);
            }
        }
    }
    b.finish()
}

/// Turn a profile (`(radius, y)` from bottom to top) round the y axis in
/// `sides` steps; smooth normals. Caps are not added.
pub fn lathe(profile: &[[f32; 2]], sides: u32) -> Mesh {
    let s = sides.clamp(3, 256);
    let mut b = MeshBuilder::new();
    for p in profile {
        for i in 0..=s {
            let a = i as f32 / s as f32 * std::f32::consts::TAU;
            b.vertex([p[0] * a.cos(), p[1], -p[0] * a.sin()], [0.0, 1.0, 0.0]);
        }
    }
    let row = s + 1;
    for j in 0..profile.len().saturating_sub(1) as u32 {
        for i in 0..s {
            let a = j * row + i;
            b.quad(a, a + 1, a + row + 1, a + row);
        }
    }
    let mut m = b.finish();
    smooth_normals(&mut m);
    m
}

/// Scale then move every vertex (normals re-normalised for the scale).
pub fn transform(m: &mut Mesh, scale: [f32; 3], offset: [f32; 3]) {
    for v in &mut m.vertices {
        for k in 0..3 {
            v[k] = v[k] * scale[k] + offset[k];
        }
        let n = normalize([v[3] / scale[0], v[4] / scale[1], v[5] / scale[2]]);
        v[3] = n[0];
        v[4] = n[1];
        v[5] = n[2];
    }
}

/// Append `b` to `a`.
pub fn merge(a: &mut Mesh, b: &Mesh) {
    let base = a.vertices.len() as u32;
    a.vertices.extend_from_slice(&b.vertices);
    a.indices.extend(b.indices.iter().map(|i| i + base));
}

/// The mesh's bounds (min, max); zeros when empty.
pub fn bounds(m: &Mesh) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in &m.vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    if m.vertices.is_empty() { ([0.0; 3], [0.0; 3]) } else { (lo, hi) }
}
