//! A glTF document as JSON plus one binary buffer: read and write `.glb`,
//! and read a `.gltf` whose buffers are files beside it.

use std::path::Path;

use serde_json::Value;

/// JSON and the single binary buffer (buffer 0).
#[derive(Clone, Debug)]
pub struct Doc {
    pub json: Value,
    pub bin: Vec<u8>,
}

impl Doc {
    pub fn read_glb(bytes: &[u8]) -> Result<Doc, String> {
        let (json, bin) = evani_gltf::split_glb(bytes).map_err(|e| format!("not a GLB file: {}", e.0))?;
        let json: Value = serde_json::from_slice(json).map_err(|e| format!("GLB JSON does not parse: {e}"))?;
        Ok(Doc { json, bin: bin.map(<[u8]>::to_vec).unwrap_or_default() })
    }

    pub fn read_file(path: &Path) -> Result<Doc, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gltf")) {
            return Self::read_gltf(&bytes, path.parent().unwrap_or(Path::new(".")));
        }
        Self::read_glb(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// A `.gltf` with at most one buffer, read from a file beside it.
    pub fn read_gltf(bytes: &[u8], dir: &Path) -> Result<Doc, String> {
        let mut json: Value = serde_json::from_slice(bytes).map_err(|e| format!("glTF JSON does not parse: {e}"))?;
        let buffers = json.get("buffers").and_then(Value::as_array).cloned().unwrap_or_default();
        if buffers.len() > 1 {
            return Err("a .gltf with more than one buffer (pack it with `gltf-transform copy` first)".into());
        }
        let mut bin = Vec::new();
        if let Some(b) = buffers.first() {
            let uri = b.get("uri").and_then(Value::as_str).ok_or("buffer without a uri")?;
            if uri.starts_with("data:") {
                return Err("data: URIs are not read here (pack the file with `gltf-transform copy` first)".into());
            }
            let p = dir.join(uri_path(uri));
            bin = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            if let Some(b0) = json.pointer_mut("/buffers/0").and_then(Value::as_object_mut) {
                b0.remove("uri");
            }
        }
        Ok(Doc { json, bin })
    }

    /// The GLB bytes: JSON chunk padded with spaces, BIN chunk with zeros.
    pub fn to_glb(&self) -> Result<Vec<u8>, String> {
        let mut json = self.json.clone();
        if self.bin.is_empty() {
            if let Some(o) = json.as_object_mut()
                && o.get("buffers").and_then(Value::as_array).is_some_and(|b| b.len() == 1 && b[0].get("uri").is_none())
            {
                o.remove("buffers");
            }
        } else if let Some(b0) = json.pointer_mut("/buffers/0") {
            b0["byteLength"] = Value::from(self.bin.len());
        }
        let mut j = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let mut b = self.bin.clone();
        while !b.len().is_multiple_of(4) {
            b.push(0);
        }
        let total = 12 + 8 + j.len() + if b.is_empty() { 0 } else { 8 + b.len() };
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(total).map_err(|_| "GLB over 4 GB")?.to_le_bytes());
        out.extend_from_slice(&u32::try_from(j.len()).map_err(|_| "JSON over 4 GB")?.to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&j);
        if !b.is_empty() {
            out.extend_from_slice(&u32::try_from(b.len()).map_err(|_| "BIN over 4 GB")?.to_le_bytes());
            out.extend_from_slice(b"BIN\0");
            out.extend_from_slice(&b);
        }
        Ok(out)
    }

    pub fn arr(&self, key: &str) -> &[Value] {
        self.json.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }
}

/// A relative URI as a file path (percent-escapes decoded); one copy, shared
/// with `cargo xtask check`.
pub use evani_guide::modelcheck::uri_path;

/// The generator string (`asset.generator`).
pub fn generator(json: &Value) -> &str {
    json.pointer("/asset/generator").and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn glb_round_trip_pads_chunks() {
        let d = Doc { json: json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":3}]}), bin: vec![1, 2, 3] };
        let g = d.to_glb().unwrap();
        assert_eq!(g.len() % 4, 0);
        let back = Doc::read_glb(&g).unwrap();
        assert_eq!(&back.bin[..3], &[1, 2, 3]);
        assert_eq!(back.json["buffers"][0]["byteLength"], 3);
        assert_eq!(uri_path("a%20b.png"), "a b.png");
    }
}
