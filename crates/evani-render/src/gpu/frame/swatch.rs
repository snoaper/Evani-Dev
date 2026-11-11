//! The swatch pass (feature `swatches`, `crate::swatch`): a child of `frame`
//! so it can read the frame's private buffers. Four hooks:
//!
//! - [`swatch_poll`] at the start of a frame: finished copies go back to
//!   [`Swatches`] as pictures.
//! - [`swatch_take`] once the views and the models are prepared: this
//!   frame's requests (at most [`PER_FRAME`], only models whose material
//!   pipelines are built), each with its own frame uniform in the frame's
//!   light.
//! - [`swatch_draw`] after the graph walk: each ball in its cell of the
//!   offscreen target (the forward pass's sample count, so the forward
//!   pass's material pipelines draw it), the scene's tonemap pass into an
//!   sRGB target, and the cells copied into a mappable buffer.
//! - [`swatch_after_submit`]: the copies start mapping.
//!
//! [`swatch_poll`]: super::Gpu::swatch_poll
//! [`swatch_take`]: super::Gpu::swatch_take
//! [`swatch_draw`]: super::Gpu::swatch_draw
//! [`swatch_after_submit`]: super::Gpu::swatch_after_submit

use std::cell::Cell;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::plugin::MaterialDesc;

use super::{a_group, pipelines};
use crate::graph::TargetKind;
use crate::shaders::{self, Lighting};
use crate::swatch::{IN_FLIGHT, MAX_SIZE, PER_FRAME, SwatchImage, Swatches};

const NEW: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;
/// The offscreen target: one cell per request of a frame, side by side.
const WIDTH: u32 = MAX_SIZE * PER_FRAME as u32;
/// Bytes per row of the copy (whole cells, already 256-byte aligned).
const ROW: u32 = (WIDTH * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
/// The colour behind the ball (linear, before the tonemap): the page's dark
/// panel, so a round crop's edge never shows a bright rim.
const CLEAR: wgpu::Color = wgpu::Color { r: 0.012, g: 0.012, b: 0.014, a: 1.0 };

/// One request taken this frame.
pub(super) struct Taken {
    key: u64,
    model: usize,
    off: u32,
    cell: u32,
    size: u32,
    clear: wgpu::Color,
}

/// The offscreen targets, made on first use at the forward pass's sample count.
struct Targets {
    ms: bool,
    _color: Option<wgpu::Texture>,
    color: Option<wgpu::TextureView>,
    _depth: wgpu::Texture,
    depth: wgpu::TextureView,
    _hdr: wgpu::Texture,
    hdr: wgpu::TextureView,
    hdr_bg: wgpu::BindGroup,
    ldr: wgpu::Texture,
    ldr_view: wgpu::TextureView,
}

/// One copy on its way back.
struct Flight {
    items: Vec<(u64, u32, u32)>,
    buf: wgpu::Buffer,
    state: Rc<Cell<u8>>,
}

#[derive(Default)]
pub(super) struct SwatchGpu {
    targets: Option<Targets>,
    taken: Vec<Taken>,
    flights: Vec<Flight>,
}

impl super::Gpu {
    /// Start of a frame: finished copies become pictures.
    pub(super) fn swatch_poll(&mut self, world: &World) {
        if self.swatch.flights.is_empty() {
            return;
        }
        let Some(mut s) = world.try_res_mut::<Swatches>() else { return };
        let mut i = 0;
        while i < self.swatch.flights.len() {
            let st = self.swatch.flights[i].state.get();
            if st != READY && st != FAILED {
                i += 1;
                continue;
            }
            let f = self.swatch.flights.swap_remove(i);
            if st == FAILED {
                for &(key, ..) in &f.items {
                    s.fail(key, "the swatch copy could not be mapped");
                }
                continue;
            }
            match f.buf.get_mapped_range(..) {
                Ok(view) => {
                    for &(key, cell, size) in &f.items {
                        s.complete(SwatchImage { key, size, rgba: cut(&view, cell, size) });
                    }
                }
                Err(e) => {
                    for &(key, ..) in &f.items {
                        s.fail(key, format!("swatch map range: {e:?}"));
                    }
                }
            }
            f.buf.unmap();
        }
    }

    /// Take this frame's requests and write each one's frame uniform (the
    /// frame's light; the shading block of the frame words is the frame's).
    pub(super) fn swatch_take(&mut self, world: &World, lighting: &Lighting, materials: &[MaterialDesc]) {
        self.swatch.taken.clear();
        if self.swatch.flights.len() >= IN_FLIGHT {
            return;
        }
        let Some(mut s) = world.try_res_mut::<Swatches>() else { return };
        if s.pending() == 0 {
            return;
        }
        let ms = self.msaa.is_some();
        let (models, pipes) = (&self.models, &self.pipes);
        let reqs = s.take_ready(PER_FRAME, |r, have| have >= r.uploads && models.swatch_ready(r.model.0 as usize, ms, pipes));
        if !reqs.is_empty() {
            s.busy_frames += 1;
        }
        for (cell, r) in reqs.into_iter().enumerate() {
            let vp = shaders::view_proj(&r.camera, 1.0);
            shaders::write_frame(&mut self.frame_words, &vp, r.camera.eye, lighting, materials);
            let off = self.ring.push(&self.frame_words[..]);
            let clear = r.background.map_or(CLEAR, |c| wgpu::Color { r: f64::from(c[0]), g: f64::from(c[1]), b: f64::from(c[2]), a: 1.0 });
            self.swatch.taken.push(Taken { key: r.key, model: r.model.0 as usize, off, cell: cell as u32, size: r.size, clear });
        }
    }

    /// Make (or remake for a new sample count) the offscreen targets.
    fn swatch_targets(&mut self, ms: bool) {
        if self.swatch.targets.as_ref().is_some_and(|t| t.ms == ms) {
            return;
        }
        let make = |label, kind, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: WIDTH, height: MAX_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: pipelines::target_format(kind, self.formats),
                usage,
                view_formats: &[],
            })
        };
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let samples = if ms { pipelines::MSAA_SAMPLES } else { 1 };
        let color = ms.then(|| make("evani.swatch.msaa", TargetKind::HdrColor, samples, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let depth = make("evani.swatch.depth", TargetKind::Depth, samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let hdr = make("evani.swatch.hdr", TargetKind::HdrColor, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let ldr = make("evani.swatch.ldr", TargetKind::Ldr, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let hdr_view = view(&hdr);
        let hdr_bg = a_group(&self.device, &self.layouts, &hdr_view, &self.sampler, &self.ring.buf);
        self.swatch.targets = Some(Targets {
            ms,
            color: color.as_ref().map(view),
            _color: color,
            depth: view(&depth),
            _depth: depth,
            hdr: hdr_view,
            _hdr: hdr,
            hdr_bg,
            ldr_view: view(&ldr),
            ldr,
        });
    }

    /// Draw this frame's balls, tonemap them and copy them out. Returns the
    /// draw count.
    pub(super) fn swatch_draw(&mut self, enc: &mut wgpu::CommandEncoder, tone_off: u32, tris: &mut u32) -> u32 {
        if self.swatch.taken.is_empty() {
            return 0;
        }
        let ms = self.msaa.is_some();
        let Some(tone) = self.pipes.engine(pipelines::TONEMAP) else {
            return 0;
        };
        let tone = tone.clone();
        self.swatch_targets(ms);
        let Some(t) = self.swatch.targets.as_ref() else { return 0 };
        let mut draws = 0;
        // Every request of a frame shares one background (`take_ready`).
        let clear = self.swatch.taken.first().map_or(CLEAR, |k| k.clear);
        {
            let (view, resolve) = match &t.color {
                Some(c) => (c, Some(&t.hdr)),
                None => (&t.hdr, None),
            };
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("swatches"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store } },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(crate::depth::CLEAR), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            #[cfg(feature = "point-lights")]
            self.lights.bind(&mut rp);
            #[cfg(feature = "kit-material")]
            self.kit.bind(&mut rp);
            #[cfg(feature = "shadows")]
            let frame_bg = &self.shadows.fwd_bg;
            #[cfg(feature = "ambient")]
            let frame_bg = self.ambient.bg.as_ref().unwrap_or(frame_bg);
            #[cfg(not(feature = "shadows"))]
            let frame_bg = &self.frame_bg;
            for k in &self.swatch.taken {
                let px = [(k.cell * MAX_SIZE) as f32, 0.0, k.size as f32, k.size as f32];
                draws += self.models.draw_swatch(&mut rp, frame_bg, k.off, px, k.model, ms, &self.pipes, tris);
            }
        }
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("swatches.tonemap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.ldr_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&tone);
            rp.set_bind_group(0, &t.hdr_bg, &[tone_off]);
            rp.draw(0..3, 0..1);
            draws += 1;
        }
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.swatch.copy"),
            size: u64::from(ROW) * u64::from(MAX_SIZE),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.ldr, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ROW), rows_per_image: Some(MAX_SIZE) } },
            wgpu::Extent3d { width: WIDTH, height: MAX_SIZE, depth_or_array_layers: 1 },
        );
        let items = self.swatch.taken.drain(..).map(|k| (k.key, k.cell, k.size)).collect();
        self.swatch.flights.push(Flight { items, buf, state: Rc::new(Cell::new(NEW)) });
        draws
    }

    /// After submit: this frame's copy starts mapping.
    pub(super) fn swatch_after_submit(&mut self) {
        for f in self.swatch.flights.iter_mut().filter(|f| f.state.get() == NEW) {
            let state = f.state.clone();
            state.set(MAPPING);
            f.buf.map_async(wgpu::MapMode::Read, .., move |r| state.set(if r.is_ok() { READY } else { FAILED }));
        }
    }
}

/// Cell `cell`'s `size` x `size` picture out of the mapped copy.
fn cut(view: &[u8], cell: u32, size: u32) -> Vec<u8> {
    let (x0, row, n) = ((cell * MAX_SIZE * 4) as usize, ROW as usize, (size * 4) as usize);
    let mut out = Vec::with_capacity(n * size as usize);
    for y in 0..size as usize {
        let at = y * row + x0;
        match view.get(at..at + n) {
            Some(s) => out.extend_from_slice(s),
            None => out.resize(out.len() + n, 0),
        }
    }
    out
}
