//! Baked data in hot mode (docs/LIVE_API.md 10.4): a bake writes a look's or
//! kit's data files, and those must not set off a Rust code swap.
//!
//! * When dx starts, the bridge reads every look's and kit's baked files
//!   (the set `tools/bake-index.rs` embeds) and gives dx `EVANI_BAKED_STAMP`,
//!   a hash of them. With the stamp set, the build script writes the
//!   contents into the build and tracks no data file, so dx never rebuilds
//!   for one.
//! * The files as they were then are kept. `/` gets
//!   `<script type="application/json" id="evani-live-baked">` with every file
//!   that differs now (`{owner: {path: text}}`), and the page's live module
//!   applies them at start, so a reload always shows the latest bake.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

/// `(owner, path in the crate)` → text. Owners are `look.<name>` for
/// `looks/evani-look-<name>` and `kit.<name>` for `kits/evani-kit-<name>`
/// (the names `App::look`/`App::kit` give them).
pub type Snapshot = BTreeMap<(String, String), String>;

/// The `<script>` element's id the page reads.
pub const SCRIPT_ID: &str = "evani-live-baked";

/// Every look's and kit's baked files under `root` now.
pub fn snapshot(root: &Path) -> Snapshot {
    let mut out = Snapshot::new();
    for (dir, prefix, owner) in [("looks", "evani-look-", "look"), ("kits", "evani-kit-", "kit")] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(short) = name.strip_prefix(prefix) else { continue };
            let crate_dir = e.path();
            if !crate_dir.is_dir() {
                continue;
            }
            let owner = format!("{owner}.{short}");
            for rel in crate_files(&crate_dir) {
                if let Ok(text) = std::fs::read_to_string(crate_dir.join(&rel)) {
                    out.insert((owner.clone(), rel), text);
                }
            }
        }
    }
    out
}

/// The baked files of one crate, as `tools/bake-index.rs` picks them:
/// `params.json`, `policy.json`, `slots/*.wgsl`, `slots/*.expr`,
/// `curves/*.json`.
fn crate_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["params.json", "policy.json"] {
        if dir.join(name).is_file() {
            out.push(name.to_string());
        }
    }
    for (sub, exts) in [("slots", &["wgsl", "expr"][..]), ("curves", &["json"][..])] {
        let Ok(entries) = std::fs::read_dir(dir.join(sub)) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if p.is_file() && exts.contains(&ext) {
                out.push(format!("{sub}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    out.sort();
    out
}

/// `EVANI_BAKED_STAMP`: FNV-1a 64 of every owner, path and text, in hex.
pub fn stamp(s: &Snapshot) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for ((owner, rel), text) in s {
        for part in [owner.as_bytes(), b"\0", rel.as_bytes(), b"\0", text.as_bytes(), b"\0"] {
            for b in part {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    format!("{h:016x}")
}

/// The files of `now` that are new or differ from `built`, as
/// `{owner: {path: text}}` (empty: the build has them all).
pub fn changed(built: &Snapshot, now: &Snapshot) -> Map<String, Value> {
    let mut out = Map::new();
    for ((owner, rel), text) in now {
        if built.get(&(owner.clone(), rel.clone())) == Some(text) {
            continue;
        }
        let entry = out.entry(owner.clone()).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = entry {
            m.insert(rel.clone(), Value::String(text.clone()));
        }
    }
    out
}

/// `index` with the changed files in a JSON `<script>` at the end of
/// `<head>` (or at the start without one). Every `<` in the JSON is
/// written as `<`, so no text can close the element.
pub fn inject(index: &str, files: &Map<String, Value>) -> String {
    if files.is_empty() {
        return index.to_string();
    }
    let json = Value::Object(files.clone()).to_string().replace('<', "\\u003c");
    let tag = format!("<script type=\"application/json\" id=\"{SCRIPT_ID}\">{json}</script>");
    match index.to_ascii_lowercase().find("</head") {
        Some(at) => format!("{}{tag}\n{}", &index[..at], &index[at..]),
        None => format!("{tag}{index}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_stamp_and_changes() {
        let root = std::env::temp_dir().join(format!("evani-bridge-baked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let look = root.join("looks/evani-look-toon");
        std::fs::create_dir_all(look.join("slots")).unwrap();
        std::fs::create_dir_all(root.join("kits/evani-kit-kart-racer")).unwrap();
        std::fs::create_dir_all(root.join("looks/not-a-look")).unwrap();
        std::fs::write(look.join("params.json"), r#"{"look.toon.bands":3}"#).unwrap();
        std::fs::write(look.join("slots/fog.wgsl"), "fn fog() {}").unwrap();
        std::fs::write(look.join("slots/notes.txt"), "ignored").unwrap();
        std::fs::write(look.join("CHANGES.md"), "ignored").unwrap();
        std::fs::write(root.join("kits/evani-kit-kart-racer/params.json"), "{}").unwrap();
        std::fs::write(root.join("looks/not-a-look/params.json"), "{}").unwrap();

        let built = snapshot(&root);
        let keys: Vec<String> = built.keys().map(|(o, r)| format!("{o}:{r}")).collect();
        assert_eq!(keys, ["kit.kart-racer:params.json", "look.toon:params.json", "look.toon:slots/fog.wgsl"]);
        let st = stamp(&built);
        assert_eq!(st.len(), 16);
        assert_eq!(st, stamp(&snapshot(&root)), "same files, same stamp");
        assert!(changed(&built, &built).is_empty());
        assert_eq!(inject("<head></head>", &changed(&built, &built)), "<head></head>");

        // A bake: only the changed file goes to the page.
        std::fs::write(look.join("params.json"), r#"{"look.toon.bands":4,"x":"</script><!--"}"#).unwrap();
        let now = snapshot(&root);
        assert_ne!(stamp(&now), st);
        let c = changed(&built, &now);
        assert_eq!(c.len(), 1);
        assert_eq!(c["look.toon"].as_object().unwrap().len(), 1);
        let page = inject("<html><HEAD><title>g</title></HEAD><body></body></html>", &c);
        assert!(page.contains("<title>g</title><script type=\"application/json\" id=\"evani-live-baked\">"), "{page}");
        assert!(!page.contains("</script><!--"), "{page}");
        let start = page.find("baked\">").unwrap() + 7;
        let end = start + page[start..].find("</script>").unwrap();
        let back: Value = serde_json::from_str(&page[start..end]).unwrap();
        assert_eq!(back["look.toon"]["params.json"], r#"{"look.toon.bands":4,"x":"</script><!--"}"#);
        let _ = std::fs::remove_dir_all(&root);
    }
}
