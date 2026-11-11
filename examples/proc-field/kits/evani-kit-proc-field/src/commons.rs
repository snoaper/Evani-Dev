//! The render service's own views of the plain field (kit feature `commons`,
//! turned on only by the separate app `examples/proc-field/apps/commons-render`,
//! never by the proof game or a scene). Evani.App's server renders a shared
//! Evani.commons version (docs/EVANI_COMMONS.md) under the engine's realism
//! studio, with cameras and a sun of its own choosing, once more without the
//! asset (its mask), and reads the asset's mesh stats. It judges each asset by
//! its reference kind: a studio object from the orbit camera, a scene from a
//! player standing in it (`cam_free` 2), a large landform from above and from
//! inside (`view_preset`), and a behaviour asset by its effect on a standard
//! test scene (`test_scene`, `time_s`, `pose`) with the scene's own measures
//! (`measure`; src/scenes.rs). Every default leaves the field as it was:
//! `cam_free`, `sun_free`, `subject_off`, `stats`, `view_preset`,
//! `time_free`, `test_scene` and `measure` are 0.
//!
//! Staging (`staging`, 2026-11-11): an object is shot as its reference is
//! made, alone on a seamless neutral grey studio sweep under a soft even key
//! light (catalog/procedural/ref_classes.json class `object`: "neutral grey
//! backdrop, soft key light"); scenes, landforms and test scenes keep the
//! studio sky over the field. `staging` 0 (auto, the default) picks the sweep
//! for the server's object views (the orbit camera, `cam_free` 1, with no
//! `view_preset` and no `test_scene`), 1 always the sweep, 2 always the sky.
//! The orbit camera's lens is `cam_lens_deg` (vertical, default 50); the
//! framing distance follows it, so the asset fills the frame the same.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::{Path, ProcScene, Registry, SpecKind, Value, Values};
use evani_render::{MeshId, MeshInstance, RenderWorld};

use crate::scenes;

/// The field's fixed camera (lib.rs), which `cam_free` 0 keeps.
const EYE: [f32; 3] = [0.0, 16.0, 34.0];
const TARGET: [f32; 3] = [0.0, 1.0, 0.0];
const FOV: f32 = 50.0;
/// The grey studio sweep (object staging, after the pebbles' dev scene,
/// pebbles.rs): the backdrop's grey, the floor's (drawn by lib.rs) and the
/// soft, even, neutral key and fill over the realism studio.
/// Matched to the Pebbles reference (an object reference's studio, ref_classes.json class `object`) by
/// measuring view 0 against it (2026-11-11, .evani/shots/object-staging): backdrop and floor both about
/// 136 (8-bit luminance) against the reference's 134 and 142, saturation 1 against 2. The backdrop is
/// the clear colour (`render.clear_color`, which the look's haze also fades the floor into) and is set
/// to the grey the lit floor renders at, so the floor's far edge never draws a line under it (the haze
/// only reaches the floor in some builds, so the sweep can't rely on it).
pub const SWEEP_BACKDROP: [f32; 3] = [0.317, 0.314, 0.314];
pub const SWEEP_FLOOR: [f32; 4] = [0.257, 0.265, 0.283, 1.0];
pub const SWEEP_FOG: [f32; 3] = SWEEP_BACKDROP;
const SWEEP: [(&str, ParamValue); 19] = [
    ("render.light.color", ParamValue::Color([1.0, 1.0, 1.0])),
    ("render.light.intensity", ParamValue::F32(2.1)),
    ("render.ambient", ParamValue::Color([0.92, 0.93, 0.96])),
    ("render.ao.strength", ParamValue::F32(0.6)),
    ("render.shadow_tint_strength", ParamValue::F32(0.0)),
    ("render.ibl.strength", ParamValue::F32(0.6)),
    ("render.exposure", ParamValue::F32(0.9)),
    ("look.stylizedpbr.env_spec", ParamValue::F32(0.3)),
    ("look.stylizedpbr.fill", ParamValue::F32(0.15)),
    ("look.stylizedpbr.wrap", ParamValue::F32(0.75)),
    ("look.stylizedpbr.rim", ParamValue::F32(0.0)),
    ("look.stylizedpbr.pcss_light_deg", ParamValue::F32(8.0)),
    ("look.stylizedpbr.pcss_search", ParamValue::F32(24.0)),
    ("render.sky.sun_glow", ParamValue::F32(0.0)),
    ("render.sky.zenith", ParamValue::Color(SWEEP_BACKDROP)),
    ("render.sky.horizon", ParamValue::Color(SWEEP_BACKDROP)),
    ("render.sky.ground", ParamValue::Color(SWEEP_BACKDROP)),
    ("render.clear_color", ParamValue::Color(SWEEP_FOG)),
    // The floor rises into the backdrop evenly down the picture (film.rs).
    ("look.stylizedpbr.haze_curve", ParamValue::F32(1.0)),
];
/// The sweep's exposure (`render.exposure` in [`SWEEP`]).
const SWEEP_EXPOSURE: f32 = 0.9;
/// How a sweep colour shows in the picture (8-bit sRGB of `GAIN * c - OFFSET`,
/// linear): the exposure, and the film tonemap's black offset below its knee.
/// Measured on renders of the sweep (2026-11-11, .evani/shots/object-staging-exact).
const SHOW_GAIN: f32 = SWEEP_EXPOSURE;
const SHOW_OFFSET: f32 = 0.04;
/// The lit floor's gain over its colour for a sun `p` above the horizon:
/// `FLOOR_SUN * sin(p) + FLOOR_SKY` (the key's share and the ambient, image
/// light and fill's), fitted to renders of the sweep's floor at a sun 20, 45
/// and 75 degrees up (gains 1.14, 1.32, 1.43; the backdrop's grey changes
/// none of them).
const FLOOR_SUN: f32 = 0.47;
const FLOOR_SKY: f32 = 0.975;

/// An 8-bit sRGB value as linear.
fn lin8(v: f32) -> f32 {
    let c = (v / 255.0).clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The linear colour that shows as `grey` (8-bit sRGB luminance) with
/// `warmth` red minus blue, before the light and the film (`gain` 1 for the
/// backdrop, which is drawn as it is; the floor's light gain otherwise).
pub fn studio_colour(grey: f32, warmth: f32, gain: f32) -> [f32; 3] {
    // Red up and blue down by half the warmth each; all three down by the
    // luminance that adds (0.2126 - 0.0722) / 2 of it, so the grey stays.
    let g = grey - warmth * 0.0702;
    [g + warmth * 0.5, g, g - warmth * 0.5].map(|c| ((lin8(c) + SHOW_OFFSET) / SHOW_GAIN / gain.max(0.05)).max(0.0))
}

/// The floor's light gain for a sun `elevation` (radians) above the horizon.
pub fn floor_gain(elevation: f32) -> f32 {
    FLOOR_SUN * elevation.sin().max(0.0) + FLOOR_SKY
}

/// The sweep's params the realism studio doesn't set: put back to their
/// defaults when the sky staging returns.
const SWEEP_ONLY: [&str; 8] = ["render.sky.zenith", "render.sky.horizon", "render.sky.ground", "render.clear_color", "look.stylizedpbr.haze_near", "look.stylizedpbr.haze_far", "look.stylizedpbr.haze_curve", "fx.shadows.distance"];
/// The fixed camera's pitch above its target: atan(15 / 34) in degrees.
const PITCH0: f32 = 23.81;
/// The player camera's preset (ref_classes.json viewpoint `player-inside`):
/// eye height and horizontal field of view.
const EYE_M: f32 = 1.7;
const FOV_H: f32 = 60.0;
/// The two-view landform preset (viewpoints `aerial-overview` and
/// `player-inside`, layout `two-panel`): the overview 30 degrees off the
/// long axis, 45 degrees down; the inside view 0.3 of the radius from the
/// centre, facing along the long axis.
const OVERVIEW_YAW: f32 = 30.0;
const OVERVIEW_PITCH: f32 = 45.0;
const INSIDE_STAND: f32 = 0.3;

#[derive(Debug, Clone, Copy)]
struct Knobs {
    cam_free: F32Param,
    yaw: F32Param,
    pitch: F32Param,
    dist: F32Param,
    stand: F32Param,
    eye: F32Param,
    fov: F32Param,
    preset: F32Param,
    sun_free: F32Param,
    sun_yaw: F32Param,
    sun_pitch: F32Param,
    subject_off: F32Param,
    stats: F32Param,
    time_free: F32Param,
    time_s: F32Param,
    scene: F32Param,
    pose: F32Param,
    measure: F32Param,
    staging: F32Param,
    lens: F32Param,
    floor: Vec3Param,
    aim: F32Param,
    backdrop: F32Param,
    floor_grey: F32Param,
    warmth: F32Param,
}

/// What the field measured and staged (once per placement set).
#[derive(Debug, Default)]
struct Field {
    staged: bool,
    key: String,
    bounds: Option<([f32; 3], [f32; 3])>,
    /// The placed assets' upward-facing triangles (world), for the ground
    /// under the player camera.
    ground: Vec<[[f32; 3]; 3]>,
    triangles: u64,
    vertices: u64,
    assets: usize,
    sun_saved: Option<ParamValue>,
    sun_set: Option<(f32, f32)>,
    stats_sent: bool,
    /// The staging in force: Some(true) the grey sweep, Some(false) the sky.
    sweep: Option<bool>,
    /// The sweep's haze (near, far) and shadow reach last set.
    sweep_reach: Option<(f32, f32, f32)>,
    /// The sweep's backdrop colour last set (linear).
    sweep_back: Option<[f32; 3]>,
}

/// The test scene's state: the asset params a pose overrode (to put back),
/// the measure line sent, and the drawn proxies of the last step.
#[derive(Debug, Default)]
pub struct Staging {
    saved: Vec<(String, ParamValue)>,
    sent: Option<u32>,
    /// The last measure line (also written to the console on the web).
    pub line: Option<String>,
    drawn: Option<(u32, u64, u64)>,
    pub instances: Vec<MeshInstance>,
}

fn knob(kit: &mut PluginContext, name: &str, d: f32, lo: f32, hi: f32, unit: &str, what: &str) -> Result<F32Param, PluginError> {
    let mut def = ParamDef::f32(name, d).range(lo, hi).describe(what);
    if !unit.is_empty() {
        def = def.unit(unit);
    }
    Ok(kit.param_f32(def)?)
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let k = Knobs {
        cam_free: knob(kit, "kit.proc_field.cam_free", 0.0, 0.0, 3.0, "", "1: the camera orbits the asset, framed on its bounds (cam_yaw_deg, cam_pitch_deg, cam_dist_scale); 2: a player standing at cam_stand (cam_eye_m, cam_pitch_deg, cam_fov_deg); 3: the orbit with cam_fov_deg across and framed for any picture shape (the aerial overview); 0: the field's fixed camera")?,
        yaw: knob(kit, "kit.proc_field.cam_yaw_deg", 0.0, -360.0, 360.0, "deg", "Free camera: the angle round the asset from +z (the fixed camera's side) towards +x; the player stands on that side, facing the centre's direction")?,
        pitch: knob(kit, "kit.proc_field.cam_pitch_deg", PITCH0, -89.0, 89.0, "deg", "Orbit: the angle above the asset's centre; player camera (cam_free 2): the look up (+) or down (-) from level, so set 0 for a level view")?,
        dist: knob(kit, "kit.proc_field.cam_dist_scale", 1.0, 0.1, 10.0, "", "Orbit: the distance as a share of the one that frames the whole asset")?,
        stand: knob(kit, "kit.proc_field.cam_stand", 0.0, 0.0, 5.0, "", "Player camera (cam_free 2): where the player stands, as a share of the asset's horizontal bounds radius from its centre (0 the centre, 1 the edge, over 1 outside), on the side cam_yaw_deg faces from")?,
        eye: knob(kit, "kit.proc_field.cam_eye_m", EYE_M, 0.1, 100.0, "m", "Player camera: eye height above the ground under the player (the asset's own surface there, else the floor)")?,
        fov: knob(kit, "kit.proc_field.cam_fov_deg", FOV_H, 10.0, 140.0, "deg", "Player and overview cameras (cam_free 2 and 3): the horizontal field of view")?,
        preset: knob(kit, "kit.proc_field.view_preset", 0.0, 0.0, 2.0, "", "Two-view landform panels: 1 the aerial overview (left panel: 45 degrees down, 60 degrees across, the whole landform in frame, 30 degrees off its long axis), 2 the player inside it (right panel: eye 1.7 m, level, 60 degrees, 0.3 of the radius from the centre, facing along the long axis); cam_yaw_deg turns both round; 0 off")?,
        sun_free: knob(kit, "kit.proc_field.sun_free", 0.0, 0.0, 1.0, "", "1: the sun comes from sun_yaw_deg and sun_pitch_deg; 0: the studio's own (restored)")?,
        sun_yaw: knob(kit, "kit.proc_field.sun_yaw_deg", 135.0, -360.0, 360.0, "deg", "Sun azimuth from +z towards +x")?,
        sun_pitch: knob(kit, "kit.proc_field.sun_pitch_deg", 35.0, 1.0, 89.0, "deg", "Sun elevation")?,
        subject_off: knob(kit, "kit.proc_field.subject_off", 0.0, 0.0, 1.0, "", "1: the field without its asset (floor and backdrop only), for a render's asset mask")?,
        stats: knob(kit, "kit.proc_field.stats", 0.0, 0.0, 1.0, "", "1: write the placed asset's mesh stats once to the console as `evani-field-stats {json}` (asset-shoot.mjs --stats)")?,
        time_free: knob(kit, "kit.proc_field.time_free", 0.0, 0.0, 1.0, "", "1: the test scene stands at time_s (deterministic frames); 0: it runs with the sim clock")?,
        time_s: knob(kit, "kit.proc_field.time_s", 0.0, 0.0, 600.0, "s", "The test scene's sim time while time_free is 1 (behaviour frames: t0, +0.5, +1.0, +1.5 s)")?,
        scene: knob(kit, "kit.proc_field.test_scene", 0.0, 0.0, 4.0, "", "The behaviour test scene (ref_classes.json test_scenes): 1 wind-field, 2 current-channel, 3 pose-set, 4 expression-set; 0 the plain field")?,
        pose: knob(kit, "kit.proc_field.pose", 0.0, 0.0, 3.0, "", "Pose-set and expression-set: the frame's pose 0..3 (rest, stride, crouch, reach; neutral, smile, surprise, blink)")?,
        measure: knob(kit, "kit.proc_field.measure", 0.0, 0.0, 1.0, "", "1: run the test scene's own probes and write `evani-field-measure {\"<spec id>\": number|null}` once to the console (asset-shoot.mjs --measure)")?,
        staging: knob(kit, "kit.proc_field.staging", 0.0, 0.0, 2.0, "", "0 auto: the grey studio sweep for an object's orbit views (cam_free 1, no view_preset, no test_scene), else the studio sky; 1 always the grey studio sweep (seamless grey floor and backdrop, soft even key); 2 always the studio sky over the field")?,
        floor: kit.param_vec3(ParamDef::color("kit.proc_field.sweep_floor", [SWEEP_FLOOR[0], SWEEP_FLOOR[1], SWEEP_FLOOR[2]]).describe("The grey studio sweep's floor colour (the backdrop is render.sky.* and render.clear_color, which the floor fades into)"))?,
        lens: knob(kit, "kit.proc_field.cam_lens_deg", FOV, 10.0, 90.0, "deg", "Orbit camera (cam_free 1): the vertical field of view; the framing distance follows it, so the asset fills the frame the same (a longer lens, a flatter perspective)")?,
        aim: knob(kit, "kit.proc_field.cam_aim_y", 0.0, -1.0, 1.0, "", "Orbit camera (cam_free 1): where it aims, above (+) or below (-) the asset's centre as a share of its bounds radius (a reference camera's `cam_aim_y`: the asset sits lower in the frame for +, higher for -)")?,
        backdrop: knob(kit, "kit.proc_field.studio_backdrop", 0.0, 0.0, 255.0, "", "Grey studio sweep: the backdrop's grey as the picture shows it (8-bit sRGB luminance, a reference's `studio.backdrop_grey`); 0 the sweep's own")?,
        floor_grey: knob(kit, "kit.proc_field.studio_floor", 0.0, 0.0, 255.0, "", "Grey studio sweep: the lit floor's grey in front of the asset as the picture shows it (8-bit, `studio.floor_grey`), whatever the sun's height; 0 the sweep_floor colour")?,
        warmth: knob(kit, "kit.proc_field.studio_warmth", 0.0, -40.0, 40.0, "", "Grey studio sweep: the backdrop's and floor's red minus blue (8-bit, `studio.warmth`; + warm, - cool, 0 neutral grey)")?,
    };
    kit.resource(k);
    kit.resource(Field::default());
    kit.resource(Staging::default());
    Ok(())
}

fn scene_of(world: &World) -> u32 {
    let Some(k) = world.try_res::<Knobs>().map(|k| *k) else { return 0 };
    world.res::<ParamRegistry>().f32(k.scene).round().clamp(0.0, 4.0) as u32
}

/// True while the field should draw no asset: the mask pass, and the wind
/// and current scenes (their asset acts only through its field).
pub fn subject_off(world: &World) -> bool {
    let Some(k) = world.try_res::<Knobs>().map(|k| *k) else { return false };
    world.res::<ParamRegistry>().f32(k.subject_off) >= 0.5 || matches!(scene_of(world), 1 | 2)
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && let Some(v) = reg.value(id).and_then(|like| evani_procedural::studio::coerce(like, v))
    {
        let _ = reg.set(id, v);
    }
}

/// The camera mode: `cam_free` rounded (0..3).
fn mode(v: f32) -> u32 {
    if v < 0.5 {
        0
    } else if v < 1.5 {
        1
    } else if v < 2.5 {
        2
    } else {
        3
    }
}

/// The vertical field of view (deg) for `fov_h` across a picture of
/// `aspect` (width / height).
fn fov_y(fov_h: f32, aspect: f32) -> f32 {
    (2.0 * ((fov_h * 0.5).to_radians().tan() / aspect.max(0.05)).atan()).to_degrees()
}

/// An orbit pose round `t` at `d` along yaw and pitch (deg).
fn orbit(t: [f32; 3], d: f32, yaw: f32, pitch: f32, fov_y_deg: f32) -> CameraPose {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    CameraPose { eye: [t[0] + d * p.cos() * y.sin(), t[1] + d * p.sin(), t[2] + d * p.cos() * y.cos()], target: t, up: [0.0, 1.0, 0.0], fov_y_deg }
}

/// The ground height at `(x, z)`: the highest upward-facing triangle of
/// the placed asset there, else the floor (0).
fn ground_at(tris: &[[[f32; 3]; 3]], x: f32, z: f32) -> f32 {
    let mut best: Option<f32> = None;
    for [a, b, c] in tris {
        let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if d.abs() < 1e-12 {
            continue;
        }
        let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
        let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
        let l3 = 1.0 - l1 - l2;
        if l1 < -1e-5 || l2 < -1e-5 || l3 < -1e-5 {
            continue;
        }
        let y = l1 * a[1] + l2 * b[1] + l3 * c[1];
        best = Some(best.map_or(y, |v: f32| v.max(y)));
    }
    best.unwrap_or(0.0)
}

/// What the cameras frame: the centre, the 3D and horizontal bounds radius
/// and the half sizes; a test scene's own extent for the wind and current
/// scenes.
fn framing(world: &World, scene: u32) -> ([f32; 3], f32, f32, [f32; 3]) {
    let b = match scene {
        1 => Some(([-scenes::MEADOW_HALF, 0.0, -scenes::MEADOW_HALF], [scenes::MEADOW_HALF, scenes::TREE_H, scenes::MEADOW_HALF])),
        2 => Some(([-scenes::CHANNEL_HALF_LEN, -0.3, -3.0], [scenes::CHANNEL_HALF_LEN, 0.5, 3.0])),
        _ => world.res::<Field>().bounds,
    };
    match b {
        Some((lo, hi)) => {
            let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
            let h = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
            let r = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt().max(0.05);
            (c, r, (h[0] * h[0] + h[2] * h[2]).sqrt().max(0.05), h)
        }
        None => (TARGET, 0.0, 0.05, [0.0; 3]),
    }
}

/// Stage the studio once, move or restore the sun, measure and report the
/// asset when asked, stage the test scene; the free camera for this frame,
/// or `None` for the field's own.
pub fn update(world: &mut World) -> Option<CameraPose> {
    let k = *world.try_res::<Knobs>()?;
    let (m, yaw, pitch, dist, sun, stats) = {
        let r = world.res::<ParamRegistry>();
        let sun = (r.f32(k.sun_free) >= 0.5).then(|| (r.f32(k.sun_yaw), r.f32(k.sun_pitch)));
        (mode(r.f32(k.cam_free)), r.f32(k.yaw), r.f32(k.pitch), r.f32(k.dist), sun, r.f32(k.stats) >= 0.5)
    };
    let (stand, eye_m, fov_h, preset, staging, lens, aim) = {
        let r = world.res::<ParamRegistry>();
        (r.f32(k.stand), r.f32(k.eye), r.f32(k.fov), r.f32(k.preset).round() as u32, r.f32(k.staging).round() as u32, r.f32(k.lens).clamp(10.0, 90.0), r.f32(k.aim).clamp(-1.0, 1.0))
    };
    let scene = scene_of(world);
    stage(world, sun);
    if m >= 1 || stats || preset >= 1 || scene >= 1 {
        measure(world);
    }
    report(world, stats);
    test_scene(world, k, scene);
    let aspect = world.try_res::<RenderWorld>().map(|r| r.viewport_css).filter(|v| v[0] > 0.0 && v[1] > 0.0).map_or(1.5, |v| v[0] / v[1]);
    // The two-view preset, round the landform's long axis.
    let (m, yaw, pitch, dist, stand, eye_m, fov_h) = match preset {
        1 | 2 => {
            let (_, _, _, h) = framing(world, scene);
            let long = if h[0] >= h[2] { 90.0 } else { 0.0 };
            if preset == 1 { (3, long + OVERVIEW_YAW + yaw, OVERVIEW_PITCH, 1.0, stand, eye_m, FOV_H) } else { (2, long + yaw, 0.0, dist, INSIDE_STAND, EYE_M, FOV_H) }
        }
        _ => (m, yaw, pitch, dist, stand, eye_m, fov_h),
    };
    let pose = free_camera(world, scene, aspect, (m, yaw, pitch, dist, stand, eye_m, fov_h, lens, aim));
    reach(world, scene, pose.as_ref());
    sweep(world, sweep_wanted(staging, m, preset, scene), scene, pose.as_ref());
    pose
}

/// Whether the grey studio sweep stages this frame: `staging` 1 always, 2
/// never, 0 (auto) for the server's object views: the orbit camera
/// (`cam_free` 1) with no landform preset and no test scene. Scenes (the
/// player camera, the overview), landforms and behaviours keep the sky.
pub fn sweep_wanted(staging: u32, cam_mode: u32, preset: u32, scene: u32) -> bool {
    match staging {
        1 => true,
        2 => false,
        _ => cam_mode == 1 && preset == 0 && scene == 0,
    }
}

/// The sweep's floor colour while the grey studio sweep is staged (lib.rs
/// draws its floor in it), else `None`.
pub fn sweep_floor(world: &World) -> Option<[f32; 4]> {
    let k = *world.try_res::<Knobs>()?;
    world.try_res::<Field>().filter(|f| f.sweep == Some(true))?;
    let reg = world.res::<ParamRegistry>();
    let grey = reg.f32(k.floor_grey);
    let c = if grey > 0.0 {
        // The per-object floor: the grey it shows whatever the sun's height.
        let up = reg.id("render.light.direction").and_then(|i| reg.value(i)).map_or(0.64, |v| match v {
            ParamValue::Vec3(d) => d[1] / (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6),
            _ => 0.64,
        });
        studio_colour(grey, reg.f32(k.warmth), floor_gain(up.clamp(-1.0, 1.0).asin()))
    } else {
        reg.vec3(k.floor)
    };
    Some([c[0], c[1], c[2], 1.0])
}

/// The backdrop colour (linear) the sweep draws: the per-object grey
/// (`studio_backdrop` over 0, with its warmth), else the sweep's own.
fn sweep_backdrop(reg: &ParamRegistry, k: Knobs) -> [f32; 3] {
    let grey = reg.f32(k.backdrop);
    if grey > 0.0 { studio_colour(grey, reg.f32(k.warmth), 1.0) } else { SWEEP_BACKDROP }
}

/// Two significant figures: the haze is compiled into the look's shader, so
/// it changes only when the framing really does.
fn sig2(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let p = 10f32.powf(v.log10().floor() - 1.0);
    (v / p).round() * p
}

/// The sweep's reach for a camera `cam_d` from what it frames (radius `r`):
/// the floor fades into the backdrop from just behind the subject (haze
/// near) evenly down the picture to the horizon (`haze_curve` 1: all of it
/// by 30 times as far, where the floor is a sliver under the horizon), and
/// the shadows reach past the subject.
pub fn sweep_reach(cam_d: f32, r: f32) -> (f32, f32, f32) {
    let near = sig2(cam_d + 1.5 * r);
    let far = sig2(near * 30.0);
    let shadow = sig2(2.2 * (cam_d + r));
    (near, far, shadow)
}

/// Switch the staging between the grey studio sweep and the studio sky
/// (the realism studio again, the sweep's own params back to their
/// defaults), and keep the sweep's haze and shadow reach on the framing.
fn sweep(world: &mut World, on: bool, scene: u32, pose: Option<&CameraPose>) {
    let (was, reach_was, back_was) = {
        let f = world.res::<Field>();
        (f.sweep, f.sweep_reach, f.sweep_back)
    };
    let reach = if on {
        let (c, r, _, _) = framing(world, scene);
        let r = r.max(0.05);
        let d = pose.map_or(r * 2.6, |p| ((p.eye[0] - c[0]).powi(2) + (p.eye[1] - c[1]).powi(2) + (p.eye[2] - c[2]).powi(2)).sqrt());
        Some(sweep_reach(d, r))
    } else {
        None
    };
    let back = match world.try_res::<Knobs>() {
        Some(k) if on => Some(sweep_backdrop(&world.res::<ParamRegistry>(), *k)),
        _ => None,
    };
    if was == Some(on) && reach == reach_was && back == back_was {
        return;
    }
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        if was != Some(on) {
            if on {
                for (name, v) in SWEEP {
                    set(&mut reg, name, v);
                }
            } else if was.is_some() {
                let _ = evani_procedural::studio::apply(&mut reg);
                set(&mut reg, "aq.enabled", ParamValue::Bool(false));
                for name in SWEEP_ONLY {
                    if let Some(id) = reg.id(name) {
                        let _ = reg.reset(id);
                    }
                }
            }
        }
        if let Some((near, far, shadow)) = reach {
            set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(near));
            set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(far));
            set(&mut reg, "fx.shadows.distance", ParamValue::F32(shadow));
        }
        // The backdrop (the clear colour the floor's haze fades to, and the
        // studio sky drawn in it), per object when its studio says so.
        if let Some(c) = back {
            for name in ["render.sky.zenith", "render.sky.horizon", "render.sky.ground", "render.clear_color"] {
                set(&mut reg, name, ParamValue::Color(c));
            }
        }
    }
    let mut f = world.res_mut::<Field>();
    f.sweep = Some(on);
    f.sweep_reach = reach;
    f.sweep_back = back;
}

/// The far plane a pose needs to see all of what it frames: from the eye to
/// the far side of the asset's bounds, never nearer than the engine's own
/// 1 km (an object's pictures stay as they were) and at most its 100 km.
/// Mountains r12 (2026-11-08): a 5.2 km massif with 26 km of land round it
/// was framed from about 30 km away and cut off at 1 km, so the server's
/// reference view showed almost nothing of it.
pub fn far_for(eye: [f32; 3], centre: [f32; 3], radius: f32) -> f32 {
    let d = ((eye[0] - centre[0]).powi(2) + (eye[1] - centre[1]).powi(2) + (eye[2] - centre[2]).powi(2)).sqrt();
    ((d + radius) * 1.1 + 10.0).clamp(1000.0, 100_000.0)
}

/// Sets `render.camera.far` (the render app's `camera-range`) for this
/// frame's free camera; the field's own camera keeps the engine's planes.
fn reach(world: &mut World, scene: u32, pose: Option<&CameraPose>) {
    let (c, r, _, _) = framing(world, scene);
    let far = pose.map_or(1000.0, |p| far_for(p.eye, c, r));
    let mut reg = world.res_mut::<ParamRegistry>();
    let Some(id) = reg.id("render.camera.far") else { return };
    if !matches!(reg.value(id), Some(ParamValue::F32(v)) if (v - far).abs() <= far * 0.01) {
        set(&mut reg, "render.camera.far", ParamValue::F32(far));
    }
}

type Free = (u32, f32, f32, f32, f32, f32, f32, f32, f32);

/// The orbit's target `aim` bounds radii above the centre `t` (the radius
/// back from the framing distance `d0` the lens gave it).
fn aimed(t: [f32; 3], d0: f32, lens: f32, aim: f32) -> [f32; 3] {
    let r = d0 / 1.1 * (lens * 0.5).to_radians().sin();
    [t[0], t[1] + aim * r, t[2]]
}

/// The free camera's pose for this frame, or `None` for the field's own.
fn free_camera(world: &World, scene: u32, aspect: f32, (m, yaw, pitch, dist, stand, eye_m, fov_h, lens, aim): Free) -> Option<CameraPose> {
    match m {
        0 => scene_camera(world, scene, aspect),
        1 => {
            let bounds = world.res::<Field>().bounds;
            let (t, d0) = match bounds {
                Some((lo, hi)) => {
                    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
                    let h = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
                    let r = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt().max(0.05);
                    (c, r / (lens * 0.5).to_radians().sin() * 1.1)
                }
                None => {
                    let d = [EYE[0] - TARGET[0], EYE[1] - TARGET[1], EYE[2] - TARGET[2]];
                    (TARGET, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
                }
            };
            let (t, d0) = if matches!(scene, 1 | 2) {
                let (c, r, _, _) = framing(world, scene);
                (c, r / (lens * 0.5).to_radians().sin() * 1.1)
            } else {
                (t, d0)
            };
            let t = if matches!(scene, 1 | 2) { t } else { aimed(t, d0, lens, aim) };
            Some(orbit(t, d0 * dist.max(0.05), yaw, pitch, lens))
        }
        2 => {
            let (c, _, rh, _) = framing(world, scene);
            let y = yaw.to_radians();
            let (px, pz) = (c[0] + stand * rh * y.sin(), c[2] + stand * rh * y.cos());
            let g = match scene {
                1 => 0.0,
                2 => if pz.abs() > 2.0 { 0.25 } else { 0.0 },
                _ => ground_at(&world.res::<Field>().ground, px, pz),
            };
            let p = pitch.to_radians();
            let eye = [px, g + eye_m, pz];
            let dir = [-y.sin() * p.cos(), p.sin(), -y.cos() * p.cos()];
            Some(CameraPose { eye, target: [eye[0] + dir[0], eye[1] + dir[1], eye[2] + dir[2]], up: [0.0, 1.0, 0.0], fov_y_deg: fov_y(fov_h, aspect) })
        }
        _ => {
            let (c, r, _, _) = framing(world, scene);
            let fy = fov_y(fov_h, aspect);
            let d0 = r.max(0.05) / (fy.min(fov_h) * 0.5).to_radians().sin() * 1.1;
            Some(orbit(c, d0 * dist.max(0.05), yaw, pitch, fy))
        }
    }
}

/// A test scene's own fixed camera (`cam_free` 0), or the field's.
fn scene_camera(world: &World, scene: u32, aspect: f32) -> Option<CameraPose> {
    let fy = fov_y(FOV_H, aspect);
    match scene {
        // From the meadow's edge at eye height, nearly level.
        1 => Some(CameraPose { eye: [0.0, EYE_M, scenes::MEADOW_HALF + 2.0], target: [0.0, 1.2, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: fy }),
        // From the bank, looking down over the water to the rock.
        2 => Some(CameraPose { eye: [-1.0, 0.25 + EYE_M, 3.4], target: [0.6, 0.0, -0.4], up: [0.0, 1.0, 0.0], fov_y_deg: fy }),
        // The body from the front three-quarter, the head from the front.
        3 | 4 => {
            let (c, r, _, _) = framing(world, scene);
            (r > 0.0).then(|| orbit(c, r / (FOV * 0.5).to_radians().sin() * 1.1, if scene == 3 { 35.0 } else { 0.0 }, if scene == 3 { 8.0 } else { 0.0 }, FOV))
        }
        _ => None,
    }
}

/// The engine's realism studio, fixed quality (every render compares like
/// with like), and the sun when `sun_free` is on; turning it off puts the
/// studio's sun back.
fn stage(world: &mut World, sun: Option<(f32, f32)>) {
    let (staged, prev, saved) = {
        let f = world.res::<Field>();
        (f.staged, f.sun_set, f.sun_saved)
    };
    if staged && prev == sun {
        return;
    }
    let keep = {
        let mut reg = world.res_mut::<ParamRegistry>();
        if !staged {
            let _ = evani_procedural::studio::apply(&mut reg);
            set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        }
        let id = reg.id("render.light.direction");
        match sun {
            Some((yaw, pitch)) => {
                let keep = if prev.is_none() { id.and_then(|i| reg.value(i)) } else { saved };
                let (el, az) = (pitch.to_radians(), yaw.to_radians());
                set(&mut reg, "render.light.direction", ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]));
                keep
            }
            None => {
                if let (Some(i), Some(v)) = (id, saved) {
                    let _ = reg.set(i, v);
                }
                None
            }
        }
    };
    let mut f = world.res_mut::<Field>();
    f.staged = true;
    f.sun_set = sun;
    f.sun_saved = keep;
}

/// The placed assets' bounds and mesh sizes, generated once per placement
/// set. An empty set (the mask pass) keeps the last bounds, so the camera
/// doesn't move between a view and its mask.
fn measure(world: &mut World) {
    let places: Vec<(String, [f32; 3], f32)> = match world.try_res::<ProcScene>() {
        Some(s) => s.placements.iter().map(|p| (p.feature.clone(), p.position, p.scale)).collect(),
        None => return,
    };
    if places.is_empty() {
        return;
    }
    let key = places.iter().map(|(f, p, s)| format!("{f}@{:?}x{s}", p)).collect::<Vec<_>>().join(";");
    if world.res::<Field>().key == key {
        return;
    }
    let style = world.try_res::<ProcScene>().map(|s| s.style.clone()).unwrap_or_default();
    let (mut bounds, mut tri, mut ver) = (None::<([f32; 3], [f32; 3])>, 0u64, 0u64);
    let mut ground = Vec::new();
    if let Some(lib) = world.try_res::<Registry>() {
        let reg = world.res::<ParamRegistry>();
        for (feature, pos, scale) in &places {
            let Some(g) = lib.generate(&reg, feature, &style) else { continue };
            tri += u64::from(g.output.triangles());
            ver += g.output.meshes().iter().map(|m| m.vertices.len() as u64).sum::<u64>();
            if let Some((lo, hi)) = g.output.bounds() {
                let lo = std::array::from_fn(|i| lo[i] * scale + pos[i]);
                let hi = std::array::from_fn(|i| hi[i] * scale + pos[i]);
                bounds = Some(match bounds {
                    Some((a, b)) => (std::array::from_fn(|i| f32::min(a[i], lo[i])), std::array::from_fn(|i| f32::max(b[i], hi[i]))),
                    None => (lo, hi),
                });
            }
            for m in g.output.meshes() {
                let at = |i: u32| m.vertices.get(i as usize).map(|v| [v[0] * scale + pos[0], v[1] * scale + pos[1], v[2] * scale + pos[2]]);
                for t in m.indices.as_chunks::<3>().0 {
                    let (Some(a), Some(b), Some(c)) = (at(t[0]), at(t[1]), at(t[2])) else { continue };
                    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    if l > 1e-12 && n[1] / l > 0.3 {
                        ground.push([a, b, c]);
                    }
                }
            }
        }
    }
    let mut f = world.res_mut::<Field>();
    f.key = key;
    f.bounds = bounds;
    f.ground = ground;
    f.triangles = tri;
    f.vertices = ver;
    f.assets = places.len();
    f.stats_sent = false;
}

/// One console line with the stats while `stats` is on (again after a new
/// placement set or after it was turned off and on).
fn report(world: &mut World, stats: bool) {
    let mut f = world.res_mut::<Field>();
    if !stats {
        f.stats_sent = false;
        return;
    }
    if f.stats_sent || f.key.is_empty() {
        return;
    }
    f.stats_sent = true;
    let (lo, hi) = f.bounds.unwrap_or(([0.0; 3], [0.0; 3]));
    let line = format!(
        "evani-field-stats {{\"triangles\":{},\"vertices\":{},\"assets\":{},\"bbox_min\":[{},{},{}],\"bbox_max\":[{},{},{}]}}",
        f.triangles, f.vertices, f.assets, lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
    );
    #[cfg(target_arch = "wasm32")]
    evani_core::web::console_log(false, &line);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = line;
}

// ------------------------------------------------------------ test scenes

/// The first CPU asset this build carries (the one the render app lists).
fn subject(lib: &Registry) -> Option<&dyn evani_procedural::ProcAsset> {
    lib.assets().find(|a| a.info().path == Path::Cpu)
}

/// The params pose `pose` of `scene` sets on asset `a`: its own `pose`
/// param when it has one, else the scene's table; clamped to each range.
fn pose_values(a: &dyn evani_procedural::ProcAsset, scene: u32, pose: u32) -> Vec<(&'static str, f32)> {
    let specs = Registry::specs(a);
    let range = |name: &str| {
        specs.iter().find(|s| s.name == name).and_then(|s| match s.kind {
            SpecKind::Num { min, max, .. } => Some((s.name, min, max)),
            _ => None,
        })
    };
    if let Some((n, lo, hi)) = range("pose") {
        return vec![(n, (pose as f32).clamp(lo, hi))];
    }
    let table: &[(&str, [f32; 4])] = if scene == 3 { &scenes::POSE_TABLE } else { &scenes::EXPRESSION_TABLE };
    table.iter().filter_map(|(name, v)| range(name).map(|(n, lo, hi)| (n, v[pose.min(3) as usize].clamp(lo, hi)))).collect()
}

/// The scene's sim step: `time_s` while `time_free`, else the sim clock.
fn step_now(world: &World, k: Knobs) -> u64 {
    let r = world.res::<ParamRegistry>();
    let t = if r.f32(k.time_free) >= 0.5 { f64::from(r.f32(k.time_s)) } else { world.try_res::<evani_core::time::Time>().map_or(0.0, |t| t.sim_seconds) };
    (t.max(0.0) / f64::from(scenes::DT)).round() as u64
}

/// The asset's wind (scene 1) as a closure, or none.
type WindBox = Box<scenes::WindField<'static>>;
type FlowBox = Box<scenes::FlowField<'static>>;

fn wind_of(lib: &Registry, reg: &ParamRegistry) -> WindBox {
    let b = lib.behaviours();
    for f in b.wind_features() {
        if let (Some(w), Some((v, seed)), true) = (b.wind(f), lib.values(reg, f), lib.is_on(reg, f)) {
            return Box::new(move |p, t| w(&v, seed, p, t));
        }
    }
    Box::new(|_, _| [0.0; 3])
}

fn flow_of(lib: &Registry, reg: &ParamRegistry) -> FlowBox {
    let b = lib.behaviours();
    for f in b.flow_features() {
        if let (Some(w), Some((v, seed)), true) = (b.flow(f), lib.values(reg, f), lib.is_on(reg, f)) {
            return Box::new(move |c, p, t| w(&v, seed, c, p, t));
        }
    }
    Box::new(|_, _, _| [0.0; 2])
}

/// The asset's meshes (local, its own parts) at poses 0..3 of `scene`.
fn posed_meshes(lib: &Registry, reg: &ParamRegistry, scene: u32) -> Vec<scenes::MeshSet> {
    let Some(a) = subject(lib) else { return Vec::new() };
    let f = a.info().feature();
    let Some((base, seed)) = lib.values(reg, &f) else { return Vec::new() };
    (0..4)
        .map(|p| {
            let mut v: Values = base.clone();
            for (n, x) in pose_values(a, scene, p) {
                v.set(n, Value::F32(x));
            }
            a.generate(&v, seed, evani_procedural::REALISM).meshes().iter().map(|m| (m.vertices.clone(), m.indices.clone())).collect()
        })
        .collect()
}

/// Pose the asset (scenes 3 and 4), put its params back after, and write
/// the measure line once while `measure` is on.
fn test_scene(world: &mut World, k: Knobs, scene: u32) {
    let (pose, on) = {
        let r = world.res::<ParamRegistry>();
        (r.f32(k.pose).round().clamp(0.0, 3.0) as u32, r.f32(k.measure) >= 0.5)
    };
    // The pose's params, written only when they differ (each write would
    // regenerate the asset).
    let want: Vec<(String, f32)> = match (scene, world.try_res::<Registry>()) {
        (3 | 4, Some(lib)) => subject(&lib).map(|a| {
            let p = a.info().prefix();
            pose_values(a, scene, pose).into_iter().map(|(n, x)| (format!("{p}.{n}"), x)).collect()
        }).unwrap_or_default(),
        _ => Vec::new(),
    };
    {
        let mut st = world.res_mut::<Staging>();
        let mut reg = world.res_mut::<ParamRegistry>();
        // Put back what this pose no longer sets.
        let saved = std::mem::take(&mut st.saved);
        for (n, v) in saved {
            if want.iter().any(|(w, _)| *w == n) {
                st.saved.push((n, v));
            } else if let Some(id) = reg.id(&n) {
                let _ = reg.set(id, v);
            }
        }
        for (n, x) in &want {
            let Some(id) = reg.id(n) else { continue };
            let cur = reg.value(id);
            if !st.saved.iter().any(|(s, _)| s == n)
                && let Some(c) = cur
            {
                st.saved.push((n.clone(), c));
            }
            if !matches!(cur, Some(ParamValue::F32(c)) if (c - x).abs() < 1e-6) {
                let _ = reg.set(id, ParamValue::F32(*x));
            }
        }
    }
    // The measure line.
    let sent = world.res::<Staging>().sent;
    if !on {
        world.res_mut::<Staging>().sent = None;
        return;
    }
    if sent == Some(scene) {
        return;
    }
    let m = {
        let Some(lib) = world.try_res::<Registry>() else { return };
        let reg = world.res::<ParamRegistry>();
        match scene {
            1 => scenes::wind_measures(&*wind_of(&lib, &reg)),
            2 => scenes::current_measures(&*flow_of(&lib, &reg)),
            3 => scenes::pose_measures(&posed_meshes(&lib, &reg, 3)),
            4 => scenes::expression_measures(&posed_meshes(&lib, &reg, 4)),
            _ => Vec::new(),
        }
    };
    let line = scenes::measure_line(&m);
    #[cfg(target_arch = "wasm32")]
    evani_core::web::console_log(false, &line);
    let mut st = world.res_mut::<Staging>();
    st.sent = Some(scene);
    st.line = Some(line);
}

fn inst(mesh: MeshId, pos: [f32; 3], scale: [f32; 3], rot: [f32; 4], mat: u32, rgb: [f32; 3]) -> MeshInstance {
    let mut m = MeshInstance::new(mesh, pos);
    m.scale = scale;
    m.rotation = rot;
    m.material = mat;
    m.color = [rgb[0], rgb[1], rgb[2], 1.0];
    m
}

/// A piece of height `h` standing at `base`, leaning by `d` (its top's
/// offset): its centre and rotation.
fn lean(base: [f32; 3], h: f32, d: [f32; 2]) -> ([f32; 3], [f32; 4]) {
    let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if l < 1e-6 {
        return ([base[0], base[1] + h * 0.5, base[2]], [0.0, 0.0, 0.0, 1.0]);
    }
    let th = (l / h).atan();
    let (s, c) = th.sin_cos();
    let q = evani_render::math::quat_axis_angle([d[1] / l, 0.0, -d[0] / l], th);
    ([base[0] + d[0] / l * s * h * 0.5, base[1] + c * h * 0.5, base[2] + d[1] / l * s * h * 0.5], q)
}

/// A small seeded number in [0, 1) (the scene's own, no rand).
fn hash01(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ k.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The wind meadow's and the stream's proxies at the scene's step (made
/// once per step and param revision, then drawn every frame).
pub fn draw_scene(world: &mut World) {
    let Some(k) = world.try_res::<Knobs>().map(|k| *k) else { return };
    let scene = scene_of(world);
    if !matches!(scene, 1 | 2) {
        return;
    }
    let n = step_now(world, k);
    let rev = world.res::<ParamRegistry>().revision();
    let fresh = world.res::<Staging>().drawn == Some((scene, n, rev));
    if !fresh {
        let mat = RenderWorld::material(&world.res::<LookData>(), "stone");
        let out = {
            let Some(lib) = world.try_res::<Registry>() else { return };
            let reg = world.res::<ParamRegistry>();
            if scene == 1 { meadow(&*wind_of(&lib, &reg), n, mat) } else { stream(&*flow_of(&lib, &reg), n, mat) }
        };
        let mut st = world.res_mut::<Staging>();
        st.instances = out;
        st.drawn = Some((scene, n, rev));
    }
    let list = world.res::<Staging>().instances.clone();
    let mut rw = world.res_mut::<RenderWorld>();
    for m in list {
        rw.draw_mesh(m);
    }
}

const ID: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The wind-field: the meadow, its grass, two young trees and the flag.
pub fn meadow(wind: &scenes::WindField<'_>, n: u64, mat: u32) -> Vec<MeshInstance> {
    let h = scenes::MEADOW_HALF;
    let mut out = vec![inst(MeshId::PLANE, [0.0, 0.02, 0.0], [2.0 * h + 1.0, 1.0, 2.0 * h + 1.0], ID, mat, [0.32, 0.4, 0.2])];
    // Knee-high grass: 48 x 48 tufts.
    let g = 48u32;
    for i in 0..g * g {
        let (x, z) = ((i % g) as f32 + hash01(i, 1), (i / g) as f32 + hash01(i, 2));
        let p = [-h + 2.0 * h * x / g as f32, -h + 2.0 * h * z / g as f32];
        let ht = scenes::GRASS_H * (0.75 + 0.5 * hash01(i, 3));
        let d = scenes::grass_at(wind, p, n);
        let (c, q) = lean([p[0], 0.02, p[1]], ht, [d[0] * ht / scenes::GRASS_H, d[1] * ht / scenes::GRASS_H]);
        let tint = 0.8 + 0.4 * hash01(i, 4);
        out.push(inst(MeshId::CONE, c, [0.13, ht, 0.13], q, mat, [0.28 * tint, 0.46 * tint, 0.16 * tint]));
    }
    // Two young broadleaf trees: a trunk and a crown of three clumps, all
    // leaning with the crown's sway.
    for p in scenes::TREES {
        let d = scenes::tree_at(wind, p, n);
        let th = scenes::TREE_H;
        let (c, q) = lean([p[0], 0.0, p[1]], th * 0.62, [d[0] * 0.62, d[1] * 0.62]);
        out.push(inst(MeshId::CYLINDER, c, [0.22, th * 0.62, 0.22], q, mat, [0.32, 0.25, 0.18]));
        for (j, (y, r)) in [(0.66f32, 1.25f32), (0.8, 1.05), (0.93, 0.7)].into_iter().enumerate() {
            let s = y * y;
            let jx = 0.25 * (hash01(j as u32, 9) - 0.5);
            out.push(inst(MeshId::LOW_SPHERE, [p[0] + d[0] * s + jx, th * y, p[1] + d[1] * s], [2.0 * r, 1.7 * r, 2.0 * r], ID, mat, [0.22, 0.38, 0.14]));
        }
    }
    // The flag on its pole: six strips along the line, the flap's wave
    // running out to the free edge.
    let fp = scenes::FLAG_POLE;
    out.push(inst(MeshId::CYLINDER, [fp[0], scenes::POLE_H * 0.5, fp[1]], [0.07, scenes::POLE_H, 0.07], ID, mat, [0.75, 0.75, 0.72]));
    let f = scenes::flag_at(wind, n);
    let e = f.extend.max(0.1);
    let (sa, ca) = f.azimuth.sin_cos();
    let q = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], -f.azimuth);
    let seg = 6;
    for j in 0..seg {
        let u = (j as f32 + 0.5) / seg as f32;
        let lat = 0.12 * e * u * (f.phase - 1.6 * std::f32::consts::PI * (1.0 - u)).sin();
        let r = scenes::FLAG_L * e * u;
        let y = scenes::POLE_H - 0.3 - scenes::FLAG_L * 0.8 * (1.0 - e) * u;
        out.push(inst(MeshId::CUBE, [fp[0] + ca * r - sa * lat, y, fp[1] + sa * r + ca * lat], [scenes::FLAG_L * e / seg as f32 + 0.01, 0.6, 0.02], q, mat, [0.7, 0.12, 0.1]));
    }
    out
}

/// The current-channel: the water, its banks, the rock and the leaves.
pub fn stream(flow: &scenes::FlowField<'_>, n: u64, mat: u32) -> Vec<MeshInstance> {
    let l = scenes::CHANNEL_HALF_LEN;
    let c = scenes::channel();
    let ([rx, rz], r) = scenes::ROCK;
    let mut out = vec![
        inst(MeshId::PLANE, [0.0, 0.0, 0.0], [2.0 * l, 1.0, 2.0 * c.half_width], ID, mat, [0.07, 0.2, 0.22]),
        inst(MeshId::CUBE, [0.0, 0.0, c.half_width + 2.0], [2.0 * l + 0.4, 0.5, 4.0], ID, mat, [0.3, 0.4, 0.18]),
        inst(MeshId::CUBE, [0.0, 0.0, -c.half_width - 2.0], [2.0 * l + 0.4, 0.5, 4.0], ID, mat, [0.3, 0.4, 0.18]),
        inst(MeshId::LOW_SPHERE, [rx, 0.05, rz], [2.1 * r, 1.4 * r, 2.0 * r], ID, mat, [0.42, 0.41, 0.38]),
    ];
    for (i, p) in scenes::leaves_at(flow, n).into_iter().enumerate() {
        let u = flow(&c, [p[0], 0.0, p[1]], n as f32 * scenes::DT);
        let q = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], -u[1].atan2(u[0]) + 0.6 * hash01(i as u32, 5));
        let tint = 0.8 + 0.4 * hash01(i as u32, 6);
        out.push(inst(MeshId::CUBE, [p[0], 0.015, p[1]], [0.13, 0.012, 0.08], q, mat, [0.75 * tint, 0.42 * tint, 0.12]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_free_camera_at_its_defaults_without_an_asset_is_the_fixed_one() {
        let d = [EYE[0] - TARGET[0], EYE[1] - TARGET[1], EYE[2] - TARGET[2]];
        let pitch = (d[1] / (d[0] * d[0] + d[2] * d[2]).sqrt()).atan().to_degrees();
        assert!((pitch - PITCH0).abs() < 0.01, "{pitch}");
    }

    #[test]
    fn the_far_plane_reaches_past_a_landform_kilometres_across() {
        // An object a metre across keeps the engine's 1 km.
        assert_eq!(super::far_for([0.0, 1.0, 3.0], [0.0, 0.5, 0.0], 0.8), 1000.0);
        // Mountains r12: 26 km of land (radius about 18.4 km) seen from about 32 km away.
        let far = super::far_for([0.0, 22_600.0, 22_600.0], [0.0, 600.0, 0.0], 18_400.0);
        assert!(far > 32_000.0 + 18_400.0 && far <= 100_000.0, "{far}");
        assert_eq!(super::far_for([0.0, 0.0, 900_000.0], [0.0; 3], 1.0), 100_000.0);
    }

    #[test]
    fn objects_get_the_grey_sweep_and_scenes_and_behaviours_keep_the_sky() {
        // Auto: the server's object views (orbit, no preset, no test scene).
        assert!(sweep_wanted(0, 1, 0, 0));
        // Its scene views: the player camera, the overview, the landform panels.
        assert!(!sweep_wanted(0, 2, 0, 0));
        assert!(!sweep_wanted(0, 3, 0, 0));
        assert!(!sweep_wanted(0, 1, 1, 0));
        assert!(!sweep_wanted(0, 1, 2, 0));
        // Behaviour frames (test scenes) and the field's fixed camera.
        for scene in 1..=4 {
            assert!(!sweep_wanted(0, 1, 0, scene));
            assert!(!sweep_wanted(0, 0, 0, scene));
        }
        assert!(!sweep_wanted(0, 0, 0, 0));
        // The switch: 1 always the sweep, 2 always the sky.
        assert!(sweep_wanted(1, 2, 1, 3) && !sweep_wanted(2, 1, 0, 0));
    }

    #[test]
    fn the_sweep_fades_the_floor_behind_the_subject_and_its_shadows_reach_it() {
        for (cam_d, r) in [(0.64, 0.45), (2.6, 1.0), (40.0, 15.0)] {
            let (near, far, shadow) = sweep_reach(cam_d, r);
            assert!(near > cam_d + r, "the fade starts behind the subject: {near} vs {}", cam_d + r);
            assert!(far > near * 1.5, "{far} vs {near}");
            assert!(shadow > cam_d + r, "{shadow}");
        }
        // Rounded to two figures, so a shader rebuild only follows a real change.
        assert_eq!(sweep_reach(0.64, 0.45), sweep_reach(0.641, 0.451));
        assert!((sig2(1.3149) - 1.3).abs() < 1e-5);
        assert_eq!(sig2(0.0), 0.0);
    }

    #[test]
    fn the_sweep_is_neutral_grey() {
        let grey = |c: [f32; 3]| (c[0] - c[1]).abs() < 0.05 && (c[1] - c[2]).abs() < 0.05 && (0.2..0.7).contains(&c[1]);
        assert!(grey(SWEEP_BACKDROP) && grey(SWEEP_FOG));
        assert!(grey([SWEEP_FLOOR[0], SWEEP_FLOOR[1], SWEEP_FLOOR[2]]));
        for name in SWEEP_ONLY {
            assert!(name == "look.stylizedpbr.haze_near" || name == "look.stylizedpbr.haze_far" || name == "fx.shadows.distance" || SWEEP.iter().any(|(n, _)| *n == name), "{name}");
        }
    }

    /// The 8-bit sRGB luminance a linear colour shows as after the sweep's
    /// exposure and the film's black offset (the inverse of studio_colour).
    fn shows(c: [f32; 3], gain: f32) -> f32 {
        let s = |l: f32| {
            let v = (SHOW_GAIN * gain * l - SHOW_OFFSET).max(0.0);
            255.0 * if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
        };
        0.2126 * s(c[0]) + 0.7152 * s(c[1]) + 0.0722 * s(c[2])
    }

    #[test]
    fn a_studio_grey_shows_as_that_grey_with_its_warmth() {
        for grey in [60.0, 112.0, 134.5, 153.5, 170.8, 220.0] {
            let c = studio_colour(grey, 0.0, 1.0);
            assert!((c[0] - c[1]).abs() < 1e-6 && (c[1] - c[2]).abs() < 1e-6, "neutral: {c:?}");
            assert!((shows(c, 1.0) - grey).abs() < 0.6, "{grey}: {}", shows(c, 1.0));
            // The floor's colour under its light gain shows the same grey.
            let g = floor_gain(40f32.to_radians());
            assert!((shows(studio_colour(grey, 0.0, g), g) - grey).abs() < 0.6);
        }
        // Warm: red over blue by about the warmth, the grey kept.
        let w = studio_colour(150.0, 20.0, 1.0);
        assert!(w[0] > w[1] && w[1] > w[2], "{w:?}");
        assert!((shows(w, 1.0) - 150.0).abs() < 1.5, "{}", shows(w, 1.0));
        let cool = studio_colour(150.0, -10.0, 1.0);
        assert!(cool[2] > cool[0]);
    }

    #[test]
    fn the_floor_takes_more_light_the_higher_the_sun() {
        let g = |deg: f32| floor_gain(deg.to_radians());
        assert!(g(20.0) < g(45.0) && g(45.0) < g(75.0));
        // The measured gains (renders of the sweep's floor, 2026-11-11).
        for (deg, want) in [(20.0, 1.136), (45.0, 1.32), (75.0, 1.43)] {
            assert!((g(deg) - want).abs() < 0.03, "{deg}: {} vs {want}", g(deg));
        }
        assert!((g(-10.0) - FLOOR_SKY).abs() < 1e-6, "a sun below the floor adds nothing");
    }

    #[test]
    fn the_aim_moves_the_target_up_or_down_by_bounds_radii() {
        // A radius-1 asset framed at 50 degrees: d0 = 1 / sin(25) * 1.1.
        let d0 = 1.0 / 25f32.to_radians().sin() * 1.1;
        assert_eq!(aimed([1.0, 2.0, 3.0], d0, 50.0, 0.0), [1.0, 2.0, 3.0]);
        let up = aimed([1.0, 2.0, 3.0], d0, 50.0, 0.5);
        assert!((up[1] - 2.5).abs() < 1e-5 && up[0] == 1.0 && up[2] == 3.0, "{up:?}");
        let down = aimed([0.0; 3], d0, 50.0, -1.0);
        assert!((down[1] + 1.0).abs() < 1e-5, "{down:?}");
    }
}
