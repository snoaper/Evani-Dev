//! Sun shadow cascades and soft shadows (S5-R4): features `shadow-cascades`
//! and `pcss` (both imply `shadows`). Pure data, beside [`crate::shadow`]
//! (which stays as it was): the GPU side is `gpu/frame/cascade.rs` and the
//! WGSL is chosen at compile time (`shaders.rs`, `FORWARD_EXT`).
//!
//! Cascades. The far cascade is today's shadow box exactly (same centre,
//! size and depth). The near cascade has the same centre and depth and
//! `render.shadow_cascade_split` of its width (at least 1 m), so the bias
//! and normal offset mean the same in both. Both are res x res maps side by
//! side in one depth atlas (far left, near right). Each box is snapped to
//! its own texel grid by [`crate::shaders::light_view_proj`] and neither
//! box's size changes as the camera turns, so edges do not shimmer. The
//! shader picks the near cascade inside its box and blends to the far one
//! over the outer `render.shadow_cascade_blend` of it. A tier gets two when
//! its `shadow_cascades` is 2 (High, Ultra) and the device fits a 2 res wide
//! texture with res >= 256; otherwise one map, as before.
//!
//! Soft shadows: the `pcss_params` slot the PCSS lookup calls (the
//! stylizedpbr look's name and default).

// The frame code that calls the cascade split is wasm only (`gpu`).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(feature = "shadow-cascades")]
use evani_core::params::{ParamDef, ParamError, ParamRegistry};
#[cfg(feature = "shadow-cascades")]
use evani_core::plugin::PluginContext;
#[cfg(feature = "shadow-cascades")]
use evani_core::tiers::Tiers;

#[cfg(feature = "shadow-cascades")]
use crate::math::Mat4;
#[cfg(feature = "shadow-cascades")]
use crate::shaders::{self, FRAME_WORDS, SHADING_AT};
#[cfg(feature = "shadow-cascades")]
use crate::shadow::{self, ShadowPlan};
#[cfg(feature = "shadow-cascades")]
use crate::world::RenderWorld;

/// Set in the shadow map key (`ShadowPlan::res`) when the map is the
/// two-cascade atlas (2 res wide).
#[cfg(feature = "shadow-cascades")]
pub(crate) const WIDE: u32 = 1 << 31;

/// Defaults of `render.shadow_cascade_split` and `render.shadow_cascade_blend`.
#[cfg(feature = "shadow-cascades")]
pub const SPLIT: f32 = 0.3;
#[cfg(feature = "shadow-cascades")]
pub const BLEND: f32 = 0.15;

/// Register the cascade params (render module build).
#[cfg(feature = "shadow-cascades")]
pub(crate) fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    ctx.param_f32(
        ParamDef::f32("render.shadow_cascade_split", SPLIT)
            .range(0.1, 0.8)
            .step(0.01)
            .unit("x")
            .describe("Near shadow cascade width as a share of the whole shadow box (sharper shadows close to the camera)")
            .gpu(),
    )?;
    ctx.param_f32(
        ParamDef::f32("render.shadow_cascade_blend", BLEND)
            .range(0.02, 0.5)
            .step(0.01)
            .describe("Share of the near cascade's box over which it blends into the far one (hides the switch)")
            .gpu(),
    )?;
    Ok(())
}

#[cfg(feature = "shadow-cascades")]
fn param(reg: &ParamRegistry, name: &str, default: f32) -> f32 {
    reg.f32_handle(name).map_or(default, |h| reg.f32(h))
}

/// Split this frame's shadow plan into cascades and write the shader's
/// cascade data into the frame uniform (after `write_shading`). Returns the
/// plan to draw (its `res` carries [`WIDE`] for the atlas) and the near
/// cascade's light matrix when there are two.
#[cfg(feature = "shadow-cascades")]
pub(crate) fn split(
    reg: &ParamRegistry,
    tiers: &Tiers,
    rw: &RenderWorld,
    light_dir: [f32; 3],
    plan: Option<ShadowPlan>,
    max_dim: u32,
    out: &mut [f32; FRAME_WORDS],
) -> (Option<ShadowPlan>, Option<Mat4>) {
    let Some(mut sp) = plan else {
        return (None, None);
    };
    let res = sp.res.min(max_dim / 2);
    let mut near = None;
    if tiers.active().shadow_cascades >= 2 && res >= 256 {
        // Today's box: half width from the matrix's x row (1 / half), centre
        // from the same focus the plan used.
        let m = &sp.light_vp;
        let row = (m[0] * m[0] + m[4] * m[4] + m[8] * m[8]).sqrt();
        let half = if row > 0.0 { 1.0 / row } else { shadow::DISTANCE };
        let (centre, _) = shadow::focus(rw);
        let depth = shadow::DEPTH.max(half);
        let k = param(reg, "render.shadow_cascade_split", SPLIT).clamp(0.05, 1.0);
        sp.res = res;
        sp.light_vp = shaders::light_view_proj(centre, half, depth, light_dir, res);
        near = Some(shaders::light_view_proj(centre, (half * k).max(1.0), depth, light_dir, res));
    }
    write(out, &sp, near.as_ref(), param(reg, "render.shadow_cascade_blend", BLEND));
    if near.is_some() {
        sp.res |= WIDE;
    }
    (Some(sp), near)
}

/// The cascade words: far matrix and texel in the shadow fields, the near
/// matrix in `blobs[0..4]` and (cascades, blend start, atlas x scale, 0) in
/// `blobs[4]` (no blobs while the shadow map is on).
#[cfg(feature = "shadow-cascades")]
fn write(out: &mut [f32; FRAME_WORDS], sp: &ShadowPlan, near: Option<&Mat4>, blend: f32) {
    let at = SHADING_AT;
    out[at..at + 16].copy_from_slice(&sp.light_vp);
    out[at + 18] = 1.0 / sp.res.max(1) as f32;
    let b = at + 24;
    let (n, xs) = match near {
        Some(m) => {
            out[b..b + 16].copy_from_slice(m);
            (2.0, 0.5)
        }
        None => (1.0, 1.0),
    };
    out[b + 16..b + 20].copy_from_slice(&[n, 1.0 - blend.clamp(0.02, 0.5), xs, 0.0]);
}

/// Soft-shadow constants (penumbra per metre, search radius, min and max
/// filter radius in texels): the stylizedpbr look's `pcss_params` slot.
#[cfg(feature = "pcss")]
pub const PCSS_PARAMS: evani_core::slots::SlotSignature = evani_core::slots::SlotSignature {
    name: "pcss_params",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Soft shadows: penumbra per metre, search radius, min and max filter radius (texels)",
    default_body: "    return vec4<f32>(0.02, 6.0, 1.0, 12.0);",
};
