//! The native `bake.export` planner (bridge side): bundle + files on disk ->
//! files to write. Pure: the caller reads and writes.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::LiveError;

/// One file `bake.export` writes.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedFile {
    /// Relative to the engine root, `/`-separated.
    pub path: String,
    /// `created` or `updated`.
    pub action: &'static str,
    pub contents: String,
}

/// What [`plan`] decided: files to write and files that already match.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BakePlan {
    pub files: Vec<PlannedFile>,
    pub unchanged: Vec<String>,
    /// Path of CHANGES.md when it gets a new section.
    pub changes_md: Option<String>,
    pub from_tx: u64,
    pub to_tx: u64,
    pub game: String,
    pub dry_run: bool,
}

impl BakePlan {
    /// The 6.23 result; `dry_run` adds `preview` (the full new contents).
    pub fn to_result(&self) -> Value {
        let written: Vec<Value> = self
            .files
            .iter()
            .map(|f| {
                let mut o = json!({ "path": f.path, "action": f.action,
                                    "bytes": f.contents.len() });
                if self.dry_run {
                    o["preview"] = json!(f.contents);
                }
                o
            })
            .collect();
        let game = if self.game.is_empty() {
            "<game>"
        } else {
            &self.game
        };
        json!({
            "written": written,
            "unchanged": self.unchanged,
            "changes_md": self.changes_md,
            "from_tx": self.from_tx,
            "to_tx": self.to_tx,
            "dry_run": self.dry_run,
            "rebuild": format!("cargo xtask build {game} --ship reproduces this look"),
        })
    }
}

fn bad(msg: impl Into<String>) -> LiveError {
    LiveError::new("bad_request", msg)
}

fn io_err(path: &str, msg: impl Into<String>) -> LiveError {
    LiveError::new("io_error", msg).with(json!({ "path": path }))
}

/// A file name part: `[a-z0-9_.-]`, not empty, not starting with `.`.
pub fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && !s.contains("..")
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')
        })
}

/// A relative path bake may write: `looks/<crate>/…`, `kits/<crate>/…` or
/// `assets/tiers/…`, every part a [`safe_name`].
pub fn allowed_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.iter().any(|p| !safe_name(p)) {
        return false;
    }
    match parts.as_slice() {
        ["looks", c, rest @ ..] => c.starts_with("evani-look-") && !rest.is_empty(),
        ["kits", c, rest @ ..] => c.starts_with("evani-kit-") && !rest.is_empty(),
        ["assets", "tiers", f] => f.ends_with(".json"),
        _ => false,
    }
}

enum Edit {
    Merge(Map<String, Value>),
    Text(String),
    Json(Value),
}

/// JSON the way the engine's data files look: objects one key per line
/// (sorted), arrays of scalars inline.
pub fn pretty_json(v: &Value) -> String {
    let mut s = String::new();
    write_json(v, 0, &mut s);
    s.push('\n');
    s
}

fn write_json(v: &Value, depth: usize, out: &mut String) {
    let pad = |d: usize| "  ".repeat(d);
    match v {
        Value::Object(m) if !m.is_empty() => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push_str(": ");
                if let Some(x) = m.get(*k) {
                    write_json(x, depth + 1, out);
                }
                if i + 1 < keys.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
        Value::Array(a) if a.iter().any(|x| x.is_array() || x.is_object()) => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                write_json(x, depth + 1, out);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&x.to_string());
            }
            out.push(']');
        }
        v => out.push_str(&v.to_string()),
    }
}

fn str_field<'a>(v: &'a Value, k: &str) -> Result<&'a str, LiveError> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("bundle: {k} missing")))
}

fn list<'a>(b: &'a Value, k: &str) -> &'a [Value] {
    b.get(k)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Turn a `bake.export` bundle into the files to write.
///
/// `read(path)` returns a file's current contents (paths relative to the
/// engine root, `/`-separated) or `None` if it does not exist. `now_utc` is
/// the CHANGES.md heading time, e.g. `2026-10-13 14:35`.
pub fn plan(
    bundle: &Value,
    read: impl Fn(&str) -> Option<String>,
    now_utc: &str,
) -> Result<BakePlan, LiveError> {
    let crate_of = |target: &str| -> Result<String, LiveError> {
        let (key, prefix, dir) = match target {
            "look" | "policy" => ("look_crate", "evani-look-", "looks"),
            "kit" => ("kit_crate", "evani-kit-", "kits"),
            t => return Err(bad(format!("bundle: unknown bake target {t}"))),
        };
        let c = bundle
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| bad(format!("this game has no {target} crate to bake into")))?;
        if !c.starts_with(prefix) || !safe_name(c) {
            return Err(bad(format!("bad {target} crate name {c}")));
        }
        Ok(format!("{dir}/{c}"))
    };
    let checked = |name: &str| -> Result<(), LiveError> {
        if safe_name(name) {
            Ok(())
        } else {
            Err(bad(format!(
                "{name:?} is not a safe file name ([a-z0-9_.-])"
            )))
        }
    };

    let mut edits: BTreeMap<String, Edit> = BTreeMap::new();
    let merge = |edits: &mut BTreeMap<String, Edit>, path: String, k: &str, v: Value| {
        if let Edit::Merge(m) = edits.entry(path).or_insert_with(|| Edit::Merge(Map::new())) {
            m.insert(k.to_string(), v);
        }
    };

    for p in list(bundle, "params") {
        let name = str_field(p, "name")?;
        let target = str_field(p, "bake")?;
        let value = p.get("value").cloned().unwrap_or(Value::Null);
        let file = if target == "policy" {
            "policy.json"
        } else {
            "params.json"
        };
        let path = format!("{}/{file}", crate_of(target)?);
        merge(&mut edits, path, name, value);
    }
    for s in list(bundle, "slots") {
        let name = str_field(s, "name")?;
        checked(name)?;
        let source = str_field(s, "source")?;
        let (ext, text) = match str_field(s, "lang")? {
            "wgsl" => ("wgsl", source.to_string()),
            // Expression files are read with trailing whitespace trimmed.
            "expr" => ("expr", format!("{}\n", source.trim_end())),
            l => return Err(bad(format!("bundle: unknown slot language {l}"))),
        };
        let path = format!("{}/slots/{name}.{ext}", crate_of(str_field(s, "target")?)?);
        edits.insert(path, Edit::Text(text));
    }
    for c in list(bundle, "curves") {
        let name = str_field(c, "name")?;
        checked(name)?;
        let points = c.get("points").cloned().unwrap_or(Value::Null);
        let path = format!("{}/curves/{name}.json", crate_of(str_field(c, "target")?)?);
        edits.insert(path, Edit::Json(json!({ "points": points })));
    }
    for t in list(bundle, "tiers") {
        let id = str_field(t, "id")?;
        checked(id)?;
        let path = format!("assets/tiers/{id}.json");
        if let Some(set) = t.get("set").and_then(Value::as_object) {
            for (k, v) in set {
                merge(&mut edits, path.clone(), k, v.clone());
            }
        }
    }

    let mut out = BakePlan {
        from_tx: bundle.get("from_tx").and_then(Value::as_u64).unwrap_or(0),
        to_tx: bundle.get("to_tx").and_then(Value::as_u64).unwrap_or(0),
        game: bundle
            .get("game")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        dry_run: bundle
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ..BakePlan::default()
    };

    for (path, edit) in edits {
        if !allowed_path(&path) {
            return Err(bad(format!("refusing to write {path}")));
        }
        let old = read(&path);
        let parse_old = |o: &str| -> Result<Value, LiveError> {
            serde_json::from_str::<Value>(o)
                .map_err(|e| io_err(&path, format!("{path} is not valid JSON: {e}")))
        };
        let new = match edit {
            Edit::Text(t) => (old.as_deref() != Some(t.as_str())).then_some(t),
            Edit::Json(v) => {
                let same = match &old {
                    Some(o) => parse_old(o)? == v,
                    None => false,
                };
                (!same).then(|| pretty_json(&v))
            }
            Edit::Merge(m) => {
                let base = match &old {
                    Some(o) => parse_old(o)?,
                    None => Value::Object(Map::new()),
                };
                let Value::Object(mut obj) = base.clone() else {
                    return Err(io_err(&path, format!("{path} is not a JSON object")));
                };
                for (k, v) in m {
                    obj.insert(k, v);
                }
                let merged = Value::Object(obj);
                (merged != base || old.is_none()).then(|| pretty_json(&merged))
            }
        };
        match new {
            Some(contents) => out.files.push(PlannedFile {
                action: if old.is_some() { "updated" } else { "created" },
                path,
                contents,
            }),
            None => out.unchanged.push(path),
        }
    }

    if !out.files.is_empty() {
        let base = match bundle.get("look_crate").and_then(Value::as_str) {
            Some(_) => crate_of("look")?,
            None => crate_of("kit")?,
        };
        let path = format!("{base}/CHANGES.md");
        let title = bundle
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Live session");
        let txs = if out.from_tx == out.to_tx {
            format!("tx {}", out.to_tx)
        } else {
            format!("tx {}–{}", out.from_tx, out.to_tx)
        };
        let mut section = format!("## {now_utc} UTC — {title} ({txs})\n\n");
        for n in list(bundle, "notes").iter().filter_map(Value::as_str) {
            section.push_str("- ");
            section.push_str(n);
            section.push('\n');
        }
        section.push('\n');
        let old = read(&path);
        let contents = insert_section(old.as_deref(), &section);
        out.files.push(PlannedFile {
            action: if old.is_some() { "updated" } else { "created" },
            path: path.clone(),
            contents,
        });
        out.changes_md = Some(path);
    }
    Ok(out)
}

/// Put a new `## ` section first: after the file's `# ` title block, before
/// the previous newest section.
fn insert_section(old: Option<&str>, section: &str) -> String {
    let Some(old) = old.filter(|o| !o.trim().is_empty()) else {
        return format!(
            "# Changes\n\nTuned live and written by bake.export, newest first.\n\n{section}"
        );
    };
    // Write the new lines in the file's own line-ending style (CRLF after a
    // git checkout on Windows), so the file never ends up mixed.
    let crlf = old.contains("\r\n");
    let section = if crlf {
        section.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        section.to_string()
    };
    let eol = if crlf { "\r\n" } else { "\n" };
    let at = if old.starts_with("## ") {
        Some(0)
    } else {
        old.find("\n## ").map(|i| i + 1)
    };
    match at {
        Some(i) => format!("{}{section}{}", &old[..i], &old[i..]),
        None => {
            let mut s = old.trim_end().to_string();
            s.push_str(eol);
            s.push_str(eol);
            s.push_str(&section);
            s
        }
    }
}

/// `YYYY-MM-DD HH:MM` (UTC) for the CHANGES.md heading.
pub fn heading_time(iso: &str) -> String {
    // now_iso() is `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    let s: String = iso.chars().take(16).collect();
    s.replace('T', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn bundle() -> Value {
        json!({
            "title": "Sunset", "dry_run": false, "from_tx": 1, "to_tx": 4, "game": "sandbox",
            "look_crate": "evani-look-basic", "kit_crate": "evani-kit-example",
            "params": [
                { "name": "look.basic.saturation", "bake": "look", "value": 1.2 },
                { "name": "kit.example.spin_speed", "bake": "kit", "value": 3.0 },
                { "name": "aq.hysteresis_pct", "bake": "policy", "value": 15.0 }
            ],
            "slots": [{ "name": "tonemap", "target": "look", "lang": "wgsl", "source": "return c;" }],
            "curves": [{ "name": "kit.example.spin_falloff", "target": "kit",
                         "points": [[0.5, 1.0], [1.5, 2.0]] }],
            "tiers": [{ "id": "medium", "set": { "effects_on": ["bloom"] } }],
            "notes": ["**look.basic.saturation** 1 → 1.2 — test *(AI)*"]
        })
    }

    fn reader(files: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        move |p| files.get(p).cloned()
    }

    #[test]
    fn merge_keeps_other_keys_and_sorts() {
        let mut files = HashMap::new();
        files.insert(
            "looks/evani-look-basic/params.json".to_string(),
            "{\"zz.other\": 2, \"look.basic.saturation\": 1.0}".to_string(),
        );
        files.insert(
            "assets/tiers/medium.json".to_string(),
            "{\"id\":\"medium\",\"effects_on\":[\"vignette\",\"bloom\"],\"fps_max\":null}"
                .to_string(),
        );
        let p = plan(&bundle(), reader(&files), "2026-10-13 14:35").unwrap();
        let f = p
            .files
            .iter()
            .find(|f| f.path == "looks/evani-look-basic/params.json")
            .unwrap();
        assert_eq!(f.action, "updated");
        assert_eq!(
            f.contents,
            "{\n  \"look.basic.saturation\": 1.2,\n  \"zz.other\": 2\n}\n"
        );
        let t = p
            .files
            .iter()
            .find(|f| f.path == "assets/tiers/medium.json")
            .unwrap();
        let v: Value = serde_json::from_str(&t.contents).unwrap();
        assert_eq!(v["effects_on"], json!(["bloom"]));
        assert_eq!(v["fps_max"], Value::Null);
        assert_eq!(v["id"], "medium");
        let paths: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
        for want in [
            "kits/evani-kit-example/params.json",
            "looks/evani-look-basic/policy.json",
            "looks/evani-look-basic/slots/tonemap.wgsl",
            "kits/evani-kit-example/curves/kit.example.spin_falloff.json",
            "looks/evani-look-basic/CHANGES.md",
        ] {
            assert!(paths.contains(&want), "{want} in {paths:?}");
        }
        assert_eq!(
            p.changes_md.as_deref(),
            Some("looks/evani-look-basic/CHANGES.md")
        );
        let r = p.to_result();
        assert_eq!(r["from_tx"], 1);
        assert!(r["written"][0].get("preview").is_none());
    }

    #[test]
    fn unchanged_files_are_not_written() {
        let first = plan(&bundle(), |_| None, "2026-10-13 14:35").unwrap();
        assert!(first.files.iter().all(|f| f.action == "created"));
        let files: HashMap<String, String> = first
            .files
            .iter()
            .map(|f| (f.path.clone(), f.contents.clone()))
            .collect();
        let second = plan(&bundle(), reader(&files), "2026-10-13 15:00").unwrap();
        assert!(second.files.is_empty(), "{:?}", second.files);
        assert!(second.changes_md.is_none());
        assert_eq!(second.unchanged.len(), first.files.len() - 1);
        // Same values, different formatting: still unchanged.
        let mut files = files;
        files.insert(
            "kits/evani-kit-example/curves/kit.example.spin_falloff.json".into(),
            "{\"points\":[[0.5,1.0],[1.5,2.0]]}".into(),
        );
        let third = plan(&bundle(), reader(&files), "x").unwrap();
        assert!(third.files.is_empty());
    }

    #[test]
    fn refuses_unsafe_paths_and_names() {
        for name in [
            "../x",
            "/etc/passwd",
            "C:\\x",
            "Tonemap",
            "a/b",
            "",
            ".hidden",
            "a..b",
        ] {
            let mut b = bundle();
            b["slots"][0]["name"] = json!(name);
            let e = plan(&b, |_| None, "t").unwrap_err();
            assert_eq!(e.code, "bad_request", "{name}");
        }
        for c in ["../evil", "evani-look-../x", "other-crate", "/abs"] {
            let mut b = bundle();
            b["look_crate"] = json!(c);
            assert!(plan(&b, |_| None, "t").is_err(), "{c}");
        }
        let mut b = bundle();
        b["tiers"][0]["id"] = json!("../../secret");
        assert!(plan(&b, |_| None, "t").is_err());
        assert!(allowed_path("looks/evani-look-basic/params.json"));
        assert!(allowed_path("assets/tiers/low.json"));
        assert!(!allowed_path("assets/other/low.json"));
        assert!(!allowed_path("looks/evani-look-basic/../x"));
        assert!(!allowed_path("src/main.rs"));
    }

    #[test]
    fn changes_md_newest_first() {
        let mut files = HashMap::new();
        files.insert(
            "looks/evani-look-basic/CHANGES.md".to_string(),
            "# Changes\n\nIntro.\n\n## 2026-10-01 10:00 — Old pass (tx 1)\n\n- old\n".to_string(),
        );
        let p = plan(&bundle(), reader(&files), "2026-10-13 14:35").unwrap();
        let md = &p
            .files
            .iter()
            .find(|f| f.path.ends_with("CHANGES.md"))
            .unwrap()
            .contents;
        let new_at = md.find("## 2026-10-13 14:35 UTC — Sunset (tx 1–4)").unwrap();
        let old_at = md.find("## 2026-10-01 10:00").unwrap();
        assert!(md.starts_with("# Changes\n\nIntro.\n\n"));
        assert!(new_at < old_at);
        assert!(md.contains("- **look.basic.saturation** 1 → 1.2 — test *(AI)*\n"));
        let fresh = insert_section(None, "## a\n\n- x\n\n");
        assert!(fresh.starts_with("# Changes\n"));
        assert_eq!(heading_time("2026-10-13T14:35:09.123Z"), "2026-10-13 14:35");
    }

    #[test]
    fn changes_md_keeps_crlf_endings() {
        let old = "# Changes\r\n\r\nIntro.\r\n\r\n## 2026-10-01 10:00 — Old (tx 1)\r\n\r\n- old\r\n";
        let md = insert_section(Some(old), "## new\n\n- x\n\n");
        assert!(md.contains("## new\r\n\r\n- x\r\n\r\n## 2026-10-01"));
        assert_eq!(md.matches('\n').count(), md.matches("\r\n").count());
        let bare = insert_section(Some("# Changes\r\n\r\nIntro.\r\n"), "## n\n\n- y\n\n");
        assert_eq!(bare.matches('\n').count(), bare.matches("\r\n").count());
        let lf = insert_section(Some("# Changes\n\n## a\n\n- z\n"), "## n\n\n- y\n\n");
        assert!(!lf.contains('\r'));
    }

    #[test]
    fn dry_run_adds_preview() {
        let mut b = bundle();
        b["dry_run"] = json!(true);
        let r = plan(&b, |_| None, "t").unwrap().to_result();
        assert!(r["written"][0]["preview"].is_string());
        assert_eq!(r["dry_run"], true);
    }
}
