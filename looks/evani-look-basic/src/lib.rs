//! `evani-look-basic`: the smallest real look. It proves the [`LookPlugin`]
//! interface end to end and gives `bake.export` a look crate to write into.
//!
//! * Palette `basic` and materials `ground`, `accent`, `glow` (warm orange,
//!   unlit, the bloom source), `stone` (pale warm stone) and `slate` (blue
//!   grey) for props and scenery.
//! * Shader-slot bodies, validated as WGSL by the native tests: `tonemap`
//!   (Reinhard on luminance, gentler than the engine's ACES default) and
//!   `fog` (a light distance haze in the sky colour from 60 m out, never
//!   more than 55 %, so far hills fade without vanishing).
//! * Param (owner `look.basic`): `look.basic.style`.
//! * Post settings: wants bloom and vignette on (the tier budget still
//!   decides) and overrides the renderer's own defaults for
//!   `render.saturation`, `render.ambient`, `render.clear_color` (sky) and
//!   `fx.bloom.threshold`. Those values live in GPU uniforms owned by the
//!   renderer, so the look sets them instead of registering copies.
//! * Generator `checker`: a deterministic checkerboard texture.

use evani_core::effects::StyleClasses;
use evani_core::params::ParamDef;
use evani_core::plugin::{
    GenRequest, GeneratedAsset, LookContext, LookPlugin, MaterialDesc, PluginError,
};
use evani_core::rng::Pcg32;
use serde_json::json;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// This look's tonemap: Reinhard on luminance, keeps hue, soft shoulder.
pub const TONEMAP_BODY: &str = "    // evani-look-basic: Reinhard on luminance (keeps hue)\n    let l = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));\n    let mapped = l / (1.0 + l);\n    let scale = select(0.0, mapped / l, l > 0.0001);\n    return clamp(color * scale, vec3<f32>(0.0), vec3<f32>(1.0));";

/// Distance haze: none within 60 m, then thickening slowly, thinner higher
/// up, capped at 55 %.
pub const FOG_BODY: &str = "    // evani-look-basic: light distance haze, thinner higher up\n    let d = max(distance - 60.0, 0.0);\n    let haze = 1.0 - exp(-d * 0.003);\n    let thin = clamp(1.0 - max(height, 0.0) / 150.0, 0.4, 1.0);\n    return min(haze * thin, 0.55);";

/// Pale warm stone, `#e9e4da` (linear RGBA): terrace tops, column caps,
/// the rings round the orbs.
pub const STONE: [f32; 4] = [0.815, 0.776, 0.701, 1.0];
/// Blue-grey slate, `#56606e` (linear RGBA): the palette dark lifted so lit
/// faces keep their colour.
pub const SLATE: [f32; 4] = [0.093, 0.117, 0.155, 1.0];

/// The `glow` material's base (linear RGBA): deep orange. Unlit, so the
/// screen colour is base + [`GLOW_EMISSIVE`] through the tonemap.
pub const GLOW_BASE: [f32; 4] = [1.0, 0.15, 0.02, 1.0];
/// The `glow` material's emissive. Base + emissive is about [6.0, 0.87,
/// 0.04]: luminance about 1.9, over the 1.5 bloom threshold, and the
/// luminance tonemap clips red first, so the screen shows warm orange,
/// about `#ff961e`, not the lemon yellow a greener mix gives.
pub const GLOW_EMISSIVE: [f32; 3] = [5.0, 0.72, 0.02];

/// Palette `basic` (linear RGBA).
pub const PALETTE: [[f32; 4]; 5] = [
    [0.050, 0.060, 0.080, 1.0],
    [0.180, 0.420, 0.300, 1.0],
    [0.900, 0.550, 0.150, 1.0],
    [0.300, 0.550, 0.900, 1.0],
    [0.950, 0.920, 0.880, 1.0],
];

/// The basic look.
#[derive(Debug, Default, Clone, Copy)]
pub struct BasicLook;

impl LookPlugin for BasicLook {
    fn name(&self) -> &'static str {
        "basic"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// The look itself is cheap: lit flat colours, no extra passes.
    fn points(&self) -> u32 {
        1
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        look.palette("basic", &PALETTE);
        look.material(MaterialDesc::new("ground", PALETTE[1]));
        look.material(MaterialDesc::new("accent", PALETTE[3]));
        let mut glow = MaterialDesc::new("glow", GLOW_BASE);
        glow.emissive = GLOW_EMISSIVE;
        glow.unlit = true;
        look.material(glow);
        look.material(MaterialDesc::new("stone", STONE));
        look.material(MaterialDesc::new("slate", SLATE));

        look.shader_slot("tonemap", TONEMAP_BODY)?;
        look.shader_slot("fog", FOG_BODY)?;
        // Values tuned live and written by bake.export (params.json,
        // policy.json, slots/*.wgsl in this crate); applied at start.
        look.baked_files(BAKED);

        look.param_enum(
            ParamDef::enumeration("look.basic.style", &["flat", "soft"], 1)
                .describe("Shading style: flat facets or soft normals."),
        )?;

        look.post_effect("bloom", true);
        look.post_effect("vignette", true);
        look.post_param("fx.bloom.threshold", json!(1.1));
        look.post_param("render.saturation", json!(1.05));
        look.post_param("render.ambient", json!([0.08, 0.09, 0.12]));
        look.post_param("render.clear_color", json!([0.35, 0.55, 0.85]));

        look.generator("checker", checker);
        Ok(())
    }
}

/// Checkerboard in two palette colours with a little seeded noise.
/// `params.cells` (default 8) sets the number of cells per edge.
pub fn checker(req: &GenRequest, rng: &mut Pcg32) -> GeneratedAsset {
    let size = req.size.clamp(1, 1024);
    let cells = req.params.get("cells").copied().unwrap_or(8.0).max(1.0) as u32;
    let cell = (size / cells).max(1);
    let to8 = |c: f32| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    let mut rgba8 = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let odd = ((x / cell) + (y / cell)) % 2 == 1;
            let base = if odd { PALETTE[1] } else { PALETTE[4] };
            let n = rng.range_f32(-0.02, 0.02);
            rgba8.extend_from_slice(&[to8(base[0] + n), to8(base[1] + n), to8(base[2] + n), 255]);
        }
    }
    GeneratedAsset::Texture {
        width: size,
        height: size,
        rgba8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::ParamRegistry;
    use evani_core::plugin::LookData;
    use evani_core::slots::ShaderSlots;

    #[test]
    fn registers_tunables() {
        let mut a = App::new("t").with_look(BasicLook);
        a.start();
        let reg = a.world.res::<ParamRegistry>();
        let id = reg.id("look.basic.style").unwrap();
        assert_eq!(reg.def(id).unwrap().owner, "look.basic");
        // Without evani-render in the app, the render.* overrides are
        // ignored with a note (the params are compiled out), not an error.
        assert!(reg.id("look.basic.saturation").is_none());
        drop(reg);
        let d = a.world.res::<LookData>();
        assert_eq!(d.name, "basic");
        assert_eq!(d.points, 1);
        assert!(d.palette("basic").is_some());
        assert!(d.material("glow").is_some());
        assert!(d.post_effects.contains(&("bloom".to_string(), true)));
    }

    #[test]
    fn tonemap_slot_is_valid_wgsl() {
        let mut a = App::new("t").with_look(BasicLook);
        a.start();
        let slots = a.world.res::<ShaderSlots>();
        let id = slots.id("tonemap").unwrap();
        let mut src = slots.wgsl_function(id).unwrap();
        assert!(src.contains("Reinhard"), "look body not installed");
        src.push_str(
            "\n@fragment fn fs_main() -> @location(0) vec4<f32> {\n    return vec4<f32>(slot_tonemap(vec3<f32>(2.0, 1.0, 0.5)), 1.0);\n}\n",
        );
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn fog_slot_is_valid_wgsl_and_capped() {
        let mut a = App::new("t").with_look(BasicLook);
        a.start();
        let d = a.world.res::<LookData>();
        assert!(d.material("stone").is_some() && d.material("slate").is_some());
        drop(d);
        let slots = a.world.res::<ShaderSlots>();
        let id = slots.id("fog").unwrap();
        let mut src = slots.wgsl_function(id).unwrap();
        assert!(src.contains("evani-look-basic"), "look body not installed");
        assert!(FOG_BODY.contains("0.55"), "the haze cap");
        src.push_str(
            "\n@fragment fn fs_main() -> @location(0) vec4<f32> {\n    return vec4<f32>(slot_fog(250.0, 12.0), 0.0, 0.0, 1.0);\n}\n",
        );
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    /// The glow reads warm orange on screen: the CPU twin of the unlit
    /// path (base + emissive) and [`TONEMAP_BODY`], then sRGB.
    #[test]
    fn glow_reads_orange_and_blooms() {
        let c: Vec<f32> = (0..3).map(|i| GLOW_BASE[i] + GLOW_EMISSIVE[i]).collect();
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        assert!(l > 1.5, "over the baked bloom threshold: {l}");
        let srgb: Vec<f32> = c
            .iter()
            .map(|v| (v / (1.0 + l)).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0)
            .collect();
        assert!(srgb[0] > 240.0, "red saturates: {srgb:?}");
        assert!((120.0..170.0).contains(&srgb[1]), "orange, not yellow: {srgb:?}");
        assert!(srgb[2] < 60.0, "{srgb:?}");
    }

    #[test]
    fn checker_is_deterministic() {
        let a = App::new("t").with_look(BasicLook);
        let req = GenRequest {
            name: "checker".into(),
            size: 16,
            seed: 7,
            params: Default::default(),
        };
        let d = a.world.res::<LookData>();
        let one = d.generate(&req).unwrap();
        let two = d.generate(&req).unwrap();
        assert_eq!(one, two);
        let GeneratedAsset::Texture { width, rgba8, .. } = one else {
            panic!("expected a texture");
        };
        assert_eq!(width, 16);
        assert_eq!(rgba8.len(), 16 * 16 * 4);
    }
}
