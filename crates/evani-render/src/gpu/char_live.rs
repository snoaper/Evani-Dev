//! The character materials' live constants on the GPU (feature
//! `char-materials` without `relief`): binding 14 of the model material
//! group, a 464-byte uniform the `char_params` slot's body reads
//! (evani-material-character `shader::DATA_BODY`). The CPU side keeps
//! [`CharLive`] in step with the params and the tier; this uploads it when
//! its generation moves. Before, those values were written into the slot
//! body, so every tier change and every character slider rebuilt both model
//! pipelines (one 190 KB module), and the page froze 30 to 53 s while the
//! browser compiled each new text (2026-10-25). Kept in its own file so the
//! model pipeline's file only gains same-line, cfg'd additions.

use evani_core::ecs::World;
use evani_material_character::params::{CharLive, ENTRIES};

pub(super) struct CharLiveGpu {
    pub(super) buf: wgpu::Buffer,
    /// The generation last uploaded (0: none yet).
    sent: u64,
}

impl CharLiveGpu {
    pub(super) fn new(dev: &wgpu::Device) -> Self {
        let buf = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.model.char_live"),
            size: (ENTRIES * 16) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buf, sent: 0 }
    }

    /// Upload the constants when they changed since the last upload.
    pub(super) fn write_frame(&mut self, queue: &wgpu::Queue, world: &World) {
        let Some(live) = world.try_res::<CharLive>() else {
            return;
        };
        if live.generation == self.sent {
            return;
        }
        self.sent = live.generation;
        queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(&live.entries));
    }
}
