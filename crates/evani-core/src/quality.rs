//! Adaptive quality: hold the tier's frame-time target by pausing the
//! effects that give the least value per budget point first, then stepping
//! the render scale down in 0.05 steps to a floor (never under 80 % of the
//! tier's scale), and recovering in reverse when there is headroom again.
//! It judges the frame's real work time (CPU + GPU; a percentile over a
//! short window, so one slow frame never counts), never the vsync-bound
//! frame interval, and ignores frames while pipelines rebuild, the tier
//! switches or a live change lands (plus a grace time). Plus the first-run
//! tier warm-up.
//!
//! This ships in games (it is not part of evani-live). The logic here is
//! pure and tested natively; evani-render feeds it frame samples and applies
//! the actions, evani-fx reads [`QualityState`] when it plans the budget.
//!
//! Every number is a registered policy parameter (`aq.*`, owner `aq`,
//! baked into `policy.json`), named as docs/LIVE_API.md `policy.get` lists
//! them.

use std::collections::VecDeque;

use crate::params::{
    BakeTarget, BoolParam, EnumParam, F32Param, I32Param, ParamDef, ParamError, ParamRegistry,
};
use crate::tiers::Tiers;

/// `aq.degrade_order` options.
pub const DEGRADE_ORDERS: [&str; 2] = ["value_per_point", "scale_first"];

/// Adaptive quality never renders below this fraction of the tier's own
/// render scale (the `aq.render_scale_floor` range starts here, and
/// [`QualityState::apply`] clamps to it as well).
pub const MIN_SCALE_FRACTION: f32 = 0.8;

/// Handles of the `aq.*` policy params.
#[derive(Debug, Clone, Copy)]
pub struct AqParams {
    pub enabled: BoolParam,
    pub target_ms: F32Param,
    pub window_ms: F32Param,
    pub degrade_pct: F32Param,
    pub safe_pct: F32Param,
    pub hysteresis_pct: F32Param,
    pub degrade_frames: I32Param,
    pub upgrade_frames: I32Param,
    pub cooldown_ms: F32Param,
    pub render_scale_step: F32Param,
    pub render_scale_floor: F32Param,
    pub degrade_order: EnumParam,
    pub percentile: F32Param,
    pub grace_ms: F32Param,
    pub fast_pct: F32Param,
    pub fast_ms: F32Param,
}

fn pol(def: ParamDef) -> ParamDef {
    def.owner("aq").bake(BakeTarget::Policy)
}

impl AqParams {
    /// Register every `aq.*` param (defaults = [`AqPolicy::default`]).
    /// Idempotent: returns the existing handles when `aq.*` is already
    /// registered (the app registers it in `App::new`).
    pub fn register(reg: &mut ParamRegistry) -> Result<Self, ParamError> {
        if let Some(h) = Self::lookup(reg) {
            return Ok(h);
        }
        let d = AqPolicy::default();
        Ok(Self {
            enabled: reg.register_bool(pol(ParamDef::bool("aq.enabled", d.enabled)
                .describe("Adaptive quality on: pause effects, then lower render scale, when the frame's real work time (CPU + GPU) is over the target.")))?,
            target_ms: reg.register_f32(pol(ParamDef::f32("aq.target_ms", 0.0)
                .range(0.0, 100.0)
                .step(0.1)
                .unit("ms")
                .describe("Work-time target; 0 = the active tier's frame budget (1000 / fps_target).")))?,
            window_ms: reg.register_f32(pol(ParamDef::f32("aq.window_ms", d.window_ms)
                .range(50.0, 5000.0)
                .step(10.0)
                .unit("ms")
                .describe("Smoothing window of the work-time average.")))?,
            degrade_pct: reg.register_f32(pol(ParamDef::f32("aq.degrade_pct", d.degrade_pct)
                .range(0.0, 100.0)
                .step(1.0)
                .unit("%")
                .describe("How far over the target the smoothed work time must be before quality steps down.")))?,
            safe_pct: reg.register_f32(pol(ParamDef::f32("aq.safe_pct", d.safe_pct)
                .range(10.0, 100.0)
                .step(1.0)
                .unit("%")
                .describe("Work under this share of the target is safe: quality never steps down there, and only steps back up below it.")))?,
            hysteresis_pct: reg.register_f32(pol(ParamDef::f32("aq.hysteresis_pct", d.hysteresis_pct)
                .range(0.0, 50.0)
                .step(1.0)
                .unit("%")
                .describe("Step hysteresis: undo a step only if the work it saved, added back, still leaves this much of the target free.")))?,
            degrade_frames: reg.register_i32(pol(ParamDef::i32("aq.degrade_frames", d.degrade_frames as i32)
                .range(1.0, 600.0)
                .unit("frames")
                .describe("Consecutive measured frames over the degrade line before quality steps down.")))?,
            upgrade_frames: reg.register_i32(pol(ParamDef::i32("aq.upgrade_frames", d.upgrade_frames as i32)
                .range(1.0, 3600.0)
                .unit("frames")
                .describe("Consecutive measured frames under the safe line before quality steps back up.")))?,
            cooldown_ms: reg.register_f32(pol(ParamDef::f32("aq.cooldown_ms", d.cooldown_ms)
                .range(0.0, 30_000.0)
                .step(50.0)
                .unit("ms")
                .describe("Minimum time between two quality steps (doubled with prefers-reduced-motion).")))?,
            render_scale_step: reg.register_f32(pol(ParamDef::f32("aq.render_scale_step", d.scale_step)
                .range(0.05, 0.1)
                .step(0.05)
                .unit("x")
                .describe("Render-scale change per step once no effect is left to pause.")))?,
            render_scale_floor: reg.register_f32(pol(ParamDef::f32("aq.render_scale_floor", d.scale_floor)
                .range(MIN_SCALE_FRACTION, 1.0)
                .step(0.05)
                .unit("x")
                .describe("Lowest render scale adaptive quality may use, as a fraction of the tier's own scale (never under 0.8).")))?,
            degrade_order: reg.register_enum(pol(ParamDef::enumeration("aq.degrade_order", &DEGRADE_ORDERS, 0)
                .describe("value_per_point: pause the lowest value-per-point effect first, then scale; scale_first: the reverse.")))?,
            percentile: reg.register_f32(pol(ParamDef::f32("aq.percentile", d.percentile)
                .range(50.0, 99.0)
                .step(1.0)
                .unit("%")
                .describe("Work time judged: this percentile of the frames in the window (80 = quality steps down only when more than a fifth of the frames are over), so one slow frame never cuts quality.")))?,
            grace_ms: reg.register_f32(pol(ParamDef::f32("aq.grace_ms", d.grace_ms)
                .range(0.0, 10_000.0)
                .step(50.0)
                .unit("ms")
                .describe("Frames are not judged during a pipeline rebuild or shader compile, a tier switch or a live change, nor for this long after one.")))?,
            fast_pct: reg.register_f32(pol(ParamDef::f32("aq.fast_pct", d.fast_pct)
                .range(0.0, 100.0)
                .step(1.0)
                .unit("%")
                .describe("Ample headroom: work under this share of the target for aq.fast_ms brings back every paused step whose measured cost still fits, one per frame, instead of one step per cooldown (0 = off).")))?,
            fast_ms: reg.register_f32(pol(ParamDef::f32("aq.fast_ms", d.fast_ms)
                .range(0.0, 10_000.0)
                .step(50.0)
                .unit("ms")
                .describe("How long the work must stay under aq.fast_pct before the fast recovery (doubled after each fast recovery that had to be cut again within 10 s).")))?,
        })
    }

    /// Handles of already registered `aq.*` params (`None` if any is missing).
    pub fn lookup(reg: &ParamRegistry) -> Option<Self> {
        let id = |n: &str| reg.id(n);
        Some(Self {
            enabled: BoolParam(id("aq.enabled")?),
            target_ms: F32Param(id("aq.target_ms")?),
            window_ms: F32Param(id("aq.window_ms")?),
            degrade_pct: F32Param(id("aq.degrade_pct")?),
            safe_pct: F32Param(id("aq.safe_pct")?),
            hysteresis_pct: F32Param(id("aq.hysteresis_pct")?),
            degrade_frames: I32Param(id("aq.degrade_frames")?),
            upgrade_frames: I32Param(id("aq.upgrade_frames")?),
            cooldown_ms: F32Param(id("aq.cooldown_ms")?),
            render_scale_step: F32Param(id("aq.render_scale_step")?),
            render_scale_floor: F32Param(id("aq.render_scale_floor")?),
            degrade_order: EnumParam(id("aq.degrade_order")?),
            percentile: F32Param(id("aq.percentile")?),
            grace_ms: F32Param(id("aq.grace_ms")?),
            fast_pct: F32Param(id("aq.fast_pct")?),
            fast_ms: F32Param(id("aq.fast_ms")?),
        })
    }

    /// Current values; `tier_target_ms` replaces a 0 target.
    pub fn policy(&self, reg: &ParamRegistry, tier_target_ms: f32) -> AqPolicy {
        let target = reg.f32(self.target_ms);
        AqPolicy {
            enabled: reg.bool(self.enabled),
            target_ms: if target > 0.0 { target } else { tier_target_ms },
            window_ms: reg.f32(self.window_ms),
            degrade_pct: reg.f32(self.degrade_pct),
            safe_pct: reg.f32(self.safe_pct),
            hysteresis_pct: reg.f32(self.hysteresis_pct),
            degrade_frames: reg.i32(self.degrade_frames).max(1) as u32,
            upgrade_frames: reg.i32(self.upgrade_frames).max(1) as u32,
            cooldown_ms: reg.f32(self.cooldown_ms),
            scale_step: reg.f32(self.render_scale_step),
            scale_floor: reg.f32(self.render_scale_floor),
            scale_first: reg.enum_index(self.degrade_order) == 1,
            percentile: reg.f32(self.percentile),
            grace_ms: reg.f32(self.grace_ms),
            fast_pct: reg.f32(self.fast_pct),
            fast_ms: reg.f32(self.fast_ms),
        }
    }
}

/// A snapshot of the policy params.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AqPolicy {
    pub enabled: bool,
    pub target_ms: f32,
    pub window_ms: f32,
    pub degrade_pct: f32,
    pub safe_pct: f32,
    pub hysteresis_pct: f32,
    pub degrade_frames: u32,
    pub upgrade_frames: u32,
    pub cooldown_ms: f32,
    pub scale_step: f32,
    pub scale_floor: f32,
    pub scale_first: bool,
    /// Percentile of the window's frames that is judged (50 to 99).
    pub percentile: f32,
    /// Frames after a disturbance (pipeline rebuild, tier switch, live
    /// change) that are not judged, in ms.
    pub grace_ms: f32,
    /// Ample headroom, as a share of the target (%): work under it for
    /// `fast_ms` restores every step that fits at once (0 = off).
    pub fast_pct: f32,
    /// How long the work must stay under `fast_pct` first, in ms.
    pub fast_ms: f32,
}

impl Default for AqPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            target_ms: 1000.0 / 60.0,
            window_ms: 500.0,
            degrade_pct: 0.0,
            safe_pct: 70.0,
            hysteresis_pct: 10.0,
            degrade_frames: 20,
            upgrade_frames: 180,
            cooldown_ms: 2000.0,
            scale_step: 0.05,
            scale_floor: MIN_SCALE_FRACTION,
            scale_first: false,
            percentile: 80.0,
            grace_ms: 1000.0,
            fast_pct: 50.0,
            fast_ms: 1000.0,
        }
    }
}

impl AqPolicy {
    /// Smoothed work time above which quality steps down.
    pub fn degrade_line(&self) -> f32 {
        self.target_ms * (1.0 + self.degrade_pct.max(0.0) / 100.0)
    }

    /// Smoothed work time below which quality never steps down, and under
    /// which it may step back up.
    pub fn safe_line(&self) -> f32 {
        self.target_ms * self.safe_pct.clamp(0.0, 100.0) / 100.0
    }

    /// Work time a step back up may lead to at most (step hysteresis).
    pub fn restore_line(&self) -> f32 {
        self.target_ms * (1.0 - self.hysteresis_pct.clamp(0.0, 100.0) / 100.0)
    }

    /// Smoothed work time under which the headroom is ample (fast
    /// recovery).
    pub fn fast_line(&self) -> f32 {
        self.target_ms * self.fast_pct.clamp(0.0, 100.0) / 100.0
    }

    /// Render-scale floor, as a fraction of the tier's scale (never under
    /// [`MIN_SCALE_FRACTION`]).
    pub fn floor(&self) -> f32 {
        self.scale_floor.clamp(MIN_SCALE_FRACTION, 1.0)
    }

    /// Render-scale step (0.05 by default; at least 0.01).
    pub fn step(&self) -> f32 {
        self.scale_step.max(0.01)
    }
}

/// Round a scale to 1/100 so repeated steps stay on the 0.05 grid.
fn snap(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// One frame's measurements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AqSample {
    /// Time since the previous frame. Only weights the average (a long
    /// frame counts for more of the window); it is never judged, because it
    /// is pinned to the display's refresh by vsync.
    pub frame_ms: f32,
    /// CPU time of the frame callback (simulation, systems, render encode).
    pub cpu_ms: f32,
    /// GPU time of the frame's passes (timestamp queries), or the
    /// submit-to-done estimate when timestamps are missing.
    pub gpu_ms: Option<f32>,
    pub now_ms: f64,
    /// prefers-reduced-motion: quality changes are rarer (doubled cooldown),
    /// so the image doesn't keep popping.
    pub reduced_motion: bool,
}

impl AqSample {
    /// The frame's real work: CPU plus GPU time. Conservative (the browser
    /// may overlap them), and independent of vsync.
    pub fn work_ms(&self) -> f32 {
        self.cpu_ms.max(0.0) + self.gpu_ms.unwrap_or(0.0).max(0.0)
    }
}

/// What the controller decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AqAction {
    /// Pause an effect the player has on (shown as "on, paused to hold
    /// frame rate").
    ShedEffect(&'static str),
    RestoreEffect(&'static str),
    /// New render-scale multiplier.
    Scale(f32),
}

/// Degradation the controller applied. World resource; evani-fx pauses
/// the effects in `shed` (state `on, paused to hold frame rate`),
/// evani-render multiplies the tier's render scale by `scale`.
#[derive(Debug, Clone, PartialEq)]
pub struct QualityState {
    /// Paused effects, in the order they were paused.
    pub shed: Vec<&'static str>,
    /// Render-scale multiplier (1.0 = the tier's own scale; never under
    /// [`MIN_SCALE_FRACTION`]).
    pub scale: f32,
    pub last_action: Option<AqAction>,
    /// Work time (CPU + GPU ms, the window's `aq.percentile`) the
    /// controller judges, once measured. Shown in the overlay.
    pub work_ms: Option<f32>,
    /// The work-time target in force (ms).
    pub target_ms: f32,
    /// This frame is stalled on purpose (capture, held clock, bench or A/B
    /// run, settle time after a restart) and is not judged.
    pub held: bool,
    /// Frames are not judged right now: a pipeline rebuild, tier switch or
    /// live change happened less than `aq.grace_ms` ago.
    pub settling: bool,
    /// The tier this run's first-run warm-up picked, once it has. The
    /// overlay's tier line names it while another tier is active (a
    /// `tier.set`, a capture script or the game's menu changed it since),
    /// so the warm-up's log line is not mistaken for the tier in use.
    pub warmup_pick: Option<String>,
    revision: u64,
    disturbances: u64,
}

impl Default for QualityState {
    fn default() -> Self {
        Self {
            shed: Vec::with_capacity(8),
            scale: 1.0,
            last_action: None,
            work_ms: None,
            target_ms: 1000.0 / 60.0,
            held: false,
            settling: false,
            warmup_pick: None,
            revision: 0,
            disturbances: 0,
        }
    }
}

impl QualityState {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Something that stalls or changes frames on purpose happened (a
    /// pipeline rebuild or shader compile, a tier switch, a live change):
    /// adaptive quality ignores the frames from now until `aq.grace_ms`
    /// after the last call. Cheap; call it every frame while it lasts.
    pub fn disturb(&mut self) {
        self.disturbances = self.disturbances.wrapping_add(1);
    }

    /// Bumped by every [`Self::disturb`].
    pub fn disturbances(&self) -> u64 {
        self.disturbances
    }

    pub fn is_degraded(&self) -> bool {
        !self.shed.is_empty() || self.scale < 1.0
    }

    pub fn is_shed(&self, effect: &str) -> bool {
        self.shed.contains(&effect)
    }

    pub fn apply(&mut self, a: AqAction) {
        match a {
            AqAction::ShedEffect(n) => {
                if !self.shed.contains(&n) {
                    self.shed.push(n);
                }
            }
            AqAction::RestoreEffect(n) => self.shed.retain(|s| *s != n),
            AqAction::Scale(s) => self.scale = snap(s).clamp(MIN_SCALE_FRACTION, 1.0),
        }
        self.last_action = Some(a);
        self.revision += 1;
    }

    /// Back to full quality (tier change, controller off).
    pub fn reset(&mut self) {
        if self.is_degraded() || self.last_action.is_some() {
            self.shed.clear();
            self.scale = 1.0;
            self.last_action = None;
            self.revision += 1;
        }
    }
}

/// Most frames the judging window keeps (a 5 s window at 200 fps).
const MAX_WINDOW_FRAMES: usize = 1024;

/// A step back up that its measured cost blocks is tried anyway after this
/// many times `aq.upgrade_frames` of sustained headroom, doubling (up to
/// [`MAX_PROBE_BACKOFF`] times) each time such a try had to be cut again.
const PROBE_FACTOR: u32 = 4;
const MAX_PROBE_BACKOFF: u32 = 4;

/// A fast recovery that has to be cut again within this long was too
/// early: the next one waits twice as long (up to [`MAX_PROBE_BACKOFF`]
/// doublings). One that holds this long resets the wait.
const FAST_HOLD_MS: f64 = 10_000.0;

/// A confirmed step's cost (see [`StepCost::confirmed`]) counts as current
/// while the judged work is at least this share of the work it was
/// measured at; under it the scene changed and the step may be tried again.
const CURRENT_FRACTION: f32 = 0.8;

/// What one degrade step saved, for the step hysteresis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct StepCost {
    /// Judged work just before the step.
    before_ms: f32,
    /// Judged work over the first full window after the step.
    after_ms: Option<f32>,
    /// The step was cut within [`FAST_HOLD_MS`] of a step back up: the
    /// restore did not hold, and this cost was measured under today's
    /// conditions, so it is not tried again while the work stays there.
    confirmed: bool,
}

impl StepCost {
    /// Is this a confirmed cost, measured at about today's `judged` work?
    /// Then trying the step anyway would only repeat the cut (a flap).
    fn is_current(&self, judged: f32) -> bool {
        self.confirmed && self.after_ms.is_some_and(|a| judged >= a * CURRENT_FRACTION)
    }

    /// Work undoing the step is expected to add back when `now_ms` of work
    /// is judged. The saving was measured at `after_ms`; work measured
    /// under heavier load (a GPU-bound spike slows every pass, the step's
    /// own included) overstates what the step costs once that load is
    /// gone, so the saving scales with the work level now, never up.
    fn cost_at(&self, now_ms: f32) -> f32 {
        let Some(after) = self.after_ms else {
            return 0.0;
        };
        let saved = (self.before_ms - after).max(0.0);
        if after > 0.0 {
            saved * (now_ms / after).clamp(0.0, 1.0)
        } else {
            saved
        }
    }
}

/// A step's saving, measured over the first full window after it
/// (`before - after` ms), for the log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepMeasure {
    pub before_ms: f32,
    pub after_ms: f32,
}

/// The adaptive-quality controller. Pure: feed it one [`AqSample`] per
/// measured frame with the effects that are on and their value per point.
///
/// It judges the frame's real work time (CPU + GPU), never the frame
/// interval, which vsync pins to the display: a scene that needs 2 ms of a
/// 16.7 ms budget is never degraded, however the browser paces its frames.
/// The judged value is a percentile (`aq.percentile`, 80 by default) of the
/// frames in the last `window_ms`, not their average, so one slow frame (a
/// hitch, a GC pause) cannot cut quality: more than a fifth of the window
/// must be over, for `degrade_frames` frames in a row and at least one
/// whole window. Frames during a
/// disturbance (pipeline rebuild, tier switch, live change, see
/// [`Self::disturb`]) and `grace_ms` after it are not judged at all.
#[derive(Debug, Clone, Default)]
pub struct AqController {
    /// `(frame ms, work ms)` of the frames in the judging window, oldest
    /// first.
    window: VecDeque<(f32, f32)>,
    window_ms: f32,
    /// Reused buffer for the percentile (no allocation once grown).
    scratch: Vec<f32>,
    judged: Option<f32>,
    over: u32,
    /// Time (ms) the judged work has been over the degrade line.
    over_ms: f32,
    under: u32,
    /// Frames of headroom that a step's measured cost kept from stepping
    /// back up.
    blocked: u32,
    probe_backoff: u32,
    /// The last step back up went ahead against its measured cost.
    probing: bool,
    last_action_ms: Option<f64>,
    /// Frames before this time (ms) are not judged (disturbance grace).
    ignore_until_ms: f64,
    /// One entry per degrade step still in force, oldest first.
    steps: Vec<StepCost>,
    measured: Option<StepMeasure>,
    /// Time (ms) the judged work has been under the fast line.
    fast_under_ms: f32,
    /// Steps a fast recovery still has to undo, one per frame.
    fast_left: u32,
    /// When the last fast recovery finished (for its flap check).
    fast_at_ms: Option<f64>,
    fast_backoff: u32,
    /// When the last step back up (any path) was taken.
    restored_at_ms: Option<f64>,
}

impl AqController {
    /// Forget the measurements (tab shown again, tier switched). Step costs
    /// are kept unless the quality state was reset too ([`Self::clear`]).
    pub fn reset(&mut self) {
        self.window.clear();
        self.window_ms = 0.0;
        self.judged = None;
        self.over = 0;
        self.over_ms = 0.0;
        self.under = 0;
        self.blocked = 0;
        self.last_action_ms = None;
        self.fast_under_ms = 0.0;
        self.fast_left = 0;
    }

    /// Forget everything, step costs included (the quality state went back
    /// to full quality).
    pub fn clear(&mut self) {
        self.reset();
        self.steps.clear();
        self.probe_backoff = 0;
        self.probing = false;
        self.ignore_until_ms = 0.0;
        self.fast_at_ms = None;
        self.fast_backoff = 0;
        self.restored_at_ms = None;
    }

    /// A pipeline rebuild or shader compile, a tier switch or a live change
    /// is under way at `now_ms`: drop the window and judge nothing until
    /// `grace_ms` after the last call. Steps in force and their costs stay.
    pub fn disturb(&mut self, now_ms: f64, grace_ms: f32) {
        self.window.clear();
        self.window_ms = 0.0;
        self.over = 0;
        self.over_ms = 0.0;
        self.under = 0;
        self.blocked = 0;
        self.fast_under_ms = 0.0;
        self.ignore_until_ms = self
            .ignore_until_ms
            .max(now_ms + f64::from(grace_ms.max(0.0)));
    }

    /// Is a disturbance (or its grace time) keeping frames from being judged?
    pub fn settling(&self, now_ms: f64) -> bool {
        now_ms < self.ignore_until_ms
    }

    /// The judged work time (CPU + GPU ms, the window's `aq.percentile`),
    /// once a full window is measured.
    pub fn work_ms(&self) -> Option<f32> {
        self.judged
    }

    /// The saving of the latest step, once measured (taken once, for the
    /// log).
    pub fn take_measured(&mut self) -> Option<StepMeasure> {
        self.measured.take()
    }

    /// Add one frame to the window; the judged percentile once the window
    /// is full.
    fn push(&mut self, p: &AqPolicy, dt: f32, work: f32) -> Option<f32> {
        let span = p.window_ms.max(1.0);
        self.window.push_back((dt, work));
        self.window_ms += dt;
        while let Some(&(front, _)) = self.window.front() {
            if self.window.len() > MAX_WINDOW_FRAMES
                || (self.window.len() > 1 && self.window_ms - front >= span)
            {
                self.window.pop_front();
                self.window_ms -= front;
            } else {
                break;
            }
        }
        if self.window_ms < span && self.window.len() < MAX_WINDOW_FRAMES {
            return None;
        }
        self.scratch.clear();
        self.scratch.extend(self.window.iter().map(|w| w.1));
        let n = self.scratch.len();
        let rank = ((p.percentile.clamp(1.0, 100.0) / 100.0) * n as f32).ceil() as usize;
        let k = rank.clamp(1, n) - 1;
        let (_, v, _) = self.scratch.select_nth_unstable_by(k, f32::total_cmp);
        Some(*v)
    }

    /// One frame. `on_effects` are the effects currently on with their
    /// value per point. Returns at most one action; the caller applies it
    /// to the [`QualityState`].
    pub fn update(
        &mut self,
        p: &AqPolicy,
        s: &AqSample,
        state: &QualityState,
        on_effects: &[(&'static str, f32)],
    ) -> Option<AqAction> {
        if !p.enabled {
            self.fast_left = 0;
            return None;
        }
        // A fast recovery already decided goes on one step per frame, even
        // through a disturbance (the decision is made; frames are only not
        // judged).
        if self.fast_left > 0 {
            if let Some(a) = Self::upgrade(p, state) {
                self.fast_left -= 1;
                self.steps.pop();
                self.after_action(s.now_ms);
                self.restored_at_ms = Some(s.now_ms);
                if self.fast_left == 0 {
                    self.fast_at_ms = Some(s.now_ms);
                }
                return Some(a);
            }
            self.fast_left = 0;
            self.fast_at_ms = Some(s.now_ms);
        }
        if s.now_ms < self.ignore_until_ms {
            return None;
        }
        let work = s.work_ms();
        if !work.is_finite() {
            return None;
        }
        let dt = if s.frame_ms.is_finite() && s.frame_ms > 0.0 {
            s.frame_ms
        } else {
            p.target_ms
        };
        // Only judge once a full window is measured.
        let judged = self.push(p, dt, work)?;
        self.judged = Some(judged);

        // Step hysteresis: the first full window after a step measures what
        // it saved.
        if let Some(last) = self.steps.last_mut()
            && last.after_ms.is_none()
        {
            last.after_ms = Some(judged);
            self.measured = Some(StepMeasure {
                before_ms: last.before_ms,
                after_ms: judged,
            });
        }

        // A fast recovery that held for FAST_HOLD_MS was right: the next
        // one waits the normal time again.
        if self.fast_at_ms.is_some_and(|t| s.now_ms - t >= FAST_HOLD_MS) {
            self.fast_at_ms = None;
            self.fast_backoff = 0;
        }
        if judged < p.fast_line() {
            self.fast_under_ms += dt;
        } else {
            self.fast_under_ms = 0.0;
        }

        let degrade_line = p.degrade_line().max(p.safe_line());
        let mut probe = false;
        if judged > degrade_line {
            self.over += 1;
            self.over_ms += dt;
            self.under = 0;
            self.blocked = 0;
        } else {
            self.over = 0;
            self.over_ms = 0.0;
            if judged < p.safe_line() {
                let last = self.steps.last();
                let cost = last.map_or(0.0, |c| c.cost_at(judged));
                if judged + cost <= p.restore_line() {
                    self.under += 1;
                    self.blocked = 0;
                } else if last.is_some_and(|c| c.is_current(judged)) {
                    // A restore of this step already failed at about this
                    // work: its cost is known. It comes back when the
                    // scene gets lighter, never as a try (no flapping).
                    self.under = 0;
                    self.blocked = 0;
                } else {
                    // Headroom, but the step's measured cost says it would
                    // not fit back. Measurements can be wrong (a hitch in
                    // the window): after long enough, try it anyway.
                    self.blocked += 1;
                    let wait = p
                        .upgrade_frames
                        .saturating_mul(PROBE_FACTOR << self.probe_backoff.min(MAX_PROBE_BACKOFF));
                    if self.blocked >= wait {
                        probe = true;
                        self.under = p.upgrade_frames;
                    } else {
                        self.under = 0;
                    }
                }
            } else {
                self.under = 0;
                self.blocked = 0;
            }
        }

        let cooldown = f64::from(p.cooldown_ms) * if s.reduced_motion { 2.0 } else { 1.0 };
        if self.last_action_ms.is_some_and(|t| s.now_ms - t < cooldown) {
            return None;
        }
        // Over for `degrade_frames` frames and a whole window: a burst of
        // slow frames shorter than the window never cuts quality.
        let fast_wait = p.fast_ms.max(0.0) * (1u32 << self.fast_backoff.min(MAX_PROBE_BACKOFF)) as f32;
        let action = if self.over >= p.degrade_frames && self.over_ms >= p.window_ms {
            let a = Self::degrade(p, state, on_effects);
            if a.is_some() {
                if self.fast_at_ms.take().is_some_and(|t| s.now_ms - t < FAST_HOLD_MS) {
                    // The fast recovery came too early: wait longer next time.
                    self.fast_backoff = (self.fast_backoff + 1).min(MAX_PROBE_BACKOFF);
                }
                if self.probing {
                    // The tried step did not fit after all: wait longer
                    // before the next try.
                    self.probe_backoff = (self.probe_backoff + 1).min(MAX_PROBE_BACKOFF);
                }
                self.probing = false;
                // Cut again soon after a step back up: that restore did not
                // hold, and this step's cost is measured as things are now.
                let confirmed = self
                    .restored_at_ms
                    .take()
                    .is_some_and(|t| s.now_ms - t < FAST_HOLD_MS);
                self.steps.push(StepCost {
                    before_ms: judged,
                    after_ms: None,
                    confirmed,
                });
            }
            a
        } else if let Some(fit) = (state.is_degraded() && p.fast_pct > 0.0 && self.fast_under_ms >= fast_wait)
            .then(|| self.fast_fit(p, judged))
            .filter(|&n| n > 0)
        {
            // Ample headroom for long enough: undo every step whose
            // measured cost (scaled to the work now) still fits under the
            // restore line, newest first, one per frame from this one on.
            // A step that does not fit is left to the slow path below
            // (its hysteresis and probe waits).
            let a = Self::upgrade(p, state);
            if a.is_some() {
                self.steps.pop();
                self.probing = false;
                self.restored_at_ms = Some(s.now_ms);
                self.fast_left = fit - 1;
                if self.fast_left == 0 {
                    self.fast_at_ms = Some(s.now_ms);
                }
            }
            a
        } else if self.under >= p.upgrade_frames {
            let a = Self::upgrade(p, state);
            if a.is_some() {
                self.steps.pop();
                self.probing = probe;
                self.restored_at_ms = Some(s.now_ms);
            }
            a
        } else {
            None
        };
        if action.is_some() {
            self.after_action(s.now_ms);
        }
        action
    }

    /// Measure the new configuration from scratch after a step.
    fn after_action(&mut self, now_ms: f64) {
        self.window.clear();
        self.window_ms = 0.0;
        self.over = 0;
        self.over_ms = 0.0;
        self.under = 0;
        self.blocked = 0;
        self.fast_under_ms = 0.0;
        self.last_action_ms = Some(now_ms);
    }

    /// How many upgrade steps a fast recovery may take at `judged` ms of
    /// work: tracked steps newest first while their summed cost still fits
    /// under the restore line; if every tracked step fits, also any
    /// untracked degradation (unknown cost) up to full quality.
    fn fast_fit(&self, p: &AqPolicy, judged: f32) -> u32 {
        let mut work = judged;
        for (n, c) in self.steps.iter().rev().enumerate() {
            work += c.cost_at(judged);
            if work > p.restore_line() {
                return n as u32;
            }
        }
        u32::MAX
    }

    /// The next step down: the on effect with the lowest value per point
    /// (ties: the later one; stable, no hashing), then render scale in
    /// `scale_step` steps down to the floor. `scale_first` swaps the two.
    pub fn degrade(p: &AqPolicy, state: &QualityState, on: &[(&'static str, f32)]) -> Option<AqAction> {
        let shed = on
            .iter()
            .filter(|(n, v)| v.is_finite() && !state.is_shed(n))
            .fold(None::<(&'static str, f32)>, |best, &(n, v)| match best {
                Some((_, bv)) if bv < v => best,
                _ => Some((n, v)),
            })
            .map(|(n, _)| AqAction::ShedEffect(n));
        let floor = p.floor();
        let scale = (state.scale > floor + 1e-4)
            .then(|| AqAction::Scale(snap((state.scale - p.step()).max(floor))));
        if p.scale_first {
            scale.or(shed)
        } else {
            shed.or(scale)
        }
    }

    /// The next step up: the reverse of [`Self::degrade`] (undo the most
    /// recent kind of step first).
    pub fn upgrade(p: &AqPolicy, state: &QualityState) -> Option<AqAction> {
        let scale = (state.scale < 1.0 - 1e-4)
            .then(|| AqAction::Scale(snap((state.scale + p.step()).min(1.0))));
        let restore = state.shed.last().map(|n| AqAction::RestoreEffect(n));
        if p.scale_first {
            restore.or(scale)
        } else {
            scale.or(restore)
        }
    }
}

/// What the warm-up wants this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmupStep {
    /// Keep measuring.
    Continue,
    /// Switch to this tier index and measure it.
    Try(usize),
    /// Finished: use this tier index.
    Done(usize),
}

/// First-run tier selection: try the auto-select tiers from the highest
/// down, measure each for an equal share of the warm-up time (after a few
/// settle frames, while targets and pipelines rebuild), and pick the first
/// that holds its frame-time target. Tiers whose target resolution exceeds
/// the adapter's largest texture are skipped. Pure; tested natively.
#[derive(Debug, Clone)]
pub struct TierWarmup {
    /// Tier indices to try, highest first.
    candidates: Vec<usize>,
    pos: usize,
    per_tier_ms: f32,
    settle_frames: u32,
    tolerance_pct: f32,
    frames: u32,
    measured_ms: f32,
    sum_frame_ms: f32,
    samples: u32,
    /// `(tier index, mean judged ms)` of every tier measured (work time
    /// when fed through [`TierWarmup::update_work`]).
    pub results: Vec<(usize, f32)>,
}

impl TierWarmup {
    /// `None` when no tier qualifies (then keep the default tier).
    pub fn new(
        tiers: &Tiers,
        max_texture_dim: u32,
        total_ms: f32,
        settle_frames: u32,
        tolerance_pct: f32,
    ) -> Option<Self> {
        let mut candidates: Vec<usize> = tiers
            .presets()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.auto_select && !p.stereo)
            .filter(|(_, p)| {
                p.target_resolution
                    .is_none_or(|[w, h]| w.max(h) <= max_texture_dim)
            })
            .map(|(i, _)| i)
            .collect();
        candidates.reverse();
        if candidates.is_empty() {
            return None;
        }
        let per_tier_ms = total_ms / candidates.len() as f32;
        Some(Self {
            candidates,
            pos: 0,
            per_tier_ms,
            settle_frames,
            tolerance_pct,
            frames: 0,
            measured_ms: 0.0,
            sum_frame_ms: 0.0,
            samples: 0,
            results: Vec::new(),
        })
    }

    /// The tier to start measuring.
    pub fn first(&self) -> usize {
        self.candidates[0]
    }

    /// The tier being measured now.
    pub fn current(&self) -> usize {
        self.candidates[self.pos]
    }

    /// Feed one frame's time (ms) measured on [`TierWarmup::current`].
    pub fn update(&mut self, tiers: &Tiers, frame_ms: f32) -> WarmupStep {
        self.update_work(tiers, frame_ms, frame_ms)
    }

    /// Feed one frame: `frame_ms` (the frame interval) paces the
    /// measurement, `work_ms` (CPU + GPU time, see [`AqSample::work_ms`]) is
    /// what is judged against the tier's target. With vsync the interval
    /// reads the display refresh (16.7 ms at 60 Hz) however light the frame
    /// is, and stretches when the browser is busy, so it says nothing about
    /// what the tier costs.
    pub fn update_work(&mut self, tiers: &Tiers, frame_ms: f32, work_ms: f32) -> WarmupStep {
        self.frames += 1;
        if self.frames <= self.settle_frames {
            return WarmupStep::Continue;
        }
        self.measured_ms += frame_ms;
        self.sum_frame_ms += work_ms;
        self.samples += 1;
        if self.measured_ms < self.per_tier_ms {
            return WarmupStep::Continue;
        }
        let idx = self.current();
        let mean = self.sum_frame_ms / self.samples.max(1) as f32;
        self.results.push((idx, mean));
        let budget = tiers.presets()[idx].frame_budget_ms() * (1.0 + self.tolerance_pct / 100.0);
        if mean <= budget {
            return WarmupStep::Done(idx);
        }
        if self.pos + 1 >= self.candidates.len() {
            // Nothing held its target: the lowest tier it is.
            return WarmupStep::Done(idx);
        }
        self.pos += 1;
        self.frames = 0;
        self.measured_ms = 0.0;
        self.sum_frame_ms = 0.0;
        self.samples = 0;
        WarmupStep::Try(self.current())
    }
}

/// [`crate::save::SaveData`] key of the tier the warm-up picked.
pub const SAVED_TIER_KEY: &str = "render.tier";

#[cfg(test)]
mod tests {
    use super::*;

    /// A vsync-paced frame (the interval) with `work` ms of CPU work and no
    /// GPU timing.
    fn sample(frame_ms: f32, work: f32, now_ms: f64) -> AqSample {
        AqSample {
            frame_ms,
            cpu_ms: work,
            gpu_ms: None,
            now_ms,
            reduced_motion: false,
        }
    }

    fn policy() -> AqPolicy {
        AqPolicy {
            degrade_frames: 3,
            upgrade_frames: 5,
            cooldown_ms: 100.0,
            window_ms: 50.0,
            ..AqPolicy::default()
        }
    }

    /// Run frames of `frame_ms` interval and `work` ms work until an action
    /// or `max` frames.
    #[allow(clippy::too_many_arguments)]
    fn run(
        c: &mut AqController,
        p: &AqPolicy,
        st: &mut QualityState,
        on: &[(&'static str, f32)],
        frame_ms: f32,
        work: f32,
        now: &mut f64,
        max: u32,
    ) -> Option<AqAction> {
        for _ in 0..max {
            *now += f64::from(frame_ms);
            if let Some(a) = c.update(p, &sample(frame_ms, work, *now), st, on) {
                st.apply(a);
                return Some(a);
            }
        }
        None
    }

    /// Note 1: the frame interval is never judged. Frames pinned at 33 ms
    /// (a 30 Hz display, a throttled tab) with 2 ms of work: nothing is
    /// cut; frames at 16.7 ms with 11 ms of work (66 % of the target, under
    /// the 70 % safe line): nothing is cut either.
    #[test]
    fn slow_frame_interval_with_light_work_never_degrades() {
        let p = policy();
        let mut st = QualityState::default();
        let on = [("bloom", 0.27), ("vignette", 0.3)];
        let mut now = 0.0;
        let mut c = AqController::default();
        assert_eq!(run(&mut c, &p, &mut st, &on, 33.3, 2.0, &mut now, 2000), None);
        assert_eq!(run(&mut c, &p, &mut st, &on, 16.7, 11.0, &mut now, 2000), None);
        assert!(c.work_ms().is_some_and(|w| (w - 11.0).abs() < 0.5));
        // Up to the target itself (degrade_pct 0): still nothing.
        assert_eq!(run(&mut c, &p, &mut st, &on, 16.7, 16.5, &mut now, 2000), None);
        assert!(!st.is_degraded());
        // GPU time counts: 3 ms CPU + 15 ms GPU = 18 ms of work, over.
        let mut c = AqController::default();
        let mut hit = None;
        for _ in 0..200 {
            now += 16.7;
            let s = AqSample {
                gpu_ms: Some(15.0),
                ..sample(16.7, 3.0, now)
            };
            if let Some(a) = c.update(&p, &s, &st, &on) {
                hit = Some(a);
                break;
            }
        }
        assert_eq!(hit, Some(AqAction::ShedEffect("bloom")));
    }

    /// Note 3, rule 1: effects go first, lowest value per point first, and
    /// only then render scale.
    #[test]
    fn effects_go_first_by_lowest_value_per_point() {
        let p = policy();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let mut on: Vec<(&'static str, f32)> =
            vec![("vignette", 0.30), ("bloom", 0.27), ("outline", 0.55)];
        let mut order = Vec::new();
        while let Some(a) = run(&mut c, &p, &mut st, &on, 16.7, 30.0, &mut now, 200) {
            order.push(a);
            if let AqAction::ShedEffect(n) = a {
                on.retain(|(m, _)| *m != n);
            }
        }
        assert_eq!(
            &order[..3],
            [
                AqAction::ShedEffect("bloom"),
                AqAction::ShedEffect("vignette"),
                AqAction::ShedEffect("outline")
            ]
        );
        assert!(order[3..].iter().all(|a| matches!(a, AqAction::Scale(_))));
        // The order follows the weights, not the list order.
        let st = QualityState::default();
        let on = [("a", 2.0), ("b", 0.5), ("c", 1.0)];
        assert_eq!(
            AqController::degrade(&p, &st, &on),
            Some(AqAction::ShedEffect("b"))
        );
    }

    /// Note 3, rule 2: render scale steps by 0.05 and never goes below
    /// 80 % of the tier's scale, even if the policy asks for less.
    #[test]
    fn scale_steps_by_005_down_to_80_percent_and_back() {
        let p = policy();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let mut scales = Vec::new();
        while let Some(AqAction::Scale(s)) = run(&mut c, &p, &mut st, &[], 16.7, 40.0, &mut now, 200)
        {
            scales.push((s * 100.0).round() as i32);
        }
        assert_eq!(scales, [95, 90, 85, 80]);
        assert!((st.scale - 0.8).abs() < 1e-6);
        // No floor below 0.8, whatever the policy says.
        let low = AqPolicy {
            scale_floor: 0.5,
            scale_step: 0.3,
            ..p
        };
        assert_eq!(low.floor(), MIN_SCALE_FRACTION);
        let mut st2 = QualityState::default();
        st2.apply(AqAction::Scale(0.85));
        assert_eq!(AqController::degrade(&low, &st2, &[]), Some(AqAction::Scale(0.8)));
        st2.apply(AqAction::Scale(0.3));
        assert_eq!(st2.scale, MIN_SCALE_FRACTION);
        assert_eq!(AqController::degrade(&low, &st2, &[]), None);
        // Headroom: back up in the same steps.
        let mut ups = Vec::new();
        while let Some(AqAction::Scale(s)) = run(&mut c, &p, &mut st, &[], 16.7, 2.0, &mut now, 200) {
            ups.push((s * 100.0).round() as i32);
        }
        assert_eq!(ups, [85, 90, 95, 100]);
        assert!(!st.is_degraded());
    }

    #[test]
    fn degrades_then_recovers_in_reverse() {
        let p = policy();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let on_all = [("bloom", 0.27), ("vignette", 0.3)];
        let a = run(&mut c, &p, &mut st, &on_all, 33.3, 30.0, &mut now, 50);
        assert_eq!(a, Some(AqAction::ShedEffect("bloom")));
        let on = [("vignette", 0.3)];
        assert_eq!(
            run(&mut c, &p, &mut st, &on, 33.3, 30.0, &mut now, 50),
            Some(AqAction::ShedEffect("vignette"))
        );
        while let Some(AqAction::Scale(_)) = run(&mut c, &p, &mut st, &[], 33.3, 30.0, &mut now, 50) {}
        assert_eq!(st.shed, ["bloom", "vignette"]);
        // Plenty of headroom: scale back up first, then vignette, then bloom.
        let mut ups = Vec::new();
        while let Some(a) = run(&mut c, &p, &mut st, &[], 16.7, 2.0, &mut now, 200) {
            ups.push(a);
        }
        assert_eq!(ups.len(), 6);
        assert_eq!(ups[3], AqAction::Scale(1.0));
        assert_eq!(ups[4], AqAction::RestoreEffect("vignette"));
        assert_eq!(ups[5], AqAction::RestoreEffect("bloom"));
        assert!(!st.is_degraded());
    }

    /// Step hysteresis: a step that saved 8 ms is not undone while adding
    /// those 8 ms back would leave less than 10 % of the target free, even
    /// though the work is under the safe line.
    #[test]
    fn step_hysteresis_keeps_a_step_that_would_not_fit_back() {
        let p = policy();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        // 18 ms of work: over the 16.7 ms target, bloom goes.
        assert_eq!(
            run(&mut c, &p, &mut st, &[("bloom", 0.27)], 16.7, 18.0, &mut now, 100),
            Some(AqAction::ShedEffect("bloom"))
        );
        // Without bloom 10 ms (saved 8): 10 + 8 > 15 (90 % of 16.7), so
        // bloom stays paused although 10 ms is under the 11.7 ms safe line.
        let wait = p.upgrade_frames * PROBE_FACTOR;
        assert_eq!(run(&mut c, &p, &mut st, &[], 16.7, 10.0, &mut now, wait), None);
        assert!(st.is_shed("bloom"));
        // The scene gets lighter (5 ms): 5 + 8 fits, bloom comes back.
        assert_eq!(
            run(&mut c, &p, &mut st, &[], 16.7, 5.0, &mut now, 1000),
            Some(AqAction::RestoreEffect("bloom"))
        );
        // Between the safe line and the target nothing moves either way.
        let mut st = QualityState::default();
        st.apply(AqAction::ShedEffect("vignette"));
        let mut c = AqController::default();
        assert_eq!(run(&mut c, &p, &mut st, &[], 16.7, 14.0, &mut now, 1000), None);
    }

    /// A step its measured cost keeps paused is tried anyway after
    /// PROBE_FACTOR times `upgrade_frames` of headroom (the measurement may
    /// have been wrong). If it really does not fit and has to go again, its
    /// cost is confirmed: at the same work it is never tried again (no
    /// flapping). Once the scene changes (work under 80 % of what the cost
    /// was measured at) it may be tried again, after twice the wait.
    #[test]
    fn a_blocked_step_is_tried_with_growing_waits() {
        let p = policy();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let bloom = [("bloom", 0.27)];
        assert_eq!(
            run(&mut c, &p, &mut st, &bloom, 16.7, 22.0, &mut now, 100),
            Some(AqAction::ShedEffect("bloom"))
        );
        let wait = p.upgrade_frames * PROBE_FACTOR;
        assert_eq!(run(&mut c, &p, &mut st, &[], 16.7, 10.0, &mut now, wait), None);
        assert_eq!(
            run(&mut c, &p, &mut st, &[], 16.7, 10.0, &mut now, 100),
            Some(AqAction::RestoreEffect("bloom"))
        );
        // Bloom really costs 12 ms: it goes again, and its cost is now
        // confirmed at 10 ms of work.
        assert_eq!(
            run(&mut c, &p, &mut st, &bloom, 16.7, 22.0, &mut now, 100),
            Some(AqAction::ShedEffect("bloom"))
        );
        assert_eq!(run(&mut c, &p, &mut st, &[], 16.7, 10.0, &mut now, 20 * wait), None);
        assert!(st.is_shed("bloom"));
        // The scene changes: 7.5 ms (under 80 % of 10). 7.5 + 12 * 0.75 is
        // still over the restore line, so it is a try, after twice the wait.
        assert_eq!(run(&mut c, &p, &mut st, &[], 16.7, 7.5, &mut now, 2 * wait), None);
        assert_eq!(
            run(&mut c, &p, &mut st, &[], 16.7, 7.5, &mut now, 100),
            Some(AqAction::RestoreEffect("bloom"))
        );
    }

    #[test]
    fn cooldown_spaces_steps_and_reduced_motion_doubles_it() {
        let p = policy();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let a = run(&mut c, &p, &mut st, &[("v", 1.0)], 40.0, 40.0, &mut now, 10);
        assert_eq!(a, Some(AqAction::ShedEffect("v")));
        let t0 = now;
        let a = run(&mut c, &p, &mut st, &[], 40.0, 40.0, &mut now, 100);
        assert!(matches!(a, Some(AqAction::Scale(_))));
        assert!(now - t0 >= 100.0);
        let mut c2 = AqController::default();
        let mut st2 = QualityState::default();
        let mut t = 0.0;
        let mut first = None;
        let mut second = None;
        for _ in 0..100 {
            t += 40.0;
            let s = AqSample {
                reduced_motion: true,
                ..sample(40.0, 40.0, t)
            };
            if let Some(a) = c2.update(&p, &s, &st2, &[]) {
                st2.apply(a);
                if first.is_none() {
                    first = Some(t);
                } else if second.is_none() {
                    second = Some(t);
                }
            }
        }
        assert!(second.unwrap() - first.unwrap() >= 200.0);
    }

    #[test]
    fn scale_first_order_and_disabled() {
        let p = AqPolicy {
            scale_first: true,
            ..policy()
        };
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let on = [("bloom", 0.27)];
        assert!(matches!(
            run(&mut c, &p, &mut st, &on, 40.0, 40.0, &mut now, 50),
            Some(AqAction::Scale(_))
        ));
        let off = AqPolicy {
            enabled: false,
            ..p
        };
        assert_eq!(
            run(&mut c, &off, &mut st, &on, 40.0, 40.0, &mut now, 100),
            None
        );
        st.reset();
        assert!(!st.is_degraded());
    }

    /// A frame of the sandbox at High in a simple model: 2 ms of scene,
    /// bloom 1 ms and vignette 0.3 ms while not paused, plus `load` ms of
    /// GPU-bound work that also slows bloom (they share the GPU), all
    /// times render scale squared. Frames are vsync-paced at 60 Hz.
    fn high_frame(st: &QualityState, load: f32, now: f64) -> AqSample {
        let k = st.scale * st.scale;
        let bloom = if st.is_shed("bloom") { 0.0 } else { 1.0 + load * 0.5 };
        let vignette = if st.is_shed("vignette") { 0.0 } else { 0.3 };
        let work = (2.0 + load + bloom + vignette) * k;
        AqSample {
            gpu_ms: Some(work - 0.5),
            ..sample(work.max(1000.0 / 60.0), 0.5, now)
        }
    }

    /// Chris's review, follow-up 1 (and Gate 0 question 1): after a load
    /// spike ends, every paused effect and the full render scale come back,
    /// in the reverse of the cut order, within RECOVERY_S seconds (default
    /// policy, 60 Hz). The work is far under half the target, so the fast
    /// recovery restores all six steps on consecutive frames once the
    /// window's 80th percentile has been under `aq.fast_pct` for
    /// `aq.fast_ms` (the old one-step-per-cooldown recovery took 22 s).
    /// Bloom's saving was
    /// measured under the load (13.5 ms, the GPU was shared with it); the
    /// old controller added that back in full and kept bloom paused for
    /// good at under 1 ms of work.
    #[test]
    fn everything_comes_back_after_a_load_spike() {
        const RECOVERY_S: f64 = 5.0;
        let p = AqPolicy::default();
        let on = [("bloom", 0.27), ("vignette", 0.3)];
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let mut cuts = Vec::new();
        // 15 s of a 25 ms load.
        while now < 15_000.0 {
            let s = high_frame(&st, 25.0, now);
            now += f64::from(s.frame_ms);
            if let Some(a) = c.update(&p, &AqSample { now_ms: now, ..s }, &st, &on) {
                st.apply(a);
                cuts.push(a);
            }
        }
        assert_eq!(
            cuts,
            [
                AqAction::ShedEffect("bloom"),
                AqAction::ShedEffect("vignette"),
                AqAction::Scale(0.95),
                AqAction::Scale(0.9),
                AqAction::Scale(0.85),
                AqAction::Scale(0.8),
            ]
        );
        // The load ends.
        let end = now;
        let mut ups = Vec::new();
        while st.is_degraded() && now - end < 60_000.0 {
            let s = high_frame(&st, 0.0, now);
            now += f64::from(s.frame_ms);
            if let Some(a) = c.update(&p, &AqSample { now_ms: now, ..s }, &st, &on) {
                st.apply(a);
                ups.push(a);
            }
        }
        let took_s = (now - end) / 1000.0;
        assert!(!st.is_degraded(), "still degraded after {took_s:.1} s: {st:?}");
        println!("full quality back {took_s:.1} s after the load ended");
        assert!(took_s <= RECOVERY_S, "took {took_s:.1} s");
        let mut reverse = cuts.clone();
        reverse.reverse();
        let expect: Vec<AqAction> = reverse
            .iter()
            .zip([0.85, 0.9, 0.95, 1.0, 0.0, 0.0])
            .map(|(a, up)| match a {
                AqAction::ShedEffect(n) => AqAction::RestoreEffect(n),
                _ => AqAction::Scale(up),
            })
            .collect();
        assert_eq!(ups, expect, "reverse of the cut order");
        // And it stays there: no flapping.
        let mut late = None;
        while now - end < 90_000.0 {
            let s = high_frame(&st, 0.0, now);
            now += f64::from(s.frame_ms);
            late = late.or(c.update(&p, &AqSample { now_ms: now, ..s }, &st, &on));
        }
        assert_eq!(late, None);
    }

    /// Gate 0 question 1: with work under 50 % of the target everything is
    /// back within 5 s of the load ending, whatever the frame rate (30 and
    /// 60 Hz displays, effects and scale both cut); near the limit (work
    /// between the fast line and the safe line) the slow one-step-at-a-time
    /// recovery still applies; a fast recovery that has to be cut again
    /// within 10 s doubles the wait before the next one (no flapping).
    #[test]
    fn fast_recovery_within_5_s_when_work_is_under_half_the_target() {
        for hz in [30.0f32, 60.0, 144.0] {
            let p = AqPolicy::default();
            let on = [("bloom", 0.27), ("vignette", 0.3), ("outline", 0.5)];
            let mut c = AqController::default();
            let mut st = QualityState::default();
            let mut now = 0.0;
            let dt = 1000.0 / hz;
            let mut cuts = 0;
            while now < 20_000.0 {
                now += f64::from(dt);
                if let Some(a) = c.update(&p, &sample(dt, 30.0, now), &st, &on) {
                    st.apply(a);
                    cuts += 1;
                }
            }
            assert!(cuts >= 5 && st.scale < 1.0 && st.shed.len() == 3, "{hz} Hz: {st:?}");
            // The load ends: 4 ms of work (24 % of the 16.7 ms target).
            let end = now;
            while st.is_degraded() && now - end < 30_000.0 {
                now += f64::from(dt);
                if let Some(a) = c.update(&p, &sample(dt, 4.0, now), &st, &on) {
                    st.apply(a);
                }
            }
            let took = now - end;
            assert!(!st.is_degraded() && took <= 5000.0, "{hz} Hz: {took:.0} ms, {st:?}");
            println!("{hz} Hz: full quality back {:.1} s after the load ended", took / 1000.0);
        }

        // Near the limit: 10 ms of work (60 %, between the 50 % fast line
        // and the 70 % safe line) only steps back one cooldown at a time.
        let p = AqPolicy::default();
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        while st.scale > 0.85 {
            now += 16.7;
            if let Some(a) = c.update(&p, &sample(16.7, 30.0, now), &st, &[]) {
                st.apply(a);
            }
        }
        let mut ups = Vec::new();
        let t0 = now;
        while st.is_degraded() && now - t0 < 60_000.0 {
            now += 16.7;
            if let Some(a) = c.update(&p, &sample(16.7, 10.0, now), &st, &[]) {
                st.apply(a);
                ups.push(now);
            }
        }
        assert_eq!(ups.len(), 3);
        assert!(ups.windows(2).all(|w| w[1] - w[0] >= f64::from(p.cooldown_ms)), "{ups:?}");

        // A fast recovery that does not hold: the load comes back right
        // after it. The next fast recovery waits twice as long.
        let p = policy();
        // (A long upgrade_frames keeps the slow path out of it.)
        let fast = AqPolicy { fast_ms: 200.0, upgrade_frames: 600, ..p };
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let bloom = [("bloom", 0.27)];
        assert_eq!(
            run(&mut c, &fast, &mut st, &bloom, 16.7, 18.0, &mut now, 100),
            Some(AqAction::ShedEffect("bloom"))
        );
        // Bloom saved little (the load was elsewhere): 16 ms without it.
        assert_eq!(run(&mut c, &fast, &mut st, &[], 16.7, 16.0, &mut now, 20), None);
        // The load ends: 2 ms.
        let t0 = now;
        assert_eq!(
            run(&mut c, &fast, &mut st, &[], 16.7, 2.0, &mut now, 1000),
            Some(AqAction::RestoreEffect("bloom"))
        );
        let first_wait = now - t0;
        assert_eq!(
            run(&mut c, &fast, &mut st, &bloom, 16.7, 18.0, &mut now, 100),
            Some(AqAction::ShedEffect("bloom"))
        );
        assert_eq!(run(&mut c, &fast, &mut st, &[], 16.7, 16.0, &mut now, 20), None);
        let t1 = now;
        assert_eq!(
            run(&mut c, &fast, &mut st, &[], 16.7, 2.0, &mut now, 1000),
            Some(AqAction::RestoreEffect("bloom"))
        );
        let second_wait = now - t1;
        assert!(
            second_wait >= first_wait + f64::from(fast.fast_ms) - 20.0,
            "{first_wait:.0} then {second_wait:.0} ms"
        );
    }

    /// Work of a modelled frame: `base` ms of scene, bloom `bloom` ms,
    /// vignette 1 ms and outline 2 ms while not paused, all times render
    /// scale squared, plus `load` ms of other work.
    fn scene_work(st: &QualityState, base: f32, bloom: f32, load: f32) -> f32 {
        let fx = |n: &str, ms: f32| if st.is_shed(n) { 0.0 } else { ms };
        (base + fx("bloom", bloom) + fx("vignette", 1.0) + fx("outline", 2.0)) * st.scale * st.scale
            + load
    }

    /// Drive the default policy at `hz` with a 30 ms load for 20 s (every
    /// effect paused, scale at the floor), then without it for `after_s`
    /// seconds. Returns the actions after the load ended with their time
    /// since the end (ms).
    fn load_then_steady(hz: f32, base: f32, bloom: f32, after_s: f64) -> (QualityState, Vec<(f64, AqAction)>) {
        let p = AqPolicy::default();
        let on_all = [("bloom", 0.27), ("vignette", 0.3), ("outline", 0.5)];
        let on_now = |st: &QualityState| -> Vec<(&'static str, f32)> {
            on_all.iter().copied().filter(|(n, _)| !st.is_shed(n)).collect()
        };
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let dt = 1000.0 / hz;
        let mut now = 0.0;
        while now < 20_000.0 {
            now += f64::from(dt);
            let w = scene_work(&st, base, bloom, 30.0);
            if let Some(a) = c.update(&p, &sample(dt.max(w), w, now), &st, &on_now(&st)) {
                st.apply(a);
            }
        }
        assert!(st.shed.len() == 3 && st.scale <= 0.8 + 1e-4, "{hz} Hz: load cut all: {st:?}");
        let end = now;
        let mut acts = Vec::new();
        while now - end < after_s * 1000.0 {
            now += f64::from(dt);
            let w = scene_work(&st, base, bloom, 0.0);
            if let Some(a) = c.update(&p, &sample(dt.max(w), w, now), &st, &on_now(&st)) {
                st.apply(a);
                acts.push((now - end, a));
            }
        }
        (st, acts)
    }

    /// Gate 0 follow-up: a fast recovery restores several steps at once
    /// (three effects and four scale steps on consecutive frames), and
    /// then a steady moderate load (60 to 75 % of the target with
    /// everything on) never cuts or restores again: 40 s watched at 30, 60
    /// and 144 Hz.
    #[test]
    fn multi_step_fast_restore_then_steady_moderate_load_never_flaps() {
        let target = AqPolicy::default().target_ms;
        for hz in [30.0f32, 60.0, 144.0] {
            // base + bloom + 1 + 2 at full quality: 10.5 ms (63 %) and
            // 12.5 ms (75 %); 3.8 and 4.5 ms with everything cut, under the
            // 50 % fast line.
            for (base, bloom) in [(6.0f32, 1.5f32), (7.0, 2.5)] {
                let full = base + bloom + 3.0;
                assert!((0.6..=0.75).contains(&(full / target)), "{full}");
                let (st, acts) = load_then_steady(hz, base, bloom, 45.0);
                assert!(!st.is_degraded(), "{hz} Hz {full} ms: {st:?} {acts:?}");
                // All seven steps back in one fast recovery: on
                // consecutive frames, within 5 s of the load ending.
                assert_eq!(acts.len(), 7, "{hz} Hz {full} ms: {acts:?}");
                let span = acts[6].0 - acts[0].0;
                assert!(span <= 7.0 * f64::from(1000.0 / hz) + 1e-6, "{hz} Hz: {acts:?}");
                assert!(acts[6].0 <= 5000.0, "{hz} Hz: {acts:?}");
                // Nothing after that for the rest of the 45 s (40 s and
                // more of steady work at 63 or 75 %).
                assert!(45_000.0 - acts[6].0 >= 30_000.0);
                println!(
                    "{hz} Hz, {:.0} % of target: 7 steps back in {:.0} ms ({:.1} s after the load), then no change for {:.1} s",
                    100.0 * full / target,
                    span,
                    acts[6].0 / 1000.0,
                    (45_000.0 - acts[6].0) / 1000.0
                );
            }
        }
    }

    /// Gate 0 follow-up: when the fast recovery restores steps the scene
    /// cannot afford all of (their costs were measured under the load and
    /// scaled down too far), the controller cuts once and then stays put
    /// for good: no probe, no second fast recovery, no flapping, over 40 s
    /// at 30, 60 and 144 Hz. Two scenes: bloom costs 9 ms (work 9 ms
    /// without it, between the fast and the safe line) or 14 ms (7 ms
    /// without it, under the fast line).
    #[test]
    fn a_fast_restore_that_only_partly_fits_cuts_once_then_settles() {
        let target = AqPolicy::default().target_ms;
        for hz in [30.0f32, 60.0, 144.0] {
            for (base, bloom) in [(6.0f32, 9.0f32), (4.0, 14.0)] {
                assert!(base + bloom + 3.0 > target);
                let (st, acts) = load_then_steady(hz, base, bloom, 45.0);
                let restores: Vec<_> = acts
                    .iter()
                    .filter(|(_, a)| !matches!(a, AqAction::ShedEffect(_)))
                    .collect();
                let cuts: Vec<_> = acts
                    .iter()
                    .filter(|(_, a)| matches!(a, AqAction::ShedEffect(_)))
                    .collect();
                assert_eq!(restores.len(), 7, "{hz} Hz bloom {bloom}: {acts:?}");
                assert_eq!(cuts.len(), 1, "{hz} Hz bloom {bloom}: cut more than once: {acts:?}");
                assert_eq!(cuts[0].1, AqAction::ShedEffect("bloom"));
                assert_eq!(acts.last().unwrap().1, AqAction::ShedEffect("bloom"));
                assert!(45_000.0 - cuts[0].0 >= 30_000.0, "{acts:?}");
                assert_eq!(st.shed, ["bloom"]);
                assert!((st.scale - 1.0).abs() < 1e-4);
                println!(
                    "{hz} Hz, bloom {bloom} ms: 7 steps back, bloom cut again at {:.1} s, then no change for {:.1} s",
                    cuts[0].0 / 1000.0,
                    (45_000.0 - cuts[0].0) / 1000.0
                );
            }
        }
    }

    /// Follow-up 2: one-off hitches never cut quality. A 250 ms frame every
    /// 2 s, and a burst of five 40 ms frames every 3 s, on a scene that
    /// needs 4 ms: the window's 80th percentile stays at 4 ms, where the
    /// mean of a window holding one 250 ms frame is over 19 ms. A sustained
    /// load still does cut.
    #[test]
    fn one_off_hitches_never_cut_quality() {
        let p = AqPolicy::default();
        let on = [("bloom", 0.27), ("vignette", 0.3)];
        let mut c = AqController::default();
        let mut st = QualityState::default();
        let mut now = 0.0;
        let mut frame = 0u32;
        let mut actions = Vec::new();
        while now < 60_000.0 {
            frame += 1;
            let (dt, work) = if frame.is_multiple_of(120) {
                (250.0, 250.0)
            } else if frame % 180 < 5 {
                (40.0, 40.0)
            } else {
                (16.7, 4.0)
            };
            now += f64::from(dt);
            if let Some(a) = c.update(&p, &sample(dt, work, now), &st, &on) {
                st.apply(a);
                actions.push(a);
            }
        }
        assert_eq!(actions, []);
        assert!(c.work_ms().is_some_and(|w| w < 5.0));
        // 25 ms of work on every frame for a second: bloom goes.
        let t0 = now;
        let mut cut = None;
        while cut.is_none() && now - t0 < 2000.0 {
            now += 25.0;
            cut = c.update(&p, &sample(25.0, 25.0, now), &st, &on);
        }
        assert_eq!(cut, Some(AqAction::ShedEffect("bloom")));
    }

    /// Follow-up 2: frames during a disturbance (pipeline rebuild, shader
    /// compile, tier switch, live change) and `aq.grace_ms` after the last
    /// one are never judged, however slow.
    #[test]
    fn disturbances_and_their_grace_are_not_judged() {
        let p = AqPolicy::default();
        let on = [("bloom", 0.27)];
        let mut c = AqController::default();
        let st = QualityState::default();
        let mut now = 0.0;
        // A shader compiles for 3 s: 300 ms stalls, every frame disturbed.
        while now < 3000.0 {
            now += 300.0;
            c.disturb(now, p.grace_ms);
            assert_eq!(c.update(&p, &sample(300.0, 300.0, now), &st, &on), None);
        }
        let last = now;
        // The grace second after it: still not judged.
        while now < last + f64::from(p.grace_ms) - 40.0 {
            now += 40.0;
            assert!(c.settling(now));
            assert_eq!(c.update(&p, &sample(40.0, 40.0, now), &st, &on), None);
        }
        assert_eq!(c.work_ms(), None, "nothing measured yet");
        // Then a real 40 ms load is judged from scratch: one window over
        // plus degrade_frames.
        let mut cut = None;
        while cut.is_none() && now - last < 5000.0 {
            now += 40.0;
            cut = c.update(&p, &sample(40.0, 40.0, now), &st, &on);
        }
        assert_eq!(cut, Some(AqAction::ShedEffect("bloom")));
        assert!(now - last > f64::from(p.grace_ms + p.window_ms), "{}", now - last);
    }

    #[test]
    fn warmup_picks_highest_tier_that_holds() {
        let tiers = Tiers::builtin().unwrap();
        let idx = |id: &str| tiers.presets().iter().position(|p| p.id == id).unwrap();
        let mut w = TierWarmup::new(&tiers, 8192, 2000.0, 5, 15.0).unwrap();
        assert_eq!(w.first(), idx("ultra"));
        // Ultra runs at 30 fps, high at 60.
        let mut step = WarmupStep::Continue;
        let mut frames = 0;
        while step == WarmupStep::Continue {
            step = w.update(&tiers, 33.3);
            frames += 1;
        }
        assert_eq!(step, WarmupStep::Try(idx("high")));
        // 2000 ms over 5 candidates (Lite to Ultra) = 400 ms per tier: 12 frames at 33.3 ms.
        assert!(frames >= 5 + 12, "{frames}");
        loop {
            match w.update(&tiers, 16.7) {
                WarmupStep::Continue => {}
                s => {
                    step = s;
                    break;
                }
            }
        }
        assert_eq!(step, WarmupStep::Done(idx("high")));
        assert_eq!(w.results.len(), 2);

        // Small adapters skip Ultra (2560x1440 target).
        let w = TierWarmup::new(&tiers, 2048, 2000.0, 5, 15.0).unwrap();
        assert_eq!(w.first(), idx("high"));

        // Nothing holds: the lowest (Lite).
        let mut w = TierWarmup::new(&tiers, 8192, 400.0, 0, 0.0).unwrap();
        let last = loop {
            if let WarmupStep::Done(i) = w.update(&tiers, 50.0) {
                break i;
            }
        };
        assert_eq!(last, idx("lite"));
    }

    #[test]
    fn params_register_with_policy_bake() {
        let mut reg = ParamRegistry::new();
        let p = AqParams::register(&mut reg).unwrap();
        let d = reg.def(reg.id("aq.hysteresis_pct").unwrap()).unwrap();
        assert_eq!(d.bake, BakeTarget::Policy);
        assert_eq!(d.owner, "aq");
        let pol = p.policy(&reg, 13.9);
        assert_eq!(pol.target_ms, 13.9);
        assert_eq!(pol.window_ms, 500.0);
        assert_eq!(pol.safe_pct, 70.0);
        assert_eq!(pol.scale_step, 0.05);
        assert_eq!(pol.floor(), 0.8);
        reg.set_json("aq.target_ms", &serde_json::json!(20.0))
            .unwrap();
        assert_eq!(p.policy(&reg, 13.9).target_ms, 20.0);
    }
}
