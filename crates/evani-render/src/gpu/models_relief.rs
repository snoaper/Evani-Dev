//! GPU side of surface relief (feature `relief`, S3-10; the CPU side is
//! `crate::model::relief`, the shader `shaders/model_relief.wgsl`): the
//! material group's three extra bindings (5 the height map, 6 the
//! material's relief words, 7 the shared per-frame relief uniform) and the
//! buffers behind them. Kept in its own file so the model pipeline's file
//! only gains same-line, cfg'd additions (no line of a non-relief build
//! moves).

use evani_core::ecs::World;
use evani_core::tiers::Tiers;

use crate::model::relief::{self, RELIEF_WORDS, Relief};

/// A fragment-stage uniform entry of the material group.
pub(super) fn uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn buffer(dev: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
    dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The shared per-frame relief uniform (binding 7): x = parallax steps.
pub(super) fn frame_buffer(dev: &wgpu::Device) -> wgpu::Buffer {
    buffer(dev, "evani.model.relief_frame", 16)
}

/// This frame's relief uniform: the active tier's parallax step count.
pub(super) fn write_frame(queue: &wgpu::Queue, buf: &wgpu::Buffer, world: &World) {
    let steps = world.try_res::<Tiers>().map_or(0, |t| relief::parallax_steps(&t.active().id));
    queue.write_buffer(buf, 0, bytemuck::cast_slice(&[steps as f32, 0.0, 0.0, 0.0]));
}

/// A material's relief uniform (binding 6). `height`: its height map is on
/// the GPU; without it the parallax is off (the micro-detail stays).
pub(super) fn material_buffer(dev: &wgpu::Device, queue: &wgpu::Queue, r: &Relief, height: bool) -> wgpu::Buffer {
    let mut words: [f32; RELIEF_WORDS] = r.words;
    if !height {
        words[0] = 0.0;
    }
    let b = buffer(dev, "evani.model.relief", (RELIEF_WORDS * 4) as u64);
    queue.write_buffer(&b, 0, bytemuck::cast_slice(&words));
    b
}
