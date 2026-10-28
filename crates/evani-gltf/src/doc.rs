//! The glTF objects the engine reads, parsed from `serde_json::Value`. Every
//! index is checked against its array here, so later reads cannot go out of
//! range.

use crate::meshopt::{Filter, Mode};
use crate::{Anchor, Error, Evani};
use serde_json::Value;

/// A component type (glTF `componentType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentType {
    I8,
    U8,
    I16,
    U16,
    U32,
    F32,
}

impl ComponentType {
    fn from_gl(v: u64) -> Option<Self> {
        Some(match v {
            5120 => Self::I8,
            5121 => Self::U8,
            5122 => Self::I16,
            5123 => Self::U16,
            5125 => Self::U32,
            5126 => Self::F32,
            _ => return None,
        })
    }

    /// Bytes per component.
    pub fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::U32 | Self::F32 => 4,
        }
    }

    /// One component as a float, dequantised by the glTF rules (KHR_mesh_quantization
    /// and core): normalised signed `max(c / MAX, -1)`, unsigned `c / MAX`.
    /// `b` should hold at least [`ComponentType::size`] bytes (missing bytes read as 0).
    pub fn to_f32(self, b: &[u8], normalized: bool) -> f32 {
        let w = word(b);
        let (v, max) = match self {
            Self::I8 => (w[0] as i8 as f32, 127.0),
            Self::U8 => (w[0] as f32, 255.0),
            Self::I16 => (i16::from_le_bytes([w[0], w[1]]) as f32, 32767.0),
            Self::U16 => (u16::from_le_bytes([w[0], w[1]]) as f32, 65535.0),
            Self::U32 => (u32::from_le_bytes(w) as f32, 4_294_967_295.0),
            Self::F32 => return f32::from_le_bytes(w),
        };
        if normalized { (v / max).max(-1.0) } else { v }
    }

    /// One integer component (floats truncate). `b` as for [`ComponentType::to_f32`].
    pub fn to_u32(self, b: &[u8]) -> u32 {
        let w = word(b);
        match self {
            Self::I8 => w[0] as i8 as u32,
            Self::U8 => w[0] as u32,
            Self::I16 => i16::from_le_bytes([w[0], w[1]]) as u32,
            Self::U16 => u16::from_le_bytes([w[0], w[1]]) as u32,
            Self::U32 => u32::from_le_bytes(w),
            Self::F32 => f32::from_le_bytes(w) as u32,
        }
    }
}

/// The first four bytes of `b` (zeros past its end): reads with no panic path.
fn word(b: &[u8]) -> [u8; 4] {
    let mut w = [0; 4];
    for (o, x) in w.iter_mut().zip(b) {
        *o = *x;
    }
    w
}

#[derive(Clone, Debug, Default)]
pub struct Buffer {
    pub byte_length: usize,
    pub uri: Option<String>,
    /// EXT_meshopt_compression fallback: never fetched, never read.
    pub fallback: bool,
}

/// EXT_meshopt_compression on a buffer view: where the compressed bytes are.
#[derive(Clone, Debug)]
pub struct MeshoptView {
    pub buffer: usize,
    pub byte_offset: usize,
    pub byte_length: usize,
    pub byte_stride: usize,
    pub count: usize,
    pub mode: Mode,
    pub filter: Filter,
}

#[derive(Clone, Debug)]
pub struct BufferView {
    pub buffer: usize,
    pub byte_offset: usize,
    pub byte_length: usize,
    pub byte_stride: Option<usize>,
    pub meshopt: Option<MeshoptView>,
}

#[derive(Clone, Debug)]
pub struct Accessor {
    pub buffer_view: Option<usize>,
    pub byte_offset: usize,
    pub component_type: ComponentType,
    pub normalized: bool,
    pub count: usize,
    /// Components per element: SCALAR 1, VEC2 2, VEC3 3, VEC4 4, MAT4 16.
    pub components: usize,
    pub min: Vec<f32>,
    pub max: Vec<f32>,
}

impl Accessor {
    /// Bytes per element (no matrix column padding: MAT2 and MAT3 of 8- or
    /// 16-bit components are refused at load).
    pub fn element_size(&self) -> usize {
        self.components * self.component_type.size()
    }
}

#[derive(Clone, Debug)]
pub struct Primitive {
    /// `(semantic, accessor)`, e.g. `("POSITION", 0)`.
    pub attributes: Vec<(String, usize)>,
    pub indices: Option<usize>,
    pub material: Option<usize>,
    /// 0 points, 1 lines, 4 triangles (default), ...
    pub mode: u32,
}

impl Primitive {
    /// The accessor of an attribute, by semantic.
    pub fn attribute(&self, semantic: &str) -> Option<usize> {
        self.attributes.iter().find(|a| a.0 == semantic).map(|a| a.1)
    }
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub name: String,
    pub primitives: Vec<Primitive>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub children: Vec<usize>,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
    pub translation: [f32; 3],
    /// Quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Column-major; when present it replaces translation/rotation/scale.
    pub matrix: Option<[f32; 16]>,
    pub extras: Value,
}

impl Node {
    /// The local transform, column-major (`T * R * S`, or `matrix`).
    pub fn local_matrix(&self) -> [f32; 16] {
        if let Some(m) = self.matrix {
            return m;
        }
        let [x, y, z, w] = self.rotation;
        let [sx, sy, sz] = self.scale;
        let [tx, ty, tz] = self.translation;
        [
            (1.0 - 2.0 * (y * y + z * z)) * sx,
            (2.0 * (x * y + z * w)) * sx,
            (2.0 * (x * z - y * w)) * sx,
            0.0,
            (2.0 * (x * y - z * w)) * sy,
            (1.0 - 2.0 * (x * x + z * z)) * sy,
            (2.0 * (y * z + x * w)) * sy,
            0.0,
            (2.0 * (x * z + y * w)) * sz,
            (2.0 * (y * z - x * w)) * sz,
            (1.0 - 2.0 * (x * x + y * y)) * sz,
            0.0,
            tx,
            ty,
            tz,
            1.0,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub name: String,
    pub nodes: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Skin {
    pub name: String,
    pub joints: Vec<usize>,
    pub inverse_bind_matrices: Option<usize>,
    pub skeleton: Option<usize>,
}

/// KHR_texture_transform on a texture reference (gltfpack writes it for
/// quantised texture coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct TexTransform {
    pub offset: [f32; 2],
    pub rotation: f32,
    pub scale: [f32; 2],
    pub tex_coord: Option<usize>,
}

/// A material's texture reference.
#[derive(Clone, Debug, PartialEq)]
pub struct TexRef {
    pub texture: usize,
    pub tex_coord: usize,
    /// `normalTexture.scale` or `occlusionTexture.strength` (1 elsewhere).
    pub amount: f32,
    pub transform: Option<TexTransform>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

/// A core metallic-roughness material, every field kept so a later
/// realistic look needs no re-conversion (the toon look reads only some).
#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub base_color_factor: [f32; 4],
    pub base_color_texture: Option<TexRef>,
    pub metallic_factor: f32,
    pub roughness_factor: f32,
    pub metallic_roughness_texture: Option<TexRef>,
    pub normal_texture: Option<TexRef>,
    pub occlusion_texture: Option<TexRef>,
    pub emissive_texture: Option<TexRef>,
    pub emissive_factor: [f32; 3],
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    pub extras: Value,
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub name: String,
    pub sampler: Option<usize>,
    pub source: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub name: String,
    pub uri: Option<String>,
    pub mime_type: Option<String>,
    pub buffer_view: Option<usize>,
}

/// GL filter and wrap enums as in the file (wrap default 10497 = REPEAT).
#[derive(Clone, Debug)]
pub struct Sampler {
    pub mag_filter: Option<u32>,
    pub min_filter: Option<u32>,
    pub wrap_s: u32,
    pub wrap_t: u32,
}

/// A loaded model. Fields are the file's arrays; the methods read data.
#[derive(Clone, Debug, Default)]
pub struct Gltf {
    pub generator: Option<String>,
    pub extensions_used: Vec<String>,
    pub extensions_required: Vec<String>,
    pub buffers: Vec<Buffer>,
    pub buffer_views: Vec<BufferView>,
    pub accessors: Vec<Accessor>,
    pub meshes: Vec<Mesh>,
    pub nodes: Vec<Node>,
    pub scenes: Vec<Scene>,
    pub scene: Option<usize>,
    pub skins: Vec<Skin>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub images: Vec<Image>,
    pub samplers: Vec<Sampler>,
    #[cfg(feature = "anim-source")]
    pub animations: Vec<crate::Animation>,
    /// The root `extras` as written.
    pub extras: Value,
    /// The root `extras.evani`, when the file has one.
    pub evani: Option<Evani>,
    /// Empty nodes named `anchor:<name>`, in node order.
    pub anchors: Vec<Anchor>,
    /// The node named `collision` (collision proxies are its children).
    pub collision: Option<usize>,
    pub(crate) data: Vec<Option<Vec<u8>>>,
    pub(crate) decoded: Vec<Option<Vec<u8>>>,
    pub(crate) parents: Vec<Option<usize>>,
    /// Nodes ordered parents first.
    pub(crate) order: Vec<usize>,
}

pub(crate) fn idx(v: &Value, k: &str) -> Option<usize> {
    v.get(k)?.as_u64().and_then(|x| usize::try_from(x).ok())
}

pub(crate) fn num(v: &Value, k: &str, d: f32) -> f32 {
    v.get(k).and_then(Value::as_f64).map_or(d, |x| x as f32)
}

pub(crate) fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k)?.as_str().map(str::to_owned)
}

pub(crate) fn floats<const N: usize>(v: &Value, k: &str, d: [f32; N]) -> Result<[f32; N], Error> {
    let mut r = d;
    floats_into(v, k, &mut r)?;
    Ok(r)
}

/// Fill `out` from the number array `k` (missing: `out` unchanged).
fn floats_into(v: &Value, k: &str, out: &mut [f32]) -> Result<(), Error> {
    let Some(a) = v.get(k) else { return Ok(()) };
    let a = a.as_array().filter(|a| a.len() == out.len()).ok_or(err!("G17", "gltf: a number array has the wrong length"))?;
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64().ok_or(err!("G18", "gltf: a number array holds a non-number"))? as f32;
    }
    Ok(())
}

/// An array property as a slice (missing = empty; not an array = error).
pub(crate) fn list<'a>(v: &'a Value, k: &str) -> Result<&'a [Value], Error> {
    match v.get(k) {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(err!("G19", "gltf: a property that must be an array is not")),
    }
}

/// An optional whole number (offset, stride): missing is `None`, present
/// but negative, fractional or not a number is an error, never a silent 0.
fn whole(v: &Value, k: &str) -> Result<Option<usize>, Error> {
    match v.get(k) {
        None => Ok(None),
        Some(x) => x.as_u64().and_then(|x| usize::try_from(x).ok()).map(Some).ok_or(err!("G20", "gltf: an offset or stride is not a whole number")),
    }
}

/// A required index below `len`.
fn need(v: &Value, k: &str, len: usize) -> Result<usize, Error> {
    idx(v, k).filter(|&i| i < len).ok_or(err!("G21", "gltf: missing or out-of-range index"))
}

/// An optional index; present but out of range is an error.
fn opt(v: &Value, k: &str, len: usize) -> Result<Option<usize>, Error> {
    match v.get(k) {
        None => Ok(None),
        Some(_) => need(v, k, len).map(Some),
    }
}

fn indices(v: &Value, k: &str, len: usize) -> Result<Vec<usize>, Error> {
    list(v, k)?.iter().map(|x| x.as_u64().and_then(|i| usize::try_from(i).ok()).filter(|&i| i < len).ok_or(err!("G22", "gltf: index list entry out of range"))).collect()
}

fn ext<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v.get("extensions")?.get(name)
}

fn strings(v: &Value, k: &str) -> Result<Vec<String>, Error> {
    Ok(list(v, k)?.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
}

fn tex_ref(m: &Value, k: &str, amount_key: &str, textures: usize) -> Result<Option<TexRef>, Error> {
    let Some(t) = m.get(k) else { return Ok(None) };
    let transform = match ext(t, "KHR_texture_transform") {
        Some(x) => Some(TexTransform { offset: floats(x, "offset", [0.0; 2])?, rotation: num(x, "rotation", 0.0), scale: floats(x, "scale", [1.0; 2])?, tex_coord: idx(x, "texCoord") }),
        None => None,
    };
    Ok(Some(TexRef { texture: need(t, "index", textures)?, tex_coord: idx(t, "texCoord").unwrap_or(0), amount: num(t, amount_key, 1.0), transform }))
}

/// Parse the arrays (no buffer data yet).
pub(crate) fn parse(root: &Value) -> Result<Gltf, Error> {
    let l_buffers = list(root, "buffers")?;
    let l_views = list(root, "bufferViews")?;
    let l_accessors = list(root, "accessors")?;
    let l_meshes = list(root, "meshes")?;
    let l_nodes = list(root, "nodes")?;
    let l_skins = list(root, "skins")?;
    let l_materials = list(root, "materials")?;
    let l_textures = list(root, "textures")?;
    let l_images = list(root, "images")?;
    let l_samplers = list(root, "samplers")?;
    let l_scenes = list(root, "scenes")?;
    let (n_buffers, n_views, n_accessors, n_meshes, n_nodes, n_skins, n_materials, n_textures, n_images, n_samplers) = (l_buffers.len(), l_views.len(), l_accessors.len(), l_meshes.len(), l_nodes.len(), l_skins.len(), l_materials.len(), l_textures.len(), l_images.len(), l_samplers.len());

    let mut g = Gltf::default();
    for b in l_buffers {
        let fallback = ext(b, "EXT_meshopt_compression").and_then(|e| e.get("fallback")).and_then(Value::as_bool).unwrap_or(false);
        g.buffers.push(Buffer { byte_length: idx(b, "byteLength").ok_or(err!("G23", "gltf: buffer without byteLength"))?, uri: text(b, "uri"), fallback });
    }

    for v in l_views {
        let meshopt = match ext(v, "EXT_meshopt_compression") {
            None => None,
            Some(m) => {
                let mode = match m.get("mode").and_then(Value::as_str) {
                    Some("ATTRIBUTES") => Mode::Attributes,
                    Some("TRIANGLES") => Mode::Triangles,
                    Some("INDICES") => Mode::Indices,
                    _ => return Err(err!("M4", "meshopt: unknown mode")),
                };
                let filter = match m.get("filter").and_then(Value::as_str) {
                    None | Some("NONE") => Filter::None,
                    Some("OCTAHEDRAL") => Filter::Octahedral,
                    Some("QUATERNION") => Filter::Quaternion,
                    Some("EXPONENTIAL") => Filter::Exponential,
                    _ => return Err(err!("M5", "meshopt: unknown filter")),
                };
                let buffer = need(m, "buffer", n_buffers)?;
                if g.buffers[buffer].fallback {
                    return Err(err!("M6", "meshopt: compressed data in a fallback buffer"));
                }
                Some(MeshoptView {
                    buffer,
                    byte_offset: whole(m, "byteOffset")?.unwrap_or(0),
                    byte_length: idx(m, "byteLength").ok_or(err!("M7", "meshopt: no byteLength"))?,
                    byte_stride: idx(m, "byteStride").ok_or(err!("M8", "meshopt: no byteStride"))?,
                    count: idx(m, "count").ok_or(err!("M9", "meshopt: no count"))?,
                    mode,
                    filter,
                })
            }
        };
        let buffer = need(v, "buffer", n_buffers)?;
        if g.buffers[buffer].fallback && meshopt.is_none() {
            return Err(err!("M10", "meshopt: a plain view reads a fallback buffer"));
        }
        let view = BufferView {
            buffer,
            byte_offset: whole(v, "byteOffset")?.unwrap_or(0),
            byte_length: idx(v, "byteLength").ok_or(err!("G24", "gltf: buffer view without byteLength"))?,
            byte_stride: whole(v, "byteStride")?,
            meshopt,
        };
        // Both ranges must fit their buffer's declared length, even the
        // fallback range of a compressed view that is never read.
        let fits = |b: usize, o: usize, l: usize| o.checked_add(l).is_some_and(|e| e <= g.buffers[b].byte_length);
        if !fits(view.buffer, view.byte_offset, view.byte_length) || view.meshopt.as_ref().is_some_and(|m| !fits(m.buffer, m.byte_offset, m.byte_length)) {
            return Err(err!("G25", "gltf: a buffer view runs past its buffer's byteLength"));
        }
        g.buffer_views.push(view);
    }

    for a in l_accessors {
        if a.get("sparse").is_some() {
            return Err(err!("G26", "gltf: sparse accessors are not supported"));
        }
        let component_type = a.get("componentType").and_then(Value::as_u64).and_then(ComponentType::from_gl).ok_or(err!("G27", "gltf: bad componentType"))?;
        let components = match a.get("type").and_then(Value::as_str) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT4") => 16,
            Some("MAT2" | "MAT3") if component_type.size() == 4 => {
                if a["type"] == "MAT2" {
                    4
                } else {
                    9
                }
            }
            _ => return Err(err!("G28", "gltf: unsupported accessor type")),
        };
        let bound = |k: &str| -> Vec<f32> { list(a, k).unwrap_or(&[]).iter().filter_map(Value::as_f64).map(|x| x as f32).collect() };
        g.accessors.push(Accessor {
            buffer_view: opt(a, "bufferView", n_views)?,
            byte_offset: whole(a, "byteOffset")?.unwrap_or(0),
            component_type,
            normalized: a.get("normalized").and_then(Value::as_bool).unwrap_or(false),
            count: idx(a, "count").ok_or(err!("G29", "gltf: accessor without count"))?,
            components,
            min: bound("min"),
            max: bound("max"),
        });
    }

    for m in l_meshes {
        let mut primitives = Vec::new();
        for p in list(m, "primitives")? {
            let mut attributes = Vec::new();
            if let Some(Value::Object(at)) = p.get("attributes") {
                for (k, v) in at {
                    let i = v.as_u64().and_then(|i| usize::try_from(i).ok()).filter(|&i| i < n_accessors).ok_or(err!("G30", "gltf: attribute accessor out of range"))?;
                    attributes.push((k.clone(), i));
                }
            }
            primitives.push(Primitive {
                attributes,
                indices: opt(p, "indices", n_accessors)?,
                material: opt(p, "material", n_materials)?,
                mode: idx(p, "mode").unwrap_or(4) as u32,
            });
        }
        g.meshes.push(Mesh { name: text(m, "name").unwrap_or_default(), primitives });
    }

    for n in l_nodes {
        let matrix = match n.get("matrix") {
            Some(_) => Some(floats(n, "matrix", [0.0; 16])?),
            None => None,
        };
        g.nodes.push(Node {
            name: text(n, "name").unwrap_or_default(),
            children: indices(n, "children", n_nodes)?,
            mesh: opt(n, "mesh", n_meshes)?,
            skin: opt(n, "skin", n_skins)?,
            translation: floats(n, "translation", [0.0; 3])?,
            rotation: floats(n, "rotation", [0.0, 0.0, 0.0, 1.0])?,
            scale: floats(n, "scale", [1.0; 3])?,
            matrix,
            extras: Value::Null, // moved in by `take_extras`
        });
    }

    for s in l_scenes {
        g.scenes.push(Scene { name: text(s, "name").unwrap_or_default(), nodes: indices(s, "nodes", n_nodes)? });
    }

    for s in l_skins {
        g.skins.push(Skin {
            name: text(s, "name").unwrap_or_default(),
            joints: indices(s, "joints", n_nodes)?,
            inverse_bind_matrices: opt(s, "inverseBindMatrices", n_accessors)?,
            skeleton: opt(s, "skeleton", n_nodes)?,
        });
    }

    for m in l_materials {
        let null = Value::Null;
        let pbr = m.get("pbrMetallicRoughness").unwrap_or(&null);
        g.materials.push(Material {
            name: text(m, "name").unwrap_or_default(),
            base_color_factor: floats(pbr, "baseColorFactor", [1.0; 4])?,
            base_color_texture: tex_ref(pbr, "baseColorTexture", "", n_textures)?,
            metallic_factor: num(pbr, "metallicFactor", 1.0),
            roughness_factor: num(pbr, "roughnessFactor", 1.0),
            metallic_roughness_texture: tex_ref(pbr, "metallicRoughnessTexture", "", n_textures)?,
            normal_texture: tex_ref(m, "normalTexture", "scale", n_textures)?,
            occlusion_texture: tex_ref(m, "occlusionTexture", "strength", n_textures)?,
            emissive_texture: tex_ref(m, "emissiveTexture", "", n_textures)?,
            emissive_factor: floats(m, "emissiveFactor", [0.0; 3])?,
            alpha_mode: match m.get("alphaMode").and_then(Value::as_str) {
                Some("MASK") => AlphaMode::Mask,
                Some("BLEND") => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            },
            alpha_cutoff: num(m, "alphaCutoff", 0.5),
            double_sided: m.get("doubleSided").and_then(Value::as_bool).unwrap_or(false),
            extras: Value::Null, // moved in by `take_extras`
        });
    }

    for t in l_textures {
        g.textures.push(Texture { name: text(t, "name").unwrap_or_default(), sampler: opt(t, "sampler", n_samplers)?, source: opt(t, "source", n_images)? });
    }

    for i in l_images {
        g.images.push(Image { name: text(i, "name").unwrap_or_default(), uri: text(i, "uri"), mime_type: text(i, "mimeType"), buffer_view: opt(i, "bufferView", n_views)? });
    }

    for s in l_samplers {
        g.samplers.push(Sampler {
            mag_filter: idx(s, "magFilter").map(|x| x as u32),
            min_filter: idx(s, "minFilter").map(|x| x as u32),
            wrap_s: idx(s, "wrapS").unwrap_or(10497) as u32,
            wrap_t: idx(s, "wrapT").unwrap_or(10497) as u32,
        });
    }

    g.generator = root.get("asset").and_then(|a| text(a, "generator"));
    g.extensions_used = strings(root, "extensionsUsed")?;
    g.extensions_required = strings(root, "extensionsRequired")?;
    g.scene = opt(root, "scene", g.scenes.len())?;
    #[cfg(feature = "anim-source")]
    {
        g.animations = crate::anim::parse(root, n_nodes, n_accessors)?;
    }
    Ok(g)
}

/// Move the node, material and root `extras` out of the parsed JSON into
/// `g` (which [`parse`] left `Null`). Moved, not cloned: a deep clone of a
/// `Value` links its whole clone code into the download (size trim).
pub(crate) fn take_extras(g: &mut Gltf, root: &mut Value) {
    fn take(v: &mut Value) -> Value {
        v.get_mut("extras").map(Value::take).unwrap_or(Value::Null)
    }
    if let Some(Value::Array(a)) = root.get_mut("nodes") {
        for (n, v) in g.nodes.iter_mut().zip(a) {
            n.extras = take(v);
        }
    }
    if let Some(Value::Array(a)) = root.get_mut("materials") {
        for (m, v) in g.materials.iter_mut().zip(a) {
            m.extras = take(v);
        }
    }
    g.extras = take(root);
}

impl Gltf {
    /// After the data is in: node hierarchy (one parent each, no cycles),
    /// accessor ranges, anchors, the collision node and `extras.evani`.
    pub(crate) fn finish(&mut self) -> Result<(), Error> {
        let n = self.nodes.len();
        let mut parents = vec![None; n];
        for (p, node) in self.nodes.iter().enumerate() {
            for &c in &node.children {
                if parents[c].is_some() || c == p {
                    return Err(err!("G31", "gltf: a node has two parents or is its own child"));
                }
                parents[c] = Some(p);
            }
        }
        // Parents-first order; a cycle leaves nodes unreached.
        let mut order: Vec<usize> = (0..n).filter(|&i| parents[i].is_none()).collect();
        let mut at = 0;
        while at < order.len() {
            order.extend(self.nodes[order[at]].children.iter().copied());
            at += 1;
        }
        if order.len() != n {
            return Err(err!("G32", "gltf: the node hierarchy has a cycle"));
        }
        self.parents = parents;
        self.order = order;
        // Every accessor must fit its view (so reads cannot fail later).
        for i in 0..self.accessors.len() {
            self.accessor_bytes(i)?;
        }
        let world = self.world_matrices();
        self.anchors = crate::evani::anchors(&self.nodes, &world);
        self.collision = self.node_by_name("collision");
        self.evani = match self.extras.get("evani") {
            Some(e) => Some(crate::evani::parse(e, self)?),
            None => None,
        };
        Ok(())
    }
}
