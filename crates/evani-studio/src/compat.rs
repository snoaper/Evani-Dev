//! Compatibility rules: can material M go on asset A, and on which region?
//! All of it is data: the material's `belongs_on`, the class's `exclusive`
//! and `swatch` flags, and each region's `accepts` list.
//!
//! | Level | When | The page |
//! |---|---|---|
//! | [`Compat::Native`] | M belongs on A's class | a normal tile |
//! | [`Compat::Generic`] | M belongs nowhere, belongs on a class that isn't exclusive (brick), or A is a swatch | a normal tile |
//! | [`Compat::Foreign`] | M belongs on another, exclusive class (a horse coat on a cow) | greyed, "Made for horses" |
//! | [`Compat::NoRegion`] | no region of A accepts M | greyed, "Can't go on a cow" |
//!
//! Every style goes with every asset: styles have no compatibility level.

use crate::Catalogs;
use crate::assets::{AssetRow, Part};
use crate::materials::{MaterialInfo, class_matches};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compat {
    Native,
    Generic,
    /// Made for `made_for` (a class id), which is exclusive.
    Foreign { made_for: String },
    NoRegion,
}

impl Compat {
    /// Can the material be applied (native or generic, with a region)?
    pub fn applies(&self) -> bool {
        matches!(self, Self::Native | Self::Generic)
    }
}

/// The class level only (no region test).
fn level(c: &Catalogs, asset: &AssetRow, m: &MaterialInfo) -> Compat {
    if m.belongs_on.iter().any(|p| class_matches(p, &asset.class)) {
        return Compat::Native;
    }
    if m.belongs_on.is_empty() || c.assets.class(&asset.class).is_some_and(|k| k.swatch) {
        return Compat::Generic;
    }
    // The first exclusive class the material belongs on makes it foreign.
    let exclusive = c.assets.classes.iter().find(|k| k.exclusive && m.belongs_on.iter().any(|p| class_matches(p, &k.id)));
    match exclusive {
        Some(k) => Compat::Foreign { made_for: k.id.clone() },
        None => Compat::Generic,
    }
}

/// Does region token `t` take material `m` at class level `lv`? Specific
/// tokens only (`class`, `recipe:`, `sub:`); `generic` is the fallback pass.
fn specific(t: &str, m: &MaterialInfo, lv: &Compat) -> bool {
    match t.split_once(':') {
        None => t == "class" && *lv == Compat::Native,
        Some(("recipe", r)) => m.recipe == r,
        Some(("sub", cs)) => cs.split_once('/').is_some_and(|(cat, sub)| m.category == cat && m.subcategory == sub),
        Some(_) => false,
    }
}

/// Is region id `region` one of the words of material id `id` (split on `.`
/// and `_`): `horse.muzzle.pink` names the muzzle, `horn.cattle` the horn.
fn named_by(id: &str, region: &str) -> bool {
    id.split(['.', '_']).any(|w| w == region)
}

/// The region (index into `asset.regions`) material `m` lands on, if it can
/// be applied. Among the regions a specific token takes it (`class`,
/// `recipe:`, `sub:`), the one the material's id names wins (a muzzle skin
/// on the muzzle, not the coat that also takes hide; a hoof on the hoof, not
/// the horn that also takes keratin), else the first in the asset's order;
/// with no specific taker, the first that accepts `generic`. Kept regions
/// never take a material.
pub fn place(c: &Catalogs, asset: &AssetRow, m: &MaterialInfo) -> Option<usize> {
    let lv = level(c, asset, m);
    if !lv.applies() {
        return None;
    }
    let open = || asset.regions.iter().enumerate().filter(|(_, r)| !r.keep);
    let takers = || open().filter(|(_, r)| r.accepts.iter().any(|t| specific(t, m, &lv)));
    takers()
        .find(|(_, r)| named_by(&m.id, &r.id))
        .or_else(|| takers().next())
        .or_else(|| open().find(|(_, r)| r.accepts.iter().any(|t| t == "generic")))
        .map(|(i, _)| i)
}

/// The full level, the region test included.
pub fn compat(c: &Catalogs, asset: &AssetRow, m: &MaterialInfo) -> Compat {
    let lv = level(c, asset, m);
    if lv.applies() && place(c, asset, m).is_none() {
        return Compat::NoRegion;
    }
    lv
}

/// Does part `part` of `asset` take material `m` (picked for that part)?
/// Its class level must apply, and its lead region (or, for a part of kept
/// regions, any of its regions) must accept it: by a specific token, or by
/// `generic` on a region that isn't kept.
pub fn part_takes(c: &Catalogs, asset: &AssetRow, part: &Part, m: &MaterialInfo) -> bool {
    let lv = level(c, asset, m);
    if !lv.applies() {
        return false;
    }
    let regions = asset.part_regions(part);
    let open: Vec<usize> = regions.iter().copied().filter(|&i| !asset.follows_within_part(i)).collect();
    open.iter().any(|&i| {
        let r = &asset.regions[i];
        r.accepts.iter().any(|t| specific(t, m, &lv) || (!r.keep && t == "generic"))
    })
}

/// Is part `part` made for `m`: does it take it by a specific token? A
/// material made for a class (a roof tile, a horse coat) counts only by
/// `class` or `sub:` (a slate roof is masonry, but the brick gable isn't made
/// for roof slate); one made for nothing in particular also by `recipe:`.
/// "All parts" spreads a material over these.
pub fn part_made_for(c: &Catalogs, asset: &AssetRow, part: &Part, m: &MaterialInfo) -> bool {
    let lv = level(c, asset, m);
    let token_ok = |t: &str| specific(t, m, &lv) && (m.belongs_on.is_empty() || !t.starts_with("recipe:"));
    lv.applies() && asset.part_lead(part).is_some_and(|i| asset.regions[i].accepts.iter().any(|t| token_ok(t)))
}

/// The full level of `m` on one part: [`Compat::NoRegion`] when the part
/// doesn't take it.
pub fn part_compat(c: &Catalogs, asset: &AssetRow, part: &Part, m: &MaterialInfo) -> Compat {
    let lv = level(c, asset, m);
    if lv.applies() && !part_takes(c, asset, part, m) {
        return Compat::NoRegion;
    }
    lv
}

/// The asset a material previews on when nothing was chosen on purpose:
/// the primary asset of the first exact class it belongs on (a `family.*`
/// pattern alone names no asset).
pub fn preview_asset<'a>(c: &'a Catalogs, m: &MaterialInfo) -> Option<&'a AssetRow> {
    m.belongs_on.iter().filter(|p| !p.ends_with(".*")).find_map(|p| c.assets.class(p)).and_then(|k| c.assets.asset(&k.primary_asset))
}
