//! The build queue (startfix): pipelines are built when something needs
//! them, off the browser's GPU main thread, one heavy build at a time.
//!
//! * [`Pipelines::launch`] only queues a [`Job`]. Engine shaders, the UI,
//!   the depth resolve and the effects start at once; the model, skinned,
//!   see-through and glass pipelines and the 4x forward pipeline start the
//!   first frame a draw asks for them (their getters call [`Queue::want`]).
//!   With the `prewarm` feature every job starts at once, as before.
//! * A job starts in two steps. Warm-up: the pipeline is described through
//!   wgpu as usual, but the browser's `createRenderPipeline` is swapped for
//!   `createRenderPipelineAsync` for that one call, so the driver compiles
//!   on a worker thread while frames keep drawing. Build: once that resolves,
//!   the same descriptor goes through wgpu's normal (blocking) call, which the
//!   browser answers from the pipeline it just compiled (measured: 1-30 ms
//!   instead of 24 s for the 242 KB model shader on an RTX 4060 Ti).
//! * At most one heavy job (source over [`HEAVY_BYTES`]) warms up at a time;
//!   up to [`MAX_LIGHT`] light ones run beside it.
//! * A heavy model or skinned pipeline gets a stand-in first: the same
//!   module with the plain `model_fs` fragment stage ([`Key::ModelLite`]),
//!   which builds in about 2 s and draws until the full one is in.
//! * A job built from the current slot bodies is spliced again just before
//!   it starts and once its warm-up is done: a body that changed meanwhile
//!   restarts it with the new source, so a pipeline is never installed with
//!   stale bodies and then rebuilt (the double build).
//! * Every valid pipeline is kept in a small variant cache keyed by its
//!   source: building the same variant again (a slider moved back) is free.

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::slots::ShaderSlots;
use js_sys::{Function, Object, Promise, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{Formats, Key, Layouts, Outcome, Pipelines, Purpose, create_pipeline};
use crate::graph::RenderGraph;
use crate::post::PostEffects;
use crate::splice::{Bodies, Spliced};

/// A source this long is a heavy compile (the forward and model shaders).
pub(super) const HEAVY_BYTES: usize = 64 * 1024;
/// Light warm-ups running at once beside the heavy one.
const MAX_LIGHT: u32 = 4;
/// Valid pipelines kept for reuse.
const CACHE_MAX: usize = if cfg!(feature = "material-split") { 160 } else { 24 };

pub(super) struct Job {
    pub key: Key,
    pub purpose: Purpose,
    pub spliced: Spliced,
    /// Built from the current slot bodies (spliced again before it starts).
    pub current: bool,
}

/// A finished warm-up, waiting for its build.
struct Warmed {
    epoch: u32,
    job: Job,
    module: wgpu::ShaderModule,
    /// The browser built it; false: rejected (the build reports why).
    ok: bool,
    heavy: bool,
    /// The browser's pipeline, alive until the build has reused it.
    keep: Option<JsValue>,
}

#[derive(Default)]
pub(super) struct Queue {
    jobs: RefCell<Vec<Job>>,
    /// Lazy keys a draw asked for, with the frame it last asked.
    wanted: RefCell<Vec<(Key, u64)>>,
    warm: Rc<RefCell<Vec<Warmed>>>,
    heavy: Cell<u32>,
    light: Cell<u32>,
    tick: Cell<u64>,
    /// A stand-in drew this frame.
    stand_in: Cell<bool>,
    cache: RefCell<Vec<(Key, u64, wgpu::RenderPipeline)>>,
    /// The status last written to the page.
    shown: Cell<Option<(u32, bool)>>, #[cfg(feature = "forward")] twins: Twins,
}

impl Key {
    /// The full pipeline a stand-in stands in for (itself otherwise).
    pub(super) fn full(self) -> Key {
        match self {
            #[cfg(feature = "models")]
            Key::ModelLite(ms) => Key::Model(ms),
            #[cfg(feature = "skinning")]
            Key::SkinLite(ms) => Key::Skin(ms),
            k => k,
        }
    }

    fn is_lite(self) -> bool {
        self.full() != self
    }

    /// Built only once a draw asks for it.
    fn lazy(self) -> bool {
        if cfg!(feature = "prewarm") {
            return false;
        }
        match self {
            #[cfg(feature = "forward")]
            Key::EngineMs(_) => true,
            #[cfg(feature = "models")]
            Key::Model(_) | Key::ModelLite(_) => true,
            #[cfg(feature = "skinning")]
            Key::Skin(_) | Key::SkinLite(_) => true,
            #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
            Key::Blend(_) => true,
            #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))]
            Key::Glass => true, #[cfg(feature = "material-split")] Key::ModelSplit(..) => true,
            _ => false,
        }
    }
}

fn hash(source: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

impl Queue {
    /// Queue a job; a newer start-up job for the same key replaces the old.
    pub(super) fn push(&self, job: Job) {
        let mut jobs = self.jobs.borrow_mut();
        if matches!(job.purpose, Purpose::Initial { .. }) {
            jobs.retain(|j| !(j.key == job.key && matches!(j.purpose, Purpose::Initial { .. })));
        }
        jobs.push(job);
    }

    /// A draw needs `key` (a lazy key not built yet).
    #[cfg_attr(not(any(feature = "forward", feature = "models")), allow(dead_code))] pub(super) fn want(&self, key: Key) {
        let now = self.tick.get();
        let mut w = self.wanted.borrow_mut();
        match w.iter_mut().find(|(k, _)| *k == key) {
            Some(e) => e.1 = now,
            None => w.push((key, now)),
        }
    }

    #[cfg(feature = "models")] pub(super) fn stand_in(&self) {
        self.stand_in.set(true);
    }

    /// When the job may start: start-up jobs of lazy keys wait for a draw.
    fn rank(&self, job: &Job) -> Option<u64> { #[cfg(feature = "forward")] if self.twins.is(job.key) { return Some(0); } // a twin waits behind every build a draw asked for
        if !job.key.lazy() || !matches!(job.purpose, Purpose::Initial { .. }) {
            return Some(u64::MAX);
        }
        self.wanted.borrow().iter().find(|(k, _)| *k == job.key).map(|e| e.1)
    }

    /// Builds something on screen is waiting for.
    fn waiting(&self) -> u32 {
        let queued = self.jobs.borrow().iter().filter(|j| self.rank(j).is_some()).count() as u32; #[cfg(feature = "forward")] let queued = queued.saturating_sub(self.twins.queued(&self.jobs.borrow()) + self.twins.flight.get()); // twins are not "preparing" #[cfg(all(feature = "kit-material", feature = "shadows"))] let queued = queued + super::kit_material::warming();
        queued + self.heavy.get() + self.light.get()
    }

    /// Any build queued for a draw, warming up or waiting for its build.
    pub(super) fn busy(&self) -> bool {
        self.waiting() > 0 || !self.warm.borrow().is_empty()
    }

    pub(super) fn remember(&self, key: Key, source: &str, p: &wgpu::RenderPipeline) {
        let h = hash(source);
        let mut c = self.cache.borrow_mut();
        if c.iter().any(|(k, x, _)| *k == key && *x == h) {
            return;
        }
        if c.len() >= CACHE_MAX {
            c.remove(0);
        }
        c.push((key, h, p.clone()));
    }

    fn cached(&self, key: Key, source: &str) -> Option<wgpu::RenderPipeline> {
        let h = hash(source);
        self.cache.borrow().iter().find(|(k, x, _)| *k == key && *x == h).map(|e| e.2.clone())
    }
}

/// What the swapped `createRenderPipeline` caught.
pub(super) enum Caught {
    /// The browser's `createRenderPipelineAsync` promise.
    Warming(Promise),
    /// The call was not swapped: a normal, finished build.
    Built(wgpu::RenderPipeline),
}

/// Run `make` (one wgpu `create_render_pipeline`) with the device's
/// `createRenderPipeline` swapped for `createRenderPipelineAsync` on the
/// same descriptor. The wgpu pipeline `make` returns then wraps a stand-in
/// object and is dropped unused.
pub(super) fn warm_call(dev: &wgpu::Device, make: impl FnOnce() -> wgpu::RenderPipeline) -> Caught {
    let Some(raw) = dev.as_webgpu() else {
        return Caught::Built(make());
    };
    let raw: JsValue = raw.clone().into();
    let name = JsValue::from_str("createRenderPipeline");
    let seen: Rc<RefCell<Option<Option<JsValue>>>> = Rc::new(RefCell::new(None));
    let (s, r) = (seen.clone(), raw.clone());
    let swap = Closure::<dyn FnMut(JsValue) -> JsValue>::new(move |desc: JsValue| {
        let p = Reflect::get(&r, &JsValue::from_str("createRenderPipelineAsync"))
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .and_then(|f| f.call1(&r, &desc).ok());
        *s.borrow_mut() = Some(p);
        Object::new().into()
    });
    if Reflect::set(&raw, &name, swap.as_ref()).is_err() {
        return Caught::Built(make());
    }
    let pipeline = make();
    let _ = Reflect::delete_property(raw.unchecked_ref::<Object>(), &name);
    drop(swap);
    let caught = seen.borrow_mut().take();
    match caught {
        None => Caught::Built(pipeline),
        Some(p) => {
            drop(pipeline);
            Caught::Warming(p.and_then(|p| p.dyn_into::<Promise>().ok()).unwrap_or_else(|| Promise::reject(&JsValue::NULL)))
        }
    }
}

/// Write the build status to the page (`<html data-evani-preparing="n"
/// data-evani-stand-in>`), so a page can say "preparing materials".
fn show_status(waiting: u32, stand_in: bool) {
    let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) else {
        return;
    };
    let _ = el.set_attribute("data-evani-preparing", &waiting.to_string());
    let _ = if stand_in { el.set_attribute("data-evani-stand-in", "") } else { el.remove_attribute("data-evani-stand-in") };
}

impl Pipelines {
    /// Once a frame, after [`Pipelines::poll`] and the slot requests: build
    /// the warmed-up jobs, start the jobs that may start, report the status.
    pub fn pump(&mut self, dev: &wgpu::Device, layouts: &Layouts, formats: Formats, world: &World) {
        let tick = self.q.tick.get() + 1;
        self.q.tick.set(tick);
        let fx = world.res::<PostEffects>();
        let graph = world.res::<RenderGraph>();
        let slots = world.res::<ShaderSlots>();
        self.queue_stand_ins(&fx, &slots); #[cfg(feature = "material-split")] self.queue_splits(&fx, &slots);

        let warmed: Vec<Warmed> = core::mem::take(&mut *self.q.warm.borrow_mut());
        for w in warmed {
            let n = if w.heavy { &self.q.heavy } else { &self.q.light };
            n.set(n.get().saturating_sub(1)); #[cfg(feature = "forward")] self.q.twins.landed(w.job.key);
            if w.epoch != self.epoch {
                continue;
            }
            let mut job = w.job;
            if job.current
                && let Some(Ok(sp)) = Self::splice_key(job.key, &fx, &slots, Bodies::Current)
                && sp.source != job.spliced.source
            {
                job.spliced = sp;
                self.q.push(job);
                continue;
            }
            let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = if w.ok { w.module } else { module_of(dev, &job.spliced) };
            let pipeline = create_pipeline(dev, layouts, formats, &graph, &fx, job.key, &module);
            let err = scope.pop();
            drop(w.keep);
            self.finish(job.key, job.purpose, job.spliced, &module, pipeline, err);
        }

        let jobs: Vec<Job> = core::mem::take(&mut *self.q.jobs.borrow_mut());
        let mut ranked: Vec<(u64, Job)> = Vec::new();
        let mut held = Vec::new();
        for j in jobs {
            match self.q.rank(&j) {
                Some(r) => ranked.push((r, j)),
                None => held.push(j),
            }
        }
        // Most recently asked for first (start-up and slot jobs before any).
        ranked.sort_by_key(|(r, _)| core::cmp::Reverse(*r));
        for (_, mut job) in ranked {
            #[cfg(not(feature = "material-split"))] if job.current
                && let Some(Ok(sp)) = Self::splice_key(job.key, &fx, &slots, Bodies::Current)
            {
                job.spliced = sp;
            }
            let heavy = job.spliced.source.len() > HEAVY_BYTES && !job.key.is_lite(); #[cfg(feature = "material-split")] let heavy = heavy && !matches!(job.key, Key::ModelSplit(..));
            let room = if heavy { self.q.heavy.get() == 0 } else { self.q.light.get() < MAX_LIGHT };
            if !room {
                held.push(job);
                continue;
            } #[cfg(feature = "material-split")] if job.current && let Some(Ok(sp)) = Self::splice_key(job.key, &fx, &slots, Bodies::Current) { job.spliced = sp; }
            if let Some(p) = self.q.cached(job.key, &job.spliced.source) { #[cfg(feature = "forward")] self.q.twins.done(job.key);
                self.in_flight.set(self.in_flight.get() + 1);
                self.inbox.borrow_mut().push(Outcome { epoch: self.epoch, key: job.key, purpose: job.purpose, pipeline: p, scope_error: None, messages: Vec::new(), spliced: job.spliced });
                continue;
            }
            self.warm_up(dev, layouts, formats, &graph, &fx, job, heavy);
        }
        // Keep the queue order for the next frame (jobs queued meanwhile last).
        let mut q = self.q.jobs.borrow_mut();
        held.append(&mut q);
        *q = held;
        drop(q);

        #[cfg(feature = "forward")] self.queue_twins(&fx, &slots, world); let status = (self.q.waiting(), self.q.stand_in.replace(false));
        if self.q.shown.get() != Some(status) {
            self.q.shown.set(Some(status));
            show_status(status.0, status.1);
        }
    }

    /// A stand-in asked for by a draw gets a job when its full pipeline is
    /// heavy and not built yet (a light full pipeline needs none).
    fn queue_stand_ins(&self, fx: &PostEffects, slots: &ShaderSlots) {
        let lites: Vec<Key> = self.q.wanted.borrow().iter().map(|e| e.0).filter(|k| k.is_lite()).collect();
        for lite in lites {
            let full = lite.full();
            let queued = |k: Key| self.q.jobs.borrow().iter().any(|j| j.key == k);
            if self.installed(lite) || queued(lite) || self.installed(full) {
                continue;
            }
            let Some(Ok(sp)) = Self::splice_key(lite, fx, slots, Bodies::Current) else {
                continue;
            };
            if sp.source.len() > HEAVY_BYTES || cfg!(feature = "material-split") {
                self.q.push(Job { key: lite, purpose: Purpose::Initial { fallback: false }, spliced: sp, current: true });
            } else {
                // Not needed: forget the ask so it is not checked every frame.
                self.q.wanted.borrow_mut().retain(|e| e.0 != lite);
            }
        }
    }

    /// Start the browser's off-thread compile of `job`.
    #[allow(clippy::too_many_arguments)]
    fn warm_up(&self, dev: &wgpu::Device, layouts: &Layouts, formats: Formats, graph: &RenderGraph, fx: &PostEffects, job: Job, heavy: bool) {
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = module_of(dev, &job.spliced);
        let caught = warm_call(dev, || create_pipeline(dev, layouts, formats, graph, fx, job.key, &module));
        let err = scope.pop();
        match caught {
            Caught::Built(pipeline) => self.finish(job.key, job.purpose, job.spliced, &module, pipeline, err),
            Caught::Warming(promise) => { #[cfg(feature = "forward")] self.q.twins.started(job.key); // only a warm-up lands later
                let n = if heavy { &self.q.heavy } else { &self.q.light };
                n.set(n.get() + 1);
                let inbox = self.q.warm.clone();
                let epoch = self.epoch;
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = err.await;
                    let r = JsFuture::from(promise).await;
                    inbox.borrow_mut().push(Warmed { epoch, job, module, ok: r.is_ok(), heavy, keep: r.ok() });
                });
            }
        }
    }

    /// Whether the 4x model pipelines a 4x forward pass would draw with are
    /// ready (full or stand-in), for the models drawn so far. Asks for them.
    #[cfg_attr(not(feature = "forward"), allow(dead_code))] pub fn ms_models_ready(&self) -> bool {
        #[cfg(feature = "models")]
        {
            let drawn = |k: Key| self.q.wanted.borrow().iter().any(|e| e.0 == k) || self.installed(k);
            if drawn(Key::Model(false)) && self.model(true).is_none() {
                return false;
            } #[cfg(feature = "material-split")] if !self.split_ms_ready() { return false; }
            #[cfg(feature = "skinning")]
            if drawn(Key::Skin(false)) && self.skin(true).is_none() {
                return false;
            }
        }
        true
    }
}

fn module_of(dev: &wgpu::Device, sp: &Spliced) -> wgpu::ShaderModule {
    dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(sp.source.as_str().into()),
    })
}

/// Material pipelines (`material-split`). Kept at the end so no shipped
/// line moves.
#[cfg(feature = "material-split")]
impl Pipelines {
    /// A material key a draw asked for gets its job, once. One no draw has
    /// asked for in [`STALE_TICKS`] frames and not started yet is dropped
    /// (the 1x keys once the 4x pass takes over, a material left behind by
    /// a scene change), so it neither runs nor holds the page's "preparing"
    /// count; asked for again, it is queued again. The stand-ins are queued
    /// whatever their size (cut down to a few KB, but a material's own
    /// pipeline still takes a second or more).
    fn queue_splits(&self, fx: &PostEffects, slots: &ShaderSlots) {
        let now = self.q.tick.get();
        let stale: Vec<Key> = self
            .q
            .wanted
            .borrow()
            .iter()
            .filter(|(k, at)| matches!(k, Key::ModelSplit(..)) && now.saturating_sub(*at) > STALE_TICKS)
            .map(|e| e.0)
            .collect();
        if !stale.is_empty() {
            let mut dropped = Vec::new();
            self.q.jobs.borrow_mut().retain(|j| {
                let drop = stale.contains(&j.key) && matches!(j.purpose, Purpose::Initial { .. });
                if drop {
                    dropped.push(j.key);
                }
                !drop
            });
            self.q.wanted.borrow_mut().retain(|e| !stale.contains(&e.0));
            self.split_asked.borrow_mut().retain(|k| !dropped.contains(k));
        }
        let keys: Vec<Key> = self.q.wanted.borrow().iter().map(|e| e.0).filter(|k| matches!(k, Key::ModelSplit(..))).collect();
        for key in keys {
            if self.installed(key) || self.split_asked.borrow().contains(&key) {
                continue;
            }
            self.split_asked.borrow_mut().push(key);
            if let Some(Ok(sp)) = Self::splice_key(key, fx, slots, Bodies::Current) {
                self.q.push(Job { key, purpose: Purpose::Initial { fallback: false }, spliced: sp, current: true });
            }
        }
    }

    /// The 4x pass may start once each material drawn at 1x has its 4x
    /// pipeline: its own when its own 1x one is in (so a finished material
    /// never steps back to the stand-in), else at least the 4x stand-in.
    /// Asks for what is missing.
    fn split_ms_ready(&self) -> bool {
        use evani_material_recipes::split::OPAQUE;
        let drawn: Vec<Key> = self.q.wanted.borrow().iter().map(|e| e.0).chain(self.split.iter().map(|e| e.0)).collect();
        let mut ready = true;
        for k in drawn {
            let Key::ModelSplit(see, false, code, kind) = k else {
                continue;
            };
            if see != OPAQUE && see != super::split::SKIN {
                continue;
            }
            let full = Key::ModelSplit(see, true, code, kind);
            if self.installed(full) {
                continue;
            }
            self.q.want(full);
            if self.installed(k) {
                ready = false;
                continue;
            }
            #[cfg(feature = "skinning")]
            let lite = if see == super::split::SKIN { Key::SkinLite(true) } else { Key::ModelLite(true) };
            #[cfg(not(feature = "skinning"))]
            let lite = Key::ModelLite(true);
            self.q.want(lite);
            if !self.installed(lite) {
                ready = false;
            }
        }
        ready
    }
}

/// Frames a material key may go unasked before its queued job is dropped
/// (half a second at 60 frames a second).
#[cfg(feature = "material-split")]
const STALE_TICKS: u64 = 30;

#[cfg(feature = "material-split")]
impl Queue {
    /// Whether a draw asked for `key` in the last [`STALE_TICKS`] frames.
    pub(super) fn drawn_lately(&self, key: Key) -> bool {
        let now = self.tick.get();
        self.wanted.borrow().iter().any(|(k, at)| *k == key && now.saturating_sub(*at) <= STALE_TICKS)
    }
}

/// The other tiers' pipelines, warmed in the background (ov1 tier switch,
/// 2026-11-02). A tier switch between one sample (Lite, Low) and four
/// (Medium and up) needs the other sample count of every pipeline drawn so
/// far; built on the switch, the frame waits for it (material-view's Ultra
/// to Lite: 997 ms, two pipelines). Once nothing a draw asked for is
/// waiting, each drawn pipeline's twin is queued behind everything else
/// (one heavy at a time, as any job), only for a sample count a selectable
/// tier uses. Twins are not counted in `data-evani-preparing` (the page's
/// "preparing materials"); capture builds write them as
/// `data-evani-warming`. No download: the same shaders, compiled earlier.
#[cfg(feature = "forward")]
#[derive(Default)]
struct Twins {
    /// Queued or warming, until built.
    asked: RefCell<Vec<Key>>,
    /// Every twin ever asked for (one try each).
    tried: RefCell<Vec<Key>>,
    /// Warming up now.
    flight: Cell<u32>,
    /// The count last written to the page.
    #[cfg(feature = "capture")]
    shown: Cell<Option<u32>>,
}

#[cfg(feature = "forward")]
impl Twins {
    fn is(&self, k: Key) -> bool {
        self.asked.borrow().contains(&k)
    }

    fn queued(&self, jobs: &[Job]) -> u32 {
        jobs.iter().filter(|j| self.is(j.key)).count() as u32
    }

    fn started(&self, k: Key) {
        if self.is(k) {
            self.flight.set(self.flight.get() + 1);
        }
    }

    fn landed(&self, k: Key) {
        if self.is(k) {
            self.flight.set(self.flight.get().saturating_sub(1));
            self.done(k);
        }
    }

    fn done(&self, k: Key) {
        self.asked.borrow_mut().retain(|x| *x != k);
    }
}

/// The key with the other sample count, for the lazy keys that have one.
#[cfg(feature = "forward")]
pub(super) fn twin(k: Key) -> Option<Key> {
    Some(match k {
        Key::EngineMs(i) => return (i == super::FORWARD).then_some(Key::Engine(i)),
        Key::Engine(super::FORWARD) => Key::EngineMs(super::FORWARD),
        #[cfg(feature = "models")]
        Key::Model(ms) => Key::Model(!ms),
        #[cfg(feature = "models")]
        Key::ModelLite(ms) => Key::ModelLite(!ms),
        #[cfg(feature = "skinning")]
        Key::Skin(ms) => Key::Skin(!ms),
        #[cfg(feature = "skinning")]
        Key::SkinLite(ms) => Key::SkinLite(!ms),
        #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
        Key::Blend(ms) => Key::Blend(!ms),
        #[cfg(feature = "material-split")]
        Key::ModelSplit(see, ms, c, kind) if see != evani_material_recipes::split::GLASS => Key::ModelSplit(see, !ms, c, kind),
        _ => return None,
    })
}

/// Whether key `k` draws four samples (`None`: no sample count of its own).
#[cfg(feature = "forward")]
fn four(k: Key) -> Option<bool> {
    match k {
        Key::Engine(super::FORWARD) => Some(false),
        Key::EngineMs(_) => Some(true),
        #[cfg(feature = "models")]
        Key::Model(ms) | Key::ModelLite(ms) => Some(ms),
        #[cfg(feature = "skinning")]
        Key::Skin(ms) | Key::SkinLite(ms) => Some(ms),
        #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
        Key::Blend(ms) => Some(ms),
        #[cfg(feature = "material-split")]
        Key::ModelSplit(_, ms, ..) => Some(ms),
        _ => None,
    }
}

#[cfg(feature = "forward")]
impl Pipelines {
    /// The installed keys that may have a twin.
    fn twin_sources(&self) -> Vec<Key> {
        let mut v = vec![Key::Engine(super::FORWARD), Key::EngineMs(super::FORWARD)];
        #[cfg(feature = "models")]
        v.extend([Key::Model(false), Key::Model(true), Key::ModelLite(false), Key::ModelLite(true)]);
        #[cfg(feature = "skinning")]
        v.extend([Key::Skin(false), Key::Skin(true), Key::SkinLite(false), Key::SkinLite(true)]);
        #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
        v.extend([Key::Blend(false), Key::Blend(true)]);
        v.retain(|k| self.installed(*k)); #[cfg(feature = "material-split")] v.extend(self.split.iter().map(|e| e.0));
        v
    }

    /// Queue the twins of what is drawn once nothing else waits (see
    /// [`Twins`]). Only for a sample count some selectable tier draws with.
    fn queue_twins(&self, fx: &PostEffects, slots: &ShaderSlots, world: &World) {
        let t = &self.q.twins;
        #[cfg(feature = "capture")]
        {
            let n = t.asked.borrow().len() as u32;
            if t.shown.get() != Some(n) {
                t.shown.set(Some(n));
                if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
                    let _ = el.set_attribute("data-evani-warming", &n.to_string());
                }
            }
        }
        if !t.asked.borrow().is_empty() || self.q.waiting() > 0 || !self.q.warm.borrow().is_empty() || self.q.heavy.get() + self.q.light.get() > 0 {
            return;
        }
        let Some(tiers) = world.try_res::<evani_core::tiers::Tiers>() else { return };
        let uses = |four: bool| tiers.presets().iter().any(|p| !p.stereo && (p.msaa >= super::MSAA_SAMPLES) == four);
        for k in self.twin_sources() {
            let Some(tw) = twin(k) else { continue };
            if self.installed(tw) || t.tried.borrow().contains(&tw) || !four(tw).is_some_and(uses) || self.q.jobs.borrow().iter().any(|j| j.key == tw) {
                continue;
            }
            t.tried.borrow_mut().push(tw);
            let Some(Ok(sp)) = Self::splice_key(tw, fx, slots, Bodies::Current) else { continue };
            t.asked.borrow_mut().push(tw); #[cfg(feature = "material-split")] if matches!(tw, Key::ModelSplit(..)) { self.split_asked.borrow_mut().push(tw); }
            self.q.push(Job { key: tw, purpose: Purpose::Initial { fallback: false }, spliced: sp, current: true });
        }
    }
}
