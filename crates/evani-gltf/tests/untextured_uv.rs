//! Every shipped game model keeps true texture coordinates where a material
//! reads them without a texture. gltfpack quantizes texture coordinates to
//! `-vt` bits and puts the undo (a `KHR_texture_transform` scale) on the
//! material's textures only; a material with no texture has nowhere to keep
//! it, so its coordinates arrive 4 to 16 times too small. The character
//! shading reads an eye's iris from its coordinates and hair locks from
//! theirs, the layered cloth its weave: the material viewer's head lost its
//! irises on every converted tier (critic round 2 of material-view). The
//! converter now keeps such coordinates as floats (`-vtf`, evani-convert
//! `untextured_uv`); this scans `apps/*/assets/models/*.glb` so a model
//! packed the old way fails here before it reaches a review picture.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use evani_gltf::split_glb;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Texture slots a material can carry the undo on.
const SLOTS: [&str; 5] = ["/pbrMetallicRoughness/baseColorTexture", "/pbrMetallicRoughness/metallicRoughnessTexture", "/normalTexture", "/occlusionTexture", "/emissiveTexture"];

/// Primitives of `glb` whose material has no texture but whose
/// TEXCOORD_0 is a normalized integer (quantized with nowhere to undo it).
fn shrunk(glb: &[u8]) -> usize {
    let doc: serde_json::Value = serde_json::from_slice(split_glb(glb).unwrap().0).unwrap();
    let mats = doc["materials"].as_array().cloned().unwrap_or_default();
    let mut bad = 0;
    for m in doc["meshes"].as_array().into_iter().flatten() {
        for p in m["primitives"].as_array().into_iter().flatten() {
            let Some(uv) = p.pointer("/attributes/TEXCOORD_0").and_then(serde_json::Value::as_u64) else { continue };
            let textured = p["material"].as_u64().and_then(|i| mats.get(i as usize)).is_some_and(|mat| SLOTS.iter().any(|s| mat.pointer(s).is_some()));
            let a = &doc["accessors"][uv as usize];
            if !textured && a["normalized"].as_bool() == Some(true) {
                bad += 1;
            }
        }
    }
    bad
}

/// Models packed before the converter kept such coordinates, by file under
/// `apps/`, with their count of shrunk primitives: other apps' proof
/// models, listed for their owners to reconvert (the sandbox's layered
/// proof draws its velvet weave from them). A count that grows, or a file
/// not listed, fails.
const KNOWN: &[(&str, usize)] = &[
    ("kart-racer/proof-barrel.glb", 1),
    ("kart-racer/proof-crate.glb", 1),
    ("kart-racer/proof-kart.glb", 3),
    ("sandbox/layer-brushed.glb", 2),
    ("sandbox/layer-brushed.low.glb", 2),
    ("sandbox/layer-bubble.glb", 2),
    ("sandbox/layer-bubble.low.glb", 2),
    ("sandbox/layer-coat.glb", 2),
    ("sandbox/layer-coat.low.glb", 2),
    ("sandbox/layer-velvet.glb", 2),
    ("sandbox/layer-velvet.low.glb", 2),
    ("sandbox/proof-barrel.glb", 1),
    ("sandbox/proof-crate.glb", 1),
    ("sandbox/proof-kart.glb", 3),
];

#[test]
fn no_shipped_model_shrinks_untextured_coordinates() {
    let mut found = Vec::new();
    for app in std::fs::read_dir(repo().join("apps")).unwrap().flatten() {
        let dir = app.path().join("assets").join("models");
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for f in list.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("glb") {
                continue;
            }
            let bad = shrunk(&std::fs::read(&path).unwrap());
            let rel = format!("{}/{}", app.file_name().to_string_lossy(), f.file_name().to_string_lossy());
            let known = KNOWN.iter().find(|(f, _)| *f == rel).map_or(0, |k| k.1);
            if bad > known {
                found.push(format!("{rel}: {bad} primitive(s)"));
            }
        }
    }
    assert!(found.is_empty(), "untextured texture coordinates quantized with no undo (reconvert with `cargo xtask convert`):\n{}", found.join("\n"));
}

#[test]
fn a_quantized_untextured_primitive_is_found() {
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "materials": [{ "name": "eye" }],
        "accessors": [{ "componentType": 5126, "count": 3, "type": "VEC3" }, { "componentType": 5123, "normalized": true, "count": 3, "type": "VEC2" }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0, "TEXCOORD_0": 1 }, "material": 0 }] }]
    });
    let mut j = serde_json::to_vec(&json).unwrap();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let mut glb = Vec::new();
    glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((12 + 8 + j.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(j.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    glb.extend_from_slice(&j);
    assert_eq!(shrunk(&glb), 1);
}
