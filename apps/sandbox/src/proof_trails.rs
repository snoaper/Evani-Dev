//! The ribbons and trails proof (sandbox feature `trails-proof`; never in
//! evani.toml or `default`, so the default and ship builds are unchanged): a
//! kart drifting at speed and a sword swinging, on a clear lot, drawn with the
//! `trails` effect or, for the picture beside it, with today's dense
//! particles along the same paths.
//!
//! The page address picks what shows:
//!
//! * `?tr=strip` (the effect), `dense` (the same paths as dense particles) or
//!   `off` (no marks at all);
//! * `&view=0..7`: 0 to 3 look at the kart (chase, high, side, front), 4 to 7
//!   at the sword (front, side, above, close); `&set=stress` flies the tier's
//!   whole cap of emitters instead, `&set=screen` a comet in screen space;
//! * `&hold=<tick>`: stop the sim clock at that fixed tick. The page address's
//!   `#h=<tick>` moves it on, so a harness can step frame by frame;
//! * `&tier=ultra|high|medium|low`: the quality tier, adaptive quality off;
//! * `&still=<tick>`: hold the camera where this view has it at that tick;
//!   `&actor=0` hides the kart and the sword; `&steady=1` keeps the marks' colour
//!   and width for their whole life and turns bloom off, so anything changing
//!   between two frames of the harness's temporal score is the strip's shape,
//!   not its fading or a glow;
//! * `&bench=1`: every 240 frames turn the strips' draws off or on (the compute
//!   pass keeps running, so no other pass changes) and report the median pass
//!   times of each half in the title, for the GPU cost.
//!
//! Poses are pure functions of the fixed tick, so a capture at a tick is the
//! same on every run. The page title carries the numbers (`tr ...`).

use core::fmt::Write;

use evani_core::camera::CameraPose;
use evani_core::capture::CaptureQueue;
use evani_core::curve::Curve;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::formulas::{FormulaJson, Formulas};
use evani_core::overlay::RenderStats;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::particles::{Emit, ParticleQueue, Space};
use evani_core::plugin::{LookData, PluginContext};
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_core::trails::{TrailQueue, TrailStyle};
use evani_render::atlas::cell;
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// How the marks are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marks {
    Off,
    Dense,
    Strip,
}

/// What flies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Set {
    Kart,
    Sword,
    Stress,
    Screen,
}

/// The proof's state (a world resource; tests insert one with the scene set).
#[derive(Debug, Clone)]
pub struct ProofTrails {
    pub marks: Marks,
    pub set: Set,
    pub view: u32,
    pub hold: Option<u64>,
    pub tier: Option<String>,
    pub bench: bool,
    pub still: Option<u64>,
    pub actor: bool,
    pub steady: bool,
    /// `&flame=0`: the kart without its exhaust flame (the marks alone).
    pub no_flame: bool,
    started: bool,
    frames: u64,
    bench_sim: Vec<f32>,
    bench_window: Vec<f32>,
    bench_with: Option<f32>,
    bench_diffs: Vec<f32>,
    report: String,
}

impl ProofTrails {
    pub fn new(marks: Marks, set: Set, view: u32) -> Self {
        Self {
            marks,
            set,
            view,
            hold: None,
            tier: None,
            bench: false,
            still: None,
            actor: true,
            steady: false,
            no_flame: false,
            started: false,
            frames: 0,
            bench_sim: Vec::new(),
            bench_window: Vec::new(),
            bench_with: None,
            bench_diffs: Vec::new(),
            report: String::new(),
        }
    }

    /// The scene a page address asks for.
    pub fn from_url(url: Option<&str>) -> Self {
        let q = |k: &str| url.and_then(|u| query_value(u, k));
        let marks = match q("tr") {
            Some("dense") => Marks::Dense,
            Some("off") => Marks::Off,
            _ => Marks::Strip,
        };
        let view = q("view").and_then(|v| v.parse().ok()).unwrap_or(0u32).min(7);
        let set = match q("set") {
            Some("stress") => Set::Stress,
            Some("screen") => Set::Screen,
            _ if view >= 4 => Set::Sword,
            _ => Set::Kart,
        };
        let mut p = Self::new(marks, set, view);
        p.hold = q("hold").and_then(|v| v.parse().ok());
        p.tier = q("tier").map(str::to_string);
        p.bench = q("bench").is_some();
        p.still = q("still").and_then(|v| v.parse().ok());
        p.actor = q("actor") != Some("0");
        p.steady = q("steady").is_some();
        p.no_flame = q("flame") == Some("0");
        p
    }
}

/// The value of `key` in a page address (`?a=1&key=v#...`).
fn query_value<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let q = url.split_once('?')?.1;
    let q = q.split('#').next().unwrap_or(q);
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then_some(v)
    })
}

/// The page address (wasm; `None` natively).
fn page_url() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.document()?.url().ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// `#h=<tick>` in a page address: a harness moving the hold on.
fn hash_hold(url: &str) -> Option<u64> {
    url.split_once("#h=")?.1.parse().ok()
}

fn set_title(title: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(d) = web_sys::window().and_then(|w| w.document()) {
        d.set_title(title);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = title;
}

pub const DT: f32 = 1.0 / 60.0;

// ---- the kart: a figure of eight taken at drift speed ----------------------

/// Ticks to the middle of the kart's drift round the east end of its eight.
#[cfg(test)]
const KART_HOLD: u64 = 175;

const A: f32 = 40.0;
const B: f32 = 18.0;
const W: f32 = 0.6;
/// Where the eight lies on the lot.
const KART_AT: [f32; 2] = [0.0, 0.0];

/// The kart at a time: position on the ground, velocity and heading with the
/// drift in it (the tail swings out in proportion to the turn).
#[derive(Debug, Clone, Copy)]
pub struct Kart {
    pub pos: [f32; 3],
    #[cfg_attr(not(test), allow(dead_code))]
    pub speed: f32,
    pub yaw: f32,
    pub drift: f32,
}

pub fn kart_at(t: f32) -> Kart {
    let (s, c) = (W * t).sin_cos();
    let (s2, c2) = (2.0 * W * t).sin_cos();
    let pos = [KART_AT[0] + A * s, 0.0, KART_AT[1] + B * s2];
    let vel = [A * W * c, 0.0, 2.0 * B * W * c2];
    let acc = [-A * W * W * s, 0.0, -4.0 * B * W * W * s2];
    let speed2 = vel[0] * vel[0] + vel[2] * vel[2];
    // Signed turn rate about y: positive turns towards +x from +z.
    let turn = (vel[2] * acc[0] - vel[0] * acc[2]) / speed2.max(1.0);
    let drift = (turn * 0.9).clamp(-0.55, 0.55);
    let yaw = vel[0].atan2(vel[2]) + drift;
    Kart { pos, speed: speed2.sqrt(), yaw, drift }
}

impl Kart {
    /// A point on the kart: `x` right, `z` forward, from its middle.
    pub fn local(&self, x: f32, y: f32, z: f32) -> [f32; 3] {
        let (s, c) = self.yaw.sin_cos();
        [self.pos[0] + x * c + z * s, y, self.pos[2] - x * s + z * c]
    }

    /// Half a tyre's width across the kart.
    fn axle(&self, half: f32) -> [f32; 3] {
        let (s, c) = self.yaw.sin_cos();
        [half * c, 0.0, -half * s]
    }
}

const MARK_LIFE: f32 = 2.6;
const MARK_HALF: f32 = 0.13;
const FLAME_LIFE: f32 = 0.5;
const REAR: [[f32; 3]; 2] = [[-0.62, 0.025, -0.78], [0.62, 0.025, -0.78]];
const PIPE: [f32; 3] = [0.0, 0.42, -1.05];

const MARK_STYLE: TrailStyle = TrailStyle {
    color: [0.015, 0.012, 0.012, 0.85],
    color_end: [0.02, 0.018, 0.018, 0.0],
    width: 0.0,
    life: MARK_LIFE,
    additive: false,
    space: Space::World3d,
};
const FLAME_STYLE: TrailStyle = TrailStyle {
    color: [5.0, 2.6, 0.7, 1.0],
    color_end: [1.6, 0.25, 0.05, 1.0],
    width: 0.3,
    life: FLAME_LIFE,
    additive: true,
    space: Space::World3d,
};

// ---- the sword: a fast sweep, a beat, and back ------------------------------

/// A tick in the middle of a swing (sim ticks; a swing takes [`SWING`] ticks
/// out of each [`SWING_EVERY`]).
#[cfg(test)]
const SWORD_HOLD: u64 = 320 + 13;
const SWING: f32 = 26.0;
const SWING_EVERY: u64 = 40;
const SHOULDER: [f32; 3] = [60.0, 1.45, 0.0];
const BLADE_IN: f32 = 0.55;
const BLADE_OUT: f32 = 1.95;

/// The blade's direction (a unit vector) at a time in ticks (fractions too).
pub fn blade_at(ticks: f32) -> [f32; 3] {
    let every = SWING_EVERY as f32;
    let n = (ticks / every).floor();
    let phase = ((ticks - n * every) / SWING).clamp(0.0, 1.0);
    // Smooth start and stop; every other swing comes back.
    let ease = phase * phase * (3.0 - 2.0 * phase);
    let sweep = if (n as i64) % 2 == 0 { ease } else { 1.0 - ease };
    let a = -1.9 + 3.8 * sweep;
    // The swing's plane leans over.
    let (s, c) = a.sin_cos();
    let v = [s, 0.25 + 0.3 * c, c];
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}

fn along(d: [f32; 3], r: f32) -> [f32; 3] {
    [SHOULDER[0] + d[0] * r, SHOULDER[1] + d[1] * r, SHOULDER[2] + d[2] * r]
}

const SWEEP_STYLE: TrailStyle = TrailStyle {
    color: [0.7, 1.5, 2.6, 0.9],
    color_end: [0.5, 0.15, 1.3, 0.0],
    width: 0.0,
    life: 0.3,
    additive: true,
    space: Space::World3d,
};
const TIP_STYLE: TrailStyle = TrailStyle {
    color: [3.2, 3.6, 4.4, 1.0],
    color_end: [0.8, 0.4, 2.0, 1.0],
    width: 0.09,
    life: 0.3,
    additive: true,
    space: Space::World3d,
};

/// `&steady=1`: one colour for the whole life.
fn held_colour(mut s: TrailStyle, steady: bool) -> TrailStyle {
    if steady {
        s.color[3] = s.color[3].max(0.6);
        s.color_end = s.color;
    }
    s
}

// ---- emitters -----------------------------------------------------------------

const ID_MARK: [u32; 2] = [1, 2];
const ID_FLAME: u32 = 3;
const ID_SWEEP: u32 = 4;
const ID_TIP: u32 = 5;
const ID_SCREEN: u32 = 6;
const ID_STRESS: u32 = 16;

/// Dense stand-ins per tick: sub-steps along the path and, for a ribbon, across
/// its span. Enough that at speed the particles overlap.
const DENSE_STEPS: u32 = 4;
const DENSE_ACROSS: u32 = 8;
/// `&steady=1`: how much longer everything lives.
const STEADY_LIFE: f32 = 3.0;

fn dense_emit(q: &mut ParticleQueue, pos: [f32; 3], color: [f32; 4], life: f32, size: f32, additive: bool, steady: bool) {
    let life = if steady { life * STEADY_LIFE } else { life };
    let mut e = Emit::new(pos, [0.0; 3], color, life, size, cell::GLOW, Space::World3d);
    e.additive = additive;
    e.size_end = if steady { 1.0 } else { 0.15 };
    q.emit(e);
}

/// The fixed step: lay this tick's points or particles.
fn step(world: &mut World) {
    let tick = world.res::<Time>().tick;
    let (set, marks, steady, no_flame) = {
        let p = world.res::<ProofTrails>();
        (p.set, p.marks, p.steady, p.no_flame)
    };
    let t = tick as f32 * DT;
    match (set, marks) {
        (_, Marks::Off) => {}
        (Set::Kart, Marks::Strip) => {
            let k = kart_at(t);
            let mut q = world.res_mut::<TrailQueue>();
            for (id, r) in ID_MARK.iter().zip(REAR) {
                q.style(*id, held_colour(MARK_STYLE, steady));
                if k.drift.abs() > 0.05 {
                    q.ribbon(*id, k.local(r[0], r[1], r[2]), k.axle(MARK_HALF));
                } else {
                    q.cut(*id);
                }
            }
            if !no_flame {
                q.style(ID_FLAME, held_colour(FLAME_STYLE, steady));
                q.point(ID_FLAME, k.local(PIPE[0], PIPE[1], PIPE[2]));
            }
        }
        (Set::Kart, Marks::Dense) => {
            let mut q = world.res_mut::<ParticleQueue>();
            for i in 0..DENSE_STEPS {
                let at = t - DT * i as f32 / DENSE_STEPS as f32;
                let k = kart_at(at);
                if k.drift.abs() > 0.05 {
                    for r in REAR {
                        dense_emit(&mut q, k.local(r[0], r[1], r[2]), [0.015, 0.012, 0.012, 0.85], MARK_LIFE, MARK_HALF * 2.0, false, steady);
                    }
                }
                dense_emit(&mut q, k.local(PIPE[0], PIPE[1], PIPE[2]), [5.0, 2.6, 0.7, 1.0], FLAME_LIFE, 0.3, true, steady);
            }
        }
        (Set::Sword, Marks::Strip) => {
            let d = blade_at(tick as f32);
            let r = (BLADE_IN + BLADE_OUT) * 0.5;
            let half = (BLADE_OUT - BLADE_IN) * 0.5;
            let mut q = world.res_mut::<TrailQueue>();
            q.style(ID_SWEEP, held_colour(SWEEP_STYLE, steady));
            q.ribbon(ID_SWEEP, along(d, r), [d[0] * half, d[1] * half, d[2] * half]);
            q.style(ID_TIP, held_colour(TIP_STYLE, steady));
            q.point(ID_TIP, along(d, BLADE_OUT));
        }
        (Set::Sword, Marks::Dense) => {
            let mut q = world.res_mut::<ParticleQueue>();
            for i in 0..DENSE_STEPS {
                let d = blade_at(tick as f32 - i as f32 / DENSE_STEPS as f32);
                for j in 0..DENSE_ACROSS {
                    let r = BLADE_IN + (BLADE_OUT - BLADE_IN) * j as f32 / (DENSE_ACROSS - 1) as f32;
                    dense_emit(&mut q, along(d, r), [0.7, 1.5, 2.6, 0.6], SWEEP_STYLE.life, 0.22, true, steady);
                }
            }
        }
        (Set::Stress, Marks::Strip) => {
            let cap = world.res::<evani_render::trails::TrailSystem>().cap();
            let mut q = world.res_mut::<TrailQueue>();
            for i in 0..cap {
                let id = ID_STRESS + i;
                q.style(id, TrailStyle::new([0.9, 0.7, 2.0, 0.8], 0.45, 1.0, Space::World3d));
                q.point(id, stress_at(i, cap, tick));
            }
        }
        (Set::Screen, Marks::Strip) => {
            let a = t * 2.4;
            let mut q = world.res_mut::<TrailQueue>();
            let mut style = TrailStyle::new([1.0, 0.8, 0.3, 1.0], 26.0, 0.8, Space::Screen2d { layer: 20 });
            style.color_end = [0.9, 0.2, 0.6, 0.0];
            q.style(ID_SCREEN, style);
            q.point(ID_SCREEN, [640.0 + 380.0 * a.cos(), 360.0 + 230.0 * (a * 2.0).sin(), 0.0]);
        }
        _ => {}
    }
}

/// An emitter of the stress set: a ring of circles spread over the lot.
pub fn stress_at(i: u32, cap: u32, tick: u64) -> [f32; 3] {
    let n = cap.max(1) as f32;
    let f = i as f32 / n;
    let side = n.sqrt().ceil();
    let (cx, cz) = ((i as f32 % side) * 9.0 - side * 4.5, (i as f32 / side).floor() * 9.0 - side * 4.5);
    let a = tick as f32 * DT * (3.0 + f) + f * 40.0;
    [cx + 3.6 * a.cos(), 1.2 + 0.7 * (a * 1.7).sin(), cz + 3.6 * a.sin()]
}

// ---- the picture ------------------------------------------------------------------

/// The held view: `view` at a sim time in seconds.
pub fn camera(view: u32, t: f32) -> CameraPose {
    let up = [0.0, 1.0, 0.0];
    if view < 4 {
        let k = kart_at(t);
        let (eye, target, fov) = match view {
            // Chase: behind and above, looking along the drift.
            0 => (k.local(0.0, 3.0, -8.5), k.local(0.0, 0.6, 3.0), 55.0),
            // High: over the corner.
            1 => (k.local(10.0, 22.0, -14.0), k.local(0.0, 0.0, -6.0), 50.0),
            // Side: low on the outside of the bend.
            2 => (k.local(-13.0, 1.5, -4.0), k.local(0.0, 0.7, -4.0), 55.0),
            // Front: looking back along the marks.
            _ => (k.local(-3.0, 2.2, 9.0), k.local(0.0, 0.5, -5.0), 55.0),
        };
        CameraPose { eye, target, up, fov_y_deg: fov }
    } else {
        let c = [SHOULDER[0], SHOULDER[1] - 0.2, SHOULDER[2]];
        let (eye, fov) = match view {
            4 => ([c[0], 1.9, c[2] + 5.2], 50.0),
            5 => ([c[0] + 5.4, 1.5, c[2] + 0.8], 50.0),
            6 => ([c[0] + 1.6, 6.0, c[2] + 2.8], 50.0),
            _ => ([c[0] - 1.8, 1.0, c[2] + 2.5], 60.0),
        };
        CameraPose { eye, target: c, up, fov_y_deg: fov }
    }
}

fn box_at(mesh: MeshId, c: [f32; 3], size: [f32; 3], yaw: f32, material: u32, color: [f32; 3]) -> MeshInstance {
    let mut m = MeshInstance::new(mesh, c);
    m.scale = size;
    m.rotation = quat_axis_angle([0.0, 1.0, 0.0], yaw);
    m.material = material;
    m.color = [color[0], color[1], color[2], 1.0];
    m
}

fn draw_kart(rw: &mut RenderWorld, k: &Kart, accent: u32) {
    let body = k.local(0.0, 0.42, 0.0);
    rw.draw_mesh(box_at(MeshId::BEVEL_CUBE, body, [1.3, 0.34, 2.1], k.yaw, accent, [0.85, 0.12, 0.1]));
    rw.draw_mesh(box_at(MeshId::SPHERE, k.local(0.0, 0.98, -0.1), [0.46, 0.46, 0.46], k.yaw, accent, [0.95, 0.85, 0.2]));
    for (x, z) in [(-0.72, -0.78), (0.72, -0.78), (-0.72, 0.72), (0.72, 0.72)] {
        let mut w = box_at(MeshId::CYLINDER, k.local(x, 0.26, z), [0.52, 0.3, 0.52], 0.0, accent, [0.06, 0.06, 0.07]);
        let turn = quat_axis_angle([0.0, 1.0, 0.0], k.yaw);
        let lie = quat_axis_angle([0.0, 0.0, 1.0], core::f32::consts::FRAC_PI_2);
        w.rotation = evani_render::math::quat_mul(turn, lie);
        rw.draw_mesh(w);
    }
}

/// A box from radius `from` to `to` along the blade.
fn blade_part(d: [f32; 3], from: f32, to: f32, thick: [f32; 2], material: u32, color: [f32; 3]) -> MeshInstance {
    // Turn +z onto the blade.
    let axis = [-d[1], d[0], 0.0];
    let l = (axis[0] * axis[0] + axis[1] * axis[1]).sqrt();
    let turn = if l > 1e-5 { quat_axis_angle([axis[0] / l, axis[1] / l, 0.0], d[2].clamp(-1.0, 1.0).acos()) } else { [0.0, 0.0, 0.0, 1.0] };
    let mut m = MeshInstance::new(MeshId::CUBE, along(d, (from + to) * 0.5));
    m.scale = [thick[0], thick[1], to - from];
    m.rotation = turn;
    m.material = material;
    m.color = [color[0], color[1], color[2], 1.0];
    m
}

fn draw_sword(rw: &mut RenderWorld, d: [f32; 3], accent: u32) {
    // A figure on a post, its arm and blade along `d`.
    let at = [SHOULDER[0], 0.75, SHOULDER[2]];
    rw.draw_mesh(box_at(MeshId::CYLINDER, at, [0.52, 1.5, 0.42], 0.0, accent, [0.2, 0.22, 0.3]));
    rw.draw_mesh(box_at(MeshId::SPHERE, [SHOULDER[0], 1.72, SHOULDER[2]], [0.34, 0.34, 0.34], 0.0, accent, [0.75, 0.6, 0.5]));
    rw.draw_mesh(blade_part(d, 0.0, BLADE_IN, [0.09, 0.09], accent, [0.3, 0.22, 0.16]));
    rw.draw_mesh(blade_part(d, BLADE_IN, BLADE_OUT, [0.07, 0.02], accent, [0.86, 0.9, 0.98]));
}

fn start(world: &mut World) {
    let mut p = world.res_mut::<ProofTrails>();
    if p.started {
        return;
    }
    p.started = true;
    let tier = p.tier.clone();
    let dense = p.marks == Marks::Dense;
    let steady = p.steady;
    drop(p);
    if steady {
        let mut f = world.res_mut::<Formulas>();
        if let Ok(c) = Curve::new(&[[0.0, 1.0], [1.0, 1.0]]) {
            for n in [evani_render::trails::CURVE_WIDTH, evani_render::trails::CURVE_ALPHA] {
                let _ = f.set_by_name(n, FormulaJson::Curve(c.clone()));
            }
        }
    }
    let mut reg = world.res_mut::<ParamRegistry>();
    if let Some(id) = reg.id("aq.enabled") {
        let _ = reg.set(id, ParamValue::Bool(false));
    }
    if steady {
        if let Some(id) = reg.id("fx.trails.fade") {
            let _ = reg.set(id, ParamValue::F32(0.02));
        }
        if let Some(id) = reg.id("fx.trails.life") {
            let _ = reg.set(id, ParamValue::F32(STEADY_LIFE));
        }
        if let Some(id) = reg.id("fx.bloom.enabled") {
            let _ = reg.set(id, ParamValue::Bool(false));
        }
    }
    if dense && let Some(id) = reg.id("fx.particles.max") {
        // The tier still caps it.
        let _ = reg.set(id, ParamValue::F32(250_000.0));
    }
    drop(reg);
    if let Some(t) = tier {
        world.res_mut::<Tiers>().set_active(&t);
    }
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// `&bench=1`: every 240 frames the draws go off or on (the first 90 of each
/// half settle). The draw's cost is the median over neighbouring windows of the
/// forward pass with the draws less without them, so slow drift cancels.
fn bench(world: &mut World) {
    let frame = world.res::<ProofTrails>().frames;
    let on = (frame / 240).is_multiple_of(2);
    let (sim, fwd) = {
        let s = world.res::<RenderStats>();
        let pass = |n: &str| s.passes.iter().find(|p| p.name == n).map_or(0.0, |p| p.gpu_ms);
        (pass("trails.sim"), pass("forward"))
    };
    let mut p = world.res_mut::<ProofTrails>();
    if frame % 240 > 90 {
        p.bench_sim.push(sim);
        p.bench_window.push(fwd);
    }
    if frame.is_multiple_of(240) && !p.bench_window.is_empty() {
        // The window that just ended ran with the opposite of `on`.
        let mut window = core::mem::take(&mut p.bench_window);
        let m = median(&mut window);
        if on {
            if let Some(with) = p.bench_with.take() {
                p.bench_diffs.push(with - m);
            }
        } else {
            p.bench_with = Some(m);
        }
        let mut diffs = p.bench_diffs.clone();
        let mut sims = p.bench_sim.clone();
        let (draw, sim) = (median(&mut diffs), median(&mut sims));
        p.report = format!("bench n={} sim={sim:.4} draw={draw:.4} fwd_off={m:.3}", diffs.len());
    }
    drop(p);
    world.res_mut::<evani_render::trails::TrailSystem>().draws_off = !on;
}

/// Where the strips' ends are this frame (heads and tails, world space): the
/// harness's temporal score leaves the ground they reach or leave out of it.
fn strip_ends(set: Set, ticks: f32, steady: bool) -> Vec<[f32; 3]> {
    let mult = if steady { STEADY_LIFE } else { 1.0 };
    let mut out = Vec::new();
    match set {
        Set::Kart => {
            let k = kart_at(ticks * DT);
            for r in REAR {
                out.push(k.local(r[0], r[1], r[2]));
            }
            out.push(k.local(PIPE[0], PIPE[1], PIPE[2]));
            let f = kart_at(ticks * DT - FLAME_LIFE * mult);
            out.push(f.local(PIPE[0], PIPE[1], PIPE[2]));
            let m = kart_at(ticks * DT - MARK_LIFE * mult);
            for r in REAR {
                out.push(m.local(r[0], r[1], r[2]));
            }
        }
        Set::Sword => {
            for at in [ticks, ticks - SWEEP_STYLE.life * mult / DT] {
                let d = blade_at(at);
                for i in 0..6 {
                    out.push(along(d, BLADE_IN + (BLADE_OUT - BLADE_IN) * i as f32 / 5.0));
                }
            }
        }
        Set::Stress | Set::Screen => {}
    }
    out
}

/// The lot: the kit's scene goes, a plain ground with lane marks takes its place.
fn draw_lot(rw: &mut RenderWorld, material: u32) {
    rw.meshes.clear();
    rw.decals.clear();
    rw.sprites.clear();
    let mut g = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
    g.scale = [2000.0, 1.0, 2000.0];
    g.material = material;
    g.color = [0.2, 0.2, 0.22, 1.0];
    rw.draw_mesh(g);
    for i in -12..=12i32 {
        for j in -6..=6i32 {
            let c = [i as f32 * 8.0, 0.005, j as f32 * 8.0];
            rw.draw_mesh(box_at(MeshId::CUBE, c, [0.12, 0.01, 1.6], 0.0, material, [0.5, 0.5, 0.48]));
        }
    }
    for j in -6..=6i32 {
        let c = [60.0 + j as f32 * 1.6, 0.005, 0.0];
        rw.draw_mesh(box_at(MeshId::CUBE, c, [0.06, 0.01, 20.0], 0.0, material, [0.4, 0.4, 0.4]));
    }
}

fn frame(world: &mut World) {
    start(world);
    let (marks, set, view, hold, frames, want_bench, still, actor, steady) = {
        let mut p = world.res_mut::<ProofTrails>();
        p.frames += 1;
        if let Some(h) = page_url().as_deref().and_then(hash_hold) {
            p.hold = Some(h);
        }
        (p.marks, p.set, p.view, p.hold, p.frames, p.bench, p.still, p.actor, p.steady)
    };
    if want_bench {
        bench(world);
    }
    // Hold the sim clock once it has reached the tick asked for.
    let (tick, alpha, dt) = {
        let t = world.res::<Time>();
        (t.tick, t.alpha, t.fixed_dt)
    };
    let held = hold.is_some_and(|h| tick >= h);
    world.res_mut::<CaptureQueue>().sim_time_override = held.then_some(tick as f64 * f64::from(dt));
    // Where an interpolated draw has the emitters: the point laid in the tick
    // just run is reached at the frame's alpha 1, the strip's head with it.
    let ticks = tick as f32 - 2.0 + alpha;
    let t = ticks * dt;
    let shot_time = still.map_or(t, |s| (s as f32 - 2.0) * dt);
    let look = world.res::<LookData>();
    let accent = RenderWorld::material(&look, "accent");
    drop(look);
    let mut ends = String::new();
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        draw_lot(&mut rw, accent);
        match set {
            Set::Kart => {
                rw.camera = camera(view, shot_time);
                if actor {
                    draw_kart(&mut rw, &kart_at(t), accent);
                }
            }
            Set::Sword => {
                rw.camera = camera(view, shot_time);
                if actor {
                    draw_sword(&mut rw, blade_at(ticks), accent);
                }
            }
            Set::Stress | Set::Screen => {
                rw.camera = CameraPose { eye: [0.0, 24.0, 42.0], target: [0.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 50.0 };
            }
        }
        if held {
            let [w, h] = rw.viewport_css;
            let (eye, target) = (rw.camera.eye, rw.camera.target);
            let fwd = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
            for p in strip_ends(set, ticks, steady) {
                // An end behind the camera has no place on screen.
                let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
                if d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2] <= 0.0 {
                    ends.push_str("-,-;");
                    continue;
                }
                let (x, y) = evani_kit_example::draw::ndc(&rw.camera, p, w / h.max(1.0));
                let _ = write!(ends, "{:.0},{:.0};", (x + 1.0) * 0.5 * w, (1.0 - y) * 0.5 * h);
            }
        }
    }
    let stats = world.res::<RenderStats>();
    let tier = world.res::<Tiers>().active().id.clone();
    let trails = world.res::<evani_render::trails::TrailSystem>();
    let report = world.res::<ProofTrails>().report.clone();
    let title = format!(
        "tr marks={marks:?} set={set:?} view={view} tier={tier} tick={tick} alpha={alpha:.3} held={} frames={frames} live={} cap={} steps={} draws={} tris={} ends={ends} {report}",
        u8::from(held),
        trails.live(),
        trails.cap(),
        trails.steps(),
        stats.draw_calls,
        stats.triangles,
    );
    set_title(&title);
}

/// Add the proof's systems and state.
pub fn install(ctx: &mut PluginContext) {
    ctx.resource(ProofTrails::from_url(page_url().as_deref()));
    if ctx.world.try_res::<CaptureQueue>().is_none() {
        ctx.resource(CaptureQueue::default());
    }
    ctx.system(SystemDesc::new("sandbox.proof_trails.step", Stage::Update, step));
    ctx.system(SystemDesc::new("sandbox.proof_trails.frame", Stage::Frame, frame));
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    fn run(p: ProofTrails, frames: u32) -> App {
        let mut app = crate::build_app();
        *app.world.res_mut::<ProofTrails>() = p;
        assert!(app.world.res_mut::<Tiers>().set_active("high"));
        app.run_headless(frames, 1000.0 / 60.0);
        app
    }

    #[test]
    fn addresses_pick_the_scene() {
        assert_eq!(hash_hold("http://x/?tr=strip&hold=5#h=212"), Some(212));
        assert_eq!(hash_hold("http://x/?tr=strip&hold=5"), None);
        let p = ProofTrails::from_url(Some("http://x/?tr=dense&view=5&hold=99&tier=low&bench=1"));
        assert_eq!((p.marks, p.set, p.view, p.hold, p.tier.as_deref(), p.bench), (Marks::Dense, Set::Sword, 5, Some(99), Some("low"), true));
        let p = ProofTrails::from_url(None);
        assert_eq!((p.marks, p.set, p.view, p.hold), (Marks::Strip, Set::Kart, 0, None));
        assert_eq!(ProofTrails::from_url(Some("http://x/?set=stress")).set, Set::Stress);
    }

    #[test]
    fn the_paths_are_at_speed() {
        let k = kart_at(KART_HOLD as f32 * DT);
        assert!(k.speed > 15.0, "{} m/s", k.speed);
        assert!(k.drift.abs() > 0.2, "mid-drift at the hold: {}", k.drift);
        // The blade's tip covers about a third of a metre a tick mid-swing.
        let (a, b) = (blade_at(SWORD_HOLD as f32), blade_at(SWORD_HOLD as f32 + 1.0));
        let tip = [(a[0] - b[0]) * BLADE_OUT, (a[1] - b[1]) * BLADE_OUT, (a[2] - b[2]) * BLADE_OUT];
        let step = (tip[0] * tip[0] + tip[1] * tip[1] + tip[2] * tip[2]).sqrt();
        assert!(step > 0.3, "{step} m a tick");
    }

    #[test]
    fn strip_set_lays_strips_and_no_particles() {
        let app = run(ProofTrails::new(Marks::Strip, Set::Kart, 0), 200);
        let t = app.world.res::<evani_render::trails::TrailSystem>();
        assert!(t.is_on());
        assert_eq!(t.live(), 3, "two marks and the flame");
        drop(t);
        assert_eq!(app.world.res::<evani_core::overlay::ErrorLog>().error_count(), 0);
    }

    #[test]
    fn stress_set_fills_the_tier_cap() {
        let app = run(ProofTrails::new(Marks::Strip, Set::Stress, 0), 80);
        let t = app.world.res::<evani_render::trails::TrailSystem>();
        assert_eq!((t.cap(), t.live()), (32, 32));
    }

    #[test]
    fn dense_set_lays_particles_and_no_strips() {
        let app = run(ProofTrails::new(Marks::Dense, Set::Sword, 4), 60);
        assert_eq!(app.world.res::<evani_render::trails::TrailSystem>().live(), 0);
        assert!(app.world.res::<evani_render::particles::ParticleSystem>().live() > 100);
    }

    #[test]
    fn the_hold_stops_the_clock_on_the_tick() {
        let mut p = ProofTrails::new(Marks::Strip, Set::Sword, 4);
        p.hold = Some(30);
        let app = run(p, 120);
        assert_eq!(app.world.res::<Time>().tick, 30);
    }

    /// The side view looks along the ground: the marks are still 1.5 to 7 px
    /// thick there, so a picture without them would be a drawing fault, not a
    /// thin strip. Geometry only: the harness's coverage check (strip against
    /// dense) is what catches marks that fail to draw.
    #[test]
    fn the_side_view_sees_the_marks_thick_enough_to_draw() {
        let hold = 175u64;
        let t_cam = (hold as f32 - 2.0) * DT;
        let cam = camera(2, t_cam);
        for back in [0u64, 5, 10, 20, 40, 80, 120] {
            let tick = hold - 2 - back;
            let k = kart_at(tick as f32 * DT);
            for r in REAR {
                let p = k.local(r[0], r[1], r[2]);
                let e = k.axle(MARK_HALF);
                let a = [p[0] - e[0], p[1], p[2] - e[2]];
                let b = [p[0] + e[0], p[1], p[2] + e[2]];
                let (x0, y0) = evani_kit_example::draw::ndc(&cam, a, 1600.0 / 900.0);
                let (x1, y1) = evani_kit_example::draw::ndc(&cam, b, 1600.0 / 900.0);
                let (xc, yc) = evani_kit_example::draw::ndc(&cam, p, 1600.0 / 900.0);
                let span = (((x1 - x0) * 800.0).powi(2) + ((y1 - y0) * 450.0).powi(2)).sqrt();
                assert!(span > 1.5, "{back} ticks back: {span} px");
                assert!(xc.abs() < 1.0 && yc.abs() < 1.0, "{back} ticks back: off screen");
            }
        }
    }}
