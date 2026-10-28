//! `evani-geocheck-2d --scene <scene.json> --out <dir> [--tags <geocheck.toml>]`
//!
//! Runs the eight 2D checks on a kit's scene dump and writes
//! `<dir>/report.md`, `<dir>/report.json` and `<dir>/crops/*.png`.
//! Tags come from the scene's `"geocheck"` array and from `geocheck.toml`
//! next to the scene (or `--tags`). Exit code: 0 when every issue is tagged
//! (or there are none), 1 when any untagged issue is left, 2 on an error.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: evani-geocheck-2d --scene <scene.json> --out <dir> [--tags <geocheck.toml>]";

fn run() -> Result<bool, String> {
    let mut scene: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut tags: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--scene" => scene = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "--tags" => tags = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(true);
            }
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    let scene_path = scene.ok_or(USAGE)?;
    let out = out.ok_or(USAGE)?;
    let (scene, atlas) = evani_geocheck_2d::scene::load(&scene_path)?;
    let toml_path = tags.unwrap_or_else(|| scene_path.parent().map_or_else(|| PathBuf::from("geocheck.toml"), |p| p.join("geocheck.toml")));
    let extra = match std::fs::read_to_string(&toml_path) {
        Ok(src) => evani_geocheck_2d::tags::parse_toml(&src)?,
        Err(_) => Vec::new(),
    };
    let issues = evani_geocheck_2d::run_all(&scene, atlas.as_ref(), &extra)?;
    let sum = evani_geocheck_2d::report::write(&out, &scene, atlas.as_ref(), &issues)?;
    println!("evani-geocheck-2d: {} issues, {} failing, {} skipped; report in {}", sum.issues, sum.failing, sum.skipped, out.join("report.md").display());
    Ok(sum.failing == 0)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("evani-geocheck-2d: {e}");
            ExitCode::from(2)
        }
    }
}
