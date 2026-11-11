//! Stale construct outputs (docs/MODELGEN_PLAN.md 3.8): a kit's model
//! recipe (`kits/evani-kit-<id>/models/<model>.construct.json`) whose files
//! were built from an older version of it, or never built.
//!
//! `cargo xtask model <id>` writes the build report `<model>.evmodel.json`
//! beside each recipe, carrying the recipe's content hash (FNV-1a of its
//! canonical JSON, `evani_construct::recipe::content_hash`: key order and
//! whitespace don't count). `check` compares the two, the same way it
//! catches a stale `stream.json`.

use std::path::Path;

/// One stale or unbuilt recipe: `(what, fix)`.
pub struct Problem {
    pub what: String,
    pub fix: String,
}

/// Every construct recipe under `kits/*/models/` whose report is missing or
/// names another recipe hash.
pub fn problems(root: &Path) -> Vec<Problem> {
    let mut out = Vec::new();
    let Ok(kits) = std::fs::read_dir(root.join("kits")) else { return out };
    let mut kits: Vec<_> = kits.flatten().map(|e| e.path()).filter(|p| p.join("models").is_dir()).collect();
    kits.sort();
    for kit in kits {
        let kit_name = kit.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let id = kit_name.strip_prefix("evani-kit-").unwrap_or(&kit_name).to_string();
        let Ok(files) = std::fs::read_dir(kit.join("models")) else { continue };
        let mut recipes: Vec<_> = files.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".construct.json")).collect();
        recipes.sort();
        for r in recipes {
            let file = r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let model = file.trim_end_matches(".construct.json").to_string();
            let rel = format!("kits/{kit_name}/models/{file}");
            let fix = format!("Run `cargo xtask model {id} --model {model}` (it rebuilds the tier files, converts them and rewrites kits/{kit_name}/models/{model}.evmodel.json).");
            let json: serde_json::Value = match std::fs::read_to_string(&r).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
                Ok(j) => j,
                Err(e) => {
                    out.push(Problem { what: format!("`{rel}` can't be read as JSON: {e}"), fix: "Fix the recipe's JSON (docs/MODELGEN_PLAN.md 3.4).".into() });
                    continue;
                }
            };
            let hash = evani_construct::recipe::content_hash(&json);
            let report = kit.join("models").join(format!("{model}.evmodel.json"));
            let built = std::fs::read_to_string(&report).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            match built.as_ref().and_then(|b| b.get("recipe_hash")).and_then(|h| h.as_str()) {
                None => out.push(Problem { what: format!("`{rel}` has never been built (no {model}.evmodel.json beside it), so the game has no files for it"), fix }),
                Some(h) if h != hash => out.push(Problem { what: format!("`{rel}` changed since its model files were built (recipe {hash}, files from {h}), so the game ships the old model"), fix }),
                Some(_) => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_unbuilt_and_stale_recipes_but_not_reformatted_ones() {
        let root = std::env::temp_dir().join(format!("evani-construct-stale-{}", std::process::id()));
        let models = root.join("kits").join("evani-kit-demo").join("models");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&models).unwrap();
        let recipe = r#"{ "construct": 1, "id": "box", "parts": [] }"#;
        std::fs::write(models.join("box.construct.json"), recipe).unwrap();
        let p = problems(&root);
        assert!(p.len() == 1 && p[0].what.contains("never been built"), "{:?}", p.iter().map(|x| &x.what).collect::<Vec<_>>());
        assert!(p[0].fix.contains("cargo xtask model demo --model box"));
        let hash = evani_construct::recipe::content_hash(&serde_json::from_str(recipe).unwrap());
        std::fs::write(models.join("box.evmodel.json"), format!(r#"{{ "recipe_hash": "{hash}" }}"#)).unwrap();
        assert!(problems(&root).is_empty());
        // Whitespace and key order don't make it stale; a real change does.
        std::fs::write(models.join("box.construct.json"), r#"{"parts":[],"id":"box",   "construct":1}"#).unwrap();
        assert!(problems(&root).is_empty());
        std::fs::write(models.join("box.construct.json"), r#"{ "construct": 1, "id": "box", "seed": 2, "parts": [] }"#).unwrap();
        let p = problems(&root);
        assert!(p.len() == 1 && p[0].what.contains("changed since"), "{:?}", p.iter().map(|x| &x.what).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&root);
    }
}
