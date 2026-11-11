//! The studio's state: the selection record, the auto-match rule, and what
//! is remembered per combination.
//!
//! **The record** ([`Selection`]): asset, style and material. The drawn
//! result is [`crate::resolve::resolve`] of the record and of what is
//! remembered *for that record* ([`Settings`], keyed by the record), so how
//! the user got there can't change it.
//!
//! **Outside the record** ([`View`]): the light preset and the quality tier.
//! They never enter a cache key and never rebuild the object.
//!
//! **The auto-match rule** ([`StudioState::apply`]):
//! - Pick a material while the asset wasn't chosen on purpose: if the
//!   material belongs on a class, the asset becomes that class's primary
//!   asset ("Showing Horse coat, bay on a horse").
//! - Pick a material while the asset *was* chosen on purpose: kept if the
//!   material is native or generic on it. A foreign material is **not**
//!   applied and the asset is **not** replaced: the notice offers "Show on a
//!   horse" ([`Pick::ShowOn`]).
//! - Pick an asset: the style stays. The material stays if it goes on the
//!   new asset; else the one last used on that class, else the native
//!   material closest in colour, else the asset's default, and the notice
//!   says which and why.
//! - Pick a style: nothing else changes.
//!
//! **Parts** (`assets.json` `parts`): the record also holds a material per
//! part the user gave one ([`Selection::parts`]). With a part selected
//! ([`StudioState::part`], [`Pick::Part`]) a material pick goes on that part
//! only; with "All parts" (none selected) it is the record's material, and
//! it also goes on every other part made to take it (a roof tile on the
//! ridge too, never on the brick gable). The record stays canonical, so
//! click order still can't matter: a part's material that equals what the
//! part would wear anyway is not stored, and a pick for the part the
//! record's material sits on changes the record's material.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Catalogs;
use crate::compat::{Compat, compat, part_compat, part_made_for, part_takes, place, preview_asset};
use crate::materials::{MaterialInfo, class_matches};
use crate::styles::control;

/// The record: WHAT (asset), HOW (style), OF WHAT (material).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Selection {
    pub asset: String,
    pub style: String,
    pub material: String,
    /// Part id -> the material the user gave that part (only parts that
    /// differ from what they'd wear anyway).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, String>,
}

impl Selection {
    /// A record with no per-part materials.
    pub fn new(asset: &str, style: &str, material: &str) -> Self {
        Self { asset: asset.into(), style: style.into(), material: material.into(), parts: BTreeMap::new() }
    }

    /// `asset|style|material` and, when parts have their own materials,
    /// `|part=material,...` (ids never contain `|`, `=` or `,`; the catalogue
    /// checks enforce it): the key of [`StudioState::memory`].
    pub fn key(&self) -> String {
        let mut k = format!("{}|{}|{}", self.asset, self.style, self.material);
        if !self.parts.is_empty() {
            let list: Vec<String> = self.parts.iter().map(|(p, m)| format!("{p}={m}")).collect();
            k.push('|');
            k.push_str(&list.join(","));
        }
        k
    }

    /// What part `part` wears in this record (its lead region's material;
    /// `None` for a part that keeps the model's own colour).
    pub fn part_material(&self, c: &Catalogs, part: &str) -> Option<String> {
        let r = crate::resolve::resolve(c, self, &BTreeMap::new(), None).ok()?;
        let asset = c.assets.asset(&self.asset)?;
        let lead = asset.part(part).and_then(|p| asset.part_lead(p))?;
        r.bindings.get(lead).and_then(|b| b.material.clone())
    }

    /// This record with `material` on part `part` only, canonical (module
    /// docs). The part's level when it doesn't take the material.
    pub fn with_part(&self, c: &Catalogs, part: &str, material: &str) -> Result<Self, Compat> {
        let (Some(asset), Some(m)) = (c.assets.asset(&self.asset), c.materials.get(material)) else { return Err(Compat::NoRegion) };
        let Some(p) = asset.part(part) else { return Err(Compat::NoRegion) };
        let level = part_compat(c, asset, p, m);
        if !level.applies() {
            return Err(level);
        }
        let mut out = self.clone();
        let in_part = |i: Option<usize>| i.and_then(|i| asset.part_of_region(i)).is_some_and(|k| asset.parts[k].id == part);
        let main = c.materials.get(&self.material).and_then(|cur| place(c, asset, cur));
        out.parts.remove(part);
        if in_part(main) && in_part(place(c, asset, m)) {
            // The part the record's material sits on: the record's material.
            out.material = material.to_string();
        } else if out.part_material(c, part).as_deref() != Some(material) {
            out.parts.insert(part.to_string(), material.to_string());
        }
        out.canonical(c);
        Ok(out)
    }

    /// Drop part materials the asset doesn't have, that don't go on their
    /// part, or that equal what the part would wear anyway.
    pub fn canonical(&mut self, c: &Catalogs) {
        let Some(asset) = c.assets.asset(&self.asset) else {
            self.parts.clear();
            return;
        };
        self.parts.retain(|p, m| match (asset.part(p), c.materials.get(m)) {
            (Some(part), Some(mat)) => mat.ready() && part_compat(c, asset, part, mat).applies(),
            _ => false,
        });
        let ids: Vec<String> = self.parts.keys().cloned().collect();
        for p in ids {
            let Some(m) = self.parts.remove(&p) else { continue };
            if self.part_material(c, &p).as_deref() != Some(m.as_str()) {
                self.parts.insert(p, m);
            }
        }
    }
}

/// Was each part chosen on purpose (true) or picked by the auto-match?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Explicit {
    pub asset: bool,
    pub material: bool,
}

/// What the user does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Asset(String),
    Style(String),
    /// A material for the selected part, or for "All parts" when none is.
    Material(String),
    /// Select a part (`None`: "All parts"). Changes no record.
    Part(Option<String>),
    /// A material for one part, whichever part is selected.
    PartMaterial { part: String, material: String },
    /// The notice's one click: show the material that wasn't applied on
    /// this asset (sets the asset on purpose).
    ShowOn(String),
}

/// Why a new asset got another material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Because {
    /// The material last used on this class.
    Remembered,
    /// The native material closest in colour to the previous one.
    Closest,
    /// The asset's own default.
    Default,
}

/// What the page tells the user after a pick (one line, never a dialog).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Notice {
    #[default]
    None,
    /// "Showing Horse coat, bay on a horse."
    ShownOn { material: String, asset: String },
    /// "Horse coat, bay is made for horses · [Show on a horse]". The asset
    /// was chosen on purpose, so it stays and the material isn't applied.
    NotApplied { material: String, made_for: String, offer: String },
    /// "Glass can't go on a dog."
    NoPlace { material: String, asset: String },
    /// "Horse coat, bay can't go on the roof's gable ends."
    NotOnPart { material: String, asset: String, part: String },
    /// "Cow coat, black (closest to Horse coat, bay)."
    Substituted { from: String, to: String, because: Because },
    /// The material is planned, not built yet: nothing changed.
    NotBuilt { material: String },
}

/// One tuned value of a combination (a finish slider, a colour, a switch).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Tweak {
    Num(f32),
    Colour([f32; 3]),
    Flag(bool),
}

/// The orbit camera of a combination.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 3],
}

/// What is remembered for one combination (test 8): the material edits by
/// parameter name and the camera.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Settings {
    pub edits: BTreeMap<String, Tweak>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<Camera>,
}

/// Outside the record: never rebuilds the object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    /// A light preset id (`studio`, `daylight`, `sunset`, `night`).
    pub light: String,
    /// A quality tier id (`auto`, `lite`, `low`, `medium`, `high`, `ultra`).
    pub quality: String,
}

impl Default for View {
    fn default() -> Self {
        Self { light: "studio".into(), quality: "auto".into() }
    }
}

/// A saved combination ("Save to library", the presets row).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub name: String,
    pub selection: Selection,
    pub settings: Settings,
}

/// The whole studio state. Serialises to a small JSON for the page's
/// storage (`to_json` / `from_json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioState {
    pub selection: Selection,
    pub explicit: Explicit,
    /// The selected part (`None`: "All parts"): where a material pick goes.
    /// Outside the record: never in a key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// A material picked but not applied (foreign or no region): the
    /// materials bar keeps it marked "not applied" and the notice offers it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
    pub view: View,
    /// Style id -> control id -> value (per style: a voxel size holds for
    /// every asset in that style).
    #[serde(default)]
    pub controls: BTreeMap<String, BTreeMap<String, f32>>,
    /// [`Selection::key`] -> what was tuned there.
    #[serde(default)]
    pub memory: BTreeMap<String, Settings>,
    /// Class id -> the material last applied on it.
    #[serde(default)]
    pub last_material_for: BTreeMap<String, String>,
    /// Comparison mode: 2 to 4 records side by side (empty = one view).
    #[serde(default)]
    pub compare: Vec<Selection>,
    #[serde(default)]
    pub saved: Vec<Saved>,
    #[serde(skip)]
    pub notice: Notice,
    /// Bumped on every change the page shows.
    #[serde(skip)]
    pub revision: u64,
}

/// At most this many records side by side (model slots used by compare).
pub const MAX_COMPARE: usize = 4;

fn rgb_distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (i32::from(*x) - i32::from(y)).unsigned_abs().pow(2)).sum()
}

impl StudioState {
    /// The opening state, all from data: the catalogue's default asset, its
    /// default material, the first style. Nothing is "explicit" yet.
    pub fn new(c: &Catalogs) -> Result<Self, String> {
        let asset = c.assets.asset(&c.assets.default_asset).ok_or("no default asset")?;
        let style = c.styles.styles.first().ok_or("no styles")?;
        Ok(Self {
            selection: Selection::new(&asset.id, &style.id, &asset.default_material),
            explicit: Explicit::default(),
            part: None,
            pending: None,
            view: View::default(),
            controls: BTreeMap::new(),
            memory: BTreeMap::new(),
            last_material_for: BTreeMap::new(),
            compare: Vec::new(),
            saved: Vec::new(),
            notice: Notice::None,
            revision: 1,
        })
    }

    /// Apply one pick by the auto-match rule (module docs). An unknown id is
    /// an error and changes nothing.
    pub fn apply(&mut self, c: &Catalogs, pick: Pick) -> Result<(), String> {
        match pick {
            Pick::Style(s) => {
                c.styles.style(&s).ok_or_else(|| format!("no style `{s}`"))?;
                self.selection.style = s;
                self.notice = Notice::None;
            }
            Pick::Material(m) => match self.part.clone() {
                Some(p) => self.pick_part_material(c, &p, &m)?,
                None => self.pick_material(c, &m)?,
            },
            Pick::PartMaterial { part, material } => self.pick_part_material(c, &part, &material)?,
            Pick::Part(p) => {
                if let Some(id) = &p {
                    let a = c.assets.asset(&self.selection.asset).ok_or("the selected asset is gone")?;
                    a.part(id).ok_or_else(|| format!("`{}` has no part `{id}`", a.id))?;
                }
                self.part = p;
                self.notice = Notice::None;
            }
            Pick::Asset(a) | Pick::ShowOn(a) => self.pick_asset(c, &a)?,
        }
        self.revision += 1;
        Ok(())
    }

    fn material<'a>(c: &'a Catalogs, id: &str) -> Result<&'a MaterialInfo, String> {
        c.materials.get(id).ok_or_else(|| format!("no material `{id}`"))
    }

    fn remember(&mut self, c: &Catalogs) {
        if let Some(a) = c.assets.asset(&self.selection.asset) {
            self.last_material_for.insert(a.class.clone(), self.selection.material.clone());
        }
    }

    fn pick_material(&mut self, c: &Catalogs, id: &str) -> Result<(), String> {
        let m = Self::material(c, id)?;
        if !m.ready() {
            self.notice = Notice::NotBuilt { material: id.to_string() };
            return Ok(());
        }
        let current = c.assets.asset(&self.selection.asset).ok_or("the selected asset is gone")?;
        // Auto: the material chooses where it previews, unless it already
        // belongs on the current asset.
        if !self.explicit.asset
            && !m.belongs_on.iter().any(|p| class_matches(p, &current.class))
            && let Some(target) = preview_asset(c, m)
            && compat(c, target, m).applies()
        {
            self.move_to(c, &target.id);
            self.selection.material = id.to_string();
            self.spread(c);
            self.explicit.material = true;
            self.pending = None;
            self.notice = Notice::ShownOn { material: id.to_string(), asset: target.id.clone() };
            self.remember(c);
            return Ok(());
        }
        match compat(c, current, m) {
            Compat::Native | Compat::Generic => {
                self.selection.material = id.to_string();
                self.spread(c);
                self.explicit.material = true;
                self.pending = None;
                self.notice = Notice::None;
                self.remember(c);
            }
            Compat::Foreign { made_for } => {
                let offer = c.assets.class(&made_for).map(|k| k.primary_asset.clone()).unwrap_or_default();
                self.pending = Some(id.to_string());
                self.notice = Notice::NotApplied { material: id.to_string(), made_for, offer };
            }
            Compat::NoRegion => {
                self.pending = Some(id.to_string());
                self.notice = Notice::NoPlace { material: id.to_string(), asset: current.id.clone() };
            }
        }
        Ok(())
    }

    /// "All parts": the record's material just changed; every other part
    /// made to take it (by class, recipe or family; for a material made for
    /// nothing in particular, every part that takes it) wears it too, a part
    /// that follows another follows again, the others keep theirs.
    fn spread(&mut self, c: &Catalogs) {
        let (Some(asset), Some(m)) = (c.assets.asset(&self.selection.asset), c.materials.get(&self.selection.material)) else { return };
        let at = place(c, asset, m).and_then(|i| asset.part_of_region(i));
        for (k, p) in asset.parts.iter().enumerate() {
            let lead = asset.part_lead(p).map(|i| &asset.regions[i]);
            let anything = m.belongs_on.is_empty() && lead.is_some_and(|r| !r.keep) && part_takes(c, asset, p, m);
            if Some(k) == at || lead.is_some_and(|r| r.follows.is_some()) {
                self.selection.parts.remove(&p.id);
            } else if part_made_for(c, asset, p, m) || anything {
                self.selection.parts.insert(p.id.clone(), m.id.clone());
            }
        }
        self.selection.canonical(c);
    }

    /// The record moves to asset `to`: per-part materials carry over by part
    /// kind where the new asset has such a part that takes them (a horse's
    /// tail colour onto the cow's tail); the selected part stays if the new
    /// asset has one of that id.
    fn move_to(&mut self, c: &Catalogs, to: &str) {
        let (Some(from), Some(dest)) = (c.assets.asset(&self.selection.asset), c.assets.asset(to)) else {
            self.selection.asset = to.to_string();
            self.selection.parts.clear();
            return;
        };
        if from.id != dest.id {
            let mut carried = BTreeMap::new();
            for (pid, mid) in &self.selection.parts {
                let Some(kind) = from.part(pid).map(|p| p.kind.as_str()) else { continue };
                if let Some(np) = dest.parts.iter().find(|p| p.kind == kind) {
                    carried.insert(np.id.clone(), mid.clone());
                }
            }
            self.selection.parts = carried;
        }
        self.selection.asset = dest.id.clone();
        if self.part.as_deref().is_some_and(|p| dest.part(p).is_none()) {
            self.part = None;
        }
        self.selection.canonical(c);
    }

    /// A material for one part (module docs).
    fn pick_part_material(&mut self, c: &Catalogs, part: &str, id: &str) -> Result<(), String> {
        let m = Self::material(c, id)?;
        let asset = c.assets.asset(&self.selection.asset).ok_or("the selected asset is gone")?;
        asset.part(part).ok_or_else(|| format!("`{}` has no part `{part}`", asset.id))?;
        if !m.ready() {
            self.notice = Notice::NotBuilt { material: id.to_string() };
            return Ok(());
        }
        match self.selection.with_part(c, part, id) {
            Ok(s) => {
                self.selection = s;
                self.explicit.material = true;
                self.pending = None;
                self.notice = Notice::None;
                self.remember(c);
            }
            Err(Compat::Foreign { made_for }) => {
                let offer = c.assets.class(&made_for).map(|k| k.primary_asset.clone()).unwrap_or_default();
                self.pending = Some(id.to_string());
                self.notice = Notice::NotApplied { material: id.to_string(), made_for, offer };
            }
            Err(_) => {
                self.pending = Some(id.to_string());
                self.notice = Notice::NotOnPart { material: id.to_string(), asset: asset.id.clone(), part: part.to_string() };
            }
        }
        Ok(())
    }

    fn pick_asset(&mut self, c: &Catalogs, id: &str) -> Result<(), String> {
        let a = c.assets.asset(id).ok_or_else(|| format!("no asset `{id}`"))?;
        self.remember(c);
        self.move_to(c, &a.id);
        self.explicit.asset = true;
        self.notice = Notice::None;
        // A material waiting for an asset it goes on.
        if let Some(p) = self.pending.take() {
            if let Ok(m) = Self::material(c, &p)
                && compat(c, a, m).applies()
            {
                self.selection.material = p;
                self.selection.canonical(c);
                self.explicit.material = true;
                self.remember(c);
                return Ok(());
            }
            self.pending = Some(p);
        }
        let old = Self::material(c, &self.selection.material)?.clone();
        if compat(c, a, &old).applies() {
            self.remember(c);
            return Ok(());
        }
        let usable = |id: &str| c.materials.get(id).is_some_and(|m| m.ready() && compat(c, a, m).applies());
        let remembered = self.last_material_for.get(&a.class).filter(|m| usable(m)).cloned();
        let closest = || {
            let from = old.colour?;
            c.materials
                .rows
                .iter()
                .filter(|m| m.ready() && m.colour.is_some() && compat(c, a, m) == Compat::Native)
                .min_by_key(|m| (m.colour.map_or(u32::MAX, |x| rgb_distance(x, from)), m.id.clone()))
                .map(|m| m.id.clone())
        };
        let (to, because) = match remembered {
            Some(m) => (m, Because::Remembered),
            None => match closest() {
                Some(m) => (m, Because::Closest),
                None => (a.default_material.clone(), Because::Default),
            },
        };
        self.notice = Notice::Substituted { from: old.id.clone(), to: to.clone(), because };
        self.selection.material = to;
        self.selection.canonical(c);
        self.remember(c);
        Ok(())
    }

    /// What is remembered for the current record, if anything.
    pub fn settings(&self) -> Option<&Settings> {
        self.memory.get(&self.selection.key())
    }

    /// The current record's memory, made on first use.
    pub fn settings_mut(&mut self) -> &mut Settings {
        self.revision += 1;
        self.memory.entry(self.selection.key()).or_default()
    }

    /// The current style's control values.
    pub fn style_controls(&self) -> BTreeMap<String, f32> {
        self.controls.get(&self.selection.style).cloned().unwrap_or_default()
    }

    /// Set one of the current style's controls (clamped to its range).
    pub fn set_control(&mut self, c: &Catalogs, id: &str, value: f32) -> Result<(), String> {
        let style = c.styles.style(&self.selection.style).ok_or("the selected style is gone")?;
        let def = control(id).filter(|_| style.controls.iter().any(|x| x == id)).ok_or_else(|| format!("style `{}` has no control `{id}`", style.id))?;
        if !value.is_finite() {
            return Err(format!("`{id}` must be a number"));
        }
        self.controls.entry(style.id.clone()).or_default().insert(id.to_string(), value.clamp(def.min, def.max));
        self.revision += 1;
        Ok(())
    }

    /// The light preset: changes nothing else.
    pub fn set_light(&mut self, light: &str) {
        self.view.light = light.to_string();
        self.revision += 1;
    }

    /// The quality tier: changes nothing else.
    pub fn set_quality(&mut self, quality: &str) {
        self.view.quality = quality.to_string();
        self.revision += 1;
    }

    /// Side by side: 2 to [`MAX_COMPARE`] records, or none for one view.
    pub fn set_compare(&mut self, c: &Catalogs, list: Vec<Selection>) -> Result<(), String> {
        if list.len() == 1 || list.len() > MAX_COMPARE {
            return Err(format!("compare shows 2 to {MAX_COMPARE} records (or none)"));
        }
        for s in &list {
            Self::check(c, s)?;
        }
        self.compare = list;
        self.revision += 1;
        Ok(())
    }

    /// Save the current record and its settings under `name`.
    pub fn save(&mut self, name: &str) {
        let settings = self.settings().cloned().unwrap_or_default();
        self.saved.retain(|s| s.name != name);
        self.saved.push(Saved { name: name.to_string(), selection: self.selection.clone(), settings });
        self.revision += 1;
    }

    /// Every id of `s` is in the catalogues, the material goes on the asset
    /// and each part's material on its part.
    pub fn check(c: &Catalogs, s: &Selection) -> Result<(), String> {
        let a = c.assets.asset(&s.asset).ok_or_else(|| format!("no asset `{}`", s.asset))?;
        c.styles.style(&s.style).ok_or_else(|| format!("no style `{}`", s.style))?;
        let m = Self::material(c, &s.material)?;
        if !compat(c, a, m).applies() {
            return Err(format!("`{}` doesn't go on `{}`", s.material, s.asset));
        }
        for (pid, mid) in &s.parts {
            let p = a.part(pid).ok_or_else(|| format!("`{}` has no part `{pid}`", s.asset))?;
            if !part_compat(c, a, p, Self::material(c, mid)?).applies() {
                return Err(format!("`{mid}` doesn't go on the {} of `{}`", p.label, s.asset));
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| e.to_string())
    }

    /// Restore a saved state; anything the catalogues no longer hold is
    /// dropped (an unknown current record falls back to the opening state).
    pub fn from_json(json: &str, c: &Catalogs) -> Result<Self, String> {
        let mut s: StudioState = serde_json::from_str(json).map_err(|e| format!("studio state: {e}"))?;
        // Part materials the catalogues no longer allow are dropped, not the
        // whole record.
        for sel in std::iter::once(&mut s.selection).chain(s.compare.iter_mut()).chain(s.saved.iter_mut().map(|x| &mut x.selection)) {
            if c.assets.asset(&sel.asset).is_some() && c.materials.get(&sel.material).is_some() {
                sel.canonical(c);
            }
        }
        if Self::check(c, &s.selection).is_err() {
            let fresh = Self::new(c)?;
            s.selection = fresh.selection;
            s.explicit = Explicit::default();
        }
        s.compare.retain(|x| Self::check(c, x).is_ok());
        if s.compare.len() < 2 {
            s.compare.clear();
        }
        s.saved.retain(|x| Self::check(c, &x.selection).is_ok());
        s.pending = s.pending.filter(|p| c.materials.get(p).is_some());
        s.last_material_for.retain(|_, m| c.materials.get(m).is_some());
        s.controls.retain(|st, _| c.styles.style(st).is_some());
        if s.part.as_deref().is_some_and(|p| c.assets.asset(&s.selection.asset).is_none_or(|a| a.part(p).is_none())) {
            s.part = None;
        }
        s.notice = Notice::None;
        s.revision = 1;
        Ok(s)
    }
}
