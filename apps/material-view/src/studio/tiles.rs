//! The asset tiles: the Variations strip, the Presets panel and the current
//! combination's picture show the asset really drawn in each tile's
//! material and the style, by the engine, never its thumbnail recoloured.
//!
//! - **How.** A tile is a whole record: the variation's (the current record
//!   with that material picked, `StudioCommand::Variation`), a saved preset's
//!   (`StudioCommand::Open`) or the current one, resolved by the kit's own
//!   rules on a copy of the state. Its model is built the way the view
//!   builds its own ([`scene_build::build`]: the asset's source through the
//!   style's geometry stage, each region in its bound catalogue row, in the
//!   style's surface treatment) and drawn offscreen by evani-render's swatch
//!   pass (`evani_render::swatch`, the material balls' renderer) in the
//!   frame's light, sky and exposure, framed from the asset's own home view
//!   ([`swatch::frame_box`]) on the page's card colour ([`BACKGROUND`]).
//! - **Small models.** The source is the asset's shared low-tier model (the
//!   loader's studio tier: the lowest level of detail every device draws),
//!   the same mesh the view starts from; nothing is downloaded for a tile.
//! - **Lazily**, like the balls: the page lists the tiles on screen, most
//!   wanted first (the current combination, the variations left to right,
//!   the presets); [`POOL`] model slots take turns; one tile is built a
//!   frame at most, never on a frame a ball was built; the renderer draws at
//!   most two pictures a frame, each once its material pipelines are built.
//! - **Cached** by the record's variant key (asset, style shape, bindings,
//!   surface, edits), the light (the balls' settled light key), the quality
//!   tier and the picture size. Until a tile's picture is in, the page shows
//!   the recoloured thumbnail under it, never an empty box.

use std::collections::HashMap;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::plugin::PluginContext;
use evani_core::tiers::Tiers;
use evani_kit_material_view::Viewer;
use evani_kit_material_view::catalogue::{CatalogueView, LibraryData};
use evani_kit_material_view::studio::evani_studio::resolve::Availability;
use evani_kit_material_view::studio::{Catalogs, Resolved, Studio, StudioCommand, StudioData, apply};
use evani_render::model::{ModelId, ModelInstance, Models};
use evani_render::swatch::{self, SwatchRequest, Swatches};

use super::balls::{Balls, Picture, fnv};
use super::scene::{self, SCENE, render_of, toward_eye};
use super::scene_build::{self, Build, Display};
use super::scene_source::{Source, Styled, StudioSources, StyledCache};

/// Model slots the tiles take turns in (each holds one low-tier asset).
pub const POOL: usize = 2;
/// Pictures kept (least recently shown dropped first): about 4 MB at 128 px.
pub const CACHE: usize = 64;
/// Frames a tile may wait for its picture before its slot is freed.
pub const TIMEOUT: u64 = 600;
/// Frames tile builds wait after the style, light or tier changes, so they
/// never land on the frame the view rebuilds its own model.
pub const SETTLE: u64 = 8;
/// The asset's outline's share of its picture (its widest corner).
pub const FILL: f32 = 0.86;
/// The colour behind the asset (linear, before the tonemap): close to the
/// page's tile card (`.pic.img`'s grey gradient), so a drawn tile sits on the
/// card as the thumbnail did.
pub const BACKGROUND: [f32; 3] = [0.042, 0.04, 0.047];

/// Which record a tile shows (the page's `data-tile`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the page (wasm) lists the tiles on screen"))]
pub enum Tile {
    /// The current record.
    Current,
    /// The current record with this material picked (a variation tile, a
    /// suggested preset).
    Variation(String),
    /// A saved preset, by name.
    Saved(String),
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the page (wasm) reads and writes the ids"))]
impl Tile {
    /// The page's `data-tile` value.
    #[cfg(test)] pub fn id(&self) -> String {
        match self {
            Self::Current => "c".into(),
            Self::Variation(m) => format!("v:{m}"),
            Self::Saved(n) => format!("s:{n}"),
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id.split_once(':') {
            None if id == "c" => Some(Self::Current),
            Some(("v", m)) if !m.is_empty() => Some(Self::Variation(m.into())),
            Some(("s", n)) if !n.is_empty() => Some(Self::Saved(n.into())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Job {
    key: u64,
    since: u64,
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    model: ModelId,
    job: Option<Job>,
}

/// The tiles' state (a world resource).
#[derive(Debug, Default)]
pub struct Tiles {
    slots: Vec<Slot>,
    cache: Vec<Picture>,
    failed: Vec<u64>,
    /// The tiles the page shows this frame, most wanted first.
    pub wanted: Vec<Tile>,
    /// Tile models built so far.
    pub builds: u64,
    /// Pictures received so far.
    pub pictures: u64,
    frame: u64,
    /// Each tile's record, resolved at the studio state's revision.
    records: HashMap<Tile, Option<Resolved>>,
    revision: Option<u64>,
    /// The context last seen and the frame it changed.
    seen: (u64, u64),
    /// The balls' build count seen last frame (a ball built this frame
    /// takes the frame's one build).
    balls_seen: u64,
}

impl Tiles {
    /// The picture for `key`, marked as just shown.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the page's canvases (wasm) read the pictures"))]
    pub fn picture(&mut self, key: u64) -> Option<&Picture> {
        let i = self.cache.iter().position(|p| p.key == key)?;
        let p = self.cache.remove(i);
        self.cache.push(p);
        self.cache.last()
    }

    pub fn has(&self, key: u64) -> bool {
        self.cache.iter().any(|p| p.key == key)
    }

    /// A tile whose picture can't be made (it keeps its thumbnail).
    pub fn failed(&self, key: u64) -> bool {
        self.failed.contains(&key)
    }

    fn busy(&self, key: u64) -> bool {
        self.slots.iter().any(|s| s.job.is_some_and(|j| j.key == key))
    }

    fn insert(&mut self, p: Picture) {
        self.cache.retain(|c| c.key != p.key);
        self.cache.push(p);
        if self.cache.len() > CACHE {
            self.cache.remove(0);
        }
    }

    /// `tile`'s record (resolved once per studio state revision).
    fn record(&mut self, c: &Catalogs, studio: &Studio, tile: &Tile) -> Option<Resolved> {
        if self.revision != Some(studio.state.revision) {
            self.revision = Some(studio.state.revision);
            self.records.clear();
        }
        self.records.entry(tile.clone()).or_insert_with(|| record_of(c, studio, tile)).clone()
    }
}

/// `tile`'s record: the kit's own command on a copy of the state, resolved
/// (`None`: it can't be shown).
pub fn record_of(c: &Catalogs, studio: &Studio, tile: &Tile) -> Option<Resolved> {
    let mut state = studio.state.clone();
    match tile {
        Tile::Current => return studio.current.clone().or_else(|| state.resolve(c).ok()),
        Tile::Variation(m) => apply(&mut state, c, StudioCommand::Variation(m.clone())).ok()?,
        Tile::Saved(n) => apply(&mut state, c, StudioCommand::Open(n.clone())).ok()?,
    }
    state.resolve(c).ok().filter(|r| r.availability != Availability::NotBuilt)
}

/// What a picture depends on besides its record.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub light: u64,
    pub tier: String,
    pub size: u32,
    /// The open catalogue row's id and edit revision (a tile wearing the row
    /// being edited follows the edit).
    pub open: (Option<String>, u64),
}

/// The picture key of record `r` in `ctx`.
pub fn key(ctx: &Context, r: &Resolved) -> u64 {
    let edit = match &ctx.open.0 {
        Some(m) if r.bindings.iter().any(|b| b.material.as_deref() == Some(m.as_str())) => ctx.open.1,
        _ => 0,
    };
    fnv(&format!("tile|{:?}|{}|{}|{}|{edit}", r.variant_key, ctx.light, ctx.tier, ctx.size))
}

fn context(world: &World, lib: &LibraryData) -> Option<Context> {
    let b = world.try_res::<Balls>()?;
    let tier = world.try_res::<Tiers>().map_or_else(String::new, |t| t.active().id.clone());
    let open = world.try_res::<CatalogueView>().map_or((None, 0), |v| match (v.open, v.edited.is_some()) {
        (Some(i), true) => (lib.0.cat.rows.get(i).map(|r| r.id.clone()), v.revision),
        _ => (None, 0),
    });
    Some(Context { light: b.light(), tier, size: b.size, open })
}

/// The tile model of record `r`: the asset's shape (`mesh`) in its bound
/// materials and the style's surface, as the view builds it. Returns the
/// `.glb` and the shape's box (model units).
pub fn tile_glb(data: &Catalogs, r: &Resolved, mesh: &evani_kit_material_view::studio::evani_studio::geo::StyledMesh, lib: &LibraryData, open: Option<&CatalogueView>) -> Result<(Vec<u8>, [[f32; 3]; 2]), String> {
    let asset = data.assets.asset(&r.selection.asset).ok_or_else(|| format!("{}: no such asset", r.selection.asset))?;
    let (looks, _) = scene::looks(r, asset, lib, open);
    let surface = render_of(&r.surface, &r.pass);
    let light = surface.as_ref().map_or(crate::treat::SUN, |s| scene_build::band_light(toward_eye(asset.frame.yaw, asset.frame.pitch), s));
    let (bytes, _) = scene_build::build(&Build { mesh, looks: &looks, surface: surface.as_ref(), light, display: Display::Render, wire: 0.0, uv: scene_build::STUDIO_UV })?;
    Ok((bytes, scene::bounds_of(mesh)))
}

/// The shape of record `r` now: the style's own once made (asked of the
/// shape queue meanwhile), else `None` (the tile waits; its thumbnail stays).
fn shape(world: &World, data: &Catalogs, r: &Resolved) -> Result<Option<std::rc::Rc<evani_kit_material_view::studio::evani_studio::geo::StyledMesh>>, String> {
    let asset = data.assets.asset(&r.selection.asset).ok_or("no such asset")?;
    let src = match world.try_res::<StudioSources>().map(|s| s.get(asset)) {
        Some(Source::Ready(s)) => s,
        Some(Source::Failed(e)) => return Err(e),
        _ => return Ok(None),
    };
    let Some(mut cache) = world.try_res_mut::<StyledCache>() else { return Ok(None) };
    let mut variants = world.try_res_mut::<evani_style_geo::cache::Variants>();
    match cache.get(r.geometry_key, &src, asset, &r.geometry, variants.as_deref_mut()) {
        Styled::Ready(m) => Ok(Some(m)),
        Styled::Making => Ok(None),
        Styled::NotBuilt(e) => Err(e),
    }
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(Tiles::default());
}

/// After the balls' system (it takes their pictures first).
pub fn install_system(ctx: &mut PluginContext) {
    ctx.system(SystemDesc::new("material-view.studio.tiles", Stage::Frame, run));
}

fn run(world: &mut World) {
    if !world.try_res::<Viewer>().is_some_and(|v| v.scene == SCENE) {
        return;
    }
    // The balls took theirs: what is left is the tiles' (and any orphan of a
    // freed slot, dropped here).
    let (done, failed) = match world.try_res_mut::<Swatches>() {
        Some(mut s) => (s.take_done(), s.take_failed()),
        None => return,
    };
    let Some(data) = world.try_res::<StudioData>().map(|d| d.0.clone()) else { return };
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.clone()) else { return };
    let Some(ctx) = context(world, &lib) else { return };
    let balls_built = world.try_res::<Balls>().map_or(0, |b| b.builds);
    {
        let Some(mut t) = world.try_res_mut::<Tiles>() else { return };
        t.frame += 1;
        let frame = t.frame;
        for img in done {
            if !t.busy(img.key) {
                continue;
            }
            for s in t.slots.iter_mut().filter(|s| s.job.is_some_and(|j| j.key == img.key)) {
                s.job = None;
            }
            t.pictures += 1;
            t.insert(Picture { key: img.key, size: img.size, rgba: img.rgba });
        }
        for (k, _) in failed {
            if t.busy(k) {
                t.failed.push(k);
            }
            for s in t.slots.iter_mut().filter(|s| s.job.is_some_and(|j| j.key == k)) {
                s.job = None;
            }
        }
        for s in &mut t.slots {
            if s.job.is_some_and(|j| frame - j.since > TIMEOUT) {
                s.job = None;
            }
        }
    }
    // The page: paint the pictures that are in, list the tiles on screen
    // still waiting for theirs.
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = dom::paint(world, &data, &ctx) {
        world.res_mut::<Tiles>().wanted = w;
    }
    // One new tile a frame, into a free slot, once the context holds still
    // and no ball was built this frame.
    let next = {
        let studio = world.res::<Studio>();
        let mut t = world.res_mut::<Tiles>();
        let ball_frame = balls_built != t.balls_seen;
        t.balls_seen = balls_built;
        let now = fnv(&format!("{}|{}|{}|{}", studio.state.revision, ctx.light, ctx.tier, ctx.size));
        if t.seen.0 != now {
            t.seen = (now, t.frame);
        }
        let settled = t.frame.saturating_sub(t.seen.1) >= SETTLE || t.builds == 0;
        let free = t.slots.iter().position(|s| s.job.is_none()).or((t.slots.len() < POOL).then_some(t.slots.len()));
        let mut out = Vec::new();
        if let Some(f) = free.filter(|_| settled && !ball_frame) {
            let wanted = t.wanted.clone();
            for tile in wanted {
                let Some(r) = t.record(&data, &studio, &tile) else { continue };
                let k = key(&ctx, &r);
                if !t.has(k) && !t.failed(k) && !t.busy(k) && !out.iter().any(|(_, _, kk): &(usize, Resolved, u64)| *kk == k) {
                    out.push((f, r, k));
                }
            }
        }
        out
    };
    // The first wanted tile whose shape is ready (the others wait).
    let open = world.try_res::<CatalogueView>().map(|v| v.clone());
    for (slot, r, k) in next {
        let mesh = match shape(world, &data, &r) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(_) => {
                world.res_mut::<Tiles>().failed.push(k);
                continue;
            }
        };
        let asset = data.assets.asset(&r.selection.asset).map(|a| a.frame);
        match (tile_glb(&data, &r, &mesh, &lib, open.as_ref()), asset) {
            (Ok((bytes, bounds)), Some(frame)) => {
                let old = world.res::<Tiles>().slots.get(slot).map(|s| s.model);
                let model = {
                    let mut models = world.res_mut::<Models>();
                    match old {
                        Some(m) => {
                            models.replace_glb(m, &bytes);
                            m
                        }
                        None => models.add_glb(&format!("studio/tile{slot}.glb"), &bytes),
                    }
                };
                {
                    let mut s = world.res_mut::<Swatches>();
                    s.add_model(model);
                    s.cancel(model);
                    let uploads = s.uploads(model) + 1;
                    let camera = swatch::frame_box(bounds, frame.yaw, frame.pitch, FILL);
                    s.request(SwatchRequest { key: k, model, uploads, camera, size: ctx.size, background: Some(BACKGROUND) });
                }
                let mut t = world.res_mut::<Tiles>();
                let job = Some(Job { key: k, since: t.frame });
                if slot < t.slots.len() {
                    t.slots[slot].job = job;
                } else {
                    t.slots.push(Slot { model, job });
                }
                t.builds += 1;
            }
            _ => world.res_mut::<Tiles>().failed.push(k),
        }
        break;
    }
    // Every tile waiting for its picture is drawn (so the renderer has it
    // staged); the swatch pass keeps it out of the view.
    let busy: Vec<ModelId> = world.res::<Tiles>().slots.iter().filter(|s| s.job.is_some()).map(|s| s.model).collect();
    let mut models = world.res_mut::<Models>();
    for m in busy {
        models.draw(ModelInstance::new(m, swatch::AT));
    }
}

/// The page side (browser only): every `<canvas data-tile="...">`
/// (page_view.rs writes them over the tile's thumbnail) is painted with its
/// picture once it is in (`data-k`: the picture's key; CSS shows the canvas
/// only then); the ones on screen without one are the wanted list.
#[cfg(target_arch = "wasm32")]
mod dom {
    use evani_core::ecs::World;
    use evani_kit_material_view::studio::{Catalogs, Studio};
    use wasm_bindgen::{Clamped, JsCast};
    use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

    use super::{Context, Picture, Tile, Tiles, key};

    fn put(c: &HtmlCanvasElement, p: &Picture) -> bool {
        if c.width() != p.size || c.height() != p.size {
            c.set_width(p.size);
            c.set_height(p.size);
        }
        let Some(g) = c.get_context("2d").ok().flatten().and_then(|g| g.dyn_into::<CanvasRenderingContext2d>().ok()) else { return false };
        let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&p.rgba), p.size, p.size) else { return false };
        g.put_image_data(&img, 0, 0).is_ok()
    }

    /// Paint what is in; return the tiles on screen still waiting, most
    /// wanted first (`None`: no page).
    pub fn paint(world: &World, data: &Catalogs, ctx: &Context) -> Option<Vec<Tile>> {
        let win = web_sys::window()?;
        let doc = win.document()?;
        let list = doc.query_selector_all("canvas[data-tile]").ok()?;
        let num = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| v.ok().and_then(|x| x.as_f64()).unwrap_or(0.0);
        let (vw, vh) = (num(win.inner_width()), num(win.inner_height()));
        let studio = world.res::<Studio>();
        let mut tiles = world.res_mut::<Tiles>();
        let mut wanted: Vec<(u8, f64, Tile)> = Vec::new();
        for i in 0..list.length() {
            let Some(c) = list.item(i).and_then(|n| n.dyn_into::<HtmlCanvasElement>().ok()) else { continue };
            let Some(tile) = c.get_attribute("data-tile").and_then(|t| Tile::parse(&t)) else { continue };
            let Some(r) = tiles.record(data, &studio, &tile) else {
                let _ = c.set_attribute("data-nb", "1");
                continue;
            };
            let k = key(ctx, &r);
            let ks = format!("{k:x}");
            if c.get_attribute("data-k").as_deref() == Some(ks.as_str()) {
                continue;
            }
            if let Some(p) = tiles.picture(k) {
                if put(&c, p) {
                    let _ = c.set_attribute("data-k", &ks);
                    let _ = c.remove_attribute("data-nb");
                }
                continue;
            }
            if tiles.failed(k) {
                // A record that can't be drawn keeps its thumbnail, marked so
                // a check can tell it from a tile still on its way.
                let _ = c.set_attribute("data-nb", "1");
                continue;
            }
            let r = c.get_bounding_client_rect();
            if r.width() <= 0.0 || r.right() < 0.0 || r.left() > vw || r.bottom() < 0.0 || r.top() > vh {
                continue;
            }
            // A variation scrolled out of its strip is not on screen.
            if let Some(row) = c.closest(".st-vars-row").ok().flatten() {
                let pr = row.get_bounding_client_rect();
                if r.right() < pr.left() || r.left() > pr.right() {
                    continue;
                }
            }
            let pri = match tile {
                Tile::Current => 0,
                Tile::Variation(_) if c.closest(".st-vars-row").ok().flatten().is_some() => 1,
                _ => 2,
            };
            wanted.push((pri, r.left() + r.top() * 4.0, tile));
        }
        wanted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut out: Vec<Tile> = Vec::with_capacity(wanted.len());
        for (_, _, t) in wanted {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_render::model::recipes::RECIPE_KEY;
    use evani_render::swatch::SwatchImage;

    const DT: f64 = 1000.0 / 60.0;

    fn json(glb: &[u8]) -> serde_json::Value {
        assert!(evani_gltf::Gltf::from_glb(glb).is_ok(), "the engine reads it");
        let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&glb[20..20 + len]).unwrap()
    }

    fn studio_app() -> evani_core::App {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        app.world.res_mut::<Viewer>().scene = SCENE.into();
        // The horse streams in over a few frames natively too.
        for _ in 0..120 {
            app.run_headless(1, DT);
            let data = app.world.res::<StudioData>().0.clone();
            let ready = data.assets.asset("animal.horse").is_some_and(|a| matches!(app.world.res::<StudioSources>().get(a), Source::Ready(_)));
            if ready {
                break;
            }
        }
        app
    }

    /// Page ids round-trip.
    #[test]
    fn tile_ids_round_trip() {
        for t in [Tile::Current, Tile::Variation("brick.red.common".into()), Tile::Saved("Toon Horse · Horse coat: bay".into())] {
            assert_eq!(Tile::parse(&t.id()), Some(t.clone()));
        }
        assert_eq!(Tile::parse("x:1"), None);
        assert_eq!(Tile::parse("v:"), None);
    }

    /// A variation tile is the asset itself (its own shape, not a ball) in
    /// that material: its model wears the material's recipe block, the
    /// current tile and the current material's variation share one key,
    /// and keys follow the record, the light, the tier and the size.
    #[test]
    fn a_tile_is_the_asset_in_its_material() {
        let app = studio_app();
        let data = app.world.res::<StudioData>().0.clone();
        let lib = app.world.res::<LibraryData>().clone();
        let studio = app.world.res::<Studio>().clone();
        let cur = record_of(&data, &studio, &Tile::Current).unwrap();
        let mat = cur.selection.material.clone();
        let same = record_of(&data, &studio, &Tile::Variation(mat.clone())).unwrap();
        let ctx = Context { light: 1, tier: "high".into(), size: 128, open: (None, 0) };
        assert_eq!(key(&ctx, &cur), key(&ctx, &same), "the current tile is its own variation");
        let other = evani_kit_material_view::studio::variations(&data, &cur.selection, 12, 3).into_iter().find(|t| t.why.is_none() && !t.current).map(|t| t.material).unwrap();
        let var = record_of(&data, &studio, &Tile::Variation(other.clone())).unwrap();
        assert_eq!((var.selection.asset.as_str(), var.selection.style.as_str()), (cur.selection.asset.as_str(), cur.selection.style.as_str()));
        assert_ne!(key(&ctx, &var), key(&ctx, &cur));
        for c in [Context { light: 2, ..ctx.clone() }, Context { tier: "low".into(), ..ctx.clone() }, Context { size: 96, ..ctx.clone() }] {
            assert_ne!(key(&c, &cur), key(&ctx, &cur), "{c:?}");
        }
        let mesh = match shape(&app.world, &data, &var).unwrap() {
            Some(m) => m,
            None => panic!("the horse's shape is ready"),
        };
        let (glb, bounds) = tile_glb(&data, &var, &mesh, &lib, None).unwrap();
        let j = json(&glb);
        let verts: usize = mesh.parts.iter().map(|p| p.positions.len()).sum();
        let ball = crate::shapes::ball().positions.len();
        assert!(verts > 0 && verts != ball, "a tile is the asset's own shape");
        assert!(j["materials"].as_array().unwrap().iter().any(|m| m["extras"].get(RECIPE_KEY).is_some()), "the material's recipe rides on the tile");
        assert!(bounds[1][1] > bounds[0][1]);
    }

    /// The guard against thumbnails coming back: once the swatch pass is
    /// there, the page's wanted tiles are built (one a frame, at most POOL
    /// slots, never on a ball's build frame), asked of the swatch pass with
    /// the asset framed on the card colour, kept out of the view; and a
    /// finished picture lands in the cache for the page to paint over the
    /// thumbnail.
    #[test]
    fn wanted_tiles_are_drawn_by_the_engine_and_cached() {
        let mut app = studio_app();
        let data = app.world.res::<StudioData>().0.clone();
        let sel = app.world.res::<Studio>().state.selection.clone();
        let mats: Vec<String> = evani_kit_material_view::studio::variations(&data, &sel, 12, 3).into_iter().filter(|t| t.why.is_none()).take(4).map(|t| t.material).collect();
        let mut wanted = vec![Tile::Current];
        wanted.extend(mats.iter().cloned().map(Tile::Variation));
        app.world.res_mut::<Tiles>().wanted = wanted;
        for _ in 0..(SETTLE as u32 + 30) {
            app.run_headless(1, DT);
            if app.world.res::<Tiles>().slots.len() == POOL {
                break;
            }
        }
        let (slots, builds): (Vec<Slot>, u64) = {
            let t = app.world.res::<Tiles>();
            (t.slots.clone(), t.builds)
        };
        assert_eq!((slots.len(), builds), (POOL, POOL as u64), "POOL tiles built, one a frame");
        let jobs: Vec<u64> = {
            let s = app.world.res::<Swatches>();
            for slot in &slots {
                assert!(s.is_swatch(slot.model.0 as usize), "a tile model never draws in the view");
            }
            slots.iter().filter_map(|s| s.job.map(|j| j.key)).collect()
        };
        assert_eq!(jobs.len(), POOL);
        // The renderer hands the pictures back (no GPU here: by hand).
        for &k in &jobs {
            app.world.res_mut::<Swatches>().complete(SwatchImage { key: k, size: 8, rgba: vec![7; 256] });
        }
        app.run_headless(1, DT);
        let t = app.world.res::<Tiles>();
        assert!(jobs.iter().all(|k| t.has(*k)), "drawn tiles are kept for the page");
        assert_eq!(t.pictures, POOL as u64);
    }
}
