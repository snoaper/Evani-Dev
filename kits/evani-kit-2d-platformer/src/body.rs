//! Kinematic box bodies against the tile map and moving platforms.
//!
//! A body is an axis-aligned box `2 * hw` wide and `h` tall whose position
//! is the bottom centre of its feet. Moves are resolved per axis, x then y:
//!
//! * X checks only the leading-edge column against solid tiles. A solid
//!   whose top is within [`STEP`] pixels of the feet doesn't block while the
//!   body isn't rising; the y pass then steps up onto it (no sticking at
//!   slope ends). Slopes never block sideways.
//! * Y (not rising) lands on the highest surface between [`STEP`] above the
//!   old feet and the new feet (plus `snap` when the body was grounded, so
//!   it stays glued walking down slopes): the foot point's slope first,
//!   else solid tops under the foot span, one-way tops the feet were on or
//!   above, and moving platforms. Rising, the head stops under solid tiles.

use crate::level::{Level, Surface, SurfaceKind, TILE, cell};

/// Highest step a body walks up without jumping (pixels).
pub const STEP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Body {
    /// Foot point (bottom centre), pixels.
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub hw: f32,
    pub h: f32,
    pub grounded: bool,
    /// What it stands on (when grounded).
    pub ground: Option<SurfaceKind>,
    /// Hit a wall this move (-1 left, 1 right, 0 none).
    pub wall: i8,
    /// Hit its head this move.
    pub bonked: bool,
}

/// A moving platform's top this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlatformTop {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// Its top last tick (one-way landing test).
    pub prev_y: f32,
}

/// Options for one move.
#[derive(Debug, Clone, Copy, Default)]
pub struct MoveOpts {
    /// Ignore one-way tiles and platforms (dropping through).
    pub drop_through: bool,
    /// Snap down this far when grounded (slopes).
    pub snap: f32,
}

impl Body {
    pub fn new(pos: [f32; 2], hw: f32, h: f32) -> Self {
        Self { pos, hw, h, ..Self::default() }
    }

    pub fn top(&self) -> f32 {
        self.pos[1] - self.h
    }

    /// `[x0, y0, x1, y1]`.
    pub fn rect(&self) -> [f32; 4] {
        [self.pos[0] - self.hw, self.pos[1] - self.h, self.pos[0] + self.hw, self.pos[1]]
    }

    /// Move by `vel * dt` against the level and platforms.
    pub fn step(&mut self, level: &Level, plats: &[PlatformTop], dt: f32, opts: MoveOpts) {
        self.wall = 0;
        self.bonked = false;
        self.move_x(level, self.vel[0] * dt);
        self.move_y(level, plats, self.vel[1] * dt, opts);
    }

    /// Horizontal move: only the leading-edge column blocks.
    pub fn move_x(&mut self, level: &Level, dx: f32) {
        if dx == 0.0 {
            return;
        }
        let nx = self.pos[0] + dx;
        let dir = dx.signum();
        let edge = nx + dir * self.hw;
        let tx = cell(if dir > 0.0 { edge - 0.001 } else { edge });
        let (y0, y1) = (self.pos[1] - self.h + 0.01, self.pos[1] - 0.01);
        let rising = self.vel[1] < 0.0;
        for ty in cell(y0)..=cell(y1) {
            if !level.tile(tx, ty).is_solid() {
                continue;
            }
            let top = ty as f32 * TILE;
            // A low step: let the y pass climb it.
            if !rising && self.pos[1] - top <= STEP && !level.tile(tx, ty - 1).is_solid() {
                continue;
            }
            self.pos[0] = if dir > 0.0 { tx as f32 * TILE - self.hw } else { (tx + 1) as f32 * TILE + self.hw };
            self.vel[0] = 0.0;
            self.wall = dir as i8;
            return;
        }
        self.pos[0] = nx;
    }

    /// Vertical move: landing (slopes, solids, one-ways, platforms) or a
    /// head bump.
    pub fn move_y(&mut self, level: &Level, plats: &[PlatformTop], dy: f32, opts: MoveOpts) {
        let old = self.pos[1];
        let ny = old + dy;
        let (x0, x1, xc) = (self.pos[0] - self.hw, self.pos[0] + self.hw, self.pos[0]);
        if self.vel[1] < 0.0 {
            self.grounded = false;
            self.ground = None;
            // Head against solid tiles.
            let head = ny - self.h;
            let ty = cell(head);
            let blocked = (cell(x0)..=cell(x1 - 0.001)).any(|tx| level.tile(tx, ty).is_solid());
            if blocked {
                self.pos[1] = (ty + 1) as f32 * TILE + self.h;
                self.vel[1] = 0.0;
                self.bonked = true;
            } else {
                self.pos[1] = ny;
            }
            return;
        }
        let snap = if self.grounded { opts.snap } else { 0.0 };
        let (lo, hi) = (old - STEP, ny + snap);
        let oneway = if opts.drop_through { None } else { Some(old) };
        let mut best = level.surface(x0, x1, xc, lo, hi, oneway);
        if !opts.drop_through {
            for (i, p) in plats.iter().enumerate() {
                if x1 <= p.x0 || x0 >= p.x1 {
                    continue;
                }
                // One-way: the feet were on or above its top.
                if old > p.prev_y.max(p.y) + 0.5 || p.y < lo || p.y > hi {
                    continue;
                }
                if best.is_none_or(|b| p.y < b.y) {
                    best = Some(Surface { y: p.y, kind: SurfaceKind::Platform(i) });
                }
            }
        }
        match best {
            Some(s) => {
                self.pos[1] = s.y;
                self.vel[1] = 0.0;
                self.grounded = true;
                self.ground = Some(s.kind);
            }
            None => {
                self.pos[1] = ny;
                self.grounded = false;
                self.ground = None;
            }
        }
    }

    /// Is there floor (static) just ahead of the feet in direction `dir`?
    /// Enemies use it to turn at ledges.
    pub fn floor_ahead(&self, level: &Level, dir: f32) -> bool {
        self.floor_at(level, dir * (self.hw + 2.0))
    }

    /// Is there floor (static, not deadly) at the feet's height `dx` pixels
    /// to the side of the centre?
    pub fn floor_at(&self, level: &Level, dx: f32) -> bool {
        let x = self.pos[0] + dx;
        let y = self.pos[1];
        level.surface(x - 1.0, x + 1.0, x, y - STEP, y + 10.0, Some(y)).is_some() && !level.tile_at(x, y + 2.0).is_hazard()
    }
}

/// Do two `[x0, y0, x1, y1]` boxes overlap?
pub fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;

    /// A small test map. Row 4 is the floor top (y = 64 + 16 = 80).
    fn map(rows: &[&str]) -> Level {
        let rows: Vec<String> = rows.iter().map(|r| format!("\"{}\"", r.replace('\\', "\\\\"))).collect();
        let src = format!(
            r##"{{"name":"t","time_limit_s":10,"tile":16,"legend":{{
                ".":{{"kind":"empty"}},"#":{{"kind":"solid"}},"=":{{"kind":"oneway"}},
                "/":{{"kind":"slope","from":0,"to":16}},"\\":{{"kind":"slope","from":16,"to":0}},
                "a":{{"kind":"slope","from":0,"to":8}},"b":{{"kind":"slope","from":8,"to":16}}}},
                "rows":[{}],"entities":{{"start":[0,0],"goal":[1,0]}}}}"##,
            rows.join(",")
        );
        Level::from_json(&src).unwrap()
    }

    fn run(level: &Level, b: &mut Body, vx: f32, ticks: usize) {
        let dt = 1.0 / 60.0;
        for _ in 0..ticks {
            b.vel[0] = vx;
            b.vel[1] = (b.vel[1] + 980.0 * dt).min(330.0);
            b.step(level, &[], dt, MoveOpts { drop_through: false, snap: 6.0 });
        }
    }

    #[test]
    fn walks_up_and_down_a_45_degree_hill_without_sticking() {
        // Two 45 degree steps up to a top at y = 48, two down, floor y = 80.
        let l = map(&["..........", "..........", "..........", ".../#\\....", "../###\\...", "##########"]);
        let mut b = Body::new([8.0, 80.0], 5.0, 14.0);
        run(&l, &mut b, 0.0, 5);
        assert!(b.grounded && b.pos[1] == 80.0, "{:?}", b.pos);
        let mut min_y = f32::MAX;
        for _ in 0..70 {
            run(&l, &mut b, 120.0, 1);
            min_y = min_y.min(b.pos[1]);
            assert_eq!(b.wall, 0, "stuck at {:?}", b.pos);
        }
        // Over the top (row 3 top = 48) and back down to the floor.
        assert!(min_y <= 49.0, "{min_y}");
        assert!(b.pos[0] > 140.0 && b.pos[1] == 80.0 && b.grounded, "{:?}", b.pos);
    }

    #[test]
    fn foot_point_follows_slopes_exactly() {
        let l = map(&["......", "......", "......", "..ab##", "######"]);
        let mut b = Body::new([8.0, 64.0], 5.0, 14.0);
        run(&l, &mut b, 0.0, 3);
        // Walk to x = 40 (a quarter of tile 2, the 0 -> 8 slope): floor 64 - 2.
        while b.pos[0] < 40.0 {
            run(&l, &mut b, 60.0, 1);
        }
        let want = 64.0 - (b.pos[0] - 32.0) / 16.0 * 8.0;
        assert!((b.pos[1] - want).abs() < 0.01, "{} vs {want}", b.pos[1]);
        assert!(b.grounded);
        // Gentle pair reaches the next level up.
        while b.pos[0] < 72.0 {
            run(&l, &mut b, 60.0, 1);
        }
        assert_eq!(b.pos[1], 48.0);
    }

    #[test]
    fn one_ways_land_from_above_and_pass_from_below() {
        let l = map(&["......", "......", "..==..", "......", "......", "######"]);
        // From below: jump up through the plank.
        let mut b = Body::new([40.0, 80.0], 5.0, 14.0);
        run(&l, &mut b, 0.0, 2);
        b.vel[1] = -330.0;
        let mut passed = false;
        for _ in 0..40 {
            run(&l, &mut b, 0.0, 1);
            if b.pos[1] < 32.0 {
                passed = true;
            }
        }
        assert!(passed, "never rose through the plank");
        // And lands on it on the way down.
        assert!(b.grounded && b.pos[1] == 32.0, "{:?}", b.pos);
        // Dropping through.
        b.step(&l, &[], 1.0 / 60.0, MoveOpts { drop_through: true, snap: 0.0 });
        for _ in 0..30 {
            b.vel[1] = (b.vel[1] + 980.0 / 60.0).min(330.0);
            b.step(&l, &[], 1.0 / 60.0, MoveOpts { drop_through: true, snap: 0.0 });
        }
        assert_eq!(b.pos[1], 80.0);
    }

    #[test]
    fn walls_block_and_low_steps_are_climbed() {
        let l = map(&["......", "....#.", "....#.", "...##.", "######"]);
        let mut b = Body::new([8.0, 64.0], 5.0, 14.0);
        run(&l, &mut b, 0.0, 2);
        // A 16 px block is not a step.
        for _ in 0..60 {
            run(&l, &mut b, 90.0, 1);
        }
        assert_eq!(b.pos[0], 48.0 - 5.0);
        assert_eq!(b.wall, 1);
        // A 4 px ledge is.
        let l2 = map(&["......", "......", "......", "......", "######"]);
        let mut b2 = Body::new([8.0, 68.0], 5.0, 14.0);
        b2.grounded = true;
        // Standing 4 px below the top of row 4 (inside it) next to it.
        b2.move_y(&l2, &[], 0.0, MoveOpts::default());
        assert_eq!(b2.pos[1], 64.0);
    }

    #[test]
    fn rides_platforms_as_one_ways() {
        let l = map(&["......", "......", "......", "......", "......"]);
        let plat = PlatformTop { x0: 20.0, x1: 52.0, y: 40.0, prev_y: 40.0 };
        let mut b = Body::new([36.0, 20.0], 5.0, 14.0);
        for _ in 0..30 {
            b.vel[1] = (b.vel[1] + 980.0 / 60.0).min(330.0);
            b.step(&l, &[plat], 1.0 / 60.0, MoveOpts::default());
        }
        assert_eq!(b.pos[1], 40.0);
        assert_eq!(b.ground, Some(SurfaceKind::Platform(0)));
        // From below it passes through.
        let mut c = Body::new([36.0, 60.0], 5.0, 14.0);
        c.vel[1] = -300.0;
        c.step(&l, &[plat], 1.0 / 10.0, MoveOpts::default());
        assert!(c.pos[1] < 40.0);
    }
}
