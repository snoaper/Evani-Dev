//! Indexed pixel art built into the renderer's [`SpriteSheet`].
//!
//! Every pixel names a colour ramp and one of its four shades (dark to
//! light), never a colour, so a look recolours the whole game: the ramps
//! come from the look's `ramp.<name>` palettes when it has them, else from
//! `data/art.json`. Hand-drawn sprites come from `art.json`; tiles, water,
//! fire, bushes, trees, rocks, clouds, the parallax planes and the HUD
//! glyphs are generated here (deterministic, no randomness).
//!
//! Light comes from the top-left everywhere: sprites get a highlight on
//! their top and left edges, a shadow on their bottom and right edges and
//! a selective outline (the ramp's darkest shade on the lit side, ink on
//! the shaded side); blobs, bricks and hills are shaded the same way.
//! Water and torch fire are palette-cycled: each has four frames whose
//! pixels step through a fixed run of shades, played at `cycle_fps`.
//! The parallax planes have the look's layer tints (multiply, then haze)
//! baked in.

use std::collections::BTreeMap;

use evani_core::plugin::LookData;
use evani_render::SpriteSheet;
use serde::Deserialize;

use crate::font;
use crate::level::{Level, Tile};

/// The colour ramps, in index order (the look names them `ramp.<name>`).
pub const RAMPS: [&str; 15] = [
    "ink", "fur", "teal", "green", "leaf", "brown", "stone", "gold", "red", "blue", "water", "sky", "white", "shadow", "soil",
];

/// Ramp indices into [`RAMPS`].
pub mod ramp {
    pub const INK: u8 = 0;
    pub const FUR: u8 = 1;
    pub const TEAL: u8 = 2;
    pub const GREEN: u8 = 3;
    pub const LEAF: u8 = 4;
    pub const BROWN: u8 = 5;
    pub const STONE: u8 = 6;
    pub const GOLD: u8 = 7;
    pub const RED: u8 = 8;
    pub const BLUE: u8 = 9;
    pub const WATER: u8 = 10;
    pub const SKY: u8 = 11;
    pub const WHITE: u8 = 12;
    pub const SHADOW: u8 = 13;
    /// Deep earth, darker than brown (terrain darkens with depth).
    pub const SOIL: u8 = 14;
}

/// Hand-drawn sprites `art.json` must provide.
pub const SPRITES: [&str; 30] = [
    "hero_idle0",
    "hero_idle1",
    "hero_run0",
    "hero_run1",
    "hero_run2",
    "hero_run3",
    "hero_jump",
    "hero_fall",
    "hero_hurt",
    "beetle0",
    "beetle1",
    "beetle_flat",
    "hopper_sit",
    "hopper_jump",
    "hopper_flat",
    "coin0",
    "coin1",
    "coin2",
    "flag_down",
    "flag_up",
    "sign",
    "flower",
    "grass",
    "reed",
    "icon_head",
    "icon_clock",
    "torch",
    // Generated, listed so the names stay reserved.
    "bush",
    "tree",
    "rock",
];

/// Decoration kinds the level may use.
pub const DECOS: [&str; 7] = ["sign", "bush", "flower", "tree", "grass", "reed", "rock"];

// ---------------------------------------------------------------------------
// art.json

#[derive(Deserialize)]
struct ArtJson {
    ramps: BTreeMap<String, [String; 4]>,
    layers: BTreeMap<String, (String, String, f32)>,
    sky: Vec<String>,
    keys: BTreeMap<String, (String, u8, bool)>,
    sprites: BTreeMap<String, Vec<String>>,
}

/// One key: a ramp, a shade and whether top-left light applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub ramp: u8,
    pub shade: u8,
    pub lit: bool,
}

/// Parsed and validated `art.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtData {
    /// Default ramps, sRGB `0xRRGGBB`, dark to light.
    pub ramps: [[u32; 4]; 15],
    /// Parallax plane tints (far, mid, near): multiply, haze, haze amount.
    pub layers: [(u32, u32, f32); 3],
    /// Sky bands, top to horizon.
    pub sky: Vec<u32>,
    keys: BTreeMap<char, Key>,
    sprites: BTreeMap<String, Vec<String>>,
}

fn hex(s: &str) -> Result<u32, String> {
    if s.len() != 6 {
        return Err(format!("colour '{s}' must be 6 hex digits"));
    }
    u32::from_str_radix(s, 16).map_err(|_| format!("colour '{s}' is not hex"))
}

impl ArtData {
    /// Parse and validate: all 15 ramps, three layers, known keys, every
    /// required sprite present with rectangular rows of known characters.
    pub fn from_json(src: &str) -> Result<Self, String> {
        let j: ArtJson = serde_json::from_str(src).map_err(|e| format!("art.json: {e}"))?;
        let mut ramps = [[0u32; 4]; 15];
        for (i, name) in RAMPS.iter().enumerate() {
            let r = j.ramps.get(*name).ok_or_else(|| format!("art.json: missing ramp '{name}'"))?;
            for (s, c) in r.iter().enumerate() {
                ramps[i][s] = hex(c).map_err(|e| format!("art.json: ramp '{name}': {e}"))?;
            }
        }
        let mut layers = [(0u32, 0u32, 0.0f32); 3];
        for (i, name) in ["far", "mid", "near"].iter().enumerate() {
            let (t, h, a) = j.layers.get(*name).ok_or_else(|| format!("art.json: missing layer '{name}'"))?;
            layers[i] = (hex(t)?, hex(h)?, a.clamp(0.0, 1.0));
        }
        if j.sky.is_empty() {
            return Err("art.json: sky needs at least one band".into());
        }
        let sky = j.sky.iter().map(|s| hex(s)).collect::<Result<Vec<_>, _>>()?;
        let mut keys = BTreeMap::new();
        for (k, (r, shade, lit)) in &j.keys {
            let mut ch = k.chars();
            let (Some(c), None) = (ch.next(), ch.next()) else {
                return Err(format!("art.json: key '{k}' must be one character"));
            };
            if c == '.' {
                return Err("art.json: '.' is transparent and can't be a key".into());
            }
            let ramp = RAMPS.iter().position(|n| n == r).ok_or_else(|| format!("art.json: key '{k}': unknown ramp '{r}'"))?;
            if *shade > 3 {
                return Err(format!("art.json: key '{k}': shade must be 0..3"));
            }
            keys.insert(c, Key { ramp: ramp as u8, shade: *shade, lit: *lit });
        }
        for (name, rows) in &j.sprites {
            let w = rows.first().map_or(0, |r| r.chars().count());
            if w == 0 {
                return Err(format!("art.json: sprite '{name}' is empty"));
            }
            for (y, row) in rows.iter().enumerate() {
                if row.chars().count() != w {
                    return Err(format!("art.json: sprite '{name}' row {y} is not {w} wide"));
                }
                if let Some((x, c)) = row.chars().enumerate().find(|(_, c)| *c != '.' && !keys.contains_key(c)) {
                    return Err(format!("art.json: sprite '{name}' row {y} column {x}: unknown key '{c}'"));
                }
            }
        }
        for name in &SPRITES[..27] {
            if !j.sprites.contains_key(*name) {
                return Err(format!("art.json: missing sprite '{name}'"));
            }
        }
        Ok(Self { ramps, layers, sky, keys, sprites: j.sprites })
    }

    /// A hand-drawn sprite, lit (no outline yet).
    pub fn sprite(&self, name: &str) -> Img {
        let rows = self.sprites.get(name).map_or(&[][..], |r| &r[..]);
        let w = rows.first().map_or(1, |r| r.chars().count()) as u32;
        let mut img = Img::new(w, rows.len().max(1) as u32);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if let Some(k) = self.keys.get(&c) {
                    img.put_lit(x as i32, y as i32, k.ramp, k.shade, k.lit);
                }
            }
        }
        img.light();
        img
    }
}

// ---------------------------------------------------------------------------
// Indexed images

/// A transparent pixel.
pub const CLEAR: u8 = 0;
/// Pure white (tinted when drawn: shadows, solid fills).
pub const WHITE: u8 = 255;

/// The pixel value of `ramp`, `shade`.
pub fn ix(ramp: u8, shade: u8) -> u8 {
    1 + ramp * 4 + shade.min(3)
}

fn ramp_of(p: u8) -> u8 {
    (p - 1) / 4
}

fn shade_of(p: u8) -> u8 {
    (p - 1) % 4
}

/// An indexed image: [`CLEAR`], [`WHITE`] or [`ix`] per pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
    lit: Vec<bool>,
}

impl Img {
    pub fn new(w: u32, h: u32) -> Self {
        let n = (w * h) as usize;
        Self { w, h, px: vec![CLEAR; n], lit: vec![false; n] }
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h).then(|| (y as u32 * self.w + x as u32) as usize)
    }

    pub fn get(&self, x: i32, y: i32) -> u8 {
        self.idx(x, y).map_or(CLEAR, |i| self.px[i])
    }

    pub fn opaque(&self, x: i32, y: i32) -> bool {
        self.get(x, y) != CLEAR
    }

    /// A nearest-neighbour copy at `w x h`, baked into its own frame so a
    /// squash is drawn 1:1 on the pixel grid (never scaled when drawn).
    pub fn resized(&self, w: u32, h: u32) -> Img {
        let mut o = Img::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let sx = ((x as f32 + 0.5) * self.w as f32 / w as f32) as i32;
                let sy = ((y as f32 + 0.5) * self.h as f32 / h as f32) as i32;
                if let (Some(s), Some(d)) = (self.idx(sx, sy), o.idx(x as i32, y as i32)) {
                    o.px[d] = self.px[s];
                    o.lit[d] = self.lit[s];
                }
            }
        }
        o
    }

    pub fn set(&mut self, x: i32, y: i32, p: u8) {
        if let Some(i) = self.idx(x, y) {
            self.px[i] = p;
            self.lit[i] = false;
        }
    }

    pub fn put(&mut self, x: i32, y: i32, ramp: u8, shade: u8) {
        self.set(x, y, ix(ramp, shade));
    }

    fn put_lit(&mut self, x: i32, y: i32, ramp: u8, shade: u8, lit: bool) {
        if let Some(i) = self.idx(x, y) {
            self.px[i] = ix(ramp, shade);
            self.lit[i] = lit;
        }
    }

    /// Top-left light on lit pixels: one shade up on edges facing up or
    /// left, one down on edges facing down or right.
    pub fn light(&mut self) {
        let src = self.px.clone();
        let op = |x: i32, y: i32| x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h && src[(y as u32 * self.w + x as u32) as usize] != CLEAR;
        let mut out = src.clone();
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                let i = (y as u32 * self.w + x as u32) as usize;
                let p = src[i];
                if !self.lit[i] || p == CLEAR || p == WHITE {
                    continue;
                }
                let s = shade_of(p);
                let s = if !op(x, y - 1) || !op(x - 1, y) {
                    (s + 1).min(3)
                } else if !op(x, y + 1) || !op(x + 1, y) {
                    s.saturating_sub(1)
                } else {
                    s
                };
                out[i] = ix(ramp_of(p), s);
            }
        }
        self.px = out;
    }

    /// One pixel bigger on every side, with a selective outline: the
    /// neighbour's darkest shade above and left of the shape (the lit
    /// side), ink below and right.
    pub fn outlined(&self) -> Img {
        let mut o = Img::new(self.w + 2, self.h + 2);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                o.set(x + 1, y + 1, self.get(x, y));
            }
        }
        let src = o.clone();
        for y in 0..o.h as i32 {
            for x in 0..o.w as i32 {
                if src.opaque(x, y) {
                    continue;
                }
                let lit_side = [src.get(x, y + 1), src.get(x + 1, y)].into_iter().find(|p| *p != CLEAR && *p != WHITE);
                if let Some(p) = lit_side {
                    o.put(x, y, ramp_of(p), 0);
                } else if src.opaque(x - 1, y) || src.opaque(x, y - 1) {
                    o.put(x, y, ramp::INK, 0);
                }
            }
        }
        o
    }

    /// Outline every side in ink and add a one-pixel drop shadow down and
    /// to the right (HUD glyphs).
    fn glyph_frame(&self) -> Img {
        let mut o = Img::new(self.w + 3, self.h + 3);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                o.set(x + 1, y + 1, self.get(x, y));
            }
        }
        let body = o.clone();
        for y in 0..o.h as i32 {
            for x in 0..o.w as i32 {
                if body.opaque(x, y) {
                    continue;
                }
                let near = (-1..=1).any(|dy| (-1..=1).any(|dx| body.opaque(x + dx, y + dy)));
                if near || body.opaque(x - 2, y - 2) || body.opaque(x - 1, y - 2) || body.opaque(x - 2, y - 1) {
                    o.put(x, y, ramp::INK, 0);
                }
            }
        }
        o
    }

    /// The image mirrored left to right.
    pub fn flipped(&self) -> Img {
        let mut o = Img::new(self.w, self.h);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                o.set(self.w as i32 - 1 - x, y, self.get(x, y));
            }
        }
        o
    }

    /// Copy `src` with its top-left at `(x, y)` (clear pixels skipped).
    pub fn blit(&mut self, src: &Img, x: i32, y: i32) {
        for sy in 0..src.h as i32 {
            for sx in 0..src.w as i32 {
                let p = src.get(sx, sy);
                if p != CLEAR {
                    self.set(x + sx, y + sy, p);
                }
            }
        }
    }
}

/// A small deterministic hash.
pub fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// 4 × 4 ordered dither threshold in `-0.5..0.5`.
fn bayer(x: i32, y: i32) -> f32 {
    const M: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    (M[(y & 3) as usize][(x & 3) as usize] as f32 + 0.5) / 16.0 - 0.5
}

/// Rows of the ordered dither between two sky bands (half above the band
/// edge, half below).
pub const SKY_DITHER_H: u32 = 8;
/// Width of the sky dither strip (a whole number of 4 × 4 Bayer cells, so
/// strips laid side by side continue the pattern).
pub const SKY_DITHER_W: u32 = 64;

/// The step between two sky bands (look bible: "a six-band dithered blue
/// sky"; critic round 2, item 4): white where the lower band shows, from
/// one pixel in sixteen on the top row to fifteen in sixteen on the bottom
/// row, by the same 4 × 4 Bayer matrix as the earth and the hills. Tinted
/// with the lower band's colour and drawn over the upper band, it turns
/// one flat colour into the next with no new colour.
pub fn sky_dither() -> Img {
    let mut m = Img::new(SKY_DITHER_W, SKY_DITHER_H);
    for y in 0..SKY_DITHER_H as i32 {
        let t = (y as f32 + 0.5) / SKY_DITHER_H as f32 - 0.5;
        for x in 0..SKY_DITHER_W as i32 {
            if t > bayer(x, y) {
                m.set(x, y, WHITE);
            }
        }
    }
    m
}

/// A round blob lit from the top-left: `(cx, cy, r)`.
type Blob = (f32, f32, f32);

/// Fill the union of `blobs` in `ramp`, each pixel shaded by the sphere in
/// front (light from the top-left, dithered band edges). `lift` raises
/// every shade (0 normal).
fn blobs(img: &mut Img, list: &[Blob], ramp: u8, lift: f32) {
    for y in 0..img.h as i32 {
        for x in 0..img.w as i32 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut best: Option<(f32, f32)> = None;
            for &(cx, cy, r) in list {
                let (nx, ny) = ((px - cx) / r, (py - cy) / r);
                let d = nx * nx + ny * ny;
                if d > 1.0 {
                    continue;
                }
                let z = (1.0 - d).sqrt();
                // Front-most: highest sphere point (blobs lower down are nearer).
                let key = z * r + cy * 0.05;
                let light = -(nx * 0.55 + ny * 0.7) + z * 0.45;
                if best.is_none_or(|(k, _)| key > k) {
                    best = Some((key, light));
                }
            }
            if let Some((_, l)) = best {
                let v = l + lift + bayer(x, y) * 0.22;
                let s = if v > 0.62 {
                    3
                } else if v > 0.18 {
                    2
                } else if v > -0.28 {
                    1
                } else {
                    0
                };
                img.put(x, y, ramp, s);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Colours

fn srgb_to_linear(v: u32) -> f32 {
    let c = v.min(255) as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn rgb_linear(rgb: u32) -> [f32; 4] {
    [srgb_to_linear((rgb >> 16) & 255), srgb_to_linear((rgb >> 8) & 255), srgb_to_linear(rgb & 255), 1.0]
}

fn linear_to_srgb8(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// The resolved colours (linear RGBA): the look's palettes where it has
/// them, else `art.json`'s.
#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub ramps: [[[f32; 4]; 4]; 15],
    /// Per plane (far, mid, near): multiply tint, haze (alpha = amount).
    pub layers: [([f32; 4], [f32; 4]); 3],
    pub sky: Vec<[f32; 4]>,
}

impl Colors {
    pub fn new(art: &ArtData, look: Option<&LookData>) -> Self {
        let mut ramps = [[[0.0; 4]; 4]; 15];
        for (i, name) in RAMPS.iter().enumerate() {
            let pal = look.and_then(|l| l.palette(&format!("ramp.{name}"))).filter(|p| p.colors.len() >= 4);
            for (s, slot) in ramps[i].iter_mut().enumerate() {
                *slot = pal.map_or_else(|| rgb_linear(art.ramps[i][s]), |p| p.colors[s]);
            }
        }
        let mut layers = [([1.0; 4], [0.0; 4]); 3];
        for (i, name) in ["layer.far", "layer.mid", "layer.near"].iter().enumerate() {
            let (t, h, a) = art.layers[i];
            let mut haze = rgb_linear(h);
            haze[3] = a;
            layers[i] = match look.and_then(|l| l.palette(name)).filter(|p| p.colors.len() >= 2) {
                Some(p) => (p.colors[0], p.colors[1]),
                None => (rgb_linear(t), haze),
            };
        }
        let sky = match look.and_then(|l| l.palette("sky")).filter(|p| !p.colors.is_empty()) {
            Some(p) => p.colors.clone(),
            None => art.sky.iter().map(|c| rgb_linear(*c)).collect(),
        };
        Self { ramps, layers, sky }
    }

    /// Linear colour of an indexed pixel, through plane tint `layer`.
    pub fn color(&self, p: u8, layer: Option<usize>) -> [f32; 4] {
        let mut c = match p {
            CLEAR => return [0.0; 4],
            WHITE => [1.0; 4],
            _ => self.ramps[ramp_of(p).min(RAMPS.len() as u8 - 1) as usize][shade_of(p) as usize],
        };
        if let Some(l) = layer {
            let (t, h) = self.layers[l.min(2)];
            for k in 0..3 {
                c[k] = c[k] * t[k] * (1.0 - h[3]) + h[k] * h[3];
            }
        }
        c[3] = 1.0;
        c
    }

    /// A ramp shade (linear).
    pub fn shade(&self, ramp: u8, shade: u8) -> [f32; 4] {
        self.color(ix(ramp, shade), None)
    }
}

// ---------------------------------------------------------------------------
// The built sheet

/// A sheet frame: its sprite cell and size in art pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub cell: u16,
    pub w: f32,
    pub h: f32,
}

/// How a level tile is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileArt {
    #[default]
    None,
    /// One frame.
    Still(u16),
    /// Four palette-cycled frames from this cell.
    Cycle(u16),
    /// Drawn behind the playfield (castle walls).
    Back(u16),
}

/// Hero poses.
pub mod pose {
    pub const IDLE0: usize = 0;
    pub const IDLE1: usize = 1;
    pub const RUN0: usize = 2;
    pub const JUMP: usize = 6;
    pub const FALL: usize = 7;
    pub const HURT: usize = 8;
}

/// Everything the draw system needs: the sheet and where each thing is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Art {
    pub sheet: SpriteSheet,
    pub white: Frame,
    /// Idle 0..1, run 0..3, jump, fall, hurt (see [`pose`]).
    pub hero: [Frame; 9],
    /// Each hero pose squashed for the moment after landing (drawn 1:1).
    pub hero_land: [Frame; 9],
    pub beetle: [Frame; 3],
    pub hopper: [Frame; 3],
    /// Face, three-quarter, edge.
    pub coin: [Frame; 3],
    pub flag: [Frame; 2],
    /// Same order as [`DECOS`].
    pub deco: [Frame; 7],
    pub platform: [Frame; 3],
    pub torch: Frame,
    pub flame: [Frame; 4],
    pub gate: Frame,
    /// Far, mid and near parallax strips (tileable), their fill colour
    /// below, and the clouds.
    pub planes: [Frame; 3],
    pub plane_fill: [[f32; 4]; 3],
    /// The castle on the far hills and its top-left corner in the far
    /// plane's pixels (drawn once every two far strips).
    pub far_castle: Frame,
    pub far_castle_at: [f32; 2],
    pub clouds: [Frame; 2],
    pub sky: Vec<[f32; 4]>,
    /// The ordered-dither step between two sky bands ([`sky_dither`]):
    /// drawn tinted in the lower band's colour over the upper band.
    pub sky_dither: Frame,
    /// Drop shadows by width: `shadow[w - SHADOW_MIN_W]` is `w` px wide.
    pub shadow: Vec<Frame>,
    pub shadow_color: [f32; 4],
    /// The "LEVEL CLEAR!" banner: its panel and its border (ink).
    pub banner_colors: [[f32; 4]; 2],
    pub sparkle: [Frame; 2],
    /// The hero's glow (an extra life, the level clear), strongest first.
    pub glow: [Frame; 3],
    /// The extra-life burst, innermost first.
    pub burst: [Frame; 3],
    pub dust: [Frame; 3],
    pub puff: [Frame; 2],
    pub glyphs: [Frame; font::HUD.len()],
    pub mini: [Frame; 18],
    pub icon_head: Frame,
    pub icon_coin: Frame,
    pub icon_clock: Frame,
    /// Per level tile (`ty * w + tx`).
    pub tiles: Vec<TileArt>,
    pub level_w: usize,
}

impl Art {
    /// Tile art at `(tx, ty)`.
    pub fn tile(&self, tx: i32, ty: i32) -> TileArt {
        if tx < 0 || ty < 0 || tx as usize >= self.level_w {
            return TileArt::None;
        }
        self.tiles.get(ty as usize * self.level_w + tx as usize).copied().unwrap_or_default()
    }
}

/// Packs frames into rows of a fixed-width sheet.
struct Packer {
    w: u32,
    x: u32,
    y: u32,
    row_h: u32,
    items: Vec<(Img, Option<usize>, [u32; 2])>,
}

impl Packer {
    fn new(w: u32) -> Self {
        Self { w, x: 0, y: 0, row_h: 0, items: Vec::new() }
    }

    fn add_in(&mut self, img: Img, layer: Option<usize>) -> Frame {
        let (w, h) = (img.w.min(self.w), img.h);
        if self.x + w > self.w {
            self.x = 0;
            self.y += self.row_h + 1;
            self.row_h = 0;
        }
        let at = [self.x, self.y];
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        let cell = evani_render::SpriteSheet::cell(self.items.len());
        self.items.push((img, layer, at));
        Frame { cell, w: w as f32, h: h as f32 }
    }

    fn add(&mut self, img: Img) -> Frame {
        self.add_in(img, None)
    }

    fn finish(self, colors: &Colors, revision: u64) -> SpriteSheet {
        let height = self.y + self.row_h;
        let width = self.w;
        let mut rgba8 = vec![0u8; (width * height * 4) as usize];
        let mut frames = Vec::with_capacity(self.items.len());
        for (img, layer, at) in &self.items {
            let w = img.w.min(width);
            frames.push([at[0], at[1], w, img.h]);
            for y in 0..img.h {
                for x in 0..w {
                    let p = img.px[(y * img.w + x) as usize];
                    if p == CLEAR {
                        continue;
                    }
                    let c = colors.color(p, *layer);
                    let o = (((at[1] + y) * width + at[0] + x) * 4) as usize;
                    rgba8[o] = linear_to_srgb8(c[0]);
                    rgba8[o + 1] = linear_to_srgb8(c[1]);
                    rgba8[o + 2] = linear_to_srgb8(c[2]);
                    rgba8[o + 3] = 255;
                }
            }
        }
        SpriteSheet { width, height, rgba8, frames, revision }
    }
}

/// Sheet width in pixels.
pub const SHEET_W: u32 = 512;

impl Art {
    /// Build every frame for `level` in `colors`.
    pub fn build(data: &ArtData, level: &Level, colors: &Colors, revision: u64) -> Art {
        let mut pk = Packer::new(SHEET_W);
        let mut a = Art { level_w: level.w, ..Art::default() };
        // Wide strips first (they fill whole rows).
        a.planes = [pk.add_in(far_plane(), Some(0)), pk.add_in(mid_plane(), Some(1)), pk.add_in(near_plane(), Some(2))];
        for (i, (img, layer)) in [(far_plane(), 0), (mid_plane(), 1), (near_plane(), 2)].iter().enumerate() {
            let p = img.get(0, img.h as i32 - 1);
            a.plane_fill[i] = colors.color(p, Some(*layer));
        }
        let (castle, at) = far_castle();
        a.far_castle = pk.add_in(castle, Some(0));
        a.far_castle_at = at;
        a.clouds = [pk.add_in(cloud(44, 16, 1), Some(0)), pk.add_in(cloud(30, 12, 2), Some(0))];
        let mut white = Img::new(4, 4);
        white.px.fill(WHITE);
        a.white = pk.add(white);
        a.sky_dither = pk.add(sky_dither());
        let names = [
            "hero_idle0",
            "hero_idle1",
            "hero_run0",
            "hero_run1",
            "hero_run2",
            "hero_run3",
            "hero_jump",
            "hero_fall",
            "hero_hurt",
        ];
        for (i, n) in names.iter().enumerate() {
            let s = data.sprite(n);
            a.hero[i] = pk.add(s.outlined());
            // The landing squash: 2 px wider, 2 px shorter, its own frame.
            a.hero_land[i] = pk.add(s.resized(s.w + 2, s.h.saturating_sub(2).max(1)).outlined());
        }
        for (i, n) in ["beetle0", "beetle1", "beetle_flat"].iter().enumerate() {
            a.beetle[i] = pk.add(data.sprite(n).outlined());
        }
        for (i, n) in ["hopper_sit", "hopper_jump", "hopper_flat"].iter().enumerate() {
            a.hopper[i] = pk.add(data.sprite(n).outlined());
        }
        for (i, n) in ["coin0", "coin1", "coin2"].iter().enumerate() {
            a.coin[i] = pk.add(data.sprite(n).outlined());
        }
        a.flag = [pk.add(data.sprite("flag_down").outlined()), pk.add(data.sprite("flag_up").outlined())];
        for (i, n) in DECOS.iter().enumerate() {
            let img = match *n {
                "bush" => bush(),
                "tree" => tree(),
                "rock" => rock(),
                _ => data.sprite(n),
            };
            a.deco[i] = pk.add(img.outlined());
        }
        a.platform = [pk.add(platform(0)), pk.add(platform(1)), pk.add(platform(2))];
        a.torch = pk.add(data.sprite("torch").outlined());
        a.flame = [0, 1, 2, 3].map(|k| pk.add(flame(k)));
        a.gate = pk.add(gate());
        // Drop shadows: one ellipse per drawn width, each drawn 1:1 (a
        // stretched sprite would break the pixel grid).
        a.shadow = (SHADOW_MIN_W..=SHADOW_MAX_W).map(|w| pk.add(shadow_ellipse(w))).collect();
        a.shadow_color = colors.shade(ramp::SHADOW, 0);
        a.banner_colors = [colors.shade(ramp::INK, 1), colors.shade(ramp::INK, 3)];
        a.sparkle = [pk.add(sparkle(0)), pk.add(sparkle(1))];
        a.glow = [0, 1, 2].map(|k| pk.add(glow(k)));
        a.burst = [0, 1, 2].map(|k| pk.add(burst(k)));
        a.dust = [pk.add(dust(0)), pk.add(dust(1)), pk.add(dust(2))];
        a.puff = [pk.add(puff(0)), pk.add(puff(1))];
        for (i, g) in font::HUD.iter().enumerate() {
            a.glyphs[i] = pk.add(glyph(g, false));
        }
        for (i, g) in font::MINI.iter().enumerate() {
            a.mini[i] = pk.add(glyph(g, true));
        }
        a.icon_head = pk.add(data.sprite("icon_head").glyph_frame());
        a.icon_clock = pk.add(data.sprite("icon_clock").glyph_frame());
        a.icon_coin = pk.add(data.sprite("coin0").glyph_frame());
        a.sky.clone_from(&colors.sky);
        // Level tiles: one frame per distinct look.
        let mut made: BTreeMap<u64, TileArt> = BTreeMap::new();
        let mut tiles = Vec::with_capacity(level.w * level.h);
        // A bottomless pit is drawn as a dark void: frames from the lip
        // down, each with or without a wall rim on either side, so it can
        // never read as ground or as the background.
        let mut abyss = [[0u16; 4]; ABYSS_ROWS];
        for (d, frames) in abyss.iter_mut().enumerate() {
            for (sides, cell) in frames.iter_mut().enumerate() {
                *cell = pk.add(abyss_tile(d as i32, sides & 1 != 0, sides & 2 != 0)).cell;
            }
        }
        let lips = level.pit_lips();
        let wall = |tx: i32, ty: i32| matches!(level.tile(tx, ty), Tile::Solid { .. } | Tile::Slope { .. });
        for ty in 0..level.h as i32 {
            for tx in 0..level.w as i32 {
                let t = level.tile(tx, ty);
                let Some(key) = tile_key(level, t, tx, ty) else {
                    let pit = if t == Tile::Empty { lips[tx as usize].filter(|&lip| ty >= lip) } else { None };
                    tiles.push(pit.map_or(TileArt::None, |lip| {
                        let sides = usize::from(wall(tx - 1, ty)) | usize::from(wall(tx + 1, ty)) << 1;
                        TileArt::Back(abyss[((ty - lip) as usize).min(ABYSS_ROWS - 1)][sides])
                    }));
                    continue;
                };
                let art = *made.entry(key).or_insert_with(|| match t {
                    Tile::Water { .. } => {
                        let first = pk.add(water(key as u32, 0));
                        for k in 1..4 {
                            pk.add(water(key as u32, k));
                        }
                        TileArt::Cycle(first.cell)
                    }
                    Tile::Decoration { .. } => TileArt::Back(pk.add(tile_img(key)).cell),
                    _ => TileArt::Still(pk.add(tile_img(key)).cell),
                });
                tiles.push(art);
            }
        }
        a.tiles = tiles;
        a.sheet = pk.finish(colors, revision);
        a
    }
}

// ---------------------------------------------------------------------------
// Tiles

/// Tile kinds in a tile key's top byte.
mod tk {
    pub const EARTH: u32 = 1;
    pub const STONE: u32 = 2;
    pub const PLANK: u32 = 3;
    pub const LEDGE: u32 = 4;
    pub const SLOPE: u32 = 5;
    pub const WATER: u32 = 6;
    pub const SPIKES: u32 = 7;
    pub const CASTLE: u32 = 8;
}

/// Whether a neighbour tile shows earth's edge (grass on top, light or
/// shadow on the sides).
fn open(t: Tile) -> bool {
    !matches!(t, Tile::Solid { .. } | Tile::Slope { .. })
}

/// What a tile looks like, as a key (kind in bits 24..31, edges, variant;
/// earth's slope surface from bit 32, see [`surface_bits`]); `None` for
/// empty tiles.
fn tile_key(level: &Level, t: Tile, tx: i32, ty: i32) -> Option<u64> {
    let edges = |same: &dyn Fn(Tile) -> bool| -> u32 {
        u32::from(!same(level.tile(tx, ty - 1)))
            | u32::from(!same(level.tile(tx - 1, ty))) << 1
            | u32::from(!same(level.tile(tx + 1, ty))) << 2
    };
    let solid_like = |n: Tile| !open(n);
    Some(u64::from(match t {
        Tile::Empty => return None,
        Tile::Solid { art: 0 } => {
            let stone = |n: Tile| matches!(n, Tile::Solid { art: 1 });
            let (variant, shape) = rare_detail(level, tx, ty).unwrap_or((earth_variant(tx, ty), 0));
            let bits = tk::EARTH << 24
                | edges(&solid_like)
                | variant << 3
                | shape << 18
                | depth_below_surface(level, tx, ty).min(7) << 7
                | u32::from(stone(level.tile(tx - 1, ty))) << 10
                | u32::from(stone(level.tile(tx + 1, ty))) << 11
                | side_depth(level, tx - 1, ty) << 12
                | side_depth(level, tx + 1, ty) << 15;
            return Some(surface_bits(level, tx, ty) << 32 | u64::from(bits));
        }
        Tile::Solid { .. } => {
            let stone = |n: Tile| matches!(n, Tile::Solid { art: 1 });
            let e = edges(&solid_like);
            // A pillar: the cap where the stone ends upwards, the plinth
            // where an open side meets the ground on the next row down.
            let role = if !stone(level.tile(tx, ty - 1)) {
                stone_role::CAP
            } else if (e & 2 != 0 && !open(level.tile(tx - 1, ty + 1))) || (e & 4 != 0 && !open(level.tile(tx + 1, ty + 1))) {
                stone_role::PLINTH
            } else {
                stone_role::SHAFT
            };
            tk::STONE << 24 | e | role << 3 | (hash(tx, ty, 21) & 3) << 5
        }
        Tile::OneWay { art } => {
            let same = |n: Tile| n == t;
            let e = edges(&same) >> 1;
            (if art == 0 { tk::PLANK } else { tk::LEDGE }) << 24 | e
        }
        Tile::Slope { from, to } => tk::SLOPE << 24 | (from as u32) << 8 | to as u32,
        Tile::Water { .. } => {
            let surface = !matches!(level.tile(tx, ty - 1), Tile::Water { .. });
            tk::WATER << 24 | u32::from(surface)
        }
        Tile::Spikes => tk::SPIKES << 24,
        Tile::Decoration { .. } => {
            let same = |n: Tile| matches!(n, Tile::Decoration { .. });
            let e = edges(&same);
            let window = e == 0 && hash(tx, ty, 9).is_multiple_of(4) && matches!(level.tile(tx, ty + 1), Tile::Decoration { .. });
            tk::CASTLE << 24 | e | u32::from(window) << 3
        }
    }))
}

/// Tile rows of the pit's fade: frame `row` is that many tiles below the
/// lip, and every row from the last on is the deepest frame.
pub const ABYSS_ROWS: usize = 5;

/// The void of a bottomless pit, `row` tiles below its lip (0 to
/// [`ABYSS_ROWS`] − 1, the last for every row after): a slate blue at the
/// lip fading through four darker steps to black, the steps joined by an
/// ordered dither like the earth's, and a dithered lighter rim along a
/// wall on the `left` or `right`, so the hole reads as depth with sides
/// (review of critic round 2's fixes), not a flat panel. It shares no
/// colour with the earth, the bushes or the sky, so the gap reads as a drop.
fn abyss_tile(row: i32, left: bool, right: bool) -> Img {
    const LADDER: [(u8, u8); 5] = [(ramp::SHADOW, 3), (ramp::SHADOW, 2), (ramp::SHADOW, 1), (ramp::SHADOW, 0), (ramp::INK, 0)];
    let last = LADDER.len() - 1;
    let mut m = Img::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let depth = (row * 16 + y) as f32;
            // Flat bands (14 px each, the first after 2), the last 40 % of
            // each dithered into the next.
            let d = ((depth - 2.0) / 14.0).max(0.0);
            let band = d.floor();
            let t = ((d - band - 0.6) / 0.4).clamp(0.0, 1.0);
            let mut step = (band + if t > bayer(x, y) + 0.5 { 1.0 } else { 0.0 }).min(last as f32) as usize;
            // The wall's face: one step lighter for 2 px, dithered on the 3rd.
            let wall = [(left, x), (right, 15 - x)].iter().filter(|(on, _)| *on).map(|&(_, e)| e).min();
            if wall.is_some_and(|e| e < 2 || (e == 2 && bayer(x, y) < 0.0)) {
                step = step.saturating_sub(1);
            }
            let (r, s) = LADDER[step];
            m.put(x, y, r, s);
        }
    }
    m
}

fn tile_img(key: u64) -> Img {
    let kind = (key >> 24 & 0xFF) as u32;
    let bits = (key & 0xFF_FFFF) as u32;
    match kind {
        tk::EARTH => earth(bits, (key >> 32) as u32),
        tk::STONE => stone(bits),
        tk::CASTLE => bricks(bits, ramp::STONE, 2, bits & 8 != 0),
        tk::PLANK => plank(bits),
        tk::LEDGE => ledge(bits),
        tk::SLOPE => slope(((bits >> 8) & 0xFF) as u8, (bits & 0xFF) as u8),
        tk::SPIKES => spikes(),
        _ => Img::new(16, 16),
    }
}

/// Row (0..=16) of a slope's top pixel in column `x` of its tile: the
/// floor line the hero's foot follows (`Tile::slope_height`) at the
/// column's centre, rounded, so the drawn lip and the collision floor never
/// part by more than half an art pixel. 16: no ground in that column.
pub fn slope_top(from: u8, to: u8, x: i32) -> i32 {
    let h = Tile::Slope { from, to }.slope_height((x as f32 + 0.5) / 16.0).unwrap_or(0.0);
    (16.0 - h).round().clamp(0.0, 16.0) as i32
}

/// The grass lip of a slope `d` px below its floor line: a lit top line,
/// grass, a darker band and a dark seam; `None` below the lip. The slope
/// tile draws it, and so does the earth under it where the lip runs on
/// past the slope tile's bottom (near its low end).
fn slope_lip(d: i32, lit: bool) -> Option<(u8, u8)> {
    match d {
        0 => Some((ramp::GREEN, if lit { 3 } else { 2 })),
        1 | 2 => Some((ramp::GREEN, 2)),
        3 | 4 => Some((ramp::GREEN, 1)),
        5 => Some((ramp::BROWN, 0)),
        _ => None,
    }
}

/// Where the ground's surface is when slope tiles top an earth tile's
/// column or its side neighbours' (earth key bits 32 and up): bit 0 a slope
/// tops this column, bits 1..5 and 6..10 its `from` and `to`, bits 11..15
/// and 16..20 the left and right neighbour column's slope top row
/// ([`slope_top`]) at the shared edge (0 on flat ground). earth() measures
/// the dirt's depth from that line, so the shading and the grass lip follow
/// the slope instead of the tile grid.
fn surface_bits(level: &Level, tx: i32, ty: i32) -> u64 {
    // The slope topping column `tx` at row `ty` (it may be that tile).
    let top = |tx: i32| -> Option<(u8, u8)> {
        if open(level.tile(tx, ty)) {
            return None;
        }
        let d = depth_below_surface(level, tx, ty) as i32;
        match level.tile(tx, ty - d) {
            Tile::Slope { from, to } => Some((from, to)),
            _ => None,
        }
    };
    let mut b = 0u64;
    if let Some((from, to)) = top(tx) {
        b |= 1 | u64::from(from) << 1 | u64::from(to) << 6;
    }
    if let Some((from, to)) = top(tx - 1) {
        b |= (slope_top(from, to, 15) as u64) << 11;
    }
    if let Some((from, to)) = top(tx + 1) {
        b |= (slope_top(from, to, 0) as u64) << 16;
    }
    b
}

/// Rows of solid ground (solid or slope tiles) straight above `(tx, ty)`:
/// 0 at a column's surface.
fn depth_below_surface(level: &Level, tx: i32, ty: i32) -> u32 {
    let mut d = 0;
    while d < 15 && !open(level.tile(tx, ty - 1 - d as i32)) {
        d += 1;
    }
    d
}

/// Depth of the ground beside a tile (key bits 12..14 and 15..17), so the
/// darkness blends across columns; 0 when that side is open (earth()
/// then uses the tile's own depth on that side).
fn side_depth(level: &Level, tx: i32, ty: i32) -> u32 {
    if open(level.tile(tx, ty)) { 0 } else { depth_below_surface(level, tx, ty).min(7) }
}

/// Earth detail variants (key bits 3..6): 0..=12 are common dirt (pebbles,
/// rocks, clods), the rest are rare.
mod earth_v {
    pub const BONE: u32 = 13;
    pub const FOSSIL: u32 = 14;
    pub const BOULDER: u32 = 15;
}

/// The variant of the earth tile at `(tx, ty)`: a hash, so the pattern
/// never repeats in step with the tile grid; boulders are rare. Bones and
/// fossils come from [`rare_detail`] only.
fn earth_variant(tx: i32, ty: i32) -> u32 {
    // The hash's high bits (its low bits follow x too closely).
    let h = hash(tx, ty, 3);
    match (h >> 16) % 64 {
        2 | 3 => earth_v::BOULDER,
        _ => (h >> 4) % 13,
    }
}

/// Strips of whole columns (tiles wide) that hold at most one buried bone
/// or fossil.
const RARE_W: i32 = 10;
/// Distinct buried pictures ([`BURIED`]): four fossils and four bones.
const RARE_DESIGNS: i32 = 8;

/// A buried bone or fossil at `(tx, ty)`: `(variant, shape)`, shape
/// `design_in_kind * 2 + turned` (0..7). Each strip of [`RARE_W`] columns
/// holds at most one (every third strip none), on a hashed earth tile at least
/// two tiles deep, at any height. The picture steps with the strip,
/// `strip mod 8` (even: a fossil, odd: a bone), so one design comes back
/// only eight strips on, at least 71 columns away, while the widest view
/// is 40 columns (`view_width_max_px` 640) and a portrait phone's under
/// 19: no screen the game can show, at any height, holds one picture twice
/// (look bible: rare details, 16 variants, no visible repeat on one
/// screen; critic round 1, item 7, and round 2, item 3: two turns of one
/// spiral on the spikes screen still read as the same fossil).
fn rare_detail(level: &Level, tx: i32, ty: i32) -> Option<(u32, u32)> {
    let sx = tx.div_euclid(RARE_W);
    // Every third strip stays plain (in step, so each of the eight designs
    // still turns up within 24 strips).
    if sx.rem_euclid(3) == 2 {
        return None;
    }
    let mut spots = Vec::new();
    for y in 0..level.h as i32 {
        for x in sx * RARE_W..(sx + 1) * RARE_W {
            if matches!(level.tile(x, y), Tile::Solid { art: 0 }) && depth_below_surface(level, x, y) >= 2 {
                spots.push((x, y));
            }
        }
    }
    if spots.is_empty() {
        return None;
    }
    let pick = spots[hash(sx, 0, 51) as usize % spots.len()];
    if pick != (tx, ty) {
        return None;
    }
    let design = sx.rem_euclid(RARE_DESIGNS) as u32;
    let turned = hash(sx, 0, 57) & 1;
    let kind = if design & 1 == 0 { earth_v::FOSSIL } else { earth_v::BONE };
    Some((kind, (design >> 1) * 2 + turned))
}

/// The earth's shades from the surface down: grass-lit brown, brown, then
/// the darker soil ramp (index 2 is the surface dirt).
const EARTH_LADDER: [(u8, u8); 7] = [
    (ramp::BROWN, 3),
    (ramp::BROWN, 2),
    (ramp::BROWN, 1),
    (ramp::BROWN, 0),
    (ramp::SOIL, 2),
    (ramp::SOIL, 1),
    (ramp::SOIL, 0),
];

/// Earth ladder step at `px` art pixels below the surface: flat bands (one
/// every 22 px, the first after 10) joined by a short ordered dither.
fn earth_step(px: f32, x: i32, y: i32) -> usize {
    let d = ((px - 10.0) / 22.0).max(-0.5);
    let band = d.floor();
    let f = d - band;
    // Each band is flat for 60 %, then dithers into the next.
    let t = ((f - 0.6) / 0.4).clamp(0.0, 1.0);
    let step = band + if t > bayer(x, y) + 0.5 { 1.0 } else { 0.0 };
    (2.0 + step.max(0.0)).min(6.0) as usize
}

fn put_step(m: &mut Img, x: i32, y: i32, i: usize) {
    let (r, s) = EARTH_LADDER[i.min(6)];
    m.put(x, y, r, s);
}

/// A small boulder lit from the top-left, in the stone ramp, with a contact
/// shadow on its lower right.
fn rock_in(m: &mut Img, cx: f32, cy: f32, rx: f32, ry: f32, dim: u8) {
    for y in (cy - ry - 1.0) as i32..=(cy + ry + 1.0) as i32 {
        for x in (cx - rx - 1.0) as i32..=(cx + rx + 1.0) as i32 {
            let (nx, ny) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
            if nx * nx + ny * ny > 1.0 {
                let (sx, sy) = ((x as f32 - 0.5 - cx) / rx, (y as f32 - 0.5 - cy) / ry);
                if sx * sx + sy * sy <= 1.0 {
                    m.put(x, y, ramp::SOIL, 0);
                }
                continue;
            }
            let l = -(nx * 0.6 + ny * 0.8) + bayer(x, y) * 0.35;
            let s: u8 = if l > 0.55 {
                3
            } else if l > -0.1 {
                2
            } else if l > -0.6 {
                1
            } else {
                0
            };
            m.put(x, y, ramp::STONE, s.saturating_sub(dim));
        }
    }
}

/// The eight buried pictures (critic round 2, item 3: visibly different
/// shapes, not turns of one): four fossils in the stone ramp, then four
/// bones in the white ramp. `#` is the body, `+` a groove (fossils) or a
/// hole (bones), `.` the earth. Each fits 14 × 14 with its shadow, so a
/// 16 px tile holds it with a pixel to spare.
const BURIED: [&[&str]; RARE_DESIGNS as usize] = [
    // Fossil: a coiled ammonite.
    &["..####..", ".#....#.", "#..##..#", "#.#..#.#", "#.#.##.#", "#..#...#", ".#....#.", "..####.."],
    // Bone: a long bone with knobbed ends.
    &["##.......##", "###########", ".#########.", "###########", "##.......##"],
    // Fossil: a trilobite, segmented with a middle lobe.
    &[".#####.", "##+#+##", "#+###+#", ".#+#+#.", "##+#+##", ".#+#+#.", "##+#+##", "..#+#.."],
    // Bone: a skull with two eye holes and teeth.
    &["..#####..", ".#######.", "#########", "#++###++#", "#++###++#", ".###+###.", "..#####..", "..#.#.#.."],
    // Fossil: a fish skeleton, head left, ribs, forked tail.
    &[".##..#.#.#..#", "####.#.#.#.##", "#+###########", "####.#.#.#.##", ".##..#.#.#..#"],
    // Bone: a ribcage hanging from its spine.
    &[".#########.", "#.#.#.#.#.#", "#.#.#.#.#.#", "#.#.#.#.#.#", ".#.#.#.#.#."],
    // Fossil: a ribbed scallop shell.
    &["..#####..", ".#+#+#+#.", "#+#+#+#+#", "#+#+#+#+#", ".#+#+#+#.", "..#+#+#..", "...###..."],
    // Bone: a jaw with its teeth up.
    &[".#.#.#.#....", "##########..", "###########.", "........####", ".........###"],
];

/// Draw buried picture `design` ([`BURIED`]) with its top-left at
/// `(x, y)`, turned a quarter (lying on its side) when `turned`. The
/// shading follows each pixel's place in the turned picture (lit top-left,
/// dark bottom-right) and a soil shadow falls on its lower right, so every
/// turn keeps the scene's light.
fn buried_in(m: &mut Img, x: i32, y: i32, design: usize, turned: bool) {
    let Some(rows) = BURIED.get(design) else {
        return;
    };
    let (r, groove) = if design.is_multiple_of(2) { (ramp::STONE, (ramp::STONE, 0)) } else { (ramp::WHITE, (ramp::SOIL, 0)) };
    let h = rows.len() as i32;
    let w = rows.iter().map(|s| s.len()).max().unwrap_or(0) as i32;
    let (tw, th) = if turned { (h, w) } else { (w, h) };
    let mut cells = Vec::new();
    for (j, row) in rows.iter().enumerate() {
        for (i, ch) in row.bytes().enumerate() {
            if ch == b'.' {
                continue;
            }
            let (i, j) = (i as i32, j as i32);
            // A quarter turn clockwise: column i becomes row i.
            let (u, v) = if turned { (h - 1 - j, i) } else { (i, j) };
            cells.push((u, v, ch == b'+'));
        }
    }
    for &(u, v, _) in &cells {
        if !cells.iter().any(|&(a, b, _)| (a, b) == (u + 1, v + 1)) {
            m.put(x + u + 1, y + v + 1, ramp::SOIL, 0);
        }
    }
    for &(u, v, hole) in &cells {
        if hole {
            m.put(x + u, y + v, groove.0, groove.1);
            continue;
        }
        let t = u as f32 / tw.max(1) as f32 + v as f32 / th.max(1) as f32;
        let s = if t < 0.55 {
            3
        } else if t < 1.15 {
            2
        } else {
            1
        };
        m.put(x + u, y + v, r, s);
    }
}

/// Width and height of buried picture `design`, turned or not.
fn buried_size(design: usize, turned: bool) -> (i32, i32) {
    let rows = BURIED.get(design).copied().unwrap_or(&[]);
    let (w, h) = (rows.iter().map(|s| s.len()).max().unwrap_or(0) as i32, rows.len() as i32);
    if turned { (h, w) } else { (w, h) }
}

/// Roots hanging from the grass: one or two strands wandering down,
/// lighter than the dirt, each with a rootlet.
fn roots_in(m: &mut Img, seed: u32, from_y: i32) {
    let n = 1 + (hash(1, 1, seed) & 1) as i32;
    for r in 0..n {
        let mut x = 2 + (hash(r, 2, seed) % 12) as i32;
        let len = 5 + (hash(r, 3, seed) % 7) as i32;
        for y in from_y..(from_y + len).min(16) {
            m.put(x, y, ramp::BROWN, 2);
            if hash(x, y, seed).is_multiple_of(3) {
                m.put(x + 1, y, ramp::BROWN, 0);
            }
            if hash(y, r, seed + 7).is_multiple_of(3) {
                x += if hash(y, x, seed).is_multiple_of(2) { 1 } else { -1 };
                x = x.clamp(1, 14);
            }
        }
        let ry = from_y + 2 + (hash(r, 4, seed) % 3) as i32;
        let dir = if hash(r, 5, seed).is_multiple_of(2) { 1 } else { -1 };
        m.put(x + dir, ry, ramp::BROWN, 2);
        m.put(x + 2 * dir, ry + 1, ramp::BROWN, 2);
    }
}

/// Stone blocks of a neighbouring pillar bonded into the dirt along the
/// left (or right) side, and a dark seam where stone meets earth.
fn stone_seam(m: &mut Img, right: bool, seed: u32) {
    let edge = if right { 15 } else { 0 };
    let inward = if right { -1 } else { 1 };
    for y in 0..16 {
        m.put(edge, y, ramp::SOIL, 0);
    }
    // Chunks 2-4 px deep and 2-4 px tall, a few px apart.
    let mut y = (hash(0, 0, seed) % 4) as i32;
    while y < 15 {
        let h = (2 + (hash(y, 1, seed) % 3) as i32).min(16 - y);
        let w = 2 + (hash(y, 2, seed) % 3) as i32;
        for j in 0..h {
            for i in 0..w {
                let s = if j == 0 {
                    2
                } else if j == h - 1 || i == w - 1 {
                    0
                } else {
                    1
                };
                m.put(edge + inward * i, y + j, ramp::STONE, s);
            }
            m.put(edge + inward * w, y + j, ramp::SOIL, 0);
        }
        y += h + 2 + (hash(y, 3, seed) % 4) as i32;
    }
}

/// Earth (Chris's review, point 2): dirt that darkens with depth below the
/// column's surface (brown, then the darker soil ramp; flat bands joined
/// by an ordered dither, continuous across tile rows), clods and specks,
/// one of 16 hashed detail variants (pebbles, stone rocks, a boulder, and
/// rarely a bone or a fossil below the surface row), roots under the
/// grass, grass when the top is open, light on an open left side, shadow
/// on an open right side, and stone bonded in with a seam next to a stone
/// pillar. Key bits: 0..2 open top/left/right, 3..6 variant, 7..9 depth in
/// tiles, 10/11 stone on the left/right, 12..14 and 15..17 the left and
/// right neighbours' depth, 18..20 a bone's or fossil's shape
/// ([`rare_detail`]). `surf`: slopes topping this column and its
/// neighbours ([`surface_bits`]); depth is then measured from the slope's
/// line, and the slope's grass lip runs on into this tile near its low end.
fn earth(bits: u32, surf: u32) -> Img {
    let (top, left, right) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
    let variant = bits >> 3 & 15;
    let depth = (bits >> 7 & 7) as i32;
    let (stone_l, stone_r) = (bits >> 10 & 1 != 0, bits >> 11 & 1 != 0);
    let shape = bits >> 18 & 7;
    let slope = (surf & 1 != 0).then_some(((surf >> 1 & 31) as u8, (surf >> 6 & 31) as u8));
    let seed = 11 + variant * 7 + depth as u32 * 131 + shape * 977;
    let mut m = Img::new(16, 16);
    // Depth (px) of the tile's row 0 in column x: whole tiles of ground
    // above, less the top row of a slope topping the column. It blends
    // across the tile from the neighbours' (half-way at each edge), so
    // columns of different depth meet without a step.
    let (dl, dr) = ((bits >> 12 & 7) as f32, (bits >> 15 & 7) as f32);
    let (sl, sr) = ((surf >> 11 & 31) as f32, (surf >> 16 & 31) as f32);
    let dc = depth as f32;
    let own = |x: i32| dc * 16.0 - slope.map_or(0.0, |(f, t)| slope_top(f, t, x) as f32);
    let edge_l = if left { own(0) } else { (dl * 16.0 - sl + own(0)) * 0.5 };
    let edge_r = if right { own(15) } else { (dr * 16.0 - sr + own(15)) * 0.5 };
    let col_px = |x: i32| {
        let f = (x as f32 + 0.5) / 16.0;
        if f < 0.5 { own(x) + (edge_l - own(0)) * (1.0 - f * 2.0) } else { own(x) + (edge_r - own(15)) * (f * 2.0 - 1.0) }
    };
    let below_at = |x: i32, y: i32| col_px(x) + y as f32;
    for y in 0..16 {
        for x in 0..16 {
            let mut i = earth_step(below_at(x, y), x, y);
            // Clods: a darker speck, now and then a lighter one.
            let h = hash(x, y, seed);
            if h.is_multiple_of(19) {
                i = (i + 1).min(6);
            } else if h.is_multiple_of(41) {
                i = i.saturating_sub(1).max(1);
            }
            put_step(&mut m, x, y, i);
        }
    }
    let at = |k: i32, lo: i32, span: u32| lo + (hash(k, variant as i32, seed) % span) as i32;
    // Stones deeper down catch less light.
    let dim = u8::from(depth >= 2);
    match variant {
        earth_v::BONE | earth_v::FOSSIL if depth >= 1 => {
            // Fossil designs are even, bones odd ([`BURIED`]); shape is the
            // design within its kind and the turn ([`rare_detail`]).
            let design = ((shape >> 1) * 2 + u32::from(variant == earth_v::BONE)) as usize;
            let turned = shape & 1 != 0;
            let (w, h) = buried_size(design, turned);
            // Anywhere inside the tile with a pixel to spare (and the shadow).
            let (x, y) = (at(1, 1, (15 - w).max(1) as u32), at(2, 1, (15 - h).max(1) as u32));
            buried_in(&mut m, x, y, design, turned);
        }
        earth_v::BOULDER => rock_in(&mut m, at(1, 5, 6) as f32, at(2, 8, 4) as f32 + 0.5, 4.5, 3.5, dim),
        6 | 7 => rock_in(&mut m, at(1, 3, 10) as f32, at(2, 4, 10) as f32, 2.5 + (variant - 6) as f32 * 0.5, 2.0, dim),
        8 => {
            rock_in(&mut m, at(1, 2, 5) as f32, at(2, 7, 4) as f32, 2.0, 1.5, dim);
            rock_in(&mut m, at(3, 9, 5) as f32, at(4, 10, 4) as f32, 2.0, 1.5, dim);
        }
        _ => {
            // Pebbles of the dirt around them: lit top-left, shadow under.
            for k in 0..(variant % 3) as i32 {
                let (x, y) = (at(k * 2 + 1, 2, 11), at(k * 2 + 2, 7, 7));
                let i = earth_step(below_at(x, y), x, y);
                put_step(&mut m, x, y, i.saturating_sub(2));
                put_step(&mut m, x + 1, y, i.saturating_sub(1));
                put_step(&mut m, x, y + 1, i.saturating_sub(1));
                put_step(&mut m, x + 1, y + 1, i);
                put_step(&mut m, x + 2, y + 1, i + 1);
                put_step(&mut m, x, y + 2, i + 1);
                put_step(&mut m, x + 1, y + 2, i + 1);
            }
        }
    }
    if top && variant.is_multiple_of(2) {
        roots_in(&mut m, seed, 6);
    }
    if stone_l {
        stone_seam(&mut m, false, seed);
    }
    if stone_r {
        stone_seam(&mut m, true, seed + 1);
    }
    if left {
        for y in 0..16 {
            let i = earth_step(below_at(0, y), 0, y);
            put_step(&mut m, 0, y, i.saturating_sub(2));
            put_step(&mut m, 1, y, i.saturating_sub(1));
        }
    }
    if right {
        for y in 0..16 {
            let i = earth_step(below_at(15, y), 15, y);
            put_step(&mut m, 15, y, i + 1);
        }
    }
    if top {
        for x in 0..16 {
            let gd = 4 + (hash(x, 0, seed) % 3) as i32;
            for y in 0..gd {
                let s = if y == 0 {
                    3
                } else if y < gd - 2 {
                    2
                } else {
                    1
                };
                m.put(x, y, ramp::GREEN, s);
            }
            m.put(x, gd, ramp::BROWN, 0);
            // Blades of light along the top.
            if hash(x, 5, seed).is_multiple_of(3) {
                m.put(x, 1, ramp::GREEN, 3);
            }
        }
        if left {
            for y in 0..8 {
                m.put(0, y, ramp::GREEN, 3);
                m.put(1, y, ramp::GREEN, if y < 6 { 2 } else { 1 });
            }
        }
        if right {
            for y in 0..7 {
                m.put(15, y, ramp::GREEN, 1);
            }
        }
    }
    // A slope's grass lip running on below the slope tile.
    if let Some((from, to)) = slope {
        for x in 0..16 {
            let s = slope_top(from, to, x);
            for y in 0..16 {
                if let Some((r, sh)) = slope_lip(depth * 16 + y - s, to > from) {
                    m.put(x, y, r, sh);
                }
            }
        }
    }
    m
}

/// Stone tile roles (key bits 3..4).
mod stone_role {
    pub const SHAFT: u32 = 0;
    pub const CAP: u32 = 1;
    pub const PLINTH: u32 = 2;
}

/// One dressed stone block lit from the top-left, `x0..x1` by `y0..y1`
/// (exclusive), with a dark joint on its right and bottom.
fn block(m: &mut Img, x0: i32, x1: i32, y0: i32, y1: i32, seed: u32) {
    for y in y0..y1 {
        for x in x0..x1 {
            let s = if x == x1 - 1 || y == y1 - 1 {
                0
            } else if y == y0 || x == x0 || hash(x, y, seed).is_multiple_of(11) {
                // Lit top and left edges, and a fleck of light on the face.
                2
            } else {
                1
            };
            m.put(x, y, ramp::STONE, s);
        }
    }
    m.put(x0, y0, ramp::STONE, 3);
    m.put(x0 + 1, y0, ramp::STONE, 3);
}

/// A dressed-stone pillar (Chris's review, point 5): big ashlar blocks in
/// 8 px courses; the cap is a lit slab over a moulding (walkable, a little
/// moss on top), the plinth a moulding over a base course where the
/// pillar meets the ground beside it. The shaft is inset 1 px on an open
/// side so the cap and plinth read wider. Key bits: 0..2 open
/// top/left/right, 3..4 role, 5..6 variant.
fn stone(bits: u32) -> Img {
    let (left, right) = (bits & 2 != 0, bits & 4 != 0);
    let role = bits >> 3 & 3;
    let seed = 40 + (bits >> 5 & 3);
    let mut m = Img::new(16, 16);
    let (xl, xr) = (i32::from(left), 16 - i32::from(right));
    let shaft = |m: &mut Img, y0: i32, y1: i32| {
        for (c, cy) in [(0u32, 0), (1, 8)] {
            let (a, b) = (cy.max(y0), (cy + 8).min(y1));
            if a >= b {
                continue;
            }
            if (c + seed).is_multiple_of(2) {
                block(m, xl, xr, a, b, seed);
            } else {
                block(m, xl, 8, a, b, seed);
                block(m, 8, xr, a, b, seed + 1);
            }
        }
    };
    match role {
        stone_role::CAP => {
            shaft(&mut m, 6, 16);
            for x in 0..16 {
                m.put(x, 0, ramp::STONE, 3);
                m.put(x, 1, ramp::STONE, 2);
                m.put(x, 2, ramp::STONE, 2);
                m.put(x, 3, ramp::STONE, 0);
                m.put(x, 4, ramp::STONE, if x < 2 { 3 } else { 2 });
                m.put(x, 5, ramp::STONE, 0);
            }
            m.put(15, 1, ramp::STONE, 1);
            m.put(15, 2, ramp::STONE, 1);
            // Moss on the slab.
            for k in 0..3 {
                let x = 2 + (hash(k, 9, seed) % 12) as i32;
                m.put(x, 0, ramp::GREEN, 2);
                m.put(x + 1, 0, ramp::GREEN, 3);
                m.put(x, 1, ramp::GREEN, 1);
            }
        }
        stone_role::PLINTH => {
            shaft(&mut m, 0, 9);
            for x in 0..16 {
                m.put(x, 9, ramp::STONE, 0);
                m.put(x, 10, ramp::STONE, if x < 2 { 3 } else { 2 });
                m.put(x, 11, ramp::STONE, 1);
                m.put(x, 12, ramp::STONE, 3);
                for y in 13..16 {
                    m.put(x, y, ramp::STONE, if y == 15 { 0 } else { 1 });
                }
            }
            m.put(0, 13, ramp::STONE, 2);
            m.put(0, 14, ramp::STONE, 2);
        }
        _ => shaft(&mut m, 0, 16),
    }
    m
}

/// Staggered bricks lit from the top-left. `base` is the face shade
/// (castle walls are a step lighter than the stone wall).
fn bricks(bits: u32, r: u8, base: u8, window: bool) -> Img {
    let (top, left, right) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
    let mut m = Img::new(16, 16);
    for y in 0..16 {
        let row = y / 4;
        let off = if row % 2 == 0 { 0 } else { 4 };
        for x in 0..16 {
            let bx = (x + off) % 8;
            let by = y % 4;
            let s = if bx == 7 || by == 3 {
                base - 1
            } else if by == 0 || bx == 0 {
                (base + 1).min(3)
            } else if by == 2 && bx == 6 {
                base - 1
            } else {
                base
            };
            m.put(x, y, r, s);
        }
    }
    if window {
        for y in 4..13 {
            for x in 6..10 {
                let s = if y == 4 || x == 6 { 1 } else { 0 };
                m.put(x, y, ramp::INK, s);
            }
        }
        for x in 5..11 {
            m.put(x, 13, r, 3);
        }
    }
    if top {
        for x in 0..16 {
            m.put(x, 0, r, 3);
            m.put(x, 1, r, base.max(2));
        }
    }
    if left {
        for y in 0..16 {
            m.put(0, y, r, 3);
        }
    }
    if right {
        for y in 0..16 {
            m.put(15, y, r, 0);
        }
    }
    m
}

/// A wooden plank (one-way), 6 px thick, rounded open ends, with a bracket.
fn plank(bits: u32) -> Img {
    let (left, right) = (bits & 1 != 0, bits & 2 != 0);
    let mut m = Img::new(16, 16);
    for y in 0..6 {
        for x in 0..16 {
            let s = match y {
                0 => 3,
                5 => 0,
                4 => 1,
                _ if hash(x / 3, y, 5).is_multiple_of(4) => 1,
                _ => 2,
            };
            m.put(x, y, ramp::BROWN, s);
        }
        m.put(7, y, ramp::BROWN, if y == 0 { 2 } else { 0 });
    }
    for (x, y) in [(2, 2), (12, 2)] {
        m.put(x, y, ramp::STONE, 3);
        m.put(x + 1, y + 1, ramp::STONE, 0);
    }
    if left {
        m.set(0, 0, CLEAR);
        m.set(0, 5, CLEAR);
        for y in 1..5 {
            m.put(0, y, ramp::BROWN, 3);
        }
    }
    if right {
        m.set(15, 0, CLEAR);
        m.set(15, 5, CLEAR);
        for y in 1..5 {
            m.put(15, y, ramp::BROWN, 0);
        }
    }
    // A bracket under each end.
    for (on, x0) in [(left, 2), (right, 11)] {
        if on {
            for y in 6..10 {
                let w = 10 - y;
                for x in 0..w {
                    let xx = if x0 == 2 { x0 + x } else { x0 + 3 - x };
                    m.put(xx, y, ramp::BROWN, if x == 0 { 2 } else { 1 });
                }
            }
        }
    }
    m
}

/// A grassy floating ledge (one-way) with a rounded earth underside.
fn ledge(bits: u32) -> Img {
    let (left, right) = (bits & 1 != 0, bits & 2 != 0);
    let mut m = Img::new(16, 16);
    for y in 0..11 {
        for x in 0..16 {
            let inset = (y - 3).max(0);
            if (left && x < inset) || (right && 15 - x < inset) {
                continue;
            }
            let ragged = y == 10 && hash(x, 0, 21).is_multiple_of(2);
            if ragged {
                continue;
            }
            if y < 4 {
                let s = if y == 0 { 3 } else if y < 3 { 2 } else { 1 };
                m.put(x, y, ramp::GREEN, s);
            } else {
                let edge_l = left && x == inset;
                let edge_r = right && 15 - x == inset;
                let s = if y == 4 || y >= 9 || edge_r {
                    0
                } else if edge_l {
                    2
                } else {
                    1
                };
                m.put(x, y, ramp::BROWN, s);
            }
        }
    }
    if left {
        m.set(0, 0, CLEAR);
    }
    if right {
        m.set(15, 0, CLEAR);
    }
    m
}

/// A slope: a floor line from `from` to `to` px above the tile bottom,
/// grass along it (brighter facing up-left), the earth's dirt below (its
/// shade by depth under the line, with clods). The line is the collision
/// floor ([`slope_top`]); the earth tile underneath carries the lip on
/// where it runs past this tile's bottom.
fn slope(from: u8, to: u8) -> Img {
    let mut m = Img::new(16, 16);
    let lit = to > from;
    for x in 0..16 {
        let ys = slope_top(from, to, x);
        for y in ys..16 {
            let d = y - ys;
            if let Some((r, s)) = slope_lip(d, lit) {
                m.put(x, y, r, s);
                continue;
            }
            let mut i = earth_step(d as f32, x, y);
            let h = hash(x, y, 13);
            if h.is_multiple_of(19) {
                i = (i + 1).min(6);
            } else if h.is_multiple_of(41) {
                i = i.saturating_sub(1).max(1);
            }
            put_step(&mut m, x, y, i);
        }
    }
    m
}

/// Two steel spikes lit from the left on a dark mount.
fn spikes() -> Img {
    let mut m = Img::new(16, 16);
    for y in 5..14 {
        for x in 0..16 {
            let lx = (x % 8) as f32 + 0.5;
            if (lx - 4.0).abs() <= (y - 5) as f32 * 0.5 + 0.5 {
                let s = if y == 5 || lx < 4.0 { 3 } else { 1 };
                m.put(x, y, ramp::STONE, s);
            }
        }
    }
    for x in 0..16 {
        m.put(x, 14, ramp::STONE, 1);
        m.put(x, 15, ramp::STONE, 0);
    }
    m
}

/// Water frame `k` of 4: pixels keep their place and step through a run
/// of shades (palette cycling), so bands of light drift across.
fn water(key: u32, k: i32) -> Img {
    let surface = key & 1 != 0;
    let mut m = Img::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            if surface && y < 4 {
                continue;
            }
            let (run, c) = if surface && y == 4 {
                ([3, 3, 2, 2], (x / 2) % 4)
            } else if surface && y == 5 {
                ([2, 3, 2, 2], (x / 3) % 4)
            } else if surface {
                ([2, 1, 1, 1], ((x + 2 * y) / 3) % 4)
            } else {
                ([1, 0, 0, 0], ((x + 2 * y) / 4) % 4)
            };
            let s = run[((c - k).rem_euclid(4)) as usize];
            m.put(x, y, ramp::WATER, s);
        }
    }
    m
}

/// Torch flame frame `k` of 4 (palette-cycled bands rising).
fn flame(k: i32) -> Img {
    const RUN: [(u8, u8); 4] = [(ramp::GOLD, 3), (ramp::GOLD, 2), (ramp::RED, 2), (ramp::GOLD, 2)];
    let mut m = Img::new(8, 12);
    for y in 0..12 {
        for x in 0..8 {
            let (fx, fy) = (x as f32 + 0.5 - 4.0, y as f32 + 0.5);
            // Teardrop: narrow at the top, round at the bottom.
            let half = if fy < 7.0 { fy * 0.5 } else { (3.6 - (fy - 7.0) * 0.55).max(0.0) };
            if fx.abs() > half {
                continue;
            }
            let core = fx.abs() < half * 0.45 && fy > 5.0;
            if core {
                m.put(x, y, ramp::WHITE, 3);
                continue;
            }
            let c = (y / 2 + i32::from(fx.abs() > half * 0.75)) % 4;
            let (r, s) = RUN[((c + k).rem_euclid(4)) as usize];
            m.put(x, y, r, s);
        }
    }
    m
}

/// Moving platform segment: 0 left end, 1 middle, 2 right end.
fn platform(part: u8) -> Img {
    let mut m = Img::new(16, 8);
    for y in 0..8 {
        for x in 0..16 {
            let (r, s) = match y {
                0 => (ramp::GOLD, 3),
                1 => (ramp::GOLD, 1),
                7 => (ramp::STONE, 0),
                6 => (ramp::STONE, 1),
                _ => {
                    let bx = x % 8;
                    if bx == 7 {
                        (ramp::STONE, 0)
                    } else if bx == 0 || y == 2 {
                        (ramp::STONE, 3)
                    } else {
                        (ramp::STONE, 2)
                    }
                }
            };
            m.put(x, y, r, s);
        }
    }
    if part == 0 {
        m.set(0, 0, CLEAR);
        m.set(0, 7, CLEAR);
        for y in 1..7 {
            m.put(0, y, ramp::STONE, 3);
        }
        m.put(0, 1, ramp::GOLD, 3);
    }
    if part == 2 {
        m.set(15, 0, CLEAR);
        m.set(15, 7, CLEAR);
        for y in 1..7 {
            m.put(15, y, ramp::STONE, 0);
        }
    }
    // Rivets.
    for x in [3, 11] {
        m.put(x, 4, ramp::GOLD, 2);
    }
    m
}

/// The castle gate: a dark doorway under a stone arch with the portcullis
/// raised, 48 × 48.
fn gate() -> Img {
    let mut m = Img::new(48, 48);
    for y in 0..48 {
        for x in 0..48 {
            let (dx, dy) = (x as f32 + 0.5 - 24.0, y as f32 + 0.5 - 22.0);
            let inside = y >= 22 || dx * dx + dy * dy <= 20.0 * 20.0;
            let inner = (2..46).contains(&x) && (y >= 22 || dx * dx + dy * dy <= 18.5 * 18.5);
            if !inside {
                let row = y / 4;
                let bx = (x + if row % 2 == 0 { 0 } else { 4 }) % 8;
                let s = if bx == 7 || y % 4 == 3 { 1 } else if y % 4 == 0 { 3 } else { 2 };
                m.put(x, y, ramp::STONE, s);
            } else if !inner {
                // Arch stones: lit on the upper left.
                let s = if dx < 0.0 && dy < 6.0 { 3 } else { 1 };
                m.put(x, y, ramp::STONE, s);
            } else {
                // Dark hall, a little light near the floor.
                let s = if y > 40 { 1 } else { 0 };
                m.put(x, y, ramp::INK, s);
            }
        }
    }
    // Raised portcullis: bars hanging in the arch.
    for x in (6..44).step_by(6) {
        for y in 4..12 {
            if m.get(x, y) == ix(ramp::INK, 0) {
                m.put(x, y, ramp::STONE, 2);
                m.put(x + 1, y, ramp::STONE, 0);
            }
        }
        m.put(x, 12, ramp::STONE, 3);
    }
    for x in 5..44 {
        if m.get(x, 8) != ix(ramp::STONE, 2) && m.get(x, 8) == ix(ramp::INK, 0) {
            m.put(x, 8, ramp::STONE, 1);
        }
    }
    m
}

/// Periodic hill height: sum of sines whose periods divide `w`.
fn ridge(x: f32, w: f32, waves: &[(f32, f32, f32)]) -> f32 {
    waves.iter().map(|&(n, amp, ph)| amp * (core::f32::consts::TAU * n * x / w + ph).sin()).sum()
}

/// Shade of a hill pixel `d` px under its rim on a slope rising by `rise`
/// (up-right is negative rise: faces the light).
fn hill_shade(d: i32, rise: f32, x: i32, y: i32) -> u8 {
    let faces_light = rise > 0.05;
    let v = if d < 2 {
        if faces_light { 3.0 } else { 2.0 }
    } else {
        let base = if faces_light { 1.9 } else { 1.4 };
        base - d as f32 * 0.035 + bayer(x, y) * 0.5
    };
    v.round().clamp(0.0, 3.0) as u8
}

/// The far plane's size.
const FAR_W: u32 = 512;
const FAR_H: u32 = 96;

/// Height of the far plane's front hills at `x`.
fn far_front(x: f32) -> f32 {
    34.0 + ridge(x, FAR_W as f32, &[(3.0, 10.0, 1.9), (7.0, 5.0, 0.4), (13.0, 2.0, 1.0)])
}

/// Far plane (512 × 96): pale mountains and rolling hills. The castle on
/// its hilltop is a sprite of its own ([`far_castle`]), drawn once every
/// two strips, so a wide view never shows it twice.
fn far_plane() -> Img {
    let (w, h) = (FAR_W, FAR_H);
    let mut m = Img::new(w, h);
    let wf = w as f32;
    let back = |x: f32| 58.0 + ridge(x, wf, &[(2.0, 14.0, 0.7), (5.0, 7.0, 2.1), (9.0, 3.0, 0.3)]);
    let front = far_front;
    for x in 0..w as i32 {
        let xf = x as f32;
        let (b, f) = (back(xf), front(xf));
        let (rb, rf) = (back(xf + 1.0) - back(xf - 1.0), front(xf + 1.0) - front(xf - 1.0));
        for y in 0..h as i32 {
            let up = h as f32 - y as f32;
            if up <= f {
                let d = (f - up) as i32;
                m.put(x, y, ramp::LEAF, hill_shade(d, rf, x, y).min(2));
            } else if up <= b {
                let d = (b - up) as i32;
                let s = if d < 2 && rb > 0.05 { 3 } else { 2 };
                m.put(x, y, ramp::SKY, s);
            }
        }
    }
    m
}

/// Where the far castle stands in the far plane: its centre column.
const FAR_CASTLE_X: i32 = 330;

/// The far castle (24 × 30) and where its top-left corner sits in the far
/// plane's pixels: a small castle on the front hills, its towers sunk two
/// rows into the hill, with a pennant.
fn far_castle() -> (Img, [f32; 2]) {
    let (w, h) = (24, 30);
    let cx = FAR_CASTLE_X;
    let top = (FAR_H as f32 - far_front(cx as f32)) as i32;
    let at = [cx - 12, top - 28];
    let mut m = Img::new(w, h);
    let mut put = |x: i32, y: i32, r: u8, s: u8| m.put(x - at[0], y - at[1], r, s);
    for (x0, w0, hgt) in [(cx - 12, 6, 20), (cx - 6, 12, 14), (cx + 6, 6, 22)] {
        for y in top - hgt..top + 2 {
            for x in x0..x0 + w0 {
                let s = if x == x0 { 3 } else if x == x0 + w0 - 1 { 1 } else { 2 };
                put(x, y, ramp::STONE, s);
            }
        }
        // Crenels.
        for x in (x0..x0 + w0).step_by(2) {
            put(x, top - hgt - 1, ramp::STONE, 3);
        }
        put(x0 + w0 / 2, top - hgt + 5, ramp::INK, 1);
    }
    // A pennant.
    put(cx + 9, top - 27, ramp::STONE, 1);
    put(cx + 9, top - 26, ramp::STONE, 1);
    put(cx + 9, top - 25, ramp::STONE, 1);
    put(cx + 10, top - 27, ramp::RED, 2);
    put(cx + 11, top - 27, ramp::RED, 2);
    put(cx + 10, top - 26, ramp::RED, 1);
    (m, [at[0] as f32, at[1] as f32])
}

/// Mid plane (512 × 80): green hills topped with round trees.
fn mid_plane() -> Img {
    let (w, h) = (512, 80);
    let mut m = Img::new(w, h);
    let wf = w as f32;
    let hill = |x: f32| 30.0 + ridge(x, wf, &[(4.0, 8.0, 0.2), (9.0, 4.0, 1.7), (16.0, 1.5, 0.9)]);
    // Trees first (behind the hill fronts), as lit blobs.
    let mut list: Vec<Blob> = Vec::new();
    let mut x = 6.0;
    let mut i = 0;
    while x < wf - 6.0 {
        let r = 6.0 + (hash(i, 0, 31) % 5) as f32;
        list.push((x, h as f32 - hill(x) - r * 0.4, r));
        x += r * 1.4 + (hash(i, 1, 31) % 16) as f32;
        i += 1;
    }
    // Tile seamlessly: repeat the trees one period to each side.
    let wrapped: Vec<Blob> = list.iter().flat_map(|&(x, y, r)| [(x - wf, y, r), (x, y, r), (x + wf, y, r)]).collect();
    blobs(&mut m, &wrapped, ramp::LEAF, -0.1);
    for x in 0..w as i32 {
        let xf = x as f32;
        let f = hill(xf);
        let rise = hill(xf + 1.0) - hill(xf - 1.0);
        for y in 0..h as i32 {
            let up = h as f32 - y as f32;
            if up <= f {
                m.put(x, y, ramp::GREEN, hill_shade((f - up) as i32, rise, x, y));
            }
        }
    }
    m
}

/// Height of the near plane's bushes; [`MIST`] rows of fade hang below.
const BUSH_H: u32 = 44;

/// Rows below the bushes where their dark base fades into a pale, cool
/// mist (over the valley haze, [`HAZE`]), so a gap in the ground
/// line shows a lighter, less saturated backdrop and never reads as
/// ground (critic round 1, item 2; look bible "Sky and backdrop"). The
/// plane is placed by its bushes' base, so the mist hangs below it.
pub const MIST: u32 = 20;

/// The fade under the bushes, dark to light: bush shadow, leaf greens, the
/// pale sky blue the valley haze starts from.
const MIST_LADDER: [(u8, u8); 5] = [(ramp::GREEN, 0), (ramp::LEAF, 1), (ramp::LEAF, 2), (ramp::LEAF, 3), (ramp::SKY, 3)];

/// Rows under the mist: the valley below the hedge, three rows of far
/// treetops in blue haze. With the camera framing the ground at 72 %, a
/// drop seen from high ground shows this much of the backdrop under the
/// hedge; it stays sky-ramp blue (never ground) but is never one flat
/// colour (review of critic round 2's fixes).
pub const HAZE: u32 = 52;

/// Near plane (256 × 116): a row of round bushes over a dithered fade into
/// mist, then the hazy valley of treetops.
fn near_plane() -> Img {
    let (w, h) = (256, BUSH_H + MIST + HAZE);
    let mut m = Img::new(w, h);
    let mut list: Vec<Blob> = Vec::new();
    let mut x = 0.0;
    let mut i = 0;
    while x < w as f32 + 10.0 {
        let r = 9.0 + (hash(i, 0, 41) % 7) as f32;
        let cy = BUSH_H as f32 - 8.0 - (hash(i, 1, 41) % 8) as f32;
        list.push((x, cy, r));
        x += r * 1.25;
        i += 1;
    }
    // Tile seamlessly: repeat the blobs one period to each side.
    let wrapped: Vec<Blob> = list
        .iter()
        .flat_map(|&(x, y, r)| [(x - w as f32, y, r), (x, y, r), (x + w as f32, y, r)])
        .collect();
    blobs(&mut m, &wrapped, ramp::GREEN, -0.05);
    // The bushes' base: their shadow between and under the blobs (as
    // before), then one ladder step every 4 rows, joined by an ordered
    // dither, down to flat mist on the last rows.
    let base = BUSH_H as i32 - 8;
    let last = MIST_LADDER.len() - 1;
    let haze_top = (BUSH_H + MIST) as i32;
    for y in base..haze_top {
        for x in 0..w as i32 {
            if m.opaque(x, y) && y < BUSH_H as i32 - 3 {
                continue;
            }
            let t = (y - (BUSH_H as i32 - 5)) as f32 / 4.0;
            let step = if t < 0.0 { 0 } else { ((t + bayer(x, y) + 0.5).floor().max(0.0) as usize).min(last) };
            let (r, s) = MIST_LADDER[step];
            m.put(x, y, r, s);
        }
    }
    valley_haze(&mut m, haze_top);
    m
}

/// Per column, the top edge of a row of round crowns (radius `r0` plus up
/// to 3, tops `y0` to 6 rows lower), tiling seamlessly across the strip's
/// width.
fn crown_line(w: i32, k: i32, y0: i32, r0: f32) -> Vec<f32> {
    let wf = w as f32;
    let mut line = vec![f32::MAX; w as usize];
    let mut x = 0.0;
    let mut i = 0;
    while x < wf {
        let r = r0 + (hash(i, k, 43) % 4) as f32;
        let cy = y0 as f32 + r + (hash(i, k + 10, 43) % 7) as f32;
        for c in [x - wf, x, x + wf] {
            for (px, top) in line.iter_mut().enumerate() {
                let dx = px as f32 + 0.5 - c;
                if dx.abs() < r {
                    *top = top.min(cy - (r * r - dx * dx).sqrt());
                }
            }
        }
        x += r * 1.5;
        i += 1;
    }
    line
}

/// The valley under the mist (rows `top` to the strip's bottom): three rows
/// of treetops stepping down, each crown with a dithered lit rim, the row
/// behind shaded just above the crowns in front. Sky ramp only, so it reads
/// as far haze, never as ground; the last rows are the plane's fill colour.
fn valley_haze(m: &mut Img, top: i32) {
    let w = m.w as i32;
    let rows = [crown_line(w, 0, top - 2, 4.0), crown_line(w, 1, top + 13, 6.0), crown_line(w, 2, top + 29, 8.0)];
    for y in top..m.h as i32 {
        let fy = y as f32 + 0.5;
        for x in 0..w {
            let col = x as usize;
            let (r, s) = match rows.iter().rposition(|l| fy >= l[col]) {
                None => (ramp::SKY, 3),
                Some(k) => {
                    let lit = fy - rows[k][col] < 2.0;
                    let shaded = rows.get(k + 1).is_some_and(|l| l[col] - fy < 2.0);
                    if lit && bayer(x, y) < 0.0 {
                        (ramp::SKY, 3)
                    } else if shaded && bayer(x, y) < 0.0 {
                        (ramp::SKY, 1)
                    } else {
                        (ramp::SKY, 2)
                    }
                }
            };
            m.put(x, y, r, s);
        }
    }
}

/// A cloud: lit white blobs.
fn cloud(w: u32, h: u32, seed: u32) -> Img {
    let mut m = Img::new(w, h);
    let wf = w as f32;
    let hf = h as f32;
    let list: Vec<Blob> = vec![
        (wf * 0.25, hf * 0.62, hf * 0.38),
        (wf * 0.45, hf * 0.45, hf * 0.45 + (seed % 2) as f32),
        (wf * 0.68, hf * 0.55, hf * 0.42),
        (wf * 0.85, hf * 0.7, hf * 0.28),
        (wf * 0.12, hf * 0.75, hf * 0.24),
    ];
    blobs(&mut m, &list, ramp::WHITE, 0.55);
    // Flat underside.
    for y in (h as i32 - 2)..h as i32 {
        for x in 0..w as i32 {
            m.set(x, y, CLEAR);
        }
    }
    for x in 0..w as i32 {
        let y = h as i32 - 3;
        if m.opaque(x, y) {
            m.put(x, y, ramp::WHITE, 1);
        }
    }
    m
}

/// A leafy bush, 24 × 14.
fn bush() -> Img {
    let mut m = Img::new(24, 14);
    blobs(&mut m, &[(6.0, 9.0, 5.5), (12.0, 6.5, 6.5), (18.5, 9.0, 5.5)], ramp::GREEN, 0.0);
    for x in 1..23 {
        if m.opaque(x, 13) {
            m.put(x, 13, ramp::GREEN, 0);
        }
    }
    m
}

/// A round tree, 30 × 44: a lit canopy on a trunk.
fn tree() -> Img {
    let mut m = Img::new(30, 44);
    for y in 22..44 {
        for x in 12..18 {
            let s = if x == 12 { 3 } else if x >= 16 { 1 } else { 2 };
            m.put(x, y, ramp::BROWN, s);
        }
    }
    for x in 10..20 {
        m.put(x, 43, ramp::BROWN, if x < 12 { 2 } else { 1 });
    }
    let mut c = Img::new(30, 30);
    blobs(
        &mut c,
        &[(15.0, 11.0, 10.0), (8.0, 17.0, 7.5), (22.0, 17.0, 7.5), (15.0, 21.0, 8.0)],
        ramp::LEAF,
        0.05,
    );
    m.blit(&c, 0, 0);
    // A few berries.
    for (x, y) in [(10, 12), (19, 9), (21, 19), (13, 22)] {
        m.put(x, y, ramp::RED, 2);
    }
    m
}

/// A mossy rock, 16 × 10.
fn rock() -> Img {
    let mut m = Img::new(16, 10);
    blobs(&mut m, &[(6.0, 7.0, 5.5), (10.5, 6.5, 5.0)], ramp::STONE, 0.0);
    for x in 0..16 {
        for y in 0..10 {
            if m.opaque(x, y) && !m.opaque(x, y - 1) && x < 10 {
                m.put(x, y, ramp::GREEN, 2);
            }
        }
    }
    for y in 8..10 {
        for x in 0..16 {
            if y == 9 && m.opaque(x, y) {
                m.put(x, y, ramp::STONE, 0);
            }
        }
    }
    m
}

/// A four-point sparkle, 7 × 7 (frame 1 is smaller).
/// Narrowest and widest drop shadow (art px); a body wider than the widest
/// gets the widest.
pub const SHADOW_MIN_W: u32 = 4;
pub const SHADOW_MAX_W: u32 = 24;

/// A drop shadow's height for its width: 3 px under small bodies, 4 from
/// 10 px wide.
pub fn shadow_height(w: u32) -> u32 {
    if w >= 10 { 4 } else { 3 }
}

/// A flat white ellipse `w` px wide (tinted when drawn).
fn shadow_ellipse(w: u32) -> Img {
    let h = shadow_height(w);
    let mut img = Img::new(w, h);
    let (rx, ry) = (w as f32 * 0.5, h as f32 * 0.5);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = ((x as f32 + 0.5 - rx) / rx, (y as f32 + 0.5 - ry) / ry);
            if nx * nx + ny * ny <= 1.0 {
                img.set(x as i32, y as i32, WHITE);
            }
        }
    }
    img
}

fn sparkle(k: i32) -> Img {
    let mut m = Img::new(7, 7);
    let r: i32 = if k == 0 { 3 } else { 2 };
    for d in -r..=r {
        let s = if d.abs() <= 1 { 3 } else { 2 };
        m.put(3 + d, 3, ramp::GOLD, s);
        m.put(3, 3 + d, ramp::GOLD, s);
    }
    m.put(3, 3, ramp::WHITE, 3);
    m
}

/// Radius of the hero's glow (art px); its frame is `2 * GLOW_R + 1` square.
pub const GLOW_R: i32 = 13;

/// A soft light around the hero, in pixels only: a disc of pale gold whose
/// ordered dither thins from the centre to the rim (no blur, no gradient),
/// white at the heart. Frame `k` (0..3) is fainter each step, so the glow
/// fades by its pattern rather than by transparency.
fn glow(k: i32) -> Img {
    let n = (GLOW_R * 2 + 1) as u32;
    let mut m = Img::new(n, n);
    let strength = [1.0, 0.7, 0.42][k.clamp(0, 2) as usize];
    for y in 0..n as i32 {
        for x in 0..n as i32 {
            let (dx, dy) = ((x - GLOW_R) as f32, (y - GLOW_R) as f32);
            let d = (dx * dx + dy * dy).sqrt() / (GLOW_R as f32 + 0.5);
            if d > 1.0 {
                continue;
            }
            // Dense in the middle, gone at the rim.
            let density = (1.0 - d) * 1.35 * strength;
            if density - 0.5 <= bayer(x, y) {
                continue;
            }
            if d < 0.3 * strength {
                m.put(x, y, ramp::WHITE, 3);
            } else {
                m.put(x, y, ramp::GOLD, if d < 0.6 { 3 } else { 2 });
            }
        }
    }
    m
}

/// The extra-life burst: eight short rays flying out from the hero, frame
/// `k` (0..3) further out and shorter. Rays are orange with a white tip;
/// diagonals start nearer in (they reach as far as the straight ones).
fn burst(k: i32) -> Img {
    let r = GLOW_R + 4;
    let n = (r * 2 + 1) as u32;
    let mut m = Img::new(n, n);
    let inner = 6 + k * 3;
    let len = 6 - k;
    for (ux, uy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
        let diag = ux != 0 && uy != 0;
        let (start, l) = if diag { ((inner * 7) / 10, len.max(1)) } else { (inner, len) };
        for i in 0..l {
            let d = start + i;
            let tip = i + 1 == l;
            // A deep orange body (darker than the pale gold glow behind, so
            // the rays stand out on the pale sky) with a white tip, two
            // pixels wide: the second pixel is the ray's dark orange edge.
            let (rp, s, es) = if tip { (ramp::WHITE, 3, 3) } else { (ramp::GOLD, 1, if diag { 1 } else { 0 }) };
            m.put(r + ux * d, r + uy * d, rp, s);
            let (px, py) = if diag { (r + ux * d - ux, r + uy * d) } else { (r + ux * d + uy, r + uy * d + ux) };
            m.put(px, py, rp, es);
        }
    }
    m
}

/// Dust puff frame `k` (growing, thinning).
fn dust(k: i32) -> Img {
    let w = 6 + k as u32 * 2;
    let mut m = Img::new(w, 5);
    let r = 1.6 + k as f32 * 0.5;
    let list = [(r, 5.0 - r, r), (w as f32 - r, 5.0 - r, r)];
    blobs(&mut m, &list, ramp::WHITE, 0.5 - k as f32 * 0.2);
    m
}

/// Stomp puff frame `k`: a ring of small clouds.
fn puff(k: i32) -> Img {
    let mut m = Img::new(18, 12);
    let spread = 4.0 + k as f32 * 2.5;
    let r = 3.2 - k as f32 * 0.8;
    let list = [(9.0 - spread, 8.0, r), (9.0 + spread, 8.0, r), (9.0 - spread * 0.5, 5.0, r), (9.0 + spread * 0.5, 5.0, r)];
    blobs(&mut m, &list, ramp::WHITE, 0.4);
    m
}

/// A glyph: white fading to gold, outlined in ink with a drop shadow.
fn glyph(rows: &[&str], mini: bool) -> Img {
    let h = rows.len() as u32;
    let w = rows.first().map_or(1, |r| r.len()) as u32;
    let mut m = Img::new(w, h);
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '#' {
                let (r, s) = if mini {
                    if y < 2 { (ramp::WHITE, 3) } else { (ramp::GOLD, 3) }
                } else if y < 3 {
                    (ramp::WHITE, 3)
                } else if y < 5 {
                    (ramp::WHITE, 2)
                } else {
                    (ramp::GOLD, 3)
                };
                m.put(x as i32, y as i32, r, s);
            }
        }
    }
    m.glyph_frame()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ART_JSON, LEVEL_JSON};

    fn art() -> (ArtData, Level) {
        (ArtData::from_json(ART_JSON).unwrap(), Level::from_json(LEVEL_JSON).unwrap())
    }

    /// Critic round 1, item 7 and round 2, item 3: no buried design (a
    /// fossil or bone picture, turned or not) twice on any screen the game
    /// can show: every window of the widest view's columns (640 art px,
    /// up to 41 tile columns with the partly shown ones) at every scroll,
    /// over the level's whole height, which covers the 1080p, 1440p and
    /// phone views. The 16 pictures all differ, and so do the 8 designs'
    /// silhouettes, so no two read as one shape.
    #[test]
    fn buried_bones_and_fossils_never_repeat_on_one_screen() {
        let (_, l) = art();
        let mut rare = Vec::new();
        for ty in 0..l.h as i32 {
            for tx in 0..l.w as i32 {
                let Some(key) = tile_key(&l, l.tile(tx, ty), tx, ty) else { continue };
                let bits = key as u32;
                let v = bits >> 3 & 15;
                if bits >> 24 == tk::EARTH && (v == earth_v::BONE || v == earth_v::FOSSIL) {
                    let shape = bits >> 18 & 7;
                    let design = (shape >> 1) * 2 + u32::from(v == earth_v::BONE);
                    rare.push((tx, ty, design, shape & 1));
                }
            }
        }
        assert!(rare.iter().filter(|r| r.2 % 2 == 1).count() >= 4, "{rare:?}");
        assert!(rare.iter().filter(|r| r.2 % 2 == 0).count() >= 4, "{rare:?}");
        let designs: std::collections::BTreeSet<u32> = rare.iter().map(|r| r.2).collect();
        assert_eq!(designs.len(), RARE_DESIGNS as usize, "every design is used: {rare:?}");
        let t = crate::Tune::default();
        let cols = (t.view_width_max_px / crate::level::TILE).ceil() as i32 + 1;
        assert!(cols >= 41, "{cols}");
        let mut windows = 0;
        for x0 in -cols..l.w as i32 {
            let on: Vec<_> = rare.iter().filter(|r| (x0..x0 + cols).contains(&r.0)).collect();
            for (i, a) in on.iter().enumerate() {
                for b in &on[i + 1..] {
                    assert!(a.2 != b.2, "{a:?} and {b:?} on one screen at column {x0}");
                }
            }
            windows += 1;
        }
        assert!(windows > l.w);
        // Each strip of columns holds at most one.
        for (i, a) in rare.iter().enumerate() {
            for b in &rare[i + 1..] {
                assert!(a.0.div_euclid(RARE_W) != b.0.div_euclid(RARE_W), "{a:?} {b:?}");
            }
        }
        // The pictures themselves differ (same spot, every design and turn),
        // and so do the designs' outlines (any turn of one against any turn
        // of another), so no two designs are turns of one shape.
        let pic = |d: usize, turned: bool| {
            let mut m = Img::new(16, 16);
            buried_in(&mut m, 1, 1, d, turned);
            m
        };
        let n = RARE_DESIGNS as usize;
        let pics: Vec<Img> = (0..n * 2).map(|k| pic(k / 2, k % 2 == 1)).collect();
        for i in 0..pics.len() {
            assert!(pics[i].px.iter().filter(|&&p| p != CLEAR).count() >= 16, "picture {i} is too small");
            for j in i + 1..pics.len() {
                assert_ne!(pics[i], pics[j], "pictures {i} and {j}");
            }
        }
        for a in 0..n {
            for b in a + 1..n {
                for ta in [false, true] {
                    for tb in [false, true] {
                        assert_ne!(buried_mask(a, ta), buried_mask(b, tb), "designs {a} and {b}");
                    }
                }
            }
        }
        // Every picture with its shadow stays inside its tile.
        for d in 0..n {
            for turned in [false, true] {
                let (w, h) = buried_size(d, turned);
                assert!(w + 2 <= 16 && h + 2 <= 16, "design {d}: {w} x {h}");
            }
        }
    }

    /// The set cells of a buried design, turned or not, from its top-left.
    fn buried_mask(d: usize, turned: bool) -> Vec<(i32, i32)> {
        let mut m = Img::new(16, 16);
        buried_in(&mut m, 0, 0, d, turned);
        let mut v = Vec::new();
        for y in 0..16 {
            for x in 0..16 {
                if m.opaque(x, y) {
                    v.push((x, y));
                }
            }
        }
        v
    }

    /// Critic round 1, item 2: under the near bushes the backdrop fades to
    /// a pale mist, so a gap in the ground line never shows a dark green
    /// that reads as ground. Review of the round-2 fixes: below the mist the
    /// lower camera shows a hazy valley of treetops, never one flat band.
    #[test]
    fn near_plane_fades_into_a_pale_mist_over_a_hazy_valley() {
        let m = near_plane();
        assert_eq!(m.h, BUSH_H + MIST + HAZE);
        let haze_top = (BUSH_H + MIST) as i32;
        let sky_only = |y: i32| (0..m.w as i32).all(|x| (ix(ramp::SKY, 0)..=ix(ramp::SKY, 3)).contains(&m.get(x, y)));
        // The mist's last row is the palest sky shade all along.
        for x in 0..m.w as i32 {
            assert_eq!(m.get(x, haze_top - 1), ix(ramp::SKY, 3), "{x}");
        }
        // The haze is sky ramp only, and every 8-row band of it shows
        // crowns (at least two shades), down to its last rows.
        for y in haze_top..m.h as i32 {
            assert!(sky_only(y), "row {y}");
        }
        for y0 in (haze_top..haze_top + 40).step_by(8) {
            let mut seen = std::collections::BTreeSet::new();
            for y in y0..y0 + 8 {
                for x in 0..m.w as i32 {
                    seen.insert(m.get(x, y));
                }
            }
            assert!(seen.len() >= 2, "rows {y0}..{}: {seen:?}", y0 + 8);
        }
        // The last row is one colour (the plane fills on with it).
        let last = m.get(0, m.h as i32 - 1);
        assert!((0..m.w as i32).all(|x| m.get(x, m.h as i32 - 1) == last));
        let (a, l) = art();
        let colors = Colors::new(&a, None);
        let built = Art::build(&a, &l, &colors, 1);
        let luma = |c: [f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let fill = built.plane_fill[2];
        // Lighter than the grass and the earth, and bluer than it is green.
        assert!(luma(fill) > luma(colors.shade(ramp::GREEN, 2)), "{fill:?}");
        assert!(luma(fill) > luma(colors.shade(ramp::BROWN, 3)), "{fill:?}");
        assert!(fill[2] > fill[1], "{fill:?}");
    }

    /// Review of the round-2 fixes: the pit fades from a slate lip into
    /// black and its walls catch a lighter rim, so it reads as depth.
    #[test]
    fn pit_fades_from_its_lip_into_black_with_lit_walls() {
        let lip = abyss_tile(0, false, false);
        assert_eq!(lip.get(8, 2), ix(ramp::SHADOW, 3));
        let deep = abyss_tile(ABYSS_ROWS as i32 - 1, false, false);
        assert!((0..16).all(|x| (0..16).all(|y| deep.get(x, y) == ix(ramp::INK, 0))));
        let (walled, open) = (abyss_tile(2, true, true), abyss_tile(2, false, false));
        for y in 0..16 {
            for x in [0, 1, 14, 15] {
                assert_ne!(walled.get(x, y), open.get(x, y), "{x},{y}");
            }
            assert_eq!(walled.get(8, y), open.get(8, y), "{y}");
        }
    }

    #[test]
    fn art_json_is_valid() {
        let (a, _) = art();
        assert_eq!(a.sprite("hero_idle0").w, 14);
        assert!(ArtData::from_json(&ART_JSON.replace("\"hero_jump\"", "\"hero_jmp\"")).unwrap_err().contains("hero_jump"));
        let bad = ART_JSON.replacen("......d...d...", "......d...d..", 1);
        assert!(ArtData::from_json(&bad).unwrap_err().contains("row 1"));
        let bad = ART_JSON.replacen("......d...d...", "......d...d..Q", 1);
        assert!(ArtData::from_json(&bad).unwrap_err().contains("unknown key"));
    }

    #[test]
    fn sprites_are_lit_from_the_top_left_and_outlined() {
        let (a, _) = art();
        let img = a.sprite("coin0");
        // Top-left edge brighter than bottom-right edge of the same ramp.
        let top = img.get(5, 0);
        let bottom = img.get(5, 10);
        assert_eq!(ramp_of(top), ramp::GOLD);
        assert!(shade_of(top) > shade_of(bottom), "{} {}", shade_of(top), shade_of(bottom));
        let o = img.outlined();
        assert_eq!((o.w, o.h), (img.w + 2, img.h + 2));
        // Lit side: the gold ramp's darkest; shaded side: ink.
        assert_eq!(o.get(3, 0), ix(ramp::GOLD, 0));
        assert_eq!(o.get(3, o.h as i32 - 1), ix(ramp::INK, 0));
    }

    #[test]
    fn water_and_fire_cycle_the_palette() {
        let f: Vec<Img> = (0..4).map(|k| water(tk::WATER << 24 | 1, k)).collect();
        assert_ne!(f[0], f[1]);
        assert_eq!(water(1, 4), water(1, 0));
        // Same pixels, only the shades move: the set of shades per frame is
        // the same multiset.
        let count = |m: &Img| {
            let mut c = [0usize; 256];
            for p in &m.px {
                c[*p as usize] += 1;
            }
            c
        };
        assert_eq!(f.iter().filter(|m| m.opaque(0, 3)).count(), 0);
        assert!(f.iter().all(|m| count(m)[CLEAR as usize] == count(&f[0])[CLEAR as usize]));
        assert_ne!(flame(0), flame(1));
    }

    #[test]
    fn sheet_holds_every_frame_and_tile() {
        let (a, l) = art();
        let colors = Colors::new(&a, None);
        let built = Art::build(&a, &l, &colors, 7);
        let s = &built.sheet;
        assert!(s.is_valid());
        assert_eq!(s.revision, 7);
        assert!(s.height <= 2048, "{}", s.height);
        assert_eq!(built.tiles.len(), l.w * l.h);
        // Every non-empty tile has art; water cycles; the castle is behind.
        for ty in 0..l.h as i32 {
            for tx in 0..l.w as i32 {
                let t = l.tile(tx, ty);
                let art = built.tile(tx, ty);
                match t {
                    Tile::Empty => assert!(art == TileArt::None || matches!(art, TileArt::Back(_)), "{tx},{ty}"),
                    Tile::Water { .. } => assert!(matches!(art, TileArt::Cycle(_))),
                    Tile::Decoration { .. } => assert!(matches!(art, TileArt::Back(_))),
                    _ => assert!(matches!(art, TileArt::Still(_)), "{tx},{ty}"),
                }
            }
        }
        // Cells point inside the frame table.
        let n = s.frames.len() as u16;
        for f in built.hero.iter().chain(&built.glyphs).chain(&built.planes) {
            assert!(f.cell >= evani_render::SHEET_CELL0 && f.cell < evani_render::SHEET_CELL0 + n);
        }
        // The hero frame is 16 wide with its outline.
        assert_eq!(built.hero[0].w, 16.0);
    }

    #[test]
    fn look_palettes_recolour_the_art() {
        let (a, l) = art();
        let mut look = LookData::default();
        look.palettes.push(evani_core::plugin::Palette { name: "ramp.fur".into(), colors: vec![[1.0, 0.0, 0.0, 1.0]; 4] });
        let c = Colors::new(&a, Some(&look));
        assert_eq!(c.shade(ramp::FUR, 2), [1.0, 0.0, 0.0, 1.0]);
        assert_ne!(c.shade(ramp::TEAL, 2), [1.0, 0.0, 0.0, 1.0]);
        let plain = Art::build(&a, &l, &Colors::new(&a, None), 1);
        let red = Art::build(&a, &l, &c, 1);
        assert_ne!(plain.sheet.rgba8, red.sheet.rgba8);
    }

    /// Chris's review of the slopes: every slope in the level is drawn
    /// along the collision floor, with an unbroken grass lip under it and
    /// nothing above it, at every column (uphill, downhill, gentle and
    /// steep, where slopes meet flat ground and in the valley), the lip
    /// running on into the earth tile below near a slope's low end.
    #[test]
    fn slopes_draw_along_the_collision_floor_without_notches() {
        let (_, level) = art();
        let img = |tx: i32, ty: i32| tile_key(&level, level.tile(tx, ty), tx, ty).map(tile_img);
        let px = |tx: i32, ty: i32, x: i32, y: i32| img(tx, ty).map_or(CLEAR, |m| m.get(x, y));
        let mut slopes = 0;
        for ty in 0..level.h as i32 {
            for tx in 0..level.w as i32 {
                let t = level.tile(tx, ty);
                let Tile::Slope { from, to } = t else { continue };
                slopes += 1;
                assert!(open(level.tile(tx, ty - 1)), "slope at ({tx}, {ty}) has ground above");
                for x in 0..16 {
                    let s = slope_top(from, to, x);
                    // The drawn top and the collision floor at the column's
                    // centre are within half an art pixel.
                    let floor = 16.0 - t.slope_height((x as f32 + 0.5) / 16.0).unwrap();
                    assert!((s as f32 - floor).abs() <= 0.5, "({tx}, {ty}) col {x}: top {s}, floor {floor}");
                    for y in 0..s {
                        assert_eq!(px(tx, ty, x, y), CLEAR, "({tx}, {ty}) col {x}: pixel above the line at {y}");
                    }
                    // The lip: five grass rows and a dark seam, then dirt,
                    // crossing into the tile below when it has to.
                    for d in 0..8 {
                        let y = s + d;
                        let (ty2, y2) = (ty + y / 16, y % 16);
                        let p = px(tx, ty2, x, y2);
                        let want = slope_lip(d, to > from).map(|(r, _)| r).unwrap_or(ramp::BROWN);
                        assert_ne!(p, CLEAR, "({tx}, {ty}) col {x}: hole {d} px under the line");
                        if want == ramp::GREEN || d == 5 {
                            assert_eq!(ramp_of(p), want, "({tx}, {ty}) col {x}: {d} px under the line");
                        } else {
                            assert_ne!(ramp_of(p), ramp::GREEN, "({tx}, {ty}) col {x}: grass {d} px under the line");
                        }
                    }
                }
            }
        }
        assert!(slopes >= 12, "{slopes}");
    }
}
