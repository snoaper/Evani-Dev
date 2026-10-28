//! `evani-look-pixelhd`: HD pixel art (catalog look `pixelhd`, 2 points, 2D;
//! cameras side, top-down, isometric, board-ui; full look from tier Low).
//! "Modern pixel art with a smooth camera, dynamic light and lots of
//! particles." Built for the top-down action-adventure (kit 004) but usable
//! by any 2D kit that draws indexed pixel art.
//!
//! Catalog render rules, and where each is met:
//!
//! * "480x270 internal": the look publishes the grid as the params
//!   `look.pixelhd.internal_width` / `internal_height`. The engine's
//!   render path owns the internal target; until it reads these, a kit
//!   snaps art pixels to the grid itself. `render.pixel_art` is on (nearest
//!   sampling, whole-pixel scaling).
//! * "pixel-perfect sprites with sub-pixel camera smoothing": sprites sit on
//!   whole grid pixels; the camera follows smoothly (`camera_smooth`) and the
//!   remainder below one grid pixel (`camera_subpixel`, on) is carried into
//!   the scaled frame so motion is not stepped. The kit applies both.
//! * "optional normal-mapped 2D lights": see [`LIGHTS_2D_EFFECT`]. The look
//!   registers the light palette and light params, prefers the catalog
//!   effect `lights2d` on and posts its light values to `fx.lights2d.*`;
//!   the effect (S5-R1, evani-fx and evani-render `lights2d`) draws them.
//! * "particles at native resolution": `particles` is preferred on and
//!   `particle_native` (on) tells kits to draw particles at the full
//!   device resolution, not on the 480x270 grid.
//!
//! Registered as parameters (owner `look.pixelhd`, read by name as
//! `look.pixelhd.<param>`): see [`STYLE`], [`LIGHT_STYLE`], the three
//! switches and the internal size.
//!
//! Palettes (linear RGBA):
//! * `ramp.<name>`: 4 shades, dark to light, for each of [`RAMPS`]. Hue
//!   shifted (shadows cooler, lights warmer) and full colour, not snapped
//!   to a console grid. The names match `evani-look-pixel16` (plus
//!   `grass`, `sand`, `wood`), so a kit's indexed art recolours between
//!   the two looks.
//! * `light`: the light colours of the optional lights, in the order of
//!   [`LIGHT_NAMES`].
//! * `sky`: 6 bands, top of the screen to the horizon.
//!
//! Shader slot `tonemap`: exact pixel colours up to a knee, then a soft
//! shoulder, so painted colours reach the screen untouched and only bright
//! HDR light accents and particles roll off.

use evani_core::effects::StyleClasses;
use evani_core::params::{BakeTarget, ParamDef};
use evani_core::plugin::{LookContext, LookPlugin, PluginError};
use serde_json::json;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Exact below 0.9, a soft shoulder to 1.0 above (smooth at the knee).
pub const TONEMAP_BODY: &str = "    // evani-look-pixelhd: painted colours as drawn, soft shoulder for HDR light accents\n    let c = max(color, vec3<f32>(0.0));\n    let knee = 0.9;\n    let over = max(c - vec3<f32>(knee), vec3<f32>(0.0));\n    let soft = vec3<f32>(knee) + (1.0 - knee) * over / (over + vec3<f32>(1.0 - knee));\n    return select(c, soft, c > vec3<f32>(knee));";

/// The internal grid the catalog asks for.
pub const INTERNAL_SIZE: [i32; 2] = [480, 270];

/// The catalog effect that carries the optional normal-mapped 2D lights
/// (`catalog/effects.json` `lights2d`: "Normal-mapped sprites lit by point
/// lights accumulated in a light buffer"). The look prefers it on and
/// supplies its defaults; the tier budget and the game's `evani.toml` still
/// decide whether it exists. Nothing in this crate draws light.
pub const LIGHTS_2D_EFFECT: &str = "lights2d";

/// The colour ramps (`ramp.<name>`), sRGB `0xRRGGBB`, dark to light.
pub const RAMPS: [(&str, [u32; 4]); 18] = [
    ("ink", [0x14_1222, 0x2C_2A44, 0x4E_4C6C, 0x7A_7898]),
    ("fur", [0x7C_2E14, 0xC4_5A1C, 0xF0_8E3C, 0xFF_C87C]),
    ("teal", [0x0C_4458, 0x14_8494, 0x34_C2B4, 0x8C_F4DC]),
    ("green", [0x1C_4A2A, 0x38_8426, 0x66_BC34, 0xB4_EC5E]),
    ("leaf", [0x14_3434, 0x24_6040, 0x3E_9058, 0x74_C27A]),
    ("brown", [0x3E_1E1C, 0x70_4222, 0xA4_6C3A, 0xD6_9C64]),
    ("stone", [0x3A_3C52, 0x62_6C84, 0x94_A0B4, 0xCC_D6E0]),
    ("gold", [0x7A_4410, 0xC8_8A0C, 0xFC_CE24, 0xFF_F6A8]),
    ("red", [0x5C_1026, 0x9C_2234, 0xDC_4440, 0xFF_8C84]),
    ("blue", [0x1C_2862, 0x2C_4CAA, 0x4C_84EA, 0x98_C8FF]),
    ("water", [0x14_3C80, 0x24_68B8, 0x3C_9CE0, 0x88_D8FF]),
    ("sky", [0x3C_5CB8, 0x5C_90E0, 0x8C_C0F6, 0xC8_E8FF]),
    ("white", [0x7C_7C98, 0xAC_AEC8, 0xDC_DEEE, 0xFF_FFFF]),
    ("shadow", [0x12_1A2C, 0x22_2C48, 0x34_3E62, 0x46_5282]),
    ("soil", [0x14_0C14, 0x24_1418, 0x38_2018, 0x58_3420]),
    ("grass", [0x22_5028, 0x42_8A2C, 0x78_C040, 0xC0_EC6C]),
    ("sand", [0x8A_6A3C, 0xBE_9A5A, 0xE4_C888, 0xFA_EEC0]),
    ("wood", [0x34_1C1C, 0x62_3A26, 0x92_5E38, 0xC4_8E58]),
];

/// Sky bands, top of the screen to the horizon.
pub const SKY: [u32; 6] = [0x28_4CB4, 0x38_64CC, 0x50_80E0, 0x6C_9CEC, 0x92_BCF4, 0xC0_DEFC];

/// The light palette `light`, in order: ambient, sun, warm (torch, fire),
/// cool (magic, moon), rim accent.
pub const LIGHT_NAMES: [&str; 5] = ["ambient", "sun", "warm", "cool", "rim"];
/// Colours of [`LIGHT_NAMES`], sRGB `0xRRGGBB`.
pub const LIGHTS: [u32; 5] = [0x58_5E88, 0xFF_E8B8, 0xFF_A850, 0x64_B4FF, 0xFF_F0D0];

/// Pixel and camera style params: name, default, min, max, unit, description.
pub const STYLE: [(&str, f32, f32, f32, &str, &str); 4] = [
    ("look.pixelhd.camera_smooth", 0.18, 0.0, 1.0, "x", "Camera follow smoothing per step (0 = locked on, 1 = barely moves)."),
    ("look.pixelhd.particle_scale", 1.0, 0.25, 4.0, "x", "Particle size and count scale; particles are drawn at native resolution."),
    ("look.pixelhd.shadow", 0.35, 0.0, 1.0, "", "Contact shadow opacity under sprites."),
    ("look.pixelhd.accent_glow", 0.6, 0.0, 2.0, "x", "Strength of the soft glow on bright light accents (pairs with the bloom effect)."),
];

/// Prefix of the light params; the rest of each name is the `fx.lights2d.*`
/// param the look posts its value to.
pub const LIGHT_PREFIX: &str = "look.pixelhd.light.";

/// Params for the optional 2D lights. Their defaults are posted to the
/// `lights2d` effect's `fx.lights2d.*` params of the same last name.
pub const LIGHT_STYLE: [(&str, f32, f32, f32, &str, &str); 5] = [
    ("look.pixelhd.light.ambient", 0.55, 0.0, 1.0, "", "Base light level before any dynamic light (1 = fully lit)."),
    ("look.pixelhd.light.strength", 1.0, 0.0, 3.0, "x", "Brightness of the dynamic lights."),
    ("look.pixelhd.light.normal_strength", 0.8, 0.0, 2.0, "x", "How much a sprite normal map bends the light (0 = flat)."),
    ("look.pixelhd.light.radius_scale", 1.0, 0.25, 3.0, "x", "Scale on every light radius."),
    ("look.pixelhd.light.rim", 0.25, 0.0, 1.0, "", "Rim accent on the edge of lit sprites."),
];

/// Sub-pixel camera smoothing: name, default, description.
pub const CAMERA_SUBPIXEL: (&str, bool, &str) = (
    "look.pixelhd.camera_subpixel",
    true,
    "Carry the camera remainder below one grid pixel into the scaled frame, so motion is not stepped.",
);
/// Particles drawn at native (device) resolution: name, default, description.
pub const PARTICLE_NATIVE: (&str, bool, &str) = (
    "look.pixelhd.particle_native",
    true,
    "Draw particles at full device resolution instead of on the 480x270 grid.",
);
/// Whether the optional 2D lights are wanted (the `lights2d` hook): name,
/// default, description.
pub const LIGHTS_ON: (&str, bool, &str) = (
    "look.pixelhd.light.enabled",
    true,
    "Ask for the optional normal-mapped 2D lights (needs the lights2d effect and a tier that allows it).",
);

/// One sRGB byte (0..=255) to linear 0..1.
pub fn srgb_to_linear(v: u32) -> f32 {
    let c = v.min(255) as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// An sRGB `0xRRGGBB` colour as linear RGBA.
pub fn linear(rgb: u32) -> [f32; 4] {
    let ch = |shift: u32| srgb_to_linear((rgb >> shift) & 0xFF);
    [ch(16), ch(8), ch(0), 1.0]
}

/// The HD pixel look.
#[derive(Debug, Default, Clone, Copy)]
pub struct PixelHdLook;

impl LookPlugin for PixelHdLook {
    fn name(&self) -> &'static str {
        "pixelhd"
    }

    /// Catalog `cost_points` 2.
    fn points(&self) -> u32 {
        2
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::TWO_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        for (name, shades) in RAMPS {
            let colors = shades.map(linear);
            look.palette(&format!("ramp.{name}"), &colors);
        }
        let sky = SKY.map(linear);
        look.palette("sky", &sky);
        look.palette("light", &LIGHTS.map(linear));

        for (name, d, min, max, unit, desc) in STYLE.into_iter().chain(LIGHT_STYLE) {
            look.param_f32(ParamDef::f32(name, d).range(min, max).unit(unit).describe(desc).bake(BakeTarget::Look))?;
        }
        for (name, d, desc) in [CAMERA_SUBPIXEL, PARTICLE_NATIVE, LIGHTS_ON] {
            look.param_bool(ParamDef::bool(name, d).describe(desc).bake(BakeTarget::Look))?;
        }
        look.param_i32(
            ParamDef::i32("look.pixelhd.internal_width", INTERNAL_SIZE[0])
                .range(160.0, 1920.0)
                .unit("px")
                .describe("Internal grid width in art pixels (catalog: 480).")
                .bake(BakeTarget::Look),
        )?;
        look.param_i32(
            ParamDef::i32("look.pixelhd.internal_height", INTERNAL_SIZE[1])
                .range(90.0, 1080.0)
                .unit("px")
                .describe("Internal grid height in art pixels (catalog: 270).")
                .bake(BakeTarget::Look),
        )?;

        look.shader_slot("tonemap", TONEMAP_BODY)?;
        // Values tuned live and written by bake.export; applied at start.
        look.baked_files(BAKED);

        // Effects the look prefers on (the tier budget still decides; a
        // look can only veto an effect, never force one).
        look.post_effect("bloom", true);
        look.post_effect("particles", true);
        look.post_effect("juice", true);
        look.post_effect("shake", true);
        look.post_effect("vignette", true);
        // The optional 2D lights (S5-R1, evani-fx `lights2d`): preferred on,
        // and the look's light values become the effect's defaults
        // (`look.pixelhd.light.<x>` -> `fx.lights2d.<x>`; ignored with a
        // note in a game built without the effect).
        look.post_effect(LIGHTS_2D_EFFECT, true);
        for (name, d, ..) in LIGHT_STYLE {
            if let Some(key) = name.strip_prefix(LIGHT_PREFIX) {
                look.post_param(&format!("fx.{LIGHTS_2D_EFFECT}.{key}"), json!(d));
            }
        }
        // Depth-edge ink is for 3D.
        look.post_effect("outline", false);

        look.post_param("render.pixel_art", json!(true));
        look.post_param("render.exposure", json!(1.0));
        look.post_param("render.saturation", json!(1.08));
        look.post_param("render.clear_color", json!([sky[0][0], sky[0][1], sky[0][2]]));
        // Only bright light accents glow; painted pixels stay crisp. Tuned
        // only when the game builds bloom, so no "not in this build" note.
        #[cfg(feature = "bloom")]
        {
            look.post_param("fx.bloom.threshold", json!(1.0));
            look.post_param("fx.bloom.intensity", json!(0.35));
        }
        Ok(())
    }
}
