//! Every shipped game model's vertex normals face the same way as the
//! triangles they belong to. A normal that points against its own triangle
//! (dot below -0.5 with the face normal) shades that corner as if lit from
//! behind: the material viewer's test balls had their pole normals flipped
//! and drew a dark dot on top of every ball (critic round 1 of
//! material-view). This scans `apps/*/assets/models/*.glb` with the engine's
//! own reader, so a model with the fault fails here before it reaches a
//! review picture.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use evani_gltf::{Gltf, split_glb, transform_point};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Vertices whose normal points against every (non-degenerate) triangle
/// that uses it, and the vertices checked, over every primitive of `glb`.
/// A few folded triangles from simplification leave a vertex with some
/// triangles still agreeing; a flipped vertex normal agrees with none.
fn flipped(glb: &[u8]) -> (usize, usize) {
    let g = Gltf::from_glb(glb).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(split_glb(glb).unwrap().0).unwrap();
    let mats = g.world_matrices();
    let (mut bad, mut all) = (0, 0);
    for (ni, node) in doc["nodes"].as_array().into_iter().flatten().enumerate() {
        let Some(mi) = node["mesh"].as_u64() else { continue };
        let m = &mats[ni];
        // The node's linear part (no translation) turns normals well enough
        // for a sign test.
        let lin = |v: [f32; 3]| {
            let a = transform_point(m, v);
            let o = transform_point(m, [0.0; 3]);
            [a[0] - o[0], a[1] - o[1], a[2] - o[2]]
        };
        // A mirrored node (negative determinant) winds its triangles the
        // other way in the world: its front face is clockwise (the glTF rule,
        // and what the engine draws: model.wgsl `mirror`), so the face
        // normal from the world corners turns round while the normals turn
        // with the node. Without this every corner of a mirrored part, such
        // as the kart's right-hand wheels, counted as flipped.
        let [x, y, z] = [lin([1.0, 0.0, 0.0]), lin([0.0, 1.0, 0.0]), lin([0.0, 0.0, 1.0])];
        let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1]) + z[0] * (x[1] * y[2] - x[2] * y[1]);
        let face_sign = if det < 0.0 { -1.0 } else { 1.0 };
        for prim in doc["meshes"][mi as usize]["primitives"].as_array().into_iter().flatten() {
            let at = &prim["attributes"];
            let (Some(pi), Some(nidx)) = (at["POSITION"].as_u64(), at["NORMAL"].as_u64()) else { continue };
            let p = g.read_f32(pi as usize).unwrap();
            let n = g.read_f32(nidx as usize).unwrap();
            let count = p.len() / 3;
            let idx: Vec<u32> = match prim["indices"].as_u64() {
                Some(i) => g.read_u32(i as usize).unwrap(),
                None => (0..count as u32).collect(),
            };
            let pw: Vec<[f32; 3]> = (0..count).map(|k| transform_point(m, [p[k * 3], p[k * 3 + 1], p[k * 3 + 2]])).collect();
            let nw: Vec<[f32; 3]> = (0..count).map(|k| lin([n[k * 3], n[k * 3 + 1], n[k * 3 + 2]])).collect();
            // Per vertex: triangles that use it, and how many it points against.
            let mut uses = vec![(0u32, 0u32); count];
            for t in idx.as_chunks::<3>().0 {
                let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
                let e1 = [pw[b][0] - pw[a][0], pw[b][1] - pw[a][1], pw[b][2] - pw[a][2]];
                let e2 = [pw[c][0] - pw[a][0], pw[c][1] - pw[a][1], pw[c][2] - pw[a][2]];
                let f = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]].map(|c| c * face_sign);
                let fl = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
                if fl < 1e-10 {
                    continue;
                }
                for v in [a, b, c] {
                    let nv = nw[v];
                    let nl = (nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]).sqrt();
                    if nl < 1e-6 {
                        continue;
                    }
                    uses[v].0 += 1;
                    if (nv[0] * f[0] + nv[1] * f[1] + nv[2] * f[2]) / (nl * fl) < -0.5 {
                        uses[v].1 += 1;
                    }
                }
            }
            all += uses.iter().filter(|u| u.0 > 0).count();
            bad += uses.iter().filter(|u| u.0 > 0 && u.1 == u.0).count();
        }
    }
    (bad, all)
}

/// Known faults left for their owners, with their flipped-vertex count. A
/// file here may only get better: more flipped vertices, or any other file
/// with one, fails. Empty since 2026-10-25: the kart proof models' 402 were
/// 401 corners of the two mirrored wheels (this test missing the mirror
/// rule above) and one real corner, the barrel's 8 real; those 9 were
/// repaired with `tools/models/fix_flipped_normals.py --fix`.
const KNOWN: &[(&str, usize)] = &[];

#[test]
fn shipped_model_normals_face_their_triangles() {
    let mut report = Vec::new();
    let mut files = 0;
    for app in std::fs::read_dir(repo().join("apps")).unwrap().flatten() {
        let dir = app.path().join("assets/models");
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for f in list.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("glb") {
                continue;
            }
            files += 1;
            let (bad, all) = flipped(&std::fs::read(&path).unwrap());
            let rel = path.strip_prefix(repo()).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let known = KNOWN.iter().find(|(f, _)| *f == rel).map_or(0, |k| k.1);
            if bad > known {
                report.push(format!("{rel}: {bad} of {all} vertices (known: {known})"));
            }
        }
    }
    assert!(files > 10, "found {files} model files");
    assert!(report.is_empty(), "vertex normals pointing against their own triangle:\n{}", report.join("\n"));
}

/// The check itself: a triangle whose corners carry the opposite normal is
/// caught, the same triangle with matching normals is not.
#[test]
fn a_flipped_corner_is_caught() {
    fn glb(nz: f32, mirror_x: f32) -> Vec<u8> {
        let mut bin = Vec::new();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        for _ in 0..3 {
            for c in [0.0f32, 0.0, nz] {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let json = serde_json::json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0, "scale": [mirror_x, 1.0, 1.0]}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
                {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}
            ],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 36}],
            "buffers": [{"byteLength": 72}]
        });
        let mut js = serde_json::to_vec(&json).unwrap();
        while !js.len().is_multiple_of(4) {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
    assert_eq!(flipped(&glb(1.0, 1.0)), (0, 3));
    assert_eq!(flipped(&glb(-1.0, 1.0)), (3, 3));
    // Under a mirrored node the same triangle's front face is clockwise: the
    // matching normal (mirrored with it) is fine, the opposite one is caught.
    assert_eq!(flipped(&glb(1.0, -1.0)), (0, 3));
    assert_eq!(flipped(&glb(-1.0, -1.0)), (3, 3));
}
