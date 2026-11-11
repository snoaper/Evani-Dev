//! Checks that catch a broken contract (errors become tools): the
//! catalogue cross-checks [`problems`] (run by `cargo xtask check`), the
//! asset x style coverage check [`coverage`], and the list of styles still
//! waiting for a geometry generator [`gaps`].

use crate::Catalogs;
use crate::assets::{AssetStatus, SourceKind};
use crate::compat::{Compat, compat, place};
use crate::geo::generator_version;
use crate::resolve::{Availability, resolve};
use crate::state::Selection;
use crate::styles::GeometryKind;

/// A broken rule, with its fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub what: String,
    pub fix: String,
}

fn p(what: String, fix: &str) -> Problem {
    Problem { what, fix: fix.to_string() }
}

/// The cross-checks between the asset, material and style catalogues.
pub fn problems(c: &Catalogs) -> Vec<Problem> {
    let mut out = Vec::new();
    // belongs_on names real classes.
    for m in &c.materials.rows {
        for pat in &m.belongs_on {
            let known = match pat.strip_suffix(".*") {
                Some(f) => c.assets.classes.iter().any(|k| k.id.split_once('.').is_some_and(|(x, _)| x == f)),
                None => c.assets.class(pat).is_some(),
            };
            if !known {
                out.push(p(
                    format!("material `{}`: belongs_on `{pat}` is no asset class", m.id),
                    "Use a class id from `classes` in catalog/assets/assets.json (or add the class there with its primary asset).",
                ));
            }
        }
    }
    let ready = |id: &str| c.materials.get(id).is_some_and(|m| m.ready());
    for a in &c.assets.assets {
        // The default material exists, is built and goes on the asset.
        match c.materials.get(&a.default_material) {
            None => out.push(p(format!("asset `{}`: default_material `{}` isn't in the material catalogue", a.id, a.default_material), "Name a row of catalog/materials/materials.json.")),
            Some(m) if !m.ready() => out.push(p(format!("asset `{}`: default_material `{}` is only planned", a.id, a.default_material), "Pick a ready material.")),
            Some(m) => {
                if !compat(c, a, m).applies() {
                    out.push(p(format!("asset `{}`: its default material `{}` doesn't go on it ({:?})", a.id, m.id, compat(c, a, m)), "Pick a native or generic material, or fix the region's `accepts`."));
                }
            }
        }
        for r in &a.regions {
            if let Some(d) = &r.default_material
                && !ready(d)
            {
                out.push(p(format!("asset `{}`, region `{}`: default_material `{d}` isn't a ready material", a.id, r.id), "Name a ready row of the material catalogue."));
            }
            for t in &r.accepts {
                if let Some(rec) = t.strip_prefix("recipe:")
                    && !c.materials.rows.iter().any(|m| m.recipe == rec)
                {
                    out.push(p(format!("asset `{}`, region `{}`: `{t}` names a recipe no material uses", a.id, r.id), "Use a recipe id from catalog/materials/recipes.json."));
                }
            }
        }
        if a.status == AssetStatus::Ready && a.source.kind != SourceKind::Code && a.source.model.is_none() {
            out.push(p(format!("asset `{}` is ready but names no model", a.id), "Set source.model to the app's model id."));
        }
    }
    // A material that belongs on a class previews on that class's asset.
    for m in c.materials.rows.iter().filter(|m| m.ready() && !m.belongs_on.is_empty()) {
        if let Some(a) = crate::compat::preview_asset(c, m)
            && place(c, a, m).is_none()
        {
            out.push(p(format!("material `{}` belongs on `{}` but no region of `{}` takes it", m.id, a.class, a.id), "Add a region token (`class`, `recipe:<id>`) that takes it, or fix belongs_on."));
        }
    }
    // `points_on` names points regions of the asset the row previews on.
    for m in c.materials.rows.iter().filter(|m| !m.points_on.is_empty()) {
        let Some(a) = crate::compat::preview_asset(c, m) else {
            out.push(p(format!("material `{}` has points_on but no asset to wear it", m.id), "Give it a belongs_on class, or drop points_on."));
            continue;
        };
        for r in m.points_on.iter().filter(|r| !a.regions.iter().any(|x| &x.id == *r && x.tone == crate::assets::Tone::Points)) {
            out.push(p(format!("material `{}`: points_on `{r}` is no `tone: points` region of `{}`", m.id, a.id), "Name regions of the asset whose tone is points (a horse: leg, mane, tail)."));
        }
    }
    out
}

/// One cell of the asset x style matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub asset: String,
    pub style: String,
    pub availability: Availability,
}

/// Every asset in every style: each cell must resolve to the same asset,
/// with its default material, to a ready, generating or not-built state
/// (there is no "unavailable" end state), and a ready asset's cells must not
/// be "not built". Returns every cell, or every problem.
pub fn coverage(c: &Catalogs) -> Result<Vec<Cell>, Vec<Problem>> {
    let mut cells = Vec::new();
    let mut bad = Vec::new();
    let none = std::collections::BTreeMap::new();
    for a in &c.assets.assets {
        for s in &c.styles.styles {
            let sel = Selection::new(&a.id, &s.id, &a.default_material);
            match resolve(c, &sel, &none, None) {
                Ok(r) if r.selection.asset != a.id => bad.push(p(format!("{} x {}: resolved to another asset `{}`", a.id, s.id, r.selection.asset), "resolve() must never substitute an object.")),
                Ok(r) if a.status == AssetStatus::Ready && r.availability == Availability::NotBuilt => {
                    bad.push(p(format!("{} x {}: a ready asset resolved as not built", a.id, s.id), "A ready asset's cells are ready or generating."))
                }
                Ok(r) if r.bindings.len() != a.regions.len() => bad.push(p(format!("{} x {}: {} bindings for {} regions", a.id, s.id, r.bindings.len(), a.regions.len()), "One binding per region.")),
                Ok(r) => cells.push(Cell { asset: a.id.clone(), style: s.id.clone(), availability: r.availability }),
                Err(e) => bad.push(p(format!("{} x {}: {e}", a.id, s.id), "Every asset must resolve in every style.")),
            }
        }
    }
    if bad.is_empty() { Ok(cells) } else { Err(bad) }
}

/// Styles whose geometry stage has no generator yet, with the ready assets
/// that wait on them: (style id, geometry kind, ready assets). Empty once
/// every generator is built. `cargo xtask check` lists them; the style
/// matrix (WP9) fails on them for ready assets.
pub fn gaps(c: &Catalogs) -> Vec<(String, GeometryKind, usize)> {
    let ready = c.assets.assets.iter().filter(|a| a.status == AssetStatus::Ready).count();
    c.styles.styles.iter().filter(|s| generator_version(s.geometry.kind()) == 0).map(|s| (s.id.clone(), s.geometry.kind(), ready)).collect()
}

/// Foreign pairs, for the page and the tests: (asset, material) where the
/// material is made for another, exclusive class.
pub fn foreign_pairs(c: &Catalogs) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for a in &c.assets.assets {
        for m in c.materials.rows.iter().filter(|m| !m.belongs_on.is_empty()) {
            if matches!(compat(c, a, m), Compat::Foreign { .. }) {
                out.push((a.id.clone(), m.id.clone()));
            }
        }
    }
    out
}
