//! The trail queue (feature `trails`): what kits call to leave ribbons and
//! trails behind vehicles, blades and projectiles.
//!
//! A kit gives each emitter its own number and, from its fixed step, feeds
//! it the emitter's position once per tick:
//!
//! * [`TrailQueue::point`]: a trail, a strip that always faces the camera.
//! * [`TrailQueue::ribbon`]: a ribbon, a strip with a fixed span the kit
//!   gives (half of it to each side), for a blade's sweep or a tyre mark flat
//!   on the ground.
//! * [`TrailQueue::cut`]: lift the pen; the next point starts a new strip.
//!
//! Evani-render's trail system (feature `trails`, turned on by the evani-fx
//! `trails` effect) attaches to the queue and drains it every frame. Without
//! that system nothing is attached: the calls return at once and the app
//! clears the queue every frame. Trails are visual only: nothing they do ever
//! feeds back into the game, so effects on or off never change it.
//!
//! A game that does not list the effect compiles none of this.

pub use crate::particles::Space;

/// How an emitter's strip looks. Units follow the space: CSS pixels for
/// [`Space::Screen2d`], world units for [`Space::World3d`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailStyle {
    /// Linear RGBA at the emitter (HDR values above 1 glow under bloom).
    pub color: [f32; 4],
    /// Linear RGBA at the end of its life.
    pub color_end: [f32; 4],
    /// Full width of a [`TrailQueue::point`] strip (a ribbon's span comes
    /// from its points).
    pub width: f32,
    /// Seconds a point lives.
    pub life: f32,
    /// Adds light instead of covering what is behind.
    pub additive: bool,
    pub space: Space,
}

impl TrailStyle {
    /// One colour for its whole life, alpha-blended.
    pub fn new(color: [f32; 4], width: f32, life: f32, space: Space) -> Self {
        Self { color, color_end: color, width, life, additive: false, space }
    }
}

impl Default for TrailStyle {
    fn default() -> Self {
        Self::new([1.0; 4], 0.25, 0.5, Space::World3d)
    }
}

/// One queued instruction, kept in the order the kit made them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrailOp {
    Style { id: u32, style: TrailStyle },
    /// `edge` zero: faces the camera. `width` scales the strip at this point.
    Point { id: u32, tick: u64, pos: [f32; 3], edge: [f32; 3], width: f32 },
    Cut { id: u32 },
    Clear,
}

/// World resource: instructions waiting for the trail system.
#[derive(Debug, Default)]
pub struct TrailQueue {
    pending: Vec<TrailOp>,
    /// The fixed tick running now (set by the app before each tick).
    tick: u64,
    /// A trail system drains this queue.
    attached: bool,
}

impl TrailQueue {
    /// Set how emitter `id` looks from here on (its points already laid
    /// change too): in the same tick as its first point, or simply every tick.
    /// An emitter with no style uses [`TrailStyle::default`].
    pub fn style(&mut self, id: u32, style: TrailStyle) {
        if self.attached {
            self.pending.push(TrailOp::Style { id, style });
        }
    }

    /// The emitter's position this tick: a strip facing the camera.
    #[inline]
    pub fn point(&mut self, id: u32, pos: [f32; 3]) {
        self.point_scaled(id, pos, 1.0);
    }

    /// As [`TrailQueue::point`], with the strip's width scaled at this point
    /// (a drift that grows, a flame that gutters).
    #[inline]
    pub fn point_scaled(&mut self, id: u32, pos: [f32; 3], width: f32) {
        if self.attached {
            self.pending.push(TrailOp::Point { id, tick: self.tick, pos, edge: [0.0; 3], width });
        }
    }

    /// The emitter's position this tick and its half span: a ribbon from
    /// `pos - edge` to `pos + edge`, whichever way the camera looks.
    #[inline]
    pub fn ribbon(&mut self, id: u32, pos: [f32; 3], edge: [f32; 3]) {
        if self.attached {
            self.pending.push(TrailOp::Point { id, tick: self.tick, pos, edge, width: 1.0 });
        }
    }

    /// Lift the pen: what is there stays and ages, the next point of this
    /// emitter starts a new strip.
    pub fn cut(&mut self, id: u32) {
        if self.attached {
            self.pending.push(TrailOp::Cut { id });
        }
    }

    /// Drop every strip (a level restart, a replayed moment). Instructions
    /// made after this call in the same frame stay.
    pub fn clear(&mut self) {
        if self.attached {
            self.pending.clear();
            self.pending.push(TrailOp::Clear);
        }
    }

    /// Instructions waiting.
    pub fn pending(&self) -> &[TrailOp] {
        &self.pending
    }

    /// Is a trail system draining this queue?
    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Called by the trail system when it is built.
    pub fn attach(&mut self) {
        self.attached = true;
    }

    /// Move the waiting instructions into `out` (the trail system, each frame).
    pub fn drain_into(&mut self, out: &mut Vec<TrailOp>) {
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
    use crate::rng::Pcg32;

    fn emitter(w: &mut World) {
        let x = w.res_mut::<Pcg32>().range_f32(0.0, 100.0);
        let mut q = w.res_mut::<TrailQueue>();
        q.style(1, TrailStyle::default());
        q.point(1, [x, 0.0, 0.0]);
        q.ribbon(2, [x, 0.0, 0.0], [0.0, 1.0, 0.0]);
        q.cut(1);
    }

    #[test]
    fn queue_is_a_no_op_without_the_system() {
        // No trail system: instructions vanish, nothing grows, and the
        // game's own RNG and clock go exactly as with no calls at all.
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
            let q = app.world.res::<TrailQueue>();
            assert!(!q.is_attached());
            assert!(q.pending().is_empty());
            assert_eq!(q.pending.capacity(), 0, "nothing was ever stored");
            let next = app.world.res_mut::<Pcg32>().next_u32();
            (next, app.world.res::<crate::time::Time>().tick)
        };
        assert_eq!(run(true), run(false));
    }

    #[test]
    fn attached_queue_stamps_the_fixed_tick_and_keeps_the_order() {
        let mut app = App::new("t").with_system(SystemDesc::new("emit", Stage::Update, emitter));
        app.world.res_mut::<TrailQueue>().attach();
        // No system took them: the app clears them at the end of the frame.
        app.run_headless(1, 3.0 * 1000.0 / 60.0 + 0.5);
        assert!(app.world.res::<TrailQueue>().pending().is_empty());

        let mut q = app.world.res_mut::<TrailQueue>();
        q.set_tick(7);
        q.point(3, [1.0, 2.0, 3.0]);
        q.cut(3);
        let mut got = Vec::new();
        q.drain_into(&mut got);
        assert!(q.pending().is_empty());
        assert_eq!(
            got,
            [
                TrailOp::Point { id: 3, tick: 7, pos: [1.0, 2.0, 3.0], edge: [0.0; 3], width: 1.0 },
                TrailOp::Cut { id: 3 },
            ]
        );
        // A clear drops what waits; what follows it stays.
        q.point(3, [0.0; 3]);
        q.clear();
        q.cut(4);
        assert_eq!(q.pending(), [TrailOp::Clear, TrailOp::Cut { id: 4 }]);
    }
}
