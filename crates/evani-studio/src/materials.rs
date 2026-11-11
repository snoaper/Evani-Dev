//! The material catalogue (`catalog/materials/materials.json`) as the
//! studio reads it: the few fields matching and the page need, plus
//! `belongs_on`.
//!
//! **`belongs_on`** (optional on any row): the asset class or classes a
//! material is made for, as one string or a list, each `family.kind` (an
//! asset class in catalog/assets/assets.json) or `family.*` (every class of
//! the family). Absent: a generic surface that goes on anything.
//! - A class marked `exclusive` makes the material foreign on other classes
//!   (a horse coat on a cow).
//! - Any class picks where the material previews when no asset was chosen
//!   on purpose (a brick previews on a wall).
//!
//! The recipe crate's reader ignores the field: it is a studio hint and
//! never reaches the 32-byte block (like `params.shape`). The CSV export
//! doesn't carry it either (like `params`).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::assets::{dotted_id, plain_id};

/// One catalogue row, as the studio sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub subcategory: String,
    pub recipe: String,
    /// `planned`, `ready`, `reviewed` or `alias`.
    pub status: String,
    /// Class patterns (`quadruped.horse`, `quadruped.*`); empty = generic.
    pub belongs_on: Vec<String>,
    /// Its first colour (sRGB8), from `params.colours` (an alias: its
    /// target's), when the row has one.
    pub colour: Option<[u8; 3]>,
    /// `params.points_on`: the `tone: points` regions that wear its points
    /// colour (a palomino's mane and tail, not its legs); empty = all.
    pub points_on: Vec<String>,
}

impl MaterialInfo {
    /// Can it be drawn now (not just planned)?
    pub fn ready(&self) -> bool {
        matches!(self.status.as_str(), "ready" | "reviewed" | "alias")
    }
}

/// Is `p` a well-formed `belongs_on` entry (`family.kind` or `family.*`)?
pub fn class_pattern_ok(p: &str) -> bool {
    match p.strip_suffix(".*") {
        Some(family) => plain_id(family),
        None => dotted_id(p),
    }
}

/// Does pattern `p` take class `class`?
pub fn class_matches(p: &str, class: &str) -> bool {
    match p.strip_suffix(".*") {
        Some(family) => class.split_once('.').is_some_and(|(f, _)| f == family),
        None => p == class,
    }
}

/// `belongs_on` as written on a row: absent, one string or a list of them.
pub fn parse_belongs_on(v: Option<&Value>) -> Result<Vec<String>, String> {
    let list: Vec<String> = match v {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| "belongs_on entries must be strings".to_string())).collect::<Result<_, _>>()?,
        Some(_) => return Err("belongs_on must be a class id or a list of them".into()),
    };
    if list.is_empty() {
        return Err("belongs_on: leave it out instead of an empty list".into());
    }
    if let Some(bad) = list.iter().find(|p| !class_pattern_ok(p)) {
        return Err(format!("belongs_on: `{bad}` isn't `family.kind` or `family.*`"));
    }
    Ok(list)
}

fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let b = |i: usize| s.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some([b(0)?, b(2)?, b(4)?])
}

/// The catalogue, indexed by id.
#[derive(Debug, Clone, Default)]
pub struct MaterialIndex {
    pub rows: Vec<MaterialInfo>,
    by_id: BTreeMap<String, usize>,
}

impl MaterialIndex {
    pub fn parse(materials_json: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(materials_json).map_err(|e| format!("materials.json: {e}"))?;
        let rows = v.get("rows").and_then(Value::as_array).ok_or("materials.json: no `rows` list")?;
        let s = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("materials.json: a row lacks `{k}`"));
        let mut out = Self::default();
        let mut alias_of: Vec<(usize, String)> = Vec::new();
        for r in rows {
            let id = s(r, "id")?;
            if id.contains('|') {
                return Err(format!("materials.json: `{id}`: ids never contain `|` (the studio's selection key separator)"));
            }
            let belongs_on = parse_belongs_on(r.get("belongs_on")).map_err(|e| format!("materials.json: `{id}`: {e}"))?;
            let params = r.get("params");
            let colour = params.and_then(|p| p.get("colours")).and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).and_then(hex);
            if let Some(t) = params.and_then(|p| p.get("alias_of")).and_then(Value::as_str) {
                alias_of.push((out.rows.len(), t.to_string()));
            }
            let info = MaterialInfo {
                name: s(r, "name")?,
                category: s(r, "category")?,
                subcategory: s(r, "subcategory")?,
                recipe: s(r, "recipe")?,
                status: s(r, "status")?,
                belongs_on,
                colour,
                points_on: params.and_then(|p| p.get("points_on")).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
                id: id.clone(),
            };
            if out.by_id.insert(id.clone(), out.rows.len()).is_some() {
                return Err(format!("materials.json: `{id}` is listed twice"));
            }
            out.rows.push(info);
        }
        for (i, target) in alias_of {
            let c = out.get(&target).and_then(|t| t.colour);
            if let Some(r) = out.rows.get_mut(i)
                && r.colour.is_none()
            {
                r.colour = c;
            }
        }
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Option<&MaterialInfo> {
        self.by_id.get(id).and_then(|&i| self.rows.get(i))
    }
}
