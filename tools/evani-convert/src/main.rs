//! `cargo xtask convert ...` runs this (`cargo run -p evani-convert -- ...`
//! from the engine checkout). Exit 0: converted and every check passed;
//! 1: converted, with problems listed in the report; 2: could not convert.

use std::path::PathBuf;
use std::process::ExitCode;

fn root() -> PathBuf {
    let here = std::env::current_dir().unwrap_or_default();
    here.ancestors().find(|a| a.join("catalog").is_dir() && a.join("assets").join("tiers").is_dir()).map(PathBuf::from).unwrap_or(here)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") || args.is_empty() {
        println!("usage: {}", evani_convert::USAGE);
        return ExitCode::SUCCESS;
    }
    let root = root();
    // Tiling texture layers for kit-textures (`cargo xtask convert-textures`).
    if args.first().map(String::as_str) == Some("textures") {
        let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from);
        let force = args.iter().any(|a| a == "--force");
        // `--app-dir <app>`: the layer list and the web root from the game's
        // own folder (W3d, games outside the engine); `--out` still wins.
        let from_app = match flag("--app-dir") {
            Some(app) => match evani_convert::groundtex::app_dir_paths(&app) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("convert-textures: {e}");
                    return ExitCode::from(2);
                }
            },
            None => None,
        };
        let positional = args.iter().enumerate().skip(1).find(|(i, a)| !a.starts_with("--") && !matches!(args.get(i - 1).map(String::as_str), Some("--out" | "--app-dir"))).map(|(_, a)| PathBuf::from(a));
        let spec = positional.or_else(|| from_app.as_ref().map(|p| p.0.clone()));
        let out = flag("--out").or_else(|| from_app.as_ref().map(|p| p.1.clone()));
        let (Some(spec), Some(out)) = (spec, out) else {
            eprintln!("usage: evani-convert textures <layers.json> --out <game assets folder> [--force]\n       evani-convert textures --app-dir <game app folder> [<layers.json>] [--force]");
            return ExitCode::from(2);
        };
        return match evani_convert::groundtex::run(&root, &spec, &out, force) {
            Ok(d) => {
                println!("wrote {} layer files ({} layers up to date) and {}", d.files.len(), d.skipped, d.manifest.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("convert-textures: {e}");
                ExitCode::from(2)
            }
        };
    }
    // The gate's blind test (no model needed).
    if let Some(i) = args.iter().position(|a| a == "--calibration" || a == "--calibration-result") {
        let Some(dir) = args.get(i + 1).map(PathBuf::from) else {
            eprintln!("convert: {} needs a folder\nusage: {}", args[i], evani_convert::USAGE);
            return ExitCode::from(2);
        };
        let r = if args[i] == "--calibration" {
            evani_convert::gate::calibration_page(&root, &dir)
        } else {
            evani_convert::gate::calibration_result(&dir).map(|v| println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()))
        };
        return match r {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("convert: {e}");
                ExitCode::from(2)
            }
        };
    }
    let opt = match evani_convert::Options::parse(&root, &args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("convert: {e}");
            return ExitCode::from(2);
        }
    };
    match evani_convert::convert(&opt) {
        Ok(o) => {
            let r = &o.report;
            println!("converted {} into {}", o.id, o.out_dir.display());
            for t in r["tiers"].as_array().into_iter().flatten() {
                let lods: Vec<String> = t["lods"].as_array().into_iter().flatten().map(|l| l["triangles"].to_string()).collect();
                println!(
                    "  {:<7} {:<28} triangles {:<16} draw calls {:<3} {:>10} bytes ({} gzip)",
                    t["tier"].as_str().unwrap_or(""),
                    t["file"].as_str().unwrap_or(""),
                    lods.join("/"),
                    t["draw_calls"],
                    t["bytes"]["raw"],
                    t["bytes"]["gzip"]
                );
            }
            let p = &r["package"];
            println!("  package {} bytes of the {} cap{}", p["raw"], p["cap"], if p["over_cap"].as_bool() == Some(true) { " (OVER)" } else { "" });
            println!("  report: {}", o.out_dir.join(format!("{}.report.md", o.id)).display());
            for n in r["notes"].as_array().into_iter().flatten() {
                println!("  note: {}", n.as_str().unwrap_or(""));
            }
            if o.errors.is_empty() {
                ExitCode::SUCCESS
            } else {
                for e in &o.errors {
                    eprintln!("ERROR: {e}");
                }
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("convert: {e}");
            ExitCode::from(2)
        }
    }
}
