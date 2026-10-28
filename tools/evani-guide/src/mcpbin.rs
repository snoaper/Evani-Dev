//! The prebuilt live MCP server in `.evani/bin/`.
//!
//! Why: `.mcp.json` used to start the server with `cargo run`. The bridge's
//! sources change almost daily, so most session starts rebuilt it (40 to
//! 150 s at two build jobs) and Claude Code gave up waiting (about 30 s):
//! `evani-live` "failed to connect". Now `.mcp.json` names the binary
//! `doctor --fix` (or `init-ai`) built and copied here, with a stamp of the
//! sources it was built from; `doctor` and `check` say when it is stale.
//! A stale binary still starts at once.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Folder of the installed binary (gitignored with `.evani/`).
pub const BIN_DIR: &str = ".evani/bin";
const STAMP: &str = "evani-live-bridge.stamp";

/// The sources the bridge is built from (its path dependencies).
const SOURCES: &[&str] = &[
    "tools/evani-live-bridge",
    "tools/evani-lookguard",
    "tools/evani-guide",
    "tools/evani-geocheck",
    "crates/evani-live",
    "crates/evani-core",
    "crates/evani-render",
    "Cargo.lock",
];

pub fn exe_name() -> String {
    format!("evani-live-bridge{}", std::env::consts::EXE_SUFFIX)
}

pub fn exe_path(root: &Path) -> PathBuf {
    root.join(BIN_DIR).join(exe_name())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Missing,
    /// Built from older sources.
    Stale,
    Current,
}

/// A hash of every source file (path and bytes), `target`/`dist` skipped.
pub fn source_hash(root: &Path) -> String {
    let mut files = Vec::new();
    for s in SOURCES {
        collect(&root.join(s), &mut files);
    }
    files.sort();
    let mut buf = Vec::new();
    for f in &files {
        buf.extend_from_slice(crate::rel(root, f).as_bytes());
        buf.push(0);
        if let Ok(b) = std::fs::read(f) {
            // Line endings don't change the build.
            buf.extend(b.iter().filter(|c| **c != b'\r'));
        }
        buf.push(0);
    }
    crate::fnv_hex(&buf)
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_file() {
        out.push(p.to_path_buf());
        return;
    }
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), "target" | "dist" | ".git" | "node_modules") {
            continue;
        }
        collect(&e.path(), out);
    }
}

pub fn state(root: &Path) -> State {
    if !exe_path(root).is_file() {
        return State::Missing;
    }
    match std::fs::read_to_string(root.join(BIN_DIR).join(STAMP)) {
        Ok(s) if s.trim() == source_hash(root) => State::Current,
        _ => State::Stale,
    }
}

/// Build the bridge (`target/mcp`, debug) and copy it to `.evani/bin/`. A
/// running server keeps its file locked on Windows, so the old file is moved
/// aside first; the next session starts the new one.
pub fn install(root: &Path) -> Result<PathBuf, String> {
    let hash = source_hash(root);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(root)
        .args(["build", "--quiet", "--package", "evani-live-bridge", "--target-dir", "target/mcp"])
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build -p evani-live-bridge failed ({status})"));
    }
    let built = root.join("target/mcp/debug").join(exe_name());
    let dest = exe_path(root);
    std::fs::create_dir_all(root.join(BIN_DIR)).map_err(|e| e.to_string())?;
    if std::fs::copy(&built, &dest).is_err() {
        let aside = dest.with_extension(format!("old-{}", now_ms()));
        std::fs::rename(&dest, &aside).map_err(|e| format!("{} is in use and can't be moved aside: {e}", dest.display()))?;
        std::fs::copy(&built, &dest).map_err(|e| format!("copy to {}: {e}", dest.display()))?;
    }
    remove_old_copies(root);
    crate::write_file(&root.join(BIN_DIR).join(STAMP), &format!("{hash}\n"))?;
    Ok(dest)
}

/// Best effort: delete copies moved aside earlier (still locked ones stay).
fn remove_old_copies(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root.join(BIN_DIR)) else { return };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().contains(".old-") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_tracks_the_sources() {
        let r = crate::testutil::temp_root("mcpbin");
        crate::write_file(&r.join("crates/evani-live/src/lib.rs"), "a").unwrap();
        crate::write_file(&r.join("crates/evani-live/target/debug/junk"), "x").unwrap();
        assert_eq!(state(&r), State::Missing);
        crate::write_file(&exe_path(&r), "bin").unwrap();
        assert_eq!(state(&r), State::Stale, "no stamp");
        let h = source_hash(&r);
        crate::write_file(&r.join(BIN_DIR).join(STAMP), &h).unwrap();
        assert_eq!(state(&r), State::Current);
        crate::write_file(&r.join("crates/evani-live/target/debug/junk"), "y").unwrap();
        assert_eq!(state(&r), State::Current, "build output doesn't count");
        crate::write_file(&r.join("crates/evani-live/src/lib.rs"), "a\r\n").unwrap();
        crate::write_file(&r.join("crates/evani-live/src/lib.rs"), "b").unwrap();
        assert_eq!(state(&r), State::Stale);
    }
}
