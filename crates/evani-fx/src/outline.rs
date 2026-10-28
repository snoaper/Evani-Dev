//! Ink outlines (feature `outline`): one pass that finds depth edges
//! (silhouettes, against the sky too) and convex creases (view-space points
//! rebuilt from the depth bend at an edge; see the WGSL) and multiplies
//! dark ink into `hdr` before bloom and tonemap. Reads the scene depth
//! (`EffectShader.depth`), so the forward pass keeps it while this is on.
//! Full-canvas over the depth, so split-screen views are covered (the seam
//! between two views inks as a thin divider).

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph};
use evani_render::depth::inverse_distance;
use evani_render::shaders::{FAR, NEAR};

use crate::{
    Blend, Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame, draw_effect,
    put_words,
};

pub static INFO: EffectInfo = EffectInfo {
    name: "outline",
    cost: 2,
    value: 0.7,
    styles: StyleClasses::TWO_D.union(StyleClasses::THREE_D),
    vr_comfort_off: false,
    description: "Ink outlines on depth edges",
};

pub const PASSES: &[&str] = &["outline"];

static SHADERS: [EffectShader; 1] = [EffectShader {
    name: "outline",
    pass: "outline",
    source: include_str!("../shaders/outline.wgsl"),
    slots: &[],
    blend: Blend::Multiply,
    uniform_words: 12,
    input: false,
    depth: true,
}];

/// Defaults (also used when a param is missing).
pub const THICKNESS: f32 = 1.0;
pub const DEPTH_SENSITIVITY: f32 = 1.0;
pub const STRENGTH: f32 = 0.85;
pub const COLOR: [f32; 3] = [0.035, 0.025, 0.07];
pub const CREASE: f32 = 1.0;
/// Crease lines fade out by this distance (m): further away, 32-bit depth
/// can't tell a crease from noise across a pixel.
pub const CREASE_FAR: f32 = 70.0;
/// Pixel scale for the crease test at a 60 degree field of view (it only
/// sets how strongly a crease reads; flat stays flat at any scale).
const TAN_HALF_FOV: f32 = 0.577_35;

#[derive(Debug, Clone, Copy)]
pub struct OutlineParams {
    pub thickness: F32Param,
    pub depth_sensitivity: F32Param,
    pub strength: F32Param,
    pub crease: F32Param,
    pub color: Vec3Param,
}

#[derive(Debug, Default)]
pub struct Outline {
    pub params: Option<OutlineParams>,
}

fn f(name: &str, d: f32, min: f32, max: f32, step: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(step)
        .unit(unit)
        .describe(desc)
        .owner("fx.outline")
        .bake(BakeTarget::Look)
        .gpu()
}

impl Effect for Outline {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(OutlineParams {
            thickness: ctx.param_f32(f(
                "fx.outline.thickness",
                THICKNESS,
                0.0,
                4.0,
                0.25,
                "px",
                "Line width in pixels (whole pixels; below 1 fades the lines out).",
            ))?,
            depth_sensitivity: ctx.param_f32(f(
                "fx.outline.depth_sensitivity",
                DEPTH_SENSITIVITY,
                0.0,
                8.0,
                0.05,
                "",
                "How small a depth step still inks (higher = more, finer edges).",
            ))?,
            strength: ctx.param_f32(f(
                "fx.outline.strength",
                STRENGTH,
                0.0,
                1.0,
                0.01,
                "",
                "How dark the ink is (0 = no lines, 1 = full ink colour).",
            ))?,
            crease: ctx.param_f32(f(
                "fx.outline.crease",
                CREASE,
                0.0,
                3.0,
                0.05,
                "",
                "Ink on creases (0 = silhouettes only, 1 = edges of about 45 degrees and sharper, higher = gentler bends too).",
            ))?,
            color: ctx.param_vec3(
                ParamDef::color("fx.outline.color", COLOR)
                    .unit("linear rgb")
                    .describe("Ink colour (multiplied into the scene).")
                    .owner("fx.outline")
                    .bake(BakeTarget::Look)
                    .gpu(),
            )?,
        });
        Ok(())
    }

    /// Order 5: after the forward pass, before sprites (HUD stays clean)
    /// and bloom (glows spread over the ink, not under it).
    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        g.add_pass(
            PassDesc::new("outline", "fx.outline")
                .reads(&["hdr", "depth"])
                .writes(&["hdr"])
                .order(5),
        )
    }
}

impl PostEffect for Outline {
    fn name(&self) -> &'static str {
        INFO.name
    }

    /// One depth-reading draw multiplied into `hdr`.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        if pass == "outline" {
            draw_effect(self, rec, reg, 0, None, "hdr", 0);
        }
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn write_uniforms(&self, shader: &str, reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        if shader != "outline" {
            return 0;
        }
        let p = self.params;
        let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
        let c = p.map_or(COLOR, |p| reg.vec3(p.color));
        let [inv0, inv1] = inverse_distance(NEAR, FAR); #[cfg(feature = "camera-range")] let [inv0, inv1] = { let _ = [inv0, inv1]; let (n, f) = evani_render::camera_range::current(); inverse_distance(n, f) };
        put_words(
            out,
            &[
                c[0],
                c[1],
                c[2],
                v(p.map(|p| p.strength), STRENGTH),
                v(p.map(|p| p.thickness), THICKNESS),
                v(p.map(|p| p.depth_sensitivity), DEPTH_SENSITIVITY),
                inv0,
                inv1,
                v(p.map(|p| p.crease), CREASE),
                2.0 * TAN_HALF_FOV / frame.target_size[1].max(1.0),
                CREASE_FAR,
                0.0,
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FxModule, FxStatus};
    use evani_core::App;
    use evani_core::params::ParamValue;
    use evani_core::tiers::Tiers;
    use evani_render::RenderModule;

    #[test]
    fn outline_reads_depth_and_stays_on_in_vr() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Outline::default()));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("outline"));
        {
            let g = a.world.res::<RenderGraph>();
            let pass = g.pass("outline").unwrap();
            assert!(pass.enabled);
            assert!(pass.inputs.contains(&"depth"));
        }
        assert!(SHADERS[0].depth && !SHADERS[0].input);
        // Not a comfort effect: stays on in VR.
        a.world.res_mut::<Tiers>().set_xr_session(true);
        assert_eq!(a.world.res::<Tiers>().active().id, "vr");
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("outline"));
    }

    #[test]
    fn uniforms_carry_params_and_clip_planes() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Outline::default()));
        a.run_headless(1, 16.0);
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("fx.outline.thickness", ParamValue::F32(2.0))
            .unwrap();
        let reg = a.world.res::<ParamRegistry>();
        let pe = a.world.res::<crate::PostEffects>();
        let (_, fx) = pe.for_pass("outline").unwrap();
        let mut out = [0.0f32; 12];
        let frame = UniformFrame {
            source_size: [0.0; 2],
            target_size: [640.0, 360.0],
            mip: 0,
        };
        assert_eq!(fx.write_uniforms("outline", &reg, &frame, &mut out), 12);
        assert_eq!(&out[..4], &[COLOR[0], COLOR[1], COLOR[2], STRENGTH]);
        assert_eq!(&out[4..8], &[2.0, DEPTH_SENSITIVITY, 1.0 / FAR, (FAR - NEAR) / (NEAR * FAR)]);
        assert_eq!(out[8], CREASE);
        assert!((out[9] - 2.0 * TAN_HALF_FOV / 360.0).abs() < 1e-9);
        assert_eq!(out[10], CREASE_FAR);
    }
}
