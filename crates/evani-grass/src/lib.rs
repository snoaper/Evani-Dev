//! evani-grass: fully procedural, cinematic grass and crops for 3D games.
//! No meshes or textures ship; every blade is made on the GPU each frame.
//!
//! * **Blades** are cubic Bezier strips (3 to 15 vertices, packed towards
//!   the tip), each a circular arc of its own length so it never stretches,
//!   with height, width, lean, curl, facing and colour from an integer hash
//!   of its tile and index (no random-number crate; the same field on every
//!   run and device).
//! * **Generation and culling** run in one compute pass (`grass`): the CPU
//!   lists the tiles in reach and in view ([`plan::plan_jobs`]), the GPU
//!   makes each candidate blade, culls it against the view and the LOD
//!   thinning and appends it to one of three LOD buckets, which are drawn
//!   with one indirect draw each inside the forward pass (evani-render's
//!   `world-draws`). Every frame is generated from scratch, so a fast camera
//!   never waits on tile builds: the cost is the same each frame.
//! * **LOD**: full density near the camera, then fewer, wider blades with
//!   the square of the distance, fading in and out smoothly, over a ground
//!   carpet in the sward's colour so far grass still reads as grass.
//! * **Wind**: a slow sway, gust waves travelling downwind and a per-blade
//!   flutter, one shared field; the bend angle grows with the blade's
//!   length, so the tip moves with the square of the height.
//! * **Trampling**: characters ([`Trampler`]) are stamped as capsules
//!   swept from last frame into a small toroidal window that follows the
//!   camera (just in front of the eye); it
//!   relaxes with a half-life (`material.grass.trample.half_life`, slower
//!   when wet), or keeps flat (`material.grass.trample.keep_flat`). Visual
//!   only: nothing feeds back into the simulation.
//! * **Rain**: `material.grass.wetness` darkens the blades, adds a tight
//!   wet sheen, makes them droop and slows their spring-back; bare soil
//!   turns to shining mud.
//! * **Kinds** ([`Kind`]): the long meadow, the short lawn, muddy trampled
//!   ground and uncut wheat or hay (a stalk with a nodding ear, a chevron
//!   of grains and awns drawn in the fragment stage), all parameter sets
//!   of the one system (`material.grass.<kind>.*`).
//!
//! Every value is a live param ([`params`]). Quality follows the device
//! tier or `material.grass.quality` ([`plan::TIERS`]).
//!
//! Turning it on: `[modules] grass = true` in evani.toml (the app's `grass`
//! feature, an optional dependency on this crate), add [`GrassModule`]
//! after the render module, then put patches in the [`Grass`] resource and
//! a [`Trampler`] for each character every frame.
//!
//! Prior art (ideas only, nothing copied): Sucker Punch's Ghost of Tsushima
//! grass talk (GDC 2021) for the Bezier blades, tiles and LOD; Jahrmann and
//! Wimmer (I3D 2017) for the spring-back state; the trample-map write-ups
//! for the relaxing window. See DECISIONS.md "evani-grass".

pub mod params;
pub mod plan;
pub mod wgsl;

#[cfg(target_arch = "wasm32")]
mod gpu;

#[cfg(test)]
mod tests;

use core::cell::RefCell;
use core::fmt::Write as _;
use std::rc::Rc;

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use evani_core::slots::ShaderSlots;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_render::RenderWorld;

pub use params::GrassParams;
use params::{g, k};
use plan::{PatchPlan, PlanStats, Shade, Sim, Stamp, TierCfg};

/// The fog slot's default (no fog), for builds without the slot.
pub const NO_FOG: &str = "fn slot_fog(distance: f32, height: f32) -> f32 {\n    return 0.0;\n}\n";

/// The grass kinds: parameter sets of the one blade system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Kind {
    /// Long meadow grass, knee high.
    Meadow = 0,
    /// Short mown lawn.
    Lawn = 1,
    /// Muddy, trampled ground with sparse broken grass.
    Mud = 2,
    /// Uncut wheat or hay: a stalk with a drooping ear.
    Wheat = 3,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Meadow, Kind::Lawn, Kind::Mud, Kind::Wheat];

    /// The param name part (`material.grass.<name>.*`).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Meadow => "meadow",
            Kind::Lawn => "lawn",
            Kind::Mud => "mud",
            Kind::Wheat => "wheat",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Meadow => "Long meadow",
            Kind::Lawn => "Short lawn",
            Kind::Mud => "Muddy trampled ground",
            Kind::Wheat => "Uncut wheat",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// A rectangle of one kind of grass on flat ground, or on a height grid
/// ([`Grass::add_patch_on`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    pub kind: Kind,
    /// Corner with the smallest x and z, and the opposite one (m).
    pub min: [f32; 2],
    pub max: [f32; 2],
    /// Height of the ground (m).
    pub ground_y: f32,
    /// The edge band over which the grass thins out to bare ground (m).
    pub feather: f32,
    /// Density against the kind's own (0..2).
    pub density: f32,
    /// How far its height grid rises over `ground_y` (m; 0 on flat ground):
    /// the tiles' culling boxes reach it. Set by [`Grass::add_patch_on`].
    pub relief: f32,
}

impl Patch {
    pub fn new(kind: Kind, min: [f32; 2], max: [f32; 2]) -> Self {
        Self { kind, min: [min[0].min(max[0]), min[1].min(max[1])], max: [min[0].max(max[0]), min[1].max(max[1])], ground_y: 0.0, feather: 1.5, density: 1.0, relief: 0.0 }
    }

    pub fn at(mut self, ground_y: f32) -> Self {
        self.ground_y = ground_y;
        self
    }

    pub fn feather(mut self, m: f32) -> Self {
        self.feather = m.max(0.0);
        self
    }

    pub fn density(mut self, d: f32) -> Self {
        self.density = evani_core::num::clamp(d, 0.0, 2.0);
        self
    }

    fn valid(&self) -> bool {
        let fin = self.min.iter().chain(&self.max).all(|v| v.is_finite()) && self.ground_y.is_finite();
        fin && self.max[0] > self.min[0] && self.max[1] > self.min[1]
    }

    fn contains(&self, x: f32, z: f32) -> bool {
        x >= self.min[0] && x <= self.max[0] && z >= self.min[1] && z <= self.max[1]
    }
}

/// A character pressing the grass this frame: its capsule from `a` to `b`
/// (feet to head, or any two points) with `radius`. `id` follows it from
/// frame to frame, so a fast mover leaves an unbroken path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trampler {
    pub id: u32,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
}

#[derive(Debug, Default)]
struct Field {
    patches: Vec<Patch>,
    /// Each patch's height grid (heights over its `ground_y`, n x n, row by
    /// row from its min corner, x fastest); empty for flat ground.
    grounds: Vec<Vec<f32>>,
    tramplers: Vec<Trampler>,
    centre: Option<[f32; 2]>,
    stats: PlanStats,
    level: usize,
}

/// The grass to draw (a world resource after the module's build; clones
/// share it). Patches stay until changed; tramplers are for one frame.
#[derive(Clone, Default)]
pub struct Grass(Rc<RefCell<Field>>);

impl Grass {
    /// Add a patch; `None` (not added) when it is empty, not finite or the
    /// field already has [`plan::MAX_PATCHES`].
    pub fn add_patch(&self, p: Patch) -> Option<usize> {
        let mut f = self.0.borrow_mut();
        if !p.valid() || f.patches.len() >= plan::MAX_PATCHES {
            return None;
        }
        f.patches.push(p);
        f.grounds.push(Vec::new());
        Some(f.patches.len() - 1)
    }

    /// Add a patch whose ground follows a height grid (CORE_REQUESTS
    /// `grass-heightfield`, procedural realism round 9): `n` x `n` heights
    /// (m, world y; row by row from the patch's min corner, x fastest) over
    /// its rectangle. Blades stand on the grid (bilinear between samples),
    /// so grass on a hill neither floats nor sinks; the patch's `ground_y`
    /// becomes the grid's lowest point and its culling boxes reach the top.
    /// `None` (not added) as [`Self::add_patch`], or when `n` is under 2,
    /// over 129 or `heights` is not `n * n` finite values. The ground carpet
    /// (the `ground` switch) stays flat at `ground_y`.
    pub fn add_patch_on(&self, mut p: Patch, n: usize, heights: &[f32]) -> Option<usize> {
        if !(2..=129).contains(&n) || heights.len() != n * n || !heights.iter().all(|h| h.is_finite()) {
            return None;
        }
        let lo = heights.iter().copied().fold(f32::MAX, f32::min);
        let hi = heights.iter().copied().fold(f32::MIN, f32::max);
        p.ground_y = lo;
        p.relief = hi - lo;
        let i = self.add_patch(p)?;
        self.0.borrow_mut().grounds[i] = heights.iter().map(|h| h - lo).collect();
        Some(i)
    }

    pub fn clear_patches(&self) {
        let mut f = self.0.borrow_mut();
        f.patches.clear();
        f.grounds.clear();
    }

    pub fn patches(&self) -> Vec<Patch> {
        self.0.borrow().patches.clone()
    }

    /// Press the grass with `t` this frame (up to [`plan::MAX_STAMPS`]).
    pub fn trample(&self, t: Trampler) {
        let mut f = self.0.borrow_mut();
        if f.tramplers.len() < plan::MAX_STAMPS && t.a.iter().chain(&t.b).all(|v| v.is_finite()) && t.radius > 0.0 {
            f.tramplers.push(t);
        }
    }

    /// Centre the trample window on (x, z) instead of just in front of the
    /// camera.
    pub fn set_trample_centre(&self, c: Option<[f32; 2]>) {
        self.0.borrow_mut().centre = c;
    }

    /// Last frame's plan: tiles, jobs, candidate blades, budget scale.
    pub fn stats(&self) -> PlanStats {
        self.0.borrow().stats
    }

    /// The quality level drawn (0 Lite .. 4 Ultra).
    pub fn level(&self) -> usize {
        self.0.borrow().level
    }
}

/// What the GPU side reads each frame (filled by the module's frame step).
pub(crate) struct Shared {
    pub on: bool,
    pub sim: Sim,
    pub jobs: Vec<[u32; 4]>,
    pub cfg: TierCfg,
    /// One camera per view (the main camera when there are no views).
    pub cameras: Vec<CameraPose>,
    pub shade: Shade,
    pub patches: u32,
    /// Every patch's height grid, one after another (the `ground` buffer),
    /// and its revision (the GPU uploads it when it changes).
    pub ground: Vec<f32>,
    pub ground_rev: u64,
    pub fog_wgsl: String,
    pub fog_rev: u64,
    #[cfg_attr(not(all(target_arch = "wasm32", feature = "capture")), allow(dead_code))]
    pub jitter: [f32; 2],
    /// A GPU build failure to report once (set by the GPU side).
    pub gpu_error: Option<String>,
}

impl Default for Shared {
    fn default() -> Self {
        Self { on: false, sim: Sim::default(), jobs: Vec::new(), cfg: plan::tier(2), cameras: Vec::new(), shade: Shade::default(), patches: 0, ground: Vec::new(), ground_rev: 0, fog_wgsl: NO_FOG.to_string(), fog_rev: 0, jitter: [0.0; 2], gpu_error: None }
    }
}

/// The module (`evani.module.grass`).
#[derive(Default)]
pub struct GrassModule {
    grass: Grass,
    params: Option<GrassParams>,
    shared: Rc<RefCell<Shared>>,
    last_t: Option<f64>,
    prev: Vec<(u32, [f32; 2])>,
    prev_origin: Option<([i32; 2], u32)>,
}

impl GrassModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared grass (also a world resource after `build`).
    pub fn grass(&self) -> Grass {
        self.grass.clone()
    }
}

fn kind_rows(p: &GrassParams, reg: &ParamRegistry, kind: Kind, density: f32) -> [[f32; 4]; 9] {
    let reach = plan::reach(p.kf(reg, kind, k::HEIGHT));
    let f = |i| p.kf(reg, kind, i);
    let c = |i| p.kc(reg, kind, i);
    let (root, tip, dry, soil) = (c(k::ROOT), c(k::TIP), c(k::DRY), c(k::SOIL));
    [
        [f(k::HEIGHT), f(k::HEIGHT_VAR), f(k::WIDTH), density],
        [f(k::LEAN), f(k::CURL), f(k::STIFFNESS), f(k::FLAT)],
        [root[0], root[1], root[2], f(k::VARIATION)],
        [tip[0], tip[1], tip[2], f(k::DRYNESS)],
        [dry[0], dry[1], dry[2], f(k::CLUMP)],
        [f(k::EAR_LENGTH), f(k::EAR_WIDTH), f(k::BARE), reach],
        [soil[0], soil[1], soil[2], f(k::MUD)],
        [f(k::HEADS), f(k::DROOP), f(k::TUFT), f(k::WEEDS)],
        [f(k::FINE), f(k::MOTTLE), f(k::SHEEN), 0.0],
    ]
}

impl EngineModule for GrassModule {
    fn id(&self) -> &'static str {
        "evani.module.grass"
    }

    fn label(&self) -> &'static str {
        "grass"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = GrassParams::register(&mut ctx.world.res_mut::<ParamRegistry>()).map_err(|e| PluginError(format!("material.grass: {e}")))?;
        self.params = Some(p);
        if ctx.world.try_res::<RenderWorld>().is_none() {
            return Err(PluginError("material.grass: no render world (add the render module first)".into()));
        }
        if ctx.world.try_res::<evani_render::world_draw::WorldDraws>().is_none() {
            ctx.resource(evani_render::world_draw::WorldDraws::default());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let d: Rc<dyn evani_render::world_draw::WorldDraw> = Rc::new(gpu::GrassDraw::new(self.shared.clone()));
            ctx.world.res_mut::<evani_render::world_draw::WorldDraws>().draws.push(d);
        }
        ctx.resource(self.grass.clone());
        Ok(())
    }

    fn frame(&mut self, world: &mut World) {
        let Some(p) = self.params.clone() else {
            return;
        };
        let tier = world.try_res::<Tiers>().map_or_else(|| "medium".to_string(), |t| t.active().id.clone());
        let now = world.try_res::<Time>().map_or(0.0, |t| t.render_seconds());
        let dt = self.last_t.map_or(0.0, |l| evani_core::num::clamp((now - l) as f32, 0.0, 0.1));
        self.last_t = Some(now);
        let mut field = self.grass.0.borrow_mut();
        let reg = world.res::<ParamRegistry>();
        let lvl = plan::level(p.f(&reg, g::QUALITY), &tier);
        field.level = lvl;
        let cfg = plan::tier(lvl).scaled(p.f(&reg, g::BLADES), p.f(&reg, g::SEGMENTS));
        let mut sh = self.shared.borrow_mut();
        if let Some(msg) = sh.gpu_error.take()
            && let Some(mut log) = world.try_res_mut::<evani_core::overlay::ErrorLog>()
        {
            log.error(msg);
        }
        let on = p.on(&reg, g::ENABLED) && !field.patches.is_empty();
        if !on {
            sh.on = false;
            field.tramplers.clear();
            field.stats = PlanStats::default();
            return;
        }
        // Cameras: one per view. A capture from its own pose swaps the
        // camera only when the frame is drawn: plan from that pose.
        let (cameras, aspect) = match world.try_res::<RenderWorld>() {
            Some(rw) => {
                let size = if rw.target_px[0] > 0 && rw.target_px[1] > 0 { rw.target_px } else { [16, 9] };
                if rw.views.is_empty() {
                    #[cfg(feature = "capture")]
                    let cam = world.try_res::<evani_core::capture::CaptureQueue>().and_then(|q| q.view_pose()).unwrap_or(rw.camera);
                    #[cfg(not(feature = "capture"))]
                    let cam = rw.camera;
                    (vec![cam], size[0] as f32 / size[1] as f32)
                } else {
                    let px = rw.views[0].pixels(size);
                    (rw.views.iter().map(|v| v.camera).collect::<Vec<_>>(), px[2] / px[3].max(1.0))
                }
            }
            None => (vec![CameraPose::default()], 16.0 / 9.0),
        };
        #[cfg(feature = "capture")]
        {
            sh.jitter = world.try_res::<evani_core::capture::CaptureQueue>().and_then(|q| q.jitter_px).unwrap_or([0.0; 2]);
        }
        let cam = cameras[0];
        let vp = evani_render::shaders::view_proj(&cam, aspect);
        let planes = plan::planes(&vp);
        let culled = cameras.len() == 1;
        let max_dist = cfg.max_dist * p.f(&reg, g::DISTANCE);
        let full_dist = cfg.full_dist * p.f(&reg, g::DISTANCE);
        let density = p.f(&reg, g::DENSITY) * cfg.density;
        let plans: Vec<PatchPlan> = field
            .patches
            .iter()
            .map(|pt| {
                let h = p.kf(&reg, pt.kind, k::HEIGHT);
                PatchPlan { patch: *pt, density: p.kf(&reg, pt.kind, k::DENSITY) * pt.density * density, max_height: pt.relief + h * (1.0 + p.kf(&reg, pt.kind, k::HEIGHT_VAR)), reach: plan::reach(h) }
            })
            .collect();
        let mut jobs = core::mem::take(&mut sh.jobs);
        let stats = plan::plan_jobs(&plans, cam.eye, culled.then_some(&planes), &cfg, max_dist, &mut jobs);
        field.stats = stats;
        sh.jobs = jobs;
        // The generation uniform.
        let mut sim = core::mem::take(&mut sh.sim);
        sim.words.iter_mut().for_each(|w| *w = 0);
        sim.put(plan::at::VIEW_PROJ, &vp);
        for (i, pl) in planes.iter().enumerate() {
            sim.put(plan::at::PLANES + i * 4, pl);
        }
        sim.put(plan::at::EYE, &[cam.eye[0], cam.eye[1], cam.eye[2], (now % 3600.0) as f32]);
        let wa = p.f(&reg, g::WIND_DIR).to_radians();
        sim.put(plan::at::WIND, &[wa.cos(), wa.sin(), p.f(&reg, g::WIND), p.f(&reg, g::GUST)]);
        sim.put(plan::at::WIND2, &[p.f(&reg, g::GUST_SPEED), p.f(&reg, g::GUST_SCALE), p.f(&reg, g::FLUTTER), p.f(&reg, g::SWAY)]);
        let wet = p.f(&reg, g::WETNESS);
        let scale = max_dist / cfg.max_dist.max(1e-3);
        sim.put(plan::at::LOD, &[full_dist, cfg.lod1 * scale, max_dist, plan::TILE]);
        sim.put(plan::at::LOD2, &[cfg.lod0 * scale, cfg.width_cap, wet, p.f(&reg, g::WET_DROOP)]);
        // The trample window follows the camera: it covers the ground just in
        // front of the eye (or the set centre), so a path stays pressed while
        // it is in view, however far the walker has gone.
        let n = cfg.trample_n;
        let cell = cfg.trample_cell;
        let half = n as f32 * cell * 0.5;
        let fwd = [cam.target[0] - cam.eye[0], cam.target[2] - cam.eye[2]];
        let fl = (fwd[0] * fwd[0] + fwd[1] * fwd[1]).sqrt();
        let ahead = if fl > 1e-3 { fl.min(half * 0.6) / fl } else { 0.0 };
        let centre = field.centre.unwrap_or([cam.eye[0] + fwd[0] * ahead, cam.eye[2] + fwd[1] * ahead]);
        let origin = plan::trample_origin(centre, n, cell);
        let prev = match self.prev_origin {
            Some((o, pn)) if pn == n => o,
            _ => origin,
        };
        self.prev_origin = Some((origin, n));
        let relax = plan::relax(dt, p.f(&reg, g::HALF_LIFE), wet, p.f(&reg, g::WET_SLOW), p.on(&reg, g::KEEP_FLAT));
        sim.put(plan::at::TRAMPLE, &[origin[0] as f32, origin[1] as f32, cell, n as f32]);
        sim.put(plan::at::TRAMPLE2, &[prev[0] as f32, prev[1] as f32, relax, 0.0]);
        let strength = p.f(&reg, g::TRAMPLE);
        let mut seen: Vec<(u32, [f32; 2])> = Vec::with_capacity(field.tramplers.len());
        let mut stamps = 0usize;
        for t in &field.tramplers {
            let c = [(t.a[0] + t.b[0]) * 0.5, (t.a[2] + t.b[2]) * 0.5];
            let low = t.a[1].min(t.b[1]) - t.radius;
            let ground = field.patches.iter().find(|pt| pt.contains(c[0], c[1])).map(|pt| pt.ground_y);
            seen.push((t.id, c));
            if ground.is_some_and(|gy| low > gy + 0.3) {
                continue;
            }
            let from = self.prev.iter().find(|(id, _)| *id == t.id).map_or(c, |(_, pc)| *pc);
            let mv = [c[0] - from[0], c[1] - from[1]];
            let ml = (mv[0] * mv[0] + mv[1] * mv[1]).sqrt();
            // A jump of more than 2 m is a teleport, not a stride.
            let from = if ml > 2.0 { c } else { from };
            let motion = if ml > 1e-3 && ml <= 2.0 { [mv[0] / ml, mv[1] / ml] } else { [0.0; 2] };
            let spread = ((t.a[0] - t.b[0]).powi(2) + (t.a[2] - t.b[2]).powi(2)).sqrt() * 0.5;
            sim.put_stamp(stamps, &Stamp { from, to: c, radius: t.radius + spread, strength, motion });
            stamps += 1;
        }
        self.prev = seen;
        field.tramplers.clear();
        sim.put(plan::at::STAMPS_N, &[stamps as f32, 0.0, if culled { 1.0 } else { 0.0 }, 0.0]);
        sim.put(plan::at::SCALES, &[stats.scales[0], stats.scales[1], stats.scales[2], 0.0]);
        let caps = cfg.caps;
        sim.put_u(plan::at::CAPS, &[caps[0], caps[1], caps[2], stats.jobs]);
        sim.put_u(plan::at::BASES, &[0, caps[0], caps[0] + caps[1], 0]);
        for kind in Kind::ALL {
            let dens = p.kf(&reg, kind, k::DENSITY);
            sim.put_kind(kind, &kind_rows(&p, &reg, kind, dens));
        }
        let mut ground = Vec::new();
        for (i, pt) in field.patches.iter().enumerate() {
            let g = field.grounds.get(i).map_or(&[][..], |g| &g[..]);
            let n = (g.len() as f32).sqrt().round() as usize;
            sim.put_patch(i, pt, ground.len(), if n * n == g.len() { n } else { 0 });
            ground.extend_from_slice(g);
        }
        if ground != sh.ground {
            sh.ground = ground;
            sh.ground_rev += 1;
        }
        sh.sim = sim;
        sh.patches = field.patches.len() as u32;
        let light = p.light(&reg);
        sh.shade = Shade {
            sun: light.dir,
            translucency: p.f(&reg, g::TRANSLUCENCY) * cfg.translucency,
            sun_color: light.color,
            specular: p.f(&reg, g::SPECULAR) * cfg.specular,
            ambient: light.ambient,
            wetness: wet,
            fog: light.fog,
            max_dist,
            round: p.f(&reg, g::ROUND),
            ao: p.f(&reg, g::AO),
            wet_darken: p.f(&reg, g::WET_DARKEN),
            wet_gloss: p.f(&reg, g::WET_GLOSS) * cfg.gloss,
            soil: p.color(&reg, g::SOIL),
            ground: p.on(&reg, g::GROUND),
            time: (now % 3600.0) as f32,
            full_dist,
        };
        sh.cfg = cfg;
        sh.cameras = cameras;
        drop(reg);
        // The game's fog, so grass fades into it like every other surface.
        if let Some(slots) = world.try_res::<ShaderSlots>() {
            let fog = slots.id("fog").and_then(|id| slots.wgsl_function(id)).unwrap_or_else(|| NO_FOG.to_string());
            if fog != sh.fog_wgsl {
                sh.fog_wgsl = fog;
                sh.fog_rev += 1;
            }
        }
        sh.on = stats.jobs > 0;
    }

    fn status(&self, _world: &World, out: &mut String) {
        let f = self.grass.0.borrow();
        let names = ["lite", "low", "medium", "high", "ultra"];
        let _ = write!(out, "{} · {} tiles · {} blades", names.get(f.level).copied().unwrap_or("?"), f.stats.tiles, f.stats.candidates);
    }
}
