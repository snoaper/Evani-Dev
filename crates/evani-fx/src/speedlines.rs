//! Speed lines (feature `speedlines`): radial streaks from the centre of
//! each view, added into `hdr`, scaled by a per-player drive the game sets
//! from speed and boosts. Split-aware (one view, or two stacked views).
//! `fx.speedlines.fov_kick_deg` is read by the game for its camera FOV kick
//! while the effect is on. Comfort-off in VR.
//!
//! Game-driven params (never baked): `fx.speedlines.drive_a`,
//! `fx.speedlines.drive_b`, `fx.speedlines.split` and `fx.speedlines.phase`
//! (0..1; advance it every frame and wrap, the renderer has no clock for
//! effects).

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph};

use crate::{
    Blend, Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame, draw_effect,
    put_words,
};

pub static INFO: EffectInfo = EffectInfo {
    name: "speedlines",
    cost: 1,
    value: 0.5,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: true,
    description: "Radial speed streaks and a FOV kick",
};

pub const PASSES: &[&str] = &["speedlines"];

static SHADERS: [EffectShader; 1] = [EffectShader {
    name: "speedlines",
    pass: "speedlines",
    source: include_str!("../shaders/speedlines.wgsl"),
    slots: &[],
    blend: Blend::Additive,
    uniform_words: 8,
    input: false,
    depth: false,
}];

/// Defaults (also used when a param is missing).
pub const INTENSITY: f32 = 0.6;
pub const DENSITY: f32 = 48.0;
pub const INNER_RADIUS: f32 = 0.35;
pub const FOV_KICK_DEG: f32 = 8.0;

#[derive(Debug, Clone, Copy)]
pub struct SpeedlinesParams {
    pub intensity: F32Param,
    pub density: F32Param,
    pub inner_radius: F32Param,
    pub fov_kick_deg: F32Param,
    pub drive_a: F32Param,
    pub drive_b: F32Param,
    pub split: F32Param,
    pub phase: F32Param,
}

#[derive(Debug, Default)]
pub struct Speedlines {
    pub params: Option<SpeedlinesParams>,
}

#[allow(clippy::too_many_arguments)]
fn f(name: &str, d: f32, min: f32, max: f32, step: f32, unit: &str, bake: BakeTarget, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(step)
        .unit(unit)
        .describe(desc)
        .owner("fx.speedlines")
        .bake(bake)
        .gpu()
}

impl Effect for Speedlines {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        use BakeTarget::{Look, None as Live};
        self.params = Some(SpeedlinesParams {
            intensity: ctx.param_f32(f(
                "fx.speedlines.intensity",
                INTENSITY,
                0.0,
                2.0,
                0.01,
                "",
                Look,
                "Brightness of a streak at full drive.",
            ))?,
            density: ctx.param_f32(f(
                "fx.speedlines.density",
                DENSITY,
                8.0,
                128.0,
                1.0,
                "streaks",
                Look,
                "Streak slots around the circle (more = finer lines).",
            ))?,
            inner_radius: ctx.param_f32(f(
                "fx.speedlines.inner_radius",
                INNER_RADIUS,
                0.0,
                1.5,
                0.01,
                "view",
                Look,
                "Clear centre radius (1 = half the view height).",
            ))?,
            fov_kick_deg: ctx.param_f32(
                ParamDef::f32("fx.speedlines.fov_kick_deg", FOV_KICK_DEG)
                    .range(0.0, 20.0)
                    .step(0.5)
                    .unit("deg")
                    .describe("Extra camera field of view at full drive (the game applies it).")
                    .owner("fx.speedlines")
                    .bake(Look),
            )?,
            drive_a: ctx.param_f32(f(
                "fx.speedlines.drive_a",
                0.0,
                0.0,
                1.0,
                0.01,
                "",
                Live,
                "Streak strength for player 1 or the only view (set by the game).",
            ))?,
            drive_b: ctx.param_f32(f(
                "fx.speedlines.drive_b",
                0.0,
                0.0,
                1.0,
                0.01,
                "",
                Live,
                "Streak strength for player 2's view (set by the game).",
            ))?,
            split: ctx.param_f32(f(
                "fx.speedlines.split",
                0.0,
                0.0,
                1.0,
                1.0,
                "",
                Live,
                "0 = one full view, 1 = two stacked views (set by the game).",
            ))?,
            phase: ctx.param_f32(f(
                "fx.speedlines.phase",
                0.0,
                0.0,
                1.0,
                0.001,
                "",
                Live,
                "Animation phase 0..1, wrapping (advanced by the game each frame).",
            ))?,
        });
        Ok(())
    }

    /// Order 8: after the outlines, before sprites (HUD stays clean) and
    /// bloom. Lists `hdr` as an input only to be ordered after the forward
    /// pass; the shader samples nothing.
    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        g.add_pass(
            PassDesc::new("speedlines", "fx.speedlines")
                .reads(&["hdr"])
                .writes(&["hdr"])
                .order(8),
        )
    }
}

impl PostEffect for Speedlines {
    fn name(&self) -> &'static str {
        INFO.name
    }

    /// One uniform-only draw added into `hdr`; skipped while every drive is 0.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        if pass != "speedlines" {
            return;
        }
        if let Some(p) = self.params {
            let split = reg.f32(p.split) > 0.5;
            if reg.f32(p.drive_a) <= 0.0 && (!split || reg.f32(p.drive_b) <= 0.0) {
                return;
            }
        }
        draw_effect(self, rec, reg, 0, None, "hdr", 0);
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn write_uniforms(&self, shader: &str, reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        if shader != "speedlines" {
            return 0;
        }
        let p = self.params;
        let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
        let aspect = frame.target_size[0] / frame.target_size[1].max(1.0);
        put_words(
            out,
            &[
                v(p.map(|p| p.intensity), INTENSITY),
                v(p.map(|p| p.density), DENSITY),
                v(p.map(|p| p.inner_radius), INNER_RADIUS),
                v(p.map(|p| p.drive_a), 0.0),
                v(p.map(|p| p.drive_b), 0.0),
                v(p.map(|p| p.split), 0.0),
                aspect,
                v(p.map(|p| p.phase), 0.0),
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

    struct Count(usize);
    impl PostRecorder for Count {
        fn target_size(&self, _: &str) -> [f32; 2] {
            [800.0, 400.0]
        }
        fn draw(&mut self, _: usize, src: Option<&str>, dst: &str, uniforms: &[f32]) {
            assert!(src.is_none());
            assert_eq!(dst, "hdr");
            assert_eq!(uniforms.len(), 8);
            assert_eq!(uniforms[6], 2.0, "aspect");
            self.0 += 1;
        }
    }

    fn app() -> App {
        App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Speedlines::default()))
    }

    #[test]
    fn game_params_are_live_only_and_drive_the_draw() {
        let mut a = app();
        a.run_headless(1, 16.0);
        {
            let reg = a.world.res::<ParamRegistry>();
            for n in ["fx.speedlines.drive_a", "fx.speedlines.drive_b", "fx.speedlines.split", "fx.speedlines.phase"] {
                assert_eq!(reg.def(reg.id(n).unwrap()).unwrap().bake, BakeTarget::None, "{n}");
            }
            assert_eq!(reg.f32(reg.f32_handle("fx.speedlines.fov_kick_deg").unwrap()), FOV_KICK_DEG);
        }
        // A stand-alone instance without params still draws (defaults).
        let fx = Speedlines::default();
        let mut rec = Count(0);
        fx.record("speedlines", &a.world.res::<ParamRegistry>(), &mut rec);
        assert_eq!(rec.0, 1);

        // The registered instance skips the draw while every drive is 0.
        let pe = a.world.res::<crate::PostEffects>().clone();
        let (_, live) = pe.for_pass("speedlines").unwrap();
        let mut rec = Count(0);
        live.record("speedlines", &a.world.res::<ParamRegistry>(), &mut rec);
        assert_eq!(rec.0, 0);
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("fx.speedlines.drive_b", ParamValue::F32(0.8))
            .unwrap();
        live.record("speedlines", &a.world.res::<ParamRegistry>(), &mut rec);
        assert_eq!(rec.0, 0, "drive_b only counts in split view");
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("fx.speedlines.split", ParamValue::F32(1.0))
            .unwrap();
        live.record("speedlines", &a.world.res::<ParamRegistry>(), &mut rec);
        assert_eq!(rec.0, 1);
    }

    #[test]
    fn comfort_off_in_vr() {
        let mut a = app();
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("speedlines"));
        a.world.res_mut::<Tiers>().set_xr_session(true);
        assert_eq!(a.world.res::<Tiers>().active().id, "vr");
        a.run_headless(1, 16.0);
        assert!(!a.world.res::<FxStatus>().is_on("speedlines"));
        assert!(!a.world.res::<RenderGraph>().pass("speedlines").unwrap().enabled);
    }
}
