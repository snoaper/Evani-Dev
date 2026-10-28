//! Per-tier download sizes of a package against the catalog's targets
//! (S7-1), shared by `cargo xtask publish` (PASS or FAIL per tier with the
//! margin, in the size report and summary.json) and `cargo xtask bytes`
//! (S7-2, the ledger of where every byte went).
//!
//! A tier's download is the code (index.html and the files it loads, the
//! same on every tier) plus the streamed files that tier fetches
//! ([`evani_guide::tier_fetch::plan`]). Players get one texture family, so
//! each tier is judged on its heaviest family (ASTC, BC7 or the PNG/JPEG
//! fallback): a pass holds for every device.

use std::path::Path;

use serde_json::{Value, json};

use evani_guide::size_targets::{self, Kind, Targets, TierVerdict};
use evani_guide::tier_fetch::{self, FAMILIES};

use crate::publish::PackageManifest;

/// One streamed file a tier fetches, with its sizes from the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFile {
    /// Under `models/`.
    pub name: String,
    pub group: String,
    pub why: String,
    pub raw: u64,
    pub gzip: u64,
}

/// One tier with one family: the files and their gzip totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyFetch {
    pub family: String,
    pub files: Vec<StreamFile>,
    pub boot_gzip: u64,
    pub streamed_gzip: u64,
}

/// One tier: every family's fetch and the verdict on the heaviest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierRow {
    pub tier: String,
    pub families: Vec<FamilyFetch>,
    /// The heaviest family, the one judged.
    pub worst: String,
    pub verdict: Option<TierVerdict>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TierReport {
    pub kind: Option<Kind>,
    pub targets: Option<Targets>,
    /// Code gzip (index.html and the files it loads).
    pub code_gzip: u64,
    pub rows: Vec<TierRow>,
    /// Why no verdicts were made, or what could not be read.
    pub notes: Vec<String>,
}

/// The streamed files of `pkg` (`models/stream.json`) for every tier and
/// family. `files` gives the package's sizes by path.
pub fn fetches(
    pkg: &Path,
    files: &PackageManifest,
    tier: &str,
) -> Result<Vec<FamilyFetch>, String> {
    let models = pkg.join("models");
    let text = match std::fs::read_to_string(models.join("stream.json")) {
        Ok(t) => t,
        Err(_) => {
            return Ok(FAMILIES
                .iter()
                .map(|f| FamilyFetch {
                    family: (*f).to_string(),
                    files: Vec::new(),
                    boot_gzip: 0,
                    streamed_gzip: 0,
                })
                .collect());
        }
    };
    let all = tier_fetch::entries(&text)?;
    let mut out = Vec::new();
    for family in FAMILIES {
        let mut read = |name: &str| {
            std::fs::read(models.join(name))
                .ok()
                .and_then(|b| tier_fetch::gltf_of(name, &b))
        };
        let (plan, unread) = tier_fetch::plan(&all, tier, family, &mut read);
        if let Some(u) = unread.first() {
            return Err(format!(
                "models/{u}: the model can't be read, so its textures can't be counted"
            ));
        }
        let mut ff = FamilyFetch {
            family: family.to_string(),
            files: Vec::new(),
            boot_gzip: 0,
            streamed_gzip: 0,
        };
        for f in plan {
            let path = format!("models/{}", f.name);
            let e = files.files.get(&path).ok_or_else(|| {
                format!("{path}: the stream lists it but the package has no such file")
            })?;
            if f.group == "boot" {
                ff.boot_gzip += e.gzip;
            }
            ff.streamed_gzip += e.gzip;
            ff.files.push(StreamFile {
                name: f.name,
                group: f.group,
                why: f.why,
                raw: e.size,
                gzip: e.gzip,
            });
        }
        out.push(ff);
    }
    Ok(out)
}

/// The report for a package. `look_style` and `models` decide the kind of
/// game (the code ceiling); `code_gzip` is the page download.
pub fn report(
    root: &Path,
    look_style: Option<&str>,
    models: bool,
    pkg: &Path,
    files: &PackageManifest,
    code_gzip: u64,
) -> TierReport {
    let mut r = TierReport {
        code_gzip,
        ..TierReport::default()
    };
    let targets = match size_targets::load(root) {
        Ok(Some(t)) => Some(t),
        Ok(None) => {
            r.notes
                .push("catalog/tiers.json sets no download_targets, so none are checked".into());
            None
        }
        Err(e) => {
            r.notes.push(format!("the targets can't be read: {e}"));
            None
        }
    };
    let kind = Kind::of(look_style, models);
    r.kind = Some(kind);
    for tier in evani_guide::stream::TIERS {
        let fams = match fetches(pkg, files, tier) {
            Ok(f) => f,
            Err(e) => {
                r.notes.push(format!("{tier}: {e}"));
                continue;
            }
        };
        let worst = fams
            .iter()
            .max_by_key(|f| (f.streamed_gzip, f.boot_gzip))
            .map(|f| f.family.clone())
            .unwrap_or_default();
        let verdict = targets.as_ref().and_then(|t| {
            let w = fams.iter().find(|f| f.family == worst)?;
            size_targets::judge(t, kind, tier, code_gzip, w.boot_gzip, w.streamed_gzip)
        });
        r.rows.push(TierRow {
            tier: tier.to_string(),
            families: fams,
            worst,
            verdict,
        });
    }
    r.targets = targets;
    r
}

impl TierReport {
    pub fn failed(&self) -> Vec<&TierVerdict> {
        self.rows
            .iter()
            .filter_map(|r| r.verdict.as_ref())
            .filter(|v| !v.pass())
            .collect()
    }

    /// The size report's lines.
    pub fn lines(&self) -> Vec<String> {
        let mut l = Vec::new();
        match (&self.targets, self.kind) {
            (Some(t), Some(k)) => {
                l.push(format!(
                    "  per-tier download targets (catalog/tiers.json, {}; gzip; {} game, code ceiling {} KB; each tier on its heaviest texture family):",
                    t.status.split(';').next().unwrap_or("set"),
                    k.label(),
                    t.code_kb(k)
                ));
                for r in &self.rows {
                    if let Some(v) = &r.verdict {
                        l.push(format!(
                            "    {} [{}]",
                            size_targets::verdict_line(v),
                            r.worst
                        ));
                    }
                }
            }
            _ => l.push("  per-tier download targets: not checked".to_string()),
        }
        for n in &self.notes {
            l.push(format!("    note: {n}"));
        }
        l
    }

    pub fn to_json(&self) -> Value {
        let tiers: Vec<Value> = self
            .rows
            .iter()
            .filter_map(|r| {
                let v = r.verdict.as_ref()?;
                Some(json!({
                    "tier": r.tier, "family": r.worst, "pass": v.pass(), "margin": v.margin(), "over": v.over(),
                    "code": v.code, "code_target": v.code_target, "boot": v.boot, "boot_target": v.boot_target,
                    "total": v.total, "total_target": v.total_target, "line": size_targets::verdict_line(v),
                }))
            })
            .collect();
        json!({
            "kind": self.kind.map(Kind::label),
            "status": self.targets.as_ref().map(|t| t.status.clone()),
            "basis": "gzip -9 of index.html, the files it loads and the streamed files the tier fetches, on its heaviest texture family; 1 KB = 1024 bytes",
            "passed": self.targets.is_some() && self.failed().is_empty(),
            "tiers": tiers,
            "notes": self.notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_flags_a_tier_over_its_target() {
        let pkg = std::env::temp_dir().join(format!("evani-tier-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pkg);
        std::fs::create_dir_all(pkg.join("models")).unwrap();
        // 3 MB that gzip can't shrink: over Lite's 2.5 MB total, under Low's 3.5 MB.
        let mut x: u32 = 1;
        let noise: Vec<u8> = (0..3 * 1024 * 1024)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) as u8
            })
            .collect();
        std::fs::write(pkg.join("models/level.bin"), &noise).unwrap();
        std::fs::write(pkg.join("index.html"), "<html></html>").unwrap();
        let stream = r#"{ "version": 2, "assets": [{ "name": "level.bin", "hash": "0", "size": 3145728, "url": "level.bin", "kind": "data", "priority": 0, "group": "later" }] }"#;
        std::fs::write(pkg.join("models/stream.json"), stream).unwrap();
        let files = PackageManifest::scan(&pkg).unwrap();
        let r = report(
            &crate::workspace_root(),
            Some("2d"),
            false,
            &pkg,
            &files,
            100 * 1024,
        );
        let row = |t: &str| {
            r.rows
                .iter()
                .find(|x| x.tier == t)
                .unwrap()
                .verdict
                .clone()
                .unwrap()
        };
        assert!(!row("lite").pass(), "{:?}", row("lite"));
        assert_eq!(row("lite").over(), ["total"]);
        assert!(row("lite").margin() < 0);
        assert!(row("low").pass(), "{:?}", row("low"));
        assert_eq!(r.failed().len(), 1);
        let text = r.lines().join("\n");
        assert!(
            text.contains("FAIL lite:") && text.contains("PASS low:"),
            "{text}"
        );
        assert_eq!(r.to_json()["tiers"][0]["pass"], false);
        let _ = std::fs::remove_dir_all(&pkg);
    }
}
