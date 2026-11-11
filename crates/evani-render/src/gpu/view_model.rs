//! The first-person view-model pass (feature `view-model`, S5-E3; the CPU
//! side is `crate::view_model`). A child of `frame` (declared at the end of
//! frame.rs), so the hooks there are appended to existing lines and no
//! line of a game without the feature moves.
//!
//! Each frame the view-model meshes go into world space through the frame's
//! camera and join the end of the instance stream (after the world's runs,
//! so the shadow pass, which draws only those runs, never sees them), with
//! a frame uniform of their own: the view model's field of view and near
//! plane, the world's lighting and shading. After the forward pass (and its
//! MSAA resolve into `hdr`) a `view_model` pass loads `hdr`, clears a
//! one-sample depth buffer of its own, draws them with the one-sample
//! forward pipeline and throws that depth away.
//!
//! With `ViewModel::mark_depth` (off by default) that depth is kept and
//! bound, and `vm_mark` runs a full-screen step on the scene depth after
//! the forward pass's depth is final (resolved from 4x when it is): wherever
//! the view model drew it writes depth 1.0, the nearest in reversed-Z.
//! While the option is off, none of this exists: no pipeline, no bind
//! group, the same depth usage, the same store op, no extra pass.

use evani_core::plugin::MaterialDesc;

use core::cell::Cell;

use super::{Gpu, Target, pipelines};
use crate::mesh::MESH_SLOTS;
use crate::shaders::{self, InstanceRaw, Lighting};
use crate::world::{MeshInstance, RenderWorld};

/// The view model's per-frame GPU state.
pub(super) struct VmGpu {
    /// Frame-uniform offset this frame (`None`: nothing to draw).
    off: Option<u32>,
    /// Pixel viewport.
    px: [f32; 4],
    /// Instances per mesh slot, and the stream index of the first one.
    counts: [u32; MESH_SLOTS],
    first: u32,
    /// Reused world-space instances and their packed form.
    world: Vec<MeshInstance>,
    raw: Vec<InstanceRaw>,
    /// The view model's own depth buffer, at the `hdr` target's size.
    depth: Option<([u32; 2], wgpu::Texture, wgpu::TextureView)>,
    /// The mark-depth step runs this frame (option on, a depth reader).
    mark: bool,
    /// The depth texture above can be bound for reading (made while the
    /// mark-depth option was on).
    bindable: bool,
    /// The view-model pass really ran this frame (its depth is fresh).
    drew: Cell<bool>,
    /// The mark step's pipeline and bindings; made on first use only.
    marker: Option<Marker>,
}

/// GPU side of `ViewModel::mark_depth`.
struct Marker {
    pipe: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// Reads the view model's depth; rebuilt with that texture.
    bg: Option<wgpu::BindGroup>,
}

impl VmGpu {
    pub(super) fn new() -> Self {
        Self {
            off: None,
            px: [0.0; 4],
            counts: [0; MESH_SLOTS],
            first: 0,
            world: Vec::new(),
            raw: Vec::new(),
            depth: None,
            mark: false,
            bindable: false,
            drew: Cell::new(false),
            marker: None,
        }
    }
}

impl Gpu {
    /// Pack this frame's view model (after the world's instances are
    /// packed, before they are uploaded) and write its frame uniform.
    pub(super) fn vm_prepare(&mut self, rw: &RenderWorld, lighting: &Lighting, materials: &[MaterialDesc], depth_read: bool) {
        self.vm.off = None; self.vm.mark = crate::view_model::marks_depth(&rw.view_model, depth_read); self.vm.drew.set(false); #[cfg(feature = "view-model-models")] if rw.view_model.meshes.is_empty() { self.vm_models_frame(rw, lighting, materials); return; }
        let vm = &rw.view_model;
        let Some(size) = find_hdr(&self.targets) else {
            return;
        };
        if vm.meshes.is_empty() || size[0] == 0 || size[1] == 0 {
            return;
        }
        let (cam, px) = match rw.views.first() {
            Some(v) => (v.camera, v.pixels(size)),
            None => (rw.camera, [0.0, 0.0, size[0] as f32, size[1] as f32]),
        };
        vm.world_instances(&cam, &mut self.vm.world);
        shaders::pack_instances(&self.vm.world, &mut self.vm.raw, &mut self.vm.counts);
        self.vm.first = self.instances.staging.len() as u32;
        self.instances.staging.extend_from_slice(&self.vm.raw);
        let vp = vm.view_proj(&cam, px[2] / px[3].max(1.0));
        // The shading tail (shadow map or blobs) is already in the words.
        shaders::write_frame(&mut self.frame_words, &vp, cam.eye, lighting, materials);
        self.vm.off = Some(self.ring.push(&self.frame_words[..]));
        self.vm.px = px;
        self.vm_ensure_depth(size);
    }

    /// The `view_model` pass into `hdr` (after the forward pass and its
    /// resolve). Returns the draw count and adds the triangles to `tris`.
    pub(super) fn vm_draw(&self, enc: &mut wgpu::CommandEncoder, hdr: &Target, tris: &mut u32) -> u32 {
        let (Some(off), Some((dsize, _, depth)), Some(pipe)) =
            (self.vm.off, self.vm.depth.as_ref(), self.pipes.engine(pipelines::FORWARD))
        else {
            return 0;
        };
        if *dsize != hdr.size {
            return 0;
        }
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("view_model"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &hdr.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(crate::depth::CLEAR),
                    store: depth_store(self.vm.mark),
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.vm.drew.set(true);
        rp.set_pipeline(pipe);
        rp.set_vertex_buffer(0, self.mesh_vb.slice(..));
        rp.set_vertex_buffer(1, self.instances.buf.slice(..));
        rp.set_index_buffer(self.mesh_ib.slice(..), wgpu::IndexFormat::Uint32);
        #[cfg(feature = "point-lights")]
        self.lights.bind(&mut rp);
        #[cfg(feature = "kit-material")]
        self.kit.bind(&mut rp);
        #[cfg(feature = "shadows")]
        let frame_bg = &self.shadows.fwd_bg;
        // With sky ambient (S5-R5) the forward layout's frame group is the
        // ambient one, as in the main forward pass.
        #[cfg(feature = "ambient")]
        let frame_bg = self.ambient.bg.as_ref().unwrap_or(frame_bg);
        #[cfg(not(feature = "shadows"))]
        let frame_bg = &self.frame_bg;
        rp.set_bind_group(0, frame_bg, &[off]);
        let px = self.vm.px;
        rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
        let mut draws = 0;
        let mut first = self.vm.first;
        for (i, &n) in self.vm.counts.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let (fi, ic, bv) = self.mesh_ranges[i];
            rp.draw_indexed(fi..fi + ic, bv, first..first + n);
            first += n;
            draws += 1;
            *tris = tris.saturating_add(n.saturating_mul(self.mesh_tris[i]));
        } #[cfg(feature = "view-model-models")] { draws += self.models.draw_vm(&mut rp, frame_bg, off, &self.pipes, tris); }
        draws
    }
}

impl Gpu {
    /// The view model's depth buffer at `size`: made again when the size
    /// changes, or when the mark-depth step needs to read it and the one
    /// made before could not be. Also makes the mark step's pipeline (once)
    /// and its binding. With the option off this is only the plain
    /// `RENDER_ATTACHMENT` texture the pass always had.
    fn vm_ensure_depth(&mut self, size: [u32; 2]) {
        let mark = self.vm.mark;
        let stale = self.vm.depth.as_ref().map(|d| d.0) != Some(size) || (mark && !self.vm.bindable);
        if stale {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("evani.view_model_depth"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: depth_usage(mark),
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.vm.depth = Some((size, tex, view));
            self.vm.bindable = mark;
            if let Some(m) = self.vm.marker.as_mut() {
                m.bg = None;
            }
        }
        if !mark {
            return;
        }
        if self.vm.marker.is_none() {
            self.vm.marker = Some(make_marker(&self.device));
        }
        if let (Some(m), Some((_, _, view))) = (self.vm.marker.as_mut(), self.vm.depth.as_ref())
            && m.bg.is_none()
        {
            m.bg = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("evani.view_model_mark"),
                layout: &m.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                }],
            }));
        }
    }

    /// The mark-depth step (`ViewModel::mark_depth`): after the scene depth
    /// is final, write depth 1.0 into `depth` where the view-model pass drew.
    /// Nothing runs unless the option is on, a pass reads depth and the
    /// view-model pass ran this frame.
    pub(super) fn vm_mark(&self, enc: &mut wgpu::CommandEncoder, depth: &Target) -> u32 {
        let (true, true, Some(Marker { pipe, bg: Some(bg), .. })) = (self.vm.mark, self.vm.drew.get(), self.vm.marker.as_ref()) else {
            return 0;
        };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("view_model_mark_depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, bg, &[]);
        rp.draw(0..3, 0..1);
        1
    }
}

/// The usage of the view model's depth texture: attachment only, plus
/// binding for reading while the mark-depth step needs it.
fn depth_usage(mark: bool) -> wgpu::TextureUsages {
    if mark {
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
    } else {
        wgpu::TextureUsages::RENDER_ATTACHMENT
    }
}

/// What the view-model pass does with its depth at the end: keep it only
/// for the mark-depth step to read.
fn depth_store(mark: bool) -> wgpu::StoreOp {
    if mark { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard }
}

fn make_marker(device: &wgpu::Device) -> Marker {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("evani.view_model_mark"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("evani.view_model_mark"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.view_model_mark"),
        source: wgpu::ShaderSource::Wgsl(crate::view_model::MARK_DEPTH_WGSL.into()),
    });
    let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.view_model_mark"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    });
    Marker { pipe, layout, bg: None }
}

/// The `hdr` target's size, if it exists.
fn find_hdr(targets: &[Target]) -> Option<[u32; 2]> {
    targets.iter().find(|t| t.name == "hdr").map(|t| t.size)
}

/// First-person models without view-model meshes (feature
/// `view-model-models`): the view model's frame uniform and depth buffer
/// for a frame whose view model is models only.
#[cfg(feature = "view-model-models")]
impl Gpu {
    fn vm_models_frame(&mut self, rw: &RenderWorld, lighting: &Lighting, materials: &[MaterialDesc]) {
        let Some(size) = find_hdr(&self.targets) else {
            return;
        };
        if !self.models.vm_active() || size[0] == 0 || size[1] == 0 {
            return;
        }
        let (cam, px) = match rw.views.first() {
            Some(v) => (v.camera, v.pixels(size)),
            None => (rw.camera, [0.0, 0.0, size[0] as f32, size[1] as f32]),
        };
        self.vm.counts = [0; MESH_SLOTS];
        let vp = rw.view_model.view_proj(&cam, px[2] / px[3].max(1.0));
        shaders::write_frame(&mut self.frame_words, &vp, cam.eye, lighting, materials);
        self.vm.off = Some(self.ring.push(&self.frame_words[..]));
        self.vm.px = px;
        self.vm_ensure_depth(size);
    }
}
