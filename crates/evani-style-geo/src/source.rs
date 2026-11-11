//! An asset's source model to a [`SourceMesh`] (WP2, the asset loader;
//! docs/studio/WORK_PACKAGES.md). The geometry stage only ever sees this.
//!
//! The rest pose, unskinned (a skinned tier is read at bind pose by
//! [`crate::rest`]), every primitive merged into one mesh in model space,
//! scaled to the asset's bounds at radius 1 around their centre (as the
//! viewer's shapes), with a region index per vertex (an index into the asset
//! row's `regions`), decided per vertex in this order:
//! 1. [`PART_ATTRIBUTE`] when the primitive has it: one unsigned integer per
//!    vertex written by the asset's generator (the studio tiers, the
//!    quadruped generator WP7, the building kit).
//! 2. On a primitive with the body chart [`CHART_ATTRIBUTE`] (the horse v5
//!    tiers: along, around, distal, fur length), the asset's steps in
//!    `catalog/assets/source_rules.json` (data, not code). The chart comes
//!    first there: the horse's body material is called `coat` and would
//!    otherwise swallow its legs, ears and muzzle.
//! 3. A glTF material named after one of the row's regions (`hoof`, `eye`,
//!    `tiles`...): the studio tiers carry one primitive per region, so a
//!    converter that drops the byte still keeps the regions.
//! 4. The steps again (only their material conditions can hold), else the
//!    first region (an asset with one region needs nothing).
//!
//! Then the rules' `proxies` add the solid shapes the tier lacks (the
//! horse's mane and tail, [`crate::proxy`]), only when no vertex already
//! holds that region. A name the row doesn't hold, or a vertex index past
//! its regions, is an error, never a guess.

use std::collections::BTreeMap;

use evani_gltf::Gltf;
use evani_studio::assets::{AssetCatalog, AssetRow};
use evani_studio::geo::SourceMesh;
use serde::Deserialize;

use crate::math::{V3, bounds, len, scale, sub};
use crate::proxy;
use crate::rest::{RestMesh, rest_from_glb};

/// The per-vertex region index attribute the studio's generators write.
pub const PART_ATTRIBUTE: &str = "_EVANI_PART";
/// The body chart the horse's converter baked (horse-model branch).
pub const CHART_ATTRIBUTE: &str = "_EVANI_REGION";

/// `catalog/assets/source_rules.json`, built in.
pub const RULES_JSON: &str = include_str!("../../../catalog/assets/source_rules.json");

/// The rules file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRules {
    pub version: u32,
    pub note: String,
    /// By asset id.
    pub assets: BTreeMap<String, AssetRules>,
}

/// One asset's rules.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRules {
    /// What the chart is, for the reader of the file.
    #[serde(default)]
    pub chart: Option<String>,
    /// Tried in order; the first match wins; the last matches anything.
    pub steps: Vec<Step>,
    #[serde(default)]
    pub proxies: Vec<ProxyRule>,
}

/// A step: the region a vertex takes when every condition given holds.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub region: String,
    /// The vertex's glTF material name.
    #[serde(default)]
    pub material: Option<String>,
    /// Chart channels, `[min, max)`.
    #[serde(default)]
    pub along: Option<[f32; 2]>,
    #[serde(default)]
    pub around: Option<[f32; 2]>,
    #[serde(default)]
    pub distal: Option<[f32; 2]>,
    #[serde(default)]
    pub length: Option<[f32; 2]>,
}

impl Step {
    /// Matches anything (no condition).
    pub fn catch_all(&self) -> bool {
        self.material.is_none() && self.along.is_none() && self.around.is_none() && self.distal.is_none() && self.length.is_none()
    }

    fn matches(&self, material: &str, chart: [f32; 4]) -> bool {
        let inside = |r: Option<[f32; 2]>, x: f32| r.is_none_or(|[lo, hi]| x >= lo && x < hi);
        self.material.as_deref().is_none_or(|m| m == material) && inside(self.along, chart[0]) && inside(self.around, chart[1]) && inside(self.distal, chart[2]) && inside(self.length, chart[3])
    }
}

/// A solid shape built from the tier's anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyShape {
    /// A fin along the crest ([`proxy::mane`]).
    Mane,
    /// A hanging tail from the dock ([`proxy::tail`]).
    Tail,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyRule {
    pub region: String,
    pub shape: ProxyShape,
}

impl SourceRules {
    pub fn parse(json: &str) -> Result<Self, String> {
        let r: SourceRules = serde_json::from_str(json).map_err(|e| format!("source_rules.json: {e}"))?;
        if r.version != 1 {
            return Err("source_rules.json: version must be 1".into());
        }
        Ok(r)
    }

    /// The built-in file.
    pub fn builtin() -> Result<Self, String> {
        Self::parse(RULES_JSON)
    }

    /// Cross-checks against the asset catalogue: every asset exists, every
    /// region name is one of its row's, every step list ends in a catch-all
    /// and has one before nothing else, each proxy region once.
    pub fn problems(&self, assets: &AssetCatalog) -> Vec<String> {
        let mut out = Vec::new();
        for (id, r) in &self.assets {
            let at = format!("source_rules.json: {id}");
            let Some(row) = assets.asset(id) else {
                out.push(format!("{at}: no such asset in assets.json"));
                continue;
            };
            let names = r.steps.iter().map(|s| &s.region).chain(r.proxies.iter().map(|p| &p.region));
            for n in names {
                if row.region(n).is_none() {
                    out.push(format!("{at}: `{n}` isn't one of the row's regions ({})", row.regions.iter().map(|x| x.id.as_str()).collect::<Vec<_>>().join(", ")));
                }
            }
            match r.steps.iter().position(Step::catch_all) {
                Some(i) if i + 1 == r.steps.len() => {}
                Some(_) => out.push(format!("{at}: a catch-all step before the last makes the rest unreachable")),
                None => out.push(format!("{at}: the last step must match anything (a region with no condition), so no vertex is guessed")),
            }
            for (i, p) in r.proxies.iter().enumerate() {
                if r.proxies[..i].iter().any(|q| q.region == p.region) {
                    out.push(format!("{at}: two proxies for `{}`", p.region));
                }
            }
            for s in &r.steps {
                for (ch, range) in [("along", s.along), ("around", s.around), ("distal", s.distal), ("length", s.length)] {
                    if range.is_some_and(|[lo, hi]| !(lo.is_finite() && hi.is_finite() && lo < hi)) {
                        out.push(format!("{at}: step `{}`: {ch} must be [min, max) with min < max", s.region));
                    }
                }
            }
        }
        out
    }
}

/// Read `glb` (a model tier) for `asset`, with the built-in rules.
pub fn source_from_glb(glb: &[u8], asset: &AssetRow) -> Result<SourceMesh, String> {
    let rules = SourceRules::builtin()?;
    source_from_glb_with(glb, asset, rules.assets.get(&asset.id))
}

/// Read `glb` for `asset` with these rules (`None`: the asset has none).
pub fn source_from_glb_with(glb: &[u8], asset: &AssetRow, rules: Option<&AssetRules>) -> Result<SourceMesh, String> {
    let at = |e: String| format!("{}: {e}", asset.id);
    let glb = first_lod_only(glb).map_err(at)?;
    let rest = rest_from_glb(&glb).map_err(at)?;
    let tags = tags_from_glb(&glb, rest.positions.len()).map_err(at)?;
    source_from_rest(&rest, &tags, asset, rules)
}

/// A converted tier holds its detail levels side by side
/// (`extras.evani.lods`, most detailed first, each a node tree): keep the
/// first only, so the source is one horse, not two on top of each other.
/// A file without levels comes back as it is.
pub fn first_lod_only(glb: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, String> {
    use serde_json::Value;
    let (json, bin) = evani_gltf::split_glb(glb).map_err(|e| format!("model: {e}"))?;
    let mut doc: Value = serde_json::from_slice(json).map_err(|e| format!("model: {e}"))?;
    let lods: Vec<String> = doc.pointer("/extras/evani/lods").and_then(Value::as_array).map(|a| a.iter().filter_map(|l| l.get("node").and_then(Value::as_str).map(str::to_owned)).collect()).unwrap_or_default();
    if lods.len() < 2 {
        return Ok(std::borrow::Cow::Borrowed(glb));
    }
    let nodes = doc.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    let by_name = |n: &str| nodes.iter().position(|x| x.get("name").and_then(Value::as_str) == Some(n));
    let mut drop: Vec<usize> = lods[1..].iter().filter_map(|n| by_name(n)).collect();
    if by_name(&lods[0]).is_none() || drop.len() + 1 != lods.len() {
        return Err("model: extras.evani.lods names a node the file doesn't have".into());
    }
    // Every node under a dropped level loses its mesh.
    while let Some(i) = drop.pop() {
        let Some(n) = doc.get_mut("nodes").and_then(|v| v.get_mut(i)).and_then(Value::as_object_mut) else { continue };
        n.remove("mesh");
        let kids: Vec<usize> = n.get("children").and_then(Value::as_array).map(|a| a.iter().filter_map(|k| k.as_u64().map(|k| k as usize)).collect()).unwrap_or_default();
        drop.extend(kids);
    }
    let mut js = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    Ok(std::borrow::Cow::Owned(glb_container(&js, bin.unwrap_or_default())))
}

/// A .glb from its JSON (padded to 4) and binary chunk.
fn glb_container(js: &[u8], bin: &[u8]) -> Vec<u8> {
    let pad = (4 - bin.len() % 4) % 4;
    let bin_len = bin.len() + pad;
    let total = 12 + 8 + js.len() + if bin.is_empty() { 0 } else { 8 + bin_len };
    let mut out = Vec::with_capacity(total);
    for w in [0x4654_6C67u32, 2, total as u32, js.len() as u32, 0x4E4F_534A] {
        out.extend(w.to_le_bytes());
    }
    out.extend(js);
    if !bin.is_empty() {
        for w in [bin_len as u32, 0x004E_4942] {
            out.extend(w.to_le_bytes());
        }
        out.extend(bin);
        out.extend(std::iter::repeat_n(0u8, pad));
    }
    out
}

/// What a vertex's primitive says about its region beyond [`RestMesh`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tags {
    /// Its [`PART_ATTRIBUTE`] value, if the primitive has one.
    pub part: Option<u32>,
    /// The primitive carries the body chart [`CHART_ATTRIBUTE`].
    pub charted: bool,
}

/// Per vertex, in [`crate::rest`]'s order (nodes, then their triangle
/// primitives): its [`Tags`].
pub fn tags_from_glb(glb: &[u8], vertices: usize) -> Result<Vec<Tags>, String> {
    let g = Gltf::from_glb(glb).map_err(|e| format!("model: {e}"))?;
    let mut out: Vec<Tags> = Vec::with_capacity(vertices);
    for node in &g.nodes {
        let Some(mesh) = node.mesh.and_then(|m| g.meshes.get(m)) else { continue };
        for p in mesh.primitives.iter().filter(|p| p.mode == 4) {
            let n = p.attribute("POSITION").and_then(|a| g.accessors.get(a)).map_or(0, |a| a.count);
            let charted = p.attribute(CHART_ATTRIBUTE).is_some();
            match p.attribute(PART_ATTRIBUTE) {
                Some(a) => {
                    let v = match g.read_u32(a) {
                        Ok(v) => v,
                        // A float attribute (some exporters write floats only).
                        Err(_) => g.read_f32(a).map_err(|e| format!("model: {PART_ATTRIBUTE}: {e}"))?.into_iter().map(|x| x.round().max(0.0) as u32).collect(),
                    };
                    // A converted tier carries it as unorm8 x4 (evani-convert's
                    // carry.rs parks it in a colour through gltfpack): the
                    // region index is the first channel's byte.
                    let k = g.accessors.get(a).map_or(1, |x| x.components);
                    let v = if k > 1 && v.len() == n * k { v.into_iter().step_by(k).collect() } else { v };
                    if v.len() != n {
                        return Err(format!("model: {PART_ATTRIBUTE} must be one scalar per vertex"));
                    }
                    out.extend(v.into_iter().map(|x| Tags { part: Some(x), charted }));
                }
                None => out.extend(std::iter::repeat_n(Tags { part: None, charted }, n)),
            }
        }
    }
    if out.len() != vertices {
        return Err(format!("model: read {} vertices for {PART_ATTRIBUTE}, the rest pose has {vertices}", out.len()));
    }
    Ok(out)
}

/// The source from a tier at rest (`tags`: [`tags_from_glb`]), at radius 1.
pub fn source_from_rest(rest: &RestMesh, tags: &[Tags], asset: &AssetRow, rules: Option<&AssetRules>) -> Result<SourceMesh, String> {
    let mut src = regions_from_rest(rest, tags, asset, rules)?;
    to_radius_one(&mut src);
    src.validate(asset.regions.len()).map_err(|e| format!("{}: {e}", asset.id))?;
    Ok(src)
}

/// The same in the file's own units (metres), for the studio tier writer.
pub fn regions_from_rest(rest: &RestMesh, tags: &[Tags], asset: &AssetRow, rules: Option<&AssetRules>) -> Result<SourceMesh, String> {
    let n_regions = asset.regions.len();
    let region = |id: &str| asset.region(id).map(|r| r as u8).ok_or_else(|| format!("{}: the rules name `{id}`, not a region of the row", asset.id));
    if tags.len() != rest.positions.len() {
        return Err(format!("{}: {} tags for {} vertices", asset.id, tags.len(), rest.positions.len()));
    }
    // The steps with their region indices resolved once.
    let steps: Vec<(&Step, u8)> = match rules {
        Some(r) => r.steps.iter().map(|s| region(&s.region).map(|i| (s, i))).collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    let mut regions = Vec::with_capacity(rest.positions.len());
    for (v, t) in tags.iter().enumerate() {
        let material = rest.material_name(v);
        let chart = rest.chart.get(v).copied().unwrap_or_default().map(|b| f32::from(b) / 255.0);
        let by_steps = || steps.iter().find(|(s, _)| s.matches(material, chart)).map(|(_, i)| *i);
        let r = match t.part {
            Some(p) if (p as usize) < n_regions => p as u8,
            Some(p) => return Err(format!("{}: vertex {v} has {PART_ATTRIBUTE} {p}, the row has {n_regions} regions", asset.id)),
            None => match (t.charted && !steps.is_empty(), asset.region(material)) {
                (true, _) => by_steps().ok_or_else(|| format!("{}: vertex {v} matches no step (the last step must match anything)", asset.id))?,
                (false, Some(i)) if !material.is_empty() => i as u8,
                _ => by_steps().unwrap_or(0),
            },
        };
        regions.push(r);
    }
    let mut src = SourceMesh { positions: rest.positions.clone(), normals: rest.normals.clone(), indices: rest.indices.clone(), regions };
    for p in rules.map_or(&[][..], |r| &r.proxies) {
        let r = region(&p.region)?;
        if src.regions.contains(&r) {
            continue;
        }
        let solid = match p.shape {
            ProxyShape::Mane => proxy::mane(rest),
            ProxyShape::Tail => proxy::tail(rest),
        };
        let Some(solid) = solid else {
            return Err(format!("{}: the tier has no anchors to build its `{}` from", asset.id, p.region));
        };
        let base = src.positions.len() as u32;
        src.positions.extend(&solid.positions);
        src.normals.extend(&solid.normals);
        src.indices.extend(solid.indices.iter().map(|i| base + i));
        src.regions.extend(std::iter::repeat_n(r, solid.positions.len()));
    }
    src.validate(n_regions).map_err(|e| format!("{}: {e}", asset.id))?;
    Ok(src)
}

/// Centre on the bounds and scale so the farthest vertex is at radius 1.
pub fn to_radius_one(src: &mut SourceMesh) {
    let (lo, hi) = bounds(&src.positions);
    let c: V3 = scale([lo[0] + hi[0], lo[1] + hi[1], lo[2] + hi[2]], 0.5);
    let r = src.positions.iter().map(|p| len(sub(*p, c))).fold(0.0f32, f32::max);
    let k = if r > 0.0 { 1.0 / r } else { 1.0 };
    for p in &mut src.positions {
        *p = scale(sub(*p, c), k);
    }
}

/// Vertices per region (the row's order).
pub fn region_counts(src: &SourceMesh, region_count: usize) -> Vec<usize> {
    let mut n = vec![0usize; region_count];
    for &r in &src.regions {
        if let Some(c) = n.get_mut(usize::from(r)) {
            *c += 1;
        }
    }
    n
}

/// What a ready source must hold: every region the row lists has vertices
/// (a kept or following region too: the voxel horse needs its legs), and a
/// thin region at least `min_thin` of them. The check and the tests use it.
pub fn coverage_problems(src: &SourceMesh, asset: &AssetRow, min_thin: usize) -> Vec<String> {
    let counts = region_counts(src, asset.regions.len());
    asset
        .regions
        .iter()
        .zip(&counts)
        .filter_map(|(r, &c)| match (c, r.thin) {
            (0, _) => Some(format!("{}: region `{}` has no vertices in the source", asset.id, r.id)),
            (c, true) if c < min_thin => Some(format!("{}: thin region `{}` has only {c} vertices (at least {min_thin})", asset.id, r.id)),
            _ => None,
        })
        .collect()
}

/// A source tier file: one primitive per region (a glTF material named
/// after the region, coloured `colours[region]`, linear RGB), positions,
/// normals and the [`PART_ATTRIBUTE`] byte (stride 4, as glTF wants vertex
/// attributes aligned). Unskinned, no clips, no textures: what the studio
/// tiers are and what [`source_from_glb`] reads back exactly. The tool
/// `examples/studio_assets.rs` writes these before `cargo xtask convert`.
pub fn source_to_glb(src: &SourceMesh, asset: &AssetRow, colours: &[[f32; 3]], generator: &str) -> Result<Vec<u8>, String> {
    use serde_json::{Value, json};

    use crate::GeometryGenerator;
    let parts = crate::Passthrough.generate(src, asset, &crate::Geometry::None)?;
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut prims, mut materials) = (Vec::<Value>::new(), Vec::<Value>::new(), Vec::<Value>::new(), Vec::<Value>::new());
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], stride: Option<usize>, target: u32| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target});
        if let Some(st) = stride {
            v["byteStride"] = json!(st);
        }
        bin.extend_from_slice(bytes);
        views.push(v);
        views.len() - 1
    };
    for part in &parts.parts {
        let region = asset.regions.get(usize::from(part.region)).ok_or("source: a part past the regions")?;
        let (lo, hi) = bounds(&part.positions);
        let f32s = |v: &[V3]| v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let n = part.positions.len();
        let pv = push_view(&mut bin, &f32s(&part.positions), None, 34962);
        accessors.push(json!({"bufferView": pv, "componentType": 5126, "count": n, "type": "VEC3", "min": lo, "max": hi}));
        let nv = push_view(&mut bin, &f32s(&part.normals), None, 34962);
        accessors.push(json!({"bufferView": nv, "componentType": 5126, "count": n, "type": "VEC3"}));
        let tag: Vec<u8> = (0..n).flat_map(|_| [part.region, 0, 0, 0]).collect();
        let tv = push_view(&mut bin, &tag, Some(4), 34962);
        accessors.push(json!({"bufferView": tv, "componentType": 5121, "count": n, "type": "SCALAR"}));
        let idx: Vec<u8> = part.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let iv = push_view(&mut bin, &idx, None, 34963);
        accessors.push(json!({"bufferView": iv, "componentType": 5125, "count": part.indices.len(), "type": "SCALAR"}));
        let a = accessors.len() - 4;
        // Each region's colour a little different from every other, so a
        // converter that merges equal materials (gltfpack does) never folds
        // two regions into one (the bay's mane and tail are both black).
        let c = colours.get(usize::from(part.region)).copied().unwrap_or([0.6, 0.6, 0.6]).map(|x| (x + 0.004 * f32::from(part.region)).min(1.0));
        materials.push(json!({"name": region.id, "pbrMetallicRoughness": {"baseColorFactor": [c[0], c[1], c[2], 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.85}}));
        prims.push(json!({"attributes": {"POSITION": a, "NORMAL": a + 1, PART_ATTRIBUTE: a + 2}, "indices": a + 3, "material": materials.len() - 1, "mode": 4}));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": generator},
        "extras": {"studio_source": asset.id},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": asset.id.replace('.', "_"), "mesh": 0}],
        "meshes": [{"name": asset.id, "primitives": prims}],
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}],
    });
    let mut js = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    Ok(glb_container(&js, &bin))
}

/// Where the studio's model tiers and thumbnails live (the material
/// viewer's `models/`, served as the page's `models/`).
pub const APP_MODELS: &str = "apps/material-view/assets/models";
/// A thin region (legs, ears, mane, tail...) needs at least this many
/// vertices in a source, or the shapes lose it.
pub const MIN_THIN_VERTICES: usize = 12;

/// The file a ready model asset draws on device `tier`: `<model>.<tier>.glb`,
/// else the shared `<model>.glb` (the engine's own rule, `stream::model_file`).
pub fn tier_file(models: &std::path::Path, model: &str, tier: &str) -> Option<std::path::PathBuf> {
    [format!("{model}.{tier}.glb"), format!("{model}.glb")].into_iter().map(|f| models.join(f)).find(|p| p.is_file())
}

/// An asset's source on device `tier`: a code shape is built, a model tier
/// read from `models` (radius 1, regions per vertex).
pub fn load_source(models: &std::path::Path, asset: &AssetRow, tier: &str) -> Result<SourceMesh, String> {
    use evani_studio::assets::SourceKind;
    match asset.source.kind {
        SourceKind::Code => {
            let shape = asset.source.shape.as_deref().unwrap_or_default();
            crate::code::code_source(shape).ok_or_else(|| format!("{}: no code shape `{shape}` (evani-style-geo code.rs)", asset.id))
        }
        SourceKind::Model | SourceKind::BlenderScript => {
            let model = asset.source.model.as_deref().unwrap_or_default();
            let file = tier_file(models, model, tier).ok_or_else(|| format!("{}: tier `{tier}` has no `{model}.{tier}.glb` or `{model}.glb` in {}", asset.id, models.display()))?;
            let glb = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            source_from_glb(&glb, asset)
        }
    }
}

/// Errors become tools: every ready asset of the catalogue loads on every
/// device tier (`assets/tiers/*.json`) as a valid source with every region
/// present and its thin regions kept, lists every tier, and has its
/// thumbnail. `(what, fix)` per problem; `cargo xtask check` runs it.
pub fn asset_problems(root: &std::path::Path, c: &evani_studio::Catalogs) -> Vec<(String, String)> {
    use evani_studio::assets::AssetStatus;
    let models = root.join(APP_MODELS);
    let mut device: Vec<String> = std::fs::read_dir(root.join("assets/tiers"))
        .map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(str::to_owned)).collect())
        .unwrap_or_default();
    device.sort();
    let mut out = Vec::new();
    const TOOL: &str = "cargo run -p evani-style-geo --example studio_assets --";
    for a in c.assets.assets.iter().filter(|a| a.status == AssetStatus::Ready) {
        let mut bad = |what: String, fix: String| out.push((format!("studio asset `{}`: {what}", a.id), fix));
        match &a.thumbnail {
            None => bad("no thumbnail".into(), format!("Draw it with `{TOOL} thumb {} {APP_MODELS}/thumbs/{}.png` and set `thumbnail` in catalog/assets/assets.json.", a.id, a.id)),
            Some(t) if !models.join(t).is_file() => bad(format!("thumbnail `{t}` isn't in {APP_MODELS}"), format!("Draw it with `{TOOL} thumb {} {APP_MODELS}/{t}`.", a.id)),
            Some(_) => {}
        }
        let missing: Vec<&str> = device.iter().filter(|t| !a.tiers.contains(t)).map(String::as_str).collect();
        if !missing.is_empty() {
            bad(format!("tiers: missing {}", missing.join(", ")), "A ready asset lists every device tier (assets/tiers/*.json) in `tiers`, and each must load.".into());
        }
        for t in a.tiers.iter().filter(|t| !device.contains(t)) {
            bad(format!("tiers: `{t}` is no device tier"), "Use the names of assets/tiers/*.json.".into());
        }
        // Each distinct file is read once; code shapes are built once.
        let mut seen = std::collections::BTreeSet::new();
        for t in a.tiers.iter().filter(|t| device.contains(t)) {
            let key = a.source.model.as_deref().and_then(|m| tier_file(&models, m, t));
            if !seen.insert(key) {
                continue;
            }
            let fix = "Re-make the asset's studio tier (studio_assets tier, then cargo xtask convert) or fix its region rules in catalog/assets/source_rules.json.";
            match load_source(&models, a, t) {
                Err(e) => bad(format!("tier `{t}`: {e}"), fix.into()),
                Ok(src) => {
                    if let Err(e) = src.validate(a.regions.len()) {
                        bad(format!("tier `{t}`: {e}"), fix.into());
                    }
                    for p in coverage_problems(&src, a, MIN_THIN_VERTICES) {
                        bad(format!("tier `{t}`: {p}"), fix.into());
                    }
                }
            }
        }
    }
    out
}

/// WP3's maths helpers its voxel and low-poly code uses (studio-shapes):
/// named here until that code is on this branch, so the crate builds
/// without dead-code warnings. Drop it when voxel.rs lands.
#[allow(dead_code, clippy::type_complexity)]
const WP3_HELPERS: (fn(V3, V3, V3) -> V3, fn(V3, V3, V3, V3) -> V3, fn(&[u8]) -> u64, fn([[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3])) =
    (crate::math::tri_normal, crate::math::closest_on_triangle, crate::math::fnv1a, crate::math::eigen_sym);

#[cfg(test)]
mod tests {
    use evani_studio::Catalogs;

    use super::*;
    use crate::GeometryGenerator;

    fn root() -> std::path::PathBuf {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).to_path_buf()
    }

    fn catalogs() -> Catalogs {
        Catalogs::load(&root()).unwrap()
    }

    /// The built-in rules agree with the asset catalogue.
    #[test]
    fn the_rules_file_names_only_real_regions() {
        let c = catalogs();
        let r = SourceRules::builtin().unwrap();
        assert_eq!(r.problems(&c.assets), Vec::<String>::new());
    }

    /// The bug class this guards: a rule naming a region the row lacks, a
    /// list with no catch-all (a vertex would be guessed), an empty range.
    #[test]
    fn bad_rules_are_named() {
        let c = catalogs();
        let bad = r#"{"version":1,"note":"","assets":{"animal.horse":{"steps":[{"region":"wing","along":[0.5,0.4]}],"proxies":[{"region":"mane","shape":"mane"},{"region":"mane","shape":"tail"}]},"animal.unicorn":{"steps":[{"region":"coat"}]}}}"#;
        let p = SourceRules::parse(bad).unwrap().problems(&c.assets).join("\n");
        for want in ["`wing` isn't one of the row's regions", "must match anything", "two proxies for `mane`", "min < max", "animal.unicorn: no such asset"] {
            assert!(p.contains(want), "{want} missing from:\n{p}");
        }
    }

    /// WP2's done-when: the horse's low tier (skinned, with the body chart)
    /// becomes a valid source with every region present, the thin ones
    /// well filled, at radius 1, deterministically.
    #[test]
    fn the_horse_tier_reads_with_every_region() {
        let c = catalogs();
        let horse = c.assets.asset("animal.horse").unwrap();
        let glb = std::fs::read(root().join("tests/fixtures/models/horse/horse.low.glb")).unwrap();
        let src = source_from_glb(&glb, horse).unwrap();
        src.validate(horse.regions.len()).unwrap();
        assert_eq!(coverage_problems(&src, horse, 24), Vec::<String>::new(), "counts {:?}", region_counts(&src, horse.regions.len()));
        let r = src.positions.iter().map(|p| len(*p)).fold(0.0f32, f32::max);
        assert!((r - 1.0).abs() < 1e-4, "radius {r}");
        assert_eq!(src, source_from_glb(&glb, horse).unwrap(), "deterministic");
        // A horse stands on its legs: the hooves are the lowest region.
        let lowest = src.positions.iter().zip(&src.regions).fold((f32::MAX, 0u8), |m, (p, &r)| if p[1] < m.0 { (p[1], r) } else { m });
        assert_eq!(horse.regions[usize::from(lowest.1)].id, "hoof");
    }

    /// Every high-res tier reads the same way (the studio may pick any).
    #[test]
    fn every_horse_tier_reads() {
        let c = catalogs();
        let horse = c.assets.asset("animal.horse").unwrap();
        for t in ["low", "high", "ultra"] {
            let glb = std::fs::read(root().join(format!("tests/fixtures/models/horse/horse.{t}.glb"))).unwrap();
            let src = source_from_glb(&glb, horse).unwrap_or_else(|e| panic!("{t}: {e}"));
            assert_eq!(coverage_problems(&src, horse, 24), Vec::<String>::new(), "{t}");
        }
    }

    /// A written source tier reads back with the same regions per vertex,
    /// and with the part byte stripped (a converter that drops it) the
    /// region-named materials still give the same regions.
    #[test]
    fn a_source_tier_round_trips() {
        let c = catalogs();
        let horse = c.assets.asset("animal.horse").unwrap();
        let glb = std::fs::read(root().join("tests/fixtures/models/horse/horse.low.glb")).unwrap();
        let src = source_from_glb(&glb, horse).unwrap();
        let out = source_to_glb(&src, horse, &[], "test").unwrap();
        let back = source_from_glb(&out, horse).unwrap();
        let parts = crate::Passthrough.generate(&src, horse, &crate::Geometry::None).unwrap();
        let mut want = vec![0usize; 8];
        for p in &parts.parts {
            want[usize::from(p.region)] += p.positions.len();
        }
        assert_eq!(region_counts(&back, 8), want);
        let rest = rest_from_glb(&out).unwrap();
        let none = vec![Tags::default(); rest.positions.len()];
        assert_eq!(source_from_rest(&rest, &none, horse, None).unwrap().regions, back.regions);
    }

    /// The app's studio tier, as `cargo xtask convert` left it (the part
    /// byte dropped, equal materials merged, two detail levels side by
    /// side): every region present, thin ones kept, one horse (not the two
    /// levels on top of each other).
    #[test]
    fn the_converted_studio_tier_reads_with_every_region() {
        let c = catalogs();
        let horse = c.assets.asset("animal.horse").unwrap();
        let glb = std::fs::read(root().join("apps/material-view/assets/models/horse.glb")).unwrap();
        let src = source_from_glb(&glb, horse).unwrap();
        assert_eq!(coverage_problems(&src, horse, 12), Vec::<String>::new(), "counts {:?}", region_counts(&src, 8));
        let one = first_lod_only(&glb).unwrap();
        // The lower level loses its meshes, and the rest reader (which skips
        // later levels itself) sees the same one horse either way.
        let drawn = |b: &[u8]| Gltf::from_glb(b).unwrap().nodes.iter().filter(|n| n.mesh.is_some()).count();
        assert!(one.len() < glb.len() + 64 && drawn(&one) < drawn(&glb), "the lower level is dropped");
        assert_eq!(rest_from_glb(&one).unwrap().positions.len(), rest_from_glb(&glb).unwrap().positions.len(), "one horse, never two");
    }

    /// WP7's done-when: the cow and the dog from the quadruped generator
    /// (`_EVANI_PART` per vertex, kept through the converter by carry.rs),
    /// the studio tier and every rigged tier, read with every region of
    /// their rows present and thin ones kept; the cow stands on its hooves.
    #[test]
    fn the_cow_and_dog_read_with_every_region() {
        let c = catalogs();
        for (id, model, lowest) in [("animal.cow", "cow", Some("hoof")), ("animal.dog", "dog", None)] {
            let row = c.assets.asset(id).unwrap();
            let n = row.regions.len();
            let glb = std::fs::read(root().join(format!("apps/material-view/assets/models/{model}.glb"))).unwrap();
            let src = source_from_glb(&glb, row).unwrap_or_else(|e| panic!("{model} studio: {e}"));
            src.validate(n).unwrap();
            assert_eq!(coverage_problems(&src, row, 12), Vec::<String>::new(), "{model} studio, counts {:?}", region_counts(&src, n));
            for t in ["low", "medium", "high", "ultra"] {
                let glb = std::fs::read(root().join(format!("tests/fixtures/models/{model}/{model}.{t}.glb"))).unwrap();
                let src = source_from_glb(&glb, row).unwrap_or_else(|e| panic!("{model} {t}: {e}"));
                assert_eq!(coverage_problems(&src, row, 24), Vec::<String>::new(), "{model} {t}, counts {:?}", region_counts(&src, n));
                if let Some(want) = lowest {
                    let low = src.positions.iter().zip(&src.regions).fold((f32::MAX, 0u8), |m, (p, &r)| if p[1] < m.0 { (p[1], r) } else { m });
                    assert_eq!(row.regions[usize::from(low.1)].id, want, "{model} {t}");
                }
            }
        }
    }

    /// Errors become tools: the catalogue's ready assets all load on every
    /// tier with their regions, thin parts and thumbnails; and the check
    /// does catch a broken row.
    #[test]
    fn every_ready_asset_loads_on_every_tier() {
        let c = catalogs();
        assert_eq!(asset_problems(&root(), &c), Vec::<(String, String)>::new());
        let mut broken = catalogs();
        let horse = broken.assets.assets.iter_mut().find(|a| a.id == "animal.horse").unwrap();
        horse.tiers.retain(|t| t != "low");
        horse.thumbnail = None;
        horse.source.model = Some("no-such-model".into());
        let found: Vec<String> = asset_problems(&root(), &broken).into_iter().map(|p| p.0).collect();
        for want in ["no thumbnail", "tiers: missing low", "has no `no-such-model"] {
            assert!(found.iter().any(|f| f.contains(want)), "{want}: {found:?}");
        }
    }

    /// A part byte past the row's regions is refused, never wrapped.
    #[test]
    fn a_part_past_the_regions_is_an_error() {
        let c = catalogs();
        let wall = c.assets.asset("building.wall").unwrap();
        let rest = RestMesh { positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], normals: vec![[0.0, 0.0, 1.0]; 3], indices: vec![0, 1, 2], material: vec![255; 3], chart: vec![[0; 4]; 3], ..RestMesh::default() };
        let tag = |p: Option<u32>| Tags { part: p, charted: false };
        let e = source_from_rest(&rest, &[tag(Some(0)), tag(Some(1)), tag(Some(7))], wall, None).unwrap_err();
        assert!(e.contains("_EVANI_PART 7"), "{e}");
        let ok = source_from_rest(&rest, &[tag(Some(0)), tag(Some(1)), tag(Some(1))], wall, None).unwrap();
        assert_eq!(ok.regions, vec![0, 1, 1]);
        // No byte, no material, no rules: the first region.
        assert_eq!(source_from_rest(&rest, &[tag(None); 3], wall, None).unwrap().regions, vec![0, 0, 0]);
        // A material named after a region lands there.
        let named = RestMesh { material: vec![0, 0, 0], material_names: vec!["cap".into()], ..rest.clone() };
        assert_eq!(source_from_rest(&named, &[tag(None); 3], wall, None).unwrap().regions, vec![1, 1, 1]);
    }
}
