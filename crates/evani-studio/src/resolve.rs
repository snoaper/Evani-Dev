//! The one rendering input: [`resolve`] turns a record (and what is
//! remembered for it) into everything the scene needs. It is a pure
//! function: same record, same catalogues -> same [`Resolved`], whatever the
//! click order. It never names another asset than the record's, never falls
//! back to a sample shape, and has no "unavailable" outcome: a cell is
//! ready, being generated, or its asset is still being built.

use std::collections::BTreeMap;

use crate::assets::{AssetStatus, Tone};
use crate::compat::place;
use crate::geo::generator_version;
use crate::hash::{GeometryKey, KeyWriter, PassKey, VariantKey};
use crate::state::{Selection, Settings, StudioState, Tweak};
use crate::styles::{Geometry, Pass, Surface};
use crate::{Catalogs, ENGINE_VERSION};

/// Where a region's material comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundBy {
    /// The record's material landed here.
    Selected,
    /// The region follows another (the region id).
    Follows(String),
    /// The region's own default material, or the asset's for a leading
    /// region the pick didn't land on.
    Default,
    /// The source model's own colour (eyes).
    Keep,
    /// The material the record gives this part (the part id): the user
    /// picked it for that part only.
    Part(String),
}

/// One region's material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub region: String,
    /// `None` for a kept region.
    pub material: Option<String>,
    pub tone: Tone,
    pub from: BoundBy,
}

/// Can the scene draw it now?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Draw the source with the bindings (geometry stage `none`).
    Ready,
    /// Run (or fetch from the cache) the geometry stage keyed by this key;
    /// meanwhile draw the same asset's plain source ("Making voxel horse...").
    Generate(GeometryKey),
    /// The asset's source model is still being built: say so (same asset id,
    /// never another object).
    NotBuilt,
}

/// Everything the scene needs for one record.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub selection: Selection,
    pub availability: Availability,
    /// The geometry stage with the style's controls applied.
    pub geometry: Geometry,
    pub surface: Surface,
    pub pass: Pass,
    /// One per asset region, in the asset's order (at most 16).
    pub bindings: Vec<Binding>,
    pub geometry_key: GeometryKey,
    pub variant_key: VariantKey,
    pub pass_key: PassKey,
}

fn write_controls(w: &mut KeyWriter, values: &BTreeMap<String, f32>, prefix: &str) {
    for (k, v) in values.iter().filter(|(k, _)| k.starts_with(prefix)) {
        w.str(k).f32(*v);
    }
}

/// The geometry stage's cache key: engine version, generator version, asset
/// id, source hash, the geometry stage with its controls.
pub fn geometry_key(c: &Catalogs, asset: &str, geometry: &Geometry, controls: &BTreeMap<String, f32>) -> GeometryKey {
    let mut w = KeyWriter::new("geometry");
    w.str(ENGINE_VERSION).u64(u64::from(generator_version(geometry.kind()))).str(asset).u64(c.source_hashes.get(asset).copied().unwrap_or(0));
    geometry.write_key(&mut w);
    write_controls(&mut w, controls, "geometry.");
    GeometryKey(w.finish())
}

/// Resolve `sel` with the style controls `controls` and the remembered
/// `settings` (edits enter the variant key).
pub fn resolve(c: &Catalogs, sel: &Selection, controls: &BTreeMap<String, f32>, settings: Option<&Settings>) -> Result<Resolved, String> {
    let asset = c.assets.asset(&sel.asset).ok_or_else(|| format!("no asset `{}`", sel.asset))?;
    let style = c.styles.style(&sel.style).ok_or_else(|| format!("no style `{}`", sel.style))?;
    let mat = c.materials.get(&sel.material).ok_or_else(|| format!("no material `{}`", sel.material))?;
    let at = place(c, asset, mat).ok_or_else(|| format!("`{}` doesn't go on `{}`", sel.material, sel.asset))?;
    let (geometry, surface, pass) = style.with_controls(controls);
    // The record's per-part materials, by region: a part's material lands on
    // its regions that don't follow another region of the same part.
    let mut own: Vec<Option<(&str, &str)>> = vec![None; asset.regions.len()];
    for (pid, mid) in &sel.parts {
        let part = asset.part(pid).ok_or_else(|| format!("`{}` has no part `{pid}`", sel.asset))?;
        c.materials.get(mid).ok_or_else(|| format!("no material `{mid}`"))?;
        for i in asset.part_regions(part) {
            if !asset.follows_within_part(i) {
                own[i] = Some((pid.as_str(), mid.as_str()));
            }
        }
    }
    let overridden = |i: usize| asset.part_of_region(i).is_some_and(|p| sel.parts.contains_key(&asset.parts[p].id));

    // Leading regions first, then the ones that follow them.
    let lead = |i: usize| -> Binding {
        let r = &asset.regions[i];
        if let Some((pid, mid)) = own[i] {
            return Binding { region: r.id.clone(), material: Some(mid.to_string()), tone: crate::assets::Tone::Base, from: BoundBy::Part(pid.to_string()) };
        }
        let (material, from) = if i == at {
            (Some(sel.material.clone()), BoundBy::Selected)
        } else if r.keep {
            (None, BoundBy::Keep)
        } else {
            (Some(r.default_material.clone().unwrap_or_else(|| asset.default_material.clone())), BoundBy::Default)
        };
        Binding { region: r.id.clone(), material, tone: r.tone, from }
    };
    let mut bindings: Vec<Binding> = (0..asset.regions.len()).map(lead).collect();
    for (i, r) in asset.regions.iter().enumerate() {
        if i == at || own[i].is_some() {
            continue;
        }
        if let Some(l) = &r.follows
            && let Some(li) = asset.region(l)
            // A part given its own material stops following across parts.
            && !(overridden(i) && !asset.follows_within_part(i))
        {
            let leader = bindings[li].material.clone();
            bindings[i] = Binding { region: r.id.clone(), material: leader, tone: r.tone, from: BoundBy::Follows(l.clone()) };
        }
    }

    let geometry_key = geometry_key(c, &asset.id, &geometry, controls);
    let mut w = KeyWriter::new("variant");
    w.u64(geometry_key.0);
    for b in &bindings {
        w.str(&b.region).str(b.material.as_deref().unwrap_or("-")).bool(b.tone == Tone::Points);
    }
    surface.write_key(&mut w);
    write_controls(&mut w, controls, "surface.");
    if let Some(s) = settings {
        for (k, t) in &s.edits {
            w.str(k);
            match *t {
                Tweak::Num(x) => {
                    w.f32(x);
                }
                Tweak::Colour(rgb) => {
                    w.f32(rgb[0]).f32(rgb[1]).f32(rgb[2]);
                }
                Tweak::Flag(f) => {
                    w.bool(f);
                }
            }
        }
    }
    let variant_key = VariantKey(w.finish());
    let mut w = KeyWriter::new("pass");
    pass.write_key(&mut w);
    write_controls(&mut w, controls, "pass.");
    let pass_key = PassKey(w.finish());

    let availability = match (asset.status, &geometry) {
        (AssetStatus::Planned | AssetStatus::Building, _) => Availability::NotBuilt,
        (AssetStatus::Ready, Geometry::None) => Availability::Ready,
        (AssetStatus::Ready, _) => Availability::Generate(geometry_key),
    };
    Ok(Resolved { selection: sel.clone(), availability, geometry, surface, pass, bindings, geometry_key, variant_key, pass_key })
}

/// Each part of the record's asset with the binding of its lead region (what
/// the part wears), in the asset's part order.
pub fn part_bindings<'a>(c: &'a Catalogs, r: &'a Resolved) -> Vec<(&'a crate::assets::Part, &'a Binding)> {
    let Some(asset) = c.assets.asset(&r.selection.asset) else { return Vec::new() };
    asset.parts.iter().filter_map(|p| asset.part_lead(p).and_then(|i| r.bindings.get(i)).map(|b| (p, b))).collect()
}

impl StudioState {
    /// [`resolve`] of the current record.
    pub fn resolve(&self, c: &Catalogs) -> Result<Resolved, String> {
        resolve(c, &self.selection, &self.style_controls(), self.settings())
    }
}
