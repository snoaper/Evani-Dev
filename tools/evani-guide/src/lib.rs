//! evani-guide: the AI guidance that ships with the engine (docs/AI_GUIDANCE.md).
//!
//! One source, versioned with the engine, written into every project that
//! uses it:
//!
//! - [`render`]: the templates in `guide/` (the root AGENTS.md block, the
//!   short nested ones for games, kits and looks, and the pointer files).
//! - [`block`]: the engine-owned marked block (`<!-- BEGIN:evani-... -->`)
//!   that `init-ai` and `doctor --fix` rewrite without touching the rest of
//!   the file, stamped with the engine version and a hash of its body.
//! - [`targets`]: which files get a block (AGENTS.md, CLAUDE.md, GEMINI.md,
//!   `.cursor/rules/evani.mdc`, nested AGENTS.md) and the two JSON files
//!   (`.mcp.json` for the live MCP server, `.claude/settings.json` for the
//!   Stop hook that runs `check`).
//! - [`doctor`]: what is missing, stale or edited, and the fix.
//! - [`mcpbin`]: the prebuilt live MCP server in `.evani/bin/` (so an agent
//!   session never waits for a build when it starts).
//! - [`check`]: the one gate: every game's module list and catalog fit, the
//!   geocheck report, the review card, the look guard, and with `--full` the
//!   tests and the ship build, each issue with a fix.
//! - [`quick`]: `check --quick`, the fast loop: fmt (advisory), clippy and
//!   tests for only the crates changed since the last commit and their
//!   dependents, with capped load; the load caps `--full` shares.
//! - [`playground`]: a game marked for the Evani.app library must support
//!   free roam (Chris's rule); `check` and the ship build enforce it.
//! - [`modelcheck`]: what the engine's model and KTX2 readers would refuse or
//!   draw wrong (normals, LOD nodes, texture sets); `check` and evani-convert
//!   both use it.
//! - [`clipblock`]: the compact animation clip block evani-convert bakes and
//!   S3-8's decoder reads (reference reader and writer; the model check uses it).
//! - [`scaffold`]: `new-game`, which also writes the new game's guidance.
//! - [`genre`]: `new-game --genre/--effects` checked against the catalog, the
//!   kit's SPEC.md with the reference image prompt, and `catalog --json`.
//! - [`evals`]: the one-shot "make a game" eval plan and scorer (stub).
//! - [`cli`]: the command line shared by `cargo xtask` and this crate's
//!   binary (a future standalone `evani` CLI can call the same function).
//!
//! A native dev tool: nothing here reaches a game's download.

pub mod app_dir;
pub mod block;
pub mod check;
pub mod cli;
pub mod clipblock;
pub mod content;
pub mod doctor;
pub mod evals;
pub mod extools;
pub mod fonttools;
pub mod genre;
pub mod manifest;
pub mod mcpbin;
pub mod modelcheck;
pub mod playground;
pub mod quick;
pub mod render;
pub mod render_paths;
pub mod reports;
pub mod scaffold;
pub mod size_targets;
pub mod stream;
pub mod targets;
pub mod tier_fetch;

use std::path::{Path, PathBuf};

/// The engine version every generated block is stamped with.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The engine root: `start` or the nearest folder above it whose Cargo.toml
/// has a `[workspace]` and that has an `apps/` folder.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| is_root(d)).map(Path::to_path_buf)
}

fn is_root(d: &Path) -> bool {
    d.join("apps").is_dir()
        && std::fs::read_to_string(d.join("Cargo.toml")).is_ok_and(|t| t.lines().any(|l| l.trim() == "[workspace]"))
}

/// FNV-1a 64 of `bytes` as 16 lowercase hex chars: a change detector for
/// guidance bodies and sources, not a security hash.
pub fn fnv_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `path` relative to `root`, with `/` separators.
pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Write `contents` to `path`, creating its folder.
pub fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
pub(crate) mod testutil {
    use std::path::PathBuf;

    /// A fresh temporary folder for one test.
    pub fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("evani-guide-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A minimal engine root: workspace Cargo.toml, a catalog, the effects
    /// crate's features and one game `demo` (kit `demo`, look `basic`).
    pub fn fake_engine(name: &str) -> PathBuf {
        let r = temp_root(name);
        let w = |p: &str, s: &str| crate::write_file(&r.join(p), s).unwrap();
        w("Cargo.toml", "[workspace]\nmembers = [\n    \"apps/demo\",\n]\n");
        w(
            "catalog/looks.json",
            r#"{"looks":[{"id":"toon","style_class":"3d"},{"id":"pixel16","style_class":"2d"}]}"#,
        );
        w(
            "catalog/effects.json",
            r#"{"effects":[{"id":"bloom","works_with":["2d","3d"]},{"id":"outline","works_with":["3d"]},{"id":"ssao","works_with":["3d"]}]}"#,
        );
        w(
            "catalog/genres.json",
            r#"{"genres":[{"id":"kart-racer","physics":"simple","compatible_looks":["toon"]}]}"#,
        );
        w("crates/evani-fx/Cargo.toml", "[features]\ndefault = []\nbloom = []\noutline = []\n");
        w(
            "apps/demo/evani.toml",
            "name = \"demo\"\nkit = \"demo\"\nlook = \"basic\"\neffects = [\"bloom\"]\n\n[modules]\nphysics = \"simple\"\nlive = true\n",
        );
        w(
            "apps/demo/Cargo.toml",
            "[package]\nname = \"evani-demo\"\n\n[features]\ndefault = [\"bloom\", \"physics-simple\", \"live\"]\nfx = [\"dep:evani-fx\"]\nbloom = [\"fx\", \"evani-fx/bloom\"]\nphysics-simple = [\"dep:evani-physics\"]\nlive = [\"dep:evani-live\"]\n\n[dependencies]\nevani-kit-demo.workspace = true\nevani-look-basic.workspace = true\nevani-fx = { workspace = true, optional = true }\nevani-physics = { workspace = true, optional = true }\nevani-live = { workspace = true, optional = true }\n",
        );
        w("apps/demo/src/main.rs", "fn main() {}\n");
        w("kits/evani-kit-demo/src/lib.rs", "// kit\n");
        // The `basic` look is 3D: the kit turns on the renderer's 3D path.
        w(
            "kits/evani-kit-demo/Cargo.toml",
            "[package]\nname = \"evani-kit-demo\"\n\n[dependencies]\nevani-render = { workspace = true, features = [\"forward\"] }\n",
        );
        w("looks/evani-look-basic/src/lib.rs", "// look\n");
        r
    }
}
