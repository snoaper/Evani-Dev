//! The photoreal look's `tonemap` slot body: a filmic stage (scene-linear
//! HDR to display range) followed by the grade stage ([`crate::grade`]).
//!
//! Slot bodies see only their arguments, so the tunable values are written
//! into the body as constants. The look registers them as params
//! (`look.photoreal.tonemap.*`, `look.photoreal.grade.*`) and a small
//! frame system ([`resync`]) re-composes the body and asks the renderer to
//! rebuild the tonemap pipeline when one of them changes (live tuning, or
//! baked values applied at start). The system only ever replaces a body it
//! composed itself: a hand edit through `slot.set`, or a baked
//! `slots/tonemap.wgsl`, is left alone.
//!
//! Filmic stage, per channel (so very bright colours roll towards white the
//! way film does):
//! * toe: `x * x / (x + toe)`, rescaled so mid grey (0.18) stays put: deeper,
//!   denser blacks;
//! * shoulder: extended Reinhard with a white point, `y (1 + y / w²) / (1 + y)`,
//!   which reaches exactly 1 at `w`.

use crate::grade::{self, GRADE_SOURCES, GradeSettings, lit};
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{EnumParam, F32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{LookContext, PluginError};
use evani_core::slots::{ShaderSlots, SlotRebuilds};

/// The filmic stage's settings (one per `look.photoreal.tonemap.*` param).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilmicSettings {
    /// Scene value that maps to display white.
    pub white: f32,
    /// Toe strength: how much the blacks are pressed down (0 = none).
    pub toe: f32,
}

impl Default for FilmicSettings {
    fn default() -> Self {
        Self {
            white: 11.0,
            toe: 0.03,
        }
    }
}

impl FilmicSettings {
    /// The filmic stage as WGSL statements: reads `color`, declares `var c`.
    pub fn wgsl(&self) -> String {
        let white = self.white.clamp(1.0, 64.0);
        let toe = self.toe.clamp(0.0, 0.5);
        format!(
            "    // evani-look-photoreal: filmic tonemap (toe, then a white-point shoulder)\n\
             \x20   let x = max(color, vec3<f32>(0.0));\n\
             \x20   let toe = {toe};\n\
             \x20   let y = x * x / (x + vec3<f32>(toe + 0.000001)) * ((0.18 + toe) / 0.18);\n\
             \x20   let white = {white};\n\
             \x20   var c = y * (vec3<f32>(1.0) + y / (white * white)) / (vec3<f32>(1.0) + y);\n\
             \x20   c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));",
            toe = lit(toe),
            white = lit(white),
        )
    }
}

/// The whole `tonemap` slot body: filmic stage, then the grade stage if any.
pub fn compose(filmic: &FilmicSettings, grade_stage: Option<&str>) -> String {
    let mut body = filmic.wgsl();
    body.push('\n');
    if let Some(g) = grade_stage {
        body.push_str(g);
        body.push('\n');
    }
    body.push_str("    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));");
    body
}

/// Handles to the look's tonemap and grade params.
#[derive(Debug, Clone, Copy)]
pub struct TonemapParams {
    pub white: F32Param,
    pub toe: F32Param,
    pub source: EnumParam,
    pub contrast: F32Param,
    pub saturation: F32Param,
    pub lift: F32Param,
    pub split: F32Param,
    pub shadow_tint: Vec3Param,
    pub highlight_tint: Vec3Param,
}

impl TonemapParams {
    pub fn register(look: &mut LookContext, filmic: &FilmicSettings, grade: &GradeSettings, source: u32) -> Result<Self, PluginError> {
        let white = look.param_f32(
            ParamDef::f32("look.photoreal.tonemap.white", filmic.white)
                .range(1.0, 64.0)
                .step(0.1)
                .unit("x")
                .describe("Filmic tonemap: the scene brightness that maps to display white (higher keeps more highlight detail, lower looks punchier)"),
        )?;
        let toe = look.param_f32(
            ParamDef::f32("look.photoreal.tonemap.toe", filmic.toe)
                .range(0.0, 0.5)
                .step(0.005)
                .describe("Filmic tonemap: how far the blacks are pressed down (mid grey stays put; 0 = no toe)"),
        )?;
        let source = look.param_enum(
            ParamDef::enumeration("look.photoreal.grade.source", &GRADE_SOURCES, source)
                .describe("Colour grade after the tonemap: the look's photographic grade, a game's custom grade body, or none (for the separate LUT grade effect)"),
        )?;
        let contrast = look.param_f32(
            ParamDef::f32("look.photoreal.grade.contrast", grade.contrast)
                .range(0.25, 4.0)
                .step(0.01)
                .unit("x")
                .describe("Grade: contrast around mid grey (1 = unchanged)"),
        )?;
        let saturation = look.param_f32(
            ParamDef::f32("look.photoreal.grade.saturation", grade.saturation)
                .range(0.0, 2.0)
                .step(0.01)
                .unit("x")
                .describe("Grade: colour saturation (1 = unchanged; the documentary look sits under 1)"),
        )?;
        let lift = look.param_f32(
            ParamDef::f32("look.photoreal.grade.lift", grade.lift)
                .range(0.0, 0.5)
                .step(0.001)
                .describe("Grade: lifts the blacks towards a matte print (0 = none)"),
        )?;
        let split = look.param_f32(
            ParamDef::f32("look.photoreal.grade.split", grade.split)
                .range(0.0, 1.0)
                .step(0.01)
                .describe("Grade: how far shadows and highlights lean towards their tints (0 = none)"),
        )?;
        let shadow_tint = look.param_vec3(
            ParamDef::color("look.photoreal.grade.shadow_tint", grade.shadow_tint)
                .describe("Grade: the tint shadows lean towards (linear RGB; brightness is kept)"),
        )?;
        let highlight_tint = look.param_vec3(
            ParamDef::color("look.photoreal.grade.highlight_tint", grade.highlight_tint)
                .describe("Grade: the tint highlights lean towards (linear RGB; brightness is kept)"),
        )?;
        Ok(Self {
            white,
            toe,
            source,
            contrast,
            saturation,
            lift,
            split,
            shadow_tint,
            highlight_tint,
        })
    }

    /// Current values.
    pub fn read(&self, reg: &ParamRegistry) -> (FilmicSettings, GradeSettings, u32) {
        (
            FilmicSettings {
                white: reg.f32(self.white),
                toe: reg.f32(self.toe),
            },
            GradeSettings {
                contrast: reg.f32(self.contrast),
                saturation: reg.f32(self.saturation),
                lift: reg.f32(self.lift),
                split: reg.f32(self.split),
                shadow_tint: reg.vec3(self.shadow_tint),
                highlight_tint: reg.vec3(self.highlight_tint),
            },
            reg.enum_index(self.source),
        )
    }
}

/// The body for the given values.
pub fn body_for(filmic: &FilmicSettings, grade_settings: &GradeSettings, source: u32, custom: Option<&'static str>) -> String {
    let stage = grade::stage(source, grade_settings, custom);
    compose(filmic, stage.as_deref())
}

/// Install the body and the resync system.
pub fn install(
    look: &mut LookContext,
    filmic: FilmicSettings,
    grade_settings: GradeSettings,
    source: u32,
    custom: Option<&'static str>,
) -> Result<(), PluginError> {
    let params = TonemapParams::register(look, &filmic, &grade_settings, source)?;
    let body = body_for(&filmic, &grade_settings, source, custom);
    look.shader_slot("tonemap", &body)?;
    let mut sync = Resync {
        params,
        custom,
        values: (filmic, grade_settings, source),
        applied: body,
        pending: None,
    };
    look.system(SystemDesc::new("look.photoreal.tonemap_resync", Stage::Frame, move |w| resync(&mut sync, w)));
    Ok(())
}

/// State of the resync system.
pub struct Resync {
    params: TonemapParams,
    custom: Option<&'static str>,
    /// The values the last composed body was built from.
    values: (FilmicSettings, GradeSettings, u32),
    /// The last composed body the renderer is known to run.
    applied: String,
    /// A composed body asked for and not yet swapped in.
    pending: Option<String>,
}

/// Re-compose the tonemap body when a look param changed, if the slot still
/// holds a body this look composed.
pub fn resync(s: &mut Resync, world: &mut World) {
    let now = s.params.read(&world.res::<ParamRegistry>());
    let current = {
        let slots = world.res::<ShaderSlots>();
        let Some(id) = slots.id("tonemap") else {
            return;
        };
        let Some(def) = slots.get(id) else {
            return;
        };
        def.body.clone()
    };
    if s.pending.as_deref() == Some(current.as_str()) {
        s.applied = current.clone();
        s.pending = None;
    }
    if now == s.values {
        return;
    }
    if current != s.applied && s.pending.is_none() {
        // Someone else owns the body now (a hand edit or a baked slot file).
        s.values = now;
        return;
    }
    let (filmic, grade_settings, source) = now;
    let body = body_for(&filmic, &grade_settings, source, s.custom);
    s.values = now;
    if body == current {
        return;
    }
    let queued = {
        let slots = world.res::<ShaderSlots>();
        world.res_mut::<SlotRebuilds>().request_detached(&slots, "tonemap", &body, 0.0)
    };
    if queued.is_ok() {
        s.pending = Some(body);
    }
}
