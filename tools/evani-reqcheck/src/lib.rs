//! The build requirements checker (docs/ENGINE_PACK.md, "Requirements
//! checker"). It answers one question for Evani.App's Evani.dev tab: can
//! this computer build an Evani.Dev game right now?
//!
//! Two layers, and only the second may say "ready":
//! 1. Detection: the system tools we may not ship are looked for where their
//!    official installers put them (Windows: the MSVC linker through
//!    `vswhere.exe` and the Visual Studio folders, the Windows SDK through the
//!    registry and its default folder; Mac: `xcode-select -p` and
//!    `xcrun --find clang`). A missing tool gets one plain sentence: "To build
//!    games you need <tool>, because <why>", the official installer link, and
//!    the re-check hint.
//! 2. The probe: with every tool found, a tiny crate with a build script is
//!    built offline for `wasm32-unknown-unknown` with the pack's own
//!    toolchain. The build script is a host program, so this proves the host
//!    linker and SDK work; the wasm link proves `rust-lld` and the wasm std.
//!
//! Standard library only (it runs before any Rust is installed). Re-checking
//! is running it again; Evani.App does that when the installer it started
//! exits, when its window regains focus, and on "Check again" (never on a
//! timer).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// The JSON report's schema name.
pub const SCHEMA: &str = "evani-requirements/1";
/// The id of the runtime requirement (needed for importing, not for building).
pub const RUNTIME_ID: &str = "vc-runtime";
/// The x64 runtime files the import helpers load (in System32).
pub const RUNTIME_DLLS: &[&str] = &["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"];
/// Microsoft's Build Tools page, and the direct download of the redistributable.
pub const BUILD_TOOLS_URL: &str = "https://visualstudio.microsoft.com/visual-cpp-build-tools/";
pub const REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";
/// The environment variable that hides tools (a comma list of ids), for
/// tests and the "missing" demo. Hiding can only make the answer worse:
/// nothing here can turn a missing tool into a found one.
pub const HIDE_VAR: &str = "EVANI_REQCHECK_HIDE";

/// The platforms the engine pack is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    MacosArm64,
    MacosX64,
    /// Anything else (Linux, 32-bit, ...): no pack exists yet.
    Other,
}

impl Platform {
    pub fn host() -> Platform {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Platform::WindowsX64
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Platform::MacosArm64
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Platform::MacosX64
        } else {
            Platform::Other
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Platform::WindowsX64 => "windows-x64",
            Platform::MacosArm64 => "macos-arm64",
            Platform::MacosX64 => "macos-x64",
            Platform::Other => "other",
        }
    }

    /// The ids of the system tools this platform needs (never shipped).
    pub fn requirement_ids(self) -> &'static [&'static str] {
        match self {
            Platform::WindowsX64 => &["msvc-linker", "windows-sdk"],
            Platform::MacosArm64 | Platform::MacosX64 => &["xcode-clt"],
            Platform::Other => &[],
        }
    }
}

/// What a requirement is, in plain words, and where its official installer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub id: &'static str,
    /// The tool's name as the user will see it on the installer.
    pub name: &'static str,
    /// Why a game build needs it (finishes "because ...").
    pub why: &'static str,
    /// The official page (never a mirror, never a copy of ours).
    pub install_url: &'static str,
    /// What to do there, in plain words.
    pub install_hint: &'static str,
}

/// Every requirement the checker knows.
pub const INFOS: &[Info] = &[
    Info {
        id: "msvc-linker",
        name: "Microsoft's free C++ Build Tools",
        why: "Evani uses their linker to put together the small helper programs every game build runs on this PC first, and Microsoft doesn't allow us to include them in Evani.App",
        install_url: "https://visualstudio.microsoft.com/visual-cpp-build-tools/",
        install_hint: "Download Build Tools from Microsoft's page, tick \"Desktop development with C++\" (keep the Windows SDK box ticked), install, then choose Check again. Windows may ask for your administrator OK; that is Microsoft's installer asking, and it is fine because you started it.",
    },
    Info {
        id: "windows-sdk",
        name: "the Windows SDK",
        why: "it holds the Windows system libraries that the linker joins into those helper programs, and Microsoft doesn't allow us to include it in Evani.App",
        install_url: "https://developer.microsoft.com/windows/downloads/windows-sdk/",
        install_hint: "It comes with Microsoft's C++ Build Tools when \"Windows SDK\" stays ticked under \"Desktop development with C++\"; or install it on its own from Microsoft's page. Then choose Check again.",
    },
    Info {
        id: "vc-runtime",
        name: "the Microsoft Visual C++ runtime",
        why: "the model and texture import helpers Evani includes (made by others) are built on it and won't start without it, and Microsoft doesn't allow us to include it in Evani.App",
        install_url: "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist",
        install_hint: "To import models only, install Microsoft's \"Visual C++ Redistributable (x64)\" from that page (direct download: https://aka.ms/vs/17/release/vc_redist.x64.exe), then choose Check again. If you install Microsoft's C++ Build Tools for building games, they include this runtime, so you don't need both.",
    },
    Info {
        id: "xcode-clt",
        name: "Apple's Command Line Tools for Xcode",
        why: "Evani uses Apple's linker to put together the small helper programs every game build runs on this Mac first, and Apple doesn't allow us to include them in Evani.App",
        install_url: "https://developer.apple.com/xcode/resources/",
        install_hint: "Choose Install: Apple's own installer opens (the same as running `xcode-select --install`). When it finishes, choose Check again. You don't need the full Xcode app.",
    },
];

pub fn info(id: &str) -> Option<&'static Info> {
    INFOS.iter().find(|i| i.id == id)
}

/// The environment the checker reads, captured once so tests (and the
/// "missing" demo) can give it another one.
#[derive(Clone, Debug, Default)]
pub struct Env {
    vars: BTreeMap<String, String>,
    hide: BTreeSet<String>,
}

/// The variables detection reads.
const VARS: &[&str] = &["ProgramFiles(x86)", "ProgramFiles", "SystemRoot", HIDE_VAR];

impl Env {
    pub fn from_process() -> Env {
        let mut e = Env::default();
        for k in VARS {
            if let Ok(v) = std::env::var(k) {
                e.vars.insert((*k).to_string(), v);
            }
        }
        let hide = e.vars.get(HIDE_VAR).cloned().unwrap_or_default();
        e.hide_ids(&hide);
        e
    }

    pub fn with_var(mut self, k: &str, v: &str) -> Env {
        self.vars.insert(k.to_string(), v.to_string());
        self
    }

    /// Hide a comma list of requirement ids (`all` hides every one).
    pub fn hide_ids(&mut self, list: &str) {
        for id in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            self.hide.insert(id.to_string());
        }
    }

    pub fn var(&self, k: &str) -> Option<&str> {
        self.vars.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }

    pub fn hidden(&self, id: &str) -> bool {
        self.hide.contains(id) || self.hide.contains("all")
    }
}

/// One requirement's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Found, with where.
    Found(String),
    /// Missing, with what was looked at.
    Missing(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub info: &'static Info,
    pub state: State,
}

impl Requirement {
    pub fn found(&self) -> bool {
        matches!(self.state, State::Found(_))
    }

    /// The one plain sentence the tab shows for a missing tool.
    pub fn message(&self) -> String {
        format!("To build games you need {}, because {}.", self.info.name, self.info.why)
    }
}

// ---------------------------------------------------------------- Windows

/// Lines of `vswhere -property installationPath` output (one path a line).
pub fn parse_lines(out: &str) -> Vec<PathBuf> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

/// Folders below `dir`, sorted newest version first (by name, with numeric
/// parts compared as numbers, so `14.44.35207` beats `14.9.1`).
fn subdirs_newest_first(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort_by_key(|p| std::cmp::Reverse(version_key(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())));
    v
}

/// `10.0.26100.0` -> [10, 0, 26100, 0]; non-numbers sort lowest.
fn version_key(name: &str) -> Vec<u64> {
    name.split(['.', '-', '_']).map(|p| p.parse().unwrap_or(0)).collect()
}

/// The x64 `link.exe` of the newest MSVC under any of `installs` (Visual
/// Studio or Build Tools installation folders).
pub fn find_link_exe(installs: &[PathBuf]) -> Option<PathBuf> {
    installs.iter().find_map(|inst| {
        subdirs_newest_first(&inst.join("VC").join("Tools").join("MSVC"))
            .into_iter()
            .map(|v| v.join("bin").join("Hostx64").join("x64").join("link.exe"))
            .find(|l| l.is_file())
    })
}

/// Visual Studio installation folders: what `vswhere.exe` (Microsoft's own
/// locator, installed with every Visual Studio and Build Tools since 2017)
/// reports for the C++ x64 tools component, then the default folders
/// `<Program Files>/Microsoft Visual Studio/<year>/<edition>` as a fallback.
pub fn vs_installs(env: &Env) -> (Vec<PathBuf>, Vec<String>) {
    let mut looked = Vec::new();
    let mut out = Vec::new();
    if let Some(pf86) = env.var("ProgramFiles(x86)") {
        let vswhere = Path::new(pf86).join("Microsoft Visual Studio").join("Installer").join("vswhere.exe");
        looked.push(vswhere.display().to_string());
        if vswhere.is_file()
            && let Ok(o) = Command::new(&vswhere)
                .args(["-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath", "-utf8"])
                .output()
            && o.status.success()
        {
            out.extend(parse_lines(&String::from_utf8_lossy(&o.stdout)));
        }
    }
    for k in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = env.var(k) {
            let base = Path::new(pf).join("Microsoft Visual Studio");
            looked.push(base.display().to_string());
            for year in subdirs_newest_first(&base) {
                out.extend(subdirs_newest_first(&year));
            }
        }
    }
    (out, looked)
}

/// The value `name` in `reg query` output (a line like
/// `    KitsRoot10    REG_SZ    C:\Program Files (x86)\Windows Kits\10\`).
pub fn parse_reg_value(out: &str, name: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let mut parts = l.trim().splitn(3, "    ");
        let (n, ty, v) = (parts.next()?, parts.next()?, parts.next()?);
        (n.eq_ignore_ascii_case(name) && ty.starts_with("REG_")).then(|| v.trim().to_string()).filter(|v| !v.is_empty())
    })
}

/// Windows SDK roots: the registry's `KitsRoot10` (64- and 32-bit views,
/// read with Windows' own `reg.exe`), then the default folder.
pub fn sdk_roots(env: &Env) -> (Vec<PathBuf>, Vec<String>) {
    let mut looked = Vec::new();
    let mut out = Vec::new();
    if let Some(sys) = env.var("SystemRoot") {
        let reg = Path::new(sys).join("System32").join("reg.exe");
        for key in [r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots"] {
            looked.push(format!("{key} KitsRoot10"));
            if reg.is_file()
                && let Ok(o) = Command::new(&reg).args(["query", key, "/v", "KitsRoot10"]).output()
                && o.status.success()
                && let Some(v) = parse_reg_value(&String::from_utf8_lossy(&o.stdout), "KitsRoot10")
            {
                out.push(PathBuf::from(v));
            }
        }
    }
    if let Some(pf86) = env.var("ProgramFiles(x86)") {
        let d = Path::new(pf86).join("Windows Kits").join("10");
        looked.push(d.display().to_string());
        out.push(d);
    }
    (out, looked)
}

/// The x64 `kernel32.lib` of the newest SDK under any of `roots`.
pub fn find_kernel32(roots: &[PathBuf]) -> Option<PathBuf> {
    roots.iter().find_map(|r| {
        subdirs_newest_first(&r.join("Lib"))
            .into_iter()
            .map(|v| v.join("um").join("x64").join("kernel32.lib"))
            .find(|l| l.is_file())
    })
}

// ---------------------------------------------------------------- Mac

fn command_ok(prog: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(prog).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// ---------------------------------------------------------------- detection

/// Detect one requirement.
pub fn detect(id: &str, env: &Env) -> Option<Requirement> {
    let info = info(id)?;
    if env.hidden(id) {
        return Some(Requirement { info, state: State::Missing(format!("hidden by {HIDE_VAR} or --hide (a test)")) });
    }
    let state = match id {
        "msvc-linker" => {
            let (installs, looked) = vs_installs(env);
            match find_link_exe(&installs) {
                Some(l) => State::Found(l.display().to_string()),
                None => State::Missing(format!("no x64 link.exe under VC\\Tools\\MSVC in any Visual Studio or Build Tools install (looked at: {})", looked.join("; "))),
            }
        }
        "windows-sdk" => {
            let (roots, looked) = sdk_roots(env);
            match find_kernel32(&roots) {
                Some(l) => State::Found(l.display().to_string()),
                None => State::Missing(format!("no Lib\\<version>\\um\\x64\\kernel32.lib in any Windows 10/11 SDK (looked at: {})", looked.join("; "))),
            }
        }
        RUNTIME_ID => {
            // The files the helpers load: 64-bit Windows keeps the 64-bit
            // runtime in System32, whether the redistributable or the Build
            // Tools put it there. Looking for the files themselves (not a
            // registry flag) answers the real question: will they start?
            match env.var("SystemRoot").map(|r| Path::new(r).join("System32")) {
                Some(d) => {
                    let missing: Vec<&str> = RUNTIME_DLLS.iter().copied().filter(|n| !d.join(n).is_file()).collect();
                    if missing.is_empty() {
                        State::Found(d.display().to_string())
                    } else {
                        State::Missing(format!("{} not found in {}", missing.join(", "), d.display()))
                    }
                }
                None => State::Missing("SystemRoot is not set, so System32 can't be checked".into()),
            }
        }
        "xcode-clt" => match (command_ok("xcode-select", &["-p"]), command_ok("xcrun", &["--find", "clang"])) {
            (Some(dir), Some(clang)) => State::Found(format!("{dir} (clang: {clang})")),
            (None, _) => State::Missing("`xcode-select -p` found no developer tools".into()),
            (Some(dir), None) => State::Missing(format!("`xcode-select -p` names {dir} but `xcrun --find clang` finds no compiler there")),
        },
        _ => return None,
    };
    Some(Requirement { info, state })
}

// ---------------------------------------------------------------- probe

/// The toolchain the probe builds with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    pub cargo: PathBuf,
    /// `RUSTC` for the child (the pack's own rustc); `None` = cargo finds it.
    pub rustc: Option<PathBuf>,
    /// `pack` (the engine pack's toolchain/), `given` (--cargo) or `path`.
    pub source: &'static str,
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// The pack's toolchain if this program sits in an engine pack
/// (`<pack>/tools/bin/evani-reqcheck` beside `<pack>/toolchain/bin/cargo`).
pub fn pack_toolchain(exe_dir: &Path) -> Option<Toolchain> {
    let pack = exe_dir.parent()?.parent()?;
    let bin = pack.join("toolchain").join("bin");
    let cargo = bin.join(exe("cargo"));
    cargo.is_file().then(|| Toolchain { rustc: Some(bin.join(exe("rustc"))).filter(|r| r.is_file()), cargo, source: "pack" })
}

/// `--cargo <path>` wins, then the pack's toolchain, then `cargo` on PATH.
pub fn find_toolchain(given: Option<&Path>, exe_dir: Option<&Path>) -> Option<Toolchain> {
    if let Some(c) = given {
        return Some(Toolchain { cargo: c.to_path_buf(), rustc: None, source: "given" });
    }
    if let Some(t) = exe_dir.and_then(pack_toolchain) {
        return Some(t);
    }
    command_ok("cargo", &["-V"]).map(|_| Toolchain { cargo: PathBuf::from("cargo"), rustc: None, source: "path" })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Probe {
    Passed { seconds: f64 },
    Failed { detail: String, seconds: f64 },
    Skipped(String),
}

pub const PROBE_TARGET: &str = "wasm32-unknown-unknown";

/// The probe crate: a build script (a host program: needs the host linker
/// and system libraries) and a `cdylib` for wasm (needs rust-lld and the
/// wasm std). No dependencies, so it builds offline anywhere.
pub fn probe_files() -> [(&'static str, &'static str); 4] {
    [
        ("Cargo.toml", "[package]\nname = \"evani-reqprobe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\npath = \"lib.rs\"\n\n[workspace]\n"),
        ("build.rs", "fn main() {\n    println!(\"cargo:rustc-env=EVANI_PROBE=ok\");\n}\n"),
        ("lib.rs", "#[no_mangle]\npub extern \"C\" fn evani_probe() -> u32 {\n    env!(\"EVANI_PROBE\").len() as u32\n}\n"),
        ("Cargo.lock", "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"evani-reqprobe\"\nversion = \"0.0.0\"\n"),
    ]
}

/// The last `n` non-empty lines of `s`.
fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Build the probe crate in `work` (its own folder; written fresh each run).
pub fn run_probe(tc: &Toolchain, work: &Path) -> Probe {
    let start = Instant::now();
    let src = work.join("src");
    if let Err(e) = std::fs::create_dir_all(&src) {
        return Probe::Failed { detail: format!("can't make the probe folder {}: {e}", src.display()), seconds: 0.0 };
    }
    for (name, text) in probe_files() {
        if let Err(e) = std::fs::write(src.join(name), text) {
            return Probe::Failed { detail: format!("can't write {}: {e}", src.join(name).display()), seconds: 0.0 };
        }
    }
    let mut cmd = Command::new(&tc.cargo);
    cmd.arg("build")
        .arg("--offline")
        .arg("--locked")
        .arg("--target")
        .arg(PROBE_TARGET)
        .arg("--manifest-path")
        .arg(src.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(work.join("target"))
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TERM_COLOR", "never")
        // A user's own flags or target must not change the answer.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_TARGET_DIR");
    if let Some(r) = &tc.rustc {
        cmd.env("RUSTC", r);
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Probe::Failed { detail: format!("can't start {}: {e}", tc.cargo.display()), seconds: start.elapsed().as_secs_f64() },
    };
    let seconds = start.elapsed().as_secs_f64();
    let wasm = work.join("target").join(PROBE_TARGET).join("debug").join("evani_reqprobe.wasm");
    if out.status.success() && wasm.is_file() {
        Probe::Passed { seconds }
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Probe::Failed { detail: format!("cargo build ({}) failed:\n{}", out.status, tail(&err, 15)), seconds }
    }
}

// ---------------------------------------------------------------- report

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub platform: Platform,
    pub requirements: Vec<Requirement>,
    pub toolchain: Option<Toolchain>,
    pub probe: Probe,
    /// The Visual C++ runtime (Windows only; `None` where none is needed).
    pub runtime: Option<Requirement>,
}

/// One thing the user wants to do, and whether this PC can do it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub id: &'static str,
    /// Plain label ("Building games").
    pub label: &'static str,
    pub ready: bool,
    /// "ready", or "needs ...".
    pub status: String,
    /// Ids of the requirements missing for this capability.
    pub needs: Vec<String>,
}

impl Report {
    /// Building games follows [`Report::ready`] (so `ready` and the exit code
    /// keep their meaning); importing models needs only the runtime.
    pub fn capabilities(&self) -> Vec<Capability> {
        let unavailable = "not available on this computer yet (Windows 64-bit and Mac only)".to_string();
        let build_ready = self.ready();
        let build_status = if build_ready {
            "ready".to_string()
        } else if self.platform == Platform::Other {
            unavailable.clone()
        } else if self.platform == Platform::WindowsX64 {
            "needs Microsoft C++ Build Tools (+ Windows SDK)".to_string()
        } else {
            "needs Apple's Command Line Tools for Xcode".to_string()
        };
        let missing: Vec<String> = self.requirements.iter().filter(|r| !r.found()).map(|r| r.info.id.to_string()).collect();
        let build = Capability { id: "build-games", label: "Building games", ready: build_ready, status: build_status, needs: missing };
        let (ready, status, needs) = match &self.runtime {
            _ if self.platform == Platform::Other => (false, unavailable, Vec::new()),
            Some(r) if !r.found() => (false, "needs the Microsoft Visual C++ runtime".to_string(), vec![r.info.id.to_string()]),
            _ => (true, "ready".to_string(), Vec::new()),
        };
        vec![build, Capability { id: "import-models", label: "Importing models", ready, status, needs }]
    }

    fn runtime_missing(&self) -> Option<&Requirement> {
        self.runtime.as_ref().filter(|r| !r.found())
    }
}

impl Report {
    /// Ready only when every tool is found and the probe built.
    pub fn ready(&self) -> bool {
        self.platform != Platform::Other && self.requirements.iter().all(Requirement::found) && matches!(self.probe, Probe::Passed { .. })
    }

    /// The plain-words text (the card's lines, then details).
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ready() {
            s.push_str("Ready: this computer can build Evani games.\n");
        } else if self.platform == Platform::Other {
            s.push_str("Not ready: Evani can build games on Windows (64-bit) and Mac only for now.\n");
        } else {
            s.push_str("Not ready to build games yet.\n");
        }
        for r in &self.requirements {
            match &r.state {
                State::Found(at) => s.push_str(&format!("  found  {} ({at})\n", r.info.name)),
                State::Missing(looked) => {
                    s.push_str(&format!("  MISSING  {}\n    {}\n    {}\n    Official installer: {}\n    (details: {looked})\n", r.info.name, r.message(), r.info.install_hint, r.info.install_url));
                }
            }
        }
        match &self.probe {
            Probe::Passed { seconds } => s.push_str(&format!("  test build: passed in {seconds:.1} s ({})\n", self.toolchain_text())),
            Probe::Failed { detail, seconds } => s.push_str(&format!(
                "  test build: FAILED after {seconds:.1} s ({})\n    Evani tried a tiny test build and it did not work, so games can't be built yet. Reinstall the tool above if you just installed it, then choose Check again.\n    (details: {})\n",
                self.toolchain_text(),
                detail.replace('\n', "\n      ")
            )),
            Probe::Skipped(why) => s.push_str(&format!("  test build: not run ({why})\n")),
        }
        s.push_str("What this computer can do:\n");
        for c in self.capabilities() {
            s.push_str(&format!("  {}: {}\n", c.label, c.status));
        }
        if let Some(r) = self.runtime_missing() {
            let looked = match &r.state {
                State::Missing(l) | State::Found(l) => l,
            };
            s.push_str(&format!(
                "  MISSING  {}\n    To import models you need {}, because {}.\n    {}\n    Official page (Visual C++ Redistributable, x64): {}\n    Microsoft's C++ Build Tools page: {} (the Build Tools include this runtime)\n    (details: {looked})\n",
                r.info.name, r.info.name, r.info.why, r.info.install_hint, r.info.install_url, BUILD_TOOLS_URL
            ));
        }
        if (!self.ready() && self.platform != Platform::Other) || self.runtime_missing().is_some() {
            s.push_str("When you've installed what's missing, check again (Evani.App: \"Check again\").\n");
        }
        s
    }

    fn toolchain_text(&self) -> String {
        self.toolchain.as_ref().map_or("no toolchain".into(), |t| format!("{} toolchain: {}", t.source, t.cargo.display()))
    }

    /// The JSON report Evani.App reads (schema [`SCHEMA`]).
    pub fn json(&self) -> String {
        let mut reqs = Vec::new();
        for r in &self.requirements {
            let (status, detail) = match &r.state {
                State::Found(at) => ("found", at.as_str()),
                State::Missing(looked) => ("missing", looked.as_str()),
            };
            reqs.push(format!(
                "{{\"id\":{},\"name\":{},\"status\":{},\"detail\":{},\"message\":{},\"why\":{},\"install_url\":{},\"install_hint\":{}}}",
                js(r.info.id),
                js(r.info.name),
                js(status),
                js(detail),
                js(&r.message()),
                js(r.info.why),
                js(r.info.install_url),
                js(r.info.install_hint)
            ));
        }
        let probe = match &self.probe {
            Probe::Passed { seconds } => format!("{{\"status\":\"passed\",\"seconds\":{seconds:.2}}}"),
            Probe::Failed { detail, seconds } => format!("{{\"status\":\"failed\",\"seconds\":{seconds:.2},\"detail\":{}}}", js(detail)),
            Probe::Skipped(why) => format!("{{\"status\":\"skipped\",\"detail\":{}}}", js(why)),
        };
        let tc = self.toolchain.as_ref().map_or("null".into(), |t| format!("{{\"source\":{},\"cargo\":{}}}", js(t.source), js(&t.cargo.display().to_string())));
        let caps: Vec<String> = self
            .capabilities()
            .iter()
            .map(|c| {
                format!(
                    "{{\"id\":{},\"label\":{},\"ready\":{},\"status\":{},\"needs\":[{}]}}",
                    js(c.id),
                    js(c.label),
                    c.ready,
                    js(&c.status),
                    c.needs.iter().map(|n| js(n)).collect::<Vec<_>>().join(",")
                )
            })
            .collect();
        let runtime = match &self.runtime {
            None => "null".to_string(),
            Some(r) => {
                let (status, detail) = match &r.state {
                    State::Found(at) => ("found", at.as_str()),
                    State::Missing(looked) => ("missing", looked.as_str()),
                };
                format!(
                    "{{\"id\":{},\"name\":{},\"status\":{},\"detail\":{},\"message\":{},\"why\":{},\"install_url\":{},\"install_hint\":{},\"redist_url\":{},\"build_tools_url\":{},\"build_tools_include_it\":true}}",
                    js(r.info.id),
                    js(r.info.name),
                    js(status),
                    js(detail),
                    js(&format!("To import models you need {}, because {}.", r.info.name, r.info.why)),
                    js(r.info.why),
                    js(r.info.install_url),
                    js(r.info.install_hint),
                    js(REDIST_URL),
                    js(BUILD_TOOLS_URL)
                )
            }
        };
        format!(
            "{{\"schema\":{},\"platform\":{},\"ready\":{},\"requirements\":[{}],\"toolchain\":{tc},\"probe\":{probe},\"capabilities\":[{}],\"runtime\":{runtime}}}",
            js(SCHEMA),
            js(self.platform.id()),
            self.ready(),
            reqs.join(","),
            caps.join(",")
        )
    }
}

/// A JSON string literal.
pub fn js(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Detection for `platform`, then the probe when everything was found.
pub fn check(platform: Platform, env: &Env, toolchain: Option<Toolchain>, work: &Path, probe: bool) -> Report {
    let requirements: Vec<Requirement> = platform.requirement_ids().iter().filter_map(|id| detect(id, env)).collect();
    let probe = if platform == Platform::Other {
        Probe::Skipped("no engine pack for this platform".into())
    } else if !probe {
        Probe::Skipped("--no-probe".into())
    } else if let Some(m) = requirements.iter().find(|r| !r.found()) {
        Probe::Skipped(format!("{} is missing", m.info.name))
    } else if let Some(tc) = &toolchain {
        run_probe(tc, work)
    } else {
        Probe::Failed { detail: "no Rust toolchain: not inside an engine pack, no --cargo and no `cargo` on PATH (Evani.App's engine pack may be damaged: reinstall Evani.App)".into(), seconds: 0.0 }
    };
    let runtime = if platform == Platform::WindowsX64 { detect(RUNTIME_ID, env) } else { None };
    Report { platform, requirements, toolchain, probe, runtime }
}

// ---------------------------------------------------------------- cli

pub const USAGE: &str = "usage: evani-reqcheck [--json] [--no-probe] [--hide <id,id|all>] [--cargo <path>] [--work <dir>]\n       evani-reqcheck --open <id>     (opens the official installer for one requirement)\n  (also `cargo xtask doctor --requirements [..]`)\n  ids: msvc-linker, windows-sdk, vc-runtime (Windows); xcode-clt (Mac). Exit 0 = ready to build games, 1 = not ready to build, 2 = usage. (Importing models is reported as its own capability and does not change the exit code.)";

/// Open the official installer for `id`: Apple's own dialog for the Command
/// Line Tools, else the official page in the default browser. Evani.App runs
/// the check again when it's done (installer exit, window focus, Check again).
pub fn open(id: &str) -> Result<(), String> {
    let i = info(id).ok_or_else(|| format!("unknown requirement `{id}`\n{USAGE}"))?;
    let status = if id == "xcode-clt" {
        Command::new("xcode-select").arg("--install").status()
    } else if cfg!(windows) {
        Command::new("rundll32").args(["url.dll,FileProtocolHandler", i.install_url]).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(i.install_url).status()
    } else {
        Command::new("xdg-open").arg(i.install_url).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("opening {} ended with {s}; open {} yourself", i.name, i.install_url)),
        Err(e) => Err(format!("can't open {}: {e}; open {} yourself", i.name, i.install_url)),
    }
}

/// The work folder: `--work`, else `$EVANI_STATE_DIR/reqcheck`, else the
/// system temp folder. Never inside the pack (it is read-only).
pub fn default_work() -> PathBuf {
    match std::env::var_os("EVANI_STATE_DIR").filter(|v| !v.is_empty()) {
        Some(s) => PathBuf::from(s).join("reqcheck"),
        None => std::env::temp_dir().join("evani-reqcheck"),
    }
}

/// Run the checker with `args` (without the program name); the exit code.
pub fn cli(args: &[String]) -> i32 {
    let mut json = false;
    let mut probe = true;
    let mut env = Env::from_process();
    let mut cargo = None;
    let mut work = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value\n{USAGE}"));
        let r = match a.as_str() {
            "--json" => {
                json = true;
                Ok(())
            }
            "--no-probe" => {
                probe = false;
                Ok(())
            }
            "--hide" => value("--hide").map(|v| env.hide_ids(&v)),
            "--cargo" => value("--cargo").map(|v| cargo = Some(PathBuf::from(v))),
            "--work" => value("--work").map(|v| work = Some(PathBuf::from(v))),
            "--open" => {
                return match value("--open").and_then(|id| open(&id)) {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("{e}");
                        2
                    }
                };
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return 0;
            }
            other => Err(format!("unknown option `{other}`\n{USAGE}")),
        };
        if let Err(e) = r {
            eprintln!("{e}");
            return 2;
        }
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let tc = find_toolchain(cargo.as_deref(), exe_dir.as_deref());
    let report = check(Platform::host(), &env, tc, &work.unwrap_or_else(default_work), probe);
    if json {
        println!("{}", report.json());
    } else {
        print!("{}", report.text());
    }
    if report.ready() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-reqcheck-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn finds_newest_msvc_link_exe() {
        let d = tmp("msvc");
        let inst = d.join("BuildTools");
        touch(&inst.join("VC/Tools/MSVC/14.9.1/bin/Hostx64/x64/link.exe"));
        touch(&inst.join("VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe"));
        // A newer version without the x64 linker is skipped.
        std::fs::create_dir_all(inst.join("VC/Tools/MSVC/14.50.1/bin")).unwrap();
        let l = find_link_exe(&[d.join("nothing"), inst.clone()]).unwrap();
        assert!(l.to_string_lossy().contains("14.44.35207"), "{}", l.display());
        assert_eq!(find_link_exe(&[d.join("nothing")]), None);
    }

    #[test]
    fn default_vs_folders_are_scanned_without_vswhere() {
        let d = tmp("vsdefault");
        touch(&d.join("Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.1/bin/Hostx64/x64/link.exe"));
        let env = Env::default().with_var("ProgramFiles(x86)", &d.display().to_string());
        let r = detect("msvc-linker", &env).unwrap();
        assert!(r.found(), "{r:?}");
        let empty = tmp("vsempty");
        let env = Env::default().with_var("ProgramFiles(x86)", &empty.display().to_string()).with_var("ProgramFiles", &empty.display().to_string());
        let r = detect("msvc-linker", &env).unwrap();
        assert!(!r.found());
        assert_eq!(r.message(), "To build games you need Microsoft's free C++ Build Tools, because Evani uses their linker to put together the small helper programs every game build runs on this PC first, and Microsoft doesn't allow us to include them in Evani.App.");
    }

    #[test]
    fn reads_kitsroot_from_reg_output() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows Kits\\Installed Roots\r\n    KitsRoot10    REG_SZ    C:\\Program Files (x86)\\Windows Kits\\10\\\r\n\r\n";
        assert_eq!(parse_reg_value(out, "KitsRoot10").as_deref(), Some("C:\\Program Files (x86)\\Windows Kits\\10\\"));
        assert_eq!(parse_reg_value(out, "KitsRoot81"), None);
        assert_eq!(parse_reg_value("ERROR: The system was unable to find the specified registry key or value.", "KitsRoot10"), None);
    }

    #[test]
    fn finds_sdk_kernel32_in_the_default_folder() {
        let d = tmp("sdk");
        touch(&d.join("Windows Kits/10/Lib/10.0.22621.0/um/x64/kernel32.lib"));
        touch(&d.join("Windows Kits/10/Lib/10.0.26100.0/um/x64/kernel32.lib"));
        // SystemRoot unset: no registry read, the default folder is used.
        let env = Env::default().with_var("ProgramFiles(x86)", &d.display().to_string());
        let r = detect("windows-sdk", &env).unwrap();
        match &r.state {
            State::Found(at) => assert!(at.contains("10.0.26100.0"), "{at}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hidden_tools_are_missing_and_the_probe_does_not_run() {
        let mut env = Env::default();
        env.hide_ids("msvc-linker, windows-sdk");
        let tc = Toolchain { cargo: PathBuf::from("cargo-that-must-not-run"), rustc: None, source: "given" };
        let r = check(Platform::WindowsX64, &env, Some(tc), &tmp("hidden"), true);
        assert!(!r.ready());
        assert!(r.requirements.iter().all(|q| !q.found()));
        assert!(matches!(r.probe, Probe::Skipped(_)), "{:?}", r.probe);
        let text = r.text();
        assert!(text.contains("To build games you need Microsoft's free C++ Build Tools, because"), "{text}");
        assert!(text.contains("https://visualstudio.microsoft.com/visual-cpp-build-tools/"), "{text}");
        assert!(text.contains("check again"), "{text}");
        let json = r.json();
        assert!(json.starts_with("{\"schema\":\"evani-requirements/1\",\"platform\":\"windows-x64\",\"ready\":false,"), "{json}");
        assert!(json.contains("\"status\":\"missing\""), "{json}");
        let mut all = Env::default();
        all.hide_ids("all");
        assert!(all.hidden("xcode-clt"));
    }

    #[test]
    fn mac_needs_the_command_line_tools() {
        assert_eq!(Platform::MacosArm64.requirement_ids(), &["xcode-clt"]);
        let mut env = Env::default();
        env.hide_ids("xcode-clt");
        let r = check(Platform::MacosArm64, &env, None, &tmp("mac"), true);
        assert!(r.text().contains("To build games you need Apple's Command Line Tools for Xcode, because Evani uses Apple's linker"), "{}", r.text());
    }

    #[test]
    fn detection_alone_never_says_ready() {
        let r = Report { platform: Platform::WindowsX64, requirements: Vec::new(), toolchain: None, probe: Probe::Skipped("--no-probe".into()), runtime: None };
        assert!(!r.ready());
        let failed = Report { probe: Probe::Failed { detail: "x".into(), seconds: 1.0 }, ..r.clone() };
        assert!(!failed.ready());
        let passed = Report { probe: Probe::Passed { seconds: 1.0 }, ..r };
        assert!(passed.ready());
        let other = Report { platform: Platform::Other, ..passed };
        assert!(!other.ready());
    }

    #[test]
    fn pack_layout_toolchain_is_found_next_to_the_program() {
        let d = tmp("pack");
        touch(&d.join("toolchain/bin").join(exe("cargo")));
        touch(&d.join("toolchain/bin").join(exe("rustc")));
        let bin = d.join("tools/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let t = find_toolchain(None, Some(&bin)).unwrap();
        assert_eq!(t.source, "pack");
        assert_eq!(t.cargo, d.join("toolchain/bin").join(exe("cargo")));
        assert!(t.rustc.is_some());
        let given = find_toolchain(Some(Path::new("x/cargo")), Some(&bin)).unwrap();
        assert_eq!(given.source, "given");
    }

    fn sys32(dir: &Path, names: &[&str]) -> Env {
        std::fs::create_dir_all(dir.join("System32")).unwrap();
        for n in names {
            touch(&dir.join("System32").join(n));
        }
        Env::default().with_var("SystemRoot", &dir.display().to_string())
    }

    #[test]
    fn runtime_present_means_importing_is_ready() {
        let env = sys32(&tmp("rt-ok"), RUNTIME_DLLS);
        assert!(detect(RUNTIME_ID, &env).unwrap().found());
        let rep = check(Platform::WindowsX64, &env, None, &tmp("rt-ok-w"), false);
        let caps = rep.capabilities();
        assert_eq!(caps[1].id, "import-models");
        assert!(caps[1].ready && caps[1].status == "ready" && caps[1].needs.is_empty(), "{caps:?}");
        assert!(rep.text().contains("Importing models: ready"), "{}", rep.text());
    }

    #[test]
    fn runtime_missing_is_reported_in_plain_words_with_both_links() {
        // One of the three files absent counts as missing.
        let mut env = sys32(&tmp("rt-missing"), &["vcruntime140.dll", "msvcp140.dll"]);
        env.hide_ids("msvc-linker, windows-sdk");
        let rep = check(Platform::WindowsX64, &env, None, &tmp("rt-missing-w"), true);
        assert!(!rep.runtime.as_ref().unwrap().found());
        let caps = rep.capabilities();
        assert_eq!(caps[0].status, "needs Microsoft C++ Build Tools (+ Windows SDK)");
        assert_eq!(caps[0].needs, vec!["msvc-linker", "windows-sdk"]);
        assert!(!caps[1].ready && caps[1].status == "needs the Microsoft Visual C++ runtime" && caps[1].needs == vec!["vc-runtime"], "{caps:?}");
        let text = rep.text();
        assert!(text.contains("Building games: needs Microsoft C++ Build Tools (+ Windows SDK)"), "{text}");
        assert!(text.contains("Importing models: needs the Microsoft Visual C++ runtime"), "{text}");
        assert!(text.contains("https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist") && text.contains(BUILD_TOOLS_URL), "{text}");
        assert!(text.contains("vcruntime140_1.dll") && text.contains("Build Tools include this runtime"), "{text}");
        // The old contract is intact: same schema and leading fields; the new ones are added.
        let json = rep.json();
        assert!(json.starts_with("{\"schema\":\"evani-requirements/1\",\"platform\":\"windows-x64\",\"ready\":false,\"requirements\":["), "{json}");
        assert!(json.contains("\"capabilities\":[{\"id\":\"build-games\",\"label\":\"Building games\",\"ready\":false"), "{json}");
        assert!(json.contains("{\"id\":\"import-models\",\"label\":\"Importing models\",\"ready\":false,\"status\":\"needs the Microsoft Visual C++ runtime\",\"needs\":[\"vc-runtime\"]}"), "{json}");
        assert!(json.contains("\"build_tools_include_it\":true") && json.contains("https://aka.ms/vs/17/release/vc_redist.x64.exe"), "{json}");
    }

    #[test]
    fn runtime_can_be_hidden_like_the_c_plus_plus_tools() {
        let mut env = sys32(&tmp("rt-hide"), RUNTIME_DLLS);
        env.hide_ids("vc-runtime");
        let rep = check(Platform::WindowsX64, &env, None, &tmp("rt-hide-w"), false);
        assert!(!rep.runtime.unwrap().found());
        let mut all = Env::default();
        all.hide_ids("all");
        assert!(!detect(RUNTIME_ID, &all).unwrap().found());
    }

    #[test]
    fn mac_needs_no_visual_cpp_runtime() {
        let rep = check(Platform::MacosArm64, &Env::default(), None, &tmp("rt-mac"), false);
        assert!(rep.runtime.is_none());
        assert!(rep.capabilities()[1].ready);
        assert!(rep.json().contains("\"runtime\":null"));
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(js("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

    #[test]
    fn probe_crate_is_self_contained() {
        let files = probe_files();
        let toml = files[0].1;
        assert!(toml.contains("[workspace]") && !toml.contains("[dependencies]\n"));
        assert!(files.iter().any(|(n, t)| *n == "build.rs" && t.contains("cargo:rustc-env")));
    }
}
