//! `evani-look-photoreal`: "Gritty photoreal" (catalog look `photoreal`, 5
//! points, 3D, full look from the High tier): desaturated documentary
//! realism, grime and wear. Chosen for Hexagon, kit 006's demo
//! (docs/HEXAGON_PLAN.md, section 5 question 2).
//!
//! What this scaffold provides (S5-L3):
//! * Palette `photoreal` and the metal-roughness materials of [`MATERIALS`]
//!   (weathered concrete, bare and worn steel, team paints, grime, lamps and
//!   a muzzle flash that bloom catches).
//! * Shader slots: `tonemap` = a filmic stage ([`tonemap`]) followed by the
//!   look's colour-grade slot ([`grade`]), both tunable through
//!   `look.photoreal.*` params; `fog` = exponential height haze, the
//!   look's cheap atmosphere on every tier.
//! * Post settings: wants `shadows` and `bloom` (the tier budget decides),
//!   a hazy overcast sky and a soft neutral sun; render saturation stays at
//!   1 because the grade does the desaturation.
//!
//! What it expects from later pieces (not built here): the catalog's
//! render rules ask for full PBR with detail maps, grime decals, a
//! photographic grade, volumetric atmosphere and high-resolution KTX2
//! textures streamed by distance. Cascaded shadows, image-based lighting,
//! clustered point lights, the `decals`, `volumetric` and `grade` (LUT)
//! effects and metal-roughness / occlusion / detail map sampling come from
//! the render-path and effect pieces of Stage 5; see the crate README.

use evani_core::effects::StyleClasses;
use evani_core::plugin::{LookContext, LookPlugin, MaterialDesc, PluginError};
use serde_json::json;

pub mod grade;
pub mod tonemap;

pub use grade::{GRADE_BEGIN, GRADE_END, GRADE_HOOK, GRADE_SOURCES, GradeSettings};
pub use tonemap::FilmicSettings;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Catalog `cost_points`.
pub const COST_POINTS: u32 = 5;

/// Catalog `full_look_from_tier`: below it a game should drop the look's
/// optional extras (detail maps, grime decals, volumetric atmosphere); the
/// tonemap, grade and haze are cheap and stay on every tier.
pub const FULL_LOOK_FROM_TIER: &str = "high";

/// Height haze: none within 25 m, thickening with distance, thinner with
/// height (scale height about 35 m), capped at 70 % so far walls stay
/// readable. The forward pass fades towards `render.clear_color`.
pub const FOG_BODY: &str = "    // evani-look-photoreal: exponential height haze\n    let d = max(distance - 25.0, 0.0);\n    let density = 0.006 * exp(-max(height, 0.0) / 35.0);\n    let haze = 1.0 - exp(-d * density);\n    return min(haze, 0.7);";

/// Palette `photoreal` (linear RGBA): the documentary swatches.
pub const PALETTE: [[f32; 4]; 8] = [
    [0.330, 0.320, 0.300, 1.0], // poured concrete
    [0.140, 0.135, 0.128, 1.0], // wet concrete
    [0.520, 0.530, 0.540, 1.0], // bare steel
    [0.090, 0.085, 0.075, 1.0], // grime
    [0.060, 0.110, 0.210, 1.0], // team blue, faded
    [0.420, 0.150, 0.040, 1.0], // team orange, faded
    [0.110, 0.120, 0.085, 1.0], // olive drab
    [0.160, 0.125, 0.090, 1.0], // dust
];

/// One material row: name, linear base RGBA, HDR emissive, roughness,
/// metallic, unlit.
pub type MaterialRow = (&'static str, [f32; 4], [f32; 3], f32, f32, bool);

/// The look's materials, in order (index = material slot in the renderer).
pub const MATERIALS: [MaterialRow; 13] = [
    ("concrete", PALETTE[0], [0.0; 3], 0.92, 0.0, false),
    ("concrete_dark", PALETTE[1], [0.0; 3], 0.95, 0.0, false),
    ("steel", PALETTE[2], [0.0; 3], 0.40, 1.0, false),
    ("steel_worn", [0.360, 0.350, 0.330, 1.0], [0.0; 3], 0.62, 0.8, false),
    ("paint_blue", PALETTE[4], [0.0; 3], 0.70, 0.0, false),
    ("paint_orange", PALETTE[5], [0.0; 3], 0.70, 0.0, false),
    ("cloth", PALETTE[6], [0.0; 3], 0.90, 0.0, false),
    ("rubber", [0.030, 0.030, 0.032, 1.0], [0.0; 3], 0.88, 0.0, false),
    ("dirt", PALETTE[7], [0.0; 3], 1.00, 0.0, false),
    ("grime", PALETTE[3], [0.0; 3], 0.97, 0.0, false),
    ("scorch", [0.020, 0.018, 0.016, 1.0], [0.0; 3], 1.00, 0.0, false),
    // Caged work lamp: warm, well over the bloom threshold.
    ("lamp", [1.000, 0.880, 0.700, 1.0], [6.0, 5.0, 3.6], 0.5, 0.0, true),
    // Muzzle flash: hot orange core for a frame or two. Just over the bloom
    // threshold (3): at 14 it filled the first-person view with white.
    ("muzzle_flash", [1.000, 0.600, 0.200, 1.0], [5.0, 2.8, 0.9], 0.5, 0.0, true),
];

/// The photoreal look. `PhotorealLook::default()` uses the look's own
/// filmic and grade settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhotorealLook {
    filmic: FilmicSettings,
    grade: GradeSettings,
    custom_grade: Option<&'static str>,
}

impl PhotorealLook {
    /// Start-up values for the filmic stage (still tunable live).
    pub fn with_filmic(mut self, filmic: FilmicSettings) -> Self {
        self.filmic = filmic;
        self
    }

    /// Start-up values for the look's grade (still tunable live).
    pub fn with_grade(mut self, grade: GradeSettings) -> Self {
        self.grade = grade;
        self
    }

    /// Replace the look's grade with a game's own grade stage: WGSL
    /// statements that read and write `var c: vec3<f32>` (display-referred
    /// linear, 0..1). Selects grade source `custom` at start.
    pub fn with_grade_body(mut self, body: &'static str) -> Self {
        self.custom_grade = Some(body);
        self
    }
}

impl LookPlugin for PhotorealLook {
    fn name(&self) -> &'static str {
        "photoreal"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// Catalog `cost_points` 5.
    fn points(&self) -> u32 {
        COST_POINTS
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        look.palette("photoreal", &PALETTE);
        for (name, base, emissive, roughness, metallic, unlit) in MATERIALS {
            let mut m = MaterialDesc::new(name, base);
            m.emissive = emissive;
            m.roughness = roughness;
            m.metallic = metallic;
            m.unlit = unlit;
            look.material(m);
        }

        let source = if self.custom_grade.is_some() { 1 } else { 0 };
        tonemap::install(look, self.filmic, self.grade, source, self.custom_grade)?;
        look.shader_slot("fog", FOG_BODY)?;
        // Values tuned live and written by bake.export; applied at start.
        look.baked_files(BAKED);

        look.post_effect("shadows", true);
        look.post_effect("bloom", true);
        // Overcast documentary daylight: a hazy grey-blue sky, cool fill
        // and a soft, nearly neutral sun high to one side.
        look.post_param("render.clear_color", json!([0.40, 0.43, 0.46]));
        // Overcast sky fill: walls out of the sun stay readable grey
        // concrete (at 0.10 they went near-black).
        look.post_param("render.ambient", json!([0.22, 0.24, 0.27]));
        look.post_param("render.light.direction", json!([0.35, 0.9, 0.25]));
        look.post_param("render.light.color", json!([1.0, 0.96, 0.9]));
        look.post_param("render.light.intensity", json!(3.0));
        look.post_param("render.exposure", json!(1.0));
        // Only lamps, flashes and blasts glow.
        look.post_param("fx.bloom.threshold", json!(3.0));
        Ok(())
    }
}

pub mod daylight;
