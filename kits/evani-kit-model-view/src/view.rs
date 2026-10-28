//! The viewer's state: what is shown, the load in flight, the camera, the
//! fetch queue the app serves, and the hot swap (docs/EVANI_MODEL_TAB.md
//! sections 4.1, 4.4, 5.2, 5.4 and 6).
//!
//! Loading never blocks drawing: the app fetches the files the queue asks
//! for (the browser's `fetch` on the web, a test's map natively); when the
//! tier file has arrived and decoded, the next frame swaps it in with the
//! camera, the turntable angle and the frame counter untouched.

use evani_render::mesh::{BUILTIN_MESH_COUNT, MAX_MESHES, Mesh};
use evani_render::{MeshId, MeshLibrary};
use serde_json::{Value, json};

use crate::decode::{self, Decoded};
use crate::orbit::{Orbit, Turntable};

/// Tier names, in order.
pub const TIERS: [&str; 4] = ["low", "medium", "high", "ultra"];

/// Most colour groups a model gets: every custom mesh slot.
pub const MAX_GROUPS: usize = MAX_MESHES - BUILTIN_MESH_COUNT;

/// Files the app is asked to fetch (relative URLs on the page's own
/// origin), and what came back. The app moves results into `done`.
#[derive(Debug, Default)]
pub struct Fetches {
    next: u64,
    pub want: Vec<(u64, String)>,
    /// `Err` holds a short reason; `"http 404"` for a missing file.
    pub done: Vec<(u64, Result<Vec<u8>, String>)>,
}

impl Fetches {
    pub fn request(&mut self, url: String) -> u64 {
        self.next += 1;
        self.want.push((self.next, url));
        self.next
    }

    fn take(&mut self, id: u64) -> Option<Result<Vec<u8>, String>> {
        let i = self.done.iter().position(|d| d.0 == id)?;
        Some(self.done.swap_remove(i).1)
    }
}

/// Pointer input since the last frame, filled by the app from ordinary
/// pointer events (no pointer lock, no keyboard lock).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PointerInput {
    /// Drag movement in CSS pixels.
    pub drag: [f32; 2],
    /// Wheel delta (positive: away).
    pub wheel: f32,
    /// A double-click: frame the model.
    pub frame: bool,
}

/// One version's tier on screen (or held as the previous one).
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub model: String,
    pub version: u64,
    pub tier: String,
    /// That version's `version.json`.
    pub meta: Value,
    pub decoded: Decoded,
}

impl Shown {
    /// The tier's triangle count as the version's metadata states it.
    pub fn meta_tris(&self) -> Option<u64> {
        self.meta.get("tiers")?.get(&self.tier)?.get("tris")?.as_u64()
    }
}

/// What the page shows when there is nothing to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Starting,
    Shown,
    /// No bridge or no models: the page's fallback panel says how to start it.
    NoModels(String),
    /// The first load failed; the line says why.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Step {
    List(u64),
    History(u64),
    File(u64, Value),
}

/// A reply owed to a `view.load` / `view.tier` (token and request id).
pub type Reply = (u64, Value);

#[derive(Debug, Clone, PartialEq)]
struct Load {
    model: Option<String>,
    version: Option<u64>,
    tier: String,
    keep_camera: bool,
    reply: Option<Reply>,
    step: Step,
    started_ms: f64,
    file: String,
}

/// An error a command or load answers with.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewError {
    pub code: &'static str,
    pub message: String,
    pub file: Option<String>,
}

fn verr(code: &'static str, message: impl Into<String>) -> ViewError {
    ViewError { code, message: message.into(), file: None }
}

/// What one frame's update finished: replies and events for the live
/// layer to send (lib.rs), and a log line for the page's console.
#[derive(Debug, Default)]
pub struct Outcome {
    pub replies: Vec<(Reply, Result<Value, ViewError>)>,
    pub events: Vec<(&'static str, Value)>,
    pub log: Vec<String>,
}

/// The viewer resource.
#[derive(Debug)]
pub struct Viewer {
    pub orbit: Orbit,
    pub turn: Turntable,
    pub shown: Option<Shown>,
    /// The version shown before the last swap (kept for Before/After, T2).
    pub previous: Option<Shown>,
    pub status: Status,
    pub fetches: Fetches,
    pub pointer: PointerInput,
    /// Swaps done so far.
    pub swaps: u64,
    /// The frame number of the last swap.
    pub swap_frame: u64,
    load: Option<Load>,
    /// Mesh slots this viewer owns in the [`MeshLibrary`].
    slots: Vec<MeshId>,
    /// Whether the camera was given (address or command) before the first model.
    camera_given: bool,
    aspect: f32,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            orbit: Orbit::default(),
            turn: Turntable::default(),
            shown: None,
            previous: None,
            status: Status::Starting,
            fetches: Fetches::default(),
            pointer: PointerInput::default(),
            swaps: 0,
            swap_frame: 0,
            load: None,
            slots: Vec::new(),
            camera_given: false,
            aspect: 16.0 / 9.0,
        }
    }
}

/// A model id the routes accept: lowercase letters, digits, `-` and `_`.
pub fn valid_model_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The page address's view part (4.4): `#model=..&v=..&tier=..&cam=az,el,dist`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Address {
    pub model: Option<String>,
    pub version: Option<u64>,
    pub tier: Option<String>,
    pub cam: Option<[f32; 3]>,
}

impl Address {
    pub fn parse(hash: &str) -> Self {
        let mut a = Address::default();
        for kv in hash.trim_start_matches('#').split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            match k {
                "model" if valid_model_id(v) => a.model = Some(v.to_string()),
                "v" => a.version = v.parse().ok(),
                "tier" if TIERS.contains(&v) => a.tier = Some(v.to_string()),
                "cam" => {
                    let n: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    if let [az, el, dist] = n[..] {
                        a.cam = Some([az, el, dist]);
                    }
                }
                _ => {}
            }
        }
        a
    }
}

impl Viewer {
    /// Start from the page address: load what it names, or the newest model.
    pub fn start(&mut self, address: &Address, now_ms: f64) {
        if let Some([az, el, dist]) = address.cam {
            self.orbit = Orbit { az, el, dist, ..self.orbit }.clamped();
            self.camera_given = true;
        }
        let tier = address.tier.clone().unwrap_or_else(|| "high".into());
        match &address.model {
            Some(m) => {
                let keep = self.camera_given;
                self.begin(Some(m.clone()), address.version, tier, keep, None, now_ms);
            }
            None => {
                self.begin(None, None, tier, false, None, now_ms);
            }
        }
    }

    /// The address for the view shown now.
    pub fn address(&self) -> String {
        let o = &self.orbit;
        let cam = format!("cam={:.0},{:.0},{:.2}", o.az, o.el, o.dist);
        match &self.shown {
            Some(s) => format!("#model={}&v={}&tier={}&{cam}", s.model, s.version, s.tier),
            None => format!("#{cam}"),
        }
    }

    /// Whether a load is in flight.
    pub fn loading(&self) -> bool {
        self.load.is_some()
    }

    /// Start loading; a load already in flight is dropped (its caller gets
    /// `superseded`).
    pub fn begin(&mut self, model: Option<String>, version: Option<u64>, tier: String, keep_camera: bool, reply: Option<Reply>, now_ms: f64) -> Option<Reply> {
        let step = match &model {
            Some(m) => Step::History(self.fetches.request(format!("/model/v1/{m}/history"))),
            None => Step::List(self.fetches.request("/model/v1/models".into())),
        };
        let old = self.load.take().and_then(|l| l.reply);
        self.load = Some(Load { model, version, tier, keep_camera, reply, step, started_ms: now_ms, file: String::new() });
        old
    }

    /// `view.load` params → a load. Errors are answered at once.
    pub fn command_load(&mut self, params: &Value, reply: Reply, now_ms: f64) -> Result<Option<Reply>, ViewError> {
        let model = params.get("model").and_then(Value::as_str).ok_or_else(|| verr("bad_request", "view.load needs \"model\""))?;
        if !valid_model_id(model) {
            return Err(verr("unknown_model", format!("No model named {model}")));
        }
        let version = match params.get("version") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| verr("bad_request", "\"version\" must be a whole number"))?),
        };
        let tier = self.tier_param(params)?;
        let keep = params.get("keep_camera").and_then(Value::as_bool).unwrap_or(true);
        Ok(self.begin(Some(model.to_string()), version, tier, keep, Some(reply), now_ms))
    }

    /// `view.tier`: the shown version at another tier.
    pub fn command_tier(&mut self, params: &Value, reply: Reply, now_ms: f64) -> Result<Option<Reply>, ViewError> {
        let tier = self.tier_param(params)?;
        let Some(s) = &self.shown else {
            return Err(verr("unknown_model", "No model is shown yet; send view.load first"));
        };
        let (m, v) = (s.model.clone(), s.version);
        Ok(self.begin(Some(m), Some(v), tier, true, Some(reply), now_ms))
    }

    fn tier_param(&self, params: &Value) -> Result<String, ViewError> {
        match params.get("tier").and_then(Value::as_str) {
            Some(t) if TIERS.contains(&t) => Ok(t.to_string()),
            Some(t) => Err(verr("no_tier", format!("No tier named {t} (low, medium, high, ultra)"))),
            None => Ok(self.shown.as_ref().map_or_else(|| "high".to_string(), |s| s.tier.clone())),
        }
    }

    /// `view.camera`.
    pub fn command_camera(&mut self, params: &Value, tick: u64, deg_per_s: f32) -> Result<Value, ViewError> {
        let num = |k: &str| -> Result<Option<f32>, ViewError> {
            match params.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v.as_f64().map(|x| Some(x as f32)).ok_or_else(|| verr("invalid_value", format!("\"{k}\" must be a number"))),
            }
        };
        let (az, el, dist) = (num("az")?, num("el")?, num("dist")?);
        if let Some(d) = dist
            && d <= 0.0
        {
            return Err(verr("invalid_value", "\"dist\" must be above 0"));
        }
        let mut o = self.orbit;
        if let Some(v) = az {
            o.az = v;
        }
        if let Some(v) = el {
            o.el = v;
        }
        if let Some(v) = dist {
            o.dist = v;
        }
        self.orbit = o.clamped();
        if az.is_some() || el.is_some() || dist.is_some() {
            self.camera_given = true;
        }
        if params.get("frame").and_then(Value::as_bool) == Some(true) {
            self.frame();
        }
        if let Some(on) = params.get("turntable").and_then(Value::as_bool) {
            self.turn.set(on, tick, deg_per_s);
        }
        Ok(self.camera_json(tick, deg_per_s))
    }

    pub fn camera_json(&self, tick: u64, deg_per_s: f32) -> Value {
        let o = &self.orbit;
        json!({ "az": r2(o.az), "el": r2(o.el), "dist": r3(o.dist), "turntable": self.turn.on,
                "turntable_deg": r2(self.turn.angle(tick, deg_per_s)) })
    }

    /// Frame the shown model (double-click, `frame: true`).
    pub fn frame(&mut self) {
        if let Some(s) = &self.shown {
            let c = s.decoded.center();
            self.orbit = self.orbit.framed(c, s.decoded.radius(), self.aspect);
        }
    }

    /// `view_stats` / the `stats` of a load result.
    pub fn stats(&self, frame_ms: f32) -> Value {
        match &self.shown {
            Some(s) => json!({ "tier": s.tier, "tris": s.decoded.tris, "verts": s.decoded.verts,
                "draws": s.decoded.groups.len(), "kb": s.decoded.bytes.div_ceil(1024),
                "meta_tris": s.meta_tris(), "frame_ms": r2(frame_ms), "gpu_ms": Value::Null }),
            None => Value::Null,
        }
    }

    /// `view.state`.
    pub fn state(&self, tick: u64, frame: u64, deg_per_s: f32, frame_ms: f32) -> Value {
        let s = self.shown.as_ref();
        json!({
            "model": s.map(|s| s.model.clone()), "version": s.map(|s| s.version), "tier": s.map(|s| s.tier.clone()),
            "tiers": s.and_then(|s| s.meta.get("tiers")).map(|t| TIERS.iter().filter(|n| t.get(**n).is_some()).copied().collect::<Vec<&str>>()),
            "camera": self.camera_json(tick, deg_per_s),
            "before_after": Value::Null, "reference": Value::Null, "overlays": {}, "highlighted": [],
            "address": self.address(), "loading": self.loading(), "frame": frame, "swaps": self.swaps,
            "stats": self.stats(frame_ms), "shading": "preview",
        })
    }

    /// Apply the pointer input gathered since the last frame.
    pub fn apply_pointer(&mut self) {
        let p = std::mem::take(&mut self.pointer);
        if p.drag != [0.0, 0.0] || p.wheel != 0.0 {
            let mut o = self.orbit;
            o.az -= p.drag[0] * 0.3;
            o.el += p.drag[1] * 0.3;
            o.dist *= (p.wheel * 0.0015).exp();
            self.orbit = o.clamped();
            self.camera_given = true;
        }
        if p.frame {
            self.frame();
        }
    }

    /// Advance the load in flight with whatever the app fetched; swap a
    /// decoded tier into the mesh library. `frame` is the frame number.
    pub fn update(&mut self, lib: &mut MeshLibrary, aspect: f32, frame: u64, now_ms: f64, frame_ms: f32) -> Outcome {
        let mut out = Outcome::default();
        if aspect > 0.0 {
            self.aspect = aspect;
        }
        let Some(mut load) = self.load.take() else { return out };
        let id = match &load.step {
            Step::List(id) | Step::History(id) | Step::File(id, _) => *id,
        };
        let Some(got) = self.fetches.take(id) else {
            self.load = Some(load);
            return out;
        };
        let res = match load.step.clone() {
            Step::List(_) => self.on_list(&mut load, got),
            Step::History(_) => self.on_history(&mut load, got),
            Step::File(_, meta) => match got {
                Ok(bytes) => match decode::decode(&bytes, MAX_GROUPS) {
                    Ok(d) => {
                        self.swap(lib, &load, meta, d, frame, now_ms, frame_ms, &mut out);
                        return out;
                    }
                    Err(e) => Err(ViewError { code: e.code, message: e.message, file: Some(load.file.clone()) }),
                },
                Err(e) => Err(ViewError { code: "fetch_failed", message: format!("{} did not load ({e})", load.file), file: Some(load.file.clone()) }),
            },
        };
        match res {
            Ok(()) => self.load = Some(load),
            Err(e) => {
                if self.shown.is_none() {
                    self.status = match e.code {
                        "no_models" | "no_bridge" => Status::NoModels(e.message.clone()),
                        _ => Status::Error(e.message.clone()),
                    };
                }
                out.log.push(format!("model-view: {} ({})", e.message, e.code));
                if load.step != Step::List(id) || self.shown.is_some() {
                    out.events.push(("view_error", json!({ "code": e.code, "message": e.message, "file": e.file })));
                }
                if let Some(r) = load.reply {
                    out.replies.push((r, Err(e)));
                }
            }
        }
        out
    }

    fn on_list(&mut self, load: &mut Load, got: Result<Vec<u8>, String>) -> Result<(), ViewError> {
        let bytes = got.map_err(|e| verr("no_bridge", format!("No model bridge answered ({e}). Open this page from Evani.App's Model tab, or start it with: cargo xtask live --game apps/model-view --port 8097 --models")))?;
        let list: Value = serde_json::from_slice(&bytes).map_err(|e| verr("bad_reply", format!("the model list is not JSON ({e})")))?;
        let first = list.as_array().and_then(|a| a.first()).and_then(|m| m.get("model")).and_then(Value::as_str);
        let Some(m) = first.filter(|m| valid_model_id(m)) else {
            return Err(verr("no_models", "The model workspace has no models yet. Make one with Evani.Model, or import tier files with: cargo xtask model-import <model> <folder>"));
        };
        load.model = Some(m.to_string());
        load.step = Step::History(self.fetches.request(format!("/model/v1/{m}/history")));
        Ok(())
    }

    fn on_history(&mut self, load: &mut Load, got: Result<Vec<u8>, String>) -> Result<(), ViewError> {
        let model = load.model.clone().unwrap_or_default();
        let bytes = got.map_err(|e| {
            if e.contains("404") { verr("unknown_model", format!("No model named {model}")) } else { verr("fetch_failed", format!("the history of {model} did not load ({e})")) }
        })?;
        let hist: Value = serde_json::from_slice(&bytes).map_err(|e| verr("bad_reply", format!("the history of {model} is not JSON ({e})")))?;
        let versions = hist.as_array().cloned().unwrap_or_default();
        let pick = match load.version {
            Some(n) => versions.iter().find(|v| v.get("version").and_then(Value::as_u64) == Some(n)).cloned(),
            None => versions.last().cloned(),
        };
        let Some(meta) = pick else {
            return Err(match load.version {
                Some(n) => verr("unknown_version", format!("{model} has no version {n}")),
                None => verr("unknown_model", format!("{model} has no versions yet")),
            });
        };
        let version = meta.get("version").and_then(Value::as_u64).unwrap_or(0);
        let Some(file) = meta.get("tiers").and_then(|t| t.get(&load.tier)).and_then(|t| t.get("file")).and_then(Value::as_str) else {
            return Err(verr("no_tier", format!("{model} version {version} has no {} tier", load.tier)));
        };
        load.version = Some(version);
        load.file = format!("/model/v1/{model}/v{version}/{file}");
        load.step = Step::File(self.fetches.request(load.file.clone()), meta.clone());
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn swap(&mut self, lib: &mut MeshLibrary, load: &Load, meta: Value, d: Decoded, frame: u64, now_ms: f64, frame_ms: f32, out: &mut Outcome) {
        // Meshes: reuse this viewer's slots, add more when needed, empty the rest.
        for (i, g) in d.groups.iter().enumerate() {
            match self.slots.get(i) {
                Some(&id) => {
                    lib.replace(id, g.mesh.clone());
                }
                None => match lib.add(g.mesh.clone()) {
                    Some(id) => self.slots.push(id),
                    None => out.log.push("model-view: out of mesh slots; some colours are not drawn".into()),
                },
            }
        }
        for &id in self.slots.iter().skip(d.groups.len()) {
            lib.replace(id, Mesh::default());
        }
        let model = load.model.clone().unwrap_or_default();
        let version = load.version.unwrap_or(0);
        let shown = Shown { model: model.clone(), version, tier: load.tier.clone(), meta, decoded: d };
        let first = self.shown.is_none();
        let (c, r) = (shown.decoded.center(), shown.decoded.radius());
        let reframe = (first && !self.camera_given) || !load.keep_camera || self.orbit.misses(c, r, self.aspect);
        if let Some(old) = self.shown.replace(shown)
            && (old.model != model || old.version != version)
        {
            self.previous = Some(old);
        }
        if reframe {
            self.frame();
        }
        self.status = Status::Shown;
        self.swaps += 1;
        self.swap_frame = frame;
        let ms = (now_ms - load.started_ms).max(0.0);
        out.log.push(format!(
            "model-view: swapped in {model} v{version} {} at frame {frame} ({} tris, camera {})",
            load.tier,
            self.shown.as_ref().map_or(0, |s| s.decoded.tris),
            if reframe { "framed" } else { "kept" }
        ));
        let loaded = json!({ "model": model, "version": version, "tier": load.tier });
        out.events.push(("view_loaded", json!({ "model": model, "version": version, "tier": load.tier, "ms": r2(ms as f32) })));
        out.events.push(("view_stats", self.stats(frame_ms)));
        if let Some(r) = load.reply.clone() {
            out.replies.push((r, Ok(json!({ "loaded": loaded, "stats": self.stats(frame_ms) }))));
        }
    }

    /// The meshes to draw: `(slot, colour)` per group of the shown model.
    pub fn instances(&self) -> Vec<(MeshId, [f32; 4])> {
        match &self.shown {
            Some(s) => s.decoded.groups.iter().zip(&self.slots).map(|(g, &id)| (id, g.color)).collect(),
            None => Vec::new(),
        }
    }
}

fn r2(x: f32) -> f64 {
    (x as f64 * 100.0).round() / 100.0
}

fn r3(x: f32) -> f64 {
    (x as f64 * 1000.0).round() / 1000.0
}
