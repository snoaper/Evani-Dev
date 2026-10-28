//! The catalog side of `new-game --genre/--effects` and `catalog --json`
//! (STAGES.md Stage 6, "New Game in App Studio").
//!
//! - [`Catalog`]: `catalog/genres.json`, `looks.json`, `effects.json` and the
//!   features of `crates/evani-fx`, read as they are; nothing here contradicts
//!   them.
//! - [`resolve`]: a `--genre` / `--look` / `--effects` request checked against
//!   the catalog (the look must be in the genre's `compatible_looks`; physics
//!   comes from the genre; every effect must exist in `effects.json`, work
//!   with the look's style class, and be an `evani-fx` feature, the same rules
//!   `cargo xtask check` applies later).
//! - [`reference_prompt`] and [`spec_md`]: the pinned reference image prompt
//!   (the KIT_PROCESS template, filled from the catalog) and the kit's
//!   `SPEC.md` that carries it.
//! - [`Catalog::ranking_json`]: the genres ranked by most played.
//!
//! Most played: the catalog's own `reach_index` (its `about` calls rank and
//! reach_index "estimates of how much each genre is played (2025 data)"). No
//! live play counts exist anywhere in the repo (ROADMAP.md has none), so the
//! ranking is that estimate: highest `reach_index` first, ties by the
//! catalog's `rank`, then by the order in the file.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};

/// The effects the sandbox template starts with; a `--genre` game without
/// `--effects` keeps those that fit its look.
pub const TEMPLATE_EFFECTS: &[&str] = &["bloom", "vignette", "shadows"];

/// The neutral look that is not a catalog look (see `check::NEUTRAL_LOOKS`).
const NEUTRAL: &[(&str, &str)] = crate::check::NEUTRAL_LOOKS;

#[derive(Debug)]
pub struct Catalog {
    genres: Vec<Value>,
    looks: Vec<Value>,
    effects: Vec<Value>,
    /// Features of `crates/evani-fx`; `None` when the crate can't be read.
    fx_features: Option<BTreeSet<String>>,
}

fn list(root: &Path, file: &str, key: &str) -> Result<Vec<Value>, String> {
    let path = root.join("catalog").join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("catalog/{file}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("catalog/{file}: {e}"))?;
    v.get(key).and_then(Value::as_array).cloned().ok_or_else(|| format!("catalog/{file}: no `{key}` list"))
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn strs(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

fn by_id<'a>(items: &'a [Value], id: &str) -> Option<&'a Value> {
    items.iter().find(|x| s(x, "id") == id)
}

impl Catalog {
    pub fn load(root: &Path) -> Result<Self, String> {
        let fx_features = std::fs::read_to_string(root.join("crates/evani-fx/Cargo.toml"))
            .ok()
            .and_then(|t| t.parse::<toml::Table>().ok())
            .and_then(|t| t.get("features").and_then(toml::Value::as_table).map(|f| f.keys().cloned().collect()));
        Ok(Self { genres: list(root, "genres.json", "genres")?, looks: list(root, "looks.json", "looks")?, effects: list(root, "effects.json", "effects")?, fx_features })
    }

    /// The genres, most played first (see the module docs).
    pub fn ranked(&self) -> Vec<&Value> {
        let mut g: Vec<(usize, &Value)> = self.genres.iter().enumerate().collect();
        let reach = |v: &Value| v.get("reach_index").and_then(Value::as_f64).unwrap_or(0.0);
        let rank = |v: &Value| v.get("rank").and_then(Value::as_i64).unwrap_or(i64::MAX);
        g.sort_by(|(ia, a), (ib, b)| reach(b).total_cmp(&reach(a)).then(rank(a).cmp(&rank(b))).then(ia.cmp(ib)));
        g.into_iter().map(|(_, v)| v).collect()
    }

    /// `cargo xtask catalog --json`: the genres ranked by most played, each
    /// with what `new-game --genre` takes from it.
    pub fn ranking_json(&self) -> Value {
        let genres: Vec<Value> = self
            .ranked()
            .iter()
            .enumerate()
            .map(|(i, g)| {
                json!({
                    "position": i + 1,
                    "id": s(g, "id"),
                    "name": s(g, "name"),
                    "family": s(g, "family"),
                    "catalog_rank": g.get("rank"),
                    "reach_index": g.get("reach_index"),
                    "physics": s(g, "physics"),
                    "cameras": strs(g, "cameras"),
                    "multiplayer_by_nature": g.get("multiplayer_by_nature"),
                    "systems": strs(g, "systems"),
                    "image_scene": s(g, "image_scene"),
                    "compatible_looks": strs(g, "compatible_looks"),
                })
            })
            .collect();
        json!({
            "ranked_by": "reach_index, highest first (ties by catalog rank, then catalog order)",
            "source": "catalog/genres.json: rank and reach_index are the catalog's own estimates of how much each genre is played (2025 data); there are no live play counts in the repo",
            "count": genres.len(),
            "genres": genres,
        })
    }

    /// The style class (`2d`, `pseudo-3d`, `3d`) of a catalog or neutral look.
    fn style(&self, look: &str) -> Option<String> {
        NEUTRAL.iter().find(|(l, _)| *l == look).map(|(_, st)| (*st).to_string()).or_else(|| by_id(&self.looks, look).map(|l| s(l, "style_class").to_string()))
    }

    /// Effects that exist and fit `style`, for error messages.
    fn fitting(&self, style: &str) -> Vec<String> {
        self.effects.iter().filter(|e| strs(e, "works_with").iter().any(|w| w == style)).map(|e| s(e, "id").to_string()).collect()
    }
}

/// What the user asked for.
#[derive(Debug, Default, Clone)]
pub struct Request {
    pub genre: Option<String>,
    pub look: Option<String>,
    /// `Some(vec![])` is `--effects none`.
    pub effects: Option<Vec<String>>,
}

/// A request that passed the catalog.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub look: String,
    /// `None`: leave the template's physics.
    pub physics: Option<String>,
    /// `None`: leave the template's effects.
    pub effects: Option<Vec<String>>,
    /// The kit's SPEC.md (only with a genre).
    pub spec: Option<String>,
    /// Things the user should know (an effect dropped, a look picked).
    pub notes: Vec<String>,
}

/// Parse `--effects a,b,c` (`none` or an empty value: no effects).
pub fn parse_effects(arg: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in arg.split(',').map(str::trim).filter(|e| !e.is_empty() && *e != "none") {
        if !out.iter().any(|o| o == e) {
            out.push(e.to_string());
        }
    }
    out
}

fn near<'a>(ids: impl Iterator<Item = &'a str>, wanted: &str) -> Vec<String> {
    let w = wanted.to_lowercase();
    ids.filter(|i| i.contains(&w) || w.contains(i)).take(8).map(str::to_string).collect()
}

/// Check `req` against the catalog and the engine's looks and effects.
pub fn resolve(root: &Path, req: &Request) -> Result<Resolved, String> {
    let cat = Catalog::load(root)?;
    let mut notes = Vec::new();
    let genre = match &req.genre {
        None => None,
        Some(id) => Some(by_id(&cat.genres, id).ok_or_else(|| {
            let close = near(cat.genres.iter().map(|g| s(g, "id")), id);
            let hint = if close.is_empty() { String::new() } else { format!(" Close: {}.", close.join(", ")) };
            format!("`{id}` is not a genre in catalog/genres.json.{hint} `cargo xtask catalog --json` lists all {} genres, most played first.", cat.genres.len())
        })?),
    };

    // The look.
    let look = match (&req.look, genre) {
        (Some(l), _) => l.clone(),
        (None, None) => "basic".to_string(),
        (None, Some(g)) => {
            // The template default (`basic`) is not a catalog look, so with a genre
            // the first compatible look that has a crate is the default.
            let compat = strs(g, "compatible_looks");
            let have: Vec<&String> = compat.iter().filter(|l| root.join("looks").join(format!("evani-look-{l}")).is_dir()).collect();
            let pick = have.first().ok_or_else(|| {
                let mut built: Vec<String> = std::fs::read_dir(root.join("looks"))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|e| e.file_name().to_string_lossy().strip_prefix("evani-look-").map(str::to_string))
                    .collect();
                built.sort();
                format!(
                    "no look crate exists yet for genre `{}`: its compatible_looks ({}) have no looks/evani-look-<look>. Looks built here: {}. Build one first (docs/KIT_PROCESS.md step 5).",
                    s(g, "id"),
                    compat.join(", "),
                    built.join(", ")
                )
            })?;
            notes.push(format!("no --look given: using `{pick}`, the first of the genre's compatible_looks that has a crate"));
            (*pick).clone()
        }
    };
    if let Some(g) = genre {
        let compat = strs(g, "compatible_looks");
        if !compat.contains(&look) {
            return Err(format!("look `{look}` is not one of genre `{}`'s compatible_looks (catalog/genres.json): {}", s(g, "id"), compat.join(", ")));
        }
    }
    let style = cat.style(&look);
    if genre.is_some() && style.is_none() {
        return Err(format!("look `{look}` is not in catalog/looks.json"));
    }

    // The effects.
    let effects = match (&req.effects, genre) {
        (Some(e), _) => Some(e.clone()),
        (None, Some(_)) => {
            let keep: Vec<String> = TEMPLATE_EFFECTS.iter().filter(|e| fits(&cat, e, style.as_deref()).is_ok()).map(|e| (*e).to_string()).collect();
            let dropped: Vec<&str> = TEMPLATE_EFFECTS.iter().copied().filter(|e| !keep.iter().any(|k| k.as_str() == *e)).collect();
            if !dropped.is_empty() {
                notes.push(format!("no --effects given: the template's {} don't fit the `{look}` look (or evani-fx), so they are left out", dropped.join(", ")));
            }
            Some(keep)
        }
        (None, None) => None,
    };
    if let Some(list) = &req.effects {
        for e in list {
            fits(&cat, e, style.as_deref())?;
        }
    }

    let physics = genre.map(|g| s(g, "physics")).filter(|p| !p.is_empty()).map(str::to_string);
    let spec = match genre {
        Some(g) => {
            let l = by_id(&cat.looks, &look).ok_or_else(|| format!("look `{look}` is not in catalog/looks.json"))?;
            Some(spec_md(g, l, &cat, effects.as_deref().unwrap_or(&[]), physics.as_deref(), req))
        }
        None => None,
    };
    Ok(Resolved { look, physics, effects, spec, notes })
}

/// One effect against the catalog, the look's style and `evani-fx`.
fn fits(cat: &Catalog, effect: &str, style: Option<&str>) -> Result<(), String> {
    let e = by_id(&cat.effects, effect).ok_or_else(|| {
        let close = near(cat.effects.iter().map(|e| s(e, "id")), effect);
        let hint = if close.is_empty() { String::new() } else { format!(" Close: {}.", close.join(", ")) };
        format!("effect `{effect}` is not in catalog/effects.json.{hint}")
    })?;
    let works = strs(e, "works_with");
    if let Some(st) = style
        && !works.is_empty()
        && !works.iter().any(|w| w == st)
    {
        return Err(format!("effect `{effect}` works with {} looks, but the look is a {st} look. Effects that fit it: {}", works.join("/"), cat.fitting(st).join(", ")));
    }
    if let Some(fx) = &cat.fx_features
        && !fx.contains(effect)
    {
        return Err(format!(
            "effect `{effect}` is in the catalog but crates/evani-fx has no `{effect}` feature yet. Implemented effects: {}",
            fx.iter().filter(|f| cat.effects.iter().any(|e| s(e, "id") == f.as_str())).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

/// The camera: the first genre camera the look also supports, else the
/// genre's first (the rule `cargo xtask review` uses).
fn camera(genre: &Value, look: &Value) -> String {
    let (gc, lc) = (strs(genre, "cameras"), strs(look, "cameras"));
    gc.iter().find(|c| lc.contains(c)).or(gc.first()).cloned().unwrap_or_else(|| "gameplay".into())
}

/// The reference image prompt: the docs/KIT_PROCESS.md template filled from
/// the catalog. `cargo xtask review` builds the same text
/// (`reference_from_catalog`), so the card and the SPEC agree.
pub fn reference_prompt(genre: &Value, look: &Value) -> String {
    let quality = if s(look, "type").eq_ignore_ascii_case("2D") {
        "Crisp, polished 2D at native resolution: clean edges, readable silhouettes, rich colour and lighting where the style allows."
    } else {
        "Top-quality real-time 3D at the engine's Ultra tier: rich global illumination, soft shadows, clean anti-aliasing, film-quality colour grading."
    };
    format!(
        "In-game screenshot from an original {} video game, {} view.\n{}\nArt style: {}. {}\n{quality}\n16:9 widescreen game capture, 2560×1440. Everything is original: characters, vehicles, creatures, logos and UI. No existing game franchises, characters, brands or real people, and no watermark. Any on-screen text is short and legible.",
        s(genre, "name"),
        camera(genre, look),
        s(genre, "image_scene"),
        s(look, "name"),
        s(look, "look"),
    )
}

/// The kit's `SPEC.md` (docs/KIT_PROCESS.md step 3), started from the
/// catalog. The reference image prompt sits under a "Reference image prompt"
/// heading, where `cargo xtask review` looks for it; keep that heading last
/// and the only one that names it.
fn spec_md(genre: &Value, look: &Value, cat: &Catalog, effects: &[String], physics: Option<&str>, req: &Request) -> String {
    use std::fmt::Write as _;
    let id = s(genre, "id");
    let mut o = String::new();
    let _ = writeln!(o, "# {} ({id}): SPEC\n", s(genre, "name"));
    let _ = writeln!(
        o,
        "Started by `cargo xtask new-game` with `--genre {id}` from catalog/genres.json (catalog rank {}, reach index {}). The catalog is the source: where this file disagrees with it, the catalog wins. Turn each section into concrete mechanics and data (docs/KIT_PROCESS.md step 3); the target pack goes in `target/` next to this file.\n",
        genre.get("rank").map_or("?".to_string(), Value::to_string),
        genre.get("reach_index").map_or("?".to_string(), Value::to_string),
    );
    let _ = writeln!(o, "## Catalog choices\n");
    let _ = writeln!(o, "- Genre: {} ({id}), family {}.", s(genre, "name"), s(genre, "family"));
    let _ = writeln!(o, "- Look: {} (`{}`), {} look. {}", s(look, "name"), s(look, "id"), s(look, "style_class"), s(look, "look"));
    let _ = writeln!(o, "- Camera: {} (genre cameras: {}; this look supports: {}).", camera(genre, look), strs(genre, "cameras").join(", "), strs(look, "cameras").join(", "));
    let _ = writeln!(o, "- Physics: `{}`, exactly the catalog's (`[modules] physics` in apps/{id}/evani.toml).", physics.unwrap_or("none"));
    let _ = writeln!(
        o,
        "- Multiplayer by nature: {}. Ship offline-first, with bots and local play.",
        if genre.get("multiplayer_by_nature").and_then(Value::as_bool).unwrap_or(false) { "yes" } else { "no" },
    );
    if s(look, "type").eq_ignore_ascii_case("2D") {
        let _ = writeln!(o, "- Note: the game kit is copied from the 3D example (`evani-render` with `forward`). This look is 2D, so drop that feature in the kit's Cargo.toml (README \"Render\") before building.");
    }
    let _ = writeln!(o, "\n## Effects\n");
    if effects.is_empty() {
        let _ = writeln!(o, "None to start with (`effects = []`). Add one at a time in a live session and keep it only if the capture is clearly better (KIT_PROCESS \"Choosing effects\").");
    } else {
        for e in effects {
            let info = by_id(&cat.effects, e);
            let _ = writeln!(
                o,
                "- `{e}` ({}, cost {}): why it earns its place: TODO.",
                info.map_or("", |i| s(i, "group")),
                info.and_then(|i| i.get("cost_points")).map_or("?".to_string(), Value::to_string),
            );
        }
        if req.effects.is_none() {
            let _ = writeln!(o, "\nThese are the template's defaults that fit the look; replace them with the effects this game needs.");
        }
    }
    let _ = writeln!(o, "\n## Gameplay systems (from the catalog)\n");
    let systems = strs(genre, "systems");
    for (i, sys) in systems.iter().enumerate() {
        let _ = writeln!(o, "{}. {sys}: mechanics and data (what is tunable, what lives in JSON): TODO.", i + 1);
    }
    let _ = writeln!(o, "\n## The playtest bot\n\nHow it plays to a win and how to a lose: TODO.\n");
    let _ = writeln!(o, "## Reference games (mechanics only)\n");
    for g in strs(genre, "reference_games_mechanics_only") {
        let _ = writeln!(o, "- {g}");
    }
    let _ = writeln!(o, "\nAll content is original; these describe mechanics only.\n");
    let _ = writeln!(o, "## Reference image prompt\n");
    let _ = writeln!(o, "Pinned, built from the catalog's scene line for this genre and look. Copy it into `target/reference-prompt.md` (KIT_PROCESS step 2); `cargo xtask review` puts it on the review card.\n");
    let _ = writeln!(o, "```\n{}\n```", reference_prompt(genre, look));
    o
}

/// Replace the first line that starts with `prefix` (after leading
/// whitespace) by `new`, keeping the file's line ending.
pub fn replace_line(text: &str, prefix: &str, new: &str) -> Option<String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let i = lines.iter().position(|l| l.trim_start().starts_with(prefix))?;
    lines[i] = new.to_string();
    let mut out = lines.join(nl);
    if text.ends_with('\n') {
        out.push_str(nl);
    }
    Some(out)
}
