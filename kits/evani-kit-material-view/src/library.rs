//! The viewer's data, read from the kit's JSON files (never hard-coded):
//!
//! * `data/library.json`: the scenes, the four material families with their
//!   materials (presets over the registered params) and the keyword rules
//!   that sort a family's params into the Base, Finish and Detail groups;
//! * `data/lights.json`: the four fixed light presets;
//! * `data/world_rooms.json`: Evani.Materials' world rooms, one per world
//!   kit, each with material slots by Meridian Vale zone.
//!
//! The tuning panel is generated from the registered parameter list
//! (an idea borrowed from Filament's material sandbox): [`panel_rows`] picks
//! the params a material shows by name prefix and [`Groups::group_of`] puts
//! each in its group, so a param a family adds later shows up by itself.

use std::collections::BTreeMap;

use serde::Deserialize;

use evani_core::params::{ParamDef, ParamId, ParamRegistry, ParamType, ParamValue};

pub const LIBRARY_JSON: &str = include_str!("../data/library.json");
pub const LIGHTS_JSON: &str = include_str!("../data/lights.json");
pub const ROOMS_JSON: &str = include_str!("../data/world_rooms.json");

/// A camera orbit as the data gives it (degrees and metres).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct OrbitData {
    pub az: f32,
    pub el: f32,
    pub dist: f32,
    pub target: [f32; 3],
    #[serde(default)]
    pub min_dist: Option<f32>,
    #[serde(default)]
    pub max_dist: Option<f32>,
    /// The subject's radius round the target (m): the app keeps a sphere
    /// this big inside the part of the view the page's bars and panels
    /// leave open, pulling the camera back when the home view would not fit
    /// it (critic round 2: on phones the HUD covered or cropped the
    /// subject). None: the camera is only shifted into the open part.
    #[serde(default)]
    pub fit: Option<f32>,
    /// The subject's half width across the view (m) when it is wider than
    /// tall (the pool's basin and table, the yard's bench and pillar):
    /// kept inside the open part's width while `fit` keeps its height
    /// (critic round 3: phones held upright cropped them). None: `fit`.
    #[serde(default)]
    pub fit_width: Option<f32>,
    /// The box (min, max corners, m) holding every prop of the scene
    /// (critic round 6: held upright, phones and tablets cut the pool's lamp
    /// and hedge and the yard's stone ball off the edges). When the view is
    /// taller than wide, the app pulls the camera back until all eight
    /// corners are inside its open part; wider views keep the framing above.
    /// The app's tests hold every drawn prop inside it.
    #[serde(default)]
    pub bounds: Option<[[f32; 3]; 2]>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SceneDef {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub family: String,
    /// The material the scene opens with.
    pub default: String,
    pub orbit: OrbitData,
    /// The scene's close-up (critic round 6: the head moment showed the
    /// whole bust with the face about 170 px tall): the camera its review
    /// moment takes. Opening the scene still frames the whole subject.
    #[serde(default)]
    pub close: Option<OrbitData>,
    /// Light params this scene sets over every light preset's own (the
    /// head's clear backdrop: critic round 3 found a lone cloud wisp
    /// behind the head).
    #[serde(default)]
    pub light: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MaterialDef {
    pub id: String,
    pub name: String,
    pub scene: String,
    /// Which model the scene shows for it (scenes with several).
    #[serde(default)]
    pub show: Option<String>,
    /// A camera that frames this material (else the scene's).
    #[serde(default)]
    pub orbit: Option<OrbitData>,
    /// Name prefixes of the params its panel shows (else the family's).
    #[serde(default)]
    pub panel: Vec<String>,
    /// Values set over the family's defaults when it opens (numbers, bools
    /// or `[r, g, b]`).
    #[serde(default)]
    pub params: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Family {
    pub id: String,
    /// The family name the world rooms' slots use.
    pub rooms: String,
    pub name: String,
    /// The app's Cargo feature that compiles it in (evani.toml
    /// `[modules] materials`).
    pub feature: String,
    pub about: String,
    pub prefixes: Vec<String>,
    pub default: String,
    /// Its params belong to the viewer's own state (the art styles), so
    /// opening it doesn't reset them.
    #[serde(default)]
    pub keep_params: bool,
    pub materials: Vec<MaterialDef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Groups {
    pub order: Vec<String>,
    pub names: BTreeMap<String, String>,
    /// Exact names that go to the Quality section instead.
    pub quality: Vec<String>,
    /// Exact names never shown (internal bookkeeping params).
    pub hidden: Vec<String>,
    /// Keywords (in the name's last part) that put a param in Detail, then
    /// Base; anything else is Finish.
    pub detail: Vec<String>,
    pub base: Vec<String>,
}

impl Groups {
    /// The group a param name belongs to: `quality`, `base`, `finish` or
    /// `detail`; `None` when hidden.
    pub fn group_of(&self, name: &str) -> Option<&'static str> {
        if self.hidden.iter().any(|h| h == name) {
            return None;
        }
        if self.quality.iter().any(|q| q == name) {
            return Some("quality");
        }
        let last = name.rsplit('.').next().unwrap_or(name);
        if self.detail.iter().any(|k| last.contains(k.as_str())) {
            Some("detail")
        } else if self.base.iter().any(|k| last.contains(k.as_str())) {
            Some("base")
        } else {
            Some("finish")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Library {
    pub library: String,
    pub version: u32,
    pub scenes: Vec<SceneDef>,
    pub families: Vec<Family>,
    pub groups: Groups,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Sun {
    pub el: f32,
    pub az: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LightPreset {
    pub id: String,
    pub name: String,
    pub sun: Sun,
    pub color: [f32; 3],
    pub intensity: f32,
    pub ambient: [f32; 3],
    pub exposure: f32,
    pub clear: [f32; 3],
    #[serde(default)]
    pub params: BTreeMap<String, serde_json::Value>,
}

/// A plain surface the scenes' stage needs (floor, props, a glowing bulb)
/// that the look may not have. The app adds it to the look's materials only
/// when the look lacks that name; scenes tint it per instance.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StageMaterial {
    pub name: String,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    /// Linear RGB, HDR allowed.
    #[serde(default)]
    pub emissive: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Lights {
    pub presets: Vec<LightPreset>,
    #[serde(default)]
    pub stage_materials: Vec<StageMaterial>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Slot {
    pub id: String,
    /// A family's `rooms` name, or `GAP` (no family covers it yet).
    pub family: String,
    #[serde(default)]
    pub note: Option<String>,
    pub look: String,
    /// The material filling the slot, once the library has one.
    #[serde(default)]
    pub material: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Room {
    pub room: String,
    pub zone: String,
    pub name: String,
    pub description: String,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rooms {
    pub library: String,
    pub version: u32,
    pub source: String,
    pub rooms: Vec<Room>,
}

/// Everything the viewer reads, parsed and cross-checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Data {
    pub library: Library,
    pub lights: Lights,
    pub rooms: Rooms,
}

impl Data {
    /// Parse the shipped files.
    pub fn shipped() -> Result<Self, String> {
        Self::parse(LIBRARY_JSON, LIGHTS_JSON, ROOMS_JSON)
    }

    /// Parse and check that every reference resolves.
    pub fn parse(library: &str, lights: &str, rooms: &str) -> Result<Self, String> {
        let library: Library = serde_json::from_str(library).map_err(|e| format!("library.json: {e}"))?;
        let lights: Lights = serde_json::from_str(lights).map_err(|e| format!("lights.json: {e}"))?;
        let rooms: Rooms = serde_json::from_str(rooms).map_err(|e| format!("world_rooms.json: {e}"))?;
        let d = Self { library, lights, rooms };
        d.validate()?;
        Ok(d)
    }

    fn validate(&self) -> Result<(), String> {
        let lib = &self.library;
        if lib.scenes.is_empty() || self.lights.presets.is_empty() {
            return Err("library.json needs scenes and lights.json presets".into());
        }
        for s in &lib.scenes {
            let f = self.family(&s.family).ok_or_else(|| format!("scene `{}`: no family `{}`", s.id, s.family))?;
            let m = self.material(&s.default).ok_or_else(|| format!("scene `{}`: no material `{}`", s.id, s.default))?;
            if m.1.scene != s.id || m.0.id != f.id {
                return Err(format!("scene `{}`: its default `{}` is not one of its materials", s.id, s.default));
            }
        }
        for f in &lib.families {
            if self.material(&f.default).is_none_or(|(ff, _)| ff.id != f.id) {
                return Err(format!("family `{}`: default `{}` is not its material", f.id, f.default));
            }
            for m in &f.materials {
                let s = self.scene(&m.scene).ok_or_else(|| format!("material `{}`: no scene `{}`", m.id, m.scene))?;
                if s.family != f.id {
                    return Err(format!("material `{}`: scene `{}` belongs to `{}`", m.id, s.id, s.family));
                }
            }
        }
        for r in &self.rooms.rooms {
            for s in &r.slots {
                if s.family != "GAP" && !lib.families.iter().any(|f| f.rooms == s.family) {
                    return Err(format!("room slot `{}`: unknown family `{}`", s.id, s.family));
                }
                if let Some(m) = &s.material
                    && self.material(m).is_none()
                {
                    return Err(format!("room slot `{}`: unknown material `{m}`", s.id));
                }
            }
        }
        Ok(())
    }

    pub fn scene(&self, id: &str) -> Option<&SceneDef> {
        self.library.scenes.iter().find(|s| s.id == id)
    }

    pub fn family(&self, id: &str) -> Option<&Family> {
        self.library.families.iter().find(|f| f.id == id)
    }

    /// The family a world-room slot names (`layered`, `nature`...).
    pub fn family_for_rooms(&self, rooms: &str) -> Option<&Family> {
        self.library.families.iter().find(|f| f.rooms == rooms)
    }

    pub fn material(&self, id: &str) -> Option<(&Family, &MaterialDef)> {
        self.library
            .families
            .iter()
            .find_map(|f| f.materials.iter().find(|m| m.id == id).map(|m| (f, m)))
    }

    /// What a world-room slot opens: its own material when filled, else its
    /// family's default ("try a material here"); `None` for a GAP slot.
    pub fn slot_target<'a>(&'a self, slot: &'a Slot) -> Option<&'a str> {
        if let Some(m) = &slot.material {
            return Some(m.as_str());
        }
        self.family_for_rooms(&slot.family).map(|f| f.default.as_str())
    }
}

/// Whether `name` sits under `prefix` (the whole name or a dotted child).
pub fn under(name: &str, prefix: &str) -> bool {
    name == prefix || name.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('.'))
}

/// One generated panel row.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelRow {
    /// `base`, `finish`, `detail` or `quality`.
    pub group: &'static str,
    pub id: ParamId,
    pub def: ParamDef,
}

/// The rows a material's panel shows, in registry order, from the params
/// this build registered. The panel prefixes come from the material, else
/// its family; when none of a material's own prefixes is registered the
/// family's are used. Quality params of the whole family are always there.
pub fn panel_rows(reg: &ParamRegistry, groups: &Groups, family: &Family, material: Option<&MaterialDef>) -> Vec<PanelRow> {
    let pick = |prefixes: &[String]| -> Vec<PanelRow> {
        reg.defs()
            .iter()
            .filter(|d| prefixes.iter().any(|p| under(&d.name, p)))
            .filter_map(|d| {
                let group = groups.group_of(&d.name)?;
                let id = reg.id(&d.name)?;
                Some(PanelRow { group, id, def: d.clone() })
            })
            .collect()
    };
    let fam = pick(&family.prefixes);
    let own = material.filter(|m| !m.panel.is_empty()).map(|m| pick(&m.panel));
    match own {
        Some(rows) if rows.iter().any(|r| r.group != "quality") => {
            let mut rows = rows;
            for q in fam.into_iter().filter(|r| r.group == "quality") {
                if !rows.iter().any(|r| r.id == q.id) {
                    rows.push(q);
                }
            }
            rows
        }
        _ => fam,
    }
}

/// A JSON value as the param's own type (numbers, bools, `[r, g, b]`).
pub fn value_for(def: &ParamDef, v: &serde_json::Value) -> Option<ParamValue> {
    let num = || v.as_f64().map(|x| x as f32).or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 }));
    let vec3 = || {
        let a = v.as_array()?;
        let c = |i: usize| a.get(i).and_then(serde_json::Value::as_f64).map(|x| x as f32);
        Some([c(0)?, c(1)?, c(2)?])
    };
    Some(match def.ty {
        ParamType::F32 => ParamValue::F32(num()?),
        ParamType::I32 => ParamValue::I32(num()?.round() as i32),
        ParamType::Bool => ParamValue::Bool(v.as_bool().or_else(|| v.as_f64().map(|x| x != 0.0))?),
        ParamType::Vec3 => ParamValue::Vec3(vec3()?),
        ParamType::Color => ParamValue::Color(vec3()?),
        ParamType::Enum => {
            if let Some(s) = v.as_str() {
                ParamValue::Enum(def.options.iter().position(|o| o == s)? as u32)
            } else {
                ParamValue::Enum(num()?.max(0.0) as u32)
            }
        }
    })
}

/// Set a registered param from a JSON value; a param this build lacks, or a
/// value of the wrong shape, is skipped (returns false).
pub fn set_json(reg: &mut ParamRegistry, name: &str, v: &serde_json::Value) -> bool {
    let Some(def) = reg.defs().iter().find(|d| d.name == name).cloned() else {
        return false;
    };
    value_for(&def, v).is_some_and(|val| reg.set_by_name(name, val).is_ok())
}
