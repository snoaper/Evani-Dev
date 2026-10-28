//! The app: builder for games plus the per-frame loop.
//!
//! ```ignore
//! let app = App::new("sandbox")
//!     .with_kit(ExampleKit)
//!     .with_look(BasicLook);
//! #[cfg(feature = "fx")]
//! let app = app.with_module(evani_fx::FxModule::default());
//! evani_core::web::run(app); // wasm: rAF loop; native: app.run_headless(..)
//! ```
//!
//! Frame order (see [`App::frame`]): input update, N fixed ticks (fixed
//! stages, top scene, event buffer swap, input edge reset), the `Frame`
//! stage, module `frame` hooks (render), audio, overlay (every 250 ms).

use crate::assets::Assets;
use crate::audio::{AudioMixer, Bus};
use crate::particles::ParticleQueue;
use crate::ecs::{Schedule, Stage, SystemDesc, World};
use crate::effects::BudgetStatus;
use crate::events::EngineEvent;
use crate::formulas::{FormulaJson, Formulas};
use crate::input::Input;
use crate::overlay::{self, ErrorLog, Overlay, OverlayStats, Platform, RenderStats};
use crate::params::{F32Param, ParamDef, ParamRegistry};
use crate::play::PlayMode;
use crate::plugin::{
    BuildInfo, DataFiles, EngineModule, KitPlugin, LookContext, LookData, LookPlugin, PendingBaked,
    PluginContext,
};
use crate::rng::Pcg32;
use crate::save::SaveData;
use crate::scene::{SceneRegistry, SceneStack, SceneTransition};
use crate::slots::ShaderSlots;
use crate::tiers::Tiers;
use crate::time::{FixedStep, FrameStats, Time};
use crate::version::{self, PartKind, PartVersion};

/// Default seed for the world RNG (games reseed for new runs).
pub const DEFAULT_SEED: u64 = 0x5EED_E7A1;

/// Handles of the params core registers for itself.
#[derive(Debug, Clone, Copy)]
struct CoreParams {
    master: F32Param,
    music: F32Param,
    effects: F32Param,
    overlay_ms: F32Param,
}

pub struct App {
    pub world: World,
    pub schedule: Schedule,
    pub scenes: SceneRegistry,
    pub stack: SceneStack,
    name: String,
    modules: Vec<Box<dyn EngineModule>>,
    start_scene: Option<String>,
    fixed: FixedStep,
    stats: FrameStats,
    started: bool,
    hidden: bool,
    last_ms: Option<f64>,
    last_tier: usize,
    core_params: Option<CoreParams>,
    params_seen: u64,
    overlay_buf: String,
    /// The overlay's 2 s GPU medians (telemetry stays per-frame raw).
    gpu_window: overlay::GpuWindow,
}

impl App {
    /// A new app with every core resource inserted. `name` namespaces saves.
    pub fn new(name: &str) -> Self {
        let mut world = World::new();
        let mut errors = ErrorLog::default();
        let tiers = match Tiers::builtin() {
            Ok(t) => t,
            Err(e) => {
                errors.error(format!("tiers: {e}"));
                // Unreachable with the shipped JSON; keep running on a
                // minimal preset rather than crash.
                fallback_tiers()
            }
        };
        let last_tier = tiers.active_index();
        let budget_total = tiers.active().points;

        world.insert_resource(Time {
            fixed_dt: FixedStep::default().step_seconds(),
            ..Time::default()
        });
        world.insert_resource(Input::default());
        world.insert_resource(Pcg32::seeded(DEFAULT_SEED));
        let mut registry = ParamRegistry::new();
        match crate::quality::AqParams::register(&mut registry) {
            Ok(aq) => world.insert_resource(aq),
            Err(e) => errors.error(format!("aq params: {e}")),
        }
        world.insert_resource(registry);
        world.insert_resource(crate::capture::DeviceStatus::default());
        world.insert_resource(Formulas::default());
        world.insert_resource(ShaderSlots::with_builtins());
        world.insert_resource(crate::slots::SlotRebuilds::default());
        world.insert_resource(crate::quality::QualityState::default());
        world.insert_resource(tiers);
        world.insert_resource(LookData::default());
        world.insert_resource(DataFiles::default());
        world.insert_resource(PendingBaked::default());
        world.insert_resource(BuildInfo {
            game: name.to_string(),
            parts: vec![PartVersion::core()],
            ..BuildInfo::default()
        });
        world.insert_resource(BudgetStatus {
            used: 0,
            total: budget_total,
        });
        world.insert_resource(RenderStats::default());
        world.insert_resource(errors);
        world.insert_resource(Platform::default());
        world.insert_resource(Overlay::default());
        world.insert_resource(SaveData::for_platform(name));
        world.insert_resource(Assets::default());
        #[cfg(all(target_arch = "wasm32", not(evani_headless)))]
        world.insert_resource(crate::play::from_page());
        #[cfg(not(all(target_arch = "wasm32", not(evani_headless))))]
        world.insert_resource(crate::play::PlayMode::default());
        world.insert_resource(crate::play::PlayOutcomes::default());
        let audio = platform_audio(&mut world);
        world.insert_resource(audio);
        world.insert_resource(ParticleQueue::default());
        world.add_event::<EngineEvent>();

        let mut app = Self {
            world,
            schedule: Schedule::default(),
            scenes: SceneRegistry::default(),
            stack: SceneStack::default(),
            name: name.to_string(),
            modules: Vec::new(),
            start_scene: None,
            fixed: FixedStep::default(),
            stats: FrameStats::default(),
            started: false,
            hidden: false,
            last_ms: None,
            last_tier,
            core_params: None,
            params_seen: 0,
            overlay_buf: String::with_capacity(1024),
            gpu_window: overlay::GpuWindow::default(),
        };
        app.core_params = app.register_core_params();
        app
    }

    fn register_core_params(&mut self) -> Option<CoreParams> {
        let mut reg = self.world.res_mut::<ParamRegistry>();
        let vol = |name: &str, d: f32, what: &str| {
            ParamDef::f32(name, d)
                .range(0.0, 1.0)
                .unit("gain")
                .describe(what)
                .owner("core")
        };
        let r = (|| {
            Ok::<_, crate::params::ParamError>(CoreParams {
                master: reg.register_f32(vol("core.audio.master", 1.0, "Master volume"))?,
                music: reg.register_f32(vol("core.audio.music", 0.7, "Music bus volume"))?,
                effects: reg.register_f32(vol(
                    "core.audio.effects",
                    1.0,
                    "Sound-effects bus volume",
                ))?,
                overlay_ms: reg.register_f32(
                    ParamDef::f32("core.overlay.interval_ms", 250.0)
                        .range(50.0, 2000.0)
                        .unit("ms")
                        .describe("How often the debug overlay text refreshes")
                        .owner("core"),
                )?,
            })
        })();
        drop(reg);
        match r {
            Ok(p) => Some(p),
            Err(e) => {
                self.error(format!("core params: {e}"));
                None
            }
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Log an error to the overlay (and the console on wasm).
    pub fn error(&self, msg: impl Into<String>) {
        self.world.res_mut::<ErrorLog>().error(msg);
    }

    /// A registration context for game code (`owner` e.g. `game`).
    pub fn context(&mut self, owner: &str) -> PluginContext<'_> {
        PluginContext::new(
            &mut self.world,
            &mut self.schedule,
            &mut self.scenes,
            owner.to_string(),
        )
    }

    // ---- builder --------------------------------------------------------------

    pub fn with_kit(mut self, kit: impl KitPlugin) -> Self {
        self.add_kit(&kit);
        self
    }

    /// Play or free roam ([`crate::play`]). On the web the page address
    /// sets it (`?mode=free` / `#mode=free`); call this before
    /// [`App::with_kit`], since the kit reads it when it is built.
    pub fn with_play_mode(mut self, mode: PlayMode) -> Self {
        self.set_play_mode(mode);
        self
    }

    /// See [`App::with_play_mode`]. Once a kit is built the mode is fixed:
    /// a change then is logged as an error and ignored.
    pub fn set_play_mode(&mut self, mode: PlayMode) {
        if self.world.res::<BuildInfo>().kit.is_some() {
            if mode != self.play_mode() {
                self.error(format!(
                    "play mode: set it before the kit is added (it stays {})",
                    self.play_mode().label()
                ));
            }
            return;
        }
        if let Some(m) = self.world.res_get_mut::<PlayMode>() {
            *m = mode;
        }
    }

    pub fn play_mode(&self) -> PlayMode {
        // try_res: no panic path (and its type-name string) in every download.
        self.world.try_res::<PlayMode>().map_or(PlayMode::Play, |m| *m)
    }

    pub fn add_kit(&mut self, kit: &dyn KitPlugin) {
        if self.play_mode().is_free_roam() && !kit.free_roam() {
            if let Some(m) = self.world.res_get_mut::<PlayMode>() {
                *m = PlayMode::Play;
            }
            self.world
                .res_mut::<ErrorLog>()
                .note("play mode: the kit has no free roam, so it plays normally");
        }
        {
            let mut info = self.world.res_mut::<BuildInfo>();
            info.kit = Some(format!("evani-kit-{}", kit.name()));
            version::insert_part(
                &mut info.parts,
                PartVersion::new(PartKind::Kit, kit.name(), kit.version()),
            );
        }
        let owner = format!("kit.{}", kit.name());
        let r = kit.build(&mut self.context(&owner));
        if let Err(e) = r {
            self.error(format!("{owner}: {e}"));
        }
        if self.start_scene.is_none() {
            self.start_scene = kit.start_scene().map(str::to_string);
        }
        #[cfg(feature = "focus")]
        {
            if !self.world.has_resource::<crate::focus::FocusAssets>() {
                self.world.insert_resource(crate::focus::FocusAssets::default());
            }
            let dups = {
                let mut list = self.world.res_mut::<crate::focus::FocusAssets>();
                list.set_owner(kit.name());
                kit.focus_assets(&mut list);
                core::mem::take(&mut list.duplicates)
            };
            for id in dups {
                self.error(format!("{owner}: focus asset `{id}` is already registered"));
            }
        }
    }

    pub fn with_look(mut self, look: impl LookPlugin) -> Self {
        self.add_look(&look);
        self
    }

    pub fn add_look(&mut self, look: &dyn LookPlugin) {
        {
            let mut d = self.world.res_mut::<LookData>();
            d.name = look.name().to_string();
            d.points = look.points();
            d.styles = look.styles();
        }
        {
            let mut info = self.world.res_mut::<BuildInfo>();
            info.look = Some(format!("evani-look-{}", look.name()));
            version::insert_part(
                &mut info.parts,
                PartVersion::new(PartKind::Look, look.name(), look.version()),
            );
        }
        let owner = format!("look.{}", look.name());
        let mut ctx = self.context(&owner);
        let r = look.build(&mut LookContext { ctx: &mut ctx });
        if let Err(e) = r {
            self.error(format!("{owner}: {e}"));
        }
    }

    /// Add an optional engine module (call it behind the module's Cargo
    /// feature in the game crate).
    pub fn with_module(mut self, module: impl EngineModule) -> Self {
        self.add_module(Box::new(module));
        self
    }

    pub fn add_module(&mut self, mut module: Box<dyn EngineModule>) {
        let owner = module.label().to_string();
        {
            let mut info = self.world.res_mut::<BuildInfo>();
            info.modules.push((module.id(), module.label()));
            // Only Evani.Dev parts report a version; game-local modules
            // (debug panel, game UI) stay out of the versions line.
            if let Some(v) = module.version() {
                let mut part = PartVersion::new(PartKind::Module, module.label(), Some(v));
                part.features = module.version_features();
                part.protocol = module.protocol();
                version::insert_part(&mut info.parts, part);
            }
        }
        let r = module.build(&mut self.context(&owner));
        if let Err(e) = r {
            self.error(format!("{owner}: {e}"));
        }
        self.modules.push(module);
    }

    pub fn with_system(mut self, desc: SystemDesc) -> Self {
        self.schedule.add(desc);
        self
    }

    /// Scene pushed at start (overrides the kit's choice).
    pub fn with_start_scene(mut self, name: &str) -> Self {
        self.start_scene = Some(name.to_string());
        self
    }

    /// Simulation rate (default 60 Hz) and max ticks per frame (default 5).
    pub fn with_fixed_rate(mut self, hz: u32, max_steps: u32) -> Self {
        self.fixed = FixedStep::new(hz, max_steps);
        self.world.res_mut::<Time>().fixed_dt = self.fixed.step_seconds();
        self
    }

    /// `(id, label)` of every module in this build.
    pub fn modules(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.modules.iter().map(|m| (m.id(), m.label()))
    }

    pub fn has_module(&self, label: &str) -> bool {
        self.modules.iter().any(|m| m.label() == label)
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    // ---- lifecycle -------------------------------------------------------------

    /// Apply baked values, push the start scene and start modules. Called by
    /// the first [`App::frame`] if not called explicitly.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        if self.play_mode().is_free_roam() {
            // The site's headless check reads this line from the console.
            self.world.res_mut::<ErrorLog>().note("play mode: free roam");
        }
        self.apply_baked();
        self.sync_core_params(true);
        {
            let look_points = self.world.res::<LookData>().points;
            self.world.res_mut::<BudgetStatus>().used = look_points;
        }
        if let Some(scene) = self.start_scene.clone() {
            self.stack.request(SceneTransition::Push(scene));
            self.scene_step(false);
        }
        for m in &mut self.modules {
            m.start(&mut self.world);
        }
        self.world.apply_commands();
    }

    fn apply_baked(&mut self) {
        apply_pending_baked(&mut self.world);
    }

    fn sync_core_params(&mut self, force: bool) {
        let Some(p) = self.core_params else { return };
        let rev = self.world.res::<ParamRegistry>().revision();
        if !force && rev == self.params_seen {
            return;
        }
        let since = self.params_seen;
        self.params_seen = rev;
        let reg = self.world.res::<ParamRegistry>();
        if force || reg.any_changed_since(&[p.master.id(), p.music.id(), p.effects.id()], since) {
            let mut mixer = self.world.res_mut::<AudioMixer>();
            mixer.set_volume(None, reg.f32(p.master));
            mixer.set_volume(Some(Bus::Music), reg.f32(p.music));
            mixer.set_volume(Some(Bus::Effects), reg.f32(p.effects));
        }
        if force || reg.changed_since(p.overlay_ms.id(), since) {
            self.world.res_mut::<Overlay>().interval_ms = f64::from(reg.f32(p.overlay_ms));
        }
    }

    /// Update the top scene (`update`) or just apply queued transitions.
    fn scene_step(&mut self, update: bool) {
        let r = if update {
            self.stack.update(&mut self.world, &self.scenes)
        } else {
            self.stack.apply(&mut self.world, &self.scenes)
        };
        match r {
            Ok(true) => {
                let top = self.stack.top_name().map(str::to_string);
                self.world.send_event(EngineEvent::SceneChanged { top });
            }
            Ok(false) => {}
            Err(e) => self.error(e),
        }
    }

    /// Pause/resume on tab visibility. While hidden, [`App::frame`] does
    /// nothing; on return the clock restarts so hidden time is not replayed.
    pub fn set_hidden(&mut self, hidden: bool) {
        if hidden == self.hidden {
            return;
        }
        self.hidden = hidden;
        self.world.res_mut::<Platform>().hidden = hidden;
        self.world.res_mut::<AudioMixer>().set_hidden(hidden);
        if hidden {
            self.world
                .res_mut::<Input>()
                .handle(&crate::input::RawEvent::Blur);
        } else {
            self.fixed.reset();
            self.last_ms = None;
        }
        for m in &mut self.modules {
            m.visibility_changed(&mut self.world, hidden);
        }
        self.world.send_event(EngineEvent::Visibility { hidden });
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// One rendered frame at wall-clock `now_ms` (rAF timestamp).
    pub fn frame(&mut self, now_ms: f64) {
        let dt = match self.last_ms {
            Some(last) => (now_ms - last).max(0.0),
            None => 0.0,
        };
        self.last_ms = Some(now_ms);
        self.frame_by(now_ms, dt);
    }

    /// Run `frames` frames of `dt_ms` each without a browser (tests, tools,
    /// native sandbox).
    pub fn run_headless(&mut self, frames: u32, dt_ms: f64) {
        let mut now = self.last_ms.unwrap_or(0.0);
        for _ in 0..frames {
            now += dt_ms;
            self.last_ms = Some(now);
            self.frame_by(now, dt_ms);
        }
    }

    fn frame_by(&mut self, now_ms: f64, dt_ms: f64) {
        if !self.started {
            self.start();
        }
        if self.hidden {
            return;
        }
        let cpu_start = crate::time::now_ms();
        // Live capture holds the sim clock: no fixed ticks and no accumulated
        // time while held, so the sim resumes exactly where it paused.
        #[cfg(feature = "capture")]
        let held = self
            .world
            .try_res::<crate::capture::CaptureQueue>()
            .and_then(|q| q.sim_time_override);
        #[cfg(feature = "capture")]
        let plan = if held.is_some() {
            crate::time::StepPlan {
                steps: 0,
                alpha: 0.0,
                dropped_time: false,
            }
        } else {
            self.fixed.advance_by(dt_ms)
        };
        #[cfg(not(feature = "capture"))]
        let plan = self.fixed.advance_by(dt_ms);
        // Live focus can freeze the game (no fixed ticks) while render time
        // keeps running, so the focused asset still animates.
        #[cfg(feature = "focus")]
        let focus_hold = {
            let mut f = self.world.try_res_mut::<crate::focus::FocusState>();
            if let Some(f) = f.as_deref_mut() {
                f.advance((dt_ms / 1000.0) as f32);
            }
            f.is_some_and(|f| f.holds_sim())
        };
        #[cfg(feature = "focus")]
        let plan = if focus_hold {
            self.fixed.reset();
            crate::time::StepPlan {
                steps: 0,
                alpha: 0.0,
                dropped_time: false,
            }
        } else {
            plan
        };
        let fixed_dt = self.fixed.step_seconds();
        {
            let mut t = self.world.res_mut::<Time>();
            #[cfg(feature = "capture")]
            {
                t.held = held;
            }
            #[cfg(feature = "focus")]
            {
                t.focus_extra = if focus_hold { t.focus_extra + dt_ms / 1000.0 } else { 0.0 };
            }
            t.frame_dt = (dt_ms / 1000.0) as f32;
            t.alpha = plan.alpha;
            t.now_ms = now_ms;
            t.frame += 1;
            t.fixed_dt = fixed_dt;
        }
        self.sync_core_params(false);
        self.check_tier_change();
        self.world.res_mut::<Input>().update();

        for _ in 0..plan.steps {
            let tick = self.world.res::<Time>().tick;
            self.world.res_mut::<ParticleQueue>().set_tick(tick);
            self.schedule.run_fixed(&mut self.world);
            self.scene_step(true);
            self.world.update_events();
            self.world.res_mut::<Input>().end_tick();
            let mut t = self.world.res_mut::<Time>();
            t.tick += 1;
            t.sim_seconds = t.tick as f64 * f64::from(fixed_dt);
        }

        self.schedule.run_stage(Stage::Frame, &mut self.world);
        // Modules that draw go last (EngineModule::draws), so the picture is
        // drawn with what every other module set this frame.
        for drawing in [false, true] {
            for m in self.modules.iter_mut().filter(|m| m.draws() == drawing) {
                #[cfg(feature = "hotpatch")]
                m.__evani_hot_frame(&mut self.world);
                #[cfg(not(feature = "hotpatch"))]
                m.frame(&mut self.world);
            }
        }
        self.world.apply_commands();
        self.world.res_mut::<AudioMixer>().update();
        // Emits no particle system took (none in the build) are dropped.
        self.world.res_mut::<ParticleQueue>().end_frame();

        let cpu_ms = (crate::time::now_ms() - cpu_start) as f32;
        self.stats.record(dt_ms as f32, cpu_ms);
        self.world.res_mut::<Time>().cpu_ms = cpu_ms;
        self.gpu_window
            .observe(now_ms, &self.world.res::<RenderStats>());
        if self.world.res::<Overlay>().due(now_ms) {
            self.update_overlay(now_ms);
        }
    }

    fn check_tier_change(&mut self) {
        let (idx, id) = {
            let t = self.world.res::<Tiers>();
            if t.active_index() == self.last_tier {
                return;
            }
            (t.active_index(), t.active().id.clone())
        };
        self.last_tier = idx;
        let points = self.world.res::<Tiers>().active().points;
        self.world.res_mut::<BudgetStatus>().total = points;
        self.world.send_event(EngineEvent::TierChanged { tier: id });
    }

    /// Frame statistics (fps, CPU ms) over the last 64 frames.
    pub fn frame_stats(&self) -> &FrameStats {
        &self.stats
    }

    fn update_overlay(&mut self, now_ms: f64) {
        let out = &mut self.overlay_buf;
        out.clear();
        version::write_versions(out, &self.world.res::<BuildInfo>().parts);
        out.push('\n');
        {
            let render = self.world.res::<RenderStats>();
            let tiers = self.world.res::<Tiers>();
            let platform = self.world.res::<Platform>();
            let quality = self.world.try_res::<crate::quality::QualityState>();
            self.gpu_window.update(now_ms, &render.passes);
            overlay::write_stats(
                out,
                &OverlayStats {
                    fps: self.stats.fps(),
                    cpu_ms: self.stats.mean_cpu_ms(),
                    render: &render,
                    entities: self.world.entity_count(),
                    tier: &tiers.active().id,
                    budget: *self.world.res::<BudgetStatus>(),
                    download_bytes: platform.download_bytes,
                    dev_build: self.world.try_res::<overlay::DevBuild>().map(|d| *d),
                    hidden: self.hidden,
                    quality: quality.as_deref(),
                    gpu_median: Some(&self.gpu_window),
                },
            );
        }
        if self.modules.is_empty() {
            out.push_str("modules: core only\n");
        }
        for m in &self.modules {
            out.push_str(m.label());
            out.push_str(": ");
            m.status(&self.world, out);
            out.push('\n');
        }
        overlay::write_log(out, &self.world.res::<ErrorLog>(), 4);
        let mut o = self.world.res_mut::<Overlay>();
        o.set_text(out);
        o.last_ms = now_ms;
    }
}

fn platform_audio(_world: &mut World) -> AudioMixer {
    #[cfg(all(target_arch = "wasm32", not(evani_headless)))]
    {
        let audio = crate::web::WebAudio::new();
        _world.insert_resource(audio.unlocker());
        AudioMixer::new(Box::new(audio))
    }
    #[cfg(not(all(target_arch = "wasm32", not(evani_headless))))]
    {
        AudioMixer::new(Box::new(crate::audio::NullAudio::default()))
    }
}

/// Apply one baked `{"name": value}` file, logging unknown and clamped names.
/// Apply (and empty) the world's [`PendingBaked`]: look overrides, baked
/// params, shader slots and formulas, with problems in the [`ErrorLog`].
/// [`App::start`] runs it once; the live module runs it again in hot mode
/// for baked files changed since the build (docs/LIVE_API.md 10.4).
pub fn apply_pending_baked(world: &mut World) {
    let pending = core::mem::take(&mut *world.res_mut::<PendingBaked>());
    let mut log = Vec::new();
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        // Look overrides first so baked params.json (the tuned values)
        // wins over a look's `post_param` default.
        for (owner, name, value) in &pending.overrides {
            if reg.id(name).is_none() {
                log.push((false, format!("{owner}: {name} not in this build")));
            } else if let Err(e) = reg.set_json(name, value) {
                log.push((true, format!("{owner}: {name}: {e}")));
            }
        }
        for (owner, json) in &pending.params {
            apply_baked_params(&mut reg, owner, json, &mut log);
        }
        for (owner, files) in &pending.files {
            for (path, body) in files.iter() {
                if *path == "params.json" || *path == "policy.json" {
                    apply_baked_params(&mut reg, owner, body, &mut log);
                }
            }
        }
    }
    {
        let mut slots = world.res_mut::<ShaderSlots>();
        for (owner, files) in &pending.files {
            for (path, body) in files.iter() {
                let Some(name) = path
                    .strip_prefix("slots/")
                    .and_then(|p| p.strip_suffix(".wgsl"))
                else {
                    continue;
                };
                if let Err(e) = slots.set_default(name, body, owner) {
                    log.push((true, format!("{owner}: baked slot {name}: {e}")));
                }
            }
        }
    }
    {
        let mut f = world.res_mut::<Formulas>();
        for (owner, json) in &pending.formulas {
            match f.apply_values_json(json) {
                Ok(unknown) => {
                    for n in unknown {
                        log.push((
                            false,
                            format!("{owner}: baked formula {n} not in this build"),
                        ));
                    }
                }
                Err(e) => log.push((true, format!("{owner}: baked formulas: {e}"))),
            }
        }
        for (owner, files) in &pending.files {
            for (path, body) in files.iter() {
                let parsed = if let Some(n) = path
                    .strip_prefix("slots/")
                    .and_then(|p| p.strip_suffix(".expr"))
                {
                    (n, Ok(FormulaJson::Expr(body.trim_end().to_string())))
                } else if let Some(n) = path
                    .strip_prefix("curves/")
                    .and_then(|p| p.strip_suffix(".json"))
                {
                    (
                        n,
                        serde_json::from_str::<crate::curve::Curve>(body)
                            .map(FormulaJson::Curve)
                            .map_err(|e| e.to_string()),
                    )
                } else {
                    continue;
                };
                let (name, res) = parsed;
                let res = res.and_then(|j| f.set_by_name(name, j).map_err(|e| e.to_string()));
                if let Err(e) = res {
                    log.push((true, format!("{owner}: baked {path}: {e}")));
                }
            }
        }
    }
    let mut el = world.res_mut::<ErrorLog>();
    for (is_err, m) in log {
        if is_err {
            el.error(m);
        } else {
            el.note(m);
        }
    }
}

fn apply_baked_params(
    reg: &mut ParamRegistry,
    owner: &str,
    json: &str,
    log: &mut Vec<(bool, String)>,
) {
    match reg.apply_values_json(json) {
        Ok(rep) => {
            for n in rep.unknown {
                log.push((false, format!("{owner}: baked param {n} not in this build")));
            }
            for n in rep.clamped {
                log.push((false, format!("{owner}: baked param {n} clamped")));
            }
        }
        Err(e) => log.push((true, format!("{owner}: baked params: {e}"))),
    }
}

fn fallback_tiers() -> Tiers {
    let preset = crate::tiers::TierPreset {
        id: "low".into(),
        label: "Low".into(),
        fps_target: 60,
        fps_max: None,
        fps_floor: None,
        refresh_hz: None,
        target_resolution: None,
        render_scale: None,
        dpr_cap: None,
        max_draw_calls: 150,
        max_triangles: 100_000,
        max_sprites: 5_000,
        max_particles: 2_000, #[cfg(feature = "texture-budget")] max_texture_memory: 96, #[cfg(feature = "point-lights")] max_point_lights: 0, #[cfg(feature = "shadow-cascades")] shadow_cascades: 1, #[cfg(feature = "ibl")] ibl_cube: 0, #[cfg(feature = "gtao")] gtao: 0,
        points: 0,
        stereo: false,
        auto_select: true,
        msaa: 1,
        effects_on: Vec::new(),
        shadow_map: None,
        shadow_pcf: None,
    };
    Tiers::single(preset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{ActionDef, KeyCode, RawEvent};
    use crate::plugin::PluginError;
    use crate::scene::Scene;

    #[derive(Default)]
    struct Counter(u32);

    struct TestKit;
    impl KitPlugin for TestKit {
        fn name(&self) -> &'static str {
            "test"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            kit.resource(Counter::default());
            kit.action(ActionDef::button("jump", &["key:Space"])?);
            kit.param_f32(ParamDef::f32("kit.test.speed", 1.0).range(0.0, 10.0))?;
            kit.baked_params(r#"{"kit.test.speed": 4.0, "fx.gone.param": 1.0}"#);
            kit.system(SystemDesc::new("count", Stage::Update, |w| {
                w.res_mut::<Counter>().0 += 1;
            }));
            kit.scene("main", || Box::new(Main));
            Ok(())
        }
        fn start_scene(&self) -> Option<&'static str> {
            Some("main")
        }
    }

    struct Main;
    impl Scene for Main {
        fn name(&self) -> &str {
            "main"
        }
    }

    struct TestModule {
        frames: u32,
    }
    impl EngineModule for TestModule {
        fn id(&self) -> &'static str {
            "evani.module.test"
        }
        fn label(&self) -> &'static str {
            "test"
        }
        fn frame(&mut self, _world: &mut World) {
            self.frames += 1;
        }
        fn status(&self, _world: &World, out: &mut String) {
            out.push_str("ok");
        }
    }

    fn app() -> App {
        App::new("t")
            .with_kit(TestKit)
            .with_module(TestModule { frames: 0 })
    }

    #[test]
    fn fixed_ticks_and_baked_params() {
        let mut a = app();
        // 60 frames of 1/60 s -> 60 ticks (floating point may leave the
        // last one in the accumulator).
        a.run_headless(60, 1000.0 / 60.0);
        let ticks = a.world.res::<Time>().tick;
        assert!((59..=60).contains(&ticks), "{ticks}");
        assert_eq!(u64::from(a.world.res::<Counter>().0), ticks);
        let reg = a.world.res::<ParamRegistry>();
        let h = reg.f32_handle("kit.test.speed").unwrap();
        assert_eq!(reg.f32(h), 4.0);
        drop(reg);
        let log = a.world.res::<ErrorLog>();
        assert!(log.entries().any(|e| e.message.contains("fx.gone.param")));
        assert_eq!(log.error_count(), 0);
        assert_eq!(a.stack.top_name(), Some("main"));
    }

    /// Chris: "version numbers of things being used from Evani.dev". The
    /// overlay's first line names the engine, its commit and every part
    /// compiled in; a game-local module without a version is left out.
    #[test]
    fn overlay_first_line_lists_versions() {
        struct VersionedModule;
        impl EngineModule for VersionedModule {
            fn id(&self) -> &'static str {
                "evani.module.fx"
            }
            fn label(&self) -> &'static str {
                "fx"
            }
            fn version(&self) -> Option<&'static str> {
                Some("0.3.0")
            }
            fn version_features(&self) -> Vec<&'static str> {
                vec!["bloom", "outline"]
            }
        }
        struct VersionedKit;
        impl KitPlugin for VersionedKit {
            fn name(&self) -> &'static str {
                "kart-racer"
            }
            fn build(&self, _kit: &mut PluginContext) -> Result<(), PluginError> {
                Ok(())
            }
            fn version(&self) -> Option<&'static str> {
                Some("0.2.0")
            }
        }
        let mut a = App::new("t")
            .with_kit(VersionedKit)
            .with_module(TestModule { frames: 0 })
            .with_module(VersionedModule);
        a.run_headless(1, 16.0);
        a.update_overlay(0.0);
        let text = a.world.res::<Overlay>().text().to_string();
        let first = text.lines().next().unwrap_or_default();
        let v = crate::version::ENGINE_VERSION;
        let c = crate::version::ENGINE_COMMIT;
        assert_eq!(
            first,
            format!("evani {v} @{c} · core {v} · fx 0.3.0 [bloom outline] · kit kart-racer 0.2.0"),
            "{text}"
        );
        // The game-local test module (no version) still has its status
        // line, but is not an Evani.Dev part.
        assert!(text.contains("\ntest: ok\n"), "{text}");
        assert!(!first.contains("test"), "{first}");

        // A build without the fx module and without a kit lists neither.
        let mut b = App::new("t").with_module(TestModule { frames: 0 });
        b.update_overlay(0.0);
        let text = b.world.res::<Overlay>().text().to_string();
        assert_eq!(text.lines().next(), Some(format!("evani {v} @{c} · core {v}").as_str()));
        assert!(!text.contains("fx"), "{text}");
        assert_eq!(b.world.res::<BuildInfo>().parts.len(), 1);
    }

    #[test]
    fn slow_frames_run_several_ticks_and_clamp() {
        let mut a = app();
        a.run_headless(1, 51.0);
        assert_eq!(a.world.res::<Time>().tick, 3);
        a.run_headless(1, 10_000.0); // stall: clamped to 5 steps
        assert_eq!(a.world.res::<Time>().tick, 8);
    }

    #[cfg(feature = "capture")]
    #[test]
    fn capture_hold_pauses_the_sim_and_resumes() {
        use crate::capture::CaptureQueue;
        let mut a = app();
        a.world.insert_resource(CaptureQueue::default());
        a.run_headless(1, 51.0);
        assert_eq!(a.world.res::<Time>().tick, 3);
        a.world.res_mut::<CaptureQueue>().sim_time_override = Some(2.5);
        a.run_headless(30, 1000.0 / 60.0);
        {
            let t = a.world.res::<Time>();
            assert_eq!(t.tick, 3, "no ticks while held");
            assert_eq!(t.render_seconds(), 2.5);
        }
        a.world.res_mut::<CaptureQueue>().sim_time_override = None;
        a.run_headless(1, 51.0);
        let t = a.world.res::<Time>();
        assert_eq!(t.tick, 6, "resumes where it paused");
        assert!(t.held.is_none());
    }

    #[test]
    fn hidden_pauses() {
        let mut a = app();
        a.run_headless(10, 1000.0 / 60.0);
        let t = a.world.res::<Time>().tick;
        a.set_hidden(true);
        a.run_headless(10, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Time>().tick, t);
        a.set_hidden(false);
        a.frame(1_000_000.0); // first frame after resume only primes
        assert_eq!(a.world.res::<Time>().tick, t);
    }

    #[test]
    fn tier_change_event_and_overlay() {
        let mut a = app();
        a.run_headless(1, 16.0);
        assert!(a.world.res_mut::<Tiers>().set_active("ultra"));
        a.run_headless(20, 16.0);

        assert_eq!(a.world.res::<BudgetStatus>().total, 24);
        let text = a.world.res::<Overlay>().text().to_string();
        assert!(text.contains("tier ultra  budget 0/24 pts"), "{text}");
        assert!(text.contains("test: ok"), "{text}");
    }

    /// Gate 0 follow-up: the overlay's GPU lines are the 2 s median of the
    /// renderer's readings, fed by the frame loop, not the latest frame.
    #[test]
    fn overlay_shows_the_two_second_gpu_median() {
        use crate::overlay::PassTiming;
        let mut a = app();
        for i in 0..120u32 {
            // Every 10th reading is a 5x outlier (a clock dip).
            let ms = if i % 10 == 9 { 5.0 } else { 1.0 };
            {
                let mut r = a.world.res_mut::<RenderStats>();
                r.gpu_ms = Some(ms);
                r.passes.clear();
                r.passes.push(PassTiming {
                    name: "forward",
                    gpu_ms: ms * 0.5,
                });
                r.gpu_samples += 1;
            }
            a.run_headless(1, 1000.0 / 60.0);
        }
        let text = a.world.res::<Overlay>().text().to_string();
        assert!(text.contains("gpu 1.00 ms (median)"), "{text}");
        assert!(text.contains("passes  forward 0.50  (2 s median)"), "{text}");
    }

    #[test]
    fn input_edges_reach_one_tick() {
        let mut a = app();
        let jump = a.world.res::<Input>().action_id("jump").unwrap();
        a.run_headless(1, 16.7);
        a.world.res_mut::<Input>().handle(&RawEvent::Key {
            code: KeyCode::new("Space").unwrap(),
            down: true,
        });
        a.run_headless(1, 16.7);
        // The edge was consumed by the tick and cleared afterwards.
        assert!(!a.world.res::<Input>().just_pressed(jump));
        assert!(a.world.res::<Input>().down(jump));
    }

    #[test]
    fn core_params_drive_audio_and_overlay() {
        let mut a = app();
        a.world
            .res_mut::<ParamRegistry>()
            .set_json("core.audio.music", &serde_json::json!(0.25))
            .unwrap();
        a.world
            .res_mut::<ParamRegistry>()
            .set_json("core.overlay.interval_ms", &serde_json::json!(1000.0))
            .unwrap();
        a.run_headless(2, 16.0);
        assert_eq!(a.world.res::<AudioMixer>().volume(Some(Bus::Music)), 0.25);
        assert_eq!(a.world.res::<Overlay>().interval_ms, 1000.0);
    }

    static BAKED_FILES: &[(&str, &str)] = &[
        ("CHANGES.md", "# ignored"),
        ("curves/kit.baked.falloff.json", r#"{"points": [[0.0, 0.0], [1.0, 3.0]]}"#),
        ("params.json", r#"{"kit.baked.speed": 7.5}"#),
        ("policy.json", r#"{"aq.degrade_pct": 30.0}"#),
        ("slots/kit.baked.scale.expr", "x * 4\n"),
        ("slots/tonemap.wgsl", "    return color * 0.5;"),
    ];

    struct BakedKit;
    impl KitPlugin for BakedKit {
        fn name(&self) -> &'static str {
            "baked"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            kit.param_f32(ParamDef::f32("kit.baked.speed", 1.0).range(0.0, 10.0))?;
            kit.formula(
                "kit.baked.falloff",
                "test curve",
                &["x"],
                FormulaJson::Curve(
                    crate::curve::Curve::new(&[[0.0, 0.0], [1.0, 1.0]])
                        .map_err(|e| e.to_string())?,
                ),
            )?;
            kit.formula("kit.baked.scale", "test expr", &["x"], FormulaJson::Expr("x".into()))?;
            kit.baked_files(BAKED_FILES);
            Ok(())
        }
    }

    #[test]
    fn baked_files_apply_at_start() {
        let mut a = App::new("baked").with_kit(BakedKit);
        a.start();
        let reg = a.world.res::<ParamRegistry>();
        let speed = reg.f32_handle("kit.baked.speed").unwrap();
        assert_eq!(reg.f32(speed), 7.5);
        let aq = *a.world.res::<crate::quality::AqParams>();
        assert_eq!(reg.f32(aq.degrade_pct), 30.0);
        let f = a.world.res::<Formulas>();
        let c = f.id("kit.baked.falloff").unwrap();
        assert_eq!(f.eval(c, &[0.5]), 1.5);
        let e = f.id("kit.baked.scale").unwrap();
        assert_eq!(f.eval(e, &[2.0]), 8.0);
        let slots = a.world.res::<ShaderSlots>();
        let t = slots.get(slots.id("tonemap").unwrap()).unwrap();
        assert_eq!(t.body, "    return color * 0.5;");
        let log = a.world.res::<ErrorLog>();
        assert_eq!(log.error_count(), 0, "{:?}", log.entries().collect::<Vec<_>>());
    }

    // ---- frame order guard (EngineModule::draws) -------------------------------

    #[derive(Default)]
    struct OrderLog {
        calls: Vec<&'static str>,
        value: u64,
        seen: Vec<(&'static str, u64)>,
    }

    /// Logs its frame step; a drawing one also records the value it drew with,
    /// a writer sets the value to this frame's number.
    struct Probe {
        name: &'static str,
        draws: bool,
        writes: bool,
    }
    impl EngineModule for Probe {
        fn id(&self) -> &'static str {
            "evani.module.test_probe"
        }
        fn label(&self) -> &'static str {
            self.name
        }
        fn frame(&mut self, world: &mut World) {
            let frame = world.res::<Time>().frame;
            let mut log = world.res_mut::<OrderLog>();
            log.calls.push(self.name);
            if self.writes {
                log.value = frame;
            }
            if self.draws {
                let v = log.value;
                log.seen.push((self.name, v));
            }
        }
        fn draws(&self) -> bool {
            self.draws
        }
    }

    fn probe(name: &'static str, draws: bool, writes: bool) -> Probe {
        Probe { name, draws, writes }
    }

    /// The guard: a drawing module (the render module) is added first, as
    /// games must, yet draws with what every module added after it set in
    /// the same frame, and runs after all of them. Fails if App::frame ever
    /// runs module frame steps in plain add order again (the one-frame
    /// camera lag Broadside3D measured).
    #[test]
    fn drawing_modules_run_after_every_other_frame_step() {
        let mut a = App::new("t")
            .with_module(probe("render", true, false))
            .with_module(probe("nature", false, true))
            .with_module(probe("fx", false, false))
            .with_module(probe("overlay-draw", true, false));
        a.world.insert_resource(OrderLog::default());
        a.run_headless(3, 16.0);
        let log = a.world.res::<OrderLog>();
        let one = ["nature", "fx", "render", "overlay-draw"];
        assert_eq!(log.calls, [one, one, one].concat(), "update steps first, then drawing ones in add order");
        let seen: Vec<_> = log.seen.iter().filter(|s| s.0 == "render").map(|s| s.1).collect();
        assert_eq!(seen, [1, 2, 3], "each frame is drawn with that frame's value");
        drop(log);
        // Build, start and listing order is unchanged: the order added.
        let labels: Vec<_> = a.modules().map(|m| m.1).collect();
        assert_eq!(labels, ["render", "nature", "fx", "overlay-draw"]);
        // A hidden tab runs no frame step at all.
        a.set_hidden(true);
        a.run_headless(2, 16.0);
        assert_eq!(a.world.res::<OrderLog>().calls.len(), 12);
    }

    /// Modules that don't draw keep the order they were added in.
    #[test]
    fn update_modules_keep_their_order() {
        let mut a = App::new("t")
            .with_module(probe("a", false, false))
            .with_module(probe("b", false, true))
            .with_module(probe("c", false, false));
        a.world.insert_resource(OrderLog::default());
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<OrderLog>().calls, ["a", "b", "c"]);
    }
}
