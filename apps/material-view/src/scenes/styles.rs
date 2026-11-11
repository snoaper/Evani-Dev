//! The art styles' scene (feature `material-styles`): the open style's
//! starter materials (kit `styles`) on the chosen test shape (`shapes`).
//!
//! * Show all: every material of the style, side by side at one size in the
//!   part of the view the page's bars and panels leave open ([`FreeArea`]):
//!   one row while each shape gets [`ROW_MIN_PX`], else a grid. The set
//!   always faces the camera, so Spin turns each shape in place, and each
//!   shape then slides along its view ray onto the floor ([`grounded`]),
//!   so it rests there with its shadow, at the same spot on screen. Otherwise
//!   only the selected material, in the middle of that open part. A thin
//!   glowing ring on the floor marks the selected one; each shape has a
//!   contact shadow.
//! * Each drawn place has one model in the table, made once with
//!   [`Models::add_glb`] and swapped in place with [`Models::replace_glb`]
//!   whenever its shape, roughness, metallic, bump or glow change, so an
//!   edit never grows the table. The colour is a live tint (no rebuild);
//!   only a glowing material bakes it into its emissive.
//! * The sphere, cube, long box, prism, cylinder and ring are made in code;
//!   the monkey and the teapot stream in through the manifest like any
//!   model, and their mesh is read once from the same file. A pale ball
//!   stands in while a model isn't ready.
//! * A style with nothing to show yet draws only the ground (the page says
//!   "not made yet").

use std::collections::HashMap;
use std::rc::Rc;

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::ErrorLog;
use evani_core::params::ParamRegistry;
use evani_core::plugin::{LookData, PluginContext};
use evani_core::time::Time;
use evani_kit_material_view::{MaterialViewKit, Viewer};
use evani_kit_material_view::styles::{self, Finish, Render, Status, StyleView, StylesData};
use evani_render::model::{ModelHandle, ModelId, ModelInstance, Models};
use evani_render::{MeshId, RenderWorld};

use super::common::{material, mesh, showing};
use crate::shapes::{self, Mesh, Pbr, Relief, Shape};
use crate::treat;

/// The scenes this family draws.
pub const SCENES: [&str; 1] = ["styles"];
/// Metres between neighbouring shapes (before fitting).
pub const SPACING: f32 = 2.4;
/// The smallest cell, CSS px, one row may give each shape; below it the
/// shapes go into a grid.
pub const ROW_MIN_PX: f32 = 120.0;
/// CSS px kept clear inside the open part of the view.
pub const FREE_PAD: f32 = 10.0;
/// An open part narrower or lower than this (CSS px) is ignored and the
/// whole view used.
pub const FREE_MIN_PX: f32 = 60.0;
/// A shape's radius (every test shape fits about this).
pub const RADIUS: f32 = 1.05;
/// How high a grounded shape's lowest point sits above the floor (metres).
pub const GROUND_GAP: f32 = 0.004;
/// How far [`grounded`] may slide a shape along its view ray (times its
/// planned distance, either way).
pub const MAX_PUSH: f32 = 2.5;
/// The streamed shapes' model ids are their shape ids.
#[cfg(test)]
pub const STREAMED: [Shape; 2] = [Shape::Monkey, Shape::Teapot];

/// What a place's model was built from: the shape and the finish's
/// factors as exact bits (the tint only when it glows, since a plain
/// colour is a live tint), plus the style's shape look when it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildKey {
    pub shape: Shape,
    bits: [u32; 7],
    /// 0 for a plain style; else a hash of the style's shape look and the
    /// sun (to 0.02), which its colours are built from.
    treat: u64,
    /// The relief the bump draws (by the material).
    pub relief: Relief,
}

impl BuildKey {
    pub fn new(shape: Shape, f: &Finish) -> Self {
        let tint = if f.glow > 0.0 { f.tint } else { [0.0; 3] };
        Self {
            shape,
            bits: [
                f.roughness.to_bits(),
                f.metallic.to_bits(),
                f.bump.to_bits(),
                f.glow.to_bits(),
                tint[0].to_bits(),
                tint[1].to_bits(),
                tint[2].to_bits(),
            ],
            treat: 0,
            relief: Relief::Noise,
        }
    }

    /// A style with a shape look: its colours are built into the model, so
    /// the tint always counts, and so do the look and the sun.
    pub fn treated(shape: Shape, f: &Finish, style: usize, r: &Render, light: [f32; 3]) -> Self {
        let mut k = Self::new(shape, f);
        for (b, t) in k.bits[4..].iter_mut().zip(f.tint) {
            *b = t.to_bits();
        }
        let sun = light.map(|x| (x / 0.02).round() as i32);
        k.treat = fnv(format!("{style}|{sun:?}|{r:?}").as_bytes()) | 1;
        k
    }

    /// The key for this place: treated when the style changes the shapes.
    pub fn for_style(shape: Shape, f: &Finish, style: usize, r: Option<&Render>, light: [f32; 3]) -> Self {
        match r {
            Some(r) => Self::treated(shape, f, style, r, light),
            None => Self::new(shape, f),
        }
    }
}

/// One drawn place's model.
#[derive(Debug, Clone)]
pub struct Slot {
    pub model: ModelId,
    pub key: BuildKey,
    /// FNV-1a of the bytes last built (what tests compare).
    pub hash: u64,
    /// How many parts (primitives) the model has: 1 plain, more for a look.
    pub parts: usize,
}

/// wasm: a fetch's result, filled in when the file arrives.
#[cfg(target_arch = "wasm32")]
type Fetched = Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>>;

/// A streamed shape's mesh, while it is read.
#[derive(Debug, Default)]
enum Source {
    #[default]
    Waiting,
    /// wasm: the file's bytes are on their way.
    #[cfg(target_arch = "wasm32")]
    Fetching(Fetched),
    Failed,
}

/// The scene's state (a world resource).
#[derive(Debug, Default)]
pub struct StylesScene {
    /// One model per place, reused across edits and shapes.
    pub slots: Vec<Slot>,
    /// Material index drawn at each place this frame, with its model once ready.
    pub drawn: Vec<(usize, Option<ModelId>)>,
    /// Models built so far (adds and replaces).
    pub builds: u64,
    meshes: HashMap<Shape, Rc<Mesh>>,
    bumped: HashMap<(Shape, Relief, u32), Rc<Mesh>>,
    /// Treated geometry by shape, style and bump bits (only those in use).
    treated: HashMap<(Shape, usize, u32), Rc<Mesh>>,
    handles: HashMap<Shape, ModelHandle>,
    sources: HashMap<Shape, Source>,
    /// The selection ring's model, made the first time it shows.
    ring: Option<ModelId>,
}

/// A style's shape look for a build: its index, its render data and the sun.
type Look<'a> = Option<(usize, &'a Render, [f32; 3])>;

/// The sun direction (toward the light) from `render.light.direction`.
fn sun(world: &World) -> [f32; 3] {
    use evani_core::params::ParamValue;
    let reg = world.res::<ParamRegistry>();
    match reg.id("render.light.direction").and_then(|id| reg.value(id)) {
        Some(ParamValue::Vec3(v)) if v.iter().all(|x| x.is_finite()) => v,
        _ => treat::SUN,
    }
}

/// The light a style's look is cut from: the sun, turned toward the home
/// camera when it sits too far round ([`treat::key_light`]), so it doesn't
/// change while the camera spins or is dragged.
pub(super) fn band_light(world: &World, render: Option<&Render>) -> [f32; 3] {
    let s = sun(world);
    let Some(cam) = world.try_res::<Viewer>().map(|v| v.home.pose(0.0)) else { return s };
    let eye = [cam.eye[0] - cam.target[0], cam.eye[1] - cam.target[1], cam.eye[2] - cam.target[2]];
    treat::key_light(s, eye, render.map_or(0.0, |r| r.side_light))
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(StylesScene::default());
    ctx.resource(FreeArea::default());
    ctx.system(SystemDesc::new("material-view.scene.styles", Stage::Frame, run));
}

/// FNV-1a, 64 bits.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The part of the view the page's bars and panels leave open: CSS px
/// inside the canvas, `[left, top, right, bottom]`, and the canvas's own
/// CSS size. The page writes it each frame; with none written (native,
/// tests) the whole view counts as open.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FreeArea {
    pub rect: [f32; 4],
    pub view: [f32; 2],
}

/// Where each of `n` shapes stands and how big it is drawn. The set stands
/// on the plane through `cam`'s target facing `cam`, so a turning camera
/// (Spin, a drag) turns each shape in place instead of swinging the set
/// into a deep line. It fits the open part `free` (`[left, top, right,
/// bottom]`, CSS px) of a `view`-sized canvas, centred in it: one row while
/// each shape still gets [`ROW_MIN_PX`], else the grid that draws them
/// biggest (a short last row is centred). Never bigger than scale 1.
pub fn layout(n: usize, cam: &CameraPose, view: [f32; 2], free: [f32; 4]) -> (Vec<[f32; 3]>, f32) {
    if n == 0 {
        return (Vec::new(), 1.0);
    }
    let (vw, vh) = (view[0].max(1.0), view[1].max(1.0));
    let mut f = [free[0].max(0.0) + FREE_PAD, free[1].max(0.0) + FREE_PAD, free[2].min(vw) - FREE_PAD, free[3].min(vh) - FREE_PAD];
    if f[2] - f[0] < FREE_MIN_PX || f[3] - f[1] < FREE_MIN_PX {
        f = [0.0, 0.0, vw, vh];
    }
    let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let back = [cam.eye[0] - cam.target[0], cam.eye[1] - cam.target[1], cam.eye[2] - cam.target[2]];
    let d = len(back).max(0.1);
    let fwd = back.map(|x| -x / d);
    let r = cross(fwd, cam.up);
    let rl = len(r);
    let right = if rl > 1e-4 { r.map(|x| x / rl) } else { [1.0, 0.0, 0.0] };
    let up = cross(right, fwd);
    // Metres per CSS px on that plane.
    let k = 2.0 * d * (cam.fov_y_deg.to_radians() * 0.5).tan() / vh;
    let (fw, fh) = (f[2] - f[0], f[3] - f[1]);
    let cell = |cols: usize| (fw / cols as f32).min(fh / n.div_ceil(cols) as f32);
    let cols = if n == 1 || cell(n) >= ROW_MIN_PX { n } else { (1..=n).fold(n, |best, c| if cell(c) > cell(best) { c } else { best }) };
    let rows = n.div_ceil(cols);
    let s = (cell(cols) * k / SPACING).min(1.0);
    let step = SPACING * s;
    let (cx, cy) = (((f[0] + f[2]) * 0.5 - vw * 0.5) * k, (vh * 0.5 - (f[1] + f[3]) * 0.5) * k);
    let mut at = Vec::with_capacity(n);
    for i in 0..n {
        let (c, r) = (i % cols, i / cols);
        let in_row = if r + 1 == rows { n - r * cols } else { cols };
        let x = cx + (c as f32 - (in_row - 1) as f32 * 0.5) * step;
        let y = cy + ((rows - 1) as f32 * 0.5 - r as f32) * step;
        let mut p: [f32; 3] = std::array::from_fn(|a| cam.target[a] + right[a] * x + up[a] * y);
        // Never into the ground.
        p[1] = p[1].max(RADIUS * s + 0.02);
        at.push(p);
    }
    (at, s)
}

/// The planned places stood on the floor: each slid along the ray from
/// `cam`'s eye through it, and scaled by its new distance, until the
/// shape's lowest point (`bottom`, mesh units, below its centre) rests on
/// the ground, so it draws at the same spot and size on screen but its
/// shadow and the selection disc meet it. Decision: on the floor rather
/// than on a stand, because the set plane hung every shape in the air
/// (a desktop row half a radius up, a phone grid's top row higher still).
/// A ray that barely falls (a low camera, a top row) would run far off:
/// past [`MAX_PUSH`] the place stays as planned.
pub fn grounded(at: &[[f32; 3]], s: f32, cam: &CameraPose, bottom: f32) -> Vec<([f32; 3], f32)> {
    let depth = (-bottom).clamp(0.05, RADIUS);
    let h = cam.eye[1] - GROUND_GAP;
    at.iter()
        .map(|&p| {
            let d = [p[0] - cam.eye[0], p[1] - cam.eye[1], p[2] - cam.eye[2]];
            let t0 = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            // eye.y + dir.y t = depth s t / t0 + gap.
            let fall = depth * s / t0.max(1e-4) - d[1] / t0.max(1e-4);
            if t0 > 1e-4 && h > 0.0 && fall > 0.0 {
                let t = h / fall;
                if t <= t0 * MAX_PUSH && t >= t0 / MAX_PUSH {
                    let k = t / t0;
                    return (std::array::from_fn(|a| cam.eye[a] + d[a] * k), s * k);
                }
            }
            (p, s)
        })
        .collect()
}

/// The radius of the soft contact shadow under a grounded shape, in the
/// shape's own units (meshes are centred, radius 1): 0.9 for a shape that
/// meets the floor at a point (a sphere), and wider than its foot for one
/// that stands on a flat base, so the shadow shows round it (critic round 5:
/// on Low the denim pouf's disc lay wholly under the pouf).
pub fn contact_radius(m: Option<&Mesh>) -> f32 {
    let Some(m) = m else { return 0.9 };
    let bottom = m.positions.iter().map(|p| p[1]).fold(0.0_f32, f32::min);
    let foot = m.positions.iter().filter(|p| p[1] < bottom + 0.1).map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt()).fold(0.0_f32, f32::max);
    // The disc is full dark to 0.45 of its radius: at 1.8x the foot it is
    // dark right round the foot and fades over about its width.
    (1.8 * foot).max(0.9)
}

/// A streamed model file's mesh: level 0 and the shared parts of a
/// converted file, each in its node's place, centred and scaled to radius 1.
pub fn mesh_from_glb(glb: &[u8]) -> Result<Mesh, String> {
    let mut m = raw_mesh_from_glb(glb)?;
    let (c, r) = bounds(&m);
    let s = if r > 1e-6 { 1.0 / r } else { 1.0 };
    for p in &mut m.positions {
        *p = [0, 1, 2].map(|k| (p[k] - c[k]) * s);
    }
    Ok(m)
}

/// A model file's box centre and radius around it, as the file places it
/// (what [`mesh_from_glb`] moves to the origin and scales to 1).
#[cfg(feature = "material-catalogue")]
pub fn glb_fit(glb: &[u8]) -> Result<([f32; 3], f32), String> {
    raw_mesh_from_glb(glb).map(|m| bounds(&m))
}

fn bounds(m: &Mesh) -> ([f32; 3], f32) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in &m.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    let r = m.positions.iter().map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt()).fold(0.0_f32, f32::max);
    (c, r)
}

fn raw_mesh_from_glb(glb: &[u8]) -> Result<Mesh, String> {
    let g = evani_gltf::Gltf::from_glb(glb).map_err(|e| e.0.to_string())?;
    let p = evani_render::model::prepare(&g).map_err(|e| e.0.to_string())?;
    let mut m = Mesh::default();
    for part in p.parts.iter().filter(|x| x.lod == 0 || x.lod == evani_render::model::SHARED) {
        let base = m.positions.len() as u32;
        let n = &part.node;
        for v in part.vertices.as_chunks::<{ evani_render::model::VERTEX_BYTES }>().0 {
            let q = |k: usize| f32::from(u16::from_le_bytes([v[k * 2], v[k * 2 + 1]])) / 65535.0;
            let l = [0, 1, 2].map(|k| part.pos_offset[k] + part.pos_scale[k] * q(k));
            let nl = [0, 1, 2].map(|k| f32::from(v[8 + k] as i8) / 127.0);
            let pos = [0, 1, 2].map(|r| n[r] * l[0] + n[4 + r] * l[1] + n[8 + r] * l[2] + n[12 + r]);
            let nn = [0, 1, 2].map(|r| n[r] * nl[0] + n[4 + r] * nl[1] + n[8 + r] * nl[2]);
            let len = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt();
            m.positions.push(pos);
            m.normals.push(if len > 1e-6 { nn.map(|c| c / len) } else { [0.0, 1.0, 0.0] });
        }
        m.indices.extend(part.indices.iter().map(|&i| base + i));
    }
    if m.indices.is_empty() {
        return Err("no triangles".into());
    }
    Ok(m)
}

impl StylesScene {
    /// A shape's base mesh, if it is in (generated ones always are).
    pub(super) fn base(&mut self, shape: Shape, models: &mut Models, log: &mut Vec<String>) -> Option<Rc<Mesh>> {
        if let Some(m) = self.meshes.get(&shape) {
            return Some(m.clone());
        }
        if let Some(m) = shapes::build(shape) {
            let m = Rc::new(m);
            self.meshes.insert(shape, m.clone());
            return Some(m);
        }
        // Streamed: wait for the handle's file, then read its mesh once.
        let h = *self.handles.entry(shape).or_insert_with(|| models.handle(shape.id()));
        let id = models.resolve(h)?;
        let url = models.url(id)?.to_string();
        let src = self.sources.entry(shape).or_default();
        let bytes: Option<Result<Vec<u8>, String>> = match src {
            Source::Failed => return None,
            #[cfg(not(target_arch = "wasm32"))]
            Source::Waiting => {
                const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/");
                let file = url.strip_prefix(evani_render::model::stream::FOLDER).unwrap_or(&url);
                Some(std::fs::read(format!("{DIR}{file}")).map_err(|e| format!("{file}: {e}")))
            }
            #[cfg(target_arch = "wasm32")]
            Source::Waiting => {
                use evani_core::assets::AssetSource;
                let cell = Rc::new(std::cell::RefCell::new(None));
                let into = cell.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let r = evani_core::web::FetchSource.fetch(&url).await;
                    *into.borrow_mut() = Some(r);
                });
                *src = Source::Fetching(cell);
                None
            }
            #[cfg(target_arch = "wasm32")]
            Source::Fetching(cell) => cell.borrow_mut().take(),
        };
        match bytes?.and_then(|b| mesh_from_glb(&b)) {
            Ok(m) => {
                let m = Rc::new(m);
                self.sources.remove(&shape);
                self.meshes.insert(shape, m.clone());
                Some(m)
            }
            Err(e) => {
                *src = Source::Failed;
                log.push(format!("material-view: styles: the {} model: {e}", shape.label()));
                None
            }
        }
    }

    /// The mesh to build: the base, bumped when the finish asks.
    fn mesh_for(&mut self, shape: Shape, relief: Relief, bump: f32, models: &mut Models, log: &mut Vec<String>) -> Option<Rc<Mesh>> {
        let base = self.base(shape, models, log)?;
        if bump == 0.0 {
            return Some(base);
        }
        let mesh = || {
            // A patterned relief on the sphere needs the finer sphere.
            let fine = (shape == Shape::Sphere && relief != Relief::Noise).then(shapes::relief_sphere);
            Rc::new(shapes::relief(fine.as_ref().unwrap_or(&base), relief, bump))
        };
        Some(self.bumped.entry((shape, relief, bump.to_bits())).or_insert_with(mesh).clone())
    }

    /// A style's treated geometry for a shape (built once per bump value).
    fn treated_mesh(&mut self, shape: Shape, style: usize, r: &Render, bump: f32, models: &mut Models, log: &mut Vec<String>) -> Option<Rc<Mesh>> {
        if let Some(m) = self.treated.get(&(shape, style, bump.to_bits())) {
            return Some(m.clone());
        }
        let base = self.base(shape, models, log)?;
        let m = Rc::new(treat::geometry(&base, r, bump));
        self.treated.insert((shape, style, bump.to_bits()), m.clone());
        Some(m)
    }

    /// Place `i`'s model for `key` (built now when it changed). `None`
    /// while the shape's mesh isn't in.
    fn model(&mut self, i: usize, key: BuildKey, f: &Finish, look: Look, models: &mut Models, log: &mut Vec<String>) -> Option<ModelId> {
        if let Some(s) = self.slots.get(i)
            && s.key == key
        {
            return Some(s.model);
        }
        let (bytes, parts) = match look {
            Some((style, r, light)) => {
                let geo = self.treated_mesh(key.shape, style, r, f.bump, models, log)?;
                let parts = treat::shade(&geo, r, f, light);
                let list: Vec<(&Mesh, &Pbr, bool)> = parts.iter().map(|p| (&p.mesh, &p.pbr, p.double_sided)).collect();
                (shapes::glb_parts(&list), parts.len())
            }
            None => {
                let m = self.mesh_for(key.shape, key.relief, f.bump, models, log)?;
                let glow = f.glow.clamp(0.0, 1.0);
                let pbr = Pbr { base: [1.0; 4], metallic: f.metallic, roughness: f.roughness, emissive: f.tint.map(|c| c * glow) };
                (shapes::glb(&m, &pbr), 1)
            }
        };
        let hash = fnv(&bytes);
        self.builds += 1;
        match self.slots.get_mut(i) {
            Some(s) => {
                models.replace_glb(s.model, &bytes);
                s.key = key;
                s.hash = hash;
                s.parts = parts;
                Some(s.model)
            }
            None => {
                // Places are filled in order, so `i` is the next one.
                let model = models.add_glb(&format!("styles/slot{i}.glb"), &bytes);
                self.slots.push(Slot { model, key, hash, parts });
                Some(model)
            }
        }
    }
}

/// The camera the set is laid out for, the canvas's CSS size and the part
/// of it the page leaves open. The set faces the camera as it turns now
/// (orbit + turntable) but keeps the home view's centre and distance, so
/// zoom and pan still move it.
pub(super) fn view_frame(world: &World) -> Option<(CameraPose, [f32; 2], [f32; 4])> {
    let tick = world.res::<Time>().tick;
    let speed = world.try_res::<MaterialViewKit>().map_or(0.0, |k| world.res::<ParamRegistry>().f32(k.turntable_speed));
    // While a capture holds its own camera or clock (the world-edge and
    // geometry renders, live builds only), the set is laid out at the
    // turntable's start, so two renders of one pose show the same frame.
    let held = world.try_res::<evani_core::capture::CaptureQueue>().is_some_and(|q| q.camera_override.is_some() || q.sim_time_override.is_some());
    let cam = world.try_res::<Viewer>().map(|v| {
        let mut o = v.home;
        (o.az, o.el) = (v.orbit.az, v.orbit.el);
        o.pose(if held { 0.0 } else { v.turn.angle(tick, speed) })
    })?;
    let vp = world.res::<RenderWorld>().viewport_css;
    let cv = if vp[0] > 0.0 && vp[1] > 0.0 { vp } else { [1600.0, 900.0] };
    let open = world.try_res::<FreeArea>().filter(|a| a.view[0] > 0.0 && a.view[1] > 0.0);
    // The open part was measured on a canvas of `a.view`; scale it to this one.
    let free = open.map_or([0.0, 0.0, cv[0], cv[1]], |a| {
        let (sx, sy) = (cv[0] / a.view[0], cv[1] / a.view[1]);
        [a.rect[0] * sx, a.rect[1] * sy, a.rect[2] * sx, a.rect[3] * sy]
    });
    Some((cam, cv, free))
}

fn run(world: &mut World) {
    // The recipe style detail word is set again below while it applies.
    #[cfg(feature = "material-catalogue")]
    super::catalogue::style_off(world);
    let on = showing(world, &SCENES);
    let Some(mut st) = world.try_res_mut::<StylesScene>() else { return };
    st.drawn.clear();
    if !on {
        return;
    }
    drop(st);
    let ground = material(world, "ground");
    world.res_mut::<RenderWorld>().draw_mesh(super::common::floor(ground, super::common::FLOOR_SAND));
    let Some(data) = world.try_res::<StylesData>().map(|d| d.0.clone()) else { return };
    let Some(view) = world.try_res::<StyleView>().map(|v| v.clone()) else { return };
    // A catalogue material open: it takes the set's place, in this style.
    #[cfg(feature = "material-catalogue")]
    if super::catalogue::is_open(world) {
        super::catalogue::draw(world, &data, view.style);
        return;
    }
    let Some(style) = data.styles.get(view.style).filter(|s| s.status != Status::Missing && !s.materials.is_empty()) else {
        return;
    };
    let shape = Shape::from_id(&view.shape).unwrap_or(Shape::Sphere);
    let selected = view.selected.min(style.materials.len() - 1);
    let mats: Vec<usize> = if view.show_all { (0..style.materials.len()).collect() } else { vec![selected] };
    let Some((cam, canvas, free)) = view_frame(world) else { return };
    // The shape's lowest point (meshes are centred, radius 1), so a flat
    // teapot rests on the floor as well as a sphere.
    let (bottom, shadow) = world
        .try_res::<StylesScene>()
        .and_then(|st| st.meshes.get(&shape).map(|m| (m.positions.iter().map(|p| p[1]).fold(0.0_f32, f32::min), contact_radius(Some(m.as_ref())))))
        .unwrap_or((-1.0, contact_radius(None)));
    let (planned, s) = layout(mats.len(), &cam, canvas, free);
    let placed = grounded(&planned, s, &cam, bottom);
    let finishes: Vec<Option<Finish>> = {
        let look = world.try_res::<LookData>();
        mats.iter().map(|&m| styles::finish(&view, &data, look.as_deref(), view.style, m).map(|(f, _, _)| f)).collect()
    };

    let render = style.render.as_ref().filter(|r| r.shaped());
    let light = band_light(world, render);
    let mut log = Vec::new();
    let mut placeholders = Vec::new();
    {
        let Some(mut models) = world.try_res_mut::<Models>() else { return };
        let Some(mut st) = world.try_res_mut::<StylesScene>() else { return };
        super::native_stream(world, &mut models);
        #[cfg(feature = "material-catalogue")]
        if let Some(mut cat) = world.try_res_mut::<super::catalogue::CatScene>() {
            super::catalogue::reserve(&mut models, &mut cat);
        }
        let mut used = Vec::new();
        let mut used_treated = Vec::new();
        for (place, (&m, f)) in mats.iter().zip(&finishes).enumerate() {
            let Some(f) = f else { continue };
            let mut key = BuildKey::for_style(shape, f, view.style, render, light);
            key.relief = style.materials.get(m).map_or(Relief::Noise, |sm| Relief::of(&sm.id));
            if f.bump != 0.0 && render.is_none() {
                used.push((shape, key.relief, f.bump.to_bits()));
            }
            if render.is_some() {
                used_treated.push((shape, view.style, f.bump.to_bits()));
            }
            let look = render.map(|r| (view.style, r, light));
            let id = st.model(place, key, f, look, &mut models, &mut log).filter(|&id| models.ready(id));
            // A look bakes its colours in, so it draws untinted.
            let tint = if render.is_some() { [1.0; 4] } else { [f.tint[0], f.tint[1], f.tint[2], 1.0] };
            let (p, ps) = placed[place];
            match id {
                Some(id) => models.draw(ModelInstance::new(id, p).scale([ps; 3]).tint(tint)),
                None => placeholders.push((p, ps, f.tint)),
            }
            st.drawn.push((m, id));
        }
        // The selection marker: a thin glowing ring on the floor round the
        // selected shape (just under it, if it couldn't be grounded).
        if let Some(&(p, ps)) = mats.iter().position(|&m| m == selected).and_then(|place| placed.get(place)) {
            let ring = *st.ring.get_or_insert_with(|| {
                let glow = Pbr { base: [1.0, 0.82, 0.4, 1.0], metallic: 0.0, roughness: 0.4, emissive: [1.0, 0.72, 0.3] };
                models.add_glb("styles/selection-ring.glb", &shapes::glb(&shapes::selection_ring(), &glow))
            });
            if models.ready(ring) {
                let y = (p[1] + bottom * ps).max(0.0) + 0.012;
                models.draw(ModelInstance::new(ring, [p[0], y, p[2]]).scale([0.95 * ps, ps, 0.95 * ps]));
            }
        }
        // Keep only the bumped meshes still in use (a slider drag would pile them up).
        st.bumped.retain(|k, _| used.contains(k));
        st.treated.retain(|k, _| used_treated.contains(k));
    }
    super::StandIns::add(world, placeholders.len());
    let stone = material(world, "stone");
    let (blob_only, sun) = (super::common::blob_only(world), super::common::sun(world));
    let mut rw = world.res_mut::<RenderWorld>();
    for (p, ps, t) in placeholders {
        rw.draw_mesh(mesh(MeshId::SPHERE, p, [1.8 * ps; 3], stone, [t[0], t[1], t[2], 1.0]));
    }
    // A contact shadow under each grounded shape.
    for &(p, ps) in &placed {
        let foot = p[1] + bottom * ps;
        if foot >= 0.05 {
            continue;
        }
        if blob_only {
            // No shadow map (Lite, Low): a tight disc at the foot and the
            // shape's own oval where Ultra's map throws it (critic round 13:
            // one disc swept from the foot drew the Low shadows fainter than
            // Ultra's). Meshes are centred, radius 1, so the shape is a body
            // of radius `ps` at `p`.
            super::common::contact(&mut rw, [p[0], 0.0, p[2]], 0.5 * shadow * ps, 0.0);
            super::common::body_shadow(&mut rw, p, 0.95 * ps, sun);
        } else {
            super::common::contact(&mut rw, [p[0], 0.0, p[2]], shadow * ps, -2.0 * bottom * ps);
        }
    }
    drop(rw);
    if !log.is_empty() {
        let mut errors = world.res_mut::<ErrorLog>();
        for e in log {
            errors.error(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::{ParamRegistry, ParamValue};
    use evani_kit_material_view::Command;
    use evani_kit_material_view::styles::StyleCommand;

    const DT: f64 = 1000.0 / 60.0;

    fn open() -> evani_core::App {
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene("styles".into()));
        app.run_headless(8, DT);
        app
    }

    fn errors(app: &evani_core::App) -> u32 {
        app.world.res::<ErrorLog>().error_count()
    }

    /// The open style's materials all draw as ready models, one place each.
    #[test]
    fn the_style_row_draws_models() {
        let app = open();
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
        let n = app.world.res::<StylesData>().0.styles[0].materials.len();
        let st = app.world.res::<StylesScene>();
        assert_eq!(st.drawn.len(), n);
        assert!(st.drawn.iter().all(|(_, id)| id.is_some()), "{:?}", st.drawn);
        assert_eq!(st.slots.len(), n);
    }

    /// Guard (critic round 7: a Low proof was captured while three spheres
    /// were still stand-ins): the page is told how many stand-ins a frame
    /// drew, and once every model is in that count is 0.
    #[test]
    fn stand_ins_are_reported_until_the_models_are_in() {
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene("styles".into()));
        app.run_headless(1, DT);
        let shown = |app: &evani_core::App| app.world.res::<crate::scenes::StandIns>().shown;
        app.run_headless(1, DT);
        let first = shown(&app);
        assert!(first.is_some(), "the count reaches the page from the first frames");
        app.run_headless(12, DT);
        assert!(app.world.res::<StylesScene>().drawn.iter().all(|(_, id)| id.is_some()));
        app.run_headless(2, DT);
        assert_eq!(shown(&app), Some(0), "every model in: no stand-in left on the page");
        crate::scenes::StandIns::add(&app.world, 3);
        crate::scenes::StandIns::report(&mut app.world);
        assert_eq!(shown(&app), Some(3));
    }

    /// A param edit (as evani-live's `param_set` makes it) rebuilds the
    /// selected place's model in place: new bytes, same table size.
    #[test]
    fn an_edit_rebuilds_in_place() {
        let mut app = open();
        let len = app.world.res::<Models>().len();
        let id = app.world.res::<ParamRegistry>().id(styles::ROUGHNESS).unwrap();
        let mut last = app.world.res::<StylesScene>().slots[0].hash;
        for k in 0..20 {
            let v = 0.1 + k as f32 * 0.04;
            app.world.res_mut::<ParamRegistry>().set(id, ParamValue::F32(v)).unwrap();
            app.run_headless(2, DT);
            let st = app.world.res::<StylesScene>();
            assert_ne!(st.slots[0].hash, last, "edit {k} rebuilt the selected model");
            last = st.slots[0].hash;
            drop(st);
            assert_eq!(app.world.res::<Models>().len(), len, "edit {k} added no model");
        }
        assert_eq!(errors(&app), 0);
    }

    /// Switching the shape keeps the edit and still reuses the places.
    #[test]
    fn a_new_shape_keeps_the_edit() {
        let mut app = open();
        let len = app.world.res::<Models>().len();
        let id = app.world.res::<ParamRegistry>().id(styles::METALLIC).unwrap();
        app.world.res_mut::<ParamRegistry>().set(id, ParamValue::F32(0.77)).unwrap();
        app.run_headless(2, DT);
        app.world.res_mut::<StyleView>().push(StyleCommand::Shape("cube".into()));
        app.run_headless(4, DT);
        let view = app.world.res::<StyleView>().clone();
        let data = app.world.res::<StylesData>().0.clone();
        let (f, edited, _) = styles::finish(&view, &data, None, view.style, view.selected).unwrap();
        assert!(edited && f.metallic == 0.77, "{f:?}");
        let st = app.world.res::<StylesScene>();
        let r = data.styles[view.style].render.as_ref().filter(|r| r.shaped());
        assert_eq!(st.slots[0].key, BuildKey::for_style(Shape::Cube, &f, view.style, r, band_light(&app.world, r)));
        drop(st);
        assert_eq!(app.world.res::<Models>().len(), len);
        assert_eq!(errors(&app), 0);
    }

    /// The monkey and the teapot stream in and draw with each material.
    #[test]
    fn the_streamed_shapes_draw() {
        let mut app = open();
        for shape in STREAMED {
            app.world.res_mut::<StyleView>().push(StyleCommand::Shape(shape.id().into()));
            app.run_headless(10, DT);
            let st = app.world.res::<StylesScene>();
            assert!(!st.drawn.is_empty() && st.drawn.iter().all(|(_, id)| id.is_some()), "{shape:?}: {:?}", st.drawn);
            assert!(st.slots.iter().all(|s| s.key.shape == shape));
            let m = st.meshes.get(&shape).unwrap();
            assert!(m.indices.len() > 900 * 3, "{shape:?}: {} triangles", m.indices.len() / 3);
        }
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
    }

    /// Only the selected material when show-all is off; nothing but the
    /// ground for a style with nothing to show.
    #[test]
    fn one_or_none() {
        let mut app = open();
        app.world.res_mut::<StyleView>().push(StyleCommand::ShowAll(Some(false)));
        app.world.res_mut::<StyleView>().push(StyleCommand::Select(1));
        app.run_headless(3, DT);
        assert_eq!(app.world.res::<StylesScene>().drawn.iter().map(|d| d.0).collect::<Vec<_>>(), [1]);
        let missing = app.world.res::<StylesData>().0.styles.iter().position(|s| s.status == Status::Missing);
        if let Some(i) = missing {
            app.world.res_mut::<StyleView>().push(StyleCommand::Style(i));
            app.run_headless(3, DT);
            assert!(app.world.res::<StylesScene>().drawn.is_empty());
        }
    }

    /// A style with a shape look builds it into the models (cel: two light
    /// bands plus the ink outline, so three parts or more); a plain style
    /// draws the plain one-part model again.
    #[test]
    fn a_style_look_builds_into_the_shapes() {
        let mut app = open();
        app.world.res_mut::<StyleView>().push(StyleCommand::Style(2));
        app.run_headless(4, DT);
        let st = app.world.res::<StylesScene>();
        assert!(st.drawn.iter().all(|(_, id)| id.is_some()), "{:?}", st.drawn);
        assert!(st.slots[0].parts >= 3, "cel parts: {}", st.slots[0].parts);
        assert_ne!(st.slots[0].key.treat, 0);
        let cel = st.slots[0].hash;
        drop(st);
        app.world.res_mut::<StyleView>().push(StyleCommand::Style(4));
        app.run_headless(4, DT);
        let st = app.world.res::<StylesScene>();
        assert!(st.drawn.iter().all(|(_, id)| id.is_some()), "{:?}", st.drawn);
        assert_eq!((st.slots[0].parts, st.slots[0].key.treat), (1, 0));
        assert_ne!(st.slots[0].hash, cel);
        drop(st);
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
    }

    #[test]
    fn layouts_fit() {
        let cam = CameraPose { eye: [0.0, 1.0, 7.5], target: [0.0, 1.0, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 35.0 };
        // Every shape's screen box (CSS px) lies inside the open part.
        let inside = |at: &[[f32; 3]], s: f32, view: [f32; 2], free: [f32; 4]| {
            let k = 2.0 * 7.5 * (17.5_f32).to_radians().tan() / view[1];
            let r = RADIUS * s / k;
            at.iter().all(|p| {
                let (x, y) = (view[0] * 0.5 + p[0] / k, view[1] * 0.5 - (p[1] - 1.0) / k);
                x - r >= free[0] && x + r <= free[2] && y - r >= free[1] && y + r <= free[3]
            })
        };
        // Desktop: the part between the side panels and the bars takes five
        // in one even row, one size, no depth.
        let (view, free) = ([1440.0, 900.0], [313.0, 185.0, 1097.0, 705.0]);
        let (row, s) = layout(5, &cam, view, free);
        assert!(row.iter().all(|p| (p[1] - row[0][1]).abs() < 1e-5 && p[2].abs() < 1e-5));
        let gap = row[1][0] - row[0][0];
        assert!(gap > 0.0 && row.windows(2).all(|w| (w[1][0] - w[0][0] - gap).abs() < 1e-4));
        assert!(s < 1.0 && inside(&row, s, view, free));
        // Phone: a short band between the top rows and the bottom bars
        // gives a grid of bigger shapes (two rows of two).
        let (pv, pf) = ([390.0, 844.0], [0.0, 285.0, 390.0, 562.0]);
        let (grid, gs) = layout(4, &cam, pv, pf);
        assert!((grid[0][1] - grid[1][1]).abs() < 1e-5 && grid[2][1] < grid[0][1]);
        assert!(inside(&grid, gs, pv, pf));
        let in_one_row = (pf[2] - pf[0] - 2.0 * FREE_PAD) / 4.0;
        let k = 2.0 * 7.5 * (17.5_f32).to_radians().tan() / pv[1];
        assert!(gs * SPACING / k > in_one_row);
        // A camera turned a quarter round: the set still stands across the
        // view, not in a line going away from it.
        let side = CameraPose { eye: [7.5, 1.0, 0.0], ..cam };
        let (turned, _) = layout(5, &side, view, free);
        assert!(turned.iter().all(|p| p[0].abs() < 1e-4) && (turned[0][2] - turned[4][2]).abs() > 1.0);
        // No open part known: the whole view, one centred shape at most scale 1.
        let (one, s1) = layout(1, &cam, view, [0.0, 0.0, view[0], view[1]]);
        assert!(one[0][0].abs() < 1e-4 && s1 <= 1.0);
    }

    /// Grounded shapes rest on the floor (a sphere's and a teapot's lowest
    /// point at the gap) and keep their planned spot and size on screen.
    #[test]
    fn shapes_rest_on_the_floor() {
        // The styles room's home view: 12 degrees down, 7.5 m out.
        let el = 12.0_f32.to_radians();
        let cam = CameraPose { eye: [0.0, 1.0 + 7.5 * el.sin(), 7.5 * el.cos()], target: [0.0, 1.0, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 35.0 };
        for (view, free, n) in [([1440.0, 900.0], [313.0, 185.0, 1097.0, 705.0], 5), ([390.0, 844.0], [0.0, 285.0, 390.0, 562.0], 4)] {
            let (at, s) = layout(n, &cam, view, free);
            for bottom in [-1.0_f32, -0.6] {
                let placed = grounded(&at, s, &cam, bottom);
                for (&p, &(q, qs)) in at.iter().zip(&placed) {
                    assert!((q[1] + bottom * qs - GROUND_GAP).abs() < 1e-3, "{q:?} {qs}");
                    let (dp, dq) = (std::array::from_fn::<f32, 3, _>(|a| p[a] - cam.eye[a]), std::array::from_fn::<f32, 3, _>(|a| q[a] - cam.eye[a]));
                    let (lp, lq) = (dp.iter().map(|x| x * x).sum::<f32>().sqrt(), dq.iter().map(|x| x * x).sum::<f32>().sqrt());
                    assert!((0..3).all(|a| (dp[a] / lp - dq[a] / lq).abs() < 1e-4), "same ray");
                    assert!((qs / lq - s / lp).abs() < 1e-5, "same size on screen");
                }
            }
        }
    }

    /// Nothing in the viewer is compiled into the download: models stream.
    #[test]
    fn no_compiled_in_files() {
        let needle = concat!("include", "_bytes!");
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut dirs = vec![std::path::PathBuf::from(src)];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    assert!(!std::fs::read_to_string(&p).unwrap().contains(needle), "{}", p.display());
                }
            }
        }
    }
}

#[cfg(test)]
mod contact_tests {
    use super::*;

    fn ring(y: f32, r: f32) -> Vec<[f32; 3]> {
        (0..16).map(|i| (i as f32) * std::f32::consts::TAU / 16.0).map(|a| [r * a.cos(), y, r * a.sin()]).collect()
    }

    /// A shape that meets the floor at a point keeps the old disc; one on a
    /// flat base (a pouf) gets a disc wider than its foot, so it shows.
    #[test]
    fn a_flat_based_shape_casts_a_contact_shadow_wider_than_its_foot() {
        let mut ball = Mesh::default();
        for k in 0..=8 {
            let a = std::f32::consts::PI * (k as f32) / 8.0;
            ball.positions.extend(ring(-a.cos(), a.sin()));
        }
        assert_eq!(contact_radius(Some(&ball)), 0.9);
        assert_eq!(contact_radius(None), 0.9);
        let mut pouf = Mesh::default();
        pouf.positions.extend(ring(-0.45, 0.85));
        pouf.positions.extend(ring(0.45, 0.85));
        assert!(contact_radius(Some(&pouf)) > 0.85 * 1.2, "{}", contact_radius(Some(&pouf)));
    }
}
