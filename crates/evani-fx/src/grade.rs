//! Colour grade (feature `grade`): the grade slot after the tonemap, with a
//! per-scene swap and a crossfade.
//!
//! One pair of passes on the tonemapped `ldr` image: `grade.apply` samples
//! `ldr` and writes the graded image to a transient target, `grade.back`
//! copies it home (a pass cannot read the target it writes). The grade is a
//! small analytic one instead of a 32^3 lookup texture, because the renderer
//! binds only one source texture per effect draw: gain and warmth in linear
//! light, then contrast, saturation, split tone and black lift in a
//! perceptual space. Two presets live in the params, the current grade
//! (`fx.grade.*`) and the next scene's (`fx.grade.next.*`); `fx.grade.fade`
//! mixes them (0 = current, 1 = next), so a scene swap is [`set_next`],
//! animate `fade` to 1, then [`commit`].
//!
//! Next to a look's own grade (the photoreal look's `look.photoreal.grade.*`
//! stage inside its tonemap): `fx.grade.with_look` is `stack` (default; both
//! apply, and the neutral default preset changes nothing) or `replace` (while
//! this effect is on, the look's `look.photoreal.grade.source` is set to
//! `none` and put back when it goes off), so the two never fight.

use core::cell::Cell;

use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::{
    BakeTarget, EnumParam, F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param,
};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph, TargetDesc, TargetKind};

use crate::{
    Blend, Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame, draw_effect,
    put_words,
};

pub static INFO: EffectInfo = EffectInfo {
    name: "grade",
    cost: 1,
    value: 0.5,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: false,
    description: "Colour grade with a per-scene swap and crossfade",
};

pub const PASSES: &[&str] = &["grade.apply", "grade.back"];

static SHADERS: [EffectShader; 1] = [EffectShader {
    name: "grade",
    pass: "grade.apply",
    source: include_str!("../shaders/grade.wgsl"),
    slots: &[],
    blend: Blend::Replace,
    uniform_words: 16,
    input: true,
    depth: false,
}];

/// The look param this effect hands over in `replace` mode (the photoreal
/// look's grade source; absent for looks without one).
pub const LOOK_SOURCE: &str = "look.photoreal.grade.source";

/// One grade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradePreset {
    /// Brightness multiplier in linear light.
    pub gain: f32,
    /// + warmer, - cooler.
    pub warmth: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// How far shadows and highlights lean towards their tints.
    pub tone: f32,
    /// Black lift towards a matte print.
    pub lift: f32,
    /// Shadow and highlight tints (white = none).
    pub shadow: [f32; 3],
    pub highlight: [f32; 3],
}

impl GradePreset {
    /// Changes nothing.
    pub const NEUTRAL: Self = Self {
        gain: 1.0,
        warmth: 0.0,
        contrast: 1.0,
        saturation: 1.0,
        tone: 0.0,
        lift: 0.0,
        shadow: [1.0; 3],
        highlight: [1.0; 3],
    };
    /// Late afternoon: warm highlights, a little more colour.
    pub const WARM: Self = Self {
        warmth: 0.08,
        saturation: 1.1,
        tone: 0.3,
        highlight: [1.0, 0.9, 0.75],
        shadow: [0.9, 0.95, 1.0],
        ..Self::NEUTRAL
    };
    /// Night or ice: cool and a little dim.
    pub const COLD: Self = Self {
        gain: 0.95,
        warmth: -0.1,
        saturation: 0.85,
        tone: 0.3,
        shadow: [0.8, 0.9, 1.0],
        ..Self::NEUTRAL
    };
    /// Almost black and white, hard contrast.
    pub const NOIR: Self = Self {
        contrast: 1.3,
        saturation: 0.05,
        ..Self::NEUTRAL
    };
    /// Washed out, matte blacks.
    pub const FADED: Self = Self {
        contrast: 0.85,
        saturation: 0.7,
        lift: 0.08,
        ..Self::NEUTRAL
    };

    fn mix(&self, o: &Self, t: f32) -> Self {
        let m = |a: f32, b: f32| a + (b - a) * t;
        let m3 = |a: [f32; 3], b: [f32; 3]| [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])];
        Self {
            gain: m(self.gain, o.gain),
            warmth: m(self.warmth, o.warmth),
            contrast: m(self.contrast, o.contrast),
            saturation: m(self.saturation, o.saturation),
            tone: m(self.tone, o.tone),
            lift: m(self.lift, o.lift),
            shadow: m3(self.shadow, o.shadow),
            highlight: m3(self.highlight, o.highlight),
        }
    }

    fn values(&self) -> [ParamValue; 8] {
        [
            ParamValue::F32(self.gain),
            ParamValue::F32(self.warmth),
            ParamValue::F32(self.contrast),
            ParamValue::F32(self.saturation),
            ParamValue::F32(self.tone),
            ParamValue::F32(self.lift),
            ParamValue::Color(self.shadow),
            ParamValue::Color(self.highlight),
        ]
    }
}

/// Param names of a preset, in [`GradePreset::values`] order.
const FIELDS: [&str; 8] = [
    "gain",
    "warmth",
    "contrast",
    "saturation",
    "tone",
    "lift",
    "shadow_tint",
    "highlight_tint",
];

fn name(prefix: &str, field: &str) -> String {
    format!("fx.grade.{prefix}{field}")
}

/// Set the next scene's grade (`fx.grade.next.*`). Animate `fx.grade.fade`
/// from 0 to 1 afterwards, then call [`commit`].
pub fn set_next(reg: &mut ParamRegistry, p: &GradePreset) {
    for (field, v) in FIELDS.iter().zip(p.values()) {
        let _ = reg.set_by_name(&name("next.", field), v);
    }
}

/// Make the next grade the current one and reset the fade to 0.
pub fn commit(reg: &mut ParamRegistry) {
    for field in FIELDS {
        let v = reg.id(&name("next.", field)).and_then(|id| reg.value(id));
        if let Some(v) = v {
            let _ = reg.set_by_name(&name("", field), v);
        }
    }
    let _ = reg.set_by_name("fx.grade.fade", ParamValue::F32(0.0));
}

#[derive(Debug, Clone, Copy)]
struct SetParams {
    gain: F32Param,
    warmth: F32Param,
    contrast: F32Param,
    saturation: F32Param,
    tone: F32Param,
    lift: F32Param,
    shadow: Vec3Param,
    highlight: Vec3Param,
}

impl SetParams {
    fn read(&self, reg: &ParamRegistry) -> GradePreset {
        GradePreset {
            gain: reg.f32(self.gain),
            warmth: reg.f32(self.warmth),
            contrast: reg.f32(self.contrast),
            saturation: reg.f32(self.saturation),
            tone: reg.f32(self.tone),
            lift: reg.f32(self.lift),
            shadow: reg.vec3(self.shadow),
            highlight: reg.vec3(self.highlight),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GradeParams {
    current: SetParams,
    next: SetParams,
    pub fade: F32Param,
    pub amount: F32Param,
    pub with_look: EnumParam,
}

#[derive(Debug, Default)]
pub struct Grade {
    pub params: Option<GradeParams>,
    /// The look's grade source before this effect set it to `none`.
    taken: Cell<Option<u32>>,
}

fn register_set(ctx: &mut PluginContext, prefix: &str, p: &GradePreset) -> Result<SetParams, PluginError> {
    let mut f = |field: &str, d: f32, min: f32, max: f32, desc: &str| {
        ctx.param_f32(
            ParamDef::f32(&name(prefix, field), d)
                .range(min, max)
                .step(0.01)
                .describe(desc)
                .owner("fx.grade")
                .bake(BakeTarget::Look),
        )
    };
    let gain = f("gain", p.gain, 0.0, 3.0, "Brightness multiplier in linear light.")?;
    let warmth = f("warmth", p.warmth, -0.5, 0.5, "Warmer (+) or cooler (-).")?;
    let contrast = f("contrast", p.contrast, 0.25, 3.0, "Contrast around mid grey (1 = unchanged).")?;
    let saturation = f("saturation", p.saturation, 0.0, 2.0, "Colour strength (1 = unchanged, 0 = grey).")?;
    let tone = f("tone", p.tone, 0.0, 1.0, "How far shadows and highlights lean towards their tints.")?;
    let lift = f("lift", p.lift, 0.0, 0.5, "Black lift towards a matte print.")?;
    let mut c = |field: &str, d: [f32; 3], desc: &str| {
        ctx.param_vec3(
            ParamDef::color(&name(prefix, field), d)
                .unit("linear rgb")
                .describe(desc)
                .owner("fx.grade")
                .bake(BakeTarget::Look),
        )
    };
    Ok(SetParams {
        gain,
        warmth,
        contrast,
        saturation,
        tone,
        lift,
        shadow: c("shadow_tint", p.shadow, "Tint of the shadows (white = none).")?,
        highlight: c("highlight_tint", p.highlight, "Tint of the highlights (white = none).")?,
    })
}

impl Effect for Grade {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let current = register_set(ctx, "", &GradePreset::NEUTRAL)?;
        let next = register_set(ctx, "next.", &GradePreset::NEUTRAL)?;
        self.params = Some(GradeParams {
            current,
            next,
            fade: ctx.param_f32(
                ParamDef::f32("fx.grade.fade", 0.0)
                    .range(0.0, 1.0)
                    .step(0.01)
                    .describe("Crossfade from the current grade (0) to the next scene's (1).")
                    .owner("fx.grade")
                    .bake(BakeTarget::None),
            )?,
            amount: ctx.param_f32(
                ParamDef::f32("fx.grade.amount", 1.0)
                    .range(0.0, 1.0)
                    .step(0.01)
                    .describe("How much of the grade shows (0 = the plain image).")
                    .owner("fx.grade")
                    .bake(BakeTarget::Look),
            )?,
            with_look: ctx.param_enum(
                ParamDef::enumeration("fx.grade.with_look", &["stack", "replace"], 0)
                    .describe("stack: apply on top of the look's own grade; replace: switch the look's grade off while this effect is on.")
                    .owner("fx.grade")
                    .bake(BakeTarget::Look),
            )?,
        });
        Ok(())
    }

    /// Orders 150 and 151: after the tonemap (100), before the vignette (200)
    /// and the HUD.
    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        g.add_target(TargetDesc::new("grade.tmp", TargetKind::Ldr).transient())?;
        g.add_pass(
            PassDesc::new("grade.apply", "fx.grade")
                .reads(&["ldr"])
                .writes(&["grade.tmp"])
                .order(150),
        )?;
        g.add_pass(
            PassDesc::new("grade.back", "fx.grade")
                // Also reads `ldr`: it overwrites it, so it is an in-place
                // writer. Without this the graph sees `grade.back` as a pure
                // writer of `ldr` that `grade.apply` must follow, while it
                // follows `grade.apply` itself: a cycle.
                .reads(&["grade.tmp", "ldr"])
                .writes(&["ldr"])
                .order(151),
        )
    }

    /// Hand the look's grade over (or back) for `replace` mode.
    fn frame(&self, world: &World) {
        let Some(p) = self.params else {
            return;
        };
        let want = world.try_res::<EffectsStatus>().is_some_and(|s| s.is_on(INFO.name))
            && world.res::<ParamRegistry>().enum_index(p.with_look) == 1;
        if want == self.taken.get().is_some() {
            return;
        }
        let mut reg = world.res_mut::<ParamRegistry>();
        let Some(id) = reg.id(LOOK_SOURCE) else {
            return;
        };
        if want {
            let Some(ParamValue::Enum(cur)) = reg.value(id) else {
                return;
            };
            let none = reg.def(id).and_then(|d| d.options.iter().position(|o| o == "none"));
            let Some(none) = none else {
                return;
            };
            let _ = reg.set(id, ParamValue::Enum(none as u32));
            self.taken.set(Some(cur));
        } else if let Some(prev) = self.taken.get() {
            let _ = reg.set(id, ParamValue::Enum(prev));
            self.taken.set(None);
        }
    }
}

impl PostEffect for Grade {
    fn name(&self) -> &'static str {
        INFO.name
    }

    /// `grade.apply`: ldr -> grade.tmp, graded. `grade.back`: grade.tmp -> ldr
    /// (`mip` 1 asks the shader for the plain copy).
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        match pass {
            "grade.apply" => draw_effect(self, rec, reg, 0, Some("ldr"), "grade.tmp", 0),
            "grade.back" => draw_effect(self, rec, reg, 0, Some("grade.tmp"), "ldr", 1),
            _ => {}
        }
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn write_uniforms(&self, shader: &str, reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        if shader != "grade" {
            return 0;
        }
        let (g, amount) = match self.params {
            Some(p) => {
                let t = reg.f32(p.fade);
                (p.current.read(reg).mix(&p.next.read(reg), t), reg.f32(p.amount))
            }
            None => (GradePreset::NEUTRAL, 1.0),
        };
        put_words(
            out,
            &[
                g.shadow[0],
                g.shadow[1],
                g.shadow[2],
                g.gain,
                g.highlight[0],
                g.highlight[1],
                g.highlight[2],
                g.warmth,
                g.contrast,
                g.saturation,
                g.tone,
                g.lift,
                amount,
                frame.mip as f32,
                0.0,
                0.0,
            ],
        )
    }
}
