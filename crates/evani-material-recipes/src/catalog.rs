//! The catalogue reader (feature `catalog`, dev only).
//!
//! Reads `catalog/materials/recipes.json` (the 29 recipes: colours, numbers with
//! ranges and defaults, variants, extras masks, allowed conditions) and
//! `catalog/materials/materials.json` (all 1,054 rows), checks them against each
//! other, and encodes any row to its 32-byte block. Nothing here ships in a game:
//! a game gets only the blocks, inside its models.
//!
//! Failures are values: a missing recipe, an unknown variant or a condition the
//! recipe doesn't allow is a [`CatalogError`], never a panic.

use crate::block::{quantise, seed_byte, Block, Fields, COLOUR_COUNT, CONDITION_COUNT, CONDITION_NAMES, NUMBER_COUNT};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;

/// Everything that can be wrong with the catalogue data or one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// The text is not JSON.
    Json(String),
    /// A recipe or row field is missing or has the wrong type or range.
    Spec {
        /// Where (a recipe id, a row id or a file part).
        at: String,
        /// What is wrong.
        problem: String,
    },
    /// A row names a recipe that `recipes.json` doesn't have.
    MissingRecipe {
        /// The row id.
        row: String,
        /// The recipe it asked for.
        recipe: String,
    },
    /// Two rows share one id.
    DuplicateId(String),
    /// A row lists a condition that is not one of the 8 layers.
    UnknownCondition {
        /// The row id.
        row: String,
        /// The condition name.
        name: String,
    },
    /// A row lists a condition its recipe doesn't allow.
    ConditionNotAllowed {
        /// The row id.
        row: String,
        /// The recipe.
        recipe: String,
        /// The condition.
        condition: String,
    },
    /// A row's `params` or `cond_defaults` can't be applied to its recipe.
    BadParams {
        /// The row id.
        row: String,
        /// What is wrong.
        problem: String,
    },
    /// Reading a file failed.
    Io(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Json(e) => write!(f, "catalogue is not valid JSON: {e}"),
            CatalogError::Spec { at, problem } => write!(f, "{at}: {problem}"),
            CatalogError::MissingRecipe { row, recipe } => write!(f, "row {row}: there is no recipe \"{recipe}\" in recipes.json"),
            CatalogError::DuplicateId(id) => write!(f, "duplicate material id {id}"),
            CatalogError::UnknownCondition { row, name } => write!(f, "row {row}: \"{name}\" is not a condition layer"),
            CatalogError::ConditionNotAllowed { row, recipe, condition } => write!(f, "row {row}: recipe {recipe} does not allow the {condition} condition"),
            CatalogError::BadParams { row, problem } => write!(f, "row {row}: {problem}"),
            CatalogError::Io(e) => write!(f, "cannot read catalogue file: {e}"),
        }
    }
}

impl std::error::Error for CatalogError {}

fn spec(at: &str, problem: impl Into<String>) -> CatalogError {
    CatalogError::Spec { at: at.to_string(), problem: problem.into() }
}

/// One recipe number: its name, range and default.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberSpec {
    /// Snake-case name, unique inside the recipe.
    pub name: String,
    /// Value stored as 0.
    pub min: f32,
    /// Value stored as 255.
    pub max: f32,
    /// Value a row gets when it doesn't set one.
    pub default: f32,
}

/// One recipe extra: a switch with a bit in the extras byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraSpec {
    /// Name, unique inside the recipe.
    pub name: String,
    /// The bit (a power of two).
    pub mask: u8,
    /// On for a row that doesn't list extras.
    pub default: bool,
}

/// One named colour slot with its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColourSpec {
    /// Slot name.
    pub name: String,
    /// Default sRGB8.
    pub rgb: [u8; 3],
}

/// One of the 29 recipes.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    /// Block byte 0 (1-29).
    pub code: u8,
    /// Recipe id used by rows.
    pub id: String,
    /// `recipes`, `layers`, `character` or `nature`.
    pub backend: String,
    /// Other backends that serve part of it.
    pub also: Vec<String>,
    /// Colour slots A, B, C (1 to 3 of them).
    pub colours: Vec<ColourSpec>,
    /// Numbers n0.. (up to 10).
    pub numbers: Vec<NumberSpec>,
    /// Pattern kinds; the block stores the index.
    pub variants: Vec<String>,
    /// Extras with their bits.
    pub extras: Vec<ExtraSpec>,
    /// Allowed conditions as a mask in block bit order.
    pub conditions: u8,
    /// Quality groups.
    pub groups: Vec<String>,
    /// What the light version draws.
    pub light: String,
    /// Byte budget class.
    pub class: String,
    /// Viewer scene (empty until the viewer package fills it).
    pub scene: String,
}

/// `catalog/materials/recipes.json`, embedded (feature `data`, dev only).
#[cfg(feature = "data")]
pub const RECIPES_JSON: &str = include_str!("../../../catalog/materials/recipes.json");
/// `catalog/materials/materials.json`, embedded (feature `data`, dev only).
/// The studio's reader (evani-studio) takes this same copy, so the viewer
/// carries the text once.
#[cfg(feature = "data")]
pub const MATERIALS_JSON: &str = include_str!("../../../catalog/materials/materials.json");

/// One catalogue row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Unique id, for example `horse.coat.bay`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Category.
    pub category: String,
    /// Subcategory.
    pub subcategory: String,
    /// Recipe id.
    pub recipe: String,
    /// Search words (one string, as in the CSV).
    pub search_words: String,
    /// What it should look like.
    pub look_note: String,
    /// Build batch 1-11.
    pub batch: u8,
    /// `""` or `yes:<family>`.
    pub exists_today: String,
    /// Condition names, in CSV order.
    pub conditions: Vec<String>,
    /// Overrides on the recipe defaults: `variant`, `colours`, `numbers`,
    /// `extras`; and `shape`, the material viewer's test shape (not encoded).
    pub params: Map<String, Value>,
    /// Default amounts per condition (0..1); missing = 0.
    pub cond_defaults: Map<String, Value>,
    /// `planned`, `ready`, `reviewed` or `alias` (B2: the row is another
    /// ready row with its own condition amounts; `params` holds only
    /// `alias_of`, see [`Catalog::resolved`]).
    pub status: String,
}

/// The two data files, parsed.
#[derive(Clone, Debug)]
pub struct Catalog {
    /// The recipes, in code order.
    pub recipes: Vec<Recipe>,
    /// The rows, in file order.
    pub rows: Vec<Row>,
}

fn get<'a>(o: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a Value, CatalogError> {
    o.get(key).ok_or_else(|| spec(at, format!("missing \"{key}\"")))
}

fn get_str(o: &Map<String, Value>, key: &str, at: &str) -> Result<String, CatalogError> {
    get(o, key, at)?.as_str().map(str::to_string).ok_or_else(|| spec(at, format!("\"{key}\" must be a string")))
}

fn get_arr<'a>(o: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a Vec<Value>, CatalogError> {
    get(o, key, at)?.as_array().ok_or_else(|| spec(at, format!("\"{key}\" must be an array")))
}

fn get_obj<'a>(o: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a Map<String, Value>, CatalogError> {
    get(o, key, at)?.as_object().ok_or_else(|| spec(at, format!("\"{key}\" must be an object")))
}

fn str_list(v: &[Value], at: &str, what: &str) -> Result<Vec<String>, CatalogError> {
    v.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| spec(at, format!("{what} must hold strings")))).collect()
}

fn num(v: &Value, at: &str, what: &str) -> Result<f32, CatalogError> {
    match v.as_f64() {
        Some(x) if x.is_finite() => Ok(x as f32),
        _ => Err(spec(at, format!("{what} must be a finite number"))),
    }
}

/// Parse `rrggbb` or `#rrggbb` to sRGB8.
pub fn parse_hex_colour(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

/// The bit of a condition name in the block's condition mask.
pub fn condition_bit(name: &str) -> Option<usize> {
    CONDITION_NAMES.iter().position(|n| *n == name)
}

fn parse_recipe(v: &Value) -> Result<Recipe, CatalogError> {
    let o = v.as_object().ok_or_else(|| spec("recipes", "each recipe must be an object"))?;
    let id = get_str(o, "id", "recipes")?;
    let at = id.as_str();
    let code = get(o, "code", at)?.as_u64().and_then(|c| u8::try_from(c).ok()).filter(|c| *c >= 1).ok_or_else(|| spec(at, "code must be 1-255"))?;

    let colours = get_arr(o, "colours", at)?
        .iter()
        .map(|c| {
            let co = c.as_object().ok_or_else(|| spec(at, "a colour must be an object"))?;
            let rgb = parse_hex_colour(&get_str(co, "hex", at)?).ok_or_else(|| spec(at, "colour hex must be rrggbb"))?;
            Ok(ColourSpec { name: get_str(co, "name", at)?, rgb })
        })
        .collect::<Result<Vec<_>, CatalogError>>()?;
    if colours.is_empty() || colours.len() > COLOUR_COUNT {
        return Err(spec(at, "a recipe has 1 to 3 colours"));
    }

    let numbers = get_arr(o, "numbers", at)?
        .iter()
        .map(|n| {
            let no = n.as_object().ok_or_else(|| spec(at, "a number must be an object"))?;
            let name = get_str(no, "name", at)?;
            let (min, max, default) = (num(get(no, "min", at)?, at, "min")?, num(get(no, "max", at)?, at, "max")?, num(get(no, "default", at)?, at, "default")?);
            if max <= min {
                return Err(spec(at, format!("number {name}: max must be above min")));
            }
            if default < min || default > max {
                return Err(spec(at, format!("number {name}: default is outside its range")));
            }
            Ok(NumberSpec { name, min, max, default })
        })
        .collect::<Result<Vec<_>, CatalogError>>()?;
    if numbers.len() > NUMBER_COUNT {
        return Err(spec(at, "a recipe has at most 10 numbers"));
    }

    let variants = str_list(get_arr(o, "variants", at)?, at, "variants")?;
    if variants.is_empty() || variants.len() > 255 {
        return Err(spec(at, "a recipe has 1 to 255 variants"));
    }

    let extras = get_arr(o, "extras", at)?
        .iter()
        .map(|x| {
            let xo = x.as_object().ok_or_else(|| spec(at, "an extra must be an object"))?;
            let mask = get(xo, "mask", at)?.as_u64().and_then(|m| u8::try_from(m).ok()).filter(|m| m.is_power_of_two()).ok_or_else(|| spec(at, "an extra mask must be a power of two below 256"))?;
            let default = get(xo, "default", at)?.as_bool().ok_or_else(|| spec(at, "extra default must be true or false"))?;
            Ok(ExtraSpec { name: get_str(xo, "name", at)?, mask, default })
        })
        .collect::<Result<Vec<_>, CatalogError>>()?;
    let mut seen = 0u8;
    for x in &extras {
        if seen & x.mask != 0 {
            return Err(spec(at, format!("extra {} reuses a mask bit", x.name)));
        }
        seen |= x.mask;
    }

    let mut conditions = 0u8;
    for c in str_list(get_arr(o, "conditions", at)?, at, "conditions")? {
        let bit = condition_bit(&c).ok_or_else(|| spec(at, format!("\"{c}\" is not a condition layer")))?;
        conditions |= 1 << bit;
    }

    Ok(Recipe {
        code,
        backend: get_str(o, "backend", at)?,
        also: str_list(get_arr(o, "also", at)?, at, "also")?,
        colours,
        numbers,
        variants,
        extras,
        conditions,
        groups: str_list(get_arr(o, "groups", at)?, at, "groups")?,
        light: get_str(o, "light", at)?,
        class: get_str(o, "class", at)?,
        scene: get_str(o, "scene", at)?,
        id,
    })
}

fn parse_row(v: &Value) -> Result<Row, CatalogError> {
    let o = v.as_object().ok_or_else(|| spec("rows", "each row must be an object"))?;
    let id = get_str(o, "id", "rows")?;
    let at = id.as_str();
    let batch = get(o, "batch", at)?.as_u64().and_then(|b| u8::try_from(b).ok()).filter(|b| (1..=11).contains(b)).ok_or_else(|| spec(at, "batch must be 1-11"))?;
    let status = get_str(o, "status", at)?;
    if !matches!(status.as_str(), "planned" | "ready" | "reviewed" | "alias") {
        return Err(spec(at, "status must be planned, ready, reviewed or alias"));
    }
    Ok(Row {
        name: get_str(o, "name", at)?,
        category: get_str(o, "category", at)?,
        subcategory: get_str(o, "subcategory", at)?,
        recipe: get_str(o, "recipe", at)?,
        search_words: get_str(o, "search_words", at)?,
        look_note: get_str(o, "look_note", at)?,
        batch,
        exists_today: get_str(o, "exists_today", at)?,
        conditions: str_list(get_arr(o, "conditions", at)?, at, "conditions")?,
        params: get_obj(o, "params", at)?.clone(),
        cond_defaults: get_obj(o, "cond_defaults", at)?.clone(),
        status,
        id,
    })
}

impl Catalog {
    /// Parse the two files' text. Checks each part on its own (see
    /// [`Catalog::validate`] for the cross-checks).
    pub fn parse(recipes_json: &str, materials_json: &str) -> Result<Catalog, CatalogError> {
        let r: Value = serde_json::from_str(recipes_json).map_err(|e| CatalogError::Json(e.to_string()))?;
        let m: Value = serde_json::from_str(materials_json).map_err(|e| CatalogError::Json(e.to_string()))?;
        let ro = r.as_object().ok_or_else(|| spec("recipes.json", "must be an object"))?;
        let mo = m.as_object().ok_or_else(|| spec("materials.json", "must be an object"))?;

        let listed = str_list(get_arr(ro, "conditions", "recipes.json")?, "recipes.json", "conditions")?;
        if listed.len() != CONDITION_COUNT || listed.iter().zip(CONDITION_NAMES).any(|(a, b)| a != b) {
            return Err(spec("recipes.json", "conditions must be the 8 layers in block bit order"));
        }
        if get(ro, "block_bytes", "recipes.json")?.as_u64() != Some(crate::block::BLOCK_BYTES as u64) {
            return Err(spec("recipes.json", "block_bytes must be 32"));
        }

        let recipes = get_arr(ro, "recipes", "recipes.json")?.iter().map(parse_recipe).collect::<Result<Vec<_>, _>>()?;
        for (i, rec) in recipes.iter().enumerate() {
            if usize::from(rec.code) != i + 1 {
                return Err(spec(&rec.id, "recipe codes must run 1, 2, 3 ... in file order"));
            }
            if recipes[..i].iter().any(|p| p.id == rec.id) {
                return Err(spec(&rec.id, "duplicate recipe id"));
            }
        }
        let rows = get_arr(mo, "rows", "materials.json")?.iter().map(parse_row).collect::<Result<Vec<_>, _>>()?;
        Ok(Catalog { recipes, rows })
    }

    /// Read `recipes.json` and `materials.json` from a folder (normally
    /// `catalog/materials`).
    pub fn load_dir(dir: &std::path::Path) -> Result<Catalog, CatalogError> {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).map_err(|e| CatalogError::Io(format!("{name}: {e}")));
        Catalog::parse(&read("recipes.json")?, &read("materials.json")?)
    }

    /// The catalogue embedded at build time (feature `data`, dev only).
    #[cfg(feature = "data")]
    pub fn bundled() -> Result<Catalog, CatalogError> {
        Catalog::parse(RECIPES_JSON, MATERIALS_JSON)
    }

    /// A recipe by id; a missing one is an error naming the row.
    pub fn recipe_for(&self, row: &Row) -> Result<&Recipe, CatalogError> {
        self.recipes.iter().find(|r| r.id == row.recipe).ok_or_else(|| CatalogError::MissingRecipe { row: row.id.clone(), recipe: row.recipe.clone() })
    }

    /// A row by id.
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// Every id appears once.
    pub fn check_unique_ids(&self) -> Result<(), CatalogError> {
        let mut seen = std::collections::BTreeSet::new();
        for r in &self.rows {
            if !seen.insert(r.id.as_str()) {
                return Err(CatalogError::DuplicateId(r.id.clone()));
            }
        }
        Ok(())
    }

    /// Every row's conditions are real layers its recipe allows.
    pub fn check_conditions(&self) -> Result<(), CatalogError> {
        for r in &self.rows {
            self.condition_mask(r)?;
        }
        Ok(())
    }

    /// All the data's own checks: unique ids, recipes present, conditions
    /// allowed, every alias resolves, and every row encodes.
    pub fn validate(&self) -> Result<(), CatalogError> {
        self.check_unique_ids()?;
        for r in &self.rows {
            self.resolved(r)?;
            self.encode(r)?;
        }
        Ok(())
    }

    /// The row whose numbers draw `row`: the row itself, or for an `alias`
    /// row (B2: weather and wear stay condition layers, so "black ice" is
    /// asphalt with the wet layer on) the base row it names, under the alias's
    /// own id, name and search words, with the alias's `cond_defaults` laid
    /// over the base's. An alias must name a `ready` or `reviewed` row that is
    /// not itself an alias (no chains), carry nothing in `params` but
    /// `alias_of`, and share its base's recipe and conditions.
    pub fn resolved<'a>(&'a self, row: &'a Row) -> Result<std::borrow::Cow<'a, Row>, CatalogError> {
        use std::borrow::Cow;
        if row.status != "alias" {
            return Ok(Cow::Borrowed(row));
        }
        let bad = |problem: String| CatalogError::BadParams { row: row.id.clone(), problem };
        let name = row.params.get("alias_of").and_then(Value::as_str).ok_or_else(|| bad("an alias needs params.alias_of".into()))?;
        if row.params.len() != 1 {
            return Err(bad("an alias has no numbers of its own: params holds only alias_of".into()));
        }
        let base = self.row(name).ok_or_else(|| bad(format!("alias_of \"{name}\" is not a catalogue id")))?;
        if base.status != "ready" && base.status != "reviewed" {
            return Err(bad(format!("alias_of \"{name}\" must be a ready row (it is {})", base.status)));
        }
        if base.recipe != row.recipe || base.conditions != row.conditions {
            return Err(bad(format!("an alias shares the recipe and conditions of \"{name}\"")));
        }
        let mut r = base.clone();
        r.id.clone_from(&row.id);
        r.name.clone_from(&row.name);
        r.search_words.clone_from(&row.search_words);
        r.look_note.clone_from(&row.look_note);
        r.status.clone_from(&row.status);
        for (k, v) in &row.cond_defaults {
            r.cond_defaults.insert(k.clone(), v.clone());
        }
        Ok(Cow::Owned(r))
    }

    /// The row's condition mask, checked against its recipe.
    pub fn condition_mask(&self, row: &Row) -> Result<u8, CatalogError> {
        let recipe = self.recipe_for(row)?;
        let mut mask = 0u8;
        for c in &row.conditions {
            let bit = condition_bit(c).ok_or_else(|| CatalogError::UnknownCondition { row: row.id.clone(), name: c.clone() })?;
            if recipe.conditions & (1 << bit) == 0 {
                return Err(CatalogError::ConditionNotAllowed { row: row.id.clone(), recipe: recipe.id.clone(), condition: c.clone() });
            }
            mask |= 1 << bit;
        }
        Ok(mask)
    }

    /// Encode a row to its 32-byte block: the recipe's defaults, then the row's
    /// `params` and `cond_defaults` on top.
    pub fn encode(&self, row: &Row) -> Result<Block, CatalogError> {
        let row = &*self.resolved(row)?;
        let recipe = self.recipe_for(row)?;
        let bad = |problem: String| CatalogError::BadParams { row: row.id.clone(), problem };
        let cond_mask = self.condition_mask(row)?;

        let mut f = Fields { recipe: recipe.code, cond_mask, seed: seed_byte(&row.id), ..Fields::default() };
        for (i, c) in recipe.colours.iter().enumerate() {
            f.colours[i] = c.rgb;
        }
        for (i, n) in recipe.numbers.iter().enumerate() {
            f.numbers[i] = quantise(n.default, n.min, n.max);
        }
        f.extras = recipe.extras.iter().filter(|x| x.default).fold(0, |m, x| m | x.mask);

        for (key, val) in &row.params {
            match key.as_str() {
                "variant" => {
                    let name = val.as_str().ok_or_else(|| bad("variant must be a string".into()))?;
                    let idx = recipe.variants.iter().position(|v| v == name).ok_or_else(|| bad(format!("recipe {} has no variant \"{name}\"", recipe.id)))?;
                    f.variant = u8::try_from(idx).map_err(|_| bad("variant index too large".into()))?;
                }
                "colours" => {
                    let list = val.as_array().ok_or_else(|| bad("colours must be an array".into()))?;
                    if list.len() > recipe.colours.len() {
                        return Err(bad(format!("recipe {} has {} colours", recipe.id, recipe.colours.len())));
                    }
                    for (i, c) in list.iter().enumerate() {
                        if c.is_null() {
                            continue;
                        }
                        let hex = c.as_str().and_then(parse_hex_colour).ok_or_else(|| bad("a colour must be rrggbb or null".into()))?;
                        f.colours[i] = hex;
                    }
                }
                "numbers" => {
                    let obj = val.as_object().ok_or_else(|| bad("numbers must be an object".into()))?;
                    for (name, v) in obj {
                        let idx = recipe.numbers.iter().position(|n| &n.name == name).ok_or_else(|| bad(format!("recipe {} has no number \"{name}\"", recipe.id)))?;
                        let x = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(format!("number {name} must be a finite number")))? as f32;
                        let n = &recipe.numbers[idx];
                        if x < n.min || x > n.max {
                            return Err(bad(format!("number {name} is outside {}..{}", n.min, n.max)));
                        }
                        f.numbers[idx] = quantise(x, n.min, n.max);
                    }
                }
                "extras" => {
                    let list = val.as_array().ok_or_else(|| bad("extras must be an array".into()))?;
                    let mut mask = 0u8;
                    for x in list {
                        let name = x.as_str().ok_or_else(|| bad("an extra must be a string".into()))?;
                        let e = recipe.extras.iter().find(|e| e.name == name).ok_or_else(|| bad(format!("recipe {} has no extra \"{name}\"", recipe.id)))?;
                        mask |= e.mask;
                    }
                    f.extras = mask;
                }
                // The test shape the material viewer opens the row on
                // (material-view round 4: an oak leaf, a quartz point, a
                // rose). A viewer hint, not in the block, so no game reads it;
                // the viewer's tests hold every name to a shape it draws.
                "shape" => {
                    if !val.as_str().is_some_and(|s| !s.is_empty()) {
                        return Err(bad("shape must be a shape name".into()));
                    }
                }
                // Which of the asset's `tone: points` regions wear the row's
                // points colour (studio material pass, horse.coat.palomino:
                // a white mane and tail on gold legs). Left out: every one. A
                // studio hint, not in the block; the studio's tests hold every
                // name to a points region of the row's asset.
                "points_on" => {
                    let list = val.as_array().filter(|l| !l.is_empty()).ok_or_else(|| bad("points_on must be a list of region names".into()))?;
                    if !list.iter().all(|x| x.as_str().is_some_and(|s| !s.is_empty())) {
                        return Err(bad("points_on must be a list of region names".into()));
                    }
                }
                // The colour the row's points pull toward, in place of its
                // last colour (studio material pass, horse.coat.red_roan: a
                // roan's legs, mane and tail are solid dark chestnut, not
                // white). A studio hint, not in the block.
                "points_to" => {
                    if !val.as_str().is_some_and(|s| parse_hex_colour(s).is_some()) {
                        return Err(bad("points_to must be a hex colour like \"4a1a10\"".into()));
                    }
                }
                other => return Err(bad(format!("unknown param \"{other}\""))),
            }
        }

        for (name, v) in &row.cond_defaults {
            let bit = condition_bit(name).ok_or_else(|| CatalogError::UnknownCondition { row: row.id.clone(), name: name.clone() })?;
            let x = v.as_f64().filter(|x| (0.0..=1.0).contains(x)).ok_or_else(|| bad(format!("{name} amount must be 0 to 1")))? as f32;
            if x > 0.0 && cond_mask & (1 << bit) == 0 {
                return Err(bad(format!("{name} amount is set but the row does not list {name}")));
            }
            f.cond_amounts[bit] = quantise(x, 0.0, 1.0);
        }
        Ok(Block::pack(&f))
    }

    /// Row counts keyed by whatever `key` picks (category, batch, recipe ...).
    pub fn count_by<K: Ord>(&self, key: impl Fn(&Row) -> K) -> BTreeMap<K, usize> {
        let mut m = BTreeMap::new();
        for r in &self.rows {
            *m.entry(key(r)).or_insert(0) += 1;
        }
        m
    }
}
