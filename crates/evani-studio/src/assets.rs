//! The asset catalogue, `catalog/assets/assets.json`: WHAT the studio draws.
//!
//! One row per asset, one source model per asset (`source`), and every art
//! style is generated from that one source. An asset's surface is cut into
//! named regions (coat, leg, mane, hoof...), each a set of source vertices
//! (per-vertex region index, [`crate::geo::SourceMesh::regions`]); the
//! selected material lands on one region ([`crate::compat::place`]) and the
//! rest follow it, take their own default material, or keep the source
//! model's colour.
//!
//! Classes (`quadruped.horse`, `building.wall`...) are what materials point
//! at with `belongs_on`; a class says whether its materials are exclusive to
//! it (a horse coat is foreign on a cow) and whether it is a swatch (test
//! shapes show any material).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::MAX_SLOTS;

/// The file format's version.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetCatalog {
    pub version: u32,
    pub note: String,
    /// The asset the studio opens on: a real subject, never a swatch shape
    /// (the spec: no demonstration sphere as the preview object).
    pub default_asset: String,
    pub categories: Vec<Category>,
    pub classes: Vec<ClassDef>,
    /// The kinds a part can be (`roof_tiles`, `masonry`, `coat`, `mane`...):
    /// what lets a per-part material carry from one asset to another (a
    /// horse's tail colour onto the cow's tail) and the page's part icons.
    pub part_kinds: Vec<PartKind>,
    pub assets: Vec<AssetRow>,
}

/// A kind of part (`assets.json` `part_kinds`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartKind {
    /// A plain id (`roof_tiles`).
    pub id: String,
    /// What the page calls it ("Roof tiles").
    pub name: String,
}

/// A named part of an asset that takes its own material (the roof's tiles,
/// its ridge cap, its gable ends): a group of the asset's regions.
///
/// - `regions`: the regions it covers, its **lead** first. A material picked
///   for the part lands on the lead and on every listed region that doesn't
///   follow another region of the same part; listed regions that follow one
///   in the part keep following it (a horse's legs keep the coat's points
///   tone). A part's material breaks a follow that crosses parts (a mane
///   given its own hair stops following the coat).
/// - Its default material is its lead region's (the region's own
///   `default_material`, the region it follows, or the asset's).
/// - A part of kept regions (eyes) keeps the model's own colour unless one of
///   its regions accepts the picked material by a specific token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    /// The page's name for it ("Gable ends").
    pub label: String,
    /// One of `part_kinds`.
    pub kind: String,
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub name: String,
}

/// An asset class: the name materials' `belongs_on` uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassDef {
    /// `family.kind`, for example `quadruped.horse`.
    pub id: String,
    /// One word for the page ("horse").
    pub name: String,
    /// With its article, for "Show on a horse".
    pub label: String,
    /// For "Made for horses".
    pub plural: String,
    /// Materials made for this class are foreign on any other class (a horse
    /// coat on a cow). Not exclusive: they only choose where to preview (a
    /// brick wall material previews on a wall, and still goes on a horse).
    pub exclusive: bool,
    /// Shows any material, foreign ones too (test shapes).
    pub swatch: bool,
    /// The asset a material of this class previews on.
    pub primary_asset: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetStatus {
    /// Listed, not started.
    Planned,
    /// Its source model is being made: the studio shows "being built" for it
    /// (never another asset in its place).
    Building,
    /// Its source model is in place; every style must exist for it.
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Built in Rust by the app (`shape`: wall, gable roof, slope, sphere...).
    Code,
    /// A versioned Blender script (`script`, optional `species`) converted
    /// with `cargo xtask convert` into the app's model `model`.
    BlenderScript,
    /// A modelled file converted with `cargo xtask convert` (`model`).
    Model,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub species: Option<String>,
    /// The model id the app draws (`models.handle("<model>")`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The code-built shape's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
}

/// Which of a material's colours a following region takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// The material as it is (its base colours and pattern).
    #[default]
    Base,
    /// The material's "points" colour (a bay's black legs, mane and tail:
    /// colour C of a fur row, by its `points` amount).
    Points,
}

/// A named part of the asset's surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: String,
    /// Takes the material of this region (one level: a leader never follows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,
    #[serde(default)]
    pub tone: Tone,
    /// A thin part (leg, ear, tail, mane): the geometry stage must keep it
    /// (a voxel horse keeps its legs).
    #[serde(default)]
    pub thin: bool,
    /// What a picked material may land here as: `class` (made for this
    /// asset's class), `generic` (any material that isn't foreign),
    /// `recipe:<recipe>`, `sub:<category>/<subcategory>`.
    pub accepts: Vec<String>,
    /// Its own material when the pick lands elsewhere (a hoof stays horn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_material: Option<String>,
    /// Keeps the source model's own colour (eyes).
    #[serde(default)]
    pub keep: bool,
}

/// The camera that frames the asset (degrees; `fill` is the share of the
/// view's height the asset's bounds fill).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub yaw: f32,
    pub pitch: f32,
    pub fill: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRow {
    pub id: String,
    pub name: String,
    pub category: String,
    pub subcategory: String,
    pub class: String,
    pub search_words: String,
    pub source: Source,
    pub regions: Vec<Region>,
    /// The parts the user can give their own material (every region in
    /// exactly one part).
    pub parts: Vec<Part>,
    pub default_material: String,
    pub frame: Frame,
    /// The library batch (0 = the proof set B0).
    pub batch: u8,
    pub status: AssetStatus,
    /// The asset browser's picture: a file under the app's `models/`
    /// (`thumbs/<id>.png`, drawn from the source by
    /// `evani-style-geo/examples/studio_assets.rs thumb`). A ready asset has
    /// one (`check::asset_files`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// The device tiers (`assets/tiers/*.json`) whose download carries this
    /// asset's source; a ready asset lists every one, and each must resolve
    /// to a file that reads (`check::asset_files` and evani-style-geo's
    /// source check).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<String>,
}

impl AssetRow {
    pub fn region(&self, id: &str) -> Option<usize> {
        self.regions.iter().position(|r| r.id == id)
    }

    pub fn part(&self, id: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    /// The part (index into `parts`) region `region` belongs to.
    pub fn part_of_region(&self, region: usize) -> Option<usize> {
        let id = &self.regions.get(region)?.id;
        self.parts.iter().position(|p| p.regions.iter().any(|r| r == id))
    }

    /// The region indices of part `part`, its lead first.
    pub fn part_regions(&self, part: &Part) -> Vec<usize> {
        part.regions.iter().filter_map(|r| self.region(r)).collect()
    }

    /// The lead region of part `part` (index into `regions`).
    pub fn part_lead(&self, part: &Part) -> Option<usize> {
        part.regions.first().and_then(|r| self.region(r))
    }

    /// Does region `region` follow a region of its own part (it keeps
    /// following when the part gets a material)?
    pub fn follows_within_part(&self, region: usize) -> bool {
        let Some(l) = self.regions.get(region).and_then(|r| r.follows.as_deref()).and_then(|l| self.region(l)) else { return false };
        self.part_of_region(region) == self.part_of_region(l)
    }
}

/// A dotted lowercase id: `[a-z0-9_]+(\.[a-z0-9_]+)+`.
pub fn dotted_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
}

/// A plain lowercase name: `[a-z0-9_]+`.
pub fn plain_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Is `token` a well-formed `accepts` entry?
pub fn accept_token_ok(token: &str) -> bool {
    match token.split_once(':') {
        None => matches!(token, "class" | "generic"),
        Some(("recipe", r)) => plain_id(r),
        Some(("sub", cs)) => cs.split_once('/').is_some_and(|(c, s)| plain_id(c) && plain_id(s)),
        Some(_) => false,
    }
}

impl AssetCatalog {
    /// Parse and check the file on its own ([`crate::check::problems`] does
    /// the cross-checks with the materials).
    pub fn parse(json: &str) -> Result<Self, String> {
        let c: AssetCatalog = serde_json::from_str(json).map_err(|e| format!("assets.json: {e}"))?;
        c.validate()?;
        Ok(c)
    }

    pub fn asset(&self, id: &str) -> Option<&AssetRow> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn class(&self, id: &str) -> Option<&ClassDef> {
        self.classes.iter().find(|c| c.id == id)
    }

    /// The asset class `a` belongs to.
    pub fn class_of(&self, a: &AssetRow) -> Option<&ClassDef> {
        self.class(&a.class)
    }

    fn validate(&self) -> Result<(), String> {
        let err = |at: &str, what: String| Err(format!("assets.json: {at}: {what}"));
        if self.version != VERSION {
            return err("version", format!("must be {VERSION}"));
        }
        let mut seen = BTreeSet::new();
        for c in &self.categories {
            if !plain_id(&c.id) || c.name.trim().is_empty() || !seen.insert(c.id.as_str()) {
                return err(&c.id, "a category needs a unique plain id and a name".into());
            }
        }
        let mut classes = BTreeSet::new();
        for k in &self.classes {
            if !dotted_id(&k.id) || !classes.insert(k.id.as_str()) {
                return err(&k.id, "a class needs a unique `family.kind` id".into());
            }
            if [&k.name, &k.label, &k.plural].iter().any(|s| s.trim().is_empty()) {
                return err(&k.id, "a class needs a name, a label and a plural".into());
            }
            match self.asset(&k.primary_asset) {
                Some(a) if a.class == k.id => {}
                _ => return err(&k.id, format!("primary_asset `{}` must be an asset of this class", k.primary_asset)),
            }
        }
        let mut kinds = BTreeSet::new();
        for k in &self.part_kinds {
            if !plain_id(&k.id) || k.name.trim().is_empty() || !kinds.insert(k.id.as_str()) {
                return err(&k.id, "a part kind needs a unique plain id and a name".into());
            }
        }
        let mut ids = BTreeSet::new();
        for a in &self.assets {
            let at = a.id.as_str();
            if !dotted_id(at) || !ids.insert(at) {
                return err(at, "an asset needs a unique `family.name` id".into());
            }
            if a.name.trim().is_empty() || a.search_words.trim().is_empty() {
                return err(at, "needs a name and search words".into());
            }
            if !self.categories.iter().any(|c| c.id == a.category) {
                return err(at, format!("category `{}` isn't in `categories`", a.category));
            }
            if !plain_id(&a.subcategory) {
                return err(at, "subcategory must be a plain id".into());
            }
            if self.class(&a.class).is_none() {
                return err(at, format!("class `{}` isn't in `classes`", a.class));
            }
            Self::validate_source(a).or_else(|e| err(at, e))?;
            Self::validate_regions(a).or_else(|e| err(at, e))?;
            self.validate_parts(a).or_else(|e| err(at, e))?;
            if let Some(t) = &a.thumbnail
                && *t != format!("thumbs/{at}.png")
            {
                return err(at, format!("thumbnail `{t}`: must be `thumbs/{at}.png` (under the app's models/)"));
            }
            let mut tiers = BTreeSet::new();
            if let Some(t) = a.tiers.iter().find(|t| !plain_id(t) || !tiers.insert(t.as_str())) {
                return err(at, format!("tiers: `{t}` must be a plain device tier id, listed once"));
            }
            let f = a.frame;
            let ok = (-180.0..=180.0).contains(&f.yaw) && (-89.0..=89.0).contains(&f.pitch) && (0.1..=1.0).contains(&f.fill);
            if !ok {
                return err(at, "frame: yaw -180..180, pitch -89..89, fill 0.1..1".into());
            }
        }
        match self.asset(&self.default_asset) {
            None => err("default_asset", format!("`{}` isn't an asset", self.default_asset)),
            Some(a) if self.class(&a.class).is_some_and(|k| k.swatch) => {
                err("default_asset", "the studio opens on a real subject, not a test shape (spec section 2: no demonstration sphere)".into())
            }
            Some(_) => Ok(()),
        }
    }

    fn validate_source(a: &AssetRow) -> Result<(), String> {
        let s = &a.source;
        let filled = |o: &Option<String>| o.as_deref().is_some_and(|x| !x.trim().is_empty());
        let ok = match s.kind {
            SourceKind::Code => filled(&s.shape) && s.model.is_none() && s.script.is_none(),
            SourceKind::BlenderScript => filled(&s.script) && filled(&s.model) && s.shape.is_none(),
            SourceKind::Model => filled(&s.model) && s.shape.is_none() && s.script.is_none(),
        };
        if ok { Ok(()) } else { Err("source: code needs `shape`; blender-script needs `script` and `model`; model needs `model`".into()) }
    }

    fn validate_parts(&self, a: &AssetRow) -> Result<(), String> {
        if a.parts.is_empty() || a.parts.len() > MAX_SLOTS {
            return Err(format!("needs 1 to {MAX_SLOTS} parts (every region in one)"));
        }
        let mut seen = BTreeSet::new();
        let mut covered = BTreeSet::new();
        for p in &a.parts {
            if !plain_id(&p.id) || !seen.insert(p.id.as_str()) || p.label.trim().is_empty() {
                return Err(format!("part `{}`: needs a unique plain id and a label", p.id));
            }
            if !self.part_kinds.iter().any(|k| k.id == p.kind) {
                return Err(format!("part `{}`: kind `{}` isn't in `part_kinds`", p.id, p.kind));
            }
            if p.regions.is_empty() {
                return Err(format!("part `{}`: lists no region", p.id));
            }
            for r in &p.regions {
                if a.region(r).is_none() || !covered.insert(r.as_str()) {
                    return Err(format!("part `{}`: region `{r}` isn't a region of the asset, or is in another part already", p.id));
                }
            }
        }
        if let Some(r) = a.regions.iter().find(|r| !covered.contains(r.id.as_str())) {
            return Err(format!("region `{}` is in no part: every region belongs to exactly one part", r.id));
        }
        for p in &a.parts {
            if a.part_lead(p).is_some_and(|l| a.follows_within_part(l)) {
                return Err(format!("part `{}`: its lead region `{}` follows a region of the same part; list the leader first", p.id, p.regions[0]));
            }
        }
        Ok(())
    }

    fn validate_regions(a: &AssetRow) -> Result<(), String> {
        if a.regions.is_empty() || a.regions.len() > MAX_SLOTS {
            return Err(format!("needs 1 to {MAX_SLOTS} regions (one material slot each)"));
        }
        let mut seen = BTreeSet::new();
        for r in &a.regions {
            if !plain_id(&r.id) || !seen.insert(r.id.as_str()) {
                return Err(format!("region `{}`: needs a unique plain id", r.id));
            }
            if let Some(bad) = r.accepts.iter().find(|t| !accept_token_ok(t)) {
                return Err(format!("region `{}`: `{bad}` isn't class, generic, recipe:<id> or sub:<category>/<sub>", r.id));
            }
            if let Some(l) = &r.follows {
                match a.regions.iter().find(|x| &x.id == l) {
                    Some(x) if x.follows.is_none() && x.id != r.id && !x.keep => {}
                    _ => return Err(format!("region `{}`: follows `{l}`, which must be another region that follows none and isn't kept", r.id)),
                }
            }
            if r.keep && (r.default_material.is_some() || r.follows.is_some()) {
                return Err(format!("region `{}`: a kept region has no material", r.id));
            }
            if !r.keep && r.accepts.is_empty() && r.default_material.is_none() && r.follows.is_none() {
                return Err(format!("region `{}`: accepts nothing and has no material: keep it, give it a default_material or let it follow", r.id));
            }
        }
        if !a.regions.iter().any(|r| r.follows.is_none() && !r.keep && r.accepts.iter().any(|t| t == "class" || t == "generic")) {
            return Err("no leading region accepts `class` or `generic`: the asset's own materials could land nowhere".into());
        }
        Ok(())
    }
}
