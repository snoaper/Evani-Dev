//! The procedural section's library (`catalog/procedural/procedural.json`):
//! the effects the engine draws from code (water, glass, sky...), each with
//! the evani.toml module line a game adds, its headline params (the 3-5
//! sliders shown first), the rest (the Advanced fold) and its presets.
//!
//! A preset is a set of values over the item's `base`: applying one puts
//! every param of the item back to its default, then `base`, then the
//! preset ([`baseline`]), so applying a preset twice, or after another one,
//! gives the same values (presets round-trip). Every name is a registered
//! live param, so the page, the Look Lab and AI agents move the same values.

#[cfg(any(target_arch = "wasm32", test))]
use evani_core::params::ParamType;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_kit_material_view::library::value_for;
use serde_json::Value;

/// The library, embedded (dev data: only the studio app carries it).
pub const PROC_JSON: &str = include_str!("../../../../catalog/procedural/procedural.json");

/// The realism pass's references (`catalog/procedural/references.json`,
/// written by tools/procedural/gen_library.py from the manifest): per
/// library asset, its reference picture (under the viewer's `proc-refs/`,
/// copied from `assets/proc-refs`), its score out of 5, its status and a
/// short note. Dev data: only this viewer carries it, never a game.
#[cfg(any(target_arch = "wasm32", test))]
pub const REFS_JSON: &str = include_str!("../../../../catalog/procedural/references.json");

/// One asset's realism reference.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// The picture, relative to the page (`proc-refs/<feature>.jpg`).
    pub image: String,
    /// The reviewer's score (1-5), once scored.
    pub score: Option<f64>,
    /// `pass`, `needs help` or `in progress`.
    pub status: String,
    pub note: String,
}

/// Library asset `feature`'s reference, if it has one.
#[cfg(any(target_arch = "wasm32", test))]
pub fn reference_of(feature: &str) -> Option<Reference> {
    let root: Value = serde_json::from_str(REFS_JSON).ok()?;
    let r = root.get("assets")?.get(feature)?;
    let image = r.get("reference")?.as_str()?.to_string();
    Some(Reference { image, score: r.get("score").and_then(Value::as_f64), status: s(r, "status"), note: s(r, "note") })
}

/// The viewer material that opens item `id` in the procedural scene.
pub fn material_of(id: &str) -> String {
    format!("studio.proc.{id}")
}

/// The item a viewer material opens (`studio.proc.<id>`).
pub fn item_of(material: &str) -> Option<&str> {
    material.strip_prefix("studio.proc.")
}

/// How the studio's view lays out a water item (staging only, not the effect).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Open water to the horizon with rocks.
    Sea,
    /// A pond in grassy banks.
    Lake,
    /// A long channel between two banks.
    River,
    /// The spectral sea (part `spectral-sea`) to the horizon over a bed at
    /// `material.nature.water.sea_depth`.
    Open,
    /// The spectral sea against a sand beach that rises as the
    /// `material.nature.water.beach_*` params say (the sand follows them).
    Beach,
    /// A patch of evani-grass on bare soil; the kind's index (meadow 0,
    /// lawn 1, mud 2, wheat 3: `evani_grass::Kind`).
    Field(u8),
    /// A procedural library asset (evani-procedural-lib) on the floor, drawn
    /// by the procedural module through its `ProcScene`.
    Asset,
}

impl Shape {
    /// Whether the spectral sea draws this staging (not the nature pass).
    pub fn spectral(self) -> bool {
        matches!(self, Shape::Open | Shape::Beach)
    }
}

/// A preset's staging: the water's shape, the bed's depth (m) and colour,
/// and optionally its own camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stage {
    pub shape: Shape,
    pub depth: f32,
    pub bed: [f32; 3],
    pub camera: Option<StageCam>,
}

/// A preset's camera (the item's heading kept): elevation (deg), distance
/// (m) and the point looked at. The storm presets look low across the sea,
/// as from a ship's deck, so the crests stand up against the sky.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageCam {
    pub el: f32,
    pub dist: f32,
    pub target: [f32; 3],
    /// Vertical field of view (deg): the viewer's usual 35, or a longer
    /// lens for the realism assets ([`REALISM_FRAMING`]).
    pub fov: f32,
}

#[cfg(test)]
impl StageCam {
    /// The eye's height over the water (m).
    pub fn eye_height(&self) -> f32 {
        self.target[1] + self.dist * self.el.to_radians().sin()
    }
}

impl Default for Stage {
    fn default() -> Self {
        Stage { shape: Shape::Sea, depth: 4.0, bed: [0.34, 0.29, 0.21], camera: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub params: Vec<(String, Value)>,
    pub stage: Option<Stage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub about: String,
    /// Built and shown; `false` = planned ("coming soon").
    pub ready: bool,
    /// The evani.toml `[modules]` line a game adds.
    pub module: String,
    pub crate_name: String,
    pub surface: String,
    pub headline: Vec<String>,
    pub advanced: Vec<String>,
    pub base: Vec<(String, Value)>,
    pub presets: Vec<Preset>,
    /// The param the panel's Ultra/High/Medium/Low switch sets (-1 = by the
    /// device's tier), if the item has quality levels.
    pub quality: Option<String>,
    /// The lowest level each param shows on (`low`, `medium`, `high`,
    /// `ultra`; unlisted: every level), for the panel's labels.
    pub levels: Vec<(String, String)>,
    /// The switch's buttons after Auto: label and the value it writes to
    /// `quality` (`quality_levels` in the JSON; else [`QUALITY_LEVELS`]'s).
    /// The nature pass's own quality has one top step for High and Ultra
    /// and starts Low at 1, so the lake and river list theirs.
    pub quality_levels: Vec<(String, f32)>,
    /// The procedural library's category (`water`; empty: none).
    pub category: String,
    /// The library asset it is (`water-ocean-waves`), if any.
    pub asset: Option<String>,
    /// Whose staging and viewer camera it uses: its own id, the item an
    /// alias points at (`storm-sea` is the `ocean` item on its storm
    /// preset), or `library` for library assets.
    pub stage_as: String,
    /// The asset's on/off switch (`procedural.<category>.<asset>.on`).
    pub on_param: Option<String>,
}

/// The light every library asset is shown in: a low sun 45 degrees to the
/// camera's right (the library camera looks from azimuth 30; the sun at
/// 75, 26 up), so the side facing the viewer is lit, the hills keep their
/// relief, and the look's rim (strongest away from the sun) stays a thin
/// edge, not a pale ring round a dark, back-lit shape (review round 3: the
/// stand-in rock kept the ocean's low sun from behind it). Round 4's tries:
/// high and straight from the camera flattened the hills and the cube; from
/// the side (110) turned half the rock's outline to the shaded rim.
///
/// Round 8 (the reference pictures): a low warm sun from the right, as in
/// all eight references, so the shadow falls to the left and the
/// right-hand faces and the top are lit (bearing 110, the camera at 30; 32
/// degrees up: the engine studio's `SUN`, evani_procedural::studio).
pub const LIBRARY_LIGHT: [(&str, f64); 3] = [("sky.sun_elevation", 32.0), ("sky.sun_azimuth", 110.0), ("sky.cloud_coverage", 0.2)];

/// The sun's height in a realism asset's Close-up preset: grazing, so real
/// relief (a carved crack, chipped edges, the grass blades) throws shadows.
pub const GRAZING_SUN: f64 = 12.0;

/// The realism assets' framing, matched to their reference pictures (16:9,
/// the subject filling most of the height): asset, camera height (deg),
/// share of the view the subject fills, vertical field of view (deg; 20 is
/// about a 70 mm lens on full frame, flatter than the viewer's 35).
pub const REALISM_FRAMING: [(&str, f32, f32, f32); 8] = [
    ("geometry-cube-cuboid", 17.0, 0.84, 20.0),
    ("geometry-bevelled-box", 18.0, 0.86, 20.0),
    ("geometry-uv-sphere", 13.0, 0.8, 20.0),
    ("geometry-icosphere", 13.0, 0.82, 20.0),
    ("geometry-cylinder", 12.0, 0.9, 20.0),
    ("geometry-cone", 10.0, 0.88, 20.0),
    // The terrains as their references: closer, a wider lens (about 28 mm, as the references),
    // so evani-grass draws its blades over the near half. Round 9: twice the
    // fill (the view's subject fit pulls every shot back about half, and the
    // r8 plots filled 40% of the frame's width against the references' 95%).
    ("terrain-flat-terrain", 33.0, 1.75, 45.0),
    ("terrain-rolling-hills", 35.0, 1.7, 45.0),
];

/// A realism asset's close-up: the same heading and lens, a little higher,
/// a third as far, looking at the upper front edge (where a cube's crack
/// starts, a sphere's limb is, a field's near edge lies).
pub fn close_cam(lo: [f32; 3], hi: [f32; 3], c: StageCam) -> StageCam {
    let target = [(lo[0] + hi[0]) * 0.5, lo[1] + (hi[1] - lo[1]) * 0.8, lo[2] + (hi[2] - lo[2]) * 0.8];
    StageCam { el: c.el + 6.0, dist: c.dist * 0.36, target, fov: c.fov }
}

/// [`library_cam`] for asset `feature`: its [`REALISM_FRAMING`] if it has
/// one (a 16:9 view), else the library's usual camera.
pub fn library_cam_for(feature: &str, lo: [f32; 3], hi: [f32; 3]) -> StageCam {
    match REALISM_FRAMING.iter().find(|r| r.0 == feature) {
        Some(&(_, el, fill, fov)) => fit_cam(lo, hi, el, fill, fov, 16.0 / 9.0),
        None => library_cam(lo, hi),
    }
}

/// The library camera for an asset whose bounds are `lo`..`hi`: from the
/// viewer's usual heading (azimuth 30, 24 degrees up), looking at the
/// bounds' middle, as close as lets every corner draw inside
/// [`LIBRARY_FILL`] of a 16:10 view (a narrower view is pulled back by the
/// subject fit). The old fixed 2.8 times the longest side left flat and
/// long assets small in the frame.
pub fn library_cam(lo: [f32; 3], hi: [f32; 3]) -> StageCam {
    fit_cam(lo, hi, 24.0, LIBRARY_FILL, evani_kit_material_view::orbit::FOV_Y_DEG, 1.6)
}

/// The camera from azimuth 30 and `el_deg` up that fits every corner of
/// `lo`..`hi` inside `fill` of a view `fov` degrees high and `aspect` wide.
pub fn fit_cam(lo: [f32; 3], hi: [f32; 3], el_deg: f32, fill: f32, fov: f32, aspect: f32) -> StageCam {
    let (el, az) = (el_deg.to_radians(), 30f32.to_radians());
    // A little under the middle, so the asset sits on its ground.
    let target: [f32; 3] = std::array::from_fn(|k| lo[k] + (hi[k] - lo[k]) * if k == 1 { 0.45 } else { 0.5 });
    let back = [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()];
    let right = [az.cos(), 0.0, -az.sin()];
    let up = [-el.sin() * az.sin(), el.cos(), -el.sin() * az.cos()];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let tan_v = (fov.to_radians() * 0.5).tan() * fill;
    let tan_h = tan_v * aspect;
    let mut dist = 0.4f32;
    for i in 0..8 {
        let c = [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
        let rel: [f32; 3] = std::array::from_fn(|k| c[k] - target[k]);
        let z = dot(rel, back);
        dist = dist.max(z + dot(rel, right).abs() / tan_h).max(z + dot(rel, up).abs() / tan_v);
    }
    StageCam { el: el_deg, dist: dist.min(400.0), target, fov }
}

/// The share of the view's height (and width) a library asset may fill.
pub const LIBRARY_FILL: f32 = 0.8;

/// The longest side (m) the viewer shows an asset at. The renderer draws to
/// 1 km and the library camera stands at most 400 m off, so a bigger asset
/// (the mountains, a 4.4 km massif at game scale) is shown as a scale model
/// this size (2026-11-07); every smaller asset is shown as it is.
pub const VIEW_MAX: f32 = 300.0;

/// The scale the viewer shows an asset with bounds `lo`..`hi` at (1 up to
/// [`VIEW_MAX`]).
pub fn view_scale(lo: [f32; 3], hi: [f32; 3]) -> f32 {
    let side = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
    if side > VIEW_MAX { VIEW_MAX / side } else { 1.0 }
}

/// Bounds `lo`..`hi` as the viewer shows them ([`view_scale`]).
pub fn view_bounds(lo: [f32; 3], hi: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let k = view_scale(lo, hi);
    (lo.map(|v| v * k), hi.map(|v| v * k))
}

/// Whether `item` is a library stand-in (its generator not built yet).
#[cfg(any(target_arch = "wasm32", test))]
pub fn is_standin(item: &Item) -> bool {
    item.stage_as == "library" && item.icon == STANDIN_ICON
}

/// The icon of a library stand-in (a built generator has "◆").
pub const STANDIN_ICON: &str = "◇";

/// The viewer material that shows `item` (its staging's).
pub fn viewer_material(item: &Item) -> String {
    material_of(&item.stage_as)
}

/// The panel's quality switch: label and the param's value.
#[cfg(any(target_arch = "wasm32", test))]
pub const QUALITY_LEVELS: [(&str, f32); 5] = [("Auto", -1.0), ("Ultra", 3.0), ("High", 2.0), ("Medium", 1.0), ("Low", 0.0)];

impl Item {
    /// The quality switch's buttons, Auto (-1: by the device's tier) first;
    /// empty when the item has no quality setting.
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn quality_buttons(&self) -> Vec<(String, f32)> {
        if self.quality.is_none() {
            return Vec::new();
        }
        let mut out = vec![("Auto".to_string(), -1.0)];
        if self.quality_levels.is_empty() {
            out.extend(QUALITY_LEVELS.iter().skip(1).map(|(n, v)| (n.to_string(), *v)));
        } else {
            out.extend(self.quality_levels.iter().cloned());
        }
        out
    }
}

impl Item {
    /// "High and Ultra", "Medium and up", "Ultra only" for a param that
    /// doesn't show on every level.
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn level_note(&self, param: &str) -> Option<&'static str> {
        match self.levels.iter().find(|(n, _)| n == param).map(|(_, l)| l.as_str()) {
            Some("ultra") => Some("Ultra only"),
            Some("high") => Some("High and Ultra"),
            Some("medium") => Some("Medium and up"),
            _ => None,
        }
    }
}

impl Item {
    /// Every param the item touches, headline first, each once.
    pub fn params(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let names = self.headline.iter().chain(&self.advanced).cloned();
        let names = names.chain(self.base.iter().map(|(n, _)| n.clone()));
        let names = names.chain(self.presets.iter().flat_map(|p| p.params.iter().map(|(n, _)| n.clone())));
        for n in names {
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out
    }

    pub fn preset(&self, id: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcLib {
    pub items: Vec<Item>,
    /// The procedural library's categories (id, name), in its order; empty
    /// when the viewer is built without the library.
    pub categories: Vec<(String, String)>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn names(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn pairs(v: &Value, k: &str) -> Vec<(String, Value)> {
    v.get(k).and_then(Value::as_object).map(|o| o.iter().map(|(n, x)| (n.clone(), x.clone())).collect()).unwrap_or_default()
}

fn rgb(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let c = |i: usize| a[i].as_f64().map(|x| x as f32);
    Some([c(0)?, c(1)?, c(2)?])
}

fn stage(v: &Value) -> Result<Option<Stage>, String> {
    let Some(st) = v.get("stage") else { return Ok(None) };
    let d = Stage::default();
    let shape = match st.get("shape").and_then(Value::as_str).unwrap_or("sea") {
        "sea" => Shape::Sea,
        "lake" => Shape::Lake,
        "river" => Shape::River,
        "open" => Shape::Open,
        "beach" => Shape::Beach,
        "meadow" => Shape::Field(0),
        "lawn" => Shape::Field(1),
        "mud" => Shape::Field(2),
        "wheat" => Shape::Field(3),
        "asset" => Shape::Asset,
        other => return Err(format!("unknown stage shape `{other}` (sea, lake, river, open, beach, meadow, lawn, mud, wheat, asset)")),
    };
    let depth = st.get("depth").and_then(Value::as_f64).map_or(d.depth, |x| x as f32);
    let bed = st.get("bed").and_then(rgb).unwrap_or(d.bed);
    let camera = match st.get("camera") {
        None => None,
        Some(c) => {
            let num = |k: &str| c.get(k).and_then(Value::as_f64).map(|x| x as f32);
            let (Some(el), Some(dist), Some(target)) = (num("el"), num("dist"), c.get("target").and_then(rgb)) else {
                return Err("stage camera: el, dist and target [x, y, z] are needed".into());
            };
            Some(StageCam { el, dist, target, fov: evani_kit_material_view::orbit::FOV_Y_DEG })
        }
    };
    Ok(Some(Stage { shape, depth, bed, camera }))
}

/// `quality_levels`: `[["High and Ultra", 3.0], ...]` (empty when absent).
fn quality_levels(v: &Value) -> Result<Vec<(String, f32)>, String> {
    let Some(list) = v.get("quality_levels") else { return Ok(Vec::new()) };
    let bad = || "quality_levels: a list of [label, value] pairs".to_string();
    let mut out = Vec::new();
    for e in list.as_array().ok_or_else(bad)? {
        let a = e.as_array().filter(|a| a.len() == 2).ok_or_else(bad)?;
        let (Some(name), Some(x)) = (a[0].as_str(), a[1].as_f64()) else { return Err(bad()) };
        out.push((name.to_string(), x as f32));
    }
    Ok(out)
}

impl ProcLib {
    pub fn parse(text: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| format!("procedural.json: {e}"))?;
        let list = root.get("items").and_then(Value::as_array).ok_or("procedural.json: no `items`")?;
        let mut items: Vec<Item> = Vec::new();
        let mut aliases: Vec<(Value, String)> = Vec::new();
        for it in list {
            let id = s(it, "id");
            if id.is_empty() {
                return Err("procedural.json: an item without an id".into());
            }
            let mut presets = Vec::new();
            for p in it.get("presets").and_then(Value::as_array).into_iter().flatten() {
                presets.push(Preset { id: s(p, "id"), name: s(p, "name"), params: pairs(p, "params"), stage: stage(p).map_err(|e| format!("procedural.json: {id}: {e}"))? });
            }
            let ready = it.get("status").and_then(Value::as_str) == Some("ready");
            if ready && presets.is_empty() {
                return Err(format!("procedural.json: {id} is ready but has no presets"));
            }
            if let Some(target) = it.get("alias").and_then(Value::as_str) {
                aliases.push((it.clone(), target.to_string()));
                continue;
            }
            let stage_as = it.get("stage_as").and_then(Value::as_str).map_or_else(|| id.clone(), str::to_string);
            items.push(Item {
                stage_as,
                on_param: None,
                category: s(it, "category"),
                asset: it.get("asset").and_then(Value::as_str).map(str::to_string),
                name: s(it, "name"),
                icon: s(it, "icon"),
                about: s(it, "about"),
                ready,
                module: s(it, "module"),
                crate_name: s(it, "crate"),
                surface: s(it, "surface"),
                headline: names(it, "headline"),
                advanced: names(it, "advanced"),
                base: pairs(it, "base"),
                presets,
                quality: it.get("quality").and_then(Value::as_str).map(str::to_string),
                levels: it.get("levels").and_then(Value::as_object).map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|l| (k.clone(), l.to_string()))).collect()).unwrap_or_default(),
                quality_levels: quality_levels(it).map_err(|e| format!("procedural.json: {id}: {e}"))?,
                id,
            });
        }
        // An alias is its target item under another name and category, its
        // own preset first (the storm sea is the ocean on its storm preset).
        for (a, target) in aliases {
            let id = s(&a, "id");
            let Some(t) = items.iter().find(|i| i.id == target).cloned() else {
                return Err(format!("procedural.json: {id} is an alias of `{target}`, which is not an item"));
            };
            let about = s(&a, "about");
            let mut it = Item { id: id.clone(), name: s(&a, "name"), category: s(&a, "category"), asset: a.get("asset").and_then(Value::as_str).map(str::to_string), ..t };
            if !about.is_empty() {
                it.about = about;
            }
            if let Some(first) = a.get("preset").and_then(Value::as_str) {
                let Some(i) = it.presets.iter().position(|p| p.id == first) else {
                    return Err(format!("procedural.json: {id}: `{target}` has no preset `{first}`"));
                };
                let p = it.presets.remove(i);
                it.presets.insert(0, p);
            }
            items.push(it);
        }
        Ok(ProcLib { items, categories: Vec::new() })
    }

    /// Add the procedural library (evani-procedural-lib's registry): its
    /// categories, each JSON item's switch, and an item for every asset no
    /// JSON item stands for (the built generators and the placeholders),
    /// staged on the floor with a camera that fits its default size.
    #[cfg(feature = "procedural")]
    pub fn add_library(&mut self, r: &evani_procedural_lib::base::Registry) {
        use evani_procedural_lib::base::{Path, REALISM, Registry, Values};
        self.categories = r.categories().iter().map(|c| (c.id.to_string(), c.name.to_string())).collect();
        for it in &mut self.items {
            match it.asset.as_deref().and_then(|f| r.get(f)) {
                Some(a) => it.on_param = Some(format!("{}.on", a.info().prefix())),
                // Not compiled into this build: no switch, no category.
                None => {
                    it.asset = None;
                    it.category.clear();
                }
            }
        }
        for a in r.assets() {
            let f = a.info().feature();
            if self.items.iter().any(|i| i.ready && i.asset.as_deref() == Some(f.as_str())) {
                continue;
            }
            let specs = Registry::specs(a);
            let fit = a.generate(&Values::defaults(&specs), 1, REALISM).bounds().map(|(lo, hi)| view_bounds(lo, hi)).map(|(lo, hi)| library_cam_for(&f, lo, hi));
            let placeholder = evani_procedural_lib::base::placeholder::is_placeholder(a);
            let built = a.info().path == Path::Cpu && !placeholder;
            let about = if placeholder {
                format!("{}: a stand-in made from its name until its generator is built. Style: Cinematic Realism (the others fall back to it).", a.info().name)
            } else {
                format!("{}: built from code on the CPU and seeded; every setting rebuilds it at once. Style: Cinematic Realism (the others fall back to it).", a.info().name)
            };
            let names: Vec<String> = r.param_names(&f).into_iter().filter(|n| !n.ends_with(".on")).collect();
            let stage = |camera| Some(Stage { shape: Shape::Asset, depth: 0.0, bed: [0.3, 0.3, 0.3], camera });
            let mut presets = vec![Preset { id: "default".into(), name: "Default".into(), params: Vec::new(), stage: stage(fit) }];
            // A finish previewed on everyday forms (Chris, 2026-11-04: "show a
            // door, a table, a fence ... so they can see how the material
            // behaves in other forms"): one preset per form, its own camera.
            if let Some(pv) = names.iter().find(|n| n.ends_with(".preview")) {
                presets[0].params.push((pv.clone(), Value::from(0.0)));
                for (k, form) in evani_procedural_lib::base::forms::FORMS.iter().enumerate().skip(1) {
                    let Some(parts) = evani_procedural_lib::base::forms::form(k as f32, 0.3, 1) else { continue };
                    let (lo, hi) = parts.iter().flat_map(|(m, _)| m.vertices.iter()).fold(([f32::MAX; 3], [f32::MIN; 3]), |(l, h), v| (std::array::from_fn(|i| l[i].min(v[i])), std::array::from_fn(|i| h[i].max(v[i]))));
                    let name = if *form == "steps" { "On steps".to_string() } else { format!("On a {form}") };
                    presets.push(Preset { id: format!("on-{form}"), name, params: vec![(pv.clone(), Value::from(k as f64))], stage: stage(Some(library_cam(lo, hi))) });
                }
            }
            // Round 8: a realism asset's close-up under a grazing sun, right
            // after its default (real relief: the reviewer sees depth).
            if let (Some(c), Some((lo, hi))) = (fit, REALISM_FRAMING.iter().any(|r| r.0 == f).then(|| a.generate(&Values::defaults(&specs), 1, REALISM).bounds().map(|(lo, hi)| view_bounds(lo, hi))).flatten()) {
                presets.insert(1, Preset { id: "close-up".into(), name: "Close-up".into(), params: vec![("sky.sun_elevation".into(), Value::from(GRAZING_SUN))], stage: stage(Some(close_cam(lo, hi, c))) });
            }
            // A landform's live rock, grass and snow mix (Chris, 2026-11-04):
            // Rocky, Grassy and Snow-capped.
            if let (Some(rk), Some(sn)) = (names.iter().find(|n| n.ends_with(".rock")), names.iter().find(|n| n.ends_with(".snow"))) {
                for (id, name, rock, snow) in evani_procedural_lib::base::land::MIX_PRESETS {
                    presets.push(Preset { id: id.into(), name: name.into(), params: vec![(rk.clone(), Value::from(rock as f64)), (sn.clone(), Value::from(snow as f64))], stage: stage(fit) });
                }
            }
            // Round 9 (Chris's concrete spec): the concrete presets, live
            // (`<asset>.concrete`), so an AI or a person picks one by name.
            if let Some(cp) = names.iter().find(|n| n.ends_with(".concrete")) {
                for (k, name) in evani_procedural_lib::base::surface::CONCRETE_PRESETS.iter().enumerate() {
                    let id = format!("concrete-{}", name.split([',', ' ']).next().unwrap_or("preset"));
                    presets.push(Preset { id, name: format!("Concrete: {name}"), params: vec![(cp.clone(), Value::from(k as f64))], stage: stage(fit) });
                }
            }
            self.items.push(Item {
                id: f.clone(),
                name: a.info().name.to_string(),
                icon: if built { "◆".into() } else { STANDIN_ICON.into() },
                about,
                ready: true,
                module: format!("procedural = [\"{f}\"]"),
                crate_name: "evani-procedural-lib".into(),
                surface: "ProcScene (evani_procedural::ProcScene::place)".into(),
                headline: names,
                advanced: Vec::new(),
                base: LIBRARY_LIGHT.iter().map(|(n, v)| (n.to_string(), Value::from(*v))).collect(),
                presets,
                quality: None,
                levels: Vec::new(),
                quality_levels: Vec::new(),
                category: a.info().category.to_string(),
                on_param: Some(format!("{}.on", a.info().prefix())),
                asset: Some(f),
                stage_as: "library".into(),
            });
        }
    }

    /// A viewer built without the procedural library has no grass module:
    /// its grass items go.
    #[cfg(not(feature = "procedural"))]
    pub fn without_library(&mut self) {
        self.items.retain(|i| i.stage_as != "grass");
    }

    /// The ready items of category `id`, in library order.
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn in_category<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.ready().filter(move |i| i.category == id)
    }

    /// The embedded library.
    pub fn load() -> Result<Self, String> {
        Self::parse(PROC_JSON)
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn ready(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| i.ready)
    }

    /// Every param any ready item touches, each once.
    pub fn all_params(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in self.ready().flat_map(Item::params) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out
    }

    /// Names a ready item lists that the registry lacks, or values it can't
    /// take (`item: name`).
    pub fn unknown(&self, reg: &ParamRegistry) -> Vec<String> {
        let mut bad = Vec::new();
        for it in self.ready() {
            for n in it.params() {
                if reg.id(&n).is_none() {
                    bad.push(format!("{}: {n}", it.id));
                }
            }
            for (n, v) in it.base.iter().chain(it.presets.iter().flat_map(|p| p.params.iter())) {
                let Some(def) = reg.id(n).and_then(|id| reg.def(id)) else { continue };
                // A value the param can't take, or one it would clamp.
                let out = |x: f32| x < def.min - 1e-6 || x > def.max + 1e-6;
                let fits = match value_for(def, v) {
                    Some(ParamValue::F32(x)) => !out(x),
                    Some(ParamValue::Color(c) | ParamValue::Vec3(c)) => !c.iter().any(|x| out(*x)),
                    Some(_) => true,
                    None => false,
                };
                if !fits {
                    bad.push(format!("{}: {n} = {v} (range {} to {})", it.id, def.min, def.max));
                }
            }
        }
        bad
    }
}

/// What `item`'s params are with `preset` applied: each its default, then
/// the item's base, then the preset's value (names the registry lacks are
/// skipped).
pub fn baseline(reg: &ParamRegistry, item: &Item, preset: Option<&Preset>) -> Vec<(String, ParamValue)> {
    let mut out = Vec::new();
    for n in item.params() {
        let Some(def) = reg.id(&n).and_then(|id| reg.def(id)) else { continue };
        let from = |list: &[(String, Value)]| list.iter().find(|(k, _)| *k == n).and_then(|(_, v)| value_for(def, v));
        let v = preset.and_then(|p| from(&p.params)).or_else(|| from(&item.base)).unwrap_or(def.default);
        out.push((n, v));
    }
    out
}

/// The current values of `names` (those registered).
pub fn read(reg: &ParamRegistry, names: &[String]) -> Vec<(String, ParamValue)> {
    names.iter().filter_map(|n| reg.id(n).and_then(|id| reg.value(id)).map(|v| (n.clone(), v))).collect()
}

/// Write `values` where they differ from what is set.
pub fn write(reg: &mut ParamRegistry, values: &[(String, ParamValue)]) {
    for (n, v) in values {
        if let Some(id) = reg.id(n)
            && reg.value(id) != Some(*v)
        {
            let _ = reg.set(id, *v);
        }
    }
}

/// Apply `preset` of `item` (see [`baseline`]).
pub fn apply_preset(reg: &mut ParamRegistry, item: &Item, preset: Option<&Preset>) {
    let values = baseline(reg, item, preset);
    write(reg, &values);
}

#[cfg(any(target_arch = "wasm32", test))]
/// Whether `now` differs from `base` by more than half the param's step
/// (or a small tolerance), the page's "changed" marker.
pub fn changed(ty: ParamType, step: Option<f32>, now: ParamValue, base: ParamValue) -> bool {
    let tol = step.map_or(1e-4, |s| s * 0.5).max(1e-6);
    match (now, base) {
        (ParamValue::F32(a), ParamValue::F32(b)) => (a - b).abs() > tol,
        (ParamValue::Color(a) | ParamValue::Vec3(a), ParamValue::Color(b) | ParamValue::Vec3(b)) => a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1.0 / 512.0),
        _ => {
            let _ = ty;
            now != base
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
/// Linear RGB to a `#rrggbb` sRGB colour (the page's colour input).
pub fn to_hex(c: [f32; 3]) -> String {
    let enc = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let s = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", enc(c[0]), enc(c[1]), enc(c[2]))
}

#[cfg(any(target_arch = "wasm32", test))]
/// `#rrggbb` (sRGB) to linear RGB.
pub fn from_hex(h: &str) -> Option<[f32; 3]> {
    let h = h.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let dec = |b: u8| {
        let s = f32::from(b) / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    Some([dec(ch(0)?), dec(ch(2)?), dec(ch(4)?)])
}

#[cfg(any(target_arch = "wasm32", test))]
/// One param as a game writes it (`name = value`), for the "Use in a game" fold.
pub fn toml_value(v: ParamValue) -> String {
    let f = |x: f32| {
        let t = format!("{x:.4}");
        let t = t.trim_end_matches('0');
        t.strip_suffix('.').map_or_else(|| t.to_string(), |s| format!("{s}.0"))
    };
    match v {
        ParamValue::F32(x) => f(x),
        ParamValue::I32(x) => x.to_string(),
        ParamValue::Bool(b) => b.to_string(),
        ParamValue::Color(c) | ParamValue::Vec3(c) => format!("[{}, {}, {}]", f(c[0]), f(c[1]), f(c[2])),
        ParamValue::Enum(i) => i.to_string(),
    }
}

#[cfg(test)]
mod tests {

    /// Batch A's contact sheets showed small, flat and long assets small in
    /// the frame: the library camera now fits the asset's box to the view.
    /// An asset the viewer can't reach (the 4.4 km mountains) is shown as a
    /// 300 m scale model; everything smaller as it is.
    #[test]
    fn a_huge_asset_shows_as_a_scale_model() {
        assert_eq!(view_scale([-40.0, 0.0, -40.0], [40.0, 80.0, 40.0]), 1.0);
        let k = view_scale([-2200.0, 0.0, -2200.0], [2200.0, 1200.0, 2200.0]);
        assert!((k * 4400.0 - VIEW_MAX).abs() < 1e-2, "{k}");
        let (lo, hi) = view_bounds([-2200.0, 0.0, -2200.0], [2200.0, 1200.0, 2200.0]);
        assert!(library_cam(lo, hi).dist <= 400.0 && hi[0] - lo[0] <= VIEW_MAX + 1e-2);
    }

    #[test]
    fn the_library_camera_fits_the_asset_box() {
        let tile = library_cam([-0.5, 0.0, -0.5], [0.5, 0.04, 0.5]);
        assert!(tile.dist < 2.2 && tile.dist > 0.8, "a 1 m tile comes close: {tile:?}");
        let tower = library_cam([-10.0, 0.0, -10.0], [10.0, 80.0, 10.0]);
        assert!(tower.dist > 100.0 && (tower.target[1] - 36.0).abs() < 1e-3, "an 80 m tower fits: {tower:?}");
        let road = library_cam([-6.0, 0.0, -2.0], [6.0, 0.1, 2.0]);
        assert!(road.dist < 18.0, "a 12 m road fills about 3/4 of the width: {road:?}");
        // The realism assets: a longer lens from farther away, lower down.
        let cube = library_cam_for("geometry-cube-cuboid", [-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]);
        let plain = library_cam([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]);
        assert!(cube.fov < plain.fov && cube.dist > plain.dist && cube.el < plain.el, "{cube:?} {plain:?}");
    }
    use super::*;

    /// With the procedural library built in, its 24 categories and all 357
    /// assets are in the list, each asset once, each with its switch.
    #[test]
    fn the_procedural_library_joins_with_every_category_and_asset() {
        let mut lib = ProcLib::load().unwrap();
        lib.add_library(&evani_procedural_lib::registry());
        assert_eq!(lib.categories.len(), 24);
        let assets: Vec<&str> = lib.ready().filter_map(|i| i.asset.as_deref()).collect();
        assert_eq!(assets.len(), 357, "every asset is listed");
        let mut once = assets.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(once.len(), 357, "each asset once");
        for it in lib.ready().filter(|i| i.asset.is_some()) {
            assert!(it.on_param.as_deref().is_some_and(|p| p.starts_with("procedural.") && p.ends_with(".on")), "{}: its switch", it.id);
            assert!(lib.categories.iter().any(|(c, _)| *c == it.category), "{}: its category", it.id);
        }
        let hills = lib.item("terrain-rolling-hills").unwrap();
        assert_eq!((hills.stage_as.as_str(), hills.icon.as_str()), ("library", "◆"), "a built generator");
        assert!(hills.headline.iter().any(|n| n == "procedural.terrain.rolling_hills.height"));
        // Batch C built the last stand-ins: none is left.
        assert_eq!(lib.item("vehicles-sedans").map(|i| i.icon.as_str()), Some("◆"), "a basic generator (batch C)");
        assert!(!lib.ready().any(is_standin), "no stand-in is left");
        assert_eq!(lib.item("rocks-pebbles").map(|i| i.icon.as_str()), Some("◆"), "a basic generator (batch A)");
        assert_eq!(lib.item("city-bollards").map(|i| i.icon.as_str()), Some("◆"), "a basic generator (batch B)");
        assert_eq!(lib.item("ocean").and_then(|i| i.asset.as_deref()), Some("water-oceans"), "the ocean is the library's oceans");
    }

    #[test]
    fn the_library_parses_and_has_the_phase_one_items() {
        let lib = ProcLib::load().unwrap();
        let ready: Vec<&str> = lib.ready().map(|i| i.id.as_str()).collect();
        // The phase one items, the grass (evani-grass as it is), then the
        // aliases (the storm sea, the pond and the river).
        assert_eq!(ready, ["ocean", "water", "glass", "sky", "grass-lawn", "grass-meadow", "grass-wheat", "storm-sea", "pond", "river"]);
        let storm = lib.item("storm-sea").unwrap();
        assert_eq!((storm.stage_as.as_str(), storm.presets[0].id.as_str(), storm.asset.as_deref()), ("ocean", "storm", Some("water-ocean-waves")), "the storm sea is the ocean on its storm preset");
        let presets = |id: &str| lib.item(id).unwrap().presets.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        assert_eq!(presets("ocean"), ["Calm sea", "Open sea", "Rough sea", "Storm sea", "Tropical shallows", "Beach breakers"]);
        assert_eq!(presets("water"), ["Calm lake", "Flowing river", "Muddy pond"]);
        assert_eq!(presets("glass"), ["Window", "Bottle green", "Thick crystal"]);
        assert_eq!(presets("sky"), ["Clear noon", "Scattered clouds", "Overcast", "Sunset"]);
        for it in lib.ready() {
            assert!((3..=5).contains(&it.headline.len()), "{}: {} headline sliders", it.id, it.headline.len());
            assert!(!it.module.is_empty(), "{}: no module line", it.id);
        }
        // The planned ones say so and carry nothing to apply.
        assert!(lib.items.iter().filter(|i| !i.ready).all(|i| i.presets.is_empty()));
        assert!(lib.item("fire").is_some_and(|i| !i.ready));
    }

    /// Every ocean in the library is the newest sea (ocean pass 2, Join 25):
    /// the Ocean item is the spectral sea (its module line, its quality
    /// switch, every preset staged as the open sea or a beach), and no item
    /// or preset still stages the older analytic sea (`Shape::Sea`). Chris,
    /// 2026-11-02: "Use the latest ocean sessions inside the Evani.Dev
    /// library."
    #[test]
    fn every_ocean_is_the_spectral_sea() {
        let lib = ProcLib::load().unwrap();
        let ocean = lib.item("ocean").unwrap();
        assert_eq!(ocean.module, "nature = [\"spectral-sea\"]");
        assert_eq!(ocean.quality.as_deref(), Some("material.nature.water.sea_quality"));
        for p in &ocean.presets {
            let s = p.stage.unwrap_or_default();
            assert!(s.shape.spectral(), "ocean/{}: staged as {:?}, not the spectral sea", p.id, s.shape);
        }
        for it in lib.ready() {
            for p in &it.presets {
                assert_ne!(p.stage.map(|s| s.shape), Some(Shape::Sea), "{}/{}: the older analytic sea", it.id, p.id);
            }
            // One ocean item: no second sea beside it.
            assert!(it.stage_as == "ocean" || !it.module.contains("spectral-sea"), "{}: a second sea item", it.id);
        }
        // Each level label is the crate's own (Ultra only, High and Ultra,
        // Medium and up), and every setting that doesn't show on every level
        // is labelled.
        for (n, l) in &ocean.levels {
            let want = match l.as_str() {
                "ultra" => "Ultra only",
                "high" => "High and Ultra",
                _ => "Medium and up",
            };
            assert_eq!(evani_material_nature::sea::shows_on(n), want, "{n}");
        }
        for (n, _) in evani_material_nature::sea::SHOWS_FROM {
            assert!(ocean.level_note(n).is_some(), "{n} is labelled");
        }
        assert_eq!(ocean.quality_buttons().iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Auto", "Ultra", "High", "Medium", "Low"]);
        let water = lib.item("water").unwrap();
        // The nature pass has one top step: Ultra and High both set 3 (and
        // light together), so every label stays short enough for the panel.
        assert_eq!(water.quality_buttons().iter().map(|(n, v)| (n.as_str(), *v)).collect::<Vec<_>>(), [("Auto", -1.0), ("Ultra", 3.0), ("High", 3.0), ("Medium", 2.0), ("Low", 1.0)]);
    }

    /// The newest water features in the presets: storm sea chops and
    /// breaks, tropical shallows glow over a shallow bed with caustics, the
    /// beach breaks; the river flows with foam, white streaks and a faint
    /// glow; the lake is still with only a faint green glow. Every param is
    /// listed, so presets reset what the last item left (the Lake's calm
    /// water once showed the Tropical preset's crest glow in teal patches:
    /// the Lake didn't list `sss_strength`; ocean phase 1 shots).
    #[test]
    fn presets_use_the_newest_water() {
        let lib = ProcLib::load().unwrap();
        let ocean = lib.item("ocean").unwrap();
        let num = |it: &Item, p: &str, n: &str| {
            it.preset(p).and_then(|p| p.params.iter().find(|(k, _)| k == &format!("material.nature.water.{n}")).and_then(|(_, v)| v.as_f64())).unwrap_or(0.0)
        };
        // (Storm-v2: the storm's crests break as crisp lace, measured at 3-4 %
        // of the sea in foam alone at whitecaps 0.42, plus streaks and
        // spindrift; whitecaps alone at 0.6 covered a third of the sea.)
        assert!(
            num(ocean, "storm", "choppiness") >= 0.6
                && num(ocean, "storm", "whitecap_amount") >= 0.4
                && num(ocean, "storm", "foam_streaks") >= 0.5
                && num(ocean, "storm", "spindrift") >= 1.0
                && num(ocean, "storm", "wind_speed") >= 20.0,
            "storm sea chops and breaks"
        );
        assert!(num(ocean, "calm", "wind_speed") < num(ocean, "open", "wind_speed").max(9.0), "the calm sea is calmer than the open sea");
        assert!(num(ocean, "tropical", "sss_strength") > 1.0 && num(ocean, "tropical", "caustics") > 1.0 && num(ocean, "tropical", "sea_depth") < 10.0, "tropical shallows glow");
        assert!(num(ocean, "beach", "beach_slope") > 0.0 && num(ocean, "beach", "breaker_height") > 0.5, "the beach breaks");
        let water = lib.item("water").unwrap();
        assert!(water.headline.contains(&"material.nature.water.flow_speed".to_string()));
        assert!(water.headline.contains(&"material.nature.water.flow_direction".to_string()));
        assert!(num(water, "river", "flow_speed") > 0.5 && num(water, "river", "foam") > 0.5 && num(water, "river", "whitecap_amount") > 0.0, "the river flows, with foam and white streaks");
        assert_eq!(num(water, "lake", "flow_speed"), 0.0, "the lake stays still");
        assert!(num(water, "lake", "sss_strength") < 0.5, "only a faint glow on the lake");
        let mut reg = ParamRegistry::new();
        evani_material_nature::NatureParams::register(&mut reg).unwrap();
        evani_material_nature::SeaParams::register(&mut reg).unwrap();
        // The lake and river list every param the nature water reads.
        let sea_own = evani_material_nature::sea::own_param_names();
        let listed = water.params();
        for d in reg.defs().iter().filter(|d| d.name.starts_with("material.nature.water.") && !sea_own.contains(&d.name.to_string())) {
            assert!(listed.contains(&d.name.to_string()), "water: {} is listed, so its presets reset it", d.name);
        }
        // The ocean lists every param the sea reads.
        let listed = ocean.params();
        for n in evani_material_nature::sea::param_names() {
            // The quality switch is a viewing choice, kept across presets.
            if n.ends_with(".sea_quality") || n == "material.nature.enabled" {
                continue;
            }
            assert!(listed.contains(&n), "ocean: {n} is listed, so its presets reset it");
        }
        // And so they do: the tropical sea, then the lake, gives the lake's
        // own values.
        apply_preset(&mut reg, ocean, ocean.preset("tropical"));
        apply_preset(&mut reg, ocean, ocean.preset("storm"));
        let val = |reg: &ParamRegistry, n: &str| reg.id(&format!("material.nature.water.{n}")).and_then(|i| reg.value(i));
        assert_ne!(val(&reg, "sss_strength"), Some(ParamValue::F32(0.0)));
        apply_preset(&mut reg, water, water.preset("lake"));
        for n in ["choppiness", "flow_speed", "whitecap_amount"] {
            assert_eq!(val(&reg, n), Some(ParamValue::F32(0.0)), "the lake after the storm sea: {n} is off");
        }
        assert_eq!(val(&reg, "sss_strength"), Some(ParamValue::F32(0.35)), "the lake's own faint glow");
    }

    #[test]
    fn colours_round_trip_through_hex() {
        for c in [[0.05, 0.32, 0.42], [1.0, 0.0, 0.5], [0.9, 0.97, 0.94]] {
            let back = from_hex(&to_hex(c)).unwrap();
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 0.01, "{c:?} -> {back:?}");
            }
        }
        assert_eq!(from_hex("#zz0000"), None);
        assert_eq!(toml_value(ParamValue::F32(0.12)), "0.12");
        assert_eq!(toml_value(ParamValue::F32(2.0)), "2.0");
    }
}
