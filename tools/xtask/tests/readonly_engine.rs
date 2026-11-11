//! The main commands against a read-only engine (docs/ENGINE_PACK.md 2):
//! a pack-shaped copy of this engine's sources (`<pack>/manifest.json`,
//! `<pack>/engine/`) is locked against writing (Windows: a deny-write rule
//! for everyone; elsewhere: no write permission), then the real `evani`
//! binary runs the commands a user's game needs with Evani.App's
//! environment (`EVANI_ENGINE_ROOT`, `EVANI_STATE_DIR`, `EVANI_ENGINES_DIR`).
//! Afterwards every file in the engine copy is re-hashed: nothing may have
//! changed, appeared or gone. The commands that would write into the engine
//! (`init-ai`, `doctor --fix`, building its own example game) must refuse.
//!
//! No game is compiled here (seconds, not minutes); the full offline build
//! from a real pack is `cargo xtask engine-pack --verify` after the build in
//! docs/ENGINE_PACK.md section 5.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn engine_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let n = e.file_name();
        if ["dist", "target", "node_modules"].iter().any(|s| n == *s) {
            continue;
        }
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(&n));
        } else {
            std::fs::copy(e.path(), to.join(&n)).unwrap();
        }
    }
}

/// Every file under `dir` with its content (a change detector for the test).
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Locks `dir` against writing while it lives; unlocks on drop (also when
/// the test fails half-way).
struct ReadOnly(PathBuf);

impl ReadOnly {
    fn lock(dir: &Path) -> Self {
        #[cfg(windows)]
        {
            let ok = Command::new("icacls").arg(dir).args(["/deny", "*S-1-1-0:(OI)(CI)(WD,AD)", "/Q"]).status().unwrap().success();
            assert!(ok, "icacls could not lock {}", dir.display());
        }
        #[cfg(unix)]
        {
            assert!(Command::new("chmod").args(["-R", "a-w"]).arg(dir).status().unwrap().success());
        }
        Self(dir.to_path_buf())
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = Command::new("icacls").arg(&self.0).args(["/remove:d", "*S-1-1-0", "/T", "/Q"]).status();
        #[cfg(unix)]
        let _ = Command::new("chmod").args(["-R", "u+w"]).arg(&self.0).status();
    }
}

struct Run {
    code: i32,
    out: String,
}

#[test]
fn the_main_commands_never_write_into_a_read_only_engine() {
    let src = engine_src();
    let base = std::env::temp_dir().join(format!("evani-readonly-engine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let engines = base.join("engines");
    let pack = engines.join(".evani-build-evani-dev-engine-0.1.0");
    let engine = pack.join("engine");
    // What the commands read: workspace, pins, catalog, templates, looks,
    // engine crates' manifests, render paths, the kit's shared build script, docs.
    for f in ["Cargo.toml", "Cargo.lock", "AGENTS.md", "README.md", "packaging/engine-pack.json", "tools/bake-index.rs", "assets/render_paths.json"] {
        std::fs::create_dir_all(engine.join(f).parent().unwrap()).unwrap();
        std::fs::copy(src.join(f), engine.join(f)).unwrap();
    }
    for d in ["catalog", "looks", "apps/sandbox", "kits/evani-kit-example", "assets/tiers", "docs"] {
        copy_dir(&src.join(d), &engine.join(d));
    }
    for e in std::fs::read_dir(src.join("crates")).unwrap().flatten() {
        let m = e.path().join("Cargo.toml");
        if m.is_file() {
            std::fs::create_dir_all(engine.join("crates").join(e.file_name())).unwrap();
            std::fs::copy(&m, engine.join("crates").join(e.file_name()).join("Cargo.toml")).unwrap();
        }
    }
    let version = evani_version(&engine);
    std::fs::write(pack.join("manifest.json"), format!("{{\"schema\":\"evani-engine-pack/1\",\"pack_version\":\"{version}\"}}")).unwrap();
    let state = base.join("state");
    let game = base.join("games").join("ro-sea");
    let app = game.join("apps").join("ro-sea");
    let before = snapshot(&pack);
    let lock = ReadOnly::lock(&pack);
    // The lock really holds.
    assert!(std::fs::write(engine.join("probe.txt"), "x").is_err(), "the engine copy is not read-only");

    let run = |args: &[&str]| -> Run {
        let o = Command::new(env!("CARGO_BIN_EXE_evani-xtask"))
            .args(args)
            .current_dir(&base)
            .env("EVANI_ENGINE_ROOT", &engine)
            .env("EVANI_STATE_DIR", &state)
            .env("EVANI_ENGINES_DIR", &engines)
            .env_remove("EVANI_TOOLS_DIR")
            .output()
            .unwrap();
        Run { code: o.status.code().unwrap_or(-1), out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)) }
    };
    let app_s = app.display().to_string();
    let ok = |args: &[&str]| {
        let r = run(args);
        assert_eq!(r.code, 0, "`evani {}` failed:\n{}", args.join(" "), r.out);
        r.out
    };
    let refused = |args: &[&str], why: &str| {
        let r = run(args);
        assert!(r.code != 0 && r.out.contains(why), "`evani {}` should refuse ({why}):\n{}", args.join(" "), r.out);
    };

    // A new game in its own folder, pinned to this engine.
    let out = ok(&["new-game", "ro-sea", "--app-dir", &game.display().to_string(), "--look", "toon"]);
    assert!(out.contains(&format!("pinned to engine {version}")), "{out}");
    // The commands that read the game and the engine.
    assert!(ok(&["features", "--app-dir", &app_s]).contains("live"));
    ok(&["features", "--app-dir", &app_s, "--ship"]);
    ok(&["render-paths", "--app-dir", &app_s]);
    // No models yet: the stream manifest says so (and writes nothing).
    refused(&["manifest", "--app-dir", &app_s], "only games with models");
    let check = ok(&["check", "--app-dir", &app_s]);
    assert!(check.contains("evani check: clean"), "{check}");
    ok(&["relink", "--app-dir", &app_s]);
    let engines_out = ok(&["engines", "--games", &base.join("games").display().to_string(), "--json"]);
    assert!(engines_out.contains("\"schema\": \"evani-engines/1\"") && engines_out.contains("ro-sea"), "{engines_out}");
    ok(&["catalog"]);
    // A read-only look at the guidance is fine (it may report what a pack lacks).
    let _ = run(&["doctor"]);
    // As Cargo's subcommand (`cargo xtask ...` through the pack's cargo-xtask).
    ok(&["xtask", "features", "--app-dir", &app_s]);
    // What would write into the engine is refused, with the way out.
    refused(&["init-ai"], "read-only");
    refused(&["doctor", "--fix"], "read-only");
    refused(&["new-game", "inside"], "--app-dir");
    refused(&["build", "sandbox"], "new-game <id> --app-dir");
    refused(&["features", "sandbox"], "read-only");
    drop(lock);

    let after = snapshot(&pack);
    let changed: Vec<_> = after.iter().filter(|(p, b)| before.get(*p) != Some(*b)).map(|(p, _)| p.clone()).collect();
    let gone: Vec<_> = before.keys().filter(|p| !after.contains_key(*p)).cloned().collect();
    assert!(changed.is_empty() && gone.is_empty(), "the engine changed: new or changed {changed:?}, gone {gone:?}");
    // The game and the state folder got the writes instead.
    assert!(app.join("evani.toml").is_file() && game.join("Cargo.lock").is_file());
    let _ = std::fs::remove_dir_all(&base);
}

fn evani_version(engine: &Path) -> String {
    let t = std::fs::read_to_string(engine.join("packaging/engine-pack.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&t).unwrap();
    v["pack_version"].as_str().unwrap().to_string()
}
