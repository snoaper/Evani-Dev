//! `evani-look-stylizedpbr`: the stylised PBR look (catalog look
//! `stylizedpbr`, 3 points, 3D: "vibrant colours, chunky proportions, clean
//! physically based materials"; metal-roughness PBR, saturated albedo, strong
//! rim and ambient occlusion). Smooth light where `toon` has bands.
//!
//! Origin: ported from Chris Pearson's ChimpDuty project (branch
//! `chimp-model`, `handover/evani-dev/gaps/evani-look-stylizedpbr`), written
//! from scratch for Evani.Dev. XeGTAO, Bevy's SSAO and PCSS, three.js GTAO,
//! NVIDIA's shells-and-fins paper and Inigo Quilez's biplanar mapping article
//! were read for ideas only; no outside code or WGSL was copied. Where the
//! handover and `catalog/looks.json` differed, the catalog won (README.md
//! in this folder lists what changed).
//!
//! Render side: the engine half (lit model shader, rim, ambient occlusion,
//! image-based light) is the S5-R5 work and is not applied here. This crate
//! only exposes named hooks for it ([`hooks`]); `r5/look-surface.patch` is
//! the ChimpDuty reference patch for that work.
//!
//! * Slot bodies for the engine's slots: `toon_ramp` becomes a smooth
//!   wrapped diffuse (the slot takes it as is, so every forward mesh and the
//!   engine's own model shading go smooth with no engine change), `rim` a
//!   soft base-tinted rim, `tonemap` a filmic curve with a toe (authored
//!   blacks stay dark) and `fog` a light distance haze.
//! * Three slots it declares itself: `specular` (roughness/metal lobe),
//!   `surface_params` (direct occlusion, reflections, hemisphere and detail
//!   constants) and, with feature `pcss`, `pcss_params`.
//! * Model shading WGSL ([`MODEL_SURFACE_WGSL`]) that reads the glTF
//!   metallicRoughness and occlusion maps the engine binds nothing for yet.
//!   It needs the line-neutral evani-render hook in `look-surface.patch`
//!   (see `CORE_REQUESTS-entry.md`); until that lands, a renderer builds the
//!   module with [`lit_forward_template`] from public evani-render source
//!   (the look-lab demo does).
//! * Features, each strictly additive: `pcss` (soft shadows), `fur` (shells,
//!   fins, clump bake; [`fur`]), `biplanar` (tiling detail; [`detail`]) and
//!   `gtao` (asks for the evani-fx-gtao effect). Tier plans in [`tier`].
//! * Params `look.stylizedpbr.*` ([`PARAMS`]); a system rewrites the slot
//!   bodies when one changes, so they tune live like any slot.

use evani_core::ecs::{Stage, SystemDesc};
use evani_core::effects::StyleClasses;
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
#[cfg(any(feature = "biplanar", feature = "fur"))]
use evani_core::plugin::{GenRequest, GeneratedAsset};
use evani_core::plugin::{LookContext, LookPlugin, MaterialDesc, PluginError};
#[cfg(any(feature = "biplanar", feature = "fur"))]
use evani_core::rng::Pcg32;
use evani_core::slots::{ShaderSlots, SlotSignature};
use serde_json::json;

#[cfg(feature = "biplanar")]
pub mod detail;
#[cfg(feature = "fur")]
pub mod fur;
pub mod hooks; #[cfg(feature = "film")] pub mod film;
pub mod tier;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// The look's id (catalog `looks.json`, crate `evani-look-stylizedpbr`).
pub const NAME: &str = "stylizedpbr";
/// Owner string written into every slot body (tests look for it).
pub const BODY_TAG: &str = "evani-look-stylizedpbr";

// ---- slots ------------------------------------------------------------------------

/// Specular light from the sun for one pixel (multiplied by the light colour
/// and shadow by the caller). Zero by default, so a look that never sets it
/// shades as before.
pub const SPECULAR: SlotSignature = SlotSignature {
    name: "specular",
    params: &[("n_dot_h", "f32"), ("n_dot_l", "f32"), ("n_dot_v", "f32"), ("roughness", "f32"), ("metal", "f32"), ("base", "vec3<f32>")],
    ret: "vec3<f32>",
    description: "Specular reflection of the sun from N.H, N.L, N.V, roughness, metalness and the base colour (zero = none)",
    default_body: "    return vec3<f32>(0.0);",
};

/// Surface constants by index: 0 = (direct occlusion, reflection strength
/// on dielectrics, sky brightness seen in reflections as a multiple of the
/// ambient, unused), 1 = (detail strength, detail tile size m, unused,
/// unused), 2 = sky tint, 3 = ground tint (both multiply render.ambient).
pub const SURFACE_PARAMS: SlotSignature = SlotSignature {
    name: "surface_params",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Model surface constants by index: 0 occlusion/reflections, 1 detail, 2 sky tint, 3 ground tint",
    default_body: "    switch i {\n        case 0u: { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }\n        case 1u: { return vec4<f32>(0.0, 0.05, 0.0, 0.0); }\n        default: { return vec4<f32>(1.0, 1.0, 1.0, 0.0); }\n    }",
};

/// Soft-shadow constants: (penumbra metres per metre of blocker distance,
/// blocker search radius in texels, smallest and largest filter radius in
/// texels).
pub const PCSS_PARAMS: SlotSignature = SlotSignature {
    name: "pcss_params",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Soft shadows: penumbra per metre, search radius, min and max filter radius (texels)",
    default_body: "    return vec4<f32>(0.02, 6.0, 1.0, 12.0);",
};

// ---- params -----------------------------------------------------------------------

/// One param: name suffix, default, min, max, step, unit, description.
pub type ParamRow = (&'static str, f32, f32, f32, f32, &'static str, &'static str);

/// The look's params, registered as `look.stylizedpbr.<name>` in this order.
pub const PARAMS: [ParamRow; 12] = [
    ("wrap", 0.35, 0.0, 1.0, 0.01, "", "Wrapped diffuse: how far light creeps past the terminator (0 = plain N.L)"),
    ("spec", 1.0, 0.0, 2.0, 0.01, "", "Specular strength"),
    // Catalog: "strong rim" (the ChimpDuty handover had 0.3).
    ("rim", 0.55, 0.0, 1.0, 0.01, "", "Rim light strength"),
    ("rim_width", 0.4, 0.05, 1.0, 0.01, "", "Rim width (fraction of the edge falloff)"),
    ("ao_strength", 0.6, 0.0, 1.0, 0.01, "", "How much the occlusion map also darkens direct sunlight (ambient always uses it)"),
    ("env_spec", 0.5, 0.0, 1.0, 0.01, "", "Sky/ground reflections on smooth surfaces"),
    ("pcss_light_deg", 1.5, 0.1, 8.0, 0.1, "deg", "Sun's angular size for soft shadows (bigger = softer)"),
    ("pcss_search", 7.0, 1.0, 24.0, 0.5, "texels", "Soft-shadow blocker search radius"),
    ("fur_shells_max", 8.0, 0.0, 8.0, 1.0, "", "Most fur shells drawn on any tier (the tier may draw fewer)"),
    ("fur_fins", 1.0, 0.0, 1.0, 1.0, "", "Fur fins on silhouettes (1 on, 0 off)"),
    ("detail_strength", 0.85, 0.0, 1.0, 0.01, "", "Biplanar detail strength on flagged materials"),
    ("detail_scale", 0.02, 0.002, 0.2, 0.001, "m", "Biplanar detail tile size (materials may set their own)"),
];

/// Current values of [`PARAMS`], in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub wrap: f32,
    pub spec: f32,
    pub rim: f32,
    pub rim_width: f32,
    pub ao_strength: f32,
    pub env_spec: f32,
    pub pcss_light_deg: f32,
    pub pcss_search: f32,
    pub fur_shells_max: f32,
    pub fur_fins: f32,
    pub detail_strength: f32,
    pub detail_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        let mut v = [0.0; 12];
        for (i, row) in PARAMS.iter().enumerate() {
            v[i] = row.1;
        }
        Self::from_values(v)
    }
}

/// A WGSL float literal (always with a decimal point).
fn lit(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{v:.5}")
}

impl Settings {
    pub fn from_values(v: [f32; 12]) -> Self {
        Self {
            wrap: v[0],
            spec: v[1],
            rim: v[2],
            rim_width: v[3],
            ao_strength: v[4],
            env_spec: v[5],
            pcss_light_deg: v[6],
            pcss_search: v[7],
            fur_shells_max: v[8],
            fur_fins: v[9],
            detail_strength: v[10],
            detail_scale: v[11],
        }
    }

    /// Smooth wrapped diffuse: N.L wrapped by `wrap`, with a slightly
    /// softened toe so the terminator is a gradient, never a line.
    pub fn toon_ramp_body(&self) -> String {
        format!(
            "    // {BODY_TAG}: smooth wrapped diffuse\n    let w = {};\n    let x = clamp((n_dot_l + w) / (1.0 + w), 0.0, 1.0);\n    return 0.65 * x + 0.35 * x * x * (3.0 - 2.0 * x);",
            lit(self.wrap.clamp(0.0, 1.0))
        )
    }

    /// Soft rim: grows over the outer `rim_width` of the edge falloff,
    /// stronger on the side away from the sun, gone beyond 80 m.
    pub fn rim_body(&self) -> String {
        format!(
            "    // {BODY_TAG}: soft rim, stronger away from the sun\n    let e = 1.0 - clamp(n_dot_v, 0.0, 1.0);\n    let edge = smoothstep(1.0 - {}, 1.0, e);\n    let back = 0.45 + 0.55 * (1.0 - clamp(n_dot_l * 0.5 + 0.5, 0.0, 1.0));\n    let near = 1.0 - smoothstep(40.0, 80.0, distance);\n    return (base * 0.9 + vec3<f32>(0.10, 0.11, 0.13)) * (edge * back * near * {});",
            lit(self.rim_width.clamp(0.05, 1.0)),
            lit(self.rim.max(0.0))
        )
    }

    /// Neutral filmic curve, own rational fit solved for: slope 0.5 at
    /// black (a real toe, so blacks stay black), 0.18 -> 0.24, 1 -> 0.78,
    /// 4 -> 0.97 (long shoulder); part luminance-driven so bright colours
    /// keep their saturation instead of bleaching per channel.
    pub fn tonemap_body(&self) -> String {
        format!(
            "    // {BODY_TAG}: neutral filmic with a toe\n    let c = max(color, vec3<f32>(0.0));\n    let pc = (c * (12.03 * c + 0.5)) / (c * (11.6 * c + 3.46) + 1.0);\n    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));\n    let lm = (l * (12.03 * l + 0.5)) / (l * (11.6 * l + 3.46) + 1.0);\n    let bl = c * select(0.0, lm / l, l > 0.0001);\n    return clamp(mix(pc, bl, 0.3), vec3<f32>(0.0), vec3<f32>(1.0));"
        )
    }

    /// Light haze from 80 m out, thinner higher up, at most 35 %.
    pub fn fog_body(&self) -> String {
        format!(
            "    // {BODY_TAG}: light haze\n    let d = max(distance - 80.0, 0.0);\n    let haze = 1.0 - exp(-d * 0.0025);\n    let thin = clamp(1.0 - max(height, 0.0) / 150.0, 0.4, 1.0);\n    return min(haze * thin, 0.35);"
        )
    }

    /// Normalised microfacet lobe (GGX-shaped distribution, a light
    /// height-correlated visibility approximation) with Schlick-style
    /// Fresnel towards f0 = mix(0.04, base, metal). Includes N.L.
    pub fn specular_body(&self) -> String {
        format!(
            "    // {BODY_TAG}: roughness/metal specular\n    let a = max(roughness * roughness, 0.003);\n    let a2 = a * a;\n    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;\n    let ndf = a2 / (3.14159265 * d * d);\n    let k = a * 0.5;\n    let vis = 0.25 / max((n_dot_l * (1.0 - k) + k) * (n_dot_v * (1.0 - k) + k), 1e-4);\n    let f0 = mix(vec3<f32>(0.04), base, metal);\n    let e = 1.0 - clamp(n_dot_v, 0.0, 1.0);\n    let fr = f0 + (vec3<f32>(1.0) - f0) * (e * e * e * e * e);\n    return fr * min(ndf * vis, 64.0) * n_dot_l * {};",
            lit(self.spec.max(0.0))
        )
    }

    pub fn surface_params_body(&self) -> String {
        format!(
            "    // {BODY_TAG}: surface constants\n    switch i {{\n        case 0u: {{ return vec4<f32>({}, {}, 2.2, 0.0); }}\n        case 1u: {{ return vec4<f32>({}, {}, 0.0, 0.0); }}\n        case 2u: {{ return vec4<f32>(1.1, 1.08, 1.12, 0.0); }}\n        case 3u: {{ return vec4<f32>(0.55, 0.48, 0.42, 0.0); }}\n        default: {{ return vec4<f32>(0.0); }}\n    }}",
            lit(self.ao_strength.clamp(0.0, 1.0)),
            lit(self.env_spec.clamp(0.0, 1.0)),
            lit(self.detail_strength.clamp(0.0, 1.0)),
            lit(self.detail_scale.max(0.001)),
        )
    }

    /// Penumbra per metre = 2 tan(angle / 2) of the sun's angular size.
    pub fn pcss_params_body(&self) -> String {
        let per_m = 2.0 * (self.pcss_light_deg.clamp(0.05, 20.0).to_radians() * 0.5).tan();
        format!(
            "    // {BODY_TAG}: soft-shadow constants\n    return vec4<f32>({}, {}, 1.0, 14.0);",
            lit(per_m),
            lit(self.pcss_search.clamp(1.0, 32.0))
        )
    }

    /// Every slot body this look sets: (slot, body).
    pub fn bodies(&self) -> Vec<(&'static str, String)> {
        let mut v = vec![
            ("toon_ramp", self.toon_ramp_body()),
            ("rim", self.rim_body()),
            ("tonemap", self.tonemap_body()),
            ("fog", self.fog_body()),
            ("specular", self.specular_body()),
            ("surface_params", self.surface_params_body()),
            ("ibl_params", hooks::ibl_params_body()),
        ];
        if cfg!(feature = "pcss") {
            v.push(("pcss_params", self.pcss_params_body()));
        }
        v
    }
}

/// The slots the look declares (pcss_params only with feature `pcss`).
pub fn declared_slots() -> Vec<SlotSignature> {
    let mut v = vec![SPECULAR, SURFACE_PARAMS, hooks::IBL_PARAMS];
    if cfg!(feature = "pcss") {
        v.push(PCSS_PARAMS);
    }
    v
}

// ---- materials --------------------------------------------------------------------

/// Palette `stylizedpbr` (linear RGBA): vibrant, saturated swatches (catalog:
/// "vibrant colours", "saturated albedo"; the ChimpDuty handover's palette
/// was muted military tones for its own game).
pub const PALETTE: [[f32; 4]; 8] = [
    [0.300, 0.110, 0.040, 1.0], // warm fur brown
    [0.780, 0.380, 0.220, 1.0], // skin
    [0.060, 0.260, 0.700, 1.0], // fabric blue
    [0.520, 0.200, 0.060, 1.0], // leather tan
    [0.560, 0.580, 0.620, 1.0], // bright steel
    [0.420, 0.400, 0.360, 1.0], // stone
    [0.150, 0.520, 0.060, 1.0], // grass
    [0.900, 0.900, 0.880, 1.0], // white
];

/// One material row: name, linear base RGBA, roughness, metallic.
pub type MaterialRow = (&'static str, [f32; 4], f32, f32);

/// The look's materials (forward meshes look them up by name).
pub const MATERIALS: [MaterialRow; 8] = [
    ("fur", PALETTE[0], 0.8, 0.0),
    ("skin", PALETTE[1], 0.55, 0.0),
    ("fabric", PALETTE[2], 0.85, 0.0),
    ("leather", PALETTE[3], 0.6, 0.0),
    ("metal", PALETTE[4], 0.35, 0.8),
    ("stone", PALETTE[5], 0.9, 0.0),
    ("grass", PALETTE[6], 0.95, 0.0),
    ("glass", PALETTE[7], 0.08, 0.0),
];

// ---- the look ---------------------------------------------------------------------

/// The stylised-realistic look.
#[derive(Debug, Default, Clone, Copy)]
pub struct StylizedPbrLook;

impl LookPlugin for StylizedPbrLook {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// Catalog `cost_points` 3 (GTAO is a separate effect with its own cost).
    fn points(&self) -> u32 {
        3
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        look.palette(NAME, &PALETTE); #[cfg(feature = "film")] let film = film::take(look);
        for (name, base, roughness, metallic) in MATERIALS {
            let mut m = MaterialDesc::new(name, base);
            m.roughness = roughness;
            m.metallic = metallic;
            look.material(m);
        }

        // Params first, so the bodies start from their (possibly baked) values.
        let mut handles: Vec<F32Param> = Vec::with_capacity(PARAMS.len());
        for (suffix, default, min, max, step, unit, description) in PARAMS {
            #[cfg(not(feature = "film"))] let def = ParamDef::f32(&format!("look.{NAME}.{suffix}"), default).range(min, max).step(step).unit(unit).describe(description); #[cfg(feature = "film")] let def = ParamDef::f32(&format!("look.{NAME}.{suffix}"), film::start(film.as_ref(), suffix, default)).range(min, max).step(step).unit(unit).describe(description);
            handles.push(look.param_f32(def)?);
        } #[cfg(feature = "film")] let film_handles = film::register(look, film.as_ref())?;

        {
            let mut slots = look.world.res_mut::<ShaderSlots>();
            for sig in declared_slots() {
                slots.declare(sig);
            }
        }
        #[cfg(not(feature = "film"))] for (slot, body) in Settings::default().bodies() {
            look.shader_slot(slot, &body)?;
        } #[cfg(feature = "film")] for (slot, body) in film::start_bodies(film.as_ref()) { look.shader_slot(slot, &body)?; }
        // Values tuned live and written by bake.export; applied at start.
        look.baked_files(BAKED);

        // Live tuning: when any param changes, rewrite the bodies that differ.
        let mut seen = u64::MAX;
        look.system(SystemDesc::new("look.stylizedpbr.slots", Stage::Update, move |world| {
            let settings = {
                let Some(params) = world.try_res::<ParamRegistry>() else {
                    return;
                };
                if params.revision() == seen {
                    return;
                }
                seen = params.revision();
                read_settings(&params, &handles)
            }; #[cfg(feature = "film")] let settings = film::Tuned { base: settings, film: world.try_res::<ParamRegistry>().and_then(|p| film::read(&p, &film_handles)) };
            let Some(mut slots) = world.try_res_mut::<ShaderSlots>() else {
                return;
            };
            for (name, body) in settings.bodies() {
                let Some(id) = slots.id(name) else { continue };
                if slots.get(id).is_some_and(|d| d.body != body) {
                    // A structurally bad body cannot come from these templates;
                    // if one did, the previous body simply stays.
                    let _ = slots.set_body(id, &body); #[cfg(feature = "film")] let _ = world.try_res_mut::<evani_core::slots::SlotRebuilds>().map(|mut r| r.request_detached(&slots, name, &body, 0.0));
                }
            }
        }));

        // The tiling maps the engine's `look-surface` hook binds (generator
        // names from look-surface.patch: `surface.detail`, `surface.clump`).
        #[cfg(feature = "biplanar")]
        look.generator(DETAIL_GENERATOR, detail_generator);
        #[cfg(feature = "fur")]
        look.generator(CLUMP_GENERATOR, clump_generator);

        look.post_effect("shadows", true);
        look.post_effect("outline", false);
        look.post_effect("bloom", true);
        // Catalog: "strong ... ambient occlusion". The catalog effect is
        // `ssao` (the handover asked for `gtao`, its implementation's name);
        // the tier budget still decides, and Low and Lite keep the baked
        // occlusion map through the R5 lit shader.
        look.post_effect("ssao", true);
        // Catalog: "saturated albedo" (the handover had 1.08).
        look.post_param("render.saturation", json!(1.15));
        look.post_param("render.clear_color", json!([0.50, 0.64, 0.82]));
        look.post_param("render.ambient", json!([0.26, 0.27, 0.30]));
        look.post_param("render.light.direction", json!([0.45, 0.8, 0.4]));
        look.post_param("render.light.color", json!([1.0, 0.95, 0.88]));
        look.post_param("render.light.intensity", json!(2.4));
        // A faint cool lean in shadow, never purple.
        look.post_param("render.shadow_tint", json!([0.20, 0.24, 0.34]));
        look.post_param("render.shadow_tint_strength", json!(0.15));
        // Only where the game builds bloom (evani.toml `effects`; the fx
        // module registers before the look): a game without it, like Cinder
        // Ridge, would otherwise log "fx.bloom.threshold not in this build".
        if look.world.try_res::<ParamRegistry>().is_some_and(|r| r.id("fx.bloom.threshold").is_some()) {
            look.post_param("fx.bloom.threshold", json!(2.5));
        } #[cfg(feature = "film")] film::post(look, film.as_ref());
        Ok(())
    }
}

/// Generator name the engine's `look-surface` hook asks for the detail map.
pub const DETAIL_GENERATOR: &str = "surface.detail";
/// Generator name the engine's `look-surface` hook asks for the clump map.
pub const CLUMP_GENERATOR: &str = "surface.clump";

/// The weave detail map as a generated texture (linear data; `size` edge,
/// 16 threads, seed from the request; same request, same bytes).
#[cfg(feature = "biplanar")]
pub fn detail_generator(req: &GenRequest, _rng: &mut Pcg32) -> GeneratedAsset {
    let size = req.size.clamp(16, 1024);
    GeneratedAsset::Texture { width: size, height: size, rgba8: detail::weave_texture(size, 16, req.seed as u32 ^ 7) }
}

/// The fur clump map as a generated texture (linear data; 8 clumps per
/// side, seed from the request; same request, same bytes).
#[cfg(feature = "fur")]
pub fn clump_generator(req: &GenRequest, _rng: &mut Pcg32) -> GeneratedAsset {
    let size = req.size.clamp(16, 1024);
    GeneratedAsset::Texture { width: size, height: size, rgba8: fur::clump_texture(size, 8, req.seed as u32 ^ 11) }
}

/// The settings from the registry (defaults where a handle reads nothing).
pub fn read_settings(params: &ParamRegistry, handles: &[F32Param]) -> Settings {
    let mut v = [0.0; 12];
    for (i, row) in PARAMS.iter().enumerate() {
        v[i] = handles.get(i).map_or(row.1, |&h| params.f32(h));
    }
    Settings::from_values(v)
}

// ---- model surface uniform --------------------------------------------------------

/// Words in the `ModelSurface` uniform (group 1, binding 8).
pub const SURFACE_WORDS: usize = 16;

/// Fur numbers for one material (the `evani_fur` extras, tip in linear RGB).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FurSurface {
    pub length_m: f32,
    /// Strands per square centimetre.
    pub density: f32,
    pub shells: u32,
    pub clump_scale_m: f32,
    pub tip_linear: [f32; 3],
}

/// Biplanar detail on one material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailSurface {
    /// Tile size in metres (0 = the look's `detail_scale`).
    pub scale_m: f32,
    pub strength: f32,
}

/// What the lit model shader needs per material beyond the engine's
/// `ModelMaterial`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceDesc {
    pub metallic: f32,
    pub roughness: f32,
    pub occlusion_strength: f32,
    pub fur: Option<FurSurface>,
    pub detail: Option<DetailSurface>,
}

impl Default for SurfaceDesc {
    fn default() -> Self {
        Self { metallic: 0.0, roughness: 1.0, occlusion_strength: 1.0, fur: None, detail: None }
    }
}

impl SurfaceDesc {
    /// The uniform words (layout of `ModelSurface` in model_surface.wgsl).
    /// Word 6 holds the material's shell count; a renderer overwrites it
    /// with the shells it draws on the tier ([`tier::TierPlan::shells_for`]),
    /// and 0 there makes the body shade the clump-map fallback.
    pub fn words(&self) -> [f32; SURFACE_WORDS] {
        let mut w = [0.0; SURFACE_WORDS];
        w[0] = self.metallic;
        w[1] = self.roughness;
        w[2] = self.occlusion_strength;
        if let Some(f) = self.fur {
            w[3] = 1.0;
            w[4] = f.length_m;
            w[5] = f.density;
            w[6] = f.shells as f32;
            w[7] = f.clump_scale_m;
            w[8..11].copy_from_slice(&f.tip_linear);
        }
        if let Some(d) = self.detail {
            w[11] = 1.0;
            w[12] = d.scale_m;
            w[13] = d.strength;
        }
        w
    }
}

// ---- WGSL -------------------------------------------------------------------------

/// Lit model shading, appended after the engine's model.wgsl.
pub const MODEL_SURFACE_WGSL: &str = include_str!("../shaders/model_surface.wgsl");
/// `detail_sample` without feature `biplanar`.
pub const BIPLANAR_STUB_WGSL: &str = include_str!("../shaders/biplanar_stub.wgsl");
/// `fur_sample` without feature `fur`.
pub const FUR_SURFACE_STUB_WGSL: &str = include_str!("../shaders/fur_surface_stub.wgsl");
#[cfg(feature = "biplanar")]
pub const BIPLANAR_WGSL: &str = include_str!("../shaders/biplanar.wgsl");
#[cfg(feature = "fur")]
pub const FUR_SURFACE_WGSL: &str = include_str!("../shaders/fur_surface.wgsl");
#[cfg(feature = "fur")]
pub const FUR_SHELLS_WGSL: &str = include_str!("../shaders/fur_shells.wgsl");
#[cfg(feature = "pcss")]
pub const PCSS_WGSL: &str = include_str!("../shaders/pcss.wgsl");

#[cfg(feature = "biplanar")]
const BIPLANAR_REAL: Option<&str> = Some(BIPLANAR_WGSL);
#[cfg(not(feature = "biplanar"))]
const BIPLANAR_REAL: Option<&str> = None;
#[cfg(feature = "fur")]
const FUR_REAL: Option<(&str, &str)> = Some((FUR_SURFACE_WGSL, FUR_SHELLS_WGSL));
#[cfg(not(feature = "fur"))]
const FUR_REAL: Option<(&str, &str)> = None;
#[cfg(feature = "pcss")]
const PCSS_REAL: Option<&str> = Some(PCSS_WGSL);
#[cfg(not(feature = "pcss"))]
const PCSS_REAL: Option<&str> = None;

/// The lit shader's last line, which writes alpha 1.
pub const OPAQUE_ALPHA_LINE: &str = "    return vec4<f32>(mix(c, frame.fog_color.rgb, f), 1.0);";
/// The same line with feature `gtao`: alpha carries the baked occlusion as
/// `2 - occ` (1 = open, 2 = fully occluded), so GTAO can divide it out and
/// darken only where it finds more than the bake (no double darkening).
/// Everything else that writes alpha 1 (other shaders) reads as "no baked
/// occlusion", and alpha never drops below 1,
/// so the engine tonemap's screen-door mark (alpha < 0.999) never fires.
/// Under fur shells the surface is the skin at the roots, which
/// `surface_fur` already darkens (to 0.65) for the strands around it; that
/// share rides along too, so GTAO does not darken the roots a second time.
/// The decode is [`occlusion_from_alpha`]; evani-fx-gtao's
/// `baked-occlusion` feature does it on the GPU.
pub const OCCLUSION_ALPHA_LINE: &str = "    return vec4<f32>(mix(c, frame.fog_color.rgb, f), 2.0 - occ * select(1.0, 0.65, surface.pbr.w > 0.5 && surface.fur.z >= 0.5 && fur.a > 0.5));";

/// [`MODEL_SURFACE_WGSL`] with its alpha carrying the baked occlusion (see
/// [`OCCLUSION_ALPHA_LINE`]). Used by [`Modules::surface_wgsl`] when the
/// crate is built with feature `gtao`; the engine's look-surface hook does
/// the same replacement when the look asks for GTAO.
pub fn occlusion_to_alpha(surface: &str) -> String {
    surface.replacen(OPAQUE_ALPHA_LINE, OCCLUSION_ALPHA_LINE, 1)
}

/// The fur shell and fin fragment shaders' last lines (alpha 1) and, with
/// feature `gtao`, the same lines writing alpha 1.95 (occlusion 0.05, the
/// decode's floor), so GTAO leaves the strands alone: they already carry
/// their own occlusion (`fur_occ`, the dark roots), and screen-space AO over
/// alpha-tested strand depth only darkens the coat into near-black noise
/// (measured on the v2 chimp's arm: High read about 30% darker than Low).
pub const FUR_ALPHA_LINES: [(&str, &str); 2] = [
    (
        "    return vec4<f32>(fur_light(fur_colour(base, clump, in.h), in.world, n, in.h), 1.0);",
        "    return vec4<f32>(fur_light(fur_colour(base, clump, in.h), in.world, n, in.h), 1.95);",
    ),
    (
        "    return vec4<f32>(fur_light(fur_colour(base, clump, h), in.world, normalize(in.normal), h), 1.0);",
        "    return vec4<f32>(fur_light(fur_colour(base, clump, h), in.world, normalize(in.normal), h), 1.95);",
    ),
];

/// The shells and fins with their alpha opting out of GTAO (see
/// [`FUR_ALPHA_LINES`]); used by [`Modules::surface_wgsl`] with feature `gtao`.
pub fn fur_occlusion_to_alpha(shells: &str) -> String {
    FUR_ALPHA_LINES.iter().fold(shells.to_owned(), |s, (from, to)| s.replacen(from, to, 1))
}

/// CPU mirror of the decode: hdr alpha back to the baked occlusion
/// (alpha 1 or anything below, such as a cleared target, is open).
pub fn occlusion_from_alpha(alpha: f32) -> f32 {
    (2.0 - alpha).clamp(0.05, 1.0)
}

/// Which optional WGSL modules go into the lit shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modules {
    pub biplanar: bool,
    pub fur: bool,
    pub pcss: bool,
}

impl Modules {
    /// Everything this build compiled in.
    pub const BUILT: Modules = Modules { biplanar: cfg!(feature = "biplanar"), fur: cfg!(feature = "fur"), pcss: cfg!(feature = "pcss") };

    /// The modules appended after model.wgsl: model_surface, the detail and
    /// fur samplers (real or stubs) and, with fur, the shells and fins. A
    /// module asked for but not compiled in falls back to its stub.
    pub fn surface_wgsl(self) -> String {
        let mut s = String::with_capacity(16 * 1024);
        if cfg!(feature = "gtao") {
            s.push_str(&occlusion_to_alpha(MODEL_SURFACE_WGSL));
        } else {
            s.push_str(MODEL_SURFACE_WGSL);
        }
        s.push_str(if self.biplanar { BIPLANAR_REAL.unwrap_or(BIPLANAR_STUB_WGSL) } else { BIPLANAR_STUB_WGSL });
        match (self.fur, FUR_REAL) {
            (true, Some((surface, shells))) => {
                s.push_str(surface);
                if cfg!(feature = "gtao") {
                    s.push_str(&fur_occlusion_to_alpha(shells));
                } else {
                    s.push_str(shells);
                }
            }
            _ => s.push_str(FUR_SURFACE_STUB_WGSL),
        }
        s
    }

    /// Does this set draw shells and fins (`fur_shell_vs` etc. exist)?
    pub fn has_fur_passes(self) -> bool {
        self.fur && FUR_REAL.is_some()
    }

    /// The shadow lookup replacing the engine's, if any.
    pub fn shadow_wgsl(self) -> Option<&'static str> {
        if self.pcss { PCSS_REAL } else { None }
    }
}

/// The line the engine's shadow lookup (forward_shadows.wgsl) starts with in
/// the composed forward source.
pub const SHADOW_TAIL_MARKER: &str = "@group(0) @binding(1)";

/// The lit forward template from the engine's composed forward source
/// (`evani_render::shaders::FORWARD.source` with features models + shadows):
/// the source up to its shadow lookup, then [`Modules::surface_wgsl`], then
/// the engine's lookup or PCSS. Splice slots into it as usual
/// (`evani_render::splice::splice`). This is what the `look-surface` hook
/// in `look-surface.patch` does inside evani-render at build time.
pub fn lit_forward_template(engine_forward: &str, m: Modules) -> Result<String, String> {
    if !engine_forward.contains("fn model_vs") {
        return Err("the engine forward source has no model shading (build evani-render with feature `models`)".into());
    }
    let at = engine_forward
        .rfind(SHADOW_TAIL_MARKER)
        .ok_or("the engine forward source has no shadow lookup (build evani-render with feature `shadows`)")?;
    let line = engine_forward[..at].rfind('\n').map_or(0, |i| i + 1);
    let (head, tail) = engine_forward.split_at(line);
    let surface = m.surface_wgsl();
    let shadow = m.shadow_wgsl().unwrap_or(tail);
    let mut out = String::with_capacity(head.len() + surface.len() + shadow.len() + 2);
    out.push_str(head);
    out.push_str(&surface);
    out.push('\n');
    out.push_str(shadow);
    Ok(out)
}
