//! Measured on-screen importance of models (S7-3, dev only: feature
//! `importance`; `cargo xtask importance <id>` runs it).
//!
//! How much each model matters is measured, not guessed: the game runs
//! natively over its scripted capture moments and its bench camera path (the
//! deterministic pieces evani-live already uses), and every model draw of
//! those frames is projected into the view. The result is
//! `apps/<id>/assets/importance.json`, which `cargo xtask bytes` reads
//! (`max_screen_share`) and the value-per-byte budget (S7-4) will read too.
//!
//! What a sample is: a model's draw is its bounding sphere (the one
//! [`crate::model::lod`] already projects to pick a level). Its share is the
//! area of that sphere's disc on screen, clipped to the screen, over the
//! screen's area. That is an upper bound: it ignores occlusion and the empty
//! corners of the sphere, so it can only over-rate a model, never hide one.
//! The pixel size is the disc's diameter in pixels at the viewport
//! (default 1280 x 720), which caps a useful texture size.
//!
//! Nothing here ships. The module is built only with the `importance`
//! feature, which no game lists and the ship build never turns on; the three
//! hooks in the engine files are `#[cfg(feature = "importance")]` additions
//! at the ends of existing lines. [`MARKER`] is in a download only if the
//! feature is, and the ship guard fails on it.

use std::collections::BTreeMap;

use evani_core::camera::CameraPose;
use serde_json::{Map, Value};

use crate::model::{ModelInstance, lod};

/// The text that marks a build carrying this tool (the ship guard looks for it).
pub const MARKER: &str = "evani-importance";

/// The screen the shares are measured on when none is asked for.
pub const DEFAULT_VIEWPORT: [u32; 2] = [1280, 720];

/// Slices of the numeric disc-in-screen area (only for a disc crossing the edge).
const SLICES: usize = 256;

/// The disc `(cx, cy, rho)` (screen units: half the screen height) clipped to
/// the screen `|x| <= ax`, `|y| <= ay`.
fn clipped_disc_area(cx: f64, cy: f64, rho: f64, ax: f64, ay: f64) -> f64 {
    if rho <= 0.0 {
        return 0.0;
    }
    if cx.abs() + rho <= ax && cy.abs() + rho <= ay {
        return core::f64::consts::PI * rho * rho;
    }
    let (x0, x1) = ((cx - rho).max(-ax), (cx + rho).min(ax));
    if x1 <= x0 {
        return 0.0;
    }
    let dx = (x1 - x0) / SLICES as f64;
    let mut area = 0.0;
    for i in 0..SLICES {
        let x = x0 + (i as f64 + 0.5) * dx;
        let t = rho * rho - (x - cx) * (x - cx);
        if t <= 0.0 {
            continue;
        }
        let h = t.sqrt();
        let (lo, hi) = ((cy - h).max(-ay), (cy + h).min(ay));
        if hi > lo {
            area += (hi - lo) * dx;
        }
    }
    area
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n > 1e-12).then(|| [a[0] / n, a[1] / n, a[2] / n])
}

/// A sphere's `(screen share, diameter in pixels)` seen by `cam` on a
/// `viewport`; `None` when it is wholly behind the camera or off the screen.
pub fn project(centre: [f32; 3], radius: f32, cam: &CameraPose, viewport: [u32; 2]) -> Option<(f64, f64)> {
    let (w, h) = (f64::from(viewport[0].max(1)), f64::from(viewport[1].max(1)));
    let aspect = w / h;
    let eye = cam.eye.map(f64::from);
    let rel = sub(centre.map(f64::from), eye);
    let r = f64::from(radius);
    let fwd = unit(sub(cam.target.map(f64::from), eye))?;
    let z = dot(rel, fwd);
    if r.is_nan() || r <= 0.0 || z + r <= 0.0 {
        return None;
    }
    let dist = dot(rel, rel).sqrt();
    if dist <= r {
        // The camera is inside the sphere: it fills the screen.
        return Some((1.0, f64::from(lod::screen_fraction(radius, dist as f32, cam.fov_y_deg)) * h));
    }
    // The size the level choice uses: the sphere's diameter in screen heights
    // (its radius in half screen heights).
    let rho = f64::from(lod::screen_fraction(radius, dist as f32, cam.fov_y_deg));
    let t = (f64::from(cam.fov_y_deg).to_radians() * 0.5).tan().max(1e-4);
    let zc = z.max(r).max(1e-4);
    let (cx, cy) = match unit(cross(fwd, cam.up.map(f64::from))) {
        Some(right) => {
            let up = cross(right, fwd);
            (dot(rel, right) / (zc * t), dot(rel, up) / (zc * t))
        }
        None => (0.0, 0.0),
    };
    let area = clipped_disc_area(cx, cy, rho, aspect, 1.0);
    if area <= 0.0 {
        return None;
    }
    Some(((area / (4.0 * aspect)).min(1.0), rho * h))
}

/// The model id a model file belongs to: `models/hero.low.glb` -> `hero`.
fn model_id_of(url: &str) -> &str {
    let file = url.rsplit('/').next().unwrap_or(url);
    file.split('.').next().unwrap_or(file)
}

/// What one model has shown so far.
#[derive(Debug, Default, Clone)]
struct Acc {
    /// Recorded frames it was on screen in.
    frames: u32,
    /// Its share summed over those frames.
    sum: f64,
    max_share: f64,
    max_px: f64,
    /// Its largest share in each source (a moment, or the path).
    by_source: BTreeMap<String, f64>,
}

/// Round to six decimals, so the file does not carry float noise.
fn r6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn num(x: f64) -> Value {
    Value::from(r6(x))
}

/// The measurements: one sample is one frame the driver (or a test) marked
/// with [`Log::begin`]. Lives in [`crate::model::Models`] as `importance`.
#[derive(Debug, Default)]
pub struct Log {
    /// Set on the first draw seen, so [`MARKER`] is in a build that has this tool.
    tool: &'static str,
    /// The screen measured on; zero is [`DEFAULT_VIEWPORT`].
    pub viewport: [u32; 2],
    /// Measure from this camera instead of the frame's (the path phase).
    pub pose: Option<CameraPose>,
    /// The source of the frame in progress; `None`: not recording it.
    source: Option<String>,
    /// This frame's `(share, pixels)` of each model id, the largest draw.
    cur: BTreeMap<String, (f64, f64)>,
    /// Recorded frames per source.
    samples: BTreeMap<String, u32>,
    acc: BTreeMap<String, Acc>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) driver: Driver,
}

impl Log {
    fn screen(&self) -> [u32; 2] {
        if self.viewport[0] == 0 || self.viewport[1] == 0 { DEFAULT_VIEWPORT } else { self.viewport }
    }

    /// Close the frame in progress and start `source` (`None`: stop recording).
    pub fn begin(&mut self, source: Option<&str>) {
        let cur = core::mem::take(&mut self.cur);
        if let Some(src) = self.source.take() {
            *self.samples.entry(src.clone()).or_insert(0) += 1;
            for (id, (share, px)) in cur {
                let a = self.acc.entry(id).or_default();
                a.frames += 1;
                a.sum += share;
                a.max_share = a.max_share.max(share);
                a.max_px = a.max_px.max(px);
                let s = a.by_source.entry(src.clone()).or_insert(0.0);
                *s = s.max(share);
            }
        }
        self.source = source.map(str::to_owned);
    }

    /// Frames recorded so far.
    pub fn recorded(&self) -> u32 {
        self.samples.values().sum()
    }

    /// One draw of the model file `url` (the hook in `Models::select_lods`).
    pub(crate) fn note(&mut self, url: &str, ready: bool, sphere: ([f32; 3], f32), d: &ModelInstance, cams: &[CameraPose]) {
        self.tool = MARKER;
        if !ready || self.source.is_none() {
            return;
        }
        let (c, r) = sphere;
        let s = d.scale;
        let off = lod::rotate(d.rotation, [c[0] * s[0], c[1] * s[1], c[2] * s[2]]);
        let centre = [d.position[0] + off[0], d.position[1] + off[1], d.position[2] + off[2]];
        let radius = r * s[0].abs().max(s[1].abs()).max(s[2].abs());
        let screen = self.screen();
        let mut best: Option<(f64, f64)> = None;
        for cam in self.pose.as_ref().map_or(cams, core::slice::from_ref) {
            if let Some((share, px)) = project(centre, radius, cam, screen) {
                best = Some(best.map_or((share, px), |(s0, p0)| (s0.max(share), p0.max(px))));
            }
        }
        if let Some((share, px)) = best {
            let e = self.cur.entry(model_id_of(url).to_owned()).or_insert((0.0, 0.0));
            *e = (e.0.max(share), e.1.max(px));
        }
    }

    /// The measurements as `importance.json`: sorted keys, six decimals, no
    /// time or path in it, so the same run gives the same bytes.
    pub fn json(&self, game: &str) -> String {
        let total = self.recorded();
        let mut models = Vec::new();
        for (id, a) in &self.acc {
            let mean = if total == 0 { 0.0 } else { a.sum / f64::from(total) };
            let mut sources = Map::new();
            for (s, v) in &a.by_source {
                sources.insert(s.clone(), num(*v));
            }
            let want = (a.max_px.ceil().max(16.0) as u64).next_power_of_two().min(4096);
            let mut m = Map::new();
            m.insert("frames_on_screen".into(), Value::from(a.frames));
            m.insert("frames_total".into(), Value::from(total));
            m.insert("id".into(), Value::from(id.clone()));
            m.insert("importance_score".into(), num(0.5 * a.max_share + 0.5 * mean));
            m.insert("max_px".into(), num(a.max_px));
            m.insert("max_screen_share".into(), num(a.max_share));
            m.insert("mean_screen_share".into(), num(mean));
            m.insert("sources".into(), Value::Object(sources));
            m.insert("suggested_texture_px".into(), Value::from(want));
            models.push(Value::Object(m));
        }
        let moments: u32 = self.samples.iter().filter(|(k, _)| k.starts_with("moment:")).map(|(_, v)| *v).sum();
        let mut samples = Map::new();
        samples.insert("moments".into(), Value::from(moments));
        samples.insert("path".into(), Value::from(self.samples.get("path").copied().unwrap_or(0)));
        let screen = self.screen();
        let mut top = Map::new();
        top.insert("game".into(), Value::from(game));
        top.insert("models".into(), Value::Array(models));
        top.insert("samples".into(), Value::Object(samples));
        top.insert("tool".into(), Value::from(MARKER));
        top.insert("version".into(), Value::from(1));
        top.insert("viewport".into(), Value::from(vec![screen[0], screen[1]]));
        let mut text = serde_json::to_string_pretty(&Value::Object(top)).unwrap_or_default();
        text.push('\n');
        text
    }
}

// ---- the native driver --------------------------------------------------------------

/// Frames let go before the first sample (models arrive, the game settles).
#[cfg(not(target_arch = "wasm32"))]
const WARM_FRAMES: u32 = 45;
/// Frames between a moment's setup and its sample (the game catches up).
#[cfg(not(target_arch = "wasm32"))]
const MOMENT_WAIT: u32 = 3;
/// Moments sampled at most.
#[cfg(not(target_arch = "wasm32"))]
const MAX_MOMENTS: usize = 24;
/// Camera positions sampled along the bench path, one per frame.
#[cfg(not(target_arch = "wasm32"))]
const PATH_SAMPLES: u32 = 90;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Fresh,
    /// `EVANI_IMPORTANCE_OUT` is not set: the driver does nothing.
    Off,
    Warm,
    /// Moment `idx`, sub-step `sub`: 0 let the clock go, 1 set up and hold,
    /// 2.. wait, then sample.
    Moments,
    Path,
    Done,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, Clone)]
pub struct Settings {
    /// The file to write.
    pub out: std::path::PathBuf,
    /// The game's `assets` folder (its `models/` is read).
    pub assets: std::path::PathBuf,
    pub game: String,
    /// The view in pixels; [`DEFAULT_VIEWPORT`] when `None`.
    pub viewport: Option<[u32; 2]>,
}

/// The same settings as a resource beat the environment: a test sets them
/// per app instead of for the whole process (`cargo xtask importance` uses
/// `EVANI_IMPORTANCE_*`, which are read once, when the first frame runs).
#[cfg(not(target_arch = "wasm32"))]
type Cfg = Settings;

/// The run's state machine (inside [`Log`]); one step per frame.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub(crate) struct Driver {
    state: State,
    n: u32,
    cfg: Cfg,
    moments: Vec<(String, evani_core::capture::MomentSetup)>,
    path: Option<evani_core::camera::CameraPath>,
    idx: usize,
    sub: u32,
}

/// Called every frame at the start of the renderer's frame (a hook at the
/// end of `RenderModule::frame`'s first line). Does nothing unless the run
/// was started by `cargo xtask importance` (env `EVANI_IMPORTANCE_OUT`).
#[cfg(not(target_arch = "wasm32"))]
pub fn drive(world: &evani_core::ecs::World) {
    use crate::model::Models;
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    if models.importance.driver.state == State::Off || models.importance.driver.state == State::Done {
        return;
    }
    let mut d = core::mem::take(&mut models.importance.driver);
    let setup = step(&mut d, &mut models, world);
    models.importance.driver = d;
    drop(models);
    if let Some(f) = setup {
        if let Err(e) = f(world) {
            world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("importance: moment setup: {e}"));
        }
        // Hold the clock where the setup left it; the game's camera frames it.
        let t = world.try_res::<evani_core::time::Time>().map_or(0.0, |t| t.render_seconds());
        if let Some(mut q) = world.try_res_mut::<evani_core::capture::CaptureQueue>() {
            q.sim_time_override = Some(t);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn drive(_world: &evani_core::ecs::World) {}

#[cfg(not(target_arch = "wasm32"))]
fn release(world: &evani_core::ecs::World) {
    if let Some(mut q) = world.try_res_mut::<evani_core::capture::CaptureQueue>() {
        q.sim_time_override = None;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start(d: &mut Driver, models: &mut crate::model::Models, world: &evani_core::ecs::World) {
    use evani_core::capture::{BenchPath, CaptureMoments, CaptureQueue};
    if let Some(s) = world.try_res::<Settings>() {
        d.cfg = (*s).clone();
    } else if let Some(out) = std::env::var_os("EVANI_IMPORTANCE_OUT") {
        d.cfg = Cfg {
            out: out.into(),
            assets: std::env::var_os("EVANI_IMPORTANCE_ASSETS").map(Into::into).unwrap_or_default(),
            game: std::env::var("EVANI_IMPORTANCE_GAME").unwrap_or_default(),
            viewport: None,
        };
        if let Ok(v) = std::env::var("EVANI_IMPORTANCE_VIEWPORT")
            && let Some((w, h)) = v.split_once('x')
            && let (Ok(w), Ok(h)) = (w.trim().parse::<u32>(), h.trim().parse::<u32>())
        {
            d.cfg.viewport = Some([w, h]);
        }
    } else {
        d.state = State::Off;
        return;
    }
    if let Some(v) = d.cfg.viewport {
        models.importance.viewport = v;
    }
    // Moments need the held clock (live's `CaptureQueue`); without it only the path runs.
    if world.try_res::<CaptureQueue>().is_some()
        && let Some(m) = world.try_res::<CaptureMoments>()
    {
        d.moments = m.list().iter().map(|m| (m.name.clone(), m.setup)).collect();
        d.moments.sort_by(|a, b| a.0.cmp(&b.0));
        d.moments.truncate(MAX_MOMENTS);
    }
    d.path = world.try_res::<BenchPath>().map(|p| p.0);
    d.state = State::Warm;
}

/// Models arrive from `assets/models/` (the manifest too) the way a native
/// game delivers them itself; a game that already did leaves nothing to take.
#[cfg(not(target_arch = "wasm32"))]
fn deliver(d: &Driver, models: &mut crate::model::Models, world: &evani_core::ecs::World) {
    use crate::model::{handle::Manifest, stream};
    let dir = d.cfg.assets.join("models");
    if models.has_handles() && matches!(models.stream.manifest, Manifest::Unknown) {
        let text = std::fs::read_to_string(dir.join("stream.json")).map_err(|e| e.to_string());
        match text.and_then(|t| stream::manifest_from_text(&t)) {
            Ok(m) => models.set_manifest(Some(m)),
            Err(_) => models.set_manifest(None),
        }
    }
    for (id, url) in models.take_requests() {
        let file = url.strip_prefix(stream::FOLDER).unwrap_or(&url);
        match std::fs::read(dir.join(file)).map_err(|e| format!("{file}: {e}")) {
            Ok(b) => models.deliver(id, Ok(&b)),
            Err(e) => {
                world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("importance: {e}"));
                models.deliver(id, Err(e));
            }
        }
    }
}

/// One frame of the run. Returns a moment setup to call once `models` is let go.
#[cfg(not(target_arch = "wasm32"))]
fn step(d: &mut Driver, models: &mut crate::model::Models, world: &evani_core::ecs::World) -> Option<evani_core::capture::MomentSetup> {
    if d.state == State::Fresh {
        start(d, models, world);
        if d.state == State::Off {
            return None;
        }
    }
    deliver(d, models, world);
    d.n += 1;
    match d.state {
        State::Warm => {
            if d.n >= WARM_FRAMES {
                d.state = if d.moments.is_empty() { State::Path } else { State::Moments };
                d.idx = 0;
                d.sub = 0;
            }
        }
        State::Moments => {
            let sub = d.sub;
            d.sub += 1;
            match sub {
                0 => {
                    models.importance.begin(None);
                    release(world);
                }
                1 => return d.moments.get(d.idx).map(|m| m.1),
                s if s < 2 + MOMENT_WAIT => {}
                _ => {
                    let name = d.moments.get(d.idx).map(|m| m.0.clone()).unwrap_or_default();
                    models.importance.begin(Some(&format!("moment:{name}")));
                    d.idx += 1;
                    d.sub = 0;
                    if d.idx >= d.moments.len() {
                        d.state = State::Path;
                        d.idx = 0;
                    }
                }
            }
        }
        State::Path => {
            let Some(path) = d.path else {
                finish(d, models, world);
                return None;
            };
            if d.idx == 0 {
                release(world);
            }
            if d.idx as u32 >= PATH_SAMPLES {
                finish(d, models, world);
                return None;
            }
            let period = f64::from(path.period_s.abs().max(1e-3));
            let t = period * d.idx as f64 / f64::from(PATH_SAMPLES);
            models.importance.pose = Some(path.pose(t));
            models.importance.begin(Some("path"));
            d.idx += 1;
        }
        State::Fresh | State::Off | State::Done => {}
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn finish(d: &mut Driver, models: &mut crate::model::Models, world: &evani_core::ecs::World) {
    models.importance.begin(None);
    models.importance.pose = None;
    release(world);
    d.state = State::Done;
    let text = models.importance.json(&d.cfg.game);
    if let Some(dir) = d.cfg.out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&d.cfg.out, text) {
        Ok(()) => eprintln!("{MARKER}: wrote {} ({} frames recorded)", d.cfg.out.display(), models.importance.recorded()),
        Err(e) => world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("importance: {}: {e}", d.cfg.out.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelInstance, Models, count_draws};
    use crate::world::RenderWorld;
    use evani_core::overlay::RenderStats;

    fn cam() -> CameraPose {
        CameraPose { eye: [0.0, 0.0, 0.0], target: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0], fov_y_deg: 60.0 }
    }

    #[test]
    fn a_sphere_ahead_has_the_disc_share() {
        let (share, px) = project([0.0, 0.0, -10.0], 1.0, &cam(), [1280, 720]).unwrap();
        let rho = 1.0 / (10.0 * (30.0f64).to_radians().tan());
        let want = core::f64::consts::PI * rho * rho / (4.0 * 1280.0 / 720.0);
        assert!((share - want).abs() < 1e-4, "{share} vs {want}");
        assert!((px - rho * 720.0).abs() < 0.5, "{px}");
    }

    #[test]
    fn behind_or_off_screen_is_nothing_and_the_edge_is_clipped() {
        let v = [1280, 720];
        assert!(project([0.0, 0.0, 10.0], 1.0, &cam(), v).is_none(), "behind");
        assert!(project([200.0, 0.0, -10.0], 1.0, &cam(), v).is_none(), "far to the side");
        // A disc centred on the screen's right edge shows exactly half.
        let aspect = 1280.0 / 720.0;
        let (full, half) = (clipped_disc_area(0.0, 0.0, 0.3, aspect, 1.0), clipped_disc_area(aspect, 0.0, 0.3, aspect, 1.0));
        assert!((half / full - 0.5).abs() < 0.01, "{half} vs {full}");
        // A sphere half off the side of the view shows less than the same one in the middle.
        let whole = project([0.0, 0.0, -4.0], 1.0, &cam(), v).unwrap().0;
        let edge = project([4.0 * (30.0f64).to_radians().tan() as f32 * aspect as f32, 0.0, -4.0], 1.0, &cam(), v).unwrap().0;
        assert!(edge < whole * 0.8, "{edge} vs {whole}");
        // Inside the sphere the whole screen is covered.
        assert!((project([0.0, 0.0, -0.1], 5.0, &cam(), v).unwrap().0 - 1.0).abs() < 1e-3);
    }

    #[test]
    fn model_ids_come_from_the_file_stem() {
        assert_eq!(model_id_of("models/proof-figure.low.glb"), "proof-figure");
        assert_eq!(model_id_of("proof-kart.glb"), "proof-kart");
    }

    /// Run the log over a real prepared model the way the game does: draws,
    /// then `count_draws` (which calls the hook in `select_lods`).
    fn run() -> String {
        let path = format!("{}/../../tests/fixtures/models/kart.low.glb", env!("CARGO_MANIFEST_DIR"));
        let mut m = Models::default();
        let id = m.add_glb("models/kart.low.glb", &std::fs::read(path).unwrap());
        let rw = RenderWorld::default();
        let eye = rw.camera.eye;
        for k in 0..4 {
            m.importance.begin(Some("path"));
            m.draw(ModelInstance::new(id, [eye[0] + k as f32, eye[1], eye[2] - 4.0 - 6.0 * k as f32]));
            count_draws(&mut m, &rw, None, &mut RenderStats::default());
        }
        // Not recording: this draw leaves no trace.
        m.importance.begin(None);
        m.draw(ModelInstance::new(id, [eye[0], eye[1], eye[2] - 2.0]));
        count_draws(&mut m, &rw, None, &mut RenderStats::default());
        m.importance.begin(None);
        m.importance.json("test")
    }

    #[test]
    fn the_hook_measures_draws_and_the_file_is_stable() {
        let a = run();
        assert_eq!(a, run(), "the same draws give the same bytes");
        let v: Value = serde_json::from_str(&a).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["tool"], MARKER);
        assert_eq!(v["samples"]["path"], 4);
        let m = &v["models"][0];
        assert_eq!(m["id"], "kart");
        assert_eq!(m["frames_on_screen"], 4);
        assert!(m["max_screen_share"].as_f64().unwrap() > 0.0);
        assert!(m["max_screen_share"].as_f64().unwrap() <= 1.0);
        assert!(m["max_px"].as_f64().unwrap() > 0.0);
        let want = m["suggested_texture_px"].as_u64().unwrap();
        assert!(want.is_power_of_two() && (16..=4096).contains(&want), "{want}");
        // The nearest draw (4 m away) sets the maximum; the mean is lower.
        assert!(m["mean_screen_share"].as_f64().unwrap() < m["max_screen_share"].as_f64().unwrap());
    }
}
