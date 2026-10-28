//! S7-3: the measured on-screen importance is deterministic. Compiled only
//! with `--features importance` (evani-render's dev-only measuring), never in
//! a game's download: `cargo test -p evani-sandbox --features importance`.

use super::build_app;

/// The `max_screen_share` of model `id` in an importance file (keys are
/// sorted, so it follows the id within the same model entry).
fn max_share(json: &str, id: &str) -> f64 {
    let flat: String = json.chars().filter(|c| !c.is_whitespace()).collect();
    let at = flat
        .find(&format!("\"id\":\"{id}\""))
        .unwrap_or_else(|| panic!("{id} was never drawn: {json}"));
    let rest = &flat[at..];
    let key = "\"max_screen_share\":";
    let from = rest.find(key).unwrap_or_else(|| panic!("no max_screen_share for {id}")) + key.len();
    let num: String = rest[from..].chars().take_while(|c| *c != ',' && *c != '}').collect();
    num.parse().unwrap_or_else(|e| panic!("{num}: {e}"))
}

/// Two whole runs of the sandbox (fixed 60 Hz steps, its capture moments and
/// bench path, the models read from `assets/models`) write the same file,
/// byte for byte.
#[test]
fn importance_is_deterministic() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmp_root = root.join("../../.evani-tmp-s7-3");
    let tmp = tmp_root.join("importance-test");
    std::fs::create_dir_all(&tmp).unwrap();
    // Out of reach of git (the folder ignores everything).
    let _ = std::fs::write(tmp_root.join(".gitignore"), "*\n");
    // The settings go in as a resource of this one app (not the environment,
    // which every other test in the process would share and pick up).
    let run = |name: &str| {
        let out = tmp.join(name);
        let _ = std::fs::remove_file(&out);
        let mut app = build_app();
        app.context("sandbox").resource(evani_render::importance::Settings {
            out: out.clone(),
            assets: root.join("assets"),
            game: "sandbox".into(),
            viewport: None,
        });
        app.run_headless(300, 1000.0 / 60.0);
        std::fs::read_to_string(&out).unwrap_or_else(|e| panic!("{}: {e} (the run wrote no importance file)", out.display()))
    };
    let a = run("a.json");
    let b = run("b.json");
    assert_eq!(a, b, "two runs measured different importance");

    assert!(a.contains("\"version\": 1") || a.contains("\"version\":1"), "{a}");
    assert!(max_share(&a, "proof-figure") > 0.0);
    assert!(max_share(&a, "proof-kart") > 0.0);
}
