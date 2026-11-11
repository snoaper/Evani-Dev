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

/// A paint recipe material (`extras.evani_recipe` code 2): the recipe stage
/// draws its clear coat from the block, so an authored clear coat must not
/// also become layered data (a game listing `cloth` would draw two).
fn is_paint_block(m: &Value) -> bool {
    m.pointer("/extras/evani_recipe")
        .and_then(Value::as_str)
        .and_then(|h| h.get(..2))
        .and_then(|h| u8::from_str_radix(h, 16).ok())
        .and_then(evani_guide::materials::recipe_of_code)
        == Some("paint")
}

/// Write each material's layer extras, drop the four extensions (from the
/// materials and the extension lists) and say what was done.
pub fn apply(doc: &mut Doc) -> Vec<String> {
    let mut notes = Vec::new();
    let mut count = 0;
    let mut dropped = Vec::new();
    let mut paint_coats = 0;
    if let Some(mats) = doc.json.get_mut("materials").and_then(Value::as_array_mut) {
        for (i, m) in mats.iter_mut().enumerate() {
            if is_paint_block(m)
                && let Some(e) = m.get_mut("extensions").and_then(Value::as_object_mut)
                && e.remove(evani_material_layers::parse::KHR_CLEARCOAT).is_some()
            {
                paint_coats += 1;
                if e.is_empty()
                    && let Some(o) = m.as_object_mut()
                {
                    o.remove("extensions");
                }
            }
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
    if paint_coats > 0 {
        notes.push(format!("layers: {paint_coats} paint material(s) keep their clear coat in the material block (extras.evani_recipe), so the authored KHR_materials_clearcoat is left out (the recipe stage draws that coat)"));
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
    fn a_paint_block_keeps_its_clear_coat_out_of_the_layers() {
        // Paint's coat is drawn from the block (P10a): an authored clear coat
        // on it must not become layered data (a second coat in a `cloth` game).
        let paint_block = format!("02{}", "00".repeat(31));
        let mut d = doc(json!({
            "asset": {"version": "2.0"},
            "extensionsUsed": ["KHR_materials_clearcoat", "KHR_materials_sheen"],
            "materials": [
                {"name": "car", "extras": {"evani_recipe": paint_block}, "extensions": {"KHR_materials_clearcoat": {"clearcoatFactor": 1.0}}},
                {"name": "plain_car", "extensions": {"KHR_materials_clearcoat": {"clearcoatFactor": 1.0}}},
                {"name": "cape", "extensions": {"KHR_materials_sheen": {"sheenColorFactor": [1.0, 0.0, 0.0]}}}
            ]
        }));
        let notes = apply(&mut d);
        let m = &d.json["materials"];
        assert!(m[0]["extras"].get("evani_layers").is_none(), "no layered coat on a paint block: {}", m[0]);
        assert!(m[0].get("extensions").is_none(), "the authored coat extension is gone");
        assert_eq!(m[0]["extras"]["evani_recipe"], json!(paint_block), "the block is untouched");
        assert!(m[1]["extras"]["evani_layers"]["clearcoat"].is_object(), "a material without a paint block still gets its coat");
        assert!(m[2]["extras"]["evani_layers"]["sheen"].is_object());
        assert!(notes.iter().any(|n| n.contains("1 paint material(s)")), "{notes:?}");
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
