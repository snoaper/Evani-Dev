//! The grass on the GPU (browser builds): one compute pass (`grass`: the
//! trample window, blade generation and culling, the count clamp) and, in
//! the forward pass, the ground carpets and one indirect draw per LOD
//! bucket per view. Pipelines are built on first use and again when the
//! sample count or the game's fog slot changes; buffers grow as needed.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_render::world_draw::{WorldDraw, WorldGpu};

use crate::plan::{BLADE_BYTES, SIM_WORDS, VIEW_WORDS};
use crate::Shared;

/// Bytes between draw uniforms (the dynamic-offset alignment).
const SLOT: u64 = 256;
/// Most views drawn (split screen).
const MAX_VIEWS: usize = 4;

const ATTRS: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 3 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32x4, offset: 64, shader_location: 4 },
];

struct Compute {
    layout: wgpu::BindGroupLayout,
    trample: wgpu::ComputePipeline,
    blades: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
}

struct Bufs {
    sim: wgpu::Buffer,
    views: wgpu::Buffer,
    jobs: wgpu::Buffer,
    jobs_cap: usize,
    blades: wgpu::Buffer,
    blades_cap: u64,
    args: wgpu::Buffer,
    /// Kept alive for the bind group.
    _trample: wgpu::Buffer,
    trample_n: u32,
    /// The patches' height grids (binding 5), its capacity in words and the
    /// revision it holds.
    ground: wgpu::Buffer,
    ground_cap: usize,
    ground_rev: u64,
    cs_bg: wgpu::BindGroup,
    draw_bg: wgpu::BindGroup,
}

struct Draw {
    samples: u32,
    fog_rev: u64,
    blade: wgpu::RenderPipeline,
    ground: wgpu::RenderPipeline,
}

struct State {
    compute: Compute,
    draw_layout: wgpu::BindGroupLayout,
    bufs: Option<Bufs>,
    draw: Option<Draw>,
    /// This frame's compute pass ran: draw.
    ran: bool,
    views: usize,
    patches: u32,
    ground: bool,
    caps: [u32; 3],
    bases: [u32; 3],
}

/// The grass's [`WorldDraw`].
pub(crate) struct GrassDraw {
    shared: Rc<RefCell<Shared>>,
    st: RefCell<Option<State>>,
    /// A pipeline failed to build: draw nothing more (the message goes to
    /// the error log once, through the module's frame step).
    failed: Rc<Cell<bool>>,
}

impl GrassDraw {
    pub(crate) fn new(shared: Rc<RefCell<Shared>>) -> Self {
        Self { shared, st: RefCell::new(None), failed: Rc::new(Cell::new(false)) }
    }

    /// Watch the validation errors of what `make` builds: on one, stop
    /// drawing and hand the message to the module.
    fn scoped<T>(&self, device: &wgpu::Device, what: &'static str, make: impl FnOnce() -> T) -> T {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let out = make();
        let err = scope.pop();
        let failed = self.failed.clone();
        let shared = self.shared.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(e) = err.await {
                failed.set(true);
                if let Ok(mut sh) = shared.try_borrow_mut() {
                    sh.gpu_error = Some(format!("material.grass: the {what} shader failed to build, grass is off: {e}"));
                }
            }
        });
        out
    }
}

fn buffer(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some("grass"), size: size.max(16), usage, mapped_at_creation: false })
}

fn compute(device: &wgpu::Device) -> Compute {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
    let storage = |read_only| wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("grass.cs"),
        entries: &[
            entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            entry(1, storage(true)),
            entry(2, storage(false)),
            entry(3, storage(false)),
            entry(4, storage(false)),
            entry(5, storage(true)),
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("grass.cs"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("grass.cs"), source: wgpu::ShaderSource::Wgsl(crate::wgsl::cs_module().into()) });
    let make = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("grass.cs"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    Compute { trample: make("cs_trample"), blades: make("cs_blades"), finish: make("cs_finish"), layout }
}

fn draw_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, dynamic| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: None },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("grass.draw"), entries: &[entry(0, true), entry(1, false)] })
}

fn draws(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, fog: &str, samples: u32, fog_rev: u64, color: wgpu::TextureFormat, depth: wgpu::TextureFormat) -> Draw {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("grass.draw"), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
    let src = crate::wgsl::draw_module(fog);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("grass.draw"), source: wgpu::ShaderSource::Wgsl(src.into()) });
    let blade_buf = [Some(wgpu::VertexBufferLayout { array_stride: BLADE_BYTES, step_mode: wgpu::VertexStepMode::Instance, attributes: &ATTRS })];
    let make = |vs: &str, fs: &str, topology: wgpu::PrimitiveTopology, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grass.draw"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
            primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(evani_render::depth::COMPARE),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: color, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    Draw {
        samples,
        fog_rev,
        blade: make("vs_blade", "fs_blade", wgpu::PrimitiveTopology::TriangleStrip, &blade_buf),
        ground: make("vs_ground", "fs_ground", wgpu::PrimitiveTopology::TriangleList, &[]),
    }
}

impl State {
    fn new(device: &wgpu::Device) -> Self {
        Self { compute: compute(device), draw_layout: draw_layout(device), bufs: None, draw: None, ran: false, views: 0, patches: 0, ground: false, caps: [0; 3], bases: [0; 3] }
    }

    /// Buffers big enough for `blades` blades, `jobs` jobs and an `n` x `n`
    /// trample window (a new window starts flat).
    fn ensure(&mut self, device: &wgpu::Device, blades: u64, jobs: usize, n: u32, ground: usize) {
        let fits = self.bufs.as_ref().is_some_and(|b| b.blades_cap >= blades && b.jobs_cap >= jobs && b.trample_n == n && b.ground_cap >= ground);
        if fits {
            return;
        }
        let (old_jobs, old_blades) = self.bufs.as_ref().map_or((0, 0), |b| (b.jobs_cap, b.blades_cap));
        let jobs_cap = if jobs > old_jobs { (jobs + jobs / 2).max(256) } else { old_jobs };
        let blades_cap = blades.max(old_blades);
        let u = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let s = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let sim = buffer(device, (SIM_WORDS * 4) as u64, u);
        let views = buffer(device, SLOT * (MAX_VIEWS * 3) as u64, u);
        let jobs_buf = buffer(device, jobs_cap as u64 * 16, s);
        let blades_buf = buffer(device, blades_cap * BLADE_BYTES, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX);
        let args = buffer(device, 48, s | wgpu::BufferUsages::INDIRECT);
        let trample = buffer(device, u64::from(n.max(1)) * u64::from(n.max(1)) * 8, wgpu::BufferUsages::STORAGE);
        let ground_cap = ground.max(self.bufs.as_ref().map_or(0, |b| b.ground_cap)).max(4);
        let ground_buf = buffer(device, ground_cap as u64 * 4, s);
        let cs_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grass.cs"),
            layout: &self.compute.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sim.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: jobs_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: blades_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: args.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: trample.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: ground_buf.as_entire_binding() },
            ],
        });
        let draw_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grass.draw"),
            layout: &self.draw_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &views, offset: 0, size: wgpu::BufferSize::new((VIEW_WORDS * 4) as u64) }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: sim.as_entire_binding() },
            ],
        });
        self.bufs = Some(Bufs { sim, views, jobs: jobs_buf, jobs_cap, blades: blades_buf, blades_cap, args, _trample: trample, trample_n: n, ground: ground_buf, ground_cap, ground_rev: u64::MAX, cs_bg, draw_bg });
    }
}

impl WorldDraw for GrassDraw {
    fn name(&self) -> &'static str {
        "grass"
    }

    fn prepare(&self, gpu: &WorldGpu<'_>, enc: &mut wgpu::CommandEncoder, ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let sh = self.shared.borrow();
        let mut guard = self.st.borrow_mut();
        if guard.is_none() {
            *guard = Some(self.scoped(gpu.device, "generation", || State::new(gpu.device)));
        }
        let Some(st) = guard.as_mut() else {
            return;
        };
        st.ran = false;
        if self.failed.get() || !sh.on || sh.jobs.is_empty() || sh.cameras.is_empty() {
            return;
        }
        let cfg = sh.cfg;
        let total = u64::from(cfg.caps[0]) + u64::from(cfg.caps[1]) + u64::from(cfg.caps[2]);
        st.ensure(gpu.device, total, sh.jobs.len(), cfg.trample_n, sh.ground.len());
        let Some(b) = st.bufs.as_mut() else {
            return;
        };
        let q = gpu.queue;
        if b.ground_rev != sh.ground_rev {
            if !sh.ground.is_empty() {
                q.write_buffer(&b.ground, 0, bytemuck::cast_slice(&sh.ground));
            }
            b.ground_rev = sh.ground_rev;
        }
        let b = &*b;
        q.write_buffer(&b.sim, 0, bytemuck::cast_slice(&sh.sim.words));
        q.write_buffer(&b.jobs, 0, bytemuck::cast_slice(&sh.jobs));
        let v = |s: u32| 2 * s.max(1) + 1;
        let args: [u32; 12] = [v(cfg.segs[0]), 0, 0, 0, v(cfg.segs[1]), 0, 0, 0, v(cfg.segs[2]), 0, 0, 0];
        q.write_buffer(&b.args, 0, bytemuck::cast_slice(&args));
        let views = gpu.views.len().min(sh.cameras.len()).min(MAX_VIEWS);
        for (i, (_, px)) in gpu.views.iter().take(views).enumerate() {
            let cam = &sh.cameras[i];
            let vp = evani_render::shaders::view_proj(cam, px[2] / px[3].max(1.0));
            #[cfg(feature = "capture")]
            let vp = if sh.jitter != [0.0; 2] { evani_render::shaders::jitter_view_proj(&vp, sh.jitter, [px[2] as u32, px[3] as u32]) } else { vp };
            for bk in 0..3u32 {
                let words = crate::plan::view_words(&vp, cam.eye, cfg.segs[bk as usize], bk, crate::plan::px_scale(px[3], cam.fov_y_deg), &sh.shade);
                q.write_buffer(&b.views, (i as u64 * 3 + u64::from(bk)) * SLOT, bytemuck::cast_slice(&words));
            }
        }
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("grass"), timestamp_writes: ts });
        cp.set_bind_group(0, &b.cs_bg, &[]);
        if cfg.trample_n > 0 {
            let g = cfg.trample_n.div_ceil(8);
            cp.set_pipeline(&st.compute.trample);
            cp.dispatch_workgroups(g, g, 1);
        }
        let jobs = sh.jobs.len() as u32;
        let x = jobs.min(65_535);
        cp.set_pipeline(&st.compute.blades);
        cp.dispatch_workgroups(x, jobs.div_ceil(x), 1);
        cp.set_pipeline(&st.compute.finish);
        cp.dispatch_workgroups(1, 1, 1);
        drop(cp);
        st.ran = true;
        st.views = views;
        st.patches = sh.patches;
        st.ground = sh.shade.ground;
        st.caps = cfg.caps;
        st.bases = [0, cfg.caps[0], cfg.caps[0] + cfg.caps[1]];
    }

    fn draw(&self, gpu: &WorldGpu<'_>, rp: &mut wgpu::RenderPass<'_>) -> u32 {
        let mut guard = self.st.borrow_mut();
        let Some(st) = guard.as_mut() else {
            return 0;
        };
        if !st.ran || self.failed.get() {
            return 0;
        }
        {
            let sh = self.shared.borrow();
            if st.draw.as_ref().is_none_or(|d| d.samples != gpu.samples || d.fog_rev != sh.fog_rev) {
                let layout = &st.draw_layout;
                st.draw = Some(self.scoped(gpu.device, "draw", || draws(gpu.device, layout, &sh.fog_wgsl, gpu.samples, sh.fog_rev, gpu.color, gpu.depth)));
            }
        }
        let (Some(d), Some(b)) = (&st.draw, &st.bufs) else {
            return 0;
        };
        let mut n = 0;
        for (i, (_, px)) in gpu.views.iter().take(st.views).enumerate() {
            rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
            let slot = |bk: u64| ((i as u64 * 3 + bk) * SLOT) as u32;
            if st.ground && st.patches > 0 {
                rp.set_pipeline(&d.ground);
                rp.set_bind_group(0, &b.draw_bg, &[slot(0)]);
                rp.draw(0..6, 0..st.patches);
                n += 1;
            }
            rp.set_pipeline(&d.blade);
            for bk in 0..3usize {
                if st.caps[bk] == 0 {
                    continue;
                }
                let lo = u64::from(st.bases[bk]) * BLADE_BYTES;
                let hi = lo + u64::from(st.caps[bk]) * BLADE_BYTES;
                rp.set_bind_group(0, &b.draw_bg, &[slot(bk as u64)]);
                rp.set_vertex_buffer(0, b.blades.slice(lo..hi));
                rp.draw_indirect(&b.args, bk as u64 * 16);
                n += 1;
            }
        }
        n
    }
}
