//! Game passes, a working example (evani-render `game-passes`; docs/GAME_PASSES.md).
//! Built into the kit only with its `game-pass-demo` feature, so the
//! sandbox's download is unchanged; the kit's tests always check it.
//!
//! * `example.noise`: a compute pass that fills a 32^3 noise volume once
//!   (and again when its parameter changes): a LUT made at start, no download.
//! * `example.glow`: a half-resolution afterglow with a history copy: each
//!   frame keeps the bright parts of the scene and a fading copy of last
//!   frame's glow.
//! * `example.haze`: a full-screen pass on `hdr` before the tone map that
//!   reads the scene (a copy), the depth, the noise volume and the glow, and
//!   shimmers the distance like heat haze. Off below the Medium tier.
//! * `example.reticle`: a HUD overlay ring on `ldr` over everything.
//!
//! Every tunable is a live parameter: `kit.example.haze.strength`,
//! `kit.example.glow.persistence`, `kit.example.reticle.color`...

use evani_core::plugin::{PluginContext, PluginError};
use evani_render::game_pass::{self, Format, GamePass, GameTarget, PassBlend, PassParam, Stage};

/// The noise volume (written once by `example.noise`).
pub const NOISE: &str = "example.noise";
/// The afterglow (half resolution, keeps last frame).
pub const GLOW: &str = "example.glow";

const NOISE_WGSL: &str = "
fn hash3(p: vec3<u32>) -> f32 {
    var h = p.x * 374761393u + p.y * 668265263u + p.z * 1442695041u + u32(param_seed());
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32(h & 0xffffu) / 65535.0;
}
fn pass_main(id: vec3<u32>) -> vec4<f32> {
    return vec4<f32>(hash3(id), hash3(id + vec3<u32>(17u, 31u, 47u)), hash3(id + vec3<u32>(59u, 3u, 11u)), 1.0);
}
";

const GLOW_WGSL: &str = "
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let c = textureSample(scene, samp, uv).rgb;
    let bright = max(c - vec3<f32>(param_threshold()), vec3<f32>(0.0));
    let before = textureSample(prev, samp, uv).rgb;
    return vec4<f32>(max(bright, before * param_persistence()), 1.0);
}
";

const HAZE_WGSL: &str = "
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(px), 0);
    // Reversed Z: small values are far away; 0 is the sky.
    let far = select(1.0 - clamp(d * 8.0, 0.0, 1.0), 0.0, d <= 0.0);
    let n = textureSample(noise, samp_wrap, vec3<f32>(uv * 3.0, frame.camera.w * param_speed())).xy - vec2<f32>(0.5);
    let offset = n * param_distortion() * far * frame.size.zw * 8.0;
    let c = textureSample(scene, samp, uv + offset).rgb;
    let g = textureSample(glow, samp, uv).rgb;
    return vec4<f32>(c + g * param_strength() * param_tint(), 1.0);
}
";

const RETICLE_WGSL: &str = "
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let r = length(px - frame.size.xy * 0.5);
    let ring = 1.0 - smoothstep(0.0, 1.5, abs(r - param_radius()));
    return vec4<f32>(param_color(), ring * param_opacity());
}
";

/// Register the example targets and passes (call from the kit's `build`,
/// with the render module added first).
pub fn register(kit: &mut PluginContext) -> Result<(), PluginError> {
    game_pass::add_target(kit, GameTarget::volume(NOISE, Format::Rgba8Unorm, [32, 32, 32]))?;
    game_pass::add_target(kit, GameTarget::screen(GLOW, Format::Rgba16Float, 0.5).history().tier_scale("lite", 0.25).tier_scale("low", 0.25))?;
    game_pass::add(
        kit,
        GamePass::compute(NOISE, NOISE_WGSL)
            .writes(NOISE)
            .once()
            .param(PassParam::f32("seed", 1.0, 0.0, 1000.0).describe("Noise seed (changing it remakes the volume)")),
    )?;
    game_pass::add(
        kit,
        GamePass::fullscreen(GLOW, GLOW_WGSL)
            .at(Stage::BeforeTonemap)
            .reads("scene", "hdr")
            .reads_history("prev", GLOW)
            .writes(GLOW)
            .order(80)
            .param(PassParam::f32("threshold", 0.8, 0.0, 8.0).describe("Brightness the glow starts at"))
            .param(PassParam::f32("persistence", 0.85, 0.0, 0.99).describe("How much of last frame's glow stays")),
    )?;
    game_pass::add(
        kit,
        GamePass::fullscreen("example.haze", HAZE_WGSL)
            .at(Stage::BeforeTonemap)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .reads("noise", NOISE)
            .reads("glow", GLOW)
            .min_tier("medium")
            .param(PassParam::f32("strength", 0.6, 0.0, 4.0).describe("How bright the afterglow is"))
            .param(PassParam::f32("distortion", 1.0, 0.0, 8.0).describe("How far the haze shimmers, in pixels at full distance"))
            .param(PassParam::f32("speed", 0.2, 0.0, 4.0).describe("How fast the shimmer moves"))
            .param(PassParam::color("tint", [1.0, 0.75, 0.5]).describe("Afterglow colour")),
    )?;
    game_pass::add(
        kit,
        GamePass::fullscreen("example.reticle", RETICLE_WGSL)
            .at(Stage::Overlay)
            .blend(PassBlend::Alpha)
            .param(PassParam::f32("radius", 24.0, 2.0, 400.0).describe("Ring radius in pixels"))
            .param(PassParam::f32("opacity", 0.8, 0.0, 1.0).describe("Ring opacity"))
            .param(PassParam::color("color", [0.3, 1.0, 0.4]).describe("Ring colour")),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::ParamRegistry;
    use evani_core::plugin::KitPlugin;
    use evani_render::RenderGraph;

    /// The example kit with the demo passes on (what the feature builds).
    struct WithPasses;
    impl KitPlugin for WithPasses {
        fn name(&self) -> &'static str {
            "example"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            crate::ExampleKitPlugin.build(kit)?;
            register(kit)
        }
    }

    fn app() -> App {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(WithPasses);
        a.start();
        a
    }

    /// The gate for a kit with game passes: every pass is accepted, the
    /// frame orders them, and each whole WGSL module compiles.
    #[test]
    fn demo_passes_check_clean() {
        let a = app();
        let log = a.world.res::<evani_core::overlay::ErrorLog>();
        let errs: Vec<&str> = log.entries().map(|e| e.message.as_str()).filter(|m| m.contains("game pass") || m.contains("render graph")).collect();
        assert!(errs.is_empty(), "{errs:?}");
        drop(log);
        assert_eq!(game_pass::check(&a.world), Ok(4));
        let g = a.world.res::<RenderGraph>();
        let order: Vec<&str> = g.ordered().map(|p| p.name).collect();
        let at = |n: &str| order.iter().position(|&p| p == n).unwrap_or(usize::MAX);
        assert!(at("example.noise") < at("example.haze") && at("example.glow") < at("example.haze"), "{order:?}");
        assert!(at("example.haze") < at("tonemap") && at("tonemap") < at("example.reticle"), "{order:?}");
        assert!(at("example.reticle") < at("present"), "{order:?}");
    }

    /// Every tunable is a live parameter under the kit's name.
    #[test]
    fn demo_tunables_are_live_params() {
        let a = app();
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "kit.example.noise.seed",
            "kit.example.glow.persistence",
            "kit.example.haze.strength",
            "kit.example.haze.tint",
            "kit.example.haze.on",
            "kit.example.reticle.color",
        ] {
            assert!(reg.id(n).is_some(), "{n}");
        }
    }

    /// The haze is off below Medium; the glow still runs there.
    #[test]
    fn haze_follows_the_tier() {
        let mut a = app();
        assert!(a.world.res_mut::<evani_core::tiers::Tiers>().set_active("low"));
        game_pass::frame(&mut a.world);
        let g = a.world.res::<RenderGraph>();
        assert!(!g.pass("example.haze").is_some_and(|p| p.enabled));
        assert!(g.pass("example.glow").is_some_and(|p| p.enabled));
    }
}
