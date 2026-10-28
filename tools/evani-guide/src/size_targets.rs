//! Per-tier download targets (S7-1): `catalog/tiers.json` `download_targets`
//! (the code ceiling by kind of game) and each tier's `download_targets`
//! (boot, total, texture memory). Build-time tool data: `check` validates
//! them and warns when a game's last ship build is over its code ceiling;
//! `cargo xtask publish` prints PASS or FAIL per tier with the margin. They
//! are never compiled into a game (`assets/tiers/*.json` stay runtime-only).

use std::path::Path;

use serde_json::Value;

use crate::stream::TIERS;

/// The table Chris approved as provisional (STAGES.md Stage 4, S4-3,
/// 2026-10-20): `(tier, boot KB, total KB, texture memory MB)`, gzip, 1 KB =
/// 1024 bytes. The catalog must say the same (`targets_match_the_approved_table`).
pub const APPROVED_TIERS: [(&str, u64, u64, u64); 6] = [
    ("lite", 100, 2560, 48),
    ("low", 200, 3584, 72),
    ("medium", 300, 5120, 144),
    ("high", 400, 6656, 384),
    ("ultra", 500, 7168, 1152),
    ("vr", 300, 5120, 192),
];
/// Code ceilings (wasm + js + html, gzip KB): 2D games, 3D games.
pub const APPROVED_CODE_KB: (u64, u64) = (350, 500);

/// One tier's targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierTarget {
    pub tier: String,
    pub boot_kb: u64,
    pub total_kb: u64,
    pub texture_memory_mb: u64,
}

/// Everything the catalog sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Targets {
    pub code_kb_2d: u64,
    pub code_kb_3d: u64,
    pub status: String,
    pub tiers: Vec<TierTarget>,
}

/// The two kinds of game the code ceiling tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    TwoD,
    ThreeD,
}

impl Kind {
    /// `3d` for a look with the 3d style or a game with `models`, else `2d`
    /// (pseudo-3D games draw without `forward`, so they count as 2D).
    pub fn of(look_style: Option<&str>, models: bool) -> Self {
        if models || look_style == Some("3d") {
            Kind::ThreeD
        } else {
            Kind::TwoD
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::TwoD => "2d",
            Kind::ThreeD => "3d",
        }
    }
}

impl Targets {
    pub fn code_kb(&self, kind: Kind) -> u64 {
        match kind {
            Kind::TwoD => self.code_kb_2d,
            Kind::ThreeD => self.code_kb_3d,
        }
    }

    pub fn tier(&self, id: &str) -> Option<&TierTarget> {
        self.tiers.iter().find(|t| t.tier == id)
    }
}

fn num(v: &Value, k: &str, at: &str) -> Result<u64, String> {
    v.get(k)
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("catalog/tiers.json {at}: `{k}` must be a whole number above 0"))
}

/// Parse the catalog text. `Ok(None)` when it sets no targets at all.
pub fn parse(text: &str) -> Result<Option<Targets>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("catalog/tiers.json: {e}"))?;
    let Some(top) = v.get("download_targets") else {
        return Ok(None);
    };
    let code = top
        .get("code_kb")
        .ok_or("catalog/tiers.json download_targets: no `code_kb` (`2d` and `3d` ceilings)")?;
    let mut tiers = Vec::new();
    for t in v
        .get("tiers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = t
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let at = format!("tier `{id}` download_targets");
        let d = t
            .get("download_targets")
            .ok_or_else(|| format!("catalog/tiers.json: tier `{id}` has no download_targets"))?;
        tiers.push(TierTarget {
            boot_kb: num(d, "boot_kb", &at)?,
            total_kb: num(d, "total_kb", &at)?,
            texture_memory_mb: num(d, "texture_memory_mb", &at)?,
            tier: id,
        });
    }
    Ok(Some(Targets {
        code_kb_2d: num(code, "2d", "download_targets.code_kb")?,
        code_kb_3d: num(code, "3d", "download_targets.code_kb")?,
        status: top
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        tiers,
    }))
}

/// Read `catalog/tiers.json` under `root`.
pub fn load(root: &Path) -> Result<Option<Targets>, String> {
    let p = root.join("catalog").join("tiers.json");
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    parse(&text)
}

/// `"64 MB"` / `"1.5 GB (streamed)"` -> MB.
fn ceiling_mb(s: &str) -> Option<u64> {
    let mut w = s.split_whitespace();
    let n: f64 = w.next()?.parse().ok()?;
    match w.next()?.trim_end_matches(',') {
        "MB" => Some(n as u64),
        "GB" => Some((n * 1024.0) as u64),
        _ => None,
    }
}

/// What is wrong with the catalog's targets (empty when they are sound):
/// every tier has one, boot sits under total, total and code fit under the
/// 10 MB package cap, and texture memory under the tier's ceiling.
pub fn problems(text: &str) -> Vec<String> {
    let t = match parse(text) {
        Ok(Some(t)) => t,
        Ok(None) => return vec![
            "catalog/tiers.json sets no `download_targets` (STAGES.md Stage 4 S4-3 approved them)"
                .to_string(),
        ],
        Err(e) => return vec![e],
    };
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let mut out = Vec::new();
    if t.code_kb_2d > t.code_kb_3d {
        out.push(format!(
            "the 2d code ceiling ({} KB) is above the 3d one ({} KB)",
            t.code_kb_2d, t.code_kb_3d
        ));
    }
    for id in TIERS {
        let Some(tt) = t.tier(id) else {
            out.push(format!("tier `{id}` has no download_targets"));
            continue;
        };
        if tt.boot_kb >= tt.total_kb {
            out.push(format!(
                "tier `{id}`: boot {} KB is not under its total {} KB",
                tt.boot_kb, tt.total_kb
            ));
        }
        if tt.total_kb * 1024 > crate::playground::PACKAGE_CAP_BYTES {
            out.push(format!(
                "tier `{id}`: total {} KB is over the 10 MB package cap",
                tt.total_kb
            ));
        }
        if t.code_kb_3d >= tt.total_kb {
            out.push(format!(
                "tier `{id}`: total {} KB leaves no room past the 3d code ceiling {} KB",
                tt.total_kb, t.code_kb_3d
            ));
        }
        let ceiling = v["tiers"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|x| x["id"] == id)
            .and_then(|x| x["max_texture_memory"].as_str())
            .and_then(ceiling_mb);
        if let Some(c) = ceiling
            && tt.texture_memory_mb > c
        {
            out.push(format!(
                "tier `{id}`: texture memory target {} MB is over the tier's ceiling {c} MB",
                tt.texture_memory_mb
            ));
        }
    }
    out
}

/// One tier judged against its targets. Sizes in bytes (gzip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierVerdict {
    pub tier: String,
    pub code: u64,
    pub code_target: u64,
    pub boot: u64,
    pub boot_target: u64,
    pub total: u64,
    pub total_target: u64,
}

impl TierVerdict {
    pub fn pass(&self) -> bool {
        self.code <= self.code_target
            && self.boot <= self.boot_target
            && self.total <= self.total_target
    }

    /// Bytes left under the tightest of the three (negative when over).
    pub fn margin(&self) -> i64 {
        [
            (self.code, self.code_target),
            (self.boot, self.boot_target),
            (self.total, self.total_target),
        ]
        .iter()
        .map(|(have, max)| *max as i64 - *have as i64)
        .min()
        .unwrap_or(0)
    }

    /// Which targets it is over, by name.
    pub fn over(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.code > self.code_target {
            v.push("code");
        }
        if self.boot > self.boot_target {
            v.push("boot");
        }
        if self.total > self.total_target {
            v.push("total");
        }
        v
    }
}

/// Judge one tier: `code` (wasm + js + html gzip), `boot` (the boot files the
/// tier fetches, gzip) and `streamed` (every streamed file it ever fetches,
/// boot included, gzip). The total is code + streamed.
pub fn judge(
    t: &Targets,
    kind: Kind,
    tier: &str,
    code: u64,
    boot: u64,
    streamed: u64,
) -> Option<TierVerdict> {
    let tt = t.tier(tier)?;
    Some(TierVerdict {
        tier: tier.to_string(),
        code,
        code_target: t.code_kb(kind) * 1024,
        boot,
        boot_target: tt.boot_kb * 1024,
        total: code + streamed,
        total_target: tt.total_kb * 1024,
    })
}

/// `PASS low: code 376.6/500.0 KB, boot 0.0/200.0 KB, total 380.1/3584.0 KB, margin +119.9 KB`.
pub fn verdict_line(v: &TierVerdict) -> String {
    let k = |b: u64| format!("{:.1}", b as f64 / 1024.0);
    let over = v.over();
    format!(
        "{} {}: code {}/{} KB, boot {}/{} KB, total {}/{} KB, margin {:+.1} KB{}",
        if v.pass() { "PASS" } else { "FAIL" },
        v.tier,
        k(v.code),
        k(v.code_target),
        k(v.boot),
        k(v.boot_target),
        k(v.total),
        k(v.total_target),
        v.margin() as f64 / 1024.0,
        if over.is_empty() {
            String::new()
        } else {
            format!(" (over: {})", over.join(", "))
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../catalog/tiers.json"
        ))
        .unwrap()
    }

    #[test]
    fn targets_match_the_approved_table() {
        let t = parse(&catalog())
            .unwrap()
            .expect("catalog/tiers.json sets download_targets");
        assert_eq!((t.code_kb_2d, t.code_kb_3d), APPROVED_CODE_KB);
        assert_eq!(t.tiers.len(), APPROVED_TIERS.len());
        for (id, boot, total, tex) in APPROVED_TIERS {
            let tt = t.tier(id).unwrap_or_else(|| panic!("tier {id}"));
            assert_eq!(
                (tt.boot_kb, tt.total_kb, tt.texture_memory_mb),
                (boot, total, tex),
                "{id}"
            );
        }
        assert!(t.status.starts_with("provisional"), "{}", t.status);
        assert_eq!(problems(&catalog()), Vec::<String>::new());
    }

    #[test]
    fn bad_targets_are_named() {
        let text = catalog().replacen("\"boot_kb\": 100", "\"boot_kb\": 9000", 1);
        let p = problems(&text);
        assert!(
            p.iter()
                .any(|x| x.contains("lite") && x.contains("not under its total")),
            "{p:?}"
        );
        let none = r#"{ "tiers": [] }"#;
        assert!(problems(none)[0].contains("sets no `download_targets`"));
    }

    #[test]
    fn a_tier_over_its_target_fails_with_the_margin() {
        let t = parse(&catalog()).unwrap().unwrap();
        let ok = judge(&t, Kind::ThreeD, "low", 400 * 1024, 0, 100 * 1024).unwrap();
        assert!(ok.pass());
        assert_eq!(
            ok.margin(),
            100 * 1024,
            "code is the tightest: 500 - 400 KB"
        );
        let over = judge(&t, Kind::TwoD, "lite", 360 * 1024, 120 * 1024, 2300 * 1024).unwrap();
        assert!(!over.pass());
        assert_eq!(over.over(), ["code", "boot", "total"]);
        assert!(
            verdict_line(&over).starts_with("FAIL lite:")
                && verdict_line(&over).contains("margin -")
        );
        assert_eq!(Kind::of(Some("2d"), false), Kind::TwoD);
        assert_eq!(Kind::of(Some("pseudo-3d"), false), Kind::TwoD);
        assert_eq!(Kind::of(Some("2d"), true), Kind::ThreeD);
        assert_eq!(Kind::of(Some("3d"), false), Kind::ThreeD);
    }
}
