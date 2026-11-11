//! The variations strip (SPEC section 1, "Bottom"): the same asset in the
//! same style, in other materials. Pure and deterministic: the same record
//! and catalogues always give the same tiles in the same order.
//!
//! The tiles, after the current material (first, marked `current`):
//! 1. materials made for this asset's class (a horse's coats), closest in
//!    colour to the current material first;
//! 2. other materials of the current material's subcategory, then of its
//!    category, that go on the asset, closest in colour first;
//! 3. at most `marked` tiles that **don't** go on it, each with its reason
//!    ("Made for horses", "Can't go on a cow"): first the current
//!    subcategory's, then those made for another class of the asset's
//!    family (a horse coat on a cow).
//!
//! A tile's selection keeps the record's asset and style: picking it is
//! `StudioCommand::Variation`, which never moves the asset.

use evani_studio::compat::{compat, part_compat};
use evani_studio::{Compat, MaterialInfo};

use super::{Catalogs, Selection};

/// One tile.
#[derive(Debug, Clone, PartialEq)]
pub struct Variation {
    pub material: String,
    pub name: String,
    /// The record the tile shows (same asset and style).
    pub selection: Selection,
    pub compat: Compat,
    /// Why it can't be applied (`None` when it can).
    pub why: Option<String>,
    /// The record's own material.
    pub current: bool,
}

fn distance(a: Option<[u8; 3]>, b: Option<[u8; 3]>) -> u32 {
    match (a, b) {
        (Some(a), Some(b)) => a.iter().zip(b).map(|(x, y)| (i32::from(*x) - i32::from(y)).unsigned_abs().pow(2)).sum(),
        _ => u32::MAX,
    }
}

/// The reason a material can't go on the asset, in the page's words.
pub fn reason(c: &Catalogs, asset_class: &str, level: &Compat) -> Option<String> {
    match level {
        Compat::Native | Compat::Generic => None,
        Compat::Foreign { made_for } => Some(format!("Made for {}", c.assets.class(made_for).map_or(made_for.as_str(), |k| k.plural.as_str()))),
        Compat::NoRegion => Some(format!("Can't go on {}", c.assets.class(asset_class).map_or(asset_class, |k| k.label.as_str()))),
    }
}

/// The strip for `sel`: at most `count` tiles (the current one included), of
/// which at most `marked` don't go on the asset. Empty for an unknown id.
pub fn variations(c: &Catalogs, sel: &Selection, count: usize, marked: usize) -> Vec<Variation> {
    variations_for(c, sel, None, count, marked)
}

/// [`variations`] for one part of the asset (`None`: the record's material,
/// "All parts"): the part's own material first, then the materials that go
/// on that part; each tile's selection is the record with that part changed
/// (a pick goes through `StudioCommand::Variation` with the part selected).
pub fn variations_for(c: &Catalogs, sel: &Selection, part: Option<&str>, count: usize, marked: usize) -> Vec<Variation> {
    let Some(asset) = c.assets.asset(&sel.asset) else { return Vec::new() };
    let part = part.and_then(|p| asset.part(p));
    let cur_id = match part {
        Some(p) => sel.part_material(c, &p.id),
        None => Some(sel.material.clone()),
    };
    let Some(cur) = cur_id.as_deref().and_then(|m| c.materials.get(m)) else { return Vec::new() };
    let family = asset.class.split_once('.').map_or(asset.class.as_str(), |(f, _)| f);
    let level_of = |m: &MaterialInfo| match part {
        Some(p) => part_compat(c, asset, p, m),
        None => compat(c, asset, m),
    };
    let tile = |m: &MaterialInfo, level: Compat| Variation {
        material: m.id.clone(),
        name: m.name.clone(),
        selection: match part {
            Some(p) => sel.with_part(c, &p.id, &m.id).unwrap_or_else(|_| sel.clone()),
            None => Selection { material: m.id.clone(), ..sel.clone() },
        },
        why: reason(c, &asset.class, &level),
        compat: level,
        current: m.id == cur.id,
    };
    // Ready rows other than the current one; an alias is its target again.
    let others: Vec<(&MaterialInfo, Compat)> = c.materials.rows.iter().filter(|m| m.ready() && m.status != "alias" && m.id != cur.id).map(|m| (m, level_of(m))).collect();
    let near = |list: &mut Vec<&(&MaterialInfo, Compat)>| list.sort_by_key(|(m, _)| (distance(m.colour, cur.colour), m.id.clone()));
    let mut out = vec![tile(cur, level_of(cur))];
    let taken = |out: &mut Vec<Variation>, list: Vec<&(&MaterialInfo, Compat)>, limit: usize| {
        for (m, level) in list {
            if out.len() >= limit {
                break;
            }
            if !out.iter().any(|t| t.material == m.id) {
                out.push(tile(m, level.clone()));
            }
        }
    };

    // The incompatible tiles are chosen first (so the compatible ones know
    // how much room they have) but shown last.
    let mut bad_sub: Vec<_> = others.iter().filter(|(m, l)| !l.applies() && m.subcategory == cur.subcategory).collect();
    near(&mut bad_sub);
    let mut bad_family: Vec<_> = others
        .iter()
        .filter(|(m, l)| matches!(l, Compat::Foreign { made_for } if made_for.split_once('.').is_some_and(|(f, _)| f == family)) && m.subcategory != cur.subcategory)
        .collect();
    near(&mut bad_family);
    let mut bad = Vec::new();
    taken(&mut bad, bad_sub.into_iter().chain(bad_family).collect(), marked);

    let room = count.saturating_sub(bad.len()).max(1);
    let mut native: Vec<_> = others.iter().filter(|(_, l)| *l == Compat::Native).collect();
    near(&mut native);
    let mut sub: Vec<_> = others.iter().filter(|(m, l)| l.applies() && m.subcategory == cur.subcategory).collect();
    near(&mut sub);
    let mut cat: Vec<_> = others.iter().filter(|(m, l)| l.applies() && m.category == cur.category && m.subcategory != cur.subcategory).collect();
    near(&mut cat);
    taken(&mut out, native, room);
    taken(&mut out, sub, room);
    taken(&mut out, cat, room);
    out.extend(bad);
    out.truncate(count.max(1));
    out
}
