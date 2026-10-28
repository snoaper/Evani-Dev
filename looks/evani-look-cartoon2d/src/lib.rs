//! `evani-look-cartoon2d`: bold ink outlines, flat two-tone cel colours and
//! squash and stretch (catalog look `cartoon2d`, 1 point, style 2D). Built
//! for the match-3 kit (kit 003) but usable by any 2D sprite kit.
//!
//! The engine's sprites are white atlas shapes tinted per sprite, so the
//! look's outlines and cel shading are drawn by the kit as layered sprites
//! (outline, shade tone, lit tone, highlight). The look says how:
//!
//! * Palettes: `pieces` (six candy colours) and `ui` (ink, board light,
//!   board dark, frame, background glow, background dots, mascot,
//!   frosting), in that order; kits look them up by name.
//! * Style params (owner `look.cartoon2d`, read by name as
//!   `look.<look>.<param>`): `outline_px` (3.5 px ink), `shade` (0.62, the
//!   shade tone as a fraction of the colour), `highlight` (0.85),
//!   `squash` (1.0, squash-and-stretch amount) and `shadow` (0.3, contact
//!   shadow opacity).
//! * Generator `checker`: a soft bevelled board tile (atlas cell 0), white so
//!   kits tint it.
//! * Generator `sprites` (S3-13c): the smooth piece art, a sheet of white
//!   [`PIECE_FRAMES`] (`piece.cherry` to `piece.prism`) at the size the
//!   renderer asks for (128 px), antialiased from the shape's outline with
//!   16 samples per edge pixel. The silhouettes are the kit's own six
//!   (circle, rounded square, star, diamond, triangle, circle) so pieces
//!   read the same, only smoother; kits tint them and draw the cel layers
//!   on top. A kit asks by name and falls back to its built-in shapes.
//! * Shader slot `tonemap`: identity below 0.8, a soft shoulder above, so
//!   flat cel colours stay as painted and only HDR effect sprites roll off.
//! * Post settings: wants `bloom`, `particles`, `juice` and `shake` on (the
//!   tier budget still decides) and vetoes the 3D `outline` effect (its ink
//!   is in the sprites); warm candy sky `render.clear_color`,
//!   `render.saturation` 1.05, `fx.bloom.threshold` 1.2 so only HDR
//!   sparkles and beams glow.

use evani_core::effects::StyleClasses;
use evani_core::params::{BakeTarget, ParamDef};
use evani_core::plugin::{GenRequest, GeneratedAsset, LookContext, LookPlugin, PluginError};
use evani_core::rng::Pcg32;
use serde_json::json;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Flat below 0.8, soft shoulder to 1.0 above (smooth at the knee).
pub const TONEMAP_BODY: &str = "    // evani-look-cartoon2d: flat colours as painted, soft shoulder for HDR glows\n    let c = max(color, vec3<f32>(0.0));\n    let knee = 0.8;\n    let over = max(c - vec3<f32>(knee), vec3<f32>(0.0));\n    let soft = vec3<f32>(knee) + (1.0 - knee) * over / (over + vec3<f32>(1.0 - knee));\n    return select(c, soft, c > vec3<f32>(knee));";

/// Palette `pieces` (linear RGBA): cherry, toffee, lemon, mint, berry, grape.
pub const PIECES: [[f32; 4]; 6] = [
    [0.860, 0.050, 0.120, 1.0],
    [1.000, 0.420, 0.040, 1.0],
    [1.000, 0.800, 0.060, 1.0],
    [0.120, 0.780, 0.360, 1.0],
    [0.080, 0.420, 1.000, 1.0],
    [0.550, 0.140, 0.900, 1.0],
];

/// Palette `ui` (linear RGBA): ink, board light, board dark, frame,
/// background glow, background dots, mascot, frosting.
pub const UI: [[f32; 4]; 8] = [
    [0.045, 0.020, 0.080, 1.0],
    [0.970, 0.900, 0.960, 1.0],
    [0.860, 0.740, 0.900, 1.0],
    [1.000, 0.780, 0.500, 1.0],
    [1.000, 0.560, 0.660, 1.0],
    [1.000, 1.000, 1.000, 1.0],
    [0.520, 0.840, 1.000, 1.0],
    [1.000, 0.970, 0.990, 1.0],
];

/// The background behind everything (linear RGB).
pub const SKY: [f32; 3] = [0.40, 0.17, 0.52];

/// Style params: name, default, min, max, unit, description.
pub const STYLE: [(&str, f32, f32, f32, &str, &str); 5] = [
    ("look.cartoon2d.outline_px", 3.5, 0.0, 8.0, "px", "Ink outline width around sprites."),
    ("look.cartoon2d.shade", 0.62, 0.0, 1.0, "x", "Shade tone as a fraction of the colour (1 = flat)."),
    ("look.cartoon2d.highlight", 0.85, 0.0, 1.0, "", "Highlight opacity on the lit side."),
    ("look.cartoon2d.squash", 1.0, 0.0, 2.0, "x", "Squash-and-stretch amount in the animator."),
    ("look.cartoon2d.shadow", 0.3, 0.0, 1.0, "", "Contact shadow opacity."),
];

/// The cartoon 2D look.
#[derive(Debug, Default, Clone, Copy)]
pub struct Cartoon2dLook;

impl LookPlugin for Cartoon2dLook {
    fn name(&self) -> &'static str {
        "cartoon2d"
    }

    /// Catalog `cost_points` 1.
    fn points(&self) -> u32 {
        1
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::TWO_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        look.palette("pieces", &PIECES);
        look.palette("ui", &UI);
        for (name, d, min, max, unit, desc) in STYLE {
            look.param_f32(ParamDef::f32(name, d).range(min, max).unit(unit).describe(desc).bake(BakeTarget::Look))?;
        }
        look.shader_slot("tonemap", TONEMAP_BODY)?;
        look.generator("checker", tile);
        look.generator("sprites", sprites);
        // Values tuned live and written by bake.export; applied at start.
        look.baked_files(BAKED);

        look.post_effect("bloom", true);
        look.post_effect("particles", true);
        look.post_effect("juice", true);
        look.post_effect("shake", true);
        // Depth-edge ink is for 3D; this look's ink is in the sprites.
        look.post_effect("outline", false);
        look.post_param("render.clear_color", json!(SKY));
        look.post_param("render.saturation", json!(1.05));
        look.post_param("fx.bloom.threshold", json!(1.2));
        Ok(())
    }
}

/// A soft bevelled tile: rounded square, light rim top-left, darker rim
/// bottom-right, flat middle; white-ish so kits tint it. Deterministic.
pub fn tile(req: &GenRequest, _rng: &mut Pcg32) -> GeneratedAsset {
    let size = req.size.clamp(8, 1024);
    let n = size as f32;
    let r = n * 0.16;
    let to8 = |c: f32| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    let mut rgba8 = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            // Rounded-rectangle distance (negative inside), 1 px inset.
            let hx = (px - n * 0.5).abs() - (n * 0.5 - 1.0 - r);
            let hy = (py - n * 0.5).abs() - (n * 0.5 - 1.0 - r);
            let d = hx.max(0.0).hypot(hy.max(0.0)) + hx.max(hy).min(0.0) - r;
            let a = (0.5 - d).clamp(0.0, 1.0);
            // Bevel: within `rim` px of the edge, lighter on the top-left.
            let rim = n * 0.14;
            let edge = (-d / rim).clamp(0.0, 1.0);
            let side = ((n - px) + (n - py) - n) / n; // +1 top-left .. -1 bottom-right
            let bevel = (1.0 - edge) * side * 0.22;
            let v = 0.9 + bevel;
            rgba8.extend_from_slice(&[to8(v), to8(v), to8(v), (a * 255.0 + 0.5) as u8]);
        }
    }
    GeneratedAsset::Texture {
        width: size,
        height: size,
        rgba8,
    }
}

/// The named frames of the `sprites` sheet, in sheet order: one per piece
/// colour of the `pieces` palette (cherry, toffee, lemon, mint, berry,
/// grape) and the colourless prism.
pub const PIECE_FRAMES: [&str; 7] = [
    "piece.cherry",
    "piece.toffee",
    "piece.lemon",
    "piece.mint",
    "piece.berry",
    "piece.grape",
    "piece.prism",
];

/// The silhouettes of the piece frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Silhouette {
    Circle,
    Square,
    Star,
    Diamond,
    Triangle,
}

/// Which silhouette each of [`PIECE_FRAMES`] has: the kit's six shapes
/// (the grape's hole is drawn by the kit on top) and a sphere for the prism.
const FRAME_SILHOUETTES: [Silhouette; 7] = [
    Silhouette::Circle,
    Silhouette::Square,
    Silhouette::Star,
    Silhouette::Diamond,
    Silhouette::Triangle,
    Silhouette::Circle,
    Silhouette::Circle,
];

/// Signed outline value of a silhouette at `(x, y)` in -1..1 frame space
/// (screen y down): negative inside, zero on the edge. Each shape fills the
/// frame as much as the kit's built-in shape of the same name, so a piece
/// keeps its size and ink width.
fn outline(s: Silhouette, x: f32, y: f32) -> f32 {
    let r = x.hypot(y);
    match s {
        Silhouette::Circle => r - 0.85,
        Silhouette::Square => (x.abs() - 0.62).max(0.0).hypot((y.abs() - 0.62).max(0.0)) - 0.2,
        Silhouette::Diamond => x.abs() + y.abs() - 0.9,
        // Upward triangle.
        Silhouette::Triangle => (y - 0.75).max((x.abs() * 1.732 - y) * 0.5 - 0.45),
        // Five points, the first straight up.
        Silhouette::Star => {
            let a = y.atan2(x) + core::f32::consts::FRAC_PI_2;
            let k = (a * 2.5).cos().abs();
            r - (0.42 + 0.46 * k * k)
        }
    }
}

/// Edge coverage 0..255 of pixel `(px, py)` of a `size` px frame: fully in
/// or out away from the edge, 4 x 4 samples on the pixels the edge crosses.
fn edge_alpha(s: Silhouette, px: u32, py: u32, size: u32) -> u8 {
    let n = size as f32;
    let texel = 2.0 / n;
    let cx = (px as f32 + 0.5) / n * 2.0 - 1.0;
    let cy = (py as f32 + 0.5) / n * 2.0 - 1.0;
    // The star's outline value changes up to about three times faster than
    // the distance to its edge, so look four texels around it.
    let d = outline(s, cx, cy);
    if d > 4.0 * texel {
        return 0;
    }
    if d < -4.0 * texel {
        return 255;
    }
    let mut hit = 0u32;
    for j in 0..4 {
        for i in 0..4 {
            let sx = cx + ((i as f32 + 0.5) / 4.0 - 0.5) * texel;
            let sy = cy + ((j as f32 + 0.5) / 4.0 - 0.5) * texel;
            if outline(s, sx, sy) <= 0.0 {
                hit += 1;
            }
        }
    }
    (hit * 255 / 16) as u8
}

/// The smooth piece sprites: [`PIECE_FRAMES`] side by side in one row of
/// `req.size` px frames (clamped to 32..=256), white with antialiased
/// edges, so the kit tints them like the built-in shapes. Deterministic;
/// no randomness. The renderer packs them with a gutter and builds the mips.
pub fn sprites(req: &GenRequest, _rng: &mut Pcg32) -> GeneratedAsset {
    let size = req.size.clamp(32, 256);
    let count = PIECE_FRAMES.len() as u32;
    let width = size * count;
    let mut rgba8 = vec![255u8; (width * size * 4) as usize];
    let mut frames = Vec::with_capacity(PIECE_FRAMES.len());
    // Coverage per silhouette, computed once for the frames that share it.
    let mut cache: Vec<(Silhouette, Vec<u8>)> = Vec::new();
    for (i, (name, s)) in PIECE_FRAMES.iter().zip(FRAME_SILHOUETTES).enumerate() {
        let at = match cache.iter().position(|(c, _)| *c == s) {
            Some(at) => at,
            None => {
                let mut cover = Vec::with_capacity((size * size) as usize);
                for y in 0..size {
                    for x in 0..size {
                        cover.push(edge_alpha(s, x, y, size));
                    }
                }
                cache.push((s, cover));
                cache.len() - 1
            }
        };
        let x0 = i as u32 * size;
        if let Some((_, cover)) = cache.get(at) {
            for y in 0..size {
                for x in 0..size {
                    let o = ((y * width + x0 + x) * 4 + 3) as usize;
                    if let (Some(dst), Some(a)) = (rgba8.get_mut(o), cover.get((y * size + x) as usize)) {
                        *dst = *a;
                    }
                }
            }
        }
        frames.push((name.to_string(), [x0, 0, size, size]));
    }
    GeneratedAsset::SpriteSheet {
        width,
        height: size,
        rgba8,
        frames,
        pixel_art: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::{ParamRegistry, ParamValue};
    use evani_core::plugin::LookData;
    use evani_core::slots::ShaderSlots;

    #[test]
    fn registers_palettes_style_and_effects() {
        let mut a = App::new("t").with_look(Cartoon2dLook);
        a.start();
        let d = a.world.res::<LookData>();
        assert_eq!(d.name, "cartoon2d");
        assert_eq!(d.points, 1);
        assert_eq!(d.styles, StyleClasses::TWO_D);
        assert_eq!(d.palette("pieces").unwrap().colors.len(), 6);
        assert_eq!(d.palette("ui").unwrap().colors.len(), 8);
        for fx in ["bloom", "particles", "juice", "shake"] {
            assert!(d.post_effects.contains(&(fx.to_string(), true)), "{fx}");
        }
        assert!(d.post_effects.contains(&("outline".to_string(), false)));
        drop(d);
        let reg = a.world.res::<ParamRegistry>();
        let id = reg.id("look.cartoon2d.outline_px").unwrap();
        assert_eq!(reg.def(id).unwrap().owner, "look.cartoon2d");
        // The catalog's 3 to 4 px outlines.
        let Some(ParamValue::F32(o)) = reg.value(id) else { panic!() };
        assert!((3.0..=4.0).contains(&o), "{o}");
    }

    #[test]
    fn tonemap_is_flat_below_the_knee_and_valid_wgsl() {
        let mut a = App::new("t").with_look(Cartoon2dLook);
        a.start();
        let slots = a.world.res::<ShaderSlots>();
        let id = slots.id("tonemap").unwrap();
        let mut src = slots.wgsl_function(id).unwrap();
        assert!(src.contains("evani-look-cartoon2d"), "look body not installed");
        src.push_str(
            "\n@fragment fn fs_main() -> @location(0) vec4<f32> {\n    return vec4<f32>(slot_tonemap(vec3<f32>(2.0, 0.5, 0.1)), 1.0);\n}\n",
        );
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap();
        // The same curve on the CPU: flat below 0.8, under 1 above, smooth.
        let f = |c: f32| {
            let k = 0.8;
            if c > k {
                let o = c - k;
                k + (1.0 - k) * o / (o + 1.0 - k)
            } else {
                c
            }
        };
        assert_eq!(f(0.5), 0.5);
        assert!(f(3.0) < 1.0 && f(3.0) > 0.95);
        assert!((f(0.8001) - 0.8001).abs() < 1e-4);
    }

    /// Width, height, RGBA pixels and named frames of a generated sheet.
    type Sheet = (u32, u32, Vec<u8>, Vec<(String, [u32; 4])>);

    fn sheet(size: u32, seed: u64) -> Sheet {
        let req = GenRequest {
            name: "sprites".into(),
            size,
            seed,
            params: Default::default(),
        };
        let GeneratedAsset::SpriteSheet {
            width,
            height,
            rgba8,
            frames,
            pixel_art,
        } = sprites(&req, &mut Pcg32::seeded(seed))
        else {
            panic!("sprite sheet")
        };
        assert!(!pixel_art, "smooth art, with mips");
        (width, height, rgba8, frames)
    }

    #[test]
    fn the_look_supplies_smooth_white_piece_frames_by_name() {
        let (w, h, px, frames) = sheet(128, 1);
        assert_eq!((w, h), (7 * 128, 128));
        assert_eq!(px.len(), (w * h * 4) as usize);
        let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, PIECE_FRAMES);
        let alpha = |f: usize, x: u32, y: u32| px[(((y * w) + frames[f].1[0] + x) * 4 + 3) as usize];
        for (i, (_, r)) in frames.iter().enumerate() {
            assert_eq!(*r, [i as u32 * 128, 0, 128, 128], "frames sit side by side");
            assert!(r[0] + r[2] <= w && r[3] <= h);
            assert_eq!(alpha(i, 64, 64), 255, "{} is solid in the middle", names[i]);
            assert_eq!(alpha(i, 0, 0), 0, "{} has a clear corner", names[i]);
            // Smooth: the edge has in-between coverage, not just 0 and 255.
            let soft = (0..128).filter(|&x| (1..255).contains(&alpha(i, x, 64)) || (1..255).contains(&alpha(i, 64, x))).count();
            assert!(soft >= 2, "{} has an antialiased edge", names[i]);
        }
        // White: kits tint it.
        assert!(px.chunks(4).all(|p| p[..3] == [255, 255, 255]));
        // The rounded square is clear in its corner and full along its edge.
        assert!(alpha(1, 3, 3) < 20 && alpha(1, 64, 12) == 255);
        // The star is not a circle: a point reaches further up than the sides.
        assert!(alpha(2, 64, 8) > 200 && alpha(2, 8, 64) == 0);
        // A triangle point-up: wide at the foot, narrow at the top.
        let row = |f: usize, y: u32| (0..128).filter(|&x| alpha(f, x, y) > 127).count();
        assert!(row(4, 110) > 5 * row(4, 40) / 2 && row(4, 40) > 20);
        // Frames that share a silhouette are the same picture.
        let same = |a: usize, b: usize| (0..128 * 128).all(|k| alpha(a, k % 128, k / 128) == alpha(b, k % 128, k / 128));
        assert!(same(0, 5) && same(0, 6) && !same(0, 1));
        // Deterministic, whatever the seed; other sizes keep the layout.
        assert_eq!(sheet(128, 1).2, sheet(128, 99).2);
        let (w2, h2, px2, f2) = sheet(64, 1);
        assert_eq!((w2, h2, px2.len(), f2.len()), (7 * 64, 64, 7 * 64 * 64 * 4, 7));
        assert_eq!(sheet(8, 1).1, 32, "tiny requests are raised to 32 px");
    }

    #[test]
    fn the_sprites_generator_is_registered() {
        let mut a = App::new("t").with_look(Cartoon2dLook);
        a.start();
        let d = a.world.res::<LookData>();
        let got = d.generate(&GenRequest {
            name: "sprites".into(),
            size: 128,
            seed: 1,
            params: Default::default(),
        });
        assert!(matches!(got, Some(GeneratedAsset::SpriteSheet { .. })));
    }

    #[test]
    fn tile_is_a_rounded_bevelled_square() {
        let req = GenRequest {
            name: "checker".into(),
            size: 64,
            seed: 1,
            params: Default::default(),
        };
        let GeneratedAsset::Texture { width, rgba8, .. } = tile(&req, &mut Pcg32::seeded(1)) else {
            panic!("texture")
        };
        assert_eq!(width, 64);
        let at = |x: usize, y: usize| &rgba8[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "rounded corner is clear");
        assert_eq!(at(32, 32)[3], 255);
        assert!(at(6, 32)[0] > at(57, 32)[0], "lit on the left, darker on the right");
        assert_eq!(tile(&req, &mut Pcg32::seeded(9)), tile(&req, &mut Pcg32::seeded(1)));
    }
}
