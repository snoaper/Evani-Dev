//! Textured models on the CPU side (feature `models`, S3-3): the [`Models`]
//! resource kits fill each frame, and [`prepare`], which turns a glTF file
//! into what the GPU keeps: 20-byte quantised vertices (so GPU memory
//! matches the download), indices, per-part dequantisation, materials and
//! the list of textures to fetch. Only the maps the toon look samples (base
//! colour, normal, emissive) are ever listed: metallic-roughness and
//! occlusion stay in the material data and cost a toon game nothing.

use evani_core::overlay::RenderStats;
use evani_gltf::{AlphaMode, Error, Gltf, Material, Primitive};

use crate::texture::{DeviceTex, TexFormat, choose_format, set_name};
use crate::world::RenderWorld;

pub mod handle;
pub mod lod;
pub mod stream;
pub mod variant;
#[cfg(feature = "hooks")]
pub mod split;
#[cfg(feature = "hooks")]
pub use split::Split;

pub use handle::ModelHandle;
pub use variant::{Frame, Variant};

pub mod sun; #[cfg(feature = "relief")] pub mod relief; #[cfg(all(feature = "look-surface", not(feature = "relief")))] pub mod surface; #[cfg(all(feature = "material-layers", not(feature = "relief")))] pub mod layers;

/// Bytes per model vertex: position Unorm16x4 over the part's bounding box,
/// normal Snorm8x4, tangent Snorm8x4 (w = handedness, +-127), UV Unorm16x2
/// over the part's UV bounding box. The position's w (bytes 6, 7) and the
/// normal's w (byte 11) carry the baked open-sky cone ([`sun`]; zero until
/// it is baked).
pub const VERTEX_BYTES: usize = 20;
/// Floats per model instance (7 vec4): the world matrix's three rows
/// (instance times node), position scale (w = 1 when the part has
/// tangents), position offset, UV scale (xy) and offset (zw), tint.
pub const INSTANCE_WORDS: usize = 28;
/// Floats per material uniform (5 vec4): base factor; emissive (rgb) and
/// alpha cutoff; flags (mask, double-sided, normal map, normal scale); the
/// two rows of the KHR_texture_transform matrix.
pub const MATERIAL_WORDS: usize = 20;

/// A model in [`Models`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelId(pub u32);

/// One draw of a model this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelInstance {
    pub model: ModelId,
    pub position: [f32; 3],
    /// Rotation quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Linear RGBA multiplied into the base colour (alpha unused).
    pub tint: [f32; 4], #[cfg(feature = "skinning")] pub(crate) joints: u32,
    /// Named nodes this instance does not draw: bit `i` hides every part
    /// under the node [`Models::node_bit`] gave bit `i` (S3-11). Render
    /// only, like everything here.
    pub hide: u32,
    /// This instance's recolours: a range of the frame's paint list, set by
    /// [`Models::draw_with`].
    paints: (u16, u16),
}

impl ModelInstance {
    pub fn new(model: ModelId, position: [f32; 3]) -> Self {
        Self { model, position, rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3], tint: [1.0; 4], hide: 0, paints: (0, 0), #[cfg(feature = "skinning")] joints: 0 }
    }

    pub fn rotation(mut self, q: [f32; 4]) -> Self {
        self.rotation = q;
        self
    }

    pub fn scale(mut self, s: [f32; 3]) -> Self {
        self.scale = s;
        self
    }

    pub fn tint(mut self, t: [f32; 4]) -> Self {
        self.tint = t;
        self
    }
}

/// A texture a drawn material samples: which glTF texture, whether it
/// holds sRGB colour (base, emissive) or linear data (normal), and where it
/// can come from.
#[derive(Clone, Debug, PartialEq)]
pub struct TexNeed {
    pub texture: usize,
    pub srgb: bool,
    /// The core image's bytes when the file embeds it.
    pub image: Option<Vec<u8>>,
    /// The core image's URI when it is a separate file.
    pub uri: Option<String>,
    /// The per-device KTX2 sets, `(format, uri)` (`extras.evani.textures`).
    pub sets: Vec<(String, String)>,
}

/// Where a texture is fetched from on a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexSource<'a> {
    /// A KTX2 set (its URI, relative to the model) in this format.
    Ktx2(&'a str, TexFormat),
    /// The core image, embedded in the file (PNG or JPEG).
    Embedded(&'a [u8]),
    /// The core image as a separate file.
    Uri(&'a str),
    /// Nowhere: the default texture is used.
    None,
}

impl TexNeed {
    /// The set the device can sample (ASTC first, see
    /// [`choose_format`]); else the core image, decoded by the browser;
    /// a KTX2 `rgba` set only when there is no core image.
    pub fn source(&self, dev: DeviceTex) -> TexSource<'_> {
        let uri_of = |name: &str| self.sets.iter().find(|s| s.0 == name).map(|s| s.1.as_str());
        let f = choose_format(self.sets.iter().map(|s| s.0.as_str()), dev);
        if let (false, Some(u)) = (f == TexFormat::Rgba, uri_of(set_name(f))) {
            return TexSource::Ktx2(u, f);
        }
        match (&self.image, &self.uri, uri_of("rgba")) {
            (Some(b), _, _) => TexSource::Embedded(b),
            (None, Some(u), _) => TexSource::Uri(u),
            (None, None, Some(u)) => TexSource::Ktx2(u, TexFormat::Rgba),
            _ => TexSource::None,
        }
    }
}

/// One drawn primitive.
#[derive(Clone, Debug)]
pub struct Part {
    /// [`VERTEX_BYTES`] per vertex.
    pub vertices: Vec<u8>,
    pub indices: Vec<u32>,
    /// The node's world matrix in model space (column-major).
    pub node: [f32; 16],
    /// Position = offset + scale * unorm16 (per axis), where unorm16 is the
    /// GPU's `Unorm16` reading, raw / 65535 (0 to 1): the scale is the box's
    /// whole extent.
    pub pos_scale: [f32; 3],
    pub pos_offset: [f32; 3],
    /// UV = offset (zw) + scale (xy) * unorm16 (raw / 65535, as above).
    pub uv: [f32; 4],
    /// Index into [`Prepared::materials`].
    pub material: usize,
    /// The file gives tangents, so a normal map can be used.
    pub tangents: bool,
    /// `[node, mesh, primitive]` it came from.
    pub source: [usize; 3],
    /// The LOD level it belongs to (`extras.evani.lods` index), or
    /// [`SHARED`] when it is outside every level and drawn by all of them.
    pub lod: u8, #[cfg(feature = "skinning")] pub skin: Vec<u8>,
    /// Bit `i` is set when the part is under the node named
    /// [`Prepared::names`]`[i]` (or is that node): what hiding and
    /// recolouring a named node reach.
    pub tags: u32,
}

/// [`Part::lod`] of a part every level draws.
pub const SHARED: u8 = u8::MAX;

/// A material as the model pipeline reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelMaterial {
    pub words: [f32; MATERIAL_WORDS],
    /// Base, normal and emissive maps: indices into [`Prepared::textures`].
    pub maps: [Option<usize>; 3], #[cfg(feature = "relief")] pub relief: relief::Relief, #[cfg(all(feature = "look-surface", not(feature = "relief")))] pub surface: surface::Surface, #[cfg(all(feature = "material-layers", not(feature = "relief")))] pub layers: [f32; evani_material_layers::LAYER_WORDS],
}

/// A model ready for upload.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub parts: Vec<Part>,
    pub materials: Vec<ModelMaterial>,
    pub textures: Vec<TexNeed>,
    /// Triangles at full detail (level 0 and the shared parts).
    pub triangles: u32,
    /// Each level's `min_screen`, most detailed first (empty: one level).
    pub lods: Vec<f32>,
    /// Triangles and parts of each level's own parts (not the shared ones).
    pub lod_triangles: Vec<u32>,
    pub lod_parts: Vec<u32>,
    /// Bounding sphere in model space (what the LOD choice projects).
    pub centre: [f32; 3],
    pub radius: f32, #[cfg(feature = "skinning")] pub bind: Vec<[f32; 12]>,
    /// The names of the nodes parts can be hidden or recoloured by (at most
    /// 32, sorted: a name's bit is its place, the same in every tier file
    /// that has the same nodes).
    pub names: Vec<String>,
    /// The file's `anchor:<name>` nodes, sorted by name, with their world
    /// matrices (column-major, model space).
    pub anchors: Vec<(String, [f32; 16])>,
}

fn unorm16(v: f32, lo: f32, ext: f32) -> u16 {
    if ext > 0.0 { ((v - lo) / ext * 65535.0).round().clamp(0.0, 65535.0) as u16 } else { 0 }
}

fn snorm8(v: f32) -> u8 {
    (v.clamp(-1.0, 1.0) * 127.0).round() as i8 as u8
}

/// Per-component minimum and extent of `K`-component values.
fn bounds<const K: usize>(v: &[f32]) -> ([f32; K], [f32; K]) {
    let mut lo = [f32::MAX; K];
    let mut hi = [f32::MIN; K];
    for c in v.as_chunks::<K>().0 {
        for k in 0..K {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    if v.len() < K {
        return ([0.0; K], [0.0; K]);
    }
    let mut ext = [0.0; K];
    for k in 0..K {
        ext[k] = hi[k] - lo[k];
    }
    (lo, ext)
}

/// An accessor as floats, refused unless it has `comps` components.
fn read(g: &Gltf, a: usize, comps: usize, n: usize) -> Result<Vec<f32>, Error> {
    match g.accessors.get(a) {
        Some(x) if x.components == comps && (n == usize::MAX || x.count == n) => g.read_f32(a),
        _ => Err(Error("model: an attribute has the wrong shape")),
    }
}

/// Area-weighted vertex normals, for a primitive without NORMAL.
fn smooth_normals(pos: &[f32], idx: &[u32]) -> Vec<f32> {
    let mut n = vec![0.0f32; pos.len()];
    for t in idx.as_chunks::<3>().0 {
        let p = |i: u32| {
            let i = i as usize * 3;
            [pos[i], pos[i + 1], pos[i + 2]]
        };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                n[i as usize * 3 + k] += f[k];
            }
        }
    }
    for c in n.as_chunks_mut::<3>().0 {
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-12);
        c.iter_mut().for_each(|x| *x /= l);
    }
    n
}

fn part(g: &Gltf, p: &Primitive, node: [f32; 16], material: usize, source: [usize; 3]) -> Result<Option<Part>, Error> {
    let Some(pa) = p.attribute("POSITION").filter(|_| p.mode == 4) else {
        return Ok(None);
    };
    let pos = read(g, pa, 3, usize::MAX)?;
    let n = pos.len() / 3;
    let indices = match p.indices {
        Some(i) => g.read_u32(i)?,
        None => (0..n as u32).collect(),
    };
    if indices.len() % 3 != 0 || indices.iter().any(|&i| i as usize >= n) {
        return Err(Error("model: bad triangle indices"));
    }
    let normals = match p.attribute("NORMAL") {
        Some(a) => read(g, a, 3, n)?,
        None => smooth_normals(&pos, &indices),
    };
    let tangents = p.attribute("TANGENT").map(|a| read(g, a, 4, n)).transpose()?;
    let uvs = p.attribute("TEXCOORD_0").map(|a| read(g, a, 2, n)).transpose()?;
    let (plo, pext) = bounds::<3>(&pos);
    let (ulo, uext) = uvs.as_deref().map_or(([0.0; 2], [0.0; 2]), bounds::<2>);
    // Each vertex written in place (one allocation, no per-field growth).
    let mut v = vec![0u8; n * VERTEX_BYTES];
    for (i, o) in v.as_chunks_mut::<VERTEX_BYTES>().0.iter_mut().enumerate() {
        for k in 0..3 {
            o[k * 2..k * 2 + 2].copy_from_slice(&unorm16(pos[i * 3 + k], plo[k], pext[k]).to_le_bytes());
        }
        for k in 0..3 {
            o[8 + k] = snorm8(normals[i * 3 + k]);
        }
        if let Some(t) = &tangents {
            for k in 0..3 {
                o[12 + k] = snorm8(t[i * 4 + k]);
            }
            o[15] = if t[i * 4 + 3] < 0.0 { 129 } else { 127 };
        }
        for k in 0..2 {
            o[16 + k * 2..18 + k * 2].copy_from_slice(&unorm16(uvs.as_ref().map_or(0.0, |u| u[i * 2 + k]), ulo[k], uext[k]).to_le_bytes());
        }
    }
    Ok(Some(Part {
        vertices: v,
        indices,
        node,
        pos_scale: pext,
        pos_offset: plo,
        uv: [uext[0], uext[1], ulo[0], ulo[1]],
        material,
        tangents: tangents.is_some(),
        source,
        lod: SHARED, #[cfg(feature = "skinning")] skin: Vec::new(),
        tags: 0,
    }))
}

/// List texture `texture` (once per colour space) if it has any source.
fn need(g: &Gltf, texture: usize, srgb: bool, list: &mut Vec<TexNeed>) -> Option<usize> {
    let t = g.textures.get(texture)?;
    if let Some(i) = list.iter().position(|n| n.texture == texture && n.srgb == srgb) {
        return Some(i);
    }
    let sets = g
        .evani
        .as_ref()
        .and_then(|e| e.textures.iter().find(|s| s.texture == texture))
        .map(|s| s.sets.clone())
        .unwrap_or_default();
    let image = t.source.and_then(|i| g.image_bytes(i).ok().flatten()).map(<[u8]>::to_vec);
    let uri = t.source.and_then(|i| g.images.get(i)).and_then(|im| im.uri.clone());
    if image.is_none() && uri.is_none() && sets.is_empty() {
        return None;
    }
    list.push(TexNeed { texture, srgb, image, uri, sets });
    Some(list.len() - 1)
}

/// The model pipeline's view of a material (`None`: plain white). Blend
/// materials draw as opaque (no sorted transparency yet).
fn material(g: &Gltf, m: Option<&Material>, textures: &mut Vec<TexNeed>) -> ModelMaterial {
    let mut w = [0.0f32; MATERIAL_WORDS];
    let mut maps = [None; 3];
    w[..4].copy_from_slice(&[1.0; 4]);
    let mut tt = None;
    if let Some(m) = m {
        w[..4].copy_from_slice(&m.base_color_factor);
        w[4..7].copy_from_slice(&m.emissive_factor);
        w[7] = m.alpha_cutoff;
        w[8] = f32::from(u8::from(m.alpha_mode == AlphaMode::Mask));
        w[9] = f32::from(u8::from(m.double_sided));
        // Only the maps the toon look samples (base, normal, emissive).
        for (k, (r, srgb)) in [(&m.base_color_texture, true), (&m.normal_texture, false), (&m.emissive_texture, true)].into_iter().enumerate() {
            if let Some(r) = r {
                maps[k] = need(g, r.texture, srgb, textures);
                tt = tt.or(r.transform.clone());
            }
        }
        w[10] = f32::from(u8::from(maps[1].is_some()));
        w[11] = m.normal_texture.as_ref().map_or(1.0, |r| r.amount);
    }
    // KHR_texture_transform (the first map's; gltfpack writes the same on
    // every map): translation * rotation * scale, as two rows.
    let (o, r, s) = tt.map_or(([0.0; 2], 0.0, [1.0; 2]), |t| (t.offset, t.rotation, t.scale));
    let (sn, cs) = r.sin_cos();
    w[12..15].copy_from_slice(&[cs * s[0], -sn * s[1], o[0]]);
    w[16..19].copy_from_slice(&[sn * s[0], cs * s[1], o[1]]); #[cfg(feature = "relief")] let rel = relief::material(g, m, textures); #[cfg(all(feature = "look-surface", not(feature = "relief")))] let srf = surface::material(g, m, textures); #[cfg(all(feature = "material-layers", not(feature = "relief")))] let lyr = layers::material(m);
    ModelMaterial { words: w, maps, #[cfg(feature = "relief")] relief: rel, #[cfg(all(feature = "look-surface", not(feature = "relief")))] surface: srf, #[cfg(all(feature = "material-layers", not(feature = "relief")))] layers: lyr }
}

/// Turn a glTF file into upload-ready parts. Drawn: every triangle
/// primitive of every node with a mesh, except skinned ones (the anim
/// module's) and the collision proxies. Parts under a LOD node
/// (`extras.evani.lods`) are tagged with that level; the draw picks one
/// level per instance ([`lod`]).
pub fn prepare(g: &Gltf) -> Result<Prepared, Error> {
    let world = g.world_matrices();
    let hide: Vec<usize> = g.collision.into_iter().collect();
    let lod_nodes: Vec<Option<usize>> = g.evani.as_ref().map(|e| e.lods.iter().take(usize::from(SHARED)).map(|l| l.node_index).collect()).unwrap_or_default();
    // `None`: hidden; else the level (or SHARED).
    let level = |mut n: usize| {
        let mut lvl = SHARED;
        for _ in 0..=g.nodes.len() {
            if hide.contains(&n) {
                return None;
            }
            if lvl == SHARED && let Some(k) = lod_nodes.iter().position(|&l| l == Some(n)) {
                lvl = k as u8;
            }
            match g.parent(n) {
                Some(p) => n = p,
                None => return Some(lvl),
            }
        }
        None
    };
    let levels = lod_nodes.len();
    let mut out = Prepared {
        parts: Vec::new(),
        materials: Vec::new(),
        textures: Vec::new(),
        triangles: 0,
        lods: match &g.evani {
            Some(e) if levels > 1 => e.lods.iter().take(levels).map(|l| l.min_screen).collect(),
            _ => Vec::new(),
        },
        lod_triangles: vec![0; levels],
        lod_parts: vec![0; levels],
        centre: [0.0; 3],
        radius: 0.0, #[cfg(feature = "skinning")] bind: Vec::new(),
        names: Vec::new(),
        anchors: g.anchors.iter().map(|a| (a.name.clone(), a.world)).collect(),
    };
    // A stable insertion sort by name, not the standard sort: a model has a
    // handful of anchors, and each standard sort costs kilobytes of wasm.
    for k in 1..out.anchors.len() {
        let mut j = k;
        while j > 0 && out.anchors[j].0 < out.anchors[j - 1].0 {
            out.anchors.swap(j - 1, j);
            j -= 1;
        }
    }
    let mut mat_of = vec![None; g.materials.len() + 1];
    for (ni, node) in g.nodes.iter().enumerate() {
        let Some(mesh) = node.mesh.and_then(|m| g.meshes.get(m).map(|x| (m, x))) else {
            continue;
        };
        let Some(lvl) = level(ni).filter(|_| node.skin.is_none()) else {
            continue;
        };
        for (pi, p) in mesh.1.primitives.iter().enumerate() {
            let mi = p.material.filter(|&m| m < g.materials.len()).unwrap_or(g.materials.len());
            let mat = match mat_of[mi] {
                Some(i) => i,
                None => {
                    out.materials.push(material(g, g.materials.get(mi), &mut out.textures));
                    mat_of[mi] = Some(out.materials.len() - 1);
                    out.materials.len() - 1
                }
            };
            if let Some(mut pt) = part(g, p, world[ni], mat, [ni, mesh.0, pi])? {
                let tris = pt.indices.len() as u32 / 3;
                pt.lod = lvl;
                if let Some(k) = Some(usize::from(lvl)).filter(|&k| k < levels) {
                    out.lod_triangles[k] = out.lod_triangles[k].saturating_add(tris);
                    out.lod_parts[k] += 1;
                }
                if lvl == SHARED || lvl == 0 {
                    out.triangles = out.triangles.saturating_add(tris);
                }
                out.parts.push(pt);
            }
        }
    }
    // The sun cones are not baked here: only the tiers without a shadow map
    // read them, so the GPU side bakes them over frames when one draws the
    // model ([`sun::Bake`]); a PC on Medium and up never pays for it.
    (out.centre, out.radius) = sphere(&out.parts); #[cfg(feature = "skinning")] skin::add(g, &world, &level, &mut mat_of, &mut out)?;
    // Named nodes: every name on a part's way up to the root, sorted, the
    // first 32; a part's tags are the places of its own.
    let names_of = |p: &Part| core::iter::successors(Some(p.source[0]), |&k| g.parent(k)).take(g.nodes.len() + 1).filter_map(|k| g.nodes.get(k)).map(|n| n.name.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
    // Sorted and unique by insertion: the same list as sort plus dedup,
    // without linking a standard sort for strings.
    for n in out.parts.iter().flat_map(names_of) {
        if let Err(i) = out.names.binary_search_by(|x| x.as_str().cmp(n)) {
            out.names.insert(i, n.to_owned());
        }
    }
    out.names.truncate(32);
    for p in &mut out.parts {
        p.tags = names_of(p).into_iter().filter_map(|n| out.names.binary_search_by(|x| x.as_str().cmp(n)).ok()).fold(0, |t, b| t | 1 << b);
    }
    Ok(out)
}

/// A bounding sphere of every part (model space): the centre of their
/// boxes' union and half its diagonal.
fn sphere(parts: &[Part]) -> ([f32; 3], f32) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in parts {
        for c in 0..8 {
            let l = [0, 1, 2].map(|k| p.pos_offset[k] + if (c >> k) & 1 == 1 { p.pos_scale[k] } else { 0.0 });
            let m = &p.node;
            for k in 0..3 {
                let w = m[k] * l[0] + m[4 + k] * l[1] + m[8 + k] * l[2] + m[12 + k];
                lo[k] = lo[k].min(w);
                hi[k] = hi[k].max(w);
            }
        }
    }
    if parts.is_empty() {
        return ([0.0; 3], 0.0);
    }
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    let d = [0, 1, 2].map(|k| hi[k] - lo[k]);
    (c, 0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
}

/// The world matrix (column-major) of translation * rotation * scale.
pub fn trs([x, y, z, w]: [f32; 4], s: [f32; 3], p: [f32; 3]) -> [f32; 16] {
    [
        (1.0 - 2.0 * (y * y + z * z)) * s[0],
        2.0 * (x * y + z * w) * s[0],
        2.0 * (x * z - y * w) * s[0],
        0.0,
        2.0 * (x * y - z * w) * s[1],
        (1.0 - 2.0 * (x * x + z * z)) * s[1],
        2.0 * (y * z + x * w) * s[1],
        0.0,
        2.0 * (x * z + y * w) * s[2],
        2.0 * (y * z - x * w) * s[2],
        (1.0 - 2.0 * (x * x + y * y)) * s[2],
        0.0,
        p[0],
        p[1],
        p[2],
        1.0,
    ]
}

/// The instance words of `part` drawn as `inst` ([`INSTANCE_WORDS`]), its
/// tint multiplied by each of `paints` that reaches the part.
pub fn instance_words(inst: &ModelInstance, part: &Part, paints: &[(u32, [f32; 4])], out: &mut Vec<f32>) {
    let m = evani_gltf::mul(&trs(inst.rotation, inst.scale, inst.position), &part.node);
    for r in 0..3 {
        out.extend([m[r], m[4 + r], m[8 + r], m[12 + r]]);
    }
    out.extend(part.pos_scale);
    out.push(f32::from(u8::from(part.tangents)));
    out.extend(part.pos_offset);
    out.push(0.0);
    out.extend(part.uv);
    out.extend(paints.iter().filter(|p| p.0 & part.tags != 0).fold(inst.tint, |t, p| [0, 1, 2, 3].map(|k| t[k] * p.1[k])));
}

/// A texture URI as fetched: absolute, root-relative and `data:` URIs as
/// they are, relative ones under `folder` (the model's URL up to its last
/// `/`).
pub fn resolve_uri(folder: &str, uri: &str) -> String {
    // Byte checks, not `str::contains`: a str pattern pulls the two-way
    // searcher (about 1.6 KB) into the download.
    let b = uri.as_bytes();
    if b.first() == Some(&b'/') || b.starts_with(b"data:") || b.windows(3).any(|w| w == b"://") {
        uri.to_owned()
    } else {
        format!("{folder}{uri}")
    }
}

/// A model's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelState {
    /// Fetching the file or its textures, or uploading.
    Loading,
    /// Drawn when asked.
    Ready,
    /// Not drawn; the reason is in the error log.
    Failed(String),
}

#[derive(Debug)]
struct Entry {
    url: String,
    state: ModelState,
    parts: u32,
    triangles: u32,
    /// The file, for a model given in memory (re-read after a GPU device
    /// loss; fetched models are fetched again).
    glb: Option<std::rc::Rc<[u8]>>,
    /// The LOD data of [`Prepared`]: thresholds, each level's own parts and
    /// triangles, the shared parts and triangles, the bounding sphere.
    lods: Vec<f32>,
    lod_parts: Vec<u32>,
    lod_triangles: Vec<u32>,
    shared: (u32, u32),
    sphere: ([f32; 3], f32),
    /// Last frame's level of the model's n-th draw (its draw order), for
    /// the hysteresis.
    memory: Vec<u8>,
    /// [`Prepared::names`] and [`Prepared::anchors`].
    names: Vec<String>,
    anchors: Vec<(String, [f32; 16])>,
    /// Each part's `(tags, lod, triangles)`, for the native counts.
    info: Vec<(u32, u8, u32)>,
}

/// The models a game loaded, and this frame's draws (cleared every frame).
/// A model draws only once its file and every texture it samples are on
/// the GPU; until then a kit draws its own placeholder ([`Models::ready`]).
#[derive(Debug, Default)]
pub struct Models {
    entries: Vec<Entry>,
    /// Files to fetch (the GPU side takes them).
    pub(crate) fetch: Vec<ModelId>,
    /// Parsed models waiting for upload.
    pub(crate) pending: Vec<(ModelId, Prepared)>,
    pub draws: Vec<ModelInstance>,
    /// This frame's LOD level of each draw ([`Models::select_lods`]).
    pub(crate) levels: Vec<u8>,
    /// This frame's recolours, `(node bits, colour)`, in the order the draws
    /// gave them ([`Models::draw_with`]).
    paints: Vec<(u32, [f32; 4])>,
    /// Model handles and the tiered stream ([`handle`]).
    pub(crate) stream: handle::Streaming, #[cfg(feature = "skinning")] pub(crate) skin: skin::Poses, #[cfg(feature = "importance")] pub(crate) importance: crate::importance::Log, #[cfg(feature = "view-model-models")] pub(crate) vm: vm::VmDraws,
    /// The live before/after split (S3-14, `hooks` only): two models
    /// share the frame, either side of a vertical line ([`split`]).
    #[cfg(feature = "hooks")]
    pub split: Option<Split>,
}

impl Models {
    fn push(&mut self, url: &str) -> ModelId {
        self.entries.push(Entry {
            url: url.to_owned(),
            state: ModelState::Loading,
            parts: 0,
            triangles: 0,
            glb: None,
            lods: Vec::new(),
            lod_parts: Vec::new(),
            lod_triangles: Vec::new(),
            shared: (0, 0),
            sphere: ([0.0; 3], 0.0),
            memory: Vec::new(),
            names: Vec::new(),
            anchors: Vec::new(),
            info: Vec::new(),
        });
        ModelId(self.entries.len() as u32 - 1)
    }

    /// Fetch a `.glb` (relative to the page) and draw it once it is ready.
    pub fn load(&mut self, url: &str) -> ModelId {
        let id = self.push(url);
        self.fetch.push(id);
        id
    }

    /// A `.glb` already in memory; `url` is where its texture files are
    /// relative to.
    pub fn add_glb(&mut self, url: &str, glb: &[u8]) -> ModelId {
        let id = self.push(url);
        if let Some(e) = self.entries.last_mut() {
            e.glb = Some(glb.into());
        }
        self.loaded(id, glb);
        id
    }

    /// A new GPU device has none of the models: queue every model that has
    /// not failed again (in-memory ones are re-read, the rest re-fetched).
    /// Only the GPU side (browser) calls it.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn reload_all(&mut self) {
        self.fetch.clear();
        self.pending.clear();
        for i in 0..self.entries.len() {
            let id = ModelId(i as u32);
            if matches!(self.entries[i].state, ModelState::Failed(_)) {
                continue;
            }
            self.entries[i].state = ModelState::Loading;
            match self.entries[i].glb.clone() {
                Some(b) => self.loaded(id, &b),
                None => self.fetch.push(id),
            }
        }
    }

    /// Models in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The file's bytes arrived: parse and prepare it.
    pub(crate) fn loaded(&mut self, id: ModelId, glb: &[u8]) { #[cfg(feature = "level-groups")] if self.stream.levels.is_held(id) { return; }
        match Gltf::from_glb(glb).and_then(|g| prepare(&g)) {
            Ok(mut p) => {
                if let Some(e) = self.entries.get_mut(id.0 as usize) {
                    e.names = core::mem::take(&mut p.names);
                    e.anchors = core::mem::take(&mut p.anchors);
                    e.info = p.parts.iter().map(|x| (x.tags, x.lod, x.indices.len() as u32 / 3)).collect();
                    (e.parts, e.triangles) = (p.parts.len() as u32, p.triangles);
                    let own: u32 = p.lod_parts.iter().sum();
                    let own_tris: u32 = p.lod_triangles.iter().fold(0, |a, &t| a.saturating_add(t));
                    let all_tris = p.parts.iter().fold(0u32, |a, x| a.saturating_add(x.indices.len() as u32 / 3));
                    e.shared = (e.parts.saturating_sub(own), all_tris.saturating_sub(own_tris));
                    (e.lods, e.lod_parts, e.lod_triangles, e.sphere) = (p.lods.clone(), p.lod_parts.clone(), p.lod_triangles.clone(), (p.centre, p.radius));
                    e.memory.clear();
                }
                self.pending.push((id, p));
            }
            Err(e) => self.set_state(id, ModelState::Failed(e.0.to_owned())),
        }
    }

    pub(crate) fn set_state(&mut self, id: ModelId, s: ModelState) {
        if let Some(e) = self.entries.get_mut(id.0 as usize) {
            e.state = s;
        }
    }

    pub fn state(&self, id: ModelId) -> Option<&ModelState> {
        self.entries.get(id.0 as usize).map(|e| &e.state)
    }

    pub fn ready(&self, id: ModelId) -> bool {
        self.state(id) == Some(&ModelState::Ready)
    }

    pub fn url(&self, id: ModelId) -> Option<&str> {
        self.entries.get(id.0 as usize).map(|e| e.url.as_str())
    }

    /// Triangles one instance of the model draws (0 until it is read).
    pub fn triangles(&self, id: ModelId) -> u32 {
        self.entries.get(id.0 as usize).map_or(0, |e| e.triangles)
    }

    /// Draw a model this frame (nothing while it is not ready).
    pub fn draw(&mut self, i: ModelInstance) {
        if self.draws.is_empty() {
            self.paints.clear();
        }
        self.draws.push(i);
    }

    /// [`Models::draw`] with recolours: each `(node bits, colour)` multiplies
    /// the base colour of the parts under those nodes ([`Models::node_bit`];
    /// white parts take the colour whole). Hide nodes through
    /// `i.hide`. Nothing is allocated once the frame's lists have grown.
    pub fn draw_with(&mut self, mut i: ModelInstance, paints: &[(u32, [f32; 4])]) {
        if self.draws.is_empty() {
            self.paints.clear();
        }
        let at = self.paints.len();
        if at + paints.len() <= usize::from(u16::MAX) {
            i.paints = (at as u16, paints.len() as u16);
            self.paints.extend_from_slice(paints);
        }
        self.draws.push(i);
    }

    /// The bit of the node named `name` in model `id` (for
    /// [`ModelInstance::hide`] and [`Models::draw_with`]); `None` when the
    /// model is not read yet or has no such node. Resolve once, not per frame.
    pub fn node_bit(&self, id: ModelId, name: &str) -> Option<u32> {
        let n = &self.entries.get(id.0 as usize)?.names;
        n.binary_search_by(|x| x.as_str().cmp(name)).ok().map(|b| 1 << b)
    }

    /// The index of the `anchor:<name>` node of model `id` (for
    /// [`Models::anchor_frame`]); `None` as for [`Models::node_bit`].
    pub fn anchor(&self, id: ModelId, name: &str) -> Option<u16> {
        let a = &self.entries.get(id.0 as usize)?.anchors;
        a.binary_search_by(|x| x.0.as_str().cmp(name)).ok().map(|b| b as u16)
    }

    /// Where anchor `anchor` of model `id` is when the model stands at `at`:
    /// the world frame to attach a part to.
    pub fn anchor_frame(&self, id: ModelId, anchor: u16, at: &Frame) -> Option<Frame> {
        let a = self.entries.get(id.0 as usize)?.anchors.get(usize::from(anchor))?;
        Some(Frame::from_matrix(&evani_gltf::mul(&trs(at.rotation, at.scale, at.position), &a.1)))
    }

    /// The paints of draw `d`.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn paints_of(&self, d: &ModelInstance) -> &[(u32, [f32; 4])] {
        let a = usize::from(d.paints.0);
        self.paints.get(a..a + usize::from(d.paints.1)).unwrap_or_default()
    }

    /// Append the instance words of `p` (a part of model `mi`) for every
    /// draw that shows it: the draw's level reaches it and the draw does not
    /// hide a node it is under.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn stage(&self, mi: usize, p: &Part, out: &mut Vec<f32>) {
        for (k, d) in self.draws.iter().enumerate() {
            if d.model.0 as usize == mi && (p.lod == SHARED || p.lod == self.level(k)) && d.hide & p.tags == 0 {
                instance_words(d, p, self.paints_of(d), out); #[cfg(feature = "skinning")] skin::set_joints(out, self.skin.offsets.get(k).copied().unwrap_or(0));
            }
        }
    }

    /// Native counts for model `i` when a draw hides nodes: the draw calls
    /// and triangles of one view, part by part as [`Models::stage`] decides.
    fn visible(&self, i: usize) -> (u32, u32) {
        let (mut calls, mut tris) = (0u32, 0u32);
        for &(tags, lod, t) in &self.entries[i].info {
            let n = self.draws.iter().enumerate().filter(|(k, d)| d.model.0 as usize == i && (lod == SHARED || lod == self.level(*k)) && d.hide & tags == 0).count() as u32;
            calls += u32::from(n > 0);
            tris = tris.saturating_add(t.saturating_mul(n));
        }
        (calls, tris)
    }

    /// Pick this frame's LOD level of every draw from its projected size
    /// in the views (the largest), with hysteresis against the level the
    /// model's draw of the same order had last frame.
    pub fn select_lods(&mut self, rw: &RenderWorld) {
        let many: Vec<evani_core::camera::CameraPose>;
        let cams = if rw.views.is_empty() {
            core::slice::from_ref(&rw.camera)
        } else {
            many = rw.views.iter().map(|v| v.camera).collect();
            &many
        };
        self.levels.clear();
        let mut order = vec![0usize; self.entries.len()];
        for d in &self.draws {
            let i = d.model.0 as usize; #[cfg(feature = "importance")] if let Some(e) = self.entries.get(i) { self.importance.note(&e.url, e.state == ModelState::Ready, e.sphere, d, cams); }
            let (Some(e), Some(n)) = (self.entries.get_mut(i), order.get_mut(i)) else {
                self.levels.push(0);
                continue;
            };
            if e.lods.len() < 2 {
                self.levels.push(0);
                continue;
            }
            let (c, r) = e.sphere;
            let s = d.scale;
            let off = lod::rotate(d.rotation, [c[0] * s[0], c[1] * s[1], c[2] * s[2]]);
            let centre = [d.position[0] + off[0], d.position[1] + off[1], d.position[2] + off[2]];
            let size = lod::largest_fraction(centre, r * s[0].abs().max(s[1].abs()).max(s[2].abs()), cams);
            let lvl = lod::choose(e.memory.get(*n).copied(), size, &e.lods);
            if e.memory.len() <= *n {
                e.memory.resize(*n + 1, lvl);
            }
            e.memory[*n] = lvl;
            *n += 1;
            self.levels.push(lvl);
        }
    }

    /// The LOD level of draw `i` this frame (after [`Models::select_lods`]).
    pub fn level(&self, i: usize) -> u8 {
        self.levels.get(i).copied().unwrap_or(0)
    }
}

/// Native builds: what the GPU would draw, then clear the draws. Handles
/// follow the active `tier` (`None`: not settled). Per model: one draw per
/// shared part per view and one per part of each level any instance uses;
/// each instance's triangles are the shared ones plus its level's. There
/// is no upload natively, so a prepared model is ready at once.
pub fn count_draws(models: &mut Models, rw: &RenderWorld, tier: Option<&str>, stats: &mut RenderStats) {
    let views = rw.view_count() as u32;
    if let Some(t) = tier {
        models.sync_tier(t);
    }
    for (id, _) in core::mem::take(&mut models.pending) {
        models.set_state(id, ModelState::Ready);
    }
    models.select_lods(rw);
    for (i, e) in models.entries.iter().enumerate() {
        if e.state != ModelState::Ready {
            continue;
        }
        if models.draws.iter().any(|d| d.model.0 as usize == i && d.hide != 0) {
            let (calls, tris) = models.visible(i);
            stats.draw_calls += calls * views;
            stats.triangles = stats.triangles.saturating_add(tris.saturating_mul(views));
            continue;
        }
        let mut used = 0u64;
        let mut n = 0u32;
        for (k, d) in models.draws.iter().enumerate() {
            if d.model.0 as usize == i {
                let l = models.level(k);
                n += 1;
                used |= 1 << l.min(63);
                let t = e.shared.1.saturating_add(e.lod_triangles.get(usize::from(l)).copied().unwrap_or(0));
                stats.triangles = stats.triangles.saturating_add(t.saturating_mul(views));
            }
        }
        if n > 0 {
            let own: u32 = e.lod_parts.iter().enumerate().filter(|(k, _)| used & (1 << (*k).min(63)) != 0).map(|(_, p)| p).sum();
            stats.draw_calls += (e.shared.0 + own) * views;
        }
    }
    models.draws.clear(); #[cfg(feature = "skinning")] models.skin.clear(); #[cfg(feature = "view-model-models")] vm::count_draws(models, stats);
    models.levels.clear();
    models.stream.frame();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Gltf {
        let path = format!("{}/../../tests/fixtures/models/{name}", env!("CARGO_MANIFEST_DIR"));
        Gltf::from_glb(&std::fs::read(&path).unwrap()).unwrap()
    }

    #[cfg(feature = "skinning")]
    fn skinned(p: &Part) -> bool {
        !p.skin.is_empty()
    }
    #[cfg(not(feature = "skinning"))]
    fn skinned(_: &Part) -> bool {
        false
    }

    const FIXTURES: [&str; 9] = [
        "corpus.src.glb",
        "corpus.q.glb",
        "corpus.qc.glb",
        "corpus.qn.glb",
        "corpus.qnc.glb",
        "kart.low.glb",
        "kart.low.qc.glb",
        "crate.low.glb",
        "barrel.low.glb",
    ];

    /// A position or UV component as the GPU reads it: the vertex format is
    /// `Unorm16`, so the shader sees raw / 65535 (pinned by
    /// `vertex_formats_are_what_the_test_reads`).
    fn unorm16_at(v: &[u8], at: usize) -> f32 {
        f32::from(u16::from_le_bytes([v[at], v[at + 1]])) / 65535.0
    }

    /// The model pipelines' vertex formats are the ones `dequant_matches_the_reader`
    /// dequantises with (positions and UVs `Unorm16`, normals and tangents
    /// `Snorm8`), in both the forward and the shadow pipeline. A model whose
    /// scale assumed raw integers once drew as a speck in the browser while
    /// every native test passed; this ties the two together.
    #[test]
    fn vertex_formats_are_what_the_test_reads() {
        let src = include_str!("gpu/models.rs");
        let compact: String = src.split_whitespace().collect();
        assert!(compact.contains("0=>Unorm16x4,1=>Snorm8x4,2=>Snorm8x4,3=>Unorm16x2]"), "forward vertex formats changed: update unorm16_at and Part's scales");
        assert!(compact.contains("SHADOW_VERTEX_ATTRS:[wgpu::VertexAttribute;1]=wgpu::vertex_attr_array![0=>Unorm16x4]"), "shadow vertex format changed");
        for wgsl in [include_str!("../shaders/model.wgsl"), include_str!("../shaders/model_shadows.wgsl")] {
            assert!(wgsl.contains("in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz"), "the shader's dequantisation changed");
        }
        assert!(include_str!("../shaders/model.wgsl").contains("in.uv_dq.zw + in.uv_dq.xy * in.uv"));
    }

    /// Every drawn primitive's quantised vertices, dequantised the way
    /// model.wgsl does, match the reader's floats within half a
    /// quantisation step (normals and tangents within one 8-bit step), and
    /// the indices and node matrices are the reader's.
    #[test]
    fn dequant_matches_the_reader() {
        for name in FIXTURES {
            let g = fixture(name);
            let p = prepare(&g).unwrap();
            assert!(!p.parts.is_empty(), "{name}");
            let world = g.world_matrices();
            // Skinned parts (feature `skinning`) are in mesh space with an
            // identity node: model/skin.rs tests them.
            for part in p.parts.iter().filter(|x| !skinned(x)) {
                let prim = &g.meshes[part.source[1]].primitives[part.source[2]];
                assert_eq!(g.nodes[part.source[0]].mesh, Some(part.source[1]));
                assert_eq!(part.node, world[part.source[0]], "{name}");
                let pos = g.read_f32(prim.attribute("POSITION").unwrap()).unwrap();
                let n = pos.len() / 3;
                assert_eq!(part.vertices.len(), n * VERTEX_BYTES);
                let idx = match prim.indices {
                    Some(i) => g.read_u32(i).unwrap(),
                    None => (0..n as u32).collect(),
                };
                assert_eq!(part.indices, idx, "{name}");
                let nrm = prim.attribute("NORMAL").map(|a| g.read_f32(a).unwrap());
                let tan = prim.attribute("TANGENT").map(|a| g.read_f32(a).unwrap());
                let uv = prim.attribute("TEXCOORD_0").map(|a| g.read_f32(a).unwrap());
                assert_eq!(part.tangents, tan.is_some());
                for i in 0..n {
                    let v = &part.vertices[i * VERTEX_BYTES..][..VERTEX_BYTES];
                    for k in 0..3 {
                        let got = part.pos_offset[k] + part.pos_scale[k] * unorm16_at(v, k * 2);
                        let tol = part.pos_scale[k] * 0.5 / 65535.0 + pos[i * 3 + k].abs() * 1e-6 + 1e-7;
                        assert!((got - pos[i * 3 + k]).abs() <= tol, "{name} pos {i}.{k}: {got} vs {}", pos[i * 3 + k]);
                    }
                    if let Some(nrm) = &nrm {
                        for k in 0..3 {
                            let got = f32::from(v[8 + k] as i8) / 127.0;
                            assert!((got - nrm[i * 3 + k]).abs() <= 0.5 / 127.0 + 1e-6, "{name} normal");
                        }
                    }
                    if let Some(tan) = &tan {
                        for k in 0..3 {
                            let got = f32::from(v[12 + k] as i8) / 127.0;
                            assert!((got - tan[i * 4 + k]).abs() <= 0.5 / 127.0 + 1e-6, "{name} tangent");
                        }
                        assert_eq!((v[15] as i8) < 0, tan[i * 4 + 3] < 0.0);
                    }
                    if let Some(uv) = &uv {
                        for k in 0..2 {
                            let got = part.uv[2 + k] + part.uv[k] * unorm16_at(v, 16 + k * 2);
                            let tol = part.uv[k] * 0.5 / 65535.0 + uv[i * 2 + k].abs() * 1e-6 + 1e-7;
                            assert!((got - uv[i * 2 + k]).abs() <= tol, "{name} uv");
                        }
                    }
                }
            }
        }
        // The corpus's skinned tube (drawn by `skinning` only) and line
        // strip are not drawn here.
        let p = prepare(&fixture("corpus.src.glb")).unwrap();
        assert_eq!(p.parts.iter().filter(|x| !skinned(x)).count(), 1);
        assert_eq!(p.parts.len(), 1 + usize::from(cfg!(feature = "skinning")));
        // A wrong-shaped attribute is an error, not a panic.
        let g = fixture("corpus.src.glb");
        assert!(read(&g, g.meshes[0].primitives[0].attribute("TEXCOORD_0").unwrap(), 3, usize::MAX).is_err());
    }

    /// Relative texture URIs go under the model's folder; root-relative,
    /// absolute and `data:` ones are fetched as they are.
    #[test]
    fn texture_uris_resolve_against_the_model_folder() {
        let f = "models/marshal/";
        assert_eq!(resolve_uri(f, "tex/base.ktx2"), "models/marshal/tex/base.ktx2");
        assert_eq!(resolve_uri(f, "/assets/a.png"), "/assets/a.png");
        assert_eq!(resolve_uri(f, "https://cdn.example/a.png"), "https://cdn.example/a.png");
        assert_eq!(resolve_uri(f, "data:image/png;base64,AAAA"), "data:image/png;base64,AAAA");
        assert_eq!(resolve_uri(f, "a:b.png"), "models/marshal/a:b.png");
        assert_eq!(resolve_uri("", ""), "");
    }

    /// The corpus material uses every core texture; only base colour,
    /// normal and emissive are listed, and on no device does any fetch
    /// reach the metallic-roughness or occlusion texture.
    #[test]
    fn unsampled_maps_are_never_fetched() {
        let g = fixture("corpus.src.glb");
        let m = &g.materials[0];
        let (mr, occ) = (m.metallic_roughness_texture.clone().unwrap().texture, m.occlusion_texture.clone().unwrap().texture);
        let sampled = [
            m.base_color_texture.clone().unwrap().texture,
            m.normal_texture.clone().unwrap().texture,
            m.emissive_texture.clone().unwrap().texture,
        ];
        assert!(!sampled.contains(&mr) && !sampled.contains(&occ));
        let p = prepare(&g).unwrap();
        let listed: Vec<usize> = p.textures.iter().map(|t| t.texture).collect();
        #[cfg(not(all(feature = "look-surface", not(feature = "relief"))))] assert_eq!(listed, sampled); #[cfg(all(feature = "look-surface", not(feature = "relief")))] assert_eq!(listed, [&sampled[..], &[mr, occ][..]].concat(), "the lit surface pass also lists the metal-rough and occlusion maps");
        assert_eq!(p.textures.iter().map(|t| t.srgb).collect::<Vec<_>>(), if cfg!(all(feature = "look-surface", not(feature = "relief"))) { &[true, false, true, false, false][..] } else { &[true, false, true][..] });
        assert_eq!(p.materials[0].maps, [Some(0), Some(1), Some(2)]);
        // The URIs a loader could fetch, on every device.
        let off_uris: Vec<&str> = g
            .evani
            .as_ref()
            .unwrap()
            .textures
            .iter()
            .filter(|s| s.texture == mr || s.texture == occ)
            .flat_map(|s| s.sets.iter().map(|x| x.1.as_str()))
            .collect();
        let devs = (0..8).map(|b| DeviceTex { astc: b & 1 != 0, bc: b & 2 != 0, etc2: b & 4 != 0 });
        for dev in devs {
            for t in p.textures.iter().take(3) {
                match t.source(dev) {
                    TexSource::Ktx2(u, _) | TexSource::Uri(u) => assert!(!off_uris.contains(&u), "{u}"),
                    TexSource::Embedded(_) | TexSource::None => {}
                }
            }
        }
        // Merged textures (gltfpack): one texture as base and emissive is
        // listed once; as a normal map it is listed again, linear.
        let p = prepare(&fixture("corpus.q.glb")).unwrap();
        assert!(p.textures.iter().all(|t| t.texture == 0));
        assert_eq!(p.textures.iter().map(|t| t.srgb).collect::<Vec<_>>(), [true, false]);
        // Format choice: ASTC set on an ASTC device, the core image
        // otherwise, the KTX2 rgba set only without a core image.
        let mut t = TexNeed { texture: 0, srgb: true, image: Some(vec![1]), uri: None, sets: vec![("astc".into(), "a.ktx2".into()), ("rgba".into(), "r.ktx2".into())] };
        assert_eq!(t.source(DeviceTex { astc: true, bc: true, etc2: false }), TexSource::Ktx2("a.ktx2", TexFormat::Astc4x4));
        assert_eq!(t.source(DeviceTex::default()), TexSource::Embedded(&[1]));
        t.image = None;
        assert_eq!(t.source(DeviceTex::default()), TexSource::Ktx2("r.ktx2", TexFormat::Rgba));
    }

    #[test]
    fn native_draw_counts_follow_parts_and_views() {
        let path = format!("{}/../../tests/fixtures/models/kart.low.glb", env!("CARGO_MANIFEST_DIR"));
        let mut m = Models::default();
        let id = m.add_glb("models/kart.low.glb", &std::fs::read(path).unwrap());
        let bad = m.add_glb("x.glb", b"not a model");
        assert!(matches!(m.state(bad), Some(ModelState::Failed(_))));
        m.draw(ModelInstance::new(id, [0.0; 3]));
        m.draw(ModelInstance::new(id, [1.0, 0.0, 0.0]));
        m.draw(ModelInstance::new(bad, [0.0; 3]));
        let mut s = RenderStats::default();
        let mut rw = RenderWorld::default();
        rw.views = vec![crate::world::View::new(rw.camera, [0.0, 0.0, 1.0, 0.5]); 2];
        count_draws(&mut m, &rw, None, &mut s);
        assert!(m.ready(id));
        let parts = prepare(&fixture("kart.low.glb")).unwrap().parts.len() as u32;
        assert_eq!(s.draw_calls, parts * 2);
        assert!(s.triangles > 0 && s.triangles % 4 == 0);
        assert!(m.draws.is_empty());
        // Instance words: identity instance reproduces the node matrix rows.
        let p = prepare(&fixture("kart.low.glb")).unwrap();
        let mut w = Vec::new();
        instance_words(&ModelInstance::new(id, [0.0; 3]), &p.parts[0], &[], &mut w);
        assert_eq!(w.len(), INSTANCE_WORDS);
        let nm = p.parts[0].node;
        assert!((w[3] - nm[12]).abs() < 1e-6 && (w[0] - nm[0]).abs() < 1e-6);
    }

    /// Each draw gets the level for its projected size: near draws level 0,
    /// far ones the coarse level, and the native counts follow (shared
    /// parts once per view, each used level's parts once per view).
    #[test]
    fn draws_pick_their_level_by_screen_size() {
        let path = format!("{}/../../tests/fixtures/models/kart.low.glb", env!("CARGO_MANIFEST_DIR"));
        let mut m = Models::default();
        let id = m.add_glb("models/kart.glb", &std::fs::read(path).unwrap());
        let mut s = RenderStats::default();
        let rw = RenderWorld::default();
        count_draws(&mut m, &rw, None, &mut s);
        {
            let e = &mut m.entries[0];
            (e.lods, e.lod_parts, e.lod_triangles, e.shared, e.sphere) = (vec![0.2, 0.0], vec![3, 1], vec![900, 90], (2, 10), ([0.0; 3], 1.0));
        }
        let eye = rw.camera.eye;
        let near = ModelInstance::new(id, [eye[0], eye[1], eye[2] - 3.0]);
        let far = ModelInstance::new(id, [eye[0], eye[1], eye[2] - 200.0]);
        m.draw(near);
        m.draw(far);
        m.select_lods(&rw);
        assert_eq!((m.level(0), m.level(1)), (0, 1));
        let mut s = RenderStats::default();
        count_draws(&mut m, &rw, None, &mut s);
        assert_eq!(s.draw_calls, 2 + 3 + 1, "shared parts and both levels' parts");
        assert_eq!(s.triangles, (10 + 900) + (10 + 90));
        // Only far draws: level 0's parts are not drawn.
        m.draw(far);
        let mut s = RenderStats::default();
        count_draws(&mut m, &rw, None, &mut s);
        assert_eq!((s.draw_calls, s.triangles), (2 + 1, 10 + 90));
        // A far draw scaled up 100x is near again (radius follows scale).
        m.draw(far.scale([100.0; 3]));
        m.select_lods(&rw);
        assert_eq!(m.level(0), 0);
    }
}

#[cfg(feature = "skinning")]
pub mod skin;

#[cfg(feature = "view-model-models")]
pub mod vm;
