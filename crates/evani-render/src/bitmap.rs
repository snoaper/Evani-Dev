//! Pixel text (feature `text`, which turns on `sprite-art`): one small
//! original bitmap font, baked at startup into an engine sheet of the
//! [`SpriteArt`](crate::art::SpriteArt) resource, and a draw call that puts
//! it on screen as sprites. This is the pixel half of the text split: it is
//! independent of the sharp (MSDF) text of the `ui` module (feature `msdf`,
//! `msdf.rs`); the shared style and font types live in [`crate::text`].
//!
//! * The font: ASCII 32 to 126, 5 x 9 px cells ([`GLYPHS`], drawn as text
//!   art like the platformer's digits). Two sizes are baked: the small one
//!   as drawn (10 px line height) and a large one (20 px) made by Scale2x
//!   smoothing of the same bitmap, so most text needs no scaling. Other
//!   sizes scale the nearer of the two.
//! * Per glyph and size two white frames are baked: `glyph.<code>` (the fill)
//!   and `glyph.<code>.outline` (the fill grown by the outline width, used as
//!   the outline and as the drop shadow when there is an outline); the large
//!   size adds `.x2`: `glyph.<code>.x2`, `glyph.<code>.x2.outline`. A draw
//!   tints them with the style's colours, so one baked set serves every
//!   colour. Frames are pixel art (crisp, no gutter, one mip level).
//! * [`draw`] pushes sprites into [`RenderWorld`]: all shadows, then all
//!   outlines, then all fills, on the one layer (the sprite sort is stable),
//!   so neighbouring glyphs never paint over each other's fill. It returns
//!   the size; [`measure`] returns the same size without drawing.
//! * Layout is whole-pixel and deterministic: the advance of a glyph is its
//!   design advance times the scale, rounded to a whole pixel (at least 1),
//!   and the start of the line is rounded too. Any character outside ASCII
//!   32 to 126 draws as `?`. There are no line breaks, wrapping or widgets
//!   here: Stage 3's text and UI layer grows from this API.
//!
//! The engine sheet is added first (in the render module's `build`), so the
//! cell of every glyph is a constant: [`ART_CELL0`] plus its frame index.

use crate::art::{ART_CELL0, ArtSheet};
use crate::text::{Align, TextStyle};
use crate::world::{RenderWorld, Sprite};

/// Marker string of the font. It stays in the download only when the game
/// turns the `text` feature on, which is how the size tools see it.
pub const FONT_ID: &str = "evani.text.font_5x9";

/// First and last character of the font.
const FIRST: u32 = 32;
const LAST: u32 = 126;
/// Glyphs in the font.
pub const GLYPH_COUNT: usize = (LAST - FIRST + 1) as usize;
/// Glyph bitmap size in the small variant.
const GW: usize = 5;
const GH: usize = 9;
/// Design advance (glyph plus one pixel of spacing) and line height of the
/// small variant; the large one is double.
const ADV: f32 = 6.0;
const LINE: f32 = 10.0;
/// Cells per row in the baked sheet (95 glyphs = 5 rows of 19).
const COLS: usize = 19;
const ROWS: usize = GLYPH_COUNT / COLS;

// ---- the font ------------------------------------------------------------------

/// The glyphs in character order from `' '` (32) to `'~'` (126). Each is up
/// to 9 rows of 5 columns, `#` = ink, rows split by `/`; rows left out are
/// empty. Caps and digits stand 7 rows tall on row 6, the baseline;
/// descenders use rows 7 and 8.
pub const GLYPHS: [&str; GLYPH_COUNT] = [
    "",                                                  // space
    "..#../..#../..#../..#../..#../...../..#..",         // !
    ".#.#./.#.#./.#.#.",                                 // "
    ".#.#./.#.#./#####/.#.#./#####/.#.#./.#.#.",         // #
    "..#../.####/#.#../.###./..#.#/####./..#..",         // $
    "##..#/##..#/...#./..#../.#.../#..##/#..##",         // %
    ".##../#..#./#.#../.#.../#.#.#/#..#./.##.#",         // &
    "..#../..#../.#...",                                 // '
    "...#./..#../.#.../.#.../.#.../..#../...#.",         // (
    ".#.../..#../...#./...#./...#./..#../.#...",         // )
    "...../..#../#.#.#/.###./#.#.#/..#../.....",         // *
    "...../...../..#../..#../#####/..#../..#..",         // +
    "...../...../...../...../...../..#../..#../.#...",   // ,
    "...../...../...../.###.",                           // -
    "...../...../...../...../...../...../..#..",         // .
    "....#/....#/...#./..#../.#.../#..../#....",         // /
    ".###./#...#/#..##/#.#.#/##..#/#...#/.###.",         // 0
    "..#../.##../..#../..#../..#../..#../.###.",         // 1
    ".###./#...#/....#/...#./..#../.#.../#####",         // 2
    ".###./#...#/....#/..##./....#/#...#/.###.",         // 3
    "...#./..##./.#.#./#..#./#####/...#./...#.",         // 4
    "#####/#..../####./....#/....#/#...#/.###.",         // 5
    "..##./.#.../#..../####./#...#/#...#/.###.",         // 6
    "#####/....#/...#./..#../.#.../.#.../.#...",         // 7
    ".###./#...#/#...#/.###./#...#/#...#/.###.",         // 8
    ".###./#...#/#...#/.####/....#/...#./.##..",         // 9
    "...../...../..#../..#../...../...../..#../..#..",   // :
    "...../...../...../..#../...../...../..#../.#...",   // ;
    "...../...#./..#../.#.../..#../...#.",               // <
    "...../...../#####/...../#####",                     // =
    "...../.#.../..#../...#./..#../.#...",               // >
    ".###./#...#/....#/...#./..#../...../..#..",         // ?
    ".###./#...#/#.###/#.#.#/#.###/#..../.###.",         // @
    ".###./#...#/#...#/#####/#...#/#...#/#...#",         // A
    "####./#...#/#...#/####./#...#/#...#/####.",         // B
    ".###./#...#/#..../#..../#..../#...#/.###.",         // C
    "####./#...#/#...#/#...#/#...#/#...#/####.",         // D
    "#####/#..../#..../####./#..../#..../#####",         // E
    "#####/#..../#..../####./#..../#..../#....",         // F
    ".###./#...#/#..../#.###/#...#/#...#/.####",         // G
    "#...#/#...#/#...#/#####/#...#/#...#/#...#",         // H
    ".###./..#../..#../..#../..#../..#../.###.",         // I
    "..###/...#./...#./...#./...#./#..#./.##..",         // J
    "#...#/#..#./#.#../##.../#.#../#..#./#...#",         // K
    "#..../#..../#..../#..../#..../#..../#####",         // L
    "#...#/##.##/#.#.#/#.#.#/#...#/#...#/#...#",         // M
    "#...#/##..#/#.#.#/#..##/#...#/#...#/#...#",         // N
    ".###./#...#/#...#/#...#/#...#/#...#/.###.",         // O
    "####./#...#/#...#/####./#..../#..../#....",         // P
    ".###./#...#/#...#/#...#/#.#.#/#..#./.##.#",         // Q
    "####./#...#/#...#/####./#.#../#..#./#...#",         // R
    ".####/#..../#..../.###./....#/....#/####.",         // S
    "#####/..#../..#../..#../..#../..#../..#..",         // T
    "#...#/#...#/#...#/#...#/#...#/#...#/.###.",         // U
    "#...#/#...#/#...#/#...#/#...#/.#.#./..#..",         // V
    "#...#/#...#/#...#/#.#.#/#.#.#/##.##/#...#",         // W
    "#...#/#...#/.#.#./..#../.#.#./#...#/#...#",         // X
    "#...#/#...#/.#.#./..#../..#../..#../..#..",         // Y
    "#####/....#/...#./..#../.#.../#..../#####",         // Z
    ".###./.#.../.#.../.#.../.#.../.#.../.###.",         // [
    "#..../#..../.#.../..#../...#./....#/....#",         // \
    ".###./...#./...#./...#./...#./...#./.###.",         // ]
    "..#../.#.#./#...#",                                 // ^
    "...../...../...../...../...../...../...../#####",   // _
    ".#.../..#..",                                       // `
    "...../...../.###./....#/.####/#...#/.####",         // a
    "#..../#..../####./#...#/#...#/#...#/####.",         // b
    "...../...../.###./#...#/#..../#...#/.###.",         // c
    "....#/....#/.####/#...#/#...#/#...#/.####",         // d
    "...../...../.###./#...#/#####/#..../.###.",         // e
    "..##./.#.../####./.#.../.#.../.#.../.#...",         // f
    "...../...../.####/#...#/#...#/.####/....#/#...#/.###.", // g
    "#..../#..../#.##./##..#/#...#/#...#/#...#",         // h
    "..#../...../.##../..#../..#../..#../.###.",         // i
    "...#./...../..##./...#./...#./...#./...#./#..#./.##..", // j
    "#..../#..../#..#./#.#../##.../#.#../#..#.",         // k
    ".##../..#../..#../..#../..#../..#../.###.",         // l
    "...../...../##.#./#.#.#/#.#.#/#.#.#/#.#.#",         // m
    "...../...../#.##./##..#/#...#/#...#/#...#",         // n
    "...../...../.###./#...#/#...#/#...#/.###.",         // o
    "...../...../####./#...#/#...#/####./#..../#..../#....", // p
    "...../...../.####/#...#/#...#/.####/....#/....#/....#", // q
    "...../...../#.##./##..#/#..../#..../#....",         // r
    "...../...../.####/#..../.###./....#/####.",         // s
    ".#.../.#.../####./.#.../.#.../.#..#/..##.",         // t
    "...../...../#...#/#...#/#...#/#..##/.##.#",         // u
    "...../...../#...#/#...#/#...#/.#.#./..#..",         // v
    "...../...../#...#/#...#/#.#.#/#.#.#/.#.#.",         // w
    "...../...../#...#/.#.#./..#../.#.#./#...#",         // x
    "...../...../#...#/#...#/#...#/.####/....#/#...#/.###.", // y
    "...../...../#####/...#./..#../.#.../#####",         // z
    "..##./.#.../.#.../#..../.#.../.#.../..##.",         // {
    "..#../..#../..#../..#../..#../..#../..#../..#..",   // |
    ".##../...#./...#./....#/...#./...#./.##..",         // }
    "...../...../.##.#/#.##.",                           // ~
];

/// One glyph as a bitmap, `w` x `h` booleans row by row.
#[derive(Clone)]
struct Bits {
    w: usize,
    h: usize,
    on: Vec<bool>,
}

impl Bits {
    fn get(&self, x: isize, y: isize) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.w
            && (y as usize) < self.h
            && self.on.get(y as usize * self.w + x as usize).copied().unwrap_or(false)
    }
}

/// Glyph `i` (0 = space) from [`GLYPHS`] at the small size.
fn small_bits(i: usize) -> Bits {
    let mut on = vec![false; GW * GH];
    if let Some(art) = GLYPHS.get(i) {
        for (y, row) in art.split('/').take(GH).enumerate() {
            for (x, c) in row.bytes().take(GW).enumerate() {
                if let Some(px) = on.get_mut(y * GW + x) {
                    *px = c == b'#';
                }
            }
        }
    }
    Bits { w: GW, h: GH, on }
}

/// Scale2x (EPX): doubles the bitmap and rounds off the stairs of diagonals.
fn scale2x(b: &Bits) -> Bits {
    let (w, h) = (b.w * 2, b.h * 2);
    let mut on = vec![false; w * h];
    for y in 0..b.h as isize {
        for x in 0..b.w as isize {
            let p = b.get(x, y);
            let (u, d, l, r) = (b.get(x, y - 1), b.get(x, y + 1), b.get(x - 1, y), b.get(x + 1, y));
            let q = [
                if l == u && l != d && u != r { u } else { p },
                if u == r && u != l && r != d { r } else { p },
                if d == l && d != r && l != u { l } else { p },
                if r == d && r != u && d != l { d } else { p },
            ];
            let (ox, oy) = (x as usize * 2, y as usize * 2);
            for (k, v) in q.into_iter().enumerate() {
                if let Some(px) = on.get_mut((oy + k / 2) * w + ox + k % 2) {
                    *px = v;
                }
            }
        }
    }
    Bits { w, h, on }
}

/// The bitmap inside a clear border of `t` pixels.
fn pad(b: &Bits, t: usize) -> Bits {
    let (w, h) = (b.w + 2 * t, b.h + 2 * t);
    let mut on = vec![false; w * h];
    for y in 0..b.h {
        for x in 0..b.w {
            if let (Some(d), Some(v)) = (on.get_mut((y + t) * w + x + t), b.on.get(y * b.w + x)) {
                *d = *v;
            }
        }
    }
    Bits { w, h, on }
}

/// Grow the ink by `t` pixels: the first step takes all eight neighbours,
/// later steps the four edge neighbours, so a wide outline has rounded
/// corners.
fn grow(b: &Bits, t: usize) -> Bits {
    let mut cur = b.clone();
    for step in 0..t {
        let mut next = cur.clone();
        for y in 0..cur.h as isize {
            for x in 0..cur.w as isize {
                let mut hit = false;
                for dy in -1..=1isize {
                    for dx in -1..=1isize {
                        if step > 0 && dx != 0 && dy != 0 {
                            continue;
                        }
                        hit |= cur.get(x + dx, y + dy);
                    }
                }
                if let Some(px) = next.on.get_mut(y as usize * cur.w + x as usize) {
                    *px = hit;
                }
            }
        }
        cur = next;
    }
    cur
}

/// Per variant (0 small, 1 large): the margin around a glyph in its frame
/// (= the outline width) and the glyph size.
const fn dims(variant: usize) -> (usize, usize, usize) {
    let k = variant + 1;
    (k, GW * k, GH * k)
}

/// Index of a frame in the engine sheet: plane (`variant * 2 + kind`, kind 0
/// = fill, 1 = outline mask) times the glyph count plus the glyph.
fn frame_index(variant: usize, kind: usize, glyph: usize) -> usize {
    (variant * 2 + kind) * GLYPH_COUNT + glyph
}

/// The sprite cell of a baked frame. The engine sheet is the first one in
/// the `SpriteArt`, so this is fixed.
fn cell_of(variant: usize, kind: usize, glyph: usize) -> u16 {
    ART_CELL0 + frame_index(variant, kind, glyph) as u16
}

fn frame_name(variant: usize, kind: usize, code: u32) -> String {
    let size = if variant == 0 { "" } else { ".x2" };
    let mask = if kind == 0 { "" } else { ".outline" };
    format!("glyph.{code}{size}{mask}")
}

/// Bake the font: all 95 glyphs at both sizes, fill and outline mask, as one
/// pixel-art sheet. White ink on clear, straight alpha.
pub fn font_sheet() -> ArtSheet {
    // Plane (variant, kind) cell sizes and the sheet size.
    let cell = |v: usize| {
        let (t, gw, gh) = dims(v);
        (gw + 2 * t, gh + 2 * t)
    };
    let width = COLS * cell(1).0;
    let height: usize = (0..4).map(|p| ROWS * cell(p / 2).1).sum();
    let mut rgba8 = vec![0u8; width * height * 4];
    let mut frames = Vec::with_capacity(4 * GLYPH_COUNT);
    let mut top = 0;
    for plane in 0..4 {
        let (variant, kind) = (plane / 2, plane % 2);
        let (t, _, _) = dims(variant);
        let (cw, ch) = cell(variant);
        for glyph in 0..GLYPH_COUNT {
            let mut bits = small_bits(glyph);
            if variant == 1 {
                bits = scale2x(&bits);
            }
            // The glyph sits t px in from the cell edge, so the mask has room
            // to grow on every side.
            bits = pad(&bits, t);
            if kind == 1 {
                bits = grow(&bits, t);
            }
            let (ox, oy) = ((glyph % COLS) * cw, top + (glyph / COLS) * ch);
            for y in 0..bits.h {
                for x in 0..bits.w {
                    if bits.on.get(y * bits.w + x).copied().unwrap_or(false) {
                        let o = ((oy + y) * width + ox + x) * 4;
                        if let Some(d) = rgba8.get_mut(o..o + 4) {
                            d.copy_from_slice(&[255; 4]);
                        }
                    }
                }
            }
            frames.push((frame_name(variant, kind, FIRST + glyph as u32), [ox as u32, oy as u32, cw as u32, ch as u32]));
        }
        top += ROWS * ch;
    }
    ArtSheet {
        width: width as u32,
        height: height as u32,
        rgba8,
        frames,
        pixel_art: true,
    }
}

// ---- layout and drawing --------------------------------------------------------

/// Where one line of text lands: the variant and scale, whole-pixel advance
/// and line height, and the size.
struct Layout {
    variant: usize,
    scale: f32,
    advance: f32,
    size: [f32; 2],
}

impl Layout {
    fn new(chars: usize, style: &TextStyle) -> Self {
        let px = if style.size_px.is_finite() { style.size_px.clamp(1.0, 1024.0) } else { LINE };
        // The variant whose scale is nearest 1 (in ratio) wins; ties go small.
        let off = |s: f32| s.max(1.0 / s);
        let variant = usize::from(off(px / (LINE * 2.0)) < off(px / LINE));
        let k = (variant + 1) as f32;
        let scale = px / (LINE * k);
        let advance = (ADV * k * scale).round().max(1.0);
        let height = (LINE * k * scale).round().max(1.0);
        Self {
            variant,
            scale,
            advance,
            size: [chars as f32 * advance, height],
        }
    }
}

/// Size `[width, height]` in pixels that [`draw`] covers for `text`: the
/// advance times the number of characters, by the line height. Outline and
/// shadow ink may reach the outline width past that box.
pub fn measure(text: &str, style: &TextStyle) -> [f32; 2] {
    Layout::new(text.chars().count(), style).size
}

/// Glyph index (0 = space) of a character; anything outside ASCII 32 to 126
/// is `?`.
fn glyph_of(c: char) -> usize {
    let code = c as u32;
    let code = if (FIRST..=LAST).contains(&code) { code } else { u32::from(b'?') };
    (code - FIRST) as usize
}

/// Draw `text` as sprites on `layer` and return its size (as [`measure`]).
/// `pos` is the top of the line; its x is the left edge, middle or right
/// edge by [`TextStyle::align`]. The start is rounded to a whole pixel.
pub fn draw(world: &mut RenderWorld, pos: [f32; 2], layer: i16, text: &str, style: &TextStyle) -> [f32; 2] {
    let lay = Layout::new(text.chars().count(), style);
    let factor = match style.align {
        Align::Left => 0.0,
        Align::Center => 0.5,
        Align::Right => 1.0,
    };
    let x0 = (pos[0] - lay.size[0] * factor).round();
    let y0 = pos[1].round();
    let (t, gw, gh) = dims(lay.variant);
    let (margin, cw, ch) = (t as f32 * lay.scale, (gw + 2 * t) as f32 * lay.scale, (gh + 2 * t) as f32 * lay.scale);
    let offset = margin.round().max(1.0);
    // Shadow (under), outline, then fill: each pass over the whole string.
    let passes = [
        style.shadow.map(|c| (c, usize::from(style.outline.is_some()), offset)),
        style.outline.map(|c| (c, 1, 0.0)),
        Some((style.color, 0, 0.0)),
    ];
    for (color, kind, shift) in passes.into_iter().flatten() {
        for (i, c) in text.chars().enumerate() {
            let glyph = glyph_of(c);
            if glyph == 0 {
                continue;
            }
            let left = x0 + i as f32 * lay.advance - margin + shift;
            let top = y0 - margin + shift;
            world.draw_sprite(Sprite {
                pos: [left + cw * 0.5, top + ch * 0.5],
                size: [cw, ch],
                rotation: 0.0,
                cell: cell_of(lay.variant, kind, glyph),
                color,
                layer,
            });
        }
    }
    lay.size
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::art::{ArtSource, SpriteArt};

    fn art() -> SpriteArt {
        let mut a = SpriteArt::default();
        assert!(a.add(ArtSource::Engine, font_sheet()).is_ok());
        a
    }

    fn style(size: f32) -> TextStyle {
        TextStyle::new(size, [1.0, 0.5, 0.25, 1.0])
    }

    fn drawn(text: &str, s: &TextStyle) -> (RenderWorld, [f32; 2]) {
        let mut w = RenderWorld::default();
        let size = draw(&mut w, [100.0, 50.0], 3, text, s);
        (w, size)
    }

    #[test]
    fn font_has_every_glyph_with_a_distinct_shape() {
        assert_eq!(GLYPHS.len(), 95);
        let mut seen = std::collections::BTreeSet::new();
        for (i, g) in GLYPHS.iter().enumerate() {
            let rows: Vec<&str> = g.split('/').collect();
            assert!(rows.len() <= GH, "glyph {i} has {} rows", rows.len());
            for r in &rows {
                assert!(r.is_empty() || (r.len() == GW && r.bytes().all(|c| c == b'.' || c == b'#')), "glyph {i} row {r:?}");
            }
            let ink = small_bits(i).on.iter().filter(|b| **b).count();
            assert_eq!(ink == 0, i == 0, "only the space is empty (glyph {i})");
            assert!(seen.insert(small_bits(i).on), "glyph {i} repeats another");
        }
    }

    #[test]
    fn frames_are_named_and_cells_are_constants() {
        let a = art();
        for code in FIRST..=LAST {
            let g = (code - FIRST) as usize;
            for variant in 0..2 {
                for kind in 0..2 {
                    assert_eq!(
                        a.cell(&frame_name(variant, kind, code)),
                        Some(cell_of(variant, kind, g)),
                        "glyph {code} variant {variant} kind {kind}"
                    );
                }
            }
        }
        assert_eq!(a.cell("glyph.65"), Some(cell_of(0, 0, 33)));
        assert_eq!(a.cell("glyph.65.x2.outline"), Some(cell_of(1, 1, 33)));
    }

    #[test]
    fn the_font_packs_into_a_small_atlas() {
        let a = art();
        let frames: Vec<_> = a.frames().collect();
        assert_eq!(frames.len(), 4 * GLYPH_COUNT);
        let p = crate::art::pack(&frames, 4096);
        assert!(p.is_ok_and(|p| p.size[0] <= 512 && p.size[1] <= 512 && p.levels == 1));
    }

    #[test]
    fn baked_frames_hold_fill_and_a_grown_outline() {
        let sheet = font_sheet();
        let alpha = |name: &str| -> Vec<Vec<bool>> {
            let Some((_, f)) = sheet.frames.iter().find(|(n, _)| n == name) else {
                return Vec::new();
            };
            (0..f[3])
                .map(|y| {
                    (0..f[2])
                        .map(|x| sheet.rgba8.get((((f[1] + y) * sheet.width + f[0] + x) * 4 + 3) as usize).copied() == Some(255))
                        .collect()
                })
                .collect()
        };
        // Small 'I' (73): 7 x 11 cell, fill at +1,+1. Row 0 of the glyph is ".###.".
        let fill = alpha("glyph.73");
        let mask = alpha("glyph.73.outline");
        assert_eq!((fill.len(), fill[0].len()), (11, 7));
        assert_eq!(fill[1], [false, false, true, true, true, false, false]);
        assert_eq!(fill[0], [false; 7]);
        // The mask covers the fill and one more pixel all round.
        assert_eq!(mask[0], [false, true, true, true, true, true, false]);
        for (fr, mr) in fill.iter().zip(&mask) {
            for (f, m) in fr.iter().zip(mr) {
                assert!(!f || *m);
            }
        }
        // Large: 14 x 22 cells.
        let big = alpha("glyph.73.x2");
        assert_eq!((big.len(), big[0].len()), (22, 14));
        // Scale2x keeps the bar of the I: glyph row 0 is ".###.", so six
        // pixels starting two in from the glyph edge and two from the margin.
        assert_eq!(big[2][3..11], [false, true, true, true, true, true, true, false]);
    }

    #[test]
    fn measure_matches_draw() {
        for text in ["", "A", "Hello, world!", "Score 12,345", "caf\u{e9} \u{1f600}"] {
            for size in [8.0, 10.0, 14.0, 15.0, 20.0, 24.0, 33.3, 40.0] {
                for align in [Align::Left, Align::Center, Align::Right] {
                    let s = TextStyle {
                        align,
                        outline: Some([0.0, 0.0, 0.0, 1.0]),
                        shadow: Some([0.0, 0.0, 0.0, 0.5]),
                        ..style(size)
                    };
                    let (w, got) = drawn(text, &s);
                    assert_eq!(got, measure(text, &s), "{text:?} at {size}");
                    let n = text.chars().count();
                    assert_eq!((got[0] / n.max(1) as f32).fract(), 0.0, "whole-pixel advance");
                    // Every sprite lies within the box grown by its own height.
                    let from = (100.0 - got[0] * [0.0, 0.5, 1.0][align as usize]).round();
                    for sp in &w.sprites {
                        let pad = sp.size[1];
                        assert!(sp.pos[0] - sp.size[0] * 0.5 >= from - pad, "{text:?} {size}");
                        assert!(sp.pos[0] + sp.size[0] * 0.5 <= from + got[0] + pad, "{text:?} {size}");
                        assert!(sp.pos[1] - sp.size[1] * 0.5 >= 50.0 - pad);
                        assert!(sp.pos[1] + sp.size[1] * 0.5 <= 50.0 + got[1] + pad);
                    }
                }
            }
        }
        // Known sizes: 6 px per character at 10, 12 at 20.
        assert_eq!(measure("Hi", &style(10.0)), [12.0, 10.0]);
        assert_eq!(measure("Hi", &style(20.0)), [24.0, 20.0]);
        assert_eq!(measure("Hi", &style(30.0)), [36.0, 30.0]);
        assert_eq!(measure("", &style(20.0)), [0.0, 20.0]);
    }

    #[test]
    fn unknown_glyph_draws_question_mark() {
        let s = style(20.0);
        let (q, _) = drawn("?", &s);
        for odd in ["\u{e9}", "\u{1f600}", "\n", "\t", "\u{7f}", "\u{0}", "\u{4e2d}"] {
            let (w, size) = drawn(odd, &s);
            assert_eq!(w.sprites, q.sprites, "{odd:?}");
            assert_eq!(size, measure("?", &s));
        }
        // A string mixes them in place.
        let (w, _) = drawn("a\u{e9}b", &s);
        let cells: Vec<u16> = w.sprites.iter().map(|s| s.cell).collect();
        assert_eq!(cells, [cell_of(1, 0, 65), cell_of(1, 0, 31), cell_of(1, 0, 66)]);
        // Space advances but draws nothing.
        let (w, size) = drawn("  ", &s);
        assert!(w.sprites.is_empty());
        assert_eq!(size, [24.0, 20.0]);
    }

    #[test]
    fn layout_is_deterministic() {
        let s = TextStyle {
            outline: Some([0.0, 0.0, 0.0, 1.0]),
            shadow: Some([0.0, 0.0, 0.0, 0.5]),
            align: Align::Center,
            ..style(26.0)
        };
        let (a, sa) = drawn("Combo x3!", &s);
        let (b, sb) = drawn("Combo x3!", &s);
        assert_eq!((a.sprites.clone(), sa), (b.sprites, sb));
        // Drawing again into a cleared world repeats it too.
        let mut c = RenderWorld::default();
        draw(&mut c, [100.0, 50.0], 3, "Combo x3!", &s);
        c.clear();
        draw(&mut c, [100.0, 50.0], 3, "Combo x3!", &s);
        assert_eq!(c.sprites, a.sprites);
        // Whole-pixel start and advance: the same text one pixel across moves
        // by exactly one pixel, and fractions round the same way every time.
        let one = TextStyle { align: Align::Left, ..style(20.0) };
        let mut w = RenderWorld::default();
        draw(&mut w, [10.0, 5.0], 0, "AB", &one);
        draw(&mut w, [11.0, 5.0], 0, "AB", &one);
        draw(&mut w, [10.4, 5.4], 0, "AB", &one);
        let xs: Vec<f32> = w.sprites.iter().map(|s| s.pos[0]).collect();
        assert_eq!(xs[2] - xs[0], 1.0);
        assert_eq!(xs[1] - xs[0], 12.0);
        assert_eq!(xs[4], xs[0]);
        assert_eq!(w.sprites[4].pos[1], w.sprites[0].pos[1]);
        // Unscaled sizes put every sprite on whole pixels.
        for sp in &w.sprites {
            assert_eq!(sp.size, [14.0, 22.0]);
            assert_eq!((sp.pos[0] - 7.0).fract(), 0.0);
        }
    }

    #[test]
    fn sizes_pick_the_nearer_variant_and_passes_stack() {
        // 10 and 20 are the baked sizes; 14 stays small, 15 goes large.
        let cell = |size: f32| drawn("A", &style(size)).0.sprites[0].cell;
        assert_eq!(cell(10.0), cell_of(0, 0, 33));
        assert_eq!(cell(14.0), cell_of(0, 0, 33));
        assert_eq!(cell(15.0), cell_of(1, 0, 33));
        assert_eq!(cell(20.0), cell_of(1, 0, 33));
        // Shadow, outline, fill in that order, each tinted by its colour.
        let s = TextStyle {
            outline: Some([0.0, 1.0, 0.0, 1.0]),
            shadow: Some([0.0, 0.0, 1.0, 1.0]),
            ..style(10.0)
        };
        let (w, _) = drawn("AB", &s);
        let order: Vec<(u16, [f32; 4])> = w.sprites.iter().map(|s| (s.cell, s.color)).collect();
        assert_eq!(order.len(), 6);
        assert_eq!(order[0], (cell_of(0, 1, 33), [0.0, 0.0, 1.0, 1.0]));
        assert_eq!(order[1].0, cell_of(0, 1, 34));
        assert_eq!(order[2], (cell_of(0, 1, 33), [0.0, 1.0, 0.0, 1.0]));
        assert_eq!(order[4], (cell_of(0, 0, 33), s.color));
        assert!(w.sprites.iter().all(|s| s.layer == 3));
        // The shadow is one outline width down and right of the outline.
        assert_eq!(w.sprites[0].pos[0] - w.sprites[2].pos[0], 1.0);
        assert_eq!(w.sprites[0].pos[1] - w.sprites[2].pos[1], 1.0);
        // Without an outline the shadow is the fill shape.
        let s = TextStyle { shadow: Some([0.0; 4]), ..style(10.0) };
        assert_eq!(drawn("A", &s).0.sprites[0].cell, cell_of(0, 0, 33));
    }

    #[test]
    fn odd_sizes_do_not_break_layout() {
        for size in [0.0, -5.0, f32::NAN, f32::INFINITY, 1e9] {
            let (w, got) = drawn("Hi", &style(size));
            assert!(got[0].is_finite() && got[1] >= 1.0 && got[0] >= 2.0);
            assert_eq!(w.sprites.len(), 2);
        }
    }

    #[test]
    fn the_render_module_puts_the_font_first() {
        use crate::RenderModule;
        use evani_core::App;
        let mut app = App::new("t").with_module(RenderModule::new());
        app.run_headless(1, 16.0);
        let a = app.world.res::<SpriteArt>();
        assert_eq!(a.cell("glyph.65"), Some(cell_of(0, 0, 33)));
        assert_eq!(a.frames().count(), 4 * GLYPH_COUNT);
        assert_eq!(app.world.res::<evani_core::overlay::ErrorLog>().error_count(), 0);
    }
}
