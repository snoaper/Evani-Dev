//! `cargo xtask model-import <model> <dir> [--workspace <folder>]`: puts a
//! folder Evani.Model compiled (`<name>.evmodel.json`, four tier `.glb`s and
//! their `*.normal.png`) into the model workspace as the next version, by the
//! rules of EVANI_MODEL_TAB.md 8.3 (temporary folder, `version.json` last,
//! exclusive rename, retry with N+1). It stands in for MT-a's
//! `model_compile` until that is merged.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Tier order for the model hash (EVANI_MODEL_TAB.md 8.2).
const TIERS: [&str; 4] = ["low", "medium", "high", "ultra"];

pub fn usage() -> &'static str {
    "  cargo xtask model-import <model> <dir> [--workspace <folder>]   (default workspace .evani/model)"
}

/// The bridge's rule: lowercase letters, digits, `-` and `_`, never a leading
/// `_` (the `_requests` folder relies on it).
pub fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.starts_with('_')
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

pub fn cmd_model_import(args: &[String], root: &Path) -> Result<(), String> {
    let mut pos = Vec::new();
    let mut workspace = PathBuf::from(".evani/model");
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--workspace" {
            workspace = PathBuf::from(args.get(i + 1).ok_or_else(|| format!("--workspace needs a folder\n{}", usage()))?);
            i += 2;
        } else {
            pos.push(args[i].clone());
            i += 1;
        }
    }
    let [model, dir] = pos.as_slice() else {
        return Err(format!("model-import needs <model> <dir>\n{}", usage()));
    };
    let ws = if workspace.is_absolute() { workspace } else { root.join(workspace) };
    let (n, path) = import(model, Path::new(dir), &ws, SystemTime::now())?;
    println!("model-import: {model} v{n} -> {}", path.display());
    Ok(())
}

/// Highest complete version (`v<N>` holding a `version.json`) of a model folder.
fn highest_complete(model_dir: &Path) -> Option<u64> {
    fs::read_dir(model_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n: u64 = name.strip_prefix('v')?.parse().ok()?;
            e.path().join("version.json").is_file().then_some(n)
        })
        .max()
}

/// Imports `dir` as the next version of `model` in `ws`; returns its number
/// and folder.
pub fn import(model: &str, dir: &Path, ws: &Path, now: SystemTime) -> Result<(u64, PathBuf), String> {
    if !valid_model_id(model) {
        return Err(format!(
            "model id `{model}` refused: use lowercase letters, digits, `-` and `_`, not starting with `_`\nfix: pick an id like `kart-06b3`"
        ));
    }
    let meta_path = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".evmodel.json")))
        .ok_or_else(|| format!("{}: no <name>.evmodel.json\nfix: point at a folder Evani.Model compiled", dir.display()))?;
    let meta_name = file_name(&meta_path)?;
    let meta: Value = serde_json::from_slice(&fs::read(&meta_path).map_err(|e| format!("{meta_name}: {e}"))?)
        .map_err(|e| format!("{meta_name}: {e}"))?;
    let mtiers = meta.get("tiers").and_then(Value::as_object).ok_or_else(|| format!("{meta_name}: no `tiers`"))?;

    let mut tiers = serde_json::Map::new();
    let mut files: Vec<PathBuf> = vec![meta_path.clone()];
    let mut hasher = Sha256::new();
    for t in TIERS {
        let Some(mt) = mtiers.get(t) else { continue };
        let file = mt.get("file").and_then(Value::as_str).ok_or_else(|| format!("{meta_name}: tier `{t}` has no file"))?;
        if file.contains(['/', '\\']) || file.contains("..") {
            return Err(format!("{meta_name}: tier `{t}` file `{file}` is not a plain name"));
        }
        let p = dir.join(file);
        hasher.update(fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?);
        files.push(p);
        let mut row = serde_json::Map::new();
        row.insert("file".into(), json!(file));
        for k in ["tris", "verts", "kb"] {
            if let Some(v) = mt.get(k) {
                row.insert(k.into(), v.clone());
            }
        }
        tiers.insert(t.into(), Value::Object(row));
    }
    if tiers.is_empty() {
        return Err(format!("{meta_name}: no tier among {TIERS:?}"));
    }
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok) {
        let p = e.path();
        if p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".normal.png")) {
            files.push(p);
        }
    }

    let model_dir = ws.join(model);
    fs::create_dir_all(&model_dir).map_err(|e| format!("{}: {e}", model_dir.display()))?;
    let parent = highest_complete(&model_dir);
    let tmp = model_dir.join(format!("v{}.tmp-{}", parent.unwrap_or(0) + 1, std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    }
    fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let r = fill_and_publish(model, dir, &meta, mtiers, tiers, &meta_name, hasher, &files, &model_dir, &tmp, parent, now);
    if r.is_err() {
        // Never leave a half-written folder behind.
        let _ = fs::remove_dir_all(&tmp);
    }
    r
}

/// Copies the files into `tmp`, writes `version.json` last and renames it to
/// the next free `v<N>`. `parent` stays the version this writer started from
/// (EVANI_MODEL_TAB.md 8.3), even when it retries with N+1.
#[allow(clippy::too_many_arguments)]
fn fill_and_publish(
    model: &str,
    dir: &Path,
    meta: &Value,
    mtiers: &serde_json::Map<String, Value>,
    tiers: serde_json::Map<String, Value>,
    meta_name: &str,
    hasher: Sha256,
    files: &[PathBuf],
    model_dir: &Path,
    tmp: &Path,
    parent: Option<u64>,
    now: SystemTime,
) -> Result<(u64, PathBuf), String> {
    for f in files {
        let to = tmp.join(file_name(f)?);
        fs::copy(f, &to).map_err(|e| format!("{} -> {}: {e}", f.display(), to.display()))?;
    }
    let mut vj = json!({
        "format": 1, "model": model, "version": 0, "parent": parent,
        "request": null, "said": null,
        "summary": format!("Imported from {}.", dir.display()), "by": "tool",
        "restored_from": null, "via": null, "changed_parts": [],
        "hash": format!("sha256:{}", hex(&hasher.finalize())),
        "intent_hash": meta.get("intent").cloned().unwrap_or(Value::Null),
        "tiers": tiers, "meta": meta_name,
        "class": meta.get("class").cloned().unwrap_or(Value::Null),
        "checks": { "must": mtiers.get("ultra").or_else(|| mtiers.values().next()).and_then(|t| t.get("must")).cloned().unwrap_or(Value::Null) },
        "created": iso_utc(now),
    });
    let mut n = highest_complete(model_dir).unwrap_or(0) + 1;
    for _ in 0..1000 {
        vj["version"] = json!(n);
        let text = serde_json::to_string_pretty(&vj).map_err(|e| e.to_string())?;
        fs::write(tmp.join("version.json"), text).map_err(|e| format!("version.json: {e}"))?;
        let dest = model_dir.join(format!("v{n}"));
        if dest.exists() {
            n += 1;
            continue;
        }
        match fs::rename(tmp, &dest) {
            Ok(()) => return Ok((n, dest)),
            Err(_) if dest.exists() => n += 1,
            Err(e) => return Err(format!("{} -> {}: {e}", tmp.display(), dest.display())),
        }
    }
    Err(format!("{}: no free version number", model_dir.display()))
}

fn file_name(p: &Path) -> Result<String, String> {
    p.file_name().and_then(|n| n.to_str()).map(str::to_string).ok_or_else(|| format!("{}: no file name", p.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// "2026-10-16T19:02:11Z".
pub fn iso_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm), as review::utc_string.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-model-import-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A tiny compiled folder: meta, two tiers and a normal map.
    fn source(d: &Path) -> PathBuf {
        let src = d.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("kart.low.glb"), b"low").unwrap();
        fs::write(src.join("kart.ultra.glb"), b"ultra").unwrap();
        fs::write(src.join("kart.low.body.normal.png"), b"png").unwrap();
        fs::write(src.join("notes.txt"), b"not copied").unwrap();
        let meta = json!({ "model": "kart-06b3", "class": "kart", "intent": "sha256:abc",
            "tiers": { "low": { "file": "kart.low.glb", "tris": 2, "kb": 1, "must": "7/7" },
                       "ultra": { "file": "kart.ultra.glb", "tris": 9, "kb": 3, "must": "7/7" } } });
        fs::write(src.join("kart.evmodel.json"), meta.to_string()).unwrap();
        src
    }

    #[test]
    fn underscore_model_id_is_refused() {
        let d = scratch("ids");
        let src = source(&d);
        for bad in ["_requests", "Kart", "kart/../x", ""] {
            let e = import(bad, &src, &d.join("ws"), SystemTime::now()).unwrap_err();
            assert!(e.contains("refused") && e.contains("fix:"), "{bad}: {e}");
        }
        assert!(!d.join("ws").exists(), "nothing written for a refused id");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn import_writes_next_complete_version() {
        let d = scratch("next");
        let src = source(&d);
        let ws = d.join("ws");
        let t = UNIX_EPOCH + Duration::from_secs(1_791_918_131);
        let (n1, p1) = import("kart-06b3", &src, &ws, t).unwrap();
        assert_eq!(n1, 1);
        let vj: Value = serde_json::from_slice(&fs::read(p1.join("version.json")).unwrap()).unwrap();
        assert_eq!(vj["version"], 1);
        assert_eq!(vj["parent"], Value::Null);
        assert_eq!(vj["by"], "tool");
        assert_eq!(vj["class"], "kart");
        assert_eq!(vj["meta"], "kart.evmodel.json");
        assert_eq!(vj["intent_hash"], "sha256:abc");
        assert_eq!(vj["tiers"]["ultra"]["tris"], 9);
        assert_eq!(vj["created"], "2026-10-13T19:02:11Z");
        let mut h = Sha256::new();
        h.update(b"low");
        h.update(b"ultra");
        assert_eq!(vj["hash"], format!("sha256:{}", hex(&h.finalize())));
        assert!(p1.join("kart.low.body.normal.png").is_file());
        assert!(!p1.join("notes.txt").exists());

        // A half-written folder and a taken number are skipped.
        fs::create_dir_all(ws.join("kart-06b3").join("v2.tmp-1")).unwrap();
        fs::create_dir_all(ws.join("kart-06b3").join("v2")).unwrap();
        let (n2, p2) = import("kart-06b3", &src, &ws, t).unwrap();
        assert_eq!(n2, 3);
        let vj: Value = serde_json::from_slice(&fs::read(p2.join("version.json")).unwrap()).unwrap();
        assert_eq!((vj["version"].as_u64(), vj["parent"].as_u64()), (Some(3), Some(1)), "parent = the version this writer started from (8.3)");
        let left: Vec<String> = fs::read_dir(ws.join("kart-06b3")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left.iter().filter(|n| n.contains(".tmp-") && n.ends_with(&std::process::id().to_string())).count(), 0, "{left:?}");        let _ = fs::remove_dir_all(&d);
    }
}
