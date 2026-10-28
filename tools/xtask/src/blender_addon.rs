//! `cargo xtask blender-addon [--test]`: build the "Export to Evani.dev"
//! Blender add-on (tools/blender-addon/evani_export, GPL-3.0-or-later, a
//! separate folder no engine or game crate may reference) into
//! `<target>/blender-addon/` with `blender --command extension build`, and with
//! `--test` run it in a headless Blender (tools/blender-addon/tests).
//!
//! No Blender: prints `SKIPPED: Blender not installed ...` and passes; with
//! `EVANI_REQUIRE_TOOLS=1` it fails instead, so a pass then means it ran.

use std::path::{Path, PathBuf};
use std::process::Command;

fn blender(root: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EVANI_BLENDER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Ok(p) = evani_guide::extools::locate(root, "blender") {
        return Some(p);
    }
    Some(root.join(".evani").join("tools").join("blender-5.2.2-windows-x64").join("blender.exe")).filter(|p| p.is_file())
}

fn require_tools() -> bool {
    std::env::var("EVANI_REQUIRE_TOOLS").is_ok_and(|v| v == "1")
}

/// Where cargo puts build output (honours CARGO_TARGET_DIR).
fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// A Blender command with its own scratch user folder, so a build or test never
/// reads or changes the person's own Blender settings and add-ons.
fn blender_cmd(exe: &Path, scratch: &Path) -> Command {
    let mut c = Command::new(exe);
    c.env("BLENDER_USER_RESOURCES", scratch);
    c
}

/// Build the zip; returns its path, or `Ok(None)` when Blender is missing and that is allowed.
pub fn build(root: &Path) -> Result<Option<PathBuf>, String> {
    let Some(exe) = blender(root) else {
        let note = "Blender not installed: cannot build the add-on\nfix: run `cargo xtask doctor --fix --blender` (or set EVANI_BLENDER to blender.exe)";
        if require_tools() {
            return Err(format!("EVANI_REQUIRE_TOOLS=1 but {note}"));
        }
        println!("SKIPPED: {note}");
        return Ok(None);
    };
    let src = root.join("tools").join("blender-addon").join("evani_export");
    let out = target_dir(root).join("blender-addon");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for old in std::fs::read_dir(&out).map_err(|e| e.to_string())?.flatten() {
        if old.path().extension().is_some_and(|e| e == "zip") {
            let _ = std::fs::remove_file(old.path());
        }
    }
    let scratch = out.join("user");
    let output = blender_cmd(&exe, &scratch)
        .args(["--factory-startup", "--command", "extension", "build", "--source-dir"])
        .arg(&src)
        .arg("--output-dir")
        .arg(&out)
        .output()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let zip = std::fs::read_dir(&out).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "zip"));
    match (output.status.success(), zip) {
        (true, Some(z)) => {
            println!("blender-addon: built {}", z.display());
            Ok(Some(z))
        }
        _ => Err(format!("blender extension build failed ({}):\n{}", output.status, text.trim())),
    }
}

/// Install the built zip in a headless Blender, export a model made in the
/// script and check the converted files appear. `Ok(false)` = skipped.
pub fn test(root: &Path) -> Result<bool, String> {
    let Some(zip) = build(root)? else {
        return Ok(false);
    };
    let exe = blender(root).ok_or("Blender vanished")?;
    let script = root.join("tools").join("blender-addon").join("tests").join("headless_test.py");
    let scratch = zip.parent().unwrap_or(Path::new(".")).join("user");
    let output = blender_cmd(&exe, &scratch)
        .args(["--background", "--factory-startup", "-t", "2", "--python"])
        .arg(&script)
        .arg("--")
        .arg(&zip)
        .arg(root)
        .output()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    for l in text.lines().filter(|l| l.starts_with("evani-addon-test:")) {
        println!("{l}");
    }
    if output.status.success() && text.contains("evani-addon-test: PASS") {
        Ok(true)
    } else {
        let tail: Vec<&str> = text.lines().rev().take(25).collect();
        Err(format!("the headless add-on test failed ({}):\n{}", output.status, tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
    }
}

pub fn cmd_blender_addon(args: &[String], root: &Path) -> Result<(), String> {
    for a in args {
        if a != "--test" {
            return Err(format!("unknown option {a}\nusage: cargo xtask blender-addon [--test]"));
        }
    }
    if args.iter().any(|a| a == "--test") {
        if test(root)? {
            println!("blender-addon: test passed");
        }
        Ok(())
    } else {
        build(root).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).map(Path::to_path_buf).unwrap_or_default()
    }

    /// The add-on installs in a headless Blender, exports a model made in the
    /// script and the converted tier files and report appear. Skips (passes
    /// with a note) without Blender; `EVANI_REQUIRE_TOOLS=1` makes that a failure.
    #[test]
    fn blender_addon_exports_and_converts_headless() {
        match test(&root()) {
            Ok(true) => {}
            Ok(false) => eprintln!("SKIPPED: Blender not installed"),
            Err(e) => panic!("{e}"),
        }
    }
}
