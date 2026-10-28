//! Put a tier's levels of detail into one document: a root node `<id>`
//! with one child `<id>_lod<k>` per level, each holding that level's scene.
//! Buffers are joined into one; images (by URI), samplers, textures and
//! materials that are the same are kept once. Anchors and the collision
//! proxies are gameplay data, not detail: only level 0 keeps them, so each
//! name stays unique in the file.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::glb::Doc;

/// Node names that only level 0 keeps (with their subtrees).
pub fn lod0_only(name: &str) -> bool {
    name.starts_with("anchor:") || name == "collision" || name.starts_with("col:")
}

pub fn lod_node_name(id: &str, k: usize) -> String {
    format!("{id}_lod{k}")
}

fn idx(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|x| usize::try_from(x).ok())
}

fn remap(v: &mut Value, map: &[usize]) {
    if let Some(i) = idx(v)
        && let Some(&n) = map.get(i)
    {
        *v = Value::from(n);
    }
}

fn add(v: &mut Value, base: usize) {
    if let Some(i) = idx(v) {
        *v = Value::from(i + base);
    }
}

/// Every `{ "index": t }` under a key ending in `Texture` (core and
/// extension texture infos alike).
fn remap_texture_infos(v: &mut Value, map: &[usize]) {
    match v {
        Value::Object(o) => {
            for (k, child) in o.iter_mut() {
                if k.ends_with("Texture")
                    && let Some(i) = child.get_mut("index")
                {
                    remap(i, map);
                }
                remap_texture_infos(child, map);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|c| remap_texture_infos(c, map)),
        _ => {}
    }
}

/// Keep `v` once: the index of an equal entry already in `out`, else push it.
fn dedup_push(out: &mut Vec<Value>, seen: &mut BTreeMap<String, usize>, v: Value) -> usize {
    let key = v.to_string();
    if let Some(&i) = seen.get(&key) {
        return i;
    }
    out.push(v);
    seen.insert(key, out.len() - 1);
    out.len() - 1
}

fn list(j: &Value, k: &str) -> Vec<Value> {
    j.get(k).and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Can this document's levels be merged? Skins and animations point at
/// node indices across the whole file; such models get one level per tier.
pub fn mergeable(d: &Doc) -> Result<(), &'static str> {
    if !d.arr("skins").is_empty() {
        return Err("the model is skinned");
    }
    if !d.arr("animations").is_empty() {
        return Err("the model is animated");
    }
    if !d.arr("cameras").is_empty() {
        return Err("the model has cameras");
    }
    // Lights, instancing and other extensions keep their own lists at the
    // top level or on nodes; the merge does not remap those indices.
    if d.json.get("extensions").and_then(Value::as_object).is_some_and(|o| !o.is_empty()) {
        return Err("the model has file-level extension data (lights or similar)");
    }
    if d.arr("nodes").iter().any(|n| n.get("extensions").and_then(Value::as_object).is_some_and(|o| !o.is_empty())) {
        return Err("a node carries extension data (a light, instancing or similar)");
    }
    Ok(())
}

/// Merge `lods` (most detailed first) under a root node named `id`.
pub fn merge(id: &str, lods: &[Doc]) -> Result<Doc, String> {
    let first = lods.first().ok_or("no levels to merge")?;
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut meshes, mut nodes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut images, mut samplers, mut textures, mut materials) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut seen_img, mut seen_smp, mut seen_tex, mut seen_mat) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let mut used: Vec<String> = Vec::new();
    let mut required: Vec<String> = Vec::new();
    let mut lod_roots = Vec::new();

    for (k, d) in lods.iter().enumerate() {
        mergeable(d).map_err(|e| format!("level {k}: {e}"))?;
        let j = &d.json;
        if list(j, "buffers").len() > 1 {
            return Err(format!("level {k}: more than one buffer"));
        }
        while !bin.len().is_multiple_of(16) {
            bin.push(0);
        }
        let bin_base = bin.len();
        bin.extend_from_slice(&d.bin);

        let view_base = views.len();
        for mut v in list(j, "bufferViews") {
            v["buffer"] = Value::from(0);
            let off = v.get("byteOffset").and_then(idx).unwrap_or(0);
            v["byteOffset"] = Value::from(off + bin_base);
            views.push(v);
        }
        let acc_base = accessors.len();
        for mut a in list(j, "accessors") {
            if let Some(bv) = a.get_mut("bufferView") {
                add(bv, view_base);
            }
            if let Some(s) = a.get_mut("sparse") {
                for p in ["/indices/bufferView", "/values/bufferView"] {
                    if let Some(bv) = s.pointer_mut(p) {
                        add(bv, view_base);
                    }
                }
            }
            accessors.push(a);
        }
        let mut img_map = Vec::new();
        for mut im in list(j, "images") {
            if let Some(bv) = im.get_mut("bufferView") {
                add(bv, view_base);
            }
            img_map.push(dedup_push(&mut images, &mut seen_img, im));
        }
        let smp_map: Vec<usize> = list(j, "samplers").into_iter().map(|s| dedup_push(&mut samplers, &mut seen_smp, s)).collect();
        let mut tex_map = Vec::new();
        for mut t in list(j, "textures") {
            if let Some(s) = t.get_mut("sampler") {
                remap(s, &smp_map);
            }
            if let Some(s) = t.get_mut("source") {
                remap(s, &img_map);
            }
            tex_map.push(dedup_push(&mut textures, &mut seen_tex, t));
        }
        let mut mat_map = Vec::new();
        for mut m in list(j, "materials") {
            remap_texture_infos(&mut m, &tex_map);
            mat_map.push(dedup_push(&mut materials, &mut seen_mat, m));
        }
        let mesh_base = meshes.len();
        for mut m in list(j, "meshes") {
            if let Some(Value::Array(prims)) = m.get_mut("primitives") {
                for p in prims {
                    if let Some(Value::Object(at)) = p.get_mut("attributes") {
                        at.values_mut().for_each(|a| add(a, acc_base));
                    }
                    if let Some(i) = p.get_mut("indices") {
                        add(i, acc_base);
                    }
                    if let Some(mt) = p.get_mut("material") {
                        remap(mt, &mat_map);
                    }
                    if let Some(Value::Array(targets)) = p.get_mut("targets") {
                        for t in targets {
                            if let Value::Object(t) = t {
                                t.values_mut().for_each(|a| add(a, acc_base));
                            }
                        }
                    }
                }
            }
            meshes.push(m);
        }

        // Nodes reachable from the scene, minus level-0-only subtrees on
        // later levels, renumbered in their original order.
        let src = list(j, "nodes");
        let scene = j.get("scene").and_then(idx).unwrap_or(0);
        let roots: Vec<usize> = j.pointer(&format!("/scenes/{scene}/nodes")).and_then(Value::as_array).map(|a| a.iter().filter_map(idx).collect()).unwrap_or_default();
        let mut keep = vec![false; src.len()];
        let mut stack = roots.clone();
        while let Some(n) = stack.pop() {
            let Some(node) = src.get(n) else { continue };
            if keep[n] {
                continue;
            }
            if k > 0 && node.get("name").and_then(Value::as_str).is_some_and(lod0_only) {
                continue;
            }
            keep[n] = true;
            stack.extend(node.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(idx));
        }
        let base = nodes.len();
        let mut node_map = vec![usize::MAX; src.len()];
        let mut next = base;
        for (i, &kp) in keep.iter().enumerate() {
            if kp {
                node_map[i] = next;
                next += 1;
            }
        }
        for (i, mut n) in src.into_iter().enumerate() {
            if !keep[i] {
                continue;
            }
            if let Some(m) = n.get_mut("mesh") {
                add(m, mesh_base);
            }
            if let Some(Value::Array(ch)) = n.get_mut("children") {
                let kept: Vec<Value> = ch.iter().filter_map(idx).filter(|&c| keep.get(c).copied().unwrap_or(false)).map(|c| Value::from(node_map[c])).collect();
                *ch = kept;
            }
            if n.get("children").and_then(Value::as_array).is_some_and(Vec::is_empty)
                && let Some(o) = n.as_object_mut()
            {
                o.remove("children");
            }
            nodes.push(n);
        }
        let kids: Vec<Value> = roots.iter().filter(|&&r| keep.get(r).copied().unwrap_or(false)).map(|&r| Value::from(node_map[r])).collect();
        lod_roots.push(kids);

        for (key, out) in [("extensionsUsed", &mut used), ("extensionsRequired", &mut required)] {
            for e in list(j, key).iter().filter_map(Value::as_str) {
                if !out.iter().any(|x| x == e) {
                    out.push(e.to_owned());
                }
            }
        }
    }

    let mut root_children = Vec::new();
    for (k, kids) in lod_roots.into_iter().enumerate() {
        let mut n = Map::new();
        n.insert("name".into(), Value::from(lod_node_name(id, k)));
        if !kids.is_empty() {
            n.insert("children".into(), Value::Array(kids));
        }
        nodes.push(Value::Object(n));
        root_children.push(Value::from(nodes.len() - 1));
    }
    nodes.push(json!({ "name": id, "children": root_children }));
    let root = nodes.len() - 1;

    let mut out = Map::new();
    out.insert("asset".into(), first.json.get("asset").cloned().unwrap_or_else(|| json!({"version": "2.0"})));
    if !used.is_empty() {
        out.insert("extensionsUsed".into(), json!(used));
    }
    if !required.is_empty() {
        out.insert("extensionsRequired".into(), json!(required));
    }
    out.insert("scene".into(), Value::from(0));
    out.insert("scenes".into(), json!([{ "nodes": [root] }]));
    for (k, v) in [
        ("nodes", nodes),
        ("meshes", meshes),
        ("materials", materials),
        ("textures", textures),
        ("images", images),
        ("samplers", samplers),
        ("accessors", accessors),
        ("bufferViews", views),
    ] {
        if !v.is_empty() {
            out.insert(k.into(), Value::Array(v));
        }
    }
    if !bin.is_empty() {
        out.insert("buffers".into(), json!([{ "byteLength": bin.len() }]));
    }
    Ok(Doc { json: Value::Object(out), bin })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(tag: u8) -> Doc {
        Doc {
            json: json!({
                "asset": {"version": "2.0"},
                "scene": 0,
                "scenes": [{"nodes": [0]}],
                "nodes": [
                    {"name": "body", "mesh": 0, "children": [1, 2]},
                    {"name": "anchor:seat", "extras": {"evani": {"kind": "anchor"}}},
                    {"name": "collision", "children": [3]},
                    {"name": "col:body"}
                ],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
                "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}],
                "textures": [{"source": 0, "sampler": 0}],
                "images": [{"uri": "base.png"}],
                "samplers": [{}],
                "accessors": [{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3"}],
                "bufferViews": [{"buffer": 0, "byteLength": 12}],
                "buffers": [{"byteLength": 12}]
            }),
            bin: vec![tag; 12],
        }
    }

    #[test]
    fn levels_share_textures_and_only_level_0_keeps_anchors() {
        let m = merge("crate", &[level(1), level(2)]).unwrap();
        let j = &m.json;
        let names: Vec<&str> = j["nodes"].as_array().unwrap().iter().filter_map(|n| n["name"].as_str()).collect();
        assert_eq!(names, ["body", "anchor:seat", "collision", "col:body", "body", "crate_lod0", "crate_lod1", "crate"]);
        assert_eq!(j["images"].as_array().unwrap().len(), 1);
        assert_eq!(j["materials"].as_array().unwrap().len(), 1);
        assert_eq!(j["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(j["meshes"][1]["primitives"][0]["attributes"]["POSITION"], 1);
        assert_eq!(j["accessors"][1]["bufferView"], 1);
        assert_eq!(j["bufferViews"][1]["byteOffset"], 16);
        assert_eq!(m.bin[16], 2);
        assert_eq!(j["nodes"][4]["mesh"], 1);
        assert!(j["nodes"][4].get("children").is_none(), "anchor and collision unlinked on level 1");
        assert_eq!(j["scenes"][0]["nodes"][0], 7);
    }

    #[test]
    fn skinned_models_are_not_merged() {
        let mut d = level(1);
        d.json["skins"] = json!([{"joints": [0]}]);
        assert!(merge("x", &[d]).unwrap_err().contains("skinned"));
    }

    #[test]
    fn models_with_lights_fall_back_to_one_level() {
        let mut d = level(1);
        d.json["extensionsUsed"] = json!(["KHR_lights_punctual"]);
        d.json["extensions"] = json!({"KHR_lights_punctual": {"lights": [{"type": "point"}]}});
        assert!(mergeable(&d).is_err(), "file-level light list");
        d.json.as_object_mut().unwrap().remove("extensions");
        d.json["nodes"][0]["extensions"] = json!({"KHR_lights_punctual": {"light": 0}});
        assert!(mergeable(&d).unwrap_err().contains("node"), "a node pointing at a light");
        assert!(merge("x", &[d]).is_err());
        assert!(mergeable(&level(1)).is_ok());
    }
}
