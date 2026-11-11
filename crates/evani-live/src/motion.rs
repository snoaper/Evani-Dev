//! Motion trace (`motion.trace`, `motion.read`, `motion.stop`; docs/LIVE_API.md
//! 6.32): where one chosen thing was drawn on every frame, for the smoothness
//! harness (`cargo xtask smoothness`, docs/SMOOTHNESS.md).
//!
//! Kits push their draws into `RenderWorld` every frame and the renderer
//! clears it after drawing, so a drawn position exists only inside its frame.
//! [`frame`] runs at the end of `Stage::Frame` (after every kit has drawn,
//! before the geometry view tints anything) and records the probe's world
//! position and its projection on the canvas into a preallocated buffer.
//!
//! Live only: this module is part of evani-live, which shipped games never
//! contain, so a shipped game's download is unchanged by it.
//!
//! The probe is genre-agnostic: the game's smoothness config names it
//! (`tools/smoothness/README.md`), never the engine.
//! - `camera_target` / `camera_eye`: the main camera (a chase camera's target
//!   is usually the followed entity's drawn position);
//! - `mesh`: the mesh instance matching `mesh` (id) and/or `material` (look
//!   material index) nearest to the camera target (or to `near`);
//! - `sprite`: the `index`-th sprite (default 0) matching `cell` and/or `layer`.

use evani_core::ecs::World;
use evani_core::time::Time;
use evani_geocheck::image::Camera;
use evani_render::world::RenderWorld;
use serde_json::{Value, json};

use crate::LiveError;

/// Default buffer: ten minutes at 60 frames a second.
pub const DEFAULT_CAPACITY: usize = 36_000;
/// Largest buffer a trace may ask for.
pub const MAX_CAPACITY: usize = 216_000;
/// Most samples one `motion.read` answers (keeps a reply under ~200 KB).
pub const MAX_READ: usize = 4_000;

/// What the trace follows (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    CameraTarget,
    CameraEye,
    Mesh { mesh: Option<u16>, material: Option<u32>, near: Option<[f32; 3]> },
    Sprite { cell: Option<u16>, layer: Option<i16>, index: usize },
}

impl Probe {
    /// Parse `params.probe` (`{ "kind": "camera_target" }`, ...).
    pub fn parse(v: &Value) -> Result<Probe, LiveError> {
        let bad = |m: &str| LiveError::new("invalid_value", m.to_string());
        let kind = v.get("kind").and_then(Value::as_str).unwrap_or("camera_target");
        let u = |k: &str| v.get(k).and_then(Value::as_u64);
        match kind {
            "camera_target" => Ok(Probe::CameraTarget),
            "camera_eye" => Ok(Probe::CameraEye),
            "mesh" => {
                let mesh = u("mesh").map(|m| u16::try_from(m).map_err(|_| bad("probe.mesh is too large"))).transpose()?;
                let material = u("material").map(|m| u32::try_from(m).map_err(|_| bad("probe.material is too large"))).transpose()?;
                let near = match v.get("near") {
                    None | Some(Value::Null) => None,
                    Some(n) => Some(vec3(n).ok_or_else(|| bad("probe.near must be [x, y, z]"))?),
                };
                if mesh.is_none() && material.is_none() {
                    return Err(bad("a mesh probe needs probe.mesh and/or probe.material"));
                }
                Ok(Probe::Mesh { mesh, material, near })
            }
            "sprite" => {
                let cell = u("cell").map(|c| u16::try_from(c).map_err(|_| bad("probe.cell is too large"))).transpose()?;
                let layer = match v.get("layer").and_then(Value::as_i64) {
                    Some(l) => Some(i16::try_from(l).map_err(|_| bad("probe.layer is out of range"))?),
                    None => None,
                };
                let index = u("index").unwrap_or(0) as usize;
                Ok(Probe::Sprite { cell, layer, index })
            }
            k => Err(bad(&format!("unknown probe kind {k} (camera_target, camera_eye, mesh, sprite)"))),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Probe::CameraTarget => "camera_target",
            Probe::CameraEye => "camera_eye",
            Probe::Mesh { .. } => "mesh",
            Probe::Sprite { .. } => "sprite",
        }
    }
}

fn vec3(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let f = |i: usize| a[i].as_f64().map(|x| x as f32);
    Some([f(0)?, f(1)?, f(2)?])
}

/// One drawn frame: page time (ms, `performance.now()`), frame number, the
/// probe's world position (sprites have none) and where it lands on the
/// canvas in CSS px (none when behind the camera or not drawn).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t_ms: f64,
    pub frame: u64,
    pub world: Option<[f32; 3]>,
    pub screen: Option<[f32; 2]>,
}

/// The probe's position in this frame's draws.
pub fn sample(rw: &RenderWorld, probe: &Probe) -> (Option<[f32; 3]>, Option<[f32; 2]>) {
    let world = match probe {
        Probe::CameraTarget => Some(rw.camera.target),
        Probe::CameraEye => Some(rw.camera.eye),
        Probe::Mesh { mesh, material, near } => {
            let at = near.unwrap_or(rw.camera.target);
            let d2 = |p: [f32; 3]| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2);
            rw.meshes
                .iter()
                .filter(|m| mesh.is_none_or(|id| m.mesh.0 == id) && material.is_none_or(|k| m.material == k))
                .map(|m| m.position)
                .min_by(|a, b| d2(*a).total_cmp(&d2(*b)))
        }
        Probe::Sprite { cell, layer, index } => {
            let s = rw
                .sprites
                .iter()
                .filter(|s| cell.is_none_or(|c| s.cell == c) && layer.is_none_or(|l| s.layer == l))
                .nth(*index);
            return (None, s.map(|s| s.pos));
        }
    };
    let screen = world.and_then(|p| project(rw, p));
    (world, screen)
}

/// `p` on the canvas in CSS px through the main camera (None behind it or
/// before the renderer has a size).
pub fn project(rw: &RenderWorld, p: [f32; 3]) -> Option<[f32; 2]> {
    let [w, h] = rw.viewport_css;
    if w < 1.0 || h < 1.0 {
        return None;
    }
    let cam = Camera::from_pose(&rw.camera, f64::from(w) / f64::from(h));
    let [x, y] = cam.project(w as u32, h as u32, [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])?;
    Some([x as f32, y as f32])
}

/// World resource: the running trace (inserted by [`crate::LiveModule`]).
#[derive(Debug, Default)]
pub struct MotionTrace {
    pub probe: Option<Probe>,
    pub samples: Vec<Sample>,
    pub capacity: usize,
    /// Samples dropped because the buffer was full (the trace keeps the first).
    pub dropped: u64,
    /// Frames the probe was not drawn in.
    pub missing: u64,
}

impl MotionTrace {
    pub fn on(&self) -> bool {
        self.probe.is_some()
    }

    pub fn record(&mut self, rw: &RenderWorld, t_ms: f64, frame: u64) {
        let Some(probe) = &self.probe else { return };
        let (world, screen) = sample(rw, probe);
        if world.is_none() && screen.is_none() {
            self.missing += 1;
        }
        if self.samples.len() >= self.capacity {
            self.dropped += 1;
            return;
        }
        self.samples.push(Sample { t_ms, frame, world, screen });
    }
}

/// Frame system (`live.motion`, at the end of `Stage::Frame`).
pub fn frame(world: &mut World) {
    let Some(mut tr) = world.try_res_mut::<MotionTrace>() else { return };
    if !tr.on() {
        return;
    }
    let (t, f) = world.try_res::<Time>().map_or((0.0, 0), |t| (t.now_ms, t.frame));
    if let Some(rw) = world.try_res::<RenderWorld>() {
        tr.record(&rw, t, f);
    }
}

/// `motion.trace`: start (or restart) a trace with `params.probe`.
pub fn start(world: &World, params: &Value) -> Result<Value, LiveError> {
    if world.try_res::<RenderWorld>().is_none() {
        return Err(LiveError::new("unavailable", "this build has no renderer to trace"));
    }
    let probe = Probe::parse(params.get("probe").unwrap_or(&Value::Null))?;
    let capacity = match params.get("capacity").and_then(Value::as_u64) {
        Some(c) if (1..=MAX_CAPACITY as u64).contains(&c) => c as usize,
        Some(_) => return Err(LiveError::new("invalid_value", format!("capacity must be 1..{MAX_CAPACITY}"))),
        None => DEFAULT_CAPACITY,
    };
    let Some(mut tr) = world.try_res_mut::<MotionTrace>() else {
        return Err(LiveError::new("unavailable", "the motion trace is not set up"));
    };
    let kind = probe.kind();
    *tr = MotionTrace { probe: Some(probe), samples: Vec::with_capacity(capacity), capacity, dropped: 0, missing: 0 };
    Ok(json!({ "on": true, "probe": kind, "capacity": capacity }))
}

/// `motion.read`: samples from `params.since` (default 0), at most
/// `params.max` (default and cap [`MAX_READ`]), as rows
/// `[t_ms, frame, x, y, z, sx, sy]` (null where unknown).
pub fn read(world: &World, params: &Value) -> Result<Value, LiveError> {
    let Some(tr) = world.try_res::<MotionTrace>() else {
        return Err(LiveError::new("unavailable", "the motion trace is not set up"));
    };
    let since = params.get("since").and_then(Value::as_u64).unwrap_or(0) as usize;
    let max = params.get("max").and_then(Value::as_u64).map_or(MAX_READ, |m| (m as usize).clamp(1, MAX_READ));
    let from = since.min(tr.samples.len());
    let to = (from + max).min(tr.samples.len());
    let r = |v: f32| (f64::from(v) * 1000.0).round() / 1000.0;
    let rows: Vec<Value> = tr.samples[from..to]
        .iter()
        .map(|s| {
            let w = s.world.map_or([Value::Null, Value::Null, Value::Null], |p| [json!(r(p[0])), json!(r(p[1])), json!(r(p[2]))]);
            let sc = s.screen.map_or([Value::Null, Value::Null], |p| [json!(r(p[0])), json!(r(p[1]))]);
            json!([(s.t_ms * 1000.0).round() / 1000.0, s.frame, w[0], w[1], w[2], sc[0], sc[1]])
        })
        .collect();
    Ok(json!({
        "on": tr.on(),
        "probe": tr.probe.as_ref().map(Probe::kind),
        "from": from,
        "next": to,
        "total": tr.samples.len(),
        "dropped": tr.dropped,
        "missing": tr.missing,
        "columns": ["t_ms", "frame", "x", "y", "z", "sx", "sy"],
        "samples": rows,
    }))
}

/// `motion.stop`: stop recording (the samples stay readable until the next
/// `motion.trace`).
pub fn stop(world: &World) -> Result<Value, LiveError> {
    let Some(mut tr) = world.try_res_mut::<MotionTrace>() else {
        return Err(LiveError::new("unavailable", "the motion trace is not set up"));
    };
    tr.probe = None;
    Ok(json!({ "on": false, "total": tr.samples.len(), "dropped": tr.dropped }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LiveModule, submit, take_replies};
    use evani_core::App;
    use evani_core::camera::CameraPose;
    use evani_core::ecs::{Stage, SystemDesc};
    use evani_render::mesh::MeshId;
    use evani_render::world::{MeshInstance, Sprite};

    const DT: f64 = 16.0;

    /// A box moving +1 m along x per frame, the camera following 10 m behind,
    /// a sprite moving 2 px a frame.
    fn scene(world: &mut World) {
        let f = world.res::<Time>().frame as f32;
        let mut rw = world.res_mut::<RenderWorld>();
        rw.clear();
        rw.viewport_css = [1280.0, 720.0];
        let p = [f, 0.5, 0.0];
        rw.camera = CameraPose { eye: [f - 10.0, 3.0, 0.0], target: p, up: [0.0, 1.0, 0.0], fov_y_deg: 60.0 };
        let mut m = MeshInstance::new(MeshId::CUBE, p);
        m.material = 3;
        rw.draw_mesh(m);
        rw.draw_mesh(MeshInstance::new(MeshId::CUBE, [f + 50.0, 0.5, 0.0]));
        rw.draw_sprite(Sprite { pos: [2.0 * f, 100.0], size: [8.0; 2], rotation: 0.0, cell: 5, color: [1.0; 4], layer: 2 });
    }

    struct T {
        a: App,
        token: u64,
    }

    impl T {
        fn new() -> Self {
            let mut a = App::new("sandbox").with_module(LiveModule::new());
            a.world.insert_resource(RenderWorld::default());
            a = a.with_system(SystemDesc::new("test.draw", Stage::Frame, scene));
            a.run_headless(2, DT);
            Self { a, token: 0 }
        }

        fn call(&mut self, cmd: &str, params: Value) -> Value {
            self.token += 1;
            let req = json!({ "type": "cmd", "v": 1, "id": format!("m-{}", self.token), "cmd": cmd, "params": params });
            submit(&self.a.world, self.token, &req.to_string());
            for _ in 0..10 {
                self.a.run_headless(1, DT);
                if let Some((_, text)) = take_replies(&self.a.world).into_iter().find(|(t, _)| *t == self.token) {
                    return serde_json::from_str(&text).unwrap();
                }
            }
            panic!("no reply to {cmd}");
        }

        fn ok(&mut self, cmd: &str, params: Value) -> Value {
            let r = self.call(cmd, params);
            assert_eq!(r["ok"], true, "{r}");
            r["result"].clone()
        }
    }

    #[test]
    fn the_trace_records_the_drawn_position_every_frame() {
        let mut t = T::new();
        let r = t.ok("motion.trace", json!({ "probe": { "kind": "mesh", "material": 3 } }));
        assert_eq!(r["probe"], "mesh");
        t.a.run_headless(30, DT);
        let r = t.ok("motion.read", json!({}));
        let rows = r["samples"].as_array().unwrap();
        assert!(rows.len() >= 30, "{}", rows.len());
        // One row per frame, frames consecutive, x one metre further each time,
        // times 16 ms apart.
        for w in rows.windows(2) {
            assert_eq!(w[1][1].as_u64().unwrap(), w[0][1].as_u64().unwrap() + 1);
            assert!((w[1][2].as_f64().unwrap() - w[0][2].as_f64().unwrap() - 1.0).abs() < 1e-6, "{w:?}");
            assert!((w[1][0].as_f64().unwrap() - w[0][0].as_f64().unwrap() - DT).abs() < 1e-6, "{w:?}");
        }
        // The followed box is at the canvas centre.
        let last = rows.last().unwrap();
        assert!((last[5].as_f64().unwrap() - 640.0).abs() < 1.0 && (last[6].as_f64().unwrap() - 360.0).abs() < 1.0, "{last:?}");
        assert_eq!(r["missing"], 0);
    }

    #[test]
    fn reads_page_through_the_buffer_and_stop_keeps_the_samples() {
        let mut t = T::new();
        t.ok("motion.trace", json!({ "probe": { "kind": "camera_target" }, "capacity": 10 }));
        t.a.run_headless(20, DT);
        let a = t.ok("motion.read", json!({ "max": 4 }));
        assert_eq!(a["samples"].as_array().unwrap().len(), 4);
        assert_eq!(a["next"], 4);
        let b = t.ok("motion.read", json!({ "since": 4 }));
        assert_eq!(b["total"], 10, "capacity keeps the first ten");
        assert!(b["dropped"].as_u64().unwrap() >= 10);
        let s = t.ok("motion.stop", json!({}));
        assert_eq!(s["on"], false);
        t.a.run_headless(5, DT);
        let c = t.ok("motion.read", json!({}));
        assert_eq!(c["total"], 10);
        assert_eq!(c["on"], false);
    }

    #[test]
    fn sprites_report_screen_positions_and_unknown_probes_are_refused() {
        let mut t = T::new();
        t.ok("motion.trace", json!({ "probe": { "kind": "sprite", "cell": 5, "layer": 2 } }));
        t.a.run_headless(3, DT);
        let r = t.ok("motion.read", json!({}));
        let rows = r["samples"].as_array().unwrap();
        assert!(rows[0][2].is_null());
        let dx = rows[1][5].as_f64().unwrap() - rows[0][5].as_f64().unwrap();
        assert!((dx - 2.0).abs() < 1e-6);
        let bad = t.call("motion.trace", json!({ "probe": { "kind": "kart" } }));
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error"]["code"], "invalid_value");
        let bad = t.call("motion.trace", json!({ "probe": { "kind": "mesh" } }));
        assert_eq!(bad["error"]["code"], "invalid_value");
    }

    #[test]
    fn a_probe_that_is_not_drawn_counts_as_missing() {
        let rw = RenderWorld { viewport_css: [100.0, 100.0], ..Default::default() };
        let mut tr = MotionTrace { probe: Some(Probe::Mesh { mesh: Some(9), material: None, near: None }), capacity: 4, ..Default::default() };
        tr.record(&rw, 0.0, 0);
        assert_eq!(tr.missing, 1);
        assert_eq!(tr.samples[0].world, None);
    }
}
