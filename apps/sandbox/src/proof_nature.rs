//! The nature materials proof (sandbox feature `nature-proof`; never in
//! evani.toml or `default`, so the default and ship builds are unchanged):
//! a small garden east of the spinner grid with every evani-material-nature
//! kind: a raised pool of water, a window frame with its pane, a bottle on a
//! table, two bushes, a grass patch, a leaf on a stem, a little head with
//! two thin ears and a lamp, with the sun low behind them so the back-light
//! shows. The solid parts are the engine's primitive meshes; the water,
//! glass and glow are the nature pass. The camera is held (from
//! [`CLOSE_AT`] seconds it moves in over the pool and the bottle), so
//! captures see the same views every run.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::{LookData, PluginContext};
use evani_core::time::Time;
use evani_material_nature::{Kind, NatureSurfaces, Surface};
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The garden's centre on the ground (east of the grid, inside the terraces).
pub const AT: [f32; 3] = [28.0, 0.0, 0.0];

/// Towards the sun: low and behind the garden as the camera sees it.
pub const SUN: [f32; 3] = [0.25, 0.32, -1.0];

/// The held camera: south of the garden looking north into the low sun.
pub const CAMERA: CameraPose = CameraPose {
    eye: [AT[0] + 0.2, 1.75, AT[2] + 6.6],
    target: [AT[0] - 0.1, 0.85, AT[2] - 1.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 55.0,
};
/// From [`CLOSE_AT`]: close over the pool and the bottle.
pub const CLOSE: CameraPose = CameraPose {
    eye: [AT[0] - 0.4, 1.55, AT[2] + 3.0],
    target: [AT[0] - 1.3, 0.5, AT[2] - 0.2],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 50.0,
};
/// When the camera moves in (sim seconds).
pub const CLOSE_AT: f64 = 8.0;

fn at(p: [f32; 3]) -> [f32; 3] {
    [AT[0] + p[0], AT[1] + p[1], AT[2] + p[2]]
}

const POOL: [f32; 3] = [-1.8, 0.0, 0.2];
const POOL_HALF: [f32; 2] = [1.2, 0.8];
const POOL_WALL: f32 = 0.15;
const POOL_TOP: f32 = 0.55;
const WATER: f32 = 0.45;
const TABLE: [f32; 3] = [0.5, 0.0, 1.3];
const TABLE_TOP: f32 = 0.72;
const WINDOW: [f32; 3] = [1.7, 0.0, -1.3];
const HEAD: [f32; 3] = [2.7, 1.25, 0.9];
const LAMP: [f32; 3] = [-3.6, 1.55, 1.2];
const SIGN: [f32; 3] = [-3.4, 1.15, -0.9];

/// Every nature surface of the garden.
pub fn surfaces() -> Vec<Surface> {
    let mut out = vec![
        Surface::new(Kind::WaterRect, at([POOL[0], WATER, POOL[2]]), [POOL_HALF[0], 0.0, POOL_HALF[1]]),
        Surface::new(Kind::GlassPane, at([WINDOW[0], 1.3, WINDOW[2]]), [0.55, 0.5, 0.012]).with_yaw(-0.25),
        Surface::new(Kind::GlassBottle, at([TABLE[0], TABLE_TOP + 0.24, TABLE[2]]), [0.085, 0.155, 0.085]),
        Surface::new(Kind::Foliage, at([-1.0, 0.75, -2.6]), [1.1, 0.8, 0.8]),
        Surface::new(Kind::Foliage, at([3.4, 0.55, -0.6]), [0.65, 0.55, 0.6]),
        Surface::new(Kind::Grass, at([1.2, 0.14, 2.6]), [1.3, 0.14, 0.9]),
        Surface::new(Kind::Leaf, at([0.2, 1.35, -0.4]), [0.2, 0.13, 0.004]).with_yaw(0.3),
        Surface::new(Kind::Lamp, at(LAMP), [0.26, 0.2, 0.26]),
        Surface::new(Kind::ThinPatch, at(SIGN), [0.2, 0.14, 0.004]).with_yaw(0.35),
    ];
    for side in [-1.0f32, 1.0] {
        out.push(Surface::new(Kind::ThinBlob, at([HEAD[0] + side * 0.14, HEAD[1] + 0.26, HEAD[2]]), [0.07, 0.12, 0.025]));
    }
    out
}

struct Mats {
    accent: u32,
    ground: u32,
    glow: u32,
}

fn mats(world: &World) -> Mats {
    let look = world.res::<LookData>();
    Mats {
        accent: RenderWorld::material(&look, "accent"),
        ground: RenderWorld::material(&look, "ground"),
        glow: RenderWorld::material(&look, "glow"),
    }
}

/// A solid shape centred at `c` (relative to [`AT`]) with full size `size`.
fn solid(mesh: MeshId, c: [f32; 3], size: [f32; 3], yaw: f32, material: u32, color: [f32; 3]) -> MeshInstance {
    let mut m = MeshInstance::new(mesh, at(c));
    m.scale = size;
    m.rotation = quat_axis_angle([0.0, 1.0, 0.0], yaw);
    m.material = material;
    m.color = [color[0], color[1], color[2], 1.0];
    m
}

/// The solid parts: what the glow lights and what shows through the water
/// and the glass.
fn solids(m: &Mats) -> Vec<MeshInstance> {
    let stone = [0.55, 0.52, 0.48];
    let wood = [0.42, 0.27, 0.15];
    let leaf = [0.12, 0.32, 0.08];
    let mut out = Vec::new();
    // The pool: a bed of pale tiles and four low walls round the water.
    let (hx, hz, w, top) = (POOL_HALF[0], POOL_HALF[1], POOL_WALL, POOL_TOP);
    out.push(solid(MeshId::CUBE, [POOL[0], 0.01, POOL[2]], [hx * 2.0, 0.02, hz * 2.0], 0.0, m.ground, [0.55, 0.62, 0.6]));
    for (c, s) in [
        ([POOL[0], top * 0.5, POOL[2] - hz - w * 0.5], [hx * 2.0 + w * 2.0, top, w]),
        ([POOL[0], top * 0.5, POOL[2] + hz + w * 0.5], [hx * 2.0 + w * 2.0, top, w]),
        ([POOL[0] - hx - w * 0.5, top * 0.5, POOL[2]], [w, top, hz * 2.0]),
        ([POOL[0] + hx + w * 0.5, top * 0.5, POOL[2]], [w, top, hz * 2.0]),
    ] {
        out.push(solid(MeshId::BEVEL_CUBE, c, s, 0.0, m.accent, stone));
    }
    // A pebble under the water, so the depth fade has something to show.
    out.push(solid(MeshId::LOW_SPHERE, [POOL[0] + 0.4, 0.08, POOL[2] - 0.2], [0.5, 0.2, 0.4], 0.4, m.accent, [0.35, 0.3, 0.25]));
    // The window: a frame on two posts, the pane is the nature pass.
    let wy: f32 = -0.25;
    let (fw, fh, bar) = (1.1, 1.0, 0.07);
    let side = |dx: f32| {
        let (s, c) = wy.sin_cos();
        [WINDOW[0] + c * dx, 0.0, WINDOW[2] - s * dx]
    };
    for dx in [-fw * 0.5 - bar * 0.5, fw * 0.5 + bar * 0.5] {
        let p = side(dx);
        out.push(solid(MeshId::CUBE, [p[0], 0.9, p[2]], [bar, 1.8, bar], wy, m.accent, wood));
    }
    for y in [1.3 - fh * 0.5 - bar * 0.5, 1.3 + fh * 0.5 + bar * 0.5] {
        out.push(solid(MeshId::CUBE, [WINDOW[0], y, WINDOW[2]], [fw + bar * 2.0, bar, bar], wy, m.accent, wood));
    }
    // The table the bottle stands on.
    out.push(solid(MeshId::CUBE, [TABLE[0], TABLE_TOP - 0.03, TABLE[2]], [0.7, 0.06, 0.5], 0.0, m.accent, wood));
    out.push(solid(MeshId::CYLINDER, [TABLE[0], (TABLE_TOP - 0.06) * 0.5, TABLE[2]], [0.08, TABLE_TOP - 0.06, 0.08], 0.0, m.accent, wood));
    // Bushes (the crowns the foliage kind lights).
    out.push(solid(MeshId::SPHERE, [-1.0, 0.75, -2.6], [2.2, 1.6, 1.6], 0.0, m.accent, leaf));
    out.push(solid(MeshId::SPHERE, [3.4, 0.55, -0.6], [1.3, 1.1, 1.2], 0.0, m.accent, leaf));
    // The grass patch's soil.
    out.push(solid(MeshId::CUBE, [1.2, 0.012, 2.6], [2.6, 0.024, 1.8], 0.0, m.ground, [0.2, 0.32, 0.1]));
    // A leaf on a stem.
    out.push(solid(MeshId::CYLINDER, [0.2, 0.62, -0.4], [0.025, 1.24, 0.025], 0.0, m.accent, wood));
    out.push(solid(MeshId::CUBE, [0.2, 1.35, -0.4], [0.4, 0.26, 0.008], 0.3, m.accent, [0.2, 0.45, 0.1]));
    // A little head on a post, with two thin ears.
    out.push(solid(MeshId::CYLINDER, [HEAD[0], (HEAD[1] - 0.2) * 0.5, HEAD[2]], [0.06, HEAD[1] - 0.2, 0.06], 0.0, m.accent, wood));
    out.push(solid(MeshId::SPHERE, HEAD, [0.44, 0.4, 0.4], 0.0, m.accent, [0.8, 0.6, 0.48]));
    for side in [-1.0f32, 1.0] {
        out.push(solid(MeshId::SPHERE, [HEAD[0] + side * 0.14, HEAD[1] + 0.26, HEAD[2]], [0.14, 0.24, 0.05], 0.0, m.accent, [0.8, 0.58, 0.46]));
    }
    // A paper sign on a post (the thin colour glows through it).
    out.push(solid(MeshId::CYLINDER, [SIGN[0], (SIGN[1] - 0.14) * 0.5, SIGN[2] - 0.01], [0.03, SIGN[1] - 0.14, 0.03], 0.0, m.accent, wood));
    out.push(solid(MeshId::CUBE, SIGN, [0.4, 0.28, 0.008], 0.35, m.accent, [0.86, 0.82, 0.7]));
    // A lamp: a post and a shade (the nature pass lights it from inside).
    out.push(solid(MeshId::CYLINDER, [LAMP[0], (LAMP[1] - 0.2) * 0.5, LAMP[2]], [0.05, LAMP[1] - 0.2, 0.05], 0.0, m.accent, [0.2, 0.2, 0.22]));
    out.push(solid(MeshId::SPHERE, LAMP, [0.52, 0.4, 0.52], 0.0, m.accent, [0.85, 0.78, 0.62]));
    // The finial on top glows (the look's glow material).
    out.push(solid(MeshId::SPHERE, [LAMP[0], LAMP[1] + 0.22, LAMP[2]], [0.06, 0.06, 0.06], 0.0, m.glow, [1.0, 0.85, 0.6]));
    out
}

/// Hold the camera and the sun, hand the surfaces to the nature pass once,
/// and draw the solid parts.
pub fn install(ctx: &mut PluginContext) {
    ctx.system(SystemDesc::new("sandbox.proof_nature", Stage::Frame, frame));
}

fn frame(world: &mut World) {
    let t = world.try_res::<Time>().map_or(0.0, |t| t.sim_seconds);
    if let Some(n) = world.try_res::<NatureSurfaces>()
        && n.is_empty()
    {
        n.set(&surfaces());
        // The sun, once (live tuning may move it afterwards).
        let mut reg = world.res_mut::<ParamRegistry>();
        if let Some(id) = reg.id("render.light.direction") {
            let _ = reg.set(id, ParamValue::Vec3(SUN));
        }
    }
    let m = mats(world);
    let list = solids(&m);
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = if t < CLOSE_AT { CAMERA } else { CLOSE };
        #[cfg(feature = "nature-orbit")]
        {
            rw.camera = orbit(t);
        }
        for s in list {
            rw.draw_mesh(s);
        }
    }
}

/// `nature-orbit` (frame order proof): the camera circles the pool fast,
/// 3 rad per sim second, so a value a module copies a frame late shows as
/// water drawn from the wrong place. Sim time, so a frame stepped with no
/// time passing is the same view at rest.
#[cfg(feature = "nature-orbit")]
fn orbit(t: f64) -> CameraPose {
    let a = (t * 3.0) as f32;
    let c = at(POOL);
    CameraPose {
        eye: [c[0] + 4.5 * a.sin(), 2.4, c[2] + 4.5 * a.cos()],
        target: [c[0], WATER, c[2]],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: 55.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_garden_hands_every_kind_to_the_nature_pass() {
        let mut app = crate::build_app();
        app.run_headless(3, 1000.0 / 60.0);
        let n = app.world.res::<NatureSurfaces>().clone();
        assert_eq!(n.len(), surfaces().len());
        for k in Kind::ALL {
            if k == Kind::WaterEllipse {
                continue;
            }
            assert!(surfaces().iter().any(|s| s.kind == k), "no {k:?}");
        }
        assert!(n.drawn() > 0, "the held camera sees the garden");
        let reg = app.world.res::<ParamRegistry>();
        assert!(reg.id("material.nature.water.clarity").is_some());
        assert_eq!(app.world.res::<evani_core::overlay::ErrorLog>().error_count(), 0);
    }
}
