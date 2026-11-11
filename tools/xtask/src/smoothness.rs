//! `cargo xtask smoothness <id>`: frame pacing and motion steadiness of a
//! built game, optionally before and after a change (docs/SMOOTHNESS.md).
//!
//! It builds the game's dev build (the `full` variant: every evani.toml
//! module plus `live`, whose `motion.trace` command reads where the
//! config's probe is drawn each frame), and for `--net <profile>` the game's
//! headless referee, then runs `tools/smoothness/smoothness.mjs` (headless,
//! muted Chrome on the graphics card, synthetic keys). The game supplies
//! `smoothness.json` beside its evani.toml: what to track and which keys to
//! press. A tool agents choose to run, not a gate; nothing here reaches a
//! shipped game (`live` is dropped from every `--ship` build).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{AppDir, load_app, trunk_build, workspace_root};

pub fn usage() -> &'static str {
    "  cargo xtask smoothness <app> [--baseline <build folder>] [--candidate <build folder>] [--no-build] [--net none|lan|wan|bad] [--out <folder>] [--config <smoothness.json>] [--seed n] [--duration s] [--width px] [--height px] [--title text] [--baseline-label text] [--candidate-label text] [--limit key=value ..] [--no-fail]   (docs/SMOOTHNESS.md; also with --app-dir)"
}

/// Options with a value (for `--app-dir`'s positional check).
pub const VALUE_OPTIONS: &[&str] = &[
    "--baseline", "--candidate", "--net", "--out", "--config", "--seed", "--duration", "--width", "--height", "--title", "--baseline-label", "--candidate-label", "--limit",
];

/// Options passed straight to the node runner.
const PASS: &[&str] = &["--seed", "--duration", "--width", "--height", "--title", "--baseline-label", "--candidate-label", "--limit"];

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub app: Option<String>,
    pub baseline: Option<PathBuf>,
    pub candidate: Option<PathBuf>,
    pub no_build: bool,
    pub net: String,
    pub out: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub no_fail: bool,
    /// Pass-through options, in order.
    pub pass: Vec<String>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { net: "none".into(), ..Args::default() };
    let mut it = args.iter();
    while let Some(s) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{s} needs a value\n{}", usage()));
        match s.as_str() {
            "--baseline" => a.baseline = Some(PathBuf::from(val()?)),
            "--candidate" => a.candidate = Some(PathBuf::from(val()?)),
            "--net" => a.net = val()?,
            "--out" => a.out = Some(PathBuf::from(val()?)),
            "--config" => a.config = Some(PathBuf::from(val()?)),
            "--no-build" => a.no_build = true,
            "--no-fail" => a.no_fail = true,
            o if PASS.contains(&o) => {
                let v = val()?;
                a.pass.push(o.to_string());
                a.pass.push(v);
            }
            o if o.starts_with("--") => return Err(format!("unknown option {o}\n{}", usage())),
            o if a.app.is_none() => a.app = Some(o.to_string()),
            o => return Err(format!("unexpected `{o}`\n{}", usage())),
        }
    }
    if !["none", "lan", "wan", "bad"].contains(&a.net.as_str()) {
        return Err(format!("--net {}: one of none, lan, wan, bad", a.net));
    }
    Ok(a)
}

/// The node command line (without `node` itself).
pub fn node_args(root: &Path, config: &Path, candidate: &Path, a: &Args, out: &Path, referee: Option<&Path>) -> Vec<String> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let mut v = vec![s(&root.join("tools").join("smoothness").join("smoothness.mjs")), "--config".into(), s(config), "--candidate".into(), s(candidate)];
    if let Some(b) = &a.baseline {
        v.extend(["--baseline".into(), s(b)]);
    }
    v.extend(["--out".into(), s(out), "--net".into(), a.net.clone()]);
    if let Some(r) = referee {
        v.extend(["--referee".into(), s(r)]);
    }
    if a.no_fail {
        v.push("--no-fail".into());
    }
    v.extend(a.pass.iter().cloned());
    v
}

/// `"referee_package"` from the config's `net` block.
pub fn referee_package(config_text: &str) -> Result<Option<String>, String> {
    let v: serde_json::Value = serde_json::from_str(config_text).map_err(|e| format!("smoothness config: {e}"))?;
    Ok(v.get("net").and_then(|n| n.get("referee_package")).and_then(|p| p.as_str()).map(str::to_string))
}

pub fn cmd_smoothness(args: &[String]) -> Result<(), String> {
    let a = parse(args)?;
    let id = a.app.clone().ok_or_else(|| format!("which game?\n{}", usage()))?;
    let app = load_app(&id)?;
    run(&app, a)
}

pub(crate) fn cmd_smoothness_app(app: &AppDir, args: &[String]) -> Result<(), String> {
    run(app, parse(args)?)
}

fn run(app: &AppDir, a: Args) -> Result<(), String> {
    let root = workspace_root();
    let config = a.config.clone().unwrap_or_else(|| app.dir.join("smoothness.json"));
    let text = std::fs::read_to_string(&config).map_err(|e| {
        format!("{}: {e}\n  fix: write the game's smoothness config there (what to track, which keys to press): docs/SMOOTHNESS.md", config.display())
    })?;
    if !app.manifest.live {
        eprintln!("  note: {} has live = false, so its build has no motion trace; only frame times are measured", app.name);
    }
    let candidate = match &a.candidate {
        Some(c) => c.clone(),
        None => {
            let dist = app.sizes_dir().join(format!("{}-full", app.name));
            if a.no_build && dist.join("index.html").is_file() {
                eprintln!("  using the existing build in {}", dist.display());
            } else {
                trunk_build(app, "full", &app.manifest)?;
            }
            dist
        }
    };
    let referee = if a.net == "none" {
        None
    } else {
        let pkg = referee_package(&text)?.ok_or_else(|| format!("--net {}: {} has no net.referee_package (the game's headless referee crate)", a.net, config.display()))?;
        Some(build_referee(app, &pkg, a.no_build)?)
    };
    let out = a.out.clone().unwrap_or_else(|| root.join(".evani").join("smoothness").join(&app.name));
    let status = Command::new("node")
        .args(node_args(&root, &config, &candidate, &a, &out, referee.as_deref()))
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cannot run node ({e}); the smoothness harness needs Node 22 or newer"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1) => Err(format!("smoothness: over the limits (report: {})", out.join("report.html").display())),
        _ => Err(format!("smoothness: the run could not be made ({status})")),
    }
}

/// The game's headless referee (`evani-referee/1`) for the networked mode,
/// built as bridges/chimpvibe/build.mjs does, in its own target folder.
fn build_referee(app: &AppDir, pkg: &str, no_build: bool) -> Result<PathBuf, String> {
    let target = app.cargo_target_dir().join("referee");
    let wasm = target.join("wasm32-unknown-unknown").join("release").join(format!("{}.wasm", pkg.replace('-', "_")));
    if no_build && wasm.is_file() {
        return Ok(wasm);
    }
    eprintln!("  building the referee {pkg} (wasm32, headless)");
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", pkg, "--release", "--target", "wasm32-unknown-unknown", "--target-dir"])
        .arg(&target)
        .env("CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS", "--cfg=web_sys_unstable_apis --cfg=evani_headless")
        .current_dir(&app.dir)
        .status()
        .map_err(|e| format!("cargo build -p {pkg}: {e}"))?;
    if !status.success() || !wasm.is_file() {
        return Err(format!("the referee {pkg} did not build ({status}; expected {})", wasm.display()));
    }
    Ok(wasm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_options_and_passes_the_rest_through() {
        let a = parse(&s(&["kart-racer", "--baseline", "b", "--net", "wan", "--limit", "hitches_max=3", "--seed", "4", "--no-build"])).unwrap();
        assert_eq!(a.app.as_deref(), Some("kart-racer"));
        assert_eq!(a.baseline, Some(PathBuf::from("b")));
        assert_eq!(a.net, "wan");
        assert!(a.no_build);
        assert_eq!(a.pass, s(&["--limit", "hitches_max=3", "--seed", "4"]));
        assert!(parse(&s(&["x", "--net", "moon"])).is_err());
        assert!(parse(&s(&["x", "--frobnicate"])).is_err());
        assert!(parse(&s(&["x", "y"])).is_err());
        assert!(parse(&s(&["x", "--out"])).is_err());
    }

    #[test]
    fn the_node_command_line_names_both_builds() {
        let a = parse(&s(&["k", "--baseline", "old", "--net", "wan", "--no-fail", "--duration", "9"])).unwrap();
        let v = node_args(Path::new("/r"), Path::new("/r/apps/k/smoothness.json"), Path::new("/new"), &a, Path::new("/o"), Some(Path::new("/ref.wasm")));
        let j = v.join(" ");
        assert!(v[0].ends_with("smoothness.mjs"), "{j}");
        for part in ["--candidate /new", "--baseline old", "--net wan", "--referee /ref.wasm", "--no-fail", "--duration 9", "--out /o"] {
            assert!(j.replace('\\', "/").contains(part), "{part} in {j}");
        }
    }

    #[test]
    fn reads_the_referee_package_and_every_value_option_is_known() {
        assert_eq!(referee_package(r#"{"net":{"referee_package":"evani-kart-racer-referee"}}"#).unwrap().as_deref(), Some("evani-kart-racer-referee"));
        assert_eq!(referee_package("{}").unwrap(), None);
        assert!(referee_package("{").is_err());
        for o in PASS {
            assert!(VALUE_OPTIONS.contains(o), "{o}");
        }
    }

    #[test]
    fn the_kart_racer_config_is_valid() {
        let p = workspace_root().join("apps").join("kart-racer").join("smoothness.json");
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["game"], "kart-racer");
        assert!(v["probe"]["kind"].is_string());
        assert!(v["script"].as_array().is_some_and(|a| !a.is_empty()));
        assert_eq!(referee_package(&v.to_string()).unwrap().as_deref(), Some("evani-kart-racer-referee"));
    }

    /// The harness's own JS tests (tools/smoothness/smoothness.test.mjs),
    /// so `cargo test -p xtask` covers them. Skipped without Node.
    #[test]
    fn the_node_harness_tests_pass() {
        let root = workspace_root();
        let Ok(out) = Command::new("node").arg("--test").arg(root.join("tools/smoothness/smoothness.test.mjs")).current_dir(&root).output() else {
            eprintln!("node not found: the smoothness harness tests were skipped");
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "node --test failed:\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
        assert!(text.contains("fail 0"), "{text}");
    }
}
