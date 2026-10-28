//! `cargo xtask importance <app> [--viewport WxH] [--out <file>]` (S7-3): how
//! much each model matters on screen. It runs the game natively (headless, the
//! fixed 60 Hz step) with evani-render's dev-only `importance` feature on for
//! this one run, which samples the game's capture moments and its bench path
//! and writes `apps/<app>/assets/importance.json` (the screen share and pixel
//! size of every model, and a suggested texture size). `cargo xtask bytes`
//! reads that file (`bytes::importance_by_model`); the value-per-byte budget
//! (S7-4) will too.
//!
//! The feature is turned on here, on the command line, and nowhere else: not
//! in any app's `evani.toml` or default features, so a ship build carries none
//! of it (the ship guard fails on its `evani-importance` marker).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub fn usage() -> &'static str {
    "  cargo xtask importance <app> [--viewport WxH] [--out <file>]   (runs the game headless and writes apps/<app>/assets/importance.json: each model's on-screen share; dev only, 0 bytes in a shipped game)"
}

/// Parsed arguments.
#[derive(Debug, PartialEq, Eq)]
struct Args {
    app: String,
    viewport: Option<String>,
    out: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut app, mut viewport, mut out) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--viewport" => {
                let v = it.next().ok_or("--viewport needs a value like 1280x720")?;
                let ok = v
                    .split_once('x')
                    .is_some_and(|(w, h)| w.parse::<u32>().is_ok_and(|w| w > 0) && h.parse::<u32>().is_ok_and(|h| h > 0));
                if !ok {
                    return Err(format!("--viewport {v}: expected WIDTHxHEIGHT, like 1280x720"));
                }
                viewport = Some(v.clone());
            }
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out needs a file")?)),
            s if s.starts_with("--") => return Err(format!("importance: unknown option {s}\n{}", usage())),
            s => app = Some(s.to_string()),
        }
    }
    Ok(Args {
        app: app.ok_or_else(|| format!("importance: which app?\n{}", usage()))?,
        viewport,
        out,
    })
}

/// The cargo package name of the app (`evani-sandbox`), read from its Cargo.toml.
fn package_name(cargo_toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && line.starts_with("name") {
            return line.split('"').nth(1).map(str::to_string);
        }
    }
    None
}

/// Check the file a run wrote and return its table rows:
/// `(id, max share %, mean share %, frames on screen, max px, suggested texture px)`.
fn read_rows(json: &Value) -> Result<Vec<Row>, String> {
    if json.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("importance.json has no `version: 1`".into());
    }
    let models = json
        .get("models")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty())
        .ok_or("no model was drawn on screen during the run (the game draws no models, or they did not load)")?;
    let mut rows = Vec::new();
    for m in models {
        let f = |k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let id = m.get("id").and_then(Value::as_str).ok_or("a model entry has no id")?;
        rows.push(Row {
            id: id.to_string(),
            max_pct: f("max_screen_share") * 100.0,
            mean_pct: f("mean_screen_share") * 100.0,
            frames: m.get("frames_on_screen").and_then(Value::as_u64).unwrap_or(0),
            max_px: f("max_px"),
            texture_px: m.get("suggested_texture_px").and_then(Value::as_u64).unwrap_or(0),
        });
    }
    // Most important first; the id breaks ties so the table is stable.
    rows.sort_by(|a, b| b.max_pct.total_cmp(&a.max_pct).then_with(|| a.id.cmp(&b.id)));
    Ok(rows)
}

/// One model's line of the table.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    id: String,
    max_pct: f64,
    mean_pct: f64,
    frames: u64,
    max_px: f64,
    texture_px: u64,
}

fn print_table(app: &str, out: &Path, rows: &[Row]) {
    println!("importance {app}: {} model(s) -> {}", rows.len(), out.display());
    println!("  {:<28} {:>9} {:>9} {:>8} {:>8} {:>8}", "model", "max %", "mean %", "frames", "max px", "texture");
    for r in rows {
        println!("  {:<28} {:>9.3} {:>9.3} {:>8} {:>8.0} {:>8}", r.id, r.max_pct, r.mean_pct, r.frames, r.max_px, r.texture_px);
    }
}

pub fn cmd_importance(args: &[String]) -> Result<(), String> {
    let a = parse_args(args)?;
    let app = crate::load_app(&a.app)?;
    let root = crate::workspace_root();
    let cargo_toml = std::fs::read_to_string(app.dir.join("Cargo.toml")).map_err(|e| format!("{}: {e}", app.dir.display()))?;
    let package = package_name(&cargo_toml).ok_or_else(|| format!("{}: no package name", app.dir.join("Cargo.toml").display()))?;
    let assets = app.dir.join("assets");
    let out = match &a.out {
        Some(p) if p.is_absolute() => p.clone(),
        Some(p) => root.join(p),
        None => assets.join("importance.json"),
    };
    let _ = std::fs::remove_file(&out);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    // The app's own modules (`--no-default-features` plus evani.toml's list),
    // and the measuring, which exists nowhere else.
    let mut features = app.manifest.features();
    features.push("evani-render/importance".to_string());
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root)
        .args(["run", "--quiet", "-p", &package, "--no-default-features", "--features", &features.join(",")])
        .env("EVANI_IMPORTANCE_OUT", &out)
        .env("EVANI_IMPORTANCE_ASSETS", &assets)
        .env("EVANI_IMPORTANCE_GAME", &a.app);
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "2");
    }
    if let Some(v) = &a.viewport {
        cmd.env("EVANI_IMPORTANCE_VIEWPORT", v);
    }
    println!("importance {}: running {package} headless with `importance` on (this build is dev only and never shipped)", a.app);
    let status = cmd.status().map_err(|e| format!("cargo run: {e}"))?;
    if !status.success() {
        return Err(format!("ERROR {}: the headless run failed ({status})\n  fix: read the output above; the same run is `cargo run -p {package} --no-default-features --features {}`", a.app, features.join(",")));
    }
    let text = std::fs::read_to_string(&out).map_err(|_| {
        format!(
            "ERROR {}: the run wrote no importance file\n  fix: the run was too short or the game draws no models; the headless game runs a fixed number of frames and the measuring needs about 150 of them",
            a.app
        )
    })?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("ERROR {}: {} is not json: {e}", a.app, out.display()))?;
    let rows = read_rows(&json).map_err(|e| format!("ERROR {}: {e}", a.app))?;
    print_table(&a.app, &out, &rows);
    println!("  `cargo xtask bytes {}` shows this importance next to each model's bytes", a.app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes;

    fn synthetic() -> Value {
        serde_json::json!({
            "version": 1,
            "game": "demo",
            "models": [
                {"id": "tree", "max_screen_share": 0.01, "mean_screen_share": 0.005, "frames_on_screen": 10, "max_px": 40.0, "suggested_texture_px": 128},
                {"id": "hero", "max_screen_share": 0.2, "mean_screen_share": 0.1, "frames_on_screen": 90, "max_px": 500.0, "suggested_texture_px": 1024}
            ]
        })
    }

    #[test]
    fn rows_come_out_most_important_first() {
        let rows = read_rows(&synthetic()).unwrap();
        assert_eq!(rows[0].id, "hero");
        assert!((rows[0].max_pct - 20.0).abs() < 1e-9);
        assert_eq!((rows[0].frames, rows[0].texture_px), (90, 1024));
        assert_eq!(rows[1].id, "tree");
    }

    #[test]
    fn bytes_reads_what_the_run_writes() {
        let m = bytes::importance_by_model(&synthetic());
        assert_eq!(m.len(), 2);
        assert!((m["hero"] - 0.2).abs() < 1e-9);
    }

    #[test]
    fn a_bad_file_is_named() {
        let no_version = serde_json::json!({"models": [{"id": "a"}]});
        assert!(read_rows(&no_version).unwrap_err().contains("version"));
        let empty = serde_json::json!({"version": 1, "models": []});
        assert!(read_rows(&empty).unwrap_err().contains("no model"));
    }

    #[test]
    fn arguments_and_package_name() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        let p = parse_args(&a("sandbox --viewport 1920x1080 --out x/y.json")).unwrap();
        assert_eq!((p.app.as_str(), p.viewport.as_deref()), ("sandbox", Some("1920x1080")));
        assert_eq!(p.out, Some(PathBuf::from("x/y.json")));
        assert!(parse_args(&a("--viewport 12")).is_err());
        assert!(parse_args(&a("sandbox --viewport 0x5")).is_err());
        assert!(parse_args(&a("sandbox --bogus")).is_err());
        assert!(parse_args(&[]).is_err());
        let toml = "[package]\nname = \"evani-sandbox\"\nversion = 1\n[features]\nname = \"x\"\n";
        assert_eq!(package_name(toml).as_deref(), Some("evani-sandbox"));
    }
}
