//! Playgrounds (docs/PLAYGROUNDS.md): a game built for the Evani.app library.
//!
//! Chris's rule (DECISIONS.md, 2026-10-15): every playground supports free
//! roam. A game is a playground when its `evani.toml` has a `[playground]`
//! table; it then needs `free_roam = true`, a kit whose `KitPlugin` says so
//! (`fn free_roam`) and a test that runs it in free roam through
//! `evani_core::play::free_roam_test`. `check` reports each gap as an ERROR,
//! and `cargo xtask build <id> --ship` refuses to build until they are closed.
//!
//! A playground is a small interactive example of what Evani.Dev and the
//! Evani.app library can make. It has no world-size requirement, but the whole
//! uploaded package (wasm, js, html, assets) stays within [`PACKAGE_CAP_BYTES`].

use std::path::Path;

/// 10 MB, uncompressed, for a playground's whole shipped folder; Evani.App enforces the same cap on upload.
pub const PACKAGE_CAP_BYTES: u64 = 10 * 1024 * 1024;

/// Total size on disk of every file under `dir` (0 when it is missing).
pub fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// `0.00 MB` style text for a byte count.
pub fn mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// The gap when a playground's shipped folder `dist` is over the cap.
pub fn size_problem(app: &str, dist: &Path) -> Option<Problem> {
    let total = dir_bytes(dist);
    (total > PACKAGE_CAP_BYTES).then(|| Problem {
        what: format!(
            "the playground package is {} on disk, over the {} cap (docs/PLAYGROUNDS.md)",
            mb(total),
            mb(PACKAGE_CAP_BYTES)
        ),
        fix: format!(
            "Shrink {app}: drop or downsize textures, sounds and models, turn off optional modules in apps/{app}/evani.toml, then run `cargo xtask build {app} --ship` again. `--diff` lists the biggest parts."
        ),
    })
}

/// The MIT licence text with `holder` on its copyright line. The engine's root
/// LICENSE is this text for "Chris Pearson"; a new game's LICENSE uses it for
/// the game's authors (docs/LICENSING.md).
pub fn mit_licence(holder: &str) -> String {
    format!(
        "MIT License\n\
\n\
Copyright (c) {holder}\n\
\n\
Permission is hereby granted, free of charge, to any person obtaining a copy\n\
of this software and associated documentation files (the \"Software\"), to deal\n\
in the Software without restriction, including without limitation the rights\n\
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell\n\
copies of the Software, and to permit persons to whom the Software is\n\
furnished to do so, subject to the following conditions:\n\
\n\
The above copyright notice and this permission notice shall be included in all\n\
copies or substantial portions of the Software.\n\
\n\
THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\n\
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\n\
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE\n\
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\n\
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,\n\
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE\n\
SOFTWARE.\n"
    )
}

/// The engine's copyright line (the root LICENSE).
pub const ENGINE_COPYRIGHT: &str = "Copyright (c) 2026 Chris Pearson";

/// The file every playground package carries: the Evani.Dev runtime inside it
/// is MIT (docs/LICENSING.md), and MIT asks for its copyright and permission
/// notice to travel with every copy, the compiled runtime included.
pub const ENGINE_NOTICE_FILE: &str = "ENGINE-LICENSE.txt";

/// The line a package README carries about licences.
pub const README_LICENCE_LINE: &str = "Game code: MIT. Includes the Evani.Dev runtime, also MIT (see ENGINE-LICENSE.txt).";

/// The text of [`ENGINE_NOTICE_FILE`]: one line saying what it covers, then
/// the engine's root LICENSE (MIT) verbatim.
pub fn engine_notice_text(licence_text: &str) -> String {
    format!(
        "This package includes the Evani.Dev runtime (the engine code compiled into the .wasm file and its JavaScript loader), under the MIT licence below. The game's own code and art keep the licence named in this package's README and LICENSE.\n\
\n{licence_text}"
    )
}

/// Writes [`ENGINE_NOTICE_FILE`] into `dist` from the repository's root LICENSE.
pub fn write_engine_notice(root: &Path, dist: &Path) -> Result<(), String> {
    let lic = std::fs::read_to_string(root.join("LICENSE")).map_err(|e| format!("LICENSE: {e} (the engine's root LICENSE is the source of the notice)"))?;
    std::fs::write(dist.join(ENGINE_NOTICE_FILE), engine_notice_text(&lic)).map_err(|e| format!("{ENGINE_NOTICE_FILE}: {e}"))
}

/// The gap when the package folder `dist` lacks a proper engine notice, or its
/// README (when it has one) lacks the licence line.
pub fn notice_problem(app: &str, dist: &Path) -> Option<Problem> {
    let fix = format!(
        "Run `cargo xtask build {app} --ship` (it writes {ENGINE_NOTICE_FILE} into the ship folder), or for a hand-made package copy that file in; docs/PUBLISHING.md."
    );
    let Ok(text) = std::fs::read_to_string(dist.join(ENGINE_NOTICE_FILE)) else {
        return Some(Problem {
            what: format!("the playground package has no {ENGINE_NOTICE_FILE} (the Evani.Dev runtime in it needs its MIT notice, docs/LICENSING.md)"),
            fix,
        });
    };
    for needle in ["Evani.Dev runtime", "MIT License", ENGINE_COPYRIGHT, "Permission is hereby granted, free of charge", "WITHOUT WARRANTY OF ANY KIND"] {
        if !text.contains(needle) {
            return Some(Problem { what: format!("{ENGINE_NOTICE_FILE} is incomplete or out of date: it lacks \"{needle}\""), fix });
        }
    }
    if let Ok(readme) = std::fs::read_to_string(dist.join("README.md"))
        && !readme.contains(README_LICENCE_LINE)
    {
        return Some(Problem {
            what: format!("README.md does not carry the licence line (it must say: {README_LICENCE_LINE})"),
            fix: format!("Change the README's licence line to: {README_LICENCE_LINE}"),
        });
    }
    None
}

/// The helper a free-roam test calls (evani-core `play.rs`).
pub const TEST_HELPER: &str = "free_roam_test(";
/// The `KitPlugin` method that declares free roam.
pub const KIT_METHOD: &str = "fn free_roam(";

/// `[playground]` in `evani.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Playground {
    /// `free_roam = true`.
    pub free_roam: bool,
}

/// `Ok(None)`: not a playground.
pub fn parse(evani_toml: &str) -> Result<Option<Playground>, String> {
    let t: toml::Table = evani_toml.parse().map_err(|e| format!("evani.toml: {e}"))?;
    let Some(v) = t.get("playground") else { return Ok(None) };
    let p = v.as_table().ok_or("evani.toml: `[playground]` must be a table")?;
    if let Some(k) = p.keys().find(|k| k.as_str() != "free_roam") {
        return Err(format!("evani.toml: unknown playground field `{k}` (free_roam)"));
    }
    let free_roam = match p.get("free_roam") {
        None => false,
        Some(v) => v.as_bool().ok_or("evani.toml: `playground.free_roam` must be true or false")?,
    };
    Ok(Some(Playground { free_roam }))
}

/// One gap, with what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub what: String,
    pub fix: String,
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if !matches!(e.file_name().to_string_lossy().as_ref(), "target" | "dist") {
                rust_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The code of a Rust file without `//` comments, so a mention in a doc
/// comment doesn't count as the real thing.
fn code_only(text: &str) -> String {
    text.lines().map(|l| l.find("//").map_or(l, |i| &l[..i])).collect::<Vec<_>>().join("\n")
}

/// True when a `fn free_roam` in `code` has `true` in its body (a text
/// check; the free-roam test catches a kit that says true but doesn't roam).
fn says_true(code: &str) -> bool {
    code.match_indices(KIT_METHOD).any(|(i, _)| {
        let rest = &code[i..];
        let Some(open) = rest.find('{') else { return false };
        let mut depth = 0usize;
        for (j, ch) in rest[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let body = &rest[open + 1..open + j];
                        return body
                            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                            .any(|w| w == "true");
                    }
                }
                _ => {}
            }
        }
        false
    })
}

/// Every gap between a playground and Chris's free-roam rule; empty when the
/// game is not a playground or keeps the rule. `kit` is the kit id.
pub fn problems(root: &Path, app: &str, kit: &str, evani_toml: &str) -> Vec<Problem> {
    let kit_dir = format!("kits/evani-kit-{kit}");
    problems_at(&root.join(&kit_dir), &kit_dir, &root.join("apps").join(app).join("src"), &format!("apps/{app}/evani.toml"), kit, evani_toml)
}

/// [`problems`] for a game outside the engine folder: its kit crate is at
/// `kit_path` and its app sources at `app_src`; `manifest` names its
/// `evani.toml` in the messages.
pub fn problems_in(kit_path: &Path, app_src: &Path, manifest: &str, kit: &str, evani_toml: &str) -> Vec<Problem> {
    problems_at(kit_path, &kit_path.display().to_string(), app_src, manifest, kit, evani_toml)
}

fn problems_at(kit_path: &Path, kit_dir: &str, app_src: &Path, manifest: &str, kit: &str, evani_toml: &str) -> Vec<Problem> {
    let pg = match parse(evani_toml) {
        Ok(Some(p)) => p,
        Ok(None) => return Vec::new(),
        Err(e) => return vec![Problem { what: e, fix: format!("Fix `[playground]` in {manifest}: it holds only `free_roam = true`.") }],
    };
    let how = format!(
        "In {kit_dir}, drop goals, timer, lives and fail state while `PlayMode::FreeRoam` (evani_core::play), return true from `KitPlugin::free_roam`, and add a test that calls `evani_core::play::free_roam_test`"
    );
    if !pg.free_roam {
        return vec![Problem {
            what: "marked as a playground, but doesn't declare free roam; every playground must support it (docs/PLAYGROUNDS.md)".into(),
            fix: format!("{how}; then set `free_roam = true` under `[playground]` in {manifest}. If it isn't for the Evani.app library yet, remove `[playground]`."),
        }];
    }
    let mut kit_files = Vec::new();
    rust_files(kit_path, &mut kit_files);
    let mut app_files = Vec::new();
    rust_files(app_src, &mut app_files);
    let code: Vec<String> = kit_files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).map(|t| code_only(&t)).collect();
    let mut out = Vec::new();
    if !code.iter().any(|c| c.contains(KIT_METHOD)) {
        out.push(Problem {
            what: format!("declares free roam, but kit `{kit}` doesn't: its `KitPlugin` has no `fn free_roam`"),
            fix: format!("{how}. Without it the engine plays the game normally when the site opens Free roam."),
        });
    } else if !code.iter().any(|c| says_true(c)) {
        out.push(Problem {
            what: format!("declares free roam, but kit `{kit}`'s `fn free_roam` doesn't return true"),
            fix: format!("{how}. While it returns false the engine plays the game normally when the site opens Free roam."),
        });
    }
    let app_code: Vec<String> = app_files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).map(|t| code_only(&t)).collect();
    if !code.iter().chain(&app_code).any(|c| c.contains("#[test]") && c.contains(TEST_HELPER)) {
        out.push(Problem {
            what: "declares free roam, but has no free-roam test".into(),
            fix: format!(
                "Add a `#[test]` in {kit_dir} that builds the game with `.with_play_mode(PlayMode::FreeRoam)` before `.with_kit(..)` and runs `evani_core::play::free_roam_test(&mut app, seconds, &script)` with scripted input (see kits/evani-kit-example)."
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIT: &str = "impl KitPlugin for K {\n    fn free_roam(&self) -> bool {\n        true\n    }\n}\n";
    const TEST: &str = "#[cfg(test)]\nmod t {\n    #[test]\n    fn roams() {\n        evani_core::play::free_roam_test(app(), 60.0, &[]).unwrap();\n    }\n}\n";

    fn fake_dist(name: &str, bytes: u64) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("evani-pg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("game.wasm"), vec![0u8; (bytes / 2) as usize]).unwrap();
        std::fs::write(d.join("assets/a.bin"), vec![0u8; (bytes - bytes / 2) as usize]).unwrap();
        d
    }

    #[test]
    fn package_cap_is_ten_mib_on_disk() {
        assert_eq!(PACKAGE_CAP_BYTES, 10_485_760);
        let over = fake_dist("over", PACKAGE_CAP_BYTES + 1);
        assert_eq!(dir_bytes(&over), PACKAGE_CAP_BYTES + 1);
        let p = size_problem("demo", &over).expect("over the cap fails");
        assert!(p.what.contains("over the 10.00 MB cap") && p.fix.contains("cargo xtask build demo --ship"), "{p:?}");
        let under = fake_dist("under", PACKAGE_CAP_BYTES);
        assert!(size_problem("demo", &under).is_none(), "exactly the cap passes");
        assert!(size_problem("demo", &under.join("missing")).is_none());
        let _ = std::fs::remove_dir_all(&over);
        let _ = std::fs::remove_dir_all(&under);
    }

    #[test]
    fn engine_notice_is_required() {
        let d = fake_dist("notice", 10);
        let p = notice_problem("demo", &d).expect("missing notice fails");
        assert!(p.what.contains(ENGINE_NOTICE_FILE) && p.fix.contains("cargo xtask build demo --ship"), "{p:?}");
        let lic = mit_licence("2026 Chris Pearson");
        assert!(lic.contains(ENGINE_COPYRIGHT), "{lic}");
        std::fs::write(d.join(ENGINE_NOTICE_FILE), engine_notice_text(&lic)).unwrap();
        assert!(notice_problem("demo", &d).is_none());
        // A notice that is not the MIT text (an older licence, or a stub) is refused.
        std::fs::write(d.join(ENGINE_NOTICE_FILE), "Evani.Dev runtime, an older licence only").unwrap();
        assert!(notice_problem("demo", &d).unwrap().what.contains("incomplete"));
        std::fs::write(d.join(ENGINE_NOTICE_FILE), engine_notice_text(&lic).replace(ENGINE_COPYRIGHT, "Copyright (c) someone")).unwrap();
        assert!(notice_problem("demo", &d).unwrap().what.contains("Chris Pearson"));
        std::fs::write(d.join(ENGINE_NOTICE_FILE), engine_notice_text(&lic)).unwrap();
        std::fs::write(d.join("README.md"), "MIT licence").unwrap();
        assert!(notice_problem("demo", &d).unwrap().what.contains("README.md"));
        // An older README line (any other wording) is refused too.
        std::fs::write(d.join("README.md"), "Game code: MIT. Includes the Evani.Dev runtime under an older licence (see ENGINE-LICENSE.txt).").unwrap();
        assert!(notice_problem("demo", &d).unwrap().what.contains("README.md"));
        std::fs::write(d.join("README.md"), README_LICENCE_LINE).unwrap();
        assert!(notice_problem("demo", &d).is_none());
        // The real root LICENSE is the MIT text and produces a notice that passes.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root_lic = std::fs::read_to_string(root.join("LICENSE")).unwrap().replace("\r\n", "\n");
        assert_eq!(root_lic, lic, "the root LICENSE is the verbatim MIT text");
        let w = fake_dist("written", 10);
        write_engine_notice(&root, &w).unwrap();
        assert!(notice_problem("demo", &w).is_none());
        let _ = std::fs::remove_dir_all(&w);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_the_table() {
        assert_eq!(parse("name = \"x\"\n").unwrap(), None);
        assert_eq!(parse("[playground]\n").unwrap(), Some(Playground { free_roam: false }));
        assert_eq!(parse("[playground]\nfree_roam = true\n").unwrap(), Some(Playground { free_roam: true }));
        assert!(parse("[playground]\nfree_roam = \"yes\"\n").is_err());
        assert!(parse("[playground]\nfree = true\n").unwrap_err().contains("unknown playground field"));
        assert!(parse("playground = 3\n").is_err());
    }

    #[test]
    fn each_gap_is_reported() {
        let r = crate::testutil::temp_root("playground");
        let w = |p: &str, s: &str| crate::write_file(&r.join(p), s).unwrap();
        w("kits/evani-kit-demo/src/lib.rs", "// fn free_roam( only in a comment\n// #[test] free_roam_test(\n");
        assert!(problems(&r, "demo", "demo", "name = \"demo\"\n").is_empty(), "not a playground");
        assert!(says_true("fn free_roam(&self) -> bool { true }") && says_true("fn free_roam(&self) -> bool {\n    if x { y } true\n}"));
        assert!(!says_true("fn free_roam(&self) -> bool { false } fn f() { true }") && !says_true("fn free_roam(&self) -> bool { untrue }"));
        let p = problems(&r, "demo", "demo", "[playground]\n");
        assert_eq!(p.len(), 1);
        assert!(p[0].what.contains("doesn't declare free roam") && p[0].fix.contains("free_roam = true"));
        let on = "[playground]\nfree_roam = true\n";
        let p = problems(&r, "demo", "demo", on);
        assert_eq!(p.len(), 2, "comments don't count: {p:?}");
        assert!(p[0].what.contains("no `fn free_roam`") && p[1].what.contains("no free-roam test"));
        // Declared but returning false (or a stub) is a gap too.
        w("kits/evani-kit-demo/src/lib.rs", &KIT.replace("true", "false"));
        w("kits/evani-kit-demo/tests/roam.rs", TEST);
        let p = problems(&r, "demo", "demo", on);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].what.contains("doesn't return true"), "{p:?}");
        w("kits/evani-kit-demo/src/lib.rs", "impl KitPlugin for K {\n    fn free_roam(&self) -> bool {\n        todo!()\n    }\n    fn other(&self) -> bool { true }\n}\n");
        assert!(problems(&r, "demo", "demo", on)[0].what.contains("doesn't return true"));
        std::fs::remove_file(r.join("kits/evani-kit-demo/tests/roam.rs")).unwrap();
        w("kits/evani-kit-demo/src/lib.rs", KIT);
        let p = problems(&r, "demo", "demo", on);
        assert_eq!(p.len(), 1);
        assert!(p[0].what.contains("no free-roam test"));
        // The test may live in the kit or in the game crate.
        w("apps/demo/src/main.rs", TEST);
        assert!(problems(&r, "demo", "demo", on).is_empty());
        std::fs::remove_file(r.join("apps/demo/src/main.rs")).unwrap();
        w("kits/evani-kit-demo/tests/roam.rs", TEST);
        assert!(problems(&r, "demo", "demo", on).is_empty());
    }

    #[test]
    fn a_game_outside_the_engine_is_checked_in_its_own_folders() {
        let r = crate::testutil::temp_root("playground-outside");
        let w = |p: &str, s: &str| crate::write_file(&r.join(p), s).unwrap();
        let (kit, src) = (r.join("game/kit"), r.join("game/app/src"));
        let on = "[playground]\nfree_roam = true\n";
        let p = problems_in(&kit, &src, "game/app/evani.toml", "demo", "[playground]\n");
        assert!(p[0].fix.contains("game/app/evani.toml") && !p[0].fix.contains("apps/"), "{p:?}");
        assert_eq!(problems_in(&kit, &src, "game/app/evani.toml", "demo", on).len(), 2);
        w("game/kit/src/lib.rs", KIT);
        w("game/app/src/main.rs", TEST);
        assert!(problems_in(&kit, &src, "game/app/evani.toml", "demo", on).is_empty());
        // The engine's own kits folder is not looked at.
        w("kits/evani-kit-demo/src/lib.rs", KIT);
        std::fs::remove_file(r.join("game/kit/src/lib.rs")).unwrap();
        assert!(problems_in(&kit, &src, "game/app/evani.toml", "demo", on)[0].what.contains("no `fn free_roam`"));
    }
}
