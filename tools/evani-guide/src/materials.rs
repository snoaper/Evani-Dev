//! `[materials]` in `evani.toml` (material catalogue, batch 1 build plan 2.2,
//! package P10b): the recipes and condition layers a game lists, the app
//! features they turn on, and the `check` rules for the models that carry
//! catalogue materials.
//!
//! ```toml
//! [materials]
//! recipes    = ["wood", "metal", "fur", "cloth"]   # which materials the game draws
//! conditions = ["wet", "mud", "snow"]               # which weather / wear layers
//! ```
//!
//! The author thinks in recipes; the families follow:
//!
//! - a recipe the new crate serves (`wood`, `metal`, `paint`, `fur`, ...) turns
//!   on `material-recipes` and `recipe-<name>`;
//! - `cloth` turns on `material-recipes` and `recipe-cloth` only: its weaves,
//!   patterns, sheen and fuzz are all the recipe stage's since B4-P3b, so a
//!   cloth game downloads no layers family (about 7.2 KB lighter);
//! - `thin` turns on `material-recipes` and `recipe-thin` (wallpaper, batch 3)
//!   plus `material-nature` (light through paper and petals);
//! - `skin`, `hair`, `eye` turn on `char-materials`;
//! - `water`, `glass` turn on `material-nature`;
//! - `foliage` (batch 2, B2-P1) is a recipe-stage recipe: `recipe-foliage`
//!   only, nothing from the nature module (its rows still name a nature kind,
//!   which a game that also lists `water`, `glass` or `thin` draws with the
//!   placed back-light);
//! - a recipe's optional part (`wood-layouts`: wood's batch 3 layouts;
//!   `organic-food`: flesh grain, corn's kernels and the pie lattice) turns
//!   on `recipe-<part>` and its recipe, so a game whose wood is only grain and
//!   planks never downloads the layouts;
//! - `cutout` (batch 3 B3-P2: grates, chain-link, wire mesh, lattice) turns on
//!   `material-cutout` (evani-render's `material-recipes-cut`): the holes and
//!   the hole-aware shadow pipeline; a model carrying a cut-out row without it
//!   is an ERROR (it would draw solid);
//! - each condition turns on `cond-<layer>` (and `material-recipes`);
//! - a game that lists any of it also lists `models` (the materials ride in
//!   its models).
//!
//! `paint` maps to the recipe stage only: its clear coat is drawn from the
//! block (P10a), so a game that lists `paint` does not need the layers family.
//! `metal`'s brushed anisotropy is layers data and draws only in a build that
//! has the layers family (no `[materials]` name turns it on since B4-P3b);
//! without it the recipe's own streak draws.
//!
//! Everything here is a native dev tool: nothing reaches a game's download.

use std::collections::BTreeSet;
use std::path::Path;

use evani_material_recipes::block::Block;
use evani_material_recipes::catalog::Catalog;
use serde_json::Value;

use crate::check::Severity;

/// Where a recipe's materials are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The new crate's recipe stage (`recipe-<name>`).
    Recipe,
    /// The recipe stage plus the nature module (`thin`).
    RecipeNature,
    /// The layers family (no catalogue recipe needs it since B4-P3b moved
    /// cloth's sheen into the recipe stage).
    Layers,
    /// The character family (`skin`, `hair`, `eye`).
    Character,
    /// The nature module (`water`, `glass`, `thin`).
    Nature,
}

/// The 29 recipes in block-code order (code = index + 1) and their routes.
/// A test keeps this equal to `catalog/materials/recipes.json`.
pub const RECIPES: [(&str, Route); 29] = [
    ("metal", Route::Recipe),
    ("paint", Route::Recipe),
    ("plastic", Route::Recipe),
    ("wood", Route::Recipe),
    ("bark", Route::Recipe),
    ("stone", Route::Recipe),
    ("masonry", Route::Recipe),
    ("plaster", Route::Recipe),
    ("ground", Route::Recipe),
    ("cloth", Route::Recipe),
    ("leather", Route::Recipe),
    ("fur", Route::Recipe),
    ("feather", Route::Recipe),
    ("scales", Route::Recipe),
    ("hide", Route::Recipe),
    ("skin", Route::Character),
    ("hair", Route::Character),
    ("eye", Route::Character),
    ("keratin", Route::Recipe),
    ("foliage", Route::Recipe),
    ("thin", Route::RecipeNature),
    ("organic", Route::Recipe),
    ("ceramic", Route::Recipe),
    ("glass", Route::Recipe),
    ("gem", Route::Recipe),
    ("water", Route::Nature),
    ("ice", Route::Recipe),
    ("fire", Route::Recipe),
    ("glow", Route::Recipe),
];

/// The recipes' optional parts, listed in `[materials] recipes` like a recipe
/// (each implies its recipe and turns on `recipe-<part>`): the part's name,
/// its recipe, and the first variant only the part draws (without it those
/// variants draw as the recipe's stand-in).
pub const PARTS: [(&str, &str, u8); 2] = [("wood-layouts", "wood", 4), ("organic-food", "organic", 5)];

/// The part `name` (`PARTS`).
pub fn part(name: &str) -> Option<(&'static str, &'static str, u8)> {
    PARTS.iter().find(|(n, _, _)| *n == name).copied()
}

/// The cut-out (batch 3 B3-P2), listed in `[materials] recipes` like a part:
/// it implies no recipe (the cut-out rows are metal and wood) and turns on
/// [`FEATURE_CUTOUT`].
pub const CUTOUT: &str = "cutout";

/// The see-through pass (batch 3 B3-P3), listed in `[materials] recipes`
/// like the cut-out: glass, smoke, steam, fire and holograms drawn blended
/// ([`FEATURE_BLEND`]). It implies no recipe.
pub const BLEND: &str = "blend";

/// The refraction for frosted, wavy and block glass (B3-P3; implies
/// [`BLEND`]): [`FEATURE_REFRACT`].
pub const REFRACTION: &str = "refraction";

/// Names listed in `recipes` that are switches, not recipes or parts.
pub const SWITCHES: [&str; 3] = [CUTOUT, BLEND, REFRACTION];

/// The eight condition layers, in block bit order.
pub const CONDITIONS: [&str; 8] = ["wet", "mud", "dust", "snow", "wear", "oxide", "growth", "blood"];

/// The app features the umbrella and the families use (the others are
/// `recipe-<name>` and `cond-<layer>`).
pub const FEATURE_RECIPES: &str = "material-recipes";
pub const FEATURE_LAYERS: &str = "material-layers";
pub const FEATURE_CHARACTER: &str = "char-materials";
pub const FEATURE_NATURE: &str = "material-nature";
pub const FEATURE_CUTOUT: &str = "material-cutout";
pub const FEATURE_BLEND: &str = "material-blend";
pub const FEATURE_REFRACT: &str = "material-refract";

impl Route {
    /// Whether the recipe stage draws it (alone or beside a family).
    pub fn recipe_stage(self) -> bool {
        matches!(self, Route::Recipe | Route::RecipeNature)
    }
}

/// The route of recipe `name`.
pub fn route(name: &str) -> Option<Route> {
    RECIPES.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

/// The recipe name of block code `code` (1-29).
pub fn recipe_of_code(code: u8) -> Option<&'static str> {
    usize::from(code).checked_sub(1).and_then(|i| RECIPES.get(i)).map(|(n, _)| *n)
}

/// The `[materials]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listed {
    pub recipes: Vec<String>,
    pub conditions: Vec<String>,
}

impl Listed {
    /// Read the optional `[materials]` table of an `evani.toml`. Unknown
    /// recipes and conditions are kept (so `check` can name them with a fix);
    /// the wrong shape is an error.
    pub fn parse(t: &toml::Table) -> Result<Self, String> {
        let Some(v) = t.get("materials") else { return Ok(Self::default()) };
        let m = v.as_table().ok_or("evani.toml: `[materials]` must be a table")?;
        if let Some(k) = m.keys().find(|k| !matches!(k.as_str(), "recipes" | "conditions")) {
            return Err(format!("evani.toml: unknown `[materials]` field `{k}` (recipes, conditions)"));
        }
        let list = |key: &str| -> Result<Vec<String>, String> {
            let Some(v) = m.get(key) else { return Ok(Vec::new()) };
            let a = v.as_array().ok_or_else(|| format!("evani.toml: `materials.{key}` must be a list of names"))?;
            let mut out: Vec<String> = Vec::new();
            for e in a {
                let s = e.as_str().ok_or_else(|| format!("evani.toml: every `materials.{key}` entry must be a string"))?;
                if !out.iter().any(|x| x == s) {
                    out.push(s.to_string());
                }
            }
            Ok(out)
        };
        Ok(Self { recipes: list("recipes")?, conditions: list("conditions")? })
    }

    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty() && self.conditions.is_empty()
    }

    /// An error naming every unknown recipe or condition (with the valid
    /// names), for builds that can't go on with one.
    pub fn validate(&self) -> Result<(), String> {
        match self.unknown().into_iter().next() {
            Some((what, _fix)) => Err(format!("evani.toml: {what}")),
            None => Ok(()),
        }
    }

    /// `(what, fix)` for each unknown name.
    pub fn unknown(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for r in self.recipes.iter().filter(|r| route(r).is_none() && part(r).is_none() && !SWITCHES.contains(&r.as_str())) {
            out.push((
                format!("`[materials] recipes` lists `{r}`, which is not a recipe"),
                format!(
                    "Use one of: {} (or a part: {}, or `{CUTOUT}`, `{BLEND}`, `{REFRACTION}`). Fix the name in the `recipes` list of apps/<game>/evani.toml.",
                    RECIPES.map(|(n, _)| n).join(", "),
                    PARTS.map(|(n, _, _)| n).join(", ")
                ),
            ));
        }
        for c in self.conditions.iter().filter(|c| !CONDITIONS.contains(&c.as_str())) {
            out.push((
                format!("`[materials] conditions` lists `{c}`, which is not a condition layer"),
                format!("Use one of: {}. Fix the name in the `conditions` list of apps/<game>/evani.toml.", CONDITIONS.join(", ")),
            ));
        }
        out
    }

    /// The app features this list turns on (unknown names turn on nothing),
    /// without `models` (the parsers add it).
    pub fn features(&self) -> Vec<String> {
        // A part stands for its recipe as well.
        let names: Vec<&str> = self.recipes.iter().flat_map(|r| part(r).map_or([r.as_str(), ""], |(_, of, _)| [of, r.as_str()])).filter(|n| !n.is_empty()).collect();
        let routes: Vec<Route> = names.iter().filter_map(|r| route(r)).collect();
        let conds: Vec<&String> = self.conditions.iter().filter(|c| CONDITIONS.contains(&c.as_str())).collect();
        let mut f: Vec<String> = Vec::new();
        let cut = self.recipes.iter().any(|r| r == CUTOUT);
        let refract = self.recipes.iter().any(|r| r == REFRACTION);
        let blend = refract || self.recipes.iter().any(|r| r == BLEND);
        if routes.iter().any(|r| r.recipe_stage()) || !conds.is_empty() || cut || blend {
            f.push(FEATURE_RECIPES.to_string());
        }
        for r in names {
            let feature = format!("recipe-{r}");
            if (route(r).is_some_and(Route::recipe_stage) || part(r).is_some()) && !f.contains(&feature) {
                f.push(feature);
            }
        }
        for c in conds {
            f.push(format!("cond-{c}"));
        }
        if cut {
            f.push(FEATURE_CUTOUT.to_string());
        }
        if blend {
            f.push(FEATURE_BLEND.to_string());
        }
        if refract {
            f.push(FEATURE_REFRACT.to_string());
        }
        for (r, also, feature) in [(Route::Layers, Route::Layers, FEATURE_LAYERS), (Route::Character, Route::Character, FEATURE_CHARACTER), (Route::Nature, Route::RecipeNature, FEATURE_NATURE)] {
            if routes.contains(&r) || routes.contains(&also) {
                f.push(feature.to_string());
            }
        }
        f
    }

    /// Whether the list turns anything on (unknown names alone don't).
    pub fn is_active(&self) -> bool {
        !self.features().is_empty()
    }

    /// Whether recipe `name` is listed (or a part of it, which implies it).
    pub fn has_recipe(&self, name: &str) -> bool {
        self.recipes.iter().any(|r| r == name || part(r).is_some_and(|(_, of, _)| of == name))
    }
}

/// Whether `feature` is one of the features `[materials]` maps to.
pub fn is_material_feature(feature: &str) -> bool {
    matches!(feature, FEATURE_RECIPES | FEATURE_LAYERS | FEATURE_CHARACTER | FEATURE_NATURE | FEATURE_CUTOUT | FEATURE_BLEND | FEATURE_REFRACT)
        || feature.strip_prefix("recipe-").is_some_and(|n| route(n).is_some_and(Route::recipe_stage) || part(n).is_some())
        || feature.strip_prefix("cond-").is_some_and(|n| CONDITIONS.contains(&n))
}

/// What a material feature must turn on in the app crate: the Cargo feature
/// line to write and the token it has to contain.
pub fn feature_wiring(feature: &str) -> Option<(String, String)> {
    let (needs, token): (String, &str) = match feature {
        FEATURE_RECIPES => ("[\"models\", \"evani-render/material-recipes\", \"dep:evani-material-recipes\"]".into(), "evani-render/material-recipes"),
        FEATURE_LAYERS => ("[\"models\", \"evani-render/material-layers\"]".into(), "evani-render/material-layers"),
        FEATURE_CHARACTER => ("[\"models\", \"evani-render/char-materials\"]".into(), "evani-render/char-materials"),
        FEATURE_NATURE => ("[\"models\", \"dep:evani-material-nature\"]".into(), "dep:evani-material-nature"),
        FEATURE_CUTOUT => (format!("[\"{FEATURE_RECIPES}\", \"evani-render/material-recipes-cut\"]"), "evani-render/material-recipes-cut"),
        FEATURE_BLEND => (format!("[\"{FEATURE_RECIPES}\", \"evani-render/material-recipes-blend\"]"), "evani-render/material-recipes-blend"),
        FEATURE_REFRACT => (format!("[\"{FEATURE_BLEND}\", \"evani-render/material-recipes-refract\"]"), "evani-render/material-recipes-refract"),
        f if is_material_feature(f) => return Some((format!("{f} = [\"{FEATURE_RECIPES}\", \"evani-material-recipes/{f}\"]"), format!("evani-material-recipes/{f}"))),
        _ => return None,
    };
    Some((format!("{feature} = {needs}"), token.to_string()))
}

/// The optional dependency lines the features need in the app's Cargo.toml.
fn dependency_lines(features: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    if features.iter().any(|f| f == FEATURE_RECIPES) {
        out.push("evani-material-recipes = { workspace = true, optional = true }");
    }
    if features.iter().any(|f| f == FEATURE_NATURE) {
        out.push("evani-material-nature = { workspace = true, optional = true }");
    }
    out
}

/// A problem in the game's files: `(error?, what, fix)`.
pub type Problem = (Severity, String, String);

/// The app crate's `Cargo.toml` against the features `[materials]` maps to:
/// each must exist and turn on what it stands for (a `recipe-wood` that
/// doesn't reach `evani-material-recipes/recipe-wood` would list a recipe
/// the build never draws).
pub fn cargo_problems(app: &str, listed: &Listed, cargo_toml: &str) -> Vec<Problem> {
    let Ok(t) = cargo_toml.parse::<toml::Table>() else { return Vec::new() };
    let feats = t.get("features").and_then(toml::Value::as_table);
    let want = listed.features();
    let mut out = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for f in &want {
        let Some((line, token)) = feature_wiring(f) else { continue };
        match feats.and_then(|x| x.get(f.as_str())) {
            None => {
                out.push((Severity::Error, format!("evani.toml `[materials]` turns on `{f}` but the app crate has no such feature"), String::new()));
                lines.push(line);
            }
            Some(v) => {
                let has = v.as_array().is_some_and(|a| a.iter().any(|e| e.as_str() == Some(token.as_str())));
                if !has {
                    out.push((Severity::Error, format!("the app crate's `{f}` feature doesn't turn on `{token}`, so the game lists the material but the build never draws it"), String::new()));
                    lines.push(line);
                }
            }
        }
    }
    if out.is_empty() {
        return out;
    }
    let deps: Vec<&str> = dependency_lines(&want);
    let fix = format!(
        "In apps/{app}/Cargo.toml add under [features]: {}{}. (The sandbox's Cargo.toml has all of them; copy the lines you need.)",
        lines.join("; "),
        if deps.is_empty() { String::new() } else { format!(" and under [dependencies] (if missing): {}", deps.join("; ")) }
    );
    out.into_iter().map(|(s, w, _)| (s, w, fix.clone())).collect()
}

/// The materials of one model file: the pieces `check` needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelMaterial {
    /// `extras.evani_material`.
    pub id: Option<String>,
    /// `extras.evani_recipe` (the hex, as written).
    pub block: Option<String>,
    /// The material's name, for messages.
    pub name: String,
    /// `extras.evani_layers` carries a clear coat.
    pub layered_coat: bool,
}

/// The catalogue-carrying materials of a glTF document.
pub fn model_materials(doc: &Value) -> Vec<ModelMaterial> {
    let mut out = Vec::new();
    for (i, m) in doc.get("materials").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let e = m.get("extras");
        let id = e.and_then(|e| e.get("evani_material")).and_then(Value::as_str).map(str::to_string);
        let block = e.and_then(|e| e.get("evani_recipe")).and_then(Value::as_str).map(str::to_string);
        if id.is_none() && block.is_none() {
            continue;
        }
        let name = m.get("name").and_then(Value::as_str).map_or_else(|| format!("material {i}"), str::to_string);
        let layered_coat = e.and_then(|e| e.pointer("/evani_layers/clearcoat")).and_then(Value::as_f64).is_some_and(|c| c > 0.0);
        out.push(ModelMaterial { id, block, name, layered_coat });
    }
    out
}

/// The catalogue, when the engine checkout has one (`catalog/materials`).
pub fn load_catalog(root: &Path) -> Option<Catalog> {
    Catalog::load_dir(&root.join("catalog").join("materials")).ok()
}

/// Check one model file's materials against the game's list and the
/// catalogue. `file` is the path shown in messages.
pub fn model_problems(file: &str, doc: &Value, listed: &Listed, cat: Option<&Catalog>) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut need: BTreeSet<&'static str> = BTreeSet::new();
    let mut need_conds: BTreeSet<&'static str> = BTreeSet::new();
    let mut need_parts: BTreeSet<&'static str> = BTreeSet::new();
    let (mut need_cut, mut need_open) = (false, false);
    let (mut need_blend, mut need_refract) = (false, false);
    for m in model_materials(doc) {
        let who = format!("`{file}` material `{}`", m.name);
        let row = m.id.as_deref().and_then(|id| cat.and_then(|c| c.row(id)));
        if let (Some(id), Some(_)) = (&m.id, cat)
            && row.is_none()
        {
            out.push((
                Severity::Warn,
                format!("{who} names `{id}`, which is not in the material catalogue (a stale id)"),
                "Fix the id in the model or in the `--materials` map and convert the model again (`cargo xtask convert <source> --materials <map.json>`); the catalogue ids are in catalog/materials/materials.json.".to_string(),
            ));
        }
        let mut block_recipe: Option<&'static str> = None;
        if let Some(hex) = &m.block {
            match Block::from_hex(hex) {
                Err(_) => out.push((
                    Severity::Error,
                    format!("{who}: `extras.evani_recipe` is not a 64-character hex block"),
                    "Convert the model again with `cargo xtask convert <source> --materials <map.json>`; never edit the block by hand.".to_string(),
                )),
                Ok(b) => {
                    let f = b.unpack();
                    match recipe_of_code(f.recipe) {
                        None => out.push((
                            Severity::Error,
                            format!("{who}: the block's recipe code {} is not a recipe (1 to 29)", f.recipe),
                            "Convert the model again with `cargo xtask convert <source> --materials <map.json>`.".to_string(),
                        )),
                        Some(name) => {
                            block_recipe = Some(name);
                            let cut = evani_material_recipes::cut::is_cut(&b);
                            // Cloth's open weave draws its gaps dark without
                            // the cut-out: a note, not an error.
                            let open = cut && !evani_material_recipes::cut::is_cut_required(&b);
                            need_cut |= cut && !open;
                            need_open |= open;
                            let mode = evani_material_recipes::blend::mode(&b);
                            need_blend |= mode != evani_material_recipes::blend::OPAQUE;
                            need_refract |= mode == evani_material_recipes::blend::REFRACT;
                            // A cut-out variant (wood's lattice) needs the cut-out, not a part.
                            if let Some((p, _, _)) = PARTS.iter().find(|(_, of, from)| *of == name && f.variant >= *from && !cut) {
                                need_parts.insert(p);
                            }
                            for (i, c) in CONDITIONS.iter().enumerate() {
                                if f.cond_amounts[i] > 0 {
                                    need_conds.insert(c);
                                }
                            }
                            if name == "paint" && m.layered_coat {
                                out.push((
                                    Severity::Warn,
                                    format!("{who} is paint with a layered clear coat as well: paint draws its own coat from the block, so a build with the layers family would draw two"),
                                    "Convert the model again with `cargo xtask convert <source>` (it leaves the authored clear coat of a paint material out).".to_string(),
                                ));
                            }
                        }
                    }
                    if let Some(row) = row
                        && let Some(cat) = cat
                        && let Ok(want) = cat.encode(row)
                    {
                        let w = want.unpack();
                        if w.recipe != f.recipe || w.cond_mask != f.cond_mask {
                            out.push((
                                Severity::Error,
                                format!(
                                    "{who} is stale: its block says recipe {} with conditions {:08b}, but the catalogue row `{}` is recipe {} with conditions {:08b}",
                                    recipe_of_code(f.recipe).unwrap_or("?"),
                                    f.cond_mask,
                                    row.id,
                                    recipe_of_code(w.recipe).unwrap_or("?"),
                                    w.cond_mask
                                ),
                                "Convert the model again (`cargo xtask convert <source> --materials <map.json>`) so it carries the row as it is now.".to_string(),
                            ));
                        } else if want != b {
                            out.push((
                                Severity::Warn,
                                format!("{who}: the catalogue row `{}` has other numbers now than the block in the model", row.id),
                                "Convert the model again to pick up the row's current numbers, or leave it if the older look is wanted.".to_string(),
                            ));
                        }
                    }
                }
            }
        }
        // A family row (cloth, skin, ...) has no block: its recipe is the row's.
        let named = row.map(|r| r.recipe.as_str());
        if let Some(n) = block_recipe.or_else(|| named.and_then(|n| RECIPES.iter().find(|(r, _)| *r == n).map(|(r, _)| *r))) {
            need.insert(n);
        }
    }
    for n in need {
        if !listed.has_recipe(n) {
            let route = route(n);
            let how = match route {
                Some(Route::Layers) => "turns on the layers family".to_string(),
                Some(Route::RecipeNature) => format!("turns on the `{n}` recipe and the nature module"),
                Some(Route::Character) => "turns on the character materials".to_string(),
                Some(Route::Nature) => "turns on the nature module".to_string(),
                _ => format!("turns on the `{n}` recipe"),
            };
            out.push((
                Severity::Error,
                format!("`{file}` has {n} materials but evani.toml `[materials] recipes` doesn't list `{n}`, so the game would draw them as plain colours"),
                format!("Add \"{n}\" to `recipes` under [materials] in apps/<game>/evani.toml (that {how}), or convert the model with a map that gives those materials another recipe."),
            ));
        }
    }
    for p in need_parts {
        if !listed.recipes.iter().any(|r| r == p) {
            out.push((
                Severity::Error,
                format!("`{file}` has materials drawn by the `{p}` part but evani.toml `[materials] recipes` doesn't list `{p}`, so the game would draw them as plain planks"),
                format!("Add \"{p}\" to `recipes` under [materials] in apps/<game>/evani.toml (it implies its recipe), or convert the model with a map that gives those materials a row without the part."),
            ));
        }
    }
    if need_cut && !listed.recipes.iter().any(|r| r == CUTOUT) {
        out.push((
            Severity::Error,
            format!("`{file}` has cut-out materials (grate, chain-link, wire mesh or lattice) but evani.toml `[materials] recipes` doesn't list `{CUTOUT}`, so the game would draw them solid"),
            format!("Add \"{CUTOUT}\" to `recipes` under [materials] in apps/<game>/evani.toml (it turns on the holes and their shadows), or convert the model with a map that gives those materials a solid row."),
        ));
    }
    if need_open && !listed.recipes.iter().any(|r| r == CUTOUT) {
        out.push((
            Severity::Note,
            format!("`{file}` has open-weave cloth (muslin, gauze) but evani.toml `[materials] recipes` doesn't list `{CUTOUT}`, so its gaps draw dark instead of see-through"),
            format!("Add \"{CUTOUT}\" to `recipes` under [materials] in apps/<game>/evani.toml to cut the gaps, or leave it if dark gaps are fine."),
        ));
    }
    let refract = listed.recipes.iter().any(|r| r == REFRACTION);
    if need_blend && !refract && !listed.recipes.iter().any(|r| r == BLEND) {
        out.push((
            Severity::Warn,
            format!("`{file}` has see-through materials (glass, smoke, steam, fire or a hologram) but evani.toml `[materials] recipes` doesn't list `{BLEND}`, so the game draws them opaque"),
            format!("Add \"{BLEND}\" to `recipes` under [materials] in apps/<game>/evani.toml (it turns on the sorted see-through pass), or leave it if opaque glass and solid flame cards are wanted."),
        ));
    }
    if need_refract && !refract {
        out.push((
            Severity::Note,
            format!("`{file}` has frosted, wavy or block glass but evani.toml `[materials] recipes` doesn't list `{REFRACTION}`, so it is drawn as clear see-through glass without the blur and bend"),
            format!("Add \"{REFRACTION}\" to `recipes` under [materials] in apps/<game>/evani.toml (it copies the scene behind such glass, about 2.5 MB of video memory at 720p), or ignore this note."),
        ));
    }
    for c in need_conds {
        if !listed.conditions.iter().any(|x| x == c) {
            out.push((
                Severity::Note,
                format!("`{file}` has materials with a default `{c}` amount, but evani.toml `[materials] conditions` doesn't list `{c}`, so that layer is not drawn"),
                format!("Add \"{c}\" to `conditions` under [materials] in apps/<game>/evani.toml if the default should show, or ignore this note."),
            ));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn listed(s: &str) -> Listed {
        let t: toml::Table = format!("name = \"g\"\n{s}").parse().unwrap();
        Listed::parse(&t).unwrap()
    }

    fn catalog() -> Catalog {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        load_catalog(&root).unwrap()
    }

    #[test]
    fn the_route_table_is_the_catalogue() {
        let cat = catalog();
        assert_eq!(cat.recipes.len(), RECIPES.len());
        for (i, r) in cat.recipes.iter().enumerate() {
            let (name, route) = RECIPES[i];
            assert_eq!(usize::from(r.code), i + 1);
            assert_eq!(r.id, name, "recipe {} of the catalogue", i + 1);
            let also = |f: &str| r.also.iter().any(|a| a == f);
            // Cloth's sheen and fuzz are its recipe's since B4-P3b (no
            // family); metal's anisotropy is an optional layers extra, so
            // metal is the recipe stage alone too.
            assert!(!(r.id == "cloth" && also("layers")), "cloth needs no layers family");
            let want = if r.backend == "recipes" && also("nature") && r.id != "foliage" && r.id != "glass" {
                // Thin needs the nature module (light through paper); foliage's
                // placed back-light is optional (B2-P1) and so is glass's
                // placed refraction (B3-P3: the see-through pass draws it), so
                // both are the recipe stage alone.
                Route::RecipeNature
            } else if r.backend == "recipes" || also("recipes") {
                Route::Recipe
            } else {
                match r.backend.as_str() {
                    "layers" => Route::Layers,
                    "character" => Route::Character,
                    "nature" => Route::Nature,
                    other => panic!("backend {other}"),
                }
            };
            assert_eq!(route, want, "route of {name}");
        }
        // The condition order is the block's bit order.
        assert_eq!(CONDITIONS, evani_material_recipes::block::CONDITION_NAMES);
    }

    #[test]
    fn recipes_map_to_features() {
        let l = listed("[materials]\nrecipes = [\"wood\", \"metal\", \"fur\", \"cloth\"]\nconditions = [\"wet\", \"mud\", \"snow\"]\n");
        assert_eq!(l.features(), ["material-recipes", "recipe-wood", "recipe-metal", "recipe-fur", "recipe-cloth", "cond-wet", "cond-mud", "cond-snow"]);
        // Cloth alone: its recipe stage only (weaves, patterns, sheen and
        // fuzz, B4-P3b); thin: its stage (wallpaper) and the nature module
        // (light through paper).
        let c = listed("[materials]\nrecipes = [\"cloth\"]\n");
        assert_eq!(c.features(), ["material-recipes", "recipe-cloth"]);
        assert_eq!(listed("[materials]\nrecipes = [\"thin\"]\n").features(), ["material-recipes", "recipe-thin", "material-nature"]);
        assert!(is_material_feature("recipe-thin") && is_material_feature("recipe-cloth") && !is_material_feature("recipe-skin"));
        // Paint is the recipe stage only (its coat is the block's).
        assert_eq!(listed("[materials]\nrecipes = [\"paint\"]\n").features(), ["material-recipes", "recipe-paint"]);
        // The families.
        assert_eq!(listed("[materials]\nrecipes = [\"skin\", \"eye\", \"water\"]\n").features(), ["char-materials", "material-nature"]);
        // Foliage is the recipe stage's (B2-P1): nothing from nature.
        assert_eq!(listed("[materials]\nrecipes = [\"foliage\"]\n").features(), ["material-recipes", "recipe-foliage"]);
        // Conditions alone still need the recipe stage.
        assert_eq!(listed("[materials]\nconditions = [\"wet\"]\n").features(), ["material-recipes", "cond-wet"]);
        // Nothing listed: nothing on. Duplicates collapse.
        assert!(listed("").features().is_empty());
        assert_eq!(listed("[materials]\nrecipes = [\"wood\", \"wood\"]\n").features(), ["material-recipes", "recipe-wood"]);
        // A part implies its recipe, once.
        let p = listed("[materials]\nrecipes = [\"wood-layouts\"]\n");
        assert_eq!(p.features(), ["material-recipes", "recipe-wood", "recipe-wood-layouts"]);
        assert!(p.has_recipe("wood") && p.unknown().is_empty() && is_material_feature("recipe-wood-layouts"));
        assert_eq!(listed("[materials]\nrecipes = [\"wood\", \"wood-layouts\"]\n").features(), ["material-recipes", "recipe-wood", "recipe-wood-layouts"]);
    }

    #[test]
    fn the_table_must_be_well_formed() {
        let bad = |s: &str| {
            let t: toml::Table = format!("name = \"g\"\n{s}").parse().unwrap();
            Listed::parse(&t)
        };
        assert!(bad("materials = 1").is_err());
        assert!(bad("[materials]\nrecipes = \"wood\"").is_err());
        assert!(bad("[materials]\nrecipes = [1]").is_err());
        assert!(bad("[materials]\nstyles = []").is_err_and(|e| e.contains("styles")));
    }

    #[test]
    fn unknown_names_get_a_fix() {
        let l = listed("[materials]\nrecipes = [\"woood\", \"wood\"]\nconditions = [\"snowy\"]\n");
        let u = l.unknown();
        assert_eq!(u.len(), 2);
        assert!(u[0].0.contains("`woood`") && u[0].1.contains("wood") && u[0].1.contains("cloth"));
        assert!(u[1].0.contains("`snowy`") && u[1].1.contains("snow"));
        assert!(l.validate().is_err_and(|e| e.contains("woood")));
        // Unknown names turn on nothing.
        assert_eq!(l.features(), ["material-recipes", "recipe-wood"]);
    }

    #[test]
    fn the_app_crate_must_wire_each_feature() {
        let l = listed("[materials]\nrecipes = [\"wood\", \"cloth\", \"skin\"]\n");
        let good = "[features]\nmaterial-recipes = [\"models\", \"evani-render/material-recipes\", \"dep:evani-material-recipes\"]\nrecipe-wood = [\"material-recipes\", \"evani-material-recipes/recipe-wood\"]\nrecipe-cloth = [\"material-recipes\", \"evani-material-recipes/recipe-cloth\"]\nchar-materials = [\"models\", \"evani-render/char-materials\"]\n";
        assert!(cargo_problems("g", &l, good).is_empty());
        // Missing feature: named, with the exact line to add.
        let p = cargo_problems("g", &l, "[features]\nmaterial-recipes = [\"evani-render/material-recipes\"]\nrecipe-cloth = [\"material-recipes\", \"evani-material-recipes/recipe-cloth\"]\nchar-materials = [\"evani-render/char-materials\"]\n");
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].1.contains("`recipe-wood`") && p[0].2.contains("recipe-wood = [\"material-recipes\", \"evani-material-recipes/recipe-wood\"]"), "{p:?}");
        assert!(p[0].2.contains("evani-material-recipes = { workspace = true, optional = true }"));
        // Present but wired to nothing.
        let p = cargo_problems("g", &l, &good.replace(", \"evani-material-recipes/recipe-wood\"", ""));
        assert_eq!(p.len(), 1);
        assert!(p[0].1.contains("doesn't turn on `evani-material-recipes/recipe-wood`"), "{p:?}");
        // The family: the character feature missing.
        let p = cargo_problems("g", &l, &good.replace("char-materials = [\"models\", \"evani-render/char-materials\"]\n", ""));
        assert_eq!(p.len(), 1);
        assert!(p[0].1.contains("`char-materials`") && p[0].2.contains("evani-render/char-materials"));
    }

    /// The template app (the sandbox; `new-game` copies its Cargo.toml) has a
    /// wired feature for every recipe, condition and family `[materials]` can
    /// name, so a game can list any of them.
    #[test]
    fn the_template_app_wires_every_material_feature() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cargo = std::fs::read_to_string(root.join("apps/sandbox/Cargo.toml")).unwrap();
        let all = Listed {
            recipes: RECIPES.iter().map(|(n, _)| (*n).to_string()).chain(PARTS.iter().map(|(n, _, _)| (*n).to_string())).chain([CUTOUT.to_string()]).collect(),
            conditions: CONDITIONS.iter().map(|c| (*c).to_string()).collect(),
        };
        assert!(all.unknown().is_empty());
        let p = cargo_problems("sandbox", &all, &cargo);
        assert!(p.is_empty(), "{p:?}");
        // 25 recipe features (cloth, thin and glass among them, batch 3;
        // foliage, batch 2), two parts (wood's layouts, organic's food), 8
        // layers, the umbrella, two families (character, nature; no catalogue
        // recipe needs the layers family since B4-P3b) and the cut-out.
        assert_eq!(all.features().len(), 25 + 2 + 8 + 1 + 2 + 1);
    }

    /// B3-P2: `cutout` turns on the cut-out feature (and the recipe stage),
    /// implies no recipe, and a model with a cut-out row but no `cutout` is
    /// an ERROR with a fix; wood's lattice needs the cut-out, not the layouts.
    #[test]
    fn blend_and_refraction_are_listed_and_checked() {
        let l = listed("[materials]\nrecipes = [\"glass\", \"refraction\"]\n");
        assert_eq!(l.features(), ["material-recipes", "recipe-glass", "material-blend", "material-refract"]);
        assert!(l.unknown().is_empty() && is_material_feature("material-blend") && is_material_feature("material-refract"));
        assert_eq!(listed("[materials]\nrecipes = [\"fire\", \"blend\"]\n").features(), ["material-recipes", "recipe-fire", "material-blend"]);
        let (line, token) = feature_wiring("material-refract").unwrap();
        assert!(line.contains("material-blend") && token == "evani-render/material-recipes-refract");
        let cat = Catalog::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials")).unwrap();
        let doc_of = |id: &str| {
            let b = cat.encode(cat.row(id).unwrap()).unwrap();
            serde_json::json!({"materials": [{"name": "m", "extras": {"evani_material": id, "evani_recipe": b.to_hex()}}]})
        };
        let p = model_problems("a.glb", &doc_of("glass.window.frosted"), &listed("[materials]\nrecipes = [\"glass\"]\n"), Some(&cat));
        assert!(p.iter().any(|x| x.0 == Severity::Warn && x.2.contains("Add \"blend\"")), "{p:?}");
        assert!(p.iter().any(|x| x.0 == Severity::Note && x.2.contains("Add \"refraction\"")), "{p:?}");
        let ok = model_problems("a.glb", &doc_of("glass.window.frosted"), &listed("[materials]\nrecipes = [\"glass\", \"refraction\"]\n"), Some(&cat));
        assert!(ok.is_empty(), "{ok:?}");
        let flame = model_problems("a.glb", &doc_of("fire.flame.wood"), &listed("[materials]\nrecipes = [\"fire\", \"blend\"]\n"), Some(&cat));
        assert!(flame.is_empty(), "{flame:?}");
    }

    #[test]
    fn cutout_is_listed_and_checked() {
        let l = listed("[materials]\nrecipes = [\"metal\", \"cutout\"]\n");
        assert_eq!(l.features(), ["material-recipes", "recipe-metal", "material-cutout"]);
        assert!(l.unknown().is_empty() && !l.has_recipe("wood") && is_material_feature("material-cutout"));
        assert_eq!(listed("[materials]\nrecipes = [\"cutout\"]\n").features(), ["material-recipes", "material-cutout"]);
        let (line, token) = feature_wiring("material-cutout").unwrap();
        assert!(line.contains("evani-render/material-recipes-cut") && token == "evani-render/material-recipes-cut");
        let cat = catalog();
        for id in ["build.grate", "build.chain_link", "build.mesh", "wood.lattice"] {
            let row = cat.row(id).unwrap();
            let b = cat.encode(row).unwrap();
            assert!(evani_material_recipes::cut::is_cut(&b), "{id}");
            let d = serde_json::json!({"materials": [{"name": "m", "extras": {"evani_material": id, "evani_recipe": b.to_hex()}}]});
            let recipe = row.recipe.clone();
            let p = model_problems("a.glb", &d, &listed(&format!("[materials]\nrecipes = [\"{recipe}\"]\n")), Some(&cat));
            assert!(p.len() == 1 && p[0].0 == Severity::Error && p[0].2.contains("Add \"cutout\""), "{id}: {p:?}");
            let ok = model_problems("a.glb", &d, &listed(&format!("[materials]\nrecipes = [\"{recipe}\", \"cutout\"]\n")), Some(&cat));
            assert!(ok.is_empty(), "{id}: {ok:?}");
        }
    }

    /// B2-P1: a game listing only `foliage` (with rain and snow) resolves,
    /// for the web, to the recipe stage with `recipe-foliage` and without
    /// evani-material-nature (`cargo tree` on the template app with exactly
    /// the features the list maps to; resolution only, no build).
    #[test]
    fn foliage_alone_reaches_no_nature_module() {
        let l = listed("[materials]\nrecipes = [\"foliage\"]\nconditions = [\"wet\", \"snow\"]\n");
        let mut feats = l.features();
        assert_eq!(feats, ["material-recipes", "recipe-foliage", "cond-wet", "cond-snow"]);
        feats.push("models".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = std::process::Command::new(cargo)
            .current_dir(&root)
            .args(["tree", "--offline", "-p", "evani-sandbox", "--no-default-features", "--features", &feats.join(","), "-e", "normal", "--prefix", "none", "--target", "wasm32-unknown-unknown"])
            .output()
            .unwrap();
        let tree = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "cargo tree: {}", String::from_utf8_lossy(&out.stderr));
        assert!(tree.lines().any(|l| l.starts_with("evani-material-recipes ")), "{tree}");
        assert!(!tree.contains("evani-material-nature"), "a foliage-only game must not download the nature module:\n{tree}");
    }

    /// B4-P3b: a game listing only `cloth` resolves, for the web, to the
    /// recipe stage with `recipe-cloth` and without evani-material-layers
    /// (its sheen and fuzz are the recipe's now; `cargo tree` on the template
    /// app, resolution only).
    #[test]
    fn cloth_alone_reaches_no_layers_family() {
        let l = listed("[materials]\nrecipes = [\"cloth\"]\nconditions = [\"wet\"]\n");
        let mut feats = l.features();
        assert_eq!(feats, ["material-recipes", "recipe-cloth", "cond-wet"]);
        feats.push("models".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = std::process::Command::new(cargo)
            .current_dir(&root)
            .args(["tree", "--offline", "-p", "evani-sandbox", "--no-default-features", "--features", &feats.join(","), "-e", "normal", "--prefix", "none", "--format", "{p} [{f}]", "--target", "wasm32-unknown-unknown"])
            .output()
            .unwrap();
        let tree = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "cargo tree: {}", String::from_utf8_lossy(&out.stderr));
        // Each line is a package and its enabled features.
        let line = |p: &str| tree.lines().find(|l| l.starts_with(p)).unwrap_or_else(|| panic!("no {p}:\n{tree}"));
        assert!(line("evani-material-recipes ").contains("recipe-cloth"), "{tree}");
        assert!(!tree.contains("evani-material-layers"), "a cloth-only game must not download the layers family:\n{tree}");
        assert!(!line("evani-render ").contains("material-layers"), "evani-render's layered pass must stay off:\n{tree}");
    }

    /// B4-P3a: cloth's open weave (muslin) cuts its gaps with the cut-out
    /// but draws without it (dark gaps), so a missing `cutout` is a note.
    #[test]
    fn open_weave_cloth_wants_the_cutout_but_draws_without() {
        let cat = catalog();
        let b = cat.encode(cat.row("cloth.muslin").unwrap()).unwrap();
        assert!(evani_material_recipes::cut::is_cut(&b) && !evani_material_recipes::cut::is_cut_required(&b));
        let d = serde_json::json!({"materials": [{"name": "m", "extras": {"evani_material": "cloth.muslin", "evani_recipe": b.to_hex()}}]});
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"cloth\"]\n"), Some(&cat));
        assert!(p.len() == 1 && p[0].0 == Severity::Note && p[0].2.contains("Add \"cutout\""), "{p:?}");
        assert!(model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"cloth\", \"cutout\"]\n"), Some(&cat)).is_empty());
    }

    fn doc(extras: &str) -> Value {
        serde_json::from_str(&format!("{{\"materials\":[{{\"name\":\"m\",\"extras\":{extras}}}]}}")).unwrap()
    }

    fn block_of(cat: &Catalog, id: &str) -> String {
        cat.encode(cat.row(id).unwrap()).unwrap().to_hex()
    }

    fn first_ready(cat: &Catalog, recipe: &str) -> String {
        cat.rows.iter().find(|r| r.recipe == recipe && r.status == "ready").map(|r| r.id.clone()).unwrap_or_else(|| panic!("no ready {recipe} row"))
    }

    #[test]
    fn a_model_needs_its_recipe_listed() {
        let cat = catalog();
        let id = first_ready(&cat, "wood");
        let d = doc(&format!("{{\"evani_material\":\"{id}\",\"evani_recipe\":\"{}\"}}", block_of(&cat, &id)));
        // Not listed: an error with the fix.
        let p = model_problems("a.glb", &d, &Listed::default(), Some(&cat));
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].0, Severity::Error);
        assert!(p[0].1.contains("wood") && p[0].1.contains("doesn't list `wood`") && p[0].2.contains("Add \"wood\""), "{p:?}");
        // Listed: clean.
        assert!(model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\"]\n"), Some(&cat)).is_empty());
        // Without the catalogue it is still found, from the block's code.
        let p = model_problems("a.glb", &d, &Listed::default(), None);
        assert_eq!(p.len(), 1);
        assert!(p[0].1.contains("wood"));
        // A model with no catalogue materials is untouched.
        assert!(model_problems("a.glb", &serde_json::json!({"materials":[{"name":"x"}]}), &Listed::default(), Some(&cat)).is_empty());
    }

    #[test]
    fn a_wood_layout_needs_its_part_listed() {
        let cat = catalog();
        let variant = |id: &str| cat.encode(cat.row(id).unwrap()).unwrap().unpack().variant;
        let ready = |pick: &dyn Fn(u8) -> bool| cat.rows.iter().find(|r| r.recipe == "wood" && r.status == "ready" && pick(variant(&r.id))).map(|r| r.id.clone()).unwrap();
        let lay = ready(&|v| v >= 4);
        let plain = ready(&|v| v < 4);
        let d = doc(&format!("{{\"evani_material\":\"{lay}\",\"evani_recipe\":\"{}\"}}", block_of(&cat, &lay)));
        // `wood` alone: the layout would draw as planks, an error with the fix.
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\"]\n"), Some(&cat));
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].0 == Severity::Error && p[0].1.contains("`wood-layouts`") && p[0].2.contains("Add \"wood-layouts\""), "{p:?}");
        // The part listed (it implies wood): clean.
        assert!(model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood-layouts\"]\n"), Some(&cat)).is_empty());
        // Grain and planks need only wood.
        let d = doc(&format!("{{\"evani_material\":\"{plain}\",\"evani_recipe\":\"{}\"}}", block_of(&cat, &plain)));
        assert!(model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\"]\n"), Some(&cat)).is_empty());
    }

    #[test]
    fn a_family_row_needs_its_family_listed() {
        let cat = catalog();
        // A skin material named without a block (cloth was the example
        // until B4-P3b made it a recipe alone): the recipe comes from the row.
        let id = cat.rows.iter().find(|r| r.recipe == "skin").unwrap().id.clone();
        let d = doc(&format!("{{\"evani_material\":\"{id}\"}}"));
        let p = model_problems("a.glb", &d, &Listed::default(), Some(&cat));
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].1.contains("skin") && p[0].2.contains("character materials"), "{p:?}");
        assert!(model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"skin\"]\n"), Some(&cat)).is_empty());
    }

    #[test]
    fn a_stale_block_is_an_error() {
        let cat = catalog();
        let wood = first_ready(&cat, "wood");
        let stone = first_ready(&cat, "stone");
        // The model says it is the wood row but carries the stone block.
        let d = doc(&format!("{{\"evani_material\":\"{wood}\",\"evani_recipe\":\"{}\"}}", block_of(&cat, &stone)));
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\", \"stone\"]\n"), Some(&cat));
        let e: Vec<&Problem> = p.iter().filter(|p| p.0 == Severity::Error).collect();
        assert_eq!(e.len(), 1, "{p:?}");
        assert!(e[0].1.contains("stale") && e[0].1.contains(&wood) && e[0].2.contains("--materials"), "{e:?}");
        // The block's recipe (stone) is what must be listed.
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\"]\n"), Some(&cat));
        assert!(p.iter().any(|p| p.1.contains("doesn't list `stone`")), "{p:?}");
    }

    #[test]
    fn changed_numbers_are_a_warning_and_a_stale_id_too() {
        let cat = catalog();
        let id = first_ready(&cat, "wood");
        let mut b = Block::from_hex(&block_of(&cat, &id)).unwrap();
        b.0[13] = b.0[13].wrapping_add(1); // a number moved: same recipe and conditions
        let d = doc(&format!("{{\"evani_material\":\"{id}\",\"evani_recipe\":\"{}\"}}", b.to_hex()));
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\"]\n"), Some(&cat));
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].0, Severity::Warn);
        assert!(p[0].1.contains("other numbers"));
        // A stale id is a warning.
        let d = doc("{\"evani_material\":\"wood.nope.nothing\"}");
        let p = model_problems("a.glb", &d, &Listed::default(), Some(&cat));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].0, Severity::Warn);
        assert!(p[0].1.contains("stale id") && p[0].2.contains("--materials"));
    }

    #[test]
    fn a_bad_block_is_an_error() {
        let p = model_problems("a.glb", &doc("{\"evani_recipe\":\"abc\"}"), &Listed::default(), None);
        assert_eq!(p.len(), 1);
        assert!(p[0].1.contains("64-character hex"));
        let zero = "0".repeat(64);
        let p = model_problems("a.glb", &doc(&format!("{{\"evani_recipe\":\"{zero}\"}}")), &Listed::default(), None);
        assert!(p.iter().any(|p| p.1.contains("recipe code 0")), "{p:?}");
    }

    #[test]
    fn default_amounts_of_unlisted_layers_are_a_note() {
        let cat = catalog();
        let Some(row) = cat.rows.iter().find(|r| r.status == "ready" && r.recipe == "wood" && cat.encode(r).is_ok_and(|b| b.unpack().cond_amounts.iter().any(|a| *a > 0))) else {
            return; // no ready wood row carries a default amount yet (P11 adds them)
        };
        let d = doc(&format!("{{\"evani_material\":\"{}\",\"evani_recipe\":\"{}\"}}", row.id, block_of(&cat, &row.id)));
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"wood\"]\n"), Some(&cat));
        assert!(p.iter().all(|p| p.0 == Severity::Note), "{p:?}");
        assert!(!p.is_empty());
    }

    #[test]
    fn paint_with_a_second_coat_is_a_warning() {
        let cat = catalog();
        let Some(row) = cat.rows.iter().find(|r| r.recipe == "paint") else { return };
        let d = doc(&format!("{{\"evani_material\":\"{}\",\"evani_recipe\":\"{}\",\"evani_layers\":{{\"clearcoat\":1.0}}}}", row.id, block_of(&cat, &row.id)));
        let p = model_problems("a.glb", &d, &listed("[materials]\nrecipes = [\"paint\"]\n"), Some(&cat));
        assert!(p.iter().any(|p| p.0 == Severity::Warn && p.1.contains("two")), "{p:?}");
    }
}
