//! The volumetric clouds' scene (feature `clouds`, the Asset Studio clouds
//! session, column 11, 2026-11-06): the procedural library's cumulus
//! (`sky-volumetric-looking-clouds`, drawn by the library's own ray-marched
//! volume, crate evani-procedural-lib `clouds`) under its reference
//! picture's camera and light (a lone white cumulus filling nine tenths of
//! the frame's width against a flat mid-grey sky, its tallest heap right of
//! centre, lit softly from above and a little in front, its base a soft
//! darker grey), so it can be tuned and shot beside the reference.
//!
//! `kit.proc_field.clouds_view`: 0 the reference's view, 1 a close-up of the
//! tower's billows, 2 low under the base.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::RenderWorld;
use evani_render::game_pass::{self, GamePass, PassParam, Stage};

#[derive(Debug, Clone, Copy)]
pub struct Clouds {
    view: F32Param,
    sky: Vec3Param,
    sun: Vec3Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The backdrop: the reference's flat, neutral mid-grey sky.
const SKY: [f32; 3] = [0.315, 0.315, 0.317];

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.clouds_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the cloud's reference view, 1 a close-up of the tower's billows, 2 low under the base"),
    )?;
    let sky = kit.param_vec3(
        ParamDef::color("kit.proc_field.clouds_sky", SKY).describe("The flat sky behind the cloud"),
    )?;
    let sun = kit.param_vec3(
        ParamDef::vec3("kit.proc_field.clouds_sun", SUN)
            .describe("Towards the sun (the scene's light direction)"),
    )?;
    // Round 2 (round 1's review: a perfectly flat backdrop; the reference's
    // gentle studio grading helps its translucent edges read): the sky
    // behind the cloud is painted by one pass before the cloud's march.
    game_pass::add(
        kit,
        GamePass::fullscreen(BACKDROP_PASS, BACKDROP_WGSL)
            .at(Stage::AfterOpaque)
            .order(5)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(
                PassParam::f32("top", 0.34, 0.0, 2.0)
                    .describe("The backdrop's light at the top middle"),
            )
            .param(
                PassParam::f32("low", 0.36, 0.0, 2.0)
                    .describe("The backdrop's light at the bottom"),
            )
            .param(
                PassParam::f32("vignette", 0.236, 0.0, 1.0)
                    .describe("How much darker its upper corners are"),
            )
            .param(
                PassParam::f32("side", 0.05, -0.5, 0.5)
                    .describe("How much lighter its upper right is than its upper left"),
            )
            .param(
                PassParam::f32("mottle", 0.05, 0.0, 0.2)
                    .describe("A faint broad mottle (a fraction of its light)"),
            )
            .param(
                PassParam::color("tint", [1.0, 0.997, 0.985])
                    .describe("The backdrop's tint (the reference's grey is a touch warm)"),
            ),
    )?;
    kit.resource(Clouds {
        view,
        sky,
        sun,
        set_for: f32::NAN,
    });
    Ok(())
}

/// Set a param this build has (a colour or a vector, whichever it is).
fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    let Some(id) = reg.id(name) else { return };
    let v = match (reg.value(id), v) {
        (Some(ParamValue::Vec3(_)), ParamValue::Color(c)) => ParamValue::Vec3(c),
        (Some(ParamValue::Color(_)), ParamValue::Vec3(c)) => ParamValue::Color(c),
        (_, v) => v,
    };
    let _ = reg.set(id, v);
}

/// The backdrop pass (`kit.proc_field.clouds_backdrop.*`): where nothing
/// was drawn (the depth is the sky), a studio grey graded like the
/// reference's (measured: about 103 at the top left, 111 at the top right,
/// 114 at the top middle, 118 along the bottom; a touch warm), with a faint
/// broad mottle. Light values are the tone curve's input at the scene's
/// exposure.
const BACKDROP_PASS: &str = "proc_field.clouds_backdrop";
const BACKDROP_WGSL: &str = "
fn cb_h(c: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(c.x) * 747796405u) ^ (bitcast<u32>(c.y) * 2891336453u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967296.0;
}
fn cb_n(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(cb_h(i), cb_h(i + vec2<i32>(1, 0)), u.x);
    let b = mix(cb_h(i + vec2<i32>(0, 1)), cb_h(i + vec2<i32>(1, 1)), u.x);
    return mix(a, b, u.y);
}
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let c = textureSample(scene, samp, uv).rgb;
    let d = textureLoad(depth, vec2<i32>(px), 0);
    if (d > 0.0) { return vec4<f32>(c, 1.0); }
    let up = 1.0 - uv.y;
    let x = uv.x - 0.5;
    let g = mix(param_top(), param_low(), uv.y) - param_vignette() * x * x * up + param_side() * x * up;
    let n = cb_n(uv * vec2<f32>(3.0, 2.0)) * 0.6 + cb_n(uv * vec2<f32>(7.0, 5.0) + vec2<f32>(3.1, 1.7)) * 0.4;
    return vec4<f32>(param_tint() * g * (1.0 + param_mottle() * (n - 0.5)), 1.0);
}
";

/// Towards the sun: nearly overhead, a little behind and to the left (the
/// reference's billow tops are its brightest, their fronts a soft grey, the
/// base darker still).
const SUN: [f32; 3] = [-0.25, 0.92, -0.22];

/// The library's cloud: 8 m wide on a 6 m base; this seed puts its tower
/// right of centre, as the reference's.
const SEED: f32 = 3.0;
/// The reference's camera: level with the lower cloud, looking a little up
/// at its middle through a 30 degree lens, the cloud nine tenths of the
/// frame's width.
const CAM: CameraPose = CameraPose {
    eye: [0.0, 6.6, 13.7],
    target: [0.0, 8.0, 0.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 30.0,
};
const CLOSE: CameraPose = CameraPose {
    eye: [1.5, 9.8, 6.8],
    target: [1.2, 9.6, 0.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 30.0,
};
const LOW: CameraPose = CameraPose {
    eye: [-0.6, 4.4, 10.5],
    target: [-0.4, 6.8, 0.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 34.0,
};

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Clouds>();
    let (view, sky, sun) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round().clamp(0.0, 2.0),
            reg.vec3(g.sky),
            reg.vec3(g.sun),
        )
    };
    let cam = match view as u32 {
        1 => CLOSE,
        2 => LOW,
        _ => CAM,
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A photographic tone curve that keeps the greys true (Khronos PBR
        // Neutral), no rim, kick or fill lights of the look's own.
        set(&mut reg, "look.stylizedpbr.tonemap", ParamValue::F32(1.0));
        set(&mut reg, "render.saturation", ParamValue::F32(1.0));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.kick", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.0));
        // A broad grey sky light.
        set(
            &mut reg,
            "render.light.color",
            ParamValue::Color([1.0, 1.0, 1.0]),
        );
        set(&mut reg, "render.light.intensity", ParamValue::F32(1.7));
        set(
            &mut reg,
            "render.ambient",
            ParamValue::Color([0.72, 0.718, 0.713]),
        );
        set(&mut reg, "render.clear_color", ParamValue::Color(sky));
        set(&mut reg, "render.exposure", ParamValue::F32(0.62));
        set(
            &mut reg,
            "look.stylizedpbr.haze_near",
            ParamValue::F32(400.0),
        );
        set(
            &mut reg,
            "look.stylizedpbr.haze_far",
            ParamValue::F32(900.0),
        );
        set(
            &mut reg,
            "procedural.sky.volumetric_looking_clouds.seed",
            ParamValue::F32(SEED),
        );
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = view;
        *world.res_mut::<Clouds>() = g;
    }
    // The sun follows `kit.proc_field.clouds_sun` (live).
    {
        let n = (sun[0] * sun[0] + sun[1] * sun[1] + sun[2] * sun[2])
            .sqrt()
            .max(1e-4);
        set(
            &mut world.res_mut::<ParamRegistry>(),
            "render.light.direction",
            ParamValue::Vec3([sun[0] / n, sun[1] / n, sun[2] / n]),
        );
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("sky-volumetric-looking-clouds", [0.0, 0.0, 0.0]);
    }
    world.res_mut::<RenderWorld>().camera = cam;
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The clouds scene's parameters and backdrop pass alone.
    struct Scene;
    impl KitPlugin for Scene {
        fn name(&self) -> &'static str {
            "proc_field"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            build(kit)
        }
    }

    /// The gate for a kit with game passes: the backdrop pass is accepted
    /// and its whole WGSL module compiles.
    #[test]
    fn clouds_backdrop_pass_checks_clean() {
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }
}
