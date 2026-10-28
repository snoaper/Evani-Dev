//! `cargo xtask size-report [<app>..] [--modules]`: the real download size of
//! each game's ship build (release, `live` off), per file, raw / gzip / brotli.
//!
//! It also proves `wasm-opt` ran: Trunk writes wasm-bindgen's output to
//! `target/wasm-bindgen/release/<bin>_bg.wasm` and only then runs
//! `wasm-opt -Oz` into the dist folder, so the dist wasm must be smaller than
//! that file (which must come from this very build). A dist wasm that is not
//! smaller fails the report.
//!
//! Over [`LIMIT_BYTES`] raw (or with `--modules`), it rebuilds once per listed
//! module with that module off and lists the biggest contributors. A module
//! the game does not build without (its own compile check refuses) is listed
//! as needed instead of measured, and the report goes on.
//!
//! Gzip is flate2 at best compression; brotli is the `brotli` crate at
//! quality 11, window 22 (what a static host or CDN serves pre-compressed).

use std::io::Write as _;
use std::path::Path;
use std::time::SystemTime;

use super::{BuildSize, gzip_len, kb, load_app, signed_kb, trunk_build, workspace_root};

/// The size line from Chris's review: 1.5 MB, judged on the raw total
/// (the strictest reading; compressed sizes are always smaller).
pub const LIMIT_BYTES: u64 = 1536 * 1024;

pub fn usage() -> String {
    "  cargo xtask size-report [<app>..] [--modules] [--max-file-bytes N] [--max-total-bytes N]   ship build: raw/gzip/brotli per file, wasm-opt proof (default apps: sandbox kart-racer); the caps fail the report when a shipped file or the whole ship folder is over N raw bytes (a host's limits)".to_string()
}

/// What `size-report` was asked for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SizeArgs {
    pub apps: Vec<String>,
    pub modules: bool,
    /// `--max-file-bytes N`: no shipped file may be over N raw bytes.
    pub max_file: Option<u64>,
    /// `--max-total-bytes N`: the shipped files together may not be over N raw bytes.
    pub max_total: Option<u64>,
}

impl SizeArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut a = Self::default();
        let mut it = args.iter();
        while let Some(x) = it.next() {
            let mut bytes = |name: &str| -> Result<u64, String> {
                let v = it.next().ok_or_else(|| format!("{name} needs a number of bytes"))?;
                v.parse().map_err(|_| format!("{name} {v}: not a number of bytes"))
            };
            match x.as_str() {
                "--modules" => a.modules = true,
                "--max-file-bytes" => a.max_file = Some(bytes("--max-file-bytes")?),
                "--max-total-bytes" => a.max_total = Some(bytes("--max-total-bytes")?),
                f if f.starts_with("--") => return Err(format!("unknown size-report option {f}\n{}", usage())),
                app => a.apps.push(app.to_string()),
            }
        }
        Ok(a)
    }
}

/// The caps broken by these reports, one line each (`ERROR ...`).
fn over_caps(reps: &[AppReport], max_file: Option<u64>, max_total: Option<u64>) -> Vec<String> {
    let mut out = Vec::new();
    for rep in reps {
        if let Some(cap) = max_file {
            for f in rep.files.iter().filter(|f| f.raw > cap) {
                out.push(format!("ERROR {}: {} is {} bytes raw, over --max-file-bytes {cap}", rep.app, f.name, f.raw));
            }
        }
        let (raw, _, _) = rep.total();
        if let Some(cap) = max_total
            && raw > cap
        {
            out.push(format!("ERROR {}: the ship folder's files are {raw} bytes raw, over --max-total-bytes {cap}", rep.app));
        }
    }
    out
}

/// Brotli length at quality 11, window 22.
pub fn brotli_len(bytes: &[u8]) -> u64 {
    let mut w = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    if w.write_all(bytes).is_err() {
        return 0;
    }
    w.into_inner().len() as u64
}

#[derive(Debug, Clone)]
pub struct FileRow {
    pub name: String,
    pub raw: u64,
    pub gz: u64,
    pub br: u64,
}

fn file_rows(dist: &Path) -> Result<Vec<FileRow>, String> {
    let entries = std::fs::read_dir(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let shipped = [".wasm", ".js", ".html", ".css"]
            .iter()
            .any(|ext| name.ends_with(ext));
        if !shipped {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        rows.push(FileRow {
            name,
            raw: bytes.len() as u64,
            gz: gzip_len(&bytes),
            br: brotli_len(&bytes),
        });
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.raw));
    Ok(rows)
}

/// The `data-bin` of the app's index.html (falls back to `evani-<app>`).
fn bin_name(app_dir: &Path, app: &str) -> String {
    let html = std::fs::read_to_string(app_dir.join("index.html")).unwrap_or_default();
    html.split("data-bin=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map_or_else(|| format!("evani-{app}"), str::to_string)
}

/// wasm-bindgen's output (before wasm-opt) vs the dist wasm (after).
#[derive(Debug, Clone, Copy)]
pub struct OptProof {
    pub bindgen_raw: u64,
    pub dist_raw: u64,
}

impl OptProof {
    pub fn applied(&self) -> bool {
        self.dist_raw < self.bindgen_raw
    }

    pub fn saved_pct(&self) -> f64 {
        if self.bindgen_raw == 0 {
            return 0.0;
        }
        (self.bindgen_raw as f64 - self.dist_raw as f64) * 100.0 / self.bindgen_raw as f64
    }
}

fn opt_proof(target: &Path, bin: &str, started: SystemTime, dist_wasm_raw: u64) -> Result<OptProof, String> {
    let path = target
        .join("wasm-bindgen")
        .join("release")
        .join(format!("{bin}_bg.wasm"));
    let meta = std::fs::metadata(&path).map_err(|e| {
        format!(
            "{}: {e} (cannot prove wasm-opt ran without wasm-bindgen's output)",
            path.display()
        )
    })?;
    let fresh = meta.modified().map(|t| t >= started).unwrap_or(false);
    if !fresh {
        return Err(format!(
            "{} is older than this build; cannot prove wasm-opt ran",
            path.display()
        ));
    }
    Ok(OptProof {
        bindgen_raw: meta.len(),
        dist_raw: dist_wasm_raw,
    })
}

struct AppReport {
    app: String,
    files: Vec<FileRow>,
    proof: OptProof,
    contributors: Vec<(String, i64, i64)>,
    /// Listed modules the game does not build without (its own compile check
    /// says so): they can't be measured by leaving them out.
    needed: Vec<String>,
}

impl AppReport {
    fn total(&self) -> (u64, u64, u64) {
        self.files
            .iter()
            .fold((0, 0, 0), |(r, g, b), f| (r + f.raw, g + f.gz, b + f.br))
    }
}

fn report_app(app: &crate::AppDir, modules: bool) -> Result<AppReport, String> {
    let name = app.name.as_str();
    let m = app.manifest.clone().ship();
    let bin = bin_name(&app.dir, name);
    println!(
        "{name}: ship build (release, live off), features [{}]",
        m.features().join(", ")
    );
    // Filesystem times can be coarse; allow a little slack before the build.
    let started = SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(2))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let ship: BuildSize = trunk_build(app, "ship", &m)?;
    crate::ship_guard(name, &ship)?;
    let dist = app.sizes_dir().join(format!("{name}-ship"));
    let files = file_rows(&dist)?;
    // A game outside the engine builds in its own workspace's target folder.
    let target = if app.external.is_some() { app.cargo_target_dir() } else { workspace_root().join("target") };
    let proof = opt_proof(&target, &bin, started, ship.wasm_raw)?;
    if !proof.applied() {
        return Err(format!(
            "{name}: wasm-opt was not applied (dist wasm {} is not smaller than wasm-bindgen's {})",
            kb(proof.dist_raw),
            kb(proof.bindgen_raw)
        ));
    }
    let mut rep = AppReport {
        app: name.to_string(),
        files,
        proof,
        contributors: Vec::new(),
        needed: Vec::new(),
    };
    let (raw, _, _) = rep.total();
    if raw > LIMIT_BYTES || modules {
        println!("  measuring each module (one rebuild per module with it off)...");
        for (module, without) in m.without_each() {
            let variant = format!("ship-no-{}", module.replace([' ', '(', ')'], ""));
            let off = trunk_build(app, &variant, &without).map(|s| (s.total_raw(), s.total_gz()));
            account(&mut rep, module, (ship.total_raw(), ship.total_gz()), off);
        }
        match trunk_build(app, "ship-base", &m.base()) {
            Ok(base) => rep.contributors.push((
                "base (core+render+kit+look)".to_string(),
                base.total_raw() as i64,
                base.total_gz() as i64,
            )),
            // Without the modules it needs the game has no base build either.
            Err(e) if !rep.needed.is_empty() => println!("    base: not measured, the game needs {} ({})", rep.needed.join(", "), first_error(&e)),
            Err(e) => return Err(e),
        }
        rep.contributors.sort_by_key(|c| std::cmp::Reverse(c.1));
    }
    Ok(rep)
}

/// One module-off rebuild: the module's share of the full build, or, when
/// the game does not build without it, a "needed" note (the report goes on).
fn account(rep: &mut AppReport, module: String, full: (u64, u64), without: Result<(u64, u64), String>) {
    match without {
        Ok((raw, gz)) => rep.contributors.push((module, full.0 as i64 - raw as i64, full.1 as i64 - gz as i64)),
        Err(e) => {
            println!("    {module}: the game does not build without it, so it is not measured ({})", first_error(&e));
            rep.needed.push(module);
        }
    }
}

/// The first `error` line of a build failure (else its first line).
fn first_error(e: &str) -> &str {
    let mut lines = e.lines();
    e.lines().find(|l| l.trim_start().starts_with("error")).or_else(|| lines.next()).unwrap_or("").trim()
}

fn print_app(rep: &AppReport) {
    println!("\n{} (ship)", rep.app);
    println!(
        "  {:<44} {:>11} {:>11} {:>11}",
        "file", "raw", "gzip", "brotli"
    );
    for f in &rep.files {
        println!(
            "  {:<44} {:>11} {:>11} {:>11}",
            f.name,
            kb(f.raw),
            kb(f.gz),
            kb(f.br)
        );
    }
    let (raw, gz, br) = rep.total();
    println!(
        "  {:<44} {:>11} {:>11} {:>11}",
        "total",
        kb(raw),
        kb(gz),
        kb(br)
    );
    println!(
        "  wasm-opt applied: wasm-bindgen output {} -> dist {} ({:.1}% smaller)",
        kb(rep.proof.bindgen_raw),
        kb(rep.proof.dist_raw),
        rep.proof.saved_pct()
    );
    let verdict = if raw > LIMIT_BYTES { "OVER" } else { "under" };
    println!("  1.5 MB line (raw total): {verdict}");
    if !rep.contributors.is_empty() {
        println!("  biggest contributors (full ship build minus a build without it):");
        for (name, r, g) in &rep.contributors {
            println!(
                "    {name:<30} {:>11} raw {:>11} gzip",
                signed_kb(*r),
                signed_kb(*g)
            );
        }
    }
    if !rep.needed.is_empty() {
        println!("  needed by the game (it does not build without them, so not measured): {}", rep.needed.join(", "));
    }
}

fn report_json(reps: &[AppReport]) -> String {
    let mut j = String::from("[");
    for (i, rep) in reps.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        let (raw, gz, br) = rep.total();
        let files: Vec<String> = rep
            .files
            .iter()
            .map(|f| {
                format!(
                    "{{\"file\": \"{}\", \"raw\": {}, \"gz\": {}, \"br\": {}}}",
                    f.name, f.raw, f.gz, f.br
                )
            })
            .collect();
        let contrib: Vec<String> = rep
            .contributors
            .iter()
            .map(|(n, r, g)| format!("{{\"module\": \"{n}\", \"raw\": {r}, \"gz\": {g}}}"))
            .collect();
        let needed: Vec<String> = rep.needed.iter().map(|n| format!("\"{n}\"")).collect();
        j.push_str(&format!(
            "\n  {{\"app\": \"{}\", \"raw\": {raw}, \"gz\": {gz}, \"br\": {br}, \"over_limit\": {}, \"wasm_bindgen_raw\": {}, \"wasm_opt_raw\": {}, \"files\": [{}], \"contributors\": [{}], \"needed\": [{}]}}",
            rep.app,
            raw > LIMIT_BYTES,
            rep.proof.bindgen_raw,
            rep.proof.dist_raw,
            files.join(", "),
            contrib.join(", "),
            needed.join(", ")
        ));
    }
    j.push_str("\n]\n");
    j
}

/// The report file after this run: every app measured now replaces its old
/// entry (in place), apps measured earlier are kept, new ones are appended.
/// So `size-report kart-racer` adds or updates the kart racer without
/// dropping the sandbox, and the live bridge finds every game it serves.
fn merge_report(existing: Option<&str>, fresh: &str) -> Result<String, String> {
    use serde_json::Value;
    let parse = |s: &str| -> Option<Vec<Value>> {
        serde_json::from_str::<Value>(s).ok()?.as_array().cloned()
    };
    let new = parse(fresh).ok_or("size report: internal JSON error")?;
    let app = |v: &Value| v.get("app").and_then(Value::as_str).map(str::to_string);
    let mut out: Vec<Value> = existing.and_then(parse).unwrap_or_default();
    out.retain(|v| app(v).is_some());
    for entry in new {
        match out.iter_mut().find(|v| app(v) == app(&entry)) {
            Some(slot) => *slot = entry,
            None => out.push(entry),
        }
    }
    let rows: Vec<String> = out
        .iter()
        .map(|v| serde_json::to_string(v).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(format!("[\n  {}\n]\n", rows.join(",\n  ")))
}

pub fn cmd_size_report(args: &[String]) -> Result<(), String> {
    let a = SizeArgs::parse(args)?;
    let mut apps = a.apps.clone();
    if apps.is_empty() {
        apps = vec!["sandbox".to_string(), "kart-racer".to_string()];
    }
    let mut reps = Vec::new();
    for app in &apps {
        let rep = report_app(&load_app(app)?, a.modules)?;
        print_app(&rep);
        reps.push(rep);
    }
    finish(&reps, &workspace_root().join("target").join("evani-sizes").join("size-report.json"), &a)
}

/// `cargo xtask size-report --app-dir <folder> [--modules] [caps]`: the same
/// report for a game outside the engine; the JSON goes to its workspace's
/// `target/evani-sizes/size-report.json`.
pub(crate) fn cmd_size_report_app(app: &crate::AppDir, args: &[String]) -> Result<(), String> {
    let a = SizeArgs::parse(args)?;
    if let Some(x) = a.apps.first() {
        return Err(format!("`{x}`: with --app-dir the game is the folder; give no game id"));
    }
    let rep = report_app(app, a.modules)?;
    print_app(&rep);
    finish(&[rep], &app.sizes_dir().join("size-report.json"), &a)
}

/// The summary table, the JSON report at `path`, then the caps.
fn finish(reps: &[AppReport], path: &Path, a: &SizeArgs) -> Result<(), String> {
    println!(
        "\n{:<12} {:>11} {:>11} {:>11}  wasm-opt",
        "app", "raw", "gzip", "brotli"
    );
    for rep in reps {
        let (raw, gz, br) = rep.total();
        println!(
            "{:<12} {:>11} {:>11} {:>11}  -{:.1}%",
            rep.app,
            kb(raw),
            kb(gz),
            kb(br),
            rep.proof.saved_pct()
        );
    }
    let existing = std::fs::read_to_string(path).ok();
    let text = merge_report(existing.as_deref(), &report_json(reps))?;
    if std::fs::write(path, text).is_ok() {
        println!("\nreport: {}", path.display());
    }
    let over = over_caps(reps, a.max_file, a.max_total);
    if over.is_empty() {
        return Ok(());
    }
    Err(format!(
        "size-report: over the caps:\n{}\n  fix: shrink or split the files named (smaller textures, a lighter boot group, fewer modules: `--modules` lists what each costs), or raise the cap if the host allows it",
        over.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caps_are_read_and_judged() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let a = SizeArgs::parse(&s(&["kart-racer", "--max-file-bytes", "1000", "--modules", "--max-total-bytes", "5000"])).unwrap();
        assert_eq!(a, SizeArgs { apps: s(&["kart-racer"]), modules: true, max_file: Some(1000), max_total: Some(5000) });
        assert!(SizeArgs::parse(&s(&["--max-file-bytes"])).is_err());
        assert!(SizeArgs::parse(&s(&["--max-file-bytes", "big"])).is_err());
        assert!(SizeArgs::parse(&s(&["--cap"])).is_err());
        let row = |name: &str, raw: u64| FileRow { name: name.into(), raw, gz: 0, br: 0 };
        let rep = AppReport { app: "g".into(), files: vec![row("g_bg.wasm", 3000), row("g.js", 900)], proof: OptProof { bindgen_raw: 1, dist_raw: 1 }, contributors: Vec::new(), needed: Vec::new() };
        let reps = [rep];
        assert!(over_caps(&reps, None, None).is_empty());
        assert!(over_caps(&reps, Some(3000), Some(3900)).is_empty(), "at the cap is within it");
        let over = over_caps(&reps, Some(1000), Some(3000));
        assert_eq!(over.len(), 2, "{over:?}");
        assert!(over[0].contains("g_bg.wasm is 3000 bytes") && over[1].contains("3900 bytes raw, over --max-total-bytes 3000"), "{over:?}");
    }

    #[test]
    fn a_module_the_game_cannot_build_without_is_needed_not_fatal() {
        let mut rep = AppReport { app: "g".into(), files: Vec::new(), proof: OptProof { bindgen_raw: 1, dist_raw: 1 }, contributors: Vec::new(), needed: Vec::new() };
        account(&mut rep, "fx.bloom".into(), (5000, 2000), Ok((4600, 1900)));
        account(&mut rep, "nature.ocean".into(), (5000, 2000), Err("trunk build failed
error: this game is all sea
  --> app/src/main.rs".into()));
        assert_eq!(rep.contributors, [("fx.bloom".to_string(), 400, 100)]);
        assert_eq!(rep.needed, ["nature.ocean"]);
        assert_eq!(first_error("trunk build failed
error: this game is all sea"), "error: this game is all sea");
        assert_eq!(first_error("no error word"), "no error word");
        let j = report_json(std::slice::from_ref(&rep));
        assert!(j.contains("\"needed\": [\"nature.ocean\"]") && j.contains("\"module\": \"fx.bloom\""), "{j}");
    }

    #[test]
    fn brotli_shrinks_repetitive_data_more_than_nothing() {
        let data = vec![b'a'; 64 * 1024];
        let br = brotli_len(&data);
        assert!(br > 0 && br < 1024, "brotli {br}");
    }

    #[test]
    fn opt_proof_needs_a_smaller_dist() {
        let ok = OptProof {
            bindgen_raw: 1000,
            dist_raw: 800,
        };
        assert!(ok.applied());
        assert!((ok.saved_pct() - 20.0).abs() < 1e-9);
        let same = OptProof {
            bindgen_raw: 1000,
            dist_raw: 1000,
        };
        assert!(!same.applied());
    }

    #[test]
    fn merge_keeps_other_apps_and_replaces_measured_ones() {
        let old = r#"[{"app": "sandbox", "gz": 1}, {"app": "kart-racer", "gz": 2}]"#;
        let fresh = r#"[{"app": "kart-racer", "gz": 3}, {"app": "match-3", "gz": 4}]"#;
        let merged = merge_report(Some(old), fresh).unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            v,
            serde_json::json!([
                {"app": "sandbox", "gz": 1},
                {"app": "kart-racer", "gz": 3},
                {"app": "match-3", "gz": 4}
            ])
        );
        // No report yet, or an unreadable one: just this run.
        for old in [None, Some("not json")] {
            let v: serde_json::Value =
                serde_json::from_str(&merge_report(old, fresh).unwrap()).unwrap();
            assert_eq!(v.as_array().unwrap().len(), 2);
        }
    }

    #[test]
    fn bin_name_reads_data_bin() {
        let dir = std::env::temp_dir().join("evani-xtask-bin-name-test");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("index.html"),
            "<link data-trunk rel=\"rust\" data-bin=\"evani-thing\" data-wasm-opt=\"z\">",
        );
        assert_eq!(bin_name(&dir, "thing"), "evani-thing");
        assert_eq!(bin_name(&dir.join("missing"), "x"), "evani-x");
    }
}
