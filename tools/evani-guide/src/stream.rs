//! The asset manifest v2 for streamed models (`models`, S3-4):
//! `apps/<id>/assets/models/stream.json`, written by
//! `cargo xtask manifest <id>` and checked by `cargo xtask check`.
//!
//! One entry per file in `assets/models/`:
//!
//! * `tier`: the `.<tier>.` segment of the file name (`hero.low.glb`,
//!   `hero.low.base.jpg`), else none (every tier may fetch it). A tier
//!   downloads only its own and the shared files.
//! * `kind`: `model` (`.glb`, `.gltf`), `texture` (images and KTX2) or
//!   `data`. The background plan prefetches models and data; textures are
//!   fetched by the model that samples them, in the device's format.
//! * `priority`: models above textures above data.
//! * `group`: `boot` for the files evani.toml lists under
//!   `[stream] boot = [...]` (the title screen waits for those only), else
//!   `later`.
//!   With `[stream.levels]` (`forest = ["tree", "rock"]`), the files of a
//!   level are `level:<name>` instead (S7-7): a name matches a file when it
//!   is the file's name or the model id before its first dot, so `tree` takes
//!   `tree.glb`, `tree.low.glb` and `tree.low.base.bc7.ktx2`. A file that
//!   `[stream] boot` lists stays in `boot`. Level groups are never fetched
//!   in the background: the game asks with `Models::prefetch` (feature
//!   `level-groups`).
//! * `hash` and `size`: the FNV-1a 64 content hash the engine checks every
//!   download against (and keys Cache Storage by).

use std::path::Path;

use serde_json::{Value, json};

/// Where the manifest is, under the game's folder.
pub const PATH: &str = "assets/models/stream.json";
/// Tier ids a file name may carry (catalog/tiers.json).
pub const TIERS: [&str; 6] = ["lite", "low", "medium", "high", "ultra", "vr"];

/// The tier a file or URL belongs to: its `.<tier>.` segment.
pub fn tier_of(name: &str) -> Option<&'static str> {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let file = file.split(['?', '#']).next().unwrap_or(file);
    let segs: Vec<&str> = file.split('.').collect();
    // Not the first segment (the model's id) and not the extension.
    let inner = segs.get(1..segs.len().saturating_sub(1)).unwrap_or(&[]);
    TIERS.into_iter().find(|t| inner.contains(t))
}

/// 64-bit FNV-1a as 16 hex digits (the engine's `content_hash_hex`).
pub fn hash_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn kind_of(name: &str) -> (&'static str, i64) {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "glb" | "gltf" => ("model", 10),
        "png" | "jpg" | "jpeg" | "webp" | "ktx2" => ("texture", 5),
        _ => ("data", 0),
    }
}

/// The `[stream] boot` list of a game's evani.toml.
pub fn boot_list(evani_toml: &str) -> Vec<String> {
    let Ok(t) = evani_toml.parse::<toml::Table>() else { return Vec::new() };
    t.get("stream")
        .and_then(|s| s.get("boot"))
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The `[stream.levels]` table of a game's evani.toml: level name, then the
/// model ids or file names that belong to it.
pub fn levels_list(evani_toml: &str) -> Vec<(String, Vec<String>)> {
    let Ok(t) = evani_toml.parse::<toml::Table>() else { return Vec::new() };
    let Some(l) = t.get("stream").and_then(|s| s.get("levels")).and_then(toml::Value::as_table) else { return Vec::new() };
    l.iter().map(|(k, v)| (k.clone(), v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default())).collect()
}

/// Does `pattern` (a model id or a file name) take the file `rel`?
fn takes(pattern: &str, rel: &str) -> bool {
    rel == pattern || rel.strip_prefix(pattern).is_some_and(|r| r.starts_with('.'))
}

/// The manifest for the files under `dir` (the game's `assets/models`),
/// as the JSON text `cargo xtask manifest` writes. `None` when the folder
/// has no files.
pub fn build(dir: &Path, boot: &[String]) -> Result<Option<String>, String> {
    build_levels(dir, boot, &[])
}

/// [`build`] with the game's levels ([`levels_list`]).
pub fn build_levels(dir: &Path, boot: &[String], levels: &[(String, Vec<String>)]) -> Result<Option<String>, String> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            if rel == "stream.json" || rel.starts_with('.') || rel.ends_with(".evmodel.json") {
                continue;
            }
            files.push((rel, p));
        }
    }
    if files.is_empty() {
        return Ok(None);
    }
    files.sort();
    let mut assets = Vec::new();
    let mut used = vec![false; levels.iter().map(|l| l.1.len()).sum()];
    for (rel, p) in files {
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let (kind, priority) = kind_of(&rel);
        let mut hits = Vec::new();
        let mut at = 0;
        for (name, pats) in levels {
            for pat in pats {
                if takes(pat, &rel) {
                    used[at] = true;
                    if !hits.contains(&name) {
                        hits.push(name);
                    }
                }
                at += 1;
            }
        }
        if hits.len() > 1 {
            return Err(format!("stream.levels: {rel} belongs to the levels {} and {}; a file can be in one level only (move it to `[stream] boot` or name it in one level)", hits[0], hits[1]));
        }
        let group = if boot.iter().any(|b| b == &rel) {
            "boot".to_string()
        } else if let Some(l) = hits.first() {
            format!("level:{l}")
        } else {
            "later".to_string()
        };
        let mut e = json!({
            "name": rel, "hash": hash_hex(&bytes), "size": bytes.len(), "url": rel,
            "kind": kind, "priority": priority, "group": group,
        });
        if let (Some(t), Some(o)) = (tier_of(&rel), e.as_object_mut()) {
            o.insert("tier".into(), Value::from(t));
        }
        assets.push(e);
    }
    let mut at = 0;
    for (name, pats) in levels {
        for pat in pats {
            if !used[at] {
                return Err(format!("stream.levels: level {name} lists `{pat}`, which matches no file in assets/models"));
            }
            at += 1;
        }
    }
    let text = serde_json::to_string_pretty(&json!({ "version": 2, "assets": assets })).map_err(|e| e.to_string())?;
    Ok(Some(text + "\n"))
}

/// A problem with a game's manifest: `(what, fix)`.
pub struct Problem {
    pub what: String,
    pub fix: String,
}

/// `check`'s rule for a game with `models`: a manifest that no longer
/// matches the files (a file changed, was added or removed), or tier files
/// (`<id>.<tier>.glb`) with no manifest (every tier would then download
/// the shared file and none of the tier files).
pub fn problems(root: &Path, app: &str) -> Vec<Problem> {
    problems_in(&root.join("apps").join(app), &format!("apps/{app}"), app)
}

/// [`problems`] for the game in `game` (a game outside the engine too):
/// `shown` names its folder in messages, `arg` the game in commands (`<id>`
/// or `--app-dir <folder>`).
pub fn problems_in(game: &Path, shown: &str, arg: &str) -> Vec<Problem> {
    let dir = game.join("assets/models");
    if !dir.is_dir() {
        return Vec::new();
    }
    let toml = std::fs::read_to_string(game.join("evani.toml")).unwrap_or_default();
    let fix = format!("Run `cargo xtask manifest {arg}` (it rewrites {shown}/{PATH} from the files and evani.toml's [stream] boot list).");
    let want = match build_levels(&dir, &boot_list(&toml), &levels_list(&toml)) {
        Ok(w) => w,
        Err(e) if e.starts_with("stream.levels") => return vec![Problem { what: format!("{shown}/evani.toml: {e}"), fix: "Fix `[stream.levels]` in evani.toml (each name must match a model id or file in assets/models, and no file may be in two levels), then run `cargo xtask manifest`.".to_string() }],
        Err(e) => return vec![Problem { what: format!("the models folder can't be read: {e}"), fix }],
    };
    let have = std::fs::read_to_string(dir.join("stream.json")).ok();
    match (have, want) {
        (None, Some(w)) => {
            let tiered = w.contains("\"tier\"") && w.contains("\"kind\": \"model\"");
            if tiered {
                vec![Problem { what: format!("{shown}/assets/models has tier files (`<id>.<tier>.glb`) but no {PATH}, so every tier downloads the shared files and none of the tier files"), fix }]
            } else {
                Vec::new()
            }
        }
        (Some(h), Some(w)) if h.replace("\r\n", "\n") != w => {
            vec![Problem { what: format!("{shown}/{PATH} no longer matches the files in assets/models (a file was changed, added or removed), so a download would fail its hash check or a file would be missed"), fix }]
        }
        (Some(_), None) => vec![Problem { what: format!("{shown}/{PATH} lists files but assets/models has none"), fix }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_come_from_the_file_name() {
        assert_eq!(tier_of("hero.low.glb"), Some("low"));
        assert_eq!(tier_of("models/hero.ultra.base.bc7.ktx2"), Some("ultra"));
        assert_eq!(tier_of("hero.glb"), None);
        assert_eq!(tier_of("low.glb"), None, "the model's id is not a tier segment");
        assert_eq!(tier_of("hero.base.jpg?v=low"), None);
        assert_eq!(tier_of("http://h:1/models/a.medium.glb"), Some("medium"));
    }

    #[test]
    fn hash_is_the_engines() {
        assert_eq!(hash_hex(b""), "cbf29ce484222325");
        assert_eq!(hash_hex(b"foobar"), "85944171f73967e8");
    }

    #[test]
    fn levels_group_their_files() {
        let root = crate::testutil::temp_root("stream-levels");
        let dir = root.join("apps/demo/assets/models");
        std::fs::create_dir_all(&dir).unwrap();
        let toml = "name = \"demo\"
[stream]
boot = [\"logo.png\"]
[stream.levels]
forest = [\"tree\", \"logo.png\"]
cave = [\"rock\"]
";
        std::fs::write(root.join("apps/demo/evani.toml"), toml).unwrap();
        for f in ["hub.glb", "logo.png", "tree.glb", "tree.low.glb", "tree.low.base.bc7.ktx2", "treehouse.glb", "rock.glb"] {
            std::fs::write(dir.join(f), f).unwrap();
        }
        let text = build_levels(&dir, &boot_list(toml), &levels_list(toml)).unwrap().unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let group = |n: &str| v["assets"].as_array().unwrap().iter().find(|e| e["name"] == n).unwrap()["group"].as_str().unwrap().to_string();
        assert_eq!(group("hub.glb"), "later");
        assert_eq!(group("logo.png"), "boot", "boot wins");
        assert_eq!(group("tree.glb"), "level:forest");
        assert_eq!(group("tree.low.base.bc7.ktx2"), "level:forest");
        assert_eq!(group("treehouse.glb"), "later", "a name takes the id before the first dot, not a prefix");
        assert_eq!(group("rock.glb"), "level:cave");
        // The same file in two levels, and a name that matches nothing, are errors.
        let two = vec![("a".to_string(), vec!["tree".to_string()]), ("b".to_string(), vec!["tree.glb".to_string()])];
        assert!(build_levels(&dir, &[], &two).unwrap_err().contains("belongs to the levels a and b"));
        let none = vec![("a".to_string(), vec!["bush".to_string()])];
        assert!(build_levels(&dir, &[], &none).unwrap_err().contains("matches no file"));
        // A game without levels is what it was.
        assert_eq!(build(&dir, &[]).unwrap(), build_levels(&dir, &[], &[]).unwrap());
        std::fs::write(dir.join("stream.json"), &text).unwrap();
        assert!(problems(&root, "demo").is_empty(), "fresh with levels");
    }

    #[test]
    fn manifest_is_written_and_kept_fresh() {
        let root = crate::testutil::temp_root("stream-manifest");
        let dir = root.join("apps/demo/assets/models");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("apps/demo/evani.toml"), "name = \"demo\"\n[stream]\nboot = [\"logo.png\"]\n").unwrap();
        for (f, b) in [("hero.glb", "a"), ("hero.low.glb", "b"), ("hero.low.base.jpg", "c"), ("logo.png", "d")] {
            std::fs::write(dir.join(f), b).unwrap();
        }
        let p = problems(&root, "demo");
        assert_eq!(p.len(), 1, "tier files without a manifest");
        assert!(p[0].what.contains("no assets/models/stream.json") && p[0].fix.contains("cargo xtask manifest demo"));
        let text = build(&dir, &boot_list(&std::fs::read_to_string(root.join("apps/demo/evani.toml")).unwrap())).unwrap().unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], 2);
        let by = |n: &str| v["assets"].as_array().unwrap().iter().find(|e| e["name"] == n).unwrap().clone();
        assert_eq!((by("hero.low.glb")["tier"].as_str(), by("hero.low.glb")["kind"].as_str()), (Some("low"), Some("model")));
        assert!(by("hero.glb").get("tier").is_none());
        assert_eq!(by("logo.png")["group"], "boot");
        assert_eq!(by("hero.low.base.jpg")["group"], "later");
        assert!(by("hero.low.glb")["priority"].as_i64() > by("hero.low.base.jpg")["priority"].as_i64());
        std::fs::write(dir.join("stream.json"), &text).unwrap();
        assert!(problems(&root, "demo").is_empty(), "fresh");
        std::fs::write(dir.join("hero.low.glb"), "changed").unwrap();
        let p = problems(&root, "demo");
        assert!(p.len() == 1 && p[0].what.contains("no longer matches"), "stale after a file changed");
        // A game with only shared files needs no manifest.
        let root2 = crate::testutil::temp_root("stream-manifest-plain");
        std::fs::create_dir_all(root2.join("apps/demo/assets/models")).unwrap();
        std::fs::write(root2.join("apps/demo/assets/models/hero.glb"), "a").unwrap();
        assert!(problems(&root2, "demo").is_empty());
    }
}
