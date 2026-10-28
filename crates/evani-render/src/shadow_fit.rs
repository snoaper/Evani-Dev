//! Shadow fit (feature `shadow-fit`, implies `shadows`): a shadow box sized
//! for the camera, for scenes seen from far away, and a bias matched to the
//! box's texel size. Pure data beside [`crate::shadow`], tested natively.
//!
//! The plain box is `fx.shadows.distance` (26 m by default) either side of the
//! camera's target. That suits scenes a few metres across; a camera 100 m
//! back from its target sees shadow-casting things well outside it, and they
//! pop in and out of shadow at the box's edge. Raising the distance by hand
//! makes each texel bigger, so the bias and normal offset tuned for 26 m are
//! then too small and acne and flicker appear; they have to be raised to
//! match.
//!
//! - `fx.shadows.fit` (`fixed` | `view`, default `fixed`): `fixed` is the
//!   plain box, unchanged. `view` makes the half-width at least the camera's
//!   distance to its target plus `fx.shadows.view_depth`, so everything from
//!   the camera to beyond the target is in the box at any zoom. The half-width
//!   is rounded up to a whole `fx.shadows.fit_step`, so zooming changes the
//!   box (and its texel grid) only once per step, not every frame, and is
//!   capped at `fx.shadows.fit_max`. It never shrinks the plain box.
//! - `fx.shadows.auto_bias` (default off): the normal offset becomes at least
//!   [`OFFSET_TEXELS`] texels and the depth bias at least [`BIAS_TEXELS`]
//!   texels (in the box's normalised depth), so they keep pace with the texel
//!   size; the `fx.shadows.bias` / `normal_offset` values stay the minimum.
//!
//! The box still moves in whole texels ([`crate::shaders::light_view_proj`]),
//! so with a steady size its edges do not crawl as the camera moves. Games
//! without the feature compile none of this and are byte for byte what they
//! were; with it and both switches at their defaults the plan is the plain one.

use core::cell::Cell;

use evani_core::params::{BoolParam, EnumParam, F32Param, ParamDef, ParamError, ParamRegistry};
use evani_core::plugin::PluginContext;

use crate::world::RenderWorld;

pub const FIT: &str = "fx.shadows.fit";
pub const VIEW_DEPTH: &str = "fx.shadows.view_depth";
pub const FIT_STEP: &str = "fx.shadows.fit_step";
pub const FIT_MAX: &str = "fx.shadows.fit_max";
pub const AUTO_BIAS: &str = "fx.shadows.auto_bias";

/// `fx.shadows.fit` options, in index order.
pub const FIT_MODES: [&str; 2] = ["fixed", "view"];
pub const VIEW_DEPTH_DEFAULT: f32 = 30.0;
pub const FIT_STEP_DEFAULT: f32 = 8.0;
pub const FIT_MAX_DEFAULT: f32 = 400.0;
/// Normal offset of `auto_bias`, in texels of the shadow map.
pub const OFFSET_TEXELS: f32 = 1.5;
/// Depth bias of `auto_bias`, in texels (converted to normalised depth).
pub const BIAS_TEXELS: f32 = 0.5;

/// Register the `fx.shadows.fit*` and `fx.shadows.auto_bias` params.
pub fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    let f = |name: &str, d: f32, min: f32, max: f32, step: f32, desc: &str| {
        ParamDef::f32(name, d).range(min, max).step(step).unit("m").describe(desc).owner("fx.shadows").gpu()
    };
    ctx.param_enum(
        ParamDef::enumeration(FIT, &FIT_MODES, 0)
            .describe("Shadow box size: `fixed` = fx.shadows.distance around the camera's target; `view` = at least the camera's distance to its target plus fx.shadows.view_depth (for scenes seen from far away)")
            .owner("fx.shadows"),
    )?;
    ctx.param_f32(f(VIEW_DEPTH, VIEW_DEPTH_DEFAULT, 0.0, 500.0, 1.0, "With fit `view`: how far past the camera's distance to its target the shadow box reaches"))?;
    ctx.param_f32(f(FIT_STEP, FIT_STEP_DEFAULT, 0.5, 100.0, 0.5, "With fit `view`: the box's half-width is rounded up to a multiple of this, so zooming re-sizes it (and its texel grid) only once per step"))?;
    ctx.param_f32(f(FIT_MAX, FIT_MAX_DEFAULT, 4.0, 2000.0, 1.0, "With fit `view`: the largest half-width of the shadow box"))?;
    ctx.param_bool(
        ParamDef::bool(AUTO_BIAS, false)
            .describe("Match the normal offset and depth bias to the shadow map's texel size (fx.shadows.bias and normal_offset stay the minimum)")
            .owner("fx.shadows"),
    )?;
    Ok(())
}

/// Handles of the fit params, looked up once (all `None` before the
/// renderer registers them, which leaves the plain plan).
#[derive(Debug, Clone, Copy, Default)]
pub struct FitParams {
    fit: Option<EnumParam>,
    view_depth: Option<F32Param>,
    step: Option<F32Param>,
    max: Option<F32Param>,
    auto_bias: Option<BoolParam>,
}

impl FitParams {
    pub fn lookup(reg: &ParamRegistry) -> Self {
        Self {
            fit: reg.id(FIT).map(EnumParam),
            view_depth: reg.f32_handle(VIEW_DEPTH),
            step: reg.f32_handle(FIT_STEP),
            max: reg.f32_handle(FIT_MAX),
            auto_bias: reg.id(AUTO_BIAS).map(BoolParam),
        }
    }
}

/// What the fit changes in a frame's plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Half-width of the shadow box (m).
    pub half: f32,
    /// World size of one shadow-map texel (m); 0 when `auto_bias` is off.
    texel: f32,
    /// Depth of the box either side of its centre (m).
    depth: f32,
}

impl Fit {
    /// The fit for this frame: `half` is the plain half-width, `res` the
    /// map size, `depth` the plain depth rule (`DEPTH.max(half)`) applied to
    /// the fitted half.
    /// With `view`, the half-width is held from frame to frame in `state`
    /// ([`held_half`]), so a camera hovering at a step edge does not flip the
    /// box (and its texel grid) between two sizes; `res` is the size of the
    /// map the far box is drawn into ([`atlas_res`]).
    pub fn new(reg: &ParamRegistry, p: &FitParams, state: &FitState, rw: &RenderWorld, half: f32, res: u32) -> Self {
        let view = p.fit.is_some_and(|h| reg.enum_index(h) == 1);
        let half = if view {
            let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
            let (centre, _) = crate::shadow::focus(rw);
            let held = held_half(state.last.get(), half, camera_reach(rw, centre), v(p.view_depth, VIEW_DEPTH_DEFAULT), v(p.step, FIT_STEP_DEFAULT), v(p.max, FIT_MAX_DEFAULT));
            state.last.set(held);
            held
        } else {
            state.last.set(0.0);
            half
        };
        let auto = p.auto_bias.is_some_and(|h| reg.bool(h));
        Self {
            half,
            texel: if auto { texel_size(half, res) } else { 0.0 },
            depth: crate::shadow::DEPTH.max(half),
        }
    }

    /// The normal offset: at least [`OFFSET_TEXELS`] texels with `auto_bias`.
    pub fn normal_offset(&self, set: f32) -> f32 {
        set.max(OFFSET_TEXELS * self.texel)
    }

    /// The depth bias (normalised light depth over the box's 2 x depth): at
    /// least [`BIAS_TEXELS`] texels with `auto_bias`.
    pub fn bias(&self, set: f32) -> f32 {
        set.max(BIAS_TEXELS * self.texel / (2.0 * self.depth))
    }
}

/// What the fit remembers between frames: the half-width of the last `view`
/// fit (0 = none yet, or `fixed`). One per renderer, in its shadow params, so
/// renderers sharing a thread (on wasm all do: Look Lab views, off-screen
/// sweep or A/B renders, split views drawn by their own renderer) never share
/// or inherit a box size.
#[derive(Debug, Clone, Default)]
pub struct FitState {
    last: Cell<f32>,
}

/// How far the shadow box must reach from its `centre` to hold every view:
/// over the views (split-screen; the box is centred between the players) or
/// the main camera, the largest distance from the centre to that camera's
/// target plus the camera's eye-to-target distance.
pub fn camera_reach(rw: &RenderWorld, centre: [f32; 3]) -> f32 {
    let dist = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt();
    let reach = |c: &evani_core::camera::CameraPose| dist(c.target, centre) + dist(c.eye, c.target);
    if rw.views.is_empty() {
        reach(&rw.camera)
    } else {
        rw.views.iter().map(|v| reach(&v.camera)).fold(0.0, f32::max)
    }
}

/// The `view` half-width: the camera's reach plus `view_depth`, rounded up
/// to a whole `step`, never below the plain `half`, capped at `max` (or the
/// plain half, if that is bigger).
pub fn fitted_half(half: f32, reach: f32, view_depth: f32, step: f32, max: f32) -> f32 {
    let want = reach.max(0.0) + view_depth.max(0.0);
    let step = step.max(0.5);
    let snapped = (want / step).ceil() * step;
    snapped.min(max).max(half)
}

/// [`fitted_half`] held against the last frame's half-width `prev` (0 =
/// none): the box grows as soon as the fit needs more, but shrinks only once
/// the wanted size (reach + view depth) is more than half a step below the
/// next step down (`prev - step`), so a camera hovering at a step edge keeps
/// one size. Still capped at `max` and never below the plain `half`.
pub fn held_half(prev: f32, half: f32, reach: f32, view_depth: f32, step: f32, max: f32) -> f32 {
    let fitted = fitted_half(half, reach, view_depth, step, max);
    if !(prev.is_finite() && prev > 0.0) || fitted >= prev {
        return fitted;
    }
    let want = reach.max(0.0) + view_depth.max(0.0);
    if want < prev - 1.5 * step.max(0.5) {
        fitted
    } else {
        prev.min(max).max(half)
    }
}

/// The size of the map the far shadow box is drawn into, for the active
/// tier: see [`atlas_res_for`].
pub fn atlas_res(res: u32, tier: &evani_core::tiers::TierPreset, max_dim: u32) -> u32 {
    #[cfg(feature = "shadow-cascades")]
    let cascades = tier.shadow_cascades;
    #[cfg(not(feature = "shadow-cascades"))]
    let cascades = {
        let _ = tier;
        0
    };
    atlas_res_for(res, cascades, max_dim)
}

/// With two cascades (a tier asking for 2 or more) the map is an atlas of two
/// squares of at most `max_dim / 2` each (`shadow_ext::split`, which draws
/// the far box into one of them), so the far box's texels are those of that
/// square; else the map is `res`.
pub fn atlas_res_for(res: u32, cascades: u32, max_dim: u32) -> u32 {
    let half_map = res.min(max_dim / 2);
    if cascades >= 2 && half_map >= 256 {
        half_map
    } else {
        res
    }
}

/// World size (m) of one texel of a `res`-texel map over a box `half` wide
/// either side (the rule [`crate::shaders::light_view_proj`] snaps to).
pub fn texel_size(half: f32, res: u32) -> f32 {
    2.0 * half.max(0.5) / res.max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shaders;
    use crate::shadow::{self, ShadowParams};
    use evani_core::camera::CameraPose;
    use evani_core::effects::{EffectInfo, EffectState, EffectStatus, EffectsStatus, StyleClasses};
    use evani_core::params::ParamValue;
    use evani_core::tiers::Tiers;

    static INFO: EffectInfo = EffectInfo {
        name: "shadows",
        cost: 2,
        value: 0.9,
        styles: StyleClasses::THREE_D,
        vr_comfort_off: false,
        description: "",
    };

    fn on() -> EffectsStatus {
        EffectsStatus {
            effects: vec![EffectStatus {
                info: &INFO,
                enabled: None,
                weight: None,
                passes: &[],
                state: EffectState::On,
            }],
        }
    }

    fn registry() -> ParamRegistry {
        let mut app = evani_core::App::new("t").with_module(crate::RenderModule::new());
        app.run_headless(1, 16.0);
        std::mem::take(&mut *app.world.res_mut::<ParamRegistry>())
    }

    fn camera_at(back: f32) -> RenderWorld {
        RenderWorld {
            camera: CameraPose {
                eye: [0.0, back * 0.6, back * 0.8],
                target: [0.0; 3],
                ..CameraPose::default()
            },
            ..RenderWorld::default()
        }
    }

    /// A frame planned by a new renderer (no held box size).
    fn plan(reg: &ParamRegistry, rw: &RenderWorld) -> shadow::ShadowPlan {
        plan_by(reg, &ShadowParams::lookup(reg), rw)
    }

    /// A frame planned by the renderer whose shadow params are `p`.
    fn plan_by(reg: &ParamRegistry, p: &ShadowParams, rw: &RenderWorld) -> shadow::ShadowPlan {
        let mut tiers = Tiers::builtin().unwrap();
        assert!(tiers.set_active("high"));
        shadow::plan(reg, p, Some(&on()), &tiers, rw, [0.4, 1.0, 0.3], 8192).unwrap()
    }

    #[test]
    fn registered_by_the_renderer_with_the_plain_defaults() {
        let reg = registry();
        let p = FitParams::lookup(&reg);
        assert!(p.fit.is_some() && p.view_depth.is_some() && p.step.is_some() && p.max.is_some() && p.auto_bias.is_some());
        assert_eq!(reg.enum_index(p.fit.unwrap()), 0, "fixed by default");
        assert!(!reg.bool(p.auto_bias.unwrap()), "auto bias off by default");
    }

    #[test]
    fn defaults_leave_the_plan_as_it_was() {
        // The same frame planned with the fit params registered (their
        // defaults) and with no fit params at all is the same plan.
        let reg = registry();
        let bare = ParamRegistry::default();
        for back in [3.0, 20.0, 110.0, 600.0] {
            let rw = camera_at(back);
            let a = plan(&reg, &rw);
            let b = plan(&bare, &rw);
            assert_eq!(a, b, "camera {back} m back");
            assert_eq!((a.bias, a.normal_offset), (shadow::BIAS, shadow::NORMAL_OFFSET));
        }
    }

    #[test]
    fn view_fit_covers_the_camera_to_target_segment_at_any_zoom() {
        for back in [0.0f32, 1.0, 12.0, 26.0, 57.3, 110.0, 140.0, 333.0] {
            let half = fitted_half(shadow::DISTANCE, back, VIEW_DEPTH_DEFAULT, FIT_STEP_DEFAULT, FIT_MAX_DEFAULT);
            assert!(half >= back + VIEW_DEPTH_DEFAULT && half < back + VIEW_DEPTH_DEFAULT + FIT_STEP_DEFAULT, "{back}: {half}");
            assert!(half >= shadow::DISTANCE, "never smaller than the plain box");
            assert_eq!(half % FIT_STEP_DEFAULT, 0.0, "a whole step");
        }
        // Capped, but never below the plain box.
        assert_eq!(fitted_half(26.0, 5000.0, 30.0, 8.0, 400.0), 400.0);
        assert_eq!(fitted_half(500.0, 5000.0, 30.0, 8.0, 400.0), 500.0);
    }

    #[test]
    fn view_fit_through_the_plan_matches_a_hand_tuned_far_camera() {
        let mut reg = registry();
        reg.set_by_name(FIT, ParamValue::Enum(1)).unwrap();
        // A camera 110 m from its target: 110 + 30, rounded up to 8 m steps.
        let rw = camera_at(110.0);
        let p = plan(&reg, &rw);
        let fit = Fit::new(&reg, &FitParams::lookup(&reg), &FitState::default(), &rw, shadow::DISTANCE, p.res);
        assert_eq!(fit.half, 144.0);
        assert_eq!(p.light_vp, shaders::light_view_proj(rw.camera.target, 144.0, 144.0, [0.4, 1.0, 0.3], p.res));
        // Close up, `view` still reaches view_depth past the target: 4 + 30
        // rounds up to 40 m, more than the plain 26 m.
        let near = camera_at(4.0);
        assert_eq!(Fit::new(&reg, &FitParams::lookup(&reg), &FitState::default(), &near, shadow::DISTANCE, p.res).half, 40.0);
        // With a short view depth the fit wants less than the plain box (4 +
        // 10 rounds to 16 m), and the plain box stays: the same plan as with
        // no fit params at all.
        reg.set_by_name(VIEW_DEPTH, ParamValue::F32(10.0)).unwrap();
        assert_eq!(plan(&reg, &near), plan(&ParamRegistry::default(), &near));
    }

    #[test]
    fn small_zoom_changes_keep_the_box_and_its_texel_grid() {
        // Zooming within one step keeps the same half-width, so the box is
        // snapped to the same texel grid: moving the target by less than half
        // a texel gives the same projection.
        let a = fitted_half(26.0, 101.0, 30.0, 8.0, 400.0);
        let b = fitted_half(26.0, 105.5, 30.0, 8.0, 400.0);
        assert_eq!(a, b);
        let res = 2048;
        let texel = texel_size(a, res);
        let l = [0.4, 1.0, 0.3];
        let base = shaders::light_view_proj([0.0; 3], a, a, l, res);
        // The snap is in light space; a nudge along the light (y here mostly
        // projects off the grid axes) is not a crawl. Nudge across.
        for d in [0.1 * texel, 0.3 * texel, 0.45 * texel] {
            assert_eq!(shaders::light_view_proj([d, 0.0, 0.0], a, a, l, res)[12..14], base[12..14], "{d}");
        }
        // A whole texel moves the box by exactly one texel's worth in clip space.
        let one = shaders::light_view_proj([texel * 20.0, 0.0, 0.0], a, a, l, res);
        let shift = (one[12] - base[12]).abs().max((one[13] - base[13]).abs());
        let clip_texel = 2.0 / res as f32;
        assert!((shift / clip_texel - (shift / clip_texel).round()).abs() < 1e-2, "moves in whole texels: {shift}");
    }

    #[test]
    fn auto_bias_follows_the_texel_size() {
        let mut reg = registry();
        reg.set_by_name(FIT, ParamValue::Enum(1)).unwrap();
        reg.set_by_name(AUTO_BIAS, ParamValue::Bool(true)).unwrap();
        let rw = camera_at(110.0);
        let p = plan(&reg, &rw);
        let half = 144.0;
        let texel = texel_size(half, p.res);
        assert!((p.normal_offset - OFFSET_TEXELS * texel).abs() < 1e-6, "{} vs {}", p.normal_offset, texel);
        assert!((p.bias - BIAS_TEXELS * texel / (2.0 * half)).abs() < 1e-9, "{}", p.bias);
        // In world terms the depth bias is half a texel, the offset one and a half.
        assert!((p.bias * 2.0 * half / texel - BIAS_TEXELS).abs() < 1e-3);
        // Small boxes keep the set values (they are the minimum).
        let near = camera_at(2.0);
        let q = plan(&reg, &near);
        assert_eq!((q.bias, q.normal_offset), (shadow::BIAS, shadow::NORMAL_OFFSET));
        // Off: the set values, whatever the box.
        reg.set_by_name(AUTO_BIAS, ParamValue::Bool(false)).unwrap();
        let r = plan(&reg, &rw);
        assert_eq!((r.bias, r.normal_offset), (shadow::BIAS, shadow::NORMAL_OFFSET));
    }

    #[test]
    fn split_screen_reach_is_the_farthest_view() {
        let mut rw = RenderWorld::default();
        let cam = |back: f32| CameraPose {
            eye: [0.0, 0.0, back],
            target: [0.0; 3],
            ..CameraPose::default()
        };
        rw.views.push(crate::world::View::new(cam(10.0), [0.0, 0.0, 1.0, 0.5]));
        rw.views.push(crate::world::View::new(cam(70.0), [0.0, 0.5, 1.0, 0.5]));
        assert_eq!(camera_reach(&rw, [0.0; 3]), 70.0);
        // The box is centred between the players: a player whose target is
        // 40 m from that centre needs 40 m more.
        let far_off = |x: f32, back: f32| CameraPose {
            eye: [x, 0.0, back],
            target: [x, 0.0, 0.0],
            ..CameraPose::default()
        };
        rw.views.clear();
        rw.views.push(crate::world::View::new(far_off(-40.0, 10.0), [0.0, 0.0, 1.0, 0.5]));
        rw.views.push(crate::world::View::new(far_off(40.0, 20.0), [0.0, 0.5, 1.0, 0.5]));
        let (centre, _) = shadow::focus(&rw);
        assert_eq!(camera_reach(&rw, centre), 60.0);
        // The main camera's box is centred on its own target.
        let one = camera_at(50.0);
        assert!((camera_reach(&one, one.camera.target) - 50.0).abs() < 1e-4);
    }

    #[test]
    fn a_camera_wobbling_at_a_step_edge_keeps_one_box() {
        // 106 m back + 30 m view depth = 136 m, exactly a step edge (8 m steps).
        assert_eq!(fitted_half(26.0, 105.99, 30.0, 8.0, 400.0), 136.0);
        assert_eq!(fitted_half(26.0, 106.01, 30.0, 8.0, 400.0), 144.0);
        let mut prev = 0.0;
        let mut seen = Vec::new();
        for i in 0..20 {
            let back = if i % 2 == 0 { 105.99 } else { 106.01 };
            prev = held_half(prev, 26.0, back, 30.0, 8.0, 400.0);
            seen.push(prev);
        }
        // Grows once (136 -> 144), then stays put while it wobbles.
        assert_eq!(seen[0], 136.0);
        assert!(seen[1..].iter().all(|&h| h == 144.0), "{seen:?}");
        // It shrinks only once the wanted size is over half a step below the
        // next step down (144 - 8 = 136, so below 132).
        assert_eq!(held_half(144.0, 26.0, 102.5, 30.0, 8.0, 400.0), 144.0); // wants 132.5
        assert_eq!(held_half(144.0, 26.0, 101.5, 30.0, 8.0, 400.0), 136.0); // wants 131.5
        assert_eq!(held_half(144.0, 26.0, 20.0, 30.0, 8.0, 400.0), 56.0);
        // Never above the cap or below the plain box, whatever was held.
        assert_eq!(held_half(500.0, 26.0, 390.0, 30.0, 8.0, 400.0), 400.0);
        assert_eq!(held_half(30.0, 26.0, 0.0, 0.0, 8.0, 400.0), 26.0);
        assert_eq!(held_half(f32::NAN, 26.0, 106.0, 30.0, 8.0, 400.0), 136.0);

        // Through one renderer's plan, frame after frame: the same light
        // matrix every frame.
        let mut reg = registry();
        reg.set_by_name(FIT, ParamValue::Enum(1)).unwrap();
        let p = ShadowParams::lookup(&reg);
        let mats: Vec<_> = (0..10).map(|i| plan_by(&reg, &p, &camera_at(if i % 2 == 0 { 106.01 } else { 105.99 })).light_vp).collect();
        assert!(mats.windows(2).all(|w| w[0] == w[1]), "the box changed while the camera wobbled 2 cm");
        // `fixed` forgets the held size.
        reg.set_by_name(FIT, ParamValue::Enum(0)).unwrap();
        let _ = plan_by(&reg, &p, &camera_at(106.01));
        reg.set_by_name(FIT, ParamValue::Enum(1)).unwrap();
        assert_eq!(plan_by(&reg, &p, &camera_at(105.99)).light_vp, plan(&reg, &camera_at(105.99)).light_vp, "a fresh start after `fixed`");
    }

    #[test]
    fn two_renderers_keep_their_own_box_size() {
        // Two fit states on one thread (two renderers: a game view and a Look
        // Lab or off-screen view): what one holds never reaches the other.
        let mut reg = registry();
        reg.set_by_name(FIT, ParamValue::Enum(1)).unwrap();
        let params = FitParams::lookup(&reg);
        let (far, near) = (FitState::default(), FitState::default());
        let fit = |s: &FitState, back: f32| Fit::new(&reg, &params, s, &camera_at(back), shadow::DISTANCE, 2048).half;
        // The far renderer grows to 144 m (110 + 30, 8 m steps) and holds it
        // while its camera wobbles at the step edge.
        assert_eq!(fit(&far, 110.0), 144.0);
        assert_eq!(fit(&far, 106.01), 144.0);
        // The near renderer, planned in between, gets its own size, not the
        // far one's 144 m.
        assert_eq!(fit(&near, 4.0), 40.0);
        assert_eq!(fit(&far, 105.99), 144.0, "still held after the other renderer's frame");
        assert_eq!(fit(&near, 4.0), 40.0);
        // A third renderer starting at the edge gets the plain fitted size, as
        // a first frame: no history from the others.
        assert_eq!(fit(&FitState::default(), 105.99), 136.0);
        // Through the whole plan: two shadow params (two renderers) side by
        // side give each its own light matrix, the same as each alone.
        let (a, b) = (ShadowParams::lookup(&reg), ShadowParams::lookup(&reg));
        for _ in 0..3 {
            assert_eq!(plan_by(&reg, &a, &camera_at(110.0)).light_vp, plan(&reg, &camera_at(110.0)).light_vp);
            assert_eq!(plan_by(&reg, &b, &camera_at(4.0)).light_vp, plan(&reg, &camera_at(4.0)).light_vp);
        }
    }

    #[test]
    fn the_cascade_atlas_halves_the_far_box_texels() {
        assert_eq!(atlas_res_for(4096, 2, 8192), 4096);
        assert_eq!(atlas_res_for(4096, 2, 4096), 2048);
        assert_eq!(atlas_res_for(4096, 1, 4096), 4096);
        assert_eq!(atlas_res_for(4096, 0, 4096), 4096);
        // Too small for an atlas: shadow_ext keeps one map.
        assert_eq!(atlas_res_for(1024, 2, 300), 1024);
    }
}
