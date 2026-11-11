//! Game passes on the GPU (feature `game-passes`; the declarations, checks
//! and WGSL are in [`crate::game_pass`]). A child of `frame` so it can read
//! the frame's graph targets.
//!
//! * Game targets are allocated here (two textures for a history target,
//!   swapped every frame); engine targets (`hdr`, `ldr`, `depth`...) come
//!   from the frame's target list.
//! * Each pass has its own bind group layout (its inputs decide it), a
//!   pipeline built on the first frame inside a GPU error scope, an engine
//!   uniform, a params uniform and a `data` storage buffer. Bind groups are
//!   rebuilt when a target is re-created (one per history parity).
//! * A full-screen pass that reads its own output reads a copy taken just
//!   before its draw. A failed pipeline is reported once in the ErrorLog
//!   and the pass is skipped from then on.

use std::cell::Cell;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::params::ParamRegistry;
use evani_core::tiers::Tiers;

use super::{Formats, Target, Timer, pipelines};
use crate::game_pass::{self as gp, Format, GamePasses, InputKind, Kind, OutputKind, PassBlend, Run, Source};

/// A game target's textures (two for history).
struct GTex {
    name: &'static str,
    size: [u32; 3],
    tex: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
}

enum Pipe {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}

/// One pass on the GPU.
struct GPass {
    name: &'static str,
    /// The target it draws into or writes.
    out_name: &'static str,
    blend: PassBlend,
    layout: wgpu::BindGroupLayout,
    pipe: Option<Pipe>,
    failed: Rc<Cell<bool>>,
    reported: bool,
    engine_buf: wgpu::Buffer,
    param_buf: wgpu::Buffer,
    data_buf: wgpu::Buffer,
    data_cap: usize,
    data_gen: u64,
    /// Bind groups for history parity 0 and 1.
    bgs: [Option<wgpu::BindGroup>; 2],
    /// The copy of its own output it reads (full-screen passes that do).
    copy: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2])>,
    last_params: [f32; gp::MAX_PARAM_FLOATS],
    /// Run::Once bookkeeping: the key and parameters of its last run, and
    /// this frame's key (it runs again when either differs).
    once_key: Option<(u64, u64, [u32; 3])>,
    once_params: [f32; gp::MAX_PARAM_FLOATS],
    pending_key: (u64, u64, [u32; 3]),
    want_run: bool,
}

/// Every game pass and target on the GPU (a field of `Gpu`).
pub(super) struct GameGpu {
    samp: wgpu::Sampler,
    samp_wrap: wgpu::Sampler,
    textures: Vec<GTex>,
    passes: Vec<GPass>,
    parity: usize,
    built: bool,
    /// Targets changed: rebuild the bind groups.
    stale: bool,
    tier: String,
    words: Box<[f32; gp::ENGINE_WORDS]>,
    /// Bumped whenever a game texture is re-created (Run::Once passes rerun).
    tex_gen: u64,
}

impl GameGpu {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let sampler = |label, mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: mode,
                address_mode_v: mode,
                address_mode_w: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        };
        Self {
            samp: sampler("evani.game.samp", wgpu::AddressMode::ClampToEdge),
            samp_wrap: sampler("evani.game.samp_wrap", wgpu::AddressMode::Repeat),
            textures: Vec::new(),
            passes: Vec::new(),
            parity: 0,
            built: false,
            stale: true,
            tier: String::new(),
            words: Box::new([0.0; gp::ENGINE_WORDS]),
            tex_gen: 0,
        }
    }

    /// The engine targets were re-created (resize): rebuild bind groups.
    pub(super) fn invalidate(&mut self) {
        self.stale = true;
    }

    /// Take this frame's camera, light and time (before the render world
    /// is cleared).
    pub(super) fn capture(&mut self, world: &World, rw: &crate::world::RenderWorld, p: &crate::RenderParams, size: [u32; 2]) {
        if world.try_res::<GamePasses>().is_none_or(|g| g.passes().is_empty()) {
            return;
        }
        let reg = world.res::<ParamRegistry>();
        let lighting = p.lighting(&reg);
        let aspect = size[0].max(1) as f32 / size[1].max(1) as f32;
        let time = world.res::<evani_core::time::Time>();
        let tier = world.try_res::<Tiers>().map_or(0, |t| t.active_index());
        let f = gp::FrameInputs {
            view_proj: crate::shaders::view_proj(&rw.camera, aspect),
            eye: rw.camera.eye,
            seconds: (time.now_ms / 1000.0) as f32,
            light_dir: lighting.light_dir,
            light_intensity: lighting.intensity,
            light_color: lighting.light_color,
            tier: tier as u32,
            frame: time.frame,
            dt: time.frame_dt,
            parity: (self.parity ^ 1) as u32,
            ambient: lighting.ambient,
            background: lighting.fog_color,
        };
        gp::engine_words(&f, &mut self.words);
    }

    /// Before the graph walk: flip history, (re)create textures, build the
    /// pipelines on the first frame, upload uniforms and data, rebuild bind
    /// groups when targets changed.
    pub(super) fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, world: &World, targets: &[Target], screen: [u32; 2], formats: Formats) {
        let Some(gps) = world.try_res::<GamePasses>() else {
            return;
        };
        if gps.passes().is_empty() {
            return;
        }
        self.parity ^= 1;
        let tier = world.try_res::<Tiers>().map(|t| t.active().id.clone()).unwrap_or_default();
        // Textures.
        for t in gps.targets() {
            let size = t.extent(screen, &tier);
            let count = if t.history { 2 } else { 1 };
            let have = self.textures.iter().position(|g| g.name == t.name);
            if have.is_some_and(|i| self.textures[i].size == size && self.textures[i].tex.len() == count) {
                continue;
            }
            let tex: Vec<wgpu::Texture> = (0..count).map(|_| make_texture(device, t.name, t.format, size, t.is_volume())).collect();
            let views = tex.iter().map(|x| x.create_view(&wgpu::TextureViewDescriptor::default())).collect();
            let g = GTex { name: t.name, size, tex, views };
            match have {
                Some(i) => self.textures[i] = g,
                None => self.textures.push(g),
            }
            self.stale = true;
            self.tex_gen += 1;
        }
        self.tier = tier;
        // Pipelines (once; the declarations don't change after build).
        if !self.built {
            self.built = true;
            for r in gps.passes() {
                self.passes.push(build_pass(device, r, formats));
            }
        }
        // Uniforms, data, run decisions.
        let reg = world.res::<ParamRegistry>();
        let mut log = None::<String>;
        for (i, (g, r)) in self.passes.iter_mut().zip(gps.passes()).enumerate() {
            if g.failed.get() && !g.reported {
                g.reported = true;
                g.pipe = None;
                log = Some(format!(
                    "game pass {}: its WGSL failed to build on the GPU, so it is skipped. fix: run the kit's game-pass test (evani_render::game_pass::check) to see the shader error",
                    g.name
                ));
            }
            let mut words = [0.0f32; gp::MAX_PARAM_FLOATS];
            r.param_words(&reg, &mut words);
            let params_changed = words != g.last_params;
            if params_changed {
                g.last_params = words;
                queue.write_buffer(&g.param_buf, 0, bytemuck::cast_slice(&words));
            }
            let out_size = output_size(&self.textures, targets, r.output.0);
            let mut ew = *self.words;
            gp::set_output_size(&mut ew, out_size);
            queue.write_buffer(&g.engine_buf, 0, bytemuck::cast_slice(&ew[..]));
            let dgen = gps.data_generation(i);
            if dgen != g.data_gen {
                g.data_gen = dgen;
                let d = gps.data(r.pass.name);
                let d = &d[..d.len().min(gp::MAX_DATA)];
                if d.len() > g.data_cap {
                    g.data_cap = d.len().next_power_of_two();
                    g.data_buf = data_buffer(device, g.data_cap);
                    g.bgs = [None, None];
                    self.stale = true;
                }
                if !d.is_empty() {
                    queue.write_buffer(&g.data_buf, 0, bytemuck::cast_slice(d));
                }
            }
            g.pending_key = (gps.rerun_generation(i), self.tex_gen, out_size);
            g.want_run = match r.pass.run {
                Run::EveryFrame => true,
                // Marked done only when it really ran (`run`).
                Run::Once => g.once_key != Some(g.pending_key) || g.once_params != words,
            };
        }
        if let Some(msg) = log {
            world.res_mut::<ErrorLog>().error(msg);
        }
        if self.stale {
            self.stale = false;
            for (g, r) in self.passes.iter_mut().zip(gps.passes()) {
                g.copy = None;
                if r.copies_output
                    && let Some(t) = targets.iter().find(|t| t.name == r.output.0)
                {
                    let fmt = match r.output.1 {
                        OutputKind::Engine(k) => pipelines::target_format(k, formats),
                        OutputKind::Game { format, .. } => game_format(format),
                    };
                    let tex = make_copy(device, t.size, fmt);
                    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                    g.copy = Some((tex, view, t.size));
                } else if r.copies_output
                    && let Some(t) = self.textures.iter().find(|t| t.name == r.output.0)
                    && let OutputKind::Game { format, .. } = r.output.1
                {
                    let size = [t.size[0], t.size[1]];
                    let tex = make_copy(device, size, game_format(format));
                    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                    g.copy = Some((tex, view, size));
                }
                for parity in 0..2 {
                    g.bgs[parity] = bind_group(device, g, r, targets, &self.textures, parity, &self.samp, &self.samp_wrap);
                }
            }
        }
    }

    /// Run game pass `name` if it is one. Returns whether it was.
    pub(super) fn run(&mut self, name: &str, enc: &mut wgpu::CommandEncoder, targets: &[Target], timer: &mut Timer, draws: &mut u32) -> bool {
        let Some(i) = self.passes.iter().position(|g| g.name == name) else {
            return false;
        };
        let parity = self.parity;
        let textures = &self.textures;
        let g = &mut self.passes[i];
        if !g.want_run || g.failed.get() {
            return true;
        }
        let (Some(pipe), Some(bg)) = (g.pipe.as_ref(), g.bgs[parity].as_ref()) else {
            return true;
        };
        match pipe {
            Pipe::Compute(cp) => {
                let Some(size) = textures.iter().find(|t| g.layout_out(t)).map(|t| t.size) else {
                    return true;
                };
                let (wx, wy, wz) = if size[2] > 1 { (4, 4, 4) } else { (8, 8, 1) };
                let ts = timer.compute_pass(g.name);
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(g.name), timestamp_writes: ts });
                pass.set_pipeline(cp);
                pass.set_bind_group(0, bg, &[]);
                pass.dispatch_workgroups(size[0].div_ceil(wx), size[1].div_ceil(wy), size[2].div_ceil(wz));
                *draws += 1;
            }
            Pipe::Render(rp) => {
                let out = g.out_name;
                let (view, src_tex, engine) = match targets.iter().find(|t| t.name == out) {
                    Some(t) => (&t.view, Some(&t.tex), true),
                    None => match textures.iter().find(|t| t.name == out) {
                        Some(t) => {
                            let cur = if t.views.len() > 1 { parity } else { 0 };
                            let Some(v) = t.views.get(cur) else {
                                return true;
                            };
                            (v, t.tex.get(cur), false)
                        }
                        None => return true,
                    },
                };
                if let (Some((copy, _, size)), Some(src)) = (&g.copy, src_tex) {
                    enc.copy_texture_to_texture(src.as_image_copy(), copy.as_image_copy(), wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 });
                }
                let load = if engine || g.blend != PassBlend::Replace { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) };
                let ts = timer.pass(g.name);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(g.name),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: ts,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(rp);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
                *draws += 1;
            }
        }
        g.once_key = Some(g.pending_key);
        g.once_params = g.last_params;
        true
    }
}

impl GPass {
    fn layout_out(&self, t: &GTex) -> bool {
        t.name == self.out_name
    }
}

fn game_format(f: Format) -> wgpu::TextureFormat {
    match f {
        Format::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        Format::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        Format::R32Float => wgpu::TextureFormat::R32Float,
        Format::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
    }
}

fn make_texture(device: &wgpu::Device, label: &'static str, format: Format, size: [u32; 3], volume: bool) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
    if !volume {
        usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: size[2] },
        mip_level_count: 1,
        sample_count: 1,
        dimension: if volume { wgpu::TextureDimension::D3 } else { wgpu::TextureDimension::D2 },
        format: game_format(format),
        usage,
        view_formats: &[],
    })
}

fn make_copy(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("evani.game.copy"),
        size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn data_buffer(device: &wgpu::Device, records: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("evani.game.data"),
        size: (records.max(gp::MIN_DATA) * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn uniform(device: &wgpu::Device, label: &'static str, words: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (words * 4) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Output size of a pass: a game texture's, or an engine target's.
fn output_size(textures: &[GTex], targets: &[Target], name: &str) -> [u32; 3] {
    if let Some(t) = textures.iter().find(|t| t.name == name) {
        return t.size;
    }
    targets.iter().find(|t| t.name == name).map_or([1, 1, 1], |t| [t.size[0], t.size[1], 1])
}

fn blend_state(b: PassBlend) -> Option<wgpu::BlendState> {
    use wgpu::BlendFactor as F;
    let c = |src, dst| wgpu::BlendComponent { src_factor: src, dst_factor: dst, operation: wgpu::BlendOperation::Add };
    match b {
        PassBlend::Replace => None,
        PassBlend::Additive => Some(wgpu::BlendState { color: c(F::One, F::One), alpha: c(F::One, F::One) }),
        PassBlend::Multiply => Some(wgpu::BlendState { color: c(F::Dst, F::Zero), alpha: c(F::DstAlpha, F::Zero) }),
        PassBlend::Alpha => Some(wgpu::BlendState { color: c(F::SrcAlpha, F::OneMinusSrcAlpha), alpha: c(F::One, F::OneMinusSrcAlpha) }),
    }
}

/// The pass's layout, uniforms and pipeline (built inside an error scope).
fn build_pass(device: &wgpu::Device, r: &gp::Registered, formats: Formats) -> GPass {
    use gp::binding as b;
    let compute = r.pass.kind == Kind::Compute;
    let vis = if compute { wgpu::ShaderStages::COMPUTE } else { wgpu::ShaderStages::FRAGMENT };
    let buf = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: vis, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
    let samp = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: vis, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
    let mut entries = vec![
        buf(b::FRAME, wgpu::BufferBindingType::Uniform),
        buf(b::PARAMS, wgpu::BufferBindingType::Uniform),
        samp(b::SAMP),
        samp(b::SAMP_WRAP),
        buf(b::DATA, wgpu::BufferBindingType::Storage { read_only: true }),
    ];
    for (i, k) in r.inputs.iter().enumerate() {
        let (sample_type, view_dimension) = match *k {
            InputKind::Color => (wgpu::TextureSampleType::Float { filterable: true }, wgpu::TextureViewDimension::D2),
            InputKind::ColorUnfilterable => (wgpu::TextureSampleType::Float { filterable: false }, wgpu::TextureViewDimension::D2),
            InputKind::Depth => (wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2),
            InputKind::Volume { filterable } => (wgpu::TextureSampleType::Float { filterable }, wgpu::TextureViewDimension::D3),
        };
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: b::INPUT0 + i as u32,
            visibility: vis,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension, multisampled: false },
            count: None,
        });
    }
    if let (true, OutputKind::Game { format, volume }) = (compute, r.output.1) {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: b::OUT,
            visibility: vis,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: game_format(format),
                view_dimension: if volume { wgpu::TextureViewDimension::D3 } else { wgpu::TextureViewDimension::D2 },
            },
            count: None,
        });
    }
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(r.pass.name), entries: &entries });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(r.pass.name), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let source = gp::compose_wgsl(r);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(r.pass.name), source: wgpu::ShaderSource::Wgsl(source.as_str().into()) });
    let pipe = if compute {
        Pipe::Compute(device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(r.pass.name),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("cs_main"),
            compilation_options: Default::default(),
            cache: None,
        }))
    } else {
        let format = match r.output.1 {
            OutputKind::Engine(k) => pipelines::target_format(k, formats),
            OutputKind::Game { format, .. } => game_format(format),
        };
        // Only filterable formats blend.
        let blend = if matches!(r.output.1, OutputKind::Game { format: Format::R32Float | Format::Rgba32Float, .. }) { None } else { blend_state(r.pass.blend) };
        Pipe::Render(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(r.pass.name),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        }))
    };
    let err = scope.pop();
    let failed = Rc::new(Cell::new(false));
    let flag = failed.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if err.await.is_some() {
            flag.set(true);
        }
    });
    GPass {
        name: r.pass.name,
        out_name: r.output.0,
        blend: r.pass.blend,
        layout,
        pipe: Some(pipe),
        failed,
        reported: false,
        engine_buf: uniform(device, "evani.game.frame", gp::ENGINE_WORDS),
        param_buf: uniform(device, "evani.game.params", gp::MAX_PARAM_FLOATS),
        data_buf: data_buffer(device, gp::MIN_DATA),
        data_cap: gp::MIN_DATA,
        data_gen: u64::MAX,
        bgs: [None, None],
        copy: None,
        last_params: [f32::NAN; gp::MAX_PARAM_FLOATS],
        once_key: None,
        once_params: [f32::NAN; gp::MAX_PARAM_FLOATS],
        pending_key: (0, 0, [0; 3]),
        want_run: false,
    }
}

/// The view an input reads at history parity `parity`.
fn input_view<'a>(src: Source, own_copy: Option<&'a wgpu::TextureView>, out: &str, targets: &'a [Target], textures: &'a [GTex], parity: usize) -> Option<&'a wgpu::TextureView> {
    let cur = |t: &'a GTex| t.views.get(if t.views.len() > 1 { parity } else { 0 });
    match src {
        Source::Target(name) if name == out && own_copy.is_some() => own_copy,
        Source::Target(name) => match textures.iter().find(|t| t.name == name) {
            Some(t) => cur(t),
            None => targets.iter().find(|t| t.name == name).map(|t| &t.view),
        },
        Source::Depth => targets.iter().find(|t| t.name == "depth").map(|t| &t.view),
        Source::History(name) => textures.iter().find(|t| t.name == name).and_then(|t| t.views.get(if t.views.len() > 1 { parity ^ 1 } else { 0 })),
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_group(device: &wgpu::Device, g: &GPass, r: &gp::Registered, targets: &[Target], textures: &[GTex], parity: usize, samp: &wgpu::Sampler, samp_wrap: &wgpu::Sampler) -> Option<wgpu::BindGroup> {
    use gp::binding as b;
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: b::FRAME, resource: g.engine_buf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: b::PARAMS, resource: g.param_buf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: b::SAMP, resource: wgpu::BindingResource::Sampler(samp) },
        wgpu::BindGroupEntry { binding: b::SAMP_WRAP, resource: wgpu::BindingResource::Sampler(samp_wrap) },
        wgpu::BindGroupEntry { binding: b::DATA, resource: g.data_buf.as_entire_binding() },
    ];
    let copy = g.copy.as_ref().map(|(_, v, _)| v);
    for (i, inp) in r.pass.inputs.iter().enumerate() {
        let view = input_view(inp.source, copy, r.output.0, targets, textures, parity)?;
        entries.push(wgpu::BindGroupEntry { binding: b::INPUT0 + i as u32, resource: wgpu::BindingResource::TextureView(view) });
    }
    if r.pass.kind == Kind::Compute {
        let t = textures.iter().find(|t| t.name == r.output.0)?;
        let view = t.views.get(if t.views.len() > 1 { parity } else { 0 })?;
        entries.push(wgpu::BindGroupEntry { binding: b::OUT, resource: wgpu::BindingResource::TextureView(view) });
    }
    Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(g.name), layout: &g.layout, entries: &entries }))
}
