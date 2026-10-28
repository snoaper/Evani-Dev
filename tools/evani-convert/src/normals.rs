//! Normals for triangle meshes that have none (found by the quality gate,
//! S3-6). The engine draws such a mesh with smooth normals summed over its
//! own index buffer (evani-render `model.rs`, `smooth_normals`). gltfpack
//! merges vertices that share a position, which changes that sum, so a
//! converted file shaded differently from its master (kart_pip: SSIMULACRA2
//! -51 at full quality). The converter therefore writes the engine's own
//! normals into its working copy first; the master stays untouched.

use std::path::Path;

use serde_json::{Value, json};

use crate::glb::Doc;

/// The engine's normals for a mesh without them: face normals (area
/// weighted) summed per vertex index, then normalised. Same arithmetic as
/// evani-render's `smooth_normals`, so the converted file shades like the master.
pub fn engine_normals(pos: &[f32], idx: &[u32]) -> Vec<f32> {
    let mut n = vec![0.0f32; pos.len()];
    for t in idx.as_chunks::<3>().0 {
        let p = |i: u32| {
            let i = i as usize * 3;
            [pos.get(i).copied().unwrap_or(0.0), pos.get(i + 1).copied().unwrap_or(0.0), pos.get(i + 2).copied().unwrap_or(0.0)]
        };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for (k, fk) in f.iter().enumerate() {
                if let Some(x) = n.get_mut(i as usize * 3 + k) {
                    *x += fk;
                }
            }
        }
    }
    for c in n.as_chunks_mut::<3>().0 {
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-12);
        c.iter_mut().for_each(|x| *x /= l);
    }
    n
}

/// Write `master` with the engine's normals added to every triangle
/// primitive that has none to `out`. Returns how many primitives gained
/// normals (0: nothing written; compressed inputs are left alone).
pub fn add_engine_normals(master: &Path, out: &Path) -> Result<usize, String> {
    let bytes = std::fs::read(master).map_err(|e| format!("{}: {e}", master.display()))?;
    let mut d = Doc::read_glb(&bytes).map_err(|e| format!("{}: {e}", master.display()))?;
    let compressed = d.arr("extensionsUsed").iter().any(|e| matches!(e.as_str(), Some("EXT_meshopt_compression" | "KHR_draco_mesh_compression")));
    if compressed || d.arr("buffers").len() != 1 {
        return Ok(0);
    }
    let g = evani_gltf::Gltf::from_glb(&bytes).map_err(|e| format!("{}: {}", master.display(), e.0))?;
    let mut added = 0;
    for (mi, m) in g.meshes.iter().enumerate() {
        for (pi, p) in m.primitives.iter().enumerate() {
            if p.mode != 4 || p.attribute("NORMAL").is_some() {
                continue;
            }
            let Some(pa) = p.attribute("POSITION") else { continue };
            let pos = g.read_f32(pa).map_err(|e| format!("{}: mesh {mi} positions: {}", master.display(), e.0))?;
            let count = pos.len() / 3;
            let idx = match p.indices {
                Some(i) => g.read_u32(i).map_err(|e| format!("{}: mesh {mi} indices: {}", master.display(), e.0))?,
                None => (0..count as u32).collect(),
            };
            let nrm = engine_normals(&pos, &idx);
            while !d.bin.len().is_multiple_of(4) {
                d.bin.push(0);
            }
            let offset = d.bin.len();
            for x in &nrm {
                d.bin.extend_from_slice(&x.to_le_bytes());
            }
            let views = d.json.get_mut("bufferViews").and_then(Value::as_array_mut).ok_or("no bufferViews")?;
            views.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": nrm.len() * 4, "target": 34962 }));
            let view = views.len() - 1;
            let accs = d.json.get_mut("accessors").and_then(Value::as_array_mut).ok_or("no accessors")?;
            accs.push(json!({ "bufferView": view, "componentType": 5126, "count": count, "type": "VEC3" }));
            let acc = accs.len() - 1;
            let attrs = d.json.pointer_mut(&format!("/meshes/{mi}/primitives/{pi}/attributes")).and_then(Value::as_object_mut).ok_or("primitive without attributes")?;
            attrs.insert("NORMAL".into(), Value::from(acc));
            added += 1;
        }
    }
    if added > 0 {
        std::fs::write(out, d.to_glb()?).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_normals_are_unit_and_face_the_winding() {
        // One triangle in the xy plane, counter-clockwise: +z.
        let n = engine_normals(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2]);
        for c in n.as_chunks::<3>().0 {
            assert!((c[2] - 1.0).abs() < 1e-6 && c[0].abs() < 1e-6 && c[1].abs() < 1e-6, "{c:?}");
        }
    }
}
