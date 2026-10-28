//! `cargo xtask publish <id>`: one command from `apps/<id>/evani.toml` to a
//! checked package folder that Evani.App's publish card can upload
//! (STAGES.md Stage 4, "One-click publish"; docs/PUBLISHING.md).
//!
//! ```text
//! cargo xtask publish <id> [--dry-run] [--serve] [--port 8114] [--out <dir>] [--from <dir>] [--quick]
//! cargo xtask publish <id> --mark-published v5 [--out <dir>]
//! ```
//!
//! 1. The ship build (as `cargo xtask build <id> --ship`: ship guard, free
//!    roam, the 10 MB cap, the engine notice).
//! 2. The package is put together in a staging folder next to the
//!    destination (`publish/<id>/`, or `--out`): the build's files, the page
//!    with the Evani loading screen as its first script and the smooth start,
//!    and the files carried over from the previous package (README.md,
//!    LICENSE, preview.png, evani.json, evani-loader.js with its title line
//!    set to the game's title). Nothing from the old folder is kept beyond
//!    that, so stale hashed files go.
//! 3. A preflight checks the staged folder ([`preflight`]). Only when every
//!    check passes is the staging folder swapped into place (two renames, the
//!    old folder put back if the second fails); a failed run leaves the
//!    destination as it was. `--dry-run` never touches the destination: the
//!    package goes to `.evani/publish/<id>/dry-run/`.
//! 4. The size report (per module, then per file; raw, gzip and Brotli, each
//!    labelled; the 10 MB cap is on the raw whole folder) and the diff against
//!    the version recorded as live (path, SHA-256 and size per file: NEW,
//!    MODIFIED, SAME, DELETED and the download-size change; "nothing changed,
//!    skip" when the package is identical).
//! 5. `summary.json` and `summary.md` in `.evani/publish/<id>/`, outside the
//!    uploaded folder, with the version history (`published/vN.json`,
//!    `live.json`). `--mark-published vN` records the package folder as live
//!    after the owner's click on Evani.App's card, so the next diff is against
//!    what players get.
//!
//! It never uploads: only the owner's click on Evani.App's publish card does.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    AppDir, BuildSize, Manifest, absolute_paths_in, dev_only_in, gzip_len, kb, known_markers, load_app,
    playground_guard, playground_size_guard, scan_markers, ship_guard, signed_kb, trunk_build, workspace_root,
};
use crate::size_report::{LIMIT_BYTES, brotli_len};
use evani_guide::playground as pg;

/// The loading screen every playground carries (docs/PUBLISHING.md).
pub const LOADER_FILE: &str = "evani-loader.js";
/// The loader's script tag: the first script in `<head>`.
pub const LOADER_TAG: &str = "<script src=\"evani-loader.js\"></script>";
/// No favicon request (and no 404) from the page.
const ICON_TAG: &str = "<link rel=\"icon\" href=\"data:,\">";
/// The line in the loader that names the playground.
const TITLE_VAR: &str = "var EVANI_BIOME_TITLE = ";
/// Marks the smooth-start bootstrap (tools/smooth-bootstrap.mjs does the same).
pub const SMOOTH_MARK: &str = "// evani-smooth-start";
/// How a package README's licence line starts; `assemble` swaps any older
/// line that starts this way for [`pg::README_LICENCE_LINE`] when it carries
/// a README over (docs/LICENSING.md).
pub const README_LICENCE_PREFIX: &str = "Game code: MIT.";

/// `md` with every licence line ([`README_LICENCE_PREFIX`]) set to the
/// current [`pg::README_LICENCE_LINE`]; other lines are kept as they are.
pub fn with_current_licence_line(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for line in md.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        if body.trim_start().starts_with(README_LICENCE_PREFIX) && body.trim() != pg::README_LICENCE_LINE {
            let indent = &body[..body.len() - body.trim_start().len()];
            out.push_str(indent);
            out.push_str(pg::README_LICENCE_LINE);
            out.push_str(&line[body.len()..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Files carried over from the previous package; everything else is rebuilt.
pub const CARRY_OVER: [&str; 5] =["README.md", "LICENSE", "preview.png", "evani.json", LOADER_FILE];
/// The default `--serve` port.
pub const DEFAULT_PORT: u16 = 8114;
/// The summary format, so Evani.App can tell versions of it apart.
pub const SUMMARY_SCHEMA: &str = "evani.publish.summary/1";

pub fn usage() -> String {
    "  cargo xtask publish <id> [--dry-run] [--serve] [--port 8114] [--out <dir>] [--from <dir>] [--quick]\n  cargo xtask publish <id> --mark-published vN [--out <dir>]   record the package folder as the live version".to_string()
}

// ---- package manifest (path -> SHA-256 -> size) ------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub sha256: String,
    pub size: u64,
    pub gzip: u64,
    pub brotli: u64,
}

/// Every file of a package folder, by relative path (`/` separated).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageManifest {
    pub files: BTreeMap<String, FileEntry>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

impl PackageManifest {
    /// Hashes and measures every file under `dir`.
    pub fn scan(dir: &Path) -> Result<Self, String> {
        fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, FileEntry>) -> Result<(), String> {
            let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(base, &p, out)?;
                    continue;
                }
                let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                let rel = p
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(
                    rel,
                    FileEntry { sha256: sha256_hex(&bytes), size: bytes.len() as u64, gzip: gzip_len(&bytes), brotli: brotli_len(&bytes) },
                );
            }
            Ok(())
        }
        let mut files = BTreeMap::new();
        walk(dir, dir, &mut files)?;
        Ok(Self { files })
    }

    /// The package's content address: SHA-256 over every path, hash and size.
    pub fn package_id(&self) -> String {
        let mut h = Sha256::new();
        for (path, f) in &self.files {
            h.update(format!("{path}\t{}\t{}\n", f.sha256, f.size).as_bytes());
        }
        hex(&h.finalize())
    }

    /// (raw, gzip, brotli) over every file.
    pub fn totals(&self) -> (u64, u64, u64) {
        self.files.values().fold((0, 0, 0), |(r, g, b), f| (r + f.size, g + f.gzip, b + f.brotli))
    }

    pub fn to_json(&self) -> Value {
        let files: serde_json::Map<String, Value> = self
            .files
            .iter()
            .map(|(p, f)| (p.clone(), json!({"sha256": f.sha256, "size": f.size, "gzip": f.gzip, "brotli": f.brotli})))
            .collect();
        Value::Object(files)
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let obj = v.as_object().ok_or("package manifest: `files` must be an object")?;
        let mut files = BTreeMap::new();
        for (p, f) in obj {
            let num = |k: &str| f.get(k).and_then(Value::as_u64).unwrap_or(0);
            let sha256 = f.get("sha256").and_then(Value::as_str).ok_or_else(|| format!("package manifest: {p} has no sha256"))?;
            files.insert(p.clone(), FileEntry { sha256: sha256.to_string(), size: num("size"), gzip: num("gzip"), brotli: num("brotli") });
        }
        Ok(Self { files })
    }
}

// ---- diff against the live version (butler's push-preview idea) -------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    New,
    Modified,
    Same,
    Deleted,
}

impl Change {
    pub fn label(self) -> &'static str {
        match self {
            Change::New => "NEW",
            Change::Modified => "MODIFIED",
            Change::Same => "SAME",
            Change::Deleted => "DELETED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiffRow {
    pub path: String,
    pub change: Change,
    pub old: Option<FileEntry>,
    pub new: Option<FileEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct Diff {
    pub rows: Vec<DiffRow>,
}

impl Diff {
    pub fn count(&self, c: Change) -> usize {
        self.rows.iter().filter(|r| r.change == c).count()
    }

    /// Nothing new, changed or deleted: publishing would change nothing.
    pub fn unchanged(&self) -> bool {
        self.rows.iter().all(|r| r.change == Change::Same)
    }

    /// (raw, gzip, brotli) change of the whole package, new minus old.
    pub fn size_change(&self) -> (i64, i64, i64) {
        self.rows.iter().fold((0, 0, 0), |(r, g, b), row| {
            let (or, og, ob) = row.old.as_ref().map_or((0, 0, 0), |f| (f.size, f.gzip, f.brotli));
            let (nr, ng, nb) = row.new.as_ref().map_or((0, 0, 0), |f| (f.size, f.gzip, f.brotli));
            (r + nr as i64 - or as i64, g + ng as i64 - og as i64, b + nb as i64 - ob as i64)
        })
    }

    /// What a player who has the live version cached downloads again
    /// (gzip): the new and modified files among those the page loads
    /// (`page`, [`page_files`]); README, licences and preview never reach a browser.
    pub fn redownload_gzip(&self, page: &[String]) -> u64 {
        self.rows
            .iter()
            .filter(|r| matches!(r.change, Change::New | Change::Modified) && page.contains(&r.path))
            .filter_map(|r| r.new.as_ref().map(|f| f.gzip))
            .sum()
    }
}

/// Whole-file diff: a path is SAME when its SHA-256 matches.
pub fn diff(old: &PackageManifest, new: &PackageManifest) -> Diff {
    let mut rows = Vec::new();
    for (p, n) in &new.files {
        let change = match old.files.get(p) {
            None => Change::New,
            Some(o) if o.sha256 == n.sha256 => Change::Same,
            Some(_) => Change::Modified,
        };
        rows.push(DiffRow { path: p.clone(), change, old: old.files.get(p).cloned(), new: Some(n.clone()) });
    }
    for (p, o) in &old.files {
        if !new.files.contains_key(p) {
            rows.push(DiffRow { path: p.clone(), change: Change::Deleted, old: Some(o.clone()), new: None });
        }
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Diff { rows }
}

// ---- the page and the loader -------------------------------------------------------

/// The game's title: the `<title>` of `apps/<id>/index.html`.
pub fn page_title(html: &str) -> Option<String> {
    let start = html.find("<title>")? + "<title>".len();
    let end = html[start..].find("</title>")? + start;
    let t = html[start..end]
        .trim()
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    (!t.is_empty()).then_some(t)
}

/// The title the loader shows (`var EVANI_BIOME_TITLE = '...';`).
pub fn loader_title(js: &str) -> Option<String> {
    let line = js.lines().find(|l| l.trim_start().starts_with(TITLE_VAR))?;
    let rest = line.trim_start()[TITLE_VAR.len()..].trim_start();
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            c if c == quote => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// A JS string literal for `title`: single quotes unless it holds one.
fn js_string(title: &str) -> String {
    let quote = if title.contains('\'') && !title.contains('"') { '"' } else { '\'' };
    let mut s = String::from(quote);
    for c in title.chars() {
        if c == '\\' || c == quote {
            s.push('\\');
        }
        s.push(c);
    }
    s.push(quote);
    s
}

/// The loader with its title line naming `title` (unchanged when it already does).
pub fn set_loader_title(js: &str, title: &str) -> Result<String, String> {
    if loader_title(js).as_deref() == Some(title) {
        return Ok(js.to_string());
    }
    let start = js
        .find(TITLE_VAR)
        .ok_or_else(|| format!("{LOADER_FILE} has no `{TITLE_VAR}...` line (is it the Evani loading screen?)"))?;
    let end = js[start..].find(['\r', '\n']).map_or(js.len(), |e| start + e);
    Ok(format!("{}{TITLE_VAR}{};{}", &js[..start], js_string(title), &js[end..]))
}

/// The smooth start (docs/PUBLISHING.md): the wasm is fetched and compiled at
/// once, the engine starts after the loader's logo has landed, and the loader
/// is held until frames flow. Same text as tools/smooth-bootstrap.mjs.
fn smooth_boot(js: &str, wasm: &str) -> String {
    format!(
        "import init, * as bindings from '{js}';
{SMOOTH_MARK}
// Fetch and compile the wasm at once (off the main thread), but start the engine only after the loader's logo
// has landed: engine start-up on the main thread and GPU used to stall the logo's entrance. Then hold the
// loader until frames flow again, so it fades to the game and not to an empty canvas. Without the loader's
// landed and hold (an older loader) the game starts at once, as before.
const wasmUrl = '{wasm}';
const compiled = WebAssembly.compileStreaming(fetch(wasmUrl))
  .catch(() => fetch(wasmUrl).then((r) => r.arrayBuffer()).then((b) => WebAssembly.compile(b)));
const loader = window.EvaniLoader;
const release = loader && loader.landed && loader.hold ? loader.hold() : null;
if (release) await loader.landed;
let wasm;
try {{
  wasm = await init({{ module_or_path: await compiled }});
  window.wasmBindings = bindings;
  dispatchEvent(new CustomEvent(\"TrunkApplicationStarted\", {{detail: {{wasm}}}}));
  if (release) {{
    await new Promise((done) => {{
      const began = performance.now();
      let last = began;
      let smooth = 0;
      const step = (t) => {{
        smooth = t - last < 34 ? smooth + 1 : 0;
        last = t;
        if ((smooth >= 5 && t - began > 400) || t - began > 4000) done();
        else requestAnimationFrame(step);
      }};
      requestAnimationFrame(step);
      setTimeout(done, 4500);
    }});
  }}
}} finally {{
  if (release) release();
}}"
    )
}

/// The published page from Trunk's: line ends LF, the loader as the first
/// script in `<head>` (and the empty favicon), and the smooth start in place
/// of Trunk's bootstrap. Safe to run on its own output. A loader tag already
/// in the page but not first is moved to the top of `<head>`.
pub fn package_html(trunk_html: &str) -> Result<String, String> {
    let mut html = trunk_html.replace("\r\n", "\n");
    if html.contains(LOADER_TAG) && loader_first(&html).is_err() {
        // A loader tag that is not the first script in <head>: take it out
        // (with its own line when it has one) and put it back first below.
        while let Some(at) = html.find(LOADER_TAG) {
            let line_start = html[..at].rfind('\n').map_or(0, |i| i + 1);
            let end = at + LOADER_TAG.len();
            let own_line = html[line_start..at].trim().is_empty() && (html[end..].starts_with('\n') || end == html.len());
            if own_line {
                html.replace_range(line_start..(end + 1).min(html.len()), "");
            } else {
                html.replace_range(at..end, "");
            }
        }
    }
    if !html.contains(LOADER_TAG) {
        let at = html.find("<head>").ok_or("index.html has no <head>")? + "<head>".len();
        let at = if html[at..].starts_with('\n') { at + 1 } else { at };
        let mut add = format!("  {LOADER_TAG}\n");
        if !html.contains("rel=\"icon\"") {
            add.push_str(&format!("  {ICON_TAG}\n"));
        }
        html.insert_str(at, &add);
    }
    if html.contains(SMOOTH_MARK) {
        return Ok(html);
    }
    const IMPORT: &str = "import init, * as bindings from '";
    const INIT: &str = "const wasm = await init({ module_or_path: '";
    const STARTED: &str = "dispatchEvent(new CustomEvent(\"TrunkApplicationStarted\", {detail: {wasm}}));";
    let missing = "index.html: the Trunk bootstrap was not found (is it a ship build with relative ./ paths?)";
    let start = html.find(IMPORT).ok_or(missing)?;
    let js_end = html[start + IMPORT.len()..].find("';\n").ok_or(missing)? + start + IMPORT.len();
    let js = html[start + IMPORT.len()..js_end].to_string();
    let after_import = js_end + "';\n".len();
    if !html[after_import..].starts_with(INIT) {
        return Err(missing.to_string());
    }
    let wasm_start = after_import + INIT.len();
    let wasm_end = html[wasm_start..].find("' });").ok_or(missing)? + wasm_start;
    let wasm = html[wasm_start..wasm_end].to_string();
    let rest_start = wasm_end + "' });".len();
    let started = html[rest_start..].find(STARTED).ok_or(missing)? + rest_start;
    let between = html[rest_start..started].replace("window.wasmBindings = bindings;", "");
    if !between.trim().is_empty() {
        return Err("index.html: unexpected text in the Trunk bootstrap; the smooth start would drop it".to_string());
    }
    let end = started + STARTED.len();
    Ok(format!("{}{}{}", &html[..start], smooth_boot(&js, &wasm), &html[end..]))
}

/// Local files the page loads: `src`/`href` attributes, module imports, the
/// wasm path and css `url(..)`, without `./`, query or fragment.
pub fn local_refs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let pats = [
        "src=\"", "src='", "href=\"", "href='", " from '", " from \"", "module_or_path: '", "wasmUrl = '", "url(\"", "url('",
        "url(",
    ];
    for pat in pats {
        let close: &[char] = match pat.chars().last() {
            Some('"') => &['"'],
            Some('\'') => &['\''],
            _ => &[')'],
        };
        for (at, _) in html.match_indices(pat) {
            let rest = &html[at + pat.len()..];
            if pat == "url(" && (rest.starts_with('"') || rest.starts_with('\'')) {
                continue;
            }
            let Some(end) = rest.find(close) else { continue };
            let v = rest[..end].trim();
            let skip = ["http:", "https:", "data:", "//", "#", "mailto:", "blob:", "javascript:", "about:"];
            if v.is_empty() || skip.iter().any(|s| v.starts_with(s)) || v.contains("${") || v.contains('\n') || v.contains(' ') {
                continue;
            }
            let v = v.split(['?', '#']).next().unwrap_or("");
            let v = v.trim_start_matches("./");
            if !v.is_empty() {
                out.push(v.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Where the loader's script tag sits: Ok when it is the first `<script` of
/// the page and inside `<head>`.
pub fn loader_first(html: &str) -> Result<(), String> {
    let head = html.find("<head").ok_or("index.html has no <head>")?;
    let head_end = html.find("</head>").ok_or("index.html has no </head>")?;
    let first = html.find("<script").ok_or("index.html has no script")?;
    let loader = html.find(LOADER_TAG).ok_or_else(|| format!("index.html does not load {LOADER_FILE}"))?;
    if loader != first {
        return Err(format!("another script comes before {LOADER_FILE}"));
    }
    if loader < head || loader > head_end {
        return Err(format!("{LOADER_FILE} is not inside <head>"));
    }
    Ok(())
}

// ---- assembling and swapping -------------------------------------------------------

/// What the package is checked against.
#[derive(Debug, Clone, Default)]
pub struct Expect {
    pub app: String,
    pub kit: String,
    pub look: String,
    /// The game's title (the loader names it).
    pub title: String,
    /// `[playground] free_roam`, `None` when the game is not a playground.
    pub free_roam: Option<bool>,
    /// Free-roam gaps from `evani_guide::playground::problems`.
    pub playground_problems: Vec<String>,
    /// Every `evani.*` marker name (to find the live module in the wasm).
    pub known_markers: Vec<String>,
}

/// The `evani.json` a package gets when the previous one had none.
pub fn evani_json(e: &Expect) -> String {
    format!("{{\n  \"freeRoam\": {},\n  \"kit\": \"{}\",\n  \"look\": \"{}\"\n}}\n", e.free_roam.unwrap_or(false), e.kit, e.look)
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let rd = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for e in rd.flatten() {
        let p = e.path();
        let dst = to.join(e.file_name());
        if p.is_dir() {
            copy_tree(&p, &dst)?;
        } else {
            std::fs::copy(&p, &dst).map_err(|e| format!("{} -> {}: {e}", p.display(), dst.display()))?;
        }
    }
    Ok(())
}

/// Builds the package in `staging` (made fresh) from the ship folder `dist`
/// and the previous package `carry`. Returns notes on what was carried over.
pub fn assemble(root: &Path, dist: &Path, carry: Option<&Path>, staging: &Path, e: &Expect) -> Result<Vec<String>, String> {
    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|err| format!("{}: {err}", staging.display()))?;
    }
    copy_tree(dist, staging)?;
    let html = std::fs::read_to_string(dist.join("index.html")).map_err(|err| format!("{}: {err}", dist.join("index.html").display()))?;
    std::fs::write(staging.join("index.html"), package_html(&html)?).map_err(|err| err.to_string())?;
    pg::write_engine_notice(root, staging)?;
    let mut notes = Vec::new();
    for name in CARRY_OVER {
        let src = carry.map(|c| c.join(name)).filter(|p| p.is_file());
        let dst = staging.join(name);
        match (name, src) {
            (LOADER_FILE, Some(src)) => {
                let js = std::fs::read_to_string(&src).map_err(|err| format!("{}: {err}", src.display()))?;
                let fixed = set_loader_title(&js, &e.title)?;
                if fixed != js {
                    notes.push(format!("{LOADER_FILE}: title line set to {}", js_string(&e.title)));
                }
                std::fs::write(&dst, fixed).map_err(|err| err.to_string())?;
            }
            ("README.md", Some(src)) => {
                let md = std::fs::read_to_string(&src).map_err(|err| format!("{}: {err}", src.display()))?;
                let fixed = with_current_licence_line(&md);
                if fixed != md {
                    notes.push("README.md: licence line updated (game code and the Evani.Dev runtime are both MIT)".to_string());
                }
                std::fs::write(&dst, fixed).map_err(|err| err.to_string())?;
            }
            (_, Some(src)) => {
                std::fs::copy(&src, &dst).map_err(|err| format!("{}: {err}", src.display()))?;
            }
            ("evani.json", None) => {
                std::fs::write(&dst, evani_json(e)).map_err(|err| err.to_string())?;
                notes.push("evani.json: written from evani.toml (no previous one)".to_string());
            }
            (_, None) => notes.push(format!("{name}: not in the previous package (the preflight reports it)")),
        }
    }
    Ok(notes)
}

fn sibling(dest: &Path, suffix: &str) -> PathBuf {
    let name = dest.file_name().map_or_else(|| "package".to_string(), |n| n.to_string_lossy().into_owned());
    dest.with_file_name(format!(".{name}.{suffix}"))
}

/// Only a missing or empty folder, or an earlier package (it has index.html
/// and the loader), may be replaced: a mistyped `--out` never wipes a folder.
pub fn replaceable(dest: &Path) -> Result<(), String> {
    if !dest.exists() {
        return Ok(());
    }
    let empty = std::fs::read_dir(dest).map_err(|e| format!("{}: {e}", dest.display()))?.next().is_none();
    if empty || (dest.join("index.html").is_file() && dest.join(LOADER_FILE).is_file()) {
        return Ok(());
    }
    Err(format!(
        "{} is not a playground package (no index.html and {LOADER_FILE}); refusing to replace it. Pick an empty folder or an earlier package with --out",
        dest.display()
    ))
}

/// The staging folder for `dest`: next to it, so the swap is a rename.
pub fn staging_for(dest: &Path) -> PathBuf {
    sibling(dest, "staging")
}

fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = Ok(());
    for _ in 0..4 {
        last = std::fs::rename(from, to);
        if last.is_ok() {
            return last;
        }
        // A virus scanner can hold a just-written file for a moment.
        std::thread::sleep(Duration::from_millis(150));
    }
    last
}

/// Swaps `staging` into `dest`: the old folder is moved aside, the new one
/// renamed into place, then the old one removed. If the second rename fails
/// the old folder is put back, so `dest` is either the old or the new package.
pub fn swap_into(staging: &Path, dest: &Path) -> Result<(), String> {
    let old = sibling(dest, "old");
    if old.exists() {
        if dest.exists() {
            // A finished swap whose clean-up failed: the package is in `dest`.
            std::fs::remove_dir_all(&old).map_err(|e| format!("{}: {e}", old.display()))?;
        } else {
            // A run stopped between the two renames: `old` is the only copy. Put it back first.
            rename_retry(&old, dest).map_err(|e| {
                format!("{} holds the previous package and could not be put back at {} ({e}); move it back by hand", old.display(), dest.display())
            })?;
        }
    }
    replaceable(dest)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let had_old = dest.exists();
    if had_old {
        rename_retry(dest, &old).map_err(|e| {
            format!("{} is in use and was left as it was ({e}); close what holds it and run again", dest.display())
        })?;
    }
    if let Err(e) = rename_retry(staging, dest) {
        if !had_old {
            return Err(format!("could not move the new package into {} ({e})", dest.display()));
        }
        return Err(match rename_retry(&old, dest) {
            Ok(()) => format!("could not move the new package into {} ({e}); the old one is back in place", dest.display()),
            Err(e2) => format!(
                "could not move the new package into {} ({e}), nor put the old one back ({e2}): the previous package is in {}; move it back by hand",
                dest.display(),
                old.display()
            ),
        });
    }
    if had_old && let Err(e) = std::fs::remove_dir_all(&old) {
        eprintln!("  note: could not remove {} ({e}); delete it by hand", old.display());
    }
    Ok(())
}

// ---- preflight ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: &'static str,
    pub ok: bool,
    pub detail: String,
    pub fix: String,
}

fn check(id: &'static str, problem: Option<(String, String)>, ok_detail: String) -> Check {
    match problem {
        None => Check { id, ok: true, detail: ok_detail, fix: String::new() },
        Some((detail, fix)) => Check { id, ok: false, detail, fix },
    }
}

/// Width and height from a PNG's IHDR chunk.
pub fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[12..16] != b"IHDR" {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    Some((be(16), be(20)))
}

/// Evani.App's preview rule: the short side 512 px or more, square or 16:9 (1% slack).
pub fn preview_shape_ok(w: u32, h: u32) -> bool {
    let (w, h) = (u64::from(w), u64::from(h));
    let near = |a: u64, b: u64| a.abs_diff(b) * 100 <= b;
    w.min(h) >= 512 && (w == h || near(w * 9, h * 16) || near(h * 9, w * 16))
}

/// Every check a package must pass before it may be uploaded.
pub fn preflight(dir: &Path, e: &Expect) -> Vec<Check> {
    let mut out = Vec::new();
    let id = &e.app;
    let read = |n: &str| std::fs::read(dir.join(n)).ok();
    let html = read("index.html").map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();

    // 1. Size: the whole folder, uncompressed, under the cap.
    let total = pg::dir_bytes(dir);
    out.push(check(
        "size",
        (total > pg::PACKAGE_CAP_BYTES).then(|| {
            (
                format!("the package is {} (raw, whole folder), over the {} cap", pg::mb(total), pg::mb(pg::PACKAGE_CAP_BYTES)),
                format!("Shrink {id}: smaller or fewer textures, sounds and models, or optional modules off in apps/{id}/evani.toml (the module table above shows each one's size)."),
            )
        }),
        format!("{} of {} (raw, whole folder)", pg::mb(total), pg::mb(pg::PACKAGE_CAP_BYTES)),
    ));

    // 2. No dev-only code: no live module, no live code swapping, no root-absolute paths.
    let mut size = BuildSize::default();
    let mut engine_js = Vec::new();
    let mut wasm_files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for f in rd.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let Ok(bytes) = std::fs::read(f.path()) else { continue };
            if name.ends_with(".wasm") {
                size.wasm_raw += bytes.len() as u64;
                size.markers.extend(scan_markers(&bytes, &e.known_markers));
                size.dev_only.extend(dev_only_in(&bytes));
                wasm_files.push(name);
            } else if name.ends_with(".js") {
                size.dev_only.extend(dev_only_in(&bytes));
                if name != LOADER_FILE {
                    engine_js.push(name);
                }
            }
        }
    }
    size.absolute_paths = absolute_paths_in(&html);
    size.dev_only.sort();
    size.dev_only.dedup();
    out.push(check(
        "dev_only",
        ship_guard(id, &size).err().map(|err| {
            (err, format!("Run `cargo xtask publish {id}` again: it builds with `--ship`, which drops the live module and live code swapping."))
        }),
        "no live module, no live code swapping, no root-absolute paths".to_string(),
    ));

    // 3. The preview picture.
    let png = read("preview.png");
    out.push(check(
        "preview",
        match &png {
            None => Some((
                "preview.png is missing".to_string(),
                format!("Add a real in-game capture as preview.png to the previous package, or run again with `--from <main checkout>/publish/{id}` (a worktree has no publish/ folder of its own)."),
            )),
            Some(b) if !b.starts_with(b"\x89PNG\r\n\x1a\n") => {
                Some(("preview.png is not a PNG image".to_string(), "Save the preview capture as a PNG.".to_string()))
            }
            Some(b) => match png_size(b) {
                None => Some(("preview.png has no readable size (IHDR)".to_string(), "Save the preview capture as a PNG again.".to_string())),
                Some((w, h)) if !preview_shape_ok(w, h) => Some((
                    format!("preview.png is {w}x{h}; Evani.App wants 512 px or more on each side, square or 16:9, else the item stays unlisted"),
                    "Capture it at 1280x720 (16:9) or a square of 512 px or more.".to_string(),
                )),
                Some(_) => None,
            },
        },
        format!(
            "preview.png ({}, {})",
            png.as_deref().and_then(png_size).map_or_else(|| "?".to_string(), |(w, h)| format!("{w}x{h}")),
            kb(png.as_ref().map_or(0, |b| b.len() as u64))
        ),
    ));

    // 4. The loading screen: present, titled, first script in <head>.
    let loader = read(LOADER_FILE).map(|b| String::from_utf8_lossy(&b).into_owned());
    let loader_problem = match &loader {
        None => Some((
            format!("{LOADER_FILE} is missing"),
            format!("Download https://evani.app/evani-loader.js into the previous package, or run again with `--from <main checkout>/publish/{id}`; the publish command sets its title line."),
        )),
        Some(js) => match loader_title(js) {
            None => Some((format!("{LOADER_FILE} has no title line"), "Use the Evani loading screen from https://evani.app/evani-loader.js.".to_string())),
            Some(t) if t != e.title => Some((
                format!("{LOADER_FILE} names \"{t}\", the game is \"{}\"", e.title),
                format!("Run `cargo xtask publish {id}` again (it sets the title line from apps/{id}/index.html)."),
            )),
            Some(_) => loader_first(&html).err().map(|err| {
                (err, format!("Make `{LOADER_TAG}` the first script inside <head> (the publish command does this to Trunk's page)."))
            }),
        },
    };
    out.push(check("loader", loader_problem, format!("{LOADER_FILE} is the first script in <head>, titled \"{}\"", e.title)));

    // 5. Licences: the game's own LICENSE, the engine notice, the README's licence line.
    let licence = read("LICENSE").filter(|b| !b.iter().all(u8::is_ascii_whitespace));
    let readme = read("README.md");
    let lic_problem = if readme.is_none() {
        Some((
            "README.md is missing".to_string(),
            format!(
                "Run again with `--from <main checkout>/publish/{id}`, or add README.md (what it is, controls, its licence line: {}).",
                pg::README_LICENCE_LINE
            ),
        ))
    } else if licence.is_none() {
        Some((
            "LICENSE (the game's own licence) is missing or empty".to_string(),
            format!("Run again with `--from <main checkout>/publish/{id}`, or add the game's LICENSE (MIT, Copyright (c) 2026 Chris Pearson (@snoaper), unless the owner chose other terms)."),
        ))
    } else {
        pg::notice_problem(id, dir).map(|p| (p.what, p.fix))
    };
    out.push(check("licences", lic_problem, format!("README.md, LICENSE (game) and {} (engine, MIT)", pg::ENGINE_NOTICE_FILE)));

    // 6. The page loads only files that are in the package.
    let refs = local_refs(&html);
    let missing: Vec<&String> = refs.iter().filter(|r| r.contains("..") || !dir.join(r.as_str()).is_file()).collect();
    out.push(check(
        "references",
        if html.is_empty() {
            Some(("index.html is missing".to_string(), format!("Run `cargo xtask publish {id}` again.")))
        } else {
            (!missing.is_empty()).then(|| {
                (
                    format!("index.html points at files that are not in the package: {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
                    format!("Add them to the game (Trunk copies only what apps/{id}/index.html lists) or remove the reference."),
                )
            })
        },
        format!("index.html loads {} local file(s), all present", refs.len()),
    ));

    // 7. The engine files: one wasm, and no stale hashed file the page does not load.
    let stale: Vec<&String> = wasm_files.iter().chain(engine_js.iter()).filter(|f| !refs.contains(f)).collect();
    out.push(check(
        "engine_files",
        if wasm_files.len() != 1 {
            Some((format!("expected one .wasm, found {}", wasm_files.len()), format!("Run `cargo xtask publish {id}` again (it rebuilds the folder from scratch).")))
        } else {
            (!stale.is_empty()).then(|| {
                (
                    format!("files the page does not load (stale build files?): {}", stale.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
                    format!("Run `cargo xtask publish {id}` again (it rebuilds the folder from scratch)."),
                )
            })
        },
        format!("{} and {} loaded by the page, nothing stale", wasm_files.join(", "), engine_js.join(", ")),
    ));

    // 8. evani.json matches evani.toml.
    let ej = read("evani.json");
    let ej_problem = match ej.as_deref().map(serde_json::from_slice::<Value>) {
        None => Some(("evani.json is missing".to_string(), format!("Run `cargo xtask publish {id}` again (it writes one from evani.toml)."))),
        Some(Err(err)) => Some((format!("evani.json is not valid JSON ({err})"), "Fix or delete it; the publish command writes a fresh one.".to_string())),
        Some(Ok(v)) => {
            let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
            let fix = format!("Set it to match apps/{id}/evani.toml: kit \"{}\", look \"{}\", freeRoam {}.", e.kit, e.look, e.free_roam.unwrap_or(false));
            if !v.is_object() {
                Some(("evani.json is not an object".to_string(), fix))
            } else if s("kit").as_deref() != Some(e.kit.as_str()) {
                Some((format!("evani.json kit is {:?}, evani.toml says \"{}\"", s("kit"), e.kit), fix))
            } else if s("look").as_deref() != Some(e.look.as_str()) {
                Some((format!("evani.json look is {:?}, evani.toml says \"{}\"", s("look"), e.look), fix))
            } else if e.free_roam.is_some() && v.get("freeRoam").and_then(Value::as_bool) != e.free_roam {
                Some(("evani.json freeRoam does not match [playground] free_roam".to_string(), fix))
            } else {
                None
            }
        }
    };
    out.push(check("evani_json", ej_problem, format!("kit {}, look {}, freeRoam {}", e.kit, e.look, e.free_roam.unwrap_or(false))));

    // 9. A playground with free roam.
    let fr_problem = match e.free_roam {
        None => Some((
            "the game is not a playground (no [playground] in evani.toml)".to_string(),
            format!("Add `[playground]` with `free_roam = true` to apps/{id}/evani.toml (docs/PLAYGROUNDS.md)."),
        )),
        Some(false) => Some(("[playground] free_roam is not true".to_string(), "Set `free_roam = true` (docs/PLAYGROUNDS.md).".to_string())),
        Some(true) if !e.playground_problems.is_empty() => {
            Some((e.playground_problems.join("; "), "Close the free-roam gaps `cargo xtask check` lists (docs/PLAYGROUNDS.md).".to_string()))
        }
        Some(true) => None,
    };
    out.push(check("free_roam", fr_problem, "playground with free roam (declared, in the kit, tested)".to_string()));

    // 10. The smooth start.
    out.push(check(
        "smooth_start",
        (!html.contains(SMOOTH_MARK)).then(|| {
            ("index.html has no smooth start".to_string(), format!("Run `cargo xtask publish {id}` again (it rewrites Trunk's bootstrap)."))
        }),
        "engine starts after the loader's logo has landed".to_string(),
    ));
    out
}

// ---- size report -------------------------------------------------------------------

/// One module's share: the full ship build minus a build with it off
/// (raw, gzip, brotli of wasm + js + html).
#[derive(Debug, Clone)]
pub struct ModuleRow {
    pub module: String,
    pub raw: i64,
    pub gzip: i64,
    pub brotli: i64,
}

fn build_totals(dist: &Path) -> (u64, u64, u64) {
    let Ok(rd) = std::fs::read_dir(dist) else { return (0, 0, 0) };
    rd.flatten()
        .filter(|f| {
            let n = f.file_name().to_string_lossy().into_owned();
            n.ends_with(".wasm") || n.ends_with(".js") || n.ends_with(".html")
        })
        .filter_map(|f| std::fs::read(f.path()).ok())
        .fold((0, 0, 0), |(r, g, b), bytes| (r + bytes.len() as u64, g + gzip_len(&bytes), b + brotli_len(&bytes)))
}

fn variant_dist(app: &str, variant: &str) -> PathBuf {
    workspace_root().join("target").join("evani-sizes").join(format!("{app}-{variant}"))
}

/// One rebuild per listed module with it off, plus the base build.
fn module_rows(app: &AppDir, m: &Manifest) -> Result<Vec<ModuleRow>, String> {
    let (sr, sg, sb) = build_totals(&variant_dist(&app.name, "ship"));
    let mut rows = Vec::new();
    for (module, without) in m.without_each() {
        let variant = format!("ship-no-{}", module.replace([' ', '(', ')'], ""));
        trunk_build(app, &variant, &without)?;
        let (r, g, b) = build_totals(&variant_dist(&app.name, &variant));
        rows.push(ModuleRow { module, raw: sr as i64 - r as i64, gzip: sg as i64 - g as i64, brotli: sb as i64 - b as i64 });
    }
    trunk_build(app, "ship-base", &m.base())?;
    let (r, g, b) = build_totals(&variant_dist(&app.name, "ship-base"));
    rows.push(ModuleRow { module: "base (core+render+kit+look)".to_string(), raw: r as i64, gzip: g as i64, brotli: b as i64 });
    Ok(rows)
}

/// The engine files the 1.5 MB size line is judged on (as `size-report`):
/// the build's wasm, js, html and css, not the loader or the extras.
fn size_line_raw(m: &PackageManifest) -> u64 {
    m.files
        .iter()
        .filter(|(p, _)| p.as_str() != LOADER_FILE && [".wasm", ".js", ".html", ".css"].iter().any(|x| p.ends_with(x)))
        .map(|(_, f)| f.size)
        .sum()
}

/// The files a player's browser fetches: index.html and the package files it
/// loads (not README.md, LICENSE, preview.png, evani.json or the notice).
pub fn page_files(m: &PackageManifest, html: &str) -> Vec<String> {
    let mut out: Vec<String> = std::iter::once("index.html".to_string())
        .chain(local_refs(html))
        .filter(|p| m.files.contains_key(p))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// (raw, gzip, brotli) of the files a player fetches.
pub fn download_totals(m: &PackageManifest, page: &[String]) -> (u64, u64, u64) {
    page.iter().filter_map(|p| m.files.get(p)).fold((0, 0, 0), |(r, g, b), f| (r + f.size, g + f.gzip, b + f.brotli))
}

/// The text of the size report, every figure labelled raw / gzip / brotli.
/// `page` is what the browser fetches ([`page_files`]).
pub fn size_report_lines(m: &PackageManifest, page: &[String], modules: Option<&[ModuleRow]>) -> Vec<String> {
    let mut l = Vec::new();
    match modules {
        Some(rows) => {
            l.push("  by module (full ship build minus a build with it off; wasm+js+html):".to_string());
            l.push(format!("    {:<30} {:>11} {:>11} {:>11}", "module", "raw", "gzip", "brotli"));
            for r in rows {
                let f = |v: i64| if r.module.starts_with("base") { kb(v.max(0) as u64) } else { signed_kb(v) };
                l.push(format!("    {:<30} {:>11} {:>11} {:>11}", r.module, f(r.raw), f(r.gzip), f(r.brotli)));
            }
        }
        None => l.push("  by module: skipped (--quick); run without --quick for the module table".to_string()),
    }
    l.push("  by file:".to_string());
    l.push(format!("    {:<44} {:>11} {:>11} {:>11}", "file", "raw", "gzip", "brotli"));
    let mut files: Vec<(&String, &FileEntry)> = m.files.iter().collect();
    files.sort_by_key(|(_, f)| std::cmp::Reverse(f.size));
    for (p, f) in files {
        l.push(format!("    {:<44} {:>11} {:>11} {:>11}", p, kb(f.size), kb(f.gzip), kb(f.brotli)));
    }
    let (raw, gz, br) = m.totals();
    l.push(format!("    {:<44} {:>11} {:>11} {:>11}", "total", kb(raw), kb(gz), kb(br)));
    l.push(format!(
        "  10 MB cap (raw, whole folder, uncompressed; what Evani.App checks on upload): {} of {}, {}",
        pg::mb(raw),
        pg::mb(pg::PACKAGE_CAP_BYTES),
        if raw > pg::PACKAGE_CAP_BYTES { "OVER" } else { "under" }
    ));
    let line = size_line_raw(m);
    l.push(format!(
        "  1.5 MB size line (raw wasm+js+html+css of the build, as `cargo xtask size-report`): {}, {}",
        kb(line),
        if line > LIMIT_BYTES { "OVER" } else { "under" }
    ));
    let (draw, dgz, dbr) = download_totals(m, page);
    l.push(format!(
        "  download (what a player's browser fetches: index.html and the {} file(s) it loads): {} raw, {} gzip, {} brotli",
        page.len().saturating_sub(1),
        kb(draw),
        kb(dgz),
        kb(dbr)
    ));
    l.push(format!("  whole folder (what is uploaded, incl. README, licences, preview): {} raw, {} gzip, {} brotli", kb(raw), kb(gz), kb(br)));
    l
}

// ---- version history ---------------------------------------------------------------

/// `.evani/publish/<id>/`: summaries and the published history, outside the package.
pub fn history_dir(root: &Path, app: &str) -> PathBuf {
    root.join(".evani").join("publish").join(app)
}

/// `v5` or `5` -> 5.
pub fn parse_version(v: &str) -> Option<u32> {
    v.trim().trim_start_matches(['v', 'V']).parse().ok().filter(|n| *n > 0)
}

/// The version recorded as live, with its manifest.
pub fn live_version(hist: &Path) -> Option<(String, PackageManifest)> {
    let live: Value = serde_json::from_str(&std::fs::read_to_string(hist.join("live.json")).ok()?).ok()?;
    let version = live.get("version")?.as_str()?.to_string();
    let rec: Value = serde_json::from_str(&std::fs::read_to_string(hist.join("published").join(format!("{version}.json"))).ok()?).ok()?;
    let m = PackageManifest::from_json(rec.get("files")?).ok()?;
    Some((version, m))
}

/// One above the highest version recorded, or `None` with no history.
pub fn next_version(hist: &Path) -> Option<String> {
    let rd = std::fs::read_dir(hist.join("published")).ok()?;
    let top = rd
        .flatten()
        .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).and_then(parse_version))
        .max()?;
    Some(format!("v{}", top + 1))
}

fn write_ignore(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let ig = dir.join(".gitignore");
    if !ig.exists() {
        std::fs::write(&ig, "*\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Records the package in `pkg` as version `version`, now live.
pub fn mark_published(hist: &Path, app: &str, version: &str, pkg: &Path, now: &str) -> Result<String, String> {
    let n = parse_version(version).ok_or_else(|| format!("`{version}` is not a version (use v5, v6, ...)"))?;
    let version = format!("v{n}");
    if !pkg.join("index.html").is_file() {
        return Err(format!("{}: no package here (no index.html)", pkg.display()));
    }
    let m = PackageManifest::scan(pkg)?;
    let id = m.package_id();
    let dir = hist.join("published");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_ignore(hist.parent().unwrap_or(hist))?;
    let file = dir.join(format!("{version}.json"));
    if let Ok(text) = std::fs::read_to_string(&file) {
        let old: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        if old.get("package_id").and_then(Value::as_str) != Some(id.as_str()) {
            return Err(format!("{version} is already recorded with another package ({}); versions are never rewritten", file.display()));
        }
    }
    let (raw, gz, br) = m.totals();
    let rec = json!({
        "game": app, "version": version, "marked_at": now, "package_id": id,
        "package_dir": pkg.display().to_string(), "raw": raw, "gzip": gz, "brotli": br, "files": m.to_json(),
    });
    std::fs::write(&file, serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    let live = json!({"game": app, "version": version, "package_id": id, "marked_at": now});
    std::fs::write(hist.join("live.json"), serde_json::to_string_pretty(&live).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    Ok(version)
}

// ---- summary -----------------------------------------------------------------------

pub struct Summary<'a> {
    pub app: &'a str,
    pub title: &'a str,
    pub dry_run: bool,
    pub created: &'a str,
    pub package_dir: &'a Path,
    pub manifest: &'a PackageManifest,
    /// What a player's browser fetches ([`page_files`]).
    pub page: &'a [String],
    pub checks: &'a [Check],
    pub modules: Option<&'a [ModuleRow]>,
    pub live: Option<&'a str>,
    pub diff: &'a Diff,
    pub suggested: Option<String>,
    pub preview_url: Option<String>,
    pub swapped: bool,
    /// Per-tier verdicts against catalog/tiers.json `download_targets` (S7-1).
    pub tiers: &'a crate::tier_report::TierReport,
}

impl Summary<'_> {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    pub fn skip(&self) -> bool {
        self.live.is_some() && self.diff.unchanged()
    }

    pub fn to_json(&self) -> Value {
        let (raw, gz, br) = self.manifest.totals();
        let (draw, dgz, dbr) = download_totals(self.manifest, self.page);
        let (dr, dg, db) = self.diff.size_change();
        let files: Vec<Value> = {
            let mut f: Vec<(&String, &FileEntry)> = self.manifest.files.iter().collect();
            f.sort_by_key(|(_, e)| std::cmp::Reverse(e.size));
            f.iter().map(|(p, e)| json!({"path": p, "raw": e.size, "gzip": e.gzip, "brotli": e.brotli, "sha256": e.sha256})).collect()
        };
        let modules = self.modules.map(|rows| {
            rows.iter().map(|r| json!({"module": r.module, "raw": r.raw, "gzip": r.gzip, "brotli": r.brotli})).collect::<Vec<_>>()
        });
        let rows: Vec<Value> = self
            .diff
            .rows
            .iter()
            .map(|r| json!({"path": r.path, "status": r.change.label(), "old_size": r.old.as_ref().map(|f| f.size), "new_size": r.new.as_ref().map(|f| f.size)}))
            .collect();
        json!({
            "schema": SUMMARY_SCHEMA,
            "game": self.app,
            "title": self.title,
            "created": self.created,
            "dry_run": self.dry_run,
            "package_dir": self.package_dir.display().to_string(),
            "package_swapped_in": self.swapped,
            "package_id": self.manifest.package_id(),
            "ready": self.passed() && !self.skip(),
            "skip": self.skip(),
            "skip_reason": self.skip().then(|| format!("nothing changed since {}", self.live.unwrap_or("?"))),
            "live_version": self.live,
            "suggested_version": if self.skip() { None } else { self.suggested.clone() },
            "preview_url": self.preview_url,
            "sizes": {
                "raw": raw, "gzip": gz, "brotli": br, "basis": "whole folder (what is uploaded)",
                "download": {"raw": draw, "gzip": dgz, "brotli": dbr, "files": self.page, "basis": "index.html and the files it loads (what a player's browser fetches)"},
                "cap_bytes": pg::PACKAGE_CAP_BYTES, "cap_basis": "raw, whole folder, uncompressed",
                "under_cap": raw <= pg::PACKAGE_CAP_BYTES,
                "size_line_bytes": LIMIT_BYTES, "size_line_raw": size_line_raw(self.manifest),
                "tier_targets": self.tiers.to_json(),
                "modules": modules, "files": files,
            },
            "diff": {
                "against": self.live,
                "note": if self.live.is_none() { Some("no previous manifest: every file counts as NEW; record the live version with --mark-published after the owner's click") } else { None },
                "new": self.diff.count(Change::New), "modified": self.diff.count(Change::Modified),
                "same": self.diff.count(Change::Same), "deleted": self.diff.count(Change::Deleted),
                "change_basis": "whole folder", "change_raw": dr, "change_gzip": dg, "change_brotli": db,
                "redownload_gzip": self.diff.redownload_gzip(self.page),
                "redownload_basis": "new or modified files the page loads (gzip)",
                "files": rows,
            },
            "preflight": {
                "passed": self.passed(),
                "checks": self.checks.iter().map(|c| json!({"id": c.id, "ok": c.ok, "detail": c.detail, "fix": if c.ok { None } else { Some(&c.fix) }})).collect::<Vec<_>>(),
            },
        })
    }

    pub fn to_markdown(&self) -> String {
        let raw = self.manifest.totals().0;
        let (_, gz, br) = download_totals(self.manifest, self.page);
        let mut s = format!("# Publish summary: {} ({})\n\n", self.title, self.app);
        s.push_str(&format!("- Made: {}{}\n", self.created, if self.dry_run { " (dry run)" } else { "" }));
        s.push_str(&format!("- Package: `{}`\n", self.package_dir.display()));
        let verdict = if !self.passed() {
            "not ready: the preflight failed (below)".to_string()
        } else if self.skip() {
            format!("nothing changed since {}: skip, no publish needed", self.live.unwrap_or("?"))
        } else {
            format!("ready to publish as {}", self.suggested.clone().unwrap_or_else(|| "the next version (no history recorded yet)".to_string()))
        };
        s.push_str(&format!("- Verdict: {verdict}\n"));
        s.push_str(&format!("- Live: {}\n", self.live.unwrap_or("no version recorded (no previous manifest)")));
        if let Some(u) = &self.preview_url {
            s.push_str(&format!("- Preview: {u}\n"));
        }
        s.push_str(&format!(
            "\n## Size\n\n- Download (index.html and the files it loads): {} gzip, {} brotli\n- On disk: {} raw of the {} cap (the cap is on the raw whole folder)\n",
            kb(gz),
            kb(br),
            pg::mb(raw),
            pg::mb(pg::PACKAGE_CAP_BYTES)
        ));
        s.push_str("\n```text\n");
        for l in size_report_lines(self.manifest, self.page, self.modules).into_iter().chain(self.tiers.lines()) {
            s.push_str(&l);
            s.push('\n');
        }
        s.push_str("```\n\n## Changes\n\n");
        s.push_str(&diff_lines(self.diff, self.live, self.page).join("\n"));
        s.push_str("\n\n## Preflight\n\n");
        for c in self.checks {
            s.push_str(&format!("- {} {}: {}{}\n", if c.ok { "PASS" } else { "FAIL" }, c.id, c.detail, if c.ok { String::new() } else { format!(" (fix: {})", c.fix) }));
        }
        s.push_str("\nOnly the owner's click on Evani.App's publish card uploads this package.\n");
        s
    }
}

/// The diff as text; `page` is what the browser fetches ([`page_files`]).
pub fn diff_lines(d: &Diff, live: Option<&str>, page: &[String]) -> Vec<String> {
    let mut l = Vec::new();
    match live {
        None => l.push("  against: no previous manifest (no version recorded as live), so every file counts as NEW".to_string()),
        Some(v) => l.push(format!("  against: {v} (recorded as live)")),
    }
    for r in &d.rows {
        let size = match (&r.old, &r.new) {
            (Some(o), Some(n)) if o.size != n.size => format!("{} -> {}", kb(o.size), kb(n.size)),
            (_, Some(n)) => kb(n.size),
            (Some(o), None) => kb(o.size),
            (None, None) => String::new(),
        };
        l.push(format!("    {:<9} {:<44} {size}", r.change.label(), r.path));
    }
    let (dr, dg, db) = d.size_change();
    l.push(format!(
        "  {} new, {} modified, {} same, {} deleted; whole-folder size change {} raw, {} gzip, {} brotli; a returning player's browser fetches {} gzip again (changed files the page loads)",
        d.count(Change::New),
        d.count(Change::Modified),
        d.count(Change::Same),
        d.count(Change::Deleted),
        signed_kb(dr),
        signed_kb(dg),
        signed_kb(db),
        kb(d.redownload_gzip(page))
    ));
    if live.is_some() && d.unchanged() {
        l.push(format!("  nothing changed since {}, skip: no publish needed", live.unwrap_or("?")));
    }
    l
}

/// Writes summary.json, summary.md and package.json into `hist`; returns the json path.
pub fn write_summary(hist: &Path, s: &Summary) -> Result<PathBuf, String> {
    write_ignore(hist.parent().unwrap_or(hist))?;
    std::fs::create_dir_all(hist).map_err(|e| format!("{}: {e}", hist.display()))?;
    let path = hist.join("summary.json");
    std::fs::write(&path, serde_json::to_string_pretty(&s.to_json()).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    std::fs::write(hist.join("summary.md"), s.to_markdown()).map_err(|e| e.to_string())?;
    let pkg = json!({"game": s.app, "package_id": s.manifest.package_id(), "files": s.manifest.to_json()});
    std::fs::write(hist.join("package.json"), serde_json::to_string_pretty(&pkg).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    Ok(path)
}

// ---- local preview server ----------------------------------------------------------

/// Maps a request to a file of the package, which is served under `/<app>/`
/// (a subfolder, as on play.evani.app, so root-absolute paths would 404 here too).
pub fn resolve(pkg: &Path, app: &str, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let rel = path.strip_prefix(&format!("/{app}/"))?;
    let rel = if rel.is_empty() { "index.html" } else { rel };
    // No empty, parent, drive, share or percent-encoded segment: a rooted
    // part would make `join` leave the package.
    if rel.split(['/', '\\']).any(|p| p.is_empty() || p == ".." || p == "." || p.contains([':', '%'])) {
        return None;
    }
    let path = pkg.join(rel);
    path.starts_with(pkg).then_some(path)
}

fn respond(mut s: TcpStream, pkg: &Path, app: &str) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let mut got = Vec::new();
    while !got.windows(4).any(|w| w == b"\r\n\r\n") && got.len() < 16 * 1024 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&got);
    let mut parts = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let path = target.split(['?', '#']).next().unwrap_or("/");
    if path == "/" || path == format!("/{app}") {
        let header = format!("HTTP/1.1 302 Found\r\nLocation: /{app}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = s.write_all(header.as_bytes());
        return;
    }
    let file = resolve(pkg, app, target).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b)));
    let (status, ctype, body) = match (method, file) {
        ("GET" | "HEAD", Some((p, b))) => ("200 OK", crate::bench::content_type(&p), b),
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(header.as_bytes());
    if method != "HEAD" {
        let _ = s.write_all(&body);
    }
}

/// Serves `pkg` at http://localhost:<port>/<app>/ until the process is stopped.
fn serve(listener: TcpListener, pkg: PathBuf, app: String) -> ! {
    let pkg = std::sync::Arc::new(pkg);
    let app = std::sync::Arc::new(app);
    loop {
        if let Ok((s, _)) = listener.accept() {
            let (p, a) = (pkg.clone(), app.clone());
            std::thread::spawn(move || respond(s, &p, &a));
        }
    }
}

// ---- the command -------------------------------------------------------------------

struct Opts {
    app: String,
    dry_run: bool,
    serve: bool,
    quick: bool,
    port: u16,
    out: Option<PathBuf>,
    from: Option<PathBuf>,
    mark: Option<String>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let takes_value = ["--port", "--out", "--from", "--mark-published"];
    let mut positional = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if takes_value.contains(&a.as_str()) {
            skip = true;
        } else if a.starts_with("--") {
            if !["--dry-run", "--serve", "--quick"].contains(&a.as_str()) {
                return Err(format!("publish: unknown option {a}\n{}", usage()));
            }
        } else {
            positional.push(a.clone());
        }
    }
    let app = positional.first().cloned().ok_or_else(usage)?;
    let port = match value("--port") {
        Some(p) => p.parse().map_err(|_| format!("publish: --port {p} is not a port"))?,
        None => DEFAULT_PORT,
    };
    Ok(Opts {
        app,
        dry_run: args.iter().any(|a| a == "--dry-run"),
        serve: args.iter().any(|a| a == "--serve"),
        quick: args.iter().any(|a| a == "--quick"),
        port,
        out: value("--out").map(PathBuf::from),
        from: value("--from").map(PathBuf::from),
        mark: value("--mark-published"),
    })
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map_or_else(|_| p.to_path_buf(), |d| d.join(p)) }
}

/// What the package is checked against, from the game's files.
fn expect_for(root: &Path, app: &AppDir) -> Result<Expect, String> {
    let toml = std::fs::read_to_string(app.dir.join("evani.toml")).map_err(|e| e.to_string())?;
    let free_roam = pg::parse(&toml)?.map(|p| p.free_roam);
    let page = std::fs::read_to_string(app.dir.join("index.html")).unwrap_or_default();
    Ok(Expect {
        app: app.name.clone(),
        kit: app.manifest.kit.clone(),
        look: app.manifest.look.clone(),
        title: page_title(&page).unwrap_or_else(|| app.manifest.name.clone()),
        free_roam,
        playground_problems: pg::problems(root, &app.name, &app.manifest.kit, &toml).into_iter().map(|p| p.what).collect(),
        known_markers: known_markers(root),
    })
}

fn print_checks(checks: &[Check]) {
    for c in checks {
        println!("  {} {:<13} {}", if c.ok { "PASS" } else { "FAIL" }, c.id, c.detail);
        if !c.ok {
            println!("       fix: {}", c.fix);
        }
    }
}

/// `cargo xtask package-check <folder> <app>`: the preflight on an existing
/// package folder; prints every check and returns how many failed.
pub fn preflight_folder(app: &AppDir, dir: &Path) -> Result<usize, String> {
    let e = expect_for(&workspace_root(), app)?;
    let checks = preflight(dir, &e);
    println!("publish preflight ({}):", app.name);
    print_checks(&checks);
    Ok(checks.iter().filter(|c| !c.ok).count())
}

/// Result of putting a package together and checking it.
#[derive(Debug)]
pub struct Packaged {
    pub checks: Vec<Check>,
    pub manifest: PackageManifest,
    /// Where the package is now (the destination after a swap, or the dry-run folder).
    pub dir: PathBuf,
    pub swapped: bool,
    pub notes: Vec<String>,
}

/// Assembles the package from `dist` (+ `carry`) and checks it. A dry run
/// leaves it in `dry_dir`; otherwise it is staged next to `dest` and swapped
/// in only when every check passes. A failed run leaves `dest` untouched and
/// moves the rejected package to `failed_dir`, so the summary describes a
/// folder that exists.
pub fn package_into(
    root: &Path,
    dist: &Path,
    carry: Option<&Path>,
    dest: &Path,
    dry_dir: Option<&Path>,
    failed_dir: &Path,
    e: &Expect,
) -> Result<Packaged, String> {
    if dry_dir.is_none() {
        replaceable(dest)?;
    }
    let staging = dry_dir.map_or_else(|| staging_for(dest), Path::to_path_buf);
    let built = assemble(root, dist, carry, &staging, e).and_then(|notes| {
        let checks = preflight(&staging, e);
        PackageManifest::scan(&staging).map(|m| (notes, checks, m))
    });
    let (notes, checks, manifest) = match built {
        Ok(v) => v,
        Err(err) => {
            if dry_dir.is_none() {
                let _ = std::fs::remove_dir_all(&staging);
            }
            return Err(err);
        }
    };
    if dry_dir.is_some() {
        return Ok(Packaged { checks, manifest, dir: staging, swapped: false, notes });
    }
    if checks.iter().any(|c| !c.ok) {
        if failed_dir.exists() {
            std::fs::remove_dir_all(failed_dir).map_err(|err| format!("{}: {err}", failed_dir.display()))?;
        }
        if let Some(parent) = failed_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        // A rename when both are on one drive, else a copy.
        let moved = rename_retry(&staging, failed_dir).is_ok() || copy_tree(&staging, failed_dir).is_ok();
        let _ = std::fs::remove_dir_all(&staging);
        if !moved {
            return Err(format!("the preflight failed and the rejected package could not be kept in {}", failed_dir.display()));
        }
        return Ok(Packaged { checks, manifest, dir: failed_dir.to_path_buf(), swapped: false, notes });
    }
    if let Err(err) = swap_into(&staging, dest) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }
    Ok(Packaged { checks, manifest, dir: dest.to_path_buf(), swapped: true, notes })
}

/// Dev-only apps that are never a game and never published.
const DEV_ONLY_APPS: &[&str] = &["model-view"];

/// Why `cargo xtask publish <app>` refuses a dev-only app, with a `fix:`
/// line (docs/EVANI_MODEL_TAB.md 4.1). `None` for every other app.
pub fn dev_only_refusal(app: &str) -> Option<String> {
    let name = app.trim_start_matches("apps/").trim_end_matches('/');
    DEV_ONLY_APPS.contains(&name).then(|| {
        format!(
            "{name} is a dev-only tool (the page behind Evani.App's Model tab), never a game, so it is never published\n  \
             fix: publish a game or playground instead (cargo xtask publish <game>); open {name} with cargo xtask live --models"
        )
    })
}

pub fn cmd_publish(args: &[String]) -> Result<(), String> {
    let o = parse_opts(args)?;
    if let Some(why) = dev_only_refusal(&o.app) {
        return Err(format!("publish: {why}"));
    }
    let root = workspace_root();
    let app = load_app(&o.app)?;
    let hist = history_dir(&root, &app.name);
    let dest = o.out.as_deref().map_or_else(|| root.join("publish").join(&app.name), absolute);
    if dest.starts_with(&hist) || hist.starts_with(&dest) {
        return Err(format!("publish: --out {} overlaps the summary folder {}; pick another folder", dest.display(), hist.display()));
    }
    let now = crate::review::utc_string(SystemTime::now());

    if let Some(v) = &o.mark {
        let version = mark_published(&hist, &app.name, v, &dest, &now)?;
        println!("{}: recorded {} as live {version} ({})", app.name, dest.display(), hist.join("published").join(format!("{version}.json")).display());
        println!("  the next `cargo xtask publish {}` diffs against {version}", app.name);
        return Ok(());
    }

    // Bind the preview port first, so a busy port fails before the build.
    let listener = if o.serve {
        Some(TcpListener::bind(("127.0.0.1", o.port)).map_err(|e| format!("publish: port {} is busy ({e}); pass --port <P>", o.port))?)
    } else {
        None
    };

    // 1. The ship build (same guards as `cargo xtask build <id> --ship`).
    playground_guard(&app)?;
    let m = app.manifest.clone().ship();
    let e = expect_for(&root, &app)?;
    println!(
        "{} (\"{}\"): publish{} from apps/{}/evani.toml: kit {}, look {}, features [{}]",
        app.name,
        e.title,
        if o.dry_run { " --dry-run" } else { "" },
        app.name,
        m.kit,
        m.look,
        m.features().join(", ")
    );
    let ship = trunk_build(&app, "ship", &m)?;
    ship_guard(&app.name, &ship)?;
    let dist = variant_dist(&app.name, "ship");
    crate::build_facts::write(&app, &ship, &dist)?;
    playground_size_guard(&app)?;
    let modules = if o.quick {
        None
    } else {
        let extra = m.without_each().len() + 1;
        println!("size by module: {extra} more ship build(s), one per optional module off plus the base (--quick skips them)");
        Some(module_rows(&app, &m)?)
    };

    // 2-3. Package, preflight, swap.
    let carry = o.from.as_deref().map(absolute).or_else(|| {
        let own = root.join("publish").join(&app.name);
        if dest.join("index.html").is_file() { Some(dest.clone()) } else { own.is_dir().then_some(own) }
    });
    let dry_dir = hist.join("dry-run");
    let failed_dir = hist.join("failed");
    write_ignore(hist.parent().unwrap_or(&hist))?;
    std::fs::create_dir_all(&hist).map_err(|err| err.to_string())?;
    let p = package_into(&root, &dist, carry.as_deref(), &dest, o.dry_run.then_some(dry_dir.as_path()), &failed_dir, &e)?;
    let state = if o.dry_run {
        "dry run: the destination is untouched".to_string()
    } else if p.swapped {
        format!("swapped into {}", dest.display())
    } else {
        format!("rejected by the preflight and kept here to look at; {} was left as it was", dest.display())
    };
    println!("\npackage: {} ({state})", p.dir.display());
    if let Some(c) = &carry {
        println!("  carried over from {}: {}", c.display(), CARRY_OVER.join(", "));
    }
    for n in &p.notes {
        println!("  {n}");
    }

    println!("\npreflight:");
    print_checks(&p.checks);
    let page = page_files(&p.manifest, &std::fs::read_to_string(p.dir.join("index.html")).unwrap_or_default());
    println!("\nsize report:");
    for l in size_report_lines(&p.manifest, &page, modules.as_deref()) {
        println!("{l}");
    }
    let tiers = crate::tier_report::report(
        &root,
        evani_guide::render_paths::style_of_look(&root, &m.look).as_deref(),
        m.models,
        &p.dir,
        &p.manifest,
        download_totals(&p.manifest, &page).1,
    );
    for l in tiers.lines() {
        println!("{l}");
    }
    let live = live_version(&hist);
    let d = diff(&live.as_ref().map(|(_, m)| m.clone()).unwrap_or_default(), &p.manifest);
    println!("\nchanges:");
    for l in diff_lines(&d, live.as_ref().map(|(v, _)| v.as_str()), &page) {
        println!("{l}");
    }

    let preview_url = listener.as_ref().map(|_| format!("http://localhost:{}/{}/", o.port, app.name));
    let summary = Summary {
        app: &app.name,
        title: &e.title,
        dry_run: o.dry_run,
        created: &now,
        package_dir: &p.dir,
        manifest: &p.manifest,
        page: &page,
        checks: &p.checks,
        modules: modules.as_deref(),
        live: live.as_ref().map(|(v, _)| v.as_str()),
        diff: &d,
        suggested: next_version(&hist),
        preview_url: preview_url.clone(),
        swapped: p.swapped,
        tiers: &tiers,
    };
    let path = write_summary(&hist, &summary)?;
    println!("\nsummary: {} (and summary.md; outside the package, never uploaded)", path.display());
    if !summary.passed() {
        let n = p.checks.iter().filter(|c| !c.ok).count();
        return Err(format!(
            "publish: the preflight failed ({n} check(s)); {}",
            if o.dry_run { "nothing was changed".to_string() } else { format!("{} was left as it was", dest.display()) }
        ));
    }
    if summary.skip() {
        println!("verdict: nothing changed since the live version, skip: no publish needed");
    } else {
        println!(
            "verdict: ready{}; only the owner's click on Evani.App's publish card uploads it, then record it with `cargo xtask publish {} --mark-published <version>`",
            summary.suggested.as_ref().map_or_else(|| " (no version history yet: the next version is one above the live one)".to_string(), |v| format!(" as {v}")),
            app.name
        );
    }
    if let (Some(listener), Some(url)) = (listener, preview_url) {
        println!("\npreview: {url}  (free roam: {url}?mode=free; stop with Ctrl+C)");
        let _ = std::io::stdout().flush();
        serve(listener, p.dir, app.name.clone());
    }
    Ok(())
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
