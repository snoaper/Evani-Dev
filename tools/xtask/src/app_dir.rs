//! Games kept outside the engine folder (`--app-dir <path>`): `build`
//! (`--ship` too), `features`, `render-paths` and `manifest` take the game's
//! app folder (the one with its `evani.toml`, `Cargo.toml` and `index.html`)
//! instead of an id under `apps/`.
//!
//! What differs from a game in `apps/<id>`, and nothing else:
//! - the game id is the `name` in its `evani.toml` (not the folder name);
//! - its kit is the `evani-kit-<kit>` dependency's folder, read from the
//!   app's `Cargo.toml` (a `path`, or `workspace = true` and the path in the
//!   game workspace's `[workspace.dependencies]`), else the engine's
//!   `kits/evani-kit-<kit>`;
//! - Cargo runs in the game's own workspace (the nearest folder above the
//!   app with a `[workspace]` Cargo.toml), so its target folder is
//!   `CARGO_TARGET_DIR` or that workspace's `target`, and the build reports
//!   go to `<game workspace>/target/evani-sizes`;
//! - the engine's `.cargo/config.toml` is not read there, so the wasm flag
//!   it sets (`--cfg=web_sys_unstable_apis`) is passed with the build;
//! - a ship build maps the engine folder to the relative paths a game in
//!   `apps/` gets (so the engine's bytes are the same) and the game
//!   workspace's folder to `/game` in the wasm's panic paths;
//! - `build-facts.json` records the game's own git commit and whether it has
//!   uncommitted changes, beside the engine's.
//!
//! A game in `apps/` builds exactly as before: none of this runs for it.

use std::path::{Path, PathBuf};

// Finding the game's workspace and kit, and the same-engine guard, are
// shared with the live bridge (`serve --app-dir`).
pub use evani_guide::app_dir::{WEB_SYS_CFG, check_engine, kit_dir, workspace_root_of};

/// Where an out-of-tree game's sources are mapped in a ship build's wasm.
pub const GAME_PREFIX: &str = "/game";

/// An out-of-tree game's folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    /// The game's Cargo workspace root (where Cargo runs and `target/` is).
    pub root: PathBuf,
    /// The kit crate's folder.
    pub kit_dir: PathBuf,
}

/// Take `--app-dir <path>` (or `--app-dir=<path>`) out of `args`.
pub fn take_arg(args: &[String]) -> Result<(Option<PathBuf>, Vec<String>), String> {
    let mut out = Vec::with_capacity(args.len());
    let mut dir = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--app-dir" {
            let v = it.next().filter(|v| !v.starts_with("--")).ok_or("--app-dir needs a folder: `--app-dir <path to the game's app folder>`")?;
            dir = Some(PathBuf::from(v));
        } else if let Some(v) = a.strip_prefix("--app-dir=") {
            if v.is_empty() {
                return Err("--app-dir needs a folder: `--app-dir <path to the game's app folder>`".into());
            }
            dir = Some(PathBuf::from(v));
        } else {
            out.push(a.clone());
        }
    }
    Ok((dir, out))
}

/// Whether `args` hold `--app-dir` (either spelling).
pub fn has_arg(args: &[String]) -> bool {
    args.iter().any(|a| a == "--app-dir" || a.starts_with("--app-dir="))
}

/// The options of `cmd` that take a value (so the value is not a game id).
pub fn value_options(cmd: &str) -> &'static [&'static str] {
    match cmd {
        "geocheck" => &["--port", "--title-s", "--race-s", "--every", "--highlights", "--sample"],
        "netlog" => &["--tier", "--out", "--page-ms", "--shots-at", "--throttle-kbps", "--chrome", "--device"],
        "review" => &["--port", "--chrome", "--main"],
        "size-report" => &["--max-file-bytes", "--max-total-bytes"],
        _ => &[],
    }
}

/// The arguments that are neither `--options` nor the value of one of
/// `values`: with `--app-dir` there must be none (the folder is the game).
pub fn positionals<'a>(args: &'a [String], values: &[&str]) -> Vec<&'a String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if values.contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with("--") {
            out.push(a);
        }
    }
    out
}

/// The `[package] name` in `<dir>/Cargo.toml`.
pub fn package_name(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?.parse::<toml::Table>().ok()?;
    Some(t.get("package")?.get("name")?.as_str()?.to_string())
}

/// Cargo's target folder for a build run in `ws_root`: `CARGO_TARGET_DIR`
/// (`target_env`; a relative one is read against `ws_root`) or
/// `<ws_root>/target`.
pub fn target_dir(ws_root: &Path, target_env: Option<&std::ffi::OsStr>) -> PathBuf {
    match target_env.filter(|v| !v.is_empty()) {
        Some(t) => ws_root.join(t),
        None => ws_root.join("target"),
    }
}

/// The extra wasm rustflags an out-of-tree build passes: the engine's cfg
/// always, and for a ship build two path mappings after the engine's own
/// (rustc uses the last that matches):
/// - the engine folder to nothing, so the engine's sources read
///   `crates\evani-core\src\...` as in a game in `apps/` (there they are
///   workspace members and Cargo passes them relative; here they are path
///   dependencies and Cargo passes them absolute). The engine's bytes in the
///   wasm are then the same inside and outside the engine folder;
/// - the game workspace to `/game` (last, so it wins for a game workspace
///   kept inside the engine folder).
///
/// A folder with whitespace in its path can't be passed (Cargo splits these
/// flags on whitespace): it is left out and returned in the second list, for
/// the build to print a note, as [`crate::ship_rustflags`] does.
pub fn extra_rustflags(ext: &External, engine_root: &Path, ship: bool) -> (Vec<String>, Vec<PathBuf>) {
    let (mut flags, mut skipped) = (vec![WEB_SYS_CFG.to_string()], Vec::new());
    if ship {
        for (from, to) in [(engine_root, ""), (ext.root.as_path(), GAME_PREFIX)] {
            let text = from.to_string_lossy();
            if text.is_empty() {
                continue;
            }
            if text.chars().any(char::is_whitespace) {
                skipped.push(from.to_path_buf());
                continue;
            }
            flags.push(format!("--remap-path-prefix={}={to}", text.trim_end_matches(['\\', '/'])));
        }
    }
    (flags, skipped)
}

/// The game's own commit and whether its folder has uncommitted changes
/// (`None` outside git).
pub fn game_commit(root: &Path) -> Option<(String, bool)> {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "--short=7", "HEAD"]).filter(|s| !s.is_empty())?;
    let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|s| !s.is_empty());
    Some((commit, dirty))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-xtask-appdir-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn w(root: &Path, p: &str, s: &str) {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s).unwrap();
    }

    #[test]
    fn option_values_are_not_game_ids() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(has_arg(&a(&["check", "--app-dir", "x"])) && has_arg(&a(&["check", "--app-dir=x"])) && !has_arg(&a(&["check", "--app", "x"])));
        assert!(positionals(&a(&["--tier", "low,ultra", "--no-build", "--out", "o"]), value_options("netlog")).is_empty());
        assert!(positionals(&a(&["--port", "8095", "--zfight", "--sample", "5"]), value_options("geocheck")).is_empty());
        assert_eq!(positionals(&a(&["--tier", "low", "match-3"]), value_options("netlog")), [&"match-3".to_string()]);
        assert!(positionals(&a(&["--port", "8095", "--main", "m1-foundation", "--rebench"]), value_options("review")).is_empty());
        assert!(positionals(&a(&["--max-file-bytes", "26214400", "--modules"]), value_options("size-report")).is_empty());
        // A command without value options: every bare word is a game id.
        assert_eq!(positionals(&a(&["--ship", "x"]), value_options("build")), [&"x".to_string()]);
        let r = temp("package");
        w(&r, "Cargo.toml", "[package]\nname = \"evani-kit-sea\"\n");
        assert_eq!(package_name(&r).as_deref(), Some("evani-kit-sea"));
        assert_eq!(package_name(&r.join("none")), None);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn app_dir_arg_is_taken_out_in_both_spellings() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (d, rest) = take_arg(&a(&["build", "--app-dir", "../game/app", "--ship"])).unwrap();
        assert_eq!((d, rest), (Some(PathBuf::from("../game/app")), a(&["build", "--ship"])));
        let (d, rest) = take_arg(&a(&["build", "--ship", "--app-dir=C:/g/app"])).unwrap();
        assert_eq!((d, rest), (Some(PathBuf::from("C:/g/app")), a(&["build", "--ship"])));
        let (d, rest) = take_arg(&a(&["build", "sandbox", "--ship"])).unwrap();
        assert_eq!((d, rest), (None, a(&["build", "sandbox", "--ship"])));
        assert!(take_arg(&a(&["build", "--app-dir"])).is_err());
        assert!(take_arg(&a(&["build", "--app-dir", "--ship"])).is_err());
        assert!(take_arg(&a(&["build", "--app-dir="])).is_err());
    }

    #[test]
    fn the_game_workspace_and_its_kit_are_found() {
        let r = temp("ws");
        w(&r, "Cargo.toml", "[workspace]\nmembers = [\"app\", \"kit\"]\n[workspace.dependencies]\nevani-kit-demo = { path = \"kit\" }\n");
        w(&r, "app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-kit-demo.workspace = true\n");
        let engine = r.join("engine");
        assert_eq!(workspace_root_of(&r.join("app")), r);
        assert_eq!(kit_dir(&r.join("app"), &r, "demo", &engine).unwrap(), r.join("kit"));
        // A path dependency in the app itself.
        w(&r, "app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-kit-demo = { path = \"../kits/demo\" }\n");
        assert_eq!(kit_dir(&r.join("app"), &r, "demo", &engine).unwrap(), r.join("app").join("../kits/demo"));
        // Not listed: the engine's own kit.
        assert_eq!(kit_dir(&r.join("app"), &r, "other", &engine).unwrap(), engine.join("kits").join("evani-kit-other"));
        // Listed, but from git or a registry: an error with a fix, not the engine's kit.
        w(&r, "app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-kit-demo = { git = \"https://example.invalid/kit\" }\n");
        let e = kit_dir(&r.join("app"), &r, "demo", &engine).unwrap_err();
        assert!(e.starts_with("ERROR: ") && e.contains("not a path dependency") && e.contains("fix:"), "{e}");
        w(&r, "app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-kit-demo = \"0.1\"\n");
        assert!(kit_dir(&r.join("app"), &r, "demo", &engine).unwrap_err().starts_with("ERROR: "));
        // `workspace = true` with no workspace entry.
        w(&r, "app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-kit-gone.workspace = true\n");
        let e = kit_dir(&r.join("app"), &r, "gone", &engine).unwrap_err();
        assert!(e.starts_with("ERROR: ") && e.contains("[workspace.dependencies]"), "{e}");
        // No workspace above: the app folder is its own root.
        let lone = temp("lone");
        w(&lone, "app/Cargo.toml", "[package]\nname = \"g\"\n");
        assert_eq!(workspace_root_of(&lone.join("app")), lone.join("app"));
        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&lone);
    }

    #[test]
    fn target_folder_follows_cargo() {
        let ws = Path::new("C:/games/demo");
        assert_eq!(target_dir(ws, None), ws.join("target"));
        assert_eq!(target_dir(ws, Some(std::ffi::OsStr::new(""))), ws.join("target"));
        assert_eq!(target_dir(ws, Some(std::ffi::OsStr::new("E:/shared"))), PathBuf::from("E:/shared"));
        assert_eq!(target_dir(ws, Some(std::ffi::OsStr::new("out"))), ws.join("out"));
    }

    #[test]
    fn out_of_tree_flags_carry_the_cfg_and_map_the_game_folder() {
        let ext = External {
            root: PathBuf::from("C:\\games\\demo\\"),
            kit_dir: PathBuf::from("C:\\games\\demo\\kit"),
        };
        let engine = Path::new("D:\\w\\evani\\");
        assert_eq!(extra_rustflags(&ext, engine, false), (vec![WEB_SYS_CFG.to_string()], vec![]));
        assert_eq!(
            extra_rustflags(&ext, engine, true),
            (
                vec![
                    WEB_SYS_CFG.to_string(),
                    "--remap-path-prefix=D:\\w\\evani=".to_string(),
                    "--remap-path-prefix=C:\\games\\demo=/game".to_string()
                ],
                vec![]
            )
        );
        let spaced = External {
            root: PathBuf::from("C:\\my games\\demo"),
            kit_dir: PathBuf::new(),
        };
        // Folders with a space are left out and returned, for the build's note.
        assert_eq!(
            extra_rustflags(&spaced, Path::new("D:\\my engine"), true),
            (vec![WEB_SYS_CFG.to_string()], vec![PathBuf::from("D:\\my engine"), PathBuf::from("C:\\my games\\demo")])
        );
        assert_eq!(
            extra_rustflags(&spaced, engine, true),
            (vec![WEB_SYS_CFG.to_string(), "--remap-path-prefix=D:\\w\\evani=".to_string()], vec![PathBuf::from("C:\\my games\\demo")])
        );
    }

    #[test]
    fn the_game_must_build_against_the_engine_running_xtask() {
        let r = temp("engine");
        let (mine, other) = (r.join("mine"), r.join("other"));
        w(&mine, "crates/evani-core/Cargo.toml", "[package]\nname = \"evani-core\"\n");
        w(&other, "crates/evani-core/Cargo.toml", "[package]\nname = \"evani-core\"\n");
        let game = r.join("game");
        let ws = |core: &Path| format!("[workspace]\nmembers = [\"app\"]\n[workspace.dependencies]\nevani-core = {{ path = '{}' }}\n", core.display());
        w(&game, "app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-core.workspace = true\n");
        w(&game, "Cargo.toml", &ws(&mine.join("crates").join("evani-core")));
        check_engine(&game.join("app"), &game, &mine).unwrap();
        // Another engine's evani-core: refused, and told to use that engine's xtask.
        w(&game, "Cargo.toml", &ws(&other.join("crates").join("evani-core")));
        let e = check_engine(&game.join("app"), &game, &mine).unwrap_err();
        assert!(e.contains("ERROR") && e.contains("fix: run that engine's own xtask") && e.contains(&other.display().to_string()), "{e}");
        // Not listed by the app itself: the workspace's entry still decides.
        w(&game, "app/Cargo.toml", "[package]\nname = \"g\"\n");
        assert!(check_engine(&game.join("app"), &game, &mine).is_err());
        w(&game, "Cargo.toml", &ws(&mine.join("crates").join("evani-core")));
        check_engine(&game.join("app"), &game, &mine).unwrap();
        // No evani-core anywhere: can't tell, so refused with a fix.
        w(&game, "Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        let e = check_engine(&game.join("app"), &game, &mine).unwrap_err();
        assert!(e.contains("can't tell which engine") && e.contains("fix:"), "{e}");
        let _ = std::fs::remove_dir_all(&r);
    }
}
