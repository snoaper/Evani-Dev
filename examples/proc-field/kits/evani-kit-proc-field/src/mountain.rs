//! The mountains' live scene (feature `mountain`, the Asset Studio
//! mountains session, column 19, 2026-11-07): the procedural library's
//! mountain massif (`terrain-mountains`, 4.4 km across, its summit 1.2 km
//! up) as a game shows it, under its reference picture's two cameras and its
//! light (mid-morning sun from the left, a light haze in the distance): a
//! scene, not a studio. Round 3: the asset's own land round its square
//! (`horizon`, 26 km) carries the foothills on into hazy ranges of their
//! own (round 2 tiled turned copies of the massif round it, whose repeated
//! peaks and square edges showed); a ground plane carries on to the horizon
//! and a sky pass grades the sky; the camera's far plane is raised for them
//! (`camera-range`).
//!
//! `kit.proc_field.mountain_view`: 0 the aerial overview (the reference's
//! left panel: high and oblique, the whole massif in frame, the horizon at
//! the top), 1 the player's eye (the right panel: 1.7 m over the foothills
//! at the mountain's foot, level, a 60 degree view, the summit ahead), 2 a
//! close-up of the upper slopes (ridges, gullies, snow), 3 the forest's edge
//! from the meadow.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::{ProcScene, Registry};
use evani_procedural_lib::evani_grass::Grass;
use evani_procedural_lib::mountain::{self, Land};
use evani_render::game_pass::{self, GamePass, PassParam, Stage};
use evani_render::{MeshId, MeshInstance, RenderWorld};

const KEY: &str = "terrain-mountains";

#[derive(Debug, Clone, Copy)]
pub struct Mountain {
    view: F32Param,
    yaw: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    haze_near: F32Param,
    haze_far: F32Param,
    exposure: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
    /// The summit and the player's stand (eye) for the current land.
    summit: [f32; 3],
    stand: [f32; 3],
    /// Whether the meadow grass round the stand was handed to evani-grass,
    /// and the frames spent waiting for its resource.
    grass: bool,
    grass_wait: u32,
}

/// The cameras' heading: the side of the massif they look from (degrees
/// from +z towards +x; 0 looks north, from the south).
const YAW: f32 = 0.0;
/// How far round the player's stand the library draws the forest in full
/// (its `focus_radius`, m).
const FOCUS_RADIUS: f32 = 260.0;
/// The meadow grass round the stand: its square's half width and how far
/// ahead of the player its middle lies (m).
const GRASS_HALF: f32 = 80.0;
/// The aerial overview's pitch (degrees down; round 3: the reference's own,
/// its horizon just in frame), and the camera's distance out from the
/// summit and height over it (m).
const AERIAL_PITCH: f32 = 30.0;
const AERIAL_OUT: f32 = 3500.0;
const AERIAL_OVER: f32 = 550.0;
const GRASS_AHEAD: f32 = 60.0;
/// Round 3: how far round the massif the asset draws its own land (its
/// `horizon`, m from the middle).
const HORIZON: f32 = 26_000.0;

/// The sky pass (`kit.proc_field.mountain_sky.*`, after the canyons
/// scene's, column 12): where nothing was drawn, a clear mid-morning sky
/// from the hazy horizon (the background colour the distance haze fades to)
/// to a deeper blue overhead, brighter towards the sun, with a few thin
/// fair-weather clouds.
const SKY_PASS: &str = "proc_field.mountain_sky";
const SKY_WGSL: &str = "
fn sky_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn sky_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = sky_hash(i);
    let b = sky_hash(i + vec2<f32>(1.0, 0.0));
    let c = sky_hash(i + vec2<f32>(0.0, 1.0));
    let d = sky_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let c = textureSample(scene, samp, uv).rgb;
    let d = textureLoad(depth, vec2<i32>(px), 0);
    if (d > 0.0) { return vec4<f32>(c, 1.0); }
    let r = view_ray(uv);
    let hz = frame.background.rgb;
    let e = clamp(r.y, 0.0, 1.0);
    let t = 1.0 - pow(1.0 - e, param_curve());
    var s = mix(hz, hz * param_zenith(), t);
    let g = max(dot(r, normalize(frame.sun.xyz)), 0.0);
    s = s * (1.0 + param_glow() * pow(g, 5.0) * (1.0 - 0.5 * t));
    // Thin clouds on a flat layer overhead: soft-edged fbm, thinning
    // towards the horizon's haze.
    if (r.y > 0.02) {
        let q = r.xz / r.y * 1.4;
        var n = 0.0;
        var a = 0.5;
        var p = q;
        for (var i = 0; i < 5; i = i + 1) {
            n = n + a * sky_noise(p);
            p = p * 2.03 + vec2<f32>(5.2, 1.3);
            a = a * 0.5;
        }
        let cover = smoothstep(param_clouds(), param_clouds() + 0.2, n) * smoothstep(0.03, 0.22, r.y);
        // Lit tops, a greyer underside where the cloud is thick.
        let body = hz * mix(1.22, 0.95, smoothstep(0.75, 1.0, n));
        s = mix(s, body, cover * 0.9);
    }
    return vec4<f32>(s, 1.0);
}
";

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.mountain_view", 0.0)
            .range(0.0, 3.0)
            .step(1.0)
            .describe("0 the aerial overview (the reference's left panel), 1 the player's eye at the mountain's foot (the right panel), 2 a close-up of the upper slopes, 3 the forest's edge"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.mountain_yaw", YAW).range(-180.0, 180.0).describe("The side the cameras look from (degrees from +z towards +x)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.mountain_sun_az", -78.0).range(-180.0, 180.0).describe("The sun's bearing from the camera's heading (degrees; negative: from the camera's left)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.mountain_sun_el", 36.0).range(5.0, 89.0).describe("The sun's elevation (degrees; mid-morning)"))?;
    let haze_near = kit.param_f32(ParamDef::f32("kit.proc_field.mountain_haze_near", 600.0).range(0.0, 20000.0).unit("m").describe("Where the distance haze begins (m)"))?;
    let haze_far = kit.param_f32(ParamDef::f32("kit.proc_field.mountain_haze_far", 36000.0).range(100.0, 100000.0).unit("m").describe("Where the haze has hidden the land in the sky's colour (m)"))?;
    let exposure = kit.param_f32(ParamDef::f32("kit.proc_field.mountain_exposure", 0.62).range(0.05, 4.0).describe("The picture's exposure"))?;
    game_pass::add(
        kit,
        GamePass::fullscreen(SKY_PASS, SKY_WGSL)
            .at(Stage::AfterOpaque)
            .order(5)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(PassParam::color("zenith", [0.42, 0.6, 1.0]).describe("The sky overhead as a share of its colour at the horizon"))
            .param(PassParam::f32("curve", 2.6, 0.5, 8.0).describe("How quickly the sky deepens from the horizon up"))
            .param(PassParam::f32("glow", 0.3, 0.0, 3.0).describe("How much brighter the sky is towards the sun"))
            .param(PassParam::f32("clouds", 0.5, 0.0, 1.2).describe("Where the thin clouds begin (higher: fewer clouds)")),
    )?;
    kit.resource(Mountain { view, yaw, sun_az, sun_el, haze_near, haze_far, exposure, set_for: f32::NAN, summit: [0.0, 1200.0, 0.0], stand: [0.0, 100.0, 2000.0], grass: false, grass_wait: 0 });
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

/// A camera at `eye` looking `pitch` degrees down (negative: up) on heading
/// `yaw` (the direction it looks, degrees from -z towards +x), with a
/// vertical field of view `fov`.
fn look(eye: [f32; 3], yaw: f32, pitch: f32, fov: f32) -> CameraPose {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let d = [y.sin() * p.cos(), -p.sin(), -y.cos() * p.cos()];
    CameraPose { eye, target: [eye[0] + d[0] * 100.0, eye[1] + d[1] * 100.0, eye[2] + d[2] * 100.0], up: [0.0, 1.0, 0.0], fov_y_deg: fov }
}

/// The summit and an open stand on the foothills about 1.75 km south of it
/// (the reference's player: in a meadow at the forest's edge, the summit
/// ahead, a little right of the middle).
fn find_stand(world: &World, yaw: f32) -> Option<([f32; 3], [f32; 3])> {
    let r = world.try_res::<Registry>()?;
    let reg = world.try_res::<ParamRegistry>()?;
    let (mut v, seed) = r.values(&reg, KEY)?;
    // The land as the scene draws it (round 3: running on past the square,
    // its border not settled).
    v.set("horizon", evani_procedural::Value::F32(HORIZON));
    let land = Land::new(&v, seed as u32);
    let s = land.summit();
    let (sy, cy) = yaw.to_radians().sin_cos();
    // Out from the summit towards the cameras' side, then search sideways
    // for the most open ground.
    let (mut best, mut at) = (f32::MAX, [s[0], 0.0, s[2]]);
    // Round 3: 2.0 to 2.35 km out (the reference's player sees the massif
    // across a broad forested valley), inside the square.
    let half = land.size * 0.5 - 60.0;
    for k in 0..64 {
        let dist = 2000.0 + 50.0 * (k / 8) as f32;
        let side = (k % 8) as f32 * 45.0 - 160.0;
        let (x, z) = (s[0] + sy * dist + cy * side, s[2] + cy * dist - sy * side);
        if x.abs() > half || z.abs() > half {
            continue;
        }
        // Open where the player stands (12 m round), the forest close by
        // (45 m round), as the reference's meadow at the forest's edge.
        let ring = |r: f32| (0..6).map(|i| {
            let a = i as f32 * 1.0472;
            land.forest_at(&v, x + r * a.cos(), z + r * a.sin())
        }).sum::<f32>() / 6.0;
        // Round 2: on open meadow, the forest's edge 30 to 70 m off (not
        // crowding the player), clear of the valley's gravel.
        let meadow = if land.meadow_at(&v, x, z) { 0.0 } else { 3.0 };
        let river = if land.river(x, z) < 70.0 { 2.0 } else { 0.0 };
        let score = meadow + river + land.forest_at(&v, x, z) * 3.0 + ring(20.0) * 2.0 + (1.0 - ring(55.0)) * 1.5 + (1.0 - land.normal(x, z)[1]) * 4.0;
        if score < best {
            best = score;
            at = [x, land.height(x, z) + 1.7, z];
        }
    }
    Some((s, at))
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Mountain>();
    let (view, yaw, sun_az, sun_el, near, far, exposure) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 3.0), reg.f32(g.yaw), reg.f32(g.sun_az), reg.f32(g.sun_el), reg.f32(g.haze_near), reg.f32(g.haze_far), reg.f32(g.exposure))
    };
    if view != g.set_for {
        if let Some((s, at)) = find_stand(world, yaw) {
            g.summit = s;
            g.stand = at;
        }
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        // The library's own player focus (round 1): round the stand the
        // forest is full, tree by tree, with boulders and shrubs, as a game
        // sets it round its player.
        set(&mut reg, "procedural.terrain.mountains.focus_x", ParamValue::F32(g.stand[0]));
        set(&mut reg, "procedural.terrain.mountains.focus_z", ParamValue::F32(g.stand[2]));
        set(&mut reg, "procedural.terrain.mountains.focus_radius", ParamValue::F32(FOCUS_RADIUS));
        set(&mut reg, "procedural.terrain.mountains.horizon", ParamValue::F32(HORIZON));
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // Outdoors: the sun from the camera's left a little behind it,
        // mid-morning, warm white; the sky's blue as the ambient and its
        // reflections (no studio panels), soft blue shadow sides.
        let (e, az) = (sun_el.to_radians(), (yaw + sun_az).to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.95, 0.88]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.2));
        // Round 2: a cooler, stronger sky fill and blue in the shadows (the
        // reference's cool faces and blue atmospheric fill), the gullies
        // no longer near-black.
        // Round 3: cooler still (the reference's blue fill on the faces).
        set(&mut reg, "render.ambient", ParamValue::Color([0.46, 0.56, 0.78]));
        set(&mut reg, "render.shadow_tint", ParamValue::Color([0.55, 0.66, 1.0]));
        set(&mut reg, "render.sky.studio", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.studio", ParamValue::F32(0.0));
        set(&mut reg, "render.sky.zenith", ParamValue::Color([0.3, 0.46, 0.85]));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.66, 0.76, 0.92]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.32, 0.36, 0.28]));
        set(&mut reg, "render.sky.sun_glow", ParamValue::F32(0.0));
        // Round 3: softer occlusion (round 2's near-black grooves).
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.3));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.55));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.25));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.1));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.5));
        set(&mut reg, "render.exposure", ParamValue::F32(exposure));
        // The sky: the pale haze colour at the horizon (the backdrop the
        // haze fades the far ranges into); the sky pass deepens it to blue
        // overhead and lays thin clouds on it.
        set(&mut reg, "render.clear_color", ParamValue::Color([0.6, 0.74, 0.95]));
        set(&mut reg, "kit.proc_field.mountain_sky.on", ParamValue::Bool(true));
        // A sun-sized light (sharp shadows at this scale).
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(0.6));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(4.0));
        // The haze over kilometres; the far plane past the ranges.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(near));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(far));
        set(&mut reg, "render.camera.near", ParamValue::F32(if view == 1.0 || view == 3.0 { 0.3 } else { 5.0 }));
        set(&mut reg, "render.camera.far", ParamValue::F32(80_000.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view == 1.0 || view == 3.0 { 3500.0 } else { 7000.0 }));
        // The meadow's real grass (round 1): no carpet of its own (the
        // moss is the ground), reaching across the near meadow.
        set(&mut reg, "material.grass.ground", ParamValue::Bool(false));
        set(&mut reg, "material.grass.distance", ParamValue::F32(1.5));
        // A mountain meadow in early summer: shorter, fresher green than
        // the lowland default, little sun-bleached hay.
        // Round 3: shorter, more uneven tussocks.
        set(&mut reg, "material.grass.meadow.height", ParamValue::F32(0.3));
        set(&mut reg, "material.grass.meadow.height_var", ParamValue::F32(0.8));
        set(&mut reg, "material.grass.meadow.clump", ParamValue::F32(0.8));
        set(&mut reg, "material.grass.meadow.heads", ParamValue::F32(0.22));
        set(&mut reg, "material.grass.meadow.dryness", ParamValue::F32(0.12));
        set(&mut reg, "material.grass.meadow.tip", ParamValue::Color([0.07, 0.17, 0.04]));
        set(&mut reg, "material.grass.meadow.root", ParamValue::Color([0.02, 0.045, 0.014]));
        drop(reg);
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.set_for = view;
        *world.res_mut::<Mountain>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(KEY, [0.0, 0.0, 0.0]);
        // Round 3: one placement; the asset's own land (`horizon`) runs on
        // round it into the hazy ranges.
    }
    let (s, at) = (g.summit, g.stand);
    // Heading from the stand to the summit (the player faces it).
    let to = [s[0] - at[0], s[2] - at[2]];
    // Round 1: real grass on the meadow round the player, once (the
    // library's `meadow_grass`, handed over as a game does).
    if !g.grass {
        if let Some(gr) = world.try_res::<Grass>() {
            let made = match (world.try_res::<Registry>(), world.try_res::<ParamRegistry>()) {
                (Some(r), Some(reg)) => r.values(&reg, KEY),
                _ => None,
            };
            if let Some((v, seed)) = made {
                let l = (to[0] * to[0] + to[1] * to[1]).sqrt().max(1e-3);
                let c = [at[0] + to[0] / l * GRASS_AHEAD, at[2] + to[1] / l * GRASS_AHEAD];
                let (patch, n, h) = mountain::meadow_grass(&v, seed as u32, [c[0] - GRASS_HALF, c[1] - GRASS_HALF], [c[0] + GRASS_HALF, c[1] + GRASS_HALF], [0.0; 3]);
                gr.clear_patches();
                let _ = gr.add_patch_on(patch, n, &h);
                g.grass = true;
            }
        } else {
            // No grass module in this build: stop asking after a while.
            g.grass_wait += 1;
            g.grass = g.grass_wait > 120;
        }
        *world.res_mut::<Mountain>() = g;
    }
    let face = to[0].atan2(-to[1]).to_degrees();
    let cam = match view as u32 {
        // Level at eye height, the summit a little right of the middle and
        // the horizon low: tilted up 12 degrees, a 60 degree view.
        // Round 3: from about 2 km, tilted up 12.5 degrees (the summit near a
        // fifth of the way down, as the reference's).
        1 => look(at, face - 6.0, -12.5, 60.0),
        // The upper slopes: a long lens from the stand.
        2 => look(at, face, -24.0, 18.0),
        // The forest's edge from the meadow, low.
        3 => look(at, face - 70.0, -2.0, 55.0),
        // The aerial overview (round 2): high oblique, 38 degrees down, on
        // the massif's middle from 2.95 km: the whole massif, its ridges
        // and cirques, the foothills and the valley winding towards the
        // camera, the far ranges in the haze along the top.
        // Round 3: 3.5 km out and 550 m over the summit, 30 degrees down:
        // the horizon just in frame at the top, the summit near a sixth of
        // the way down, the foothills and the valley below it.
        _ => {
            let (sy, cy) = yaw.to_radians().sin_cos();
            let eye = [s[0] + sy * AERIAL_OUT, s[1] + AERIAL_OVER, s[2] + cy * AERIAL_OUT];
            look(eye, -yaw + 2.0, AERIAL_PITCH, 60.0)
        }
    };
    let ground = RenderWorld::material(&world.res::<LookData>(), "grass");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // The land beyond the squares: a wooded plain to the horizon, just under
    // the assets' settled edges.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -40.0, 0.0]);
    f.scale = [120_000.0, 1.0, 120_000.0];
    f.material = ground;
    // The wooded plain's dark canopy green (the asset's forest floor), so
    // the squares' edges sink into it under the haze.
    f.color = [0.03, 0.06, 0.026, 1.0];
    rw.draw_mesh(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The mountains scene's parameters and sky pass alone.
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
    fn mountain_sky_pass_checks_clean() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }

}
