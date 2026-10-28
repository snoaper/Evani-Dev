//! Vignette (feature `vignette`): one pass that darkens the screen edges,
//! drawn onto the tonemapped image with multiplicative blending.

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph};

use crate::{
    Blend, Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame, draw_effect,
    put_words,
};

pub static INFO: EffectInfo = EffectInfo {
    name: "vignette",
    cost: 1,
    value: 0.3,
    styles: StyleClasses::ALL,
    vr_comfort_off: true,
    description: "Darkens the screen edges",
};

pub const PASSES: &[&str] = &["vignette"];

static SHADERS: [EffectShader; 1] = [EffectShader {
    name: "vignette",
    pass: "vignette",
    source: include_str!("../shaders/vignette.wgsl"),
    slots: &[],
    blend: Blend::Multiply,
    uniform_words: 8,
    input: false,
    depth: false,
}];

#[derive(Debug, Clone, Copy)]
pub struct VignetteParams {
    pub intensity: F32Param,
    pub radius: F32Param,
    pub softness: F32Param,
    pub roundness: F32Param,
    pub color: Vec3Param,
}

#[derive(Debug, Default)]
pub struct Vignette {
    pub params: Option<VignetteParams>,
}

fn f(name: &str, d: f32, min: f32, max: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(0.01)
        .unit(unit)
        .describe(desc)
        .owner("fx.vignette")
        .bake(BakeTarget::Look)
        .gpu()
}

impl Effect for Vignette {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(VignetteParams {
            intensity: ctx.param_f32(f(
                "fx.vignette.intensity",
                0.35,
                0.0,
                1.0,
                "",
                "How dark the edges get.",
            ))?,
            radius: ctx.param_f32(f(
                "fx.vignette.radius",
                0.75,
                0.0,
                1.5,
                "screen",
                "Distance from the centre where darkening starts (1 = edge).",
            ))?,
            softness: ctx.param_f32(f(
                "fx.vignette.softness",
                0.45,
                0.01,
                1.5,
                "screen",
                "Width of the fade into the edges.",
            ))?,
            roundness: ctx.param_f32(f(
                "fx.vignette.roundness",
                1.0,
                0.0,
                1.0,
                "",
                "0 follows the shape of the screen, 1 is a circle.",
            ))?,
            color: ctx.param_vec3(
                ParamDef::color("fx.vignette.color", [0.0, 0.0, 0.0])
                    .unit("linear rgb")
                    .describe("Colour the edges fade towards.")
                    .owner("fx.vignette")
                    .bake(BakeTarget::Look)
                    .gpu(),
            )?,
        });
        Ok(())
    }

    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        g.add_pass(
            PassDesc::new("vignette", "fx.vignette")
                .reads(&["ldr"])
                .writes(&["ldr"])
                .order(200),
        )
    }
}

impl PostEffect for Vignette {
    fn name(&self) -> &'static str {
        INFO.name
    }

    /// One uniform-only draw multiplied into `ldr`.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        if pass == "vignette" {
            draw_effect(self, rec, reg, 0, None, "ldr", 0);
        }
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn write_uniforms(
        &self,
        shader: &str,
        reg: &ParamRegistry,
        frame: &UniformFrame,
        out: &mut [f32],
    ) -> usize {
        if shader != "vignette" {
            return 0;
        }
        let p = self.params;
        let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
        let c = p.map_or([0.0; 3], |p| reg.vec3(p.color));
        let aspect = frame.target_size[0] / frame.target_size[1].max(1.0);
        put_words(
            out,
            &[
                c[0],
                c[1],
                c[2],
                v(p.map(|p| p.intensity), 0.35),
                v(p.map(|p| p.radius), 0.75),
                v(p.map(|p| p.softness), 0.45),
                v(p.map(|p| p.roundness), 1.0),
                aspect,
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FxModule, FxStatus};
    use evani_core::App;
    use evani_core::tiers::Tiers;
    use evani_render::RenderModule;

    #[test]
    fn vignette_follows_tier_start_list() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Vignette::default()));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        // low.json starts vignette on.
        assert!(a.world.res::<FxStatus>().is_on("vignette"));
        assert!(a.world.res::<RenderGraph>().pass("vignette").unwrap().enabled);
        // vr: comfort-off.
        a.world.res_mut::<Tiers>().set_xr_session(true);
        assert_eq!(a.world.res::<Tiers>().active().id, "vr");
        a.run_headless(1, 16.0);
        assert!(!a.world.res::<FxStatus>().is_on("vignette"));
        assert!(!a.world.res::<RenderGraph>().pass("vignette").unwrap().enabled);
    }
}
