//! Drawing: the whole game is screen-space atlas sprites in CSS pixels.
//!
//! * Background: the look's clear colour, two big soft glows and a slow
//!   field of faint candy dots.
//! * Board tray: per playable cell an ink outline, a frame and a tile (the
//!   tray follows the board's shape, holes included), plus frosting.
//! * Pieces: contact shadow (down-right, away from the top-left light), ink
//!   outline, shade tone, lit tone offset up-left (two-tone cel shading),
//!   highlight; specials get ribbons, rings or a prism core.
//! * Juice (effect `juice`): squash and stretch on swaps, falls and
//!   landings, idle breathing, pop-and-spin clears with a shockwave ring.
//!   Off: pieces only move and vanish.
//! * Blasts (beams and rings, always, as they say what a special did), HDR
//!   so `bloom` catches them. The sparkles and confetti are not drawn here:
//!   the sim emits them into the shared GPU particle system
//!   (`crate::sparks`; effect `particles`).
//! * Puff the mascot beside the board, reacting to the game.
//! * Screen shake (`shake`) moves the whole board.
//!
//! Style numbers come from the look (`look.<name>.outline_px`, `shade`,
//! `highlight`, `squash`, `shadow`) and colours from its `pieces` and `ui`
//! palettes; without them the kit draws flat pieces with no outline.

use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::LookData;
use evani_render::atlas::cell;
use evani_render::{RenderWorld, Sprite, SpriteEdge};

use crate::board::{Kind, NO_COLOR, Piece};
use crate::board::Board;
use crate::ecs::{Blast, Grid, Mood, Motion, Popping, Puff};
use crate::game::{Game, Phase, Step};
use crate::layout::Layout;

/// Shape cell per piece colour: cherry circle, toffee square, lemon star,
/// mint diamond, berry triangle, grape donut (a circle with a hole).
pub const SHAPES: [u16; 6] = [cell::CIRCLE, cell::SQUARE, cell::STAR, cell::DIAMOND, cell::TRIANGLE, cell::CIRCLE];
/// Player-facing piece names.
pub const NAMES: [&str; 6] = ["Cherry", "Toffee", "Lemon", "Mint", "Berry", "Grape"];
/// Plain sharp-cornered rectangle (any atlas cell past the named shapes).
pub(crate) const RECT: u16 = 9;

/// Names the kit asks the look's sprite art for, one per piece colour
/// ([`NAMES`], lowercase) and the colourless prism: `piece.<name>`. A frame
/// must fill its sprite like the built-in shape of the same piece
/// ([`SHAPES`], [`shape_fill`]) so the ink outline, reach and layout hold.
pub const ART_NAMES: [&str; 7] = [
    "piece.cherry",
    "piece.toffee",
    "piece.lemon",
    "piece.mint",
    "piece.berry",
    "piece.grape",
    "piece.prism",
];
/// Index of the prism in [`ART_NAMES`] and [`Style::cells`].
const PRISM_ART: usize = 6;

/// Kit defaults when the look has no `pieces` palette (linear RGBA).
pub const DEFAULT_PIECES: [[f32; 4]; 6] = [
    [0.80, 0.06, 0.08, 1.0],
    [0.95, 0.40, 0.05, 1.0],
    [0.98, 0.80, 0.10, 1.0],
    [0.15, 0.70, 0.25, 1.0],
    [0.10, 0.35, 0.95, 1.0],
    [0.50, 0.15, 0.80, 1.0],
];

/// `ui` palette roles, in order.
pub const UI_ROLES: [&str; 8] = ["ink", "board_light", "board_dark", "frame", "bg_glow", "bg_dot", "mascot", "frosting"];
pub const DEFAULT_UI: [[f32; 4]; 8] = [
    [0.03, 0.02, 0.05, 1.0],
    [0.80, 0.80, 0.85, 1.0],
    [0.62, 0.62, 0.70, 1.0],
    [0.92, 0.92, 0.95, 1.0],
    [0.70, 0.80, 1.00, 1.0],
    [1.00, 1.00, 1.00, 1.0],
    [0.95, 0.55, 0.75, 1.0],
    [0.98, 0.90, 0.95, 1.0],
];

/// Drawing style: colours from the look's palettes, numbers from its params.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub pieces: [[f32; 4]; 6],
    pub ui: [[f32; 4]; 8],
    /// Ink outline width in CSS px (0 = none).
    pub outline_px: f32,
    /// Shade tone = colour x this (1 = flat, no cel split).
    pub shade: f32,
    /// Highlight opacity.
    pub highlight: f32,
    /// Squash-and-stretch amount (0..1.5).
    pub squash: f32,
    /// Contact shadow opacity.
    pub shadow: f32,
    /// The look draws the board tile in atlas cell 0.
    pub tile_from_look: bool,
    /// The sprite cell each piece is drawn with: the six colours, then the
    /// prism ([`ART_NAMES`]). The built-in shapes until [`Style::with_art`]
    /// finds the look's frames.
    pub cells: [u16; 7],
    /// The edge of each of [`Style::cells`]: how much of the ink outline
    /// its anti-aliased rim eats (soft built-in shapes, crisp look frames).
    pub edges: [SpriteEdge; 7],
}

/// The built-in shape of each entry of [`Style::cells`].
fn builtin_cells() -> [u16; 7] {
    let mut c = [cell::CIRCLE; 7];
    for (d, s) in c.iter_mut().zip(SHAPES) {
        *d = s;
    }
    c
}

impl Default for Style {
    fn default() -> Self {
        Self {
            pieces: DEFAULT_PIECES,
            ui: DEFAULT_UI,
            outline_px: 0.0,
            shade: 1.0,
            highlight: 0.0,
            squash: 0.5,
            shadow: 0.2,
            tile_from_look: false,
            cells: builtin_cells(),
            edges: [SpriteEdge::Builtin; 7],
        }
    }
}

/// The six piece colours: the look's `pieces` palette over the kit's
/// defaults (linear RGBA).
pub fn pieces_of(look: &LookData) -> [[f32; 4]; 6] {
    let mut out = DEFAULT_PIECES;
    if let Some(p) = look.palette("pieces") {
        for (d, c) in out.iter_mut().zip(p.colors.iter()) {
            *d = *c;
        }
    }
    out
}

impl Style {
    /// Read the look's palettes and `look.<name>.*` style params.
    pub fn from_look(look: &LookData, reg: &ParamRegistry) -> Self {
        let mut s = Self { pieces: pieces_of(look), ..Self::default() };
        if let Some(p) = look.palette("ui") {
            for (d, c) in s.ui.iter_mut().zip(p.colors.iter()) {
                *d = *c;
            }
            s.tile_from_look = true;
        }
        let f = |n: &str, d: f32| match reg.id(&format!("look.{}.{n}", look.name)).and_then(|id| reg.value(id)) {
            Some(ParamValue::F32(v)) => v,
            _ => d,
        };
        s.outline_px = f("outline_px", s.outline_px).max(0.0);
        s.shade = f("shade", s.shade).clamp(0.0, 1.0);
        s.highlight = f("highlight", s.highlight).clamp(0.0, 1.0);
        s.squash = f("squash", s.squash).clamp(0.0, 2.0);
        s.shadow = f("shadow", s.shadow).clamp(0.0, 1.0);
        s
    }

    /// Use the look's smooth frames for the pieces it has (`piece.cherry`
    /// and so on, [`ART_NAMES`]); any it lacks keep their built-in shape, so
    /// the kit works with every look.
    #[must_use]
    pub fn with_art(mut self, art: &evani_render::SpriteArt) -> Self {
        let builtin = builtin_cells();
        for (((c, e), name), b) in self.cells.iter_mut().zip(self.edges.iter_mut()).zip(ART_NAMES).zip(builtin) {
            *c = art.cell_or(name, b);
            *e = art.edge(*c);
        }
        self
    }

    fn art_index(pc: &Piece) -> usize {
        if pc.kind == Kind::Prism { PRISM_ART } else { usize::from(pc.color) }
    }

    /// The sprite cell piece `pc` is drawn with.
    fn piece_cell_of(&self, pc: &Piece) -> u16 {
        self.cells.get(Self::art_index(pc)).copied().unwrap_or(cell::CIRCLE)
    }

    /// The edge of the sprite piece `pc` is drawn with.
    fn piece_edge_of(&self, pc: &Piece) -> SpriteEdge {
        self.edges.get(Self::art_index(pc)).copied().unwrap_or(SpriteEdge::Builtin)
    }

    /// How much bigger (CSS px, in total) the ink copy behind a piece's
    /// sprite (`size` px wide) is than the sprite, so the visible ink is
    /// `outline_px` wide around its shape (which fills `fill` of the
    /// sprite, [`shape_fill`]) after the sprite's edge (`edge`,
    /// [`SpriteEdge::px`]) has eaten into it.
    pub fn ink_grow(&self, size: f32, fill: f32, edge: SpriteEdge) -> f32 {
        let o = self.outline_px;
        if o > 0.0 { 2.0 * (o + edge.px(size)) / fill } else { 0.0 }
    }

    fn color(&self, c: u8) -> [f32; 4] {
        self.pieces.get(c as usize).copied().unwrap_or([0.9, 0.9, 0.95, 1.0])
    }

    fn ink(&self) -> [f32; 4] {
        self.ui[0]
    }
}

/// What the draw system knows between frames.
#[derive(Debug, Default)]
pub struct DrawState {
    pub style: Option<Style>,
    /// Param revision the style was read at (live tuning re-reads it).
    pub style_rev: u64,
    pub sounds: Option<crate::sfx::Sounds>,
    /// Live captures: the attract board replayed to a held time, and that time.
    #[cfg(feature = "showcase")]
    pub showcase: Option<crate::Showcase>,
    /// Live captures: a scripted moment the draw system plays next frame
    /// (`crate::moments`).
    #[cfg(feature = "showcase")]
    pub stage: Option<crate::moments::Moment>,
    /// A staged moment is on screen while the clock is held (the attract
    /// replay stays off).
    #[cfg(feature = "showcase")]
    pub staged: bool,
    /// The staged moment's own clock (s since its level started, from the
    /// replay's tick count): the draw animates at this time while it is
    /// held, so a capture never depends on when it was taken.
    #[cfg(feature = "showcase")]
    pub moment_t: Option<f64>,
    /// Live captures: the `play` moment's best bot keeps playing in the
    /// running game (the review's run video) until the level ends, another
    /// moment is staged or the player touches anything (`crate::sim_system`).
    #[cfg(feature = "showcase")]
    pub autoplay: bool,
}

/// Effects that change the picture this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DrawFx {
    pub juice: bool,
    pub pop_scale: f32,
    /// Board shake offset in CSS px.
    pub shake: [f32; 2],
}

pub fn fx_on(world: &World, name: &str) -> bool {
    world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on(name))
}

pub fn param_f32(reg: &ParamRegistry, name: &str, d: f32) -> f32 {
    reg.f32_handle(name).map_or(d, |h| reg.f32(h))
}

/// Board shake offset (CSS px) at time `t` for screen-shake `trauma`
/// (0..1): `px` pixels at full trauma (amount x px per unit), falling off
/// with trauma squared, `freq` wobbles per second.
pub fn shake_offset(trauma: f32, t: f32, px: f32, freq: f32) -> [f32; 2] {
    if trauma <= 0.0 {
        return [0.0; 2];
    }
    let amp = px * trauma * trauma;
    let x = t * freq;
    [amp * noise1(x, 11), amp * noise1(x, 23)]
}

/// Smooth 1D value noise in -1..1 (for shake and wobble).
pub fn noise1(x: f32, seed: u32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let h = |n: i32| {
        let mut v = (n as u32).wrapping_mul(0x9E37_79B9) ^ seed.wrapping_mul(0x85EB_CA6B);
        v ^= v >> 16;
        v = v.wrapping_mul(0x7FEB_352D);
        v ^= v >> 15;
        (v & 0xFFFF) as f32 / 32767.5 - 1.0
    };
    let (a, b) = (h(i as i32), h(i as i32 + 1));
    let u = f * f * (3.0 - 2.0 * f);
    a + (b - a) * u
}

fn mul(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

fn alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

fn mix(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * k,
        a[1] + (b[1] - a[1]) * k,
        a[2] + (b[2] - a[2]) * k,
        a[3] + (b[3] - a[3]) * k,
    ]
}

fn grey(c: [f32; 4], k: f32) -> [f32; 4] {
    let l = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
    mix(c, [l, l, l, c[3]], k)
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn ease_in_out(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Layers, back to front (public for the geocheck scene dump).
pub mod layer {
    pub const BG: i16 = 0;
    pub const BG_DOTS: i16 = 1;
    pub const TRAY_SHADOW: i16 = 10;
    pub const TRAY_INK: i16 = 11;
    pub const TRAY: i16 = 12;
    pub const TILE: i16 = 13;
    pub const FROST: i16 = 14;
    pub const GLOW_UNDER: i16 = 18;
    /// The Pop Bomb's halo (ink rim, then the ring, in draw order), behind
    /// its piece so the piece's whole shape stays visible.
    pub const HALO: i16 = 19;
    pub const SHADOW: i16 = 20;
    pub const INK: i16 = 21;
    pub const SHADE: i16 = 22;
    pub const LIT: i16 = 23;
    pub const SHINE: i16 = 24;
    pub const DECO_INK: i16 = 25;
    pub const DECO: i16 = 26;
    pub const DECO_TOP: i16 = 27;
    pub const POP: i16 = 30;
    pub const BLAST: i16 = 40;
    pub const SPARK: i16 = 50;
    pub const CURSOR: i16 = 55;
    pub const MASCOT: i16 = 70;
}

struct Painter<'a> {
    rw: &'a mut RenderWorld,
}

impl Painter<'_> {
    #[allow(clippy::too_many_arguments)]
    fn put(&mut self, pos: [f32; 2], size: [f32; 2], rotation: f32, cell: u16, color: [f32; 4], layer: i16) {
        if color[3] <= 0.003 || size[0] <= 0.1 || size[1] <= 0.1 {
            return;
        }
        self.rw.draw_sprite(Sprite {
            pos,
            size,
            rotation,
            cell,
            color,
            layer,
        });
    }

    /// A crisp ring: `radius` (CSS px, to the middle of the band), `thick`
    /// px wide, built from tangent sharp-cornered rectangles (the atlas's
    /// soft ring shape blurs at large sizes and blooms into a glow). With
    /// `ink` px > 0 an ink rim goes under it on the same layer, drawn
    /// first. Segments whose middle falls outside `clip` (`[x, y, w, h]`)
    /// are left out, so a ring stays inside the board.
    #[allow(clippy::too_many_arguments)]
    fn ring(&mut self, centre: [f32; 2], radius: f32, thick: f32, color: [f32; 4], layer: i16, ink: f32, ink_color: [f32; 4], clip: Option<[f32; 4]>) {
        if radius <= 0.5 || thick <= 0.1 || color[3] <= 0.003 {
            return;
        }
        let n = ((core::f32::consts::TAU * radius / 7.0).ceil() as usize).clamp(20, 64);
        let step = core::f32::consts::TAU / n as f32;
        let tan = (step * 0.5).tan();
        let ink = ink.max(0.0);
        let inside = |q: [f32; 2]| {
            clip.is_none_or(|r| {
                let m = thick * 0.5;
                q[0] >= r[0] + m && q[0] <= r[0] + r[2] - m && q[1] >= r[1] + m && q[1] <= r[1] + r[3] - m
            })
        };
        for pass in 0..2 {
            let (w, col) = if pass == 0 { (thick + 2.0 * ink, ink_color) } else { (thick, color) };
            if pass == 0 && ink <= 0.0 {
                continue;
            }
            // Long enough to close the outer edge's joints.
            let len = 2.0 * (radius + w * 0.5) * tan + 0.6;
            for k in 0..n {
                let a = k as f32 * step;
                let q = [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()];
                if inside(q) {
                    self.put(q, [len, w], a + core::f32::consts::FRAC_PI_2, RECT, col, layer);
                }
            }
        }
    }
}

/// How a piece is drawn this frame.
#[derive(Debug, Clone, Copy)]
struct PieceDraw {
    pos: [f32; 2],
    /// Base size (px) before squash.
    size: f32,
    scale: [f32; 2],
    rot: f32,
    /// 0..1 towards white (pop flash).
    flash: f32,
    /// 0..1 towards grey (lost).
    grey: f32,
    opacity: f32,
    /// Lifted (selected): longer shadow.
    lift: f32,
    /// Layer offset (pops draw above the board).
    over: i16,
    /// Scale of a special's add-ons (band, arrows, halo, spark, core): 1
    /// at rest, the pop's scale while popping, [`entry_fit`] dropping in.
    deco: f32,
}

/// The atlas shape a piece is drawn with.
pub fn piece_shape(pc: &Piece) -> u16 {
    if pc.kind == Kind::Prism { cell::CIRCLE } else { SHAPES.get(pc.color as usize).copied().unwrap_or(cell::CIRCLE) }
}

/// How much of its sprite a shape fills (its visible width over the sprite
/// width).
pub fn shape_fill(shape: u16) -> f32 {
    match shape {
        cell::STAR => 0.75,
        cell::TRIANGLE => 0.7,
        cell::DIAMOND => 0.8,
        _ => 0.85,
    }
}

/// How far inside a `w` x `h` board a point in cell units (cell centres at
/// whole numbers) lies, in cells: the distance to the nearest board edge,
/// negative outside.
pub fn board_inset(pos: [f32; 2], w: usize, h: usize) -> f32 {
    let (w, h) = (w as f32, h as f32);
    (pos[0] + 0.5).min(w - 0.5 - pos[0]).min(pos[1] + 0.5).min(h - 0.5 - pos[1])
}

/// Opacity of a piece drawn at board row `row` (fractional): a piece
/// dropping in from above the board is hidden until its centre is well
/// inside the top row (row -0.3) and fades in by row -0.05. Together with
/// [`entry_fit`] nothing of it ever shows above the board's top edge
/// (sprites have no clipping).
pub fn enter_opacity(row: f32) -> f32 {
    ((row + ENTER_HIDDEN) / 0.25).clamp(0.0, 1.0)
}

/// Rows above row 0 at which a piece dropping in is still hidden.
const ENTER_HIDDEN: f32 = 0.3;

/// How far a piece's own sprites reach above its centre (CSS px) at full
/// size, not counting the ink outline: its shape (a shape fills
/// [`shape_fill`] of its sprite) at vertical scale `scale_y`, and its
/// special's add-ons (the Pop Bomb's halo, a vertical Streak's arrows).
pub fn piece_reach(pc: &Piece, size: f32, scale_y: f32) -> f32 {
    let shape = size * scale_y * shape_fill(piece_shape(pc)) * 0.5;
    let deco = match pc.kind {
        Kind::Bomb => size * (HALO_R * (1.0 + HALO_PULSE) + HALO_T * 0.5),
        Kind::StreakV => size * (ARROW_PUSH + ARROW_PULSE + ARROW_SIZE * 0.5 * shape_fill(cell::TRIANGLE)),
        _ => 0.0,
    };
    shape.max(deco)
}

/// The scale (0.05..1) a piece dropping in is drawn at so that none of it,
/// ink outline included, shows above the board's top edge `top` (CSS px):
/// its centre is at `y`, it reaches `reach` px up at full size
/// ([`piece_reach`]) and its outline adds `outline_px` + 1 px.
pub fn entry_fit(y: f32, top: f32, reach: f32, outline_px: f32) -> f32 {
    if reach <= 0.0 {
        return 1.0;
    }
    ((y - top - outline_px - 1.0) / reach).clamp(0.05, 1.0)
}

/// Pop Bomb halo: centre-line radius, thickness and pulse, in piece sizes.
const HALO_R: f32 = 0.5;
const HALO_T: f32 = 0.07;
const HALO_PULSE: f32 = 0.04;
/// Streak arrows: distance from the centre, pulse and size, in piece sizes.
const ARROW_PUSH: f32 = 0.40;
const ARROW_PULSE: f32 = 0.04;
const ARROW_SIZE: f32 = 0.3;

/// The Pop Bomb blast's rings: warm white, just over the look's bloom
/// threshold so they glow a little without blurring (linear).
pub const BOMB_RING: [f32; 4] = [1.35, 1.25, 1.05, 1.0];

/// The keyboard cursor's ring (linear, a warm glow over the pieces).
pub const CURSOR_RING: [f32; 4] = [1.6, 1.5, 1.0, 1.0];

/// A piece's drawn size, in cells.
pub const PIECE_SIZE: f32 = 0.84;
/// Most a falling piece stretches (times the style's `squash`).
pub const FALL_STRETCH: f32 = 0.16;

/// How far above the board's top edge (CSS px) the board shows. Pieces
/// dropping in never cross that edge ([`enter_opacity`], [`entry_fit`]), so
/// it is the tray's rim: its frame ([`TRAY_PAD`] cells per cell) and ink
/// outline. Popups that sit above the board keep clear of it
/// (`hud::callout_anchor`).
pub fn entry_reach(cell: f32, st: &Style) -> f32 {
    (TRAY_PAD - 1.0) * 0.5 * cell + st.outline_px
}

/// The tray's frame square per cell, in cells.
pub const TRAY_PAD: f32 = 1.32;

/// Draw one piece's full stack.
fn piece(p: &mut Painter, st: &Style, pc: &Piece, d: PieceDraw, t: f32) {
    let s = d.size;
    let w = s * d.scale[0];
    let h = s * d.scale[1];
    // The built-in shape sets the geometry (fill, reach); the sprite drawn
    // may be the look's smooth frame of the same silhouette.
    let shape = piece_shape(pc);
    let art = st.piece_cell_of(pc);
    let base = if pc.kind == Kind::Prism { [0.16, 0.08, 0.30, 1.0] } else { st.color(pc.color) };
    let base = grey(mix(base, WHITE, d.flash), d.grey);
    let ink = alpha(st.ink(), d.opacity);
    let o = st.outline_px;
    // Shapes fill less of their cell than a circle does; scale the outline
    // so it looks the same width on every shape.
    let fill = shape_fill(shape);
    // The sprite's soft edge eats into a thin rim (about a pixel for the
    // built-in shapes, under half for the look's crisp frames); grow by
    // that much more so the visible ink is `outline_px` wide.
    let grow = st.ink_grow(s, fill, st.piece_edge_of(pc));
    let (ox, oy) = ((0.06 + 0.05 * d.lift) * s, (0.08 + 0.07 * d.lift) * s);
    let c = d.pos;
    let rot = d.rot;
    // Contact shadow, away from the top-left light.
    p.put([c[0] + ox, c[1] + oy], [w, h], rot, art, alpha(st.ink(), st.shadow * d.opacity), layer::SHADOW + d.over);
    if o > 0.0 {
        p.put(c, [w + grow, h + grow], rot, art, ink, layer::INK + d.over);
    }
    let shade = mul(base, st.shade);
    p.put(c, [w, h], rot, art, alpha(shade, d.opacity), layer::SHADE + d.over);
    if st.shade < 0.999 {
        // Lit tone: smaller and up-left, so a crescent of shade stays.
        let k = 0.86;
        let off = [-0.05 * w, -0.05 * h];
        let (sn, cs) = d.rot.sin_cos();
        let off = [off[0] * cs - off[1] * sn, off[0] * sn + off[1] * cs];
        p.put([c[0] + off[0], c[1] + off[1]], [w * k, h * k], rot, art, alpha(base, d.opacity), layer::LIT + d.over);
    }
    if st.highlight > 0.0 {
        let hp = [c[0] - 0.2 * w, c[1] - 0.22 * h];
        p.put(hp, [0.24 * w, 0.14 * h], -0.6, cell::CIRCLE, alpha(WHITE, st.highlight * d.opacity), layer::SHINE + d.over);
    }
    // Grape donut: the hole.
    if pc.kind != Kind::Prism && pc.color == 5 {
        let hs = 0.34 * s;
        if o > 0.0 {
            p.put(c, [hs * d.scale[0] + o * 1.6, hs * d.scale[1] + o * 1.6], 0.0, cell::CIRCLE, ink, layer::DECO_INK + d.over);
        }
        p.put([c[0] + 0.02 * s, c[1] + 0.02 * s], [hs * d.scale[0], hs * d.scale[1]], 0.0, cell::CIRCLE, alpha(mul(base, 0.45), d.opacity), layer::DECO + d.over);
    }
    match pc.kind {
        Kind::StreakH | Kind::StreakV => {
            let vertical = pc.kind == Kind::StreakV;
            let r = if vertical { core::f32::consts::FRAC_PI_2 } else { 0.0 } + d.rot;
            let s = s * d.deco;
            let band = [s * 0.98, s * 0.2];
            if o > 0.0 {
                p.put(c, [band[0] + o * 2.0, band[1] + o * 2.0], r, RECT, ink, layer::DECO_INK + d.over);
            }
            p.put(c, band, r, RECT, alpha(mix(WHITE, base, 0.15), d.opacity), layer::DECO + d.over);
            // Arrows at both ends, pulsing outwards, inside the piece's cell.
            let push = ARROW_PUSH * s + ARROW_PULSE * s * (t * 6.0).sin().abs();
            let (dx, dy) = if vertical { (0.0, 1.0) } else { (1.0, 0.0) };
            for sign in [-1.0f32, 1.0] {
                let ap = [c[0] + dx * push * sign, c[1] + dy * push * sign];
                let ar = if vertical {
                    if sign > 0.0 { core::f32::consts::PI } else { 0.0 }
                } else if sign > 0.0 {
                    core::f32::consts::FRAC_PI_2
                } else {
                    -core::f32::consts::FRAC_PI_2
                };
                let a = s * ARROW_SIZE;
                if o > 0.0 {
                    p.put(ap, [a + o * 2.5, a + o * 2.5], ar, cell::TRIANGLE, ink, layer::DECO_INK + d.over);
                }
                p.put(ap, [a, a], ar, cell::TRIANGLE, alpha(WHITE, d.opacity), layer::DECO_TOP + d.over);
            }
        }
        Kind::Bomb => {
            // A thin white halo with an ink rim, behind the piece and outside
            // its outline, with no filled centre: the piece keeps its whole
            // shape and reads as one piece with a bomb on it.
            let s = s * d.deco;
            let pulse = 1.0 + HALO_PULSE * (t * 5.0).sin();
            let halo = mix(WHITE, base, 0.12);
            p.ring(c, s * HALO_R * pulse, s * HALO_T, alpha(halo, d.opacity), layer::HALO + d.over, (o * 0.8).max(1.0), ink, None);
            // Fuse spark lit on the halo at the top right, HDR so it glows.
            let fa = -core::f32::consts::FRAC_PI_4;
            let sp = [c[0] + fa.cos() * s * HALO_R * pulse, c[1] + fa.sin() * s * HALO_R * pulse];
            let tw = 0.8 + 0.4 * (t * 13.0).sin().abs();
            p.put(sp, [0.26 * s * tw, 0.26 * s * tw], t * 3.0, cell::STAR, [2.6, 2.2, 0.9, d.opacity], layer::DECO_TOP + d.over);
        }
        Kind::Prism => {
            // Six colour beads orbiting inside a dark sphere, a hot core.
            for k in 0..6 {
                let a = t * 1.6 + k as f32 * core::f32::consts::TAU / 6.0;
                let bp = [c[0] + a.cos() * 0.24 * w, c[1] + a.sin() * 0.24 * h];
                let bc = mul(st.color(k as u8), 1.35);
                p.put(bp, [0.2 * s * d.deco, 0.2 * s * d.deco], 0.0, cell::CIRCLE, alpha(bc, d.opacity), layer::DECO + d.over);
            }
            let tw = 0.85 + 0.15 * (t * 7.0).sin();
            let s = s * d.deco;
            p.put(c, [0.36 * s * tw, 0.36 * s * tw], t * 0.8, cell::STAR, [3.2, 3.0, 3.4, d.opacity], layer::DECO_TOP + d.over);
            p.put(c, [1.2 * s, 1.2 * s], 0.0, cell::GLOW, [0.9, 0.6, 1.4, 0.35 * d.opacity], layer::GLOW_UNDER);
        }
        Kind::Normal => {}
    }
}

/// Visual cell position of a piece on the board (fractional cells).
pub fn piece_cell(board: &Board, step: Step, i: usize, m: &Motion, swap_s: f32, shuffle_s: f32, invalid_slide: f32) -> [f32; 2] {
    let (x, y) = board.xy(i);
    let mut v = [x as f32 + m.off[0], y as f32 + m.off[1]];
    if m.swap_t >= 0.0 {
        let dur = if matches!(step, Step::Shuffle { .. }) { shuffle_s } else { swap_s };
        let k = 1.0 - ease_in_out(m.swap_t / dur.max(0.01));
        v[0] += m.swap_from[0] * k;
        v[1] += m.swap_from[1] * k;
    }
    if let Step::Invalid { a, b, t } = step
        && (i == a || i == b)
    {
        let other = if i == a { b } else { a };
        let (ox, oy) = board.xy(other);
        let k = (t / (swap_s * 2.0)).clamp(0.0, 1.0);
        let slide = (k * core::f32::consts::PI).sin() * invalid_slide;
        v[0] += (ox as f32 - x as f32) * slide;
        v[1] += (oy as f32 - y as f32) * slide;
    }
    v
}

/// Everything the draw needs besides the game.
pub struct Frame<'a> {
    pub layout: &'a Layout,
    pub style: &'a Style,
    pub fx: DrawFx,
    pub t: f32,
    pub tune: &'a crate::Tune,
}

/// Draw the background (independent of the board).
fn background(p: &mut Painter, f: &Frame) {
    let [vw, vh] = f.layout.viewport;
    let ui = &f.style.ui;
    let g = ui[4];
    p.put([vw * 0.22, vh * 0.12], [vw * 1.3, vh * 1.1], 0.0, cell::GLOW, alpha(g, 0.55), layer::BG);
    p.put([vw * 0.85, vh * 0.95], [vw * 1.1, vh * 0.9], 0.0, cell::GLOW, alpha(mix(g, ui[6], 0.5), 0.35), layer::BG);
    // Slow diagonal field of candy dots.
    let step = (vw.min(vh) * 0.09).clamp(48.0, 120.0);
    let drift = (f.t * 6.0) % (step * 2.0);
    let dot = alpha(ui[5], 0.13);
    let mut row = 0;
    let mut y = -step;
    while y < vh + step {
        let shift = if row % 2 == 0 { 0.0 } else { step * 0.5 };
        let mut x = -step + shift + drift * 0.5;
        while x < vw + step {
            p.put([x, y + drift * 0.25], [step * 0.22, step * 0.22], 0.0, cell::CIRCLE, dot, layer::BG_DOTS);
            x += step;
        }
        y += step * 0.75;
        row += 1;
    }
}

/// The tray under the board, following the board's shape.
fn tray(p: &mut Painter, board: &Board, f: &Frame, origin: [f32; 2]) {
    let l = f.layout;
    let c = l.cell;
    let st = f.style;
    let o = st.outline_px;
    let at = |x: i32, y: i32| [origin[0] + (x as f32 + 0.5) * c, origin[1] + (y as f32 + 0.5) * c];
    let tile_cell = if st.tile_from_look { cell::CHECKER } else { cell::SQUARE };
    let tile_k = if st.tile_from_look { 0.97 } else { 1.12 };
    for i in 0..board.len() {
        if !board.mask[i] {
            continue;
        }
        let (x, y) = board.xy(i);
        let pos = at(x, y);
        // RECT cells overlap into one shape per layer.
        let pad = c * TRAY_PAD;
        p.put([pos[0] + c * 0.08, pos[1] + c * 0.12], [pad, pad], 0.0, cell::SQUARE, alpha(st.ink(), 0.28), layer::TRAY_SHADOW);
        p.put(pos, [pad + 2.0 * o, pad + 2.0 * o], 0.0, cell::SQUARE, st.ink(), layer::TRAY_INK);
        p.put(pos, [pad, pad], 0.0, cell::SQUARE, st.ui[3], layer::TRAY);
        let tone = if (x + y) % 2 == 0 { st.ui[1] } else { st.ui[2] };
        p.put(pos, [c * tile_k, c * tile_k], 0.0, tile_cell, tone, layer::TILE);
        let frost = board.frost[i];
        if frost > 0 {
            let fc = st.ui[7];
            let fc = if frost >= 2 { mix(fc, [0.95, 0.45, 0.65, 1.0], 0.45) } else { fc };
            p.put([pos[0] + c * 0.03, pos[1] + c * 0.05], [c * 1.04, c * 1.04], 0.0, cell::SQUARE, alpha(mul(fc, 0.7), 0.9), layer::FROST);
            p.put(pos, [c * 1.0, c * 1.0], 0.0, cell::SQUARE, fc, layer::FROST);
            // Sprinkles, the same on every frame.
            for k in 0..4u32 {
                let hsh = (i as u32 * 7 + k * 13).wrapping_mul(2_654_435_761);
                let sx = ((hsh >> 8) % 100) as f32 / 100.0 - 0.5;
                let sy = ((hsh >> 16) % 100) as f32 / 100.0 - 0.5;
                let col = st.color((hsh % 6) as u8);
                let r = ((hsh >> 4) % 628) as f32 / 100.0;
                p.put([pos[0] + sx * c * 0.75, pos[1] + sy * c * 0.75], [c * 0.16, c * 0.05], r, RECT, col, layer::FROST);
            }
        }
    }
}

/// Draw one frame of the game into the render world.
pub fn draw_game(rw: &mut RenderWorld, w: &World, f: &Frame) {
    let (Some(g), Some(grid)) = (w.try_res::<Game>(), w.try_res::<Grid>()) else {
        return;
    };
    let board = &grid.board;
    let mut p = Painter { rw };
    background(&mut p, f);
    let l = f.layout;
    let c = l.cell;
    let tune = f.tune;
    let origin = [l.origin[0] + f.fx.shake[0], l.origin[1] + f.fx.shake[1]];
    tray(&mut p, board, f, origin);
    let to_px = |v: [f32; 2]| [origin[0] + (v[0] + 0.5) * c, origin[1] + (v[1] + 0.5) * c];
    let lost = if g.phase == Phase::Lost { (g.phase_t * 2.0).min(0.7) } else { 0.0 };
    let size = c * PIECE_SIZE;
    let sq = if f.fx.juice { f.style.squash } else { 0.0 };
    let hint = g.hint.filter(|_| g.phase == Phase::Play && g.step == Step::Idle);
    // Pieces in cell order, through their entities.
    for i in 0..board.len() {
        let Some(e) = grid.slots.get(i).copied().flatten() else {
            continue;
        };
        let (Some(pc), Some(m)) = (w.get::<Piece>(e).map(|p| *p), w.get::<Motion>(e).map(|m| *m)) else {
            continue;
        };
        let (pc, m) = (&pc, &m);
        let v = piece_cell(board, g.step, i, m, tune.swap_s, tune.shuffle_s, tune.invalid_slide);
        let enter = enter_opacity(v[1]);
        if enter <= 0.0 {
            continue;
        }
        let mut pos = to_px(v);
        let mut scale = [1.0f32, 1.0];
        let mut rot = 0.0;
        let mut lift = 0.0;
        // Squash and stretch.
        if sq > 0.0 {
            if m.off[1] < 0.0 {
                let k = (m.vy / tune.fall_speed.max(0.1)).clamp(0.0, 1.0) * FALL_STRETCH * sq;
                scale = [1.0 - k * 0.7, 1.0 + k];
            } else if m.land > 0.0 {
                let u = 1.0 - m.land / tune.land_squash_s.max(0.01);
                let k = (u * core::f32::consts::PI).sin() * 0.22 * sq;
                scale = [1.0 + k * 0.8, 1.0 - k];
                pos[1] += size * k * 0.5;
            } else if m.swap_t >= 0.0 {
                let u = (m.swap_t / tune.swap_s.max(0.01)).clamp(0.0, 1.0);
                let k = (u * core::f32::consts::PI).sin() * 0.18 * sq;
                if m.swap_from[0].abs() > m.swap_from[1].abs() {
                    scale = [1.0 + k, 1.0 - k * 0.6];
                } else {
                    scale = [1.0 - k * 0.6, 1.0 + k];
                }
            } else {
                // Idle breathing, out of step across the board.
                let (x, y) = board.xy(i);
                let k = (f.t * 2.2 + (x * 3 + y * 5) as f32 * 0.37).sin() * 0.018 * sq;
                scale = [1.0 + k, 1.0 - k];
            }
            if m.born >= 0.0 {
                let u = (m.born / 0.5).clamp(0.0, 1.0);
                let k = 1.0 + 0.6 * (1.0 - u) * (u * 9.0).cos();
                scale = [scale[0] * k, scale[1] * k];
            }
        }
        if g.selected == Some(i) {
            lift = 1.0;
            let bob = (f.t * 5.0).sin().abs();
            pos[1] -= size * 0.07 * bob;
            scale = [scale[0] * 1.08, scale[1] * 1.08];
            let pulse = 0.55 + 0.25 * (f.t * 6.0).sin();
            p.put(to_px([v[0], v[1]]), [c * 1.45, c * 1.45], 0.0, cell::GLOW, [1.6, 1.5, 1.2, pulse], layer::GLOW_UNDER);
        }
        if let Some(m) = hint
            && (m.a == i || m.b == i)
        {
            let other = if m.a == i { m.b } else { m.a };
            let (ox, oy) = board.xy(other);
            let (x, y) = board.xy(i);
            let ph = (f.t * 1.6).fract();
            let wob = if ph < 0.45 { (ph / 0.45 * core::f32::consts::TAU * 2.0).sin() * 0.09 } else { 0.0 };
            pos[0] += (ox - x) as f32 * c * wob;
            pos[1] += (oy - y) as f32 * c * wob;
            p.put(to_px(v), [c * 1.3, c * 1.3], 0.0, cell::GLOW, [1.4, 1.4, 1.1, 0.45], layer::GLOW_UNDER);
        }
        if pc.kind == Kind::Bomb && sq > 0.0 {
            rot = (f.t * 3.0).sin() * 0.05;
        }
        // Dropping in (also a falling piece stretched in the top row): shrink
        // so nothing shows above the board's top edge.
        let mut deco = 1.0;
        if v[1] < 0.0 || (m.off[1] < 0.0 && v[1] < 1.0) {
            let k = entry_fit(pos[1], origin[1], piece_reach(pc, size, scale[1]), f.style.outline_px);
            scale = [scale[0] * k, scale[1] * k];
            deco = k;
        }
        piece(
            &mut p,
            f.style,
            pc,
            PieceDraw {
                pos,
                size,
                scale,
                rot,
                flash: 0.0,
                grey: lost,
                opacity: enter,
                lift,
                over: 0,
                deco,
            },
            f.t,
        );
    }
    // Pops (already off the board).
    for &e in &grid.pops {
        if let (Some(pc), Some(pop)) = (w.get::<Piece>(e).map(|p| *p), w.get::<Popping>(e).map(|p| *p)) {
            draw_pop(&mut p, f, &pc, &pop, to_px([pop.x, pop.y]), size);
        }
    }
    // Under the title the attract board stays calm: its pieces pop, but
    // without blasts or sparkles (the sim emits none), which are bright
    // enough to clutter the title logo drawn over it.
    let calm = g.phase == Phase::Title;
    for &e in grid.blasts.iter().filter(|_| !calm) {
        if let Some(b) = w.get::<Blast>(e).map(|b| *b) {
            draw_blast(&mut p, board, f, &b, origin);
        }
    }
    // Keyboard cursor.
    if g.cursor_visible && matches!(g.phase, Phase::Play) {
        let (x, y) = board.xy(g.cursor);
        let pos = to_px([x as f32, y as f32]);
        let pulse = 1.0 + 0.05 * (f.t * 6.0).sin();
        let r = c * 1.08 * pulse;
        p.put(pos, [r + 4.0, r + 4.0], 0.0, cell::RING, f.style.ink(), layer::CURSOR);
        p.put(pos, [r, r], 0.0, cell::RING, CURSOR_RING, layer::CURSOR);
    }
    mascot(&mut p, grid.puff(w), f);
}

fn draw_pop(p: &mut Painter, f: &Frame, pc: &Piece, pop: &Popping, pos: [f32; 2], size: f32) {
    let clear_s = f.tune.clear_s.max(0.01);
    if pop.t < 0.0 {
        // Waiting for its chain delay: still drawn in place.
        piece(
            p,
            f.style,
            pc,
            PieceDraw {
                pos,
                size,
                scale: [1.0, 1.0],
                rot: 0.0,
                flash: 0.0,
                grey: 0.0,
                opacity: 1.0,
                lift: 0.0,
                over: 0,
                deco: 1.0,
            },
            f.t,
        );
        return;
    }
    let u = (pop.t / clear_s).clamp(0.0, 1.0);
    let (k, rot, flash) = if f.fx.juice {
        let up = 0.35;
        let k = if u < up {
            1.0 + (f.fx.pop_scale - 1.0) * (u / up)
        } else {
            f.fx.pop_scale * (1.0 - (u - up) / (1.0 - up)).powf(1.4)
        };
        (k, u * u * 2.4, (1.0 - u / 0.4).clamp(0.0, 1.0) * 0.8)
    } else {
        (1.0 - u, 0.0, 0.0)
    };
    piece(
        p,
        f.style,
        pc,
        PieceDraw {
            pos,
            size,
            scale: [k, k],
            rot,
            flash,
            grey: 0.0,
            opacity: (1.0 - u * u).max(0.0),
            lift: 0.0,
            over: layer::POP - layer::SHADOW,
            deco: k,
        },
        f.t,
    );
    if f.fx.juice && pc.kind != Kind::Prism {
        // Shockwave ring in the piece colour.
        let c = mul(f.style.color(pc.color), 1.6);
        let r = size * (0.7 + 1.0 * u);
        p.put(pos, [r, r], 0.0, cell::RING, alpha(c, (1.0 - u) * 0.8), layer::BLAST);
    }
}

fn draw_blast(p: &mut Painter, board: &Board, f: &Frame, b: &Blast, origin: [f32; 2]) {
    if b.t <= 0.0 {
        return;
    }
    let l = f.layout;
    let c = l.cell;
    let u = (b.t / 0.6).clamp(0.0, 1.0);
    let fade = (1.0 - u).powf(1.3);
    let center = [origin[0] + (b.x + 0.5) * c, origin[1] + (b.y + 0.5) * c];
    let col = if b.color == NO_COLOR { [1.0, 0.8, 1.0, 1.0] } else { f.style.color(b.color) };
    let hot = mul(col, 2.6);
    let (bw, bh) = (board.w as f32 * c, board.h as f32 * c);
    let clip = [origin[0], origin[1], bw, bh];
    let ink = f.style.ink();
    let o = f.style.outline_px;
    let beam =|p: &mut Painter, vertical: bool, at: f32, width: f32| {
        // Grows out from the special, then thins.
        let grow = (b.t / 0.12).min(1.0);
        // Centred on the board so the beam never leaves it (sprites have no clipping).
        let (len, pos) = if vertical {
            ((bh + c * 0.3) * grow, [origin[0] + (at + 0.5) * c, origin[1] + bh * 0.5])
        } else {
            ((bw + c * 0.3) * grow, [origin[0] + bw * 0.5, origin[1] + (at + 0.5) * c])
        };
        let r = if vertical { core::f32::consts::FRAC_PI_2 } else { 0.0 };
        p.put(pos, [len, c * 0.9 * width * fade], r, RECT, alpha(hot, 0.65 * fade), layer::BLAST);
        p.put(pos, [len, c * 0.3 * width * fade], r, RECT, [3.5, 3.4, 3.2, fade], layer::BLAST);
    };
    match (b.kind, b.combo) {
        (k, Some(c2)) if k.is_streak() && c2.is_streak() => {
            beam(p, false, b.y, 1.2);
            beam(p, true, b.x, 1.2);
        }
        (Kind::Bomb, Some(c2)) if c2.is_streak() => {
            for d in -1..=1 {
                beam(p, false, b.y + d as f32, 1.0);
                beam(p, true, b.x + d as f32, 1.0);
            }
        }
        (Kind::StreakH, _) => beam(p, false, b.y, 1.0),
        (Kind::StreakV, _) => beam(p, true, b.x, 1.0),
        (Kind::Bomb, combo) => {
            // Two crisp, flat, warm-white rings with ink rims, the second a
            // step behind the first, thinning as they spread; kept inside
            // the board. Only a little over the bloom threshold.
            let big = if combo == Some(Kind::Bomb) { 2.5 } else { 1.5 };
            for (lag, k) in [(0.0f32, 1.0f32), (0.2, 0.8)] {
                let uk = ((u - lag) / (1.0 - lag)).clamp(0.0, 1.0);
                if uk <= 0.0 || uk >= 1.0 {
                    continue;
                }
                let e = 1.0 - (1.0 - uk) * (1.0 - uk);
                let r = c * (0.55 + big * k * e);
                let th = c * 0.13 * (1.0 - uk * 0.75);
                let a = ((1.0 - uk) / 0.2).min(1.0);
                p.ring(center, r, th, alpha(BOMB_RING, a), layer::BLAST + 1, (o * 0.7).max(1.0), alpha(ink, a), Some(clip));
            }
            // A short warm flash on the bomb's own cell.
            let fl = 1.0 - (b.t / 0.16).min(1.0);
            p.put(center, [c * (0.9 + 0.3 * (1.0 - fl)), c * (0.9 + 0.3 * (1.0 - fl))], 0.0, cell::CIRCLE, alpha([1.5, 1.4, 1.1, 1.0], fl * 0.9), layer::BLAST + 2);
        }
        (Kind::Prism, _) => {
            // Six staggered rainbow rings in the piece colours, crisp with a
            // thin ink rim, kept inside the board, and a short white flash on
            // the Prism's own cell.
            for k in 0..6 {
                let uk = ((u - k as f32 * 0.07) / 0.6).clamp(0.0, 1.0);
                if uk <= 0.0 || uk >= 1.0 {
                    continue;
                }
                let e = 1.0 - (1.0 - uk) * (1.0 - uk);
                let r = c * (0.6 + 4.5 * e);
                let th = c * 0.11 * (1.0 - uk * 0.6);
                let a = ((1.0 - uk) / 0.25).min(1.0);
                let ck = mul(f.style.color(k as u8), 1.15);
                p.ring(center, r, th, alpha(ck, a), layer::BLAST + 1, (o * 0.5).max(1.0), alpha(ink, a), Some(clip));
            }
            let fl = 1.0 - (b.t / 0.18).min(1.0);
            p.put(center, [c * 1.1, c * 1.1], 0.0, cell::CIRCLE, alpha([1.6, 1.55, 1.7, 1.0], fl), layer::BLAST + 2);
        }
        _ => {}
    }
}

/// Puff blinks for `BLINK_LEN` seconds every `BLINK_PERIOD` seconds of the
/// animation clock (a staged moment steps past a blink, `crate::draw_system`).
pub const BLINK_PERIOD: f64 = 3.7;
pub const BLINK_LEN: f64 = 0.12;

/// The animation clock for a staged moment at replay time `mt`: stepped past
/// a blink, so the review picture never catches Puff with closed eyes.
#[cfg(feature = "showcase")]
pub fn moment_clock(mt: f64) -> f64 {
    let into = mt % BLINK_PERIOD;
    // One 60 Hz frame past the blink's end, clear of rounding at its edge.
    if into < BLINK_LEN { mt - into + BLINK_LEN + 1.0 / 60.0 } else { mt }
}

/// Puff: a round cloud-candy creature beside the board.
fn mascot(p: &mut Painter, puff: Puff, f: &Frame) {
    let [cx, gy, m] = f.layout.mascot;
    if m < 24.0 {
        return;
    }
    let st = f.style;
    let ink = st.ink();
    let o = st.outline_px.max(m * 0.022);
    let t = f.t;
    let mt = puff.mood_t;
    let juice = f.fx.juice;
    // Hop height and squash by mood.
    let (hop, sy, mood) = {
        let bounce = |rate: f32, h: f32| ((mt * rate).sin().abs()) * h;
        match puff.mood {
            Mood::Happy if juice => (bounce(9.0, 0.12) * (1.0 - mt / 1.2).max(0.0), 1.0, puff.mood),
            Mood::Excited if juice => (bounce(10.0, 0.2), 1.0, puff.mood),
            Mood::Cheer if juice => (bounce(7.0, 0.3), 1.0, puff.mood),
            Mood::Sad => (0.0, 0.9, puff.mood),
            m => (0.0, 1.0 + (t * 2.0).sin() * 0.025, m),
        }
    };
    let hop_px = hop * m;
    // Squash on landing between hops.
    let land = if hop < 0.02 && juice && matches!(mood, Mood::Excited | Mood::Cheer) { 0.12 } else { 0.0 };
    let (bw, bh) = (m * (1.0 + land * 0.8), m * 0.92 * (sy - land));
    // His lowest ink (body outline and soles) stands on the ground line,
    // lifted by the hop.
    let base = gy - hop_px;
    let body = [cx + if mood == Mood::Worried { (t * 20.0).sin() * m * 0.015 } else { 0.0 }, base - bh * 0.5 - o * 1.2];
    // Ground shadow (centred on the ground line) shrinks as Puff hops.
    let sh = 1.0 - hop * 1.5;
    p.put([cx + m * 0.06, gy], [m * 0.85 * sh, m * 0.18 * sh], 0.0, cell::CIRCLE, alpha(ink, 0.25), layer::MASCOT);
    let col = st.ui[6];
    // Little feet.
    for s in [-1.0f32, 1.0] {
        let fp = [body[0] + s * m * 0.22, base - m * 0.09 - o];
        p.put(fp, [m * 0.28 + o * 2.0, m * 0.18 + o * 2.0], 0.0, cell::CIRCLE, ink, layer::MASCOT + 1);
        p.put(fp, [m * 0.28, m * 0.18], 0.0, cell::CIRCLE, mul(col, st.shade.max(0.5)), layer::MASCOT + 2);
    }
    // Arms: up when cheering, waving when excited.
    let arm_up = matches!(mood, Mood::Cheer | Mood::Excited);
    for s in [-1.0f32, 1.0] {
        let wave = if arm_up { (t * 12.0 + s).sin() * 0.08 } else { 0.0 };
        let ap = if arm_up {
            [body[0] + s * bw * 0.52, body[1] - bh * (0.3 + wave)]
        } else {
            [body[0] + s * bw * 0.5, body[1] + bh * 0.1]
        };
        p.put(ap, [m * 0.22 + o * 2.0, m * 0.22 + o * 2.0], 0.0, cell::CIRCLE, ink, layer::MASCOT + 1);
        p.put(ap, [m * 0.22, m * 0.22], 0.0, cell::CIRCLE, col, layer::MASCOT + 2);
    }
    // Body: outline, shade, lit (top-left light), highlight.
    p.put(body, [bw + o * 2.4, bh + o * 2.4], 0.0, cell::CIRCLE, ink, layer::MASCOT + 3);
    p.put(body, [bw, bh], 0.0, cell::CIRCLE, mul(col, st.shade), layer::MASCOT + 4);
    p.put([body[0] - bw * 0.05, body[1] - bh * 0.05], [bw * 0.88, bh * 0.88], 0.0, cell::CIRCLE, col, layer::MASCOT + 5);
    p.put([body[0] - bw * 0.22, body[1] - bh * 0.28], [bw * 0.22, bh * 0.12], -0.6, cell::CIRCLE, alpha(WHITE, st.highlight.max(0.35)), layer::MASCOT + 6);
    // A candy star sprout on top.
    let sprout = [body[0] + bw * 0.08, body[1] - bh * 0.56 - m * 0.04 * (t * 3.0).sin()];
    p.put(sprout, [m * 0.3 + o * 2.5, m * 0.3 + o * 2.5], (t * 1.5).sin() * 0.3, cell::STAR, ink, layer::MASCOT + 3);
    p.put(sprout, [m * 0.3, m * 0.3], (t * 1.5).sin() * 0.3, cell::STAR, st.color(2), layer::MASCOT + 4);
    // Eyes look at the board (right in landscape, up in portrait), blink.
    let look = if f.layout.portrait { [0.1, -0.7] } else { [0.8, -0.2] };
    let blink = if f64::from(t) % BLINK_PERIOD < BLINK_LEN || mood == Mood::Sad { 0.25 } else { 1.0 };
    for s in [-1.0f32, 1.0] {
        let ep = [body[0] + s * bw * 0.18, body[1] - bh * 0.08];
        let es = [m * 0.2, m * 0.24 * blink];
        p.put(ep, [es[0] + o * 1.6, es[1] + o * 1.6], 0.0, cell::CIRCLE, ink, layer::MASCOT + 6);
        p.put(ep, es, 0.0, cell::CIRCLE, WHITE, layer::MASCOT + 7);
        if blink > 0.5 {
            let pp = [ep[0] + look[0] * m * 0.035, ep[1] + look[1] * m * 0.035];
            p.put(pp, [m * 0.1, m * 0.12], 0.0, cell::CIRCLE, ink, layer::MASCOT + 8);
            p.put([pp[0] - m * 0.02, pp[1] - m * 0.03], [m * 0.04, m * 0.04], 0.0, cell::CIRCLE, WHITE, layer::MASCOT + 9);
        }
        // Cheeks.
        p.put([body[0] + s * bw * 0.32, body[1] + bh * 0.1], [m * 0.14, m * 0.08], 0.0, cell::CIRCLE, alpha([1.0, 0.25, 0.4, 1.0], 0.55), layer::MASCOT + 6);
    }
    // Mouth by mood.
    let mp = [body[0], body[1] + bh * 0.16];
    match mood {
        Mood::Happy | Mood::Excited | Mood::Cheer => {
            let k = if mood == Mood::Happy { 0.16 } else { 0.22 };
            p.put(mp, [m * k, m * k * 0.8], core::f32::consts::PI, cell::TRIANGLE, ink, layer::MASCOT + 7);
            p.put([mp[0], mp[1] + m * 0.035], [m * k * 0.45, m * k * 0.3], 0.0, cell::CIRCLE, [1.0, 0.35, 0.45, 1.0], layer::MASCOT + 8);
        }
        Mood::Worried => p.put(mp, [m * 0.08, m * 0.1], 0.0, cell::CIRCLE, ink, layer::MASCOT + 7),
        Mood::Sad => p.put([mp[0], mp[1] + m * 0.02], [m * 0.14, m * 0.08], 0.0, cell::TRIANGLE, ink, layer::MASCOT + 7),
        Mood::Idle => p.put(mp, [m * 0.12, m * 0.03], 0.0, RECT, ink, layer::MASCOT + 7),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::levels_from_json;

    /// The piece colours of a fresh board and the sprite cell each piece's
    /// shade layer is drawn with, in cell order, using `style`.
    fn shade_cells(style: &Style) -> (Vec<u8>, Vec<u16>) {
        let (colours, sprites) = draw_at_rest(style);
        (colours, sprites.iter().filter(|s| s.layer == layer::SHADE).map(|s| s.cell).collect())
    }

    /// Level 1 at rest drawn with `style`: the piece colours in cell order
    /// and every sprite.
    fn draw_at_rest(style: &Style) -> (Vec<u8>, Vec<Sprite>) {
        let tune = crate::Tune::default();
        // Level 1 at rest, so every piece is on its cell and drawn.
        let g = crate::review::board_at_rest(&tune, crate::board::Rules::default());
        let (bw, bh) = (g.grid().board.w, g.grid().board.h);
        let l = crate::layout::layout(1280.0, 720.0, bw, bh, crate::layout::CellLimits::default());
        g.game_mut().layout = l;
        let mut rw = RenderWorld {
            viewport_css: [1280.0, 720.0],
            ..RenderWorld::default()
        };
        let f = Frame {
            layout: &l,
            style,
            fx: DrawFx { juice: false, pop_scale: 1.3, shake: [0.0; 2] },
            t: 1.0,
            tune: &tune,
        };
        draw_game(&mut rw, &g.world, &f);
        let colours: Vec<u8> = g.grid().slots.iter().flatten().filter_map(|&e| g.world.get::<Piece>(e).map(|p| p.color)).collect();
        (colours, rw.sprites)
    }

    /// A tiny sheet with the named frames, for the sprite art.
    fn art_with(names: &[&str]) -> evani_render::SpriteArt {
        let mut art = evani_render::SpriteArt::default();
        let sheet = evani_render::ArtSheet {
            width: 8 * names.len() as u32,
            height: 8,
            rgba8: vec![255; 8 * names.len() * 8 * 4],
            frames: names.iter().enumerate().map(|(i, n)| (n.to_string(), [8 * i as u32, 0, 8, 8])).collect(),
            pixel_art: false,
        };
        art.add(evani_render::ArtSource::Look, sheet).unwrap();
        art
    }

    /// The kit asks the look for its pieces by name; a look without them (or
    /// without some of them) leaves the built-in shapes, and the picture is
    /// otherwise the same.
    #[test]
    fn named_frame_falls_back_to_a_builtin_shape() {
        let builtin = |colours: &[u8]| -> Vec<u16> { colours.iter().map(|&c| SHAPES[usize::from(c)]).collect() };
        let plain = Style { outline_px: 3.5, shade: 0.62, ..Style::default() };
        let (colours, cells) = shade_cells(&plain);
        assert!(colours.len() > 20 && [0, 1, 2, 3].iter().all(|c| colours.contains(c)), "a board with several colours");
        assert_eq!(cells, builtin(&colours), "no art: the built-in shapes");
        // A look with no piece frames: nothing changes.
        let none = art_with(&["tile.other"]);
        assert_eq!(plain.with_art(&none), plain);
        assert_eq!(shade_cells(&plain.with_art(&none)).1, cells);
        // A look with two of the frames: those colours use them, the rest fall back.
        let some = art_with(&["piece.cherry", "piece.lemon"]);
        let cherry = some.cell("piece.cherry").unwrap();
        let lemon = some.cell("piece.lemon").unwrap();
        let (colours, got) = shade_cells(&plain.with_art(&some));
        let want: Vec<u16> = colours
            .iter()
            .map(|&c| match c {
                0 => cherry,
                2 => lemon,
                c => SHAPES[usize::from(c)],
            })
            .collect();
        assert_eq!(got, want);
        assert!(got.contains(&cherry) && got.contains(&lemon) && got.iter().any(|&c| c < evani_render::ART_CELL0));
        // A look with all seven: every piece is drawn with its own frame.
        let all = art_with(&ART_NAMES);
        let styled = plain.with_art(&all);
        assert_eq!(styled.cells, [0, 1, 2, 3, 4, 5, 6].map(|i| evani_render::ART_CELL0 + i));
        let (colours, got) = shade_cells(&styled);
        assert_eq!(got, colours.iter().map(|&c| evani_render::ART_CELL0 + u16::from(c)).collect::<Vec<_>>());
        // The prism has its own frame, the colour pieces' geometry stays the
        // built-in shape's (reach does not move).
        let prism = Piece { id: 0, kind: Kind::Prism, color: NO_COLOR };
        assert_eq!(styled.piece_cell_of(&prism), evani_render::ART_CELL0 + 6);
        assert_eq!(plain.piece_cell_of(&prism), cell::CIRCLE);
        assert_eq!(piece_shape(&prism), cell::CIRCLE);
        assert_eq!(plain.piece_edge_of(&prism), SpriteEdge::Builtin);
        assert_eq!(styled.piece_edge_of(&prism), SpriteEdge::Frame { width: 8, pixel_art: false });
    }

    /// The ink outline is `outline_px` wide whoever's art a piece uses: the
    /// copy behind a built-in (soft) shape grows by the allowance it was
    /// tuned with, behind a crisp look frame by less, so crisp art does not
    /// get a pixel-heavier outline.
    #[test]
    fn the_ink_allowance_follows_the_sprite_edge() {
        let plain = Style { outline_px: 3.5, shade: 0.62, ..Style::default() };
        // (shape fill, ink width - body width) of every piece on the board.
        let margins = |st: &Style| -> Vec<(u16, f32)> {
            let (_, sprites) = draw_at_rest(st);
            let shade = sprites.iter().filter(|s| s.layer == layer::SHADE);
            shade
                .map(|s| {
                    let ink = sprites.iter().find(|i| i.layer == layer::INK && i.cell == s.cell && i.pos == s.pos);
                    let ink = ink.unwrap_or_else(|| panic!("an ink copy behind the piece at {:?}", s.pos));
                    (s.cell, ink.size[0] - s.size[0])
                })
                .collect()
        };
        let (colours, _) = draw_at_rest(&plain);
        let fill = |c: u8| shape_fill(SHAPES[usize::from(c)]);
        // Built-in shapes: exactly the tuned 1 px allowance (unchanged).
        for ((_, m), &c) in margins(&plain).iter().zip(&colours) {
            let want = 2.0 * (3.5 + SpriteEdge::BUILTIN_PX) / fill(c);
            assert!((m - want).abs() < 1e-4, "built-in ink grows {m}, want {want}");
        }
        // 128 px look frames: half a texel, so the visible ink is 3.5 px
        // plus well under a pixel of anti-aliasing.
        let mut big = evani_render::SpriteArt::default();
        let mut sheet = evani_render::ArtSheet { width: 128 * 7, height: 128, rgba8: vec![255; 128 * 7 * 128 * 4], ..Default::default() };
        for (i, n) in ART_NAMES.iter().enumerate() {
            sheet.frames.push(((*n).to_string(), [i as u32 * 128, 0, 128, 128]));
        }
        big.add(evani_render::ArtSource::Look, sheet).unwrap();
        let crisp = plain.with_art(&big);
        let (_, sprites) = draw_at_rest(&crisp);
        let size = sprites.iter().find(|s| s.layer == layer::SHADE).map(|s| s.size[0]).unwrap();
        for ((cellno, m), &c) in margins(&crisp).iter().zip(&colours) {
            assert!(*cellno >= evani_render::ART_CELL0);
            let rim = m * fill(c) / 2.0 - 3.5;
            assert!(rim > 0.0 && rim < 0.5, "crisp ink rim {rim} px over 3.5");
            assert!((rim - 0.5 * size / 128.0).abs() < 1e-3, "half a texel at {size} px: {rim}");
        }
        // No outline: no ink copy at all.
        let (_, sprites) = draw_at_rest(&Style { outline_px: 0.0, ..crisp });
        assert!(sprites.iter().all(|s| s.layer != layer::INK));
    }

    /// The demo's look supplies every piece frame, and its silhouettes are
    /// the built-in shapes' (compared at the atlas's 64 px), so pieces read
    /// the same, only smoother. Another look supplies none.
    #[test]
    fn the_cartoon2d_look_supplies_smooth_pieces_with_the_builtin_silhouettes() {
        use evani_render::atlas::{ATLAS_CELL_PX, ATLAS_PX, build_atlas};
        let mut a = evani_core::App::new("art")
            .with_module(evani_render::RenderModule::new())
            .with_look(evani_look_cartoon2d::Cartoon2dLook)
            .with_kit(crate::Match3Plugin);
        a.start();
        a.run_headless(2, 1000.0 / 60.0);
        let art = a.world.res::<evani_render::SpriteArt>();
        let styled = Style::default().with_art(&art);
        assert!(styled.cells.iter().all(|&c| c >= evani_render::ART_CELL0), "{:?}", styled.cells);
        let builtin = build_atlas(&LookData::default());
        let grid = ATLAS_PX / ATLAS_CELL_PX;
        for (i, name) in ART_NAMES.iter().enumerate() {
            let f = art.frames().find(|f| f.name == *name).unwrap_or_else(|| panic!("{name} is supplied"));
            assert_eq!(f.rect[2], f.rect[3], "{name} is square");
            assert!(f.rect[2] >= 128, "{name} is crisp at 128 px cells");
            let k = f.rect[2] / ATLAS_CELL_PX;
            let want = SHAPES.get(i).copied().unwrap_or(cell::CIRCLE);
            let (cx, cy) = (u32::from(want) % grid * ATLAS_CELL_PX, u32::from(want) / grid * ATLAS_CELL_PX);
            let mut diff = 0.0f32;
            for y in 0..ATLAS_CELL_PX {
                for x in 0..ATLAS_CELL_PX {
                    let mut sum = 0u32;
                    for (dx, dy) in (0..k).flat_map(|dy| (0..k).map(move |dx| (dx, dy))) {
                        let o = (((f.rect[1] + y * k + dy) * f.sheet_w + f.rect[0] + x * k + dx) * 4 + 3) as usize;
                        sum += u32::from(f.sheet[o]);
                    }
                    let o = (((cy + y) * ATLAS_PX + cx + x) * 4 + 3) as usize;
                    diff += (sum as f32 / (k * k) as f32 - f32::from(builtin[o])).abs() / 255.0;
                }
            }
            let mean = diff / (ATLAS_CELL_PX * ATLAS_CELL_PX) as f32;
            assert!(mean < 0.02, "{name}: silhouette differs from the built-in shape (mean alpha diff {mean})");
        }
        drop(art);
        // Another look: no frames, the built-in shapes.
        let mut b = evani_core::App::new("art")
            .with_module(evani_render::RenderModule::new())
            .with_look(evani_look_basic::BasicLook)
            .with_kit(crate::Match3Plugin);
        b.start();
        b.run_headless(2, 1000.0 / 60.0);
        let none = Style::default().with_art(&b.world.res::<evani_render::SpriteArt>());
        assert_eq!(none, Style::default());
    }

    #[test]
    fn noise_is_smooth_and_bounded() {
        let mut prev = noise1(0.0, 3);
        for i in 1..1000 {
            let v = noise1(i as f32 * 0.01, 3);
            assert!(v.abs() <= 1.0);
            assert!((v - prev).abs() < 0.1);
            prev = v;
        }
    }

    #[test]
    fn a_frame_draws_board_pieces_and_mascot_within_budget() {
        let g = crate::game::SimWorld::new(levels_from_json(crate::LEVELS_JSON).unwrap(), crate::board::Rules::default());
        let (bw, bh) = (g.grid().board.w, g.grid().board.h);
        let l = crate::layout::layout(1920.0, 1080.0, bw, bh, crate::layout::CellLimits::default());
        g.game_mut().layout = l;
        let mut rw = RenderWorld {
            viewport_css: [1920.0, 1080.0],
            ..RenderWorld::default()
        };
        let tune = crate::Tune::default();
        let style = Style {
            outline_px: 3.5,
            shade: 0.62,
            highlight: 0.8,
            ..Style::default()
        };
        let f = Frame {
            layout: &l,
            style: &style,
            fx: DrawFx {
                juice: true,
                pop_scale: 1.3,
                shake: [0.0; 2],
            },
            t: 1.0,
            tune: &tune,
        };
        draw_game(&mut rw, &g.world, &f);
        let n = rw.sprites.len();
        // 64 pieces x 5 + tray 4 per cell + background + Puff.
        assert!(n > 64 * 5 + 64 * 4, "{n}");
        assert!(n < 2500, "{n}");
        // Everything is inside the canvas or close to it.
        assert!(rw.sprites.iter().all(|s| s.pos[0] > -400.0 && s.pos[0] < 2400.0));
    }

    /// Pieces dropping in, drawn by the real draw code (a Pop Bomb's halo,
    /// a vertical Streak's arrows and a Prism included), never show above the
    /// board's top edge, at a phone's small cells and a desktop's big ones;
    /// and they do reach it (they grow in from the edge, not from deep in
    /// the top row).
    #[test]
    fn entering_pieces_never_cross_the_board_top() {
        for (vw, vh) in [(390.0, 844.0), (1920.0, 1080.0)] {
            let g = crate::game::SimWorld::new(crate::level::levels_from_json(crate::LEVELS_JSON).unwrap(), crate::board::Rules::default());
            let l = crate::layout::layout(vw, vh, g.grid().board.w, g.grid().board.h, crate::layout::CellLimits::default());
            let tune = crate::Tune::default();
            let style = Style {
                outline_px: 4.0,
                squash: 1.0,
                ..Style::default()
            };
            let es: Vec<_> = g.grid().slots.iter().flatten().copied().collect();
            for (&e, kind) in es.iter().zip([Kind::Bomb, Kind::StreakV, Kind::Prism, Kind::StreakH]) {
                if let Some(mut pc) = g.world.get_mut::<Piece>(e) {
                    pc.kind = kind;
                }
            }
            let mut highest = f32::MAX;
            let mut worst = (0i16, 0u16, 0.0f32, [0.0f32; 2]);
            // Rows above their cells, up to (not at) resting in them. New
            // pieces always drop in plain; the specials here are a worst case.
            for k in 0..200 {
                let row = -1.2 + k as f32 * 0.006;
                for &e in &es {
                    if let Some(mut m) = g.world.get_mut::<Motion>(e) {
                        m.off = [0.0, row];
                        m.vy = tune.fall_speed * 2.0;
                        m.swap_t = -1.0;
                    }
                }
                for t in [0.0, 0.3, 0.9] {
                    let f = Frame {
                        layout: &l,
                        style: &style,
                        fx: DrawFx {
                            juice: true,
                            pop_scale: 1.3,
                            shake: [0.0; 2],
                        },
                        t,
                        tune: &tune,
                    };
                    let mut rw = RenderWorld::default();
                    draw_game(&mut rw, &g.world, &f);
                    // A piece's own sprites (halo to its top add-ons) above
                    // the board's middle; soft glows aside. Rotated sprites
                    // by their bounds; shapes by how much of the sprite they
                    // fill (a rectangle fills it all).
                    for s in rw.sprites.iter().filter(|s| (layer::HALO..=layer::DECO_TOP).contains(&s.layer) && s.cell != cell::GLOW && s.pos[1] < l.board[1] + l.board[3] * 0.5) {
                        let fill = if s.cell == RECT { 1.0 } else { shape_fill(s.cell) };
                        let (sn, cs) = s.rotation.sin_cos();
                        let h = (s.size[0].abs() * sn.abs() + s.size[1].abs() * cs.abs()) * fill;
                        if s.pos[1] - h * 0.5 < highest {
                            highest = s.pos[1] - h * 0.5;
                            worst = (s.layer, s.cell, t, s.size);
                        }
                    }
                }
            }
            assert!(highest >= l.board[1] - 0.01, "{vw}x{vh}: a piece shows {:.1} px above the board (layer, cell, t, size: {worst:?})", l.board[1] - highest);
            // The ink of the top row (always there) reaches within a fifth
            // of a cell of the edge.
            assert!(highest <= l.board[1] + 0.2 * l.cell, "{vw}x{vh}: pieces stop {:.1} px under the board's top", highest - l.board[1]);
        }
        let st = Style::default();
        assert!(entry_reach(40.0, &st) > 0.0);
    }

    /// Puff in every mood, over seconds of hops, wobble and blinks, stays
    /// under `layout::MASCOT_REACH` (the room the landscape layout keeps
    /// free above him) and on his ground line.
    #[test]
    fn board_inset_is_the_distance_to_the_nearest_edge() {
        // An 8 x 8 board spans -0.5..7.5 in cell units.
        assert!((board_inset([0.0, 3.0], 8, 8) - 0.5).abs() < 1e-6);
        assert!((board_inset([3.5, 3.5], 8, 8) - 4.0).abs() < 1e-6);
        assert!((board_inset([7.0, 7.4], 8, 8) - 0.1).abs() < 1e-6);
        assert!(board_inset([-1.0, 3.0], 8, 8) < 0.0, "left of the board");
        assert!(board_inset([3.0, -0.9], 8, 8) < 0.0, "above the board");
    }

    #[test]
    fn puff_stays_within_his_reach() {
        let l = crate::layout::layout(1920.0, 1080.0, 8, 8, crate::layout::CellLimits::default());
        let [_, gy, m] = l.mascot;
        let tune = crate::Tune::default();
        let style = Style {
            outline_px: 3.5,
            ..Style::default()
        };
        let (mut top, mut feet) = (f32::MAX, f32::MIN);
        for mood in [Mood::Idle, Mood::Happy, Mood::Excited, Mood::Cheer, Mood::Worried, Mood::Sad] {
            for i in 0..600 {
                let t = i as f32 / 120.0;
                let f = Frame {
                    layout: &l,
                    style: &style,
                    fx: DrawFx {
                        juice: true,
                        pop_scale: 1.3,
                        shake: [0.0; 2],
                    },
                    t,
                    tune: &tune,
                };
                let mut rw = RenderWorld::default();
                mascot(&mut Painter { rw: &mut rw }, Puff { mood, mood_t: t }, &f);
                for s in rw.sprites.iter().filter(|s| s.layer > layer::MASCOT) {
                    // A rotated square sprite's bounds grow by up to sqrt(2).
                    let (sn, cs) = s.rotation.sin_cos();
                    let h = s.size[0].abs() * sn.abs() + s.size[1].abs() * cs.abs();
                    top = top.min(s.pos[1] - h * 0.5);
                    if mood == Mood::Idle {
                        feet = feet.max(s.pos[1] + h * 0.5);
                    }
                }
            }
        }
        let reach = (gy - top) / m;
        assert!(reach <= crate::layout::MASCOT_REACH, "Puff reaches {reach:.3} x his size");
        assert!(reach > crate::layout::MASCOT_REACH - 0.15, "MASCOT_REACH {} wastes room: Puff reaches {reach:.3}", crate::layout::MASCOT_REACH);
        // Standing, his lowest ink (soles and body outline) is on the ground
        // line, where his ground shadow is centred.
        assert!((feet - gy).abs() <= 0.5, "feet at {feet}, ground at {gy}");
    }
}
