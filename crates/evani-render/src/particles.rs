//! The shared particle system (feature `particles`; CORE_REQUESTS.md plan C).
//!
//! Kits emit into `evani_core::particles::ParticleQueue` from their fixed
//! step. Each frame [`ParticleSystem::frame`] drains the queue and turns
//! every emit into a fixed-size [`Record`] written into a ring of
//! `cap` slots (slot `emitted % cap`; the oldest is recycled at the cap).
//! Nothing is simulated step by step: a particle's state is a closed-form
//! function of its age ([`eval`]), so it never drifts, stepped captures
//! repeat exactly, and toggling the effect off and on shows every particle
//! where it would be anyway.
//!
//! On the GPU (wasm), the `particles.sim` compute pass evaluates every slot
//! once per frame into an instance buffer (`particles_sim.wgsl`, the same
//! formula as [`eval`]); the sprites pass draws the screen-space groups
//! between the sprite layers, and the forward pass draws the world-space
//! group as camera-facing quads (depth-tested, no depth write). Blending is
//! premultiplied, so alpha and additive particles share one draw. Natively
//! (tests, the native bench) the same bookkeeping runs and [`eval`] is the
//! reference for what the GPU draws.
//!
//! Particles are visual only: nothing here writes back into the game.

use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamRegistry};
use evani_core::particles::{ParticleQueue, Space, Spawn};
use evani_core::tiers::Tiers;
use evani_core::time::Time;

use crate::graph::RenderGraph;

/// The compute pass the `particles` effect registers.
pub const SIM_PASS: &str = "particles.sim";
/// The buffer target it writes (the instances the draws read).
pub const BUFFER_TARGET: &str = "particles";
/// The game's own lower cap (registered by the evani-fx effect).
pub const GAME_CAP_PARAM: &str = "fx.particles.max";
/// Compute workgroup size.
pub const WORKGROUP: u32 = 64;
/// Group key of world-space particles.
pub const KEY_WORLD: u32 = 0;
/// Key the compute pass writes for a dead slot (matches no draw).
pub const KEY_DEAD: u32 = u32::MAX;
/// `Record::flags` bit: add light instead of covering.
pub const FLAG_ADDITIVE: u32 = 1;
/// Part of the life over which a particle fades out.
pub const FADE_TAIL: f32 = 0.25;

/// The draw group of a space: world-space particles are one group, screen
/// particles one per sprite layer (keys rise with the layer).
pub fn key_of(space: Space) -> u32 {
    match space {
        Space::World3d => KEY_WORLD,
        Space::Screen2d { layer } => 1 + (i32::from(layer) + 32768) as u32,
    }
}

/// The sprite layer of a screen group key.
pub fn layer_of(key: u32) -> Option<i16> {
    if key == KEY_WORLD || key == KEY_DEAD {
        return None;
    }
    i16::try_from(key as i64 - 1 - 32768).ok()
}

/// One emitted particle as the GPU reads it (96 bytes; `particles_sim.wgsl`
/// `Rec`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Record {
    pub pos: [f32; 3],
    /// Fixed tick it was emitted in (wrapping).
    pub spawn: u32,
    pub vel: [f32; 3],
    /// Seconds.
    pub life: f32,
    pub color: [f32; 4],
    /// Atlas uv rectangle (filled from the shape when uploaded).
    pub uv: [f32; 4],
    pub size: f32,
    pub size_end: f32,
    pub aspect: f32,
    pub spin: f32,
    pub gravity: f32,
    pub drag: f32,
    pub key: u32,
    /// [`FLAG_ADDITIVE`] in bit 0, the atlas cell in bits 16..32.
    pub flags: u32,
}

impl Record {
    pub fn from_spawn(s: &Spawn) -> Self {
        let e = &s.emit;
        Self {
            pos: e.pos,
            spawn: s.tick as u32,
            vel: e.vel,
            life: e.life.max(0.0),
            color: e.color,
            uv: [0.0; 4],
            size: e.size,
            size_end: e.size_end,
            aspect: e.aspect,
            spin: e.spin,
            gravity: e.gravity,
            drag: e.drag.max(0.0),
            key: key_of(e.space),
            flags: u32::from(e.additive) | (u32::from(e.shape) << 16),
        }
    }

    /// The atlas cell to draw.
    pub fn cell(&self) -> u16 {
        (self.flags >> 16) as u16
    }
}

/// One particle as the draws read it (64 bytes; the compute pass's output
/// and the draw pipelines' per-instance vertex buffer).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub center: [f32; 3],
    pub width: f32,
    pub height: f32,
    /// Radians (clockwise on screen).
    pub rot: f32,
    pub pad: f32,
    /// Draw group, or [`KEY_DEAD`].
    pub key: u32,
    pub uv: [f32; 4],
    /// Premultiplied colour; alpha 0 for additive particles.
    pub color: [f32; 4],
}

/// Render-time age in ticks of a particle spawned at `spawn`, `frac` of the
/// way from tick `now - 1` to `now` (the frame's interpolation alpha).
pub fn age_ticks(now: u32, frac: f32, spawn: u32) -> f32 {
    (now.wrapping_sub(spawn) as f32 + frac - 1.0).max(0.0)
}

/// Distance factors of the closed-form motion after `t` seconds with drag
/// `k`: velocity decays as `exp(-k t)`, so the start velocity carries the
/// particle `f = (1 - exp(-k t)) / k` and gravity `h = (t - f) / k` (`t`
/// and `t²/2` without drag). Below `k t` = 0.5 both use their power
/// series (eight terms, Horner), since `1 - exp(-k t)` and `t - f` lose
/// most of their f32 digits to cancellation there. `particles_sim.wgsl`
/// uses the same formula.
pub fn motion(t: f32, k: f32) -> (f32, f32) {
    let kt = k * t;
    if kt < 0.5 {
        let x = -kt;
        let f = 1.0
            + x * (1.0 / 2.0
                + x * (1.0 / 6.0 + x * (1.0 / 24.0 + x * (1.0 / 120.0 + x * (1.0 / 720.0 + x * (1.0 / 5040.0 + x / 40320.0))))));
        let h = 1.0 / 2.0
            + x * (1.0 / 6.0
                + x * (1.0 / 24.0
                    + x * (1.0 / 120.0 + x * (1.0 / 720.0 + x * (1.0 / 5040.0 + x * (1.0 / 40320.0 + x / 362880.0))))));
        (t * f, t * t * h)
    } else {
        let f = (1.0 - (-kt).exp()) / k;
        (f, (t - f) / k)
    }
}

/// The CPU reference of the compute pass: `r`'s instance at render age
/// `age` ticks of `dt` seconds (key [`KEY_DEAD`] once its life is over).
pub fn eval(r: &Record, age: f32, dt: f32) -> Instance {
    let t = age * dt;
    if t >= r.life {
        return Instance {
            key: KEY_DEAD,
            uv: r.uv,
            ..Instance::default()
        };
    }
    let (f, h) = motion(t, r.drag);
    // Gravity pulls down: +y on screen, -y in the world.
    let g = if r.key == KEY_WORLD { -r.gravity } else { r.gravity };
    let u = t / r.life;
    let w = r.size * (1.0 + (r.size_end - 1.0) * u);
    let a = r.color[3] * ((1.0 - u) / FADE_TAIL).clamp(0.0, 1.0);
    let additive = r.flags & FLAG_ADDITIVE != 0;
    Instance {
        center: [r.pos[0] + r.vel[0] * f, r.pos[1] + r.vel[1] * f + g * h, r.pos[2] + r.vel[2] * f],
        width: w,
        height: w * r.aspect,
        rot: r.spin * t,
        pad: 0.0,
        key: r.key,
        uv: r.uv,
        color: [r.color[0] * a, r.color[1] * a, r.color[2] * a, if additive { 0.0 } else { a }],
    }
}

/// A draw group with live particles: its key and the tick after which all
/// of them are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub key: u32,
    end: u32,
}

/// The compute pass's uniform (`particles_sim.wgsl` `Sim`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SimUniform {
    pub now: u32,
    pub frac: f32,
    pub dt: f32,
    pub count: u32,
}

/// World resource (inserted by the render module when the feature is on).
#[derive(Debug, Default)]
pub struct ParticleSystem {
    cap: u32,
    /// Bumped when the ring is remade (cap change): the GPU side remakes
    /// its buffers.
    generation: u32,
    emitted: u64,
    /// Per slot: spawn tick and life in ticks (live count, CPU side).
    slots: Vec<(u32, f32)>,
    /// This frame's new records in emit order (the GPU uploads them).
    staged: Vec<Record>,
    /// Groups with live particles, sorted by key.
    groups: Vec<Group>,
    scratch: Vec<Spawn>,
    sim: SimUniform,
    live: u32,
    on: bool,
    game_cap: Option<Option<F32Param>>,
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Slots in the ring.
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Bumped whenever the ring is remade.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Particles emitted since the ring was made.
    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    /// Slots the compute pass evaluates and each draw covers.
    pub fn count(&self) -> u32 {
        self.emitted.min(u64::from(self.cap)) as u32
    }

    /// Live particles this frame (what the GPU draws).
    pub fn live(&self) -> u32 {
        self.live
    }

    /// Is the effect on (its compute pass enabled)?
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Draw groups with live particles this frame, by key (the world group
    /// first, then screen layers bottom to top). Empty while off.
    pub fn groups(&self) -> &[Group] {
        if self.on && self.live > 0 { &self.groups } else { &[] }
    }

    /// Draw calls the particles add: one per screen layer, one per view for
    /// the world group.
    pub fn draw_calls(&self, views: u32) -> u32 {
        self.groups()
            .iter()
            .map(|g| if g.key == KEY_WORLD { views } else { 1 })
            .sum()
    }

    pub fn sim_uniform(&self) -> SimUniform {
        self.sim
    }

    /// This frame's new records (the GPU fills their uv and uploads them).
    pub fn staged_mut(&mut self) -> &mut [Record] {
        &mut self.staged
    }

    /// The writes that put this frame's records into the ring: at most two
    /// contiguous runs `(first slot, records)`. When a frame emits more
    /// than the cap, only the newest `cap` are written.
    pub fn uploads(&self) -> [(u32, &[Record]); 2] {
        let cap = u64::from(self.cap);
        let n = self.staged.len() as u64;
        if cap == 0 || n == 0 {
            return [(0, &[]), (0, &[])];
        }
        let keep = n.min(cap);
        let recs = &self.staged[(n - keep) as usize..];
        let first = ((self.emitted - keep) % cap) as usize;
        let run = (cap as usize - first).min(recs.len());
        [(first as u32, &recs[..run]), (0, &recs[run..])]
    }

    /// Remake the ring with `cap` slots (all particles dropped) if it has
    /// another size.
    pub fn set_cap(&mut self, cap: u32) {
        if cap == self.cap {
            return;
        }
        self.cap = cap;
        self.generation = self.generation.wrapping_add(1);
        self.emitted = 0;
        self.slots.clear();
        self.slots.resize(cap as usize, (0, 0.0));
        self.slots.shrink_to_fit();
        self.groups.clear();
        self.staged.clear();
    }

    /// Drop every particle (a level restart, a bench phase ending). The ring
    /// keeps its size; new emits fill it from the first slot again.
    pub fn clear(&mut self) {
        self.emitted = 0;
        self.groups.clear();
        self.staged.clear();
        self.live = 0;
        self.sim.count = 0;
    }

    /// Put one emit into the next slot (recycling the oldest at the cap).
    pub fn push(&mut self, s: &Spawn, dt: f32) {
        if self.cap == 0 {
            return;
        }
        let r = Record::from_spawn(s);
        let slot = (self.emitted % u64::from(self.cap)) as usize;
        let life_ticks = r.life / dt.max(1e-6);
        if let Some(m) = self.slots.get_mut(slot) {
            *m = (r.spawn, life_ticks);
        }
        // Live until render age reaches the life (one tick of render lag).
        let end = r.spawn.wrapping_add(life_ticks.ceil() as u32).wrapping_add(2);
        match self.groups.binary_search_by_key(&r.key, |g| g.key) {
            Ok(i) => {
                let g = &mut self.groups[i];
                if (end.wrapping_sub(g.end) as i32) > 0 {
                    g.end = end;
                }
            }
            Err(i) => self.groups.insert(i, Group { key: r.key, end }),
        }
        self.staged.push(r);
        self.emitted += 1;
    }

    /// Set the render time (`now` ticks, `frac` alpha) and count the live
    /// particles; groups whose particles are all gone are dropped.
    pub fn advance(&mut self, now: u32, frac: f32, dt: f32) {
        self.groups.retain(|g| (g.end.wrapping_sub(now) as i32) > 0);
        let count = self.count();
        self.sim = SimUniform { now, frac, dt, count };
        self.live = if self.groups.is_empty() {
            0
        } else {
            self.slots[..count as usize]
                .iter()
                .filter(|(spawn, life)| age_ticks(now, frac, *spawn) < *life)
                .count() as u32
        };
    }

    /// The cap this frame wants: the tier's `max_particles`, lowered by the
    /// game's `fx.particles.max` when it registers one.
    fn wanted_cap(&mut self, world: &World) -> u32 {
        let reg = world.res::<ParamRegistry>();
        let handle = *self.game_cap.get_or_insert_with(|| reg.f32_handle(GAME_CAP_PARAM));
        let tier = world.res::<Tiers>().active().max_particles;
        match handle {
            Some(h) => tier.min(reg.f32(h).max(0.0) as u32),
            None => tier,
        }
    }

    /// Once per frame, before drawing: follow the cap, drain the queue
    /// (emits are dropped while the effect is off), set the render time.
    pub fn frame(world: &mut World) {
        let Some(mut sys) = world.try_res_mut::<ParticleSystem>() else {
            return;
        };
        sys.staged.clear();
        let cap = sys.wanted_cap(world);
        sys.set_cap(cap);
        sys.on = world.res::<RenderGraph>().pass(SIM_PASS).is_some_and(|p| p.enabled);
        let mut pending = core::mem::take(&mut sys.scratch);
        // A kit's clear drops what is alive before this frame's emits land.
        let clear = world.res_mut::<ParticleQueue>().take_clear();
        world.res_mut::<ParticleQueue>().drain_into(&mut pending);
        if clear {
            sys.clear();
        }
        let (tick, alpha, dt) = {
            let t = world.res::<Time>();
            (t.tick as u32, t.alpha, t.fixed_dt)
        };
        if sys.on {
            for s in &pending {
                sys.push(s, dt);
            }
        }
        pending.clear();
        sys.scratch = pending;
        sys.advance(tick, alpha, dt);
    }
}

/// The compute shader (`particles.sim`).
pub const SIM_WGSL: &str = include_str!("../shaders/particles_sim.wgsl");
/// The draw shader (screen and world entry points).
pub const DRAW_WGSL: &str = include_str!("../shaders/particles_draw.wgsl");
/// Floats in the draw uniform (`particles_draw.wgsl` `U`).
pub const DRAW_WORDS: usize = 24;

/// The draw uniform: view-projection, eye and group key (world), canvas
/// size in CSS pixels (screen).
pub fn draw_words(vp: &crate::math::Mat4, eye: [f32; 3], key: u32, viewport: [f32; 2]) -> [f32; DRAW_WORDS] {
    let mut w = [0.0; DRAW_WORDS];
    w[..16].copy_from_slice(vp);
    w[16..19].copy_from_slice(&eye);
    w[19] = f32::from_bits(key);
    w[20] = viewport[0];
    w[21] = viewport[1];
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::ecs::{Stage, SystemDesc};
    use evani_core::particles::{Emit, Spread};
    use evani_core::rng::Pcg32;

    const DT: f32 = 1.0 / 60.0;

    fn spark(layer: i16) -> Emit {
        Emit::new([10.0, 20.0, 0.0], [30.0, -40.0, 0.0], [1.0, 0.5, 0.25, 1.0], 0.5, 6.0, 1, Space::Screen2d { layer })
    }

    fn spawn(tick: u64, e: Emit) -> Spawn {
        Spawn { tick, emit: e }
    }

    #[test]
    fn records_and_instances_match_the_wgsl_layout() {
        assert_eq!(size_of::<Record>(), 96);
        assert_eq!(size_of::<Instance>(), 64);
        assert_eq!(size_of::<SimUniform>(), 16);
        assert_eq!(DRAW_WORDS * 4, 96);
        let m = validate(SIM_WGSL);
        assert!(m.entry_points.iter().any(|e| e.name == "cs_main"));
        let m = validate(DRAW_WGSL);
        for ep in ["vs_screen", "vs_world", "fs_main"] {
            assert!(m.entry_points.iter().any(|e| e.name == ep), "{ep}");
        }
        assert!(SIM_WGSL.starts_with("// evani.shader.particles_sim"));
        assert!(DRAW_WGSL.starts_with("// evani.shader.particles_draw"));
        assert_eq!(layer_of(key_of(Space::Screen2d { layer: -7 })), Some(-7));
        assert_eq!(layer_of(key_of(Space::Screen2d { layer: i16::MAX })), Some(i16::MAX));
        assert_eq!(layer_of(KEY_WORLD), None);
        assert!(key_of(Space::Screen2d { layer: 2 }) > key_of(Space::Screen2d { layer: 1 }));
    }

    fn validate(src: &str) -> naga::Module {
        let m = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&m)
            .unwrap_or_else(|e| panic!("{e:?}"));
        m
    }

    /// The closed form against a fine numerical integration of the same
    /// motion (dv/dt = g - k v): nothing drifts however old a particle is.
    #[test]
    fn closed_form_matches_integration() {
        for (drag, gravity) in [(0.0, 30.0), (0.004, 9.0), (2.5, -12.0), (40.0, 5.0)] {
            let mut e = spark(0);
            e.drag = drag;
            e.gravity = gravity;
            e.life = 10.0;
            let r = Record::from_spawn(&spawn(0, e));
            let (mut p, mut v) = ([10.0f64, 20.0], [30.0f64, -40.0]);
            let sub = 200;
            let h = f64::from(DT) / f64::from(sub);
            for tick in 1..=240 {
                for _ in 0..sub {
                    for i in 0..2 {
                        let g = if i == 1 { f64::from(gravity) } else { 0.0 };
                        // Midpoint step.
                        let vm = v[i] + 0.5 * h * (g - f64::from(drag) * v[i]);
                        p[i] += h * vm;
                        v[i] += h * (g - f64::from(drag) * vm);
                    }
                }
                let got = eval(&r, tick as f32, DT);
                for (i, &want) in p.iter().enumerate() {
                    let err = (f64::from(got.center[i]) - want).abs();
                    assert!(err < 1e-2 + 1e-4 * want.abs(), "drag {drag} tick {tick} axis {i}: {} vs {}", got.center[i], want);
                }
            }
        }
    }

    #[test]
    fn particles_age_fade_and_die() {
        let mut e = spark(0);
        e.size_end = 0.0;
        e.spin = 2.0;
        let r = Record::from_spawn(&spawn(10, e));
        let start = eval(&r, 0.0, DT);
        assert_eq!(start.center, [10.0, 20.0, 0.0]);
        assert_eq!((start.width, start.height, start.color[3]), (6.0, 6.0, 1.0));
        let mid = eval(&r, 15.0, DT);
        assert!((mid.width - 3.0).abs() < 1e-4 && (mid.rot - 0.5).abs() < 1e-5);
        assert_eq!(mid.color[3], 1.0, "fades only in the last quarter");
        let late = eval(&r, 27.0, DT);
        assert!(late.color[3] > 0.0 && late.color[3] < 0.5 && late.key != KEY_DEAD);
        assert_eq!(eval(&r, 30.0, DT).key, KEY_DEAD);
        // Additive: colour adds, alpha covers nothing.
        let mut e = spark(0);
        e.additive = true;
        let add = eval(&Record::from_spawn(&spawn(0, e)), 1.0, DT);
        assert_eq!(add.color[3], 0.0);
        assert!(add.color[0] > 0.9);
        // World space falls the other way.
        let mut e = spark(0);
        e.space = Space::World3d;
        e.vel = [0.0; 3];
        e.gravity = 10.0;
        assert!(eval(&Record::from_spawn(&spawn(0, e)), 30.0, DT).center[1] < 20.0);
        // Render age: emitted in tick 9, drawn after tick 9 ran (now 10).
        assert_eq!(age_ticks(10, 0.0, 9), 0.0);
        assert_eq!(age_ticks(10, 0.5, 9), 0.5);
        assert_eq!(age_ticks(3, 0.0, u32::MAX), 3.0, "wraps");
    }

    #[test]
    fn oldest_is_recycled_at_the_cap() {
        let mut s = ParticleSystem::new();
        s.set_cap(8);
        for i in 0..10u64 {
            let mut e = spark(1);
            e.pos[0] = i as f32;
            s.push(&spawn(i, e), DT);
        }
        assert_eq!((s.emitted(), s.count()), (10, 8));
        // Only the newest eight are written: emits 2..8 into slots 2..8,
        // then 8 and 9 over the two oldest, in slots 0 and 1.
        let [(a, ra), (b, rb)] = s.uploads();
        assert_eq!((a, ra.len(), b, rb.len()), (2, 6, 0, 2));
        assert_eq!(ra.iter().map(|r| r.pos[0]).collect::<Vec<_>>(), [2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(rb.iter().map(|r| r.pos[0]).collect::<Vec<_>>(), [8.0, 9.0]);
        assert_eq!(s.slots[0].0, 8, "slot 0 holds emit 8 now");
        s.advance(10, 0.0, DT);
        assert_eq!(s.live(), 8, "never more than the cap");

        // Next frame: two more go into slots 2 and 3 (one run).
        s.staged.clear();
        for i in 10..12u64 {
            s.push(&spawn(i, spark(1)), DT);
        }
        let [(a, ra), (_, rb)] = s.uploads();
        assert_eq!((a, ra.len(), rb.len()), (2, 2, 0));
        // A wrap inside one frame splits into two runs.
        s.staged.clear();
        for i in 12..20u64 {
            s.push(&spawn(i, spark(1)), DT);
        }
        let [(a, ra), (b, rb)] = s.uploads();
        assert_eq!((a, ra.len(), b, rb.len()), (4, 4, 0, 4));
        // Particles die by age; their group goes once all are gone.
        s.advance(20 + 60, 0.0, DT);
        assert_eq!(s.live(), 0);
        assert!(s.groups.is_empty());
        // Clearing drops every particle but keeps the ring.
        s.push(&spawn(80, spark(1)), DT);
        s.clear();
        s.advance(81, 0.0, DT);
        assert_eq!((s.emitted(), s.count(), s.live(), s.cap()), (0, 0, 0, 8));
        assert!(s.groups.is_empty());
        // A cap change remakes the ring.
        let g = s.generation();
        s.set_cap(4);
        assert_eq!((s.cap(), s.count(), s.generation()), (4, 0, g + 1));
    }

    /// Kit-side emitter used by the App tests: bursts from its own stream.
    #[derive(Default)]
    struct KitRng(Option<Pcg32>);

    fn burster(w: &mut World) {
        let tick = w.res::<Time>().tick;
        if !tick.is_multiple_of(4) {
            return;
        }
        let mut rng = w.res_mut::<KitRng>().0.take().unwrap_or_else(|| Pcg32::new(7, 3));
        let spread = Spread { speed: [20.0, 80.0], life: [0.3, 0.9], size: [2.0, 6.0] };
        let mut base = spark((tick % 3) as i16);
        base.gravity = 40.0;
        base.drag = 1.5;
        w.res_mut::<ParticleQueue>().burst(&mut rng, 12, base, spread);
        let mut e = spark(0);
        e.space = Space::World3d;
        w.res_mut::<ParticleQueue>().emit(e);
        w.res_mut::<KitRng>().0 = Some(rng);
    }

    fn app(with_fx: bool) -> App {
        let mut a = App::new("t")
            .with_module(crate::RenderModule::new())
            .with_system(SystemDesc::new("burst", Stage::Update, burster));
        a.world.insert_resource(KitRng::default());
        if with_fx {
            let mut g = a.world.res_mut::<RenderGraph>();
            let _ = g.add_target(crate::graph::TargetDesc::new(BUFFER_TARGET, crate::graph::TargetKind::Buffer));
            let _ = g.add_pass(crate::graph::PassDesc::compute(SIM_PASS, "fx.particles").writes(&[BUFFER_TARGET]));
        }
        a
    }

    /// The same seed and the same frames give the same records and the same
    /// drawn particles, frame after frame (a stepped capture repeats).
    #[test]
    fn same_seed_same_particles() {
        let run = || {
            let mut a = app(true);
            assert!(a.world.res_mut::<Tiers>().set_active("low"));
            let mut frames = Vec::new();
            for f in 0..40 {
                a.frame(f as f64 * 1000.0 / 60.0 + if f % 5 == 0 { 7.0 } else { 0.0 });
                let s = a.world.res::<ParticleSystem>();
                let sim = s.sim_uniform();
                let drawn: Vec<Instance> = s.staged.iter().map(|r| eval(r, age_ticks(sim.now, sim.frac, r.spawn), sim.dt)).collect();
                frames.push((s.staged.clone(), drawn, s.live(), s.groups().to_vec()));
            }
            frames
        };
        let a = run();
        assert_eq!(a, run());
        assert!(a.iter().any(|f| f.2 > 0), "particles were drawn");
        assert!(a.iter().any(|f| f.3.len() >= 3), "screen layers and the world group");
    }

    /// A kit's `clear` drops every live particle before that frame's emits
    /// land, and an `emit_ago` starts part way through its life.
    #[test]
    fn a_kit_clear_drops_what_is_alive_and_emit_ago_ages() {
        let mut a = App::new("t").with_module(crate::RenderModule::new());
        {
            let mut g = a.world.res_mut::<RenderGraph>();
            let _ = g.add_target(crate::graph::TargetDesc::new(BUFFER_TARGET, crate::graph::TargetKind::Buffer));
            let _ = g.add_pass(crate::graph::PassDesc::compute(SIM_PASS, "fx.particles").writes(&[BUFFER_TARGET]));
        }
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        let mut e = spark(1);
        e.life = 1.0;
        for _ in 0..3 {
            a.world.res_mut::<ParticleQueue>().emit_ago(0, e);
        }
        a.frame(0.0);
        assert_eq!(a.world.res::<ParticleSystem>().live(), 3);
        {
            let mut q = a.world.res_mut::<ParticleQueue>();
            q.clear();
            q.emit_ago(0, e);
            q.emit_ago(30, e);
            // 90 ticks old at a life of 60: already over, never drawn.
            q.emit_ago(90, e);
        }
        a.frame(0.0);
        let s = a.world.res::<ParticleSystem>();
        assert_eq!(s.emitted(), 3, "the ring started again with this frame's emits");
        assert_eq!(s.live(), 2, "the three old ones went, the 90-tick-old one is dead");
    }

    #[test]
    fn cap_follows_the_tier() {
        // The tier files hold the catalog's caps as numbers.
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../../catalog/tiers.json")).unwrap();
        let tiers = Tiers::builtin().unwrap();
        let mut seen = 0;
        for t in catalog["tiers"].as_array().unwrap() {
            let id = t["id"].as_str().unwrap();
            let want: u32 = t["max_particles"].as_str().unwrap().replace(',', "").parse().unwrap();
            assert_eq!(tiers.get(id).unwrap().max_particles, want, "{id}");
            seen += 1;
        }
        assert_eq!(seen, 6);
        let caps: Vec<u32> = tiers.presets().iter().map(|p| p.max_particles).collect();
        assert_eq!(caps, [1_000, 2_000, 10_000, 50_000, 250_000, 5_000]);

        // The ring follows the active tier, and the game's own cap lowers it.
        let mut a = app(true);
        for (id, cap) in [("lite", 1_000), ("low", 2_000), ("ultra", 250_000), ("medium", 10_000)] {
            assert!(a.world.res_mut::<Tiers>().set_active(id));
            a.run_headless(1, 16.0);
            assert_eq!(a.world.res::<ParticleSystem>().cap(), cap, "{id}");
        }
        let h = a
            .world
            .res_mut::<ParamRegistry>()
            .register_f32(evani_core::params::ParamDef::f32(GAME_CAP_PARAM, 600.0).range(0.0, 250_000.0))
            .unwrap();
        a.world.res_mut::<ParticleSystem>().game_cap = None;
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<ParticleSystem>().cap(), 600);
        a.world.res_mut::<ParamRegistry>().set_f32(h, 50_000.0).unwrap();
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<ParticleSystem>().cap(), 10_000, "the tier still caps it");
    }

    #[test]
    fn effect_off_draws_and_keeps_nothing() {
        // Feature in the build but no compute pass (effect absent): the
        // queue is drained and nothing is kept or drawn.
        let mut a = app(false);
        a.run_headless(30, 1000.0 / 60.0);
        let s = a.world.res::<ParticleSystem>();
        assert!(!s.is_on());
        assert_eq!((s.emitted(), s.live(), s.draw_calls(1)), (0, 0, 0));
        assert!(a.world.res::<ParticleQueue>().is_attached());
        assert!(a.world.res::<ParticleQueue>().pending().is_empty());
        // With the pass: particles live and are counted as draws.
        let mut a = app(true);
        a.run_headless(30, 1000.0 / 60.0);
        let s = a.world.res::<ParticleSystem>();
        assert!(s.is_on() && s.live() > 0);
        assert_eq!(a.world.res::<evani_core::overlay::RenderStats>().particles, s.live());
        assert!(s.draw_calls(2) >= 3);
    }
}
