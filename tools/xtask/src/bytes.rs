//! `cargo xtask bytes <id> [--tier low,ultra]`: the byte ledger (S7-2), where
//! every byte a player's browser fetches went, one table per tier. It joins:
//!
//! * the code (index.html and the files it loads) by file, with the wasm's
//!   split by module from the last `cargo xtask publish` summary
//!   (`.evani/publish/<id>/summary.json` `sizes.modules`, shown as "of which"
//!   lines that are not added again);
//! * the streamed files the tier fetches with one texture family
//!   ([`crate::tier_report::fetches`]: `stream.json` plus each model's
//!   texture sets, the engine's own rules), by group and reason;
//! * what the browser really fetched (`cargo xtask netlog`'s
//!   `netlog-<tier>.json`): its byte total, and any file it fetched that the
//!   ledger has no row for (named, never lumped into "other").
//!
//! It measures the ship build netlog serves (`target/evani-sizes/<id>-ship`),
//! or a package folder with `--from`. Writes `.evani/publish/<id>/bytes.md`
//! and `bytes.json`. Each row also carries the model's on-screen importance
//! when `apps/<id>/assets/importance.json` exists (S7-3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use evani_guide::size_targets::{self, Kind};
use evani_guide::tier_fetch::{self, FAMILIES};

use crate::publish::{self, PackageManifest};
use crate::tier_report;

pub fn usage() -> &'static str {
    "  cargo xtask bytes <app> [--tier low,ultra] [--family astc|bc7|rgba] [--from <package dir>] [--netlog <dir>]   (the byte ledger: .evani/publish/<app>/bytes.md and .json)"
}

/// One ledger row: a file the tier fetches.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Path in the package (`models/hero.glb`).
    pub path: String,
    /// `code`, `manifest`, `boot`, `streamed` or `netlog only`.
    pub section: String,
    /// Why it is fetched (`page`, `model`, `texture of hero.glb`...).
    pub why: String,
    pub raw: u64,
    pub gzip: u64,
    /// The largest on-screen share of the model this file belongs to (S7-3).
    pub importance: Option<f64>,
}

/// One tier's ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct Ledger {
    pub tier: String,
    pub family: String,
    /// How the family was chosen: `--family`, `netlog` or `heaviest`.
    pub family_from: String,
    pub rows: Vec<Row>,
    /// Module split of the code (from the publish summary), not added again.
    pub modules: Vec<(String, i64)>,
    /// What the browser really fetched (bytes received, the local server
    /// sends files as they are), when a netlog run exists.
    pub netlog_bytes: Option<u64>,
    pub netlog_files: usize,
    /// Planned paths the netlog run did not fetch (still in the totals).
    pub unfetched: Vec<String>,
    pub verdict: Option<size_targets::TierVerdict>,
}

/// Marks a planned row the netlog run did not fetch.
pub const NOT_FETCHED: &str = "planned, not fetched in this run";

impl Ledger {
    pub fn raw(&self) -> u64 {
        self.rows.iter().map(|r| r.raw).sum()
    }

    /// Raw bytes of the rows a run could have seen: all of them, less the
    /// planned rows it did not fetch.
    pub fn raw_fetched(&self) -> u64 {
        self.rows
            .iter()
            .filter(|r| !self.unfetched.contains(&r.path))
            .map(|r| r.raw)
            .sum()
    }

    pub fn gzip(&self) -> u64 {
        self.rows.iter().map(|r| r.gzip).sum()
    }

    /// Bytes no named row explains: rows the netlog fetched that the ledger
    /// did not plan are named rows too, so this is what is left after both.
    pub fn other(&self) -> i64 {
        self.netlog_bytes
            .map_or(0, |n| n as i64 - self.raw_fetched() as i64)
    }

    /// Ledger raw total against the netlog total, in percent.
    pub fn off_by_percent(&self) -> Option<f64> {
        let n = self.netlog_bytes?;
        (n > 0).then(|| (self.raw_fetched() as f64 - n as f64).abs() * 100.0 / n as f64)
    }
}

/// One request the netlog recorded: the path under the site and the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub path: String,
    pub bytes: Option<u64>,
}

/// The fetched files of one `netlog-<label>.json`: finished (200/206) requests
/// to the served folder, their path without the origin, query or fragment.
pub fn netlog_fetched(v: &Value) -> Vec<Fetched> {
    v.get("requests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| matches!(r.get("status").and_then(Value::as_u64), Some(200 | 206)))
        .filter_map(|r| {
            let url = r.get("url")?.as_str()?;
            let no_q = url.split(['?', '#']).next().unwrap_or(url);
            let path = match no_q.split_once("://") {
                Some((_, rest)) => rest.split_once('/').map_or("", |(_, p)| p),
                None => no_q.trim_start_matches('/'),
            };
            let path = if path.is_empty() || path.ends_with('/') {
                format!("{path}index.html")
            } else {
                path.to_string()
            };
            Some(Fetched {
                path,
                bytes: r.get("bytes").and_then(Value::as_u64),
            })
        })
        .collect()
}

/// The texture family a netlog run used, by the texture files it fetched.
pub fn family_seen(fetched: &[Fetched]) -> Option<&'static str> {
    let fams: Vec<&str> = fetched
        .iter()
        .filter_map(|f| tier_fetch::family_of(&f.path))
        .collect();
    ["astc", "bc7"]
        .into_iter()
        .find(|x| fams.contains(x))
        .or_else(|| (!fams.is_empty()).then_some("rgba"))
}

/// Importance per model id from `importance.json` (S7-3): the largest
/// screen share over every tier and moment.
pub fn importance_by_model(v: &Value) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for m in v
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(id), Some(s)) = (
            m.get("id").and_then(Value::as_str),
            m.get("max_screen_share").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let e = out.entry(id.to_string()).or_insert(0.0f64);
        *e = e.max(s);
    }
    out
}

/// The model id a streamed file belongs to: `hero.low.base.jpg` -> `hero`.
fn model_id_of(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.split('.').next().unwrap_or(file)
}

/// Inputs of one ledger, already read.
pub struct Inputs<'a> {
    pub pkg: &'a Path,
    pub files: &'a PackageManifest,
    /// index.html and the files it loads.
    pub page: &'a [String],
    pub modules: &'a [(String, i64)],
    pub kind: Kind,
    pub targets: Option<&'a size_targets::Targets>,
    pub importance: &'a BTreeMap<String, f64>,
}

/// Build one tier's ledger. `netlog` is that tier's fetched files, if a run
/// exists; `family` forces the texture family.
pub fn ledger(
    inp: &Inputs,
    tier: &str,
    family: Option<&str>,
    netlog: Option<&[Fetched]>,
) -> Result<Ledger, String> {
    let fams = tier_report::fetches(inp.pkg, inp.files, tier)?;
    let (family, family_from) = match (family, netlog.and_then(family_seen)) {
        (Some(f), _) => (f.to_string(), "--family".to_string()),
        (None, Some(f)) => (f.to_string(), "netlog".to_string()),
        (None, None) => {
            let worst = fams
                .iter()
                .max_by_key(|f| (f.streamed_gzip, f.boot_gzip))
                .map_or("rgba".to_string(), |f| f.family.clone());
            (worst, "heaviest".to_string())
        }
    };
    let ff = fams.iter().find(|f| f.family == family).ok_or_else(|| {
        format!(
            "unknown texture family {family} (one of {})",
            FAMILIES.join(", ")
        )
    })?;
    let mut rows = Vec::new();
    let entry = |p: &str| inp.files.files.get(p);
    for p in inp.page {
        if let Some(e) = entry(p) {
            rows.push(Row {
                path: p.clone(),
                section: "code".into(),
                why: "page".into(),
                raw: e.size,
                gzip: e.gzip,
                importance: None,
            });
        }
    }
    if let Some(e) = entry("models/stream.json") {
        rows.push(Row {
            path: "models/stream.json".into(),
            section: "manifest".into(),
            why: "the stream list".into(),
            raw: e.size,
            gzip: e.gzip,
            importance: None,
        });
    }
    for f in &ff.files {
        let section = if f.group == "boot" {
            "boot"
        } else {
            "streamed"
        };
        rows.push(Row {
            path: format!("models/{}", f.name),
            section: section.into(),
            why: format!("{} ({})", f.why, f.group),
            raw: f.raw,
            gzip: f.gzip,
            importance: inp.importance.get(model_id_of(&f.name)).copied(),
        });
    }
    let (mut netlog_bytes, mut netlog_files) = (None, 0);
    let mut unfetched: Vec<String> = Vec::new();
    if let Some(n) = netlog {
        let mut total = 0u64;
        let mut seen: Vec<&str> = Vec::new();
        for f in n {
            if seen.contains(&f.path.as_str()) {
                // A second fetch of the same file (the server forbids caching):
                // count it once more on both sides.
                if let Some(r) = rows.iter().find(|r| r.path == f.path).cloned() {
                    rows.push(Row {
                        why: format!("{} (fetched again)", r.why),
                        ..r
                    });
                }
            }
            seen.push(&f.path);
            let size = f
                .bytes
                .or_else(|| entry(&f.path).map(|e| e.size))
                .unwrap_or(0);
            total += size;
            if !rows.iter().any(|r| r.path == f.path) {
                let (raw, gzip) = entry(&f.path).map_or((size, size), |e| (e.size, e.gzip));
                rows.push(Row {
                    path: f.path.clone(),
                    section: "netlog only".into(),
                    why: "fetched, but no plan names it".into(),
                    raw,
                    gzip,
                    importance: None,
                });
            }
        }
        netlog_bytes = Some(total);
        netlog_files = n.len();
        // A page draws a model only when it needs it, so a short run may not
        // fetch every planned file: name those rows, keep them in the totals.
        for r in rows.iter_mut() {
            if !n.iter().any(|f| f.path == r.path) {
                r.why = format!("{} - {NOT_FETCHED}", r.why);
                unfetched.push(r.path.clone());
            }
        }
    }
    let code: u64 = rows
        .iter()
        .filter(|r| r.section == "code")
        .map(|r| r.gzip)
        .sum();
    let boot: u64 = rows
        .iter()
        .filter(|r| r.section == "boot")
        .map(|r| r.gzip)
        .sum();
    // Everything past the code: the stream list, boot and later files, and
    // any file only the netlog saw.
    let streamed: u64 = rows
        .iter()
        .filter(|r| r.section != "code")
        .map(|r| r.gzip)
        .sum();
    let verdict = inp
        .targets
        .and_then(|t| size_targets::judge(t, inp.kind, tier, code, boot, streamed));
    Ok(Ledger {
        tier: tier.to_string(),
        family,
        family_from,
        rows,
        modules: inp.modules.to_vec(),
        netlog_bytes,
        netlog_files,
        unfetched,
        verdict,
    })
}

fn kb(b: u64) -> String {
    format!("{:.1} KB", b as f64 / 1024.0)
}

fn skb(b: i64) -> String {
    format!("{:+.1} KB", b as f64 / 1024.0)
}

/// The Markdown of every tier's ledger.
pub fn markdown(app: &str, source: &Path, ledgers: &[Ledger]) -> String {
    let mut s = format!(
        "# Byte ledger: {app}\n\nSource: `{}`. Sizes per file: raw (what the local server sends) and gzip -9 (what the size targets count); 1 KB = 1024 bytes.\n",
        source.display()
    );
    for l in ledgers {
        s.push_str(&format!(
            "\n## {} ({} textures, from {})\n\n",
            l.tier, l.family, l.family_from
        ));
        s.push_str(
            "| file | section | why | raw | gzip | importance |\n|---|---|---|---:|---:|---:|\n",
        );
        let mut rows: Vec<&Row> = l.rows.iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.gzip));
        for r in rows {
            let imp = r
                .importance
                .map_or("".to_string(), |i| format!("{:.1} %", i * 100.0));
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} | {imp} |\n",
                r.path,
                r.section,
                r.why,
                kb(r.raw),
                kb(r.gzip)
            ));
        }
        s.push_str(&format!(
            "| **total** | | {} files | **{}** | **{}** | |\n",
            l.rows.len(),
            kb(l.raw()),
            kb(l.gzip())
        ));
        if !l.modules.is_empty() {
            s.push_str("\nOf which, in the code (gzip, full ship build minus a build with that module off; from the last publish summary, not added again):\n\n");
            for (m, g) in &l.modules {
                s.push_str(&format!(
                    "- {m}: {}\n",
                    if m.starts_with("base") {
                        kb((*g).max(0) as u64)
                    } else {
                        skb(*g)
                    }
                ));
            }
        }
        match (l.netlog_bytes, l.off_by_percent()) {
            (Some(n), Some(p)) => s.push_str(&format!(
                "\nNetlog: {} requests, {} received; the ledger's raw total is {:.2} % off it ({}); unexplained bytes {}.\n",
                l.netlog_files,
                kb(n),
                p,
                if p <= 1.0 { "within 1 %" } else { "OVER 1 %" },
                skb(l.other())
            )),
            _ => s.push_str(&format!("\nNetlog: none for this tier (run `cargo xtask netlog {app} --tier {}` to compare with what a browser fetches).\n", l.tier)),
        }
        if let Some(v) = &l.verdict {
            s.push_str(&format!("\nTarget: {}\n", size_targets::verdict_line(v)));
        }
    }
    s
}

pub fn to_json(app: &str, source: &Path, ledgers: &[Ledger]) -> Value {
    json!({
        "game": app,
        "source": source.display().to_string(),
        "basis": "raw bytes per file and gzip -9; netlog bytes are what the browser received from the local server",
        "tiers": ledgers.iter().map(|l| json!({
            "tier": l.tier, "family": l.family, "family_from": l.family_from,
            "raw": l.raw(), "gzip": l.gzip(),
            "netlog_bytes": l.netlog_bytes, "netlog_requests": l.netlog_files,
            "off_by_percent": l.off_by_percent(), "other": l.other(),
            "target": l.verdict.as_ref().map(|v| json!({"pass": v.pass(), "margin": v.margin(), "line": size_targets::verdict_line(v)})),
            "modules": l.modules.iter().map(|(m, g)| json!({"module": m, "gzip": g})).collect::<Vec<_>>(),
            "rows": l.rows.iter().map(|r| json!({"path": r.path, "section": r.section, "why": r.why, "raw": r.raw, "gzip": r.gzip, "importance": r.importance})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// The module split from the publish summary (gzip), empty when none.
fn summary_modules(root: &Path, app: &str) -> Vec<(String, i64)> {
    let p = publish::history_dir(root, app).join("summary.json");
    let Some(v) = std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return Vec::new();
    };
    v.pointer("/sizes/modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                m.get("module")?.as_str()?.to_string(),
                m.get("gzip")?.as_i64()?,
            ))
        })
        .collect()
}

pub fn cmd_bytes(args: &[String]) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i].starts_with("--") {
            i += 2;
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }
    let app = positional
        .first()
        .ok_or_else(|| format!("usage:\n{}", usage()))?
        .clone();
    let root = crate::workspace_root();
    let tiers: Vec<String> = opt(args, "--tier").map_or_else(
        || {
            evani_guide::stream::TIERS
                .iter()
                .map(|s| s.to_string())
                .collect()
        },
        |t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        },
    );
    for t in &tiers {
        if !evani_guide::stream::TIERS.contains(&t.as_str()) {
            return Err(format!(
                "bytes: unknown tier {t} (one of {})",
                evani_guide::stream::TIERS.join(", ")
            ));
        }
    }
    let family = opt(args, "--family");
    if let Some(f) = family
        && !FAMILIES.contains(&f)
    {
        return Err(format!(
            "bytes: unknown texture family {f} (one of {})",
            FAMILIES.join(", ")
        ));
    }
    let source: PathBuf = opt(args, "--from").map_or_else(
        || {
            root.join("target")
                .join("evani-sizes")
                .join(format!("{app}-ship"))
        },
        |p| root.join(p),
    );
    if !source.join("index.html").is_file() {
        return Err(format!(
            "bytes: {} has no index.html\n  fix: run `cargo xtask build {app} --ship` first (or pass --from <package folder>)",
            source.display()
        ));
    }
    let toml = std::fs::read_to_string(root.join("apps").join(&app).join("evani.toml"))
        .map_err(|e| format!("bytes: apps/{app}/evani.toml: {e}"))?;
    let m = crate::Manifest::parse(&toml)?;
    let kind = Kind::of(
        evani_guide::render_paths::style_of_look(&root, &m.look).as_deref(),
        m.models,
    );
    let files = PackageManifest::scan(&source)?;
    let page = publish::page_files(
        &files,
        &std::fs::read_to_string(source.join("index.html")).unwrap_or_default(),
    );
    let modules = summary_modules(&root, &app);
    let targets = size_targets::load(&root).ok().flatten();
    let importance = std::fs::read_to_string(
        root.join("apps")
            .join(&app)
            .join("assets")
            .join("importance.json"),
    )
    .ok()
    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    .map(|v| importance_by_model(&v))
    .unwrap_or_default();
    let inp = Inputs {
        pkg: &source,
        files: &files,
        page: &page,
        modules: &modules,
        kind,
        targets: targets.as_ref(),
        importance: &importance,
    };
    let netdir = opt(args, "--netlog").map_or_else(
        || root.join(format!(".evani-shot-netlog-{app}")),
        |p| root.join(p),
    );
    let mut ledgers = Vec::new();
    for t in &tiers {
        let net = std::fs::read_to_string(netdir.join(format!("netlog-{t}.json")))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|v| netlog_fetched(&v));
        let l = ledger(&inp, t, family, net.as_deref())?;
        println!(
            "bytes {}: {} files, {} raw, {} gzip ({} textures){}",
            l.tier,
            l.rows.len(),
            kb(l.raw()),
            kb(l.gzip()),
            l.family,
            match l.off_by_percent() {
                Some(p) => format!(
                    ", netlog {} ({p:.2} % off)",
                    kb(l.netlog_bytes.unwrap_or(0))
                ),
                None => ", no netlog".to_string(),
            }
        );
        if let Some(v) = &l.verdict {
            println!("  {}", size_targets::verdict_line(v));
        }
        ledgers.push(l);
    }
    let dir = publish::history_dir(&root, &app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("bytes: {}: {e}", dir.display()))?;
    std::fs::write(dir.join("bytes.md"), markdown(&app, &source, &ledgers))
        .map_err(|e| e.to_string())?;
    let j = serde_json::to_string_pretty(&to_json(&app, &source, &ledgers))
        .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("bytes.json"), j).map_err(|e| e.to_string())?;
    println!("bytes: {} (and bytes.json)", dir.join("bytes.md").display());
    let off: Vec<&Ledger> = ledgers
        .iter()
        .filter(|l| l.off_by_percent().is_some_and(|p| p > 1.0))
        .collect();
    if let Some(l) = off.first() {
        return Err(format!(
            "bytes: on {} the ledger is {:.2} % off what the browser fetched (over 1 %)\n  fix: rebuild with `cargo xtask build {app} --ship`, then rerun `cargo xtask netlog {app} --tier {}` so both measure the same build",
            l.tier,
            l.off_by_percent().unwrap_or(0.0),
            l.tier
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// A small package: a page, its code, a stream with one model on two
    /// tiers and its textures in three families.
    fn package() -> PathBuf {
        let pkg = std::env::temp_dir().join(format!("evani-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pkg);
        let html = r#"<html><script type="module">import init from './game.js'; init({ module_or_path: 'game_bg.wasm' });</script></html>"#;
        write(&pkg, "index.html", html.as_bytes());
        write(&pkg, "game.js", &vec![b'j'; 20_000]);
        write(
            &pkg,
            "game_bg.wasm",
            &(0..300_000u32)
                .map(|i| (i * 7 % 251) as u8)
                .collect::<Vec<_>>(),
        );
        write(&pkg, "README.md", b"not fetched");
        let gltf = |img: &str, sets: &str| {
            format!(
                r#"{{ "images": [{{ "uri": "{img}" }}], "textures": [{{ "source": 0 }}], "materials": [{{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }} }}], "extras": {{ "evani": {{ "textures": [{{ "texture": 0, "sets": {sets} }}] }} }} }}"#
            )
        };
        write(
            &pkg,
            "models/hero.gltf",
            gltf(
                "hero.base.png",
                r#"{ "astc": "hero.base.astc.ktx2", "bc7": "hero.base.bc7.ktx2" }"#,
            )
            .as_bytes(),
        );
        write(
            &pkg,
            "models/hero.low.gltf",
            gltf("hero.low.base.png", "{}").as_bytes(),
        );
        for (n, len) in [
            ("hero.base.png", 90_000),
            ("hero.base.astc.ktx2", 60_000),
            ("hero.base.bc7.ktx2", 120_000),
            ("hero.low.base.png", 30_000),
        ] {
            write(
                &pkg,
                &format!("models/{n}"),
                &(0..len)
                    .map(|i: u32| (i * 13 % 241) as u8)
                    .collect::<Vec<_>>(),
            );
        }
        let stream = r#"{ "version": 2, "assets": [
            { "name": "hero.gltf", "kind": "model", "group": "later", "size": 1 },
            { "name": "hero.low.gltf", "kind": "model", "group": "later", "size": 1, "tier": "low" },
            { "name": "hero.base.png", "kind": "texture", "group": "later", "size": 1 },
            { "name": "hero.base.astc.ktx2", "kind": "texture", "group": "later", "size": 1 },
            { "name": "hero.base.bc7.ktx2", "kind": "texture", "group": "later", "size": 1 },
            { "name": "hero.low.base.png", "kind": "texture", "group": "later", "size": 1, "tier": "low" }
        ] }"#;
        write(&pkg, "models/stream.json", stream.as_bytes());
        pkg
    }

    /// What a browser on `tier` with `family` received: each file once, its
    /// size plus about 150 bytes of response headers.
    fn netlog(pkg: &Path, files: &[&str]) -> Value {
        let reqs: Vec<Value> = files
            .iter()
            .map(|f| {
                let len = std::fs::metadata(pkg.join(f)).unwrap().len();
                json!({ "url": format!("http://127.0.0.1:8240/{f}?v=1"), "status": 200, "bytes": len + 150 })
            })
            .chain([json!({ "url": "http://127.0.0.1:8240/favicon.ico", "status": 404, "bytes": 120 })])
            .collect();
        json!({ "tier": "x", "requests": reqs })
    }

    #[test]
    fn ledger_rows_sum_to_the_download() {
        let pkg = package();
        let files = PackageManifest::scan(&pkg).unwrap();
        let page = publish::page_files(
            &files,
            &std::fs::read_to_string(pkg.join("index.html")).unwrap(),
        );
        assert_eq!(page, ["game.js", "game_bg.wasm", "index.html"]);
        let modules = vec![
            ("fx".to_string(), 12_000),
            ("base (core+render+kit+look)".to_string(), 80_000),
        ];
        let targets = size_targets::load(&crate::workspace_root()).unwrap();
        let mut imp = BTreeMap::new();
        imp.insert("hero".to_string(), 0.31);
        let inp = Inputs {
            pkg: &pkg,
            files: &files,
            page: &page,
            modules: &modules,
            kind: Kind::ThreeD,
            targets: targets.as_ref(),
            importance: &imp,
        };
        let code = [
            "index.html",
            "game.js",
            "game_bg.wasm",
            "models/stream.json",
        ];
        for (tier, fetched) in [
            (
                "ultra",
                vec!["models/hero.gltf", "models/hero.base.bc7.ktx2"],
            ),
            (
                "low",
                vec!["models/hero.low.gltf", "models/hero.low.base.png"],
            ),
        ] {
            let all: Vec<&str> = code
                .iter()
                .copied()
                .chain(fetched.iter().copied())
                .collect();
            let net = netlog_fetched(&netlog(&pkg, &all));
            assert_eq!(net.len(), all.len(), "the 404 is not a fetched file");
            let l = ledger(&inp, tier, None, Some(&net)).unwrap();
            assert_eq!(l.family_from, "netlog");
            let p = l.off_by_percent().unwrap();
            assert!(
                p <= 1.0,
                "{tier}: ledger {} vs netlog {:?}: {p:.3} %",
                l.raw(),
                l.netlog_bytes
            );
            // Every fetched byte is in a named row, and every row is a file the browser fetched.
            assert!(
                l.rows.iter().all(|r| r.section != "netlog only"),
                "{:?}",
                l.rows
            );
            assert_eq!(l.rows.len(), all.len());
            assert!((l.other() as f64) < 0.01 * l.netlog_bytes.unwrap() as f64);
            assert!(
                l.verdict.as_ref().is_some_and(|v| v.pass()),
                "{:?}",
                l.verdict
            );
        }
        // Ultra on BC7 names the model's importance; Low's rows are its own files only.
        let net = netlog_fetched(&netlog(
            &pkg,
            &[
                "index.html",
                "game.js",
                "game_bg.wasm",
                "models/stream.json",
                "models/hero.gltf",
                "models/hero.base.bc7.ktx2",
            ],
        ));
        let ultra = ledger(&inp, "ultra", None, Some(&net)).unwrap();
        assert_eq!(ultra.family, "bc7");
        assert_eq!(
            ultra
                .rows
                .iter()
                .find(|r| r.path == "models/hero.base.bc7.ktx2")
                .unwrap()
                .importance,
            Some(0.31)
        );
        // A file the plan does not name shows up as its own row, not as "other".
        let mut extra = net.clone();
        extra.push(Fetched {
            path: "models/hero.base.png".into(),
            bytes: Some(90_150),
        });
        let l = ledger(&inp, "ultra", None, Some(&extra)).unwrap();
        assert!(
            l.rows
                .iter()
                .any(|r| r.path == "models/hero.base.png" && r.section == "netlog only")
        );
        assert!(l.off_by_percent().unwrap() <= 1.0);
        // Without a netlog the heaviest family is shown.
        let l = ledger(&inp, "ultra", None, None).unwrap();
        assert_eq!(
            (l.family.as_str(), l.family_from.as_str()),
            ("bc7", "heaviest")
        );
        let md = markdown("t", &pkg, &[ultra]);
        assert!(
            md.contains("within 1 %") && md.contains("31.0 %") && md.contains("fx: +11.7 KB"),
            "{md}"
        );
        let _ = std::fs::remove_dir_all(&pkg);
    }
}
