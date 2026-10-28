//! Pseudo-3D road (feature `pseudo3d`): the classic segment-projected road
//! of arcade racers, with curves, hills, roadside sprites and a scrolling
//! backdrop, drawn as flat 2D quads at the start of the sprites pass. No
//! depth buffer, no meshes and no `forward` path, so a 2D game can turn it
//! on by itself.
//!
//! * [`RoadTrack`]: the track as a ring of equal segments, each with a curve
//!   and an end height, plus its roadside sprites. Built from eased sections
//!   ([`RoadTrack::add`]) and seeded scatter ([`RoadTrack::scatter`]) with
//!   plain arithmetic (no trig, no clock, no `rand`), so the same calls give
//!   the same track on every machine, and gameplay can ask it for the curve
//!   and height under a car in the fixed step ([`RoadTrack::curve_at`],
//!   [`RoadTrack::height_at`]).
//! * [`RoadScene`]: the resource a kit inserts: track, camera, lanes and
//!   colours ([`RoadLook`]).
//! * [`RoadParams`]: the live tunables `render.pseudo3d.*` (curve, hill,
//!   draw distance, segment length, fog, camera height, field of view, road
//!   width).
//! * [`RoadScene::build`]: projects the scene for one viewport into
//!   [`RoadQuad`]s, back to front. Painter's order hides what a hill crest
//!   covers and lets nearer sprites cover farther ones, so nothing needs
//!   clipping. The GPU side (`gpu/pseudo3d.rs`) uploads them and draws them
//!   in one instanced call before the frame's sprites, so a kit's car and
//!   HUD sprites sit on top.
//!
//! Units: positions along the road (`RoadCamera::z`) are world units;
//! lateral positions and sprite sizes are road half-widths (so they follow
//! `render.pseudo3d.road_width`); hill heights are segment lengths (so they
//! follow `render.pseudo3d.segment_length` and the `hill` scale).

use evani_core::params::{F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_core::rng::Pcg32;

/// The road view's shader (one instanced quad pipeline, sprite pass group 0).
pub const WGSL: &str = include_str!("../shaders/pseudo3d.wgsl");

/// The cell a solid-colour quad samples: none (the GPU side reads one
/// white texel of the built-in square).
pub const FILL: u16 = u16::MAX;

/// A sprite standing beside (or on) the road at the near edge of its
/// segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadSprite {
    /// Atlas cell (a built-in shape, a kit sheet frame or named art).
    pub cell: u16,
    /// Lateral position in road half-widths from the centre line (negative:
    /// left). Beyond 1 is off the road; the sprite is shifted outwards by
    /// half its width so it never leans onto the road.
    pub offset: f32,
    /// Width and height in road half-widths.
    pub size: [f32; 2],
    /// Linear RGBA tint.
    pub color: [f32; 4],
}

/// One stretch of road, `render.pseudo3d.segment_length` long.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadSegment {
    /// How far the road bends per segment (positive: right), scaled by
    /// `render.pseudo3d.curve`. About 2 is gentle, 6 is sharp.
    pub curve: f32,
    /// Height at the segment's far end, in segment lengths, scaled by
    /// `render.pseudo3d.hill`.
    pub y: f32,
    /// Sprites at the segment's near edge.
    pub sprites: Vec<RoadSprite>,
}

/// The track: a ring of segments (the last one leads back to the first).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadTrack {
    segments: Vec<RoadSegment>,
    /// Sum of the curves of every segment before each one (the backdrop
    /// scroll).
    turn: Vec<f32>,
}

fn ease_in(a: f32, b: f32, p: f32) -> f32 {
    a + (b - a) * p * p
}

fn ease_out(a: f32, b: f32, p: f32) -> f32 {
    let q = 1.0 - p;
    a + (b - a) * (1.0 - q * q)
}

/// Smoothstep, the hills' ease (no trig: identical on every machine).
fn ease_in_out(a: f32, b: f32, p: f32) -> f32 {
    a + (b - a) * p * p * (3.0 - 2.0 * p)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl RoadTrack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn segments(&self) -> &[RoadSegment] {
        &self.segments
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Height at the end of the last segment (0 for an empty track).
    pub fn last_y(&self) -> f32 {
        self.segments.last().map_or(0.0, |s| s.y)
    }

    fn push(&mut self, curve: f32, y: f32) {
        let before = match (self.turn.last(), self.segments.last()) {
            (Some(t), Some(s)) => t + s.curve,
            _ => 0.0,
        };
        self.turn.push(before);
        self.segments.push(RoadSegment { curve, y, sprites: Vec::new() });
    }

    /// A section of `enter + hold + leave` segments: the curve eases in to
    /// `curve` over `enter`, holds, and eases out over `leave`; the height
    /// rises by `rise` segment lengths (negative: falls) with a smooth start
    /// and end across the whole section.
    pub fn add(&mut self, enter: u32, hold: u32, leave: u32, curve: f32, rise: f32) -> &mut Self {
        let start_y = self.last_y();
        let end_y = start_y + rise;
        let total = (enter + hold + leave).max(1) as f32;
        let y_at = |k: u32| ease_in_out(start_y, end_y, k as f32 / total);
        let mut k = 0u32;
        for n in 0..enter {
            k += 1;
            self.push(ease_in(0.0, curve, n as f32 / enter as f32), y_at(k));
        }
        for _ in 0..hold {
            k += 1;
            self.push(curve, y_at(k));
        }
        for n in 0..leave {
            k += 1;
            self.push(ease_out(curve, 0.0, n as f32 / leave as f32), y_at(k));
        }
        self
    }

    /// `n` straight, level segments.
    pub fn straight(&mut self, n: u32) -> &mut Self {
        self.add(0, n, 0, 0.0, 0.0)
    }

    /// A bend of `n` segments (a quarter easing in, a quarter out).
    pub fn bend(&mut self, n: u32, curve: f32) -> &mut Self {
        let e = n / 4;
        self.add(e, n - 2 * e, e, curve, 0.0)
    }

    /// A hill of `n` segments rising by `rise` segment lengths.
    pub fn hill(&mut self, n: u32, rise: f32) -> &mut Self {
        let e = n / 4;
        self.add(e, n - 2 * e, e, 0.0, rise)
    }

    /// `n` segments that bring the height back to 0, so the ring closes
    /// without a step.
    pub fn close(&mut self, n: u32) -> &mut Self {
        let fall = -self.last_y();
        self.add(0, n, 0, 0.0, fall)
    }

    /// Put `sprite` at the near edge of segment `segment`. False when there
    /// is no such segment.
    pub fn sprite(&mut self, segment: usize, sprite: RoadSprite) -> bool {
        match self.segments.get_mut(segment) {
            Some(s) => {
                s.sprites.push(sprite);
                true
            }
            None => false,
        }
    }

    /// One sprite every `every` segments (from the first), each a cell picked
    /// from `cells` on a random side between 1.2 and 2.4 half-widths out,
    /// its size between 0.75x and 1.25x `size`. Seeded: the same `rng`
    /// state gives the same scenery.
    pub fn scatter(&mut self, rng: &mut Pcg32, every: u32, cells: &[u16], size: [f32; 2], color: [f32; 4]) -> &mut Self {
        if cells.is_empty() {
            return self;
        }
        let every = every.max(1) as usize;
        for i in (0..self.segments.len()).step_by(every) {
            let cell = cells[rng.below(cells.len() as u32) as usize];
            let side = if rng.chance(0.5) { -1.0 } else { 1.0 };
            let offset = side * rng.range_f32(1.2, 2.4);
            let k = rng.range_f32(0.75, 1.25);
            self.segments[i].sprites.push(RoadSprite { cell, offset, size: [size[0] * k, size[1] * k], color });
        }
        self
    }

    /// Height at the start of segment `i` (the end of the one before; 0 for
    /// the first).
    fn start_y(&self, i: usize) -> f32 {
        if i == 0 {
            0.0
        } else {
            self.segments.get(i - 1).map_or(0.0, |s| s.y)
        }
    }

    /// Length of the ring in world units.
    pub fn length(&self, segment_length: f32) -> f32 {
        self.segments.len() as f32 * segment_length.max(1.0)
    }

    /// The segment under `z` (world units, wrapped round the ring) and how
    /// far along it, 0..1. `None` for an empty track.
    pub fn locate(&self, z: f32, segment_length: f32) -> Option<(usize, f32)> {
        let n = self.segments.len();
        if n == 0 {
            return None;
        }
        let seg = segment_length.max(1.0);
        let z = z.rem_euclid(seg * n as f32);
        let i = ((z / seg) as usize).min(n - 1);
        Some((i, ((z - i as f32 * seg) / seg).clamp(0.0, 1.0)))
    }

    /// The curve of the segment under `z` (unscaled; multiply by the
    /// `curve` value for what the view shows).
    pub fn curve_at(&self, z: f32, segment_length: f32) -> f32 {
        self.locate(z, segment_length).map_or(0.0, |(i, _)| self.segments[i].curve)
    }

    /// Ground height under `z`, in segment lengths (unscaled).
    pub fn height_at(&self, z: f32, segment_length: f32) -> f32 {
        self.locate(z, segment_length)
            .map_or(0.0, |(i, t)| lerp(self.start_y(i), self.segments[i].y, t))
    }

    /// How far the road has turned up to `z`, counting whole laps (the sum
    /// of every curve passed, unscaled): the backdrop scrolls with it.
    pub fn turn_at(&self, z: f32, segment_length: f32) -> f32 {
        let Some((i, t)) = self.locate(z, segment_length) else {
            return 0.0;
        };
        let lap_turn = self.turn.last().copied().unwrap_or(0.0) + self.segments.last().map_or(0.0, |s| s.curve);
        let laps = (z / self.length(segment_length)).floor();
        laps * lap_turn + self.turn[i] + self.segments[i].curve * t
    }
}

/// Where the eye is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoadCamera {
    /// Distance along the road in world units (may grow past a lap; it
    /// wraps).
    pub z: f32,
    /// Lateral position in road half-widths from the centre line.
    pub x: f32,
}

/// Colours of the road view (linear RGB). Bands alternate every
/// [`RoadScene::band`] segments, light then dark.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadLook {
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    /// What distance fades to (also below the horizon).
    pub fog: [f32; 3],
    pub hills_far: [f32; 3],
    pub hills_near: [f32; 3],
    /// Light and dark verge.
    pub grass: [[f32; 3]; 2],
    /// Light and dark kerb.
    pub rumble: [[f32; 3]; 2],
    /// Light and dark tarmac.
    pub road: [[f32; 3]; 2],
    /// Lane markings (drawn on light bands only, so they dash).
    pub lane: [f32; 3],
}

impl Default for RoadLook {
    /// A sunset coast: violet sky, orange horizon, warm haze.
    fn default() -> Self {
        Self {
            sky_top: [0.05, 0.06, 0.25],
            sky_horizon: [0.95, 0.42, 0.18],
            fog: [0.62, 0.36, 0.30],
            hills_far: [0.30, 0.16, 0.30],
            hills_near: [0.08, 0.16, 0.12],
            grass: [[0.10, 0.36, 0.08], [0.07, 0.28, 0.06]],
            rumble: [[0.85, 0.85, 0.85], [0.60, 0.04, 0.03]],
            road: [[0.16, 0.16, 0.17], [0.13, 0.13, 0.14]],
            lane: [0.85, 0.85, 0.80],
        }
    }
}

/// The road view a kit shows: insert it as a resource; the renderer draws it
/// every frame it is `visible`.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadScene {
    pub track: RoadTrack,
    pub camera: RoadCamera,
    pub look: RoadLook,
    /// Lanes (markings between them).
    pub lanes: u32,
    /// Segments per colour band.
    pub band: u32,
    pub visible: bool,
}

impl RoadScene {
    pub fn new(track: RoadTrack) -> Self {
        Self { track, camera: RoadCamera::default(), look: RoadLook::default(), lanes: 3, band: 3, visible: true }
    }
}

/// One quad of the road view in CSS pixels, as the GPU side draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadQuad {
    /// Top-left, top-right, bottom-left, bottom-right.
    pub corners: [[f32; 2]; 4],
    /// Atlas cell, or [`FILL`] for a solid colour.
    pub cell: u16,
    /// Linear RGBA at the top and bottom edges (a vertical gradient).
    pub top: [f32; 4],
    pub bottom: [f32; 4],
    /// How far towards the look's fog colour, 0..1.
    pub fog: f32,
}

/// The values [`RoadScene::build`] projects with (the `render.pseudo3d.*`
/// parameters; [`RoadParams::values`] reads them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadValues {
    pub curve: f32,
    pub hill: f32,
    pub draw_distance: u32,
    pub segment_length: f32,
    pub fog: f32,
    pub camera_height: f32,
    pub fov_deg: f32,
    pub road_width: f32,
}

impl Default for RoadValues {
    fn default() -> Self {
        Self {
            curve: 1.0,
            hill: 1.0,
            draw_distance: 300,
            segment_length: 200.0,
            fog: 5.0,
            camera_height: 1000.0,
            fov_deg: 100.0,
            road_width: 2000.0,
        }
    }
}

/// Handles of the `render.pseudo3d.*` parameters (a resource, registered by
/// the render module when the feature is on).
#[derive(Debug, Clone, Copy)]
pub struct RoadParams {
    pub curve: F32Param,
    pub hill: F32Param,
    pub draw_distance: I32Param,
    pub segment_length: F32Param,
    pub fog: F32Param,
    pub camera_height: F32Param,
    pub fov: F32Param,
    pub road_width: F32Param,
}

impl RoadParams {
    pub fn values(&self, reg: &ParamRegistry) -> RoadValues {
        RoadValues {
            curve: reg.f32(self.curve),
            hill: reg.f32(self.hill),
            draw_distance: reg.i32(self.draw_distance).max(1) as u32,
            segment_length: reg.f32(self.segment_length),
            fog: reg.f32(self.fog),
            camera_height: reg.f32(self.camera_height),
            fov_deg: reg.f32(self.fov),
            road_width: reg.f32(self.road_width),
        }
    }
}

/// Register the `render.pseudo3d.*` parameters and the [`RoadParams`]
/// resource (the render module calls it in `build`).
pub fn register(ctx: &mut PluginContext) -> Result<(), PluginError> {
    let d = RoadValues::default();
    let curve = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.curve", d.curve)
            .range(0.0, 4.0)
            .step(0.01)
            .unit("x")
            .describe("Pseudo-3D road: how strongly the track's bends show (0 = straight, 1 = as built)"),
    )?;
    let hill = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.hill", d.hill)
            .range(0.0, 4.0)
            .step(0.01)
            .unit("x")
            .describe("Pseudo-3D road: how tall the track's hills show (0 = flat, 1 = as built)"),
    )?;
    let draw_distance = ctx.param_i32(
        ParamDef::i32("render.pseudo3d.draw_distance", d.draw_distance as i32)
            .range(20.0, 1000.0)
            .step(1.0)
            .unit("segments")
            .describe("Pseudo-3D road: how many segments ahead are drawn"),
    )?;
    let segment_length = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.segment_length", d.segment_length)
            .range(20.0, 1000.0)
            .step(1.0)
            .describe("Pseudo-3D road: length of one segment in world units (longer stretches the track)"),
    )?;
    let fog = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.fog", d.fog)
            .range(0.0, 30.0)
            .step(0.1)
            .describe("Pseudo-3D road: fog density towards the draw distance (0 = none)"),
    )?;
    let camera_height = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.camera_height", d.camera_height)
            .range(100.0, 5000.0)
            .step(10.0)
            .describe("Pseudo-3D road: eye height above the road in world units"),
    )?;
    let fov = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.fov", d.fov_deg)
            .range(30.0, 150.0)
            .step(1.0)
            .unit("deg")
            .describe("Pseudo-3D road: horizontal field of view"),
    )?;
    let road_width = ctx.param_f32(
        ParamDef::f32("render.pseudo3d.road_width", d.road_width)
            .range(200.0, 8000.0)
            .step(10.0)
            .describe("Pseudo-3D road: half the road's width in world units"),
    )?;
    ctx.resource(RoadParams { curve, hill, draw_distance, segment_length, fog, camera_height, fov, road_width });
    Ok(())
}

/// Reused buffers for [`RoadScene::build`] (no allocation once warm).
#[derive(Debug, Default)]
pub struct RoadOutput {
    /// This frame's quads, back to front.
    pub quads: Vec<RoadQuad>,
    near: Vec<RoadQuad>,
    ranges: Vec<(usize, usize)>,
}

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

/// A trapezoid between two horizontal edges: `(left, right)` at `y_top`
/// and at `y_bottom`.
fn trap(out: &mut Vec<RoadQuad>, top: (f32, f32, f32), bottom: (f32, f32, f32), color: [f32; 4], fog: f32) {
    out.push(RoadQuad {
        corners: [[top.0, top.2], [top.1, top.2], [bottom.0, bottom.2], [bottom.1, bottom.2]],
        cell: FILL,
        top: color,
        bottom: color,
        fog,
    });
}

/// Backdrop silhouette height at `u` (one screen width = 1), 0..1. Whole
/// frequencies, so it tiles as it scrolls.
fn ridge(u: f32, seed: f32) -> f32 {
    let tau = core::f32::consts::TAU;
    0.55 + 0.25 * (tau * (2.0 * u + seed)).sin() + 0.13 * (tau * (5.0 * u + 0.37 + seed)).sin() + 0.07 * (tau * (11.0 * u + 0.71)).sin()
}

impl RoadScene {
    /// Project the scene for a `viewport` (CSS pixels) with `v` into
    /// `out.quads`, back to front: sky and backdrop, then every segment from
    /// the draw distance in, each followed by its sprites. Empty when the
    /// scene is hidden, the track is empty or the viewport has no size.
    pub fn build(&self, v: &RoadValues, viewport: [f32; 2], out: &mut RoadOutput) {
        out.quads.clear();
        out.near.clear();
        out.ranges.clear();
        let [w, h] = viewport;
        let n = self.track.len();
        if !self.visible || n == 0 || w < 1.0 || h < 1.0 {
            return;
        }
        let seg = v.segment_length.max(1.0);
        let depth = 1.0 / (v.fov_deg.clamp(10.0, 170.0).to_radians() * 0.5).tan();
        let (hw, hh) = (w * 0.5, h * 0.5);
        let look = &self.look;
        let z = self.camera.z.rem_euclid(self.track.length(seg));
        let Some((base, pct)) = self.track.locate(z, seg) else {
            return;
        };
        // The eye follows the ground where the car sits (the bottom of the
        // screen), as in the classic road.
        let ahead = v.camera_height * depth;
        let cam_y = self.track.height_at(z + ahead, seg) * seg * v.hill + v.camera_height;
        let cam_x = self.camera.x * v.road_width;

        // ---- sky and backdrop ----
        let fog_c = rgba(look.fog);
        out.quads.push(RoadQuad {
            corners: [[0.0, 0.0], [w, 0.0], [0.0, hh], [w, hh]],
            cell: FILL,
            top: rgba(look.sky_top),
            bottom: rgba(look.sky_horizon),
            fog: 0.0,
        });
        trap(&mut out.quads, (0.0, w, hh), (0.0, w, h), fog_c, 0.0);
        let turn = self.track.turn_at(self.camera.z, seg) * v.curve;
        for (color, amp, parallax, seed, fog) in [
            (look.hills_far, 0.16, 0.0004, 0.0, 0.45),
            (look.hills_near, 0.09, 0.0009, 0.43, 0.2),
        ] {
            const COLS: u32 = 32;
            let phase = turn * parallax;
            let y_at = |c: u32| hh - ridge(c as f32 / COLS as f32 + phase, seed) * amp * h;
            for c in 0..COLS {
                let (x0, x1) = (c as f32 / COLS as f32 * w, (c + 1) as f32 / COLS as f32 * w);
                out.quads.push(RoadQuad {
                    corners: [[x0, y_at(c)], [x1, y_at(c + 1)], [x0, h], [x1, h]],
                    cell: FILL,
                    top: rgba(color),
                    bottom: rgba(color),
                    fog,
                });
            }
        }

        // ---- segments, near to far (curves accumulate outwards) ----
        let draw = (v.draw_distance as usize).min(n);
        let near = 1.0;
        let hill = seg * v.hill;
        let lanes = self.lanes.max(1);
        let band = self.band.max(1) as usize;
        let mut x = 0.0;
        let mut dx = -(self.track.segments[base].curve * v.curve * pct);
        let mut max_y = h;
        for i in 0..draw {
            let idx = (base + i) % n;
            let s = &self.track.segments[idx];
            let z1 = (base + i) as f32 * seg - z;
            let z2 = z1 + seg;
            let (y1, y2) = (self.track.start_y(idx) * hill - cam_y, s.y * hill - cam_y);
            let (x1, x2) = (x - cam_x, x + dx - cam_x);
            x += dx;
            dx += s.curve * v.curve;
            let start = out.near.len();
            if z2 <= near {
                continue;
            }
            // Clip the near edge to the near plane.
            let (z1c, x1c, y1c) = if z1 < near {
                let t = (near - z1) / (z2 - z1);
                (near, lerp(x1, x2, t), lerp(y1, y2, t))
            } else {
                (z1, x1, y1)
            };
            let (s1, s2) = (depth / z1c, depth / z2);
            let (px1, py1, pw1) = (hw + s1 * x1c * hw, hh - s1 * y1c * hh, s1 * v.road_width * hw);
            let (px2, py2, pw2) = (hw + s2 * x2 * hw, hh - s2 * y2 * hh, s2 * v.road_width * hw);
            let d = i as f32 / draw as f32;
            let fog = 1.0 - (-(d * d * v.fog)).exp();
            // Ground, unless a nearer crest already hides all of it.
            if py2 < py1 && py2 < max_y {
                let k = usize::from((idx / band) % 2 == 1);
                let (r1, r2) = (pw1 / (2.0 * lanes as f32).max(6.0), pw2 / (2.0 * lanes as f32).max(6.0));
                trap(&mut out.near, (0.0, w, py2), (0.0, w, py1), rgba(look.grass[k]), fog);
                trap(&mut out.near, (px2 - pw2 - r2, px2 - pw2, py2), (px1 - pw1 - r1, px1 - pw1, py1), rgba(look.rumble[k]), fog);
                trap(&mut out.near, (px2 + pw2, px2 + pw2 + r2, py2), (px1 + pw1, px1 + pw1 + r1, py1), rgba(look.rumble[k]), fog);
                trap(&mut out.near, (px2 - pw2, px2 + pw2, py2), (px1 - pw1, px1 + pw1, py1), rgba(look.road[k]), fog);
                if k == 0 && lanes > 1 {
                    let (l1, l2) = (pw1 / (8.0 * lanes as f32).max(32.0), pw2 / (8.0 * lanes as f32).max(32.0));
                    for lane in 1..lanes {
                        let f = lane as f32 / lanes as f32 * 2.0 - 1.0;
                        let (c1, c2) = (px1 + f * pw1, px2 + f * pw2);
                        trap(&mut out.near, (c2 - l2 * 0.5, c2 + l2 * 0.5, py2), (c1 - l1 * 0.5, c1 + l1 * 0.5, py1), rgba(look.lane), fog);
                    }
                }
                max_y = py2;
            }
            // Sprites stand on the segment's near edge; painter's order lets
            // nearer ground hide their feet behind a crest.
            if z1 >= near {
                for sp in &s.sprites {
                    let sw = sp.size[0] * v.road_width * s1 * hw;
                    let sh = sp.size[1] * v.road_width * s1 * hw;
                    if sw < 0.25 || sh < 0.25 {
                        continue;
                    }
                    let out_shift = if sp.offset.abs() >= 1.0 { sp.offset.signum() * sw * 0.5 } else { 0.0 };
                    let cx = px1 + s1 * sp.offset * v.road_width * hw + out_shift;
                    let (l, r) = (cx - sw * 0.5, cx + sw * 0.5);
                    if r < 0.0 || l > w || py1 - sh > h {
                        continue;
                    }
                    out.near.push(RoadQuad {
                        corners: [[l, py1 - sh], [r, py1 - sh], [l, py1], [r, py1]],
                        cell: sp.cell,
                        top: sp.color,
                        bottom: sp.color,
                        fog,
                    });
                }
            }
            out.ranges.push((start, out.near.len()));
        }
        // Far to near.
        for &(a, b) in out.ranges.iter().rev() {
            out.quads.extend_from_slice(&out.near[a..b]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> RoadTrack {
        let mut t = RoadTrack::new();
        t.straight(20).bend(40, 4.0).hill(40, 20.0).bend(40, -3.0).close(40);
        let mut rng = Pcg32::seeded(7);
        t.scatter(&mut rng, 4, &[3, 5], [0.4, 0.8], [1.0; 4]);
        t
    }

    #[test]
    fn track_is_deterministic_and_closes() {
        let (a, b) = (track(), track());
        assert_eq!(a, b);
        assert_eq!(a.len(), 180);
        assert!(a.last_y().abs() < 1e-4);
        assert!(a.height_at(20.0 * 200.0 + 40.0 * 200.0 + 20.0 * 200.0, 200.0) > 5.0);
    }

    #[test]
    fn build_is_deterministic_back_to_front() {
        let mut scene = RoadScene::new(track());
        scene.camera.z = 12_345.0;
        let (mut o1, mut o2) = (RoadOutput::default(), RoadOutput::default());
        scene.build(&RoadValues::default(), [640.0, 360.0], &mut o1);
        scene.build(&RoadValues::default(), [640.0, 360.0], &mut o2);
        assert_eq!(o1.quads, o2.quads);
        assert!(o1.quads.len() > 66);
        // The sky comes first; the last ground quad reaches the screen bottom.
        assert_eq!(o1.quads[0].corners[0], [0.0, 0.0]);
        assert!(o1.quads.iter().any(|q| q.cell != FILL));
    }

    #[test]
    fn hidden_or_empty_scene_draws_nothing() {
        let mut out = RoadOutput::default();
        RoadScene::new(RoadTrack::new()).build(&RoadValues::default(), [640.0, 360.0], &mut out);
        assert!(out.quads.is_empty());
        let mut scene = RoadScene::new(track());
        scene.visible = false;
        scene.build(&RoadValues::default(), [640.0, 360.0], &mut out);
        assert!(out.quads.is_empty());
    }
}
