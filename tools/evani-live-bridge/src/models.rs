//! Model mode (`serve --models <folder>`, docs/EVANI_MODEL_TAB.md 4.2, 4.3,
//! 5.3 and 8): the Evani.Model workspace read for the Model tab's page.
//!
//! The bridge only reads here. A version is complete when its folder is
//! named `v<N>` and holds `version.json` (writers rename a `v<N>.tmp-<pid>`
//! folder into place with `version.json` written last, 8.3); a folder
//! without it, or named `*.tmp-*`, is never read. Files are served only when
//! that version's `version.json` lists them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

/// How often the watcher looks at the workspace (8.3).
pub const WATCH_MS: u64 = 250;

/// The requests folder (7.3); model ids never start with `_`.
const REQUESTS: &str = "_requests";

/// A model id: lowercase letters, digits, `-` and `_`, not starting with `_`.
pub fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.starts_with('_')
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// A request id: letters, digits, `-` and `_` (so never a path).
fn valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `v12` → 12 (a complete version folder name; never `v12.tmp-…`).
fn version_of(name: &str) -> Option<u64> {
    let n = name.strip_prefix('v')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// One error a `model.*` command or route answers with.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelErr {
    pub code: &'static str,
    pub message: String,
}

fn merr(code: &'static str, message: impl Into<String>) -> ModelErr {
    ModelErr { code, message: message.into() }
}

/// The workspace folder.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub dir: PathBuf,
    /// The folder as given (for `/live/status`).
    pub shown: String,
}

impl Workspace {
    pub fn new(dir: PathBuf, shown: String) -> Self {
        Self { dir, shown }
    }

    /// Model folders (valid ids only), sorted by id.
    fn model_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_model_id(n))
            .collect();
        out.sort();
        out
    }

    /// Complete versions of a model, oldest first.
    fn versions(&self, model: &str) -> Vec<u64> {
        let mut out: Vec<u64> = std::fs::read_dir(self.dir.join(model))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|n| version_of(&n))
            .filter(|n| self.dir.join(model).join(format!("v{n}")).join("version.json").is_file())
            .collect();
        out.sort_unstable();
        out
    }

    fn version_json(&self, model: &str, n: u64) -> Option<Value> {
        let bytes = std::fs::read(self.dir.join(model).join(format!("v{n}")).join("version.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// `GET /model/v1/models` and `model.list`: newest first.
    pub fn list(&self) -> Value {
        let mut rows: Vec<(SystemTime, Value)> = Vec::new();
        for m in self.model_ids() {
            let vs = self.versions(&m);
            let Some(&latest) = vs.last() else { continue };
            let vj = self.version_json(&m, latest).unwrap_or(Value::Null);
            let class = vj.get("class").cloned().filter(|c| !c.is_null()).or_else(|| {
                let meta = vj.get("meta")?.as_str()?;
                let bytes = std::fs::read(self.dir.join(&m).join(format!("v{latest}")).join(safe_name(meta)?)).ok()?;
                serde_json::from_slice::<Value>(&bytes).ok()?.get("class").cloned()
            });
            let when = std::fs::metadata(self.dir.join(&m).join(format!("v{latest}")).join("version.json"))
                .and_then(|md| md.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            rows.push((when, json!({ "model": m, "class": class.unwrap_or(Value::Null), "versions": vs.len(),
                                     "latest": latest, "updated": vj.get("created").cloned().unwrap_or(Value::Null) })));
        }
        // Newest first; the same time keeps id order.
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        Value::Array(rows.into_iter().map(|r| r.1).collect())
    }

    /// `GET /model/v1/<model>/history` and `model.history`: every
    /// `version.json`, oldest first.
    pub fn history(&self, model: &str) -> Result<Value, ModelErr> {
        if !valid_model_id(model) || !self.dir.join(model).is_dir() {
            return Err(merr("unknown_model", format!("No model named {model}")));
        }
        let out: Vec<Value> = self.versions(model).into_iter().filter_map(|n| self.version_json(model, n)).collect();
        Ok(Value::Array(out))
    }

    /// `GET /model/v1/requests/<request>`: the status file, or idle.
    pub fn request(&self, request: &str) -> Value {
        if !valid_request_id(request) {
            return json!({ "state": "idle" });
        }
        std::fs::read(self.dir.join(REQUESTS).join(format!("{request}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| json!({ "state": "idle" }))
    }

    /// The file a `GET /model/v1/<model>/v<N>/<file>` may serve: only one
    /// that version's `version.json` lists (a tier file, the metadata, the
    /// intent). Anything else is `None` (404).
    pub fn version_file(&self, model: &str, version: &str, file: &str) -> Option<PathBuf> {
        if !valid_model_id(model) {
            return None;
        }
        let n = version_of(version)?;
        let file = safe_name(file)?;
        let vj = self.version_json(model, n)?;
        let mut listed: BTreeSet<&str> = BTreeSet::new();
        if let Some(t) = vj.get("tiers").and_then(Value::as_object) {
            listed.extend(t.values().filter_map(|t| t.get("file").and_then(Value::as_str)));
        }
        for k in ["meta", "intent", "sheet"] {
            if let Some(f) = vj.get(k).and_then(Value::as_str) {
                listed.insert(f);
            }
        }
        if !listed.contains(file) {
            return None;
        }
        let p = self.dir.join(model).join(format!("v{n}")).join(file);
        p.is_file().then_some(p)
    }

    /// `/live/status` `models` (4.2).
    pub fn status(&self) -> Value {
        let list = self.list();
        let rows = list.as_array().map_or(&[][..], Vec::as_slice);
        json!({ "folder": self.shown, "count": rows.len(),
                "latest": rows.first().and_then(|r| r.get("model")).cloned().unwrap_or(Value::Null) })
    }

    /// `model.*` commands answered by the bridge itself (5.3).
    pub fn command(&self, cmd: &str, params: &Value) -> Option<Result<Value, ModelErr>> {
        Some(match cmd {
            "model.list" => Ok(self.list()),
            "model.history" => match params.get("model").and_then(Value::as_str) {
                Some(m) => self.history(m),
                None => Err(merr("bad_request", "model.history needs \"model\"")),
            },
            _ => return None,
        })
    }
}

/// A plain file name (no folders, no `..`), as listed in `version.json`.
fn safe_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.starts_with('.');
    ok.then_some(name)
}

/// What the watcher has seen: complete versions and request files.
#[derive(Debug, Default)]
pub struct Watcher {
    started: bool,
    versions: BTreeSet<(String, u64)>,
    requests: BTreeMap<String, Value>,
}

impl Watcher {
    /// One look at the workspace: the events for what is new since the last
    /// look. The first look only records what is there (no events).
    pub fn step(&mut self, ws: &Workspace) -> Vec<(&'static str, Value)> {
        let mut events = Vec::new();
        let first = !self.started;
        self.started = true;
        for m in ws.model_ids() {
            for n in ws.versions(&m) {
                if self.versions.contains(&(m.clone(), n)) {
                    continue;
                }
                // version.json is written last, so a complete folder reads.
                let Some(vj) = ws.version_json(&m, n) else { continue };
                self.versions.insert((m.clone(), n));
                if !first {
                    events.push(("model_version", vj));
                }
            }
        }
        let dir = ws.dir.join(REQUESTS);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
            .filter(|n| valid_request_id(n))
            .collect();
        names.sort();
        for r in names {
            // A file being written may not parse yet: look again next time.
            let Some(v) = std::fs::read(dir.join(format!("{r}.json"))).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
                continue;
            };
            if self.requests.get(&r) == Some(&v) {
                continue;
            }
            self.requests.insert(r, v.clone());
            if !first {
                events.push(("model_status", v));
            }
        }
        events
    }
}

/// The workspace as an absolute path from `--models <folder>` (relative to
/// the engine root).
pub fn resolve(root: &Path, folder: &Path) -> PathBuf {
    if folder.is_absolute() { folder.to_path_buf() } else { root.join(folder) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-models-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Write a complete version the way writers do (8.3).
    fn write_version(dir: &Path, model: &str, n: u64, files: &[(&str, &[u8])]) {
        let tmp = dir.join(model).join(format!("v{n}.tmp-1"));
        std::fs::create_dir_all(&tmp).unwrap();
        for (f, b) in files {
            std::fs::write(tmp.join(f), b).unwrap();
        }
        let vj = json!({ "format": 1, "model": model, "version": n, "parent": if n > 1 { json!(n - 1) } else { Value::Null },
            "by": "tool", "tiers": { "low": { "file": "m.low.glb", "tris": 12 } }, "meta": "m.evmodel.json", "created": "2026-10-16T19:02:11Z" });
        std::fs::write(tmp.join("version.json"), vj.to_string()).unwrap();
        std::fs::rename(&tmp, dir.join(model).join(format!("v{n}"))).unwrap();
    }

    #[test]
    fn model_routes_serve_only_listed_files() {
        let d = tmp("routes");
        write_version(&d, "kart-06b3", 1, &[("m.low.glb", b"glTF"), ("m.evmodel.json", b"{}"), ("secret.txt", b"no")]);
        std::fs::write(d.join("kart-06b3").join("reference.json"), "{}").unwrap();
        let ws = Workspace::new(d.clone(), ".evani/model".into());
        assert_eq!(ws.version_file("kart-06b3", "v1", "m.low.glb"), Some(d.join("kart-06b3").join("v1").join("m.low.glb")));
        assert!(ws.version_file("kart-06b3", "v1", "m.evmodel.json").is_some());
        // Unlisted, traversal, other folders, bad ids: all None (404).
        assert_eq!(ws.version_file("kart-06b3", "v1", "secret.txt"), None);
        assert_eq!(ws.version_file("kart-06b3", "v1", "version.json"), None);
        assert_eq!(ws.version_file("kart-06b3", "v1", "../reference.json"), None);
        assert_eq!(ws.version_file("kart-06b3", "v1", "..\\reference.json"), None);
        assert_eq!(ws.version_file("kart-06b3", "v1", ".."), None);
        assert_eq!(ws.version_file("kart-06b3", "v2", "m.low.glb"), None);
        assert_eq!(ws.version_file("kart-06b3", "v1.tmp-1", "m.low.glb"), None);
        assert_eq!(ws.version_file("..", "v1", "m.low.glb"), None);
        assert_eq!(ws.version_file("_requests", "v1", "m.low.glb"), None);
        assert_eq!(ws.version_file("KART", "v1", "m.low.glb"), None);
        // A listed name that tries to climb out is refused too.
        let v1 = d.join("kart-06b3").join("v1").join("version.json");
        std::fs::write(&v1, json!({ "tiers": { "low": { "file": "../reference.json" } } }).to_string()).unwrap();
        assert_eq!(ws.version_file("kart-06b3", "v1", "../reference.json"), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn list_and_history_skip_incomplete_versions() {
        let d = tmp("list");
        write_version(&d, "kart-06b3", 1, &[("m.low.glb", b"glTF")]);
        write_version(&d, "kart-06b3", 2, &[("m.low.glb", b"glTF")]);
        // A half-written folder and a folder without version.json are never read.
        std::fs::create_dir_all(d.join("kart-06b3").join("v3.tmp-9")).unwrap();
        std::fs::create_dir_all(d.join("kart-06b3").join("v4")).unwrap();
        std::fs::create_dir_all(d.join("_requests")).unwrap();
        let ws = Workspace::new(d.clone(), ".evani/model".into());
        let list = ws.list();
        assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
        assert_eq!(list[0]["model"], "kart-06b3");
        assert_eq!(list[0]["versions"], 2);
        assert_eq!(list[0]["latest"], 2);
        let h = ws.history("kart-06b3").unwrap();
        assert_eq!(h.as_array().map(|a| a.iter().map(|v| v["version"].as_u64().unwrap()).collect::<Vec<_>>()), Some(vec![1, 2]));
        assert_eq!(ws.history("nope").unwrap_err().code, "unknown_model");
        assert_eq!(ws.history("_requests").unwrap_err().code, "unknown_model");
        assert_eq!(ws.status()["count"], 1);
        assert_eq!(ws.status()["latest"], "kart-06b3");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn new_version_folder_sends_one_event() {
        let d = tmp("watch-version");
        write_version(&d, "kart-06b3", 1, &[("m.low.glb", b"glTF")]);
        let ws = Workspace::new(d.clone(), ".evani/model".into());
        let mut w = Watcher::default();
        assert!(w.step(&ws).is_empty(), "the first look only records what is there");
        // A writer's temporary folder is invisible.
        std::fs::create_dir_all(d.join("kart-06b3").join("v2.tmp-7")).unwrap();
        std::fs::write(d.join("kart-06b3").join("v2.tmp-7").join("m.low.glb"), b"glTF").unwrap();
        assert!(w.step(&ws).is_empty());
        let _ = std::fs::remove_dir_all(d.join("kart-06b3").join("v2.tmp-7"));
        write_version(&d, "kart-06b3", 2, &[("m.low.glb", b"glTF")]);
        let ev = w.step(&ws);
        assert_eq!(ev.len(), 1, "{ev:?}");
        assert_eq!(ev[0].0, "model_version");
        assert_eq!(ev[0].1["version"], 2);
        assert!(w.step(&ws).is_empty(), "one event, not one per look");
        // A brand-new model's first version is news too.
        write_version(&d, "character-a861", 1, &[("m.low.glb", b"glTF")]);
        let ev = w.step(&ws);
        assert_eq!((ev.len(), ev[0].1["model"].as_str()), (1, Some("character-a861")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn request_status_sends_one_event() {
        let d = tmp("watch-request");
        let req = d.join("_requests");
        std::fs::create_dir_all(&req).unwrap();
        let ws = Workspace::new(d.clone(), ".evani/model".into());
        let mut w = Watcher::default();
        assert!(w.step(&ws).is_empty());
        // A request for a new model: no model folder yet, `model` is null.
        let s1 = json!({ "request": "r-7f3a9c-0043", "model": null, "state": "working", "step": "changing", "line": "Changing the model", "version": null });
        std::fs::write(req.join("r-7f3a9c-0043.json"), s1.to_string()).unwrap();
        let ev = w.step(&ws);
        assert_eq!(ev.len(), 1, "{ev:?}");
        assert_eq!(ev[0].0, "model_status");
        assert_eq!(ev[0].1["model"], Value::Null);
        assert!(w.step(&ws).is_empty(), "unchanged: no event");
        assert_eq!(ws.request("r-7f3a9c-0043")["state"], "working");
        // Half-written: skipped until it parses.
        std::fs::write(req.join("r-7f3a9c-0043.json"), b"{\"request\":").unwrap();
        assert!(w.step(&ws).is_empty());
        let s2 = json!({ "request": "r-7f3a9c-0043", "model": "marshal-3a1f", "state": "failed", "line": "A word was misspelt" });
        std::fs::write(req.join("r-7f3a9c-0043.json"), s2.to_string()).unwrap();
        let ev = w.step(&ws);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].1["state"], "failed");
        assert_eq!(ws.request("r-unknown")["state"], "idle");
        assert_eq!(ws.request("../x")["state"], "idle");
        let _ = std::fs::remove_dir_all(&d);
    }
}
