//! The look guard's append-only audit log (docs/LIVE_API.md 15.8):
//! `.evani/look-guard/audit.jsonl` in the project, one JSON object per line.
//!
//! Each line carries `n` (1, 2, …), `at` (UTC), `event`, its data, and
//! `prev`: the SHA-256 of the previous line's bytes (64 zeros for the
//! first). Editing or deleting a line breaks the chain, which
//! [`verify_chain`] reports. (Anyone who can write the file can rewrite the
//! whole chain; the log makes quiet edits visible, it is not a vault. The
//! real guard is the signed record and the bake check.)

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

pub fn path(root: &Path) -> PathBuf {
    root.join(".evani").join("look-guard").join("audit.jsonl")
}

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Append one event. `data` must be a JSON object; `n`, `at`, `event` and
/// `prev` are added. Returns the line written.
pub fn append(root: &Path, event: &str, data: Value, now_ms: u64) -> Result<String, String> {
    let p = path(root);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let existing = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    let last = existing.lines().rev().find(|l| !l.trim().is_empty());
    let (n, prev) = match last {
        None => (1, ZERO.to_string()),
        Some(l) => {
            let v: Value = serde_json::from_str(l).unwrap_or(Value::Null);
            (v["n"].as_u64().unwrap_or(0) + 1, crate::sha256_hex(l.as_bytes()))
        }
    };
    let mut m = Map::new();
    m.insert("n".into(), json!(n));
    m.insert("at".into(), json!(crate::iso_utc_ms(now_ms)));
    m.insert("event".into(), json!(event));
    if let Value::Object(d) = data {
        for (k, v) in d {
            if !matches!(k.as_str(), "n" | "at" | "event" | "prev") {
                m.insert(k, v);
            }
        }
    }
    m.insert("prev".into(), json!(prev));
    let line = Value::Object(m).to_string();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    // A file that doesn't end in a newline gets one first.
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    f.write_all(format!("{sep}{line}\n").as_bytes()).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(line)
}

/// Check the chain. `Ok(number of lines)`, or the first broken line.
pub fn verify_chain(text: &str) -> Result<usize, String> {
    let mut prev = ZERO.to_string();
    let mut n = 0u64;
    for (i, l) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let v: Value = serde_json::from_str(l).map_err(|_| format!("line {} is not JSON", i + 1))?;
        n += 1;
        if v["n"].as_u64() != Some(n) {
            return Err(format!("line {} has n {} (expected {n}): a line was removed or added", i + 1, v["n"]));
        }
        if v["prev"].as_str() != Some(prev.as_str()) {
            return Err(format!("line {} doesn't chain to the line before: the log was edited", i + 1));
        }
        prev = crate::sha256_hex(l.as_bytes());
    }
    Ok(n as usize)
}

/// The events in the log, oldest first (for tests and `look.status`).
pub fn read(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(path(root))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chained_and_tamper_evident() {
        let root = std::env::temp_dir().join(format!("evani-lg-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        append(&root, "proposed", json!({ "look": "evani-look-basic", "approvalId": "la-1" }), 1_760_000_000_000).unwrap();
        append(&root, "approved", json!({ "approvalId": "la-1", "n": 99 }), 1_760_000_001_000).unwrap();
        append(&root, "baked", json!({ "look": "evani-look-basic" }), 1_760_000_002_000).unwrap();
        let text = std::fs::read_to_string(path(&root)).unwrap();
        assert_eq!(verify_chain(&text).unwrap(), 3);
        let ev = read(&root);
        assert_eq!(ev[1]["event"], "approved");
        assert_eq!(ev[1]["n"], 2, "callers can't set n");
        assert_eq!(ev[0]["at"], "2025-10-09T08:53:20Z");
        // Edit a line: the chain breaks at the next one.
        let edited = text.replacen("la-1", "la-2", 1);
        assert!(verify_chain(&edited).unwrap_err().starts_with("line 2"));
        // Drop a line.
        let dropped: Vec<&str> = text.lines().enumerate().filter(|(i, _)| *i != 1).map(|(_, l)| l).collect();
        assert!(verify_chain(&dropped.join("\n")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
