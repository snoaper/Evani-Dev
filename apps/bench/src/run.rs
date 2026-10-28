//! The bench state machine: one `Stage::Frame` system that walks every tier
//! through settle and measure frames of the 3D phase, the 2D phase, the
//! particle phase, the model phase and the character phase, and judges the tier with [`crate::check::evaluate`].
//!
//! * It disables adaptive quality (`aq.enabled = false`) and forces the tier,
//!   so the run measures the tier, not the controller. It lifts the game's
//!   own particle cap (`fx.particles.max`) to its top, so the particle
//!   phase fills each tier's own `max_particles`. The first-run warm-up
//!   is skipped by saving the first tier as the "already warmed up" choice
//!   (the previous value is restored at the end).
//! * Device mode (`?device=1`) does the opposite: it clears the saved tier,
//!   lets the renderer's real first-run warm-up pick a tier while drawing
//!   the 3D content of each tier it tries, then benches every tier and
//!   judges only the picked one ([`Report::device_pick`],
//!   [`Report::device_pass`]; the page says `device picked <tier>: PASS`).
//! * Stats are read from the previous rendered frame (that is when the
//!   renderer writes them), always inside the same phase because every phase
//!   settles first.
//! * Frame time is the real time between frames in the browser
//!   (`Time::frame_dt`) and the CPU time of the engine's frame natively.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::effects::BudgetStatus;
use evani_core::overlay::{ErrorLog, Platform, RenderStats, Severity};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::particles::ParticleQueue;
use evani_core::plugin::LookData;
use evani_core::quality::{QualityState, SAVED_TIER_KEY};
use evani_core::save::SaveData;
use evani_core::tiers::{TierPreset, Tiers};
use evani_core::time::Time;
use evani_render::control::render_size;
use evani_render::RenderWorld;
use evani_render::particles::{GAME_CAP_PARAM, ParticleSystem};

use crate::check::{PhaseResult, TierResult, evaluate, evaluate_stress, summarize_phase};
use crate::content::{MODEL_URL, Phase, STRESS_FILL_LAYERS, STRESS_TRIANGLES, Stress, mesh_plan, model_plan, sprite_plan, triangle_budget};
use evani_render::model::{ModelId, ModelInstance, Models};

/// Canvas size natively (there is no canvas; sprites still need a size).
const NATIVE_VIEWPORT: [f32; 2] = [1920.0, 1080.0];
/// How long the GPU may take to come up before the run gives up (ms).
pub const GPU_WAIT_MS: f64 = 30_000.0;

/// Which tiers to run and for how long.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchConfig {
    /// Tier ids; empty = every tier in the tier files.
    pub tiers: Vec<String>,
    /// Frames per phase that are drawn but not measured (pipelines, targets
    /// and the effect budget settle).
    pub warmup_frames: u32,
    /// Measured frames per phase.
    pub measure_frames: u32,
    /// Force this MSAA sample count on every tier (`?msaa=1` or `4`), to
    /// measure what MSAA costs; `None` = each tier's own.
    pub msaa: Option<u32>,
    /// Effects switched off for the whole run (`?off=bloom,outline`), to
    /// measure what they cost.
    pub off: Vec<String>,
    /// The stress scene (`?stress=1`): the same fixed, GPU-heavy content on
    /// every tier ([`Stress::build_stress`]), 3D phase only, so the GPU
    /// stays clocked up and only the tier settings differ. For timing
    /// (cost order, per-pass cost); judged only on whether it ran and drew
    /// ([`crate::check::evaluate_stress`]).
    pub stress: bool,
    /// Fill layers of the stress scene (`?fill=N`); `None` =
    /// [`crate::content::STRESS_FILL_LAYERS`].
    pub stress_fill: Option<u32>,
    /// Device mode (`?device=1`): the renderer's own first-run tier warm-up
    /// runs first (as in every game), on the 3D content of whichever tier
    /// it is trying; then every tier is benched as usual, and only the tier
    /// the warm-up picked is judged ([`Report::device_pick`]).
    pub device: bool,
}

/// Measured frames per tier in the stress scene unless `frames=` says
/// otherwise.
pub const STRESS_MEASURE_FRAMES: u32 = 600;

impl BenchConfig {
    /// Browser defaults: 2 s settle, 5 s measured per phase at 60 Hz.
    pub fn browser() -> Self {
        Self {
            tiers: Vec::new(),
            warmup_frames: 120,
            measure_frames: 300,
            msaa: None,
            off: Vec::new(),
            stress: false,
            stress_fill: None,
            device: false,
        }
    }

    /// Native CPU-only defaults (short: it only counts and times the CPU).
    pub fn native() -> Self {
        Self {
            tiers: Vec::new(),
            warmup_frames: 10,
            measure_frames: 60,
            msaa: None,
            off: Vec::new(),
            stress: false,
            stress_fill: None,
            device: false,
        }
    }

    /// Apply `?tier=a,b&warmup=N&frames=N&msaa=N&off=a,b&stress=1` on top
    /// of `self`. `stress=1` measures [`STRESS_MEASURE_FRAMES`] frames
    /// unless `frames=` is given too.
    pub fn with_query(mut self, query: &str) -> Self {
        let mut frames_given = false;
        for pair in query.trim_start_matches('?').split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            match k {
                "tier" => {
                    self.tiers = v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
                }
                "warmup" => {
                    if let Ok(n) = v.parse() {
                        self.warmup_frames = n;
                    }
                }
                "frames" => {
                    if let Ok(n) = v.parse::<u32>()
                        && n > 0
                    {
                        self.measure_frames = n;
                        frames_given = true;
                    }
                }
                "msaa" => self.msaa = v.parse().ok().filter(|n| *n == 1 || *n == 4),
                "off" => {
                    self.off = v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
                }
                "stress" => self.stress = matches!(v, "1" | "true" | "on"),
                "fill" => self.stress_fill = v.parse().ok(),
                "device" => self.device = matches!(v, "1" | "true" | "on"),
                _ => {}
            }
        }
        if self.stress && !frames_given {
            self.measure_frames = self.measure_frames.max(STRESS_MEASURE_FRAMES);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    WaitGpu,
    /// Device mode: the renderer's tier warm-up is measuring; the bench
    /// draws the 3D content of whichever tier it is trying.
    DevicePick,
    Settle,
    Measure,
    Finished,
}

/// What the page publishes (`window.__bench`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub done: bool,
    pub error: Option<String>,
    pub mode: String,
    pub progress: String,
    pub results: Vec<TierResult>,
    /// Device mode: the tier this device's first-run warm-up picked (the
    /// same warm-up every game runs). Only this tier is judged; the other
    /// rows are information.
    #[serde(default)]
    pub device_pick: Option<String>,
    /// Device mode: did the picked tier pass its frame target and floor
    /// (and every other check)? Set when the run is done.
    #[serde(default)]
    pub device_pass: Option<bool>,
}

/// World resource driving the whole run.
pub struct BenchRun {
    cfg: BenchConfig,
    ids: Vec<String>,
    tier_i: usize,
    phase: Phase,
    stage: State,
    /// Frame number inside the current stage.
    idx: u32,
    /// Frame number inside the current phase (settle + measure): the camera
    /// path time.
    phase_frame: u32,
    content: Option<Stress>,
    frame_ms: Vec<f32>,
    /// GPU ms of every new GPU reading during the measured frames.
    gpu_ms: Vec<f32>,
    /// Per-pass GPU ms of every new reading the pass ran in, in first-seen
    /// order (median and mean per pass at the end of the phase).
    pass_samples: Vec<(&'static str, Vec<f32>)>,
    /// [`RenderStats::gpu_samples`] last seen: a reading is sampled once,
    /// when it arrives, not again on frames that bring no new one.
    last_gpu_sample: u64,
    acc: PhaseResult,
    phases: Vec<PhaseResult>,
    errors_at_start: u32,
    /// What `render.tier` held before the run (restored at the end).
    prev_saved: Option<Option<String>>,
    started_ms: f64,
    report: Report,
    render_size: Option<[u32; 2]>,
    /// Bumped whenever the report changes; the page is told only then.
    rev: u64,
    published_rev: u64,
    /// CPU ms spent submitting content over this phase's measured frames.
    submit_total_ms: f64,
    /// [`QualityState::disturbances`] last seen: a pipeline compile or swap
    /// (or a tier switch) bumps it, and the settle count starts again.
    disturbances: u64,
    /// Settle frames of this phase so far, restarts included.
    settle_total: u32,
    /// Device mode: the tier whose 3D content is drawn while the warm-up
    /// measures, and when the wait for its pick started (ms).
    content_tier: Option<String>,
    pick_started_ms: f64,
    /// The model phase's model, asked for when the run starts.
    model: Option<ModelId>,
    /// The character phase's rig, added when the run starts.
    crowd: Option<crate::characters::Crowd>,
}

impl BenchRun {
    pub fn new(cfg: BenchConfig) -> Self {
        Self {
            cfg,
            ids: Vec::new(),
            tier_i: 0,
            phase: Phase::ThreeD,
            stage: State::WaitGpu,
            idx: 0,
            phase_frame: 0,
            content: None,
            frame_ms: Vec::new(),
            gpu_ms: Vec::new(),
            pass_samples: Vec::new(),
            last_gpu_sample: 0,
            acc: PhaseResult::default(),
            phases: Vec::new(),
            errors_at_start: 0,
            prev_saved: None,
            started_ms: -1.0,
            report: Report::default(),
            render_size: None,
            rev: 1,
            published_rev: 0,
            submit_total_ms: 0.0,
            disturbances: 0,
            settle_total: 0,
            content_tier: None,
            pick_started_ms: 0.0,
            model: None,
            crowd: None,
        }
    }

    fn progress(&mut self, text: String) {
        self.report.progress = text;
        self.rev += 1;
    }

    /// The tier ids this run will go through (all tiers when `cfg.tiers` is
    /// empty). Unknown ids are an error the report carries.
    pub fn resolve_ids(cfg: &BenchConfig, tiers: &Tiers) -> Result<Vec<String>, String> {
        if cfg.tiers.is_empty() {
            return Ok(tiers.presets().iter().map(|p| p.id.clone()).collect());
        }
        for id in &cfg.tiers {
            if tiers.get(id).is_none() {
                return Err(format!("unknown tier '{id}'"));
            }
        }
        Ok(cfg.tiers.clone())
    }

    pub fn report(&self) -> &Report {
        &self.report
    }

    pub fn is_done(&self) -> bool {
        self.report.done
    }

    pub fn config(&self) -> &BenchConfig {
        &self.cfg
    }

    /// Id of the first tier (its saved value skips the warm-up).
    pub fn first_tier(&self, tiers: &Tiers) -> Option<String> {
        Self::resolve_ids(&self.cfg, tiers).ok().and_then(|v| v.first().cloned())
    }

    fn fail(&mut self, msg: impl Into<String>) {
        self.report.error = Some(msg.into());
        self.report.done = true;
        self.stage = State::Finished;
        self.rev += 1;
    }
}

/// The `Stage::Frame` system.
pub fn system() -> SystemDesc {
    SystemDesc::new("bench.run", Stage::Frame, step)
}

fn viewport(world: &World, web: bool) -> [f32; 2] {
    if web {
        world.res::<RenderWorld>().viewport_css
    } else {
        NATIVE_VIEWPORT
    }
}

fn materials(world: &World) -> [u32; 3] {
    let look = world.res::<LookData>();
    [
        RenderWorld::material(&look, "ground"),
        RenderWorld::material(&look, "accent"),
        RenderWorld::material(&look, "glow"),
    ]
}

fn active_tier(world: &World, run: &BenchRun) -> Option<TierPreset> {
    run.ids
        .get(run.tier_i)
        .and_then(|id| world.res::<Tiers>().get(id).cloned())
}

fn step(world: &mut World) {
    let world: &World = world;
    let web = world.res::<Platform>().web;
    let now_ms = world.res::<Time>().now_ms;
    let mut run = world.res_mut::<BenchRun>();
    match run.stage {
        State::Finished => {}
        State::WaitGpu => wait_gpu(world, &mut run, web, now_ms),
        State::DevicePick => device_pick(world, &mut run, web, now_ms),
        State::Settle | State::Measure => frame(world, &mut run, web),
    }
    if run.rev != run.published_rev {
        run.published_rev = run.rev;
        publish(&run);
    }
}

/// Wait for the renderer to be live, then set the run up.
fn wait_gpu(world: &World, run: &mut BenchRun, web: bool, now_ms: f64) {
    if run.started_ms < 0.0 {
        run.started_ms = now_ms;
        run.report.mode = if web { "browser-gpu" } else { "native-cpu" }.to_string();
        run.rev += 1;
        run.progress("waiting for WebGPU".to_string());
    }
    // The renderer sets the canvas size once it has drawn a real frame.
    let live = !web || viewport(world, true).iter().all(|v| *v > 0.0);
    if !live {
        if now_ms - run.started_ms > GPU_WAIT_MS {
            let notes: Vec<String> = world
                .res::<ErrorLog>()
                .entries()
                .filter(|e| e.severity == Severity::Error)
                .map(|e| e.message.clone())
                .collect();
            run.fail(format!(
                "WebGPU did not come up within {} s{}",
                (GPU_WAIT_MS / 1000.0) as u32,
                if notes.is_empty() { String::new() } else { format!(": {}", notes.join("; ")) }
            ));
        }
        return;
    }
    if let Err(e) = setup(world, run) {
        return run.fail(e);
    }
    if run.cfg.device {
        run.stage = State::DevicePick;
        run.pick_started_ms = now_ms;
        run.progress("device: tier warm-up measuring".to_string());
        return device_pick(world, run, web, now_ms);
    }
    begin_tier(world, run);
}

/// How long device mode waits for the warm-up to pick a tier (ms). The
/// warm-up itself takes about [`evani_render::control::WARMUP_TOTAL_MS`]
/// plus settle frames; the rest is room for slow pipeline compiles.
pub const DEVICE_PICK_WAIT_MS: f64 = 30_000.0;

/// Device mode, while the renderer's first-run warm-up measures: draw the
/// 3D content of the tier it is trying (rebuilt when it moves on), and
/// once it has picked, bench every tier as usual (the pick is judged).
fn device_pick(world: &World, run: &mut BenchRun, web: bool, now_ms: f64) {
    let pick = world.try_res::<QualityState>().and_then(|q| q.warmup_pick.clone());
    if let Some(id) = pick {
        if !run.ids.contains(&id) {
            run.ids.push(id.clone());
        }
        run.report.device_pick = Some(id);
        run.content = None;
        run.content_tier = None;
        // Nothing of the warm-up content may draw in the first tier.
        if let Some(mut ps) = world.try_res_mut::<ParticleSystem>() {
            ps.clear();
        }
        run.rev += 1;
        return begin_tier(world, run);
    }
    if now_ms - run.pick_started_ms > DEVICE_PICK_WAIT_MS {
        return run.fail(format!(
            "device mode: the first-run tier warm-up did not pick a tier within {} s (tab hidden, or no GPU timing?)",
            (DEVICE_PICK_WAIT_MS / 1000.0) as u32
        ));
    }
    let active = world.res::<Tiers>().active().clone();
    if run.content_tier.as_deref() != Some(active.id.as_str()) {
        run.content = Some(Stress::build(&active, Phase::ThreeD, materials(world)));
        run.content_tier = Some(active.id.clone());
        run.phase = Phase::ThreeD;
        run.phase_frame = 0;
        run.progress(format!("device: tier warm-up measuring {}", active.id));
    }
    let vp = viewport(world, web);
    if let Some(c) = &run.content {
        c.draw(&mut world.res_mut::<RenderWorld>(), run.phase_frame, vp);
        c.emit_particles(&mut world.res_mut::<ParticleQueue>(), run.phase_frame, vp);
    }
    run.phase_frame += 1;
}

/// Tier ids and the run-wide settings: adaptive quality off, the game's
/// particle cap lifted, `off=` effects off, a forced MSAA count.
fn setup(world: &World, run: &mut BenchRun) -> Result<(), String> {
    run.ids = BenchRun::resolve_ids(&run.cfg, &world.res::<Tiers>())?;
    // Adaptive quality off: the bench measures the tier, not the controller.
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        let set = reg
            .id("aq.enabled")
            .ok_or_else(|| "aq.enabled is not registered".to_string())
            .and_then(|id| reg.set(id, ParamValue::Bool(false)).map(|_| ()).map_err(|e| format!("{e:?}")));
        set.map_err(|e| format!("cannot disable adaptive quality: {e}"))?;
        // The tier's particle cap, not the game's lower one, bounds the
        // particle phase.
        if let Some(id) = reg.id(GAME_CAP_PARAM) {
            let top = reg.def(id).map_or(250_000.0, |d| d.max);
            reg.set(id, ParamValue::F32(top)).map_err(|e| format!("cannot lift {GAME_CAP_PARAM}: {e:?}"))?;
        }
        // Effects the run switches off (to measure what they cost).
        for name in run.cfg.off.clone() {
            let key = format!("fx.{name}.enabled");
            let set = reg
                .id(&key)
                .ok_or_else(|| format!("no effect '{name}' in this build ({key})"))
                .and_then(|id| reg.set(id, ParamValue::Bool(false)).map(|_| ()).map_err(|e| format!("{e:?}")));
            set?;
        }
    }
    // The model phase's model: fetched now (browser), so it is on the GPU
    // by the time the phase starts; read from the repository natively.
    if !run.cfg.stress
        && let Some(mut models) = world.try_res_mut::<Models>()
    {
        run.model = Some(load_model(&mut models)?);
        run.crowd = Some(crate::characters::Crowd::load(&mut models)?);
    }
    // A forced MSAA count replaces each tier's own.
    if let Some(n) = run.cfg.msaa {
        let mut tiers = world.res_mut::<Tiers>();
        for id in &run.ids {
            if let Some(mut p) = tiers.get(id).cloned() {
                p.msaa = n;
                tiers.upsert(p);
            }
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn load_model(models: &mut Models) -> Result<ModelId, String> {
    Ok(models.load(MODEL_URL))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_model(models: &mut Models) -> Result<ModelId, String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../sandbox/assets/models/proof-figure.glb");
    let glb = std::fs::read(path).map_err(|e| format!("model phase: {path}: {e}"))?;
    Ok(models.add_glb(MODEL_URL, &glb))
}

/// The model phase's copies for this frame. Returns whether the model is
/// ready (it draws nothing until then); once it is, the phase's plan is the
/// backdrop plus what the copies draw.
fn draw_models(world: &World, run: &mut BenchRun) -> bool {
    let (Some(id), Some(c), Some(tier)) = (run.model, run.content.as_ref(), active_tier(world, run)) else {
        return false;
    };
    let Some(mut models) = world.try_res_mut::<Models>() else {
        return false;
    };
    if !models.ready(id) {
        return false;
    }
    let tris = models.triangles(id);
    let n = model_plan(&tier, tris);
    for [x, y, z, yaw] in c.model_placements(n) {
        let (s, co) = (yaw * 0.5).sin_cos();
        models.draw(ModelInstance::new(id, [x, y, z]).rotation([0.0, s, 0.0, co]));
    }
    run.acc.planned_triangles = c.triangles + n * tris;
    true
}

/// The character phase's rigs for this frame, each posed for the phase
/// frame. Returns whether the rig is ready (nothing is drawn until then).
fn draw_characters(world: &World, run: &mut BenchRun) -> bool {
    let (Some(crowd), Some(c)) = (run.crowd.as_mut(), run.content.as_ref()) else {
        return false;
    };
    let Some(mut models) = world.try_res_mut::<Models>() else {
        return false;
    };
    if !models.ready(crowd.model) {
        return false;
    }
    let tris = models.triangles(crowd.model);
    let at = c.model_placements(crate::characters::CHARACTERS);
    // Planned for every copy: a copy whose pose fails is content not drawn.
    crowd.draw(&mut models, &at, run.phase_frame);
    run.acc.planned_triangles = c.triangles + crate::characters::CHARACTERS * tris;
    true
}

fn begin_tier(world: &World, run: &mut BenchRun) {
    let Some(id) = run.ids.get(run.tier_i).cloned() else {
        return finish(world, run);
    };
    // The bench is the one place the VR preset runs outside XR (mono).
    if !world.res_mut::<Tiers>().set_active_preset_run(&id) {
        return run.fail(format!("unknown tier '{id}'"));
    }
    run.errors_at_start = world.res::<ErrorLog>().error_count();
    run.phases.clear();
    run.phase = Phase::ThreeD;
    begin_phase(world, run);
}

fn begin_phase(world: &World, run: &mut BenchRun) {
    let Some(tier) = active_tier(world, run) else {
        return run.fail("tier vanished");
    };
    let content = if run.cfg.stress {
        Stress::build_stress_with(run.cfg.stress_fill.unwrap_or(STRESS_FILL_LAYERS), materials(world))
    } else {
        Stress::build(&tier, run.phase, materials(world))
    };
    run.acc = PhaseResult {
        name: run.phase.name().to_string(),
        planned_triangles: match run.phase {
            Phase::ThreeD if run.cfg.stress => mesh_plan(STRESS_TRIANGLES).1,
            Phase::ThreeD => mesh_plan(triangle_budget(&tier, Phase::ThreeD)).1,
            // Until the model is ready, the full share: a model that never
            // arrives fails the phase as content not drawn.
            Phase::Models | Phase::Characters => (tier.max_triangles as f32 * crate::content::TRI_FILL) as u32,
            Phase::TwoD | Phase::Particles => 0,
        },
        planned_sprites: match run.phase {
            Phase::ThreeD | Phase::Particles | Phase::Models | Phase::Characters => 0,
            Phase::TwoD => sprite_plan(tier.max_sprites),
        },
        planned_particles: content.particles,
        ..PhaseResult::default()
    };
    run.content = Some(content);
    run.frame_ms.clear();
    run.gpu_ms.clear();
    run.pass_samples.clear();
    run.submit_total_ms = 0.0;
    run.idx = 0;
    run.phase_frame = 0;
    run.settle_total = 0;
    run.stage = State::Settle;
    run.progress(format!("{} {} settling", tier.id, run.phase.name()));
}

/// The settle part of a phase lasts at most this many times
/// `warmup_frames` when pipelines keep compiling.
pub const SETTLE_RESTART_CAP: u32 = 10;

/// A stress phase in the browser measures until it has `measure_frames`
/// GPU readings, or this many times `measure_frames` frames.
pub const STRESS_FRAME_CAP: u32 = 4;

/// One settle or measure frame: sample the last frame (measure only), then
/// draw this one.
fn frame(world: &World, run: &mut BenchRun, web: bool) {
    let measuring = run.stage == State::Measure;
    if measuring {
        sample(world, run, web);
    }
    let vp = viewport(world, web);
    if let Some(c) = &run.content {
        let t0 = evani_core::time::now_ms();
        c.draw(&mut world.res_mut::<RenderWorld>(), run.phase_frame, vp);
        c.emit_particles(&mut world.res_mut::<ParticleQueue>(), run.phase_frame, vp);
        if measuring {
            run.submit_total_ms += evani_core::time::now_ms() - t0;
        }
    }
    let waiting_for_model = match run.phase {
        Phase::Models => !draw_models(world, run),
        Phase::Characters => !draw_characters(world, run),
        _ => false,
    };
    run.phase_frame += 1;
    run.idx += 1;
    // Settle counts frames after the last pipeline compile or swap: a fast
    // tier (Low at well under 1 ms a frame) would otherwise finish its
    // settle frames before an effect's pipeline is ready and measure
    // without that pass. At most `SETTLE_RESTART_CAP` times the settle
    // frames in all, so a pipeline that never finishes cannot stall it.
    if !measuring {
        run.settle_total += 1;
        let d = world.try_res::<QualityState>().map_or(0, |q| q.disturbances());
        // The model phase also settles only once its model is on the GPU.
        if d != run.disturbances || waiting_for_model {
            run.disturbances = d;
            if run.settle_total < run.cfg.warmup_frames.saturating_mul(SETTLE_RESTART_CAP) {
                run.idx = 0;
            }
        }
    }
    let (limit, next) = if measuring {
        (run.cfg.measure_frames, State::Finished)
    } else {
        (run.cfg.warmup_frames, State::Measure)
    };
    // The stress scene in the browser counts GPU readings (one frame's GPU
    // time each), not frames: a GPU-bound tier is not timed on every frame.
    // At most `STRESS_FRAME_CAP` times the frames, so it always ends.
    let done = if measuring && run.cfg.stress && web {
        run.gpu_ms.len() as u32 >= limit || run.idx >= limit.saturating_mul(STRESS_FRAME_CAP)
    } else {
        run.idx >= limit
    };
    if done {
        if measuring {
            end_phase(world, run);
        } else {
            run.stage = next;
            run.idx = 0;
            if let Some(tier) = active_tier(world, run) {
                let q = world.try_res::<QualityState>().map_or(1.0, |q| q.scale);
                let dpr = world.res::<Platform>().device_pixel_ratio;
                run.render_size = web.then(|| render_size(vp, dpr, &tier, q, 16_384));
                run.progress(format!("{} {} measuring", tier.id, run.phase.name()));
            }
        }
    } else if run.idx.is_multiple_of(30) {
        let what = if measuring { "measuring" } else { "settling" };
        let text = format!(
            "{} {} {what} {}/{}",
            run.ids.get(run.tier_i).map_or("", String::as_str),
            run.phase.name(),
            run.idx,
            limit
        );
        run.progress(text);
    }
}

fn sample(world: &World, run: &mut BenchRun, web: bool) {
    let t = world.res::<Time>();
    let ms = if web { t.frame_dt * 1000.0 } else { t.cpu_ms };
    drop(t);
    run.frame_ms.push(ms);
    let st = world.res::<RenderStats>();
    if let Some(g) = st.gpu_ms
        && st.gpu_samples != run.last_gpu_sample
    {
        run.last_gpu_sample = st.gpu_samples;
        run.gpu_ms.push(g);
        for p in &st.passes {
            match run.pass_samples.iter_mut().find(|(n, _)| *n == p.name) {
                Some(s) => s.1.push(p.gpu_ms),
                None => run.pass_samples.push((p.name, vec![p.gpu_ms])),
            }
        }
    }
    run.acc.draw_calls = run.acc.draw_calls.max(st.draw_calls);
    run.acc.triangles = run.acc.triangles.max(st.triangles);
    run.acc.sprites = run.acc.sprites.max(st.sprites);
    run.acc.particles = run.acc.particles.max(st.particles);
    drop(st);
    run.acc.points_used = run.acc.points_used.max(world.res::<BudgetStatus>().used);
}

fn end_phase(world: &World, run: &mut BenchRun) {
    let mut done = summarize_phase(run.phase.name(), &mut run.frame_ms, &run.gpu_ms);
    done.submit_ms = (run.submit_total_ms / f64::from(done.frames.max(1))) as f32;
    done.gpu_samples = run.gpu_ms.len() as u32;
    done.gpu_passes = crate::check::pass_stats(&mut run.pass_samples, run.gpu_ms.len());
    done.draw_calls = run.acc.draw_calls;
    done.triangles = run.acc.triangles;
    done.sprites = run.acc.sprites;
    done.particles = run.acc.particles;
    done.planned_particles = run.acc.planned_particles;
    done.points_used = run.acc.points_used;
    done.planned_triangles = run.acc.planned_triangles;
    done.planned_sprites = run.acc.planned_sprites;
    run.phases.push(done);
    match run.phase {
        // The stress scene has one phase.
        Phase::ThreeD if run.cfg.stress => end_tier(world, run),
        Phase::ThreeD => {
            run.phase = Phase::TwoD;
            begin_phase(world, run);
        }
        Phase::TwoD => {
            run.phase = Phase::Particles;
            begin_phase(world, run);
        }
        Phase::Particles => {
            // Nothing of this tier's particles may draw in the next phase.
            if let Some(mut ps) = world.try_res_mut::<ParticleSystem>() {
                ps.clear();
            }
            run.phase = Phase::Models;
            begin_phase(world, run);
        }
        Phase::Models => {
            run.phase = Phase::Characters;
            begin_phase(world, run);
        }
        Phase::Characters => end_tier(world, run),
    }
}

fn end_tier(world: &World, run: &mut BenchRun) {
    let Some(tier) = active_tier(world, run) else {
        return run.fail("tier vanished");
    };
    let log = world.res::<ErrorLog>();
    let new_errors = log.error_count().saturating_sub(run.errors_at_start);
    let first = log
        .entries()
        .find(|e| e.severity == Severity::Error)
        .map(|e| e.message.clone())
        .unwrap_or_default();
    drop(log);
    let judge = if run.cfg.stress { evaluate_stress } else { evaluate };
    let mut result = judge(&tier, &run.phases, Some((new_errors, first)));
    result.mode = run.report.mode.clone();
    result.render_size = run.render_size;
    run.report.results.push(result);
    run.rev += 1;
    run.tier_i += 1;
    if run.tier_i < run.ids.len() {
        begin_tier(world, run);
    } else {
        finish(world, run);
    }
}

fn finish(world: &World, run: &mut BenchRun) {
    run.content = None;
    // Put back whatever tier choice the page had before the bench.
    if let Some(mut save) = world.try_res_mut::<SaveData>() {
        match run.prev_saved.take() {
            Some(Some(v)) => {
                let _ = save.save_raw(SAVED_TIER_KEY, &v);
            }
            Some(None) => {
                let _ = save.delete(SAVED_TIER_KEY);
            }
            None => {}
        }
    }
    if let Some(pick) = &run.report.device_pick {
        run.report.device_pass = Some(run.report.results.iter().any(|r| &r.tier == pick && r.pass));
    }
    run.progress("done".to_string());
    run.report.done = true;
    run.stage = State::Finished;
}

/// Hand the report to the page (browser) at the moments it changes.
fn publish(run: &BenchRun) {
    #[cfg(target_arch = "wasm32")]
    crate::page::publish(run.report());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = run;
}

/// Called by `build_app`: save the first tier as the chosen one, so the
/// renderer's first-run warm-up does not run, and remember what was there.
pub fn skip_warmup(world: &mut World, run: &mut BenchRun) {
    let first = run.first_tier(&world.res::<Tiers>());
    let Some(first) = first else { return };
    if let Some(mut save) = world.try_res_mut::<SaveData>() {
        run.prev_saved = Some(save.load_raw(SAVED_TIER_KEY).ok().flatten());
        let _ = save.save_raw(SAVED_TIER_KEY, &first);
    }
}

/// Called by `build_app` in device mode: clear the saved tier so the
/// renderer's first-run warm-up runs, as on a game's first visit, and
/// remember what was there (put back at the end).
pub fn prepare_device(world: &mut World, run: &mut BenchRun) {
    if let Some(mut save) = world.try_res_mut::<SaveData>() {
        run.prev_saved = Some(save.load_raw(SAVED_TIER_KEY).ok().flatten());
        let _ = save.delete(SAVED_TIER_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_overrides_the_config() {
        let c = BenchConfig::browser().with_query("?tier=low,high&warmup=5&frames=20&x=1");
        assert_eq!(c.tiers, ["low", "high"]);
        assert_eq!((c.warmup_frames, c.measure_frames), (5, 20));
        let c = BenchConfig::browser().with_query("");
        assert_eq!(c, BenchConfig::browser());
        let c = BenchConfig::browser().with_query("frames=0&warmup=abc");
        assert_eq!(c, BenchConfig::browser());
        let c = BenchConfig::browser().with_query("msaa=1&off=bloom,outline");
        assert_eq!(c.msaa, Some(1));
        assert_eq!(c.off, ["bloom", "outline"]);
        assert_eq!(BenchConfig::browser().with_query("msaa=3").msaa, None);
        // Stress: 600 measured frames unless frames= says otherwise.
        let c = BenchConfig::browser().with_query("?stress=1&tier=high");
        assert!(c.stress);
        assert_eq!(c.measure_frames, STRESS_MEASURE_FRAMES);
        let c = BenchConfig::browser().with_query("frames=50&stress=1");
        assert!(c.stress);
        assert_eq!(c.measure_frames, 50);
        assert!(!BenchConfig::browser().with_query("stress=0").stress);
    }

    #[test]
    fn resolves_tier_ids() {
        let t = Tiers::builtin().unwrap();
        let all = BenchRun::resolve_ids(&BenchConfig::native(), &t).unwrap();
        assert_eq!(all, ["lite", "low", "medium", "high", "ultra", "vr"]);
        let mut cfg = BenchConfig::native();
        cfg.tiers = vec!["vr".into()];
        assert_eq!(BenchRun::resolve_ids(&cfg, &t).unwrap(), ["vr"]);
        cfg.tiers = vec!["potato".into()];
        assert!(BenchRun::resolve_ids(&cfg, &t).is_err());
    }

    /// Frames (60 Hz, headless) until the first tier starts measuring,
    /// calling `QualityState::disturb` (a pipeline compiling) on the first
    /// `busy` frames.
    fn frames_to_measure(busy: u32) -> u32 {
        let cfg = BenchConfig {
            tiers: vec!["low".into()],
            warmup_frames: 5,
            measure_frames: 4,
            msaa: None,
            off: Vec::new(),
            stress: false,
            stress_fill: None,
            device: false,
        };
        let mut app = crate::build_app(cfg);
        for n in 0..500 {
            if app.world.res::<BenchRun>().stage == State::Measure {
                return n;
            }
            if n < busy {
                app.world.res_mut::<QualityState>().disturb();
            }
            app.run_headless(1, 1000.0 / 60.0);
        }
        panic!("never measured");
    }

    #[test]
    fn settle_waits_for_pipelines_then_caps() {
        let calm = frames_to_measure(0);
        // A compile in flight for 20 frames: settle starts again after it.
        let compiling = frames_to_measure(20);
        assert!(compiling >= calm + 15, "calm {calm}, compiling {compiling}");
        // One that never finishes: settle gives up at 10 x warmup frames.
        let stuck = frames_to_measure(10_000);
        assert!(stuck <= calm + 5 * SETTLE_RESTART_CAP + 2, "calm {calm}, stuck {stuck}");
        assert!(stuck >= 5 * SETTLE_RESTART_CAP, "stuck {stuck}");
    }

    /// Device mode: while the warm-up measures, the bench draws the 3D
    /// content of whichever tier is active; once it picks (set by hand here:
    /// native builds have no renderer warm-up), every tier is benched and
    /// only the pick is judged; the saved tier choice is put back.
    #[test]
    fn device_mode_waits_for_the_pick_then_judges_only_it() {
        let cfg = BenchConfig {
            tiers: Vec::new(),
            warmup_frames: 2,
            measure_frames: 4,
            msaa: None,
            off: Vec::new(),
            stress: false,
            stress_fill: None,
            device: true,
        };
        let mut app = crate::build_app(cfg);
        // The saved tier is cleared so the warm-up runs.
        assert_eq!(app.world.res::<SaveData>().load_raw(SAVED_TIER_KEY).unwrap(), None);
        // What the page had saved before the bench comes back at the end.
        app.world.res_mut::<BenchRun>().prev_saved = Some(Some("high".into()));
        app.run_headless(5, 1000.0 / 60.0);
        {
            let run = app.world.res::<BenchRun>();
            assert_eq!(run.stage, State::DevicePick);
            let active = app.world.res::<Tiers>().active().id.clone();
            assert_eq!(run.content_tier.as_deref(), Some(active.as_str()));
            assert!(run.phase_frame >= 4);
            assert!(!run.report.done);
        }
        assert!(app.world.res::<RenderStats>().triangles > 0, "warm-up content not drawn");
        // The warm-up tries another tier: its content follows.
        assert!(app.world.res_mut::<Tiers>().set_active("medium"));
        app.run_headless(2, 1000.0 / 60.0);
        assert_eq!(app.world.res::<BenchRun>().content_tier.as_deref(), Some("medium"));
        // The warm-up picks Lite (and saves it, as the renderer does).
        app.world.res_mut::<QualityState>().warmup_pick = Some("lite".into());
        let _ = app.world.res_mut::<SaveData>().save_raw(SAVED_TIER_KEY, "lite");
        for _ in 0..2_000 {
            if app.world.res::<BenchRun>().is_done() {
                break;
            }
            app.run_headless(1, 1000.0 / 60.0);
        }
        let r = app.world.res::<BenchRun>().report().clone();
        assert!(r.done, "{:?}", r.error);
        assert_eq!(r.error, None);
        let ids: Vec<&str> = r.results.iter().map(|t| t.tier.as_str()).collect();
        assert_eq!(ids, ["lite", "low", "medium", "high", "ultra", "vr"]);
        assert_eq!(r.device_pick.as_deref(), Some("lite"));
        let lite = r.results.iter().find(|t| t.tier == "lite").map(|t| t.pass);
        assert_eq!(r.device_pass, lite);
        assert!(r.device_pass.is_some());
        assert_eq!(
            app.world.res::<SaveData>().load_raw(SAVED_TIER_KEY).unwrap().as_deref(),
            Some("high")
        );
        let line = crate::check::device_line("lite", &r.results);
        assert!(line.starts_with("device picked lite: "), "{line}");
    }
}
