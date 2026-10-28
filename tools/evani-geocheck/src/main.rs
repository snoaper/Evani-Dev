//! `cargo run -p evani-geocheck -- <app>` (or `cargo xtask geocheck <app>`):
//! the geometry integrity report of a game. See docs/GEOCHECK.md.
//!
//! 1. Runs the game natively (no browser) through its kit adapter and
//!    records what it draws: the title screen's attract race, then a race on
//!    the autopilot.
//! 2. Splits the draw lists into the static world and the movers, and runs
//!    the geometry checks: static overlaps, floating objects, z-fighting
//!    faces, mesh validation, moving clipping, camera clipping (bench orbit
//!    and the recorded game cameras) and collision mismatch (wall probes and
//!    a wall-hugging bot).
//! 3. Render phase (skip with `--no-render`): captures in the live game for
//!    gaps (magenta sky) and flicker (sub-pixel jitter), debug-view shots and
//!    a highlighted capture of the worst issues.
//! 4. Applies the intentional-case tags and writes
//!    `.vibedeck/geocheck/<app>/report.md` and `report.json`.
//!
//! Exit code: 0 when every issue is tagged, 1 when any is untagged, 2 when
//! the check could not run.
//!
//! Options: `--title-s 40` (attract race seconds), `--race-s 30`,
//! `--every 6` (sample every n-th frame), `--no-render`, `--port 8095`,
//! `--highlights 40`, `--keep-raw` (keep the raw captures), `--zfight` (the
//! native run and the z-fighting test only: `zfight/zfight.json`, `.md` and
//! debug images from the game cameras; see `cli/zfight.rs`). With
//! `--zfight`: the scan is cached (`cli/cache.rs`), `--report-only`
//! re-renders the report from the cached scan, `--sample N` scans a fixed
//! subset of N cameras into a partial `zfight-sample/` report.

mod cli;

use std::path::{Path, PathBuf};

use evani_core::camera::CameraPose;
use evani_geocheck::checks;
use evani_geocheck::image::Camera;
use evani_geocheck::report::{Check, Report};
use evani_geocheck::scene::{Config, Scene, default_label, split_static};
use evani_geocheck::shape::Shapes;
use evani_geocheck::tags::{self, Tag};
use evani_geocheck::zfight::{self, DepthSetup, Viewer};
use evani_render::mesh::{BUILTIN_MESH_COUNT, MeshId};
use evani_render::world::MeshInstance;
use serde_json::Value;

use cli::{kart, sandbox};
use cli::render::{self, Setup, Shot};

struct Args {
    app: String,
    title_s: f64,
    race_s: f64,
    every: u32,
    render: bool,
    port: u16,
    highlights: usize,
    keep_raw: bool,
    zfight: bool,
    report_only: bool,
    sample: Option<usize>,
}

const USAGE: &str = "usage: evani-geocheck <app> [--title-s 40] [--race-s 30] [--every 6] [--no-render] [--port 8095] [--highlights 40] [--keep-raw] [--zfight [--report-only] [--sample N]]

  --zfight        the native replay and the z-fighting scan only (zfight/zfight.json, .md, debug images).
                  The scan is cached in <target>/evani-geocheck-cache/<app>/<key>/ (key: build, scene,
                  camera set, capture settings); a repeat run with the same key skips the replay and the scan.
  --report-only   with --zfight: re-render the report from the cached scan in seconds; fails, saying what
                  changed, when no cached scan matches (then run without --report-only).
  --sample N      with --zfight: scan a fixed, seeded subset of N cameras for a quick check. The report is
                  partial (zfight-sample/, \"partial\": true): never proof, and `cargo xtask check` refuses it.";

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        app: String::new(),
        title_s: 40.0,
        race_s: 30.0,
        every: 6,
        render: true,
        port: 8095,
        highlights: 40,
        keep_raw: false,
        zfight: false,
        report_only: false,
        sample: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value\n{USAGE}"));
        match x.as_str() {
            "--title-s" => a.title_s = val("--title-s")?.parse().map_err(|_| "bad --title-s")?,
            "--race-s" => a.race_s = val("--race-s")?.parse().map_err(|_| "bad --race-s")?,
            "--every" => a.every = val("--every")?.parse().map_err(|_| "bad --every")?,
            "--port" => a.port = val("--port")?.parse().map_err(|_| "bad --port")?,
            "--highlights" => a.highlights = val("--highlights")?.parse().map_err(|_| "bad --highlights")?,
            "--no-render" => a.render = false,
            "--keep-raw" => a.keep_raw = true,
            "--zfight" => a.zfight = true,
            "--report-only" => a.report_only = true,
            "--sample" => {
                let n: usize = val("--sample")?.parse().map_err(|_| "bad --sample (a number of cameras, at least 1)")?;
                if n == 0 {
                    return Err("bad --sample: at least 1 camera".into());
                }
                a.sample = Some(n);
            }
            "-h" | "--help" => return Err(USAGE.into()),
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n{USAGE}")),
            s => {
                if !a.app.is_empty() {
                    return Err(format!("one app at a time\n{USAGE}"));
                }
                a.app = s.to_string();
            }
        }
    }
    if a.app.is_empty() {
        return Err(USAGE.into());
    }
    if (a.report_only || a.sample.is_some()) && !a.zfight {
        return Err(format!("--report-only and --sample work with --zfight only (the z-fighting scan)\n{USAGE}"));
    }
    Ok(a)
}

/// The workspace root: the nearest folder upwards with a `[workspace]`
/// Cargo.toml.
fn workspace_root() -> Result<PathBuf, String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    for d in here.ancestors() {
        let t = d.join("Cargo.toml");
        if std::fs::read_to_string(&t).is_ok_and(|s| s.contains("[workspace]")) {
            return Ok(d.to_path_buf());
        }
    }
    Err("run it inside the Evani.Dev workspace (no [workspace] Cargo.toml above the current folder)".into())
}

/// Thresholds and tags from `apps/<app>/geocheck.toml`,
/// `kits/evani-kit-<app>/geocheck.toml` and `"geocheck"` arrays in the kit's
/// `data/*.json`.
fn load_config(root: &Path, app: &str, cfg: &mut Config, notes: &mut Vec<String>) -> Result<Vec<Tag>, String> {
    let mut out = Vec::new();
    let kit = root.join("kits").join(format!("evani-kit-{app}"));
    for f in [root.join("apps").join(app).join("geocheck.toml"), kit.join("geocheck.toml")] {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let v: Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        if let Some(t) = v.get("thresholds") {
            cfg.apply_json(t).map_err(|e| format!("{}: {e}", f.display()))?;
        }
        if let Some(t) = v.get("tag") {
            let n = tags::parse(t).map_err(|e| format!("{}: {e}", f.display()))?;
            notes.push(format!("{} tag(s) from {}", n.len(), rel(root, &f)));
            out.extend(n);
        }
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(kit.join("data"))
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        if let Some(t) = v.get("geocheck") {
            let n = tags::parse(t).map_err(|e| format!("{}: {e}", f.display()))?;
            notes.push(format!("{} tag(s) from {}", n.len(), rel(root, &f)));
            out.extend(n);
        }
    }
    Ok(out)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string().replace('\\', "/")
}

fn pose_f32(eye: [f32; 3], target: [f32; 3], fov: f32) -> CameraPose {
    CameraPose {
        eye,
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

/// The cameras of the recorded frames, labelled with their phase ("title",
/// "race"; "orbit" when the recording has no phases).
fn viewers_of(frames: &[checks::Snapshot], phases: &[&str], height: f64) -> Vec<Viewer> {
    frames
        .iter()
        .enumerate()
        .map(|(i, f)| Viewer::new(f.camera, height, phases.get(i).copied().unwrap_or("orbit"), Some(f.time)))
        .collect()
}

/// Check 4's geometry test: the static world from every recorded camera,
/// then each frame's movers and runtime decals from that frame's camera.
/// `flicker_test`: whether the render phase's jitter flicker test runs too
/// (not under `--zfight` or `--no-render`), so the summary says what ran.
fn zfight_checks(report: &mut Report, stat: &Scene, viewers: &[Viewer], dyn_scenes: &[Scene], flicker_test: bool) {
    let set = DepthSetup::engine();
    report.issues.extend(zfight::zfight(stat, set, viewers));
    let frames: Vec<(&Scene, Viewer)> = dyn_scenes.iter().zip(viewers).map(|(d, v)| (d, v.clone())).collect();
    report.issues.extend(zfight::zfight_runtime(stat, &frames, set));
    let c = &stat.cfg;
    report.ran.push((
        Check::ZFight,
        format!(
            "faces of different items parallel within {} degrees, and an item's own opposite-facing faces (thin slabs, back faces drawn), that overlap where the depth buffer can't separate them ({}, near {:.2} m, far {:.0} m, {}; per point the error is one depth step + f32 vertex rounding + sub-pixel snapping by the depth slope; decals counted with their {:.1e} depth offset), seen from {} recorded cameras out to {} m, at least {} px and in sight; movers and runtime decals per frame from that frame's camera{}.",
            c.zfight_angle,
            set.format.name(),
            set.near,
            set.far,
            cli::zfight::reversed_words(&set),
            set.decal_offset,
            viewers.len(),
            c.view_distance.min(set.far),
            c.zfight_px,
            if flicker_test { "; plus the render flicker test" } else { "" }
        ),
    ));
}

/// `--zfight`: the native replay and the z-fighting scan only, through the
/// capture cache (`cli/cache.rs`): the cache key from the build, the scene,
/// the camera set and the capture settings, then [`zfight_run`].
fn run_zfight(args: &Args, root: &Path, out: &Path, report: Report, cfg: Config, tag_list: &[Tag]) -> Result<i32, String> {
    let started = std::time::Instant::now();
    // "; "-separated items, so a stale --report-only names the one that changed.
    let thresholds = format!("{cfg:?}").trim_start_matches("Config {").trim_end_matches('}').trim().replace(", ", "; ").replace(": ", " ");
    let scene = format!(
        "app {}; replay {} s title + {} s race, every {} frames; {thresholds}",
        args.app, args.title_s, args.race_s, args.every
    );
    let cameras = match args.sample {
        Some(n) => format!("--sample {n} (seed {:#x}) of the replay's cameras", cli::cache::SAMPLE_SEED),
        None => "every camera of the replay".to_string(),
    };
    let settings = format!(
        "depth {:?}; viewport {:?}; still switch {}; evani-geocheck {}; cache format {}",
        DepthSetup::engine(),
        kart::VIEWPORT,
        kart::STILL_PARAM,
        env!("CARGO_PKG_VERSION"),
        cli::cache::FORMAT
    );
    let key = cli::cache::Key::compute(root, &args.app, &scene, &cameras, &settings)?;
    let mut rerun = format!("cargo xtask geocheck {} --zfight", args.app);
    if let Some(n) = args.sample {
        rerun.push_str(&format!(" --sample {n}"));
    }
    let z = ZRun {
        root,
        app: &args.app,
        out,
        app_cache: cli::cache::cache_root(root).join(&args.app),
        key,
        report_only: args.report_only,
        render: args.render,
        port: args.port,
        highlights: args.highlights,
        rerun,
    };
    let (code, _) = zfight_run(&z, &cfg, report.notes, tag_list, || zfight_capture(args, &cfg))?;
    eprintln!("geocheck: --zfight took {:.1} s", started.elapsed().as_secs_f64());
    Ok(code)
}

/// What a `--zfight` run needs besides the config and the capture.
struct ZRun<'a> {
    root: &'a Path,
    app: &'a str,
    out: &'a Path,
    /// `<target>/evani-geocheck-cache/<app>`.
    app_cache: PathBuf,
    key: cli::cache::Key,
    report_only: bool,
    render: bool,
    port: u16,
    highlights: usize,
    /// The command that makes a capture (for `--report-only`'s message).
    rerun: String,
}

/// Use the capture saved under the key, or (not `--report-only`) make one
/// with `capture` and save it; then write the report from it. A fresh
/// capture also goes through the saved encoding, so a full run and a later
/// `--report-only` write the same bytes. Returns the exit code and whether
/// the cache was used.
fn zfight_run(z: &ZRun, cfg: &Config, notes: Vec<String>, tag_list: &[Tag], capture: impl FnOnce() -> Result<cli::cache::Capture, String>) -> Result<(i32, bool), String> {
    let dir = z.app_cache.join(&z.key.key);
    let (cap, hit) = match cli::cache::load(&dir) {
        Ok(c) => {
            eprintln!(
                "geocheck: used the cached capture {} ({}): no replay, no z-fight scan",
                z.key.key,
                rel(z.root, &dir)
            );
            (c, true)
        }
        Err(why) if z.report_only => return Err(cli::cache::explain_missing(&z.app_cache, z.app, &z.key, &why, &z.rerun)),
        Err(_) => {
            let fresh = capture()?.to_json();
            match cli::cache::save(&dir, &z.key, &fresh) {
                Ok(()) => eprintln!("geocheck: saved the capture as {} ({})", z.key.key, rel(z.root, &dir)),
                Err(e) => eprintln!("geocheck: warning: the capture was not cached ({e})"),
            }
            (cli::cache::Capture::from_json(&fresh)?, false)
        }
    };
    let stat = cap.scene(cfg.clone());
    let mut notes = notes;
    notes.extend(cap.notes.iter().cloned());
    let mut issues = cap.issues.clone();
    tags::apply(&mut issues, tag_list);
    let partial = cap.sample.as_ref().map(|s| (s.len(), cap.cameras_total));
    let dir_name = cli::zfight::dir_name(partial.is_some());
    let setup = cli::zfight::Setup {
        root: z.root,
        app: z.app,
        out: z.out,
        port: z.port,
        tier: (!cap.tier.is_empty()).then(|| cap.tier.clone()),
        still_param: Some(kart::STILL_PARAM),
        render: z.render,
        shots: z.highlights,
        dir_name,
        partial,
        shots_cache: Some(dir.join("shots")),
        report_only: z.report_only,
    };
    let un = cli::zfight::write(&setup, &stat, &cap.viewers, &mut issues, &cap.ran, &notes)?;
    eprintln!(
        "geocheck: {} z-fighting issue(s), {un} untagged{} -> {}",
        issues.len(),
        partial.map_or(String::new(), |(n, m)| format!(" (PARTIAL: {n} of {m} cameras; not proof)")),
        rel(z.root, &z.out.join(dir_name).join("zfight.md"))
    );
    Ok((if un > 0 { 1 } else { 0 }, hit))
}

/// The expensive part of `--zfight`: the native replay and the z-fighting
/// scan from every recorded camera (or the `--sample`), as a capture.
fn zfight_capture(args: &Args, cfg: &Config) -> Result<cli::cache::Capture, String> {
    eprintln!("geocheck: running {} natively ({} s title, {} s race)", args.app, args.title_s, args.race_s);
    let rec = kart::record(args.title_s, args.race_s, args.every)?;
    if rec.frames.is_empty() {
        return Err("the native run recorded no frames".into());
    }
    let materials = rec.materials.clone();
    let label = |m: &MeshInstance| default_label(m, &materials);
    let lists: Vec<Vec<MeshInstance>> = rec.frames.iter().map(|f| f.meshes.clone()).collect();
    let (statics, dynamic) = split_static(&lists, cfg.static_frac);
    let mut shapes = Shapes::new();
    shapes.add_library(&rec.library);
    for &id in &rec.open_meshes {
        shapes.allow_open(id);
    }
    let decal_from = rec.frames[0].decal_from;
    let stat = Scene::build_with(&statics, &mut shapes, &label, cfg.clone(), &|i| i >= decal_from);
    let races = rec.phases.iter().filter(|p| **p == "race").count();
    let mut notes = vec![format!(
        "Native run: {} sampled frames (every {} frames at 60 fps; {} title, {} race), tier {}; static world {} instances in {} objects, {:.0} x {:.0} m.",
        rec.frames.len(),
        args.every,
        rec.frames.len() - races,
        races,
        if rec.tier.is_empty() { "default" } else { &rec.tier },
        stat.items.len(),
        stat.objects.len(),
        stat.bounds.extent()[0],
        stat.bounds.extent()[2]
    )];
    let all = viewers_of(&rec.frames, &rec.phases, f64::from(kart::VIEWPORT[1]));
    let pick: Vec<usize> = match args.sample {
        Some(n) => cli::cache::sample(all.len(), n, cli::cache::SAMPLE_SEED),
        None => (0..all.len()).collect(),
    };
    if let Some(n) = args.sample {
        notes.push(format!(
            "PARTIAL (--sample {n}): {} of the {} recorded cameras, a fixed seeded subset (seed {:#x}); a quick check, not proof. Run without --sample for the full scan.",
            pick.len(),
            all.len(),
            cli::cache::SAMPLE_SEED
        ));
    }
    // Per-frame scenes of the movers, for the scanned cameras' frames.
    let dyn_scenes: Vec<Scene> = pick
        .iter()
        .map(|&k| {
            let f = &rec.frames[k];
            let mask = &dynamic[k];
            let list: Vec<(usize, MeshInstance)> = f.meshes.iter().enumerate().filter(|(i, _)| mask[*i]).map(|(i, m)| (i, *m)).collect();
            Scene::build_with(&list, &mut shapes, &label, cfg.clone(), &|i| i >= f.decal_from)
        })
        .collect();
    let viewers: Vec<Viewer> = pick.iter().map(|&k| all[k].clone()).collect();
    eprintln!("geocheck: z-fighting ({} cameras)", viewers.len());
    let mut report = Report::default();
    zfight_checks(&mut report, &stat, &viewers, &dyn_scenes, false);
    let ran = report.ran.iter().find(|r| r.0 == Check::ZFight).map_or(String::new(), |r| r.1.clone());
    Ok(cli::cache::Capture {
        tier: rec.tier.clone(),
        materials: rec.materials.clone(),
        decal_from,
        statics,
        library: rec.library.meshes().to_vec(),
        open_meshes: rec.open_meshes.iter().map(|m| m.0).collect(),
        cameras_total: all.len(),
        sample: args.sample.map(|_| pick.clone()),
        viewers,
        issues: report.issues.into_iter().filter(|i| i.check == Check::ZFight).collect(),
        ran,
        notes,
    })
}

fn run() -> Result<i32, String> {
    let args = parse_args()?;
    let root = workspace_root()?;
    if args.app == "sandbox" {
        if args.report_only || args.sample.is_some() {
            return Err("--report-only and --sample: only kart-racer has the cached --zfight scan".into());
        }
        return run_sandbox(&args, &root);
    }
    if args.app != "kart-racer" {
        return Err(format!(
            "no geocheck adapter for {} yet: only kart-racer and sandbox have one (2D games use evani-geocheck-2d). An adapter runs the game natively and says how its collision works; see docs/GEOCHECK.md.",
            args.app
        ));
    }
    let out = render::out_dir(&root, &args.app);
    std::fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;

    let mut report = Report {
        app: args.app.clone(),
        ..Report::default()
    };
    let mut cfg = Config::default();
    let tag_list = load_config(&root, &args.app, &mut cfg, &mut report.notes)?;
    if args.zfight {
        return run_zfight(&args, &root, &out, report, cfg, &tag_list);
    }

    // ---- 1. native run
    eprintln!("geocheck: running {} natively ({} s title, {} s race)", args.app, args.title_s, args.race_s);
    let rec = kart::record(args.title_s, args.race_s, args.every)?;
    if rec.frames.is_empty() {
        return Err("the native run recorded no frames".into());
    }
    let materials = rec.materials.clone();
    let label = |m: &MeshInstance| default_label(m, &materials);
    let role = kart::role(&materials);
    let lists: Vec<Vec<MeshInstance>> = rec.frames.iter().map(|f| f.meshes.clone()).collect();
    let (statics, dynamic) = split_static(&lists, cfg.static_frac);
    let mut shapes = Shapes::new();
    shapes.add_library(&rec.library);
    for &id in &rec.open_meshes {
        shapes.allow_open(id);
    }
    // The draw list of a frame is its meshes followed by its decals.
    let decal_from = rec.frames[0].decal_from;
    let stat = Scene::build_with(&statics, &mut shapes, &label, cfg.clone(), &|i| i >= decal_from);
    let races = rec.phases.iter().filter(|p| **p == "race").count();
    report.notes.push(format!(
        "Native run: {} sampled frames (every {} frames at 60 fps; {} title, {} race), tier {}; static world {} instances in {} objects, {:.0} x {:.0} m.",
        rec.frames.len(),
        args.every,
        rec.frames.len() - races,
        races,
        if rec.tier.is_empty() { "default" } else { &rec.tier },
        stat.items.len(),
        stat.objects.len(),
        stat.bounds.extent()[0],
        stat.bounds.extent()[2]
    ));

    // (`--zfight` went to `run_zfight` above; the guards below stay for it.)
    if !args.zfight {
        // ---- 2. checks on the static world
        eprintln!("geocheck: static checks ({} instances)", stat.items.len());
        report.issues.extend(checks::overlaps(&stat));
        report.ran.push((
            Check::Overlap,
            format!("exact convex intersection volume between parts of different objects, above {} m^3 (sinking up to {} m into terrain or road is contact; decals skipped).", cfg.overlap_volume, cfg.sink),
        ));
        report.issues.extend(checks::floating(&stat));
        report.ran.push((
            Check::Floating,
            format!("rays down from each object's base; gaps above {} m, or nothing below.", cfg.float_gap),
        ));
        let all: Vec<MeshInstance> = lists.iter().flatten().copied().collect();
        let used = checks::used_meshes(&all);
        report.issues.extend(checks::meshes(&shapes, &used, &|id| shapes.open_intended(id)));
        let custom_ids: Vec<MeshId> = used.iter().map(|u| u.0).filter(|id| usize::from(id.0) >= BUILTIN_MESH_COUNT).collect();
        report.ran.push((
            Check::Mesh,
            format!(
                "{} meshes drawn ({} custom), each validated (degenerate, duplicate, open, non-manifold, flipped, inside-out, self-intersecting; the plane is open on purpose).",
                used.len(),
                custom_ids.len()
            ),
        ));
        report.notes.extend(shapes.notes(&custom_ids));
        for (id, n) in &stat.skipped {
            report.notes.push(format!("Not checked: {n} instance(s) of mesh {} (its geometry is not in the mesh library the run ended with).", id.0));
        }
        if rec.library_changed {
            report.notes.push("The mesh library changed during the run: custom meshes are checked as they were at the end of it.".into());
        }

        // ---- moving clipping
        eprintln!("geocheck: moving clipping over {} frames", rec.frames.len());
        report.issues.extend(checks::moving(&stat, &rec.frames, &dynamic, &mut shapes, &label, &role));
        report.ran.push((
            Check::Moving,
            format!(
                "{} sampled frames of the attract race and an autopilot race: movers deeper than {} m into static solids, crossing a solid between samples, leaving the world, or a driver inside its kart's body.",
                rec.frames.len(),
                cfg.penetration
            ),
        ));
    }

    // Per-frame scenes of the movers.
    let dyn_scenes: Vec<Scene> = rec
        .frames
        .iter()
        .zip(&dynamic)
        .map(|(f, mask)| {
            let list: Vec<(usize, MeshInstance)> = f.meshes.iter().enumerate().filter(|(i, _)| mask[*i]).map(|(i, m)| (i, *m)).collect();
            Scene::build_with(&list, &mut shapes, &label, cfg.clone(), &|i| i >= f.decal_from)
        })
        .collect();

    // ---- camera clipping
    let orbit = evani_kit_kart_racer::draw::orbit_path(&rec.track);
    if !args.zfight {
        let orbit_frames: Vec<(f64, Camera, Vec<&Scene>)> = (0..((f64::from(orbit.period_s) / 0.5).ceil() as usize))
            .map(|k| {
                let t = k as f64 * 0.5;
                (t, Camera::from_pose(&orbit.pose(t), kart::ASPECT), vec![&stat])
            })
            .collect();
        let mut cam_issues = checks::camera_clip(&orbit_frames);
        for i in &mut cam_issues {
            i.detail = format!("bench orbit at t={:.1}s: {}", i.time.unwrap_or(0.0), i.detail);
        }
        let game_frames: Vec<(f64, Camera, Vec<&Scene>)> = rec
            .frames
            .iter()
            .zip(&dyn_scenes)
            .map(|(f, d)| (f.time, f.camera, vec![&stat, d]))
            .collect();
        let mut game_cam = checks::camera_clip(&game_frames);
        for i in &mut game_cam {
            i.detail = format!("game camera: {}", i.detail);
        }
        report.ran.push((
            Check::Camera,
            format!(
                "the near plane (eye, corners, edge midpoints, centre) tested against solids along the bench orbit every 0.5 s ({} poses) and in every recorded game frame ({}).",
                orbit_frames.len(),
                game_frames.len()
            ),
        ));
        report.issues.extend(cam_issues);
        report.issues.extend(game_cam);
    }

    // ---- z-fighting, by the depth maths from the recorded cameras
    let viewers = viewers_of(&rec.frames, &rec.phases, f64::from(kart::VIEWPORT[1]));
    eprintln!("geocheck: z-fighting ({} cameras)", viewers.len());
    zfight_checks(&mut report, &stat, &viewers, &dyn_scenes, args.render && !args.zfight);

    // ---- collision mismatch
    let first_race = rec.phases.iter().position(|p| *p == "race");
    let hw = first_race.and_then(|i| kart::visible_half_width(&dyn_scenes[i], &rec.track, &materials));
    let half_width = match hw {
        Some(w) => {
            report.notes.push(format!("Kart drawn half-width {w:.2} m (measured on the starting grid)."));
            w
        }
        None => {
            report.notes.push(format!(
                "No kart found on the starting grid: the kart's half-width is taken as its collision radius ({} m).",
                rec.tune.kart_radius
            ));
            f64::from(rec.tune.kart_radius)
        }
    };
    let probes = kart::probes(&rec.track, &rec.tune, half_width, 2.0);
    let bot = kart::wall_bot(&rec, half_width);
    eprintln!("geocheck: collision ({} probes, {} bot samples)", probes.len(), bot.len());
    report.issues.extend(checks::collision(&stat, &probes, &bot));
    report.ran.push((
        Check::Collision,
        format!(
            "the kit stops a kart's side at {:.2} m from the centreline; {} probes every 2 m on both sides compare that with the first drawn wall (tolerance {} m), and a bot pressed against each wall for a lap ({} samples) checks it is stopped where a wall is drawn.",
            kart::limit(&rec.track, &rec.tune) + half_width,
            probes.len(),
            cfg.collision_tol,
            bot.len()
        ),
    ));

    tags::apply(&mut report.issues, &tag_list);

    // ---- 3. render phase
    if args.render {
        let track = &rec.track;
        let mut views = Vec::new();
        for k in 0..8 {
            let t = f64::from(orbit.period_s) * f64::from(k) / 8.0;
            views.push(Shot {
                name: format!("orbit{}", k + 1),
                pose: orbit.pose(t),
            });
        }
        for k in 0..8 {
            let s = track.length * k as f32 / 8.0;
            let e = track.point_at(s - 8.0, 0.0);
            let g = track.point_at(s + 12.0, 0.0);
            views.push(Shot {
                name: format!("track{}", k + 1),
                pose: pose_f32([e[0], e[1] + 3.0, e[2]], [g[0], g[1] + 1.0, g[2]], 60.0),
            });
        }
        let debug_views: Vec<Shot> = [0usize, 2, 4, 6]
            .iter()
            .map(|&k| Shot {
                name: format!("orbit{}", k + 1),
                pose: views[k].pose,
            })
            .collect();
        let setup = Setup {
            root: &root,
            app: &args.app,
            out: &out,
            port: args.port,
            tier: (!rec.tier.is_empty()).then(|| rec.tier.clone()),
            views,
            debug_views,
            highlights: args.highlights,
            keep_raw: args.keep_raw,
            still_param: Some(kart::STILL_PARAM),
        };
        render::run(&setup, &stat, &mut report)?;
    } else {
        report.ran.push((Check::Gap, "not run (--no-render): gaps need captures in the live game.".into()));
        report.notes.push("Render phase skipped (--no-render): no gap, flicker or highlight captures.".into());
    }

    // ---- 4. tags and the report
    tags::apply(&mut report.issues, &tag_list);
    if !tag_list.is_empty() {
        report.notes.push(format!("{} intentional-case tag(s) applied.", tag_list.len()));
    }
    report.ran.sort_by_key(|r| r.0);
    let md = report.to_markdown();
    let js = serde_json::to_string_pretty(&report.to_json()).map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.md"), md).map_err(|e| format!("write report.md: {e}"))?;
    std::fs::write(out.join("report.json"), js).map_err(|e| format!("write report.json: {e}"))?;
    let un = report.untagged();
    eprintln!(
        "geocheck: {} issue(s), {} untagged -> {}",
        report.issues.len(),
        un,
        rel(&root, &out.join("report.md"))
    );
    for c in Check::ALL {
        eprintln!("  {:<20} {:>4} untagged, {:>3} tagged", c.title(), report.count(c, false), report.count(c, true));
    }
    Ok(if un > 0 { 1 } else { 0 })
}

/// The sandbox: one turn of its orbit natively, the static and moving
/// checks, the camera check along the orbit, no collision (nothing in the
/// sandbox collides), and the render phase from eight orbit poses and eight
/// low views at eye height round the grid.
fn run_sandbox(args: &Args, root: &Path) -> Result<i32, String> {
    let out = render::out_dir(root, &args.app);
    std::fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
    let mut report = Report {
        app: args.app.clone(),
        ..Report::default()
    };
    let mut cfg = Config::default();
    let tag_list = load_config(root, &args.app, &mut cfg, &mut report.notes)?;

    let orbit = sandbox::orbit();
    let seconds = f64::from(orbit.period_s);
    eprintln!("geocheck: running sandbox natively ({seconds} s, one orbit)");
    let rec = sandbox::record(seconds, args.every)?;
    if rec.frames.is_empty() {
        return Err("the native run recorded no frames".into());
    }
    let materials = rec.materials.clone();
    let label = |m: &MeshInstance| default_label(m, &materials);
    let lists: Vec<Vec<MeshInstance>> = rec.frames.iter().map(|f| f.meshes.clone()).collect();
    let (statics, dynamic) = split_static(&lists, cfg.static_frac);
    let mut shapes = Shapes::new();
    shapes.add_library(&rec.library);
    let decal_from = rec.frames[0].decal_from;
    let stat = Scene::build_with(&statics, &mut shapes, &label, cfg.clone(), &|i| i >= decal_from);
    report.notes.push(format!(
        "Native run: {} sampled frames (every {} frames at 60 fps, one {seconds} s orbit), tier {}; static world {} instances in {} objects, {:.0} x {:.0} m.",
        rec.frames.len(),
        args.every,
        if rec.tier.is_empty() { "default" } else { &rec.tier },
        stat.items.len(),
        stat.objects.len(),
        stat.bounds.extent()[0],
        stat.bounds.extent()[2]
    ));
    report.notes.push(
        "Built without fx, so no shadow map: every spinner draws its shadow disc (a decal), as on Low. The spinners turn every frame, so they are movers; the ground, terraces, columns, hills and the discs are the static world.".into(),
    );

    eprintln!("geocheck: static checks ({} instances)", stat.items.len());
    report.issues.extend(checks::overlaps(&stat));
    report.ran.push((
        Check::Overlap,
        format!("exact convex intersection volume between parts of different objects, above {} m^3 (sinking up to {} m into the ground is contact; decals skipped).", cfg.overlap_volume, cfg.sink),
    ));
    report.issues.extend(checks::floating(&stat));
    report.ran.push((Check::Floating, format!("rays down from each object's base; gaps above {} m, or nothing below.", cfg.float_gap)));
    let all: Vec<MeshInstance> = lists.iter().flatten().copied().collect();
    let used = checks::used_meshes(&all);
    report.issues.extend(checks::meshes(&shapes, &used, &|id| shapes.open_intended(id)));
    report.ran.push((
        Check::Mesh,
        format!("{} built-in meshes drawn, each validated (degenerate, duplicate, open, non-manifold, flipped, inside-out, self-intersecting; the plane is open on purpose).", used.len()),
    ));
    for (id, n) in &stat.skipped {
        report.notes.push(format!("Not checked: {n} instance(s) of mesh {} (its geometry is not in the mesh library).", id.0));
    }

    eprintln!("geocheck: moving clipping over {} frames", rec.frames.len());
    report.issues.extend(checks::moving(&stat, &rec.frames, &dynamic, &mut shapes, &label, &sandbox::role));
    report.ran.push((
        Check::Moving,
        format!(
            "{} sampled frames of one orbit: spinners deeper than {} m into static solids (the ground, their shadow discs aside), crossing a solid between samples or leaving the world.",
            rec.frames.len(),
            cfg.penetration
        ),
    ));
    let dyn_scenes: Vec<Scene> = rec
        .frames
        .iter()
        .zip(&dynamic)
        .map(|(f, mask)| {
            let list: Vec<(usize, MeshInstance)> = f.meshes.iter().enumerate().filter(|(i, _)| mask[*i]).map(|(i, m)| (i, *m)).collect();
            Scene::build_with(&list, &mut shapes, &label, cfg.clone(), &|i| i >= f.decal_from)
        })
        .collect();

    let orbit_frames: Vec<(f64, Camera, Vec<&Scene>)> = (0..((seconds / 0.5).ceil() as usize))
        .map(|k| {
            let t = k as f64 * 0.5;
            (t, Camera::from_pose(&orbit.pose(t), sandbox::ASPECT), vec![&stat])
        })
        .collect();
    let mut cam_issues = checks::camera_clip(&orbit_frames);
    let game_frames: Vec<(f64, Camera, Vec<&Scene>)> = rec
        .frames
        .iter()
        .zip(&dyn_scenes)
        .map(|(f, d)| (f.time, f.camera, vec![&stat, d]))
        .collect();
    cam_issues.extend(checks::camera_clip(&game_frames));
    let viewers = viewers_of(&rec.frames, &[], f64::from(sandbox::VIEWPORT[1]));
    eprintln!("geocheck: z-fighting ({} cameras)", viewers.len());
    zfight_checks(&mut report, &stat, &viewers, &dyn_scenes, args.render);
    for i in &mut cam_issues {
        i.detail = format!("orbit camera at t={:.1}s: {}", i.time.unwrap_or(0.0), i.detail);
    }
    report.ran.push((
        Check::Camera,
        format!(
            "the near plane (eye, corners, edge midpoints, centre) tested against solids along the orbit every 0.5 s ({} poses) and in every recorded frame ({}).",
            orbit_frames.len(),
            game_frames.len()
        ),
    ));
    report.issues.extend(cam_issues);
    report.ran.push((
        Check::Collision,
        "not applicable: nothing in the sandbox collides (the spinners turn in place, the camera flies a fixed orbit and there is no player).".into(),
    ));
    tags::apply(&mut report.issues, &tag_list);

    if args.render {
        let mut views = Vec::new();
        for k in 0..8 {
            let t = seconds * f64::from(k) / 8.0;
            views.push(Shot {
                name: format!("orbit{}", k + 1),
                pose: orbit.pose(t),
            });
        }
        // Low views at eye height from inside the terrace ring, each looking
        // across the grid at the far side.
        for k in 0..8 {
            let a = (k as f32 + 0.5) / 8.0 * core::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            views.push(Shot {
                name: format!("low{}", k + 1),
                pose: pose_f32([24.0 * s, 1.7, 24.0 * c], [-10.0 * s, 0.6, -10.0 * c], 55.0),
            });
        }
        let debug_views: Vec<Shot> = [0usize, 2, 4, 6, 8, 12]
            .iter()
            .map(|&k| Shot {
                name: views[k].name.clone(),
                pose: views[k].pose,
            })
            .collect();
        let setup = Setup {
            root,
            app: &args.app,
            out: &out,
            port: args.port,
            tier: (!rec.tier.is_empty()).then(|| rec.tier.clone()),
            views,
            debug_views,
            highlights: args.highlights,
            keep_raw: args.keep_raw,
            // The spinners are movers: held still and hidden for the gap
            // and flicker shots, which judge the static world.
            still_param: Some(sandbox::STILL_PARAM),
        };
        render::run(&setup, &stat, &mut report)?;
    } else {
        report.ran.push((Check::Gap, "not run (--no-render): gaps need captures in the live game.".into()));
        report.notes.push("Render phase skipped (--no-render): no gap, flicker or highlight captures.".into());
    }
    finish(root, &out, report, &tag_list)
}

/// Apply the tags and write `report.md` and `report.json`; the exit code.
fn finish(root: &Path, out: &Path, mut report: Report, tag_list: &[Tag]) -> Result<i32, String> {
    tags::apply(&mut report.issues, tag_list);
    if !tag_list.is_empty() {
        report.notes.push(format!("{} intentional-case tag(s) applied.", tag_list.len()));
    }
    report.ran.sort_by_key(|r| r.0);
    let md = report.to_markdown();
    let js = serde_json::to_string_pretty(&report.to_json()).map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.md"), md).map_err(|e| format!("write report.md: {e}"))?;
    std::fs::write(out.join("report.json"), js).map_err(|e| format!("write report.json: {e}"))?;
    let un = report.untagged();
    eprintln!("geocheck: {} issue(s), {} untagged -> {}", report.issues.len(), un, rel(root, &out.join("report.md")));
    for c in Check::ALL {
        eprintln!("  {:<20} {:>4} untagged, {:>3} tagged", c.title(), report.count(c, false), report.count(c, true));
    }
    Ok(if un > 0 { 1 } else { 0 })
}

#[cfg(test)]
mod zfight_tests;

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("evani-geocheck: {e}");
            std::process::exit(2);
        }
    }
}
