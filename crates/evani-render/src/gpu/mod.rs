//! The WebGPU side of the renderer (wasm only): device start-up and
//! recovery, canvas size tracking, error reporting and the test hooks.
//! [`frame::Gpu`] owns one live device and draws.

#[cfg(feature = "capture")]
mod capture;
mod frame;
#[cfg(feature = "hooks")]
mod hooks;
#[cfg(feature = "particles")]
mod particles; #[cfg(feature = "msdf")] mod msdf; #[cfg(feature = "pseudo3d")] mod pseudo3d;
mod pipelines; #[cfg(feature = "models")] mod models; #[cfg(feature = "relief")] mod models_relief; #[cfg(all(feature = "look-surface", not(feature = "relief")))] mod models_surface; #[cfg(feature = "point-lights")] mod lights; #[cfg(feature = "ambient")] mod ambient; #[cfg(feature = "gtao")] mod gtao; #[cfg(all(feature = "material-layers", not(feature = "relief")))] mod models_layers;
mod timing; #[cfg(all(feature = "lights2d", not(feature = "msdf")))] mod lights2d;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use evani_core::ecs::World;
use evani_core::overlay::{ErrorLog, RenderStats};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::control::QualityDriver;
use crate::world::RenderWorld;
use crate::{GpuStatus, RenderParams, no_webgpu_message, no_webgpu_reason, secure_context, webgpu_available};

/// Uncaptured GPU errors kept between frames (the rest are counted).
const MAX_PENDING_ERRORS: usize = 16;
/// Adaptive quality ignores this long after a device start or recovery.
const SETTLE_AFTER_START_MS: f64 = 1500.0;

/// What the async start-up hands to the next frame.
pub(crate) struct Init {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub timestamps: bool,
    /// The GPU name read from the browser when wgpu's is empty.
    pub adapter_label: String,
}

type Observer = (web_sys::ResizeObserver, Closure<dyn FnMut(js_sys::Array)>);
type InitSlot = Rc<RefCell<Option<Result<Init, String>>>>;

#[derive(Default)]
pub struct Renderer {
    status: GpuStatus,
    started: bool,
    pending: InitSlot,
    gpu: Option<frame::Gpu>,
    driver: QualityDriver,
    canvas: Option<web_sys::HtmlCanvasElement>,
    /// `render.pixel_art` as last applied to the canvas's upscale filter.
    pixelated: Option<bool>,
    css: Rc<Cell<[f32; 2]>>,
    /// The canvas's exact device-pixel box and the devicePixelRatio it was
    /// measured at (`[0, 0]` until the browser reports one).
    #[cfg(feature = "pixel-art")]
    px_box: Rc<Cell<([u32; 2], f32)>>,
    resize: Option<Observer>,
    errors: Arc<Mutex<Vec<String>>>,
    lost: Arc<AtomicBool>,
    lost_msg: Arc<Mutex<String>>,
    /// Device generation; callbacks of older devices are ignored.
    epoch: Arc<AtomicU32>,
    recoveries: u32,
    #[cfg(feature = "hooks")]
    hooks: hooks::Hooks,
}

/// Show the right "no WebGPU" message for what the browser reports now.
fn show_no_webgpu(has_gpu: bool, has_adapter: bool) {
    if let Some((title, body)) = no_webgpu_message(no_webgpu_reason(secure_context(), has_gpu, has_adapter)) {
        evani_core::web::show_message(title, body);
    }
}

impl Renderer {
    pub fn status(&self) -> GpuStatus {
        self.status.clone()
    }

    pub fn start(&mut self, world: &mut World, params: Option<RenderParams>) {
        self.started = true;
        self.driver = QualityDriver::new(params.map(|p| p.aq));
        if !webgpu_available() {
            self.status = GpuStatus::Unavailable("navigator.gpu missing".into());
            world.res_mut::<ErrorLog>().error("WebGPU unavailable");
            show_no_webgpu(false, false);
            return;
        }
        let Some(canvas) = find_canvas() else {
            self.status = GpuStatus::Unavailable("no canvas".into());
            world.res_mut::<ErrorLog>().error("render: could not find or create #evani-canvas");
            return;
        };
        self.css.set([canvas.client_width() as f32, canvas.client_height() as f32]);
        self.observe(&canvas, world);
        self.canvas = Some(canvas);
        #[cfg(feature = "hooks")]
        self.hooks.install();
        self.request_device();
    }

    /// Track the canvas's CSS size (and, with the `pixel-art` feature, its
    /// exact device-pixel box).
    fn observe(&mut self, canvas: &web_sys::HtmlCanvasElement, world: &mut World) {
        let css = self.css.clone();
        #[cfg(feature = "pixel-art")]
        let px_box = self.px_box.clone();
        let cb = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
            for e in entries.iter() {
                #[cfg(feature = "pixel-art")]
                px_box.set(device_box(&e));
                if let Ok(e) = e.dyn_into::<web_sys::ResizeObserverEntry>() {
                    let r = e.content_rect();
                    css.set([r.width() as f32, r.height() as f32]);
                }
            }
        });
        match web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
            Ok(obs) => {
                #[cfg(feature = "pixel-art")]
                observe_device_box(&obs, canvas);
                #[cfg(not(feature = "pixel-art"))]
                obs.observe(canvas);
                self.resize = Some((obs, cb));
            }
            Err(e) => world
                .res_mut::<ErrorLog>()
                .error(format!("ResizeObserver: {}", evani_core::web::js_err(e))),
        }
    }

    fn request_device(&mut self) {
        let Some(canvas) = self.canvas.clone() else {
            return;
        };
        self.status = GpuStatus::Pending;
        let slot = self.pending.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r = init(canvas).await;
            *slot.borrow_mut() = Some(r);
        });
    }

    pub fn frame(&mut self, world: &mut World, params: &RenderParams) {
        if !self.started {
            return;
        }
        self.drain_errors(world);
        #[cfg(feature = "hooks")]
        {
            if self.hooks.poll(world)
                && let Some(g) = &self.gpu
            {
                world.res_mut::<ErrorLog>().note("test hook: destroying the GPU device");
                g.device.destroy();
            }
        }
        if self.lost.swap(false, Ordering::SeqCst) && self.gpu.is_some() {
            let msg = self.lost_msg.lock().map(|m| m.clone()).unwrap_or_default();
            self.recoveries += 1;
            bump_lost(world);
            world.res_mut::<ErrorLog>().error(format!(
                "GPU device lost ({msg}); recovering (#{})",
                self.recoveries
            ));
            #[cfg(feature = "capture")]
            if let Some(g) = &mut self.gpu {
                g.readbacks.fail_all(world, "GPU device lost");
            }
            self.epoch.fetch_add(1, Ordering::SeqCst);
            self.gpu = None;
            self.request_device();
        }
        if self.gpu.is_none() {
            let ready = self.pending.borrow_mut().take();
            match ready {
                Some(Ok(init)) => self.adopt(init, world),
                Some(Err(e)) => {
                    world.res_mut::<ErrorLog>().error(format!("WebGPU start failed: {e}"));
                    if self.recoveries == 0 {
                        show_no_webgpu(true, false);
                    } else {
                        // A failed recovery counts as another loss.
                        bump_lost(world);
                    }
                    self.status = GpuStatus::Unavailable(e);
                }
                None => {}
            }
        }
        self.upscale_filter(world, params);
        #[cfg_attr(not(feature = "pixel-art"), allow(unused_mut))]
        let mut css = self.css.get();
        let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio() as f32);
        // Pixel art sizes its backing store from the browser's exact
        // device-pixel box: `round(css x dpr)` can be one pixel off it at a
        // fractional devicePixelRatio (125 %, 150 % Windows scaling), and the
        // browser then stretches the canvas by that pixel, doubling one row
        // or column of art pixels. A CSS size of box / dpr makes
        // `render_size_exact` land on the box exactly.
        // The box is trusted only within a pixel or so of css x dpr: device
        // emulation (DevTools, headless captures) reports it unscaled.
        #[cfg(feature = "pixel-art")]
        if self.pixelated == Some(true) {
            let ([w, h], at) = self.px_box.get();
            let near = |b: u32, c: f32| (b as f32 - c * dpr).abs() <= 1.5;
            if w > 0 && h > 0 && at == dpr && dpr > 0.0 && near(w, css[0]) && near(h, css[1]) {
                css = [w as f32 / dpr, h as f32 / dpr];
            }
        }
        // Parked off-screen (live builds only): draw a few frames a second.
        #[cfg(feature = "capture")]
        if let Some(g) = &self.gpu
            && !paced(world, g.readbacks.idle())
        {
            world.res_mut::<RenderWorld>().clear();
            return;
        }
        match &mut self.gpu {
            Some(g) => g.frame(world, params, &mut self.driver, css, dpr),
            None => world.res_mut::<RenderWorld>().clear(),
        }
    }

}

/// Draw this frame? [`evani_core::capture::RenderPace`] (inserted by
/// evani-live, docs/LIVE_API.md 3.5) slows drawing while the page is parked;
/// a waiting capture, a camera override or a readback in flight always draws.
#[cfg(feature = "capture")]
fn paced(world: &World, readbacks_idle: bool) -> bool {
    use evani_core::capture::{CaptureQueue, RenderPace};
    let Some(mut pace) = world.try_res_mut::<RenderPace>() else {
        return true;
    };
    let busy = !readbacks_idle
        || world
            .try_res::<CaptureQueue>()
            .is_some_and(|q| q.has_pending() || q.overriding());
    let now = world.res::<evani_core::time::Time>().now_ms;
    pace.admit(now, busy)
}

impl Renderer {
    /// The browser upscales the canvas (render scale, DPR cap) to its CSS
    /// size: smooth (linear) by default, nearest with `render.pixel_art`.
    fn upscale_filter(&mut self, world: &World, params: &RenderParams) {
        let on = world.res::<evani_core::params::ParamRegistry>().bool(params.pixel_art);
        if self.pixelated == Some(on) {
            return;
        }
        if let Some(c) = &self.canvas {
            let _ = c
                .style()
                .set_property("image-rendering", if on { "pixelated" } else { "auto" });
            self.pixelated = Some(on);
        }
    }

    /// Build the renderer on a freshly started device.
    fn adopt(&mut self, init: Init, world: &mut World) {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let errors = self.errors.clone();
        init.device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            if let Ok(mut v) = errors.lock()
                && v.len() < MAX_PENDING_ERRORS
            {
                v.push(e.to_string());
            }
        }));
        let (lost, lost_msg, cur) = (self.lost.clone(), self.lost_msg.clone(), self.epoch.clone());
        init.device.set_device_lost_callback(move |reason, msg| {
            if cur.load(Ordering::SeqCst) == epoch {
                if let Ok(mut m) = lost_msg.lock() {
                    *m = format!("{reason:?}: {msg}");
                }
                lost.store(true, Ordering::SeqCst);
            }
        });
        let gpu = frame::Gpu::new(init, world, epoch);
        // Pipelines compile over the next frames: keep adaptive quality from
        // reading that as the scene being too heavy.
        self.driver.settle(world, SETTLE_AFTER_START_MS);
        let ts = gpu.timestamps();
        self.status = GpuStatus::Ready {
            adapter: gpu.adapter_name.clone(),
        };
        {
            let mut st = world.res_mut::<RenderStats>();
            if !ts {
                st.gpu_note = "timestamp-query unsupported";
            }
        }
        world.res_mut::<ErrorLog>().note(format!(
            "webgpu: {} (max texture {}, timestamps {})",
            gpu.adapter_name,
            gpu.max_dim(),
            if ts { "on" } else { "off" }
        ));
        self.gpu = Some(gpu);
    }

    fn drain_errors(&mut self, world: &mut World) {
        let Ok(mut v) = self.errors.lock() else {
            return;
        };
        if v.is_empty() {
            return;
        }
        let mut log = world.res_mut::<ErrorLog>();
        for e in v.drain(..) {
            log.error(format!("gpu: {e}"));
        }
    }

    pub fn visibility(&mut self, world: &mut World, hidden: bool) {
        self.driver.visibility(world, hidden);
    }

    /// Extra overlay status: render size and recoveries.
    pub fn status_extra(&self, out: &mut String) {
        use core::fmt::Write;
        if let Some(g) = &self.gpu {
            let _ = write!(out, " · {}x{}", g.last_size[0], g.last_size[1]);
            let samples = g.msaa_samples();
            if samples > 1 {
                let _ = write!(out, " · msaa {samples}x");
            }
        }
        if self.driver.warming_up() {
            out.push_str(" · tier warm-up");
        }
        if self.recoveries > 0 {
            let _ = write!(out, " · recovered {}x", self.recoveries);
        }
    }
}

/// Count a device loss (or failed recovery) in `DeviceStatus`; auto-revert
/// in live watches it, shipped games just count.
fn bump_lost(world: &World) {
    if let Some(mut d) = world.try_res_mut::<evani_core::capture::DeviceStatus>() {
        d.lost_count = d.lost_count.saturating_add(1);
    }
}

/// Observe `canvas`'s device-pixel content box, so the observer also fires
/// when only the devicePixelRatio changes; browsers without that box
/// (`observe` throws on the option) fall back to the content box.
#[cfg(feature = "pixel-art")]
fn observe_device_box(obs: &web_sys::ResizeObserver, canvas: &web_sys::HtmlCanvasElement) {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;
    let opts = js_sys::Object::new();
    let _ = Reflect::set(&opts, &JsValue::from_str("box"), &JsValue::from_str("device-pixel-content-box"));
    let observed = Reflect::get(obs, &JsValue::from_str("observe"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .is_some_and(|f| f.call2(obs, canvas, &opts).is_ok());
    if !observed {
        obs.observe(canvas);
    }
}

/// A resize entry's `devicePixelContentBoxSize[0]` in whole device pixels
/// and the devicePixelRatio now; `[0, 0]` where the browser has none.
#[cfg(feature = "pixel-art")]
fn device_box(entry: &wasm_bindgen::JsValue) -> ([u32; 2], f32) {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio() as f32);
    let size = Reflect::get(entry, &JsValue::from_str("devicePixelContentBoxSize"))
        .ok()
        .filter(|a| !a.is_undefined() && !a.is_null())
        .and_then(|a| Reflect::get_u32(&a, 0).ok())
        .filter(|s| !s.is_undefined() && !s.is_null());
    let dim = |s: &JsValue, k: &str| Reflect::get(s, &JsValue::from_str(k)).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    match size {
        // Horizontal writing mode: inline is the width, block the height.
        Some(s) => ([dim(&s, "inlineSize").max(0.0) as u32, dim(&s, "blockSize").max(0.0) as u32], dpr),
        None => ([0, 0], dpr),
    }
}

/// `#evani-canvas`, created (full window) when the page has none.
fn find_canvas() -> Option<web_sys::HtmlCanvasElement> {
    let doc = web_sys::window()?.document()?;
    if let Some(el) = doc.get_element_by_id("evani-canvas") {
        return el.dyn_into().ok();
    }
    let el = doc.create_element("canvas").ok()?;
    el.set_id("evani-canvas");
    let _ = el.set_attribute("style", "display:block;width:100vw;height:100vh");
    doc.body()?.append_child(&el).ok()?;
    el.dyn_into().ok()
}

async fn init(canvas: web_sys::HtmlCanvasElement) -> Result<Init, String> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::Instance::new(desc);
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|e| format!("surface: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| format!("adapter: {e}"))?;
    let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    #[cfg(not(feature = "models"))] let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("evani"),
            required_features: if timestamps {
                wgpu::Features::TIMESTAMP_QUERY
            } else {
                wgpu::Features::empty()
            },
            required_limits: adapter.limits(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: Default::default(),
        })
        .await
        .map_err(|e| format!("device: {e}"))?; #[cfg(feature = "models")] let (device, queue) = models::request_device(&adapter, timestamps).await?;
    let mut adapter_label = adapter.get_info().name;
    if adapter_label.trim().is_empty() {
        adapter_label = browser_adapter_label().await.unwrap_or_default();
    }
    Ok(Init {
        instance,
        surface,
        adapter,
        device,
        queue,
        timestamps,
        adapter_label,
    })
}

/// The GPU name from the browser's own `GPUAdapterInfo`. wgpu 30 copies only
/// `description` into the adapter name, and Chrome fills that only with
/// `--enable-webgpu-developer-features`; `vendor` and `architecture` are
/// there by default. Asks `navigator.gpu` for an adapter with the same
/// preference and reads `adapter.info`, or `requestAdapterInfo()` in older
/// browsers. `None` if the browser gives nothing.
async fn browser_adapter_label() -> Option<String> {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_futures::JsFuture;
    let get = |o: &JsValue, k: &str| {
        Reflect::get(o, &JsValue::from_str(k))
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null())
    };
    let call = |o: &JsValue, k: &str, arg: Option<&JsValue>| -> Option<js_sys::Promise> {
        let f: js_sys::Function = get(o, k)?.dyn_into().ok()?;
        let r = match arg {
            Some(a) => f.call1(o, a),
            None => f.call0(o),
        };
        r.ok()?.dyn_into().ok()
    };
    let navigator: JsValue = web_sys::window()?.navigator().into();
    let gpu = get(&navigator, "gpu")?;
    let opts = js_sys::Object::new();
    Reflect::set(&opts, &JsValue::from_str("powerPreference"), &JsValue::from_str("high-performance")).ok()?;
    let adapter = JsFuture::from(call(&gpu, "requestAdapter", Some(&opts))?).await.ok()?;
    if adapter.is_null() || adapter.is_undefined() {
        return None;
    }
    let info = match get(&adapter, "info") {
        Some(i) => i,
        None => JsFuture::from(call(&adapter, "requestAdapterInfo", None)?).await.ok()?,
    };
    let field = |k: &str| get(&info, k).and_then(|v| v.as_string()).unwrap_or_default();
    let label = crate::adapter_label(&field("description"), &field("vendor"), &field("architecture"), &field("device"));
    (!label.is_empty()).then_some(label)
}
