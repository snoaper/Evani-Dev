//! Drop-to-convert, the page side (S3-14, docs/LIVE_API.md 16). Dev builds
//! of model games only (this crate's `models` feature, which a game's
//! `models` feature turns on with `evani-live?/models`).
//!
//! * `model.load { job, tier?, compare? }` hot-loads a finished conversion (the
//!   bridge's job record: `base_url`, `master`, `files`, `report`) into a
//!   focus view of its own, `live.model`, without a reload: the game is
//!   frozen and hidden, the model stands on a ground plane on the turntable.
//!   It answers once both files are on the GPU (or failed).
//! * The view is a before/after split: the original (`<id>.master.glb`) is
//!   drawn left of the line, the converted tier file right of it, in the
//!   same place and through the same camera (evani-render's `hooks`
//!   [`Split`] scissor).
//! * `model.compare { at?, tier?, split? }` moves the line, switches the tier
//!   file on the right, or shows the converted model whole.
//!
//! Focus is a view, not a change: no note, no history, never baked. A
//! `focus.clear` (or `asset.clear`) ends it and the game draws again.

use evani_core::ecs::World;
use evani_core::focus::{FocusFrame, FocusState};
use evani_core::plugin::LookData;
use evani_core::tiers::Tiers;
use evani_render::model::{ModelId, ModelInstance, ModelState, Models, Split};
use evani_render::{MeshId, MeshInstance, RenderWorld};
use serde_json::{Value, json};

use crate::LiveError;

/// The focus id of the converted-model view (not a kit asset).
pub const FOCUS_ID: &str = "live.model";

/// Frames `model.load` waits for its files before it answers `load_timeout`
/// (about 30 s at 60 frames a second).
pub const LOAD_FRAMES: u32 = 1800;

/// One tier file of a conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct TierFile {
    pub tier: String,
    pub file: String,
}

/// The conversion on show.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub hash: String,
    pub id: String,
    pub base_url: String,
    pub master: Option<String>,
    pub tiers: Vec<TierFile>,
    /// The tier file on the right.
    pub tier: String,
    pub original: Option<ModelId>,
    pub converted: Option<ModelId>,
    /// The bridge's report summary (the panel's numbers).
    pub report: Value,
}

#[derive(Debug, Clone)]
struct Waiter {
    token: u64,
    id: Value,
    frames_left: u32,
}

/// The split view's state (a resource while live is in the build).
#[derive(Debug, Clone)]
pub struct CompareView {
    pub shown: Option<Shown>,
    /// The original's share of the width, from the left (0..1).
    pub at: f32,
    /// Off: the converted model draws whole.
    pub split: bool,
    /// Frames the view has drawn since the last `model.load`.
    pub frames: u64,
    waiting: Vec<Waiter>,
    /// Files already in the model table (a model is never fetched twice).
    loaded: Vec<(String, ModelId)>,
}

impl Default for CompareView {
    fn default() -> Self {
        Self { shown: None, at: 0.5, split: true, frames: 0, waiting: Vec::new(), loaded: Vec::new() }
    }
}

impl CompareView {
    /// Is the view on screen (its focus is the active one)?
    pub fn active(&self, world: &World) -> bool {
        self.shown.is_some() && world.try_res::<FocusState>().is_some_and(|f| f.active.as_deref() == Some(FOCUS_ID))
    }

    fn model(&mut self, models: &mut Models, url: &str) -> ModelId {
        if let Some((_, id)) = self.loaded.iter().find(|(u, _)| u == url) {
            return *id;
        }
        let id = models.load(url);
        self.loaded.push((url.to_string(), id));
        id
    }
}

fn bad(msg: impl Into<String>) -> LiveError {
    LiveError::new("invalid_value", msg)
}

/// A file name the bridge listed: plain relative names only.
fn safe_file(f: &str) -> bool {
    !f.is_empty() && !f.starts_with('/') && !f.contains('\\') && !f.contains("..") && !f.contains(':')
}

/// The longest file name the drop zone accepts (the same cap as evani-convert).
pub const MAX_NAME: usize = 255;

/// A dropped file's name, checked before anything is sent: the bridge names
/// the job's folder by the content hash and never by this, so the name is
/// only ever a label; still, a name with a path in it, a control character
/// or an unknown extension is refused here (a `.gltf` dropped on its own
/// has no buffers, so it is the zip that carries one).
pub fn accept_name(name: &str) -> Result<&str, LiveError> {
    let name = name.trim();
    let bad_ext = || {
        LiveError::new("unsupported", "Drop a .glb, a .blend, or a .zip holding one .gltf and its buffers.")
            .with(json!({ "file": name }))
    };
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(bad("the file has no usable name").with(json!({ "file": name })));
    }
    if name.starts_with('.') || name.contains(['/', '\\', ':', '\0']) || name.chars().any(char::is_control) {
        return Err(bad("the file name has a path in it").with(json!({ "file": name })));
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if !["glb", "blend", "zip"].contains(&ext.as_str()) {
        return Err(bad_ext());
    }
    Ok(name)
}

/// The bridge's upload URL for a file: `/live/v1/convert`, with `id` (the
/// file's stem, already through [`crate::convert::accept_name`]) as the query
/// when the job may use it. Only `[a-z0-9_-]` ids are sent; anything else is
/// left to the bridge's `model` default.
pub fn upload_url(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let id: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .take(80)
        .collect();
    let id = id.trim_matches(['_', '-']);
    if id.is_empty() || id.starts_with(['_', '-']) {
        "/live/v1/convert".to_string()
    } else {
        format!("/live/v1/convert?id={id}")
    }
}

/// The most the page sends in one drop (the bridge's own cap).
pub const UPLOAD_CAP: f64 = 256.0 * 1024.0 * 1024.0;
/// Bytes read for the type check: the longest of the three signatures.
pub const HEAD_BYTES: u64 = 16;

/// A dropped file's kind, by its first bytes (the bridge's rule, here so a
/// wrong file is named before anything is sent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed {
    Glb,
    Zip,
    Blend,
}

impl Sniffed {
    /// `glTF`, a zip, or a `.blend` in its plain, zstd or gzip form.
    pub fn of(b: &[u8]) -> Option<Self> {
        if b.starts_with(b"glTF") {
            Some(Self::Glb)
        } else if b.starts_with(b"PK\x03\x04") {
            Some(Self::Zip)
        } else if b.starts_with(b"BLENDER") || b.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) || b.starts_with(&[0x1F, 0x8B]) {
            Some(Self::Blend)
        } else {
            None
        }
    }
}

/// Everything a dropped file must pass before it is sent: its checked name,
/// its size against the cap and its first bytes.
pub fn accept_drop(name: &str, size: f64, head: &[u8]) -> Result<String, LiveError> {
    let name = accept_name(name)?.to_string();
    if size <= 0.0 {
        return Err(LiveError::new("unsupported", "That file is empty.").with(json!({ "file": name })));
    }
    if size > UPLOAD_CAP {
        return Err(LiveError::new("too_large", "That file is over 256 MiB; convert a smaller one.")
            .with(json!({ "file": name, "size": size, "cap": UPLOAD_CAP })));
    }
    if Sniffed::of(head).is_none() {
        return Err(LiveError::new(
            "unsupported",
            "That isn't a model file: drop a .glb, a .blend, or a .zip holding one .gltf and its buffers.",
        )
        .with(json!({ "file": name })));
    }
    Ok(name)
}

/// The zone's sentence for a failure: the error's message, naming the file
/// when the error carries one.
pub fn sentence(e: &LiveError) -> String {
    let file = e.data.as_ref().and_then(|d| d["file"].as_str()).filter(|f| !f.is_empty());
    match file {
        Some(f) => format!("{f}: {}", e.message),
        None => e.message.clone(),
    }
}

/// The error codes the page can get back from `model.*` calls and the
/// bridge (LIVE_API.md 5.5 and 16); [`LiveError::code`] is `&'static str`,
/// so a code from the page's JS side is mapped onto this list, anything
/// else reading as `internal`.
pub const PAGE_CODES: &[&str] = &[
    "unauthorized",
    "forbidden_origin",
    "too_large",
    "unsupported",
    "busy",
    "unsafe_zip",
    "bad_request",
    "no_model",
    "load_failed",
    "load_timeout",
    "invalid_value",
    "not_compiled_in",
    "convert_failed",
    "no_report",
    "not_found",
    "no_engine",
    "internal",
];

/// A code string from the page mapped onto [`PAGE_CODES`].
pub fn page_code(code: &str) -> &'static str {
    PAGE_CODES.iter().copied().find(|c| *c == code).unwrap_or("internal")
}

/// A `convert_done` event's job record, checked before `model.load` uses
/// it: a hash, the bridge's base URL for that hash, and at least one file.
pub fn job_from_done(data: &Value) -> Result<Value, LiveError> {
    let hash = data["hash"].as_str().filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit())).ok_or_else(|| {
        LiveError::new("internal", "the bridge's convert_done has no hash").with(data.clone())
    })?;
    let base = data["base_url"].as_str().unwrap_or_default();
    if !base.starts_with(&format!("/live/v1/convert/{hash}/")) {
        return Err(LiveError::new("internal", "the bridge's convert_done names another folder").with(data.clone()));
    }
    let files = data["files"].as_array().cloned().unwrap_or_default();
    if files.is_empty() {
        let log = data["log"].as_str().unwrap_or_default();
        return Err(LiveError::new("convert_failed", if log.is_empty() { "the conversion wrote no files".to_string() } else { format!("the conversion wrote no files:\n{log}") })
            .with(data.clone()));
    }
    Ok(data.clone())
}

/// The report panel's lines from a `convert_done` record's `report`
/// summary: the package size against the cap, then each tier's size and
/// gate score. Plain text, so the page shows it in a `<pre>`.
pub fn report_lines(report: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let pkg = &report["package"];
    if let (Some(raw), Some(cap)) = (pkg["raw"].as_u64(), pkg["cap"].as_u64()) {
        let flag = if pkg["over_cap"].as_bool() == Some(true) { " (over the cap)" } else { "" };
        out.push(format!("package {} of {}{flag}", bytes(raw), bytes(cap)));
    }
    for t in report["tiers"].as_array().into_iter().flatten() {
        let tier = t["tier"].as_str().unwrap_or("?");
        let size = t["bytes"]["gzip"].as_u64().or_else(|| t["bytes"]["raw"].as_u64()).map_or("?".to_string(), bytes);
        let tris = t["triangles"].as_u64().map_or(String::new(), |n| format!(" · {n} tris"));
        let score = match t["gate"]["pass"].as_bool() {
            Some(true) => t["gate"]["ssimulacra2"].as_f64().map_or(" · gate passed".to_string(), |s| format!(" · gate passed ({s:.1})")),
            Some(false) => " · gate failed".to_string(),
            None => String::new(),
        };
        out.push(format!("{tier}: {size}{tris}{score}"));
    }
    if out.is_empty() {
        out.push("no report (the conversion wrote files but no report.json)".to_string());
    }
    out
}

fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.0} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// The tier files of a job record: the report's tiers, else every listed
/// `<id>.<tier>.glb` but the master.
pub fn tier_files(job: &Value) -> Vec<TierFile> {
    let id = job["id"].as_str().unwrap_or("");
    let mut out: Vec<TierFile> = job["report"]["tiers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let tier = t["tier"].as_str()?;
            let file = t["file"].as_str()?;
            safe_file(file).then(|| TierFile { tier: tier.to_string(), file: file.to_string() })
        })
        .collect();
    if out.is_empty() {
        let prefix = format!("{id}.");
        for f in job["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let Some(tier) = f.strip_prefix(&prefix).and_then(|r| r.strip_suffix(".glb")) else {
                continue;
            };
            if tier != "master" && !tier.is_empty() && !tier.contains('.') && !tier.contains('/') && safe_file(f) {
                out.push(TierFile { tier: tier.to_string(), file: f.to_string() });
            }
        }
    }
    out
}

/// The tier to show first: the asked one, else the game's active tier, else
/// `high`, else the first file.
fn pick_tier(tiers: &[TierFile], asked: Option<&str>, active: Option<&str>) -> Result<String, LiveError> {
    if let Some(a) = asked {
        return tiers
            .iter()
            .find(|t| t.tier == a)
            .map(|t| t.tier.clone())
            .ok_or_else(|| bad(format!("tier {a} has no file in this conversion")).with(json!({ "tiers": tiers.iter().map(|t| t.tier.as_str()).collect::<Vec<_>>() })));
    }
    for want in [active, Some("high")].into_iter().flatten() {
        if let Some(t) = tiers.iter().find(|t| t.tier == want) {
            return Ok(t.tier.clone());
        }
    }
    tiers.first().map(|t| t.tier.clone()).ok_or_else(|| LiveError::new("no_model", "the conversion wrote no tier file"))
}

fn url_of(base: &str, file: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), file)
}

fn state_json(models: &Models, id: Option<ModelId>) -> Value {
    let Some(id) = id else { return Value::Null };
    let state = match models.state(id) {
        Some(ModelState::Ready) => json!("ready"),
        Some(ModelState::Loading) | None => json!("loading"),
        Some(ModelState::Failed(e)) => json!({ "failed": e }),
    };
    json!({ "url": models.url(id), "state": state, "triangles": models.triangles(id) })
}

/// The view as JSON (`model.load` and `model.compare` results).
pub fn view_json(world: &World) -> Value {
    let Some(cv) = world.try_res::<CompareView>() else {
        return json!({ "shown": null });
    };
    let Some(s) = &cv.shown else {
        return json!({ "shown": null, "at": cv.at, "split": cv.split });
    };
    let models = world.try_res::<Models>();
    let side = |id| models.as_deref().map_or(Value::Null, |m| state_json(m, id));
    json!({
        "shown": { "hash": s.hash, "id": s.id },
        "focused": cv.active(world),
        "tier": s.tier,
        "tiers": s.tiers.iter().map(|t| json!({ "tier": t.tier, "file": t.file })).collect::<Vec<_>>(),
        "original": side(s.original),
        "converted": side(s.converted),
        "at": cv.at,
        "split": cv.split,
        "frames": cv.frames,
        "report": s.report,
    })
}

/// `model.load { job, tier?, compare? }` (`compare` false: the converted
/// model whole, no split). `Ok(None)`: the answer comes once the files
/// are loaded ([`frame`]).
pub fn load(world: &World, token: u64, id: &Value, params: &Value) -> Result<Option<Value>, LiveError> {
    let job = params.get("job").filter(|j| j.is_object()).ok_or_else(|| {
        LiveError::new("no_model", "model.load needs a finished conversion: drop a model, or run model.convert first")
            .with(json!({ "field": "job" }))
    })?;
    let hash = job["hash"].as_str().filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit())).ok_or_else(|| bad("job.hash must be a convert hash"))?;
    let mid = job["id"].as_str().unwrap_or("model").to_string();
    let base = job["base_url"].as_str().map_or_else(|| format!("/live/v1/convert/{hash}/"), str::to_string);
    if !base.starts_with("/live/v1/convert/") || base.contains("..") {
        return Err(bad("job.base_url must be the bridge's /live/v1/convert/<hash>/"));
    }
    let tiers = tier_files(job);
    let active = world.try_res::<Tiers>().map(|t| t.active().id.clone());
    let tier = pick_tier(&tiers, params.get("tier").and_then(Value::as_str), active.as_deref())?;
    let master = job["master"].as_str().filter(|m| safe_file(m)).map(str::to_string);
    // `compare` (default on): the before/after split; off shows the
    // converted model whole.
    let compare = match params.get("compare") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or_else(|| bad("compare must be true or false").with(json!({ "field": "compare" })))?,
    };
    let Some(mut models) = world.try_res_mut::<Models>() else {
        return Err(LiveError::new("internal", "this build has no model table"));
    };
    if !world.has_resource::<FocusState>() || !world.has_resource::<CompareView>() {
        return Err(LiveError::new("internal", "focus state not installed"));
    }
    let mut cv = world.res_mut::<CompareView>();
    let file = tiers.iter().find(|t| t.tier == tier).map(|t| t.file.clone()).unwrap_or_default();
    let converted = Some(cv.model(&mut models, &url_of(&base, &file)));
    let original = master.as_ref().map(|m| cv.model(&mut models, &url_of(&base, m)));
    cv.shown = Some(Shown { hash: hash.to_string(), id: mid, base_url: base, master, tiers, tier, original, converted, report: job["report"].clone() });
    cv.split = compare;
    cv.frames = 0;
    cv.waiting.push(Waiter { token, id: id.clone(), frames_left: LOAD_FRAMES });
    drop((cv, models));
    let mut st = world.res_mut::<FocusState>();
    if st.active.as_deref() != Some(FOCUS_ID) {
        st.focus(FOCUS_ID);
        // The game holds still behind the model; the turntable keeps turning.
        st.paused = true;
    }
    Ok(None)
}

/// `model.compare { at?, tier?, split? }`: everything is checked first.
pub fn compare(world: &World, params: &Value) -> Result<Value, LiveError> {
    let at = match params.get("at") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|x| (0.0..=1.0).contains(x)).ok_or_else(|| bad("at must be a number from 0 (all converted) to 1 (all original)").with(json!({ "field": "at", "min": 0, "max": 1 })))? as f32),
    };
    let split = match params.get("split") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad("split must be true or false"))?),
    };
    let tier = match params.get("tier") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().ok_or_else(|| bad("tier must be a tier id"))?.to_string()),
    };
    {
        let Some(mut cv) = world.try_res_mut::<CompareView>() else {
            return Err(LiveError::new("internal", "compare view not installed"));
        };
        if cv.shown.is_none() {
            return Err(LiveError::new("no_model", "nothing to compare: model.load a conversion first"));
        }
        if let Some(t) = &tier {
            let s = cv.shown.as_ref().map(|s| (s.tiers.clone(), s.base_url.clone()));
            let (tiers, base) = s.unwrap_or_default();
            let picked = pick_tier(&tiers, Some(t), None)?;
            let file = tiers.iter().find(|x| x.tier == picked).map(|x| x.file.clone()).unwrap_or_default();
            let mut models = world.try_res_mut::<Models>().ok_or_else(|| LiveError::new("internal", "this build has no model table"))?;
            let id = cv.model(&mut models, &url_of(&base, &file));
            if let Some(s) = cv.shown.as_mut() {
                s.tier = picked;
                s.converted = Some(id);
            }
        }
        if let Some(a) = at {
            cv.at = a;
        }
        if let Some(s) = split {
            cv.split = s;
        }
    }
    Ok(view_json(world))
}

/// Answer the `model.load` calls whose files are in (or failed, or ran out
/// of frames).
fn answer(world: &World) {
    let ready = {
        let Some(mut cv) = world.try_res_mut::<CompareView>() else { return };
        if cv.waiting.is_empty() {
            return;
        }
        let models = world.try_res::<Models>();
        let settled = |id: Option<ModelId>| id.is_none_or(|id| models.as_deref().is_none_or(|m| !matches!(m.state(id), Some(ModelState::Loading))));
        let (o, c) = cv.shown.as_ref().map_or((None, None), |s| (s.original, s.converted));
        let done = settled(o) && settled(c);
        let failed = c.and_then(|c| match models.as_deref().and_then(|m| m.state(c)) {
            Some(ModelState::Failed(e)) => Some(e.clone()),
            _ => None,
        });
        drop(models);
        let mut out = Vec::new();
        cv.waiting.retain_mut(|w| {
            w.frames_left = w.frames_left.saturating_sub(1);
            if done || w.frames_left == 0 {
                out.push((w.token, w.id.clone(), done, failed.clone()));
                false
            } else {
                true
            }
        });
        out
    };
    for (token, id, done, failed) in ready {
        let res = match (done, failed) {
            (_, Some(e)) => Err(LiveError::new("load_failed", format!("the converted model didn't load: {e}")).with(view_json(world))),
            (true, None) => Ok(view_json(world)),
            (false, None) => Err(LiveError::new("load_timeout", "the model files didn't arrive in time").with(view_json(world))),
        };
        crate::commands::reply(world, token, &id, res);
    }
}

/// The split view's frame system (last but one in the frame stage, after
/// every kit has drawn): while `live.model` is focused, the frame shows only
/// the ground and the model, original left of the line and converted right.
pub fn frame(world: &mut World) {
    answer(world);
    let on = world.try_res::<CompareView>().is_some_and(|cv| cv.active(world));
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    if !on {
        models.split = None;
        return;
    }
    let Some(mut cv) = world.try_res_mut::<CompareView>() else { return };
    let Some(s) = cv.shown.clone() else { return };
    let ready = |id: Option<ModelId>| id.filter(|i| models.ready(*i));
    let (orig, conv) = (ready(s.original), ready(s.converted));
    // The kit's draws this frame give way to the model.
    models.draws.clear();
    let sphere = conv.or(orig).and_then(|i| models.sphere(i)).unwrap_or(([0.0, 0.5, 0.0], 0.5));
    models.split = match (orig, conv) {
        (Some(left), Some(right)) if cv.split => Some(Split { at: cv.at, left, right }),
        _ => None,
    };
    let both_split = models.split.is_some();
    for m in [conv, if both_split || conv.is_none() { orig } else { None }].into_iter().flatten() {
        models.draw(ModelInstance::new(m, [0.0; 3]));
    }
    cv.frames += 1;
    drop((cv, models));
    let ground = world.try_res::<LookData>().map_or(0, |l| RenderWorld::material(&l, "ground"));
    let pose = {
        let mut st = world.res_mut::<FocusState>();
        st.frame(FocusFrame { center: sphere.0, radius: sphere.1.max(0.05), front_yaw_deg: 0.0 })
    };
    let mut rw = world.res_mut::<RenderWorld>();
    rw.clear();
    rw.camera = pose;
    let mut g = MeshInstance::new(MeshId::PLANE, [sphere.0[0], 0.0, sphere.0[2]]);
    let span = (sphere.1 * 8.0).max(4.0);
    g.scale = [span, 1.0, span];
    g.material = ground;
    g.color = [0.55, 0.56, 0.58, 1.0];
    rw.draw_mesh(g);
    rw.blobs.push([sphere.0[0], 0.0, sphere.0[2], sphere.1 * 0.8]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_files_come_from_the_report_or_the_file_list() {
        let job = json!({ "id": "kart", "report": { "tiers": [{ "tier": "low", "file": "kart.low.glb" }, { "tier": "high", "file": "kart.glb" }] } });
        let t = tier_files(&job);
        assert_eq!(t.len(), 2);
        assert_eq!(t[1], TierFile { tier: "high".into(), file: "kart.glb".into() });
        let job = json!({ "id": "kart", "report": null, "files": ["kart.master.glb", "kart.low.glb", "kart.ultra.glb", "../x.glb", "kart.report.json"] });
        let t: Vec<String> = tier_files(&job).into_iter().map(|t| t.tier).collect();
        assert_eq!(t, ["low", "ultra"]);
        let evil = json!({ "id": "k", "report": { "tiers": [{ "tier": "low", "file": "../../secret.glb" }] } });
        assert!(tier_files(&evil).is_empty());
    }

    #[test]
    fn the_first_tier_follows_the_game() {
        let t = vec![TierFile { tier: "low".into(), file: "a".into() }, TierFile { tier: "high".into(), file: "b".into() }];
        assert_eq!(pick_tier(&t, None, Some("low")).unwrap(), "low");
        assert_eq!(pick_tier(&t, None, Some("ultra")).unwrap(), "high");
        assert_eq!(pick_tier(&t, Some("low"), Some("high")).unwrap(), "low");
        assert_eq!(pick_tier(&t, Some("ultra"), None).unwrap_err().code, "invalid_value");
        assert_eq!(pick_tier(&[], None, None).unwrap_err().code, "no_model");
    }

    // ---- the drop zone's checks (the page runs them before it sends) ----

    const GLB: &[u8] = b"glTF\x02\0\0\0 a model";

    #[test]
    fn a_dropped_name_is_checked_before_anything_is_sent() {
        assert_eq!(accept_drop("kart.glb", 12.0, GLB).unwrap(), "kart.glb");
        assert_eq!(accept_drop(" Kart Blender.BLEND ", 12.0, b"BLENDER-v1").unwrap(), "Kart Blender.BLEND");
        // A path in the name, a hidden file, a drive letter, a backslash: refused.
        for bad in ["../kart.glb", "a/b.glb", "C:kart.glb", ".kart.glb", "", "a\\b.glb"] {
            assert_eq!(accept_drop(bad, 12.0, GLB).unwrap_err().code, "invalid_value", "{bad}");
        }
        // A name longer than the bridge's cap.
        let long = format!("{}.glb", "a".repeat(MAX_NAME));
        assert!(accept_drop(&long, 12.0, GLB).is_err());
        // The wrong kind of file, by extension and by its bytes.
        assert_eq!(accept_drop("kart.gltf", 12.0, GLB).unwrap_err().code, "unsupported");
        assert_eq!(accept_drop("kart.png", 12.0, b"\x89PNG\r\n").unwrap_err().code, "unsupported");
        let e = accept_drop("renamed.glb", 12.0, b"\x89PNG\r\n").unwrap_err();
        assert_eq!(e.code, "unsupported");
        assert!(e.message.contains("isn't a model file"), "{}", e.message);
    }

    #[test]
    fn an_empty_or_oversize_file_is_refused() {
        assert_eq!(accept_drop("kart.glb", 0.0, GLB).unwrap_err().code, "unsupported");
        let e = accept_drop("kart.glb", UPLOAD_CAP + 1.0, GLB).unwrap_err();
        assert_eq!(e.code, "too_large");
        assert!(e.message.contains("over 256 MiB"), "{}", e.message);
        // Exactly the cap is allowed (the bridge allows it too).
        assert_eq!(accept_drop("kart.glb", UPLOAD_CAP, GLB).unwrap(), "kart.glb");
    }

    #[test]
    fn the_kind_comes_from_the_first_bytes() {
        assert_eq!(Sniffed::of(GLB), Some(Sniffed::Glb));
        assert_eq!(Sniffed::of(b"PK\x03\x04rest"), Some(Sniffed::Zip));
        assert_eq!(Sniffed::of(b"BLENDER-v300"), Some(Sniffed::Blend));
        assert_eq!(Sniffed::of(&[0x28, 0xB5, 0x2F, 0xFD]), Some(Sniffed::Blend));
        assert_eq!(Sniffed::of(&[0x1F, 0x8B]), Some(Sniffed::Blend));
        assert_eq!(Sniffed::of(b"<html>"), None);
        assert_eq!(Sniffed::of(b""), None);
        // The longest signature fits in the bytes the page reads.
        assert!(HEAD_BYTES as usize >= b"BLENDER".len());
    }

    #[test]
    fn the_sentence_names_the_file() {
        let e = accept_drop("kart.png", 12.0, GLB).unwrap_err();
        assert_eq!(sentence(&e), format!("kart.png: {}", e.message));
        // An error with no file still reads as a sentence.
        let e = LiveError::new("internal", "the page has no window");
        assert_eq!(sentence(&e), "the page has no window");
    }

    #[test]
    fn a_wrong_file_says_what_to_drop() {
        let e = accept_drop("photo.jpg", 12.0, b"\xff\xd8\xff").unwrap_err();
        assert!(e.message.contains(".glb") && e.message.contains(".blend") && e.message.contains(".zip"));
    }

    #[test]
    fn accept_name_takes_plain_model_names_only() {
        assert_eq!(accept_name("  crate.zip\t").unwrap(), "crate.zip");
        assert_eq!(accept_name("Crate.GLB").unwrap(), "Crate.GLB");
        assert_eq!(accept_name(&format!("{}.glb", "a".repeat(MAX_NAME - 4))).unwrap().len(), MAX_NAME);
        assert_eq!(accept_name(&format!("{}.glb", "a".repeat(MAX_NAME - 3))).unwrap_err().code, "invalid_value");
        for bad in ["a\0.glb", "a\u{7}.glb", "/abs.glb", "dir\\x.glb", "C:\\x.glb", ".glb", "   "] {
            assert_eq!(accept_name(bad).unwrap_err().code, "invalid_value", "{bad:?}");
        }
        for wrong in ["noext", "model.gltf", "model.fbx", "model.glb.exe"] {
            let e = accept_name(wrong).unwrap_err();
            assert_eq!(e.code, "unsupported", "{wrong}");
            assert_eq!(e.data.as_ref().and_then(|d| d["file"].as_str()), Some(wrong));
        }
    }

    #[test]
    fn upload_url_sends_only_a_safe_id() {
        assert_eq!(upload_url("kart.glb"), "/live/v1/convert?id=kart");
        assert_eq!(upload_url("Kart Blender.BLEND"), "/live/v1/convert?id=kart_blender");
        assert_eq!(upload_url("my-crate_02.zip"), "/live/v1/convert?id=my-crate_02");
        // Leading and trailing separators go; nothing usable leaves no id.
        assert_eq!(upload_url("__kart__.glb"), "/live/v1/convert?id=kart");
        assert_eq!(upload_url("ünïcødé.glb"), "/live/v1/convert?id=n_c_d");
        assert_eq!(upload_url("___.glb"), "/live/v1/convert");
        assert_eq!(upload_url("&id=x?.glb"), "/live/v1/convert?id=id_x");
        // At most 80 characters of id.
        let long = upload_url(&format!("{}.glb", "a".repeat(200)));
        assert_eq!(long.len(), "/live/v1/convert?id=".len() + 80);
        // Only `[a-z0-9_-]` ever reaches the query.
        let url = upload_url("A b/c\\d%e&f=g#h.glb");
        let q = url.trim_start_matches("/live/v1/convert?id=");
        assert!(q.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'), "{url}");
    }

    #[test]
    fn job_from_done_checks_the_hash_folder_and_files() {
        let good = json!({ "hash": "ab12", "id": "kart", "base_url": "/live/v1/convert/ab12/", "files": ["kart.glb"] });
        assert_eq!(job_from_done(&good).unwrap(), good);
        let no_hash = json!({ "base_url": "/live/v1/convert/ab12/", "files": ["kart.glb"] });
        assert_eq!(job_from_done(&no_hash).unwrap_err().code, "internal");
        let bad_hash = json!({ "hash": "../x", "base_url": "/live/v1/convert/../x/", "files": ["kart.glb"] });
        assert_eq!(job_from_done(&bad_hash).unwrap_err().code, "internal");
        let other = json!({ "hash": "ab12", "base_url": "/live/v1/convert/cd34/", "files": ["kart.glb"] });
        assert!(job_from_done(&other).unwrap_err().message.contains("another folder"));
        let elsewhere = json!({ "hash": "ab12", "base_url": "http://evil.example/ab12/", "files": ["kart.glb"] });
        assert_eq!(job_from_done(&elsewhere).unwrap_err().code, "internal");
        let empty = json!({ "hash": "ab12", "base_url": "/live/v1/convert/ab12/", "files": [], "log": "blender: no mesh" });
        let e = job_from_done(&empty).unwrap_err();
        assert_eq!(e.code, "convert_failed");
        assert!(e.message.contains("blender: no mesh"), "{}", e.message);
    }

    #[test]
    fn report_lines_read_the_package_and_each_tier() {
        let report = json!({
            "package": { "raw": 2_621_440, "cap": 10_485_760, "over_cap": false },
            "tiers": [
                { "tier": "low", "bytes": { "gzip": 40_960, "raw": 90_000 }, "triangles": 1200, "gate": { "pass": true, "ssimulacra2": 82.44 } },
                { "tier": "high", "bytes": { "raw": 900 }, "gate": { "pass": false } },
                { "tier": "ultra" },
            ],
        });
        assert_eq!(
            report_lines(&report),
            [
                "package 2.5 MiB of 10.0 MiB",
                "low: 40 KiB · 1200 tris · gate passed (82.4)",
                "high: 900 B · gate failed",
                "ultra: ?",
            ]
        );
        let over = json!({ "package": { "raw": 11_534_336, "cap": 10_485_760, "over_cap": true } });
        assert_eq!(report_lines(&over), ["package 11.0 MiB of 10.0 MiB (over the cap)"]);
        assert_eq!(report_lines(&Value::Null), ["no report (the conversion wrote files but no report.json)"]);
    }

    #[test]
    fn page_codes_map_onto_the_known_list() {
        assert_eq!(page_code("unauthorized"), "unauthorized");
        assert_eq!(page_code("load_timeout"), "load_timeout");
        assert_eq!(page_code("something_new"), "internal");
        assert_eq!(page_code(""), "internal");
    }

    // ---- hot-load on a running app ----

    fn kart_bytes() -> Vec<u8> {
        let path = format!("{}/../../tests/fixtures/models/kart.low.glb", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap()
    }

    fn frame_of(a: &evani_core::App) -> u64 {
        a.world.res::<evani_core::time::Time>().frame
    }

    #[test]
    fn hot_load_keeps_the_frame_counter_running() {
        use crate::{LiveModule, submit, take_replies};
        let mut a = evani_core::App::new("sandbox").with_module(LiveModule::new());
        a.world.insert_resource(RenderWorld::default());
        a.world.insert_resource(Models::default());
        a.run_headless(3, 16.0);
        let before = frame_of(&a);
        assert!(before > 0);

        let job = json!({
            "hash": "ab12cd", "id": "kart", "base_url": "/live/v1/convert/ab12cd/",
            "master": "kart.master.glb", "files": ["kart.master.glb", "kart.low.glb"],
            "report": { "tiers": [{ "tier": "low", "file": "kart.low.glb" }] },
        });
        let req = json!({ "type": "cmd", "v": 1, "id": "m-1", "cmd": "model.load", "params": { "job": job } });
        submit(&a.world, 1, &req.to_string());
        a.run_headless(2, 16.0);
        // The files are on their way: no answer yet, the game keeps counting
        // frames (no reload), and the view is focused.
        assert!(take_replies(&a.world).is_empty());
        assert!(frame_of(&a) >= before + 2);
        let (orig, conv) = {
            let cv = a.world.res::<CompareView>();
            assert!(cv.active(&a.world));
            assert!(cv.split);
            let s = cv.shown.as_ref().unwrap();
            assert_eq!(s.tier, "low");
            (s.original.unwrap(), s.converted.unwrap())
        };
        {
            let models = a.world.res::<Models>();
            assert_eq!(models.url(orig), Some("/live/v1/convert/ab12cd/kart.master.glb"));
            assert_eq!(models.url(conv), Some("/live/v1/convert/ab12cd/kart.low.glb"));
        }

        // The browser's fetch and upload, natively.
        let bytes = kart_bytes();
        {
            let mut models = a.world.res_mut::<Models>();
            models.arrived(orig, &bytes);
            models.arrived(conv, &bytes);
        }
        let at_arrival = frame_of(&a);
        a.run_headless(1, 16.0);
        let replies = take_replies(&a.world);
        assert_eq!(replies.len(), 1, "{replies:?}");
        let r: Value = serde_json::from_str(&replies[0].1).unwrap();
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["result"]["converted"]["state"], "ready", "{r}");
        assert_eq!(r["result"]["original"]["state"], "ready", "{r}");
        assert_eq!(r["result"]["split"], true);

        // The split view draws every frame, and the frame counter runs on.
        let drawn = a.world.res::<CompareView>().frames;
        a.run_headless(5, 16.0);
        assert_eq!(frame_of(&a), at_arrival + 6);
        assert!(a.world.res::<CompareView>().frames >= drawn + 5);
        assert_eq!(a.world.res::<Models>().split, Some(Split { at: 0.5, left: orig, right: conv }));

        // Move the line, then show the converted model whole.
        let v = compare(&a.world, &json!({ "at": 0.25 })).unwrap();
        assert_eq!(v["at"], 0.25);
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<Models>().split.map(|s| s.at), Some(0.25));
        compare(&a.world, &json!({ "split": false })).unwrap();
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<Models>().split, None);
        assert_eq!(compare(&a.world, &json!({ "at": 2 })).unwrap_err().code, "invalid_value");
        assert_eq!(compare(&a.world, &json!({ "split": 1 })).unwrap_err().code, "invalid_value");
    }

    #[test]
    fn model_load_with_compare_off_shows_the_converted_model_whole() {
        use crate::{LiveModule, submit};
        let mut a = evani_core::App::new("sandbox").with_module(LiveModule::new());
        a.world.insert_resource(RenderWorld::default());
        a.world.insert_resource(Models::default());
        a.run_headless(1, 16.0);
        let job = json!({ "hash": "ab12", "id": "kart", "files": ["kart.master.glb", "kart.low.glb"], "master": "kart.master.glb" });
        let req = json!({ "type": "cmd", "v": 1, "id": "m-2", "cmd": "model.load", "params": { "job": job, "compare": false } });
        submit(&a.world, 2, &req.to_string());
        a.run_headless(1, 16.0);
        assert!(!a.world.res::<CompareView>().split);
        let bad = json!({ "type": "cmd", "v": 1, "id": "m-3", "cmd": "model.load", "params": { "job": job, "compare": "yes" } });
        submit(&a.world, 3, &bad.to_string());
        a.run_headless(1, 16.0);
        let r = crate::take_replies(&a.world);
        let r: Value = serde_json::from_str(&r.iter().find(|(t, _)| *t == 3).unwrap().1).unwrap();
        assert_eq!(r["error"]["code"], "invalid_value", "{r}");
    }
}
