//! 2D dynamic lights (feature `lights2d`, catalog `lights2d`, 2 points, 2D):
//! "normal-mapped sprites lit by point lights accumulated in a light
//! buffer". The drawing lives in the renderer (feature
//! `evani-render/lights2d`, turned on with this one): the sprites pass sums
//! the kit's `evani_render::Lights2d` per pixel through each sprite's normal
//! map (frame `<name>.n`), flat-facing without one. This effect is the
//! budgeted switch and the tunables. It starts on where the tier gives a
//! light count (High 4, Ultra 7: the catalog's 2D render path rows), and a
//! look can still veto it (`evani-look-pixelhd` prefers it on).

use evani_core::params::{BakeTarget, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, RenderGraph};
use evani_render::lights2d as l;

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: l::EFFECT,
    cost: 2,
    value: 0.8,
    // Catalog works_with: 2d.
    styles: StyleClasses::TWO_D,
    vr_comfort_off: false,
    description: "Normal-mapped sprites lit by point lights",
};

#[derive(Debug, Default)]
pub struct Lights2d;

/// Does the tier start the lights on? (It gives them a count.)
pub fn tier_on(tier: &str) -> bool {
    l::tier_cap(tier) > 0
}

impl Effect for Lights2d {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        // Names, defaults and ranges match evani-look-pixelhd's
        // `look.pixelhd.light.*`, plus the light height.
        let defs: [(f32, f32, f32, &str, &str); 6] = [
            (l::AMBIENT, 0.0, 1.0, "", "Base light level before any dynamic light (1 = fully lit)."),
            (l::STRENGTH, 0.0, 3.0, "x", "Brightness of the dynamic lights."),
            (l::NORMAL_STRENGTH, 0.0, 2.0, "x", "How much a sprite normal map bends the light (0 = flat)."),
            (l::RADIUS_SCALE, 0.25, 3.0, "x", "Scale on every light radius."),
            (l::RIM, 0.0, 1.0, "", "Rim accent on the edge of lit sprites."),
            (l::HEIGHT, 0.05, 2.0, "x", "Light height above the sprites, as a share of its radius (low = grazing)."),
        ];
        for (name, (d, min, max, unit, desc)) in l::PARAMS.into_iter().zip(defs) {
            ctx.param_f32(
                ParamDef::f32(name, d)
                    .range(min, max)
                    .step(0.01)
                    .unit(unit)
                    .describe(desc)
                    .owner("fx.lights2d")
                    .bake(BakeTarget::Look)
                    .gpu(),
            )?;
        }
        Ok(())
    }

    /// No graph pass: the lights are drawn in the sprites pass.
    fn passes(&self, _g: &mut RenderGraph) -> Result<(), GraphError> {
        Ok(())
    }
}

impl PostEffect for Lights2d {
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
