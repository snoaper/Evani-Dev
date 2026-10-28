//! Screen shake (feature `shake`): no render pass, params only. A trauma
//! value (0..1, raised by the game on impacts) decays every frame and
//! drives a smooth-noise camera offset and roll; the game applies them to
//! its camera while `EffectsStatus::is_on("shake")`. Cost 0, comfort-off in
//! VR. [`decay_trauma`] and [`sample`] are the shared maths, so every game
//! shakes the same way.

use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, RenderGraph};

use crate::{Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame};

pub static INFO: EffectInfo = EffectInfo {
    name: "shake",
    cost: 0,
    value: 0.4,
    // Catalog works_with: 2d, pseudo-3d, 3d.
    styles: StyleClasses::ALL,
    vr_comfort_off: true,
    description: "Camera shake from impacts",
};

/// Defaults (also used when a param is missing).
pub const AMOUNT: f32 = 0.35;
pub const ROLL_DEG: f32 = 3.0;
pub const DECAY: f32 = 1.6;
pub const FREQUENCY: f32 = 18.0;

#[derive(Debug, Clone, Copy)]
pub struct ShakeParams {
    pub amount: F32Param,
    pub roll_deg: F32Param,
    pub decay: F32Param,
    pub frequency: F32Param,
}

/// Current values of the shake params.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShakeSettings {
    pub amount: f32,
    pub roll_deg: f32,
    pub decay: f32,
    pub frequency: f32,
}

impl Default for ShakeSettings {
    fn default() -> Self {
        Self {
            amount: AMOUNT,
            roll_deg: ROLL_DEG,
            decay: DECAY,
            frequency: FREQUENCY,
        }
    }
}

impl ShakeSettings {
    /// Read the `fx.shake.*` params (defaults for any that are missing).
    pub fn from_registry(reg: &ParamRegistry) -> Self {
        let d = Self::default();
        let v = |n: &str, d: f32| reg.f32_handle(n).map_or(d, |h| reg.f32(h));
        Self {
            amount: v("fx.shake.amount", d.amount),
            roll_deg: v("fx.shake.roll_deg", d.roll_deg),
            decay: v("fx.shake.decay", d.decay),
            frequency: v("fx.shake.frequency", d.frequency),
        }
    }
}

/// Camera offset (metres, camera-local x/y/z) and roll (radians).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShakeSample {
    pub offset: [f32; 3],
    pub roll_rad: f32,
}

/// Trauma after `dt_s` seconds of decay (clamped to 0..1).
pub fn decay_trauma(trauma: f32, dt_s: f32, s: &ShakeSettings) -> f32 {
    (trauma - s.decay * dt_s.max(0.0)).clamp(0.0, 1.0)
}

/// Shake at `time_s` for `trauma` (0..1). Strength is trauma squared, so
/// small knocks stay subtle. `seed` separates cameras (one per player).
pub fn sample(trauma: f32, time_s: f32, seed: u32, s: &ShakeSettings) -> ShakeSample {
    let t = trauma.clamp(0.0, 1.0);
    let k = t * t;
    if k <= 0.0 {
        return ShakeSample::default();
    }
    let x = time_s * s.frequency;
    let n = |ch: u32| noise1(x, seed.wrapping_mul(4).wrapping_add(ch));
    ShakeSample {
        offset: [s.amount * k * n(0), s.amount * k * n(1), s.amount * k * 0.5 * n(2)],
        roll_rad: s.roll_deg.to_radians() * k * n(3),
    }
}

/// Smooth 1D gradient noise in about -1..1 (0 at whole numbers).
fn noise1(x: f32, seed: u32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let i = i as i32;
    let g0 = grad(i, seed);
    let g1 = grad(i.wrapping_add(1), seed);
    let u = f * f * (3.0 - 2.0 * f);
    let a = g0 * f;
    let b = g1 * (f - 1.0);
    (a + (b - a) * u) * 2.0
}

/// Hash a lattice point to a gradient in -1..1.
fn grad(i: i32, seed: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B9) ^ seed.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 32767.5 - 1.0
}

#[derive(Debug, Default)]
pub struct Shake {
    pub params: Option<ShakeParams>,
}

fn f(name: &str, d: f32, min: f32, max: f32, step: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(step)
        .unit(unit)
        .describe(desc)
        .owner("fx.shake")
        .bake(BakeTarget::Look)
}

impl Effect for Shake {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(ShakeParams {
            amount: ctx.param_f32(f(
                "fx.shake.amount",
                AMOUNT,
                0.0,
                2.0,
                0.01,
                "m",
                "Largest camera offset at full trauma.",
            ))?,
            roll_deg: ctx.param_f32(f(
                "fx.shake.roll_deg",
                ROLL_DEG,
                0.0,
                15.0,
                0.1,
                "deg",
                "Largest camera roll at full trauma.",
            ))?,
            decay: ctx.param_f32(f(
                "fx.shake.decay",
                DECAY,
                0.1,
                10.0,
                0.05,
                "/s",
                "Trauma lost per second.",
            ))?,
            frequency: ctx.param_f32(f(
                "fx.shake.frequency",
                FREQUENCY,
                1.0,
                60.0,
                0.5,
                "Hz",
                "How fast the camera jitters.",
            ))?,
        });
        Ok(())
    }

    /// No passes: the game moves its camera.
    fn passes(&self, _g: &mut RenderGraph) -> Result<(), GraphError> {
        Ok(())
    }
}

impl PostEffect for Shake {
    fn name(&self) -> &'static str {
        INFO.name
    }

    fn pass_names(&self) -> &'static [&'static str] {
        &[]
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &[]
    }

    fn write_uniforms(&self, _: &str, _: &ParamRegistry, _: &UniformFrame, _: &mut [f32]) -> usize {
        0
    }

    fn record(&self, _: &str, _: &ParamRegistry, _: &mut dyn PostRecorder) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FxModule, FxStatus};
    use evani_core::App;
    use evani_core::effects::BudgetStatus;
    use evani_core::tiers::Tiers;
    use evani_render::RenderModule;

    #[test]
    fn params_only_free_and_comfort_off() {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Shake::default()));
        assert!(a.world.res_mut::<Tiers>().set_active("low"));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<FxStatus>().is_on("shake"));
        assert_eq!(a.world.res::<BudgetStatus>().used, 0);
        assert_eq!(
            ShakeSettings::from_registry(&a.world.res::<ParamRegistry>()),
            ShakeSettings::default()
        );
        a.world.res_mut::<Tiers>().set_xr_session(true);
        assert_eq!(a.world.res::<Tiers>().active().id, "vr");
        a.run_headless(1, 16.0);
        assert!(!a.world.res::<FxStatus>().is_on("shake"));
    }

    #[test]
    fn trauma_decays_and_drives_a_bounded_smooth_shake() {
        let s = ShakeSettings::default();
        assert_eq!(decay_trauma(1.0, 0.5, &s), 1.0 - 0.8);
        assert_eq!(decay_trauma(0.1, 1.0, &s), 0.0);
        assert_eq!(sample(0.0, 1.0, 0, &s), ShakeSample::default());
        let mut prev = sample(1.0, 0.0, 1, &s);
        let mut moved = false;
        for i in 1..600 {
            let x = sample(1.0, i as f32 / 600.0, 1, &s);
            for c in 0..3 {
                assert!(x.offset[c].abs() <= s.amount * 1.01, "{x:?}");
                // Smooth: a 1/600 s step moves well under the full range.
                assert!((x.offset[c] - prev.offset[c]).abs() < s.amount * 0.25);
            }
            assert!(x.roll_rad.abs() <= s.roll_deg.to_radians() * 1.01);
            moved |= x.offset[0].abs() > 0.05;
            prev = x;
        }
        assert!(moved);
        // Same inputs, same shake; another seed shakes differently.
        assert_eq!(sample(0.7, 0.33, 2, &s), sample(0.7, 0.33, 2, &s));
        assert_ne!(sample(0.7, 0.33, 2, &s), sample(0.7, 0.33, 3, &s));
    }
}
