//! `check --quick`: the fast loop while you work.
//!
//! The same cheap static checks as the plain `check` run first (the caller
//! prints them), then fmt (check mode only, never writes), clippy and tests
//! for ONLY the workspace crates that changed since the last commit (staged,
//! unstaged and untracked files against `HEAD`) and every workspace crate that
//! depends on them (transitively, from `cargo metadata`). It uses the normal
//! target folder, so cargo's incremental cache is reused, and it keeps the
//! machine light: a capped job count and test-thread count and, on Windows, a
//! below-normal process priority.
//!
//! It is not a substitute for `check --full` before review or hand-over.
//!
//! fmt is advisory: the repository is not rustfmt-formatted (long lines, hand
//! layout) and must not be reformatted, so a scoped `cargo fmt --check` would
//! always fail. Quick mode reports the touched files that differ as warnings.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Windows `BELOW_NORMAL_PRIORITY_CLASS` process creation flag.
pub const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Files outside every crate folder whose changes still need one crate's
/// tests: `(path prefix, crate)`.
const SHARED_FORMAT_FILES: &[(&str, &str)] = &[("tests/fixtures/models/", "evani-gltf"), ("docs/MODEL_FORMAT.md", "evani-gltf")];

/// Files at the workspace root that change how every crate builds.
const WORKSPACE_WIDE: &[&str] = &["Cargo.toml", "Cargo.lock", "clippy.toml", ".clippy.toml", "rustfmt.toml", ".rustfmt.toml", "rust-toolchain", "rust-toolchain.toml"];

// ---- load caps -------------------------------------------------------------------

/// The load limits for cargo children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caps {
    /// `CARGO_BUILD_JOBS`.
    pub jobs: usize,
    /// `RUST_TEST_THREADS`.
    pub test_threads: usize,
}

impl Caps {
    /// `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS` win when they are positive
    /// numbers; otherwise about half the cores (at least 1) for both.
    pub fn from_env_values(cores: usize, jobs_env: Option<&str>, threads_env: Option<&str>) -> Caps {
        let half = (cores / 2).max(1);
        let pos = |v: Option<&str>| v.and_then(|s| s.trim().parse::<usize>().ok()).filter(|n| *n > 0);
        let jobs = pos(jobs_env).unwrap_or(half);
        Caps { jobs, test_threads: pos(threads_env).unwrap_or(jobs) }
    }

    pub fn detect() -> Caps {
        let cores = std::thread::available_parallelism().map_or(2, usize::from);
        Caps::from_env_values(cores, std::env::var("CARGO_BUILD_JOBS").ok().as_deref(), std::env::var("RUST_TEST_THREADS").ok().as_deref())
    }
}

/// Which limits a cargo child gets (below-normal priority on Windows always).
#[derive(Debug, Clone, Copy, Default)]
pub struct Limits {
    pub jobs: Option<usize>,
    pub test_threads: Option<usize>,
}

impl Limits {
    /// Quick mode: both caps.
    pub fn quick(c: &Caps) -> Limits {
        Limits { jobs: Some(c.jobs), test_threads: Some(c.test_threads) }
    }

    /// `--full`: priority and the test-thread cap only, so the build keeps
    /// every job it would otherwise use (what it verifies is unchanged).
    pub fn full(c: &Caps) -> Limits {
        Limits { jobs: None, test_threads: Some(c.test_threads) }
    }
}

#[cfg(windows)]
fn lower_priority(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
}

#[cfg(not(windows))]
fn lower_priority(_cmd: &mut Command) {}

/// `cargo <args>` in `root` with the limits applied.
pub(crate) fn cargo_command(root: &Path, args: &[&str], limits: Limits) -> Command {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args(args);
    if let Some(j) = limits.jobs {
        cmd.env("CARGO_BUILD_JOBS", j.to_string());
    }
    if let Some(t) = limits.test_threads {
        cmd.env("RUST_TEST_THREADS", t.to_string());
    }
    lower_priority(&mut cmd);
    cmd
}

// ---- which crates ----------------------------------------------------------------

/// One workspace crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkg {
    pub name: String,
    /// Folder from the workspace root, `/` separators.
    pub dir: String,
    /// Names of workspace crates it depends on (any kind of dependency).
    pub deps: Vec<String>,
}

fn slashes(s: &str) -> String {
    s.replace('\\', "/")
}

/// The workspace crates in `cargo metadata --no-deps` output.
pub fn parse_metadata(v: &Value) -> Vec<Pkg> {
    let ws = slashes(v.get("workspace_root").and_then(Value::as_str).unwrap_or("")).trim_end_matches('/').to_string();
    let pkgs = v.get("packages").and_then(Value::as_array).cloned().unwrap_or_default();
    let names: BTreeSet<String> = pkgs.iter().filter_map(|p| p.get("name").and_then(Value::as_str)).map(str::to_string).collect();
    let mut out = Vec::new();
    for p in &pkgs {
        let Some(name) = p.get("name").and_then(Value::as_str) else { continue };
        let manifest = slashes(p.get("manifest_path").and_then(Value::as_str).unwrap_or(""));
        let dir = manifest.rsplit_once('/').map_or("", |(d, _)| d);
        let dir = dir.strip_prefix(&ws).unwrap_or(dir).trim_matches('/').to_string();
        let mut deps: Vec<String> = p
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| d.get("name").and_then(Value::as_str))
            .filter(|d| *d != name && names.contains(*d))
            .map(str::to_string)
            .collect();
        deps.sort();
        deps.dedup();
        out.push(Pkg { name: name.to_string(), dir, deps });
    }
    out
}

/// What `--quick` will check, and why.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Every changed file.
    pub files: Vec<String>,
    /// Crates with a changed file, and those files.
    pub touched: BTreeMap<String, Vec<String>>,
    /// Crates that depend (transitively) on a touched one, and the crate they
    /// were reached from.
    pub dependents: BTreeMap<String, String>,
    /// A workspace-wide file changed (the first one): every crate is chosen.
    pub workspace_wide: Option<String>,
    /// Changed files outside every crate (docs, catalog, assets...).
    pub other: usize,
    /// Every workspace crate (the `workspace_wide` case).
    all: Vec<String>,
}

impl Plan {
    /// Nothing changed since the last commit.
    pub fn nothing_changed(&self) -> bool {
        self.files.is_empty()
    }

    /// The crates cargo will check, sorted.
    pub fn chosen(&self) -> Vec<String> {
        if self.workspace_wide.is_some() {
            return self.all.clone();
        }
        let set: BTreeSet<&String> = self.touched.keys().chain(self.dependents.keys()).collect();
        set.into_iter().cloned().collect()
    }

    /// The crates with changed `.rs` files (what fmt looks at).
    pub fn touched_rs(&self) -> Vec<String> {
        self.touched.values().flatten().filter(|f| f.ends_with(".rs")).cloned().collect()
    }

    /// The report: which crates, and why.
    pub fn describe(&self) -> String {
        let mut s = String::new();
        if self.nothing_changed() {
            s.push_str("quick: nothing changed since the last commit (no staged, unstaged or untracked files); only the static checks ran.\n");
            return s;
        }
        s.push_str(&format!("quick: {} file{} changed since the last commit", self.files.len(), plural(self.files.len())));
        if self.other > 0 {
            s.push_str(&format!(" ({} outside any crate: docs, catalog, assets)", self.other));
        }
        s.push('\n');
        if let Some(f) = &self.workspace_wide {
            s.push_str(&format!("  all {} workspace crates: `{f}` changed and affects every crate\n", self.all.len()));
            return s;
        }
        if self.touched.is_empty() {
            s.push_str("  no crate source changed, so there is nothing for fmt, clippy or tests to check; only the static checks ran.\n");
            return s;
        }
        for (name, files) in &self.touched {
            let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
            let more = if files.len() > 3 { format!(", +{} more", files.len() - 3) } else { String::new() };
            s.push_str(&format!("  {name}: touched ({}{more})\n", shown.join(", ")));
        }
        for (name, via) in &self.dependents {
            s.push_str(&format!("  {name}: dependent (via {via})\n"));
        }
        s
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Map changed files to crates and add the transitive reverse dependents.
pub fn plan(pkgs: &[Pkg], files: &[String]) -> Plan {
    let mut files: Vec<String> = files.iter().map(|f| slashes(f)).collect();
    files.sort();
    files.dedup();
    let mut p = Plan { files: files.clone(), all: pkgs.iter().map(|k| k.name.clone()).collect(), ..Plan::default() };
    for f in &files {
        if WORKSPACE_WIDE.contains(&f.as_str()) || f.starts_with(".cargo/") {
            p.workspace_wide.get_or_insert_with(|| f.clone());
            continue;
        }
        // The deepest crate folder holding the file.
        let owner = pkgs
            .iter()
            .filter(|k| !k.dir.is_empty() && (f == &k.dir || f.starts_with(&format!("{}/", k.dir))))
            .max_by_key(|k| k.dir.len());
        // Shared format files outside any crate belong to the crate whose
        // tests hold them to their document (the model fixtures and
        // docs/MODEL_FORMAT.md: the model reader).
        let owner = owner.or_else(|| {
            let (_, name) = SHARED_FORMAT_FILES.iter().find(|(prefix, _)| f.starts_with(prefix))?;
            pkgs.iter().find(|k| k.name == *name)
        });
        match owner {
            Some(k) => p.touched.entry(k.name.clone()).or_default().push(f.clone()),
            None => p.other += 1,
        }
    }
    // Reverse dependents, breadth first from every touched crate.
    let mut queue: VecDeque<String> = p.touched.keys().cloned().collect();
    while let Some(cur) = queue.pop_front() {
        for k in pkgs.iter().filter(|k| k.deps.contains(&cur)) {
            if p.touched.contains_key(&k.name) || p.dependents.contains_key(&k.name) {
                continue;
            }
            p.dependents.insert(k.name.clone(), cur.clone());
            queue.push_back(k.name.clone());
        }
    }
    p
}

/// Staged, unstaged and untracked files against `HEAD`, from the workspace
/// root, with `/` separators.
pub fn changed_files(root: &Path) -> Result<Vec<String>, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let o = Command::new("git").current_dir(root).args(["-c", "core.quotepath=off"]).args(args).output().map_err(|e| format!("git: {e}"))?;
        if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) } else { Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim())) }
    };
    let mut files = BTreeSet::new();
    for out in [git(&["diff", "--name-only", "--no-renames", "--relative", "HEAD"])?, git(&["ls-files", "--others", "--exclude-standard"])?] {
        files.extend(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(slashes));
    }
    Ok(files.into_iter().collect())
}

fn workspace(root: &Path) -> Result<Vec<Pkg>, String> {
    let o = cargo_command(root, &["metadata", "--format-version", "1", "--no-deps"], Limits::default()).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !o.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    let v: Value = serde_json::from_slice(&o.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
    Ok(parse_metadata(&v))
}

// ---- running ---------------------------------------------------------------------

/// The files in rustfmt `--check` output that differ, among `touched`.
/// Lines look like `Diff in <path>:<line>:` (older: `Diff in <path> at line <n>:`).
pub fn fmt_diff_files(output: &str, touched: &[String]) -> Vec<String> {
    let mut hit = BTreeSet::new();
    for l in output.lines() {
        let Some(rest) = l.trim().strip_prefix("Diff in ") else { continue };
        let rest = slashes(rest.trim_end_matches(':'));
        let path = match rest.split_once(" at line ") {
            Some((p, _)) => p.to_string(),
            None => match rest.rsplit_once(':') {
                Some((p, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => p.to_string(),
                _ => rest,
            },
        };
        if let Some(t) = touched.iter().find(|t| path == **t || path.ends_with(&format!("/{t}"))) {
            hit.insert(t.clone());
        }
    }
    hit.into_iter().collect()
}

/// Lines worth showing from a failed cargo run: diagnostics and failures,
/// else the last lines.
fn failure_lines(text: &str, n: usize) -> Vec<String> {
    let all: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let key = |l: &&str| {
        let low = l.to_lowercase();
        low.contains("error") || low.contains("warning") || low.contains("failed") || low.contains("panicked")
    };
    let picked: Vec<&str> = all.iter().copied().filter(key).take(n).collect();
    let lines = if picked.is_empty() { all[all.len().saturating_sub(n)..].to_vec() } else { picked };
    lines.into_iter().map(|l| format!("    {l}")).collect()
}

/// The distinct `path:line:col: warning: ...` lines of a `--message-format short` run.
pub fn warning_lines(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    text.lines().map(str::trim).filter(|l| l.contains(": warning") && seen.insert(l.to_string())).map(str::to_string).collect()
}

fn secs(d: Duration) -> String {
    format!("{:.1}s", d.as_secs_f64())
}

/// Run one cargo step; prints its result line. Returns whether it passed.
fn step(name: &str, mut cmd: Command) -> bool {
    use std::io::Write as _;
    print!("  {name} ... ");
    let _ = std::io::stdout().flush();
    let t = Instant::now();
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let text = format!("{}
{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let warns = warning_lines(&text);
            if warns.is_empty() {
                println!("ok ({})", secs(t.elapsed()));
            } else {
                println!("ok ({}), WARN {} warning{} (advisory: they may predate your change)", secs(t.elapsed()), warns.len(), plural(warns.len()));
                for w in warns.iter().take(10) {
                    println!("    {w}");
                }
            }
            true
        }
        Ok(o) => {
            println!("FAILED ({})", secs(t.elapsed()));
            let text = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            for l in failure_lines(&text, 20) {
                println!("{l}");
            }
            false
        }
        Err(e) => {
            println!("FAILED to start: {e}");
            false
        }
    }
}

/// The text of the final line.
pub fn final_line(problems: usize, chosen: usize) -> String {
    const NOT_FULL: &str = "This is not a substitute for `cargo xtask check --full` before review or hand-over.";
    if problems == 0 {
        format!("evani check --quick: clean ({chosen} crate{} checked). {NOT_FULL}", plural(chosen))
    } else {
        format!("evani check --quick: {problems} problem{} in {chosen} crate{}. Fix them and run it again. {NOT_FULL}", plural(problems), plural(chosen))
    }
}

/// Run quick mode in `root` (the static checks were already printed).
/// Returns how many steps failed.
pub fn run(root: &Path) -> Result<usize, String> {
    let files = changed_files(root)?;
    let pkgs = workspace(root)?;
    let plan = plan(&pkgs, &files);
    print!("{}", plan.describe());
    let chosen = plan.chosen();
    if chosen.is_empty() {
        println!("{}", final_line(0, 0));
        return Ok(0);
    }
    let caps = Caps::detect();
    println!("  load caps: {} build job{}, {} test thread{}{}", caps.jobs, plural(caps.jobs), caps.test_threads, plural(caps.test_threads), if cfg!(windows) { ", below-normal priority" } else { "" });
    let limits = Limits::quick(&caps);
    let pk: Vec<String> = chosen.iter().flat_map(|n| ["-p".to_string(), n.clone()]).collect();
    let mut failed = 0;

    // fmt: advisory, check mode only, the touched crates and files.
    let rs = plan.touched_rs();
    if !rs.is_empty() {
        let tp: Vec<String> = plan.touched.keys().flat_map(|n| ["-p".to_string(), n.clone()]).collect();
        let mut args: Vec<&str> = vec!["fmt"];
        args.extend(tp.iter().map(String::as_str));
        args.extend(["--", "--check"]);
        match cargo_command(root, &args, limits).output() {
            Ok(o) => {
                let text = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let diff = fmt_diff_files(&text, &rs);
                if diff.is_empty() && o.status.success() {
                    println!("  fmt ... ok");
                } else if diff.is_empty() {
                    println!("  fmt ... WARN could not check: {}", failure_lines(&text, 1).first().map_or("", |s| s.trim()));
                } else {
                    println!("  fmt ... WARN {} touched file{} differ from rustfmt (advisory: this repo is not rustfmt-formatted and is never reformatted)", diff.len(), plural(diff.len()));
                    for f in diff.iter().take(10) {
                        println!("    {f}");
                    }
                }
            }
            Err(e) => println!("  fmt ... WARN could not run rustfmt: {e}"),
        }
    }

    let clippy = clippy_args(&pk);
    if !step("clippy", cargo_command(root, &clippy, limits)) {
        failed += 1;
    }
    let mut test: Vec<&str> = vec!["test", "--quiet", "--no-fail-fast"];
    test.extend(pk.iter().map(String::as_str));
    if !step("tests", cargo_command(root, &test, limits)) {
        failed += 1;
    }
    println!("{}", final_line(failed, chosen.len()));
    Ok(failed)
}

/// The clippy command line: every warning is an error (`-D warnings`; the workspace is clean).
fn clippy_args(pk: &[String]) -> Vec<&str> {
    let mut clippy: Vec<&str> = vec!["clippy", "--all-targets", "--message-format", "short"];
    clippy.extend(pk.iter().map(String::as_str));
    clippy.extend(["--", "-D", "warnings"]);
    clippy
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clippy_warnings_fail_quick_mode() {
        let pk = vec!["-p".to_string(), "evani-core".to_string()];
        let args = clippy_args(&pk);
        assert_eq!(args[..2], ["clippy", "--all-targets"]);
        assert!(args.ends_with(&["--", "-D", "warnings"]), "{args:?}");
        let dash = args.iter().position(|a| *a == "--");
        assert!(dash > args.iter().position(|a| *a == "evani-core"), "-D warnings goes after the package list");
    }

    fn pkg(name: &str, dir: &str, deps: &[&str]) -> Pkg {
        Pkg { name: name.into(), dir: dir.into(), deps: deps.iter().map(|s| (*s).to_string()).collect() }
    }

    /// core <- render <- kit <- app ; look <- app ; tool (alone).
    fn ws() -> Vec<Pkg> {
        vec![
            pkg("evani-core", "crates/evani-core", &[]),
            pkg("evani-render", "crates/evani-render", &["evani-core"]),
            pkg("evani-kit-a", "kits/evani-kit-a", &["evani-core", "evani-render"]),
            pkg("evani-look-x", "looks/evani-look-x", &["evani-render"]),
            pkg("evani-a", "apps/a", &["evani-kit-a", "evani-look-x"]),
            pkg("evani-tool", "tools/evani-tool", &[]),
        ]
    }

    fn files(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_changed_file_maps_to_its_crate() {
        let p = plan(&ws(), &files(&["kits/evani-kit-a/src/lib.rs", "kits/evani-kit-a/Cargo.toml", "DECISIONS.md"]));
        assert_eq!(p.touched.keys().collect::<Vec<_>>(), ["evani-kit-a"]);
        assert_eq!(p.touched["evani-kit-a"].len(), 2);
        assert_eq!(p.other, 1, "DECISIONS.md is outside every crate");
        assert!(p.workspace_wide.is_none());
    }

    #[test]
    fn the_deepest_crate_folder_wins() {
        let pkgs = vec![pkg("outer", "tools", &[]), pkg("inner", "tools/inner", &[])];
        let p = plan(&pkgs, &files(&["tools/inner/src/lib.rs", "tools/outer.rs", "toolsy/x.rs"]));
        assert_eq!(p.touched["inner"], ["tools/inner/src/lib.rs"]);
        assert_eq!(p.touched["outer"], ["tools/outer.rs"]);
        assert_eq!(p.other, 1, "`toolsy/` is not inside `tools/`");
    }

    #[test]
    fn dependents_are_transitive_and_say_why() {
        let p = plan(&ws(), &files(&["crates/evani-core/src/lib.rs"]));
        assert_eq!(p.chosen(), ["evani-a", "evani-core", "evani-kit-a", "evani-look-x", "evani-render"]);
        assert_eq!(p.dependents["evani-render"], "evani-core");
        assert_eq!(p.dependents["evani-a"], "evani-kit-a", "reached through the kit, not directly");
        assert!(!p.chosen().contains(&"evani-tool".to_string()), "an unrelated crate is not chosen");
        let text = p.describe();
        assert!(text.contains("evani-core: touched (crates/evani-core/src/lib.rs)"), "{text}");
        assert!(text.contains("evani-render: dependent (via evani-core)"), "{text}");
    }

    #[test]
    fn model_fixtures_and_format_doc_choose_the_model_reader() {
        let mut pkgs = ws();
        pkgs.push(pkg("evani-gltf", "crates/evani-gltf", &[]));
        for f in ["tests/fixtures/models/kart.low.glb", "docs/MODEL_FORMAT.md"] {
            let p = plan(&pkgs, &files(&[f]));
            assert_eq!(p.touched.keys().collect::<Vec<_>>(), ["evani-gltf"], "{f}");
        }
        // Without the crate in the workspace, nothing is chosen.
        assert!(plan(&ws(), &files(&["tests/fixtures/models/kart.low.glb"])).touched.is_empty());
    }

    #[test]
    fn a_leaf_crate_has_no_dependents() {
        let p = plan(&ws(), &files(&["apps/a/src/main.rs"]));
        assert_eq!(p.chosen(), ["evani-a"]);
        assert!(p.dependents.is_empty());
    }

    #[test]
    fn a_touched_crate_is_never_listed_as_a_dependent() {
        let p = plan(&ws(), &files(&["crates/evani-core/src/lib.rs", "crates/evani-render/src/lib.rs"]));
        assert!(p.touched.contains_key("evani-render"));
        assert!(!p.dependents.contains_key("evani-render"));
    }

    #[test]
    fn a_workspace_wide_file_chooses_every_crate() {
        for f in ["Cargo.lock", "Cargo.toml", "clippy.toml", ".cargo/config.toml"] {
            let p = plan(&ws(), &files(&[f]));
            assert_eq!(p.workspace_wide.as_deref(), Some(f));
            assert_eq!(p.chosen().len(), 6, "{f}");
            assert!(p.describe().contains(&format!("`{f}` changed")), "{f}");
        }
    }

    #[test]
    fn non_crate_changes_choose_nothing() {
        let p = plan(&ws(), &files(&["docs/X.md", "catalog/looks.json"]));
        assert!(!p.nothing_changed());
        assert!(p.chosen().is_empty());
        assert!(p.describe().contains("no crate source changed"));
    }

    #[test]
    fn nothing_changed_says_so() {
        let p = plan(&ws(), &[]);
        assert!(p.nothing_changed());
        assert!(p.chosen().is_empty());
        assert!(p.describe().contains("nothing changed since the last commit"));
        let end = final_line(0, 0);
        assert!(end.contains("not a substitute for `cargo xtask check --full`"), "{end}");
    }

    #[test]
    fn backslash_paths_and_duplicates_are_normalised() {
        let p = plan(&ws(), &files(&["crates\\evani-core\\src\\lib.rs", "crates/evani-core/src/lib.rs"]));
        assert_eq!(p.files, ["crates/evani-core/src/lib.rs"]);
        assert_eq!(p.touched["evani-core"].len(), 1);
    }

    #[test]
    fn metadata_gives_crates_dirs_and_only_workspace_deps() {
        let v = json!({
            "workspace_root": "C:\\work\\eng",
            "packages": [
                { "name": "evani-core", "manifest_path": "C:\\work\\eng\\crates\\evani-core\\Cargo.toml", "dependencies": [{ "name": "serde" }] },
                { "name": "evani-kit", "manifest_path": "C:\\work\\eng\\kits\\evani-kit\\Cargo.toml",
                  "dependencies": [{ "name": "evani-core", "kind": null }, { "name": "evani-core", "kind": "dev" }, { "name": "wgpu" }, { "name": "evani-kit" }] }
            ]
        });
        let pkgs = parse_metadata(&v);
        assert_eq!(pkgs, [pkg("evani-core", "crates/evani-core", &[]), pkg("evani-kit", "kits/evani-kit", &["evani-core"])]);
    }

    #[test]
    fn caps_respect_the_environment_else_half_the_cores() {
        assert_eq!(Caps::from_env_values(16, None, None), Caps { jobs: 8, test_threads: 8 });
        assert_eq!(Caps::from_env_values(1, None, None), Caps { jobs: 1, test_threads: 1 });
        assert_eq!(Caps::from_env_values(16, Some("2"), None), Caps { jobs: 2, test_threads: 2 });
        assert_eq!(Caps::from_env_values(16, Some("3"), Some("5")), Caps { jobs: 3, test_threads: 5 });
        assert_eq!(Caps::from_env_values(16, Some("default"), Some("0")), Caps { jobs: 8, test_threads: 8 }, "not a positive number: ignored");
    }

    #[test]
    fn fmt_diff_lines_are_matched_to_touched_files_only() {
        let out = "Diff in C:\\w\\eng\\kits\\evani-kit-a\\src\\lib.rs:12:\n-x\n+y\nDiff in /w/eng/kits/evani-kit-a/src/other.rs at line 3:\nDiff in C:\\w\\eng\\kits\\evani-kit-a\\src\\lib.rs:40:\n";
        let hit = fmt_diff_files(out, &files(&["kits/evani-kit-a/src/lib.rs", "kits/evani-kit-a/src/more.rs"]));
        assert_eq!(hit, ["kits/evani-kit-a/src/lib.rs"]);
    }

    #[test]
    fn warning_lines_are_distinct_and_skip_summaries() {
        let w = r"crates\a\src\lib.rs:3:5: warning: unused thing";
        let out = format!("{w}\nwarning: `a` (lib) generated 1 warning\n{w}\nerror: aborting\n");
        assert_eq!(warning_lines(&out), [w]);
        assert!(warning_lines("Finished `dev`\n").is_empty());
    }

    #[test]
    fn the_final_line_never_claims_to_replace_full() {
        assert!(final_line(0, 3).starts_with("evani check --quick: clean (3 crates checked)"));
        assert!(final_line(2, 1).contains("2 problems in 1 crate"));
        assert!(final_line(2, 1).ends_with("before review or hand-over."));
    }
}
