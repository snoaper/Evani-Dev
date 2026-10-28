//! The ChimpDuty intake proofs (sandbox features `render-limits-before` and
//! `render-limits-proof`; never in evani.toml, so the default and ship
//! builds are unchanged). One scene per engine limit, picked by the page
//! address (`?rl=<scene>`); the same scenes build twice, once without the new
//! engine features (`render-limits-before`) and once with them
//! (`render-limits-proof`), so each picture has a true before and after.
//!
//! * `near`: the eye 6 cm from a wall, a first-person stand-in model held in
//!   front (limits 1a and 1b). Before: the 0.1 m near plane cuts the wall
//!   away and the held model is a world draw. After: `render.camera.near`
//!   0.01 keeps the wall, and `Models::draw_view_model` keeps the held model
//!   on top (`&vm=world` draws it in the world instead, inside the wall).
//! * `fog`: the plain grid; the proof look asks for fog with
//!   `LookContext::request_effect` (limit 2). Before: ignored (no tier starts
//!   fog). After: on in Medium and up, vetoed by Low.
//! * `haze`: a low view to a bright horizon (limit 3): the background and the
//!   haze at HDR values with a fog colour of its own. Before: both clamp to
//!   1 and the fog takes the background colour.
//! * `pcss`: a body lying on the floor under a low sun, soft shadows on
//!   (limit 5). Before: the PCSS speckle on the floor; after: `pcss-stable`.
//! * `warm`: the plain grid, plus kit material code set on the first frame,
//!   as ChimpDuty's material-proc does (limit 4). The proof measures the
//!   frame gaps after the first drawn frame. After: `prewarm` builds it all
//!   behind the loading state first.
//!
//! The page title carries the numbers (`rl ...`) for the headless harness.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::effects::{EffectsStatus, StyleClasses};
use evani_core::overlay::RenderStats;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::{LookContext, LookData, LookPlugin, PluginError};
use evani_render::kit_material::KitMaterial;
use evani_render::model::{ModelHandle, ModelInstance, Models};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The proof scenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// No `rl` in the address: the sandbox as it is.
    Grid,
    Near,
    Fog,
    Haze,
    Pcss,
    Warm,
}

/// The value of `key` in a page address (`?a=1&key=v#...`).
pub fn query_value<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let q = url.split_once('?')?.1;
    let q = q.split('#').next().unwrap_or(q);
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then_some(v)
    })
}

/// The scene a page address asks for.
pub fn scene_of(url: Option<&str>) -> Scene {
    match url.and_then(|u| query_value(u, "rl")) {
        Some("near") => Scene::Near,
        Some("fog") => Scene::Fog,
        Some("haze") => Scene::Haze,
        Some("pcss") => Scene::Pcss,
        Some("warm") => Scene::Warm,
        _ => Scene::Grid,
    }
}

/// The page address (wasm; `None` natively).
fn page_url() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.document()?.url().ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// The proof's state (a world resource; tests insert one with the scene set).
#[derive(Debug, Clone)]
pub struct ProofLimits {
    pub scene: Scene,
    /// `&vm=world`: the held model as a world draw even where the layer exists.
    pub vm_world: bool,
    pub frames: u32,
    /// When the first frame with draws was seen (ms, `now_ms`).
    pub first_draw_ms: Option<f64>,
    /// Frame gaps in the 4 s after the first drawn frame.
    pub max_gap_ms: f64,
    pub gaps_over_50: u32,
    pub gaps_over_100: u32,
    last_ms: Option<f64>,
    held: Option<ModelHandle>,
}

impl ProofLimits {
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            vm_world: false,
            frames: 0,
            first_draw_ms: None,
            max_gap_ms: 0.0,
            gaps_over_50: 0,
            gaps_over_100: 0,
            last_ms: None,
            held: None,
        }
    }
}

/// The proof look: layered over the basic look (same name, points and
/// styles); it asks for fog in the `fog` and `haze` scenes. Without
/// evani-fx's `look-effects` (the before build) the request is never read.
pub struct ProofLook;

impl LookPlugin for ProofLook {
    fn name(&self) -> &'static str {
        "basic"
    }
    fn points(&self) -> u32 {
        1
    }
    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }
    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        if matches!(scene_of(page_url().as_deref()), Scene::Fog | Scene::Haze) {
            look.request_effect("fog");
        }
        Ok(())
    }
}

/// Kit material code like ChimpDuty's procedural surfaces: a few octaves of
/// value noise tinting every mesh (set on the first frame of `warm`).
pub const WARM_WGSL: &str = r#"
fn rl_hash(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}
fn rl_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(rl_hash(i), rl_hash(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(rl_hash(i + vec3<f32>(0.0, 1.0, 0.0)), rl_hash(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(rl_hash(i + vec3<f32>(0.0, 0.0, 1.0)), rl_hash(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(rl_hash(i + vec3<f32>(0.0, 1.0, 1.0)), rl_hash(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}
fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
    var sum = 0.0;
    var amp = 0.5;
    var q = p * 0.7 + n * 0.3;
    for (var o = 0; o < 6; o = o + 1) {
        sum = sum + amp * rl_noise(q);
        q = q * 2.03 + vec3<f32>(1.7, 9.2, 3.1);
        amp = amp * 0.5;
    }
    let tint = mix(vec3<f32>(0.80, 0.77, 0.72), vec3<f32>(1.05, 1.03, 1.0), sum);
    return vec4<f32>(tint, 1.0);
}
"#;

/// The `Stage::Frame` system (registered after the kit and the model proof,
/// so its scenes replace what they drew).
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_limits", Stage::Frame, run)
}

fn set(world: &World, name: &str, v: ParamValue) {
    // A param the build lacks (the before build) is simply not there.
    let _ = world.res_mut::<ParamRegistry>().set_by_name(name, v);
}

fn first_frame(world: &mut World, p: &ProofLimits) {
    match p.scene {
        Scene::Near => {
            // Limit 1a: a first-person near plane (only in the after build).
            set(world, "render.camera.near", ParamValue::F32(0.01));
        }
        Scene::Haze => {
            // Limit 3: a bright HDR horizon, the haze a little dimmer and
            // bluer, its own colour (both clamp to 1 in the before build).
            set(world, "render.clear_color", ParamValue::Vec3([3.2, 2.5, 1.7]));
            set(world, "render.fog.own_color", ParamValue::Bool(true));
            set(world, "render.fog.color", ParamValue::Vec3([2.2, 2.0, 1.8]));
            set(world, "fx.fog.density", ParamValue::F32(0.02));
        }
        Scene::Pcss => {
            // Limit 5: a low sun (about 14 degrees up) from the side.
            set(world, "render.light.direction", ParamValue::Vec3([0.9, 0.25, 0.35]));
        }
        Scene::Warm => {
            // Limit 4: kit code on the first frame, as material-proc does.
            if let Some(mut km) = world.try_res_mut::<KitMaterial>() {
                km.set_wgsl(WARM_WGSL);
            }
        }
        Scene::Grid | Scene::Fog => {}
    }
}

fn run(world: &mut World) {
    if world.try_res::<ProofLimits>().is_none() {
        let url = page_url();
        let mut p = ProofLimits::new(scene_of(url.as_deref()));
        p.vm_world = url.as_deref().and_then(|u| query_value(u, "vm")) == Some("world");
        world.insert_resource(p);
    }
    let Some(mut p) = world.try_res_mut::<ProofLimits>().map(|p| p.clone()) else { return };
    if p.frames == 0 {
        first_frame(world, &p);
    }
    p.frames = p.frames.saturating_add(1);
    measure(world, &mut p);
    match p.scene {
        Scene::Near => near(world, &mut p),
        Scene::Haze => haze(world),
        Scene::Pcss => pcss(world),
        Scene::Grid | Scene::Fog | Scene::Warm => {}
    }
    if p.scene != Scene::Grid && p.frames % 10 == 0 {
        report(world, &p);
    }
    if let Some(mut r) = world.try_res_mut::<ProofLimits>() {
        *r = p;
    }
}

/// Frame gaps after the first frame that drew anything.
fn measure(world: &World, p: &mut ProofLimits) {
    let now = evani_core::time::now_ms();
    let drew = world.try_res::<RenderStats>().is_some_and(|s| s.draw_calls > 0);
    if p.first_draw_ms.is_none() {
        if drew {
            p.first_draw_ms = Some(now);
            p.last_ms = Some(now);
        }
        return;
    }
    let (Some(first), Some(last)) = (p.first_draw_ms, p.last_ms) else { return };
    if now - first <= 4000.0 {
        let gap = now - last;
        p.max_gap_ms = p.max_gap_ms.max(gap);
        p.gaps_over_50 += u32::from(gap > 50.0);
        p.gaps_over_100 += u32::from(gap > 100.0);
    }
    p.last_ms = Some(now);
}

fn report(world: &World, p: &ProofLimits) {
    let fog = world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on("fog"));
    let reg = world.res::<ParamRegistry>();
    let get = |n: &str| reg.id(n).and_then(|id| reg.value(id)).map_or("absent".to_string(), |v| format!("{v:?}"));
    #[cfg(feature = "render-limits-proof")]
    let warm = world
        .try_res::<evani_render::prewarm::Prewarm>()
        .map_or("none".to_string(), |w| format!("held_ms={:?} frames={} timed_out={}", w.held_ms, w.held_frames, w.timed_out));
    #[cfg(not(feature = "render-limits-proof"))]
    let warm = "off";
    let title = format!(
        "rl scene={:?} build={} frames={} first_draw_ms={:.0} max_gap_ms={:.1} gaps50={} gaps100={} fog={} near={} clear={} fog_own={} fog_color={} prewarm=[{}]",
        p.scene,
        if cfg!(feature = "render-limits-proof") { "after" } else { "before" },
        p.frames,
        p.first_draw_ms.unwrap_or(-1.0),
        p.max_gap_ms,
        p.gaps_over_50,
        p.gaps_over_100,
        if fog { "on" } else { "off" },
        get("render.camera.near"),
        get("render.clear_color"),
        get("render.fog.own_color"),
        get("render.fog.color"),
        warm,
    );
    #[cfg(target_arch = "wasm32")]
    if let Some(d) = web_sys::window().and_then(|w| w.document()) {
        d.set_title(&title);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = title;
}

fn material(world: &World, name: &str) -> u32 {
    world.try_res::<LookData>().map_or(0, |l| RenderWorld::material(&l, name))
}

/// Take over the frame: the proof's camera, nothing the kit or the model
/// proof drew.
fn own_frame(world: &World, cam: CameraPose) {
    if let Some(mut models) = world.try_res_mut::<Models>() {
        models.draws.clear();
    }
    let mut rw = world.res_mut::<RenderWorld>();
    rw.meshes.clear();
    rw.blobs.clear();
    rw.camera = cam;
}

fn mesh(mesh: MeshId, at: [f32; 3], scale: [f32; 3], material: u32, color: [f32; 4]) -> MeshInstance {
    let mut m = MeshInstance::new(mesh, at);
    m.scale = scale;
    m.material = material;
    m.color = color;
    m
}

fn ground(world: &World, size: f32) -> MeshInstance {
    mesh(MeshId::PLANE, [0.0, 0.0, 0.0], [size, 1.0, size], material(world, "ground"), [1.0; 4])
}

/// The near-plane scene: the eye 6 cm from a wall that covers the right of
/// the view; red posts stand 3 m behind the wall (seen only through a cut).
fn near(world: &mut World, p: &mut ProofLimits) {
    own_frame(
        world,
        CameraPose { eye: [0.0, 1.6, 0.0], target: [0.0, 1.6, -1.0], up: [0.0, 1.0, 0.0], fov_y_deg: 70.0 },
    );
    let stone = material(world, "stone");
    let accent = material(world, "accent");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.draw_mesh(ground(world, 40.0));
    // The wall's face is at z = -0.06, from x = -0.03 to the right.
    rw.draw_mesh(mesh(MeshId::CUBE, [1.47, 1.5, -0.16], [3.0, 3.0, 0.2], stone, [0.95, 0.92, 0.85, 1.0]));
    for x in [0.3, 1.0, 1.8] {
        rw.draw_mesh(mesh(MeshId::CUBE, [x, 1.0, -3.2], [0.3, 2.0, 0.3], accent, [1.0, 0.15, 0.1, 1.0]));
    }
    drop(rw);
    // The held stand-in (a crate stretched into a gun shape): camera space
    // 35 cm ahead, so in world space it is inside the wall.
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let h = *p.held.get_or_insert_with(|| models.handle("proof-crate"));
    let Some(id) = models.resolve(h) else { return };
    let held = ModelInstance::new(id, [0.03, -0.09, -0.35]).scale([0.07, 0.07, 0.3]);
    #[cfg(feature = "render-limits-proof")]
    if !p.vm_world {
        models.draw_view_model(held);
        return;
    }
    // Without the layer: the same pose in the world. The proof camera looks
    // straight down -z with y up, so camera space is world space moved to
    // the eye.
    let e = world.res::<RenderWorld>().camera.eye;
    let mut in_world = held;
    in_world.position = [e[0] + held.position[0], e[1] + held.position[1], e[2] + held.position[2]];
    models.draw(in_world);
}

/// The haze scene: a low view over the scenery to the horizon.
fn haze(world: &mut World) {
    own_frame(
        world,
        CameraPose { eye: [0.0, 2.5, 40.0], target: [0.0, 6.0, -200.0], up: [0.0, 1.0, 0.0], fov_y_deg: 55.0 },
    );
    let stone = material(world, "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.draw_mesh(ground(world, 1200.0));
    // Rows of pillars marching away, so the haze has depth to show.
    for i in 0..14 {
        let z = 20.0 - i as f32 * 28.0;
        for x in [-14.0f32, 14.0] {
            rw.draw_mesh(mesh(MeshId::BEVEL_CUBE, [x, 4.0, z], [3.0, 8.0, 3.0], stone, [0.9, 0.88, 0.84, 1.0]));
        }
    }
}

/// The soft-shadow scene: a body (a lying capsule and a head) on the floor
/// under a low sun, seen close.
fn pcss(world: &mut World) {
    own_frame(
        world,
        CameraPose { eye: [1.3, 1.25, 2.1], target: [-0.1, 0.05, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 50.0 },
    );
    let accent = material(world, "accent");
    let stone = material(world, "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.draw_mesh(mesh(MeshId::PLANE, [0.0, 0.0, 0.0], [30.0, 1.0, 30.0], stone, [0.85, 0.83, 0.8, 1.0]));
    // Lying along x: a cylinder turned 90 degrees about z, 0.3 m thick.
    let (s, c) = (core::f32::consts::FRAC_PI_4.sin(), core::f32::consts::FRAC_PI_4.cos());
    let mut body = mesh(MeshId::CYLINDER, [0.0, 0.15, 0.0], [0.3, 1.5, 0.22], accent, [0.35, 0.45, 0.3, 1.0]);
    body.rotation = [0.0, 0.0, s, c];
    rw.draw_mesh(body);
    rw.draw_mesh(mesh(MeshId::SPHERE, [-0.95, 0.12, 0.0], [0.24; 3], accent, [0.8, 0.62, 0.5, 1.0]));
    // A low flattened box beside it: the stand-in limb.
    rw.draw_mesh(mesh(MeshId::BEVEL_CUBE, [0.2, 0.06, 0.32], [0.9, 0.12, 0.14], accent, [0.35, 0.45, 0.3, 1.0]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_picks_the_scene() {
        assert_eq!(scene_of(None), Scene::Grid);
        assert_eq!(scene_of(Some("http://x/")), Scene::Grid);
        assert_eq!(scene_of(Some("http://x/?rl=near&vm=world")), Scene::Near);
        assert_eq!(scene_of(Some("http://x/?a=1&rl=pcss#top")), Scene::Pcss);
        assert_eq!(scene_of(Some("http://x/?rl=warm")), Scene::Warm);
        assert_eq!(query_value("http://x/?rl=near&vm=world", "vm"), Some("world"));
        assert_eq!(query_value("http://x/?rl", "rl"), Some(""));
    }

    fn run_scene(scene: Scene) -> evani_core::App {
        let mut app = crate::build_app();
        app.world.insert_resource(ProofLimits::new(scene));
        app.run_headless(3, 1000.0 / 60.0);
        app
    }

    #[test]
    fn the_pcss_scene_draws_its_body_under_a_low_sun() {
        let app = run_scene(Scene::Pcss);
        let reg = app.world.res::<ParamRegistry>();
        let dir = reg.id("render.light.direction").and_then(|id| reg.value(id));
        assert_eq!(dir, Some(ParamValue::Vec3([0.9, 0.25, 0.35])));
        assert!(app.world.res::<ProofLimits>().frames >= 3);
    }

    #[test]
    fn the_near_scene_sets_the_near_plane_where_the_build_has_it() {
        let app = run_scene(Scene::Near);
        let reg = app.world.res::<ParamRegistry>();
        let near = reg.id("render.camera.near").and_then(|id| reg.value(id));
        if cfg!(feature = "render-limits-proof") {
            assert_eq!(near, Some(ParamValue::F32(0.01)));
        } else {
            assert_eq!(near, None);
        }
    }

    #[test]
    fn the_warm_scene_sets_kit_code_on_its_first_frame() {
        let app = run_scene(Scene::Warm);
        assert!(app.world.res::<KitMaterial>().has_code());
    }
}
