//! The material library in every game (Chris, 2026-11-03: "putting the
//! material library in everything").
//!
//! Every game in `apps/` and every example game in `examples/*/apps/` carries
//! the `[materials]` feature lines (the sandbox's), so it can list any
//! Evani.Materials recipe, part, switch or condition in its evani.toml and the
//! build draws it. Each line is off unless evani.toml lists it, so a game that
//! lists none downloads none of it (the byte proof in DECISIONS.md).
//!
//! The second test resolves catalogue materials by name for every game, and so
//! for every kit and look a game is built from (each of them is checked to be
//! covered by at least one game): the row is found by its id, encoded into the
//! 32-byte block the converter writes into a model, and the features that row
//! needs are wired in that game's app crate.

use evani_guide::materials::{self, Listed};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every game folder: `apps/<id>` and `examples/<name>/apps/<id>` with an evani.toml.
fn games(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut scan = |dir: PathBuf| {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("evani.toml").is_file()).collect();
            v.sort();
            out.extend(v);
        }
    };
    scan(root.join("apps"));
    if let Ok(rd) = std::fs::read_dir(root.join("examples")) {
        let mut ex: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("apps")).collect();
        ex.sort();
        for d in ex {
            scan(d);
        }
    }
    out
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Every name `[materials]` accepts: the 29 recipes, the parts, the cut-out,
/// blend and refraction switches, and the eight condition layers.
fn everything() -> Listed {
    let mut recipes: Vec<String> = materials::RECIPES.iter().map(|(n, _)| n.to_string()).collect();
    recipes.extend(materials::PARTS.iter().map(|(n, _, _)| n.to_string()));
    recipes.extend(materials::SWITCHES.iter().map(|s| s.to_string()));
    Listed { recipes, conditions: materials::CONDITIONS.iter().map(|c| c.to_string()).collect() }
}

/// `kit = "..."` / `look = "..."` of an evani.toml.
fn field(toml_text: &str, key: &str) -> String {
    let t: toml::Table = toml_text.parse().unwrap_or_else(|e| panic!("evani.toml: {e}"));
    t.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

#[test]
fn every_game_and_example_can_list_every_library_material() {
    let root = root();
    let all = everything();
    let found = games(&root);
    assert!(found.len() >= 10, "expected the nine engine games and the example, found {found:?}");
    let mut failures = Vec::new();
    for dir in &found {
        let cargo = read(&dir.join("Cargo.toml"));
        for (_sev, what, fix) in materials::cargo_problems(&name(dir), &all, &cargo) {
            failures.push(format!("{}: {what}. {fix}", dir.display()));
        }
        let t: toml::Table = cargo.parse().unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let deps = t.get("dependencies").and_then(|d| d.as_table());
        for krate in ["evani-material-recipes", "evani-material-nature"] {
            let optional = deps.and_then(|d| d.get(krate)).and_then(|v| v.as_table()).and_then(|v| v.get("optional")).and_then(|o| o.as_bool()) == Some(true);
            if !optional {
                failures.push(format!("{}: `{krate}` must be an optional dependency (the features turn it on)", dir.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_kit_and_look_resolves_library_materials_by_name() {
    let root = root();
    let cat = materials::load_catalog(&root).expect("catalog/materials loads");
    let mut kits = std::collections::BTreeSet::new();
    let mut looks = std::collections::BTreeSet::new();
    for dir in games(&root) {
        let manifest = read(&dir.join("evani.toml"));
        let cargo = read(&dir.join("Cargo.toml"));
        let (kit, look) = (field(&manifest, "kit"), field(&manifest, "look"));
        let mut resolved = 0;
        for (recipe, _) in materials::RECIPES {
            // One catalogue row per recipe, looked up by its name (id).
            let Some(id) = cat.rows.iter().find(|r| r.recipe == recipe && r.status == "ready").map(|r| r.id.clone()) else { continue };
            let row = cat.row(&id).unwrap_or_else(|| panic!("{}: `{id}` does not resolve by name", dir.display()));
            let block = cat.encode(row).unwrap_or_else(|e| panic!("{}: `{id}` does not encode: {e:?}", dir.display()));
            assert_eq!(block.to_hex().len(), 64, "{id}: a 32-byte block");
            let listed = Listed { recipes: vec![row.recipe.clone()], conditions: Vec::new() };
            let problems = materials::cargo_problems(&name(&dir), &listed, &cargo);
            assert!(problems.is_empty(), "{} ({kit} kit, {look} look): `{id}` resolves, but the game can't draw it: {problems:?}", dir.display());
            resolved += 1;
        }
        assert!(resolved >= 25, "{}: only {resolved} recipes have a ready row", dir.display());
        println!("{}: {kit} kit, {look} look: {resolved} library materials resolved by name, each drawable", dir.display());
        kits.insert(kit);
        looks.insert(look);
    }
    // Every kit and look crate in the workspace is reached through a game.
    for (folder, prefix, seen) in [("kits", "evani-kit-", &kits), ("looks", "evani-look-", &looks)] {
        for e in std::fs::read_dir(root.join(folder)).expect("folder").flatten() {
            let n = name(&e.path());
            if let Some(id) = n.strip_prefix(prefix)
                && e.path().join("Cargo.toml").is_file()
            {
                assert!(seen.contains(id), "{folder}/{n} is in no game, so nothing shows it can use the material library");
            }
        }
    }
}
