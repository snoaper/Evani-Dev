//! The short grass's live scene (feature `sgrass`, Asset Studio column 68,
//! 2026-11-10): the procedural library's short grass (`vegetation-short-grass`,
//! evani-grass's lawn kind) as a grazed pasture, as a game shows it, from a
//! player standing in the field, under the reference's camera and light: a
//! standing eye (1.7 m), a 60 degree view at 16:9, the horizon on the upper
//! third, a mid-morning sun from the left and a little behind, a light haze
//! in the distance. A scene, not a studio: one level field of the grass's own
//! patch (`wrapped::short_grass_patch`) with its pasture recipe
//! (`wrapped::short_grass_pasture`), the blades round the player and the
//! module's carpet on to the horizon, a plain under the haze past it. It
//! rides in the rolling hills scene's slot (its modules, its sky pass).
//!
//! `kit.proc_field.sgrass_view`: 0 the reference view, 1 the ground at the
//! player's feet (looking down), 2 low along the ground, 3 from the air.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural_lib::evani_grass::Grass;
use evani_procedural_lib::wrapped::{short_grass_pasture, short_grass_patch};
use evani_render::game_pass::{self, GamePass, PassParam, Stage};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The asset's on/off switch (the library's, read by the host).
const ON: &str = "procedural.vegetation.short_grass.on";
/// The field: half its width (m). Its far edge lies a pixel under the
/// horizon from a standing eye.
const FIELD_HALF: f32 = 1500.0;
/// A 60 degree horizontal view at 16:9 as a vertical field of view.
const FOV_Y: f32 = 36.0;

#[derive(Debug, Clone, Copy)]
pub struct ShortGrassScene {
    view: F32Param,
    heading: F32Param,
    pitch: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    exposure: F32Param,
    haze_near: F32Param,
    haze_far: F32Param,
    /// The light the scene was last set up for (NaN: not yet).
    lit_for: [f32; 6],
    /// Whether the field's patch is with evani-grass, and the frames spent
    /// waiting for its resource.
    grass: bool,
    grass_wait: u32,
}

const SKY_PASS: &str = "proc_field.sgrass_sky";

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let p = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(&format!("kit.proc_field.sgrass_{name}"), v).range(lo, hi).describe(what);
    let view = kit.param_f32(p("view", 0.0, 0.0, 3.0, "0 the reference view (a standing player in the field), 1 the ground at the player's feet, 2 low along the ground, 3 from the air").step(1.0))?;
    let heading = kit.param_f32(p("heading", 0.0, -180.0, 180.0, "The player's heading (degrees from -z towards +x)"))?;
    let pitch = kit.param_f32(p("pitch", 4.85, -30.0, 60.0, "How far the player looks down (degrees; 4.85: the horizon on the upper third)"))?;
    let sun_az = kit.param_f32(p("sun_az", -112.0, -180.0, 180.0, "The sun's bearing from the player's heading (degrees; negative: from the left)"))?;
    let sun_el = kit.param_f32(p("sun_el", 30.0, 5.0, 89.0, "The sun's elevation (degrees; mid-morning)"))?;
    let exposure = kit.param_f32(p("exposure", 0.66, 0.05, 4.0, "The picture's exposure"))?;
    let haze_near = kit.param_f32(p("haze_near", 200.0, 0.0, 20000.0, "Where the distance haze begins (m)"))?;
    let haze_far = kit.param_f32(p("haze_far", 9000.0, 100.0, 100000.0, "Where the haze has hidden the land in the sky's colour (m)"))?;
    // The rolling hills scene's sky: clear, deeper blue overhead, brighter
    // towards the sun, fair-weather clouds.
    game_pass::add(
        kit,
        GamePass::fullscreen(SKY_PASS, super::hills::SKY_WGSL)
            .at(Stage::AfterOpaque)
            .order(5)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(PassParam::color("zenith", [0.3, 0.5, 1.0]).describe("The sky overhead as a share of its colour at the horizon"))
            .param(PassParam::f32("curve", 2.4, 0.5, 8.0).describe("How quickly the sky deepens from the horizon up"))
            .param(PassParam::f32("glow", 0.3, 0.0, 3.0).describe("How much brighter the sky is towards the sun"))
            .param(PassParam::f32("clouds", 0.66, 0.0, 1.2).describe("Where the clouds begin (higher: fewer clouds)")),
    )?;
    kit.resource(ShortGrassScene { view, heading, pitch, sun_az, sun_el, exposure, haze_near, haze_far, lit_for: [f32::NAN; 6], grass: false, grass_wait: 0 });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && let Some(v) = reg.value(id).and_then(|like| evani_procedural::studio::coerce(like, v))
    {
        let _ = reg.set(id, v);
    }
}

/// A camera at `eye` looking `pitch` degrees down on heading `yaw` (degrees
/// from -z towards +x), a vertical field of view `fov`.
fn look(eye: [f32; 3], yaw: f32, pitch: f32, fov: f32) -> CameraPose {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let d = [y.sin() * p.cos(), -p.sin(), -y.cos() * p.cos()];
    CameraPose { eye, target: [eye[0] + d[0] * 100.0, eye[1] + d[1] * 100.0, eye[2] + d[2] * 100.0], up: [0.0, 1.0, 0.0], fov_y_deg: fov }
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<ShortGrassScene>();
    let (view, face, pitch, sun_az, sun_el, exposure, near, far, on) = {
        let reg = world.res::<ParamRegistry>();
        let on = !matches!(reg.id(ON).and_then(|id| reg.value(id)), Some(ParamValue::Bool(false)));
        (reg.f32(g.view).round().clamp(0.0, 3.0), reg.f32(g.heading), reg.f32(g.pitch), reg.f32(g.sun_az), reg.f32(g.sun_el), reg.f32(g.exposure), reg.f32(g.haze_near), reg.f32(g.haze_far), on)
    };
    let lit = [face, sun_az, sun_el, exposure, near, far];
    if lit != g.lit_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        if g.lit_for[0].is_nan() {
            let _ = evani_procedural::studio::apply(&mut reg);
            // The asset's pasture recipe, once, as a game sets it (so a live
            // change to any of its params afterwards stays).
            for (name, v) in short_grass_pasture() {
                set(&mut reg, name, v);
            }
        }
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // Outdoors, mid-morning: a warm white sun from the player's left, the
        // sky's blue as the ambient and in the shadows (no studio panels).
        let (e, az) = (sun_el.to_radians(), (face + sun_az).to_radians());
        // The light's direction is towards the sun; headings are from -z
        // towards +x.
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), -e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.92, 0.8]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.4));
        set(&mut reg, "render.ambient", ParamValue::Color([0.46, 0.55, 0.72]));
        set(&mut reg, "render.shadow_tint", ParamValue::Color([0.55, 0.66, 1.0]));
        set(&mut reg, "render.sky.studio", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.studio", ParamValue::F32(0.0));
        set(&mut reg, "render.sky.zenith", ParamValue::Color([0.3, 0.46, 0.85]));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.66, 0.76, 0.92]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.3, 0.33, 0.24]));
        set(&mut reg, "render.sky.sun_glow", ParamValue::F32(0.0));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.4));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.5));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.2));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.1));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.5));
        set(&mut reg, "render.exposure", ParamValue::F32(exposure));
        // The sky: the pale haze colour at the horizon (the backdrop the haze
        // fades the far field into); the sky pass deepens it to blue overhead
        // and lays fair-weather clouds on it.
        set(&mut reg, "render.clear_color", ParamValue::Color([0.52, 0.68, 0.95]));
        set(&mut reg, "kit.proc_field.sgrass_sky.on", ParamValue::Bool(true));
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(near));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(far));
        set(&mut reg, "render.camera.near", ParamValue::F32(0.12));
        set(&mut reg, "render.camera.far", ParamValue::F32(80_000.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(300.0));
        drop(reg);
        g.lit_for = lit;
    }
    // The field's patch with evani-grass, as a game hands it over; taken
    // away again while the asset is switched off.
    if let Some(gr) = world.try_res::<Grass>() {
        if on && !g.grass {
            gr.clear_patches();
            g.grass = gr.add_patch(short_grass_patch([0.0, 0.0], FIELD_HALF, 0.0)).is_some();
        } else if !on && g.grass {
            gr.clear_patches();
            g.grass = false;
        }
    } else if g.grass_wait <= 120 {
        // No grass module yet (or none in this build).
        g.grass_wait += 1;
    }
    *world.res_mut::<ShortGrassScene>() = g;
    let eye = [0.0, 1.7, 0.0];
    let cam = match view as u32 {
        // The ground at the player's feet.
        1 => look(eye, face + 20.0, 50.0, 40.0),
        // Low along the ground.
        2 => look([0.0, 0.45, 0.0], face - 6.0, 1.0, FOV_Y),
        // From the air, as a free-flying camera sees the field.
        3 => look([0.0, 30.0, 0.0], face, 24.0, 50.0),
        _ => look(eye, face, pitch, FOV_Y),
    };
    let ground = RenderWorld::material(&world.res::<LookData>(), "grass");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // Past the field: a plain under the haze, in the far sward's green.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.3, 0.0]);
    f.scale = [120_000.0, 1.0, 120_000.0];
    f.material = ground;
    f.color = [0.1, 0.17, 0.04, 1.0];
    rw.draw_mesh(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The short grass scene's parameters and sky pass alone.
    struct Scene;
    impl KitPlugin for Scene {
        fn name(&self) -> &'static str {
            "proc_field"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            build(kit)
        }
    }

    /// The gate for a kit with game passes: the sky pass is accepted and its
    /// whole WGSL module compiles.
    #[test]
    fn sgrass_sky_pass_checks_clean() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }

    /// The pasture recipe names only params evani-grass registers, and the
    /// field's patch is one evani-grass accepts.
    #[test]
    fn pasture_recipe_names_real_params() {
        let names = evani_procedural_lib::evani_grass::params::names();
        for (name, _) in short_grass_pasture() {
            assert!(names.iter().any(|n| n == name), "{name}");
        }
        let g = Grass::default();
        assert_eq!(g.add_patch(short_grass_patch([0.0, 0.0], FIELD_HALF, 0.0)), Some(0));
    }

    /// The pasture's blades stay within the landscape budget at the High
    /// tier from any camera: every bucket full is at most 500,000 triangles
    /// (the carpet under them is two more).
    #[test]
    fn pasture_blades_fit_the_landscape_budget_at_high() {
        let recipe = short_grass_pasture();
        let f = |name: &str| match recipe.iter().find(|(n, _)| *n == name) {
            Some((_, ParamValue::F32(v))) => *v,
            _ => 1.0,
        };
        let high = evani_procedural_lib::evani_grass::plan::tier(3).scaled(f("material.grass.blades"), f("material.grass.segments"));
        assert!(high.max_triangles() + 2 <= 500_000, "{} triangles", high.max_triangles());
    }
}
