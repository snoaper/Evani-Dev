//! Trails (feature `trails`): ribbons and trails behind vehicles, blades and
//! projectiles. Cost 1, as in the catalog.
//!
//! Kits feed `evani_core::trails::TrailQueue` from their fixed step; the
//! feature turns on `evani-render/trails`, which keeps a ring of 64 points per
//! emitter, ages them in the `trails.sim` compute pass and draws each strip
//! from neighbouring points in the vertex shader. Toggled off (by the budget
//! or live), the pass is disabled: points are dropped, nothing draws and the
//! game itself never changes.
//!
//! The strip's look over its life comes from two curves
//! (`fx.trails.width_curve`, `fx.trails.alpha_curve`; x is the part of the
//! life gone, 0 to 1), the live API's `curve.set`, and bakes into the look.
//! Tier caps: 64 / 32 / 16 / 8 emitters on Ultra / High / Medium / Low.

use evani_core::curve::Curve;
use evani_core::formulas::Formulas;
use evani_core::params::{BakeTarget, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph, TargetDesc, TargetKind};
use evani_render::trails::{BUFFER_TARGET, CURVE_ALPHA, CURVE_WIDTH, FADE, PARAM_FADE, PARAM_LIFE, PARAM_MAX, PARAM_SOFT, PARAM_WIDTH, SIM_PASS, SOFT};

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "trails",
    cost: 1,
    value: 0.5,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: false,
    description: "Ribbons and trails behind vehicles, blades and projectiles",
};

/// The lowest quality level each parameter shows on: all of them on every
/// level; Low only draws the strip's points without rounding the corners.
pub const SHOWS_FROM: &[(&str, &str)] = &[
    (PARAM_WIDTH, "low"),
    (PARAM_LIFE, "low"),
    (PARAM_FADE, "low"),
    (PARAM_SOFT, "low"),
    (PARAM_MAX, "low"),
    (CURVE_WIDTH, "low"),
    (CURVE_ALPHA, "low"),
];

/// The effect's params: name, default, range, step, unit, what it does.
const PARAMS: [(&str, f32, f32, f32, f32, &str, &str); 5] = [
    (PARAM_WIDTH, 1.0, 0.0, 4.0, 0.05, "x", "Strip width, a multiple of the game's."),
    (PARAM_LIFE, 1.0, 0.0, 4.0, 0.05, "x", "Point life, a multiple of the game's."),
    (PARAM_FADE, FADE, 0.0, 1.0, 0.01, "part", "Last part of a point's life that fades."),
    (PARAM_SOFT, SOFT, 0.0, 1.0, 0.01, "part", "Part of the half width that fades at the edges."),
    (PARAM_MAX, 64.0, 0.0, 64.0, 1.0, "count", "Most emitters (the tier caps it too)."),
];

#[derive(Debug, Default)]
pub struct Trails;

/// A life curve through `points` (x: the part of a point's life gone). Built
/// point by point because `Curve::new` would bring its sort into the download;
/// the points are fixed and in order, so the inserts cannot fail.
fn life_curve(ctx: &mut PluginContext, name: &str, what: &str, points: &[[f32; 2]]) {
    let mut c = Curve::constant(1.0);
    for p in points {
        let _ = c.insert_point(*p);
    }
    ctx.world.res_mut::<Formulas>().add_life_curve(name, what, c);
}

impl Effect for Trails {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        for (name, d, min, max, step, unit, what) in PARAMS {
            ctx.param_f32(ParamDef::f32(name, d).range(min, max).step(step).unit(unit).describe(what).owner("fx.trails").bake(BakeTarget::Look))?;
        }
        life_curve(ctx, CURVE_WIDTH, "Width over a point's life.", &[[0.6, 0.8], [1.0, 0.15]]);
        life_curve(ctx, CURVE_ALPHA, "Opacity over a point's life.", &[[1.0, 1.0]]);
        Ok(())
    }

    /// The `trails.sim` compute pass writes the buffer the forward pass
    /// (world strips) and the sprites pass (screen strips) draw from, so the
    /// graph runs it before both.
    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        g.add_target(TargetDesc::new(BUFFER_TARGET, TargetKind::Buffer))?;
        g.add_pass(PassDesc::compute(SIM_PASS, "fx.trails").writes(&[BUFFER_TARGET]))?;
        for pass in ["forward", "sprites"] {
            g.add_input(pass, BUFFER_TARGET);
        }
        Ok(())
    }
}

impl PostEffect for Trails {
    fn name(&self) -> &'static str {
        INFO.name
    }

    fn pass_names(&self) -> &'static [&'static str] {
        &[SIM_PASS]
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
    use evani_core::trails::{TrailQueue, TrailStyle};
    use evani_render::RenderModule;
    use evani_render::trails::{TrailSystem, tier_cap};

    fn app() -> App {
        App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Trails))
    }

    #[test]
    fn costs_what_the_catalog_says_and_registers_its_params() {
        let catalog = include_str!("../../../catalog/effects.json");
        let at = catalog.find("\"id\": \"trails\"").unwrap();
        assert!(catalog[at..].lines().find(|l| l.contains("cost_points")).unwrap().contains(&format!(": {},", INFO.cost)));
        let mut a = app();
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("trails"), "starts on wherever it is listed");
        assert_eq!(a.world.res::<BudgetStatus>().used, 1);
        let reg = a.world.res::<ParamRegistry>();
        for (n, d) in [(PARAM_WIDTH, 1.0), (PARAM_LIFE, 1.0), (PARAM_FADE, FADE), (PARAM_SOFT, SOFT), (PARAM_MAX, 64.0)] {
            let h = reg.f32_handle(n).unwrap();
            assert_eq!(reg.f32(h), d, "{n}");
            let def = reg.def(reg.id(n).unwrap()).unwrap();
            assert_eq!(def.owner, "fx.trails", "{n}");
            assert_eq!(def.bake, BakeTarget::Look, "{n} bakes into the look");
            assert!(SHOWS_FROM.iter().any(|(s, _)| *s == n), "{n} has a level label");
        }
        drop(reg);
        // The curves are live formulas with bounded x and y.
        let f = a.world.res::<Formulas>();
        for n in [CURVE_WIDTH, CURVE_ALPHA] {
            let id = f.id(n).unwrap();
            assert_eq!(f.def(id).unwrap().x_range, Some([0.0, 1.0]), "{n}");
            assert!(SHOWS_FROM.iter().any(|(s, _)| *s == n), "{n} has a level label");
        }
        drop(f);
        // Its compute pass runs before the passes that draw strips.
        let g = a.world.res::<RenderGraph>();
        assert!(g.pass(SIM_PASS).is_some_and(|p| p.enabled));
        let order: Vec<&str> = g.ordered().map(|p| p.name).collect();
        let at = |n: &str| order.iter().position(|p| *p == n).unwrap();
        assert!(at(SIM_PASS) < at("sprites"), "{order:?}");
        assert_eq!(order.contains(&"forward"), evani_render::FORWARD_PATH, "{order:?}");
        if evani_render::FORWARD_PATH {
            assert!(at(SIM_PASS) < at("forward"), "{order:?}");
        }
    }

    #[test]
    fn the_tier_caps_the_emitters_and_the_param_lowers_them() {
        let mut a = app();
        for (id, cap) in [("ultra", 64), ("high", 32), ("medium", 16), ("low", 8), ("lite", 8), ("vr", 16)] {
            assert!(a.world.res_mut::<Tiers>().set_active_preset_run(id));
            a.run_headless(1, 16.0);
            assert_eq!(a.world.res::<TrailSystem>().cap(), cap, "{id}");
            assert_eq!(tier_cap(id), cap, "{id}");
        }
        a.world.res_mut::<ParamRegistry>().set_by_name(PARAM_MAX, evani_core::params::ParamValue::F32(3.0)).unwrap();
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<TrailSystem>().cap(), 3);
    }

    #[test]
    fn toggled_off_nothing_is_kept_and_the_strip_breaks() {
        let mut a = app();
        assert!(a.world.res_mut::<Tiers>().set_active("high"));
        let feed = |a: &mut App, n: u32| {
            for i in 0..n {
                {
                    let mut q = a.world.res_mut::<TrailQueue>();
                    q.style(1, TrailStyle::default());
                    q.point(1, [i as f32 * 0.1, 0.0, 0.0]);
                }
                a.run_headless(1, 1000.0 / 60.0);
            }
        };
        feed(&mut a, 5);
        assert_eq!(a.world.res::<TrailSystem>().live(), 1);
        a.world.res_mut::<ParamRegistry>().set_by_name("fx.trails.enabled", evani_core::params::ParamValue::Bool(false)).unwrap();
        feed(&mut a, 60);
        let s = a.world.res::<TrailSystem>();
        assert!(!s.is_on());
        assert_eq!((s.live(), s.groups().len()), (0, 0), "the strip aged out and no point joined it");
    }
}
