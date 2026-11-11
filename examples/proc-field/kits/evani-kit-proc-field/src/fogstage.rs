//! The volumetric air effects' neutral staging (feature `fogstage`, the
//! Asset Studio distance fog session, column 74, and from 1.5.45 every air
//! effect on staging; docs/ASSET_STUDIO_GUIDE.md "Attribute assets: neutral
//! staging"). An air effect is an attribute, not a thing, so its reference
//! and its renders show it alone on the same plain staging
//! (catalog/procedural/ref_classes.json `assets.<asset>.staged`): flat,
//! featureless mid-grey ground to the horizon and plain matte grey boxes and
//! pillars at near, middle and far distance, from a standing player's eye
//! (1.7 m). Nothing in the frame but the effect changes from near to far, so
//! render and reference compare like for like.
//!
//! `kit.proc_field.fogstage_subject` picks the effect (`SUBJECTS`, the
//! catalogue's `staged.scene_subject`): 0 the distance fog (the default, as
//! before), 1 ground mist, 2 dust storms, 3 cloud layers, 4 volumetric-looking
//! clouds, 5 smoke, 6 steam, 7 dust clouds. Each has its own staging, laid out
//! to line up with its own stored reference picture (blocks solved from the
//! picture's outlines and the camera's tilt from its horizon), its own light
//! and sky, and the library asset placed where its picture shows it. The
//! app's `fogstage` feature builds the fog alone; `airstage` builds all eight
//! (an asset not built is simply not drawn).
//!
//! The distance fog (subject 0) is the procedural library's
//! `sky-distance-fog` alone (placed at the eye; its floor is the ground): a
//! depth-based height fog the library draws itself
//! (evani-procedural-lib's fog.rs, Asset Studio column 77), so the picture is
//! what a game that lists and places the asset gets. The look's own distance
//! haze is off for it (`HAZE_OFF`; `kit.proc_field.fogstage_haze_*` stay
//! live-tunable). Switching to another subject sets those three params to
//! that subject's starting haze and sky (they stay live-tunable). The
//! staging itself (blocks, ground, camera, light) is fixed: never add
//! terrain, grass, trees or textures to it.
//!
//! `kit.proc_field.fogstage_view`: 0 the reference's view, 1 the same
//! staging from 6 m up (how the effect thins with height).
//! `kit.proc_field.fogstage_asset`: 1 the library's effect on the staging
//! (the round's render), 0 the staging alone (a baseline; the fog's: no fog
//! at all).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The asset the fog scene shows (subject 0).
pub const ASSET: &str = "sky-distance-fog";

/// The staging's one grey (linear), blocks and ground alike.
pub const GREY: [f32; 4] = [0.42, 0.42, 0.43, 1.0];

/// The colour behind the fog's staging (linear): a pale grey-white. The
/// library's fog covers the whole sky in its own colour, so this shows only
/// with the asset off.
pub const FOG: [f32; 3] = [0.80, 0.81, 0.83];

/// The look's distance haze switched off (it starts and ends beyond anything
/// the staging draws): the fog's picture is the library asset alone.
pub const HAZE_OFF: [f32; 2] = [20000.0, 120000.0];

/// The reference's camera: eye height (m) and the vertical field of view of
/// a 60 degree horizontal one at its 1672 x 941 aspect (36.0 degrees).
pub const EYE: f32 = 1.7;
pub const FOV_Y: f32 = 36.0;
/// The fog reference camera's tilt down (degrees).
pub const PITCH_DEG: f32 = 0.9;

/// One block of the staging: its centre on the ground (x, z; the camera
/// looks down -z), its size (x, y, z) and which row it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub x: f32,
    pub z: f32,
    pub size: [f32; 3],
    pub row: Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Near,
    Middle,
    Far,
}

const fn b(x: f32, z: f32, size: [f32; 3], row: Row) -> Block {
    Block { x, z, size, row }
}

/// The fog's staging. The reference prompt (ref_classes.json `staged.show`) asks for a near pair of 1 m cubes
/// about 6 m away, left and right of centre; a middle group of a 2 m block
/// and a 4 m pillar about 30 m away; a far line of 6 m pillars from 80 m out
/// to 300 m, receding to the right.
pub fn blocks() -> Vec<Block> {
    FOG_BLOCKS.to_vec()
}

// Solved from the stored reference picture's own outlines (1672 x 941,
// generated-images/2026-11-11_0708_...: each block's left, right, top and
// ground line in pixels, through the camera below), so the staging and
// the picture line up and only the fog differs. The far pillars' bases
// are lost in the picture's ground fog, so their distances are set to
// step back (45 to 140 m) and their widths and tops follow from that.
const FOG_BLOCKS: &[Block] = &[
    b(-1.90, -7.78, [1.0, 1.0, 1.0], Row::Near),
    b(1.76, -7.56, [1.0, 1.0, 1.0], Row::Near),
    b(-2.31, -23.06, [1.94, 1.25, 1.5], Row::Middle),
    b(0.13, -22.22, [1.0, 2.72, 1.0], Row::Middle),
    b(8.97, -45.0, [1.46, 5.17, 1.46], Row::Far),
    b(20.33, -70.0, [1.69, 5.31, 1.69], Row::Far),
    b(35.74, -100.0, [1.86, 4.72, 1.86], Row::Far),
    b(57.09, -140.0, [2.03, 4.38, 2.03], Row::Far),
];

// The other effects' stagings (1.5.45), each solved the same way from its own
// stored reference (.evani/shots/air-effect-refs/ref-<asset>.png; generated-
// images/2026-11-11_094*): the camera's tilt from the picture's horizon, each
// near and middle block from its ground line, left, right and top edges, and
// a far pillar whose base is lost on the horizon from its known height
// (the wording's) and its outline. A block lower than the eye is sized from
// its back top edge, as the picture shows it.

/// Ground mist: the fog's rows (cubes ~10 m, a block and a pillar ~31 m, far
/// pillars 78 to 242 m), its picture's camera level.
const MIST: &[Block] = &[
    b(-2.69, -10.52, [1.22, 0.69, 1.22], Row::Near),
    b(2.73, -10.54, [1.10, 0.66, 1.10], Row::Near),
    b(-1.85, -31.75, [1.82, 1.00, 1.50], Row::Middle),
    b(1.04, -31.53, [1.13, 2.64, 1.13], Row::Middle),
    b(-30.70, -78.75, [1.50, 5.80, 1.50], Row::Far),
    b(23.20, -94.75, [1.50, 5.56, 1.50], Row::Far),
    b(60.00, -128.75, [1.50, 5.46, 1.50], Row::Far),
    b(-81.00, -167.75, [1.50, 5.70, 1.50], Row::Far),
    b(-39.70, -167.75, [1.50, 7.06, 1.50], Row::Far),
    b(25.20, -197.75, [1.50, 6.67, 1.50], Row::Far),
    b(-23.80, -241.75, [1.50, 7.90, 1.50], Row::Far),
];

/// Dust storms, at horizon scale: cubes ~16 m, a pair of tall pillars
/// ~150 m (their bases sit on the picture's horizon), and 25 m pillars
/// from 260 to 600 m behind the dust front (lost in it, as the picture).
const DUST_STORM: &[Block] = &[
    b(-3.99, -15.80, [1.29, 1.19, 1.29], Row::Near),
    b(4.34, -15.89, [1.08, 1.20, 1.08], Row::Near),
    b(-27.70, -151.35, [2.70, 16.20, 2.70], Row::Middle),
    b(28.10, -151.35, [2.70, 16.20, 2.70], Row::Middle),
    b(50.00, -261.50, [3.00, 25.00, 3.00], Row::Far),
    b(-120.00, -381.50, [3.00, 25.00, 3.00], Row::Far),
    b(160.00, -481.50, [3.00, 25.00, 3.00], Row::Far),
    b(-60.00, -601.50, [3.00, 25.00, 3.00], Row::Far),
];

/// Cloud layers, at sky scale (camera tilted up 19 degrees, the picture is
/// 1536 x 1024): cubes ~20 m, 3 m pillars ~76 m, 38 m pillars 680 m to 1 km.
const CLOUD_LAYERS: &[Block] = &[
    b(-5.28, -19.62, [0.91, 0.68, 0.91], Row::Near),
    b(5.31, -19.62, [0.90, 0.68, 0.90], Row::Near),
    b(-10.19, -75.75, [0.92, 3.12, 0.92], Row::Middle),
    b(10.30, -75.79, [0.99, 3.19, 0.99], Row::Middle),
    b(-435.26, -681.44, [5.77, 37.46, 5.77], Row::Far),
    b(447.65, -693.52, [6.52, 37.44, 6.52], Row::Far),
    b(-462.21, -955.47, [4.50, 37.90, 4.50], Row::Far),
    b(492.56, -1003.72, [5.67, 38.79, 5.67], Row::Far),
];

/// Volumetric-looking clouds, at sky scale (camera tilted up 16 degrees):
/// cubes ~23 m, 8 m pillars ~180 m, tall pillars 1.9 to 2.4 km (the farthest
/// brought in to the ground's edge with the same outline).
const CUMULUS: &[Block] = &[
    b(-7.44, -22.92, [1.08, 0.80, 1.08], Row::Near),
    b(7.43, -22.67, [1.05, 0.81, 1.05], Row::Near),
    b(-26.29, -175.64, [1.85, 7.99, 1.85], Row::Middle),
    b(28.38, -192.40, [2.19, 8.59, 2.19], Row::Middle),
    b(779.50, -1900.66, [11.72, 48.65, 11.72], Row::Far),
    b(-72.03, -2404.24, [8.47, 38.51, 8.47], Row::Far),
    b(131.34, -2404.24, [8.48, 36.46, 8.48], Row::Far),
    b(-1279.86, -2404.24, [8.48, 32.36, 8.48], Row::Far),
];

/// Smoke: cubes ~10 m; in the middle (~34 m) the source pillar (the smoke
/// leaves its top) and a tall pillar to its left; far pillars 72 to 171 m.
const SMOKE: &[Block] = &[
    b(-2.79, -9.86, [1.21, 1.03, 1.21], Row::Near),
    b(2.67, -9.85, [1.20, 1.12, 1.20], Row::Near),
    b(-3.12, -34.17, [1.47, 6.22, 1.47], Row::Middle),
    b(-0.59, -33.70, [1.40, 2.74, 1.40], Row::Middle),
    b(-34.13, -72.14, [1.86, 7.35, 1.86], Row::Far),
    b(28.01, -75.11, [1.69, 7.44, 1.69], Row::Far),
    b(22.60, -98.39, [1.54, 7.48, 1.54], Row::Far),
    b(-35.30, -118.21, [1.62, 7.76, 1.62], Row::Far),
    b(62.26, -123.34, [1.94, 7.76, 1.94], Row::Far),
    b(14.17, -150.46, [1.34, 7.78, 1.34], Row::Far),
    b(-30.59, -171.06, [1.41, 7.91, 1.41], Row::Far),
];

/// Steam: as the smoke's (its own picture): the source pillar ~33 m.
const STEAM: &[Block] = &[
    b(-3.27, -10.89, [1.27, 1.14, 1.27], Row::Near),
    b(3.05, -11.02, [1.25, 1.13, 1.25], Row::Near),
    b(-3.81, -36.51, [1.28, 7.08, 1.28], Row::Middle),
    b(-0.32, -33.15, [1.09, 2.95, 1.09], Row::Middle),
    b(-23.21, -47.84, [0.72, 7.49, 0.72], Row::Far),
    b(14.63, -52.09, [0.75, 7.46, 0.75], Row::Far),
    b(30.67, -58.79, [0.96, 7.42, 0.96], Row::Far),
    b(-23.62, -72.83, [0.85, 7.49, 0.85], Row::Far),
    b(12.72, -97.99, [0.74, 7.75, 0.74], Row::Far),
    b(44.82, -109.05, [0.90, 7.76, 0.90], Row::Far),
    b(-22.99, -126.30, [0.78, 7.76, 0.78], Row::Far),
    b(-58.34, -142.87, [0.88, 7.77, 0.88], Row::Far),
];

/// Dust clouds: cubes ~10 m; the source pillar (the dust bursts at its foot)
/// ~31 m and a tall pillar ~27 m (their bases hidden in the dust: placed from
/// their widths); far pillars 62 to 165 m.
const DUST_CLOUD: &[Block] = &[
    b(-3.18, -10.38, [1.14, 1.05, 1.14], Row::Near),
    b(3.11, -10.39, [1.17, 1.05, 1.17], Row::Near),
    b(-3.66, -26.70, [1.20, 7.47, 1.20], Row::Middle),
    b(-0.10, -30.50, [1.20, 3.73, 1.20], Row::Middle),
    b(-31.20, -62.60, [1.20, 6.00, 1.20], Row::Far),
    b(-26.90, -74.60, [1.20, 6.00, 1.20], Row::Far),
    b(-22.00, -84.60, [1.20, 6.00, 1.20], Row::Far),
    b(36.60, -124.60, [1.20, 6.00, 1.20], Row::Far),
    b(60.20, -145.60, [1.20, 6.00, 1.20], Row::Far),
    b(84.50, -164.60, [1.20, 6.00, 1.20], Row::Far),
];

/// How a staging is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    /// Soft overcast: light from high overhead, low, and a bright even
    /// ambient; faint shadows only (the fog's).
    Overcast,
    /// A dust storm's: a high sun dimmed and turned warm brown.
    Dust,
    /// A clear afternoon's: the sun behind the camera and to its right.
    Sun,
}

/// One effect on the staging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subject {
    /// The library asset (its key: `<category>-<id>`).
    pub asset: &'static str,
    /// Where it is placed (its base).
    pub at: [f32; 3],
    /// Its placement's uniform scale, so the library asset at its own
    /// default size reads at the staging's scale as its picture shows it
    /// (a scene setting: the asset's recipe and params are its own).
    pub scale: f32,
    /// The reference camera's tilt down (degrees; negative looks up).
    pub pitch_deg: f32,
    /// The vertical field of view (degrees) of the reference's horizontal
    /// one at the reference picture's size.
    pub fov_y: f32,
    /// The reference picture's size (capture at it: `asset-shoot --size`).
    pub size: [u32; 2],
    pub light: Light,
    /// The sky at the horizon (and the haze's colour), linear.
    pub horizon: [f32; 3],
    /// The sky overhead, linear (the fog's sky is all its colour).
    pub zenith: [f32; 3],
    /// The look's distance haze: starts (m) and is the horizon's colour (m).
    pub haze: [f32; 2],
    pub blocks: &'static [Block],
}

/// Every staged effect, by `kit.proc_field.fogstage_subject` (= the
/// catalogue's `staged.scene_subject`).
pub const SUBJECTS: &[Subject] = &[
    Subject { asset: ASSET, at: [0.0, 0.0, 0.0], scale: 1.0, pitch_deg: PITCH_DEG, fov_y: FOV_Y, size: [1672, 941], light: Light::Overcast, horizon: FOG, zenith: FOG, haze: HAZE_OFF, blocks: FOG_BLOCKS },
    Subject { asset: "sky-ground-mist", at: [0.0, 0.0, -60.0], scale: 15.0, pitch_deg: 0.0, fov_y: FOV_Y, size: [1672, 941], light: Light::Overcast, horizon: [0.80, 0.83, 0.88], zenith: [0.33, 0.45, 0.66], haze: [60.0, 3000.0], blocks: MIST },
    Subject { asset: "sky-dust-storms", at: [0.0, 0.0, -200.0], scale: 30.0, pitch_deg: -5.35, fov_y: FOV_Y, size: [1672, 941], light: Light::Dust, horizon: [0.30, 0.17, 0.08], zenith: [0.12, 0.07, 0.04], haze: [10.0, 400.0], blocks: DUST_STORM },
    Subject { asset: "sky-cloud-layers", at: [0.0, 0.0, -1300.0], scale: 62.5, pitch_deg: -19.07, fov_y: 54.18, size: [1536, 1024], light: Light::Sun, horizon: [0.55, 0.66, 0.82], zenith: [0.13, 0.27, 0.62], haze: [300.0, 20000.0], blocks: CLOUD_LAYERS },
    Subject { asset: "sky-volumetric-looking-clouds", at: [300.0, 0.0, -1200.0], scale: 125.0, pitch_deg: -15.96, fov_y: 46.71, size: [1672, 941], light: Light::Sun, horizon: [0.45, 0.60, 0.82], zenith: [0.03, 0.12, 0.42], haze: [300.0, 20000.0], blocks: CUMULUS },
    Subject { asset: "effects-smoke", at: [-0.59, 2.74, -33.70], scale: 3.5, pitch_deg: -4.99, fov_y: FOV_Y, size: [1672, 941], light: Light::Overcast, horizon: [0.45, 0.46, 0.49], zenith: [0.40, 0.41, 0.44], haze: [15.0, 600.0], blocks: SMOKE },
    Subject { asset: "effects-steam", at: [-0.32, 2.95, -33.15], scale: 3.0, pitch_deg: -5.19, fov_y: FOV_Y, size: [1672, 941], light: Light::Overcast, horizon: [0.45, 0.46, 0.49], zenith: [0.40, 0.41, 0.44], haze: [15.0, 600.0], blocks: STEAM },
    Subject { asset: "effects-dust-clouds", at: [-0.10, 0.0, -30.50], scale: 4.0, pitch_deg: -4.80, fov_y: FOV_Y, size: [1672, 941], light: Light::Overcast, horizon: [0.50, 0.51, 0.53], zenith: [0.45, 0.46, 0.48], haze: [15.0, 600.0], blocks: DUST_CLOUD },
];

/// The subject a param value picks (rounded, clamped to the list).
pub fn subject(v: f32) -> &'static Subject {
    let i = if v.is_finite() { v.round().clamp(0.0, (SUBJECTS.len() - 1) as f32) as usize } else { 0 };
    &SUBJECTS[i]
}

#[derive(Debug, Clone, Copy)]
pub struct FogStage {
    view: F32Param,
    asset: F32Param,
    subject: F32Param,
    near: F32Param,
    far: F32Param,
    colour: Vec3Param,
    /// The (view, near, far, colour, subject) the scene was last set for (NaN: not yet).
    set_for: [f32; 7],
    /// The subject whose starting haze and sky were last written (NaN: none
    /// yet; the fog's are the params' own defaults).
    started: f32,
}

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.fogstage_view", 0.0)
            .range(0.0, 1.0)
            .step(1.0)
            .describe("The fog staging's view: 0 the reference's (eye height), 1 the same staging from 6 m up"),
    )?;
    let asset = kit.param_f32(
        ParamDef::f32("kit.proc_field.fogstage_asset", 1.0)
            .range(0.0, 1.0)
            .step(1.0)
            .describe("1 the library's air effect on the staging (the round's render), 0 the staging alone (its baseline)"),
    )?;
    let subject = kit.param_f32(
        ParamDef::f32("kit.proc_field.fogstage_subject", 0.0)
            .range(0.0, (SUBJECTS.len() - 1) as f32)
            .step(1.0)
            .describe("The air effect on the staging: 0 distance fog, 1 ground mist, 2 dust storms, 3 cloud layers, 4 volumetric-looking clouds, 5 smoke, 6 steam, 7 dust clouds (each with its own staging, camera and light, lined up with its reference)"),
    )?;
    let near = kit.param_f32(
        ParamDef::f32("kit.proc_field.fogstage_haze_near", HAZE_OFF[0])
            .range(0.0, 20000.0)
            .describe("Where the look's distance haze starts (m); off for the distance fog, whose fog is the library asset's own"),
    )?;
    let far = kit.param_f32(
        ParamDef::f32("kit.proc_field.fogstage_haze_far", HAZE_OFF[1])
            .range(1.0, 120000.0)
            .describe("Where the look's distance haze is the backdrop's colour (m)"),
    )?;
    let colour = kit.param_vec3(
        ParamDef::color("kit.proc_field.fogstage_colour", FOG)
            .range(0.0, 2.0)
            .describe("The backdrop's colour (linear): the sky behind the staging and the look's haze"),
    )?;
    kit.resource(FogStage {
        view,
        asset,
        subject,
        near,
        far,
        colour,
        set_for: [f32::NAN; 7],
        started: f32::NAN,
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

/// The fog's camera for each view: looking down -z from the origin.
pub fn pose(view: f32) -> CameraPose {
    pose_for(&SUBJECTS[0], view)
}

/// A subject's camera for each view: 0 its reference's (eye height, its
/// tilt), 1 the same from 6 m up.
pub fn pose_for(s: &Subject, view: f32) -> CameraPose {
    let h = if view > 0.5 { 6.0 } else { EYE };
    // Tilted as the reference's camera (the fog's: its near cubes' and middle
    // block's outlines put it there; the others': their horizons).
    let drop = 100.0 * s.pitch_deg.to_radians().tan();
    CameraPose {
        eye: [0.0, h, 0.0],
        target: [0.0, h - drop, -100.0],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: s.fov_y,
    }
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<FogStage>();
    let sv = world.res::<ParamRegistry>().f32(g.subject).round();
    let sub = subject(sv);
    // Another subject: its starting haze and sky into the live params (the fog
    // at start keeps the params' own defaults, as before).
    if sv != g.started && !(g.started.is_nan() && sv == 0.0) {
        let mut reg = world.res_mut::<ParamRegistry>();
        set(&mut reg, "kit.proc_field.fogstage_haze_near", ParamValue::F32(sub.haze[0]));
        set(&mut reg, "kit.proc_field.fogstage_haze_far", ParamValue::F32(sub.haze[1]));
        set(&mut reg, "kit.proc_field.fogstage_colour", ParamValue::Color(sub.horizon));
        drop(reg);
        g.started = sv;
    } else if g.started.is_nan() {
        g.started = sv;
    }
    let (view, near, far, colour) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round(),
            reg.f32(g.near),
            reg.f32(g.far),
            reg.vec3(g.colour),
        )
    };
    let now = [view, near, far, colour[0], colour[1], colour[2], sv];
    if now != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        // A reference picture holds every effect (dev scene only).
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        let (dir, col, int, amb, shadow) = match sub.light {
            // Soft overcast: light from high overhead, low, and a bright even
            // ambient; faint shadows only.
            Light::Overcast => ([0.15, 0.97, 0.2], [1.0, 1.0, 1.0], 1.1, [0.62, 0.63, 0.65], 0.25),
            // A high sun ahead, dimmed and turned warm brown by the dust.
            Light::Dust => ([0.1, 0.95, -0.3], [1.0, 0.72, 0.45], 0.6, [0.40, 0.28, 0.18], 0.15),
            // Behind the camera and to its right (the cubes' shadows fall left).
            Light::Sun => ([0.45, 0.55, 0.70], [1.0, 0.97, 0.92], 2.2, [0.35, 0.42, 0.55], 0.6),
        };
        set(&mut reg, "render.light.direction", ParamValue::Vec3(dir));
        set(&mut reg, "render.light.color", ParamValue::Color(col));
        set(&mut reg, "render.light.intensity", ParamValue::F32(int));
        set(&mut reg, "render.ambient", ParamValue::Color(amb));
        set(&mut reg, "fx.shadows.strength", ParamValue::F32(shadow));
        set(
            &mut reg,
            "look.stylizedpbr.pcss_light_deg",
            ParamValue::F32(25.0),
        );
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.05));
        set(&mut reg, "render.exposure", ParamValue::F32(0.85));
        // The sky behind the staging is the haze's colour (the fog's: all of
        // it), and the distance haze fades the ground and blocks into it.
        set(&mut reg, "render.clear_color", ParamValue::Color(colour));
        set(&mut reg, "render.sky.horizon", ParamValue::Color(colour));
        set(&mut reg, "render.sky.ground", ParamValue::Color(colour));
        set(&mut reg, "render.fog.color", ParamValue::Color(colour));
        if sv != 0.0 || g.set_for[6] > 0.0 {
            // Another effect's sky overhead (back to the fog: its colour again).
            let z = if sv == 0.0 { colour } else { sub.zenith };
            set(&mut reg, "render.sky.zenith", ParamValue::Color(z));
        }
        set(
            &mut reg,
            "look.stylizedpbr.haze_near",
            ParamValue::F32(near),
        );
        set(
            &mut reg,
            "look.stylizedpbr.haze_far",
            ParamValue::F32(far.max(near + 1.0)),
        );
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(60.0));
        drop(reg);
        g.set_for = now;
    }
    *world.res_mut::<FogStage>() = g;
    let with_asset = world.res::<ParamRegistry>().f32(g.asset) > 0.5;
    if let Some(mut s) = world.try_res_mut::<ProcScene>() {
        let placed = s.placements.len() == 1
            && s.placements[0].feature == sub.asset
            && s.placements[0].position == sub.at
            && s.placements[0].scale == sub.scale;
        if with_asset && !placed {
            s.placements.clear();
            let i = s.place(sub.asset, sub.at);
            s.placements[i].scale = sub.scale;
        } else if !with_asset && !s.placements.is_empty() {
            s.placements.clear();
        }
    }
    // The look's neutral "stone" (the studio floors' material), tinted the
    // staging's one grey: blocks and ground alike, no texture of their own.
    let grey = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = pose_for(sub, view);
    // Flat, featureless grey ground to the horizon.
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, -1000.0]);
    f.scale = [4000.0, 1.0, 4000.0];
    f.material = grey;
    f.color = GREY;
    rw.draw_mesh(f);
    for b in sub.blocks {
        let mut m = MeshInstance::new(MeshId::CUBE, [b.x, b.size[1] * 0.5, b.z]);
        m.scale = b.size;
        m.material = grey;
        m.color = GREY;
        rw.draw_mesh(m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The staging matches its reference wording (ref_classes.json
    /// `staged.show`) as the picture drew it: near cubes about 7.5 m away,
    /// middle blocks about 22 m, far pillars stepping back from 45 m, every block in front of the camera
    /// and inside the 60 degree view.
    #[test]
    fn the_staging_is_near_middle_and_far_grey_blocks_in_view() {
        let b = blocks();
        let dist = |r: Row| {
            b.iter()
                .filter(|x| x.row == r)
                .map(|x| -x.z)
                .collect::<Vec<_>>()
        };
        assert!(dist(Row::Near).iter().all(|d| (6.0..=9.0).contains(d)));
        assert!(dist(Row::Middle).iter().all(|d| (20.0..=25.0).contains(d)));
        let far = dist(Row::Far);
        assert!(far.windows(2).all(|w| w[1] > w[0]) && far.len() == 4);
        assert!(far.first().is_some_and(|d| *d >= 40.0) && far.last().is_some_and(|d| *d >= 120.0));
        let half = 30.0_f32.to_radians().tan();
        for x in &b {
            assert!(
                x.z < 0.0 && x.x.abs() + x.size[0] * 0.5 < -x.z * half + 0.5,
                "{x:?} is outside the view"
            );
        }
        assert!(
            b.iter()
                .filter(|x| x.row == Row::Near)
                .all(|x| x.size == [1.0, 1.0, 1.0])
        );
        assert!(
            b.iter()
                .filter(|x| x.row == Row::Far)
                .all(|x| x.size[1] > 4.0)
        );
        // no block hides another: their bearings from the eye are at least 2 degrees apart beyond the near pair
        let mut bearings: Vec<f32> = b
            .iter()
            .filter(|x| x.row != Row::Near)
            .map(|x| (x.x / -x.z).atan().to_degrees())
            .collect();
        bearings.sort_by(f32::total_cmp);
        assert!(
            bearings.windows(2).all(|w| w[1] - w[0] >= 2.0),
            "{bearings:?}"
        );
    }

    #[test]
    fn the_reference_view_is_at_eye_height_tilted_as_the_picture() {
        let p = pose(0.0);
        assert_eq!(p.eye[1], EYE);
        let tilt = ((p.eye[1] - p.target[1]) / (p.eye[2] - p.target[2]))
            .atan()
            .to_degrees();
        assert!((tilt - PITCH_DEG).abs() < 0.01, "{tilt}");
        // 60 degrees across at 1672 x 941.
        let h = 2.0
            * ((FOV_Y * 0.5).to_radians().tan() * 1672.0 / 941.0)
                .atan()
                .to_degrees();
        assert!((h - 60.0).abs() < 0.3, "{h}");
        assert!(pose(1.0).eye[1] > EYE);
    }

    /// The staging is the catalogue's: ref_classes.json names this scene for
    /// the staged asset, and its wording has the same three distances.
    #[test]
    fn the_catalogue_names_this_staging() {
        let rc = include_str!("../../../../../catalog/procedural/ref_classes.json");
        assert!(rc.contains("\"engine_scene\": \"fogstage\""));
        for words in [
            "1 m cubes about 6 m away",
            "about 30 m away",
            "from about 80 m out to 300 m",
        ] {
            assert!(
                rc.contains(words),
                "ref_classes.json staged.show lacks {words:?}"
            );
        }
    }

    /// The fog is subject 0 and its staging, camera and light are as before
    /// the other effects joined (1.5.44); since column 77 its fog is the
    /// library asset alone, the look's haze off.
    #[test]
    fn the_fog_is_subject_zero_unchanged() {
        let f = subject(0.0);
        assert_eq!(f.asset, ASSET);
        assert_eq!(f.blocks, blocks().as_slice());
        assert_eq!(f.light, Light::Overcast);
        assert_eq!(pose_for(f, 0.0), pose(0.0));
        assert_eq!((f.at, f.scale), ([0.0, 0.0, 0.0], 1.0));
        assert_eq!((f.horizon, f.haze), (FOG, HAZE_OFF));
        assert!(HAZE_OFF[0] > 3000.0 && HAZE_OFF[1] > HAZE_OFF[0], "beyond the staging's ground");
        assert!(subject(f32::NAN) == f && subject(-3.0) == f);
        assert_eq!(subject(99.0).asset, "effects-dust-clouds");
    }

    /// Every staged asset in the catalogue has its subject here, at its
    /// `scene_subject`, and every subject is a staged asset with its
    /// capture line naming that number.
    #[test]
    fn every_staged_asset_has_its_subject() {
        let rc = include_str!("../../../../../catalog/procedural/ref_classes.json");
        let lib = include_str!("../../../../../catalog/procedural/library.json");
        assert_eq!(rc.matches("\"scene_subject\": ").count(), SUBJECTS.len());
        assert_eq!(lib.matches("\"staging\": \"isolated\"").count(), SUBJECTS.len());
        for (i, s) in SUBJECTS.iter().enumerate() {
            let at = rc
                .find(&format!("  \"{}\": {{", s.asset))
                .unwrap_or_else(|| panic!("{} is not in ref_classes.json", s.asset));
            let entry = &rc[at..at + rc[at..].find("\n  \"").unwrap_or(rc.len() - at)];
            assert!(entry.contains(&format!("\"scene_subject\": {i},")), "{} is not scene_subject {i}", s.asset);
            if i > 0 {
                assert!(entry.contains(&format!("kit.proc_field.fogstage_subject {i},")), "{}'s capture line", s.asset);
            }
        }
    }

    /// Each effect's staging: near, middle and far rows, every block in front
    /// of the camera and inside its view, near rows lower than the eye, far
    /// ones farther and taller than the middle ones, and its camera at eye
    /// height with its picture's tilt (sky effects look up).
    #[test]
    fn every_effects_staging_is_in_view_and_in_rows() {
        for s in SUBJECTS {
            let rows = |r: Row| s.blocks.iter().filter(|x| x.row == r).collect::<Vec<_>>();
            let (n, m, f) = (rows(Row::Near), rows(Row::Middle), rows(Row::Far));
            assert!(n.len() == 2 && m.len() == 2 && f.len() >= 4, "{}", s.asset);
            let max_d = |v: &Vec<&Block>| v.iter().map(|x| -x.z).fold(0.0, f32::max);
            let min_d = |v: &Vec<&Block>| v.iter().map(|x| -x.z).fold(f32::MAX, f32::min);
            assert!(max_d(&n) < min_d(&m) && max_d(&m) < min_d(&f), "{}: rows out of order", s.asset);
            assert!(n.iter().all(|x| x.size[1] < EYE), "{}", s.asset);
            assert!(min_d(&f) > 40.0 && max_d(&f) < 3000.0, "{}", s.asset);
            let aspect = s.size[0] as f32 / s.size[1] as f32;
            let half = (s.fov_y * 0.5).to_radians().tan() * aspect;
            for x in s.blocks {
                assert!(x.z < 0.0 && x.x.abs() + x.size[0] * 0.5 < -x.z * half, "{}: {x:?} is outside the view", s.asset);
            }
            let p = pose_for(s, 0.0);
            assert_eq!(p.eye[1], EYE);
            let up = s.asset.contains("cloud") && s.asset.starts_with("sky-");
            assert!(if up { s.pitch_deg < -10.0 } else { s.pitch_deg.abs() < 6.0 }, "{}", s.asset);
            assert!((20.0..=3000.0).contains(&-s.at[2]) || s.asset == ASSET, "{}: placed in front of the camera", s.asset);
            assert!((1.0..=200.0).contains(&s.scale), "{}", s.asset);
        }
        // a sky effect's horizon in the bottom quarter, as its picture: the tilt is (horizon - centre) / focal
        for s in SUBJECTS.iter().filter(|s| s.pitch_deg < -10.0) {
            let below = (-s.pitch_deg).to_radians().tan() / (s.fov_y * 0.5).to_radians().tan();
            assert!((0.6..0.75).contains(&below), "{}: horizon at {below}", s.asset);
        }
    }

    /// Smoke and steam leave the top of the source pillar; the dust cloud
    /// bursts at its foot.
    #[test]
    fn plumes_start_at_the_source_pillar() {
        for s in SUBJECTS.iter().filter(|s| s.asset.starts_with("effects-")) {
            let src = s.blocks.iter().filter(|x| x.row == Row::Middle).min_by(|a, b| a.size[1].total_cmp(&b.size[1]));
            let src = src.copied().unwrap_or(s.blocks[0]);
            assert!((s.at[0] - src.x).abs() < 0.2 && (s.at[2] - src.z).abs() < 1.0, "{}", s.asset);
            let want = if s.asset == "effects-dust-clouds" { 0.0 } else { src.size[1] };
            assert!((s.at[1] - want).abs() < 0.01, "{}", s.asset);
        }
    }
}
