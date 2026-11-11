//! The rivers' live scene (feature `river`, the Asset Studio rivers session,
//! column 20, 2026-11-07): the procedural library's river valley
//! (`water-rivers`) seen the way its reference picture shows it, as a game
//! shows it: outdoors under the Preetham sky dome (evani-material-proc's
//! `sky`, the photoreal look's daylight preset layered over the film look,
//! as the Material library does), a mid-morning sun from one side and a
//! light haze in the distance, its water drawn by the nature pass and the
//! long grass on the banks by evani-grass, both handed over the way a game
//! hands them (`river::water`, `river::bank_grass`).
//!
//! `kit.proc_field.river_view`: 0 the reference's left panel (the aerial
//! overview: high over the valley, 42 degrees down from 340 m, the
//! river's bends winding away to the mountains), 1 its right panel (the
//! player's eye: 1.7 m over the outer bank of the first bend, a 60 degree
//! view, the horizon on the upper third, the river passing and bending
//! away, the gravel bar on the far bank), 2 a close-up of that bar from low
//! on its own bank.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_core::slots::{ShaderSlots, SlotRebuilds};
use evani_procedural::{ProcScene, Registry};
use evani_procedural_lib::evani_grass::Grass;
use evani_procedural_lib::evani_material_nature::NatureSurfaces;
use evani_procedural_lib::river;
use evani_render::RenderWorld;

#[derive(Debug, Clone, Copy)]
pub struct River {
    view: F32Param,
    yaw: F32Param,
    el: F32Param,
    dist: F32Param,
    sun_az: F32Param,
    sun_el: F32Param,
    look: F32Param,
    up: F32Param,
    sky_radius: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
    /// The view, turn and stand point the player's eye was last worked out
    /// for (NaN: not yet).
    eye_for: [f32; 3],
    /// Whether the bank grass was handed to evani-grass, and the frames
    /// spent waiting for its resource; the eye it was laid round.
    grass: bool,
    grass_wait: u32,
    grass_at: [f32; 3],
    /// The player's eye and looking direction (plan), and the water level,
    /// worked out from the valley when the view was set.
    eye: [f32; 3],
    dir: [f32; 2],
    wl: f32,
    /// The current's heading on that reach (degrees, 0 +x, 90 +z).
    flow: f32,
    /// Frames since the haze body was last requested.
    fog_wait: u32,
}

/// The aerial view: heading round the aim (degrees from +z towards +x),
/// elevation (degrees), distance (m), aim and lens (vertical degrees).
/// Round 7: 42 degrees down from 340 m, the aim on the valley's middle:
/// wider than round 4's (the bends and the wooded slopes beside them in
/// frame) while the land stays inside the haze's reach so the frame holds the valley's bends, its side
/// hills and the river running on into the hazy distance, as the
/// reference's overview does (round 4: 44 degrees from 215 m, the river
/// ran out of the frame).
const YAW: f32 = 4.0;
const EL: f32 = 42.0;
const DIST: f32 = 340.0;
const AIM: [f32; 3] = [0.0, 4.0, 40.0];
const FOV: f32 = 60.0;
/// The player's eye height over the ground (m) and pitch (degrees down:
/// the horizon on the upper third of a 60 degree view).
const EYE: f32 = 1.7;
const PITCH: f32 = 10.9;
/// The bend the player stands by (design plan point, m) and how far out
/// from the water's edge on its outer bank (m).
const BEND: [f32; 2] = [-86.0, 46.0];
const OUT: f32 = 5.5;
/// How far upstream of the bend's apex the player stands (m), and how far
/// past the water's edge the bar's middle lies on the inner side (m).
/// Round 10: 85 m (round 7's 36 m stood over the bend's deep pool, so the
/// near water was an opaque sheet; this far up the bend's entry the near
/// side runs shallow and clear over a stony bed, the river bending away
/// beyond with the bar on its inside).
const UPSTREAM: f32 = 85.0;
const BAR_IN: f32 = 14.0;
/// The land round the valley block here (m; the asset's `surround`, 260 by
/// default): out past where the aerial view's haze closes, so no edge of
/// the plot shows from the air.
const SURROUND: f32 = 500.0;

/// The haze, the same from both cameras (a second body would rebuild the
/// scene's pipelines, some 45 s, on every switch): none on the sky dome; a
/// blue aerial haze that is slight close to (a twentieth at 260 m, so the
/// aerial view's near fields keep their contrast) and builds with real
/// distance (a sixth by 400 m, a third by 600 m), full at the dome so
/// the land meets its horizon. Past the dome's radius the haze fades out
/// with height (full under 60 m, none over 180 m): land there (the aerial
/// view sees it, the dome being open below the horizon) is full haze, the
/// sky from the air is clear, and from the bank the sky's lowest few
/// degrees blend softly into a hazy horizon.
const FOG: &str = "    // rivers scene: aerial haze, fading out up the sky dome\n    if (distance > 690.0) {\n        return 1.0 - smoothstep(60.0, 180.0, height);\n    }\n    let d = max(distance - 80.0, 0.0) / 760.0;\n    let f = 1.0 - exp(-d * d * exp(-max(height, 0.0) / 400.0));\n    let edge = smoothstep(600.0, 688.0, distance);\n    return clamp(max(min(f, 0.75), edge), 0.0, 1.0);";

/// The haze from the air (view 0), round 10: the same blue haze over the
/// land, but no wall at the dome's radius. From 230 m up the frame's top
/// edge looks 12 degrees down, so the dome (open below the horizon) is never
/// in it, and the land out to the far plane keeps its own haze: the ridges
/// 600 m to 1 km off stand in layers, each paler than the one before,
/// instead of one flat band of haze over the top of the frame (rounds 4 to
/// 9). Full only at the far plane, so the plot's end never shows. Round 11:
/// it starts further out and builds more slowly (the valley under the
/// camera, 250 to 450 m off, kept a grey veil: low contrast where the
/// reference is crisp), so the near valley stays clear and the haze goes to
/// the far ridges, each band bluer than the one before. Round 12: thinner
/// over the wooded slopes and ridges (0.42 at 700 m, where it was 0.49, and
/// at most 0.56 short of the far plane, where it was 0.72): the reference's
/// far ridges keep their light and shade under a blue veil, where ours went
/// to one pale wash over the top of the frame.
const FOG_AIR: &str = "    // rivers scene: aerial haze from the air, layered out to the far plane\n    let d = max(distance - 230.0, 0.0) / 740.0;\n    let f = 1.0 - exp(-d * d * 1.3 * exp(-max(height, 0.0) / 400.0));\n    let edge = smoothstep(900.0, 995.0, distance);\n    return clamp(max(min(f, 0.56), edge), 0.0, 1.0);";

/// The haze colour: a clear blue a little deeper than the dome's pale
/// horizon, as distant hills turn on a clear morning.
const HAZE: [f32; 3] = [0.4, 0.52, 0.76];
/// The dome's brightness under this scene's exposure.
const SKY_BRIGHTNESS: f32 = 0.1;
/// The sky dome's radius here (m): evani-material-proc draws it 500 m out;
/// the valley's far ridges stand 500 to 680 m from the cameras, so the dome
/// is set out behind them (at 950 m, near the camera's 1000 m far plane, it
/// drew grey discs, and so did a 1.8 km dome under a raised far plane;
/// 700 m draws clean).
const SKY_RADIUS: f32 = 700.0;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.river_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the reference's aerial overview (left panel), 1 the player's eye on the first bend's outer bank (right panel), 2 a close-up of the gravel bar"),
    )?;
    let yaw = kit.param_f32(ParamDef::f32("kit.proc_field.river_yaw", YAW).range(-180.0, 180.0).describe("The aerial view's heading round its aim (degrees from +z towards +x)"))?;
    let el = kit.param_f32(ParamDef::f32("kit.proc_field.river_el", EL).range(5.0, 89.0).describe("The aerial view's elevation (degrees down)"))?;
    let dist = kit.param_f32(ParamDef::f32("kit.proc_field.river_dist", DIST).range(20.0, 900.0).unit("m").describe("The aerial view's distance from its aim (m)"))?;
    let sun_az = kit.param_f32(ParamDef::f32("kit.proc_field.river_sun_az", 112.0).range(-180.0, 180.0).describe("The sun's bearing (degrees from +z towards +x)"))?;
    let sun_el = kit.param_f32(ParamDef::f32("kit.proc_field.river_sun_el", 38.0).range(5.0, 89.0).describe("The sun's elevation (degrees): mid-morning"))?;
    let sky_radius = kit.param_f32(ParamDef::f32("kit.proc_field.river_sky_radius", SKY_RADIUS).range(100.0, 990.0).unit("m").describe("The sky dome's radius in this scene (m): set out behind the far ridges"))?;
    let look = kit.param_f32(ParamDef::f32("kit.proc_field.river_look", -15.0).range(-90.0, 90.0).describe("The player's eye: turn from looking at the bend's gravel bar (degrees, towards the outer bank)"))?;
    let up = kit.param_f32(ParamDef::f32("kit.proc_field.river_up", UPSTREAM).range(0.0, 120.0).unit("m").describe("The player's eye: how far upstream of the bend's apex it stands on the outer bank (m)"))?;
    // The sky dome comes with the procedural materials, which need one
    // box: a small one out of sight under the valley (as the grass proof).
    let mut anchor = evani_procedural_lib::evani_material_proc::ProcBox::aabb([0.0, -40.0, 0.0], [1.0, -39.0, 1.0], evani_procedural_lib::evani_material_proc::surface::GROUND);
    anchor.bake_ao = false;
    let scene = evani_procedural_lib::evani_material_proc::Scene { boxes: vec![anchor], regions: Vec::new(), default_region: None };
    evani_procedural_lib::evani_material_proc::install(kit, scene).map_err(|e| PluginError(format!("proc_field river: {e}")))?;
    kit.system(SystemDesc::new("proc_field.river.sky_far", Stage::Frame, sky_far).order(i32::MAX - 1));
    kit.resource(River { view, yaw, el, dist, sun_az, sun_el, look, up, sky_radius, set_for: f32::NAN, eye_for: [f32::NAN; 3], grass: false, grass_wait: 0, grass_at: [f32::NAN; 3], eye: [0.0; 3], dir: [0.0, -1.0], wl: 2.0, flow: 0.0, fog_wait: 1000 });
    Ok(())
}

/// The sky dome out at [`SKY_RADIUS`] (after evani-material-proc has
/// queued it), so the far ridges stand in front of it.
fn sky_far(world: &mut World) {
    let Some(sky) = world.try_res::<LookData>().and_then(|l| l.materials.iter().position(|m| m.name == "sky")).map(|i| i as u32) else { return };
    let radius = match (world.try_res::<River>(), world.try_res::<ParamRegistry>()) {
        (Some(g), Some(reg)) => reg.f32(g.sky_radius),
        _ => SKY_RADIUS,
    };
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        for m in rw.meshes.iter_mut().filter(|m| m.material == sky) {
            m.scale = [2.0 * radius; 3];
        }
    }
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

/// A camera `dist` off at `el` degrees up, heading `yaw`, on `target`.
fn orbit(yaw: f32, el: f32, dist: f32, target: [f32; 3], fov: f32) -> CameraPose {
    let (y, e) = (yaw.to_radians(), el.to_radians());
    CameraPose {
        eye: [target[0] + dist * e.cos() * y.sin(), target[1] + dist * e.sin(), target[2] + dist * e.cos() * y.cos()],
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

/// The player's place by the bend nearest [`BEND`]: eye, looking direction
/// (plan, unit) and the current's heading there (degrees, 0 +x, 90 +z).
fn bank_eye(va: &river::Valley, look_deg: f32, upstream: f32) -> ([f32; 3], [f32; 2], f32) {
    let k = va.k;
    let want = [BEND[0] * k, BEND[1] * k];
    let line = va.line();
    // The apex of the bend nearest BEND (its cut bank: boulders at the
    // water's edge, a big oak just downstream).
    let mut best = (f32::MAX, 0usize, 1.0f32);
    for (i, outer) in va.apexes() {
        let p = line.get(i).copied().unwrap_or([0.0, 0.0]);
        let d = (p[0] - want[0]).powi(2) + (p[1] - want[1]).powi(2);
        if d < best.0 {
            best = (d, i, outer);
        }
    }
    let (i, outer) = (best.1, best.2);
    // Round 7: the player stands on the outer bank some way upstream of the
    // apex and looks down the bend at its inner gravel bar, so the frame
    // holds the near bank, the water flowing away round the bend and the
    // bar's broad crescent beyond it, as the reference's right panel does
    // (standing at the apex and turning to the river showed little bar).
    let up = (upstream * k / (2.0 * k)).round() as usize;
    let jj = i.saturating_sub(up);
    let pj = line.get(jj).copied().unwrap_or([0.0, 0.0]);
    let tj = va.tangent(jj);
    let nj = [-tj[1], tj[0]];
    let off = outer * (va.half_at(va.near(pj[0], pj[1])) + OUT * k.sqrt());
    let (x, z) = (pj[0] + nj[0] * off, pj[1] + nj[1] * off);
    let y = va.height(x, z) + EYE;
    // The bar's middle: off the apex on its inner side.
    let p = line.get(i).copied().unwrap_or([0.0, 0.0]);
    let t = va.tangent(i);
    let nrm = [-t[1], t[0]];
    let q = va.near(p[0], p[1]);
    let into = -outer * (va.half_at(q) + BAR_IN * k);
    let target = [p[0] + nrm[0] * into, p[1] + nrm[1] * into];
    let base = (target[1] - z).atan2(target[0] - x) - look_deg.to_radians() * outer;
    let dir = [base.cos(), base.sin()];
    let flow = t[1].atan2(t[0]).to_degrees().rem_euclid(360.0);
    ([x, y, z], dir, flow)
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<River>();
    let (view, yaw, el, dist, sun_az, sun_el, look, upstream) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(g.view).round().clamp(0.0, 2.0), reg.f32(g.yaw), reg.f32(g.el), reg.f32(g.dist), reg.f32(g.sun_az), reg.f32(g.sun_el), reg.f32(g.look), reg.f32(g.up))
    };
    // The valley (its line and ground) only when the view, the player's turn
    // or stand point changes, or the grass still waits for it: building it
    // is too slow for every frame.
    let key = [view, look, upstream];
    let va = if key != g.eye_for || !g.grass {
        match (world.try_res::<Registry>(), world.try_res::<ParamRegistry>()) {
            (Some(r), Some(reg)) => river::valley(&r, &reg),
            _ => None,
        }
    } else {
        None
    };
    if let Some(va) = &va {
        let (eye, dir, flow) = bank_eye(va, look, upstream);
        (g.eye, g.dir, g.wl, g.flow) = (eye, dir, va.water_level(), flow);
        g.eye_for = key;
        if (eye[0] - g.grass_at[0]).abs() + (eye[2] - g.grass_at[2]).abs() > 1.0 {
            // The player moved along the bank: lay the long grass round
            // the new stand point.
            g.grass = false;
        }
    }
    let (eye, dir) = (g.eye, g.dir);
    let cam = match view as u32 {
        1 => {
            let p = PITCH.to_radians();
            CameraPose {
                eye,
                target: [eye[0] + dir[0] * p.cos() * 50.0, eye[1] - p.sin() * 50.0, eye[2] + dir[1] * p.cos() * 50.0],
                up: [0.0, 1.0, 0.0],
                fov_y_deg: FOV,
            }
        }
        2 => {
            // Low on the near bank, looking across the water at the bar.
            let target = [eye[0] + dir[0] * 22.0, g.wl + 0.4, eye[2] + dir[1] * 22.0];
            CameraPose { eye: [eye[0], eye[1] - 0.9, eye[2]], target, up: [0.0, 1.0, 0.0], fov_y_deg: 40.0 }
        }
        _ => orbit(yaw, el, dist, AIM, FOV),
    };
    if view != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // Mid-morning sun from one side; the dome's sun follows it.
        let (e, az) = (sun_el.to_radians(), sun_az.to_radians());
        set(&mut reg, "render.light.direction", ParamValue::Vec3([e.cos() * az.sin(), e.sin(), e.cos() * az.cos()]));
        set(&mut reg, "render.light.color", ParamValue::Color([1.0, 0.94, 0.85]));
        set(&mut reg, "render.light.intensity", ParamValue::F32(3.4));
        set(&mut reg, "render.ambient", ParamValue::Color([0.33, 0.35, 0.39]));
        set(&mut reg, "render.sky.zenith", ParamValue::Color([0.55, 0.68, 1.0]));
        set(&mut reg, "render.sky.horizon", ParamValue::Color([0.9, 0.95, 1.0]));
        set(&mut reg, "render.sky.ground", ParamValue::Color([0.55, 0.6, 0.4]));
        set(&mut reg, "render.shadow_tint_strength", ParamValue::F32(0.0));
        set(&mut reg, "render.exposure", ParamValue::F32(0.62));
        set(&mut reg, "render.clear_color", ParamValue::Color(HAZE));
        set(&mut reg, "render.ao.strength", ParamValue::F32(0.8));
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(&mut reg, "look.stylizedpbr.pcss_light_deg", ParamValue::F32(0.6));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(if view == 0.0 { 520.0 } else { 160.0 }));
        set(&mut reg, "sky.follow_light", ParamValue::Bool(true));
        // From the air the land ends in the haze before the horizon: the
        // dome stays open below it, so what shows there is the haze's own
        // colour (the background), not the dome's grey underside.
        set(&mut reg, "sky.open_below", ParamValue::Bool(true));
        set(&mut reg, "procedural.water.rivers.surround", ParamValue::F32(SURROUND));
        // The sky dome asset's own photoreal recipe (column 21: a deeper,
        // less cyan blue), as a game listing both assets starts it.
        let _ = evani_procedural_lib::skydome::apply_photoreal(&mut reg);
        // Its cumulus are the recipe's cloud volume (heaps with flat grey
        // bases and sunlit crowns, lit through themselves; column 21 round
        // 2). Then this scene's exposure: the dome far dimmer than its
        // default (brighter, its blue and green clip together in the film
        // tonemap and the sky reads mint; at 0.075 the clear sky measures
        // the reference's blue).
        set(&mut reg, "sky.brightness", ParamValue::F32(SKY_BRIGHTNESS));
        // A paler, less saturated clear sky than the recipe's at this
        // exposure (round 4 measured 129/181/226 against the reference's
        // 181/205/235), and smaller, softer-based cumulus.
        set(&mut reg, "sky.tint", ParamValue::Color([0.94, 0.95, 1.0]));
        set(&mut reg, "sky.zenith_deep", ParamValue::F32(0.15));
        set(&mut reg, "sky.cloud_size", ParamValue::F32(2.0));
        set(&mut reg, "sky.cloud_soft", ParamValue::F32(0.4));
        set(&mut reg, "sky.cloud_coverage", ParamValue::F32(0.26));
        // Long grass on the banks: no carpet of its own (the turf is the
        // ground), reaching the far side of the near meadow.
        set(&mut reg, "material.grass.ground", ParamValue::Bool(false));
        set(&mut reg, "material.grass.distance", ParamValue::F32(1.5));
        let _ = river::apply_water_look(&mut reg);
        // The current runs down the reach the player stands by.
        set(&mut reg, "material.nature.water.flow_direction", ParamValue::F32(g.flow));
        drop(reg);
        // The valley stays placed: both cameras look at the same one (and
        // it is too big to make again on every switch).
        g.set_for = view;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("water-rivers", [0.0, 0.0, 0.0]);
    }
    // The water: the nature pass draws the library's rectangle.
    let water = match (world.try_res::<Registry>(), world.try_res::<ParamRegistry>()) {
        (Some(r), Some(reg)) => river::water(&r, &reg, [0.0, 0.0, 0.0], 0.0, 1.0),
        _ => None,
    };
    if let (Some(w), Some(ns)) = (water, world.try_res::<NatureSurfaces>()) {
        ns.set(&[w]);
    }
    // The long grass on the player's bank, once.
    if !g.grass
        && let Some(va) = &va
    {
        if let Some(gr) = world.try_res::<Grass>() {
            let c = [eye[0] + dir[0] * 14.0, eye[2] + dir[1] * 14.0];
            let half = 24.0;
            let (patch, n, h) = river::bank_grass(va, [c[0] - half, c[1] - half], [c[0] + half, c[1] + half], [0.0, 0.0, 0.0]);
            gr.clear_patches();
            gr.add_patch_on(patch, n, &h);
            g.grass = true;
            g.grass_at = eye;
        } else {
            // No grass module in this build: stop asking after a while.
            g.grass_wait += 1;
            g.grass = g.grass_wait > 120;
        }
    }
    // The haze: the scene's body for this view, put back if the look (or the
    // other view) rewrote it.
    let fog = if view == 0.0 { FOG_AIR } else { FOG };
    g.fog_wait = g.fog_wait.saturating_add(1);
    if g.fog_wait > 20 {
        let differs = world.try_res::<ShaderSlots>().is_some_and(|s| s.id("fog").and_then(|id| s.get(id)).is_some_and(|d| d.body != fog));
        if differs {
            if let (Some(mut slots), Some(mut rb)) = (world.try_res_mut::<ShaderSlots>(), world.try_res_mut::<SlotRebuilds>()) {
                if let Some(id) = slots.id("fog") {
                    let _ = slots.set_body(id, fog);
                }
                let _ = rb.request_detached(&slots, "fog", fog, 0.0);
            }
            g.fog_wait = 0;
        }
    }
    *world.res_mut::<River>() = g;
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
}
