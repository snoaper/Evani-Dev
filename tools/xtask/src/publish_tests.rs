//! Tests of `cargo xtask publish` (publish.rs): packaging, the preflight, the
//! diff and the version history, on synthetic folders (no build).

use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("evani-publish-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const TRUNK_HTML: &str = "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>Test Game</title>\n<script type=\"module\">\nimport init, * as bindings from './evani-t-abc.js';\nconst wasm = await init({ module_or_path: './evani-t-abc_bg.wasm' });\n\n\nwindow.wasmBindings = bindings;\n\n\ndispatchEvent(new CustomEvent(\"TrunkApplicationStarted\", {detail: {wasm}}));\n\n</script>\n<link rel=\"modulepreload\" href=\"./evani-t-abc.js\" crossorigin=\"anonymous\"><link rel=\"preload\" href=\"./evani-t-abc_bg.wasm\" as=\"fetch\" type=\"application/wasm\"></head>\n<body><canvas id=\"evani-canvas\"></canvas></body>\n</html>\n";

const LOADER: &str = "/*! evani-loader v1 */\nvar EVANI_BIOME_TITLE = 'Old name';\nwindow.EvaniLoader = {};\n";

fn expect() -> Expect {
    Expect {
        app: "t".into(),
        kit: "k".into(),
        look: "l".into(),
        title: "Test Game".into(),
        free_roam: Some(true),
        playground_problems: Vec::new(),
        known_markers: vec!["evani.module.fx".into(), "evani.module.live".into()],
    }
}

/// A Trunk ship folder: page, js, wasm.
fn fake_dist(dir: &Path, wasm_extra: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("index.html"), TRUNK_HTML.replace('\n', "\r\n")).unwrap();
    std::fs::write(dir.join("evani-t-abc.js"), "export default function init() {}\n").unwrap();
    let mut wasm = b"\0asm\x01\0\0\0evani.module.fx".to_vec();
    wasm.extend_from_slice(wasm_extra);
    std::fs::write(dir.join("evani-t-abc_bg.wasm"), wasm).unwrap();
}

/// The previous package's hand-made files.
fn fake_carry(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("README.md"), format!("# Test\n\n{}\n", pg::README_LICENCE_LINE)).unwrap();
    std::fs::write(dir.join("LICENSE"), "MIT License\n\nCopyright (c) 2026 Chris Pearson\n").unwrap();
    std::fs::write(dir.join("preview.png"), png_header(1280, 720)).unwrap();
    std::fs::write(dir.join("evani.json"), "{\n  \"freeRoam\": true,\n  \"kit\": \"k\",\n  \"look\": \"l\"\n}\n").unwrap();
    std::fs::write(dir.join(LOADER_FILE), LOADER).unwrap();
}

/// A PNG signature and IHDR chunk with this size (enough for the preflight).
fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    b
}

#[test]
fn preview_must_be_big_enough_and_square_or_wide() {
    assert_eq!(png_size(&png_header(1280, 720)), Some((1280, 720)));
    assert!(preview_shape_ok(1280, 720) && preview_shape_ok(512, 512) && preview_shape_ok(1920, 1080));
    // As Evani.App: 512 px or more on each side (800x450 publishes unlisted).
    assert!(!preview_shape_ok(800, 450));
    assert!(!preview_shape_ok(1000, 700), "neither square nor 16:9");
    assert!(png_size(b"\x89PNG\r\n\x1a\nfake").is_none());
}

fn entry(sha: &str, size: u64) -> FileEntry {
    FileEntry { sha256: sha.into(), size, gzip: size / 2, brotli: size / 3 }
}

fn manifest(files: &[(&str, &str, u64)]) -> PackageManifest {
    PackageManifest { files: files.iter().map(|(p, s, n)| (p.to_string(), entry(s, *n))).collect() }
}

#[test]
fn diff_classifies_every_file() {
    let old = manifest(&[("index.html", "a", 100), ("game-1.js", "b", 1000), ("README.md", "c", 50), ("preview.png", "d", 10)]);
    let new = manifest(&[("index.html", "a2", 120), ("game-2.js", "e", 900), ("README.md", "c", 50), ("preview.png", "d", 10)]);
    let d = diff(&old, &new);
    let status = |p: &str| d.rows.iter().find(|r| r.path == p).map(|r| r.change);
    assert_eq!(status("index.html"), Some(Change::Modified));
    assert_eq!(status("game-2.js"), Some(Change::New));
    assert_eq!(status("game-1.js"), Some(Change::Deleted));
    assert_eq!(status("README.md"), Some(Change::Same));
    assert_eq!((d.count(Change::New), d.count(Change::Modified), d.count(Change::Same), d.count(Change::Deleted)), (1, 1, 2, 1));
    // 120 + 900 - 100 - 1000 raw.
    assert_eq!(d.size_change().0, -80);
    let page: Vec<String> = ["index.html", "game-2.js"].map(String::from).to_vec();
    assert_eq!(d.redownload_gzip(&page), 60 + 450);
    assert!(!d.unchanged());
    let lines = diff_lines(&d, Some("v4"), &page).join("\n");
    for label in ["NEW", "MODIFIED", "SAME", "DELETED", "whole-folder size change", "against: v4"] {
        assert!(lines.contains(label), "{label}: {lines}");
    }
}

#[test]
fn identical_package_says_skip() {
    let m = manifest(&[("index.html", "a", 100), ("game.wasm", "w", 5000)]);
    let d = diff(&m, &m);
    assert!(d.unchanged());
    assert!(diff_lines(&d, Some("v5"), &[]).join("\n").contains("nothing changed since v5, skip"));
    let dir = tmp("skip");
    let s = Summary {
        app: "t",
        title: "T",
        dry_run: true,
        created: "now",
        package_dir: &dir,
        manifest: &m,
        page: &["index.html".to_string(), "game.wasm".to_string()],
        checks: &[],
        modules: None,
        live: Some("v5"),
        diff: &d,
        suggested: Some("v6".into()),
        preview_url: None,
        swapped: false,
        tiers: &crate::tier_report::TierReport::default(),
    };
    let j = s.to_json();
    assert_eq!(j["skip"], true);
    assert_eq!(j["ready"], false);
    assert!(j["suggested_version"].is_null());
    assert_eq!(j["skip_reason"], "nothing changed since v5");
    // Without a live version nothing is skipped: every file is NEW.
    let first = diff(&PackageManifest::default(), &m);
    assert_eq!(first.count(Change::New), 2);
    assert!(diff_lines(&first, None, &[]).join("\n").contains("no previous manifest"));
}

#[test]
fn only_files_the_page_loads_count_as_fetched_again() {
    let old = manifest(&[("index.html", "a", 100), ("g_bg.wasm", "w", 5000), ("README.md", "r", 50), ("preview.png", "p", 9000)]);
    let new = manifest(&[("index.html", "a", 100), ("g_bg.wasm", "w", 5000), ("README.md", "r2", 60), ("preview.png", "p2", 9500)]);
    let d = diff(&old, &new);
    let page: Vec<String> = ["g_bg.wasm", "index.html"].map(String::from).to_vec();
    assert_eq!(d.count(Change::Modified), 2);
    assert_eq!(d.redownload_gzip(&page), 0, "README and preview never reach a browser");
    let line = diff_lines(&d, Some("v4"), &page).join("\n");
    assert!(line.contains(&format!("fetches {} gzip again (changed files the page loads)", kb(0))), "{line}");
    let s = Summary {
        app: "t",
        title: "T",
        dry_run: true,
        created: "now",
        package_dir: Path::new("pkg"),
        manifest: &new,
        page: &page,
        checks: &[],
        modules: None,
        live: Some("v4"),
        diff: &d,
        suggested: Some("v5".into()),
        preview_url: None,
        swapped: false,
        tiers: &crate::tier_report::TierReport::default(),
    };
    assert_eq!(s.to_json()["diff"]["redownload_gzip"], 0);
    // A first publish: everything is NEW, but only the page's files are fetched.
    let first = diff(&PackageManifest::default(), &new);
    assert_eq!(first.redownload_gzip(&page), 50 + 2500);
}

#[test]
fn package_html_puts_the_loader_first_and_the_smooth_start_in() {
    let out = package_html(&TRUNK_HTML.replace('\n', "\r\n")).unwrap();
    assert!(!out.contains('\r'), "line ends are LF");
    assert!(out.starts_with("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <script src=\"evani-loader.js\"></script>\n  <link rel=\"icon\" href=\"data:,\">\n  <meta"));
    assert!(loader_first(&out).is_ok());
    assert!(out.contains(SMOOTH_MARK));
    assert!(out.contains("const wasmUrl = './evani-t-abc_bg.wasm';"));
    assert!(out.contains("import init, * as bindings from './evani-t-abc.js';"));
    assert!(!out.contains("const wasm = await init({ module_or_path: './"));
    // As the hand-made packages: the bootstrap ends, then a blank line, then </script>.
    assert!(out.contains("  if (release) release();\n}\n\n</script>"));
    assert_eq!(out.matches("TrunkApplicationStarted").count(), 1);
    // Running it again changes nothing.
    assert_eq!(package_html(&out).unwrap(), out);
    // A page without Trunk's bootstrap is refused, not half-rewritten.
    assert!(package_html("<html><head></head><body></body></html>").is_err());
}

#[test]
fn package_html_moves_a_late_loader_tag_to_the_top_of_head() {
    // On its own line after Trunk's script, and once more inline before </head>.
    let late = TRUNK_HTML.replace("</script>\n<link", "</script>\n  <script src=\"evani-loader.js\"></script>\n<link");
    let late = late.replace("</head>", "<script src=\"evani-loader.js\"></script></head>");
    assert!(loader_first(&late).is_err());
    let out = package_html(&late).unwrap();
    assert!(loader_first(&out).is_ok(), "{out}");
    assert_eq!(out.matches(LOADER_TAG).count(), 1);
    assert!(out.starts_with("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <script src=\"evani-loader.js\"></script>\n"));
    assert!(out.contains("</script>\n<link rel=\"modulepreload\""), "the late tag's own line is gone: {out}");
    assert!(out.contains("as=\"fetch\" type=\"application/wasm\"></head>"));
    assert_eq!(package_html(&out).unwrap(), out);
}

#[test]
fn loader_must_be_the_first_script_in_head() {
    let ok = "<html><head>\n<script src=\"evani-loader.js\"></script>\n<script>1</script></head></html>";
    assert!(loader_first(ok).is_ok());
    let late = "<html><head>\n<script>1</script>\n<script src=\"evani-loader.js\"></script></head></html>";
    assert!(loader_first(late).unwrap_err().contains("before"));
    let body = "<html><head></head><body><script src=\"evani-loader.js\"></script></body></html>";
    assert!(loader_first(body).is_err());
    assert!(loader_first("<html><head></head></html>").is_err());
}

#[test]
fn loader_title_line_follows_the_game() {
    assert_eq!(loader_title(LOADER).as_deref(), Some("Old name"));
    let js = set_loader_title(LOADER, "Fennel's Leap").unwrap();
    assert!(js.contains("var EVANI_BIOME_TITLE = \"Fennel's Leap\";\n"), "{js}");
    assert_eq!(loader_title(&js).as_deref(), Some("Fennel's Leap"));
    let js = set_loader_title(LOADER, "Sugarpeak Rally").unwrap();
    assert!(js.contains("var EVANI_BIOME_TITLE = 'Sugarpeak Rally';\n"));
    // Only the title line changes; an already right one is left byte for byte.
    assert_eq!(js.replace("'Sugarpeak Rally'", "'Old name'"), LOADER);
    assert_eq!(set_loader_title(&js, "Sugarpeak Rally").unwrap(), js);
    assert!(set_loader_title("console.log(1)", "X").is_err());
    assert_eq!(page_title("<title>Puff&#39;s Sweet Grid</title>").as_deref(), Some("Puff's Sweet Grid"));
}

#[test]
fn local_refs_finds_page_files_only() {
    let html = package_html(TRUNK_HTML).unwrap()
        + "<a href=\"https://evani.app\">x</a><img src=\"data:,\"><a href=\"#top\"></a><style>b{background:url(img/bg.png)}</style>";
    let refs = local_refs(&html);
    assert_eq!(refs, ["evani-loader.js", "evani-t-abc.js", "evani-t-abc_bg.wasm", "img/bg.png"]);
}

#[test]
fn good_package_swaps_in_and_drops_stale_files() {
    let root = workspace_root();
    let t = tmp("good");
    let (dist, carry, dest) = (t.join("dist"), t.join("carry"), t.join("publish").join("t"));
    fake_dist(&dist, b"");
    fake_carry(&carry);
    // The old package: a stale hashed build from an earlier publish.
    fake_carry(&dest);
    std::fs::write(dest.join("evani-t-old_bg.wasm"), b"old").unwrap();
    std::fs::write(dest.join("index.html"), "old").unwrap();
    let p = package_into(&root, &dist, Some(&carry), &dest, None, &t.join("failed"), &expect()).unwrap();
    let failed: Vec<&Check> = p.checks.iter().filter(|c| !c.ok).collect();
    assert!(failed.is_empty(), "{failed:#?}");
    assert!(p.swapped);
    assert!(!dest.join("evani-t-old_bg.wasm").exists(), "stale hashed file removed");
    assert!(!staging_for(&dest).exists() && !t.join("publish").join(".t.old").exists());
    for f in ["index.html", "evani-t-abc.js", "evani-t-abc_bg.wasm", pg::ENGINE_NOTICE_FILE, "README.md", "LICENSE", "preview.png", "evani.json", LOADER_FILE] {
        assert!(dest.join(f).is_file(), "{f}");
    }
    let loader = std::fs::read_to_string(dest.join(LOADER_FILE)).unwrap();
    assert_eq!(loader_title(&loader).as_deref(), Some("Test Game"));
    assert!(p.notes.iter().any(|n| n.contains("title line")));
    assert_eq!(PackageManifest::scan(&dest).unwrap(), p.manifest);
    let notice = std::fs::read_to_string(dest.join(pg::ENGINE_NOTICE_FILE)).unwrap();
    assert!(notice.contains("MIT License") && notice.contains(pg::ENGINE_COPYRIGHT) && notice.contains("Permission is hereby granted"), "the package carries the engine's MIT notice");
}

#[test]
fn an_old_readme_licence_line_is_updated() {
    let root = workspace_root();
    let t = tmp("oldline");
    let (dist, carry, dest) = (t.join("dist"), t.join("carry"), t.join("publish").join("t"));
    fake_dist(&dist, b"");
    fake_carry(&carry);
    let older = "Game code: MIT. Includes the Evani.Dev runtime under an older engine licence (see ENGINE-LICENSE.txt).";
    std::fs::write(carry.join("README.md"), format!("# Test\n\n{older}\n\nMore text.\n")).unwrap();
    let p = package_into(&root, &dist, Some(&carry), &dest, None, &t.join("failed"), &expect()).unwrap();
    let failed: Vec<&Check> = p.checks.iter().filter(|c| !c.ok).collect();
    assert!(failed.is_empty(), "{failed:#?}");
    let readme = std::fs::read_to_string(dest.join("README.md")).unwrap();
    assert_eq!(readme, format!("# Test\n\n{}\n\nMore text.\n", pg::README_LICENCE_LINE));
    assert!(p.notes.iter().any(|n| n.contains("licence line updated")));
}

#[test]
fn the_licence_line_swap_keeps_other_lines_and_endings() {
    let md = "# T\r\n  Game code: MIT. old words\r\nGame code is fun\r\n";
    let fixed = with_current_licence_line(md);
    assert_eq!(fixed, format!("# T\r\n  {}\r\nGame code is fun\r\n", pg::README_LICENCE_LINE));
    assert_eq!(with_current_licence_line(&fixed), fixed, "the current line is left alone");
}

#[test]
fn failed_preflight_leaves_the_old_package_untouched() {
    let root = workspace_root();
    let t = tmp("atomic");
    let (dist, carry, dest) = (t.join("dist"), t.join("carry"), t.join("publish").join("t"));
    fake_dist(&dist, b"");
    fake_carry(&carry);
    let failed = t.join("hist").join("failed");
    package_into(&root, &dist, Some(&carry), &dest, None, &failed, &expect()).unwrap();
    let before = PackageManifest::scan(&dest).unwrap();
    assert!(!failed.exists(), "nothing rejected yet");

    // Over the 10 MB cap: refused, and the published folder is as it was.
    let big = t.join("dist-big");
    fake_dist(&big, b"");
    std::fs::write(big.join("music.ogg"), vec![7u8; (pg::PACKAGE_CAP_BYTES + 1) as usize]).unwrap();
    let p = package_into(&root, &big, Some(&carry), &dest, None, &failed, &expect()).unwrap();
    let size = p.checks.iter().find(|c| c.id == "size").unwrap();
    assert!(!size.ok && size.detail.contains("over the 10.00 MB cap") && size.detail.contains("raw"), "{size:?}");
    assert!(!p.swapped);
    assert_eq!(PackageManifest::scan(&dest).unwrap(), before);
    assert!(!staging_for(&dest).exists(), "staging cleared");
    // The rejected package is kept to look at, and the result points at it.
    assert_eq!(p.dir, failed);
    assert_eq!(PackageManifest::scan(&failed).unwrap(), p.manifest);

    // Dev-only code: refused the same way (the earlier rejected package is replaced).
    let dev = t.join("dist-dev");
    fake_dist(&dev, b"evani.module.live hotpatch");
    let p = package_into(&root, &dev, Some(&carry), &dest, None, &failed, &expect()).unwrap();
    assert!(!p.checks.iter().find(|c| c.id == "dev_only").unwrap().ok);
    assert!(!p.swapped);
    assert_eq!(PackageManifest::scan(&dest).unwrap(), before);
    assert!(!failed.join("music.ogg").exists());

    // A dry run never touches the destination either.
    let dry = t.join("dry");
    let p = package_into(&root, &big, Some(&carry), &dest, Some(&dry), &failed, &expect()).unwrap();
    assert_eq!(p.dir, dry);
    assert!(dry.join("music.ogg").is_file());
    assert_eq!(PackageManifest::scan(&dest).unwrap(), before);
}

#[test]
fn preflight_reports_each_missing_piece() {
    let root = workspace_root();
    let t = tmp("pieces");
    let (dist, dir) = (t.join("dist"), t.join("pkg"));
    fake_dist(&dist, b"");
    // No previous package: only evani.json can be made from evani.toml.
    assemble(&root, &dist, None, &dir, &expect()).unwrap();
    let checks = preflight(&dir, &expect());
    let ok = |id: &str| checks.iter().find(|c| c.id == id).map(|c| c.ok);
    assert_eq!(ok("preview"), Some(false));
    assert_eq!(ok("loader"), Some(false));
    assert_eq!(ok("licences"), Some(false));
    assert_eq!(ok("evani_json"), Some(true));
    assert_eq!(ok("size"), Some(true));
    assert_eq!(ok("smooth_start"), Some(true));
    assert!(checks.iter().filter(|c| !c.ok).all(|c| !c.fix.is_empty()), "every failure says how to fix it");

    // A stale wasm and a reference to a missing file.
    fake_carry(&dir);
    std::fs::write(dir.join(LOADER_FILE), set_loader_title(LOADER, "Test Game").unwrap()).unwrap();
    std::fs::write(dir.join("evani-t-old_bg.wasm"), b"\0asm").unwrap();
    let html = std::fs::read_to_string(dir.join("index.html")).unwrap().replace("<body>", "<body><img src=\"gone.png\">");
    std::fs::write(dir.join("index.html"), html).unwrap();
    let checks = preflight(&dir, &expect());
    let get = |id: &str| checks.iter().find(|c| c.id == id).unwrap().clone();
    assert!(!get("engine_files").ok && get("engine_files").detail.contains("expected one .wasm"));
    assert!(!get("references").ok && get("references").detail.contains("gone.png"));
    std::fs::remove_file(dir.join("evani-t-old_bg.wasm")).unwrap();
    std::fs::write(dir.join("evani-t-stale.js"), "x").unwrap();
    let checks = preflight(&dir, &expect());
    let stale = checks.iter().find(|c| c.id == "engine_files").unwrap();
    assert!(!stale.ok && stale.detail.contains("evani-t-stale.js"), "{stale:?}");

    // evani.json and free roam must match evani.toml.
    std::fs::write(dir.join("evani.json"), "{\"kit\": \"k\", \"look\": \"other\", \"freeRoam\": true}").unwrap();
    let mut e = expect();
    e.free_roam = None;
    let checks = preflight(&dir, &e);
    assert!(!checks.iter().find(|c| c.id == "evani_json").unwrap().ok);
    assert!(!checks.iter().find(|c| c.id == "free_roam").unwrap().ok);
}

#[test]
fn size_report_labels_every_figure() {
    let m = manifest(&[("index.html", "a", 7000), ("g_bg.wasm", "b", 800_000), ("evani-loader.js", "c", 13_000)]);
    let rows = [ModuleRow { module: "fx.bloom".into(), raw: 9000, gzip: 3000, brotli: 2500 }];
    let page = page_files(&m, "<script src=\"evani-loader.js\"></script><script>const wasmUrl = './g_bg.wasm';</script><img src=\"gone.png\">");
    assert_eq!(page, ["evani-loader.js", "g_bg.wasm", "index.html"], "only files in the package");
    let text = size_report_lines(&m, &page, Some(&rows)).join("\n");
    assert!(text.contains("whole folder (what is uploaded"));
    assert!(text.contains("download (what a player's browser fetches: index.html and the 2 file(s) it loads)"));
    // README, licences and the preview are uploaded but never fetched by the page.
    let with_extras = manifest(&[("index.html", "a", 7000), ("g_bg.wasm", "b", 800_000), ("preview.png", "p", 90_000)]);
    let page2 = page_files(&with_extras, "<script>const wasmUrl = './g_bg.wasm';</script>");
    assert_eq!(download_totals(&with_extras, &page2).0, 807_000);
    assert_eq!(with_extras.totals().0, 897_000);
    assert!(text.contains("raw") && text.contains("gzip") && text.contains("brotli"));
    assert!(text.contains("by module") && text.find("by module") < text.find("by file"), "modules first");
    assert!(text.contains("10 MB cap (raw, whole folder, uncompressed"));
    assert!(text.contains("1.5 MB size line (raw"));
    assert!(text.contains("+8.8 KB"));
    // The 1.5 MB line counts the build's files, not the loader.
    assert_eq!(size_line_raw(&m), 807_000);
    assert!(size_report_lines(&m, &page, None).join("\n").contains("skipped (--quick)"));
}

#[test]
fn history_records_versions_and_never_rewrites_one() {
    let t = tmp("history");
    let root = t.join("repo");
    let hist = history_dir(&root, "t");
    let pkg = t.join("pkg");
    fake_dist(&pkg, b"");
    assert!(live_version(&hist).is_none());
    assert!(next_version(&hist).is_none());
    assert_eq!(mark_published(&hist, "t", "v4", &pkg, "2026-10-16 18:00 UTC").unwrap(), "v4");
    let (v, m) = live_version(&hist).unwrap();
    assert_eq!(v, "v4");
    assert_eq!(m, PackageManifest::scan(&pkg).unwrap());
    assert_eq!(next_version(&hist).as_deref(), Some("v5"));
    // The same package again is fine; another package under v4 is refused.
    assert!(mark_published(&hist, "t", "4", &pkg, "later").is_ok());
    std::fs::write(pkg.join("evani-t-abc.js"), "changed").unwrap();
    assert!(mark_published(&hist, "t", "v4", &pkg, "later").unwrap_err().contains("never rewritten"));
    assert_eq!(mark_published(&hist, "t", "v5", &pkg, "later").unwrap(), "v5");
    assert_eq!(next_version(&hist).as_deref(), Some("v6"));
    assert!(mark_published(&hist, "t", "five", &pkg, "x").is_err());
    assert!(root.join(".evani").join("publish").join(".gitignore").is_file());
}

#[test]
fn summary_lands_outside_the_package() {
    let t = tmp("summary");
    let root = t.join("repo");
    let pkg = root.join("publish").join("t");
    fake_dist(&pkg, b"");
    let hist = history_dir(&root, "t");
    let m = PackageManifest::scan(&pkg).unwrap();
    let d = diff(&PackageManifest::default(), &m);
    let checks = preflight(&pkg, &expect());
    let s = Summary {
        app: "t",
        title: "Test Game",
        dry_run: false,
        created: "2026-10-16 18:00 UTC",
        package_dir: &pkg,
        manifest: &m,
        page: &page_files(&m, &std::fs::read_to_string(pkg.join("index.html")).unwrap()),
        checks: &checks,
        modules: None,
        live: None,
        diff: &d,
        suggested: None,
        preview_url: Some("http://localhost:8114/t/".into()),
        swapped: true,
        tiers: &crate::tier_report::TierReport::default(),
    };
    let path = write_summary(&hist, &s).unwrap();
    assert!(!path.starts_with(&pkg) && path.is_file());
    assert!(hist.join("summary.md").is_file() && hist.join("package.json").is_file());
    assert_eq!(PackageManifest::scan(&pkg).unwrap(), m, "nothing written into the package");
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(j["schema"], SUMMARY_SCHEMA);
    assert_eq!(j["sizes"]["cap_basis"], "raw, whole folder, uncompressed");
    assert_eq!(j["sizes"]["tier_targets"]["tiers"], serde_json::json!([]));
    assert_eq!(j["diff"]["new"], m.files.len());
    assert_eq!(j["preview_url"], "http://localhost:8114/t/");
    assert_eq!(j["preflight"]["passed"], false);
    assert!(std::fs::read_to_string(hist.join("summary.md")).unwrap().contains("FAIL preview"));
}

#[test]
fn preview_server_serves_the_package_in_a_subfolder() {
    let pkg = PathBuf::from("pkg");
    assert_eq!(resolve(&pkg, "t", "/t/"), Some(pkg.join("index.html")));
    assert_eq!(resolve(&pkg, "t", "/t/a.js?x=1"), Some(pkg.join("a.js")));
    assert_eq!(resolve(&pkg, "t", "/a.js"), None, "root paths 404, as on the real host");
    assert_eq!(resolve(&pkg, "t", "/t/../secret"), None);
    for bad in ["/t//Windows/win.ini", "/t/\\\\x\\y", "/t/%2e%2e/x", "/t/C:/x", "/t/a//b"] {
        assert_eq!(resolve(&pkg, "t", bad), None, "{bad}");
    }
    assert_eq!(resolve(&pkg, "t", "/t/index.html"), Some(pkg.join("index.html")));
}

#[test]
fn never_replaces_a_folder_that_is_not_a_package() {
    let root = workspace_root();
    let t = tmp("notpkg");
    let (dist, carry, dest) = (t.join("dist"), t.join("carry"), t.join("Desktop"));
    fake_dist(&dist, b"");
    fake_carry(&carry);
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("notes.txt"), "mine").unwrap();
    assert!(package_into(&root, &dist, Some(&carry), &dest, None, &t.join("failed"), &expect()).unwrap_err().contains("not a playground package"));
    assert_eq!(std::fs::read_to_string(dest.join("notes.txt")).unwrap(), "mine");
    assert!(!staging_for(&dest).exists());
}

#[test]
fn an_interrupted_swap_puts_the_old_package_back() {
    let t = tmp("interrupted");
    let dest = t.join("pkg");
    let old = t.join(".pkg.old");
    fake_dist(&old, b"");
    std::fs::write(old.join(LOADER_FILE), LOADER).unwrap();
    let before = PackageManifest::scan(&old).unwrap();
    // The new package can't move in (no staging folder): the old one must survive.
    let err = swap_into(&t.join("missing-staging"), &dest).unwrap_err();
    assert!(err.contains("old one is back"), "{err}");
    assert_eq!(PackageManifest::scan(&dest).unwrap(), before);
    assert!(!old.exists());
}

#[test]
fn options_parse() {
    let a = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
    let o = parse_opts(&a("kart-racer --dry-run --out C:/x --port 8200 --quick")).unwrap();
    assert_eq!((o.app.as_str(), o.dry_run, o.quick, o.port, o.serve), ("kart-racer", true, true, 8200, false));
    assert_eq!(o.out, Some(PathBuf::from("C:/x")));
    let o = parse_opts(&a("match-3 --mark-published v5")).unwrap();
    assert_eq!(o.mark.as_deref(), Some("v5"));
    assert_eq!(o.port, DEFAULT_PORT);
    assert!(parse_opts(&a("x --bogus")).is_err());
    assert!(parse_opts(&a("--dry-run")).is_err());
}

#[test]
fn model_view_is_never_published() {
    for name in ["model-view", "apps/model-view", "apps/model-view/"] {
        let why = dev_only_refusal(name).unwrap();
        assert!(why.contains("never published") && why.contains("\n  fix: "), "{why}");
        // Refused before anything is built, including --dry-run and --mark.
        let args: Vec<String> = [name, "--dry-run"].iter().map(|s| s.to_string()).collect();
        assert!(cmd_publish(&args).unwrap_err().contains("fix:"));
    }
    for game in ["kart-racer", "match-3", "2d-platformer", "sandbox"] {
        assert_eq!(dev_only_refusal(game), None, "{game}");
    }
}
