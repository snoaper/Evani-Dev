//! The renderer's CPU-side control logic, kept out of the GPU code so it is
//! tested natively: the render size for a tier, and the [`QualityDriver`]
//! that runs the first-run tier warm-up and the adaptive-quality controller
//! (both pure, in `evani_core::quality`) against the world each frame.

use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::num::{fixed, short_debug};
use evani_core::overlay::{ErrorLog, Platform};
use evani_core::params::ParamRegistry;
use evani_core::quality::{
    AqAction, AqController, AqParams, AqSample, QualityState, SAVED_TIER_KEY, TierWarmup, WarmupStep,
};
use evani_core::save::SaveData;
use evani_core::tiers::{TierPreset, Tiers};
use evani_core::time::Time;

/// Warm-up length (ms) over all candidate tiers, settle frames per tier and
/// the tolerance (%) over a tier's frame budget that still counts as holding
/// it. Engine policy, not tier data: the tier numbers stay in the tier JSON.
pub const WARMUP_TOTAL_MS: f32 = 2000.0;
pub const WARMUP_SETTLE_FRAMES: u32 = 10;
pub const WARMUP_TOLERANCE_PCT: f32 = 10.0;

/// Live builds only: after a capture or held-clock run, adaptive quality
/// ignores frame times for this long (ms), so readback stalls never shed
/// effects.
#[cfg(feature = "capture")]
pub const CAPTURE_SETTLE_MS: f64 = 1000.0;

/// Pixel size to render at: CSS size x min(devicePixelRatio, tier DPR cap)
/// x tier render scale x adaptive-quality scale, clamped to `1..=max_dim`.
pub fn render_size(
    css: [f32; 2],
    dpr: f32,
    tier: &TierPreset,
    quality_scale: f32,
    max_dim: u32,
) -> [u32; 2] {
    let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let k = tier.effective_dpr(dpr) * tier.render_scale_or_native() * quality_scale.clamp(0.05, 1.0);
    let max = max_dim.max(1);
    css.map(|v| {
        let px = (v.max(0.0) * k).round();
        if px.is_finite() { (px as u32).clamp(1, max) } else { 1 }
    })
}

/// Pixel size for `render.pixel_art`: the full device-pixel size (CSS size
/// x devicePixelRatio), ignoring the tier DPR cap, tier render scale and
/// adaptive-quality scale, clamped to `1..=max_dim`. A pixel-art game snaps
/// its art pixels to whole device pixels; any fractional resample (Low's
/// 0.75 render scale, a DPR cap) would break that integer grid.
pub fn render_size_exact(css: [f32; 2], dpr: f32, max_dim: u32) -> [u32; 2] {
    let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let max = max_dim.max(1);
    css.map(|v| {
        let px = (v.max(0.0) * dpr).round();
        if px.is_finite() { (px as u32).clamp(1, max) } else { 1 }
    })
}

/// Runs the tier warm-up (first run only) and then the adaptive-quality
/// controller each rendered frame. Allocation-free per frame.
#[derive(Debug, Default)]
pub struct QualityDriver {
    params: Option<AqParams>,
    aq: AqController,
    warmup: Option<TierWarmup>,
    /// Tier index the warm-up switched to (a different active tier means
    /// someone else switched: the warm-up stops).
    warm_expect: Option<usize>,
    last_tier: Option<usize>,
    on_effects: Vec<(&'static str, f32)>,
    started: bool,
    /// Adaptive quality ignores frames before this time (ms): pipeline
    /// compiles after a (re)start are not the scene's real cost.
    settle_until_ms: f64,
    /// [`QualityState::disturbances`] last seen.
    disturbances: u64,
}

impl QualityDriver {
    pub fn new(params: Option<AqParams>) -> Self {
        Self {
            params,
            on_effects: Vec::with_capacity(16),
            ..Self::default()
        }
    }

    /// Ignore frame times for the next `ms` (after a device start or
    /// recovery) and forget what was measured so far. The tier and any
    /// effects already shed stay as they are.
    pub fn settle(&mut self, world: &World, ms: f64) {
        self.aq.reset();
        self.settle_until_ms = world.res::<Time>().now_ms + ms;
    }

    /// Is the first-run warm-up still measuring?
    pub fn warming_up(&self) -> bool {
        self.warmup.is_some()
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// Call once when the GPU is ready. Applies the saved tier, or starts
    /// the warm-up when nothing is saved yet.
    pub fn begin(&mut self, world: &mut World, max_texture_dim: u32) {
        self.started = true;
        let saved = world
            .try_res::<SaveData>()
            .and_then(|s| s.load_raw(SAVED_TIER_KEY).ok().flatten());
        if let Some(id) = saved {
            let ok = world.res_mut::<Tiers>().set_active(&id);
            if ok {
                world.res_mut::<ErrorLog>().note(format!("tier: {id} (saved)"));
                self.last_tier = Some(world.res::<Tiers>().active_index());
                return;
            }
        }
        let w = {
            let tiers = world.res::<Tiers>();
            TierWarmup::new(
                &tiers,
                max_texture_dim,
                WARMUP_TOTAL_MS,
                WARMUP_SETTLE_FRAMES,
                WARMUP_TOLERANCE_PCT,
            )
        };
        if let Some(w) = w {
            let first = w.first();
            self.switch_tier(world, first);
            self.warm_expect = Some(first);
            self.warmup = Some(w);
            world.res_mut::<ErrorLog>().note("tier warm-up: measuring");
        }
        self.last_tier = Some(world.res::<Tiers>().active_index());
    }

    fn switch_tier(&mut self, world: &mut World, idx: usize) {
        let id = world.res::<Tiers>().presets().get(idx).map(|p| p.id.clone());
        if let Some(id) = id {
            world.res_mut::<Tiers>().set_active(&id);
        }
    }

    /// One rendered frame. `gpu_ms` is the last measured GPU time.
    pub fn frame(&mut self, world: &mut World, gpu_ms: Option<f32>) {
        if !self.started {
            return;
        }
        let (frame_ms, cpu_ms, now_ms) = {
            let t = world.res::<Time>();
            (t.frame_dt * 1000.0, t.cpu_ms, t.now_ms)
        };
        let active = world.res::<Tiers>().active_index();

        if let Some(mut w) = self.warmup.take() {
            if self.warm_expect != Some(active) {
                world
                    .res_mut::<ErrorLog>()
                    .note("tier warm-up stopped: tier changed by hand (not saved)");
                self.warm_expect = None;
            } else {
                // Judge work (CPU + GPU ms), not the vsync-bound interval
                // (which reads 16.7 ms at 60 Hz on every tier that holds
                // the refresh). Without a GPU time yet, the interval is the
                // conservative stand-in.
                let work_ms = gpu_ms.map_or(frame_ms, |g| cpu_ms.max(0.0) + g.max(0.0));
                let step = w.update_work(&world.res::<Tiers>(), frame_ms, work_ms);
                match step {
                    WarmupStep::Continue => self.warmup = Some(w),
                    WarmupStep::Try(i) => {
                        self.switch_tier(world, i);
                        self.warm_expect = Some(i);
                        self.warmup = Some(w);
                    }
                    WarmupStep::Done(i) => {
                        self.switch_tier(world, i);
                        self.warm_expect = None;
                        let id = world.res::<Tiers>().active().id.clone();
                        if let Some(mut q) = world.try_res_mut::<QualityState>() {
                            q.warmup_pick = Some(id.clone());
                        }
                        let mut line = format!("tier warm-up picked {id} (work: ");
                        for (k, (ti, ms)) in w.results.iter().enumerate() {
                            let tid = world
                                .res::<Tiers>()
                                .presets()
                                .get(*ti)
                                .map_or("?", |p| p.id.as_str())
                                .to_string();
                            if k > 0 {
                                line.push_str(", ");
                            }
                            line.push_str(&format!("{tid} {} ms", fixed(*ms, 1)));
                        }
                        line.push(')');
                        let saved = world
                            .try_res_mut::<SaveData>()
                            .map(|mut s| s.save_raw(SAVED_TIER_KEY, &id));
                        if let Some(Err(e)) = saved {
                            line.push_str(&format!("; not saved: {e}"));
                        }
                        world.res_mut::<ErrorLog>().note(line);
                    }
                }
                self.last_tier = Some(world.res::<Tiers>().active_index());
                return;
            }
        }

        if self.last_tier != Some(active) {
            self.last_tier = Some(active);
            self.aq.clear();
            if let Some(mut q) = world.try_res_mut::<QualityState>() {
                q.reset();
                // A tier switch rebuilds targets: not judged, plus the grace.
                q.disturb();
            }
        }

        // A live capture, bench or A/B run holds the sim clock (or drives the
        // camera) and reads pixels back, which stalls frames on purpose.
        // Adaptive quality must not shed effects because of that (the
        // capture would then show a degraded picture), so it ignores those
        // frames and the next second; the frames are marked held.
        #[cfg(feature = "capture")]
        if world
            .try_res::<evani_core::capture::CaptureQueue>()
            .is_some_and(|q| q.has_pending() || q.overriding())
        {
            self.settle(world, CAPTURE_SETTLE_MS);
            Self::mark_held(world, true);
            return;
        }
        if now_ms < self.settle_until_ms {
            Self::mark_held(world, true);
            return;
        }
        let Some(params) = self.params else {
            return;
        };
        let policy = {
            let reg = world.res::<ParamRegistry>();
            params.policy(&reg, world.res::<Tiers>().active().frame_budget_ms())
        };
        // A pipeline rebuild, tier switch or live change since the last
        // frame: drop the window and wait out the grace time.
        let disturbances = world.try_res::<QualityState>().map_or(0, |q| q.disturbances());
        if disturbances != self.disturbances {
            self.disturbances = disturbances;
            self.aq.disturb(now_ms, policy.grace_ms);
        }
        self.on_effects.clear();
        if let Some(fs) = world.try_res::<EffectsStatus>() {
            let reg = world.res::<ParamRegistry>();
            for e in fs.effects.iter().filter(|e| e.state.is_on()) {
                let value = e.weight.map_or(e.info.value, |w| reg.f32(w));
                self.on_effects.push((e.info.name, e.info.value_per_point(value)));
            }
        }
        let sample = AqSample {
            frame_ms,
            cpu_ms,
            gpu_ms,
            now_ms,
            reduced_motion: world.try_res::<Platform>().is_some_and(|p| p.reduced_motion),
        };
        let action = match world.try_res::<QualityState>() {
            Some(q) => self.aq.update(&policy, &sample, &q, &self.on_effects),
            None => None,
        };
        if let Some(mut q) = world.try_res_mut::<QualityState>() {
            q.held = false;
            let settling = policy.enabled && self.aq.settling(now_ms);
            if q.settling != settling {
                q.settling = settling;
            }
            q.target_ms = policy.target_ms;
            // Keep the last full measurement on screen while a fresh one
            // starts after a step.
            if let Some(w) = self.aq.work_ms() {
                q.work_ms = Some(w);
            }
            if let Some(a) = action {
                q.apply(a);
            }
        }
        if let Some(m) = self.aq.take_measured() {
            world.res_mut::<ErrorLog>().note(format!(
                "adaptive quality: last step saved {} ms (work {} -> {} ms)",
                fixed((m.before_ms - m.after_ms).max(0.0), 1),
                fixed(m.before_ms, 1),
                fixed(m.after_ms, 1)
            ));
        }
        if let Some(a) = action {
            let work = self.aq.work_ms().unwrap_or(0.0);
            world.res_mut::<ErrorLog>().note(format!(
                "adaptive quality: {} (work {} of {} ms)",
                action_text(a),
                fixed(sample.work_ms().max(work), 1),
                fixed(policy.target_ms, 1)
            ));
        }
    }

    /// Flag this frame as stalled on purpose (not judged).
    fn mark_held(world: &World, held: bool) {
        if let Some(mut q) = world.try_res_mut::<QualityState>()
            && q.held != held
        {
            q.held = held;
        }
    }

    /// The tab was hidden or shown: forget the measurements (the first
    /// frames after a hidden tab are not representative). A warm-up in
    /// progress is dropped without saving; the next run measures again.
    pub fn visibility(&mut self, world: &mut World, hidden: bool) {
        self.aq.reset();
        if hidden && self.warmup.take().is_some() {
            self.warm_expect = None;
            world
                .res_mut::<ErrorLog>()
                .note("tier warm-up stopped: tab hidden (not saved)");
        }
    }
}

/// `{:?}` of the action (`Scale(0.85)`, `ShedEffect("bloom")`) without
/// float `Debug`, which would ship core's float formatting code.
fn action_text(a: AqAction) -> String {
    match a {
        AqAction::ShedEffect(n) => format!("ShedEffect({n:?})"),
        AqAction::RestoreEffect(n) => format!("RestoreEffect({n:?})"),
        AqAction::Scale(s) => format!("Scale({})", short_debug(s, 4)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::quality::AqAction;

    #[test]
    fn action_text_matches_debug() {
        let mut acts = vec![AqAction::ShedEffect("bloom"), AqAction::RestoreEffect("outline")];
        for k in 0..=200 {
            acts.push(AqAction::Scale(k as f32 / 100.0));
        }
        for a in acts {
            assert_eq!(action_text(a), format!("{a:?}"));
        }
    }

    fn tier(json_id: &str) -> TierPreset {
        Tiers::builtin()
            .unwrap()
            .get(json_id)
            .cloned()
            .unwrap()
    }

    #[test]
    fn render_size_applies_dpr_cap_scale_and_clamp() {
        let mut t = tier("high");
        t.dpr_cap = Some(1.5);
        t.render_scale = Some(0.5);
        assert_eq!(render_size([1000.0, 500.0], 2.0, &t, 1.0, 8192), [750, 375]);
        assert_eq!(render_size([1000.0, 500.0], 1.0, &t, 0.5, 8192), [250, 125]);
        assert_eq!(render_size([1000.0, 500.0], 2.0, &t, 1.0, 600), [600, 375]);
        assert_eq!(render_size([0.0, 0.0], 2.0, &t, 1.0, 600), [1, 1]);
        t.dpr_cap = None;
        t.render_scale = None;
        assert_eq!(render_size([800.0, 600.0], f32::NAN, &t, 1.0, 8192), [800, 600]);
    }

    #[test]
    fn render_size_exact_ignores_tier_and_quality() {
        assert_eq!(render_size_exact([960.0, 540.0], 2.0, 8192), [1920, 1080]);
        assert_eq!(render_size_exact([1920.0, 1080.0], 1.0, 8192), [1920, 1080]);
        assert_eq!(render_size_exact([1920.0, 1080.0], f32::NAN, 1000), [1000, 1000]);
        assert_eq!(render_size_exact([0.0, 0.0], 2.0, 600), [1, 1]);
    }

    fn app() -> App {
        let mut a = App::new("t");
        a.run_headless(1, 16.0);
        a
    }

    fn step(a: &mut App, d: &mut QualityDriver, ms: f32, n: u32) {
        for _ in 0..n {
            {
                let mut t = a.world.res_mut::<Time>();
                t.frame_dt = ms / 1000.0;
                t.cpu_ms = ms;
                t.now_ms += f64::from(ms);
            }
            d.frame(&mut a.world, None);
        }
    }

    #[test]
    fn warmup_picks_saves_and_is_skipped_next_run() {
        let mut a = app();
        let mut d = QualityDriver::new(None);
        d.begin(&mut a.world, 8192);
        assert!(d.warming_up());
        // Fast frames: the first (highest) candidate holds its target.
        let first = a.world.res::<Tiers>().active().id.clone();
        step(&mut a, &mut d, 4.0, 2000);
        assert!(!d.warming_up());
        assert_eq!(a.world.res::<Tiers>().active().id, first);
        let saved = a.world.res::<SaveData>().load_raw(SAVED_TIER_KEY).unwrap();
        assert_eq!(saved.as_deref(), Some(first.as_str()));

        // Next run with the same save: no warm-up.
        let mut d2 = QualityDriver::new(None);
        a.world.res_mut::<Tiers>().set_active("low");
        d2.begin(&mut a.world, 8192);
        assert!(!d2.warming_up());
        assert_eq!(a.world.res::<Tiers>().active().id, first);
    }

    /// One frame with a frame interval and a separate CPU + GPU work time.
    fn step_work(a: &mut App, d: &mut QualityDriver, interval_ms: f32, cpu: f32, gpu: f32, n: u32) {
        for _ in 0..n {
            {
                let mut t = a.world.res_mut::<Time>();
                t.frame_dt = interval_ms / 1000.0;
                t.cpu_ms = cpu;
                t.now_ms += f64::from(interval_ms);
            }
            d.frame(&mut a.world, Some(gpu));
        }
    }

    /// The warm-up judged the frame interval, which reads the display
    /// refresh (or a busy browser), not what the tier costs. A light frame
    /// (3 ms of work) on a page the browser holds at 30 fps used to walk
    /// down to the lowest tier; heavy work is still caught.
    #[test]
    fn warmup_judges_work_not_the_frame_interval() {
        let mut a = app();
        let mut d = QualityDriver::new(None);
        d.begin(&mut a.world, 8192);
        let first = a.world.res::<Tiers>().active().id.clone();
        step_work(&mut a, &mut d, 33.3, 1.0, 2.0, 400);
        assert!(!d.warming_up());
        assert_eq!(a.world.res::<Tiers>().active().id, first);
        assert_eq!(a.world.res::<QualityState>().warmup_pick.as_deref(), Some(first.as_str()));
        let log: Vec<String> = a.world.res::<ErrorLog>().entries().map(|e| e.message.clone()).collect();
        assert!(
            log.iter().any(|l| l.starts_with(&format!("tier warm-up picked {first} (work: {first} 3.0 ms"))),
            "{log:?}"
        );

        // Vsync-bound interval but 40 ms of work: no tier holds it.
        let mut a = app();
        let mut d = QualityDriver::new(None);
        d.begin(&mut a.world, 8192);
        step_work(&mut a, &mut d, 16.7, 2.0, 38.0, 1000);
        assert!(!d.warming_up());
        let lowest_auto = a
            .world
            .res::<Tiers>()
            .presets()
            .iter()
            .find(|p| p.auto_select && !p.stereo)
            .map(|p| p.id.clone());
        assert_eq!(Some(a.world.res::<Tiers>().active().id.clone()), lowest_auto);
    }

    #[test]
    fn slow_frames_walk_down_the_tiers() {
        let mut a = app();
        let mut d = QualityDriver::new(None);
        d.begin(&mut a.world, 8192);
        step(&mut a, &mut d, 100.0, 400);
        assert!(!d.warming_up());
        let lowest_auto = a
            .world
            .res::<Tiers>()
            .presets()
            .iter()
            .find(|p| p.auto_select && !p.stereo)
            .map(|p| p.id.clone());
        assert_eq!(Some(a.world.res::<Tiers>().active().id.clone()), lowest_auto);
    }

    #[test]
    fn manual_tier_change_stops_warmup_without_saving() {
        let mut a = app();
        let mut d = QualityDriver::new(None);
        d.begin(&mut a.world, 8192);
        step(&mut a, &mut d, 4.0, 3);
        a.world.res_mut::<Tiers>().set_active("low");
        step(&mut a, &mut d, 4.0, 1);
        assert!(!d.warming_up());
        assert_eq!(a.world.res::<Tiers>().active().id, "low");
        assert_eq!(a.world.res::<SaveData>().load_raw(SAVED_TIER_KEY).unwrap(), None);
    }

    #[test]
    fn tier_change_resets_quality_state() {
        let mut a = app();
        let p = AqParams::register(&mut a.world.res_mut::<ParamRegistry>()).unwrap();
        a.world
            .res_mut::<SaveData>()
            .save_raw(SAVED_TIER_KEY, "high")
            .unwrap();
        let mut d = QualityDriver::new(Some(p));
        d.begin(&mut a.world, 8192);
        a.world.res_mut::<QualityState>().apply(AqAction::Scale(0.85));
        step(&mut a, &mut d, 16.0, 1);
        assert_eq!(a.world.res::<QualityState>().scale, 0.85);
        a.world.res_mut::<Tiers>().set_active("low");
        step(&mut a, &mut d, 16.0, 1);
        assert_eq!(a.world.res::<QualityState>().scale, 1.0);
    }

    #[test]
    fn sustained_slow_frames_degrade_quality() {
        let mut a = app();
        let p = AqParams::register(&mut a.world.res_mut::<ParamRegistry>()).unwrap();
        a.world
            .res_mut::<SaveData>()
            .save_raw(SAVED_TIER_KEY, "high")
            .unwrap();
        let mut d = QualityDriver::new(Some(p));
        d.begin(&mut a.world, 8192);
        step(&mut a, &mut d, 40.0, 200);
        assert!(a.world.res::<QualityState>().is_degraded());
    }

    /// Note 1: frames pinned to a slow display interval with little work
    /// never cut anything; the overlay's work time is the real cost.
    #[test]
    fn vsync_bound_frames_with_light_work_keep_full_quality() {
        let mut a = app();
        let p = AqParams::register(&mut a.world.res_mut::<ParamRegistry>()).unwrap();
        a.world
            .res_mut::<SaveData>()
            .save_raw(SAVED_TIER_KEY, "high")
            .unwrap();
        let mut d = QualityDriver::new(Some(p));
        d.begin(&mut a.world, 8192);
        for _ in 0..2000 {
            {
                let mut t = a.world.res_mut::<Time>();
                t.frame_dt = 0.0333;
                t.now_ms += 33.3;
                t.cpu_ms = 0.8;
            }
            d.frame(&mut a.world, Some(1.2));
        }
        let q = a.world.res::<QualityState>();
        assert!(!q.is_degraded());
        assert!(q.work_ms.is_some_and(|w| (w - 2.0).abs() < 0.05), "{:?}", q.work_ms);
        assert!(!q.held);
    }

    #[test]
    fn settle_ignores_slow_frames_after_a_restart() {
        let mut a = app();
        let p = AqParams::register(&mut a.world.res_mut::<ParamRegistry>()).unwrap();
        a.world
            .res_mut::<SaveData>()
            .save_raw(SAVED_TIER_KEY, "high")
            .unwrap();
        let mut d = QualityDriver::new(Some(p));
        d.begin(&mut a.world, 8192);
        d.settle(&a.world, 5000.0);
        // 100 x 40 ms = 4 s of hitches, all inside the settle window.
        step(&mut a, &mut d, 40.0, 100);
        assert!(!a.world.res::<QualityState>().is_degraded());
        step(&mut a, &mut d, 40.0, 200);
        assert!(a.world.res::<QualityState>().is_degraded());
    }

    /// Review follow-up 2: frames while something disturbs them on purpose
    /// (a shader compile or pipeline swap, a live change, a tier switch:
    /// all call `QualityState::disturb`) and `aq.grace_ms` after the last
    /// one are not judged; the overlay says "settling" meanwhile.
    #[test]
    fn disturbed_frames_and_the_grace_after_are_not_judged() {
        let mut a = app();
        let p = AqParams::register(&mut a.world.res_mut::<ParamRegistry>()).unwrap();
        a.world
            .res_mut::<SaveData>()
            .save_raw(SAVED_TIER_KEY, "high")
            .unwrap();
        let mut d = QualityDriver::new(Some(p));
        d.begin(&mut a.world, 8192);
        step(&mut a, &mut d, 16.0, 120);
        // A shader compiles for 3 s: every frame 100 ms, every frame disturbed.
        for _ in 0..30 {
            a.world.res_mut::<QualityState>().disturb();
            step(&mut a, &mut d, 100.0, 1);
            assert!(a.world.res::<QualityState>().settling);
        }
        // The grace second: 40 ms frames, still not judged.
        step(&mut a, &mut d, 40.0, 24);
        assert!(a.world.res::<QualityState>().settling);
        assert!(!a.world.res::<QualityState>().is_degraded());
        // Then real slowness is judged again.
        step(&mut a, &mut d, 40.0, 60);
        assert!(!a.world.res::<QualityState>().settling);
        assert!(a.world.res::<QualityState>().is_degraded());

        // A tier switch disturbs too.
        a.world.res_mut::<Tiers>().set_active("medium");
        step(&mut a, &mut d, 40.0, 1);
        assert!(a.world.res::<QualityState>().settling);
        assert!(!a.world.res::<QualityState>().is_degraded(), "reset by the switch");
    }

    /// Capture and held-clock runs stall frames on purpose: no effect is
    /// shed during them or in the second after.
    #[cfg(feature = "capture")]
    #[test]
    fn captures_never_degrade_quality() {
        use evani_core::capture::CaptureQueue;
        let mut a = app();
        let p = AqParams::register(&mut a.world.res_mut::<ParamRegistry>()).unwrap();
        a.world
            .res_mut::<SaveData>()
            .save_raw(SAVED_TIER_KEY, "high")
            .unwrap();
        a.world.insert_resource(CaptureQueue::default());
        let mut d = QualityDriver::new(Some(p));
        d.begin(&mut a.world, 8192);
        step(&mut a, &mut d, 16.0, 120);
        a.world.res_mut::<CaptureQueue>().sim_time_override = Some(6.0);
        step(&mut a, &mut d, 200.0, 40);
        a.world.res_mut::<CaptureQueue>().sim_time_override = None;
        // The readback hitch lands just after the hold; then normal frames.
        step(&mut a, &mut d, 200.0, 4);
        step(&mut a, &mut d, 16.0, 300);
        assert!(!a.world.res::<QualityState>().is_degraded());
        // Real slowness is still caught.
        step(&mut a, &mut d, 40.0, 300);
        assert!(a.world.res::<QualityState>().is_degraded());
    }
}
