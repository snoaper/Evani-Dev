//! Game passes (evani-render `game-passes`, docs/GAME_PASSES.md): a kit that
//! adds its own GPU passes must also carry a test that compiles them, so a
//! WGSL typo or a bad input is caught in `cargo test`, not as a black frame
//! in the browser. `check` reports each gap as an ERROR:
//!
//! * the kit's code uses `game_pass::` but no `#[test]` calls
//!   `game_pass::check(..)`;
//! * no dev-dependency turns on evani-render's `game-passes-check` (the
//!   feature that holds `check`).

use std::path::Path;

use crate::playground::{Problem, code_only, rust_files};

/// What a kit test calls to compile every game pass.
pub const TEST_HELPER: &str = "game_pass::check(";
/// The evani-render feature that holds it.
pub const CHECK_FEATURE: &str = "game-passes-check";

/// Every gap for the kit at `kit_path` (`kit_dir` names it in the fixes).
pub fn problems(kit_path: &Path, kit_dir: &str, kit_cargo: &str) -> Vec<Problem> {
    let mut files = Vec::new();
    rust_files(kit_path, &mut files);
    let code: Vec<String> = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).map(|t| code_only(&t)).collect();
    problems_from(&code, kit_dir, kit_cargo)
}

/// [`problems`] over the kit's code (comments removed) and Cargo.toml text.
pub fn problems_from(code: &[String], kit_dir: &str, kit_cargo: &str) -> Vec<Problem> {
    if !code.iter().any(|c| c.contains("game_pass::")) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if !code.iter().any(|c| c.contains("#[test]") && c.contains(TEST_HELPER)) {
        out.push(Problem {
            what: "the kit adds game passes, but no test compiles them".into(),
            fix: format!(
                "Add a `#[test]` in {kit_dir} that builds the app with the render module before the kit and asserts `evani_render::game_pass::check(&app.world)` is Ok (see kits/evani-kit-example/src/game_pass_demo.rs)."
            ),
        });
    }
    let dev = kit_cargo.split("[dev-dependencies]").nth(1).map(|s| s.split("\n[").next().unwrap_or(s)).unwrap_or("");
    if !dev.lines().any(|l| l.trim_start().starts_with("evani-render") && l.contains(CHECK_FEATURE)) {
        out.push(Problem {
            what: format!("the kit adds game passes, but its tests can't compile them (no `{CHECK_FEATURE}`)"),
            fix: format!(
                "In {kit_dir}/Cargo.toml add under [dev-dependencies]: evani-render = {{ workspace = true, features = [\"{CHECK_FEATURE}\"] }} (it never reaches the game's download)."
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_OK: &str = "[dependencies]\nevani-render = { workspace = true, features = [\"game-passes\"] }\n\n[dev-dependencies]\nevani-render = { workspace = true, features = [\"forward\", \"game-passes-check\"] }\n";

    #[test]
    fn a_kit_without_game_passes_has_nothing_to_fix() {
        assert!(problems_from(&["fn build() {}".into()], "kits/k", "").is_empty());
    }

    #[test]
    fn game_passes_need_a_compiling_test_and_the_check_feature() {
        let uses = "game_pass::add(kit, p)?;".to_string();
        let p = problems_from(std::slice::from_ref(&uses), "kits/k", "[dependencies]\nevani-render = { workspace = true, features = [\"game-passes-check\"] }\n");
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].fix.contains("game_pass::check(&app.world)") && p[1].fix.contains("[dev-dependencies]"), "{p:?}");
        let test = "#[test]\nfn passes() { assert!(evani_render::game_pass::check(&a.world).is_ok()); }".to_string();
        assert!(problems_from(&[uses, test], "kits/k", CARGO_OK).is_empty());
    }

    /// The repo's example kit keeps the rule.
    #[test]
    fn the_example_kit_keeps_the_rule() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../kits/evani-kit-example");
        let cargo = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
        assert!(problems(&root, "kits/evani-kit-example", &cargo).is_empty());
    }
}
