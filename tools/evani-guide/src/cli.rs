//! The command line shared by `cargo xtask <cmd>` and this crate's binary
//! (the Stop hook runs the binary; a future standalone `evani` CLI can call
//! [`run`] too).

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::{check, doctor, evals, extools, fonttools, genre, quick, scaffold, targets};

pub const USAGE: &str = "\
cargo xtask init-ai [--with copilot] [--no-mcp-build]  write the AI guidance, .mcp.json, the Stop hook; install the live MCP server
cargo xtask doctor [--fix] [--no-mcp-build] [--no-tools] [--blender]
                                                      is the guidance current and the live MCP server installed? --fix repairs,
                                                      and installs the pinned font-atlas tool (msdf-atlas-gen) and the
                                                      model-converter tools (--blender: Blender too)
cargo xtask check [--app <id>].. [--full | --quick] [--strict] [--json]
                                                      the gate: every game's module list, catalog fit, geocheck, review card,
                                                      look guard (--full: tests and the ship build), each issue with a fix.
                                                      --quick (while you work, not a substitute for --full): also fmt (advisory),
                                                      clippy (warnings fail) and tests for only the crates changed since the last commit and their dependents
cargo xtask check --app-dir <game app folder> [--full] [--strict] [--json]
                                                      the same per-game gate for one game kept outside the engine folder
cargo xtask new-game <id> [--look <look>] [--genre <genre>] [--effects <a,b,..|none>]
                                                      a new game (apps/<id> + kits/evani-kit-<id>) with its guidance. --genre checks the
                                                      look against the genre's compatible_looks (catalog/genres.json), takes its physics,
                                                      and writes kits/<id>/SPEC.md with the reference image prompt; --effects must exist
                                                      in catalog/effects.json and fit the look's style
cargo xtask catalog [--json]                          the genres ranked by most played (the catalog's reach_index)
cargo xtask ai-eval --plan [--agent <cmd>] | --score <dir> [--full]
                                                      one-shot game-making evals: the plan, or a finished worktree's score";

fn root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    crate::find_root(&cwd).ok_or_else(|| "not inside an Evani.Dev project (no Cargo.toml with [workspace] and apps/ above here)".into())
}

/// Run `args` (the command first). Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let cmd = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or(&[]);
    let result = match cmd {
        Some("init-ai") => init_ai(rest),
        Some("doctor") => cmd_doctor(rest),
        Some("check") => cmd_check(rest),
        Some("new-game") => cmd_new_game(rest),
        Some("catalog") => cmd_catalog(rest),
        Some("ai-eval") => cmd_eval(rest),
        _ => Err(format!("usage:\n{USAGE}")),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn flag(rest: &[String], f: &str) -> bool {
    rest.iter().any(|a| a == f)
}

fn value(rest: &[String], f: &str) -> Result<Option<String>, String> {
    match rest.iter().position(|a| a == f) {
        None => Ok(None),
        Some(i) => rest.get(i + 1).cloned().map(Some).ok_or_else(|| format!("{f} needs a value")),
    }
}

fn known(rest: &[String], flags: &[&str], values: &[&str]) -> Result<(), String> {
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        if values.contains(&a) {
            i += 2;
            continue;
        }
        if !flags.contains(&a) {
            return Err(format!("unknown argument `{a}`\nusage:\n{USAGE}"));
        }
        i += 1;
    }
    Ok(())
}

fn opts(rest: &[String]) -> Result<targets::Options, String> {
    let copilot = match value(rest, "--with")?.as_deref() {
        None => false,
        Some("copilot") => true,
        Some(x) => return Err(format!("--with {x}: only `copilot` is optional")),
    };
    Ok(targets::Options { copilot })
}

fn print_fixed(root: &Path, f: &doctor::Fixed) -> i32 {
    for w in &f.written {
        println!("wrote {w}");
    }
    if let Some(m) = &f.mcp {
        println!("installed the live MCP server: {m}");
    }
    for e in &f.errors {
        eprintln!("error: {e}");
    }
    let d = doctor::doctor(root, targets::Options::default());
    for l in d.lines() {
        println!("still: {l}");
    }
    i32::from(!f.errors.is_empty())
}

fn init_ai(rest: &[String]) -> Result<i32, String> {
    known(rest, &["--no-mcp-build"], &["--with"])?;
    let root = root()?;
    let o = opts(rest)?;
    let f = doctor::fix(&root, o, !flag(rest, "--no-mcp-build"));
    let code = print_fixed(&root, &f);
    println!(
        "Evani AI guidance is in place (engine {}). Commit AGENTS.md, CLAUDE.md, GEMINI.md, .cursor/, .mcp.json and .claude/settings.json. In Claude Code, approve the `evani-live` server once (/mcp). After pulling a new engine version run `cargo xtask doctor --fix`.",
        crate::ENGINE_VERSION
    );
    Ok(code)
}

fn cmd_doctor(rest: &[String]) -> Result<i32, String> {
    known(rest, &["--fix", "--no-mcp-build", "--no-tools", "--blender"], &["--with"])?;
    let root = root()?;
    let o = opts(rest)?;
    if flag(rest, "--fix") {
        let f = doctor::fix(&root, o, !flag(rest, "--no-mcp-build"));
        let mut code = print_fixed(&root, &f);
        if !flag(rest, "--no-tools") {
            match fonttools::install(&root) {
                Ok(true) => println!("installed font tool {} {} into {}", fonttools::NAME, fonttools::VERSION, fonttools::BIN_DIR),
                Ok(false) => {}
                Err(e) => {
                    eprintln!("error: font tool: {e}");
                    code = 1;
                }
            }
            let extra: &[&str] = if flag(rest, "--blender") { &["blender"] } else { &[] };
            let t = extools::install(&root, extra);
            for i in &t.installed {
                println!("installed model-converter tool {i} into {}", extools::BIN_DIR);
            }
            for e in &t.errors {
                eprintln!("error: model-converter tools: {e}");
            }
            code = code.max(i32::from(!t.errors.is_empty()));
        }
        return Ok(code);
    }
    let d = doctor::doctor(&root, o);
    // Only games with `[ui] fonts` need the font tool: a note, never a failure.
    if let Some(l) = fonttools::line(&root) {
        println!("note: {l}; `cargo xtask doctor --fix` installs it");
    }
    // The converter's tools are only needed by `cargo xtask convert`, so
    // their absence is a note, never a doctor failure.
    for l in extools::lines(&root) {
        println!("note: {l}; `cargo xtask doctor --fix` installs it");
    }
    if d.is_clean() {
        println!("evani doctor: the AI guidance (engine {}) and the live MCP server are current", crate::ENGINE_VERSION);
        return Ok(0);
    }
    for l in d.lines() {
        println!("{l}");
    }
    println!("fix: cargo xtask doctor --fix");
    Ok(1)
}

fn cmd_check(rest: &[String]) -> Result<i32, String> {
    if flag(rest, "--hook") {
        let mut input = String::new();
        let _ = std::io::stdin().read_to_string(&mut input);
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let (code, msg) = check::hook(&input, &cwd);
        if !msg.is_empty() {
            eprint!("{msg}");
        }
        return Ok(code);
    }
    let mode = check_mode(rest)?;
    let root = root()?;
    let mut apps = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--app" {
            apps.push(rest.get(i + 1).cloned().ok_or("--app needs a game id")?);
            i += 1;
        }
        i += 1;
    }
    let strict = flag(rest, "--strict");
    let o = check::Options { apps: (!apps.is_empty()).then_some(apps), full: mode == CheckMode::Full, strict, skip_guidance: false };
    let report = check::run(&root, &o);
    if flag(rest, "--json") {
        println!("{}", serde_json::to_string_pretty(&report.to_json(strict)).unwrap_or_default());
    } else {
        print!("{}", report.text(strict, check::Severity::Note));
    }
    let mut code = i32::from(!report.passes(strict));
    if mode == CheckMode::Quick {
        println!();
        // A failing step is reported by the run itself; so is a missing git or cargo.
        code |= i32::from(quick::run(&root)? > 0);
    }
    Ok(code)
}

/// `cargo xtask check --app-dir <folder> [--full] [--strict] [--json]`: the
/// per-game gate for one game outside the engine at `root`, which xtask has
/// already found (`game`). `rest` is the arguments after `check`, without
/// `--app-dir`. Returns the exit code.
pub fn check_game(root: &Path, game: &check::Game, rest: &[String]) -> i32 {
    let run = || -> Result<i32, String> {
        if flag(rest, "--hook") || rest.iter().any(|a| a == "--app") {
            return Err("--app-dir checks the one game in that folder: drop --app and --hook".into());
        }
        let mode = check_mode(rest)?;
        if mode == CheckMode::Quick {
            return Err("--quick is not available with --app-dir yet (it reads the engine's own git changes); run the plain check, or --full before review".into());
        }
        let strict = flag(rest, "--strict");
        let o = check::Options { apps: None, full: mode == CheckMode::Full, strict, skip_guidance: true };
        let report = check::run_game(root, game, &o);
        if flag(rest, "--json") {
            println!("{}", serde_json::to_string_pretty(&report.to_json(strict)).unwrap_or_default());
        } else {
            print!("{}", report.text(strict, check::Severity::Note));
        }
        Ok(i32::from(!report.passes(strict)))
    };
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Which gate `cargo xtask check` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    /// The static checks.
    Plain,
    /// `--full`: also the tests and the ship build.
    Full,
    /// `--quick`: the static checks plus fmt, clippy and tests for the crates
    /// changed since the last commit and their dependents.
    Quick,
}

/// Parse the flags of `check` (not `--hook`).
pub fn check_mode(rest: &[String]) -> Result<CheckMode, String> {
    known(rest, &["--full", "--strict", "--json", "--quick"], &["--app"])?;
    match (flag(rest, "--full"), flag(rest, "--quick")) {
        (true, true) => Err("--quick and --full are different gates: --quick is the fast loop while you work, --full is the one run before review. Pick one.".into()),
        (false, true) if flag(rest, "--json") => Err("--quick prints text only; drop --json (or use the plain check for JSON)".into()),
        (true, false) => Ok(CheckMode::Full),
        (false, true) => Ok(CheckMode::Quick),
        (false, false) => Ok(CheckMode::Plain),
    }
}

fn cmd_new_game(rest: &[String]) -> Result<i32, String> {
    let usage = "usage: cargo xtask new-game <id> [--look <look>] [--genre <genre>] [--effects <a,b,..|none>]";
    let id = rest.first().filter(|a| !a.starts_with("--")).ok_or(usage)?;
    known(&rest[1..], &[], &["--look", "--genre", "--effects"])?;
    let look = value(rest, "--look")?;
    let genre_id = value(rest, "--genre")?;
    let effects = value(rest, "--effects")?.map(|e| genre::parse_effects(&e));
    let root = root()?;
    // Without the new flags this is the plain scaffold, unchanged.
    if genre_id.is_none() && effects.is_none() {
        let look = look.unwrap_or_else(|| "basic".into());
        let written = scaffold::new_game(&root, id, &look)?;
        print_new_game(id, &written);
        return Ok(0);
    }
    let r = genre::resolve(&root, &genre::Request { genre: genre_id.clone(), look, effects })?;
    let extras = scaffold::Extras { physics: r.physics.clone(), effects: r.effects.clone(), spec: r.spec.clone() };
    let written = scaffold::new_game_with(&root, id, &r.look, &extras)?;
    print_new_game(id, &written);
    for n in &r.notes {
        println!("note: {n}");
    }
    if let Some(g) = &genre_id {
        println!("catalog: genre {g}, look {}, physics {}, effects [{}]", r.look, r.physics.as_deref().unwrap_or("none"), r.effects.as_deref().unwrap_or(&[]).join(", "));
        println!("  kits/{id}/SPEC.md has the gameplay systems and the pinned reference image prompt (fill it in: docs/KIT_PROCESS.md step 3).");
    }
    Ok(0)
}

fn print_new_game(id: &str, written: &[String]) {
    println!("made the game `{id}` ({} files):", written.len());
    for w in written.iter().filter(|w| w.ends_with("Cargo.toml") || w.ends_with("AGENTS.md") || w.ends_with("evani.toml") || w.ends_with("SPEC.md")) {
        println!("  {w}");
    }
    println!(
        "next: make its target pack (kits/{id}/target, docs/KIT_PROCESS.md step 2), replace the example spinners in kits/evani-kit-{id}, run it with `cargo xtask live --game {id}` and finish with `cargo xtask check --app {id}`."
    );
}

/// `catalog [--json]`: the genres ranked by most played.
fn cmd_catalog(rest: &[String]) -> Result<i32, String> {
    known(rest, &["--json"], &[])?;
    let cat = genre::Catalog::load(&root()?)?;
    let ranking = cat.ranking_json();
    if flag(rest, "--json") {
        println!("{}", serde_json::to_string_pretty(&ranking).unwrap_or_default());
        return Ok(0);
    }
    println!("genres, most played first ({}; {})", ranking["ranked_by"].as_str().unwrap_or(""), ranking["source"].as_str().unwrap_or(""));
    for g in ranking["genres"].as_array().into_iter().flatten() {
        println!(
            "{:>4}  {:<34} reach {:<3} physics {:<6} {}",
            g["position"].as_u64().unwrap_or(0),
            g["id"].as_str().unwrap_or(""),
            g["reach_index"].as_f64().unwrap_or(0.0),
            g["physics"].as_str().unwrap_or(""),
            g["name"].as_str().unwrap_or("")
        );
    }
    Ok(0)
}

fn cmd_eval(rest: &[String]) -> Result<i32, String> {
    known(rest, &["--plan", "--full"], &["--agent", "--score"])?;
    if let Some(dir) = value(rest, "--score")? {
        let s = evals::score(Path::new(&dir), flag(rest, "--full"));
        println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
        return Ok(i32::from(s["pass"] != true));
    }
    if flag(rest, "--plan") {
        let agent = value(rest, "--agent")?.unwrap_or_else(|| "claude -p --permission-mode acceptEdits".into());
        let p = evals::plan(&root()?, &agent)?;
        println!("{}", serde_json::to_string_pretty(&p).unwrap_or_default());
        return Ok(0);
    }
    Err("usage: cargo xtask ai-eval --plan [--agent <cmd>] | --score <dir> [--full]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn check_flags_pick_the_gate() {
        assert_eq!(check_mode(&args(&[])), Ok(CheckMode::Plain));
        assert_eq!(check_mode(&args(&["--strict", "--app", "demo"])), Ok(CheckMode::Plain));
        assert_eq!(check_mode(&args(&["--full"])), Ok(CheckMode::Full));
        assert_eq!(check_mode(&args(&["--quick"])), Ok(CheckMode::Quick));
        assert_eq!(check_mode(&args(&["--app", "demo", "--quick", "--strict"])), Ok(CheckMode::Quick));
    }

    #[test]
    fn quick_and_full_exclude_each_other() {
        let e = check_mode(&args(&["--quick", "--full"])).unwrap_err();
        assert!(e.contains("Pick one"), "{e}");
    }

    #[test]
    fn quick_has_no_json_and_unknown_flags_are_refused() {
        assert!(check_mode(&args(&["--quick", "--json"])).unwrap_err().contains("--json"));
        assert_eq!(check_mode(&args(&["--json"])), Ok(CheckMode::Plain));
        assert!(check_mode(&args(&["--quik"])).unwrap_err().contains("unknown argument `--quik`"));
        assert!(USAGE.contains("--quick"));
    }

    #[test]
    fn check_with_an_app_dir_takes_only_the_plain_and_full_gates() {
        let root = crate::testutil::fake_engine("cli-check-game");
        let g = crate::check::Game::outside("demo", &root.join("apps/demo"), &root, &root.join("kits/evani-kit-demo"));
        assert_eq!(check_game(&root, &g, &args(&["--quick"])), 1);
        assert_eq!(check_game(&root, &g, &args(&["--app", "x"])), 1);
        assert_eq!(check_game(&root, &g, &args(&["--hook"])), 1);
        assert_eq!(check_game(&root, &g, &args(&["--json"])), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
