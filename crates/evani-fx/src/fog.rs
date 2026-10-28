//! Distance fog (feature `fog`): exponential distance fog, thinner with
//! height, that also hides the draw-distance edge. No render pass and no
//! shader file: the effect writes a body for the engine's `fog` shader slot
//! (`slot_fog(distance, height) -> f32`, spliced into the forward and model
//! shaders), so fog is computed where each surface is shaded and costs
//! nothing extra per frame. The fog colour is the renderer's clear colour,
//! which is also what the sky is, so fogged geometry fades into it.
//!
//! The body is built from the `fx.fog.*` params and handed to the renderer
//! through [`SlotRebuilds`] whenever they change (one rebuild at a time; a
//! body that failed to compile is not retried until a param changes). While
//! the effect is off the look's own fog body is put back.

use core::cell::RefCell;

use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_core::slots::{ShaderSlots, SlotRebuilds};
use evani_render::graph::{GraphError, RenderGraph};
#[cfg(not(feature = "camera-range"))] use evani_render::shaders::FAR;

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "fog",
    cost: 1,
    value: 0.6,
    // Catalog works_with: pseudo-3d, 3d.
    styles: StyleClasses::PSEUDO_3D.union(StyleClasses::THREE_D),
    vr_comfort_off: false,
    description: "Distance fog, thinner with height",
};

/// Defaults (also used when a param is missing).
pub const DENSITY: f32 = 0.006;
pub const START: f32 = 25.0;
pub const MAX_OPACITY: f32 = 0.8;
pub const HEIGHT_FALLOFF: f32 = 0.02;
pub const BASE_HEIGHT: f32 = 0.0;
pub const EDGE: f32 = 0.85;

#[derive(Debug, Clone, Copy)]
pub struct FogParams {
    pub density: F32Param,
    pub start: F32Param,
    pub max_opacity: F32Param,
    pub height_falloff: F32Param,
    pub base_height: F32Param,
    pub edge: F32Param,
}

#[derive(Debug, Default)]
pub struct Fog {
    pub params: Option<FogParams>,
    /// The body last handed to the renderer (`None`: the look's own is in use).
    sent: RefCell<Option<String>>,
}

fn f(name: &str, d: f32, min: f32, max: f32, step: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(step)
        .unit(unit)
        .describe(desc)
        .owner("fx.fog")
        .bake(BakeTarget::Look)
}

/// A WGSL float literal (always with a decimal point).
fn lit(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{v:.5}")
}

/// The `fog` slot body for these values (statements only).
pub fn body(density: f32, start: f32, max_opacity: f32, falloff: f32, base: f32, edge: f32) -> String {
    let mut b = format!(
        "    // fx.fog: exponential distance fog, thinner with height\n    let d = max(distance - {}, 0.0);\n    let h = max(height - {}, 0.0);\n    var f = min((1.0 - exp(-d * {})) * exp(-h * {}), {});\n",
        lit(start),
        lit(base),
        lit(density),
        lit(falloff),
        lit(max_opacity)
    );
    // Fully fogged by the far plane, so the draw-distance edge never shows.
    if edge < 0.999 { #[cfg(feature = "camera-range")] #[allow(non_snake_case)] let FAR = evani_render::camera_range::current().1;
        b.push_str(&format!("    f = max(f, smoothstep({}, {}, distance));\n", lit(edge * FAR), lit(FAR)));
    }
    b.push_str("    return f;");
    b
}

impl Effect for Fog {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(FogParams {
            density: ctx.param_f32(f(
                "fx.fog.density",
                DENSITY,
                0.0,
                0.2,
                0.0005,
                "1/m",
                "How fast the fog thickens with distance.",
            ))?,
            start: ctx.param_f32(f(
                "fx.fog.start",
                START,
                0.0,
                500.0,
                1.0,
                "m",
                "Distance where the fog begins; nearer is clear.",
            ))?,
            max_opacity: ctx.param_f32(f(
                "fx.fog.max_opacity",
                MAX_OPACITY,
                0.0,
                1.0,
                0.01,
                "",
                "Most the fog can cover a surface (the far edge still fades fully).",
            ))?,
            height_falloff: ctx.param_f32(f(
                "fx.fog.height_falloff",
                HEIGHT_FALLOFF,
                0.0,
                0.5,
                0.001,
                "1/m",
                "How fast the fog thins going up (0 = the same at every height).",
            ))?,
            base_height: ctx.param_f32(f(
                "fx.fog.base_height",
                BASE_HEIGHT,
                -50.0,
                500.0,
                0.5,
                "m",
                "World height where the fog is thickest; it thins above this.",
            ))?,
            edge: ctx.param_f32(f(
                "fx.fog.edge",
                EDGE,
                0.1,
                1.0,
                0.01,
                "far plane",
                "Fraction of the draw distance where the fog turns fully opaque, hiding the edge (1 = off).",
            ))?,
        });
        Ok(())
    }

    /// No passes: the fog is a shader slot body.
    fn passes(&self, _g: &mut RenderGraph) -> Result<(), GraphError> {
        Ok(())
    }

    fn frame(&self, world: &World) {
        let (Some(p), Ok(mut sent)) = (self.params, self.sent.try_borrow_mut()) else {
            return;
        };
        let on = world.try_res::<EffectsStatus>().is_some_and(|s| s.is_on(INFO.name));
        let want = {
            let Some(slots) = world.try_res::<ShaderSlots>() else {
                return;
            };
            let Some(id) = slots.id("fog") else {
                return;
            };
            if on {
                let reg = world.res::<ParamRegistry>();
                Some(body(
                    reg.f32(p.density),
                    reg.f32(p.start),
                    reg.f32(p.max_opacity),
                    reg.f32(p.height_falloff),
                    reg.f32(p.base_height),
                    reg.f32(p.edge),
                ))
            } else if sent.is_some() {
                // Off again: back to the look's own fog.
                slots.get(id).map(|d| d.default_body.clone())
            } else {
                None
            }
        };
        let Some(want) = want else {
            return;
        };
        if on && sent.as_deref() == Some(want.as_str()) {
            return;
        }
        let queued = match world.try_res_mut::<SlotRebuilds>() {
            Some(mut rb) => {
                if rb.has_pending() {
                    return;
                }
                let slots = world.res::<ShaderSlots>();
                rb.request_detached(&slots, "fog", &want, evani_core::time::now_ms()).is_ok()
            }
            None => {
                let mut slots = world.res_mut::<ShaderSlots>();
                slots.id("fog").is_some_and(|id| slots.set_body(id, &want).is_ok())
            }
        };
        if queued {
            *sent = if on { Some(want) } else { None };
        }
    }
}

impl PostEffect for Fog {
    fn name(&self) -> &'static str {
        INFO.name
    }

    fn pass_names(&self) -> &'static [&'static str] {
        &[]
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &[]
    }

    fn write_uniforms(&self, _: &str, _: &ParamRegistry, _: &UniformFrame, _: &mut [f32]) -> usize {
        0
    }

    fn record(&self, _: &str, _: &ParamRegistry, _: &mut dyn PostRecorder) {}
}
