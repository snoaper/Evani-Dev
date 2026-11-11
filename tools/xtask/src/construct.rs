//! `cargo xtask model`: build Evani.Model construct recipes (the exact-piece
//! back end, docs/MODELGEN_PLAN.md 3.6 and 3.8) into a game's model files.
//!
//! For a game: every `kits/evani-kit-<app>/models/<model>.construct.json`
//! (or the one `--model` names) is built at its tiers by `evani-model
//! construct` (checks 1 and 4, budgets, `--check` builds twice and compares
//! bytes) into `.evani/tmp/construct/<app>/<model>/`, converted by the
//! model converter in Evani.Model mode (compressed and checked, never
//! re-simplified) into `apps/<app>/assets/models/<model>/`, its report is
//! written beside the recipe (`<model>.evmodel.json`, which `cargo xtask
//! check` compares with the recipe), and the game's `stream.json` is
//! rewritten. One recipe anywhere: `--recipe <file>` writes the tier files,
//! report and tier picture to `--out` and, with `--convert`, the converted
//! files to `<out>/converted` (a game outside this workspace takes those).

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn usage() -> String {
    "  cargo xtask model <app> [--model <name>] [--tiers low,high] [--check] [--no-convert]   (construct recipes in kits/evani-kit-<app>/models/ to apps/<app>/assets/models/<name>/)\n  cargo xtask model --recipe <file.construct.json> [--out <folder>] [--tiers ...] [--check] [--convert]   (one recipe: tier files, report and tier picture; --convert adds <folder>/converted)".to_string()
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Build one recipe into `work`; returns the recipe's id (its model name).
fn build(root: &Path, recipe: &Path, work: &Path, tiers: Option<&str>, check: bool) -> Result<String, String> {
    std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["run", "--release", "--quiet", "-p", "evani-model-tool", "--", "construct"]).arg(recipe).arg("--out").arg(work).arg("--sheet");
    if let Some(t) = tiers {
        cmd.args(["--tiers", t]);
    }
    if check {
        cmd.arg("--check");
    }
    let out = cmd.current_dir(root).output().map_err(|e| format!("cargo run -p evani-model-tool: {e}"))?;
    let reply: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| format!("evani-model construct gave no reply for {}:\n{}{}", recipe.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))?;
    if !out.status.success() {
        let msg = match reply.get("problems").and_then(|p| p.as_array()) {
            Some(ps) => ps.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("\n  "),
            None => reply.get("msg").and_then(|m| m.as_str()).unwrap_or("failed").to_string(),
        };
        return Err(format!("{}:\n  {msg}", recipe.display()));
    }
    let id = reply.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string();
    for (tier, t) in reply.get("tiers").and_then(|t| t.as_object()).into_iter().flatten() {
        println!(
            "  {id} {tier}: {} triangles (budget {}), {} draw calls, {} bytes, {} pieces",
            t["triangles"], t["budget"], t["draw_calls"], t["bytes"], t["pieces"]
        );
    }
    // Tier gain: each costlier tier against the one below, at its design view.
    for g in reply.get("tier_gain").and_then(|t| t.as_array()).into_iter().flatten() {
        println!(
            "  {id} tier gain {} -> {}: {:+} triangles, {:+} bytes, {:.1}% of the model's pixels changed at {} px ({})",
            g["lower"].as_str().unwrap_or("?"),
            g["higher"].as_str().unwrap_or("?"),
            g["extra_triangles"].as_i64().unwrap_or(0),
            g["extra_bytes"].as_i64().unwrap_or(0),
            g["changed_share"].as_f64().unwrap_or(0.0) * 100.0,
            g["design_px"],
            if g["ok"].as_bool().unwrap_or(false) { "ok" } else { "WARN" }
        );
    }
    for w in reply.get("warnings").and_then(|w| w.as_array()).into_iter().flatten().filter_map(|w| w.as_str()) {
        println!("{w}");
    }
    Ok(id)
}

/// Convert the tier files in `work` (Evani.Model mode) into `out`.
fn convert(root: &Path, work: &Path, id: &str, out: &Path) -> Result<(), String> {
    let top = ["ultra", "vr", "high", "medium", "low"].iter().map(|t| work.join(format!("{id}.{t}.glb"))).find(|p| p.is_file()).ok_or_else(|| format!("no tier files for {id} in {}", work.display()))?;
    let status = Command::new(env!("CARGO"))
        .args(["run", "--release", "--quiet", "-p", "evani-convert", "--"])
        .arg(&top)
        .args(["--id", id, "--out"])
        .arg(out)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo run -p evani-convert: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("converting {id} failed ({status}); its report is in {}", out.display())) }
}

pub fn cmd_model(args: &[String], root: &Path, manifest: &dyn Fn(&str) -> Result<(), String>) -> Result<(), String> {
    let tiers = value(args, "--tiers");
    let check = args.iter().any(|a| a == "--check");
    if let Some(recipe) = value(args, "--recipe") {
        let here = std::env::current_dir().map_err(|e| e.to_string())?;
        let recipe = here.join(recipe);
        let stem = recipe.file_name().map(|n| n.to_string_lossy().trim_end_matches(".construct.json").to_string()).unwrap_or_default();
        let out = value(args, "--out").map(|o| here.join(o)).unwrap_or_else(|| root.join(".evani").join("tmp").join("construct").join(&stem));
        let id = build(root, &recipe, &out, tiers.as_deref(), check)?;
        if args.iter().any(|a| a == "--convert") {
            convert(root, &out, &id, &out.join("converted"))?;
        }
        println!("model {id}: files in {}", out.display());
        return Ok(());
    }
    // The first word that is neither a flag nor a flag's value.
    let mut words = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "--model" | "--tiers" | "--out" | "--recipe") {
            it.next();
        } else if !a.starts_with("--") {
            words.push(a);
        }
    }
    let app = words.first().ok_or_else(usage)?.as_str();
    let kit_models = root.join("kits").join(format!("evani-kit-{app}")).join("models");
    let only = value(args, "--model");
    let mut recipes: Vec<PathBuf> = std::fs::read_dir(&kit_models)
        .map_err(|_| format!("no {} folder: a game's construct recipes live there ({app}'s kit)", kit_models.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".construct.json"))
        .filter(|p| only.as_deref().is_none_or(|m| p.file_name().is_some_and(|n| n.to_string_lossy() == format!("{m}.construct.json"))))
        .collect();
    recipes.sort();
    if recipes.is_empty() {
        return Err(format!("no construct recipe{} in {}", only.map(|m| format!(" named {m}")).unwrap_or_default(), kit_models.display()));
    }
    let convert_too = !args.iter().any(|a| a == "--no-convert");
    for r in &recipes {
        let stem = r.file_name().map(|n| n.to_string_lossy().trim_end_matches(".construct.json").to_string()).unwrap_or_default();
        let work = root.join(".evani").join("tmp").join("construct").join(app).join(&stem);
        let id = build(root, r, &work, tiers.as_deref(), check)?;
        if id != stem {
            return Err(format!("{}: the recipe's id is `{id}` but its file is named {stem}.construct.json; name the file after the id", r.display()));
        }
        if convert_too {
            convert(root, &work, &id, &root.join("apps").join(app).join("assets").join("models").join(&id))?;
        }
        std::fs::copy(work.join(format!("{id}.evmodel.json")), kit_models.join(format!("{id}.evmodel.json"))).map_err(|e| format!("report for {id}: {e}"))?;
        println!("model {id}: built{}; tier picture {}", if convert_too { " and converted" } else { "" }, work.join(format!("{id}.tiers.png")).display());
    }
    if convert_too {
        manifest(app)?;
    }
    Ok(())
}
