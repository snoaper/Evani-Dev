//! The concrete cube's live scene (feature `cube`, the Asset Studio cube
//! session, 2026-11-05): the procedural library's cast-concrete cube
//! (`geometry-cube-cuboid`) in the engine's realism studio, under its
//! reference picture's camera and light (best.png: a three-quarter view from
//! a little above through a long lens, a low warm sun from behind on the
//! right, the soft shadow going left and a little towards the camera, a
//! pale concrete floor fading into a hazy backdrop), so it can be tuned live
//! beside the reference.
//!
//! `kit.proc_field.cube_view`: 0 the reference's view, 1 a close-up of the
//! top front corner (the arrises' chips, the pores, the top face), 2 a
//! close-up of the foot (the bottom arris and where the cube meets the
//! floor). `kit.proc_field.cube_subject`: 0 the cube, 1 the concrete cone, 2
//! the faceted ball, 3 the granite ball: the others share the concrete
//! shader, the studio or the stone, and are shot before and after a cube
//! change to show they draw the same (under this scene's light and camera).
//!
//! The floor's grain is the faceted ball scene's game pass
//! (`kit.proc_field.facet_floor.*`, built by that scene, which this feature
//! turns on); this scene sets its strengths for the cube's camera.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::game_pass::{self, GamePass, PassParam, Stage};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The subjects, by `kit.proc_field.cube_subject`.
pub const SUBJECTS: [&str; 4] = ["geometry-cube-cuboid", "geometry-cone", "geometry-icosphere", "geometry-uv-sphere"];

#[derive(Debug, Clone, Copy)]
pub struct Cube {
    view: F32Param,
    subject: F32Param,
    floor: Vec3Param,
    /// The (view, subject) the scene was last set up for (NaN: not yet).
    set_for: (f32, f32),
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.cube_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the cube's reference view (best.png's camera and light), 1 a close-up of its top front corner, 2 a close-up of its foot"),
    )?;
    let subject = kit.param_f32(
        ParamDef::f32("kit.proc_field.cube_subject", 0.0)
            .range(0.0, 3.0)
            .step(1.0)
            .describe("0 the concrete cube, 1 the concrete cone, 2 the faceted ball, 3 the granite ball (the no-change proof)"),
    )?;
    let floor = kit.param_vec3(ParamDef::color("kit.proc_field.cube_floor", FLOOR).describe("The floor's colour under the concrete cube"))?;
    // Round 2: the contact crease, one pass after the 3D (see CONTACT_WGSL).
    game_pass::add(
        kit,
        GamePass::fullscreen(CONTACT_PASS, CONTACT_WGSL)
            .at(Stage::BeforeTonemap)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(PassParam::f32("strength", CONTACT.0, 0.0, 1.0).describe("How much light the floor and the cube's foot lose where they meet"))
            .param(PassParam::f32("reach", CONTACT.1, 0.005, 0.5).describe("How far from the cube's foot that loss reaches (m)"))
            .param(PassParam::f32("half", 0.5, 0.05, 10.0).describe("Half the cube's width and depth (m; the scene's cube is 1 m)"))
            .param(PassParam::f32("pool", POOL, 0.0, 2.0).describe("The warm patch of sun on the floor behind the cube on the right (best.png's), its strength"))
            .param(PassParam::f32("scuff", SCUFF, 0.0, 1.0).describe("The floor's low-contrast mottling and scuff marks near the cube, in sun and shadow alike")),
    )?;
    kit.resource(Cube {
        view,
        subject,
        floor,
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

/// best.png's camera, solved from the cube's seven visible corners (rms
/// 4.4 px on the 1672 x 941 picture): 8.5 m off, 15.3 degrees up, from
/// 57.5 degrees round (from +z towards +x), an 11 degree lens (vertical).
const DEFAULT_CAM: CameraPose = CameraPose {
    eye: [6.8974, 2.6813, 4.3844],
    target: [0.0058, 0.4396, -0.0092],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 11.0,
};

/// The top front corner, from the same side, closer and a little higher:
/// the two lit arrises, the corner's chips and the top face's skin.
const CORNER_CAM: CameraPose = CameraPose {
    eye: [1.55, 1.62, 1.18],
    target: [0.38, 0.86, 0.28],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 30.0,
};

/// The foot: the front bottom arris and the floor beside it, low.
const FOOT_CAM: CameraPose = CameraPose {
    eye: [1.6, 0.42, 1.25],
    target: [0.38, 0.12, 0.3],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 30.0,
};

/// A scene's staging: the sun (elevation, azimuth from +z towards +x,
/// degrees), its colour and strength, the fill, the sky, the exposure, the
/// backdrop and haze (m), the floor's grain (the floor pass's broad and
/// fine) and the floor's colour (the look's stone, times twice this).
pub struct Staging {
    pub sun: (f32, f32),
    pub sun_col: [f32; 3],
    pub sun_i: f32,
    pub ambient: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
    pub fill: f32,
    /// The sky's share of the ambient light (0: the flat `ambient` alone).
    pub ibl: f32,
    pub exposure: f32,
    pub backdrop: [f32; 3],
    pub haze: (f32, f32),
    pub grain: (f32, f32),
    pub floor: [f32; 3],
    /// The screen-space ambient occlusion: strength and reach (m).
    pub ao: (f32, f32),
    /// The contact crease (the contact pass's strength; 0: off).
    pub contact: f32,
    /// The sky light's tint straight up, and its share drawn as the studio's
    /// soft panels (1) rather than a sky (0).
    pub zenith: [f32; 3],
    pub studio: f32,
    /// Sky reflections: the look's environment specular and the sky's F0.
    pub env_spec: f32,
    pub ibl_spec: f32,
    /// The warm sun patch on the floor (the contact pass's `pool`).
    pub pool: f32,
}

/// The session's first staging, kept for the other subjects so their
/// before-and-after pictures compare like with like. The sun's azimuth from
/// best.png's two shadow edges (each fitted alone: 133 and 135 degrees).
pub const STAGE_R0: Staging = Staging {
    sun: (14.0, 134.0),
    sun_col: [1.0, 0.74, 0.47],
    sun_i: 3.6,
    ambient: [0.84, 0.78, 0.7],
    horizon: [0.86, 0.85, 0.83],
    ground: [0.92, 0.86, 0.78],
    fill: 0.2,
    ibl: 1.0,
    exposure: 0.47,
    backdrop: [0.74, 0.72, 0.7],
    haze: (8.0, 26.0),
    grain: (0.22, 0.22),
    floor: FLOOR,
    ao: (0.0, 0.3),
    contact: 0.0,
    zenith: [1.0, 1.0, 1.0],
    studio: 1.0,
    env_spec: 0.35,
    ibl_spec: 0.04,
    pool: 0.0,
};

/// The cube's own staging (round 1, measured against best.png's region
/// tones): the sun 28 degrees up (the lit floor against its shadow, 157 to
/// 91; best.png 148-173 to 85); a flat, neutral ambient instead of the
/// studio's sky light, so the cube's shaded face and the floor's shadow take
/// the same fill (76 and 91; best.png 77 and 85: the studio panels face the
/// sun and left that face at 41); a cool grey floor with only a faint grain
/// (the faceted ball's mottling read as smoke at this distance); a paler
/// backdrop and a nearer haze (best.png's far floor about 170).
/// Column 6 round 1: a sky light instead of the flat ambient (a dim, cool
/// zenith, a white horizon, a warm bounce from the sunlit floor), so the
/// top takes the sky's cooler light and the shaded face the floor's warmer
/// (best.png: top R-B 23, shaded face +4, lit face about +40); a more
/// orange sun; a near-white backdrop (best.png's far floor 166-175); a
/// neutral floor with a visible grain and mottle; weaker sky reflections; a
/// stronger warm sun patch.
/// Column 6 round 2 (the tab's review: a warm backdrop, a dark, smooth
/// floor): a cool white backdrop (the far floor's R-B 16 to 9; best.png 8), a
/// lighter, bluer floor (its sunlit right 162 to 169, R-B 39 to 34; best.png
/// 170 and 34) and its broad mottle up from 0.05 to 0.08 (low-contrast marks,
/// not a fine grain; more read as blotches on the far floor).
/// Column 6 round 3 (the review: the shaded face 2.6 levels light and a
/// little cool): a dimmer, warmer ambient (shaded face 78.8 to 76.1, R-B
/// 2.0 to 3.0; best.png 76.0 and 4.0), the floor 2% lighter to hold its 169.
/// Round 4: the floor a touch bluer (its R-B 36 against best.png's 34).
pub const STAGE: Staging = Staging {
    sun: (22.0, 134.0),
    sun_col: [1.0, 0.68, 0.46],
    sun_i: 3.9,
    ambient: [0.86, 0.78, 0.68],
    horizon: [1.0, 1.0, 1.0],
    ground: [1.0, 0.86, 0.7],
    fill: 0.0,
    ibl: 1.0,
    exposure: 0.45,
    backdrop: [0.92, 0.96, 1.0],
    haze: (7.0, 20.0),
    grain: (0.08, 0.22),
    floor: [0.54, 0.585, 0.695],
    ao: (0.0, 0.3),
    contact: CONTACT.0,
    zenith: [0.4, 0.45, 0.53],
    studio: 0.0,
    env_spec: 0.12,
    ibl_spec: 0.02,
    pool: 1.3,
};

/// The floor's marks near the cube (column 6 round 3, the review: the floor
/// uniformly grained, best.png's has irregular low-contrast marks, its
/// shadow too): a 4-60 cm mottle of about +-5% and short scuffs a few
/// percent darker, along wandering directions, faded out 2.5-7 m from the
/// cube (further off they read as blotches).
const SCUFF: f32 = 1.0;

/// The sun patch's strength (round 4: best.png's floor behind the cube on
/// the right is about 190 of 255, the rest 145-160; a warm gaussian 0.9 m
/// across, 1.9 m behind the cube, from the solved camera).
const POOL: f32 = 1.3;

/// Pass `kit.proc_field.cube_contact.*` (live; round 2, the review: no
/// contact crease, the cube floated): one full-screen pass after the 3D (on
/// the HDR picture) that finds each pixel's world point from the depth and
/// darkens it by its distance from the cube's square foot (`half` either
/// side of the origin), a gaussian over `reach`, on the floor and on the
/// cube's own faces the same height up: best.png's dark seam where the
/// cube meets the floor, fading out over a few centimetres both ways. Off
/// for the other subjects (their before-and-after pictures).
const CONTACT_PASS: &str = "proc_field.cube_contact";
/// The crease's strength and reach (m) (round 4: best.png's crease is a
/// thin, dark line, about 4 px; round 3's 3 cm reach drew a 19 px band;
/// column 6 round 1: round 5's 0.75 over 5 mm read as a drawn outline).
const CONTACT: (f32, f32) = (0.45, 0.008);
const CONTACT_WGSL: &str = "
fn cube_h(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.547);
}
fn cube_n(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(cube_h(i), cube_h(i + vec2<f32>(1.0, 0.0)), u.x), mix(cube_h(i + vec2<f32>(0.0, 1.0)), cube_h(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let c = textureSample(scene, samp, uv).rgb;
    let d = textureLoad(depth, vec2<i32>(px), 0);
    let w = world_at(uv, max(d, 1e-6));
    let e = max(abs(w.xz) - vec2<f32>(param_half()), vec2<f32>(0.0));
    let r = (length(e) + max(w.y, 0.0)) / max(param_reach(), 1e-3);
    // A faint, wider occlusion of the floor round the foot (round 5:
    // best.png's floor darkens a little over 5-15 px out).
    let r2 = r * param_reach() / 0.05;
    let k = select(0.0, param_strength() * exp(-r * r) + 0.05 * exp(-r2 * r2), d > 0.0);
    let pd = (w.xz - vec2<f32>(0.0, -1.9)) / 0.9;
    let pool = select(0.0, param_pool() * exp(-dot(pd, pd)), d > 0.0 && w.y < 0.01);
    // Column 6 round 3: the floor's mottle and scuffs (in sun and shadow).
    let fq = w.xz;
    let mot = cube_n(fq * 2.3) * 0.4 + cube_n(fq * 7.1 + vec2<f32>(3.1, 7.7)) * 0.35 + cube_n(fq * 19.0 + vec2<f32>(1.7, 4.4)) * 0.25 - 0.5;
    // Round 4 (the review: round 3's scuffs were long, fine, near-parallel
    // hairlines): short broken marks, a few cm long, each 50 cm patch of
    // floor with its own direction, and fewer of them.
    let ang = cube_h(floor(fq * 2.0)) * 6.28;
    let dir = vec2<f32>(cos(ang), sin(ang));
    let sc = cube_n(vec2<f32>(dot(fq, dir) * 14.0, dot(fq, vec2<f32>(-dir.y, dir.x)) * 45.0));
    let scuff = smoothstep(0.78, 0.95, sc) * smoothstep(0.55, 0.75, cube_n(fq * 2.7 + vec2<f32>(5.0, 2.0)));
    let fm = select(0.0, param_scuff() * (1.0 - smoothstep(2.5, 7.0, length(fq))), d > 0.0 && w.y < 0.01);
    let marks = 1.0 + fm * (mot * 0.12 - scuff * 0.06);
    return vec4<f32>(c * (1.0 - k) * (1.0 + pool * vec3<f32>(1.0, 0.82, 0.62)) * marks, 1.0);
}
";

/// The floor's colour by default (a colour param keeps 0..1; the floor is
/// drawn at twice it).
const FLOOR: [f32; 3] = [0.5, 0.6, 0.75];

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Cube>();
    let (view, subject) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.subject).round().clamp(0.0, 3.0))
    };
    let cam = match view as u32 {
        1 => CORNER_CAM,
        2 => FOOT_CAM,
        _ => DEFAULT_CAM,
    };
    if (view, subject) != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        // The engine's studio, then the scene's own key and fill.
        let _ = evani_procedural::studio::apply(&mut reg);
        // A capture scene: fixed quality, so every round's pictures compare.
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        let st = if subject < 0.5 { &STAGE } else { &STAGE_R0 };
        let (el, az) = (st.sun.0.to_radians(), st.sun.1.to_radians());
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        set(&mut reg, "render.light.color", ParamValue::Color(st.sun_col));
        set(&mut reg, "render.light.intensity", ParamValue::F32(st.sun_i));
        set(&mut reg, "render.ambient", ParamValue::Color(st.ambient));
        // The screen-space AO draws streaks beside a foot (the cube's first
        // session); the soft shadow carries the contact.
        set(&mut reg, "render.ao.strength", ParamValue::F32(st.ao.0));
        set(&mut reg, "render.ao.radius", ParamValue::F32(st.ao.1));
        set(&mut reg, "render.sky.horizon", ParamValue::Color(st.horizon));
        set(&mut reg, "render.sky.ground", ParamValue::Color(st.ground));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.15));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(st.env_spec));
        set(&mut reg, "render.sky.zenith", ParamValue::Color(st.zenith));
        set(&mut reg, "render.sky.studio", ParamValue::F32(st.studio));
        set(&mut reg, "render.ibl.specular", ParamValue::F32(st.ibl_spec));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(st.fill));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(st.ibl));
        set(&mut reg, "render.exposure", ParamValue::F32(st.exposure));
        set(&mut reg, "render.clear_color", ParamValue::Color(st.backdrop));
        // The haze from about the cube's distance out, so the far floor
        // reads as the reference's pale backdrop.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(st.haze.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(st.haze.1));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view > 0.5 { 4.0 } else { 9.0 }));
        // The floor's grain (the faceted ball scene's pass), no contact disc
        // (a cube's contact follows its edges; the soft shadow carries it).
        set(&mut reg, "kit.proc_field.facet_floor.on", ParamValue::Bool(true));
        set(&mut reg, "kit.proc_field.facet_floor.broad", ParamValue::F32(st.grain.0));
        set(&mut reg, "kit.proc_field.facet_floor.fine", ParamValue::F32(st.grain.1));
        set(&mut reg, "kit.proc_field.facet_floor.contact", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.facet_floor.far", ParamValue::F32(0.0));
        set(&mut reg, "kit.proc_field.cube_floor", ParamValue::Color(st.floor));
        set(&mut reg, "kit.proc_field.cube_contact.on", ParamValue::Bool(st.contact > 0.0));
        set(&mut reg, "kit.proc_field.cube_contact.strength", ParamValue::F32(st.contact));
        set(&mut reg, "kit.proc_field.cube_contact.pool", ParamValue::F32(st.pool));
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = (view, subject);
        *world.res_mut::<Cube>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(SUBJECTS[subject as usize], [0.0, 0.0, 0.0]);
    }
    let rgb = world.res::<ParamRegistry>().vec3(g.floor);
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // The reference's pale, warm-grey concrete floor.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The cube scene's parameters and contact pass alone.
    struct Scene;
    impl KitPlugin for Scene {
        fn name(&self) -> &'static str {
            "proc_field"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            build(kit)
        }
    }

    /// The gate for a kit with game passes: the contact pass is accepted and
    /// its whole WGSL module compiles.
    #[test]
    fn cube_contact_pass_checks_clean() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }
}
