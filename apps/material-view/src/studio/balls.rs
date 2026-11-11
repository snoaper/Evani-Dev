//! The material balls: every ball the page shows (the inspector's, the
//! combination card's, the materials bar's chips) is a real picture of the
//! material drawn by the engine, never a tinted sphere.
//!
//! - **How.** A ball is the material's catalogue row on a small sphere, built
//!   the way the studio builds its asset ([`scene_build::build`]: the row's
//!   own glTF material, so the recipe stage draws its pattern, colour
//!   variation and sheen; in the open style's surface treatment) and drawn
//!   offscreen by evani-render's swatch pass (`evani_render::swatch`) in the
//!   frame's own light, sky and exposure. Nothing is downloaded: the
//!   pictures are made on the device, from the same 694 rows the view draws.
//! - **Lazily.** The page tells [`Balls::wanted`] which balls are on screen,
//!   most wanted first (the inspector, the card, then the chips left to
//!   right). Three model slots take turns ([`POOL`]): one ball is built a
//!   frame at most, and the renderer draws at most two a frame, each only
//!   once its material pipeline is built (so a ball never shows the plain
//!   stand-in).
//! - **Cached** by material, style (and its surface), light (the preset and
//!   the light's numbers, once they hold still) and quality tier, at the
//!   page's picture size ([`Balls::size`]: 96 px, 128 px from about 1.8
//!   device pixels per CSS pixel). A new style, light or tier makes new
//!   pictures; the old ones stay up until each new one is in.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::ParamRegistry;
use evani_core::plugin::PluginContext;
use evani_core::tiers::Tiers;
use evani_kit_material_view::Viewer;
use evani_kit_material_view::catalogue::{CatalogueView, LibraryData, working_row};
use evani_kit_material_view::studio::Studio;
use evani_kit_material_view::studio::evani_studio::geo::{StyledMesh, StyledPart};
use evani_kit_material_view::styles::Render;
use evani_render::model::{ModelId, ModelInstance, Models};
use evani_render::swatch::{self, SwatchRequest, Swatches};

use super::scene::{SCENE, render_of};
use super::scene_build::{self, Build, Display};

/// The ball's radius (m): a 70 cm sample, so a pattern laid by position
/// (brick courses, tiles) shows a few readable repeats at 96-128 px (about
/// three bricks across, nine courses) and one laid round the ball keeps its
/// catalogue size.
pub const RADIUS: f32 = 0.35;
/// Model slots the balls take turns in.
pub const POOL: usize = 3;
/// Pictures kept (least recently shown dropped first): about 6 MB at 128 px.
pub const CACHE: usize = 96;
/// Frames a ball may wait for its picture before its slot is freed.
pub const TIMEOUT: u64 = 600;
/// Frames the light's numbers must hold still before balls follow them (a
/// dragged sun dial makes one new set, not one a frame).
pub const LIGHT_SETTLE: u32 = 20;
/// The ball seen from the front, a little from the right and above (used
/// when the light comes from straight above).
pub const TOWARD: [f32; 3] = [0.35, 0.42, 1.0];
/// How far round from the light the ball is seen (degrees): a material
/// ball's three-quarter light, a lit side and a shadow side, so pattern and
/// relief read and a light facing the camera never washes the ball out.
pub const AROUND_DEG: f32 = 60.0;
/// The ball's view height (degrees above its equator).
pub const VIEW_UP_DEG: f32 = 20.0;

/// The direction from the ball to its camera for a light coming from
/// `light` (toward the light): [`AROUND_DEG`] round from it, a little above.
pub fn view_from(light: [f32; 3]) -> [f32; 3] {
    if light[0].hypot(light[2]) < 1e-3 {
        return TOWARD;
    }
    let az = light[0].atan2(light[2]) + AROUND_DEG.to_radians();
    let up = VIEW_UP_DEG.to_radians();
    [up.cos() * az.sin(), up.sin(), up.cos() * az.cos()]
}

/// One finished picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub key: u64,
    pub size: u32,
    /// `size * size * 4` bytes, RGBA8 sRGB, top row first.
    pub rgba: Vec<u8>,
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

/// The balls' state (a world resource).
#[derive(Debug)]
pub struct Balls {
    slots: Vec<Slot>,
    cache: Vec<Picture>,
    failed: Vec<u64>,
    /// The materials the page shows this frame, most wanted first.
    pub wanted: Vec<String>,
    /// Picture size (px, square).
    pub size: u32,
    /// Ball models built so far.
    pub builds: u64,
    /// Pictures received so far.
    pub pictures: u64,
    frame: u64,
    /// The light key in use, the one seen last and for how many frames.
    light: (u64, u64, u32),
    /// The view direction that goes with the light in use.
    toward: [f32; 3],
    /// The context last seen (style, light, tier, size) and the frame it
    /// changed: builds wait [`SETTLE`] frames after a change.
    seen: (u64, u64),
}

/// Frames ball builds wait after the style, light or tier changes, so they
/// never land on the frame the view rebuilds its own model.
pub const SETTLE: u64 = 8;

impl Default for Balls {
    fn default() -> Self {
        Self { slots: Vec::new(), cache: Vec::new(), failed: Vec::new(), wanted: Vec::new(), size: 96, builds: 0, pictures: 0, frame: 0, light: (0, 0, 0), toward: TOWARD, seen: (0, 0) }
    }
}

impl Balls {
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

    /// The light key the pictures use (the frame's, once it held still for
    /// [`LIGHT_SETTLE`] frames); the asset tiles follow the same one.
    pub fn light(&self) -> u64 {
        self.light.0
    }

    /// A ball whose picture can't be made (its row isn't ready to draw).
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
}

/// What a picture depends on besides its material.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub style: String,
    /// The style's surface treatment (`None`: the lit materials).
    pub surface: Option<Render>,
    pub light: u64,
    pub tier: String,
    pub size: u32,
    /// The open catalogue row and its edit revision (a ball of the row being
    /// edited follows the edit).
    pub open: (Option<usize>, u64),
    /// Where the ball is seen from ([`view_from`] the light in use).
    pub toward: [f32; 3],
}

/// FNV-1a, 64 bit: a stable key across runs and builds.
pub fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The picture key of `material` in `ctx`.
pub fn key(ctx: &Context, material: &str, lib: Option<&LibraryData>) -> u64 {
    let edit = lib.and_then(|l| l.0.find(material)).filter(|(i, _)| ctx.open.0 == Some(*i)).map_or(0, |_| ctx.open.1);
    fnv(&format!("{material}|{}|{:?}|{}|{}|{}|{edit}", ctx.style, ctx.surface, ctx.light, ctx.tier, ctx.size))
}

/// The light's key from the light preset and the light's numbers.
fn light_now(world: &World, preset: &str) -> u64 {
    let reg = world.res::<ParamRegistry>();
    let mut s = preset.to_string();
    for name in ["render.light.direction", "render.light.color", "render.light.intensity", "render.ambient", "render.exposure", "render.saturation"] {
        if let Some(v) = reg.id(name).and_then(|id| reg.value(id)) {
            s.push_str(&format!("|{v:.2?}"));
        }
    }
    fnv(&s)
}

/// The current context, or `None` while the studio has no record.
pub fn context(world: &World, light: u64, size: u32, toward: [f32; 3]) -> Option<Context> {
    let studio = world.try_res::<Studio>()?;
    let r = studio.current.as_ref()?;
    let tier = world.try_res::<Tiers>().map_or_else(String::new, |t| t.active().id.clone());
    let open = world.try_res::<CatalogueView>().map_or((None, 0), |v| (v.open, if v.edited.is_some() { v.revision } else { 0 }));
    Some(Context { style: r.selection.style.clone(), surface: render_of(&r.surface, &r.pass), light, tier, size, open, toward })
}

/// The ball model of `material`: its catalogue row (as edited, when it is
/// the open one) on [`crate::shapes::ball`], in `surface`'s treatment.
pub fn ball_glb(lib: &LibraryData, material: &str, surface: Option<&Render>, open: Option<&CatalogueView>, toward: [f32; 3]) -> Result<Vec<u8>, String> {
    let (i, row) = lib.0.find(material).ok_or_else(|| format!("{material}: not in the catalogue"))?;
    if !lib.0.openable(i) {
        return Err(format!("{material}: not ready to draw"));
    }
    let edited = open.filter(|v| v.open == Some(i)).and_then(|v| v.edited.clone());
    let row = edited.unwrap_or_else(|| working_row(&lib.0.cat, row));
    let look = scene_build::region_look(&lib.0.cat, &row, false)?;
    let m = crate::shapes::ball();
    let positions = m.positions.iter().map(|p| p.map(|x| x * RADIUS)).collect();
    let mesh = StyledMesh { parts: vec![StyledPart { region: 0, positions, normals: m.normals, indices: m.indices, samples: Vec::new() }] };
    let light = surface.map_or(crate::treat::SUN, |s| scene_build::band_light(toward, s));
    scene_build::build(&Build { mesh: &mesh, looks: &[look], surface, light, display: Display::Render, wire: 0.0, uv: "ball" }).map(|(b, _)| b)
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(Balls::default());
    ctx.resource(Swatches::default());
}

/// After the studio scene's draw system (main.rs installs the studio's
/// late systems after the other scenes').
pub fn install_system(ctx: &mut PluginContext) {
    ctx.system(SystemDesc::new("material-view.studio.balls", Stage::Frame, run));
}

fn run(world: &mut World) {
    if !world.try_res::<Viewer>().is_some_and(|v| v.scene == SCENE) {
        return;
    }
    // Only the balls' own pictures: the asset tiles (tiles.rs) share the
    // swatch pass and take theirs after.
    let mine: Vec<u64> = world.try_res::<Balls>().map(|b| b.slots.iter().filter_map(|s| s.job.map(|j| j.key)).collect()).unwrap_or_default();
    let (done, failed) = match world.try_res_mut::<Swatches>() {
        Some(mut s) => (s.take_done_where(|k| mine.contains(&k)), s.take_failed_where(|k| mine.contains(&k))),
        None => return,
    };
    let preset = world.try_res::<Studio>().map(|s| s.state.view.light.clone()).unwrap_or_default();
    let light = light_now(world, &preset);
    let toward = {
        let reg = world.res::<ParamRegistry>();
        match reg.id("render.light.direction").and_then(|id| reg.value(id)) {
            Some(evani_core::params::ParamValue::Vec3(l)) => view_from(l),
            _ => TOWARD,
        }
    };
    let (ctx, lib) = {
        let Some(mut b) = world.try_res_mut::<Balls>() else { return };
        b.frame += 1;
        let frame = b.frame;
        for img in done {
            for s in b.slots.iter_mut().filter(|s| s.job.is_some_and(|j| j.key == img.key)) {
                s.job = None;
            }
            b.pictures += 1;
            b.insert(Picture { key: img.key, size: img.size, rgba: img.rgba });
        }
        for (k, _) in failed {
            for s in b.slots.iter_mut().filter(|s| s.job.is_some_and(|j| j.key == k)) {
                s.job = None;
            }
        }
        for s in &mut b.slots {
            if s.job.is_some_and(|j| frame - j.since > TIMEOUT) {
                s.job = None;
            }
        }
        // The light the balls use follows the frame's once it holds still.
        if light == b.light.1 {
            b.light.2 = b.light.2.saturating_add(1);
        } else {
            b.light.1 = light;
            b.light.2 = 0;
        }
        if b.light.0 == 0 || b.light.2 >= LIGHT_SETTLE {
            b.light.0 = b.light.1;
            b.toward = toward;
        }
        let (l, size, toward) = (b.light.0, b.size, b.toward);
        drop(b);
        let Some(ctx) = context(world, l, size, toward) else { return };
        (ctx, world.try_res::<LibraryData>().map(|l| l.clone()))
    };
    let Some(lib) = lib else { return };
    // The page: paint the pictures that are in, and list the balls on
    // screen still waiting for theirs.
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = dom::paint(world, &ctx, &lib) {
        world.res_mut::<Balls>().wanted = w;
    }
    let wanted = world.res::<Balls>().wanted.clone();
    // One new ball a frame, into a free slot, once the context holds still.
    let next = {
        let mut b = world.res_mut::<Balls>();
        let now = key(&ctx, "", None);
        if b.seen.0 != now {
            b.seen = (now, b.frame);
        }
        let settled = b.frame.saturating_sub(b.seen.1) >= SETTLE || b.builds == 0;
        let free = b.slots.iter().position(|s| s.job.is_none()).or((b.slots.len() < POOL).then_some(b.slots.len()));
        free.filter(|_| settled).and_then(|f| wanted.iter().map(|m| (m, key(&ctx, m, Some(&lib)))).find(|(_, k)| !b.has(*k) && !b.failed(*k) && !b.busy(*k)).map(|(m, k)| (f, m.clone(), k)))
    };
    if let Some((slot, material, k)) = next {
        let open = world.try_res::<CatalogueView>().map(|v| v.clone());
        match ball_glb(&lib, &material, ctx.surface.as_ref(), open.as_ref(), ctx.toward) {
            Ok(bytes) => {
                let old = world.res::<Balls>().slots.get(slot).map(|s| s.model);
                let model = {
                    let mut models = world.res_mut::<Models>();
                    match old {
                        Some(m) => {
                            models.replace_glb(m, &bytes);
                            m
                        }
                        None => models.add_glb(&format!("studio/ball{slot}.glb"), &bytes),
                    }
                };
                {
                    let mut s = world.res_mut::<Swatches>();
                    s.add_model(model);
                    s.cancel(model);
                    let uploads = s.uploads(model) + 1;
                    s.request(SwatchRequest { key: k, model, uploads, camera: swatch::camera(RADIUS, ctx.toward), size: ctx.size, background: None });
                }
                let mut b = world.res_mut::<Balls>();
                let job = Some(Job { key: k, since: b.frame });
                if slot < b.slots.len() {
                    b.slots[slot].job = job;
                } else {
                    b.slots.push(Slot { model, job });
                }
                b.builds += 1;
            }
            Err(_) => world.res_mut::<Balls>().failed.push(k),
        }
    }
    // Every ball waiting for its picture is drawn (so the renderer has it
    // staged); the swatch pass keeps it out of the view.
    let busy: Vec<ModelId> = world.res::<Balls>().slots.iter().filter(|s| s.job.is_some()).map(|s| s.model).collect();
    let mut models = world.res_mut::<Models>();
    for m in busy {
        models.draw(ModelInstance::new(m, swatch::AT));
    }
}

/// The page side (browser only): every `<canvas data-ball="<material>">`
/// (page_view.rs writes them; `data-pri` 0 for the inspector and the card,
/// 1 for the chips) is painted with its picture once it is in (`data-k` is
/// the picture's key); the ones on screen without one are the wanted list.
#[cfg(target_arch = "wasm32")]
mod dom {
    use evani_core::ecs::World;
    use evani_kit_material_view::catalogue::LibraryData;
    use wasm_bindgen::{Clamped, JsCast};
    use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

    use super::{Balls, Context, Picture, key};

    fn put(c: &HtmlCanvasElement, p: &Picture) -> bool {
        if c.width() != p.size || c.height() != p.size {
            c.set_width(p.size);
            c.set_height(p.size);
        }
        let Some(g) = c.get_context("2d").ok().flatten().and_then(|g| g.dyn_into::<CanvasRenderingContext2d>().ok()) else { return false };
        let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&p.rgba), p.size, p.size) else { return false };
        g.put_image_data(&img, 0, 0).is_ok()
    }

    /// Paint what is in; return the materials on screen still waiting, most
    /// wanted first (`None`: no page).
    pub fn paint(world: &World, ctx: &Context, lib: &LibraryData) -> Option<Vec<String>> {
        let win = web_sys::window()?;
        let doc = win.document()?;
        let size = if win.device_pixel_ratio() >= 1.75 { 128 } else { 96 };
        let list = doc.query_selector_all("canvas[data-ball]").ok()?;
        let num = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| v.ok().and_then(|x| x.as_f64()).unwrap_or(0.0);
        let (vw, vh) = (num(win.inner_width()), num(win.inner_height()));
        let mut balls = world.res_mut::<Balls>();
        balls.size = size;
        let mut wanted: Vec<(u8, f64, String)> = Vec::new();
        for i in 0..list.length() {
            let Some(c) = list.item(i).and_then(|n| n.dyn_into::<HtmlCanvasElement>().ok()) else { continue };
            let Some(m) = c.get_attribute("data-ball") else { continue };
            let k = key(ctx, &m, Some(lib));
            let ks = format!("{k:x}");
            if c.get_attribute("data-k").as_deref() == Some(ks.as_str()) {
                continue;
            }
            if let Some(p) = balls.picture(k) {
                if put(&c, p) {
                    let _ = c.set_attribute("data-k", &ks);
                }
                continue;
            }
            if balls.failed(k) {
                // A row not ready to draw keeps the soft colour, marked so a
                // check can tell it from a ball still on its way.
                let _ = c.set_attribute("data-nb", "1");
                continue;
            }
            let r = c.get_bounding_client_rect();
            if r.width() <= 0.0 || r.right() < 0.0 || r.left() > vw || r.bottom() < 0.0 || r.top() > vh {
                continue;
            }
            // A chip scrolled out of its row is not on screen.
            if let Some(row) = c.closest(".st-chips, .st-morepop, #st-morepop").ok().flatten() {
                let pr = row.get_bounding_client_rect();
                if r.right() < pr.left() || r.left() > pr.right() || r.bottom() < pr.top() || r.top() > pr.bottom() {
                    continue;
                }
            }
            let pri = c.get_attribute("data-pri").and_then(|p| p.parse().ok()).unwrap_or(2);
            wanted.push((pri, r.left() + r.top() * 4.0, m));
        }
        wanted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut out: Vec<String> = Vec::with_capacity(wanted.len());
        for (_, _, m) in wanted {
            if !out.contains(&m) {
                out.push(m);
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_render::model::recipes::RECIPE_KEY;

    const DT: f64 = 1000.0 / 60.0;

    /// A `.glb`'s JSON chunk (the engine reads the same file).
    fn json(glb: &[u8]) -> serde_json::Value {
        assert!(evani_gltf::Gltf::from_glb(glb).is_ok(), "the engine reads it");
        let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&glb[20..20 + len]).unwrap()
    }

    /// The recipe block of a ball model's first material (`None`: a plain
    /// tinted material, no pattern).
    fn recipe(glb: &[u8]) -> Option<serde_json::Value> {
        json(glb)["materials"][0]["extras"].get(RECIPE_KEY).cloned()
    }

    fn library() -> LibraryData {
        LibraryData(std::rc::Rc::new(evani_kit_material_view::catalogue::Library::bundled().unwrap()))
    }

    /// The guard against plain balls: brick, wood, fur and metal balls are
    /// each their own catalogue row's material (its recipe block, so the
    /// recipe stage draws the pattern), never a tinted sphere; their blocks
    /// differ; and the ball is a whole sphere of radius 1.
    #[test]
    fn a_ball_is_the_real_material_never_a_tint() {
        let lib = library();
        let mut seen = Vec::new();
        for m in ["brick.red.common", "wood.walnut.stock", "horse.coat.bay", "metal.steel.brushed"] {
            let glb = ball_glb(&lib, m, None, None, TOWARD).unwrap();
            let block = recipe(&glb).unwrap_or_else(|| panic!("{m}: the ball is a plain tint, not the material"));
            assert!(!seen.contains(&block), "{m}: the same block as another material");
            seen.push(block);
            assert_eq!(json(&glb)["meshes"].as_array().map(Vec::len), Some(1));
        }
        // A toon style keeps the pattern on the ball's lit body.
        let toon = Render { bands: 3, outline: 0.02, ..Render::default() };
        assert!(recipe(&ball_glb(&lib, "brick.red.common", Some(&toon), None, TOWARD).unwrap()).is_some());
        assert!(ball_glb(&lib, "no.such.material", None, None, TOWARD).is_err());
    }

    /// Keys follow every input of the picture and nothing else; pictures
    /// are kept least recently shown first.
    #[test]
    fn keys_and_the_cache() {
        let ctx = Context { style: "WK01".into(), surface: None, light: 7, tier: "high".into(), size: 128, open: (None, 0), toward: TOWARD };
        let k = key(&ctx, "brick.red.common", None);
        assert_eq!(k, key(&ctx.clone(), "brick.red.common", None), "stable");
        for other in [
            Context { style: "WK08".into(), ..ctx.clone() },
            Context { light: 8, ..ctx.clone() },
            Context { tier: "low".into(), ..ctx.clone() },
            Context { size: 96, ..ctx.clone() },
            Context { surface: Some(Render { bands: 3, ..Render::default() }), ..ctx.clone() },
        ] {
            assert_ne!(key(&other, "brick.red.common", None), k, "{other:?}");
        }
        assert_ne!(key(&ctx, "brick.red.clean", None), k);
        let mut b = Balls::default();
        for i in 0..(CACHE as u64 + 2) {
            b.insert(Picture { key: i, size: 1, rgba: vec![0; 4] });
        }
        assert!(!b.has(0) && !b.has(1) && b.has(2));
        assert!(b.picture(2).is_some());
        b.insert(Picture { key: 999, size: 1, rgba: vec![0; 4] });
        assert!(b.has(2) && !b.has(3), "a picture shown again is kept");
    }

    /// The ball is seen AROUND_DEG round from the light, a little above, so
    /// it has a lit side and a shadow side; a light from straight above
    /// keeps the fixed view.
    #[test]
    fn the_ball_is_lit_from_three_quarters() {
        for l in [[1.0, 1.0, 0.0], [-0.3, 0.8, 0.6], [0.0, 0.5, -1.0]] {
            let v = view_from(l);
            let (a, b) = (l[0].atan2(l[2]), v[0].atan2(v[2]));
            let d = (b - a).to_degrees().rem_euclid(360.0);
            assert!((d - AROUND_DEG).abs() < 1e-3, "{d}");
            assert!((v[1] - VIEW_UP_DEG.to_radians().sin()).abs() < 1e-5);
        }
        assert_eq!(view_from([0.0, 1.0, 0.0]), TOWARD);
    }

    /// In the app: the page's wanted balls get built one a frame into at
    /// most POOL slots, each asked of the swatch pass, drawn far from the
    /// scene while they wait, and marked as swatch models (never drawn in
    /// the view). Without a GPU no picture comes back, so the slots stay
    /// busy and nothing more is built.
    #[test]
    fn wanted_balls_are_built_a_few_at_a_time_and_kept_out_of_the_view() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        app.world.res_mut::<Viewer>().scene = SCENE.into();
        app.run_headless(2, DT);
        let wanted: Vec<String> = ["brick.red.common", "wood.walnut.stock", "horse.coat.bay", "metal.steel.brushed", "brick.red.clean"].map(String::from).to_vec();
        app.world.res_mut::<Balls>().wanted = wanted;
        app.run_headless(1, DT);
        assert_eq!(app.world.res::<Balls>().builds, 1, "one ball a frame");
        app.run_headless(SETTLE as u32 + 6, DT);
        let b = app.world.res::<Balls>();
        assert_eq!((b.builds, b.slots.len()), (POOL as u64, POOL));
        let s = app.world.res::<Swatches>();
        assert_eq!(s.pending(), POOL);
        assert_eq!(s.models().len(), POOL);
        for slot in &b.slots {
            assert!(s.is_swatch(slot.model.0 as usize));
        }
    }
}
