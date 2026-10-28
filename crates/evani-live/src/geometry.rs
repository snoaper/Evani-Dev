//! The geometry debug view (`debug.geometry`, docs/LIVE_API.md 6.27): the
//! geocheck library's overlap, floating and z-fighting checks run on what
//! the game drew this frame, and the view paints the result into the scene
//! (overlaps red, floating objects yellow, z-fighting cyan, everything else
//! grey) with the sky in magenta, so gaps and cracks show as magenta.
//!
//! Live only: this module is part of evani-live, which shipped games never
//! contain. The renderer honours the magenta sky (`clear_override`) and the
//! focus camera (`camera_override`) only under its `capture` feature, which
//! only the `live` feature turns on.
//!
//! Kits push their draws into `RenderWorld` every frame and the renderer
//! clears it after drawing, so [`frame`] runs last in `Stage::Frame`: it
//! copies the frame's instances, re-analyses them at most every `every_s`
//! seconds, and tints them before the renderer draws.

use std::collections::HashMap;

use evani_core::camera::CameraPose;
use evani_core::capture::CaptureQueue;
use evani_core::ecs::World;
use evani_core::plugin::LookData;
use evani_core::time::Time;
use evani_geocheck::checks;
use evani_geocheck::geom::{Aabb, V3};
use evani_geocheck::image::Camera;
use evani_geocheck::report::{Check, Issue};
use evani_geocheck::scene::{self, Config, Scene};
use evani_geocheck::shape::Shapes;
use evani_geocheck::tags::{self, Tag};
use evani_geocheck::zfight::{self, DepthSetup, Viewer};
use evani_render::mesh::{BUILTIN_MESH_COUNT, MeshId, MeshLibrary};
use evani_render::world::{MeshInstance, RenderWorld};
use serde_json::{Map, Value, json};

use crate::LiveError;

/// The checks the live view runs (the rest need several frames, captures
/// or a kit adapter: `cargo xtask geocheck <app>`).
pub const VIEW_CHECKS: [Check; 3] = [Check::Overlap, Check::Floating, Check::ZFight];

/// Linear RGB of the sky while the view's `sky` is on (gaps show magenta).
pub const SKY: [f32; 3] = [1.0, 0.0, 1.0];

/// Colour of everything without an issue.
pub const GREY: [f32; 3] = [0.55, 0.55, 0.55];

/// Most issues a `debug.geometry` reply lists (`total` counts them all).
pub const MAX_LISTED: usize = 200;

/// A `debug.geometry` request waiting for the next analysis.
#[derive(Debug, Clone)]
struct PendingReply {
    token: u64,
    id: Value,
}

/// The view's state (a world resource the live module inserts).
#[derive(Debug)]
pub struct GeometryView {
    /// Tint the scene (and paint the sky when `sky`).
    pub on: bool,
    pub checks: Vec<Check>,
    pub sky: bool,
    /// Issue index (in [`GeometryView::issues`]) the camera frames.
    pub focus: Option<usize>,
    /// Re-analyse at most this often while on (seconds).
    pub every_s: f64,
    pub tags: Vec<Tag>,
    pub cfg: Config,
    /// The latest analysis (in check order, then as found).
    pub issues: Vec<Issue>,
    /// Instances in the analysed frame.
    pub instances: usize,
    /// Frame number of the latest analysis.
    pub analysed_frame: Option<u64>,
    /// What the latest analysis could not check (custom meshes treated as
    /// surfaces only, instances with no registered geometry).
    pub notes: Vec<String>,
    /// `Time::now_ms` of the latest analysis.
    last_ms: Option<f64>,
    /// Tint by instance identity (static things), and by index with the
    /// same mesh and material (moving things keep their slot).
    tint_key: HashMap<[i64; 12], [f32; 3]>,
    tint_index: Vec<Option<(MeshId, u32, [f32; 3])>>,
    shapes: Shapes,
    lib_rev: Option<u64>,
    pending: Vec<PendingReply>,
    /// The view's own overrides (restored when it stops using them).
    clear_set: Option<Option<[f32; 3]>>,
    camera_set: Option<Option<CameraPose>>,
    /// Apply `sky` / `focus` after the next analysis.
    sync: bool,
}

impl Default for GeometryView {
    fn default() -> Self {
        Self {
            on: false,
            checks: VIEW_CHECKS.to_vec(),
            sky: true,
            focus: None,
            every_s: 0.5,
            tags: Vec::new(),
            cfg: Config::default(),
            issues: Vec::new(),
            instances: 0,
            analysed_frame: None,
            notes: Vec::new(),
            last_ms: None,
            tint_key: HashMap::new(),
            tint_index: Vec::new(),
            shapes: Shapes::new(),
            lib_rev: None,
            pending: Vec::new(),
            clear_set: None,
            camera_set: None,
            sync: false,
        }
    }
}

impl GeometryView {
    /// Turn the view on (analysis at the next frame); used by
    /// `capture.frame` with `geometry: true`.
    pub fn switch_on(&mut self) {
        self.on = true;
        self.last_ms = None;
        self.sync = true;
    }

    /// Turn the view off and give back the overrides it set.
    pub fn switch_off(&mut self, q: Option<&mut CaptureQueue>) {
        self.on = false;
        self.focus = None;
        self.tint_key.clear();
        self.tint_index.clear();
        if let Some(q) = q {
            q.plain_materials = false;
            if let Some(prev) = self.clear_set.take() {
                q.clear_override = prev;
            }
            if let Some(prev) = self.camera_set.take() {
                q.camera_override = prev;
            }
        }
    }

    /// The tint of instance `i` of this frame (`None`: no issue).
    fn tint(&self, i: usize, m: &MeshInstance) -> Option<[f32; 3]> {
        if let Some(c) = self.tint_key.get(&scene::instance_key(m)) {
            return Some(*c);
        }
        match self.tint_index.get(i) {
            Some(Some((mesh, mat, c))) if *mesh == m.mesh && *mat == m.material => Some(*c),
            _ => None,
        }
    }

    /// Issue counts per check (untagged), plus `tagged`.
    pub fn counts(&self) -> Value {
        let mut m = Map::new();
        for c in &self.checks {
            let n = self.issues.iter().filter(|i| i.check == *c && i.tagged.is_none()).count();
            m.insert(c.name().into(), json!(n));
        }
        m.insert("tagged".into(), json!(self.issues.iter().filter(|i| i.tagged.is_some()).count()));
        Value::Object(m)
    }

    /// The `debug.geometry` result.
    pub fn result(&self) -> Value {
        let issues: Vec<Value> = self
            .issues
            .iter()
            .take(MAX_LISTED)
            .enumerate()
            .map(|(n, i)| {
                let mut v = i.to_json();
                v["index"] = json!(n);
                v
            })
            .collect();
        let colors: Map<String, Value> = self
            .checks
            .iter()
            .map(|c| (c.name().to_string(), json!(hex(c.color()))))
            .collect();
        json!({
            "on": self.on,
            "sky": self.on && self.sky,
            "checks": self.checks.iter().map(|c| c.name()).collect::<Vec<_>>(),
            "focus": self.focus,
            "every_s": self.every_s,
            "frame": self.analysed_frame,
            "instances": self.instances,
            "counts": self.counts(),
            "total": self.issues.len(),
            "issues": issues,
            "notes": self.notes,
            "colors": colors,
            "summary": self.summary(),
        })
    }

    /// The issues as readable text: a count line, then one line per listed
    /// issue (index, check, kind, objects, where, detail and fix).
    pub fn summary(&self) -> String {
        let counts: Vec<String> = self
            .checks
            .iter()
            .map(|c| format!("{} {}", c.name(), self.issues.iter().filter(|i| i.check == *c && i.tagged.is_none()).count()))
            .collect();
        let tagged = self.issues.iter().filter(|i| i.tagged.is_some()).count();
        let mut out = format!("{} issue(s) ({}, tagged {tagged})", self.issues.len(), counts.join(", "));
        for (n, i) in self.issues.iter().take(MAX_LISTED).enumerate() {
            out.push_str(&format!(
                "\n{n}. {}, {}{}: {} at ({:.1}, {:.1}, {:.1}): {} Fix: {}",
                i.check.name(),
                i.kind,
                if i.tagged.is_some() { " (tagged)" } else { "" },
                i.objects.join(" / "),
                i.at[0],
                i.at[1],
                i.at[2],
                i.detail,
                i.fix
            ));
        }
        if self.issues.len() > MAX_LISTED {
            out.push_str(&format!("\n... and {} more (see total).", self.issues.len() - MAX_LISTED));
        }
        out
    }

    /// Analyse one frame's instances.
    /// `meshes` is the frame's draw list followed by its decals; the decals
    /// start at index `n_meshes`. `viewer` is the game's camera this frame:
    /// z-fighting is judged by the depth maths as that camera sees it.
    fn analyse(&mut self, meshes: &[MeshInstance], n_meshes: usize, materials: &[String], viewer: &Viewer) {
        let list: Vec<(usize, MeshInstance)> = meshes.iter().copied().enumerate().collect();
        let label = |m: &MeshInstance| scene::default_label(m, materials);
        let s = Scene::build_with(&list, &mut self.shapes, &label, self.cfg.clone(), &|i| i >= n_meshes);
        let custom: Vec<MeshId> = checks::used_meshes(meshes)
            .into_iter()
            .map(|u| u.0)
            .filter(|id| usize::from(id.0) >= BUILTIN_MESH_COUNT)
            .collect();
        let mut notes = self.shapes.notes(&custom);
        for (id, n) in &s.skipped {
            notes.push(format!("Not checked: {n} instance(s) of mesh {} (no geometry registered for it).", id.0));
        }
        self.notes = notes;
        let mut issues = Vec::new();
        for c in VIEW_CHECKS {
            if !self.checks.contains(&c) {
                continue;
            }
            issues.extend(match c {
                Check::Overlap => checks::overlaps(&s),
                Check::Floating => checks::floating(&s),
                _ => zfight::zfight(&s, DepthSetup::engine(), std::slice::from_ref(viewer)),
            });
        }
        tags::apply(&mut issues, &self.tags);
        self.tint_key.clear();
        self.tint_index = vec![None; meshes.len()];
        // First issue wins: overlaps, then floating, then z-fighting.
        for i in issues.iter().filter(|i| i.tagged.is_none()) {
            for &k in &i.items {
                let Some(m) = meshes.get(k) else { continue };
                let c = i.check.color();
                self.tint_key.entry(scene::instance_key(m)).or_insert(c);
                if self.tint_index[k].is_none() {
                    self.tint_index[k] = Some((m.mesh, m.material, c));
                }
            }
        }
        self.instances = meshes.len();
        self.issues = issues;
    }
}

fn hex(c: [f32; 3]) -> String {
    let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// A camera pose framing `b` (from above and to the side).
pub fn framing_pose(b: &Aabb, aspect: f64) -> CameraPose {
    let c = Camera::framing(b, [1.0, 0.8, 1.0], 50.0, aspect);
    let f = |p: V3| p.map(|x| x as f32);
    CameraPose {
        eye: f(c.eye),
        target: f(c.target),
        up: f(c.up),
        fov_y_deg: c.fov_y_deg as f32,
    }
}

fn issue_box(i: &Issue) -> Aabb {
    i.aabb.unwrap_or(Aabb { min: i.at, max: i.at }).grow(0.25)
}

/// Frame system (`live.geometry`, last in `Stage::Frame`).
pub fn frame(world: &mut World) {
    let Some(mut gv) = world.try_res_mut::<GeometryView>() else {
        return;
    };
    if !gv.on && gv.pending.is_empty() {
        return;
    }
    let (now_ms, frame_no) = world.try_res::<Time>().map_or((0.0, 0), |t| (t.now_ms, t.frame));
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else {
        // No renderer: answer and stop.
        let err = LiveError::new("unavailable", "this build has no 3D renderer to analyse");
        for p in core::mem::take(&mut gv.pending) {
            crate::commands::send_reply(world, p.token, &p.id, Err(err.clone()));
        }
        return;
    };
    let due = !gv.pending.is_empty() || gv.last_ms.is_none_or(|t| now_ms - t >= gv.every_s * 1000.0);
    if due {
        if let Some(lib) = world.try_res::<MeshLibrary>()
            && gv.lib_rev != Some(lib.revision())
        {
            gv.shapes.add_library(&lib);
            gv.lib_rev = Some(lib.revision());
        }
        let materials: Vec<String> = world
            .try_res::<LookData>()
            .map(|l| l.materials.iter().map(|m| m.name.clone()).collect())
            .unwrap_or_default();
        let n_meshes = rw.meshes.len();
        let mut all = rw.meshes.clone();
        all.extend_from_slice(&rw.decals);
        let aspect = f64::from(rw.viewport_css[0].max(1.0)) / f64::from(rw.viewport_css[1].max(1.0));
        let viewer = Viewer::new(Camera::from_pose(&rw.camera, aspect), f64::from(rw.viewport_css[1]), "live", Some(now_ms / 1000.0));
        gv.analyse(&all, n_meshes, &materials, &viewer);
        gv.last_ms = Some(now_ms);
        gv.analysed_frame = Some(frame_no);
    }
    if gv.sync {
        gv.sync = false;
        let aspect = f64::from(rw.viewport_css[0].max(1.0)) / f64::from(rw.viewport_css[1].max(1.0));
        if let Some(mut q) = world.try_res_mut::<CaptureQueue>() {
            sync_overrides(&mut gv, &mut q, aspect);
        }
    }
    if gv.on {
        for i in 0..rw.meshes.len() {
            let m = rw.meshes[i];
            let c = gv.tint(i, &m).unwrap_or(GREY);
            rw.meshes[i].color = [c[0], c[1], c[2], m.color[3]];
        }
        let n = rw.meshes.len();
        for k in 0..rw.decals.len() {
            let d = rw.decals[k];
            let c = gv.tint(n + k, &d).unwrap_or(GREY);
            rw.decals[k].color = [c[0], c[1], c[2], d.color[3]];
        }
    }
    let pending = core::mem::take(&mut gv.pending);
    if !pending.is_empty() {
        let res = gv.result();
        drop(gv);
        drop(rw);
        for p in pending {
            crate::commands::send_reply(world, p.token, &p.id, Ok(res.clone()));
        }
    }
}

/// Apply (or give back) the view's sky and focus overrides.
fn sync_overrides(gv: &mut GeometryView, q: &mut CaptureQueue, aspect: f64) {
    // Plain materials while on: the renderer multiplies instance colours by
    // the look's material colours, which would hide the check colours.
    q.plain_materials = gv.on;
    let want_sky = gv.on && gv.sky;
    match (want_sky, gv.clear_set) {
        (true, None) => {
            gv.clear_set = Some(q.clear_override);
            q.clear_override = Some(SKY);
        }
        (false, Some(prev)) => {
            q.clear_override = prev;
            gv.clear_set = None;
        }
        _ => {}
    }
    if gv.focus.is_some_and(|n| n >= gv.issues.len()) {
        gv.focus = None;
    }
    let focus = if gv.on { gv.focus.and_then(|n| gv.issues.get(n)) } else { None };
    match focus {
        Some(i) => {
            if gv.camera_set.is_none() {
                gv.camera_set = Some(q.camera_override);
            }
            q.camera_override = Some(framing_pose(&issue_box(i), aspect));
        }
        None => {
            if let Some(prev) = gv.camera_set.take() {
                q.camera_override = prev;
            }
        }
    }
}

/// `debug.geometry` (6.27). `Ok(None)`: the reply waits for the next
/// analysis (sent by [`frame`]); `Ok(Some(result))` answers at once.
pub(crate) fn command(world: &World, token: u64, id: &Value, params: &Value) -> Result<Option<Value>, LiveError> {
    if !world.has_resource::<GeometryView>() {
        return Err(LiveError::new("internal", "no geometry view in this build"));
    }
    if !world.has_resource::<RenderWorld>() {
        return Err(LiveError::new("unavailable", "this build has no 3D renderer to analyse"));
    }
    let bad = |m: String| LiveError::new("invalid_value", m);
    let on = match params.get("on") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad("on must be true or false".into())),
    };
    let checks = match params.get("checks") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for v in a {
                let name = v.as_str().unwrap_or("");
                match Check::parse(name) {
                    Some(c) if VIEW_CHECKS.contains(&c) => out.push(c),
                    Some(_) => {
                        return Err(bad(format!(
                            "the live view runs overlap, floating and zfight; {name} needs the full tool (cargo xtask geocheck <app>)"
                        )));
                    }
                    None => return Err(bad(format!("unknown check {name:?} (overlap, floating, zfight)"))),
                }
            }
            if out.is_empty() {
                return Err(bad("checks must name at least one of overlap, floating, zfight".into()));
            }
            Some(out)
        }
        Some(_) => return Err(bad("checks must be a list of check names".into())),
    };
    let sky = match params.get("sky") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad("sky must be true or false".into())),
    };
    let focus = match params.get("focus") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => match v.as_u64() {
            Some(n) => Some(Some(n as usize)),
            None => return Err(bad("focus must be an issue index or null".into())),
        },
    };
    let every_s = match params.get("every_s") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_f64() {
            Some(x) if (0.05..=30.0).contains(&x) => Some(x),
            _ => return Err(bad("every_s must be a number 0.05-30".into())),
        },
    };
    let tag_list = match params.get("tags") {
        None | Some(Value::Null) => None,
        Some(v) => Some(tags::parse(v).map_err(bad)?),
    };
    let cfg = match params.get("thresholds") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let mut c = Config::default();
            c.apply_json(v).map_err(bad)?;
            Some(c)
        }
    };
    let mut gv = world.res_mut::<GeometryView>();
    if on == Some(false) {
        let mut q = world.try_res_mut::<CaptureQueue>();
        gv.switch_off(q.as_deref_mut());
        gv.pending.clear();
        return Ok(Some(gv.result()));
    }
    if let Some(c) = checks {
        gv.checks = c;
    }
    if let Some(s) = sky {
        gv.sky = s;
    }
    if let Some(f) = focus {
        gv.focus = f;
    }
    if let Some(e) = every_s {
        gv.every_s = e;
    }
    if let Some(t) = tag_list {
        gv.tags = t;
    }
    if let Some(c) = cfg {
        gv.cfg = c;
    }
    if on == Some(true) {
        gv.on = true;
    }
    gv.sync = true;
    gv.pending.push(PendingReply { token, id: id.clone() });
    Ok(None)
}
