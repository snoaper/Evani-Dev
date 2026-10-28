//! First-person models on the GPU (feature `view-model-models`; the CPU side
//! is `crate::model::vm`). Declared at the end of gpu/models.rs, so the
//! hooks there sit at the ends of existing lines.
//!
//! `prepare` stages the camera-space draws (in world space through the
//! frame's camera) after the world's runs, as runs of their own that the
//! forward and shadow passes never see; the `view_model` pass draws them
//! with the one-sample model pipeline and the view model's frame uniform.

use super::{ModelGpu, Run};
use crate::gpu::pipelines::Pipelines;
use crate::model::{INSTANCE_WORDS, Models};
use crate::world::RenderWorld;

impl ModelGpu {
    /// Stage this frame's first-person model draws, then clear them.
    pub(super) fn vm_stage(&mut self, models: &mut Models, rw: &RenderWorld) {
        self.vm_runs.clear();
        let cam = rw.views.first().map_or(rw.camera, |v| v.camera);
        for (mi, gm) in self.ready.iter().enumerate() {
            let Some(gm) = gm.as_ref().filter(|g| g.shown) else {
                continue;
            };
            if !models.vm.draws_model(mi) {
                continue;
            }
            for (pi, p) in gm.parts.iter().enumerate() {
                #[cfg(feature = "skinning")]
                if p.sb.is_some() {
                    continue;
                }
                let first = (self.staging.len() / INSTANCE_WORDS) as u32;
                models.vm.stage(mi, &p.part, &cam, &mut self.staging);
                let count = (self.staging.len() / INSTANCE_WORDS) as u32 - first;
                if count > 0 {
                    self.vm_runs.push(Run { model: mi, part: pi, first, count });
                }
            }
        }
        models.vm.clear();
    }

    /// Whether any first-person model draws this frame.
    pub(in crate::gpu) fn vm_active(&self) -> bool {
        !self.vm_runs.is_empty()
    }

    /// The first-person model draws, inside the `view_model` pass (its
    /// viewport already set): frame uniform at `off`. Returns the draw count
    /// and adds the triangles to `tris`.
    pub(in crate::gpu) fn draw_vm(
        &self,
        rp: &mut wgpu::RenderPass<'_>,
        frame_bg: &wgpu::BindGroup,
        off: u32,
        pipes: &Pipelines,
        tris: &mut u32,
    ) -> u32 {
        let (false, Some(pipe)) = (self.vm_runs.is_empty(), pipes.model(false)) else {
            return 0;
        };
        rp.set_pipeline(pipe);
        rp.set_vertex_buffer(1, self.instances.slice(..));
        rp.set_bind_group(0, frame_bg, &[off]);
        let mut draws = 0;
        for r in &self.vm_runs {
            let Some(gm) = self.ready.get(r.model).and_then(Option::as_ref) else {
                continue;
            };
            let Some(p) = gm.parts.get(r.part) else {
                continue;
            };
            let Some(mat) = gm.materials.get(p.part.material) else {
                continue;
            };
            rp.set_bind_group(1, mat, &[]);
            rp.set_vertex_buffer(0, p.vb.slice(..));
            rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..p.count, 0, r.first..r.first + r.count);
            draws += 1;
            *tris = tris.saturating_add((p.count / 3).saturating_mul(r.count));
        }
        draws
    }
}
