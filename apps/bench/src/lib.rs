//! Evani.Dev bench: stress content at every tier, along a fixed camera path,
//! judged against the tier limits.
//!
//! * Browser (`cargo xtask bench`, or open the Trunk build by hand): the real
//!   WebGPU renderer. The page shows the table and sets
//!   `window.__bench = { done, error, mode, progress, results }`.
//! * Native (`cargo run -p evani-bench`): the same content and state machine
//!   headless, with the render module only counting what it would draw. The
//!   frame-time column is the engine's CPU time and GPU ms is n/a.
//!
//! Limits come only from `assets/tiers/*.json` through
//! [`evani_core::tiers::Tiers`]; see [`check`], [`content`] and [`run`].

pub mod characters;
pub mod check;
pub mod content;
#[cfg(target_arch = "wasm32")]
pub mod page;
pub mod run;

use evani_core::App;
use evani_look_basic::BasicLook;
use evani_render::RenderModule;

pub use check::{
    PassMs, PhaseResult, TierResult, format_passes, format_stress_table, format_table, mean_results,
    order_failures, tier_order_failures,
};
pub use run::{BenchConfig, BenchRun, Report, STRESS_MEASURE_FRAMES};

/// Build the bench app: render, fx (bloom, vignette), the basic look and
/// the bench system.
pub fn build_app(cfg: BenchConfig) -> App {
    let mut app = App::new("bench")
        .with_module(RenderModule::new())
        .with_module(evani_fx::FxModule::compiled())
        .with_look(BasicLook);
    let mut state = BenchRun::new(cfg);
    if state.config().device {
        run::prepare_device(&mut app.world, &mut state);
    } else {
        run::skip_warmup(&mut app.world, &mut state);
    }
    app.world.insert_resource(state);
    app.context("bench").system(run::system());
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> BenchConfig {
        BenchConfig {
            tiers: Vec::new(),
            warmup_frames: 2,
            measure_frames: 4,
            msaa: None,
            off: Vec::new(),
            stress: false,
            stress_fill: None,
            device: false,
        }
    }

    fn run_native(cfg: BenchConfig) -> Report {
        let mut app = build_app(cfg);
        for _ in 0..2_000 {
            if app.world.res::<BenchRun>().is_done() {
                break;
            }
            app.run_headless(1, 1000.0 / 60.0);
        }
        app.world.res::<BenchRun>().report().clone()
    }

    #[test]
    fn every_tier_runs_and_counts_what_it_drew() {
        let r = run_native(quick());
        assert!(r.done, "{:?}", r.error);
        assert_eq!(r.error, None);
        let ids: Vec<&str> = r.results.iter().map(|t| t.tier.as_str()).collect();
        assert_eq!(ids, ["lite", "low", "medium", "high", "ultra", "vr"]);
        for t in &r.results {
            // Five phases (3D, 2D, particles, models, characters) of 4
            // measured frames.
            assert_eq!(t.frames, 20, "{}", t.tier);
            // The character phase drew every posed rig over the backdrop.
            let c = t.phases.iter().find(|p| p.name == "characters").unwrap();
            assert!(c.triangles >= c.planned_triangles && c.planned_triangles >= characters::CHARACTERS * 36, "{}: {} of {}", t.tier, c.triangles, c.planned_triangles);
            assert!(c.triangles <= t.limits.max_triangles, "{}", t.tier);
            // The model phase drew the textured model: at least one copy
            // over the backdrop, within the tier's triangle limit.
            let m = t.phases.iter().find(|p| p.name == "models").unwrap();
            assert!(m.triangles >= m.planned_triangles && m.planned_triangles > 19_000, "{}: {} of {}", t.tier, m.triangles, m.planned_triangles);
            assert!(m.triangles <= t.limits.max_triangles, "{}", t.tier);
            // The particle phase fills the tier's whole particle cap.
            assert_eq!(t.particles, t.limits.max_particles, "{}", t.tier);
            assert!(t.limits.max_particles >= 1_000, "{}", t.tier);
            assert!(t.triangles as f32 >= t.limits.max_triangles as f32 * 0.89, "{}", t.tier);
            assert!(t.triangles <= t.limits.max_triangles, "{}", t.tier);
            assert!(t.sprites as f32 >= t.limits.max_sprites as f32 * 0.89, "{}", t.tier);
            assert!(t.sprites <= t.limits.max_sprites, "{}", t.tier);
            assert!(t.draw_calls <= t.limits.max_draw_calls, "{}", t.tier);
            assert_eq!(t.points_total, t.limits.points);
            assert_eq!(t.gpu_ms, None);
            assert_eq!(t.mode, "native-cpu");
            assert!(
                !t.failures.iter().any(|f| f.starts_with("content not drawn")),
                "{:?}",
                t.failures
            );
        }
        let vr = &r.results[5];
        assert_eq!(vr.note, "(mono, preset only)");
        // The table has a row for every tier.
        let table = format_table(&r.results);
        for id in ids {
            assert!(table.contains(id), "{table}");
        }
    }

    #[test]
    fn the_stress_scene_is_the_same_on_every_tier_and_judged_on_drawing_only() {
        let mut cfg = quick();
        cfg.stress = true;
        let r = run_native(cfg);
        assert!(r.done, "{:?}", r.error);
        assert_eq!(r.results.len(), 6);
        let planned = content::mesh_plan(content::STRESS_TRIANGLES).1;
        for t in &r.results {
            // One phase (3D), the same content everywhere, even over a tier's
            // own triangle limit (Low's 100k): only running and drawing count.
            assert_eq!(t.phases.len(), 1, "{}", t.tier);
            assert_eq!(t.frames, 4, "{}", t.tier);
            assert_eq!(t.triangles, planned, "{}", t.tier);
            assert!(t.pass, "{}: {:?}", t.tier, t.failures);
            assert!(t.note.starts_with("stress scene"), "{}", t.note);
        }
        assert!(r.results[0].triangles > r.results[0].limits.max_triangles);
    }

    #[test]
    fn adaptive_quality_is_off_and_tiers_are_forced() {
        use evani_core::params::{ParamRegistry, ParamValue};
        use evani_core::tiers::Tiers;
        let mut cfg = quick();
        cfg.tiers = vec!["ultra".into()];
        let mut app = build_app(cfg);
        app.run_headless(4, 1000.0 / 60.0);
        {
            let reg = app.world.res::<ParamRegistry>();
            let id = reg.id("aq.enabled").unwrap();
            assert_eq!(reg.value(id), Some(ParamValue::Bool(false)));
        }
        assert_eq!(app.world.res::<Tiers>().active().id, "ultra");
    }

    #[test]
    fn unknown_tier_is_reported_not_run() {
        let mut cfg = quick();
        cfg.tiers = vec!["potato".into()];
        let r = run_native(cfg);
        assert!(r.done);
        assert!(r.error.as_deref().is_some_and(|e| e.contains("potato")), "{:?}", r.error);
        assert!(r.results.is_empty());
    }

    #[test]
    fn runs_are_repeatable() {
        let a = run_native(quick());
        let b = run_native(quick());
        for (x, y) in a.results.iter().zip(&b.results) {
            assert_eq!(
                (x.triangles, x.sprites, x.particles, x.draw_calls),
                (y.triangles, y.sprites, y.particles, y.draw_calls)
            );
            assert_eq!(x.points_used, y.points_used);
        }
    }
}
