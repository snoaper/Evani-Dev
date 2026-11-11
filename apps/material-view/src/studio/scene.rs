//! WP4 (docs/studio/WORK_PACKAGES.md): the studio scene, the viewport that
//! draws whatever the one selection record says, and nothing else.
//!
//! - **What it draws.** Each frame it resolves the record (or the 2 to 4
//!   records of comparison mode, side by side) and draws the asset: its
//!   source (WP2's loader, kept by [`super::scene_source::StudioSources`]) through the style's
//!   geometry stage ([`super::scene_source::StyledCache`]), each region in
//!   its bound material and tone, in the style's surface treatment
//!   ([`super::scene_build`]). One model slot per view, made once
//!   ([`Models::add_glb`]) and swapped in place ([`Models::replace_glb`]), so
//!   the custom-model cap is never touched.
//! - **Never another object.** An asset still being built draws nothing but
//!   the floor and says so ([`ViewState::NotBuilt`]); a model on its way says
//!   it is loading; while a style's shape is made (or its generator isn't
//!   built yet) the *same asset* is drawn plain and marked, then the styled
//!   one is swapped in. There is no stand-in ball anywhere on this path.
//! - **What rebuilds.** Only a new [`BuildKey`]: the record's variant key
//!   (asset, geometry, bindings, surface, edits), whether the styled shape
//!   is in, the display mode. The light preset and the quality tier never
//!   enter it ([`StudioScene::builds`] stays still: tested), and the bands
//!   are cut from a fixed key light ([`super::scene_build::band_light`]).
//! - **The camera.** A new asset (or comparison set) is framed from its
//!   drawn bounds and its row's `frame` (yaw, pitch, the share of the view
//!   it fills): that becomes the orbit's home, so Reset goes back to it. A
//!   record with a remembered camera keeps it (WP1's bridge puts it back).
//!   Orbit, pan and zoom are the viewer's own (drag, wheel, pinch, the
//!   viewport's tool rail).
//! - **Display modes** ([`Display`], param `kit.material_view.studio.view.display`):
//!   render view, material only (the asset's own shape in its materials,
//!   no style), wireframe (the drawn shape's edges).
//!
//! The page (WP6) lays the panels out round the viewport; the viewport's
//! own overlay (tool rail, combination card, state label, view modes) is
//! `scene_hud.rs`.

use std::collections::BTreeMap;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::ErrorLog;
use evani_core::params::{EnumParam, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::PluginContext;
use evani_kit_material_view::Viewer;
use evani_kit_material_view::catalogue::{CatalogueView, DetailParams, LibraryData, working_row};
use evani_kit_material_view::studio::evani_studio::assets::{AssetRow, Tone};
use evani_kit_material_view::studio::evani_studio::geo::StyledMesh;
use evani_kit_material_view::studio::evani_studio::hash::VariantKey;
use evani_kit_material_view::studio::evani_studio::resolve::{Availability, BoundBy, geometry_key, resolve};
use evani_kit_material_view::studio::evani_studio::styles::{Geometry, Pass, Surface};
use evani_kit_material_view::studio::{Catalogs, Resolved, Selection, Studio, StudioData};
use evani_kit_material_view::styles::Render;
use evani_material_recipes::style::RecipeStyle;
use evani_render::RenderWorld;
use evani_render::model::{ModelId, ModelInstance, Models};

use super::loader::StudioSource;
use super::scene_build::{self, Build, Display, RegionLook};
use super::scene_source::{Source, Styled, StudioSources, StyledCache};
use crate::scenes::common::{FLOOR_SAND, floor, material};

/// The viewer scene the studio draws in (library.json).
pub const SCENE: &str = "studio";
/// Comparison mode draws at most this many records.
pub const MAX_VIEWS: usize = 4;
/// The display mode param (render, material, wireframe).
pub const DISPLAY: &str = "kit.material_view.studio.view.display";
/// An asset's radius on the stage, metres (sources are radius 1).
pub const SIZE: &str = "kit.material_view.studio.view.size";
/// The gap between compared assets, metres.
pub const GAP: &str = "kit.material_view.studio.view.gap";
/// The wireframe's strip width, as a share of the asset's radius.
pub const WIRE: &str = "kit.material_view.studio.view.wire";

/// The scene's registered params.
#[derive(Debug, Clone, Copy)]
pub struct SceneParams {
    pub display: EnumParam,
    pub size: F32Param,
    pub gap: F32Param,
    pub wire: F32Param,
}

/// Where a view stands.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewState {
    /// Drawn in its style.
    Ready,
    /// The same asset drawn plain while its style's shape is made.
    Generating,
    /// The style's shape generator isn't built yet: the same asset drawn
    /// plain, marked, with the reason.
    StyleNotBuilt(String),
    /// The asset's model is on its way.
    Loading,
    /// The asset is still being built: nothing drawn but the label.
    NotBuilt,
    /// The source can't be read (the reason; nothing drawn).
    Failed(String),
}

/// One drawn record.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub selection: Selection,
    /// The asset, style and material names, as the page shows them.
    pub names: [String; 3],
    pub state: ViewState,
    /// The model drawn (once it is ready in the table).
    pub model: Option<ModelId>,
    /// Its parts (primitives).
    pub parts: usize,
    /// Where it stands and its scale.
    pub at: [f32; 3],
    pub scale: f32,
    /// Its drawn box, world space.
    pub bounds: [[f32; 3]; 2],
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code, reason = "the viewport overlay (scene_hud.rs, wasm) and the page (WP6) read them"))]
impl View {
    /// "Voxel + Brick + Horse", the mockup's combination title.
    pub fn title(&self) -> String {
        format!("{} + {} + {}", self.names[1], self.names[2], self.names[0])
    }

    /// The state as one plain line for the viewport (`None` when ready).
    pub fn status(&self) -> Option<String> {
        let (asset, style) = (&self.names[0], &self.names[1]);
        match &self.state {
            ViewState::Ready => None,
            ViewState::Generating => Some(format!("Making the {style} {}… showing the plain one meanwhile", asset.to_lowercase())),
            ViewState::StyleNotBuilt(_) => Some(format!("{style} shapes aren't built yet: showing the plain {}", asset.to_lowercase())),
            ViewState::Loading => Some(format!("Loading the {}…", asset.to_lowercase())),
            ViewState::NotBuilt => Some(format!("The {} is being built: not ready to show yet", asset.to_lowercase())),
            ViewState::Failed(_) => Some(format!("The {} can't be shown yet", asset.to_lowercase())),
        }
    }

    /// The technical reason behind the state, for a tooltip.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the viewport overlay (wasm) reads it"))]
    pub fn reason(&self) -> Option<&str> {
        match &self.state {
            ViewState::StyleNotBuilt(r) | ViewState::Failed(r) => Some(r),
            _ => None,
        }
    }
}

/// What a view's model was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildKey {
    pub variant: VariantKey,
    /// The style's own shape (else the plain source while it is made).
    pub styled: bool,
    pub display: Display,
    /// The open catalogue row's revision, when the selected region wears it
    /// (an edit made in the panel).
    pub edited: u64,
    /// The wireframe's strip width (wireframe only).
    pub wire: u32,
    /// The part flashing after it was picked (its regions as bits; 0: none).
    pub flash: u32,
}

/// One view's model slot.
#[derive(Debug, Clone)]
struct Slot {
    model: ModelId,
    key: Option<BuildKey>,
    parts: usize,
    /// The built mesh's box, model units.
    bounds: [[f32; 3]; 2],
    /// The shape drawn (a tap is tested against it).
    mesh: std::rc::Rc<StyledMesh>,
}

/// The framing a camera was set for.
#[derive(Debug, Clone, PartialEq)]
struct FrameKey {
    assets: Vec<String>,
    /// Which views had their real bounds (a model arriving reframes once).
    known: Vec<bool>,
    size: u32,
    gap: u32,
    /// The open part of the view (8 px steps): a panel opening or the
    /// window resizing reframes the home view.
    open: [i32; 4],
}

/// The viewport overlay's bottom bar (scene_hud.rs `.svp-bar`: 10 px up,
/// about 44 px tall) and some air: the asset is framed above it, so its
/// hooves never sit under the view-mode buttons (the 700 px proof shot).
pub const BAR_BAND_PX: f32 = 64.0;

/// The scene's state (a world resource).
#[derive(Debug, Default)]
pub struct StudioScene {
    /// The records drawn this frame (1, or 2 to 4 in comparison).
    pub views: Vec<View>,
    slots: Vec<Option<Slot>>,
    /// Models built so far (adds and replaces). A light or quality change
    /// leaves it still.
    pub builds: u64,
    /// Times the camera was framed for a new asset or set.
    pub framings: u64,
    pub display: Display,
    framed: Option<FrameKey>,
    shown: bool,
    /// Build errors already logged (by key), so one bad row logs once.
    logged: Vec<BuildKey>,
    /// The selected part last frame (asset, part), and the frames its flash
    /// has left ([`super::pick::FLASH_FRAMES`]).
    last_part: Option<(String, String)>,
    flash_left: u32,
    /// Parts picked in the view so far (a tap that met the asset).
    pub picks: u64,
}

impl StudioScene {
    /// The record the page's combination card shows: the selected one.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the viewport overlay (wasm) reads it"))]
    pub fn current(&self, sel: &Selection) -> Option<&View> {
        self.views.iter().find(|v| v.selection == *sel).or(self.views.first())
    }
}

pub fn install(ctx: &mut PluginContext) {
    let params = (|| -> Result<SceneParams, evani_core::params::ParamError> {
        Ok(SceneParams {
            display: ctx.param_enum(ParamDef::enumeration(DISPLAY, &Display::IDS, 0).describe("The studio viewport: render view (the asset in its style), material only (its own shape in its materials, no style) or wireframe (the drawn shape's edges)."))?,
            size: ctx.param_f32(ParamDef::f32(SIZE, 1.0).range(0.3, 3.0).step(0.05).unit("m").describe("An asset's radius on the studio stage."))?,
            gap: ctx.param_f32(ParamDef::f32(GAP, 0.45).range(0.0, 2.0).step(0.05).unit("m").describe("The gap between compared assets."))?,
            wire: ctx.param_f32(ParamDef::f32(WIRE, 0.0045).range(0.001, 0.02).step(0.0005).describe("The wireframe's line width, as a share of the asset's radius."))?,
        })
    })();
    match params {
        Ok(p) => ctx.resource(p),
        Err(e) => ctx.world.res_mut::<ErrorLog>().error(format!("material-view: studio scene: {e}")),
    }
    ctx.resource(StudioScene::default());
    ctx.resource(super::pick::StudioTap::default());
    ctx.resource(StudioSources::default());
    ctx.resource(StyledCache::default());
}

/// The draw system (installed after the other scenes, so the styles
/// scene's per-frame resets run first).
pub fn install_system(ctx: &mut PluginContext) {
    ctx.system(SystemDesc::new("material-view.scene.studio", Stage::Frame, run));
}

/// The ink shell's width on a studio asset, times the style's own. The
/// styles' widths were set on the sphere and the teapot; at full width the
/// shell swallows a horse's legs (WP5's finding).
pub const HULL: f32 = 0.3;

/// The surface stage as treat.rs takes it (`None`: the lit materials).
///
/// The screen pass (`pass.rs`, WP5) owns two things the style's block also
/// names, so the surface stage leaves them out: a pixel pass draws the
/// style's line itself, and the engraving's ink (`dither`) is laid in
/// screen space over the plain lit shape.
pub fn render_of(s: &Surface, pass: &Pass) -> Option<Render> {
    let pixel = matches!(pass, Pass::Pixel { .. });
    let r = Render {
        bands: s.bands,
        shadow: s.shadow,
        shadow_hue: s.shadow_hue,
        highlight: s.highlight,
        outline: if pixel { 0.0 } else { s.outline * HULL },
        outline_color: s.outline_color,
        outline_glow: s.outline_glow,
        posterize: s.posterize,
        dither: false,
        paper: s.paper,
        mottle: s.mottle,
        desaturate: s.desaturate,
        value: s.value,
        side_light: s.side_light,
        detail: s.detail,
        detail_step: s.detail_step,
        ..Render::default()
    };
    r.shaped().then_some(r)
}

/// Toward the eye of an asset's home view (yaw, pitch in degrees).
pub fn toward_eye(yaw: f32, pitch: f32) -> [f32; 3] {
    let (az, el) = (yaw.to_radians(), pitch.to_radians());
    [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]
}

/// The camera's right along the floor for a home view at `yaw` (compared
/// assets stand in a row across the view).
fn right_of(yaw: f32) -> [f32; 3] {
    let az = yaw.to_radians();
    [az.cos(), 0.0, -az.sin()]
}

pub(super) fn bounds_of(m: &StyledMesh) -> [[f32; 3]; 2] {
    let mut b = [[f32::MAX; 3], [f32::MIN; 3]];
    for p in m.parts.iter().flat_map(|p| &p.positions) {
        for k in 0..3 {
            b[0][k] = b[0][k].min(p[k]);
            b[1][k] = b[1][k].max(p[k]);
        }
    }
    if b[0][0] > b[1][0] { [[-1.0; 3], [1.0; 3]] } else { b }
}

/// The records to draw: comparison's 2 to 4, else the selection.
fn records(studio: &Studio) -> Vec<Selection> {
    let c = &studio.state.compare;
    if c.len() >= 2 { c.iter().take(MAX_VIEWS).cloned().collect() } else { vec![studio.state.selection.clone()] }
}

/// Resolve one record with its own style's controls and remembered settings.
fn resolved(c: &Catalogs, studio: &Studio, sel: &Selection) -> Result<Resolved, String> {
    if *sel == studio.state.selection
        && let Some(r) = &studio.current
        && r.selection == *sel
    {
        return Ok(r.clone());
    }
    let controls = studio.state.controls.get(&sel.style).cloned().unwrap_or_default();
    resolve(c, sel, &controls, studio.state.memory.get(&sel.key()))
}

/// Each region's look for a record. The selected region wears the open
/// catalogue row as edited when it is that material (the panel's edits).
pub(super) fn looks(r: &Resolved, asset: &AssetRow, lib: &LibraryData, open: Option<&CatalogueView>) -> (Vec<RegionLook>, bool) {
    let mut used_edit = false;
    let looks = asset
        .regions
        .iter()
        .map(|region| {
            let Some(b) = r.bindings.iter().find(|b| b.region == region.id) else { return scene_build::clay_look() };
            let Some(id) = &b.material else { return scene_build::kept_look() };
            let Some((i, row)) = lib.0.find(id) else { return scene_build::clay_look() };
            if !lib.0.openable(i) {
                return scene_build::clay_look();
            }
            let edited = open.filter(|v| b.from == BoundBy::Selected && v.open == Some(i)).and_then(|v| v.edited.clone());
            used_edit |= edited.is_some();
            let row = edited.unwrap_or_else(|| working_row(&lib.0.cat, row));
            let points = b.tone == Tone::Points && scene_build::points_here(&row, &region.id);
            let row = scene_build::region_row(&row, &region.id, b.tone == Tone::Points);
            scene_build::region_look(&lib.0.cat, &row, points).unwrap_or_else(|_| scene_build::clay_look())
        })
        .collect();
    (looks, used_edit)
}

fn run(world: &mut World) {
    let showing = world.try_res::<Viewer>().is_some_and(|v| v.scene == SCENE);
    if !showing && let Some(mut t) = world.try_res_mut::<super::pick::StudioTap>() {
        // A tap in another scene never picks a part later.
        t.0 = None;
    }
    if let Some(mut s) = world.try_res_mut::<Studio>()
        && s.active != showing
    {
        s.active = showing;
    }
    {
        let Some(mut st) = world.try_res_mut::<StudioScene>() else { return };
        if !showing {
            st.views.clear();
            st.shown = false;
            return;
        }
        if !st.shown {
            // Entering the studio: the viewer put its scene camera back, so
            // frame the asset again.
            st.shown = true;
            st.framed = None;
            drop(st);
            // The asset holds its own framing: the viewer's turntable (on in
            // the legacy scenes) stops; Spin turns it on again.
            if let Some(mut v) = world.try_res_mut::<Viewer>() {
                v.push(evani_kit_material_view::Command::Turntable(Some(false)));
            }
        }
    }
    let ground = material(world, "ground");
    world.res_mut::<RenderWorld>().draw_mesh(floor(ground, FLOOR_SAND));
    let Some(data) = world.try_res::<StudioData>().map(|d| d.0.clone()) else { return };
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.clone()) else { return };
    let Some(p) = world.try_res::<SceneParams>().map(|p| *p) else { return };
    let (display, size, gap, wire) = {
        let reg = world.res::<ParamRegistry>();
        (Display::from_index(reg.enum_index(p.display)), reg.f32(p.size), reg.f32(p.gap), reg.f32(p.wire))
    };
    let open = world.try_res::<CatalogueView>().map(|v| v.clone());
    let (sels, records, current_sel, part) = {
        let studio = world.res::<Studio>();
        let sels = records(&studio);
        let rs: Vec<Result<Resolved, String>> = sels.iter().map(|s| resolved(&data, &studio, s)).collect();
        (sels, rs, studio.state.selection.clone(), studio.state.part.clone())
    };
    // A newly selected part flashes for a moment (picked in the view or in
    // the parts list).
    let flash = {
        let mut st = world.res_mut::<StudioScene>();
        let now = part.clone().map(|p| (current_sel.asset.clone(), p));
        if now != st.last_part {
            st.flash_left = if now.is_some() { super::pick::FLASH_FRAMES } else { 0 };
            st.last_part = now;
        }
        st.flash_left = st.flash_left.saturating_sub(1);
        let asset = data.assets.asset(&current_sel.asset);
        match (asset, &part) {
            (Some(a), Some(p)) if st.flash_left > 0 => a.part(p).map_or(0, |p| a.part_regions(p).iter().fold(0u32, |m, r| m | (1 << r))),
            _ => 0,
        }
    };
    // WP2's loader has read the current record's asset (this frame or
    // before): keep it for this view and any comparison view.
    if let (Some(from), Some(mut sources)) = (world.try_res::<StudioSource>(), world.try_res_mut::<StudioSources>()) {
        sources.sync(&from, data.assets.asset(&from.asset));
    }
    let mut log = Vec::new();
    let mut views = Vec::with_capacity(sels.len());
    {
        let Some(mut models) = world.try_res_mut::<Models>() else { return };
        let Some(mut st) = world.try_res_mut::<StudioScene>() else { return };
        let Some(sources) = world.try_res::<StudioSources>() else { return };
        let Some(mut cache) = world.try_res_mut::<StyledCache>() else { return };
        let mut variants = world.try_res_mut::<evani_style_geo::cache::Variants>();
        crate::scenes::native_stream(world, &mut models);
        st.display = display;
        for (i, (sel, r)) in sels.iter().zip(&records).enumerate() {
            let Some(asset) = data.assets.asset(&sel.asset) else { continue };
            let style = data.styles.style(&sel.style);
            let names = [asset.name.clone(), style.map_or(sel.style.clone(), |s| s.name.clone()), data.materials.get(&sel.material).map_or(sel.material.clone(), |m| m.name.clone())];
            let mut view = View { selection: sel.clone(), names, state: ViewState::Loading, model: None, parts: 0, at: [0.0; 3], scale: size, bounds: [[-1.0; 3], [1.0; 3]] };
            let r = match r {
                Ok(r) => r,
                Err(e) => {
                    view.state = ViewState::Failed(e.clone());
                    views.push(view);
                    continue;
                }
            };
            if r.availability == Availability::NotBuilt {
                view.state = ViewState::NotBuilt;
                views.push(view);
                continue;
            }
            let src = match sources.get(asset) {
                Source::Ready(s) => s,
                Source::Loading => {
                    views.push(view);
                    continue;
                }
                Source::Failed(e) => {
                    view.state = ViewState::Failed(e);
                    views.push(view);
                    continue;
                }
            };
            // The shape: the style's own once it is made; meanwhile (and in
            // material-only mode) the same asset plain.
            let plain_key = geometry_key(&data, &asset.id, &Geometry::None, &BTreeMap::new());
            let wants_styled = display != Display::Material && r.geometry != Geometry::None;
            let styled = if wants_styled { Some(cache.get(r.geometry_key, &src, asset, &r.geometry, variants.as_deref_mut())) } else { None };
            let (mesh, is_styled) = match styled {
                Some(Styled::Ready(m)) => {
                    view.state = ViewState::Ready;
                    (Some(m), true)
                }
                Some(Styled::Making) => {
                    view.state = ViewState::Generating;
                    (None, false)
                }
                Some(Styled::NotBuilt(why)) => {
                    view.state = ViewState::StyleNotBuilt(why);
                    (None, false)
                }
                None => {
                    view.state = ViewState::Ready;
                    (None, false)
                }
            };
            let mesh = match mesh {
                Some(m) => m,
                None => match cache.get(plain_key, &src, asset, &Geometry::None, None) {
                    Styled::Ready(m) => m,
                    Styled::Making => {
                        view.state = ViewState::Loading;
                        views.push(view);
                        continue;
                    }
                    Styled::NotBuilt(e) => {
                        view.state = ViewState::Failed(e);
                        views.push(view);
                        continue;
                    }
                },
            };
            let (mut region_looks, used_edit) = looks(r, asset, &lib, open.as_ref());
            let flash = if *sel == current_sel { flash } else { 0 };
            for (k, look) in region_looks.iter_mut().enumerate() {
                if flash & (1 << k) != 0 {
                    scene_build::flash(look, super::pick::FLASH_GLOW);
                }
            }
            let key = BuildKey {
                variant: r.variant_key,
                styled: is_styled,
                display,
                edited: if used_edit { open.as_ref().map_or(0, |v| v.revision) } else { 0 },
                wire: if display == Display::Wireframe { wire.to_bits() } else { 0 },
                flash,
            };
            // One slot per view: made with its first model, then replaced
            // in place.
            if st.slots.len() <= i {
                st.slots.resize(i + 1, None);
            }
            if st.slots[i].as_ref().and_then(|s| s.key) != Some(key) && !st.logged.contains(&key) {
                let surface = if display == Display::Render { render_of(&r.surface, &r.pass) } else { None };
                let light = surface.as_ref().map_or(crate::treat::SUN, |s| scene_build::band_light(toward_eye(asset.frame.yaw, asset.frame.pitch), s));
                let built = scene_build::build(&Build { mesh: &mesh, looks: &region_looks, surface: surface.as_ref(), light, display, wire, uv: &scene_build::asset_uv(asset) });
                match built {
                    Ok((bytes, parts)) => {
                        let model = match &st.slots[i] {
                            Some(slot) => {
                                models.replace_glb(slot.model, &bytes);
                                slot.model
                            }
                            None => models.add_glb(&format!("studio/view{i}.glb"), &bytes),
                        };
                        st.slots[i] = Some(Slot { model, key: Some(key), parts, bounds: bounds_of(&mesh), mesh: mesh.clone() });
                        st.builds += 1;
                    }
                    Err(e) => {
                        st.logged.push(key);
                        log.push(format!("material-view: studio scene: {}: {e}", sel.key()));
                    }
                }
            }
            if let Some(slot) = st.slots[i].as_ref().filter(|s| s.key == Some(key)) {
                view.model = models.ready(slot.model).then_some(slot.model);
                view.parts = slot.parts;
                view.bounds = slot.bounds;
            }
            views.push(view);
        }
    }
    // Stand the views in a row across the asset's home view, on the floor.
    let yaw = data.assets.asset(&current_sel.asset).map_or(35.0, |a| a.frame.yaw);
    let right = right_of(yaw);
    let n = views.len();
    let step = 2.0 * size + gap;
    for (i, v) in views.iter_mut().enumerate() {
        let x = (i as f32 - (n as f32 - 1.0) * 0.5) * step;
        let lift = -v.bounds[0][1] * size;
        v.at = [right[0] * x, lift, right[2] * x];
        v.scale = size;
        v.bounds = [0, 1].map(|e| std::array::from_fn(|k| v.at[k] + v.bounds[e][k] * size));
    }
    frame(world, &data, &views, &current_sel, size, gap);
    pick_part(world, &data, &views);
    // The recipe stage's style detail for the current record's surface: the
    // material's own pattern simplified the style's way (banded, posterized,
    // inked), as the catalogue scene does; the styles scene cleared it
    // earlier this frame.
    if display == Display::Render
        && let Some(Ok(r)) = sels.iter().zip(&records).find(|(s, _)| **s == current_sel).map(|(_, r)| r)
        && let Some(surface) = render_of(&r.surface, &r.pass)
    {
        let live = world.try_res::<DetailParams>().zip(world.try_res::<ParamRegistry>()).and_then(|(d, reg)| d.get(&reg, &current_sel.style));
        let word = crate::scenes::catalogue::style_of(&surface, live.unwrap_or((surface.detail, surface.detail_step)), size).pack();
        if let Some(mut w) = world.try_res_mut::<RecipeStyle>() {
            w.0 = word;
        }
    }
    {
        let Some(mut models) = world.try_res_mut::<Models>() else { return };
        for v in &views {
            if let Some(m) = v.model {
                models.draw(ModelInstance::new(m, v.at).scale([v.scale; 3]));
            }
        }
    }
    {
        let mut rw = world.res_mut::<RenderWorld>();
        for v in views.iter().filter(|v| v.model.is_some()) {
            let r = ((v.bounds[1][0] - v.bounds[0][0]).max(v.bounds[1][2] - v.bounds[0][2]) * 0.5).max(0.2);
            crate::scenes::common::contact(&mut rw, [(v.bounds[0][0] + v.bounds[1][0]) * 0.5, 0.0, (v.bounds[0][2] + v.bounds[1][2]) * 0.5], r, v.bounds[1][1].max(0.0));
        }
    }
    world.res_mut::<StudioScene>().views = views;
    if !log.is_empty() {
        let mut errors = world.res_mut::<ErrorLog>();
        for e in log {
            errors.error(e);
        }
    }
}

/// A tap on the view this frame selects the part under it (the nearest
/// drawn triangle's region); a tap that meets nothing changes nothing.
fn pick_part(world: &mut World, data: &Catalogs, views: &[View]) {
    let Some(at) = world.try_res_mut::<super::pick::StudioTap>().and_then(|mut t| t.0.take()) else { return };
    let (cam, size) = {
        let rw = world.res::<RenderWorld>();
        (rw.camera, rw.viewport_css)
    };
    let aspect = if size[0] > 0.0 && size[1] > 0.0 { size[0] / size[1] } else { 16.0 / 9.0 };
    let (o, d) = super::pick::ray(&cam, aspect, at);
    let hit = {
        let st = world.res::<StudioScene>();
        views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let slot = st.slots.get(i)?.as_ref()?;
                super::pick::hit(&slot.mesh, v.at, v.scale, o, d).map(|(t, region)| (t, region, v.selection.clone()))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    };
    let Some((_, region, sel)) = hit else { return };
    let Some(asset) = data.assets.asset(&sel.asset) else { return };
    let Some(part) = asset.part_of_region(usize::from(region)).map(|k| asset.parts[k].id.clone()) else { return };
    world.res_mut::<StudioScene>().picks += 1;
    let mut studio = world.res_mut::<Studio>();
    if studio.state.selection.asset == sel.asset {
        studio.push(evani_kit_material_view::studio::StudioCommand::Pick(evani_kit_material_view::studio::evani_studio::Pick::Part(Some(part))));
    }
}

/// Frame the drawn set when it changed: the home view from the current
/// asset's `frame` and the set's box; the orbit follows unless the record
/// has a remembered camera (WP1's bridge put it back).
fn frame(world: &mut World, data: &Catalogs, views: &[View], current: &Selection, size: f32, gap: f32) {
    // The canvas and the open part of it the page's panels leave (CSS px);
    // with none measured (native, tests) the whole canvas is open.
    let view = world.res::<RenderWorld>().viewport_css;
    let canvas = if view[0] > 0.0 && view[1] > 0.0 { view } else { [1600.0, 900.0] };
    let open = world
        .try_res::<crate::scenes::styles::FreeArea>()
        .map(|a| *a)
        .filter(|a| a.view[0] > 0.0 && a.view[1] > 0.0)
        .map_or([0.0, 0.0, canvas[0], canvas[1]], |a| {
            let (sx, sy) = (canvas[0] / a.view[0], canvas[1] / a.view[1]);
            [a.rect[0] * sx, a.rect[1] * sy, a.rect[2] * sx, a.rect[3] * sy]
        });
    let key = FrameKey {
        assets: views.iter().map(|v| v.selection.asset.clone()).collect(),
        known: views.iter().map(|v| v.model.is_some()).collect(),
        size: size.to_bits(),
        gap: gap.to_bits(),
        open: open.map(|x| (x / 8.0).round() as i32),
    };
    if world.try_res::<StudioScene>().is_some_and(|s| s.framed.as_ref() == Some(&key)) || views.is_empty() {
        return;
    }
    let Some(asset) = data.assets.asset(&current.asset) else { return };
    let mut b = [[f32::MAX; 3], [f32::MIN; 3]];
    for v in views {
        b = [0, 1].map(|e| std::array::from_fn(|k| if e == 0 { b[0][k].min(v.bounds[0][k]) } else { b[1][k].max(v.bounds[1][k]) }));
    }
    let home = framed_in(b, asset.frame.yaw, asset.frame.pitch, asset.frame.fill, canvas, open);
    let remembered = views.len() == 1 && world.res::<Studio>().state.memory.get(&current.key()).is_some_and(|s| s.camera.is_some());
    // Only the open part changed (a panel, the window): the user's own
    // camera stays unless it was the old home view.
    let same_set = world.try_res::<StudioScene>().and_then(|s| s.framed.clone()).is_some_and(|f| FrameKey { open: key.open, ..f } == key);
    if let Some(mut v) = world.try_res_mut::<Viewer>() {
        let at_home = v.orbit == v.home;
        v.home = home;
        if !remembered && (!same_set || at_home) {
            v.orbit = home;
        }
        v.revision += 1;
    }
    let mut st = world.res_mut::<StudioScene>();
    st.framed = Some(key);
    st.framings += 1;
}

/// [`framed_orbit`] for the part of a `canvas` (CSS px) the page leaves
/// `open` (`[left, top, right, bottom]`), less the overlay's bottom bar
/// ([`BAR_BAND_PX`]): the box fills `fill` of that part, and the target
/// drops by half the band so the box draws in its middle (the page's fit
/// step centres the target in the whole open part).
pub fn framed_in(b: [[f32; 3]; 2], yaw: f32, pitch: f32, fill: f32, canvas: [f32; 2], open: [f32; 4]) -> evani_kit_material_view::orbit::Orbit {
    let ch = canvas[1].max(1.0);
    let fw = (open[2] - open[0]).max(1.0);
    let full = (open[3] - open[1]).max(1.0);
    let band = BAR_BAND_PX.min(full * 0.25);
    let fh = full - band;
    let mut o = framed_orbit(b, yaw, pitch, fill * fh / ch, fw / fh);
    let tan = (evani_kit_material_view::orbit::Orbit::default().pose(0.0).fov_y_deg.to_radians() * 0.5).tan();
    // Metres per CSS px at the target, then half the band, straight down.
    let m = 2.0 * o.dist * tan / ch;
    o.target[1] -= band * 0.5 * m / pitch.to_radians().cos().max(0.3);
    o
}

/// The orbit that frames box `b` from yaw and pitch (degrees) so the box
/// fills `fill` of the view's height (and of its width on a narrow view).
pub fn framed_orbit(b: [[f32; 3]; 2], yaw: f32, pitch: f32, fill: f32, aspect: f32) -> evani_kit_material_view::orbit::Orbit {
    let centre: [f32; 3] = std::array::from_fn(|k| (b[0][k] + b[1][k]) * 0.5);
    let half: [f32; 3] = std::array::from_fn(|k| (b[1][k] - b[0][k]) * 0.5);
    let fov = evani_kit_material_view::orbit::Orbit::default().pose(0.0).fov_y_deg;
    let tan = (fov.to_radians() * 0.5).tan();
    let fill = fill.clamp(0.2, 0.95);
    // The box's size seen from the home view: its height and the width it
    // shows across the view.
    let right = right_of(yaw);
    let across = (half[0] * right[0]).abs() + (half[2] * right[2]).abs();
    let deep = (half[0] * right[2]).abs() + (half[2] * right[0]).abs();
    let (el, sin_el) = (pitch.to_radians(), pitch.to_radians().sin().abs());
    let tall = half[1] * el.cos() + deep * sin_el;
    let dist = (tall / (fill * tan)).max(across / (fill * tan * aspect.max(0.2))) + deep;
    let dist = dist.max(0.5);
    evani_kit_material_view::orbit::Orbit { az: yaw, el: pitch, dist, target: centre, min_dist: dist * 0.2, max_dist: dist * 5.0, fov: evani_kit_material_view::orbit::FOV_Y_DEG }.clamped()
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_kit_material_view::Command;
    use evani_kit_material_view::studio::evani_studio::AssetStatus;
    use evani_kit_material_view::studio::evani_studio::geo::SourceMesh;
    use evani_kit_material_view::studio::{Pick, StudioCommand};

    const DT: f64 = 1000.0 / 60.0;

    /// A small closed source with one vertex group per region of `asset`
    /// (each region a box along x), at radius about 1: a test source, never
    /// shown by the app (the real one is WP2's).
    fn source(asset: &AssetRow) -> SourceMesh {
        let n = asset.regions.len();
        let mut m = SourceMesh::default();
        for r in 0..n {
            let x0 = -0.9 + 1.8 * r as f32 / n as f32;
            let w = 1.8 / n as f32;
            let base = m.positions.len() as u32;
            for c in 0..8u32 {
                let p = [x0 + if c & 1 == 1 { w } else { 0.0 }, if c & 2 == 2 { 0.5 } else { -0.5 }, if c & 4 == 4 { 0.3 } else { -0.3 }];
                m.positions.push(p);
                m.normals.push(crate::shapes::normalize(p, [0.0, 1.0, 0.0]));
                m.regions.push(r as u8);
            }
            for t in [[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]] {
                m.indices.extend(t.map(|i| base + i));
            }
        }
        m
    }

    /// The app in the studio scene on the horse, with a test source given
    /// (standing in for WP2's reader) and the horse's row marked ready in
    /// the app's copy of the catalogue (it is `building` until WP2 lands).
    fn studio_ready() -> evani_core::App {
        let mut app = crate::build_app();
        let mut c = (*app.world.res::<StudioData>().0).clone();
        for a in c.assets.assets.iter_mut().filter(|a| a.id == "animal.horse") {
            a.status = AssetStatus::Ready;
        }
        let row = c.assets.asset("animal.horse").unwrap().clone();
        app.world.insert_resource(StudioData(std::rc::Rc::new(c)));
        app.world.res_mut::<StudioSources>().insert("animal.horse", source(&row));
        app.world.res_mut::<Studio>().push(StudioCommand::Pick(Pick::Style("WK01".into())));
        app.world.res_mut::<Viewer>().push(Command::Scene(SCENE.into()));
        app.run_headless(4, DT);
        app
    }

    fn scene(app: &evani_core::App) -> std::cell::Ref<'_, StudioScene> {
        app.world.res::<StudioScene>()
    }

    fn push(app: &mut evani_core::App, c: StudioCommand) {
        app.world.res_mut::<Studio>().push(c);
        app.run_headless(3, DT);
    }

    /// An asset still being built draws nothing (no model, no stand-in) and
    /// says so; the studio is marked active while shown.
    #[test]
    fn an_asset_being_built_shows_its_state_and_no_other_object() {
        let mut app = crate::build_app();
        // The horse is built now; mark it building in the app's copy.
        let mut c = (*app.world.res::<StudioData>().0).clone();
        for a in c.assets.assets.iter_mut().filter(|a| a.id == "animal.horse") {
            a.status = AssetStatus::Building;
        }
        app.world.insert_resource(StudioData(std::rc::Rc::new(c)));
        app.world.res_mut::<Viewer>().push(Command::Scene(SCENE.into()));
        app.run_headless(4, DT);
        assert!(app.world.res::<Studio>().active);
        assert!(!app.world.res::<Viewer>().spin, "the asset holds its framing: no turntable");
        let st = scene(&app);
        assert_eq!(st.views.len(), 1);
        let v = &st.views[0];
        assert_eq!(v.selection.asset, "animal.horse");
        assert_eq!(v.model, None, "nothing in the horse's place");
        assert!(matches!(v.state, ViewState::NotBuilt | ViewState::Failed(_) | ViewState::Loading), "{:?}", v.state);
        assert!(v.status().is_some_and(|s| s.contains("horse")), "{:?}", v.status());
        drop(st);
        app.world.res_mut::<Viewer>().push(Command::Scene("styles".into()));
        app.run_headless(3, DT);
        assert!(!app.world.res::<Studio>().active, "leaving the studio clears it");
        assert!(scene(&app).views.is_empty());
    }

    /// Lighting and quality never rebuild; a style change rebuilds once;
    /// the drawn model's parts follow the bindings (a bay horse: coat,
    /// points, hoof, muzzle, eye); a material change keeps the geometry key.
    #[test]
    fn light_and_quality_never_rebuild_and_the_parts_follow_the_bindings() {
        let mut app = studio_ready();
        let data = app.world.res::<StudioData>().0.clone();
        {
            let st = scene(&app);
            let v = &st.views[0];
            assert!(v.model.is_some(), "{:?}", st.views);
            assert_eq!(v.state, ViewState::Ready);
            assert_eq!(v.title(), format!("{} + Horse coat, bay + Horse", data.styles.style("WK01").unwrap().name));
            // A toon surface: its bands per region, within the slot cap.
            assert!(v.parts > 8 && v.parts <= evani_kit_material_view::studio::evani_studio::MAX_SLOTS, "{}", v.parts);
        }
        // The fur's own pattern is simplified the toon way.
        assert_ne!(app.world.res::<RecipeStyle>().0, 0, "the style detail word is set");
        // A lit style (no surface treatment, no generated shape): one part
        // per binding: coat, legs, ears, mane, tail, hooves, muzzle, eyes.
        let lit = data.styles.styles.iter().find(|s| render_of(&s.surface, &s.pass).is_none() && s.geometry == Geometry::None).unwrap().id.clone();
        push(&mut app, StudioCommand::Pick(Pick::Style(lit)));
        let bindings = app.world.res::<Studio>().current.as_ref().unwrap().bindings.len();
        assert_eq!((scene(&app).views[0].parts, bindings), (8, 8));
        let before = scene(&app).builds;
        for l in ["daylight", "sunset", "night", "studio"] {
            push(&mut app, StudioCommand::Light(l.into()));
        }
        push(&mut app, StudioCommand::Quality("low".into()));
        assert_eq!(scene(&app).builds, before, "a light or quality change built a model");
        let g = app.world.res::<Studio>().current.as_ref().unwrap().geometry_key;
        // Brick on a horse works (a wall's material on a non-exclusive
        // class) once the horse was chosen on purpose; otherwise auto-match
        // shows brick on its own asset, the wall.
        push(&mut app, StudioCommand::Pick(Pick::Asset("animal.horse".into())));
        let other = data.materials.rows.iter().find(|m| m.ready() && m.recipe == "masonry").unwrap().id.clone();
        push(&mut app, StudioCommand::Pick(Pick::Material(other.clone())));
        let sel = app.world.res::<Studio>().state.selection.clone();
        assert_eq!((sel.asset.as_str(), sel.material), ("animal.horse", other));
        assert_eq!(scene(&app).views[0].state, ViewState::Ready);
        assert_eq!(app.world.res::<Studio>().current.as_ref().unwrap().geometry_key, g, "a material change keeps the shape");
        assert!(scene(&app).builds > before);
    }

    /// A tap on the drawn horse's middle selects the part under it (as the
    /// parts list would), the part flashes (one build lit, one back), and a tap on
    /// the empty floor changes nothing.
    #[test]
    fn a_tap_on_the_view_selects_the_part_under_it() {
        let mut app = studio_ready();
        let (cam, b) = (app.world.res::<RenderWorld>().camera, scene(&app).views[0].bounds);
        let mid = [0, 1, 2].map(|k| (b[0][k] + b[1][k]) * 0.5);
        // Where the box's middle is on the canvas (16:9, as the frame).
        let fwd = crate::studio::pick::tests_support::norm([cam.target[0] - cam.eye[0], cam.target[1] - cam.eye[1], cam.target[2] - cam.eye[2]]);
        let right = crate::studio::pick::tests_support::norm(crate::studio::pick::tests_support::cross(fwd, cam.up));
        let up = crate::studio::pick::tests_support::cross(right, fwd);
        let d = [mid[0] - cam.eye[0], mid[1] - cam.eye[1], mid[2] - cam.eye[2]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let t = (cam.fov_y_deg.to_radians() * 0.5).tan();
        let x = dot(d, right) / dot(d, fwd) / (t * 16.0 / 9.0);
        let y = dot(d, up) / dot(d, fwd) / t;
        let builds = scene(&app).builds;
        app.world.res_mut::<crate::studio::pick::StudioTap>().0 = Some([(x + 1.0) * 0.5, (1.0 - y) * 0.5]);
        app.run_headless(3, DT);
        let picked = app.world.res::<Studio>().state.part.clone();
        let horse = app.world.res::<StudioData>().0.assets.asset("animal.horse").unwrap().clone();
        assert!(picked.as_deref().is_some_and(|p| horse.part(p).is_some()), "{picked:?}, picks {}", scene(&app).picks);
        assert!(scene(&app).builds > builds, "the coat flashes");
        app.run_headless((crate::studio::pick::FLASH_FRAMES + 3) as _, DT);
        let after = scene(&app).builds;
        // The floor in a corner of the view: nothing changes.
        app.world.res_mut::<crate::studio::pick::StudioTap>().0 = Some([0.02, 0.98]);
        app.run_headless(3, DT);
        assert_eq!(app.world.res::<Studio>().state.part, picked);
        assert_eq!(scene(&app).builds, after);
    }

    /// The display modes rebuild the view; the wireframe adds its strips.
    #[test]
    fn display_modes_switch() {
        let mut app = studio_ready();
        let (parts, builds) = (scene(&app).views[0].parts, scene(&app).builds);
        let id = app.world.res::<ParamRegistry>().id(DISPLAY).unwrap();
        app.world.res_mut::<ParamRegistry>().set(id, evani_core::params::ParamValue::Enum(2)).unwrap();
        app.run_headless(3, DT);
        assert_eq!(scene(&app).display, Display::Wireframe);
        assert_eq!(scene(&app).views[0].parts, 2, "a dark body and its strips (was {parts})");
        assert_eq!(scene(&app).builds, builds + 1);
    }

    /// The frame puts the whole box in view at its fill, from the row's yaw
    /// and pitch.
    #[test]
    fn framing_fits_the_box() {
        let b = [[-1.0, 0.0, -0.4], [1.0, 1.6, 0.4]];
        let o = framed_orbit(b, 35.0, 12.0, 0.8, 16.0 / 9.0);
        assert_eq!((o.az, o.el), (35.0, 12.0));
        let cam = o.pose(0.0);
        let free = [0.0, 0.0, 1600.0, 900.0];
        assert!(crate::scenes::fit::box_inside(&cam, [1600.0, 900.0], free, b), "{o:?}");
        // Not absurdly far: the box fills a fair share of the view.
        let tall = framed_orbit(b, 35.0, 12.0, 0.8, 0.5);
        assert!(tall.dist > o.dist, "a narrow view pulls back");
        let cam = tall.pose(0.0);
        assert!(crate::scenes::fit::box_inside(&cam, [450.0, 900.0], [0.0, 0.0, 450.0, 900.0], b));
    }

    /// The bar guard (the 700 px proof shot put the hooves under the view
    /// buttons): at a half window, a laptop and a wide screen, the horse's
    /// box draws inside the open part above the overlay's bottom bar, once
    /// the page's fit step has centred it.
    #[test]
    fn framing_clears_the_view_bar() {
        let b = [[-0.35, 0.0, -1.1], [0.35, 1.7, 1.1]];
        for (canvas, open) in [([700.0, 820.0], [0.0, 147.0, 700.0, 640.0]), ([1280.0, 720.0], [294.0, 56.0, 936.0, 578.0]), ([2560.0, 1440.0], [294.0, 56.0, 2216.0, 1298.0])] {
            let o = framed_in(b, 35.0, 12.0, 0.8, canvas, open);
            let cam = crate::scenes::fit::fitted(&o.pose(0.0), canvas, open, None, o.dist);
            let above = [open[0], open[1], open[2], open[3] - BAR_BAND_PX];
            assert!(crate::scenes::fit::box_inside(&cam, canvas, above, b), "{canvas:?}: {o:?}");
        }
    }

    /// Progressive: while the style's shape is made the same asset is drawn
    /// plain and marked; when it arrives it is swapped in, one build more.
    #[test]
    fn the_styled_shape_swaps_in_when_it_is_made() {
        let mut app = studio_ready();
        push(&mut app, StudioCommand::Pick(Pick::Style("WK08".into())));
        let key = app.world.res::<Studio>().current.as_ref().unwrap().geometry_key;
        // WP3's queue at work.
        app.world.res_mut::<StyledCache>().insert(key, Styled::Making);
        app.run_headless(2, DT);
        let (builds, plain_parts) = {
            let st = scene(&app);
            assert_eq!(st.views[0].state, ViewState::Generating);
            assert!(st.views[0].model.is_some(), "the plain horse stays on screen");
            assert!(st.views[0].status().is_some_and(|s| s.contains("Making")));
            (st.builds, st.views[0].parts)
        };
        // The made shape: here the source's coat alone, so it differs.
        let mut made = scene_source_mesh(&app);
        made.parts.truncate(1);
        app.world.res_mut::<StyledCache>().insert(key, Styled::Ready(std::rc::Rc::new(made)));
        app.run_headless(2, DT);
        let st = scene(&app);
        assert_eq!(st.views[0].state, ViewState::Ready);
        assert_eq!(st.builds, builds + 1);
        assert!(st.views[0].parts < plain_parts);
    }

    fn scene_source_mesh(app: &evani_core::App) -> StyledMesh {
        let data = app.world.res::<StudioData>().0.clone();
        let row = data.assets.asset("animal.horse").unwrap().clone();
        use evani_kit_material_view::studio::evani_studio::geo::GeometryGenerator;
        evani_kit_material_view::studio::evani_studio::geo::Passthrough.generate(&source(&row), &row, &Geometry::None).unwrap()
    }

    /// The app as shipped: the real horse comes through WP2's loader (its
    /// studio tier), with no test source, and is drawn in all its regions.
    #[test]
    fn the_real_horse_comes_from_the_loader() {
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene(SCENE.into()));
        app.run_headless(8, DT);
        let row = app.world.res::<StudioData>().0.assets.asset("animal.horse").unwrap().clone();
        assert!(matches!(app.world.res::<super::super::loader::StudioSource>().state, super::super::loader::SourceState::Ready { .. }));
        let st = scene(&app);
        assert_eq!(st.views.len(), 1);
        assert_eq!(st.views[0].selection.asset, "animal.horse");
        assert!(st.views[0].model.is_some(), "{:?}", st.views[0].state);
        assert!(st.views[0].parts >= row.regions.len(), "{} parts", st.views[0].parts);
    }

    /// Progressive, end to end on the real horse: a voxel pick first draws
    /// the plain horse marked "Making..." while WP3's queue works, then the
    /// voxel horse, made once; going back to it later makes nothing again.
    #[test]
    fn a_voxel_horse_comes_from_the_shape_queue() {
        use evani_style_geo::cache::Variants;
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene(SCENE.into()));
        app.run_headless(8, DT);
        app.world.res_mut::<Studio>().push(StudioCommand::Pick(Pick::Style("WK08".into())));
        app.run_headless(1, DT);
        {
            let st = scene(&app);
            assert_eq!(st.views[0].state, ViewState::Generating, "the plain horse, marked, while the voxel one is made");
            assert!(st.views[0].model.is_some());
        }
        app.run_headless(3, DT);
        assert_eq!(scene(&app).views[0].state, ViewState::Ready);
        let made = app.world.res::<Variants>().made;
        assert_eq!(made, 1);
        push(&mut app, StudioCommand::Pick(Pick::Style("WK01".into())));
        push(&mut app, StudioCommand::Pick(Pick::Style("WK08".into())));
        assert_eq!(scene(&app).views[0].state, ViewState::Ready);
        assert_eq!(app.world.res::<Variants>().made, made, "kept, not made again");
    }

    /// Comparison draws each record side by side, in order, at most four.
    #[test]
    fn comparison_draws_records_side_by_side() {
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene(SCENE.into()));
        app.run_headless(3, DT);
        let base = app.world.res::<Studio>().state.selection.clone();
        let other = Selection { style: "WK08".into(), ..base.clone() };
        push(&mut app, StudioCommand::Compare(vec![base.clone(), other.clone()]));
        let st = scene(&app);
        assert_eq!(st.views.len(), 2);
        assert_eq!((st.views[0].selection.clone(), st.views[1].selection.clone()), (base, other));
        assert!(st.views[0].at != st.views[1].at, "side by side");
        assert!(st.framings >= 2, "the set is framed again");
    }

    /// The styles' surfaces: the plain PBR styles draw lit, the toon ones
    /// treated.
    #[test]
    fn surfaces_follow_the_style() {
        let data = evani_kit_material_view::studio::catalogs().unwrap();
        let shaped: Vec<&str> = data.styles.styles.iter().filter(|s| render_of(&s.surface, &s.pass).is_some()).map(|s| s.name.as_str()).collect();
        let plain: Vec<&str> = data.styles.styles.iter().filter(|s| render_of(&s.surface, &s.pass).is_none()).map(|s| s.name.as_str()).collect();
        assert!(shaped.iter().any(|n| n.contains("Toon")), "{shaped:?}");
        assert!(plain.iter().any(|n| n.contains("Realism") || n.contains("Painterly")), "{plain:?}");
    }

    /// What the screen pass draws, the surface stage leaves out: a pixel
    /// pass's line, and the engraving's ink. Every other line is the thin
    /// studio shell.
    #[test]
    fn the_pass_and_the_surface_never_both_draw() {
        let data = evani_kit_material_view::studio::catalogs().unwrap();
        let mut pixel = 0;
        for s in &data.styles.styles {
            let r = render_of(&s.surface, &s.pass);
            assert!(!r.as_ref().is_some_and(|r| r.dither), "{}: the pass lays the engraving's ink", s.name);
            if matches!(s.pass, Pass::Pixel { .. }) {
                pixel += 1;
                assert_eq!(r.as_ref().map_or(0.0, |r| r.outline), 0.0, "{}: the pixel pass draws the line", s.name);
            } else if s.surface.outline > 0.0 {
                let w = r.as_ref().map_or(0.0, |r| r.outline);
                assert!((w - s.surface.outline * HULL).abs() < 1e-6, "{}: the thin studio shell", s.name);
            }
        }
        assert!(pixel >= 2, "Pixel and Pixel HD");
        assert!(data.styles.styles.iter().any(|s| s.surface.dither), "the engraving style is in the catalogue");
    }
}
