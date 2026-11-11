//! The bridge from the studio's record to the viewer's machinery (WP1 steps
//! 2 to 4), only while the studio is on screen ([`super::Studio::active`]):
//!
//! - **The material.** The record's material opens its catalogue row
//!   (`kit.material_view.cat.open`), so the recipe stage builds its block.
//!   Once the catalogue has opened it (every switch on the row's own
//!   values: the *baseline*), the record's remembered edits are laid over
//!   it. A switch moved while the record is shown (the panel, evani-live)
//!   is stored in the record's `Settings.edits` by param name; moved back
//!   onto the row's own value, the edit is dropped. Another record with the
//!   same material reuses the baseline: only the edits are swapped.
//! - **The camera.** Leaving a record stores the orbit camera in its
//!   settings; returning puts it back. A record never seen keeps the camera
//!   as it is (the loader frames a new asset).
//! - **The light and quality** follow `View`: a preset id opens that light
//!   preset, a tier id makes that tier active (`auto` leaves it to the
//!   engine). The light key (L) writes the new preset back into `View`.
//!
//! None of it ever changes the record itself, and a light or quality change
//! never reopens the row or moves a cache key.

use std::collections::BTreeMap;

use evani_core::ecs::World;
use evani_core::params::{ParamRegistry, ParamType, ParamValue};
use evani_core::tiers::Tiers;
use evani_material_recipes::block::CONDITION_NAMES;
use evani_studio::state::Camera;

use super::{Catalogs, Selection, Studio, Tweak};
use crate::catalogue::{self, CatalogueView, LibraryData, OPEN};
use crate::viewer::{Command, Viewer};

/// The bridge's own memory (a world resource).
#[derive(Debug, Clone, Default)]
pub struct Bridge {
    /// The record whose edits are in the params now.
    record: Option<Selection>,
    /// The material whose baseline is held.
    row: Option<String>,
    /// Param name -> the row's own value (every switch of its recipe).
    baseline: BTreeMap<String, ParamValue>,
    /// Param name -> the value as the bridge last saw or wrote it.
    synced: BTreeMap<String, ParamValue>,
    /// The catalogue row (index) asked for and not opened yet.
    waiting: Option<usize>,
    /// The bridge set `cat.open` (put back to none when the studio leaves).
    set_open: bool,
    light: Option<String>,
    viewer_light: Option<usize>,
    quality: Option<String>,
}

impl Bridge {
    /// The material whose catalogue row the bridge has open (for the scene
    /// and the tests).
    pub fn open_material(&self) -> Option<&str> {
        self.row.as_deref().filter(|_| self.waiting.is_none())
    }

    /// The open row's switches (param names) the bridge keeps per record.
    pub fn switches(&self) -> impl Iterator<Item = &str> {
        self.baseline.keys().map(String::as_str)
    }
}

/// The light preset ids, in order (empty without the viewer's data).
pub fn light_ids(world: &World) -> Vec<String> {
    world.try_res::<crate::ViewerData>().map(|d| d.0.lights.presets.iter().map(|p| p.id.clone()).collect()).unwrap_or_default()
}

fn tweak(v: ParamValue) -> Option<Tweak> {
    match v {
        ParamValue::F32(x) => Some(Tweak::Num(x)),
        ParamValue::Bool(b) => Some(Tweak::Flag(b)),
        ParamValue::Color(c) | ParamValue::Vec3(c) => Some(Tweak::Colour(c)),
        ParamValue::I32(_) | ParamValue::Enum(_) => None,
    }
}

fn value(t: Tweak, ty: ParamType) -> Option<ParamValue> {
    match (t, ty) {
        (Tweak::Num(x), ParamType::F32) => Some(ParamValue::F32(x)),
        (Tweak::Flag(b), ParamType::Bool) => Some(ParamValue::Bool(b)),
        (Tweak::Colour(c), ParamType::Color) => Some(ParamValue::Color(c)),
        (Tweak::Colour(c), ParamType::Vec3) => Some(ParamValue::Vec3(c)),
        _ => None,
    }
}

fn read(reg: &ParamRegistry, names: impl Iterator<Item = String>) -> BTreeMap<String, ParamValue> {
    names.filter_map(|n| reg.id(&n).and_then(|id| reg.value(id)).map(|v| (n, v))).collect()
}

/// Every switch a material's recipe has: its numbers, its extras and the
/// eight condition switches.
fn switch_names(world: &World, material: &str) -> Vec<String> {
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.0.clone()) else { return Vec::new() };
    let Some((_, row)) = lib.find(material) else { return Vec::new() };
    let row = catalogue::working_row(&lib.cat, row);
    let Ok(recipe) = lib.cat.recipe_for(&row) else { return Vec::new() };
    let mut out: Vec<String> = recipe.numbers.iter().map(|n| catalogue::number_param(&n.name)).collect();
    out.extend(recipe.extras.iter().map(|e| catalogue::extra_param(&recipe.id, &e.name)));
    for layer in CONDITION_NAMES {
        out.push(catalogue::cond_param(layer, "on"));
        out.push(catalogue::cond_param(layer, "amount"));
    }
    out
}

/// Step 1 (before the commands): a switch moved since the bridge last
/// looked is an edit of the record shown.
pub fn capture(world: &mut World) {
    let Some(mut b) = world.try_res_mut::<Bridge>().map(|mut b| std::mem::take(&mut *b)) else { return };
    capture_into(world, &mut b);
    world.insert_resource(b);
}

fn capture_into(world: &mut World, b: &mut Bridge) {
    let (active, sel) = {
        let s = world.res::<Studio>();
        (s.active, s.state.selection.clone())
    };
    if !active || b.waiting.is_some() || b.baseline.is_empty() || b.record.as_ref() != Some(&sel) || b.row.as_deref() != Some(sel.material.as_str()) {
        return;
    }
    let now = read(&world.res::<ParamRegistry>(), b.baseline.keys().cloned());
    let moved: Vec<(&String, &ParamValue)> = now.iter().filter(|(k, v)| b.synced.get(*k) != Some(*v)).collect();
    if moved.is_empty() {
        return;
    }
    {
        let mut s = world.res_mut::<Studio>();
        let edits = &mut s.state.settings_mut().edits;
        for (name, v) in &moved {
            if b.baseline.get(*name) == Some(*v) {
                edits.remove(*name);
            } else if let Some(t) = tweak(**v) {
                edits.insert((*name).clone(), t);
            }
        }
    }
    b.synced = now;
}

fn camera_of(world: &World) -> Option<Camera> {
    world.try_res::<Viewer>().map(|v| Camera { yaw: v.orbit.az, pitch: v.orbit.el, distance: v.orbit.dist, target: v.orbit.target })
}

/// Store the orbit camera in `record`'s settings.
fn store_camera(world: &mut World, record: &Selection) {
    let Some(cam) = camera_of(world) else { return };
    let mut s = world.res_mut::<Studio>();
    let settings = s.state.memory.entry(record.key()).or_default();
    if settings.camera != Some(cam) {
        settings.camera = Some(cam);
        s.state.revision += 1;
    }
}

/// Put `record`'s stored camera back, if it has one.
fn restore_camera(world: &mut World, record: &Selection) {
    let cam = world.res::<Studio>().state.memory.get(&record.key()).and_then(|s| s.camera);
    let (Some(cam), Some(mut v)) = (cam, world.try_res_mut::<Viewer>()) else { return };
    v.orbit.az = cam.yaw;
    v.orbit.el = cam.pitch;
    v.orbit.dist = cam.distance;
    v.orbit.target = cam.target;
    v.orbit = v.orbit.clamped();
    v.revision += 1;
}

/// Set `cat.open` to option `k` (0 = none). True when it moved.
fn set_open(reg: &mut ParamRegistry, k: u32) -> bool {
    let Some(id) = reg.id(OPEN) else { return false };
    reg.set(id, ParamValue::Enum(k)).is_ok_and(|o| o.previous != o.value)
}

/// Open `material`'s catalogue row (or wait for the catalogue to open it).
fn engage(world: &mut World, b: &mut Bridge, material: &str) {
    b.baseline.clear();
    b.synced.clear();
    b.waiting = None;
    b.row = Some(material.to_string());
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.0.clone()) else { return };
    let option = {
        let reg = world.res::<ParamRegistry>();
        reg.id(OPEN).and_then(|id| reg.def(id)).and_then(|d| d.options.iter().position(|o| o == material))
    };
    let found = lib.find(material).map(|(i, _)| i).filter(|&i| lib.openable(i));
    let (Some(k), Some(i)) = (option, found) else {
        // Not a catalogue row this build can open: nothing to lay edits on.
        if b.set_open {
            set_open(&mut world.res_mut::<ParamRegistry>(), 0);
        }
        return;
    };
    let k = u32::try_from(k).unwrap_or(0);
    b.set_open = true;
    if set_open(&mut world.res_mut::<ParamRegistry>(), k) {
        b.waiting = Some(i);
        return;
    }
    if world.try_res::<CatalogueView>().is_some_and(|v| v.open == Some(i)) {
        // Already open (the library opened it, or the last record had it):
        // its switches back on the row's own values, which the catalogue
        // won't redo for an unchanged `cat.open`.
        let row = catalogue::working_row(&lib.cat, &lib.cat.rows[i]);
        if let (Ok(recipe), Some(cond)) = (lib.cat.recipe_for(&row), catalogue::cond_params(world)) {
            catalogue::open_row(&mut world.res_mut::<ParamRegistry>(), &cond, recipe, &row);
        }
        b.baseline = read(&world.res::<ParamRegistry>(), switch_names(world, material).into_iter());
    } else {
        b.waiting = Some(i);
    }
}

/// The record's edits over the baseline, written where they differ.
fn lay_edits(world: &mut World, b: &mut Bridge, record: &Selection) {
    let edits = world.res::<Studio>().state.memory.get(&record.key()).map(|s| s.edits.clone()).unwrap_or_default();
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        for (name, base) in &b.baseline {
            let Some(id) = reg.id(name) else { continue };
            let ty = reg.def(id).map(|d| d.ty);
            let want = edits.get(name).and_then(|t| ty.and_then(|ty| value(*t, ty))).unwrap_or(*base);
            if reg.value(id) != Some(want) {
                let _ = reg.set(id, want);
            }
        }
    }
    b.synced = read(&world.res::<ParamRegistry>(), b.baseline.keys().cloned());
}

/// The light preset and the quality tier from `View` (and the light key
/// back into it).
fn light_and_quality(world: &mut World, b: &mut Bridge) {
    let ids = light_ids(world);
    let (want, quality) = {
        let s = world.res::<Studio>();
        (s.state.view.light.clone(), s.state.view.quality.clone())
    };
    let shown = world.try_res::<Viewer>().map(|v| v.light);
    if !ids.is_empty() {
        if b.light.as_deref() != Some(want.as_str()) {
            let i = ids.iter().position(|x| *x == want);
            match i {
                Some(i) => {
                    if let Some(mut v) = world.try_res_mut::<Viewer>() {
                        v.push(Command::Light(i));
                    }
                    b.light = Some(want);
                    b.viewer_light = Some(i);
                }
                None => {
                    // A stored state naming a preset that is gone: the first.
                    if let Some(first) = ids.first() {
                        world.res_mut::<Studio>().state.set_light(first);
                    }
                }
            }
        } else if let Some(now) = shown
            && b.viewer_light.is_some_and(|seen| seen != now)
            && let Some(id) = ids.get(now)
        {
            world.res_mut::<Studio>().state.set_light(id);
            b.light = Some(id.clone());
            b.viewer_light = Some(now);
        }
    }
    if b.quality.as_deref() != Some(quality.as_str()) {
        let r = if quality == "auto" { Ok(()) } else { world.try_res_mut::<Tiers>().map_or(Ok(()), |mut t| t.try_set_active(&quality).map_err(|e| format!("quality `{quality}`: {e}"))) };
        match r {
            Ok(()) => b.quality = Some(quality),
            Err(e) => {
                let mut s = world.res_mut::<Studio>();
                s.error = Some(e);
                let back = b.quality.clone().unwrap_or_else(|| "auto".into());
                s.state.set_quality(&back);
                b.quality = Some(back);
            }
        }
    }
}

/// Steps 2 to 4 (after the commands): follow the record.
pub fn sync(world: &mut World, _c: &Catalogs) {
    let Some(mut b) = world.try_res_mut::<Bridge>().map(|mut b| std::mem::take(&mut *b)) else { return };
    let (active, restored, sel) = {
        let mut s = world.res_mut::<Studio>();
        (s.active, std::mem::take(&mut s.restored), s.state.selection.clone())
    };
    if restored {
        // A new state: start over (the old record's camera isn't the new
        // state's to keep).
        b.record = None;
        b.light = None;
        b.viewer_light = None;
        b.quality = None;
    }
    if !active {
        if let Some(old) = b.record.take() {
            store_camera(world, &old);
            if b.set_open {
                set_open(&mut world.res_mut::<ParamRegistry>(), 0);
            }
            b = Bridge::default();
        }
        world.insert_resource(b);
        return;
    }
    if b.record.as_ref() != Some(&sel) {
        if let Some(old) = b.record.take() {
            store_camera(world, &old);
        }
        restore_camera(world, &sel);
        if b.row.as_deref() != Some(sel.material.as_str()) || (b.baseline.is_empty() && b.waiting.is_none()) {
            engage(world, &mut b, &sel.material);
        }
        b.record = Some(sel.clone());
    }
    if let Some(i) = b.waiting
        && world.try_res::<CatalogueView>().is_some_and(|v| v.open == Some(i))
    {
        b.waiting = None;
        b.baseline = read(&world.res::<ParamRegistry>(), switch_names(world, &sel.material).into_iter());
    }
    if b.waiting.is_none() && !b.baseline.is_empty() {
        lay_edits(world, &mut b, &sel);
    }
    light_and_quality(world, &mut b);
    world.insert_resource(b);
}
