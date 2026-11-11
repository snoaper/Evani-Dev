//! The art styles split into three stages (HOW an asset is drawn):
//!
//! 1. **Geometry**: changes the shape itself, from the asset's one source
//!    model ([`Geometry`]): voxels, low-poly, papercraft, clay lumps, yarn
//!    ridges, rubber-hose proportions, or none.
//! 2. **Surface**: the colour treatment on that shape ([`Surface`]): light
//!    bands, highlight, ink outline, posterize, dither, mottle... The fields
//!    are the style's `render` block (the viewer's existing treatment).
//! 3. **Pass**: a screen pass over the whole view ([`Pass`]): pixel grid,
//!    paper, tilt-shift, silhouette, or none.
//!
//! The geometry and pass stages come from each style's `studio` block in the
//! viewer's `styles.json`; the surface from its `render` block. Every style
//! is compatible with every asset (the owner's rule): a style can only be
//! "generating", never unavailable.
//!
//! Style controls (voxel size, toon bands...) are listed per style by id
//! ([`CONTROLS`]); each is a registered parameter
//! ([`control_param`]) and enters the cache key of its stage only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hash::KeyWriter;

/// The geometry stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Geometry {
    /// The source shape as it is.
    None,
    /// Rebuilt from cubes. `cell`: the cube size in source units at radius 1
    /// (the asset's bounds scaled to radius 1, as the viewer's shapes);
    /// `keep_thin`: thin regions keep at least one cube per segment.
    Voxel { cell: f32, keep_thin: bool },
    /// Simplified to about `target_tris` triangles with flat facets
    /// (`cluster`: the legacy grid merge size, radius-1 units).
    Lowpoly { cluster: f32, target_tris: u32, facets: bool },
    /// Low-poly cut into flat folded panels.
    Papercraft { cluster: f32, target_tris: u32 },
    /// Hand-pressed lumps in the silhouette (clay).
    Lumpy { amount: f32 },
    /// Knitted ridges around the shape (yarn).
    Ridges { amount: f32 },
    /// Cartoon proportions from the skeleton (rubber-hose, storybook):
    /// head and limb scale.
    Proportion { head: f32, limbs: f32 },
}

/// The geometry stage's kind, without its numbers (the generator table key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GeometryKind {
    None,
    Voxel,
    Lowpoly,
    Papercraft,
    Lumpy,
    Ridges,
    Proportion,
}

impl GeometryKind {
    pub const ALL: [GeometryKind; 7] = [Self::None, Self::Voxel, Self::Lowpoly, Self::Papercraft, Self::Lumpy, Self::Ridges, Self::Proportion];

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Voxel => "voxel",
            Self::Lowpoly => "lowpoly",
            Self::Papercraft => "papercraft",
            Self::Lumpy => "lumpy",
            Self::Ridges => "ridges",
            Self::Proportion => "proportion",
        }
    }
}

impl Geometry {
    pub fn kind(&self) -> GeometryKind {
        match self {
            Self::None => GeometryKind::None,
            Self::Voxel { .. } => GeometryKind::Voxel,
            Self::Lowpoly { .. } => GeometryKind::Lowpoly,
            Self::Papercraft { .. } => GeometryKind::Papercraft,
            Self::Lumpy { .. } => GeometryKind::Lumpy,
            Self::Ridges { .. } => GeometryKind::Ridges,
            Self::Proportion { .. } => GeometryKind::Proportion,
        }
    }

    /// Every number, in a fixed order (the cache key).
    pub fn write_key(&self, w: &mut KeyWriter) {
        w.str(self.kind().name());
        match *self {
            Self::None => {}
            Self::Voxel { cell, keep_thin } => {
                w.f32(cell).bool(keep_thin);
            }
            Self::Lowpoly { cluster, target_tris, facets } => {
                w.f32(cluster).u64(u64::from(target_tris)).bool(facets);
            }
            Self::Papercraft { cluster, target_tris } => {
                w.f32(cluster).u64(u64::from(target_tris));
            }
            Self::Lumpy { amount } | Self::Ridges { amount } => {
                w.f32(amount);
            }
            Self::Proportion { head, limbs } => {
                w.f32(head).f32(limbs);
            }
        }
    }

    fn validate(&self) -> Result<(), String> {
        let within = |x: f32, lo: f32, hi: f32| x.is_finite() && (lo..=hi).contains(&x);
        let ok = match *self {
            Self::None => true,
            Self::Voxel { cell, .. } => within(cell, 0.02, 0.5),
            Self::Lowpoly { cluster, target_tris, .. } | Self::Papercraft { cluster, target_tris } => within(cluster, 0.0, 0.6) && (100..=4000).contains(&target_tris),
            Self::Lumpy { amount } => within(amount, 0.0, 0.3),
            Self::Ridges { amount } => within(amount, 0.0, 0.2),
            Self::Proportion { head, limbs } => within(head, 0.5, 2.0) && within(limbs, 0.5, 2.0),
        };
        if ok { Ok(()) } else { Err(format!("geometry `{}`: a number is out of range", self.kind().name())) }
    }
}

/// The screen pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Pass {
    None,
    /// Low-resolution target, `px` screen pixels per art pixel, `levels`
    /// colour levels per channel (0 = no palette cut), an id outline.
    Pixel { px: f32, levels: u32, outline: bool },
    /// Paper: grain, edge darkening and pigment bleed (0 to 1 each).
    Paper { grain: f32, edge: f32, bleed: f32 },
    /// Miniature: the band in focus (0 to 1 of the view's height) and blur.
    Tiltshift { focus: f32, blur: f32 },
    /// Shapes as dark cut-outs in atmospheric fog.
    Silhouette { fog: f32 },
}

impl Pass {
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pixel { .. } => "pixel",
            Self::Paper { .. } => "paper",
            Self::Tiltshift { .. } => "tiltshift",
            Self::Silhouette { .. } => "silhouette",
        }
    }

    pub fn write_key(&self, w: &mut KeyWriter) {
        w.str(self.name());
        match *self {
            Self::None => {}
            Self::Pixel { px, levels, outline } => {
                w.f32(px).u64(u64::from(levels)).bool(outline);
            }
            Self::Paper { grain, edge, bleed } => {
                w.f32(grain).f32(edge).f32(bleed);
            }
            Self::Tiltshift { focus, blur } => {
                w.f32(focus).f32(blur);
            }
            Self::Silhouette { fog } => {
                w.f32(fog);
            }
        }
    }

    fn validate(&self) -> Result<(), String> {
        let unit = |x: f32| x.is_finite() && (0.0..=1.0).contains(&x);
        let ok = match *self {
            Self::None => true,
            Self::Pixel { px, levels, .. } => px.is_finite() && (1.0..=12.0).contains(&px) && (levels == 0 || (2..=32).contains(&levels)),
            Self::Paper { grain, edge, bleed } => unit(grain) && unit(edge) && unit(bleed),
            Self::Tiltshift { focus, blur } => unit(focus) && unit(blur),
            Self::Silhouette { fog } => unit(fog),
        };
        if ok { Ok(()) } else { Err(format!("pass `{}`: a number is out of range", self.name())) }
    }
}

/// The surface stage: the style's `render` block without its shape fields
/// (the viewer's treat.rs draws it; the kit's `styles::Render` validates it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Surface {
    pub bands: u32,
    pub shadow: f32,
    pub shadow_hue: [f32; 3],
    pub highlight: f32,
    pub outline: f32,
    pub outline_color: [f32; 3],
    pub outline_glow: bool,
    pub posterize: u32,
    pub dither: bool,
    pub paper: [f32; 3],
    pub mottle: f32,
    pub desaturate: f32,
    pub value: f32,
    pub side_light: f32,
    pub detail: f32,
    pub detail_step: f32,
}

impl Default for Surface {
    /// The kit's `Render::default()` values.
    fn default() -> Self {
        Self {
            bands: 0,
            shadow: 0.45,
            shadow_hue: [1.0; 3],
            highlight: 0.0,
            outline: 0.0,
            outline_color: [0.03, 0.025, 0.03],
            outline_glow: false,
            posterize: 0,
            dither: false,
            paper: [0.92, 0.88, 0.78],
            mottle: 0.0,
            desaturate: 0.0,
            value: 1.0,
            side_light: 0.0,
            detail: 1.0,
            detail_step: 0.5,
        }
    }
}

impl Surface {
    pub fn write_key(&self, w: &mut KeyWriter) {
        w.u64(u64::from(self.bands)).f32(self.shadow);
        for c in self.shadow_hue.iter().chain(&self.outline_color).chain(&self.paper) {
            w.f32(*c);
        }
        w.f32(self.highlight).f32(self.outline).bool(self.outline_glow).u64(u64::from(self.posterize)).bool(self.dither);
        w.f32(self.mottle).f32(self.desaturate).f32(self.value).f32(self.side_light).f32(self.detail).f32(self.detail_step);
    }
}

/// Which stage a control acts on (and so which cache key it enters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Geometry,
    Surface,
    Pass,
}

/// A style control: a live, registered parameter the inspector shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlDef {
    /// `<stage>.<name>`, as styles.json lists it.
    pub id: &'static str,
    pub stage: Stage,
    pub label: &'static str,
    pub what: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

/// Every style control there is. A style lists the ones it shows; the
/// default value comes from the style ([`StyleDef::control_default`]).
pub const CONTROLS: [ControlDef; 9] = [
    ControlDef { id: "geometry.cell", stage: Stage::Geometry, label: "Cube size", what: "Voxel cube size, times the style's own (smaller is finer).", min: 0.5, max: 2.0, step: 0.05 },
    ControlDef { id: "geometry.detail", stage: Stage::Geometry, label: "Detail", what: "Triangle count, times the style's own.", min: 0.5, max: 2.0, step: 0.05 },
    ControlDef { id: "geometry.amount", stage: Stage::Geometry, label: "Strength", what: "How strong the shape change is (lumps, ridges), times the style's own.", min: 0.0, max: 2.0, step: 0.05 },
    ControlDef { id: "geometry.head", stage: Stage::Geometry, label: "Head size", what: "Head scale of the cartoon proportions.", min: 0.5, max: 2.0, step: 0.05 },
    ControlDef { id: "surface.bands", stage: Stage::Surface, label: "Shading bands", what: "Hard light bands (1 = one flat colour).", min: 1.0, max: 6.0, step: 1.0 },
    ControlDef { id: "surface.outline", stage: Stage::Surface, label: "Outline", what: "Ink outline width, times the style's own.", min: 0.0, max: 2.0, step: 0.05 },
    ControlDef { id: "pass.px", stage: Stage::Pass, label: "Pixel size", what: "Screen pixels per art pixel.", min: 1.0, max: 12.0, step: 1.0 },
    ControlDef { id: "pass.blur", stage: Stage::Pass, label: "Blur", what: "Miniature blur outside the band in focus.", min: 0.0, max: 1.0, step: 0.05 },
    ControlDef { id: "pass.grain", stage: Stage::Pass, label: "Paper grain", what: "Paper grain strength.", min: 0.0, max: 1.0, step: 0.05 },
];

pub fn control(id: &str) -> Option<&'static ControlDef> {
    CONTROLS.iter().find(|c| c.id == id)
}

/// The registered parameter of `style`'s control `id`:
/// `kit.material_view.studio.<style id, lowercase>.<id>`.
pub fn control_param(style: &str, id: &str) -> String {
    format!("kit.material_view.studio.{}.{id}", style.to_ascii_lowercase())
}

/// One art style, split.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleDef {
    pub id: String,
    pub name: String,
    pub short: String,
    pub geometry: Geometry,
    pub surface: Surface,
    pub pass: Pass,
    /// Control ids ([`CONTROLS`]) the inspector shows for this style.
    pub controls: Vec<String>,
}

impl StyleDef {
    /// A control's value before the user moves it: 1 for the "times the
    /// style's own" controls, else the style's own number.
    pub fn control_default(&self, id: &str) -> Option<f32> {
        let g = &self.geometry;
        match id {
            "geometry.cell" => matches!(g, Geometry::Voxel { .. }).then_some(1.0),
            "geometry.detail" => matches!(g, Geometry::Lowpoly { .. } | Geometry::Papercraft { .. }).then_some(1.0),
            "geometry.amount" => matches!(g, Geometry::Lumpy { .. } | Geometry::Ridges { .. }).then_some(1.0),
            "surface.outline" => (self.surface.outline > 0.0).then_some(1.0),
            "geometry.head" => match self.geometry {
                Geometry::Proportion { head, .. } => Some(head),
                _ => None,
            },
            "surface.bands" => Some(self.surface.bands.max(1) as f32),
            "pass.px" => match self.pass {
                Pass::Pixel { px, .. } => Some(px),
                _ => None,
            },
            "pass.blur" => match self.pass {
                Pass::Tiltshift { blur, .. } => Some(blur),
                _ => None,
            },
            "pass.grain" => match self.pass {
                Pass::Paper { grain, .. } => Some(grain),
                _ => None,
            },
            _ => None,
        }
    }

    /// The three stages with `values` applied (control id -> value; a
    /// missing control keeps its default; values are clamped to the
    /// control's range).
    pub fn with_controls(&self, values: &BTreeMap<String, f32>) -> (Geometry, Surface, Pass) {
        let get = |id: &str| -> Option<f32> {
            let c = control(id)?;
            self.controls.iter().any(|x| x == id).then_some(())?;
            values.get(id).map(|v| v.clamp(c.min, c.max))
        };
        let mut g = self.geometry.clone();
        match &mut g {
            Geometry::Voxel { cell, .. } => {
                if let Some(k) = get("geometry.cell") {
                    *cell *= k;
                }
            }
            Geometry::Lowpoly { target_tris, .. } | Geometry::Papercraft { target_tris, .. } => {
                if let Some(k) = get("geometry.detail") {
                    *target_tris = ((*target_tris as f32) * k).round() as u32;
                }
            }
            Geometry::Lumpy { amount } | Geometry::Ridges { amount } => {
                if let Some(k) = get("geometry.amount") {
                    *amount *= k;
                }
            }
            Geometry::Proportion { head, .. } => {
                if let Some(v) = get("geometry.head") {
                    *head = v;
                }
            }
            Geometry::None => {}
        }
        let mut s = self.surface.clone();
        if let Some(v) = get("surface.bands") {
            s.bands = v.round() as u32;
        }
        if let Some(k) = get("surface.outline") {
            s.outline *= k;
        }
        let mut p = self.pass.clone();
        match &mut p {
            Pass::Pixel { px, .. } => {
                if let Some(v) = get("pass.px") {
                    *px = v;
                }
            }
            Pass::Tiltshift { blur, .. } => {
                if let Some(v) = get("pass.blur") {
                    *blur = v;
                }
            }
            Pass::Paper { grain, .. } => {
                if let Some(v) = get("pass.grain") {
                    *grain = v;
                }
            }
            Pass::None | Pass::Silhouette { .. } => {}
        }
        (g, s, p)
    }
}

/// A style's `studio` block in styles.json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioBlock {
    pub geometry: Geometry,
    pub pass: Pass,
    #[serde(default)]
    pub controls: Vec<String>,
}

/// Every style, split.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleCatalog {
    pub styles: Vec<StyleDef>,
}

impl StyleCatalog {
    /// Read the viewer's styles.json: each style's `studio` block and the
    /// surface fields of its `render` block. Every style must have a
    /// `studio` block (no style is left without a definition).
    pub fn parse(styles_json: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(styles_json).map_err(|e| format!("styles.json: {e}"))?;
        let list = v.get("styles").and_then(Value::as_array).ok_or("styles.json: no `styles` list")?;
        let mut styles = Vec::with_capacity(list.len());
        for s in list {
            let str_of = |k: &str| s.get(k).and_then(Value::as_str).map(str::to_string);
            let id = str_of("id").ok_or("styles.json: a style lacks `id`")?;
            let err = |e: String| format!("styles.json: style `{id}`: {e}");
            let block: StudioBlock = serde_json::from_value(s.get("studio").cloned().ok_or_else(|| err("no `studio` block (geometry, pass, controls)".into()))?).map_err(|e| err(format!("studio: {e}")))?;
            let surface: Surface = match s.get("render") {
                Some(r) => serde_json::from_value(r.clone()).map_err(|e| err(format!("render: {e}")))?,
                None => Surface::default(),
            };
            block.geometry.validate().map_err(err)?;
            block.pass.validate().map_err(err)?;
            for c in &block.controls {
                if control(c).is_none() {
                    return Err(err(format!("control `{c}` isn't one of {}", CONTROLS.iter().map(|c| c.id).collect::<Vec<_>>().join(", "))));
                }
            }
            let def = StyleDef {
                name: str_of("name").ok_or_else(|| err("no `name`".into()))?,
                short: str_of("short").ok_or_else(|| err("no `short`".into()))?,
                geometry: block.geometry,
                surface,
                pass: block.pass,
                controls: block.controls,
                id: id.clone(),
            };
            if let Some(c) = def.controls.iter().find(|c| def.control_default(c).is_none()) {
                return Err(err(format!("control `{c}` does nothing in this style's stages")));
            }
            if styles.iter().any(|x: &StyleDef| x.id == id) {
                return Err(err("listed twice".into()));
            }
            styles.push(def);
        }
        if styles.is_empty() {
            return Err("styles.json: no styles".into());
        }
        Ok(Self { styles })
    }

    pub fn style(&self, id: &str) -> Option<&StyleDef> {
        self.styles.iter().find(|s| s.id == id)
    }
}
