//! Ribbons and trails (feature `trails`, turned on by the evani-fx `trails`
//! effect; the plan's item R5). Ideas from Effekseer's ribbon and track nodes
//! and life curves, written from our own design; nothing of it is copied.
//!
//! Kits feed `evani_core::trails::TrailQueue` from their fixed step. Each
//! frame [`TrailSystem::frame`] drains the queue into one ring of
//! [`POINTS`] points per emitter. A point's look is a closed-form function of
//! its age, so nothing drifts, stepped captures repeat exactly and the game
//! is never touched. The rings are sized once for the most emitters any tier
//! allows ([`MAX_TRAILS`], about 400 KB of GPU memory), so a tier switch
//! never drops a strip; the tier only says how many are in use.
//!
//! On the GPU (wasm), the `trails.sim` compute pass ages every point into a
//! buffer that the draws read as neighbouring instances (`trails_draw.wgsl`);
//! no mesh is built on the CPU. The world group draws in the forward pass,
//! screen groups between the sprite layers. Natively (tests) the same
//! bookkeeping runs; `trails/tests.rs` holds the reference for what the GPU
//! writes and draws.
//!
//! A long-lived strip spreads its points: the ring always covers the life,
//! the newest point follows the emitter tick by tick, and only every few
//! ticks is it left behind as a kept point.

use evani_core::ecs::World;
use evani_core::formulas::{FormulaBody, FormulaId, Formulas};
use evani_core::params::{F32Param, ParamRegistry};
use evani_core::particles::Space;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_core::trails::{TrailOp, TrailQueue, TrailStyle};

use crate::graph::RenderGraph;

/// The compute pass the `trails` effect registers.
pub const SIM_PASS: &str = "trails.sim";
/// The buffer target it writes (what the draws read).
pub const BUFFER_TARGET: &str = "trails";
/// Points in each emitter's ring (also the compute workgroup size).
pub const POINTS: u32 = 64;
/// Emitter rings there are (the highest tier cap).
pub const MAX_TRAILS: u32 = 64;
/// Group key of world-space strips.
pub const KEY_WORLD: u32 = 0;
/// Key the compute pass writes where no segment starts (matches no draw).
pub const KEY_NONE: u32 = u32::MAX;
/// `Head::key` bit: add light instead of covering.
pub const KEY_ADDITIVE: u32 = 1 << 31;
/// Samples of each life curve in the compute uniform.
pub const LUT: usize = 16;

/// Param names (registered by the evani-fx effect; defaults hold without it).
pub const PARAM_MAX: &str = "fx.trails.max_trails";
pub const PARAM_WIDTH: &str = "fx.trails.width";
pub const PARAM_LIFE: &str = "fx.trails.life";
pub const PARAM_FADE: &str = "fx.trails.fade";
pub const PARAM_SOFT: &str = "fx.trails.soft";
pub const CURVE_WIDTH: &str = "fx.trails.width_curve";
pub const CURVE_ALPHA: &str = "fx.trails.alpha_curve";
/// Defaults of `fx.trails.fade` and `fx.trails.soft`.
pub const FADE: f32 = 0.4;
pub const SOFT: f32 = 0.35;

/// Most emitters on a tier (Lite shows as Low, VR as Medium).
pub fn tier_cap(tier: &str) -> u32 {
    match tier {
        "ultra" => 64,
        "high" => 32,
        "medium" | "vr" => 16,
        _ => 8,
    }
}

/// Quads per segment on a tier: Low draws the points as they are, the others
/// round the corners a fast blade cuts between two ticks.
pub fn tier_steps(tier: &str) -> u32 {
    match tier {
        "ultra" => 4,
        "high" => 3,
        "medium" | "vr" => 2,
        _ => 1,
    }
}

/// The draw group of a space: world strips are one group, screen strips one
/// per sprite layer (keys rise with the layer).
pub fn key_of(space: Space) -> u32 {
    match space {
        Space::World3d => KEY_WORLD,
        Space::Screen2d { layer } => 1 + (i32::from(layer) + 32768) as u32,
    }
}

/// The sprite layer of a screen group key.
pub fn layer_of(key: u32) -> Option<i16> {
    if key == KEY_WORLD {
        return None;
    }
    i16::try_from(i64::from(key) - 1 - 32768).ok()
}

/// One point as the GPU reads it (32 bytes; `trails_sim.wgsl` `Point`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Point {
    pub pos: [f32; 3],
    /// Fixed tick it was laid in (wrapping).
    pub tick: u32,
    /// Half span of a ribbon; zero faces the camera.
    pub edge: [f32; 3],
    /// Scale here; below zero the strip stops at its older neighbour.
    pub width: f32,
}

/// One emitter as the GPU reads it (64 bytes; `trails_sim.wgsl` `Head`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Head {
    pub prev: [f32; 3],
    pub head: u32,
    pub color: [f32; 4],
    pub color_end: [f32; 4],
    pub count: u32,
    pub life: f32,
    pub width: f32,
    pub key: u32,
}

/// One aged point as the draws read it (64 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vert {
    pub pos: [f32; 3],
    pub width: f32,
    pub tan: [f32; 3],
    pub pad: f32,
    pub edge: [f32; 3],
    pub key: u32,
    /// Premultiplied colour; alpha 0 for additive strips.
    pub color: [f32; 4],
}

/// The compute uniform (`trails_sim.wgsl` `Sim`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SimUniform {
    pub now: u32,
    pub frac: f32,
    pub dt: f32,
    pub trails: u32,
    pub fade: f32,
    pub width: f32,
    pub pad: [f32; 2],
    /// The width curve's samples, then the alpha curve's.
    pub luts: [f32; LUT * 2],
}

impl Default for SimUniform {
    fn default() -> Self {
        Self { now: 0, frac: 0.0, dt: 0.0, trails: 0, fade: FADE, width: 1.0, pad: [0.0; 2], luts: [1.0; LUT * 2] }
    }
}

/// An emitter's slot, CPU side (what the GPU reads too is in `heads`).
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    id: u32,
    used: bool,
    /// The next point starts a new strip.
    broken: bool,
    /// Ticks between kept points, so the ring covers the life.
    every: u32,
    /// Seconds, as the kit asked.
    life: f32,
    /// The newest point: it follows the emitter.
    newest: Point,
    /// The last point left behind.
    kept_tick: u32,
    kept_pos: [f32; 3],
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Closer than this to the last kept point, a point moves the newest one
/// instead of using up the ring (an emitter standing still).
const STILL: f32 = 1e-6;

/// World resource (inserted by the render module when the feature is on).
#[derive(Debug, Default)]
pub struct TrailSystem {
    cap: u32,
    slots: Vec<Slot>,
    /// This frame's point writes: absolute slot in the point buffer.
    staged: Vec<(u32, Point)>,
    /// One per slot; the GPU reads them up to the highest in use.
    heads: Vec<Head>,
    /// Draw groups with live strips, sorted (the world group first).
    groups: Vec<u32>,
    scratch: Vec<TrailOp>,
    sim: SimUniform,
    live: u32,
    steps: u32,
    soft: f32,
    on: bool,
    life_mult: f32,
    seen_curves: Option<u64>,
    handles: Option<Handles>,
    /// Age everything as usual but draw nothing: a bench compares a frame
    /// with and without the draws, every other pass the same.
    pub draws_off: bool,
    /// Styles for emitters with no slot yet, taken by their first point this
    /// frame (a style alone never holds a slot another emitter needs).
    pending_styles: Vec<(u32, TrailStyle)>,
    /// A GPU has taken this system's writes, and did so last frame.
    gpu_seen: bool,
    uploaded: bool,
    #[cfg(all(feature = "forward", not(feature = "trails-screen")))]
    warned_screen: bool,
}

#[derive(Debug, Clone, Copy)]
struct Handles {
    /// In the order of `PARAMS`.
    params: [Option<F32Param>; 5],
    curves: [Option<FormulaId>; 2],
}

const PARAMS: [&str; 5] = [PARAM_MAX, PARAM_WIDTH, PARAM_LIFE, PARAM_FADE, PARAM_SOFT];

impl TrailSystem {
    pub fn new() -> Self {
        Self { life_mult: 1.0, steps: 1, soft: SOFT, slots: vec![Slot::default(); MAX_TRAILS as usize], heads: vec![Head::default(); MAX_TRAILS as usize], ..Self::default() }
    }

    /// Emitters in use at most on this tier.
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Slots the compute pass reads (up to the highest used).
    pub fn trails(&self) -> u32 {
        self.sim.trails
    }

    /// Emitters with a live strip this frame.
    pub fn live(&self) -> u32 {
        self.live
    }

    /// Is the effect on (its compute pass enabled)?
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Draw groups with live strips this frame; empty while off.
    pub fn groups(&self) -> &[u32] {
        if self.on && self.live > 0 && !self.draws_off { &self.groups } else { &[] }
    }

    /// Quads per segment on this tier.
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// How much of the half width fades at the edges.
    pub fn soft(&self) -> f32 {
        self.soft
    }

    pub fn sim_uniform(&self) -> SimUniform {
        self.sim
    }

    pub fn heads(&self) -> &[Head] {
        self.heads.get(..self.sim.trails as usize).unwrap_or_default()
    }

    pub fn staged(&self) -> &[(u32, Point)] {
        &self.staged
    }

    /// Use at most `cap` emitters. Strips within the new cap stay as they
    /// are; the ones past it go (a tier switch drops only those).
    pub fn set_cap(&mut self, cap: u32) {
        self.cap = cap.min(MAX_TRAILS);
        for (s, h) in self.slots.iter_mut().zip(&mut self.heads).skip(self.cap as usize) {
            s.used = false;
            h.count = 0;
        }
    }

    /// The GPU took this frame's writes (the renderer calls it).
    pub fn mark_uploaded(&mut self) {
        self.gpu_seen = true;
        self.uploaded = true;
    }

    /// Drop every strip. The rings keep their size.
    pub fn clear(&mut self) {
        for (s, h) in self.slots.iter_mut().zip(&mut self.heads) {
            s.used = false;
            h.count = 0;
        }
        self.staged.clear();
    }

    fn set_style(&mut self, i: usize, style: &TrailStyle) {
        if let (Some(s), Some(h)) = (self.slots.get_mut(i), self.heads.get_mut(i)) {
            s.life = style.life;
            h.color = style.color;
            h.color_end = style.color_end;
            h.width = style.width;
            h.key = key_of(style.space) | if style.additive { KEY_ADDITIVE } else { 0 };
        }
    }

    /// The slot emitter `id` holds.
    fn slot_of(&self, id: u32) -> Option<usize> {
        self.slots.iter().take(self.cap as usize).position(|s| s.used && s.id == id)
    }

    /// The slot of emitter `id`, for a point: its own or a free one, styled
    /// from this frame's `style` call if it made one. With every slot taken a
    /// new emitter draws nothing until one frees up: recycling a busy one
    /// would cut two strips short every tick.
    fn slot_for(&mut self, id: u32) -> Option<usize> {
        if let Some(i) = self.slot_of(id) {
            return Some(i);
        }
        let i = self.slots.iter().take(self.cap as usize).position(|s| !s.used)?;
        self.slots[i] = Slot { id, used: true, every: 1, ..Slot::default() };
        self.heads[i].count = 0;
        let style = self.pending_styles.iter().find(|(p, _)| *p == id).map_or_else(TrailStyle::default, |(_, s)| *s);
        self.set_style(i, &style);
        Some(i)
    }

    /// Apply one instruction from the queue.
    pub fn apply(&mut self, op: &TrailOp, dt: f32) {
        match *op {
            TrailOp::Style { id, style } => match self.slot_of(id) {
                Some(i) => self.set_style(i, &style),
                None => match self.pending_styles.iter_mut().find(|(p, _)| *p == id) {
                    Some(p) => p.1 = style,
                    None => self.pending_styles.push((id, style)),
                },
            },
            TrailOp::Point { id, tick, pos, edge, width } => self.point(id, tick as u32, pos, edge, width, dt),
            TrailOp::Cut { id } => {
                if let Some(s) = self.slots.iter_mut().find(|s| s.used && s.id == id) {
                    s.broken = true;
                }
            }
            TrailOp::Clear => self.clear(),
        }
    }

    fn point(&mut self, id: u32, tick: u32, pos: [f32; 3], edge: [f32; 3], width: f32, dt: f32) {
        let life_mult = self.life_mult;
        let Some(i) = self.slot_for(id) else {
            return;
        };
        let at = i as u32 * POINTS;
        let (s, h) = (&mut self.slots[i], &mut self.heads[i]);
        let width = width.max(0.0);
        // The ring covers the life: a point kept every few ticks.
        s.every = ((s.life * life_mult / ((POINTS - 2) as f32 * dt.max(1e-6))).ceil() as u32).max(1);
        let mut p = Point { pos, tick, edge, width };
        // Below zero: the first point of a strip, which never joins the older
        // one (`Point::width`).
        let first = -width.max(1e-6);
        let leave = if h.count == 0 {
            s.broken = false;
            h.head = POINTS - 1;
            h.prev = pos;
            true
        } else if s.broken {
            s.broken = false;
            h.prev = pos;
            p.width = first;
            true
        } else {
            let from = if h.count == 1 { pos } else { s.kept_pos };
            let moved = dist2(s.newest.pos, from) > STILL;
            let waited = s.newest.tick.wrapping_sub(s.kept_tick) >= s.every;
            h.prev = s.newest.pos;
            let leave = moved && (waited || h.count == 1);
            if !leave && s.newest.width < 0.0 {
                p.width = first;
            }
            leave
        };
        if leave {
            s.kept_tick = s.newest.tick;
            s.kept_pos = s.newest.pos;
            h.head = (h.head + 1) % POINTS;
            h.count = (h.count + 1).min(POINTS);
        }
        s.newest = p;
        self.staged.push((at + h.head, p));
    }

    /// Set the render time (`now` ticks, `frac` alpha) and free the slots
    /// whose strips are over.
    pub fn advance(&mut self, now: u32, frac: f32, dt: f32) {
        self.groups.clear();
        self.live = 0;
        let span = (POINTS - 2) as f32 * dt.max(1e-6);
        let mut high = 0;
        for (i, (s, h)) in self.slots.iter_mut().zip(&mut self.heads).enumerate() {
            if !s.used {
                continue;
            }
            h.life = (s.life * self.life_mult).min(span * s.every.max(1) as f32).max(0.0);
            let age = (now.wrapping_sub(s.newest.tick) as f32 + frac - 1.0).max(0.0) * dt;
            if h.count == 0 || age >= h.life {
                s.used = false;
                h.count = 0;
                continue;
            }
            high = i as u32 + 1;
            if h.count > 1 {
                self.live += 1;
                let key = h.key & !KEY_ADDITIVE;
                if let Err(at) = self.groups.binary_search(&key) {
                    self.groups.insert(at, key);
                }
            }
        }
        self.sim.now = now;
        self.sim.frac = frac;
        self.sim.dt = dt;
        self.sim.trails = high;
        self.pending_styles.clear();
    }

    fn follow_params(&mut self, world: &World) {
        let reg = world.res::<ParamRegistry>();
        let formulas = world.res::<Formulas>();
        let h = *self.handles.get_or_insert_with(|| Handles {
            params: PARAMS.map(|n| reg.f32_handle(n)),
            curves: [CURVE_WIDTH, CURVE_ALPHA].map(|n| formulas.id(n)),
        });
        let read = |i: usize, d: f32| h.params[i].map_or(d, |p| reg.f32(p));
        self.sim.width = read(1, 1.0).max(0.0);
        self.life_mult = read(2, 1.0).max(0.0);
        self.sim.fade = read(3, FADE).clamp(0.0, 1.0);
        self.soft = read(4, SOFT).clamp(0.0, 1.0);
        let tiers = world.res::<Tiers>();
        let tier = tiers.active();
        self.steps = tier_steps(&tier.id);
        let cap = tier_cap(&tier.id).min(read(0, 64.0).max(0.0) as u32);
        drop(tiers);
        if self.seen_curves != Some(formulas.revision()) {
            self.seen_curves = Some(formulas.revision());
            for (at, c) in self.sim.luts.iter_mut().enumerate() {
                // A trail's curves are curves; an expression put there reads flat.
                let body = h.curves[at / LUT].and_then(|id| formulas.body(id));
                *c = match body {
                    Some(FormulaBody::Curve(c)) => c.eval((at % LUT) as f32 / (LUT - 1) as f32),
                    _ => 1.0,
                };
            }
        }
        drop(reg);
        drop(formulas);
        self.set_cap(cap);
    }

    /// Once per frame, before drawing: follow the params, drain the queue
    /// (instructions are dropped while the effect is off, and strips break
    /// there), set the render time.
    pub fn frame(world: &mut World) {
        let Some(mut sys) = world.try_res_mut::<TrailSystem>() else {
            return;
        };
        // Points written last frame that never reached the GPU (a device lost
        // or not ready): the rings there no longer match, so every strip
        // starts again rather than joining points the GPU never had.
        if sys.gpu_seen && !sys.uploaded && !sys.staged.is_empty() {
            sys.clear();
        }
        sys.uploaded = false;
        sys.staged.clear();
        sys.follow_params(world);
        sys.on = world.res::<RenderGraph>().pass(SIM_PASS).is_some_and(|p| p.enabled);
        let mut pending = core::mem::take(&mut sys.scratch);
        world.res_mut::<TrailQueue>().drain_into(&mut pending);
        let (tick, alpha, dt) = {
            let t = world.res::<Time>();
            (t.tick as u32, t.alpha, t.fixed_dt)
        };
        let on = sys.on;
        for op in &pending {
            if on || matches!(op, TrailOp::Style { .. } | TrailOp::Clear) {
                sys.apply(op, dt);
            }
        }
        if !on {
            for s in &mut sys.slots {
                s.broken = true;
            }
        }
        pending.clear();
        sys.scratch = pending;
        sys.advance(tick, alpha, dt);
        #[cfg(all(feature = "forward", not(feature = "trails-screen")))]
        if !sys.warned_screen && sys.groups().iter().any(|&k| k != KEY_WORLD) {
            sys.warned_screen = true;
            drop(sys);
            world.res_mut::<evani_core::overlay::ErrorLog>().error("trails: screen strips need evani-render's `trails-screen` in a 3D game".to_string());
        }
    }
}

/// A WGSL file without what the compiler never reads (comments, indentation,
/// line ends, spaces beside operators), keeping its first line, the
/// `evani.shader.*` marker, and the length of the result. Runs at compile time,
/// so the source stays readable and only the short form ships.
const fn squeezed<const N: usize>(src: &[u8]) -> ([u8; N], usize) {
    const fn word(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_'
    }
    let mut out = [0u8; N];
    let (mut i, mut n) = (0, 0);
    let mut marker = true;
    let mut space = false;
    while i < src.len() {
        let c = src[i];
        i += 1;
        if marker {
            if c != b'\r' {
                out[n] = c;
                n += 1;
            }
            marker = c != b'\n';
        } else if c == b'/' && i < src.len() && src[i] == b'/' {
            while i < src.len() && src[i] != b'\n' {
                i += 1;
            }
        } else if matches!(c, b' ' | b'\n' | b'\r') {
            space = true;
        } else {
            // A space only where two words would run together, or two signs
            // would read as one (`x - -y`).
            if space && ((word(out[n - 1]) && word(c)) || out[n - 1] == c) {
                out[n] = b' ';
                n += 1;
            }
            space = false;
            out[n] = c;
            n += 1;
        }
    }
    (out, n)
}

/// The first `M` bytes of `a`.
const fn head<const M: usize, const N: usize>(a: &[u8; N]) -> [u8; M] {
    let mut out = [0u8; M];
    let mut i = 0;
    while i < M {
        out[i] = a[i];
        i += 1;
    }
    out
}

/// Bytes as text (a compile error if they are not UTF-8).
const fn text(bytes: &'static [u8]) -> &'static str {
    match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => panic!("WGSL must be UTF-8"),
    }
}

const SIM_SRC: &[u8] = include_bytes!("../shaders/trails_sim.wgsl");
const DRAW_SRC: &[u8] = include_bytes!("../shaders/trails_draw.wgsl");
const SCREEN_SRC: &[u8] = include_bytes!("../shaders/trails_screen.wgsl");
const SIM_FULL: ([u8; SIM_SRC.len()], usize) = squeezed(SIM_SRC);
const DRAW_FULL: ([u8; DRAW_SRC.len()], usize) = squeezed(DRAW_SRC);
const SCREEN_FULL: ([u8; SCREEN_SRC.len()], usize) = squeezed(SCREEN_SRC);
static SIM_SHORT: [u8; SIM_FULL.1] = head(&SIM_FULL.0);
static DRAW_SHORT: [u8; DRAW_FULL.1] = head(&DRAW_FULL.0);
static SCREEN_SHORT: [u8; SCREEN_FULL.1] = head(&SCREEN_FULL.0);
/// The compute entry point (`trails.sim`).
pub const SIM_WGSL: &str = text(&SIM_SHORT);
/// The world draw and what the draws share.
pub const DRAW_WGSL: &str = text(&DRAW_SHORT);
/// The screen draw (joined after [`DRAW_WGSL`]).
pub const SCREEN_WGSL: &str = text(&SCREEN_SHORT);
/// Does this build draw screen-space strips? 2D builds do; 3D builds only
/// with the `trails-screen` feature, so a 3D game downloads none of it.
pub const SCREEN: bool = cfg!(any(not(feature = "forward"), feature = "trails-screen"));
/// Floats in the draw uniform (`trails_draw.wgsl` `U`).
pub const DRAW_WORDS: usize = 24;

/// The draw uniform.
pub fn draw_words(vp: &crate::math::Mat4, eye: [f32; 3], key: u32, viewport: [f32; 2], steps: u32, soft: f32) -> [f32; DRAW_WORDS] {
    let mut w = [0.0; DRAW_WORDS];
    w[..16].copy_from_slice(vp);
    w[16..19].copy_from_slice(&eye);
    w[19] = f32::from_bits(key);
    w[20] = viewport[0];
    w[21] = viewport[1];
    w[22] = steps as f32;
    w[23] = soft;
    w
}

#[cfg(test)]
mod tests;
