//! `evani-look-pixel16`: 16-bit era pixel art (catalog look `pixel16`, 1
//! point, 2D). Rich colour ramps on the 15-bit grid of 16-bit hardware,
//! cooler and hazier parallax planes with distance, a banded sky and crisp
//! whole-pixel scaling. Built for the 2D platformer (kit 002) but usable by
//! any 2D kit that draws indexed art.
//!
//! * Palettes (linear RGBA, each colour snapped to 5 bits per channel):
//!   - `ramp.<name>`: 4 shades, dark to light, for the 15 ramps of
//!     [`RAMPS`]. A kit's indexed art names a ramp and a shade, so another
//!     look recolours everything.
//!   - `layer.far`, `layer.mid`, `layer.near`: two entries per parallax
//!     plane: a multiply tint, then a haze colour whose alpha is how much of
//!     it is mixed in (distance haze: cooler and lighter far away).
//!   - `sky`: 6 bands, top of the screen to the horizon.
//! * Shader slot `tonemap`: passes colours through unchanged (clamped), so
//!   the palette colours reach the screen exactly; no filmic curve.
//! * Post settings: `render.pixel_art` on (nearest sampling, whole-pixel
//!   scaling), exposure and saturation 1, the top sky band as
//!   `render.clear_color`; asks for `parallax` on (the tier budget still
//!   decides).

use evani_core::effects::StyleClasses;
use evani_core::plugin::{LookContext, LookPlugin, PluginError};
use serde_json::json;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Exact colours: clamp to the display range and nothing else.
pub const TONEMAP_BODY: &str = "    // evani-look-pixel16: exact palette colours, no curve\n    return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));";

/// The colour ramps (`ramp.<name>`), sRGB `0xRRGGBB`, dark to light.
/// [`snap555`] puts each on the 15-bit grid.
pub const RAMPS: [(&str, [u32; 4]); 15] = [
    ("ink", [0x10_1018, 0x28_2838, 0x48_4860, 0x70_7088]),
    ("fur", [0x78_2808, 0xC0_5010, 0xF0_8830, 0xF8_C070]),
    ("teal", [0x08_4850, 0x10_8888, 0x30_C0B0, 0x88_F0D8]),
    ("green", [0x18_4010, 0x30_7818, 0x58_B028, 0xA8_E850]),
    ("leaf", [0x10_3028, 0x20_5838, 0x38_8850, 0x68_B870]),
    ("brown", [0x38_1808, 0x68_3818, 0x98_6030, 0xC8_9058]),
    ("stone", [0x38_3848, 0x60_6878, 0x90_98A8, 0xC8_D0D8]),
    ("gold", [0x70_4000, 0xC0_8000, 0xF8_C800, 0xF8_F8A0]),
    ("red", [0x58_0818, 0x98_1828, 0xD8_3838, 0xF8_8080]),
    ("blue", [0x18_2058, 0x28_40A0, 0x48_78E0, 0x90_C0F8]),
    ("water", [0x10_3878, 0x20_60B0, 0x38_90D8, 0x80_D0F8]),
    ("sky", [0x38_58B0, 0x58_88D8, 0x88_B8F0, 0xC0_E0F8]),
    ("white", [0x78_7890, 0xA8_A8C0, 0xD8_D8E8, 0xF8_F8F8]),
    ("shadow", [0x10_1828, 0x20_2840, 0x30_3858, 0x40_4870]),
    ("soil", [0x10_0810, 0x1C_0E12, 0x2A_1410, 0x48_2818]),
];

/// Parallax plane tints: `(name, multiply 0xRRGGBB, haze 0xRRGGBB, haze amount)`.
pub const LAYERS: [(&str, u32, u32, f32); 3] = [
    ("layer.far", 0xD0_E0FF, 0xA8_C8F0, 0.45),
    ("layer.mid", 0xE8_F0FF, 0xB0_D0F0, 0.22),
    ("layer.near", 0xFF_FFFF, 0xC0_E0F8, 0.0),
];

/// Sky bands, top of the screen to the horizon.
pub const SKY: [u32; 6] = [0x30_60C0, 0x40_78D0, 0x58_90E0, 0x70_A8E8, 0x90_C0F0, 0xB8_D8F8];

/// Snap an sRGB `0xRRGGBB` colour to 5 bits per channel (the 15-bit colour
/// of 16-bit consoles) and return it as linear RGBA.
pub fn snap555(rgb: u32) -> [f32; 4] {
    let ch = |shift: u32| {
        let v = (rgb >> shift) & 0xFF;
        let five = (v * 31 + 127) / 255;
        srgb_to_linear((five * 255 + 15) / 31)
    };
    [ch(16), ch(8), ch(0), 1.0]
}

/// One sRGB byte to linear 0..1.
pub fn srgb_to_linear(v: u32) -> f32 {
    let c = v.min(255) as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The 16-bit pixel look.
#[derive(Debug, Default, Clone, Copy)]
pub struct Pixel16Look;

impl LookPlugin for Pixel16Look {
    fn name(&self) -> &'static str {
        "pixel16"
    }

    /// Catalog `cost_points` 1.
    fn points(&self) -> u32 {
        1
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::TWO_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        for (name, shades) in RAMPS {
            let colors = shades.map(snap555);
            look.palette(&format!("ramp.{name}"), &colors);
        }
        for (name, tint, haze, amount) in LAYERS {
            let mut h = snap555(haze);
            h[3] = amount;
            look.palette(name, &[snap555(tint), h]);
        }
        let sky = SKY.map(snap555);
        look.palette("sky", &sky);

        look.shader_slot("tonemap", TONEMAP_BODY)?;
        // Values tuned live and written by bake.export; applied at start.
        look.baked_files(BAKED);

        look.post_effect("parallax", true);
        look.post_param("render.pixel_art", json!(true));
        look.post_param("render.exposure", json!(1.0));
        look.post_param("render.saturation", json!(1.0));
        look.post_param("render.clear_color", json!([sky[0][0], sky[0][1], sky[0][2]]));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::LookData;
    use evani_core::slots::ShaderSlots;

    #[test]
    fn registers_ramps_layers_sky_and_effects() {
        let mut a = App::new("t").with_look(Pixel16Look);
        a.start();
        let d = a.world.res::<LookData>();
        assert_eq!(d.name, "pixel16");
        assert_eq!(d.points, 1);
        assert_eq!(d.styles, StyleClasses::TWO_D);
        for (name, _) in RAMPS {
            let p = d.palette(&format!("ramp.{name}")).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(p.colors.len(), 4, "{name}");
            // Dark to light.
            let lum = |c: [f32; 4]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
            assert!(p.colors.windows(2).all(|w| lum(w[0]) < lum(w[1])), "{name} not dark to light");
        }
        for l in ["layer.far", "layer.mid", "layer.near"] {
            assert_eq!(d.palette(l).unwrap().colors.len(), 2, "{l}");
        }
        assert_eq!(d.palette("sky").unwrap().colors.len(), 6);
        assert!(d.post_effects.contains(&("parallax".to_string(), true)));
        assert!(d.materials.is_empty(), "2D: no materials");
    }

    #[test]
    fn colours_sit_on_the_15_bit_grid() {
        // Every channel, back in sRGB bytes, is a multiple of 255/31.
        for (_, shades) in RAMPS {
            for c in shades.map(snap555) {
                for ch in &c[..3] {
                    let s = if *ch <= 0.0031308 { ch * 12.92 } else { 1.055 * ch.powf(1.0 / 2.4) - 0.055 };
                    let byte = (s * 255.0).round();
                    let five = (byte * 31.0 / 255.0).round();
                    assert!((five * 255.0 / 31.0 - byte).abs() <= 1.0, "{byte}");
                }
            }
        }
        assert_eq!(snap555(0xFF_FFFF), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(snap555(0), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn sets_pixel_art_and_exact_tonemap() {
        let mut a = App::new("t")
            .with_module(evani_core_render_stub::Stub)
            .with_look(Pixel16Look);
        a.start();
        let slots = a.world.res::<ShaderSlots>();
        let id = slots.id("tonemap").unwrap();
        let mut src = slots.wgsl_function(id).unwrap();
        assert!(src.contains("evani-look-pixel16"));
        src.push_str("\n@fragment fn fs_main() -> @location(0) vec4<f32> {\n    return vec4<f32>(slot_tonemap(vec3<f32>(2.0, 1.0, 0.5)), 1.0);\n}\n");
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
        drop(slots);
        let reg = a.world.res::<evani_core::params::ParamRegistry>();
        let id = reg.id("render.pixel_art").unwrap();
        assert_eq!(reg.value(id), Some(evani_core::params::ParamValue::Bool(true)));
    }

    /// A stand-in for the render module: registers `render.pixel_art` so the
    /// look's `post_param` has something to override (the look crate does
    /// not depend on evani-render).
    mod evani_core_render_stub {
        use evani_core::params::ParamDef;
        use evani_core::plugin::{EngineModule, PluginContext, PluginError};

        pub struct Stub;
        impl EngineModule for Stub {
            fn id(&self) -> &'static str {
                "evani.module.test_render"
            }
            fn label(&self) -> &'static str {
                "render stub"
            }
            fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
                ctx.param_bool(ParamDef::bool("render.pixel_art", false))?;
                Ok(())
            }
        }
    }
}
