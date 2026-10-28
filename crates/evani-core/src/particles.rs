//! The particle queue: what kits call to emit sparks, sparkles, dust and
//! confetti (core request "shared particles", CORE_REQUESTS.md plan C).
//!
//! Always present and tiny. A kit pushes [`Emit`]s into the
//! [`ParticleQueue`] resource from its fixed-step systems, with any
//! randomness drawn from its own seeded `Pcg32` stream at emit time. The
//! GPU particle system (evani-render feature `particles`, turned on by the
//! evani-fx `particles` effect) attaches to the queue and drains it every
//! frame. Without that system in the build nothing is attached: `emit`
//! returns at once and the app clears the queue every frame, so kits call
//! it without a feature, the same way `AudioMixer` voices are no-ops without
//! a backend. Particles are visual only: nothing they do ever feeds back
//! into the game, so effects on or off never change it.

use crate::rng::Pcg32;

/// Where a particle lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// Screen space in CSS pixels from the top-left (y down), drawn with
    /// the sprites at this sprite layer.
    Screen2d { layer: i16 },
    /// World space (y up), drawn as camera-facing quads in the 3D pass,
    /// depth-tested without writing depth.
    World3d,
}

/// One particle to emit. Units follow the space: CSS pixels for
/// [`Space::Screen2d`], world units for [`Space::World3d`]; times in
/// seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emit {
    pub pos: [f32; 3],
    /// Units per second.
    pub vel: [f32; 3],
    /// Linear RGBA (HDR values above 1 glow under bloom).
    pub color: [f32; 4],
    /// Seconds alive.
    pub life: f32,
    /// Width in the space's units.
    pub size: f32,
    /// Atlas cell (a built-in shape or a sprite-sheet frame).
    pub shape: u16,
    pub space: Space,
    /// Downward pull in units per second squared (screen: +y, world: -y).
    pub gravity: f32,
    /// Velocity decay per second (0 = none): the velocity decays as
    /// `exp(-drag * t)` over the particle's age `t` (evaluated in closed
    /// form, so it never drifts).
    pub drag: f32,
    /// Spin in radians per second (clockwise on screen).
    pub spin: f32,
    /// Height as a multiple of the width (1 = square).
    pub aspect: f32,
    /// Size at the end of its life as a multiple of the start (1 = steady).
    pub size_end: f32,
    /// Adds light instead of covering what is behind (sparks, glints).
    pub additive: bool,
}

impl Emit {
    /// A particle with no gravity, drag or spin, square, steady size,
    /// alpha-blended.
    pub fn new(pos: [f32; 3], vel: [f32; 3], color: [f32; 4], life: f32, size: f32, shape: u16, space: Space) -> Self {
        Self {
            pos,
            vel,
            color,
            life,
            size,
            shape,
            space,
            gravity: 0.0,
            drag: 0.0,
            spin: 0.0,
            aspect: 1.0,
            size_end: 1.0,
            additive: false,
        }
    }
}

/// How a [`ParticleQueue::burst`] spreads its particles. Each particle
/// draws its speed, life and size from these ranges, uniformly, on the
/// kit's own `Pcg32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    /// Speed range (units per second) along a random direction (a circle in
    /// the screen plane for 2D, a sphere for 3D), added to the base velocity.
    pub speed: [f32; 2],
    /// Life range in seconds.
    pub life: [f32; 2],
    /// Size range.
    pub size: [f32; 2],
}

/// One queued emit and the fixed tick it was made in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    pub tick: u64,
    pub emit: Emit,
}

/// World resource: emits waiting for the particle system.
#[derive(Debug, Default)]
pub struct ParticleQueue {
    pending: Vec<Spawn>,
    /// The fixed tick running now (set by the app before each tick).
    tick: u64,
    /// A particle system drains this queue.
    attached: bool,
    /// The kit asked for every live particle to go (taken by the system).
    clear_asked: bool,
}

impl ParticleQueue {
    /// Queue one particle. A no-op when no particle system is in the build.
    #[inline]
    pub fn emit(&mut self, e: Emit) {
        if self.attached {
            self.pending.push(Spawn { tick: self.tick, emit: e });
        }
    }

    /// Queue one particle as if it had been emitted `ticks_ago` fixed ticks
    /// before the tick running now (a kit that replays several ticks in one
    /// frame, such as a scripted capture moment, stamps each replayed tick's
    /// emits with their own age). A no-op when no particle system is in the
    /// build.
    #[inline]
    pub fn emit_ago(&mut self, ticks_ago: u32, e: Emit) {
        if self.attached {
            self.pending.push(Spawn {
                tick: self.tick.wrapping_sub(u64::from(ticks_ago)),
                emit: e,
            });
        }
    }

    /// Drop every live particle and every emit still waiting (a level
    /// restart, a replayed moment). Emits made after this call in the same
    /// frame stay. A no-op when no particle system is in the build.
    pub fn clear(&mut self) {
        if self.attached {
            self.pending.clear();
            self.clear_asked = true;
        }
    }

    /// The particle system, each frame: did the kit ask to clear (and reset
    /// the ask)?
    pub fn take_clear(&mut self) -> bool {
        core::mem::take(&mut self.clear_asked)
    }

    /// Queue `n` particles from `base`, each sent in a random direction
    /// with speed, life and size drawn from `spread` on `rng` (the kit's
    /// own stream, so the game's numbers never depend on effects). The
    /// draws happen even when no system is attached, so a kit's stream
    /// advances the same either way.
    pub fn burst(&mut self, rng: &mut Pcg32, n: u32, base: Emit, spread: Spread) {
        for _ in 0..n {
            let speed = rng.range_f32(spread.speed[0], spread.speed[1]);
            let life = rng.range_f32(spread.life[0], spread.life[1]);
            let size = rng.range_f32(spread.size[0], spread.size[1]);
            let a = rng.range_f32(0.0, core::f32::consts::TAU);
            let dir = match base.space {
                Space::Screen2d { .. } => [a.cos(), a.sin(), 0.0],
                Space::World3d => {
                    let z = rng.range_f32(-1.0, 1.0);
                    let r = (1.0 - z * z).max(0.0).sqrt();
                    [r * a.cos(), r * a.sin(), z]
                }
            };
            let mut e = base;
            for (v, d) in e.vel.iter_mut().zip(dir) {
                *v += d * speed;
            }
            e.life = life;
            e.size = size;
            self.emit(e);
        }
    }

    /// Emits waiting (the particle system takes them each frame).
    pub fn pending(&self) -> &[Spawn] {
        &self.pending
    }

    /// Is a particle system draining this queue?
    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Called by the particle system when it is built.
    pub fn attach(&mut self) {
        self.attached = true;
    }

    /// Move the waiting emits into `out` (the particle system, each frame).
    pub fn drain_into(&mut self, out: &mut Vec<Spawn>) {
        out.append(&mut self.pending);
    }

    /// The app, before each fixed tick.
    pub(crate) fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// The app, at the end of every frame: whatever no system took is gone.
    pub(crate) fn end_frame(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use crate::ecs::{Stage, SystemDesc, World};

    fn spark(x: f32) -> Emit {
        Emit::new([x, 0.0, 0.0], [0.0, -10.0, 0.0], [1.0; 4], 0.5, 4.0, 1, Space::Screen2d { layer: 3 })
    }

    fn emitter(w: &mut World) {
        let x = w.res_mut::<Pcg32>().range_f32(0.0, 100.0);
        w.res_mut::<ParticleQueue>().emit(spark(x));
    }

    #[test]
    fn queue_is_a_no_op_without_the_system() {
        // No particle system: emits vanish, nothing grows, and the game's
        // own RNG and clock go exactly as with no emits at all.
        let run = |emit: bool| {
            let mut app = App::new("t");
            if emit {
                app = app.with_system(SystemDesc::new("emit", Stage::Update, emitter));
            } else {
                app = app.with_system(SystemDesc::new("emit", Stage::Update, |w: &mut World| {
                    let _ = w.res_mut::<Pcg32>().range_f32(0.0, 100.0);
                }));
            }
            app.run_headless(30, 1000.0 / 60.0);
            let q = app.world.res::<ParticleQueue>();
            assert!(!q.is_attached());
            assert!(q.pending().is_empty());
            assert_eq!(q.pending.capacity(), 0, "nothing was ever stored");
            let next = app.world.res_mut::<Pcg32>().next_u32();
            (next, app.world.res::<crate::time::Time>().tick)
        };
        assert_eq!(run(true), run(false));
        // burst draws the same numbers attached or not.
        let mut a = Pcg32::seeded(9);
        let mut b = Pcg32::seeded(9);
        let mut q = ParticleQueue::default();
        q.burst(&mut a, 5, spark(0.0), Spread { speed: [1.0, 2.0], life: [0.1, 0.2], size: [1.0, 2.0] });
        let mut q2 = ParticleQueue::default();
        q2.attach();
        q2.burst(&mut b, 5, spark(0.0), Spread { speed: [1.0, 2.0], life: [0.1, 0.2], size: [1.0, 2.0] });
        assert!(q.pending().is_empty());
        assert_eq!(q2.pending().len(), 5);
        assert_eq!(a.next_u32(), b.next_u32());
    }

    #[test]
    fn attached_queue_stamps_the_fixed_tick() {
        let mut app = App::new("t").with_system(SystemDesc::new("emit", Stage::Update, emitter));
        app.world.res_mut::<ParticleQueue>().attach();
        // One frame of three ticks: three emits stamped 0, 1, 2; the app
        // clears them at the end of the frame (no system took them).
        app.run_headless(1, 3.0 * 1000.0 / 60.0 + 0.5);
        assert!(app.world.res::<ParticleQueue>().pending().is_empty());
        let mut got = Vec::new();
        app.world.res_mut::<ParticleQueue>().set_tick(7);
        app.world.res_mut::<ParticleQueue>().emit(spark(1.0));
        app.world.res_mut::<ParticleQueue>().drain_into(&mut got);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tick, 7);
        assert!(app.world.res::<ParticleQueue>().pending().is_empty());
    }

    #[test]
    fn emit_ago_stamps_an_earlier_tick_and_clear_drops_what_waits() {
        let mut q = ParticleQueue::default();
        q.emit_ago(2, spark(0.0));
        q.clear();
        assert!(q.pending().is_empty() && !q.take_clear(), "no system: nothing happens");
        q.attach();
        q.set_tick(10);
        q.emit(spark(1.0));
        q.clear();
        assert!(q.pending().is_empty(), "the waiting emit went");
        q.emit_ago(3, spark(2.0));
        q.emit_ago(0, spark(3.0));
        let ticks: Vec<u64> = q.pending().iter().map(|s| s.tick).collect();
        assert_eq!(ticks, [7, 10]);
        assert!(q.take_clear());
        assert!(!q.take_clear(), "the ask is taken once");
        q.set_tick(1);
        q.emit_ago(3, spark(4.0));
        assert_eq!(q.pending()[2].tick, 1u64.wrapping_sub(3), "wraps like the stored tick");
    }
}
