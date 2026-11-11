//! The studio's per-vertex tags through the packer (Asset & Style Studio
//! WP7; docs/studio/CONTRACTS.md): `_EVANI_PART` (the region index, one
//! unsigned byte per vertex) and `_EVANI_COAT` (the coat chart, unorm8 x4:
//! face, belly, lower legs, patches), written by the quadruped generator
//! (tools/models/quadruped).
//!
//! gltfpack drops every custom attribute, so a tier file would lose them.
//! [`park`] moves them into vertex colour slots gltfpack keeps byte for byte
//! (`_EVANI_COAT` into `COLOR_0`; `_EVANI_PART` into `COLOR_1` as unorm8 x4,
//! the index in the first byte), and [`unpark`] names them back in each
//! tier file. In a tier file `_EVANI_PART` is therefore unorm8 x4 with the
//! index in its first channel (x 255); the master keeps the input's own
//! layout. A model that already has vertex colours and these tags is
//! refused, never guessed.
//!
//! A stop-gap until the general custom-attribute carry (hair-v2 branch,
//! `attributes.rs`) reaches `studio`; that one replaces this module.

use serde_json::{Value, json};

use crate::glb::Doc;

pub const PART: &str = "_EVANI_PART";
pub const COAT: &str = "_EVANI_COAT";
const PART_SLOT: &str = "COLOR_1";
const COAT_SLOT: &str = "COLOR_0";

/// Whether any primitive carries a studio tag.
pub fn has(j: &Value) -> bool {
    prims(j).any(|p| p.get("attributes").is_some_and(|a| a.get(PART).is_some() || a.get(COAT).is_some()))
}

fn prims(j: &Value) -> impl Iterator<Item = &Value> {
    j.get("meshes").and_then(Value::as_array).into_iter().flatten().flat_map(|m| m.get("primitives").and_then(Value::as_array).into_iter().flatten())
}

fn idx(v: &Value, k: &str) -> Option<usize> {
    v.get(k).and_then(Value::as_u64).and_then(|x| usize::try_from(x).ok())
}

/// The bytes of a plain (uncompressed) u8 scalar accessor.
fn read_u8_scalar(d: &Doc, acc_i: usize) -> Result<Vec<u8>, String> {
    let acc = d.json.pointer(&format!("/accessors/{acc_i}")).ok_or_else(|| format!("{PART}: accessor {acc_i} missing"))?;
    if acc.get("componentType").and_then(Value::as_u64) != Some(5121) || acc.get("type").and_then(Value::as_str) != Some("SCALAR") {
        return Err(format!("{PART} must be an unsigned byte scalar per vertex (docs/studio/CONTRACTS.md); write it with tools/models/quadruped/glbfix.py"));
    }
    let count = idx(acc, "count").ok_or(format!("{PART}: no count"))?;
    let view_i = idx(acc, "bufferView").ok_or(format!("{PART}: no buffer view"))?;
    let view = d.json.pointer(&format!("/bufferViews/{view_i}")).ok_or(format!("{PART}: buffer view missing"))?;
    let stride = idx(view, "byteStride").unwrap_or(1);
    let off = idx(view, "byteOffset").unwrap_or(0) + idx(acc, "byteOffset").unwrap_or(0);
    (0..count).map(|i| d.bin.get(off + i * stride).copied().ok_or_else(|| format!("{PART}: past the end of the buffer"))).collect()
}

/// Move the tags into the colour slots gltfpack keeps. Returns the number of
/// primitives moved.
pub fn park(d: &mut Doc) -> Result<usize, String> {
    let n_prims: Vec<(usize, usize)> = d
        .json
        .get("meshes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(mi, m)| (0..m.get("primitives").and_then(Value::as_array).map_or(0, Vec::len)).map(move |pi| (mi, pi)))
        .collect();
    let mut moved = 0;
    let mut done_part: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for (mi, pi) in n_prims {
        let ptr = format!("/meshes/{mi}/primitives/{pi}/attributes");
        let Some(attrs) = d.json.pointer(&ptr).cloned() else { continue };
        let (part, coat) = (idx(&attrs, PART), idx(&attrs, COAT));
        if part.is_none() && coat.is_none() {
            continue;
        }
        for slot in [PART_SLOT, COAT_SLOT] {
            if attrs.get(slot).is_some() {
                return Err(format!("mesh {mi} primitive {pi}: has {slot} and the studio tags; vertex colours and `{PART}`/`{COAT}` can't both be carried"));
            }
        }
        let mut new_attrs = attrs.clone();
        if let Some(c) = coat {
            let acc = &d.json["accessors"][c];
            if acc.get("componentType").and_then(Value::as_u64) != Some(5121) || acc.get("type").and_then(Value::as_str) != Some("VEC4") || acc.get("normalized").and_then(Value::as_bool) != Some(true) {
                return Err(format!("{COAT} must be unorm8 x4 (docs/studio/CONTRACTS.md); write it with tools/models/quadruped/glbfix.py"));
            }
            if let Some(o) = new_attrs.as_object_mut() {
                o.remove(COAT);
                o.insert(COAT_SLOT.into(), Value::from(c));
            }
        }
        if let Some(p) = part {
            let a = match done_part.get(&p) {
                Some(a) => *a,
                None => {
                    let vals = read_u8_scalar(d, p)?;
                    while !d.bin.len().is_multiple_of(4) {
                        d.bin.push(0);
                    }
                    let off = d.bin.len();
                    for v in &vals {
                        d.bin.extend_from_slice(&[*v, 0, 0, 255]);
                    }
                    let views = d.json["bufferViews"].as_array_mut().ok_or("no bufferViews")?;
                    views.push(json!({"buffer": 0, "byteOffset": off, "byteLength": vals.len() * 4, "byteStride": 4, "target": 34962}));
                    let view_i = views.len() - 1;
                    let accs = d.json["accessors"].as_array_mut().ok_or("no accessors")?;
                    accs.push(json!({"bufferView": view_i, "componentType": 5121, "normalized": true, "count": vals.len(), "type": "VEC4"}));
                    let a = accs.len() - 1;
                    done_part.insert(p, a);
                    a
                }
            };
            if let Some(o) = new_attrs.as_object_mut() {
                o.remove(PART);
                o.insert(PART_SLOT.into(), Value::from(a));
            }
        }
        if let Some(t) = d.json.pointer_mut(&ptr) {
            *t = new_attrs;
        }
        moved += 1;
    }
    Ok(moved)
}

/// A packed file's parked tags named back, in every primitive. Returns the
/// number of primitives named back.
pub fn unpark(j: &mut Value) -> usize {
    let mut n = 0;
    for m in j.get_mut("meshes").and_then(Value::as_array_mut).into_iter().flatten() {
        for p in m.get_mut("primitives").and_then(Value::as_array_mut).into_iter().flatten() {
            let Some(a) = p.get_mut("attributes").and_then(Value::as_object_mut) else { continue };
            let mut hit = false;
            for (slot, name) in [(PART_SLOT, PART), (COAT_SLOT, COAT)] {
                if let Some(v) = a.remove(slot) {
                    a.insert(name.into(), v);
                    hit = true;
                }
            }
            n += usize::from(hit);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Doc {
        // two vertices: positions (2 x 12 bytes), part [3, 7] (stride 4), coat (2 x 4 bytes)
        let mut bin = Vec::new();
        for v in [0f32, 0., 0., 1., 0., 0.] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        bin.extend_from_slice(&[3, 0, 0, 0, 7, 0, 0, 0]);
        bin.extend_from_slice(&[255, 0, 128, 10, 0, 255, 0, 20]);
        Doc {
            json: json!({
                "asset": {"version": "2.0"},
                "buffers": [{"byteLength": bin.len()}],
                "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 24}, {"buffer": 0, "byteOffset": 24, "byteLength": 8, "byteStride": 4}, {"buffer": 0, "byteOffset": 32, "byteLength": 8}],
                "accessors": [{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3"},
                              {"bufferView": 1, "componentType": 5121, "count": 2, "type": "SCALAR"},
                              {"bufferView": 2, "componentType": 5121, "normalized": true, "count": 2, "type": "VEC4"}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "_EVANI_PART": 1, "_EVANI_COAT": 2}}, {"attributes": {"POSITION": 0}}]}]
            }),
            bin,
        }
    }

    #[test]
    fn park_moves_the_tags_into_colour_slots_and_unpark_names_them_back() {
        let mut d = doc();
        assert!(has(&d.json));
        assert_eq!(park(&mut d).unwrap(), 1);
        let a = &d.json["meshes"][0]["primitives"][0]["attributes"];
        assert!(a.get(PART).is_none() && a.get(COAT).is_none());
        assert_eq!(a[COAT_SLOT], 2);
        let p = usize::try_from(a[PART_SLOT].as_u64().unwrap()).unwrap();
        assert_eq!(d.json["accessors"][p]["type"], "VEC4");
        // the index rides in the first byte of each unorm8 x4
        let g = evani_gltf::Gltf::from_glb(&d.to_glb().unwrap()).unwrap();
        let vals = g.read_f32(p).unwrap();
        assert_eq!((vals[0] * 255.0).round() as u32, 3);
        assert_eq!((vals[4] * 255.0).round() as u32, 7);
        let mut j = d.json.clone();
        assert_eq!(unpark(&mut j), 1);
        let a = &j["meshes"][0]["primitives"][0]["attributes"];
        assert!(a.get(PART).is_some() && a.get(COAT).is_some() && a.get(COAT_SLOT).is_none());
    }

    #[test]
    fn vertex_colours_and_tags_together_are_refused() {
        let mut d = doc();
        d.json["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"] = json!(2);
        assert!(park(&mut d).unwrap_err().contains("can't both be carried"));
    }

    #[test]
    fn a_float_part_index_is_refused_with_the_fix() {
        let mut d = doc();
        d.json["accessors"][1]["componentType"] = json!(5126);
        assert!(park(&mut d).unwrap_err().contains("glbfix.py"));
    }
}
