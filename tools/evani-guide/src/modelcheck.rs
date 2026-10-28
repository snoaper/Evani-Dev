//! Model-file checks: what the engine's model reader and its KTX2 reader
//! would refuse or draw wrong, found before a game ships the file.
//!
//! One source for two callers: `cargo xtask check` runs [`file_problems`]
//! on every model a game (or the shared `assets/models/`) holds, and
//! evani-convert runs [`problems`] on every tier file it writes. Each rule is
//! a mistake that has happened once (Chris's rule: an error becomes a tool):
//! - Normals that are not unit length. gltfpack `-vn 12` stores normals as
//!   int16 at 12-bit range, so they decode to length 1/16 and light dark;
//!   the Khronos validator can't see inside EXT_meshopt_compression, so only
//!   reading the decoded file finds it.
//! - A LOD level whose node isn't in the file.
//! - A referenced image, texture set or normal map that isn't beside the file.
//! - A KTX2 texture set the engine's reader (evani-render, S3-3) refuses:
//!   supercompressed, not plain 2D, not ASTC 4x4 or BC7, short levels.
//! - A skin GPU skinning (S3-9) refuses: over 256 joints, or a skinned mesh
//!   without joints and weights. Over the soft cap of 64 joints is a
//!   warning ([`skin_warnings`]), not an error.
//! - A surface-relief recipe (`extras.evani.relief`) the engine would turn
//!   off without a word, or relief on a primitive without tangents.
//!
//! A native dev tool: nothing here reaches a game's download.

use std::path::Path;

use evani_gltf::Gltf;
use serde_json::Value;

/// How far a decoded normal's length may be from 1. Quantised normals
/// (8-bit: about 1/127 off at worst) pass; a wrong bit count fails.
pub const NORMAL_TOLERANCE: f32 = 0.05;

/// The most findings of one kind listed per file (the rest are counted).
const MAX_LINES: usize = 5;

/// The KTX2 `vkFormat` field, or `None` when the bytes are not KTX2.
pub fn ktx2_vk_format(bytes: &[u8]) -> Option<u32> {
    if bytes.get(0..12) != Some(&[0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return None;
    }
    bytes.get(12..16).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The vkFormats a texture-set key may hold (`astc`: ASTC 4x4 UNORM / SRGB,
/// `bc7`: BC7 UNORM / SRGB). Empty for keys whose file isn't a KTX2 the
/// engine reads (`rgba`, `etc2`): those are only checked to exist.
pub fn set_formats(set: &str) -> &'static [u32] {
    match set {
        "astc" => &[157, 158],
        "bc7" => &[145, 146],
        _ => &[],
    }
}

/// The rules the engine's KTX2 reader (evani-render, S3-3) applies: no
/// supercompression, a plain 2D texture, a block format the engine uploads
/// (ASTC 4x4 or BC7, 16 bytes per 4 x 4 block), every level inside the file
/// at the length its size needs.
pub fn ktx2_check(b: &[u8]) -> Result<(), String> {
    let vk = ktx2_vk_format(b).ok_or("not a KTX2 file")?;
    let u = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or("truncated header");
    let q = |o: usize| b.get(o..o + 8).map(|x| u64::from_le_bytes([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]])).ok_or("truncated level index");
    let (w, h, depth, layers, faces) = (u(20)?, u(24)?, u(28)?, u(32)?, u(36)?);
    let levels = u(40)?.max(1);
    if u(44)? != 0 {
        return Err("supercompressed (the engine reads plain levels)".into());
    }
    if w == 0 || h == 0 || depth > 1 || layers > 1 || faces != 1 {
        return Err("not a plain 2D texture".into());
    }
    if !matches!(vk, 145 | 146 | 157 | 158) {
        return Err(format!("vkFormat {vk} is not ASTC 4x4 or BC7"));
    }
    if levels > 16 || (w >> (levels - 1)) == 0 && (h >> (levels - 1)) == 0 {
        return Err(format!("{levels} levels for {w}x{h}"));
    }
    for i in 0..levels {
        let at = 80 + 24 * i as usize;
        let (off, len) = (q(at)?, q(at + 8)?);
        if off.checked_add(len).is_none_or(|end| end > b.len() as u64) {
            return Err(format!("level {i} runs past the end of the file"));
        }
        let (lw, lh) = (u64::from((w >> i).max(1)), u64::from((h >> i).max(1)));
        let want = lw.div_ceil(4) * lh.div_ceil(4) * 16;
        if len != want {
            return Err(format!("level {i} is {len} bytes, {lw}x{lh} needs {want}"));
        }
    }
    Ok(())
}

/// A relative URI as a file path (`%20` and friends decoded).
pub fn uri_path(uri: &str) -> String {
    let b = uri.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&uri[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The bytes of a `data:...;base64,` URI.
fn data_uri(uri: &str) -> Option<Vec<u8>> {
    let (_, b64) = uri.strip_prefix("data:")?.split_once(";base64,")?;
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::with_capacity(b64.len() / 4 * 3), 0u32, 0u32);
    for c in b64.bytes().filter(|c| *c != b'=' && !c.is_ascii_whitespace()) {
        acc = (acc << 6) | u32::from(val(c)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Open a `.glb` or `.gltf` with the engine's reader (a `.gltf`'s buffers
/// are read from beside it).
pub fn read(path: &Path) -> Result<Gltf, String> {
    let b = std::fs::read(path).map_err(|e| format!("can't read it: {e}"))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let g = if b.starts_with(b"glTF") {
        Gltf::from_glb(&b)
    } else {
        Gltf::from_gltf(&b, &mut |uri| if uri.starts_with("data:") { data_uri(uri) } else { std::fs::read(dir.join(uri_path(uri))).ok() })
    };
    g.map_err(|e| format!("the engine's model reader refuses it: {}", e.0))
}

/// Every normal attribute whose decoded vectors are not unit length, as one
/// line per primitive.
pub fn bad_normals(g: &Gltf) -> Vec<String> {
    let mut out = Vec::new();
    for (mi, m) in g.meshes.iter().enumerate() {
        for (pi, p) in m.primitives.iter().enumerate() {
            let Some(a) = p.attribute("NORMAL") else { continue };
            let Ok(v) = g.read_f32(a) else {
                out.push(format!("mesh {mi} primitive {pi}: its normals can't be read"));
                continue;
            };
            let (mut bad, mut lo, mut hi) = (0usize, f32::MAX, 0f32);
            let n = v.len() / 3;
            for c in v.as_chunks::<3>().0 {
                let len = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
                if (len - 1.0).abs() > NORMAL_TOLERANCE {
                    bad += 1;
                    lo = lo.min(len);
                    hi = hi.max(len);
                }
            }
            if bad > 0 {
                let name = if m.name.is_empty() { String::new() } else { format!(" `{}`", m.name) };
                out.push(format!("mesh {mi}{name} primitive {pi}: {bad} of {n} normals are not unit length (lengths {lo:.4} to {hi:.4}); it lights wrong"));
            }
        }
    }
    out
}

/// Every external file a model points at, as `(kind, uri)`: core images,
/// `extras.evani.textures` sets and `extras.evani.normal_maps` files and sets.
/// Embedded (`data:`) images are left out.
pub fn referenced_files(g: &Gltf) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for im in &g.images {
        if let Some(u) = im.uri.as_ref().filter(|u| !u.starts_with("data:")) {
            out.push(("image".to_owned(), u.clone()));
        }
    }
    if let Some(e) = &g.evani {
        for t in &e.textures {
            for (f, u) in &t.sets {
                out.push((f.clone(), u.clone()));
            }
        }
        for m in e.raw.pointer("/normal_maps/maps").and_then(Value::as_array).into_iter().flatten() {
            if let Some(f) = m.get("file").and_then(Value::as_str) {
                out.push(("normal map".into(), f.to_owned()));
            }
            for (f, u) in m.get("sets").and_then(Value::as_object).into_iter().flatten() {
                if let Some(u) = u.as_str() {
                    out.push((f.clone(), u.to_owned()));
                }
            }
        }
    }
    out
}

/// What is wrong with an opened model whose files live in `dir`: one line
/// per finding, without the file's name.
pub fn problems(g: &Gltf, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let normals = bad_normals(g);
    let extra = normals.len().saturating_sub(MAX_LINES);
    out.extend(normals.into_iter().take(MAX_LINES));
    if extra > 0 {
        out.push(format!("{extra} more primitives with normals that are not unit length"));
    }
    if let Some(e) = &g.evani {
        for l in e.lods.iter().filter(|l| l.node_index.is_none()) {
            out.push(format!("LOD level node `{}` is not in the file", l.node));
        }
    }
    if let Some(c) = g.extras.pointer("/evani/clips") {
        out.extend(clip_problems(g, c));
    }
    out.extend(skin_errors(g));
    out.extend(relief_problems(g));
    let mut seen = std::collections::BTreeSet::new();
    for (kind, uri) in referenced_files(g) {
        if !seen.insert((kind.clone(), uri.clone())) {
            continue;
        }
        let Ok(b) = std::fs::read(dir.join(uri_path(&uri))) else {
            out.push(format!("{kind} file `{uri}` is missing"));
            continue;
        };
        let want = set_formats(&kind);
        if want.is_empty() {
            continue;
        }
        match ktx2_vk_format(&b) {
            Some(f) if want.contains(&f) => {
                if let Err(e) = ktx2_check(&b) {
                    out.push(format!("`{uri}`: the engine's KTX2 reader would refuse it: {e}"));
                }
            }
            f => out.push(format!("`{uri}` is listed as `{kind}` but is not a plain {kind} KTX2 (vkFormat {f:?})")),
        }
    }
    out
}

/// `extras.evani.clips` (S3-7): the block's buffer view is in the file, the
/// block parses ([`crate::clipblock`]), and its clips match the list.
pub fn clip_problems(g: &Gltf, c: &Value) -> Vec<String> {
    use crate::clipblock;
    let fmt = c.get("format").and_then(Value::as_u64);
    if fmt != Some(clipblock::FORMAT) {
        return vec![format!("extras.evani.clips format {fmt:?}: this engine reads format {}", clipblock::FORMAT)];
    }
    if c.get("rate").and_then(Value::as_u64) != Some(u64::from(clipblock::RATE)) {
        return vec![format!("extras.evani.clips rate is not {}", clipblock::RATE)];
    }
    let Some(view) = c.get("view").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).filter(|&v| v < g.buffer_views.len()) else {
        return vec!["extras.evani.clips names no buffer view in the file".into()];
    };
    let bytes = match g.view_bytes(view) {
        Ok(b) => b,
        Err(e) => return vec![format!("extras.evani.clips buffer view {view} cannot be read: {}", e.0)],
    };
    let clips = match clipblock::parse(bytes) {
        Ok(c) => c,
        Err(e) => return vec![format!("the clip block (buffer view {view}) does not parse: {e}")],
    };
    let list = c.get("list").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.len() != clips.len() {
        return vec![format!("extras.evani.clips lists {} clips, the block holds {}", list.len(), clips.len())];
    }
    let mut out = Vec::new();
    for (l, k) in list.iter().zip(&clips) {
        let name = l.get("name").and_then(Value::as_str).unwrap_or("");
        if l.get("frames").and_then(Value::as_u64) != Some(u64::from(k.frames)) {
            out.push(format!("clip `{name}`: listed frames {:?}, the block says {}", l.get("frames"), k.frames));
        }
        if let Some(t) = k.tracks.iter().find(|t| usize::from(t.node) >= g.nodes.len()) {
            out.push(format!("clip `{name}`: a track targets node {}, the file has {} nodes", t.node, g.nodes.len()));
        }
    }
    out
}

/// The joints a rig may have before [`skin_warnings`] warns: GPU skinning's
/// soft cap (STAGE3_PLAN S3-9; evani-render's `model::skin::SOFT_JOINTS`).
/// More still draw, at more matrices per character per frame.
pub const SOFT_JOINTS: usize = 64;
/// The joints a skin can have at all (u8 joint indices; the engine refuses
/// more, `model::skin::MAX_JOINTS`).
pub const MAX_JOINTS: usize = 256;

/// Skins over the soft cap ([`SOFT_JOINTS`]): one warning line each (not an
/// error, the model draws). A skin over [`MAX_JOINTS`] is in [`skin_errors`].
pub fn skin_warnings(g: &Gltf) -> Vec<String> {
    g.skins
        .iter()
        .enumerate()
        .filter(|(_, s)| s.joints.len() > SOFT_JOINTS && s.joints.len() <= MAX_JOINTS)
        .map(|(i, s)| format!("skin {i} has {} joints, over the soft cap of {SOFT_JOINTS} for GPU skinning: every drawn character uploads one matrix per joint each frame", s.joints.len()))
        .collect()
}

/// Skins the engine refuses: over [`MAX_JOINTS`] joints, or a skinned mesh
/// without JOINTS_0 and WEIGHTS_0.
pub fn skin_errors(g: &Gltf) -> Vec<String> {
    let mut out: Vec<String> = g.skins.iter().enumerate().filter(|(_, s)| s.joints.len() > MAX_JOINTS).map(|(i, s)| format!("skin {i} has {} joints; the engine reads at most {MAX_JOINTS} (u8 joint indices)", s.joints.len())).collect();
    for n in g.nodes.iter().filter(|n| n.skin.is_some()) {
        let Some(m) = n.mesh.and_then(|m| g.meshes.get(m)) else { continue };
        if m.primitives.iter().any(|p| p.attribute("JOINTS_0").is_none() || p.attribute("WEIGHTS_0").is_none()) {
            out.push(format!("skinned mesh `{}` has a primitive without JOINTS_0 and WEIGHTS_0", m.name));
        }
    }
    out
}

/// The micro patterns evani-render's `relief` reads (`model::relief::PATTERNS`,
/// held equal by a test here).
pub const RELIEF_PATTERNS: [&str; 4] = ["knurl", "ribs", "dots", "grain"];
/// The deepest height map evani-render accepts (`model::relief::MAX_DEPTH`).
pub const RELIEF_MAX_DEPTH: f64 = 0.25;

/// `extras.evani.relief` on materials (S3-10, docs/MODEL_FORMAT.md
/// "Surface relief"): the engine turns off any part of a recipe it can't
/// read without a word, so the model draws flatter than its author made it.
/// Every such part is a finding, and so is relief on a material whose
/// primitives carry no tangents (relief tips the mapped normal, which needs
/// them).
pub fn relief_problems(g: &Gltf) -> Vec<String> {
    let mut out = Vec::new();
    for (mi, m) in g.materials.iter().enumerate() {
        let Some(r) = m.extras.pointer("/evani/relief") else {
            continue;
        };
        let at = format!("material `{}` relief", if m.name.is_empty() { mi.to_string() } else { m.name.clone() });
        let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        if !r.is_object() {
            out.push(format!("{at} is not an object"));
            continue;
        }
        if let Some(h) = r.get("height") {
            match h.get("index").and_then(Value::as_u64) {
                Some(i) if (i as usize) < g.textures.len() => {}
                i => out.push(format!("{at}: height.index {i:?} is not one of the file's {} textures", g.textures.len())),
            }
            match num(h, "depth") {
                Some(d) if d > 0.0 && d <= RELIEF_MAX_DEPTH => {}
                d => out.push(format!("{at}: height.depth {d:?} is not above 0 and at most {RELIEF_MAX_DEPTH} (UV units)")),
            }
        }
        if let Some(mc) = r.get("micro") {
            let p = mc.get("pattern").and_then(Value::as_str);
            if !p.is_some_and(|p| RELIEF_PATTERNS.contains(&p)) {
                out.push(format!("{at}: micro.pattern {p:?} is not one of {RELIEF_PATTERNS:?}"));
            }
            if !num(mc, "size").is_some_and(|s| s > 0.0) {
                out.push(format!("{at}: micro.size {:?} is not a period above 0 (UV units)", mc.get("size")));
            }
            if mc.get("depth").is_some() && !num(mc, "depth").is_some_and(|d| (0.0..=1.0).contains(&d)) {
                out.push(format!("{at}: micro.depth {:?} is not 0 to 1", mc.get("depth")));
            }
            if mc.get("angle").is_some() && num(mc, "angle").is_none() {
                out.push(format!("{at}: micro.angle {:?} is not a number (degrees)", mc.get("angle")));
            }
            let mask_ok = match mc.get("mask") {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) => s == "none",
                Some(o) => {
                    matches!(o.get("texture").and_then(Value::as_str), Some("base" | "normal"))
                        && matches!(o.get("channel").and_then(Value::as_str), Some("r" | "g" | "b" | "a"))
                }
            };
            if !mask_ok {
                out.push(format!("{at}: micro.mask {:?} is not \"none\" or {{\"texture\": \"base\" | \"normal\", \"channel\": \"r\" | \"g\" | \"b\" | \"a\"}}", mc.get("mask")));
            }
        }
        let used: Vec<_> = g.meshes.iter().flat_map(|me| &me.primitives).filter(|p| p.material == Some(mi)).collect();
        if used.iter().any(|p| p.attribute("TANGENT").is_none()) {
            out.push(format!("{at}: a primitive using it has no TANGENT attribute, so the relief never shows (export tangents)"));
        }
    }
    out
}

/// [`read`] then [`skin_warnings`] for one file on disk (nothing when it
/// can't be read: [`file_problems`] reports that).
pub fn file_warnings(path: &Path) -> Vec<String> {
    read(path).map(|g| skin_warnings(&g)).unwrap_or_default()
}

/// [`read`] then [`problems`] for one file on disk.
pub fn file_problems(path: &Path) -> Vec<String> {
    match read(path) {
        Ok(g) => problems(&g, path.parent().unwrap_or(Path::new("."))),
        Err(e) => vec![e],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
        let mut js = serde_json::to_vec(doc).unwrap();
        while !js.len().is_multiple_of(4) {
            js.push(b' ');
        }
        let mut bin = bin.to_vec();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((js.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(&js);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&bin);
        out
    }

    /// One triangle whose int16 normals all point up at `n` / 32767, with
    /// KHR_mesh_quantization, plus `extras` on the root.
    fn triangle(n: i16, extras: Value) -> Vec<u8> {
        let mut bin = Vec::new();
        for p in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            p.iter().for_each(|c| bin.extend(c.to_le_bytes()));
        }
        for _ in 0..3 {
            // x, y, z, pad: an 8-byte stride keeps each element 4-aligned.
            [0i16, n, 0, 0].iter().for_each(|c| bin.extend(c.to_le_bytes()));
        }
        let doc = json!({
            "asset": {"version": "2.0"},
            "extensionsUsed": ["KHR_mesh_quantization"],
            "extensionsRequired": ["KHR_mesh_quantization"],
            "buffers": [{"byteLength": 60}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 24, "byteStride": 8}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 0, 1]},
                {"bufferView": 1, "componentType": 5122, "normalized": true, "count": 3, "type": "VEC3"}
            ],
            "meshes": [{"name": "body", "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}],
            // An embedded image (never a missing file) for texture sets to point at.
            "images": [{"uri": "data:image/png;base64,AAAA"}],
            "textures": [{"source": 0}],
            "nodes": [{"name": "kart_lod0", "mesh": 0}],
            "scenes": [{"nodes": [0]}],
            "scene": 0,
            "extras": extras
        });
        glb(&doc, &bin)
    }

    /// The triangle skinned to a rig of `joints` joints (all under the mesh
    /// node's parent), with or without JOINTS_0 and WEIGHTS_0.
    fn skinned(joints: usize, weights: bool) -> Gltf {
        let mut bin = Vec::new();
        for p in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            p.iter().for_each(|c| bin.extend(c.to_le_bytes()));
        }
        bin.extend([0u8; 12]);
        for _ in 0..3 {
            [1f32, 0.0, 0.0, 0.0].iter().for_each(|c| bin.extend(c.to_le_bytes()));
        }
        let mut attrs = json!({"POSITION": 0});
        if weights {
            attrs["JOINTS_0"] = json!(1);
            attrs["WEIGHTS_0"] = json!(2);
        }
        let mut nodes = vec![json!({"name": "rig", "children": (1..=joints + 1).collect::<Vec<_>>()}), json!({"name": "body", "mesh": 0, "skin": 0})];
        nodes.extend((0..joints).map(|k| json!({"name": format!("j{k}")})));
        let doc = json!({
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength": bin.len()}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 12}, {"buffer": 0, "byteOffset": 48, "byteLength": 48}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 0, 1]},
                {"bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC4"},
                {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4"}
            ],
            "meshes": [{"name": "body", "primitives": [{"attributes": attrs}]}],
            "skins": [{"joints": (2..joints + 2).collect::<Vec<_>>()}],
            "nodes": nodes,
            "scenes": [{"nodes": [0]}],
            "scene": 0
        });
        Gltf::from_glb(&glb(&doc, &bin)).unwrap()
    }

    /// GPU skinning's joint caps (S3-9): up to 64 joints is quiet, over 64
    /// a warning (not a problem: it draws), over 256 an error, and a
    /// skinned mesh without joints and weights an error.
    #[test]
    fn skins_over_the_soft_cap_warn() {
        let dir = Path::new(".");
        let ok = skinned(SOFT_JOINTS, true);
        assert!(skin_warnings(&ok).is_empty() && problems(&ok, dir).is_empty());
        let big = skinned(SOFT_JOINTS + 1, true);
        let w = skin_warnings(&big);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("65 joints") && w[0].contains("soft cap of 64"), "{w:?}");
        assert!(problems(&big, dir).is_empty(), "a warning, not an error");
        let huge = skinned(MAX_JOINTS + 1, true);
        assert!(skin_warnings(&huge).is_empty());
        assert!(problems(&huge, dir).iter().any(|p| p.contains("257 joints")));
        let bare = skinned(4, false);
        assert!(problems(&bare, dir).iter().any(|p| p.contains("without JOINTS_0 and WEIGHTS_0")));
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("evani-modelcheck-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A plain 2D KTX2 header with one level of `len` bytes at offset 104.
    fn ktx2(vk: u32, w: u32, h: u32, supercompression: u32, len: u64) -> Vec<u8> {
        let mut b = vec![0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A];
        for v in [vk, 1, w, h, 0, 0, 1, 1, supercompression] {
            b.extend(v.to_le_bytes());
        }
        b.resize(80, 0);
        for v in [104u64, len, len] {
            b.extend(v.to_le_bytes());
        }
        b.resize(104 + len as usize, 0);
        b
    }

    #[test]
    fn ktx2_rules_match_the_engine_reader() {
        assert_eq!(ktx2_check(&ktx2(157, 8, 8, 0, 64)), Ok(()), "ASTC 4x4 8x8: four 16-byte blocks");
        assert_eq!(ktx2_check(&ktx2(145, 6, 6, 0, 64)), Ok(()), "BC7 rounds partial blocks up");
        assert!(ktx2_check(&ktx2(157, 8, 8, 2, 64)).unwrap_err().contains("supercompressed"));
        assert!(ktx2_check(&ktx2(37, 8, 8, 0, 64)).unwrap_err().contains("not ASTC 4x4 or BC7"));
        assert!(ktx2_check(&ktx2(157, 8, 8, 0, 48)).unwrap_err().contains("needs 64"));
        let mut cut = ktx2(157, 8, 8, 0, 64);
        cut.truncate(120);
        assert!(ktx2_check(&cut).unwrap_err().contains("past the end"));
        assert!(ktx2_check(b"not a texture").is_err());
        assert_eq!(ktx2_vk_format(&ktx2(146, 4, 4, 0, 16)), Some(146));
        assert_eq!(set_formats("astc"), &[157, 158]);
        assert!(set_formats("rgba").is_empty());
    }

    #[test]
    fn short_normals_are_found_in_the_decoded_file() {
        // 2047 / 32767 = 0.0625: 12-bit normals stored as int16 (gltfpack -vn 12).
        let bad = Gltf::from_glb(&triangle(2047, json!({}))).unwrap();
        let lines = bad_normals(&bad);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("3 of 3 normals") && lines[0].contains("0.0625") && lines[0].contains("`body`"), "{}", lines[0]);
        let good = Gltf::from_glb(&triangle(32767, json!({}))).unwrap();
        assert!(bad_normals(&good).is_empty());
        assert!(problems(&good, Path::new(".")).is_empty());
    }

    #[test]
    fn missing_files_and_lod_nodes_are_named() {
        let d = tmp("missing");
        let extras = json!({"evani": {
            "format": 1,
            "lods": [{"node": "kart_lod0", "min_screen": 0.2}, {"node": "kart_lod1", "min_screen": 0.0}],
            "textures": [{"texture": 0, "name": "base", "sets": {"astc": "textures/base.astc.ktx2", "bc7": "textures/base.bc7.ktx2"}}],
            "normal_maps": {"maps": [{"file": "body.normal.png"}]}
        }});
        let file = d.join("kart.low.glb");
        std::fs::write(&file, triangle(32767, extras)).unwrap();
        std::fs::create_dir_all(d.join("textures")).unwrap();
        std::fs::write(d.join("textures/base.astc.ktx2"), ktx2(157, 8, 8, 0, 64)).unwrap();
        std::fs::write(d.join("textures/base.bc7.ktx2"), ktx2(157, 8, 8, 0, 64)).unwrap();
        let p = file_problems(&file);
        let has = |s: &str| p.iter().any(|l| l.contains(s));
        assert!(has("`kart_lod1` is not in the file"), "{p:?}");
        assert!(!has("`kart_lod0`"), "{p:?}");
        assert!(has("normal map file `body.normal.png` is missing"), "{p:?}");
        assert!(has("`textures/base.bc7.ktx2` is listed as `bc7`"), "an ASTC file under the bc7 key: {p:?}");
        assert!(!has("base.astc"), "the ASTC set is fine: {p:?}");
        assert_eq!(p.len(), 3, "{p:?}");
        // A file the reader refuses is one finding, not a panic.
        std::fs::write(d.join("broken.glb"), b"glTF nonsense").unwrap();
        let p = file_problems(&d.join("broken.glb"));
        assert!(p.len() == 1 && p[0].contains("refuses it"), "{p:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The shared relief fixture is clean; every unreadable part of a recipe
    /// and relief without tangents is named; the pattern list and depth cap
    /// are the renderer's own.
    #[test]
    fn relief_recipes_the_engine_would_drop_are_named() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let tile = root.join("tests/fixtures/models/relief-tile.glb");
        assert_eq!(file_problems(&tile), Vec::<String>::new());
        let mut g = read(&tile).unwrap();
        assert!(g.materials.iter().any(|m| m.extras.pointer("/evani/relief/height").is_some()));
        g.materials[0].extras = json!({"evani": {"relief": {
            "height": {"index": 99, "depth": 0.5},
            "micro": {"pattern": "waves", "size": 0, "depth": 2, "angle": "steep", "mask": {"texture": "height", "channel": "r"}}
        }}});
        let p = relief_problems(&g);
        for s in ["height.index Some(99)", "height.depth Some(0.5)", "micro.pattern Some(\"waves\")", "micro.size", "micro.depth", "micro.angle", "micro.mask"] {
            assert!(p.iter().any(|l| l.contains(s)), "{s}: {p:?}");
        }
        assert_eq!(p.len(), 7, "{p:?}");
        // Relief on the triangle (no tangents) never shows.
        let mut t = Gltf::from_glb(&triangle(32767, json!({}))).unwrap();
        t.materials.push(g.materials[1].clone());
        t.meshes[0].primitives[0].material = Some(0);
        let p = relief_problems(&t);
        assert!(p.len() == 1 && p[0].contains("no TANGENT"), "{p:?}");
        // Drift guard against the renderer's reader.
        let src = std::fs::read_to_string(root.join("crates/evani-render/src/model/relief.rs")).unwrap();
        assert!(src.contains(&format!("pub const PATTERNS: [&str; 4] = {RELIEF_PATTERNS:?};")), "evani-render's relief patterns changed");
        assert!(src.contains(&format!("pub const MAX_DEPTH: f32 = {RELIEF_MAX_DEPTH};")), "evani-render's relief depth cap changed");
    }

    #[test]
    fn uris_and_data_uris_decode() {
        assert_eq!(uri_path("a%20b.png"), "a b.png");
        assert_eq!(data_uri("data:application/octet-stream;base64,AAEC/w==").unwrap(), vec![0, 1, 2, 255]);
    }
}
