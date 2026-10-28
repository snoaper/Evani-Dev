//! The two-cascade shadow atlas on the GPU (feature `shadow-cascades`,
//! S5-R4; the plan is `crate::shadow_ext`). A child of `frame` so it can
//! read the frame's private buffers. The map key (`ShadowPlan::res`) carries
//! [`WIDE`] for the atlas: two res x res cascades side by side, far on the
//! left (drawn by the existing shadow pass) and near on the right (drawn
//! here, in the same pass, with its own uniform).

use crate::mesh;
use crate::shadow_ext::WIDE;

/// Atlas width of map key `key` (2 res when [`WIDE`]).
pub(super) fn width(key: u32) -> u32 {
    let r = key & !WIDE;
    if key & WIDE != 0 { r * 2 } else { r }
}

/// Atlas height of map key `key`.
pub(super) fn height(key: u32) -> u32 {
    key & !WIDE
}

/// Draw into cascade `c` (0 far, 1 near) of an atlas; a one-map key draws
/// to the whole map as before.
pub(super) fn viewport(rp: &mut wgpu::RenderPass<'_>, key: u32, c: u32) {
    if key & WIDE != 0 {
        let r = height(key) as f32;
        rp.set_viewport(c as f32 * r, 0.0, r, r, 0.0, 1.0);
    }
}

impl super::Gpu {
    /// The near cascade's casters, after the far cascade's in the same pass:
    /// meshes (ground planes cast nothing) and models. Returns the draw count.
    pub(super) fn draw_near_cascade(&self, rp: &mut wgpu::RenderPass<'_>, key: u32) -> u32 {
        let Some(off) = self.shadows.near else {
            return 0;
        };
        if key & WIDE == 0 {
            return 0;
        }
        viewport(rp, key, 1);
        rp.set_pipeline(&self.shadows.pipe);
        // With a kit material, the near cascade keeps its cut-outs too.
        #[cfg(feature = "kit-material")]
        self.kit.bind_shadow(rp);
        rp.set_bind_group(0, &self.small_bg, &[off]);
        rp.set_vertex_buffer(0, self.mesh_vb.slice(..));
        rp.set_vertex_buffer(1, self.instances.buf.slice(..));
        rp.set_index_buffer(self.mesh_ib.slice(..), wgpu::IndexFormat::Uint32);
        let mut draws = 0;
        let mut first = 0u32;
        for (i, &n) in self.counts.iter().enumerate() {
            if n > 0 && i != usize::from(mesh::MeshId::PLANE.0) {
                let (fi, ic, bv) = self.mesh_ranges[i];
                rp.draw_indexed(fi..fi + ic, bv, first..first + n);
                draws += 1;
            }
            first += n;
        }
        #[cfg(feature = "models")]
        {
            draws += self.models.draw_shadows(rp, &self.small_bg, off);
        }
        draws
    }
}
