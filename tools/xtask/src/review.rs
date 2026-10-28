//! `cargo xtask review <app-or-kit>`: the kit review card, made at a kit's
//! hand-over (docs/KIT_PROCESS.md step 9) so the reviewer sees everything in
//! one place.
//!
//! It writes `.vibedeck/review/<app>/` with `card.md` (a plain summary on top,
//! the details below), `card.json` and three captures (`capture-1.png` ..)
//! taken at fixed times on the game's bench camera path. Steps:
//!
//! 1. The shipped build (`--ship`, no live) through the same Trunk build as
//!    `cargo xtask build`: gzipped size and the markers in the wasm, checked
//!    against evani.toml (nothing unlisted may reach the download).
//! 2. The GPU bench (`cargo xtask bench`), or its saved
//!    `target/evani-bench/results.json` when that is newer than every engine
//!    source and tier file (`--rebench` forces a run).
//! 3. The captures: `tools/live-check/capture.mjs --times ..` (the live
//!    bridge + headless Chrome + `capture.frame` with the bench camera).
//! 4. The process checks (docs/KIT_PROCESS.md items 11 to 15, `checks.rs`):
//!    tier cost order from the bench, and from the same capture session the
//!    entity count and registered params, the overlay and HUD probes, and the
//!    black/white clear-colour renders of every scripted camera for the
//!    world-edge check; plus the hand-over (size and engine commit from git).
//!    Items 16 to 27 (`kitchecks.rs`, `critic.rs`): the target pack, the
//!    showcase captures and video, the kit's review dump and playtest tests,
//!    the visual regression, the geocheck report and the critic's last round.
//!    `AutoCheck` is the registry that maps each item to its result.
//! 5. Size change against the previous card (`.vibedeck/review/history.json`,
//!    see `pick_previous`), the reference image prompt, what to try, and the
//!    done-when checklist parsed from docs/KIT_PROCESS.md with the
//!    automatically checkable items ticked or failed.
//!
//! Everything that decides what the card says is a pure function with tests;
//! `cmd_review` only gathers the inputs.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use evani_core::tiers::Tiers;
use serde_json::{Value, json};

use crate::checks::{self, CaptureProbe, EdgeParams, EdgeView, HandOver, Verdict};
use crate::critic::{self, CriticReport};
use crate::kitchecks;
use crate::{BuildSize, Manifest, kb, load_app, markers_in_text, trunk_build, workspace_root};

/// Bench camera times for the three captures (seconds of sim time). The
/// sandbox's orbit takes 60 s, so these are three views a third apart.
pub const CAPTURE_TIMES: [f64; 3] = [0.0, 20.0, 40.0];
pub const CAPTURE_WIDTH: u32 = 640;

/// The first port the review's bridge tries (capture.mjs walks up from it
/// past ports in use): away from the Evani.dev tab's 8096, so a review
/// never drives the tab's page or its bridge. The bridge it starts keeps its
/// own session file (`.evani/live/review/<port>/session.json`), never the
/// tab's `.evani/live/session.json`.
pub const REVIEW_PORT: u16 = 8180;

/// The port the captures start from: `--port`, else [`REVIEW_PORT`].
fn capture_port(a: &ReviewArgs) -> u16 {
    a.port.unwrap_or(REVIEW_PORT)
}

pub fn usage() -> &'static str {
    "  cargo xtask review <app-or-kit> [--rebench] [--port <P>] [--chrome <path>] [--main <ref>]\n  cargo xtask review <app-or-kit> --accept    keep the last showcase captures as the visual-regression baseline (after Chris approves)"
}

#[derive(Debug, Default, PartialEq)]
pub struct ReviewArgs {
    pub name: String,
    pub rebench: bool,
    /// Copy the last showcase captures to `accepted/` and stop.
    pub accept: bool,
    pub port: Option<u16>,
    pub chrome: Option<String>,
    /// The main line the hand-over check compares the engine commit with
    /// (default: `main` when it exists, else `m1-foundation`).
    pub main: Option<String>,
}

impl ReviewArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        Self::parse_with(args, true)
    }

    /// [`Self::parse`]; `need_name` false for `--app-dir` (the folder is the game).
    pub fn parse_with(args: &[String], need_name: bool) -> Result<Self, String> {
        let mut a = Self::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--rebench" => a.rebench = true,
                "--accept" => a.accept = true,
                "--port" => {
                    let v = it.next().ok_or("--port needs a value")?;
                    a.port = Some(v.parse().map_err(|_| format!("bad --port {v}"))?);
                }
                "--chrome" => a.chrome = Some(it.next().ok_or("--chrome needs a value")?.clone()),
                "--main" => a.main = Some(it.next().ok_or("--main needs a value")?.clone()),
                other if other.starts_with("--") => {
                    return Err(format!("unknown review option {other}\n{}", usage()));
                }
                other if a.name.is_empty() => a.name = other.to_string(),
                other => return Err(format!("review takes one app or kit, got another: {other}")),
            }
        }
        if need_name && a.name.is_empty() {
            return Err(format!("review needs an app or kit name\n{}", usage()));
        }
        if !need_name && !a.name.is_empty() {
            return Err(format!("`{}`: with --app-dir the game is the folder; give no game id", a.name));
        }
        Ok(a)
    }
}

// ---- bench ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum TierVerdict {
    Pass,
    Fail,
    /// The browser can't run this tier (VR without WebXR + WebGPU).
    Unavailable(String),
    /// No result came back (a run error).
    NoResult(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TierLine {
    pub id: String,
    pub label: String,
    pub verdict: TierVerdict,
    /// Failed checks with their numbers, as the bench reports them.
    pub failures: Vec<String>,
    pub avg_ms: Option<f32>,
    pub p95_ms: Option<f32>,
    pub gpu_ms: Option<f32>,
    /// Median GPU ms per frame (what the bench's order check judges).
    pub gpu_p50_ms: Option<f32>,
    pub target_ms: Option<f32>,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BenchInfo {
    pub lines: Vec<TierLine>,
    pub gpu: String,
    pub browser: String,
    /// "fresh run" or "saved results" (with the time they were made).
    pub source: String,
    pub errors: Vec<String>,
    /// The saved stress-scene run (`cargo xtask bench --stress`), when it is
    /// newer than every engine source: `(tier, median of the run medians)`.
    /// The cost-order check prefers it to the bench path's numbers.
    pub stress_gpu: Vec<(String, f32)>,
    /// How many runs `stress_gpu` is the median of.
    pub stress_runs: u64,
}

impl BenchInfo {
    /// GPU ms per tier, Lite to Ultra, for the cost-order check, and where
    /// they come from: the stress scene (median of its runs) when it has
    /// all five tiers, otherwise the bench path's median (or mean) GPU ms.
    pub fn order_inputs(&self) -> (Vec<(String, Option<f32>)>, String) {
        let stress: Vec<(String, Option<f32>)> = checks::COST_ORDER
            .iter()
            .map(|t| ((*t).to_string(), self.stress_gpu.iter().find(|(id, _)| id == t).map(|(_, g)| *g)))
            .collect();
        if stress.iter().all(|(_, g)| g.is_some()) {
            return (
                stress,
                format!("stress scene, median GPU ms per frame, median of {} runs", self.stress_runs),
            );
        }
        let path = checks::COST_ORDER
            .iter()
            .map(|t| {
                let l = self.lines.iter().find(|l| l.id == *t);
                ((*t).to_string(), l.and_then(|l| l.gpu_p50_ms.or(l.gpu_ms)))
            })
            .collect();
        (path, "bench path, median GPU ms per frame; no fresh `cargo xtask bench --stress` run".to_string())
    }
}

impl BenchInfo {
    /// Lite to Ultra pass, and VR passes or is unavailable (done-when 3).
    pub fn passes(&self) -> bool {
        !self.lines.is_empty()
            && self.lines.iter().all(|l| match &l.verdict {
                TierVerdict::Pass => true,
                TierVerdict::Unavailable(_) => l.id == "vr",
                _ => false,
            })
    }
}

/// One line per tier, in tier order, from the runner's saved JSON.
pub fn bench_lines(tiers: &[(String, String)], runner: &Value) -> BenchInfo {
    let c = crate::bench::collect(runner);
    let text = |v: Option<&Value>, k: &str| v.and_then(|a| a.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
    let adapter = runner.get("adapter").filter(|a| !a.is_null());
    let gpu = [text(adapter, "vendor"), text(adapter, "description")]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let lines = tiers
        .iter()
        .map(|(id, label)| {
            let prefix = format!("{id}: ");
            let errs: Vec<String> = c
                .errors
                .iter()
                .filter_map(|e| e.strip_prefix(&prefix).map(str::to_string))
                .collect();
            match c.results.iter().find(|r| &r.tier == id) {
                Some(r) => TierLine {
                    id: id.clone(),
                    label: label.clone(),
                    verdict: if r.pass { TierVerdict::Pass } else { TierVerdict::Fail },
                    failures: r.failures.clone(),
                    avg_ms: Some(r.avg_ms),
                    p95_ms: Some(r.p95_ms),
                    gpu_ms: r.gpu_ms,
                    gpu_p50_ms: r.gpu_p50_ms,
                    target_ms: Some(r.limits.target_ms),
                    note: r.note.clone(),
                },
                None => {
                    let why = if errs.is_empty() { "not run".to_string() } else { errs.join("; ") };
                    let webxr = why.to_lowercase().contains("webxr");
                    TierLine {
                        id: id.clone(),
                        label: label.clone(),
                        verdict: if id == "vr" && webxr {
                            TierVerdict::Unavailable(why)
                        } else {
                            TierVerdict::NoResult(why)
                        },
                        failures: Vec::new(),
                        avg_ms: None,
                        p95_ms: None,
                        gpu_ms: None,
                        gpu_p50_ms: None,
                        target_ms: None,
                        note: String::new(),
                    }
                }
            }
        })
        .collect();
    BenchInfo {
        lines,
        gpu,
        browser: text(Some(runner), "version"),
        source: String::new(),
        errors: c.errors,
        ..BenchInfo::default()
    }
}

/// The check's name without its numbers: "frame-time target (avg ..)" -> "frame-time target".
fn short_check(f: &str) -> &str {
    f.split(" (").next().unwrap_or(f).trim()
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The plain one-liner for the top of the card.
pub fn bench_sentence(b: &BenchInfo) -> String {
    if b.lines.is_empty() {
        return "no speed test results.".into();
    }
    let names = |f: &dyn Fn(&TierLine) -> bool| -> Vec<String> {
        b.lines.iter().filter(|l| f(l)).map(|l| l.label.clone()).collect()
    };
    let pass = names(&|l| l.verdict == TierVerdict::Pass);
    let fails: Vec<String> = b
        .lines
        .iter()
        .filter(|l| l.verdict == TierVerdict::Fail)
        .map(|l| {
            let checks: Vec<&str> = l.failures.iter().map(|f| short_check(f)).collect();
            format!("{} ({})", l.label, checks.join(", "))
        })
        .collect();
    let unavailable = names(&|l| matches!(l.verdict, TierVerdict::Unavailable(_)));
    let missing = names(&|l| matches!(l.verdict, TierVerdict::NoResult(_)));
    let mut parts = Vec::new();
    if fails.is_empty() && unavailable.is_empty() && missing.is_empty() {
        return format!("passes on {}.", join_and(&pass));
    }
    if !fails.is_empty() {
        parts.push(format!("FAILS on {}", join_and(&fails)));
    }
    if !pass.is_empty() {
        parts.push(format!("passes on {}", join_and(&pass)));
    }
    if !unavailable.is_empty() {
        parts.push(format!("{} not available in this browser", join_and(&unavailable)));
    }
    if !missing.is_empty() {
        parts.push(format!("no result for {}", join_and(&missing)));
    }
    format!("{}.", parts.join("; "))
}

// ---- size and history -------------------------------------------------------------

/// One produced card's size, kept in `.vibedeck/review/history.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeRecord {
    pub app: String,
    pub kit: String,
    pub total_gz: u64,
    pub wasm_gz: u64,
    pub js_gz: u64,
    pub when: String,
    pub commit: String,
}

impl SizeRecord {
    pub fn to_json(&self) -> Value {
        json!({
            "app": self.app, "kit": self.kit, "total_gz": self.total_gz,
            "wasm_gz": self.wasm_gz, "js_gz": self.js_gz, "when": self.when, "commit": self.commit,
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |k: &str| v.get(k).and_then(Value::as_u64);
        Some(Self {
            app: s("app")?,
            kit: s("kit")?,
            total_gz: n("total_gz")?,
            wasm_gz: n("wasm_gz").unwrap_or(0),
            js_gz: n("js_gz").unwrap_or(0),
            when: s("when").unwrap_or_default(),
            commit: s("commit").unwrap_or_default(),
        })
    }
}

pub fn parse_history(text: &str) -> Vec<SizeRecord> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(SizeRecord::from_json).collect()))
        .unwrap_or_default()
}

/// Which earlier card the size is compared with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Basis {
    /// The last card for the same kit: what this round of work changed.
    SameKit,
    /// The nearest kit before this one in ROADMAP order that has a card.
    EarlierKit,
    /// The most recent card of any kit (this kit is not in the roadmap).
    MostRecent,
}

impl Basis {
    pub fn describe(self) -> &'static str {
        match self {
            Basis::SameKit => "the previous card for this kit",
            Basis::EarlierKit => "the nearest earlier kit in ROADMAP order",
            Basis::MostRecent => "the most recent card for any kit",
        }
    }
}

/// The card to compare with (`history` is oldest first and does not hold the
/// card being made). See DECISIONS.md, "Kit review card".
pub fn pick_previous<'a>(history: &'a [SizeRecord], kit: &str, roadmap: &[String]) -> Option<(&'a SizeRecord, Basis)> {
    let last_for = |k: &str| history.iter().rev().find(|r| r.kit == k);
    if let Some(r) = last_for(kit) {
        return Some((r, Basis::SameKit));
    }
    if let Some(pos) = roadmap.iter().position(|id| id == kit) {
        return roadmap[..pos]
            .iter()
            .rev()
            .find_map(|id| last_for(id))
            .map(|r| (r, Basis::EarlierKit));
    }
    history.last().map(|r| (r, Basis::MostRecent))
}

/// "1.3 KB larger (+0.6%)", "2.0 KB smaller (-0.9%)" or "the same size".
pub fn describe_delta(now: u64, prev: u64) -> String {
    let d = now as i64 - prev as i64;
    if d == 0 {
        return "the same size".into();
    }
    let pct = if prev == 0 { 0.0 } else { d as f64 * 100.0 / prev as f64 };
    let word = if d > 0 { "larger" } else { "smaller" };
    format!("{:.1} KB {word} ({pct:+.1}%)", d.unsigned_abs() as f64 / 1024.0)
}

/// For a sentence: "1.3 KB (+0.6%) larger than" or "the same size as".
pub fn delta_phrase(now: u64, prev: u64) -> String {
    let d = now as i64 - prev as i64;
    if d == 0 {
        return "the same size as".into();
    }
    let pct = if prev == 0 { 0.0 } else { d as f64 * 100.0 / prev as f64 };
    let word = if d > 0 { "larger" } else { "smaller" };
    format!("{:.1} KB ({pct:+.1}%) {word} than", d.unsigned_abs() as f64 / 1024.0)
}

// ---- build-time on/off ------------------------------------------------------------

/// What the shipped wasm holds that evani.toml doesn't list (`extra`, a
/// failure) and what it lists but the wasm lacks (`missing`, also a
/// failure). `shaderless` are listed effects with no shader of their own
/// (shake, parallax: params and CPU maths only), so no shader marker can
/// show them; their Cargo feature is what builds them in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OnOff {
    pub extra: Vec<String>,
    pub missing: Vec<String>,
    pub shaderless: Vec<String>,
}

impl OnOff {
    pub fn pass(&self) -> bool {
        self.extra.is_empty() && self.missing.is_empty()
    }
}

/// `m` is the shipped manifest; `own` are markers the app, kit or look
/// crates declare themselves (their own modules and shaders); `engine` are
/// the markers declared anywhere in the engine crates (which effects have a
/// shader at all).
pub fn onoff_check(markers: &[String], m: &Manifest, own: &[String], engine: &[String]) -> OnOff {
    let mut wanted_modules = vec!["render".to_string()];
    if !m.effects.is_empty() {
        wanted_modules.push("fx".into());
    }
    if m.physics.is_some() {
        wanted_modules.push("physics".into());
    }
    for (on, name) in [(m.net, "net"), (m.xr, "xr"), (m.live, "live"), (m.models, "models"), (m.anim, "anim"), (m.ui, "ui")] {
        if on {
            wanted_modules.push(name.into());
        }
    }
    let effect_of = |shader: &str| -> Option<String> {
        m.effects
            .iter()
            .find(|e| shader == e.as_str() || shader.starts_with(&format!("{e}_")))
            .cloned()
    };
    let mut out = OnOff::default();
    for mk in markers {
        if own.contains(mk) {
            continue;
        }
        let ok = if let Some(id) = mk.strip_prefix("evani.module.") {
            wanted_modules.iter().any(|w| w == id)
        } else if let Some(id) = mk.strip_prefix("evani.shader.") {
            effect_of(id).is_some()
        } else {
            true
        };
        if !ok {
            out.extra.push(mk.clone());
        }
    }
    for w in &wanted_modules {
        let mk = format!("evani.module.{w}");
        if !markers.contains(&mk) {
            out.missing.push(mk);
        }
    }
    let is_effect_shader = |e: &str, mk: &String| mk.strip_prefix("evani.shader.").is_some_and(|id| id == e || id.starts_with(&format!("{e}_")));
    for e in &m.effects {
        if !engine.iter().any(|mk| is_effect_shader(e, mk)) {
            out.shaderless.push(e.clone());
            continue;
        }
        if !markers.iter().any(|mk| is_effect_shader(e, mk)) {
            out.missing.push(format!("evani.shader.{e}*"));
        }
    }
    out
}

/// The markers a game's own crates may carry without evani.toml listing them:
/// a listed material module (`[modules] nature`, `proc`) keeps all its markers;
/// the crates a kit's render-pass features pull in count for their
/// `evani.shader.*` markers only (as the render module's own shaders do), never
/// for a module marker such as `evani.module.models` that `dep:evani-gltf`
/// declares, so an unlisted `models` module still fails the on/off check.
pub fn listed_engine_markers(root: &Path, m: &Manifest, game_cargo_tomls: &[String]) -> Vec<String> {
    let material = listed_engine_crates(root, m, &[]);
    let passes: Vec<PathBuf> = listed_engine_crates(root, m, game_cargo_tomls).into_iter().filter(|d| !material.contains(d)).collect();
    let mut out = markers_under(&material);
    out.extend(markers_under(&passes).into_iter().filter(|mk| mk.starts_with("evani.shader.")));
    out
}

/// Engine crates whose markers a game lists without naming each one: the
/// material modules evani.toml lists (`[modules] nature`, `proc`:
/// `evani_guide::manifest::MATERIAL_MODULES`), and the crates behind the
/// evani-render features the game's own crates turn on in their evani-render
/// dependency (render passes are the kit's choice, not modules; like the
/// render module's own shaders, theirs come with it). `game_cargo_tomls` are
/// the app and kit crates' Cargo.toml texts.
pub fn listed_engine_crates(root: &Path, m: &Manifest, game_cargo_tomls: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for (key, krate) in evani_guide::manifest::MATERIAL_MODULES {
        let listed = match *key {
            "nature" => m.nature.is_some(),
            "proc" => m.proc.is_some(),
            _ => false,
        };
        if listed {
            out.push(root.join("crates").join(krate));
        }
    }
    let render: toml::Table = std::fs::read_to_string(root.join("crates").join("evani-render").join("Cargo.toml"))
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or_default();
    let features = render.get("features").and_then(toml::Value::as_table);
    let mut todo: Vec<String> = game_cargo_tomls.iter().flat_map(|t| render_features(t)).collect();
    let mut seen: Vec<String> = Vec::new();
    while let Some(f) = todo.pop() {
        if seen.contains(&f) {
            continue;
        }
        seen.push(f.clone());
        for item in features.and_then(|x| x.get(&f)).and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str) {
            if let Some(dep) = item.strip_prefix("dep:") {
                let dir = root.join("crates").join(dep);
                if dir.is_dir() && !out.contains(&dir) {
                    out.push(dir);
                }
            } else if !item.contains('/') {
                todo.push(item.to_string());
            }
        }
    }
    out
}

/// The evani-render features a crate's Cargo.toml turns on in its own
/// `evani-render = { .., features = [..] }` dependency (target tables too).
fn render_features(cargo_toml: &str) -> Vec<String> {
    let Ok(t) = cargo_toml.parse::<toml::Table>() else { return Vec::new() };
    let mut tables: Vec<&toml::Table> = t.get("dependencies").and_then(toml::Value::as_table).into_iter().collect();
    for target in t.get("target").and_then(toml::Value::as_table).into_iter().flat_map(|x| x.values()) {
        tables.extend(target.get("dependencies").and_then(toml::Value::as_table));
    }
    tables
        .iter()
        .filter_map(|d| d.get("evani-render"))
        .filter_map(|r| r.get("features"))
        .filter_map(toml::Value::as_array)
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect()
}

// ---- reference image prompt, what to try ------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Reference {
    /// From the kit's SPEC.md (path relative to the workspace).
    Spec { path: String, text: String },
    /// Built from catalog/genres.json and looks.json with the KIT_PROCESS template.
    Catalog { text: String },
    /// Nothing: why, in plain words.
    None { why: String },
}

/// The reference image prompt in a SPEC.md: the first fenced block (or else
/// the paragraph) under a heading that names it.
pub fn reference_from_spec(md: &str) -> Option<String> {
    let mut in_section = false;
    let mut in_fence = false;
    let mut fence = Vec::new();
    let mut para = Vec::new();
    for line in md.lines() {
        let t = line.trim_end();
        if !in_fence && t.starts_with('#') {
            if in_section {
                break;
            }
            in_section = t.to_lowercase().contains("reference image");
            continue;
        }
        if !in_section {
            continue;
        }
        if t.trim_start().starts_with("```") {
            if in_fence {
                return Some(fence.join("\n").trim().to_string()).filter(|s| !s.is_empty());
            }
            in_fence = true;
            continue;
        }
        if in_fence {
            fence.push(t.to_string());
        } else if !t.trim().is_empty() {
            para.push(t.trim().to_string());
        }
    }
    Some(para.join("\n")).filter(|s| !s.is_empty())
}

fn find_by_id<'a>(catalog: &'a Value, list: &str, id: &str) -> Option<&'a Value> {
    catalog
        .get(list)?
        .as_array()?
        .iter()
        .find(|g| g.get("id").and_then(Value::as_str) == Some(id))
}

/// The KIT_PROCESS template filled from the catalog, when the kit id is a
/// catalog genre and the look a catalog look.
pub fn reference_from_catalog(genres: &Value, looks: &Value, kit: &str, look: &str) -> Option<String> {
    let g = find_by_id(genres, "genres", kit)?;
    let l = find_by_id(looks, "looks", look)?;
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let cams = |v: &Value| -> Vec<String> {
        v.get("cameras")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };
    let look_cams = cams(l);
    let genre_cams = cams(g);
    let camera = genre_cams
        .iter()
        .find(|c| look_cams.contains(c))
        .or(genre_cams.first())
        .cloned()
        .unwrap_or_else(|| "gameplay".into());
    let quality = if s(l, "type").eq_ignore_ascii_case("2D") {
        "Crisp, polished 2D at native resolution: clean edges, readable silhouettes, rich colour and lighting where the style allows."
    } else {
        "Top-quality real-time 3D at the engine's Ultra tier: rich global illumination, soft shadows, clean anti-aliasing, film-quality colour grading."
    };
    Some(format!(
        "In-game screenshot from an original {} video game, {camera} view.\n{}\nArt style: {}. {}\n{quality}\n16:9 widescreen game capture, 2560×1440. Everything is original: characters, vehicles, creatures, logos and UI. No existing game franchises, characters, brands or real people, and no watermark. Any on-screen text is short and legible.",
        s(g, "name"),
        s(g, "image_scene"),
        s(l, "name"),
        s(l, "look"),
    ))
}

/// Bullets under a "What to try" heading in a README.
pub fn try_from_readme(md: &str) -> Vec<String> {
    let mut in_section = false;
    let mut out = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            if in_section {
                break;
            }
            in_section = t.to_lowercase().contains("what to try");
            continue;
        }
        if in_section && let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            out.push(item.trim().to_string());
        }
    }
    out
}

// ---- done-when checklist ----------------------------------------------------------

/// The numbered items under `## Done when`, with "(... planned ...)" notes removed.
pub fn parse_done_when(md: &str) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut in_section = false;
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("## ") {
            in_section = t.eq_ignore_ascii_case("## done when");
            continue;
        }
        if !in_section || t.is_empty() {
            continue;
        }
        let num: String = t.chars().take_while(char::is_ascii_digit).collect();
        if !num.is_empty()
            && let Some(rest) = t[num.len()..].strip_prefix(". ")
            && let Ok(n) = num.parse()
        {
            out.push((n, rest.trim().to_string()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(' ');
            last.1.push_str(t);
        }
    }
    for item in &mut out {
        item.1 = drop_planned_notes(&item.1);
    }
    out
}

/// Remove parenthesised notes that say a part is still planned.
fn drop_planned_notes(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')').map(|c| open + c) else {
            break;
        };
        let inner = &rest[open..=close];
        out.push_str(&rest[..open]);
        if !inner.contains("planned") {
            out.push_str(inner);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckStatus {
    Pass,
    Fail,
    /// QQ checks it by hand.
    Manual,
    /// The check could not run (the kit lacks what it reads); blocks the
    /// hand-over unless its evidence says it is only waiting.
    NotRun,
    /// Passed, with a big change for the critic and Chris to look at.
    Flag,
    /// The check does not apply to this kit.
    NotApplicable,
}

impl CheckStatus {
    pub fn from_verdict(v: &Verdict) -> Self {
        match (v.kind, v.pass) {
            (checks::Kind::NotRun, _) => CheckStatus::NotRun,
            (checks::Kind::Flag, _) => CheckStatus::Flag,
            (checks::Kind::NotApplicable, _) => CheckStatus::NotApplicable,
            (checks::Kind::Measured, true) => CheckStatus::Pass,
            (checks::Kind::Measured, false) => CheckStatus::Fail,
        }
    }

    /// `(tick mark, word)` on the card's checklist.
    pub fn mark(self) -> (&'static str, &'static str) {
        match self {
            CheckStatus::Pass => ("x", "PASS"),
            CheckStatus::Fail => (" ", "FAIL"),
            CheckStatus::Manual => (" ", "QQ"),
            CheckStatus::NotRun => (" ", "NOT RUN"),
            CheckStatus::Flag => ("x", "FLAG"),
            CheckStatus::NotApplicable => ("x", "N/A"),
        }
    }

    pub fn json(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Fail => "fail",
            CheckStatus::Manual => "manual",
            CheckStatus::NotRun => "not_run",
            CheckStatus::Flag => "flag",
            CheckStatus::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckItem {
    pub n: u32,
    pub text: String,
    pub status: CheckStatus,
    pub evidence: String,
}

/// Which automatic check covers a done-when item, from its wording.
///
/// The registry: to add a check, add a variant here, its wording hook in
/// [`AutoCheck::hooks`] (lowercase phrases that all must appear in the item;
/// they must not collide with another check's), a function in `checks.rs`,
/// one gather step in `cmd_review` that calls `AutoResults::set`, and a
/// numbered item at the end of docs/KIT_PROCESS.md "Done when".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AutoCheck {
    BuildAndSize,
    Bench,
    Docs,
    /// Item 11: GPU time rises Lite to Ultra.
    TierCostOrder,
    /// Item 12: gameplay objects live in the ECS, tunables are registered.
    Architecture,
    /// Item 13: captures hide the debug overlay and show the HUD.
    CaptureHud,
    /// Item 14: no visible world edge from any scripted camera.
    WorldEdge,
    /// Item 15: the hand-over states the published size and the engine commit.
    HandOver,
    /// Item 16: the target pack is complete.
    TargetPack,
    /// Item 18: every signature moment on Low and Ultra at 1080p, labelled, plus a video.
    CaptureSet,
    /// Item 19: every object grounded with a contact shadow (the kit's review dump).
    ContactShadows,
    /// Item 20: distinct racer or unit colours, the player marked (the kit's review dump).
    Colours,
    /// Item 21: visual regression against the accepted captures.
    Regression,
    /// Item 22: the playtest bot reaches a win and a lose.
    Playtest,
    /// Item 23: no untagged issue in the geocheck report.
    Geocheck,
    /// Item 24 (2D): camera framing, from the geocheck-2d report.
    CameraFraming,
    /// Item 25 (2D): bot level checks, from the geocheck-2d report.
    BotLevel,
    /// Item 26 (2D): the UI text rule, from the geocheck-2d report.
    UiText,
    /// Item 27: the independent critic passed (its newest round).
    Critic,
    /// Item 29: a playground supports free roam (declared, in the kit, tested).
    FreeRoam,
}

impl AutoCheck {
    pub const ALL: [AutoCheck; 20] = [
        AutoCheck::BuildAndSize,
        AutoCheck::Bench,
        AutoCheck::Docs,
        AutoCheck::TierCostOrder,
        AutoCheck::Architecture,
        AutoCheck::CaptureHud,
        AutoCheck::WorldEdge,
        AutoCheck::HandOver,
        AutoCheck::TargetPack,
        AutoCheck::CaptureSet,
        AutoCheck::ContactShadows,
        AutoCheck::Colours,
        AutoCheck::Regression,
        AutoCheck::Playtest,
        AutoCheck::Geocheck,
        AutoCheck::CameraFraming,
        AutoCheck::BotLevel,
        AutoCheck::UiText,
        AutoCheck::Critic,
        AutoCheck::FreeRoam,
    ];

    /// Phrases (lowercase). An item belongs to the check if it contains any
    /// one of the groups in full: `[[a, b], [c]]` means (a and b) or c.
    fn hooks(self) -> &'static [&'static [&'static str]] {
        match self {
            AutoCheck::BuildAndSize => &[&["gzipped"], &["listed modules"]],
            AutoCheck::Bench => &[&["apps/bench"], &["passes on low"]],
            AutoCheck::Docs => &[&["spec.md", "readme"]],
            AutoCheck::TierCostOrder => &[&["tier cost order"]],
            AutoCheck::Architecture => &[&["architecture conformance"]],
            AutoCheck::CaptureHud => &[&["captures hide the debug overlay"]],
            AutoCheck::WorldEdge => &[&["world edge"]],
            AutoCheck::HandOver => &[&["hand-over states"]],
            AutoCheck::TargetPack => &[&["target pack complete:"]],
            AutoCheck::CaptureSet => &[&["signature-moment captures:"]],
            AutoCheck::ContactShadows => &[&["contact shadows:"]],
            AutoCheck::Colours => &[&["distinct colours:"]],
            AutoCheck::Regression => &[&["visual regression:"]],
            AutoCheck::Playtest => &[&["playtest bot:"]],
            AutoCheck::Geocheck => &[&["geocheck reports:"]],
            AutoCheck::CameraFraming => &[&["camera framing (2d):"]],
            AutoCheck::BotLevel => &[&["bot level checks (2d):"]],
            AutoCheck::UiText => &[&["ui text rule (2d):"]],
            AutoCheck::Critic => &[&["independent critic:"]],
            AutoCheck::FreeRoam => &[&["free roam:"]],
        }
    }

    /// A short name for the card's process-check lines.
    pub fn label(self) -> &'static str {
        match self {
            AutoCheck::BuildAndSize => "build and size",
            AutoCheck::Bench => "bench",
            AutoCheck::Docs => "docs",
            AutoCheck::TierCostOrder => "tier cost order",
            AutoCheck::Architecture => "architecture",
            AutoCheck::CaptureHud => "captures and HUD",
            AutoCheck::WorldEdge => "world edge",
            AutoCheck::HandOver => "hand-over",
            AutoCheck::TargetPack => "target pack",
            AutoCheck::CaptureSet => "showcase captures",
            AutoCheck::ContactShadows => "contact shadows",
            AutoCheck::Colours => "distinct colours",
            AutoCheck::Regression => "visual regression",
            AutoCheck::Playtest => "playtest bot",
            AutoCheck::Geocheck => "geocheck",
            AutoCheck::CameraFraming => "camera framing (2D)",
            AutoCheck::BotLevel => "bot level checks (2D)",
            AutoCheck::UiText => "UI text rule (2D)",
            AutoCheck::Critic => "critic",
            AutoCheck::FreeRoam => "free roam",
        }
    }

    /// The process checks added after the first three (items 11 and on).
    pub fn is_process(self) -> bool {
        !matches!(self, AutoCheck::BuildAndSize | AutoCheck::Bench | AutoCheck::Docs)
    }
}

pub fn auto_check_for(text: &str) -> Option<AutoCheck> {
    let t = text.to_lowercase();
    AutoCheck::ALL.into_iter().find(|c| c.hooks().iter().any(|group| group.iter().all(|p| t.contains(p))))
}

/// Results of the automatic checks, as (status, evidence).
#[derive(Debug, Clone, Default)]
pub struct AutoResults {
    pub results: std::collections::BTreeMap<AutoCheck, (CheckStatus, String)>,
}

impl AutoResults {
    pub fn set(&mut self, c: AutoCheck, pass: bool, evidence: impl Into<String>) {
        self.results.insert(c, (if pass { CheckStatus::Pass } else { CheckStatus::Fail }, evidence.into()));
    }

    pub fn set_verdict(&mut self, c: AutoCheck, v: &Verdict) {
        self.results.insert(c, (CheckStatus::from_verdict(v), v.evidence.clone()));
    }

    pub fn get(&self, c: AutoCheck) -> Option<&(CheckStatus, String)> {
        self.results.get(&c)
    }
}

pub fn checklist(items: &[(u32, String)], auto: &AutoResults) -> Vec<CheckItem> {
    items
        .iter()
        .map(|(n, text)| {
            let r = auto_check_for(text).and_then(|c| auto.get(c));
            let (status, evidence) = match r {
                Some((st, e)) => (*st, e.clone()),
                None => (CheckStatus::Manual, "check by hand".to_string()),
            };
            CheckItem {
                n: *n,
                text: text.clone(),
                status,
                evidence,
            }
        })
        .collect()
}

/// `(id, status)` per kit row of ROADMAP.md, in order.
pub fn roadmap_rows(md: &str) -> Vec<(String, String)> {
    md.lines()
        .filter_map(|l| {
            let cols: Vec<&str> = l.split('|').map(str::trim).collect();
            // | # | Kit | ID | Rank | Physics | First look | Status | Notes |
            if cols.len() < 9 || !cols[1].chars().all(|c| c.is_ascii_digit()) || cols[1].is_empty() {
                return None;
            }
            Some((cols[3].trim_matches('`').to_string(), cols[7].to_string()))
        })
        .collect()
}

// ---- the card ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Shot {
    pub file: String,
    pub t: f64,
    pub width: u32,
    pub height: u32,
    pub tier: String,
    /// The scripted moment (`[review] moments`) and what it shows; `None`
    /// for a bench-path view at time `t`.
    pub moment: Option<String>,
    pub label: String,
}

impl Shot {
    /// Column heading on the card: the moment and its tier, or the time.
    pub fn heading(&self) -> String {
        match &self.moment {
            Some(m) if self.tier.is_empty() => m.replace('_', " "),
            Some(m) => format!("{} ({})", m.replace('_', " "), self.tier),
            None => format!("t = {} s", self.t),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub app: String,
    pub game: String,
    pub kit: String,
    pub look: String,
    pub when: String,
    pub commit: String,
    pub bench: BenchInfo,
    pub size: SizeRecord,
    pub html_raw: u64,
    pub previous: Option<(SizeRecord, Basis)>,
    pub markers: Vec<String>,
    pub onoff: OnOff,
    /// Optional modules on in evani.toml, in plain words.
    pub modules: Vec<String>,
    pub effects: Vec<String>,
    pub shots: Vec<Shot>,
    pub capture_error: Option<String>,
    pub reference: Reference,
    pub try_list: Vec<String>,
    pub try_source: String,
    pub checks: Vec<CheckItem>,
    pub process: ProcessInfo,
}

/// What the process checks (items 11 and on) measured, for the card.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcessInfo {
    /// One verdict per process check, in `AutoCheck::ALL` order.
    pub results: Vec<(AutoCheck, Verdict)>,
    /// GPU ms per tier, Low to Ultra (see [`BenchInfo::order_inputs`]).
    pub tier_gpu: Vec<(String, Option<f32>)>,
    /// Where `tier_gpu` comes from (stress scene or bench path).
    pub tier_gpu_source: String,
    /// The world-edge result of every scripted camera.
    pub edges: Vec<(String, Result<EdgeView, String>)>,
    /// The captures' overlay and HUD probes, and the HUD selectors the kit declared.
    pub captures: Vec<CaptureProbe>,
    pub hud_declared: Vec<String>,
    /// Page screenshots next to the captures: `(capture name, file)`.
    pub page_shots: Vec<(String, String)>,
    pub handover: HandOver,
    /// The labelled 1080p captures and the run video (item 18).
    pub showcase: kitchecks::Showcase,
    /// The newest critic round, and how many rounds ran (item 27).
    pub critic: Option<CriticReport>,
    pub critic_rounds: usize,
    pub critic_mean: Option<f64>,
}

impl ProcessInfo {
    pub fn verdict(&self, c: AutoCheck) -> Option<&Verdict> {
        self.results.iter().find(|(k, _)| *k == c).map(|(_, v)| v)
    }
}

/// Modules as a reader would say them.
pub fn module_words(m: &Manifest) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(p) = &m.physics {
        v.push(format!("physics ({p})"));
    }
    if m.net {
        v.push("multiplayer (net)".into());
    }
    if m.xr {
        v.push("VR (xr)".into());
    }
    if m.anim {
        v.push("models with animation (models, anim)".into());
    } else if m.models {
        v.push("3D models (models)".into());
    }
    if m.ui {
        v.push("game text and menus (ui)".into());
    }
    if m.live {
        v.push("live tuning (dev builds only, never in the download)".into());
    }
    v
}

/// The top of the card, for a non-technical reader. The worker pastes it into
/// its "Ready for review" report.
pub fn top_section(c: &Card) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "**Ready for review: {}** (kit {}, look {})", c.game, c.kit, c.look);
    let _ = writeln!(s);
    let _ = writeln!(s, "- **Speed test:** {}", bench_sentence(&c.bench));
    let size = match &c.previous {
        Some((p, basis)) => format!(
            "{} to download (compressed), {} {} ({}, {}).",
            kb(c.size.total_gz),
            delta_phrase(c.size.total_gz, p.total_gz),
            basis.describe(),
            p.app,
            p.when
        ),
        None => format!("{} to download (compressed); the first card, so nothing to compare with yet.", kb(c.size.total_gz)),
    };
    let _ = writeln!(s, "- **Download size:** {size}");
    let effects = if c.effects.is_empty() { "no effects".to_string() } else { format!("effects {}", join_and(&c.effects)) };
    let modules = if c.modules.is_empty() { "no optional modules".to_string() } else { join_and(&c.modules) };
    let onoff = if !c.onoff.extra.is_empty() {
        "WARNING: the download holds parts the game doesn't list"
    } else if !c.onoff.missing.is_empty() {
        "WARNING: parts the game lists are not in the download"
    } else {
        "nothing else is in the download"
    };
    let _ = writeln!(s, "- **What's in it:** {effects}; {modules}; {onoff}.");
    let pictures = match (&c.capture_error, c.shots.len()) {
        (Some(e), _) => format!("no pictures, the capture failed ({e})"),
        (None, 0) => "no pictures".to_string(),
        (None, n) if c.shots.iter().any(|x| x.moment.is_some()) => format!(
            "{n} scripted moments from the demo below, each on its own tier ({})",
            c.shots
                .iter()
                .map(|x| format!("{} on {}", x.moment.as_deref().unwrap_or("view").replace('_', " "), if x.tier.is_empty() { "the active tier" } else { &x.tier }))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        (None, n) => format!("{n} views from the demo below"),
    };
    let reference = match &c.reference {
        Reference::Spec { .. } => "next to the target picture description from the kit's spec".to_string(),
        Reference::Catalog { .. } => "next to a target picture description built from the catalog (the kit's spec has none)".to_string(),
        Reference::None { .. } => "no target picture description exists for this kit yet".to_string(),
    };
    let _ = writeln!(s, "- **Pictures:** {pictures}; {reference}.");
    let try_line = if c.try_list.is_empty() {
        "nothing listed yet (add `[review] try = [...]` to evani.toml or a \"What to try\" section to the kit README).".to_string()
    } else {
        c.try_list.iter().enumerate().map(|(i, t)| format!("({}) {t}", i + 1)).collect::<Vec<_>>().join(" ")
    };
    let _ = writeln!(s, "- **Try in the demo:** {try_line}");
    if let Some(h) = c.process.verdict(AutoCheck::HandOver) {
        let _ = writeln!(s, "- **Hand-over:** {} {}", if h.pass { "PASS." } else { "FAIL." }, h.evidence);
    }
    if !c.process.results.is_empty() {
        let with = |st: CheckStatus| -> Vec<&str> { c.process.results.iter().filter(|(_, v)| CheckStatus::from_verdict(v) == st).map(|(k, _)| k.label()).collect() };
        let (passed, failed, not_run, flagged, na) =
            (with(CheckStatus::Pass), with(CheckStatus::Fail), with(CheckStatus::NotRun), with(CheckStatus::Flag), with(CheckStatus::NotApplicable));
        let mut parts = Vec::new();
        for (word, list) in [("failed", &failed), ("not run", &not_run), ("flagged for a look", &flagged), ("not applicable", &na)] {
            if !list.is_empty() {
                parts.push(format!("{word}: {}", list.join(", ")));
            }
        }
        let _ = writeln!(
            s,
            "- **Process checks:** {} of {} passed{}.",
            passed.len(),
            c.process.results.len() - na.len(),
            if parts.is_empty() { String::new() } else { format!("; {}", parts.join("; ")) }
        );
    }
    if let Some(v) = c.process.verdict(AutoCheck::Critic) {
        let _ = writeln!(s, "- **Critic:** {} after {} of {} rounds. {}", v.label(), c.process.critic_rounds, critic::MAX_ROUNDS, v.evidence);
    }
    let count = |st: CheckStatus| c.checks.iter().filter(|i| i.status == st).count();
    let _ = writeln!(
        s,
        "- **Checks for QQ:** {} passed automatically, {} failed, {} to check by hand (the done-when checklist below).",
        count(CheckStatus::Pass),
        count(CheckStatus::Fail),
        count(CheckStatus::Manual)
    );
    s
}

fn ms(v: Option<f32>) -> String {
    v.map_or("n/a".into(), |x| format!("{x:.2}"))
}

pub fn render_markdown(c: &Card) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Kit review card: {}", c.app);
    let _ = writeln!(s);
    s.push_str(&top_section(c));
    let _ = writeln!(s);
    let _ = writeln!(s, "---");
    let _ = writeln!(s);
    let _ = writeln!(s, "## Details");
    let _ = writeln!(s);
    let _ = writeln!(s, "Made {} at commit `{}` by `cargo xtask review {}`.", c.when, c.commit, c.app);

    // Bench.
    let _ = writeln!(s);
    let _ = writeln!(s, "### Bench per tier");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "`apps/bench` (the engine's stress scene sized from each tier's limits, on the fixed camera path) in headless Chrome. GPU: {}. Browser: {}. Results: {}.",
        if c.bench.gpu.is_empty() { "unknown" } else { &c.bench.gpu },
        if c.bench.browser.is_empty() { "unknown" } else { &c.bench.browser },
        c.bench.source
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "| Tier | Result | Avg ms | p95 ms | GPU ms | Target ms | Failing check or note |");
    let _ = writeln!(s, "|---|---|---|---|---|---|---|");
    for l in &c.bench.lines {
        let (verdict, why) = match &l.verdict {
            TierVerdict::Pass => ("PASS".to_string(), l.note.clone()),
            TierVerdict::Fail => ("FAIL".to_string(), l.failures.join("; ")),
            TierVerdict::Unavailable(w) => ("unavailable".to_string(), w.clone()),
            TierVerdict::NoResult(w) => ("no result".to_string(), w.clone()),
        };
        let _ = writeln!(
            s,
            "| {} | {verdict} | {} | {} | {} | {} | {} |",
            l.label,
            ms(l.avg_ms),
            ms(l.p95_ms),
            ms(l.gpu_ms),
            ms(l.target_ms),
            if why.is_empty() { "-".into() } else { why.replace('|', "/") }
        );
    }
    for e in &c.bench.errors {
        let _ = writeln!(s, "\nRun error: {e}");
    }

    // Size.
    let _ = writeln!(s);
    let _ = writeln!(s, "### Download size (shipped build, no live module)");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Part | gzip |");
    let _ = writeln!(s, "|---|---|");
    let _ = writeln!(s, "| wasm | {} |", kb(c.size.wasm_gz));
    let _ = writeln!(s, "| js | {} |", kb(c.size.js_gz));
    let _ = writeln!(s, "| html (raw) | {} |", kb(c.html_raw));
    let _ = writeln!(s, "| **total** | **{}** |", kb(c.size.total_gz));
    let _ = writeln!(s);
    match &c.previous {
        Some((p, basis)) => {
            let _ = writeln!(
                s,
                "Compared with {}: `{}` (kit {}) at {}, commit `{}`, {}. Change: {}.",
                basis.describe(),
                p.app,
                p.kit,
                p.when,
                p.commit,
                kb(p.total_gz),
                describe_delta(c.size.total_gz, p.total_gz)
            );
        }
        None => {
            let _ = writeln!(s, "No earlier card to compare with.");
        }
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "Markers in the wasm: {}", c.markers.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(" "));
    let _ = writeln!(s);
    if c.onoff.pass() {
        let _ = writeln!(s, "Build-time on/off: PASS, nothing outside evani.toml reached the download and everything it lists is in it.");
    } else if !c.onoff.extra.is_empty() {
        let _ = writeln!(s, "Build-time on/off: FAIL, not listed in evani.toml but in the download: {}.", c.onoff.extra.join(", "));
    } else {
        let _ = writeln!(s, "Build-time on/off: FAIL, listed in evani.toml but not in the download.");
    }
    if !c.onoff.missing.is_empty() {
        let _ = writeln!(s, "\nListed but not found in the wasm (check the feature wiring): {}.", c.onoff.missing.join(", "));
    }
    if !c.onoff.shaderless.is_empty() {
        let _ = writeln!(s, "\nEffects with no shader of their own (params and CPU maths only, built in by their Cargo feature): {}.", c.onoff.shaderless.join(", "));
    }

    // Modules.
    let _ = writeln!(s);
    let _ = writeln!(s, "### Modules and effects in evani.toml");
    let _ = writeln!(s);
    let _ = writeln!(s, "- Game: `{}`, kit `{}`, look `{}`", c.game, c.kit, c.look);
    let _ = writeln!(s, "- Effects: {}", if c.effects.is_empty() { "none (no fx module)".into() } else { c.effects.join(", ") });
    let _ = writeln!(s, "- Modules: {}", if c.modules.is_empty() { "none".into() } else { c.modules.join(", ") });

    // Captures.
    let _ = writeln!(s);
    let _ = writeln!(s, "### Captures and the reference image prompt");
    let _ = writeln!(s);
    if let Some(e) = &c.capture_error {
        let _ = writeln!(s, "The capture failed: {e}");
    }
    if !c.shots.is_empty() {
        let _ = writeln!(s, "| {} |", c.shots.iter().map(Shot::heading).collect::<Vec<_>>().join(" | "));
        let _ = writeln!(s, "|{}", "---|".repeat(c.shots.len()));
        let _ = writeln!(s, "| {} |", c.shots.iter().map(|x| format!("![{}]({})", x.heading(), x.file)).collect::<Vec<_>>().join(" | "));
        let _ = writeln!(s);
        if c.shots.iter().any(|x| x.moment.is_some()) {
            let _ = writeln!(
                s,
                "Scripted moments (`capture.frame {{ moment }}`, the game's own camera), {} x {}, adaptive quality off:",
                c.shots[0].width, c.shots[0].height
            );
            let _ = writeln!(s);
            for x in &c.shots {
                let tier = if x.tier.is_empty() { "the active tier" } else { &x.tier };
                let _ = writeln!(s, "- **{}** on {tier}: {}", x.moment.as_deref().unwrap_or("view").replace('_', " "), x.label);
            }
        } else {
            let _ = writeln!(
                s,
                "Bench camera path at fixed sim times, {} x {}, tier {}.",
                c.shots[0].width,
                c.shots[0].height,
                c.shots.iter().map(|x| x.tier.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().first().copied().unwrap_or("active")
            );
        }
    }
    let _ = writeln!(s);
    match &c.reference {
        Reference::Spec { path, text } => {
            let _ = writeln!(s, "Reference image prompt (from `{path}`):\n\n```\n{text}\n```");
        }
        Reference::Catalog { text } => {
            let _ = writeln!(s, "Reference image prompt (built from the catalog; the kit's SPEC.md has none):\n\n```\n{text}\n```");
        }
        Reference::None { why } => {
            let _ = writeln!(s, "Reference image prompt: none. {why}");
        }
    }

    process_section(&mut s, c);

    // Try.
    let _ = writeln!(s);
    let _ = writeln!(s, "### What to try in the demo");
    let _ = writeln!(s);
    if c.try_list.is_empty() {
        let _ = writeln!(s, "Nothing listed ({}).", c.try_source);
    } else {
        for t in &c.try_list {
            let _ = writeln!(s, "- {t}");
        }
        let _ = writeln!(s, "\n(From {}.)", c.try_source);
    }

    // Checklist.
    let _ = writeln!(s);
    let _ = writeln!(s, "### Done-when checklist for QQ (docs/KIT_PROCESS.md)");
    let _ = writeln!(s);
    let _ = writeln!(s, "Ticked items were checked automatically; FAIL items go back to the worker; the rest QQ checks by hand.");
    let _ = writeln!(s);
    for i in &c.checks {
        let (mark, label) = i.status.mark();
        let _ = writeln!(s, "- [{mark}] **{}. {label}:** {}  \n  {}", i.n, i.text, i.evidence);
    }
    s
}

/// The "Process checks" details: one line per check, then the measurements.
fn process_section(s: &mut String, c: &Card) {
    let p = &c.process;
    if p.results.is_empty() {
        return;
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "### Process checks (automatic, docs/KIT_PROCESS.md items 11 and on)");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Check | Result | Evidence |");
    let _ = writeln!(s, "|---|---|---|");
    for (k, v) in &p.results {
        let _ = writeln!(s, "| {} | {} | {} |", k.label(), v.label(), v.evidence.replace('|', "/"));
    }
    if !p.showcase.shots.is_empty() || p.showcase.video.is_some() {
        let _ = writeln!(s);
        let _ = writeln!(s, "Showcase captures (whole page with the HUD, labelled with tier and engine commit):");
        let _ = writeln!(s);
        for x in &p.showcase.shots {
            let _ = writeln!(s, "- ![{} on {}]({}) {} x {}, label \"{}\"", x.moment, x.tier, x.file, x.width, x.height, x.label);
        }
        if let Some(v) = &p.showcase.video {
            let _ = writeln!(s);
            let file = if v.file.is_empty() { "no video file".to_string() } else { format!("[{}]({})", v.file, v.file) };
            let _ = writeln!(s, "Run video: {file}, {:.1} s. Frames: {}", v.seconds, v.frames.iter().map(|f| format!("![{f}]({f})")).collect::<Vec<_>>().join(" "));
        }
    }
    if let Some(r) = &p.critic {
        let _ = writeln!(s);
        let _ = writeln!(s, "Critic round {} ({} of {} rounds run), its verdict `{}`, mean of applicable items {}: {}", r.round, p.critic_rounds, critic::MAX_ROUNDS, r.verdict, p.critic_mean.map_or("n/a".into(), |m| format!("{m:.2}")), r.summary);
        let _ = writeln!(s);
        let _ = writeln!(s, "| Item | Score | Evidence |");
        let _ = writeln!(s, "|---|---|---|");
        for sc in &r.scores {
            let _ = writeln!(s, "| {} | {} | {} |", sc.id, if sc.applicable { sc.score.to_string() } else { "n/a".into() }, sc.evidence.replace('|', "/"));
        }
        if !r.must_fix.is_empty() {
            let _ = writeln!(s);
            let _ = writeln!(s, "Must fix:");
            for m in &r.must_fix {
                let _ = writeln!(s, "- {m}");
            }
        }
    }
    let _ = writeln!(s);
    let gpu = p.tier_gpu.iter().map(|(t, g)| format!("{t} {} ms", ms(*g))).collect::<Vec<_>>().join(" -> ");
    let _ = writeln!(s, "GPU time per tier ({}): {gpu}.", p.tier_gpu_source);
    if !p.edges.is_empty() {
        let _ = writeln!(s);
        let _ = writeln!(s, "| Camera | Background pixels | Edge pixels | Edge share | Deepest below horizon |");
        let _ = writeln!(s, "|---|---|---|---|---|");
        for (name, v) in &p.edges {
            match v {
                Ok(v) => {
                    let _ = writeln!(
                        s,
                        "| {name} | {} | {} | {:.2}% | {:.1} deg (allowed {:.1}) |",
                        v.bg_px,
                        v.edge_px,
                        v.share() * 100.0,
                        v.max_below_deg,
                        v.tolerance_deg
                    );
                }
                Err(e) => {
                    let _ = writeln!(s, "| {name} | - | - | - | could not be checked: {} |", e.replace('|', "/"));
                }
            }
        }
    }
    if !p.captures.is_empty() {
        let _ = writeln!(s);
        let _ = writeln!(
            s,
            "HUD selectors declared in evani.toml: {}.{}",
            if p.hud_declared.is_empty() { "none".to_string() } else { p.hud_declared.join(", ") },
            if p.hud_declared.iter().any(|h| h == "canvas") {
                " `canvas` is a HUD drawn inside the game picture (the kit's HUD sprite layers), counted in sprites, so it also shows in the capture pictures above."
            } else {
                ""
            }
        );
        let _ = writeln!(s);
        let _ = writeln!(s, "| Capture | Debug overlay | HUD |");
        let _ = writeln!(s, "|---|---|---|");
        for cap in &p.captures {
            let hud = if cap.hud.is_empty() {
                "none declared".to_string()
            } else {
                cap.hud
                    .iter()
                    .map(|h| {
                        let unit = if h.selector == "canvas" { "sprites" } else { "chars" };
                        format!("`{}` {}", h.selector, if h.visible { format!("visible ({} {unit})", h.text_len) } else { "not visible".into() })
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            let _ = writeln!(s, "| {} | {} | {hud} |", cap.name, if cap.overlay_visible { "VISIBLE" } else { "hidden" });
        }
    }
    if !p.page_shots.is_empty() {
        let _ = writeln!(s);
        if !p.hud_declared.is_empty() && p.hud_declared.iter().all(|h| h == "canvas") {
            let _ = writeln!(s, "Page screenshots (the whole page as a player sees it; this kit draws its HUD inside the canvas, so the capture pictures above show it too):");
        } else {
            let _ = writeln!(s, "Page screenshots (the DOM HUD over the canvas; the capture pictures above come from the render target and never show the DOM):");
        }
        let _ = writeln!(s);
        let _ = writeln!(s, "| {} |", p.page_shots.iter().map(|(n, _)| n.replace('_', " ")).collect::<Vec<_>>().join(" | "));
        let _ = writeln!(s, "|{}", "---|".repeat(p.page_shots.len()));
        let _ = writeln!(s, "| {} |", p.page_shots.iter().map(|(n, f)| format!("![{n}]({f})")).collect::<Vec<_>>().join(" | "));
    }
}

pub fn card_json(c: &Card) -> Value {
    let verdict = |v: &TierVerdict| match v {
        TierVerdict::Pass => json!({"result": "pass"}),
        TierVerdict::Fail => json!({"result": "fail"}),
        TierVerdict::Unavailable(w) => json!({"result": "unavailable", "why": w}),
        TierVerdict::NoResult(w) => json!({"result": "no_result", "why": w}),
    };
    let reference = match &c.reference {
        Reference::Spec { path, text } => json!({"source": "spec", "path": path, "text": text}),
        Reference::Catalog { text } => json!({"source": "catalog", "text": text}),
        Reference::None { why } => json!({"source": "none", "why": why}),
    };
    json!({
        "app": c.app, "game": c.game, "kit": c.kit, "look": c.look, "when": c.when, "commit": c.commit,
        "top": top_section(c),
        "bench": {
            "passes": c.bench.passes(), "gpu": c.bench.gpu, "browser": c.bench.browser, "source": c.bench.source,
            "errors": c.bench.errors,
            "tiers": c.bench.lines.iter().map(|l| {
                let mut v = verdict(&l.verdict);
                if let Some(o) = v.as_object_mut() {
                    o.insert("tier".into(), json!(l.id));
                    o.insert("label".into(), json!(l.label));
                    o.insert("failures".into(), json!(l.failures));
                    o.insert("avg_ms".into(), json!(l.avg_ms));
                    o.insert("p95_ms".into(), json!(l.p95_ms));
                    o.insert("gpu_ms".into(), json!(l.gpu_ms));
                    o.insert("target_ms".into(), json!(l.target_ms));
                    o.insert("note".into(), json!(l.note));
                }
                v
            }).collect::<Vec<_>>(),
        },
        "size": {
            "total_gz": c.size.total_gz, "wasm_gz": c.size.wasm_gz, "js_gz": c.size.js_gz, "html_raw": c.html_raw,
            "previous": c.previous.as_ref().map(|(p, b)| json!({
                "card": p.to_json(), "basis": b.describe(),
                "delta_bytes": c.size.total_gz as i64 - p.total_gz as i64,
                "delta": describe_delta(c.size.total_gz, p.total_gz),
            })),
            "markers": c.markers,
            "onoff": {"pass": c.onoff.pass(), "extra": c.onoff.extra, "missing": c.onoff.missing, "shaderless": c.onoff.shaderless},
        },
        "modules": c.modules, "effects": c.effects,
        "captures": c.shots.iter().map(|x| json!({"file": x.file, "t": x.t, "width": x.width, "height": x.height, "tier": x.tier, "moment": x.moment, "label": x.label})).collect::<Vec<_>>(),
        "capture_error": c.capture_error,
        "reference": reference,
        "try": c.try_list, "try_source": c.try_source,
        "handover": {
            "size_gz": c.process.handover.size_gz, "engine": c.process.handover.engine,
            "main_ref": c.process.handover.main_ref, "main": c.process.handover.main,
            "dirty": c.process.handover.dirty,
        },
        "process": {
            "checks": c.process.results.iter().map(|(k, v)| json!({"check": k.label(), "pass": v.pass, "result": CheckStatus::from_verdict(v).json(), "evidence": v.evidence})).collect::<Vec<_>>(),
            "showcase": c.process.showcase.shots.iter().map(|x| json!({"moment": x.moment, "tier": x.tier, "file": x.file, "width": x.width, "height": x.height, "label": x.label})).collect::<Vec<_>>(),
            "video": c.process.showcase.video.as_ref().map(|v| json!({"file": v.file, "seconds": v.seconds, "frames": v.frames})),
            "critic": c.process.critic.as_ref().map(|r| json!({
                "round": r.round, "rounds_run": c.process.critic_rounds, "max_rounds": critic::MAX_ROUNDS,
                "verdict": r.verdict, "mean": c.process.critic_mean, "must_fix": r.must_fix, "summary": r.summary,
                "scores": r.scores.iter().map(|s| json!({"id": s.id, "score": s.score, "applicable": s.applicable, "evidence": s.evidence})).collect::<Vec<_>>(),
            })),
            "tier_gpu_ms": c.process.tier_gpu.iter().map(|(t, g)| json!({"tier": t, "gpu_ms": g})).collect::<Vec<_>>(),
            "tier_gpu_source": c.process.tier_gpu_source,
            "world_edge": c.process.edges.iter().map(|(n, v)| match v {
                Ok(v) => json!({"camera": n, "background_px": v.bg_px, "edge_px": v.edge_px, "total_px": v.total_px, "edge_share": v.share(), "max_below_deg": v.max_below_deg, "tolerance_deg": v.tolerance_deg}),
                Err(e) => json!({"camera": n, "error": e}),
            }).collect::<Vec<_>>(),
            "hud_declared": c.process.hud_declared,
            "captures": c.process.captures.iter().map(|k| json!({
                "name": k.name, "overlay_visible": k.overlay_visible,
                "hud": k.hud.iter().map(|h| json!({"selector": h.selector, "matched": h.matched, "visible": h.visible, "text_len": h.text_len})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "page_shots": c.process.page_shots.iter().map(|(n, f)| json!({"name": n, "file": f})).collect::<Vec<_>>(),
        },
        "checklist": c.checks.iter().map(|i| json!({
            "n": i.n, "text": i.text, "evidence": i.evidence,
            "status": i.status.json(),
        })).collect::<Vec<_>>(),
    })
}

// ---- gathering the inputs ---------------------------------------------------------

/// `apps/<name>`, or the app whose evani.toml names kit `<name>`.
pub fn resolve_app(root: &Path, name: &str) -> Result<String, String> {
    let name = name.trim_start_matches("apps/").trim_start_matches("apps\\");
    if root.join("apps").join(name).join("evani.toml").exists() {
        return Ok(name.to_string());
    }
    let rd = std::fs::read_dir(root.join("apps")).map_err(|e| format!("apps/: {e}"))?;
    for e in rd.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("evani.toml")) else {
            continue;
        };
        if Manifest::parse(&text).is_ok_and(|m| m.kit == name) {
            return Ok(e.file_name().to_string_lossy().into_owned());
        }
    }
    Err(format!("no app apps/{name} and no app whose evani.toml has kit = \"{name}\""))
}

fn walk_files(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let skip = p.file_name().is_some_and(|n| n == "target" || n == "dist" || n == "node_modules");
        if p.is_dir() {
            if !skip {
                walk_files(&p, f);
            }
        } else {
            f(&p);
        }
    }
}

fn markers_under(dirs: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for d in dirs {
        walk_files(d, &mut |p| {
            if p.extension().is_some_and(|x| x == "rs" || x == "wgsl")
                && let Ok(t) = std::fs::read_to_string(p)
            {
                markers_in_text(&t, &mut out);
            }
        });
    }
    out.sort();
    out.dedup();
    out
}

fn newest_mtime(dirs: &[PathBuf]) -> Option<SystemTime> {
    let mut newest = None;
    for d in dirs {
        walk_files(d, &mut |p| {
            if let Ok(t) = p.metadata().and_then(|m| m.modified())
                && newest.is_none_or(|n| t > n)
            {
                newest = Some(t);
            }
        });
    }
    newest
}

/// "2026-10-14 13:05 UTC" from a system time.
pub fn utc_string(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

fn git_commit(root: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Saved bench results, rerun when stale (or asked).
fn gather_bench(root: &Path, a: &ReviewArgs) -> BenchInfo {
    let tiers: Vec<(String, String)> = match Tiers::builtin() {
        Ok(t) => t.presets().iter().map(|p| (p.id.clone(), p.label.clone())).collect(),
        Err(e) => return BenchInfo { errors: vec![e], ..BenchInfo::default() },
    };
    let saved = root.join("target").join("evani-bench").join("results.json");
    let saved_at = saved.metadata().and_then(|m| m.modified()).ok();
    let sources = newest_mtime(&[
        root.join("crates"),
        root.join("apps").join("bench"),
        root.join("assets").join("tiers"),
        root.join("Cargo.lock"),
    ]);
    let fresh_enough = matches!((saved_at, sources), (Some(s), Some(src)) if s > src);
    let mut source = String::new();
    let mut run_error = None;
    if a.rebench || !fresh_enough {
        eprintln!(
            "  running the bench ({})",
            if a.rebench { "--rebench" } else if saved_at.is_none() { "no saved results" } else { "saved results are older than the engine sources" }
        );
        let started = SystemTime::now();
        let mut args = Vec::new();
        if let Some(c) = &a.chrome {
            args.extend(["--chrome".to_string(), c.clone()]);
        }
        // A failing tier is a finding (the run still saved its results); only a
        // run that saved nothing is an error here.
        if let Err(e) = crate::bench::cmd_bench(&args) {
            let wrote = saved.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= started);
            if !wrote {
                run_error = Some(format!("the bench did not run: {e}"));
            }
        }
        source = format!("fresh run, {}", utc_string(SystemTime::now()));
    } else if let Some(t) = saved_at {
        source = format!("saved results from {} (newer than every engine source; `--rebench` reruns)", utc_string(t));
    }
    let mut info = match std::fs::read(&saved).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        Some(json) if run_error.is_none() => bench_lines(&tiers, &json),
        _ => {
            let why = run_error.clone().unwrap_or_else(|| "no saved results".into());
            let lines = tiers
                .iter()
                .map(|(id, label)| TierLine {
                    id: id.clone(),
                    label: label.clone(),
                    verdict: TierVerdict::NoResult(why.clone()),
                    failures: Vec::new(),
                    avg_ms: None,
                    p95_ms: None,
                    gpu_ms: None,
                    gpu_p50_ms: None,
                    target_ms: None,
                    note: String::new(),
                })
                .collect();
            BenchInfo { lines, errors: vec![why], ..BenchInfo::default() }
        }
    };
    info.source = source;
    // The stress-scene run (not rerun here: five runs of four tiers take
    // minutes), when it is newer than the engine sources.
    let stress = root.join("target").join("evani-bench").join("stress").join("summary.json");
    let stress_at = stress.metadata().and_then(|m| m.modified()).ok();
    if matches!((stress_at, sources), (Some(s), Some(src)) if s > src)
        && let Some((gpu, runs)) = std::fs::read(&stress)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|j| crate::bench::stress_order_inputs(&j))
    {
        info.stress_gpu = gpu;
        info.stress_runs = runs;
    }
    info
}

/// The showcase tiers (item 18).
pub const SHOWCASE_TIERS: [&str; 2] = ["low", "ultra"];
pub const VIDEO_SECONDS: u32 = 10;

/// What the capture script's showcase phase shoots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShowcasePlan {
    pub moments: Vec<String>,
    pub tiers: Vec<String>,
    pub size: (u32, u32),
    pub engine: String,
    pub video: String,
    pub video_seconds: u32,
    /// The run video's frame size: a whole multiple of the internal
    /// resolution that fits [`VIDEO_MAX`] (pixel art stays crisp, e.g.
    /// 480x270 -> 960x540), or `None` for the capture script's default.
    pub video_size: Option<(u32, u32)>,
}

/// The largest frame the review's screencast records.
pub const VIDEO_MAX: (u32, u32) = (1280, 720);

/// A whole multiple of `internal` that fits [`VIDEO_MAX`] (critic round 2 of
/// kit 002: a 1280x720 recording of a 480x270 view scales each art pixel to
/// 2.67 pixels and blurs it).
pub fn video_size(internal: Option<(u32, u32)>) -> Option<(u32, u32)> {
    let (w, h) = internal?;
    let k = (VIDEO_MAX.0 / w.max(1)).min(VIDEO_MAX.1 / h.max(1));
    (k >= 1).then_some((w * k, h * k))
}

/// The target pack's files in `kits/<id>/target/`.
fn read_pack(dir: &Path) -> kitchecks::TargetPack {
    let f = |n: &str| read(&dir.join(n));
    let known = ["reference-prompt.md", "look-bible.md", "moments.md", "hud-layout.md", "playbook.md"];
    let others = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.ends_with(".md") && !known.contains(&n.as_str())
                })
                .filter_map(|e| read(&e.path()))
                .collect()
        })
        .unwrap_or_default();
    kitchecks::TargetPack {
        exists: dir.is_dir(),
        reference_prompt: f("reference-prompt.md"),
        reference_image: ["png", "jpg", "webp"].iter().any(|x| dir.join(format!("reference.{x}")).exists()),
        look_bible: f("look-bible.md"),
        moments: f("moments.md"),
        hud_layout: f("hud-layout.md"),
        playbook: f("playbook.md"),
        others,
    }
}

/// Every `.rs` file of the kit crate, concatenated (to find the test names).
fn kit_source(dir: &Path) -> String {
    let mut s = String::new();
    walk_files(dir, &mut |p| {
        if p.extension().is_some_and(|x| x == "rs")
            && let Some(t) = read(p)
        {
            s.push_str(&t);
            s.push('\n');
        }
    });
    s
}

/// `(tests or None, why not, dump or None, why not)`.
type KitTestResults = (Option<Vec<(String, bool)>>, String, Option<kitchecks::ReviewDump>, String);

/// The kit's playtest tests and review dump (`cargo test -p evani-kit-<id>
/// --lib -- playtest_ review_dump` with `EVANI_REVIEW_DUMP` set), when the
/// kit's source has them; otherwise why not, without compiling anything.
fn run_kit_tests(root: &Path, kit: &str, out: &Path) -> KitTestResults {
    run_kit_tests_in(root, &root.join("kits").join(format!("evani-kit-{kit}")), &format!("evani-kit-{kit}"), out)
}

/// [`run_kit_tests`] for the kit crate in `kit_dir` (package `pkg`), with
/// cargo run in `cwd` (the engine, or an outside game's workspace).
fn run_kit_tests_in(cwd: &Path, kit_dir: &Path, pkg: &str, out: &Path) -> KitTestResults {
    let hooks = kitchecks::kit_hooks(&kit_source(kit_dir));
    let no_tests = "the kit crate has no playtest_win* / playtest_lose* tests".to_string();
    let no_dump = "the kit crate has no review_dump test writing $EVANI_REVIEW_DUMP/review.json".to_string();
    if !hooks.any() {
        return (None, no_tests, None, no_dump);
    }
    let dump_dir = out.join("dump");
    let _ = std::fs::remove_dir_all(&dump_dir);
    let _ = std::fs::create_dir_all(&dump_dir);
    eprintln!("  running the kit's playtest and review_dump tests");
    let o = Command::new("cargo")
        .args(["test", "-p", pkg, "--lib", "--", "playtest_", "review_dump"])
        .env("EVANI_REVIEW_DUMP", &dump_dir)
        .current_dir(cwd)
        .stderr(Stdio::inherit())
        .output();
    let text = match o {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(e) => {
            let why = format!("cargo test could not start: {e}");
            return (None, why.clone(), None, why);
        }
    };
    let tests = kitchecks::parse_libtest(&text);
    let (tests, tests_why) = if hooks.playtest_win || hooks.playtest_lose { (Some(tests), String::new()) } else { (None, no_tests) };
    let (dump, dump_why) = if !hooks.review_dump {
        (None, no_dump)
    } else {
        match read(&dump_dir.join("review.json")).map(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()).and_then(|v| kitchecks::parse_review_dump(&v))) {
            Some(Ok(d)) => (Some(d), String::new()),
            Some(Err(e)) => (None, format!("review.json: {e}")),
            None => (None, "the review_dump test wrote no review.json (did it pass?)".into()),
        }
    };
    (tests, tests_why, dump, dump_why)
}

/// A capture file and its difference from the accepted twin, if it has one.
type RegressionResult = (String, Option<Result<f64, String>>);

/// Each showcase capture against its twin in `accepted/`.
fn regression_inputs(out: &Path, sc: &kitchecks::Showcase) -> (Vec<RegressionResult>, bool) {
    let acc = out.join("accepted");
    let accepted_any = std::fs::read_dir(&acc).is_ok_and(|mut rd| rd.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("shot-"))));
    let load = |p: &Path| std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| checks::decode_png(&b));
    let results = sc
        .shots
        .iter()
        .map(|s| {
            let twin = acc.join(&s.file);
            let r = twin.exists().then(|| {
                let (now, then) = (load(&out.join(&s.file))?, load(&twin)?);
                kitchecks::diff_share(&now, &then, &s.label_rect.into_iter().collect::<Vec<_>>())
            });
            (s.file.clone(), r)
        })
        .collect();
    (results, accepted_any)
}

/// `--accept`: keep the last showcase captures as the regression baseline.
fn accept_shots(out: &Path) -> Result<usize, String> {
    let acc = out.join("accepted");
    std::fs::create_dir_all(&acc).map_err(|e| format!("{}: {e}", acc.display()))?;
    let mut n = 0;
    for f in std::fs::read_dir(out).map_err(|e| format!("{}: {e}", out.display()))?.flatten() {
        let name = f.file_name().to_string_lossy().into_owned();
        if name.starts_with("shot-") && name.ends_with(".png") {
            std::fs::copy(f.path(), acc.join(&name)).map_err(|e| format!("{name}: {e}"))?;
            n += 1;
        }
    }
    if n == 0 {
        return Err("no showcase captures (shot-*.png) to accept; run `cargo xtask review` first".into());
    }
    Ok(n)
}

/// The catalog look's `style_class` is "2d", and its `render_rules`.
fn look_facts(root: &Path, look: &str) -> (bool, String) {
    let Some(v) = read(&root.join("catalog").join("looks.json")).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        return (false, String::new());
    };
    match find_by_id(&v, "looks", look) {
        Some(l) => (
            l.get("style_class").and_then(Value::as_str) == Some("2d"),
            l.get("render_rules").and_then(Value::as_str).unwrap_or("").to_string(),
        ),
        None => (false, String::new()),
    }
}

/// `[review] hud` split into the canvas HUD (the entry `canvas`: a HUD the
/// kit draws inside the canvas on its declared HUD sprite layers) and the DOM
/// selectors.
fn hud_entries(hud: &[String]) -> (bool, Vec<String>) {
    let canvas = hud.iter().any(|h| h == "canvas");
    (canvas, hud.iter().filter(|h| *h != "canvas").cloned().collect())
}

/// The captures through tools/live-check/capture.mjs: the `[review]
/// moments` when evani.toml lists any (moments mode: each on its tier,
/// adaptive quality off), else three bench-path views (review mode).
fn gather_shots(root: &Path, app: &str, app_dir: Option<&Path>, out: &Path, a: &ReviewArgs, m: &Manifest, show: &ShowcasePlan) -> Result<(Vec<Shot>, Value), String> {
    let moments = &m.review_moments;
    // The last run's showcase and video.
    if let Ok(dir) = std::fs::read_dir(out) {
        for f in dir.flatten() {
            let n = f.file_name().to_string_lossy().into_owned();
            if (n.starts_with("shot-") && n.ends_with(".png")) || n.starts_with("video-frame-") || n == "video.webm" {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    for i in 1..=CAPTURE_TIMES.len() {
        let _ = std::fs::remove_file(out.join(format!("capture-{i}.png")));
    }
    for mo in moments {
        let _ = std::fs::remove_file(out.join(format!("moment-{}.png", mo.name)));
    }
    // The probe files of the last run (page screenshots, clear-colour pairs).
    if let Ok(dir) = std::fs::read_dir(out) {
        for f in dir.flatten() {
            let n = f.file_name().to_string_lossy().into_owned();
            if n.ends_with(".png") && (n.starts_with("page-") || n.starts_with("geo-")) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    let times: Vec<String> = CAPTURE_TIMES.iter().map(|t| t.to_string()).collect();
    let mut cmd = Command::new("node");
    cmd.arg(root.join("tools").join("live-check").join("capture.mjs"));
    // A game outside the engine: the bridge serves its folder (`serve --app-dir`).
    match app_dir {
        Some(d) => cmd.arg("--app-dir").arg(d),
        None => cmd.args(["--game", app]),
    };
    cmd.args(["--width", &CAPTURE_WIDTH.to_string()]);
    if moments.is_empty() {
        cmd.args(["--times", &times.join(",")]);
    } else {
        cmd.args(["--moments", &moments.iter().map(crate::ReviewMoment::arg).collect::<Vec<_>>().join(",")]);
    }
    // `hud = ["canvas"]` is a HUD drawn inside the canvas (the HUD sprite
    // layers the kit declares), read from each capture's reply; it never goes
    // to the DOM probe, where `canvas` would match the game canvas itself.
    let (canvas_hud, dom_hud) = hud_entries(&m.review_hud);
    if !dom_hud.is_empty() {
        cmd.args(["--hud", &dom_hud.join(",")]);
    }
    if canvas_hud {
        cmd.args(["--canvas-hud", "1"]);
    }
    if let Some(dpr) = m.review_phone_dpr {
        cmd.args(["--phone-dpr", &dpr.to_string()]);
    }
    if !show.moments.is_empty() {
        cmd.args(["--showcase", &show.tiers.join(","), "--showcase-moments", &show.moments.join(",")]);
        cmd.args(["--showcase-size", &format!("{}x{}", show.size.0, show.size.1), "--engine", &show.engine]);
        cmd.args(["--video", &show.video, "--video-seconds", &show.video_seconds.to_string()]);
        if let Some((w, h)) = show.video_size {
            cmd.args(["--video-size", &format!("{w}x{h}")]);
        }
    }
    cmd.arg("--out")
        .arg(out)
        .current_dir(root)
        .env_remove("NO_COLOR")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "4");
    }
    cmd.args(["--port", &capture_port(a).to_string()]);
    if let Some(c) = &a.chrome {
        cmd.args(["--chrome", c]);
    }
    let o = cmd.output().map_err(|e| format!("cannot run node ({e}); the capture needs Node 22 or newer"))?;
    if !o.status.success() {
        return Err("the capture script failed (see the lines above)".into());
    }
    let v: Value = serde_json::from_slice(&o.stdout).map_err(|e| format!("unreadable capture output: {e}"))?;
    let shots = shots_from(&v, if moments.is_empty() { CAPTURE_TIMES.len() } else { moments.len() })?;
    Ok((shots, v))
}

/// Run git in the repo; its trimmed stdout when it succeeded.
fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The engine is `crates/` and `assets/tiers/`, except each crate's
/// Cargo.toml (a kit adds its own features there). Cargo.lock lives outside.
const ENGINE_PATHS: [&str; 3] = ["crates", "assets/tiers", ":(exclude,glob)crates/**/Cargo.toml"];

/// The newest commit of `rev` that touched the engine.
fn engine_commit(root: &Path, rev: &str) -> Option<String> {
    let mut args = vec!["log", "-1", "--format=%h", rev, "--"];
    args.extend(ENGINE_PATHS);
    git_out(root, &args).filter(|s| !s.is_empty())
}

/// The engine has uncommitted changes.
pub(crate) fn engine_dirty(root: &Path) -> bool {
    let mut args = vec!["status", "--porcelain", "--"];
    args.extend(ENGINE_PATHS);
    git_out(root, &args).is_some_and(|s| !s.is_empty())
}

/// The main line: the given ref, else `main` when it exists, else `m1-foundation`.
fn main_ref(root: &Path, given: Option<&str>) -> String {
    if let Some(g) = given {
        return g.to_string();
    }
    if git_out(root, &["rev-parse", "--verify", "--quiet", "main"]).is_some() { "main".into() } else { "m1-foundation".into() }
}

/// The black and white renders of every scripted camera, read from `out`
/// and analysed; a camera whose files can't be read is an error entry.
fn edge_views(out: &Path, geo: &[checks::GeoFiles], p: &EdgeParams) -> Vec<(String, Result<EdgeView, String>)> {
    geo.iter()
        .map(|g| {
            let load = |f: &str| -> Result<checks::Rgb, String> {
                let bytes = std::fs::read(out.join(f)).map_err(|e| format!("{f}: {e}"))?;
                checks::decode_png(&bytes).map_err(|e| format!("{f}: {e}"))
            };
            let r = (|| {
                let pose = g.pose.as_ref().ok_or("the capture reply carried no camera pose")?;
                let (b, w) = (load(&g.black)?, load(&g.white)?);
                checks::edge_view(&b, &w, pose, p)
            })();
            (g.name.clone(), r)
        })
        .collect()
}

/// Everything the process checks need once it is measured.
pub struct ProcessInputs<'a> {
    pub kit: &'a str,
    pub bench: &'a BenchInfo,
    /// The capture's probe, or why there is none.
    pub probe: Result<&'a checks::Probe, &'a str>,
    pub hud_declared: &'a [String],
    /// The kit has a 3D ground to fall off (`[review] world_edge`, default true).
    pub world_edge_applies: bool,
    pub edges: Vec<(String, Result<EdgeView, String>)>,
    pub handover: HandOver,
    pub size_text: String,
    pub kit_inputs: KitInputs,
}

/// What the kit-process checks (items 16 on, `kitchecks.rs`) read, gathered
/// by `cmd_review`.
#[derive(Debug, Clone, Default)]
pub struct KitInputs {
    pub pack: kitchecks::TargetPack,
    /// The signature moments the showcase captures (target/moments.md, else `[review] moments`).
    pub moments: Vec<String>,
    pub tiers: Vec<String>,
    pub showcase: kitchecks::Showcase,
    /// The engine commit the labels must name.
    pub engine: String,
    pub internal: Option<(u32, u32)>,
    pub regression: Vec<RegressionResult>,
    pub accepted_any: bool,
    /// The kit's review dump, or why there is none.
    pub dump: Option<kitchecks::ReviewDump>,
    pub dump_why: String,
    /// The kit's test results, or why they did not run.
    pub tests: Option<Vec<(String, bool)>>,
    pub tests_why: String,
    pub geocheck: Option<Value>,
    pub is_2d: bool,
    pub critic: Option<Result<CriticReport, String>>,
    pub critic_rounds: usize,
    pub stage: u32,
    /// `None`: not a playground; else what keeps it from free roam (empty: nothing).
    pub playground: Option<Vec<String>>,
}

/// The process checks (items 11 on), each from its own function in
/// `checks.rs` or `kitchecks.rs`. Add a check by adding one entry here.
pub fn process_info(i: ProcessInputs<'_>) -> ProcessInfo {
    let (tier_gpu, tier_gpu_source) = i.bench.order_inputs();
    let no_capture = |why: &str| Verdict::fail(format!("No capture session ran, so this could not be measured ({why})."));
    let k = &i.kit_inputs;
    let results = AutoCheck::ALL
        .into_iter()
        .filter(|c| c.is_process())
        .map(|c| {
            let v = match c {
                AutoCheck::TierCostOrder => checks::tier_cost_order(&tier_gpu, &tier_gpu_source),
                AutoCheck::Architecture => i.probe.map_or_else(no_capture, |p| checks::architecture(i.kit, &p.arch)),
                AutoCheck::CaptureHud => i.probe.map_or_else(no_capture, |p| checks::capture_hud(i.hud_declared, &p.captures)),
                AutoCheck::WorldEdge if !i.world_edge_applies => {
                    Verdict::pass("Not applicable: evani.toml sets `[review] world_edge = false` (the game has no 3D ground to fall off).")
                }
                AutoCheck::WorldEdge => i.probe.map_or_else(no_capture, |_| checks::world_edge(&i.edges, &EdgeParams::default())),
                AutoCheck::HandOver => checks::handover(&i.handover, &i.size_text),
                AutoCheck::TargetPack => kitchecks::target_pack(i.kit, &k.pack),
                AutoCheck::CaptureSet => {
                    let tiers: Vec<&str> = k.tiers.iter().map(String::as_str).collect();
                    kitchecks::capture_set(&k.moments, &tiers, &k.showcase, &k.engine, k.internal)
                }
                AutoCheck::ContactShadows => kitchecks::grounded(k.dump.as_ref().and_then(|d| d.objects.as_deref()), k.is_2d, &k.dump_why),
                AutoCheck::Colours => kitchecks::colours(k.dump.as_ref().and_then(|d| d.palette.as_ref()), &k.dump_why),
                AutoCheck::Regression => kitchecks::regression(&k.regression, k.accepted_any),
                AutoCheck::Playtest => kitchecks::playtest(k.tests.as_deref(), &k.tests_why),
                AutoCheck::Geocheck => kitchecks::geocheck(k.geocheck.as_ref()),
                AutoCheck::CameraFraming => kitchecks::geocheck_area(k.geocheck.as_ref(), "camera", "Camera framing", k.is_2d),
                AutoCheck::BotLevel => kitchecks::geocheck_area(k.geocheck.as_ref(), "bot", "Bot level checks", k.is_2d),
                AutoCheck::UiText => kitchecks::geocheck_area(k.geocheck.as_ref(), "popups", "UI text rule", k.is_2d),
                AutoCheck::Critic => critic::critic_verdict(k.critic.as_ref(), k.critic_rounds, k.stage),
                AutoCheck::FreeRoam => kitchecks::free_roam(k.playground.as_deref()),
                AutoCheck::BuildAndSize | AutoCheck::Bench | AutoCheck::Docs => Verdict::fail("not a process check"),
            };
            (c, v)
        })
        .collect();
    ProcessInfo {
        results,
        tier_gpu,
        tier_gpu_source,
        edges: i.edges,
        captures: i.probe.map(|p| p.captures.clone()).unwrap_or_default(),
        hud_declared: i.hud_declared.to_vec(),
        page_shots: i.probe.map(|p| p.page_shots.clone()).unwrap_or_default(),
        handover: i.handover,
        showcase: k.showcase.clone(),
        critic: k.critic.as_ref().and_then(|r| r.as_ref().ok()).cloned(),
        critic_rounds: k.critic_rounds,
        critic_mean: k.critic.as_ref().and_then(|r| r.as_ref().ok()).map(|r| critic::judge(r, k.stage).mean),
    }
}

/// Shots from capture.mjs's summary; `expected` captures must be there.
pub fn shots_from(v: &Value, expected: usize) -> Result<Vec<Shot>, String> {
    let caps = v.get("captures").and_then(Value::as_array).ok_or("the capture returned no captures")?;
    let shots: Vec<Shot> = caps
        .iter()
        .map(|c| {
            let file = c.get("file").and_then(Value::as_str).unwrap_or("");
            Shot {
                file: Path::new(file).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
                t: c.get("t").and_then(Value::as_f64).unwrap_or(0.0),
                width: c.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                height: c.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                tier: c
                    .pointer("/stats/tier")
                    .or_else(|| c.get("tier"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                moment: c.get("moment").and_then(Value::as_str).map(str::to_string),
                label: c.get("label").and_then(Value::as_str).unwrap_or("").to_string(),
            }
        })
        .collect();
    if shots.len() != expected {
        return Err(format!("expected {expected} captures, got {}", shots.len()));
    }
    Ok(shots)
}

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.exists()).cloned()
}

/// Where one reviewed game's parts are: in the engine (`apps/<id>`,
/// `kits/evani-kit-<kit>`, the target pack in `kits/<kit>`) or, for
/// `--app-dir`, in its own workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// Where the review, geocheck and history files live: the engine root,
    /// or the game's workspace.
    pub home: PathBuf,
    /// The kit crate's folder and package.
    pub kit_crate: PathBuf,
    pub kit_package: String,
    /// The target pack's folder (`<it>/target`, `<it>/SPEC.md`).
    pub kit_pack: PathBuf,
    /// The app folder as messages name it.
    pub shown: String,
    /// The app folder for `--app-dir` (an outside game), else `None`.
    pub app_dir: Option<PathBuf>,
}

impl Place {
    /// A game in the engine's `apps/`.
    pub fn in_engine(root: &Path, app: &str, kit: &str) -> Self {
        Self {
            home: root.to_path_buf(),
            kit_crate: root.join("kits").join(format!("evani-kit-{kit}")),
            kit_package: format!("evani-kit-{kit}"),
            kit_pack: root.join("kits").join(kit),
            shown: format!("apps/{app}"),
            app_dir: None,
        }
    }

    /// A game outside the engine (`--app-dir`): its workspace, its kit crate
    /// (as `build --app-dir` finds it) and its target pack, which is
    /// `<kit crate>/../<kit>/target` (beside the kit crate, as in the
    /// engine), else `<game workspace>/kits/<kit>/target`; neither is an
    /// ERROR naming both (docs/KIT_PROCESS.md, step 2).
    pub fn outside(app: &crate::AppDir) -> Result<Self, String> {
        let ext = app.external.as_ref().ok_or("not a game outside the engine")?;
        let kit = &app.manifest.kit;
        let sibling = ext.kit_dir.parent().map(|p| p.join(kit));
        let in_ws = ext.root.join("kits").join(kit);
        let candidates: Vec<PathBuf> = sibling.iter().cloned().chain([in_ws.clone()]).collect();
        let kit_pack = match candidates.into_iter().find(|p| p.join("target").is_dir()) {
            Some(p) => p,
            None => {
                return Err(format!(
                    "ERROR: no target pack for {}: looked for {} and {}\n  fix: put the kit's target pack (reference prompt, look bible, moments: docs/KIT_PROCESS.md step 2) in `<kit>/target` beside the kit crate, or in `kits/<kit>/target` of the game workspace",
                    app.name,
                    sibling.map_or_else(|| "(the kit crate has no parent folder)".to_string(), |p| p.join("target").display().to_string()),
                    in_ws.join("target").display()
                ));
            }
        };
        Ok(Self {
            home: ext.root.clone(),
            kit_crate: ext.kit_dir.clone(),
            kit_package: crate::app_dir::package_name(&ext.kit_dir).unwrap_or_else(|| format!("evani-kit-{kit}")),
            kit_pack,
            shown: app.dir.display().to_string(),
            app_dir: Some(app.dir.clone()),
        })
    }
}

pub fn cmd_review(args: &[String]) -> Result<(), String> {
    let a = ReviewArgs::parse(args)?;
    let root = workspace_root();
    let app_name = resolve_app(&root, &a.name)?;
    let app = load_app(&app_name)?;
    let place = Place::in_engine(&root, &app_name, &app.manifest.kit);
    review_game(&root, &app, &place, &a)
}

/// `cargo xtask review --app-dir <folder> [options]`: the same card for a
/// game outside the engine, written to `<game workspace>/.vibedeck/review/<id>/`.
pub(crate) fn cmd_review_app(app: &crate::AppDir, args: &[String]) -> Result<(), String> {
    let a = ReviewArgs::parse_with(args, false)?;
    let place = Place::outside(app)?;
    review_game(&workspace_root(), app, &place, &a)
}

fn review_game(root: &Path, app: &crate::AppDir, place: &Place, a: &ReviewArgs) -> Result<(), String> {
    let root = root.to_path_buf();
    let app_name = app.name.clone();
    let m = app.manifest.clone();
    let out = place.home.join(".vibedeck").join("review").join(&app_name);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    if a.accept {
        let n = accept_shots(&out)?;
        println!("Accepted {n} showcase captures as the visual-regression baseline: {}", out.join("accepted").display());
        return Ok(());
    }
    println!("Kit review card for {app_name} (kit {}, look {})", m.kit, m.look);

    // 1. Shipped build: size and build-time on/off.
    let ship = m.clone().ship();
    let size: BuildSize = trunk_build(app, "ship", &ship)?;
    let kit_dirs = [place.kit_crate.clone(), place.kit_pack.clone()];
    let look_dir = root.join("looks").join(format!("evani-look-{}", m.look));
    let engine = markers_under(&[root.join("crates")]);
    let mut own: Vec<String> = markers_under(&[app.dir.join("src"), kit_dirs[0].clone(), kit_dirs[1].clone(), look_dir.clone()])
        .into_iter()
        .filter(|mk| !engine.contains(mk))
        .collect();
    // The render module is always in; its own shaders (forward, sprite,
    // tonemap, present) come with it.
    own.extend(
        markers_under(&[root.join("crates").join("evani-render")])
            .into_iter()
            .filter(|mk| mk.starts_with("evani.shader.")),
    );
    // The material modules evani.toml lists, and the crates behind the render
    // passes the app or kit crate turns on in its evani-render dependency.
    let game_tomls: Vec<String> = [app.dir.join("Cargo.toml"), kit_dirs[0].join("Cargo.toml")]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    own.extend(listed_engine_markers(&root, &ship, &game_tomls));
    let onoff = onoff_check(&size.markers, &ship, &own, &engine);

    // 2. Bench.
    let bench = gather_bench(&root, a);

    // 3. Captures, with the showcase of the target pack's moments.
    let pack = read_pack(&place.kit_pack.join("target"));
    let target_moments = pack.moments.as_deref().map(kitchecks::moment_ids).unwrap_or_default();
    let mut show_moments: Vec<String> = if target_moments.is_empty() { m.review_moments.iter().map(|x| x.name.clone()).collect() } else { target_moments };
    // `[review] showcase_extra`: shot on every showcase tier too.
    for x in &m.review_showcase_extra {
        if !show_moments.contains(x) {
            show_moments.push(x.clone());
        }
    }
    let (is_2d, render_rules) = look_facts(&root, &m.look);
    let internal = kitchecks::internal_resolution(m.review_internal_resolution.as_deref(), &render_rules);
    let engine_now = engine_commit(&root, "HEAD");
    let plan = ShowcasePlan {
        video: m.review_video.clone().or_else(|| show_moments.first().cloned()).unwrap_or_default(),
        moments: show_moments,
        tiers: SHOWCASE_TIERS.iter().map(|t| (*t).to_string()).collect(),
        size: kitchecks::showcase_size(internal),
        engine: engine_now.clone().unwrap_or_else(|| "unknown".into()),
        video_seconds: VIDEO_SECONDS,
        video_size: video_size(internal),
    };
    let (shots, capture_error, summary) = match gather_shots(&root, &app_name, place.app_dir.as_deref(), &out, a, &m, &plan) {
        Ok((s, v)) => (s, None, Some(v)),
        Err(e) => (Vec::new(), Some(e), None),
    };
    let probe = summary.as_ref().map(|v| checks::probe_from(v, &m.kit));
    let edges = match (&probe, m.review_world_edge) {
        (Some(p), true) => edge_views(&out, &p.geo, &EdgeParams::default()),
        _ => Vec::new(),
    };
    let main = main_ref(&root, a.main.as_deref());
    let handover = HandOver {
        size_gz: size.total_gz(),
        engine: engine_now,
        main: engine_commit(&root, &main),
        main_ref: main,
        dirty: engine_dirty(&root),
    };

    // 4. Reference prompt, what to try, docs, roadmap.
    let spec = first_existing(&[kit_dirs[1].join("SPEC.md"), kit_dirs[0].join("SPEC.md")]);
    let readme = first_existing(&[kit_dirs[0].join("README.md"), kit_dirs[1].join("README.md")]);
    let changes = first_existing(&[look_dir.join("CHANGES.md"), kit_dirs[0].join("CHANGES.md"), kit_dirs[1].join("CHANGES.md")]);
    let catalog = |f: &str| read(&root.join("catalog").join(f)).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let reference = match spec.as_ref().and_then(|p| read(p).and_then(|t| reference_from_spec(&t)).map(|t| (p, t))) {
        Some((p, text)) => Reference::Spec { path: rel(&root, p), text },
        None => match (catalog("genres.json"), catalog("looks.json")) {
            (Some(g), Some(l)) => match reference_from_catalog(&g, &l, &m.kit, &m.look) {
                Some(text) => Reference::Catalog { text },
                None => Reference::None {
                    why: format!(
                        "The kit has no SPEC.md with a reference image prompt, and kit `{}` / look `{}` are not a genre and look in catalog/, so none can be built from the template.",
                        m.kit, m.look
                    ),
                },
            },
            _ => Reference::None { why: "No SPEC.md prompt and catalog/ could not be read.".into() },
        },
    };
    let (try_list, try_source) = if !m.review_try.is_empty() {
        (m.review_try.clone(), format!("`[review] try` in {}/evani.toml", place.shown))
    } else {
        match readme.as_ref().and_then(|p| read(p).map(|t| (p, try_from_readme(&t)))) {
            Some((p, list)) if !list.is_empty() => (list, format!("\"What to try\" in {}", rel(&root, p))),
            _ => (Vec::new(), "no `[review] try` in evani.toml and no \"What to try\" section in a kit README".into()),
        }
    };
    let roadmap = read(&root.join("ROADMAP.md")).map(|t| roadmap_rows(&t)).unwrap_or_default();
    let row_status = roadmap.iter().find(|(id, _)| id == &m.kit).map(|(_, s)| s.clone());

    // 5. Size history.
    let history_path = place.home.join(".vibedeck").join("review").join("history.json");
    let mut history = read(&history_path).map(|t| parse_history(&t)).unwrap_or_default();
    let roadmap_ids: Vec<String> = roadmap.iter().map(|(id, _)| id.clone()).collect();
    let previous = pick_previous(&history, &m.kit, &roadmap_ids).map(|(r, b)| (r.clone(), b));
    let when = utc_string(SystemTime::now());
    let commit = git_commit(&root);
    let record = SizeRecord {
        app: app_name.clone(),
        kit: m.kit.clone(),
        total_gz: size.total_gz(),
        wasm_gz: size.wasm_gz,
        js_gz: size.js_gz,
        when: when.clone(),
        commit: commit.clone(),
    };

    // 6. Checklist.
    let mut auto = AutoResults::default();
    let build_ev = if onoff.pass() {
        format!("Shipped build made from evani.toml, {} gzipped; only listed modules and effects in the wasm.", kb(size.total_gz()))
    } else if !onoff.extra.is_empty() {
        format!("{} gzipped, but the download holds unlisted parts: {}.", kb(size.total_gz()), onoff.extra.join(", "))
    } else {
        format!("{} gzipped, but listed parts are not in the wasm: {}.", kb(size.total_gz()), onoff.missing.join(", "))
    };
    auto.set(AutoCheck::BuildAndSize, onoff.pass(), build_ev);
    auto.set(AutoCheck::Bench, bench.passes(), format!("Bench: {}", bench_sentence(&bench)));
    let mut missing_docs = Vec::new();
    let outside = place.app_dir.is_some();
    if spec.is_none() {
        missing_docs.push(if outside { place.kit_pack.join("SPEC.md").display().to_string() } else { format!("kits/{}/SPEC.md", m.kit) });
    }
    if readme.is_none() {
        missing_docs.push(if outside { place.kit_crate.join("README.md").display().to_string() } else { format!("kits/evani-kit-{}/README.md", m.kit) });
    }
    if changes.is_none() {
        missing_docs.push(format!("looks/evani-look-{}/CHANGES.md (or the kit's)", m.look));
    }
    // The engine's ROADMAP lists the engine's own kits; a game outside the
    // engine has no row there, so the row is not part of its docs check.
    let row_ok = outside || row_status.as_deref() == Some("review");
    let docs_ev = format!(
        "{}; ROADMAP row: {}.",
        if missing_docs.is_empty() { "SPEC.md, README and CHANGES.md present".to_string() } else { format!("missing {}", missing_docs.join(", ")) },
        if outside { "not applicable (a game outside the engine)".to_string() } else { row_status.as_deref().map_or("none for this kit".to_string(), |s| format!("`{s}`")) }
    );
    auto.set(AutoCheck::Docs, missing_docs.is_empty() && row_ok, docs_ev);
    let showcase = summary.as_ref().map(kitchecks::showcase_from).unwrap_or_default();
    let (regression, accepted_any) = regression_inputs(&out, &showcase);
    let (tests, tests_why, dump, dump_why) = if outside { run_kit_tests_in(&place.home, &place.kit_crate, &place.kit_package, &out) } else { run_kit_tests(&root, &m.kit, &out) };
    let geocheck = read(&place.home.join(".vibedeck").join("geocheck").join(&app_name).join("report.json")).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let (critic_rounds, critic_latest) = critic::read_rounds(&out.join("critic"));
    let stage = read(&root.join("STAGES.md")).map(|t| critic::current_stage(&t)).unwrap_or(0);
    let playground = read(&app.dir.join("evani.toml")).and_then(|t| {
        let problems = if outside {
            evani_guide::playground::problems_in(&place.kit_crate, &app.dir.join("src"), &format!("{}/evani.toml", place.shown), &m.kit, &t)
        } else {
            evani_guide::playground::problems(&root, &app_name, &m.kit, &t)
        };
        (evani_guide::playground::parse(&t) != Ok(None)).then(|| problems.into_iter().map(|p| format!("{} (fix: {})", p.what, p.fix)).collect())
    });
    let kit_inputs = KitInputs {
        pack,
        moments: plan.moments.clone(),
        tiers: plan.tiers.clone(),
        showcase,
        engine: plan.engine.clone(),
        internal,
        regression,
        accepted_any,
        dump,
        dump_why,
        tests,
        tests_why,
        geocheck,
        is_2d,
        critic: critic_latest,
        critic_rounds: critic_rounds.len(),
        stage,
        playground,
    };
    let process_info = process_info(ProcessInputs {
        kit: &m.kit,
        bench: &bench,
        probe: match (&probe, &capture_error) {
            (Some(p), _) => Ok(p),
            (None, Some(e)) => Err(e.as_str()),
            (None, None) => Err("no capture output"),
        },
        hud_declared: &m.review_hud,
        world_edge_applies: m.review_world_edge,
        edges,
        handover,
        size_text: kb(size.total_gz()),
        kit_inputs,
    });
    for (k, v) in &process_info.results {
        auto.set_verdict(*k, v);
    }
    let process = read(&root.join("docs").join("KIT_PROCESS.md")).ok_or("docs/KIT_PROCESS.md is missing")?;
    let checks = checklist(&parse_done_when(&process), &auto);

    let card = Card {
        app: app_name.clone(),
        game: m.name.clone(),
        kit: m.kit.clone(),
        look: m.look.clone(),
        when,
        commit,
        bench,
        size: record.clone(),
        html_raw: size.html_raw,
        previous,
        markers: size.markers.clone(),
        onoff,
        modules: module_words(&m),
        effects: m.effects.clone(),
        shots,
        capture_error,
        reference,
        try_list,
        try_source,
        checks,
        process: process_info,
    };
    let md = render_markdown(&card);
    std::fs::write(out.join("card.md"), &md).map_err(|e| format!("card.md: {e}"))?;
    let json = serde_json::to_vec_pretty(&card_json(&card)).map_err(|e| e.to_string())?;
    std::fs::write(out.join("card.json"), json).map_err(|e| format!("card.json: {e}"))?;
    history.push(record);
    let hist = Value::Array(history.iter().map(SizeRecord::to_json).collect());
    let _ = std::fs::write(&history_path, serde_json::to_vec_pretty(&hist).unwrap_or_default());

    println!();
    print!("{}", top_section(&card));
    println!("\ncard: {}", out.join("card.md").display());
    if card.capture_error.is_some() || card.bench.lines.iter().any(|l| matches!(l.verdict, TierVerdict::NoResult(_))) {
        return Err("the card is incomplete (see the capture or bench error above)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outside_game_finds_its_target_pack_beside_its_kit_or_in_its_workspace() {
        let engine = workspace_root();
        let base = std::env::temp_dir().join(format!("evani-review-place-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let app_dir = crate::tests::out_of_tree_fixture(&engine, "match-3", &base.join("game"));
        let mut app = crate::load_app_dir(&app_dir).unwrap();
        let ws = base.join("game");
        let kit = ws.join("crates").join("evani-kit-match-3");
        std::fs::create_dir_all(&kit).unwrap();
        std::fs::write(kit.join("Cargo.toml"), "[package]\nname = \"evani-kit-sea\"\n").unwrap();
        app.external.as_mut().unwrap().kit_dir = kit.clone();
        // Neither place: an ERROR naming both.
        let e = Place::outside(&app).unwrap_err();
        let sibling = ws.join("crates").join("match-3").join("target");
        let in_ws = ws.join("kits").join("match-3").join("target");
        assert!(e.starts_with("ERROR: no target pack for match-3") && e.contains(&sibling.display().to_string()) && e.contains(&in_ws.display().to_string()) && e.contains("fix:"), "{e}");
        // In the game workspace's kits/.
        std::fs::create_dir_all(&in_ws).unwrap();
        let p = Place::outside(&app).unwrap();
        assert_eq!((p.kit_pack.clone(), p.home.clone(), p.kit_package.as_str()), (ws.join("kits").join("match-3"), ws.clone(), "evani-kit-sea"));
        assert_eq!(p.app_dir.as_deref(), Some(app.dir.as_path()));
        // Beside the kit crate wins.
        std::fs::create_dir_all(&sibling).unwrap();
        assert_eq!(Place::outside(&app).unwrap().kit_pack, ws.join("crates").join("match-3"));
        // A game in apps/: the engine's paths, as before.
        let inside = Place::in_engine(&engine, "match-3", "match-3");
        assert_eq!((inside.kit_pack, inside.kit_crate, inside.shown.as_str(), inside.app_dir), (engine.join("kits").join("match-3"), engine.join("kits").join("evani-kit-match-3"), "apps/match-3", None));
        // review and critic with --app-dir: the folder is the game.
        assert!(ReviewArgs::parse_with(&s(&["--rebench"]), false).is_ok());
        assert!(ReviewArgs::parse_with(&s(&["match-3"]), false).unwrap_err().contains("give no game id"));
        assert!(crate::critic::cmd_critic_app(&app, &s(&["--round"])).unwrap_err().contains("only --extra-round"));
        let _ = std::fs::remove_dir_all(&base);
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// The review's captures hide every dev-only page element the live build
    /// adds over the game: the debug overlay and the model drop bar (its ids
    /// read from the live crate, so renaming one there fails here); and the
    /// phone shots are taken as a touch screen, put back after.
    #[test]
    fn the_capture_hides_dev_ui_and_shoots_phones_as_touch_screens() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let capture = std::fs::read_to_string(root.join("tools/live-check/capture.mjs")).expect("capture.mjs");
        let convert = std::fs::read_to_string(root.join("crates/evani-live/src/convert_ui.rs")).expect("convert_ui.rs");
        let hide = capture.lines().find(|l| l.contains("display: none !important")).expect("the hide rule");
        let ids: Vec<&str> = convert
            .lines()
            .filter(|l| l.starts_with("const ") && l.contains("_ID: &str"))
            .filter_map(|l| l.split('"').nth(1))
            .collect();
        assert_eq!(ids, ["evani-convert-zone", "evani-convert-report"]);
        for id in ["evani-overlay", "evani-panel"].iter().chain(ids.iter()) {
            assert!(hide.contains(&format!("#{id}")), "#{id} shows in review captures");
        }
        let on = capture.find("setTouchEmulationEnabled', { enabled: true").expect("touch on for the phone shots");
        let off = capture.find("setTouchEmulationEnabled', { enabled: false").expect("touch off after");
        let phone = capture.find("Phone shot of").expect("the phone shot");
        assert!(on < phone && phone < off, "touch is on for the phone and portrait shots only");
        let tap = capture.find("Input.dispatchTouchEvent").expect("a first touch shows the touch controls");
        assert!(on < tap && tap < phone, "the first touch comes before the phone shot is staged");
        let key = capture[off..].find("Input.dispatchKeyEvent").map(|k| k + off).expect("a key hides the touch controls again");
        assert!(key > phone, "the touch controls are hidden after the phone shots");
        assert!(capture.contains("process.env.CARGO_TARGET_DIR"), "the bridge is looked for where cargo built it");
    }

    const SANDBOX: &str = r#"
name = "sandbox"
kit = "example"
look = "basic"
effects = ["bloom", "vignette"]
[modules]
physics = "simple"
net = true
xr = true
live = true
[review]
try = ["Orbit the camera", "Toggle bloom"]
"#;

    fn tiers() -> Vec<(String, String)> {
        [("lite", "Lite"), ("low", "Low"), ("medium", "Medium"), ("high", "High"), ("ultra", "Ultra"), ("vr", "VR")]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn result(tier: &str, label: &str, pass: bool, failures: &[&str]) -> Value {
        json!({
            "tier": tier, "label": label, "mode": "browser-gpu", "frames": 600, "avg_ms": 10.0, "p95_ms": 12.0,
            "p99_ms": 13.0, "gpu_ms": 2.0, "draw_calls": 7, "triangles": 1, "sprites": 1, "points_used": 2,
            "points_total": 7, "render_size": [1, 1],
            "limits": { "floor_ms": null, "max_draw_calls": 150, "max_sprites": 5000, "max_triangles": 100000, "points": 7, "target_ms": 16.666 },
            "phases": [], "pass": pass, "failures": failures, "note": if tier == "vr" { "(mono, preset only)" } else { "" },
        })
    }

    fn runner(results: Vec<Value>) -> Value {
        let jobs: Vec<Value> = results
            .into_iter()
            .map(|r| {
                let tier = r["tier"].clone();
                json!({ "tier": tier, "error": null, "console": [], "bench": { "done": true, "error": null, "mode": "browser-gpu", "progress": "done", "results": [r] } })
            })
            .collect();
        json!({ "version": "Chrome/155", "adapter": { "vendor": "nvidia", "description": "RTX" }, "jobs": jobs })
    }

    #[test]
    fn manifest_reads_the_review_list() {
        let m = Manifest::parse(SANDBOX).unwrap();
        assert_eq!(m.review_try, ["Orbit the camera", "Toggle bloom"]);
        assert!(!m.features().iter().any(|f| f.contains("review")));
        assert!(Manifest::parse(&SANDBOX.replace("try =", "nope =")).is_err());
        assert!(Manifest::parse(&SANDBOX.replace("[\"Orbit the camera\", \"Toggle bloom\"]", "3")).is_err());
        let none = SANDBOX.split("[review]").next().unwrap();
        assert!(Manifest::parse(none).unwrap().review_try.is_empty());
        assert!(m.review_moments.is_empty());
        let with = Manifest::parse(&format!("{SANDBOX}moments = [\"drift@high\", \"finish\"]
")).unwrap();
        assert_eq!(with.review_moments.iter().map(crate::ReviewMoment::arg).collect::<Vec<_>>(), ["drift@high", "finish"]);
        assert_eq!(with.review_moments[0].tier.as_deref(), Some("high"));
        for bad in ["\"Drift\"", "\"drift@fast\"", "\"\"", "3"] {
            assert!(Manifest::parse(&format!("{SANDBOX}moments = [{bad}]
")).is_err(), "{bad}");
        }
    }

    #[test]
    fn manifest_reads_hud_and_world_edge() {
        let m = Manifest::parse(SANDBOX).unwrap();
        assert!(m.review_hud.is_empty());
        assert!(m.review_world_edge, "a ground is assumed unless the game says otherwise");
        let with = Manifest::parse(&format!("{SANDBOX}hud = [\"#hud0\", \".score\"]\nworld_edge = false\n")).unwrap();
        assert_eq!(with.review_hud, ["#hud0", ".score"]);
        assert!(!with.review_world_edge);
        assert!(Manifest::parse(&format!("{SANDBOX}world_edge = \"no\"\n")).is_err());
        assert!(Manifest::parse(&format!("{SANDBOX}hud = 3\n")).is_err());
        assert!(!with.features().iter().any(|f| f.contains("hud") || f.contains("edge")), "review keys never become Cargo features");
    }

    #[test]
    fn a_canvas_hud_never_goes_to_the_dom_probe() {
        let h = |v: &[&str]| hud_entries(&v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
        assert_eq!(h(&["canvas"]), (true, vec![]));
        assert_eq!(h(&["#hud0", "canvas", ".score"]), (true, vec!["#hud0".to_string(), ".score".to_string()]));
        assert_eq!(h(&["#hud"]), (false, vec!["#hud".to_string()]));
    }

    #[test]
    fn video_frames_scale_pixel_art_by_a_whole_number() {
        assert_eq!(video_size(Some((480, 270))), Some((960, 540)));
        assert_eq!(video_size(Some((320, 180))), Some((1280, 720)));
        assert_eq!(video_size(Some((320, 224))), Some((960, 672)));
        assert_eq!(video_size(None), None);
        assert_eq!(video_size(Some((2000, 100))), None);
    }

    #[test]
    fn manifest_reads_video_and_internal_resolution() {
        let m = Manifest::parse(SANDBOX).unwrap();
        assert_eq!((m.review_video.as_deref(), m.review_internal_resolution.as_deref()), (None, None));
        let with = Manifest::parse(&format!("{SANDBOX}video = \"drift\"\ninternal_resolution = \"320x180\"\n")).unwrap();
        assert_eq!((with.review_video.as_deref(), with.review_internal_resolution.as_deref()), (Some("drift"), Some("320x180")));
        for bad in ["video = \"Drift\"", "video = 3", "internal_resolution = \"320*180\"", "internal_resolution = \"0x180\""] {
            assert!(Manifest::parse(&format!("{SANDBOX}{bad}\n")).is_err(), "{bad}");
        }
        assert!(Manifest::parse(&format!("{SANDBOX}vidoe = \"drift\"\n")).unwrap_err().contains("internal_resolution"));
    }

    #[test]
    fn manifest_reads_phone_dpr() {
        assert_eq!(Manifest::parse(SANDBOX).unwrap().review_phone_dpr, None);
        for (v, want) in [("3", 3.0), ("2.5", 2.5), ("1", 1.0)] {
            assert_eq!(Manifest::parse(&format!("{SANDBOX}phone_dpr = {v}\n")).unwrap().review_phone_dpr, Some(want), "{v}");
        }
        for bad in ["phone_dpr = 0", "phone_dpr = 5", "phone_dpr = \"3\"", "phone_dpr = 0.5"] {
            assert!(Manifest::parse(&format!("{SANDBOX}{bad}\n")).unwrap_err().contains("phone_dpr"), "{bad}");
        }
    }

    #[test]
    fn parses_main_option() {
        let a = ReviewArgs::parse(&s(&["kart-racer", "--main", "m1-foundation"])).unwrap();
        assert_eq!(a.main.as_deref(), Some("m1-foundation"));
        assert!(ReviewArgs::parse(&s(&["kart-racer", "--main"])).is_err());
        assert_eq!(ReviewArgs::parse(&s(&["kart-racer"])).unwrap().main, None);
    }

    #[test]
    fn parses_args() {
        let a = ReviewArgs::parse(&s(&["sandbox", "--rebench", "--port", "8100"])).unwrap();
        assert_eq!((a.name.as_str(), a.rebench, a.port), ("sandbox", true, Some(8100)));
        assert!(ReviewArgs::parse(&[]).is_err());
        assert!(ReviewArgs::parse(&s(&["a", "b"])).is_err());
        assert!(ReviewArgs::parse(&s(&["a", "--nope"])).is_err());
    }

    #[test]
    fn a_review_never_touches_the_tabs_bridge_or_session_file() {
        // Its captures start away from the tab's port.
        assert_eq!(capture_port(&ReviewArgs::parse(&s(&["kart-racer"])).unwrap()), REVIEW_PORT);
        assert_ne!(REVIEW_PORT, 8096);
        assert_eq!(capture_port(&ReviewArgs::parse(&s(&["kart-racer", "--port", "8185"])).unwrap()), 8185);
        // Every bridge the capture scripts start gets its own session file,
        // and they only ever remove that one: the tab's session file is
        // read at most, never written or deleted.
        let root = workspace_root();
        for script in ["capture.mjs", "pilot.mjs"] {
            let text = std::fs::read_to_string(root.join("tools/live-check").join(script)).unwrap();
            let serves: Vec<&str> = text.lines().filter(|l| l.contains("['serve'")).collect();
            assert!(!serves.is_empty(), "{script} starts a bridge");
            for l in serves {
                assert!(l.contains("'--session', ownSession(port)"), "{script}: a bridge without its own session file: {l}");
            }
            assert!(text.contains("const ownSession = (port) => join(ROOT, '.evani', 'live', 'review', String(port), 'session.json');"), "{script}");
            for (i, l) in text.lines().enumerate() {
                if l.contains("rmSync(f") {
                    let before: Vec<&str> = text.lines().take(i).collect();
                    let f = before.iter().rev().find(|b| b.contains("const f =")).unwrap();
                    assert!(f.contains("ownSession(port)"), "{script}:{}: removes a session file that is not its own: {f}", i + 1);
                }
                assert!(!(l.contains("writeFileSync") && l.contains("session.json")), "{script}:{}: writes a session file", i + 1);
            }
        }
        // The bridge takes the option.
        let bridge = std::fs::read_to_string(root.join("tools/evani-live-bridge/src/main.rs")).unwrap();
        assert!(bridge.contains("\"--session\" => a.session"));
    }

    #[test]
    fn bench_lines_follow_tier_order_and_show_failures() {
        let r = runner(vec![
            result("ultra", "Ultra", false, &["frame-time target (avg 22.74 ms > 18.33 ms for 60 fps +10%)"]),
            result("lite", "Lite", true, &[]),
            result("low", "Low", true, &[]),
            result("medium", "Medium", true, &[]),
            result("high", "High", true, &[]),
            result("vr", "VR", true, &[]),
        ]);
        let b = bench_lines(&tiers(), &r);
        let ids: Vec<&str> = b.lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["lite", "low", "medium", "high", "ultra", "vr"]);
        assert_eq!(b.gpu, "nvidia RTX");
        assert!(!b.passes());
        assert_eq!(
            bench_sentence(&b),
            "FAILS on Ultra (frame-time target); passes on Lite, Low, Medium, High and VR."
        );
        let all = runner(["lite", "low", "medium", "high", "ultra", "vr"].iter().map(|t| result(t, &t.to_uppercase(), true, &[])).collect());
        let b = bench_lines(&tiers(), &all);
        assert!(b.passes());
        assert_eq!(bench_sentence(&b), "passes on Lite, Low, Medium, High, Ultra and VR.");
    }

    #[test]
    fn missing_tiers_and_unavailable_vr() {
        let r = json!({ "jobs": [
            { "tier": "low", "error": null, "bench": { "done": true, "error": null, "mode": "browser-gpu", "progress": "done", "results": [result("low", "Low", true, &[])] } },
            { "tier": "vr", "error": "WebXR with WebGPU is not available", "bench": null },
        ]});
        let b = bench_lines(&tiers(), &r);
        assert!(matches!(b.lines[5].verdict, TierVerdict::Unavailable(_)));
        assert!(matches!(b.lines[2].verdict, TierVerdict::NoResult(_)));
        assert!(!b.passes());
        assert!(bench_sentence(&b).contains("VR not available"), "{}", bench_sentence(&b));
    }

    fn rec(app: &str, kit: &str, gz: u64) -> SizeRecord {
        SizeRecord { app: app.into(), kit: kit.into(), total_gz: gz, wasm_gz: 0, js_gz: 0, when: "t".into(), commit: "c".into() }
    }

    #[test]
    fn previous_card_choice() {
        let roadmap = s(&["kart-racer", "2d-platformer", "match-3"]);
        let h = vec![rec("kart-racer", "kart-racer", 100), rec("sandbox", "example", 50), rec("kart-racer", "kart-racer", 110)];
        // Same kit wins, newest first.
        let (p, b) = pick_previous(&h, "kart-racer", &roadmap).unwrap();
        assert_eq!((p.total_gz, b), (110, Basis::SameKit));
        // A new roadmap kit compares with the nearest earlier kit that has a card.
        let (p, b) = pick_previous(&h, "match-3", &roadmap).unwrap();
        assert_eq!((p.total_gz, b), (110, Basis::EarlierKit));
        // The first roadmap kit has nothing earlier.
        assert!(pick_previous(&h[1..2], "kart-racer", &roadmap).is_none());
        // A kit outside the roadmap compares with the most recent card.
        let (p, b) = pick_previous(&h, "other", &roadmap).unwrap();
        assert_eq!((p.total_gz, b), (110, Basis::MostRecent));
        assert!(pick_previous(&[], "other", &roadmap).is_none());
    }

    #[test]
    fn size_delta_wording() {
        assert_eq!(describe_delta(2048, 1024), "1.0 KB larger (+100.0%)");
        assert_eq!(describe_delta(1024, 2048), "1.0 KB smaller (-50.0%)");
        assert_eq!(describe_delta(5, 5), "the same size");
        assert_eq!(describe_delta(1024, 0), "1.0 KB larger (+0.0%)");
    }

    #[test]
    fn history_round_trip() {
        let h = vec![rec("a", "k", 1), rec("b", "j", 2)];
        let text = Value::Array(h.iter().map(SizeRecord::to_json).collect()).to_string();
        assert_eq!(parse_history(&text), h);
        assert!(parse_history("not json").is_empty());
    }

    #[test]
    fn onoff_flags_unlisted_parts() {
        let m = Manifest::parse(SANDBOX).unwrap().ship();
        let marks = s(&[
            "evani.module.debug_panel", "evani.module.fx", "evani.module.net", "evani.module.physics",
            "evani.module.render", "evani.module.xr", "evani.shader.bloom_bright", "evani.shader.vignette",
        ]);
        let own = s(&["evani.module.debug_panel"]);
        let engine = s(&["evani.shader.bloom_bright", "evani.shader.outline", "evani.shader.vignette"]);
        assert_eq!(onoff_check(&marks, &m, &own, &engine), OnOff::default());
        let mut leaky = marks.clone();
        leaky.push("evani.module.live".into());
        leaky.push("evani.shader.outline".into());
        let r = onoff_check(&leaky, &m, &own, &engine);
        assert_eq!(r.extra, ["evani.module.live", "evani.shader.outline"]);
        assert!(!r.pass());
        let mut no_xr = Manifest::parse(SANDBOX).unwrap().ship();
        no_xr.xr = false;
        no_xr.effects = vec!["bloom".into()];
        let r = onoff_check(&marks, &no_xr, &own, &engine);
        assert_eq!(r.extra, ["evani.module.xr", "evani.shader.vignette"]);
        let r = onoff_check(&marks[..5], &m, &own, &engine);
        assert!(r.missing.contains(&"evani.module.xr".to_string()));
        assert!(r.missing.contains(&"evani.shader.vignette*".to_string()));
        assert!(!r.pass(), "a listed effect whose shader is not in the wasm fails");
    }

    #[test]
    fn onoff_effects_without_a_shader_are_not_missing() {
        let mut m = Manifest::parse(SANDBOX).unwrap().ship();
        m.effects = vec!["vignette".into(), "shake".into(), "parallax".into()];
        let marks = s(&[
            "evani.module.debug_panel", "evani.module.fx", "evani.module.net", "evani.module.physics",
            "evani.module.render", "evani.module.xr", "evani.shader.vignette",
        ]);
        let own = s(&["evani.module.debug_panel"]);
        let engine = s(&["evani.shader.vignette"]);
        let r = onoff_check(&marks, &m, &own, &engine);
        assert!(r.missing.is_empty(), "{r:?}");
        assert_eq!(r.shaderless, ["shake", "parallax"]);
        assert!(r.pass());
        // A shader effect that didn't make it in still fails.
        let r = onoff_check(&marks[..6], &m, &own, &engine);
        assert_eq!(r.missing, ["evani.shader.vignette*"]);
        assert!(!r.pass());
    }

    #[test]
    fn listed_material_modules_and_kit_render_passes_count_as_listed() {
        let root = std::env::temp_dir().join(format!("evani-onoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for c in ["evani-render", "evani-material-character", "evani-material-layers", "evani-material-nature", "evani-material-proc"] {
            std::fs::create_dir_all(root.join("crates").join(c).join("src")).unwrap();
        }
        std::fs::write(
            root.join("crates").join("evani-render").join("Cargo.toml"),
            "[package]\nname = \"evani-render\"\n[features]\nforward = []\nlook-surface = [\"forward\"]\n\
             char-materials = [\"look-surface\", \"dep:evani-material-character\"]\n\
             material-layers = [\"look-surface\", \"dep:evani-material-layers\"]\nmodels = [\"forward\", \"evani-core/stream\"]\n",
        )
        .unwrap();
        let kit = "[package]\nname = \"k\"\n[dependencies]\nevani-render = { workspace = true, features = [\"forward\", \"char-materials\"] }\n".to_string();
        let mut m = Manifest::parse("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n[modules]\nnature = [\"ocean\"]\n").unwrap();
        let dirs = listed_engine_crates(&root, &m, std::slice::from_ref(&kit));
        let has = |c: &str| dirs.contains(&root.join("crates").join(c));
        assert!(has("evani-material-nature"), "a listed material module: {dirs:?}");
        assert!(has("evani-material-character"), "behind a render feature the kit turns on: {dirs:?}");
        assert!(!has("evani-material-layers") && !has("evani-material-proc"), "nothing the game didn't choose: {dirs:?}");
        // Off in evani.toml, off here: its shaders in a download stay a failure.
        m.nature = None;
        let dirs = listed_engine_crates(&root, &m, &[]);
        assert!(dirs.is_empty(), "{dirs:?}");
        assert_eq!(render_features("[target.'cfg(x)'.dependencies]\nevani-render = { features = [\"material-layers\"] }\n"), ["material-layers"]);
        assert!(render_features("not toml [").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reference_prompt_sources() {
        let spec = "# Spec\n\n## Look\nToon.\n\n## Reference image prompt\n\n```\nIn-game screenshot.\nArt style: toon.\n```\n\n## Next\n";
        assert_eq!(reference_from_spec(spec).unwrap(), "In-game screenshot.\nArt style: toon.");
        let para = "## Reference image prompt\nA plain paragraph.\nTwo lines.\n## Other\n";
        assert_eq!(reference_from_spec(para).unwrap(), "A plain paragraph.\nTwo lines.");
        assert!(reference_from_spec("# Spec\n## Look\n").is_none());
        let genres = json!({ "genres": [{ "id": "kart-racer", "name": "Kart racer", "cameras": ["chase", "top-down"], "image_scene": "Karts drift." }] });
        let looks = json!({ "looks": [{ "id": "toon", "name": "Toon", "type": "3D", "cameras": ["top-down"], "look": "Cel shading." }] });
        let t = reference_from_catalog(&genres, &looks, "kart-racer", "toon").unwrap();
        assert!(t.starts_with("In-game screenshot from an original Kart racer video game, top-down view.\nKarts drift.\nArt style: Toon. Cel shading.\nTop-quality real-time 3D"), "{t}");
        assert!(t.contains("2560×1440"));
        assert!(reference_from_catalog(&genres, &looks, "example", "basic").is_none());
    }

    #[test]
    fn reads_the_real_catalog() {
        let root = workspace_root();
        let g: Value = serde_json::from_str(&std::fs::read_to_string(root.join("catalog/genres.json")).unwrap()).unwrap();
        let l: Value = serde_json::from_str(&std::fs::read_to_string(root.join("catalog/looks.json")).unwrap()).unwrap();
        let t = reference_from_catalog(&g, &l, "kart-racer", "toon").unwrap();
        assert!(t.contains("Art style:"), "{t}");
    }

    #[test]
    fn try_list_from_readme() {
        let md = "# Kit\n\n## What to try\n\n- Drift round the first bend\n* Fire a shell\nnot a bullet\n\n## Notes\n- no\n";
        assert_eq!(try_from_readme(md), ["Drift round the first bend", "Fire a shell"]);
        assert!(try_from_readme("# Kit\n- a\n").is_empty());
    }

    #[test]
    fn parses_the_done_when_list() {
        let md = "# X\n## Steps\n1. Not this.\n## Done when\n\n1. `xtask` builds it. The gzipped size is reported. (The xtask build is planned, M1 in progress.)\n2. The demo is playable\n   from title to win.\n3. `apps/bench` passes on Low.\n\n## After\n4. no\n";
        let items = parse_done_when(md);
        assert_eq!(
            items,
            vec![
                (1, "`xtask` builds it. The gzipped size is reported.".to_string()),
                (2, "The demo is playable from title to win.".to_string()),
                (3, "`apps/bench` passes on Low.".to_string()),
            ]
        );
        assert_eq!(drop_planned_notes("a (keep this) b (planned, later) c"), "a (keep this) b c");
    }

    /// Whether `text` holds one of `c`'s hook groups (ignoring the others).
    fn auto_check_for_only(c: AutoCheck, text: &str) -> bool {
        let t = text.to_lowercase();
        c.hooks().iter().any(|group| group.iter().all(|p| t.contains(p)))
    }

    #[test]
    fn the_real_kit_process_has_auto_checks() {
        let md = std::fs::read_to_string(workspace_root().join("docs/KIT_PROCESS.md")).unwrap();
        let items = parse_done_when(&md);
        assert_eq!(items.len(), 29, "{items:?}");
        assert_eq!(items.iter().map(|(n, _)| *n).collect::<Vec<_>>(), (1..=29).collect::<Vec<_>>());
        let kinds: Vec<Option<AutoCheck>> = items.iter().map(|(_, t)| auto_check_for(t)).collect();
        assert_eq!(kinds[0], Some(AutoCheck::BuildAndSize));
        assert_eq!(kinds[2], Some(AutoCheck::Bench));
        assert_eq!(kinds[6], Some(AutoCheck::Docs));
        // Items 11 to 29 map one to one, in order, onto the process checks,
        // apart from the hand-checked two build passes (17) and human fix
        // rounds (28); 29 is free roam for playgrounds.
        let process: Vec<AutoCheck> = AutoCheck::ALL.into_iter().filter(|c| c.is_process()).collect();
        assert_eq!(process.len(), 17);
        assert_eq!((kinds[16], kinds[27]), (None, None), "items 17 and 28 are hand checks");
        assert_eq!(kinds[10..].iter().flatten().copied().collect::<Vec<_>>(), process);
        assert_eq!(kinds[28], Some(AutoCheck::FreeRoam));
        // Every item's text matches exactly one check's hook, never two.
        for (n, t) in &items {
            let hits: Vec<AutoCheck> = AutoCheck::ALL.into_iter().filter(|c| auto_check_for_only(*c, t)).collect();
            assert!(hits.len() <= 1, "item {n} matches {hits:?}: {t}");
        }
        assert!(items[28].1.len() < 200, "the last item must not swallow the explanation below it: {}", items[28].1);
        // The visual quality, placeholder and Stage 3 hero-asset bars (8–10),
        // the two build passes (17) and human fix rounds (28) are hand checks.
        assert_eq!(kinds.iter().filter(|k| k.is_none()).count(), 9);
        assert!(items.iter().all(|(_, t)| !t.contains("planned")));
    }

    #[test]
    fn checklist_marks_auto_items() {
        let items = vec![(1, "The gzipped size is reported.".to_string()), (2, "Playable.".to_string()), (3, "apps/bench passes on Low".to_string())];
        let mut auto = AutoResults::default();
        auto.set(AutoCheck::BuildAndSize, true, "221 KB");
        auto.set(AutoCheck::Bench, false, "Ultra fails");
        let c = checklist(&items, &auto);
        assert_eq!(c.iter().map(|i| i.status).collect::<Vec<_>>(), [CheckStatus::Pass, CheckStatus::Manual, CheckStatus::Fail]);
        assert_eq!(c[2].evidence, "Ultra fails");
    }

    #[test]
    fn every_auto_check_has_its_own_wording_hook() {
        let said = [
            (AutoCheck::TierCostOrder, "Tier cost order: GPU time rises from Low to Ultra."),
            (AutoCheck::Architecture, "Architecture conformance: entities and registered params."),
            (AutoCheck::CaptureHud, "The captures hide the debug overlay and show the HUD."),
            (AutoCheck::WorldEdge, "No visible World edge from any scripted camera."),
            (AutoCheck::HandOver, "The hand-over states the published size and the engine commit."),
        ];
        for (want, text) in said {
            assert_eq!(auto_check_for(text), Some(want), "{text}");
        }
        // The first three keep their wording, and no check's hook matches another's sentence.
        assert_eq!(auto_check_for("`apps/bench` passes on Low."), Some(AutoCheck::Bench));
        assert_eq!(auto_check_for("A SPEC.md and a README exist."), Some(AutoCheck::Docs));
        assert_eq!(auto_check_for("The demo is playable."), None);
        for c in AutoCheck::ALL {
            let hits: Vec<_> = said.iter().filter(|(_, t)| auto_check_for(t) == Some(c)).collect();
            assert!(hits.len() <= 1, "{c:?} matches {hits:?}");
        }
        assert!(AutoCheck::ALL.iter().filter(|c| c.is_process()).count() == 17);
        assert_eq!(auto_check_for("Free roam: a playground supports free roam."), Some(AutoCheck::FreeRoam));
    }

    #[test]
    fn results_for_the_process_checks_fill_their_items() {
        let items = vec![(11, "Tier cost order: rises.".to_string()), (12, "Architecture conformance: ecs.".to_string()), (13, "World edge: none.".to_string())];
        let mut auto = AutoResults::default();
        auto.set(AutoCheck::TierCostOrder, true, "Low 0.4 -> Ultra 3.0");
        auto.set(AutoCheck::WorldEdge, false, "menu shows the edge");
        let c = checklist(&items, &auto);
        assert_eq!(c.iter().map(|i| i.status).collect::<Vec<_>>(), [CheckStatus::Pass, CheckStatus::Manual, CheckStatus::Fail]);
    }

    #[test]
    fn reads_roadmap_rows() {
        let md = "| # | Kit | ID | Rank | Physics | First look | Status | Notes |\n|---|---|---|---|---|---|---|---|\n| 001 | Kart racer | `kart-racer` | #19 | simple | `toon` | review | 220 KB |\n| 002 | 2D | `2d-platformer` | #36 | simple | `pixel16` | todo |  |\n";
        assert_eq!(roadmap_rows(md), vec![("kart-racer".into(), "review".into()), ("2d-platformer".into(), "todo".into())]);
        let real = roadmap_rows(&std::fs::read_to_string(workspace_root().join("ROADMAP.md")).unwrap());
        assert_eq!(real.first().map(|r| r.0.as_str()), Some("kart-racer"));
        assert!(real.len() > 20);
    }

    #[test]
    fn utc_dates() {
        assert_eq!(utc_string(UNIX_EPOCH), "1970-01-01 00:00 UTC");
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_791_936_000 + 3_660);
        assert_eq!(utc_string(t), "2026-10-14 01:01 UTC");
    }

    #[test]
    fn shots_need_all_three() {
        let v = json!({ "captures": [
            { "file": "C:/x/capture-1.png", "t": 0, "width": 640, "height": 360, "stats": { "tier": "high" } },
            { "file": "C:/x/capture-2.png", "t": 20, "width": 640, "height": 360, "stats": { "tier": "high" } },
            { "file": "C:/x/capture-3.png", "t": 40, "width": 640, "height": 360, "stats": { "tier": "high" } },
        ]});
        let shots = shots_from(&v, 3).unwrap();
        assert_eq!(shots[1].file, "capture-2.png");
        assert_eq!((shots[2].t, shots[0].tier.as_str()), (40.0, "high"));
        assert_eq!(shots[0].heading(), "t = 0 s");
        assert!(shots_from(&json!({ "captures": [] }), 3).is_err());
        assert!(shots_from(&json!({}), 3).is_err());
    }

    #[test]
    fn moment_shots_carry_label_moment_and_tier() {
        let v = json!({ "mode": "moments", "captures": [
            { "file": "C:/x/moment-drift.png", "moment": "drift", "label": "A drift", "tier": "high", "t": 31.5, "width": 640, "height": 360, "stats": { "tier": "high" } },
            { "file": "C:/x/moment-item_hit.png", "moment": "item_hit", "label": "A hit", "tier": "low", "width": 640, "height": 360, "stats": { "tier": "low" } },
        ]});
        let shots = shots_from(&v, 2).unwrap();
        assert_eq!(shots[1].file, "moment-item_hit.png");
        assert_eq!((shots[1].moment.as_deref(), shots[1].label.as_str(), shots[1].tier.as_str()), (Some("item_hit"), "A hit", "low"));
        assert_eq!(shots[1].heading(), "item hit (low)");
        assert!(shots_from(&v, 3).is_err());
        let mut card = sample_card();
        card.shots = shots;
        let md = render_markdown(&card);
        assert!(md.contains("| drift (high) | item hit (low) |"), "{md}");
        assert!(md.contains("- **item hit** on low: A hit"), "{md}");
        assert!(top_section(&card).contains("2 scripted moments from the demo below, each on its own tier (drift on high, item hit on low)"));
        let j = card_json(&card);
        assert_eq!(j["captures"][0]["moment"], "drift");
        assert_eq!(j["captures"][1]["label"], "A hit");
    }

    fn sample_card() -> Card {
        let m = Manifest::parse(SANDBOX).unwrap();
        let r = runner(vec![
            result("lite", "Lite", true, &[]),
            result("low", "Low", true, &[]),
            result("medium", "Medium", true, &[]),
            result("high", "High", true, &[]),
            result("ultra", "Ultra", false, &["frame-time target (avg 22.74 ms > 18.33 ms for 60 fps +10%)"]),
            result("vr", "VR", true, &[]),
        ]);
        let mut bench = bench_lines(&tiers(), &r);
        bench.source = "fresh run".into();
        Card {
            app: "sandbox".into(),
            game: m.name.clone(),
            kit: m.kit.clone(),
            look: m.look.clone(),
            when: "2026-10-14 13:00 UTC".into(),
            commit: "abc123".into(),
            bench,
            size: rec("sandbox", "example", 226_509),
            html_raw: 1200,
            previous: Some((rec("sandbox", "example", 225_000), Basis::SameKit)),
            markers: s(&["evani.module.render"]),
            onoff: OnOff::default(),
            modules: module_words(&m.clone().ship()),
            effects: m.effects.clone(),
            shots: (1..=3).map(|i| Shot { file: format!("capture-{i}.png"), t: (i - 1) as f64 * 20.0, width: 640, height: 360, tier: "high".into(), moment: None, label: String::new() }).collect(),
            capture_error: None,
            reference: Reference::None { why: "No prompt for the example kit.".into() },
            try_list: m.review_try.clone(),
            try_source: "evani.toml".into(),
            process: ProcessInfo::default(),
            checks: checklist(
                &[(1, "gzipped".to_string()), (2, "Playable".to_string()), (3, "apps/bench".to_string())],
                &{
                    let mut a = AutoResults::default();
                    a.set(AutoCheck::BuildAndSize, true, "ok");
                    a.set(AutoCheck::Bench, false, "Ultra");
                    a
                },
            ),
        }
    }

    #[test]
    fn assembles_a_complete_card() {
        let c = sample_card();
        let top = top_section(&c);
        assert!(top.contains("**Ready for review: sandbox** (kit example, look basic)"), "{top}");
        assert!(top.contains("FAILS on Ultra (frame-time target)"), "{top}");
        assert!(top.contains("221.2 KB to download (compressed), 1.5 KB (+0.7%) larger than the previous card for this kit (sandbox, t)"), "{top}");
        assert!(top.contains("effects bloom and vignette; physics (simple), multiplayer (net) and VR (xr); nothing else is in the download"), "{top}");
        assert!(top.contains("3 views from the demo below; no target picture description exists"), "{top}");
        assert!(top.contains("(1) Orbit the camera (2) Toggle bloom"), "{top}");
        assert!(top.contains("1 passed automatically, 1 failed, 1 to check by hand"), "{top}");
        assert!(!top.contains("live tuning"), "a shipped card lists no live module: {top}");

        let md = render_markdown(&c);
        for want in [
            "# Kit review card: sandbox",
            "| Ultra | FAIL | 10.00 | 12.00 | 2.00 | 16.67 | frame-time target (avg 22.74 ms > 18.33 ms for 60 fps +10%) |",
            "| VR | PASS |",
            "| **total** | **221.2 KB** |",
            "Build-time on/off: PASS",
            "![t = 20 s](capture-2.png)",
            "Reference image prompt: none.",
            "- Toggle bloom",
            "- [x] **1. PASS:** gzipped",
            "- [ ] **2. QQ:** Playable",
            "- [ ] **3. FAIL:** apps/bench",
        ] {
            assert!(md.contains(want), "missing {want:?} in\n{md}");
        }
        let j = card_json(&c);
        assert_eq!(j["bench"]["passes"], false);
        assert_eq!(j["bench"]["tiers"][4]["result"], "fail");
        assert_eq!(j["size"]["previous"]["delta_bytes"], 1509);
        assert_eq!(j["captures"].as_array().map(Vec::len), Some(3));
        assert_eq!(j["checklist"][1]["status"], "manual");
        assert_eq!(j["reference"]["source"], "none");
    }

    fn probe_json(entities: u64, overlay_visible: bool, hud_visible: bool) -> Value {
        json!({"probe": {
            "entities": [{"at": "boot", "count": entities}, {"at": "drift", "count": entities}],
            "params": [{"name": "kit.example.speed", "owner": "kit"}, {"name": "render.clear_color", "owner": "render"}],
            "captures": [{"name": "boot", "page_png": "page-boot.png", "overlay": {"present": true, "visible": overlay_visible},
                "hud": [{"selector": "#hud0", "matched": 1, "visible": hud_visible, "text_len": 12}]}],
            "geo": [{"name": "boot", "black": "geo-boot-black.png", "white": "geo-boot-white.png",
                "camera": {"eye": [0, 5, 10], "target": [0, 0, 0], "up": [0, 1, 0], "fov_y_deg": 60}}],
        }})
    }

    fn process_for(probe: Result<&checks::Probe, &str>, edges: Vec<(String, Result<EdgeView, String>)>, world_edge: bool, handover: HandOver) -> ProcessInfo {
        let mut bench = sample_card().bench;
        for (l, g) in bench.lines.iter_mut().filter(|l| checks::COST_ORDER.contains(&l.id.as_str())).zip([0.3, 0.5, 1.0, 2.0, 3.0]) {
            l.gpu_ms = Some(g);
        }
        let hud = ["#hud0".to_string()];
        process_info(ProcessInputs { kit: "example", bench: &bench, probe, hud_declared: &hud, world_edge_applies: world_edge, edges, handover, size_text: "221.2 KB".into(), kit_inputs: KitInputs::default() })
    }

    fn clear_edge() -> EdgeView {
        EdgeView { bg_px: 100, edge_px: 0, total_px: 1000, max_below_deg: 0.0, tolerance_deg: 1.5, ..EdgeView::default() }
    }

    fn current_engine() -> HandOver {
        HandOver { size_gz: 226_509, engine: Some("abc1234".into()), main_ref: "m1-foundation".into(), main: Some("abc1234".into()), dirty: false }
    }

    #[test]
    fn process_checks_all_pass_on_good_measurements() {
        let probe = checks::probe_from(&probe_json(12, false, true), "example");
        let p = process_for(Ok(&probe), vec![("boot".into(), Ok(clear_edge()))], true, current_engine());
        assert_eq!(p.results.len(), 17);
        assert!(p.results.iter().take(5).all(|(_, v)| v.pass), "{:?}", p.results);
        assert_eq!(p.tier_gpu.len(), 5);
        assert_eq!(p.page_shots, [("boot".to_string(), "page-boot.png".to_string())]);
    }

    #[test]
    fn cost_order_prefers_the_stress_scene_then_the_path_median() {
        let mut bench = sample_card().bench;
        for (l, (mean, p50)) in bench
            .lines
            .iter_mut()
            .filter(|l| checks::COST_ORDER.contains(&l.id.as_str()))
            .zip([(0.2, Some(0.2)), (2.5, Some(0.3)), (1.0, None), (2.0, Some(1.9)), (3.0, Some(2.8))])
        {
            l.gpu_ms = Some(mean);
            l.gpu_p50_ms = p50;
        }
        // No stress run: the path's median, the mean where there is none.
        let (g, src) = bench.order_inputs();
        assert_eq!(g.iter().map(|(_, v)| *v).collect::<Vec<_>>(), [Some(0.2), Some(0.3), Some(1.0), Some(1.9), Some(2.8)]);
        assert!(src.starts_with("bench path"), "{src}");
        // A stress run missing a tier is not used.
        bench.stress_gpu = vec![("lite".into(), 0.5), ("low".into(), 1.0), ("medium".into(), 1.2), ("high".into(), 2.0)];
        assert!(bench.order_inputs().1.starts_with("bench path"));
        bench.stress_gpu.push(("ultra".into(), 4.0));
        bench.stress_runs = 5;
        let (g, src) = bench.order_inputs();
        assert_eq!(g[4], ("ultra".to_string(), Some(4.0)));
        assert_eq!(src, "stress scene, median GPU ms per frame, median of 5 runs");
    }

    #[test]
    fn process_checks_fail_with_their_own_reasons() {
        let probe = checks::probe_from(&probe_json(0, true, false), "example");
        let edge = EdgeView { bg_px: 400, edge_px: 300, total_px: 1000, max_below_deg: 30.0, tolerance_deg: 1.5, ..EdgeView::default() };
        let old = HandOver { engine: Some("old1111".into()), ..current_engine() };
        let p = process_for(Ok(&probe), vec![("boot".into(), Ok(edge))], true, old);
        let by = |c: AutoCheck| p.verdict(c).unwrap();
        assert!(by(AutoCheck::TierCostOrder).pass);
        assert!(!by(AutoCheck::Architecture).pass && by(AutoCheck::Architecture).evidence.contains("0 entities"), "{}", by(AutoCheck::Architecture).evidence);
        assert!(!by(AutoCheck::CaptureHud).pass);
        assert!(!by(AutoCheck::WorldEdge).pass && by(AutoCheck::WorldEdge).evidence.contains("boot"), "{}", by(AutoCheck::WorldEdge).evidence);
        assert!(!by(AutoCheck::HandOver).pass && by(AutoCheck::HandOver).evidence.contains("old1111"));
    }

    #[test]
    fn world_edge_is_not_applicable_when_the_manifest_says_so_and_a_failed_capture_fails_the_rest() {
        let probe = checks::probe_from(&probe_json(5, false, true), "example");
        let p = process_for(Ok(&probe), Vec::new(), false, current_engine());
        let v = p.verdict(AutoCheck::WorldEdge).unwrap();
        assert!(v.pass && v.evidence.starts_with("Not applicable"), "{}", v.evidence);
        let q = process_for(Err("the capture script failed"), Vec::new(), true, current_engine());
        for c in [AutoCheck::Architecture, AutoCheck::CaptureHud, AutoCheck::WorldEdge] {
            let v = q.verdict(c).unwrap();
            assert!(!v.pass && v.evidence.contains("the capture script failed"), "{c:?}: {}", v.evidence);
        }
        assert!(q.verdict(AutoCheck::TierCostOrder).unwrap().pass);
    }

    #[test]
    fn the_card_lists_the_process_checks() {
        let probe = checks::probe_from(&probe_json(12, true, true), "example");
        let mut c = sample_card();
        c.process = process_for(Ok(&probe), vec![("boot".into(), Ok(clear_edge()))], true, HandOver { dirty: true, ..current_engine() });
        let top = top_section(&c);
        assert!(top.contains("- **Hand-over:** FAIL. Published size 221.2 KB; engine commit abc1234"), "{top}");
        assert!(top.contains("- **Process checks:** 3 of 13 passed; failed: captures and HUD, hand-over, target pack; not run: showcase captures,"), "{top}");
        assert!(top.contains("; not applicable: camera framing (2D), bot level checks (2D), UI text rule (2D), free roam.") && top.contains("- **Critic:** not run after 0 of 3 rounds."), "{top}");
        let md = render_markdown(&c);
        for want in [
            "### Process checks (automatic, docs/KIT_PROCESS.md items 11 and on)",
            "| tier cost order | PASS |",
            "GPU time per tier (bench path, median GPU ms per frame; no fresh `cargo xtask bench --stress` run): lite 0.30 ms -> low 0.50 ms -> medium 1.00 ms -> high 2.00 ms -> ultra 3.00 ms.",
            "| boot | 100 | 0 | 0.00% |",
            "| boot | VISIBLE | `#hud0` visible (12 chars) |",
            "![boot](page-boot.png)",
        ] {
            assert!(md.contains(want), "missing {want:?} in\n{md}");
        }
        let j = card_json(&c);
        assert_eq!(j["handover"]["engine"], "abc1234");
        assert_eq!(j["handover"]["dirty"], true);
        assert_eq!(j["process"]["checks"].as_array().map(Vec::len), Some(17));
        assert_eq!(j["process"]["checks"][15]["result"], "not_run");
        assert_eq!(j["process"]["world_edge"][0]["camera"], "boot");
        assert_eq!(j["process"]["captures"][0]["overlay_visible"], true);
    }

    #[test]
    fn first_card_and_same_size_wording() {
        let mut c = sample_card();
        c.previous = None;
        assert!(top_section(&c).contains("the first card, so nothing to compare with yet"));
        c.previous = Some((rec("sandbox", "example", 226_509), Basis::SameKit));
        assert!(top_section(&c).contains("the same size as the previous card"), "{}", top_section(&c));
    }
}

/// bsjoin3 reviewer proof for bs/review-onoff, run on the real engine tree:
/// what the game really leaves out of evani.toml must still fail the on/off check.
#[cfg(test)]
mod onoff_proof {
    use super::*;

    fn real_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn check(manifest: &str, kit_toml: &str, markers: &[&str]) -> OnOff {
        let root = real_root();
        let m = Manifest::parse(manifest).expect("manifest");
        let engine = markers_under(&[root.join("crates")]);
        let mut own: Vec<String> = markers_under(&[root.join("crates").join("evani-render")]).into_iter().filter(|mk| mk.starts_with("evani.shader.")).collect();
        own.extend(listed_engine_markers(&root, &m, &[kit_toml.to_string()]));
        let markers: Vec<String> = markers.iter().map(|s| s.to_string()).collect();
        onoff_check(&markers, &m, &own, &engine)
    }

    const KIT_PLAIN: &str = "[package]\nname = \"k\"\n[dependencies]\nevani-render = { workspace = true, features = [\"forward\"] }\n";
    const KIT_CHAR: &str = "[package]\nname = \"k\"\n[dependencies]\nevani-render = { workspace = true, features = [\"forward\", \"char-materials\"] }\n";

    /// A module or material module evani.toml does not list still fails (passes on the branch).
    #[test]
    fn unlisted_modules_still_fail() {
        let r = check("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n", KIT_CHAR, &["evani.module.render", "evani.module.physics", "evani.module.material_nature", "evani.shader.nature", "evani.shader.character"]);
        assert!(r.extra.contains(&"evani.module.physics".to_string()), "{r:?}");
        assert!(r.extra.contains(&"evani.module.material_nature".to_string()), "{r:?}");
        assert!(r.extra.contains(&"evani.shader.nature".to_string()), "{r:?}");
        assert!(!r.extra.contains(&"evani.shader.character".to_string()), "kit-chosen pass is listed: {r:?}");
        let r = check("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n", KIT_PLAIN, &["evani.module.render", "evani.shader.character"]);
        assert!(r.extra.contains(&"evani.shader.character".to_string()), "a pass the kit did not choose fails: {r:?}");
    }

    /// The `models` module evani.toml does not list must fail even when the kit
    /// turns on a render pass that implies evani-render's `models` feature
    /// (char-materials -> look-surface -> models -> dep:evani-gltf, which
    /// declares `evani.module.models`).
    #[test]
    fn unlisted_models_module_still_fails_behind_a_kit_render_pass() {
        let r = check("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n", KIT_CHAR, &["evani.module.render", "evani.module.models", "evani.shader.character"]);
        assert!(r.extra.contains(&"evani.module.models".to_string()), "models is not in evani.toml but passes the on/off check: {r:?}");
    }
}
