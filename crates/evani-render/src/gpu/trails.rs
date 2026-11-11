//! GPU side of ribbons and trails (feature `trails`; the CPU side and the
//! formula are in `crate::trails`).
//!
//! * The point rings (`STORAGE | COPY_DST`), the per-emitter data and the
//!   aged points (`STORAGE | VERTEX`), made once for [`MAX_TRAILS`] emitters.
//! * `trails.sim`: one workgroup of 64 per emitter ages its ring.
//! * The draws bind the aged points twice, one point apart, so an instance
//!   sees a point and its neighbour with no vertex-stage storage: one pipeline
//!   for screen groups (sprites pass), one per sample count for the world
//!   group (forward pass).
//!
//! Pipelines are built on first use.

use crate::trails::{DRAW_WGSL, Head, MAX_TRAILS, POINTS, Point, SIM_WGSL, SimUniform, TrailSystem, Vert};
#[cfg(any(not(feature = "forward"), feature = "trails-screen"))]
use crate::trails::SCREEN_WGSL;

use super::pipelines::Layouts;
#[cfg(feature = "forward")]
use super::pipelines::MSAA_SAMPLES;

// This point: the layout of `Vert`, spelt out because of its gaps.
const POINT_ATTRS: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 32, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 44, shader_location: 3 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 4 },
];
// The neighbour: the same layout without the key.
const NEXT_ATTRS: [wgpu::VertexAttribute; 4] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 5 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 6 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 32, shader_location: 7 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 8 },
];

const PREMULTIPLIED: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

struct Pipes {
    layout: wgpu::BindGroupLayout,
    compute: wgpu::ComputePipeline,
    #[cfg(any(not(feature = "forward"), feature = "trails-screen"))]
    screen: wgpu::RenderPipeline,
    /// World group: 1x and 4x (MSAA forward pass); 3D path only.
    #[cfg(feature = "forward")]
    world: [wgpu::RenderPipeline; 2],
}

struct Buffers {
    points: wgpu::Buffer,
    heads: wgpu::Buffer,
    verts: wgpu::Buffer,
    bg: wgpu::BindGroup,
}

/// Owned by `Gpu`; `prepare` once a frame before the graph walk.
#[derive(Default)]
pub(super) struct GpuTrails {
    pipes: Option<Pipes>,
    sim: Option<wgpu::Buffer>,
    bufs: Option<Buffers>,
    /// Run the compute pass and the draws this frame.
    pub run: bool,
    trails: u32,
    steps: u32,
    /// Screen groups this frame: sprites drawn before the group and its
    /// uniform offset, by layer.
    #[cfg(any(not(feature = "forward"), feature = "trails-screen"))]
    pub screen: Vec<(u32, u32)>,
    /// World group: one uniform offset per view (empty: none live).
    #[cfg(feature = "forward")]
    pub world: Vec<u32>,
}

impl GpuTrails {
    /// Upload this frame's points and the per-emitter data; decide whether
    /// the pass and the draws run. Called every frame the system ran, so no
    /// point is lost.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layouts: &Layouts, sys: &mut TrailSystem) {
        self.run = false;
        #[cfg(any(not(feature = "forward"), feature = "trails-screen"))]
        self.screen.clear();
        #[cfg(feature = "forward")]
        self.world.clear();
        if !sys.is_on() || sys.cap() == 0 {
            return;
        }
        let pipes = self.pipes.get_or_insert_with(|| build_pipes(device, layouts));
        let sim = self.sim.get_or_insert_with(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: size_of::<SimUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        if self.bufs.is_none() {
            let cap = u64::from(MAX_TRAILS);
            let n = cap * u64::from(POINTS);
            let make = |size, usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage,
                    mapped_at_creation: false,
                })
            };
            let store = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
            let points = make(n * size_of::<Point>() as u64, store);
            let heads = make(cap * size_of::<Head>() as u64, store);
            let verts = make(
                n * size_of::<Vert>() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            );
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipes.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 4, resource: points.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: heads.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: verts.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: sim.as_entire_binding() },
                ],
            });
            self.bufs = Some(Buffers { points, heads, verts, bg });
            // New rings start empty: no strip may join points they never had.
            sys.clear();
        }
        let Some(bufs) = &self.bufs else {
            return;
        };
        for (at, p) in sys.staged() {
            queue.write_buffer(&bufs.points, u64::from(*at) * size_of::<Point>() as u64, bytemuck::bytes_of(p));
        }
        sys.mark_uploaded();
        if sys.live() == 0 {
            return;
        }
        queue.write_buffer(&bufs.heads, 0, bytemuck::cast_slice(sys.heads()));
        let u = sys.sim_uniform();
        queue.write_buffer(sim, 0, bytemuck::bytes_of(&u));
        self.trails = u.trails;
        self.steps = sys.steps().max(1);
        self.run = u.trails > 0;
    }

    /// The `trails.sim` compute pass.
    pub fn dispatch(&self, enc: &mut wgpu::CommandEncoder, ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let (true, Some(p), Some(b)) = (self.run, &self.pipes, &self.bufs) else {
            return;
        };
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: ts,
        });
        cp.set_pipeline(&p.compute);
        cp.set_bind_group(0, &b.bg, &[]);
        cp.dispatch_workgroups(self.trails, 1, 1);
    }

    fn draw(&self, rp: &mut wgpu::RenderPass<'_>, pipe: &wgpu::RenderPipeline, bufs: &Buffers, atlas_bg: &wgpu::BindGroup, off: u32) -> u32 {
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, atlas_bg, &[off]);
        rp.set_vertex_buffer(0, bufs.verts.slice(..));
        rp.set_vertex_buffer(1, bufs.verts.slice(size_of::<Vert>() as u64..));
        // The last slot has no neighbour; it never starts a segment.
        rp.draw(0..6 * self.steps, 0..(self.trails * POINTS).saturating_sub(1));
        1
    }

    /// Triangles one draw of a group covers (dead segments included).
    pub fn tris(&self) -> u32 {
        if self.run { 2 * self.steps * (self.trails * POINTS).saturating_sub(1) } else { 0 }
    }

    /// One screen group inside the sprites pass. Returns the draws made.
    #[cfg(any(not(feature = "forward"), feature = "trails-screen"))]
    pub fn draw_screen(&self, rp: &mut wgpu::RenderPass<'_>, atlas_bg: &wgpu::BindGroup, off: u32) -> u32 {
        let (true, Some(p), Some(b)) = (self.run, &self.pipes, &self.bufs) else {
            return 0;
        };
        self.draw(rp, &p.screen, b, atlas_bg, off)
    }

    /// The world group for one view inside the forward pass (`msaa`: the
    /// pass draws 4x). Returns the draws made.
    #[cfg(feature = "forward")]
    pub fn draw_world(&self, rp: &mut wgpu::RenderPass<'_>, atlas_bg: &wgpu::BindGroup, off: u32, msaa: bool) -> u32 {
        let (true, Some(p), Some(b)) = (self.run, &self.pipes, &self.bufs) else {
            return 0;
        };
        self.draw(rp, &p.world[usize::from(msaa)], b, atlas_bg, off)
    }
}

fn build_pipes(device: &wgpu::Device, layouts: &Layouts) -> Pipes {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    };
    let storage = |read_only| wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            entry(4, storage(true)),
            entry(5, storage(true)),
            entry(6, storage(false)),
            entry(
                7,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    // One module for the compute pass and the draws.
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl([SIM_WGSL, DRAW_WGSL, #[cfg(any(not(feature = "forward"), feature = "trails-screen"))] SCREEN_WGSL].concat().into()),
    });
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pl),
        module: &module,
        entry_point: Some("cs_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let stride = size_of::<Vert>() as u64;
    let buffers = [
        Some(wgpu::VertexBufferLayout { array_stride: stride, step_mode: wgpu::VertexStepMode::Instance, attributes: &POINT_ATTRS }),
        Some(wgpu::VertexBufferLayout { array_stride: stride, step_mode: wgpu::VertexStepMode::Instance, attributes: &NEXT_ATTRS }),
    ];
    let draw = |vs: &str, depth: bool, samples: u32| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layouts.pl_a),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(vs),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth.then(|| wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(crate::depth::COMPARE),
                stencil: wgpu::StencilState::default(),
                // A ribbon lying on the ground (a tyre mark) seen at a low angle:
                // reversed Z, so a positive bias pulls it towards the eye, most
                // where the view grazes the surface.
                bias: wgpu::DepthBiasState { constant: 4, slope_scale: 4.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(PREMULTIPLIED),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    Pipes {
        #[cfg(any(not(feature = "forward"), feature = "trails-screen"))]
        screen: draw("vs_screen", false, 1),
        #[cfg(feature = "forward")]
        world: [draw("vs_world", true, 1), draw("vs_world", true, MSAA_SAMPLES)],
        layout,
        compute,
    }
}
