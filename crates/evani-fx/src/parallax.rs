//! Parallax layers (feature `parallax`): no render pass, params only. A 2D
//! game draws its background planes (far, mid, near) while
//! `EffectsStatus::is_on("parallax")`, each scrolled by its factor of the
//! camera movement and tiled without end; with the effect off it draws a
//! flat sky. Cost 1 (the planes are extra sprites), 2D and pseudo-3D looks
//! only. [`ParallaxSettings::scroll`] is the shared maths, so every game
//! scrolls its planes the same way.

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, RenderGraph};

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "parallax",
    cost: 1,
    value: 0.6,
    // Catalog works_with: 2d, pseudo-3d.
    styles: StyleClasses::TWO_D.union(StyleClasses::PSEUDO_3D),
    vr_comfort_off: false,
    description: "Background layers scrolling at different rates",
};

/// Defaults (also used when a param is missing).
pub const FAR: f32 = 0.15;
pub const MID: f32 = 0.4;
pub const NEAR: f32 = 0.7;
pub const VERTICAL: f32 = 0.3;

#[derive(Debug, Clone, Copy)]
pub struct ParallaxParams {
    pub far: F32Param,
    pub mid: F32Param,
    pub near: F32Param,
    pub vertical: F32Param,
}

/// Current values of the parallax params.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParallaxSettings {
    pub far: f32,
    pub mid: f32,
    pub near: f32,
    pub vertical: f32,
}

impl Default for ParallaxSettings {
    fn default() -> Self {
        Self {
            far: FAR,
            mid: MID,
            near: NEAR,
            vertical: VERTICAL,
        }
    }
}

impl ParallaxSettings {
    /// Read the `fx.parallax.*` params (defaults for any that are missing).
    pub fn from_registry(reg: &ParamRegistry) -> Self {
        let d = Self::default();
        let v = |n: &str, d: f32| reg.f32_handle(n).map_or(d, |h| reg.f32(h));
        Self {
            far: v("fx.parallax.far", d.far),
            mid: v("fx.parallax.mid", d.mid),
            near: v("fx.parallax.near", d.near),
            vertical: v("fx.parallax.vertical", d.vertical),
        }
    }

    /// Scroll factor of plane `layer` (0 far, 1 mid, 2 near).
    pub fn factor(&self, layer: usize) -> f32 {
        match layer {
            0 => self.far,
            1 => self.mid,
            _ => self.near,
        }
    }

    /// Offset of plane `layer` for a camera at `cam` (world units): the
    /// plane moves by `factor` of the camera horizontally and by
    /// `factor * vertical` vertically. Horizontal offsets wrap into
    /// `0..period` so a plane `period` wide tiles without end.
    pub fn scroll(&self, layer: usize, cam: [f32; 2], period: f32) -> [f32; 2] {
        let f = self.factor(layer);
        let x = cam[0] * f;
        let x = if period > 0.0 { x.rem_euclid(period) } else { x };
        [x, cam[1] * f * self.vertical]
    }
}

#[derive(Debug, Default)]
pub struct Parallax {
    pub params: Option<ParallaxParams>,
}

fn f(name: &str, d: f32, max: f32, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(0.0, max)
        .step(0.01)
        .unit("x")
        .describe(desc)
        .owner("fx.parallax")
        .bake(BakeTarget::Look)
}

impl Effect for Parallax {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(ParallaxParams {
            far: ctx.param_f32(f("fx.parallax.far", FAR, 1.0, "Scroll of the far plane (share of the camera movement)."))?,
            mid: ctx.param_f32(f("fx.parallax.mid", MID, 1.0, "Scroll of the middle plane."))?,
            near: ctx.param_f32(f("fx.parallax.near", NEAR, 1.0, "Scroll of the near plane."))?,
            vertical: ctx.param_f32(f(
                "fx.parallax.vertical",
                VERTICAL,
                1.0,
                "How much of each plane's scroll also applies vertically.",
            ))?,
        });
        Ok(())
    }

    /// No passes: the game draws its planes.
    fn passes(&self, _g: &mut RenderGraph) -> Result<(), GraphError> {
        Ok(())
    }
}

impl PostEffect for Parallax {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FxModule, FxStatus};
    use evani_core::App;
    use evani_core::effects::BudgetStatus;
    use evani_core::tiers::Tiers;
    use evani_render::RenderModule;

    #[test]
    fn params_only_costs_one_point_and_stays_on_in_vr() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Parallax::default()));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("parallax"));
        assert_eq!(a.world.res::<BudgetStatus>().used, 1);
        assert_eq!(
            ParallaxSettings::from_registry(&a.world.res::<ParamRegistry>()),
            ParallaxSettings::default()
        );
        // VR is only selectable inside an immersive session (main, 2026-10-14).
        a.world.res_mut::<Tiers>().set_xr_session(true);
        assert!(a.world.res_mut::<Tiers>().set_active("vr"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("parallax"), "not a comfort-off effect");
    }

    #[test]
    fn planes_scroll_by_their_factor_and_wrap() {
        let s = ParallaxSettings::default();
        let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        assert!(near(s.scroll(0, [100.0, 0.0], 0.0), [15.0, 0.0]));
        assert!(near(s.scroll(1, [100.0, 50.0], 0.0), [40.0, 50.0 * 0.4 * 0.3]));
        // Wraps into 0..period, also for negative camera positions.
        assert!((s.scroll(2, [1000.0, 0.0], 256.0)[0] - 700.0 % 256.0).abs() < 1e-3);
        let w = s.scroll(2, [-10.0, 0.0], 256.0)[0];
        assert!((w - (256.0 - 7.0)).abs() < 1e-4, "{w}");
        // Nearer planes move more.
        assert!(s.factor(0) < s.factor(1) && s.factor(1) < s.factor(2));
    }
}
