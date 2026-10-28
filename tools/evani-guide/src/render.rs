//! The guidance texts, from the templates in `guide/` (compiled in, so they
//! always match this engine version) and the short pointer texts.
//!
//! Template rules: `{{app}}` is replaced by the game id; a line that starts
//! with `?<path> ` is kept (without that prefix) only when `<path>` exists in
//! the project, so the block never names a doc this checkout doesn't have.

use std::path::Path;

/// The root AGENTS.md block.
pub const ROOT_ID: &str = "evani-agent-rules";
/// The block in a file that only points to AGENTS.md (CLAUDE.md, GEMINI.md,
/// Cursor and Copilot rules).
pub const POINTER_ID: &str = "evani-pointer";
/// The block in a nested AGENTS.md (a game, `kits/`, `looks/`).
pub const FOLDER_ID: &str = "evani-folder-rules";

const ROOT: &str = include_str!("../guide/AGENTS.md");
const APP: &str = include_str!("../guide/app.md");
const KITS: &str = include_str!("../guide/kits.md");
const LOOKS: &str = include_str!("../guide/looks.md");

/// Apply the template rules to `template`.
pub fn fill(template: &str, root: &Path, vars: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for line in template.lines() {
        let line = match line.strip_prefix('?') {
            Some(rest) => {
                let (path, text) = rest.split_once(' ').unwrap_or((rest, ""));
                if !root.join(path).exists() {
                    continue;
                }
                text
            }
            None => line,
        };
        let mut l = line.to_string();
        for (k, v) in vars {
            l = l.replace(&format!("{{{{{k}}}}}"), v);
        }
        out.push_str(&l);
        out.push('\n');
    }
    out
}

/// The body of the root AGENTS.md block.
pub fn root_body(root: &Path) -> String {
    fill(ROOT, root, &[])
}

/// The always-loaded root block must stay under this many bytes (long or
/// generic context files make agents worse).
pub const ROOT_LIMIT: usize = 8 * 1024;

/// The size of the root block as rendered for `root`, when it is at or over
/// [`ROOT_LIMIT`]. One measurement for the unit test and `cargo xtask check`.
pub fn root_over_limit(root: &Path) -> Option<usize> {
    root_over(root, ROOT_LIMIT)
}

/// [`root_over_limit`] against any `limit` (tests lower it).
pub fn root_over(root: &Path, limit: usize) -> Option<usize> {
    let n = root_body(root).len();
    (n >= limit).then_some(n)
}

/// The body of `apps/<app>/AGENTS.md`.
pub fn app_body(root: &Path, app: &str) -> String {
    fill(APP, root, &[("app", app)])
}

/// The body of `kits/AGENTS.md`.
pub fn kits_body(root: &Path) -> String {
    fill(KITS, root, &[])
}

/// The body of `looks/AGENTS.md`.
pub fn looks_body(root: &Path) -> String {
    fill(LOOKS, root, &[])
}

/// CLAUDE.md: Claude Code reads CLAUDE.md, not AGENTS.md, so it imports it.
pub fn claude_pointer() -> String {
    "@AGENTS.md\n\nThe Evani.Dev rules for AI agents are in AGENTS.md (imported above); they ship with the engine. Project notes go below this block.\n".into()
}

/// GEMINI.md: Gemini CLI imports with `@./file`.
pub fn gemini_pointer() -> String {
    "@./AGENTS.md\n\nThe Evani.Dev rules for AI agents are in AGENTS.md (imported above); they ship with the engine. Project notes go below this block.\n".into()
}

/// The front matter of `.cursor/rules/evani.mdc` (written once; the block
/// goes after it).
pub const CURSOR_FRONT: &str = "---\ndescription: Evani.Dev engine rules for AI agents\nalwaysApply: true\n---\n";

/// The rule body for Cursor and Copilot, which don't import files.
pub fn read_agents_pointer() -> String {
    "Before changing anything, read and follow AGENTS.md at the repository root (and the AGENTS.md in the folder you work in): the Evani.Dev engine rules and tools, versioned with the engine. A task is done only when `cargo xtask check` prints `evani check: clean`.\n".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_lines_follow_the_checkout() {
        let r = crate::testutil::temp_root("render-opt");
        let t = "a\n?docs/X.md b {{app}}\nc {{app}}\n";
        assert_eq!(fill(t, &r, &[("app", "kart")]), "a\nc kart\n");
        crate::write_file(&r.join("docs/X.md"), "x").unwrap();
        assert_eq!(fill(t, &r, &[("app", "kart")]), "a\nb kart\nc kart\n");
    }

    #[test]
    fn the_root_block_stays_lean() {
        let r = crate::testutil::temp_root("render-lean");
        let body = root_body(&r);
        // The research behind this: long or generic context files make
        // agents worse. Keep the always-loaded block well under 8 KB.
        assert!(body.len() < ROOT_LIMIT, "{} bytes", body.len());
        assert_eq!(root_over_limit(&r), None);
        assert!(body.contains("cargo xtask check"));
        assert!(!body.contains("{{"));
        assert!(!body.lines().any(|l| l.starts_with('?')));
        assert!(app_body(&r, "kart").contains("apps/kart/evani.toml"));
    }
}
