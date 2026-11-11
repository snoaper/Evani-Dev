//! `cargo xtask commons`: Evani.commons asset versions (docs/EVANI_COMMONS.md).
//!
//! An asset version is one procedural library asset in one art style, as an
//! Asset Studio round left it: its code change against the engine commit it
//! was built on, the live params of its scored picture, its independent score
//! and the pictures. This module packs one into a small `evani-commons/1`
//! folder (`export`), checks one (`verify`), adopts one into a project
//! (`import`: a 3-way merge per file, all or nothing) and answers the share
//! hint (`gate`: the server's own render and score decide).
//!
//! Nothing here uploads anything: Evani.App's Share card does, and only on the
//! owner's click. Package pictures stay in the package folder: the importer
//! writes only source files, so they never reach a game's download.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FORMAT: &str = "evani-commons/1";
const ADOPTED_FORMAT: &str = "evani-commons-adopted/1";
/// The share floor when nothing is published for an asset x style yet.
pub const FLOOR: f64 = 3.0;
const MAX_FILES: usize = 64;
const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_ORIGINAL: u64 = 8 * 1024 * 1024;
const MAX_TOTAL: u64 = 12 * 1024 * 1024;
const REF_MAX_SIDE: u32 = 1600;
const ROUND_MAX_SIDE: u32 = 1024;
const MAX_PICTURES: usize = 6;
const JPEG_QUALITY: u8 = 85;

/// Where a package's code may go: the procedural library, its base, the
/// material crates the library's assets paint with, the grass module and the
/// proof game's dev scenes. Anything else stays out of a package.
pub const ALLOWED_DIRS: &[&str] = &[
    "crates/evani-procedural-lib/",
    "crates/evani-procedural/",
    "crates/evani-material-recipes/",
    "crates/evani-material-nature/",
    "crates/evani-material-proc/",
    "crates/evani-material-layers/",
    "crates/evani-grass/",
    "examples/proc-field/",
];
pub const ALLOWED_FILES: &[&str] = &["catalog/procedural/library.json"];
const TEXT_EXT: &[&str] = &["rs", "wgsl", "toml", "json", "md"];

/// Added Rust lines may not use these: shared code is built on the adopter's
/// PC and on the server, and the procedural library needs none of them.
const DENY: &[(&str, &str)] = &[
    ("std::process", "runs other programs"),
    ("Command::new", "runs other programs"),
    ("std::net", "opens network connections"),
    ("TcpStream", "opens network connections"),
    ("UdpSocket", "opens network connections"),
    ("std::fs", "reads or writes files"),
    ("std::env", "reads the environment"),
    ("std::os", "makes operating-system calls"),
    ("std::ffi", "calls foreign code"),
    ("extern\"", "calls foreign code"),
    ("#[link", "links native code"),
    ("#[no_mangle]", "exports native symbols"),
    ("#[unsafe(", "exports native symbols"),
    ("include!(", "pulls in other files"),
    ("asm!(", "runs raw machine code"),
    ("libc::", "makes operating-system calls"),
    ("winapi::", "makes operating-system calls"),
    ("windows_sys::", "makes operating-system calls"),
];
const DENY_STD_GROUP: &[&str] = &["process", "net", "fs", "env", "os", "ffi"];

pub fn usage() -> &'static str {
    "  cargo xtask commons export --asset <key> [--style photoreal] [--commit <ref>|--branch <ref>] [--onto <ref>] [--base <ref>]\n                 --score <0-5> --independent --reviewer-model <m> [--reviewer-account <a>] [--reviewed-at <iso>] [--author-model <m>] [--summary <s>]\n                 [--column N] [--round N] [--reference <png|jpg>] [--reference-original <png>] [--reference-generator <g>] [--reference-prompt <p>] [--reference-made <iso>]\n                 [--picture [r<R>v<V>=]<png|jpg>]... [--params <json file>] [--scene <feature> --view-param <p> --views 0,1 --size WxH]\n                 [--area official|community] [--based-on <hash12>] [--author <name>] [--out <folder>]\n                 [--stats <asset-shoot stats.json>] [--leave-out <examples/proc-field/... file>]... [--categories <round categories.json>] [--dry-run]   (Evani.commons: pack one asset version, with the server's preflight; a round already folded into the main branch packs as the main-branch commit that holds it; never uploads)\n  cargo xtask commons verify <folder>   (content hash, allowed paths, code safety, patch consistency)\n  cargo xtask commons import <folder> [--root <project>] [--dry-run] [--server-score <x>] [--version-id <hash12>] [--check|--no-check]   (adopt: 3-way merge per file, all or nothing)\n  cargo xtask commons gate --score <x> [--independent] [--published <y>|none] [--floor 3.0]   (the share hint; the server decides)\n  cargo xtask commons locked-ref --asset <key> [--style photoreal] --sha256 <hex> [--site <api>]   (Evani.app's locked reference into .evani/commons/locked/<asset>/<style>/: from the project's history, else read from the public API; checked by hash, never uploads)"
}

pub fn cmd_commons(root: &Path, args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("export") => {
            let o = ExportOpts::from_args(root, rest)?;
            let r = export(&o)?;
            println!("{}", r.report);
            if o.dry_run {
                let name = r.dir.file_name().map(|n| n.to_string_lossy().into_owned());
                println!(
                    "commons export --dry-run: {} ({}), not kept",
                    name.unwrap_or_default(),
                    r.content_hash
                );
            } else {
                println!("commons export: {} ({})", r.dir.display(), r.content_hash);
            }
            Ok(())
        }
        Some("verify") => {
            let dir = positional(rest).ok_or("commons verify <folder>")?;
            let p = verify(Path::new(&dir))?;
            println!("{}", p.summary());
            println!("commons verify: ok ({})", p.content_hash);
            Ok(())
        }
        Some("import") => {
            let dir = positional(rest).ok_or("commons import <folder> [--root <project>]")?;
            let target = opt(rest, "--root")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.to_path_buf());
            // `--no-check` states the default out loud (Evani.App's library
            // builder passes it; `library build` checks the result after).
            if has(rest, "--check") && has(rest, "--no-check") {
                return Err("commons import: --check and --no-check together".into());
            }
            let o = ImportOpts {
                root: target,
                dry_run: has(rest, "--dry-run"),
                server_score: opt(rest, "--server-score")
                    .map(|s| parse_score(&s))
                    .transpose()?,
                version_id: opt(rest, "--version-id"),
                check: has(rest, "--check"),
                when: opt(rest, "--when"),
            };
            let r = import(Path::new(&dir), &o, &run_check)?;
            println!("{}", r.report);
            println!(
                "commons import: {} written, {} already as the version, {} merged",
                r.written.len(),
                r.already.len(),
                r.merged.len()
            );
            Ok(())
        }
        Some("gate") => {
            let score = opt(rest, "--score").map(|s| parse_score(&s)).transpose()?;
            let published = match opt(rest, "--published").as_deref() {
                None | Some("none") => None,
                Some(v) => Some(parse_score(v)?),
            };
            let floor = opt(rest, "--floor")
                .map(|s| parse_score(&s))
                .transpose()?
                .unwrap_or(FLOOR);
            let g = gate(score, has(rest, "--independent"), published, floor);
            println!("{}", json!({ "likely": g.likely, "reason": g.reason }));
            Ok(())
        }
        Some("locked-ref") => {
            let asset = opt(rest, "--asset").ok_or("commons locked-ref --asset <key> [--style photoreal] --sha256 <hex> [--site <api>]")?;
            let style = opt(rest, "--style").unwrap_or_else(|| "photoreal".into());
            let sha = opt(rest, "--sha256").ok_or("commons locked-ref: --sha256 <Evani.app's locked reference hash>")?;
            let site = opt(rest, "--site").unwrap_or_else(|| PUBLIC_API.into());
            let r = fetch_locked(root, &asset, &style, &sha, &site)?;
            println!("{}", json!({ "path": r.path, "sha256": sha.to_ascii_lowercase(), "from": r.from }));
            println!("commons locked-ref: {} ({})", r.path, r.from);
            Ok(())
        }
        _ => Err(format!("usage:\n{}", usage())),
    }
}

// ---------- arguments ----------

fn opt(args: &[String], k: &str) -> Option<String> {
    let i = args.iter().position(|a| a == k)?;
    args.get(i + 1).filter(|v| !v.starts_with("--")).cloned()
}
fn opts(args: &[String], k: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == k)
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect()
}
fn has(args: &[String], k: &str) -> bool {
    args.iter().any(|a| a == k)
}
fn positional(args: &[String]) -> Option<String> {
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with("--") {
            skip = !matches!(a.as_str(), "--dry-run" | "--check" | "--independent");
            continue;
        }
        return Some(a.clone());
    }
    None
}
fn parse_score(s: &str) -> Result<f64, String> {
    let v: f64 = s
        .trim()
        .parse()
        .map_err(|_| format!("score `{s}`: a number from 0 to 5"))?;
    if !(0.0..=5.0).contains(&v) || (v * 2.0).fract() != 0.0 {
        return Err(format!("score `{s}`: 0 to 5 in half stars"));
    }
    Ok(v)
}

// ---------- the share hint ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    pub likely: bool,
    pub reason: String,
}

/// The client-side share hint: likely to pass when this version's independent
/// score beats the server's score of the published version (or reaches the
/// floor when nothing is published). The server's own check decides.
pub fn gate(score: Option<f64>, independent: bool, published: Option<f64>, floor: f64) -> Gate {
    let no = |r: String| Gate {
        likely: false,
        reason: r,
    };
    let Some(s) = score else {
        return no("no independent score yet".into());
    };
    if !independent {
        return no("only an independent reviewer's score counts, never the author's own".into());
    }
    match published {
        Some(p) if s > p => Gate {
            likely: true,
            reason: format!(
                "likely to pass: {s} beats the published {p} (the server's own check decides)"
            ),
        },
        Some(p) if s == p => no(format!(
            "{s} equals the published {p}: only a higher score can replace it"
        )),
        Some(p) => no(format!("{s} is below the published {p}")),
        None if s >= floor => Gate {
            likely: true,
            reason: format!(
                "likely to pass: nothing is published yet and {s} reaches the floor {floor}"
            ),
        },
        None => no(format!(
            "{s} is below the floor {floor} for a first version"
        )),
    }
}

// ---------- paths and code safety ----------

/// Why a path may not travel in a package, or None when it may.
pub fn path_problem(p: &str) -> Option<String> {
    if p.is_empty() || p.len() > 200 {
        return Some(format!("`{p}`: not a usable path"));
    }
    if !p
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
    {
        return Some(format!(
            "`{p}`: only plain letters, digits, `.`, `_`, `-` and `/` in paths"
        ));
    }
    if p.starts_with('/') || p.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return Some(format!("`{p}`: not a plain relative path"));
    }
    let name = p.rsplit('/').next().unwrap_or(p);
    if name == "Cargo.lock" {
        return Some(format!("`{p}`: a Cargo.lock never travels in a package"));
    }
    if name == "build.rs" {
        return Some(format!(
            "`{p}`: build scripts never travel in a package (they run on the adopter's PC)"
        ));
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if !TEXT_EXT.contains(&ext) {
        return Some(format!(
            "`{p}`: only text source files travel (.rs, .wgsl, .toml, .json, .md)"
        ));
    }
    if !(ALLOWED_FILES.contains(&p) || ALLOWED_DIRS.iter().any(|d| p.starts_with(d))) {
        return Some(format!(
            "`{p}`: outside the procedural library and material crates"
        ));
    }
    None
}

fn is_word_at(line: &str, i: usize, w: &str) -> bool {
    let b = line.as_bytes();
    let before = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
    let j = i + w.len();
    let after = j >= b.len() || !(b[j].is_ascii_alphanumeric() || b[j] == b'_');
    before && after
}
fn has_word(line: &str, w: &str) -> bool {
    line.match_indices(w).any(|(i, _)| is_word_at(line, i, w))
}

/// What an added Rust line may not do, if anything.
pub fn line_problem(line: &str) -> Option<&'static str> {
    let t = line.trim_start();
    if t.starts_with("//") {
        return None;
    }
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    for (tok, why) in DENY {
        if compact.contains(tok) {
            return Some(why);
        }
    }
    if has_word(line, "unsafe") {
        return Some("uses unsafe code");
    }
    if (compact.contains("include_str!(") || compact.contains("include_bytes!("))
        && compact.contains("..")
    {
        return Some("pulls in files from outside its crate");
    }
    // `use std::{fs, process}` and the like.
    if let Some(i) = compact.find("std::{") {
        let inner = &compact[i + 6..];
        let inner = inner.split('}').next().unwrap_or(inner);
        if inner
            .split(',')
            .any(|n| DENY_STD_GROUP.contains(&n.split("::").next().unwrap_or(n)))
        {
            return Some("uses the standard library's files, processes, network or environment");
        }
    }
    None
}

/// A Cargo.toml may change only its `[features]` table: no new dependencies,
/// no build or proc-macro crates, no new or removed crates.
pub fn cargo_toml_problem(path: &str, base: Option<&[u8]>, after: Option<&[u8]>) -> Option<String> {
    let (Some(b), Some(a)) = (base, after) else {
        return Some(format!("`{path}`: a package may not add or remove a crate"));
    };
    let parse = |x: &[u8]| -> Option<toml::Table> {
        let mut t: toml::Table = std::str::from_utf8(x).ok()?.parse().ok()?;
        t.remove("features");
        Some(t)
    };
    match (parse(b), parse(a)) {
        (Some(x), Some(y)) if x == y => None,
        (Some(_), Some(_)) => Some(format!(
            "`{path}`: a package may change only the [features] table of a Cargo.toml"
        )),
        _ => Some(format!("`{path}`: not a readable Cargo.toml")),
    }
}

// ---------- the server's own preflight, mirrored ----------
//
// Evani.App and the Evani.commons server both run `patchProblems`, the
// triangle budget and the engine pack check (Evani.App's
// platform/protocol/commons.ts). The packer runs the same rules before a
// package is made, so a refusal is caught here with a plain reason instead
// of costing one of the day's server checks.

/// The first engine commit with the server's render app as the server builds
/// it (examples/proc-field/apps/commons-render): a package on an older base
/// can't be built by the server ("the build failed", Cube r6 on 5f1dd26e).
pub const RENDER_APP_COMMIT: &str = "ea0dd44064fab2bde6084ad3c877e08a68349e92";
/// The dev scene: kept for adopters, never in the server's build
/// (`DEV_SCENE_PREFIX`); the only files `--leave-out` may drop.
pub const DEV_SCENE_PREFIX: &str = "examples/proc-field/";
/// The tier the server renders and counts triangles at (`COMMONS_RENDER_TIER`).
pub const RENDER_TIER: &str = "high";
/// The upload rules' flat model limit (`MAX_TRIANGLES`).
pub const MAX_TRIANGLES: u64 = 100_000;
/// Budgets by asset kind (`TRIANGLE_BUDGETS`, the owner 2026-11-08), Low,
/// Medium, High, Ultra. Live on evani.app since 2026-11-08 13:57 UTC (Evani.App
/// 72e54307); `KIND_BUDGETS_LIVE` off would hold every kind to the flat
/// `MAX_TRIANGLES` again.
pub const TRIANGLE_BUDGETS: &[(&str, [u64; 4])] = &[
    ("object", [100_000, 100_000, 100_000, 100_000]),
    ("landscape", [50_000, 200_000, 500_000, 1_000_000]),
    ("sky-water", [25_000, 100_000, 250_000, 500_000]),
];
pub const KIND_BUDGETS_LIVE: bool = true;

/// Which budget an asset gets (`budgetKindOf`): from its catalogue class
/// only. A scene is a landscape, except a sky or a still-water surface; a
/// river, shoreline or waterfall carves the land, so it is a landscape.
pub fn budget_kind(ref_class: Option<&str>, category: &str, asset: &str) -> &'static str {
    if ref_class != Some("scene") {
        return "object";
    }
    let cat = category.to_ascii_lowercase();
    let sky = [
        "sky",
        "skies",
        "atmosphere",
        "weather",
        "cloud",
        "clouds",
        "fog",
        "mist",
    ];
    if sky.contains(&cat.as_str()) {
        return "sky-water";
    }
    if ["water", "ocean", "sea", "lake", "lakes"].contains(&cat.as_str()) {
        const LAND: &[&str] = &[
            "river",
            "rivers",
            "shore",
            "shores",
            "shoreline",
            "shorelines",
            "coast",
            "coasts",
            "coastline",
            "coastlines",
            "waterfall",
            "waterfalls",
            "delta",
            "deltas",
            "estuary",
            "estuaries",
            "stream",
            "streams",
            "creek",
            "creeks",
        ];
        let a = asset.to_ascii_lowercase();
        return if a.split('-').any(|w| LAND.contains(&w)) {
            "landscape"
        } else {
            "sky-water"
        };
    }
    "landscape"
}

/// The triangle limit for a kind at the render tier (`triangleBudget`), or
/// the flat limit while the kind budgets aren't live on the server.
pub fn triangle_budget(kind: &str, live: bool) -> u64 {
    if !live {
        return MAX_TRIANGLES;
    }
    let tier = ["low", "medium", "high", "ultra"]
        .iter()
        .position(|t| *t == RENDER_TIER)
        .unwrap_or(2);
    TRIANGLE_BUDGETS
        .iter()
        .find(|(k, _)| *k == kind)
        .or_else(|| TRIANGLE_BUDGETS.first())
        .map_or(MAX_TRIANGLES, |(_, b)| b[tier])
}

/// The server's triangle refusal, in its words (`triangleProblem`).
pub fn triangle_problem(triangles: u64, kind: &str, live: bool) -> Option<String> {
    let limit = triangle_budget(kind, live);
    if triangles <= limit {
        return None;
    }
    let n = |x: u64| {
        let s = x.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    let what = match kind {
        "landscape" => "a landscape",
        "sky-water" => "a sky, atmosphere or water surface",
        _ => "an object",
    };
    Some(if live {
        format!(
            "The asset has {} triangles; the limit for {what} at the High tier is {}.",
            n(triangles),
            n(limit)
        )
    } else {
        format!(
            "The asset has {} triangles; the server's limit is {} (the budgets by kind aren't live on the server yet).",
            n(triangles),
            n(limit)
        )
    })
}

/// Why an engine base can't go to the server: it is older than the render
/// app the server builds (`has_render_app`: the base contains
/// `RENDER_APP_COMMIT`; None when git couldn't tell).
pub fn engine_base_problem(base: &str, has_render_app: Option<bool>) -> Option<String> {
    match has_render_app {
        Some(false) => Some(format!(
            "the engine base {} is older than the server's render app ({}): the server can't build it (\"the build failed\"). Merge the main branch into the round's branch and pack that commit (a round already folded into the main branch packs by itself as the main-branch commit that holds it: give it with --commit and no --base)",
            &base[..12.min(base.len())],
            &RENDER_APP_COMMIT[..8]
        )),
        _ => None,
    }
}

/// One file's section of a git diff, as the server splits it (`splitPatch`).
#[derive(Debug, Clone, Default)]
pub struct ServerFile {
    pub path: String,
    pub old_path: String,
    pub text: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

fn strip_ab(p: &str) -> String {
    let p = p
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(p);
    p.strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(p)
        .to_string()
}

/// `diff --git A B` with each side a plain word or a quoted path.
fn git_header(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("diff --git ")?;
    let word = |s: &str| -> Option<(String, usize)> {
        if let Some(q) = s.strip_prefix('"') {
            let end = q.find('"')?;
            (end > 0).then(|| (s[..end + 2].to_string(), end + 2))
        } else {
            let end = s.find(char::is_whitespace).unwrap_or(s.len());
            (end > 0).then(|| (s[..end].to_string(), end))
        }
    };
    let (a, n) = word(rest)?;
    let rest = rest[n..].strip_prefix(' ')?;
    let (b, m) = word(rest)?;
    (m == rest.len()).then_some((a, b))
}

/// Splits a git diff into its files the way the server does.
pub fn server_split(diff: &str) -> Result<Vec<ServerFile>, String> {
    if diff.trim().is_empty() {
        return Ok(vec![]);
    }
    let text = diff.replace("\r\n", "\n");
    let mut files: Vec<ServerFile> = vec![];
    let mut cur: Option<ServerFile> = None;
    let mut buf: Vec<&str> = vec![];
    let flush = |cur: &mut Option<ServerFile>, buf: &mut Vec<&str>, files: &mut Vec<ServerFile>| {
        if let Some(mut f) = cur.take() {
            f.text = format!("{}\n", buf.join("\n"));
            files.push(f);
        }
        buf.clear();
    };
    for line in text.split('\n') {
        if let Some((a, b)) = git_header(line) {
            flush(&mut cur, &mut buf, &mut files);
            cur = Some(ServerFile {
                path: strip_ab(&b),
                old_path: strip_ab(&a),
                ..Default::default()
            });
            buf.push(line);
            continue;
        }
        let Some(f) = cur.as_mut() else {
            if !line.trim().is_empty() {
                return Err(
                    "patch.diff must be a git diff: every file starts with \"diff --git\"."
                        .into(),
                );
            }
            continue;
        };
        buf.push(line);
        if let Some(p) = line
            .strip_prefix("+++ ")
            .or_else(|| line.strip_prefix("--- "))
        {
            let p = p.trim();
            if p != "/dev/null" {
                if line.starts_with("+++ ") {
                    f.path = strip_ab(p);
                } else {
                    f.old_path = strip_ab(p);
                }
            }
            continue;
        }
        if let Some(l) = line.strip_prefix('+') {
            f.added.push(l.to_string());
        } else if let Some(l) = line.strip_prefix('-') {
            f.removed.push(l.to_string());
        }
    }
    flush(&mut cur, &mut buf, &mut files);
    Ok(files)
}

/// `\s*,?\s*(?:#.*)?$`: what may follow a feature list's value.
fn feature_tail(s: &str) -> bool {
    let s = s.trim_start();
    let s = s.strip_prefix(',').unwrap_or(s).trim_start();
    s.is_empty() || s.starts_with('#')
}

/// A Cargo.toml line the server takes as a feature-list line (`FEATURE_LINE`):
/// a comment, `[features]`, `name = [ ...`, ONE quoted entry, a closing `]`
/// or a blank line. Several quoted entries on one line are refused.
pub fn feature_line(l: &str) -> bool {
    let l = l.strip_suffix('\r').unwrap_or(l);
    let t = l.trim_start();
    if t.is_empty() || t.starts_with('#') || t == "[features]" {
        return true;
    }
    if let Some(after) = t.strip_prefix(']') {
        let a = after.trim_start();
        return a.is_empty() || a == ",";
    }
    if let Some(q) = t.strip_prefix('"') {
        return q.find('"').is_some_and(|e| feature_tail(&q[e + 1..]));
    }
    let name_end = t
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(t.len());
    if name_end == 0 {
        return false;
    }
    let Some(rest) = t[name_end..].trim_start().strip_prefix('=') else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix('[') else {
        return false;
    };
    match rest.find([']', '{', '}', '=']) {
        None => true,
        Some(i) if rest[i..].starts_with(']') => feature_tail(&rest[i + 1..]),
        Some(_) => false,
    }
}

/// `\bname\s*!` in a line.
fn macro_at(line: &str, name: &str) -> bool {
    let b = line.as_bytes();
    line.match_indices(name).any(|(i, _)| {
        let before = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        let after = &line[i + name.len()..];
        let next_word = after
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_');
        before && !next_word && after.trim_start().starts_with('!')
    })
}

/// `#\s*!?\s*\[\s*name\b` in a line.
fn attr_at(line: &str, name: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    ["#[", "#!["].iter().any(|open| {
        let pat = format!("{open}{name}");
        compact.match_indices(&pat).any(|(i, _)| {
            !compact[i + pat.len()..]
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
    })
}

/// What an added Rust line uses that the server refuses (`FORBIDDEN_CODE`),
/// comments included (the server reads every added line).
pub fn server_code_hit(line: &str) -> Option<&'static str> {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    for (tok, name) in [
        ("std::process", "std::process"),
        ("Command::new", "Command::new"),
        ("std::net", "std::net"),
        ("std::fs", "std::fs"),
        ("std::env", "std::env"),
    ] {
        if compact.contains(tok) {
            return Some(name);
        }
    }
    if has_word(line, "unsafe") {
        return Some("unsafe");
    }
    let extern_c = line.match_indices("extern").any(|(i, _)| {
        let after = &line[i + 6..];
        is_word_at(line, i, "extern")
            && after.starts_with(char::is_whitespace)
            && after.trim_start().starts_with('"')
    });
    if extern_c {
        return Some("extern \"…\"");
    }
    if attr_at(line, "link") {
        return Some("#[link]");
    }
    if attr_at(line, "path") {
        return Some("#[path]");
    }
    if ["include", "include_str", "include_bytes"]
        .iter()
        .any(|m| macro_at(line, m))
    {
        return Some("include!");
    }
    if macro_at(line, "env") || macro_at(line, "option_env") {
        return Some("env!");
    }
    if has_word(line, "proc_macro") {
        return Some("proc_macro");
    }
    if macro_at(line, "asm") || macro_at(line, "global_asm") {
        return Some("asm!");
    }
    None
}

fn server_allowed_path(p: &str) -> bool {
    let inside = ALLOWED_DIRS
        .iter()
        .chain([DEV_SCENE_PREFIX].iter())
        .any(|d| p.starts_with(d))
        || ALLOWED_FILES.contains(&p);
    inside && !p.split('/').any(|s| s.is_empty() || s == "." || s == "..")
}

/// Every reason the server (and Evani.App before sending) would refuse this
/// patch, in its own words (`patchProblems`); empty when it takes it.
pub fn server_patch_problems(diff: &str) -> Vec<String> {
    let files = match server_split(diff) {
        Ok(f) => f,
        Err(e) => return vec![e],
    };
    let mut out: Vec<String> = vec![];
    let mut push = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    let starts = |text: &str, heads: &[&str]| {
        text.split('\n')
            .any(|l| heads.iter().any(|h| l.starts_with(h)))
    };
    for f in &files {
        let mut seen = vec![&f.path];
        if f.old_path != f.path {
            seen.push(&f.old_path);
        }
        for p in seen {
            if !server_allowed_path(p) {
                push(format!("The patch changes {p}, which is outside the engine folders an Evani.commons version may change."));
            }
        }
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        if name == "build.rs" {
            push(format!(
                "The patch adds or changes {}: build scripts aren’t allowed.",
                f.path
            ));
        }
        if name == "Cargo.lock" {
            push("The patch changes Cargo.lock, which isn’t allowed.".into());
        }
        if starts(&f.text, &["GIT binary patch", "Binary files "]) {
            push(format!(
                "The patch has a binary change in {}, which isn’t allowed.",
                f.path
            ));
        }
        if starts(
            &f.text,
            &[
                "new file mode 12",
                "old mode 12",
                "new mode 12",
                "deleted file mode 12",
            ],
        ) {
            push(format!(
                "The patch makes {} a link, which isn’t allowed.",
                f.path
            ));
        }
        if name == "Cargo.toml" && f.added.iter().chain(&f.removed).any(|l| !feature_line(l)) {
            push(format!(
                "The patch changes more than the features in {}.",
                f.path
            ));
        }
        if f.path.ends_with(".rs")
            && let Some(hit) = f.added.iter().find_map(|l| server_code_hit(l))
        {
            push(format!(
                "The patch adds {hit} in {}, which an Evani.commons version may not use.",
                f.path
            ));
        }
    }
    out
}

/// The catalogue class of `key` in a library.json text: the asset's own
/// `ref_class`, else its category's.
fn library_ref_class(text: &[u8], key: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(text).ok()?;
    for c in v.get("categories")?.as_array()? {
        for a in c.get("assets")?.as_array()? {
            if a.get("feature").and_then(Value::as_str) == Some(key) {
                return a
                    .get("ref_class")
                    .or_else(|| c.get("ref_class"))
                    .and_then(Value::as_str)
                    .map(String::from);
            }
        }
    }
    None
}

// ---------- unified diffs ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Added,
    Deleted,
    Modified,
}
impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Added => "added",
            Status::Deleted => "deleted",
            Status::Modified => "modified",
        }
    }
}

#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    old_len: usize,
    /// (' ' | '-' | '+', the line with its line end, unless it had none).
    lines: Vec<(char, String)>,
}

#[derive(Debug, Clone)]
pub struct FilePatch {
    pub path: String,
    pub status: Status,
    hunks: Vec<Hunk>,
}
impl FilePatch {
    /// The added lines with their line number in the new file.
    pub fn added(&self) -> Vec<(usize, String)> {
        let mut out = vec![];
        for h in &self.hunks {
            let mut n = if h.old_len == 0 {
                h.old_start + 1
            } else {
                h.old_start
            };
            // The new side starts where the old one does, shifted by earlier hunks;
            // only relative numbers matter for messages, so count from the hunk.
            for (c, l) in &h.lines {
                match c {
                    '+' => {
                        out.push((n, l.trim_end_matches(['\n', '\r']).to_string()));
                        n += 1;
                    }
                    ' ' => n += 1,
                    _ => {}
                }
            }
        }
        out
    }
}

fn parse_range(s: &str) -> Result<(usize, usize), String> {
    let (a, b) = s.split_once(',').unwrap_or((s, "1"));
    let a = a.parse().map_err(|_| format!("bad hunk range `{s}`"))?;
    let b = b.parse().map_err(|_| format!("bad hunk range `{s}`"))?;
    Ok((a, b))
}

/// Read a `git diff` (text, no renames, no binaries).
pub fn parse_patch(text: &str) -> Result<Vec<FilePatch>, String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out: Vec<FilePatch> = vec![];
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let bare = l.trim_end_matches(['\n', '\r']);
        if let Some(rest) = bare.strip_prefix("diff --git ") {
            let (a, b) = rest
                .split_once(" b/")
                .ok_or_else(|| format!("patch: unreadable header `{bare}`"))?;
            let a = a
                .strip_prefix("a/")
                .ok_or_else(|| format!("patch: unreadable header `{bare}`"))?;
            if a != b {
                return Err(format!(
                    "patch: `{a}` is renamed to `{b}`; renames never travel"
                ));
            }
            out.push(FilePatch {
                path: a.to_string(),
                status: Status::Modified,
                hunks: vec![],
            });
            i += 1;
            continue;
        }
        let Some(cur) = out.last_mut() else {
            if bare.is_empty() {
                i += 1;
                continue;
            }
            return Err(format!("patch: text before the first file: `{bare}`"));
        };
        if bare.starts_with("new file mode") {
            cur.status = Status::Added;
        } else if bare.starts_with("deleted file mode") {
            cur.status = Status::Deleted;
        } else if bare.starts_with("rename ")
            || bare.starts_with("copy ")
            || bare.starts_with("similarity")
        {
            return Err(format!(
                "patch: `{}` is renamed or copied; that never travels",
                cur.path
            ));
        } else if bare.starts_with("Binary files") || bare.starts_with("GIT binary patch") {
            return Err(format!(
                "patch: `{}` is a binary file; only text travels",
                cur.path
            ));
        } else if bare.starts_with("new mode") || bare.starts_with("old mode") {
            if bare.ends_with("120000") || bare.ends_with("160000") {
                return Err(format!("patch: `{}` is a link or a submodule", cur.path));
            }
        } else if let Some(p) = bare.strip_prefix("--- ") {
            if p != "/dev/null" && p.strip_prefix("a/") != Some(cur.path.as_str()) {
                return Err(format!("patch: `{p}` doesn't match `{}`", cur.path));
            }
        } else if let Some(p) = bare.strip_prefix("+++ ") {
            if p != "/dev/null" && p.strip_prefix("b/") != Some(cur.path.as_str()) {
                return Err(format!("patch: `{p}` doesn't match `{}`", cur.path));
            }
        } else if let Some(h) = bare.strip_prefix("@@ -") {
            let (ranges, _) = h
                .split_once(" @@")
                .ok_or_else(|| format!("patch: bad hunk `{bare}`"))?;
            let (old, new) = ranges
                .split_once(" +")
                .ok_or_else(|| format!("patch: bad hunk `{bare}`"))?;
            let (old_start, old_len) = parse_range(old)?;
            let (_, new_len) = parse_range(new)?;
            let (mut o, mut n) = (old_len, new_len);
            let mut hunk = Hunk {
                old_start,
                old_len,
                lines: vec![],
            };
            i += 1;
            while (o > 0 || n > 0) && i < lines.len() {
                let hl = lines[i];
                let c = hl.chars().next().unwrap_or(' ');
                let body = hl.get(1..).unwrap_or("").to_string();
                match c {
                    ' ' if o > 0 && n > 0 => {
                        o -= 1;
                        n -= 1;
                    }
                    '-' if o > 0 => o -= 1,
                    '+' if n > 0 => n -= 1,
                    '\\' => {
                        strip_newline(&mut hunk);
                        i += 1;
                        continue;
                    }
                    _ => return Err(format!("patch: `{}`: a hunk ends early", cur.path)),
                }
                hunk.lines.push((c, body));
                i += 1;
            }
            if o > 0 || n > 0 {
                return Err(format!("patch: `{}`: a hunk is cut short", cur.path));
            }
            if i < lines.len() && lines[i].starts_with('\\') {
                strip_newline(&mut hunk);
                i += 1;
            }
            cur.hunks.push(hunk);
            continue;
        }
        // `index ...` and anything else in a header: nothing to keep.
        i += 1;
    }
    Ok(out)
}

fn strip_newline(h: &mut Hunk) {
    if let Some((_, l)) = h.lines.last_mut()
        && l.ends_with('\n')
    {
        l.pop();
    }
}

/// Apply one file's patch to its exact base text. None: the file is deleted.
pub fn apply(base: Option<&[u8]>, fp: &FilePatch) -> Result<Option<Vec<u8>>, String> {
    let base = std::str::from_utf8(base.unwrap_or(b""))
        .map_err(|_| format!("`{}`: not UTF-8 text", fp.path))?;
    let src: Vec<&str> = base.split_inclusive('\n').collect();
    let mut out = String::with_capacity(base.len() + 1024);
    let mut pos = 0;
    for h in &fp.hunks {
        let start = if h.old_len == 0 {
            h.old_start
        } else {
            h.old_start.saturating_sub(1)
        };
        if start < pos || start > src.len() {
            return Err(format!(
                "`{}`: hunk at line {} is out of order",
                fp.path, h.old_start
            ));
        }
        for l in &src[pos..start] {
            out.push_str(l);
        }
        pos = start;
        for (c, l) in &h.lines {
            match c {
                ' ' | '-' => {
                    if src.get(pos) != Some(&l.as_str()) {
                        return Err(format!(
                            "`{}`: line {} doesn't match the version's base",
                            fp.path,
                            pos + 1
                        ));
                    }
                    if *c == ' ' {
                        out.push_str(l);
                    }
                    pos += 1;
                }
                _ => out.push_str(l),
            }
        }
    }
    for l in &src[pos..] {
        out.push_str(l);
    }
    if fp.status == Status::Deleted {
        if !out.is_empty() {
            return Err(format!(
                "`{}`: a deleted file's patch leaves text behind",
                fp.path
            ));
        }
        return Ok(None);
    }
    Ok(Some(out.into_bytes()))
}

// ---------- small helpers ----------

pub fn sha_hex(b: &[u8]) -> String {
    let d = Sha256::digest(b);
    d.iter().map(|x| format!("{x:02x}")).collect()
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e} (Evani.commons needs git on the PATH)"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}
fn git_s(root: &Path, args: &[&str]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&git(root, args)?)
        .trim()
        .to_string())
}
fn git_show(root: &Path, commit: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let spec = format!("{commit}:{path}");
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &spec])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Ok(None);
    }
    git(root, &["show", &spec]).map(Some)
}

fn lf(b: &[u8]) -> (Vec<u8>, bool) {
    let crlf = b.windows(2).any(|w| w == b"\r\n");
    if !crlf {
        return (b.to_vec(), false);
    }
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    (out, true)
}
fn to_crlf(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + b.len() / 30);
    for &c in b {
        if c == b'\n' {
            out.push(b'\r');
        }
        out.push(c);
    }
    out
}

fn write_file(p: &Path, b: &[u8]) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))
}

/// Every file under `dir`, relative with `/`, sorted; refuses links.
fn list_files(dir: &Path) -> Result<Vec<String>, String> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) -> Result<(), String> {
        for e in std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))? {
            let e = e.map_err(|e| e.to_string())?;
            let ft = e.file_type().map_err(|e| e.to_string())?;
            let p = e.path();
            if ft.is_symlink() {
                return Err(format!("{}: links never travel in a package", p.display()));
            }
            if ft.is_dir() {
                walk(base, &p, out)?;
            } else {
                let rel = p
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = vec![];
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

const HASH_PLACEHOLDER: &str = "\"content_hash\": \"\"";

/// The content hash: SHA-256 of the sorted lines `<sha256>  <path>\n` for
/// every file but commons.json, then one line for commons.json as written
/// with `"content_hash": ""` (the exporter writes it so, hashes it, then fills
/// the value in; a reader puts the placeholder back before hashing).
pub fn content_hash(dir: &Path) -> Result<String, String> {
    let files = list_files(dir)?;
    let mut acc = String::new();
    for f in files.iter().filter(|f| *f != "commons.json") {
        let b = std::fs::read(dir.join(f)).map_err(|e| format!("{f}: {e}"))?;
        acc.push_str(&format!("{}  {f}\n", sha_hex(&b)));
    }
    let m = std::fs::read_to_string(dir.join("commons.json"))
        .map_err(|e| format!("commons.json: {e}"))?;
    let blank = blank_hash(&m)?;
    acc.push_str(&format!("{}  commons.json\n", sha_hex(blank.as_bytes())));
    Ok(format!("sha256:{}", sha_hex(acc.as_bytes())))
}
fn blank_hash(m: &str) -> Result<String, String> {
    if m.matches(HASH_PLACEHOLDER).count() == 1 {
        return Ok(m.to_string());
    }
    let key = "\"content_hash\": \"sha256:";
    let hits: Vec<usize> = m.match_indices(key).map(|(i, _)| i).collect();
    let [i] = hits.as_slice() else {
        return Err("commons.json: needs exactly one \"content_hash\" line".into());
    };
    let start = i + key.len();
    let hex = m
        .get(start..start + 64)
        .ok_or("commons.json: content_hash is cut short")?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) || m.as_bytes().get(start + 64) != Some(&b'"') {
        return Err("commons.json: content_hash is not sha256:<64 hex>".into());
    }
    Ok(format!(
        "{}{}{}",
        &m[..*i],
        HASH_PLACEHOLDER,
        &m[start + 65..]
    ))
}

// ---------- pictures ----------

/// A small JPEG of a PNG or JPEG picture, longest side at most `max_side`.
/// A JPEG already small enough is kept byte for byte.
pub fn picture_jpeg(bytes: &[u8], max_side: u32) -> Result<(Vec<u8>, u32, u32), String> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let (w, h) = jpeg_size(bytes).ok_or("a JPEG without a readable size")?;
        if w.max(h) <= max_side {
            return Ok((bytes.to_vec(), w, h));
        }
        return Err(format!(
            "a {w}x{h} JPEG is over {max_side} px: give the PNG or a smaller copy"
        ));
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("only PNG or JPEG pictures".into());
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| format!("png: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or("png: too big")?];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png: {e}"))?;
    let (w, h) = (info.width, info.height);
    let ch = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => return Err("png: palette not expanded".into()),
    };
    let data = &buf[..info.buffer_size()];
    let line = info.line_size;
    let px = |x: u32, y: u32| -> [u32; 3] {
        let o = y as usize * line + x as usize * ch;
        match ch {
            1 | 2 => [data[o] as u32; 3],
            _ => [data[o] as u32, data[o + 1] as u32, data[o + 2] as u32],
        }
    };
    let scale = (w.max(h) as f64 / max_side as f64).max(1.0);
    let tw = ((w as f64 / scale).round() as u32).clamp(1, max_side);
    let th = ((h as f64 / scale).round() as u32).clamp(1, max_side);
    let mut rgb = Vec::with_capacity(tw as usize * th as usize * 3);
    for ty in 0..th {
        let y0 = (ty as u64 * h as u64 / th as u64) as u32;
        let y1 = (((ty + 1) as u64 * h as u64 / th as u64) as u32)
            .max(y0 + 1)
            .min(h);
        for tx in 0..tw {
            let x0 = (tx as u64 * w as u64 / tw as u64) as u32;
            let x1 = (((tx + 1) as u64 * w as u64 / tw as u64) as u32)
                .max(x0 + 1)
                .min(w);
            let mut s = [0u32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = px(x, y);
                    s[0] += p[0];
                    s[1] += p[1];
                    s[2] += p[2];
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            rgb.extend(s.iter().map(|v| ((v + n / 2) / n) as u8));
        }
    }
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, JPEG_QUALITY);
    enc.encode(&rgb, tw as u16, th as u16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| format!("jpeg: {e}"))?;
    Ok((out, tw, th))
}

fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let m = b[i + 1];
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 2 + len;
    }
    None
}

/// True when a PNG carries a C2PA manifest (a `caBX` chunk): the content
/// credentials the server checks for a new Community item's reference.
pub fn png_has_c2pa(b: &[u8]) -> bool {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut i = 8;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        if &b[i + 4..i + 8] == b"caBX" {
            return true;
        }
        i += 12 + len;
    }
    false
}

/// A PNG or JPEG decoded to 8-bit RGB: (width, height, pixels).
fn decode_rgb(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        let px = d.decode().map_err(|e| format!("jpeg: {e}"))?;
        let info = d.info().ok_or("jpeg: no size")?;
        let (w, h) = (info.width as u32, info.height as u32);
        let rgb = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => px,
            jpeg_decoder::PixelFormat::L8 => px.iter().flat_map(|&v| [v, v, v]).collect(),
            jpeg_decoder::PixelFormat::L16 => px
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|c| [c[0], c[0], c[0]])
                .collect(),
            jpeg_decoder::PixelFormat::CMYK32 => px
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|c| {
                    let k = 255 - c[3] as u32;
                    [c[0], c[1], c[2]].map(|v| ((255 - v as u32) * k / 255) as u8)
                })
                .collect(),
        };
        return Ok((w, h, rgb));
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("only PNG or JPEG pictures".into());
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| format!("png: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or("png: too big")?];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png: {e}"))?;
    let ch = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => return Err("png: palette not expanded".into()),
    };
    let (w, h) = (info.width, info.height);
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for y in 0..h as usize {
        let row = &buf[y * info.line_size..];
        for x in 0..w as usize {
            let p = &row[x * ch..];
            if ch < 3 {
                rgb.extend([p[0]; 3]);
            } else {
                rgb.extend(&p[..3]);
            }
        }
    }
    Ok((w, h, rgb))
}

/// Side of the grid two pictures are compared on (`picture_difference`).
const SAME_GRID: u32 = 64;
/// Most mean difference (0-255 per channel) of two encodings of one picture.
/// The catalogue's smaller JPEG copies of the Official references measure
/// under 1 against their PNGs (Office 0.41, Pebbles 0.26, Oak 0.93, 2026-11-08).
pub const SAME_PICTURE_MAX: f64 = 4.0;

/// How far apart two pictures are: the mean absolute difference (0-255) of
/// their colours box-sampled on a 64 x 64 grid over the whole frame, so a
/// re-encoded or resized copy compares as the same picture. `None` when their
/// shapes differ (aspect ratio more than 2% apart).
pub fn picture_difference(a: &[u8], b: &[u8]) -> Result<Option<f64>, String> {
    let grid = |bytes: &[u8]| -> Result<(f64, Vec<f64>), String> {
        let (w, h, rgb) = decode_rgb(bytes)?;
        if w == 0 || h == 0 {
            return Err("an empty picture".into());
        }
        let n = SAME_GRID as u64;
        let mut out = Vec::with_capacity((n * n * 3) as usize);
        for gy in 0..n {
            let y0 = (gy * h as u64 / n) as usize;
            let y1 = (((gy + 1) * h as u64 / n) as usize).max(y0 + 1);
            for gx in 0..n {
                let x0 = (gx * w as u64 / n) as usize;
                let x1 = (((gx + 1) * w as u64 / n) as usize).max(x0 + 1);
                let mut s = [0u64; 3];
                for y in y0..y1.min(h as usize) {
                    for x in x0..x1.min(w as usize) {
                        let o = (y * w as usize + x) * 3;
                        for (c, v) in s.iter_mut().enumerate() {
                            *v += rgb[o + c] as u64;
                        }
                    }
                }
                let cnt = ((y1.min(h as usize) - y0) * (x1.min(w as usize) - x0)).max(1) as f64;
                out.extend(s.iter().map(|v| *v as f64 / cnt));
            }
        }
        Ok((w as f64 / h as f64, out))
    };
    let (ra, ga) = grid(a)?;
    let (rb, gb) = grid(b)?;
    if (ra / rb - 1.0).abs() > 0.02 {
        return Ok(None);
    }
    let d = ga.iter().zip(&gb).map(|(x, y)| (x - y).abs()).sum::<f64>() / ga.len() as f64;
    Ok(Some(d))
}

/// The item's locked reference (SHA-256 `locked`) for a round scored against
/// the picture `scored`: found in the history of the catalogue's reference
/// file for `asset` (`apps/material-view/assets/proc-refs/<asset>.jpg`, the
/// picture Evani.app's Official catalogue was seeded from), and taken only
/// when it is the same picture as `scored`. (where it was found, difference).
pub fn locked_reference(
    root: &Path,
    asset: &str,
    locked: &str,
    scored: &[u8],
) -> Result<(String, f64), String> {
    let path = catalogue_ref_path(asset);
    let Some((from, b)) = find_locked(root, asset, locked)? else {
        return Err(format!(
            "made against a different reference: Evani.app's locked reference for {asset} is {}, the round was scored against {}, and the locked picture isn't in this project's history ({path}); the server would refuse it. Not packed: the owner gives the item the new reference, or the round is scored against the locked one",
            &locked[..12],
            &sha_hex(scored)[..12]
        ));
    };
    match picture_difference(scored, &b)? {
        Some(d) if d <= SAME_PICTURE_MAX => Ok((from, d)),
        other => Err(format!(
            "made against a different reference: the round was scored against {} but Evani.app's locked reference for {asset} is {} ({from}), a different picture ({}); the server would refuse it, and naming the locked one would claim a score it wasn't given. Not packed: the owner gives the item the new reference, or the round is scored against the locked one",
            &sha_hex(scored)[..12],
            &locked[..12],
            match other {
                Some(d) => format!("mean difference {d:.1} of 255"),
                None => "another shape".to_string(),
            }
        )),
    }
}

/// The catalogue's reference file for `asset` (the picture Evani.app's
/// Official catalogue was seeded from).
fn catalogue_ref_path(asset: &str) -> String {
    format!("apps/material-view/assets/proc-refs/{asset}.jpg")
}

/// The picture with SHA-256 `locked` among the project's own copies of the
/// asset's catalogue reference: the working-tree file, then every version of
/// it in the git history. (where it was found, bytes), or None.
pub fn find_locked(root: &Path, asset: &str, locked: &str) -> Result<Option<(String, Vec<u8>)>, String> {
    let path = catalogue_ref_path(asset);
    if let Ok(b) = std::fs::read(root.join(&path))
        && sha_hex(&b) == locked
    {
        return Ok(Some((path, b)));
    }
    let log = git_s(root, &["log", "--all", "--format=%H", "--", &path])?;
    let mut seen = BTreeSet::new();
    for c in log.lines().map(str::trim).filter(|c| !c.is_empty()) {
        let Ok(blob) = git_s(root, &["rev-parse", &format!("{c}:{path}")]) else {
            continue;
        };
        if !seen.insert(blob.clone()) {
            continue;
        }
        let b = git(root, &["cat-file", "blob", &blob])?;
        if sha_hex(&b) == locked {
            return Ok(Some((format!("{path} at {}", &c[..12.min(c.len())]), b)));
        }
    }
    Ok(None)
}

// ---------- the item's locked reference, in the project (`commons locked-ref`) ----------

/// Evani.app's public API (read-only, no sign-in): `GET <site>/public/commons/<asset>/<style>/reference`
/// answers the item's locked reference picture.
pub const PUBLIC_API: &str = "https://ytuxp3jeac.execute-api.eu-west-2.amazonaws.com/v1";

/// Where Asset Studio scores a column of a locked item: `.evani/commons/locked/<asset>/<style>/locked-<sha12>.<ext>`
/// (under `.evani`, so never committed and never in a download).
pub fn locked_dir(asset: &str, style: &str) -> String {
    format!(".evani/commons/locked/{asset}/{style}")
}

fn pic_ext(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// The locked picture written into the project: its project-relative path and where it came from.
#[derive(Debug)]
pub struct LockedRef {
    pub path: String,
    pub from: String,
}

/// Puts the item's locked reference (SHA-256 `locked`) at `locked_dir(asset, style)/locked-<sha12>.<ext>`:
/// already there, else from the project (`find_locked`), else read from Evani.app's public API
/// (`site`; read-only). Written only when its bytes have exactly that SHA-256; it never replaces,
/// uploads or changes anything on the website.
pub fn fetch_locked(root: &Path, asset: &str, style: &str, locked: &str, site: &str) -> Result<LockedRef, String> {
    let ok_part = |s: &str| !s.is_empty() && s.len() <= 120 && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
    if !ok_part(asset) || !ok_part(style) {
        return Err(format!("commons locked-ref: asset {asset:?} and style {style:?} are library keys (a-z, 0-9, -)"));
    }
    let locked = locked.to_ascii_lowercase();
    if locked.len() != 64 || !locked.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("commons locked-ref: --sha256 {locked}: 64 hex digits"));
    }
    let dir_rel = locked_dir(asset, style);
    let dir = root.join(&dir_rel);
    let name = format!("locked-{}", &locked[..12]);
    for ext in ["jpg", "png", "webp"] {
        let p = dir.join(format!("{name}.{ext}"));
        if let Ok(b) = std::fs::read(&p)
            && sha_hex(&b) == locked
        {
            return Ok(LockedRef { path: format!("{dir_rel}/{name}.{ext}"), from: "already in the project".into() });
        }
    }
    let (from, bytes) = match find_locked(root, asset, &locked)? {
        Some(f) => f,
        None => {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let part = dir.join(format!("{name}.part"));
            let url = format!("{}/public/commons/{asset}/{style}/reference", site.trim_end_matches('/'));
            let st = Command::new(curl_exe())
                .args(["-sSfL", "--retry", "2", "--max-time", "120", "-o"])
                .arg(&part)
                .arg(&url)
                .status()
                .map_err(|e| format!("curl {url}: {e}"))?;
            let b = if st.success() { std::fs::read(&part).unwrap_or_default() } else { Vec::new() };
            let _ = std::fs::remove_file(&part);
            if !st.success() {
                return Err(format!("commons locked-ref: the locked picture isn't in this project's history and Evani.app's public API didn't answer it ({url}: {st})"));
            }
            let got = sha_hex(&b);
            if got != locked {
                return Err(format!(
                    "commons locked-ref: Evani.app answered a picture with SHA-256 {} for {asset}@{style}, not the locked {}: nothing written",
                    &got[..12],
                    &locked[..12]
                ));
            }
            (url, b)
        }
    };
    let ext = pic_ext(&bytes).ok_or_else(|| format!("commons locked-ref: the locked picture ({from}) is not a JPEG, PNG or WebP"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(format!("{name}.{ext}"));
    let tmp = dir.join(format!("{name}.{ext}.tmp"));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(LockedRef { path: format!("{dir_rel}/{name}.{ext}"), from })
}

/// `curl` (Windows ships one in System32).
fn curl_exe() -> PathBuf {
    if cfg!(windows) {
        let sys = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        let c = sys.join("System32").join("curl.exe");
        if c.is_file() {
            return c;
        }
    }
    PathBuf::from("curl")
}

// ---------- category scores (the radar chart's data) ----------

/// Asset Studio's five scoring categories in the radar chart's fixed order,
/// each with its weight in percent of the total (tools/evani-commons-tab/radar.js).
pub const CATEGORY_WEIGHTS: [(&str, u32); 5] = [
    ("silhouette", 25),
    ("structure", 25),
    ("material", 20),
    ("colour", 15),
    ("lighting", 15),
];

/// A categories round's data (`{scores, reasons, total}`, as the tab stores it)
/// checked and written the way `commons.json` carries it in `score.categories`:
/// every category a whole number 0-20, reasons optional (240 characters at
/// most), the weighted total out of 100 worked out here (a `total` that differs
/// is refused) and, with `score`, that total must be that half-star score
/// (total / 20 to the nearest half star), so the two never disagree.
pub fn categories_value(text: &str, score: Option<f64>) -> Result<Value, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("--categories: not JSON ({e})"))?;
    let known = |k: &str| CATEGORY_WEIGHTS.iter().any(|(id, _)| *id == k);
    let scores = v.get("scores").and_then(Value::as_object).ok_or(
        "--categories: needs \"scores\" { silhouette, structure, material, colour, lighting }",
    )?;
    if let Some(k) = scores.keys().find(|k| !known(k)) {
        return Err(format!("--categories: unknown category `{k}`"));
    }
    let mut out = Map::new();
    let mut sum = 0u32;
    for (id, w) in CATEGORY_WEIGHTS {
        let n = scores
            .get(id)
            .and_then(Value::as_u64)
            .filter(|n| *n <= 20)
            .ok_or_else(|| format!("--categories: `{id}` needs a whole number 0-20"))?;
        let n = u32::try_from(n).map_err(|_| format!("--categories: `{id}` out of range"))?;
        sum += n * w;
        out.insert(id.into(), json!(n));
    }
    let total = (f64::from(sum) / 20.0 * 10.0).round() / 10.0;
    if let Some(t) = v.get("total").filter(|t| !t.is_null()) {
        let t = t
            .as_f64()
            .ok_or("--categories: `total` must be a number")?;
        if (t - total).abs() > 0.05 {
            return Err(format!(
                "--categories: `total` is {t}, but the scores give {total} (silhouette 25, structure 25, material 20, colour 15, lighting 15)"
            ));
        }
    }
    let mut reasons = Map::new();
    if let Some(r) = v.get("reasons").filter(|r| !r.is_null()) {
        let r = r
            .as_object()
            .ok_or("--categories: `reasons` must be { <category>: text }")?;
        for (k, t) in r {
            if !known(k) {
                return Err(format!("--categories: a reason for unknown category `{k}`"));
            }
            let t = t
                .as_str()
                .ok_or_else(|| format!("--categories: the reason for `{k}` must be text"))?
                .trim();
            if t.chars().count() > 240 {
                return Err(format!(
                    "--categories: the reason for `{k}` is over 240 characters"
                ));
            }
            if !t.is_empty() {
                reasons.insert(k.clone(), json!(t));
            }
        }
    }
    if let Some(s) = score {
        let stars = (total / 20.0 * 2.0).round() / 2.0;
        if (stars - s).abs() > 1e-9 {
            return Err(format!(
                "--categories: the total {total}/100 is {stars} stars, but --score is {s}"
            ));
        }
    }
    Ok(json!({
        "scale": "0-20 per category; total out of 100",
        "weights": Value::Object(CATEGORY_WEIGHTS.iter().map(|(id, w)| ((*id).to_string(), json!(w))).collect()),
        "scores": Value::Object(out),
        "reasons": Value::Object(reasons),
        "total": total,
    }))
}

// ---------- export ----------

pub struct ExportOpts {
    pub root: PathBuf,
    pub asset: String,
    pub style: String,
    pub commit: String,
    pub onto: Option<String>,
    pub base: Option<String>,
    pub score: Option<f64>,
    pub independent: bool,
    pub reviewer_model: String,
    pub reviewer_account: String,
    pub reviewed_at: String,
    pub author_model: String,
    pub summary: String,
    pub column: Option<u32>,
    pub round: Option<u32>,
    pub reference: Option<PathBuf>,
    pub reference_original: Option<PathBuf>,
    pub reference_generator: String,
    pub reference_prompt: String,
    pub reference_made: String,
    /// The SHA-256 of the item's reference locked on Evani.app (the item
    /// card's `reference_sha256`). When it isn't `--reference`'s own hash, the
    /// export finds that file in the catalogue's history and packs it only if
    /// it is the same picture the round was scored against.
    pub locked_sha256: Option<String>,
    /// (round, view, file)
    pub pictures: Vec<(Option<u32>, Option<u32>, PathBuf)>,
    pub params: Option<PathBuf>,
    pub scene: Option<String>,
    pub view_param: Option<String>,
    pub views: Vec<u32>,
    pub size: Option<String>,
    pub area: Option<String>,
    pub based_on: Option<String>,
    pub author: Option<String>,
    pub out: PathBuf,
    /// Dev-scene files (examples/proc-field/) left out of the package: the
    /// server's build never uses them (`--leave-out <path>`, repeatable).
    pub leave_out: Vec<String>,
    /// The asset's mesh stats from the render app (asset-shoot.mjs --stats):
    /// its triangle count is checked against the server's budget.
    pub stats: Option<PathBuf>,
    /// A categories round's five scores and reasons (Asset Studio v1.5.36,
    /// the radar chart's data: `{scores, reasons, total}`): checked, its total
    /// worked out again, and written to `commons.json` as `score.categories`.
    pub categories: Option<PathBuf>,
    /// The commit that brought in the server's render app (RENDER_APP_COMMIT;
    /// tests name one of their own).
    pub render_app: String,
    /// `--dry-run`: every check and the package's hash, nothing kept.
    pub dry_run: bool,
}

impl ExportOpts {
    pub fn from_args(root: &Path, a: &[String]) -> Result<Self, String> {
        let num = |k: &str| -> Result<Option<u32>, String> {
            opt(a, k)
                .map(|v| v.parse().map_err(|_| format!("{k} {v}: a whole number")))
                .transpose()
        };
        let mut pictures = vec![];
        for p in opts(a, "--picture") {
            pictures.push(match p.split_once('=') {
                Some((tag, f)) if tag.starts_with('r') && tag.contains('v') => {
                    let (r, v) = tag[1..]
                        .split_once('v')
                        .ok_or_else(|| format!("--picture {p}: r<round>v<view>=<file>"))?;
                    (
                        Some(
                            r.parse()
                                .map_err(|_| format!("--picture {p}: r<round>v<view>=<file>"))?,
                        ),
                        Some(
                            v.parse()
                                .map_err(|_| format!("--picture {p}: r<round>v<view>=<file>"))?,
                        ),
                        PathBuf::from(f),
                    )
                }
                _ => (None, None, PathBuf::from(p)),
            });
        }
        let commit = opt(a, "--commit")
            .or_else(|| opt(a, "--branch"))
            .unwrap_or_else(|| "HEAD".into());
        Ok(ExportOpts {
            root: opt(a, "--root")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.to_path_buf()),
            asset: opt(a, "--asset").ok_or("commons export: --asset <library key> is needed")?,
            style: opt(a, "--style").unwrap_or_else(|| "photoreal".into()),
            commit,
            onto: opt(a, "--onto"),
            base: opt(a, "--base"),
            score: opt(a, "--score").map(|s| parse_score(&s)).transpose()?,
            independent: has(a, "--independent"),
            reviewer_model: opt(a, "--reviewer-model").unwrap_or_default(),
            reviewer_account: opt(a, "--reviewer-account").unwrap_or_default(),
            reviewed_at: opt(a, "--reviewed-at").unwrap_or_default(),
            author_model: opt(a, "--author-model").unwrap_or_default(),
            summary: opt(a, "--summary").unwrap_or_default(),
            column: num("--column")?,
            round: num("--round")?,
            reference: opt(a, "--reference").map(PathBuf::from),
            reference_original: opt(a, "--reference-original").map(PathBuf::from),
            reference_generator: opt(a, "--reference-generator").unwrap_or_default(),
            reference_prompt: opt(a, "--reference-prompt").unwrap_or_default(),
            reference_made: opt(a, "--reference-made").unwrap_or_default(),
            locked_sha256: opt(a, "--locked-sha256")
                .map(|s| {
                    let s = s.trim().to_ascii_lowercase();
                    if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
                        Ok(s)
                    } else {
                        Err(format!("--locked-sha256 {s}: 64 hex digits"))
                    }
                })
                .transpose()?,
            pictures,
            params: opt(a, "--params").map(PathBuf::from),
            scene: opt(a, "--scene"),
            view_param: opt(a, "--view-param"),
            views: opt(a, "--views")
                .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                .unwrap_or_default(),
            size: opt(a, "--size"),
            area: opt(a, "--area"),
            based_on: opt(a, "--based-on"),
            author: opt(a, "--author"),
            leave_out: opts(a, "--leave-out")
                .into_iter()
                .map(|p| p.replace('\\', "/"))
                .collect(),
            stats: opt(a, "--stats").map(PathBuf::from),
            categories: opt(a, "--categories").map(PathBuf::from),
            render_app: RENDER_APP_COMMIT.to_string(),
            dry_run: has(a, "--dry-run"),
            out: opt(a, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join(".evani").join("commons").join("out")),
        })
    }
}

pub struct ExportResult {
    pub dir: PathBuf,
    pub content_hash: String,
    pub report: String,
}

fn valid_key(s: &str, max: usize) -> bool {
    s.len() >= 2
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The library entry for `key` in a library.json text: (category id, name).
fn library_entry(text: &[u8], key: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_slice(text).ok()?;
    for c in v.get("categories")?.as_array()? {
        for a in c.get("assets")?.as_array()? {
            if a.get("feature").and_then(Value::as_str) == Some(key) {
                return Some((
                    c.get("id")?.as_str()?.to_string(),
                    a.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(key)
                        .to_string(),
                ));
            }
        }
    }
    None
}

/// Whether `base` contains the server's render app: None when git can't
/// tell (a project without that commit, say a test repository).
fn has_render_app(root: &Path, render_app: &str, base: &str) -> Option<bool> {
    is_ancestor(root, render_app, base)
}

/// Whether `a` is an ancestor of (or is) `b`: None when git can't tell.
fn is_ancestor(root: &Path, a: &str, b: &str) -> Option<bool> {
    let st = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", a, b])
        .output()
        .ok()?;
    match st.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

fn default_onto(root: &Path) -> Result<String, String> {
    // The branch the project's main checkout is on (the first worktree).
    let wl = git_s(root, &["worktree", "list", "--porcelain"])?;
    for l in wl.lines() {
        if let Some(b) = l.strip_prefix("branch refs/heads/") {
            return Ok(b.to_string());
        }
        if l.is_empty() {
            break;
        }
    }
    git_s(root, &["rev-parse", "--abbrev-ref", "HEAD"])
}

// ---------- a round already folded into the main branch ----------
//
// A package is the round's change on top of an engine commit the server can
// build. Once a round is folded, its change is in the main branch itself: the
// merge-base with the main branch is the round's own commit, so there is no
// change on top, and the round's own parent may be older than the server's
// render app. Such a round travels as the main-branch commit that holds it:
// the first one after the fold that also has the render app and whose copy of
// every file the round changed is still the round's own (checked file by
// file). The patch is empty, the files are in code/base, and the manifest
// names the round's commit, the fold and the engine commit. A round whose
// files were changed again on the main branch since (a later round folded over
// it, say) has no such commit and is refused: it is no longer what the main
// branch holds.

/// The library entry's fields that are the review's record (score, status,
/// notes), written by folds after the round was scored, never what is built.
const LIBRARY_RECORD_FIELDS: &[&str] = &["realism", "notes", "status"];

/// Where a folded round joined the main branch.
pub struct FoldPoint {
    /// The main branch's first-parent history, oldest first.
    pub line: Vec<String>,
    /// The index in `line` of the first commit that holds the round.
    pub at: usize,
    /// That commit (the fold: a merge, or the round itself).
    pub fold: String,
    /// The engine the round was made on: its merge-base with the main branch
    /// just before the fold (the base an unfolded package would have had).
    pub round_base: String,
}

/// The first index in `line[lo..]` where `test` holds (it holds from there on).
fn first_true(
    line: &[String],
    lo: usize,
    test: &dyn Fn(&str) -> Result<bool, String>,
) -> Result<Option<usize>, String> {
    let (mut lo, mut hi) = (lo, line.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if test(&line[mid])? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok((lo < line.len()).then_some(lo))
}

pub fn fold_point(root: &Path, commit: &str, onto: &str) -> Result<FoldPoint, String> {
    let line: Vec<String> = git_s(root, &["rev-list", "--first-parent", "--reverse", onto])?
        .lines()
        .map(str::to_string)
        .collect();
    let has = |c: &str| -> Result<bool, String> {
        is_ancestor(root, commit, c).ok_or_else(|| format!("git merge-base --is-ancestor {commit} {c} failed"))
    };
    let at = first_true(&line, 0, &has)?.ok_or_else(|| {
        format!(
            "commons export: {} isn't in {onto}'s history",
            &commit[..12.min(commit.len())]
        )
    })?;
    let fold = line[at].clone();
    let before = git_s(root, &["rev-parse", "--verify", &format!("{fold}^1")]).map_err(|_| {
        "commons export: the round is the first commit of the main branch: there is no engine under it".to_string()
    })?;
    let round_base = git_s(root, &["merge-base", commit, &before])?;
    Ok(FoldPoint {
        line,
        at,
        fold,
        round_base,
    })
}

/// How one file of a folded round was found in the engine commit.
pub struct FoldedFile {
    pub path: String,
    pub status: &'static str,
    /// "file" (the same bytes), "library entry" (the asset's own entry in
    /// library.json, less its review record), "features" (the Cargo features
    /// the round changed), "dev scene" (never in the server's build; noted).
    pub compared: &'static str,
    /// The file at the engine commit, when it is carried in code/base.
    pub copy: Option<Vec<u8>>,
    pub engine_blob: Option<String>,
    pub round_blob: Option<String>,
}

fn blob_at(root: &Path, commit: &str, path: &str) -> Option<String> {
    git_s(root, &["rev-parse", "--verify", "-q", &format!("{commit}:{path}")]).ok()
}

fn library_asset(text: Option<&[u8]>, key: &str) -> Option<Value> {
    let v: Value = serde_json::from_slice(text?).ok()?;
    for c in v.get("categories")?.as_array()? {
        for a in c.get("assets")?.as_array()? {
            if a.get("feature").and_then(Value::as_str) == Some(key) {
                let mut a = a.clone();
                if let Some(o) = a.as_object_mut() {
                    for f in LIBRARY_RECORD_FIELDS {
                        o.remove(*f);
                    }
                }
                return Some(a);
            }
        }
    }
    None
}

fn cargo_features(text: Option<&[u8]>) -> BTreeMap<String, toml::Value> {
    text.and_then(|t| std::str::from_utf8(t).ok())
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("features").and_then(|f| f.as_table()).cloned())
        .map(|f| f.into_iter().collect())
        .unwrap_or_default()
}

/// Why `engine`'s copy of one round file isn't the round's: None when it is.
fn folded_file_problem(
    root: &Path,
    asset: &str,
    path: &str,
    round_base: &str,
    commit: &str,
    engine: &str,
) -> Result<(Option<String>, &'static str), String> {
    let short = &engine[..12.min(engine.len())];
    if path.starts_with(DEV_SCENE_PREFIX) {
        return Ok((None, "dev scene"));
    }
    if path == "catalog/procedural/library.json" {
        let r = git_show(root, commit, path)?;
        let e = git_show(root, engine, path)?;
        let (r, e) = (library_asset(r.as_deref(), asset), library_asset(e.as_deref(), asset));
        return Ok((
            (r != e).then(|| {
                format!("{path}: `{asset}`'s entry at {short} isn't the round's (beyond its review record)")
            }),
            "library entry",
        ));
    }
    if path.rsplit('/').next() == Some("Cargo.toml") {
        let b = cargo_features(git_show(root, round_base, path)?.as_deref());
        let r = cargo_features(git_show(root, commit, path)?.as_deref());
        let e = cargo_features(git_show(root, engine, path)?.as_deref());
        let touched: BTreeSet<&String> = b
            .keys()
            .chain(r.keys())
            .filter(|k| b.get(*k) != r.get(*k))
            .collect();
        let off: Vec<&str> = touched
            .into_iter()
            .filter(|k| r.get(*k) != e.get(*k))
            .map(String::as_str)
            .collect();
        return Ok((
            (!off.is_empty()).then(|| {
                format!("{path}: the feature(s) {} the round changed are different at {short}", off.join(", "))
            }),
            "features",
        ));
    }
    let (r, e) = (blob_at(root, commit, path), blob_at(root, engine, path));
    Ok((
        (r != e).then(|| format!("{path} at {short} isn't the round's file (changed again on the main branch since)")),
        "file",
    ))
}

/// The engine commit for a folded round: the first main-branch commit from
/// the fold on that has the render app and the round's own copy of every file
/// in `paths`. Err with every mismatch when there is none.
pub fn folded_engine(
    root: &Path,
    fp: &FoldPoint,
    render_app: &str,
    asset: &str,
    commit: &str,
    paths: &[(String, &'static str)],
) -> Result<(String, Vec<FoldedFile>), String> {
    // The render app's commit may be unknown to git (a test repository): then
    // every commit counts as having it, as the engine-base check does.
    let known = git_s(root, &["cat-file", "-e", &format!("{render_app}^{{commit}}")]).is_ok();
    let start = if known {
        first_true(&fp.line, fp.at, &|c| {
            is_ancestor(root, render_app, c).ok_or_else(|| "git merge-base failed".to_string())
        })?
        .ok_or_else(|| {
            "commons export refused (server preflight): the main branch doesn't have the server's render app yet: fold the main branch forward first".to_string()
        })?
    } else {
        fp.at
    };
    let first = fp.line[start].clone();
    let head = fp.line.last().cloned().unwrap_or_else(|| first.clone());
    // Only commits that changed one of the round's files can bring them back.
    let mut candidates = vec![first.clone()];
    if first != head {
        let mut a: Vec<String> = vec![
            "rev-list".into(),
            "--first-parent".into(),
            "--reverse".into(),
            format!("{first}..{head}"),
            "--".into(),
        ];
        a.extend(paths.iter().map(|(p, _)| p.clone()));
        let refs: Vec<&str> = a.iter().map(String::as_str).collect();
        candidates.extend(git_s(root, &refs)?.lines().map(str::to_string));
    }
    let mut first_problems = vec![];
    for c in &candidates {
        let mut problems = vec![];
        let mut files = vec![];
        for (p, status) in paths {
            let (why, compared) = folded_file_problem(root, asset, p, &fp.round_base, commit, c)?;
            if let Some(w) = why {
                problems.push(w);
                continue;
            }
            let engine_blob = blob_at(root, c, p);
            let copy = if compared == "file" && engine_blob.is_some() {
                git_show(root, c, p)?
            } else {
                None
            };
            files.push(FoldedFile {
                path: p.clone(),
                status,
                compared,
                copy,
                engine_blob,
                round_blob: blob_at(root, commit, p),
            });
        }
        if problems.is_empty() {
            return Ok((c.clone(), files));
        }
        if first_problems.is_empty() {
            first_problems = problems;
        }
    }
    Err(format!(
        "commons export refused: round {} was folded into the main branch ({}), but no main-branch commit with the server's render app still holds what was scored, so packing it would send something else:\n  {}\n  fix: pack the round the main branch holds now (its latest folded round), never an older one",
        &commit[..12.min(commit.len())],
        &fp.fold[..12.min(fp.fold.len())],
        first_problems.join("\n  ")
    ))
}

pub fn export(o: &ExportOpts) -> Result<ExportResult, String> {
    if !valid_key(&o.asset, 80) {
        return Err(format!(
            "--asset `{}`: a library key (lower-case letters, digits and -)",
            o.asset
        ));
    }
    if !valid_key(&o.style, 40) {
        return Err(format!(
            "--style `{}`: an art style id (lower-case letters, digits and -)",
            o.style
        ));
    }
    if o.score.is_none() || o.reviewer_model.trim().is_empty() {
        return Err(
            "commons export: a version needs its independent review: --score and --reviewer-model"
                .into(),
        );
    }
    if !o.independent {
        return Err("commons export: the score must come from an independent reviewer (--independent), never the author's own".into());
    }
    let categories = match &o.categories {
        Some(p) => {
            let t = std::fs::read_to_string(p)
                .map_err(|e| format!("--categories {}: {e}", p.display()))?;
            Some(categories_value(&t, o.score)?)
        }
        None => None,
    };
    if !o.author_model.is_empty()
        && o.author_model == o.reviewer_model
        && o.reviewer_account.is_empty()
    {
        return Err("commons export: the reviewer is named as the author's own model with no account of its own: not an independent review".into());
    }
    let root = &o.root;
    let commit = git_s(
        root,
        &["rev-parse", "--verify", &format!("{}^{{commit}}", o.commit)],
    )?;
    // `base`: the engine the round's change is made on. A round already folded
    // into the main branch (its merge-base with it is itself) is found where it
    // joined, and its change is the one it brought on the engine under it.
    let (base, fold_pt) = match &o.base {
        Some(b) => (
            git_s(root, &["rev-parse", "--verify", &format!("{b}^{{commit}}")])?,
            None,
        ),
        None => {
            let onto = match &o.onto {
                Some(x) => x.clone(),
                None => default_onto(root)?,
            };
            let mb = git_s(root, &["merge-base", &commit, &onto])?;
            if mb == commit {
                let fp = fold_point(root, &commit, &onto)?;
                (fp.round_base.clone(), Some(fp))
            } else {
                (mb, None)
            }
        }
    };
    if base == commit {
        return Err(format!(
            "commons export: {} has no change of its own on top of its base (give the column's branch with --branch, or --base)",
            &commit[..12.min(commit.len())]
        ));
    }
    if fold_pt.is_none()
        && let Some(why) = engine_base_problem(&base, has_render_app(root, &o.render_app, &base))
    {
        return Err(format!("commons export refused (server preflight): {why}"));
    }
    for p in &o.leave_out {
        if !p.starts_with(DEV_SCENE_PREFIX) {
            return Err(format!(
                "--leave-out {p}: only dev-scene files under {DEV_SCENE_PREFIX} may be left out (the server's build never uses them)"
            ));
        }
    }
    // What changed, and what of it may travel.
    let names = git_s(
        root,
        &["diff", "--name-status", "--no-renames", &base, &commit],
    )?;
    let mut paths = vec![];
    let mut left_out = vec![];
    for l in names.lines() {
        let Some((_, p)) = l.split_once('\t') else {
            continue;
        };
        let p = p.trim().to_string();
        if o.leave_out.contains(&p) {
            left_out.push(format!("{p} (--leave-out: dev scene only)"));
            continue;
        }
        match path_problem(&p) {
            None => paths.push(p),
            Some(why) => {
                // A change inside the library that must not travel is refused, and so is
                // a changed lock file or crate manifest elsewhere (new dependencies the
                // version would need); docs, DECISIONS and the like are just left out.
                let name = p.rsplit('/').next().unwrap_or(&p);
                if ALLOWED_DIRS.iter().any(|d| p.starts_with(d)) {
                    return Err(format!("commons export refused: {why}"));
                }
                if name == "Cargo.lock" || name == "Cargo.toml" {
                    return Err(format!(
                        "commons export refused: `{p}` changed: a version may not bring new dependencies or crates"
                    ));
                }
                left_out.push(p);
            }
        }
    }
    if let Some(p) = o
        .leave_out
        .iter()
        .find(|p| !left_out.iter().any(|l| l.starts_with(p.as_str())))
    {
        return Err(format!("--leave-out {p}: the round doesn't change that file"));
    }
    if paths.is_empty() {
        return Err(
            "commons export: nothing in the procedural library or material crates changed".into(),
        );
    }
    let mut args: Vec<&str> = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-renames",
        "--text",
        "--full-index",
        "-U3",
        &base,
        &commit,
        "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let patch = git(root, &args)?;
    let patch_text = String::from_utf8(patch.clone())
        .map_err(|_| "commons export: the diff is not UTF-8 text")?;
    let fps = parse_patch(&patch_text)?;
    let mut files = vec![];
    let mut base_blobs = Map::new();
    let mut problems = vec![];
    let mut bases: Vec<(String, Vec<u8>)> = vec![];
    for fp in &fps {
        let b = git_show(root, &base, &fp.path)?;
        let a = git_show(root, &commit, &fp.path)?;
        let rebuilt = apply(b.as_deref(), fp)?;
        if rebuilt != a {
            return Err(format!(
                "commons export: `{}`: the diff doesn't rebuild the version's file (a bug in the exporter: please report it)",
                fp.path
            ));
        }
        problems.extend(code_problems(fp, b.as_deref(), a.as_deref()));
        if let Some(bb) = &b {
            base_blobs.insert(
                fp.path.clone(),
                Value::String(git_s(root, &["rev-parse", &format!("{base}:{}", fp.path)])?),
            );
            bases.push((fp.path.clone(), bb.clone()));
        }
        files.push(json!({
            "path": fp.path, "status": fp.status.name(),
            "base_sha256": b.as_deref().map(sha_hex), "after_sha256": a.as_deref().map(sha_hex),
        }));
    }
    if !problems.is_empty() {
        return Err(format!(
            "commons export refused:\n  {}",
            problems.join("\n  ")
        ));
    }
    // The same patch rules Evani.App and the server run before they take it.
    let server = server_patch_problems(&patch_text);
    if !server.is_empty() {
        let dev: Vec<&str> = fps
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| p.starts_with(DEV_SCENE_PREFIX) && server.iter().any(|s| s.contains(*p)))
            .collect();
        let hint = if dev.is_empty() {
            String::new()
        } else {
            format!(
                "\n  {} is the dev scene (never in the server's build): pack again with {}",
                dev.join(", "),
                dev.iter()
                    .map(|p| format!("--leave-out {p}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        return Err(format!(
            "commons export refused (server preflight: Evani.App and the server would refuse it):\n  {}{hint}",
            server.join("\n  ")
        ));
    }
    // A folded round: the same checks ran on the change it brought; it travels
    // as the main-branch commit that holds it, with an empty patch.
    let folded = match &fold_pt {
        Some(fp) => {
            let with_status: Vec<(String, &'static str)> = fps
                .iter()
                .map(|f| (f.path.clone(), f.status.name()))
                .collect();
            let (engine, ffiles) =
                folded_engine(root, fp, &o.render_app, &o.asset, &commit, &with_status)?;
            if let Some(why) =
                engine_base_problem(&engine, has_render_app(root, &o.render_app, &engine))
            {
                return Err(format!("commons export refused (server preflight): {why}"));
            }
            Some((engine, ffiles))
        }
        None => None,
    };
    let engine = folded
        .as_ref()
        .map_or_else(|| base.clone(), |(e, _)| e.clone());
    let (patch, files, base_blobs, bases) = if folded.is_some() {
        (vec![], vec![], Map::new(), vec![])
    } else {
        (patch, files, base_blobs, bases)
    };
    // The asset, from the version's own library list.
    let lib_after = git_show(root, &commit, "catalog/procedural/library.json")?;
    let lib_base = git_show(root, &base, "catalog/procedural/library.json")?;
    let entry = lib_after
        .as_deref()
        .and_then(|t| library_entry(t, &o.asset));
    let (category, name) = entry.clone().unwrap_or_else(|| {
        (
            o.asset.split('-').next().unwrap_or("").to_string(),
            o.asset.clone(),
        )
    });
    let in_base = lib_base
        .as_deref()
        .and_then(|t| library_entry(t, &o.asset))
        .is_some();
    let ref_class = lib_after
        .as_deref()
        .and_then(|t| library_ref_class(t, &o.asset));
    let kind = budget_kind(ref_class.as_deref(), &category, &o.asset);
    let triangles_note = match &o.stats {
        Some(f) => {
            let t = std::fs::read_to_string(f).map_err(|e| format!("--stats {}: {e}", f.display()))?;
            let n = serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| v.get("triangles").and_then(Value::as_u64))
                .ok_or_else(|| {
                    format!(
                        "--stats {}: the render app's stats ({{\"triangles\": n, ...}}, asset-shoot.mjs --stats)",
                        f.display()
                    )
                })?;
            if let Some(why) = triangle_problem(n, kind, KIND_BUDGETS_LIVE) {
                return Err(format!(
                    "commons export refused (server preflight): {why} The column must bring the asset within the budget; never cut it down by hand to pass."
                ));
            }
            format!(
                "triangles: {n} at the {RENDER_TIER} tier, within the server's {} for {kind}",
                triangle_budget(kind, KIND_BUDGETS_LIVE)
            )
        }
        None => format!(
            "WARN: the triangle count wasn't checked (no --stats): the server's limit for {kind} is {}",
            triangle_budget(kind, KIND_BUDGETS_LIVE)
        ),
    };
    let area = o.area.clone().unwrap_or_else(|| {
        if in_base {
            "official".into()
        } else {
            "community".into()
        }
    });
    if area != "official" && area != "community" {
        return Err(format!("--area {area}: official or community"));
    }
    let engine_version = git_show(root, &engine, "Cargo.toml")?
        .and_then(|t| String::from_utf8(t).ok())
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| {
            t.get("workspace")?
                .get("package")?
                .get("version")?
                .as_str()
                .map(String::from)
        })
        .unwrap_or_default();
    let made = git_s(root, &["log", "-1", "--format=%cI", &commit])?;
    let author = match &o.author {
        Some(a) => a.clone(),
        None => git_s(root, &["log", "-1", "--format=%an", &commit])?,
    };
    let branch = o.commit.clone();

    // Build in a fresh staging folder, then rename to <asset>@<style>-<hash12>.
    // --dry-run: the same package, made in the temp folder and removed after.
    let out_dir = if o.dry_run {
        std::env::temp_dir().join(format!("evani-commons-dry-run-{}", std::process::id()))
    } else {
        o.out.clone()
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let stage = out_dir.join(format!(".staging-{}", std::process::id()));
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    }
    let res = (|| -> Result<ExportResult, String> {
        write_file(&stage.join("patch.diff"), &patch)?;
        for (p, b) in &bases {
            write_file(&stage.join("code").join("base").join(p), b)?;
        }
        // A folded round's files as they are at the engine commit (the round's own).
        let mut folded_json = Value::Null;
        if let Some((_, ffiles)) = &folded {
            let mut recs = vec![];
            for f in ffiles {
                if let Some(b) = &f.copy {
                    write_file(&stage.join("code").join("base").join(&f.path), b)?;
                }
                recs.push(json!({
                    "path": f.path, "status": f.status, "compared": f.compared,
                    "copy": f.copy.is_some(), "sha256": f.copy.as_deref().map(sha_hex),
                    "engine_blob": f.engine_blob, "round_blob": f.round_blob,
                }));
            }
            let fp = fold_pt.as_ref().ok_or("commons export: a folded round lost its fold")?;
            folded_json = json!({
                "round_commit": commit, "round_base": fp.round_base, "fold": fp.fold,
                "engine": engine, "files": recs,
            });
        }
        // Params: an object of finite numbers.
        let params: Map<String, Value> = match &o.params {
            Some(p) => {
                let t = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                let v: Value =
                    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))?;
                v.as_object()
                    .cloned()
                    .ok_or("--params: a JSON object of param names to numbers")?
            }
            None => Map::new(),
        };
        if let Some(p) = params_problem(&params) {
            return Err(p);
        }
        write_file(
            &stage.join("params.json"),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(params)).map_err(|e| e.to_string())?
            )
            .as_bytes(),
        )?;
        // The reference and its untouched original.
        let mut reference = json!({ "file": Value::Null, "sha256": Value::Null, "generator": o.reference_generator, "prompt": o.reference_prompt, "made": o.reference_made, "original": Value::Null });
        // `sha256` names the item's reference file itself (the server compares it with its
        // locked one; Evani.App's docs/evani-commons.md 2.3): never the hash of the small
        // reference.jpg preview, which is re-encoded. `source_sha256` is the picture the
        // round was scored against.
        let mut ref_note = None;
        if let Some(r) = &o.reference {
            let b = std::fs::read(r).map_err(|e| format!("{}: {e}", r.display()))?;
            let (j, _, _) = picture_jpeg(&b, REF_MAX_SIDE)
                .map_err(|e| format!("--reference {}: {e}", r.display()))?;
            write_file(&stage.join("reference.jpg"), &j)?;
            let scored = sha_hex(&b);
            let mut item_sha = scored.clone();
            if let Some(locked) = o.locked_sha256.as_deref().filter(|l| *l != scored) {
                let (from, diff) = locked_reference(&o.root, &o.asset, locked, &b)?;
                ref_note = Some(format!(
                    "reference: the round was scored against {} and Evani.app locked {} ({from}): the same picture (mean difference {diff:.1} of 255), so the package names the locked one",
                    &scored[..12],
                    &locked[..12]
                ));
                item_sha = locked.to_string();
            }
            reference["file"] = json!("reference.jpg");
            reference["sha256"] = json!(item_sha);
            reference["source_sha256"] = json!(scored);
        } else if o.locked_sha256.is_some() {
            return Err(
                "--locked-sha256 needs --reference (the picture the round was scored against)"
                    .into(),
            );
        }
        let mut warnings = vec![];
        if let Some(r) = &o.reference_original {
            let b = std::fs::read(r).map_err(|e| format!("{}: {e}", r.display()))?;
            if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(format!(
                    "--reference-original {}: the generator's untouched PNG",
                    r.display()
                ));
            }
            if b.len() as u64 > MAX_ORIGINAL {
                return Err(format!("--reference-original {}: over 8 MB", r.display()));
            }
            if !png_has_c2pa(&b) {
                warnings.push("the original reference has no C2PA content credentials: the server refuses it for a new Community item".to_string());
            }
            write_file(&stage.join("reference-original.png"), &b)?;
            reference["original"] = json!("reference-original.png");
            reference["original_sha256"] = json!(sha_hex(&b));
        } else if area == "community" {
            warnings.push(
                "a new Community item needs --reference-original (the generator's untouched PNG)"
                    .to_string(),
            );
        }
        // Round pictures.
        if o.pictures.len() > MAX_PICTURES {
            return Err(format!("at most {MAX_PICTURES} pictures"));
        }
        let mut pics = vec![];
        let mut seen = BTreeSet::new();
        for (k, (r, v, f)) in o.pictures.iter().enumerate() {
            let round = r.or(o.round).unwrap_or(0);
            let view = v.unwrap_or(k as u32);
            if !seen.insert((round, view)) {
                return Err(format!("two pictures for round {round} view {view}"));
            }
            let b = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
            let (j, w, h) = picture_jpeg(&b, ROUND_MAX_SIDE)
                .map_err(|e| format!("--picture {}: {e}", f.display()))?;
            let file = if view == 0 {
                format!("rounds/round{round}.jpg")
            } else {
                format!("rounds/round{round}-v{view}.jpg")
            };
            write_file(&stage.join(&file), &j)?;
            pics.push(json!({ "file": file, "round": round, "view": view, "sha256": sha_hex(&j), "width": w, "height": h }));
        }
        let year = made.get(..4).unwrap_or("2026").to_string();
        write_file(&stage.join("LICENSE"), mit(&year, &author).as_bytes())?;
        let score = o.score.unwrap_or(0.0);
        let readme = format!(
            "# {name} ({style})\n\nAn Evani.commons asset version: the procedural code of the library asset `{key}` in the `{style}` style, as Asset Studio round {round} left it.\n\n- Independent review: {score} of 5 ({rm}). The server renders and scores every shared version itself; this is the maker's review.\n- Built on Evani.Dev {ev} ({c12}).\n- Code: {code}\n\nAdopt it: `cargo xtask commons import <this folder>`, then `cargo xtask check`.\n\nLicence: MIT (see LICENSE).\n",
            name = name,
            style = o.style,
            key = o.asset,
            round = o.round.map_or("?".into(), |r| r.to_string()),
            score = score,
            rm = o.reviewer_model,
            ev = engine_version,
            c12 = &engine[..12.min(engine.len())],
            code = match (&folded, &fold_pt) {
                (Some((_, ff)), Some(fp)) => format!(
                    "already in that engine commit: the round ({r12}) was folded into Evani.Dev's main branch at {f12}, so `patch.diff` is empty and `code/base` holds the {n} file(s) it changed, as they are at that commit (the round's own; no meshes, no textures).",
                    r12 = &commit[..12.min(commit.len())],
                    f12 = &fp.fold[..12.min(fp.fold.len())],
                    n = ff.len()
                ),
                _ => format!(
                    "{} file(s) in `patch.diff` (no meshes, no textures).",
                    fps.len()
                ),
            }
        );
        write_file(&stage.join("README.md"), readme.as_bytes())?;
        let render = json!({
            "app": "examples/proc-field/apps/proc-field",
            "scene_feature": o.scene,
            "asset_feature": format!("evani-procedural-lib/{}", o.asset),
            "view_param": o.view_param,
            "views": o.views,
            "size": o.size,
        });
        let mut manifest = json!({
            "format": FORMAT,
            "asset": { "key": o.asset, "category": category, "name": name, "style": o.style, "feature": format!("evani-procedural-lib/{}", o.asset) },
            "area": area,
            "based_on": o.based_on,
            "version": { "column": o.column, "round": o.round, "branch": branch, "commit": commit, "made": made,
                         "fold": fold_pt.as_ref().map(|f| f.fold.clone()) },
            "score": { "value": score, "scale": "0-5, half stars", "independent": true, "reviewer_model": o.reviewer_model,
                       "reviewer_account": o.reviewer_account, "reviewed_at": o.reviewed_at, "author_model": o.author_model, "summary": o.summary },
            "reference": reference,
            "engine": { "version": engine_version, "commit": engine, "base_blobs": Value::Object(base_blobs.clone()) },
            "code": { "patch": "patch.diff",
                      "paths": if folded.is_some() { vec![] } else { fps.iter().map(|f| f.path.clone()).collect::<Vec<_>>() },
                      "sha256": sha_hex(&patch), "bytes": patch.len(), "base_dir": "code/base", "files": files },
            "params": { "file": "params.json" },
            "render": render,
            "licence": { "spdx": "MIT", "author": author, "year": year.parse::<u32>().unwrap_or(2026) },
            "pictures": pics,
            "content_hash": "",
        });
        if !folded_json.is_null() {
            manifest["code"]["folded"] = folded_json.clone();
        }
        if let Some(c) = &categories {
            // v1.5.36: a categories round carries its five scores and reasons (the radar chart's data)
            manifest["score"]["categories"] = c.clone();
        }
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?
        );
        write_file(&stage.join("commons.json"), text.as_bytes())?;
        let hash = content_hash(&stage)?;
        let filled = text.replacen(
            HASH_PLACEHOLDER,
            &format!("\"content_hash\": \"{hash}\""),
            1,
        );
        write_file(&stage.join("commons.json"), filled.as_bytes())?;
        let hex = hash.trim_start_matches("sha256:");
        let dir = out_dir.join(format!("{}@{}-{}", o.asset, o.style, &hex[..12]));
        // The same content hash: the folder is this package already. A copy that no longer
        // verifies (build output added after it was made, say a target/ from a cargo run in
        // that folder) is replaced by the clean one just made.
        if dir.exists() && verify(&dir).is_ok() {
            std::fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
        } else {
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::rename(&stage, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let pkg = verify(&dir)?;
        let mut report = pkg.summary();
        if !left_out.is_empty() {
            report.push_str(&format!(
                "\nleft out (not library code): {}",
                left_out.join(", ")
            ));
        }
        if let Some(n) = &ref_note {
            report.push_str(&format!("\n{n}"));
        }
        report.push_str(&format!("\n{triangles_note}"));
        for w in &warnings {
            report.push_str(&format!("\nWARN: {w}"));
        }
        if let (Some((e, ff)), Some(fp)) = (&folded, &fold_pt) {
            report.push_str(&format!(
                "\nfolded round: {} joined the main branch at {}; packed as engine commit {} (it has the server's render app and the round's own copy of: {}), with an empty patch",
                &commit[..12.min(commit.len())],
                &fp.fold[..12.min(fp.fold.len())],
                &e[..12.min(e.len())],
                ff.iter()
                    .map(|f| format!("{} ({})", f.path, f.compared))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if o.dry_run {
            report.push_str("\ncommons export --dry-run: every check passed; nothing kept");
        }
        Ok(ExportResult {
            dir,
            content_hash: hash,
            report,
        })
    })();
    if res.is_err() && stage.exists() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    if o.dry_run {
        let _ = std::fs::remove_dir_all(&out_dir);
    }
    res
}

fn mit(year: &str, author: &str) -> String {
    format!(
        "MIT License\n\nCopyright (c) {year} {author}\n\nPermission is hereby granted, free of charge, to any person obtaining a copy\nof this software and associated documentation files (the \"Software\"), to deal\nin the Software without restriction, including without limitation the rights\nto use, copy, modify, merge, publish, distribute, sublicense, and/or sell\ncopies of the Software, and to permit persons to whom the Software is\nfurnished to do so, subject to the following conditions:\n\nThe above copyright notice and this permission notice shall be included in all\ncopies or substantial portions of the Software.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\nIMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\nFITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE\nAUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\nLIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,\nOUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE\nSOFTWARE.\n"
    )
}

fn params_problem(p: &Map<String, Value>) -> Option<String> {
    for (k, v) in p {
        if k.is_empty()
            || k.len() > 120
            || !k
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
        {
            return Some(format!(
                "params.json: `{k}` is not a param name (kit.<id>.<name>)"
            ));
        }
        if !v.as_f64().is_some_and(f64::is_finite) {
            return Some(format!("params.json: `{k}` is not a finite number"));
        }
        if server_owned(k) {
            return Some(format!(
                "params.json: `{k}` is the server's to set (its cameras, sun, staging and studio); leave it out"
            ));
        }
    }
    None
}

/// The render app's params the server sets itself on every view (its
/// cameras, sun, staging, grey studio sweep and per-object studio): a
/// version may not set them (Evani.App's server refuses one that does,
/// 2026-11-11), so the exporter never writes them.
pub(crate) fn server_owned(k: &str) -> bool {
    let Some(rest) = k.strip_prefix("kit.proc_field.") else {
        return false;
    };
    ["cam_", "sun_", "sweep_", "studio_"].iter().any(|p| rest.starts_with(p)) || matches!(rest, "staging" | "view_preset")
}

/// Every safety problem of one file's change.
pub fn code_problems(fp: &FilePatch, base: Option<&[u8]>, after: Option<&[u8]>) -> Vec<String> {
    let mut out = vec![];
    if let Some(p) = path_problem(&fp.path) {
        out.push(p);
        return out;
    }
    if fp.path.ends_with("/Cargo.toml") || fp.path == "Cargo.toml" {
        out.extend(cargo_toml_problem(&fp.path, base, after));
    }
    if fp.path.ends_with(".rs") {
        for (n, l) in fp.added() {
            if let Some(why) = line_problem(&l) {
                out.push(format!(
                    "`{}` near line {n}: {why} (`{}`), which shared library code may not do",
                    fp.path,
                    l.trim().chars().take(80).collect::<String>()
                ));
            }
        }
    }
    out
}

// ---------- verify ----------

pub struct Package {
    pub manifest: Value,
    pub patches: Vec<FilePatch>,
    pub bases: BTreeMap<String, Vec<u8>>,
    /// A folded round's files (code.folded): its copy at the engine commit,
    /// None for a file the round deleted.
    pub folded: BTreeMap<String, Option<Vec<u8>>>,
    pub content_hash: String,
    pub bytes: u64,
    pub sizes: BTreeMap<&'static str, u64>,
}
impl Package {
    pub fn key(&self) -> String {
        format!(
            "{}@{}",
            self.manifest["asset"]["key"].as_str().unwrap_or("?"),
            self.manifest["asset"]["style"].as_str().unwrap_or("?")
        )
    }
    pub fn hash12(&self) -> String {
        self.content_hash
            .trim_start_matches("sha256:")
            .chars()
            .take(12)
            .collect()
    }
    pub fn score(&self) -> Option<f64> {
        self.manifest["score"]["value"].as_f64()
    }
    pub fn summary(&self) -> String {
        let m = &self.manifest;
        let parts: Vec<String> = self
            .sizes
            .iter()
            .map(|(k, v)| format!("{k} {}", kb(*v)))
            .collect();
        format!(
            "{} ({} area), round {}, independent score {} by {}\n  built on engine {} {}\n  code: {} file(s): {}\n  size: {} in total ({})",
            self.key(),
            m["area"].as_str().unwrap_or("?"),
            m["version"]["round"],
            m["score"]["value"],
            m["score"]["reviewer_model"].as_str().unwrap_or("?"),
            m["engine"]["version"].as_str().unwrap_or("?"),
            m["engine"]["commit"]
                .as_str()
                .map(|c| &c[..12.min(c.len())])
                .unwrap_or("?"),
            if m["code"]["folded"].is_object() {
                m["code"]["folded"]["files"].as_array().map_or(0, Vec::len)
            } else {
                self.patches.len()
            },
            if m["code"]["folded"].is_object() {
                format!(
                    "already in the engine commit (round {} folded at {}; empty patch): {}",
                    m["code"]["folded"]["round_commit"].as_str().map_or("?", |c| &c[..12.min(c.len())]),
                    m["code"]["folded"]["fold"].as_str().map_or("?", |c| &c[..12.min(c.len())]),
                    m["code"]["folded"]["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|f| format!(
                            "{} ({}, {})",
                            f["path"].as_str().unwrap_or("?"),
                            f["status"].as_str().unwrap_or("?"),
                            f["compared"].as_str().unwrap_or("?")
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                self.patches
                    .iter()
                    .map(|p| format!("{} ({})", p.path, p.status.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            kb(self.bytes),
            parts.join(", ")
        )
    }
}
fn kb(b: u64) -> String {
    if b >= 1024 * 1024 {
        format!("{:.2} MB", b as f64 / 1048576.0)
    } else {
        format!("{:.1} KB", b as f64 / 1024.0)
    }
}

/// Build output that can land in a package folder after it is made (a cargo
/// run there writes target/; tools leave lock files, .d and .rlib files).
pub fn is_build_output(f: &str) -> bool {
    const DIRS: &[&str] = &[
        "target",
        "node_modules",
        ".evani",
        ".vibedeck",
        ".git",
        ".cargo",
        ".fingerprint",
    ];
    const EXT: &[&str] = &[
        "lock", "d", "rlib", "rmeta", "o", "obj", "pdb", "exe", "dll", "tmp", "log",
    ];
    let parts: Vec<&str> = f.split('/').collect();
    if parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|p| DIRS.contains(p))
    {
        return true;
    }
    let name = parts.last().copied().unwrap_or("");
    matches!(name, "CACHEDIR.TAG" | ".rustc_info.json")
        || name.rsplit_once('.').is_some_and(|(_, e)| EXT.contains(&e))
}

fn allowed_package_file(f: &str) -> bool {
    if matches!(
        f,
        "commons.json"
            | "patch.diff"
            | "params.json"
            | "reference.jpg"
            | "reference-original.png"
            | "LICENSE"
            | "README.md"
    ) {
        return true;
    }
    if let Some(r) = f
        .strip_prefix("rounds/round")
        .and_then(|r| r.strip_suffix(".jpg"))
    {
        let (a, b) = r.split_once("-v").unwrap_or((r, "0"));
        return !a.is_empty()
            && a.bytes().all(|c| c.is_ascii_digit())
            && !b.is_empty()
            && b.bytes().all(|c| c.is_ascii_digit());
    }
    if let Some(p) = f.strip_prefix("code/base/") {
        return path_problem(p).is_none();
    }
    false
}

/// Check a package folder: names and sizes, the content hash, the licence, the
/// params, the patch (allowed paths, code safety, every file rebuilt exactly).
pub fn verify(dir: &Path) -> Result<Package, String> {
    let bad = |m: String| format!("commons: {} is not a usable package: {m}", dir.display());
    let files = list_files(dir).map_err(bad)?;
    if files.len() > MAX_FILES {
        return Err(bad(format!("{} files (at most {MAX_FILES})", files.len())));
    }
    let mut total = 0u64;
    let mut sizes: BTreeMap<&'static str, u64> = BTreeMap::new();
    for f in &files {
        if !allowed_package_file(f) {
            if is_build_output(f) {
                return Err(bad(format!(
                    "`{f}` is build output, not a package file (a build ran inside the package folder): run `cargo xtask commons export` again from the project folder, which replaces the folder with a clean one"
                )));
            }
            return Err(bad(format!("`{f}` doesn't belong in a package")));
        }
        let n = std::fs::metadata(dir.join(f))
            .map_err(|e| bad(format!("{f}: {e}")))?
            .len();
        let cap = if f == "reference-original.png" {
            MAX_ORIGINAL
        } else {
            MAX_FILE
        };
        if n > cap {
            return Err(bad(format!("`{f}` is {} (at most {})", kb(n), kb(cap))));
        }
        total += n;
        let part = if f.starts_with("rounds/") {
            "pictures"
        } else if f.starts_with("reference") {
            "reference"
        } else if f.starts_with("code/") || f == "patch.diff" {
            "code"
        } else {
            "manifest and licence"
        };
        *sizes.entry(part).or_default() += n;
    }
    if total > MAX_TOTAL {
        return Err(bad(format!(
            "{} in total (at most {})",
            kb(total),
            kb(MAX_TOTAL)
        )));
    }
    let text = std::fs::read_to_string(dir.join("commons.json"))
        .map_err(|e| bad(format!("commons.json: {e}")))?;
    let m: Value = serde_json::from_str(&text).map_err(|e| bad(format!("commons.json: {e}")))?;
    if m["format"].as_str() != Some(FORMAT) {
        return Err(bad(format!(
            "format is {} (this engine reads {FORMAT})",
            m["format"]
        )));
    }
    let hash = content_hash(dir).map_err(bad)?;
    if m["content_hash"].as_str() != Some(hash.as_str()) {
        return Err(bad(
            "its content hash doesn't match its files: it was changed after it was made".into(),
        ));
    }
    let key = m["asset"]["key"].as_str().unwrap_or("");
    let style = m["asset"]["style"].as_str().unwrap_or("");
    if !valid_key(key, 80) || !valid_key(style, 40) {
        return Err(bad("asset key or style is not a plain id".into()));
    }
    if m["licence"]["spdx"].as_str() != Some("MIT") || !files.iter().any(|f| f == "LICENSE") {
        return Err(bad(
            "Evani.commons takes MIT only, with its LICENSE file".into()
        ));
    }
    let params: Value = serde_json::from_slice(
        &std::fs::read(dir.join("params.json")).map_err(|e| bad(format!("params.json: {e}")))?,
    )
    .map_err(|e| bad(format!("params.json: {e}")))?;
    let params = params
        .as_object()
        .ok_or_else(|| bad("params.json is not an object".into()))?;
    if let Some(p) = params_problem(params) {
        return Err(bad(p));
    }
    let patch =
        std::fs::read(dir.join("patch.diff")).map_err(|e| bad(format!("patch.diff: {e}")))?;
    if m["code"]["sha256"].as_str() != Some(sha_hex(&patch).as_str()) {
        return Err(bad("patch.diff doesn't match its record".into()));
    }
    let patches = parse_patch(
        &String::from_utf8(patch).map_err(|_| bad("patch.diff is not UTF-8 text".into()))?,
    )
    .map_err(bad)?;
    let listed: BTreeSet<&str> = m["code"]["paths"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let in_patch: BTreeSet<&str> = patches.iter().map(|p| p.path.as_str()).collect();
    let folded_meta = m["code"]["folded"].as_object().cloned();
    if listed != in_patch || (patches.is_empty() && folded_meta.is_none()) {
        return Err(bad("the files in patch.diff don't match code.paths".into()));
    }
    // A folded round: no patch; code/base holds the files it changed, as they
    // are at the engine commit (the round's own, checked when it was packed).
    let mut folded = BTreeMap::new();
    if let Some(f) = &folded_meta {
        if !patches.is_empty() {
            return Err(bad("a folded version carries no patch: its code is in its engine commit".into()));
        }
        if f.get("engine").and_then(Value::as_str) != m["engine"]["commit"].as_str()
            || f.get("round_commit").and_then(Value::as_str) != m["version"]["commit"].as_str()
        {
            return Err(bad("code.folded doesn't name the version's engine and round commits".into()));
        }
        for r in f.get("files").and_then(Value::as_array).into_iter().flatten() {
            let path = r["path"]
                .as_str()
                .filter(|p| path_problem(p).is_none())
                .ok_or_else(|| bad("code.folded lists a path a version may not hold".into()))?;
            if r["copy"].as_bool() == Some(true) {
                let b = std::fs::read(dir.join("code").join("base").join(path)).map_err(|_| {
                    bad(format!("`{path}` has no copy in code/base"))
                })?;
                if r["sha256"].as_str() != Some(sha_hex(&b).as_str()) {
                    return Err(bad(format!("`{path}`: its copy doesn't match its record")));
                }
                folded.insert(path.to_string(), Some(b));
            } else if r["compared"].as_str() == Some("file") && r["status"].as_str() == Some("deleted") {
                folded.insert(path.to_string(), None);
            }
        }
        if let Some(extra) = files
            .iter()
            .filter_map(|x| x.strip_prefix("code/base/"))
            .find(|p| !matches!(folded.get(*p), Some(Some(_))))
        {
            return Err(bad(format!("`code/base/{extra}` isn't one of the folded round's files")));
        }
    }
    let records: BTreeMap<&str, &Value> = m["code"]["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| Some((f["path"].as_str()?, f)))
                .collect()
        })
        .unwrap_or_default();
    let mut bases = BTreeMap::new();
    let mut problems = vec![];
    for fp in &patches {
        let rec = records
            .get(fp.path.as_str())
            .ok_or_else(|| bad(format!("`{}` has no record in code.files", fp.path)))?;
        let base_file = dir.join("code").join("base").join(&fp.path);
        let base = if fp.status == Status::Added {
            None
        } else {
            let b = std::fs::read(&base_file)
                .map_err(|_| bad(format!("`{}` has no base copy in code/base", fp.path)))?;
            if rec["base_sha256"].as_str() != Some(sha_hex(&b).as_str()) {
                return Err(bad(format!(
                    "`{}`: its base copy doesn't match its record",
                    fp.path
                )));
            }
            Some(b)
        };
        let after = apply(base.as_deref(), fp).map_err(bad)?;
        if rec["after_sha256"].as_str() != after.as_deref().map(sha_hex).as_deref() {
            return Err(bad(format!(
                "`{}`: the patch doesn't rebuild the recorded file",
                fp.path
            )));
        }
        problems.extend(code_problems(fp, base.as_deref(), after.as_deref()));
        if let Some(b) = base {
            bases.insert(fp.path.clone(), b);
        }
    }
    if !problems.is_empty() {
        return Err(bad(format!(
            "unsafe or out-of-bounds code:\n  {}",
            problems.join("\n  ")
        )));
    }
    if let Some(pics) = m["pictures"].as_array() {
        for p in pics {
            let f = p["file"].as_str().unwrap_or("");
            let b =
                std::fs::read(dir.join(f)).map_err(|_| bad(format!("picture `{f}` is missing")))?;
            if p["sha256"].as_str() != Some(sha_hex(&b).as_str()) {
                return Err(bad(format!("picture `{f}` doesn't match its record")));
            }
        }
    }
    Ok(Package {
        manifest: m,
        patches,
        bases,
        folded,
        content_hash: hash,
        bytes: total,
        sizes,
    })
}

// ---------- import (adopt) ----------

pub struct ImportOpts {
    pub root: PathBuf,
    pub dry_run: bool,
    pub server_score: Option<f64>,
    pub version_id: Option<String>,
    pub check: bool,
    /// The adoption time to record (tests pin it; default: the package's own date).
    pub when: Option<String>,
}

pub struct ImportResult {
    pub written: Vec<String>,
    pub already: Vec<String>,
    pub merged: Vec<String>,
    pub report: String,
}

enum Plan {
    Write(Vec<u8>),
    Delete,
}

/// 3-way merge of one text file; Ok(None) on a conflict.
pub fn merge3(ours: &[u8], base: &[u8], theirs: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let dir = std::env::temp_dir().join(format!(
        "evani-commons-merge-{}-{}",
        std::process::id(),
        MERGE_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let r = (|| {
        let (o, b, t) = (dir.join("ours"), dir.join("base"), dir.join("theirs"));
        std::fs::write(&o, ours).map_err(|e| e.to_string())?;
        std::fs::write(&b, base).map_err(|e| e.to_string())?;
        std::fs::write(&t, theirs).map_err(|e| e.to_string())?;
        let out = Command::new("git")
            .args([
                "merge-file",
                "-p",
                "-L",
                "this project",
                "-L",
                "the version's base",
                "-L",
                "the version",
            ])
            .arg(&o)
            .arg(&b)
            .arg(&t)
            .output()
            .map_err(|e| {
                format!("git merge-file: {e} (adopting into a changed file needs git on the PATH)")
            })?;
        match out.status.code() {
            Some(0) => Ok(Some(out.stdout)),
            Some(c) if c > 0 => Ok(None),
            _ => Err(format!(
                "git merge-file: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    r
}
static MERGE_N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The project gate, run in-process: 0 when clean.
pub fn run_check(root: &Path) -> i32 {
    evani_guide::cli::run_in(Some(root), &["check".to_string()])
}

/// Adopt a package into the project at `o.root`: every file is planned first
/// (a 3-way merge when the project's copy changed since the version's base),
/// and nothing is written unless every file merges cleanly. With `check`,
/// the project gate runs after, and a check that was clean before and isn't
/// after undoes the whole import.
pub fn import(
    dir: &Path,
    o: &ImportOpts,
    check: &dyn Fn(&Path) -> i32,
) -> Result<ImportResult, String> {
    let pkg = verify(dir)?;
    let root = &o.root;
    let mut plans: Vec<(String, Plan, bool)> = vec![];
    let (mut already, mut merged, mut conflicts) = (vec![], vec![], vec![]);
    for fp in &pkg.patches {
        let base = pkg.bases.get(&fp.path).map(Vec::as_slice);
        let theirs = apply(base, fp)?;
        let target = root.join(&fp.path);
        let ours_raw = if target.exists() {
            Some(std::fs::read(&target).map_err(|e| format!("{}: {e}", target.display()))?)
        } else {
            None
        };
        let (ours, crlf) = match &ours_raw {
            Some(b) => {
                let (x, c) = lf(b);
                (Some(x), c)
            }
            None => (None, false),
        };
        let plan = match (fp.status, ours.as_deref(), theirs.as_deref()) {
            (_, o2, t) if o2 == t => {
                already.push(fp.path.clone());
                continue;
            }
            (Status::Added, None, Some(t)) => Plan::Write(t.to_vec()),
            (Status::Deleted, Some(x), None) if Some(x) == base => Plan::Delete,
            (Status::Modified, Some(x), Some(t)) if Some(x) == base => Plan::Write(t.to_vec()),
            (Status::Modified, Some(x), Some(t)) => match merge3(x, base.unwrap_or(b""), t)? {
                Some(m) => {
                    merged.push(fp.path.clone());
                    Plan::Write(m)
                }
                None => {
                    conflicts.push(format!("{} (changed here in the same places)", fp.path));
                    continue;
                }
            },
            (Status::Modified, None, _) => {
                conflicts.push(format!("{} (missing in this project)", fp.path));
                continue;
            }
            (Status::Added, Some(_), _) => {
                conflicts.push(format!(
                    "{} (this project already has a different file there)",
                    fp.path
                ));
                continue;
            }
            _ => {
                conflicts.push(format!(
                    "{} (changed here since the version's base)",
                    fp.path
                ));
                continue;
            }
        };
        plans.push((fp.path.clone(), plan, crlf));
    }
    let c12 = pkg.manifest["engine"]["commit"]
        .as_str()
        .map(|c| c[..12.min(c.len())].to_string())
        .unwrap_or_default();
    // A folded round is part of its engine commit: adopted when this project's
    // files are that commit's; never written over a different copy (there is
    // no older base to merge with).
    let mut folded_off = vec![];
    for (path, copy) in &pkg.folded {
        let target = root.join(path);
        let ours = std::fs::read(&target).ok().map(|b| lf(&b).0);
        let theirs = copy.as_deref().map(|b| lf(b).0);
        if ours == theirs {
            already.push(path.clone());
        } else {
            folded_off.push(path.clone());
        }
    }
    if !folded_off.is_empty() {
        return Err(format!(
            "This version is part of engine commit {c12} (its round was folded into Evani.Dev's main branch), and this project's copy differs. Nothing was changed.\n  {}\n  fix: update this project's engine to {c12} or later",
            folded_off.join("\n  ")
        ));
    }
    if !conflicts.is_empty() {
        return Err(format!(
            "This version can't be adopted into this project: its code was made on engine {c12} and this project's copy differs where it changes things. Nothing was changed.\n  {}\n  fix: update this project's engine to {c12} or later, or pick a version made on your engine",
            conflicts.join("\n  ")
        ));
    }
    let written: Vec<String> = plans.iter().map(|(p, _, _)| p.clone()).collect();
    let mut report = format!(
        "{}: {} file(s) to change, {} already as the version",
        pkg.key(),
        written.len(),
        already.len()
    );
    if !merged.is_empty() {
        report.push_str(&format!(
            ", {} merged with this project's own changes ({})",
            merged.len(),
            merged.join(", ")
        ));
    }
    if o.dry_run {
        report.push_str("\ncommons import --dry-run: nothing written");
        return Ok(ImportResult {
            written,
            already,
            merged,
            report,
        });
    }
    let before_clean = if o.check {
        Some(check(root) == 0)
    } else {
        None
    };
    // Write: keep every original in memory so a failure puts them all back.
    let catalog = root.join("catalog").join("procedural").join("commons.json");
    let refs = root
        .join("catalog")
        .join("procedural")
        .join("references.json");
    let mut undo: Vec<(PathBuf, Option<Vec<u8>>)> = vec![];
    for p in [&catalog, &refs] {
        undo.push((p.to_path_buf(), std::fs::read(p).ok()));
    }
    let restore = |undo: &[(PathBuf, Option<Vec<u8>>)]| {
        for (p, b) in undo.iter().rev() {
            let _ = match b {
                Some(b) => std::fs::write(p, b),
                None => std::fs::remove_file(p).or(Ok(())),
            };
        }
    };
    let res = (|| -> Result<(), String> {
        for (path, plan, crlf) in &plans {
            let target = root.join(path);
            undo.push((target.clone(), std::fs::read(&target).ok()));
            match plan {
                Plan::Write(b) => {
                    let bytes = if *crlf { to_crlf(b) } else { b.clone() };
                    let tmp = target.with_extension("commons-tmp");
                    write_file(&tmp, &bytes)?;
                    std::fs::rename(&tmp, &target)
                        .map_err(|e| format!("{}: {e}", target.display()))?;
                }
                Plan::Delete => std::fs::remove_file(&target)
                    .map_err(|e| format!("{}: {e}", target.display()))?,
            }
        }
        record_adoption(&pkg, o, &catalog, &written)?;
        if let Ok(t) = std::fs::read_to_string(&refs) {
            let score = o.server_score.or(pkg.score());
            if let (Some(s), Some(key)) = (score, pkg.manifest["asset"]["key"].as_str())
                && let Some(nt) = set_reference_score(&t, key, s)
            {
                std::fs::write(&refs, nt).map_err(|e| format!("{}: {e}", refs.display()))?;
            }
        }
        Ok(())
    })();
    if let Err(e) = res {
        restore(&undo);
        return Err(format!(
            "commons import failed and was undone (nothing changed): {e}"
        ));
    }
    if let Some(was_clean) = before_clean {
        let now_clean = check(root) == 0;
        if was_clean && !now_clean {
            restore(&undo);
            return Err("This version was undone: the project's check was clean before it and isn't with it. Nothing was changed.".into());
        }
        report.push_str(if now_clean {
            "\ncheck: clean"
        } else {
            "\ncheck: not clean (it wasn't clean before the import either)"
        });
    }
    report.push_str(&format!(
        "\ncommons import: adopted {} ({}), recorded in catalog/procedural/commons.json",
        pkg.key(),
        pkg.hash12()
    ));
    Ok(ImportResult {
        written,
        already,
        merged,
        report,
    })
}

fn record_adoption(
    pkg: &Package,
    o: &ImportOpts,
    catalog: &Path,
    written: &[String],
) -> Result<(), String> {
    let mut v: Value = match std::fs::read_to_string(catalog) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", catalog.display()))?,
        Err(_) => {
            json!({ "format": ADOPTED_FORMAT, "about": "Evani.commons versions adopted into this project (cargo xtask commons import; docs/EVANI_COMMONS.md). Written by the importer.", "adopted": {} })
        }
    };
    let m = &pkg.manifest;
    let rec = json!({
        "version_id": o.version_id.clone().unwrap_or_else(|| pkg.hash12()),
        "content_hash": pkg.content_hash,
        "score": m["score"]["value"],
        "reviewer_model": m["score"]["reviewer_model"],
        "server_score": o.server_score,
        "engine_commit": m["engine"]["commit"],
        "author": m["licence"]["author"],
        "area": m["area"],
        "round": m["version"]["round"],
        "files": written,
        "adopted_at": o.when.clone().unwrap_or_else(|| m["version"]["made"].as_str().unwrap_or("").to_string()),
    });
    let adopted = v
        .get_mut("adopted")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{}: no \"adopted\" object", catalog.display()))?;
    adopted.insert(pkg.key(), rec);
    write_file(
        catalog,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        )
        .as_bytes(),
    )
}

/// references.json with `assets.<key>.score` set, changing only that value
/// (the file keeps its order and layout); None when the asset isn't there.
pub fn set_reference_score(text: &str, key: &str, score: f64) -> Option<String> {
    let open = format!("\"{key}\": {{");
    let start = text.find(&open)? + open.len();
    // The object's end: count braces outside strings.
    let b = text.as_bytes();
    let (mut depth, mut i, mut in_str, mut esc) = (1i32, start, false, false);
    while i < b.len() && depth > 0 {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
        }
        i += 1;
    }
    let obj = &text[start..i];
    let k = obj.find("\"score\": ")? + "\"score\": ".len();
    let vstart = start + k;
    let vend = vstart + text[vstart..].find([',', '\n', '}'])?;
    let val = if score.fract() == 0.0 {
        format!("{score:.1}")
    } else {
        format!("{score}")
    };
    Some(format!("{}{}{}", &text[..vstart], val, &text[vend..]))
}

#[cfg(test)]
#[path = "commons_tests.rs"]
mod tests;
