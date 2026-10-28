//! `cargo xtask`: the Evani.Dev build tool.
//!
//! ```text
//! cargo xtask build <app> [--ship] [--no-diff]   build + per-module size report
//! cargo xtask build --app-dir <dir> [--ship] ..    the same for a game kept outside the engine folder (see app_dir.rs)
//! cargo xtask sizes [<app>]                       full / no bloom / no fx / no live table
//! cargo xtask size-report [<app>..] [--modules]   ship build raw/gzip/brotli per file + wasm-opt proof (see size_report.rs)
//! cargo xtask features <app> [--ship]             print the Cargo features evani.toml maps to
//! cargo xtask package-check <folder> [<app>]      a publish folder must carry ENGINE-LICENSE.txt (the engine's MIT notice; build --ship writes it); with <app>, the full publish preflight
//! cargo xtask publish <app> [--dry-run] [--serve]  ship build -> checked package in publish/<app>/, size report, diff vs live (see publish.rs)
//! cargo xtask bench [--tier ..]                    GPU bench of apps/bench in headless Chrome (see bench.rs)
//! cargo xtask live [--game <app>] [--port <P>]     build the live bridge and run `serve` (docs/LIVE_API.md)
//! cargo xtask dev --hot <app> [--port <P>]         the same, with live code swapping (pinned dx, Trunk fallback; LIVE_API 10.4)
//! cargo xtask dx-install [--print]                 build the pinned dx into target/evani-tools/dx (about 30 min)
//! cargo xtask review <app-or-kit> [--rebench]      the kit review card in .vibedeck/review/<app>/ (see review.rs)
//! cargo xtask init-ai | doctor | check | new-game | catalog | ai-eval
//!                                                  AI guidance and the check gate (tools/evani-guide, docs/AI_GUIDANCE.md)
//! ```
//!
//! `build` reads `apps/<app>/evani.toml`, maps it to the app crate's Cargo
//! features, runs `trunk build --release --no-default-features --features ..`
//! (Trunk runs wasm-bindgen and `wasm-opt -Oz`, see the app's index.html),
//! then prints raw and gzipped sizes and the `evani.module.*` /
//! `evani.shader.*` markers found in the wasm.
//!
//! Per-module sizes are measured, not estimated from symbols: each listed
//! module is switched off in turn and the app rebuilt; the module's size is
//! the difference to the full build. A "base" build with every optional
//! module off gives the engine core + render + kit + look. Diffs need not add
//! up exactly to the total (LTO and wasm-opt share code between modules).

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod app_dir;
mod bench;
mod blender_addon;
mod build_facts;
mod checks;
mod critic;
mod fonts;
mod kitchecks;
mod model_import;
mod netlog;
mod review;
mod publish;
mod size_report;
mod tier_report;
mod bytes;
mod importance;

use flate2::Compression;
use flate2::write::GzEncoder;

/// Unity's web export starting point the pillars compare against.
const UNITY_START_BYTES: u64 = 8 * 1024 * 1024;

// ---- evani.toml --------------------------------------------------------------

/// A game's module manifest (`apps/<app>/evani.toml`).
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub kit: String,
    pub look: String,
    pub effects: Vec<String>,
    /// `simple`, `rigid` or `None`.
    pub physics: Option<String>,
    pub net: bool,
    pub xr: bool,
    pub live: bool,
    /// Stage 3 modules. `anim` implies `models` (the parser sets both).
    pub models: bool,
    pub anim: bool,
    pub ui: bool,
    /// `[modules] nature = [..]` / `proc = [..]`: the material modules and
    /// their parts (evani_guide::manifest::MATERIAL_MODULES; `None`: off).
    pub nature: Option<Vec<String>>,
    pub proc: Option<Vec<String>>,
    /// Optional `[review] try = [..]`: what a reviewer should try in the demo
    /// (`cargo xtask review` puts it on the card). Never a Cargo feature.
    pub review_try: Vec<String>,
    /// Optional `[review] moments = ["drift@high", ..]`: scripted capture
    /// moments (the game's `CaptureMoments`) the card shows instead of the
    /// bench-path views, each with the tier to shoot it on (none: the
    /// active tier). Never a Cargo feature.
    pub review_moments: Vec<ReviewMoment>,
    /// Optional `[review] hud = ["#hud0", ..]`: CSS selectors of the game's
    /// on-screen HUD. The review's capture-HUD check needs at least one of
    /// them visible and not empty in every capture. Never a Cargo feature.
    pub review_hud: Vec<String>,
    /// Optional `[review] world_edge = false`: the game has no 3D ground to
    /// fall off (a 2D or board game), so the review's world-edge check does
    /// not apply. Default true.
    pub review_world_edge: bool,
    /// Optional `[review] video = "drift"`: the moment the review's run video
    /// starts from (default: the first signature moment).
    pub review_video: Option<String>,
    /// Optional `[review] internal_resolution = "320x180"`: a pixel game's
    /// internal size, so the showcase captures are integer-scaled from it
    /// (default: the look's render rules in catalog/looks.json).
    pub review_internal_resolution: Option<String>,
    /// Optional `[review] showcase_extra = ["threat"]`: moments the showcase
    /// shoots on every showcase tier besides the target pack's signature
    /// moments (target/moments.md keeps 5 to 8 of those). Each must also be a
    /// `[review] moments` entry, so it has a page shot.
    pub review_showcase_extra: Vec<String>,
    /// Optional `[review] phone_dpr = 3`: the device pixel ratio of the
    /// review's phone page shots (844x390 and 390x844 CSS px; default 1). A
    /// pixel-art game picks its whole-pixel scale from the device pixels, so
    /// it is captured at a real phone's ratio (1 to 4).
    pub review_phone_dpr: Option<f64>,
    /// Built to ship ([`Self::ship`]): the build also strips the builder's
    /// home and workspace folders from the wasm ([`ship_rustflags`]). Never
    /// read from evani.toml.
    pub shipped: bool,
}

/// One `[review] moments` entry: `name` or `name@tier`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewMoment {
    pub name: String,
    pub tier: Option<String>,
}

impl ReviewMoment {
    pub fn parse(s: &str) -> Result<Self, String> {
        let (name, tier) = match s.split_once('@') {
            Some((n, t)) => (n, Some(t)),
            None => (s, None),
        };
        let id = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !id(name) {
            return Err(format!("evani.toml: review moment {s:?}: the name must be a-z, 0-9 and _"));
        }
        if let Some(t) = tier
            && !matches!(t, "lite" | "low" | "medium" | "high" | "ultra" | "vr")
        {
            return Err(format!("evani.toml: review moment {s:?}: the tier must be lite, low, medium, high, ultra or vr"));
        }
        Ok(Self {
            name: name.to_string(),
            tier: tier.map(str::to_string),
        })
    }

    /// As capture.mjs `--moments` takes it.
    pub fn arg(&self) -> String {
        match &self.tier {
            Some(t) => format!("{}@{t}", self.name),
            None => self.name.clone(),
        }
    }
}

fn str_field(t: &toml::Table, key: &str) -> Result<String, String> {
    t.get(key)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("evani.toml: `{key}` must be a string"))
}

fn bool_field(t: Option<&toml::Table>, key: &str) -> Result<bool, String> {
    match t.and_then(|m| m.get(key)) {
        None => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("evani.toml: `modules.{key}` must be true or false")),
    }
}

/// What `[review]` holds.
#[derive(Debug, Clone, PartialEq)]
struct ReviewFields {
    tries: Vec<String>,
    moments: Vec<ReviewMoment>,
    hud: Vec<String>,
    world_edge: bool,
    video: Option<String>,
    internal_resolution: Option<String>,
    showcase_extra: Vec<String>,
    phone_dpr: Option<f64>,
}

impl Default for ReviewFields {
    fn default() -> Self {
        Self { tries: Vec::new(), moments: Vec::new(), hud: Vec::new(), world_edge: true, video: None, internal_resolution: None, showcase_extra: Vec::new(), phone_dpr: None }
    }
}

/// `[review]`: `try`, `moments` and `hud` (lists of strings), `world_edge` (a
/// bool), `video` (a moment name), `internal_resolution` ("WxH") and
/// `showcase_extra` (moment names, each also in `moments`) and `phone_dpr`
/// (a number from 1 to 4).
fn review_fields(v: &toml::Value) -> Result<ReviewFields, String> {
    let r = v.as_table().ok_or("evani.toml: `[review]` must be a table")?;
    const KEYS: &str = "try, moments, hud, world_edge, video, internal_resolution, showcase_extra, phone_dpr";
    if let Some(key) = r
        .keys()
        .find(|k| !matches!(k.as_str(), "try" | "moments" | "hud" | "world_edge" | "video" | "internal_resolution" | "showcase_extra" | "phone_dpr"))
    {
        return Err(format!("evani.toml: unknown review field `{key}` ({KEYS})"));
    }
    let string = |key: &str| -> Result<Option<String>, String> {
        r.get(key).map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("evani.toml: `review.{key}` must be a string"))).transpose()
    };
    let video = string("video")?;
    if let Some(v) = &video {
        ReviewMoment::parse(v).map_err(|e| e.replace("review moment", "review video"))?;
    }
    let internal_resolution = string("internal_resolution")?;
    if let Some(ir) = &internal_resolution {
        let ok = ir.split_once('x').is_some_and(|(w, h)| w.parse::<u32>().is_ok_and(|w| w > 0) && h.parse::<u32>().is_ok_and(|h| h > 0));
        if !ok {
            return Err(format!("evani.toml: `review.internal_resolution` must be \"WxH\" like \"320x180\", got {ir:?}"));
        }
    }
    let strings = |key: &str| -> Result<Vec<String>, String> {
        let Some(list) = r.get(key) else {
            return Ok(Vec::new());
        };
        list.as_array()
            .ok_or_else(|| format!("evani.toml: `review.{key}` must be a list of strings"))?
            .iter()
            .map(|e| {
                e.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("evani.toml: every `review.{key}` entry must be a string"))
            })
            .collect()
    };
    let moments = strings("moments")?
        .iter()
        .map(|m| ReviewMoment::parse(m))
        .collect::<Result<Vec<_>, _>>()?;
    let world_edge = match r.get("world_edge") {
        None => true,
        Some(v) => v.as_bool().ok_or("evani.toml: `review.world_edge` must be true or false")?,
    };
    let showcase_extra = strings("showcase_extra")?;
    if let Some(x) = showcase_extra.iter().find(|x| !moments.iter().any(|m| &m.name == *x)) {
        return Err(format!("evani.toml: `review.showcase_extra` entry {x:?} must also be a `review.moments` entry"));
    }
    let phone_dpr = match r.get("phone_dpr") {
        None => None,
        Some(v) => match v.as_float().or_else(|| v.as_integer().map(|i| i as f64)) {
            Some(d) if (1.0..=4.0).contains(&d) => Some(d),
            _ => return Err("evani.toml: `review.phone_dpr` must be a number from 1 to 4 (a phone's device pixel ratio)".into()),
        },
    };
    Ok(ReviewFields { tries: strings("try")?, moments, hud: strings("hud")?, world_edge, video, internal_resolution, showcase_extra, phone_dpr })
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let t: toml::Table = text.parse().map_err(|e| format!("evani.toml: {e}"))?;
        let effects = match t.get("effects") {
            None => Vec::new(),
            Some(v) => v
                .as_array()
                .ok_or("evani.toml: `effects` must be a list of names")?
                .iter()
                .map(|e| {
                    e.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "evani.toml: every effect must be a string".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let modules = match t.get("modules") {
            None => None,
            Some(v) => Some(
                v.as_table()
                    .ok_or("evani.toml: `[modules]` must be a table")?,
            ),
        };
        let physics = match modules.and_then(|m| m.get("physics")) {
            None => None,
            Some(v) => match v.as_str() {
                Some("none") => None,
                Some(p @ ("simple" | "rigid")) => Some(p.to_string()),
                _ => {
                    return Err(
                        "evani.toml: `modules.physics` must be \"simple\", \"rigid\" or \"none\""
                            .into(),
                    );
                }
            },
        };
        for key in modules.into_iter().flat_map(|m| m.keys()) {
            if !matches!(key.as_str(), "physics" | "net" | "xr" | "live" | "models" | "anim" | "ui" | "nature" | "proc") {
                return Err(format!(
                    "evani.toml: unknown module `{key}` (physics, net, xr, live, models, anim, ui, nature, proc)"
                ));
            }
        }
        let anim = bool_field(modules, "anim")?;
        let review = match t.get("review") {
            None => ReviewFields::default(),
            Some(v) => review_fields(v)?,
        };
        Ok(Self {
            name: str_field(&t, "name")?,
            kit: str_field(&t, "kit")?,
            look: str_field(&t, "look")?,
            effects,
            physics,
            net: bool_field(modules, "net")?,
            xr: bool_field(modules, "xr")?,
            live: bool_field(modules, "live")?,
            models: bool_field(modules, "models")? || anim,
            anim,
            ui: bool_field(modules, "ui")?,
            nature: evani_guide::manifest::parts(modules, "nature")?,
            proc: evani_guide::manifest::parts(modules, "proc")?,
            review_try: review.tries,
            review_moments: review.moments,
            review_hud: review.hud,
            review_world_edge: review.world_edge,
            review_video: review.video,
            review_internal_resolution: review.internal_resolution,
            review_showcase_extra: review.showcase_extra,
            review_phone_dpr: review.phone_dpr,
            shipped: false,
        })
    }

    /// The app crate features this manifest turns on (`--no-default-features`
    /// is always passed, so this is the complete set).
    pub fn features(&self) -> Vec<String> {
        let mut f = Vec::new();
        if !self.effects.is_empty() {
            f.push("fx".to_string());
        }
        f.extend(self.effects.iter().cloned());
        if let Some(p) = &self.physics {
            f.push(format!("physics-{p}"));
        }
        for (on, name) in [
            (self.net, "net"),
            (self.xr, "xr"),
            (self.live, "live"),
            (self.models, "models"),
            (self.anim, "anim"),
            (self.ui, "ui"),
        ] {
            if on {
                f.push(name.to_string());
            }
        }
        f.extend(evani_guide::manifest::material_features(self.nature.as_deref(), self.proc.as_deref()));
        f
    }

    /// A shipped game never contains the dev-only live module, nor the
    /// builder's folder paths.
    pub fn ship(mut self) -> Self {
        self.live = false;
        self.shipped = true;
        self
    }

    /// One variant per listed module with that module switched off.
    pub fn without_each(&self) -> Vec<(String, Manifest)> {
        let mut out = Vec::new();
        for e in &self.effects {
            let mut m = self.clone();
            m.effects.retain(|x| x != e);
            out.push((format!("fx.{e}"), m));
        }
        if !self.effects.is_empty() {
            let mut m = self.clone();
            m.effects.clear();
            out.push(("fx (all)".to_string(), m));
        }
        if self.physics.is_some() {
            let mut m = self.clone();
            m.physics = None;
            out.push(("physics".to_string(), m));
        }
        for name in ["net", "xr", "live", "models", "anim", "ui"] {
            let mut m = self.clone();
            let flag = match name {
                "net" => &mut m.net,
                "xr" => &mut m.xr,
                "models" => {
                    // No models means no animation either.
                    m.anim = false;
                    &mut m.models
                }
                "anim" => &mut m.anim,
                "ui" => &mut m.ui,
                _ => &mut m.live,
            };
            if *flag {
                *flag = false;
                out.push((name.to_string(), m));
            }
        }
        // The material modules: each part off, then the whole module.
        for key in ["nature", "proc"] {
            let parts = if key == "nature" { &self.nature } else { &self.proc };
            let Some(parts) = parts else { continue };
            for p in parts {
                let mut m = self.clone();
                let list = if key == "nature" { &mut m.nature } else { &mut m.proc };
                if let Some(l) = list {
                    l.retain(|x| x != p);
                }
                out.push((format!("{key}.{p}"), m));
            }
            let mut m = self.clone();
            *(if key == "nature" { &mut m.nature } else { &mut m.proc }) = None;
            out.push((key.to_string(), m));
        }
        out
    }

    /// Every optional module off: core + render + kit + look.
    pub fn base(&self) -> Self {
        Self {
            effects: Vec::new(),
            physics: None,
            net: false,
            xr: false,
            live: false,
            models: false,
            anim: false,
            ui: false,
            nature: None,
            proc: None,
            ..self.clone()
        }
    }
}

/// Check every feature exists in the app crate and the kit/look crates are
/// its dependencies, so a typo in evani.toml fails loudly.
/// The same check `cargo xtask check` runs (evani_guide::manifest).
fn check_against_crate(m: &Manifest, cargo_toml: &str) -> Result<(), String> {
    match evani_guide::manifest::crate_problems(&m.features(), &m.kit, &m.look, cargo_toml).into_iter().next() {
        Some(problem) => Err(format!(
            "ERROR: {problem}
  fix: make evani.toml and the app's Cargo.toml agree: each listed effect or module needs a feature of that name turning on its optional dependency, and the app depends on the kit and look crates (README \"evani.toml\"; `cargo xtask features <app>` lists what evani.toml asks for)"
        )),
        None => Ok(()),
    }
}

// ---- sizes and markers ----------------------------------------------------------

pub fn gzip_len(bytes: &[u8]) -> u64 {
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    if enc.write_all(bytes).is_err() {
        return 0;
    }
    enc.finish().map(|v| v.len() as u64).unwrap_or(0)
}

const MARKER_PREFIXES: [&str; 3] = ["evani.module.", "evani.shader.", "evani.text."];

fn is_ident(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

/// Every `evani.module.<id>` / `evani.shader.<id>` / `evani.text.<id>` named in source text. In
/// source the names are delimited (quotes, line ends), so a greedy scan works.
pub fn markers_in_text(text: &str, out: &mut Vec<String>) {
    let bytes = text.as_bytes();
    for prefix in MARKER_PREFIXES {
        let mut from = 0;
        while let Some(pos) = text[from..].find(prefix) {
            let start = from + pos;
            let mut end = start + prefix.len();
            while end < bytes.len() && is_ident(bytes[end]) {
                end += 1;
            }
            if end > start + prefix.len() {
                out.push(text[start..end].to_string());
            }
            from = end;
        }
    }
}

/// Marker names declared anywhere in the engine, kits, looks and apps
/// (`.rs` and `.wgsl`), sorted and deduplicated.
pub fn known_markers(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs" || x == "wgsl")
                && let Ok(t) = std::fs::read_to_string(&p)
            {
                markers_in_text(&t, out);
            }
        }
    }
    let mut out = Vec::new();
    for d in ["crates", "kits", "looks", "apps"] {
        walk(&root.join(d), &mut out);
    }
    out.sort();
    out.dedup();
    out
}

/// Which `known` markers occur in the binary. The wasm data section packs
/// strings back to back with no separators (`evani.module.fxfxevani...`), so
/// boundaries can't be found by scanning; instead each known name is looked
/// up exactly. An occurrence that is really the start of a longer known name
/// (`evani.module.net` inside `evani.module.network`) doesn't count.
pub fn scan_markers(bytes: &[u8], known: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for m in known {
        let needle = m.as_bytes();
        let longer: Vec<&[u8]> = known
            .iter()
            .filter(|k| k.len() > m.len() && k.starts_with(m.as_str()))
            .map(|k| k.as_bytes())
            .collect();
        let hit = bytes.windows(needle.len()).enumerate().any(|(i, w)| {
            w == needle
                && !longer
                    .iter()
                    .any(|l| bytes.get(i..i + l.len()).is_some_and(|s| s == *l))
        });
        if hit {
            found.push(m.clone());
        }
    }
    found
}

/// Byte strings that only the dev-only live code swapping brings in: the
/// `subsecond` crate (its source paths survive in panic locations), the
/// engine's `hotpatch` modules, the in-page toast, and the S7-3
/// `evani-importance` marker (the dev-only importance measuring in
/// evani-render). A shipped (`--ship`) game must contain none of them.
pub const DEV_ONLY_STRINGS: [&str; 5] = ["subsecond", "hotpatch", "evani-code-toast", "__evani_code", "evani-importance"];

/// Which [`DEV_ONLY_STRINGS`] occur in `bytes`.
pub fn dev_only_in(bytes: &[u8]) -> Vec<String> {
    DEV_ONLY_STRINGS
        .iter()
        .filter(|s| bytes.windows(s.len()).any(|w| w == s.as_bytes()))
        .map(|s| (*s).to_string())
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct BuildSize {
    pub wasm_raw: u64,
    pub wasm_gz: u64,
    pub js_raw: u64,
    pub js_gz: u64,
    pub html_raw: u64,
    pub markers: Vec<String>,
    /// [`DEV_ONLY_STRINGS`] found in the wasm or js.
    pub dev_only: Vec<String>,
    /// Root-absolute local paths found in the built html (see [`absolute_paths_in`]).
    pub absolute_paths: Vec<String>,
    /// Snippet files in the dist folder that no shipped js imports (see [`orphan_snippets`]).
    pub orphan_snippets: Vec<String>,
}

impl BuildSize {
    pub fn total_raw(&self) -> u64 {
        self.wasm_raw + self.js_raw + self.html_raw
    }

    /// What a player downloads with gzip on (html counted raw; it is tiny).
    pub fn total_gz(&self) -> u64 {
        self.wasm_gz + self.js_gz + self.html_raw
    }

    pub fn has(&self, marker: &str) -> bool {
        self.markers.iter().any(|m| m == marker)
    }

    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.markers.iter().any(|m| m.starts_with(prefix))
    }
}

fn measure(dist: &Path) -> Result<BuildSize, String> {
    let known = known_markers(&workspace_root());
    let mut s = BuildSize::default();
    let mut top_js = String::new();
    let entries = std::fs::read_dir(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if name.ends_with(".wasm") {
            s.wasm_raw += bytes.len() as u64;
            s.wasm_gz += gzip_len(&bytes);
            s.markers.extend(scan_markers(&bytes, &known));
            s.dev_only.extend(dev_only_in(&bytes));
        } else if name.ends_with(".js") {
            s.js_raw += bytes.len() as u64;
            s.js_gz += gzip_len(&bytes);
            s.dev_only.extend(dev_only_in(&bytes));
            top_js.push_str(&String::from_utf8_lossy(&bytes));
        } else if name.ends_with(".html") {
            s.html_raw += bytes.len() as u64;
            s.absolute_paths
                .extend(absolute_paths_in(&String::from_utf8_lossy(&bytes)));
        }
    }
    if s.wasm_raw == 0 {
        return Err(format!("no .wasm in {}", dist.display()));
    }
    // wasm-bindgen snippets (`#[wasm_bindgen(module = ...)]` files such as
    // the models `stream` shim) are part of the page's JS.
    let mut dirs = vec![dist.join("snippets")];
    let mut snippet_paths = Vec::new();
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "js")
                && let Ok(bytes) = std::fs::read(&path)
            {
                s.js_raw += bytes.len() as u64;
                s.js_gz += gzip_len(&bytes);
                s.dev_only.extend(dev_only_in(&bytes));
                if let Ok(rel) = path.strip_prefix(dist) {
                    snippet_paths.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    s.orphan_snippets = orphan_snippets(&top_js, &snippet_paths);
    s.markers.sort();
    s.markers.dedup();
    s.dev_only.sort();
    s.dev_only.dedup();
    s.absolute_paths.sort();
    s.absolute_paths.dedup();
    Ok(s)
}

/// The snippet files (paths relative to the dist folder, `/`-separated) that
/// the page's js never imports. wasm-bindgen keeps its output folder between
/// builds, so a snippet from another game's build (for example the models
/// `stream` shim) can be copied into a game that never uses it: dead weight
/// in its download.
pub fn orphan_snippets(page_js: &str, snippet_paths: &[String]) -> Vec<String> {
    let mut out: Vec<String> = snippet_paths
        .iter()
        .filter(|p| !page_js.contains(p.as_str()))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Patterns that make a built page load engine files from the site root. A
/// game is served from a subfolder (`play.evani.app/<item>/<version>/`), so
/// these 404 there and the game shows a blank screen.
const ABSOLUTE_PATH_PATTERNS: [&str; 4] = ["src=\"/", "href=\"/", "from '/", "module_or_path: '/"];

/// The root-absolute local paths in one html file (`//host` urls are not local).
pub fn absolute_paths_in(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    for pat in ABSOLUTE_PATH_PATTERNS {
        for (at, _) in html.match_indices(pat) {
            let start = at + pat.len();
            let rest = &html[start..];
            if rest.starts_with('/') {
                continue;
            }
            let len = rest.find(['"', '\'']).unwrap_or(rest.len()).min(60);
            found.push(format!("{}{}", &pat[..pat.len() - 1], &html[start - 1..start + len]));
        }
    }
    found
}

/// The ship guard: a shipped build carries neither the live module nor any
/// part of live code swapping.
pub fn ship_guard(app: &str, s: &BuildSize) -> Result<(), String> {
    if s.has("evani.module.live") {
        return Err(format!("{app}: the ship build still contains the live module"));
    }
    if s.dev_only.iter().any(|d| d == "evani-importance") {
        return Err(format!(
            "ERROR {app}: the ship build carries the importance measuring tool (marker `evani-importance`)\n  fix: build without evani-render's `importance` feature; `cargo xtask importance {app}` turns it on for its own run only and never for `build --ship`"
        ));
    }
    if !s.dev_only.is_empty() {
        return Err(format!(
            "{app}: the ship build still contains live code swapping ({})",
            s.dev_only.join(", ")
        ));
    }
    if !s.absolute_paths.is_empty() {
        return Err(format!(
            "ERROR {app}: index.html loads files from the site root ({}); the game is served from a subfolder, so these 404 and the screen stays blank\n  fix: build with a relative public url (`trunk build --public-url ./`, which `cargo xtask build {app} --ship` passes) and make sure no template writes a path starting with `/`",
            s.absolute_paths.join(", ")
        ));
    }
    if !s.orphan_snippets.is_empty() {
        return Err(format!(
            "ERROR {app}: the ship folder carries js files the page never loads ({})\n  fix: they are left over in wasm-bindgen's shared output folder from another game's build; `cargo xtask build {app} --ship` clears it before building, so build again through xtask rather than plain trunk",
            s.orphan_snippets.join(", ")
        ));
    }
    Ok(())
}

// ---- building -------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.ancestors()
        .nth(2)
        .map_or(here.clone(), Path::to_path_buf)
}

/// Cargo's target folder: `CARGO_TARGET_DIR` when set (a relative one is
/// read against the workspace root), else `<root>/target`. Trunk writes
/// wasm-bindgen's shared output there, so the stale-snippet clear must look
/// there too (a worktree build with its own target folder failed the ship
/// guard on another game's `stream.js`).
fn cargo_target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()) {
        Some(t) => workspace_root().join(t),
        None => workspace_root().join("target"),
    }
}

struct AppDir {
    name: String,
    dir: PathBuf,
    manifest: Manifest,
    /// Set for a game outside the engine folder (`--app-dir`).
    external: Option<app_dir::External>,
}

impl AppDir {
    /// The kit crate's folder.
    fn kit_dir(&self) -> PathBuf {
        match &self.external {
            Some(e) => e.kit_dir.clone(),
            None => workspace_root().join("kits").join(format!("evani-kit-{}", self.manifest.kit)),
        }
    }

    /// Where builds of this game go (`<variant>` folders and reports):
    /// `target/evani-sizes` of the engine, or of the game's own workspace.
    fn sizes_dir(&self) -> PathBuf {
        match &self.external {
            Some(e) => e.root.join("target").join("evani-sizes"),
            None => workspace_root().join("target").join("evani-sizes"),
        }
    }

    /// Where this game's baked fonts are cached (`<id>/` inside): the
    /// engine's `target/evani-fonts`, or the game workspace's (beside its
    /// sizes folder), so two out-of-tree games with the same id never share
    /// a cache in the engine.
    fn fonts_cache_dir(&self) -> PathBuf {
        match &self.external {
            Some(e) => e.root.join("target").join("evani-fonts"),
            None => workspace_root().join("target").join("evani-fonts"),
        }
    }

    /// Cargo's target folder for this game's builds.
    fn cargo_target_dir(&self) -> PathBuf {
        match &self.external {
            Some(e) => app_dir::target_dir(&e.root, std::env::var_os("CARGO_TARGET_DIR").as_deref()),
            None => cargo_target_dir(),
        }
    }
}

fn load_app(name: &str) -> Result<AppDir, String> {
    let dir = workspace_root().join("apps").join(name);
    let text = std::fs::read_to_string(dir.join("evani.toml"))
        .map_err(|e| format!("{}: {e}", dir.join("evani.toml").display()))?;
    let manifest = Manifest::parse(&text)?;
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml"))
        .map_err(|e| format!("{}: {e}", dir.join("Cargo.toml").display()))?;
    check_against_crate(&manifest, &cargo)?;
    Ok(AppDir {
        name: name.to_string(),
        dir,
        manifest,
        external: None,
    })
}

/// A game outside the engine folder: `dir` is its app folder (`evani.toml`,
/// `Cargo.toml`, `index.html`); its id is the `name` in `evani.toml`.
fn load_app_dir(dir: &Path) -> Result<AppDir, String> {
    load_app_dir_with(dir, true)
}

/// [`load_app_dir`]; `rules` refuses a game whose evani.toml and Cargo.toml
/// disagree or that has a plain dependency on an optional engine crate.
/// `check --app-dir` loads without them: it reports the same problems as
/// ERROR lines with the rest of the gate.
fn load_app_dir_with(dir: &Path, rules: bool) -> Result<AppDir, String> {
    let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = std::fs::read_to_string(dir.join("evani.toml"))
        .map_err(|e| format!("{}: {e} (--app-dir takes the game's app folder, the one with its evani.toml)", dir.join("evani.toml").display()))?;
    let manifest = Manifest::parse(&text)?;
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml"))
        .map_err(|e| format!("{}: {e}", dir.join("Cargo.toml").display()))?;
    if rules {
        check_against_crate(&manifest, &cargo)?;
    }
    // A build is refused when the module on/off rule is broken: a plain
    // dependency on an optional engine crate would ship in every build while
    // the build facts list only what evani.toml turns on. (`check --app-dir`
    // reports it instead.)
    let plain = if rules { evani_guide::manifest::not_optional(&cargo) } else { Vec::new() };
    if !plain.is_empty() {
        let lines: Vec<String> = plain
            .iter()
            .map(|c| format!("ERROR: `{c}` is a plain dependency, so it is in every build whether evani.toml lists it or not\n  fix: make it `{c} = {{ workspace = true, optional = true }}` and turn it on only from the features evani.toml maps to"))
            .collect();
        return Err(format!("{}:\n{}", dir.join("Cargo.toml").display(), lines.join("\n")));
    }
    if !evani_guide::scaffold::valid_id(&manifest.name) {
        return Err(format!("ERROR: {}: name `{}` is not a game id (lowercase letters, digits and single `-`)", dir.join("evani.toml").display(), manifest.name));
    }
    let root = app_dir::workspace_root_of(&dir);
    app_dir::check_engine(&dir, &root, &workspace_root())?;
    let kit_dir = app_dir::kit_dir(&dir, &root, &manifest.kit, &workspace_root())?;
    Ok(AppDir {
        name: manifest.name.clone(),
        dir,
        manifest,
        external: Some(app_dir::External { root, kit_dir }),
    })
}

/// The environment variable Cargo reads as extra wasm rustflags. It is
/// merged with `.cargo/config.toml`'s `[target.wasm32-unknown-unknown]
/// rustflags` (`RUSTFLAGS` would replace that list, dropping
/// `--cfg=web_sys_unstable_apis`). Cargo splits it on whitespace.
const WASM_RUSTFLAGS_ENV: &str = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";

/// A ship build's `--remap-path-prefix` flags: the builder's home folder,
/// Cargo's and rustup's homes and the workspace, so the panic locations in
/// a shipped wasm read `/cargo/registry/src/...` and not
/// `C:\Users\<name>\.cargo\registry\src\...` (privacy; DECISIONS "Ship
/// builds strip the builder's paths"). rustc uses the last flag that
/// matches, so the wider home folder comes first. A folder whose path holds
/// whitespace cannot be passed through [`WASM_RUSTFLAGS_ENV`] and is
/// returned in the second list instead.
pub fn ship_rustflags(home: Option<&Path>, cargo_home: Option<&Path>, rustup_home: Option<&Path>, root: &Path) -> (Vec<String>, Vec<PathBuf>) {
    let (mut flags, mut skipped) = (Vec::new(), Vec::new());
    for (from, to) in [(home, "/home"), (cargo_home, "/cargo"), (rustup_home, "/rustup"), (Some(root), "/evani")] {
        let Some(from) = from.filter(|p| !p.as_os_str().is_empty()) else { continue };
        let text = from.to_string_lossy();
        if text.chars().any(char::is_whitespace) {
            skipped.push(from.to_path_buf());
            continue;
        }
        flags.push(format!("--remap-path-prefix={}={to}", text.trim_end_matches(['\\', '/'])));
    }
    (flags, skipped)
}

/// [`ship_rustflags`] for this machine.
fn ship_rustflags_here() -> (Vec<String>, Vec<PathBuf>) {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = var("USERPROFILE").or_else(|| var("HOME"));
    let cargo_home = var("CARGO_HOME").or_else(|| home.as_ref().map(|h| h.join(".cargo")));
    let rustup_home = var("RUSTUP_HOME").or_else(|| home.as_ref().map(|h| h.join(".rustup")));
    ship_rustflags(home.as_deref(), cargo_home.as_deref(), rustup_home.as_deref(), &workspace_root())
}

/// `trunk build --release` of one feature set into `target/evani-sizes/<app>-<variant>`.
fn trunk_build(app: &AppDir, variant: &str, m: &Manifest) -> Result<BuildSize, String> {
    let features = m.features().join(",");
    let dist = app.sizes_dir().join(format!("{}-{variant}", app.name));
    eprintln!("  building {variant:<12} features [{features}]");
    // wasm-bindgen's snippet output is shared by every build in this target
    // folder; clear it (and the old dist copy) so no other game's shim ships here.
    for stale in [
        app.cargo_target_dir().join("wasm-bindgen").join("release").join("snippets"),
        dist.join("snippets"),
    ] {
        if stale.is_dir() {
            std::fs::remove_dir_all(&stale)
                .map_err(|e| format!("{}: cannot clear ({e})", stale.display()))?;
        }
    }
    let mut cmd = Command::new("trunk");
    cmd.env_remove("NO_COLOR"); // trunk refuses NO_COLOR=1
    cmd.current_dir(&app.dir)
        .args(["build", "--release", "--no-default-features"])
        // Relative paths: the folder works from any subfolder and from the site root.
        .args(["--public-url", "./"])
        .arg("--dist")
        .arg(&dist);
    if !features.is_empty() {
        cmd.args(["--features", &features]);
    }
    // Keep the PC responsive: cap rustc parallelism unless the caller chose.
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "6");
    }
    // A ship build carries no builder folder paths (dev builds unchanged).
    if m.shipped {
        let (flags, mut skipped) = ship_rustflags_here();
        let mut all: Vec<String> = std::env::var(WASM_RUSTFLAGS_ENV).ok().into_iter().filter(|v| !v.trim().is_empty()).collect();
        all.extend(flags);
        if let Some(e) = &app.external {
            let (extra, more) = app_dir::extra_rustflags(e, &workspace_root(), true);
            all.extend(extra);
            skipped.extend(more);
        }
        for p in skipped {
            eprintln!("  note: {} has a space in its path, so ship builds cannot strip it from the wasm", p.display());
        }
        cmd.env(WASM_RUSTFLAGS_ENV, all.join(" "));
    } else if let Some(e) = &app.external {
        // The engine's .cargo/config.toml is not read in the game's workspace.
        let mut all: Vec<String> = std::env::var(WASM_RUSTFLAGS_ENV).ok().into_iter().filter(|v| !v.trim().is_empty()).collect();
        all.extend(app_dir::extra_rustflags(e, &workspace_root(), false).0);
        cmd.env(WASM_RUSTFLAGS_ENV, all.join(" "));
    }
    let out = cmd.output().map_err(|e| {
        format!("cannot run trunk ({e}); install it with `cargo install trunk --locked`")
    })?;
    if !out.status.success() {
        let log = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = log.lines().rev().take(25).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!(
            "trunk build failed for {variant}:\n{}",
            tail.join("\n")
        ));
    }
    // `ui`: the game's `[ui] fonts`, baked (cached) and shipped in fonts/.
    if m.ui {
        fonts::copy_to(&fonts::bake_into(&workspace_root(), &app.fonts_cache_dir(), &app.name, &app.dir)?, &dist)?;
    }
    measure(&dist)
}

// ---- reports ----------------------------------------------------------------------

fn kb(bytes: u64) -> String {
    format!("{:.1} KB", bytes as f64 / 1024.0)
}

fn signed_kb(bytes: i64) -> String {
    format!("{:+.1} KB", bytes as f64 / 1024.0)
}

fn print_size(label: &str, s: &BuildSize) {
    println!("{label}");
    println!(
        "  wasm   {:>10} raw  {:>10} gzip",
        kb(s.wasm_raw),
        kb(s.wasm_gz)
    );
    println!(
        "  js     {:>10} raw  {:>10} gzip",
        kb(s.js_raw),
        kb(s.js_gz)
    );
    println!(
        "  total  {:>10} raw  {:>10} gzip  ({:.1}% of Unity's 8 MB web starting point)",
        kb(s.total_raw()),
        kb(s.total_gz()),
        s.total_gz() as f64 * 100.0 / UNITY_START_BYTES as f64
    );
    println!("  markers: {}", s.markers.join(" "));
    if !s.dev_only.is_empty() {
        println!("  dev-only (live code swapping): {}", s.dev_only.join(" "));
    }
}

fn report_json(
    app: &str,
    full: &BuildSize,
    rows: &[(String, i64, i64)],
    base: Option<&BuildSize>,
) -> String {
    let mut j = String::new();
    let _ = write!(
        j,
        "{{\n  \"app\": \"{app}\",\n  \"wasm_raw\": {},\n  \"wasm_gz\": {},\n  \"js_raw\": {},\n  \"js_gz\": {},\n  \"total_gz\": {},\n",
        full.wasm_raw,
        full.wasm_gz,
        full.js_raw,
        full.js_gz,
        full.total_gz()
    );
    let markers: Vec<String> = full.markers.iter().map(|m| format!("\"{m}\"")).collect();
    let _ = writeln!(j, "  \"markers\": [{}],", markers.join(", "));
    let dev: Vec<String> = full.dev_only.iter().map(|m| format!("\"{m}\"")).collect();
    let _ = writeln!(j, "  \"dev_only\": [{}],", dev.join(", "));
    if let Some(b) = base {
        let _ = writeln!(j, "  \"base_gz\": {},", b.total_gz());
    }
    j.push_str("  \"modules\": [");
    for (i, (name, raw, gz)) in rows.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        let _ = write!(
            j,
            "\n    {{\"module\": \"{name}\", \"raw\": {raw}, \"gz\": {gz}}}"
        );
    }
    j.push_str("\n  ]\n}\n");
    j
}

/// A playground (`[playground]` in evani.toml) ships only with free roam
/// declared, in its kit too, and tested (docs/PLAYGROUNDS.md; the same rule
/// `cargo xtask check` reports).
fn playground_guard(app: &AppDir) -> Result<(), String> {
    let text = std::fs::read_to_string(app.dir.join("evani.toml")).map_err(|e| e.to_string())?;
    let problems = match &app.external {
        None => evani_guide::playground::problems(&workspace_root(), &app.name, &app.manifest.kit, &text),
        Some(e) => evani_guide::playground::problems_in(&e.kit_dir, &app.dir.join("src"), &app.dir.join("evani.toml").display().to_string(), &app.manifest.kit, &text),
    };
    if problems.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = problems.iter().map(|p| format!("  {}\n  fix: {}", p.what, p.fix)).collect();
    Err(format!(
        "{}: refusing the ship build: a playground must support free roam (docs/PLAYGROUNDS.md)\n{}",
        app.name,
        lines.join("\n")
    ))
}

/// What the game's look gets on each tier it targets (assets/render_paths.json),
/// one line per tier; the same table `cargo xtask check` enforces.
fn render_path_lines(app: &AppDir) -> Vec<String> {
    let root = workspace_root();
    match evani_guide::render_paths::style_of_look(&root, &app.manifest.look) {
        Some(style) => evani_guide::render_paths::report(&root, &style, app.manifest.xr),
        None => Vec::new(),
    }
}

fn print_render_paths(app: &AppDir) {
    let lines = render_path_lines(app);
    if !lines.is_empty() {
        println!("render paths (look {}):", app.manifest.look);
        for l in lines {
            println!("  {l}");
        }
    }
}

/// A playground's shipped folder must fit the package cap; prints the total either way.
fn playground_size_guard(app: &AppDir) -> Result<(), String> {
    use evani_guide::playground as pg;
    let text = std::fs::read_to_string(app.dir.join("evani.toml")).map_err(|e| e.to_string())?;
    if !matches!(pg::parse(&text), Ok(Some(_))) {
        return Ok(());
    }
    let dist = app.sizes_dir().join(format!("{}-ship", app.name));
    println!(
        "  playground package: {} of {} cap (whole ship folder, uncompressed)",
        pg::mb(pg::dir_bytes(&dist)),
        pg::mb(pg::PACKAGE_CAP_BYTES)
    );
    if let Some(p) = pg::size_problem(&app.name, &dist) {
        return Err(format!("{}: ERROR: {}\n  fix: {}", app.name, p.what, p.fix));
    }
    // The runtime is MIT: every package carries its copyright and permission notice.
    pg::write_engine_notice(&workspace_root(), &dist)?;
    match pg::notice_problem(&app.name, &dist) {
        None => {
            println!("  engine notice: {} written (Evani.Dev runtime, MIT)", pg::ENGINE_NOTICE_FILE);
            Ok(())
        }
        Some(p) => Err(format!("{}: ERROR: {}\n  fix: {}", app.name, p.what, p.fix)),
    }
}

/// `cargo xtask package-check <folder> [<app>]`: a package folder (for example
/// `publish/<id>`) must carry the engine notice; run it before `evani_publish_check`.
fn cmd_package_check(dir: &str, app: &str) -> Result<(), String> {
    let dist = Path::new(dir);
    if !dist.is_dir() {
        return Err(format!("{dir}: not a folder"));
    }
    if let Some(p) = evani_guide::playground::notice_problem(app, dist) {
        return Err(format!("{dir}: ERROR: {}\n  fix: {}", p.what, p.fix));
    }
    println!("package-check: {dir} carries {} (clean)", evani_guide::playground::ENGINE_NOTICE_FILE);
    // With a game named, the whole publish preflight (publish.rs).
    if app == "<app>" {
        return Ok(());
    }
    let failed = publish::preflight_folder(&load_app(app)?, dist)?;
    if failed > 0 {
        return Err(format!("{dir}: the publish preflight failed ({failed} check(s))"));
    }
    Ok(())
}

fn cmd_build(app_name: &str, ship: bool, diff: bool) -> Result<(), String> {
    build_app(&load_app(app_name)?, ship, diff)
}

fn build_app(app: &AppDir, ship: bool, diff: bool) -> Result<(), String> {
    if let Some(e) = &app.external {
        println!("{}: outside the engine folder (game workspace {}, kit {})", app.name, e.root.display(), e.kit_dir.display());
    }
    if ship {
        playground_guard(app)?;
    }
    let m = if ship {
        app.manifest.clone().ship()
    } else {
        app.manifest.clone()
    };
    println!(
        "{} ({}): kit {}, look {}, features [{}]",
        app.name,
        m.name,
        m.kit,
        m.look,
        m.features().join(", ")
    );
    print_render_paths(app);
    let variant = if ship { "ship" } else { "full" };
    let full = trunk_build(app, variant, &m)?;
    print_size(&format!("\n{variant} build"), &full);
    if ship {
        ship_guard(&app.name, &full)?;
        println!("  ship guard: no live module, no live code swapping");
        let ship_dir = app.sizes_dir().join(format!("{}-ship", app.name));
        build_facts::write(app, &full, &ship_dir)?;
        playground_size_guard(app)?;
    }

    let mut rows = Vec::new();
    let mut base = None;
    if diff {
        println!("\nPer-module size (full build minus a build with that module off):");
        for (name, without) in m.without_each() {
            let s = trunk_build(
                app,
                &format!("no-{}", name.replace([' ', '(', ')'], "")),
                &without,
            )?;
            let raw = full.total_raw() as i64 - s.total_raw() as i64;
            let gz = full.total_gz() as i64 - s.total_gz() as i64;
            rows.push((name, raw, gz));
        }
        let b = trunk_build(app, "base", &m.base())?;
        println!("  {:<14} {:>12} {:>12}", "module", "raw", "gzip");
        for (name, raw, gz) in &rows {
            println!(
                "  {name:<14} {:>12} {:>12}",
                signed_kb(*raw),
                signed_kb(*gz)
            );
        }
        println!(
            "  {:<14} {:>12} {:>12}   (core + render + kit + look, every optional module off)",
            "base",
            kb(b.total_raw()),
            kb(b.total_gz())
        );
        base = Some(b);
    }
    let dir = app.sizes_dir();
    let path = dir.join(format!("{}-{variant}.json", app.name));
    let json = report_json(&app.name, &full, &rows, base.as_ref());
    if std::fs::write(&path, json).is_ok() {
        println!("\nreport: {}", path.display());
    }
    Ok(())
}

fn cmd_sizes(app_name: &str) -> Result<(), String> {
    let app = load_app(app_name)?;
    let full_m = app.manifest.clone();
    let mut no_bloom = full_m.clone();
    no_bloom.effects.retain(|e| e != "bloom");
    let mut no_fx = full_m.clone();
    no_fx.effects.clear();
    let mut no_live = full_m.clone();
    no_live.live = false;
    let variants = [
        ("full", full_m),
        ("no-bloom", no_bloom),
        ("no-fx", no_fx),
        ("no-live", no_live),
    ];
    let mut rows = Vec::new();
    for (name, m) in &variants {
        rows.push((name, trunk_build(&app, name, m)?));
    }
    let full_gz = rows.first().map_or(0, |(_, s)| s.total_gz());
    println!(
        "\n{:<10} {:>11} {:>11} {:>10}  {:<6} {:<6} {:<6} {:<6} {:<6} {:<6} {:<6}",
        "variant", "wasm raw", "total gzip", "vs full", "bloom", "fx", "live", "pixel", "models", "anim", "ui"
    );
    for (name, s) in &rows {
        let yn = |b: bool| if b { "yes" } else { "no" };
        println!(
            "{name:<10} {:>11} {:>11} {:>10}  {:<6} {:<6} {:<6} {:<6} {:<6} {:<6} {:<6}",
            kb(s.wasm_raw),
            kb(s.total_gz()),
            signed_kb(s.total_gz() as i64 - full_gz as i64),
            yn(s.has_prefix("evani.shader.bloom")),
            yn(s.has("evani.module.fx")),
            yn(s.has("evani.module.live")),
            yn(s.has_prefix("evani.text.")),
            yn(s.has("evani.module.models")),
            yn(s.has("evani.module.anim")),
            yn(s.has("evani.module.ui")),
        );
    }
    println!("\nbloom = bloom WGSL markers in the wasm; fx/live/models/anim/ui = module id markers; pixel = the bitmap font (feature `text`; the sharp MSDF text is the `ui` module).");
    Ok(())
}

/// `cargo xtask live [--game <name|apps/name>] [--port <P>]`: build the live
/// bridge, then run `serve` in the foreground (it builds the game with the
/// `live` module itself and serves it at http://localhost:<P>/?live=1).
/// Live code swapping follows the bridge's default (on, with the Trunk
/// fallback); `--no-hot` turns it off.
fn cmd_live(args: &[String]) -> Result<(), String> {
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let (game, port, mut extra) = live_args(args);
    let game = value("--game").unwrap_or(game);
    // No --port: 8096, or the next free port if another session has it
    // (LIVE_API.md 2.6). Model mode keeps 8097, which the Model tab expects.
    if value("--port").is_none() && !args.iter().any(|a| a == "--models") {
        extra.push("--any-port".into());
    }
    let port = value("--port").unwrap_or(port);
    let hot = !args.iter().any(|a| a == "--no-hot");
    run_bridge(&game, &port, hot, &extra)
}

/// `live`'s defaults and extra bridge arguments. `--models [<folder>]` is
/// model mode for the Model tab's page (docs/EVANI_MODEL_TAB.md 4.2): the
/// viewer app on port 8097, its own session file, and the Evani.Model
/// workspace (default `.evani/model`).
fn live_args(args: &[String]) -> (String, String, Vec<String>) {
    let Some(i) = args.iter().position(|a| a == "--models") else {
        return ("sandbox".into(), "8096".into(), Vec::new());
    };
    let folder = args.get(i + 1).filter(|f| !f.starts_with("--")).cloned().unwrap_or_else(|| ".evani/model".into());
    let extra = vec!["--session".into(), ".evani/live/model/session.json".into(), "--models".into(), folder];
    ("apps/model-view".into(), "8097".into(), extra)
}

/// `cargo xtask dev [--hot] <game> [--port <P>]`: the same `serve` as
/// `live`, named for the edit loop. `--hot` (live code swapping with the
/// pinned dx, Trunk if dx is missing or fails) is what the Evani.dev tab
/// gets by default; without it the game is built with Trunk only.
fn cmd_dev(args: &[String]) -> Result<(), String> {
    let (game, port, hot) = dev_args(args)?;
    run_bridge(&game, &port, hot, &dev_extra(args))
}

/// `dev` without `--port` moves to a free port when 8096 is taken (2.6).
fn dev_extra(args: &[String]) -> Vec<String> {
    if args.iter().any(|a| a == "--port") { Vec::new() } else { vec!["--any-port".into()] }
}

/// `(game, port, hot)` from `dev`'s arguments.
fn dev_args(args: &[String]) -> Result<(String, String, bool), String> {
    let mut game = None;
    let mut port = "8096".to_string();
    let mut hot = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hot" => hot = true,
            "--no-hot" => hot = false,
            "--port" => port = it.next().cloned().ok_or("--port needs a value")?,
            "--game" => game = Some(it.next().cloned().ok_or("--game needs a value")?),
            s if s.starts_with("--") => return Err(format!("dev: unknown option {s}")),
            s => game = Some(s.to_string()),
        }
    }
    let game = game.ok_or_else(usage)?;
    Ok((game, port, hot))
}

/// The dioxus git revision the pinned dx is built from. Released dx 0.7.x
/// can't read the wasm-bindgen output this workspace makes; keep this equal
/// to `DX_REV` in tools/evani-live-bridge/src/code.rs.
pub const DX_REV: &str = "f420ca3d6cb6812ed27a021e239d7ec0941ff317";

/// The `cargo install` that puts the pinned dx in `target/evani-tools/dx`,
/// where the live bridge looks for it first (after `EVANI_DX`).
fn dx_install_args(root: &Path) -> Vec<String> {
    let into = root.join("target").join("evani-tools").join("dx");
    vec![
        "install".into(),
        "--git".into(),
        "https://github.com/DioxusLabs/dioxus".into(),
        "--rev".into(),
        DX_REV.into(),
        "dioxus-cli".into(),
        "--root".into(),
        into.display().to_string(),
    ]
}

/// `cargo xtask dx-install`: build the pinned dx (about 30 minutes on 2
/// jobs). Pass `--print` to only show the command.
fn cmd_dx_install(args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let cargo_args = dx_install_args(&root);
    println!("cargo {}", cargo_args.join(" "));
    if args.iter().any(|a| a == "--print") {
        return Ok(());
    }
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(&cargo_args).current_dir(&root);
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "2");
    }
    let status = cmd.status().map_err(|e| format!("cargo install: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo install dioxus-cli exited with {status}"))
    }
}

fn run_bridge(game: &str, port: &str, hot: bool, extra: &[String]) -> Result<(), String> {
    port.parse::<u16>().map_err(|_| format!("bad --port {port}"))?;
    let root = workspace_root();
    let status = Command::new(env!("CARGO"))
        .args(["build", "-j", "4", "-p", "evani-live-bridge"])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err("evani-live-bridge did not build".into());
    }
    let exe = root
        .join("target")
        .join("debug")
        .join(format!("evani-live-bridge{}", std::env::consts::EXE_SUFFIX));
    let mut cmd = Command::new(exe);
    cmd.arg("serve")
        .arg("--root")
        .arg(&root)
        .args(["--port", port, "--game", game])
        .args(extra);
    if !hot {
        cmd.arg("--no-hot");
    }
    let status = cmd
        .current_dir(&root)
        .status()
        .map_err(|e| format!("evani-live-bridge: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("evani-live-bridge exited with {status}"))
    }
}

/// `cargo xtask geocheck <app> [options]`: the geometry integrity report
/// (`cargo run -p evani-geocheck -- <app> [options]`, see docs/GEOCHECK.md).
/// Fails when the report has untagged issues or the check could not run.
fn cmd_geocheck(args: &[String]) -> Result<(), String> {
    let Some(app) = args.first().filter(|a| !a.starts_with("--")) else {
        return Err(usage());
    };
    // A 2D kit ships a scene-dump example; its games go through
    // evani-geocheck-2d instead of the 3D runner.
    if let Ok(a) = load_app(app) {
        let kit = format!("evani-kit-{}", a.manifest.kit);
        let root = workspace_root();
        if root.join("kits").join(&kit).join("examples").join("geocheck_dump.rs").is_file() {
            return geocheck_2d(&root, &a, &kit);
        }
    }
    let status = Command::new(env!("CARGO"))
        .args(["run", "-p", "evani-geocheck", "--"])
        .args(args)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("cargo run -p evani-geocheck: {e}"))?;
    let report = if !args.iter().any(|a| a == "--zfight") {
        "report.md"
    } else if args.iter().any(|a| a == "--sample") {
        "zfight-sample/zfight.md, a partial report"
    } else {
        "zfight/zfight.md"
    };
    match status.code() {
        Some(0) => Ok(()),
        Some(1) => Err(format!("geocheck: untagged geometry issues (see .vibedeck/geocheck/{app}/{report})")),
        _ => Err(format!("geocheck could not run ({status})")),
    }
}

/// The 2D route of `cargo xtask geocheck <app>`: the kit's `geocheck_dump`
/// example writes `.vibedeck/geocheck/<app>/dump/scene.json` (and its
/// atlas), then `evani-geocheck-2d` writes the report next to it. Tags come
/// from `apps/<app>/geocheck.toml`, else `kits/evani-kit-<kit>/geocheck.toml`,
/// when one exists (the scene's own `geocheck` array always applies).
fn geocheck_2d(root: &Path, app: &AppDir, kit: &str) -> Result<(), String> {
    let out = root.join(".vibedeck").join("geocheck").join(&app.name);
    let tags = [app.dir.join("geocheck.toml"), root.join("kits").join(kit).join("geocheck.toml")];
    geocheck_2d_at(root, root, kit, &out, &tags)
}

/// The 2D route with its folders given: the kit's dump runs in `game_ws`
/// (the engine, or an outside game's workspace), `evani-geocheck-2d` in the
/// engine at `root`; the report goes to `out`, tags from the first of `tags`
/// that exists.
fn geocheck_2d_at(root: &Path, game_ws: &Path, kit: &str, out: &Path, tags: &[PathBuf]) -> Result<(), String> {
    let dump = out.join("dump");
    let crops = out.join("crops");
    if crops.is_dir() {
        // Crops are numbered per run; stale ones from a longer run would linger.
        std::fs::remove_dir_all(&crops).map_err(|e| format!("{}: {e}", crops.display()))?;
    }
    let status = Command::new(env!("CARGO"))
        .args(["run", "--release", "-p", kit, "--example", "geocheck_dump", "--"])
        .arg(&dump)
        .current_dir(game_ws)
        .status()
        .map_err(|e| format!("cargo run -p {kit} --example geocheck_dump: {e}"))?;
    if !status.success() {
        return Err(format!("geocheck: the scene dump failed ({status})"));
    }
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["run", "--release", "-p", "evani-geocheck-2d", "--", "--scene"])
        .arg(dump.join("scene.json"))
        .arg("--out")
        .arg(out);
    if let Some(t) = tags.iter().find(|t| t.is_file()) {
        cmd.arg("--tags").arg(t);
    }
    let status = cmd
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo run -p evani-geocheck-2d: {e}"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1) => Err(format!("geocheck: untagged issues (see {})", out.join("report.md").display())),
        _ => Err(format!("geocheck-2d could not run ({status})")),
    }
}

/// `cargo xtask geocheck --app-dir <folder> [options]` for a game outside the
/// engine (docs/GEOCHECK.md "Games outside the engine"). The engine's 3D
/// runner only has adapters for the engine's own kits, so the game's kit
/// brings its own: `examples/geocheck_dump.rs` takes the 2D route (as in
/// apps/), and `examples/geocheck.rs` is a 3D adapter that runs the game
/// natively with the evani-geocheck library and writes `report.json` and
/// `report.md` into `--out` (exit 0 clean, 1 untagged issues, 2 could not
/// run). Both run in the game's workspace; the report goes to
/// `<game workspace>/.vibedeck/geocheck/<id>/`, where `check --app-dir`
/// reads it.
fn geocheck_app_dir(app: &AppDir, args: &[String]) -> Result<(), String> {
    let ext = app.external.as_ref().ok_or("geocheck --app-dir: not a game outside the engine")?;
    let kit_dir = &ext.kit_dir;
    let kit = app_dir::package_name(kit_dir).unwrap_or_else(|| format!("evani-kit-{}", app.manifest.kit));
    let out = ext.root.join(".vibedeck").join("geocheck").join(&app.name);
    let tags = [app.dir.join("geocheck.toml"), kit_dir.join("geocheck.toml")];
    let arg = format!("--app-dir {}", app.dir.display());
    // The kit's example runs with `cargo run -p` in the game's workspace, so
    // the kit must be one of its members.
    if geocheck_route(kit_dir).is_some() && !std::fs::canonicalize(kit_dir).ok().zip(std::fs::canonicalize(&ext.root).ok()).is_some_and(|(k, r)| k.starts_with(r)) {
        return Err(format!(
            "ERROR: geocheck {arg}: the kit crate {} is outside the game's workspace {}, so its example can't be run from there\n  fix: make the kit a member of the game workspace (its path under {}), or run the geocheck from the workspace that holds the kit",
            kit_dir.display(),
            ext.root.display(),
            ext.root.display()
        ));
    }
    match geocheck_route(kit_dir) {
        Some(GeoRoute::Dump) => geocheck_2d_at(&workspace_root(), &ext.root, &kit, &out, &tags).map_err(|e| {
            format!("ERROR: geocheck {arg} (the kit's 2D scene dump): {e}\n  fix: tag or fix each issue in {} (docs/GEOCHECK.md), or fix what stopped the dump (its cargo output above)", out.join("report.md").display())
        }),
        Some(GeoRoute::Adapter) => {
            std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            let status = Command::new(env!("CARGO"))
                .args(["run", "--release", "-p", &kit, "--example", "geocheck", "--", "--out"])
                .arg(&out)
                .args(["--app", &app.name])
                .args(args)
                .current_dir(&ext.root)
                .status()
                .map_err(|e| format!("cargo run -p {kit} --example geocheck: {e}"))?;
            match status.code() {
                Some(0) if out.join("report.json").is_file() => Ok(()),
                Some(0) => Err(format!(
                    "ERROR: geocheck {arg}: the kit's adapter exited 0 but wrote no {}\n  fix: the adapter must write report.json and report.md into the folder it gets as --out (docs/GEOCHECK.md \"Games outside the engine\")",
                    out.join("report.json").display()
                )),
                Some(1) => Err(format!(
                    "ERROR: geocheck {arg}: untagged geometry issues (see {})\n  fix: fix each one in the kit's level or scene data, or tag it with a reason in {} if it is intended (docs/GEOCHECK.md, tags)",
                    out.join("report.md").display(),
                    app.dir.join("geocheck.toml").display()
                )),
                _ => Err(format!(
                    "ERROR: geocheck {arg}: the kit's adapter could not run ({status})\n  fix: run `cargo run --release -p {kit} --example geocheck -- --out <folder>` in {} and fix what it reports (exit 2 means it could not run)",
                    ext.root.display()
                )),
            }
        }
        None => Err(format!(
            "ERROR: no geocheck adapter for {}: a game outside the engine brings its own in its kit ({})\n  fix: add `examples/geocheck_dump.rs` (a 2D scene dump, tools/evani-geocheck-2d README) or `examples/geocheck.rs` (a 3D adapter that writes report.json and report.md into --out), docs/GEOCHECK.md \"Games outside the engine\"",
            app.name,
            kit_dir.display()
        )),
    }
}

/// Which of the kit's own geocheck hooks a game outside the engine uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeoRoute {
    /// `examples/geocheck_dump.rs`: the 2D scene dump.
    Dump,
    /// `examples/geocheck.rs`: a 3D adapter that writes its own report.
    Adapter,
}

fn geocheck_route(kit_dir: &Path) -> Option<GeoRoute> {
    let ex = kit_dir.join("examples");
    if ex.join("geocheck_dump.rs").is_file() {
        Some(GeoRoute::Dump)
    } else if ex.join("geocheck.rs").is_file() {
        Some(GeoRoute::Adapter)
    } else {
        None
    }
}

/// `cargo xtask manifest <app>`: write the streamed-model manifest (v2) for
/// the files in `apps/<app>/assets/models` (evani-guide `stream`).
fn cmd_manifest(app: &str) -> Result<(), String> {
    manifest_in(&workspace_root().join("apps").join(app), &format!("apps/{app}"))
}

/// [`cmd_manifest`] for the game in `game` (shown as `app`).
fn manifest_in(game: &Path, app: &str) -> Result<(), String> {
    let toml = std::fs::read_to_string(game.join("evani.toml")).map_err(|e| format!("{app}/evani.toml: {e}"))?;
    let dir = game.join("assets/models");
    if !dir.is_dir() {
        return Err(format!("{app}/assets/models does not exist (only games with models need a manifest)"));
    }
    let out = dir.join("stream.json");
    match evani_guide::stream::build_levels(&dir, &evani_guide::stream::boot_list(&toml), &evani_guide::stream::levels_list(&toml))? {
        Some(text) => {
            std::fs::write(&out, &text).map_err(|e| format!("{}: {e}", out.display()))?;
            let n = text.matches("\"name\"").count();
            println!("manifest: wrote {app}/{} ({n} files)", evani_guide::stream::PATH);
        }
        None => println!("manifest: {app}/assets/models has no files; nothing written"),
    }
    Ok(())
}

/// `cargo xtask convert <file> ...`: the model converter (tools/evani-convert,
/// dev only, never linked into a game). Paths are made absolute against the
/// caller's folder first, since the converter runs from the engine root.
fn cmd_convert(args: &[String]) -> Result<(), String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut fixed = Vec::with_capacity(args.len());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--id" | "--look" | "--min-distance" | "--tiers" | "--clip-error" => {
                fixed.push(a.clone());
                if let Some(v) = it.next() {
                    fixed.push(v.clone());
                }
            }
            "--out" | "--gate-pairs" | "--calibration" | "--calibration-result" => {
                fixed.push(a.clone());
                if let Some(v) = it.next() {
                    fixed.push(here.join(v).to_string_lossy().into_owned());
                }
            }
            f if f.starts_with('-') => fixed.push(a.clone()),
            f => fixed.push(here.join(f).to_string_lossy().into_owned()),
        }
    }
    let status = Command::new(env!("CARGO"))
        .args(["run", "--release", "--quiet", "-p", "evani-convert", "--"])
        .args(&fixed)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("cargo run -p evani-convert: {e}"))?;
    match status.code() {
        Some(0) => Ok(()),
        // 1: converted, but the report lists problems; 2: could not convert.
        Some(c @ (1 | 2)) => std::process::exit(c),
        _ => Err(format!("convert could not run ({status})")),
    }
}

fn usage() -> String {
    format!(
        "usage:\n  cargo xtask build <app> [--ship] [--no-diff]\n  cargo xtask build|features|render-paths|manifest|check|geocheck|netlog|review|critic|size-report --app-dir <game app folder> [..]   (a game outside the engine folder)\n  cargo xtask render-paths <app>\n  cargo xtask sizes [<app>]\n  cargo xtask features <app> [--ship]\n  cargo xtask live [--game <app>] [--port <P>] [--no-hot] [--models [<folder>]]\n  cargo xtask dev [--hot] <app> [--port <P>]\n  cargo xtask dx-install [--print]\n  cargo xtask geocheck <app> [--no-render] [--port 8095] [--title-s 40] [--race-s 30] [--every 6] [--highlights 40] [--keep-raw] [--zfight [--report-only] [--sample N]]\n  cargo xtask package-check <folder> [<app>]\n  cargo xtask convert <file.glb|.gltf|.blend|<name>.<tier>.glb> [--id <id>] [--look <look> [--snap-palette]] [--min-distance <m>] [--tiers low,medium,high,ultra] [--clip-error <m>] [--out <folder>] [--keep-work]\n  cargo xtask blender-addon [--test]   (builds the Blender add-on zip under target/blender-addon; --test runs it in a headless Blender)\n  cargo xtask manifest <app>   (writes assets/models/stream.json: tier, priority, group per file)\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        netlog::usage(),
        bytes::usage(),
        importance::usage(),
        evani_guide::cli::USAGE,
        fonts::usage(),
        model_import::usage(),
        size_report::usage(),
        publish::usage(),
        bench::usage(),
        review::usage(),
        critic::usage()
    )
}

fn run(args: &[String]) -> Result<(), String> {
    let (app_dir, args) = app_dir::take_arg(args)?;
    if let Some(dir) = app_dir {
        return run_app_dir(&dir, &args);
    }
    let args = &args[..];
    let flag = |f: &str| args.iter().any(|a| a == f);
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    match args.first().map(String::as_str) {
        Some("build") => {
            let app = positional.first().ok_or_else(usage)?;
            cmd_build(app, flag("--ship"), !flag("--no-diff"))
        }
        Some("render-paths") => {
            let app = load_app(positional.first().ok_or_else(usage)?)?;
            print_render_paths(&app);
            Ok(())
        }
        Some("package-check") => cmd_package_check(positional.first().ok_or_else(usage)?, positional.get(1).map_or("<app>", |s| s.as_str())),
        Some("bench") => bench::cmd_bench(&args[1..]),
        Some("review") => review::cmd_review(&args[1..]),
        Some("critic") => critic::cmd_critic(&args[1..]),
        Some("live") => cmd_live(&args[1..]),
        Some("dev") => cmd_dev(&args[1..]),
        Some("dx-install") => cmd_dx_install(&args[1..]),
        Some("geocheck") => cmd_geocheck(&args[1..]),
        Some("convert") => cmd_convert(&args[1..]),
        Some("blender-addon") => blender_addon::cmd_blender_addon(&args[1..], &workspace_root()),
        Some("size-report") => size_report::cmd_size_report(&args[1..]),
        Some("publish") => publish::cmd_publish(&args[1..]),
        Some("model-import") => model_import::cmd_model_import(&args[1..], &workspace_root()),
        Some("manifest") => cmd_manifest(positional.first().ok_or_else(usage)?),
        Some("netlog") => netlog::cmd_netlog(&args[1..]),
        Some("bytes") => bytes::cmd_bytes(&args[1..]),
        Some("importance") => importance::cmd_importance(&args[1..]),
        Some("fonts") => fonts::cmd_fonts(&args[1..], &workspace_root()),
        Some("sizes") => cmd_sizes(positional.first().map_or("sandbox", |s| s.as_str())),
        Some("features") => {
            let app = load_app(positional.first().ok_or_else(usage)?)?;
            let m = if flag("--ship") {
                app.manifest.ship()
            } else {
                app.manifest
            };
            println!("{}", m.features().join(","));
            Ok(())
        }
        _ => Err(usage()),
    }
}

/// The commands that take `--app-dir <dir>` (a game outside the engine
/// folder) instead of an id under `apps/`.
fn run_app_dir(dir: &Path, args: &[String]) -> Result<(), String> {
    let flag = |f: &str| args.iter().any(|a| a == f);
    let cmd = args.first().map(String::as_str);
    if let Some(e) = app_dir::positionals(&args[args.len().min(1)..], app_dir::value_options(cmd.unwrap_or(""))).first() {
        return Err(format!("`{e}`: with --app-dir the game is the folder; give no game id"));
    }
    if cmd == Some("check") {
        // A folder that can't be read as a game is the gate's first ERROR.
        let app = load_app_dir_with(dir, false).map_err(|e| {
            if e.starts_with("ERROR") {
                e
            } else {
                format!("ERROR: {e}\n  fix: correct {} (README \"evani.toml\"), then run `cargo xtask check --app-dir {}` again", dir.join("evani.toml").display(), dir.display())
            }
        })?;
        let ext = app.external.as_ref().ok_or("check --app-dir: not a game outside the engine")?;
        let game = evani_guide::check::Game::outside(&app.name, &app.dir, &ext.root, &ext.kit_dir);
        return match evani_guide::cli::check_game(&workspace_root(), &game, &args[1..]) {
            0 => Ok(()),
            // The report (or the argument error) is printed above it.
            _ => Err("check --app-dir: not clean".into()),
        };
    }
    let app = load_app_dir(dir)?;
    match cmd {
        Some("build") => build_app(&app, flag("--ship"), !flag("--no-diff")),
        Some("features") => {
            let m = if flag("--ship") { app.manifest.ship() } else { app.manifest };
            println!("{}", m.features().join(","));
            Ok(())
        }
        Some("render-paths") => {
            print_render_paths(&app);
            Ok(())
        }
        Some("manifest") => manifest_in(&app.dir, &app.dir.display().to_string()),
        Some("geocheck") => geocheck_app_dir(&app, &args[1..]),
        Some("netlog") => netlog::cmd_netlog_app(&app, &args[1..]),
        Some("review") => review::cmd_review_app(&app, &args[1..]),
        Some("critic") => critic::cmd_critic_app(&app, &args[1..]),
        Some("size-report") => size_report::cmd_size_report_app(&app, &args[1..]),
        Some(other) => Err(format!("`cargo xtask {other}` does not take --app-dir yet; it takes: build (--ship), features, render-paths, manifest, check, geocheck, netlog, review, critic, size-report")),
        None => Err(usage()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The AI guidance commands live in evani-guide (shared with its small
    // binary, which the Claude Code Stop hook runs).
    // `check --app-dir` is resolved here (the game's workspace, kit and
    // engine, as for build), then runs evani-guide's check on that game.
    let check_app_dir = args.first().is_some_and(|a| a == "check") && app_dir::has_arg(&args);
    if !check_app_dir && matches!(args.first().map(String::as_str), Some("init-ai" | "doctor" | "check" | "new-game" | "catalog" | "ai-eval")) {
        std::process::exit(evani_guide::cli::run(&args));
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy of `apps/<game>` in its own Cargo workspace at `dest` (outside
    /// the engine folder): the engine's `[workspace.package]`,
    /// `[workspace.dependencies]` (paths made absolute into the engine),
    /// lints, profiles and `Cargo.lock`, and the app as `dest/apps/<game>`
    /// (the game's own folder name is part of its panic paths, so the same
    /// layout gives the same bytes). No `.cargo/config.toml`: xtask passes
    /// the wasm cfg itself.
    pub(crate) fn out_of_tree_fixture(engine: &Path, game: &str, dest: &Path) -> PathBuf {
        let _ = std::fs::remove_dir_all(dest);
        let ws: toml::Table = std::fs::read_to_string(engine.join("Cargo.toml")).unwrap().parse().unwrap();
        let mut w = ws["workspace"].as_table().unwrap().clone();
        w.insert("members".into(), toml::Value::Array(vec![format!("apps/{game}").into()]));
        w.remove("exclude");
        w.remove("default-members");
        if let Some(deps) = w.get_mut("dependencies").and_then(toml::Value::as_table_mut) {
            for (_, dep) in deps.iter_mut() {
                if let Some(p) = dep.get("path").and_then(toml::Value::as_str).map(str::to_string) {
                    let abs = engine.join(p).to_string_lossy().replace('\\', "/");
                    dep.as_table_mut().unwrap().insert("path".into(), abs.into());
                }
            }
        }
        let mut out = toml::Table::new();
        out.insert("workspace".into(), toml::Value::Table(w));
        if let Some(p) = ws.get("profile") {
            out.insert("profile".into(), p.clone());
        }
        std::fs::create_dir_all(dest).unwrap();
        std::fs::write(dest.join("Cargo.toml"), toml::to_string(&out).unwrap()).unwrap();
        std::fs::copy(engine.join("Cargo.lock"), dest.join("Cargo.lock")).unwrap();
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let name = e.file_name();
                if ["dist", "target", "review"].iter().any(|s| name == *s) {
                    continue;
                }
                if e.path().is_dir() {
                    copy(&e.path(), &to.join(&name));
                } else {
                    std::fs::copy(e.path(), to.join(&name)).unwrap();
                }
            }
        }
        let app = dest.join("apps").join(game);
        copy(&engine.join("apps").join(game), &app);
        app
    }

    /// The content of each shipped `.wasm` / `.js` file of a ship folder, by
    /// extension (Trunk's file names carry a hash of the build folder).
    fn shipped(dist: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dist)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "wasm" || x == "js"))
            .map(|p| (p.extension().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
            .collect();
        v.sort();
        v
    }

    /// The source paths baked into a wasm's panic locations (`crates\...\x.rs`).
    fn panic_paths(wasm: &[u8]) -> std::collections::BTreeSet<String> {
        let path_byte = |b: u8| b.is_ascii_alphanumeric() || b"_-./\\:".contains(&b);
        let mut out = std::collections::BTreeSet::new();
        for i in 0..wasm.len().saturating_sub(3) {
            if &wasm[i..i + 3] == b".rs" && !wasm.get(i + 3).is_some_and(|b| b.is_ascii_alphanumeric()) {
                let start = (0..i).rev().take_while(|&j| path_byte(wasm[j])).last().unwrap_or(i);
                if wasm[start..i].iter().any(|b| *b == b'/' || *b == b'\\') {
                    out.insert(String::from_utf8_lossy(&wasm[start..i + 3]).into_owned());
                }
            }
        }
        out
    }

    /// wasm-bindgen's glue with its closure-shim function numbers blanked
    /// (`__wasm_bindgen_func_elem_<n>`): they are the wasm's function indices.
    fn js_without_func_numbers(js: &[u8]) -> Vec<u8> {
        const P: &[u8] = b"__wasm_bindgen_func_elem_";
        let mut out = Vec::with_capacity(js.len());
        let mut i = 0;
        while i < js.len() {
            if js[i..].starts_with(P) {
                out.extend_from_slice(P);
                i += P.len();
                while i < js.len() && js[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(b'#');
            } else {
                out.push(js[i]);
                i += 1;
            }
        }
        out
    }

    #[test]
    fn the_out_of_tree_comparison_reads_paths_and_blanks_shim_numbers() {
        let wasm = b"\x01\x02crates\\evani-core\\src\\num.rs\x00\x07/cargo/x/src/lib.rs\x00a/foo.rsx not.rs";
        let p = panic_paths(wasm);
        assert_eq!(p.into_iter().collect::<Vec<_>>(), ["/cargo/x/src/lib.rs", "crates\\evani-core\\src\\num.rs"]);
        assert_eq!(js_without_func_numbers(b"a(__wasm_bindgen_func_elem_2121, x_12)"), b"a(__wasm_bindgen_func_elem_#, x_12)");
        assert_eq!(js_without_func_numbers(b"__wasm_bindgen_func_elem_7_3"), js_without_func_numbers(b"__wasm_bindgen_func_elem_918_3"));
    }

    /// The same shipped game built inside (`apps/match-3`) and outside the
    /// engine folder (`--app-dir`, laid out as `apps/match-3` in its own
    /// workspace) ships the same code. Not byte for byte: Cargo hashes a path
    /// package outside the workspace by its absolute folder, so the engine's
    /// crates get other `-C metadata` hashes there, and the linker orders
    /// their functions differently (a few bytes; function numbers shift).
    /// What is equal: the JS glue apart from those function numbers, every
    /// panic-location path (no builder folder, engine files read as in
    /// `apps/`), and the wasm size to 0.1%. Two release builds, so not in the
    /// normal run:
    /// `EVANI_GIT_COMMIT=<c> cargo test -p evani-xtask -- --ignored out_of_tree`.
    #[test]
    #[ignore = "two release wasm builds (minutes); run by hand for the --app-dir proof"]
    fn out_of_tree_ship_build_matches_in_tree() {
        let engine = workspace_root();
        let outside = std::env::temp_dir().join(format!("evani-out-of-tree-{}", std::process::id()));
        let app_dir = out_of_tree_fixture(&engine, "match-3", &outside);
        let app = load_app_dir(&app_dir).unwrap();
        assert_eq!(app.name, "match-3");
        assert_eq!(app.kit_dir(), engine.join("kits").join("evani-kit-match-3"));
        build_app(&app, true, false).unwrap();
        cmd_build("match-3", true, false).unwrap();
        let out = shipped(&app.sizes_dir().join("match-3-ship"));
        let inside = shipped(&engine.join("target").join("evani-sizes").join("match-3-ship"));
        assert_eq!(out.len(), 2, "one wasm and one js");
        assert_eq!(inside.len(), 2, "one wasm and one js");
        let (js_out, js_in) = (&out[0].1, &inside[0].1);
        let (wasm_out, wasm_in) = (&out[1].1, &inside[1].1);
        assert_eq!((out[0].0.as_str(), out[1].0.as_str()), ("js", "wasm"));
        assert!(js_without_func_numbers(js_out) == js_without_func_numbers(js_in), "js: outside {} B, inside {} B", js_out.len(), js_in.len());
        let (p_out, p_in) = (panic_paths(wasm_out), panic_paths(wasm_in));
        assert!(!p_in.is_empty());
        assert_eq!(p_out, p_in, "panic paths differ");
        assert!(p_out.iter().all(|p| !p.contains(':')), "a builder folder in the wasm: {p_out:?}");
        let (a, b) = (wasm_out.len() as f64, wasm_in.len() as f64);
        assert!((a - b).abs() <= b * 0.001, "wasm: outside {a} B, inside {b} B");
        let facts: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(app.sizes_dir().join("match-3-ship").join(build_facts::FACTS_FILE)).unwrap()).unwrap();
        assert!(facts["game"]["commit"].is_string() && facts["engine"]["commit"].is_string(), "{facts}");
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_game_outside_the_engine_loads_from_its_folder() {
        let engine = workspace_root();
        let outside = std::env::temp_dir().join(format!("evani-out-of-tree-load-{}", std::process::id()));
        let app_dir = out_of_tree_fixture(&engine, "match-3", &outside);
        let app = load_app_dir(&app_dir).unwrap();
        let inside = load_app("match-3").unwrap();
        assert_eq!(app.name, "match-3");
        assert_eq!(app.manifest.clone().ship().features(), inside.manifest.clone().ship().features());
        let e = app.external.clone().unwrap();
        assert_eq!(e.root, outside);
        assert_eq!(app.kit_dir(), inside.kit_dir());
        assert_eq!(app.sizes_dir(), outside.join("target").join("evani-sizes"));
        assert_eq!(inside.sizes_dir(), engine.join("target").join("evani-sizes"));
        assert!(inside.external.is_none());
        // A folder without an evani.toml says what --app-dir wants.
        let err = load_app_dir(&outside).map(|_| ()).unwrap_err();
        assert!(err.contains("--app-dir takes the game's app folder"), "{err}");
        // A game id may not be given beside --app-dir; unsupported commands say so.
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(run(&a(&["build", "match-3", "--app-dir", &app_dir.to_string_lossy()])).unwrap_err().contains("give no game id"));
        assert!(run(&a(&["publish", "--app-dir", &app_dir.to_string_lossy()])).unwrap_err().contains("does not take --app-dir yet"));
        run(&a(&["features", "--ship", "--app-dir", &app_dir.to_string_lossy()])).unwrap();
        // check, geocheck and netlog take it; option values are not game ids.
        let dir = app_dir.to_string_lossy().into_owned();
        run(&a(&["check", "--app-dir", &dir])).unwrap();
        assert!(run(&a(&["check", "--app-dir", &dir, "--app", "x"])).unwrap_err().contains("give no game id"));
        assert!(run(&a(&["check", "--app-dir", &dir, "--quick"])).is_err());
        assert!(!run(&a(&["netlog", "--app-dir", &dir, "--tier", "nope"])).unwrap_err().contains("game id"), "--tier's value is not a game id");
        assert!(run(&a(&["netlog", "--app-dir", &dir, "--tier", "nope"])).unwrap_err().contains("unknown tier nope"));
        // The module on/off rule: a plain dependency on an optional engine
        // crate is refused at load by build; check --app-dir reports it as an
        // ERROR line with the rest of the gate.
        let cargo = std::fs::read_to_string(app_dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("evani-net = { workspace = true, optional = true }"), "fixture changed");
        std::fs::write(app_dir.join("Cargo.toml"), cargo.replace("evani-net = { workspace = true, optional = true }", "evani-net = { workspace = true }")).unwrap();
        let err = load_app_dir(&app_dir).map(|_| ()).unwrap_err();
        assert!(err.contains("ERROR: `evani-net` is a plain dependency") && err.contains("fix:"), "{err}");
        assert_eq!(run(&a(&["check", "--app-dir", &dir])).unwrap_err(), "check --app-dir: not clean");
        std::fs::write(app_dir.join("Cargo.toml"), &cargo).unwrap();
        // A name that is not a game id: an ERROR line (the load is shared by every --app-dir command).
        let toml = std::fs::read_to_string(app_dir.join("evani.toml")).unwrap();
        std::fs::write(app_dir.join("evani.toml"), toml.replace("name = \"match-3\"", "name = \"Match 3\"")).unwrap();
        let err = load_app_dir(&app_dir).map(|_| ()).unwrap_err();
        assert!(err.starts_with("ERROR: ") && err.contains("not a game id"), "{err}");
        // An evani.toml that doesn't parse: check --app-dir says so as an ERROR with a fix.
        std::fs::write(app_dir.join("evani.toml"), "name = \"match-3\nkit = ").unwrap();
        let err = run(&a(&["check", "--app-dir", &dir])).unwrap_err();
        assert!(err.starts_with("ERROR: ") && err.contains("evani.toml") && err.contains("\n  fix: correct "), "{err}");
        std::fs::write(app_dir.join("evani.toml"), &toml).unwrap();
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn an_outside_kit_brings_its_own_geocheck_hook() {
        let base = std::env::temp_dir().join(format!("evani-geocheck-route-{}", std::process::id()));
        let kit = |name: &str, files: &[&str]| {
            let d = base.join(name);
            std::fs::create_dir_all(d.join("examples")).unwrap();
            for f in files {
                std::fs::write(d.join("examples").join(f), "fn main() {}\n").unwrap();
            }
            d
        };
        assert_eq!(geocheck_route(&kit("none", &[])), None);
        assert_eq!(geocheck_route(&kit("dump", &["geocheck_dump.rs"])), Some(GeoRoute::Dump));
        assert_eq!(geocheck_route(&kit("adapter", &["geocheck.rs"])), Some(GeoRoute::Adapter));
        assert_eq!(geocheck_route(&kit("both", &["geocheck.rs", "geocheck_dump.rs"])), Some(GeoRoute::Dump), "the 2D dump wins, as in apps/");
        // No hook: an ERROR naming both, before anything is built or run.
        let engine = workspace_root();
        let app_dir = out_of_tree_fixture(&engine, "match-3", &base.join("game"));
        let mut app = load_app_dir(&app_dir).unwrap();
        app.external.as_mut().unwrap().kit_dir = base.join("none");
        let e = geocheck_app_dir(&app, &[]).unwrap_err();
        assert!(e.starts_with("ERROR: no geocheck adapter for match-3") && e.contains("examples/geocheck_dump.rs") && e.contains("examples/geocheck.rs") && e.contains("fix:"), "{e}");
        // A kit with a hook but outside the game's workspace: an ERROR before cargo runs.
        app.external.as_mut().unwrap().kit_dir = base.join("adapter");
        let e = geocheck_app_dir(&app, &[]).unwrap_err();
        assert!(e.starts_with("ERROR: geocheck --app-dir") && e.contains("outside the game's workspace") && e.contains("fix:"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn snippet_the_page_never_imports_is_an_orphan() {
        let js = "import * as s from './snippets/evani-render-abc/js/stream.js';";
        let files = vec![
            "snippets/evani-render-abc/js/stream.js".to_string(),
            "snippets/evani-render-old/js/stream.js".to_string(),
        ];
        assert_eq!(
            orphan_snippets(js, &files),
            vec!["snippets/evani-render-old/js/stream.js".to_string()]
        );
        assert!(orphan_snippets("no imports", &[]).is_empty());
        let s = BuildSize {
            wasm_raw: 1,
            orphan_snippets: vec!["snippets/x.js".to_string()],
            ..BuildSize::default()
        };
        assert!(ship_guard("demo", &s).is_err());
    }

    const SANDBOX: &str = r#"
name = "sandbox"
kit = "example"
look = "basic"
effects = ["bloom", "vignette"]
[modules]
physics = "simple"
net = true
xr = false
live = true
"#;

    #[test]
    fn only_ship_builds_strip_builder_paths() {
        let m = Manifest::parse(SANDBOX).unwrap();
        assert!(!m.shipped, "a dev build is unchanged");
        let s = m.ship();
        assert!(s.shipped && s.base().shipped, "a ship build and its size variants strip paths");
        assert!(s.without_each().iter().all(|(_, v)| v.shipped));
    }

    #[test]
    fn ship_rustflags_remap_home_folders_wider_first() {
        let (f, skipped) = ship_rustflags(
            Some(Path::new(r"C:\Users\a")),
            Some(Path::new(r"C:\Users\a\.cargo")),
            Some(Path::new(r"C:\Users\a\.rustup")),
            Path::new(r"D:\w\evani\"),
        );
        assert_eq!(
            f,
            [
                r"--remap-path-prefix=C:\Users\a=/home",
                r"--remap-path-prefix=C:\Users\a\.cargo=/cargo",
                r"--remap-path-prefix=C:\Users\a\.rustup=/rustup",
                r"--remap-path-prefix=D:\w\evani=/evani",
            ]
        );
        assert!(skipped.is_empty());
        // Cargo splits the flags on whitespace: such a folder is reported, not passed.
        let (f, skipped) = ship_rustflags(Some(Path::new(r"C:\Users\Jo Bloggs")), None, None, Path::new("/w"));
        assert_eq!(f, ["--remap-path-prefix=/w=/evani"]);
        assert_eq!(skipped, [PathBuf::from(r"C:\Users\Jo Bloggs")]);
    }

    #[test]
    fn maps_manifest_to_features() {
        let m = Manifest::parse(SANDBOX).unwrap();
        assert_eq!(
            m.features(),
            ["fx", "bloom", "vignette", "physics-simple", "net", "live"]
        );
        assert_eq!(
            m.clone().ship().features(),
            ["fx", "bloom", "vignette", "physics-simple", "net"]
        );
        assert!(m.base().features().is_empty());
    }

    #[test]
    fn maps_the_material_modules() {
        let m = Manifest::parse("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n[modules]\nnature = [\"ocean\"]\nproc = [\"sky\"]\n").unwrap();
        assert_eq!(m.features(), ["nature", "nature-ocean", "proc", "proc-sky"]);
        assert_eq!(m.clone().ship().features(), m.features(), "shipped as listed");
        assert!(m.base().features().is_empty());
        let names: Vec<String> = m.without_each().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["nature.ocean", "nature", "proc.sky", "proc"]);
        let off = m.without_each();
        assert_eq!(off[0].1.features(), ["nature", "proc", "proc-sky"], "the part off, the crate still on");
        assert_eq!(off[1].1.features(), ["proc", "proc-sky"]);
        // Not listed: none of it; a bad list or an unknown key is refused.
        assert!(Manifest::parse("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n").unwrap().features().is_empty());
        assert!(Manifest::parse("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n[modules]\nnature = true\n").is_err());
        let e = Manifest::parse("name = \"g\"\nkit = \"k\"\nlook = \"l\"\n[modules]\nsea = [\"ocean\"]\n").unwrap_err();
        assert!(e.contains("unknown module `sea`") && e.contains("nature, proc"), "{e}");
    }

    #[test]
    fn maps_models_anim_ui() {
        let with = |mods: &str| format!("name = \"g\"\nkit = \"k\"\nlook = \"basic\"\n[modules]\n{mods}\n");
        // Nothing listed: nothing turned on.
        assert!(Manifest::parse(&with("")).unwrap().features().is_empty());
        let m = Manifest::parse(&with("models = true\nui = true")).unwrap();
        assert_eq!(m.features(), ["models", "ui"]);
        // `anim` implies `models`, in both orders of writing.
        let m = Manifest::parse(&with("anim = true")).unwrap();
        assert_eq!(m.features(), ["models", "anim"]);
        // Dropping `models` drops `anim` too; each variant is one switch.
        let all = Manifest::parse(&with("anim = true\nui = true")).unwrap();
        let names: Vec<String> = all.without_each().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["models", "anim", "ui"]);
        let (_, no_models) = &all.without_each()[0];
        assert_eq!(no_models.features(), ["ui"]);
        assert!(all.base().features().is_empty());
        // The ship build keeps them (only `live` is dev-only).
        assert_eq!(all.clone().ship().features(), ["models", "anim", "ui"]);
    }

    #[test]
    fn variants_drop_one_module_each() {
        let m = Manifest::parse(SANDBOX).unwrap();
        let names: Vec<String> = m.without_each().into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            [
                "fx.bloom",
                "fx.vignette",
                "fx (all)",
                "physics",
                "net",
                "live"
            ]
        );
        let (_, no_bloom) = &m.without_each()[0];
        assert_eq!(
            no_bloom.features(),
            ["fx", "vignette", "physics-simple", "net", "live"]
        );
    }

    #[test]
    fn rejects_bad_manifests() {
        assert!(Manifest::parse("name = 1").is_err());
        let bad_physics = SANDBOX.replace("\"simple\"", "\"soft\"");
        assert!(Manifest::parse(&bad_physics).is_err());
        let unknown = format!("{SANDBOX}audio = true\n");
        assert!(Manifest::parse(&unknown).is_err());
        let none = SANDBOX.replace("\"simple\"", "\"none\"");
        assert_eq!(Manifest::parse(&none).unwrap().physics, None);
    }

    #[test]
    fn checks_features_exist_in_crate() {
        let m = Manifest::parse(SANDBOX).unwrap();
        let cargo = r#"
[features]
fx = []
bloom = []
vignette = []
physics-simple = []
net = []
live = []
[dependencies]
evani-kit-example = "1"
evani-look-basic = "1"
"#;
        assert!(check_against_crate(&m, cargo).is_ok());
        let missing = cargo.replace("live = []\n", "");
        let e = check_against_crate(&m, &missing).unwrap_err();
        assert!(e.starts_with("ERROR: ") && e.contains("`live`") && e.contains("\n  fix: "), "{e}");
    }

    #[test]
    fn real_sandbox_manifest_matches_its_crate() {
        let app = load_app("sandbox").unwrap();
        assert!(app.manifest.features().contains(&"fx".to_string()));
    }

    #[test]
    fn scans_markers() {
        let known: Vec<String> = [
            "evani.module.fx",
            "evani.module.net",
            "evani.module.network",
            "evani.module.live",
            "evani.shader.bloom_up",
        ]
        .map(String::from)
        .to_vec();
        // Packed like a wasm data section: no separators between strings.
        let bytes = b"\0evani.module.fxfxevani.module.networknetworkevani.shader.bloom_up\x01";
        assert_eq!(
            scan_markers(bytes, &known),
            [
                "evani.module.fx",
                "evani.module.network",
                "evani.shader.bloom_up"
            ]
        );
    }

    #[test]
    fn finds_markers_declared_in_sources() {
        let mut out = Vec::new();
        markers_in_text(
            "fn id() -> &str { \"evani.module.fx\" }\n// evani.shader.bloom_up\n`evani.module.<x>`",
            &mut out,
        );
        assert_eq!(out, ["evani.module.fx", "evani.shader.bloom_up"]);
        let all = known_markers(&workspace_root());
        for m in [
            "evani.module.live",
            "evani.module.render",
            "evani.shader.vignette",
            "evani.text.font_5x9",
        ] {
            assert!(all.iter().any(|k| k == m), "{m} missing from {all:?}");
        }
    }

    #[test]
    fn gzip_shrinks_repetitive_data() {
        let data = vec![7u8; 10_000];
        let gz = gzip_len(&data);
        assert!(gz > 0 && gz < 200, "{gz}");
    }

    #[test]
    fn ship_guard_catches_live_code_swapping() {
        let wasm = b"\0asm..panicked at C:\\x\\subsecond-0.7.10\\src\\lib.rs..crates\\evani-live\\src\\hotpatch.rs..";
        assert_eq!(dev_only_in(wasm), ["subsecond", "hotpatch"]);
        assert!(dev_only_in(b"\0asm evani.module.render hot patch sub second").is_empty());
        let js = b"document.getElementById('evani-code-toast'); window.__evani_code = {};";
        assert_eq!(dev_only_in(js), ["evani-code-toast", "__evani_code"]);

        let clean = BuildSize {
            wasm_raw: 1,
            markers: vec!["evani.module.render".into()],
            ..BuildSize::default()
        };
        assert!(ship_guard("kart", &clean).is_ok());
        let hot = BuildSize {
            dev_only: vec!["subsecond".into()],
            ..clean.clone()
        };
        let err = ship_guard("kart", &hot).unwrap_err();
        assert!(err.contains("live code swapping") && err.contains("subsecond"), "{err}");
        let live = BuildSize {
            markers: vec!["evani.module.live".into()],
            ..clean
        };
        assert!(ship_guard("kart", &live).unwrap_err().contains("live module"));
    }

    #[test]
    fn ship_guard_refuses_the_importance_tool() {
        let wasm = b"\0asm..evani.module.render..evani-importance..";
        assert_eq!(dev_only_in(wasm), ["evani-importance"]);
        let bad = BuildSize {
            wasm_raw: 1,
            dev_only: dev_only_in(wasm),
            ..BuildSize::default()
        };
        let err = ship_guard("sandbox", &bad).unwrap_err();
        assert!(err.contains("ERROR") && err.contains("fix:") && err.contains("importance"), "{err}");
    }

    /// The measuring tool is reachable only through evani-render's
    /// `importance` feature, which `cargo xtask importance` adds on the
    /// command line: no app's shipped features, and no app's `default`
    /// features (what a plain trunk build turns on), may name it.
    #[test]
    fn importance_never_reaches_a_ship_build() {
        let root = workspace_root();
        for app in ["kart-racer", "sandbox", "match-3"] {
            let m = load_app(app).unwrap().manifest.ship();
            assert!(!m.features().iter().any(|f| f.contains("importance")), "{app} ship has importance");
        }
        for entry in std::fs::read_dir(root.join("apps")).unwrap().flatten() {
            let cargo = entry.path().join("Cargo.toml");
            if let Ok(text) = std::fs::read_to_string(&cargo) {
                let default = text.split("default = [").nth(1).and_then(|t| t.split(']').next()).unwrap_or("");
                assert!(!default.contains("importance"), "{} turns importance on by default", cargo.display());
            }
        }
    }

    #[test]
    fn ship_guard_refuses_root_absolute_paths() {
        let bad = "<link rel=\"modulepreload\" href=\"/evani-x.js\"><script type=\"module\">import init from '/evani-x.js'; init({ module_or_path: '/evani-x_bg.wasm' })</script><img src=\"/a.png\">";
        assert_eq!(absolute_paths_in(bad).len(), 4);
        let good = "<link href=\"./evani-x.js\"><script>import init from './evani-x.js'; init({ module_or_path: './evani-x_bg.wasm' })</script><a href=\"//cdn.example/x\">";
        assert!(absolute_paths_in(good).is_empty());
        let clean = BuildSize {
            wasm_raw: 1,
            ..BuildSize::default()
        };
        assert!(ship_guard("kart", &clean).is_ok());
        let abs = BuildSize {
            absolute_paths: absolute_paths_in(bad),
            ..clean
        };
        let err = ship_guard("kart", &abs).unwrap_err();
        assert!(err.contains("ERROR") && err.contains("fix:") && err.contains("/evani-x.js"), "{err}");
    }

    /// The hot-patch client is only reachable through the app's `live`
    /// feature (evani-live's default `hotpatch`), which a ship build drops.
    #[test]
    fn hotpatch_only_comes_with_live() {
        let root = workspace_root();
        let live = std::fs::read_to_string(root.join("crates/evani-live/Cargo.toml")).unwrap();
        assert!(live.contains("hotpatch = [\"evani-core/hotpatch\""), "{live}");
        for app in ["kart-racer", "sandbox"] {
            let cargo = std::fs::read_to_string(root.join("apps").join(app).join("Cargo.toml")).unwrap();
            assert!(!cargo.contains("hotpatch"), "{app} must get hotpatch only via live");
            let m = load_app(app).unwrap().manifest.ship();
            assert!(!m.features().iter().any(|f| f == "live"), "{app} ship has live");
        }
    }

    #[test]
    fn live_models_mode_args() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(live_args(&a("--game kart-racer")), ("sandbox".into(), "8096".into(), vec![]));
        let (game, port, extra) = live_args(&a("--models"));
        assert_eq!((game.as_str(), port.as_str()), ("apps/model-view", "8097"));
        assert_eq!(extra, a("--session .evani/live/model/session.json --models .evani/model"));
        let (_, _, extra) = live_args(&a("--models C:/w/.evani/model --port 8120"));
        assert_eq!(extra[3], "C:/w/.evani/model");
        let (_, _, extra) = live_args(&a("--models --port 8120"));
        assert_eq!(extra[3], ".evani/model");
    }

    #[test]
    fn dev_takes_hot_and_a_game() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(dev_args(&a("--hot kart-racer")).unwrap(), ("kart-racer".into(), "8096".into(), true));
        assert_eq!(dev_args(&a("sandbox --port 8160")).unwrap(), ("sandbox".into(), "8160".into(), false));
        assert!(dev_args(&a("--hot")).is_err());
        assert!(dev_args(&a("--hot kart --bogus")).is_err());
        // A free port when 8096 is taken, unless a port was asked for.
        assert_eq!(dev_extra(&a("--hot kart-racer")), a("--any-port"));
        assert!(dev_extra(&a("sandbox --port 8160")).is_empty());
    }

    #[test]
    fn dx_install_uses_the_bridges_pin() {
        let root = workspace_root();
        let code = std::fs::read_to_string(root.join("tools/evani-live-bridge/src/code.rs")).unwrap();
        assert!(code.contains(&format!("DX_REV: &str = \"{DX_REV}\"")));
        let args = dx_install_args(&root);
        assert!(args.windows(2).any(|w| w[0] == "--rev" && w[1] == DX_REV));
        assert!(args.last().unwrap().replace('\\', "/").ends_with("target/evani-tools/dx"));
    }
}
