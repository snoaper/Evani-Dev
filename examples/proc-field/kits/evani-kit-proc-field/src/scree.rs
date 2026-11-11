//! The scree slopes' live scene (feature `scree`, the Asset Studio scree
//! slopes session, column 27, 2026-11-08): the procedural library's scree
//! slope (`rocks-scree-slopes`) as a game shows it, from a player standing
//! on it, under its reference picture's camera and light: a standing eye
//! (1.7 m), level heading along the slope a little downhill, a 60 degree
//! view, the horizon on the upper third, a mid-morning sun. A scene, not a
//! studio: round 2, the slope's foot turns round a cirque's bowl (the
//! asset's own `curve`) and runs out into its debris apron, the valley floor
//! runs on to the horizon and the library's mountain massif stands across
//! the valley, under a sky pass of bright blue and big fair-weather clouds
//! (it rides in the mountains scene's slot: `scree` turns `mountain` on for
//! its modules and the camera's raised far plane).
//!
//! `kit.proc_field.scree_view`: 0 the reference view, 1 the stones at the
//! player's feet (looking down), 2 low along the slope.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::{ProcScene, Registry};
use evani_procedural_lib::scree::Slope;
use evani_render::game_pass::{self, GamePass, PassParam, Stage};
use evani_render::{MeshId, MeshInstance, RenderWorld};

const KEY: &str = "rocks-scree-slopes";
const MOUNTAINS: &str = "terrain-mountains";
const PREFIX: &str = "procedural.rocks.scree_slopes";

/// The scene's slope: width along its foot, its run from the wall's foot to
/// the toe, its apron and how far its foot turns round (m, m, m, degrees).
const SIZE: f32 = 300.0;
const LENGTH: f32 = 90.0;
const APRON: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
pub struct Scree {
    view: F32Param,
    stand_u: F32Param,
    stand_z: F32Param,
    heading: F32Param,
    pitch: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    exposure: F32Param,
    mtn_bearing: F32Param,
    mtn_dist: F32Param,
    curve: F32Param,
    /// How far the slope has turned at the player (degrees).
    turn: f32,
    /// The massif's summit in its own frame (x, z).
    peak: [f32; 2],
    /// The view and stand the scene was last set up for, and the light
    /// (NaN: not yet).
    set_for: [f32; 4],
    lit_for: [f32; 4],
    /// The player's eye.
    eye: [f32; 3],
    /// Frames to wait after the slope's params change before placing it,
    /// so the library makes it once, with the player's point already set.
    wait: u32,
}

/// The sky pass (the mountains scene's, column 19): where nothing was
/// drawn, a clear mid-morning sky from the hazy horizon to a deeper blue
/// overhead, brighter towards the sun, with fair-weather clouds.
const SKY_PASS: &str = "proc_field.scree_sky";
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
    // Round 3: heaped fair-weather cumulus over the horizon, laid out in
    // azimuth and elevation (so they are not smeared flat by the
    // perspective): a flat grey base, rounded bright tops lit from the sun's
    // side, their threshold rising with height so the tops round off.
    let el = asin(clamp(r.y, -1.0, 1.0));
    let az = atan2(r.x, -r.z);
    if (el > 0.0) {
        var n = 0.0;
        var a = 0.5;
        var p = vec2<f32>(az * 3.4, el * 7.5);
        for (var i = 0; i < 6; i = i + 1) {
            n = n + a * sky_noise(p);
            p = p * 2.07 + vec2<f32>(3.1, 7.7);
            a = a * 0.5;
        }
        let base = 0.05 + 0.05 * sky_noise(vec2<f32>(az * 1.3, 4.0));
        let h = (el - base) / 0.24;
        let d = n - param_clouds() - 0.32 * h * h;
        let cover = smoothstep(0.0, 0.05, d) * smoothstep(0.0, 0.03, el - base);
        let side = max(dot(normalize(vec2<f32>(r.x, r.z)), normalize(vec2<f32>(frame.sun.x, frame.sun.z))), 0.0);
        let shade = mix(0.7, 1.0, clamp(h * 1.4 + 0.25 * side + 0.15, 0.0, 1.0));
        let white = vec3<f32>(1.0, 0.99, 0.96) * 1.3 * shade;
        s = mix(s, white, cover);
    }
    return vec4<f32>(s, 1.0);
}
";

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let p = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(&format!("kit.proc_field.scree_{name}"), v).range(lo, hi).describe(what);
    let view = kit.param_f32(p("view", 0.0, 0.0, 3.0, "0 the reference view (a standing player on the slope), 1 the stones at the player's feet, 2 low along the slope, 3 a plan from overhead (the layout)").step(1.0))?;
    let stand_u = kit.param_f32(p("stand_u", 3.5, 1.0, 40.0, "How far below the wall's foot the player stands (m, level)"))?;
    let stand_z = kit.param_f32(p("stand_z", 100.0, -150.0, 150.0, "Where along the slope's foot the player stands (m from its middle, in the slope's own frame)"))?;
    let heading = kit.param_f32(p("heading", 15.0, -60.0, 60.0, "The player's heading off the slope's run (degrees; positive: downhill)"))?;
    let pitch = kit.param_f32(p("pitch", 7.0, -30.0, 60.0, "How far the player looks down (degrees; 7: the horizon on the upper third)"))?;
    let sun_az = kit.param_f32(p("sun_az", 140.0, -180.0, 180.0, "The sun's bearing from the player's heading (degrees; negative: from the left)"))?;
    let sun_el = kit.param_f32(p("sun_el", 40.0, 5.0, 89.0, "The sun's elevation (degrees; mid-morning)"))?;
    let exposure = kit.param_f32(p("exposure", 0.46, 0.05, 4.0, "The picture's exposure"))?;
    let mtn_bearing = kit.param_f32(p("mtn_bearing", 8.0, -90.0, 90.0, "The far massif's summit's bearing from the player's heading (degrees)"))?;
    let mtn_dist = kit.param_f32(p("mtn_dist", 1700.0, 500.0, 20000.0, "How far off the far massif's summit stands (m)"))?;
    let curve = kit.param_f32(p("curve", 50.0, 0.0, 180.0, "How far the slope's foot turns round the cirque (degrees, the asset's `curve`)"))?;
    game_pass::add(
        kit,
        GamePass::fullscreen(SKY_PASS, SKY_WGSL)
            .at(Stage::AfterOpaque)
            .order(5)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(PassParam::color("zenith", [0.4, 0.62, 1.0]).describe("The sky overhead as a share of its colour at the horizon"))
            .param(PassParam::f32("curve", 2.2, 0.5, 8.0).describe("How quickly the sky deepens from the horizon up"))
            .param(PassParam::f32("glow", 0.25, 0.0, 3.0).describe("How much brighter the sky is towards the sun"))
            .param(PassParam::f32("clouds", 0.4, 0.0, 1.2).describe("Where the cumulus begin (higher: fewer clouds)")),
    )?;
    kit.resource(Scree { view, stand_u, stand_z, heading, pitch, sun_az, sun_el, exposure, mtn_bearing, mtn_dist, curve, turn: 0.0, peak: [0.0; 2], set_for: [f32::NAN; 4], lit_for: [f32::NAN; 4], eye: [0.0, 30.0, 40.0], wait: 0 });
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

/// The slope as the scene sets it (the asset's own layout, for the stand).
fn slope(world: &World) -> Option<Slope> {
    let r = world.try_res::<Registry>()?;
    let reg = world.try_res::<ParamRegistry>()?;
    let (v, seed) = r.values(&reg, KEY)?;
    Some(Slope::new(&v, seed as u32))
}

/// The massif's summit (x, z in its own frame) for the values it is set to.
fn massif_peak(world: &World) -> Option<[f32; 2]> {
    let r = world.try_res::<Registry>()?;
    let reg = world.try_res::<ParamRegistry>()?;
    let (v, seed) = r.values(&reg, MOUNTAINS)?;
    let p = evani_procedural_lib::mountain::Land::new(&v, seed as u32).summit();
    Some([p[0], p[2]])
}

/// A 60 degree horizontal view at 3:2 as a vertical field of view.
const FOV_Y: f32 = 42.1;

pub fn draw(world: &mut World) {
    let mut g = *world.res::<Scree>();
    let (view, u, z, heading, pitch, sun_az, sun_el, exposure, mb, md, curve) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 3.0), reg.f32(g.stand_u), reg.f32(g.stand_z), reg.f32(g.heading), reg.f32(g.pitch), reg.f32(g.sun_az), reg.f32(g.sun_el), reg.f32(g.exposure), reg.f32(g.mtn_bearing), reg.f32(g.mtn_dist), reg.f32(g.curve))
    };
    let key = [view, u, z, curve];
    let lit = [heading, sun_az, sun_el, exposure];
    if key != g.set_for || lit != g.lit_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, &format!("{PREFIX}.size"), ParamValue::F32(SIZE));
        set(&mut reg, &format!("{PREFIX}.length"), ParamValue::F32(LENGTH));
        set(&mut reg, &format!("{PREFIX}.apron"), ParamValue::F32(APRON));
        set(&mut reg, &format!("{PREFIX}.curve"), ParamValue::F32(curve));
        // The far massif: bare rock, scree and snow above the valley, no
        // forest (an alpine valley over the tree line).
        set(&mut reg, "procedural.terrain.mountains.forest", ParamValue::F32(0.0));
        set(&mut reg, "procedural.terrain.mountains.snow", ParamValue::F32(0.2));
        set(&mut reg, "procedural.terrain.mountains.tree_line", ParamValue::F32(0.05));
        set(&mut reg, "procedural.terrain.mountains.erosion", ParamValue::F32(1.0));
        set(&mut reg, "procedural.terrain.mountains.rock", ParamValue::F32(1.0));
        set(&mut reg, "procedural.terrain.mountains.ruggedness", ParamValue::F32(0.9));
        set(&mut reg, "procedural.terrain.mountains.height", ParamValue::F32(380.0));
        set(&mut reg, "procedural.terrain.mountains.size", ParamValue::F32(3200.0));
        drop(reg);
        // The stand: `u` m below the wall's foot, `z` along the slope; the
        // library's own player focus round it (every stone drawn there).
        // Until the library can say where the slope lies, try again next
        // frame (round 1's first picture stood the player in the air).
        let Some(sl) = slope(world) else { return };
        let x = sl.head().0 + u;
        // In the slope's frame, then bent round with it (round 2).
        g.eye = sl.warp([x, sl.ground(x, z) + 1.7, z]);
        g.turn = sl.turn(z).to_degrees();
        // The massif's summit, for aiming it (round 3).
        if let Some(pk) = massif_peak(world) {
            g.peak = pk;
        }
        let mut reg = world.res_mut::<ParamRegistry>();
        // The focus a few metres ahead of the player, a little downhill.
        set(&mut reg, &format!("{PREFIX}.focus_x"), ParamValue::F32(x + 2.0));
        set(&mut reg, &format!("{PREFIX}.focus_z"), ParamValue::F32(z - 6.0));
        set(&mut reg, &format!("{PREFIX}.focus_radius"), ParamValue::F32(26.0));
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // Outdoors, mid-morning: a warm white sun, the sky's blue as the
        // ambient and in the shadows (no studio panels).
        let (e, az) = (sun_el.to_radians(), (heading - g.turn + sun_az).to_radians());
        // The light's direction is towards the sun; the camera's heading is
        // from -z towards +x.
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), -e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.95, 0.88]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.3));
        set(&mut reg, "render.ambient", ParamValue::Color([0.46, 0.55, 0.74]));
        set(&mut reg, "render.shadow_tint", ParamValue::Color([0.55, 0.66, 1.0]));
        set(&mut reg, "render.sky.studio", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.studio", ParamValue::F32(0.0));
        set(&mut reg, "render.sky.zenith", ParamValue::Color([0.3, 0.46, 0.85]));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.66, 0.76, 0.92]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.3, 0.29, 0.26]));
        set(&mut reg, "render.sky.sun_glow", ParamValue::F32(0.0));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.9));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.5));
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.25));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(0.1));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.5));
        set(&mut reg, "render.exposure", ParamValue::F32(exposure));
        set(&mut reg, "render.clear_color", ParamValue::Color([0.6, 0.8, 1.1]));
        set(&mut reg, "kit.proc_field.scree_sky.on", ParamValue::Bool(true));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(0.6));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(4.0));
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(250.0));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(16000.0));
        set(&mut reg, "render.camera.near", ParamValue::F32(0.15));
        set(&mut reg, "render.camera.far", ParamValue::F32(80_000.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(220.0));
        drop(reg);
        if key != g.set_for {
            if let Some(mut s) = world.try_res_mut::<ProcScene>() {
                s.clear();
            }
            g.wait = 3;
        }
        g.set_for = key;
        g.lit_for = lit;
        *world.res_mut::<Scree>() = g;
    }
    if g.wait > 0 {
        g.wait -= 1;
        *world.res_mut::<Scree>() = g;
    } else if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        // One slope, its foot turned round the cirque by the asset itself.
        s.place(KEY, [0.0, 0.0, 0.0]);
        // The far massif across the valley.
        // Its summit at the bearing and distance asked (round 3).
        let b = (heading - g.turn + mb).to_radians();
        let at = [g.eye[0] + b.sin() * md, g.eye[2] - b.cos() * md];
        let peak = g.peak;
        s.place(MOUNTAINS, [at[0] - peak[0], 0.0, at[1] - peak[1]]);
    }
    // The heading in the world: the slope has turned under the player.
    let heading = heading - g.turn;
    let cam = match view as u32 {
        // The stones at the player's feet.
        1 => look(g.eye, heading + 25.0, 48.0, 38.0),
        // Low along the slope.
        2 => look([g.eye[0], g.eye[1] - 1.1, g.eye[2]], heading - 4.0, 2.0, FOV_Y),
        // A plan from overhead: the layout round the player's stand.
        3 => CameraPose { eye: [g.eye[0], g.eye[1] + 30.0, g.eye[2]], target: [g.eye[0], g.eye[1], g.eye[2] - 0.01], up: [0.0, 0.0, -1.0], fov_y_deg: 50.0 },
        _ => look(g.eye, heading, pitch, FOV_Y),
    };
    let ground = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // The valley floor to the horizon: alpine meadow, just under the
    // slope's toe.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.05, 0.0]);
    f.scale = [120_000.0, 1.0, 120_000.0];
    f.material = ground;
    f.color = [0.125, 0.135, 0.1, 1.0];
    rw.draw_mesh(f);
    if view == 3.0 {
        // The stand, marked.
        let mut m = MeshInstance::new(MeshId::CUBE, g.eye);
        m.scale = [1.5, 6.0, 1.5];
        m.color = [1.0, 0.0, 0.8, 1.0];
        rw.draw_mesh(m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The scree scene's parameters and sky pass alone.
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
    fn scree_sky_pass_checks_clean() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }
}
