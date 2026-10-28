//! Text in the renderer: the shared style types and the draw entry points.
//! Two independent features supply the drawing behind them:
//!
//! * `text` (pixel text): one small original bitmap font, baked at startup
//!   into an engine sheet (`bitmap.rs`). It draws [`FontId::BITMAP`], the
//!   default font. Games that only draw pixel text download none of the
//!   sharp-text code.
//! * `msdf` (sharp text, the game's `ui` module): MSDF fonts baked at build
//!   time and fetched at run time (`msdf.rs`). It draws every other
//!   [`FontId`]. It does not need the pixel font: a game that turns on only
//!   `msdf` downloads none of the bitmap glyphs, and a style that names
//!   [`FontId::BITMAP`] (the default) then draws nothing and measures
//!   `[0, 0]`; name a font from [`crate::msdf::Fonts::request`].
//!
//! With both features on, [`draw`] and [`measure_in`] pick the drawing by
//! `TextStyle::font`, as before. This module exists when either feature is
//! on; [`measure`], [`font_sheet`], [`GLYPHS`] and the other pixel-font items
//! exist only with `text`.

#[cfg(feature = "text")]
pub use crate::bitmap::{FONT_ID, GLYPH_COUNT, GLYPHS, font_sheet, measure};
use crate::world::RenderWorld;

/// Alignment of the text against the `pos` given to [`draw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// `pos.x` is the left edge.
    #[default]
    Left,
    /// `pos.x` is the middle.
    Center,
    /// `pos.x` is the right edge.
    Right,
}

/// Which font draws the text: [`FontId::BITMAP`] (the default) is the small
/// bitmap font, for debug text and pixel looks; any other id is an MSDF
/// font from [`crate::msdf::Fonts::request`] (feature `msdf`, the `ui`
/// module), sharp at any size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontId(pub u8);

impl FontId {
    pub const BITMAP: FontId = FontId(0);
}

/// How to draw text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// The font ([`FontId::BITMAP`] by default).
    pub font: FontId,
    /// Wrap width in CSS pixels for MSDF fonts (0, the default: no wrap;
    /// `\n` always breaks). The bitmap font draws one line.
    pub max_width: f32,
    /// Linear RGBA of a soft glow around every letter (MSDF fonts only).
    pub glow: Option<[f32; 4]>,
    /// Line height in CSS pixels. 10 and 20 (and their multiples) are drawn
    /// without any resampling; any other size scales a baked size by a
    /// fraction and, sampled crisply, shows uneven pixel widths, so pixel art
    /// should stay on those sizes.
    pub size_px: f32,
    /// Linear RGBA of the letters.
    pub color: [f32; 4],
    /// Linear RGBA of an outline around every letter, 1 px wide at 10 and 2
    /// px at 20 (scaled with the text).
    pub outline: Option<[f32; 4]>,
    /// Linear RGBA of a drop shadow, one outline width down and to the right.
    pub shadow: Option<[f32; 4]>,
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: FontId::BITMAP,
            max_width: 0.0,
            glow: None,
            size_px: 20.0,
            color: [1.0; 4],
            outline: None,
            shadow: None,
            align: Align::Left,
        }
    }
}

impl TextStyle {
    /// Plain text of a size and colour: no outline, no shadow, left aligned.
    pub fn new(size_px: f32, color: [f32; 4]) -> Self {
        Self {
            size_px,
            color,
            ..Self::default()
        }
    }
}


/// [`measure`] for any font: an MSDF font's size needs its table, which
/// lives in the [`RenderWorld`] (feature `msdf`). A [`FontId::BITMAP`] style
/// measures as the pixel font with `text`, and `[0, 0]` without it.
pub fn measure_in(world: &RenderWorld, text: &str, style: &TextStyle) -> [f32; 2] {
    #[cfg(feature = "msdf")]
    if style.font != FontId::BITMAP {
        return crate::msdf::measure(&world.fonts, text, style);
    }
    let _ = world;
    #[cfg(feature = "text")]
    return measure(text, style);
    #[cfg(not(feature = "text"))]
    {
        let _ = (text, style);
        [0.0, 0.0]
    }
}

/// Draw `text` as sprites on `layer` and return its size (as
/// [`measure_in`]). `pos` is the top of the line; its x is the left edge,
/// middle or right edge by [`TextStyle::align`]. With only `msdf` on, a
/// [`FontId::BITMAP`] style draws nothing.
pub fn draw(world: &mut RenderWorld, pos: [f32; 2], layer: i16, text: &str, style: &TextStyle) -> [f32; 2] {
    #[cfg(feature = "msdf")]
    if style.font != FontId::BITMAP {
        return crate::msdf::draw(world, pos, layer, text, style);
    }
    #[cfg(feature = "text")]
    return crate::bitmap::draw(world, pos, layer, text, style);
    #[cfg(not(feature = "text"))]
    {
        let _ = (world, pos, layer, text, style);
        [0.0, 0.0]
    }
}
