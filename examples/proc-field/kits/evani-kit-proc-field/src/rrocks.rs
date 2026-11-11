//! The rounded river rocks' live scene (feature `rrocks`, the Asset Studio
//! rounded river rocks session, column 25, 2026-11-07): the procedural
//! library's river rocks (`rocks-rounded-river-rocks`) in the engine's
//! realism studio under their reference picture's camera and light (a
//! studio group of five water-polished stones of different rock, a
//! three-quarter view from above with a long lens, a soft key light from the
//! upper left, soft shadows going right, a neutral grey sweep behind), so
//! they can be tuned and shot beside the reference. Adapted from the medium
//! rocks' scene (src/mrocks.rs, column 23), whose floor pass it shares.
//!
//! `kit.proc_field.rrocks_view`: 0 the reference's view, 1 a close-up of
//! the big veined stone and the sandstone (the grain, the veins, the worn
//! dents), 2 low along the ground (where the stones meet the floor).

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{F32Param, ParamDef, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{LookData, PluginContext, PluginError};
use evani_procedural::ProcScene;
use evani_render::{MeshId, MeshInstance, RenderWorld};

#[derive(Debug, Clone, Copy)]
pub struct RiverRocks {
    view: F32Param,
    yaw: F32Param,
    aim: F32Param,
    el: F32Param,
    dist: F32Param,
    fov: F32Param,
    sun: F32Param,
    floor: Vec3Param,
    ao: F32Param,
    ao_r: F32Param,
    ground: F32Param,
    soft: F32Param,
    contact: F32Param,
    noff: F32Param,
    sbox: F32Param,
    sun_el: F32Param,
    wrap: F32Param,
    /// The view the scene was last set up for (NaN: not yet).
    set_for: f32,
}

/// The floor's colour by default (drawn at twice it): a neutral studio grey.
/// Round 2: cooler (round 1's review: the floor R-B 5.5 against 0.7).
const FLOOR: [f32; 3] = [0.33, 0.336, 0.376];

/// The reference view: heading (degrees from +z towards +x), aim height
/// (m), elevation (degrees), distance (m), vertical field of view
/// (degrees) and the key light's heading off the camera's (degrees).
const YAW: f32 = 0.0;
const AIM: f32 = 0.16;
const EL: f32 = 20.0;
const DIST: f32 = 3.05;
const FOV: f32 = 14.6;
const SUN: f32 = -62.0;
/// Round 2 (round 1's review: soft, detached contact shadows with broad
/// banded halos): no screen-space occlusion (its floor drew stepped rings
/// round every stone at any reach and strength above 0, isolated captures
/// at 0.1, 0.15 and 0.3 m; none with it off: column 23's finding too), and
/// far less light bounced up from the floor, so each stone's underside where
/// it curves in to the floor falls into a dark core, as the reference's.
const AO: f32 = 0.0;
const AO_R: f32 = 0.15;
const GROUND: f32 = 0.12;
/// The key light's size for the soft shadows (degrees).
const SOFT: f32 = 8.0;
/// Round 2: tight soft contact discs under each stone (the reference's dark
/// line where every stone meets the floor), a row along its length, each a
/// quarter wider than the stone so it fades out within a few centimetres of
/// its edge (column 23's, 1.3 times wider and lighter, read as floating
/// pads), sunk a little so they darken only the floor and the stone's lowest
/// upward faces.
const CONTACT: f32 = 0.9;
/// Round 3 (round 2's review: the stones look suspended over their
/// shadows): the shadow test's normal offset is in metres, and at the
/// engine's 6 cm the floor was tested 6 cm above itself, so each stone's
/// shadow began a hand's width from its base (a lit gap under every stone in
/// the low view). A small offset keeps the shadow at the contact.
const NOFF: f32 = 0.008;
const SBOX: f32 = 22.0;
/// The key light's elevation (degrees) and how far its light wraps past
/// the terminator. Round 3: 46 degrees and 0.7 lit the stones' downward
/// faces, so each bottom edge read as a bright rim over its contact shadow
/// (the review's "suspended"); a higher key wrapping less leaves each
/// stone's lower front in its own shade down to the floor, as the
/// reference's.
const SUN_EL: f32 = 60.0;
const WRAP: f32 = 0.3;
const CONTACT_SINK: f32 = 0.08;
const CONTACT_WIDE: f32 = 1.25;

pub fn build(kit: &mut PluginContext) -> Result<(), PluginError> {
    let view = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_view", 0.0)
            .range(0.0, 2.0)
            .step(1.0)
            .describe("0 the river rocks' reference view, 1 a close-up of the big veined stone and the sandstone, 2 low along the ground"),
    )?;
    let yaw = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_yaw", YAW)
            .range(-180.0, 180.0)
            .describe("The camera's heading round the group (degrees from +z towards +x)"),
    )?;
    let aim = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_aim", AIM)
            .range(-0.3, 0.6)
            .unit("m")
            .describe("The reference view's aim: the height the camera looks at (m)"),
    )?;
    let el = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_el", EL)
            .range(0.0, 80.0)
            .describe("The reference view's elevation (degrees above the floor)"),
    )?;
    let dist = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_dist", DIST)
            .range(0.5, 20.0)
            .unit("m")
            .describe("The reference view's distance from its aim (m)"),
    )?;
    let fov = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_fov", FOV)
            .range(5.0, 60.0)
            .describe("The reference view's vertical field of view (degrees)"),
    )?;
    let sun = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_sun", SUN)
            .range(-180.0, 180.0)
            .describe("The key light's heading off the camera's (degrees; negative from the left)"),
    )?;
    let floor = kit.param_vec3(
        ParamDef::color("kit.proc_field.rrocks_floor", FLOOR)
            .describe("The studio floor's colour under the stones"),
    )?;
    let ao = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_ao", AO)
            .range(0.0, 1.0)
            .describe("How dark the crevice is where each stone meets the floor (ambient occlusion strength)"),
    )?;
    let ao_r = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_ao_r", AO_R)
            .range(0.1, 1.0)
            .unit("m")
            .describe("How far the contact darkening reaches (ambient occlusion radius, m)"),
    )?;
    let ground = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_ground", GROUND)
            .range(0.0, 1.0)
            .describe("Light bounced up from the floor onto the stones' undersides"),
    )?;
    let soft = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_soft", SOFT)
            .range(1.0, 20.0)
            .describe("The key light's size: how soft the stones' shadows are (degrees)"),
    )?;
    let contact = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_contact", CONTACT)
            .range(0.0, 1.0)
            .describe("How dark the floor is right at each stone's foot (soft contact discs; 0: none)"),
    )?;
    let noff = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_noff", NOFF)
            .range(0.0, 0.1)
            .unit("m")
            .describe("The shadow test's normal offset (m): larger lifts the shadows off the stones' bases")
    )?;
    let sbox = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_sbox", SBOX)
            .range(4.0, 40.0)
            .unit("m")
            .describe("The shadow map's half width (m): smaller is sharper")
    )?;
    let sun_el = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_sun_el", SUN_EL)
            .range(10.0, 85.0)
            .describe("The key light's elevation (degrees)"),
    )?;
    let wrap = kit.param_f32(
        ParamDef::f32("kit.proc_field.rrocks_wrap", WRAP)
            .range(0.0, 1.0)
            .describe("How far the key light wraps past the terminator onto the stones' undersides"),
    )?;
    kit.resource(RiverRocks {
        view,
        yaw,
        aim,
        el,
        dist,
        fov,
        sun,
        floor,
        ao,
        ao_r,
        ground,
        soft,
        contact,
        noff,
        sbox,
        sun_el,
        wrap,
        set_for: f32::NAN,
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

/// A camera `dist` off at `el` degrees up, heading `yaw`, on `target`.
fn orbit(yaw: f32, el: f32, dist: f32, target: [f32; 3], fov: f32) -> CameraPose {
    let (y, e) = (yaw.to_radians(), el.to_radians());
    CameraPose {
        eye: [
            target[0] + dist * e.cos() * y.sin(),
            target[1] + dist * e.sin(),
            target[2] + dist * e.cos() * y.cos(),
        ],
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: fov,
    }
}

pub fn draw(world: &mut World) {
    let mut g = *world.res::<RiverRocks>();
    let (view, yaw, aim, el, dist, fov, sun, ao, ao_r, ground, soft, noff, sbox, sun_el, wrap) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32(g.view).round().clamp(0.0, 2.0),
            reg.f32(g.yaw),
            reg.f32(g.aim),
            reg.f32(g.el),
            reg.f32(g.dist),
            reg.f32(g.fov),
            reg.f32(g.sun),
            reg.f32(g.ao),
            reg.f32(g.ao_r),
            reg.f32(g.ground),
            reg.f32(g.soft),
            reg.f32(g.noff),
            reg.f32(g.sbox),
            reg.f32(g.sun_el),
            reg.f32(g.wrap),
        )
    };
    let cam = match view as u32 {
        1 => orbit(yaw + 12.0, 30.0, 1.5, [0.08, 0.13, -0.14], 18.0),
        2 => orbit(yaw + 6.0, 5.0, 2.8, [0.0, 0.08, 0.0], 16.0),
        _ => orbit(yaw, el, dist, [0.0, aim, 0.0], fov),
    };
    // Set up each time the view, or the light's heading, changes.
    let key = view * 1000.0 + sun + dist * 0.001 + ao * 7.1 + ao_r * 13.3 + ground * 17.7 + soft * 0.37 + noff * 91.0 + sbox * 0.011 + sun_el * 0.0031 + wrap * 23.9;
    if key != g.set_for {
        let mut reg = world.res_mut::<ParamRegistry>();
        let _ = evani_procedural::studio::apply(&mut reg);
        set(&mut reg, "aq.enabled", ParamValue::Bool(false));
        // A soft key from the camera's upper left (the reference's shadows
        // fall right and a little back), a broad neutral fill.
        let (el, az) = (sun_el.to_radians(), (yaw + sun).to_radians());
        set(
            &mut reg,
            "render.light.direction",
            ParamValue::Vec3([el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]),
        );
        set(
            &mut reg,
            "render.light.color",
            ParamValue::Color([1.0, 1.0, 1.0]),
        );
        set(&mut reg, "render.light.intensity", ParamValue::F32(1.9));
        set(
            &mut reg,
            "render.ambient",
            ParamValue::Color([1.02, 1.03, 1.05]),
        );
        set(&mut reg, "render.ao.strength", ParamValue::F32(ao));
        set(&mut reg, "render.ao.radius", ParamValue::F32(ao_r));
        set(
            &mut reg,
            "render.sky.horizon",
            ParamValue::Color([0.8, 0.8, 0.8]),
        );
        set(
            &mut reg,
            "render.sky.ground",
            ParamValue::Color([ground, ground, ground]),
        );
        set(
            &mut reg,
            "render.shadow_tint_strength",
            ParamValue::F32(0.0),
        );
        // Round 3: less of the pale floor mirrored in the stones' undersides.
        set(&mut reg, "look.stylizedpbr.env_spec", ParamValue::F32(0.12));
        set(&mut reg, "look.stylizedpbr.fill", ParamValue::F32(0.28));
        set(&mut reg, "render.ibl.strength", ParamValue::F32(0.6));
        set(&mut reg, "render.exposure", ParamValue::F32(0.5));
        set(
            &mut reg,
            "render.clear_color",
            ParamValue::Color([0.69, 0.69, 0.695]),
        );
        // The seamless sweep: the floor fades into the backdrop behind.
        set(&mut reg, "look.stylizedpbr.haze_near", ParamValue::F32(dist + 0.25));
        set(&mut reg, "look.stylizedpbr.haze_far", ParamValue::F32(dist + 1.95));
        // A wide shadow box (column 23's medium rocks, round 3): a shadow
        // texel about 17 mm, so the soft shadow's fixed taps land about a
        // texel apart and blend; in a small box they drew stacked copies of
        // each stone's outline in the penumbra.
        // Round 2 (round 1's review: a flat backdrop, SD 0.2 against 3.4):
        // a soft vignette grades the sweep towards the frame's corners
        // (column 24's jagged formation scene).
        set(&mut reg, "fx.vignette.intensity", ParamValue::F32(0.14));
        set(&mut reg, "fx.vignette.radius", ParamValue::F32(0.6));
        set(&mut reg, "fx.vignette.softness", ParamValue::F32(0.9));
        set(&mut reg, "fx.vignette.roundness", ParamValue::F32(0.6));
        set(&mut reg, "fx.shadows.distance", ParamValue::F32(sbox));
        set(&mut reg, "fx.shadows.bias", ParamValue::F32(0.0001));
        set(&mut reg, "fx.shadows.normal_offset", ParamValue::F32(noff));
        // A broad soft studio light: a wide sun, a wide blocker search, a
        // wrapped terminator and a strong even ambient.
        set(
            &mut reg,
            "look.stylizedpbr.pcss_light_deg",
            ParamValue::F32(soft),
        );
        set(
            &mut reg,
            "look.stylizedpbr.pcss_search",
            ParamValue::F32(24.0),
        );
        set(&mut reg, "look.stylizedpbr.wrap", ParamValue::F32(wrap));
        set(&mut reg, "look.stylizedpbr.rim", ParamValue::F32(0.0));
        set(
            &mut reg,
            "kit.proc_field.facet_floor.on",
            ParamValue::Bool(true),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.broad",
            ParamValue::F32(0.0),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.fine",
            ParamValue::F32(0.04),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.contact",
            ParamValue::F32(0.0),
        );
        set(
            &mut reg,
            "kit.proc_field.facet_floor.far",
            ParamValue::F32(0.0),
        );
        drop(reg);
        if g.set_for.is_nan()
            && let Some(mut s) = world.try_res_mut::<ProcScene>()
        {
            s.clear();
        }
        g.set_for = key;
        *world.res_mut::<RiverRocks>() = g;
    }
    if let Some(mut s) = world.try_res_mut::<ProcScene>()
        && s.placements.is_empty()
    {
        s.place("rocks-rounded-river-rocks", [0.0, 0.0, 0.0]);
    }
    let (rgb, contact) = {
        let reg = world.res::<ParamRegistry>();
        (reg.vec3(g.floor), reg.f32(g.contact))
    };
    let floor = RenderWorld::material(&world.res::<LookData>(), "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = cam;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.002, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, 1.0];
    rw.draw_mesh(f);
    if contact > 0.0 {
        rw.blob_strength = contact;
        for (c, half_l, half_w, yaw) in evani_procedural_lib::river_rocks::footprints(0.4) {
            let ax = [yaw.cos(), -yaw.sin()];
            let n = ((half_l / half_w).round() as i32).max(1);
            for i in 0..=n {
                let t = (i as f32 / n as f32 - 0.5) * 2.0 * (half_l - 0.9 * half_w).max(0.0);
                rw.draw_blob(
                    [c[0] + ax[0] * t, -CONTACT_SINK, c[1] + ax[1] * t],
                    half_w * CONTACT_WIDE,
                );
            }
        }
    }
}
