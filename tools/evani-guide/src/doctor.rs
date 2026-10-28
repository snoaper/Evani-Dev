//! `doctor`: is the guidance in this project the one this engine version
//! writes, and is the live MCP server installed? `--fix` makes it so.

use std::path::Path;

use crate::mcpbin;
use crate::targets::{self, Options, TState};

#[derive(Debug, Clone)]
pub struct Report {
    /// Every guidance file with its state.
    pub files: Vec<(String, TState)>,
    pub mcp: mcpbin::State,
}

impl Report {
    /// Files that need `--fix`.
    pub fn problems(&self) -> Vec<&(String, TState)> {
        self.files.iter().filter(|(_, s)| *s != TState::Current).collect()
    }

    pub fn is_clean(&self) -> bool {
        self.problems().is_empty() && self.mcp == mcpbin::State::Current
    }

    /// One line per problem, for people and agents.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .problems()
            .iter()
            .map(|(rel, s)| match s {
                TState::Missing => format!("{rel}: Evani guidance missing"),
                TState::Stale(why) => format!("{rel}: Evani guidance out of date ({why})"),
                TState::Edited => format!("{rel}: the Evani block was edited by hand (`--fix` puts it back; keep your notes outside the markers)"),
                TState::Broken(why) => format!("{rel}: can't be updated safely: {why}; fix it by hand, then run `cargo xtask doctor --fix`"),
                TState::Current => String::new(),
            })
            .collect();
        match self.mcp {
            mcpbin::State::Missing => out.push(format!("{}: live MCP server not installed, so `evani-live` can't connect", mcpbin::BIN_DIR)),
            mcpbin::State::Stale => out.push(format!("{}: live MCP server built from older sources", mcpbin::BIN_DIR)),
            mcpbin::State::Current => {}
        }
        out
    }
}

pub fn doctor(root: &Path, opts: Options) -> Report {
    let files = targets::targets(root, opts).iter().map(|t| (t.rel.clone(), t.state(root))).collect();
    Report { files, mcp: mcpbin::state(root) }
}

/// What `fix` did.
#[derive(Debug, Default)]
pub struct Fixed {
    pub written: Vec<String>,
    pub mcp: Option<String>,
    pub errors: Vec<String>,
}

/// Rewrite every guidance file that isn't current (broken ones are left
/// alone and reported) and, with `build_mcp`, install the live MCP server
/// when it is missing or stale.
pub fn fix(root: &Path, opts: Options, build_mcp: bool) -> Fixed {
    let mut f = Fixed::default();
    for t in targets::targets(root, opts) {
        match t.state(root) {
            TState::Current => {}
            TState::Broken(why) => f.errors.push(format!("{}: {why}", t.rel)),
            _ => match t.write(root) {
                Ok(true) => f.written.push(t.rel.clone()),
                Ok(false) => {}
                Err(e) => f.errors.push(e),
            },
        }
    }
    if build_mcp && mcpbin::state(root) != mcpbin::State::Current {
        match mcpbin::install(root) {
            Ok(p) => f.mcp = Some(crate::rel(root, &p)),
            Err(e) => f.errors.push(format!("live MCP server: {e}")),
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_fixes_missing_stale_and_edited_files() {
        let r = crate::testutil::fake_engine("doctor");
        let d = doctor(&r, Options::default());
        assert!(!d.is_clean());
        assert!(d.problems().iter().all(|(_, s)| *s == TState::Missing));
        let f = fix(&r, Options::default(), false);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        assert!(f.written.contains(&"AGENTS.md".to_string()));
        assert!(doctor(&r, Options::default()).problems().is_empty());

        // An older engine's block, and a hand edit inside a block.
        let agents = std::fs::read_to_string(r.join("AGENTS.md")).unwrap();
        let old = crate::block::upsert(Some(&agents), crate::render::ROOT_ID, "0.0.1", &crate::render::root_body(&r), crate::block::Place::Top).unwrap();
        std::fs::write(r.join("AGENTS.md"), format!("{old}\nMy own note.\n")).unwrap();
        let gem = std::fs::read_to_string(r.join("GEMINI.md")).unwrap();
        std::fs::write(r.join("GEMINI.md"), gem.replace("@./AGENTS.md", "@./AGENTS.md and more")).unwrap();
        let d = doctor(&r, Options::default());
        let state = |rel: &str| d.files.iter().find(|(x, _)| x == rel).unwrap().1.clone();
        assert!(matches!(state("AGENTS.md"), TState::Stale(_)));
        assert_eq!(state("GEMINI.md"), TState::Edited);
        assert!(d.lines().iter().any(|l| l.starts_with("AGENTS.md: Evani guidance out of date")));

        let f = fix(&r, Options::default(), false);
        assert_eq!(f.written.len(), 2, "{:?}", f.written);
        assert!(doctor(&r, Options::default()).problems().is_empty());
        let agents = std::fs::read_to_string(r.join("AGENTS.md")).unwrap();
        assert!(agents.ends_with("\nMy own note.\n"), "the project's text survives");
        assert!(agents.contains(&format!("engine={}", crate::ENGINE_VERSION)));
    }
}
