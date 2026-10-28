//! `evani-look-toon`: hard-edged lighting bands, a saturated candy palette
//! and ink outlines, like an animated film (catalog look `toon`, 2 points,
//! 3D). Built for the kart racer (kit 001) but usable by any 3D kit.
//!
//! * Palette `toon` and the 16 materials of [`MATERIALS`], in that order
//!   (kits look them up by name). `item_box`, `boost_flame` and `spark` are
//!   unlit with HDR emissive so bloom catches them; `kart_body` and `spark`
//!   have bright bases so a kit's instance tint (which may exceed 1) reads.
//! * Shader slots: `toon_ramp` (three bands with soft edges), `tonemap`
//!   (gentle filmic that keeps saturation) and `fog` (light distance haze in
//!   the sky colour; the forward pass fades towards `render.clear_color`).
//!   Slot bodies are spliced into the renderer's shaders and see only their
//!   arguments, so the band thresholds are constants in the body (tune them
//!   live through the slot itself; `bake.export` writes `slots/*.wgsl`).
//! * Post settings: wants `outline` and `bloom` on (the tier budget still
//!   decides) and sets `render.saturation`, `render.clear_color` (sky),
//!   `render.ambient`, warm `render.light.*` and the bloom threshold so only
//!   the emissive materials glow.

use evani_core::effects::StyleClasses;
use evani_core::plugin::{LookContext, LookPlugin, MaterialDesc, PluginError};
use serde_json::json;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Diffuse ramp: shadow 0.25, mid 0.6, lit 1.0. Band edges at N.L 0.02
/// (`band_low`) and 0.45 (`band_high`), each softened over only +-0.025 so
/// the three bands read as flat, hard-edged shapes.
pub const TOON_RAMP_BODY: &str = "    // evani-look-toon: three hard bands (shadow, mid, lit)\n    let band_low = 0.02;\n    let band_high = 0.45;\n    let softness = 0.025;\n    let low = smoothstep(band_low - softness, band_low + softness, n_dot_l);\n    let high = smoothstep(band_high - softness, band_high + softness, n_dot_l);\n    return 0.25 + 0.35 * low + 0.4 * high;";

/// Filmic curve (ACES fit) blended between per-channel and luminance-only
/// mapping: per-channel lets hot emissives roll to white, luminance-only
/// keeps the candy colours saturated.
pub const TONEMAP_BODY: &str = "    // evani-look-toon: gentle filmic, keeps saturation\n    let c = max(color, vec3<f32>(0.0));\n    let per_channel = (c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14);\n    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));\n    let lm = (l * (2.51 * l + 0.03)) / (l * (2.43 * l + 0.59) + 0.14);\n    let by_luminance = c * select(0.0, lm / l, l > 0.0001);\n    return clamp(mix(per_channel, by_luminance, 0.6), vec3<f32>(0.0), vec3<f32>(1.0));";

/// Haze from 40 m out, thinner higher up, never more than 60 %, so distant
/// mountains keep their colour.
pub const FOG_BODY: &str = "    // evani-look-toon: light distance haze, thinner higher up\n    let d = max(distance - 40.0, 0.0);\n    let haze = 1.0 - exp(-d * 0.004);\n    let thin = clamp(1.0 - max(height, 0.0) / 120.0, 0.35, 1.0);\n    return min(haze * thin, 0.6);";

/// Rim light: a thin cool-white edge on the silhouette's side away from the
/// sun (N.L under 0.3), about +20 % value, only within 60 m of the camera so
/// distant hills and flat ground seen edge-on stay clean.
pub const RIM_BODY: &str = "    // evani-look-toon: thin cool-white rim on the shadow side, near the camera\n    let edge = smoothstep(0.7, 0.8, 1.0 - max(n_dot_v, 0.0));\n    let away = 1.0 - smoothstep(-0.1, 0.3, n_dot_l);\n    let near = 1.0 - smoothstep(30.0, 60.0, distance);\n    return (base * 0.45 + vec3<f32>(0.2, 0.24, 0.3)) * (edge * away * near);";

/// The shadow tint: a soft purple (sRGB `#a98bf0`, the look bible's shadow
/// colour, in linear RGB) that cast shadows lean towards instead of going
/// grey, and how far. The look sets `render.shadow_tint` and
/// `render.shadow_tint_strength` to these; both are tunable live.
pub const SHADOW_TINT: [f32; 3] = [0.397, 0.258, 0.871];
/// How far shadows lean towards [`SHADOW_TINT`] (0 = not at all).
pub const SHADOW_TINT_STRENGTH: f32 = 0.65;

/// Palette `toon` (linear RGBA): the candy swatches.
pub const PALETTE: [[f32; 4]; 8] = [
    [0.880, 0.080, 0.150, 1.0], // cherry
    [0.950, 0.450, 0.620, 1.0], // bubblegum
    [0.350, 0.850, 0.620, 1.0], // mint
    [0.620, 0.480, 0.920, 1.0], // lilac
    [0.200, 0.640, 0.120, 1.0], // apple
    [1.000, 0.700, 0.080, 1.0], // tangerine
    [0.250, 0.780, 1.000, 1.0], // sky candy
    [0.930, 0.930, 0.960, 1.0], // sugar
];

/// One material row: name, linear base RGBA, HDR emissive, unlit.
pub type MaterialRow = (&'static str, [f32; 4], [f32; 3], bool);

/// The look's materials, in order (index = material slot in the renderer).
pub const MATERIALS: [MaterialRow; 16] = [
    ("road", [0.160, 0.150, 0.210, 1.0], [0.0; 3], false),
    ("kerb_red", PALETTE[0], [0.0; 3], false),
    ("kerb_white", PALETTE[7], [0.0; 3], false),
    ("grass", PALETTE[4], [0.0; 3], false),
    ("mountain_pink", PALETTE[1], [0.0; 3], false),
    ("mountain_mint", PALETTE[2], [0.0; 3], false),
    ("mountain_lilac", PALETTE[3], [0.0; 3], false),
    ("tree", [0.090, 0.520, 0.240, 1.0], [0.0; 3], false),
    ("trunk", [0.420, 0.200, 0.090, 1.0], [0.0; 3], false),
    // Near-white: the kit tints it per kart.
    ("kart_body", [0.950, 0.950, 0.950, 1.0], [0.0; 3], false),
    ("wheel", [0.050, 0.050, 0.070, 1.0], [0.0; 3], false),
    // Cyan base with a magenta glow: reads as a rainbow-ish shimmer.
    ("item_box", [0.500, 1.000, 1.000, 1.0], [2.4, 1.4, 2.8], true),
    ("boost_flame", [1.000, 0.500, 0.080, 1.0], [4.0, 2.0, 0.3], true),
    // HDR white-blue base (the kit's tint multiplies it, per drift tier)
    // plus a smaller untinted glow.
    ("spark", [1.200, 1.400, 1.800, 1.0], [0.6, 0.8, 1.2], true),
    ("goo", [0.450, 0.900, 0.150, 1.0], [0.0; 3], false),
    ("bomb", [0.080, 0.070, 0.120, 1.0], [0.0; 3], false),
];

/// The toon look.
#[derive(Debug, Default, Clone, Copy)]
pub struct ToonLook;

impl LookPlugin for ToonLook {
    fn name(&self) -> &'static str {
        "toon"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// Catalog `cost_points` 2 (the outlines are a separate effect).
    fn points(&self) -> u32 {
        2
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        look.palette("toon", &PALETTE);
        for (name, base, emissive, unlit) in MATERIALS {
            let mut m = MaterialDesc::new(name, base);
            m.emissive = emissive;
            m.unlit = unlit;
            m.roughness = 0.8;
            look.material(m);
        }

        look.shader_slot("toon_ramp", TOON_RAMP_BODY)?;
        look.shader_slot("tonemap", TONEMAP_BODY)?;
        look.shader_slot("fog", FOG_BODY)?;
        look.shader_slot("rim", RIM_BODY)?; #[cfg(feature = "relief")] relief::install(look)?;
        // Values tuned live and written by bake.export; applied at start.
        look.baked_files(BAKED);

        look.post_effect("outline", true);
        look.post_effect("bloom", true);
        look.post_param("render.saturation", json!(1.15));
        look.post_param("render.clear_color", json!([0.30, 0.62, 0.98]));
        look.post_param("render.ambient", json!([0.24, 0.26, 0.34]));
        // A mid-afternoon sun from the side and a little ahead: every kart,
        // prop and hill splits into a lit, a mid and a shadow side, and the
        // shadows fall clearly to one side of the road.
        look.post_param("render.light.direction", json!([0.62, 0.85, 0.28]));
        look.post_param("render.light.color", json!([1.0, 0.92, 0.78]));
        look.post_param("render.light.intensity", json!(1.6));
        // Shadows lean soft purple, never near-black (look bible).
        look.post_param("render.shadow_tint", json!(SHADOW_TINT));
        look.post_param("render.shadow_tint_strength", json!(SHADOW_TINT_STRENGTH));
        // Lit white kerbs stay under the threshold; emissives glow.
        look.post_param("fx.bloom.threshold", json!(2.0));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::LookData;
    use evani_core::slots::ShaderSlots;

    const NAMES: [&str; 16] = [
        "road",
        "kerb_red",
        "kerb_white",
        "grass",
        "mountain_pink",
        "mountain_mint",
        "mountain_lilac",
        "tree",
        "trunk",
        "kart_body",
        "wheel",
        "item_box",
        "boost_flame",
        "spark",
        "goo",
        "bomb",
    ];

    #[test]
    fn registers_materials_points_and_effects() {
        let mut a = App::new("t").with_look(ToonLook);
        a.start();
        let d = a.world.res::<LookData>();
        assert_eq!(d.name, "toon");
        assert_eq!(d.points, 2);
        assert_eq!(d.styles, StyleClasses::THREE_D);
        assert!(d.palette("toon").is_some());
        let names: Vec<&str> = d.materials.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, NAMES);
        for glow in ["item_box", "boost_flame", "spark"] {
            let m = d.material(glow).unwrap();
            assert!(m.unlit, "{glow} unlit");
            assert!(m.emissive.iter().any(|&e| e > 1.0), "{glow} HDR emissive");
        }
        assert!(!d.material("kart_body").unwrap().unlit);
        assert!(d.post_effects.contains(&("outline".to_string(), true)));
        assert!(d.post_effects.contains(&("bloom".to_string(), true)));
    }

    #[test]
    fn shadow_tint_defaults_to_the_look_bibles_soft_purple() {
        let srgb = |h: u32| -> [f32; 3] {
            let c = |s: u32| {
                let v = ((h >> s) & 0xff) as f32 / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            [c(16), c(8), c(0)]
        };
        let want = srgb(0xa98bf0);
        for i in 0..3 {
            assert!((SHADOW_TINT[i] - want[i]).abs() < 0.002, "channel {i}: {} vs {}", SHADOW_TINT[i], want[i]);
        }
        // The bible names the same colour.
        let bible = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../kits/kart-racer/target/look-bible.md")).unwrap();
        assert!(bible.to_lowercase().contains("#a98bf0"), "look bible lists the shadow tint");
        // In range (the renderer registers the params; the kart kit's tests
        // check the running values).
        assert!((0.0..=1.0).contains(&SHADOW_TINT_STRENGTH) && SHADOW_TINT_STRENGTH > 0.0);
    }

    #[test]
    fn slot_bodies_are_valid_wgsl() {
        let mut a = App::new("t").with_look(ToonLook);
        a.start();
        let slots = a.world.res::<ShaderSlots>();
        for (slot, call, ret) in [
            ("toon_ramp", "slot_toon_ramp(0.3)", "f32"),
            ("tonemap", "slot_tonemap(vec3<f32>(2.0, 1.0, 0.5))", "vec3<f32>"),
            ("fog", "slot_fog(250.0, 12.0)", "f32"),
            ("rim", "slot_rim(0.2, -0.3, 12.0, vec3<f32>(1.0, 0.3, 0.5))", "vec3<f32>"),
        ] {
            let id = slots.id(slot).unwrap();
            let mut src = slots.wgsl_function(id).unwrap();
            assert!(src.contains("evani-look-toon"), "{slot}: look body not installed");
            let wrap = if ret == "f32" { format!("vec4<f32>({call})") } else { format!("vec4<f32>({call}, 1.0)") };
            src.push_str(&format!(
                "\n@fragment fn fs_main() -> @location(0) vec4<f32> {{\n    return {wrap};\n}}\n"
            ));
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{slot}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{slot}: {e:?}"));
        }
    }
}

#[cfg(feature = "relief")]
pub mod relief;
