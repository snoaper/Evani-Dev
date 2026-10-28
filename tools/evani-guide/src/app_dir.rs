//! Finding a game kept outside the engine folder (`--app-dir <path>`): its
//! Cargo workspace, its kit crate, and the guard that it builds against the
//! engine whose tools are running. Shared by xtask (build, check, ..) and the
//! live bridge (`serve --app-dir`), so both read a game's folders the same way.

use std::path::{Path, PathBuf};

/// The wasm cfg the engine's `.cargo/config.toml` sets for its own builds
/// (web-sys gates the WebGPU bindings behind it). A build run in a game's
/// own workspace doesn't read that file, so the tool passes it.
pub const WEB_SYS_CFG: &str = "--cfg=web_sys_unstable_apis";

/// The environment variable Cargo reads as extra wasm rustflags (merged
/// with any `.cargo/config.toml` list; Cargo splits it on whitespace).
pub const WASM_RUSTFLAGS_ENV: &str = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";

/// A game outside the engine, resolved from its app folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outside {
    /// The game id: the `name` in its `evani.toml`.
    pub id: String,
    /// The app folder (absolute): `evani.toml`, `Cargo.toml`, `index.html`.
    pub dir: PathBuf,
    /// The game's Cargo workspace root (where Cargo runs and `target/` is).
    pub root: PathBuf,
    /// The kit crate's folder.
    pub kit_dir: PathBuf,
}

/// Resolve the app folder `dir` against the engine at `engine_root`: its id
/// (a valid game id), workspace, kit, and the same-engine guard
/// ([`check_engine`]). Every refusal is an `ERROR:` line with a `fix:`.
pub fn resolve(dir: &Path, engine_root: &Path) -> Result<Outside, String> {
    let dir = std::path::absolute(dir).map_err(|e| format!("ERROR: {}: {e}", dir.display()))?;
    let toml_path = dir.join("evani.toml");
    let text = std::fs::read_to_string(&toml_path)
        .map_err(|e| format!("ERROR: {}: {e}\n  fix: --app-dir takes the game's app folder, the one with its evani.toml", toml_path.display()))?;
    let m = crate::manifest::Modules::parse(&text).map_err(|e| format!("ERROR: {}: {e}", toml_path.display()))?;
    if !crate::scaffold::valid_id(&m.name) {
        return Err(format!(
            "ERROR: {}: name `{}` is not a game id\n  fix: use lowercase letters, digits and single `-`",
            toml_path.display(),
            m.name
        ));
    }
    let root = workspace_root_of(&dir);
    check_engine(&dir, &root, engine_root)?;
    let kit_dir = kit_dir(&dir, &root, &m.kit, engine_root)?;
    Ok(Outside { id: m.name, dir, root, kit_dir })
}

/// The nearest folder at or above `app_dir` whose `Cargo.toml` has a
/// `[workspace]` table; `app_dir` itself when there is none.
pub fn workspace_root_of(app_dir: &Path) -> PathBuf {
    for d in app_dir.ancestors() {
        let Ok(text) = std::fs::read_to_string(d.join("Cargo.toml")) else { continue };
        if text.parse::<toml::Table>().is_ok_and(|t| t.contains_key("workspace")) {
            return d.to_path_buf();
        }
    }
    app_dir.to_path_buf()
}

/// Where the app's dependency `name` lives: its folder for a `path`
/// dependency in the app's `Cargo.toml` (relative to the app), or for
/// `workspace = true` its `path` in `<ws_root>/Cargo.toml`
/// `[workspace.dependencies]` (relative to that root). `Ok(None)` when the
/// app doesn't list it; an error when it is listed but not by path (a git,
/// registry or version dependency), since the tools then can't read its folder.
pub fn dep_path(app_dir: &Path, ws_root: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let read = |p: &Path| std::fs::read_to_string(p).ok().and_then(|t| t.parse::<toml::Table>().ok());
    let not_path = |file: &Path| {
        format!(
            "ERROR: {}: `{name}` is not a path dependency (git, registry or version), so xtask can't read its folder\n  fix: depend on it by path, `{name} = {{ path = \"<engine>/...\" }}` (or through `[workspace.dependencies]`)",
            file.display()
        )
    };
    let Some(app) = read(&app_dir.join("Cargo.toml")) else { return Ok(None) };
    let Some(dep) = app.get("dependencies").and_then(|d| d.get(name)) else { return Ok(None) };
    if let Some(p) = dep.get("path").and_then(toml::Value::as_str) {
        return Ok(Some(app_dir.join(p)));
    }
    if dep.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        let ws_toml = ws_root.join("Cargo.toml");
        let ws_dep = read(&ws_toml).and_then(|ws| ws.get("workspace")?.get("dependencies")?.get(name).cloned());
        return match ws_dep {
            Some(d) => match d.get("path").and_then(toml::Value::as_str) {
                Some(p) => Ok(Some(ws_root.join(p))),
                None => Err(not_path(&ws_toml)),
            },
            None => Err(format!(
                "ERROR: {}: `{name}` says `workspace = true` but {} has no `[workspace.dependencies] {name}`\n  fix: add `{name} = {{ path = \"<engine>/...\" }}` to the game workspace's `[workspace.dependencies]`",
                app_dir.join("Cargo.toml").display(),
                ws_toml.display()
            )),
        };
    }
    Err(not_path(&app_dir.join("Cargo.toml")))
}

/// The kit crate's folder for kit `kit`: the `evani-kit-<kit>` dependency's
/// folder ([`dep_path`]), else the engine's `kits/evani-kit-<kit>`. A kit
/// listed but not by path is an error, not a silent fall-back to the engine's.
pub fn kit_dir(app_dir: &Path, ws_root: &Path, kit: &str, engine_root: &Path) -> Result<PathBuf, String> {
    let name = format!("evani-kit-{kit}");
    Ok(dep_path(app_dir, ws_root, &name)?.unwrap_or_else(|| engine_root.join("kits").join(name)))
}

/// The engine a game outside the engine folder builds against must be the
/// one running the tool: build facts record this engine's commit, and the
/// ship and playground guards (and the live bridge's build) are this
/// engine's. The game's `evani-core` (listed in the app, else in the game
/// workspace's `[workspace.dependencies]`) has to be inside `engine_root`.
pub fn check_engine(app_dir: &Path, ws_root: &Path, engine_root: &Path) -> Result<(), String> {
    let ws_core = || -> Option<PathBuf> {
        let t = std::fs::read_to_string(ws_root.join("Cargo.toml")).ok()?.parse::<toml::Table>().ok()?;
        let p = t.get("workspace")?.get("dependencies")?.get("evani-core")?.get("path")?.as_str()?.to_string();
        Some(ws_root.join(p))
    };
    let Some(core) = dep_path(app_dir, ws_root, "evani-core")?.or_else(ws_core) else {
        return Err(format!(
            "ERROR: can't tell which engine {} builds against (no path dependency on evani-core in it or in {})\n  fix: list `evani-core = {{ path = \"<engine>/crates/evani-core\" }}` in the game workspace's `[workspace.dependencies]`",
            app_dir.display(),
            ws_root.join("Cargo.toml").display()
        ));
    };
    let canon = |p: &Path| {
        std::fs::canonicalize(p).map_err(|e| format!("ERROR: {}: {e}\n  fix: point the game's evani-core path dependency at an engine folder that exists", p.display()))
    };
    if canon(&core)?.starts_with(canon(engine_root)?) {
        return Ok(());
    }
    let other = core.ancestors().nth(2).unwrap_or(&core);
    Err(format!(
        "ERROR: {} builds against the engine at {} (its evani-core is {}), not the engine running this xtask ({}); the build facts and guards would describe the wrong engine\n  fix: run that engine's own xtask: from {} run `cargo xtask build --app-dir {}`",
        app_dir.display(),
        other.display(),
        core.display(),
        engine_root.display(),
        other.display(),
        app_dir.display()
    ))
}

/// `path` canonicalised, if it exists and is inside `base` (canonicalised
/// too): what a server may hand out from `base`. A `..` that slipped through,
/// a symlink or a junction leading out of `base` gives `None`.
pub fn contained(base: &Path, path: &Path) -> Option<PathBuf> {
    let base = std::fs::canonicalize(base).ok()?;
    let p = std::fs::canonicalize(path).ok()?;
    p.starts_with(&base).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(root: &Path, p: &str, s: &str) {
        crate::write_file(&root.join(p), s).unwrap();
    }

    #[test]
    fn an_outside_game_resolves_against_its_engine() {
        let base = crate::testutil::temp_root("appdir-resolve");
        let engine = base.join("engine");
        w(&engine, "crates/evani-core/Cargo.toml", "[package]\nname = \"evani-core\"\n");
        let ws = format!("[workspace]\nmembers = [\"app\"]\n[workspace.dependencies]\nevani-core = {{ path = \"{}\" }}\nevani-kit-sea = {{ path = \"kit\" }}\n", engine.join("crates/evani-core").display().to_string().replace('\\', "/"));
        w(&base, "game/Cargo.toml", &ws);
        w(&base, "game/app/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\nevani-core.workspace = true\nevani-kit-sea.workspace = true\n");
        w(&base, "game/app/evani.toml", "name = \"sea-game\"\nkit = \"sea\"\nlook = \"basic\"\n");
        let o = resolve(&base.join("game/app"), &engine).unwrap();
        assert_eq!((o.id.as_str(), o.root.clone(), o.kit_dir.clone()), ("sea-game", base.join("game"), base.join("game").join("kit")));
        // Not a game id, no evani.toml, another engine: ERROR lines with a fix.
        w(&base, "game/app/evani.toml", "name = \"Sea Game\"\nkit = \"sea\"\nlook = \"basic\"\n");
        let e = resolve(&base.join("game/app"), &engine).unwrap_err();
        assert!(e.starts_with("ERROR: ") && e.contains("not a game id") && e.contains("fix:"), "{e}");
        let e = resolve(&base.join("game"), &engine).unwrap_err();
        assert!(e.starts_with("ERROR: ") && e.contains("--app-dir takes the game's app folder"), "{e}");
        w(&base, "game/app/evani.toml", "name = \"sea-game\"\nkit = \"sea\"\nlook = \"basic\"\n");
        let other = base.join("other");
        w(&other, "crates/evani-core/Cargo.toml", "[package]\nname = \"evani-core\"\n");
        let e = resolve(&base.join("game/app"), &other).unwrap_err();
        assert!(e.starts_with("ERROR: ") && e.contains("not the engine running"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn contained_refuses_what_leads_out() {
        let base = crate::testutil::temp_root("appdir-contained");
        w(&base, "dist/index.html", "<p>");
        w(&base, "secret.txt", "no");
        let dist = base.join("dist");
        assert!(contained(&dist, &dist.join("index.html")).is_some());
        assert!(contained(&dist, &dist.join("..").join("secret.txt")).is_none(), "a .. out of the folder");
        assert!(contained(&dist, &dist.join("missing.js")).is_none());
        // A junction (Windows) or symlink (elsewhere) inside dist that points out.
        let link = dist.join("out");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&base).output().is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&base, &link).is_ok();
        assert!(made, "could not make the test link");
        assert!(link.join("secret.txt").is_file(), "the link leads out");
        assert!(contained(&dist, &link.join("secret.txt")).is_none(), "a junction out of the folder");
        #[cfg(windows)]
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&base);
    }
}
