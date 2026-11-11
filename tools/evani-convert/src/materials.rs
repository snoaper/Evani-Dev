//! Catalogue materials (`cargo xtask convert --materials <map.json>`, batch 1
//! build plan 2.2, package P10b): fill a model's glTF materials from the
//! material catalogue so the model carries its numbers.
//!
//! The map is a JSON object from a material's **name in the model** to a
//! **catalogue id**:
//!
//! ```json
//! { "plank": "wood.planks.oak", "iron_band": "metal.iron.wrought" }
//! ```
//!
//! For each named material this writes (evani-material-recipes `gltf`, the
//! P10a adapter): `extras.evani_material` (the id), `extras.evani_recipe`
//! (the 32-byte block as 64 hex characters, for recipe-backed rows), the plain
//! PBR factors, and the family's own fields (cloth, skin, hair, eye, water,
//! glass, foliage, thin). A material that already has `extras.evani_material`
//! (set in Blender) is filled the same way with no map entry. A file with
//! neither goes through untouched.
//!
//! Dev tool only: the game gets the written numbers, never the catalogue.

use std::collections::BTreeMap;
use std::path::Path;

use evani_material_recipes::catalog::Catalog;
use evani_material_recipes::gltf::{self, MATERIAL_KEY};
use serde_json::{Value, json};

use crate::glb::Doc;

/// The map file: material name -> catalogue id.
pub fn load_map(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("--materials {}: {e}", path.display()))?;
    parse_map(&text).map_err(|e| format!("--materials {}: {e}", path.display()))
}

/// [`load_map`] from text.
pub fn parse_map(text: &str) -> Result<BTreeMap<String, String>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let o = v.as_object().ok_or("the map must be a JSON object: { \"<material name in the model>\": \"<catalogue id>\" }")?;
    o.iter().map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())).ok_or_else(|| format!("\"{k}\": the catalogue id must be a string"))).collect()
}

/// The catalogue from the engine checkout.
pub fn load_catalog(root: &Path) -> Result<Catalog, String> {
    Catalog::load_dir(&root.join("catalog").join("materials")).map_err(|e| format!("catalog/materials: {e}"))
}

/// Some material names a catalogue row already.
pub fn has(j: &Value) -> bool {
    j.get("materials").and_then(Value::as_array).into_iter().flatten().any(|m| m.pointer(&format!("/extras/{MATERIAL_KEY}")).is_some())
}

/// The ids closest to `id` (shared prefix, then shared words), for the
/// "close ones" list.
fn suggestions(cat: &Catalog, id: &str) -> Vec<String> {
    let words: Vec<&str> = id.split(['.', '_', '-']).filter(|w| w.len() > 1).collect();
    let mut scored: Vec<(usize, &str)> = cat
        .rows
        .iter()
        .map(|r| {
            let common = r.id.chars().zip(id.chars()).take_while(|(a, b)| a == b).count();
            let hits = words.iter().filter(|w| r.id.contains(**w)).count();
            (common + hits * 4, r.id.as_str())
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    scored.into_iter().take(5).map(|(_, i)| i.to_string()).collect()
}

/// Fill the document's materials from the catalogue: the map's entries
/// first (they set `extras.evani_material`), then every material that names a
/// row. Returns the report notes. Errors name the material or id and what is
/// valid; nothing is changed when one occurs.
pub fn apply(doc: &mut Doc, map: &BTreeMap<String, String>, cat: &Catalog) -> Result<Vec<String>, String> {
    let names: Vec<String> = doc
        .json
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, m)| m.get("name").and_then(Value::as_str).map_or_else(|| format!("material {i}"), str::to_string))
        .collect();
    let mut work = doc.json.clone();
    for (name, id) in map {
        if cat.row(id).is_none() {
            let near = suggestions(cat, id);
            return Err(format!(
                "--materials: \"{name}\" maps to \"{id}\", which is not a catalogue id{}\nfix: use an id from catalog/materials/materials.json (the viewer's Copy id gives one)",
                if near.is_empty() { String::new() } else { format!("; close ones: {}", near.join(", ")) }
            ));
        }
        let mut found = false;
        if let Some(mats) = work.get_mut("materials").and_then(Value::as_array_mut) {
            for (i, m) in mats.iter_mut().enumerate() {
                if names[i] == *name {
                    found = true;
                    if !m.is_object() {
                        *m = json!({});
                    }
                    if !m.get("extras").is_some_and(Value::is_object) {
                        m["extras"] = json!({});
                    }
                    m["extras"][MATERIAL_KEY] = json!(id);
                }
            }
        }
        if !found {
            return Err(format!(
                "--materials: the model has no material named \"{name}\" (it has: {})\nfix: name a material that is in the model, or rename it in the model",
                if names.is_empty() { "none".to_string() } else { names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ") }
            ));
        }
    }
    let done = gltf::apply_catalog(&mut work, cat).map_err(|e| format!("--materials: {e}"))?;
    doc.json = work;
    let mut notes = Vec::new();
    if !done.is_empty() {
        let planned: Vec<&str> = done.iter().filter_map(|(_, id)| cat.row(id)).filter(|r| r.status == "planned").map(|r| r.id.as_str()).collect();
        notes.push(format!(
            "materials: {} material(s) filled from the catalogue ({}); they carry extras.evani_material and, for recipe rows, extras.evani_recipe. List the recipes under `[materials]` in evani.toml",
            done.len(),
            done.iter().map(|(i, id)| format!("{} = {id}", names.get(*i).map_or("?", String::as_str))).collect::<Vec<_>>().join(", ")
        ));
        if !planned.is_empty() {
            notes.push(format!("materials: {} still have no numbers of their own in the catalogue (status planned) and draw with their recipe's defaults: {}", planned.len(), planned.join(", ")));
        }
    }
    Ok(notes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        load_catalog(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
    }

    fn doc(j: Value) -> Doc {
        Doc { json: j, bin: Vec::new() }
    }

    fn row_of(cat: &Catalog, recipe: &str) -> String {
        cat.rows.iter().find(|r| r.recipe == recipe).unwrap().id.clone()
    }

    #[test]
    fn a_map_fills_the_named_materials() {
        let cat = cat();
        let (wood, metal) = (row_of(&cat, "wood"), row_of(&cat, "metal"));
        let mut d = doc(json!({"asset": {"version": "2.0"}, "materials": [{"name": "plank"}, {"name": "band", "extras": {"keep": 1}}, {"name": "plain"}]}));
        let map = parse_map(&format!("{{\"plank\": \"{wood}\", \"band\": \"{metal}\"}}")).unwrap();
        let notes = apply(&mut d, &map, &cat).unwrap();
        let m = &d.json["materials"];
        assert_eq!(m[0]["extras"]["evani_material"], json!(wood));
        assert_eq!(m[0]["extras"]["evani_recipe"].as_str().unwrap().len(), 64);
        assert_eq!(m[1]["extras"]["evani_material"], json!(metal));
        assert_eq!(m[1]["extras"]["keep"], json!(1), "other extras kept");
        assert!(m[2].get("extras").is_none(), "an unnamed material is untouched");
        assert!(notes[0].contains("2 material(s)") && notes[0].contains(&format!("plank = {wood}")), "{notes:?}");
        // The block is the catalogue row's.
        let want = cat.encode(cat.row(&wood).unwrap()).unwrap().to_hex();
        assert_eq!(m[0]["extras"]["evani_recipe"], json!(want));
    }

    #[test]
    fn a_material_that_names_a_row_needs_no_map() {
        let cat = cat();
        let wood = row_of(&cat, "wood");
        let mut d = doc(json!({"materials": [{"name": "a", "extras": {"evani_material": wood}}]}));
        assert!(has(&d.json));
        apply(&mut d, &BTreeMap::new(), &cat).unwrap();
        assert!(d.json["materials"][0]["extras"]["evani_recipe"].is_string());
        assert!(!has(&json!({"materials": [{"name": "a"}]})));
    }

    /// Each family's data lands where its renderer reads it. Cloth (since
    /// B4-P3b, merged in wave 1) carries its sheen in the recipe block: the
    /// recipe stage draws the weave and the sheen after lighting, so the block
    /// must hold the sheen switch and the row's sheen numbers, and no
    /// `KHR_materials_sheen`, `evani_layers` or `evani_cloth` may be written
    /// (a build with the layers family would draw the sheen twice). Glass, a
    /// nature row, still carries its kind and transmission for the nature
    /// pass.
    #[test]
    fn family_rows_carry_their_family_data() {
        let cat = cat();
        let cloth = cat
            .rows
            .iter()
            .filter(|r| r.recipe == "cloth" && r.status != "planned")
            .find(|r| gltf::row_values(&cat, r).unwrap().number("sheen").is_some_and(|s| s > 0.0))
            .expect("a drawn cloth row with a sheen")
            .id
            .clone();
        let glass = row_of(&cat, "glass");
        let mut d = doc(json!({"materials": [{"name": "cape"}, {"name": "pane"}]}));
        apply(&mut d, &parse_map(&format!("{{\"cape\": \"{cloth}\", \"pane\": \"{glass}\"}}")).unwrap(), &cat).unwrap();
        let m = &d.json["materials"][0];
        let hex = m["extras"]["evani_recipe"].as_str().unwrap_or_else(|| panic!("cloth's weave and sheen are the recipe stage's: {m}"));
        let block = evani_material_recipes::block::Block::from_hex(hex).unwrap();
        let row = cat.row(&cloth).unwrap();
        assert_eq!(block, cat.encode(row).unwrap(), "the block is the row's");
        let v = gltf::row_values(&cat, row).unwrap();
        let f = block.unpack();
        assert_eq!(f.recipe, v.recipe.code, "{cloth}: a cloth block");
        let sheen = v.recipe.extras.iter().find(|x| x.name == "sheen").expect("cloth has a sheen switch");
        assert_ne!(f.extras & sheen.mask, 0, "{cloth}: the sheen switch is on in the block");
        let at = v.recipe.numbers.iter().position(|n| n.name == "sheen").expect("cloth has a sheen number");
        assert!(f.numbers[at] > 0 && v.num("sheen", 0.0) > 0.0, "{cloth}: the block carries the row's sheen ({})", v.num("sheen", 0.0));
        for gone in ["evani_layers", "evani_cloth"] {
            assert!(m["extras"].get(gone).is_none(), "{cloth}: no {gone} (the sheen would draw twice): {m}");
        }
        assert!(m.pointer("/extensions/KHR_materials_sheen").is_none(), "{cloth}: no KHR_materials_sheen (the sheen would draw twice): {m}");
        let p = &d.json["materials"][1];
        assert!(p["extras"]["evani_kind"].is_string() && p["extensions"]["KHR_materials_transmission"].is_object(), "{glass}: the nature pass's kind and transmission: {p}");
    }

    #[test]
    fn bad_maps_say_what_is_wrong() {
        let cat = cat();
        let wood = row_of(&cat, "wood");
        let mut d = doc(json!({"materials": [{"name": "plank"}]}));
        let before = d.json.clone();
        // A material the model does not have: its names are listed.
        let e = apply(&mut d, &parse_map(&format!("{{\"planck\": \"{wood}\"}}")).unwrap(), &cat).unwrap_err();
        assert!(e.contains("\"planck\"") && e.contains("\"plank\"") && e.contains("fix:"), "{e}");
        // An id the catalogue does not have: close ones are suggested.
        let near = wood.split('.').take(2).collect::<Vec<_>>().join(".");
        let e = apply(&mut d, &parse_map(&format!("{{\"plank\": \"{near}.nope\"}}")).unwrap(), &cat).unwrap_err();
        assert!(e.contains("not a catalogue id") && e.contains("close ones:") && e.contains("fix:"), "{e}");
        assert_eq!(d.json, before, "nothing changes on an error");
        // A map that is not an object of strings.
        assert!(parse_map("[1]").is_err());
        assert!(parse_map("{\"a\": 1}").is_err_and(|e| e.contains("must be a string")));
        assert!(parse_map("nope").is_err());
    }

    #[test]
    fn converting_twice_is_the_same_file() {
        let cat = cat();
        let wood = row_of(&cat, "wood");
        let mut d = doc(json!({"materials": [{"name": "plank"}]}));
        let map = parse_map(&format!("{{\"plank\": \"{wood}\"}}")).unwrap();
        apply(&mut d, &map, &cat).unwrap();
        let once = d.json.clone();
        apply(&mut d, &map, &cat).unwrap();
        assert_eq!(d.json, once);
    }
}
