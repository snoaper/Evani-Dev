//! Layered materials: a material's glTF `KHR_materials_sheen`,
//! `KHR_materials_clearcoat`, `KHR_materials_anisotropy` and
//! `KHR_materials_iridescence` factors become `extras.evani_layers` (crate
//! evani-material-layers, `Layers::to_extras`), which the clean and gltfpack
//! keep (`-ke`) and the engine reads at model load (evani-render feature
//! `material-layers`). The four extensions themselves are then removed:
//! the engine reads factors only, so their textures would be bytes nobody
//! draws (each one is named in the report). A file without any of them goes
//! through untouched.

use evani_material_layers::Layers;
use evani_material_layers::parse::{EXTRAS_KEY, KHR_NAMES};
use serde_json::{Value, json};

use crate::glb::Doc;

/// Some material carries one of the four extensions.
pub fn has(j: &Value) -> bool {
    j.get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|m| m.get("extensions").and_then(Value::as_object).is_some_and(|e| KHR_NAMES.iter().any(|n| e.contains_key(*n))))
}

/// Write each material's layer extras, drop the four extensions (from the
/// materials and the extension lists) and say what was done.
pub fn apply(doc: &mut Doc) -> Vec<String> {
    let mut notes = Vec::new();
    let mut count = 0;
    let mut dropped = Vec::new();
    if let Some(mats) = doc.json.get_mut("materials").and_then(Value::as_array_mut) {
        for (i, m) in mats.iter_mut().enumerate() {
            let Some(ext) = m.get("extensions").cloned() else { continue };
            let Some(layers) = Layers::from_khr(&ext) else { continue };
            let name = m.get("name").and_then(Value::as_str).map_or_else(|| format!("material {i}"), str::to_owned);
            dropped.extend(Layers::dropped_textures(&ext).into_iter().map(|t| format!("{name}: {t}")));
            if !m.get("extras").is_some_and(Value::is_object) {
                m["extras"] = json!({});
            }
            m["extras"][EXTRAS_KEY] = layers.to_extras();
            if let Some(e) = m.get_mut("extensions").and_then(Value::as_object_mut) {
                for n in KHR_NAMES {
                    e.remove(n);
                }
                if e.is_empty()
                    && let Some(o) = m.as_object_mut()
                {
                    o.remove("extensions");
                }
            }
            count += 1;
        }
    }
    for key in ["extensionsUsed", "extensionsRequired"] {
        if let Some(a) = doc.json.get_mut(key).and_then(Value::as_array_mut) {
            a.retain(|e| !e.as_str().is_some_and(|s| KHR_NAMES.contains(&s)));
            if a.is_empty()
                && let Some(o) = doc.json.as_object_mut()
            {
                o.remove(key);
            }
        }
    }
    if count > 0 {
        notes.push(format!("layers: {count} material(s) carry sheen, clear coat, anisotropy or iridescence as extras.evani_layers (drawn with the engine's `material-layers` render feature)"));
    }
    if !dropped.is_empty() {
        notes.push(format!("layers: factors only, these extension textures are left out: {}", dropped.join(", ")));
    }
    notes
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn doc(j: Value) -> Doc {
        Doc { json: j, bin: Vec::new() }
    }

    #[test]
    fn khr_factors_become_layer_extras() {
        let mut d = doc(json!({
            "asset": {"version": "2.0"},
            "extensionsUsed": ["KHR_materials_sheen", "KHR_materials_clearcoat", "KHR_texture_transform"],
            "materials": [
                {"name": "velvet", "extensions": {"KHR_materials_sheen": {"sheenColorFactor": [0.9, 0.3, 0.4], "sheenRoughnessFactor": 0.5, "sheenColorTexture": {"index": 0}}}, "extras": {"evani_cloth": {"weave": "plain"}}},
                {"name": "paint", "extensions": {"KHR_materials_clearcoat": {"clearcoatFactor": 1.0, "clearcoatRoughnessFactor": 0.04}, "KHR_texture_transform": {}}},
                {"name": "plain"}
            ]
        }));
        assert!(has(&d.json));
        let notes = apply(&mut d);
        let m = &d.json["materials"];
        assert_eq!(m[0]["extras"]["evani_layers"]["sheen"]["roughness"], json!(0.5));
        assert_eq!(m[0]["extras"]["evani_cloth"]["weave"], json!("plain"), "other extras kept");
        assert!(m[0].get("extensions").is_none());
        assert_eq!(m[1]["extras"]["evani_layers"]["clearcoat"]["factor"], json!(1.0));
        assert!(m[1]["extensions"].get("KHR_texture_transform").is_some(), "other extensions kept");
        assert!(m[2].get("extras").is_none(), "a plain material is untouched");
        assert_eq!(d.json["extensionsUsed"], json!(["KHR_texture_transform"]));
        assert!(!has(&d.json));
        assert!(notes.iter().any(|n| n.contains("2 material(s)")));
        assert!(notes.iter().any(|n| n.contains("velvet: KHR_materials_sheen.sheenColorTexture")));
        // The engine reads back what was written.
        let l = Layers::from_extras(&m[0]["extras"]).unwrap();
        assert_eq!(l.sheen_color, [0.9, 0.3, 0.4]);
    }

    #[test]
    fn files_without_layers_are_untouched() {
        let j = json!({"asset": {"version": "2.0"}, "materials": [{"name": "a", "extensions": {"KHR_materials_emissive_strength": {}}}]});
        let mut d = doc(j.clone());
        assert!(!has(&d.json));
        assert!(apply(&mut d).is_empty());
        assert_eq!(d.json, j);
    }
}
