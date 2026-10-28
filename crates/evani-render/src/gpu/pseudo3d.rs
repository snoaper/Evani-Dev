//! GPU side of the pseudo-3D road (feature `pseudo3d`; the projector is
//! `crate::pseudo3d`). Once a frame `prepare` projects the kit's
//! [`RoadScene`] into quads and uploads them as one instance stream; the
//! sprites pass then draws them in one instanced call before the frame's
//! sprites, with the sprite pass's own atlas bind group and viewport
//! uniform (layout A). The pipeline is built on the first frame with a
//! visible road, so a build that never shows one pays only the shader text.

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;

use super::pipelines::Layouts;
use crate::pseudo3d::{FILL, RoadOutput, RoadParams, RoadScene, WGSL};

/// One quad as `shaders/pseudo3d.wgsl` reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    top: [f32; 4],
    bottom: [f32; 4],
    uv: [f32; 4],
    color_top: [f32; 4],
    color_bottom: [f32; 4],
    fog: [f32; 4],
}

const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x4,
    1 => Float32x4,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
];

const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Owned by `Gpu`.
#[derive(Default)]
pub(super) struct GpuRoad {
    pipe: Option<wgpu::RenderPipeline>,
    buf: Option<(wgpu::Buffer, u64)>,
    out: RoadOutput,
    staging: Vec<Instance>,
    count: u32,
}

fn pipeline(device: &wgpu::Device, layouts: &Layouts) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.pseudo3d"),
        source: wgpu::ShaderSource::Wgsl(WGSL.into()),
    });
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: size_of::<Instance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.pseudo3d"),
        layout: Some(&layouts.pl_a),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba16Float,
                blend: Some(BLEND),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl GpuRoad {
    /// Project and upload this frame's road (nothing when the kit has no
    /// visible [`RoadScene`]). `uv_of` maps an atlas cell to its uv rectangle,
    /// as the sprite packer does.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layouts: &Layouts, world: &World, css: [f32; 2], uv_of: impl Fn(u16) -> [f32; 4]) {
        self.count = 0;
        let (Some(scene), Some(params)) = (world.try_res::<RoadScene>(), world.try_res::<RoadParams>()) else {
            return;
        };
        let v = params.values(&world.res::<ParamRegistry>());
        scene.build(&v, css, &mut self.out);
        if self.out.quads.is_empty() {
            return;
        }
        // Solid fills read the centre texel of the built-in white square.
        let sq = uv_of(crate::atlas::cell::SQUARE);
        let mid = [(sq[0] + sq[2]) * 0.5, (sq[1] + sq[3]) * 0.5];
        let fill = [mid[0], mid[1], mid[0], mid[1]];
        let fog = scene.look.fog;
        self.staging.clear();
        self.staging.extend(self.out.quads.iter().map(|q| Instance {
            top: [q.corners[0][0], q.corners[0][1], q.corners[1][0], q.corners[1][1]],
            bottom: [q.corners[2][0], q.corners[2][1], q.corners[3][0], q.corners[3][1]],
            uv: if q.cell == FILL { fill } else { uv_of(q.cell) },
            color_top: q.top,
            color_bottom: q.bottom,
            fog: [fog[0], fog[1], fog[2], q.fog.clamp(0.0, 1.0)],
        }));
        let bytes: &[u8] = bytemuck::cast_slice(&self.staging);
        let need = bytes.len() as u64;
        if self.buf.as_ref().is_none_or(|(_, cap)| *cap < need) {
            let cap = need.next_power_of_two().max(16 * 1024);
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("evani.pseudo3d"),
                size: cap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.buf = Some((buf, cap));
        }
        if let Some((buf, _)) = &self.buf {
            queue.write_buffer(buf, 0, bytes);
        }
        if self.pipe.is_none() {
            self.pipe = Some(pipeline(device, layouts));
        }
        self.count = self.staging.len() as u32;
    }

    /// Is there a road to draw this frame? (Only the 3D path asks: it skips
    /// an empty sprites pass.)
    #[cfg_attr(not(feature = "forward"), allow(dead_code))]
    pub fn drawing(&self) -> bool {
        self.count > 0
    }

    /// Draw the road into the open sprites pass (before the sprites); the
    /// draw count.
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>, atlas_bg: &wgpu::BindGroup, sprite_off: u32) -> u32 {
        let (Some(pipe), Some((buf, _)), true) = (&self.pipe, &self.buf, self.count > 0) else {
            return 0;
        };
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, atlas_bg, &[sprite_off]);
        rp.set_vertex_buffer(0, buf.slice(..));
        rp.draw(0..6, 0..self.count);
        1
    }
}
