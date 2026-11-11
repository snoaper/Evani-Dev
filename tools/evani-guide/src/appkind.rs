//! The app kind: `kind = "game"` (the default) or `kind = "tool"` at the top
//! of `apps/<id>/evani.toml`.
//!
//! A tool (the material viewer) is an app built on the engine that has no
//! kit gameplay: nothing to win or lose and no game objects, so the review's
//! "game objects live in the ECS" entity count does not apply to it. That is
//! the ONLY check a tool skips (DECISIONS.md, "Tool apps"); every other
//! check, the kit params included, holds for tools as for games.
//!
//! So a game can't skip the entity check by writing `kind = "tool"`, a tool
//! must show it has no gameplay, all of:
//!
//! 1. `[modules] physics = "none"` and `net = false`: nothing simulated,
//!    nothing shared between players;
//! 2. its kit is not a genre in `catalog/genres.json` (a genre kit is a game);
//! 3. its kit's row is in ROADMAP.md's [`ROADMAP_SECTION`] table, where a
//!    reviewer sees it listed as a tool and not as a game;
//! 4. its kit crate does not depend on a gameplay crate ([`GAMEPLAY_CRATES`]).
//!
//! `cargo xtask check` reports every broken rule as an error, and the review
//! card's architecture check skips the entity count only when [`problems`] is
//! empty.

use std::path::Path;

pub use crate::playground::Problem;

/// The ROADMAP.md heading (`## ...`, prefix match) whose table lists the tools.
pub const ROADMAP_SECTION: &str = "Tool apps";

/// Engine crates that only gameplay needs: a tool's kit may not depend on them.
pub const GAMEPLAY_CRATES: &[&str] = &["evani-physics", "evani-net", "evani-combat", "evani-destruct"];

/// What `kind` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppKind {
    #[default]
    Game,
    Tool,
}

impl AppKind {
    pub fn is_tool(self) -> bool {
        self == Self::Tool
    }
}

/// Reads the top-level `kind` of an evani.toml (absent: a game).
pub fn parse(evani_toml: &str) -> Result<AppKind, String> {
    let t: toml::Table = evani_toml.parse().map_err(|e| format!("evani.toml: {e}"))?;
    match t.get("kind") {
        None => Ok(AppKind::Game),
        Some(v) => match v.as_str() {
            Some("game") => Ok(AppKind::Game),
            Some("tool") => Ok(AppKind::Tool),
            _ => Err("evani.toml: `kind` must be \"game\" or \"tool\"".into()),
        },
    }
}

/// The kit ids in ROADMAP.md's [`ROADMAP_SECTION`] table.
pub fn roadmap_tools(md: &str) -> Vec<String> {
    let mut inside = false;
    let mut out = Vec::new();
    for l in md.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            inside = h.trim().starts_with(ROADMAP_SECTION);
            continue;
        }
        if !inside {
            continue;
        }
        let cols: Vec<&str> = l.split('|').map(str::trim).collect();
        // | # | Kit | ID | Rank | Physics | First look | Status | Notes |
        if cols.len() >= 9 && !cols[1].is_empty() && cols[1].chars().all(|c| c.is_ascii_digit()) {
            out.push(cols[3].trim_matches('`').to_string());
        }
    }
    out
}

/// Whether `catalog/genres.json` (its text) has a genre with id `kit`.
fn is_genre(genres_json: &str, kit: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(genres_json) else { return false };
    v.get("genres").and_then(|g| g.as_array()).is_some_and(|g| g.iter().any(|x| x.get("id").and_then(|i| i.as_str()) == Some(kit)))
}

/// The gameplay crates a kit's Cargo.toml (its text) depends on, in any
/// dependency table (dev-dependencies too: a test of gameplay is gameplay).
fn gameplay_deps(cargo_toml: &str) -> Vec<&'static str> {
    let Ok(t) = cargo_toml.parse::<toml::Table>() else { return Vec::new() };
    let tables = ["dependencies", "dev-dependencies", "build-dependencies"];
    GAMEPLAY_CRATES
        .iter()
        .copied()
        .filter(|c| tables.iter().any(|k| t.get(*k).and_then(|d| d.as_table()).is_some_and(|d| d.contains_key(*c))))
        .collect()
}

/// Every way an app that says `kind = "tool"` breaks the tool rule; empty for
/// a game (it claims nothing) and for a real tool. `kit` is the kit id.
pub fn problems(root: &Path, app: &str, kit: &str, evani_toml: &str) -> Vec<Problem> {
    let kind = match parse(evani_toml) {
        Ok(k) => k,
        Err(e) => return vec![Problem { what: e, fix: format!("In apps/{app}/evani.toml write `kind = \"tool\"` for a tool app, or leave `kind` out for a game.") }],
    };
    if !kind.is_tool() {
        return Vec::new();
    }
    let game = format!("If it is a game, remove `kind = \"tool\"` from apps/{app}/evani.toml: a game keeps its gameplay objects in the ECS and passes the entity check.");
    let mut out = Vec::new();
    let t: toml::Table = evani_toml.parse().unwrap_or_default();
    let modules = t.get("modules").and_then(|m| m.as_table());
    let physics = modules.and_then(|m| m.get("physics")).and_then(|p| p.as_str()).unwrap_or("none");
    if physics != "none" {
        out.push(Problem { what: format!("says `kind = \"tool\"` but lists physics `{physics}`; a tool simulates nothing"), fix: format!("Set `physics = \"none\"` under `[modules]`. {game}") });
    }
    if modules.and_then(|m| m.get("net")).and_then(|n| n.as_bool()) == Some(true) {
        out.push(Problem { what: "says `kind = \"tool\"` but turns on `net`; a tool has no players to share a game between".into(), fix: format!("Set `net = false` under `[modules]`. {game}") });
    }
    let genres = std::fs::read_to_string(root.join("catalog").join("genres.json")).unwrap_or_default();
    if is_genre(&genres, kit) {
        out.push(Problem { what: format!("says `kind = \"tool\"` but kit `{kit}` is a genre in catalog/genres.json, so it is a game"), fix: game.clone() });
    }
    let roadmap = std::fs::read_to_string(root.join("ROADMAP.md")).unwrap_or_default();
    if !roadmap_tools(&roadmap).iter().any(|id| id == kit) {
        out.push(Problem {
            what: format!("says `kind = \"tool\"` but kit `{kit}` has no row in ROADMAP.md's \"{ROADMAP_SECTION}\" table"),
            fix: format!("List the tool in the `## {ROADMAP_SECTION}` table of ROADMAP.md (and nowhere else). {game}"),
        });
    }
    let cargo = std::fs::read_to_string(root.join("kits").join(format!("evani-kit-{kit}")).join("Cargo.toml")).unwrap_or_default();
    let deps = gameplay_deps(&cargo);
    if !deps.is_empty() {
        out.push(Problem {
            what: format!("says `kind = \"tool\"` but kit `{kit}` depends on gameplay crates ({})", deps.join(", ")),
            fix: format!("Drop them from kits/evani-kit-{kit}/Cargo.toml; a tool has no gameplay. {game}"),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROADMAP: &str = "# Roadmap\n\n## Phase A\n\n| # | Kit | ID | Rank | Physics | First look | Status | Notes |\n|---|---|---|---|---|---|---|---|\n| 001 | Kart racer | `kart-racer` | #19 | simple | `toon` | done | x |\n\n## Tool apps (not games)\n\n| # | Kit | ID | Rank | Physics | First look | Status | Notes |\n|---|---|---|---|---|---|---|---|\n| 900 | Viewer | `viewer` | - | none | `basic` | review | y |\n";
    const TOOL: &str = "name = \"viewer\"\nkit = \"viewer\"\nlook = \"basic\"\nkind = \"tool\"\n[modules]\nphysics = \"none\"\n";

    fn root(name: &str) -> std::path::PathBuf {
        let r = crate::testutil::temp_root(name);
        let w = |p: &str, s: &str| crate::write_file(&r.join(p), s).unwrap();
        w("ROADMAP.md", ROADMAP);
        w("catalog/genres.json", r#"{"genres":[{"id":"kart-racer"}]}"#);
        w("kits/evani-kit-viewer/Cargo.toml", "[package]\nname = \"evani-kit-viewer\"\n[dependencies]\nevani-core = { workspace = true }\n");
        w("kits/evani-kit-kart-racer/Cargo.toml", "[package]\nname = \"k\"\n[dependencies]\nevani-core = { workspace = true }\n");
        r
    }

    #[test]
    fn reads_the_kind() {
        assert_eq!(parse("name = \"x\"\n").unwrap(), AppKind::Game);
        assert_eq!(parse("kind = \"game\"\n").unwrap(), AppKind::Game);
        assert_eq!(parse("kind = \"tool\"\n").unwrap(), AppKind::Tool);
        assert!(parse("kind = \"toy\"\n").is_err());
        assert!(parse("kind = true\n").is_err());
        assert_eq!(roadmap_tools(ROADMAP), vec!["viewer".to_string()]);
    }

    #[test]
    fn a_real_tool_passes_and_a_game_claims_nothing() {
        let r = root("kind-ok");
        assert_eq!(problems(&r, "viewer", "viewer", TOOL), Vec::new());
        let game = "name = \"kart-racer\"\nkit = \"kart-racer\"\nlook = \"toon\"\n[modules]\nphysics = \"simple\"\n";
        assert_eq!(problems(&r, "kart-racer", "kart-racer", game), Vec::new(), "a game that claims nothing has nothing to break");
        let _ = std::fs::remove_dir_all(&r);
    }

    /// A game can't skip the entity check by writing `kind = "tool"`: each
    /// rule, broken alone, refuses the claim.
    #[test]
    fn a_game_claiming_the_tool_kind_is_refused() {
        let r = root("kind-claim");
        let has = |p: &[Problem], s: &str| p.iter().any(|x| x.what.contains(s));
        // The kart racer as it is, plus `kind = "tool"`: a genre, physics, no tool row.
        let kart = "name = \"kart-racer\"\nkit = \"kart-racer\"\nlook = \"toon\"\nkind = \"tool\"\n[modules]\nphysics = \"simple\"\n";
        let p = problems(&r, "kart-racer", "kart-racer", kart);
        assert!(has(&p, "physics `simple`") && has(&p, "genre") && has(&p, "Tool apps"), "{p:?}");
        // One rule at a time on the real tool.
        let p = problems(&r, "viewer", "viewer", &TOOL.replace("\"none\"", "\"rigid\""));
        assert!(p.len() == 1 && has(&p, "physics"), "{p:?}");
        let p = problems(&r, "viewer", "viewer", &format!("{TOOL}net = true\n"));
        assert!(p.len() == 1 && has(&p, "net"), "{p:?}");
        crate::write_file(&r.join("catalog/genres.json"), r#"{"genres":[{"id":"viewer"}]}"#).unwrap();
        let p = problems(&r, "viewer", "viewer", TOOL);
        assert!(p.len() == 1 && has(&p, "genre"), "{p:?}");
        crate::write_file(&r.join("catalog/genres.json"), r#"{"genres":[]}"#).unwrap();
        crate::write_file(&r.join("ROADMAP.md"), &ROADMAP.replace("## Tool apps (not games)", "## Phase B")).unwrap();
        let p = problems(&r, "viewer", "viewer", TOOL);
        assert!(p.len() == 1 && has(&p, "ROADMAP"), "a tool row in a game table doesn't count: {p:?}");
        crate::write_file(&r.join("ROADMAP.md"), ROADMAP).unwrap();
        crate::write_file(&r.join("kits/evani-kit-viewer/Cargo.toml"), "[package]\nname = \"v\"\n[dev-dependencies]\nevani-physics = { workspace = true }\n").unwrap();
        let p = problems(&r, "viewer", "viewer", TOOL);
        assert!(p.len() == 1 && has(&p, "evani-physics"), "{p:?}");
        assert!(!problems(&r, "viewer", "viewer", "name = \"v\"\nkind = 3\n").is_empty());
        let _ = std::fs::remove_dir_all(&r);
    }

    /// On this repo: the material viewer is a real tool, and no shipped game
    /// could claim the tool kind.
    #[test]
    fn real_apps() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Ok(rd) = std::fs::read_dir(root.join("apps")) else { return };
        let mut tools = Vec::new();
        for e in rd.flatten() {
            let app = e.file_name().to_string_lossy().to_string();
            let Ok(text) = std::fs::read_to_string(e.path().join("evani.toml")) else { continue };
            let Ok(m) = crate::manifest::Modules::parse(&text) else { continue };
            if parse(&text) == Ok(AppKind::Tool) {
                assert_eq!(problems(&root, &app, &m.kit, &text), Vec::new(), "{app} says it is a tool");
                tools.push(app);
                continue;
            }
            let genres = std::fs::read_to_string(root.join("catalog/genres.json")).unwrap_or_default();
            if is_genre(&genres, &m.kit) {
                let claimed = format!("kind = \"tool\"\n{text}");
                assert!(!problems(&root, &app, &m.kit, &claimed).is_empty(), "genre game {app} could claim the tool kind");
            }
        }
        if root.join("apps/material-view").is_dir() {
            assert!(tools.iter().any(|t| t == "material-view"), "the material viewer is a tool app: {tools:?}");
        }
    }
}
