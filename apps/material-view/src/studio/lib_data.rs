//! The Evani Library list (design part E2, docs/EVANI_COMMONS.md "The library
//! window"): the approved assets evani.app serves, as this window shows them.
//! Pure, so it is tested natively; `lib_page.rs` fetches the files.
//!
//! Evani.App downloads the library from evani.app, checks every file's hash
//! and the build's signature, keeps it in its own cache and serves this
//! window from there. Two files tell the window what the library holds:
//!
//! * `./evani-library.json`: the website manifest (`evani-library/1`, the
//!   schema of Evani.App's `platform/protocol/library.ts`) or the part of it
//!   the window reads (`format`, `stage`, `rev`, `published_at`, `assets`),
//!   plus an optional `cache` block Evani.App adds (`state`, `saved_at`,
//!   `checked_at`) when it serves a saved copy;
//! * `./library-contents.json` (`evani-library-contents/1`, written by
//!   `cargo xtask library build`): the versions compiled into this build.
//!
//! Each asset is checked by the protocol's own rules (`readAsset`); one that
//! fails is left out with a plain reason and the rest still show. A file
//! that fails as a whole (not JSON, another format) shows nothing new: the
//! window's last good copy if it has one, said plainly, else why not.

use std::collections::HashSet;

use serde_json::Value;

use super::proc_data::{Item, ProcLib};

/// The manifest's format.
pub const FORMAT: &str = "evani-library/1";
/// The build's contents list's format (E1).
pub const CONTENTS_FORMAT: &str = "evani-library-contents/1";
/// The files the window reads, next to its `index.html` (fetched by the
/// page, so only the wasm build uses the names).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const MANIFEST_FILE: &str = "evani-library.json";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const CONTENTS_FILE: &str = "library-contents.json";
/// Where the window asks for a reference picture by its SHA-256 (Evani.App
/// serves it from its cache; the window checks the hash before showing it).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const PICTURE_DIR: &str = "evani-library/blobs/";
/// The most assets a manifest lists (`LIBRARY_LIMITS.assets`).
pub const MAX_ASSETS: usize = 2000;
/// The biggest manifest the window reads (`LIBRARY_LIMITS.manifestBytes`).
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// The radar's categories, in the website's order (`SCORE_CATEGORIES`).
pub const SCORE_CATEGORIES: [(&str, &str); 5] = [("silhouette", "Silhouette"), ("structure", "Structure"), ("material", "Material"), ("colour", "Colour"), ("lighting", "Lighting")];

/// An asset's place in the library (`ASSET_STATES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The library build carries this version.
    InLibrary,
    /// Approved; the next library build carries it.
    Arriving,
    /// No published version: the base engine's own.
    Base,
    /// Approved, but the build couldn't take it (`why` says why).
    HeldBack,
}

impl State {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "in-library" => Self::InLibrary,
            "arriving" => Self::Arriving,
            "base" => Self::Base,
            "held-back" => Self::HeldBack,
            _ => return None,
        })
    }
}

/// A reference picture on the file host, by its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub sha256: String,
    pub kind: String,
}

/// One approved asset, as the manifest lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    /// `asset@style`.
    pub id: String,
    /// The library key (`rocks-pebbles`), the procedural item it opens.
    pub asset: String,
    pub style: String,
    pub name: String,
    pub category: String,
    pub area: String,
    /// The approved version's hash12; `None` only for `base`.
    pub version: Option<String>,
    pub server_score: Option<f64>,
    /// The five categories' scores (0-5), when the website has them.
    pub score_categories: Option<[f64; 5]>,
    pub author: Option<String>,
    pub approved_at: Option<String>,
    pub state: State,
    pub why: Option<String>,
    pub in_library_since: Option<String>,
    pub reference: Option<Picture>,
}

impl Asset {
    /// The version's name, `asset@style#hash12` (builds and jobs use it).
    pub fn version_ref(&self) -> Option<String> {
        self.version.as_ref().map(|v| format!("{}#{v}", self.id))
    }
}

/// An entry left out, and why, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub label: String,
    pub why: String,
}

/// What Evani.App says about the copy it serves (the `cache` block).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheNote {
    /// Evani.App's library state (`ready`, `offline`, `server-error`, ...).
    pub state: String,
    pub saved_at: Option<String>,
    pub checked_at: Option<String>,
}

impl CacheNote {
    /// The copy is a saved one Evani.App couldn't bring up to date.
    pub fn stale(&self) -> bool {
        !matches!(self.state.as_str(), "" | "ready" | "updating" | "arriving")
    }
}

/// A manifest, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub stage: String,
    pub rev: u64,
    pub published_at: String,
    pub assets: Vec<Asset>,
    pub skipped: Vec<Skipped>,
    pub cache: Option<CacheNote>,
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `[a-z0-9][a-z0-9-]{1,max}`
fn is_slug(s: &str, max: usize) -> bool {
    let b = s.as_bytes();
    (2..=max + 1).contains(&b.len()) && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// `asset@style` (the protocol's `ITEM_ID_RE`).
pub fn is_item_id(s: &str) -> bool {
    s.split_once('@').is_some_and(|(a, st)| is_slug(a, 60) && is_slug(st, 30))
}

/// `asset@style#hash12` (the protocol's `VERSION_REF_RE`).
pub fn is_version_ref(s: &str) -> bool {
    s.split_once('#').is_some_and(|(id, h)| is_item_id(id) && is_hex(h, 12))
}

/// `2026-11-10T19:02:11Z` (milliseconds allowed; the protocol's `ISO_RE`).
pub fn is_iso(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|x| x.iter().all(u8::is_ascii_digit));
    let head = b.len() >= 20 && digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10) && b[10] == b'T' && digits(11..13) && b[13] == b':' && digits(14..16) && b[16] == b':' && digits(17..19);
    head && match &b[19..] {
        [b'Z'] => true,
        [b'.', rest @ .., b'Z'] => (1..=3).contains(&rest.len()) && rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn text(v: &Value, k: &str, max: usize) -> Option<String> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && s.chars().count() <= max).map(str::to_string)
}

/// `null` or a short text; `Err` when it is something else.
fn opt_text(v: &Value, k: &str, max: usize) -> Result<Option<String>, ()> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => text(v, k, max).map(Some).ok_or(()),
    }
}

fn score(x: &Value) -> Option<f64> {
    x.as_f64().filter(|s| (0.0..=5.0).contains(s))
}

/// One asset by the protocol's rules (`readAsset`), or why it is left out.
pub fn read_asset(raw: &Value) -> Result<Asset, String> {
    if !raw.is_object() {
        return Err("an entry that isn't an asset".into());
    }
    let id = raw.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let asset = raw.get("asset").and_then(Value::as_str).unwrap_or_default().to_string();
    let style = raw.get("style").and_then(Value::as_str).unwrap_or_default().to_string();
    if !is_item_id(&id) || format!("{asset}@{style}") != id {
        return Err("its name isn't a library name (asset@style)".into());
    }
    let Some(state) = raw.get("state").and_then(Value::as_str).and_then(State::parse) else {
        return Err("its place in the library is unknown to this window".into());
    };
    let base = state == State::Base;
    let version = raw.get("version").and_then(Value::as_str).map(str::to_string);
    let version_ok = if base { raw.get("version") == Some(&Value::Null) } else { version.as_deref().is_some_and(|v| is_hex(v, 12)) };
    if !version_ok {
        return Err("its version name didn't pass its check".into());
    }
    let hash = raw.get("content_hash").and_then(Value::as_str);
    let hash_ok = if base { raw.get("content_hash") == Some(&Value::Null) } else { hash.and_then(|h| h.strip_prefix("sha256:")).is_some_and(|h| is_hex(h, 64)) };
    if !hash_ok {
        return Err("its content hash didn't pass its check".into());
    }
    let server_score = match raw.get("server_score") {
        None | Some(Value::Null) => None,
        Some(x) => Some(score(x).ok_or("its score isn't a score from 0 to 5")?),
    };
    if !base && server_score.is_none() {
        return Err("an approved version needs its score".into());
    }
    let score_categories = match raw.get("score_categories") {
        None | Some(Value::Null) => None,
        Some(c) => {
            let mut out = [0.0; 5];
            for (k, (cid, _)) in SCORE_CATEGORIES.iter().enumerate() {
                out[k] = c.get(cid).and_then(score).ok_or("its category scores didn't pass their check")?;
            }
            Some(out)
        }
    };
    let reference = match raw.get("reference") {
        None | Some(Value::Null) => None,
        Some(r) => {
            let sha = r.get("sha256").and_then(Value::as_str).filter(|s| is_hex(s, 64));
            let kind = r.get("type").and_then(Value::as_str).filter(|t| matches!(*t, "image/png" | "image/jpeg" | "image/webp"));
            match (sha, kind) {
                (Some(s), Some(t)) => Some(Picture { sha256: s.into(), kind: t.into() }),
                _ => return Err("its picture's hash didn't pass its check".into()),
            }
        }
    };
    let who = (opt_text(raw, "author", 80), opt_text(raw, "approved_by", 80));
    let (Ok(author), Ok(approved_by)) = who else { return Err("who made or approved it isn't readable".into()) };
    let approved_at = match raw.get("approved_at") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if is_iso(s) => Some(s.clone()),
        Some(_) => return Err("when it was approved isn't a time".into()),
    };
    if !base && (approved_at.is_none() || approved_by.is_none()) {
        return Err("an approved version needs who approved it and when".into());
    }
    let (Some(name), Some(category)) = (text(raw, "name", 120), text(raw, "category", 60)) else {
        return Err("its name or category is missing".into());
    };
    let area = raw.get("area").and_then(Value::as_str).unwrap_or_default().to_string();
    if area != "official" && area != "community" {
        return Err("its area is neither official nor community".into());
    }
    let why = text(raw, "why", 400);
    if state == State::HeldBack && why.is_none() {
        return Err("it is held back without a reason".into());
    }
    let in_library_since = raw.get("in_library_since").and_then(Value::as_str).filter(|s| state == State::InLibrary && is_iso(s)).map(str::to_string);
    Ok(Asset {
        id,
        asset,
        style,
        name,
        category,
        area,
        version: if base { None } else { version },
        server_score,
        score_categories,
        author,
        approved_at,
        state,
        why: if state == State::HeldBack { why } else { None },
        in_library_since,
        reference,
    })
}

/// A manifest's text, or why the whole file can't be used. Assets that fail
/// their own checks are left out (in `skipped`), never the whole list.
pub fn read_listing(text: &str) -> Result<Listing, String> {
    if text.len() > MAX_BYTES {
        return Err("the list is too big to be a library list".into());
    }
    let raw: Value = serde_json::from_str(text).map_err(|_| "the list isn't readable (not JSON)".to_string())?;
    if !raw.is_object() {
        return Err("the list isn't a library list".into());
    }
    match raw.get("format").and_then(Value::as_str) {
        Some(FORMAT) => {}
        Some(f) if f.starts_with("evani-library/") => return Err(format!("the list is in a newer format ({f}) than this window reads")),
        _ => return Err("the list isn't a library list (no evani-library/1 format)".into()),
    }
    // Evani.App's virtual copy may leave the stage out: then it is the live one.
    let stage = match raw.get("stage") {
        None | Some(Value::Null) => "live".to_string(),
        Some(s) => s.as_str().filter(|s| matches!(*s, "live" | "test")).ok_or("the list's stage is neither live nor test")?.to_string(),
    };
    let rev = raw.get("rev").and_then(Value::as_u64).filter(|r| *r >= 1).ok_or("the list's revision isn't a whole number from 1")?;
    let published_at = raw.get("published_at").and_then(Value::as_str).filter(|s| is_iso(s)).ok_or("the list's date isn't a time")?.to_string();
    let list = raw.get("assets").and_then(Value::as_array).filter(|a| a.len() <= MAX_ASSETS).ok_or("the list has no asset list")?;
    let mut assets: Vec<Asset> = Vec::new();
    let mut skipped = Vec::new();
    let mut ids = HashSet::new();
    for a in list {
        let label = a.get("name").and_then(Value::as_str).or_else(|| a.get("id").and_then(Value::as_str)).unwrap_or("An entry").chars().take(80).collect::<String>();
        match read_asset(a) {
            Ok(x) if !ids.insert(x.id.clone()) => skipped.push(Skipped { label, why: "it is listed twice".into() }),
            Ok(x) => assets.push(x),
            Err(why) => skipped.push(Skipped { label, why }),
        }
    }
    let cache = raw.get("cache").filter(|c| c.is_object()).map(|c| {
        let time = |k: &str| c.get(k).and_then(Value::as_str).filter(|s| is_iso(s)).map(str::to_string);
        CacheNote { state: c.get("state").and_then(Value::as_str).unwrap_or_default().chars().take(40).collect(), saved_at: time("saved_at"), checked_at: time("checked_at") }
    });
    Ok(Listing { stage, rev, published_at, assets, skipped, cache })
}

/// The build's contents list (E1): what versions it carries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Contents {
    pub build: String,
    pub built_at: Option<String>,
    /// Every compiled-in version ref: `included`, `already` and `in_base`.
    pub carried: HashSet<String>,
}

/// The contents list's text, or why it can't be used.
pub fn read_contents(text: &str) -> Result<Contents, String> {
    let raw: Value = serde_json::from_str(text).map_err(|_| "this build's contents list isn't readable".to_string())?;
    if raw.get("format").and_then(Value::as_str) != Some(CONTENTS_FORMAT) {
        return Err("this build's contents list is in a format this window doesn't read".into());
    }
    let build = raw.get("build").and_then(Value::as_str).filter(|b| is_hex(b, 12)).ok_or("this build's contents list has no build name")?.to_string();
    let mut carried = HashSet::new();
    for k in ["included", "already", "in_base"] {
        for r in raw.get(k).and_then(Value::as_array).into_iter().flatten() {
            if let Some(r) = r.as_str().filter(|r| is_version_ref(r)) {
                carried.insert(r.to_string());
            }
        }
    }
    Ok(Contents { build, built_at: raw.get("built_at").and_then(Value::as_str).filter(|s| is_iso(s)).map(str::to_string), carried })
}

/// What a fetch of one file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Text(String),
    /// Not there (404): no library list served yet.
    Missing,
    /// Couldn't be read (the reason).
    Failed(String),
}

/// The window's own last good copy (localStorage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub text: String,
    pub saved_at: String,
}

/// Where the list shown comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Fresh from Evani.App.
    Fresh,
    /// A saved copy Evani.App serves (it couldn't reach evani.app).
    AppSaved,
    /// The window's own saved copy: the latest didn't load (the reason).
    WindowSaved { why: String },
}

/// What the Library tab shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    /// No list yet and no saved copy: the reason, in plain words;
    /// `missing`: none was served at all (not a broken one).
    Nothing { why: String, missing: bool },
    List { listing: Listing, source: Source, saved_at: Option<String> },
}

/// What to show from a fetch and the window's saved copy, and whether to
/// save the fetched text as the new copy (only a list that reads).
pub fn resolve(got: &Fetched, saved: Option<&Saved>) -> (Shown, bool) {
    let why = match got {
        Fetched::Text(t) => match read_listing(t) {
            Ok(listing) => {
                let source = if listing.cache.as_ref().is_some_and(CacheNote::stale) { Source::AppSaved } else { Source::Fresh };
                let saved_at = listing.cache.as_ref().and_then(|c| c.saved_at.clone());
                return (Shown::List { listing, source, saved_at }, true);
            }
            Err(e) => e,
        },
        Fetched::Missing => "Evani.App hasn't served the library list yet".into(),
        Fetched::Failed(e) => e.clone(),
    };
    if let Some(s) = saved
        && let Ok(listing) = read_listing(&s.text)
    {
        return (Shown::List { listing, source: Source::WindowSaved { why }, saved_at: Some(s.saved_at.clone()) }, false);
    }
    (Shown::Nothing { why, missing: *got == Fetched::Missing }, false)
}

/// Whether this build carries an asset's approved version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// The contents list names this version.
    Yes,
    /// The contents list doesn't: the build has another version of it.
    Other,
    /// No contents list (a developer build): not known.
    Unknown,
}

/// The procedural item an asset opens: the ready item for its library key
/// (the generator's own card before an alias's).
pub fn item_for<'a>(lib: &'a ProcLib, asset: &str) -> Option<&'a Item> {
    lib.ready().find(|i| i.id == asset).or_else(|| lib.ready().find(|i| i.asset.as_deref() == Some(asset)))
}

/// One row of the Library tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub asset: Asset,
    /// The item it opens in this window, if the window has it.
    pub item: Option<String>,
    pub carried: Carried,
}

/// The rows: every listed asset with what it opens here, in the list's order.
pub fn rows(listing: &Listing, contents: Option<&Contents>, lib: &ProcLib) -> Vec<Row> {
    listing
        .assets
        .iter()
        .map(|a| {
            let carried = match (contents, a.version_ref()) {
                (None, _) => Carried::Unknown,
                // The base engine's own version is in every build.
                (Some(_), _) if a.state == State::Base => Carried::Yes,
                (Some(c), Some(r)) if c.carried.contains(&r) => Carried::Yes,
                (Some(_), _) => Carried::Other,
            };
            Row { asset: a.clone(), item: item_for(lib, &a.asset).map(|i| i.id.clone()), carried }
        })
        .collect()
}

/// How many rows have an approved version (every state but `base`).
pub fn approved_count(rows: &[Row]) -> usize {
    rows.iter().filter(|r| r.asset.state != State::Base).count()
}

/// `2026-11-09T10:42:00Z` as "9 Nov 2026, 10:42 UTC" (unreadable: as given).
pub fn when(iso: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if !is_iso(iso) {
        return iso.to_string();
    }
    let n = |r: std::ops::Range<usize>| iso.get(r).and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
    let month = MONTHS.get(n(5..7).saturating_sub(1)).copied().unwrap_or("?");
    format!("{} {month} {}, {} UTC", n(8..10), n(0..4), &iso[11..16])
}

/// A picture's check: the hash the window worked out against the manifest's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PictureCheck {
    /// It matched: show it.
    Good,
    /// Not served (or not readable): show the placeholder, say nothing.
    Absent,
    /// It didn't match its check: never shown, and the row says so.
    Mismatch,
}

pub fn check_picture(expected: &str, got: Option<&str>) -> PictureCheck {
    match got {
        None => PictureCheck::Absent,
        Some(h) if h.eq_ignore_ascii_case(expected) => PictureCheck::Good,
        Some(_) => PictureCheck::Mismatch,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const VALID: &str = include_str!("fixtures/library/valid.json");
    const EMPTY: &str = include_str!("fixtures/library/empty.json");
    const BAD_HASH: &str = include_str!("fixtures/library/bad-hash.json");
    const UNKNOWN_VERSION: &str = include_str!("fixtures/library/unknown-version.json");
    const OFFLINE: &str = include_str!("fixtures/library/offline.json");
    const CONTENTS: &str = include_str!("fixtures/library/library-contents.json");

    /// The window's items, made once for every test (the procedural
    /// library generates each asset to frame it: slow in a debug build).
    pub(crate) fn lib() -> &'static ProcLib {
        static LIB: std::sync::OnceLock<ProcLib> = std::sync::OnceLock::new();
        LIB.get_or_init(|| {
            #[allow(unused_mut)]
            let mut l = ProcLib::load().unwrap();
            #[cfg(feature = "procedural")]
            l.add_library(&evani_procedural_lib::registry());
            l
        })
    }

    /// A list in the website's exact schema reads whole: every asset with
    /// its name, style, version name, picture hash and scores.
    #[test]
    fn a_valid_list_reads_every_asset() {
        let l = read_listing(VALID).unwrap();
        assert_eq!((l.stage.as_str(), l.rev, l.assets.len(), l.skipped.len()), ("live", 8, 4, 0));
        let p = &l.assets[0];
        assert_eq!((p.id.as_str(), p.name.as_str(), p.style.as_str()), ("rocks-pebbles@photoreal", "Pebbles", "photoreal"));
        assert_eq!(p.version_ref().as_deref(), Some("rocks-pebbles@photoreal#3f9a0c1e2b4d"));
        assert_eq!(p.server_score, Some(4.0));
        assert_eq!(p.score_categories, Some([4.0, 3.5, 4.0, 4.5, 3.5]));
        assert_eq!(p.reference.as_ref().map(|r| r.kind.as_str()), Some("image/jpeg"));
        assert_eq!(p.in_library_since.as_deref(), Some("2026-11-09T10:42:00Z"));
        assert_eq!(l.assets[1].state, State::Arriving);
        assert_eq!(l.assets[1].score_categories, None, "null until the radar work is resumed");
        assert_eq!((l.assets[2].state, l.assets[2].version.as_deref()), (State::Base, None));
        assert_eq!(l.assets[3].why.as_deref(), Some("its change clashes with Rocks: cobble; the builder will try again"));
        assert_eq!(l.cache, None);
        let (shown, save) = resolve(&Fetched::Text(VALID.into()), None);
        assert!(save);
        assert!(matches!(shown, Shown::List { source: Source::Fresh, .. }));
    }

    #[test]
    fn an_empty_list_is_a_list() {
        let l = read_listing(EMPTY).unwrap();
        assert!(l.assets.is_empty() && l.skipped.is_empty());
        assert_eq!(l.rev, 1);
        // Evani.App's served copy: { format, rev, published_at, assets }, no stage.
        let served = r#"{"format":"evani-library/1","rev":3,"published_at":"2026-11-10T09:00:00Z","assets":[]}"#;
        assert_eq!(read_listing(served).map(|l| l.stage), Ok("live".to_string()));
        assert!(read_listing(&served.replace("\"rev\"", "\"stage\":\"prod\",\"rev\"")).is_err());
    }

    /// An asset whose hashes fail their checks is left out with a plain
    /// reason; the rest still show; nothing panics.
    #[test]
    fn a_bad_hash_leaves_that_asset_out_with_a_reason() {
        let l = read_listing(BAD_HASH).unwrap();
        assert_eq!(l.assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["rocks-pebbles@photoreal"]);
        let why: Vec<(&str, &str)> = l.skipped.iter().map(|s| (s.label.as_str(), s.why.as_str())).collect();
        assert_eq!(
            why,
            [
                ("Short grass", "its content hash didn't pass its check"),
                ("Rolling hills", "its picture's hash didn't pass its check"),
                ("Medium rocks", "its version name didn't pass its check"),
                ("Pebbles", "it is listed twice"),
            ]
        );
    }

    /// A list in a format this window doesn't know is refused as a whole
    /// (with why); the window's saved copy shows instead, said plainly.
    #[test]
    fn an_unknown_version_falls_back_to_the_saved_copy() {
        let got = Fetched::Text(UNKNOWN_VERSION.into());
        let (shown, save) = resolve(&got, None);
        assert!(!save);
        assert_eq!(shown, Shown::Nothing { why: "the list is in a newer format (evani-library/2) than this window reads".into(), missing: false });
        let saved = Saved { text: VALID.into(), saved_at: "2026-11-09T12:00:00Z".into() };
        let (shown, save) = resolve(&got, Some(&saved));
        assert!(!save, "a refused list never replaces the saved copy");
        let Shown::List { listing, source: Source::WindowSaved { why }, saved_at } = shown else { panic!("{shown:?}") };
        assert_eq!((listing.rev, saved_at.as_deref()), (8, Some("2026-11-09T12:00:00Z")));
        assert!(why.contains("newer format"), "{why}");
        // A version this build doesn't carry is marked on its row.
        let c = read_contents(CONTENTS).unwrap();
        let r = rows(&listing, Some(&c), lib());
        assert_eq!(r[0].carried, Carried::Yes);
        assert_eq!(r[1].carried, Carried::Other, "arriving: the build carries another version");
        assert_eq!(r[2].carried, Carried::Yes, "the base engine's own version is in every build");
        assert_eq!(rows(&listing, None, lib())[0].carried, Carried::Unknown);
    }

    /// No list served yet: a plain message; offline with a saved copy: the
    /// copy, marked as saved; Evani.App's own saved copy says so too.
    #[test]
    fn no_list_offline_and_cached() {
        assert_eq!(resolve(&Fetched::Missing, None).0, Shown::Nothing { why: "Evani.App hasn't served the library list yet".into(), missing: true });
        let saved = Saved { text: VALID.into(), saved_at: "2026-11-09T12:00:00Z".into() };
        let (shown, _) = resolve(&Fetched::Failed("the window couldn't reach Evani.App".into()), Some(&saved));
        assert!(matches!(shown, Shown::List { source: Source::WindowSaved { .. }, .. }));
        let (shown, save) = resolve(&Fetched::Text(OFFLINE.into()), None);
        assert!(save);
        let Shown::List { listing, source, saved_at } = shown else { panic!() };
        assert_eq!((source, saved_at.as_deref()), (Source::AppSaved, Some("2026-11-09T09:15:00Z")));
        assert!(listing.cache.is_some_and(|c| c.stale()));
        // Not JSON at all, and no copy: why, never a panic.
        assert!(matches!(resolve(&Fetched::Text("<html>".into()), None).0, Shown::Nothing { .. }));
        assert!(read_listing("[]").is_err());
        assert!(read_listing(r#"{"format":"evani-library/1","stage":"live","rev":0,"published_at":"x","assets":[]}"#).is_err());
    }

    /// The listed assets open the procedural items of their library key.
    #[test]
    fn rows_open_their_procedural_item() {
        let l = read_listing(VALID).unwrap();
        let r = rows(&l, None, lib());
        let item = |i: usize| r[i].item.as_deref();
        #[cfg(feature = "procedural")]
        {
            assert_eq!(item(0), Some("rocks-pebbles"));
            assert_eq!(item(2), Some("terrain-rolling-hills"));
        }
        let none = read_listing(&VALID.replace("rocks-pebbles", "rocks-unicorn")).unwrap();
        assert_eq!(rows(&none, None, lib())[0].item, None, "an asset this window lacks opens nothing");
        let _ = item;
    }

    #[test]
    fn names_times_and_pictures() {
        for ok in ["a1@photoreal", "rocks-pebbles@photoreal"] {
            assert!(is_item_id(ok), "{ok}");
        }
        for bad in ["rocks@", "@x", "Rocks@photoreal", "rocks-pebbles", "a@b@c", "-x@photoreal"] {
            assert!(!is_item_id(bad), "{bad}");
        }
        assert!(is_version_ref("rocks-pebbles@photoreal#3f9a0c1e2b4d"));
        assert!(!is_version_ref("rocks-pebbles@photoreal#3F9A0C1E2B4D"));
        assert!(is_iso("2026-11-10T19:02:11Z") && is_iso("2026-11-10T19:02:11.123Z"));
        assert!(!is_iso("2026-11-10 19:02:11Z") && !is_iso("2026-11-10T19:02:11.1234Z") && !is_iso("2026-11-10T19:02:11"));
        assert_eq!(when("2026-11-09T10:42:00Z"), "9 Nov 2026, 10:42 UTC");
        let h = "ab".repeat(32);
        assert_eq!(check_picture(&h, Some(&h.to_uppercase())), PictureCheck::Good);
        assert_eq!(check_picture(&h, Some(&"cd".repeat(32))), PictureCheck::Mismatch);
        assert_eq!(check_picture(&h, None), PictureCheck::Absent);
        let c = read_contents(CONTENTS).unwrap();
        assert_eq!(c.build, "a41c700dcf58");
        assert_eq!(c.built_at.as_deref(), Some("2026-11-09T10:30:00Z"));
        assert!(c.carried.contains("rocks-pebbles@photoreal#3f9a0c1e2b4d") && c.carried.contains("rocks-medium-rocks@photoreal#0123456789ab"));
        assert_eq!(c.carried.len(), 2);
        assert!(read_contents("{\"format\":\"other\"}").is_err());
    }
}
