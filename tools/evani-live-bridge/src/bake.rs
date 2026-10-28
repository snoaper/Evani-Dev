//! `bake.export` on the bridge side (docs/LIVE_API.md 6.23): the page answers
//! with a bundle of tuned values; the bridge plans the files with
//! `evani_live::bake::plan` against the engine root and writes them.
//!
//! Writes stay inside the engine root: relative paths only, no `..`, and the
//! parent folder is canonicalized and checked to be under the root, so a
//! symlink can't lead a write elsewhere.

use std::path::{Component, Path, PathBuf};

use evani_live::bake;
use serde_json::{Value, json};

use crate::proto;

/// `root/rel` if `rel` is a plain relative `/`-separated path.
fn join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel.contains('\\') || rel.contains(':') {
        return Err(format!("refusing path {rel}"));
    }
    let p = Path::new(rel);
    if !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("refusing path {rel}"));
    }
    Ok(root.join(p))
}

/// The real folder of `path`'s parent, if it exists and is under `root`.
fn inside(root: &Path, path: &Path, rel: &str) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("engine root: {e}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("refusing path {rel}"))?;
    let real = parent.canonicalize().map_err(|e| format!("{rel}: {e}"))?;
    if !real.starts_with(&root) {
        return Err(format!("{rel} leads outside the engine root"));
    }
    if let Ok(m) = std::fs::symlink_metadata(path)
        && m.file_type().is_symlink()
    {
        return Err(format!("{rel} is a symlink; not writing through it"));
    }
    Ok(())
}

/// Read a file under the root (`None` if missing or not safely inside).
pub fn read(root: &Path, rel: &str) -> Option<String> {
    let p = join(root, rel).ok()?;
    if !p.is_file() || inside(root, &p, rel).is_err() {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// Write `contents` to `root/rel`, creating folders, refusing escapes.
pub fn write(root: &Path, rel: &str, contents: &str) -> Result<(), String> {
    write_bytes(root, rel, contents.as_bytes())
}

/// Delete the file `root/rel`, refusing escapes. Missing is fine.
pub fn remove(root: &Path, rel: &str) -> Result<(), String> {
    let p = join(root, rel)?;
    if !p.exists() {
        return Ok(());
    }
    inside(root, &p, rel)?;
    std::fs::remove_file(&p).map_err(|e| format!("{rel}: {e}"))
}

/// [`write`] for bytes.
pub fn write_bytes(root: &Path, rel: &str, contents: &[u8]) -> Result<(), String> {
    let p = join(root, rel)?;
    let parent = p.parent().ok_or_else(|| format!("refusing path {rel}"))?;
    // Check the deepest existing ancestor before creating anything.
    let mut probe = parent;
    while !probe.exists() {
        probe = probe
            .parent()
            .ok_or_else(|| format!("refusing path {rel}"))?;
    }
    inside(root, &probe.join("x"), rel)?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{rel}: {e}"))?;
    inside(root, &p, rel)?;
    std::fs::write(&p, contents).map_err(|e| format!("{rel}: {e}"))
}

/// Turn the page's OK `bake.export` result (`result.bundle`) into the 6.23
/// result, writing the files unless `dry_run`.
pub fn finish(root: &Path, id: &Value, res: Value, now_utc: &str) -> Value {
    let bundle = &res["result"]["bundle"];
    if !bundle.is_object() {
        return proto::error(id, "internal", "The page sent no bake bundle.", None);
    }
    let plan = match bake::plan(bundle, |rel| read(root, rel), now_utc) {
        Ok(p) => p,
        Err(e) => return proto::error(id, e.code, &e.message, e.data),
    };
    if !plan.dry_run {
        for f in &plan.files {
            if let Err(msg) = write(root, &f.path, &f.contents) {
                return proto::error(id, "io_error", &msg, Some(json!({ "path": f.path })));
            }
        }
    }
    let mut out = res;
    out["result"] = plan.to_result();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("evani-bridge-bake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn page_result(dry_run: bool) -> Value {
        json!({ "type": "result", "id": "x", "ok": true, "result": { "bundle": {
            "title": "T", "dry_run": dry_run, "from_tx": 1, "to_tx": 2, "game": "sandbox",
            "look_crate": "evani-look-basic", "kit_crate": "evani-kit-example",
            "params": [{ "name": "look.basic.saturation", "bake": "look", "value": 1.2 }],
            "slots": [], "curves": [], "tiers": [], "notes": ["**look.basic.saturation** 1.0 → 1.2"]
        } } })
    }

    #[test]
    fn writes_files_and_reports_the_6_23_shape() {
        let root = temp("write");
        let r = finish(&root, &json!("c1"), page_result(false), "2026-10-14 12:00");
        assert_eq!(r["ok"], true, "{r}");
        let res = &r["result"];
        assert_eq!(res["written"].as_array().unwrap().len(), 2);
        assert_eq!(res["changes_md"], "looks/evani-look-basic/CHANGES.md");
        assert!(res["written"][0].get("preview").is_none());
        let text =
            std::fs::read_to_string(root.join("looks/evani-look-basic/params.json")).unwrap();
        assert!(text.contains("\"look.basic.saturation\": 1.2"));
        // Again: nothing differs, nothing written.
        let r = finish(&root, &json!("c2"), page_result(false), "2026-10-14 12:05");
        assert_eq!(r["result"]["written"], json!([]));
        assert_eq!(
            r["result"]["unchanged"],
            json!(["looks/evani-look-basic/params.json"])
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_writes_nothing_and_previews() {
        let root = temp("dry");
        let r = finish(&root, &json!("c1"), page_result(true), "t");
        assert!(r["result"]["written"][0]["preview"].is_string(), "{r}");
        assert!(!root.join("looks").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_escapes() {
        let root = temp("escape");
        for rel in [
            "../x.json",
            "/abs.json",
            "C:/x.json",
            "a\\b.json",
            "a/../../b.json",
            "",
        ] {
            assert!(write(&root, rel, "{}").is_err(), "{rel}");
        }
        assert!(write(&root, "looks/evani-look-basic/params.json", "{}").is_ok());
        let mut bad = page_result(false);
        bad["result"]["bundle"]["look_crate"] = json!("../evil");
        let r = finish(&root, &json!("c"), bad, "t");
        assert_eq!(r["ok"], false);
        let _ = std::fs::remove_dir_all(&root);
    }
}
