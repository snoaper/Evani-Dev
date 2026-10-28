//! `ai-eval`: one-shot "make a game" evals scored by `check` (stub).
//!
//! `--plan` prints, per prompt in `guide/evals.toml`, the worktree to make
//! and the agent command to run; `--score <dir>` scores a finished worktree
//! with `check` as JSON. This never starts an agent itself: agent runs cost
//! money and time and are started by a person (docs/AI_GUIDANCE.md).

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use crate::check;

const EVALS: &str = include_str!("../guide/evals.toml");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eval {
    pub id: String,
    pub prompt: String,
    /// What a human grader looks for beyond `check`.
    pub checks: String,
}

pub fn evals() -> Result<Vec<Eval>, String> {
    let t: toml::Table = EVALS.parse().map_err(|e| format!("evals.toml: {e}"))?;
    t.get("eval")
        .and_then(toml::Value::as_array)
        .ok_or("evals.toml: no [[eval]]")?
        .iter()
        .map(|e| {
            let s = |k: &str| e.get(k).and_then(toml::Value::as_str).map(str::to_string).ok_or_else(|| format!("evals.toml: an eval has no `{k}`"));
            Ok(Eval { id: s("id")?, prompt: s("prompt")?, checks: s("checks").unwrap_or_default() })
        })
        .collect()
}

/// The plan: one entry per eval with its worktree and commands.
pub fn plan(root: &Path, agent: &str) -> Result<Value, String> {
    let name = root.file_name().map_or("project".into(), |n| n.to_string_lossy().into_owned());
    let parent = root.parent().unwrap_or(root);
    let runs: Vec<Value> = evals()?
        .into_iter()
        .map(|e| {
            let dir = parent.join(format!("{name}-eval-{}", e.id)).display().to_string();
            json!({
                "id": e.id,
                "prompt": e.prompt,
                "grader_checks": e.checks,
                "worktree": dir,
                "setup": [
                    format!("git worktree add --detach \"{dir}\" HEAD"),
                    format!("cd \"{dir}\" && cargo xtask init-ai"),
                ],
                "agent": format!("{agent} {}", shell_quote(&e.prompt)),
                "score": format!("cargo xtask ai-eval --score \"{dir}\""),
                "cleanup": format!("git worktree remove --force \"{dir}\""),
            })
        })
        .collect();
    Ok(json!({ "note": "Run each agent by hand or from a script; ai-eval never starts one.", "runs": runs }))
}

fn shell_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// Games the agent added (untracked `apps/<id>/` in the worktree).
fn new_games(dir: &Path) -> Vec<String> {
    let Ok(o) = Command::new("git").current_dir(dir).args(["status", "--porcelain", "--untracked-files=normal", "--", "apps"]).output() else {
        return Vec::new();
    };
    let mut out: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("?? apps/"))
        .filter_map(|r| r.split('/').next())
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Score a finished eval worktree: `check` (with `full`) on the whole project.
pub fn score(dir: &Path, full: bool) -> Value {
    let report = check::run(dir, &check::Options { full, ..check::Options::default() });
    let games = new_games(dir);
    let errors = report.count(check::Severity::Error);
    json!({
        "dir": dir.display().to_string(),
        "new_games": games,
        "errors": errors,
        "warnings": report.count(check::Severity::Warn),
        "pass": !games.is_empty() && errors == 0,
        "issues": report.issues.iter().map(check::Issue::to_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_evals_parse_and_plan() {
        let e = evals().unwrap();
        assert!(e.len() >= 3);
        assert!(e.iter().all(|x| !x.prompt.is_empty()));
        let r = crate::testutil::temp_root("evals");
        let p = plan(&r, "claude -p").unwrap();
        let first = &p["runs"][0];
        assert!(first["agent"].as_str().unwrap().starts_with("claude -p \""));
        assert!(first["worktree"].as_str().unwrap().contains("-eval-"));
    }

    #[test]
    fn scoring_a_project_without_new_games_fails() {
        let r = crate::testutil::fake_engine("evals-score");
        let s = score(&r, false);
        assert_eq!(s["pass"], false);
        assert_eq!(s["errors"], 0);
    }
}
