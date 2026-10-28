//! Sky ambient in the frame (features `ibl` and `gtao`, S5-R5): a child of
//! `frame` so it can read the frame's private buffers. Two hooks: after the
//! instances are packed ([`ambient_prepare`]: the ambient uniform and, with
//! `gtao`, the AO prepass uniforms) and after the shadow pass
//! ([`ambient_frame`]: the sky cube, the AO passes and the forward frame
//! group, which then replaces the shadow one).
//!
//! [`ambient_prepare`]: super::Gpu::ambient_prepare
//! [`ambient_frame`]: super::Gpu::ambient_frame

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_core::plugin::MaterialDesc;
use evani_core::tiers::Tiers;

use super::FRAME_BINDING;
use crate::ambient::AmbientParams;
use crate::gpu::ambient::{BindKey, BindViews};
use crate::shaders::Lighting;
use crate::world::RenderWorld;

impl super::Gpu {
    /// Write this frame's ambient uniform (and the AO prepass uniforms).
    pub(super) fn ambient_prepare(&mut self, world: &World, rw: &RenderWorld, lighting: &Lighting, materials: &[MaterialDesc]) {
        let Some(p) = world.try_res::<AmbientParams>() else {
            return;
        };
        let reg = world.res::<ParamRegistry>();
        let tiers = world.res::<Tiers>();
        let tier = tiers.active();
        #[cfg(feature = "ibl")]
        let cube = tier.ibl_cube;
        #[cfg(not(feature = "ibl"))]
        let cube = 0;
        // Every material occludes, unlit ones too (the prepass gets no
        // material list), so `materials` is not needed here.
        #[cfg(feature = "gtao")]
        let ao = {
            let _ = materials;
            self.gtao_prepare(&reg, &p, tier.gtao, rw)
        };
        #[cfg(not(feature = "gtao"))]
        let ao = {
            let _ = (rw, materials, tier);
            [0.0; 4]
        };
        self.ambient.prepare(&self.device, &self.queue, &reg, &p, lighting, cube, ao);
    }

    /// The sky cube and AO passes (before the graph), then the forward
    /// frame group over the current shadow map.
    pub(super) fn ambient_frame(&mut self, enc: &mut wgpu::CommandEncoder, draws: &mut u32) {
        #[cfg(feature = "ibl")]
        {
            *draws += self.ambient.draw_cube(enc);
        }
        #[cfg(feature = "gtao")]
        {
            *draws += self.gtao_draw(enc);
        }
        #[cfg(not(any(feature = "ibl", feature = "gtao")))]
        let _ = (enc, draws);
        let (shadow, view) = match &self.shadows.map {
            Some((res, _, v)) => (*res, v),
            None => (0, &self.shadows.dummy_view),
        };
        let views = BindViews { ring: &self.ring.buf, ring_size: FRAME_BINDING, shadow: view, cmp: &self.shadows.cmp };
        self.ambient.bind(&self.device, &self.layouts.fwd_amb, BindKey { shadow, cube: 0, ao: 0 }, &views);
    }
}

/// Effect radius multiplier and falloff fraction (XeGTAO's defaults).
#[cfg(feature = "gtao")]
const RADIUS_MUL: f32 = 1.457;
#[cfg(feature = "gtao")]
const FALLOFF: f32 = 0.615;
/// XeGTAO's final power on the visibility.
#[cfg(feature = "gtao")]
const FINAL_POWER: f32 = 2.2;

#[cfg(feature = "gtao")]
impl super::Gpu {
    /// The AO views for this frame at tier quality `quality` (0 off): the
    /// prepass uniforms (each view's matrix with depth flipped to the
    /// shadow pipeline's conventional direction: row 2 = row 3 - row 2) and
    /// the AO pass slots. Returns the ambient uniform's AO vec4.
    fn gtao_prepare(&mut self, reg: &ParamRegistry, p: &AmbientParams, quality: u32, rw: &RenderWorld) -> [f32; 4] {
        use crate::gpu::gtao::MAX_VIEWS;
        use crate::shaders::{self, SHADOW_WORDS}; #[cfg(not(feature = "camera-range"))] use crate::shaders::{FAR, NEAR}; #[cfg(feature = "camera-range")] #[allow(non_snake_case)] let (NEAR, FAR) = crate::camera_range::current();
        let (strength, radius) = p.ao(reg);
        let full = [self.size[0].max(1), self.size[1].max(1)];
        let inv = [1.0 / full[0] as f32, 1.0 / full[1] as f32];
        if quality == 0 || strength <= 0.0 || radius <= 0.0 {
            self.ambient.gtao.off();
            return [0.0, strength, inv[0], inv[1]];
        }
        let half = quality == 1;
        let scale = if half { 0.5 } else { 1.0 };
        let ao_size = if half { [full[0].div_ceil(2), full[1].div_ceil(2)] } else { full };
        let (slices, steps) = match quality {
            1 => (1.0, 2.0),
            2 => (2.0, 2.0),
            _ => (3.0, 3.0),
        };
        let r = NEAR / (FAR - NEAR);
        let g = &mut self.ambient.gtao;
        g.ensure(&self.device, ao_size);
        g.views.clear();
        let whole = [0.0, 0.0, full[0] as f32, full[1] as f32];
        let mut words = [0.0f32; SHADOW_WORDS];
        for i in 0..rw.views.len().clamp(1, MAX_VIEWS) {
            let (cam, px) = match rw.views.get(i) {
                Some(v) => (&v.camera, v.pixels(self.size)),
                None => (&rw.camera, whole),
            };
            if px[2] < 1.0 || px[3] < 1.0 {
                continue;
            }
            let aspect = px[2] / px[3];
            let mut vp = shaders::view_proj(cam, aspect);
            for c in 0..4 {
                vp[c * 4 + 2] = vp[c * 4 + 3] - vp[c * 4 + 2];
            }
            shaders::write_shadow_uniform(&mut words, &vp, &[]);
            let off = self.ring.push(&words[..]);
            let rect = px.map(|v| v * scale);
            let tan_y = (cam.fov_y_deg.to_radians() * 0.5).tan();
            let slot = g.views.len();
            g.set_view(
                slot,
                [rect, [tan_y * aspect, tan_y, radius * RADIUS_MUL, FALLOFF], [slices, steps, r * FAR, 1.0 + r], [FAR * 0.99, FINAL_POWER, 0.0, 0.0]],
            );
            g.views.push((off, rect));
        }
        g.upload(&self.queue);
        let on = if g.views.is_empty() { 0.0 } else { 1.0 };
        [on, strength, inv[0], inv[1]]
    }

    /// The AO depth prepass (every mesh, ground planes included, and the
    /// models, per view at AO resolution), then the GTAO passes. Returns the
    /// draw count.
    fn gtao_draw(&self, enc: &mut wgpu::CommandEncoder) -> u32 {
        let g = &self.ambient.gtao;
        let Some(depth) = g.depth_view() else {
            return 0;
        };
        if g.views.is_empty() {
            return 0;
        }
        let mut draws = 0;
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gtao_depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for &(off, r) in &g.views {
                rp.set_viewport(r[0], r[1], r[2], r[3], 0.0, 1.0);
                rp.set_pipeline(&self.shadows.pipe);
                // A kit material's cut-outs stay open in the AO depth too.
                #[cfg(feature = "kit-material")]
                self.kit.bind_shadow(&mut rp);
                rp.set_bind_group(0, &self.small_bg, &[off]);
                rp.set_vertex_buffer(0, self.mesh_vb.slice(..));
                rp.set_vertex_buffer(1, self.instances.buf.slice(..));
                rp.set_index_buffer(self.mesh_ib.slice(..), wgpu::IndexFormat::Uint32);
                let mut first = 0u32;
                for (i, &n) in self.counts.iter().enumerate() {
                    if n > 0 {
                        let (fi, ic, bv) = self.mesh_ranges[i];
                        rp.draw_indexed(fi..fi + ic, bv, first..first + n);
                        draws += 1;
                    }
                    first += n;
                }
                #[cfg(feature = "models")]
                {
                    draws += self.models.draw_shadows(&mut rp, &self.small_bg, off);
                }
            }
        }
        draws + g.draw(enc)
    }
}
