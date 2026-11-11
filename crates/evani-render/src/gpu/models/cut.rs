//! The cut-out's shadow draws (batch 3 B3-P2, `material-recipes-cut` with
//! `shadows`): the masked parts of the shadow pass through the hole-aware
//! depth pipeline (`super::super::models_recipes::CutShadows`). Its own file,
//! so the model pipeline's file only gains same-line, cfg'd additions and no
//! line of it moves for games without the cut-out.

use super::ModelGpu;

impl ModelGpu {
    /// The masked parts of the shadow pass (batch 3 B3-P2): drawn through the
    /// hole-aware depth pipeline, which drops what the colour pass drops (a
    /// texture's alpha under its cutoff, a cut-out block's holes).
    pub(super) fn cut_shadows(&self, rp: &mut wgpu::RenderPass<'_>, bg: &wgpu::BindGroup, off: u32) -> u32 {
        let mut draws = 0;
        let mut set = false;
        for r in &self.runs {
            let Some(p) = self.ready.get(r.model).and_then(Option::as_ref).and_then(|m| m.parts.get(r.part)) else {
                continue;
            }; #[cfg(feature = "skinning")] if p.sb.is_some() { continue; }
            let Some(g) = self.cut.group(r.model, p.part.material) else {
                continue;
            }; #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] if self.blend.ao.get() && self.blend.mode(r.model, p.part.material) != 0 { continue; }
            if !set {
                rp.set_pipeline(&self.cut.pipeline);
                rp.set_bind_group(0, bg, &[off]);
                rp.set_vertex_buffer(1, self.instances.slice(..));
                set = true;
            }
            rp.set_bind_group(1, g, &[]);
            rp.set_vertex_buffer(0, p.vb.slice(..));
            rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..p.count, 0, r.first..r.first + r.count);
            draws += 1;
        }
        draws
    }
}
