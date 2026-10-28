//! GPU side of per-level asset groups (feature `level-groups`, S7-7; the CPU
//! side and the rules are `crate::model::handle::level`).
//!
//! Once a frame the renderer drops the GPU copies of the models a released
//! group held (textures, vertex and index buffers, bind groups go with the
//! `GpuModel`) and takes their texture bytes off the tier's budget, so the
//! next level's textures fit under `max_texture_memory`.

use crate::model::Models;

use super::ModelGpu;

impl ModelGpu {
    /// Drop what released models still hold, whether uploaded or still on
    /// their way, and give back their texture bytes.
    pub(super) fn sweep_released(&mut self, models: &mut Models) {
        let (ids, freed) = models.stream.levels.take_sweep();
        for id in ids {
            if let Some(slot) = self.ready.get_mut(id.0 as usize) {
                *slot = None;
            }
            self.loading.retain(|l| l.id != id);
        }
        self.resident = self.resident.saturating_sub(freed);
    }
}
