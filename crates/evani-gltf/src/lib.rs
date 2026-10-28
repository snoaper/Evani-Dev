//! `evani-gltf`: the engine's own model reader (part of the `models` module,
//! `evani.module.models`). docs/MODEL_FORMAT.md is the contract it reads.
//!
//! - Containers: `.glb` (header, JSON chunk, BIN chunk) and `.gltf` with
//!   external buffers (the caller fetches each URI; buffers tagged as
//!   EXT_meshopt_compression fallbacks are never requested).
//! - Only the fields the engine uses: buffers, buffer views, accessors,
//!   meshes and primitives, nodes, scenes, skins, materials with every core
//!   metallic-roughness field, textures, images and samplers, and the root
//!   `extras.evani`. Parsed through `serde_json::Value`, which every game
//!   already carries (no new dependency).
//! - KHR_mesh_quantization: integer and normalised attributes are read and
//!   dequantised by the spec's rules ([`Gltf::read_f32`]); the node scale and
//!   offset that undo position quantisation are ordinary node transforms
//!   ([`Gltf::world_matrices`]).
//! - EXT_meshopt_compression: compressed buffer views are decoded once at load
//!   ([`meshopt`]); fallback buffers with no data are ignored.
//! - Evani metadata: anchors from empty nodes named `anchor:<name>`, the
//!   `collision` node, and from `extras.evani` the format version, part tree,
//!   LOD thresholds and texture-set names ([`Evani`]).
//! - Feature `anim-source` (native only, the converter's clip baking): glTF
//!   animation channels and samplers. No game turns it on.
//!
//! Bad input is an [`Error`], never a panic: every offset, length and index is
//! checked before use.

#[cfg(all(feature = "anim-source", target_arch = "wasm32"))]
compile_error!("evani-gltf's `anim-source` is native only (the converter); games never turn it on");

#[cfg(target_arch = "wasm32")]
macro_rules! err {
    ($code:literal, $text:literal) => {
        $crate::Error($code)
    };
}
#[cfg(not(target_arch = "wasm32"))]
macro_rules! err {
    ($code:literal, $text:literal) => {
        $crate::Error($text)
    };
}

mod doc;
mod evani;
pub mod meshopt;

#[cfg(feature = "anim-source")]
mod anim;

#[cfg(feature = "anim-source")]
pub use anim::{AnimChannel, AnimSampler, Animation};
pub use doc::*;
pub use evani::{Anchor, Evani, Lod, Part, TextureSet, EVANI_FORMAT};

use serde_json::Value;

/// A reading error. Static text keeps the reader small in a game's download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

/// Extensions this reader understands. A file that requires any other is
/// refused (it could not be drawn correctly).
pub const SUPPORTED_EXTENSIONS: [&str; 3] = ["KHR_mesh_quantization", "EXT_meshopt_compression", "KHR_texture_transform"];

/// The `models` module's marker, read by `cargo xtask` (size report and the
/// build-time on/off check). Every read goes through `Gltf::load`, which keeps
/// it in the download, so a game's wasm carries it exactly when the reader is
/// linked, and only the `models` feature links it.
pub const MODULE_ID: &str = "evani.module.models";

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o.checked_add(4)?)?.try_into().ok()?))
}

/// Split a `.glb` into its JSON chunk and optional BIN chunk.
pub fn split_glb(glb: &[u8]) -> Result<(&[u8], Option<&[u8]>), Error> {
    if glb.get(0..4) != Some(b"glTF") {
        return Err(err!("B1", "glb: bad magic (not a glTF binary)"));
    }
    if u32_at(glb, 4) != Some(2) {
        return Err(err!("B2", "glb: not glTF version 2"));
    }
    let total = u32_at(glb, 8).ok_or(err!("B3", "glb: truncated header"))? as usize;
    if total > glb.len() || total < 20 {
        return Err(err!("B4", "glb: length field does not match the file"));
    }
    let glb = &glb[..total];
    let chunk = |at: usize| -> Result<Option<(u32, &[u8])>, Error> {
        if at >= glb.len() {
            return Ok(None);
        }
        let len = u32_at(glb, at).ok_or(err!("B5", "glb: truncated chunk header"))? as usize;
        let ty = u32_at(glb, at + 4).ok_or(err!("B5", "glb: truncated chunk header"))?;
        let body = glb.get(at + 8..(at + 8).checked_add(len).ok_or(err!("B6", "glb: bad chunk length"))?).ok_or(err!("B7", "glb: chunk runs past the end"))?;
        Ok(Some((ty, body)))
    };
    let (ty, json) = chunk(12)?.ok_or(err!("B8", "glb: no JSON chunk"))?;
    if ty != 0x4E4F_534A {
        return Err(err!("B9", "glb: first chunk is not JSON"));
    }
    let bin = match chunk(20 + json.len())? {
        Some((0x004E_4942, b)) => Some(b),
        Some(_) | None => None,
    };
    Ok((json, bin))
}

impl Gltf {
    /// Read a `.glb`. Buffer 0 without a URI is the BIN chunk; any other
    /// buffer must be a meshopt fallback (or unused), since a `.glb` has no
    /// way to fetch more.
    pub fn from_glb(glb: &[u8]) -> Result<Self, Error> {
        let (json, bin) = split_glb(glb)?;
        Self::load(json, bin, &mut |_| None)
    }

    /// Read a `.gltf`: `fetch` is called once per buffer URI the file needs
    /// (relative to the `.gltf`), never for a meshopt fallback buffer.
    /// `None` from `fetch` is an error only if a view reads that buffer.
    pub fn from_gltf(json: &[u8], fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Self, Error> {
        Self::load(json, None, fetch)
    }

    fn load(json: &[u8], bin: Option<&[u8]>, fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Self, Error> {
        std::hint::black_box(MODULE_ID);
        // From a `str`, not a slice: the engine already parses JSON text into
        // a `Value` (params, curves), so this reuses that parser instead of
        // shipping a second, byte-slice one (about 2 KB gzip).
        let text = core::str::from_utf8(json).map_err(|_| err!("G1", "gltf: the JSON does not parse"))?;
        let mut root: Value = serde_json::from_str(text).map_err(|_| err!("G1", "gltf: the JSON does not parse"))?;
        if !root.is_object() {
            return Err(err!("G2", "gltf: the JSON is not an object"));
        }
        let version = root.get("asset").and_then(|a| a.get("version")).and_then(Value::as_str);
        if !version.is_some_and(|v| v.starts_with("2.")) {
            return Err(err!("G3", "gltf: asset.version is not 2.x"));
        }
        let mut g = doc::parse(&root)?;
        if g.extensions_required.iter().any(|e| !SUPPORTED_EXTENSIONS.contains(&e.as_str())) {
            return Err(err!("G4", "gltf: the file requires an extension this reader does not support"));
        }
        // Buffer data: the BIN chunk, fetched URIs, nothing for fallbacks.
        let mut data = Vec::with_capacity(g.buffers.len());
        for (i, b) in g.buffers.iter().enumerate() {
            let d = match (&b.uri, i, bin) {
                _ if b.fallback => None,
                (None, 0, Some(bin)) => Some(bin.to_vec()),
                (Some(uri), _, _) => fetch(uri),
                _ => None,
            };
            if let Some(d) = &d
                && d.len() < b.byte_length
            {
                return Err(err!("G5", "gltf: buffer data is shorter than its byteLength"));
            }
            data.push(d);
        }
        g.data = data;
        // Decode every meshopt-compressed view once.
        let mut decoded = Vec::with_capacity(g.buffer_views.len());
        for v in &g.buffer_views {
            let Some(m) = &v.meshopt else {
                decoded.push(None);
                continue;
            };
            if v.byte_length != m.count.saturating_mul(m.byte_stride) || v.byte_stride.is_some_and(|s| s != m.byte_stride) {
                return Err(err!("M1", "meshopt: view length or stride does not match the extension"));
            }
            let src = g.slice(m.buffer, m.byte_offset, m.byte_length)?;
            // Refuse before allocating: each mode needs a minimum number of
            // compressed bytes per element, so a huge count with a tiny
            // stream is corrupt.
            let min_src = match m.mode {
                meshopt::Mode::Attributes => (m.count / 64).saturating_mul(m.byte_stride),
                meshopt::Mode::Triangles => m.count / 3,
                meshopt::Mode::Indices => m.count,
            };
            if min_src > src.len() {
                return Err(err!("M2", "meshopt: count too large for the compressed data"));
            }
            let mut out = Vec::new();
            out.try_reserve_exact(v.byte_length).map_err(|_| err!("M3", "meshopt: view too large"))?;
            out.resize(v.byte_length, 0);
            meshopt::decode(&mut out, m.count, m.byte_stride, m.mode, m.filter, src)?;
            decoded.push(Some(out));
        }
        g.decoded = decoded;
        doc::take_extras(&mut g, &mut root);
        g.finish()?;
        Ok(g)
    }

    fn slice(&self, buffer: usize, offset: usize, len: usize) -> Result<&[u8], Error> {
        let d = self.data.get(buffer).ok_or(err!("G6", "gltf: buffer index out of range"))?;
        let d = d.as_deref().ok_or(err!("G7", "gltf: a view reads a buffer with no data (missing file or fallback buffer)"))?;
        d.get(offset..offset.checked_add(len).ok_or(err!("G8", "gltf: bad byte range"))?).ok_or(err!("G9", "gltf: byte range past the end of its buffer"))
    }

    /// The bytes of a buffer view (decoded, if it was meshopt-compressed).
    pub fn view_bytes(&self, view: usize) -> Result<&[u8], Error> {
        if let Some(Some(d)) = self.decoded.get(view) {
            return Ok(d);
        }
        let v = self.buffer_views.get(view).ok_or(err!("G10", "gltf: buffer view index out of range"))?;
        self.slice(v.buffer, v.byte_offset, v.byte_length)
    }

    /// An accessor's raw element bytes: `(bytes from element 0, stride)`.
    /// Element `i` is `bytes[i * stride..][..accessor.element_size()]`. This is
    /// what a GPU upload keeps quantised. `None` bytes: no buffer view (all zeros).
    pub fn accessor_bytes(&self, i: usize) -> Result<(Option<&[u8]>, usize), Error> {
        let a = self.accessors.get(i).ok_or(err!("G11", "gltf: accessor index out of range"))?;
        let size = a.element_size();
        let Some(vi) = a.buffer_view else { return Ok((None, size)) };
        let v = &self.buffer_views[vi];
        let stride = v.byte_stride.unwrap_or(size);
        if stride < size {
            return Err(err!("G12", "gltf: byteStride smaller than the element"));
        }
        let bytes = self.view_bytes(vi)?;
        let need = match a.count {
            0 => 0,
            n => (n - 1).checked_mul(stride).and_then(|x| x.checked_add(size)).ok_or(err!("G13", "gltf: accessor too large"))?,
        };
        let end = a.byte_offset.checked_add(need).ok_or(err!("G13", "gltf: accessor too large"))?;
        if end > bytes.len() {
            return Err(err!("G14", "gltf: accessor runs past the end of its buffer view"));
        }
        Ok((Some(&bytes[a.byte_offset..end]), stride))
    }

    /// An accessor as floats, `count * components` values, dequantised the
    /// glTF way: normalised signed `max(c / MAX, -1)`, normalised unsigned
    /// `c / MAX`, integers as their value (KHR_mesh_quantization's node
    /// transform then scales them).
    pub fn read_f32(&self, i: usize) -> Result<Vec<f32>, Error> {
        Ok(self.read_words(i, true)?.into_iter().map(f32::from_bits).collect())
    }

    /// An integer accessor (indices, joints) as `u32`s. Floats are refused.
    pub fn read_u32(&self, i: usize) -> Result<Vec<u32>, Error> {
        let a = self.accessors.get(i).ok_or(err!("G11", "gltf: accessor index out of range"))?;
        if a.component_type == ComponentType::F32 {
            return Err(err!("G15", "gltf: integer accessor expected"));
        }
        self.read_words(i, false)
    }

    /// Every component of an accessor, as `f32` bits (`float`) or as `u32`.
    /// No buffer view: all zeros (0.0 and 0 have the same bits).
    fn read_words(&self, i: usize, float: bool) -> Result<Vec<u32>, Error> {
        let a = self.accessors.get(i).ok_or(err!("G11", "gltf: accessor index out of range"))?;
        let n = a.count.checked_mul(a.components).ok_or(err!("G13", "gltf: accessor too large"))?;
        let (bytes, stride) = self.accessor_bytes(i)?;
        // An accessor with no buffer view can claim any count: allocate
        // fallibly, so an absurd one is an error, not an aborted game.
        let mut out = Vec::new();
        out.try_reserve_exact(n).map_err(|_| err!("G13", "gltf: accessor too large"))?;
        out.resize(n, 0u32);
        if let Some(bytes) = bytes {
            let (t, k, cs) = (a.component_type, a.components, a.component_type.size());
            for (j, o) in out.iter_mut().enumerate() {
                let b = bytes.get(j / k * stride + j % k * cs..).unwrap_or_default();
                *o = if float { t.to_f32(b, a.normalized).to_bits() } else { t.to_u32(b) };
            }
        }
        Ok(out)
    }

    /// The bytes of an image stored in a buffer view (`None` for an image
    /// referenced by URI: the caller fetches it).
    pub fn image_bytes(&self, image: usize) -> Result<Option<&[u8]>, Error> {
        let im = self.images.get(image).ok_or(err!("G16", "gltf: image index out of range"))?;
        im.buffer_view.map(|v| self.view_bytes(v)).transpose()
    }

    /// Every node's world matrix (column-major 4x4), parents first.
    pub fn world_matrices(&self) -> Vec<[f32; 16]> {
        let mut w = vec![IDENTITY; self.nodes.len()];
        for &n in &self.order {
            let local = self.nodes[n].local_matrix();
            w[n] = match self.parents[n] {
                Some(p) => mul(&w[p], &local),
                None => local,
            };
        }
        w
    }

    /// The node's parent, if any.
    pub fn parent(&self, node: usize) -> Option<usize> {
        self.parents.get(node).copied().flatten()
    }

    /// Find a node by name.
    pub fn node_by_name(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }
}

/// The 4x4 identity, column-major.
pub const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// `a * b` for column-major 4x4 matrices.
pub fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[c * 4 + k]).sum();
        }
    }
    r
}

/// Transform a point by a column-major 4x4 matrix (affine).
pub fn transform_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}
