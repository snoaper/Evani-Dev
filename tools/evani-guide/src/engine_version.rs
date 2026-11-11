//! The engine version a game is made with (`[engine] version` in its
//! `evani.toml`), the engine packs installed beside the running one, and the
//! rule for which old packs stay on disk (docs/ENGINE_PACK.md 4.4 and 4.5).
//!
//! - **Pin.** `[engine] version = "<pack_version>"`. `new-game --app-dir`
//!   writes it; a game without one is pinned to the engine that first builds
//!   it. A game is only ever built and checked by the engine it pins; moving
//!   it to another engine is `upgrade-engine` (a copy is built and checked
//!   first, and the pin changes only when that passes). Never silently.
//! - **Running version.** `pack_version` in the engine's
//!   `packaging/engine-pack.json` (the same number the pack's manifest
//!   carries), so a checkout and its pack agree.
//! - **Installed packs.** The folders beside the running pack (or
//!   `EVANI_ENGINES_DIR`), each with a `manifest.json`. Evani.App names them
//!   with its disk clean-up classes: `.evani-build-evani-dev-engine-<v>` (the
//!   live pack, marked by a `.evani-live` file, and old unpinned packs) and
//!   `.evani-keep-evani-dev-engine-<v>` (old packs a game still pins).
//! - **Retention.** The live pack plus every pack a known game pins, at most
//!   [`MAX_KEPT`] besides the live one; a pack is never deleted here, only
//!   renamed between the two classes, and over the cap the user is asked.
//! - **Relink.** A game outside the engine names engine crates by absolute
//!   path; renaming a pack folder (or moving the game to another PC) moves
//!   them, so [`relink`] points them at the running engine again. Only the
//!   path values of engine crates change.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::check::Severity;

/// Points at the folder that holds the installed engine packs (default: the
/// folder above the running pack).
pub const ENGINES_DIR_VAR: &str = "EVANI_ENGINES_DIR";
/// Folder-name prefix of the live pack and of old unpinned packs (Evani.App's
/// "build" clean-up class: the live one and the newest two are kept).
pub const BUILD_PREFIX: &str = ".evani-build-evani-dev-engine-";
/// Folder-name prefix of an old pack a game still pins (the "keep" class:
/// never offered for deletion).
pub const KEEP_PREFIX: &str = ".evani-keep-evani-dev-engine-";
/// The marker file in the live pack's folder (Evani.App's `LIVE_MARKER`).
pub const LIVE_MARKER: &str = ".evani-live";
/// Old packs kept for pinned games, besides the live one (owner decision).
pub const MAX_KEPT: usize = 3;
/// The pins file inside an engine (its `pack_version`).
pub const PINS_FILE: &str = "packaging/engine-pack.json";

/// A pack version: `<major>.<minor>.<patch>` with an optional `-<pre>`
/// (lowercase letters, digits and `.`).
pub fn valid_version(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let nums: Vec<&str> = core.split('.').collect();
    nums.len() == 3
        && nums.iter().all(|n| !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.'))
}

/// Order two pack versions: numbers first; a pre-release sorts before its release.
pub fn compare(a: &str, b: &str) -> Ordering {
    let split = |v: &str| -> (Vec<u64>, Option<String>) {
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v, None),
        };
        (core.split('.').map(|n| n.parse().unwrap_or(0)).collect(), pre)
    };
    let (an, ap) = split(a);
    let (bn, bp) = split(b);
    an.cmp(&bn).then_with(|| match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    })
}

/// The `[engine] version` of an `evani.toml` (`None` when there is no pin).
pub fn pin(evani_toml: &str) -> Result<Option<String>, String> {
    let t: toml::Table = evani_toml.parse().map_err(|e| format!("evani.toml: {e}"))?;
    let Some(engine) = t.get("engine") else { return Ok(None) };
    let engine = engine.as_table().ok_or("evani.toml: `[engine]` must be a table")?;
    for k in engine.keys() {
        if k != "version" {
            return Err(format!("evani.toml: unknown key `engine.{k}` (only `version`)"));
        }
    }
    let Some(v) = engine.get("version") else { return Ok(None) };
    let v = v.as_str().ok_or("evani.toml: `engine.version` must be a string like \"0.1.0\"")?;
    if !valid_version(v) {
        return Err(format!("evani.toml: `engine.version = \"{v}\"` is not an engine version (like \"0.1.0\")"));
    }
    Ok(Some(v.to_string()))
}

/// `evani_toml` with its pin set to `version`: the `version` line of an
/// existing `[engine]` table is replaced (or added under its header);
/// otherwise the table is appended. Everything else is kept as written.
pub fn set_pin(evani_toml: &str, version: &str) -> String {
    let nl = if evani_toml.contains("\r\n") { "\r\n" } else { "\n" };
    let line = format!("version = \"{version}\"");
    let mut lines: Vec<String> = evani_toml.lines().map(str::to_string).collect();
    if let Some(h) = lines.iter().position(|l| l.trim() == "[engine]") {
        let end = (h + 1..lines.len()).find(|&i| lines[i].trim_start().starts_with('[')).unwrap_or(lines.len());
        match (h + 1..end).find(|&i| lines[i].trim_start().starts_with("version")) {
            Some(i) => lines[i] = line,
            None => lines.insert(h + 1, line),
        }
        let mut s = lines.join(nl);
        s.push_str(nl);
        return s;
    }
    let mut s = evani_toml.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push_str(nl);
    }
    s.push_str(&format!("{nl}# The engine this game is made with. It is built only with this engine;{nl}# `upgrade-engine` moves it to a newer one after a checked test build.{nl}[engine]{nl}{line}{nl}"));
    s
}

/// The version of the engine at `root`: `pack_version` of its pins file,
/// else this crate's version.
pub fn running(root: &Path) -> String {
    std::fs::read_to_string(root.join(PINS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("pack_version").and_then(serde_json::Value::as_str).map(str::to_string))
        .unwrap_or_else(|| crate::ENGINE_VERSION.to_string())
}

/// What `evani --version` prints for the engine at `root`: `evani <version>`,
/// the same version `running` gives (and `new-game` pins a game to).
pub fn version_line(root: &Path) -> String {
    format!("evani {}", running(root))
}

/// The pack folder when the engine at `root` is a pack's `engine/` (a
/// `manifest.json` beside it); `None` for a checkout.
pub fn pack_dir_of(root: &Path) -> Option<PathBuf> {
    let parent = root.parent()?;
    (root.file_name().is_some_and(|n| n == "engine") && parent.join("manifest.json").is_file()).then(|| parent.to_path_buf())
}

/// The folder of installed packs: `EVANI_ENGINES_DIR`, else the one above
/// the running pack.
pub fn engines_dir(root: &Path) -> Option<PathBuf> {
    engines_dir_from(root, std::env::var_os(ENGINES_DIR_VAR).as_deref())
}

pub fn engines_dir_from(root: &Path, env: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    match env.filter(|v| !v.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => pack_dir_of(root)?.parent().map(Path::to_path_buf),
    }
}

/// Which clean-up class a pack folder's name puts it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Build,
    Keep,
    /// Any other name (the packer's own `<version>/<platform>` output, a test copy).
    Other,
}

/// One installed pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    /// The pack folder (`manifest.json`, `engine/`, `tools/`, ...).
    pub dir: PathBuf,
    pub class: Class,
    /// It holds the `.evani-live` marker.
    pub live: bool,
}

impl Installed {
    /// The engine folder inside the pack.
    pub fn engine(&self) -> PathBuf {
        self.dir.join("engine")
    }
}

fn class_of(name: &str) -> Class {
    if name.starts_with(BUILD_PREFIX) {
        Class::Build
    } else if name.starts_with(KEEP_PREFIX) {
        Class::Keep
    } else {
        Class::Other
    }
}

fn manifest_version(dir: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    Some(v.get("pack_version")?.as_str()?.to_string())
}

/// The packs in `engines` (each child folder with a `manifest.json`), oldest
/// version first. The version is the manifest's, not the folder name's.
pub fn installed(engines: &Path) -> Vec<Installed> {
    let mut out: Vec<Installed> = std::fs::read_dir(engines)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir = e.path();
            let version = manifest_version(&dir)?;
            Some(Installed { class: class_of(&e.file_name().to_string_lossy()), live: dir.join(LIVE_MARKER).is_file(), version, dir })
        })
        .collect();
    out.sort_by(|a, b| compare(&a.version, &b.version).then_with(|| a.dir.cmp(&b.dir)));
    out
}

/// What `check` says about a game's pin, for the engine at `root`:
/// `(severity, what, fix)`. `outside` is a game kept outside the engine
/// folder; `shown`/`arg` name it as messages and commands do.
pub fn pin_problems(root: &Path, evani_toml: &str, outside: bool, shown: &str, arg: &str) -> Vec<(Severity, String, String)> {
    let here = running(root);
    let set = format!("Add `[engine]` with `version = \"{here}\"` to {shown}/evani.toml");
    match pin(evani_toml) {
        Err(e) => vec![(Severity::Error, e, format!("{set} (an engine version like \"0.1.0\")."))],
        Ok(None) if !outside => Vec::new(),
        Ok(None) => {
            let sev = if pack_dir_of(root).is_some() { Severity::Warn } else { Severity::Note };
            vec![(sev, format!("not pinned to an engine version; it would build with whichever engine runs (this is {here})"), format!("{set}, or build it once with this engine (`cargo xtask build {arg}` pins it)."))]
        }
        Ok(Some(v)) if v == here => Vec::new(),
        Ok(Some(v)) => {
            let others = engines_dir(root).map(|d| installed(&d)).unwrap_or_default();
            let upgrade = format!("move it to engine {here} with `cargo xtask upgrade-engine {arg}` (it builds and checks a copy first and changes the pin only when that passes)");
            match others.iter().find(|p| p.version == v) {
                Some(p) => vec![(
                    Severity::Error,
                    format!("pinned to engine {v}, but this is engine {here}; a game is built and checked only by the engine it pins"),
                    format!("Run engine {v}'s own tools ({}), or {upgrade}.", p.dir.join("tools").join("bin").join("evani").display()),
                )],
                None => {
                    let have: Vec<&str> = others.iter().map(|p| p.version.as_str()).collect();
                    let have = if have.is_empty() { here.clone() } else { have.join(", ") };
                    vec![(Severity::Error, format!("pinned to engine {v}, which is not installed (installed: {have})"), format!("Upgrade: {upgrade}. Installing the Evani.App release that carried engine {v} also brings it back."))]
                }
            }
        }
    }
}

/// The refusal a build or check gives when the game pins another engine
/// (`None` when it may run here). Same words as [`pin_problems`].
pub fn refuse_other_engine(root: &Path, evani_toml: &str, shown: &str, arg: &str) -> Option<String> {
    pin_problems(root, evani_toml, true, shown, arg)
        .into_iter()
        .find(|(s, _, _)| *s == Severity::Error)
        .map(|(_, what, fix)| format!("ERROR: {shown}: {what}\n  fix: {fix}"))
}

// ---- relink -------------------------------------------------------------------

/// The engine's own `[workspace.dependencies]` that are path dependencies:
/// crate name -> its folder relative to the engine root.
pub fn engine_crates(engine_root: &Path) -> Result<BTreeMap<String, String>, String> {
    let p = engine_root.join("Cargo.toml");
    let t: toml::Table = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?.parse().map_err(|e| format!("{}: {e}", p.display()))?;
    let deps = t.get("workspace").and_then(|w| w.get("dependencies")).and_then(toml::Value::as_table).ok_or_else(|| format!("{}: no [workspace.dependencies]", p.display()))?;
    Ok(deps.iter().filter_map(|(k, v)| Some((k.clone(), v.get("path")?.as_str()?.to_string()))).collect())
}

/// Dependency tables whose path values [`relink_text`] may change.
const DEP_TABLES: &[&str] = &["[workspace.dependencies]", "[dependencies]", "[dev-dependencies]", "[build-dependencies]"];

/// `cargo_toml` with the `path` of every engine crate in `crates` (one-line
/// entries in a dependency table) set to `<engine_root>/<rel>`. Returns the
/// new text and the crates changed. Entries that point inside the game (a
/// relative path that isn't an engine crate's) are left alone.
pub fn relink_text(cargo_toml: &str, crates: &BTreeMap<String, String>, engine_root: &Path) -> (String, Vec<String>) {
    let nl = if cargo_toml.contains("\r\n") { "\r\n" } else { "\n" };
    let base = engine_root.to_string_lossy().replace('\\', "/");
    let base = base.trim_end_matches('/');
    let mut in_deps = false;
    let mut changed = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for line in cargo_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = DEP_TABLES.contains(&t);
        }
        let name = t.split(['=', ' ', '.']).next().unwrap_or("").trim_matches('"');
        let rel = crates.get(name);
        let (Some(rel), true) = (rel, in_deps) else {
            out.push(line.to_string());
            continue;
        };
        let Some(at) = line.find("path = \"") else {
            out.push(line.to_string());
            continue;
        };
        let start = at + "path = \"".len();
        let Some(len) = line[start..].find('"') else {
            out.push(line.to_string());
            continue;
        };
        let old = &line[start..start + len];
        // A relative path is the game's own copy of the crate (a kit kept in
        // the game), never the engine's.
        if !Path::new(old).is_absolute() {
            out.push(line.to_string());
            continue;
        }
        let new = format!("{base}/{rel}");
        if old != new {
            changed.push(name.to_string());
        }
        out.push(format!("{}{new}{}", &line[..start], &line[start + len..]));
    }
    let mut s = out.join(nl);
    if cargo_toml.ends_with('\n') {
        s.push_str(nl);
    }
    (s, changed)
}

/// Point the game at `app_dir` (and its workspace) at the engine at
/// `engine_root`: rewrites the absolute paths of engine crates in the
/// workspace's and the app's `Cargo.toml`. Refused when the game pins another
/// engine version. Returns the files changed.
pub fn relink(app_dir: &Path, engine_root: &Path) -> Result<Vec<PathBuf>, String> {
    let evani_toml = std::fs::read_to_string(app_dir.join("evani.toml")).map_err(|e| format!("{}: {e}", app_dir.join("evani.toml").display()))?;
    let here = running(engine_root);
    if let Some(v) = pin(&evani_toml)?
        && v != here
    {
        return Err(format!(
            "ERROR: {} is pinned to engine {v}; this is engine {here}, so relinking would move it to another engine\n  fix: relink with engine {v}'s own tools, or use `upgrade-engine`",
            app_dir.display()
        ));
    }
    let crates = engine_crates(engine_root)?;
    let ws = crate::app_dir::workspace_root_of(app_dir);
    let mut files = vec![ws.join("Cargo.toml")];
    if ws != app_dir {
        files.push(app_dir.join("Cargo.toml"));
    }
    let mut changed = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let (new, names) = relink_text(&text, &crates, engine_root);
        if !names.is_empty() {
            std::fs::write(&f, new).map_err(|e| format!("{}: {e}", f.display()))?;
            changed.push(f);
        }
    }
    Ok(changed)
}

// ---- games and retention ----------------------------------------------------

/// A game found under a folder: its app folder and pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGame {
    pub app_dir: PathBuf,
    pub pin: Option<String>,
}

/// Every game (`evani.toml` with a `name` and `kit`) at most `depth` folders
/// under each of `roots`. Build output, git and engine-pack folders are skipped.
pub fn find_games(roots: &[PathBuf], depth: usize) -> Vec<FoundGame> {
    fn walk(d: &Path, left: usize, out: &mut Vec<FoundGame>) {
        let toml = d.join("evani.toml");
        if let Ok(text) = std::fs::read_to_string(&toml)
            && let Ok(t) = text.parse::<toml::Table>()
            && t.contains_key("name")
            && t.contains_key("kit")
        {
            out.push(FoundGame { app_dir: d.to_path_buf(), pin: pin(&text).ok().flatten() });
        }
        if left == 0 || d.join("manifest.json").is_file() && d.join("engine").is_dir() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut kids: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                !matches!(n.as_ref(), "target" | "dist" | "node_modules" | ".git" | "vendor") && !n.starts_with(".evani")
            })
            .map(|e| e.path())
            .collect();
        kids.sort();
        for k in kids {
            walk(&k, left - 1, out);
        }
    }
    let mut out = Vec::new();
    for r in roots {
        walk(r, depth, &mut out);
    }
    out
}

/// One step of a retention plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Rename a pack folder (to move it between the build and keep classes).
    Rename { version: String, from: PathBuf, to: PathBuf, why: String },
    /// Put the `.evani-live` marker in this pack (and take it out of the others).
    MarkLive { version: String, dir: PathBuf },
    /// Two folders hold the same version: left as they are, for a person to look at.
    Duplicate { version: String, dirs: Vec<PathBuf> },
}

/// What [`plan`] decides.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub live: Option<String>,
    pub steps: Vec<Step>,
    /// Old packs kept for games (oldest first) and the games pinning each.
    pub kept: Vec<(String, Vec<PathBuf>)>,
    /// More than [`MAX_KEPT`] old packs are pinned: ask the user which game to
    /// upgrade (or which old engine to remove). Nothing is deleted here.
    pub over_cap: bool,
    /// Pins naming a version that isn't installed: version -> games.
    pub missing: Vec<(String, Vec<PathBuf>)>,
}

/// The retention plan for `packs`, given the games found and the live
/// version (`live`, else the pack with the marker, else the newest).
pub fn plan(packs: &[Installed], games: &[FoundGame], live: Option<&str>) -> Plan {
    let mut p = Plan::default();
    let live = live.map(str::to_string).or_else(|| packs.iter().find(|x| x.live).map(|x| x.version.clone())).or_else(|| packs.last().map(|x| x.version.clone()));
    let mut pins: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for g in games {
        if let Some(v) = &g.pin {
            pins.entry(v.clone()).or_default().push(g.app_dir.clone());
        }
    }
    let mut by_version: BTreeMap<String, Vec<&Installed>> = BTreeMap::new();
    for x in packs {
        by_version.entry(x.version.clone()).or_default().push(x);
    }
    for (v, list) in &by_version {
        if list.len() > 1 {
            p.steps.push(Step::Duplicate { version: v.clone(), dirs: list.iter().map(|x| x.dir.clone()).collect() });
            continue;
        }
        let x = list[0];
        let is_live = live.as_deref() == Some(v.as_str());
        let pinned = pins.get(v);
        let (prefix, why) = if is_live {
            (BUILD_PREFIX, "the live engine".to_string())
        } else if let Some(g) = pinned {
            (KEEP_PREFIX, format!("pinned by {} game(s)", g.len()))
        } else {
            (BUILD_PREFIX, "no game pins it (the normal clean-up may offer it)".to_string())
        };
        if !is_live && let Some(g) = pinned {
            p.kept.push((v.clone(), g.clone()));
        }
        let want = format!("{prefix}{v}");
        let name = x.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if x.class != Class::Other && name != want {
            p.steps.push(Step::Rename { version: v.clone(), from: x.dir.clone(), to: x.dir.with_file_name(&want), why });
        }
        if is_live && !x.live {
            p.steps.push(Step::MarkLive { version: v.clone(), dir: x.dir.clone() });
        }
    }
    p.kept.sort_by(|a, b| compare(&a.0, &b.0));
    p.over_cap = p.kept.len() > MAX_KEPT;
    p.missing = pins.into_iter().filter(|(v, _)| !by_version.contains_key(v)).collect();
    p.live = live;
    p
}

/// Carry out `plan` in `engines`: renames, the live marker, then relink every
/// game pinning a renamed pack (its engine paths moved). Never deletes a
/// pack. Returns one line per thing done or refused.
pub fn apply(engines: &Path, plan: &Plan, games: &[FoundGame]) -> Vec<String> {
    let mut log = Vec::new();
    for s in &plan.steps {
        match s {
            Step::Rename { version, from, to, why } => {
                if to.exists() {
                    log.push(format!("refused: {} -> {}: the new name is taken", from.display(), to.display()));
                    continue;
                }
                match std::fs::rename(from, to) {
                    Ok(()) => {
                        log.push(format!("renamed engine {version}: {} -> {} ({why})", from.display(), to.display()));
                        for g in games.iter().filter(|g| g.pin.as_deref() == Some(version.as_str())) {
                            match relink(&g.app_dir, &to.join("engine")) {
                                Ok(f) if f.is_empty() => {}
                                Ok(_) => log.push(format!("relinked {} to {}", g.app_dir.display(), to.join("engine").display())),
                                Err(e) => log.push(format!("relink failed for {}: {e}", g.app_dir.display())),
                            }
                        }
                    }
                    Err(e) => log.push(format!("refused: rename {} -> {}: {e} (in use? close what runs from it and try again)", from.display(), to.display())),
                }
            }
            Step::MarkLive { version, dir } => {
                for x in installed(engines) {
                    if x.live && x.version != *version {
                        let _ = std::fs::remove_file(x.dir.join(LIVE_MARKER));
                    }
                }
                // The folder may have been renamed by a step above.
                let target = installed(engines).into_iter().find(|x| x.version == *version).map_or_else(|| dir.clone(), |x| x.dir);
                match std::fs::write(target.join(LIVE_MARKER), "") {
                    Ok(()) => log.push(format!("marked engine {version} live ({})", target.display())),
                    Err(e) => log.push(format!("refused: live marker in {}: {e}", target.display())),
                }
            }
            Step::Duplicate { version, dirs } => {
                log.push(format!("left alone: engine {version} is in {} folders ({})", dirs.len(), dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")));
            }
        }
    }
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(root: &Path, p: &str, s: &str) {
        crate::write_file(&root.join(p), s).unwrap();
    }

    fn pack(engines: &Path, name: &str, v: &str) -> PathBuf {
        let d = engines.join(name);
        w(&d, "manifest.json", &format!("{{\"pack_version\":\"{v}\"}}"));
        w(&d, "engine/packaging/engine-pack.json", &format!("{{\"pack_version\":\"{v}\"}}"));
        w(&d, "engine/Cargo.toml", "[workspace]\nmembers = []\n[workspace.dependencies]\nevani-core = { path = \"crates/evani-core\" }\nserde = \"1\"\n");
        d
    }

    #[test]
    fn versions_parse_and_order() {
        for ok in ["0.1.0", "10.20.30", "1.0.0-rc.1"] {
            assert!(valid_version(ok), "{ok}");
        }
        for bad in ["", "1.0", "1.0.0.0", "v1.0.0", "1.0.x", "1.0.0-", "1.0.0-RC"] {
            assert!(!valid_version(bad), "{bad}");
        }
        assert_eq!(compare("0.2.0", "0.10.0"), Ordering::Less);
        assert_eq!(compare("1.0.0-rc.1", "1.0.0"), Ordering::Less);
        assert_eq!(compare("1.0.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn version_line_prints_the_pack_version_in_the_format_evani_version_uses() {
        let d = std::env::temp_dir().join(format!("evani-version-line-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        w(&d, "packaging/engine-pack.json", "{\"pack_version\":\"0.1.0\"}");
        assert_eq!(version_line(&d), "evani 0.1.0");
        // Without a pins file the line shows this crate's version, still `evani <x.y.z>`.
        let bare = std::env::temp_dir().join(format!("evani-version-bare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bare);
        std::fs::create_dir_all(&bare).unwrap();
        let line = version_line(&bare);
        assert_eq!(line, format!("evani {}", crate::ENGINE_VERSION));
        assert!(valid_version(line.trim_start_matches("evani ")), "{line}");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn the_pin_is_read_and_written_without_touching_the_rest() {
        let base = "name = \"sea\"\nkit = \"sea\"\nlook = \"toon\"\n\n[modules]\nlive = true\n";
        assert_eq!(pin(base).unwrap(), None);
        let pinned = set_pin(base, "0.1.0");
        assert!(pinned.starts_with(base) && pinned.ends_with("[engine]\nversion = \"0.1.0\"\n"), "{pinned}");
        assert_eq!(pin(&pinned).unwrap().as_deref(), Some("0.1.0"));
        // Re-pinning replaces the line in place; nothing is duplicated.
        let again = set_pin(&pinned, "0.2.0");
        assert_eq!(again.matches("[engine]").count(), 1);
        assert_eq!(pin(&again).unwrap().as_deref(), Some("0.2.0"));
        assert_eq!(again.replace("0.2.0", "0.1.0"), pinned);
        // A table without a version gets the line under its header.
        let bare = format!("{base}\n[engine]\n\n[playground]\nfree_roam = true\n");
        let set = set_pin(&bare, "0.3.0");
        assert!(set.contains("[engine]\nversion = \"0.3.0\"\n\n[playground]"), "{set}");
        // Bad pins are errors with the reason.
        assert!(pin("[engine]\nversion = 1\n").unwrap_err().contains("must be a string"));
        assert!(pin("[engine]\nversion = \"latest\"\n").unwrap_err().contains("not an engine version"));
        assert!(pin("[engine]\nchannel = \"beta\"\n").unwrap_err().contains("unknown key"));
        // CRLF files stay CRLF.
        assert!(set_pin("name = \"a\"\r\n", "0.1.0").contains("[engine]\r\nversion = \"0.1.0\"\r\n"));
    }

    #[test]
    fn check_errors_on_a_pin_this_engine_is_not_and_says_how_to_fix_it() {
        let base = crate::testutil::temp_root("enginever-check");
        let engines = base.join("engines");
        let live = pack(&engines, &format!("{BUILD_PREFIX}0.2.0"), "0.2.0");
        pack(&engines, &format!("{KEEP_PREFIX}0.1.0"), "0.1.0");
        let root = live.join("engine");
        let toml = |v: Option<&str>| {
            let t = "name = \"sea\"\nkit = \"sea\"\nlook = \"toon\"\n".to_string();
            v.map_or(t.clone(), |v| set_pin(&t, v))
        };
        assert_eq!(running(&root), "0.2.0");
        assert_eq!(engines_dir_from(&root, None), Some(engines.clone()));
        assert!(pin_problems(&root, &toml(Some("0.2.0")), true, "g/app", "--app-dir g/app").is_empty());
        // Installed elsewhere: build it with that engine, or upgrade.
        let p = pin_problems(&root, &toml(Some("0.1.0")), true, "g/app", "--app-dir g/app");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].0, Severity::Error);
        assert!(p[0].1.contains("pinned to engine 0.1.0, but this is engine 0.2.0"), "{p:?}");
        assert!(p[0].2.contains("upgrade-engine --app-dir g/app") && p[0].2.contains(&format!("{KEEP_PREFIX}0.1.0")), "{p:?}");
        // Not installed at all.
        let p = pin_problems(&root, &toml(Some("0.0.9")), true, "g/app", "--app-dir g/app");
        assert!(p[0].0 == Severity::Error && p[0].1.contains("not installed (installed: 0.1.0, 0.2.0)") && p[0].2.contains("upgrade-engine"), "{p:?}");
        // No pin: a warning in a pack, nothing for a game inside the engine.
        let p = pin_problems(&root, &toml(None), true, "g/app", "--app-dir g/app");
        assert!(p[0].0 == Severity::Warn && p[0].2.contains("version = \"0.2.0\""), "{p:?}");
        assert!(pin_problems(&root, &toml(None), false, "apps/sea", "sea").is_empty());
        // A broken pin is an error.
        assert_eq!(pin_problems(&root, "name = \"a\"\nkit = \"a\"\nlook = \"b\"\n[engine]\nversion = \"x\"\n", true, "a", "a")[0].0, Severity::Error);
        assert!(refuse_other_engine(&root, &toml(Some("0.1.0")), "g/app", "--app-dir g/app").unwrap().starts_with("ERROR: g/app: pinned to engine 0.1.0"));
        assert!(refuse_other_engine(&root, &toml(None), "g/app", "--app-dir g/app").is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn relink_moves_only_the_engine_crate_paths() {
        let crates: BTreeMap<String, String> = [("evani-core".to_string(), "crates/evani-core".to_string()), ("evani-kit-example".to_string(), "kits/evani-kit-example".to_string())].into();
        let text = "[workspace]\nmembers = [\"app\", \"kit\"]\n\n[workspace.dependencies]\nevani-core = { path = \"C:/old/engine/crates/evani-core\" }\nevani-kit-sea = { path = \"kit\" }\nevani-kit-example = { path = \"kit-copy\" }\nserde = \"1\"\n\n[package.metadata.x]\nevani-core = { path = \"C:/elsewhere\" }\n";
        let (new, changed) = relink_text(text, &crates, Path::new("D:\\packs\\.evani-keep-evani-dev-engine-0.1.0\\engine"));
        assert_eq!(changed, ["evani-core"]);
        assert!(new.contains("evani-core = { path = \"D:/packs/.evani-keep-evani-dev-engine-0.1.0/engine/crates/evani-core\" }"), "{new}");
        assert!(new.contains("evani-kit-sea = { path = \"kit\" }") && new.contains("evani-kit-example = { path = \"kit-copy\" }"), "a game's own crates stay: {new}");
        assert!(new.contains("[package.metadata.x]\nevani-core = { path = \"C:/elsewhere\" }"), "outside dependency tables nothing moves: {new}");
        // Already right: nothing changes.
        let (same, none) = relink_text(&new, &crates, Path::new("D:/packs/.evani-keep-evani-dev-engine-0.1.0/engine"));
        assert!(none.is_empty() && same == new);
    }

    #[test]
    fn retention_keeps_the_live_and_pinned_packs_and_never_deletes() {
        let base = crate::testutil::temp_root("enginever-retain");
        let engines = base.join("engines");
        pack(&engines, &format!("{BUILD_PREFIX}0.1.0"), "0.1.0");
        pack(&engines, &format!("{BUILD_PREFIX}0.2.0"), "0.2.0");
        pack(&engines, &format!("{KEEP_PREFIX}0.3.0"), "0.3.0");
        let new = pack(&engines, &format!("{BUILD_PREFIX}0.4.0"), "0.4.0");
        // A game pinned to 0.1.0, outside the engines, linked to its pack.
        let game = base.join("games").join("sea");
        let old_engine = engines.join(format!("{BUILD_PREFIX}0.1.0")).join("engine");
        w(&game, "Cargo.toml", &format!("[workspace]\nmembers = [\"app\"]\n[workspace.dependencies]\nevani-core = {{ path = \"{}/crates/evani-core\" }}\n", old_engine.display().to_string().replace('\\', "/")));
        w(&game, "app/Cargo.toml", "[package]\nname = \"sea\"\n");
        w(&game, "app/evani.toml", &set_pin("name = \"sea\"\nkit = \"sea\"\nlook = \"toon\"\n", "0.1.0"));
        w(&base, "games/unpinned/evani.toml", "name = \"u\"\nkit = \"u\"\nlook = \"toon\"\n");
        let games = find_games(&[base.join("games")], 4);
        assert_eq!(games.len(), 2);
        let packs = installed(&engines);
        assert_eq!(packs.iter().map(|p| p.version.as_str()).collect::<Vec<_>>(), ["0.1.0", "0.2.0", "0.3.0", "0.4.0"]);
        let p = plan(&packs, &games, Some("0.4.0"));
        assert_eq!(p.live.as_deref(), Some("0.4.0"));
        assert!(!p.over_cap);
        assert_eq!(p.kept.iter().map(|k| k.0.as_str()).collect::<Vec<_>>(), ["0.1.0"]);
        // 0.1.0 becomes keep; 0.3.0 (no longer pinned) goes back to build; 0.4.0 gets the marker.
        assert!(p.steps.contains(&Step::Rename { version: "0.1.0".into(), from: engines.join(format!("{BUILD_PREFIX}0.1.0")), to: engines.join(format!("{KEEP_PREFIX}0.1.0")), why: "pinned by 1 game(s)".into() }), "{:?}", p.steps);
        assert!(p.steps.iter().any(|s| matches!(s, Step::Rename { version, to, .. } if version == "0.3.0" && to.ends_with(format!("{BUILD_PREFIX}0.3.0")))));
        assert!(p.steps.contains(&Step::MarkLive { version: "0.4.0".into(), dir: new.clone() }));
        let log = apply(&engines, &p, &games);
        assert!(log.iter().all(|l| !l.starts_with("refused")), "{log:?}");
        assert!(new.join(LIVE_MARKER).is_file());
        assert_eq!(installed(&engines).len(), 4, "nothing deleted");
        // The pinned game now points at the renamed pack.
        let ws = std::fs::read_to_string(game.join("Cargo.toml")).unwrap();
        assert!(ws.contains(&format!("{KEEP_PREFIX}0.1.0/engine/crates/evani-core")), "{ws}");
        // Applying again changes nothing.
        let again = plan(&installed(&engines), &games, None);
        assert!(again.steps.is_empty(), "{:?}", again.steps);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn more_than_three_pinned_old_packs_asks_instead_of_deleting() {
        let packs: Vec<Installed> = ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.5.0"]
            .iter()
            .map(|v| Installed { version: (*v).into(), dir: PathBuf::from(format!("E:/e/{KEEP_PREFIX}{v}")), class: Class::Keep, live: false })
            .collect();
        let games: Vec<FoundGame> = ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.9.0"].iter().map(|v| FoundGame { app_dir: PathBuf::from(format!("C:/g/{v}")), pin: Some((*v).into()) }).collect();
        let p = plan(&packs, &games, Some("0.5.0"));
        assert!(p.over_cap && p.kept.len() == 4, "{p:?}");
        assert_eq!(p.missing, vec![("0.9.0".to_string(), vec![PathBuf::from("C:/g/0.9.0")])]);
        // The live pack goes to the build class with its marker; none is removed.
        assert!(p.steps.iter().any(|s| matches!(s, Step::Rename { version, .. } if version == "0.5.0")));
        assert!(p.steps.iter().all(|s| !matches!(s, Step::Rename { version, .. } if version != "0.5.0")));
    }
}
