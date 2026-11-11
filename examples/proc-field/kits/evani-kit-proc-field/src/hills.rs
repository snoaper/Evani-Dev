//! The rolling hills' live scene (feature `hills`, the Asset Studio rolling
//! hills session, column 28, 2026-11-08): the procedural library's rolling
//! hills (`terrain-rolling-hills`) at a walker's scale, as a game shows them,
//! from a player standing in them, under the reference's camera and light: a
//! standing eye (1.7 m), level, a 60 degree view, the horizon on the upper
//! third, a mid-morning sun from one side and a light haze in the distance.
//! A scene, not a studio: the hills run on to the horizon (the asset's own
//! `horizon`), worn scars with limestone in them on the steeper banks (its
//! `scars`), and its own meadow grass round the player (`hills-grass`). It
//! rides in the mountains scene's slot, as the scree's: `hills` turns
//! `mountain` on for its modules and the camera's raised far plane.
//!
//! The player's stand and heading are found on the land itself (a gentle
//! spot where the hills rise across the middle distance and a scarred bank
//! faces the player), as a game would pick a spawn; `hills_auto` 0 takes the
//! `hills_stand_*` and `hills_heading` params instead.
//!
//! `kit.proc_field.hills_view`: 0 the reference view, 1 the ground at the
//! player's feet (looking down), 2 low along the ground, 3 a plan from
//! overhead (the layout, the stand marked).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::{ProcScene, Registry};
use evani_procedural_lib::evani_grass::Grass;
use evani_procedural_lib::terrain::{Hills, hills_meadow_grass};
use evani_render::game_pass::{self, GamePass, PassParam, Stage};
use evani_render::{MeshId, MeshInstance, RenderWorld};

const KEY: &str = "terrain-rolling-hills";
const PREFIX: &str = "procedural.terrain.rolling_hills";

/// The hills at a walker's scale (the params' largest square and hill
/// spacing): the square (m), the tallest hill over the lowest ground (m),
/// hilltop to hilltop (m), and how far the land runs on.
const SIZE: f32 = 400.0;
const HEIGHT: f32 = 34.0;
const FEATURE: f32 = 200.0;
const HORIZON: f32 = 9000.0;
/// The meadow grass round the stand: its square's half width and how far
/// ahead of the player its middle lies (m).
const GRASS_HALF: f32 = 60.0;
const GRASS_AHEAD: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
pub struct HillsScene {
    view: F32Param,
    auto: F32Param,
    stand_x: F32Param,
    stand_z: F32Param,
    heading: F32Param,
    pitch: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    exposure: F32Param,
    haze_near: F32Param,
    haze_far: F32Param,
    /// The stand and light the scene was last set up for (NaN: not yet).
    set_for: [f32; 4],
    lit_for: [f32; 6],
    /// The player's eye and heading (degrees from -z towards +x).
    eye: [f32; 3],
    face: f32,
    /// Whether the meadow grass round the stand was handed to evani-grass,
    /// and the frames spent waiting for its resource.
    grass: bool,
    grass_wait: u32,
    /// Frames to wait after the hills' params change before placing them,
    /// so the library makes them once, with every param already set.
    wait: u32,
}

/// The sky pass (the scree's and mountains' scenes', columns 19 and 27):
/// where nothing was drawn, a clear mid-morning sky from the hazy horizon to
/// a deeper blue overhead, brighter towards the sun, with fair-weather
/// clouds.
const SKY_PASS: &str = "proc_field.hills_sky";
pub(crate) const SKY_WGSL: &str = "
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
    // Fair-weather clouds on a flat layer overhead: soft-edged fbm with
    // bright tops, thinning towards the horizon's haze.
    if (r.y > 0.02) {
        let q = r.xz / r.y * 0.9;
        var n = 0.0;
        var a = 0.5;
        var p = q;
        for (var i = 0; i < 6; i = i + 1) {
            n = n + a * sky_noise(p);
            p = p * 2.03 + vec2<f32>(5.2, 1.3);
            a = a * 0.5;
        }
        let cover = smoothstep(param_clouds(), param_clouds() + 0.12, n) * smoothstep(0.02, 0.16, r.y);
        let body = hz * mix(1.45, 1.1, smoothstep(0.7, 1.0, n));
        s = mix(s, body, cover * 0.95);
    }
    return vec4<f32>(s, 1.0);
}
";

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let p = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(&format!("kit.proc_field.hills_{name}"), v).range(lo, hi).describe(what);
    let view = kit.param_f32(p("view", 0.0, 0.0, 3.0, "0 the reference view (a standing player in the hills), 1 the ground at the player's feet, 2 low along the ground, 3 a plan from overhead (the layout)").step(1.0))?;
    let auto = kit.param_f32(p("auto", 1.0, 0.0, 1.0, "1: find the player's stand and heading on the land; 0: take the stand and heading params").step(1.0))?;
    let stand_x = kit.param_f32(p("stand_x", 0.0, -190.0, 190.0, "Where the player stands (m, x; with auto 0)"))?;
    let stand_z = kit.param_f32(p("stand_z", 0.0, -190.0, 190.0, "Where the player stands (m, z; with auto 0)"))?;
    let heading = kit.param_f32(p("heading", 0.0, -180.0, 180.0, "The player's heading (degrees from -z towards +x; with auto 0)"))?;
    let pitch = kit.param_f32(p("pitch", 7.3, -30.0, 60.0, "How far the player looks down (degrees; 7.3: the horizon on the upper third)"))?;
    let sun_az = kit.param_f32(p("sun_az", 105.0, -180.0, 180.0, "The sun's bearing from the player's heading (degrees; positive: from the right)"))?;
    let sun_el = kit.param_f32(p("sun_el", 26.0, 5.0, 89.0, "The sun's elevation (degrees; mid-morning)"))?;
    let exposure = kit.param_f32(p("exposure", 0.66, 0.05, 4.0, "The picture's exposure"))?;
    let haze_near = kit.param_f32(p("haze_near", 250.0, 0.0, 20000.0, "Where the distance haze begins (m)"))?;
    let haze_far = kit.param_f32(p("haze_far", 14000.0, 100.0, 100000.0, "Where the haze has hidden the land in the sky's colour (m)"))?;
    game_pass::add(
        kit,
        GamePass::fullscreen(SKY_PASS, SKY_WGSL)
            .at(Stage::AfterOpaque)
            .order(5)
            .reads("scene", "hdr")
            .reads_depth("depth")
            .param(PassParam::color("zenith", [0.3, 0.5, 1.0]).describe("The sky overhead as a share of its colour at the horizon"))
            .param(PassParam::f32("curve", 2.4, 0.5, 8.0).describe("How quickly the sky deepens from the horizon up"))
            .param(PassParam::f32("glow", 0.3, 0.0, 3.0).describe("How much brighter the sky is towards the sun"))
            .param(PassParam::f32("clouds", 0.66, 0.0, 1.2).describe("Where the clouds begin (higher: fewer clouds)")),
    )?;
    kit.resource(HillsScene {
        view,
        auto,
        stand_x,
        stand_z,
        heading,
        pitch,
        sun_az,
        sun_el,
        exposure,
        haze_near,
        haze_far,
        set_for: [f32::NAN; 4],
        lit_for: [f32::NAN; 6],
        eye: [0.0, 20.0, 0.0],
        face: 0.0,
        grass: false,
        grass_wait: 0,
        wait: 0,
    });
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

/// The hills as the scene sets them.
fn land(world: &World) -> Option<(evani_procedural::Values, u64)> {
    let r = world.try_res::<Registry>()?;
    let reg = world.try_res::<ParamRegistry>()?;
    r.values(&reg, KEY)
}

/// A player's stand and heading on the hills, as a game picks a spawn: on
/// gentle ground in the square's middle, where the land ahead rises a few
/// degrees over the eye across the middle distance (the hills fill it, the
/// sky above them) without a bank close in front blocking the view, and a
/// worn scar on a bank in view, facing the player.
fn find_stand(l: &Hills) -> ([f32; 3], f32) {
    let (mut best, mut at) = (f32::MIN, ([0.0, l.ground(0.0, 0.0) + 1.7, 0.0], 0.0));
    let reach = l.size * 0.32;
    for jz in 0..15 {
        for jx in 0..15 {
            let (x, z) = (-reach + 2.0 * reach * jx as f32 / 14.0, -reach + 2.0 * reach * jz as f32 / 14.0);
            if l.steep(x, z) > 0.03 {
                continue;
            }
            let eye = l.ground(x, z) + 1.7;
            for k in 0..12 {
                let yaw = k as f32 * 30.0;
                // The land's rise over the eye across the view (degrees), near
                // (25 to 90 m) and in the middle distance (110 to 650 m).
                let (mut near, mut mid, mut scars) = (f32::MIN, f32::MIN, 0u32);
                for a in [-24.0f32, -12.0, 0.0, 12.0, 24.0] {
                    let (s, c) = (yaw + a).to_radians().sin_cos();
                    for d in [25.0f32, 45.0, 70.0, 90.0, 110.0, 160.0, 220.0, 300.0, 400.0, 520.0, 650.0] {
                        let (px, pz) = (x + s * d, z - c * d);
                        let g = l.ground(px, pz);
                        let rise = ((g - eye) / d).atan().to_degrees();
                        if d <= 90.0 {
                            near = near.max(rise);
                        } else {
                            mid = mid.max(rise);
                        }
                        // A scar in view (60 to 300 m) on a bank facing the
                        // player: the ground climbs away from the eye there.
                        if (60.0..=300.0).contains(&d) {
                            let up = l.ground(px + s * 4.0, pz - c * 4.0) - l.ground(px - s * 4.0, pz + c * 4.0);
                            if up > 0.0 && l.scar_at(px, pz) {
                                scars += 1;
                            }
                        }
                    }
                }
                let score = 3.0 - (mid - 3.2).abs() - 2.0 * (near - 2.0).max(0.0) + 0.6 * scars.min(5) as f32;
                if score > best {
                    best = score;
                    at = ([x, eye, z], yaw);
                }
            }
        }
    }
    at
}

/// A 60 degree horizontal view at 3:2 as a vertical field of view.
const FOV_Y: f32 = 42.1;

pub fn draw(world: &mut World) {
    let mut g = *world.res::<HillsScene>();
    let (view, auto, sx, sz, hd, pitch, sun_az, sun_el, exposure, near, far) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 3.0), reg.f32(g.auto).round(), reg.f32(g.stand_x), reg.f32(g.stand_z), reg.f32(g.heading), reg.f32(g.pitch), reg.f32(g.sun_az), reg.f32(g.sun_el), reg.f32(g.exposure), reg.f32(g.haze_near), reg.f32(g.haze_far))
    };
    let key = [auto, sx, sz, hd];
    if key != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        // The hills at a walker's scale, running on to the horizon, worn
        // scars and their limestone on the steeper banks.
        for (n, v) in [("size", SIZE), ("height", HEIGHT), ("feature_size", FEATURE), ("roughness", 0.42), ("octaves", 5.0), ("resolution", 200.0), ("edge", 0.0), ("outcrops", 1.0), ("horizon", HORIZON), ("scars", 0.55)] {
            set(&mut reg, &format!("{PREFIX}.{n}"), ParamValue::F32(v));
        }
        drop(reg);
        // Until the library can say where the hills lie, try again next frame.
        let Some((v, seed)) = land(world) else { return };
        let l = Hills::new(&v, seed);
        (g.eye, g.face) = if auto >= 0.5 { find_stand(&l) } else { ([sx, l.ground(sx, sz) + 1.7, sz], hd) };
        if let Some(mut s) = world.try_res_mut::<ProcScene>() {
            s.clear();
        }
        g.wait = 3;
        g.grass = false;
        g.set_for = key;
    }
    let lit = [view, g.face + sun_az, sun_el, exposure, near, far];
    if lit != g.lit_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // Outdoors, mid-morning: a warm white sun from the player's side, the
        // sky's blue as the ambient and in the shadows (no studio panels).
        let (e, az) = (sun_el.to_radians(), (g.face + sun_az).to_radians());
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
        // fades the far hills into); the sky pass deepens it to blue overhead
        // and lays fair-weather clouds on it.
        set(&mut reg, "render.clear_color", ParamValue::Color([0.52, 0.68, 0.95]));
        set(&mut reg, "kit.proc_field.hills_sky.on", ParamValue::Bool(true));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(0.6));
        set(&mut reg, "look.stylizedpbr.pcss_search", ParamValue::F32(4.0));
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(near));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(far));
        set(&mut reg, "render.camera.near", ParamValue::F32(if view == 3.0 { 5.0 } else { 0.12 }));
        set(&mut reg, "render.camera.far", ParamValue::F32(80_000.0));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view == 3.0 { 1500.0 } else { 500.0 }));
        // The meadow's real grass: no carpet of its own (the turf is the
        // ground), reaching across the near meadow. A grazed hill pasture:
        // short, uneven tussocks, green with sun-bleached, dry blades and
        // seed heads among them.
        set(&mut reg, "material.grass.ground", ParamValue::Bool(false));
        set(&mut reg, "material.grass.distance", ParamValue::F32(1.6));
        set(&mut reg, "material.grass.meadow.height", ParamValue::F32(0.3));
        set(&mut reg, "material.grass.meadow.height_var", ParamValue::F32(0.75));
        set(&mut reg, "material.grass.meadow.clump", ParamValue::F32(0.75));
        set(&mut reg, "material.grass.meadow.heads", ParamValue::F32(0.25));
        set(&mut reg, "material.grass.meadow.dryness", ParamValue::F32(0.65));
        set(&mut reg, "material.grass.meadow.tip", ParamValue::Color([0.16, 0.22, 0.055]));
        set(&mut reg, "material.grass.meadow.root", ParamValue::Color([0.035, 0.05, 0.016]));
        set(&mut reg, "material.grass.meadow.dry", ParamValue::Color([0.55, 0.45, 0.2]));
        drop(reg);
        g.lit_for = lit;
    }
    if g.wait > 0 {
        g.wait -= 1;
    } else if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place(KEY, [0.0, 0.0, 0.0]);
    }
    // Real grass on the meadow round the player, once (the library's
    // `hills_meadow_grass`, handed over as a game does).
    if !g.grass {
        if let Some(gr) = world.try_res::<Grass>() {
            if let Some((v, seed)) = land(world) {
                let (s, c) = g.face.to_radians().sin_cos();
                let m = [g.eye[0] + s * GRASS_AHEAD, g.eye[2] - c * GRASS_AHEAD];
                let (patch, n, h) = hills_meadow_grass(&v, seed, [m[0] - GRASS_HALF, m[1] - GRASS_HALF], [m[0] + GRASS_HALF, m[1] + GRASS_HALF], [0.0; 3]);
                gr.clear_patches();
                let _ = gr.add_patch_on(patch, n, &h);
                g.grass = true;
            }
        } else {
            // No grass module in this build: stop asking after a while.
            g.grass_wait += 1;
            g.grass = g.grass_wait > 120;
        }
    }
    *world.res_mut::<HillsScene>() = g;
    let cam = match view as u32 {
        // The ground at the player's feet.
        1 => look(g.eye, g.face + 20.0, 50.0, 40.0),
        // Low along the ground.
        2 => look([g.eye[0], g.eye[1] - 1.2, g.eye[2]], g.face - 6.0, 1.0, FOV_Y),
        // A plan from overhead: the layout round the player's stand.
        3 => CameraPose { eye: [g.eye[0], g.eye[1] + 700.0, g.eye[2]], target: [g.eye[0], g.eye[1], g.eye[2] - 0.01], up: [0.0, 0.0, -1.0], fov_y_deg: 60.0 },
        _ => look(g.eye, g.face, pitch, FOV_Y),
    };
    let ground = RenderWorld::material(&world.res::<LookData>(), "grass");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    // Past the hills' own horizon: a plain under the haze.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -2.0, 0.0]);
    f.scale = [120_000.0, 1.0, 120_000.0];
    f.material = ground;
    f.color = [0.05, 0.07, 0.03, 1.0];
    rw.draw_mesh(f);
    if view == 3.0 {
        // The stand, marked, and the heading.
        let mut m = MeshInstance::new(MeshId::CUBE, g.eye);
        m.scale = [8.0, 30.0, 8.0];
        m.color = [1.0, 0.0, 0.8, 1.0];
        rw.draw_mesh(m);
        let (s, c) = g.face.to_radians().sin_cos();
        let mut a = MeshInstance::new(MeshId::CUBE, [g.eye[0] + s * 60.0, g.eye[1], g.eye[2] - c * 60.0]);
        a.scale = [5.0, 30.0, 5.0];
        a.color = [0.0, 0.9, 1.0, 1.0];
        rw.draw_mesh(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::plugin::KitPlugin;

    /// The hills scene's parameters and sky pass alone.
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
    fn hills_sky_pass_checks_clean() {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(Scene);
        a.start();
        assert_eq!(game_pass::check(&a.world), Ok(1));
    }
}
