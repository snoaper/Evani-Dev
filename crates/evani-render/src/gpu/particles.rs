//! GPU side of the shared particle system (feature `particles`; the CPU
//! side and the formula are in `crate::particles`).
//!
//! * A spawn-record ring (`STORAGE | COPY_DST`) and an instance buffer
//!   (`STORAGE | VERTEX`), both sized once from the cap (remade only when
//!   the tier's cap changes). New records go in with one or two
//!   `write_buffer` calls a frame.
//! * `particles.sim`: one compute dispatch (workgroups of 64) evaluates
//!   every slot into the instance buffer.
//! * The draws read the instance buffer as a per-instance vertex buffer (no
//!   vertex-stage storage, so it works within every WebGPU limit), with the
//!   sprite atlas and the uniform ring of layout A: one pipeline for the
//!   screen groups (sprites pass) and one per sample count for the world
//!   group (forward pass, 1x or 4x to match MSAA).
//!
//! Pipelines are built on first use, so a game that never emits pays
//! nothing but the shader text.

use crate::particles::{DRAW_WGSL, Instance, ParticleSystem, Record, SIM_WGSL, SimUniform, WORKGROUP};

use super::pipelines::Layouts;
#[cfg(feature = "forward")]
use super::pipelines::MSAA_SAMPLES;

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x4,
    1 => Float32x3,
    2 => Uint32,
    3 => Float32x4,
    4 => Float32x4,
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
    screen: wgpu::RenderPipeline,
    /// World group: 1x and 4x (MSAA forward pass); 3D path only.
    #[cfg(feature = "forward")]
    world: [wgpu::RenderPipeline; 2],
}

struct Buffers {
    generation: u32,
    records: wgpu::Buffer,
    instances: wgpu::Buffer,
    bg: wgpu::BindGroup,
}

/// Owned by `Gpu`; `prepare` once a frame before the graph walk.
#[derive(Default)]
pub(super) struct GpuParticles {
    pipes: Option<Pipes>,
    sim: Option<wgpu::Buffer>,
    bufs: Option<Buffers>,
    /// Run the compute pass and the draws this frame.
    pub run: bool,
    count: u32,
    /// Screen groups this frame: sprites drawn before the group (the
    /// sprite list's split index) and the group's uniform offset.
    pub screen: Vec<(u32, u32)>,
    /// World group: one uniform offset per view (empty: none live); 3D
    /// path only.
    #[cfg(feature = "forward")]
    pub world: Vec<u32>,
    /// Live particles this frame (the stats).
    pub live: u32,
}

impl GpuParticles {
    /// Upload this frame's records and the sim uniform; decide whether the
    /// pass and the draws run. Called every frame the system ran, even when
    /// the frame draws nothing, so no record is lost.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &Layouts,
        sys: &mut ParticleSystem,
        uv_of: impl Fn(u16) -> [f32; 4],
    ) {
        self.run = false;
        self.live = 0;
        self.screen.clear();
        #[cfg(feature = "forward")]
        self.world.clear();
        if !sys.is_on() || sys.cap() == 0 {
            return;
        }
        let pipes = self.pipes.get_or_insert_with(|| build_pipes(device, layouts));
        let sim = self.sim.get_or_insert_with(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("evani.particles.sim"),
                size: size_of::<SimUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        if self.bufs.as_ref().is_none_or(|b| b.generation != sys.generation()) {
            // Drop the old ring first so the peak is one ring, not two.
            self.bufs = None;
            let cap = u64::from(sys.cap());
            let make = |label, size, usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage,
                    mapped_at_creation: false,
                })
            };
            let records = make(
                "evani.particles.records",
                cap * size_of::<Record>() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            let instances = make(
                "evani.particles.instances",
                cap * size_of::<Instance>() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            );
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("evani.particles"),
                layout: &pipes.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: records.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: instances.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: sim.as_entire_binding() },
                ],
            });
            self.bufs = Some(Buffers {
                generation: sys.generation(),
                records,
                instances,
                bg,
            });
        }
        let Some(bufs) = &self.bufs else {
            return;
        };
        for r in sys.staged_mut() {
            r.uv = uv_of(r.cell());
        }
        for (slot, recs) in sys.uploads() {
            if !recs.is_empty() {
                let at = u64::from(slot) * size_of::<Record>() as u64;
                queue.write_buffer(&bufs.records, at, bytemuck::cast_slice(recs));
            }
        }
        if sys.live() == 0 {
            return;
        }
        let u = sys.sim_uniform();
        queue.write_buffer(sim, 0, bytemuck::bytes_of(&u));
        self.count = u.count;
        self.live = sys.live();
        self.run = u.count > 0;
    }

    /// The `particles.sim` compute pass.
    pub fn dispatch(&self, enc: &mut wgpu::CommandEncoder, ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let (true, Some(p), Some(b)) = (self.run, &self.pipes, &self.bufs) else {
            return;
        };
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("particles.sim"),
            timestamp_writes: ts,
        });
        cp.set_pipeline(&p.compute);
        cp.set_bind_group(0, &b.bg, &[]);
        cp.dispatch_workgroups(self.count.div_ceil(WORKGROUP), 1, 1);
    }

    /// One screen group inside the sprites pass. Returns the draws made.
    pub fn draw_screen(&self, rp: &mut wgpu::RenderPass<'_>, atlas_bg: &wgpu::BindGroup, off: u32) -> u32 {
        let (true, Some(p), Some(b)) = (self.run, &self.pipes, &self.bufs) else {
            return 0;
        };
        rp.set_pipeline(&p.screen);
        rp.set_bind_group(0, atlas_bg, &[off]);
        rp.set_vertex_buffer(0, b.instances.slice(..));
        rp.draw(0..6, 0..self.count);
        1
    }

    /// The world group for one view inside the forward pass (`msaa`: the
    /// pass draws 4x). Returns the draws made.
    #[cfg(feature = "forward")]
    pub fn draw_world(&self, rp: &mut wgpu::RenderPass<'_>, atlas_bg: &wgpu::BindGroup, off: u32, msaa: bool) -> u32 {
        let (true, Some(p), Some(b)) = (self.run, &self.pipes, &self.bufs) else {
            return 0;
        };
        rp.set_pipeline(&p.world[usize::from(msaa)]);
        rp.set_bind_group(0, atlas_bg, &[off]);
        rp.set_vertex_buffer(0, b.instances.slice(..));
        rp.draw(0..6, 0..self.count);
        1
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
        label: Some("evani.layout.particles"),
        entries: &[
            entry(0, storage(true)),
            entry(1, storage(false)),
            entry(
                2,
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
    let sim_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.particles_sim"),
        source: wgpu::ShaderSource::Wgsl(SIM_WGSL.into()),
    });
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("evani.particles_sim"),
        layout: Some(&pl),
        module: &sim_module,
        entry_point: Some("cs_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let draw_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.particles_draw"),
        source: wgpu::ShaderSource::Wgsl(DRAW_WGSL.into()),
    });
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: size_of::<Instance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE_ATTRS,
    })];
    let draw = |vs: &str, depth: bool, samples: u32| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("evani.particles_draw"),
            layout: Some(&layouts.pl_a),
            vertex: wgpu::VertexState {
                module: &draw_module,
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
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &draw_module,
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
        screen: draw("vs_screen", false, 1),
        #[cfg(feature = "forward")]
        world: [draw("vs_world", true, 1), draw("vs_world", true, MSAA_SAMPLES)],
        layout,
        compute,
    }
}
