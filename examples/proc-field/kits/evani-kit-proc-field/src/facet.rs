//! The faceted ball's live scene (feature `facet`, the live faceted-ball
//! session, 2026-11-04): the procedural library's faceted concrete ball
//! (`geometry-icosphere`) in the engine's realism studio, under its
//! reference picture's camera and light (best.png: three-quarter view from
//! a little above, a warm low sun from behind on the right, the shadow
//! going left towards the camera, a pale concrete floor), so it can be
//! tuned live beside the reference (art-source/style-refs/faceted-ball).
//!
//! `kit.proc_field.facet_view`: 0 the reference's view, 1 a grazing
//! close-up (the light skimming the facets of the sunny side, the relief,
//! pores and joints at full size). `kit.proc_field.subject`: 0 the faceted
//! ball, 1 the concrete cube, 2 the granite ball (the other assets under the
//! same camera, for the before-and-after proof that the ball's changes
//! leave them as they were). The game sets only what a game sets: the
//! studio preset, its own sun, a floor, a haze, and its camera. Round 6: the
//! ball's own staging (a lower sun, a warmer light, a soft contact shadow:
//! `kit.proc_field.contact`, `contact_r`); the cube and the
//! granite ball keep round 5's scene.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::game_pass::{self, GamePass, PassParam, Stage};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The subjects, in `kit.proc_field.subject`'s order.
pub const SUBJECTS: [&str; 3] = ["geometry-icosphere", "geometry-cube-cuboid", "geometry-uv-sphere"];

#[derive(Debug, Clone, Copy)]
pub struct Facet {
    view: F32Param,
    subject: F32Param,
    floor: Vec3Param,
    contact: F32Param,
    contact_r: F32Param,
    /// The view and subject the scene was last set up for (NaN: not yet).
    set_for: (f32, f32),
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.facet_view", 0.0)
            .range(0.0, 1.0)
            .step(1.0)
            .describe("0 the faceted ball's reference view (best.png's camera and light), 1 a grazing close-up of its sunny side"),
    )?;
    let subject = kit.param_f32(
        ParamDef::f32("kit.proc_field.subject", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the faceted ball, 1 the concrete cube, 2 the granite ball (the before-and-after proof)"),
    )?;
    // The floor's colour (best.png's pale, neutral concrete floor: about
    // 133 of 255 far off, its cast shadow a cool 81).
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.floor", FLOOR).describe("The floor's colour under the faceted ball"))?;
    // The ball's soft contact shadow on the floor (round 6's review: weak
    // contact).
    let contact = kit.param_f32(
        ParamDef::f32("kit.proc_field.contact", CONTACT.0)
            .range(0.0, 1.0)
            .describe("The ball's soft contact shadow on the floor: its strength (0: none)"),
    )?;
    let contact_r = kit.param_f32(
        ParamDef::f32("kit.proc_field.contact_r", CONTACT.1)
            .range(0.1, 2.0)
            .unit("m")
            .describe("The ball's soft contact shadow: its radius"),
    )?;
    // Round 11: the floor's grain and the contact, one pass after the 3D
    // (see FLOOR_WGSL); on only under the faceted ball.
    game_pass::add(
        kit,
        GamePass::fullscreen(FLOOR_PASS, FLOOR_WGSL)
            .at(Stage::BeforeTonemap)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(PassParam::f32("broad", FLOOR_GRAIN.0, 0.0, 1.0).describe("The floor's mottling (4-33 cm), a fraction of its light"))
            .param(PassParam::f32("fine", FLOOR_GRAIN.1, 0.0, 1.0).describe("The floor's fine concrete grain (3-7 mm), a fraction of its light"))
            .param(PassParam::f32("far", FLOOR_FAR, 0.0, 0.5).describe("How much darker the floor grows from 3 m to 9 m off"))
            .param(PassParam::f32("contact", FLOOR_CONTACT.0, 0.0, 1.0).describe("How much light the floor and the ball lose where they meet"))
            .param(PassParam::f32("reach", FLOOR_CONTACT.1, 0.01, 1.0).describe("How far from the touching point that loss reaches (m)")),
    )?;
    kit.resource(Facet {
        view,
        subject,
        floor,
        contact,
        contact_r,
        set_for: (f32::NAN, f32::NAN),
    });
    Ok(())
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && let Some(v) = reg
            .value(id)
            .and_then(|like| evani_procedural::studio::coerce(like, v))
    {
        let _ = reg.set(id, v);
    }
}

/// The reference's camera: about 2.6 m from the ball's centre, 24 degrees
/// up, a 27 degree lens (the ball about 0.8 of the frame's height, centred).
const DEFAULT_CAM: CameraPose = CameraPose {
    eye: [0.0, 1.56, 2.38],
    target: [0.0, 0.5, 0.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 27.0,
};

/// The grazing close-up: the ball's upper front from above, where the low
/// side sun skims the facets (relief, pores, joints and grain at full size).
const CLOSE_CAM: CameraPose = CameraPose {
    eye: [0.2, 1.38, 1.2],
    target: [0.08, 0.86, 0.3],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 32.0,
};

/// The sun (elevation, azimuth from +z towards +x, degrees): low, from the
/// right and a little behind (best.png: the right of the ball lit, the
/// shadow going left).
const SUN: (f32, f32) = (20.0, 95.0);
/// Round 6, the ball's own staging (the cube and granite ball keep round 5's
/// scene, so their before-and-after proof compares like with like): the sun
/// 12 degrees up (round 5's top was 17 levels brighter than best.png's: the
/// studio's overhead panel is the engine's, so the sun lights the top less
/// instead; round 7: 10 degrees, the paler mottle lifted the top again);
/// round 7: the studio's own soft-shadow search (round 6's wider
/// one blurred the shadow into a pale band).
const SUN_BALL: (f32, f32) = (10.0, 95.0);
const AMBIENT: [f32; 3] = [0.77, 0.73, 0.67];
const AMBIENT_BALL: [f32; 3] = [0.88, 0.75, 0.62];
/// Round 7, the ball's sun a little more golden and its fill warmer (round
/// 6's review: the shaded side neutral grey, R-B 0 against best.png's +10;
/// the lit side R-B +42 against +62).
const SUN_COL_BALL: [f32; 3] = [1.0, 0.7, 0.4];
/// The contact shadow by default: the occlusion discs' strength (all of the
/// scene's discs share it) and the foot's disc's radius (m).
const CONTACT: (f32, f32) = (0.9, 0.18);
/// The floor (round 9): the plain floor again. Round 8's plaster slab read as
/// salt or glitter and cost about 1.3 ms; a fine grid of tilted normals in the
/// look's stone material cost about 2 ms (the model path's lighting); the
/// plain floor costs next to nothing (round 6's review asked for about 0.3 ms).
///
/// Round 10 (round 9's review: a flat black pool from the foot to the frame's
/// edge, the shadow's core 43 against best.png's 78): the floor lit by a 10
/// degree sun takes almost all of its light from the studio's sky, so the
/// ball's bare cast shadow is 101 against a sunlit floor of only 121; a
/// paler floor (the sunlit floor about 150) and three sets of occlusion
/// discs, each only darkening the sky's light (the sun's is already gone in
/// the shadow). A disc's strength falls the further its centre sinks under
/// the floor (forward.wgsl's height band), so one shared strength gives:
/// - the foot's disc on the floor, full strength, just in front of the
///   touching point (where the floor shows under the ball's lower edge): a
///   thin dark line where the ball meets the floor, back to the floor's
///   tone within a few centimetres;
/// - the shadow's discs, sunk SHADE_SINK (about two thirds of the strength),
///   SHADE_N of them close together along the shadow (so its edge stays
///   smooth), each as wide as the ball's shadow is there: the cast shadow a
///   mid grey (about 75), darkest along its middle, fading out at its edges.
///
/// No discs for the floor's texture or the far floor any more (round 10's
/// review: they read as smoky horizontal bands with arcs at their edges):
/// FLOOR_WGSL draws the floor's grain in world space instead.
const FOOT: [f32; 3] = [0.0, 0.0, 0.06];
/// Round 12: the shadow's discs sink a little more with each step away from
/// the ball (its core about 75 near the foot, lighter towards its end).
/// Round 15: the far end lighter (round 14's review: too dark there).
const SHADE_SINK: (f32, f32) = (0.118, 0.011);
const SHADE_N: usize = 12;
/// The ball's radius in this scene (the library's default), for its
/// shadow's shape.
const BALL_R: f32 = 0.5;
/// Round 11 (round 10's review: smoky floor bands and disc arcs, no fine
/// grain, the ground under the ball too light, the shadow's inside flat):
/// one full-screen pass after the 3D (BeforeTonemap, on the HDR picture)
/// that finds the floor from the depth (the ground plane, in world space)
/// and varies its light by three octaves of mottling (4-33 cm) and two of
/// fine grain (3-7 mm), each faded wherever it would be under about two
/// pixels (no glitter, no shimmer), the far floor a little darker; the
/// same variation runs on inside the cast shadow. Then, around the point
/// where the ball touches the floor, both surfaces lose light (a soft
/// gaussian in 3D: the floor at the foot and the ball's own underside).
/// Pass `kit.proc_field.facet_floor.*` (live), off for the cube and granite.
const FLOOR_PASS: &str = "proc_field.facet_floor";
/// Broad and fine strength (fractions of the floor's light).
const FLOOR_GRAIN: (f32, f32) = (0.22, 0.22);
/// The far floor a little darker (best.png's far floor 142, its near 152).
const FLOOR_FAR: f32 = 0.09;
/// Contact strength and reach (m).
const FLOOR_CONTACT: (f32, f32) = (0.45, 0.4);
const FLOOR_WGSL: &str = "
fn ff_h(c: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(c.x) * 747796405u) ^ (bitcast<u32>(c.y) * 2891336453u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967296.0;
}
fn ff_n(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(ff_h(i), ff_h(i + vec2<i32>(1, 0)), u.x);
    let b = mix(ff_h(i + vec2<i32>(0, 1)), ff_h(i + vec2<i32>(1, 1)), u.x);
    return mix(a, b, u.y) - 0.5;
}
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let c = textureSample(scene, samp, uv).rgb;
    let d = textureLoad(depth, vec2<i32>(px), 0);
    let w = world_at(uv, max(d, 1e-6));
    let q = w.xz;
    // Metres a pixel covers on the floor (its steeper direction).
    let fw = max(abs(dpdx(q)), abs(dpdy(q)));
    let mp = max(fw.x, fw.y);
    // Mottling at 33, 11 and 4 cm (round 11's first try at 0.5-1.3 m drew
    // smoky bands towards the horizon), each faded under about two pixels.
    let qr = vec2<f32>(q.x * 0.8 - q.y * 0.6, q.x * 0.6 + q.y * 0.8);
    let broad = ff_n(q * 3.0) * 0.6 + ff_n(qr * 9.0 + vec2<f32>(7.3, 2.1)) * 0.5 * (1.0 - smoothstep(0.3, 0.6, mp * 9.0))
        + ff_n(q * 0.9 + vec2<f32>(2.7, 6.1)) * 3.0 * smoothstep(3.5, 7.0, length(w - frame.camera.xyz))
        + ff_n(q * vec2<f32>(26.0, 14.0) + vec2<f32>(1.3, 8.2)) * 0.4 * (1.0 - smoothstep(0.3, 0.6, mp * 26.0));
    // The grain's two octaves turned against each other and the noise grid
    // (no grid shows), kept down to about a pixel (round 11 cut it under
    // two, so the middle and far floor went bare), its strength varied
    // over 30-50 cm patches (trowelled concrete, not sandpaper).
    let qs = vec2<f32>(q.x * 0.96 + q.y * 0.28, q.y * 0.96 - q.x * 0.28);
    // Round 13 (round 12's review: about twice best.png's fine grain beside
    // the ball, spread evenly): patches from light (0.35; round 13's 0.15
    // read as too patchy) to full (1.2), the grain halved from about 2.6 m
    // to 4 m off.
    let dist = length(w - frame.camera.xyz);
    let gm = clamp(0.85 + ff_n(q * 2.5 + vec2<f32>(4.1, 9.3)) * 1.0, 0.45, 1.15) * (1.0 - 0.5 * smoothstep(2.6, 4.0, dist));
    let f1 = ff_n(qs * 150.0 + vec2<f32>(3.1, 5.7)) * (1.0 - smoothstep(0.5, 1.0, mp * 150.0)) * gm;
    let f2 = ff_n(qr * 330.0 + vec2<f32>(11.7, 1.9)) * 0.7 * (1.0 - smoothstep(0.5, 1.0, mp * 330.0)) * gm;
    let fl = select(0.0, 1.0 - smoothstep(0.003, 0.01, w.y), d > 0.0);
    let far = 1.0 - param_far() * smoothstep(3.0, 9.0, dist);
    let tex = 1.0 + fl * (broad * param_broad() + (f1 + f2) * param_fine() + far - 1.0);
    let r = length(w) / max(param_reach(), 1e-3);
    let ao = 1.0 - select(0.0, param_contact() * exp(-r * r), d > 0.0);
    return vec4<f32>(c * tex * ao, 1.0);
}
";
/// The ball's haze (times the camera's distance): round 10, further off, so
/// the floor reads in perspective to the top of the frame (round 9's review:
/// a hazy sweep).
const HAZE_BALL: (f32, f32) = (2.5, 12.0);
/// Round 7, the ball's sun, exposure and floor: the paler mottle lifted the
/// ball about 7% (exposure 0.53 to 0.47), a slightly stronger sun for the
/// lit and shaded faces' contrast, and the floor `kit.proc_field.floor`
/// times this, lighter and neutral (best.png's floor is a pale neutral grey;
/// the golden sun warms it).
/// Round 11 (round 10's review: the lit side 6.5 levels dark, no satin): 4.3;
/// round 12 (its review: the lit side lifted flat and yellow, still no
/// satin): 4.2 (a stronger rim sheen to spend the light on drew a glowing
/// outline round the whole ball, the shaded edge too).
/// Round 13: 3.8, the satin now from the paste's own sun-side lobe; round
/// 15: 4.0 (the tighter lobe no longer lifts the lit faces); round 16: 3.8
/// (the lit side about 5% too bright).
const SUN_I_BALL: f32 = 3.8;
const EXPOSURE_BALL: f32 = 0.445;
/// Round 12: a greyer, less sandy floor (round 11's review: R-B +26 against
/// best.png's +12).
/// Round 13: 5% lighter (the near-right floor 144 against best.png's 152).
const FLOOR_K_BALL: [f32; 3] = [2.02, 2.34, 2.86];

/// The floor's colour by default (round 4: darker, the far floor 133 as
/// best.png's, not 149).
const FLOOR: [f32; 3] = [0.5, 0.515, 0.56];

pub fn draw(world: &mut World) {
    let mut w = *world.res::<Facet>();
    let (view, subject) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(w.view).round(), reg.f32(w.subject).round().clamp(0.0, 2.0))
    };
    let ball = subject < 0.5;
    let (contact, contact_r) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(w.contact), reg.f32(w.contact_r))
    };
    if (view, subject) != w.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the game's own sun and fill.
        let _ = evani_procedural::studio::apply(&mut reg);
        let sun = if ball { SUN_BALL } else { SUN };
        let (el, az) = (sun.0.to_radians(), sun.1.to_radians());
        // Measured against best.png (the live session): its lit side about
        // 157 of 255 and golden (186,158,132), its shaded side a grey about
        // 72, its front 89 and centre 105, the far floor 133 and the cast
        // shadow a cool 81 (76,79,84). Round 4 (its review: too bright on the
        // right, too dark on the left and in front, the floor too pale): a
        // more golden sun at a lower exposure (the pyramids' lit and shaded
        // faces apart, as best.png's), a warmer fill, the floor's bounce
        // lifting the shaded and lower faces (it lights no floor), more light
        // from the camera's side.
        set(&mut reg, "render.light.color", ParamValue::Color(if ball { SUN_COL_BALL } else { [1.0, 0.75, 0.5] }));
        set(&mut reg, "render.light.intensity", ParamValue::F32(if ball { SUN_I_BALL } else { 3.3 }));
        // A neutral fill: the shaded side reads grey (round 1: beige; round
        // 2: slate blue), lifted from below by the floor's warm bounce.
        set(&mut reg, "render.ambient", ParamValue::Color(if ball { AMBIENT_BALL } else { AMBIENT }));
        // The screen-space AO draws streaks beside a foot (the cube's
        // session); the soft shadow carries the contact here.
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.0));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.84, 0.83, 0.81]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.92, 0.86, 0.78]));
        // The cast shadow a little cool, as best.png's (more would turn the
        // ball's own shaded side blue; best.png's is a warm grey).
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.15));
        // Matte concrete: the studio's reflections dimmed (round 2's
        // review saw pale swirls of sheen on the panels).
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.35));
        // A little light from the camera's side: best.png's front faces are
        // mid-grey (about 89 to 105), not in full shade.
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.26));
        set(&mut reg, "render.exposure", ParamValue::F32(if ball { EXPOSURE_BALL } else { 0.53 }));
        // The floor pass only under the faceted ball (the cube and granite
        // keep round 5's scene, pixel for pixel).
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(ball));
        // Round 12 (round 11's review: the close-up's background busier than
        // the reference's soft, out-of-focus floor): less grain behind the
        // close-up.
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(FLOOR_GRAIN.1 * if view > 0.5 { 0.35 } else { 1.0 }));
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        set(&mut reg, "render.clear_color", ParamValue::Color([0.7, 0.68, 0.65]));
        let cam = if view > 0.5 { CLOSE_CAM } else { DEFAULT_CAM };
        let dist = (0..3).map(|k| (cam.eye[k] - cam.target[k]).powi(2)).sum::<f32>().sqrt();
        let haze = if ball { HAZE_BALL } else { (1.7, 5.5) };
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist * haze.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist * haze.1));
        // The shadow box fitted to the subject and its shadow.
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(6.0));
        // A capture scene: fixed quality, so every round's pictures compare
        // (adaptive quality shed the shadow and the render scale whenever the
        // ball regenerated or other work shared the graphics card).
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        w.set_for = (view, subject);
        *world.res_mut::<Facet>() = w;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        // The cube stands centred on the ball's place (its base on the ground).
        s.place(SUBJECTS[subject as usize], [0.0, 0.0, 0.0]);
    }
    let mut floor_rgb = world.res::<ParamRegistry>().vec3(w.floor);
    if ball {
        floor_rgb = std::array::from_fn(|k| floor_rgb[k] * FLOOR_K_BALL[k]);
    }
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = if view > 0.5 { CLOSE_CAM } else { DEFAULT_CAM };
    // The reference's pale, warm concrete floor (round 7: the plain floor
    // again for the ball too: round 6's concrete slab took the studio's
    // strong overhead light, so its shadow read pale, and it doubled the
    // frame's cost).
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [floor_rgb[0], floor_rgb[1], floor_rgb[2], 1.0];
    rw.draw_mesh(f);
    if ball && contact > 0.0 {
        rw.blob_strength = contact;
        rw.draw_blob(FOOT, contact_r);
        // Along the shadow: away from the sun across the floor. The ball's
        // shadow is an ellipse BALL_R wide, BALL_R / sin(elevation) long,
        // its middle BALL_R / tan(elevation) from the foot.
        let (el, az) = (SUN_BALL.0.to_radians(), SUN_BALL.1.to_radians());
        let away = [-az.sin(), -az.cos()];
        let (mid, long) = (BALL_R / el.tan(), BALL_R / el.sin());
        for k in 0..SHADE_N {
            let t = 0.15 + 0.155 * k as f32;
            let across = (1.0 - ((t - mid) / long).powi(2)).max(0.0).sqrt() * BALL_R;
            rw.draw_blob([FOOT[0] + away[0] * t, -(SHADE_SINK.0 + SHADE_SINK.1 * k as f32), FOOT[2] + away[1] * t], across.max(0.15));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The faceted ball scene's parameters and floor pass alone.
    struct Scene;
    impl KitPlugin for Scene {
        fn name(&self) -> &'static str {
            "proc_field"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            build(kit)
        }
    }

    /// The gate for a kit with game passes: the floor pass is accepted and
    /// its whole WGSL module compiles.
    #[test]
    fn facet_floor_pass_checks_clean() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }
}
