//! Writes `report.md`, `report.json` and `crops/*.png` into an output folder.

use crate::checks::{fmt_px, fmt_rect};
use crate::scene::{Atlas, Scene};
use crate::{Check, Issue};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// Counts after a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub issues: usize,
    pub failing: usize,
    pub skipped: usize,
}

fn location(i: &Issue) -> String {
    let mut parts = Vec::new();
    if let Some(o) = &i.object {
        parts.push(o.clone());
    }
    if let Some(t) = i.tile {
        parts.push(format!("tile ({}, {})", t[0], t[1]));
    }
    parts.push(format!("px {}", fmt_rect(i.px)));
    if let Some(t) = i.tick {
        parts.push(format!("tick {t}"));
    }
    parts.join(", ")
}

fn issue_json(n: usize, i: &Issue, crop: &str) -> Value {
    json!({
        "id": n,
        "check": i.check.slug(),
        "type": i.kind,
        "severity": i.severity,
        "object": i.object,
        "tile": i.tile,
        "px": i.px,
        "tick": i.tick,
        "location": location(i),
        "message": i.message,
        "fix": i.fix,
        "crop": crop,
        "tagged": i.tagged.is_some(),
        "reason": i.tagged,
    })
}

/// The `n` worst untagged issues: highest severity, then the largest amount within a type.
pub fn worst(issues: &[Issue], n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..issues.len()).filter(|&i| issues[i].tagged.is_none()).collect();
    idx.sort_by(|&a, &b| issues[b].severity.cmp(&issues[a].severity).then(issues[b].amount.total_cmp(&issues[a].amount)).then(a.cmp(&b)));
    // One per type first, so the list is not five of the same thing.
    let mut out: Vec<usize> = Vec::new();
    for &i in &idx {
        if out.len() < n && !out.iter().any(|&o| issues[o].kind == issues[i].kind) {
            out.push(i);
        }
    }
    for &i in &idx {
        if out.len() < n && !out.contains(&i) {
            out.push(i);
        }
    }
    out.sort_by(|&a, &b| issues[b].severity.cmp(&issues[a].severity).then(a.cmp(&b)));
    out
}

/// Render every crop and write both reports. `issues` come from [`crate::run_all`].
pub fn write(out: &Path, scene: &Scene, atlas: Option<&Atlas>, issues: &[Issue]) -> Result<Summary, String> {
    let crops = out.join("crops");
    std::fs::create_dir_all(&crops).map_err(|e| format!("{}: {e}", crops.display()))?;
    // Clear crops from an earlier run (only the files this writer makes).
    if let Ok(rd) = std::fs::read_dir(&crops) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".png") && name.len() > 4 && name[..3].chars().all(|c| c.is_ascii_digit()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut crop_paths = Vec::with_capacity(issues.len());
    for (n, i) in issues.iter().enumerate() {
        let rel = format!("crops/{:03}-{}.png", n + 1, i.kind);
        let cv = crate::render::crop(scene, atlas, i);
        crate::image::write_png(&out.join(&rel), cv.w, cv.h, &cv.rgba)?;
        crop_paths.push(rel);
    }
    let failing = issues.iter().filter(|i| i.tagged.is_none()).count();
    let sum = Summary { issues: issues.len(), failing, skipped: issues.len() - failing };

    // Group by type, in check order.
    let mut groups: BTreeMap<(Check, String), Vec<usize>> = BTreeMap::new();
    for (n, i) in issues.iter().enumerate() {
        groups.entry((i.check, i.kind.clone())).or_default().push(n);
    }

    // ---- report.json ----
    let by_type: serde_json::Map<String, Value> = groups
        .iter()
        .map(|((_, k), v)| {
            let f = v.iter().filter(|&&n| issues[n].tagged.is_none()).count();
            (k.clone(), json!({"count": v.len(), "failing": f, "skipped": v.len() - f}))
        })
        .collect();
    let gj: Vec<Value> = groups
        .iter()
        .map(|((c, k), v)| {
            json!({
                "check": c.slug(), "number": c.number(), "title": c.title(), "type": k, "count": v.len(),
                "failing": v.iter().filter(|&&n| issues[n].tagged.is_none()).count(),
                "issues": v.iter().map(|&n| issue_json(n + 1, &issues[n], &crop_paths[n])).collect::<Vec<_>>(),
            })
        })
        .collect();
    let not_run = crate::not_run(scene);
    let report = json!({
        "format": "evani-geocheck-2d-report/1",
        "scene": scene.name,
        "source": scene.source,
        "summary": {"issues": sum.issues, "failing": sum.failing, "skipped": sum.skipped, "by_type": by_type},
        "worst": worst(issues, 5).iter().map(|&n| n + 1).collect::<Vec<_>>(),
        "limits": scene.limits,
        "notes": scene.notes,
        "not_run": not_run.iter().map(|(c, why)| json!({"check": c.slug(), "number": c.number(), "reason": why})).collect::<Vec<_>>(),
        "groups": gj,
    });
    let text = serde_json::to_string_pretty(&report).map_err(|e| format!("report.json: {e}"))?;
    std::fs::write(out.join("report.json"), text).map_err(|e| format!("report.json: {e}"))?;

    // ---- report.md ----
    let mut md = String::new();
    let _ = writeln!(md, "# Geocheck 2D: {}\n", if scene.name.is_empty() { "scene" } else { &scene.name });
    if !scene.source.is_empty() {
        let _ = writeln!(md, "Source: {}\n", scene.source);
    }
    let _ = writeln!(md, "**{} issues: {} failing, {} skipped (tagged as intentional).**\n", sum.issues, sum.failing, sum.skipped);
    let _ = writeln!(md, "| # | Check | Type | Count | Failing |\n|---|---|---|---|---|");
    for c in Check::ALL {
        let rows: Vec<_> = groups.iter().filter(|((gc, _), _)| *gc == c).collect();
        if rows.is_empty() && not_run.iter().any(|(n, _)| *n == c) {
            let _ = writeln!(md, "| {} | {} | (not run) | - | - |", c.number(), c.title());
        } else if rows.is_empty() {
            let _ = writeln!(md, "| {} | {} | (none) | 0 | 0 |", c.number(), c.title());
        }
        for ((_, k), v) in rows {
            let f = v.iter().filter(|&&n| issues[n].tagged.is_none()).count();
            let _ = writeln!(md, "| {} | {} | `{k}` | {} | {f} |", c.number(), c.title(), v.len());
        }
    }
    let w = worst(issues, 5);
    if !w.is_empty() {
        let _ = writeln!(md, "\n## Worst {}\n", w.len());
        for (k, &n) in w.iter().enumerate() {
            let i = &issues[n];
            let _ = writeln!(md, "{}. **`{}`** (#{}, check {} {}, severity {}): {}", k + 1, i.kind, n + 1, i.check.number(), i.check.title(), i.severity, i.message);
            let _ = writeln!(md, "   - Where: {}", location(i));
            let _ = writeln!(md, "   - Fix: {}", i.fix);
            let _ = writeln!(md, "\n   ![#{}]({})\n", n + 1, crop_paths[n]);
        }
    }
    for c in Check::ALL {
        let _ = writeln!(md, "\n## {}. {}\n", c.number(), c.title());
        let list: Vec<usize> = (0..issues.len()).filter(|&n| issues[n].check == c).collect();
        if let Some((_, why)) = not_run.iter().find(|(n, _)| *n == c) {
            let _ = writeln!(md, "Not run: {why}.");
            continue;
        }
        if list.is_empty() {
            let _ = writeln!(md, "No issues.");
            continue;
        }
        for ((_, k), v) in groups.iter().filter(|((gc, _), _)| *gc == c) {
            let _ = writeln!(md, "### `{k}` ({})\n", v.len());
            for &n in v {
                let i = &issues[n];
                let tag = match &i.tagged {
                    Some(r) => format!(" *Skipped (tagged): {r}*"),
                    None => String::new(),
                };
                let _ = writeln!(md, "- **#{}** {}{tag}\n  - Where: {}\n  - Fix: {}\n  - Crop: [{}]({})", n + 1, i.message, location(i), i.fix, crop_paths[n], crop_paths[n]);
            }
            let _ = writeln!(md);
        }
    }
    let l = &scene.limits;
    let _ = writeln!(md, "\n## Limits used\n");
    let _ = writeln!(
        md,
        "Overlap {} px; clear air {} px; float {} px; sink {} px; foot support {:.0} %; framing zone {:.0} % to {:.0} % of the view height from the top (feet), runs from {} s, {} s grace after landing on a new height while the camera catches up; terrain below the player at most {:.0} %; coin arc tolerance {} px; hazard cover at most {:.0} %; popups visible at most {} s (alpha under {} counts as gone); draw joins: gaps up to {} art px, lines from {} art px.",
        fmt_px(l.overlap_px),
        fmt_px(l.air_px),
        fmt_px(l.float_px),
        fmt_px(l.sink_px),
        l.support_frac * 100.0,
        l.zone_top * 100.0,
        l.zone_bottom * 100.0,
        fmt_px(l.camera_min_s),
        fmt_px(l.land_grace_s),
        l.max_terrain * 100.0,
        fmt_px(l.arc_tol_px),
        l.hazard_cover * 100.0,
        fmt_px(l.popup_max_s),
        fmt_px(l.popup_min_alpha),
        fmt_px(l.join_gap_px),
        fmt_px(l.join_line_px)
    );
    if !scene.notes.is_empty() {
        let _ = writeln!(md, "\n## Notes\n");
        for n in &scene.notes {
            let _ = writeln!(md, "- {n}");
        }
    }
    std::fs::write(out.join("report.md"), md).map_err(|e| format!("report.md: {e}"))?;
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_picks_one_per_type_first() {
        let mk = |k: &str, s: u8, a: f32| Issue::new(Check::Overlaps, k, [0.0; 4]).sev(s, a);
        let mut v = vec![mk("a", 3, 1.0), mk("a", 3, 9.0), mk("b", 2, 1.0), mk("c", 5, 1.0)];
        v[3].tagged = Some("ok".into());
        assert_eq!(worst(&v, 2), vec![1, 2]);
    }

    #[test]
    fn writes_reports_and_crops() {
        let s = crate::checks::testkit::ground(8, 6, 4);
        let mut v = vec![Issue::new(Check::Seams, "seam", [16.0, 64.0, 16.0, 16.0]).msg("m".into(), "f".into())];
        v.push(Issue::new(Check::Seams, "seam", [48.0, 64.0, 16.0, 16.0]));
        v[1].tagged = Some("on purpose".into());
        let dir = std::env::temp_dir().join(format!("geocheck2d-report-{}", std::process::id()));
        let sum = write(&dir, &s, None, &v).unwrap();
        assert_eq!(sum, Summary { issues: 2, failing: 1, skipped: 1 });
        let md = std::fs::read_to_string(dir.join("report.md")).unwrap();
        assert!(md.contains("Skipped (tagged): on purpose"), "{md}");
        let js: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
        assert_eq!(js["summary"]["by_type"]["seam"]["count"], 2);
        assert!(dir.join("crops/001-seam.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn smooth_art_scenes_list_the_pixel_check_as_not_run() {
        let mut s = crate::checks::testkit::ground(8, 6, 4);
        s.pixel_art = false;
        // A sub-pixel object would fail check 4 on a pixel-art scene.
        s.objects.push(crate::checks::testkit::obj("blob", crate::scene::Kind::Collectible, [10.5, 10.25, 7.3, 7.3]));
        let v = crate::run_all(&s, None, &[]).unwrap();
        assert!(v.iter().all(|i| i.check != Check::Pixels), "{v:?}");
        let dir = std::env::temp_dir().join(format!("geocheck2d-notrun-{}", std::process::id()));
        write(&dir, &s, None, &v).unwrap();
        let md = std::fs::read_to_string(dir.join("report.md")).unwrap();
        assert!(md.contains("| 4 | Pixel integrity | (not run) |"), "{md}");
        assert!(md.contains("Not run: the scene declares `pixel_art: false`"), "{md}");
        let js: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
        assert_eq!(js["not_run"][0]["check"], "pixels");
        // Draw joins are checked on the art-pixel grid too.
        assert!(md.contains("| 8 | Draw joins | (not run) |"), "{md}");
        assert_eq!(js["not_run"][1]["check"], "joins");
        let _ = std::fs::remove_dir_all(&dir);
        // A pixel-art scene whose frames list no sprites cannot be checked for joins.
        let mut p = crate::checks::testkit::ground(8, 6, 4);
        p.pixel_art = true;
        assert!(crate::not_run(&p).iter().any(|(c, why)| *c == Check::Joins && why.contains("no frame")));
        // The same object on a pixel-art scene is reported.
        s.pixel_art = true;
        assert!(crate::run_all(&s, None, &[]).unwrap().iter().any(|i| i.check == Check::Pixels));
    }
}
