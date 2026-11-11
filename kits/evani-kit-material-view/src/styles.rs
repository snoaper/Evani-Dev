//! The 24 art styles' starter materials (`data/styles.json`), and the state
//! behind the viewer's style row: which style and material are selected, the
//! test shape, and the user's edits per material.
//!
//! The selected material's finish lives in five params under
//! `kit.material_view.style` (tint, roughness, metallic, bump, glow), so the
//! panel and an AI over evani-live `param_set` edit it the same way.
//! [`sync_system`] stores any such change as an edit of the selected
//! material and writes the next material's finish when the selection moves.
//!
//! Only builds with the app's `material-styles` feature call [`install`];
//! no other build gets these params.

use std::collections::BTreeMap;
use std::rc::Rc;

use serde::Deserialize;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::num::clamp;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};

pub const STYLES_JSON: &str = include_str!("../data/styles.json");

/// Every style param sits under this prefix.
pub const PREFIX: &str = "kit.material_view.style";
pub const TINT: &str = "kit.material_view.style.tint";
pub const ROUGHNESS: &str = "kit.material_view.style.roughness";
pub const METALLIC: &str = "kit.material_view.style.metallic";
pub const BUMP: &str = "kit.material_view.style.bump";
pub const GLOW: &str = "kit.material_view.style.glow";

/// The renderer's material slots; a higher slot draws as the last one.
pub const RENDER_SLOTS: usize = 16;

/// Lowest roughness the param allows (drafts must sit inside it, so a
/// reset writes them exactly).
const ROUGHNESS_MIN: f32 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Rough starter numbers, not art-directed yet.
    Draft,
    /// Taken live from a look built into the viewer.
    Look,
    /// Nothing to show yet.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StyleMaterial {
    pub id: String,
    pub name: String,
    /// Linear RGB, 0..1.
    pub color: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub bump: f32,
    pub glow: f32,
    /// The look material its values come from (`status: look` only); the
    /// numbers above are the fallback.
    #[serde(default)]
    pub look_material: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Style {
    pub id: String,
    pub num: u32,
    pub name: String,
    /// Label for a button.
    pub short: String,
    /// The style in a few original words (what it looks like). Never the
    /// name of an existing game, studio or brand: a test checks the data.
    pub mood: String,
    /// The local reference folder (dev only, never shipped).
    pub refs: String,
    pub status: Status,
    #[serde(default)]
    pub look: Option<String>,
    pub note: String,
    pub materials: Vec<StyleMaterial>,
    /// How the shapes are drawn in this style beyond their finish (toon
    /// bands, ink outlines, voxels...). Absent: lit as they are.
    #[serde(default)]
    pub render: Option<Render>,
}

/// Whether a style's shape treatment is its own look or a stand-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fidelity {
    /// The style's defining look (cel bands, outlines, voxels, facets...).
    True,
    /// A stand-in for a look the viewer can't draw (a brush, paper, a
    /// screen-space pixel grid): close, not the real thing.
    #[default]
    Approx,
}

/// A style's shape treatment (`render` in styles.json). The viewer builds it
/// into each shape's model: no engine pass or shader, so every value here is
/// data the page and the tests can read. The five style sliders still act:
/// shine sizes the highlight, metal deepens the bands, bump wobbles them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Render {
    /// One line for the page: what the shapes show.
    pub label: String,
    pub fidelity: Fidelity,
    /// Light bands: 0 = smooth (the engine's lighting), 1 = one flat
    /// colour, 2 to 6 = hard cel bands from the scene's sun.
    pub bands: u32,
    /// The darkest band's brightness (0 to 1).
    pub shadow: f32,
    /// Shadow colour shift, multiplied into the dark bands (0 to 2 each).
    pub shadow_hue: [f32; 3],
    /// A hard highlight spot facing the sun (0 = none, 1 = white).
    pub highlight: f32,
    /// Ink outline width in shape units (radius 1; 0 = none).
    pub outline: f32,
    pub outline_color: [f32; 3],
    /// The outline glows in the material's colour (neon), or in
    /// `outline_color` when the material is too grey to give a hue.
    pub outline_glow: bool,
    /// Flat faces: one normal per triangle.
    pub facets: bool,
    /// Low-poly: merge vertices on a grid of this size (0 = off).
    pub cluster: f32,
    /// Voxels: rebuild the shape from cubes of this size (0 = off).
    pub voxel: f32,
    /// Colour levels per channel (0 = off).
    pub posterize: u32,
    /// Two-tone ordered dither between `outline_color` (ink) and `paper`.
    pub dither: bool,
    pub paper: [f32; 3],
    /// Patchy pigment: colour jitter over the surface (0 = none).
    pub mottle: f32,
    /// Hand-pressed lumps in the silhouette (clay; 0 = none).
    pub lumpy: f32,
    /// Knitted ridges around the shape (yarn; 0 = none).
    pub ridges: f32,
    /// Grey the colours (0 = none, 1 = grey).
    pub desaturate: f32,
    /// Brightness multiplier.
    pub value: f32,
    /// Side light: the least angle (degrees) between the light the bands
    /// and the dither are cut from and the view from the home camera, so
    /// a two-tone style shows a real shadow side, not a sliver at the rim
    /// (0 = the sun as it is, kept within 50 degrees of the view).
    pub side_light: f32,
    /// A catalogue material's own pattern (grain, stripes, scales, rust)
    /// inside this style, drawn by the recipe stage's style detail step: 0 =
    /// the flat main colour only, 1 = the pattern as it is, up to 2. Live as
    /// `kit.material_view.detail.<style>.strength`.
    pub detail: f32,
    /// The pattern posterized in steps of this many stops (0 = smooth, up to
    /// 1.75): bigger steps leave fewer, cleaner strokes. Live as
    /// `kit.material_view.detail.<style>.step`.
    pub detail_step: f32,
}

impl Default for Render {
    fn default() -> Self {
        Self {
            label: String::new(),
            fidelity: Fidelity::Approx,
            bands: 0,
            shadow: 0.45,
            shadow_hue: [1.0; 3],
            highlight: 0.0,
            outline: 0.0,
            outline_color: [0.03, 0.025, 0.03],
            outline_glow: false,
            facets: false,
            cluster: 0.0,
            voxel: 0.0,
            posterize: 0,
            dither: false,
            paper: [0.92, 0.88, 0.78],
            mottle: 0.0,
            lumpy: 0.0,
            ridges: 0.0,
            desaturate: 0.0,
            value: 1.0,
            side_light: 0.0,
            detail: 1.0,
            detail_step: 0.5,
        }
    }
}

impl Render {
    /// Does it change the shapes at all? (A label alone doesn't.)
    pub fn shaped(&self) -> bool {
        self.bands > 0
            || self.outline > 0.0
            || self.facets
            || self.cluster > 0.0
            || self.voxel > 0.0
            || self.posterize > 0
            || self.dither
            || self.mottle > 0.0
            || self.lumpy > 0.0
            || self.ridges > 0.0
            || self.desaturate > 0.0
            || self.value != 1.0
            || self.shadow_hue != [1.0; 3]
    }

    fn validate(&self) -> Result<(), String> {
        let within = |x: f32, lo: f32, hi: f32| x.is_finite() && (lo..=hi).contains(&x);
        let colour = |c: [f32; 3], hi: f32| c.iter().all(|&x| within(x, 0.0, hi));
        let checks = [
            (!self.label.trim().is_empty(), "needs a label"),
            (self.bands <= 6, "bands is 0 to 6"),
            (within(self.shadow, 0.0, 1.0), "shadow is 0 to 1"),
            (colour(self.shadow_hue, 2.0), "shadow_hue is 0 to 2"),
            (within(self.highlight, 0.0, 1.0), "highlight is 0 to 1"),
            (within(self.outline, 0.0, 0.2), "outline is 0 to 0.2"),
            (colour(self.outline_color, 1.0) && colour(self.paper, 1.0), "colours are 0 to 1"),
            (within(self.cluster, 0.0, 0.6), "cluster is 0 to 0.6"),
            (self.voxel == 0.0 || within(self.voxel, 0.04, 0.5), "voxel is 0 or 0.04 to 0.5"),
            (self.posterize == 0 || (2..=16).contains(&self.posterize), "posterize is 0 or 2 to 16"),
            (within(self.mottle, 0.0, 0.5), "mottle is 0 to 0.5"),
            (within(self.lumpy, 0.0, 0.3), "lumpy is 0 to 0.3"),
            (within(self.ridges, 0.0, 0.2), "ridges is 0 to 0.2"),
            (within(self.desaturate, 0.0, 1.0), "desaturate is 0 to 1"),
            (within(self.value, 0.02, 2.0), "value is 0.02 to 2"),
            (within(self.detail, 0.0, 2.0), "detail is 0 to 2"),
            (within(self.detail_step, 0.0, 1.75), "detail_step is 0 to 1.75"),
        ];
        match checks.iter().find(|(ok, _)| !ok) {
            Some((_, why)) => Err((*why).to_string()),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Styles {
    pub library: String,
    pub version: u32,
    /// Where the dev page finds the reference pictures.
    pub refs_url: String,
    /// Pictures per style (`1.jpg`..).
    pub refs_count: u32,
    pub styles: Vec<Style>,
}

impl Styles {
    /// Parse the shipped file.
    pub fn shipped() -> Result<Self, String> {
        Self::parse(STYLES_JSON)
    }

    /// Parse and check the file.
    pub fn parse(json: &str) -> Result<Self, String> {
        let s: Styles = serde_json::from_str(json).map_err(|e| format!("styles.json: {e}"))?;
        s.validate()?;
        Ok(s)
    }

    fn validate(&self) -> Result<(), String> {
        if self.styles.is_empty() {
            return Err("styles.json: no styles".into());
        }
        for (i, s) in self.styles.iter().enumerate() {
            if self.styles[..i].iter().any(|o| o.id == s.id) {
                return Err(format!("style `{}`: id used twice", s.id));
            }
            if s.num as usize != i + 1 {
                return Err(format!("style `{}`: number {} should be {}", s.id, s.num, i + 1));
            }
            if s.refs.is_empty() || s.name.is_empty() || s.short.is_empty() {
                return Err(format!("style `{}`: needs a name, a short label and a refs folder", s.id));
            }
            if s.status == Status::Look && s.look.is_none() {
                return Err(format!("style `{}`: status `look` needs a `look`", s.id));
            }
            if let Some(r) = &s.render {
                r.validate().map_err(|e| format!("style `{}`: render: {e}", s.id))?;
            }
            for (j, m) in s.materials.iter().enumerate() {
                if s.materials[..j].iter().any(|o| o.id == m.id) {
                    return Err(format!("style `{}`: material `{}` used twice", s.id, m.id));
                }
                let unit = |x: f32| x.is_finite() && (0.0..=1.0).contains(&x);
                let ok = m.color.iter().all(|&c| unit(c))
                    && unit(m.roughness)
                    && m.roughness >= ROUGHNESS_MIN
                    && unit(m.metallic)
                    && unit(m.bump)
                    && unit(m.glow);
                if !ok {
                    return Err(format!("style `{}`, material `{}`: a value is out of range", s.id, m.id));
                }
            }
        }
        Ok(())
    }
}

/// One material's finish: the five style params' values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finish {
    pub tint: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub bump: f32,
    pub glow: f32,
}

/// The parsed styles file (shared, read-only).
#[derive(Debug, Clone)]
pub struct StylesData(pub Rc<Styles>);

/// Handles of the five style params.
#[derive(Debug, Clone, Copy)]
pub struct StyleParams {
    pub tint: Vec3Param,
    pub roughness: F32Param,
    pub metallic: F32Param,
    pub bump: F32Param,
    pub glow: F32Param,
}

/// What the page asks the style row to do.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleCommand {
    /// Open style `i` (its first material).
    Style(usize),
    /// Select material `i` of the open style.
    Select(usize),
    /// The test shape the materials sit on.
    Shape(String),
    /// Show every material at once; `None` toggles.
    ShowAll(Option<bool>),
    /// The selected material back to its default.
    Reset,
    /// Every edit of every style dropped.
    ResetAll,
}

/// The style row's state.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleView {
    /// Index into the styles.
    pub style: usize,
    /// Index into the open style's materials.
    pub selected: usize,
    pub shape: String,
    pub show_all: bool,
    /// The user's edits, keyed by (style id, material id).
    pub edits: BTreeMap<(String, String), Finish>,
    /// The params as last written or seen; a difference is an edit.
    synced: Option<Finish>,
    pub pending: Vec<StyleCommand>,
    /// Bumped on every change the page shows.
    pub revision: u64,
}

impl Default for StyleView {
    fn default() -> Self {
        Self {
            style: 0,
            selected: 0,
            shape: "sphere".to_string(),
            show_all: true,
            edits: BTreeMap::new(),
            synced: None,
            pending: Vec::new(),
            revision: 1,
        }
    }
}

impl StyleView {
    pub fn push(&mut self, c: StyleCommand) {
        self.pending.push(c);
    }

    /// The (style id, material id) of the selection, when it can be edited.
    fn key(&self, styles: &Styles) -> Option<(String, String)> {
        let s = styles.styles.get(self.style).filter(|s| s.status != Status::Missing)?;
        let m = s.materials.get(self.selected)?;
        Some((s.id.clone(), m.id.clone()))
    }
}

/// A material's default finish: its draft numbers, or for a look style the
/// look material it names. The warning (shown on the page) says when the
/// look material can't be used as it is; never a silent fallback.
pub fn default_finish(style: &Style, m: &StyleMaterial, look: Option<&LookData>) -> (Finish, Option<String>) {
    let draft = Finish { tint: m.color, roughness: m.roughness, metallic: m.metallic, bump: m.bump, glow: m.glow };
    let Some(name) = m.look_material.as_deref().filter(|_| style.status == Status::Look) else {
        return (draft, None);
    };
    let missing = || format!("Look material `{name}` isn't in this build: showing draft values.");
    let Some(look) = look else { return (draft, Some(missing())) };
    let Some(slot) = look.materials.iter().position(|d| d.name == name) else {
        return (draft, Some(missing()));
    };
    let d = &look.materials[slot];
    let glow = clamp(d.emissive.iter().fold(0.0_f32, |a, &e| a.max(e)), 0.0, 1.0);
    let f = Finish {
        tint: [d.base_color[0], d.base_color[1], d.base_color[2]],
        roughness: d.roughness,
        metallic: d.metallic,
        // A look material has no relief of its own: the style's bump draws
        // it (critic round 3: Painterly's grass, fabric and leather were
        // smooth balls).
        bump: m.bump,
        glow,
    };
    let warn = (slot >= RENDER_SLOTS).then(|| {
        format!(
            "Look material `{name}` is in slot {slot}, past the renderer's {RENDER_SLOTS} slots: it would draw as slot {}.",
            RENDER_SLOTS - 1
        )
    });
    (f, warn)
}

/// A material's finish now: the user's edit if any (`true`), else its
/// default, with any warning. `None` for a missing style or material.
pub fn finish(view: &StyleView, styles: &Styles, look: Option<&LookData>, style: usize, mat: usize) -> Option<(Finish, bool, Option<String>)> {
    let s = styles.styles.get(style).filter(|s| s.status != Status::Missing)?;
    let m = s.materials.get(mat)?;
    let (def, warn) = default_finish(s, m, look);
    match view.edits.get(&(s.id.clone(), m.id.clone())) {
        Some(e) => Some((*e, true, warn)),
        None => Some((def, false, warn)),
    }
}

fn read(reg: &ParamRegistry, p: StyleParams) -> Finish {
    Finish {
        tint: reg.vec3(p.tint),
        roughness: reg.f32(p.roughness),
        metallic: reg.f32(p.metallic),
        bump: reg.f32(p.bump),
        glow: reg.f32(p.glow),
    }
}

fn write(reg: &mut ParamRegistry, p: StyleParams, f: Finish) {
    let _ = reg.set(p.tint.id(), ParamValue::Color(f.tint));
    let _ = reg.set_f32(p.roughness, f.roughness);
    let _ = reg.set_f32(p.metallic, f.metallic);
    let _ = reg.set_f32(p.bump, f.bump);
    let _ = reg.set_f32(p.glow, f.glow);
}

/// Keep the style params and the edits in step (Stage::Frame).
pub fn sync_system(world: &mut World) {
    let Some(styles) = world.try_res::<StylesData>().map(|s| s.0.clone()) else { return };
    let Some(p) = world.try_res::<StyleParams>().map(|p| *p) else { return };
    let Some(mut view) = world.try_res_mut::<StyleView>() else { return };
    let mut reg = world.res_mut::<ParamRegistry>();
    let look = world.try_res::<LookData>();
    let look = look.as_deref();

    // An edit by the panel or over evani-live: it belongs to the selection.
    let now = read(&reg, p);
    if view.synced.is_some_and(|s| s != now) {
        if let Some(key) = view.key(&styles) {
            let s = &styles.styles[view.style];
            let (default, _) = default_finish(s, &s.materials[view.selected], look);
            // Back on the default is no edit.
            if default == now {
                view.edits.remove(&key);
            } else {
                view.edits.insert(key, now);
            }
        }
        view.synced = Some(now);
        view.revision += 1;
    }

    // The page's commands, in order.
    let cmds = std::mem::take(&mut view.pending);
    let mut rewrite = view.synced.is_none();
    for c in cmds {
        match c {
            StyleCommand::Style(i) => {
                view.style = i.min(styles.styles.len().saturating_sub(1));
                view.selected = 0;
                rewrite = true;
            }
            StyleCommand::Select(i) => {
                let n = styles.styles.get(view.style).map_or(0, |s| s.materials.len());
                view.selected = i.min(n.saturating_sub(1));
                rewrite = true;
            }
            StyleCommand::Shape(id) => view.shape = id,
            StyleCommand::ShowAll(t) => view.show_all = t.unwrap_or(!view.show_all),
            StyleCommand::Reset => {
                if let Some(key) = view.key(&styles) {
                    view.edits.remove(&key);
                }
                rewrite = true;
            }
            StyleCommand::ResetAll => {
                view.edits.clear();
                rewrite = true;
            }
        }
        view.revision += 1;
    }

    // Show the selection's finish; a style with nothing to show leaves the
    // params alone.
    if rewrite && let Some((f, _, _)) = finish(&view, &styles, look, view.style, view.selected) {
        write(&mut reg, p, f);
        view.synced = Some(read(&reg, p));
    }
}

/// Register the style params, data and system. The app calls this only when
/// built with its `material-styles` feature.
pub fn install(ctx: &mut PluginContext) -> Result<(), PluginError> {
    let styles = Styles::shipped().map_err(PluginError)?;
    let first = styles
        .styles
        .first()
        .and_then(|s| s.materials.first())
        .map_or(Finish { tint: [0.5; 3], roughness: 0.5, metallic: 0.0, bump: 0.0, glow: 0.0 }, |m| Finish {
            tint: m.color,
            roughness: m.roughness,
            metallic: m.metallic,
            bump: m.bump,
            glow: m.glow,
        });
    let tint = ctx.param_vec3(ParamDef::color(TINT, first.tint).describe("Base colour of the selected material."))?;
    let mut unit = |name: &str, v: f32, min: f32, what: &str| ctx.param_f32(ParamDef::f32(name, v).range(min, 1.0).step(0.01).describe(what));
    let roughness = unit(ROUGHNESS, first.roughness, ROUGHNESS_MIN, "Roughness: low is shiny, high is matte.")?;
    let metallic = unit(METALLIC, first.metallic, 0.0, "Metallic: 0 is paint or plastic, 1 is bare metal.")?;
    let bump = unit(BUMP, first.bump, 0.0, "Bump strength: tilts the surface normals with fine noise.")?;
    let glow = unit(GLOW, first.glow, 0.0, "Glow: the selected material's own light.")?;
    ctx.data_file("material_view.styles", STYLES_JSON);
    ctx.resource(StylesData(Rc::new(styles)));
    ctx.resource(StyleParams { tint, roughness, metallic, bump, glow });
    ctx.resource(StyleView::default());
    ctx.system(SystemDesc::new("material_view.styles", Stage::Frame, sync_system));
    Ok(())
}

#[cfg(test)]
mod tests {
    use evani_core::App;
    use evani_core::plugin::MaterialDesc;

    use super::*;

    const DT: f64 = 1000.0 / 60.0;

    fn app() -> App {
        let mut a = App::new("t");
        install(&mut a.context("material-view")).unwrap();
        a
    }

    fn params(a: &App) -> Finish {
        let p = *a.world.res::<StyleParams>();
        read(&a.world.res::<ParamRegistry>(), p)
    }

    fn draft(style: usize, mat: usize) -> Finish {
        let s = Styles::shipped().unwrap();
        let m = &s.styles[style].materials[mat];
        Finish { tint: m.color, roughness: m.roughness, metallic: m.metallic, bump: m.bump, glow: m.glow }
    }

    fn push(a: &mut App, c: StyleCommand) {
        a.world.res_mut::<StyleView>().push(c);
        a.run_headless(1, DT);
    }

    #[test]
    fn the_shipped_styles_parse_in_order() {
        let s = Styles::shipped().unwrap();
        assert_eq!(s.styles.len(), 24);
        for (i, st) in s.styles.iter().enumerate() {
            assert_eq!(st.id, format!("WK{:02}", i + 1));
            assert!(st.refs.starts_with(&st.id), "{}", st.refs);
        }
        let names: Vec<_> = s.styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names[0], "Bright Toon");
        assert_eq!(names[4], "Painterly 3D");
        assert_eq!(names[9], "Cinematic Pixel");
        assert_eq!(names[23], "Cinematic Realism");
        let looks: Vec<_> = s.styles.iter().filter(|s| s.status == Status::Look).map(|s| s.id.as_str()).collect();
        assert_eq!(looks, ["WK05"]);
        assert_eq!(s.styles[4].look.as_deref(), Some("stylizedpbr"));
        for st in s.styles.iter().filter(|s| s.status == Status::Draft) {
            assert!(st.materials.len() >= 4, "{}", st.id);
            assert!(st.materials.iter().all(|m| m.look_material.is_none()), "{}", st.id);
        }
        assert!(s.styles[4].materials.iter().all(|m| m.look_material.is_some()));
    }

    #[test]
    fn a_bad_styles_file_is_refused() {
        let twice = STYLES_JSON.replacen("\"id\": \"WK02\"", "\"id\": \"WK01\"", 1);
        assert!(Styles::parse(&twice).unwrap_err().contains("twice"));
        let order = STYLES_JSON.replacen("\"num\": 2,", "\"num\": 3,", 1);
        assert!(Styles::parse(&order).unwrap_err().contains("number"));
        let range = STYLES_JSON.replacen("\"roughness\": 0.25,", "\"roughness\": 1.5,", 1);
        assert!(Styles::parse(&range).unwrap_err().contains("range"));
        let look = STYLES_JSON.replacen("\"look\": \"stylizedpbr\"", "\"look\": null", 1);
        assert!(Styles::parse(&look).unwrap_err().contains("needs a `look`"));
        let bands = STYLES_JSON.replacen("\"bands\": 3,", "\"bands\": 9,", 1);
        assert!(Styles::parse(&bands).unwrap_err().contains("render: bands"));
        let voxel = STYLES_JSON.replacen("\"voxel\": 0.12", "\"voxel\": 0.01", 1);
        assert!(Styles::parse(&voxel).unwrap_err().contains("render: voxel"));
        let unknown = STYLES_JSON.replacen("\"bands\": 3,", "\"bandz\": 3,", 1);
        assert!(Styles::parse(&unknown).unwrap_err().contains("bandz"));
    }

    /// Every style says what its shapes show; all but the two lit by the
    /// engine as they are (the film look and realism) change the shapes,
    /// and the families get their defining treatment.
    #[test]
    fn every_style_has_a_shape_look() {
        let s = Styles::shipped().unwrap();
        let by = |id: &str| s.styles.iter().find(|x| x.id == id).and_then(|x| x.render.clone()).unwrap();
        for st in &s.styles {
            let r = st.render.as_ref().unwrap_or_else(|| panic!("{} has no render", st.id));
            assert!(!r.label.is_empty());
            assert_eq!(r.shaped(), !matches!(st.id.as_str(), "WK05" | "WK24"), "{}", st.id);
        }
        // Toon and cel families: hard bands; cel, anime and comic ink lines.
        for id in ["WK01", "WK02", "WK03", "WK04", "WK17"] {
            assert!(by(id).bands >= 2 && by(id).fidelity == Fidelity::True, "{id}");
        }
        for id in ["WK03", "WK04", "WK12", "WK15", "WK16", "WK17"] {
            assert!(by(id).outline > 0.0, "{id}");
        }
        assert!(by("WK06").facets && by("WK06").cluster > 0.0);
        assert!(by("WK08").voxel > 0.0 && by("WK08").fidelity == Fidelity::True);
        assert!(by("WK09").voxel > 0.0 && by("WK09").posterize > 0 && by("WK09").fidelity == Fidelity::Approx);
        assert!(by("WK21").dither && by("WK23").outline_glow);
        assert!(by("WK19").ridges > 0.0 && by("WK20").lumpy > 0.0);
    }

    #[test]
    fn edits_follow_the_selected_material() {
        let mut a = app();
        a.run_headless(1, DT);
        assert_eq!(params(&a), draft(0, 0), "first frame writes the defaults");
        // An AI over evani-live sets a param.
        let p = *a.world.res::<StyleParams>();
        a.world.res_mut::<ParamRegistry>().set_f32(p.roughness, 0.9).unwrap();
        a.run_headless(1, DT);
        let edited = Finish { roughness: 0.9, ..draft(0, 0) };
        let key = ("WK01".to_string(), "red_plastic".to_string());
        assert_eq!(a.world.res::<StyleView>().edits.get(&key), Some(&edited));
        // Another material shows its own default; back again, the edit returns.
        push(&mut a, StyleCommand::Select(1));
        assert_eq!(params(&a), draft(0, 1));
        push(&mut a, StyleCommand::Select(0));
        assert_eq!(params(&a), edited);
        // Reset is exact.
        push(&mut a, StyleCommand::Reset);
        assert_eq!(params(&a), draft(0, 0));
        assert!(a.world.res::<StyleView>().edits.is_empty());
        // A style change keeps other styles' edits; ResetAll clears them.
        a.world.res_mut::<ParamRegistry>().set_f32(p.glow, 0.4).unwrap();
        a.run_headless(1, DT);
        push(&mut a, StyleCommand::Style(22));
        assert_eq!(params(&a), draft(22, 0));
        assert_eq!(a.world.res::<StyleView>().selected, 0);
        a.world.res_mut::<ParamRegistry>().set_f32(p.bump, 0.7).unwrap();
        a.run_headless(1, DT);
        assert_eq!(a.world.res::<StyleView>().edits.len(), 2);
        push(&mut a, StyleCommand::Style(0));
        assert_eq!(params(&a), Finish { glow: 0.4, ..draft(0, 0) });
        push(&mut a, StyleCommand::ResetAll);
        assert!(a.world.res::<StyleView>().edits.is_empty());
        assert_eq!(params(&a), draft(0, 0));
        // Out-of-range picks are clamped; shape and show-all are kept.
        push(&mut a, StyleCommand::Select(99));
        assert_eq!(a.world.res::<StyleView>().selected, 3);
        push(&mut a, StyleCommand::Shape("cube".into()));
        push(&mut a, StyleCommand::ShowAll(None));
        let v = a.world.res::<StyleView>();
        assert_eq!((v.shape.as_str(), v.show_all), ("cube", false));
    }

    #[test]
    fn a_look_style_reads_the_look_and_warns_when_it_cant() {
        let s = Styles::shipped().unwrap();
        let wk05 = &s.styles[4];
        let metal = wk05.materials.iter().find(|m| m.look_material.as_deref() == Some("metal")).unwrap();
        // Not in this build: draft values, with a warning.
        let (f, warn) = default_finish(wk05, metal, Some(&LookData::default()));
        assert_eq!(f.roughness, metal.roughness);
        assert!(warn.unwrap().contains("isn't in this build"));
        // In the look: its values.
        let mut look = LookData::default();
        let mut d = MaterialDesc::new("metal", [0.7, 0.6, 0.5, 1.0]);
        d.roughness = 0.3;
        d.metallic = 0.9;
        d.emissive = [0.0, 2.0, 0.5];
        look.materials.push(d.clone());
        let (f, warn) = default_finish(wk05, metal, Some(&look));
        assert_eq!(f, Finish { tint: [0.7, 0.6, 0.5], roughness: 0.3, metallic: 0.9, bump: 0.0, glow: 1.0 });
        assert!(warn.is_none());
        // Past the renderer's slots: still its values, with a warning.
        let mut far = LookData::default();
        for i in 0..RENDER_SLOTS {
            far.materials.push(MaterialDesc::new(&format!("m{i}"), [1.0; 4]));
        }
        far.materials.push(d);
        let (f, warn) = default_finish(wk05, metal, Some(&far));
        assert_eq!(f.metallic, 0.9);
        assert!(warn.unwrap().contains("slot 16"));
        // In a running app the look style shows the look's values.
        let mut a = app();
        a.world.res_mut::<LookData>().materials = look.materials.clone();
        a.run_headless(1, DT);
        push(&mut a, StyleCommand::Style(4));
        let stone = finish(&a.world.res::<StyleView>(), &s, Some(&look), 4, 0).unwrap();
        assert!(stone.2.is_some(), "stone isn't in that look");
        push(&mut a, StyleCommand::Select(1));
        assert_eq!(params(&a).metallic, 0.9);
    }
}
