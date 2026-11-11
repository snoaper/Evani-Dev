//! The material catalogue in the viewer (kit feature `catalogue`; batch 1
//! build plan 2.7, packages P13 and P14). Dev data only: the 1,054 rows of
//! `catalog/materials/materials.json` and the 29 recipes ride in the viewer
//! (`evani-material-recipes/data`), never in a game.
//!
//! * [`Library`]: the catalogue, its category tree (9 categories plus the two
//!   optional ones, each with subcategories and "ready / total" counts), the
//!   search index and the filters. Search folds case and accents, ranks an
//!   exact name, then a name start, then the name, id and search words, then
//!   the look note, and forgives one typo in a word of 4 letters or more (two
//!   from 8), with a prefix match on the word being typed. Ties go by id, so
//!   the order is the same every run.
//! * Only `ready` (or `reviewed`) rows open; planned rows show greyed with
//!   their reason ([`Library::planned_reason`], data/planned.json).
//! * One control API for a person and an AI: opening a row and every switch
//!   on it are registered params, so the page's clicks and evani-live's
//!   `param_set` are the same edit:
//!   - `kit.material_view.cat.open` (enum: `none` or a ready row id),
//!     `kit.material_view.cat.shape` (enum: `auto` or a test shape);
//!   - `kit.material_view.cond.<layer>.on` and `.amount` (the open material's
//!     own condition switches, written into its block);
//!   - `kit.material_view.extra.<recipe>.<extra>` (the open material's extras
//!     bits) and `kit.material_view.mat.<number>` (one per number of the open
//!     recipe, over that number's range), registered the first time a row of
//!     that recipe opens;
//!   - `kit.material_view.detail.<style>.strength` and `.step`: how much of
//!     the open material's own pattern a non-realistic style keeps, and how
//!     coarsely it is posterized (one pair per style with a shape look;
//!     defaults from styles.json `render.detail`, `render.detail_step`);
//!   - the engine's own `material.quality.<group>` and `material.cond.*`.
//! * [`CatalogueView`] holds what is open and its current edited row; the
//!   app draws it (the material-view app's `scenes/catalogue.rs`).

use std::collections::BTreeMap;
use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{BoolParam, EnumParam, F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{PluginContext, PluginError};
use evani_material_recipes::block::{Block, CONDITION_NAMES};
use evani_material_recipes::catalog::{Catalog, Recipe, Row};
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Why planned rows are planned (data/planned.json).
pub const PLANNED_JSON: &str = include_str!("../data/planned.json");

/// Param names.
pub const OPEN: &str = "kit.material_view.cat.open";
pub const SHAPE: &str = "kit.material_view.cat.shape";
pub const COND: &str = "kit.material_view.cond";
pub const EXTRA: &str = "kit.material_view.extra";
pub const MAT: &str = "kit.material_view.mat";
pub const DETAIL: &str = "kit.material_view.detail";

/// The categories in tree order with their display names (CATALOGUE_PLAN
/// section 2); the last two are the optional ones.
pub const CATEGORIES: [(&str, &str); 11] = [
    ("animals", "Animals"),
    ("people", "People"),
    ("clothing", "Clothing"),
    ("weapons_tools", "Weapons and tools"),
    ("buildings", "Buildings"),
    ("land", "Land"),
    ("plants", "Plants"),
    ("household", "House props"),
    ("elements", "Water, ice and fire"),
    ("vehicles", "Vehicles"),
    ("fantasy_scifi", "Fantasy and sci-fi"),
];

/// The recipe filter chips, grouped (CATALOGUE_PLAN section 8).
pub const RECIPE_GROUPS: [(&str, &[&str]); 4] = [
    ("Hard", &["metal", "paint", "plastic", "wood", "stone", "masonry", "plaster", "ground", "ceramic", "gem"]),
    ("Soft", &["cloth", "leather", "thin", "organic"]),
    ("Living", &["fur", "feather", "scales", "hide", "skin", "hair", "eye", "keratin", "foliage", "bark"]),
    ("Liquid and light", &["water", "glass", "ice", "fire", "glow"]),
];

/// The test shapes a catalogue material can sit on (`cat.shape`), `auto`
/// first: the recipe's own viewer scene picks.
/// B2-P8 added `leaf`, `spray` and `tuft` (a card cut to a leaf, a needle
/// spray or a grass tuft by an alpha mask), `slope` (a terrain tile with a
/// bank) and `rock` (a lumpy sphere). Critic round 1 added `petal` (a cupped
/// card cut to a rose petal). Round 4 added the shapes rows name in their
/// catalogue `shape`: an oak and a maple leaf card, wheat with ears, a hay
/// pile and a meadow tuft (masked cards), a quartz cluster, a slime blob and
/// a rose of layered petals. Round 5 added a pine sprig card, and its
/// follow-up spruce and fir sprigs (short needles; a flat comb). Round 11:
/// the lawn row's own `lawn`, the slope with short standing blades.
pub const SHAPES: [&str; 31] = [
    "auto", "ball", "tile", "cylinder", "card", "cube", "monkey", "teapot", "ring", "cushion", "head", "leaf", "spray", "tuft", "slope", "rock", "petal", "oak_leaf", "maple_leaf", "wheat", "hay", "meadow", "crystal", "blob", "rose", "pine", "spruce", "fir", "slab", "pie", "lawn",
];

/// Lower case with accents folded; anything not a letter or digit becomes a
/// space.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let f = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'ç' | 'č' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'ē' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ñ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => "o",
            'ù' | 'ú' | 'û' | 'ü' => "u",
            'ý' | 'ÿ' => "y",
            'æ' => "ae",
            'œ' => "oe",
            'ß' => "ss",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => " ",
        };
        out.push_str(f);
    }
    out
}

/// The folded words of `s`.
pub fn words(s: &str) -> Vec<String> {
    fold(s).split_whitespace().map(str::to_string).collect()
}

/// Edit distance (insert, delete, change), giving up past `max`.
pub fn edits(a: &str, b: &str, max: usize) -> Option<usize> {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        if cur.iter().min().is_some_and(|&m| m > max) {
            return None;
        }
        prev = cur;
    }
    prev.last().copied().filter(|&d| d <= max)
}

/// How many typos a query word may carry.
pub fn typos_allowed(w: &str) -> usize {
    match w.chars().count() {
        n if n >= 8 => 2,
        n if n >= 4 => 1,
        _ => 0,
    }
}

/// How well query word `w` matches one of `tokens`: 0 exact, 1 a prefix (only
/// for the word being typed), 2 a typo; `None` when it doesn't.
fn word_match(w: &str, last: bool, tokens: &[String]) -> Option<u8> {
    if tokens.iter().any(|t| t == w) {
        return Some(0);
    }
    if last && tokens.iter().any(|t| t.starts_with(w)) {
        return Some(1);
    }
    let k = typos_allowed(w);
    (k > 0 && tokens.iter().any(|t| edits(t, w, k).is_some())).then_some(2)
}

/// A row's search index.
#[derive(Debug, Clone, Default)]
struct Entry {
    name: String,
    /// Name and id words.
    name_words: Vec<String>,
    search_words: Vec<String>,
    note_words: Vec<String>,
}

/// One search result: the row (index into the catalogue's rows) and its
/// rank (lower first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub row: usize,
    /// 0 exact name, 1 name start, 2 name / id / search words, 3 look note.
    pub tier: u8,
    /// 0 exact words, 1 a prefix, 2 a typo.
    pub fuzz: u8,
    /// Query words found only in the search words or note (fewer first).
    pub off_name: u8,
}

/// Ready or planned, for the status filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Ready,
    Planned,
}

/// What the list is narrowed to. Everything combines with the search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub category: Option<String>,
    pub subcategory: Option<String>,
    /// Any of these recipes (empty: all).
    pub recipes: Vec<String>,
    /// Rows listing every one of these layers.
    pub conditions: Vec<String>,
    /// Only rows the engine had before the catalogue (`exists_today`).
    pub exists_today: bool,
    pub batch: Option<u8>,
    pub status: Option<StatusFilter>,
}

/// One subcategory in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubNode {
    pub id: String,
    pub name: String,
    pub ready: usize,
    pub total: usize,
}

/// One category in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatNode {
    pub id: String,
    pub name: String,
    pub ready: usize,
    pub total: usize,
    pub subs: Vec<SubNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlannedGroup {
    reason: String,
    ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Planned {
    default: String,
    batch: String,
    backend: BTreeMap<String, String>,
    groups: Vec<PlannedGroup>,
    rows: BTreeMap<String, String>,
}

/// The catalogue, its index and the planned reasons.
#[derive(Debug, Clone)]
pub struct Library {
    pub cat: Catalog,
    entries: Vec<Entry>,
    planned: Planned,
}

/// A subcategory id as words: `horse_coats` is "Horse coats".
pub fn sub_name(id: &str) -> String {
    let s = id.replace('_', " ");
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// The display name of category `id`.
pub fn category_name(id: &str) -> String {
    CATEGORIES.iter().find(|(c, _)| *c == id).map_or_else(|| sub_name(id), |(_, n)| (*n).to_string())
}

/// The recipe group a recipe's chip sits in.
pub fn recipe_group(recipe: &str) -> &'static str {
    RECIPE_GROUPS.iter().find(|(_, rs)| rs.contains(&recipe)).map_or("Other", |(g, _)| g)
}

impl Library {
    /// The catalogue built into the viewer.
    pub fn bundled() -> Result<Self, String> {
        let cat = Catalog::bundled().map_err(|e| format!("catalogue: {e}"))?;
        Self::new(cat, PLANNED_JSON)
    }

    /// Index `cat`; `planned` is data/planned.json's text.
    pub fn new(cat: Catalog, planned: &str) -> Result<Self, String> {
        let planned: Planned = serde_json::from_str(planned).map_err(|e| format!("planned.json: {e}"))?;
        let entries = cat
            .rows
            .iter()
            .map(|r| {
                let mut name_words = words(&r.name);
                for w in words(&r.id) {
                    if !name_words.contains(&w) {
                        name_words.push(w);
                    }
                }
                Entry { name: words(&r.name).join(" "), name_words, search_words: words(&r.search_words), note_words: words(&r.look_note) }
            })
            .collect();
        Ok(Self { cat, entries, planned })
    }

    /// Whether row `i` can be opened (its numbers are set and it was looked at).
    pub fn openable(&self, i: usize) -> bool {
        self.cat.rows.get(i).is_some_and(row_ready)
    }

    /// The row with this id and its index.
    pub fn find(&self, id: &str) -> Option<(usize, &Row)> {
        self.cat.rows.iter().enumerate().find(|(_, r)| r.id == id)
    }

    /// Why row `i` is still planned (`None` for a ready one).
    pub fn planned_reason(&self, i: usize) -> Option<String> {
        let r = self.cat.rows.get(i)?;
        if row_ready(r) {
            return None;
        }
        if let Some(s) = self.planned.rows.get(&r.id) {
            return Some(s.clone());
        }
        if let Some(g) = self.planned.groups.iter().find(|g| g.ids.contains(&r.id)) {
            return Some(g.reason.clone());
        }
        if r.batch != 1 && r.batch != 11 {
            return Some(self.planned.batch.replace("{batch}", &r.batch.to_string()));
        }
        let backend = self.cat.recipes.iter().find(|x| x.id == r.recipe).map(|x| x.backend.as_str()).unwrap_or("");
        Some(self.planned.backend.get(backend).cloned().unwrap_or_else(|| self.planned.default.clone()))
    }

    /// Whether row `r` passes `f`.
    pub fn passes(&self, r: &Row, f: &Filter) -> bool {
        f.category.as_ref().is_none_or(|c| *c == r.category)
            && f.subcategory.as_ref().is_none_or(|s| *s == r.subcategory)
            && (f.recipes.is_empty() || f.recipes.contains(&r.recipe))
            && f.conditions.iter().all(|c| r.conditions.contains(c))
            && (!f.exists_today || !r.exists_today.is_empty())
            && f.batch.is_none_or(|b| b == r.batch)
            && match f.status {
                None => true,
                Some(StatusFilter::Ready) => row_ready(r),
                Some(StatusFilter::Planned) => !row_ready(r),
            }
    }

    /// How row `i` matches `query` (already folded into words), if it does.
    fn rank(&self, i: usize, q: &str, qw: &[String]) -> Option<Hit> {
        let e = self.entries.get(i)?;
        if qw.is_empty() {
            return Some(Hit { row: i, tier: 0, fuzz: 0, off_name: 0 });
        }
        let mut tier = 2u8;
        let mut fuzz = 0u8;
        let mut off_name = 0u8;
        for (k, w) in qw.iter().enumerate() {
            let last = k + 1 == qw.len();
            let named = word_match(w, last, &e.name_words);
            let found = named.or_else(|| word_match(w, last, &e.search_words)).map(|m| (m, 2u8)).or_else(|| word_match(w, last, &e.note_words).map(|m| (m, 3u8)));
            let (m, t) = found?;
            if named.is_none() {
                off_name = off_name.saturating_add(1);
            }
            fuzz = fuzz.max(m);
            tier = tier.max(t);
        }
        if e.name == q {
            tier = 0;
        } else if e.name.starts_with(q) {
            tier = tier.min(1);
        }
        Some(Hit { row: i, tier, fuzz, off_name })
    }

    /// The rows that match `query` and pass `f`, best first (ties by id). An
    /// empty query lists every row passing `f` in catalogue order.
    pub fn search(&self, query: &str, f: &Filter) -> Vec<Hit> {
        let qw = words(query);
        let q = qw.join(" ");
        let mut hits: Vec<Hit> = (0..self.cat.rows.len()).filter(|&i| self.passes(&self.cat.rows[i], f)).filter_map(|i| self.rank(i, &q, &qw)).collect();
        if !qw.is_empty() {
            let id = |h: &Hit| self.cat.rows[h.row].id.as_str();
            hits.sort_by(|a, b| (a.tier, a.fuzz, a.off_name).cmp(&(b.tier, b.fuzz, b.off_name)).then_with(|| id(a).cmp(id(b))));
        }
        hits
    }

    /// The category tree over `rows` (indices; all rows for the full counts),
    /// in [`CATEGORIES`] order, subcategories in catalogue order. Categories
    /// with none of the rows are left out.
    pub fn tree(&self, rows: &[usize]) -> Vec<CatNode> {
        let mut out: Vec<CatNode> = CATEGORIES
            .iter()
            .map(|(id, name)| CatNode { id: (*id).to_string(), name: (*name).to_string(), ready: 0, total: 0, subs: Vec::new() })
            .collect();
        for &i in rows {
            let Some(r) = self.cat.rows.get(i) else { continue };
            let at = match out.iter().position(|c| c.id == r.category) {
                Some(at) => at,
                None => {
                    out.push(CatNode { id: r.category.clone(), name: category_name(&r.category), ready: 0, total: 0, subs: Vec::new() });
                    out.len() - 1
                }
            };
            let c = &mut out[at];
            let ready = usize::from(row_ready(r));
            c.total += 1;
            c.ready += ready;
            match c.subs.iter_mut().find(|s| s.id == r.subcategory) {
                Some(s) => {
                    s.total += 1;
                    s.ready += ready;
                }
                None => c.subs.push(SubNode { id: r.subcategory.clone(), name: sub_name(&r.subcategory), ready, total: 1 }),
            }
        }
        out.retain(|c| c.total > 0);
        out
    }

    /// The ids of every row that can open, in catalogue order.
    pub fn ready_ids(&self) -> Vec<&str> {
        self.cat.rows.iter().filter(|r| row_ready(r)).map(|r| r.id.as_str()).collect()
    }
}

/// `ready`, `reviewed` or `alias` (an alias opens its base row with the
/// alias's own conditions on: "black ice" is asphalt with the wet layer).
pub fn row_ready(r: &Row) -> bool {
    r.status == "ready" || r.status == "reviewed" || r.status == "alias"
}

/// The row to draw for catalogue row `r`: the row itself, or for an alias its
/// base row under the alias's own id and name, with the alias's condition
/// amounts laid over, as an ordinary ready row.
pub fn working_row(cat: &Catalog, r: &Row) -> Row {
    match cat.resolved(r) {
        Ok(c) => {
            let mut w = c.into_owned();
            if w.status == "alias" {
                w.status = "ready".into();
            }
            w
        }
        Err(_) => r.clone(),
    }
}

// ------------------------------------------------------------ the switches

/// The condition switch handles, in block bit order.
#[derive(Debug, Clone, Copy)]
pub struct CondParams {
    pub on: [BoolParam; 8],
    pub amount: [F32Param; 8],
}

/// What is open, as the app draws it (a world resource).
#[derive(Debug, Clone, Default)]
pub struct CatalogueView {
    /// The open row's index.
    pub open: Option<usize>,
    /// The open row with the switches applied: its numbers, extras and
    /// condition amounts as the sliders have them, and only the conditions
    /// switched on. What the shape's glTF material is written from.
    pub edited: Option<Row>,
    /// Its block (recipe-stage rows only).
    pub block: Option<Block>,
    /// Bumped when `open` changes.
    pub opened: u64,
    /// Bumped whenever `edited` changes.
    pub revision: u64,
    /// The shape: `cat.shape`, with `auto` resolved by the recipe's scene.
    pub shape: String,
    reg_seen: u64,
}

/// The parsed library (shared, read-only).
#[derive(Debug, Clone)]
pub struct LibraryData(pub Rc<Library>);

#[derive(Debug, Clone, Copy)]
struct Handles {
    open: EnumParam,
    shape: EnumParam,
    cond: CondParams,
}

/// The shape a row opens on: its recipe's, except that (B2-P8) leaves,
/// needles and grass blades open on a masked card (leaf, spray, tuft), turf,
/// litter, moss and ground on a bank, and stone on a rock; petals (critic
/// round 1) on a cupped petal card. A row that names its own `shape` in the
/// catalogue (round 4: oak and maple leaves, wheat, hay, meadow, the quartz,
/// slime and the rose) opens on that, when the viewer draws it.
pub fn shape_for(recipe: &Recipe, row: &Row) -> &'static str {
    if let Some(s) = row.params.get("shape").and_then(Value::as_str).and_then(|s| SHAPES.iter().copied().find(|x| *x == s && *x != "auto")) {
        return s;
    }
    let variant = row.params.get("variant").and_then(Value::as_str).unwrap_or("");
    match (recipe.id.as_str(), variant) {
        ("foliage", "needle") => "spray",
        ("foliage", "grass" | "straw") => "tuft",
        ("foliage", "turf" | "moss" | "litter") | ("ground", _) => "slope",
        ("foliage", _) => "leaf",
        ("stone", _) => "rock",
        ("thin", "petal") => "petal",
        // A hoof is a wall: tubules and stripes run down it (v along the
        // cylinder), as on a ball they would bend into the poles.
        ("keratin", "hoof") => "cylinder",
        // P7-2: a quill is one feather on a card (the leaf card's outline is
        // a vane's), a wing a card with its base at the bottom (the petal's
        // round outline), shrimp segments are rings round a body.
        ("feather", "quill") => "leaf",
        ("thin", "wing") => "petal",
        ("thin", "segments") => "cylinder",
        _ => auto_shape(recipe),
    }
}

/// The shape a recipe opens on (its `scene` in recipes.json).
pub fn auto_shape(recipe: &Recipe) -> &'static str {
    match recipe.scene.as_str() {
        "tile" => "tile",
        "cylinder" => "cylinder",
        "card" => "card",
        "cushion" => "cushion",
        "head" => "head",
        _ => "ball",
    }
}

/// `kit.material_view.mat.<number>`.
pub fn number_param(name: &str) -> String {
    format!("{MAT}.{name}")
}

/// `kit.material_view.extra.<recipe>.<extra>`.
pub fn extra_param(recipe: &str, extra: &str) -> String {
    format!("{EXTRA}.{recipe}.{extra}")
}

/// `kit.material_view.cond.<layer>.<what>`.
pub fn cond_param(layer: &str, what: &str) -> String {
    format!("{COND}.{layer}.{what}")
}

/// `kit.material_view.detail.<style>.<what>` (`style` the style id, lower
/// case: `wk01`).
pub fn detail_param(style: &str, what: &str) -> String {
    format!("{DETAIL}.{}.{what}", style.to_ascii_lowercase().replace('-', "_"))
}

/// Each shaped style's detail switches (strength, step), by style id.
#[derive(Debug, Clone, Default)]
pub struct DetailParams(pub BTreeMap<String, (F32Param, F32Param)>);

impl DetailParams {
    /// A style's (strength, step) now, or `None` for a style without them.
    pub fn get(&self, reg: &ParamRegistry, style: &str) -> Option<(f32, f32)> {
        self.0.get(style).map(|&(s, k)| (reg.f32(s), reg.f32(k)))
    }
}

/// Register the detail pair of every style with a shape look.
fn detail_params(ctx: &mut PluginContext) -> Result<DetailParams, PluginError> {
    let styles = crate::styles::Styles::shipped().map_err(PluginError)?;
    let mut out = DetailParams::default();
    for s in &styles.styles {
        let Some(r) = s.render.as_ref().filter(|r| r.shaped()) else { continue };
        let strength = ctx.param_f32(
            ParamDef::f32(&detail_param(&s.id, "strength"), r.detail)
                .range(0.0, 2.0)
                .step(0.05)
                .describe(&format!("{}: how much of a catalogue material's own pattern (grain, stripes, scales, rust) shows in this style (0 = flat colour, 1 = as it is).", s.name)),
        )?;
        let step = ctx.param_f32(
            ParamDef::f32(&detail_param(&s.id, "step"), r.detail_step)
                .range(0.0, 1.75)
                .step(0.25)
                .describe(&format!("{}: the pattern posterized in steps of this many stops (0 = smooth; bigger = fewer, cleaner strokes).", s.name)),
        )?;
        out.0.insert(s.id.clone(), (strength, step));
    }
    Ok(out)
}

/// Register the catalogue (kit feature `catalogue`): the library, the open
/// and shape switches, the 16 condition switches, and the system that keeps
/// [`CatalogueView`] in step with them.
pub fn install(ctx: &mut PluginContext) -> Result<(), PluginError> {
    let lib = Library::bundled().map_err(PluginError)?;
    let mut options = vec!["none"];
    options.extend(lib.ready_ids());
    let open = ctx.param_enum(ParamDef::enumeration(OPEN, &options, 0).describe("The catalogue material open on the test shape (none: the art style's own materials)."))?;
    let shape = ctx.param_enum(ParamDef::enumeration(SHAPE, &SHAPES, 0).describe("The test shape the catalogue material sits on (auto: the recipe's own)."))?;
    let mut on = Vec::new();
    let mut amount = Vec::new();
    for layer in CONDITION_NAMES {
        on.push(ctx.param_bool(ParamDef::bool(&cond_param(layer, "on"), false).describe(&format!("The open material's {layer} layer on or off (only if it lists {layer}).")))?);
        amount.push(ctx.param_f32(ParamDef::f32(&cond_param(layer, "amount"), 0.5).range(0.0, 1.0).step(0.01).describe(&format!("How much {layer} the open material carries.")))?);
    }
    let cond = CondParams {
        on: on.try_into().map_err(|_| PluginError("catalogue: condition switches".into()))?,
        amount: amount.try_into().map_err(|_| PluginError("catalogue: condition amounts".into()))?,
    };
    ctx.data_file("material_view.planned", PLANNED_JSON);
    ctx.resource(LibraryData(Rc::new(lib)));
    let detail = detail_params(ctx)?;
    ctx.resource(Handles { open, shape, cond });
    ctx.resource(detail);
    ctx.resource(CatalogueView::default());
    ctx.system(SystemDesc::new("material_view.catalogue", Stage::Frame, sync_system));
    Ok(())
}

/// The condition handles (for the page).
pub fn cond_params(world: &World) -> Option<CondParams> {
    world.try_res::<Handles>().map(|h| h.cond)
}

/// Register `def` unless a param of that name exists.
fn ensure(reg: &mut ParamRegistry, def: ParamDef) {
    if reg.id(&def.name).is_none() {
        let _ = reg.register(def);
    }
}

/// The value a row gives number `n` of its recipe.
fn row_number(row: &Row, name: &str) -> Option<f32> {
    row.params.get("numbers")?.get(name)?.as_f64().map(|x| x as f32)
}

/// The extras a row has on (its list, else the recipe's defaults).
fn row_extras<'a>(row: &Row, recipe: &'a Recipe) -> Vec<&'a str> {
    match row.params.get("extras").and_then(Value::as_array) {
        Some(list) => recipe.extras.iter().filter(|e| list.iter().any(|x| x.as_str() == Some(e.name.as_str()))).map(|e| e.name.as_str()).collect(),
        None => recipe.extras.iter().filter(|e| e.default).map(|e| e.name.as_str()).collect(),
    }
}

/// Opening row `row`: register its recipe's number and extra switches (the
/// first time) and set every switch to the row's own values.
pub fn open_row(reg: &mut ParamRegistry, cond: &CondParams, recipe: &Recipe, row: &Row) {
    for n in &recipe.numbers {
        let name = number_param(&n.name);
        let v = row_number(row, &n.name).unwrap_or(n.default).clamp(n.min, n.max);
        ensure(reg, ParamDef::f32(&name, n.default).range(n.min, n.max).describe(&format!("The open material's {} (its recipe's number).", n.name.replace('_', " "))));
        if let Some(id) = reg.id(&name) {
            let _ = reg.set_range(id, n.min, n.max);
            let _ = reg.set(id, ParamValue::F32(v));
        }
    }
    let on = row_extras(row, recipe);
    for e in &recipe.extras {
        let name = extra_param(&recipe.id, &e.name);
        ensure(reg, ParamDef::bool(&name, e.default).describe(&format!("The open {} material's {} extra.", recipe.id, e.name.replace('_', " "))));
        let _ = reg.set_by_name(&name, ParamValue::Bool(on.contains(&e.name.as_str())));
    }
    for (bit, layer) in CONDITION_NAMES.iter().enumerate() {
        let own = row.cond_defaults.get(*layer).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let _ = reg.set(cond.on[bit].id(), ParamValue::Bool(own > 0.0 && row.conditions.iter().any(|c| c == layer)));
        let _ = reg.set(cond.amount[bit].id(), ParamValue::F32(if own > 0.0 { own } else { 0.5 }));
    }
}

/// The open row with the switches applied: numbers and extras from their
/// params, only the listed conditions that are on, each at its amount.
pub fn edited_row(reg: &ParamRegistry, cond: &CondParams, recipe: &Recipe, row: &Row) -> Row {
    let mut r = row.clone();
    let mut numbers = Map::new();
    for n in &recipe.numbers {
        let v = reg.id(&number_param(&n.name)).and_then(|id| reg.value(id)).and_then(|v| match v {
            ParamValue::F32(x) => Some(x),
            _ => None,
        });
        let v = v.or_else(|| row_number(row, &n.name)).unwrap_or(n.default).clamp(n.min, n.max);
        numbers.insert(n.name.clone(), json!(v));
    }
    r.params.insert("numbers".into(), Value::Object(numbers));
    let extras: Vec<Value> = recipe.extras.iter().filter(|e| reg.bool_or(&extra_param(&recipe.id, &e.name))).map(|e| json!(e.name)).collect();
    r.params.insert("extras".into(), Value::Array(extras));
    r.conditions = row.conditions.iter().filter(|c| CONDITION_NAMES.iter().position(|l| l == c).is_some_and(|bit| reg.bool(cond.on[bit]))).cloned().collect();
    r.cond_defaults = Map::new();
    for c in &r.conditions {
        if let Some(bit) = CONDITION_NAMES.iter().position(|l| l == c) {
            r.cond_defaults.insert(c.clone(), json!(reg.f32(cond.amount[bit]).clamp(0.0, 1.0)));
        }
    }
    r
}

/// Whether a recipe's materials are drawn by the recipe stage (a block).
pub fn has_block(recipe: &Recipe) -> bool {
    recipe.backend == "recipes" || recipe.also.iter().any(|a| a == "recipes")
}

/// Keep [`CatalogueView`] in step with the switches: a new `cat.open` opens
/// its row (setting every switch to the row's values); any switch moving
/// rewrites the edited row and its block.
pub fn sync_system(world: &mut World) {
    let Some(h) = world.try_res::<Handles>().map(|h| *h) else { return };
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.0.clone()) else { return };
    let (open_id, shape_i, rev) = {
        let reg = world.res::<ParamRegistry>();
        (reg.enum_index(h.open), reg.enum_index(h.shape), reg.revision())
    };
    let Some(mut view) = world.try_res_mut::<CatalogueView>() else { return };
    if rev == view.reg_seen && view.reg_seen != 0 {
        return;
    }
    view.reg_seen = rev;
    // Option 0 is `none`; option k is the k-th ready row.
    let want = usize::try_from(open_id).ok().and_then(|k| k.checked_sub(1)).and_then(|k| lib.ready_ids().get(k).copied()).and_then(|id| lib.find(id)).map(|(i, _)| i);
    if want != view.open {
        view.open = want;
        view.opened += 1;
        drop(view);
        if let Some(i) = want {
            let row = working_row(&lib.cat, &lib.cat.rows[i]);
            if let Ok(recipe) = lib.cat.recipe_for(&row) {
                open_row(&mut world.res_mut::<ParamRegistry>(), &h.cond, recipe, &row);
            }
        }
        view = world.res_mut::<CatalogueView>();
        view.reg_seen = world.res::<ParamRegistry>().revision();
    }
    let Some(i) = view.open else {
        if view.edited.is_some() {
            view.edited = None;
            view.block = None;
            view.revision += 1;
        }
        return;
    };
    let row = working_row(&lib.cat, &lib.cat.rows[i]);
    let Ok(recipe) = lib.cat.recipe_for(&row) else { return };
    let edited = edited_row(&world.res::<ParamRegistry>(), &h.cond, recipe, &row);
    let block = if has_block(recipe) { lib.cat.encode(&edited).ok() } else { None };
    let shape = match SHAPES.get(shape_i as usize).copied().unwrap_or("auto") {
        "auto" => shape_for(recipe, &row),
        s => s,
    };
    if view.edited.as_ref() != Some(&edited) || view.block != block || view.shape != shape {
        view.edited = Some(edited);
        view.block = block;
        view.shape = shape.to_string();
        view.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Library {
        Library::bundled().unwrap()
    }

    fn first(l: &Library, q: &str) -> String {
        let h = l.search(q, &Filter::default());
        h.first().map(|h| l.cat.rows[h.row].id.clone()).unwrap_or_default()
    }

    #[test]
    fn folding_and_typos() {
        assert_eq!(fold("Crème Brûlée"), "creme brulee");
        assert_eq!(words("horse.coat.bay"), ["horse", "coat", "bay"]);
        assert_eq!(edits("grnite", "granite", 1), Some(1));
        assert_eq!(edits("granite", "grenade", 1), None);
        assert_eq!(typos_allowed("cat"), 0);
        assert_eq!(typos_allowed("grnite"), 1);
        assert_eq!(typos_allowed("alligatr"), 2);
    }

    /// The plan's searches (P13): penny finds copper first, gator the
    /// alligator, a typo still finds granite.
    #[test]
    fn the_plan_searches() {
        let l = lib();
        assert_eq!(first(&l, "penny"), "metal.copper.polished");
        assert!(first(&l, "gator").starts_with("alligator."), "{}", first(&l, "gator"));
        assert!(first(&l, "grnite").contains("granite"), "{}", first(&l, "grnite"));
        // A name match beats a search-word match; an exact name beats all.
        assert_eq!(first(&l, "Copper, polished"), "metal.copper.polished");
        assert_eq!(l.search("Copper, polished", &Filter::default())[0].tier, 0);
        // The word being typed matches as a prefix.
        assert!(first(&l, "alliga").starts_with("alligator."));
        assert!(l.search("zzqqx", &Filter::default()).is_empty());
    }

    /// The same query always gives the same order, ranked then by id.
    #[test]
    fn ranking_is_stable() {
        let l = lib();
        for q in ["horse", "red", "wood", "grnite", "fur coat"] {
            let a = l.search(q, &Filter::default());
            let b = l.search(q, &Filter::default());
            assert_eq!(a, b, "{q}");
            for w in a.windows(2) {
                let key = |h: &Hit| (h.tier, h.fuzz, h.off_name, l.cat.rows[h.row].id.clone());
                assert!(key(&w[0]) < key(&w[1]), "{q}: {:?} then {:?}", key(&w[0]), key(&w[1]));
            }
        }
    }

    /// The tree's counts are CATALOGUE_PLAN section 2's.
    #[test]
    fn tree_counts_match_the_plan() {
        let l = lib();
        let all: Vec<usize> = (0..l.cat.rows.len()).collect();
        let t = l.tree(&all);
        let want = [
            ("animals", 283),
            ("people", 94),
            ("clothing", 104),
            ("weapons_tools", 62),
            ("buildings", 102),
            ("land", 90),
            ("plants", 98),
            ("household", 120),
            ("elements", 41),
            ("vehicles", 30),
            ("fantasy_scifi", 30),
        ];
        assert_eq!(t.iter().map(|c| (c.id.as_str(), c.total)).collect::<Vec<_>>(), want);
        assert_eq!(t.iter().map(|c| c.total).sum::<usize>(), 1054);
        let animals = &t[0];
        assert_eq!(animals.subs[0].name, "Horse coats");
        assert_eq!(animals.subs[0].total, 40);
        assert_eq!(animals.subs.iter().map(|s| s.total).sum::<usize>(), 283);
        for c in &t {
            assert_eq!(c.subs.iter().map(|s| s.ready).sum::<usize>(), c.ready, "{}", c.id);
            assert!(c.ready <= c.total);
        }
        assert_eq!(t.iter().map(|c| c.ready).sum::<usize>(), l.ready_ids().len());
    }

    /// Filters narrow the search and combine with it.
    #[test]
    fn filters_combine_with_search() {
        let l = lib();
        let all = l.search("horse", &Filter::default());
        let fur = Filter { recipes: vec!["fur".into()], ..Filter::default() };
        let horse_fur = l.search("horse", &fur);
        assert!(!horse_fur.is_empty() && horse_fur.len() < all.len());
        assert!(horse_fur.iter().all(|h| l.cat.rows[h.row].recipe == "fur"));
        let ready = Filter { status: Some(StatusFilter::Ready), ..fur.clone() };
        let r = l.search("horse", &ready);
        assert!(!r.is_empty() && r.iter().all(|h| l.openable(h.row)));
        let rust = Filter { conditions: vec!["oxide".into()], category: Some("weapons_tools".into()), ..Filter::default() };
        let hits = l.search("", &rust);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| {
            let r = &l.cat.rows[h.row];
            r.category == "weapons_tools" && r.conditions.iter().any(|c| c == "oxide")
        }));
        let b11 = Filter { batch: Some(11), ..Filter::default() };
        assert_eq!(l.search("", &b11).len(), 60);
        let today = Filter { exists_today: true, ..Filter::default() };
        assert!(l.search("", &today).iter().all(|h| !l.cat.rows[h.row].exists_today.is_empty()));
    }

    /// Planned rows can't open, and every one says why.
    #[test]
    fn planned_rows_cant_open() {
        let l = lib();
        let mut planned = 0;
        for i in 0..l.cat.rows.len() {
            let r = &l.cat.rows[i];
            if row_ready(r) {
                assert!(l.openable(i));
                assert!(l.planned_reason(i).is_none());
            } else {
                planned += 1;
                assert!(!l.openable(i), "{}", r.id);
                let why = l.planned_reason(i).unwrap();
                assert!(!why.is_empty(), "{}", r.id);
            }
        }
        assert!(planned > 0);
        let (deer, _) = l.find("deer.coat.brown").unwrap();
        assert!(l.planned_reason(deer).unwrap().contains("body-region"));
        let later = l.cat.rows.iter().position(|r| r.batch == 5).unwrap();
        assert_eq!(l.planned_reason(later).unwrap(), "Planned for batch 5: not built yet.");
        // Only ready rows are options of `cat.open`.
        assert!(!l.ready_ids().contains(&"deer.coat.brown"));
    }

    /// A row that was tried and held back (planned, with numbers kept) says
    /// its own reason: its batch's "not built yet" line, or the default, would
    /// tell the user something false (W3 guard; dragonfly.wing was the first).
    #[test]
    fn a_held_row_gives_its_own_reason() {
        let l = lib();
        let generic: Vec<String> = (1..=11).map(|b| l.planned.batch.replace("{batch}", &b.to_string())).chain([l.planned.default.clone()]).collect();
        let mut bad = Vec::new();
        for i in 0..l.cat.rows.len() {
            let r = &l.cat.rows[i];
            if row_ready(r) || !r.params.contains_key("numbers") {
                continue;
            }
            let why = l.planned_reason(i).unwrap_or_default();
            if generic.contains(&why) || l.planned.backend.values().any(|b| *b == why) {
                bad.push(r.id.clone());
            }
        }
        assert!(bad.is_empty(), "add a line for each to data/planned.json `rows`: {bad:?}");
    }

    /// Every recipe has a shape, and the scene names are the ones the app draws.
    #[test]
    fn every_recipe_has_a_shape() {
        let l = lib();
        for r in &l.cat.recipes {
            assert!(SHAPES.contains(&auto_shape(r)), "{}", r.id);
            for row in l.cat.rows.iter().filter(|w| w.recipe == r.id) {
                assert!(SHAPES.contains(&shape_for(r, row)), "{}", row.id);
                // A shape a row names is one the viewer draws (round 4), not
                // a typo that falls back to the recipe's.
                if let Some(s) = row.params.get("shape").and_then(Value::as_str) {
                    assert!(s != "auto" && SHAPES.contains(&s) && shape_for(r, row) == s, "{}: shape {s}", row.id);
                }
            }
        }
        let by = |id: &str| auto_shape(l.cat.recipes.iter().find(|r| r.id == id).unwrap());
        assert_eq!(by("fur"), "ball");
        assert_eq!(by("ground"), "tile");
        assert_eq!(by("bark"), "cylinder");
        assert_eq!(by("fire"), "card");
        assert_eq!(by("cloth"), "cushion");
        assert_eq!(by("skin"), "head");
    }

    fn app() -> evani_core::App {
        let mut a = evani_core::App::new("t");
        install(&mut a.context("material-view")).unwrap();
        a
    }

    fn set_open(a: &mut evani_core::App, id: &str) {
        let _ = a.world.res_mut::<ParamRegistry>().set_json(OPEN, &json!(id));
        a.run_headless(2, 1000.0 / 60.0);
    }

    /// Opening a fur row sets its switches to the row's values and offers
    /// only its own conditions; a slider then changes only the block.
    #[test]
    fn opening_a_row_sets_its_switches() {
        let mut a = app();
        set_open(&mut a, "horse.coat.bay");
        let lib = a.world.res::<LibraryData>().0.clone();
        let v = a.world.res::<CatalogueView>().clone();
        let (i, row) = lib.find("horse.coat.bay").unwrap();
        assert_eq!(v.open, Some(i));
        assert_eq!(v.shape, "ball");
        let block = v.block.unwrap();
        assert_eq!(block, lib.cat.encode(&v.edited.clone().unwrap()).unwrap());
        // No condition is on yet (the row has no default amounts).
        assert_eq!(block.unpack().cond_mask, 0);
        {
            let reg = a.world.res::<ParamRegistry>();
            assert!((reg.f32_or(&number_param("sheen"), 0.0) - 0.8).abs() < 1e-6);
            assert!(reg.bool_or(&extra_param("fur", "pattern")));
        }
        // Mud on: the block carries mud (the row lists it) at its amount.
        let _ = a.world.res_mut::<ParamRegistry>().set_by_name(&cond_param("mud", "on"), ParamValue::Bool(true));
        let _ = a.world.res_mut::<ParamRegistry>().set_by_name(&cond_param("mud", "amount"), ParamValue::F32(1.0));
        // Oxide on: the row doesn't list it, so nothing changes for it.
        let _ = a.world.res_mut::<ParamRegistry>().set_by_name(&cond_param("oxide", "on"), ParamValue::Bool(true));
        a.run_headless(1, 1000.0 / 60.0);
        let f = a.world.res::<CatalogueView>().block.unwrap().unpack();
        assert_eq!(f.cond_mask, 0b10, "mud only: {:08b}", f.cond_mask);
        assert_eq!(f.cond_amounts[1], 255);
        assert_eq!(row.conditions, ["wet", "mud", "dust", "snow", "blood"]);
        // A number slider moves its byte.
        let before = a.world.res::<CatalogueView>().block.unwrap().unpack().numbers;
        let _ = a.world.res_mut::<ParamRegistry>().set_by_name(&number_param("sheen"), ParamValue::F32(1.0));
        a.run_headless(1, 1000.0 / 60.0);
        let after = a.world.res::<CatalogueView>().block.unwrap().unpack().numbers;
        assert_ne!(before, after);
        assert_eq!(after[2], 255, "sheen is the fur recipe's third number");
        // `none` closes it.
        set_open(&mut a, "none");
        assert!(a.world.res::<CatalogueView>().open.is_none());
        assert!(a.world.res::<CatalogueView>().edited.is_none());
    }

    /// A family row (skin) has no block and opens on its family's head; since
    /// batch 3 cloth is drawn by the recipe stage too (its weave, pile and
    /// motifs), so velvet carries a block and still opens on its cushion.
    #[test]
    fn a_family_row_opens_on_its_family_shape() {
        let mut a = app();
        set_open(&mut a, "skin.tone.05.neutral");
        let v = a.world.res::<CatalogueView>().clone();
        assert!(v.open.is_some());
        assert!(v.block.is_none());
        assert_eq!(v.shape, "head");
        set_open(&mut a, "cloth.velvet.red");
        let v = a.world.res::<CatalogueView>().clone();
        assert!(v.open.is_some());
        assert!(v.block.is_some(), "cloth's recipe stage (batch 3)");
        assert_eq!(v.shape, "cushion");
        let _ = a.world.res_mut::<ParamRegistry>().set_json(SHAPE, &json!("ball"));
        a.run_headless(1, 1000.0 / 60.0);
        assert_eq!(a.world.res::<CatalogueView>().shape, "ball");
    }

    /// B2-P8: an alias row resolves to its base (here asphalt) under its own id,
    /// as an ordinary ready row with the alias's condition amounts laid over.
    /// No catalogue row is an alias today (black ice is held for batch 10), so
    /// the mechanism is tested on a row made here. "Black ice" is still found
    /// by search, but it is held, with its own reason.
    #[test]
    fn an_alias_opens_its_base_with_the_condition_on() {
        let l = lib();
        let (_, base) = l.find("asphalt.road").unwrap();
        let mut alias = base.clone();
        alias.id = "test.wet_road".into();
        alias.status = "alias".into();
        alias.params = json!({"alias_of": "asphalt.road"}).as_object().cloned().unwrap();
        alias.cond_defaults = json!({"wet": 1.0}).as_object().cloned().unwrap();
        assert!(row_ready(&alias), "an alias is drawable");
        let w = working_row(&l.cat, &alias);
        assert_eq!((w.id.as_str(), w.status.as_str()), ("test.wet_road", "ready"), "drawn as an ordinary row");
        assert_eq!(w.params, base.params, "the base's numbers");
        assert_eq!(w.cond_defaults["wet"], json!(1.0), "the alias's own wet amount");
        // Black ice is held, with its own reason (not a batch line).
        assert_eq!(first(&l, "black ice"), "ground.ice.sheet_road");
        let (i, ice) = l.find("ground.ice.sheet_road").unwrap();
        assert_eq!(ice.status, "planned");
        assert!(!l.openable(i));
        assert!(l.planned_reason(i).unwrap_or_default().contains("near-black"));
    }

    /// B2-P8: foliage opens on a masked card (leaf, needle spray, grass
    /// tuft) or, for turf, moss and litter, on a bank; ground on a bank and
    /// stone on a rock.
    #[test]
    fn foliage_ground_and_stone_open_on_their_own_shapes() {
        let mut a = app();
        for (id, shape) in [("leaf.oak", "oak_leaf"), ("leaf.maple.autumn", "maple_leaf"), ("leaf.birch", "leaf"), ("needle.pine", "pine"), ("needle.spruce", "spruce"), ("needle.fir", "fir"), ("crop.wheat.ripe", "wheat"), ("crop.hay", "hay"), ("ground.grass.meadow", "meadow"), ("roof.thatch", "tuft"), ("gem.quartz_clear", "crystal"), ("magic.slime", "blob"), ("ground.grass.lawn", "lawn"), ("ground.moss.carpet", "slope"), ("ground.leaf_litter", "slope"), ("ground.soil.loam", "slope"), ("rock.granite.grey", "rock"), ("flower.rose.red", "rose"), ("flower.rose.white", "rose"), ("bark.oak", "cylinder"), ("horse.hoof.dark", "cylinder"), ("quill.feather", "leaf"), ("butterfly.orange", "petal"), ("dragonfly.wing", "petal"), ("shrimp.shell", "cylinder"), ("egret.feather", "ball")] {
            set_open(&mut a, id);
            assert_eq!(a.world.res::<CatalogueView>().shape, shape, "{id}");
        }
    }
}
