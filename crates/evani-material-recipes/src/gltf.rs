//! Row to glTF (feature `gltf`, dev and tools only; batch 1 build plan 2.2
//! and package P10a): turn any catalogue row into the glTF material that
//! games and the viewer both draw, and put it on a model.
//!
//! How a model's material reaches its 32-byte block:
//!
//! - **By name:** a glTF material whose `extras.evani_material` is a
//!   catalogue id (set in Blender, or by `cargo xtask convert --materials`'s
//!   map) is filled in by [`apply_catalog`], which writes everything below
//!   into it. This is the route for authored models.
//! - **By value:** [`gltf_material`] returns the material object for one row;
//!   [`apply`] merges it into an existing material (its textures, name and
//!   other fields kept). Code-built meshes (the viewer's shapes, the sandbox
//!   proof) use this.
//!
//! What [`gltf_material`] writes:
//!
//! - always: the plain PBR factors (colour A in linear, the recipe's `rough`
//!   and `metal` numbers when it has them), what draws when no family or
//!   recipe stage is compiled in; and `extras.evani_material` (the id);
//! - recipe-backed rows (backend `recipes`, or `recipes` in `also`, so paint
//!   too): `extras.evani_recipe`, the block as 64 hex characters, which
//!   evani-render's `material-recipes` reads into binding 12;
//! - layers rows (metal with anisotropy): the `KHR_materials_anisotropy`
//!   extension for outside tools, plus the same numbers as
//!   `extras.evani_layers`, which evani-render's `material-layers` reads at
//!   load (the converter writes those from the extensions; a code-built mesh
//!   skips the converter, so the adapter writes both, with the same values);
//! - character rows (skin, hair, eye): `extras.evani_kind` and the kind's own
//!   record (`evani_skin`, `evani_hair`, `evani_eye`), read by
//!   evani-material-character's `record::apply`;
//! - nature rows (foliage, thin, glass, water): `extras.evani_kind` (the
//!   nature pass's surface kind) and the transmission extensions the nature
//!   pass falls back on (`KHR_materials_transmission` + `KHR_materials_ior`
//!   for glass and water, `KHR_materials_diffuse_transmission` for foliage
//!   and thin). Thin's wallpaper is a wall, not a light-through sheet: it
//!   gets only its block (batch 3). Foliage (batch 2, B2-P1) is drawn by the
//!   recipe stage, so its rows carry the block as well: a game with only
//!   `recipe-foliage` draws the block, one that also has the nature module
//!   keeps its placed back-light from the kind;
//! - cloth carries its block only: since B4-P3b the recipe stage draws all of
//!   it (weaves, sett, slubs, motifs, and the sheen and fuzz after lighting),
//!   so no `KHR_materials_sheen`, `evani_layers` or `evani_cloth` is written
//!   (a build with the layers family would draw the sheen twice).
//!
//! **Paint's clear coat is not a layers field.** The recipe stage draws it
//! from the block's `coat` and `coat_rough` numbers (`rc_paint_coat`), so a
//! game listing paint needs no layers family and a live block edit moves the
//! coat too. The adapter therefore writes no `KHR_materials_clearcoat` for
//! paint (the converter would turn it into a second, layered coat).
//!
//! Each family's numbers are the row's own block values, decoded back over
//! each number's range ([`RowValues`]), so the family data and the block can
//! never disagree. The field mappings follow each family's own parser
//! (evani-material-layers `parse.rs`, evani-material-character `record.rs`,
//! evani-material-nature `gltf.rs`); `tests/gltf.rs` runs those parsers on
//! the written JSON.

use crate::block::{Block, COLOUR_COUNT};
use crate::catalog::{Catalog, CatalogError, Recipe, Row};
use serde_json::{json, Map, Value};

/// The extras key naming the catalogue row.
pub const MATERIAL_KEY: &str = "evani_material";
/// The extras key of the block (64 hex characters).
pub const RECIPE_KEY: &str = "evani_recipe";

/// The extras keys the adapter owns: removed from a material before a row is
/// applied, so a material that changes family keeps nothing stale.
pub const OWNED_EXTRAS: [&str; 8] = [
    RECIPE_KEY,
    "evani_layers",
    "evani_cloth",
    "evani_kind",
    "evani_skin",
    "evani_hair",
    "evani_eye",
    MATERIAL_KEY,
];

/// The glTF extensions the adapter owns (removed before a row is applied).
pub const OWNED_EXTENSIONS: [&str; 6] = [
    "KHR_materials_sheen",
    "KHR_materials_clearcoat",
    "KHR_materials_anisotropy",
    "KHR_materials_transmission",
    "KHR_materials_ior",
    "KHR_materials_diffuse_transmission",
];

/// A row's values as the block carries them: its recipe, block, variant name,
/// colours (linear) and numbers (decoded over their ranges, so exactly what
/// the shader sees), and its extras mask.
#[derive(Clone, Debug)]
pub struct RowValues<'a> {
    /// The row's recipe.
    pub recipe: &'a Recipe,
    /// The row's 32-byte block.
    pub block: Block,
    /// The variant name.
    pub variant: &'a str,
    /// Colours A, B and C in linear (unused ones are black).
    pub colours: [[f32; 3]; COLOUR_COUNT],
    /// The numbers, in the recipe's order.
    pub numbers: Vec<f32>,
    /// The extras mask.
    pub extras: u8,
}

impl RowValues<'_> {
    /// Number `name` (as the block carries it), or `None` if the recipe has
    /// no such number.
    pub fn number(&self, name: &str) -> Option<f32> {
        self.recipe.numbers.iter().position(|n| n.name == name).and_then(|i| self.numbers.get(i).copied())
    }

    /// Number `name`, or `or` if the recipe has none.
    pub fn num(&self, name: &str, or: f32) -> f32 {
        self.number(name).unwrap_or(or)
    }

    /// Whether extra `name` is on for this row (a recipe without that extra:
    /// on).
    pub fn extra(&self, name: &str) -> bool {
        self.recipe.extras.iter().find(|x| x.name == name).is_none_or(|x| self.extras & x.mask != 0)
    }
}

/// sRGB8 to linear.
pub fn linear(c: u8) -> f32 {
    let s = f32::from(c) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// `x` rounded to 4 decimals, so the written JSON is short and stable.
pub fn r4(x: f32) -> f64 {
    (f64::from(x) * 10_000.0).round() / 10_000.0
}

fn rgb(c: [f32; 3]) -> Value {
    json!([r4(c[0]), r4(c[1]), r4(c[2])])
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// The row's block and its values decoded back.
pub fn row_values<'a>(cat: &'a Catalog, row: &'a Row) -> Result<RowValues<'a>, CatalogError> {
    let recipe = cat.recipe_for(row)?;
    let block = cat.encode(row)?;
    let f = block.unpack();
    let variant = recipe.variants.get(usize::from(f.variant)).map_or("", String::as_str);
    let colours = f.colours.map(|c| c.map(linear));
    let numbers = recipe
        .numbers
        .iter()
        .zip(f.numbers)
        .map(|(n, q)| n.min + f32::from(q) / 255.0 * (n.max - n.min))
        .collect();
    Ok(RowValues { recipe, block, variant, colours, numbers, extras: f.extras })
}

/// Whether the row's recipe draws through the recipe stage (writes a block).
pub fn has_block(recipe: &Recipe) -> bool {
    recipe.backend == "recipes" || recipe.also.iter().any(|a| a == "recipes")
}

/// The glTF material object for `row` (see the module docs for what it
/// holds). Its `name` is the row id.
pub fn gltf_material(cat: &Catalog, row: &Row) -> Result<Value, CatalogError> {
    let v = row_values(cat, row)?;
    let mut base = v.colours[0];
    let mut rough = v.num("rough", 0.5);
    let metal = if v.recipe.id == "metal" { v.num("metal", 1.0) } else { 0.0 };
    let mut extras = Map::new();
    let mut ext = Map::new();
    extras.insert(MATERIAL_KEY.into(), json!(row.id));
    if has_block(v.recipe) {
        extras.insert(RECIPE_KEY.into(), json!(v.block.to_hex()));
    }
    match v.recipe.id.as_str() {
        "metal" => {
            let a = v.num("aniso", 0.0);
            if a > 0.0 {
                ext.insert("KHR_materials_anisotropy".into(), json!({"anisotropyStrength": r4(a), "anisotropyRotation": 0.0}));
                extras.insert("evani_layers".into(), json!({"anisotropy": {"strength": r4(a), "rotation": 0.0}}));
            }
        }
        "skin" => {
            // Redness warms the tone towards the trait colour (the family has
            // no redness field); freckles have no field at all.
            base = mix3(base, v.colours[2], 0.35 * v.num("redness", 0.0));
            extras.insert("evani_kind".into(), json!("skin"));
            extras.insert(
                "evani_skin".into(),
                json!({
                    "scatter_color": rgb(v.colours[1]),
                    "wrinkle_strength": r4(2.0 * v.num("wrinkles", 0.0)),
                    "pore_scale": r4(v.num("pore_scale", 120.0)),
                    "spots": r4(v.num("freckles", 0.0)),
                }),
            );
        }
        "hair" => {
            // The root colour has no family field: it darkens the base.
            base = mix3(base, v.colours[1], 0.5 * v.num("root_dark", 0.0));
            extras.insert("evani_kind".into(), json!("hair"));
            extras.insert(
                "evani_hair".into(),
                json!({
                    "highlight_color": rgb(mix3(v.colours[0], [1.0; 3], 0.6 * v.num("highlight_1", 0.4))),
                    "shift": r4(2.0 * v.num("shift", 0.5) - 1.0),
                }),
            );
        }
        "eye" => {
            // The material colour is the white of the eye (the family's live
            // default) for builds without the character family.
            base = [0.80, 0.76, 0.72];
            rough = 0.05 + 0.4 * (1.0 - v.num("wetness", 0.7));
            extras.insert("evani_kind".into(), json!("eye"));
            extras.insert(
                "evani_eye".into(),
                json!({"iris_color": rgb(v.colours[0]), "inner_color": rgb(v.colours[1]), "pupil_size": r4(v.num("pupil", 0.4).clamp(0.05, 0.9))}),
            );
        }
        // Thin's printed and openwork variants (wallpaper, newsprint, lace)
        // are the recipe stage's alone, not light-through sheets.
        "thin" if matches!(v.variant, "wallpaper" | "newsprint" | "lace") => {}
        "foliage" | "thin" => {
            let kind = nature_kind(&v.recipe.id, v.variant);
            let through = if v.recipe.id == "foliage" { v.colours[2] } else { v.colours[1] };
            extras.insert("evani_kind".into(), json!(kind));
            ext.insert(
                "KHR_materials_diffuse_transmission".into(),
                json!({"diffuseTransmissionFactor": r4((1.0 - v.num("thickness", 0.3)).max(0.05)), "diffuseTransmissionColorFactor": rgb(through)}),
            );
        }
        "glass" | "water" => {
            let water = v.recipe.id == "water";
            let frost = v.num("frost", 0.0);
            rough = if water { 0.05 } else { 0.05 + 0.6 * frost };
            extras.insert("evani_kind".into(), json!(nature_kind(&v.recipe.id, v.variant)));
            ext.insert("KHR_materials_transmission".into(), json!({"transmissionFactor": r4(1.0 - 0.5 * frost)}));
            ext.insert("KHR_materials_ior".into(), json!({"ior": if water { 1.33 } else { 1.5 }}));
        }
        _ => {}
    }
    let mut m = Map::new();
    m.insert("name".into(), json!(row.id));
    m.insert(
        "pbrMetallicRoughness".into(),
        json!({"baseColorFactor": [r4(base[0]), r4(base[1]), r4(base[2]), 1.0], "roughnessFactor": r4(rough.clamp(0.0, 1.0)), "metallicFactor": r4(metal.clamp(0.0, 1.0))}),
    );
    if has_block(v.recipe) && crate::cut::is_cut(&v.block) {
        // A cut-out (B3-P2): seen from both sides, masked (its alpha stays
        // 1; the holes come from the block, so a reader without the recipes
        // draws it solid).
        m.insert("alphaMode".into(), json!("MASK"));
        m.insert("alphaCutoff".into(), json!(0.5));
        m.insert("doubleSided".into(), json!(true));
    }
    if has_block(v.recipe) && crate::blend::mode(&v.block) != crate::blend::OPAQUE {
        // See-through (B3-P3): blended, its alpha from the recipe (base alpha
        // stays 1, so a reader without the recipes draws it solid). Panes and
        // cards are seen from both sides; closed glassware and holograms are
        // not (their far side would blend over their near side unsorted).
        m.insert("alphaMode".into(), json!("BLEND"));
        if matches!((v.recipe.id.as_str(), v.variant), ("glass", "window" | "stained" | "leaded" | "wired" | "block") | ("fire", _)) {
            m.insert("doubleSided".into(), json!(true));
        }
    }
    if !ext.is_empty() {
        m.insert("extensions".into(), Value::Object(ext));
    }
    m.insert("extras".into(), Value::Object(extras));
    Ok(Value::Object(m))
}

/// The nature pass's surface kind name for a recipe variant (names
/// evani-material-nature's `Kind::from_name` reads).
pub fn nature_kind(recipe: &str, variant: &str) -> &'static str {
    match (recipe, variant) {
        ("foliage", "leaf") => "leaf",
        ("foliage", "grass" | "turf") => "grass",
        ("foliage", _) => "foliage",
        ("thin", "paper") => "paper",
        ("thin", "wax") => "wax",
        ("thin", "petal") => "petal",
        ("thin", "lampshade") => "lampshade",
        ("thin", _) => "thin",
        ("glass", "bottle" | "crystal") => "glass_bottle",
        ("glass", _) => "glass_pane",
        ("water", "pool") => "pool",
        ("water", "sea") => "water_rect",
        _ => "water",
    }
}

/// One live value for a `material.nature.*` param (colours linear).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NatureValue {
    F32(f32),
    Colour([f32; 3]),
}

/// The `material.nature.*` params a nature row sets (full param names).
///
/// The nature pass keeps one look table for the whole scene (a surface only
/// has a kind and a place), so a nature row cannot colour one pond differently
/// from another through the model. What it can do is set that table: a kit
/// with one body of water, one kind of foliage or one window applies the
/// row's values to the live params, and the viewer does it for the row it
/// opens. Each number maps to the param that means the same thing (ranges are
/// the params' own, the values are clamped into them):
///
/// - water: colour to `water.deep_color` (the light the water lets through),
///   deep colour to `water.scatter_color`; clarity (metres) falls with
///   `depth_fade` (`0.2 * 100^(1 - depth_fade)`) and with `murk`; waves from
///   `wave_size`; foam; a murky pond reflects less and glints softer;
/// - glass: tint, thickness (metres), frost to roughness and a weaker
///   reflection, waviness to how far a bottle bends the view;
/// - foliage: the back-light colour to `foliage.color` (leaf and needle) or
///   `grass.color` (grass only; straw is a crown), variation, glow strength and how fast a
///   crown swallows light from `thickness`;
/// - thin: the light-through colour to `thin.color` (a petal: `foliage.color`),
///   thickness (metres) and glow strength.
pub fn nature_params(cat: &Catalog, row: &Row) -> Result<Vec<(&'static str, NatureValue)>, CatalogError> {
    use NatureValue::{Colour, F32};
    let v = row_values(cat, row)?;
    let n = |name: &str, or: f32| v.num(name, or);
    let c = |i: usize| Colour(v.colours[i]);
    let f = |x: f32, lo: f32, hi: f32| F32(x.clamp(lo, hi));
    Ok(match v.recipe.id.as_str() {
        "water" => {
            let (fade, murk, size, foam) = (n("depth_fade", 0.5), n("murk", 0.2), n("wave_size", 0.3), n("foam", 0.0));
            vec![
                ("material.nature.water.deep_color", c(0)),
                ("material.nature.water.scatter_color", c(1)),
                ("material.nature.water.clarity", f(0.2 * 100f32.powf(1.0 - fade) / (1.0 + 3.0 * murk), 0.05, 50.0)),
                ("material.nature.water.wave_strength", f(0.02 + 0.3 * size, 0.0, 1.0)),
                ("material.nature.water.wave_scale", f(3.0 - 2.5 * size, 0.05, 8.0)),
                ("material.nature.water.foam", f(1.5 * foam, 0.0, 2.0)),
                ("material.nature.water.foam_width", f(0.1 + 0.8 * foam, 0.0, 3.0)),
                ("material.nature.water.reflection", f(1.0 - 0.4 * murk, 0.0, 2.0)),
                ("material.nature.water.roughness", f(0.05 + 0.15 * murk, 0.0, 1.0)),
            ]
        }
        "glass" => {
            let (thick, frost, wavy) = (n("thickness", 0.3), n("frost", 0.0), n("waviness", 0.0));
            vec![
                ("material.nature.glass.tint", c(0)),
                ("material.nature.glass.thickness", f(0.003 + 0.05 * thick, 0.001, 0.2)),
                ("material.nature.glass.roughness", f(0.03 + 0.4 * frost, 0.0, 1.0)),
                ("material.nature.glass.reflection", f(1.0 - 0.5 * frost, 0.0, 2.0)),
                ("material.nature.glass.refraction", f(1.0 + 2.0 * wavy, 0.0, 4.0)),
            ]
        }
        "foliage" => {
            let (var, thick) = (n("variation", 0.3), n("thickness", 0.3));
            let grass = v.variant == "grass";
            let (glow, vary, strength) = if grass {
                ("material.nature.grass.color", "material.nature.grass.variation", "material.nature.grass.strength")
            } else {
                ("material.nature.foliage.color", "material.nature.foliage.variation", "material.nature.foliage.strength")
            };
            let mut out = vec![(glow, c(2)), (vary, f(var, 0.0, 1.0)), (strength, f(2.0 * (1.0 - thick), 0.0, 4.0))];
            if !grass {
                out.push(("material.nature.foliage.absorb", f(0.2 + 5.0 * thick, 0.0, 20.0)));
            }
            out
        }
        // A petal is the pass's leaf kind, lit with the foliage colour.
        "thin" if v.variant == "petal" => {
            let thick = n("thickness", 0.3);
            vec![("material.nature.foliage.color", c(1)), ("material.nature.foliage.strength", f(2.0 * (1.0 - thick), 0.0, 4.0))]
        }
        "thin" => {
            let thick = n("thickness", 0.3);
            vec![
                ("material.nature.thin.color", c(1)),
                ("material.nature.thin.thickness", f(0.001 + 0.03 * thick, 0.0005, 0.2)),
                ("material.nature.thin.strength", f(2.0 * (1.0 - thick), 0.0, 4.0)),
            ]
        }
        _ => Vec::new(),
    })
}

/// Merge a written material (from [`gltf_material`]) into `material`: the
/// adapter's own extras and extensions are replaced, its PBR factors set,
/// and everything else (textures, name, alpha mode, other extras) kept. A
/// `material` that is not an object becomes one.
pub fn apply(material: &mut Value, written: &Value) {
    if !material.is_object() {
        *material = Value::Object(Map::new());
    }
    let Some(m) = material.as_object_mut() else { return };
    let extras = m.entry("extras").or_insert_with(|| Value::Object(Map::new()));
    if !extras.is_object() {
        *extras = Value::Object(Map::new());
    }
    if let Some(e) = extras.as_object_mut() {
        for k in OWNED_EXTRAS {
            e.remove(k);
        }
        if let Some(w) = written.get("extras").and_then(Value::as_object) {
            e.extend(w.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    let mut ext = m.remove("extensions").and_then(|e| match e {
        Value::Object(o) => Some(o),
        _ => None,
    });
    if let Some(e) = ext.as_mut() {
        for k in OWNED_EXTENSIONS {
            e.remove(k);
        }
    }
    if let Some(w) = written.get("extensions").and_then(Value::as_object) {
        ext.get_or_insert_with(Map::new).extend(w.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if let Some(e) = ext.filter(|e| !e.is_empty()) {
        m.insert("extensions".into(), Value::Object(e));
    }
    // A cut-out row brings its mask and both sides (B3-P2).
    for k in ["alphaMode", "alphaCutoff", "doubleSided"] {
        if let Some(v) = written.get(k) {
            m.insert(k.into(), v.clone());
        }
    }
    let pbr = m.entry("pbrMetallicRoughness").or_insert_with(|| Value::Object(Map::new()));
    if !pbr.is_object() {
        *pbr = Value::Object(Map::new());
    }
    if let (Some(p), Some(w)) = (pbr.as_object_mut(), written.get("pbrMetallicRoughness").and_then(Value::as_object)) {
        p.extend(w.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

/// Keep the document's `extensionsUsed` list in step with the extensions
/// its materials carry (adds what is missing; never removes).
pub fn note_extensions(doc: &mut Value) {
    let mut used: Vec<String> = Vec::new();
    for m in doc.get("materials").and_then(Value::as_array).into_iter().flatten() {
        for k in m.get("extensions").and_then(Value::as_object).into_iter().flat_map(Map::keys) {
            if !used.contains(k) {
                used.push(k.clone());
            }
        }
    }
    if used.is_empty() {
        return;
    }
    let Some(o) = doc.as_object_mut() else { return };
    let list = o.entry("extensionsUsed").or_insert_with(|| json!([]));
    if let Some(a) = list.as_array_mut() {
        for k in used {
            if !a.iter().any(|x| x.as_str() == Some(k.as_str())) {
                a.push(json!(k));
            }
        }
    }
}

/// Fill every material of a glTF document that names a catalogue row
/// (`extras.evani_material`) with that row's material ([`apply`]), and note
/// the extensions. Returns the (material index, id) pairs it filled. An id
/// the catalogue doesn't have is an error naming it (nothing is changed).
pub fn apply_catalog(doc: &mut Value, cat: &Catalog) -> Result<Vec<(usize, String)>, CatalogError> {
    let mut todo = Vec::new();
    for (i, m) in doc.get("materials").and_then(Value::as_array).into_iter().flatten().enumerate() {
        if let Some(id) = m.pointer("/extras/evani_material").and_then(Value::as_str) {
            let row = cat.row(id).ok_or_else(|| CatalogError::Spec { at: format!("material {i}"), problem: format!("\"{id}\" is not a catalogue id") })?;
            todo.push((i, id.to_string(), gltf_material(cat, row)?));
        }
    }
    let mut done = Vec::with_capacity(todo.len());
    for (i, id, written) in todo {
        if let Some(m) = doc.get_mut("materials").and_then(|m| m.get_mut(i)) {
            apply(m, &written);
            done.push((i, id));
        }
    }
    note_extensions(doc);
    Ok(done)
}

/// A `.glb` split into its JSON document and its binary chunk (`None` when it
/// has none). A JSON text (`.gltf`) comes back with no binary chunk.
pub fn glb_parts(bytes: &[u8]) -> Result<(Value, Option<Vec<u8>>), String> {
    if bytes.first() == Some(&b'{') {
        return serde_json::from_slice(bytes).map(|v| (v, None)).map_err(|e| format!("glTF JSON: {e}"));
    }
    let word = |at: usize| -> Option<u32> { bytes.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes) };
    if word(0) != Some(0x4654_6C67) {
        return Err("not a glb (no `glTF` magic)".into());
    }
    let len = word(12).ok_or("glb: cut short before the JSON chunk")? as usize;
    if word(16) != Some(0x4E4F_534A) {
        return Err("glb: the first chunk is not JSON".into());
    }
    let json = bytes.get(20..20 + len).ok_or("glb: the JSON chunk runs past the end")?;
    let doc = serde_json::from_slice(json).map_err(|e| format!("glTF JSON: {e}"))?;
    let at = 20 + len;
    let bin = match word(at) {
        Some(n) if word(at + 4) == Some(0x004E_4942) => Some(bytes.get(at + 8..at + 8 + n as usize).ok_or("glb: the binary chunk runs past the end")?.to_vec()),
        _ => None,
    };
    Ok((doc, bin))
}

/// A `.glb` from a JSON document and an optional binary chunk (each padded
/// to 4 bytes, as the spec asks).
pub fn glb_build(doc: &Value, bin: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut js = serde_json::to_vec(doc).map_err(|e| format!("glTF JSON: {e}"))?;
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let mut bin = bin.map(<[u8]>::to_vec).unwrap_or_default();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 20 + js.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let u = |n: usize| u32::try_from(n).map(u32::to_le_bytes).map_err(|_| "glb: over 4 GB".to_string());
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u(total)?);
    out.extend_from_slice(&u(js.len())?);
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    if !bin.is_empty() {
        out.extend_from_slice(&u(bin.len())?);
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    Ok(out)
}

/// `glb` with material `index` (or every material, `None`) filled with
/// `written` ([`apply`]); the binary chunk is copied as is.
pub fn glb_with_material(glb: &[u8], index: Option<usize>, written: &Value) -> Result<Vec<u8>, String> {
    let (mut doc, bin) = glb_parts(glb)?;
    let mats = doc.get_mut("materials").and_then(Value::as_array_mut).ok_or("glb: no materials")?;
    match index {
        Some(i) => apply(mats.get_mut(i).ok_or_else(|| format!("glb: no material {i}"))?, written),
        None => mats.iter_mut().for_each(|m| apply(m, written)),
    }
    note_extensions(&mut doc);
    glb_build(&doc, bin.as_deref())
}
