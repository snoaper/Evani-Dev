//! `cargo xtask library build|check`: the Evani Library build (docs/EVANI_COMMONS.md
//! section 8, "The Evani Library build").
//!
//! The library Evani.App opens from its Evani.dev tab ("Open material library") is the
//! material viewer (`apps/material-view`) built for the web, with every asset compiled
//! in. `library build` makes it in the form Evani.App ships and opens: the viewer's ship
//! build (`build material-view --ship`: no live module, no dev-only code, the ship guard,
//! build facts and the engine notice), copied into `--out`, plus `library-contents.json`,
//! the version stamp: which engine commit it was built on, which Evani.commons versions
//! it carries (`asset@style#hash12`, the refs Evani.App's library manifest uses) and
//! every file with its size, SHA-256 and download stage. `library check <dir>` proves a
//! folder has that shape, so a broken build is refused before anyone ships it.
//!
//! Evani.App's library builder (its part A2) runs `commons import --no-check` for each
//! approved version in a worktree at the base commit, then `library build --out <dir>`,
//! hashes, signs and uploads the folder. This command never uploads, signs or publishes.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};

/// The viewer the library is built from.
pub const APP: &str = "material-view";
/// The version stamp beside index.html.
pub const CONTENTS_FILE: &str = "library-contents.json";
pub const CONTENTS_FORMAT: &str = "evani-library-contents/1";
/// The page Evani.App opens.
pub const ENTRY: &str = "index.html";
/// Where Evani.App looks for a built library on this PC (its `materialsSubdirs`, first
/// choice): the default `--out`.
pub const DEFAULT_OUT: [&str; 3] = [".evani", "serve", APP];

// Evani.App's limits for a library (platform/protocol/library.ts LIBRARY_LIMITS): it
// refuses a library past any of them, so the build is refused here first.
pub const MAX_FILES: usize = 400;
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PATH_CHARS: usize = 200;

pub fn usage() -> &'static str {
    "  cargo xtask library build [--out <folder>] [--modules]   (the Evani Library: material-view's ship build + library-contents.json, checked; default --out .evani/serve/material-view, where Evani.App opens it; --modules adds the per-module size report, about 80 more builds)\n  cargo xtask library check <folder>   (a library folder's shape: files, version stamp, hashes, Evani.App's limits, no dev-only code)"
}

/// What `cmd_library` needs from the rest of xtask (main.rs gives the real ones; tests
/// give stand-ins).
pub struct Tools<'a> {
    /// The viewer's ship build (`modules`: with the per-module size report); returns its folder.
    pub build: &'a dyn Fn(bool) -> Result<PathBuf, String>,
    /// The ship guard on a built folder (no live module, no dev-only code); returns a
    /// one-line code size summary.
    pub guard: &'a dyn Fn(&Path) -> Result<String, String>,
}

pub fn cmd_library(root: &Path, args: &[String], tools: &Tools) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("build") => {
            let out = match value(rest, "--out")? {
                Some(o) => absolute(Path::new(&o))?,
                None => DEFAULT_OUT.iter().fold(root.to_path_buf(), |p, s| p.join(s)),
            };
            let started = Instant::now();
            let s = build(root, &out, rest.iter().any(|a| a == "--modules"), tools)?;
            println!("{}", s.report());
            println!("library build: {} ({}, build {}), {:.0} s", out.display(), mb(s.total), s.build, started.elapsed().as_secs_f64());
            Ok(())
        }
        Some("check") => {
            let dir = rest.iter().find(|a| !a.starts_with("--")).ok_or("library check <folder>")?;
            let dir = absolute(Path::new(dir))?;
            let mut s = check_dir(&dir)?;
            s.code = (tools.guard)(&dir)?;
            println!("{}", s.report());
            println!("library check: {} is a library build ({}, build {})", dir.display(), mb(s.total), s.build);
            Ok(())
        }
        _ => Err(format!("usage:\n{}", usage())),
    }
}

fn value(args: &[String], k: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == k) {
        None => Ok(None),
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().map(Some).ok_or_else(|| format!("{k} needs a folder")),
    }
}

fn absolute(p: &Path) -> Result<PathBuf, String> {
    std::path::absolute(p).map_err(|e| format!("{}: {e}", p.display()))
}

// ---------- which Evani.commons versions the build carries ----------

/// The versions a build carries, as `asset@style#hash12` refs (sorted).
#[derive(Debug, Default, PartialEq)]
pub struct Lists {
    /// Imported onto the base for this build, with code changes.
    pub included: Vec<String>,
    /// Imported for this build, but the base already had their code.
    pub already: Vec<String>,
    /// Adopted in the base commit itself (catalog/procedural/commons.json at HEAD).
    pub in_base: Vec<String>,
}

/// `asset@style#hash12` for one record of catalog/procedural/commons.json's `adopted`.
pub fn version_ref(key: &str, rec: &Value) -> Result<String, String> {
    let id = rec["version_id"].as_str().filter(|v| is_hex(v, 12)).map(str::to_string).or_else(|| {
        rec["content_hash"].as_str().and_then(|h| h.strip_prefix("sha256:")).filter(|h| is_hex(h, 64)).map(|h| h[..12].to_string())
    });
    let r = format!("{key}#{}", id.ok_or_else(|| format!("catalog/procedural/commons.json: `{key}` has no 12-hex version_id or sha256 content_hash"))?);
    if valid_ref(&r) { Ok(r) } else { Err(format!("catalog/procedural/commons.json: `{key}` is not an asset@style key")) }
}

/// The record format of `commons import`: `{ adopted: { "asset@style": { version_id,
/// content_hash, files, .. } } }`. `working` is the checkout's file, `head` the base
/// commit's (None: no file). A record unchanged since the base is `in_base`; a new or
/// changed one is `included`, or `already` when the import changed no file.
pub fn lists(working: Option<&str>, head: Option<&str>) -> Result<Lists, String> {
    let parse = |t: Option<&str>, what: &str| -> Result<serde_json::Map<String, Value>, String> {
        let Some(t) = t else { return Ok(Default::default()) };
        let v: Value = serde_json::from_str(t).map_err(|e| format!("catalog/procedural/commons.json ({what}): {e}"))?;
        Ok(v["adopted"].as_object().cloned().unwrap_or_default())
    };
    let (now, base) = (parse(working, "this checkout")?, parse(head, "the base commit")?);
    let mut l = Lists::default();
    for (key, rec) in &now {
        let r = version_ref(key, rec)?;
        let same = base.get(key).is_some_and(|b| b["content_hash"] == rec["content_hash"] && b["version_id"] == rec["version_id"]);
        let changed = rec["files"].as_array().is_some_and(|f| !f.is_empty());
        match (same, changed) {
            (true, _) => l.in_base.push(r),
            (false, true) => l.included.push(r),
            (false, false) => l.already.push(r),
        }
    }
    for v in [&mut l.included, &mut l.already, &mut l.in_base] {
        v.sort();
    }
    Ok(l)
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// [`lists`] for the checkout at `root` (its commons.json against the one at HEAD).
pub fn lists_in(root: &Path) -> Result<Lists, String> {
    let path = root.join("catalog").join("procedural").join("commons.json");
    let working = std::fs::read_to_string(&path).ok();
    let head = git(root, &["show", "HEAD:catalog/procedural/commons.json"]);
    lists(working.as_deref(), head.as_deref())
}

/// The full commit the checkout at `root` is on.
pub fn base_commit(root: &Path) -> Result<String, String> {
    git(root, &["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| is_hex(s, 40))
        .ok_or_else(|| format!("{}: not a git checkout, so the library's base commit is unknown\n  fix: build the library in a git checkout (or worktree) of the engine at its base commit", root.display()))
}

// ---------- files ----------

#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub stage: u8,
}

/// The download stage Evani.App gives a file (platform/protocol/library.ts): 1 to open
/// the library (html, js, wasm, snippets, build facts, the stamp), 2 models, 3 pictures.
pub fn stage_of(path: &str) -> u8 {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("models/") {
        2
    } else if lower.starts_with("proc-refs/") || [".jpg", ".jpeg", ".png", ".webp"].iter().any(|x| lower.ends_with(x)) {
        3
    } else {
        1
    }
}

/// A path Evani.App accepts: `[A-Za-z0-9._-]` parts joined by `/`, no `.` or `..`
/// part, at most 200 characters (its `safeLibraryPath`).
pub fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= MAX_PATH_CHARS
        && p.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && part.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
}

pub fn valid_ref(r: &str) -> bool {
    let slug = |s: &str, max: usize| {
        let b = s.as_bytes();
        (2..=max + 1).contains(&b.len()) && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    };
    let Some((id, h)) = r.split_once('#') else { return false };
    let Some((asset, style)) = id.split_once('@') else { return false };
    slug(asset, 60) && slug(style, 30) && is_hex(h, 12)
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Every file under `dir` but the stamp itself, sorted by path (`/`-separated).
pub fn list_files(dir: &Path) -> Result<Vec<FileEntry>, String> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = e.map_err(|e| format!("{}: {e}", d.display()))?.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            let rel = p.strip_prefix(dir).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
            if rel == CONTENTS_FILE {
                continue;
            }
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.push(FileEntry { stage: stage_of(&rel), path: rel, size: bytes.len() as u64, sha256: hex(&Sha256::digest(&bytes)) });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The build's name: 12 hex of the SHA-256 over its file list (`<sha256>  <path>` lines),
/// so the same files always give the same name and any changed byte a new one.
pub fn build_id(files: &[FileEntry]) -> String {
    let mut h = Sha256::new();
    for f in files {
        h.update(format!("{}  {}\n", f.sha256, f.path).as_bytes());
    }
    hex(&h.finalize())[..12].to_string()
}

// ---------- the stamp ----------

pub struct Stamp<'a> {
    pub base: &'a str,
    pub engine_version: &'a str,
    pub built_at: &'a str,
}

/// Writes [`CONTENTS_FILE`] into `dir` (after every other file is in place).
pub fn write_contents(dir: &Path, stamp: &Stamp, l: &Lists) -> Result<Value, String> {
    let files = list_files(dir)?;
    let total: u64 = files.iter().map(|f| f.size).sum();
    let v = json!({
        "format": CONTENTS_FORMAT,
        "about": "Written by `cargo xtask library build` (docs/EVANI_COMMONS.md section 8). The Evani Library's version stamp: the engine commit it was built on, the Evani.commons versions it carries and every file with its size, SHA-256 and download stage. Not loaded by the viewer.",
        "app": APP,
        "build": build_id(&files),
        "base": stamp.base,
        "engine": { "name": "Evani.Dev", "version": stamp.engine_version },
        "built_at": stamp.built_at,
        "entry": ENTRY,
        "included": l.included,
        "already": l.already,
        "in_base": l.in_base,
        "files": files.iter().map(|f| json!({ "path": f.path, "size": f.size, "sha256": f.sha256, "stage": f.stage })).collect::<Vec<_>>(),
        "total": { "files": files.len() + 1, "size": total },
    });
    let text = format!("{}\n", serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?);
    std::fs::write(dir.join(CONTENTS_FILE), text).map_err(|e| format!("{CONTENTS_FILE}: {e}"))?;
    Ok(v)
}

// ---------- the shape check ----------

#[derive(Debug, Default)]
pub struct Summary {
    pub build: String,
    pub base: String,
    pub files: usize,
    pub total: u64,
    /// Bytes by part: code (stage 1), models (stage 2), pictures (stage 3).
    pub stages: [u64; 3],
    pub included: usize,
    pub already: usize,
    pub in_base: usize,
    /// The ship guard's code size line.
    pub code: String,
}

impl Summary {
    pub fn report(&self) -> String {
        let mut s = format!(
            "library {} on engine {}:\n  {} files, {} on disk\n  stage 1 (to open: page, code, stamp) {}\n  stage 2 (models)                      {}\n  stage 3 (pictures)                    {}\n  Evani.commons versions: {} included, {} already in the base, {} adopted in the base commit",
            self.build,
            &self.base[..12.min(self.base.len())],
            self.files,
            mb(self.total),
            mb(self.stages[0]),
            mb(self.stages[1]),
            mb(self.stages[2]),
            self.included,
            self.already,
            self.in_base
        );
        if !self.code.is_empty() {
            s.push_str(&format!("\n  {}", self.code));
        }
        s
    }
}

fn mb(b: u64) -> String {
    format!("{:.2} MB", b as f64 / (1024.0 * 1024.0))
}

/// Is `dir` a library build Evani.App can take? Every problem is listed in the error.
pub fn check_dir(dir: &Path) -> Result<Summary, String> {
    let mut bad: Vec<String> = Vec::new();
    let files = list_files(dir)?;
    let has = |p: &str| files.iter().any(|f| f.path == p);
    for p in [ENTRY, crate::build_facts::FACTS_FILE, evani_guide::playground::ENGINE_NOTICE_FILE] {
        if !has(p) {
            bad.push(format!("{p} is missing"));
        }
    }
    let wasm: Vec<&FileEntry> = files.iter().filter(|f| !f.path.contains('/') && f.path.ends_with("_bg.wasm")).collect();
    match wasm.as_slice() {
        [w] => {
            let js = format!("{}.js", w.path.trim_end_matches("_bg.wasm"));
            if !has(&js) {
                bad.push(format!("{js} (the viewer's loader for {}) is missing", w.path));
            }
        }
        [] => bad.push("the viewer's *_bg.wasm is missing".into()),
        many => bad.push(format!("more than one viewer wasm ({})", many.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().join(", "))),
    }
    // The ship build's own statement: the viewer, built without live tuning.
    if let Ok(t) = std::fs::read_to_string(dir.join(crate::build_facts::FACTS_FILE)) {
        match serde_json::from_str::<Value>(&t) {
            Ok(f) => {
                if f["game"]["id"] != APP {
                    bad.push(format!("build-facts.json is not the {APP} build (game.id {})", f["game"]["id"]));
                }
                if f["modules"]["live_tuning"] != false {
                    bad.push("build-facts.json says live tuning is on: not a ship build".into());
                }
            }
            Err(e) => bad.push(format!("build-facts.json: {e}")),
        }
    }
    // Evani.App's limits.
    if files.len() + 1 > MAX_FILES {
        bad.push(format!("{} files, over Evani.App's {MAX_FILES}", files.len() + 1));
    }
    for f in &files {
        if !safe_path(&f.path) {
            bad.push(format!("{}: a path Evani.App refuses (letters, digits, . _ - and /, at most {MAX_PATH_CHARS} characters)", f.path));
        }
        if f.size > MAX_FILE_BYTES {
            bad.push(format!("{}: {}, over Evani.App's {} a file", f.path, mb(f.size), mb(MAX_FILE_BYTES)));
        }
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    // The stamp.
    let mut s = Summary { files: files.len() + 1, ..Default::default() };
    match std::fs::read_to_string(dir.join(CONTENTS_FILE)).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
        Err(e) => bad.push(format!("{CONTENTS_FILE}: {e}")),
        Ok(c) => {
            let text = |k: &str| c[k].as_str().unwrap_or("").to_string();
            if c["format"] != CONTENTS_FORMAT {
                bad.push(format!("{CONTENTS_FILE}: format is not {CONTENTS_FORMAT}"));
            }
            if c["app"] != APP {
                bad.push(format!("{CONTENTS_FILE}: app is not {APP}"));
            }
            if c["entry"] != ENTRY {
                bad.push(format!("{CONTENTS_FILE}: entry is not {ENTRY}"));
            }
            s.base = text("base");
            if !is_hex(&s.base, 40) {
                bad.push(format!("{CONTENTS_FILE}: base is not a 40-hex commit"));
            }
            if !iso(&text("built_at")) {
                bad.push(format!("{CONTENTS_FILE}: built_at is not a UTC time like 2026-11-10T19:02:11Z"));
            }
            if c["engine"]["version"].as_str().is_none_or(str::is_empty) {
                bad.push(format!("{CONTENTS_FILE}: engine.version is missing"));
            }
            s.build = text("build");
            let want = build_id(&files);
            if s.build != want {
                bad.push(format!("{CONTENTS_FILE}: build {} does not name these files (they are build {want})", s.build));
            }
            let listed: Vec<FileEntry> = c["files"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|f| FileEntry {
                            path: f["path"].as_str().unwrap_or("").into(),
                            size: f["size"].as_u64().unwrap_or(u64::MAX),
                            sha256: f["sha256"].as_str().unwrap_or("").into(),
                            stage: f["stage"].as_u64().and_then(|n| u8::try_from(n).ok()).unwrap_or(0),
                        })
                        .collect()
                })
                .unwrap_or_default();
            if listed != files {
                let (a, b): (BTreeSet<_>, BTreeSet<_>) = (listed.iter().map(|f| &f.path).collect(), files.iter().map(|f| &f.path).collect());
                let missing: Vec<_> = a.difference(&b).collect();
                let extra: Vec<_> = b.difference(&a).collect();
                let changed: Vec<_> = listed.iter().filter(|l| files.iter().any(|f| f.path == l.path && f != *l)).map(|f| &f.path).collect();
                bad.push(format!("{CONTENTS_FILE}: its file list does not match the folder (listed but missing {missing:?}, not listed {extra:?}, changed {changed:?})"));
            }
            if c["total"]["size"].as_u64() != Some(total) || c["total"]["files"].as_u64() != Some(files.len() as u64 + 1) {
                bad.push(format!("{CONTENTS_FILE}: total does not match the folder"));
            }
            let mut seen = BTreeSet::new();
            for (k, n) in [("included", &mut s.included), ("already", &mut s.already), ("in_base", &mut s.in_base)] {
                match c[k].as_array() {
                    None => bad.push(format!("{CONTENTS_FILE}: {k} is not a list")),
                    Some(a) => {
                        *n = a.len();
                        for r in a {
                            let r = r.as_str().unwrap_or("");
                            if !valid_ref(r) {
                                bad.push(format!("{CONTENTS_FILE}: {k}: `{r}` is not asset@style#hash12"));
                            } else if !seen.insert(r.to_string()) {
                                bad.push(format!("{CONTENTS_FILE}: `{r}` is listed twice"));
                            }
                        }
                    }
                }
            }
        }
    }
    let stamp = std::fs::metadata(dir.join(CONTENTS_FILE)).map_or(0, |m| m.len());
    s.total = total + stamp;
    if s.total > MAX_TOTAL_BYTES {
        bad.push(format!("{} in all, over Evani.App's {}", mb(s.total), mb(MAX_TOTAL_BYTES)));
    }
    for f in &files {
        s.stages[usize::from(f.stage.clamp(1, 3) - 1)] += f.size;
    }
    s.stages[0] += stamp;
    if bad.is_empty() {
        Ok(s)
    } else {
        Err(format!("{}: not a library build Evani.App can take:\n  {}\n  fix: build it again with `cargo xtask library build`", dir.display(), bad.join("\n  ")))
    }
}

/// "2026-11-10T19:02:11Z" (seconds, UTC).
fn iso(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

// ---------- the build ----------

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        let dest = to.join(p.file_name().ok_or("a file without a name")?);
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            std::fs::copy(&p, &dest).map_err(|e| format!("{} -> {}: {e}", p.display(), dest.display()))?;
        }
    }
    Ok(())
}

fn sibling(out: &Path, suffix: &str) -> PathBuf {
    let name = out.file_name().map_or_else(|| APP.into(), |n| n.to_string_lossy().into_owned());
    out.with_file_name(format!("{name}.{suffix}"))
}

/// The ship build, staged beside `out`, stamped, checked, then swapped into `out` (the
/// old copy stays in place until the new one is complete and checked).
pub fn build(root: &Path, out: &Path, modules: bool, tools: &Tools) -> Result<Summary, String> {
    let base = base_commit(root)?;
    let l = lists_in(root)?;
    let ship = (tools.build)(modules).map_err(|e| format!("the library build failed: the viewer's ship build did not finish:\n{e}"))?;
    let code = (tools.guard)(&ship)?;
    let staging = sibling(out, "building");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| format!("{}: cannot clear an earlier unfinished build ({e})", staging.display()))?;
    }
    copy_dir(&ship, &staging)?;
    let when = crate::model_import::iso_utc(SystemTime::now());
    write_contents(&staging, &Stamp { base: &base, engine_version: env!("CARGO_PKG_VERSION"), built_at: &when }, &l)?;
    let mut s = check_dir(&staging)?;
    s.code = code;
    swap_in(&staging, out)?;
    Ok(s)
}

/// Moves the finished `staging` folder to `out`; an older `out` is moved aside first
/// and put back if the move fails.
fn swap_in(staging: &Path, out: &Path) -> Result<(), String> {
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    let old = sibling(out, "replaced");
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| format!("{}: {e}", old.display()))?;
    }
    let had = out.exists();
    if had {
        std::fs::rename(out, &old).map_err(|e| format!("{}: cannot replace it ({e}); if Evani.App has the library open, close it and build again. The new build is ready in {}", out.display(), staging.display()))?;
    }
    if let Err(e) = std::fs::rename(staging, out) {
        if had {
            let _ = std::fs::rename(&old, out);
        }
        return Err(format!("{}: cannot move the new build in ({e}); it is in {}", out.display(), staging.display()));
    }
    if had {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-library-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// A stand-in for the viewer's ship folder (the real one is minutes of trunk).
    fn fake_ship(dir: &Path) {
        put(dir, "index.html", b"<!doctype html><script type=module>import init from './evani-material-view-0123456789abcdef.js'; init();</script>");
        put(dir, "evani-material-view-0123456789abcdef.js", b"export default function init() {}");
        put(dir, "evani-material-view-0123456789abcdef_bg.wasm", b"\0asm\x01\0\0\0");
        put(dir, "snippets/evani-models-1/stream.js", b"export function s() {}");
        put(dir, "models/cow.glb", b"glTF....");
        put(dir, "proc-refs/rocks-pebbles.jpg", b"\xff\xd8\xff");
        put(dir, "ENGINE-LICENSE.txt", b"MIT");
        put(dir, "build-facts.json", br#"{"schema":"evani-build-facts/1","game":{"id":"material-view"},"modules":{"live_tuning":false}}"#);
    }

    const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

    fn stamped(name: &str, l: &Lists) -> PathBuf {
        let d = tmp(name);
        fake_ship(&d);
        write_contents(&d, &Stamp { base: BASE, engine_version: "0.1.0", built_at: "2026-11-10T19:02:11Z" }, l).unwrap();
        d
    }

    fn rec(h: &str, files: &[&str]) -> Value {
        json!({ "version_id": &h[..12], "content_hash": format!("sha256:{h}"), "files": files })
    }

    #[test]
    fn versions_are_sorted_into_included_already_and_in_base() {
        let (a, b, c) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
        let head = json!({ "adopted": { "trees-oak@photoreal": rec(&c, &["x.rs"]) } }).to_string();
        let now = json!({ "adopted": {
            "rocks-pebbles@photoreal": rec(&a, &["crates/evani-procedural-lib/src/rocks.rs"]),
            "sky-dome@photoreal": rec(&b, &["crates/evani-procedural-lib/src/sky.rs"]),
            "rocks-cobble@stylized": rec(&b, &[]),
            "trees-oak@photoreal": rec(&c, &["x.rs"]),
        } })
        .to_string();
        let l = lists(Some(&now), Some(&head)).unwrap();
        assert_eq!(l.included, vec![format!("rocks-pebbles@photoreal#{}", &a[..12]), format!("sky-dome@photoreal#{}", &b[..12])]);
        assert_eq!(l.already, vec![format!("rocks-cobble@stylized#{}", &b[..12])]);
        assert_eq!(l.in_base, vec![format!("trees-oak@photoreal#{}", &c[..12])]);
        // A newer version of a base asset is this build's.
        let newer = json!({ "adopted": { "trees-oak@photoreal": rec(&a, &["x.rs"]) } }).to_string();
        assert_eq!(lists(Some(&newer), Some(&head)).unwrap().included.len(), 1);
        // Nothing adopted: empty lists, not an error.
        assert_eq!(lists(None, None).unwrap(), Lists::default());
        assert!(lists(Some("{\"adopted\": {\"Bad Key\": {\"version_id\": \"aaaaaaaaaaaa\"}}}"), None).is_err());
    }

    #[test]
    fn refs_paths_and_stages_follow_evani_app_s_rules() {
        assert!(valid_ref("rocks-pebbles@photoreal#0123456789ab"));
        for bad in ["rocks-pebbles@photoreal", "Rocks@photoreal#0123456789ab", "rocks@photoreal#0123456789AB", "rocks#0123456789ab", "r@photoreal#0123456789ab"] {
            assert!(!valid_ref(bad), "{bad}");
        }
        assert!(safe_path("models/cow.glb"));
        for bad in ["../x", "/x", "a//b", "a/./b", "http://x", "a b", &"x".repeat(201)] {
            assert!(!safe_path(bad), "{bad}");
        }
        assert_eq!(stage_of("index.html"), 1);
        assert_eq!(stage_of("snippets/a/b.js"), 1);
        assert_eq!(stage_of("models/cow.glb"), 2);
        assert_eq!(stage_of("proc-refs/rocks.jpg"), 3);
        assert!(iso("2026-11-10T19:02:11Z") && !iso("2026-11-10 19:02 UTC"));
    }

    #[test]
    fn a_stamped_folder_passes_and_names_its_files() {
        let l = Lists { included: vec!["rocks-pebbles@photoreal#0123456789ab".into()], ..Default::default() };
        let d = stamped("ok", &l);
        let s = check_dir(&d).unwrap();
        assert_eq!((s.files, s.included, s.base.as_str()), (9, 1, BASE));
        assert_eq!(s.build.len(), 12);
        let c: Value = serde_json::from_str(&std::fs::read_to_string(d.join(CONTENTS_FILE)).unwrap()).unwrap();
        assert_eq!(c["format"], CONTENTS_FORMAT);
        assert_eq!(c["entry"], "index.html");
        let stage = |p: &str| c["files"].as_array().unwrap().iter().find(|f| f["path"] == p).map(|f| f["stage"].clone());
        assert_eq!(stage("models/cow.glb"), Some(json!(2)));
        assert_eq!(stage("proc-refs/rocks-pebbles.jpg"), Some(json!(3)));
        assert_eq!(stage(CONTENTS_FILE), None, "the stamp does not list itself");
        // The same files make the same build name.
        assert_eq!(check_dir(&stamped("ok2", &l)).unwrap().build, s.build);
    }

    #[test]
    fn a_broken_folder_is_refused_with_every_problem() {
        let d = stamped("broken", &Lists::default());
        put(&d, "models/cow.glb", b"changed");
        put(&d, "extra.js", b"x");
        std::fs::remove_file(d.join("ENGINE-LICENSE.txt")).unwrap();
        let e = check_dir(&d).unwrap_err();
        for want in ["ENGINE-LICENSE.txt is missing", "does not name these files", "not listed [\"extra.js\"]", "changed [\"models/cow.glb\"]"] {
            assert!(e.contains(want), "{want}: {e}");
        }
        // No wasm, live tuning on, no stamp.
        let d = tmp("nowasm");
        fake_ship(&d);
        std::fs::remove_file(d.join("evani-material-view-0123456789abcdef_bg.wasm")).unwrap();
        put(&d, "build-facts.json", br#"{"game":{"id":"material-view"},"modules":{"live_tuning":true}}"#);
        let e = check_dir(&d).unwrap_err();
        for want in ["*_bg.wasm is missing", "live tuning is on", "library-contents.json:"] {
            assert!(e.contains(want), "{want}: {e}");
        }
        // A file over Evani.App's limit, or a path it refuses.
        let d = stamped("big", &Lists::default());
        put(&d, "models/a b.glb", b"x");
        let e = check_dir(&d).unwrap_err();
        assert!(e.contains("a path Evani.App refuses"), "{e}");
        // A bad ref in the stamp.
        let d = stamped("badref", &Lists { already: vec!["rocks@photoreal".into()], ..Default::default() });
        assert!(check_dir(&d).unwrap_err().contains("is not asset@style#hash12"));
    }

    fn git_repo(name: &str) -> Option<PathBuf> {
        let d = tmp(name);
        let ok = |args: &[&str]| Command::new("git").arg("-C").arg(&d).args(args).output().is_ok_and(|o| o.status.success());
        let made = ok(&["init", "-q"]) && ok(&["config", "user.email", "t@example.com"]) && ok(&["config", "user.name", "T"]);
        put(&d, "README.md", b"x");
        (made && ok(&["add", "-A"]) && ok(&["commit", "-q", "-m", "base"])).then_some(d)
    }

    #[test]
    fn build_stages_stamps_checks_and_swaps_the_folder_in() {
        let Some(root) = git_repo("build") else {
            eprintln!("skipped: no git");
            return;
        };
        let ship = root.join("ship");
        fake_ship(&ship);
        let built = |_: bool| -> Result<PathBuf, String> { Ok(ship.clone()) };
        let guard = |_: &Path| -> Result<String, String> { Ok("code: fine".into()) };
        let tools = Tools { build: &built, guard: &guard };
        let out = root.join("serve").join(APP);
        put(&out, "old.txt", b"the previous build");
        let s = build(&root, &out, false, &tools).unwrap();
        assert!(out.join(CONTENTS_FILE).is_file() && out.join(ENTRY).is_file());
        assert!(!out.join("old.txt").exists(), "the old build is replaced");
        assert!(!sibling(&out, "building").exists() && !sibling(&out, "replaced").exists());
        assert_eq!(s.base, base_commit(&root).unwrap());
        assert_eq!(check_dir(&out).unwrap().build, s.build);
        // A failed ship build leaves the old library alone.
        let failed = |_: bool| -> Result<PathBuf, String> { Err("trunk build failed".into()) };
        let e = build(&root, &out, false, &Tools { build: &failed, guard: &guard }).unwrap_err();
        assert!(e.contains("the library build failed") && e.contains("trunk build failed"), "{e}");
        assert!(check_dir(&out).is_ok());
        // Dev-only code found by the guard: refused, old library kept.
        let dev = |_: &Path| -> Result<String, String> { Err("material-view: the ship build still contains live code swapping".into()) };
        assert!(build(&root, &out, false, &Tools { build: &built, guard: &dev }).is_err());
        assert!(check_dir(&out).is_ok());
        // Not a git checkout: refused before building.
        let plain = tmp("plain");
        assert!(build(&plain, &plain.join("out"), false, &tools).unwrap_err().contains("not a git checkout"));
    }
}
