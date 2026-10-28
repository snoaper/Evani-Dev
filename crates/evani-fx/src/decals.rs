//! Decals (feature `decals`): skid marks, scorch and bullet holes from a
//! pooled ring buffer, capped by the tier.
//!
//! * [`DecalPool`] (a world resource) is the pool: a game calls
//!   [`DecalPool::stamp`] where something happened. When it is full the
//!   oldest decal is overwritten. Decals fade over their `life` (module
//!   frames; 0 = until overwritten). A 2D game reads the pool with
//!   [`DecalPool::each`] and draws the marks itself, so decals cost it no
//!   render pass.
//! * In a 3D game (the graph has a depth target) one pass, `decals`, projects
//!   up to four decals per draw onto whatever the scene depth shows inside
//!   their reach and multiplies the mark into `hdr` before tonemap (marks
//!   darken or tint; they cannot lighten). The camera comes from
//!   `RenderWorld::camera`, so it is a full-canvas pass like the outline:
//!   split-screen views share the main camera's marks.
//!
//! The pool is capped by `fx.decals.max` and by the active tier (Lite 8, Low
//! 16, Medium 32, High 64, Ultra 96, VR 32); each four live decals cost one
//! depth-reading draw.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::num::clamp;
use evani_core::params::{BakeTarget, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_core::tiers::Tiers;
use evani_render::RenderWorld;
use evani_render::depth::inverse_distance;
use evani_render::graph::{GraphError, PassDesc, RenderGraph};
use evani_render::shaders::{FAR, NEAR};

use crate::{
    Blend, Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame, draw_effect,
};

pub static INFO: EffectInfo = EffectInfo {
    name: "decals",
    cost: 1,
    value: 0.4,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: false,
    description: "Skid marks, scorch and bullet holes in a capped pool",
};

pub const PASSES: &[&str] = &["decals"];

static SHADERS: [EffectShader; 1] = [EffectShader {
    name: "decals",
    pass: "decals",
    source: include_str!("../shaders/decals.wgsl"),
    slots: &[],
    blend: Blend::Multiply,
    uniform_words: 64,
    input: false,
    depth: true,
}];

/// Most decals any tier can hold.
pub const MAX_DECALS: usize = 128;
/// Decals one draw projects.
const BATCH: usize = 4;
const DEFAULT_CAP: usize = 32;

/// What kind of mark a decal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecalKind {
    /// A soft burnt patch.
    Scorch,
    /// A hard dark hole inside a faint scorch ring.
    Hole,
    /// A streaky stripe (a tyre mark between two points).
    Skid,
}

impl DecalKind {
    fn code(self) -> f32 {
        match self {
            DecalKind::Scorch => 0.0,
            DecalKind::Hole => 1.0,
            DecalKind::Skid => 2.0,
        }
    }
}

/// One mark. `a == b` is a round spot; otherwise a stripe from `a` to `b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decal {
    pub kind: DecalKind,
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// Reach in metres (half the width of a stripe).
    pub radius: f32,
    /// Linear rgb the surface is multiplied towards, and strength 0..1.
    pub color: [f32; 4],
    /// Module frames it lasts (0 = until overwritten).
    pub life: u32,
    pub age: u32,
}

impl Decal {
    /// A round mark at `at`.
    pub fn spot(kind: DecalKind, at: [f32; 3], radius: f32, color: [f32; 4], life: u32) -> Self {
        Self::skid(kind, at, at, radius, color, life)
    }

    /// A stripe from `from` to `to`.
    pub fn skid(kind: DecalKind, from: [f32; 3], to: [f32; 3], radius: f32, color: [f32; 4], life: u32) -> Self {
        Self {
            kind,
            a: from,
            b: to,
            radius,
            color,
            life,
            age: 0,
        }
    }
}

struct Pool {
    items: RefCell<Vec<Decal>>,
    /// Next slot to overwrite once full.
    head: Cell<usize>,
    cap: Cell<usize>,
    /// Fraction of `life` after which a decal fades (`fx.decals.fade_start`).
    fade_start: Cell<f32>,
    opacity: Cell<f32>,
    camera: Cell<CameraPose>,
}

/// The decal pool (world resource; cheap to clone, clones share the pool).
#[derive(Clone)]
pub struct DecalPool(Rc<Pool>);

impl Default for DecalPool {
    fn default() -> Self {
        Self(Rc::new(Pool {
            items: RefCell::new(Vec::new()),
            head: Cell::new(0),
            cap: Cell::new(DEFAULT_CAP),
            fade_start: Cell::new(0.6),
            opacity: Cell::new(1.0),
            camera: Cell::new(CameraPose::default()),
        }))
    }
}

impl DecalPool {
    /// Add a decal, overwriting the oldest when the pool is full.
    pub fn stamp(&self, d: Decal) {
        let cap = self.0.cap.get().max(1);
        let Ok(mut v) = self.0.items.try_borrow_mut() else {
            return;
        };
        if v.len() < cap {
            v.push(d);
        } else {
            let h = self.0.head.get() % v.len();
            v[h] = d;
            self.0.head.set((h + 1) % v.len());
        }
    }

    /// Remove every decal.
    pub fn clear(&self) {
        if let Ok(mut v) = self.0.items.try_borrow_mut() {
            v.clear();
        }
        self.0.head.set(0);
    }

    /// Decals held (some may have faded out).
    pub fn len(&self) -> usize {
        self.0.items.try_borrow().map_or(0, |v| v.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The capacity now in force (`fx.decals.max` and the tier).
    pub fn capacity(&self) -> usize {
        self.0.cap.get()
    }

    /// Visit every decal still visible with its current strength 0..1 (for a
    /// 2D game that draws its own marks).
    pub fn each(&self, mut f: impl FnMut(&Decal, f32)) {
        if let Ok(v) = self.0.items.try_borrow() {
            for d in v.iter() {
                let a = self.alpha(d);
                if a > 0.0 {
                    f(d, a);
                }
            }
        }
    }

    fn alpha(&self, d: &Decal) -> f32 {
        let mut a = d.color[3] * self.0.opacity.get();
        if d.life > 0 {
            if d.age >= d.life {
                return 0.0;
            }
            let t = d.age as f32 / d.life as f32;
            let s = self.0.fade_start.get().min(0.999);
            if t > s {
                a *= (1.0 - t) / (1.0 - s);
            }
        }
        clamp(a, 0.0, 1.0)
    }

    /// Decals worth a draw this frame.
    fn live(&self) -> usize {
        let mut n = 0;
        self.each(|_, _| n += 1);
        n
    }

    fn tick(&self) {
        if let Ok(mut v) = self.0.items.try_borrow_mut() {
            for d in v.iter_mut() {
                d.age = d.age.saturating_add(1);
            }
        }
    }

    fn set_cap(&self, cap: usize) {
        let cap = cap.clamp(1, MAX_DECALS);
        self.0.cap.set(cap);
        if let Ok(mut v) = self.0.items.try_borrow_mut() {
            if v.len() > cap {
                v.truncate(cap);
            }
            let len = v.len().max(1);
            self.0.head.set(self.0.head.get() % len);
        }
    }
}

/// Pool size the tier allows.
fn tier_cap(id: &str) -> usize {
    match id {
        "lite" => 8,
        "low" => 16,
        "medium" | "vr" => 32,
        "high" => 64,
        "ultra" => 96,
        _ => DEFAULT_CAP,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DecalParams {
    pub max: I32Param,
    pub fade_start: F32Param,
    pub opacity: F32Param,
    pub grit: F32Param,
    pub softness: F32Param,
}

#[derive(Default)]
pub struct Decals {
    pub params: Option<DecalParams>,
    pool: DecalPool,
}

fn f(name: &str, d: f32, min: f32, max: f32, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(0.01)
        .describe(desc)
        .owner("fx.decals")
        .bake(BakeTarget::Look)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    evani_render::math::normalize(v)
}

impl Effect for Decals {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(DecalParams {
            max: ctx.param_i32(
                ParamDef::i32("fx.decals.max", DEFAULT_CAP as i32)
                    .range(1.0, MAX_DECALS as f32)
                    .unit("decals")
                    .describe("Most decals kept (the tier can lower it; the oldest is overwritten).")
                    .owner("fx.decals")
                    .bake(BakeTarget::Look),
            )?,
            fade_start: ctx.param_f32(f(
                "fx.decals.fade_start",
                0.6,
                0.0,
                1.0,
                "Fraction of a decal's life after which it fades out.",
            ))?,
            opacity: ctx.param_f32(f("fx.decals.opacity", 1.0, 0.0, 1.0, "Strength of every mark."))?,
            grit: ctx.param_f32(f(
                "fx.decals.grit",
                0.3,
                0.0,
                1.0,
                "How ragged the edges are (0 = clean).",
            ))?,
            softness: ctx.param_f32(f(
                "fx.decals.softness",
                0.35,
                0.05,
                1.0,
                "Width of the fade at the edge of a mark.",
            ))?,
        });
        ctx.resource(self.pool.clone());
        Ok(())
    }

    /// Order 3: after the forward pass, before the outline (so inked edges
    /// stay crisp over marks) and the sprites. A 2D build has no depth target
    /// and so no pass; the pool is still there for the game to draw.
    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        if g.target("depth").is_none() {
            return Ok(());
        }
        g.add_pass(
            PassDesc::new("decals", "fx.decals")
                .reads(&["hdr", "depth"])
                .writes(&["hdr"])
                .order(3),
        )
    }

    fn frame(&self, world: &World) {
        if let Some(p) = self.params {
            let reg = world.res::<ParamRegistry>();
            let tier = world.try_res::<Tiers>().map_or(DEFAULT_CAP, |t| tier_cap(&t.active().id));
            self.pool.set_cap((reg.i32(p.max).max(1) as usize).min(tier));
            self.pool.0.fade_start.set(reg.f32(p.fade_start));
            self.pool.0.opacity.set(reg.f32(p.opacity));
        }
        if let Some(rw) = world.try_res::<RenderWorld>() {
            self.pool.0.camera.set(rw.camera);
        }
        self.pool.tick();
    }
}

impl PostEffect for Decals {
    fn name(&self) -> &'static str {
        INFO.name
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    /// One depth-reading draw per four live decals, multiplied into `hdr`.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        if pass != "decals" {
            return;
        }
        for batch in 0..self.pool.live().div_ceil(BATCH) {
            draw_effect(self, rec, reg, 0, None, "hdr", batch as u32);
        }
    }

    /// `frame.mip` is the batch: decals `4 * mip ..` of the visible ones.
    fn write_uniforms(&self, shader: &str, reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        if shader != "decals" || out.len() < 64 {
            return 0;
        }
        let cam = self.pool.0.camera.get();
        let fwd = norm([
            cam.target[0] - cam.eye[0],
            cam.target[1] - cam.eye[1],
            cam.target[2] - cam.eye[2],
        ]);
        let right = norm(cross(fwd, cam.up));
        let up = cross(right, fwd);
        let tan = (cam.fov_y_deg.to_radians() * 0.5).tan();
        let aspect = frame.target_size[0] / frame.target_size[1].max(1.0);
        let [inv0, inv1] = inverse_distance(NEAR, FAR); #[cfg(feature = "camera-range")] let [inv0, inv1] = { let _ = [inv0, inv1]; let (n, f) = evani_render::camera_range::current(); inverse_distance(n, f) };
        let p = self.params;
        let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
        let grit = v(p.map(|p| p.grit), 0.3);
        let soft = v(p.map(|p| p.softness), 0.35);
        let head = [
            [cam.eye[0], cam.eye[1], cam.eye[2], inv0],
            [fwd[0], fwd[1], fwd[2], inv1],
            [right[0] * tan * aspect, right[1] * tan * aspect, right[2] * tan * aspect, grit],
            [up[0] * tan, up[1] * tan, up[2] * tan, soft],
        ];
        for (i, row) in head.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(row);
        }
        out[16..64].fill(0.0);
        let skip = frame.mip as usize * BATCH;
        let mut seen = 0;
        let mut slot = 0;
        self.pool.each(|d, alpha| {
            if seen >= skip && slot < BATCH {
                let o = 16 + slot * 12;
                out[o..o + 3].copy_from_slice(&d.a);
                out[o + 3] = d.radius.max(0.0);
                out[o + 4..o + 7].copy_from_slice(&d.b);
                out[o + 7] = d.kind.code();
                out[o + 8..o + 11].copy_from_slice(&d.color[..3]);
                out[o + 11] = alpha;
                slot += 1;
            }
            seen += 1;
        });
        64
    }
}
