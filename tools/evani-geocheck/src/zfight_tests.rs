//! `--zfight`'s cache, `--report-only` and `--sample`, on a real scan of a
//! small scene (no replay, no live game).

use super::*;
use cli::cache::{Capture, Key};
use std::cell::Cell;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("evani-geocheck-main-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn inst(p: [f32; 3], s: [f32; 3], material: u32) -> MeshInstance {
    let mut m = MeshInstance::new(evani_render::mesh::MeshId::CUBE, p);
    m.scale = s;
    m.material = material;
    m
}

/// A real scan: a road slab and a white line on the same plane (a coplanar
/// fight), from three cameras; `sample` scans a fixed subset, as
/// `--sample` does.
fn capture(sample: Option<usize>) -> Capture {
    let materials = vec!["road".to_string(), "paint".to_string()];
    let statics = vec![(0, inst([0.0, -0.1, 0.0], [10.0, 0.2, 40.0], 0)), (1, inst([0.0, -0.05, 0.0], [0.3, 0.1, 6.0], 1))];
    let label = |m: &MeshInstance| default_label(m, &materials);
    let stat = Scene::build_with(&statics, &mut Shapes::new(), &label, Config::default(), &|_| false);
    let cam = |eye: [f64; 3], t: f64| {
        let c = Camera { eye, target: [0.0; 3], up: [0.0, 1.0, 0.0], fov_y_deg: 60.0, aspect: 16.0 / 9.0 };
        Viewer::new(c, 720.0, "title", Some(t))
    };
    let all = [cam([0.0, 6.0, -15.0], 0.1), cam([0.0, 4.0, 12.0], 0.2), cam([3.0, 5.0, 10.0], 0.3)];
    let pick: Vec<usize> = sample.map_or((0..all.len()).collect(), |n| cli::cache::sample(all.len(), n, cli::cache::SAMPLE_SEED));
    let viewers: Vec<Viewer> = pick.iter().map(|&k| all[k].clone()).collect();
    let dyn_scenes: Vec<Scene> = viewers.iter().map(|_| Scene::build_with(&[], &mut Shapes::new(), &label, Config::default(), &|_| false)).collect();
    let mut report = Report::default();
    zfight_checks(&mut report, &stat, &viewers, &dyn_scenes, false);
    assert!(!report.issues.is_empty(), "the test scene fights");
    Capture {
        tier: "high".into(),
        materials: materials.clone(),
        decal_from: usize::MAX,
        statics,
        library: Vec::new(),
        open_meshes: Vec::new(),
        cameras_total: all.len(),
        sample: sample.map(|_| pick),
        viewers,
        issues: report.issues,
        ran: report.ran[0].1.clone(),
        notes: vec!["Native run: test.".into()],
    }
}

#[allow(clippy::too_many_arguments)]
fn run_once(root: &Path, out: &Path, key: &Key, report_only: bool, render: bool, sample: Option<usize>, calls: &Cell<u32>) -> Result<(i32, bool), String> {
    let z = ZRun {
        root,
        app: "demo",
        out,
        app_cache: root.join("cache/demo"),
        key: key.clone(),
        report_only,
        render,
        port: 1,
        highlights: 40,
        rerun: "cargo xtask geocheck demo --zfight".into(),
    };
    zfight_run(&z, &Config::default(), vec!["1 tag(s) from geocheck.toml".into()], &[], || {
        calls.set(calls.get() + 1);
        Ok(capture(sample))
    })
}

/// The files of a folder (not its subfolders), with their bytes.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().to_string(), std::fs::read(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

fn key(cameras: &str) -> Key {
    Key::from_parts(vec![("src/a.rs".into(), "h".into())], "rustc test", "scene", cameras, "settings", "build")
}

fn setup<'a>(root: &'a Path, out: &'a Path, render: bool, shots_cache: Option<PathBuf>, report_only: bool) -> cli::zfight::Setup<'a> {
    cli::zfight::Setup {
        root,
        app: "demo",
        out,
        port: 1,
        tier: Some("high".into()),
        still_param: Some(kart::STILL_PARAM),
        render,
        shots: 40,
        dir_name: "zfight",
        partial: None,
        shots_cache,
        report_only,
    }
}

#[test]
fn report_only_writes_what_the_full_run_wrote() {
    let root = tmp("same");
    let (full, again, only) = (root.join("full"), root.join("again"), root.join("only"));
    let calls = Cell::new(0);
    let k = key("all");
    // Nothing saved yet: --report-only fails and says what to do.
    let err = run_once(&root, &only, &k, true, false, None, &calls).unwrap_err();
    assert!(err.contains("--report-only") && err.contains("Run without --report-only") && err.contains("No capture is saved for demo yet"), "{err}");
    assert_eq!(calls.get(), 0);
    // The full run scans and saves; the repeat and --report-only reuse it.
    assert_eq!(run_once(&root, &full, &k, false, false, None, &calls).unwrap(), (1, false));
    assert_eq!(calls.get(), 1);
    assert_eq!(run_once(&root, &again, &k, false, false, None, &calls).unwrap(), (1, true), "a repeat run uses the cache");
    assert_eq!(run_once(&root, &only, &k, true, false, None, &calls).unwrap(), (1, true));
    assert_eq!(calls.get(), 1, "no second scan");
    let a = files(&full.join("zfight"));
    assert!(a.iter().any(|f| f.0 == "zfight.json") && a.iter().any(|f| f.0 == "zfight.md"));
    assert_eq!(a, files(&again.join("zfight")), "repeat run: byte-identical");
    assert_eq!(a, files(&only.join("zfight")), "--report-only: byte-identical");
    // And the same as writing straight from the scan, with no cache at all
    // (what --zfight wrote before the cache).
    let direct = root.join("direct");
    let cap = capture(None);
    let materials = cap.materials.clone();
    let label = |m: &MeshInstance| default_label(m, &materials);
    let stat = Scene::build_with(&cap.statics, &mut Shapes::new(), &label, Config::default(), &|_| false);
    let mut issues = cap.issues.clone();
    let notes = vec!["1 tag(s) from geocheck.toml".to_string(), "Native run: test.".to_string()];
    cli::zfight::write(&setup(&root, &direct, false, None, false), &stat, &cap.viewers, &mut issues, &cap.ran, &notes).unwrap();
    assert_eq!(a, files(&direct.join("zfight")), "the cache changes nothing in the report");
    let js: Value = serde_json::from_slice(&a.iter().find(|f| f.0 == "zfight.json").unwrap().1).unwrap();
    assert_eq!(js["partial"], false);
    assert_eq!(js["cameras"], 3);
    assert_eq!(js["cameras_total"], 3);
    // A different key (here the camera set) is a miss that says what changed.
    let err = run_once(&root, &only, &key("other"), true, false, None, &calls).unwrap_err();
    assert!(err.contains("the cameras: saved \"all\", now \"other\""), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn debug_images_come_back_from_the_cached_captures() {
    let root = tmp("shots");
    let calls = Cell::new(0);
    let k = key("all");
    let out = root.join("full");
    // Make the capture (no images), then put raw shots where the cache
    // keeps them, as a live capture would have.
    run_once(&root, &out, &k, false, false, None, &calls).unwrap();
    let cap = capture(None);
    let stat = cap.scene(Config::default());
    let shots = root.join("cache/demo").join(&k.key).join("shots");
    let plan = cli::zfight::planned_shots(&setup(&root, &out, true, Some(shots.clone()), true), &stat, &cap.viewers, &cap.issues);
    assert!(plan.len() >= 2, "an overview and a patch image: {plan:?}");
    // --report-only with a shot missing fails clearly.
    let err = run_once(&root, &root.join("only"), &k, true, true, None, &calls).unwrap_err();
    assert!(err.contains("not in the saved capture") && err.contains("run without --report-only"), "{err}");
    std::fs::create_dir_all(&shots).unwrap();
    for (n, (_, name)) in plan.iter().enumerate() {
        let mut img = evani_geocheck::image::Image::new(320, 180);
        for x in 0..320 {
            img.set(x, (n as u32 * 7) % 180, [10, 20, 30, 255]);
        }
        std::fs::write(shots.join(format!("{name}.png")), cli::png::encode(&img)).unwrap();
    }
    let (a, b) = (root.join("a"), root.join("b"));
    run_once(&root, &a, &k, true, true, None, &calls).unwrap();
    run_once(&root, &b, &k, false, true, None, &calls).unwrap();
    assert_eq!(calls.get(), 1);
    let fa = files(&a.join("zfight"));
    let names: Vec<&String> = fa.iter().map(|f| &f.0).collect();
    assert!(names.iter().any(|n| *n == "overview.png") && names.iter().any(|n| n.starts_with("01-")), "{names:?}");
    assert_eq!(fa, files(&b.join("zfight")), "same images, json and md from the cache, report-only or not");
    assert!(!a.join("zfight/raw").exists() && !b.join("zfight/raw").exists(), "no live capture ran");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_sample_is_fixed_labelled_partial_and_kept_apart() {
    let root = tmp("sample");
    let out = root.join("out");
    let calls = Cell::new(0);
    // The full report first.
    run_once(&root, &out, &key("all"), false, false, None, &calls).unwrap();
    let full = files(&out.join("zfight"));
    let ks = key("--sample 2");
    run_once(&root, &out, &ks, false, false, Some(2), &calls).unwrap();
    assert_eq!(files(&out.join("zfight")), full, "a sample never replaces the full report");
    let dir = out.join("zfight-sample");
    let js: Value = serde_json::from_slice(&std::fs::read(dir.join("zfight.json")).unwrap()).unwrap();
    assert_eq!(js["partial"], true);
    assert_eq!(js["cameras"], 2);
    assert_eq!(js["cameras_total"], 3);
    let md = std::fs::read_to_string(dir.join("zfight.md")).unwrap();
    assert!(md.starts_with("# Z-fighting: demo (PARTIAL: 2 of 3 cameras, --sample 2)"), "{md}");
    assert_eq!(std::fs::read_to_string(dir.join(".gitignore")).unwrap(), "*\n");
    // The same subset every time, and a repeat (report-only) is identical.
    assert_eq!(capture(Some(2)).sample, capture(Some(2)).sample);
    let first = files(&dir);
    run_once(&root, &root.join("again"), &ks, true, false, Some(2), &calls).unwrap();
    assert_eq!(calls.get(), 2, "one full scan, one sample scan");
    assert_eq!(first, files(&root.join("again/zfight-sample")));
    let _ = std::fs::remove_dir_all(&root);
}
