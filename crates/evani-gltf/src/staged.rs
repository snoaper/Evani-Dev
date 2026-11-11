//! A `.glb` read in steps (model-read-spread, 2026-11-02): the browser
//! decodes a big streamed model over several frames instead of in the one
//! it arrives, so a first visit to a tier never stops the frames (the
//! material viewer's 850 KB bust held one frame for 300 ms). The result is
//! the same file [`Gltf::from_glb`] reads in one go; that one runs these
//! steps back to back.

use serde_json::Value;

use crate::{Error, Gltf, doc, meshopt};

/// A file being read: the JSON is parsed and the buffers are checked; each
/// [`Reading::step`] decodes one EXT_meshopt_compression view, and
/// [`Reading::finish`] checks the whole file.
pub struct Reading {
    g: Gltf,
    root: Value,
    /// The next buffer view to look at.
    next: usize,
}

impl Gltf {
    /// Start reading a `.glb` (see [`Reading`]).
    pub fn begin_glb(glb: &[u8]) -> Result<Reading, Error> {
        let (json, bin) = crate::split_glb(glb)?;
        Self::begin(json, bin, &mut |_| None)
    }

    /// Parse the JSON and attach the buffer data: what every read does
    /// before the meshopt views are decoded.
    pub(crate) fn begin(json: &[u8], bin: Option<&[u8]>, fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Reading, Error> {
        std::hint::black_box(crate::MODULE_ID);
        // From a `str`, not a slice: the engine already parses JSON text into
        // a `Value` (params, curves), so this reuses that parser instead of
        // shipping a second, byte-slice one (about 2 KB gzip).
        let text = core::str::from_utf8(json).map_err(|_| err!("G1", "gltf: the JSON does not parse"))?;
        let root: Value = serde_json::from_str(text).map_err(|_| err!("G1", "gltf: the JSON does not parse"))?;
        if !root.is_object() {
            return Err(err!("G2", "gltf: the JSON is not an object"));
        }
        let version = root.get("asset").and_then(|a| a.get("version")).and_then(Value::as_str);
        if !version.is_some_and(|v| v.starts_with("2.")) {
            return Err(err!("G3", "gltf: asset.version is not 2.x"));
        }
        let mut g = doc::parse(&root)?;
        if g.extensions_required.iter().any(|e| !crate::SUPPORTED_EXTENSIONS.contains(&e.as_str())) {
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
        g.decoded = Vec::with_capacity(g.buffer_views.len());
        Ok(Reading { g, root, next: 0 })
    }
}

impl Reading {
    /// Decode the next meshopt-compressed view (views without compression
    /// are passed over in the same step). True once every view is done.
    pub fn step(&mut self) -> Result<bool, Error> {
        let g = &mut self.g;
        while self.next < g.buffer_views.len() {
            let v = &g.buffer_views[self.next];
            self.next += 1;
            let Some(m) = &v.meshopt else {
                g.decoded.push(None);
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
            g.decoded.push(Some(out));
            return Ok(self.next >= g.buffer_views.len());
        }
        Ok(true)
    }

    /// Every view decoded (stepping on first if not): check the file and
    /// give it.
    pub fn finish(mut self) -> Result<Gltf, Error> {
        while !self.step()? {}
        doc::take_extras(&mut self.g, &mut self.root);
        self.g.finish()?;
        Ok(self.g)
    }
}
