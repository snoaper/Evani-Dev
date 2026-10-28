//! `check`: the one gate an agent runs before it says it is done.
//!
//! Per game (`apps/<id>/evani.toml`), each issue with a way to fix it and a
//! doc:
//!
//! - ERROR: the module list doesn't match the game crate; an optional engine
//!   crate that isn't optional; a look, effect, physics or effect/look pair
//!   the catalog doesn't allow; untagged geocheck issues; FAIL lines on the
//!   review card; a look guard failure; a playground (`[playground]` in
//!   evani.toml) without free roam, its kit's declaration or its free-roam
//!   test (docs/PLAYGROUNDS.md); a model with a normal map on a primitive
//!   without tangents.
//! - WARN: no geocheck report; `default` features that don't mirror the
//!   list; a look guard not approvable here (no record or key); evidence
//!   older than the game's sources ("re-run"); out-of-date guidance or live
//!   MCP server.
//! - `--full`: the game's and kit's tests and the ship build too.
//!
//! Evidence (geocheck report, review card) only counts when it is newer than
//! everything the game is built from; otherwise it is a "stale, re-run"
//! warning. `--hook` is the Claude Code Stop hook: only the games the
//! session touched, exit 2 with the fixes while there are errors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

use crate::manifest::{self, Modules};
use crate::render_paths;
use crate::reports;
use crate::targets;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Note,
    Warn,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warn => "WARN",
            Severity::Note => "note",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// `manifest`, `catalog`, `playground`, `geocheck`, `review`,
    /// `lookguard`, `tests`, `ship`, `guidance`, `mcp`, `modules`, `content`.
    pub area: &'static str,
    pub app: Option<String>,
    pub what: String,
    /// What to do about it.
    pub fix: String,
    /// Where to read more (a path from the project root).
    pub docs: &'static str,
}

impl Issue {
    fn new(severity: Severity, area: &'static str, app: Option<&str>, what: impl Into<String>, fix: impl Into<String>, docs: &'static str) -> Self {
        Self { severity, area, app: app.map(str::to_string), what: what.into(), fix: fix.into(), docs }
    }

    pub fn to_json(&self) -> Value {
        json!({ "severity": self.severity.label(), "area": self.area, "app": self.app, "what": self.what, "fix": self.fix, "docs": self.docs })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Only these games (`None`: all).
    pub apps: Option<Vec<String>>,
    /// Also run the tests and the ship build.
    pub full: bool,
    /// Warnings count as failures.
    pub strict: bool,
    /// Skip the guidance and MCP server checks (the Stop hook).
    pub skip_guidance: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub apps: Vec<String>,
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn count(&self, s: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity == s).count()
    }

    pub fn passes(&self, strict: bool) -> bool {
        self.count(Severity::Error) == 0 && (!strict || self.count(Severity::Warn) == 0)
    }

    /// The last line: `evani check: clean ...` or the counts.
    pub fn summary(&self, strict: bool) -> String {
        let (e, w) = (self.count(Severity::Error), self.count(Severity::Warn));
        let games = if self.apps.is_empty() { "no games".to_string() } else { format!("games: {}", self.apps.join(", ")) };
        let plural = |n: usize, s: &str| format!("{n} {s}{}", if n == 1 { "" } else { "s" });
        if self.passes(strict) {
            if w == 0 { format!("evani check: clean ({games})") } else { format!("evani check: clean ({games}; {})", plural(w, "warning")) }
        } else {
            format!("evani check: {}, {} ({games}). Fix them and run it again.", plural(e, "error"), plural(w, "warning"))
        }
    }

    pub fn text(&self, strict: bool, min: Severity) -> String {
        let mut out = String::new();
        let mut sorted: Vec<&Issue> = self.issues.iter().filter(|i| i.severity >= min).collect();
        sorted.sort_by_key(|i| std::cmp::Reverse(i.severity));
        for i in sorted {
            let app = i.app.as_deref().map(|a| format!("{a} · ")).unwrap_or_default();
            out.push_str(&format!("{} {app}{}: {}\n", i.severity.label(), i.area, i.what));
            if !i.fix.is_empty() {
                out.push_str(&format!("  fix: {}\n", i.fix));
            }
            out.push_str(&format!("  docs: {}\n", i.docs));
        }
        out.push_str(&self.summary(strict));
        out.push('\n');
        out
    }

    pub fn to_json(&self, strict: bool) -> Value {
        json!({
            "clean": self.passes(strict),
            "errors": self.count(Severity::Error),
            "warnings": self.count(Severity::Warn),
            "apps": self.apps,
            "issues": self.issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
            "summary": self.summary(strict),
        })
    }
}

// ---- where a game is -------------------------------------------------------------

/// Where one game's parts are: `apps/<id>` and `kits/evani-kit-<kit>` in the
/// engine, or (`cargo xtask check --app-dir`) the folders of a game kept in
/// its own workspace outside the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    /// The app folder (its `evani.toml`, `Cargo.toml`, `src/`).
    pub dir: PathBuf,
    /// The app folder as messages name it (`apps/<id>`, or the folder).
    pub shown: String,
    /// How a `cargo xtask` command names the game (`<id>`, or `--app-dir <folder>`).
    pub arg: String,
    /// Where the game's evidence lives (`.vibedeck/geocheck`, `.vibedeck/review`,
    /// `.evani/publish`, `target/evani-sizes`) and its own tests run: the
    /// engine root, or the game's workspace.
    pub home: PathBuf,
    /// The kit crate's folder when it is not the engine's `kits/evani-kit-<kit>`.
    pub kit_dir: Option<PathBuf>,
}

impl Game {
    /// The game `apps/<id>` of the engine at `root`.
    pub fn in_engine(root: &Path, id: &str) -> Self {
        Self { id: id.to_string(), dir: root.join("apps").join(id), shown: format!("apps/{id}"), arg: id.to_string(), home: root.to_path_buf(), kit_dir: None }
    }

    /// A game outside the engine: app folder `dir`, game workspace `home`, kit
    /// crate folder `kit_dir` (as `cargo xtask build --app-dir` resolves them).
    pub fn outside(id: &str, dir: &Path, home: &Path, kit_dir: &Path) -> Self {
        let shown = dir.display().to_string();
        let arg = if shown.chars().any(char::is_whitespace) { format!("--app-dir \"{shown}\"") } else { format!("--app-dir {shown}") };
        Self { id: id.to_string(), dir: dir.to_path_buf(), shown, arg, home: home.to_path_buf(), kit_dir: Some(kit_dir.to_path_buf()) }
    }

    /// Outside the engine folder (`--app-dir`).
    pub fn is_outside(&self) -> bool {
        self.kit_dir.is_some()
    }

    /// The kit crate's folder for kit `kit`, and how messages name it.
    fn kit(&self, root: &Path, kit: &str) -> (PathBuf, String) {
        match &self.kit_dir {
            Some(d) => (d.clone(), d.display().to_string()),
            None => (root.join("kits").join(format!("evani-kit-{kit}")), format!("kits/evani-kit-{kit}")),
        }
    }
}

// ---- catalog --------------------------------------------------------------------

/// Looks the engine ships that aren't catalog looks: the neutral look the
/// example game uses, with the style its effects are checked against.
pub(crate) const NEUTRAL_LOOKS: &[(&str, &str)] = &[("basic", "3d")];

#[derive(Debug, Default)]
struct Catalog {
    /// look id -> style class (`2d`, `pseudo-3d`, `3d`).
    looks: Option<BTreeMap<String, String>>,
    /// effect id -> styles it works with.
    effects: Option<BTreeMap<String, Vec<String>>>,
    /// genre id -> (physics, compatible looks).
    genres: Option<BTreeMap<String, (String, Vec<String>)>>,
    /// Features of `crates/evani-fx`.
    fx_features: Option<BTreeSet<String>>,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

impl Catalog {
    fn load(root: &Path) -> Self {
        let list = |file: &str, key: &str| -> Option<Vec<Value>> { read_json(&root.join("catalog").join(file))?.get(key)?.as_array().cloned() };
        let id = |x: &Value| x.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Catalog {
            looks: list("looks.json", "looks").map(|l| l.iter().map(|x| (id(x), s(x, "style_class"))).collect()),
            effects: list("effects.json", "effects").map(|l| l.iter().map(|x| (id(x), strs(x.get("works_with")))).collect()),
            genres: list("genres.json", "genres").map(|l| l.iter().map(|x| (id(x), (s(x, "physics"), strs(x.get("compatible_looks"))))).collect()),
            fx_features: std::fs::read_to_string(root.join("crates/evani-fx/Cargo.toml"))
                .ok()
                .and_then(|t| t.parse::<toml::Table>().ok())
                .and_then(|t| t.get("features").and_then(toml::Value::as_table).map(|f| f.keys().cloned().collect())),
        }
    }

    fn style(&self, look: &str) -> Option<String> {
        NEUTRAL_LOOKS.iter().find(|(l, _)| *l == look).map(|(_, s)| (*s).to_string()).or_else(|| self.looks.as_ref()?.get(look).cloned())
    }
}

// ---- freshness ------------------------------------------------------------------

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn newest(p: &Path, skip: &[&str], best: &mut Option<SystemTime>) {
    let Ok(meta) = std::fs::metadata(p) else { return };
    if meta.is_file() {
        if let Ok(t) = meta.modified()
            && best.is_none_or(|b| t > b)
        {
            *best = Some(t);
        }
        return;
    }
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), "target" | "dist" | ".git" | "node_modules") || skip.contains(&name.as_ref()) {
            continue;
        }
        newest(&e.path(), skip, best);
    }
}

/// The newest change to anything the game is built from.
fn sources_time(root: &Path, g: &Game, kit_dir: &Path, m: &Modules) -> Option<SystemTime> {
    let mut best = None;
    newest(&g.dir, &["AGENTS.md", "CLAUDE.md"], &mut best);
    newest(kit_dir, &[], &mut best);
    if g.is_outside() {
        newest(&g.home.join("Cargo.lock"), &[], &mut best);
    }
    // The look's guard set and bake notes don't change what is drawn.
    newest(&root.join("looks").join(format!("evani-look-{}", m.look)), &["guard", "CHANGES.md"], &mut best);
    for p in ["crates", "assets", "Cargo.lock"] {
        newest(&root.join(p), &[], &mut best);
    }
    best
}

/// Evidence counts when it is at least as new as the sources.
fn fresh(evidence: &Path, sources: Option<SystemTime>) -> bool {
    match (mtime(evidence), sources) {
        (Some(e), Some(s)) => e >= s,
        _ => true,
    }
}

// ---- one game -------------------------------------------------------------------

fn check_game(root: &Path, g: &Game, cat: &Catalog, trust: &evani_lookguard::verify::Trust, full: bool, out: &mut Vec<Issue>) {
    let (app, shown, arg) = (g.id.as_str(), g.shown.as_str(), g.arg.as_str());
    let a = Some(app);
    let dir = &g.dir;
    let text = std::fs::read_to_string(dir.join("evani.toml"));
    let m = match text.as_ref().map_err(|e| e.to_string()).and_then(|t| Modules::parse(t)) {
        Ok(m) => m,
        Err(e) => {
            out.push(Issue::new(Severity::Error, "manifest", a, e, format!("Fix {shown}/evani.toml (see the other games' evani.toml)."), "README.md"));
            return;
        }
    };
    let feats = m.features();
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    let manifest_fix = format!(
        "Make {shown}/evani.toml and {shown}/Cargo.toml agree: each listed effect or module needs a feature of the same name that turns on its optional dependency, and the crate depends on evani-kit-{} and evani-look-{}.",
        m.kit, m.look
    );
    for p in manifest::crate_problems(&feats, &m.kit, &m.look, &cargo) {
        out.push(Issue::new(Severity::Error, "manifest", a, p, manifest_fix.clone(), "README.md"));
    }
    for c in manifest::not_optional(&cargo) {
        out.push(Issue::new(
            Severity::Error,
            "manifest",
            a,
            format!("`{c}` is a plain dependency, so it is in every build whether evani.toml lists it or not"),
            format!("In {shown}/Cargo.toml make it `{c} = {{ workspace = true, optional = true }}` and turn it on only from the features evani.toml maps to."),
            "README.md",
        ));
    }
    if let Some((missing, extra)) = manifest::default_mismatch(&feats, &cargo) {
        out.push(Issue::new(
            Severity::Warn,
            "manifest",
            a,
            format!("`default` features don't mirror evani.toml (missing: {}; extra: {})", list(&missing), list(&extra)),
            format!("Set `default` in {shown}/Cargo.toml to the features evani.toml maps to (`cargo xtask features {arg}`), so `trunk serve` matches the real build."),
            "README.md",
        ));
    }
    let (kit_dir, kit_shown) = g.kit(root, &m.kit);
    check_catalog(&m, app, shown, cat, out);
    check_render_path(root, app, &m, out);
    check_forward(root, app, &m, &cargo, &kit_dir, &kit_shown, out);
    check_modules(root, g, &m, &kit_dir, out);
    let kit_cargo = std::fs::read_to_string(kit_dir.join("Cargo.toml")).unwrap_or_default();
    for (sev, what, fix) in material_problems(root, shown, &kit_shown, &m, &cargo, &kit_cargo) {
        out.push(Issue::new(sev, "manifest", a, what, fix, "README.md"));
    }
    let evani_toml = text.as_deref().unwrap_or_default();
    let playground = if g.is_outside() {
        crate::playground::problems_in(&kit_dir, &dir.join("src"), &format!("{shown}/evani.toml"), &m.kit, evani_toml)
    } else {
        crate::playground::problems(root, app, &m.kit, evani_toml)
    };
    for p in playground {
        out.push(Issue::new(Severity::Error, "playground", a, p.what, p.fix, "docs/PLAYGROUNDS.md"));
    }
    if crate::playground::parse(evani_toml).ok().flatten().is_some() {
        let dist = g.home.join("target").join("evani-sizes").join(format!("{app}-ship"));
        if let Some(p) = crate::playground::size_problem(app, &dist) {
            out.push(Issue::new(Severity::Warn, "playground", a, format!("last ship build: {}", p.what), p.fix, "docs/PLAYGROUNDS.md"));
        }
    }
    check_tier_targets_summary(&g.home, app, out);
    let sources = sources_time(root, g, &kit_dir, &m);
    check_geocheck(g, &kit_dir, &m, cat, sources, out);
    check_card(g, sources, out);
    check_look_guard(root, app, &m, trust, out);
    if full {
        check_full(root, g, &m, &cargo, &kit_dir, out);
    }
}

fn list(v: &[String]) -> String {
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

/// `shown` names the app folder in the fixes (`apps/<id>`, or an outside game's folder).
fn check_catalog(m: &Modules, app: &str, shown: &str, cat: &Catalog, out: &mut Vec<Issue>) {
    let a = Some(app);
    let neutral = NEUTRAL_LOOKS.iter().any(|(l, _)| *l == m.look);
    if let Some(looks) = &cat.looks
        && !neutral
        && !looks.contains_key(&m.look)
    {
        out.push(Issue::new(
            Severity::Error,
            "catalog",
            a,
            format!("look `{}` is not in catalog/looks.json", m.look),
            "Pick a look id from catalog/looks.json (one the genre's `compatible_looks` lists) and use its crate looks/evani-look-<look>; don't invent looks.",
            "catalog/looks.json",
        ));
    }
    let style = cat.style(&m.look);
    for e in &m.effects {
        let works = match &cat.effects {
            Some(effects) => match effects.get(e) {
                None => {
                    out.push(Issue::new(
                        Severity::Error,
                        "catalog",
                        a,
                        format!("effect `{e}` is not in catalog/effects.json"),
                        format!("Remove `{e}` from {shown}/evani.toml or use an effect id from catalog/effects.json."),
                        "catalog/effects.json",
                    ));
                    continue;
                }
                Some(w) => w.clone(),
            },
            None => Vec::new(),
        };
        if let Some(fx) = &cat.fx_features
            && !fx.contains(e)
        {
            out.push(Issue::new(
                Severity::Error,
                "catalog",
                a,
                format!("effect `{e}` is in the catalog but crates/evani-fx has no `{e}` feature yet"),
                format!("Use an effect evani-fx implements (its [features]), or add `{e}` to evani-fx first (README.md, \"add an effect\")."),
                "README.md",
            ));
        }
        if let Some(s) = &style
            && !works.is_empty()
            && !works.contains(s)
        {
            out.push(Issue::new(
                Severity::Error,
                "catalog",
                a,
                format!("effect `{e}` works with {} looks, but `{}` is a {s} look", works.join("/"), m.look),
                format!("Drop `{e}` from {shown}/evani.toml or choose a look whose style_class it works with."),
                "catalog/effects.json",
            ));
        }
    }
    if let Some((physics, looks)) = cat.genres.as_ref().and_then(|g| g.get(&m.kit)) {
        if !neutral && !looks.is_empty() && !looks.contains(&m.look) {
            out.push(Issue::new(
                Severity::Error,
                "catalog",
                a,
                format!("look `{}` is not one of genre `{}`'s compatible_looks", m.look, m.kit),
                format!("Use a look from catalog/genres.json `{}`.compatible_looks.", m.kit),
                "catalog/genres.json",
            ));
        }
        let have = m.physics.clone().unwrap_or_else(|| "none".into());
        if !physics.is_empty() && *physics != have {
            out.push(Issue::new(
                Severity::Error,
                "catalog",
                a,
                format!("physics is `{have}` but genre `{}` uses `{physics}`", m.kit),
                format!("Set `[modules] physics = \"{physics}\"` in {shown}/evani.toml (the genre's catalog entry)."),
                "catalog/genres.json",
            ));
        }
    }
}

/// The render path the game's look needs must be built on every tier the game targets (assets/render_paths.json).
fn check_render_path(root: &Path, app: &str, m: &Modules, out: &mut Vec<Issue>) {
    let Some(style) = render_paths::style_of_look(root, &m.look) else { return };
    for p in render_paths::problems(root, &style, &m.look) {
        out.push(Issue::new(Severity::Error, "render path", Some(app), p.what, p.fix, "assets/render_paths.json"));
    }
    let lines = render_paths::report(root, &style, m.xr);
    if !lines.is_empty() {
        out.push(Issue::new(Severity::Note, "render path", Some(app), lines.join("; "), "Nothing to fix: this is what the game gets on each tier.", "assets/render_paths.json"));
    }
}

/// The Cargo features of the engine crate `krate` (`crates/<krate>`).
fn crate_features(root: &Path, krate: &str) -> Option<BTreeSet<String>> {
    let t: toml::Table = std::fs::read_to_string(root.join("crates").join(krate).join("Cargo.toml")).ok()?.parse().ok()?;
    Some(t.get("features")?.as_table()?.keys().filter(|k| *k != "default").cloned().collect())
}

/// Whether a Cargo.toml lists `krate` as a dependency (any table, optional or not).
fn depends_on(cargo: &str, krate: &str) -> bool {
    let Ok(t) = cargo.parse::<toml::Table>() else { return false };
    let mut tables: Vec<&toml::Table> = t.get("dependencies").and_then(toml::Value::as_table).into_iter().collect();
    for target in t.get("target").and_then(toml::Value::as_table).into_iter().flat_map(|x| x.values()) {
        tables.extend(target.get("dependencies").and_then(toml::Value::as_table));
    }
    tables.iter().any(|d| d.contains_key(krate))
}

/// The module rules for the material modules (`[modules] nature = [..]`,
/// `proc = [..]`) and for the kit's own Cargo.toml: `(severity, what, fix)`.
/// `app` and `kit` name the app and kit folders in messages.
/// - a listed part the engine crate has no feature for: ERROR;
/// - a listed module neither the app nor the kit depends on: ERROR;
/// - an optional engine crate the kit depends on plainly: ERROR (every game
///   on the kit would download it, listed or not; the app's own plain
///   dependencies are the `not_optional` rule above).
fn material_problems(root: &Path, app: &str, kit: &str, m: &Modules, app_cargo: &str, kit_cargo: &str) -> Vec<(Severity, String, String)> {
    let mut out = Vec::new();
    for (key, krate, parts) in m.materials() {
        if let Some(have) = crate_features(root, krate) {
            for p in parts.iter().filter(|p| !have.contains(*p)) {
                out.push((
                    Severity::Error,
                    format!("`modules.{key}` lists `{p}`, which {krate} has no feature for (its parts: {})", list(&have.iter().cloned().collect::<Vec<_>>())),
                    format!("List only parts {krate} has in {app}/evani.toml, or add the `{p}` feature to crates/{krate} first."),
                ));
            }
        }
        if !depends_on(app_cargo, krate) && !depends_on(kit_cargo, krate) {
            out.push((
                Severity::Error,
                format!("evani.toml lists `{key}` but neither the app nor the kit depends on {krate}, so the `{key}` features turn nothing on"),
                format!("Add `{krate} = {{ workspace = true, optional = true }}` to {kit}/Cargo.toml (or {app}/Cargo.toml) and turn it on from the app's `{key}` feature (e.g. `{key} = [\"dep:{krate}\"]`, or a kit feature it forwards to), or remove `{key}` from [modules]."),
            ));
        }
    }
    for c in manifest::not_optional(kit_cargo) {
        out.push((
            Severity::Error,
            format!("the kit depends on `{c}` as a plain dependency, so every game on this kit downloads it whether its evani.toml lists it or not"),
            format!("In {kit}/Cargo.toml make it `{c} = {{ workspace = true, optional = true }}` behind a kit feature, and turn that on from the app feature evani.toml maps to (e.g. `nature-ocean = [\"nature\", \"evani-kit-<kit>/ocean\"]`)."),
        ));
    }
    out
}

/// Folders under a game or its kit that never hold the game's shipped assets.
const ASSET_SKIP: &[&str] = &["target", "review", "dist", "node_modules", ".git"];

/// Model and font files found in a game's folder and its kit's folder
/// (Stage 3 `models`, `anim` and `ui` modules).
#[derive(Debug, Default, PartialEq, Eq)]
struct AssetScan {
    /// `.glb` / `.gltf` files, as `apps/<id>/...` paths.
    models: Vec<String>,
    /// The models that carry a skin (a skeleton to animate).
    skinned: Vec<String>,
    /// Font files (`.ttf`, `.otf`, `.woff`, `.woff2`).
    fonts: Vec<String>,
    /// Models with a normal-mapped primitive that has no TANGENT.
    untangent: Vec<String>,
}

/// A glTF / GLB file's JSON.
fn gltf_json(bytes: &[u8]) -> Option<Value> {
    let json: &[u8] = if bytes.starts_with(b"glTF") {
        // 12-byte header, then the first chunk: u32 length, u32 type (JSON), data.
        let len = bytes.get(12..16).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        bytes.get(20..20usize.saturating_add(len))?
    } else {
        bytes
    };
    serde_json::from_slice::<Value>(json).ok()
}

/// Whether a glTF / GLB file's JSON declares at least one skin.
fn has_skin(bytes: &[u8]) -> bool {
    gltf_json(bytes).and_then(|v| v.get("skins").and_then(Value::as_array).map(|a| !a.is_empty())).unwrap_or(false)
}

/// Whether a primitive's material has a normal map but the primitive has no
/// TANGENT attribute: the model path then cannot use the map (S3-3 shades
/// such a part with its vertex normal), so the detail silently vanishes.
fn normal_map_without_tangents(bytes: &[u8]) -> bool {
    let Some(v) = gltf_json(bytes) else { return false };
    let mats = v.get("materials").and_then(Value::as_array).cloned().unwrap_or_default();
    let mapped = |m: Option<u64>| m.and_then(|i| mats.get(i as usize)).is_some_and(|m| m.get("normalTexture").is_some());
    v.get("meshes").and_then(Value::as_array).into_iter().flatten().flat_map(|m| m.get("primitives").and_then(Value::as_array).into_iter().flatten()).any(|p| {
        mapped(p.get("material").and_then(Value::as_u64)) && p.get("attributes").and_then(|a| a.get("TANGENT")).is_none()
    })
}

fn scan_assets(root: &Path, dirs: &[PathBuf]) -> AssetScan {
    let mut out = AssetScan::default();
    for dir in dirs {
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if !p.file_name().is_some_and(|n| ASSET_SKIP.iter().any(|s| n == *s)) {
                        stack.push(p);
                    }
                    continue;
                }
                let ext = p.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                match ext.as_str() {
                    "glb" | "gltf" => {
                        let bytes = std::fs::read(&p).unwrap_or_default();
                        if has_skin(&bytes) {
                            out.skinned.push(rel.clone());
                        }
                        if normal_map_without_tangents(&bytes) {
                            out.untangent.push(rel.clone());
                        }
                        out.models.push(rel);
                    }
                    "ttf" | "otf" | "woff" | "woff2" => out.fonts.push(rel),
                    _ => {}
                }
            }
        }
    }
    out.models.sort();
    out.skinned.sort();
    out.fonts.sort();
    out.untangent.sort();
    out
}

fn first_of(v: &[String]) -> String {
    match v {
        [] => String::new(),
        [one] => format!("`{one}`"),
        [first, rest @ ..] => format!("`{first}` and {} more", rest.len()),
    }
}

/// The module rules for assets, as a pure function: `(what, fix)` pairs. A game
/// that ships `.glb` models must list `models`, a skinned model needs `anim`,
/// a font file needs `ui`.
/// `shown` names the app folder in messages, `arg` the game in commands.
fn module_problems(shown: &str, arg: &str, m: &Modules, scan: &AssetScan) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !scan.models.is_empty() && !m.models {
        out.push((
            format!("the game has model files ({}) but evani.toml doesn't list the `models` module, so nothing reads them", first_of(&scan.models)),
            format!("Add `models = true` under [modules] in {shown}/evani.toml and a `models` feature in {shown}/Cargo.toml (`cargo xtask features {arg}` shows what is built), or remove the model files."),
        ));
    }
    if !scan.skinned.is_empty() && !m.anim {
        out.push((
            format!("the model {} has a skin (a skeleton) but evani.toml doesn't list the `anim` module, so it can't be animated", first_of(&scan.skinned)),
            format!("Add `anim = true` under [modules] in {shown}/evani.toml (it brings `models` with it) and an `anim` feature in {shown}/Cargo.toml, or export the model without a skin."),
        ));
    }
    if !scan.fonts.is_empty() && !m.ui {
        out.push((
            format!("the game lists a font ({}) but evani.toml doesn't list the `ui` module, so the font is never drawn", first_of(&scan.fonts)),
            format!("Add `ui = true` under [modules] in {shown}/evani.toml and a `ui` feature in {shown}/Cargo.toml, or remove the font file."),
        ));
    }
    out
}

/// Model, skin and font files must match the `models`, `anim` and `ui`
/// modules (docs/STAGE3_PLAN.md section 4).
fn check_modules(root: &Path, g: &Game, m: &Modules, kit_dir: &Path, out: &mut Vec<Issue>) {
    let app = g.id.as_str();
    let dirs = [g.dir.clone(), kit_dir.to_path_buf()];
    // Paths in messages are from the game's home (the engine root for apps/).
    let scan = scan_assets(&g.home, &dirs);
    for (what, fix) in module_problems(&g.shown, &g.arg, m, &scan) {
        out.push(Issue::new(Severity::Error, "modules", Some(app), what, fix, "docs/STAGE3_PLAN.md"));
    }
    // The `[ui] fonts` list: what the font step would refuse, caught here.
    let toml = std::fs::read_to_string(g.dir.join("evani.toml")).unwrap_or_default();
    for (error, what, fix) in crate::fonttools::list_problems_in(root, &g.dir, &g.shown, m.ui, &toml) {
        out.push(Issue::new(if error { Severity::Error } else { Severity::Note }, "fonts", Some(app), what, fix, "README.md"));
    }
    for (what, fix) in model_problems(&scan) {
        out.push(Issue::new(Severity::Error, "models", Some(app), what, fix, "docs/MODEL_FORMAT.md"));
    }
    if m.models {
        for p in crate::stream::problems_in(&g.dir, &g.shown, &g.arg) {
            out.push(Issue::new(Severity::Error, "stream", Some(app), p.what, p.fix, "docs/STAGE3_PLAN.md"));
        }
        for w in crate::content::warnings_in(&g.dir, &g.shown, &g.arg) {
            out.push(Issue::new(Severity::Warn, "content", Some(app), w.what, w.fix, "docs/CONTENT.md"));
        }
    }
    check_model_files(&g.home, Some(app), &scan.models, out);
}

/// Converter inputs and scratch: the untouched master (`<id>.master.glb`)
/// and the `.work` folder are never downloaded, so they aren't checked.
fn shipped_model(rel: &str) -> bool {
    !rel.ends_with(".master.glb") && !rel.contains("/.work/")
}

/// Every shipped model file through the engine's own readers
/// ([`crate::modelcheck`]): one ERROR per file, listing what is wrong.
fn check_model_files(root: &Path, app: Option<&str>, models: &[String], out: &mut Vec<Issue>) {
    for rel in models.iter().filter(|r| shipped_model(r)) {
        let lines = crate::modelcheck::file_problems(&root.join(rel));
        if !lines.is_empty() {
            out.push(Issue::new(
                Severity::Error,
                "models",
                app,
                format!("`{rel}`: {}", lines.join("; ")),
                "Convert the source model again with `cargo xtask convert <source>` (it writes every tier file and checks it with these same rules), or fix what is named in the file.",
                "docs/MODEL_FORMAT.md",
            ));
        }
        for w in crate::modelcheck::file_warnings(&root.join(rel)) {
            out.push(Issue::new(
                Severity::Warn,
                "models",
                app,
                format!("`{rel}`: {w}"),
                "Merge or drop joints the rig doesn't need (finger and twist bones far from the camera), down to 64 or fewer; it draws as it is, at more cost per character.",
                "docs/MODEL_FORMAT.md",
            ));
        }
    }
}

/// Model files that would draw wrong, as `(what, fix)` pairs: a normal map
/// on a primitive without tangents.
fn model_problems(scan: &AssetScan) -> Vec<(String, String)> {
    scan.untangent
        .iter()
        .map(|f| {
            (
                format!("the model `{f}` has a normal-mapped material on a primitive without tangents (no TANGENT attribute), so its normal map can't be used and the surface detail is lost"),
                "Export the model with tangents (Blender's glTF exporter: Mesh > Tangents; gltfpack keeps them), or remove the material's normal map.".to_string(),
            )
        })
        .collect()
}

/// What in a kit's source draws on the 3D path (the renderer's `forward`
/// feature): mesh instances, decals, world-space particles.
const DRAWS_3D: &[&str] = &["MeshInstance", "draw_mesh(", "draw_decal(", ".meshes.push", "decals.push", "Space::World3d"];

/// Whether a crate's Cargo.toml turns on evani-render's 3D path on its
/// `evani-render` dependency line: `forward`, or `meshes` / `shadows`,
/// which imply it.
fn dep_turns_on_forward(cargo: &str) -> bool {
    cargo.lines().map(str::trim).filter(|l| l.starts_with("evani-render") && !l.starts_with("evani-render-")).any(|l| {
        l.split_once("features").is_some_and(|(_, f)| ["\"forward\"", "\"meshes\"", "\"shadows\""].iter().any(|n| f.contains(n)))
    })
}

/// The first 3D draw marker in the `.rs` files under `dir`, as `file: marker`.
fn first_3d_draw(dir: &Path) -> Option<String> {
    let mut stack = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    files.sort();
    files.iter().find_map(|f| {
        let text = std::fs::read_to_string(f).ok()?;
        let m = DRAWS_3D.iter().find(|m| text.contains(**m))?;
        let name = f.strip_prefix(dir).unwrap_or(f).to_string_lossy().replace('\\', "/");
        Some(format!("src/{name} uses `{m}`"))
    })
}

/// The 3D path rule as a pure function: `style` is the look's style,
/// `draws` the kit's first 3D draw (if any); `kit` is the kit's folder as
/// messages name it (`kits/evani-kit-<kit>`). An ERROR when the game draws
/// 3D but nothing turns `forward` on (its draws would be dropped), a WARN
/// when a 2D game downloads the 3D path without drawing 3D.
fn forward_problem(kit: &str, style: Option<&str>, draws: Option<&str>, kit_cargo: &str, app_cargo: &str, effects: &[String]) -> Option<(Severity, String, String)> {
    let by_effect = effects.iter().find(|e| matches!(e.as_str(), "outline" | "shadows"));
    let on = dep_turns_on_forward(kit_cargo) || dep_turns_on_forward(app_cargo) || by_effect.is_some();
    let fix_on = format!("In {kit}/Cargo.toml make the renderer line `evani-render = {{ workspace = true, features = [\"forward\"] }}` (README \"Render\"; `cargo xtask new-game` writes it for 3D games).");
    match (style == Some("3d") || draws.is_some(), on) {
        (true, false) => {
            let why = match (style, draws) {
                (Some("3d"), _) => "its look's style is `3d`".to_string(),
                (_, Some(d)) => format!("the kit draws 3D ({d})"),
                _ => "it draws 3D".to_string(),
            };
            Some((Severity::Error, format!("{why}, but no crate turns on evani-render's `forward` feature (the 3D path), so its 3D draws would be dropped"), fix_on))
        }
        (false, true) if style == Some("2d") => {
            let how = match by_effect {
                Some(e) => format!("the `{e}` effect"),
                None => "a Cargo.toml".to_string(),
            };
            Some((
                Severity::Warn,
                format!("a 2D game turns on evani-render's `forward` (through {how}) but its kit draws no 3D: the 3D path is downloaded and its pass runs empty every frame"),
                format!("Remove `forward` (and `meshes` / `shadows`) from the evani-render line in {kit}/Cargo.toml and apps/<game>/Cargo.toml, unless the kit is about to draw meshes."),
            ))
        }
        _ => None,
    }
}

/// The 3D path (evani-render feature `forward`) must be on exactly when the
/// game draws 3D (README "Render", CORE_REQUESTS `forward-optional`).
fn check_forward(root: &Path, app: &str, m: &Modules, app_cargo: &str, kit_dir: &Path, kit_shown: &str, out: &mut Vec<Issue>) {
    let kit_cargo = std::fs::read_to_string(kit_dir.join("Cargo.toml")).unwrap_or_default();
    let style = render_paths::style_of_look(root, &m.look);
    let draws = first_3d_draw(&kit_dir.join("src"));
    if let Some((sev, what, fix)) = forward_problem(kit_shown, style.as_deref(), draws.as_deref(), &kit_cargo, app_cargo, &m.effects) {
        out.push(Issue::new(sev, "render path", Some(app), what, fix, "README.md"));
    }
}

fn check_geocheck(g: &Game, kit_dir: &Path, m: &Modules, cat: &Catalog, sources: Option<SystemTime>, out: &mut Vec<Issue>) {
    let (app, arg) = (g.id.as_str(), g.arg.as_str());
    let a = Some(app);
    let is_2d = cat.style(&m.look).as_deref() == Some("2d");
    // A 2D kit with a scene-dump example runs through the same command.
    let has_dump = kit_dir.join("examples/geocheck_dump.rs").is_file();
    // A 3D game outside the engine brings its own adapter (docs/GEOCHECK.md).
    let has_adapter = kit_dir.join("examples/geocheck.rs").is_file();
    let run = if is_2d && !has_dump {
        if g.is_outside() {
            format!("the 2D geometry check (add the kit's `examples/geocheck_dump.rs`, tools/evani-geocheck-2d README, then `cargo xtask geocheck {arg}`)")
        } else {
            "the 2D geometry check (add the kit's `examples/geocheck_dump.rs`, tools/evani-geocheck-2d README, then `cargo xtask geocheck <app>`)".to_string()
        }
    } else if g.is_outside() && !has_dump && !has_adapter {
        format!("the geometry check (add the kit's own adapter `examples/geocheck.rs`, docs/GEOCHECK.md \"Games outside the engine\", then `cargo xtask geocheck {arg}`)")
    } else {
        format!("`cargo xtask geocheck {arg}`")
    };
    let path = g.home.join(".vibedeck/geocheck").join(app).join("report.json");
    // A partial report (`--sample N`) is a quick check, never proof: refused
    // wherever it lands among the reports this reads.
    let mut refused = false;
    for (file, p) in [("report.json", path.clone()), ("zfight/zfight.json", g.home.join(".vibedeck/geocheck").join(app).join("zfight").join("zfight.json"))] {
        if let Some(what) = read_json(&p).and_then(|r| reports::partial(&r)) {
            out.push(Issue::new(
                Severity::Error,
                "geocheck",
                a,
                format!("the saved geometry report {file} is partial ({what}): a --sample quick check is not proof"),
                format!("Re-run {run} without --sample (the full scan); --sample reports belong in zfight-sample/, never in .vibedeck/geocheck/{app}/{file}."),
                "docs/GEOCHECK.md",
            ));
            refused |= file == "report.json";
        }
    }
    if refused {
        return;
    }
    let Some(report) = read_json(&path) else {
        out.push(Issue::new(Severity::Warn, "geocheck", a, "no geometry check report", format!("Run {run}; fix or tag every issue it finds."), "docs/GEOCHECK.md"));
        return;
    };
    let untagged: Vec<reports::GeoIssue> = reports::geo_issues(&report).into_iter().filter(|i| !i.tagged).collect();
    if untagged.is_empty() {
        if !fresh(&path, sources) {
            out.push(Issue::new(Severity::Note, "geocheck", a, "the geometry report is older than the game's sources", format!("Re-run {run} after geometry changes."), "docs/GEOCHECK.md"));
        }
        return;
    }
    if !fresh(&path, sources) {
        out.push(Issue::new(
            Severity::Warn,
            "geocheck",
            a,
            format!("the geometry report ({} untagged issues) is older than the game's sources", untagged.len()),
            format!("Re-run {run}; it decides whether they are fixed."),
            "docs/GEOCHECK.md",
        ));
        return;
    }
    const SHOWN: usize = 8;
    for i in untagged.iter().take(SHOWN) {
        let what = if i.what.is_empty() { i.check.clone() } else { format!("{}: {}", i.check, i.what) };
        let fix = if i.fix.is_empty() { "Fix it in the kit's level or scene data, or tag it with a reason if it is intended (docs/GEOCHECK.md, tags).".to_string() } else { format!("{} (Or, only if intended, tag it with a reason.)", i.fix) };
        out.push(Issue::new(Severity::Error, "geocheck", a, format!("untagged geometry issue, {what}"), fix, "docs/GEOCHECK.md"));
    }
    if untagged.len() > SHOWN {
        out.push(Issue::new(
            Severity::Error,
            "geocheck",
            a,
            format!("{} more untagged geometry issues", untagged.len() - SHOWN),
            format!("See .vibedeck/geocheck/{app}/report.json (`fix` of each issue), or the live `debug_geometry` tool."),
            "docs/GEOCHECK.md",
        ));
    }
}

fn check_card(g: &Game, sources: Option<SystemTime>, out: &mut Vec<Issue>) {
    let (app, arg) = (g.id.as_str(), g.arg.as_str());
    let a = Some(app);
    let path = g.home.join(".vibedeck/review").join(app).join("card.json");
    let Some(card) = read_json(&path) else { return };
    let fails: Vec<reports::CardLine> = reports::card_lines(&card).into_iter().filter(|l| l.status == "fail").collect();
    if fails.is_empty() {
        return;
    }
    if !fresh(&path, sources) {
        out.push(Issue::new(
            Severity::Warn,
            "review",
            a,
            format!("the review card ({} FAIL lines) is older than the game's sources", fails.len()),
            format!("Re-run `cargo xtask review {arg}` to see whether they are fixed."),
            "docs/KIT_PROCESS.md",
        ));
        return;
    }
    for l in fails {
        out.push(Issue::new(
            Severity::Error,
            "review",
            a,
            format!("review card line {} fails: {}", l.n, l.evidence),
            format!("Fix what the evidence says (the line: {}), then re-run `cargo xtask review {arg}`.", l.text),
            "docs/KIT_PROCESS.md",
        ));
    }
}

fn check_look_guard(root: &Path, app: &str, m: &Modules, trust: &evani_lookguard::verify::Trust, out: &mut Vec<Issue>) {
    let look = format!("evani-look-{}", m.look);
    if !root.join("looks").join(&look).is_dir() {
        return;
    }
    let a = Some(app);
    match evani_lookguard::verify::verify_look(root, &look, trust) {
        Ok(_) => {}
        Err(f) if matches!(f.code, "record_missing" | "no_pubkey") => out.push(Issue::new(
            Severity::Warn,
            "lookguard",
            a,
            f.message,
            "Only the project owner approves a look's guard set, in Evani.App; propose with `look_limits_propose` / `look_baseline_propose`. Not something an agent fixes.",
            "docs/LIVE_API.md",
        )),
        Err(f) => out.push(Issue::new(
            Severity::Error,
            "lookguard",
            a,
            format!("{} ({})", f.message, f.code),
            format!("Never edit looks/{look}/guard to pass: put back the approved files (`git checkout -- looks/{look}/guard`), or propose the change with `look_limits_propose` / `look_baseline_propose` for the project owner to approve in Evani.App."),
            "docs/LIVE_API.md",
        )),
    }
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

fn run_cmd(root: &Path, args: &[&str]) -> Result<(), String> {
    // Same work as ever; the children just run at below-normal priority with
    // capped test threads (the shared PC stays usable).
    let limits = crate::quick::Limits::full(&crate::quick::Caps::detect());
    let t = std::time::Instant::now();
    let result = match crate::quick::cargo_command(root, args, limits).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(tail(&format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), 12)),
        Err(e) => Err(e.to_string()),
    };
    eprintln!("  full: `cargo {}` took {:.1}s", args.iter().take(3).copied().collect::<Vec<_>>().join(" "), t.elapsed().as_secs_f64());
    result
}

fn check_full(root: &Path, g: &Game, m: &Modules, cargo_toml: &str, kit_dir: &Path, out: &mut Vec<Issue>) {
    let (app, arg) = (g.id.as_str(), g.arg.as_str());
    let a = Some(app);
    let package_of = |text: &str| text.parse::<toml::Table>().ok().and_then(|t| t.get("package")?.get("name")?.as_str().map(str::to_string));
    let pkg = package_of(cargo_toml).unwrap_or_else(|| format!("evani-{app}"));
    // An outside kit is named by its own Cargo.toml; the engine's kits by the convention.
    let outside_kit = g.is_outside().then(|| std::fs::read_to_string(kit_dir.join("Cargo.toml")).ok()).flatten().and_then(|t| package_of(&t));
    let kit = outside_kit.unwrap_or_else(|| format!("evani-kit-{}", m.kit));
    // The game's own tests run in its workspace; the ship build is the
    // engine's xtask (an outside game is built with --app-dir).
    if let Err(e) = run_cmd(&g.home, &["test", "--quiet", "-p", &pkg, "-p", &kit]) {
        out.push(Issue::new(Severity::Error, "tests", a, format!("`cargo test -p {pkg} -p {kit}` fails: {e}"), "Make the failing tests pass; don't delete or weaken them.", "docs/KIT_PROCESS.md"));
    }
    let dir = g.dir.to_string_lossy();
    let build: Vec<&str> = if g.is_outside() { vec!["xtask", "build", "--app-dir", &dir, "--ship"] } else { vec!["xtask", "build", app, "--ship"] };
    if let Err(e) = run_cmd(root, &build) {
        out.push(Issue::new(
            Severity::Error,
            "ship",
            a,
            format!("`cargo xtask build {arg} --ship` fails: {e}"),
            "A ship build must hold only what evani.toml lists and no dev-only code; follow the build's own message.",
            "README.md",
        ));
    }
}

/// Engine crates whose tests hold a shared file format to its document (the
/// model reader: the gltfpack corpus and Evani.Model's files against
/// docs/MODEL_FORMAT.md). `--full` runs them once per project, whether or not
/// a game uses them yet, so a reader or decoder change that misreads a file is
/// caught before any game ships it (the corpus test found one such bug).
/// `(crate, extra cargo args, document)`. The model reader runs twice: as games
/// build it, and with the converter's native-only `anim-source` reading.
const FORMAT_TESTS: [(&str, &[&str], &str); 2] =
    [("evani-gltf", &[], "docs/MODEL_FORMAT.md"), ("evani-gltf", &["--features", "anim-source"], "docs/MODEL_FORMAT.md")];

fn check_format_tests(root: &Path, out: &mut Vec<Issue>) {
    for (krate, extra, docs) in FORMAT_TESTS {
        if !root.join("crates").join(krate).join("Cargo.toml").is_file() {
            continue;
        }
        let mut args = vec!["test", "--quiet", "-p", krate];
        args.extend_from_slice(extra);
        let shown = args.join(" ");
        if let Err(e) = run_cmd(root, &args) {
            out.push(Issue::new(
                Severity::Error,
                "formats",
                None,
                format!("`cargo {shown}` fails: {e}"),
                format!("A file the format document allows no longer reads the same. Fix the reader (or the writer that made the fixture) until `cargo {shown}` passes; don't edit the fixtures or loosen the comparison to pass."),
                docs,
            ));
        }
    }
}

/// Engine shader tests that hold every game to Chris's same-look rule (the
/// look on a phone or tablet is the look on the PC): the textured model
/// shader must shade a face turned away from the sun the same with and
/// without the shadow map, and without one the baked sun cone must cast the
/// model's shadow on itself where the shadow map would (exact ray casts on
/// the shipped marshal), so Lite and Low match Medium and up, and a hidden LOD
/// level must not shade the drawn one (`hidden_lod_levels_do_not_shade_the_drawn_one`),
/// and no big face the shadow map lights whole may be drawn part dark by
/// blending corner cones (`big_faces_get_no_diagonal_sun_smear` on the relief
/// tile, `baked_sun_draws_no_smear_on_any_model` on every shipped and fixture
/// model; the whole `model_wgsl` test file runs, so every test in it is in
/// the list).
/// `--full` runs them once per project, whether or not a game uses `models` yet.
/// Surface relief (S3-10) likewise: its fragment stage validates and shades as
/// the model stage does, its micro-detail fades below 1.5 px, and only its
/// parallax step count is tier-dependent (fine on High and Ultra, 2 steps
/// elsewhere), so the relief reads the same on every device.
/// `(crate, cargo args after -p, document)`.
const LOOK_PARITY_TESTS: [(&str, &[&str], &str); 7] = [
    ("evani-render", &["--features", "models,shadows", "--test", "model_wgsl"], "docs/STAGE3_PLAN.md"),
    ("evani-render", &["--features", "relief,shadows", "--test", "relief_wgsl"], "docs/MODEL_FORMAT.md"),
    // The forward shader's own tests with each soft-shadow feature on: `pcss`
    // adds the `pcss_params` slot and `pcss-stable` swaps the tail, and a test
    // that only ran without them went stale unseen (2026-10-24).
    ("evani-render", &["--features", "pcss", "--lib"], "docs/STAGE3_PLAN.md"),
    ("evani-render", &["--features", "pcss-stable", "--lib"], "docs/STAGE3_PLAN.md"),
    // The model shader test splices the shipped forward source too, so it
    // needs the same slot with either soft-shadow feature (Join 7, 2026-10-24).
    ("evani-render", &["--features", "models,pcss", "--test", "model_wgsl"], "docs/STAGE3_PLAN.md"),
    ("evani-render", &["--features", "models,pcss-stable", "--test", "model_wgsl"], "docs/STAGE3_PLAN.md"),
    // Shadow fit (`shadow-fit`): its tests (and the shadow plan's with the
    // fit hooks compiled in) only build with the feature on.
    ("evani-render", &["--features", "shadow-fit", "--lib"], "README.md"),
];

fn check_look_parity_tests(root: &Path, out: &mut Vec<Issue>) {
    for (krate, extra, docs) in LOOK_PARITY_TESTS {
        if !root.join("crates").join(krate).join("Cargo.toml").is_file() {
            continue;
        }
        let mut args = vec!["test", "--quiet", "-p", krate];
        args.extend_from_slice(extra);
        let shown = args.join(" ");
        if let Err(e) = run_cmd(root, &args) {
            out.push(Issue::new(
                Severity::Error,
                "look-parity",
                None,
                format!("`cargo {shown}` fails: {e}"),
                format!("An engine shader no longer validates, or shades differently on tiers without the shadow map, or a model's baked self-shadow no longer matches its cast shadow (Lite and Low would not look like the PC). Fix the shader until `cargo {shown}` passes; don't loosen the test."),
                docs,
            ));
        }
    }
}

/// The netcode's tests (docs/NET_BUILD_PLAN.md N1): wire round trips and
/// limits, the referee's determinism, hostile packets, 2 to 8 clients plus
/// bots converging on the referee's state over a lossy loopback, the sync
/// test (`evani_net::synctest`, any `NetGame` can run it) and the native vs
/// wasm32 float probe. `--full` runs them once per project, whether or not a
/// game lists `net` yet, so a netcode change that breaks determinism is
/// caught before a multiplayer game ships it. `(crate, test target, document)`.
const NET_TESTS: [(&str, &str, &str); 1] = [("evani-net", "net", "docs/NET_BUILD_PLAN.md")];

fn check_net_tests(root: &Path, out: &mut Vec<Issue>) {
    for (krate, test, docs) in NET_TESTS {
        if !root.join("crates").join(krate).join("Cargo.toml").is_file() {
            continue;
        }
        let args = ["test", "--quiet", "-p", krate];
        if let Err(e) = run_cmd(root, &args) {
            out.push(Issue::new(
                Severity::Error,
                "netcode",
                None,
                format!("`cargo {}` fails (tests/{test}/ and the unit tests): {e}", args.join(" ")),
                "The multiplayer core no longer steps the same on every peer, decodes a packet differently, or lets a client drift from the referee. Fix the netcode (or the game step the failing sync test names) until the tests pass; don't loosen them.",
                docs,
            ));
        }
    }
}

/// The model converter's tests (S3-5): every tier file it writes passes the
/// Khronos validator and the shared model check, keeps anchors and extras, and
/// is the same bytes run to run. `--full` runs them once per project. Without
/// the pinned tools (no `doctor --fix` yet) the tool-driven tests print
/// `SKIPPED` and pass, so a checkout without them still gets a clean gate.
fn check_converter_tests(root: &Path, out: &mut Vec<Issue>) {
    if !root.join("tools").join("evani-convert").join("Cargo.toml").is_file() {
        return;
    }
    let args = ["test", "--quiet", "-p", "evani-convert", "--", "--test-threads=2"];
    if let Err(e) = run_cmd(root, &args) {
        out.push(Issue::new(
            Severity::Error,
            "converter",
            None,
            format!("`cargo {}` fails: {e}", args.join(" ")),
            "The model converter writes files the engine or the Khronos validator would refuse, or no longer the same bytes. Fix the converter until the tests pass; don't loosen them. A tool problem names its own fix (`cargo xtask doctor --fix`).",
            "docs/MODEL_FORMAT.md",
        ));
    }
}

/// The Blender add-on (S3-15) is GPL-3.0-or-later and must stay a separate
/// folder: no engine or game crate may include, embed or name its files.
fn check_blender_addon_isolation(root: &Path, out: &mut Vec<Issue>) {
    let dirs: Vec<PathBuf> = ["crates", "apps", "kits", "looks"].iter().map(|d| root.join(d)).collect();
    blender_addon_isolation_in(root, &dirs, out);
}

/// [`check_blender_addon_isolation`] over `dirs` (paths shown from `root`).
fn blender_addon_isolation_in(root: &Path, dirs: &[PathBuf], out: &mut Vec<Issue>) {
    fn walk(dir: &Path, hits: &mut Vec<String>, root: &Path) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !matches!(name.as_str(), "target" | "node_modules" | "dist" | ".git") {
                    walk(&p, hits, root);
                }
            } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs" | "toml" | "js" | "mjs" | "html" | "wgsl" | "json"))
                && std::fs::read_to_string(&p).is_ok_and(|t| t.contains("tools/blender-addon") || t.contains("tools\\blender-addon"))
            {
                hits.push(p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut hits = Vec::new();
    for d in dirs {
        walk(d, &mut hits, root);
    }
    if !hits.is_empty() {
        hits.sort();
        out.push(Issue::new(
            Severity::Error,
            "licence",
            None,
            format!("an engine or game file refers to tools/blender-addon (GPL-3.0-or-later): {}", hits.join(", ")),
            "Remove the reference. The Blender add-on is a separate GPL folder that engine and game crates never include, embed or name (docs/STAGE3_PLAN.md S3-15).",
            "docs/STAGE3_PLAN.md",
        ));
    }
}

// ---- the whole project ----------------------------------------------------------

/// Run the check on the project at `root`.
pub fn run(root: &Path, opts: &Options) -> Report {
    let all = targets::games(root);
    let apps: Vec<String> = match &opts.apps {
        Some(only) => only.clone(),
        None => all.clone(),
    };
    let mut issues = Vec::new();
    let cat = Catalog::load(root);
    let trust = evani_lookguard::verify::Trust::from_store();
    for app in &apps {
        if !all.contains(app) {
            issues.push(Issue::new(Severity::Error, "manifest", Some(app), format!("no game `{app}` (apps/{app}/evani.toml)"), format!("Games here: {}. Start a new one with `cargo xtask new-game {app}`.", list(&all)), "docs/AI_GUIDANCE.md"));
            continue;
        }
        check_game(root, &Game::in_engine(root, app), &cat, &trust, opts.full, &mut issues);
    }
    // The shared models `cargo xtask convert` writes by default.
    check_model_files(root, None, &scan_assets(root, &[root.join("assets").join("models")]).models, &mut issues);
    if opts.full {
        check_format_tests(root, &mut issues);
        check_look_parity_tests(root, &mut issues);
        check_converter_tests(root, &mut issues);
        check_net_tests(root, &mut issues);
    }
    check_size_targets(root, &mut issues);
    check_guide_size(root, &mut issues);
    check_blender_addon_isolation(root, &mut issues);
    if !opts.skip_guidance {
        let d = crate::doctor::doctor(root, targets::Options::default());
        let lines = d.lines();
        if !lines.is_empty() {
            let mcp_only = d.problems().is_empty();
            issues.push(Issue::new(
                Severity::Warn,
                if mcp_only { "mcp" } else { "guidance" },
                None,
                lines.join("; "),
                "Run `cargo xtask doctor --fix` (then reconnect `evani-live` with /mcp in Claude Code).",
                "docs/AI_GUIDANCE.md",
            ));
        }
    }
    Report { apps, issues }
}

/// Run the check on one game kept outside the engine (`cargo xtask check
/// --app-dir`), against the engine at `root`: every per-game check (and with
/// `full` its tests and ship build) and the Blender add-on rule on its
/// folders. The engine-wide checks (the engine's games and shared models,
/// catalog targets, guide size, guidance, the engine's own test suites) are
/// the engine's `cargo xtask check`, so they don't run here; a note says so.
/// `opts.apps` is ignored.
pub fn run_game(root: &Path, g: &Game, opts: &Options) -> Report {
    let mut issues = Vec::new();
    let cat = Catalog::load(root);
    let trust = evani_lookguard::verify::Trust::from_store();
    check_game(root, g, &cat, &trust, opts.full, &mut issues);
    let mut dirs = vec![g.dir.clone()];
    dirs.extend(g.kit_dir.iter().filter(|k| !k.starts_with(root)).cloned());
    blender_addon_isolation_in(&g.home, &dirs, &mut issues);
    issues.push(Issue::new(
        Severity::Note,
        "scope",
        Some(&g.id),
        format!("checked the game at {} against the engine at {}; the engine-wide checks are the engine's own", g.shown, root.display()),
        "Nothing to fix here: the engine's `cargo xtask check` (run in the engine) covers its games, shared models, targets and guidance.",
        "README.md",
    ));
    Report { apps: vec![g.id.clone()], issues }
}

/// The per-tier download targets in `catalog/tiers.json` (S7-1): every tier
/// has one and they are sound (boot under total, all under the 10 MB cap,
/// texture memory under the tier's ceiling).
fn check_size_targets(root: &Path, issues: &mut Vec<Issue>) {
    let Ok(text) = std::fs::read_to_string(root.join("catalog").join("tiers.json")) else { return };
    for p in crate::size_targets::problems(&text) {
        issues.push(Issue::new(
            Severity::Error,
            "catalog",
            None,
            p,
            "Set `download_targets` in catalog/tiers.json to the approved table (STAGES.md Stage 4, S4-3): code_kb 2d 350 / 3d 500; per tier boot_kb, total_kb and texture_memory_mb. Changing a target needs Chris's sign-off.",
            "docs/PUBLISHING.md",
        ));
    }
}

/// The last `cargo xtask publish` summary's per-tier verdicts: a tier over
/// its download target is a warning until the game is published again
/// within it (evidence, like the playground size line).
fn check_tier_targets_summary(root: &Path, app: &str, out: &mut Vec<Issue>) {
    let Some(s) = read_json(&root.join(".evani").join("publish").join(app).join("summary.json")) else { return };
    for t in s.pointer("/sizes/tier_targets/tiers").and_then(Value::as_array).into_iter().flatten() {
        if t.get("pass").and_then(Value::as_bool) == Some(false) {
            out.push(Issue::new(
                Severity::Warn,
                "size",
                Some(app),
                format!("last publish summary: {}", t.get("line").and_then(Value::as_str).unwrap_or("a tier is over its download target")),
                format!("Cut the largest rows of `cargo xtask bytes {app}` for that tier (smaller textures, fewer LODs, a lighter boot group), then run `cargo xtask publish {app} --dry-run` again."),
                "docs/PUBLISHING.md",
            ));
        }
    }
}

/// The engine's own always-loaded AI guide block must stay under its size
/// cap (the same measurement the `evani-guide` unit test uses). Only the
/// engine checkout has the template; a game's checkout can't fix it.
fn check_guide_size(root: &Path, issues: &mut Vec<Issue>) {
    guide_size_against(root, crate::render::ROOT_LIMIT, issues);
}

fn guide_size_against(root: &Path, limit: usize, issues: &mut Vec<Issue>) {
    if !root.join("tools/evani-guide/guide/AGENTS.md").is_file() {
        return;
    }
    if let Some(n) = crate::render::root_over(root, limit) {
        issues.push(Issue::new(
            Severity::Error,
            "guidance",
            None,
            format!("the AI guide root block is {n} bytes, at or over its {limit} byte cap"),
            "Shorten tools/evani-guide/guide/AGENTS.md (merge bullets, point to a doc instead of explaining), then run `cargo xtask doctor --fix` to rewrite AGENTS.md.",
            "docs/AI_GUIDANCE.md",
        ));
    }
}

// ---- the Claude Code Stop hook ---------------------------------------------------

/// Most times one session is sent back by the hook; after that it may stop
/// (the hand-over then says what is still failing).
const MAX_BLOCKS: u32 = 3;

/// Files the session's Edit / Write / MultiEdit / NotebookEdit calls touched.
pub fn touched_files(transcript: &str) -> BTreeSet<String> {
    fn walk(v: &Value, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(o) => {
                if o.get("type").and_then(Value::as_str) == Some("tool_use")
                    && matches!(o.get("name").and_then(Value::as_str), Some("Edit" | "Write" | "MultiEdit" | "NotebookEdit"))
                    && let Some(input) = o.get("input")
                {
                    for k in ["file_path", "notebook_path"] {
                        if let Some(p) = input.get(k).and_then(Value::as_str) {
                            out.insert(p.to_string());
                        }
                    }
                }
                o.values().for_each(|x| walk(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    for line in transcript.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if line.contains("tool_use")
            && let Ok(v) = serde_json::from_str::<Value>(line)
        {
            walk(&v, &mut out);
        }
    }
    out
}

/// The games a set of touched files can break.
pub fn games_touched(root: &Path, files: &BTreeSet<String>) -> Vec<String> {
    let games = targets::games(root);
    let mods: Vec<(String, Modules)> = games
        .iter()
        .filter_map(|g| Some((g.clone(), Modules::parse(&std::fs::read_to_string(root.join("apps").join(g).join("evani.toml")).ok()?).ok()?)))
        .collect();
    let mut hit = BTreeSet::new();
    for f in files {
        let p = PathBuf::from(f);
        let rel = if p.is_absolute() { crate::rel(root, &p) } else { f.replace('\\', "/") };
        if Path::new(&rel).is_absolute() || rel.starts_with("..") {
            continue; // outside this project
        }
        let parts: Vec<&str> = rel.split('/').collect();
        match parts.as_slice() {
            ["apps", g, ..] => {
                hit.insert((*g).to_string());
            }
            ["kits", k, ..] => {
                let k = k.strip_prefix("evani-kit-").unwrap_or(k);
                hit.extend(mods.iter().filter(|(_, m)| m.kit == k).map(|(g, _)| g.clone()));
            }
            ["looks", l, ..] => {
                let l = l.strip_prefix("evani-look-").unwrap_or(l);
                hit.extend(mods.iter().filter(|(_, m)| m.look == l).map(|(g, _)| g.clone()));
            }
            ["crates" | "catalog" | "assets", ..] | ["Cargo.toml" | "Cargo.lock"] => hit.extend(games.iter().cloned()),
            _ => {}
        }
    }
    hit.retain(|g| games.contains(g));
    hit.into_iter().collect()
}

/// The Stop hook: `input` is the hook's JSON on stdin. Returns the exit code
/// (2 = keep working) and the text for stderr.
pub fn hook(input: &str, fallback_root: &Path) -> (i32, String) {
    let v: Value = serde_json::from_str(input.trim_start_matches('\u{feff}')).unwrap_or(Value::Null);
    let cwd = v.get("cwd").and_then(Value::as_str).map(PathBuf::from).unwrap_or_else(|| fallback_root.to_path_buf());
    let Some(root) = crate::find_root(&cwd).or_else(|| crate::find_root(fallback_root)) else {
        return (0, String::new());
    };
    let transcript = v.get("transcript_path").and_then(Value::as_str).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let apps = games_touched(&root, &touched_files(&transcript));
    if apps.is_empty() {
        return (0, String::new());
    }
    let report = run(&root, &Options { apps: Some(apps), skip_guidance: true, ..Options::default() });
    if report.passes(false) {
        return (0, String::new());
    }
    let session: String = v.get("session_id").and_then(Value::as_str).unwrap_or("none").chars().filter(char::is_ascii_alphanumeric).take(64).collect();
    let counter = root.join(".evani/check").join(format!("hook-{session}.count"));
    let n: u32 = std::fs::read_to_string(&counter).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    if n >= MAX_BLOCKS {
        return (0, String::new());
    }
    let _ = crate::write_file(&counter, &format!("{}\n", n + 1));
    let mut msg = String::from("Evani check found errors in the games this session changed. Fix them (each has a `fix:` line), then run `cargo xtask check` until it is clean. If one truly can't be fixed now, say so in your hand-over.\n\n");
    msg.push_str(&report.text(false, Severity::Error));
    (2, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options { skip_guidance: true, ..Options::default() }
    }

    fn errors(r: &Report) -> Vec<String> {
        r.issues.iter().filter(|i| i.severity == Severity::Error).map(|i| format!("{}: {}", i.area, i.what)).collect()
    }

    #[test]
    fn a_good_game_is_clean() {
        let root = crate::testutil::fake_engine("check-clean");
        let r = run(&root, &opts());
        assert_eq!(r.apps, ["demo"]);
        assert!(r.passes(false), "{}", r.text(false, Severity::Note));
        assert!(r.summary(false).starts_with("evani check: clean"));
        // The only warning: no geocheck report yet.
        assert!(r.issues.iter().any(|i| i.area == "geocheck" && i.severity == Severity::Warn));
        assert!(!r.passes(true), "--strict counts warnings");
    }

    #[test]
    fn module_list_and_catalog_errors_each_have_a_fix() {
        let root = crate::testutil::fake_engine("check-errors");
        crate::write_file(
            &root.join("apps/demo/evani.toml"),
            "name = \"demo\"\nkit = \"kart-racer\"\nlook = \"pixel16\"\neffects = [\"bloom\", \"ssao\", \"glitter\", \"outline\"]\n\n[modules]\nlive = true\n",
        )
        .unwrap();
        let cargo = std::fs::read_to_string(root.join("apps/demo/Cargo.toml")).unwrap().replace("evani-fx = { workspace = true, optional = true }", "evani-fx.workspace = true");
        crate::write_file(&root.join("apps/demo/Cargo.toml"), &cargo).unwrap();
        let r = run(&root, &opts());
        let e = errors(&r);
        let has = |s: &str| e.iter().any(|x| x.contains(s));
        assert!(has("asks for `ssao`"), "{e:?}");
        assert!(has("evani-kit-kart-racer"), "{e:?}");
        assert!(has("`evani-fx` is a plain dependency"), "{e:?}");
        assert!(has("effect `glitter` is not in catalog"), "{e:?}");
        assert!(has("crates/evani-fx has no `ssao`"), "{e:?}");
        assert!(has("effect `outline` works with 3d"), "{e:?}");
        assert!(has("not one of genre `kart-racer`'s compatible_looks"), "{e:?}");
        assert!(has("physics is `none` but genre `kart-racer` uses `simple`"), "{e:?}");
        assert!(r.issues.iter().all(|i| !i.fix.is_empty() && !i.docs.is_empty()));
        assert!(!r.passes(false));
        let j = r.to_json(false);
        assert_eq!(j["clean"], false);
        assert!(j["errors"].as_u64().unwrap() >= 8);
        assert!(r.text(false, Severity::Note).contains("  fix: "));
    }

    /// The engine's real table, copied into a fake engine with a look of each style.
    fn with_render_paths(name: &str, look: &str, xr: bool) -> PathBuf {
        let root = crate::testutil::fake_engine(name);
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        crate::write_file(&root.join("assets/render_paths.json"), &std::fs::read_to_string(real.join("assets/render_paths.json")).unwrap()).unwrap();
        crate::write_file(
            &root.join("catalog/looks.json"),
            r#"{"looks":[{"id":"toon","style_class":"3d"},{"id":"pixel16","style_class":"2d"},{"id":"mode7","style_class":"pseudo-3d"}]}"#,
        )
        .unwrap();
        crate::write_file(
            &root.join("apps/demo/evani.toml"),
            &format!("name = \"demo\"
kit = \"demo\"
look = \"{look}\"

[modules]
physics = \"simple\"
xr = {xr}
live = true
"),
        )
        .unwrap();
        let cargo = std::fs::read_to_string(root.join("apps/demo/Cargo.toml")).unwrap().replace("evani-look-basic", &format!("evani-look-{look}"));
        crate::write_file(&root.join("apps/demo/Cargo.toml"), &cargo).unwrap();
        root
    }

    fn render_path_errors(r: &Report) -> Vec<&Issue> {
        r.issues.iter().filter(|i| i.area == "render path" && i.severity == Severity::Error).collect()
    }

    #[test]
    fn a_game_on_an_unbuilt_render_path_is_an_error_with_a_fix() {
        let root = with_render_paths("check-unbuilt-path", "mode7", false);
        let r = run(&root, &opts());
        let e = render_path_errors(&r);
        assert_eq!(e.len(), 1, "{}", r.text(false, Severity::Note));
        assert!(e[0].what.contains("pseudo-3d render path is not built on Lite, Low, Medium, High, Ultra"), "{}", e[0].what);
        assert!(e[0].what.contains("Stage 6") && !e[0].fix.is_empty() && e[0].docs == "assets/render_paths.json");
        assert!(!r.passes(false));
        assert!(r.text(false, Severity::Error).contains("  fix: Pick a look"));
    }

    #[test]
    fn a_game_on_a_built_render_path_passes() {
        for look in ["toon", "pixel16"] {
            let root = with_render_paths(&format!("check-built-path-{look}"), look, false);
            let r = run(&root, &opts());
            assert!(render_path_errors(&r).is_empty(), "{look}: {}", r.text(false, Severity::Note));
            assert!(r.issues.iter().any(|i| i.area == "render path" && i.severity == Severity::Note), "{look}: the per-tier report is a note");
        }
    }

    #[test]
    fn the_vr_path_is_reported_but_never_an_error() {
        let root = with_render_paths("check-xr-path", "toon", true);
        let r = run(&root, &opts());
        assert!(render_path_errors(&r).is_empty(), "{}", r.text(false, Severity::Note));
        assert!(r.issues.iter().any(|i| i.area == "render path" && i.what.contains("3d/vr: NOT BUILT")), "the VR line is in the note");
    }

    /// A minimal GLB (header and one JSON chunk, no binary data).
    fn glb(json: &str) -> Vec<u8> {
        let mut j = json.as_bytes().to_vec();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let mut b = b"glTF".to_vec();
        b.extend(2u32.to_le_bytes());
        b.extend((20 + j.len() as u32).to_le_bytes());
        b.extend((j.len() as u32).to_le_bytes());
        b.extend(b"JSON");
        b.extend(j);
        b
    }

    fn module_errors(r: &Report) -> Vec<&Issue> {
        r.issues.iter().filter(|i| i.area == "modules" && i.severity == Severity::Error).collect()
    }

    #[test]
    fn glb_without_models_is_an_error() {
        let root = crate::testutil::fake_engine("check-glb-models");
        assert!(module_errors(&run(&root, &opts())).is_empty(), "a game with no assets is fine");
        std::fs::create_dir_all(root.join("apps/demo/assets")).unwrap();
        std::fs::write(root.join("apps/demo/assets/hero.glb"), glb(r#"{"asset":{"version":"2.0"}}"#)).unwrap();
        let r = run(&root, &opts());
        let e = module_errors(&r);
        assert_eq!(e.len(), 1, "{}", r.text(false, Severity::Note));
        assert!(e[0].what.contains("apps/demo/assets/hero.glb") && e[0].what.contains("`models`"), "{}", e[0].what);
        assert!(e[0].fix.contains("models = true") && e[0].docs == "docs/STAGE3_PLAN.md");
        assert!(!r.passes(false));
        // Listing the module (and giving the crate its feature) clears it.
        let cargo = std::fs::read_to_string(root.join("apps/demo/Cargo.toml")).unwrap();
        crate::write_file(&root.join("apps/demo/Cargo.toml"), &cargo.replace("[features]", "[features]\nmodels = []")).unwrap();
        let toml = std::fs::read_to_string(root.join("apps/demo/evani.toml")).unwrap();
        let toml = if toml.contains("[modules]") { toml.replace("[modules]", "[modules]\nmodels = true") } else { format!("{toml}\n[modules]\nmodels = true\n") };
        crate::write_file(&root.join("apps/demo/evani.toml"), &toml).unwrap();
        let r = run(&root, &opts());
        assert!(module_errors(&r).is_empty(), "{}", r.text(false, Severity::Note));
    }

    #[test]
    fn a_model_the_reader_would_draw_wrong_is_an_error() {
        let root = crate::testutil::fake_engine("check-model-files");
        let models = |r: &Report| r.issues.iter().filter(|i| i.area == "models" && i.severity == Severity::Error).map(|i| i.what.clone()).collect::<Vec<_>>();
        // A game's model with a LOD level whose node isn't there.
        std::fs::create_dir_all(root.join("apps/demo/assets")).unwrap();
        let lost = r#"{"asset":{"version":"2.0"},"nodes":[{"name":"kart_lod0"}],"extras":{"evani":{"format":1,"lods":[{"node":"kart_lod0","min_screen":0.2},{"node":"kart_lod1","min_screen":0}]}}}"#;
        std::fs::write(root.join("apps/demo/assets/kart.low.glb"), glb(lost)).unwrap();
        // A shared model (cargo xtask convert's default folder) the reader refuses,
        // beside a master and a work file that are never checked.
        std::fs::create_dir_all(root.join("assets/models/kart/.work")).unwrap();
        std::fs::write(root.join("assets/models/kart/kart.high.glb"), b"glTF broken").unwrap();
        std::fs::write(root.join("assets/models/kart/kart.master.glb"), b"glTF broken").unwrap();
        std::fs::write(root.join("assets/models/kart/.work/high.packed.glb"), b"glTF broken").unwrap();
        let r = run(&root, &opts());
        let e = models(&r);
        assert_eq!(e.len(), 2, "{}", r.text(false, Severity::Note));
        assert!(e.iter().any(|w| w.contains("apps/demo/assets/kart.low.glb") && w.contains("`kart_lod1` is not in the file")), "{e:?}");
        assert!(e.iter().any(|w| w.contains("assets/models/kart/kart.high.glb") && w.contains("refuses it")), "{e:?}");
        assert!(r.issues.iter().filter(|i| i.area == "models").all(|i| i.fix.contains("cargo xtask convert")));
        // Fixed files clear it.
        std::fs::write(root.join("apps/demo/assets/kart.low.glb"), glb(&lost.replace(r#",{"node":"kart_lod1","min_screen":0}"#, ""))).unwrap();
        std::fs::remove_file(root.join("assets/models/kart/kart.high.glb")).unwrap();
        let r = run(&root, &opts());
        assert!(models(&r).is_empty(), "{}", r.text(false, Severity::Note));
    }

    #[test]
    fn a_skinned_model_needs_anim_and_a_font_needs_ui() {
        let m = |models: bool, anim: bool, ui: bool| Modules { models, anim, ui, ..Modules::default() };
        let scan = AssetScan {
            models: vec!["apps/demo/a.glb".into(), "apps/demo/b.glb".into()],
            skinned: vec!["apps/demo/b.glb".into()],
            fonts: vec!["apps/demo/f.ttf".into()],
            untangent: Vec::new(),
        };
        let p = module_problems("apps/demo", "demo", &m(true, false, false), &scan);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].0.contains("skin") && p[0].1.contains("anim = true"));
        assert!(p[1].0.contains("font") && p[1].1.contains("ui = true"));
        assert!(module_problems("apps/demo", "demo", &m(true, true, true), &scan).is_empty());
        assert_eq!(module_problems("apps/demo", "demo", &m(false, false, true), &scan).len(), 2, "models and anim both missing");
        assert!(has_skin(&glb(r#"{"skins":[{"joints":[0]}]}"#)));
        assert!(!has_skin(&glb(r#"{"skins":[]}"#)));
        assert!(has_skin(br#"{"skins":[{"joints":[0]}]}"#), "a text .gltf works too");
        assert!(!has_skin(b"glTF"), "a cut-off file is not a skin");
    }

    #[test]
    fn a_normal_map_without_tangents_is_an_error() {
        let mapped = r#"{"materials":[{"normalTexture":{"index":0}},{}],"meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1},"material":0}]}]}"#;
        assert!(normal_map_without_tangents(&glb(mapped)));
        assert!(!normal_map_without_tangents(&glb(&mapped.replace(r#""NORMAL":1"#, r#""NORMAL":1,"TANGENT":2"#))), "with tangents");
        assert!(!normal_map_without_tangents(&glb(&mapped.replace(r#""material":0"#, r#""material":1"#))), "no normal map");
        assert!(!normal_map_without_tangents(&glb(&mapped.replace(r#","material":0"#, ""))), "no material");
        assert!(!normal_map_without_tangents(b"glTF"), "a cut-off file is not flagged");

        let root = crate::testutil::fake_engine("check-glb-tangents");
        std::fs::create_dir_all(root.join("apps/demo/assets")).unwrap();
        std::fs::write(root.join("apps/demo/assets/hero.glb"), glb(mapped)).unwrap();
        let r = run(&root, &opts());
        // The stub file has no accessors, so the shared model check
        // (`modelcheck`) refuses it too; the tangents error is its own issue.
        let e: Vec<&Issue> = r.issues.iter().filter(|i| i.area == "models" && i.severity == Severity::Error && i.what.contains("tangents")).collect();
        assert_eq!(e.len(), 1, "{}", r.text(false, Severity::Note));
        assert!(e[0].what.contains("apps/demo/assets/hero.glb") && e[0].what.contains("tangents") && e[0].fix.contains("Tangents"), "{}", e[0].what);
        assert!(!r.passes(false));
    }

    #[test]
    fn a_playground_must_support_free_roam() {
        let root = crate::testutil::fake_engine("check-playground");
        let toml = std::fs::read_to_string(root.join("apps/demo/evani.toml")).unwrap();
        crate::write_file(&root.join("apps/demo/evani.toml"), &format!("{toml}\n[playground]\n")).unwrap();
        let r = run(&root, &opts());
        let p: Vec<&Issue> = r.issues.iter().filter(|i| i.area == "playground").collect();
        assert_eq!(p.len(), 1, "{}", r.text(false, Severity::Note));
        assert_eq!(p[0].severity, Severity::Error);
        assert!(p[0].what.contains("doesn't declare free roam") && p[0].fix.contains("free_roam_test") && p[0].docs == "docs/PLAYGROUNDS.md");
        assert!(!r.passes(false));

        crate::write_file(&root.join("apps/demo/evani.toml"), &format!("{toml}\n[playground]\nfree_roam = true\n")).unwrap();
        assert_eq!(errors(&run(&root, &opts())).len(), 2, "no kit declaration, no test");
        crate::write_file(
            &root.join("kits/evani-kit-demo/src/lib.rs"),
            "impl KitPlugin for Demo {\n    fn free_roam(&self) -> bool {\n        true\n    }\n}\n#[test]\nfn roams() {\n    evani_core::play::free_roam_test(app(), 30.0, &[]).unwrap();\n}\n",
        )
        .unwrap();
        let r = run(&root, &opts());
        assert!(r.passes(false), "{}", r.text(false, Severity::Note));
    }

    #[test]
    fn evidence_counts_only_when_newer_than_the_sources() {
        let root = crate::testutil::fake_engine("check-evidence");
        let report = json!({"issues":[{"check":"floating","detail":"crate 3 floats 0.4 m","fix":"Lower crate 3 by 0.4 m.","tagged":false},{"check":"overlap","detail":"x","tagged":"on purpose"}]});
        crate::write_file(&root.join(".vibedeck/geocheck/demo/report.json"), &report.to_string()).unwrap();
        let card = json!({"checklist":[{"n":7,"status":"fail","text":"No floating props.","evidence":"2 props float."},{"n":8,"status":"pass","text":"x","evidence":"y"}]});
        crate::write_file(&root.join(".vibedeck/review/demo/card.json"), &card.to_string()).unwrap();
        let r = run(&root, &opts());
        let e = errors(&r);
        assert!(e.iter().any(|x| x.contains("floating: crate 3 floats")), "{e:?}");
        assert!(r.issues.iter().any(|i| i.fix.starts_with("Lower crate 3 by 0.4 m.")));
        assert!(e.iter().any(|x| x.contains("review card line 7 fails: 2 props float.")), "{e:?}");
        assert_eq!(e.len(), 2, "the tagged issue and the passing line don't count: {e:?}");

        // A source change after the evidence: it no longer counts as an error.
        std::thread::sleep(std::time::Duration::from_millis(30));
        crate::write_file(&root.join("kits/evani-kit-demo/src/lib.rs"), "// changed\n").unwrap();
        let r = run(&root, &opts());
        assert!(r.passes(false), "{}", r.text(false, Severity::Note));
        let warns: Vec<&Issue> = r.issues.iter().filter(|i| i.severity == Severity::Warn).collect();
        assert!(warns.iter().any(|i| i.area == "geocheck" && i.what.contains("older than")));
        assert!(warns.iter().any(|i| i.area == "review" && i.fix.contains("cargo xtask review demo")));
    }

    #[test]
    fn a_partial_geometry_report_is_refused() {
        let root = crate::testutil::fake_engine("check-partial");
        let partial = json!({"check":"zfight","partial":true,"cameras":50,"cameras_total":700,"issues":[]});
        // In the z-fight folder (the full report.json is clean).
        crate::write_file(&root.join(".vibedeck/geocheck/demo/report.json"), &json!({"issues":[]}).to_string()).unwrap();
        crate::write_file(&root.join(".vibedeck/geocheck/demo/zfight/zfight.json"), &partial.to_string()).unwrap();
        let r = run(&root, &opts());
        let e = errors(&r);
        assert!(e.iter().any(|x| x.contains("zfight/zfight.json is partial (50 of 700 cameras)")), "{e:?}");
        assert!(!r.passes(false));
        // As report.json: refused, and not read as evidence.
        crate::write_file(&root.join(".vibedeck/geocheck/demo/zfight/zfight.json"), &json!({"partial":false,"issues":[]}).to_string()).unwrap();
        crate::write_file(&root.join(".vibedeck/geocheck/demo/report.json"), &partial.to_string()).unwrap();
        let r = run(&root, &opts());
        let e = errors(&r);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("report.json is partial"), "{e:?}");
        assert!(r.issues.iter().any(|i| i.fix.contains("without --sample")));
        // A full report passes again.
        crate::write_file(&root.join(".vibedeck/geocheck/demo/report.json"), &json!({"partial":false,"issues":[]}).to_string()).unwrap();
        let r = run(&root, &opts());
        assert!(r.passes(false), "{}", r.text(false, Severity::Note));
        assert_eq!(crate::reports::partial(&json!({"partial":true})).as_deref(), Some("a sample"));
        assert_eq!(crate::reports::partial(&json!({"issues":[]})), None);
    }

    #[test]
    fn unknown_game_and_guidance_warnings() {
        let root = crate::testutil::fake_engine("check-misc");
        let r = run(&root, &Options { apps: Some(vec!["nope".into()]), ..Options::default() });
        assert!(errors(&r).iter().any(|x| x.contains("no game `nope`")));
        assert!(r.issues.iter().any(|i| i.area == "guidance" && i.fix.contains("doctor --fix")));
    }

    #[test]
    fn guide_block_size_is_an_error_with_a_fix() {
        let root = crate::testutil::fake_engine("check-guide-size");
        let mut issues = Vec::new();
        // A checkout without the template (a game's) is left alone.
        guide_size_against(&root, 10, &mut issues);
        assert!(issues.is_empty());
        // The file is only a marker that this is the engine checkout; the
        // measured size comes from the compiled-in template.
        crate::write_file(&root.join("tools/evani-guide/guide/AGENTS.md"), "x").unwrap();
        guide_size_against(&root, crate::render::ROOT_LIMIT, &mut issues);
        assert!(issues.is_empty(), "the shipped block is under the cap");
        guide_size_against(&root, 10, &mut issues);
        assert_eq!(issues.len(), 1);
        assert!(matches!(issues[0].severity, Severity::Error));
        assert!(issues[0].what.contains("byte cap") && issues[0].fix.contains("doctor --fix"));
    }

    #[test]
    fn material_modules_must_match_the_crates_and_the_kit() {
        let root = crate::testutil::fake_engine("check-material-modules");
        crate::write_file(&root.join("crates/evani-material-nature/Cargo.toml"), "[package]\nname = \"evani-material-nature\"\n[features]\ndefault = []\nocean = []\n").unwrap();
        crate::write_file(&root.join("crates/evani-material-proc/Cargo.toml"), "[package]\nname = \"evani-material-proc\"\n[features]\ndefault = []\nsky = []\n").unwrap();
        let m = |mods: &str| Modules::parse(&format!("name = \"demo\"\nkit = \"demo\"\nlook = \"basic\"\n[modules]\n{mods}\n")).unwrap();
        let kit_opt = "[dependencies]\nevani-material-nature = { workspace = true, optional = true }\nevani-material-proc = { workspace = true, optional = true }\n";
        let p = |mods: &str, app: &str, kit: &str| material_problems(&root, "apps/demo", "kits/evani-kit-demo", &m(mods), app, kit);
        // Listed parts the crates have, the crates optional in the kit: clean.
        assert!(p("nature = [\"ocean\"]\nproc = [\"sky\"]", "", kit_opt).is_empty());
        // Or in the app.
        assert!(p("nature = []", "[dependencies]\nevani-material-nature = { workspace = true, optional = true }\n", "").is_empty());
        // A part the crate doesn't have.
        let e = p("nature = [\"lava\"]", "", kit_opt);
        assert!(e.len() == 1 && e[0].0 == Severity::Error && e[0].1.contains("`lava`") && e[0].1.contains("ocean"), "{e:?}");
        // Listed, but nothing depends on the crate.
        let e = p("proc = [\"sky\"]", "", "");
        assert!(e.len() == 1 && e[0].1.contains("neither the app nor the kit") && e[0].2.contains("optional = true"), "{e:?}");
        // A plain dependency in the kit: every game on it would download the crate.
        let e = p("", "", "[dependencies]\nevani-material-nature.workspace = true\n");
        assert!(e.len() == 1 && e[0].1.contains("`evani-material-nature` as a plain dependency") && e[0].2.contains("kits/evani-kit-demo/Cargo.toml"), "{e:?}");
        // Through the whole check: the game lists the sea but nothing has the crate.
        crate::write_file(&root.join("apps/demo/evani.toml"), "name = \"demo\"\nkit = \"demo\"\nlook = \"basic\"\neffects = [\"bloom\"]\n\n[modules]\nphysics = \"simple\"\nlive = true\nnature = [\"ocean\"]\n").unwrap();
        let r = run(&root, &Options { apps: Some(vec!["demo".into()]), ..opts() });
        let text = r.text(false, Severity::Note);
        assert!(!r.passes(false) && text.contains("asks for `nature`") && text.contains("neither the app nor the kit"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn blender_addon_stays_separate_from_engine_and_games() {
        let root = crate::testutil::fake_engine("check-blender-addon");
        let mut issues = Vec::new();
        crate::write_file(&root.join("crates/evani-core/src/lib.rs"), "// plain code\n").unwrap();
        crate::write_file(&root.join("tools/blender-addon/evani_export/core.py"), "# the add-on itself may name its own folder: tools/blender-addon\n").unwrap();
        check_blender_addon_isolation(&root, &mut issues);
        assert!(issues.is_empty(), "the add-on folder itself is not scanned");
        crate::write_file(&root.join("apps/demo/src/main.rs"), "const A: &str = include_str!(\"../../../tools/blender-addon/evani_export/core.py\");\n").unwrap();
        check_blender_addon_isolation(&root, &mut issues);
        assert_eq!(issues.len(), 1);
        assert!(matches!(issues[0].severity, Severity::Error));
        assert!(issues[0].what.contains("apps/demo/src/main.rs") && issues[0].fix.contains("separate GPL folder"));
    }

    #[test]
    fn hook_scope_and_blocking() {
        let root = crate::testutil::fake_engine("check-hook");
        let file = root.join("kits/evani-kit-demo/src/lib.rs");
        let line = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":file.display().to_string()}}]}});
        let other = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"apps/x/a.rs"}}]}});
        let transcript = format!("{line}\n{other}\nnot json\n");
        let touched = touched_files(&transcript);
        assert_eq!(touched.len(), 1);
        assert_eq!(games_touched(&root, &touched), ["demo"]);
        // A byte-order mark (a transcript or input written by Windows PowerShell) is ignored.
        assert_eq!(touched_files(&format!("\u{feff}{transcript}")), touched);
        let mut f = BTreeSet::new();
        f.insert("catalog/looks.json".to_string());
        f.insert("docs/README.md".to_string());
        assert_eq!(games_touched(&root, &f), ["demo"]);
        f.clear();
        f.insert("docs/x.md".into());
        assert!(games_touched(&root, &f).is_empty());

        let tpath = root.join("transcript.jsonl");
        std::fs::write(&tpath, &transcript).unwrap();
        let input = json!({"session_id":"abc","transcript_path":tpath.display().to_string(),"cwd":root.display().to_string(),"hook_event_name":"Stop"}).to_string();
        assert_eq!(hook(&input, &root).0, 0, "a clean game lets the turn end");
        crate::write_file(&root.join("apps/demo/evani.toml"), "name = \"demo\"\nkit = \"demo\"\nlook = \"basic\"\neffects = [\"ssao\"]\n").unwrap();
        for i in 0..MAX_BLOCKS {
            let input = if i == 0 { format!("\u{feff}{input}") } else { input.clone() };
            let (code, msg) = hook(&input, &root);
            assert_eq!(code, 2);
            assert!(msg.contains("ERROR demo · manifest") && msg.contains("fix:"), "{msg}");
        }
        assert_eq!(hook(&input, &root).0, 0, "never traps a session forever");
        // No edits in the session: nothing to check.
        std::fs::write(&tpath, "").unwrap();
        let fresh = json!({"session_id":"new","transcript_path":tpath.display().to_string(),"cwd":root.display().to_string()}).to_string();
        assert_eq!(hook(&fresh, &root).0, 0);
    }

    /// The 3D path rule: on exactly when the game draws 3D.
    #[test]
    fn forward_must_be_on_exactly_when_the_game_draws_3d() {
        let on = "[dependencies]\nevani-render = { workspace = true, features = [\"forward\"] }\n";
        let meshes = "[dependencies]\nevani-render = { workspace = true, features = [\"meshes\"] }\n";
        let off = "[dependencies]\nevani-render.workspace = true\n";
        let px = "[dependencies]\nevani-render = { workspace = true, features = [\"pixel-art\"] }\n";
        let none: &[String] = &[];
        let sev = |p: Option<(Severity, String, String)>| p.map(|(s, _, _)| s);
        // 3D games with the path on (directly, through `meshes`, the app, or an effect).
        assert_eq!(forward_problem("kits/evani-kit-k", Some("3d"), None, on, off, none), None);
        assert_eq!(forward_problem("kits/evani-kit-k", Some("3d"), Some("src/a.rs uses `MeshInstance`"), meshes, off, none), None);
        assert_eq!(forward_problem("kits/evani-kit-k", Some("3d"), None, off, on, none), None);
        assert_eq!(forward_problem("kits/evani-kit-k", Some("3d"), None, off, off, &["outline".to_string()]), None);
        // A 3D look, or a kit that draws meshes, without it: an error naming the fix.
        let (s, what, fix) = forward_problem("kits/evani-kit-k", Some("3d"), None, off, off, none).unwrap();
        assert_eq!(s, Severity::Error);
        assert!(what.contains("`3d`") && what.contains("dropped"), "{what}");
        assert!(fix.contains("kits/evani-kit-k/Cargo.toml") && fix.contains("features = [\"forward\"]"), "{fix}");
        let (s, what, _) = forward_problem("kits/evani-kit-k", Some("2d"), Some("src/draw.rs uses `Space::World3d`"), px, off, none).unwrap();
        assert_eq!(s, Severity::Error);
        assert!(what.contains("Space::World3d"), "{what}");
        // 2D games: off is right; on without 3D draws is a warning.
        assert_eq!(forward_problem("kits/evani-kit-k", Some("2d"), None, px, off, none), None);
        assert_eq!(sev(forward_problem("kits/evani-kit-k", Some("2d"), None, on, off, none)), Some(Severity::Warn));
        // The renderer line only: other crates' features don't count.
        assert!(!dep_turns_on_forward("evani-render-extra = { features = [\"forward\"] }\n"));
        assert!(!dep_turns_on_forward("live = [\"evani-render/hooks\"]\nevani-render.workspace = true\n"));
    }

    /// The repo's own games pass it: the 3D kits turn `forward` on and the
    /// 2D kits don't draw 3D.
    #[test]
    fn the_repo_games_follow_the_forward_rule() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (kit, style) in [("kart-racer", "3d"), ("example", "3d"), ("2d-platformer", "2d"), ("match-3", "2d")] {
            let dir = root.join("kits").join(format!("evani-kit-{kit}"));
            let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            let draws = first_3d_draw(&dir.join("src"));
            assert_eq!(draws.is_some(), style == "3d", "{kit}: {draws:?}");
            assert_eq!(forward_problem(&format!("kits/evani-kit-{kit}"), Some(style), draws.as_deref(), &cargo, "", &[]), None, "{kit}");
        }
    }

    /// `--full`'s look-parity run names a test file, features and a document
    /// that exist, so a rename can't turn the guard into a cargo error.
    #[test]
    fn look_parity_tests_point_at_real_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (krate, extra, docs) in LOOK_PARITY_TESTS {
            let dir = root.join("crates").join(krate);
            let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            let at = |flag: &str| extra.iter().position(|a| *a == flag).map(|i| extra[i + 1]);
            if let Some(test) = at("--test") {
                assert!(dir.join("tests").join(format!("{test}.rs")).is_file(), "{krate}: tests/{test}.rs");
            } else {
                assert!(extra.contains(&"--lib"), "{krate}: names neither --test nor --lib");
            }
            for f in at("--features").unwrap_or("").split(',').filter(|f| !f.is_empty()) {
                assert!(cargo.contains(&format!("\n{f} = ")), "{krate} has no feature {f}");
            }
            assert!(root.join(docs).is_file(), "{docs}");
        }
    }

    /// `--full`'s netcode run names a crate, test file and document that exist.
    #[test]
    fn net_tests_point_at_real_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (krate, test, docs) in NET_TESTS {
            let dir = root.join("crates").join(krate);
            assert!(dir.join("Cargo.toml").is_file(), "{krate}");
            assert!(dir.join("tests").join(test).join("main.rs").is_file(), "{krate}: tests/{test}/main.rs");
            assert!(root.join(docs).is_file(), "{docs}");
        }
    }

    /// `check --app-dir`: one game in its own workspace (app and kit folders
    /// of its own), checked with the engine's per-game rules; its evidence is
    /// read from its workspace and the messages name its folder and
    /// `--app-dir`, never `apps/<id>`.
    #[test]
    fn a_game_outside_the_engine_is_checked_in_its_own_folders() {
        let root = crate::testutil::fake_engine("check-outside-engine");
        let home = crate::testutil::temp_root("check-outside-game");
        let w = |p: &str, s: &str| crate::write_file(&home.join(p), s).unwrap();
        // The engine's demo, moved into its own workspace as app/ and kit/.
        for (from, to) in [
            ("apps/demo/evani.toml", "app/evani.toml"),
            ("apps/demo/Cargo.toml", "app/Cargo.toml"),
            ("apps/demo/src/main.rs", "app/src/main.rs"),
            ("kits/evani-kit-demo/Cargo.toml", "kit/Cargo.toml"),
            ("kits/evani-kit-demo/src/lib.rs", "kit/src/lib.rs"),
        ] {
            w(to, &std::fs::read_to_string(root.join(from)).unwrap());
        }
        let g = Game::outside("demo", &home.join("app"), &home, &home.join("kit"));
        assert!(g.is_outside() && g.arg == format!("--app-dir {}", home.join("app").display()));
        let r = run_game(&root, &g, &opts());
        assert_eq!(r.apps, ["demo"]);
        assert!(r.passes(false), "{}", r.text(false, Severity::Note));
        assert!(r.issues.iter().any(|i| i.area == "scope" && i.severity == Severity::Note), "the scope note");
        // No geometry report yet: a 3D game outside the engine is told about its own adapter.
        let geo = r.issues.iter().find(|i| i.area == "geocheck").unwrap();
        assert!(geo.severity == Severity::Warn && geo.fix.contains("examples/geocheck.rs") && geo.fix.contains(&g.arg), "{geo:?}");
        // The same game inside the engine gets the same verdict.
        let inside = run(&root, &Options { apps: Some(vec!["demo".into()]), ..opts() });
        assert_eq!(inside.count(Severity::Error), r.count(Severity::Error));
        // Evidence in the game's workspace counts, with fixes naming --app-dir.
        w(".vibedeck/review/demo/card.json", r#"{"checklist":[{"n":3,"status":"fail","text":"the sea reads","evidence":"too dark"}]}"#);
        w(".vibedeck/geocheck/demo/report.json", r#"{"issues":[{"check":"floating","kind":"floating","tagged":false}]}"#);
        let r = run_game(&root, &g, &opts());
        let errors: Vec<&Issue> = r.issues.iter().filter(|i| i.severity == Severity::Error).collect();
        assert!(errors.iter().any(|i| i.area == "review" && i.fix.contains(&format!("cargo xtask review {}", g.arg))), "{errors:?}");
        assert!(errors.iter().any(|i| i.area == "geocheck" && i.what.contains("floating")), "{errors:?}");
        assert!(run(&root, &Options { apps: Some(vec!["demo".into()]), ..opts() }).passes(false), "the engine's own demo has no such evidence");
        // A catalog fix names the game's folder too (an effect the catalog lacks).
        let toml = std::fs::read_to_string(home.join("app/evani.toml")).unwrap();
        w("app/evani.toml", &toml.replace("effects = [\"bloom\"]", "effects = [\"glitter\"]"));
        let r = run_game(&root, &g, &opts());
        let cat = r.issues.iter().find(|i| i.area == "catalog" && i.what.contains("glitter")).unwrap();
        assert!(cat.fix.contains(&format!("{}/evani.toml", g.shown)), "{cat:?}");
        w("app/evani.toml", &toml);
        // A plain dependency on an optional engine crate: an ERROR naming the game's folder.
        let cargo = std::fs::read_to_string(home.join("app/Cargo.toml")).unwrap();
        w("app/Cargo.toml", &cargo.replace("evani-live = { workspace = true, optional = true }", "evani-live.workspace = true"));
        let r = run_game(&root, &g, &opts());
        let plain = r.issues.iter().find(|i| i.area == "manifest" && i.what.contains("`evani-live` is a plain dependency")).unwrap();
        assert!(plain.severity == Severity::Error && plain.fix.contains(&format!("In {}/Cargo.toml", g.shown)), "{plain:?}");
        assert!(r.issues.iter().all(|i| !i.fix.contains("apps/demo") && !i.what.contains("apps/demo")), "{}", r.text(false, Severity::Note));
        // The Blender add-on rule covers the game's own folders.
        w("kit/src/lib.rs", "// see tools/blender-addon\n");
        let r = run_game(&root, &g, &opts());
        assert!(r.issues.iter().any(|i| i.area == "licence" && i.what.contains("kit/src/lib.rs")), "{}", r.text(false, Severity::Note));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }
}
