//! Which files carry Evani's guidance, what each should hold, and how to
//! write it without touching the project's own text.
//!
//! - `AGENTS.md`: the engine rules (block `evani-agent-rules`).
//! - `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/evani.mdc` (and with
//!   `--with copilot`, `.github/copilot-instructions.md`): a pointer block.
//! - `apps/<id>/AGENTS.md`, `kits/AGENTS.md`, `looks/AGENTS.md`: short
//!   folder rules, each with a `CLAUDE.md` pointer next to it (Claude Code
//!   loads nested CLAUDE.md files, not AGENTS.md).
//! - `.mcp.json`: the `evani-live` server entry only.
//! - `.claude/settings.json`: the Stop hook that runs `check --hook` only.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::block::{self, Place, State};
use crate::render;

/// What `init-ai` writes besides the defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Also `.github/copilot-instructions.md` (off by default: no `.github/`
    /// files unless the project asks for them).
    pub copilot: bool,
}

#[derive(Debug, Clone)]
pub enum Content {
    /// A marked block in a Markdown file. `prefix` starts a new file (the
    /// Cursor front matter).
    Block { id: &'static str, body: String, place: Place, prefix: &'static str },
    /// The `evani-live` entry of `.mcp.json`.
    Mcp,
    /// Evani's Stop hook in `.claude/settings.json`.
    Hook,
}

#[derive(Debug, Clone)]
pub struct Target {
    /// From the project root, `/` separators.
    pub rel: String,
    pub content: Content,
}

/// The `.mcp.json` entry: the prebuilt server, so a session never waits for
/// a build (see [`crate::mcpbin`]). A relative path: Claude Code starts stdio
/// servers in the project folder.
pub fn mcp_entry() -> Value {
    json!({ "type": "stdio", "command": ".evani/bin/evani-live-bridge", "args": ["mcp"] })
}

/// The Stop hook (exec form, no shell): `check --hook` blocks the end of a
/// turn while the games it touched have errors.
pub fn hook_entry() -> Value {
    json!({
        "type": "command",
        "command": "cargo",
        "args": ["run", "--quiet", "--package", "evani-guide", "--target-dir", "target/guide", "--", "check", "--hook"],
        "timeout": 300
    })
}

fn is_our_hook(h: &Value) -> bool {
    let args: Vec<&str> = h.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let cmd = h.get("command").and_then(Value::as_str).unwrap_or("");
    (args.contains(&"evani-guide") || cmd.contains("evani-guide")) && (args.contains(&"--hook") || cmd.contains("--hook"))
}

/// The games: every `apps/<id>/` with an `evani.toml`, sorted.
pub fn games(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root.join("apps"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("evani.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn md(rel: &str, id: &'static str, body: String) -> Target {
    Target { rel: rel.into(), content: Content::Block { id, body, place: Place::Top, prefix: "" } }
}

/// Every file the guidance lives in for the project at `root`.
pub fn targets(root: &Path, opts: Options) -> Vec<Target> {
    let mut t = vec![
        md("AGENTS.md", render::ROOT_ID, render::root_body(root)),
        md("CLAUDE.md", render::POINTER_ID, render::claude_pointer()),
        md("GEMINI.md", render::POINTER_ID, render::gemini_pointer()),
        Target {
            rel: ".cursor/rules/evani.mdc".into(),
            content: Content::Block {
                id: render::POINTER_ID,
                body: render::read_agents_pointer(),
                place: Place::AfterFrontMatter,
                prefix: render::CURSOR_FRONT,
            },
        },
    ];
    let copilot = ".github/copilot-instructions.md";
    if opts.copilot || root.join(copilot).is_file() {
        t.push(md(copilot, render::POINTER_ID, render::read_agents_pointer()));
    }
    let mut nested = |dir: String, body: String| {
        t.push(md(&format!("{dir}/AGENTS.md"), render::FOLDER_ID, body));
        t.push(md(&format!("{dir}/CLAUDE.md"), render::POINTER_ID, render::claude_pointer()));
    };
    for g in games(root) {
        nested(format!("apps/{g}"), render::app_body(root, &g));
    }
    if root.join("kits").is_dir() {
        nested("kits".into(), render::kits_body(root));
    }
    if root.join("looks").is_dir() {
        nested("looks".into(), render::looks_body(root));
    }
    t.push(Target { rel: ".mcp.json".into(), content: Content::Mcp });
    t.push(Target { rel: ".claude/settings.json".into(), content: Content::Hook });
    t
}

/// How a target compares with what this engine writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TState {
    Missing,
    Current,
    Stale(String),
    /// A block whose text no longer matches its own stamp.
    Edited,
    /// The file can't be read or rewritten safely (broken markers, bad JSON).
    Broken(String),
}

impl Target {
    pub fn state(&self, root: &Path) -> TState {
        let existing = match std::fs::read_to_string(root.join(&self.rel)) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return TState::Broken(e.to_string()),
        };
        match &self.content {
            Content::Block { id, body, .. } => {
                let Some(text) = existing else { return TState::Missing };
                match block::state(&text, id, crate::ENGINE_VERSION, body) {
                    Ok(State::Missing) => TState::Missing,
                    Ok(State::Current) => TState::Current,
                    Ok(State::Stale { engine }) => TState::Stale(format!("written by engine {engine}, or older guidance")),
                    Ok(State::Edited) => TState::Edited,
                    Err(e) => TState::Broken(e),
                }
            }
            Content::Mcp | Content::Hook => {
                let Some(text) = existing else { return TState::Missing };
                match (self.desired(Some(&text)), json_obj(&text)) {
                    (Err(e), _) | (_, Err(e)) => TState::Broken(e),
                    (Ok(want), Ok(have)) => {
                        let want = json_obj(&want).unwrap_or_default();
                        if want == have {
                            TState::Current
                        } else if self.has_entry(&have) {
                            TState::Stale("our entry differs from this engine's".into())
                        } else {
                            TState::Missing
                        }
                    }
                }
            }
        }
    }

    fn has_entry(&self, v: &Map<String, Value>) -> bool {
        match self.content {
            Content::Mcp => v.get("mcpServers").and_then(|s| s.get("evani-live")).is_some(),
            Content::Hook => stop_groups(v).iter().any(|g| hooks_of(g).iter().any(is_our_hook)),
            Content::Block { .. } => false,
        }
    }

    /// The file's text with our part set; everything else kept.
    pub fn desired(&self, existing: Option<&str>) -> Result<String, String> {
        match &self.content {
            Content::Block { id, body, place, prefix } => {
                let base = match existing {
                    Some(t) => t.to_string(),
                    None => prefix.to_string(),
                };
                block::upsert(Some(&base), id, crate::ENGINE_VERSION, body, *place)
            }
            Content::Mcp => {
                let mut v = existing.map(json_obj).transpose()?.unwrap_or_default();
                let servers = v.entry("mcpServers").or_insert_with(|| json!({}));
                let Some(servers) = servers.as_object_mut() else {
                    return Err("`mcpServers` is not an object".into());
                };
                servers.insert("evani-live".into(), mcp_entry());
                Ok(pretty(&v))
            }
            Content::Hook => {
                let mut v = existing.map(json_obj).transpose()?.unwrap_or_default();
                let hooks = v.entry("hooks").or_insert_with(|| json!({}));
                let Some(hooks) = hooks.as_object_mut() else {
                    return Err("`hooks` is not an object".into());
                };
                let stop = hooks.entry("Stop").or_insert_with(|| json!([]));
                let Some(groups) = stop.as_array_mut() else {
                    return Err("`hooks.Stop` is not a list".into());
                };
                let mut placed = false;
                for g in groups.iter_mut() {
                    if let Some(list) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                        for h in list.iter_mut() {
                            if is_our_hook(h) {
                                if placed {
                                    *h = Value::Null;
                                } else {
                                    *h = hook_entry();
                                    placed = true;
                                }
                            }
                        }
                        list.retain(|h| !h.is_null());
                    }
                }
                if !placed {
                    groups.push(json!({ "hooks": [hook_entry()] }));
                }
                Ok(pretty(&v))
            }
        }
    }

    /// Write our part. `Ok(true)` when the file changed.
    pub fn write(&self, root: &Path) -> Result<bool, String> {
        let path = root.join(&self.rel);
        let existing = std::fs::read_to_string(&path).ok();
        let want = self.desired(existing.as_deref()).map_err(|e| format!("{}: {e}", self.rel))?;
        if existing.as_deref() == Some(want.as_str()) {
            return Ok(false);
        }
        crate::write_file(&path, &want)?;
        Ok(true)
    }
}

fn json_obj(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err("not a JSON object".into()),
        Err(e) => Err(format!("not valid JSON ({e})")),
    }
}

fn stop_groups(v: &Map<String, Value>) -> Vec<Value> {
    v.get("hooks").and_then(|h| h.get("Stop")).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn hooks_of(g: &Value) -> Vec<Value> {
    g.get("hooks").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn pretty(v: &Map<String, Value>) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(t: &'a [Target], rel: &str) -> &'a Target {
        t.iter().find(|x| x.rel == rel).unwrap()
    }

    #[test]
    fn a_fresh_project_gets_every_file_and_a_second_run_changes_nothing() {
        let r = crate::testutil::fake_engine("targets-fresh");
        std::fs::create_dir_all(r.join("kits")).unwrap();
        let ts = targets(&r, Options::default());
        let rels: Vec<&str> = ts.iter().map(|t| t.rel.as_str()).collect();
        for want in ["AGENTS.md", "CLAUDE.md", "GEMINI.md", ".cursor/rules/evani.mdc", "apps/demo/AGENTS.md", "apps/demo/CLAUDE.md", "kits/AGENTS.md", ".mcp.json", ".claude/settings.json"] {
            assert!(rels.contains(&want), "{want} in {rels:?}");
        }
        assert!(!rels.iter().any(|r| r.starts_with(".github")), "copilot is opt-in");
        for t in &ts {
            assert_eq!(t.state(&r), TState::Missing, "{}", t.rel);
            assert!(t.write(&r).unwrap(), "{}", t.rel);
            assert_eq!(t.state(&r), TState::Current, "{}", t.rel);
            assert!(!t.write(&r).unwrap(), "{} twice", t.rel);
        }
        let mdc = std::fs::read_to_string(r.join(".cursor/rules/evani.mdc")).unwrap();
        assert!(mdc.starts_with("---\ndescription:") && mdc.contains("alwaysApply: true\n---\n<!-- BEGIN:evani-pointer"));
        let claude = std::fs::read_to_string(r.join("CLAUDE.md")).unwrap();
        assert!(claude.contains("\n@AGENTS.md\n"));
    }

    #[test]
    fn json_merges_keep_everything_else() {
        let r = crate::testutil::temp_root("targets-json");
        crate::write_file(
            &r.join(".mcp.json"),
            r#"{"mcpServers":{"other":{"command":"x"},"evani-live":{"command":"cargo","args":["run"]}},"extra":1}"#,
        )
        .unwrap();
        let mcp = Target { rel: ".mcp.json".into(), content: Content::Mcp };
        assert!(matches!(mcp.state(&r), TState::Stale(_)));
        mcp.write(&r).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(r.join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["extra"], 1);
        assert_eq!(v["mcpServers"]["evani-live"], mcp_entry());
        assert_eq!(mcp.state(&r), TState::Current);

        let mine = json!({"permissions":{"allow":["Bash(ls)"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo hi"}]}],"PreToolUse":[{"matcher":"Bash","hooks":[]}]}});
        crate::write_file(&r.join(".claude/settings.json"), &mine.to_string()).unwrap();
        let hook = Target { rel: ".claude/settings.json".into(), content: Content::Hook };
        assert_eq!(hook.state(&r), TState::Missing);
        hook.write(&r).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(r.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash");
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the project's own Stop hook stays");
        assert_eq!(stop[0]["hooks"][0]["command"], "echo hi");
        assert_eq!(stop[1]["hooks"][0], hook_entry());
        // An older version of our hook is replaced in place, not added twice.
        let mut old = v.clone();
        old["hooks"]["Stop"][1]["hooks"][0]["timeout"] = json!(60);
        crate::write_file(&r.join(".claude/settings.json"), &old.to_string()).unwrap();
        assert!(matches!(hook.state(&r), TState::Stale(_)));
        hook.write(&r).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(r.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["Stop"][1]["hooks"][0]["timeout"], 300);
    }

    #[test]
    fn bad_json_is_reported_not_overwritten() {
        let r = crate::testutil::temp_root("targets-badjson");
        crate::write_file(&r.join(".mcp.json"), "{ not json").unwrap();
        let mcp = Target { rel: ".mcp.json".into(), content: Content::Mcp };
        assert!(matches!(mcp.state(&r), TState::Broken(_)));
        assert!(mcp.write(&r).is_err());
        assert_eq!(std::fs::read_to_string(r.join(".mcp.json")).unwrap(), "{ not json");
    }

    #[test]
    fn project_text_in_claude_md_survives() {
        let r = crate::testutil::fake_engine("targets-claude");
        crate::write_file(&r.join("CLAUDE.md"), "# My rules\n\n- one kit per session\n").unwrap();
        let ts = targets(&r, Options { copilot: true });
        assert!(ts.iter().any(|t| t.rel == ".github/copilot-instructions.md"));
        let c = find(&ts, "CLAUDE.md");
        c.write(&r).unwrap();
        let text = std::fs::read_to_string(r.join("CLAUDE.md")).unwrap();
        assert!(text.starts_with("<!-- BEGIN:evani-pointer"));
        assert!(text.ends_with("# My rules\n\n- one kit per session\n"));
    }
}
