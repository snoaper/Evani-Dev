//! Content warnings for a game with `models` (S3-11, docs/CONTENT.md).
//! Both are WARN, never ERROR, and both sizes are parameters in evani.toml:
//!
//! * a **hero model** (listed in `[content] hero = ["<id>"]`, or any shared
//!   `<id>.glb` over `[content] hero_kb`, default 256) with no tier file
//!   (`<id>.<tier>.glb`): every tier, phones included, downloads the one
//!   shared file.
//! * a **boot group** (`[stream] boot = [...]`) over `[stream] boot_max_kb`
//!   (default 1024) in total: the title screen waits for all of it.

use std::path::Path;

use crate::stream::{boot_list, tier_of};

/// Default size of a boot group before it warns (KB).
pub const BOOT_MAX_KB: u64 = 1024;
/// Default size of a shared model before it counts as a hero (KB).
pub const HERO_KB: u64 = 256;

/// A warning with the way to fix it.
pub struct Warning {
    pub what: String,
    pub fix: String,
}

/// Every file under `dir`: `(path with / from dir, bytes)`, sorted.
fn files(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(md) = e.metadata() {
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push((rel, md.len()));
            }
        }
    }
    out.sort();
    out
}

fn number(t: &toml::Table, table: &str, key: &str, default: u64) -> u64 {
    t.get(table).and_then(|s| s.get(key)).and_then(toml::Value::as_integer).and_then(|n| u64::try_from(n).ok()).unwrap_or(default)
}

/// The warnings for `apps/<app>/assets/models`.
pub fn warnings(root: &Path, app: &str) -> Vec<Warning> {
    warnings_in(&root.join("apps").join(app), &format!("apps/{app}"), app)
}

/// [`warnings`] for the game in `game` (a game outside the engine too):
/// `shown` names its folder in messages, `arg` the game in commands.
pub fn warnings_in(game: &Path, shown: &str, arg: &str) -> Vec<Warning> {
    let all = files(&game.join("assets/models"));
    let text = std::fs::read_to_string(game.join("evani.toml")).unwrap_or_default();
    let toml: toml::Table = text.parse().unwrap_or_default();
    let mut out = Vec::new();

    let boot = boot_list(&text);
    let bytes: u64 = all.iter().filter(|f| boot.contains(&f.0)).map(|f| f.1).sum();
    let max = number(&toml, "stream", "boot_max_kb", BOOT_MAX_KB);
    if bytes > max * 1024 {
        out.push(Warning {
            what: format!("the boot group in {shown}/evani.toml is {} KB (limit {max} KB): the title screen waits for all of it before it shows", bytes.div_ceil(1024)),
            fix: format!("Move the big files out of `[stream] boot` so they download after the title (only what the title itself draws belongs there), or raise `boot_max_kb` under [stream] in {shown}/evani.toml if the wait is worth it."),
        });
    }

    let hero_kb = number(&toml, "content", "hero_kb", HERO_KB);
    let listed: Vec<String> = toml.get("content").and_then(|c| c.get("hero")).and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    for (rel, size) in &all {
        let Some(id) = rel.strip_suffix(".glb").filter(|_| tier_of(rel).is_none() && !rel.ends_with(".master.glb") && !rel.starts_with('.')) else { continue };
        let tiered = all.iter().any(|(f, _)| f.starts_with(&format!("{id}.")) && f.ends_with(".glb") && tier_of(f).is_some());
        if !tiered && (listed.iter().any(|l| l == id) || *size > hero_kb * 1024) {
            out.push(Warning {
                what: format!("the hero model `{id}` ({} KB) has no per-tier files, so every tier, phones included, downloads the one shared file", size.div_ceil(1024)),
                fix: format!("Convert it for the tiers (`cargo xtask convert <source>` writes `{id}.<tier>.glb`), then run `cargo xtask manifest {arg}`; or, if it is not a hero, raise `hero_kb` under [content] in {shown}/evani.toml."),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str, toml: &str, files: &[(&str, usize)]) -> std::path::PathBuf {
        let root = crate::testutil::temp_root(name);
        let dir = root.join("apps/demo/assets/models");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("apps/demo/evani.toml"), toml).unwrap();
        for (f, n) in files {
            std::fs::write(dir.join(f), vec![7u8; *n]).unwrap();
        }
        root
    }

    /// A boot group warns past its size, by default 1 MB, and the size is
    /// the game's parameter.
    #[test]
    fn boot_group_size_warning() {
        let files = [("logo.png", 2048), ("hero.glb", 600 * 1024), ("hero.low.glb", 10), ("big.bin", 700 * 1024)];
        let small = game("content-boot-small", "[stream]\nboot = [\"logo.png\", \"hero.glb\"]\n", &files);
        assert!(warnings(&small, "demo").is_empty(), "602 KB is under the 1024 KB default");
        let big = game("content-boot-big", "[stream]\nboot = [\"logo.png\", \"hero.glb\", \"big.bin\"]\n", &files);
        let w = warnings(&big, "demo");
        assert_eq!(w.len(), 1, "the boot group is 1302 KB");
        assert!(w[0].what.contains("1302 KB") && w[0].what.contains("limit 1024 KB") && w[0].fix.contains("boot_max_kb"), "{}", w[0].what);
        let raised = game("content-boot-raised", "[stream]\nboot = [\"logo.png\", \"hero.glb\", \"big.bin\"]\nboot_max_kb = 2000\n", &files);
        assert!(warnings(&raised, "demo").is_empty(), "the limit is a parameter");
        let tight = game("content-boot-tight", "[stream]\nboot = [\"logo.png\"]\nboot_max_kb = 1\n", &files);
        assert_eq!(warnings(&tight, "demo").len(), 1, "2 KB against 1 KB");
        let none = game("content-boot-none", "", &files);
        assert!(warnings(&none, "demo").is_empty(), "no boot list, no warning");
    }

    /// A big shared model, or a listed one, with no tier file warns; one
    /// with a tier file, a small one, a master and a converter scratch file
    /// do not.
    #[test]
    fn hero_without_tier_files_warns() {
        let root = game(
            "content-hero",
            "[content]\nhero = [\"small\"]\n",
            &[("big.glb", 300 * 1024), ("tiered.glb", 300 * 1024), ("tiered.low.glb", 100 * 1024), ("small.glb", 1024), ("tiny.glb", 1024), ("x.master.glb", 900 * 1024)],
        );
        let w = warnings(&root, "demo");
        let names: Vec<&str> = w.iter().map(|w| w.what.split('`').nth(1).unwrap_or("")).collect();
        assert_eq!(names, ["big", "small"], "{:?}", w.iter().map(|w| &w.what).collect::<Vec<_>>());
        assert!(w[0].what.contains("300 KB") && w[0].fix.contains("cargo xtask convert") && w[0].fix.contains("hero_kb"));
        let raised = game("content-hero-raised", "[content]\nhero_kb = 400\n", &[("big.glb", 300 * 1024)]);
        assert!(warnings(&raised, "demo").is_empty());
    }
}
