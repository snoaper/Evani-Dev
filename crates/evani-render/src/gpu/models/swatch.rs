//! The model side of the swatch balls (feature `swatches`, `crate::swatch`):
//! a swatch model's runs leave the frame's runs once they are staged, so the
//! main view, its shadow map and its see-through passes never draw them; the
//! swatch pass draws one model's runs at a time into its own picture.

use evani_core::ecs::World;

use super::{ModelGpu, Run};
use crate::gpu::pipelines::Pipelines;
use crate::swatch::Swatches;

impl ModelGpu {
    /// After the runs are staged: move the swatch models' runs aside.
    pub(super) fn swatch_split(&mut self, world: &World) {
        self.swatch_runs.clear();
        let Some(s) = world.try_res::<Swatches>() else { return };
        if s.models().is_empty() {
            return;
        }
        let (sw, main): (Vec<Run>, Vec<Run>) = self.runs.drain(..).partition(|r| s.is_swatch(r.model));
        self.runs = main;
        self.swatch_runs = sw;
    }

    /// Whether swatch model `model` can be drawn now: on the GPU, shown,
    /// staged this frame and every material pipeline settled (asked for
    /// while not: a material still compiling would draw the plain stand-in).
    pub(in crate::gpu) fn swatch_ready(&self, model: usize, ms: bool, pipes: &Pipelines) -> bool {
        let Some(gm) = self.ready.get(model).and_then(Option::as_ref).filter(|g| g.shown) else { return false };
        let mut any = false;
        let mut settled = true;
        for r in self.swatch_runs.iter().filter(|r| r.model == model) {
            let Some(p) = gm.parts.get(r.part) else { return false };
            any = true;
            #[cfg(feature = "material-split")]
            {
                let (code, kind) = self.recipes.split_of(model, p.part.material);
                settled &= pipes.material_settled(ms, code, kind);
            }
            #[cfg(not(feature = "material-split"))]
            {
                let _ = p;
                settled &= pipes.model(ms).is_some();
            }
        }
        any && settled
    }

    /// Draw swatch model `model` alone into the view at uniform `off` and
    /// viewport `px` (the render pass is the swatch pass's). See-through
    /// materials draw opaque here: the picture has no scene behind them.
    #[allow(clippy::too_many_arguments, reason = "the forward pass's own draw arguments plus the model")]
    pub(in crate::gpu) fn draw_swatch(&mut self, rp: &mut wgpu::RenderPass<'_>, frame_bg: &wgpu::BindGroup, off: u32, px: [f32; 4], model: usize, ms: bool, pipes: &Pipelines, tris: &mut u32) -> u32 {
        let own: Vec<Run> = self.swatch_runs.iter().filter(|r| r.model == model).map(|r| Run { model: r.model, part: r.part, first: r.first, count: r.count }).collect();
        if own.is_empty() {
            return 0;
        }
        let main = core::mem::replace(&mut self.runs, own);
        #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
        let blend = self.swatch_blend(false);
        let n = self.draw(rp, frame_bg, &[(off, px)], ms, pipes, tris);
        #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
        self.swatch_blend(blend);
        self.runs = main;
        n
    }
}
