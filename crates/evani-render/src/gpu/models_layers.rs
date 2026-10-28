//! GPU side of layered materials (feature `material-layers` without
//! `relief`; the CPU side is `crate::model::layers` and the crate
//! `evani-material-layers`, the shader `shaders/model_lit_layers.wgsl`): the
//! material group's three extra bindings on top of the lit surface pass's (9
//! the weave and knit map made in code once, 10 the material's layer record,
//! 11 the shared per-frame layer uniform: the live strengths and the tier's
//! path) and the buffers and texture behind them. Live strengths move only
//! binding 11's 32 bytes each frame, so a slider never rebuilds a pipeline.
//! Kept in its own file so the model pipeline's file only gains same-line,
//! cfg'd additions.

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_core::tiers::Tiers;
use evani_material_layers::params::{LayerParams, default_frame, frame_words};
use evani_material_layers::{FRAME_WORDS, LAYER_WORDS, weave};

/// The weave map's seed: the same tile in every game (same inputs, same bytes).
const WEAVE_SEED: u32 = 0x00e7_a7e5;

/// What the layered pass keeps beside the model pipeline: the weave map and
/// the per-frame uniform.
pub(super) struct LayersGpu {
    _weave: wgpu::Texture,
    pub(super) weave_view: wgpu::TextureView,
    pub(super) frame: wgpu::Buffer,
}

fn uniform_buffer(dev: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
    dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl LayersGpu {
    /// Bake the 64x64 weave and knit tile (linear RGBA8, one level: the
    /// shader fades it out where a texel gets smaller than a pixel) and make
    /// the frame uniform.
    pub(super) fn new(dev: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let size = wgpu::Extent3d { width: weave::SIZE, height: weave::SIZE, depth_or_array_layers: 1 };
        let t = dev.create_texture(&wgpu::TextureDescriptor {
            label: Some("evani.model.weave"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            t.as_image_copy(),
            &weave::weave_map(WEAVE_SEED),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(weave::SIZE * 4), rows_per_image: Some(weave::SIZE) },
            size,
        );
        let weave_view = t.create_view(&wgpu::TextureViewDescriptor::default());
        let frame = uniform_buffer(dev, "evani.model.layers_frame", (FRAME_WORDS * 4) as u64);
        queue.write_buffer(&frame, 0, bytemuck::cast_slice(&default_frame("medium")));
        Self { _weave: t, weave_view, frame }
    }

    /// This frame's layer uniform: the live strengths and the active tier's
    /// path (`material.layers.quality` auto: Lite and Low cheap).
    pub(super) fn write_frame(&self, queue: &wgpu::Queue, world: &World) {
        let tier = world.try_res::<Tiers>().map_or_else(|| "medium".to_string(), |t| t.active().id.clone());
        let words = match (world.try_res::<ParamRegistry>(), world.try_res::<LayerParams>()) {
            (Some(reg), Some(p)) => frame_words(&reg, &p, &tier),
            _ => default_frame(&tier),
        };
        queue.write_buffer(&self.frame, 0, bytemuck::cast_slice(&words));
    }
}

/// A material's layer record (binding 10).
pub(super) fn material_buffer(dev: &wgpu::Device, queue: &wgpu::Queue, words: &[f32; LAYER_WORDS]) -> wgpu::Buffer {
    let b = uniform_buffer(dev, "evani.model.layers", (LAYER_WORDS * 4) as u64);
    queue.write_buffer(&b, 0, bytemuck::cast_slice(words));
    b
}
