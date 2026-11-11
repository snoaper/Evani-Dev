//! Engine versions for games outside the engine (docs/ENGINE_PACK.md 4.4-4.5):
//!
//! - `cargo xtask engines [--dir <engines folder>] [--games <folder>].. [--live <version>] [--apply] [--json]`:
//!   the installed packs, the games found and their pins, and the retention
//!   plan (live + every pinned pack, at most 3 extra; renames between
//!   Evani.App's build and keep classes, never a deletion). `--apply` carries
//!   it out and relinks the games whose pack folder moved.
//! - `cargo xtask relink --app-dir <app>`: point a game at this engine again
//!   (after its pack folder was renamed, or the game moved to another PC).
//! - `cargo xtask upgrade-engine --app-dir <app> [--to <version>] [--dry-run] [--no-pictures] [--max-diff <percent>] [--shot-ms <ms>] [--chrome <exe>]`:
//!   move a game to this engine: a copy is pinned, relinked, built (ship) and
//!   checked first; the game itself changes only when that passes. The start
//!   view of the game on its old engine and of the copy on this one are
//!   captured (headless, muted browser) beside the upgrade report, and a
//!   large pixel difference is flagged (start_view.rs).
//!
//! The rules themselves (pins, installed packs, the plan, relinking) are in
//! `evani_guide::engine_version`, shared with `check` and the builds.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use evani_guide::engine_version::{self as ev, Step};
use serde_json::{Value, json};

use crate::start_view;

pub fn usage() -> &'static str {
    "cargo xtask engines [--dir <engines folder>] [--games <folder>].. [--live <version>] [--apply] [--json]
                                                      installed engine packs, the games' pins and which old packs stay (never deletes)
cargo xtask relink --app-dir <game app folder>        point a game outside the engine at this engine again (same version)
cargo xtask upgrade-engine --app-dir <game app folder> [--to <version>] [--dry-run] [--no-pictures] [--max-diff <percent, default 10>] [--shot-ms <ms, default 5000>] [--chrome <exe>]
                                                      move a game to this engine: a copy is built and checked first, the pin changes only if it passes;
                                                      before-and-after pictures of its start view and upgrade-report.json/.md go in the upgrade folder"
}

pub fn value(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().map(Some).ok_or_else(|| format!("{name} needs a value")),
    }
}

/// `engines ...`
pub fn cmd_engines(root: &Path, args: &[String]) -> Result<(), String> {
    let mut games_dirs = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--games" => {
                games_dirs.push(PathBuf::from(args.get(i + 1).ok_or("--games needs a folder")?));
                i += 1;
            }
            "--dir" | "--live" => i += 1,
            "--apply" | "--json" => {}
            other => return Err(format!("engines: unknown argument `{other}`\n{}", usage())),
        }
        i += 1;
    }
    let dir = match value(args, "--dir")? {
        Some(d) => PathBuf::from(d),
        None => ev::engines_dir(root).ok_or("engines: this engine is not an installed pack, so there is no engines folder; give it with --dir <folder> (or EVANI_ENGINES_DIR)")?,
    };
    let live = value(args, "--live")?;
    if let Some(v) = &live
        && !ev::valid_version(v)
    {
        return Err(format!("--live {v}: not an engine version (like 0.1.0)"));
    }
    let packs = ev::installed(&dir);
    let games = ev::find_games(&games_dirs, 6);
    let plan = ev::plan(&packs, &games, live.as_deref());
    let applied = if args.iter().any(|a| a == "--apply") { Some(ev::apply(&dir, &plan, &games)) } else { None };
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&plan_json(&dir, &packs, &games, &plan, applied.as_deref())).unwrap_or_default());
    } else {
        print_plan(&dir, &packs, &games, &plan, applied.as_deref());
    }
    Ok(())
}

fn dir_bytes(d: &Path) -> u64 {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(_) => e.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}

fn step_json(s: &Step) -> Value {
    match s {
        Step::Rename { version, from, to, why } => json!({ "action": "rename", "version": version, "from": from, "to": to, "why": why }),
        Step::MarkLive { version, dir } => json!({ "action": "mark-live", "version": version, "dir": dir }),
        Step::Duplicate { version, dirs } => json!({ "action": "duplicate", "version": version, "dirs": dirs }),
    }
}

/// The JSON Evani.App reads (schema `evani-engines/1`).
fn plan_json(dir: &Path, packs: &[ev::Installed], games: &[ev::FoundGame], plan: &ev::Plan, applied: Option<&[String]>) -> Value {
    json!({
        "schema": "evani-engines/1",
        "engines_dir": dir,
        "live": plan.live,
        "max_kept": ev::MAX_KEPT,
        "packs": packs.iter().map(|p| json!({
            "version": p.version,
            "dir": p.dir,
            "class": match p.class { ev::Class::Build => "build", ev::Class::Keep => "keep", ev::Class::Other => "other" },
            "live": p.live,
            "bytes": dir_bytes(&p.dir),
            "games": games.iter().filter(|g| g.pin.as_deref() == Some(p.version.as_str())).map(|g| &g.app_dir).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "games": games.iter().map(|g| json!({ "app_dir": g.app_dir, "pin": g.pin })).collect::<Vec<_>>(),
        "steps": plan.steps.iter().map(step_json).collect::<Vec<_>>(),
        "kept": plan.kept.iter().map(|(v, g)| json!({ "version": v, "games": g })).collect::<Vec<_>>(),
        "over_cap": plan.over_cap,
        "missing": plan.missing.iter().map(|(v, g)| json!({ "version": v, "games": g })).collect::<Vec<_>>(),
        "applied": applied,
    })
}

fn print_plan(dir: &Path, packs: &[ev::Installed], games: &[ev::FoundGame], plan: &ev::Plan, applied: Option<&[String]>) {
    println!("engines in {}:", dir.display());
    if packs.is_empty() {
        println!("  (none)");
    }
    for p in packs {
        let users = games.iter().filter(|g| g.pin.as_deref() == Some(p.version.as_str())).count();
        println!(
            "  {}{}  {:.0} MB  {} game(s)  {}",
            p.version,
            if plan.live.as_deref() == Some(p.version.as_str()) { " (live)" } else { "" },
            dir_bytes(&p.dir) as f64 / 1e6,
            users,
            p.dir.display()
        );
    }
    let unpinned = games.iter().filter(|g| g.pin.is_none()).count();
    println!("games found: {} ({unpinned} not pinned: they use the live engine and are pinned on their first build)", games.len());
    for (v, g) in &plan.missing {
        println!("  pinned to engine {v}, which is not installed: {}  (fix: `cargo xtask upgrade-engine --app-dir <game>`)", g.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "));
    }
    if plan.steps.is_empty() {
        println!("plan: nothing to change");
    } else {
        println!("plan:");
        for s in &plan.steps {
            match s {
                Step::Rename { version, to, why, .. } => println!("  rename engine {version} to {} ({why})", to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                Step::MarkLive { version, .. } => println!("  mark engine {version} live"),
                Step::Duplicate { version, dirs } => println!("  engine {version} is in {} folders: left alone", dirs.len()),
            }
        }
    }
    if plan.over_cap {
        println!(
            "over the limit: {} old engines are kept for games (at most {}). Ask the user which game to upgrade, or which old engine to remove: {}",
            plan.kept.len(),
            ev::MAX_KEPT,
            plan.kept.iter().map(|(v, g)| format!("{v} ({} game(s))", g.len())).collect::<Vec<_>>().join(", ")
        );
    }
    match applied {
        Some(log) => {
            for l in log {
                println!("  {l}");
            }
        }
        None if !plan.steps.is_empty() => println!("(plan only: --apply carries it out)"),
        None => {}
    }
}

/// `relink --app-dir <dir>`
pub fn cmd_relink(root: &Path, dir: &Path) -> Result<(), String> {
    let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let changed = ev::relink(&dir, root)?;
    if changed.is_empty() {
        println!("{} already builds against this engine ({})", dir.display(), root.display());
    }
    for f in changed {
        println!("relinked {} to the engine at {}", f.display(), root.display());
    }
    Ok(())
}

/// Copy the game workspace `from` to `to` without build output, git or
/// tool folders.
fn copy_game(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if matches!(n.as_ref(), "target" | "dist" | ".git" | "node_modules") || n.starts_with(".evani") {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            copy_game(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// Pin `app_dir` to this engine, point it at this engine and take this
/// engine's `Cargo.lock` for its workspace `ws`.
fn move_to_engine(root: &Path, app_dir: &Path, ws: &Path, version: &str) -> Result<(), String> {
    let toml_path = app_dir.join("evani.toml");
    let text = std::fs::read_to_string(&toml_path).map_err(|e| format!("{}: {e}", toml_path.display()))?;
    std::fs::write(&toml_path, ev::set_pin(&text, version)).map_err(|e| format!("{}: {e}", toml_path.display()))?;
    ev::relink(app_dir, root)?;
    std::fs::copy(root.join("Cargo.lock"), ws.join("Cargo.lock")).map_err(|e| format!("{}: {e}", ws.join("Cargo.lock").display()))?;
    Ok(())
}

/// What `upgrade-engine` needs from the build tools: main.rs gives the real
/// ones, the tests stand-ins.
pub struct UpgradeTools<'a> {
    /// Build (ship) and check the copy's app folder with this engine; returns its ship folder.
    pub build_and_check: &'a dyn Fn(&Path) -> Result<PathBuf, String>,
    /// Build the game (its app folder) with the installed pack in the given
    /// folder (its own `evani`); returns the game's ship folder.
    pub build_with_pack: &'a dyn Fn(&Path, &Path) -> Result<PathBuf, String>,
    /// The start view of a ship folder (`dist`, game id) as a PNG.
    pub shoot: &'a dyn Fn(&Path, &str, &Path) -> Result<(), String>,
}

/// Build the game in `app_dir` (ship) with the installed pack `pack`'s own
/// `evani`, in that pack's environment (docs/ENGINE_PACK.md 4.2): its tools
/// and toolchain first on the PATH, its rustc, engine and converter tools,
/// and (when the per-user Cargo home uses the packs' vendor source) its own
/// vendored crates. Returns the game's ship folder.
pub fn build_with_pack(pack: &Path, app_dir: &Path) -> Result<PathBuf, String> {
    let evani = pack.join("tools").join("bin").join(if cfg!(windows) { "evani.exe" } else { "evani" });
    if !evani.is_file() {
        return Err(format!("{} is missing", evani.display()));
    }
    let mut cmd = Command::new(&evani);
    cmd.args(["build", "--app-dir"]).arg(app_dir).args(["--ship", "--no-diff"]);
    cmd.env("EVANI_ENGINE_ROOT", pack.join("engine")).env("EVANI_TOOLS_DIR", pack.join("tools").join("convert")).env_remove("RUSTUP_TOOLCHAIN").stdin(Stdio::null());
    let tc = pack.join("toolchain").join("bin");
    if tc.is_dir() {
        let mut paths = vec![pack.join("tools").join("bin"), tc.clone()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        cmd.env("PATH", std::env::join_paths(paths).map_err(|e| e.to_string())?);
        cmd.env("RUSTC", tc.join(if cfg!(windows) { "rustc.exe" } else { "rustc" }));
        let home_cfg = std::env::var_os("CARGO_HOME").map(|h| PathBuf::from(h).join("config.toml"));
        if home_cfg.and_then(|c| std::fs::read_to_string(c).ok()).is_some_and(|t| t.contains("evani-pack-vendor")) {
            cmd.env("CARGO_SOURCE_EVANI_PACK_VENDOR_DIRECTORY", pack.join("vendor"));
        }
    }
    let st = cmd.status().map_err(|e| format!("{}: {e}", evani.display()))?;
    if !st.success() {
        return Err(format!("`{} build --app-dir {} --ship` failed ({st})", evani.display(), app_dir.display()));
    }
    let text = std::fs::read_to_string(app_dir.join("evani.toml")).map_err(|e| e.to_string())?;
    let id = evani_guide::manifest::Modules::parse(&text)?.name;
    Ok(evani_guide::app_dir::workspace_root_of(app_dir).join("target").join("evani-sizes").join(format!("{id}-ship")))
}

/// The start-view pictures beside the report.
pub const BEFORE_PNG: &str = "start-before.png";
pub const AFTER_PNG: &str = "start-after.png";
pub const DIFF_PNG: &str = "start-diff.png";
pub const REPORT: &str = "upgrade-report";
/// Flag the upgrade when more than this share of the start view changed.
pub const DEFAULT_MAX_DIFF: f64 = 10.0;

/// The game as it is now, on its own engine: built now by the installed
/// pack it pins when there is one (exactly the old engine), else its last
/// ship build in its workspace (whatever built it last, named in the
/// source text), else nothing.
fn before_build(root: &Path, dir: &Path, ws: &Path, id: &str, from: Option<&str>, tools: &UpgradeTools) -> Result<(PathBuf, String), String> {
    if let (Some(f), Some(engines)) = (from, ev::engines_dir(root))
        && let Some(p) = ev::installed(&engines).into_iter().find(|p| p.version == f && p.dir.join("engine") != root)
    {
        let dist = (tools.build_with_pack)(&p.dir, dir).map_err(|e| format!("building it with its own engine {f} ({}) failed: {e}", p.dir.display()))?;
        return Ok((dist, format!("built now with its own engine {f} ({})", p.dir.display())));
    }
    let dist = ws.join("target").join("evani-sizes").join(format!("{id}-ship"));
    if dist.join("index.html").is_file() {
        let facts: Option<Value> = std::fs::read_to_string(dist.join("build-facts.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
        let by = facts.as_ref().and_then(|v| v["engine"]["commit"].as_str().map(|c| format!("engine tools {} at commit {c}", v["engine"]["version"].as_str().unwrap_or("?"))));
        return Ok((dist, format!("its last ship build ({}; engine {} is not installed, so it was not rebuilt)", by.unwrap_or_else(|| "engine not recorded".into()), from.unwrap_or("unpinned"))));
    }
    Err(format!("engine {} is not installed and the game has no ship build ({}), so there is nothing to show from before", from.unwrap_or("unpinned"), dist.display()))
}

/// The before-and-after part of the report.
#[derive(Debug, Default)]
struct Pictures {
    before: Option<String>,
    after: bool,
    diff: Option<start_view::Diff>,
    max_diff: f64,
    notes: Vec<String>,
}

impl Pictures {
    fn flagged(&self) -> bool {
        self.diff.as_ref().is_some_and(|d| d.changed_percent() > self.max_diff)
    }

    fn json(&self) -> Value {
        json!({
            "taken": self.before.is_some() && self.after,
            "before": self.before.as_ref().map(|s| json!({ "file": BEFORE_PNG, "source": s })),
            "after": self.after.then(|| json!({ "file": AFTER_PNG, "source": "the upgraded copy's ship build on this engine" })),
            "diff": self.diff.as_ref().map(|d| json!({
                "file": DIFF_PNG,
                "changed_percent": (d.changed_percent() * 100.0).round() / 100.0,
                "mean_change": (d.mean_abs * 10000.0).round() / 10000.0,
                "same_size": d.same_size,
                "tolerance": start_view::TOLERANCE,
                "max_percent": self.max_diff,
                "flagged": self.flagged(),
            })),
            "size": [start_view::WIDTH, start_view::HEIGHT],
            "tier": start_view::TIER,
            "notes": self.notes,
        })
    }

    fn line(&self) -> String {
        match &self.diff {
            Some(d) if self.flagged() => format!(
                "WARN: the start view changed a lot: {:.1}% of its pixels (over {}%); look at {BEFORE_PNG} and {AFTER_PNG} ({DIFF_PNG} shows the changes in red) before keeping the upgrade",
                d.changed_percent(),
                self.max_diff
            ),
            Some(d) => format!("start view: {:.1}% of its pixels changed (at most {}% is fine): it looks the same", d.changed_percent(), self.max_diff),
            None if self.notes.is_empty() => "start view: no pictures (--no-pictures)".into(),
            None => format!("start view: no comparison ({})", self.notes.join("; ")),
        }
    }
}

/// Write `upgrade-report.json` (schema `evani-upgrade/1`) and `.md` in `work`.
fn write_report(work: &Path, r: &Value) -> Result<(), String> {
    let j = work.join(format!("{REPORT}.json"));
    std::fs::write(&j, serde_json::to_string_pretty(r).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", j.display()))?;
    let pic = &r["pictures"];
    let mut md = format!(
        "# Engine upgrade: {} from {} to {}\n\nResult: **{}**. {}\n\nGame: `{}`\nCopy tried first: `{}`\n",
        r["game"].as_str().unwrap_or(""),
        r["from"].as_str().unwrap_or(""),
        r["to"].as_str().unwrap_or(""),
        r["result"].as_str().unwrap_or(""),
        r["summary"].as_str().unwrap_or(""),
        r["app_dir"].as_str().unwrap_or(""),
        r["copy"].as_str().unwrap_or("")
    );
    md.push_str("\n## Start view, before and after\n\n");
    if let Some(b) = pic["before"]["source"].as_str() {
        md.push_str(&format!("Before ({b}):\n\n![before]({BEFORE_PNG})\n\n"));
    }
    if pic["after"].is_object() {
        md.push_str(&format!("After (this engine):\n\n![after]({AFTER_PNG})\n\n"));
    }
    if let Some(d) = pic["diff"].as_object() {
        md.push_str(&format!(
            "Changed pixels (red): {}% (flagged above {}%){}\n\n![changes]({DIFF_PNG})\n",
            d["changed_percent"], d["max_percent"], if d["flagged"] == true { ": **FLAGGED, look before keeping it**" } else { "" }
        ));
    }
    for n in pic["notes"].as_array().into_iter().flatten() {
        md.push_str(&format!("- {}\n", n.as_str().unwrap_or("")));
    }
    let m = work.join(format!("{REPORT}.md"));
    std::fs::write(&m, md).map_err(|e| format!("{}: {e}", m.display()))
}

/// `upgrade-engine --app-dir <dir> [--to <v>] [--dry-run] [--no-pictures] [--max-diff <pct>]`.
pub fn cmd_upgrade(root: &Path, dir: &Path, args: &[String], tools: &UpgradeTools) -> Result<(), String> {
    let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let to = ev::running(root);
    if let Some(v) = value(args, "--to")?
        && v != to
    {
        return Err(format!("upgrade-engine --to {v}: this is engine {to}; an upgrade runs with the target engine's own tools (engine {v}'s `upgrade-engine`)"));
    }
    let max_diff = match value(args, "--max-diff")? {
        Some(v) => v.parse::<f64>().ok().filter(|p| (0.0..=100.0).contains(p)).ok_or_else(|| format!("--max-diff {v}: a percentage, 0 to 100"))?,
        None => DEFAULT_MAX_DIFF,
    };
    let pictures_on = !args.iter().any(|a| a == "--no-pictures");
    let text = std::fs::read_to_string(dir.join("evani.toml")).map_err(|e| format!("{}: {e}", dir.join("evani.toml").display()))?;
    let from = ev::pin(&text)?;
    if from.as_deref() == Some(to.as_str()) {
        println!("{} is already on engine {to}", dir.display());
        return Ok(());
    }
    let from_s = from.clone().unwrap_or_else(|| "unpinned".into());
    if let Some(f) = &from
        && ev::compare(f, &to) == std::cmp::Ordering::Greater
    {
        return Err(format!("upgrade-engine: {} is on engine {f}, newer than this engine {to}; a game never moves to an older engine", dir.display()));
    }
    let id = evani_guide::manifest::Modules::parse(&text)?.name;
    let ws = evani_guide::app_dir::workspace_root_of(&dir);
    let rel = dir.strip_prefix(&ws).map_err(|_| "upgrade-engine: the app folder is not inside its workspace".to_string())?.to_path_buf();
    let work = evani_guide::state_dir(root).join("upgrade").join(format!("{id}-{from_s}-to-{to}"));
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let copy = work.join("game");
    let dry = args.iter().any(|a| a == "--dry-run");
    let report = |result: &str, summary: &str, pics: &Pictures| {
        let r = json!({
            "schema": "evani-upgrade/1",
            "game": id, "app_dir": dir, "from": from_s, "to": to, "dry_run": dry,
            "result": result, "summary": summary,
            "copy": copy, "kept": (result == "upgraded").then(|| work.join("before")),
            "pictures": pics.json(),
        });
        write_report(&work, &r)
    };

    // 1. Before: the game as it is now, on its own engine.
    let mut pics = Pictures { max_diff, ..Pictures::default() };
    if pictures_on {
        println!("upgrade {id}: picture of the start view on engine {from_s}");
        match before_build(root, &dir, &ws, &id, from.as_deref(), tools).and_then(|(dist, source)| (tools.shoot)(&dist, &id, &work.join(BEFORE_PNG)).map(|()| source)) {
            Ok(source) => pics.before = Some(source),
            Err(e) => pics.notes.push(format!("no before picture: {e}")),
        }
    }

    // 2. The copy on this engine, built and checked.
    println!("upgrade {id}: engine {from_s} -> {to}; trying a copy first ({})", copy.display());
    copy_game(&ws, &copy)?;
    move_to_engine(root, &copy.join(&rel), &copy, &to)?;
    let after_dist = match (tools.build_and_check)(&copy.join(&rel)) {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("the copy on engine {to} did not pass, so {} is unchanged (still engine {from_s})", dir.display());
            report("failed", &msg, &pics)?;
            return Err(format!("upgrade-engine: {msg}.\n{e}\nThe copy is kept for a look: {}\nreport: {}", copy.display(), work.join(format!("{REPORT}.md")).display()));
        }
    };

    // 3. After, and the comparison.
    if pictures_on {
        println!("upgrade {id}: picture of the start view on engine {to}");
        match (tools.shoot)(&after_dist, &id, &work.join(AFTER_PNG)) {
            Ok(()) => pics.after = true,
            Err(e) => pics.notes.push(format!("no after picture: {e}")),
        }
        if pics.before.is_some() && pics.after {
            match (start_view::read_png(&work.join(BEFORE_PNG)), start_view::read_png(&work.join(AFTER_PNG))) {
                (Ok(b), Ok(a)) => {
                    let (d, img) = start_view::compare(&b, &a);
                    start_view::write_png(&work.join(DIFF_PNG), d.width, d.height, &img)?;
                    pics.diff = Some(d);
                }
                (Err(e), _) | (_, Err(e)) => pics.notes.push(format!("could not compare the pictures: {e}")),
            }
        }
    }
    let line = pics.line();
    if dry {
        let msg = format!("dry run: the copy built and checked clean on engine {to}; {} is unchanged", dir.display());
        report("dry-run", &msg, &pics)?;
        println!("{msg}\n{line}\nreport: {}", work.join(format!("{REPORT}.md")).display());
        return Ok(());
    }
    // Keep what the game had, beside the copy, then move the game itself.
    let before = work.join("before");
    std::fs::create_dir_all(&before).map_err(|e| e.to_string())?;
    for f in [ws.join("Cargo.toml"), ws.join("Cargo.lock"), dir.join("Cargo.toml"), dir.join("evani.toml")] {
        if f.is_file() {
            let name = evani_guide::rel(&ws, &f).replace('/', "__");
            let _ = std::fs::copy(&f, before.join(name));
        }
    }
    move_to_engine(root, &dir, &ws, &to)?;
    let msg = format!("upgraded {} to engine {to} (the files it had are in {})", dir.display(), before.display());
    report("upgraded", &msg, &pics)?;
    println!("{msg}\n{line}\nreport: {}", work.join(format!("{REPORT}.md")).display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(root: &Path, p: &str, s: &str) {
        std::fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
        std::fs::write(root.join(p), s).unwrap();
    }

    fn engine(base: &Path, v: &str) -> PathBuf {
        let e = base.join(format!("engine-{v}"));
        w(&e, "packaging/engine-pack.json", &format!("{{\"pack_version\":\"{v}\"}}"));
        w(&e, "Cargo.toml", "[workspace]\nmembers = []\n[workspace.dependencies]\nevani-core = { path = \"crates/evani-core\" }\n");
        w(&e, "Cargo.lock", &format!("# engine {v}\n"));
        e
    }

    fn game(base: &Path, engine: &Path, pin: &str) -> PathBuf {
        let g = base.join("game");
        w(&g, "Cargo.toml", &format!("[workspace]\nmembers = [\"app\"]\n[workspace.dependencies]\nevani-core = {{ path = \"{}/crates/evani-core\" }}\n", engine.display().to_string().replace('\\', "/")));
        w(&g, "Cargo.lock", "# old\n");
        w(&g, "app/Cargo.toml", "[package]\nname = \"sea\"\n");
        w(&g, "app/evani.toml", &ev::set_pin("name = \"sea\"\nkit = \"sea\"\nlook = \"toon\"\n", pin));
        w(&g, "target/junk", "x");
        g.join("app")
    }

    fn no_pack(_: &Path, _: &Path) -> Result<PathBuf, String> {
        Err("no pack in this test".into())
    }
    fn no_browser(_: &Path, _: &str, _: &Path) -> Result<(), String> {
        Err("no browser in this test".into())
    }
    fn tools<'a>(build: &'a dyn Fn(&Path) -> Result<PathBuf, String>) -> UpgradeTools<'a> {
        UpgradeTools { build_and_check: build, build_with_pack: &no_pack, shoot: &no_browser }
    }
    fn report_of(work: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(work.join("upgrade-report.json")).unwrap()).unwrap()
    }

    /// A solid picture, the left `white` columns of 10 white.
    fn shot(out: &Path, white: u32) {
        let rgb: Vec<u8> = (0..10 * 10).flat_map(|i| if i % 10 < white { [255, 255, 255] } else { [40, 90, 160] }).collect();
        start_view::write_png(out, 10, 10, &rgb).unwrap();
    }

    #[test]
    fn the_start_view_is_pictured_before_and_after_and_a_large_change_flagged() {
        let base = std::env::temp_dir().join(format!("evani-xtask-upgrade-pics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (old, new) = (engine(&base, "0.1.0"), engine(&base, "0.2.0"));
        let app = game(&base, &old, "0.1.0");
        // The game's last ship build (engine 0.1.0 is not installed here).
        w(&app, "../target/evani-sizes/sea-ship/index.html", "<html>");
        w(&app, "../target/evani-sizes/sea-ship/build-facts.json", r#"{"engine":{"version":"0.1.0","commit":"abc"}}"#);
        let copy_dist = base.join("copy-dist");
        let build = |_: &Path| -> Result<PathBuf, String> { Ok(copy_dist.clone()) };
        let seen = std::cell::RefCell::new(Vec::new());
        // After: 4 of 10 columns turn white (40 % changed).
        let shoot = |dist: &Path, id: &str, out: &Path| -> Result<(), String> {
            seen.borrow_mut().push((dist.to_path_buf(), id.to_string()));
            shot(out, if out.ends_with(AFTER_PNG) { 4 } else { 0 });
            Ok(())
        };
        let t = UpgradeTools { build_and_check: &build, build_with_pack: &no_pack, shoot: &shoot };
        cmd_upgrade(&new, &app, &["--dry-run".into()], &t).unwrap();
        let work = new.join(".evani/upgrade/sea-0.1.0-to-0.2.0");
        let seen = seen.borrow().clone();
        assert!(seen[0].0.ends_with("target/evani-sizes/sea-ship") && seen[0].1 == "sea", "before: the game's own ship build: {seen:?}");
        assert_eq!(seen[1].0, copy_dist, "after: the upgraded copy's ship build");
        for f in [BEFORE_PNG, AFTER_PNG, DIFF_PNG, "upgrade-report.json", "upgrade-report.md"] {
            assert!(work.join(f).is_file(), "{f} beside the report");
        }
        let r = report_of(&work);
        assert_eq!(r["schema"], "evani-upgrade/1");
        assert_eq!(r["pictures"]["taken"], true);
        assert_eq!(r["pictures"]["diff"]["changed_percent"], 40.0);
        assert_eq!(r["pictures"]["diff"]["flagged"], true);
        assert!(r["pictures"]["before"]["source"].as_str().unwrap().contains("last ship build"));
        assert!(std::fs::read_to_string(work.join("upgrade-report.md")).unwrap().contains("FLAGGED"));
        // A higher limit: the same change is not flagged.
        cmd_upgrade(&new, &app, &["--dry-run".into(), "--max-diff".into(), "50".into()], &UpgradeTools { build_and_check: &build, build_with_pack: &no_pack, shoot: &|_, _, out| { shot(out, if out.ends_with(AFTER_PNG) { 4 } else { 0 }); Ok(()) } }).unwrap();
        assert_eq!(report_of(&work)["pictures"]["diff"]["flagged"], false);
        // No browser: the upgrade still runs, the report says why there are no pictures.
        cmd_upgrade(&new, &app, &["--dry-run".into()], &tools(&build)).unwrap();
        let r = report_of(&work);
        assert_eq!((r["result"].as_str(), r["pictures"]["taken"].as_bool()), (Some("dry-run"), Some(false)));
        assert!(r["pictures"]["notes"][0].as_str().unwrap().contains("no browser"));
        // No ship build and no old engine: no before picture, said plainly.
        std::fs::remove_dir_all(app.join("../target/evani-sizes")).unwrap();
        cmd_upgrade(&new, &app, &["--dry-run".into()], &t).unwrap();
        assert!(report_of(&work)["pictures"]["notes"][0].as_str().unwrap().contains("not installed and the game has no ship build"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn an_upgrade_changes_the_game_only_after_the_copy_passes() {
        let base = std::env::temp_dir().join(format!("evani-xtask-upgrade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (old, new) = (engine(&base, "0.1.0"), engine(&base, "0.2.0"));
        let app = game(&base, &old, "0.1.0");
        let fail = |_: &Path| -> Result<PathBuf, String> { Err("ERROR: tests fail".into()) };
        let pass = |_: &Path| -> Result<PathBuf, String> { Ok(PathBuf::new()) };
        let has_app = |a: &Path| -> Result<PathBuf, String> { if a.join("evani.toml").is_file() { Ok(PathBuf::new()) } else { Err("no app".into()) } };
        // A failing copy: the game is untouched, the copy is kept, the report says so.
        let e = cmd_upgrade(&new, &app, &["--no-pictures".into()], &tools(&fail)).unwrap_err();
        assert!(e.contains("did not pass") && e.contains("still engine 0.1.0") && e.contains("tests fail"), "{e}");
        assert_eq!(ev::pin(&std::fs::read_to_string(app.join("evani.toml")).unwrap()).unwrap().as_deref(), Some("0.1.0"));
        let work = new.join(".evani/upgrade/sea-0.1.0-to-0.2.0");
        assert_eq!(report_of(&work)["result"], "failed");
        // The copy was pinned and relinked, without build output.
        let copy = work.join("game");
        assert!(std::fs::read_to_string(copy.join("Cargo.toml")).unwrap().contains("engine-0.2.0/crates/evani-core"));
        assert!(!copy.join("target").exists());
        // A dry run passes and changes nothing.
        cmd_upgrade(&new, &app, &["--dry-run".into(), "--no-pictures".into()], &tools(&has_app)).unwrap();
        assert!(std::fs::read_to_string(app.join("../Cargo.toml")).unwrap().contains("engine-0.1.0"));
        assert_eq!(report_of(&work)["result"], "dry-run");
        // A passing copy: the game is pinned, relinked and takes the engine's lock.
        cmd_upgrade(&new, &app, &["--no-pictures".into()], &tools(&pass)).unwrap();
        assert_eq!(ev::pin(&std::fs::read_to_string(app.join("evani.toml")).unwrap()).unwrap().as_deref(), Some("0.2.0"));
        assert!(std::fs::read_to_string(app.join("../Cargo.toml")).unwrap().contains("engine-0.2.0/crates/evani-core"));
        assert_eq!(std::fs::read_to_string(app.join("../Cargo.lock")).unwrap(), "# engine 0.2.0
");
        assert!(work.join("before/Cargo.lock").is_file(), "what the game had is kept");
        assert_eq!(report_of(&work)["result"], "upgraded");
        // Never to an older engine, never to another version than this one.
        assert!(cmd_upgrade(&old, &app, &[], &tools(&pass)).unwrap_err().contains("never moves to an older engine"));
        assert!(cmd_upgrade(&new, &app, &["--to".into(), "0.3.0".into()], &tools(&pass)).unwrap_err().contains("target engine's own tools"));
        assert!(cmd_upgrade(&new, &app, &["--max-diff".into(), "200".into()], &tools(&pass)).unwrap_err().contains("percentage"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn relink_refuses_a_game_pinned_to_another_engine() {
        let base = std::env::temp_dir().join(format!("evani-xtask-relink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (old, new) = (engine(&base, "0.1.0"), engine(&base, "0.2.0"));
        let app = game(&base, &old, "0.1.0");
        assert!(cmd_relink(&new, &app).unwrap_err().contains("pinned to engine 0.1.0"));
        // The same version in a moved folder: relinked.
        let moved = base.join("moved-0.1.0");
        std::fs::rename(&old, &moved).unwrap();
        cmd_relink(&moved, &app).unwrap();
        assert!(std::fs::read_to_string(app.join("../Cargo.toml")).unwrap().contains("moved-0.1.0/crates/evani-core"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
