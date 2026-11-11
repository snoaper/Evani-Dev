//! `cargo xtask engine-pack`: the versioned, pinned engine pack that ships
//! inside every Evani.App install (docs/ENGINE_PACK.md; plan:
//! .vibedeck/plans/engine-in-app-roadmap.md, Phase 1a).
//!
//! One folder per pack version and platform:
//! ```text
//! evani-engine/<pack_version>/
//!   manifest.json         evani-engine-pack/1: versions, commit, pins, parts, sha256 per file
//!   engine/               the allow-list of packaging/engine-pack.json at the commit (git
//!                         blobs, so the working tree never leaks in), trimmed workspace
//!   vendor/               `cargo vendor` of the engine's Cargo.lock (offline builds)
//!   cargo-config.toml     the source-replacement config, with {PACK} for the pack folder
//!   toolchain/            rustc, cargo, rustdoc, clippy, rustfmt, host std, wasm32 std,
//!                         rust-lld; no .pdb, docs or rust-src
//!   tools/bin/            our prebuilt tools (`evani` = xtask, `cargo-xtask`, reqcheck,
//!                         live bridge, convert, model) and trunk, wasm-bindgen, wasm-opt
//!   tools/convert/        the model-import and font helpers (`EVANI_TOOLS_DIR`)
//!   LICENSE, THIRD_PARTY_NOTICES.md
//! ```
//! Reproducible: files come from git blobs and pinned copies, file order is
//! sorted, `created` is the commit time and every file's modified time is
//! set to it, our tools are built from the pack's own source in a fixed
//! build folder with `--locked --offline`, path prefixes remapped and (MSVC)
//! `/Brepro`. `--repro-check` builds twice from a wiped build folder and
//! requires identical manifests; `--verify` re-hashes a pack.
//!
//! `--download` fetches trunk, wasm-bindgen and wasm-opt from their pinned
//! release URLs into `--cache` and refuses any archive whose SHA-256 is not
//! the pinned `sha256_upstream`. `--archive` also writes the hostable form
//! (route B, docs/ENGINE_PACK.md 4.8): one deterministic `.tar.gz` of the pack
//! and a small JSON descriptor with its hash and the signature slot.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "evani-engine-pack/1";
pub const PINS_SCHEMA: &str = "evani-engine-pack-pins/1";
pub const PINS_FILE: &str = "packaging/engine-pack.json";
/// Where the pack's tools look for the engine (set by Evani.App; see
/// [`engine_root_from`]).
pub const ENGINE_ROOT_VAR: &str = "EVANI_ENGINE_ROOT";

pub fn usage() -> &'static str {
    "  cargo xtask engine-pack [--platform windows-x64|macos-arm64|macos-x64] [--out <dir>] [--build-dir <dir>] [--commit <rev>] [--tools-from <dir>] [--download [--cache <dir>]] [--archive] [--sign-cmd \"<command with {file}>\" --signer <name>] [--skip-prebuilt] [--allow-incomplete] [--force]\n  cargo xtask engine-pack --repro-check [same options]   (packs twice from a wiped build folder; manifests must match)\n  cargo xtask engine-pack --verify <pack folder>         (re-hashes every file against manifest.json)\n  cargo xtask doctor --requirements [--json] [--no-probe] [--hide <ids>]   (the build requirements checker)"
}

// ---------------------------------------------------------------- pins

#[derive(Clone, Debug, PartialEq)]
pub struct RustPin {
    pub version: String,
    pub commit: String,
    pub hosts: BTreeMap<String, String>,
    pub targets: Vec<String>,
    pub components: Vec<String>,
    pub bin: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prebuilt {
    pub package: String,
    pub bin: String,
    /// The name it ships under in `tools/bin` (default: `bin`).
    pub ship_as: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolPin {
    pub name: String,
    pub version: String,
    pub licence: String,
    /// `bin` (one program into tools/bin) or `convert` (a folder into tools/convert).
    pub kind: String,
    pub platforms: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pins {
    pub pack_version: String,
    pub live_protocol: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub members: Vec<String>,
    pub parts: Vec<(String, Vec<String>)>,
    pub rust: RustPin,
    pub prebuilt: Vec<Prebuilt>,
    pub tools: Vec<ToolPin>,
    pub requirements: BTreeMap<String, Vec<String>>,
    /// Programs allowed to import the dynamic C runtime, per platform, each
    /// with its reason ([`check_runtime`]).
    pub runtime_allow: BTreeMap<String, Vec<RuntimeAllow>>,
}

/// One program that may import the dynamic C runtime, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeAllow {
    pub path: String,
    pub reason: String,
}

fn strs(v: &Value, what: &str) -> Result<Vec<String>, String> {
    v.as_array().ok_or_else(|| format!("{PINS_FILE}: {what} must be a list"))?.iter().map(|s| s.as_str().map(str::to_string).ok_or_else(|| format!("{PINS_FILE}: {what} must hold strings"))).collect()
}

fn s(v: &Value, what: &str) -> Result<String, String> {
    v.as_str().map(str::to_string).ok_or_else(|| format!("{PINS_FILE}: {what} must be a string"))
}

pub fn parse_pins(text: &str) -> Result<Pins, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("{PINS_FILE}: {e}"))?;
    if v["schema"] != PINS_SCHEMA {
        return Err(format!("{PINS_FILE}: schema must be {PINS_SCHEMA}"));
    }
    let r = &v["rust"];
    let rust = RustPin {
        version: s(&r["version"], "rust.version")?,
        commit: s(&r["commit"], "rust.commit")?,
        hosts: r["hosts"].as_object().ok_or("rust.hosts must be an object")?.iter().map(|(k, h)| Ok((k.clone(), s(h, "rust.hosts")?))).collect::<Result<_, String>>()?,
        targets: strs(&r["targets"], "rust.targets")?,
        components: strs(&r["components"], "rust.components")?,
        bin: strs(&r["bin"], "rust.bin")?,
    };
    let parts = v["engine"]["parts"].as_object().ok_or("engine.parts must be an object")?.iter().map(|(k, p)| Ok((k.clone(), strs(p, "engine.parts")?))).collect::<Result<Vec<_>, String>>()?;
    let prebuilt = v["prebuilt"].as_array().ok_or("prebuilt must be a list")?.iter().map(|p| Ok(Prebuilt { package: s(&p["package"], "prebuilt.package")?, bin: s(&p["bin"], "prebuilt.bin")?, ship_as: p["as"].as_str().map(str::to_string) })).collect::<Result<Vec<_>, String>>()?;
    let tools = v["tools"]
        .as_array()
        .ok_or("tools must be a list")?
        .iter()
        .map(|t| {
            Ok(ToolPin {
                name: s(&t["name"], "tools.name")?,
                version: s(&t["version"], "tools.version")?,
                licence: s(&t["licence"], "tools.licence")?,
                kind: s(&t["kind"], "tools.kind")?,
                platforms: t["platforms"].as_object().ok_or("tools.platforms must be an object")?.iter().map(|(k, p)| (k.clone(), p.clone())).collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let requirements = v["requirements"].as_object().ok_or("requirements must be an object")?.iter().map(|(k, r)| Ok((k.clone(), strs(r, "requirements")?))).collect::<Result<_, String>>()?;
    let mut runtime_allow = BTreeMap::new();
    for (platform, list) in v["runtime_allow"].as_object().into_iter().flatten() {
        let mut entries = Vec::new();
        for e in list.as_array().ok_or("runtime_allow must hold lists")? {
            let reason = s(&e["reason"], "runtime_allow.reason")?;
            if reason.trim().is_empty() {
                return Err(format!("{PINS_FILE}: runtime_allow needs a reason for every program"));
            }
            entries.push(RuntimeAllow { path: s(&e["path"], "runtime_allow.path")?, reason });
        }
        runtime_allow.insert(platform.clone(), entries);
    }
    let pins = Pins {
        pack_version: s(&v["pack_version"], "pack_version")?,
        live_protocol: s(&v["live_protocol"], "live_protocol")?,
        include: strs(&v["engine"]["include"], "engine.include")?,
        exclude: strs(&v["engine"]["exclude"], "engine.exclude")?,
        members: strs(&v["engine"]["workspace_members"], "engine.workspace_members")?,
        parts,
        rust,
        prebuilt,
        tools,
        requirements,
        runtime_allow,
    };
    for m in &pins.members {
        if !included(&format!("{m}/Cargo.toml"), &pins.include, &pins.exclude) {
            return Err(format!("{PINS_FILE}: workspace member {m} is not in engine.include"));
        }
    }
    Ok(pins)
}

// ---------------------------------------------------------------- selection

/// Is the repo path `p` (forward slashes) in the allow-list? An entry ending
/// in `/` takes a folder, anything else one file; `exclude` folders win.
pub fn included(p: &str, include: &[String], exclude: &[String]) -> bool {
    let hit = |e: &String| if e.ends_with('/') { p.starts_with(e.as_str()) } else { p == e };
    include.iter().any(hit) && !exclude.iter().any(hit)
}

/// The size-report part of a pack path (for engine files, the part whose
/// entry is the longest match).
pub fn part_of(path: &str, parts: &[(String, Vec<String>)]) -> String {
    let top = path.split('/').next().unwrap_or("");
    match top {
        "toolchain" | "vendor" => return top.to_string(),
        "tools" if path.starts_with("tools/bin/") => return "tools-bin".into(),
        "tools" => return "tools-convert".into(),
        "engine" => {}
        _ => return "pack-files".into(),
    }
    let rel = &path["engine/".len().min(path.len())..];
    // The longest matching entry wins (tools/evani-model/ over tools/).
    parts
        .iter()
        .flat_map(|(n, prefixes)| prefixes.iter().map(move |e| (n, e)))
        .filter(|(_, e)| if e.ends_with('/') { rel.starts_with(e.as_str()) } else { rel == e.as_str() })
        .max_by_key(|(_, e)| e.len())
        .map_or_else(|| "engine-other".to_string(), |(n, _)| n.clone())
}

/// The workspace Cargo.toml for the pack: `members` replaced by `members`,
/// and `[workspace.dependencies]` path entries outside the allow-list
/// dropped (our own games' kits). Everything else is kept as written.
pub fn trim_workspace_toml(text: &str, members: &[String], include: &[String], exclude: &[String]) -> Result<String, String> {
    let start = text.find("members = [").ok_or("Cargo.toml: no `members = [`")?;
    let end = start + text[start..].find(']').ok_or("Cargo.toml: `members` is not closed")?;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str("members = [\n");
    for m in members {
        out.push_str(&format!("    \"{m}\",\n"));
    }
    out.push(']');
    for line in text[end + 1..].split_inclusive('\n') {
        if let Some(p) = line.split("path = \"").nth(1).and_then(|r| r.split('"').next())
            && line.trim_start().starts_with(|c: char| c.is_ascii_alphabetic())
            && !included(&format!("{p}/Cargo.toml"), include, exclude)
        {
            continue;
        }
        out.push_str(line);
    }
    // The members list must parse back as written.
    let t: toml::Table = out.parse().map_err(|e| format!("trimmed Cargo.toml does not parse: {e}"))?;
    let got = t.get("workspace").and_then(|w| w.get("members")).and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>());
    if got.as_deref() != Some(members) {
        return Err("trimmed Cargo.toml: members did not round-trip".into());
    }
    Ok(out)
}

/// The engine's `.cargo/config.toml` for the pack: the `xtask` alias (which
/// would compile xtask into the read-only pack) is dropped, so `cargo xtask`
/// runs the prebuilt `cargo-xtask` on PATH instead. Everything else stays.
pub fn pack_cargo_config(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut section = "";
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.starts_with('[') {
            section = if t == "[alias]" { "alias" } else { "other" };
        }
        if section == "alias" && t.split(['=', ' ']).next() == Some("xtask") {
            continue;
        }
        // A comment directly above the alias line goes with it.
        if section == "alias" && t.starts_with('#') && lines.peek().is_some_and(|n| n.trim().split(['=', ' ']).next() == Some("xtask")) {
            continue;
        }
        out.push_str(line);
    }
    // An [alias] table left empty is dropped with its header.
    out.replace("[alias]\n\n", "").replace("[alias]\r\n\r\n", "")
}

/// Does the toolchain file `rel` (relative to the sysroot, `/`) go in the
/// pack? Only what builds a game: the listed programs and their libraries,
/// the host and `targets` std, rust-lld; never `.pdb`, docs, sources or
/// other targets.
pub fn toolchain_file_wanted(rel: &str, host: &str, targets: &[String], bins: &[String]) -> bool {
    if rel.ends_with(".pdb") || rel.contains("/rust-src/") || rel.starts_with("share/") || rel.starts_with("etc/") {
        return false;
    }
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        ["bin", file] => {
            let stem = file.strip_suffix(".exe").unwrap_or(file);
            bins.iter().any(|b| b == stem) || file.ends_with(".dll")
        }
        // Mac/Linux: librustc_driver and libstd dylibs sit in lib/.
        ["lib", file] => file.ends_with(".dylib") || file.ends_with(".so") || file.contains(".so."),
        ["lib", "rustlib", t, "lib", ..] => t == &host || targets.iter().any(|x| x == t),
        // rust-lld, gcc-ld/ and wasm-component-ld live in the host's bin.
        ["lib", "rustlib", t, "bin", ..] => t == &host,
        _ => false,
    }
}

// ---------------------------------------------------------------- engine root

/// Where the engine is for a prebuilt tool: `EVANI_ENGINE_ROOT` if it holds
/// a Cargo.toml, else `<pack>/engine` when the program runs from
/// `<pack>/tools/bin/`, else the folder it was compiled in (a checkout).
pub fn engine_root_from(env: Option<&std::ffi::OsStr>, exe: Option<&Path>, compiled: &Path) -> PathBuf {
    if let Some(e) = env.filter(|e| !e.is_empty()).map(PathBuf::from)
        && e.join("Cargo.toml").is_file()
    {
        return e;
    }
    if let Some(pack) = exe.and_then(Path::parent).filter(|b| b.ends_with(Path::new("tools").join("bin"))).and_then(Path::parent).and_then(Path::parent) {
        let engine = pack.join("engine");
        if engine.join("Cargo.toml").is_file() && pack.join("manifest.json").is_file() {
            return engine;
        }
    }
    compiled.to_path_buf()
}

// ---------------------------------------------------------------- hashing

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    /// gzip (level 6) size: a stand-in for the installer's compression.
    pub gz: u64,
}

struct Counter(u64);
impl Write for Counter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 += b.len() as u64;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_file(p: &Path, with_gz: bool) -> Result<(String, u64, u64), String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = Sha256::new();
    let mut gz = with_gz.then(|| GzEncoder::new(Counter(0), Compression::new(6)));
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf).map_err(|e| format!("{}: {e}", p.display()))?;
        if k == 0 {
            break;
        }
        n += k as u64;
        h.update(&buf[..k]);
        if let Some(g) = gz.as_mut() {
            g.write_all(&buf[..k]).map_err(|e| e.to_string())?;
        }
    }
    let gz_bytes = match gz {
        Some(g) => g.finish().map_err(|e| e.to_string())?.0,
        None => 0,
    };
    Ok((hex(&h.finalize()), n, gz_bytes))
}

/// Every file under `dir` as `/`-separated relative paths, sorted.
pub fn list_files(dir: &Path) -> Result<Vec<String>, String> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) -> Result<(), String> {
        for e in std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))? {
            let e = e.map_err(|e| e.to_string())?;
            let p = e.path();
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                walk(base, &p, out)?;
            } else if ft.is_file() {
                let rel = p.strip_prefix(base).map_err(|e| e.to_string())?;
                out.push(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Files at a pack's top level that the manifest doesn't list: the manifest
/// itself, its detached signature (route B, docs/ENGINE_PACK.md 4.8) and
/// Evani.App's live marker (`.evani-live`, written into the live pack).
pub const SIDE_FILES: &[&str] = &["manifest.json", "manifest.sig", ".evani-live"];

/// Hash every file under `dir` except the [`SIDE_FILES`], printing progress.
pub fn hash_tree(dir: &Path, with_gz: bool) -> Result<Vec<FileEntry>, String> {
    let files = list_files(dir)?;
    let mut out = Vec::with_capacity(files.len());
    let t = Instant::now();
    let mut last = Instant::now();
    for (i, f) in files.iter().enumerate() {
        if SIDE_FILES.contains(&f.as_str()) {
            continue;
        }
        let (sha256, bytes, gz) = hash_file(&dir.join(f), with_gz)?;
        out.push(FileEntry { path: f.clone(), sha256, bytes, gz });
        if last.elapsed() > Duration::from_secs(10) {
            println!("  hashed {}/{} files ({:.0} s)", i + 1, files.len(), t.elapsed().as_secs_f64());
            last = Instant::now();
        }
    }
    Ok(out)
}

/// Sizes per part, sorted by name.
pub fn part_sizes(files: &[FileEntry], parts: &[(String, Vec<String>)]) -> Vec<(String, u64, u64, u64)> {
    let mut m: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    for f in files {
        let e = m.entry(part_of(&f.path, parts)).or_default();
        e.0 += 1;
        e.1 += f.bytes;
        e.2 += f.gz;
    }
    m.into_iter().map(|(k, (n, b, g))| (k, n, b, g)).collect()
}

// ---------------------------------------------------------------- signing

/// The release machine's signing step (`--sign-cmd`, docs/ENGINE_PACK.md
/// 4.10): one external command run on every program in the pack before
/// anything is hashed, so the manifest holds the signed bytes.
#[derive(Clone, PartialEq)]
pub struct SignCmd {
    /// The command split into words; `{file}` in any word is the program's path.
    pub words: Vec<String>,
    /// Who signs (recorded in the manifest), e.g. "Microsoft Trusted Signing (CN=Snoaper)".
    pub signer: String,
}

impl std::fmt::Debug for SignCmd {
    // The command may carry settings; only the signer's name is ever shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignCmd {{ signer: {:?} }}", self.signer)
    }
}

/// Split a command line into words: spaces separate, double quotes group.
pub fn split_command(line: &str) -> Result<Vec<String>, String> {
    let (mut words, mut cur, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if quoted {
        return Err("--sign-cmd: a double quote is not closed".into());
    }
    if any {
        words.push(cur);
    }
    Ok(words)
}

impl SignCmd {
    pub fn parse(line: &str, signer: &str) -> Result<Self, String> {
        let words = split_command(line)?;
        if words.is_empty() {
            return Err("--sign-cmd is empty".into());
        }
        if !words.iter().any(|w| w.contains("{file}")) {
            return Err("--sign-cmd must name the program to sign as {file}, e.g. --sign-cmd \"node C:/path/sign.mjs {file}\"".into());
        }
        if signer.trim().is_empty() {
            return Err("--sign-cmd needs --signer <name> (recorded in the manifest as who signed each program)".into());
        }
        Ok(Self { words, signer: signer.trim().to_string() })
    }

    /// Sign one file: `{file}` replaced by its path, which is also in
    /// `EVANI_SIGN_FILE`. A non-zero exit refuses the pack.
    fn run(&self, file: &Path) -> Result<(), String> {
        let path = file.display().to_string();
        let w: Vec<String> = self.words.iter().map(|w| w.replace("{file}", &path)).collect();
        let o = Command::new(&w[0]).args(&w[1..]).env("EVANI_SIGN_FILE", file).stdin(Stdio::null()).output().map_err(|e| format!("signing {}: could not start `{}`: {e}", path, w[0]))?;
        if !o.status.success() {
            let tail: String = String::from_utf8_lossy(&o.stderr).chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(format!("signing {path} failed ({}): {}", o.status, tail.trim()));
        }
        Ok(())
    }
}

/// What the manifest says about signing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signing {
    /// Every program in the pack (pack-relative), in the order it is signed.
    pub programs: Vec<String>,
    /// Who signed them; `None` for an unsigned pack.
    pub signer: Option<String>,
    /// Programs the signing command left unchanged (already signed upstream).
    pub unchanged: Vec<String>,
}

fn macho(head: &[u8]) -> bool {
    head.len() >= 4 && matches!([head[0], head[1], head[2], head[3]], [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] | [0xfe, 0xed, 0xfa, 0xcf] | [0xfe, 0xed, 0xfa, 0xce] | [0xca, 0xfe, 0xba, 0xbe])
}

/// Is this pack file a program that release signing must sign? Only in
/// `toolchain/` and `tools/`: Windows `.exe`, `.dll` and `.node`; Mac
/// `.dylib`, `.node` and any Mach-O (by its first bytes).
pub fn is_program(rel: &str, platform: &str, head: &[u8]) -> bool {
    if !(rel.starts_with("toolchain/") || rel.starts_with("tools/")) {
        return false;
    }
    let l = rel.to_ascii_lowercase();
    if platform.starts_with("windows") {
        return l.ends_with(".exe") || l.ends_with(".dll") || l.ends_with(".node");
    }
    l.ends_with(".dylib") || l.ends_with(".node") || macho(head)
}

fn have_program(pack: &Path, rel: &str, platform: &str) -> bool {
    if !(rel.starts_with("toolchain/") || rel.starts_with("tools/")) {
        return false;
    }
    let mut head = [0u8; 4];
    let n = std::fs::File::open(pack.join(rel)).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    is_program(rel, platform, &head[..n])
}

/// The pack's programs, libraries first (Mac signs a dylib before what loads it).
pub fn find_programs(pack: &Path, platform: &str) -> Result<Vec<String>, String> {
    let mut libs = Vec::new();
    let mut exes = Vec::new();
    for rel in list_files(pack)? {
        if !(rel.starts_with("toolchain/") || rel.starts_with("tools/")) {
            continue;
        }
        if have_program(pack, &rel, platform) {
            let l = rel.to_ascii_lowercase();
            if l.ends_with(".dll") || l.ends_with(".dylib") || l.ends_with(".node") { libs.push(rel) } else { exes.push(rel) }
        }
    }
    libs.extend(exes);
    Ok(libs)
}

/// Run `cmd` on every program in `pack`, before hashing. Refuses when the
/// command fails, or when it changed no program at all (a signer that does
/// nothing, e.g. sign.mjs without the owner's settings, is not signing).
pub fn sign_programs(pack: &Path, platform: &str, cmd: &SignCmd) -> Result<Signing, String> {
    let programs = find_programs(pack, platform)?;
    let t = Instant::now();
    let mut last = Instant::now();
    let mut unchanged = Vec::new();
    for (i, rel) in programs.iter().enumerate() {
        let p = pack.join(rel);
        let before = hash_file(&p, false)?.0;
        cmd.run(&p)?;
        if hash_file(&p, false)?.0 == before {
            unchanged.push(rel.clone());
        }
        if last.elapsed() > Duration::from_secs(10) {
            println!("  signed {}/{} programs ({:.0} s)", i + 1, programs.len(), t.elapsed().as_secs_f64());
            last = Instant::now();
        }
    }
    if !programs.is_empty() && unchanged.len() == programs.len() {
        return Err(format!("signing: the command ran on all {} programs and changed none of them, so nothing was signed; fix: check the signing settings (Evani.App's sign.mjs does nothing until they are set) and pack again", programs.len()));
    }
    Ok(Signing { programs, signer: Some(cmd.signer.clone()), unchanged })
}

// ---------------------------------------------------------------- verify

#[derive(Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub checked: usize,
    pub changed: Vec<String>,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    /// In a signed pack: programs the manifest doesn't record as signed.
    pub unsigned: Vec<String>,
    /// Who signed the pack's programs (`None`: an unsigned pack).
    pub signer: Option<String>,
    /// The clean-PC check: programs that need the Visual C++ runtime.
    pub runtime: RuntimeReport,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.changed.is_empty() && self.missing.is_empty() && self.extra.is_empty() && self.unsigned.is_empty() && self.runtime.ok()
    }
}

/// Re-hash `pack` against its manifest: changed, missing and extra files.
pub fn verify(pack: &Path) -> Result<VerifyReport, String> {
    let text = std::fs::read_to_string(pack.join("manifest.json")).map_err(|e| format!("{}: {e}", pack.join("manifest.json").display()))?;
    let m: Value = serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
    if m["schema"] != SCHEMA {
        return Err(format!("manifest.json: schema is not {SCHEMA}"));
    }
    let want: BTreeMap<String, (String, u64)> = m["files"]
        .as_array()
        .ok_or("manifest.json: no files list")?
        .iter()
        .map(|f| Ok((s(&f["path"], "files.path")?, (s(&f["sha256"], "files.sha256")?, f["bytes"].as_u64().ok_or("files.bytes")?))))
        .collect::<Result<_, String>>()?;
    let have: BTreeSet<String> = list_files(pack)?.into_iter().filter(|f| !SIDE_FILES.contains(&f.as_str())).collect();
    let mut r = VerifyReport::default();
    for (p, (sha, bytes)) in &want {
        if !have.contains(p) {
            r.missing.push(p.clone());
            continue;
        }
        let (h, n, _) = hash_file(&pack.join(p), false)?;
        r.checked += 1;
        if &h != sha || n != *bytes {
            r.changed.push(p.clone());
        }
    }
    r.extra = have.into_iter().filter(|p| !want.contains_key(p)).collect();
    // The clean-PC check, with the allow-list the pack was built with.
    let allow: Vec<RuntimeAllow> = m["runtime_allow"].as_array().into_iter().flatten().filter_map(|a| Some(RuntimeAllow { path: a["path"].as_str()?.to_string(), reason: a["reason"].as_str()?.to_string() })).collect();
    r.runtime = check_runtime(pack, m["platform"].as_str().unwrap_or(""), &allow)?;
    // A signed pack: every program the manifest lists carries the signer.
    if m["signing"]["status"] == "signed" {
        r.signer = m["signing"]["signer"].as_str().map(str::to_string);
        let platform = m["platform"].as_str().unwrap_or("");
        for f in m["files"].as_array().into_iter().flatten() {
            let p = f["path"].as_str().unwrap_or("");
            let marked = f["program"] == true;
            let detected = !marked && have_program(pack, p, platform);
            if (marked || detected) && f["signed_by"].as_str().is_none() {
                r.unsigned.push(p.to_string());
            }
        }
    }
    Ok(r)
}

// ---------------------------------------------------------------- C runtime check

/// DLL names (lower case) of the Visual C++ runtime and the universal C
/// runtime. A fresh Windows has neither the Visual C++ one nor, before
/// Windows 10, the other; a program that imports one does not start there
/// (STATUS_DLL_NOT_FOUND, no output). Prefix match: `vcruntime140_1.dll`,
/// `msvcp140.dll`, `api-ms-win-crt-math-l1-1-0.dll`.
const DYNAMIC_RUNTIME: [&str; 7] = ["vcruntime", "msvcp", "msvcr", "concrt", "vcomp", "ucrtbase", "api-ms-win-crt-"];

pub fn is_dynamic_runtime(dll: &str) -> bool {
    let d = dll.to_ascii_lowercase();
    DYNAMIC_RUNTIME.iter().any(|p| d.starts_with(p))
}

/// The DLL names a Windows program (`.exe`, `.dll`, `.node`) imports,
/// including delay-load imports: `None` when `bytes` is not a PE file.
/// Hand-written (the import directory is a few table walks) so the check
/// adds no dependency; no PE crate is in Cargo.lock (DECISIONS.md).
pub fn pe_imports(bytes: &[u8]) -> Option<Vec<String>> {
    let u16_at = |o: usize| bytes.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(0x3c)?;
    if bytes.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let (sections, opt_size) = (u16_at(pe + 6)?, u16_at(pe + 20)?);
    let opt = pe + 24;
    // Data directories follow the fixed part of the optional header: 96
    // bytes in a PE32 file, 112 in a PE32+ one.
    let dirs = opt + match u16_at(opt)? {
        0x10b => 96,
        0x20b => 112,
        _ => return None,
    };
    let table: Vec<(usize, usize, usize)> = (0..sections)
        .map(|i| {
            let s = opt + opt_size + i * 40;
            Some((u32_at(s + 12)?, u32_at(s + 8)?.max(u32_at(s + 16)?), u32_at(s + 20)?))
        })
        .collect::<Option<_>>()?;
    let at = |rva: usize| table.iter().find(|(va, size, _)| rva >= *va && rva < va + size).map(|(va, _, raw)| rva - va + raw);
    let name = |rva: usize| {
        let o = at(rva)?;
        let n = bytes.get(o..)?.iter().position(|b| *b == 0)?;
        Some(String::from_utf8_lossy(&bytes[o..o + n]).to_ascii_lowercase())
    };
    let mut out = Vec::new();
    // Directory 1: imports (20-byte entries, name at +12). Directory 13:
    // delay-load imports (32-byte entries, name at +4).
    for (dir, entry, name_at) in [(1usize, 20usize, 12usize), (13, 32, 4)] {
        let rva = u32_at(dirs + dir * 8)?;
        if rva == 0 {
            continue;
        }
        let mut o = at(rva)?;
        while let Some(n) = u32_at(o + name_at).filter(|n| *n != 0) {
            out.push(name(n)?);
            o += entry;
        }
    }
    Some(out)
}

/// What [`check_runtime`] found. Every list is sorted by path.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RuntimeReport {
    /// Programs that import no C runtime DLL.
    pub statics: Vec<String>,
    /// Programs that do, on the allow-list: path, reason, the imports.
    pub allowed: Vec<(String, String, Vec<String>)>,
    /// Programs that do and are not allowed: path and the imports.
    pub violations: Vec<(String, Vec<String>)>,
    /// Allow-list entries that name a program that is missing or imports no
    /// C runtime (a stale entry: remove it).
    pub stale: Vec<String>,
}

impl RuntimeReport {
    pub fn ok(&self) -> bool {
        self.violations.is_empty()
    }

    /// One line per program for the console.
    pub fn lines(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.extend(self.statics.iter().map(|p| format!("static   {p}")));
        v.extend(self.allowed.iter().map(|(p, why, imp)| format!("allowed  {p}  (imports {}; {why})", imp.join(", "))));
        v.extend(self.violations.iter().map(|(p, imp)| format!("DYNAMIC  {p}  (imports {})", imp.join(", "))));
        v.extend(self.stale.iter().map(|p| format!("stale allow-list entry: {p}")));
        v
    }
}

/// The clean-PC check: read the import table of every `.exe`, `.dll` and
/// `.node` in `pack`. A program that imports the Visual C++ or universal C
/// runtime fails the check unless it is on `allow` (with a reason).
/// Windows packs only: a Mac pack has no such imports.
pub fn check_runtime(pack: &Path, platform: &str, allow: &[RuntimeAllow]) -> Result<RuntimeReport, String> {
    let mut r = RuntimeReport::default();
    if !platform.starts_with("windows") {
        return Ok(r);
    }
    let mut seen_dynamic = BTreeSet::new();
    for rel in list_files(pack)? {
        let l = rel.to_ascii_lowercase();
        if !(l.ends_with(".exe") || l.ends_with(".dll") || l.ends_with(".node")) {
            continue;
        }
        let bytes = std::fs::read(pack.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        let Some(imports) = pe_imports(&bytes) else { continue };
        let dynamic: Vec<String> = imports.into_iter().filter(|d| is_dynamic_runtime(d)).collect::<BTreeSet<_>>().into_iter().collect();
        if dynamic.is_empty() {
            r.statics.push(rel);
        } else if let Some(a) = allow.iter().find(|a| a.path == rel) {
            seen_dynamic.insert(rel.clone());
            r.allowed.push((rel, a.reason.clone(), dynamic));
        } else {
            r.violations.push((rel, dynamic));
        }
    }
    r.stale = allow.iter().filter(|a| !seen_dynamic.contains(&a.path)).map(|a| a.path.clone()).collect();
    Ok(r)
}

/// The check's verdict as an error that names every program and the fix.
fn runtime_error(r: &RuntimeReport) -> String {
    let list: Vec<String> = r.violations.iter().map(|(p, i)| format!("  {p} imports {}", i.join(", "))).collect();
    format!(
        "engine-pack: {} program(s) need the Visual C++ runtime, which a fresh Windows does not have (they would not start, with no output):\n{}\n  fix: build our own programs with a static C runtime (`-C target-feature=+crt-static`, set by the packer); for an upstream program, either fetch a build that has one or add it to `runtime_allow` in {PINS_FILE} with the reason it is safe",
        r.violations.len(),
        list.join("\n")
    )
}

// ---------------------------------------------------------------- git

fn run_out(cmd: &mut Command, what: &str) -> Result<Vec<u8>, String> {
    let o = cmd.stderr(Stdio::piped()).output().map_err(|e| format!("{what}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{what} failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(o.stdout)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    run_out(Command::new("git").arg("-C").arg(root).args(args), &format!("git {}", args.join(" "))).map(|o| String::from_utf8_lossy(&o).trim().to_string())
}

/// `git ls-tree -r -z` output: (mode, object id, path) of every blob.
pub fn parse_ls_tree(out: &[u8]) -> Vec<(String, String, String)> {
    out.split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .filter_map(|r| {
            let r = String::from_utf8_lossy(r);
            let (meta, path) = r.split_once('\t')?;
            let mut m = meta.split(' ');
            let (mode, ty, oid) = (m.next()?, m.next()?, m.next()?);
            (ty == "blob").then(|| (mode.to_string(), oid.to_string(), path.to_string()))
        })
        .collect()
}

/// Write the blobs `items` (oid, destination) with one `git cat-file --batch`.
fn write_blobs(root: &Path, items: &[(String, PathBuf)]) -> Result<(), String> {
    let mut child = Command::new("git").arg("-C").arg(root).args(["cat-file", "--batch"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(|e| format!("git cat-file: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
    let oids: Vec<String> = items.iter().map(|(o, _)| o.clone()).collect();
    let feeder = std::thread::spawn(move || -> std::io::Result<()> {
        for o in oids {
            writeln!(stdin, "{o}")?;
        }
        Ok(())
    });
    let mut out = BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);
    for (oid, dest) in items {
        let mut header = String::new();
        out.read_line(&mut header).map_err(|e| e.to_string())?;
        let size: usize = header.trim_end().rsplit(' ').next().and_then(|n| n.parse().ok()).ok_or_else(|| format!("git cat-file: bad header for {oid}: {header}"))?;
        let mut data = vec![0u8; size];
        out.read_exact(&mut data).map_err(|e| e.to_string())?;
        let mut nl = [0u8; 1];
        out.read_exact(&mut nl).map_err(|e| e.to_string())?;
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(dest, &data).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    feeder.join().map_err(|_| "git cat-file feeder panicked".to_string())?.map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- helpers

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))
}

fn copy_tree(from: &Path, to: &Path) -> Result<usize, String> {
    let files = list_files(from)?;
    for f in &files {
        copy_file(&from.join(f), &to.join(f))?;
    }
    Ok(files.len())
}

fn remove_dir(p: &Path) -> Result<(), String> {
    if p.exists() {
        std::fs::remove_dir_all(p).map_err(|e| format!("remove {}: {e}", p.display()))?;
    }
    Ok(())
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default()
}

fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(p),
    }
}

/// Do all the numeric pieces of `version` appear in `output`?
/// (`0.21.14` in "trunk 0.21.14"; `binaryen version_125` -> "125".)
pub fn version_matches(output: &str, version: &str) -> bool {
    let pieces: Vec<&str> = version.split([' ', '_']).filter(|p| p.chars().any(|c| c.is_ascii_digit())).collect();
    !pieces.is_empty() && pieces.iter().all(|p| output.contains(p))
}

/// The `wasm-bindgen` version in a Cargo.lock.
pub fn lock_version(lock: &str, name: &str) -> Option<String> {
    let mut it = lock.lines();
    while let Some(l) = it.next() {
        if l.trim() == format!("name = \"{name}\"") {
            return it.next().and_then(|v| v.trim().strip_prefix("version = \"")).map(|v| v.trim_end_matches('"').to_string());
        }
    }
    None
}

fn exe(name: &str, platform: &str) -> String {
    if platform.starts_with("windows") { format!("{name}.exe") } else { name.to_string() }
}

/// The platform this xtask runs on.
pub fn host_platform() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x64"
    } else {
        "other"
    }
}

fn set_mtime(p: &Path, t: SystemTime) -> Result<(), String> {
    let f = std::fs::File::options().write(true).open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    f.set_modified(t).map_err(|e| format!("{}: {e}", p.display()))
}

struct Log {
    start: Instant,
}
impl Log {
    fn step(&self, what: &str) {
        println!("[engine-pack {:>5.0} s] {what}", self.start.elapsed().as_secs_f64());
        let _ = std::io::stdout().flush();
    }
}

// ---------------------------------------------------------------- options

#[derive(Clone, Debug)]
pub struct Opts {
    pub platform: String,
    pub out: PathBuf,
    pub build_dir: PathBuf,
    pub commit: String,
    pub tools_from: PathBuf,
    pub skip_prebuilt: bool,
    pub allow_incomplete: bool,
    pub force: bool,
    /// Fetch the upstream tools (`--download`) into `cache`.
    pub download: bool,
    pub cache: PathBuf,
    /// Also write the hostable archive and its descriptor (`--archive`).
    pub archive: bool,
    /// Sign every program before hashing (`--sign-cmd` + `--signer`, or
    /// `EVANI_PACK_SIGN_CMD` + `EVANI_PACK_SIGNER`).
    pub sign: Option<SignCmd>,
}

fn parse_opts(root: &Path, args: &[String]) -> Result<(Opts, bool, Option<PathBuf>), String> {
    let mut o = Opts {
        platform: host_platform().to_string(),
        out: crate_target_dir(root).join("engine-pack"),
        build_dir: PathBuf::new(),
        commit: "HEAD".into(),
        tools_from: root.join(".evani").join("bin"),
        skip_prebuilt: false,
        allow_incomplete: false,
        force: false,
        download: false,
        cache: PathBuf::new(),
        archive: false,
        sign: None,
    };
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (mut sign_cmd, mut signer) = (env("EVANI_PACK_SIGN_CMD"), env("EVANI_PACK_SIGNER"));
    let mut repro = false;
    let mut verify_dir = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |n: &str| it.next().cloned().ok_or_else(|| format!("{n} needs a value\nusage:\n{}", usage()));
        match a.as_str() {
            "--platform" => o.platform = val("--platform")?,
            "--out" => o.out = PathBuf::from(val("--out")?),
            "--build-dir" => o.build_dir = PathBuf::from(val("--build-dir")?),
            "--commit" => o.commit = val("--commit")?,
            "--tools-from" => o.tools_from = PathBuf::from(val("--tools-from")?),
            "--verify" => verify_dir = Some(PathBuf::from(val("--verify")?)),
            "--skip-prebuilt" => o.skip_prebuilt = true,
            "--allow-incomplete" => o.allow_incomplete = true,
            "--force" => o.force = true,
            "--download" => o.download = true,
            "--cache" => o.cache = PathBuf::from(val("--cache")?),
            "--archive" => o.archive = true,
            "--sign-cmd" => sign_cmd = Some(val("--sign-cmd")?),
            "--signer" => signer = Some(val("--signer")?),
            "--repro-check" => repro = true,
            other => return Err(format!("engine-pack: unknown option `{other}`\nusage:\n{}", usage())),
        }
    }
    if o.build_dir.as_os_str().is_empty() {
        o.build_dir = o.out.join(".build");
    }
    if o.cache.as_os_str().is_empty() {
        o.cache = o.out.join(".upstream");
    }
    match (sign_cmd, signer) {
        (Some(c), s) => o.sign = Some(SignCmd::parse(&c, s.as_deref().unwrap_or(""))?),
        (None, Some(_)) => return Err("--signer needs --sign-cmd <command with {file}>".into()),
        (None, None) => {}
    }
    Ok((o, repro, verify_dir))
}

fn crate_target_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()) {
        Some(t) => root.join(t),
        None => root.join("target"),
    }
}

// ---------------------------------------------------------------- command

pub fn cmd_engine_pack(root: &Path, args: &[String]) -> Result<(), String> {
    let (opts, repro, verify_dir) = parse_opts(root, args)?;
    if let Some(dir) = verify_dir {
        let t = Instant::now();
        let r = verify(&dir)?;
        println!("engine-pack --verify {}: {} files checked in {:.0} s", dir.display(), r.checked, t.elapsed().as_secs_f64());
        match &r.signer {
            Some(s) => println!("  programs signed by: {s}"),
            None => println!("  programs: unsigned"),
        }
        println!("  C runtime: {} programs static, {} allow-listed, {} need the Visual C++ runtime", r.runtime.statics.len(), r.runtime.allowed.len(), r.runtime.violations.len());
        for l in r.runtime.lines() {
            println!("    {l}");
        }
        for (what, list) in [("changed", &r.changed), ("missing", &r.missing), ("extra", &r.extra), ("not recorded as signed", &r.unsigned)] {
            for p in list.iter().take(20) {
                println!("  {what}: {p}");
            }
            if list.len() > 20 {
                println!("  ... {} more {what}", list.len() - 20);
            }
        }
        return if r.ok() { Ok(()) } else { Err("engine-pack --verify: the pack does not match its manifest".into()) };
    }
    let pins = parse_pins(&std::fs::read_to_string(root.join(PINS_FILE)).map_err(|e| format!("{PINS_FILE}: {e}"))?)?;
    if !repro {
        let dir = pack(root, &pins, &opts)?;
        println!("engine pack: {}", dir.display());
        return Ok(());
    }
    // Twice, from a wiped build folder each time, into two folders, from
    // one commit resolved once (a commit made between the runs must not
    // count as a difference).
    let commit = git(root, &["rev-parse", &format!("{}^{{commit}}", opts.commit)])?;
    println!("reproducibility check of commit {commit}");
    let mut manifests = Vec::new();
    for run in ["a", "b"] {
        println!("== reproducibility run {run} ==");
        remove_dir(&opts.build_dir)?;
        let o = Opts { out: opts.out.join(format!("repro-{run}")), force: true, commit: commit.clone(), ..opts.clone() };
        let dir = pack(root, &pins, &o)?;
        manifests.push((dir.clone(), std::fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?));
    }
    let [(a_dir, a), (b_dir, b)] = [manifests[0].clone(), manifests[1].clone()];
    if a == b {
        println!("reproducible: {} and {} have byte-identical manifests ({} bytes)", a_dir.display(), b_dir.display(), a.len());
        return Ok(());
    }
    let files = |m: &[u8]| -> BTreeMap<String, String> {
        serde_json::from_slice::<Value>(m).ok().and_then(|v| v["files"].as_array().cloned()).unwrap_or_default().iter().map(|f| (f["path"].as_str().unwrap_or("").to_string(), f["sha256"].as_str().unwrap_or("").to_string())).collect()
    };
    let (fa, fb) = (files(&a), files(&b));
    let keys: BTreeSet<&String> = fa.keys().chain(fb.keys()).collect();
    let diff: Vec<&&String> = keys.iter().filter(|k| fa.get(**k) != fb.get(**k)).collect();
    for k in diff.iter().take(30) {
        println!("  differs: {k}");
    }
    Err(format!("not reproducible: {} files differ between {} and {}", diff.len(), a_dir.display(), b_dir.display()))
}

/// The outcome of one part that may be missing on this machine.
#[derive(Default)]
struct Gaps(Vec<String>);

/// Build one pack. Returns its folder.
pub fn pack(root: &Path, pins: &Pins, o: &Opts) -> Result<PathBuf, String> {
    let log = Log { start: Instant::now() };
    let host = pins.rust.hosts.get(&o.platform).ok_or_else(|| format!("engine-pack: no rust host pinned for platform {}", o.platform))?.clone();
    let native = o.platform == host_platform();
    let commit = git(root, &["rev-parse", &format!("{}^{{commit}}", o.commit)])?;
    let created = git(root, &["show", "-s", "--format=%cI", &commit])?;
    let ctime: u64 = git(root, &["show", "-s", "--format=%ct", &commit])?.parse().map_err(|e| format!("commit time: {e}"))?;
    let mtime = UNIX_EPOCH + Duration::from_secs(ctime);
    let dest = o.out.join("evani-engine").join(&pins.pack_version).join(&o.platform);
    if dest.exists() && !o.force {
        return Err(format!("{} exists (use --force to replace it)", dest.display()));
    }
    let stage = o.out.join(format!(".staging-{}-{}", pins.pack_version, o.platform));
    remove_dir(&stage)?;
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let mut gaps = Gaps::default();
    log.step(&format!("pack {} for {} from {} ({})", pins.pack_version, o.platform, &commit[..12.min(commit.len())], created));

    // 1. engine/ from git blobs.
    let listing = run_out(Command::new("git").arg("-C").arg(root).args(["ls-tree", "-r", "-z", &commit]), "git ls-tree")?;
    let items: Vec<(String, PathBuf)> = parse_ls_tree(&listing).into_iter().filter(|(mode, _, p)| mode != "120000" && included(p, &pins.include, &pins.exclude)).map(|(_, oid, p)| (oid, stage.join("engine").join(&p))).collect();
    write_blobs(root, &items)?;
    let cargo_toml = stage.join("engine").join("Cargo.toml");
    let trimmed = trim_workspace_toml(&std::fs::read_to_string(&cargo_toml).map_err(|e| e.to_string())?, &pins.members, &pins.include, &pins.exclude)?;
    std::fs::write(&cargo_toml, trimmed).map_err(|e| e.to_string())?;
    let cfg = stage.join("engine").join(".cargo").join("config.toml");
    if cfg.is_file() {
        let text = std::fs::read_to_string(&cfg).map_err(|e| e.to_string())?;
        std::fs::write(&cfg, pack_cargo_config(&text)).map_err(|e| e.to_string())?;
    }
    log.step(&format!("engine: {} files from git", items.len()));

    // 2. Cargo.lock for the trimmed workspace, then vendor/ (both offline).
    let rust = active_rust(&pins.rust)?;
    let cargo = rust.sysroot.join("bin").join(exe("cargo", host_platform()));
    let rustc = rust.sysroot.join("bin").join(exe("rustc", host_platform()));
    let quiet_target = o.build_dir.join("meta-target");
    run_out(
        Command::new(&cargo).args(["metadata", "--offline", "--format-version", "1", "--manifest-path"]).arg(&cargo_toml).env("RUSTC", &rustc).env("CARGO_TARGET_DIR", &quiet_target).stdout(Stdio::null()),
        "cargo metadata (trim Cargo.lock to the pack's workspace)",
    )?;
    // cargo vendor's progress goes to the console (a long silent step looks hung).
    let st = Command::new(&cargo).args(["vendor", "--offline", "--locked", "--versioned-dirs", "--manifest-path"]).arg(&cargo_toml).arg(stage.join("vendor")).env("RUSTC", &rustc).stdout(Stdio::null()).status().map_err(|e| format!("cargo vendor: {e}"))?;
    if !st.success() {
        return Err(format!("cargo vendor failed ({st}); fix: build the engine once on this machine so every crate is in the local cache, then pack again"));
    }
    std::fs::write(stage.join("cargo-config.toml"), vendor_config()).map_err(|e| e.to_string())?;
    let lock = std::fs::read_to_string(stage.join("engine").join("Cargo.lock")).map_err(|e| e.to_string())?;
    log.step(&format!("vendor: {} crates", std::fs::read_dir(stage.join("vendor")).map_err(|e| e.to_string())?.count()));

    // 3. toolchain/.
    if native {
        let n = copy_toolchain(&rust.sysroot, &stage.join("toolchain"), &host, &pins.rust)?;
        log.step(&format!("toolchain: {n} files from {}", rust.sysroot.display()));
    } else {
        gaps.0.push(format!("toolchain: a {} toolchain can only be packed on that platform", o.platform));
    }

    // 4. tools/bin: our prebuilt tools, built from the pack's own source.
    let mut prebuilt = Vec::new();
    if !native {
        gaps.0.push(format!("prebuilt tools: build on {}", o.platform));
    } else if o.skip_prebuilt {
        gaps.0.push("prebuilt tools: skipped (--skip-prebuilt)".into());
    } else {
        prebuilt = build_prebuilt(&stage, pins, o, &cargo, &rustc, &log)?;
    }

    // 5. third-party tools.
    let wb_crate = lock_version(&lock, "wasm-bindgen").ok_or("Cargo.lock has no wasm-bindgen")?;
    let mut tools_json = Vec::new();
    for t in pins.tools.iter().filter(|t| t.kind != "external") {
        if t.name == "wasm-bindgen" && t.version != wb_crate {
            return Err(format!("{PINS_FILE}: wasm-bindgen CLI {} does not equal the wasm-bindgen crate {wb_crate} in Cargo.lock (they must match exactly)", t.version));
        }
        let placed = if o.download && t.kind == "bin" { download_tool(t, &o.platform, &stage, &o.cache, &log)? } else { place_tool(t, &o.platform, &stage, &o.tools_from)? };
        match placed {
            Ok(j) => tools_json.push(j),
            Err(gap) => {
                gaps.0.push(gap.clone());
                tools_json.push(json!({ "name": t.name, "version": t.version, "licence": t.licence, "kind": t.kind, "missing": gap }));
            }
        }
    }
    log.step(&format!("tools: {} placed, {} missing", tools_json.iter().filter(|j| j.get("missing").is_none()).count(), tools_json.iter().filter(|j| j.get("missing").is_some()).count()));

    // 6. LICENSE and notices.
    copy_file(&stage.join("engine").join("LICENSE"), &stage.join("LICENSE"))?;
    let notices = notices(&stage, &tools_json, &pins.rust)?;
    std::fs::write(stage.join("THIRD_PARTY_NOTICES.md"), notices).map_err(|e| e.to_string())?;

    // 7. Signing (release machines): every program, before anything is
    // hashed, so the manifest records the signed bytes.
    let signing = match &o.sign {
        Some(cmd) => {
            log.step(&format!("signing every program (signer: {})", cmd.signer));
            let s = sign_programs(&stage, &o.platform, cmd)?;
            log.step(&format!("signed {} programs ({} already signed upstream, left unchanged)", s.programs.len(), s.unchanged.len()));
            s
        }
        None => Signing { programs: find_programs(&stage, &o.platform)?, ..Signing::default() },
    };

    // 7b. The clean-PC check: nothing may need the Visual C++ runtime unless
    // the pins allow it (and say why). Before hashing, so a failure writes nothing.
    let runtime = check_runtime(&stage, &o.platform, pins.runtime_allow.get(&o.platform).map(Vec::as_slice).unwrap_or(&[]))?;
    log.step(&format!("C runtime check: {} programs static, {} allow-listed, {} dynamic", runtime.statics.len(), runtime.allowed.len(), runtime.violations.len()));
    for l in runtime.lines() {
        println!("    {l}");
    }
    if !runtime.ok() {
        return Err(runtime_error(&runtime));
    }

    // 8. Fixed times, hashes, manifest.
    for f in list_files(&stage)? {
        set_mtime(&stage.join(&f), mtime)?;
    }
    log.step("hashing every file (sha256 + gzip size)");
    let files = hash_tree(&stage, true)?;
    if !o.allow_incomplete && gaps.0.iter().any(|g| !g.contains("--skip-prebuilt")) {
        return Err(format!("engine-pack: the pack would be incomplete (use --allow-incomplete to write it anyway):\n  {}", gaps.0.join("\n  ")));
    }
    let manifest = manifest_json(&ManifestIn { pins, platform: &o.platform, host: &host, commit: &commit, created: &created, rust_commit: &rust.commit, wb_crate: &wb_crate, tools: &tools_json, prebuilt: &prebuilt, gaps: &gaps.0, files: &files, signing: &signing });
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())? + "\n";
    std::fs::write(stage.join("manifest.json"), text).map_err(|e| e.to_string())?;
    set_mtime(&stage.join("manifest.json"), mtime)?;
    remove_dir(&dest)?;
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&stage, &dest).map_err(|e| format!("rename {} -> {}: {e}", stage.display(), dest.display()))?;
    print_sizes(&files, &pins.parts);
    if !gaps.0.is_empty() {
        println!("INCOMPLETE pack (not for release):\n  {}", gaps.0.join("\n  "));
    }
    if o.archive {
        write_hosted(&dest, &o.out, &pins.pack_version, &o.platform, mtime, &log)?;
    }
    log.step("done");
    Ok(dest)
}

fn print_sizes(files: &[FileEntry], parts: &[(String, Vec<String>)]) {
    let mb = |b: u64| b as f64 / 1_048_576.0;
    println!("{:<28} {:>7} {:>11} {:>11}", "part", "files", "raw MB", "gzip MB");
    let (mut n, mut b, mut g) = (0, 0, 0);
    for (name, pn, pb, pg) in part_sizes(files, parts) {
        println!("{name:<28} {pn:>7} {:>11.2} {:>11.2}", mb(pb), mb(pg));
        n += pn;
        b += pb;
        g += pg;
    }
    println!("{:<28} {n:>7} {:>11.2} {:>11.2}", "TOTAL", mb(b), mb(g));
}

pub struct ManifestIn<'a> {
    pub pins: &'a Pins,
    pub platform: &'a str,
    pub host: &'a str,
    pub commit: &'a str,
    pub created: &'a str,
    pub rust_commit: &'a str,
    pub wb_crate: &'a str,
    pub tools: &'a [Value],
    pub prebuilt: &'a [Value],
    pub gaps: &'a [String],
    pub files: &'a [FileEntry],
    pub signing: &'a Signing,
}

/// The manifest (schema [`SCHEMA`]). Deterministic: no clock, no absolute paths.
pub fn manifest_json(m: &ManifestIn) -> Value {
    let parts: Vec<Value> = part_sizes(m.files, &m.pins.parts).into_iter().map(|(name, files, bytes, gz)| json!({ "name": name, "files": files, "bytes": bytes, "bytes_compressed": gz })).collect();
    let total: (u64, u64) = m.files.iter().fold((0, 0), |a, f| (a.0 + f.bytes, a.1 + f.gz));
    let wb_cli = m.tools.iter().find(|t| t["name"] == "wasm-bindgen").and_then(|t| t["version"].as_str()).unwrap_or("");
    let win = m.platform.starts_with("windows");
    let programs: BTreeSet<&str> = m.signing.programs.iter().map(String::as_str).collect();
    let signing = match &m.signing.signer {
        // Evani.App signs every listed program (docs/ENGINE_PACK.md 4.10).
        None => json!({ "status": "unsigned", "programs": programs.len() }),
        Some(s) => json!({ "status": "signed", "signer": s, "programs": programs.len(), "unchanged": m.signing.unchanged }),
    };
    let file_json = |f: &FileEntry| {
        let mut v = json!({ "path": f.path, "sha256": f.sha256, "bytes": f.bytes });
        if programs.contains(f.path.as_str()) {
            v["program"] = json!(true);
            if let Some(s) = &m.signing.signer {
                v["signed_by"] = json!(s);
            }
        }
        v
    };
    json!({
        "schema": SCHEMA,
        "pack_version": m.pins.pack_version,
        "engine_commit": m.commit,
        "created": m.created,
        "platform": m.platform,
        "live_protocol": m.pins.live_protocol,
        // Complete: nothing missing. Release-ready: also every bundled
        // program checked against its upstream download's pinned SHA-256.
        "complete": m.gaps.is_empty(),
        "release_ready": m.gaps.is_empty() && m.tools.iter().all(|t| t["upstream_verified"] == true),
        "gaps": m.gaps,
        "rust": { "version": m.pins.rust.version, "commit": m.rust_commit, "host": m.host, "targets": m.pins.rust.targets, "components": m.pins.rust.components },
        "wasm_bindgen": { "crate_version": m.wb_crate, "cli_version": wb_cli },
        "tools": m.tools,
        // Pinned, never in the pack: installed per user on request (4.11).
        "external_tools": m.pins.tools.iter().filter(|t| t.kind == "external").map(|t| {
            let p = t.platforms.get(m.platform);
            json!({ "name": t.name, "version": t.version, "licence": t.licence, "url": p.map(|p| p["url"].clone()), "sha256_upstream": p.map(|p| p["sha256_upstream"].clone()), "install": p.map(|p| p["install"].clone()), "bundled": false })
        }).collect::<Vec<_>>(),
        "prebuilt": m.prebuilt,
        "requirements": m.pins.requirements.get(m.platform).cloned().unwrap_or_default(),
        // Programs allowed to import the Visual C++ runtime, with the reason;
        // `--verify` runs the import check with this list (section 3 of the pins).
        "runtime_allow": m.pins.runtime_allow.get(m.platform).map(|l| l.iter().map(|a| json!({ "path": a.path, "reason": a.reason })).collect::<Vec<_>>()).unwrap_or_default(),
        "layout": {
            "engine": "engine",
            "toolchain_bin": "toolchain/bin",
            "rustc": if win { "toolchain/bin/rustc.exe" } else { "toolchain/bin/rustc" },
            "cargo": if win { "toolchain/bin/cargo.exe" } else { "toolchain/bin/cargo" },
            "tools_bin": "tools/bin",
            "tools_convert": "tools/convert",
            "vendor": "vendor",
            "cargo_config_template": "cargo-config.toml",
            "requirements_checker": if win { "tools/bin/evani-reqcheck.exe" } else { "tools/bin/evani-reqcheck" },
            "dev_harness_marker": "engine/.vibedeck/dev-harness/live.mjs"
        },
        "parts": parts,
        "totals": { "files": m.files.len(), "bytes": total.0, "bytes_compressed": total.1 },
        "signing": signing,
        "files": m.files.iter().map(file_json).collect::<Vec<_>>(),
    })
}

/// The source-replacement config Evani.App writes into the per-user
/// `CARGO_HOME/config.toml`, with `{PACK}` replaced by the pack folder.
pub fn vendor_config() -> &'static str {
    "# Evani engine pack: build offline from the pack's vendored crates.\n# Evani.App writes this into CARGO_HOME/config.toml with {PACK} replaced by\n# the pack folder (forward slashes).\n[source.crates-io]\nreplace-with = \"evani-pack-vendor\"\n\n[source.evani-pack-vendor]\ndirectory = \"{PACK}/vendor\"\n\n[net]\noffline = true\n"
}

// ---------------------------------------------------------------- toolchain

struct ActiveRust {
    sysroot: PathBuf,
    commit: String,
}

/// The active toolchain, refused unless it is exactly the pinned one.
fn active_rust(pin: &RustPin) -> Result<ActiveRust, String> {
    let vv = String::from_utf8_lossy(&run_out(Command::new("rustc").arg("-Vv"), "rustc -Vv")?).to_string();
    let field = |k: &str| vv.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string())).unwrap_or_default();
    let (release, commit) = (field("release:"), field("commit-hash:"));
    if release != pin.version || commit != pin.commit {
        return Err(format!("engine-pack: the active Rust is {release} ({commit}), not the pinned {} ({}); a toolchain change is a new pack_version (update {PINS_FILE})", pin.version, pin.commit));
    }
    let sysroot = PathBuf::from(String::from_utf8_lossy(&run_out(Command::new("rustc").args(["--print", "sysroot"]), "rustc --print sysroot")?).trim());
    Ok(ActiveRust { sysroot, commit })
}

fn copy_toolchain(sysroot: &Path, to: &Path, host: &str, pin: &RustPin) -> Result<usize, String> {
    let mut n = 0;
    for f in list_files(sysroot)? {
        if toolchain_file_wanted(&f, host, &pin.targets, &pin.bin) {
            copy_file(&sysroot.join(&f), &to.join(&f))?;
            n += 1;
        }
    }
    for b in &pin.bin {
        if !to.join("bin").join(exe(b, host_platform())).is_file() {
            return Err(format!("toolchain: {b} is pinned but not in {} (fix: `rustup component add` it)", sysroot.join("bin").display()));
        }
    }
    for t in &pin.targets {
        if !to.join("lib").join("rustlib").join(t).join("lib").is_dir() {
            return Err(format!("toolchain: target {t} is pinned but not installed (fix: `rustup target add {t}`)"));
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------- prebuilt

/// Build our tools from the pack's own engine source and vendor folder in a
/// fixed folder (same absolute paths every time, so two builds can match).
fn build_prebuilt(stage: &Path, pins: &Pins, o: &Opts, cargo: &Path, rustc: &Path, log: &Log) -> Result<Vec<Value>, String> {
    let b = std::path::absolute(&o.build_dir).map_err(|e| e.to_string())?;
    let (src, vendor, home, target) = (b.join("src"), b.join("vendor"), b.join("cargo-home"), b.join("target"));
    remove_dir(&src)?;
    remove_dir(&vendor)?;
    copy_tree(&stage.join("engine"), &src)?;
    copy_tree(&stage.join("vendor"), &vendor)?;
    let fwd = |p: &Path| p.display().to_string().replace('\\', "/");
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    std::fs::write(home.join("config.toml"), vendor_config().replace("{PACK}", &fwd(&b))).map_err(|e| e.to_string())?;
    // Path independence: every folder the build can see is mapped to a fixed
    // name, so two builds in different folders (or on different PCs) give the
    // same bytes. The workspace, CARGO_HOME, the target dir and the vendor
    // folder all sit inside the build folder; the Rust toolchain folder is
    // mapped too (it holds the user name). Both slash styles are mapped, since
    // Cargo and rustc print paths either way. Only this build gets these.
    let sysroot = rustc.parent().and_then(Path::parent).map(Path::to_path_buf);
    let mut flags: Vec<String> = Vec::new();
    let mut remap = |from: String, to: &str| {
        let f = format!("--remap-path-prefix={from}={to}");
        if !flags.contains(&f) {
            flags.push(f);
        }
    };
    for p in [Some(b.clone()), sysroot.clone()].into_iter().flatten() {
        let to = if p == b { "/evani-pack" } else { "/evani-rust" };
        remap(p.display().to_string(), to);
        remap(fwd(&p), to);
    }
    if o.platform.starts_with("windows") {
        flags.push("-Clink-arg=/Brepro".into());
        // A static C runtime: our programs must start on a PC that has no
        // Visual C++ runtime (they run before the Build Tools are installed).
        // Only this build gets it, through CARGO_ENCODED_RUSTFLAGS here; normal game and dev
        // builds are unchanged. `check_runtime` fails the pack if it is missing.
        flags.push("-Ctarget-feature=+crt-static".into());
    }
    // Encoded flags (a unit-separator list), so a folder with a space is fine.
    let encoded = flags.join("\u{1f}");
    log.step(&format!("building {} prebuilt tools (release, --locked --offline) in {}", pins.prebuilt.len(), b.display()));
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "--release", "--locked", "--offline", "--manifest-path"]).arg(src.join("Cargo.toml"));
    for p in &pins.prebuilt {
        cmd.args(["-p", &p.package]);
    }
    let status = cmd
        .env("CARGO_HOME", &home)
        .env("CARGO_TARGET_DIR", &target)
        .env("RUSTC", rustc)
        .env("CARGO_ENCODED_RUSTFLAGS", encoded)
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env("CARGO_INCREMENTAL", "0")
        .env("EVANI_PACK_BUILD", "1")
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("building the prebuilt tools failed ({status})"));
    }
    let mut out = Vec::new();
    for p in &pins.prebuilt {
        let name = exe(&p.bin, &o.platform);
        let shipped = exe(p.ship_as.as_deref().unwrap_or(&p.bin), &o.platform);
        let to = stage.join("tools").join("bin").join(&shipped);
        copy_file(&target.join("release").join(&name), &to)?;
        let (sha, bytes, _) = hash_file(&to, false)?;
        out.push(json!({ "package": p.package, "path": format!("tools/bin/{shipped}"), "sha256": sha, "bytes": bytes }));
    }
    log.step("prebuilt tools built");
    Ok(out)
}

// ---------------------------------------------------------------- tools

/// Place one third-party tool. Outer error: a real failure (a checksum that
/// doesn't match its record). Inner error: the tool is not on this machine
/// (a gap in the pack).
fn place_tool(t: &ToolPin, platform: &str, stage: &Path, tools_from: &Path) -> Result<Result<Value, String>, String> {
    let Some(p) = t.platforms.get(platform) else {
        return Ok(Err(format!("{} {}: no pin for {platform}", t.name, t.version)));
    };
    let base = json!({ "name": t.name, "version": t.version, "licence": t.licence, "kind": t.kind, "url": p["url"], "sha256_upstream": p["sha256_upstream"] });
    if t.kind == "bin" {
        let file = p["file"].as_str().ok_or_else(|| format!("{PINS_FILE}: {} file", t.name))?;
        let local = p["local"].as_array().cloned().unwrap_or_default();
        let Some(src) = local.iter().filter_map(Value::as_str).map(expand).find(|c| c.is_file()) else {
            return Ok(Err(format!("{} {}: not found on this machine (looked at {}); the release machine downloads {} and checks sha256_upstream", t.name, t.version, local.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "), p["url"].as_str().unwrap_or("?"))));
        };
        if platform == host_platform() {
            let v = Command::new(&src).arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)).unwrap_or_default();
            if !version_matches(&v, &t.version) {
                return Ok(Err(format!("{}: {} reports `{}`, not the pinned {}", t.name, src.display(), v.trim(), t.version)));
            }
        }
        let to = stage.join("tools").join("bin").join(file);
        copy_file(&src, &to)?;
        let (sha, bytes, _) = hash_file(&to, false)?;
        let mut j = base;
        j["path"] = json!(format!("tools/bin/{file}"));
        j["sha256"] = json!(sha);
        j["bytes"] = json!(bytes);
        // A local build can't be checked against the upstream archive.
        j["source"] = json!("local");
        j["upstream_verified"] = json!(false);
        return Ok(Ok(j));
    }
    // `convert`: a folder from the doctor --fix install, checked against its record.
    let dir = p["dir"].as_str().ok_or_else(|| format!("{PINS_FILE}: {} dir", t.name))?;
    let record = p["record"].as_str().ok_or_else(|| format!("{PINS_FILE}: {} record", t.name))?;
    let rec: Value = match std::fs::read_to_string(tools_from.join(record)) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{}: {e}", tools_from.join(record).display()))?,
        Err(_) => return Ok(Err(format!("{} {}: no {} in {} (run `cargo xtask doctor --fix` there, or pass --tools-from)", t.name, t.version, record, tools_from.display()))),
    };
    let r = &rec[&t.name];
    if r["version"].as_str() != Some(t.version.as_str()) {
        return Ok(Err(format!("{}: {} records version {}, not the pinned {}", t.name, tools_from.join(record).display(), r["version"], t.version)));
    }
    // The install came from exactly the pinned download (doctor --fix checked
    // it against its own pin; this keeps that pin and the pack's equal).
    let pinned: Vec<String> = match (p["sha256_upstream"].as_str(), p["lockfile"].as_str()) {
        (Some(sha), _) => vec![sha.to_string()],
        (None, Some(lock)) => {
            let path = stage.join("engine").join(lock);
            lockfile_hashes(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        (None, None) => Vec::new(),
    };
    let got = r["download_sha256"].as_str().unwrap_or("");
    if !pinned.is_empty() && !pinned.iter().any(|w| w == got) {
        return Err(format!("{}: {} records download {got} but {PINS_FILE} pins {}; reinstall it with `cargo xtask doctor --fix`, or fix the pin", t.name, tools_from.join(record).display(), pinned.join(" or ")));
    }
    let exe_rel = r["exe"].as_str().ok_or_else(|| format!("{record}: {} has no exe", t.name))?;
    let (sha, _, _) = hash_file(&tools_from.join(exe_rel), false)?;
    if Some(sha.as_str()) != r["exe_sha256"].as_str() {
        return Err(format!("{}: {} does not match the checksum in {record}; reinstall it with `cargo xtask doctor --fix`", t.name, tools_from.join(exe_rel).display()));
    }
    let copy = p["copy"].as_array().cloned().unwrap_or_default();
    let to = stage.join("tools").join("convert").join(dir);
    let mut n = 0;
    for c in copy.iter().filter_map(Value::as_str) {
        if c == "*" {
            n += copy_tree(&tools_from.join(dir), &to)?;
        } else {
            copy_file(&tools_from.join(dir).join(c), &to.join(c))?;
            n += 1;
        }
    }
    // The record the converter reads, holding only this tool's entry.
    let rec_path = stage.join("tools").join("convert").join(record);
    let mut merged: Value = std::fs::read_to_string(&rec_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
    merged[&t.name] = r.clone();
    std::fs::write(&rec_path, serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    let mut j = base;
    j["path"] = json!(format!("tools/convert/{exe_rel}"));
    j["sha256"] = json!(sha);
    j["files"] = json!(n);
    j["source"] = json!(r["source"]);
    j["sha256_upstream"] = r["download_sha256"].clone();
    j["pinned_in"] = json!(if p["lockfile"].is_string() { "lockfile" } else { "sha256_upstream" });
    // doctor --fix checked the download against the pinned SHA-256.
    j["upstream_verified"] = json!(true);
    Ok(Ok(j))
}

/// The SHA-256 of a committed lockfile as git stores it (LF) and as a
/// Windows checkout writes it (CRLF, `core.autocrlf`): `doctor --fix`
/// hashes the checked-out file, the pack has the git blob; the same content
/// either way.
pub fn lockfile_hashes(lf: &[u8]) -> Vec<String> {
    let mut out = vec![hex(&Sha256::digest(lf))];
    if !lf.contains(&b'\r') {
        let crlf: Vec<u8> = lf.iter().flat_map(|&b| if b == b'\n' { vec![b'\r', b'\n'] } else { vec![b] }).collect();
        out.push(hex(&Sha256::digest(&crlf)));
    }
    out
}

/// `curl` (the one Windows ships in System32, else on PATH): `-sSfL -o <dest> <url>`.
fn curl() -> PathBuf {
    if cfg!(windows) {
        let sys = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        sys.join("System32").join("curl.exe")
    } else {
        PathBuf::from("curl")
    }
}

fn tar() -> PathBuf {
    if cfg!(windows) {
        let sys = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        sys.join("System32").join("tar.exe")
    } else {
        PathBuf::from("tar")
    }
}

/// The upstream archive in `cache` for `url`, downloaded if missing, and
/// checked against `sha` (a mismatch deletes it and fails: never packed).
pub fn fetch_checked(url: &str, sha: &str, cache: &Path) -> Result<PathBuf, String> {
    let name = url.rsplit('/').next().filter(|n| !n.is_empty()).ok_or_else(|| format!("{url}: no file name"))?;
    let file = cache.join(name);
    if !file.is_file() {
        std::fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
        let part = cache.join(format!("{name}.part"));
        let st = Command::new(curl()).args(["-sSfL", "--retry", "2", "-o"]).arg(&part).arg(url).status().map_err(|e| format!("curl {url}: {e}"))?;
        if !st.success() {
            let _ = std::fs::remove_file(&part);
            return Err(format!("download {url} failed ({st})"));
        }
        std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
    }
    let (got, _, _) = hash_file(&file, false)?;
    if got != sha {
        let _ = std::fs::remove_file(&file);
        return Err(format!("{name}: SHA-256 {got} is not the pinned {sha} ({PINS_FILE}); the download was deleted and nothing was packed"));
    }
    Ok(file)
}

/// The first file named `name` under `dir` (sorted walk).
fn find_named(dir: &Path, name: &str) -> Option<PathBuf> {
    list_files(dir).ok()?.into_iter().find(|f| f.rsplit('/').next() == Some(name)).map(|f| dir.join(f))
}

/// Place one `bin` tool from its pinned upstream archive (`--download`):
/// fetch into `cache`, check `sha256_upstream`, unpack, copy the program.
/// Outer error: a real failure (a checksum mismatch). Inner: a gap.
fn download_tool(t: &ToolPin, platform: &str, stage: &Path, cache: &Path, log: &Log) -> Result<Result<Value, String>, String> {
    let Some(p) = t.platforms.get(platform) else {
        return Ok(Err(format!("{} {}: no pin for {platform}", t.name, t.version)));
    };
    let (url, sha) = (p["url"].as_str().unwrap_or(""), p["sha256_upstream"].as_str().unwrap_or(""));
    let file = p["file"].as_str().ok_or_else(|| format!("{PINS_FILE}: {} file", t.name))?;
    if url.is_empty() || sha.len() != 64 {
        return Ok(Err(format!("{} {}: no url or sha256_upstream pinned for {platform}", t.name, t.version)));
    }
    let archive = fetch_checked(url, sha, cache)?;
    let unpacked = cache.join("unpacked").join(format!("{}-{}-{platform}", t.name, t.version.replace(' ', "_")));
    remove_dir(&unpacked)?;
    std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
    let st = Command::new(tar()).arg("-xf").arg(&archive).arg("-C").arg(&unpacked).status().map_err(|e| format!("tar: {e}"))?;
    if !st.success() {
        return Err(format!("unpacking {} failed ({st})", archive.display()));
    }
    let Some(src) = find_named(&unpacked, file) else {
        return Err(format!("{}: no {file} inside {}", t.name, archive.display()));
    };
    if platform == host_platform() {
        let v = Command::new(&src).arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)).unwrap_or_default();
        if !version_matches(&v, &t.version) {
            return Err(format!("{}: the pinned download reports `{}`, not {}", t.name, v.trim(), t.version));
        }
    }
    let to = stage.join("tools").join("bin").join(file);
    copy_file(&src, &to)?;
    let (h, bytes, _) = hash_file(&to, false)?;
    log.step(&format!("{} {}: from {} (sha256 checked)", t.name, t.version, url));
    Ok(Ok(json!({
        "name": t.name, "version": t.version, "licence": t.licence, "kind": t.kind, "url": url,
        "sha256_upstream": sha, "path": format!("tools/bin/{file}"), "sha256": h, "bytes": bytes,
        "source": "upstream", "upstream_verified": true,
    })))
}

// ---------------------------------------------------------------- hostable archive (route B)

/// Is this pack file a program (mode 0755 in the archive)?
fn executable(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let in_bin = path.starts_with("toolchain/bin/") || path.starts_with("tools/bin/") || (path.starts_with("toolchain/lib/rustlib/") && path.contains("/bin/"));
    (in_bin && !name.contains('.')) || name.ends_with(".exe") || name.ends_with(".dll") || name.ends_with(".dylib") || (path.starts_with("tools/convert/") && !name.contains('.'))
}

/// One ustar header for a regular file (`path` split into prefix/name).
pub fn ustar_header(path: &str, size: u64, mtime: u64, mode: u32) -> Result<[u8; 512], String> {
    let (prefix, name) = if path.len() <= 100 {
        ("", path)
    } else {
        let cut = path.char_indices().filter(|(i, c)| *c == '/' && *i <= 155 && path.len() - i - 1 <= 100).map(|(i, _)| i).next_back().ok_or_else(|| format!("{path}: too long for a tar header"))?;
        (&path[..cut], &path[cut + 1..])
    };
    let mut h = [0u8; 512];
    let put = |h: &mut [u8; 512], at: usize, b: &[u8]| h[at..at + b.len()].copy_from_slice(b);
    put(&mut h, 0, name.as_bytes());
    put(&mut h, 100, format!("{mode:07o}\0").as_bytes());
    put(&mut h, 108, b"0000000\0");
    put(&mut h, 116, b"0000000\0");
    put(&mut h, 124, format!("{size:011o}\0").as_bytes());
    put(&mut h, 136, format!("{mtime:011o}\0").as_bytes());
    put(&mut h, 148, b"        ");
    h[156] = b'0';
    put(&mut h, 257, b"ustar\0");
    put(&mut h, 263, b"00");
    put(&mut h, 345, prefix.as_bytes());
    let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
    put(&mut h, 148, format!("{sum:06o}\0 ").as_bytes());
    Ok(h)
}

/// Write `pack` (every file, sorted, `manifest.json` included) as a
/// deterministic gzip'd tar at `out`: fixed owner, mode and time.
pub fn write_tar_gz(pack: &Path, out: &Path, mtime: u64) -> Result<(String, u64), String> {
    let f = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut gz = GzEncoder::new(std::io::BufWriter::new(f), Compression::new(6));
    for rel in list_files(pack)? {
        if rel == ".evani-live" {
            continue;
        }
        let data = std::fs::read(pack.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        gz.write_all(&ustar_header(&rel, data.len() as u64, mtime, if executable(&rel) { 0o755 } else { 0o644 })?).map_err(|e| e.to_string())?;
        gz.write_all(&data).map_err(|e| e.to_string())?;
        let pad = (512 - data.len() % 512) % 512;
        gz.write_all(&vec![0u8; pad]).map_err(|e| e.to_string())?;
    }
    gz.write_all(&[0u8; 1024]).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())?;
    let (sha, bytes, _) = hash_file(out, false)?;
    Ok((sha, bytes))
}

/// Route B: `<out>/hosted/evani-engine-<v>-<platform>.tar.gz` and its
/// descriptor `.json` (schema `evani-engine-download/1`): the archive's
/// SHA-256 and size, the pack manifest's SHA-256, and the signature slot
/// (`<descriptor>.sig`, made by Evani.App's release signing; never here).
fn write_hosted(pack: &Path, out: &Path, version: &str, platform: &str, mtime: SystemTime, log: &Log) -> Result<(), String> {
    let dir = out.join("hosted");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = format!("evani-engine-{version}-{platform}");
    let secs = mtime.duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    log.step(&format!("writing the hostable archive {stem}.tar.gz"));
    let (sha, bytes) = write_tar_gz(pack, &dir.join(format!("{stem}.tar.gz")), secs)?;
    let manifest = std::fs::read_to_string(pack.join("manifest.json")).map_err(|e| e.to_string())?;
    let m: Value = serde_json::from_str(&manifest).map_err(|e| e.to_string())?;
    let (msha, _, _) = hash_file(&pack.join("manifest.json"), false)?;
    let desc = json!({
        "schema": "evani-engine-download/1",
        "pack_version": version,
        "platform": platform,
        "engine_commit": m["engine_commit"],
        "live_protocol": m["live_protocol"],
        "complete": m["complete"],
        "release_ready": m["release_ready"],
        "archive": { "file": format!("{stem}.tar.gz"), "format": "tar+gzip", "sha256": sha, "bytes": bytes },
        "unpacked_bytes": m["totals"]["bytes"],
        "manifest_sha256": msha,
        "programs_signed_by": m["signing"]["signer"],
        "signature": { "file": format!("{stem}.json.sig"), "signs": "this file", "status": "unsigned" },
    });
    std::fs::write(dir.join(format!("{stem}.json")), serde_json::to_string_pretty(&desc).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    log.step(&format!("hostable: {} ({:.1} MB)", dir.join(format!("{stem}.tar.gz")).display(), bytes as f64 / 1_048_576.0));
    Ok(())
}

/// THIRD_PARTY_NOTICES.md for the pack: the engine's own notices, then the
/// bundled programs and every vendored crate with its licence.
fn notices(stage: &Path, tools: &[Value], rust: &RustPin) -> Result<String, String> {
    let mut s = String::from("# Third-party notices for the Evani engine pack\n\nThe Evani.Dev engine is MIT (LICENSE). This pack also carries the programs and source crates below, each under its own licence. The engine's own notices follow at the end.\n\n## Bundled programs\n\n| Program | Version | Licence |\n|---|---|---|\n");
    s.push_str(&format!("| Rust toolchain (rustc, cargo, rustdoc, clippy, rustfmt, std, rust-lld) | {} | MIT OR Apache-2.0 (LLVM parts: Apache-2.0 WITH LLVM-exception) |\n", rust.version));
    for t in tools.iter().filter(|t| t.get("missing").is_none()) {
        s.push_str(&format!("| {} | {} | {} |\n", t["name"].as_str().unwrap_or(""), t["version"].as_str().unwrap_or(""), t["licence"].as_str().unwrap_or("")));
    }
    s.push_str("\nNot bundled, by licence: Microsoft C++ Build Tools and the Windows SDK (Windows), Xcode Command Line Tools (Mac). The requirements checker finds them and links to the official installers. Blender (GPL-3.0-or-later, about 400 MB) is not bundled either: for .blend import only, `evani doctor --fix --blender` downloads the pinned release from blender.org into the per-user state folder.\n\n## Vendored crates (source, for offline builds)\n\n| Crate | Version | Licence |\n|---|---|---|\n");
    let vendor = stage.join("vendor");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&vendor).map_err(|e| e.to_string())?.filter_map(Result::ok).map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let Ok(t) = std::fs::read_to_string(d.join("Cargo.toml")).map(|t| t.parse::<toml::Table>()) else { continue };
        let Ok(t) = t else { continue };
        let p = t.get("package");
        let field = |k: &str| p.and_then(|p| p.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let lic = match field("license") {
            l if l.is_empty() => format!("see {} in the crate", field("license-file")),
            l => l,
        };
        s.push_str(&format!("| {} | {} | {} |\n", field("name"), field("version"), lic));
    }
    s.push_str("\n---\n\n");
    s.push_str(&std::fs::read_to_string(stage.join("engine").join("THIRD_PARTY_NOTICES.md")).unwrap_or_default());
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins_text() -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(PINS_FILE)).unwrap()
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-engine-pack-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_checked_in_pins_parse_and_are_consistent() {
        let p = parse_pins(&pins_text()).unwrap();
        assert_eq!(p.live_protocol, "evani-live/1");
        assert!(p.rust.hosts.contains_key("windows-x64") && p.rust.hosts.contains_key("macos-arm64"));
        assert_eq!(p.rust.targets, vec!["wasm32-unknown-unknown".to_string()]);
        // wasm-bindgen CLI pin equals the crate in the engine's Cargo.lock.
        let lock = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock")).unwrap();
        let wb = p.tools.iter().find(|t| t.name == "wasm-bindgen").unwrap();
        assert_eq!(Some(wb.version.clone()), lock_version(&lock, "wasm-bindgen"));
        // The requirements match what the checker knows for each platform.
        assert_eq!(p.requirements["windows-x64"], vec!["msvc-linker", "windows-sdk"]);
        assert_eq!(p.requirements["macos-arm64"], vec!["xcode-clt"]);
        for ids in p.requirements.values() {
            for id in ids {
                assert!(evani_reqcheck::info(id).is_some(), "{id}");
            }
        }
        // Every prebuilt package is a pack workspace member.
        let ws = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")).unwrap();
        assert!(ws.contains("\"tools/evani-reqcheck\""));
        assert!(p.prebuilt.iter().any(|b| b.bin == "evani-reqcheck"));
    }

    #[test]
    fn helper_pins_equal_the_doctor_fix_pins() {
        let p = parse_pins(&pins_text()).unwrap();
        let get = |name: &str| p.tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{name} pinned")).clone();
        for x in evani_guide::extools::PINS {
            let t = get(x.name);
            assert_eq!(t.version, x.version, "{}", x.name);
            let w = &t.platforms["windows-x64"];
            if x.sha256.is_empty() {
                // npm: the committed lockfile pins it.
                assert_eq!(w["lockfile"], format!("{}/package-lock.json", evani_guide::extools::NPM_DIR));
            } else {
                assert_eq!((w["url"].as_str(), w["sha256_upstream"].as_str()), (Some(x.url), Some(x.sha256)), "{}", x.name);
            }
            assert_eq!(t.kind == "external", x.optional, "{}: the optional helper is the external one", x.name);
        }
        let f = get(evani_guide::fonttools::NAME);
        assert_eq!(f.version, evani_guide::fonttools::VERSION);
        assert_eq!(f.platforms["windows-x64"]["sha256_upstream"], evani_guide::fonttools::SHA256);
        assert_eq!(f.platforms["windows-x64"]["url"], evani_guide::fonttools::URL);
        // Every bundled tool on Windows has an upstream checksum (or a lockfile).
        for t in p.tools.iter().filter(|t| t.platforms.contains_key("windows-x64")) {
            let w = &t.platforms["windows-x64"];
            assert!(w["sha256_upstream"].as_str().is_some_and(|s| s.len() == 64) || w["lockfile"].is_string(), "{}", t.name);
        }
        // A lockfile pin matches the git blob (LF) and a Windows checkout (CRLF) of the same content.
        assert_eq!(lockfile_hashes(b"a\nb\n"), vec![sha(b"a\nb\n"), sha(b"a\r\nb\r\n")]);
        assert_eq!(lockfile_hashes(b"a\r\n"), vec![sha(b"a\r\n")]);
        // The manifest lists Blender as pinned but not bundled.
        let input = ManifestIn { pins: &p, platform: "windows-x64", host: "h", commit: "c", created: "t", rust_commit: "r", wb_crate: "0.2.129", tools: &[], prebuilt: &[], gaps: &[], files: &[], signing: &Signing::default() };
        let v = manifest_json(&input);
        assert_eq!(v["external_tools"][0]["name"], "blender");
        assert_eq!(v["external_tools"][0]["bundled"], false);
        assert_eq!(v["external_tools"][0]["sha256_upstream"].as_str().map(str::len), Some(64));
    }

    #[test]
    fn a_member_outside_the_allow_list_is_refused() {
        let mut v: Value = serde_json::from_str(&pins_text()).unwrap();
        v["engine"]["workspace_members"].as_array_mut().unwrap().push(json!("kits/evani-kit-match-3"));
        let e = parse_pins(&v.to_string()).unwrap_err();
        assert!(e.contains("kits/evani-kit-match-3"), "{e}");
    }

    #[test]
    fn allow_list_takes_folders_files_and_exclusions() {
        let inc = vec!["crates/".to_string(), "docs/".to_string(), "README.md".to_string()];
        let exc = vec!["docs/model_research/".to_string()];
        assert!(included("crates/evani-core/src/lib.rs", &inc, &exc));
        assert!(included("README.md", &inc, &exc));
        assert!(!included("README.md.orig", &inc, &exc));
        assert!(!included("docs/model_research/a.png", &inc, &exc));
        assert!(included("docs/SPEC-M1.md", &inc, &exc));
        assert!(!included("kits/evani-kit-match-3/src/lib.rs", &inc, &exc));
        assert!(!included(".vibedeck/screenshots/x.png", &inc, &exc));
    }

    #[test]
    fn the_pack_never_carries_our_games_or_records() {
        let p = parse_pins(&pins_text()).unwrap();
        for bad in ["apps/kart-racer/evani.toml", "kits/evani-kit-match-3/Cargo.toml", ".vibedeck/screenshots/a.png", "tests/fixtures/x.glb", "docs/model_research/a.md", "apps/sandbox/assets/models/frontier-marshal.glb", ".evani/bin/x.exe", "target/debug/x"] {
            assert!(!included(bad, &p.include, &p.exclude), "{bad}");
        }
        for good in ["crates/evani-material-nature/src/lib.rs", "catalog/genres.json", "crates/evani-model/src/lib.rs", "tools/evani-model/src/main.rs", ".vibedeck/dev-harness/live.mjs", "AGENTS.md", "kits/evani-kit-example/src/lib.rs", "packaging/engine-pack.json"] {
            assert!(included(good, &p.include, &p.exclude), "{good}");
        }
    }

    #[test]
    fn workspace_toml_is_trimmed_to_the_pack() {
        let text = "[workspace]\nresolver = \"3\"\nmembers = [\n    \"crates/evani-core\",\n    \"kits/evani-kit-match-3\",\n    \"tools/xtask\",\n]\n\n[workspace.dependencies]\nserde = \"=1\"\n# --- workspace crates ---\nevani-core = { path = \"crates/evani-core\" }\nevani-kit-match-3 = { path = \"kits/evani-kit-match-3\" }\n";
        let members = vec!["crates/evani-core".to_string(), "tools/xtask".to_string()];
        let inc = vec!["crates/".to_string(), "tools/".to_string()];
        let out = trim_workspace_toml(text, &members, &inc, &[]).unwrap();
        assert!(!out.contains("match-3"), "{out}");
        assert!(out.contains("evani-core = { path = \"crates/evani-core\" }"));
        assert!(out.contains("# --- workspace crates ---") && out.contains("serde = \"=1\""));
    }

    #[test]
    fn the_real_workspace_trims_cleanly() {
        let p = parse_pins(&pins_text()).unwrap();
        let ws = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")).unwrap();
        let out = trim_workspace_toml(&ws, &p.members, &p.include, &p.exclude).unwrap();
        for gone in ["match-3", "tactical-fps", "apps/model-view", "2d-platformer"] {
            assert!(!out.contains(gone), "{gone}");
        }
        // The geometry check's built-in adapter needs the kart kit.
        assert!(out.contains("evani-kit-kart-racer = { path = \"kits/evani-kit-kart-racer\" }"));
        assert!(out.contains("evani-material-nature = { path = \"crates/evani-material-nature\" }"));
    }

    #[test]
    fn toolchain_keeps_only_what_builds_a_game() {
        let bins: Vec<String> = ["cargo", "rustc", "rustdoc", "cargo-clippy", "clippy-driver", "rustfmt", "cargo-fmt"].iter().map(|s| s.to_string()).collect();
        let t = vec!["wasm32-unknown-unknown".to_string()];
        let h = "x86_64-pc-windows-msvc";
        let want = |p: &str| toolchain_file_wanted(p, h, &t, &bins);
        for yes in ["bin/cargo.exe", "bin/rustc.exe", "bin/rustc_driver-abc.dll", "bin/std-abc.dll", "bin/clippy-driver.exe", "lib/rustlib/x86_64-pc-windows-msvc/lib/libstd-abc.rlib", "lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe", "lib/rustlib/x86_64-pc-windows-msvc/bin/gcc-ld/ld.lld.exe", "lib/rustlib/wasm32-unknown-unknown/lib/libcore-abc.rlib"] {
            assert!(want(yes), "{yes}");
        }
        for no in ["bin/rustc_driver-abc.pdb", "bin/rust-gdb.exe", "bin/rust-analyzer.exe", "share/doc/rust/html/index.html", "lib/rustlib/src/rust/library/core/src/lib.rs", "lib/rustlib/aarch64-apple-darwin/lib/libstd.rlib", "lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd.rlib", "lib/rustlib/components", "etc/x"] {
            assert!(!want(no), "{no}");
        }
        // Mac: the driver dylib lives in lib/.
        assert!(toolchain_file_wanted("lib/librustc_driver-abc.dylib", "aarch64-apple-darwin", &t, &bins));
    }

    #[test]
    fn ls_tree_output_is_parsed() {
        let out = b"100644 blob 1111111111111111111111111111111111111111\tREADME.md\x00120000 blob 2222222222222222222222222222222222222222\tlink\x00040000 tree 3333333333333333333333333333333333333333\tdir\x00100644 blob 4444444444444444444444444444444444444444\tdocs/a b.md\0";
        let v = parse_ls_tree(out);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], ("100644".into(), "1111111111111111111111111111111111111111".into(), "README.md".into()));
        assert_eq!(v[2].2, "docs/a b.md");
    }

    #[test]
    fn parts_and_sizes_are_grouped() {
        let p = parse_pins(&pins_text()).unwrap();
        assert_eq!(part_of("engine/crates/evani-core/src/lib.rs", &p.parts), "engine-runtime");
        assert_eq!(part_of("engine/crates/evani-material-nature/src/lib.rs", &p.parts), "procedural-and-materials");
        assert_eq!(part_of("engine/catalog/genres.json", &p.parts), "procedural-and-materials");
        assert_eq!(part_of("engine/crates/evani-model/src/lib.rs", &p.parts), "evani-model");
        assert_eq!(part_of("engine/tools/evani-model/src/main.rs", &p.parts), "evani-model");
        assert_eq!(part_of("engine/tools/xtask/src/main.rs", &p.parts), "dev-tools-source");
        assert_eq!(part_of("engine/Cargo.lock", &p.parts), "engine-other");
        assert_eq!(part_of("toolchain/bin/rustc.exe", &p.parts), "toolchain");
        assert_eq!(part_of("vendor/serde-1.0.229/src/lib.rs", &p.parts), "vendor");
        assert_eq!(part_of("tools/bin/trunk.exe", &p.parts), "tools-bin");
        assert_eq!(part_of("tools/convert/ktx-4.4.2/bin/ktx.exe", &p.parts), "tools-convert");
        assert_eq!(part_of("manifest.json", &p.parts), "pack-files");
        let files = vec![
            FileEntry { path: "toolchain/a".into(), sha256: "x".into(), bytes: 10, gz: 4 },
            FileEntry { path: "toolchain/b".into(), sha256: "y".into(), bytes: 5, gz: 2 },
            FileEntry { path: "vendor/c".into(), sha256: "z".into(), bytes: 1, gz: 1 },
        ];
        assert_eq!(part_sizes(&files, &p.parts), vec![("toolchain".into(), 2, 15, 6), ("vendor".into(), 1, 1, 1)]);
    }

    #[test]
    fn manifest_is_deterministic_and_verify_catches_changes() {
        let d = tmp("verify");
        std::fs::create_dir_all(d.join("engine/crates")).unwrap();
        std::fs::write(d.join("engine/crates/a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(d.join("LICENSE"), "MIT\n").unwrap();
        let p = parse_pins(&pins_text()).unwrap();
        let files = hash_tree(&d, true).unwrap();
        let tools = vec![json!({ "name": "wasm-bindgen", "version": "0.2.129", "upstream_verified": true })];
        let unsigned = Signing::default();
        let input = ManifestIn { pins: &p, platform: "windows-x64", host: "x86_64-pc-windows-msvc", commit: "abc", created: "2026-10-27T00:00:00+00:00", rust_commit: "def", wb_crate: "0.2.129", tools: &tools, prebuilt: &[], gaps: &[], files: &files, signing: &unsigned };
        let a = serde_json::to_string_pretty(&manifest_json(&input)).unwrap();
        let b = serde_json::to_string_pretty(&manifest_json(&input)).unwrap();
        assert_eq!(a, b);
        let v: Value = serde_json::from_str(&a).unwrap();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["complete"], true);
        assert_eq!(v["release_ready"], true);
        assert_eq!(v["wasm_bindgen"]["cli_version"], "0.2.129");
        assert_eq!(v["requirements"], json!(["msvc-linker", "windows-sdk"]));
        assert!(!a.contains(&d.display().to_string().replace('\\', "\\\\")), "no absolute paths");
        std::fs::write(d.join("manifest.json"), &a).unwrap();
        assert!(verify(&d).unwrap().ok());
        std::fs::write(d.join("LICENSE"), "changed\n").unwrap();
        std::fs::write(d.join("extra.txt"), "x").unwrap();
        // Evani.App's live marker and a detached signature are not extras.
        std::fs::write(d.join(".evani-live"), "").unwrap();
        std::fs::write(d.join("manifest.sig"), "sig").unwrap();
        std::fs::remove_file(d.join("engine/crates/a.rs")).unwrap();
        let r = verify(&d).unwrap();
        assert_eq!(r.changed, vec!["LICENSE".to_string()]);
        assert_eq!(r.extra, vec!["extra.txt".to_string()]);
        assert_eq!(r.missing, vec!["engine/crates/a.rs".to_string()]);
        // A gap makes the pack incomplete.
        let gaps = vec!["wasm-opt missing".to_string()];
        let v = manifest_json(&ManifestIn { gaps: &gaps, ..input });
        assert_eq!(v["complete"], false);
        assert_eq!(v["release_ready"], false);
        // A local build of a tool is complete but not release-ready.
        let local = vec![json!({ "name": "trunk", "version": "0.21.14", "upstream_verified": false })];
        let v = manifest_json(&ManifestIn { tools: &local, gaps: &[], ..input });
        assert_eq!((v["complete"].clone(), v["release_ready"].clone()), (json!(true), json!(false)));
    }

    #[test]
    fn versions_and_lock_entries_are_read() {
        assert!(version_matches("trunk 0.21.14\n", "0.21.14"));
        assert!(!version_matches("trunk 0.22.1\n", "0.21.14"));
        assert!(version_matches("wasm-opt version 125 (version_125)", "binaryen version_125"));
        assert!(!version_matches("wasm-opt version 124", "binaryen version_125"));
        let lock = "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n\n[[package]]\nname = \"wasm-bindgen-futures\"\nversion = \"0.4.79\"\n";
        assert_eq!(lock_version(lock, "wasm-bindgen").as_deref(), Some("0.2.129"));
        assert_eq!(lock_version(lock, "nope"), None);
    }

    #[test]
    fn engine_root_comes_from_env_then_the_pack_then_the_checkout() {
        let d = tmp("root");
        let compiled = Path::new("C:/somewhere/Evani.Dev");
        // Nothing set: the compiled-in checkout.
        assert_eq!(engine_root_from(None, None, compiled), compiled);
        // A pack: <pack>/tools/bin/evani-xtask(.exe) beside <pack>/engine.
        let pack = d.join("pack");
        std::fs::create_dir_all(pack.join("engine")).unwrap();
        std::fs::create_dir_all(pack.join("tools/bin")).unwrap();
        std::fs::write(pack.join("engine/Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(pack.join("manifest.json"), "{}").unwrap();
        let exe_path = pack.join("tools").join("bin").join("evani-xtask.exe");
        assert_eq!(engine_root_from(None, Some(&exe_path), compiled), pack.join("engine"));
        // A program elsewhere (target/debug) keeps the checkout.
        assert_eq!(engine_root_from(None, Some(&d.join("target/debug/evani-xtask.exe")), compiled), compiled);
        // EVANI_ENGINE_ROOT wins when it holds a Cargo.toml, else is ignored.
        let other = d.join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(engine_root_from(Some(other.as_os_str()), Some(&exe_path), compiled), pack.join("engine"));
        std::fs::write(other.join("Cargo.toml"), "[workspace]\n").unwrap();
        assert_eq!(engine_root_from(Some(other.as_os_str()), Some(&exe_path), compiled), other);
    }

    #[test]
    fn vendor_config_points_cargo_at_the_pack() {
        let c = vendor_config().replace("{PACK}", "C:/Users/u/AppData/Local/Evani.App/evani-dev/engines/0.1.0");
        let t: toml::Table = c.parse().unwrap();
        assert_eq!(t["source"]["crates-io"]["replace-with"].as_str(), Some("evani-pack-vendor"));
        assert!(t["source"]["evani-pack-vendor"]["directory"].as_str().unwrap().ends_with("/vendor"));
        assert_eq!(t["net"]["offline"].as_bool(), Some(true));
    }

    /// Every file a pack member's build reads from outside its own folder
    /// (a `build = "../../tools/bake-index.rs"` script, an `include_bytes!`
    /// of a shared fixture) must be in the allow-list, or the pack can't
    /// build offline. Found by building the sandbox from a pack (2026-10-27).
    #[test]
    fn files_members_reach_outside_their_folder_are_in_the_pack() {
        let p = parse_pins(&pins_text()).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let norm = |base: &str, rel: &str| -> String {
            let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
            for c in rel.split('/') {
                match c {
                    ".." => {
                        parts.pop();
                    }
                    "." | "" => {}
                    c => parts.push(c),
                }
            }
            parts.join("/")
        };
        let mut missing = Vec::new();
        for m in &p.members {
            let t: toml::Table = std::fs::read_to_string(root.join(m).join("Cargo.toml")).unwrap().parse().unwrap();
            if let Some(b) = t.get("package").and_then(|p| p.get("build")).and_then(|b| b.as_str()) {
                let f = norm(m, b);
                if !included(&f, &p.include, &p.exclude) {
                    missing.push(format!("{m}: build script {f}"));
                }
            }
            for f in list_files(&root.join(m).join("src")).unwrap_or_default().into_iter().filter(|f| f.ends_with(".rs")) {
                let text = std::fs::read_to_string(root.join(m).join("src").join(&f)).unwrap();
                let dir = norm(&format!("{m}/src"), f.rsplit_once('/').map_or("", |(d, _)| d));
                for piece in text.split("include_").skip(1) {
                    let Some(arg) = piece.split_once("!(\"").map(|(_, r)| r) else { continue };
                    let Some(rel) = arg.split('"').next().filter(|r| r.starts_with("../")) else { continue };
                    let target = norm(&dir, rel);
                    // Test-only inputs (evani-model's bench models) are not built.
                    if target.starts_with("tests/model_bench/") || included(&target, &p.include, &p.exclude) {
                        continue;
                    }
                    missing.push(format!("{m}/src/{f}: {target}"));
                }
            }
        }
        assert!(missing.is_empty(), "not in packaging/engine-pack.json engine.include:
{}", missing.join("
"));
    }

    #[test]
    fn the_pack_drops_only_the_xtask_alias() {
        let cfg = "[alias]\n# `cargo xtask build sandbox`\nxtask = \"run --quiet --package evani-xtask --\"\n\n[target.wasm32-unknown-unknown]\n# web-sys gates\nrustflags = [\"--cfg=web_sys_unstable_apis\"]\n";
        assert_eq!(pack_cargo_config(cfg), "[target.wasm32-unknown-unknown]\n# web-sys gates\nrustflags = [\"--cfg=web_sys_unstable_apis\"]\n");
        let other = "[alias]\nb = \"build\"\nxtask = \"run -p evani-xtask --\"\n";
        assert_eq!(pack_cargo_config(other), "[alias]\nb = \"build\"\n");
        // The real file keeps its wasm flag and loses the alias.
        let real = pack_cargo_config(&std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cargo/config.toml")).unwrap());
        assert!(real.contains("web_sys_unstable_apis") && !real.contains("evani-xtask"), "{real}");
    }

    #[test]
    fn the_hostable_archive_unpacks_to_the_same_files() {
        let d = tmp("archive");
        let pack = d.join("pack");
        let long = format!("vendor/{}/src/{}.rs", "a".repeat(60), "b".repeat(70));
        for (f, body) in [("manifest.json", "{}"), ("tools/bin/evani.exe", "MZ"), ("engine/Cargo.toml", "[workspace]\n"), (long.as_str(), "fn x() {}\n"), (".evani-live", "")] {
            std::fs::create_dir_all(pack.join(f).parent().unwrap()).unwrap();
            std::fs::write(pack.join(f), body).unwrap();
        }
        let h = ustar_header(&long, 10, 0, 0o644).unwrap();
        assert_eq!(&h[257..263], b"ustar\0");
        assert!(ustar_header(&"x".repeat(300), 1, 0, 0o644).is_err());
        let (a, _) = write_tar_gz(&pack, &d.join("a.tar.gz"), 1_700_000_000).unwrap();
        let (b, _) = write_tar_gz(&pack, &d.join("b.tar.gz"), 1_700_000_000).unwrap();
        assert_eq!(a, b, "the same pack gives the same archive bytes");
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert!(Command::new(tar()).arg("-xf").arg(d.join("a.tar.gz")).arg("-C").arg(&out).status().unwrap().success());
        assert_eq!(list_files(&out).unwrap(), vec!["engine/Cargo.toml".to_string(), "manifest.json".into(), "tools/bin/evani.exe".into(), long.clone()], "everything but the live marker");
        assert_eq!(std::fs::read_to_string(out.join(&long)).unwrap(), "fn x() {}\n");
        assert!(executable("tools/bin/evani.exe") && executable("toolchain/bin/rustc") && !executable("engine/Cargo.toml"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cached_download_with_the_wrong_checksum_is_deleted_and_refused() {
        let d = tmp("fetch");
        std::fs::write(d.join("tool.zip"), "not the real thing").unwrap();
        let e = fetch_checked("https://example.invalid/x/tool.zip", &"0".repeat(64), &d).unwrap_err();
        assert!(e.contains("is not the pinned") && e.contains("nothing was packed"), "{e}");
        assert!(!d.join("tool.zip").exists());
        std::fs::write(d.join("tool.zip"), "abc").unwrap();
        let ok = fetch_checked("https://example.invalid/x/tool.zip", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", &d).unwrap();
        assert_eq!(ok, d.join("tool.zip"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A dummy signer that changes the file (appends a marker), as a real
    /// signature would.
    fn dummy_signer(marker: &str) -> SignCmd {
        #[cfg(windows)]
        let line = format!("powershell -NoProfile -NonInteractive -Command Add-Content -NoNewline -LiteralPath {{file}} -Value {marker}");
        #[cfg(not(windows))]
        let line = format!("sh -c \"printf {marker} >> $EVANI_SIGN_FILE\" {{file}}");
        SignCmd::parse(&line, "dummy test signer").unwrap()
    }

    fn mini_pack(d: &Path) {
        for (p, body) in [
            ("engine/README.md", "readme"),
            ("engine/crates/x/build.exe", "engine source, not a pack program"),
            ("toolchain/bin/rustc.exe", "MZrustc"),
            ("toolchain/bin/rustc_driver.dll", "MZdriver"),
            ("tools/bin/evani.exe", "MZevani"),
            ("tools/convert/ktx-4.4.2/bin/ktx.dll", "MZktx"),
            ("tools/convert/gltf-transform/cli.js", "js"),
            ("vendor/a/lib.rs", "rs"),
        ] {
            std::fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
            std::fs::write(d.join(p), body).unwrap();
        }
    }

    fn sha(b: &[u8]) -> String {
        hex(&Sha256::digest(b))
    }

    #[test]
    fn sign_commands_are_split_and_must_name_the_file() {
        assert_eq!(split_command(r#"node "C:/a b/sign.mjs" {file}"#).unwrap(), vec!["node", "C:/a b/sign.mjs", "{file}"]);
        assert_eq!(split_command(r#"x "" y"#).unwrap(), vec!["x", "", "y"]);
        assert!(split_command(r#"node "open"#).is_err());
        assert!(SignCmd::parse("node sign.mjs", "s").unwrap_err().contains("{file}"));
        assert!(SignCmd::parse("node sign.mjs {file}", " ").unwrap_err().contains("--signer"));
        // Debug never shows the command (it may carry settings).
        let c = SignCmd::parse("tool --password hunter2 {file}", "S").unwrap();
        assert!(!format!("{c:?}").contains("hunter2"));
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (o, _, _) = parse_opts(Path::new("."), &a(&["--sign-cmd", "node s.mjs {file}", "--signer", "Trusted Signing"])).unwrap();
        assert_eq!(o.sign.unwrap().signer, "Trusted Signing");
        assert!(parse_opts(Path::new("."), &a(&["--signer", "x"])).is_err());
    }

    #[test]
    fn programs_are_found_by_platform_libraries_first() {
        let d = tmp("programs");
        mini_pack(&d);
        assert_eq!(find_programs(&d, "windows-x64").unwrap(), vec!["toolchain/bin/rustc_driver.dll", "tools/convert/ktx-4.4.2/bin/ktx.dll", "toolchain/bin/rustc.exe", "tools/bin/evani.exe"]);
        // Mac: any Mach-O by its first bytes; never engine source.
        assert!(is_program("tools/bin/trunk", "macos-arm64", &[0xcf, 0xfa, 0xed, 0xfe]));
        assert!(!is_program("tools/bin/README", "macos-arm64", b"# hi"));
        assert!(is_program("toolchain/lib/librustc_driver-x.dylib", "macos-arm64", b""));
        assert!(!is_program("engine/tools/x.dylib", "macos-arm64", b""));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_signed_pack_is_hashed_after_signing_and_verify_catches_a_later_change() {
        let d = tmp("signed");
        mini_pack(&d);
        let s = sign_programs(&d, "windows-x64", &dummy_signer("SIGNED")).unwrap();
        assert_eq!((s.programs.len(), s.unchanged.len()), (4, 0));
        // Every program got the "signature"; nothing else was touched.
        assert_eq!(std::fs::read_to_string(d.join("tools/bin/evani.exe")).unwrap(), "MZevaniSIGNED");
        assert_eq!(std::fs::read_to_string(d.join("tools/convert/ktx-4.4.2/bin/ktx.dll")).unwrap(), "MZktxSIGNED");
        assert_eq!(std::fs::read_to_string(d.join("engine/crates/x/build.exe")).unwrap(), "engine source, not a pack program");
        assert_eq!(std::fs::read_to_string(d.join("tools/convert/gltf-transform/cli.js")).unwrap(), "js");
        let files = hash_tree(&d, true).unwrap();
        let p = parse_pins(&pins_text()).unwrap();
        let tools = vec![json!({ "name": "wasm-bindgen", "version": "0.2.129", "upstream_verified": true })];
        let v = manifest_json(&ManifestIn { pins: &p, platform: "windows-x64", host: "x86_64-pc-windows-msvc", commit: "abc", created: "2026-10-27T00:00:00+00:00", rust_commit: "def", wb_crate: "0.2.129", tools: &tools, prebuilt: &[], gaps: &[], files: &files, signing: &s });
        assert_eq!(v["signing"], json!({ "status": "signed", "signer": "dummy test signer", "programs": 4, "unchanged": [] }));
        let entry = |path: &str| v["files"].as_array().unwrap().iter().find(|f| f["path"] == path).unwrap().clone();
        // The recorded hash is the signed file's, not the original's.
        assert_eq!(entry("tools/bin/evani.exe")["sha256"], json!(sha(b"MZevaniSIGNED")));
        assert_ne!(entry("tools/bin/evani.exe")["sha256"], json!(sha(b"MZevani")));
        assert_eq!((entry("tools/bin/evani.exe")["program"].clone(), entry("tools/bin/evani.exe")["signed_by"].clone()), (json!(true), json!("dummy test signer")));
        assert!(entry("engine/README.md").get("signed_by").is_none());
        std::fs::write(d.join("manifest.json"), serde_json::to_string_pretty(&v).unwrap()).unwrap();
        let r = verify(&d).unwrap();
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.signer.as_deref(), Some("dummy test signer"));
        // A program changed after signing (here: signed again) is caught.
        dummy_signer("AGAIN").run(&d.join("toolchain/bin/rustc.exe")).unwrap();
        assert_eq!(verify(&d).unwrap().changed, vec!["toolchain/bin/rustc.exe".to_string()]);
        // A signed manifest whose program lost its signer is caught too.
        std::fs::write(d.join("toolchain/bin/rustc.exe"), "MZrustcSIGNED").unwrap();
        let mut m = v.clone();
        for f in m["files"].as_array_mut().unwrap() {
            if f["path"] == "tools/bin/evani.exe" {
                f.as_object_mut().unwrap().remove("signed_by");
            }
        }
        std::fs::write(d.join("manifest.json"), serde_json::to_string_pretty(&m).unwrap()).unwrap();
        let r = verify(&d).unwrap();
        assert_eq!((r.changed.len(), r.unsigned.clone()), (0, vec!["tools/bin/evani.exe".to_string()]));
        assert!(!r.ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_signer_that_fails_or_changes_nothing_refuses_the_pack() {
        let d = tmp("signfail");
        mini_pack(&d);
        // Changes nothing (like sign.mjs without the owner's settings).
        #[cfg(windows)]
        let (noop, fail) = (SignCmd::parse("cmd /d /c rem {file}", "noop").unwrap(), SignCmd::parse("cmd /d /c exit 3 {file}", "fail").unwrap());
        #[cfg(not(windows))]
        let (noop, fail) = (SignCmd::parse("true {file}", "noop").unwrap(), SignCmd::parse("sh -c \"exit 3\" {file}", "fail").unwrap());
        assert!(sign_programs(&d, "windows-x64", &noop).unwrap_err().contains("changed none"));
        assert!(sign_programs(&d, "windows-x64", &fail).unwrap_err().contains("failed"));
        // An unsigned pack lists its programs for Evani.App, with no signer.
        let s = Signing { programs: find_programs(&d, "windows-x64").unwrap(), ..Signing::default() };
        let files = hash_tree(&d, false).unwrap();
        let p = parse_pins(&pins_text()).unwrap();
        let v = manifest_json(&ManifestIn { pins: &p, platform: "windows-x64", host: "h", commit: "c", created: "t", rust_commit: "r", wb_crate: "0.2.129", tools: &[], prebuilt: &[], gaps: &[], files: &files, signing: &s });
        assert_eq!(v["signing"], json!({ "status": "unsigned", "programs": 4 }));
        std::fs::write(d.join("manifest.json"), serde_json::to_string(&v).unwrap()).unwrap();
        let r = verify(&d).unwrap();
        assert!(r.ok() && r.signer.is_none(), "{r:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Compile a one-line program with the active rustc, with or without a
    /// static C runtime, into `dir`. Windows only (the check reads PE files).
    #[cfg(windows)]
    fn compile_hello(dir: &Path, name: &str, crt_static: bool) {
        std::fs::create_dir_all(dir).unwrap();
        let src = dir.join(format!("{name}.rs"));
        std::fs::write(&src, "fn main() { println!(\"hi\"); }\n").unwrap();
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let feature = if crt_static { "-Ctarget-feature=+crt-static" } else { "-Ctarget-feature=-crt-static" };
        let st = Command::new(rustc).args(["--edition", "2021", "-O", "--target", "x86_64-pc-windows-msvc", feature, "-o"]).arg(dir.join(format!("{name}.exe"))).arg(&src).status().unwrap();
        assert!(st.success(), "rustc failed to build the {name} test program");
    }

    #[cfg(windows)]
    #[test]
    fn a_real_program_with_a_dynamic_c_runtime_fails_and_a_static_one_passes() {
        let d = tmp("crt");
        compile_hello(&d, "dynamic", false);
        compile_hello(&d, "static", true);
        let dyn_imports = pe_imports(&std::fs::read(d.join("dynamic.exe")).unwrap()).unwrap();
        assert!(dyn_imports.iter().any(|i| i.starts_with("vcruntime")), "{dyn_imports:?}");
        let static_imports = pe_imports(&std::fs::read(d.join("static.exe")).unwrap()).unwrap();
        assert!(!static_imports.iter().any(|i| is_dynamic_runtime(i)), "{static_imports:?}");
        assert!(static_imports.iter().any(|i| i == "kernel32.dll"), "{static_imports:?}");
        // The check: dynamic fails, static passes, an allow-list entry (with a reason) lets the dynamic one through.
        let r = check_runtime(&d, "windows-x64", &[]).unwrap();
        assert_eq!(r.statics, vec!["static.exe".to_string()]);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].0, "dynamic.exe");
        assert!(!r.ok() && runtime_error(&r).contains("dynamic.exe imports") && runtime_error(&r).contains("vcruntime140.dll"), "{}", runtime_error(&r));
        let allow = [RuntimeAllow { path: "dynamic.exe".into(), reason: "test".into() }, RuntimeAllow { path: "gone.exe".into(), reason: "test".into() }];
        let r = check_runtime(&d, "windows-x64", &allow).unwrap();
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.allowed.len(), 1);
        assert_eq!(r.stale, vec!["gone.exe".to_string()]);
        // A Mac pack has no such imports: not checked.
        assert_eq!(check_runtime(&d, "macos-arm64", &[]).unwrap(), RuntimeReport::default());
        // --verify runs the same check (allow-list read from the manifest).
        let files = hash_tree(&d, false).unwrap();
        let mut p = parse_pins(&pins_text()).unwrap();
        let unsigned = Signing::default();
        let m = |p: &Pins| manifest_json(&ManifestIn { pins: p, platform: "windows-x64", host: "h", commit: "c", created: "t", rust_commit: "r", wb_crate: "0.2.129", tools: &[], prebuilt: &[], gaps: &[], files: &files, signing: &unsigned });
        p.runtime_allow.clear();
        std::fs::write(d.join("manifest.json"), serde_json::to_string(&m(&p)).unwrap()).unwrap();
        let v = verify(&d).unwrap();
        assert!(!v.ok() && v.runtime.violations.len() == 1, "{v:?}");
        p.runtime_allow.insert("windows-x64".into(), allow.to_vec());
        std::fs::write(d.join("manifest.json"), serde_json::to_string(&m(&p)).unwrap()).unwrap();
        assert!(verify(&d).unwrap().ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn c_runtime_dll_names_are_recognised() {
        for n in ["VCRUNTIME140.dll", "vcruntime140_1.dll", "MSVCP140.dll", "ucrtbase.dll", "api-ms-win-crt-math-l1-1-0.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
            assert!(is_dynamic_runtime(n), "{n}");
        }
        for n in ["kernel32.dll", "KERNEL32.dll", "ntdll.dll", "api-ms-win-core-synch-l1-2-0.dll", "bcryptprimitives.dll"] {
            assert!(!is_dynamic_runtime(n), "{n}");
        }
        // Not a PE file, or a truncated one: no imports, no panic.
        assert_eq!(pe_imports(b"hello"), None);
        assert_eq!(pe_imports(b"MZ\x00\x00"), None);
    }

    #[test]
    fn the_pins_allow_list_names_a_reason_for_every_program() {
        let p = parse_pins(&pins_text()).unwrap();
        let list = &p.runtime_allow["windows-x64"];
        assert!(list.iter().any(|a| a.path == "tools/bin/trunk.exe"));
        assert!(list.iter().all(|a| a.reason.len() > 20));
        // Our own programs are never on it: they are built static.
        assert!(!list.iter().any(|a| a.path.contains("evani")));
        let mut v: Value = serde_json::from_str(&pins_text()).unwrap();
        v["runtime_allow"]["windows-x64"][0]["reason"] = json!(" ");
        assert!(parse_pins(&v.to_string()).unwrap_err().contains("reason"));
    }
}
