//! Live capture readback (feature `capture`, live builds only): takes the
//! pending [`CaptureQueue`] requests, applies the camera override, copies the
//! final colour target (`ldr`, sRGB RGBA8) into a mappable buffer after the
//! graph walk, maps it asynchronously and completes the requests a frame or
//! two later with tightly packed pixels. Never stalls a frame.
//!
//! Downscaling and PNG encoding are evani-live's job (dev only), so nothing
//! here encodes images.

use std::cell::Cell;
use std::rc::Rc;

use evani_core::camera::CameraPose;
use evani_core::capture::{CanvasHud, CaptureQueue, CaptureRequest, CaptureResult, CaptureView, HoldCanvas};
use evani_core::ecs::World;
use evani_core::overlay::RenderStats;
use evani_core::time::Time;

const NEW: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;

/// Bytes per pixel of the `ldr` target (`Rgba8UnormSrgb`).
const BPP: u32 = 4;

/// One copy in flight: the requests it serves and its buffer.
struct InFlight {
    ids: Vec<u64>,
    buf: wgpu::Buffer,
    state: Rc<Cell<u8>>,
    size: [u32; 2],
    padded_row: u32,
    frame: u64,
    sim_time: f64,
    stats: RenderStats,
    camera: Option<CameraPose>,
    canvas_hud: Option<CanvasHud>,
}

#[derive(Default)]
pub struct Readbacks {
    /// Requests taken this frame, waiting for `record`.
    taken: Vec<CaptureRequest>,
    flights: Vec<InFlight>,
    /// This frame's canvas HUD, noted before the draw lists are cleared.
    hud: Option<CanvasHud>,
}

impl Readbacks {
    /// Nothing taken and nothing in flight.
    pub fn idle(&self) -> bool {
        self.taken.is_empty() && self.flights.is_empty()
    }

    /// Start of a rendered frame (the surface texture is acquired): take the
    /// pending requests and return the camera pose to render with, if any
    /// (a `Path` request's pose wins, else `camera_override`). Requests
    /// taken in the same frame share that frame's view.
    pub fn begin(&mut self, world: &World) -> Option<CameraPose> {
        let mut q = world.try_res_mut::<CaptureQueue>()?;
        if q.has_pending() {
            self.taken.extend(q.take_pending());
        }
        let path = self.taken.iter().find_map(|r| match &r.view {
            CaptureView::Path { path, sim_time } => Some(path.pose(*sim_time)),
            CaptureView::Current => None,
        });
        path.or(q.camera_override)
    }

    /// While sprites are packed (before the draw lists are cleared): note
    /// the canvas HUD of a frame that is being captured.
    pub fn note_hud(&mut self, rw: &crate::world::RenderWorld) {
        self.hud = if self.taken.is_empty() { None } else { rw.canvas_hud() };
    }

    /// After the graph walk, before submit: copy `ldr` for this frame's
    /// requests. `ldr` is `None` when the graph has no such target.
    pub fn record(
        &mut self,
        world: &World,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        ldr: Option<(&wgpu::Texture, [u32; 2])>,
    ) {
        if self.taken.is_empty() {
            return;
        }
        let ids: Vec<u64> = self.taken.drain(..).map(|r| r.id).collect();
        let Some((tex, size)) = ldr else {
            complete_err(world, &ids, "no ldr colour target to read back");
            return;
        };
        let padded_row = (size[0] * BPP).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.capture"),
            size: u64::from(padded_row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        let (frame, sim_time) = {
            let t = world.res::<Time>();
            (t.frame, t.render_seconds())
        };
        let camera = world.try_res::<crate::world::RenderWorld>().map(|rw| rw.camera);
        let canvas_hud = self.hud.take();
        self.flights.push(InFlight {
            ids,
            buf,
            state: Rc::new(Cell::new(NEW)),
            size,
            padded_row,
            frame,
            sim_time,
            stats: RenderStats::default(),
            camera,
            canvas_hud,
        });
    }

    /// After submit: start mapping this frame's copies and keep the frame's
    /// stats with them.
    pub fn after_submit(&mut self, stats: &RenderStats) {
        for f in self.flights.iter_mut().filter(|f| f.state.get() == NEW) {
            f.stats = stats.clone();
            let state = f.state.clone();
            state.set(MAPPING);
            f.buf.map_async(wgpu::MapMode::Read, .., move |r| {
                state.set(if r.is_ok() { READY } else { FAILED });
            });
        }
    }

    /// Start of every frame: complete the requests whose copy is mapped.
    pub fn poll(&mut self, world: &World) {
        if self.flights.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.flights.len() {
            let st = self.flights[i].state.get();
            if st != READY && st != FAILED {
                i += 1;
                continue;
            }
            let f = self.flights.swap_remove(i);
            let rgba = if st == READY { unpad(&f) } else { Err("buffer map failed".to_string()) };
            f.buf.unmap();
            let Some(mut q) = world.try_res_mut::<CaptureQueue>() else {
                continue;
            };
            for &id in &f.ids {
                q.complete(CaptureResult {
                    id,
                    rgba: rgba.clone(),
                    width: f.size[0],
                    height: f.size[1],
                    frame: f.frame,
                    sim_time: f.sim_time,
                    stats: f.stats.clone(),
                    camera: f.camera,
                    canvas_hud: f.canvas_hud,
                });
            }
        }
    }

    /// The device is gone: fail everything taken or in flight.
    pub fn fail_all(&mut self, world: &World, reason: &str) {
        let mut ids: Vec<u64> = self.taken.drain(..).map(|r| r.id).collect();
        ids.extend(self.flights.drain(..).flat_map(|f| f.ids));
        complete_err(world, &ids, reason);
    }
}

/// Copy the mapped rows into a tight `w * h * 4` buffer.
fn unpad(f: &InFlight) -> Result<Vec<u8>, String> {
    let view = f.buf.get_mapped_range(..).map_err(|e| format!("map range: {e:?}"))?;
    let row = (f.size[0] * BPP) as usize;
    let mut out = Vec::with_capacity(row * f.size[1] as usize);
    for y in 0..f.size[1] as usize {
        let at = y * f.padded_row as usize;
        match view.get(at..at + row) {
            Some(s) => out.extend_from_slice(s),
            None => return Err("short readback".into()),
        }
    }
    Ok(out)
}

fn complete_err(world: &World, ids: &[u64], reason: &str) {
    let Some(mut q) = world.try_res_mut::<CaptureQueue>() else {
        return;
    };
    for &id in ids {
        q.complete(CaptureResult {
            id,
            rgba: Err(reason.to_string()),
            width: 0,
            height: 0,
            frame: 0,
            sim_time: 0.0,
            stats: RenderStats::default(),
            camera: None,
            canvas_hud: None,
        });
    }
}

/// The canvas hold ([`HoldCanvas`], docs/LIVE_API.md 6.30):
/// while a trial run tries settings, the frame is drawn as usual but its
/// present pass writes into this off-screen texture (same size and format
/// as the canvas, so the pass costs what it always does) instead of the
/// canvas, and the canvas keeps the last frame it presented. A canvas
/// resize (which clears it: wgpu sets the canvas's width and height) waits
/// until the hold ends.
#[derive(Default)]
pub struct CanvasHold {
    /// The hold is on for this frame.
    pub active: bool,
    /// The canvas still needs the configure a resize skipped.
    pub surface_stale: bool,
    target: Option<(wgpu::TextureView, [u32; 2], wgpu::TextureFormat)>,
}

impl CanvasHold {
    /// Start of a frame: read the hold from the queue. Off frees the
    /// off-screen texture at once.
    pub fn update(&mut self, world: &World) {
        self.active = world.try_res::<HoldCanvas>().is_some_and(|h| h.0);
        if !self.active {
            self.target = None;
        }
    }

    /// The view the present pass draws into while held (made, or remade
    /// after a resize, on demand); `None` when not held.
    pub fn view(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2]) -> Option<wgpu::TextureView> {
        if !self.active {
            return None;
        }
        let size = [size[0].max(1), size[1].max(1)];
        match &self.target {
            Some((v, s, f)) if *s == size && *f == format => Some(v.clone()),
            _ => {
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("evani.held_present"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let v = tex.create_view(&wgpu::TextureViewDescriptor::default());
                self.target = Some((v.clone(), size, format));
                Some(v)
            }
        }
    }
}
