//! Decoding one tier file (`.glb`) with evani-gltf into the meshes the page
//! draws: one mesh per region colour (`COLOR_0` times the material's base
//! colour), each drawn as one instance in that colour
//! (docs/EVANI_MODEL_TAB.md 4.1, interim drawing until T4). Skinned solids
//! are shown in their stored rest pose: their node transform is ignored, as
//! glTF says for skinned meshes. The model is moved so it stands on y = 0
//! with its footprint centred on the turntable's axis.

#[cfg(feature = "models")]
use std::collections::HashMap;

#[cfg(feature = "models")]
use evani_gltf::Gltf;
use evani_render::mesh::Mesh;

/// A file that does not decode: the evani-gltf error code (MODEL_FORMAT.md
/// "Error codes") or one of this module's, and a line a person can read.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeError {
    pub code: &'static str,
    pub message: String,
}

/// One region colour and its triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Linear RGBA.
    pub color: [f32; 4],
    pub mesh: Mesh,
}

/// A decoded tier.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub groups: Vec<Group>,
    /// Triangles in the file (every triangle primitive).
    pub tris: u32,
    /// Vertices in the file (the `POSITION` counts).
    pub verts: u32,
    /// Bounds after the move onto the ground (y from 0).
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// File size in bytes.
    pub bytes: usize,
}

impl Decoded {
    /// Radius of the bounding sphere around the bounds' centre.
    pub fn radius(&self) -> f32 {
        let d = [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]];
        0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }

    /// The bounds' centre.
    pub fn center(&self) -> [f32; 3] {
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }
}

fn err(code: &'static str, message: impl Into<String>) -> DecodeError {
    DecodeError { code, message: message.into() }
}

/// A triangle soup vertex before grouping.
#[cfg(feature = "models")]
struct Tri {
    key: [u8; 3],
    color: [f32; 4],
    v: [[f32; 6]; 3],
    /// `(primitive, vertex)` of each corner, for sharing vertices.
    ids: [(u32, u32); 3],
}

#[cfg(feature = "models")]
fn quant(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(feature = "models")]
fn normalize(n: [f32; 3]) -> [f32; 3] {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 1e-12 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 1.0, 0.0] }
}

#[cfg(feature = "models")]
fn transform_dir(m: &[f32; 16], d: [f32; 3]) -> [f32; 3] {
    normalize([
        m[0] * d[0] + m[4] * d[1] + m[8] * d[2],
        m[1] * d[0] + m[5] * d[1] + m[9] * d[2],
        m[2] * d[0] + m[6] * d[1] + m[10] * d[2],
    ])
}

/// Decode a `.glb` into at most `max_groups` colour groups (colours are
/// merged more coarsely until they fit the renderer's free mesh slots).
/// Without the `models` module there is no reader: every file is refused.
pub fn decode(glb: &[u8], max_groups: usize) -> Result<Decoded, DecodeError> {
    #[cfg(feature = "models")]
    return decode_glb(glb, max_groups);
    #[cfg(not(feature = "models"))]
    {
        let _ = (glb, max_groups);
        Err(err("no_models_module", "this build has no models module (evani.toml `models = true`)"))
    }
}

#[cfg(feature = "models")]
fn decode_glb(glb: &[u8], max_groups: usize) -> Result<Decoded, DecodeError> {
    let g = Gltf::from_glb(glb).map_err(|e| err(e.0, format!("not a readable model file ({})", e.0)))?;
    let world = g.world_matrices();
    let mut tris: Vec<Tri> = Vec::new();
    let (mut n_tris, mut n_verts) = (0u32, 0u32);
    let mut prim_no = 0u32;
    for (ni, node) in g.nodes.iter().enumerate() {
        let Some(mi) = node.mesh else { continue };
        let Some(mesh) = g.meshes.get(mi) else { continue };
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let m = if node.skin.is_some() { identity } else { world.get(ni).copied().unwrap_or(identity) };
        for p in &mesh.primitives {
            prim_no += 1;
            if p.mode != 4 {
                continue;
            }
            let Some(pa) = p.attribute("POSITION") else { continue };
            let pos = g.read_f32(pa).map_err(|e| err(e.0, format!("positions unreadable ({})", e.0)))?;
            let count = pos.len() / 3;
            n_verts += count as u32;
            let nor = match p.attribute("NORMAL") {
                Some(a) => Some(g.read_f32(a).map_err(|e| err(e.0, format!("normals unreadable ({})", e.0)))?),
                None => None,
            };
            let (col, comps) = match p.attribute("COLOR_0") {
                Some(a) => {
                    let comps = g.accessors.get(a).map_or(4, |x| x.components);
                    (Some(g.read_f32(a).map_err(|e| err(e.0, format!("colours unreadable ({})", e.0)))?), comps)
                }
                None => (None, 4),
            };
            let base = p.material.and_then(|i| g.materials.get(i)).map_or([1.0; 4], |m| m.base_color_factor);
            let idx: Vec<u32> = match p.indices {
                Some(a) => g.read_u32(a).map_err(|e| err(e.0, format!("indices unreadable ({})", e.0)))?,
                None => (0..count as u32).collect(),
            };
            for t in idx.as_chunks::<3>().0 {
                if t.iter().any(|&i| i as usize >= count) {
                    return Err(err("bad_index", "a triangle points past the vertex list"));
                }
                n_tris += 1;
                let corner = |i: u32| -> [f32; 3] {
                    let i = i as usize * 3;
                    evani_gltf::transform_point(&m, [pos[i], pos[i + 1], pos[i + 2]])
                };
                let ps = [corner(t[0]), corner(t[1]), corner(t[2])];
                let face = {
                    let a = [ps[1][0] - ps[0][0], ps[1][1] - ps[0][1], ps[1][2] - ps[0][2]];
                    let b = [ps[2][0] - ps[0][0], ps[2][1] - ps[0][1], ps[2][2] - ps[0][2]];
                    normalize([a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]])
                };
                let normal = |k: usize| -> [f32; 3] {
                    match &nor {
                        Some(n) => {
                            let i = t[k] as usize * 3;
                            match n.get(i..i + 3) {
                                Some(v) => transform_dir(&m, [v[0], v[1], v[2]]),
                                None => face,
                            }
                        }
                        None => face,
                    }
                };
                let c0 = match &col {
                    Some(c) => {
                        let i = t[0] as usize * comps;
                        let r = c.get(i).copied().unwrap_or(1.0);
                        let gg = c.get(i + 1).copied().unwrap_or(1.0);
                        let b = c.get(i + 2).copied().unwrap_or(1.0);
                        [r, gg, b, 1.0]
                    }
                    None => [1.0; 4],
                };
                let color = [c0[0] * base[0], c0[1] * base[1], c0[2] * base[2], 1.0];
                let mut v = [[0.0f32; 6]; 3];
                for k in 0..3 {
                    let n = normal(k);
                    v[k] = [ps[k][0], ps[k][1], ps[k][2], n[0], n[1], n[2]];
                }
                // Flat-normal soups (no NORMAL) never share corners.
                let share = |k: usize| if nor.is_some() { (prim_no, t[k]) } else { (prim_no, u32::MAX - (n_tris * 3 + k as u32)) };
                tris.push(Tri { key: [quant(color[0]), quant(color[1]), quant(color[2])], color, v, ids: [share(0), share(1), share(2)] });
            }
        }
    }
    if tris.is_empty() {
        return Err(err("no_triangles", "the file has no triangles to show"));
    }
    // Bounds, then the move: footprint centred on x = z = 0, lowest point on y = 0.
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for t in &tris {
        for v in &t.v {
            for a in 0..3 {
                min[a] = min[a].min(v[a]);
                max[a] = max[a].max(v[a]);
            }
        }
    }
    let shift = [-0.5 * (min[0] + max[0]), -min[1], -0.5 * (min[2] + max[2])];
    for a in 0..3 {
        min[a] += shift[a];
        max[a] += shift[a];
    }
    // Colour keys, coarser until they fit (deterministic: first-seen order).
    let max_groups = max_groups.max(1);
    let mut bits = 8u32;
    let keyed = loop {
        let mask = if bits >= 8 { 0xffu8 } else { !((1u8 << (8 - bits)) - 1) };
        let mut order: Vec<[u8; 3]> = Vec::new();
        let mut seen: HashMap<[u8; 3], usize> = HashMap::new();
        let mut of: Vec<usize> = Vec::with_capacity(tris.len());
        for t in &tris {
            let k = [t.key[0] & mask, t.key[1] & mask, t.key[2] & mask];
            let gi = *seen.entry(k).or_insert_with(|| {
                order.push(k);
                order.len() - 1
            });
            of.push(gi);
        }
        if order.len() <= max_groups || bits <= 1 {
            break of.into_iter().map(|gi| gi.min(max_groups - 1)).collect::<Vec<usize>>();
        }
        bits -= 1;
    };
    let n_groups = keyed.iter().copied().max().map_or(0, |m| m + 1);
    let mut groups: Vec<Group> = Vec::with_capacity(n_groups);
    let mut maps: Vec<HashMap<(u32, u32), u32>> = vec![HashMap::new(); n_groups];
    for (t, &gi) in tris.iter().zip(&keyed) {
        while groups.len() <= gi {
            groups.push(Group { color: t.color, mesh: Mesh::default() });
        }
        let grp = &mut groups[gi];
        for k in 0..3 {
            let mut v = t.v[k];
            v[0] += shift[0];
            v[1] += shift[1];
            v[2] += shift[2];
            let next = grp.mesh.vertices.len() as u32;
            let id = *maps[gi].entry(t.ids[k]).or_insert(next);
            if id == next {
                grp.mesh.vertices.push(v);
            }
            grp.mesh.indices.push(id);
        }
    }
    Ok(Decoded { groups, tris: n_tris, verts: n_verts, min, max, bytes: glb.len() })
}
