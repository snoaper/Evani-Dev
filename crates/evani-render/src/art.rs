//! Sprite art (feature `sprite-art`): named sprite sheets from the engine,
//! the look and the kit, packed into the one sprite atlas texture with
//! gutters and a mip chain built on the CPU.
//!
//! * [`SpriteArt`] (world resource): the sheets and the name table. A kit
//!   asks for a frame by name with [`SpriteArt::cell`] or
//!   [`SpriteArt::cell_or`] (a built-in shape as the fallback), so it never
//!   depends on a specific look. The look supplies its sheet through its
//!   `sprites` generator ([`LOOK_SPRITES`], a
//!   [`GeneratedAsset::SpriteSheet`]); the render module adds it when the
//!   game starts.
//! * [`pack`]: a deterministic shelf packer. One frame list in a fixed order
//!   (engine, look, kit), each group sorted by height, width and name; every
//!   smooth frame keeps a gutter of [`ART_GUTTER`] px of its own extruded
//!   edge pixels, and every padded cell starts on a multiple of the gutter,
//!   so each texel of every mip level belongs to exactly one frame. The atlas
//!   grows through powers of two up to the device limit; past it, an
//!   [`AtlasFull`] error, never a panic.
//! * [`build_levels`]: the mip chain, per frame: a 2x2 box filter on
//!   alpha-weighted (premultiplied) linear colour, sRGB through the gamma 2.0
//!   shortcut (square to decode, square root to encode). The atlas stays
//!   straight alpha, so the sprite blend is unchanged.
//! * Pixel-art sheets keep one level: no gutter, no filtering, and the
//!   sprite shader samples them at level 0 on texel centres (their uv
//!   rectangle is moved by [`PIXEL_U`] to mark them).

use std::collections::BTreeMap;

use evani_core::plugin::{GenRequest, GeneratedAsset, LookData};

use crate::shaders::{ATLAS_CELLS, SpriteRaw};
use crate::world::Sprite;

/// First sprite cell of the named frames: frame `i` of [`SpriteArt`] (in
/// the order sheets were added) is cell `ART_CELL0 + i`.
pub const ART_CELL0: u16 = 0x8000;
/// Mip levels of an atlas with smooth art (level 0 plus three halvings:
/// sprites drawn down to an eighth of their art size stay smooth).
pub const ART_LEVELS: u32 = 4;
/// Gutter around every smooth frame, `2^(ART_LEVELS - 1)` px.
pub const ART_GUTTER: u32 = 1 << (ART_LEVELS - 1);
/// Name of the look generator that supplies sprite art.
pub const LOOK_SPRITES: &str = "sprites";
/// Frame edge (px) the render module asks the look's generator for.
pub const LOOK_SPRITES_SIZE: u32 = 128;
/// Added to the left `u` of a pixel-art frame: the shader reads it as
/// "sample level 0 on texel centres".
pub const PIXEL_U: f32 = 2.0;
/// Smallest atlas edge tried first.
const ATLAS_MIN: u32 = 256;

/// Who supplied a sheet. Packing follows this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtSource {
    Engine,
    Look,
    Kit,
}

/// A sheet of named frames. RGBA8 rows top to bottom, sRGB-encoded colour,
/// straight alpha.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtSheet {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    /// `(name, [x, y, w, h])` inside the sheet.
    pub frames: Vec<(String, [u32; 4])>,
    /// Crisp frames: one mip level, no gutter, sampled on texel centres.
    pub pixel_art: bool,
}

impl ArtSheet {
    /// The pixel buffer matches the size and every frame lies inside.
    pub fn is_valid(&self) -> bool {
        self.rgba8.len() == (self.width as usize) * (self.height as usize) * 4
            && self.frames.iter().all(|(_, f)| {
                f[2] > 0 && f[3] > 0 && f[0].saturating_add(f[2]) <= self.width && f[1].saturating_add(f[3]) <= self.height
            })
    }
}

/// The named sprite art of a game (world resource of the render module).
#[derive(Debug, Clone, Default)]
pub struct SpriteArt {
    sheets: Vec<(ArtSource, ArtSheet)>,
    /// `(name, cell)` sorted by name. On a duplicate name the earlier
    /// source wins (engine, then look, then kit), then the sheet added first.
    names: Vec<(String, u16)>,
    frames: usize,
    revision: u64,
}

impl SpriteArt {
    /// Add a sheet. Its frames get the next cells; earlier cells never move.
    pub fn add(&mut self, source: ArtSource, sheet: ArtSheet) -> Result<(), String> {
        if !sheet.is_valid() {
            return Err("sprite art: a sheet's pixels don't match its size or a frame lies outside it".into());
        }
        if self.frames + sheet.frames.len() > usize::from(u16::MAX - ART_CELL0) + 1 {
            return Err("sprite art: more than 32768 named frames".into());
        }
        self.frames += sheet.frames.len();
        self.sheets.push((source, sheet));
        let mut all = Vec::with_capacity(self.frames);
        let mut cell = ART_CELL0;
        for (src, sh) in &self.sheets {
            for (name, _) in &sh.frames {
                all.push((name.as_str(), *src, cell));
                cell = cell.wrapping_add(1);
            }
        }
        // Stable: within a name and source, the sheet added first stays first.
        all.sort_by(|a, b| a.0.cmp(b.0).then(a.1.cmp(&b.1)));
        all.dedup_by(|b, a| a.0 == b.0);
        self.names = all.into_iter().map(|(n, _, c)| (n.to_string(), c)).collect();
        self.revision += 1;
        Ok(())
    }

    /// Add the look's sheet from its `sprites` generator. `Ok(false)`: the
    /// look supplies none.
    pub fn add_look(&mut self, look: &LookData) -> Result<bool, String> {
        let req = GenRequest {
            name: LOOK_SPRITES.into(),
            size: LOOK_SPRITES_SIZE,
            seed: 1,
            params: BTreeMap::new(),
        };
        match look.generate(&req) {
            None => Ok(false),
            Some(GeneratedAsset::SpriteSheet {
                width,
                height,
                rgba8,
                frames,
                pixel_art,
            }) => self
                .add(
                    ArtSource::Look,
                    ArtSheet {
                        width,
                        height,
                        rgba8,
                        frames,
                        pixel_art,
                    },
                )
                .map(|()| true),
            Some(_) => Err(format!("sprite art: the look's `{LOOK_SPRITES}` generator must return a SpriteSheet")),
        }
    }

    /// The sprite cell of frame `name`, if any sheet has it.
    pub fn cell(&self, name: &str) -> Option<u16> {
        let i = self.names.binary_search_by(|(n, _)| n.as_str().cmp(name)).ok()?;
        self.names.get(i).map(|(_, c)| *c)
    }

    /// [`SpriteArt::cell`], or `fallback` (a built-in shape such as
    /// [`crate::atlas::cell::CIRCLE`]) when no sheet has the frame.
    pub fn cell_or(&self, name: &str, fallback: u16) -> u16 {
        self.cell(name).unwrap_or(fallback)
    }

    /// The edge of sprite cell `cell` (a named frame or a built-in shape),
    /// for kits that draw an ink outline as a bigger copy behind a sprite:
    /// how much of that outline the sprite's anti-aliased rim eats depends
    /// on whose art it is ([`SpriteEdge::px`]).
    pub fn edge(&self, cell: u16) -> SpriteEdge {
        let Some(i) = cell.checked_sub(ART_CELL0) else { return SpriteEdge::Builtin };
        self.frames().nth(usize::from(i)).map_or(SpriteEdge::Builtin, |f| SpriteEdge::Frame {
            width: f.rect[2],
            pixel_art: f.pixel_art,
        })
    }

    /// Bumped by every [`SpriteArt::add`]; the renderer rebuilds the atlas.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Every named frame in cell order.
    pub fn frames(&self) -> impl Iterator<Item = FrameRef<'_>> + '_ {
        self.sheets.iter().flat_map(|(source, sh)| {
            sh.frames.iter().map(move |(name, rect)| FrameRef {
                name,
                source: *source,
                sheet: &sh.rgba8,
                sheet_w: sh.width,
                rect: *rect,
                pixel_art: sh.pixel_art,
            })
        })
    }
}

/// The anti-aliased edge of a sprite cell ([`SpriteArt::edge`]). A kit that
/// draws an ink outline `o` px wide as a copy of the sprite grown behind it
/// grows it by `o + edge.px(drawn)` per side: the soft rim of the sprite
/// (and of the copy) eats that much of the visible ink. Built-in shapes are
/// soft (1.5 of 64 texels, magnified), look and kit frames crisp, so using
/// one allowance for both draws the ink a pixel too thick on crisp art or a
/// pixel too thin on the built-in shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpriteEdge {
    /// A built-in atlas shape ([`crate::atlas::cell`]).
    Builtin,
    /// A named frame `width` texels wide.
    Frame { width: u32, pixel_art: bool },
}

impl SpriteEdge {
    /// What a built-in shape's soft rim eats, in screen px, at the sizes
    /// kits draw pieces (about 64 to 110 px); the value the match-3 ink was
    /// tuned to.
    pub const BUILTIN_PX: f32 = 1.0;

    /// Screen px of an outline the rim eats when the sprite is drawn
    /// `drawn_px` wide: [`Self::BUILTIN_PX`] for a built-in shape, half a
    /// texel for a smooth frame (anti-aliased over about one texel), none
    /// for pixel art.
    pub fn px(self, drawn_px: f32) -> f32 {
        match self {
            Self::Builtin => Self::BUILTIN_PX,
            Self::Frame { pixel_art: true, .. } => 0.0,
            Self::Frame { width, .. } => 0.5 * drawn_px.max(0.0) / width.max(1) as f32,
        }
    }
}

/// One frame to pack: its sheet's pixels and its rectangle in the sheet.
#[derive(Debug, Clone, Copy)]
pub struct FrameRef<'a> {
    pub name: &'a str,
    pub source: ArtSource,
    pub sheet: &'a [u8],
    pub sheet_w: u32,
    pub rect: [u32; 4],
    pub pixel_art: bool,
}

/// The frames don't fit in a `max` x `max` atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasFull {
    pub max: u32,
}

impl core::fmt::Display for AtlasFull {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "sprite art doesn't fit in a {0}x{0} atlas", self.max)
    }
}

/// Where [`pack`] put the frames.
#[derive(Debug, Clone, PartialEq)]
pub struct Packed {
    /// Atlas size at level 0.
    pub size: [u32; 2],
    pub levels: u32,
    pub gutter: u32,
    /// Each frame's level-0 rectangle `[x, y, w, h]` in the atlas, in input
    /// order.
    pub rects: Vec<[u32; 4]>,
}

fn round_up(v: u32, to: u32) -> u32 {
    v.div_ceil(to) * to
}

/// Padded cell size of a frame and its gutter.
fn cell_dims(f: &FrameRef, gutter: u32) -> (u32, u32, u32) {
    let g = if f.pixel_art { 0 } else { gutter };
    let align = gutter.max(1);
    (round_up(f.rect[2] + 2 * g, align), round_up(f.rect[3] + 2 * g, align), g)
}

/// Pack `frames` (fixed order: engine, look, kit) into the smallest atlas
/// that holds them, growing through powers of two up to `max_dim`.
pub fn pack(frames: &[FrameRef], max_dim: u32) -> Result<Packed, AtlasFull> {
    let smooth = frames.iter().any(|f| !f.pixel_art);
    let (levels, gutter) = if smooth { (ART_LEVELS, ART_GUTTER) } else { (1, 0) };
    let dims: Vec<(u32, u32, u32)> = frames.iter().map(|f| cell_dims(f, gutter)).collect();
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by(|&a, &b| {
        frames[a]
            .source
            .cmp(&frames[b].source)
            .then(dims[b].1.cmp(&dims[a].1))
            .then(dims[b].0.cmp(&dims[a].0))
            .then(frames[a].name.cmp(frames[b].name))
    });
    let max = max_dim.max(1);
    let mut size = [ATLAS_MIN.min(max); 2];
    loop {
        if let Some(at) = shelf(&order, &dims, size) {
            let rects = at
                .iter()
                .zip(frames)
                .zip(&dims)
                .map(|((p, f), d)| [p[0] + d.2, p[1] + d.2, f.rect[2], f.rect[3]])
                .collect();
            return Ok(Packed {
                size,
                levels,
                gutter,
                rects,
            });
        }
        if size[0] >= max && size[1] >= max {
            return Err(AtlasFull { max });
        }
        let i = usize::from(size[0] > size[1] || size[0] >= max);
        size[i] = (size[i] * 2).min(max);
    }
}

/// Shelf placement of padded cells in `order`; the cell origin of each
/// frame (input order), or `None` when they don't fit in `size`.
fn shelf(order: &[usize], dims: &[(u32, u32, u32)], size: [u32; 2]) -> Option<Vec<[u32; 2]>> {
    let mut at = vec![[0u32; 2]; dims.len()];
    // (y, height, x cursor)
    let mut shelves: Vec<[u32; 3]> = Vec::new();
    for &i in order {
        let (w, h, _) = dims[i];
        if w > size[0] {
            return None;
        }
        if let Some(s) = shelves.iter_mut().find(|s| h <= s[1] && s[2] + w <= size[0]) {
            at[i] = [s[2], s[0]];
            s[2] += w;
        } else {
            let y = shelves.last().map_or(0, |s| s[0] + s[1]);
            if y + h > size[1] {
                return None;
            }
            shelves.push([y, h, w]);
            at[i] = [0, y];
        }
    }
    Some(at)
}

/// Every mip level of the packed atlas (`packed.levels` RGBA8 buffers, level
/// `k` is `size >> k`), built per frame from its extruded padded cell.
pub fn build_levels(frames: &[FrameRef], packed: &Packed) -> Vec<Vec<u8>> {
    let mut levels: Vec<Vec<u8>> = (0..packed.levels)
        .map(|k| vec![0u8; ((packed.size[0] >> k) * (packed.size[1] >> k) * 4) as usize])
        .collect();
    for (f, r) in frames.iter().zip(&packed.rects) {
        let (cw, ch, g) = cell_dims(f, packed.gutter);
        let (ox, oy) = (r[0] - g, r[1] - g);
        // Level 0: the frame with its edge pixels extruded into the gutter.
        let mut cell = vec![0u8; (cw * ch * 4) as usize];
        for y in 0..ch {
            let sy = f.rect[1] + (y.saturating_sub(g)).min(f.rect[3] - 1);
            for x in 0..cw {
                let sx = f.rect[0] + (x.saturating_sub(g)).min(f.rect[2] - 1);
                let s = ((sy * f.sheet_w + sx) * 4) as usize;
                let o = ((y * cw + x) * 4) as usize;
                if let (Some(d), Some(p)) = (cell.get_mut(o..o + 4), f.sheet.get(s..s + 4)) {
                    d.copy_from_slice(p);
                }
            }
        }
        if !f.pixel_art {
            dilate(&mut cell, cw, ch);
        }
        let (mut w, mut h) = (cw, ch);
        for (k, level) in levels.iter_mut().enumerate() {
            if k > 0 {
                (cell, w, h) = downsample(&cell, w, h);
            }
            blit(level, packed.size[0] >> k, &cell, w, h, [ox >> k, oy >> k]);
        }
    }
    levels
}

fn blit(dst: &mut [u8], dst_w: u32, src: &[u8], w: u32, h: u32, at: [u32; 2]) {
    let row = (w * 4) as usize;
    for y in 0..h {
        let d = (((at[1] + y) * dst_w + at[0]) * 4) as usize;
        let s = (y * w * 4) as usize;
        if let (Some(d), Some(s)) = (dst.get_mut(d..d + row), src.get(s..s + row)) {
            d.copy_from_slice(s);
        }
    }
}

/// Give every fully transparent texel the alpha-weighted colour of its
/// covered neighbours, so bilinear filtering at an edge never pulls in the
/// colour of invisible pixels.
fn dilate(px: &mut [u8], w: u32, h: u32) {
    let src = px.to_vec();
    let at = |x: u32, y: u32| ((y * w + x) * 4) as usize;
    for y in 0..h {
        for x in 0..w {
            let o = at(x, y);
            if src[o + 3] != 0 {
                continue;
            }
            let mut acc = [0u32; 4];
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let n = at(nx, ny);
                    let a = u32::from(src[n + 3]);
                    for c in 0..3 {
                        acc[c] += u32::from(src[n + c]) * a;
                    }
                    acc[3] += a;
                }
            }
            for c in 0..3 {
                if let Some(v) = acc[c].checked_div(acc[3]) {
                    px[o + c] = v as u8;
                }
            }
        }
    }
}

/// One mip step: each output texel is the 2x2 box of the level above
/// (clamped at the edge for odd sizes), averaged in linear light (gamma 2.0)
/// weighted by alpha. Returns the pixels and their size.
pub fn downsample(src: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (w2, h2) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
    let mut out = vec![0u8; (w2 * h2 * 4) as usize];
    for y in 0..h2 {
        for x in 0..w2 {
            let mut acc = [0f32; 4];
            let mut plain = [0f32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (2 * x + dx).min(w.saturating_sub(1));
                let sy = (2 * y + dy).min(h.saturating_sub(1));
                let s = ((sy * w + sx) * 4) as usize;
                let Some(p) = src.get(s..s + 4) else { continue };
                let a = f32::from(p[3]) / 255.0;
                for c in 0..3 {
                    let v = f32::from(p[c]) / 255.0;
                    acc[c] += v * v * a;
                    plain[c] += v * v;
                }
                acc[3] += a;
            }
            let o = ((y * w2 + x) * 4) as usize;
            for c in 0..3 {
                let lin = if acc[3] > 0.0 { acc[c] / acc[3] } else { plain[c] / 4.0 };
                out[o + c] = (lin.sqrt() * 255.0 + 0.5) as u8;
            }
            out[o + 3] = (acc[3] / 4.0 * 255.0 + 0.5) as u8;
        }
    }
    (out, w2, h2)
}

/// The uv rectangle of every packed frame (input order) in `out` (cleared
/// first): a half-texel inset at level 0 for smooth frames, exact edges for
/// pixel-art frames (moved by [`PIXEL_U`]) and for everything when
/// `nearest` (`render.pixel_art`).
pub fn uv_table(packed: &Packed, pixel: &[bool], nearest: bool, out: &mut Vec<[f32; 4]>) {
    let (tw, th) = (packed.size[0].max(1) as f32, packed.size[1].max(1) as f32);
    out.clear();
    for (r, &px) in packed.rects.iter().zip(pixel) {
        let inset = if nearest || px { 0.0 } else { 0.5 };
        let du = if px { PIXEL_U } else { 0.0 };
        let (x, y, w, h) = (r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32);
        out.push([(x + inset) / tw + du, (y + inset) / th, (x + w - inset) / tw + du, (y + h - inset) / th]);
    }
}

/// The uv rectangle of atlas cell `cell` in an atlas with named frames:
/// cells from [`ART_CELL0`] read the uv table from `art_base`, and a cell
/// past the table wraps onto the built-in cells. Sprites and the shared
/// particles both look their cells up here.
pub fn cell_uv(cell: u16, uvs: &[[f32; 4]], art_base: usize) -> [f32; 4] {
    let i = match cell.checked_sub(ART_CELL0) {
        Some(k) => art_base + usize::from(k),
        None => usize::from(cell),
    };
    uvs.get(i)
        .or_else(|| uvs.get(usize::from(cell) % ATLAS_CELLS))
        .copied()
        .unwrap_or([0.0; 4])
}

/// [`crate::shaders::pack_sprites_uv`] for an atlas with named frames:
/// cells from [`ART_CELL0`] read the uv table from `art_base`. A cell past
/// the table (a named frame the atlas had no room for) wraps onto the
/// built-in cells, as in [`crate::shaders::sprite_raw`].
pub fn pack_sprites(src: &[Sprite], uvs: &[[f32; 4]], art_base: usize, out: &mut Vec<SpriteRaw>) {
    out.clear();
    out.extend(src.iter().map(|s| {
        let uv = cell_uv(s.cell, uvs, art_base);
        SpriteRaw {
            pos: s.pos,
            size: s.size,
            uv,
            color: s.color,
            rotation: s.rotation,
            pad: [0.0; 3],
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::cell;
    use evani_core::rng::Pcg32;

    /// Built-in cells read their own row, named frames read from
    /// `art_base`, and a named frame past the table wraps onto the
    /// built-in cells (sprites and particles share this lookup).
    #[test]
    fn cell_uv_maps_built_in_and_named_cells() {
        let uvs: Vec<[f32; 4]> = (0..ATLAS_CELLS + 2).map(|i| [i as f32; 4]).collect();
        let base = ATLAS_CELLS;
        assert_eq!(cell_uv(3, &uvs, base), [3.0; 4]);
        assert_eq!(cell_uv(ART_CELL0, &uvs, base), [base as f32; 4]);
        assert_eq!(cell_uv(ART_CELL0 + 1, &uvs, base), [(base + 1) as f32; 4]);
        let far = ART_CELL0 + 50;
        assert_eq!(cell_uv(far, &uvs, base), [(usize::from(far) % ATLAS_CELLS) as f32; 4]);
    }

    /// A sheet of solid frames, each with a different colour and a darker
    /// one-pixel border, so extrusion and bleed are visible.
    fn sheet(sizes: &[(u32, u32)], names: &[&str], pixel_art: bool) -> ArtSheet {
        let width = sizes.iter().map(|s| s.0).sum::<u32>().max(1);
        let height = sizes.iter().map(|s| s.1).max().unwrap_or(1);
        let mut rgba8 = vec![0u8; (width * height * 4) as usize];
        let mut frames = Vec::new();
        let mut x0 = 0;
        for (i, (&(w, h), name)) in sizes.iter().zip(names).enumerate() {
            for y in 0..h {
                for x in 0..w {
                    let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                    let v = (40 + i as u32 * 50).min(255) as u8;
                    let o = ((y * width + x0 + x) * 4) as usize;
                    rgba8[o..o + 4].copy_from_slice(&if edge { [v / 2, 10, 255 - v, 255] } else { [v, 200, 255 - v, 255] });
                }
            }
            frames.push((name.to_string(), [x0, 0, w, h]));
            x0 += w;
        }
        ArtSheet {
            width,
            height,
            rgba8,
            frames,
            pixel_art,
        }
    }

    fn refs(sheets: &[(ArtSource, ArtSheet)]) -> Vec<FrameRef<'_>> {
        sheets
            .iter()
            .flat_map(|(source, sh)| {
                sh.frames.iter().map(move |(name, rect)| FrameRef {
                    name,
                    source: *source,
                    sheet: &sh.rgba8,
                    sheet_w: sh.width,
                    rect: *rect,
                    pixel_art: sh.pixel_art,
                })
            })
            .collect()
    }

    fn px(level: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * w + x) * 4) as usize;
        [level[o], level[o + 1], level[o + 2], level[o + 3]]
    }

    #[test]
    fn packer_is_deterministic_and_keeps_gutters() {
        let engine = sheet(&[(64, 64); 4], &["e0", "e1", "e2", "e3"], false);
        let look = sheet(&[(128, 128), (40, 30), (17, 9), (128, 96)], &["piece.heart", "b", "c", "piece.wrapped"], false);
        let kit = sheet(&[(33, 70), (5, 5)], &["k1", "k2"], false);
        let sheets = vec![(ArtSource::Engine, engine.clone()), (ArtSource::Look, look), (ArtSource::Kit, kit)];
        let frames = refs(&sheets);
        let a = pack(&frames, 8192).unwrap();
        assert_eq!(a, pack(&frames, 8192).unwrap(), "same frames, same atlas");
        let levels = build_levels(&frames, &a);
        assert_eq!(levels, build_levels(&frames, &a));
        assert_eq!((a.levels, a.gutter), (ART_LEVELS, ART_GUTTER));
        assert!(a.size.iter().all(|s| s.is_power_of_two() && *s >= 256), "{:?}", a.size);
        let g = a.gutter;
        for (i, (f, r)) in frames.iter().zip(&a.rects).enumerate() {
            // Padded cells start on the gutter grid, lie inside, never overlap.
            assert_eq!(((r[0] - g) % g, (r[1] - g) % g), (0, 0), "frame {i} aligned");
            assert!(r[0] + r[2] + g <= a.size[0] && r[1] + r[3] + g <= a.size[1]);
            for (j, q) in a.rects.iter().enumerate().skip(i + 1) {
                let apart = r[0] + r[2] + 2 * g <= q[0] || q[0] + q[2] + 2 * g <= r[0] || r[1] + r[3] + 2 * g <= q[1] || q[1] + q[3] + 2 * g <= r[1];
                assert!(apart, "frames {i} and {j} keep a full gutter each");
            }
            // The frame is copied exactly; the gutter repeats its edge pixels.
            let src = |x: u32, y: u32| px(f.sheet, f.sheet_w, f.rect[0] + x, f.rect[1] + y);
            let w0 = a.size[0];
            for y in 0..r[3] {
                for x in 0..r[2] {
                    assert_eq!(px(&levels[0], w0, r[0] + x, r[1] + y), src(x, y));
                }
            }
            for d in 1..=g {
                assert_eq!(px(&levels[0], w0, r[0] - d, r[1] + 1), src(0, 1), "left gutter of {i}");
                assert_eq!(px(&levels[0], w0, r[0] + r[2] - 1 + d, r[1]), src(r[2] - 1, 0), "right gutter of {i}");
                assert_eq!(px(&levels[0], w0, r[0], r[1] - d), src(0, 0), "top gutter of {i}");
                assert_eq!(px(&levels[0], w0, r[0] - d, r[1] + r[3] - 1 + d), src(0, r[3] - 1), "corner of {i}");
            }
        }
        // A kit can't move the engine's cells while the atlas size holds.
        let more = vec![sheets[0].clone(), sheets[1].clone(), (ArtSource::Kit, sheet(&[(120, 120)], &["big"], false))];
        let b = pack(&refs(&more), 8192).unwrap();
        assert_eq!(b.size, a.size);
        assert_eq!(b.rects[..8], a.rects[..8]);
        // Too big for the device: an error, never a panic; it grows otherwise.
        assert_eq!(pack(&frames, 128), Err(AtlasFull { max: 128 }));
        let many: Vec<(ArtSource, ArtSheet)> = (0..40).map(|_| (ArtSource::Kit, engine.clone())).collect();
        let grown = pack(&refs(&many), 8192).unwrap();
        assert!(grown.size[0] * grown.size[1] > 256 * 256 && grown.size.iter().all(|s| s.is_power_of_two()));
    }

    #[test]
    fn mips_are_srgb_box_filtered() {
        // Black and white average to mid grey in linear light: sqrt(0.5).
        let bw = [0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255];
        assert_eq!(downsample(&bw, 2, 2), (vec![180, 180, 180, 255], 1, 1));
        // Alpha-weighted: a clear red texel adds no red.
        let clear = [255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255];
        assert_eq!(downsample(&clear, 2, 2).0, vec![255, 255, 255, 191]);
        // Odd sizes clamp at the edge.
        let odd = [0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        assert_eq!(downsample(&odd, 3, 1), (vec![0, 0, 0, 255, 255, 255, 255, 255], 2, 1));
        // Every level of a frame's cell is its own colour: no bleed from the
        // neighbour, even at the coarsest level.
        let mut s = sheet(&[(30, 30), (30, 30)], &["red", "blue"], false);
        s.rgba8.chunks_mut(4).enumerate().for_each(|(i, p)| {
            let c: [u8; 4] = if i % 60 < 30 { [255, 0, 0, 255] } else { [0, 0, 255, 255] };
            p.copy_from_slice(&c);
        });
        let sheets = [(ArtSource::Kit, s)];
        let frames = refs(&sheets);
        let a = pack(&frames, 8192).unwrap();
        let levels = build_levels(&frames, &a);
        assert_eq!(levels.len(), ART_LEVELS as usize);
        for (k, level) in levels.iter().enumerate() {
            let w = a.size[0] >> k;
            assert_eq!(level.len(), (w * (a.size[1] >> k) * 4) as usize);
            for (r, want) in a.rects.iter().zip([[255, 0, 0, 255], [0, 0, 255, 255]]) {
                let (x0, y0) = ((r[0] - a.gutter) >> k, (r[1] - a.gutter) >> k);
                let (x1, y1) = ((r[0] + r[2] + a.gutter) >> k, (r[1] + r[3] + a.gutter) >> k);
                for y in y0..y1 {
                    for x in x0..x1 {
                        assert_eq!(px(level, w, x, y), want, "level {k} at {x},{y}");
                    }
                }
            }
        }
        // A soft edge keeps its colour: clear texels next to it take it on.
        let mut soft = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            soft[(y * 4 + 1) * 4..(y * 4 + 2) * 4].copy_from_slice(&[200, 40, 40, 255]);
        }
        dilate(&mut soft, 4, 4);
        assert_eq!(px(&soft, 4, 0, 2), [200, 40, 40, 0]);
        assert_eq!(px(&soft, 4, 3, 2), [0, 0, 0, 0], "two texels away stays as it was");
    }

    fn heart_look(_: &GenRequest, _: &mut Pcg32) -> GeneratedAsset {
        let s = sheet(&[(32, 32), (32, 32)], &["piece.heart", "piece.star"], false);
        GeneratedAsset::SpriteSheet {
            width: s.width,
            height: s.height,
            rgba8: s.rgba8,
            frames: s.frames,
            pixel_art: false,
        }
    }

    fn texture_look(_: &GenRequest, _: &mut Pcg32) -> GeneratedAsset {
        GeneratedAsset::Texture {
            width: 1,
            height: 1,
            rgba8: vec![0; 4],
        }
    }

    #[test]
    fn the_edge_allowance_follows_whose_art_a_cell_is() {
        let mut art = SpriteArt::default();
        art.add(ArtSource::Look, sheet(&[(128, 128), (64, 64)], &["piece.smooth", "piece.big"], false)).unwrap();
        art.add(ArtSource::Kit, sheet(&[(16, 16)], &["hero.pixel"], true)).unwrap();
        // Built-in shapes keep the allowance the outlines were tuned to.
        assert_eq!(art.edge(cell::CIRCLE), SpriteEdge::Builtin);
        assert_eq!(art.edge(cell::STAR).px(107.0), SpriteEdge::BUILTIN_PX);
        // A smooth 128 px frame drawn 107 px wide: half a texel, under 1 px.
        let smooth = art.edge(art.cell_or("piece.smooth", cell::CIRCLE));
        assert_eq!(smooth, SpriteEdge::Frame { width: 128, pixel_art: false });
        assert!((smooth.px(107.0) - 0.418).abs() < 0.001, "{}", smooth.px(107.0));
        assert!(smooth.px(107.0) < SpriteEdge::BUILTIN_PX);
        assert_eq!(art.edge(art.cell_or("piece.big", cell::CIRCLE)).px(128.0), 1.0, "magnified 2x: a whole px");
        // Pixel art is hard-edged; an unknown art cell counts as built-in.
        assert_eq!(art.edge(art.cell_or("hero.pixel", cell::CIRCLE)).px(64.0), 0.0);
        assert_eq!(art.edge(ART_CELL0 + 9), SpriteEdge::Builtin);
        assert_eq!(SpriteEdge::Frame { width: 0, pixel_art: false }.px(-5.0), 0.0, "no division by zero, no negative");
    }

    #[test]
    fn named_frame_falls_back_to_a_builtin_shape() {
        let mut art = SpriteArt::default();
        assert_eq!(art.cell("piece.heart"), None);
        assert_eq!(art.cell_or("piece.heart", cell::CIRCLE), cell::CIRCLE);
        // No `sprites` generator: nothing added, still the fallback.
        assert_eq!(art.add_look(&LookData::default()), Ok(false));
        // The kit's own sheet first, then the look's: the kit's cells stay,
        // and the look's frame wins a shared name.
        art.add(ArtSource::Kit, sheet(&[(8, 8), (8, 8)], &["piece.heart", "kit.only"], false)).unwrap();
        assert_eq!(art.cell("piece.heart"), Some(ART_CELL0));
        let look = LookData {
            generators: vec![(LOOK_SPRITES.into(), heart_look)],
            ..LookData::default()
        };
        assert_eq!(art.add_look(&look), Ok(true));
        assert_eq!(art.cell("kit.only"), Some(ART_CELL0 + 1));
        assert_eq!(art.cell_or("piece.heart", cell::CIRCLE), ART_CELL0 + 2);
        assert_eq!(art.cell("piece.star"), Some(ART_CELL0 + 3));
        assert_eq!(art.cell_or("piece.wrapped", cell::SQUARE), cell::SQUARE);
        assert_eq!(art.revision(), 2);
        assert_eq!(art.frames().count(), 4);
        assert_eq!(art.frames().nth(2).map(|f| (f.name, f.source)), Some(("piece.heart", ArtSource::Look)));
        // Bad input is an error to log, not a crash.
        let bad = ArtSheet {
            width: 4,
            height: 4,
            rgba8: vec![0; 64],
            frames: vec![("x".into(), [2, 2, 4, 4])],
            pixel_art: false,
        };
        assert!(art.add(ArtSource::Kit, bad).is_err());
        let wrong = LookData {
            generators: vec![(LOOK_SPRITES.into(), texture_look)],
            ..LookData::default()
        };
        assert!(art.add_look(&wrong).is_err());
        assert_eq!(art.revision(), 2);
        // Packing sprites: a named cell reads the art part of the uv table;
        // a built-in cell, or a named cell past the table, a built-in entry.
        let uvs: Vec<[f32; 4]> = (0..20).map(|i| [i as f32; 4]).collect();
        let sprite = |c: u16| Sprite {
            pos: [0.0; 2],
            size: [1.0; 2],
            rotation: 0.0,
            cell: c,
            color: [1.0; 4],
            layer: 0,
        };
        let mut out = Vec::new();
        pack_sprites(&[sprite(ART_CELL0 + 1), sprite(cell::STAR), sprite(ART_CELL0 + 9)], &uvs, 17, &mut out);
        let got: Vec<f32> = out.iter().map(|r| r.uv[0]).collect();
        assert_eq!(got, [18.0, 6.0, 9.0], "past the table: wraps onto the built-in cells");
    }

    #[test]
    fn pixel_art_sheets_keep_one_level() {
        let mut p = sheet(&[(16, 16), (7, 5)], &["hero", "coin"], true);
        p.rgba8[3] = 0; // a clear texel keeps its colour bytes as drawn
        let only = [(ArtSource::Kit, p.clone())];
        let frames = refs(&only);
        let a = pack(&frames, 8192).unwrap();
        assert_eq!((a.levels, a.gutter), (1, 0));
        let levels = build_levels(&frames, &a);
        assert_eq!(levels.len(), 1);
        for (f, r) in frames.iter().zip(&a.rects) {
            for y in 0..r[3] {
                for x in 0..r[2] {
                    assert_eq!(px(&levels[0], a.size[0], r[0] + x, r[1] + y), px(f.sheet, f.sheet_w, f.rect[0] + x, f.rect[1] + y));
                }
            }
        }
        // Exact edges, marked for level-0 texel-centre sampling.
        let mut uvs = Vec::new();
        uv_table(&a, &[true, true], false, &mut uvs);
        let r = a.rects[0];
        let (tw, th) = (a.size[0] as f32, a.size[1] as f32);
        assert_eq!(uvs[0], [r[0] as f32 / tw + PIXEL_U, r[1] as f32 / th, (r[0] + 16) as f32 / tw + PIXEL_U, (r[1] + 16) as f32 / th]);
        // Next to smooth art the atlas has mips, but pixel frames still get
        // no gutter, keep their level-0 texels, and smooth ones are inset.
        let mixed = [(ArtSource::Look, sheet(&[(64, 64)], &["soft"], false)), (ArtSource::Kit, p)];
        let frames = refs(&mixed);
        let m = pack(&frames, 8192).unwrap();
        assert_eq!(m.levels, ART_LEVELS);
        let levels = build_levels(&frames, &m);
        let hero = m.rects[1];
        assert_eq!(px(&levels[0], m.size[0], hero[0], hero[1]), px(frames[1].sheet, frames[1].sheet_w, 0, 0));
        assert_eq!(hero[0] % ART_GUTTER, 0, "no gutter before a pixel frame");
        uv_table(&m, &[false, true], false, &mut uvs);
        assert!(uvs[0][0] < 1.0 && uvs[1][0] >= PIXEL_U);
        assert_eq!(uvs[0][0], (m.rects[0][0] as f32 + 0.5) / m.size[0] as f32);
    }
}
