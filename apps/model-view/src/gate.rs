//! The quality gate's renderer (STAGE3_PLAN S3-6; dev only, feature `gate`,
//! browser only). The converter (`cargo xtask convert --gate`) opens this
//! page through tools/convert-runner/run.mjs as
//! `/?gate=1&tier=<id>&model=<url>&place=[x,y,z,s]&views=[pose,..]` and reads
//! back one RGBA picture per camera pose.
//!
//! * One model, drawn by the engine's own renderer (`models`) in the toon
//!   look with its compiled effects, nothing else in the scene. No kit.
//! * A fixed clock: the capture queue holds the sim time at 0, adaptive
//!   quality is off and the tier is the one asked for (VR as its mono
//!   preset, the bench's preset run), re-applied every frame so the first-run
//!   warm-up can never switch it.
//! * Each pose is held for a few frames, then captured (`capture` feature:
//!   the colour target at the tier's render size, read back off the frame).
//! * `window.__gate` = `{ done, error, progress, w, h, n }`;
//!   `window.__gateShots` = one `Uint8Array` (RGBA8, top row first) per pose.
//!
//! `anim` (S3-7 to S3-9) joins later: the page will then also hold a clip at
//! a fixed time; today it only turns on what exists.

use evani_core::App;
use evani_core::camera::CameraPose;
use evani_core::capture::{CaptureQueue, CaptureView};
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::Overlay;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::tiers::Tiers;
use evani_look_toon::ToonLook;
use evani_render::model::{ModelId, ModelInstance, ModelState, Models};
use evani_render::{RenderModule, RenderWorld};
use wasm_bindgen::JsValue;

/// Frames to wait once the model is on the GPU (pipelines, shadow maps).
const SETTLE_FRAMES: u32 = 10;
/// Frames each pose is held before its capture.
const HOLD_FRAMES: u32 = 3;
/// Give up when WebGPU or the model takes longer than this (ms).
const WAIT_MS: f64 = 60_000.0;

/// Is this page address a gate run (`?gate=1`)?
pub fn requested() -> bool {
    query("gate").as_deref() == Some("1")
}

fn query(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get(key)
}

/// What the page was asked to render.
struct Job {
    tier: String,
    model: String,
    place: [f32; 4],
    views: Vec<CameraPose>,
}

fn f32s(v: &serde_json::Value, n: usize, what: &str) -> Result<Vec<f32>, String> {
    let a = v.as_array().filter(|a| a.len() == n).ok_or_else(|| format!("{what}: {n} numbers expected"))?;
    a.iter().map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| format!("{what}: not a number"))).collect()
}

fn three(v: &serde_json::Value, what: &str) -> Result<[f32; 3], String> {
    let a = f32s(v, 3, what)?;
    Ok([a[0], a[1], a[2]])
}

fn parse_job() -> Result<Job, String> {
    let tier = query("tier").filter(|t| !t.is_empty()).ok_or("no tier= in the page address")?;
    let model = query("model").filter(|m| !m.is_empty()).ok_or("no model= in the page address")?;
    let place = match query("place") {
        Some(p) => {
            let v: serde_json::Value = serde_json::from_str(&p).map_err(|e| format!("place: {e}"))?;
            let a = f32s(&v, 4, "place")?;
            [a[0], a[1], a[2], a[3]]
        }
        None => [0.0, 0.0, 0.0, 1.0],
    };
    let raw = query("views").ok_or("no views= in the page address")?;
    let list: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("views: {e}"))?;
    let mut views = Vec::new();
    for v in list.as_array().into_iter().flatten() {
        views.push(CameraPose {
            eye: three(&v["eye"], "views[].eye")?,
            target: three(&v["target"], "views[].target")?,
            up: three(&v["up"], "views[].up")?,
            fov_y_deg: v["fov_y_deg"].as_f64().ok_or("views[].fov_y_deg: not a number")? as f32,
        });
    }
    if views.is_empty() {
        return Err("views: no camera poses".into());
    }
    Ok(Job { tier, model, place, views })
}

enum Phase {
    Start,
    WaitGpu { since_ms: f64 },
    Loading { id: ModelId, since_ms: f64 },
    Settle { id: ModelId, frames: u32 },
    Hold { id: ModelId, view: usize, frames: u32 },
    Capture { id: ModelId, view: usize, request: u64 },
    Done,
}

struct Gate {
    job: Option<Job>,
    phase: Phase,
    shots: js_sys::Array,
    size: [u32; 2],
}

/// The gate page: renderer, compiled effects and the toon look, plus the
/// driver system. No kit and no live module.
pub fn build() -> App {
    // Render first: fx adds its passes to the render graph render inserts.
    #[allow(unused_mut)]
    let mut app = App::new("model-view-gate").with_module(RenderModule::new());
    #[cfg(feature = "fx")]
    {
        app = app.with_module(evani_fx::FxModule::compiled());
    }
    let shots = js_sys::Array::new();
    set_global("__gateShots", &shots);
    publish(false, None, "starting", [0, 0], 0);
    let mut gate = Gate { job: None, phase: Phase::Start, shots, size: [0, 0] };
    app.with_look(ToonLook).with_system(
        SystemDesc::new("model_view.gate", Stage::Frame, move |world: &mut World| {
            if let Err(e) = step(world, &mut gate) {
                gate.phase = Phase::Done;
                publish(false, Some(&e), "failed", gate.size, 0);
            }
        })
        .order(-10),
    )
}

fn step(world: &mut World, g: &mut Gate) -> Result<(), String> {
    let now_ms = world.res::<evani_core::time::Time>().now_ms;
    if let Some(job) = &g.job {
        // Re-applied every frame: the first-run tier warm-up stops as soon
        // as it sees a tier it did not set.
        let mut tiers = world.res_mut::<Tiers>();
        if tiers.active().id != job.tier && !tiers.set_active_preset_run(&job.tier) {
            return Err(format!("unknown tier '{}'", job.tier));
        }
    }
    match g.phase {
        Phase::Start => {
            let job = parse_job()?;
            let mut queue = CaptureQueue::default();
            queue.sim_time_override = Some(0.0);
            world.insert_resource(queue);
            world.res_mut::<Overlay>().enabled = false;
            {
                let mut params = world.res_mut::<ParamRegistry>();
                if let Some(id) = params.id("aq.enabled") {
                    params.set(id, ParamValue::Bool(false)).map_err(|e| format!("aq.enabled: {e:?}"))?;
                }
            }
            g.job = Some(job);
            g.phase = Phase::WaitGpu { since_ms: now_ms };
            publish(false, None, "waiting for WebGPU", g.size, 0);
        }
        Phase::WaitGpu { since_ms } => {
            let live = world.res::<RenderWorld>().viewport_css.iter().all(|v| *v > 0.0);
            if !live {
                if now_ms - since_ms > WAIT_MS {
                    return Err("WebGPU did not come up within 60 s".into());
                }
                return Ok(());
            }
            let url = g.job.as_ref().map(|j| j.model.clone()).unwrap_or_default();
            let id = world.res_mut::<Models>().load(&url);
            g.phase = Phase::Loading { id, since_ms: now_ms };
            publish(false, None, "loading the model", g.size, 0);
        }
        Phase::Loading { id, since_ms } => {
            match world.res::<Models>().state(id) {
                Some(ModelState::Ready) => g.phase = Phase::Settle { id, frames: 0 },
                Some(ModelState::Failed(why)) => return Err(format!("the model did not load: {why}")),
                _ if now_ms - since_ms > WAIT_MS => return Err("the model did not load within 60 s".into()),
                _ => {}
            }
            draw(world, g, id);
        }
        Phase::Settle { id, frames } => {
            draw(world, g, id);
            pose(world, g, 0);
            g.phase = if frames + 1 >= SETTLE_FRAMES { Phase::Hold { id, view: 0, frames: 0 } } else { Phase::Settle { id, frames: frames + 1 } };
        }
        Phase::Hold { id, view, frames } => {
            draw(world, g, id);
            pose(world, g, view);
            if frames + 1 >= HOLD_FRAMES {
                let request = world.res_mut::<CaptureQueue>().request(0, CaptureView::Current, true);
                g.phase = Phase::Capture { id, view, request };
            } else {
                g.phase = Phase::Hold { id, view, frames: frames + 1 };
            }
        }
        Phase::Capture { id, view, request } => {
            draw(world, g, id);
            pose(world, g, view);
            let Some(r) = world.res_mut::<CaptureQueue>().take_result(request) else { return Ok(()) };
            let rgba = r.rgba.map_err(|e| format!("view {view}: the capture failed: {e}"))?;
            if view > 0 && [r.width, r.height] != g.size {
                return Err(format!("view {view}: the render size changed ({}x{} after {}x{})", r.width, r.height, g.size[0], g.size[1]));
            }
            g.size = [r.width, r.height];
            g.shots.push(&js_sys::Uint8Array::from(rgba.as_slice()));
            let n = g.job.as_ref().map_or(0, |j| j.views.len());
            if view + 1 >= n {
                g.phase = Phase::Done;
                world.res_mut::<CaptureQueue>().camera_override = None;
                publish(true, None, "done", g.size, n as u32);
            } else {
                g.phase = Phase::Hold { id, view: view + 1, frames: 0 };
                publish(false, None, &format!("view {} of {n}", view + 2), g.size, (view + 1) as u32);
            }
        }
        Phase::Done => {}
    }
    Ok(())
}

/// The model this frame, placed so it fits the poses (`place`).
fn draw(world: &World, g: &Gate, id: ModelId) {
    let Some(job) = &g.job else { return };
    let [x, y, z, s] = job.place;
    world.res_mut::<Models>().draw(ModelInstance::new(id, [x, y, z]).scale([s, s, s]));
}

/// Look through pose `view`.
fn pose(world: &World, g: &Gate, view: usize) {
    let Some(p) = g.job.as_ref().and_then(|j| j.views.get(view)).copied() else { return };
    world.res_mut::<CaptureQueue>().camera_override = Some(p);
    world.res_mut::<RenderWorld>().camera = p;
}

fn set_global(name: &str, v: &JsValue) {
    if let Some(w) = web_sys::window() {
        let _ = js_sys::Reflect::set(&w, &JsValue::from_str(name), v);
    }
}

/// `window.__gate`, read by the runner.
fn publish(done: bool, error: Option<&str>, progress: &str, size: [u32; 2], n: u32) {
    let o = js_sys::Object::new();
    let put = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &v);
    };
    put("done", JsValue::from_bool(done));
    put("error", error.map_or(JsValue::NULL, JsValue::from_str));
    put("progress", JsValue::from_str(progress));
    put("w", JsValue::from_f64(f64::from(size[0])));
    put("h", JsValue::from_f64(f64::from(size[1])));
    put("n", JsValue::from_f64(f64::from(n)));
    set_global("__gate", &o);
    if let Some(e) = error {
        web_sys::console::error_1(&JsValue::from_str(&format!("model-view gate: {e}")));
    }
}
