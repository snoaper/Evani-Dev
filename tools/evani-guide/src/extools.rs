//! The pinned external programs the model converter (`cargo xtask convert`,
//! tools/evani-convert) drives: gltfpack, KTX-Software's `ktx`, the
//! glTF-Transform CLI (with sharp, from its npm tree) and, on request,
//! Blender. They are separate programs, never linked (STAGE3_PLAN decision
//! 1 and 2). `cargo xtask doctor --fix` installs these exact versions into
//! the gitignored `.evani/bin/` and records version and checksum in
//! `.evani/bin/convert-tools.json`; the converter refuses a missing or
//! changed tool with an error that names that fix.
//!
//! Downloads are checked against the SHA-256 pinned here before anything is
//! unpacked or run. The npm tree is pinned by the committed lockfile
//! `tools/evani-convert/npm/package-lock.json` (`npm ci` checks every
//! package's integrity hash).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Where the tools live (gitignored with `.evani/`).
pub const BIN_DIR: &str = ".evani/bin";
/// The install record: version, source, download and program checksums.
pub const RECORD: &str = "convert-tools.json";
/// The one line every "tool missing" error ends with.
pub const FIX: &str = "fix: run `cargo xtask doctor --fix` (installs the pinned model-converter tools into .evani/bin)";
/// The committed npm manifest and lockfile for glTF-Transform.
pub const NPM_DIR: &str = "tools/evani-convert/npm";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A zip holding the program.
    Zip,
    /// A silent NSIS installer (`/S /D=<dir>`), which writes only into `dir`.
    Nsis,
    /// The committed npm lockfile (`npm ci`).
    Npm,
}

/// One pinned tool.
#[derive(Clone, Copy, Debug)]
pub struct Pin {
    pub name: &'static str,
    pub version: &'static str,
    pub licence: &'static str,
    pub source: Source,
    /// Download URL (Zip, Nsis), or the npm package spec (Npm).
    pub url: &'static str,
    /// SHA-256 of the download (Zip, Nsis); empty for Npm (the lockfile pins it).
    pub sha256: &'static str,
    /// The install folder under `.evani/bin/`.
    pub dir: &'static str,
    /// The program inside `dir` (for Npm: the CLI's script).
    pub exe: &'static str,
    /// Only on request (`doctor --fix --blender`): it is 400 MB.
    pub optional: bool,
}

/// The pins (Windows x64, the platform this engine is developed on; other
/// platforms get a clear "no pinned download" error).
pub const PINS: &[Pin] = &[
    Pin {
        name: "gltfpack",
        version: "1.3",
        licence: "MIT",
        source: Source::Zip,
        url: "https://github.com/zeux/meshoptimizer/releases/download/v1.3/gltfpack-windows.zip",
        sha256: "f6e9c09d66af23da3da71b86f0652d2413e81c734e0aecd1cd3d0e6e6e8645c0",
        dir: "gltfpack-1.3",
        exe: "gltfpack.exe",
        optional: false,
    },
    Pin {
        name: "ktx",
        version: "4.4.2",
        licence: "Apache-2.0",
        source: Source::Nsis,
        url: "https://github.com/KhronosGroup/KTX-Software/releases/download/v4.4.2/KTX-Software-4.4.2-Windows-x64.exe",
        sha256: "1f323b0fec19794f5e6c0425a61d4b1da396872a10be862d105f4f4b2d2957fe",
        dir: "ktx-4.4.2",
        exe: "bin/ktx.exe",
        optional: false,
    },
    Pin {
        name: "gltf-transform",
        version: "4.5.1",
        licence: "MIT",
        source: Source::Npm,
        url: "@gltf-transform/cli@4.5.1",
        sha256: "",
        dir: "gltf-transform-4.5.1",
        exe: "node_modules/@gltf-transform/cli/bin/cli.js",
        optional: false,
    },
    Pin {
        name: "blender",
        version: "5.2.2",
        licence: "GPL-3.0-or-later (a separate program; no Blender code in the engine)",
        source: Source::Zip,
        url: "https://download.blender.org/release/Blender5.2/blender-5.2.2-windows-x64.zip",
        sha256: "3849d17a682cba006075aaa3f3597ecb5c9c30ec31035b2e092c53e40679b535",
        dir: "blender-5.2.2",
        exe: "blender-5.2.2-windows-x64/blender.exe",
        optional: true,
    },
];

pub fn pin(name: &str) -> Option<&'static Pin> {
    PINS.iter().find(|p| p.name == name)
}

pub fn bin_dir(root: &Path) -> PathBuf {
    root.join(BIN_DIR)
}

/// The program's path inside `.evani/bin/` (it may not exist).
pub fn exe_path(root: &Path, p: &Pin) -> PathBuf {
    bin_dir(root).join(p.dir).join(p.exe)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_file(p: &Path) -> Result<String, String> {
    std::fs::read(p).map(|b| sha256_hex(&b)).map_err(|e| format!("{}: {e}", p.display()))
}

fn read_record(root: &Path) -> Value {
    std::fs::read_to_string(bin_dir(root).join(RECORD)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolState {
    Missing,
    /// Present but not the pinned version or checksum (the reason).
    Changed(String),
    Current,
}

/// Is `p` installed as pinned?
pub fn state(root: &Path, p: &Pin) -> ToolState {
    let exe = exe_path(root, p);
    if !exe.is_file() {
        return ToolState::Missing;
    }
    let rec = read_record(root);
    let r = &rec[p.name];
    if r["version"].as_str() != Some(p.version) {
        return ToolState::Changed(format!("installed {} is not the pinned {}", r["version"].as_str().unwrap_or("(unrecorded)"), p.version));
    }
    match (sha256_file(&exe), r["exe_sha256"].as_str()) {
        (Ok(h), Some(want)) if h == want => ToolState::Current,
        (Ok(_), Some(_)) => ToolState::Changed("the program differs from the one installed (checksum)".into()),
        _ => ToolState::Changed("no checksum recorded".into()),
    }
}

/// The program to run, or an error naming the fix.
pub fn locate(root: &Path, name: &str) -> Result<PathBuf, String> {
    let p = pin(name).ok_or_else(|| format!("no pinned tool named {name}"))?;
    match state(root, p) {
        ToolState::Current => Ok(exe_path(root, p)),
        ToolState::Missing => {
            let fix = if p.optional { format!("fix: run `cargo xtask doctor --fix --{}` (installs {} {} into .evani/bin)", p.name, p.name, p.version) } else { FIX.to_string() };
            Err(format!("{} {} is not installed ({} missing)\n{fix}", p.name, p.version, exe_path(root, p).display()))
        }
        ToolState::Changed(why) => Err(format!("{} in .evani/bin is not the pinned version: {why}\n{FIX}", p.name)),
    }
}

/// One line per non-current required tool (for `doctor`).
pub fn lines(root: &Path) -> Vec<String> {
    PINS.iter()
        .filter(|p| !p.optional)
        .filter_map(|p| match state(root, p) {
            ToolState::Current => None,
            ToolState::Missing => Some(format!("{BIN_DIR}/{}: model-converter tool {} {} not installed (needed only by `cargo xtask convert`)", p.dir, p.name, p.version)),
            ToolState::Changed(w) => Some(format!("{BIN_DIR}/{}: model-converter tool {}: {w}", p.dir, p.name)),
        })
        .collect()
}

fn system32(exe: &str) -> PathBuf {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    PathBuf::from(sys).join("System32").join(exe)
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Fetch `url` to `dest` and check its SHA-256.
fn download(url: &str, sha: &str, dest: &Path) -> Result<(), String> {
    if dest.is_file() && sha256_file(dest)? == sha {
        return Ok(());
    }
    let curl = if cfg!(windows) { system32("curl.exe") } else { PathBuf::from("curl") };
    run(Command::new(curl).args(["-sSfL", "-o"]).arg(dest).arg(url), &format!("download {url}"))?;
    let got = sha256_file(dest)?;
    if got != sha {
        let _ = std::fs::remove_file(dest);
        return Err(format!("{url}: checksum {got} is not the pinned {sha}; nothing was installed"));
    }
    Ok(())
}

fn npm() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

/// Install one pin (download, check, unpack, record).
pub fn install_one(root: &Path, p: &Pin) -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86_64")) && p.source != Source::Npm {
        return Err(format!("{}: no pinned download for this platform yet (only Windows x64); install {} {} yourself into {}", p.name, p.name, p.version, bin_dir(root).join(p.dir).display()));
    }
    let bin = bin_dir(root);
    let dl = bin.join("downloads");
    std::fs::create_dir_all(&dl).map_err(|e| e.to_string())?;
    let dir = bin.join(p.dir);
    let mut download_sha = p.sha256.to_string();
    match p.source {
        Source::Zip => {
            let file = dl.join(p.url.rsplit('/').next().unwrap_or("download.zip"));
            download(p.url, p.sha256, &file)?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let tar = if cfg!(windows) { system32("tar.exe") } else { PathBuf::from("tar") };
            run(Command::new(tar).arg("-xf").arg(&file).arg("-C").arg(&dir), &format!("unpack {}", file.display()))?;
        }
        Source::Nsis => {
            // Named `-installer.exe` so nothing mistakes the installer for
            // the tool; the converter only ever runs the installed program
            // by its full path.
            let name = p.url.rsplit('/').next().unwrap_or("download.exe").trim_end_matches(".exe");
            let file = dl.join(format!("{name}-installer.exe"));
            download(p.url, p.sha256, &file)?;
            // NSIS wants `/D=` last, unquoted, absolute. KTX-Software's
            // installer writes only into that folder (checked 2026-10-16: no
            // PATH, registry or start-menu change), but its manifest asks
            // for administrator rights, so Windows shows an elevation
            // prompt. `__COMPAT_LAYER=RunAsInvoker` (on this child only)
            // runs it as the user without a prompt; the folder is the
            // user's own. If that ever fails it is an error, never an
            // elevated retry.
            let abs = std::path::absolute(&dir).map_err(|e| e.to_string())?;
            run(
                Command::new(&file).env("__COMPAT_LAYER", "RunAsInvoker").arg("/S").arg(format!("/D={}", abs.display())),
                &format!("install {} without administrator rights (fix: make sure {} is writable by you, then run `cargo xtask doctor --fix` again)", p.name, abs.display()),
            )?;
        }
        Source::Npm => {
            let src = root.join(NPM_DIR);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for f in ["package.json", "package-lock.json"] {
                std::fs::copy(src.join(f), dir.join(f)).map_err(|e| format!("{}: {e}", src.join(f).display()))?;
            }
            run(Command::new(npm()).current_dir(&dir).args(["ci", "--no-audit", "--no-fund", "--loglevel=error"]), "npm ci (glTF-Transform)")?;
            download_sha = sha256_file(&src.join("package-lock.json"))?;
        }
    }
    let exe = exe_path(root, p);
    if !exe.is_file() {
        return Err(format!("{}: installed, but {} is not there", p.name, exe.display()));
    }
    let mut rec = read_record(root);
    rec[p.name] = json!({
        "version": p.version,
        "licence": p.licence,
        "source": p.url,
        "download_sha256": download_sha,
        "exe": format!("{}/{}", p.dir, p.exe),
        "exe_sha256": sha256_file(&exe)?,
    });
    let text = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())?;
    crate::write_file(&bin.join(RECORD), &format!("{text}\n"))
}

/// What `doctor --fix` did for the converter tools.
#[derive(Debug, Default)]
pub struct Installed {
    pub installed: Vec<String>,
    pub errors: Vec<String>,
}

/// Install every required tool that isn't current (and the optional ones
/// named in `extra`, e.g. `blender`).
pub fn install(root: &Path, extra: &[&str]) -> Installed {
    let mut out = Installed::default();
    for p in PINS.iter().filter(|p| !p.optional || extra.contains(&p.name)) {
        if state(root, p) == ToolState::Current {
            continue;
        }
        match install_one(root, p) {
            Ok(()) => out.installed.push(format!("{} {}", p.name, p.version)),
            Err(e) => out.errors.push(e),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_names_the_fix() {
        let r = crate::testutil::temp_root("extools");
        let e = locate(&r, "gltfpack").unwrap_err();
        assert!(e.contains("gltfpack 1.3 is not installed") && e.contains("cargo xtask doctor --fix"), "{e}");
        let e = locate(&r, "blender").unwrap_err();
        assert!(e.contains("doctor --fix --blender"), "{e}");
        assert_eq!(lines(&r).len(), 3, "the three required tools, not Blender");
    }

    #[test]
    fn a_changed_program_is_not_current() {
        let r = crate::testutil::temp_root("extools-changed");
        let p = pin("gltfpack").unwrap();
        crate::write_file(&exe_path(&r, p), "a program").unwrap();
        assert!(matches!(state(&r, p), ToolState::Changed(_)), "no record");
        let rec = json!({ "gltfpack": { "version": "1.3", "exe_sha256": sha256_hex(b"a program") } });
        crate::write_file(&bin_dir(&r).join(RECORD), &rec.to_string()).unwrap();
        assert_eq!(state(&r, p), ToolState::Current);
        crate::write_file(&exe_path(&r, p), "another program").unwrap();
        assert!(matches!(state(&r, p), ToolState::Changed(w) if w.contains("checksum")));
        assert!(locate(&r, "gltfpack").unwrap_err().contains("cargo xtask doctor --fix"));
    }

    #[test]
    fn every_download_is_pinned_by_checksum() {
        for p in PINS {
            match p.source {
                Source::Npm => assert!(p.sha256.is_empty() && p.url.contains(&format!("@{}", p.version))),
                _ => assert!(p.sha256.len() == 64 && p.url.contains(p.version), "{}", p.name),
            }
        }
    }
}
