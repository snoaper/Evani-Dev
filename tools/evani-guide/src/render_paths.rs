//! Render paths per tier: which of them a game's look needs and whether they
//! are built (`assets/render_paths.json`, the same table as
//! `crates/evani-render/src/render_path.rs`; a test there keeps them equal).
//! `check` gives an ERROR for a game that targets a path that isn't built;
//! `cargo xtask build` and the review card print [`report`].
//!
//! Read as plain JSON so this crate stays light (no engine crates).

use std::path::Path;

use serde_json::Value;

const TIERS: [(&str, &str); 6] = [("lite", "Lite"), ("low", "Low"), ("medium", "Medium"), ("high", "High"), ("ultra", "Ultra"), ("vr", "VR")];

fn read_json(root: &Path, rel: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(root.join(rel)).ok()?).ok()
}

/// The style class (`2d`, `pseudo-3d`, `3d`) of a look, from the catalog.
pub fn style_of_look(root: &Path, look: &str) -> Option<String> {
    if let Some((_, s)) = crate::check::NEUTRAL_LOOKS.iter().find(|(l, _)| *l == look) {
        return Some((*s).to_string());
    }
    read_json(root, "catalog/looks.json")?
        .get("looks")?
        .as_array()?
        .iter()
        .find(|l| l.get("id").and_then(Value::as_str) == Some(look))?
        .get("style_class")?
        .as_str()
        .map(str::to_string)
}

/// One tier of a style's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierPath {
    pub tier: &'static str,
    pub label: &'static str,
    pub runs: bool,
    pub missing: Vec<String>,
}

/// The tiers of `style`'s path (Lite to VR; a tier an older table lacks is
/// left out), or `None` when the table is missing or
/// has no such style.
pub fn tiers(root: &Path, style: &str) -> Option<Vec<TierPath>> {
    let j = read_json(root, "assets/render_paths.json")?;
    let p = j.get("paths")?.get(style)?;
    TIERS
        .iter()
        .filter(|(id, _)| p.get(*id).is_some())
        .map(|(id, label)| {
            let t = p.get(*id)?;
            Some(TierPath {
                tier: id,
                label,
                runs: t.get("runs")?.as_bool()?,
                missing: t.get("missing")?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect(),
            })
        })
        .collect()
}

/// Where a missing part is filed: `Stage 5 (kit 004 (look pixelhd))`.
fn filed(root: &Path, id: &str) -> String {
    let d = read_json(root, "assets/render_paths.json").and_then(|j| j.get("deferred")?.get(id).cloned());
    match d {
        Some(d) => format!("{id} (Stage {}: {})", d.get("stage").and_then(Value::as_u64).unwrap_or(0), d.get("who").and_then(Value::as_str).unwrap_or("")),
        None => id.to_string(),
    }
}

/// One line per tier the game targets: `3d/high: runs; missing point-lights, ...`.
/// VR is listed only for a game that builds the `xr` module.
pub fn report(root: &Path, style: &str, xr: bool) -> Vec<String> {
    tiers(root, style)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| xr || t.tier != "vr")
        .map(|t| {
            let state = if !t.runs {
                "NOT BUILT"
            } else if t.missing.is_empty() {
                "built"
            } else {
                "runs"
            };
            let mut s = format!("{style}/{}: {state}", t.tier);
            if !t.missing.is_empty() {
                s.push_str("; missing ");
                s.push_str(&t.missing.join(", "));
            }
            s
        })
        .collect()
}

/// One way a game targets a path that is not built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub what: String,
    pub fix: String,
}

/// The tiers of `style`'s path that have no base pass. VR never counts: the VR
/// tier exists only inside an immersive WebXR session and is reported
/// unavailable otherwise (STAGES.md Stage 0; real VR is Stage 9). Empty when
/// the table is absent (a fake or older engine).
pub fn problems(root: &Path, style: &str, look: &str) -> Vec<Problem> {
    let Some(all) = tiers(root, style) else { return Vec::new() };
    let bad: Vec<&TierPath> = all.iter().filter(|t| !t.runs && t.tier != "vr").collect();
    if bad.is_empty() {
        return Vec::new();
    }
    let names = bad.iter().map(|t| t.label).collect::<Vec<_>>().join(", ");
    let mut parts: Vec<String> = Vec::new();
    for t in &bad {
        for m in &t.missing {
            let f = filed(root, m);
            if !parts.contains(&f) {
                parts.push(f);
            }
        }
    }
    let what = format!("look `{look}` is a {style} look, and the {style} render path is not built on {names}; missing: {}", parts.join("; "));
    let fix = "Pick a look whose style is built (2d or 3d; catalog/looks.json `style_class` says which) from the genre's compatible_looks, or wait for the stage that builds the path (see `deferred` in assets/render_paths.json and CORE_REQUESTS.md). Never ship onto a path that doesn't exist.".to_string();
    vec![Problem { what, fix }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_four_games_target_built_paths() {
        let root = engine_root();
        for (look, xr) in [("basic", false), ("toon", false), ("pixel16", false), ("cartoon2d", false)] {
            let style = style_of_look(&root, look).unwrap();
            assert!(problems(&root, &style, look).is_empty(), "{look}: {:?}", problems(&root, &style, look));
            let r = report(&root, &style, xr);
            assert_eq!(r.len(), 5, "no VR line without xr: {r:?}");
            assert!(r.iter().all(|l| !l.contains("NOT BUILT")), "{r:?}");
        }
    }

    #[test]
    fn an_unbuilt_path_is_a_problem_with_a_fix_and_names_the_stage() {
        let root = engine_root();
        let p = problems(&root, "pseudo-3d", "mode7");
        assert_eq!(p.len(), 1);
        assert!(p[0].what.contains("pseudo-3d-pass (Stage 6"), "{}", p[0].what);
        assert!(p[0].what.contains("Lite, Low, Medium, High, Ultra") && !p[0].what.contains("VR"), "{}", p[0].what);
        assert!(p[0].fix.contains("Pick a look"));
        // VR is never an error (it is unavailable outside an immersive session; real VR is Stage 9) but it is reported for a game with the xr module.
        assert!(problems(&root, "3d", "toon").is_empty());
        let r = report(&root, "3d", true);
        assert_eq!(r.len(), 6);
        assert!(r[5].starts_with("3d/vr: NOT BUILT; missing vr-stereo"), "{r:?}");
    }

    #[test]
    fn report_lists_what_is_missing() {
        let r = report(&engine_root(), "3d", false);
        assert_eq!(r[0], "3d/lite: built");
        assert_eq!(r[1], "3d/low: built");
        assert!(r[2].starts_with("3d/medium: runs; missing cpu-frustum-culling"), "{r:?}");
    }

    #[test]
    fn no_table_means_no_problem() {
        let root = crate::testutil::fake_engine("render-paths-none");
        assert!(problems(&root, "pseudo-3d", "mode7").is_empty());
        assert_eq!(style_of_look(&root, "toon").as_deref(), Some("3d"));
    }
}
