//! The sprite atlas: `ATLAS_GRID x ATLAS_GRID` cells of [`ATLAS_CELL_PX`]
//! pixels, generated on the CPU at startup (no download). Cell 0 is the
//! look's `checker` texture; cells 1.. are white anti-aliased shapes that
//! sprites tint with their colour. RGBA8, sRGB-encoded colour, linear alpha.

use std::collections::BTreeMap;

use evani_core::plugin::{GenRequest, GeneratedAsset, LookData};

use crate::world::{ATLAS_GRID, SpriteSheet};

pub const ATLAS_CELL_PX: u32 = 64;
pub const ATLAS_PX: u32 = ATLAS_GRID * ATLAS_CELL_PX;

/// Where a kit's [`SpriteSheet`] sits in the atlas texture: below the
/// built-in cells.
pub const SHEET_ORIGIN: [u32; 2] = [0, ATLAS_PX];

/// Size of the atlas texture: the built-in square, plus a valid sheet
/// below it (the texture widens to the sheet when it is wider).
pub fn atlas_size(sheet: Option<&SpriteSheet>) -> [u32; 2] {
    match sheet {
        Some(s) if s.is_valid() && s.width > 0 && s.height > 0 => [ATLAS_PX.max(s.width), ATLAS_PX + s.height],
        _ => [ATLAS_PX, ATLAS_PX],
    }
}

/// Named cells, for kits.
pub mod cell {
    pub const CHECKER: u16 = 0;
    pub const CIRCLE: u16 = 1;
    pub const RING: u16 = 2;
    pub const SQUARE: u16 = 3;
    pub const DIAMOND: u16 = 4;
    pub const TRIANGLE: u16 = 5;
    pub const STAR: u16 = 6;
    pub const GLOW: u16 = 7;
    pub const PLUS: u16 = 8;
}

/// Coverage 0..1 of shape `cell` at `(x, y)` in -1..1 cell space.
fn shape(cell: u32, x: f32, y: f32, aa: f32) -> f32 {
    let edge = |d: f32| (0.5 - d / aa).clamp(0.0, 1.0);
    let r = (x * x + y * y).sqrt();
    match cell {
        1 => edge(r - 0.85),
        2 => edge((r - 0.68).abs() - 0.17),
        3 => {
            let q = (x.abs() - 0.62).max(0.0).hypot((y.abs() - 0.62).max(0.0));
            edge(q - 0.2)
        }
        4 => edge(x.abs() + y.abs() - 0.9),
        5 => {
            // Upward triangle (screen y down).
            let d = (y - 0.75).max((x.abs() * 1.732 - y) * 0.5 - 0.45);
            edge(d)
        }
        6 => {
            let a = y.atan2(x) + core::f32::consts::FRAC_PI_2;
            let k = (a * 2.5).cos().abs();
            edge(r - (0.42 + 0.46 * k * k))
        }
        7 => (1.0 - r).clamp(0.0, 1.0).powf(2.0),
        8 => edge((x.abs().min(y.abs()) - 0.22).max(x.abs().max(y.abs()) - 0.85)),
        _ => edge(x.abs().max(y.abs()) - 0.9),
    }
}

/// Build the atlas pixels (`ATLAS_PX * ATLAS_PX * 4` bytes). Deterministic.
pub fn build_atlas(look: &LookData) -> Vec<u8> {
    let w = ATLAS_PX as usize;
    let mut px = vec![0u8; w * w * 4];
    let cp = ATLAS_CELL_PX as usize;
    // Cell 0: the look's checker, or a plain grey checker without a look.
    let checker = look.generate(&GenRequest {
        name: "checker".into(),
        size: ATLAS_CELL_PX,
        seed: 1,
        params: BTreeMap::from([("cells".to_string(), 4.0)]),
    });
    for y in 0..cp {
        for x in 0..cp {
            let o = (y * w + x) * 4;
            let rgba = match &checker {
                Some(GeneratedAsset::Texture { width, height, rgba8 })
                    if *width as usize == cp && *height as usize == cp =>
                {
                    let i = (y * cp + x) * 4;
                    [rgba8[i], rgba8[i + 1], rgba8[i + 2], rgba8[i + 3]]
                }
                _ => {
                    let v = if ((x / 16) + (y / 16)) % 2 == 0 { 200 } else { 90 };
                    [v, v, v, 255]
                }
            };
            px[o..o + 4].copy_from_slice(&rgba);
        }
    }
    let aa = 2.0 / ATLAS_CELL_PX as f32 * 1.5;
    for c in 1..ATLAS_GRID * ATLAS_GRID {
        let (cx, cy) = ((c % ATLAS_GRID) as usize * cp, (c / ATLAS_GRID) as usize * cp);
        for y in 0..cp {
            for x in 0..cp {
                let fx = (x as f32 + 0.5) / cp as f32 * 2.0 - 1.0;
                let fy = (y as f32 + 0.5) / cp as f32 * 2.0 - 1.0;
                let a = (shape(c, fx, fy, aa) * 255.0 + 0.5) as u8;
                let o = ((cy + y) * w + cx + x) * 4;
                px[o..o + 4].copy_from_slice(&[255, 255, 255, a]);
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_has_checker_and_shapes() {
        let px = build_atlas(&LookData::default());
        assert_eq!(px.len(), (ATLAS_PX * ATLAS_PX * 4) as usize);
        let at = |x: u32, y: u32| {
            let o = ((y * ATLAS_PX + x) * 4) as usize;
            [px[o], px[o + 1], px[o + 2], px[o + 3]]
        };
        assert_eq!(at(1, 1)[3], 255, "checker is opaque");
        // Circle cell (1): opaque centre, clear corner.
        assert_eq!(at(64 + 32, 32), [255, 255, 255, 255]);
        assert_eq!(at(64 + 1, 1)[3], 0);
        // Every shape cell has some coverage.
        for c in 1..ATLAS_GRID * ATLAS_GRID {
            let (cx, cy) = ((c % ATLAS_GRID) * 64, (c / ATLAS_GRID) * 64);
            let sum: u32 = (0..64).map(|i| u32::from(at(cx + i, cy + 32)[3])).sum();
            assert!(sum > 0, "cell {c}");
        }
        assert_eq!(build_atlas(&LookData::default()), px);
    }

    #[test]
    fn sheet_sits_below_the_built_in_cells() {
        assert_eq!(atlas_size(None), [256, 256]);
        let mut s = SpriteSheet {
            width: 128,
            height: 64,
            rgba8: vec![0; 128 * 64 * 4],
            frames: vec![[0, 0, 16, 16], [112, 48, 16, 16]],
            revision: 1,
        };
        assert!(s.is_valid());
        assert_eq!(atlas_size(Some(&s)), [256, 320]);
        s.width = 300;
        assert_eq!(atlas_size(Some(&s)), [256, 256], "a bad buffer is ignored");
        s.rgba8 = vec![0; 300 * 64 * 4];
        assert_eq!(atlas_size(Some(&s)), [300, 320]);
        s.frames.push([290, 0, 16, 16]);
        assert!(!s.is_valid(), "frame outside the sheet");
        assert_eq!(SpriteSheet::cell(0), 16);
        assert_eq!(SHEET_ORIGIN, [0, 256]);
    }
}
