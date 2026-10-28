//! GPU side of the lit surface pass (feature `look-surface` without
//! `relief`, S5-R5b; the CPU side is `crate::model::surface`, the shader
//! `shaders/model_lit.wgsl`): the material group's three extra bindings (5
//! the metallic-roughness map, 6 the occlusion map, 8 the material's
//! surface words) and the buffer behind 8. Kept in its own file so the model
//! pipeline's file only gains same-line, cfg'd additions.

use crate::model::surface::{SURFACE_WORDS, Surface};

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

/// A material's surface uniform (binding 8). `occlusion`: its occlusion map
/// is on the GPU; without it the strength is 0 (the white default reads as
/// no occlusion either way).
pub(super) fn material_buffer(dev: &wgpu::Device, queue: &wgpu::Queue, s: &Surface, occlusion: bool) -> wgpu::Buffer {
    let mut words: [f32; SURFACE_WORDS] = s.words;
    if !occlusion {
        words[2] = 0.0;
    }
    let b = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("evani.model.surface"),
        size: (SURFACE_WORDS * 4) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&b, 0, bytemuck::cast_slice(&words));
    b
}
