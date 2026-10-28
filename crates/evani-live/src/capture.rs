//! `capture.frame`, `bench.run`, `compare.ab` (docs/LIVE_API.md 6.14-6.16)
//! and `sweep.run` (6.29, planned in [`sweep`]).
//!
//! Each command starts a [`Run`] stored in [`LiveSession::run`]: a queue of
//! steps driven once per frame by [`frame`], never blocking a frame.
//!
//! * `Apply(target)`: make the live state equal a [`FullState`] in two
//!   passes (the second puts back effect switches a tier change resets),
//!   sending WGSL slot bodies through `SlotRebuilds` and waiting for them,
//!   then letting it settle for a few frames.
//! * `Capture`: hold the sim clock at `t`, put the camera on the bench path,
//!   wait two frames, request a `CaptureView::Current` capture and wait for
//!   the renderer's pixels (5 s timeout).
//! * `Measure`: hold `t = 0` for the warm-up, then fly the bench path one
//!   frame per 1/60 s of sim time and keep each frame's telemetry sample.
//! * `Finish`: restore the overrides and reply.
//!
//! Bench and compare snapshot the state first (S0) and always end with
//! `Apply(S0)` (also after a failure), except `compare.ab` with `keep`, which
//! ends at B and records it as one transaction. While a bench or compare run
//! is active, live changes answer `busy`; a capture run pauses the inbox.

use std::collections::VecDeque;

mod sweep;

use evani_core::camera::{CameraPath, CameraPose};
use evani_core::capture::{BenchPath, CanvasHud, CaptureMoments, CaptureQueue, CaptureResult, CaptureView, HoldCanvas, MomentSetup};
use evani_core::ecs::World;
use evani_core::overlay::RenderStats;
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::slots::{RebuildHandle, ShaderSlots, SlotCompileResult, SlotId, SlotRebuilds};
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use serde_json::{Map, Value, json};

use crate::change::{Change, FullState};
use crate::img::{self, Rgb};
use crate::session::{LiveSession, NO_RENDERER_FRAMES, SLOT_TIMEOUT_MS, Tx, TxStatus, now_iso};
use crate::telemetry::{MAX_PASSES, Sample, round2};
use crate::{LiveError, MUTATING, commands};

/// The run commands.
pub const RUN_COMMANDS: &[&str] = &["capture.frame", "bench.run", "compare.ab", "sweep.run"];

/// Commands refused with `busy` while a bench or compare run is active
/// (besides [`MUTATING`]): they change state or read the run's temporary one.
const BUSY_TOO: &[&str] = &[
    "history.undo",
    "history.redo",
    "snapshot.save",
    "bake.export",
    "capture.frame",
    "bench.run",
    "compare.ab",
    "sweep.run",
    "edit.stage",
    "edit.commit",
    "edit.resolve",
];

/// Frames to let a change settle after it is applied.
pub const SETTLE_FRAMES: u32 = 3;
/// Frames between setting the capture overrides and requesting the capture.
pub const CAPTURE_DELAY_FRAMES: u32 = 2;
/// Frames between a moment's setup and the capture (the game's camera and
/// any per-look caches catch up).
pub const MOMENT_DELAY_FRAMES: u32 = 4;
/// A capture the renderer hasn't answered in this long fails with `timeout`.
pub const CAPTURE_TIMEOUT_MS: f64 = 5_000.0;
/// Sim seconds per measured frame.
const STEP_S: f64 = 1.0 / 60.0;
/// Most `capture_at` times per run.
const MAX_CAPTURES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Capture,
    Bench,
    Compare,
    Sweep,
}

/// A state to apply. Params in `skip` are left as they are (effect switches
/// the tier change resets and the side didn't set itself).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Target {
    pub state: FullState,
    pub skip: Vec<ParamId>,
}

/// One capture taken during a run.
#[derive(Debug, Clone)]
struct Shot {
    side: usize,
    t: Option<f64>,
    img: Rgb,
    frame: u64,
    sim_time: f64,
    stats: RenderStats,
    camera: Option<CameraPose>,
    /// The canvas-drawn HUD of the captured frame, if the kit declares one.
    canvas_hud: Option<CanvasHud>,
    /// A sweep's bloom-off reference shot.
    reference: bool,
    /// A sweep's thumbnail, downscaled from the same frame.
    thumb: Option<Rgb>,
}

#[derive(Debug, Clone)]
enum Info {
    Capture {
        sim_time: Option<f64>,
        /// The scripted moment (name, label), if one was asked for.
        moment: Option<(String, String)>,
        /// `{tier, values}` when the shot used a temporary tier and/or
        /// values (6.28): what was applied for the shot, then restored.
        temp: Option<Value>,
    },
    Bench {
        tier: String,
        seconds: f64,
        warmup_s: f64,
    },
    Compare {
        sides: [Value; 2],
        tier: String,
        seconds: f64,
        warmup_s: f64,
        order: &'static str,
        keep: bool,
        capture_at: Vec<f64>,
    },
    Sweep(Box<sweep::SweepInfo>),
}

#[derive(Debug)]
struct SlotWait {
    handle: RebuildHandle,
    id: SlotId,
    new: String,
    result: Option<SlotCompileResult>,
}

#[derive(Debug)]
enum ApplyPhase {
    Start,
    Slots {
        waits: Vec<SlotWait>,
        frames: u32,
        started_ms: f64,
    },
    Settle(u32),
}

#[derive(Debug)]
struct ApplyStep {
    target: Box<Target>,
    pass: u8,
    phase: ApplyPhase,
    /// The run's last apply (back to S0, or to B with `keep`).
    last: bool,
}

#[derive(Debug)]
enum CapPhase {
    Setup,
    Wait(u32),
    Pending { id: u64, since_ms: f64 },
}

#[derive(Debug)]
struct CaptureStep {
    side: usize,
    /// A scripted moment to set up first (then the clock is held where the
    /// setup left it and the game camera is used).
    moment: Option<MomentSetup>,
    /// Sim time to hold (`None`: the next frame as it is).
    t: Option<f64>,
    /// Put the camera on the bench path at `t`.
    path_cam: bool,
    /// A fixed camera (`capture.frame` `camera: {eye, target, fov_y_deg}`).
    fixed_cam: Option<CameraPose>,
    /// A sweep's bloom-off reference shot.
    reference: bool,
    /// Also keep a thumbnail fitting `(w, h)` (a sweep's `thumbs`).
    thumb: Option<(u32, u32)>,
    phase: CapPhase,
}

/// `capture.frame` geocheck options: the sky colour, sub-pixel jitter and
/// the geometry debug view for the shot (restored when the run ends).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct ShotLook {
    sky: Option<[f32; 3]>,
    jitter_px: Option<[f32; 2]>,
    geometry: bool,
}

/// What a capture's [`ShotLook`] replaced: the clear colour, the jitter
/// and whether the geometry view was on.
type LookPrev = (Option<[f32; 3]>, Option<[f32; 2]>, bool);

impl ShotLook {
    fn any(&self) -> bool {
        self.sky.is_some() || self.jitter_px.is_some() || self.geometry
    }
}

#[derive(Debug)]
enum MeasPhase {
    Start,
    Warmup { until_ms: f64 },
    Run { k: u32, n: u32, started_ms: f64 },
}

#[derive(Debug)]
struct MeasureStep {
    side: usize,
    seconds: f64,
    warmup_s: f64,
    phase: MeasPhase,
    next_progress_ms: f64,
}

#[derive(Debug)]
enum Step {
    Apply(ApplyStep),
    Capture(CaptureStep),
    Measure(MeasureStep),
    /// A sweep row's captures are in: compute its look measures.
    SweepRow(usize),
    Finish,
}

enum Poll {
    Wait,
    Done,
    Fail(LiveError),
}

/// An active capture, bench or compare run.
#[derive(Debug)]
pub struct Run {
    /// `c-1`, `b-2`, `ab-3`.
    pub id: String,
    pub kind: RunKind,
    pub cmd: &'static str,
    token: u64,
    req_id: Value,
    req: Value,
    started_ms: f64,
    est_ms: f64,
    s0: FullState,
    prev_camera: Option<CameraPose>,
    prev_sim: Option<f64>,
    /// Geocheck options of a capture and what they replaced (`Some` once
    /// applied: the previous clear colour, jitter and view state).
    look: ShotLook,
    look_prev: Option<LookPrev>,
    path: CameraPath,
    width: u32,
    steps: VecDeque<Step>,
    /// The last apply has started; a failure now just finishes.
    restoring: bool,
    /// A capture run that applied a temporary tier or values: a failure
    /// restores S0 like a bench or compare does.
    restore_on_fail: bool,
    error: Option<LiveError>,
    shots: Vec<Shot>,
    /// Measured samples per side (compare: A, B; sweep: one per row).
    samples: Vec<Vec<Sample>>,
    /// The wall-time cap cut a measure short.
    truncated: bool,
    info: Info,
    /// Note of the kept transaction (compare with `keep`).
    keep_note: String,
    /// The run tries settings or views the player must not see: the canvas
    /// stays on its last frame until the run ends ([`sync_hold`]).
    hold_canvas: bool,
    /// `capture.frame { hold_after }`: the clock stays held where the shot
    /// was taken after the reply (until the next run ends), so a page
    /// screenshot taken next shows the very same frame.
    hold_after: bool,
}

impl Run {
    /// About how long until the run ends (ms, never negative).
    pub fn remaining_ms(&self, now_ms: f64) -> f64 {
        (self.started_ms + self.est_ms - now_ms).max(0.0).round()
    }

    /// `session.status.busy` (6.26).
    pub fn busy_json(&self, now_ms: f64) -> Value {
        json!({ "run": self.id, "cmd": self.cmd, "until_ms": self.remaining_ms(now_ms) })
    }

    fn fail(&mut self, e: LiveError) {
        if self.error.is_none() {
            self.error = Some(e);
        }
        self.steps.clear();
        if !self.restoring && (self.kind != RunKind::Capture || self.restore_on_fail) {
            self.restoring = true;
            self.steps.push_back(apply(
                Target {
                    state: self.s0.clone(),
                    skip: Vec::new(),
                },
                true,
            ));
        }
        self.steps.push_back(Step::Finish);
    }
}

fn apply(target: Target, last: bool) -> Step {
    Step::Apply(ApplyStep {
        target: Box::new(target),
        pass: 1,
        phase: ApplyPhase::Start,
        last,
    })
}

fn capture_step(side: usize, t: Option<f64>, path_cam: bool) -> Step {
    Step::Capture(CaptureStep {
        side,
        moment: None,
        t,
        path_cam,
        fixed_cam: None,
        reference: false,
        thumb: None,
        phase: CapPhase::Setup,
    })
}

fn measure_step(side: usize, seconds: f64, warmup_s: f64) -> Step {
    Step::Measure(MeasureStep {
        side,
        seconds,
        warmup_s,
        phase: MeasPhase::Start,
        next_progress_ms: 0.0,
    })
}

// ---- small helpers ------------------------------------------------------------------

fn now(world: &World) -> (f64, u64) {
    world
        .try_res::<Time>()
        .map_or((0.0, 0), |t| (t.now_ms, t.frame))
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn set_hold(world: &World, t: f64, camera: Option<CameraPose>) {
    if let Some(mut q) = world.try_res_mut::<CaptureQueue>() {
        q.sim_time_override = Some(t);
        if camera.is_some() {
            q.camera_override = camera;
        }
    }
}

fn bad(msg: impl Into<String>) -> LiveError {
    LiveError::new("invalid_value", msg)
}

fn num(params: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, LiveError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => match v.as_f64() {
            Some(x) if x.is_finite() && (lo..=hi).contains(&x) => Ok(x),
            _ => Err(bad(format!("{key} must be a number {lo}-{hi}"))),
        },
    }
}

fn width_param(params: &Value, key: &str, default: u32) -> Result<u32, LiveError> {
    width_range(params, key, default, 1024)
}

/// Widest `capture.frame` (geocheck reads full-resolution frames; a shot is
/// never wider than the canvas, whatever is asked).
pub const CAPTURE_MAX_WIDTH: u32 = 4096;

fn width_range(params: &Value, key: &str, default: u32, max: u32) -> Result<u32, LiveError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => match v.as_u64() {
            Some(w) if (64..=u64::from(max)).contains(&w) => Ok(w as u32),
            _ => Err(bad(format!("{key} must be an integer 64-{max}"))),
        },
    }
}

fn floats<const N: usize>(v: &Value, what: &str) -> Result<[f32; N], LiveError> {
    let a: Vec<f32> = v
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).filter(|x| x.is_finite()).map(|x| x as f32).collect())
        .unwrap_or_default();
    a.try_into().map_err(|_| bad(format!("{what} must be a list of {N} numbers")))
}

/// `camera: {eye, target, fov_y_deg?, up?}`.
fn camera_pose(v: &Value) -> Result<CameraPose, LiveError> {
    let d = CameraPose::default();
    let eye = floats::<3>(v.get("eye").unwrap_or(&Value::Null), "camera.eye")?;
    let target = floats::<3>(v.get("target").unwrap_or(&Value::Null), "camera.target")?;
    let up = match v.get("up") {
        None | Some(Value::Null) => d.up,
        Some(u) => floats::<3>(u, "camera.up")?,
    };
    let fov_y_deg = match v.get("fov_y_deg") {
        None | Some(Value::Null) => d.fov_y_deg,
        Some(f) => match f.as_f64() {
            Some(x) if (5.0..=150.0).contains(&x) => x as f32,
            _ => return Err(bad("camera.fov_y_deg must be a number 5-150")),
        },
    };
    let dir: f32 = (0..3).map(|i| (target[i] - eye[i]).powi(2)).sum();
    if dir < 1e-8 {
        return Err(bad("camera.eye and camera.target must differ"));
    }
    Ok(CameraPose { eye, target, up, fov_y_deg })
}

/// `sky`, `jitter_px` and `geometry` of `capture.frame`.
fn shot_look(params: &Value) -> Result<ShotLook, LiveError> {
    let sky = match params.get("sky") {
        None | Some(Value::Null | Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(crate::geometry::SKY),
        Some(v) => {
            let c = floats::<3>(v, "sky")?;
            if c.iter().any(|x| !(0.0..=1.0).contains(x)) {
                return Err(bad("sky colour values must be 0-1 (linear RGB)"));
            }
            Some(c)
        }
    };
    let jitter_px = match params.get("jitter_px") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let j = floats::<2>(v, "jitter_px")?;
            if j.iter().any(|x| x.abs() > 4.0) {
                return Err(bad("jitter_px must be within 4 pixels"));
            }
            Some(j)
        }
    };
    let geometry = match params.get("geometry") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("geometry must be true or false")),
    };
    Ok(ShotLook { sky, jitter_px, geometry })
}

/// Apply a capture's geocheck options (first time only), remembering what
/// they replace.
fn apply_look(world: &World, run: &mut Run) {
    if !run.look.any() || run.look_prev.is_some() {
        return;
    }
    let Some(mut q) = world.try_res_mut::<CaptureQueue>() else {
        return;
    };
    let view_on = world
        .try_res::<crate::geometry::GeometryView>()
        .is_some_and(|g| g.on);
    run.look_prev = Some((q.clear_override, q.jitter_px, view_on));
    if let Some(c) = run.look.sky {
        q.clear_override = Some(c);
    }
    if let Some(j) = run.look.jitter_px {
        q.jitter_px = Some(j);
    }
    if run.look.geometry
        && let Some(mut g) = world.try_res_mut::<crate::geometry::GeometryView>()
    {
        g.switch_on();
    }
}

/// Give back what [`apply_look`] replaced.
fn restore_look(world: &World, run: &Run) {
    let Some((clear, jitter, view_on)) = run.look_prev else {
        return;
    };
    let Some(mut q) = world.try_res_mut::<CaptureQueue>() else {
        return;
    };
    if run.look.geometry
        && !view_on
        && let Some(mut g) = world.try_res_mut::<crate::geometry::GeometryView>()
    {
        g.switch_off(Some(&mut q));
    }
    if run.look.sky.is_some() {
        q.clear_override = clear;
    }
    if run.look.jitter_px.is_some() {
        q.jitter_px = jitter;
    }
}

fn capture_at(params: &Value, seconds: f64, default: Vec<f64>) -> Result<Vec<f64>, LiveError> {
    let Some(v) = params.get("capture_at").filter(|v| !v.is_null()) else {
        return Ok(default);
    };
    let arr = v
        .as_array()
        .ok_or_else(|| bad("capture_at must be an array of seconds"))?;
    if arr.len() > MAX_CAPTURES {
        return Err(bad(format!("capture_at takes at most {MAX_CAPTURES} times")));
    }
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        match x.as_f64() {
            Some(t) if t.is_finite() && (0.0..=seconds).contains(&t) => out.push(t),
            _ => return Err(bad(format!("capture_at times must be within 0-{seconds} s"))),
        }
    }
    Ok(out)
}

fn tier_param(world: &World, params: &Value) -> Result<Option<String>, LiveError> {
    let Some(v) = params.get("tier").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let id = v
        .as_str()
        .ok_or_else(|| LiveError::new("bad_request", "params.tier must be a string"))?;
    let tiers = world.res::<Tiers>();
    if tiers.get(id).is_none() {
        let ids: Vec<String> = tiers.presets().iter().map(|p| p.id.clone()).collect();
        return Err(
            LiveError::new("unknown_target", format!("No tier named {id}"))
                .with(json!({ "did_you_mean": ids })),
        );
    }
    Ok(Some(id.to_string()))
}

/// `base` with `changes` applied (their `new` values). Pure.
pub fn with_changes(base: &FullState, changes: &[Change]) -> FullState {
    fn put<K: PartialEq + Copy, V: Clone>(list: &mut Vec<(K, V)>, k: K, v: &V) {
        match list.iter_mut().find(|(i, _)| *i == k) {
            Some(e) => e.1 = v.clone(),
            None => list.push((k, v.clone())),
        }
    }
    let mut s = base.clone();
    for c in changes {
        match c {
            Change::Param { id, new, .. } => put(&mut s.params, *id, new),
            Change::Slot { id, new, .. } => put(&mut s.slots, *id, new),
            Change::Formula { id, new, .. } => put(&mut s.formulas, *id, new),
            Change::Tier { new, .. } => s.tier = new.clone(),
            Change::TierEffects { tier, new, .. } => {
                match s.tier_effects.iter_mut().find(|(t, _)| t == tier) {
                    Some(e) => e.1 = new.clone(),
                    None => s.tier_effects.push((tier.clone(), new.clone())),
                }
            }
        }
    }
    s
}

fn param_of(s: &FullState, id: ParamId) -> Option<ParamValue> {
    s.params.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
}

fn set_param(s: &mut FullState, id: ParamId, v: ParamValue) {
    match s.params.iter_mut().find(|(i, _)| *i == id) {
        Some(e) => e.1 = v,
        None => s.params.push((id, v)),
    }
}

/// The run's target for one side: `state` at `tier` (if given) with
/// adaptive quality off. When the tier changes and `reset_fx`, the effect
/// switches the side left as in S0 follow the new tier's defaults.
fn run_target(
    world: &World,
    s0: &FullState,
    mut state: FullState,
    tier: Option<&str>,
    reset_fx: bool,
) -> Target {
    if let Some(t) = tier {
        state.tier = t.to_string();
    }
    let mut skip = Vec::new();
    if state.tier != s0.tier && (reset_fx || tier.is_some()) {
        for (id, _) in commands::effect_switches(world) {
            if param_of(&state, id) == param_of(s0, id) {
                skip.push(id);
            }
        }
    }
    if let Some(aq) = world.res::<ParamRegistry>().id("aq.enabled") {
        set_param(&mut state, aq, ParamValue::Bool(false));
    }
    Target { state, skip }
}

/// One side of a compare, resolved against S0.
struct Side {
    label: Value,
    /// The side's real state (what `keep` ends at).
    state: FullState,
    /// Built from changes (`{cmds}` / `{values}`), not a whole snapshot.
    from_changes: bool,
}

fn resolve_side(world: &World, s0: &FullState, v: Option<&Value>, which: &str) -> Result<Side, LiveError> {
    let v = v.unwrap_or(&Value::Null);
    match v {
        Value::Null if which == "a" => Ok(Side {
            label: json!("current"),
            state: s0.clone(),
            from_changes: false,
        }),
        Value::String(s) if s == "current" => Ok(Side {
            label: json!("current"),
            state: s0.clone(),
            from_changes: false,
        }),
        Value::String(sid) => {
            let s = world.res::<LiveSession>();
            match s.snapshots.iter().find(|x| x.id == *sid || x.name == *sid) {
                Some(x) => Ok(Side {
                    label: json!(x.id),
                    state: x.state.clone(),
                    from_changes: false,
                }),
                None => {
                    let ids: Vec<&str> = s.snapshots.iter().map(|x| x.id.as_str()).collect();
                    Err(LiveError::new("unknown_target", format!("No snapshot {sid}"))
                        .with(json!({ "did_you_mean": ids, "side": which })))
                }
            }
        }
        Value::Object(o) => {
            let plan = if let Some(cmds) = o.get("cmds") {
                commands::plan(world, "batch", &json!({ "cmds": cmds }))
            } else if let Some(values) = o.get("values") {
                commands::plan(world, "param.set", &json!({ "values": values }))
            } else {
                Err(LiveError::new(
                    "bad_request",
                    format!("params.{which} must be \"current\", a snapshot id, {{cmds:[...]}} or {{values:{{...}}}}"),
                ))
            };
            let plan = plan.map_err(|mut e| {
                let mut d = e.data.take().unwrap_or_else(|| json!({}));
                if !d.is_object() {
                    d = json!({ "detail": d });
                }
                d["side"] = json!(which);
                e.data = Some(d);
                e
            })?;
            Ok(Side {
                label: json!({ "changes": plan.applied }),
                state: with_changes(s0, &plan.changes),
                from_changes: true,
            })
        }
        _ => Err(LiveError::new(
            "bad_request",
            format!("params.{which} must be \"current\", a snapshot id or an object"),
        )),
    }
}

// ---- busy and start -----------------------------------------------------------------

/// `busy` for `cmd` while a bench or compare run is active.
pub(crate) fn busy(world: &World, cmd: &str) -> Option<LiveError> {
    let s = world.res::<LiveSession>();
    let run = s.run.as_ref()?;
    if run.kind == RunKind::Capture || !(MUTATING.contains(&cmd) || BUSY_TOO.contains(&cmd)) {
        return None;
    }
    let (now_ms, _) = now(world);
    Some(
        LiveError::new(
            "busy",
            format!("{} {} is running; try again when it ends", run.cmd, run.id),
        )
        .with(json!({ "until_ms": run.remaining_ms(now_ms), "run": run.id })),
    )
}

/// Is a capture run (which pauses the inbox) active?
pub(crate) fn capture_active(world: &World) -> bool {
    world
        .try_res::<LiveSession>()
        .is_some_and(|s| s.run.as_ref().is_some_and(|r| r.kind == RunKind::Capture))
}

/// Start `capture.frame`, `bench.run` or `compare.ab`. The reply comes
/// when the run finishes; `Err` replies at once.
pub(crate) fn start(
    world: &World,
    token: u64,
    id: &Value,
    req: &Value,
    cmd: &str,
    params: &Value,
) -> Result<(), LiveError> {
    if world.res::<LiveSession>().run.is_some() {
        // Only a capture run can be active here (the others answer busy
        // earlier), and a capture run pauses the inbox; belt and braces.
        return Err(LiveError::new("busy", "a run is in progress").with(json!({ "until_ms": 0 })));
    }
    if !world.has_resource::<CaptureQueue>() {
        return Err(LiveError::new("internal", "no capture queue in this build"));
    }
    let (now_ms, _) = now(world);
    let path = world
        .try_res::<BenchPath>()
        .map_or_else(default_path, |p| p.0);
    // A clock a `hold_after` capture kept held goes back to what it was
    // before that capture when this run ends (unless this run keeps it too).
    // (Forgotten only once the run starts: a refused request leaves it.)
    let kept = world.res::<LiveSession>().kept_hold;
    let (prev_camera, prev_sim) = kept.unwrap_or_else(|| {
        let q = world.res::<CaptureQueue>();
        (q.camera_override, q.sim_time_override)
    });
    let render_t = world.try_res::<Time>().map_or(0.0, |t| t.render_seconds());
    let mut run = Run {
        id: String::new(),
        kind: RunKind::Capture,
        cmd: "capture.frame",
        token,
        req_id: id.clone(),
        req: req.clone(),
        started_ms: now_ms,
        est_ms: 0.0,
        s0: FullState::default(),
        prev_camera,
        prev_sim,
        look: ShotLook::default(),
        look_prev: None,
        path,
        width: 480,
        steps: VecDeque::new(),
        restoring: false,
        restore_on_fail: false,
        error: None,
        shots: Vec::new(),
        samples: vec![Vec::new(), Vec::new()],
        truncated: false,
        info: Info::Capture {
            sim_time: None,
            moment: None,
            temp: None,
        },
        keep_note: String::new(),
        hold_canvas: false,
        hold_after: false,
    };
    match cmd {
        "capture.frame" => {
            run.width = width_range(params, "width", 480, CAPTURE_MAX_WIDTH)?;
            run.look = shot_look(params)?;
            let sim_time = match params.get("sim_time") {
                None | Some(Value::Null) => None,
                Some(v) => match v.as_f64() {
                    Some(t) if t.is_finite() && t >= 0.0 => Some(t),
                    _ => return Err(bad("sim_time must be a number of seconds, 0 or more")),
                },
            };
            let (camera, fixed_cam) = match params.get("camera") {
                None | Some(Value::Null) => ("current", None),
                Some(Value::String(c)) if c == "current" || c == "path" => (c.as_str(), None),
                Some(v @ Value::Object(_)) => ("fixed", Some(camera_pose(v)?)),
                Some(c) => {
                    return Err(bad(format!(
                        "camera must be \"current\", \"path\" or {{eye, target, fov_y_deg}}, got {c}"
                    )));
                }
            };
            let path_cam = camera == "path";
            let moment = match params.get("moment") {
                None | Some(Value::Null) => None,
                Some(Value::String(name)) => {
                    if path_cam || fixed_cam.is_some() || sim_time.is_some() {
                        return Err(bad("moment sets its own time and uses the game camera; leave out sim_time and camera"));
                    }
                    let moments = world.try_res::<CaptureMoments>();
                    let found = moments.as_ref().and_then(|m| m.get(name)).cloned();
                    match found {
                        Some(m) => Some(m),
                        None => {
                            let names: Vec<String> = moments
                                .as_ref()
                                .map(|m| m.names().into_iter().map(String::from).collect())
                                .unwrap_or_default();
                            return Err(LiveError::new("unknown_target", format!("No capture moment {name}"))
                                .with(json!({ "did_you_mean": names })));
                        }
                    }
                }
                Some(_) => return Err(bad("moment must be a moment name (see session.status moments)")),
            };
            // The path camera needs a time: the current one if none given.
            // A fixed camera holds the sim too (now, unless sim_time says).
            let t = sim_time.or((path_cam || fixed_cam.is_some()).then_some(render_t));
            run.hold_after = match params.get("hold_after") {
                None | Some(Value::Null | Value::Bool(false)) => false,
                Some(Value::Bool(true)) if t.is_some() || moment.is_some() => true,
                Some(Value::Bool(true)) => return Err(bad("hold_after needs a held clock: give a moment, sim_time or camera")),
                Some(_) => return Err(bad("hold_after must be true or false")),
            };
            // A temporary tier and/or values for this one shot (the look
            // guard's bloom-off pass, 6.28): applied with adaptive quality
            // off, then S0 is restored (also on failure).
            let tier = tier_param(world, params)?;
            let values = match params.get("values") {
                None | Some(Value::Null) => None,
                Some(v @ Value::Object(_)) => Some(v.clone()),
                Some(_) => return Err(bad("values must be an object of param name to value")),
            };
            let mut temp = None;
            if tier.is_some() || values.is_some() {
                run.s0 = FullState::capture(world);
                let (state, applied, changed) = match &values {
                    Some(v) => {
                        let plan = commands::plan(world, "param.set", &json!({ "values": v }))?;
                        let changed: Vec<ParamId> = plan
                            .changes
                            .iter()
                            .filter_map(|c| match c {
                                Change::Param { id, .. } => Some(*id),
                                _ => None,
                            })
                            .collect();
                        (with_changes(&run.s0, &plan.changes), json!(plan.applied), changed)
                    }
                    None => (run.s0.clone(), Value::Null, Vec::new()),
                };
                let mut target = run_target(world, &run.s0, state, tier.as_deref(), true);
                // A value asked for explicitly holds even when it equals S0's
                // (bloom off stays off on a tier whose default is on).
                target.skip.retain(|id| !changed.contains(id));
                temp = Some(json!({ "tier": target.state.tier, "values": applied }));
                run.steps.push_back(apply(target, false));
                run.restore_on_fail = true;
            }
            let mut step = capture_step(0, t, path_cam);
            if let Step::Capture(c) = &mut step {
                c.moment = moment.as_ref().map(|m| m.setup);
                c.fixed_cam = fixed_cam;
            }
            run.steps.push_back(step);
            run.est_ms = if moment.is_some() { 400.0 } else { 150.0 };
            if temp.is_some() {
                run.steps.push_back(apply(
                    Target {
                        state: run.s0.clone(),
                        skip: Vec::new(),
                    },
                    true,
                ));
                run.est_ms += 400.0;
            }
            // Anything but the next frame as the player sees it (temporary
            // values or tier, geocheck options, another camera or time, a
            // moment) is shot with the canvas held.
            run.hold_canvas = temp.is_some() || run.look.any() || t.is_some() || moment.is_some();
            run.info = Info::Capture {
                sim_time: t,
                moment: moment.map(|m| (m.name, m.label)),
                temp,
            };
        }
        "bench.run" => {
            let seconds = num(params, "seconds", 10.0, 1.0, 120.0)?;
            let warmup_s = num(params, "warmup_s", 1.0, 0.0, 60.0)?;
            let tier = tier_param(world, params)?;
            let at = capture_at(params, seconds, Vec::new())?;
            run.width = width_param(params, "capture_width", 320)?;
            run.kind = RunKind::Bench;
            run.cmd = "bench.run";
            run.s0 = FullState::capture(world);
            let target = run_target(world, &run.s0, run.s0.clone(), tier.as_deref(), true);
            let tier_id = target.state.tier.clone();
            run.steps.push_back(apply(target, false));
            for t in &at {
                run.steps.push_back(capture_step(0, Some(*t), true));
            }
            run.steps.push_back(measure_step(0, seconds, warmup_s));
            run.steps.push_back(apply(
                Target {
                    state: run.s0.clone(),
                    skip: Vec::new(),
                },
                true,
            ));
            run.est_ms = 300.0 + 100.0 * at.len() as f64 + (seconds + warmup_s) * 1000.0;
            run.info = Info::Bench {
                tier: tier_id,
                seconds,
                warmup_s,
            };
        }
        "compare.ab" => {
            let seconds = num(params, "seconds", 4.0, 1.0, 120.0)?;
            let warmup_s = num(params, "warmup_s", 0.5, 0.0, 60.0)?;
            let tier = tier_param(world, params)?;
            let at = capture_at(params, seconds, vec![seconds / 2.0])?;
            run.width = width_param(params, "capture_width", 320)?;
            let order = match params.get("order").and_then(Value::as_str) {
                None | Some("ABBA") => "ABBA",
                Some("AB") => "AB",
                Some(o) => return Err(bad(format!("order must be \"ABBA\" or \"AB\", got {o:?}"))),
            };
            let keep = match params.get("keep") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(bad("keep must be true or false")),
            };
            let note = req.get("note").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if keep && (note.is_empty() || note.len() > 500) {
                return Err(LiveError::new(
                    "note_required",
                    "compare.ab with keep: true changes state and needs a note of 1-500 chars",
                ));
            }
            if params.get("b").is_none_or(Value::is_null) {
                return Err(LiveError::new("bad_request", "params.b is required"));
            }
            run.kind = RunKind::Compare;
            run.cmd = "compare.ab";
            run.s0 = FullState::capture(world);
            let a = resolve_side(world, &run.s0, params.get("a"), "a")?;
            let b = resolve_side(world, &run.s0, params.get("b"), "b")?;
            let targets = [
                run_target(world, &run.s0, a.state.clone(), tier.as_deref(), a.from_changes),
                run_target(world, &run.s0, b.state.clone(), tier.as_deref(), b.from_changes),
            ];
            let tier_id = targets[0].state.tier.clone();
            let visits: &[usize] = if order == "AB" { &[0, 1] } else { &[0, 1, 1, 0] };
            let mut seen = [false; 2];
            for &side in visits {
                run.steps.push_back(apply(targets[side].clone(), false));
                if !seen[side] {
                    seen[side] = true;
                    for t in &at {
                        run.steps.push_back(capture_step(side, Some(*t), true));
                    }
                }
                run.steps.push_back(measure_step(side, seconds, warmup_s));
            }
            let end = if keep {
                // The real B: no tier override, adaptive quality as it was.
                let mut skip = Vec::new();
                if b.from_changes && b.state.tier != run.s0.tier {
                    for (pid, _) in commands::effect_switches(world) {
                        if param_of(&b.state, pid) == param_of(&run.s0, pid) {
                            skip.push(pid);
                        }
                    }
                }
                Target {
                    state: b.state.clone(),
                    skip,
                }
            } else {
                Target {
                    state: run.s0.clone(),
                    skip: Vec::new(),
                }
            };
            run.steps.push_back(apply(end, true));
            run.est_ms = visits.len() as f64 * (150.0 + (seconds + warmup_s) * 1000.0)
                + 200.0 * at.len() as f64
                + 150.0;
            run.keep_note = note;
            run.info = Info::Compare {
                sides: [a.label, b.label],
                tier: tier_id,
                seconds,
                warmup_s,
                order,
                keep,
                capture_at: at,
            };
        }
        "sweep.run" => sweep::plan(world, &mut run, params)?,
        _ => return Err(LiveError::new("unknown_command", format!("no command {cmd}"))),
    }
    run.steps.push_back(Step::Finish);
    let mut s = world.res_mut::<LiveSession>();
    s.next_run += 1;
    let prefix = match run.kind {
        RunKind::Capture => "c",
        RunKind::Bench => "b",
        RunKind::Compare => "ab",
        RunKind::Sweep => "sw",
    };
    run.id = format!("{prefix}-{}", s.next_run);
    // Trials (A/B sides, sweep rows) are never shown: only the state the
    // run ends on is (6.30).
    if matches!(run.kind, RunKind::Compare | RunKind::Sweep) {
        run.hold_canvas = true;
    }
    let freeze = run.kind != RunKind::Capture;
    s.kept_hold = None;
    s.run = Some(run);
    drop(s);
    if freeze {
        // Keep the sim frozen for the whole run so A and B see the same scene.
        set_hold(world, render_t, None);
    }
    // Hold the canvas from the very next drawn frame, before the first
    // trial state is applied.
    sync_hold(world);
    Ok(())
}

/// Set [`HoldCanvas`] from the active run: on while a run
/// that tries settings is active, off otherwise. Called every frame (by
/// [`crate::tick`], after the run advanced) and when a run starts, so the
/// hold can never outlive its run, whatever ended it (done, failed, timed
/// out). Changing state outside a run (`param.set`, `batch`,
/// `snapshot.restore`) shows at once as before.
pub fn sync_hold(world: &World) {
    // A `together` edit commit that settles over frames holds too (6.31).
    let want = world.try_res::<LiveSession>().is_some_and(|s| {
        s.run.as_ref().is_some_and(|r| r.hold_canvas) || s.edit_hold.is_some()
    });
    if let Some(mut h) = world.try_res_mut::<HoldCanvas>()
        && h.0 != want
    {
        h.0 = want;
    }
}

/// Is the canvas held by a trial run right now?
pub fn canvas_held(world: &World) -> bool {
    world.try_res::<HoldCanvas>().is_some_and(|h| h.0)
}

/// The bench path when the game didn't set one.
pub fn default_path() -> CameraPath {
    CameraPath::orbit([0.0, 0.0, 0.0], 20.0, 8.0, 60.0)
}

// ---- per frame ----------------------------------------------------------------------

/// Advance the active run (called once per frame by [`crate::tick`]).
pub fn frame(world: &World) {
    let Some(mut run) = world.res_mut::<LiveSession>().run.take() else {
        return;
    };
    // Steps that finish at once run back to back; anything that has to see
    // a rendered frame waits for the next call.
    for _ in 0..64 {
        let Some(mut step) = run.steps.pop_front() else {
            finish(world, run);
            return;
        };
        let poll = match &mut step {
            Step::Apply(a) => {
                if a.last {
                    run.restoring = true;
                }
                poll_apply(world, a)
            }
            Step::Capture(c) => poll_capture(world, &mut run, c),
            Step::Measure(m) => poll_measure(world, &mut run, m),
            Step::SweepRow(row) => sweep::finish_row(&mut run, *row),
            Step::Finish => {
                finish(world, run);
                return;
            }
        };
        match poll {
            Poll::Wait => {
                run.steps.push_front(step);
                break;
            }
            Poll::Done => {}
            Poll::Fail(e) => {
                let e = if run.kind == RunKind::Sweep {
                    sweep::with_row(e, &step)
                } else {
                    e
                };
                run.fail(e);
            }
        }
    }
    world.res_mut::<LiveSession>().run = Some(run);
}

fn poll_apply(world: &World, a: &mut ApplyStep) -> Poll {
    loop {
        match &mut a.phase {
            ApplyPhase::Start => {
                let changes: Vec<Change> = FullState::capture(world)
                    .diff(&a.target.state)
                    .into_iter()
                    .filter(|c| !matches!(c, Change::Param { id, .. } if a.target.skip.contains(id)))
                    .collect();
                if changes.is_empty() {
                    return Poll::Done;
                }
                let (now_ms, _) = now(world);
                let mut waits = Vec::new();
                for c in &changes {
                    match c {
                        Change::Slot { id, new, .. } => {
                            let requested = {
                                let slots = world.res::<ShaderSlots>();
                                let name = slots.get(*id).map(|d| d.signature.name);
                                name.and_then(|n| {
                                    world.res_mut::<SlotRebuilds>().request(&slots, n, new, now_ms).ok()
                                })
                            };
                            match requested {
                                Some(handle) => waits.push(SlotWait {
                                    handle,
                                    id: *id,
                                    new: new.clone(),
                                    result: None,
                                }),
                                // No rebuild path: swap the body in directly.
                                None => {
                                    if let Err(e) = c.apply(world, true) {
                                        return Poll::Fail(e);
                                    }
                                }
                            }
                        }
                        other => {
                            if let Err(e) = other.apply(world, true) {
                                return Poll::Fail(e);
                            }
                        }
                    }
                }
                a.phase = if waits.is_empty() {
                    ApplyPhase::Settle(SETTLE_FRAMES)
                } else {
                    ApplyPhase::Slots {
                        waits,
                        frames: 0,
                        started_ms: now_ms,
                    }
                };
                return Poll::Wait;
            }
            ApplyPhase::Slots {
                waits,
                frames,
                started_ms,
            } => {
                *frames += 1;
                let (now_ms, _) = now(world);
                {
                    let mut rb = world.res_mut::<SlotRebuilds>();
                    if *frames >= NO_RENDERER_FRAMES && rb.has_pending() {
                        // No renderer takes slot rebuilds here: complete them.
                        for r in rb.take_pending() {
                            rb.complete(
                                r.handle,
                                SlotCompileResult {
                                    ok: true,
                                    ms: (now_ms - r.requested_ms).max(0.0),
                                    kept: None,
                                    messages: Vec::new(),
                                    pipelines_rebuilt: Vec::new(),
                                },
                            );
                        }
                    }
                    for w in waits.iter_mut().filter(|w| w.result.is_none()) {
                        w.result = rb.take_result(w.handle);
                    }
                }
                let timed_out = now_ms - *started_ms > SLOT_TIMEOUT_MS;
                if waits.iter().any(|w| w.result.is_none()) && !timed_out {
                    return Poll::Wait;
                }
                // Whatever the renderer did, the body must now be the target's
                // (it compiled before, so a failure here is a renderer quirk).
                let mut ss = world.res_mut::<ShaderSlots>();
                for w in waits.iter() {
                    if ss.get(w.id).is_some_and(|d| d.body != w.new) {
                        let _ = ss.set_body(w.id, &w.new);
                    }
                }
                drop(ss);
                a.phase = ApplyPhase::Settle(SETTLE_FRAMES);
                return Poll::Wait;
            }
            ApplyPhase::Settle(n) => {
                if *n > 1 {
                    *n -= 1;
                    return Poll::Wait;
                }
                if a.pass == 1 {
                    // Second pass: put back what the settle reset (effect
                    // switches after a tier change).
                    a.pass = 2;
                    a.phase = ApplyPhase::Start;
                    continue;
                }
                return Poll::Done;
            }
        }
    }
}

fn poll_capture(world: &World, run: &mut Run, c: &mut CaptureStep) -> Poll {
    let (now_ms, _) = now(world);
    loop {
        match &mut c.phase {
            CapPhase::Setup => {
                apply_look(world, run);
                if let Some(setup) = c.moment {
                    if let Err(e) = setup(world) {
                        return Poll::Fail(LiveError::new("internal", format!("capture moment setup failed: {e}")));
                    }
                    // Hold the clock where the setup left the sim; the game
                    // camera frames it.
                    let t = world.try_res::<Time>().map_or(0.0, |t| t.render_seconds());
                    set_hold(world, t, None);
                    c.phase = CapPhase::Wait(MOMENT_DELAY_FRAMES);
                    return Poll::Wait;
                }
                if let Some(t) = c.t {
                    let cam = c.fixed_cam.or_else(|| c.path_cam.then(|| run.path.pose(t)));
                    set_hold(world, t, cam);
                    c.phase = CapPhase::Wait(CAPTURE_DELAY_FRAMES);
                    return Poll::Wait;
                }
                if run.look.any() {
                    // The sky, jitter and debug tint show from the next frame.
                    c.phase = CapPhase::Wait(CAPTURE_DELAY_FRAMES);
                    return Poll::Wait;
                }
                c.phase = CapPhase::Wait(0);
            }
            CapPhase::Wait(n) => {
                if *n > 1 {
                    *n -= 1;
                    return Poll::Wait;
                }
                let id = world
                    .res_mut::<CaptureQueue>()
                    .request(run.width, CaptureView::Current, true);
                c.phase = CapPhase::Pending {
                    id,
                    since_ms: now_ms,
                };
                return Poll::Wait;
            }
            CapPhase::Pending { id, since_ms } => {
                let got = world.res_mut::<CaptureQueue>().take_result(*id);
                let Some(r) = got else {
                    if now_ms - *since_ms > CAPTURE_TIMEOUT_MS {
                        return Poll::Fail(LiveError::new(
                            "timeout",
                            "the renderer did not deliver the capture within 5 s",
                        ));
                    }
                    return Poll::Wait;
                };
                return match shot_image(&r, run.width) {
                    Ok(img) => {
                        let thumb = c
                            .thumb
                            .and_then(|(w, h)| shot_image(&r, sweep::fit(r.width, r.height, w, h)).ok());
                        run.shots.push(Shot {
                            side: c.side,
                            t: c.t,
                            img,
                            frame: r.frame,
                            sim_time: c.t.unwrap_or(r.sim_time),
                            stats: r.stats,
                            camera: r.camera,
                            canvas_hud: r.canvas_hud,
                            reference: c.reference,
                            thumb,
                        });
                        Poll::Done
                    }
                    Err(e) => Poll::Fail(LiveError::new("internal", format!("capture failed: {e}"))),
                };
            }
        }
    }
}

/// Downscale a finished capture to `width`.
fn shot_image(r: &CaptureResult, width: u32) -> Result<Rgb, String> {
    let px = r.rgba.as_ref().map_err(Clone::clone)?;
    let need = r.width as usize * r.height as usize * 4;
    if r.width == 0 || r.height == 0 || px.len() < need {
        return Err(format!(
            "capture has {} bytes for {}x{}",
            px.len(),
            r.width,
            r.height
        ));
    }
    Ok(img::downscale_rgba(&px[..need], r.width, r.height, width))
}

fn poll_measure(world: &World, run: &mut Run, m: &mut MeasureStep) -> Poll {
    let (now_ms, _) = now(world);
    loop {
        match &mut m.phase {
            MeasPhase::Start => {
                set_hold(world, 0.0, Some(run.path.pose(0.0)));
                m.phase = MeasPhase::Warmup {
                    until_ms: now_ms + m.warmup_s * 1000.0,
                };
                return Poll::Wait;
            }
            MeasPhase::Warmup { until_ms } => {
                if now_ms < *until_ms {
                    return Poll::Wait;
                }
                let n = (m.seconds * 60.0).round().max(1.0) as u32;
                run.samples[m.side].reserve(n as usize + 2);
                m.phase = MeasPhase::Run {
                    k: 0,
                    n,
                    started_ms: now_ms,
                };
                m.next_progress_ms = now_ms;
            }
            MeasPhase::Run { k, n, started_ms } => {
                // The frame just rendered showed t_{k-1}; the first one is
                // skipped (it straddles the warm-up).
                if *k > 1
                    && let Some(s) = world.res::<LiveSession>().tel.latest()
                {
                    run.samples[m.side].push(*s);
                }
                let over_cap = now_ms - *started_ms > (3.0 * m.seconds + 5.0) * 1000.0;
                if *k > *n || over_cap {
                    run.truncated |= over_cap;
                    return Poll::Done;
                }
                let t = f64::from(*k) * STEP_S;
                set_hold(world, t, Some(run.path.pose(t)));
                *k += 1;
                if now_ms >= m.next_progress_ms {
                    m.next_progress_ms = now_ms + 1000.0;
                    let mut data = json!({ "run": run.id, "t": r2(t), "of": m.seconds });
                    if run.kind == RunKind::Compare {
                        data["side"] = json!(if m.side == 0 { "a" } else { "b" });
                    }
                    if run.kind == RunKind::Sweep {
                        data["row"] = json!(m.side);
                    }
                    let ev = json!({ "type": "event", "v": 1, "event": "bench.progress", "data": data });
                    world.res_mut::<LiveSession>().events.push(ev.to_string());
                }
                return Poll::Wait;
            }
        }
    }
}

// ---- results ------------------------------------------------------------------------

/// Nearest-rank percentile of a sorted slice (as apps/bench).
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0 * sorted.len() as f32).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Frame statistics of a set of samples.
#[derive(Debug, Clone, Default)]
struct Stats {
    frames: usize,
    avg: f32,
    p50: f32,
    p95: f32,
    p99: f32,
    max: f32,
    cpu_avg: f32,
    gpu_total: Option<f32>,
    /// `(name, avg, p95)` per pass seen.
    passes: Vec<(&'static str, f32, f32)>,
    draw_calls: u32,
    triangles: u32,
    sprites: u32,
    budget_used: u32,
    budget_total: u32,
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        0.0
    } else {
        (v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64) as f32
    }
}

fn stats(samples: &[Sample], names: &[&'static str]) -> Stats {
    let mut ft: Vec<f32> = samples.iter().map(|s| s.frame_ms).collect();
    ft.sort_by(f32::total_cmp);
    let gpu: Vec<f32> = samples
        .iter()
        .map(|s| s.gpu_total)
        .filter(|g| !g.is_nan())
        .collect();
    let mut passes = Vec::new();
    for (i, name) in names.iter().enumerate().take(MAX_PASSES) {
        let mut v: Vec<f32> = samples
            .iter()
            .map(|s| s.passes[i])
            .filter(|g| !g.is_nan())
            .collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(f32::total_cmp);
        passes.push((*name, mean(&v), percentile(&v, 95.0)));
    }
    let cpu: Vec<f32> = samples.iter().map(|s| s.cpu_ms).collect();
    Stats {
        frames: samples.len(),
        avg: mean(&ft),
        p50: percentile(&ft, 50.0),
        p95: percentile(&ft, 95.0),
        p99: percentile(&ft, 99.0),
        max: ft.last().copied().unwrap_or(0.0),
        cpu_avg: mean(&cpu),
        gpu_total: (!gpu.is_empty()).then(|| mean(&gpu)),
        passes,
        draw_calls: samples.iter().map(|s| s.draw_calls).max().unwrap_or(0),
        triangles: samples.iter().map(|s| s.triangles).max().unwrap_or(0),
        sprites: samples.iter().map(|s| s.sprites).max().unwrap_or(0),
        budget_used: samples.iter().map(|s| s.budget_used).max().unwrap_or(0),
        budget_total: samples.last().map_or(0, |s| s.budget_total),
    }
}

fn frame_ms_json(s: &Stats) -> Value {
    if s.frames == 0 {
        return Value::Null;
    }
    json!({ "avg": round2(s.avg), "p50": round2(s.p50), "p95": round2(s.p95),
            "p99": round2(s.p99), "max": round2(s.max) })
}

/// `gpu_ms`: per pass `{avg, p95}` (bench) or the average only (compare).
fn gpu_json(s: &Stats, with_p95: bool) -> Value {
    let Some(total) = s.gpu_total else {
        return Value::Null;
    };
    let mut passes = Map::new();
    for (name, avg, p95) in &s.passes {
        let v = if with_p95 {
            json!({ "avg": round2(*avg), "p95": round2(*p95) })
        } else {
            json!(round2(*avg))
        };
        passes.insert((*name).to_string(), v);
    }
    json!({ "avg_total": round2(total), "passes": passes })
}

/// A broken tier limit (6.15 `failures[]`).
fn failure(limit: &str, max: f64, seen: f64) -> Value {
    json!({ "limit": limit, "max": r2(max), "seen": r2(seen) })
}

/// The bench verdict against a tier's limits. The same rules as apps/bench's
/// `evaluate` (copied, so live never depends on the bench app): average
/// frame time within the target +10%, p99 under the fps floor, draw calls,
/// triangles, sprites and budget points within the preset's limits.
fn bench_failures(world: &World, tier: &str, s: &Stats) -> Vec<Value> {
    const FRAME_TOLERANCE_PCT: f32 = 10.0;
    let tiers = world.res::<Tiers>();
    let Some(p) = tiers.get(tier) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if s.frames == 0 {
        out.push(failure("frames", 1.0, 0.0));
        return out;
    }
    let allowed = p.frame_budget_ms() * (1.0 + FRAME_TOLERANCE_PCT / 100.0);
    if s.avg > allowed {
        out.push(failure("frame_ms_avg", f64::from(allowed), f64::from(s.avg)));
    }
    if let Some(floor) = p.fps_floor.map(|f| 1000.0 / f.max(1) as f32)
        && s.p99 > floor
    {
        out.push(failure("frame_ms_p99", f64::from(floor), f64::from(s.p99)));
    }
    let counts = [
        ("draw_calls", p.max_draw_calls, s.draw_calls),
        ("triangles", p.max_triangles, s.triangles),
        ("sprites", p.max_sprites, s.sprites),
        ("points", p.points, s.budget_used),
    ];
    for (name, max, seen) in counts {
        if seen > max {
            out.push(failure(name, f64::from(max), f64::from(seen)));
        }
    }
    out
}

fn diff_json(d: Option<img::Diff>) -> Value {
    d.map_or(Value::Null, |d| {
        json!({ "mean_abs": r3(d.mean_abs), "rmse": r3(d.rmse), "max": d.max,
                "changed_pct": r2(d.changed_pct), "psnr_db": d.psnr_db.map(r2) })
    })
}

fn png_b64(img: &Rgb) -> String {
    img::base64(&img::encode_png(img))
}

/// The camera a frame was drawn with, for `capture.frame`'s reply.
fn pose_json(c: &CameraPose) -> Value {
    json!({ "eye": c.eye, "target": c.target, "up": c.up, "fov_y_deg": c.fov_y_deg })
}

fn stats_render(stats: &RenderStats, frame_ms: Option<f32>, tier: &str) -> Value {
    let gpu = stats.gpu_ms.map_or(Value::Null, |g| {
        let mut passes = Map::new();
        for p in &stats.passes {
            passes.insert(p.name.to_string(), json!(round2(p.gpu_ms)));
        }
        json!({ "avg_total": round2(g), "passes": passes })
    });
    json!({ "frame_ms": frame_ms.map(round2), "gpu_ms": gpu, "draw_calls": stats.draw_calls,
            "triangles": stats.triangles, "sprites": stats.sprites, "tier": tier })
}

/// A change result's `capture` (5.4 with `opts.capture`).
pub(crate) fn capture_json(world: &World, r: &CaptureResult, width: u32) -> Value {
    match shot_image(r, width) {
        Ok(img) => {
            let tier = world.res::<Tiers>().active().id.clone();
            json!({ "png_base64": png_b64(&img), "width": img.w, "height": img.h,
                    "frame": r.frame, "sim_time": r2(r.sim_time),
                    "stats": stats_render(&r.stats, None, &tier) })
        }
        Err(e) => json!({ "error": e }),
    }
}

fn finish(world: &World, run: Run) {
    restore_look(world, &run);
    if run.hold_after && run.error.is_none() {
        // The clock and camera stay where the shot was taken; the next run
        // puts back what was there before this one.
        world.res_mut::<LiveSession>().kept_hold = Some((run.prev_camera, run.prev_sim));
    } else if let Some(mut q) = world.try_res_mut::<CaptureQueue>() {
        q.camera_override = run.prev_camera;
        q.sim_time_override = run.prev_sim;
    }
    let res = match run.error.clone() {
        Some(e) => Err(e),
        None => Ok(result(world, &run)),
    };
    commands::send_reply(world, run.token, &run.req_id, res);
}

fn result(world: &World, run: &Run) -> Value {
    let names: Vec<&'static str> = world.res::<LiveSession>().tel.pass_names().to_vec();
    let gpu_timing = |s: &Stats| {
        if s.gpu_total.is_some() {
            "timestamp"
        } else {
            "unavailable"
        }
    };
    match &run.info {
        Info::Capture { sim_time, moment, temp } => {
            let Some(shot) = run.shots.first() else {
                return json!({ "run": run.id });
            };
            let frame_ms = world.res::<LiveSession>().tel.latest().map(|s| s.frame_ms);
            // The tier the shot was taken at (S0's is back by now).
            let tier = match temp {
                Some(t) => t["tier"].as_str().unwrap_or_default().to_string(),
                None => world.res::<Tiers>().active().id.clone(),
            };
            let mut v = json!({ "run": run.id, "png_base64": png_b64(&shot.img), "width": shot.img.w,
                    "height": shot.img.h, "frame": shot.frame,
                    "sim_time": sim_time.map_or_else(|| r2(shot.sim_time), r2),
                    "stats": stats_render(&shot.stats, frame_ms, &tier) });
            if let Some((name, label)) = moment {
                v["moment"] = json!({ "name": name, "label": label });
            }
            if let Some(t) = temp {
                v["tier"] = t["tier"].clone();
                v["values"] = t["values"].clone();
            }
            if let Some(c) = shot.camera {
                v["camera"] = pose_json(&c);
            }
            if let Some(h) = shot.canvas_hud {
                v["canvas_hud"] = json!({ "sprites": h.sprites, "rect_css": h.rect_css, "viewport_css": h.viewport_css });
            }
            if run.look.sky.is_some() {
                v["sky"] = json!(run.look.sky);
            }
            if run.look.jitter_px.is_some() {
                v["jitter_px"] = json!(run.look.jitter_px);
            }
            if run.look.geometry
                && let Some(g) = world.try_res::<crate::geometry::GeometryView>()
            {
                v["geometry"] = json!({ "counts": g.counts(), "total": g.issues.len(), "frame": g.analysed_frame });
            }
            v
        }
        Info::Bench {
            tier,
            seconds,
            warmup_s,
        } => {
            let s = stats(&run.samples[0], &names);
            let failures = bench_failures(world, tier, &s);
            let captures: Vec<Value> = run
                .shots
                .iter()
                .map(|c| json!({ "t": c.t, "png_base64": png_b64(&c.img), "width": c.img.w,
                                  "height": c.img.h, "frame": c.frame }))
                .collect();
            json!({
                "run": run.id, "tier": tier, "seconds": seconds, "warmup_s": warmup_s,
                "frames": s.frames, "truncated": run.truncated,
                "frame_ms": frame_ms_json(&s), "cpu_ms": { "avg": round2(s.cpu_avg) },
                "gpu_ms": gpu_json(&s, true), "gpu_timing": gpu_timing(&s),
                "draw_calls": { "max": s.draw_calls }, "triangles": { "max": s.triangles },
                "sprites": { "max": s.sprites },
                "budget": { "used": s.budget_used, "limit": s.budget_total },
                "pass": failures.is_empty(), "failures": failures, "captures": captures,
            })
        }
        Info::Compare {
            sides,
            tier,
            seconds,
            warmup_s,
            order,
            keep,
            capture_at,
        } => {
            let st = [stats(&run.samples[0], &names), stats(&run.samples[1], &names)];
            let side = |i: usize| {
                let s = &st[i];
                let mut v = json!({
                    "frames": s.frames, "frame_ms": frame_ms_json(s),
                    "cpu_ms": { "avg": round2(s.cpu_avg) }, "gpu_ms": gpu_json(s, false),
                    "draw_calls": { "max": s.draw_calls }, "triangles": { "max": s.triangles },
                    "sprites": { "max": s.sprites },
                });
                match &sides[i] {
                    Value::Object(o) => {
                        v["snapshot"] = Value::Null;
                        v["changes"] = o.get("changes").cloned().unwrap_or(Value::Null);
                    }
                    label => v["snapshot"] = label.clone(),
                }
                v
            };
            let mut pass_delta = Map::new();
            let mut pass_names: Vec<&'static str> = Vec::new();
            for s in &st {
                for (n, _, _) in &s.passes {
                    if !pass_names.contains(n) {
                        pass_names.push(n);
                    }
                }
            }
            let pass_avg = |s: &Stats, n: &str| {
                s.passes
                    .iter()
                    .find(|(m, _, _)| *m == n)
                    .map_or(0.0, |(_, a, _)| *a)
            };
            for n in pass_names {
                pass_delta.insert(
                    n.to_string(),
                    json!(round2(pass_avg(&st[1], n) - pass_avg(&st[0], n))),
                );
            }
            let gpu_delta = match (st[0].gpu_total, st[1].gpu_total) {
                (Some(a), Some(b)) => json!(round2(b - a)),
                _ => Value::Null,
            };
            // Pair captures by time; the overall diff covers every pair.
            let mut captures = Vec::new();
            let (mut all_a, mut all_b) = (Vec::new(), Vec::new());
            let (mut w, mut h) = (0u32, 0u32);
            for t in capture_at {
                let find = |side: usize| {
                    run.shots
                        .iter()
                        .find(|c| c.side == side && c.t == Some(*t))
                };
                let (Some(a), Some(b)) = (find(0), find(1)) else {
                    continue;
                };
                let d = img::diff(&a.img, &b.img);
                if d.is_some() && (w == 0 || w == a.img.w) {
                    w = a.img.w;
                    h += a.img.h;
                    all_a.extend_from_slice(&a.img.px);
                    all_b.extend_from_slice(&b.img.px);
                }
                captures.push(json!({
                    "t": t, "a_png_base64": png_b64(&a.img), "b_png_base64": png_b64(&b.img),
                    "width": a.img.w, "height": a.img.h, "diff": diff_json(d),
                }));
            }
            let overall = if h > 0 {
                img::diff(
                    &Rgb { w, h, px: all_a },
                    &Rgb { w, h, px: all_b },
                )
            } else {
                None
            };
            let kept = if *keep { keep_tx(world, run) } else { Value::Null };
            let both = |f: fn(&Stats) -> f32| r2(f64::from(f(&st[1])) - f64::from(f(&st[0])));
            let timing = if st[0].gpu_total.is_some() && st[1].gpu_total.is_some() {
                "timestamp"
            } else {
                "unavailable"
            };
            json!({
                "run": run.id, "tier": tier, "seconds": seconds, "warmup_s": warmup_s,
                "order": order, "truncated": run.truncated,
                "a": side(0), "b": side(1),
                "delta": { "frame_ms_avg": both(|s| s.avg), "frame_ms_p95": both(|s| s.p95),
                           "gpu_ms_total": gpu_delta, "passes": pass_delta },
                "gpu_timing": timing,
                "captures": captures, "diff": diff_json(overall), "kept": kept,
            })
        }
        Info::Sweep(info) => sweep::result(run, info, &names),
    }
}

/// `keep: true`: record S0 → now (the real B) as one `compare.ab`
/// transaction. `null` when B equals S0.
fn keep_tx(world: &World, run: &Run) -> Value {
    let changes = run.s0.diff(&FullState::capture(world));
    if changes.is_empty() {
        return Value::Null;
    }
    let (_, frame) = now(world);
    let summary = match changes.as_slice() {
        [one] => one.summary(world),
        many => format!("{} changes", many.len()),
    };
    let applied: Vec<Value> = changes.iter().map(|c| c.describe(world, true)).collect();
    let tx = {
        let mut s = world.res_mut::<LiveSession>();
        s.next_tx += 1;
        let tx = s.next_tx;
        s.push(Tx {
            tx,
            cmd: "compare.ab".to_string(),
            note: run.keep_note.clone(),
            source: run
                .req
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("page")
                .to_string(),
            at: now_iso(),
            frame,
            changes,
            summary,
            status: TxStatus::Applied,
            revert_reason: None,
            edit: None,
        });
        tx
    };
    crate::revert::push_mirror(world);
    json!({ "tx": tx, "applied": applied })
}
