//! Shadow plan of a frame: whether the shadow map is drawn (the `shadows`
//! effect is on and the tier gives a map size), its settings from the
//! `fx.shadows.*` params, and the box it covers. Pure data, so it is tested
//! natively; the GPU side (feature `shadows`) only draws what this decides.
//! Blob shadows ([`crate::RenderWorld::blobs`]) are used whenever there is
//! no plan.

use evani_core::effects::EffectsStatus;
use evani_core::params::{F32Param, ParamRegistry};
use evani_core::tiers::Tiers;

use crate::math::Mat4;
use crate::shaders;
use crate::world::RenderWorld;

/// Defaults of the `fx.shadows.*` params (registered by evani-fx).
pub const DISTANCE: f32 = 26.0;
pub const STRENGTH: f32 = 1.0;
/// The bias is in the light box's normalised depth, and that box is
/// 2 x [`DEPTH`] = 160 m deep (more for a wider box), so 0.0001 is about
/// 1.6 cm in the world. It was 0.0015 (about 24 cm): anything closer than
/// that to a surface along the light cast no shadow on it, so thin or low
/// casters (a ring round a ball, a kart's floor) lost the part of their
/// shadow near them and the rest ended in a straight cut. The hardware
/// slope bias of the depth pass and [`NORMAL_OFFSET`] keep acne away.
pub const BIAS: f32 = 0.0001;
pub const NORMAL_OFFSET: f32 = 0.06;
/// Shadow casters this far (m) either side of the box centre along the light
/// are in the map.
pub const DEPTH: f32 = 80.0;

/// Handles of the `fx.shadows.*` params, looked up once.
#[derive(Debug, Clone, Default)] #[cfg_attr(not(feature = "shadow-fit"), derive(Copy))]
pub struct ShadowParams {
    distance: Option<F32Param>,
    strength: Option<F32Param>,
    bias: Option<F32Param>,
    normal_offset: Option<F32Param>, #[cfg(feature = "shadow-fit")] fit: crate::shadow_fit::FitParams, #[cfg(feature = "shadow-fit")] fit_state: crate::shadow_fit::FitState,
}

impl ShadowParams {
    pub fn lookup(reg: &ParamRegistry) -> Self {
        Self {
            distance: reg.f32_handle("fx.shadows.distance"),
            strength: reg.f32_handle("fx.shadows.strength"),
            bias: reg.f32_handle("fx.shadows.bias"),
            normal_offset: reg.f32_handle("fx.shadows.normal_offset"), #[cfg(feature = "shadow-fit")] fit: crate::shadow_fit::FitParams::lookup(reg), #[cfg(feature = "shadow-fit")] fit_state: Default::default(),
        }
    }
}

/// This frame's shadow map, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowPlan {
    /// Map size in texels (square).
    pub res: u32,
    pub pcf_radius: u32,
    pub light_vp: Mat4,
    pub strength: f32,
    pub bias: f32,
    pub normal_offset: f32,
}

/// The plan for this frame: `None` when the `shadows` effect is off (or not
/// in the build) or the tier has no map. `max_dim` caps the map size.
pub fn plan(
    reg: &ParamRegistry,
    p: &ShadowParams,
    fx: Option<&EffectsStatus>,
    tiers: &Tiers,
    rw: &RenderWorld,
    light_dir: [f32; 3],
    max_dim: u32,
) -> Option<ShadowPlan> {
    if !fx.is_some_and(|f| f.is_on("shadows")) {
        return None;
    }
    let tier = tiers.active();
    let res = tier.shadow_map?.clamp(256, max_dim.max(256));
    let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
    let distance = v(p.distance, DISTANCE).max(1.0);
    let (centre, spread) = focus(rw);
    // Split-screen: grow the box to hold both players (up to 3x).
    let half = (distance + spread).min(distance * 3.0); #[cfg(feature = "shadow-fit")] let fit = crate::shadow_fit::Fit::new(reg, &p.fit, &p.fit_state, rw, half, crate::shadow_fit::atlas_res(res, tier, max_dim)); #[cfg(feature = "shadow-fit")] let half = fit.half;
    Some(ShadowPlan {
        res,
        pcf_radius: tier.shadow_pcf.unwrap_or(0).min(2),
        light_vp: shaders::light_view_proj(centre, half, DEPTH.max(half), light_dir, res),
        strength: v(p.strength, STRENGTH).clamp(0.0, 1.0),
        #[cfg(not(feature = "shadow-fit"))] bias: v(p.bias, BIAS), #[cfg(feature = "shadow-fit")] bias: fit.bias(v(p.bias, BIAS)),
        #[cfg(not(feature = "shadow-fit"))] normal_offset: v(p.normal_offset, NORMAL_OFFSET), #[cfg(feature = "shadow-fit")] normal_offset: fit.normal_offset(v(p.normal_offset, NORMAL_OFFSET)),
    })
}

/// Where the shadow box centres: the camera's target, or the middle of the
/// split-screen views' targets, plus the largest distance from there to a
/// view target.
pub fn focus(rw: &RenderWorld) -> ([f32; 3], f32) {
    if rw.views.is_empty() {
        return (rw.camera.target, 0.0);
    }
    let n = rw.views.len() as f32;
    let mut c = [0.0; 3];
    for v in &rw.views {
        for (a, b) in c.iter_mut().zip(v.camera.target) {
            *a += b / n;
        }
    }
    let spread = rw
        .views
        .iter()
        .map(|v| {
            let t = v.camera.target;
            ((t[0] - c[0]).powi(2) + (t[2] - c[2]).powi(2)).sqrt()
        })
        .fold(0.0, f32::max);
    (c, spread)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::View;
    use evani_core::camera::CameraPose;
    use evani_core::effects::{EffectInfo, EffectState, EffectStatus, StyleClasses};

    static INFO: EffectInfo = EffectInfo {
        name: "shadows",
        cost: 2,
        value: 0.9,
        styles: StyleClasses::THREE_D,
        vr_comfort_off: false,
        description: "",
    };

    fn status(state: EffectState) -> EffectsStatus {
        EffectsStatus {
            effects: vec![EffectStatus {
                info: &INFO,
                enabled: None,
                weight: None,
                passes: &[],
                state,
            }],
        }
    }

    #[test]
    fn plan_needs_the_effect_and_a_tier_map() {
        let reg = ParamRegistry::default();
        let p = ShadowParams::lookup(&reg);
        let mut tiers = Tiers::builtin().unwrap();
        let rw = RenderWorld::default();
        let on = status(EffectState::On);
        assert!(tiers.set_active("medium"));
        let plan = plan(&reg, &p, Some(&on), &tiers, &rw, [0.5, 1.0, 0.3], 8192).unwrap();
        assert_eq!((plan.res, plan.pcf_radius), (1024, 0));
        assert_eq!(plan.strength, STRENGTH);
        assert!(super::plan(&reg, &p, Some(&status(EffectState::OffByBudget)), &tiers, &rw, [0.0, 1.0, 0.0], 8192).is_none());
        assert!(super::plan(&reg, &p, None, &tiers, &rw, [0.0, 1.0, 0.0], 8192).is_none());
        assert!(tiers.set_active("ultra"));
        let plan = super::plan(&reg, &p, Some(&on), &tiers, &rw, [0.5, 1.0, 0.3], 2048).unwrap();
        assert_eq!((plan.res, plan.pcf_radius), (2048, 1), "capped by the device");
        assert!(tiers.set_active("low"));
        assert!(super::plan(&reg, &p, Some(&on), &tiers, &rw, [0.5, 1.0, 0.3], 8192).is_none());
    }

    #[test]
    fn default_bias_is_a_couple_of_centimetres_in_the_world() {
        // The lookup bias is in normalised light depth over a box 2 x DEPTH
        // deep. More than a few cm and thin casters near a surface (a 5 cm
        // ring 20 cm over the ground) lose part of their shadow.
        let metres = BIAS * 2.0 * DEPTH;
        assert!(metres > 0.0 && metres <= 0.03, "{metres} m");
    }

    #[test]
    fn focus_is_the_camera_target_or_the_views_middle() {
        let mut rw = RenderWorld::default();
        rw.camera.target = [3.0, 0.0, 4.0];
        assert_eq!(focus(&rw), ([3.0, 0.0, 4.0], 0.0));
        let cam = |x: f32| CameraPose {
            target: [x, 0.0, 0.0],
            ..CameraPose::default()
        };
        rw.views.push(View::new(cam(-10.0), [0.0, 0.0, 1.0, 0.5]));
        rw.views.push(View::new(cam(10.0), [0.0, 0.5, 1.0, 0.5]));
        let (c, spread) = focus(&rw);
        assert_eq!(c, [0.0; 3]);
        assert_eq!(spread, 10.0);
    }
}
