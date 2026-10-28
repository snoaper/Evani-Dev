//! Evani.Dev effects framework (optional module `evani.module.fx`).
//!
//! * [`Effect`]: what every post effect declares: its [`EffectInfo`] (name,
//!   cost in points, value weight, style classes, VR comfort), its tunable
//!   params, the render-graph passes it adds, and its WGSL (one file per
//!   pass, `include_str!`ed so a compiled-out effect ships no shader text).
//! * [`FxModule`]: registers `fx.<name>.enabled` and `fx.<name>.value_weight`
//!   for every compiled-in effect plus the effect's own params, sets the
//!   start state from the active tier (`effects_on`) and the look, runs the
//!   core budget calculator ([`plan_budget`]) whenever the tier, a toggle or
//!   a weight changes, enables or disables the effect's passes in the
//!   [`RenderGraph`] (shape unchanged, so no rebuild) and reports in the
//!   overlay.
//!
//! Every effect is its own Cargo feature (`bloom`, `vignette`, `outline`,
//! `speedlines`, `shake`, `juice`, `particles`, `parallax`, `shadows`, `lights2d`); nothing is on by default. [`FxModule::compiled`]
//! adds exactly the effects this build contains, so settings never show an
//! effect that isn't in the download.

#[cfg(feature = "bloom")]
pub mod bloom;
#[cfg(feature = "vignette")]
pub mod vignette;
#[cfg(feature = "outline")]
pub mod outline;
#[cfg(feature = "speedlines")]
pub mod speedlines;
#[cfg(feature = "shake")]
pub mod shake;
#[cfg(feature = "juice")]
pub mod juice;
#[cfg(feature = "particles")]
pub mod particles;
#[cfg(feature = "parallax")]
pub mod parallax;
#[cfg(feature = "shadows")]
pub mod shadows; #[cfg(feature = "lights2d")] pub mod lights2d; #[cfg(feature = "fog")] pub mod fog; #[cfg(feature = "decals")] pub mod decals; #[cfg(feature = "grade")] pub mod grade; #[cfg(feature = "look-effects")] mod look_effects;

use core::fmt::Write;

use evani_core::ecs::World;
use evani_core::effects::{
    BudgetItem, BudgetStatus, EffectState, EffectStatus, EffectWant, EffectsStatus, plan_budget,
};
use evani_core::overlay::ErrorLog;
use evani_core::params::{BakeTarget, BoolParam, F32Param, ParamDef, ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::{EngineModule, LookData, PluginContext, PluginError};
use evani_core::quality::QualityState;
use evani_core::tiers::Tiers;
use evani_render::graph::{GraphError, RenderGraph};

pub use evani_core::effects::{EffectInfo, EffectsStatus as FxStatus, StyleClasses};

// The renderer-facing half lives in evani-render (it draws the effects);
// re-exported so effects only depend on this crate's prelude.
pub use evani_render::post::{
    Blend, EffectShader, PostEffect, PostEffects, PostRecorder, UniformFrame, draw_effect, put_words,
};

use std::rc::Rc;

/// A post effect. Implementations live behind their own Cargo feature.
///
/// The [`PostEffect`] supertrait is what the renderer sees: pass names,
/// WGSL files, uniform layout and `record` (the draws of each pass). This
/// trait adds what the effects module needs: the budget info, the params and
/// the graph passes.
pub trait Effect: PostEffect {
    fn info(&self) -> &'static EffectInfo;
    /// Register the effect's own params (owner `fx.<name>`). The module
    /// registers `fx.<name>.enabled` and `fx.<name>.value_weight` itself.
    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError>;
    /// Add targets and passes to the render graph.
    fn passes(&self, graph: &mut RenderGraph) -> Result<(), GraphError>; #[doc = " Once a frame, after the budget is planned: for effects that follow the camera or the world (fog's slot body, decals' ageing, grade's look hand-off). Default: nothing."] #[cfg(any(feature = "fog", feature = "decals", feature = "grade"))] fn frame(&self, _world: &World) {}
}

struct Entry {
    /// Shared with the renderer's [`PostEffects`] after `build`.
    effect: Rc<dyn Effect>,
    enabled: Option<BoolParam>,
    weight: Option<F32Param>,
}

/// The effects module. Add it after the render module (it adds passes to
/// the [`RenderGraph`] the render module inserts).
pub struct FxModule {
    entries: Vec<Entry>,
    items: Vec<BudgetItem<'static>>,
    states: Vec<EffectState>,
    watch: Vec<ParamId>,
    last_tier: Option<usize>,
    seen_rev: u64,
    /// [`QualityState::revision`] last planned with.
    seen_quality: u64,
}

impl Default for FxModule {
    fn default() -> Self {
        Self::new()
    }
}

impl FxModule {
    /// A module with no effects (add them with [`FxModule::with_effect`]).
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            items: Vec::new(),
            states: Vec::new(),
            watch: Vec::new(),
            last_tier: None,
            seen_rev: 0,
            seen_quality: 0,
        }
    }

    /// Every effect compiled into this build, in a fixed order.
    pub fn compiled() -> Self {
        #[allow(unused_mut)]
        let mut m = Self::new();
        #[cfg(feature = "bloom")]
        {
            m = m.with_effect(bloom::Bloom::default());
        }
        #[cfg(feature = "vignette")]
        {
            m = m.with_effect(vignette::Vignette::default());
        }
        #[cfg(feature = "outline")]
        {
            m = m.with_effect(outline::Outline::default());
        }
        #[cfg(feature = "speedlines")]
        {
            m = m.with_effect(speedlines::Speedlines::default());
        }
        #[cfg(feature = "shake")]
        {
            m = m.with_effect(shake::Shake::default());
        }
        #[cfg(feature = "juice")]
        {
            m = m.with_effect(juice::Juice::default());
        }
        #[cfg(feature = "particles")]
        {
            m = m.with_effect(particles::Particles::default());
        }
        #[cfg(feature = "parallax")]
        {
            m = m.with_effect(parallax::Parallax::default());
        }
        #[cfg(feature = "shadows")]
        {
            m = m.with_effect(shadows::Shadows);
        } #[cfg(feature = "lights2d")] { m = m.with_effect(lights2d::Lights2d); } #[cfg(feature = "fog")] { m = m.with_effect(fog::Fog::default()); } #[cfg(feature = "decals")] { m = m.with_effect(decals::Decals::default()); } #[cfg(feature = "grade")] { m = m.with_effect(grade::Grade::default()); }
        m
    }

    /// Names of the effects compiled into this build.
    pub const COMPILED: &'static [&'static str] = &[
        #[cfg(feature = "bloom")]
        "bloom",
        #[cfg(feature = "vignette")]
        "vignette",
        #[cfg(feature = "outline")]
        "outline",
        #[cfg(feature = "speedlines")]
        "speedlines",
        #[cfg(feature = "shake")]
        "shake",
        #[cfg(feature = "juice")]
        "juice",
        #[cfg(feature = "particles")]
        "particles",
        #[cfg(feature = "parallax")]
        "parallax",
        #[cfg(feature = "shadows")]
        "shadows", #[cfg(feature = "lights2d")] "lights2d", #[cfg(feature = "fog")] "fog", #[cfg(feature = "decals")] "decals", #[cfg(feature = "grade")] "grade",
    ];

    pub fn with_effect(mut self, effect: impl Effect) -> Self {
        self.entries.push(Entry {
            effect: Rc::new(effect),
            enabled: None,
            weight: None,
        });
        self
    }

    pub fn effect_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|e| e.effect.info().name)
    }

    /// Set `fx.<name>.enabled` from the active tier's `effects_on`; a look
    /// that prefers an effect off vetoes it (it cannot force one on: the
    /// tier decides what starts on).
    fn apply_tier_defaults(&self, world: &World) {
        let tiers = world.res::<Tiers>();
        let tier = tiers.active();
        let look = world.res::<LookData>();
        let mut reg = world.res_mut::<ParamRegistry>();
        for e in &self.entries {
            let name = e.effect.info().name;
            let mut on = tier.effects_on.iter().any(|n| n == name); #[cfg(feature = "lights2d")] if name == lights2d::INFO.name { on |= lights2d::tier_on(&tier.id); } #[cfg(feature = "look-effects")] { on |= look_effects::requested(world, name, tier); } // 2D lights: on where the tier gives a count; look-effects: the look asked for it (its veto below and the budget still apply)
            if let Some((_, pref)) = look.post_effects.iter().rev().find(|(n, _)| n == name) {
                on &= *pref;
            }
            if let Some(h) = e.enabled {
                let _ = reg.set(h.id(), ParamValue::Bool(on));
            }
        }
    }

    /// Run the budget calculator and apply the result. No allocation unless
    /// an effect changes state (then one overlay note).
    fn replan(&mut self, world: &World) {
        let quality = world.try_res::<QualityState>();
        {
            let reg = world.res::<ParamRegistry>();
            for (item, e) in self.items.iter_mut().zip(&self.entries) {
                item.value = e.weight.map_or(item.info.value, |h| reg.f32(h));
                let on = e.enabled.is_some_and(|h| reg.bool(h));
                // Shed by the adaptive-quality controller: off, and its
                // points are free for the others.
                let shed = quality.as_ref().is_some_and(|q| q.is_shed(item.info.name));
                item.want = if on && !shed { EffectWant::On } else { EffectWant::Off };
            }
        }
        self.seen_quality = quality.as_ref().map_or(0, |q| q.revision());
        let status = {
            let tiers = world.res::<Tiers>();
            let tier = tiers.active();
            let look = world.res::<LookData>();
            plan_budget(
                tier.points,
                tier.stereo,
                look.points,
                // No look registered: don't filter effects by style.
                if look.styles.0 == 0 { StyleClasses::ALL } else { look.styles },
                &self.items,
                &mut self.states,
            )
        };
        if let Some(q) = &quality {
            let reg = world.res::<ParamRegistry>();
            for ((e, st), item) in self.entries.iter().zip(&mut self.states).zip(&self.items) {
                let on = e.enabled.is_some_and(|h| reg.bool(h));
                if on && *st == EffectState::OffByRequest && q.is_shed(item.info.name) {
                    *st = EffectState::OffByQuality;
                }
            }
        }
        drop(quality);
        *world.res_mut::<BudgetStatus>() = status;
        if let Some(mut g) = world.try_res_mut::<RenderGraph>() {
            for (e, st) in self.entries.iter().zip(&self.states) {
                for pass in e.effect.pass_names() {
                    g.set_enabled(pass, st.is_on());
                }
            }
        }
        if let Some(mut fs) = world.try_res_mut::<EffectsStatus>() {
            for (s, st) in fs.effects.iter_mut().zip(&self.states) {
                if s.state != *st && *st == EffectState::OffByBudget {
                    world.res_mut::<ErrorLog>().note(format!(
                        "fx: {} off, over the {} point budget",
                        s.name(), status.total
                    ));
                }
                if s.state != *st && *st == EffectState::OffByQuality {
                    world.res_mut::<ErrorLog>().note(format!(
                        "fx: {} off to hold the frame rate (adaptive quality)",
                        s.name()
                    ));
                }
                s.state = *st;
            }
        }
    }
}

impl EngineModule for FxModule {
    fn id(&self) -> &'static str {
        "evani.module.fx"
    }

    fn label(&self) -> &'static str {
        "fx"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// The effects this build compiled in (each is its own Cargo feature).
    fn version_features(&self) -> Vec<&'static str> {
        self.entries.iter().map(|e| e.effect.info().name).collect()
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let mut status = EffectsStatus::default();
        let mut errors = Vec::new();
        for e in &mut self.entries {
            let info = e.effect.info();
            let owner = format!("fx.{}", info.name);
            let enabled = ctx.param_bool(
                ParamDef::bool(&format!("{owner}.enabled"), true)
                    .describe(&format!(
                        "{} on or off (the tier budget can still switch it off)",
                        info.name
                    ))
                    .owner(&owner)
                    .bake(BakeTarget::None),
            );
            let weight = ctx.param_f32(
                ParamDef::f32(&format!("{owner}.value_weight"), info.value)
                    .range(0.0, 10.0)
                    .step(0.05)
                    .unit("value")
                    .describe(&format!(
                        "Visual value of {}; the budget drops the lowest value per point first",
                        info.name
                    ))
                    .owner(&owner)
                    .bake(BakeTarget::Policy),
            );
            match (&enabled, &weight) {
                (Ok(_), Ok(_)) => {}
                (Err(x), _) | (_, Err(x)) => errors.push(format!("{owner}: {x}")),
            }
            e.enabled = enabled.ok();
            e.weight = weight.ok();
            match Rc::get_mut(&mut e.effect) {
                Some(effect) => {
                    if let Err(x) = effect.register(ctx) {
                        errors.push(format!("{owner}: {x}"));
                    }
                }
                None => errors.push(format!("{owner}: built twice")),
            }
            // The renderer draws it from here on.
            match ctx.world.try_res_mut::<PostEffects>() {
                Some(mut pe) => {
                    let post: Rc<dyn PostEffect> = e.effect.clone();
                    pe.effects.push(post);
                }
                None => errors.push(format!(
                    "{owner}: no post-effect list (add the render module before fx)"
                )),
            }
            let graph = ctx.world.try_res_mut::<RenderGraph>();
            match graph {
                Some(mut g) => {
                    if let Err(x) = e.effect.passes(&mut g) {
                        errors.push(format!("{owner}: {x}"));
                    }
                }
                None => errors.push(format!(
                    "{owner}: no render graph (add the render module before fx)"
                )),
            }
            self.items.push(BudgetItem {
                info,
                value: info.value,
                want: EffectWant::Off,
            });
            self.states.push(EffectState::OffByRequest);
            self.watch.extend(e.enabled.map(|h| h.id()));
            self.watch.extend(e.weight.map(|h| h.id()));
            status.effects.push(EffectStatus {
                info,
                enabled: e.enabled,
                weight: e.weight,
                passes: e.effect.pass_names(),
                state: EffectState::OffByRequest,
            });
        }
        ctx.resource(status);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(PluginError(errors.join("; ")))
        }
    }

    fn start(&mut self, world: &mut World) {
        self.last_tier = Some(world.res::<Tiers>().active_index());
        self.apply_tier_defaults(world);
        self.replan(world);
        self.seen_rev = world.res::<ParamRegistry>().revision();
    }

    fn frame(&mut self, world: &mut World) {
        let idx = world.res::<Tiers>().active_index();
        let mut dirty = false;
        if self.last_tier != Some(idx) {
            self.last_tier = Some(idx);
            self.apply_tier_defaults(world);
            dirty = true;
        }
        {
            let reg = world.res::<ParamRegistry>();
            if reg.revision() != self.seen_rev {
                dirty |= reg.any_changed_since(&self.watch, self.seen_rev);
            }
        }
        if let Some(q) = world.try_res::<QualityState>() {
            dirty |= q.revision() != self.seen_quality;
        }
        if dirty {
            self.replan(world);
        }
        self.seen_rev = world.res::<ParamRegistry>().revision(); #[cfg(any(feature = "fog", feature = "decals", feature = "grade"))] for e in &self.entries { e.effect.frame(world); }
    }

    fn status(&self, world: &World, out: &mut String) {
        if self.entries.is_empty() {
            out.push_str("no effects compiled in");
        }
        for (i, (e, st)) in self.entries.iter().zip(&self.states).enumerate() {
            if i > 0 {
                out.push_str(" · ");
            }
            out.push_str(e.effect.info().name);
            out.push(' ');
            out.push_str(st.as_str());
        }
        let b = *world.res::<BudgetStatus>();
        let _ = write!(out, " · {}/{} pts", b.used, b.total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::effects::StyleClasses;
    use evani_core::plugin::{LookContext, LookPlugin};
    use evani_render::graph::{PassDesc, TargetDesc, TargetKind};
    use evani_render::RenderModule;

    static EXPENSIVE: EffectInfo = EffectInfo {
        name: "big",
        cost: 3,
        value: 0.8,
        styles: StyleClasses::ALL,
        vr_comfort_off: false,
        description: "test",
    };
    static CHEAP: EffectInfo = EffectInfo {
        name: "small",
        cost: 1,
        value: 0.3,
        styles: StyleClasses::ALL,
        vr_comfort_off: true,
        description: "test",
    };

    struct Fake(&'static EffectInfo, &'static [&'static str]);
    impl Effect for Fake {
        fn info(&self) -> &'static EffectInfo {
            self.0
        }
        fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
            ctx.param_f32(ParamDef::f32(&format!("fx.{}.amount", self.0.name), 1.0).range(0.0, 2.0))?;
            Ok(())
        }
        fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
            g.add_pass(PassDesc::new(self.1[0], "fx.test").reads(&["ldr"]).writes(&["ldr"]).order(200))
        }
    }
    impl PostEffect for Fake {
        fn name(&self) -> &'static str {
            self.0.name
        }
        fn pass_names(&self) -> &'static [&'static str] {
            self.1
        }
        fn shaders(&self) -> &'static [EffectShader] {
            &[]
        }
        fn write_uniforms(&self, _: &str, _: &ParamRegistry, _: &UniformFrame, _: &mut [f32]) -> usize {
            0
        }
        fn record(&self, _: &str, _: &ParamRegistry, _: &mut dyn PostRecorder) {}
    }

    struct Look(u32);
    impl LookPlugin for Look {
        fn name(&self) -> &'static str {
            "t"
        }
        fn points(&self) -> u32 {
            self.0
        }
        fn build(&self, _look: &mut LookContext) -> Result<(), PluginError> {
            Ok(())
        }
    }

    fn app(look_points: u32) -> App {
        let mut tiers = Tiers::builtin().unwrap();
        // Tests pin both effects into every tier's start list.
        for p in tiers.presets().to_vec() {
            let mut p = p;
            p.effects_on = vec!["big".into(), "small".into()];
            tiers.upsert(p);
        }
        let mut a = App::new("t");
        a.world.insert_resource(tiers);
        a.with_look(Look(look_points))
            .with_module(RenderModule::new())
            .with_module(
                FxModule::new()
                    .with_effect(Fake(&EXPENSIVE, &["big"]))
                    .with_effect(Fake(&CHEAP, &["small"])),
            )
    }

    fn set_tier(a: &mut App, id: &str) {
        assert!(a.world.res_mut::<Tiers>().set_active_preset_run(id));
        a.run_headless(1, 16.0);
    }

    #[test]
    fn registers_params_and_passes() {
        let mut a = app(0);
        a.run_headless(1, 16.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in ["fx.big.enabled", "fx.big.value_weight", "fx.big.amount", "fx.small.enabled"] {
            assert!(reg.id(n).is_some(), "{n}");
        }
        let d = reg.def(reg.id("fx.big.value_weight").unwrap()).unwrap();
        assert_eq!(d.owner, "fx.big");
        assert_eq!(d.bake, BakeTarget::Policy);
        drop(reg);
        let g = a.world.res::<RenderGraph>();
        assert!(g.pass("big").is_some() && g.pass("small").is_some());
        assert_eq!(a.world.res::<ErrorLog>().error_count(), 0);
    }

    #[test]
    fn tiers_switch_effects_by_budget() {
        let mut a = app(4);
        set_tier(&mut a, "high"); // 16 points: 4 + 3 + 1 fits.
        assert!(a.world.res::<FxStatus>().is_on("big"));
        assert!(a.world.res::<FxStatus>().is_on("small"));
        assert_eq!(*a.world.res::<BudgetStatus>(), BudgetStatus { used: 8, total: 16 });

        set_tier(&mut a, "low"); // 7 points: 4 + 3 + 1 = 8 -> big (0.27/pt) goes.
        let fx = a.world.res::<FxStatus>();
        assert_eq!(fx.get("big").unwrap().state, EffectState::OffByBudget);
        assert!(fx.is_on("small"));
        drop(fx);
        assert!(!a.world.res::<RenderGraph>().pass("big").unwrap().enabled);
        assert!(a.world.res::<RenderGraph>().pass("small").unwrap().enabled);
        assert_eq!(a.world.res::<BudgetStatus>().used, 5);

        set_tier(&mut a, "vr"); // stereo: small is comfort-off.
        assert_eq!(
            a.world.res::<FxStatus>().get("small").unwrap().state,
            EffectState::OffByVrComfort
        );
    }

    #[test]
    fn live_toggle_and_weight_replan_next_frame() {
        let mut a = app(4);
        set_tier(&mut a, "low");
        assert!(!a.world.res::<FxStatus>().is_on("big"));
        // Raise big's weight: 3.0/3 = 1.0 per point beats small's 0.3.
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("fx.big.value_weight", ParamValue::F32(3.0))
            .unwrap();
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("big"));
        assert!(!a.world.res::<FxStatus>().is_on("small"));
        // Player turns big off: small fits again.
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("fx.big.enabled", ParamValue::Bool(false))
            .unwrap();
        a.run_headless(1, 16.0);
        let fx = a.world.res::<FxStatus>();
        assert_eq!(fx.get("big").unwrap().state, EffectState::OffByRequest);
        assert!(fx.is_on("small"));
        drop(fx);
        let mut s = String::new();
        let fxm = FxModule::new();
        fxm.status(&a.world, &mut s);
        assert!(s.contains("pts"), "{s}");
    }

    #[test]
    fn effects_reach_the_renderer() {
        let a = app(0);
        let pe = a.world.res::<PostEffects>();
        let names: Vec<_> = pe.effects.iter().map(|e| e.name()).collect();
        assert_eq!(names, ["big", "small"]);
        assert_eq!(pe.for_pass("small").map(|(i, _)| i), Some(1));
    }

    #[test]
    fn quality_shed_turns_effects_off_and_back() {
        let mut a = app(4);
        set_tier(&mut a, "high");
        assert!(a.world.res::<FxStatus>().is_on("small"));
        a.world
            .res_mut::<QualityState>()
            .apply(evani_core::quality::AqAction::ShedEffect("small"));
        a.run_headless(1, 16.0);
        assert_eq!(
            a.world.res::<FxStatus>().get("small").unwrap().state,
            EffectState::OffByQuality
        );
        assert!(!a.world.res::<RenderGraph>().pass("small").unwrap().enabled);
        assert_eq!(a.world.res::<BudgetStatus>().used, 7);
        a.world.res_mut::<QualityState>().reset();
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("small"));
    }

    #[cfg(all(feature = "parallax", feature = "shake"))]
    struct Look2d;
    #[cfg(all(feature = "parallax", feature = "shake"))]
    impl LookPlugin for Look2d {
        fn name(&self) -> &'static str {
            "t2d"
        }
        fn points(&self) -> u32 {
            1
        }
        fn styles(&self) -> StyleClasses {
            StyleClasses::TWO_D
        }
        fn build(&self, _look: &mut LookContext) -> Result<(), PluginError> {
            Ok(())
        }
    }

    /// Kit 002 (SPEC §5): a 1-point 2D look with parallax and shake from
    /// the real tier start lists; VR keeps parallax (not comfort-off) and
    /// drops shake. A 3D look switches parallax off by style.
    #[cfg(all(feature = "parallax", feature = "shake"))]
    #[test]
    fn platformer_tier_budget() {
        let fx = || {
            FxModule::new()
                .with_effect(parallax::Parallax::default())
                .with_effect(shake::Shake::default())
        };
        let mut a = App::new("t").with_look(Look2d).with_module(RenderModule::new()).with_module(fx());
        for (tier, on) in [
            ("lite", &["parallax", "shake"][..]),
            ("low", &["parallax", "shake"][..]),
            ("medium", &["parallax", "shake"][..]),
            ("high", &["parallax", "shake"][..]),
            ("ultra", &["parallax", "shake"][..]),
            ("vr", &["parallax"][..]),
        ] {
            set_tier(&mut a, tier);
            let st = a.world.res::<FxStatus>();
            for e in ["parallax", "shake"] {
                assert_eq!(st.is_on(e), on.contains(&e), "{tier}: {e}");
            }
            assert_eq!(a.world.res::<BudgetStatus>().used, 2, "{tier}");
        }
        let mut b = App::new("t").with_look(Look(1)).with_module(RenderModule::new()).with_module(fx());
        set_tier(&mut b, "high");
        assert!(!b.world.res::<FxStatus>().is_on("parallax"), "3D look: off by style");
    }

    /// Kit 001 (SPEC §5): a 2-point look with outline, speedlines, shake
    /// and bloom from the real tier start lists. Bloom costs 3 points here
    /// (the catalog and SPEC table say 2), so the totals are one higher;
    /// every tier still fits. Shadows (2) start on from Medium up; Medium
    /// then uses 10 of its 11 points and VR all 9.
    #[cfg(all(
        feature = "outline",
        feature = "speedlines",
        feature = "shake",
        feature = "bloom",
        feature = "shadows"
    ))]
    #[test]
    fn kart_racer_tier_budget() {
        let mut a = App::new("t").with_look(Look(2)).with_module(RenderModule::new()).with_module(
            FxModule::new()
                .with_effect(outline::Outline::default())
                .with_effect(speedlines::Speedlines::default())
                .with_effect(shake::Shake::default())
                .with_effect(bloom::Bloom::default())
                .with_effect(shadows::Shadows),
        );
        for (tier, on, used) in [
            ("lite", &["outline", "shake"][..], 4),
            ("low", &["outline", "speedlines", "shake"][..], 5),
            ("medium", &["outline", "speedlines", "shake", "bloom", "shadows"][..], 10),
            ("high", &["outline", "speedlines", "shake", "bloom", "shadows"][..], 10),
            ("ultra", &["outline", "speedlines", "shake", "bloom", "shadows"][..], 10),
            ("vr", &["outline", "bloom", "shadows"][..], 9),
        ] {
            set_tier(&mut a, tier);
            let fx = a.world.res::<FxStatus>();
            for e in ["outline", "speedlines", "shake", "bloom", "shadows"] {
                assert_eq!(fx.is_on(e), on.contains(&e), "{tier}: {e}");
            }
            assert_eq!(a.world.res::<BudgetStatus>().used, used, "{tier}");
        }
    }

    #[test]
    fn missing_render_graph_is_reported() {
        let a = App::new("t").with_module(FxModule::new().with_effect(Fake(&CHEAP, &["small"])));
        assert_eq!(a.world.res::<ErrorLog>().error_count(), 1);
    }

    #[test]
    fn put_words_checks_length() {
        let mut out = [0.0; 3];
        assert_eq!(put_words(&mut out, &[1.0, 2.0]), 2);
        assert_eq!(out, [1.0, 2.0, 0.0]);
        assert_eq!(put_words(&mut out, &[1.0; 4]), 0);
    }

    #[test]
    fn graph_with_effects_compiles() {
        let mut g = RenderGraph::standard();
        g.add_target(TargetDesc::new("x", TargetKind::HdrColor)).unwrap();
        for e in FxModule::compiled().entries.iter() {
            e.effect.passes(&mut g).unwrap();
        }
        g.compile().unwrap();
    }

    /// Validate every compiled-in WGSL file with naga (slot functions
    /// spliced in as the renderer will) and check its uniform size.
    #[test]
    fn shaders_validate() {
        use evani_core::slots::ShaderSlots;
        let slots = ShaderSlots::with_builtins();
        for e in FxModule::compiled().entries.iter() {
            for sh in e.effect.shaders() {
                assert!(
                    sh.source.starts_with(&format!("// evani.shader.{}", sh.name)),
                    "{} needs its marker line",
                    sh.name
                );
                let markers: Vec<&str> = evani_render::splice::slot_names(sh.source).collect();
                assert_eq!(markers, sh.slots, "{}: //@slot markers vs EffectShader.slots", sh.name);
                let src = evani_render::splice::splice(sh.source, &slots, evani_render::splice::Bodies::Current)
                    .unwrap_or_else(|err| panic!("{}: {err}", sh.name))
                    .source;
                let module = naga::front::wgsl::parse_str(&src)
                    .unwrap_or_else(|err| panic!("{}: {}", sh.name, err.emit_to_string(&src)));
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::empty(),
                )
                .validate(&module)
                .unwrap_or_else(|err| panic!("{}: {err:?}", sh.name));
                let uniform_span = module
                    .global_variables
                    .iter()
                    .find(|(_, v)| v.space == naga::AddressSpace::Uniform)
                    .map(|(_, v)| module.types[v.ty].inner.size(module.to_ctx()));
                assert_eq!(
                    uniform_span,
                    Some(sh.uniform_words as u32 * 4),
                    "{} uniform size",
                    sh.name
                );
                // Big enough for the largest block (decals use 64 words).
                let mut buf = [0.0f32; 64];
                let frame = UniformFrame {
                    source_size: [960.0, 540.0],
                    target_size: [480.0, 270.0],
                    mip: 1,
                };
                let reg = ParamRegistry::new();
                // Unregistered params read as 0; the layout still fills.
                assert_eq!(
                    e.effect.write_uniforms(sh.name, &reg, &frame, &mut buf),
                    sh.uniform_words,
                    "{} write_uniforms",
                    sh.name
                );
            }
        }
    }
}
