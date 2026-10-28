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

use evani_core::plugin::MaterialDesc;

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
        }
    }
}

impl Gpu {
    /// Pack this frame's view model (after the world's instances are
    /// packed, before they are uploaded) and write its frame uniform.
    pub(super) fn vm_prepare(&mut self, rw: &RenderWorld, lighting: &Lighting, materials: &[MaterialDesc]) {
        self.vm.off = None; #[cfg(feature = "view-model-models")] if rw.view_model.meshes.is_empty() { self.vm_models_frame(rw, lighting, materials); return; }
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
        if self.vm.depth.as_ref().map(|d| d.0) != Some(size) {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.vm.depth = Some((size, tex, view));
        }
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
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
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
        if self.vm.depth.as_ref().map(|d| d.0) != Some(size) {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.vm.depth = Some((size, tex, view));
        }
    }
}
