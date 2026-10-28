//! The kit's sparkles and confetti, emitted into the shared GPU particle
//! system (`evani_core::particles::ParticleQueue`; the game lists the
//! `particles` effect). The kit keeps no particle pool and draws no
//! particle itself: it decides where, when and what colour, and the engine
//! simulates and draws them. Without the effect in the build every emit is a
//! no-op.
//!
//! The numbers are the kit's own, in board cells (cells, cells per second)
//! as the sim has them; [`Sparks`] turns them into CSS pixels with the
//! frame's layout. Particles are visual only: they use their own random
//! stream and nothing here ever feeds back into the game.
//!
//! Sprites have no clipping, and neither do particles, so the blast and pop
//! sparkles stay inside the board by ending their life where their path
//! would reach the board's edge (the engine fades the last quarter of a
//! life out, so they melt away just before it). Confetti is held to the
//! board the same way: it is shed from the top rows and falls over the tiles.

use evani_core::particles::{Emit, ParticleQueue, Space};
use evani_core::rng::Pcg32;
use evani_render::atlas::cell;

use crate::board::{Kind, MAX_COLORS, NO_COLOR};
use crate::draw::{RECT, board_inset, layer};
use crate::ecs::Blast;
use crate::layout::Layout;

/// Velocity decay of a sparkle per second.
pub const DRAG: f32 = 1.0;
/// Piece colours are lit up for the sparkles (HDR, so `bloom` catches them).
const GLOW: f32 = 1.9;
/// The white-gold sparkle (linear HDR).
const WHITE: [f32; 4] = [2.4, 2.1, 1.3, 1.0];
/// A confetti piece is a flat strip: height as a share of its width.
const CONFETTI_ASPECT: f32 = 0.55;
/// Confetti falls with this share of the sparkles' gravity, slowly enough
/// to still be over the board when the win card shows (about 1.3 s in).
const CONFETTI_GRAVITY: f32 = 0.2;
/// A sparkle ends this far (cells) before its edge reaches the board's.
const EDGE_MARGIN: f32 = 0.1;
/// Shortest life worth emitting (s).
const MIN_LIFE: f32 = 0.03;
/// Step of the edge search (s).
const EDGE_STEP: f32 = 1.0 / 60.0;

/// What a sparkle looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Star,
    Circle,
    /// A strip of confetti (flat, no drag).
    Confetti,
    Diamond,
}

impl Shape {
    /// The atlas cell the shape draws with.
    pub fn cell(self) -> u16 {
        match self {
            Self::Star => cell::STAR,
            Self::Circle => cell::CIRCLE,
            Self::Confetti => RECT,
            Self::Diamond => cell::DIAMOND,
        }
    }
}

/// Distance factors of the engine's closed-form motion after `t` seconds
/// with drag `k` (`evani_render::particles::motion`): the start velocity
/// carries a particle `f`, gravity `h`. Used to find where a sparkle meets
/// the board's edge.
fn travel(t: f32, k: f32) -> (f32, f32) {
    let kt = k * t;
    if kt < 0.01 {
        (t * (1.0 - 0.5 * kt), 0.5 * t * t * (1.0 - kt / 3.0))
    } else {
        let f = (1.0 - (-kt).exp()) / k;
        (f, (t - f) / k)
    }
}

/// The kit's emitter for one tick: the queue, the kit's effects stream and
/// the frame's board layout.
pub struct Sparks<'a> {
    pub queue: &'a mut ParticleQueue,
    pub rng: &'a mut Pcg32,
    /// Cell size and board corner in CSS pixels.
    pub layout: Layout,
    /// The board's columns and rows.
    pub dims: [usize; 2],
    /// The six piece colours (linear).
    pub colors: [[f32; 4]; 6],
    /// Cells per second squared.
    pub gravity: f32,
}

impl Sparks<'_> {
    /// One sparkle at `pos` (cells) moving `vel` (cells per second), `size`
    /// cells wide, in piece colour `color` ([`NO_COLOR`]: white-gold).
    pub fn spawn(&mut self, pos: [f32; 2], vel: [f32; 2], life: f32, size: f32, color: u8, shape: Shape) {
        // The spin is drawn first and always, so the stream is the same
        // whether or not this sparkle ends up emitted.
        let spin = self.rng.range_f32(-6.0, 6.0);
        if !self.queue.is_attached() || life <= 0.0 || self.layout.cell <= 0.0 {
            return;
        }
        let confetti = shape == Shape::Confetti;
        let drag = if confetti { 0.0 } else { DRAG };
        // Confetti floats down: a share of the sparkles' gravity.
        let gravity = if confetti { self.gravity * CONFETTI_GRAVITY } else { self.gravity };
        let kept = life.min(self.edge_time(pos, vel, life, size, drag, gravity));
        if kept < MIN_LIFE {
            return;
        }
        let c = self.layout.cell;
        let col = if color == NO_COLOR {
            WHITE
        } else {
            let p = self.colors.get(usize::from(color)).copied().unwrap_or([0.9, 0.9, 0.95, 1.0]);
            [p[0] * GLOW, p[1] * GLOW, p[2] * GLOW, p[3]]
        };
        let mut e = Emit::new(
            [self.layout.origin[0] + (pos[0] + 0.5) * c, self.layout.origin[1] + (pos[1] + 0.5) * c, 0.0],
            [vel[0] * c, vel[1] * c, 0.0],
            col,
            kept,
            size * c,
            shape.cell(),
            Space::Screen2d { layer: layer::SPARK },
        );
        e.gravity = gravity * c;
        e.drag = drag;
        // The sparkle turns `spin` radians over its whole un-clipped life
        // and shrinks to half its size over it (a clipped one stops early).
        e.spin = spin / life;
        e.size_end = 1.0 - 0.5 * kept / life;
        if shape == Shape::Confetti {
            e.aspect = CONFETTI_ASPECT;
        }
        self.queue.emit(e);
    }

    /// How long (s) a sparkle lives before its edge would reach the board's
    /// edge, up to `life`, for a sparkle with drag `drag` and gravity
    /// `gravity` (cells per second squared).
    fn edge_time(&self, pos: [f32; 2], vel: [f32; 2], life: f32, size: f32, drag: f32, gravity: f32) -> f32 {
        let [w, h] = self.dims;
        let steps = (life / EDGE_STEP).ceil() as u32;
        for i in 0..=steps {
            let t = (i as f32 * EDGE_STEP).min(life);
            let (f, hh) = travel(t, drag);
            let at = [pos[0] + vel[0] * f, pos[1] + vel[1] * f + gravity * hh];
            let half = 0.5 * size * (1.0 - 0.5 * t / life);
            if board_inset(at, w, h) - half <= EDGE_MARGIN {
                // The last step that was still inside (a fast one never ends
                // past the edge by the step that found it).
                return (t - EDGE_STEP).max(0.0);
            }
        }
        life
    }

    /// `n` sparkles in a ring around `(x, y)` (cells): stars, diamonds and
    /// circles, every fourth one white-gold, thrown up a little.
    pub fn burst(&mut self, x: f32, y: f32, color: u8, n: usize, speed: f32) {
        for k in 0..n {
            let a = self.rng.range_f32(0.0, core::f32::consts::TAU);
            let s = speed * self.rng.range_f32(0.4, 1.0);
            let shape = match k % 3 {
                0 => Shape::Star,
                1 => Shape::Diamond,
                _ => Shape::Circle,
            };
            let c = if k % 4 == 0 { NO_COLOR } else { color };
            let life = self.rng.range_f32(0.35, 0.7);
            let size = self.rng.range_f32(0.12, 0.24);
            self.spawn([x, y], [a.cos() * s, a.sin() * s - 2.0], life, size, c, shape);
        }
    }

    /// What a special did, `n` being the effects' density-scaled count.
    pub fn blast(&mut self, b: &Blast, n: usize) {
        match b.kind {
            Kind::StreakH | Kind::StreakV => {
                // Sparks streaming along the line.
                let len = if b.kind == Kind::StreakH { self.dims[0] } else { self.dims[1] } as f32;
                for k in 0..n * 2 {
                    let u = self.rng.range_f32(0.0, len) - 0.5;
                    let (pos, vel) = if b.kind == Kind::StreakH {
                        ([u, b.y], [self.rng.range_f32(-1.0, 1.0), self.rng.range_f32(-3.0, 1.0)])
                    } else {
                        ([b.x, u], [self.rng.range_f32(-3.0, 3.0), self.rng.range_f32(-1.0, 1.0)])
                    };
                    let c = if k % 2 == 0 { NO_COLOR } else { b.color };
                    self.spawn(pos, vel, 0.45, 0.2, c, Shape::Star);
                }
            }
            Kind::Bomb => self.burst(b.x, b.y, b.color, n * 2, 9.0),
            Kind::Prism => {
                for k in 0..n * 2 {
                    let c = (k % MAX_COLORS as usize) as u8;
                    self.burst(b.x, b.y, c, 1, 10.0);
                }
            }
            Kind::Normal => {}
        }
    }

    /// The win's confetti: `n` strips shed from the board's top rows that
    /// fall over the tiles and end at the board's edge (they never fall
    /// past it, as they did when they were thrown up from the middle).
    pub fn confetti(&mut self, n: usize) {
        let w = self.dims[0] as f32;
        for k in 0..n {
            let x = self.rng.range_f32(0.0, w - 1.0);
            let y = self.rng.range_f32(0.3, 2.5);
            let c = (k % MAX_COLORS as usize) as u8;
            let vx = self.rng.range_f32(-1.2, 1.2);
            let vy = self.rng.range_f32(-2.5, 1.0);
            let life = self.rng.range_f32(1.6, 2.6);
            let size = self.rng.range_f32(0.16, 0.26);
            self.spawn([x, y], [vx, vy], life, size, c, Shape::Confetti);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::DEFAULT_PIECES;
    use crate::layout::{CellLimits, layout};

    fn emitter<'a>(q: &'a mut ParticleQueue, rng: &'a mut Pcg32) -> Sparks<'a> {
        Sparks {
            queue: q,
            rng,
            layout: layout(1280.0, 720.0, 8, 8, CellLimits::default()),
            dims: [8, 8],
            colors: DEFAULT_PIECES,
            gravity: 14.0,
        }
    }

    fn attached() -> ParticleQueue {
        let mut q = ParticleQueue::default();
        q.attach();
        q
    }

    #[test]
    fn emits_are_a_no_op_without_the_system_and_the_stream_still_advances() {
        let mut off = ParticleQueue::default();
        let mut on = attached();
        let (mut a, mut b) = (Pcg32::new(7, 99), Pcg32::new(7, 99));
        let b_blast = Blast {
            x: 3.0,
            y: 3.0,
            t: 0.0,
            kind: Kind::Bomb,
            combo: None,
            color: 1,
        };
        emitter(&mut off, &mut a).blast(&b_blast, 14);
        emitter(&mut on, &mut b).blast(&b_blast, 14);
        assert!(off.pending().is_empty());
        assert!(!on.pending().is_empty());
        assert_eq!(a.next_u32(), b.next_u32(), "the effects stream advances the same either way");
    }

    #[test]
    fn a_sparkle_ends_before_it_leaves_the_board() {
        let mut q = attached();
        let mut rng = Pcg32::new(1, 99);
        let mut s = emitter(&mut q, &mut rng);
        // On the left edge, flying left: gone at once.
        s.spawn([-0.4, 3.0], [-9.0, 0.0], 0.5, 0.2, 0, Shape::Star);
        // In the middle, flying right: ends when it nears the right edge.
        s.spawn([3.0, 3.0], [9.0, 0.0], 0.7, 0.2, 0, Shape::Star);
        // Confetti is clipped too (it has no drag, so it reaches the edge
        // a little sooner than the sparkle with the same start).
        s.spawn([3.0, 3.0], [9.0, 0.0], 0.7, 0.2, 0, Shape::Confetti);
        let p = q.pending();
        assert_eq!(p.len(), 2, "the one flying out of the left edge was dropped");
        assert!(p[0].emit.life < 0.7 && p[0].emit.life > 0.1, "clipped to {}", p[0].emit.life);
        assert!(p[1].emit.life < 0.7 && p[1].emit.life > 0.1 && p[1].emit.life < p[0].emit.life, "confetti clipped to {}", p[1].emit.life);
        assert_eq!(p[1].emit.aspect, CONFETTI_ASPECT);
    }

    /// The layouts the review card shoots (a desktop, a phone held sideways
    /// and upright) plus a tablet and a short landscape screen.
    const SIZES: [(f32, f32); 5] = [(1920.0, 1080.0), (1280.0, 720.0), (844.0, 390.0), (390.0, 844.0), (768.0, 1024.0)];

    fn emitter_for<'a>(q: &'a mut ParticleQueue, rng: &'a mut Pcg32, vw: f32, vh: f32) -> Sparks<'a> {
        Sparks {
            layout: layout(vw, vh, 8, 8, CellLimits::default()),
            ..emitter(q, rng)
        }
    }

    /// Round 3 of the critic: sparkles showed at the edge of a portrait
    /// phone instead of at the blast, and confetti fell beside and below the
    /// board. Every effect of a blast starts at its piece's centre (or along
    /// its streak's row or column) on the screen, in every layout.
    #[test]
    fn blast_sparkles_start_on_the_popped_piece_in_every_layout() {
        for (vw, vh) in SIZES {
            for kind in [Kind::Bomb, Kind::Prism, Kind::StreakH, Kind::StreakV] {
                let mut q = attached();
                let mut rng = Pcg32::new(5, 99);
                let mut s = emitter_for(&mut q, &mut rng, vw, vh);
                let l = s.layout;
                let b = Blast {
                    x: 3.0,
                    y: 4.0,
                    t: 0.0,
                    kind,
                    combo: None,
                    color: 2,
                };
                s.blast(&b, 12);
                let c = l.center(3.0, 4.0);
                assert!(!q.pending().is_empty(), "{vw}x{vh} {kind:?}: nothing emitted");
                for p in q.pending() {
                    let at = p.emit.pos;
                    match kind {
                        Kind::StreakH => assert_eq!(at[1], c[1], "{vw}x{vh}: {at:?} off its row"),
                        Kind::StreakV => assert_eq!(at[0], c[0], "{vw}x{vh}: {at:?} off its column"),
                        _ => assert_eq!([at[0], at[1]], c, "{vw}x{vh} {kind:?}: starts at {at:?}, not on the piece {c:?}"),
                    }
                    assert!(at[0] >= l.board[0] && at[0] <= l.board[0] + l.board[2], "{vw}x{vh}: {at:?} outside the board {:?}", l.board);
                    assert!(at[1] >= l.board[1] && at[1] <= l.board[1] + l.board[3], "{vw}x{vh}: {at:?} outside the board {:?}", l.board);
                }
            }
        }
    }

    /// Where an emitted particle is `t` seconds in (screen pixels): the
    /// engine's closed-form motion (`evani_render::particles::motion`).
    fn at(e: &Emit, t: f32) -> [f32; 2] {
        let (f, h) = travel(t, e.drag);
        [e.pos[0] + e.vel[0] * f, e.pos[1] + e.vel[1] * f + e.gravity * h]
    }

    #[test]
    fn confetti_falls_over_the_board_and_never_past_it() {
        for (vw, vh) in SIZES {
            let mut q = attached();
            let mut rng = Pcg32::new(11, 99);
            let mut s = emitter_for(&mut q, &mut rng, vw, vh);
            let l = s.layout;
            s.confetti(80);
            assert!(q.pending().len() > 60, "{vw}x{vh}: confetti was dropped: {}", q.pending().len());
            // The win card shows about 1.3 s after the win: most of the
            // confetti is still falling then (the critic's win picture
            // showed none once it was held over the board).
            let alive = q.pending().iter().filter(|p| p.emit.life >= 1.3).count();
            assert!(alive * 10 >= q.pending().len() * 7, "{vw}x{vh}: only {alive} of {} confetti are still up at 1.3 s", q.pending().len());
            for p in q.pending() {
                let e = &p.emit;
                for i in 0..=20 {
                    let [x, y] = at(e, e.life * i as f32 / 20.0);
                    assert!(
                        x >= l.board[0] && x <= l.board[0] + l.board[2] && y >= l.board[1] && y <= l.board[1] + l.board[3],
                        "{vw}x{vh}: confetti at ({x:.0}, {y:.0}) is off the board {:?} at {:.2}s of {:.2}s",
                        l.board,
                        e.life * i as f32 / 20.0,
                        e.life
                    );
                }
            }
        }
    }

    #[test]
    fn emits_land_on_the_board_in_css_pixels() {
        let mut q = attached();
        let mut rng = Pcg32::new(1, 99);
        let mut s = emitter(&mut q, &mut rng);
        let l = s.layout;
        s.spawn([2.0, 4.0], [1.0, -2.0], 0.4, 0.2, NO_COLOR, Shape::Circle);
        let e = q.pending()[0].emit;
        assert_eq!(e.pos[0], l.origin[0] + 2.5 * l.cell);
        assert_eq!(e.pos[1], l.origin[1] + 4.5 * l.cell);
        assert_eq!(e.vel[1], -2.0 * l.cell);
        assert_eq!(e.size, 0.2 * l.cell);
        assert_eq!(e.gravity, 14.0 * l.cell);
        assert_eq!(e.color, WHITE);
        assert_eq!(e.space, Space::Screen2d { layer: layer::SPARK });
    }

    #[test]
    fn travel_matches_plain_motion_without_drag() {
        let (f, h) = travel(0.5, 0.0);
        assert!((f - 0.5).abs() < 1e-6 && (h - 0.125).abs() < 1e-6);
        let (f, h) = travel(1.0, 1.0);
        assert!((f - (1.0 - (-1.0f32).exp())).abs() < 1e-6);
        assert!((h - (1.0 - f)).abs() < 1e-6);
    }
}
