//! Sharp, scalable text (feature `msdf`, which turns on `text`): fonts baked
//! at build time into a multi-channel signed distance field atlas (MTSDF) by
//! the pinned msdf-atlas-gen (`cargo xtask build`, `[ui] fonts` in
//! evani.toml), drawn as quads through the sprite batcher with an MSDF
//! fragment variant (`shaders/sprite_msdf.wgsl`).
//!
//! * Each font ships two files next to the game: `fonts/<name>.png` (the
//!   atlas, decoded by the browser, so no image decoder ships) and
//!   `fonts/<name>.evf` (this module's compact table: metrics, glyph
//!   advances and boxes, kerning pairs; [`MsdfFont::parse`]).
//! * [`Fonts`] (in [`RenderWorld::fonts`](crate::world::RenderWorld)) holds
//!   up to [`MAX_FONTS`] fonts. A kit asks for one by name once
//!   ([`Fonts::request`]) and puts the [`FontId`] in its
//!   [`TextStyle`](crate::text::TextStyle); the renderer fetches and uploads
//!   it. Text in a font that hasn't arrived yet draws nothing.
//! * Layout: advances and kerning in em units, greedy word wrap at
//!   `max_width` (`\n` always breaks, a word longer than the line breaks
//!   between letters, the spaces at a wrap are dropped), left, centre or
//!   right per line. `size_px` is the line height, as for the bitmap font.
//!   Each line's start and baseline land on whole pixels.
//! * A character the font lacks draws as `?`.
//! * Effects per glyph, all from the one atlas: the fill (median of the
//!   three colour channels), an outline and a glow or soft shadow (from the
//!   true distance in the alpha channel), drawn shadow, glow, outline, fill
//!   on the caller's layer.
//! * The shader samples every texture on every fragment and picks the result
//!   with `select`, so the derivatives stay in uniform control flow. A glyph
//!   carries pxrange (> 0 marks it), mode and width in the instance pad.
//!
//! Complex scripts (shaping, right-to-left) and typed input stay in the DOM.

use crate::text::{Align, FontId, TextStyle};
use crate::world::{RenderWorld, Sprite};

/// Module marker: kept in the download by [`MsdfFont::parse`], which every
/// game with the `ui` module reaches, so the size and on/off checks see it.
pub const MODULE_ID: &str = "evani.module.ui";

/// Fonts a game can have loaded at once (the font slot is 2 bits of the
/// sprite cell).
pub const MAX_FONTS: usize = 4;
/// Glyphs per font (10 bits of the sprite cell).
pub const MAX_GLYPHS: usize = 1024;
/// Magic at the start of a `.evf` table.
pub const EVF_MAGIC: &[u8; 4] = b"EVF1";
/// Fixed-point scale of the em values in a `.evf` table.
pub const EM_FIXED: f32 = 4096.0;
/// First sprite cell of MSDF glyphs: `0xC000 | slot << 12 | mode << 10 |
/// glyph`. Cells below are built-in shapes, sheet frames and named art.
pub const MSDF_CELL0: u16 = 0xC000;
/// Outline width in em.
pub const OUTLINE_EM: f32 = 0.08;
/// Glow width in em (and the soft edge of the drop shadow).
pub const GLOW_EM: f32 = 0.11;
/// Drop shadow offset in em (down and right, at least one pixel).
pub const SHADOW_EM: f32 = 0.06;

/// What the fragment stage does with an MSDF glyph quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Mode {
    /// The glyph: median of the three channels, sharp at any scale.
    Fill = 0,
    /// The glyph grown by [`OUTLINE_EM`] (true distance in alpha).
    Outline = 1,
    /// A soft halo out to [`GLOW_EM`] (glow, soft shadow).
    Soft = 2,
    /// Not a glyph: a plain rectangle in the sprite's colour with hard edges,
    /// for box fills (S3-13b). Needs no font; the glyph number is ignored.
    Solid = 3,
}

/// One glyph of a font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub code: u32,
    /// Pen advance in em.
    pub advance: f32,
    /// Quad `[left, top, right, bottom]` in em from the pen on the
    /// baseline, y down. All zero for a glyph with no ink (space).
    pub plane: [f32; 4],
    /// The same quad in atlas pixels `[left, top, right, bottom]`, y down.
    pub atlas: [f32; 4],
}

impl Glyph {
    fn has_ink(&self) -> bool {
        self.plane[2] > self.plane[0] && self.plane[3] > self.plane[1]
    }
}

/// A parsed `.evf` table.
#[derive(Debug, Clone, PartialEq)]
pub struct MsdfFont {
    /// Distance range of the atlas in atlas pixels.
    pub pxrange: f32,
    /// Atlas pixels per em.
    pub em_px: f32,
    /// Line height in em.
    pub line_height: f32,
    /// Ascender above and descender below the baseline, in em (positive).
    pub ascender: f32,
    pub descender: f32,
    /// Atlas size in pixels.
    pub atlas: [u32; 2],
    /// Sorted by code.
    pub glyphs: Vec<Glyph>,
    /// `(left glyph, right glyph, em)`, sorted.
    pub kerning: Vec<(u16, u16, f32)>,
    question: u16,
}

/// A font table error: the full text natively (tests, tools), `font table
/// evf<n>` in the wasm, because every `ui` game downloads the reader (S3-2's
/// short-code rule). Search `evf_err!("<n>"` for the meaning of a code.
macro_rules! evf_err {
    ($n:literal, $text:literal) => {
        if cfg!(target_arch = "wasm32") { concat!("font table evf", $n) } else { concat!("font table: ", $text) }
    };
}
pub(crate) use evf_err;

/// Little-endian reader over a byte slice.
struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

impl Rd<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], &'static str> {
        let s = self
            .b
            .get(self.at..self.at + N)
            .and_then(|s| <[u8; N]>::try_from(s).ok())
            .ok_or(evf_err!("1", "cut short"))?;
        self.at += N;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        self.take::<2>().map(u16::from_le_bytes)
    }
    fn i16(&mut self) -> Result<f32, &'static str> {
        self.take::<2>().map(|b| f32::from(i16::from_le_bytes(b)) / EM_FIXED)
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn f32(&mut self) -> Result<f32, &'static str> {
        let v = self.take::<4>().map(f32::from_le_bytes)?;
        if v.is_finite() { Ok(v) } else { Err(evf_err!("2", "a metric is not a number")) }
    }
}

impl MsdfFont {
    /// Read a `.evf` table: `EVF1`, u16 glyph count, u16 kerning count, u16
    /// atlas width and height, f32 pxrange, em_px, line height, ascender,
    /// descender; per glyph u32 code, i16 advance, i16 x4 plane bounds (em
    /// x 4096), u16 x4 atlas bounds (half pixels); per kerning pair u16
    /// left and right glyph index, i16 value (em x 4096). Glyphs ascend by
    /// code, pairs by (left, right), and `?` is there.
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        // The marker lives here, so it ships exactly with the font reader.
        core::hint::black_box(MODULE_ID);
        let mut r = Rd { b: bytes, at: 0 };
        if &r.take::<4>()? != EVF_MAGIC {
            return Err(evf_err!("3", "not an Evani font table (no EVF1 header)"));
        }
        let (n, k) = (usize::from(r.u16()?), usize::from(r.u16()?));
        let atlas = [u32::from(r.u16()?), u32::from(r.u16()?)];
        let (pxrange, em_px, line_height, ascender, descender) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        if n == 0 || n > MAX_GLYPHS {
            return Err(evf_err!("4", "1 to 1024 glyphs"));
        }
        if pxrange <= 0.0 || em_px <= 0.0 || line_height <= 0.0 || atlas[0] == 0 || atlas[1] == 0 {
            return Err(evf_err!("5", "bad metrics"));
        }
        let mut glyphs = Vec::with_capacity(n);
        for _ in 0..n {
            let code = r.u32()?;
            let advance = r.i16()?;
            let plane = [r.i16()?, r.i16()?, r.i16()?, r.i16()?];
            let mut atlas_px = [0.0; 4];
            for v in &mut atlas_px {
                *v = f32::from(r.u16()?) * 0.5;
            }
            glyphs.push(Glyph { code, advance, plane, atlas: atlas_px });
        }
        if glyphs.windows(2).any(|w| w[0].code >= w[1].code) {
            return Err(evf_err!("6", "glyphs out of order"));
        }
        let mut kerning = Vec::with_capacity(k);
        for _ in 0..k {
            let (a, b, v) = (r.u16()?, r.u16()?, r.i16()?);
            if usize::from(a.max(b)) >= n {
                return Err(evf_err!("7", "a kerning pair names a missing glyph"));
            }
            kerning.push((a, b, v));
        }
        if kerning.windows(2).any(|w| (w[0].0, w[0].1) >= (w[1].0, w[1].1)) {
            return Err(evf_err!("8", "kerning pairs out of order"));
        }
        let question = glyphs
            .binary_search_by_key(&u32::from(b'?'), |g| g.code)
            .map_err(|_| evf_err!("9", "no `?` glyph"))? as u16;
        Ok(Self { pxrange, em_px, line_height, ascender, descender, atlas, glyphs, kerning, question })
    }

    /// The glyph index of `c`; `?` when the font lacks it.
    pub fn index(&self, c: char) -> u16 {
        self.glyphs
            .binary_search_by_key(&(c as u32), |g| g.code)
            .map_or(self.question, |i| i as u16)
    }

    pub fn glyph(&self, i: u16) -> Option<&Glyph> {
        self.glyphs.get(usize::from(i))
    }

    /// Kerning between two glyphs, in em (0 for most pairs).
    pub fn kern(&self, a: u16, b: u16) -> f32 {
        self.kerning
            .binary_search_by(|p| (p.0, p.1).cmp(&(a, b)))
            .map_or(0.0, |i| self.kerning.get(i).map_or(0.0, |p| p.2))
    }

    fn advance(&self, i: u16) -> f32 {
        self.glyph(i).map_or(0.0, |g| g.advance)
    }
}

/// Write a `.evf` table (the inverse of [`MsdfFont::parse`]; xtask has its
/// own writer, this one builds test fixtures).
#[cfg(test)]
pub(crate) fn write_evf(f: &MsdfFont) -> Vec<u8> {
    let fx = |v: f32| ((v * EM_FIXED).round() as i16).to_le_bytes();
    let mut o = EVF_MAGIC.to_vec();
    for v in [f.glyphs.len() as u16, f.kerning.len() as u16, f.atlas[0] as u16, f.atlas[1] as u16] {
        o.extend(v.to_le_bytes());
    }
    for v in [f.pxrange, f.em_px, f.line_height, f.ascender, f.descender] {
        o.extend(v.to_le_bytes());
    }
    for g in &f.glyphs {
        o.extend(g.code.to_le_bytes());
        o.extend(fx(g.advance));
        for v in g.plane {
            o.extend(fx(v));
        }
        for v in g.atlas {
            o.extend(((v * 2.0).round() as u16).to_le_bytes());
        }
    }
    for &(a, b, v) in &f.kerning {
        o.extend(a.to_le_bytes());
        o.extend(b.to_le_bytes());
        o.extend(fx(v));
    }
    o
}

// ---- the fonts a game uses -----------------------------------------------------

/// One font slot: its name (`fonts/<name>.*`), the table once fetched, and
/// where the renderer put its atlas (`[u offset, v offset, u per atlas px,
/// v per atlas px]` in the shared MSDF texture) once uploaded.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSlot {
    pub name: String,
    pub font: Option<MsdfFont>,
    pub placement: Option<[f32; 4]>,
}

/// The MSDF fonts of a game (field [`RenderWorld::fonts`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fonts {
    slots: Vec<FontSlot>,
}

impl Fonts {
    /// The font named `name` (the `name` of its `[ui] fonts` entry), asked
    /// for once, typically when the kit starts; the renderer fetches it. The
    /// same name gives the same id. Past [`MAX_FONTS`] it answers
    /// [`FontId::BITMAP`], so the text still shows in the bitmap font when
    /// feature `text` is on (and draws nothing without it).
    pub fn request(&mut self, name: &str) -> FontId {
        if let Some(i) = self.slots.iter().position(|s| s.name == name) {
            return FontId(i as u8 + 1);
        }
        if self.slots.len() >= MAX_FONTS {
            return FontId::BITMAP;
        }
        self.slots.push(FontSlot { name: name.to_string(), font: None, placement: None });
        FontId(self.slots.len() as u8)
    }

    /// Put a parsed font in a slot (the renderer, once fetched; tests).
    pub fn insert(&mut self, name: &str, font: MsdfFont) -> FontId {
        let id = self.request(name);
        if let Some(s) = self.slot_mut(id) {
            s.font = Some(font);
        }
        id
    }

    pub fn slots(&self) -> &[FontSlot] {
        &self.slots
    }

    pub(crate) fn slot_mut(&mut self, id: FontId) -> Option<&mut FontSlot> {
        usize::from(id.0).checked_sub(1).and_then(|i| self.slots.get_mut(i))
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn slots_mut(&mut self) -> &mut [FontSlot] {
        &mut self.slots
    }

    /// The font's table, once fetched (layout works from it).
    pub fn get(&self, id: FontId) -> Option<&MsdfFont> {
        usize::from(id.0)
            .checked_sub(1)
            .and_then(|i| self.slots.get(i))
            .and_then(|s| s.font.as_ref())
    }

    /// Fetched and on the GPU: its text is on screen.
    pub fn ready(&self, id: FontId) -> bool {
        usize::from(id.0)
            .checked_sub(1)
            .and_then(|i| self.slots.get(i))
            .is_some_and(|s| s.font.is_some() && s.placement.is_some())
    }
}

// ---- layout --------------------------------------------------------------------

/// One laid-out line: glyphs with their pen x in em, and the width in em.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub glyphs: Vec<(u16, f32)>,
    pub width: f32,
}

/// The line being laid out: a running pen, so each glyph costs one kerning
/// lookup however long the line gets.
struct Pen<'a> {
    font: &'a MsdfFont,
    line: Line,
    prev: Option<u16>,
}

impl Pen<'_> {
    /// What glyph `g` adds to the line after `prev` (kerning plus advance).
    fn cost(&self, prev: Option<u16>, g: u16) -> f32 {
        prev.map_or(0.0, |p| self.font.kern(p, g)) + self.font.advance(g)
    }

    fn push(&mut self, g: u16) {
        let x = self.line.width + self.prev.map_or(0.0, |p| self.font.kern(p, g));
        self.line.glyphs.push((g, x));
        self.line.width = x + self.font.advance(g);
        self.prev = Some(g);
    }

    /// Push `g`, first starting a new line if it would not fit (a word
    /// wider than the line breaks between letters).
    fn push_wrapping(&mut self, g: u16, max_em: f32, out: &mut Vec<Line>) {
        if !self.line.glyphs.is_empty() && self.line.width + self.cost(self.prev, g) > max_em {
            out.push(self.take());
        }
        self.push(g);
    }

    fn take(&mut self) -> Line {
        self.prev = None;
        core::mem::take(&mut self.line)
    }
}

/// Lay `text` out in em: `\n` breaks; with `max_em > 0` words wrap
/// greedily (a word longer than a line breaks between letters). All the
/// spaces at a wrap are dropped; spaces inside a line keep their width.
/// Linear in the text length.
pub fn layout(font: &MsdfFont, text: &str, max_em: f32) -> Vec<Line> {
    let mut out = Vec::new();
    let wrap = max_em > 0.0 && max_em.is_finite();
    for para in text.split('\n') {
        let chars: Vec<u16> = para.chars().filter(|c| *c != '\r').map(|c| font.index(c)).collect();
        let mut pen = Pen { font, line: Line::default(), prev: None };
        if !wrap {
            for &g in &chars {
                pen.push(g);
            }
            out.push(pen.take());
            continue;
        }
        let space = |i: usize| para_is_space(&chars, i, font);
        let mut i = 0;
        while i < chars.len() {
            let s0 = i;
            while i < chars.len() && space(i) {
                i += 1;
            }
            let w0 = i;
            while i < chars.len() && !space(i) {
                i += 1;
            }
            let (spaces, word) = (&chars[s0..w0], &chars[w0..i]);
            // The line's width with the spaces and the whole word on it.
            let mut prev = pen.prev;
            let mut width = pen.line.width;
            for &g in spaces.iter().chain(word) {
                width += pen.cost(prev, g);
                prev = Some(g);
            }
            let fits = width <= max_em;
            if word.is_empty() {
                // Trailing spaces: kept if they fit, else dropped.
                if fits {
                    spaces.iter().for_each(|&g| pen.push(g));
                }
                continue;
            }
            if fits || pen.line.glyphs.is_empty() {
                // Fits, or the paragraph's first word (with any leading spaces).
                for &g in spaces.iter().chain(word) {
                    pen.push_wrapping(g, max_em, &mut out);
                }
            } else {
                out.push(pen.take());
                for &g in word {
                    pen.push_wrapping(g, max_em, &mut out);
                }
            }
        }
        out.push(pen.take());
    }
    out
}

/// Is glyph `i` of a laid-out paragraph a space? (Compared by glyph, so a
/// font without a space glyph, where it maps to `?`, wraps nowhere.)
fn para_is_space(chars: &[u16], i: usize, font: &MsdfFont) -> bool {
    chars.get(i).and_then(|&g| font.glyph(g)).is_some_and(|g| g.code == u32::from(b' '))
}

/// Pixels per em at a style's size (`size_px` is the line height).
fn scale_of(font: &MsdfFont, style: &TextStyle) -> f32 {
    let px = if style.size_px.is_finite() { style.size_px.clamp(1.0, 1024.0) } else { 20.0 };
    px / font.line_height
}

fn max_em(style: &TextStyle, scale: f32) -> f32 {
    if style.max_width > 0.0 && style.max_width.is_finite() { style.max_width / scale } else { 0.0 }
}

/// Size `[width, height]` in pixels of `text` in an MSDF font: the widest
/// line by the line count times the line height. `[0, height]` until the
/// font has been fetched.
pub fn measure(fonts: &Fonts, text: &str, style: &TextStyle) -> [f32; 2] {
    let Some(font) = fonts.get(style.font) else {
        return [0.0, style.size_px.max(0.0)];
    };
    let scale = scale_of(font, style);
    let lines = layout(font, text, max_em(style, scale));
    size_of(&lines, font, scale)
}

fn size_of(lines: &[Line], font: &MsdfFont, scale: f32) -> [f32; 2] {
    let w = lines.iter().fold(0.0f32, |m, l| m.max(l.width)) * scale;
    [w.ceil(), (lines.len() as f32 * font.line_height * scale).round()]
}

/// The sprite cell of glyph `glyph` of font slot `slot` in `mode`.
pub fn cell(slot: usize, mode: Mode, glyph: u16) -> u16 {
    MSDF_CELL0 | ((slot as u16 & 3) << 12) | ((mode as u16) << 10) | (glyph & 0x3FF)
}

/// `(slot, mode, glyph)` of an MSDF cell, `None` for any other cell.
pub fn split_cell(cell: u16) -> Option<(usize, u16, u16)> {
    (cell >= MSDF_CELL0).then_some((usize::from((cell >> 12) & 3), (cell >> 10) & 3, cell & 0x3FF))
}

/// Draw `text` in an MSDF font as sprites on `layer` and return its size
/// (as [`measure`]). `pos` is the top left, top middle or top right of the
/// block by the style's alignment; each line aligns on its own. Glyph quads
/// are always axis-aligned (rotation 0): rotated MSDF text is not supported.
pub fn draw(world: &mut RenderWorld, pos: [f32; 2], layer: i16, text: &str, style: &TextStyle) -> [f32; 2] {
    let RenderWorld { fonts, sprites, .. } = world;
    let Some(font) = fonts.get(style.font) else {
        return [0.0, style.size_px.max(0.0)];
    };
    let slot = usize::from(style.font.0).saturating_sub(1);
    let scale = scale_of(font, style);
    let lines = layout(font, text, max_em(style, scale));
    let size = size_of(&lines, font, scale);
    let factor = match style.align {
        Align::Left => 0.0,
        Align::Center => 0.5,
        Align::Right => 1.0,
    };
    let line_px = font.line_height * scale;
    // Half the leading above, so the ink sits in the middle of the line.
    let base_em = (font.line_height - font.ascender - font.descender) * 0.5 + font.ascender;
    let start = sprites.len();
    for (i, line) in lines.iter().enumerate() {
        let x0 = (pos[0] - line.width * scale * factor).round();
        let baseline = (pos[1] + i as f32 * line_px + base_em * scale).round();
        for &(g, x) in &line.glyphs {
            let Some(gl) = font.glyph(g).filter(|gl| gl.has_ink()) else {
                continue;
            };
            let [l, t, r, b] = gl.plane;
            let (w, h) = ((r - l) * scale, (b - t) * scale);
            sprites.push(Sprite {
                pos: [x0 + (x + l) * scale + w * 0.5, baseline + t * scale + h * 0.5],
                size: [w, h],
                rotation: 0.0,
                cell: cell(slot, Mode::Fill, g),
                color: style.color,
                layer,
            });
        }
    }
    // Copies under the fills: shadow, glow, outline, then the fills.
    let fills = sprites.len() - start;
    let off = (SHADOW_EM * scale).round().max(1.0);
    let passes = [
        style.shadow.map(|c| (c, Mode::Soft, off)),
        style.glow.map(|c| (c, Mode::Soft, 0.0)),
        style.outline.map(|c| (c, Mode::Outline, 0.0)),
    ];
    for (color, mode, shift) in passes.into_iter().flatten() {
        for i in start..start + fills {
            if let Some(mut s) = sprites.get(i).copied() {
                s.pos = [s.pos[0] + shift, s.pos[1] + shift];
                s.cell = (s.cell & !(3 << 10)) | ((mode as u16) << 10);
                s.color = color;
                sprites.push(s);
            }
        }
    }
    if let Some(tail) = sprites.get_mut(start..) {
        tail.rotate_left(fills);
    }
    size
}

/// The fragment parameters of an MSDF sprite: `[pxrange, mode, width]`
/// (the sprite's instance pad, `rot.yzw` in the shader). `pxrange > 0` is
/// what marks a glyph; `width` is the outline or glow width as a fraction
/// of the distance range.
pub fn params(font: &MsdfFont, mode: u16) -> [f32; 3] {
    let em = match mode {
        1 => OUTLINE_EM,
        2 => GLOW_EM,
        _ => 0.0,
    };
    // The field reaches half the range past the edge; stay inside it.
    let width = (em * font.em_px / font.pxrange).min(0.49);
    [font.pxrange, f32::from(mode), width]
}

/// The sprite packer with MSDF glyphs: MSDF cells read their font's atlas
/// placement (a glyph of a font not yet on the GPU packs as an empty,
/// clear quad); every other cell goes through [`crate::art::cell_uv`] (with
/// `sprite-art`; without it, the plain built-in cell lookup).
#[cfg_attr(not(feature = "sprite-art"), allow(unused_variables))]
pub fn pack_sprites(src: &[Sprite], fonts: &Fonts, uvs: &[[f32; 4]], art_base: usize, out: &mut Vec<crate::shaders::SpriteRaw>) {
    out.clear();
    out.extend(src.iter().map(|s| {
        let mut raw = crate::shaders::SpriteRaw {
            pos: s.pos,
            size: s.size,
            uv: [0.0; 4],
            color: s.color,
            rotation: s.rotation,
            pad: [0.0; 3],
        };
        match split_cell(s.cell) {
            #[cfg(feature = "sprite-art")]
            None => raw.uv = crate::art::cell_uv(s.cell, uvs, art_base),
            // Without sprite art the atlas holds only the built-in cells and
            // the kit sheet: a plain lookup (`art_base` is unused).
            #[cfg(not(feature = "sprite-art"))]
            None => raw.uv = crate::shaders::sprite_raw(s, uvs).uv,
            Some((_, 3, _)) => raw.pad = [1.0, 3.0, 0.0],
            Some((slot, mode, g)) => {
                let slot = fonts.slots.get(slot);
                let font = slot.and_then(|f| f.font.as_ref());
                match (font, slot.and_then(|f| f.placement), font.and_then(|f| f.glyph(g))) {
                    (Some(font), Some(p), Some(gl)) => {
                        let [l, t, r, b] = gl.atlas;
                        raw.uv = [p[0] + l * p[2], p[1] + t * p[3], p[0] + r * p[2], p[1] + b * p[3]];
                        raw.pad = params(font, mode);
                    }
                    _ => {
                        raw.size = [0.0; 2];
                        raw.color = [0.0; 4];
                    }
                }
            }
        }
        raw
    }));
}

/// First sprite layer of the screen HUD (the `ui` pass). Sprites on
/// `HUD_LAYER` and above draw after the tone map and every post effect,
/// straight into the display-referred `ldr` target, so their colours land
/// as authored: linear 1.0 is 255, no bloom halo, no vignette, no tone
/// curve. Sprites below it are the scene (2D world art, labels that belong
/// to the world, floating scores) and keep the scene's post look. Layers
/// `HUD_LAYER..=i16::MAX` are the HUD's own order. Only `ui` games have the
/// pass; without `msdf` every sprite draws in the scene as before.
pub const HUD_LAYER: i16 = 16_384;

/// The `ui` pass's graph order: after every effect on `ldr` (the vignette
/// is 200) and before `present` (1000).
pub const UI_PASS_ORDER: i32 = 900;

/// Index of the first HUD sprite ([`HUD_LAYER`] and above) in a list sorted
/// by layer (`RenderWorld::sort_sprites`).
pub fn hud_from(sorted: &[Sprite]) -> u32 {
    sorted.partition_point(|s| s.layer < HUD_LAYER) as u32
}

/// The `ui` pass: draws the HUD sprites into `ldr` in place, after the tone
/// map (100) and the post effects, before `present`.
pub fn ui_pass() -> crate::graph::PassDesc {
    crate::graph::PassDesc::new("ui", "render").reads(&["ldr"]).writes(&["ldr"]).order(UI_PASS_ORDER)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny monospace-ish fixture: every printable ASCII glyph is 0.5 em
    /// wide with a box, space has no ink, and "AV" kerns by -0.1 em.
    pub(crate) fn fixture(skip: &[char]) -> MsdfFont {
        // Round-trip through the binary table, as the game gets it.
        MsdfFont::parse(&write_evf(&raw_fixture(skip))).unwrap()
    }

    /// The fixture as built, before the table round trip (so a table the
    /// parser must refuse can be made from it).
    fn raw_fixture(skip: &[char]) -> MsdfFont {
        let mut glyphs = Vec::new();
        for code in 32u32..=126 {
            let c = char::from_u32(code).unwrap_or('?');
            if skip.contains(&c) {
                continue;
            }
            let ink = code != 32;
            let i = glyphs.len() as f32;
            glyphs.push(Glyph {
                code,
                advance: 0.5,
                plane: if ink { [0.0, -0.75, 0.5, 0.25] } else { [0.0; 4] },
                atlas: if ink { [i * 12.0 + 0.5, 0.5, i * 12.0 + 12.5, 24.5] } else { [0.0; 4] },
            });
        }
        let idx = |c: char| glyphs.iter().position(|g| g.code == c as u32).unwrap_or(0) as u16;
        let mut kerning = vec![(idx('A'), idx('V'), -0.1), (idx('T'), idx('o'), -0.125)];
        kerning.sort_by_key(|k| (k.0, k.1));
        MsdfFont {
            pxrange: 6.0,
            em_px: 24.0,
            line_height: 1.25,
            ascender: 0.95,
            descender: 0.3,
            atlas: [2048, 32],
            glyphs,
            kerning,
            question: 0,
        }
    }

    fn world(font: MsdfFont) -> (RenderWorld, FontId) {
        let mut w = RenderWorld::default();
        let id = w.fonts.insert("test", font);
        (w, id)
    }

    fn style(id: FontId, size: f32) -> TextStyle {
        TextStyle { font: id, ..TextStyle::new(size, [1.0, 0.9, 0.8, 1.0]) }
    }

    fn line_of(l: &Line, f: &MsdfFont) -> String {
        l.glyphs.iter().filter_map(|(g, _)| f.glyph(*g)).filter_map(|g| char::from_u32(g.code)).collect()
    }

    #[test]
    fn evf_round_trips_and_rejects_bad_tables() {
        let f = fixture(&[]);
        assert_eq!(MsdfFont::parse(&write_evf(&f)), Ok(f.clone()));
        assert_eq!(f.glyphs.len(), 95);
        assert!(MsdfFont::parse(b"nope").is_err());
        let bytes = write_evf(&f);
        assert!(MsdfFont::parse(&bytes[..bytes.len() - 1]).is_err(), "cut short");
        // Native builds keep the full text; the wasm carries only the code.
        assert_eq!(MsdfFont::parse(&bytes[..2]).err(), Some("font table: cut short"));
        let no_q = raw_fixture(&['?']);
        assert!(MsdfFont::parse(&write_evf(&no_q)).unwrap_err().contains('?'));
    }

    #[test]
    fn msdf_layout_wraps_and_aligns() {
        let f = fixture(&[]);
        // 0.5 em per glyph: "aaa bbb ccc" is 5.5 em; a 4 em line holds
        // "aaa bbb" (3.5 em) and wraps "ccc", dropping the space.
        let lines = layout(&f, "aaa bbb ccc", 4.0);
        let text: Vec<String> = lines.iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["aaa bbb", "ccc"]);
        assert_eq!(lines[0].width, 3.5);
        // \n always breaks; no wrap without a width.
        let text: Vec<String> = layout(&f, "ab\ncd", 0.0).iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["ab", "cd"]);
        // A word longer than the line breaks between letters.
        let text: Vec<String> = layout(&f, "abcdefghij", 2.0).iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["abcd", "efgh", "ij"]);
        // Every wrapped line fits.
        for l in layout(&f, "the quick brown fox jumps over the lazy dog", 3.0) {
            assert!(l.width <= 3.0, "{}", line_of(&l, &f));
        }
        // A wrap inside a run of spaces drops the whole run: the next line
        // starts with its word, never with spaces. Inside a line they stay.
        let wrapped = layout(&f, "aaa  bbb", 2.0);
        let text: Vec<String> = wrapped.iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["aaa", "bbb"]);
        assert_eq!(wrapped[1].glyphs[0].1, 0.0, "line two's first glyph sits at pen 0");
        let text: Vec<String> = layout(&f, "aaa   bbb cc", 3.0).iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["aaa", "bbb cc"]);
        let text: Vec<String> = layout(&f, "a  b", 3.0).iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["a  b"]);
        // Leading spaces of a paragraph are kept; trailing ones past the edge go.
        let text: Vec<String> = layout(&f, "  ab    ", 2.0).iter().map(|l| line_of(l, &f)).collect();
        assert_eq!(text, ["  ab"]);
        // Long text lays out in one pass (a quadratic wrap takes seconds here).
        let long = "word ".repeat(20_000);
        assert_eq!(layout(&f, &long, 10.0).len(), 5_000);
        // A 2,000-character paragraph: a running width, so well under 1 ms
        // (the bound is loose for slow test machines; quadratic was ~100x).
        let para = "word ".repeat(400);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            assert_eq!(layout(&f, &para, 10.0).len(), 100);
        }
        assert!(t.elapsed().as_secs_f64() / 10.0 < 0.005, "{:?} per layout", t.elapsed() / 10);

        // Drawn: size_px 20 is the line height, so 16 px per em and 8 per glyph.
        let (mut w, id) = world(f);
        let s = TextStyle { max_width: 64.0, ..style(id, 20.0) };
        let size = draw(&mut w, [100.0, 10.0], 0, "aaa bbb ccc", &s);
        assert_eq!(size, [56.0, 40.0]);
        assert_eq!(size, measure(&w.fonts, "aaa bbb ccc", &s));
        // Line two starts a line height lower; both start at x 100.
        let lefts: Vec<(f32, f32)> = w.sprites.iter().map(|q| (q.pos[0] - q.size[0] * 0.5, q.pos[1])).collect();
        assert_eq!(lefts.len(), 9);
        assert_eq!(lefts[0].0, 100.0);
        assert_eq!(lefts[6].0, 100.0);
        assert_eq!(lefts[6].1 - lefts[0].1, 20.0);
        // Centre and right align each line on its own.
        for (align, at) in [(Align::Center, 0.5f32), (Align::Right, 1.0)] {
            let mut w2 = RenderWorld { fonts: w.fonts.clone(), ..RenderWorld::default() };
            draw(&mut w2, [100.0, 10.0], 0, "aaa bbb ccc", &TextStyle { align, ..s });
            let left = |i: usize| w2.sprites[i].pos[0] - w2.sprites[i].size[0] * 0.5;
            assert_eq!(left(0), (100.0 - 56.0 * at).round());
            assert_eq!(left(6), (100.0 - 24.0 * at).round());
        }
        // Lines start and sit on whole pixels at any position.
        let mut w3 = RenderWorld { fonts: w.fonts.clone(), ..RenderWorld::default() };
        draw(&mut w3, [10.3, 5.6], 0, "a", &style(id, 33.0));
        let q = w3.sprites[0];
        let scale = 33.0 / 1.25;
        assert_eq!((q.pos[0] - q.size[0] * 0.5).fract(), 0.0);
        let baseline = q.pos[1] - q.size[1] * 0.5 + 0.75 * scale;
        assert!((baseline - baseline.round()).abs() < 1e-3);
    }

    #[test]
    fn kerning_is_applied() {
        let f = fixture(&[]);
        let av = &layout(&f, "AV", 0.0)[0];
        let aa = &layout(&f, "AA", 0.0)[0];
        assert_eq!(aa.glyphs[1].1, 0.5);
        assert!((av.glyphs[1].1 - 0.4).abs() < 1e-3, "{:?}", av.glyphs);
        assert!((av.width - 0.9).abs() < 1e-3);
        assert!((f.kern(f.index('T'), f.index('o')) + 0.125).abs() < 1e-3);
        assert_eq!(f.kern(f.index('V'), f.index('A')), 0.0, "pairs are ordered");
        // Measured and drawn widths carry it too.
        let (mut w, id) = world(f);
        let s = style(id, 20.0);
        let a = measure(&w.fonts, "AV", &s);
        let b = measure(&w.fonts, "AA", &s);
        assert!(a[0] < b[0]);
        draw(&mut w, [0.0, 0.0], 0, "AV", &s);
        let gap = w.sprites[1].pos[0] - w.sprites[0].pos[0];
        // The table stores em in 1/4096 steps (-0.1 em is -410/4096), so
        // allow that rounding at 16 px per em.
        assert!((gap - 0.4 * 16.0).abs() < 16.0 / 4096.0 + 1e-4, "{gap}");
    }

    #[test]
    fn missing_glyph_draws_question_mark() {
        let f = fixture(&['e']);
        let q = f.index('?');
        for c in ['e', '\u{e9}', '\u{1f600}', '\u{4e2d}', '\t'] {
            assert_eq!(f.index(c), q, "{c:?}");
        }
        let (mut w, id) = world(f);
        draw(&mut w, [0.0, 0.0], 2, "h\u{e9}e!", &style(id, 20.0));
        let glyphs: Vec<u16> = w.sprites.iter().filter_map(|s| split_cell(s.cell)).map(|c| c.2).collect();
        let fnt = w.fonts.get(id).unwrap();
        assert_eq!(glyphs, [fnt.index('h'), q, q, fnt.index('!')]);
        assert!(w.sprites.iter().all(|s| s.layer == 2));
    }

    #[test]
    fn effects_stack_under_the_fill_and_pack_their_params() {
        let (mut w, id) = world(fixture(&[]));
        let s = TextStyle {
            outline: Some([0.0, 0.0, 0.0, 1.0]),
            glow: Some([1.0, 0.0, 0.0, 1.0]),
            shadow: Some([0.0, 0.0, 0.0, 0.5]),
            ..style(id, 20.0)
        };
        draw(&mut w, [0.0, 0.0], 0, "Hi", &s);
        let modes: Vec<u16> = w.sprites.iter().filter_map(|s| split_cell(s.cell)).map(|c| c.1).collect();
        assert_eq!(modes, [2, 2, 2, 2, 1, 1, 0, 0], "shadow, glow, outline, fill");
        assert_eq!(w.sprites[0].pos[0] - w.sprites[6].pos[0], 1.0, "shadow one px down-right at 16 px/em");
        assert_eq!(w.sprites[6].color, s.color);
        // Not on the GPU yet: glyphs pack as clear, empty quads.
        let mut out = Vec::new();
        pack_sprites(&w.sprites, &w.fonts, &[], 0, &mut out);
        assert_eq!(out.len(), 8);
        assert!(out.iter().all(|r| r.size == [0.0; 2] && r.color[3] == 0.0));
        // Placed: uvs from the atlas bounds, params by mode.
        assert!(!w.fonts.ready(id));
        if let Some(sl) = w.fonts.slot_mut(id) {
            sl.placement = Some([0.0, 0.5, 1.0 / 2048.0, 1.0 / 64.0]);
        }
        assert!(w.fonts.ready(id));
        pack_sprites(&w.sprites, &w.fonts, &[], 0, &mut out);
        let fill = out[6];
        let h = w.fonts.get(id).unwrap().index('H') as f32;
        assert_eq!(fill.uv, [(h * 12.0 + 0.5) / 2048.0, 0.5 + 0.5 / 64.0, (h * 12.0 + 12.5) / 2048.0, 0.5 + 24.5 / 64.0]);
        assert_eq!(fill.pad, [6.0, 0.0, 0.0]);
        assert_eq!(out[4].pad[1], 1.0);
        assert!((out[4].pad[2] - OUTLINE_EM * 24.0 / 6.0).abs() < 1e-6);
        assert!(out[0].pad[2] <= 0.49);
        // A plain sprite next to them keeps its cell uv and no params.
        let plain = Sprite { pos: [0.0; 2], size: [4.0; 2], rotation: 0.0, cell: 1, color: [1.0; 4], layer: 0 };
        pack_sprites(&[plain], &w.fonts, &[[0.1, 0.2, 0.3, 0.4]; 16], 16, &mut out);
        assert_eq!((out[0].uv, out[0].pad), ([0.1, 0.2, 0.3, 0.4], [0.0; 3]));
    }

    #[test]
    fn fonts_are_slots_by_name_and_unloaded_fonts_draw_nothing() {
        let mut fonts = Fonts::default();
        let a = fonts.request("a");
        assert_eq!(fonts.request("a"), a);
        let ids: Vec<FontId> = ["b", "c", "d"].iter().map(|n| fonts.request(n)).collect();
        assert_eq!(ids, [FontId(2), FontId(3), FontId(4)]);
        assert_eq!(fonts.request("e"), FontId::BITMAP, "a fifth font falls back to the bitmap font");
        let mut w = RenderWorld { fonts, ..RenderWorld::default() };
        let size = draw(&mut w, [0.0; 2], 0, "Hello", &style(a, 20.0));
        assert!(w.sprites.is_empty());
        assert_eq!(size, [0.0, 20.0]);
        // Cells keep slot, mode and glyph apart.
        for (slot, mode, g) in [(0, Mode::Fill, 0), (3, Mode::Soft, 1023), (2, Mode::Outline, 77)] {
            assert_eq!(split_cell(cell(slot, mode, g)), Some((slot, mode as u16, g)));
        }
        assert_eq!(split_cell(5), None, "a built-in sprite cell is not an MSDF cell");
        #[cfg(feature = "sprite-art")]
        assert_eq!(split_cell(crate::art::ART_CELL0 + 5), None);
    }

    #[test]
    fn msdf_wgsl_validates() {
        use naga::valid::{Capabilities, ValidationFlags, Validator};
        // With the feature (the MSDF variant) and without it (the art
        // variant every other game ships).
        let with = include_str!("../shaders/sprite_msdf.wgsl");
        let without = concat!(include_str!("../shaders/sprite.wgsl"), include_str!("../shaders/sprite_fs_art.wgsl"));
        // The shipped msdf source is `with` minus its comments.
        let shipped = crate::shaders::SPRITE.source;
        for (name, src) in [("msdf", with), ("msdf shipped", shipped), ("art", without)] {
            let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
            Validator::new(ValidationFlags::all(), Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
        assert!(shipped.starts_with("// evani.shader.sprite_msdf"), "the msdf build ships the msdf sprite shader");
        assert!(with.starts_with("// evani.shader.sprite_msdf") && shipped.contains("fn median3"));
    }

    #[test]
    fn solid_cell_packs_a_plain_rectangle_without_a_font() {
        let s = Sprite { pos: [10.0, 20.0], size: [30.0, 40.0], rotation: 0.0, cell: cell(0, Mode::Solid, 0), color: [0.1, 0.2, 0.3, 1.0], layer: 0 };
        let mut out = Vec::new();
        pack_sprites(&[s], &Fonts::default(), &[], 0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].size, [30.0, 40.0], "a solid rectangle draws even with no font loaded");
        assert_eq!(out[0].color, [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(out[0].pad, [1.0, 3.0, 0.0]);
        assert!(crate::shaders::SPRITE.source.contains("mode > 2.5"), "the shipped shader has the solid branch");
    }

    /// The scene/HUD split the `ui` pass draws from: every sprite below
    /// `HUD_LAYER` stays in the scene (under bloom and the tone map), every
    /// one on it or above is the HUD, after post.
    #[test]
    fn hud_layers_split_after_the_scene() {
        let at = |layer: i16| Sprite { pos: [0.0; 2], size: [1.0; 2], rotation: 0.0, cell: 1, color: [1.0; 4], layer };
        let mut w = RenderWorld::default();
        for l in [HUD_LAYER + 40, 3, HUD_LAYER, HUD_LAYER - 1, i16::MAX, 0] {
            w.draw_sprite(at(l));
        }
        w.sort_sprites();
        assert_eq!(hud_from(&w.sprites), 3);
        assert!(w.sprites[..3].iter().all(|s| s.layer < HUD_LAYER));
        assert!(w.sprites[3..].iter().all(|s| s.layer >= HUD_LAYER));
        assert_eq!(hud_from(&[]), 0);
        assert_eq!(hud_from(&w.sprites[..3]), 3, "no HUD: the ui pass draws nothing");
        let pass = ui_pass();
        assert_eq!((pass.name, pass.owner, pass.order), ("ui", "render", UI_PASS_ORDER));
    }
}
