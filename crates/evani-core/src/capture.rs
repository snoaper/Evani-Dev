//! Interfaces the renderer implements for live capture, bench and A/B compare
//! (`capture.frame`, `bench.run`, `compare.ab` in docs/LIVE_API.md), plus the
//! device-loss counter auto-revert watches.
//!
//! Only plain data lives here; nothing in this file runs unless a resource is
//! present. [`DeviceStatus`] is inserted by [`crate::App::new`] (shipped games
//! count device losses too). [`CaptureQueue`] is inserted only by evani-live,
//! so a build without live never queues a capture. [`BenchPath`] is inserted
//! by the game (under its `live` feature); evani-live falls back to a default
//! path when it is missing. [`CaptureMoments`] (scripted moments such as a
//! drift or a finish, for `capture.frame { moment }`) is inserted the same
//! way, so their setup code never ships.
//!
//! What the renderer does (its `capture` feature, which only live builds
//! turn on):
//! 1. If `DeviceStatus` exists, add 1 to `lost_count` whenever the GPU device
//!    is lost (and again after each failed recovery). Always, not gated.
//! 2. If a `CaptureQueue` exists:
//!    * honour `camera_override` (use that pose instead of the game camera).
//!      `sim_time_override` is honoured by the core loop (core feature
//!      `capture`): while it is `Some` no fixed ticks run and
//!      `Time::render_seconds` returns it, so the scene renders as at that
//!      sim time and the sim resumes where it paused;
//!    * `take_pending()`; for each request render the frame as described by
//!      its [`CaptureView`] (a `Path` view uses the path's pose at
//!      `sim_time`), read the colour target back (async map, no stall) and
//!      call `complete(CaptureResult)` with the full-size RGBA8 pixels a few
//!      frames later. On failure complete with `rgba: Err(reason)`.
//!      Downscaling to `width` and PNG encoding happen in evani-live (dev
//!      only), so no encoder ever ships in a game. The overlay is a DOM
//!      element, never in the colour target, so `hide_overlay` needs no work;
//!    * while [`HoldCanvas`] is set, draw as usual but leave the canvas on
//!      its last presented frame (see the type).
//! 3. Keep filling `RenderStats` (frame stats, pass timings) as today;
//!    `bench.run` and `compare.ab` are built by evani-live from those stats
//!    plus captures driven through this queue.

use std::collections::VecDeque;

use crate::camera::{CameraPath, CameraPose};
use crate::overlay::RenderStats;

/// GPU device health, written by the renderer. World resource.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceStatus {
    /// Times the device was lost since start.
    pub lost_count: u32,
}

/// Which view a capture renders.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureView {
    /// The next frame as the player sees it.
    Current,
    /// The camera at `path` evaluated at `sim_time` seconds, with the scene
    /// rendered as at that sim time (deterministic across runs).
    Path { path: CameraPath, sim_time: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureRequest {
    pub id: u64,
    /// Output width in pixels (height follows the aspect ratio).
    pub width: u32,
    pub view: CaptureView,
    pub hide_overlay: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureResult {
    pub id: u64,
    /// Tightly packed RGBA8 pixels (sRGB, top row first, `width * height * 4`
    /// bytes) at the full render size, or why the capture failed.
    pub rgba: Result<Vec<u8>, String>,
    /// Size of `rgba` in pixels (the render size, not the requested width).
    pub width: u32,
    pub height: u32,
    /// `Time::frame` of the captured frame.
    pub frame: u64,
    pub sim_time: f64,
    /// Stats of the captured frame.
    pub stats: RenderStats,
    /// The camera the frame was drawn with (the game's, a path pose or an
    /// override), or `None` when the renderer could not say. The review's
    /// world-edge check places the horizon from it.
    pub camera: Option<CameraPose>,
    /// A HUD the kit draws inside the canvas, as the captured frame drew it
    /// (`None` when the kit declares no canvas HUD). The review's captures
    /// item checks it like a DOM HUD element.
    pub canvas_hud: Option<CanvasHud>,
}

/// The canvas-drawn HUD of one frame: the sprites on the layers the kit
/// declared as its HUD (`RenderWorld::hud_layers` in evani-render).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CanvasHud {
    /// HUD sprites drawn this frame with a visible tint (alpha above 0.05).
    pub sprites: u32,
    /// Their bounding rectangle `[x, y, w, h]` in CSS pixels, clipped to the
    /// canvas (zero size when none is on the canvas).
    pub rect_css: [f32; 4],
    /// The canvas size in CSS pixels.
    pub viewport_css: [f32; 2],
}

/// Capture requests and results. World resource, inserted only by evani-live.
#[derive(Debug, Default)]
pub struct CaptureQueue {
    next_id: u64,
    pending: VecDeque<CaptureRequest>,
    results: Vec<CaptureResult>,
    /// Render from this pose instead of the game camera while `Some`.
    pub camera_override: Option<CameraPose>,
    /// Render the scene as at this sim time while `Some`.
    pub sim_time_override: Option<f64>,
    /// Clear the forward pass to this linear RGB instead of the sky colour
    /// while `Some` (geocheck paints the sky magenta to find holes; fog keeps
    /// the real sky colour).
    pub clear_override: Option<[f32; 3]>,
    /// Shift the projection by this many pixels `[x, y]` (x right, y down)
    /// while `Some`: geocheck renders a view twice, a sub-pixel apart, to
    /// find flickering (z-fighting) pixels.
    pub jitter_px: Option<[f32; 2]>,
    /// Draw every material plain white, lit, without emissive, while `true`:
    /// the geometry debug view's instance colours (check colour or grey)
    /// then show as they are instead of tinting the look's material colours.
    pub plain_materials: bool,
}

/// Keep the canvas on the last frame it showed while `.0` is `true`: a
/// `sweep.run`, `compare.ab` or a `capture.frame` with temporary values is
/// trying settings the player must not see (docs/LIVE_API.md 6.30). The
/// renderer still draws every frame through the whole graph (the present
/// pass into an off-screen texture of the canvas's size and format, so
/// timings are unchanged) and captures read `ldr` as always; it only skips
/// taking and presenting the canvas texture, and defers a canvas resize
/// until the hold ends. World resource, inserted only by evani-live, which
/// sets it every frame from the active run, so it can't outlive the run.
/// (Its own resource, not a `CaptureQueue` field, so shipped games, which
/// read `CaptureQueue`, compile exactly as before.)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HoldCanvas(pub bool);

impl CaptureQueue {
    /// Any override (camera, sim time, clear colour, jitter, plain
    /// materials) is active.
    pub fn overriding(&self) -> bool {
        self.sim_time_override.is_some()
            || self.camera_override.is_some()
            || self.clear_override.is_some()
            || self.jitter_px.is_some()
            || self.plain_materials
    }
}

impl CaptureQueue {
    /// Queue a capture; returns its id.
    pub fn request(&mut self, width: u32, view: CaptureView, hide_overlay: bool) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.pending.push_back(CaptureRequest {
            id,
            width,
            view,
            hide_overlay,
        });
        id
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Renderer: take every queued request.
    pub fn take_pending(&mut self) -> Vec<CaptureRequest> {
        self.pending.drain(..).collect()
    }

    /// Renderer: hand back a finished capture.
    pub fn complete(&mut self, result: CaptureResult) {
        self.results.push(result);
    }

    /// Live: take the result for `id` once it is there.
    pub fn take_result(&mut self, id: u64) -> Option<CaptureResult> {
        let i = self.results.iter().position(|r| r.id == id)?;
        Some(self.results.swap_remove(i))
    }
}

/// The camera path `bench.run` and `compare.ab` fly. World resource, inserted
/// by the game when built with live; without it evani-live uses a default path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BenchPath(pub CameraPath);

/// Puts the game into one scripted moment (a drift with sparks, an item hit,
/// the finish line) for `capture.frame { moment }`. It must be deterministic:
/// the same world state comes out every time, whatever ran before (reseed any
/// RNG, step the sim with a fixed dt). It may change game state freely but
/// not params; live holds the sim clock right after it returns, so the frame
/// shows exactly the state it left, through the game's own camera.
pub type MomentSetup = fn(&crate::ecs::World) -> Result<(), String>;

/// One scripted capture moment.
#[derive(Debug, Clone)]
pub struct CaptureMoment {
    /// Short id, `a-z`, `0-9` and `_` (`drift`, `item_hit`, `finish`).
    pub name: String,
    /// What the moment shows, in plain words (shown on the review card).
    pub label: String,
    pub setup: MomentSetup,
}

/// The game's scripted capture moments, in the order a review shows them.
/// World resource, inserted by the game when built with live (like
/// [`BenchPath`]), so no setup code reaches a shipped download.
#[derive(Debug, Clone, Default)]
pub struct CaptureMoments {
    moments: Vec<CaptureMoment>,
}

impl CaptureMoments {
    /// Add a moment (a moment of the same name is replaced in place).
    pub fn add(&mut self, name: &str, label: &str, setup: MomentSetup) -> Result<(), String> {
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(format!("capture moment name {name:?} must be a-z, 0-9 and _"));
        }
        let m = CaptureMoment {
            name: name.to_string(),
            label: label.to_string(),
            setup,
        };
        match self.moments.iter_mut().find(|x| x.name == name) {
            Some(x) => *x = m,
            None => self.moments.push(m),
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&CaptureMoment> {
        self.moments.iter().find(|m| m.name == name)
    }

    pub fn list(&self) -> &[CaptureMoment] {
        &self.moments
    }

    pub fn names(&self) -> Vec<&str> {
        self.moments.iter().map(|m| m.name.as_str()).collect()
    }
}

/// Render pacing while the page is parked off-screen (docs/LIVE_API.md
/// 3.5). Inserted only by evani-live; the renderer (feature `capture`)
/// draws a frame only when [`RenderPace::admit`] says so. The simulation
/// and every frame callback still run at full rate; only drawing slows.
#[derive(Debug, Default, Clone)]
pub struct RenderPace {
    /// Least time between two drawn frames; 0 draws every frame.
    pub min_interval_ms: f64,
    /// When the last drawn frame started (`Time::now_ms`).
    pub last_ms: f64,
    /// Frames drawn and frames skipped since the page started.
    pub rendered: u64,
    pub skipped: u64,
}

impl RenderPace {
    /// Draw the frame starting at `now_ms`? `force` (a capture waiting, a
    /// readback in flight) always draws.
    pub fn admit(&mut self, now_ms: f64, force: bool) -> bool {
        let due = self.min_interval_ms <= 0.0 || now_ms - self.last_ms >= self.min_interval_ms || now_ms < self.last_ms;
        if force || due {
            self.last_ms = now_ms;
            self.rendered += 1;
            true
        } else {
            self.skipped += 1;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_round_trip() {
        let mut q = CaptureQueue::default();
        let a = q.request(320, CaptureView::Current, true);
        let b = q.request(160, CaptureView::Current, false);
        assert!(q.has_pending());
        let reqs = q.take_pending();
        assert_eq!(reqs.len(), 2);
        assert!(!q.has_pending());
        q.complete(CaptureResult {
            id: b,
            rgba: Err("no gpu".into()),
            width: 0,
            height: 0,
            frame: 1,
            sim_time: 0.0,
            stats: RenderStats::default(),
            camera: None,
            canvas_hud: None,
        });
        assert!(q.take_result(a).is_none());
        assert!(q.take_result(b).is_some());
        assert!(q.take_result(b).is_none());
    }

    #[test]
    fn moments_keep_order_replace_by_name_and_check_names() {
        fn a(_: &crate::ecs::World) -> Result<(), String> {
            Ok(())
        }
        fn b(_: &crate::ecs::World) -> Result<(), String> {
            Err("no".into())
        }
        let mut m = CaptureMoments::default();
        m.add("drift", "A drift", a).unwrap();
        m.add("finish", "The line", a).unwrap();
        m.add("drift", "A drift with sparks", b).unwrap();
        assert_eq!(m.names(), ["drift", "finish"]);
        assert_eq!(m.get("drift").unwrap().label, "A drift with sparks");
        let w = crate::ecs::World::new();
        assert!((m.get("drift").unwrap().setup)(&w).is_err());
        assert!(m.get("nope").is_none());
        assert!(m.add("Bad Name", "x", a).is_err());
        assert!(m.add("", "x", a).is_err());
    }

    #[test]
    fn render_pace_draws_about_five_a_second_when_slowed() {
        let mut p = RenderPace::default();
        // Full rate: every frame.
        for i in 0..60 {
            assert!(p.admit(f64::from(i) * 16.7, false));
        }
        // 200 ms: one second of 60 Hz callbacks draws 5.
        p = RenderPace { min_interval_ms: 200.0, ..Default::default() };
        let drawn = (0..60).filter(|i| p.admit(1000.0 + f64::from(*i) * 16.7, false)).count();
        assert_eq!((drawn, p.skipped), (5, 55));
        // A capture forces a frame, and the clock going back never stalls.
        assert!(p.admit(1990.0, true));
        assert!(p.admit(10.0, false));
    }
}
