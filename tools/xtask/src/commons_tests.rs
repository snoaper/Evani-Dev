//! Tests for `cargo xtask commons` (Evani.commons packages). Each test makes
//! its own small git repository in the system temp folder: a base commit
//! (the engine) and a column round on a branch.

use super::*;
use std::cell::Cell;

fn git_ok() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("evani-commons-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn rocks(changed: &[(usize, &str)]) -> String {
    (1..=30)
        .map(|i| {
            changed
                .iter()
                .find(|(n, _)| *n == i)
                .map_or(format!("pub fn f{i}() -> f32 {{ {i}.0 }}\n"), |(_, l)| {
                    format!("{l}\n")
                })
        })
        .collect()
}

const LIB_TOML: &str = "[package]\nname = \"evani-procedural-lib\"\nversion = \"0.1.0\"\n\n[features]\nrocks-pebbles = []\n";

/// A repo with `main` (the engine) and `col` (a round on top of it).
fn repo(name: &str, round_extra: &dyn Fn(&Path)) -> PathBuf {
    let r = tmp(name);
    run(&r, &["init", "-q", "-b", "main"]);
    run(&r, &["config", "user.email", "t@example.com"]);
    run(&r, &["config", "user.name", "Test Maker"]);
    run(&r, &["config", "core.autocrlf", "false"]);
    put(
        &r,
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\n",
    );
    put(
        &r,
        "catalog/procedural/library.json",
        "{\"categories\": [{\"id\": \"rocks\", \"assets\": [{\"feature\": \"rocks-pebbles\", \"name\": \"Pebbles\"}]}]}\n",
    );
    put(
        &r,
        "catalog/procedural/references.json",
        "{\n  \"about\": \"x\",\n  \"assets\": {\n    \"rocks-pebbles\": {\n      \"reference\": \"proc-refs/rocks-pebbles.jpg\",\n      \"score\": null,\n      \"status\": \"in review\"\n    }\n  }\n}\n",
    );
    put(&r, "crates/evani-procedural-lib/Cargo.toml", LIB_TOML);
    put(&r, "crates/evani-procedural-lib/src/rocks.rs", &rocks(&[]));
    put(
        &r,
        "crates/evani-procedural-lib/src/old.rs",
        "pub fn old() {}\n",
    );
    put(
        &r,
        "crates/evani-procedural-lib/src/no_nl.rs",
        "pub fn a() {}\npub fn b() {}",
    );
    put(
        &r,
        "crates/evani-material-recipes/shaders/rc_stone.wgsl",
        "fn stone() -> f32 {\n  return 0.5;\n}\n",
    );
    put(&r, "DECISIONS.md", "# Decisions\n");
    run(&r, &["add", "-A"]);
    run(&r, &["commit", "-q", "-m", "base"]);
    run(&r, &["checkout", "-q", "-b", "col"]);
    put(
        &r,
        "crates/evani-procedural-lib/src/rocks.rs",
        &rocks(&[
            (5, "pub fn f5() -> f32 { 5.5 }"),
            (25, "pub fn f25() -> f32 { 25.5 }"),
        ]),
    );
    put(
        &r,
        "crates/evani-procedural-lib/src/pebbles.rs",
        "pub fn pebbles() -> u32 {\n    7\n}\n",
    );
    put(
        &r,
        "crates/evani-procedural-lib/src/no_nl.rs",
        "pub fn a() {}\npub fn b() { let _ = 1; }",
    );
    put(
        &r,
        "crates/evani-procedural-lib/Cargo.toml",
        &format!("{LIB_TOML}rocks-pebbles-wet = []\n"),
    );
    put(
        &r,
        "crates/evani-material-recipes/shaders/rc_stone.wgsl",
        "fn stone() -> f32 {\n  return 0.62;\n}\n",
    );
    std::fs::remove_file(r.join("crates/evani-procedural-lib/src/old.rs")).unwrap();
    put(
        &r,
        "DECISIONS.md",
        "# Decisions\n\n## Fold: pebbles round 4\n",
    );
    round_extra(&r);
    run(&r, &["add", "-A"]);
    run(&r, &["commit", "-q", "-m", "round 4"]);
    run(&r, &["checkout", "-q", "main"]);
    r
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![];
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        let data: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
        wr.write_image_data(&data).unwrap();
    }
    out
}

fn opts(r: &Path, out: &Path) -> ExportOpts {
    let pics = r.join("pics");
    std::fs::create_dir_all(&pics).unwrap();
    std::fs::write(pics.join("ref.png"), png(1800, 1200)).unwrap();
    std::fs::write(pics.join("r4.png"), png(1536, 1024)).unwrap();
    std::fs::write(pics.join("r4-close.png"), png(600, 400)).unwrap();
    std::fs::write(
        pics.join("params.json"),
        "{\"kit.proc_field.pebbles_view\": 0, \"procedural.rocks.pebbles.spread\": 1.25}",
    )
    .unwrap();
    let mut a: Vec<String> = [
        "--asset",
        "rocks-pebbles",
        "--branch",
        "col",
        "--onto",
        "main",
        "--score",
        "3.5",
        "--independent",
        "--reviewer-model",
        "gpt-6.1-sol",
        "--reviewer-account",
        "Reviewer Account",
        "--reviewed-at",
        "2026-11-06T10:00:00Z",
        "--author-model",
        "opus-xhigh",
        "--summary",
        "closer",
        "--column",
        "7",
        "--round",
        "4",
        "--reference-generator",
        "OpenAI (ChatGPT image)",
        "--reference-prompt",
        "a studio photo of pebbles",
        "--scene",
        "pebbles",
        "--view-param",
        "kit.proc_field.pebbles_view",
        "--views",
        "0,1",
        "--size",
        "1536x1024",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (k, v) in [
        ("--reference", pics.join("ref.png")),
        ("--picture", pics.join("r4.png")),
        ("--picture", pics.join("r4-close.png")),
        ("--params", pics.join("params.json")),
        ("--out", out.to_path_buf()),
    ] {
        a.push(k.to_string());
        a.push(v.to_string_lossy().into_owned());
    }
    ExportOpts::from_args(r, &a).unwrap()
}

/// A copy of the engine at `main`, checked out with Windows line ends.
fn clone(r: &Path, name: &str) -> PathBuf {
    let d = tmp(name);
    let o = Command::new("git")
        .args(["clone", "-q", "-c", "core.autocrlf=true", "-b", "main"])
        .arg(r)
        .arg(&d)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    d
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    list_files(root)
        .unwrap()
        .into_iter()
        .filter(|f| !f.starts_with(".git/"))
        .map(|f| {
            let b = std::fs::read(root.join(&f)).unwrap();
            (f, b)
        })
        .collect()
}

fn no_check(_: &Path) -> i32 {
    0
}

fn imp(root: &Path) -> ImportOpts {
    ImportOpts {
        root: root.to_path_buf(),
        dry_run: false,
        server_score: None,
        version_id: None,
        check: false,
        when: Some("2026-11-06T12:00:00Z".into()),
    }
}

#[test]
fn export_import_round_trip() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    let r = repo("trip", &|_| {});
    let out = r.join("out");
    let ex = export(&opts(&r, &out)).unwrap();
    let name = ex.dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("rocks-pebbles@photoreal-"), "{name}");
    assert!(
        ex.report
            .contains("left out (not library code): DECISIONS.md"),
        "{}",
        ex.report
    );
    let pkg = verify(&ex.dir).unwrap();
    let m = &pkg.manifest;
    assert_eq!(m["format"], FORMAT);
    assert_eq!(m["area"], "official");
    assert_eq!(m["asset"]["name"], "Pebbles");
    assert_eq!(m["asset"]["category"], "rocks");
    assert_eq!(m["asset"]["feature"], "evani-procedural-lib/rocks-pebbles");
    assert_eq!(m["score"]["value"], 3.5);
    assert_eq!(m["score"]["independent"], true);
    assert_eq!(m["licence"]["spdx"], "MIT");
    assert_eq!(m["licence"]["author"], "Test Maker");
    assert_eq!(m["engine"]["version"], "0.1.0");
    assert_eq!(
        m["engine"]["commit"].as_str().unwrap(),
        git_s(&r, &["rev-parse", "main"]).unwrap()
    );
    assert!(m["engine"]["base_blobs"]["crates/evani-procedural-lib/src/rocks.rs"].is_string());
    assert_eq!(m["render"]["scene_feature"], "pebbles");
    let paths: Vec<&str> = m["code"]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 6, "{paths:?}");
    assert!(
        !paths
            .iter()
            .any(|p| p.contains("DECISIONS") || p.contains("references.json"))
    );
    // Pictures: small JPEGs; the reference within 1600 px, rounds within 1024.
    let rf = std::fs::read(ex.dir.join("reference.jpg")).unwrap();
    assert_eq!(jpeg_size(&rf), Some((1600, 1067)));
    assert_eq!(
        jpeg_size(&std::fs::read(ex.dir.join("rounds/round4.jpg")).unwrap()),
        Some((1024, 683))
    );
    assert_eq!(
        jpeg_size(&std::fs::read(ex.dir.join("rounds/round4-v1.jpg")).unwrap()),
        Some((600, 400))
    );
    assert_eq!(m["pictures"].as_array().unwrap().len(), 2);
    assert!(pkg.bytes < 2 * 1024 * 1024, "{}", pkg.bytes);
    // Exporting the same round again makes the same package.
    let again = export(&opts(&r, &out)).unwrap();
    assert_eq!(again.content_hash, ex.content_hash);

    // Adopt into another copy of the engine (Windows line ends).
    let d = clone(&r, "trip-dest");
    let res = import(&ex.dir, &imp(&d), &no_check).unwrap();
    assert_eq!(res.written.len(), 6, "{}", res.report);
    for p in &paths {
        let want = git_show(&r, "col", p).unwrap();
        let got = std::fs::read(d.join(p)).ok();
        assert_eq!(got.as_deref().map(|b| lf(b).0), want, "{p}");
    }
    assert!(!d.join("crates/evani-procedural-lib/src/old.rs").exists());
    let rocks_now = std::fs::read(d.join("crates/evani-procedural-lib/src/rocks.rs")).unwrap();
    assert!(
        lf(&rocks_now).1,
        "the project's own CRLF line ends are kept"
    );
    // Recorded, and the library's score written in place.
    let rec: Value = serde_json::from_str(
        &std::fs::read_to_string(d.join("catalog/procedural/commons.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rec["adopted"]["rocks-pebbles@photoreal"]["version_id"],
        pkg.hash12()
    );
    assert_eq!(rec["adopted"]["rocks-pebbles@photoreal"]["score"], 3.5);
    let refs = std::fs::read_to_string(d.join("catalog/procedural/references.json")).unwrap();
    assert!(refs.contains("\"score\": 3.5,"), "{refs}");
    // Package pictures never reach the project.
    assert!(
        !snapshot(&d)
            .keys()
            .any(|f| f.ends_with(".jpg") || f.ends_with(".png"))
    );
    // Adopting again changes nothing.
    let res2 = import(&ex.dir, &imp(&d), &no_check).unwrap();
    assert!(res2.written.is_empty(), "{}", res2.report);
    assert_eq!(res2.already.len(), 6);
}

#[test]
fn import_merges_a_local_change_elsewhere_in_the_file() {
    if !git_ok() {
        return;
    }
    let r = repo("merge", &|_| {});
    let ex = export(&opts(&r, &r.join("out"))).unwrap();
    let d = clone(&r, "merge-dest");
    let local = rocks(&[(15, "pub fn f15() -> f32 { 15.25 }")]).replace('\n', "\r\n");
    std::fs::write(d.join("crates/evani-procedural-lib/src/rocks.rs"), local).unwrap();
    let res = import(&ex.dir, &imp(&d), &no_check).unwrap();
    assert_eq!(
        res.merged,
        vec!["crates/evani-procedural-lib/src/rocks.rs".to_string()]
    );
    let got = lf(&std::fs::read(d.join("crates/evani-procedural-lib/src/rocks.rs")).unwrap()).0;
    let want = rocks(&[
        (5, "pub fn f5() -> f32 { 5.5 }"),
        (15, "pub fn f15() -> f32 { 15.25 }"),
        (25, "pub fn f25() -> f32 { 25.5 }"),
    ]);
    assert_eq!(String::from_utf8(got).unwrap(), want);
}

#[test]
fn a_conflicting_adopt_changes_nothing() {
    if !git_ok() {
        return;
    }
    let r = repo("conflict", &|_| {});
    let ex = export(&opts(&r, &r.join("out"))).unwrap();
    let d = clone(&r, "conflict-dest");
    // The adopter's engine changed the same line the version changes.
    std::fs::write(
        d.join("crates/evani-procedural-lib/src/rocks.rs"),
        rocks(&[(5, "pub fn f5() -> f32 { 9.0 }")]),
    )
    .unwrap();
    let before = snapshot(&d);
    let e = import(&ex.dir, &imp(&d), &no_check).err().unwrap();
    assert!(
        e.contains("can't be adopted") && e.contains("Nothing was changed"),
        "{e}"
    );
    assert!(
        e.contains("crates/evani-procedural-lib/src/rocks.rs"),
        "{e}"
    );
    assert_eq!(snapshot(&d), before, "no partial import");
    assert!(!d.join("catalog/procedural/commons.json").exists());
}

#[test]
fn a_check_that_turns_unclean_undoes_the_adopt() {
    if !git_ok() {
        return;
    }
    let r = repo("undo", &|_| {});
    let ex = export(&opts(&r, &r.join("out"))).unwrap();
    let d = clone(&r, "undo-dest");
    let before = snapshot(&d);
    let calls = Cell::new(0);
    let check = |_: &Path| -> i32 {
        calls.set(calls.get() + 1);
        if calls.get() == 1 { 0 } else { 1 }
    };
    let mut o = imp(&d);
    o.check = true;
    let e = import(&ex.dir, &o, &check).err().unwrap();
    assert!(e.contains("undone"), "{e}");
    assert_eq!(calls.get(), 2);
    assert_eq!(snapshot(&d), before);
}

#[test]
fn a_changed_package_is_refused() {
    if !git_ok() {
        return;
    }
    let r = repo("tamper", &|_| {});
    let ex = export(&opts(&r, &r.join("out"))).unwrap();
    for (file, from, to) in [
        ("patch.diff", "5.5", "6.5"),
        ("commons.json", "\"value\": 3.5", "\"value\": 4.5"),
        ("params.json", "1.25", "2.25"),
    ] {
        let d = tmp(&format!("tamper-{}", file.replace('.', "-")));
        for f in list_files(&ex.dir).unwrap() {
            put(&d, &f, "");
            std::fs::copy(ex.dir.join(&f), d.join(&f)).unwrap();
        }
        let t = std::fs::read_to_string(d.join(file)).unwrap();
        assert!(t.contains(from), "{file}");
        std::fs::write(d.join(file), t.replacen(from, to, 1)).unwrap();
        let e = verify(&d).err().unwrap();
        assert!(e.contains("content hash"), "{file}: {e}");
    }
    // A stray file is refused too.
    let d = tmp("tamper-extra");
    for f in list_files(&ex.dir).unwrap() {
        put(&d, &f, "");
        std::fs::copy(ex.dir.join(&f), d.join(&f)).unwrap();
    }
    put(&d, "run.sh", "echo hi\n");
    assert!(verify(&d).err().unwrap().contains("doesn't belong"));
}

#[test]
fn build_output_in_a_package_is_refused_and_a_new_export_cleans_it() {
    if !git_ok() {
        return;
    }
    // Office round 2 (2026-11-07): the end-of-turn check built into target/ inside the
    // package folder after the packer had made it, and Evani.App refused the share.
    let r = repo("buildout", &|_| {});
    let ex = export(&opts(&r, &r.join("out"))).unwrap();
    put(
        &ex.dir,
        "target/guide/CACHEDIR.TAG",
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    );
    put(&ex.dir, "target/guide/debug/.cargo-lock", "");
    let e = verify(&ex.dir).err().unwrap();
    assert!(
        e.contains("build output") && e.contains("target/guide/"),
        "{e}"
    );
    for f in [
        "target/x",
        "node_modules/a/b.js",
        "code/base/x.rlib",
        "a.d",
        "Cargo.lock",
        ".evani/tmp/x.json",
    ] {
        assert!(is_build_output(f), "{f}");
    }
    for f in [
        "commons.json",
        "code/base/crates/evani-procedural-lib/src/rocks.rs",
        "rounds/round2.jpg",
    ] {
        assert!(!is_build_output(f), "{f}");
    }
    // The same round exported again: the same folder, now clean, and it verifies.
    let again = export(&opts(&r, &r.join("out"))).unwrap();
    assert_eq!(again.dir, ex.dir);
    assert_eq!(again.content_hash, ex.content_hash);
    assert!(!again.dir.join("target").exists());
    assert!(verify(&again.dir).is_ok());
    assert!(
        list_files(&again.dir)
            .unwrap()
            .iter()
            .all(|f| !is_build_output(f))
    );
}

#[test]
fn unsafe_code_is_refused() {
    if !git_ok() {
        return;
    }
    let r = repo("unsafe", &|r| {
        put(
            r,
            "crates/evani-procedural-lib/src/pebbles.rs",
            "pub fn p() {\n    let _ = std::process::Command::new(\"calc\");\n}\n",
        )
    });
    let e = export(&opts(&r, &r.join("out"))).err().unwrap();
    assert!(
        e.contains("runs other programs") && e.contains("pebbles.rs"),
        "{e}"
    );
}

#[test]
fn a_new_dependency_is_refused() {
    if !git_ok() {
        return;
    }
    let r = repo("dep", &|r| {
        put(
            r,
            "crates/evani-procedural-lib/Cargo.toml",
            &format!("{LIB_TOML}\n[dependencies]\nleftpad = \"1\"\n"),
        )
    });
    let e = export(&opts(&r, &r.join("out"))).err().unwrap();
    assert!(e.contains("only the [features] table"), "{e}");
}

#[test]
fn a_build_script_or_lock_file_is_refused() {
    if !git_ok() {
        return;
    }
    let r = repo("build-rs", &|r| {
        put(r, "crates/evani-procedural-lib/build.rs", "fn main() {}\n")
    });
    assert!(
        export(&opts(&r, &r.join("out")))
            .err()
            .unwrap()
            .contains("build scripts")
    );
    let r = repo("lock", &|r| put(r, "Cargo.lock", "# changed\n"));
    assert!(
        export(&opts(&r, &r.join("out")))
            .err()
            .unwrap()
            .contains("dependencies")
    );
}

#[test]
fn the_author_s_own_score_is_refused() {
    if !git_ok() {
        return;
    }
    let r = repo("self", &|_| {});
    let mut o = opts(&r, &r.join("out"));
    o.independent = false;
    assert!(export(&o).err().unwrap().contains("independent"));
}

#[test]
fn path_rules() {
    assert_eq!(
        path_problem("crates/evani-procedural-lib/src/rocks.rs"),
        None
    );
    assert_eq!(
        path_problem("crates/evani-material-recipes/shaders/rc_stone.wgsl"),
        None
    );
    assert_eq!(
        path_problem("crates/evani-material-nature/src/water.rs"),
        None
    );
    assert_eq!(
        path_problem("examples/proc-field/kits/evani-kit-proc-field/src/pebbles.rs"),
        None
    );
    assert_eq!(path_problem("catalog/procedural/library.json"), None);
    for bad in [
        "catalog/procedural/references.json",
        "crates/evani-core/src/lib.rs",
        "Cargo.lock",
        "crates/evani-procedural-lib/Cargo.lock",
        "crates/evani-procedural-lib/build.rs",
        "crates/evani-procedural-lib/src/../../evil.rs",
        "crates/evani-procedural-lib/assets/rock.png",
        "/crates/evani-procedural-lib/src/a.rs",
        "crates/evani-procedural-lib/src/a b.rs",
        "tools/xtask/src/main.rs",
    ] {
        assert!(path_problem(bad).is_some(), "{bad}");
    }
}

#[test]
fn line_rules() {
    for bad in [
        "use std::fs;",
        "use std::{fs, io};",
        "use std::{ io, process::Command };",
        "let _ = std :: process :: exit(1);",
        "unsafe { core::ptr::null::<u8>().read() };",
        "let t = include_str!(\"../../../secrets.txt\");",
        "extern \"C\" { fn x(); }",
        "#[link(name = \"k\")]",
        "let s = std::net::TcpStream::connect(\"a:1\");",
        "let h = std::env::var(\"HOME\");",
        "include!(\"x.rs\");",
    ] {
        assert!(line_problem(bad).is_some(), "{bad}");
    }
    for ok in [
        "let v = 1.0;",
        "// std::process is not used here",
        "let unsafe_ish = 2;",
        "#![forbid(unsafe_code)]",
        "let t = include_str!(\"shaders/rc_stone.wgsl\");",
        "use std::{f32::consts::PI, collections::BTreeMap};",
    ] {
        assert!(line_problem(ok).is_none(), "{ok}");
    }
}

#[test]
fn cargo_toml_rules() {
    let base = LIB_TOML.as_bytes();
    let feat = format!("{LIB_TOML}rocks-wet = [\"rocks-pebbles\"]\n");
    assert_eq!(
        cargo_toml_problem(
            "crates/evani-procedural-lib/Cargo.toml",
            Some(base),
            Some(feat.as_bytes())
        ),
        None
    );
    let dep = format!("{LIB_TOML}\n[build-dependencies]\ncc = \"1\"\n");
    assert!(
        cargo_toml_problem(
            "crates/evani-procedural-lib/Cargo.toml",
            Some(base),
            Some(dep.as_bytes())
        )
        .is_some()
    );
    assert!(
        cargo_toml_problem("crates/evani-procedural-lib/Cargo.toml", None, Some(base)).is_some()
    );
}

#[test]
fn gate_table() {
    let g = |s: Option<f64>, i: bool, p: Option<f64>| gate(s, i, p, FLOOR).likely;
    assert!(g(Some(4.0), true, Some(3.5)), "higher passes");
    assert!(!g(Some(3.5), true, Some(3.5)), "equal never replaces");
    assert!(!g(Some(3.0), true, Some(3.5)), "lower never replaces");
    assert!(g(Some(3.0), true, None), "none published: the floor");
    assert!(!g(Some(2.5), true, None), "below the floor");
    assert!(
        !g(Some(5.0), false, None),
        "the author's own score never counts"
    );
    assert!(!g(None, true, None), "no score yet");
    assert!(
        gate(Some(3.5), true, None, 3.5).likely,
        "a floor p-app sets"
    );
    assert!(
        gate(Some(3.0), true, Some(2.5), FLOOR)
            .reason
            .contains("likely to pass")
    );
}

#[test]
fn content_hash_definition() {
    let d = tmp("hash");
    put(&d, "a.txt", "hello\n");
    let manifest = "{\n  \"x\": 1,\n  \"content_hash\": \"\"\n}\n";
    put(&d, "commons.json", manifest);
    let want = format!(
        "sha256:{}",
        sha_hex(
            format!(
                "{}  a.txt\n{}  commons.json\n",
                sha_hex(b"hello\n"),
                sha_hex(manifest.as_bytes())
            )
            .as_bytes()
        )
    );
    assert_eq!(content_hash(&d).unwrap(), want);
    // Filled in, the hash is unchanged (the placeholder is put back first).
    let hex = want.trim_start_matches("sha256:");
    put(
        &d,
        "commons.json",
        &manifest.replace(
            HASH_PLACEHOLDER,
            &format!("\"content_hash\": \"sha256:{hex}\""),
        ),
    );
    assert_eq!(content_hash(&d).unwrap(), want);
}

#[test]
fn pictures_become_small_jpegs() {
    let (j, w, h) = picture_jpeg(&png(2000, 1000), 1024).unwrap();
    assert!(j.starts_with(&[0xFF, 0xD8]));
    assert_eq!((w, h), (1024, 512));
    assert_eq!(jpeg_size(&j), Some((1024, 512)));
    // A small JPEG is kept byte for byte; a big one is refused.
    assert_eq!(picture_jpeg(&j, 1024).unwrap().0, j);
    assert!(picture_jpeg(&j, 800).is_err());
    assert!(!png_has_c2pa(&png(4, 4)));
}

#[test]
fn reference_score_is_set_in_place() {
    let t = "{\n  \"assets\": {\n    \"a-b\": {\n      \"note\": \"a \\\"score\\\": 9 {x}\",\n      \"score\": null,\n      \"status\": \"s\"\n    },\n    \"c-d\": {\n      \"score\": 2.5\n    }\n  }\n}\n";
    let n = set_reference_score(t, "c-d", 4.0).unwrap();
    assert!(n.contains("\"c-d\": {\n      \"score\": 4.0\n"), "{n}");
    assert!(n.contains("\"score\": null"));
    let n = set_reference_score(t, "a-b", 3.5).unwrap();
    assert!(n.contains("\"score\": 3.5,\n      \"status\""), "{n}");
    assert!(set_reference_score(t, "x-y", 1.0).is_none());
}

#[test]
fn hand_made_patches_apply() {
    // No newline at the end, on both sides.
    let p = "diff --git a/crates/evani-procedural-lib/src/a.rs b/crates/evani-procedural-lib/src/a.rs\nindex 1..2 100644\n--- a/crates/evani-procedural-lib/src/a.rs\n+++ b/crates/evani-procedural-lib/src/a.rs\n@@ -1,2 +1,2 @@\n x\n-y\n\\ No newline at end of file\n+z\n\\ No newline at end of file\n";
    let fps = parse_patch(p).unwrap();
    assert_eq!(apply(Some(b"x\ny"), &fps[0]).unwrap().unwrap(), b"x\nz");
    assert!(
        apply(Some(b"x\nq"), &fps[0]).is_err(),
        "context must match the base"
    );
    // A renamed file never travels.
    assert!(parse_patch("diff --git a/crates/evani-procedural-lib/src/a.rs b/crates/evani-procedural-lib/src/b.rs\nsimilarity index 90%\n").is_err());
    assert!(parse_patch("diff --git a/crates/evani-procedural-lib/src/a.rs b/crates/evani-procedural-lib/src/a.rs\nBinary files differ\n").is_err());
}

/// A smooth picture (a colour ramp with a soft blob), so a smaller JPEG copy of
/// it compares as the same picture; `other` makes a different one.
fn smooth_png(w: u32, h: u32, other: bool) -> Vec<u8> {
    let mut out = vec![];
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
                let (cx, cy) = if other { (0.75, 0.3) } else { (0.3, 0.6) };
                let blob = (-((u - cx).powi(2) + (v - cy).powi(2)) * 18.0).exp();
                let ramp = if other { 1.0 - u } else { u };
                data.extend([
                    (40.0 + 150.0 * ramp + 60.0 * blob) as u8,
                    (60.0 + 120.0 * v + 50.0 * blob) as u8,
                    (200.0 - 120.0 * ramp * v) as u8,
                ]);
            }
        }
        wr.write_image_data(&data).unwrap();
    }
    out
}

/// The round's reference (a PNG) and a repo whose catalogue reference file
/// for pebbles was once `locked` (a later commit replaced it).
fn locked_repo(name: &str, locked: &[u8]) -> PathBuf {
    let r = repo(name, &|_| {});
    let rel = "apps/material-view/assets/proc-refs/rocks-pebbles.jpg";
    std::fs::create_dir_all(r.join(rel).parent().unwrap()).unwrap();
    std::fs::write(r.join(rel), locked).unwrap();
    run(&r, &["add", "-A"]);
    run(&r, &["commit", "-q", "-m", "seeded viewer picture"]);
    let (later, _, _) = picture_jpeg(&smooth_png(900, 600, true), 600).unwrap();
    std::fs::write(r.join(rel), later).unwrap();
    run(&r, &["add", "-A"]);
    run(&r, &["commit", "-q", "-m", "a later viewer picture"]);
    r
}

fn with_reference(r: &Path, out: &Path, reference: &[u8], locked: Option<&str>) -> ExportOpts {
    let mut o = opts(r, out);
    let p = r.join("pics").join("scored-ref.png");
    std::fs::write(&p, reference).unwrap();
    o.reference = Some(p);
    o.locked_sha256 = locked.map(str::to_string);
    o
}

#[test]
fn reference_sha256_names_the_reference_file_not_its_preview() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    // Mountains r11/r12 (2026-11-07): the package named its re-encoded preview, so
    // the server (which holds the PNG itself) refused it as "a different reference".
    let r = repo("refsha", &|_| {});
    let reference = smooth_png(1800, 1200, false);
    let ex = export(&with_reference(&r, &r.join("out"), &reference, None)).unwrap();
    let m = verify(&ex.dir).unwrap().manifest;
    let preview = std::fs::read(ex.dir.join("reference.jpg")).unwrap();
    assert_eq!(
        m["reference"]["sha256"].as_str(),
        Some(sha_hex(&reference).as_str())
    );
    assert_eq!(m["reference"]["source_sha256"], m["reference"]["sha256"]);
    assert_ne!(
        m["reference"]["sha256"].as_str(),
        Some(sha_hex(&preview).as_str())
    );
    // The locked hash given and equal to the reference: nothing else is looked up.
    let same = sha_hex(&reference);
    let ex2 = export(&with_reference(
        &r,
        &r.join("out2"),
        &reference,
        Some(&same),
    ))
    .unwrap();
    assert_eq!(
        verify(&ex2.dir).unwrap().manifest["reference"]["sha256"].as_str(),
        Some(same.as_str())
    );
    // A JPEG reference small enough is its own preview: both hashes agree.
    let (j, _, _) = picture_jpeg(&reference, 768).unwrap();
    let ex3 = export(&with_reference(&r, &r.join("out3"), &j, None)).unwrap();
    assert_eq!(
        verify(&ex3.dir).unwrap().manifest["reference"]["sha256"].as_str(),
        Some(sha_hex(&j).as_str())
    );
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn locked_reference_of_the_same_picture_is_named() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    // Office, Pebbles, Shorelines, Oak: Evani.app locked the catalogue's smaller JPEG copy of
    // the very picture the column scores against (seeded 2026-11-06, replaced since).
    let reference = smooth_png(1536, 1024, false);
    let (locked, _, _) = picture_jpeg(&reference, 768).unwrap();
    let r = locked_repo("lockedsame", &locked);
    let lh = sha_hex(&locked);
    let ex = export(&with_reference(&r, &r.join("out"), &reference, Some(&lh))).unwrap();
    let m = verify(&ex.dir).unwrap().manifest;
    assert_eq!(m["reference"]["sha256"].as_str(), Some(lh.as_str()));
    assert_eq!(
        m["reference"]["source_sha256"].as_str(),
        Some(sha_hex(&reference).as_str())
    );
    assert!(ex.report.contains("the same picture"), "{}", ex.report);
    let d = picture_difference(&reference, &locked).unwrap().unwrap();
    assert!(d < 3.0, "a JPEG copy measured {d}");
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn locked_reference_of_another_picture_is_refused() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    // Clouds: the server holds an older picture than the one the column scores
    // against; naming it would claim a score the round wasn't given.
    let (locked, _, _) = picture_jpeg(&smooth_png(1536, 1024, true), 768).unwrap();
    let r = locked_repo("lockedother", &locked);
    let reference = smooth_png(1536, 1024, false);
    let d = picture_difference(&reference, &locked).unwrap().unwrap();
    assert!(d > SAME_PICTURE_MAX, "another picture measured {d}");
    let lh = sha_hex(&locked);
    let e = export(&with_reference(&r, &r.join("out"), &reference, Some(&lh)))
        .err()
        .unwrap();
    assert!(
        e.contains("made against a different reference") && e.contains("a different picture"),
        "{e}"
    );
    assert!(!r.join("out").join("rocks-pebbles@photoreal").exists());
    // Another shape (a two-view panel vs a studio still) is never the same picture.
    let wide = smooth_png(2048, 768, false);
    assert_eq!(picture_difference(&wide, &locked).unwrap(), None);
    // A locked hash nowhere in the catalogue's history: refused, with both hashes.
    let missing = "ab".repeat(32);
    let e = export(&with_reference(
        &r,
        &r.join("out2"),
        &reference,
        Some(&missing),
    ))
    .err()
    .unwrap();
    assert!(
        e.contains("isn't in this project's history") && e.contains("abababababab"),
        "{e}"
    );
    // A bad hash is refused when read.
    let mut a = vec![
        "--asset".to_string(),
        "rocks-pebbles".into(),
        "--locked-sha256".into(),
        "xyz".into(),
    ];
    assert!(
        ExportOpts::from_args(&r, &a)
            .err()
            .unwrap()
            .contains("64 hex")
    );
    a[3] = "AB".repeat(32);
    assert_eq!(
        ExportOpts::from_args(&r, &a).unwrap().locked_sha256,
        Some("ab".repeat(32))
    );
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn locked_ref_puts_the_locked_picture_in_the_project() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    // Office r2, Townhouses r5, Jagged rocks r6, Rock piles r5, Sky dome r5 (2026-11-11): the
    // columns scored against the tab's own picture while Evani.app had locked another. The tab
    // now scores a locked item against the locked picture, which this command puts in the project.
    let reference = smooth_png(1536, 1024, false);
    let (locked, _, _) = picture_jpeg(&reference, 768).unwrap();
    let r = locked_repo("lockedfetch", &locked);
    let lh = sha_hex(&locked);
    let want = format!(".evani/commons/locked/rocks-pebbles/photoreal/locked-{}.jpg", &lh[..12]);
    // Only in the history (a later commit replaced the catalogue file): taken from there.
    let a = fetch_locked(&r, "rocks-pebbles", "photoreal", &lh.to_uppercase(), "file:///nowhere").unwrap();
    assert_eq!(a.path, want);
    assert!(a.from.contains(" at "), "{}", a.from);
    assert_eq!(std::fs::read(r.join(&want)).unwrap(), locked);
    // Asked again: it is already there.
    let b = fetch_locked(&r, "rocks-pebbles", "photoreal", &lh, "file:///nowhere").unwrap();
    assert_eq!((b.path.as_str(), b.from.as_str()), (want.as_str(), "already in the project"));
    // A round scored against that file packs with the locked hash as its own reference.
    let mut o = opts(&r, &r.join("out"));
    o.reference = Some(r.join(&want));
    o.locked_sha256 = Some(lh.clone());
    let ex = export(&o).unwrap();
    assert_eq!(verify(&ex.dir).unwrap().manifest["reference"]["sha256"].as_str(), Some(lh.as_str()));
    // Not in the project: read from the public API (a file:// copy of it here), kept only by its hash.
    let site = r.join("site");
    let api = site.join("public/commons/rocks-pebbles/toon");
    std::fs::create_dir_all(&api).unwrap();
    let other = smooth_png(640, 480, true);
    std::fs::write(api.join("reference"), &other).unwrap();
    let url = format!("file:///{}", site.display().to_string().replace('\\', "/").trim_start_matches('/'));
    let oh = sha_hex(&other);
    let c = fetch_locked(&r, "rocks-pebbles", "toon", &oh, &url).unwrap();
    assert_eq!(c.path, format!(".evani/commons/locked/rocks-pebbles/toon/locked-{}.png", &oh[..12]));
    assert_eq!(std::fs::read(r.join(&c.path)).unwrap(), other);
    // The site answering another picture: refused, nothing written.
    let wrong = "cd".repeat(32);
    let e = fetch_locked(&r, "rocks-pebbles", "toon", &wrong, &url).unwrap_err();
    assert!(e.contains("not the locked cdcdcdcdcdcd") && e.contains("nothing written"), "{e}");
    let left: Vec<_> = std::fs::read_dir(r.join(".evani/commons/locked/rocks-pebbles/toon")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left.len(), 1, "{left:?}");
    // No answer at all, and bad keys or hashes: refused.
    assert!(fetch_locked(&r, "rocks-pebbles", "lowpoly", &wrong, "file:///no/such/site").unwrap_err().contains("didn't answer"));
    assert!(fetch_locked(&r, "../x", "photoreal", &lh, &url).unwrap_err().contains("library keys"));
    assert!(fetch_locked(&r, "rocks-pebbles", "photoreal", "xyz", &url).unwrap_err().contains("64 hex"));
    let _ = std::fs::remove_dir_all(&r);
}

// ---------- the server's preflight, mirrored (2026-11-08 refusals) ----------

/// Office r2's dev-scene Cargo.toml hunk, as the server refused it: several
/// quoted features on one line.
const OFFICE_TOML_DIFF: &str = "diff --git a/examples/proc-field/apps/proc-field/Cargo.toml b/examples/proc-field/apps/proc-field/Cargo.toml\nindex 25e8aa6..e0e873a 100644\n--- a/examples/proc-field/apps/proc-field/Cargo.toml\n+++ b/examples/proc-field/apps/proc-field/Cargo.toml\n@@ -169,4 +169,4 @@ canyon = [\n office = [\n     \"procedural\", \"evani-procedural-lib/buildings-office-buildings\",\n-    \"dep:evani-fx\", \"evani-fx/shadows\",\n+    \"dep:evani-fx\", \"evani-fx/shadows\", \"evani-fx/vignette\",\n ]\n";

fn one_file(path: &str, added: &[&str]) -> String {
    let mut d = format!(
        "diff --git a/{path} b/{path}\nindex 1111111..2222222 100644\n--- a/{path}\n+++ b/{path}\n@@ -1,1 +1,{} @@\n x\n",
        added.len() + 1
    );
    for l in added {
        d.push_str(&format!("+{l}\n"));
    }
    d
}

#[test]
fn server_preflight_refuses_office_r2s_cargo_toml_lines() {
    let p = server_patch_problems(OFFICE_TOML_DIFF);
    assert_eq!(
        p,
        vec![
            "The patch changes more than the features in examples/proc-field/apps/proc-field/Cargo.toml."
                .to_string()
        ]
    );
    // One entry per line is a feature line; so are a list's head and end.
    let ok = one_file(
        "crates/evani-procedural-lib/Cargo.toml",
        &[
            "[features]",
            "rocks-pebbles-wet = []",
            "office = [",
            "    \"evani-fx/vignette\",",
            "    \"evani-render/gtao\" # soft shadows",
            "],",
            "",
            "# a note",
            "terrain = [\"dep:x\", \"y\"]",
        ],
    );
    assert!(
        server_patch_problems(&ok).is_empty(),
        "{:?}",
        server_patch_problems(&ok)
    );
    for bad in [
        "serde = \"1\"",
        "[dependencies]",
        "x = { version = \"1\" }",
        "\"a\", \"b\",",
        "] x",
    ] {
        assert!(!feature_line(bad), "{bad}");
    }
}

#[test]
fn server_preflight_mirrors_the_paths_files_and_code_rules() {
    let p = server_patch_problems(&one_file("docs/x.md", &["hi"]));
    assert!(p[0].contains("outside the engine folders"), "{p:?}");
    let p = server_patch_problems(&one_file(
        "crates/evani-procedural-lib/build.rs",
        &["fn main() {}"],
    ));
    assert!(p.iter().any(|s| s.contains("build scripts")), "{p:?}");
    let p = server_patch_problems(&one_file("crates/evani-procedural-lib/Cargo.lock", &["x"]));
    assert!(p.iter().any(|s| s.contains("Cargo.lock")), "{p:?}");
    let link = "diff --git a/crates/evani-grass/src/l.rs b/crates/evani-grass/src/l.rs\nnew file mode 120000\nindex 0000000..1111111\n--- /dev/null\n+++ b/crates/evani-grass/src/l.rs\n@@ -0,0 +1 @@\n+../x\n";
    assert!(server_patch_problems(link).iter().any(|s| s.contains("a link")));
    let bin = "diff --git a/crates/evani-grass/src/a.rs b/crates/evani-grass/src/a.rs\nindex 1..2 100644\nBinary files a/crates/evani-grass/src/a.rs and b/crates/evani-grass/src/a.rs differ\n";
    assert!(server_patch_problems(bin).iter().any(|s| s.contains("binary")));
    // Added Rust lines, comments included (the server reads every added line).
    for (line, hit) in [
        ("const S: &str = include_str!(\"s.wgsl\");", "include!"),
        ("// unsafe is never needed here", "unsafe"),
        ("let p = std :: fs::read(x);", "std::fs"),
        ("extern \"C\" { fn f(); }", "extern \"\u{2026}\""),
        ("#![link(name = \"m\")]", "#[link]"),
        ("#[path = \"x.rs\"] mod x;", "#[path]"),
        ("let v = option_env!(\"X\");", "env!"),
        ("let v = env !(\"X\");", "env!"),
        ("use proc_macro::TokenStream;", "proc_macro"),
        ("global_asm!(\"nop\");", "asm!"),
    ] {
        assert_eq!(server_code_hit(line), Some(hit), "{line}");
    }
    for line in [
        "let environment = 1.0; // env_scale",
        "#[link_section = \"x\"]",
        "fn include_map() {}",
        "let unsafely = 2;",
        "pub fn rocks() -> f32 { 0.5 }",
    ] {
        assert_eq!(server_code_hit(line), None, "{line}");
    }
    let p = server_patch_problems(&one_file(
        "crates/evani-procedural-lib/src/a.rs",
        &["let x = 1; // keep unsafe out"],
    ));
    assert_eq!(
        p,
        vec![
            "The patch adds unsafe in crates/evani-procedural-lib/src/a.rs, which an Evani.commons version may not use."
                .to_string()
        ]
    );
    // The dev scene and the library list are inside.
    assert!(
        server_patch_problems(&one_file(
            "examples/proc-field/kits/evani-kit-proc-field/src/office.rs",
            &["pub fn f() {}"]
        ))
        .is_empty()
    );
    assert!(server_patch_problems(&one_file("catalog/procedural/library.json", &["{}"])).is_empty());
}

#[test]
fn triangle_budgets_follow_the_shared_table_and_stay_flat_until_live() {
    assert_eq!(budget_kind(Some("scene"), "terrain", "terrain-canyons"), "landscape");
    assert_eq!(budget_kind(Some("scene"), "water", "water-shorelines"), "landscape");
    assert_eq!(budget_kind(Some("scene"), "water", "water-rivers"), "landscape");
    assert_eq!(budget_kind(Some("scene"), "water", "water-ocean-surface"), "sky-water");
    assert_eq!(
        budget_kind(Some("scene"), "sky", "sky-volumetric-looking-clouds"),
        "sky-water"
    );
    assert_eq!(budget_kind(Some("object"), "terrain", "terrain-x"), "object");
    assert_eq!(budget_kind(None, "rocks", "rocks-pebbles"), "object");
    // Today (flat 100,000 on the live server): Canyons r7 and Shorelines r10 are refused.
    let canyons = triangle_problem(367_652, "landscape", false).unwrap();
    assert!(
        canyons.contains("367,652") && canyons.contains("100,000"),
        "{canyons}"
    );
    assert!(triangle_problem(344_282, "landscape", false).is_some());
    assert!(triangle_problem(100_000, "object", false).is_none());
    // Once the budgets by kind are live: a landscape gets 500,000 at High.
    assert!(triangle_problem(367_652, "landscape", true).is_none());
    assert_eq!(triangle_budget("sky-water", true), 250_000);
    let o = triangle_problem(100_001, "object", true).unwrap();
    assert!(o.contains("an object at the High tier is 100,000"), "{o}");
}

#[test]
fn an_engine_base_older_than_the_render_app_is_refused() {
    let why = engine_base_problem("5f1dd26e74e0aaaa", Some(false)).unwrap();
    assert!(why.contains("5f1dd26e74e0") && why.contains("ea0dd440"), "{why}");
    assert!(engine_base_problem("abc", Some(true)).is_none());
    assert!(engine_base_problem("abc", None).is_none());
}

#[test]
fn export_refuses_what_the_server_would_and_leaves_out_the_dev_scene_on_request() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    let r = repo("preflight", &|_: &Path| {});
    // The engine gets the dev scene's Cargo.toml; the round changes it like Office r2.
    let app = "examples/proc-field/apps/proc-field/Cargo.toml";
    run(&r, &["checkout", "-q", "main"]);
    put(
        &r,
        app,
        "[package]\nname = \"pf\"\n\n[features]\npebbles = [\n    \"a\", \"b\",\n]\n",
    );
    run(&r, &["add", "-A"]);
    run(&r, &["commit", "-q", "-m", "dev scene"]);
    run(&r, &["checkout", "-q", "col"]);
    run(&r, &["merge", "-q", "--no-edit", "main"]);
    put(
        &r,
        app,
        "[package]\nname = \"pf\"\n\n[features]\npebbles = [\n    \"a\", \"b\", \"c\",\n]\n",
    );
    run(&r, &["add", "-A"]);
    run(&r, &["commit", "-q", "-m", "round 4b"]);
    run(&r, &["checkout", "-q", "main"]);
    let out = r.join("out");
    let e = match export(&opts(&r, &out)) {
        Ok(_) => panic!("exported"),
        Err(e) => e,
    };
    assert!(
        e.contains("server preflight") && e.contains("more than the features"),
        "{e}"
    );
    assert!(e.contains(&format!("--leave-out {app}")), "{e}");
    let mut o = opts(&r, &out);
    o.leave_out = vec![app.into()];
    let x = export(&o).unwrap();
    assert!(x.report.contains("--leave-out: dev scene only"), "{}", x.report);
    assert!(
        x.report.contains("WARN: the triangle count wasn't checked"),
        "{}",
        x.report
    );
    let patch = std::fs::read_to_string(x.dir.join("patch.diff")).unwrap();
    assert!(!patch.contains("apps/proc-field/Cargo.toml"));
    assert!(server_patch_problems(&patch).is_empty());
    // Only dev-scene files may be left out, and only ones the round changes.
    let mut o = opts(&r, &out);
    o.leave_out = vec!["crates/evani-procedural-lib/src/rocks.rs".into()];
    assert!(export(&o).err().unwrap().contains("only dev-scene files"));
    let mut o = opts(&r, &out);
    o.leave_out = vec![app.into(), "examples/proc-field/x.rs".into()];
    assert!(export(&o).err().unwrap().contains("doesn't change that file"));
    // Stats over the server's flat limit are refused before a package is made.
    let stats = r.join("stats.json");
    std::fs::write(&stats, "{\"triangles\": 120000, \"vertices\": 1}").unwrap();
    let mut o = opts(&r, &out);
    o.leave_out = vec![app.into()];
    o.stats = Some(stats.clone());
    let e = export(&o).err().unwrap();
    assert!(
        e.contains("120,000 triangles") && e.contains("never cut it down by hand"),
        "{e}"
    );
    std::fs::write(&stats, "{\"triangles\": 20000}").unwrap();
    let x = export(&o).unwrap();
    assert!(
        x.report.contains("triangles: 20000 at the high tier"),
        "{}",
        x.report
    );
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn category_scores_are_checked_and_the_total_worked_out() {
    let ok = r#"{"scores":{"silhouette":14,"structure":9,"material":12,"colour":16,"lighting":15},
                 "reasons":{"silhouette":"outline close","structure":" fused base "},"total":64}"#;
    let v = categories_value(ok, Some(3.0)).unwrap();
    // 14*25 + 9*25 + 12*20 + 16*15 + 15*15 = 1280; / 20 = 64
    assert_eq!(v["total"], 64.0);
    assert_eq!(v["scores"]["structure"], 9);
    assert_eq!(v["reasons"]["structure"], "fused base");
    assert!(v["reasons"].get("material").is_none());
    assert_eq!(v["weights"]["silhouette"], 25);
    // no total given: worked out; no score: no star check
    let v = categories_value(
        r#"{"scores":{"silhouette":20,"structure":20,"material":20,"colour":20,"lighting":19}}"#,
        None,
    )
    .unwrap();
    assert_eq!(v["total"], 99.3);
    for (bad, why) in [
        (r#"{"scores":{"silhouette":14,"structure":9,"material":12,"colour":16}}"#, "`lighting` needs"),
        (r#"{"scores":{"silhouette":21,"structure":9,"material":12,"colour":16,"lighting":15}}"#, "`silhouette` needs"),
        (r#"{"scores":{"silhouette":12.5,"structure":9,"material":12,"colour":16,"lighting":15}}"#, "`silhouette` needs"),
        (r#"{"scores":{"silhouette":14,"structure":9,"material":12,"colour":16,"lighting":15,"sheen":3}}"#, "unknown category `sheen`"),
        (r#"{"scores":{"silhouette":14,"structure":9,"material":12,"colour":16,"lighting":15},"total":80}"#, "the scores give 64"),
        (r#"{"scores":{"silhouette":14,"structure":9,"material":12,"colour":16,"lighting":15},"reasons":{"shine":"x"}}"#, "unknown category `shine`"),
        ("not json", "not JSON"),
    ] {
        let e = categories_value(bad, None).unwrap_err();
        assert!(e.contains(why), "{bad}: {e}");
    }
    let long = format!(
        r#"{{"scores":{{"silhouette":14,"structure":9,"material":12,"colour":16,"lighting":15}},"reasons":{{"colour":"{}"}}}}"#,
        "x".repeat(241)
    );
    assert!(categories_value(&long, None).unwrap_err().contains("over 240"));
    // the half-star score must be the total's: 64/100 is 3.0 stars, not 3.5
    assert!(categories_value(ok, Some(3.5)).unwrap_err().contains("is 3 stars"));
}

#[test]
fn a_categories_round_carries_its_scores_into_the_package() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    let r = repo("cats", &|_| {});
    let out = r.join("out");
    let mut o = opts(&r, &out);
    let f = r.join("pics").join("cats.json");
    std::fs::write(
        &f,
        r#"{"scores":{"silhouette":14,"structure":14,"material":14,"colour":14,"lighting":14},"reasons":{"structure":"two boulders fused"},"total":70}"#,
    )
    .unwrap();
    o.categories = Some(f.clone());
    let ex = export(&o).unwrap();
    let pkg = verify(&ex.dir).unwrap();
    let c = &pkg.manifest["score"]["categories"];
    assert_eq!(pkg.manifest["score"]["value"], 3.5);
    assert_eq!(c["total"], 70.0);
    assert_eq!(c["scores"]["lighting"], 14);
    assert_eq!(c["reasons"]["structure"], "two boulders fused");
    // a total that isn't the round's half-star score is refused before anything is packed
    std::fs::write(
        &f,
        r#"{"scores":{"silhouette":20,"structure":20,"material":20,"colour":20,"lighting":20}}"#,
    )
    .unwrap();
    let e = export(&o).err().unwrap_or_default();
    assert!(e.contains("is 5 stars, but --score is 3.5"), "{e}");
    // without --categories the manifest has no categories field (classic rounds are packed as before)
    let ex = export(&opts(&r, &r.join("out2"))).unwrap();
    assert!(verify(&ex.dir).unwrap().manifest["score"].get("categories").is_none());
}

#[test]
fn the_library_build_lists_imported_versions_by_ref() {
    // `cargo xtask library build` (library.rs) reads what `commons import`
    // recorded: a version that changed code is included, one whose code the
    // project already had is `already`, each as asset@style#hash12.
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    let r = repo("library", &|_| {});
    let a = export(&opts(&r, &r.join("out-a"))).unwrap();
    let mut o = opts(&r, &r.join("out-b"));
    o.style = "stylized".into();
    let b = export(&o).unwrap();
    let d = clone(&r, "library-dest");
    assert_eq!(crate::library::lists_in(&d).unwrap(), crate::library::Lists::default(), "nothing adopted yet");
    import(&a.dir, &imp(&d), &no_check).unwrap();
    import(&b.dir, &imp(&d), &no_check).unwrap();
    let h = |c: &str| c.trim_start_matches("sha256:")[..12].to_string();
    let l = crate::library::lists_in(&d).unwrap();
    assert_eq!(l.included, vec![format!("rocks-pebbles@photoreal#{}", h(&a.content_hash))]);
    assert_eq!(l.already, vec![format!("rocks-pebbles@stylized#{}", h(&b.content_hash))]);
    assert!(l.in_base.is_empty());
    // Once committed, the same records are the base's own.
    run(&d, &["add", "-A"]);
    run(&d, &["-c", "user.email=t@example.com", "-c", "user.name=T", "commit", "-q", "-m", "adopted"]);
    let l = crate::library::lists_in(&d).unwrap();
    assert_eq!((l.included.len(), l.already.len(), l.in_base.len()), (0, 0, 2));
}

/// The server views Asset Studio shoots and scores a studio-object round in
/// (tools/studio/server-views.mjs): the same render app, view 0 and drawn
/// views as Evani.App's server check, pinned by its JS tests
/// (tools/studio/server-views.test.mjs). Skipped without Node.
#[test]
fn the_server_views_are_the_servers_own() {
    let root = crate::workspace_root();
    let Ok(out) = Command::new("node").arg("--test").arg(root.join("tools/studio/server-views.test.mjs")).current_dir(&root).output() else {
        eprintln!("node not found: the server-views tests were skipped");
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "node --test failed:\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("fail 0"), "{text}");
}

// ---------- a round already folded into the main branch ----------

const LIB_WITH_RECORD: &str = "{\"categories\": [{\"id\": \"rocks\", \"assets\": [{\"feature\": \"rocks-pebbles\", \"name\": \"Pebbles\", \"realism\": {\"score\": 3.5}}]}]}\n";

fn commit_on(r: &Path, branch: &str, files: &[(&str, &str)], msg: &str) -> String {
    run(r, &["checkout", "-q", branch]);
    for (p, t) in files {
        put(r, p, t);
    }
    run(r, &["add", "-A"]);
    run(r, &["commit", "-q", "-m", msg]);
    git_s(r, &["rev-parse", "HEAD"]).unwrap()
}

/// `repo` with the round (its library entry carries a review record) folded
/// into main: an unrelated commit on main, the fold (a merge), then whatever
/// `between` commits, then the "render app" commit. Returns (repo, fold, render app).
fn folded_repo(name: &str, between: &dyn Fn(&Path)) -> (PathBuf, String, String) {
    let r = repo(name, &|r: &Path| {
        put(r, "catalog/procedural/library.json", LIB_WITH_RECORD);
    });
    commit_on(&r, "main", &[("docs/a.md", "a\n")], "engine moves on");
    run(
        &r,
        &["merge", "-q", "--no-ff", "-m", "Join col: fold: pebbles round 4", "col"],
    );
    let fold = git_s(&r, &["rev-parse", "HEAD"]).unwrap();
    between(&r);
    let render = commit_on(
        &r,
        "main",
        &[(
            "examples/proc-field/apps/commons-render/README.md",
            "render app\n",
        )],
        "the server's render app",
    );
    (r, fold, render)
}

fn o_for(r: &Path, render: &str) -> ExportOpts {
    let mut o = opts(r, &r.join("out"));
    o.render_app = render.to_string();
    o
}

#[test]
fn a_folded_round_packs_as_the_main_branch_commit_that_holds_it() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    // After the fold, main rewrites only the entry's review record (as folds do).
    let (r, fold, render) = folded_repo("folded", &|r: &Path| {
        commit_on(
            r,
            "main",
            &[(
                "catalog/procedural/library.json",
                &LIB_WITH_RECORD.replace("3.5", "4.0"),
            )],
            "a later fold records its score",
        );
    });
    let round = git_s(&r, &["rev-parse", "col"]).unwrap();
    // The round's own engine is older than the render app: the old wall.
    let mut old = o_for(&r, &render);
    old.base = Some(git_s(&r, &["rev-parse", "col~1"]).unwrap());
    assert!(
        export(&old)
            .err()
            .unwrap()
            .contains("older than the server's render app")
    );
    let o = o_for(&r, &render);
    let ex = export(&o).unwrap();
    let pkg = verify(&ex.dir).unwrap();
    let m = &pkg.manifest;
    // The engine is the first main commit with the render app; the patch is empty.
    assert_eq!(m["engine"]["commit"].as_str().unwrap(), render);
    assert_eq!(m["version"]["commit"].as_str().unwrap(), round);
    assert_eq!(m["version"]["fold"].as_str().unwrap(), fold);
    assert_eq!(m["code"]["folded"]["fold"].as_str().unwrap(), fold);
    assert_eq!(m["code"]["bytes"], 0);
    assert_eq!(std::fs::read(ex.dir.join("patch.diff")).unwrap().len(), 0);
    assert!(m["code"]["paths"].as_array().unwrap().is_empty());
    assert!(server_patch_problems("").is_empty());
    assert!(pkg.patches.is_empty());
    // Every file the round changed, compared with the engine's copy.
    let recs = m["code"]["folded"]["files"].as_array().unwrap();
    let by = |p: &str| {
        recs.iter()
            .find(|f| f["path"] == p)
            .unwrap_or_else(|| panic!("{p}: {recs:?}"))
    };
    assert_eq!(by("crates/evani-procedural-lib/src/rocks.rs")["compared"], "file");
    assert_eq!(by("crates/evani-procedural-lib/src/pebbles.rs")["status"], "added");
    assert_eq!(by("crates/evani-procedural-lib/src/old.rs")["status"], "deleted");
    assert_eq!(by("crates/evani-procedural-lib/src/old.rs")["copy"], false);
    assert_eq!(by("crates/evani-procedural-lib/Cargo.toml")["compared"], "features");
    assert_eq!(by("catalog/procedural/library.json")["compared"], "library entry");
    // Proof: each carried copy is the scored round's own file, byte for byte.
    let mut copies = 0;
    for f in recs.iter().filter(|f| f["copy"] == true) {
        let p = f["path"].as_str().unwrap();
        let carried = std::fs::read(ex.dir.join("code/base").join(p)).unwrap();
        assert_eq!(Some(carried), git_show(&r, &round, p).unwrap(), "{p}");
        assert_eq!(f["round_blob"], f["engine_blob"], "{p}");
        copies += 1;
    }
    assert_eq!(copies, 4);
    // The four copies and the deleted file.
    assert_eq!(pkg.folded.len(), 5, "{:?}", pkg.folded.keys());
    assert!(ex.report.contains("folded round:"), "{}", ex.report);
    // Packing again gives the same package (main moving on doesn't change it).
    commit_on(&r, "main", &[("docs/b.md", "b\n")], "later");
    let again = export(&o).unwrap();
    assert_eq!(again.content_hash, ex.content_hash);
    // --dry-run: the same hash, nothing kept.
    let mut d = opts(&r, &r.join("dry-out"));
    d.render_app = render.clone();
    d.dry_run = true;
    let dry = export(&d).unwrap();
    assert_eq!(dry.content_hash, ex.content_hash);
    assert!(!dry.dir.exists() && !r.join("dry-out").exists());
    // Adopting: a project at the engine commit already has it; an older one is
    // refused, never written over.
    let at = clone(&r, "folded-at");
    run(&at, &["checkout", "-q", &render]);
    let ok = import(&ex.dir, &imp(&at), &no_check).unwrap();
    assert!(ok.written.is_empty() && ok.already.len() == 5, "{}", ok.report);
    let before = clone(&r, "folded-before");
    run(&before, &["checkout", "-q", &format!("{fold}^1")]);
    let snap = snapshot(&before);
    let e = import(&ex.dir, &imp(&before), &no_check).err().unwrap();
    assert!(e.contains("part of engine commit") && e.contains("rocks.rs"), "{e}");
    assert_eq!(snap, snapshot(&before));
    // A tampered copy no longer verifies.
    let p = ex
        .dir
        .join("code/base/crates/evani-procedural-lib/src/rocks.rs");
    std::fs::write(&p, "changed\n").unwrap();
    assert!(verify(&ex.dir).is_err());
    for d in [r, at, before] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn a_never_folded_round_still_packs_as_its_change() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    let r = repo("unfolded", &|_: &Path| {});
    let main = git_s(&r, &["rev-parse", "main"]).unwrap();
    // The round's base has the render app.
    let ex = export(&o_for(&r, &main)).unwrap();
    let pkg = verify(&ex.dir).unwrap();
    let m = &pkg.manifest;
    assert_eq!(m["engine"]["commit"].as_str().unwrap(), main);
    assert!(m["code"]["folded"].is_null());
    assert!(m["version"]["fold"].is_null());
    assert_eq!(pkg.patches.len(), 6);
    assert!(pkg.folded.is_empty());
    assert!(!ex.report.contains("folded round"));
    // A render app the round's base lacks is still refused, as before.
    let later = commit_on(&r, "main", &[("docs/x.md", "x\n")], "render app later");
    assert!(
        export(&o_for(&r, &later))
            .err()
            .unwrap()
            .contains("older than the server's render app")
    );
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn a_folded_round_whose_files_changed_since_is_refused() {
    if !git_ok() {
        eprintln!("skipped: no git");
        return;
    }
    // A later round rewrites rocks.rs on main before the render app arrives:
    // no main commit with the render app holds round 4's file any more.
    let (r, _fold, render) = folded_repo("superseded", &|r: &Path| {
        commit_on(
            r,
            "main",
            &[(
                "crates/evani-procedural-lib/src/rocks.rs",
                &rocks(&[(5, "pub fn f5() -> f32 { 9.9 }")]),
            )],
            "round 5 folded",
        );
    });
    let e = export(&o_for(&r, &render)).err().unwrap();
    assert!(
        e.contains("no main-branch commit with the server's render app still holds what was scored")
            && e.contains("rocks.rs")
            && e.contains("isn't the round's file"),
        "{e}"
    );
    assert!(!r.join("out").exists() || list_files(&r.join("out")).unwrap().is_empty());
    // The round's file back as it was on a later commit (a revert): that one is used.
    let scored = git_show(&r, "col", "crates/evani-procedural-lib/src/rocks.rs")
        .unwrap()
        .unwrap();
    let back = commit_on(
        &r,
        "main",
        &[(
            "crates/evani-procedural-lib/src/rocks.rs",
            std::str::from_utf8(&scored).unwrap(),
        )],
        "revert to round 4",
    );
    let ex = export(&o_for(&r, &render)).unwrap();
    assert_eq!(
        verify(&ex.dir).unwrap().manifest["engine"]["commit"]
            .as_str()
            .unwrap(),
        back
    );
    // A Cargo feature the round added that main later changed is refused too.
    let (r2, _f, render2) = folded_repo("superseded-feature", &|r: &Path| {
        commit_on(
            r,
            "main",
            &[(
                "crates/evani-procedural-lib/Cargo.toml",
                &format!("{LIB_TOML}rocks-pebbles-wet = [\"x\"]\n"),
            )],
            "feature changed",
        );
    });
    let e = export(&o_for(&r2, &render2)).err().unwrap();
    assert!(e.contains("rocks-pebbles-wet"), "{e}");
    for d in [r, r2] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// The picture notes (visual-review.json) Asset Studio helpers hand to
/// Evani.App's visual review: written by tools/studio/visual-review.mjs with
/// every field computed and checked by Evani.App's rules (numbers or text
/// only), pinned by its JS tests (tools/studio/visual-review.test.mjs).
/// Skipped without Node.
#[test]
fn the_picture_notes_pass_evani_apps_check() {
    let root = crate::workspace_root();
    // With Evani.App's backup capture hook (.vibedeck/visual-capture.json -> tools/studio/review-backup.mjs), run as Evani.App runs it.
    let Ok(out) = Command::new("node")
        .arg("--test")
        .arg(root.join("tools/studio/visual-review.test.mjs"))
        .arg(root.join("tools/studio/review-backup.test.mjs"))
        .current_dir(&root)
        .output()
    else {
        eprintln!("node not found: the visual-review tests were skipped");
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "node --test failed:\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("fail 0"), "{text}");
}

#[test]
fn the_exporter_never_writes_the_servers_staging_params() {
    for k in ["kit.proc_field.staging", "kit.proc_field.view_preset", "kit.proc_field.sweep_floor", "kit.proc_field.cam_yaw_deg", "kit.proc_field.cam_aim_y", "kit.proc_field.cam_lens_deg", "kit.proc_field.sun_pitch_deg", "kit.proc_field.studio_backdrop", "kit.proc_field.studio_warmth"] {
        let mut m = Map::new();
        m.insert(k.to_string(), Value::from(1.0));
        let p = params_problem(&m).unwrap_or_default();
        assert!(p.contains("the server's to set"), "{k}: {p}");
    }
    let mut m = Map::new();
    m.insert("kit.rocks_pebbles.count".to_string(), Value::from(10.0));
    m.insert("kit.proc_field.spacing".to_string(), Value::from(2.0));
    m.insert("kit.proc_field.stagingx".to_string(), Value::from(2.0));
    assert_eq!(params_problem(&m), None, "the asset's own params pass");
}
