//! GPU side of skinned models (feature `skinning`, S3-9; the CPU side is
//! `crate::model::skin`).
//!
//! * One read-only storage buffer a frame holds every drawn character's
//!   joint matrices (3x4, 48 bytes each): a bind-pose palette per drawn
//!   skinned model that some draw shows unposed, then each posed draw's
//!   matrices. Each instance's position-offset w (unused by the static
//!   shader) is patched to its first matrix.
//! * Skinned parts have a second vertex buffer (joints u8x4, weights
//!   unorm8x4) and draw after the static runs with their own pipeline: the
//!   model pipeline's layout plus the joints group, `model_skin_vs`.
//! * Skinned shadows: `model_skin_shadow_vs` in the model shadow module,
//!   joints at group 1.

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;

use super::super::pipelines::{Layouts, MSAA_SAMPLES, Pipelines};
use super::{GpuModel, INSTANCE_ATTRS, INSTANCE_WORDS, ModelGpu, VERTEX_ATTRS, VERTEX_BYTES};
use crate::model::Models;
use crate::model::skin::{Mat43, SKIN_BYTES, joint_offsets};

const SKIN_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![11 => Uint8x4, 12 => Unorm8x4];
/// Bytes per joint matrix in the storage buffer (`mat3x4<f32>`).
const MAT_BYTES: u64 = 48;

/// The joints group: one read-only storage buffer, vertex stage.
fn joints_layout(dev: &wgpu::Device) -> wgpu::BindGroupLayout {
    dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("evani.layout.joints"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(MAT_BYTES),
            },
            count: None,
        }],
    })
}

fn skin_buffer() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout { array_stride: SKIN_BYTES as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &SKIN_ATTRS }
}

/// The skinned model pipeline (`model_skin_vs` / `model_fs` of the forward
/// module), 1x or 4x: the model pipeline with the joints group and the skin
/// vertex buffer.
pub(in crate::gpu) fn pipeline(dev: &wgpu::Device, layouts: &Layouts, module: &wgpu::ShaderModule, ms: bool) -> wgpu::RenderPipeline {
    #[cfg(all(feature = "shadows", not(feature = "ambient")))]
    let frame = &layouts.fwd; #[cfg(feature = "ambient")] let frame = &layouts.fwd_amb;
    #[cfg(not(feature = "shadows"))]
    let frame = &layouts.b;
    let (mat, joints) = (super::material_layout(dev), joints_layout(dev));
    let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(frame), Some(&mat), Some(&joints), #[cfg(feature = "point-lights")] Some(&layouts.lights.lights)],
        immediate_size: 0,
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout { array_stride: VERTEX_BYTES as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &VERTEX_ATTRS }),
        Some(wgpu::VertexBufferLayout { array_stride: (INSTANCE_WORDS * 4) as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &INSTANCE_ATTRS }),
        Some(skin_buffer()),
    ];
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState { module, entry_point: Some("model_skin_vs"), compilation_options: Default::default(), buffers: &buffers },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(crate::depth::COMPARE),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState { count: if ms { MSAA_SAMPLES } else { 1 }, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module,
            #[cfg(not(any(feature = "relief", feature = "look-surface")))]
            entry_point: Some("model_fs"),
            // Surface relief (S3-10) shades skinned parts too: the same
            // fragment stage as the static model pipeline, same `ModelOut`.
            #[cfg(feature = "relief")]
            entry_point: Some("model_relief_fs"), #[cfg(all(feature = "look-surface", not(feature = "relief")))] entry_point: Some(super::LIT_FS),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Skinned depth into the shadow map: the shadow layout plus joints.
#[cfg(feature = "shadows")]
fn shadow_pipeline(dev: &wgpu::Device, layouts: &Layouts, joints: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.model_skin_shadows"),
        source: wgpu::ShaderSource::Wgsl(crate::shaders::MODEL_SHADOWS.into()),
    });
    let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layouts.b), Some(joints)],
        immediate_size: 0,
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout { array_stride: VERTEX_BYTES as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &super::SHADOW_VERTEX_ATTRS }),
        Some(wgpu::VertexBufferLayout { array_stride: (INSTANCE_WORDS * 4) as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &super::SHADOW_INSTANCE_ATTRS }),
        Some(skin_buffer()),
    ];
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.model_skin_shadows"),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("model_skin_shadow_vs"), compilation_options: Default::default(), buffers: &buffers },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    })
}

/// A drawn skinned model's skin: its bind matrices, and its skinned parts'
/// own vertex bytes until the sun cones are baked ([`crate::model::skin::unproxy`]).
pub(super) struct ModelSkin {
    pub(super) bind: Vec<Mat43>,
    pub(super) keep: Vec<Option<Vec<u8>>>,
}

/// The skinning state of [`ModelGpu`].
pub(super) struct SkinGpu {
    layout: wgpu::BindGroupLayout,
    buf: wgpu::Buffer,
    bg: wgpu::BindGroup,
    #[cfg(feature = "shadows")]
    shadow: wgpu::RenderPipeline,
    /// This frame's matrices, as uploaded.
    staging: Vec<Mat43>,
    /// Scratch: each model's bind palette this frame.
    bind_at: Vec<(usize, u32)>,
    /// A posed draw with the wrong matrix count was reported.
    warned: bool,
}

fn joints_buffer(dev: &wgpu::Device, layout: &wgpu::BindGroupLayout, bytes: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("evani.model.joints"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("evani.model.joints"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
    });
    (buf, bg)
}

impl SkinGpu {
    pub(super) fn new(dev: &wgpu::Device, layouts: &Layouts) -> Self {
        #[cfg(not(feature = "shadows"))]
        let _ = layouts;
        let layout = joints_layout(dev);
        let (buf, bg) = joints_buffer(dev, &layout, MAT_BYTES * 64);
        Self {
            #[cfg(feature = "shadows")]
            shadow: shadow_pipeline(dev, layouts, &layout),
            layout,
            buf,
            bg,
            staging: Vec::new(),
            bind_at: Vec::new(),
            warned: false,
        }
    }

    /// This frame's joint buffer and each draw's first matrix: posed draws
    /// get their own matrices, the rest of a skinned model's draws share its
    /// bind palette. Uploads it (growing the buffer as needed). The offsets
    /// go to `models.skin.offsets`, which `Models::stage` writes into each
    /// instance.
    pub(super) fn frame(&mut self, dev: &wgpu::Device, queue: &wgpu::Queue, ready: &[Option<GpuModel>], models: &mut Models, world: &World) {
        let bind_of = |mi: usize| ready.get(mi).and_then(Option::as_ref).map(|g| g.skin.bind.as_slice());
        let mut offsets = core::mem::take(&mut models.skin.offsets);
        let wrong = joint_offsets(&models.draws, &models.skin, bind_of, &mut self.staging, &mut offsets, &mut self.bind_at);
        models.skin.offsets = offsets;
        if wrong && !self.warned {
            self.warned = true;
            world.res_mut::<ErrorLog>().error("model: a posed draw without one matrix per joint of its skin; drawn in bind pose".to_owned());
        }
        if self.staging.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&self.staging);
        if bytes.len() as u64 > self.buf.size() {
            (self.buf, self.bg) = joints_buffer(dev, &self.layout, (bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.buf, 0, bytes);
    }
}

impl ModelGpu {
    /// The skinned runs of the forward pass, view by view (after the static
    /// ones, which skip them). Returns the draw count and adds the triangles
    /// to `tris`.
    pub(super) fn draw_skinned(&self, rp: &mut wgpu::RenderPass<'_>, frame_bg: &wgpu::BindGroup, views: &[(u32, [f32; 4])], ms: bool, pipes: &Pipelines, tris: &mut u32) -> u32 {
        let Some(pipe) = pipes.skin(ms) else {
            return 0;
        };
        let mut draws = 0;
        let mut set = false;
        for &(off, px) in views {
            for r in &self.runs {
                let Some(gm) = self.ready.get(r.model).and_then(Option::as_ref) else {
                    continue;
                };
                let (Some(p), Some(sb)) = (gm.parts.get(r.part), gm.parts.get(r.part).and_then(|p| p.sb.as_ref())) else {
                    continue;
                };
                let Some(mat) = gm.materials.get(p.part.material) else {
                    continue;
                };
                if !set {
                    set = true;
                    rp.set_pipeline(pipe);
                    rp.set_vertex_buffer(1, self.instances.slice(..));
                    rp.set_bind_group(2, &self.skin.bg, &[]);
                    rp.set_bind_group(0, frame_bg, &[off]);
                    rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
                }
                rp.set_bind_group(1, mat, &[]);
                rp.set_vertex_buffer(0, p.vb.slice(..));
                rp.set_vertex_buffer(2, sb.slice(..));
                rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
                // The live before/after split (S3-14) cuts skinned runs like static ones.
                #[cfg(feature = "hooks")]
                if !super::split_scissor(rp, self.split, r.model, px, views.len()) {
                    continue;
                }
                rp.draw_indexed(0..p.count, 0, r.first..r.first + r.count);
                draws += 1;
                *tris = tris.saturating_add((p.count / 3).saturating_mul(r.count));
            }
            #[cfg(feature = "hooks")]
            super::split_reset(rp, self.split, px, views.len());
            // The next view's uniform and viewport, once anything is drawn.
            set = false;
        }
        draws
    }

    /// The skinned runs of the shadow pass.
    #[cfg(feature = "shadows")]
    pub(super) fn skin_shadows(&self, rp: &mut wgpu::RenderPass<'_>, bg: &wgpu::BindGroup, off: u32, draws: &mut u32) {
        let mut set = false;
        for r in &self.runs {
            let Some(p) = self.ready.get(r.model).and_then(Option::as_ref).and_then(|m| m.parts.get(r.part)) else {
                continue;
            };
            let Some(sb) = p.sb.as_ref() else {
                continue;
            };
            if !set {
                set = true;
                rp.set_pipeline(&self.skin.shadow);
                rp.set_bind_group(0, bg, &[off]);
                rp.set_bind_group(1, &self.skin.bg, &[]);
                rp.set_vertex_buffer(1, self.instances.slice(..));
            }
            rp.set_vertex_buffer(0, p.vb.slice(..));
            rp.set_vertex_buffer(2, sb.slice(..));
            rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..p.count, 0, r.first..r.first + r.count);
            *draws += 1;
        }
    }
}
