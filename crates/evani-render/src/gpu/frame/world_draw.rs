//! The renderer's side of module-drawn world geometry (feature
//! `world-draws`, `crate::world_draw`): each module's compute pass before
//! the graph walk and its draws inside the forward pass.

use evani_core::ecs::World;

use crate::world_draw::{WorldDraws, WorldGpu};

impl super::Gpu {
    /// Before the graph walk: one compute pass per module draw.
    pub(super) fn world_prepare(&mut self, world: &World, enc: &mut wgpu::CommandEncoder) {
        let Some(wd) = world.try_res::<WorldDraws>() else {
            return;
        };
        let samples = self.msaa_samples();
        for d in &wd.draws {
            let gpu = WorldGpu {
                device: &self.device,
                queue: &self.queue,
                size: self.size,
                samples,
                color: wgpu::TextureFormat::Rgba16Float,
                depth: wgpu::TextureFormat::Depth32Float,
                views: &self.views,
            };
            let ts = self.timer.compute_pass(d.name());
            d.prepare(&gpu, enc, ts);
        }
    }

    /// Inside the forward pass, after the meshes and models.
    pub(super) fn world_draw(&self, world: &World, rp: &mut wgpu::RenderPass<'_>) -> u32 {
        let Some(wd) = world.try_res::<WorldDraws>() else {
            return 0;
        };
        let gpu = WorldGpu {
            device: &self.device,
            queue: &self.queue,
            size: self.size,
            samples: self.msaa_samples(),
            color: wgpu::TextureFormat::Rgba16Float,
            depth: wgpu::TextureFormat::Depth32Float,
            views: &self.views,
        };
        wd.draws.iter().map(|d| d.draw(&gpu, rp)).sum()
    }
}
