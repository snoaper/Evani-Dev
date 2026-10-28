//! The playground surfaces proof (sandbox feature `playmat-proof`; never in
//! evani.toml or `default`, so the default and ship builds are unchanged):
//! a small yard west of the spinner grid with a brick wall and pillar, cobble
//! and glazed tile floors, a plank deck and bench, a tiled plinth and a free
//! stone ball, drawn with evani-material-proc's relief surfaces (feature
//! `relief`: baked height, bent normal and roughness through the
//! kit-surface hook) under the photoreal look's low daylight sun and the
//! Preetham sky dome.
//!
//! The page address picks what is shown, so every capture is the same:
//!
//! * `pm=before`: the same yard in the plain surfaces the engine had before
//!   (brick paving, flagstones, concrete and timber trim; the ball grey),
//!   with no relief installed.
//! * `pm=wet`, `pm=moss`, `pm=flat` (bent normal off), anything else: the
//!   relief, dry.
//! * `cam=wide|wall|floor|deck`: the held camera (default `wide`).
//!
//! The views look across the sun (its azimuth is 56 degrees, the cameras
//! look along about 150), so the relief is raked from the side.
//!
//! Don't combine it with `material-proof`: both install evani-material-proc.

#[cfg(feature = "material-proof")]
compile_error!("`playmat-proof` and `material-proof` each install evani-material-proc; turn on one");

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::plugin::{LookData, PluginContext};
use evani_material_proc::draw::{white, world_slot};
use evani_material_proc::relief::{self, Layers, ReliefConfig};
use evani_material_proc::{ProcBox, Scene, V3, surface};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The yard's centre on the ground (west of the grid, inside the terraces).
pub const AT: V3 = [-28.0, 0.0, 0.0];
/// The floor's top: just above the kit's ground plane (y = 0).
pub const FLOOR_Y: f32 = 0.02;
/// The wall height (m).
pub const WALL_H: f32 = 3.0;
/// The stone ball (centre relative to [`AT`], radius), sunk a little into
/// the tiles.
pub const BALL: (V3, f32) = ([2.2, FLOOR_Y + 0.6, 0.4], 0.7);

/// What the page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Before,
    Dry,
    Wet,
    Moss,
    Flat,
}

/// The held camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cam {
    Wide,
    Wall,
    Floor,
    Deck,
    /// Low over the tiles, looking towards the sun (az 56 deg): the glaze
    /// and the puddles catch it.
    Glint,
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

pub fn mode_of(url: Option<&str>) -> Mode {
    match url.and_then(|u| query_value(u, "pm")) {
        Some("before") => Mode::Before,
        Some("wet") => Mode::Wet,
        Some("moss") => Mode::Moss,
        Some("flat") => Mode::Flat,
        _ => Mode::Dry,
    }
}

/// `?dbg=1` puddle mask, `2` moss mask, `3` roughness (relief-debug view).
pub fn dbg_of(url: Option<&str>) -> u32 {
    url.and_then(|u| query_value(u, "dbg")).and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub fn cam_of(url: Option<&str>) -> Cam {
    match url.and_then(|u| query_value(u, "cam")) {
        Some("wall") => Cam::Wall,
        Some("floor") => Cam::Floor,
        Some("deck") => Cam::Deck,
        Some("glint") => Cam::Glint,
        _ => Cam::Wide,
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

fn pose(eye: V3, target: V3, fov_y_deg: f32) -> CameraPose {
    CameraPose {
        eye: [AT[0] + eye[0], AT[1] + eye[1], AT[2] + eye[2]],
        target: [AT[0] + target[0], AT[1] + target[1], AT[2] + target[2]],
        up: [0.0, 1.0, 0.0],
        fov_y_deg,
    }
}

/// The camera for a view.
pub fn camera(c: Cam) -> CameraPose {
    match c {
        // From the south-west corner over both floors to the wall.
        Cam::Wide => pose([-3.6, 1.7, 5.6], [1.4, 0.7, -3.2], 60.0),
        // 2.5 m from the wall, where the cobbles meet the tiles.
        Cam::Wall => pose([-0.9, 1.25, -2.4], [0.9, 0.75, -5.0], 55.0),
        // The ball on the tiles, the cobbles beyond.
        Cam::Floor => pose([0.6, 1.15, 3.1], [2.1, 0.25, 0.3], 55.0),
        // The deck and the cobbles from a knee's height.
        Cam::Deck => pose([-3.4, 0.75, 8.2], [-1.2, 0.05, 4.6], 55.0),
        Cam::Glint => pose([0.5, 0.55, 0.5], [4.7, 0.05, 3.3], 55.0),
    }
}

/// An axis-aligned box from corners relative to [`AT`].
fn b(min: V3, max: V3, s: u32) -> ProcBox {
    ProcBox::aabb(
        [AT[0] + min[0], AT[1] + min[1], AT[2] + min[2]],
        [AT[0] + max[0], AT[1] + max[1], AT[2] + max[2]],
        s,
    )
}

/// A relief surface, or the plain surface the engine had before.
fn pick(relief_id: u32, before: bool) -> u32 {
    if !before {
        return relief_id;
    }
    match relief_id {
        relief::surface::BRICK => surface::BRICK_PAVING,
        relief::surface::COBBLE => surface::PAVING,
        relief::surface::TILES => surface::CONCRETE,
        _ => surface::TRIM_WOOD,
    }
}

/// The yard (10 x 13 m: a wall on its north side, a deck on its south).
pub fn scene(before: bool) -> Scene {
    use relief::surface::{BRICK, COBBLE, PLANKS, TILES};
    let (f, h) = (FLOOR_Y, WALL_H);
    let s = |id| pick(id, before);
    let mut boxes = Vec::new();
    // Floors (world patterns, so the halves join without seams): cobbles
    // west, tiles east, and a plank deck on the south side.
    for (min, max, id) in [
        ([-5.0, f - 0.3, -5.0], [0.0, f, 5.0], COBBLE),
        ([0.0, f - 0.3, -5.0], [5.0, f, 5.0], TILES),
        ([-5.0, f - 0.3, 5.0], [5.0, f + 0.12, 8.0], PLANKS),
    ] {
        let mut fl = b(min, max, s(id));
        fl.world_pattern = true;
        boxes.push(fl);
    }
    // The north wall and a pillar in brick.
    let mut wall = b([-5.5, f, -5.5], [5.0, h, -5.0], s(BRICK));
    wall.world_pattern = true;
    boxes.push(wall);
    boxes.push(b([-1.5, f, 0.9], [-0.9, f + 1.8, 1.5], s(BRICK)));
    // A tiled plinth against the wall and a plank bench on the cobbles.
    boxes.push(b([1.0, f, -4.9], [4.0, f + 0.5, -3.9], s(TILES)));
    boxes.push(b([-3.6, f, -1.2], [-2.0, f + 0.45, -0.6], s(PLANKS)));
    Scene { boxes, regions: Vec::new(), default_region: None }
}

/// The relief config for a mode (the ball is cobble stone, triplanar).
pub fn config(mode: Mode) -> ReliefConfig {
    let mut layers = Layers::default();
    let mut height_strength = 1.0;
    match mode {
        Mode::Wet => {
            layers.wetness = 0.75;
            layers.puddles = 0.55;
        }
        Mode::Moss => layers.moss = 0.6,
        Mode::Flat => height_strength = 0.0,
        Mode::Before | Mode::Dry => {}
    }
    ReliefConfig {
        layers,
        height_strength,
        free_surface: Some(relief::surface::COBBLE),
        ..ReliefConfig::default()
    }
}

/// The proof's state.
#[derive(Debug, Clone, Copy)]
pub struct ProofPlaymat {
    pub mode: Mode,
    pub cam: Cam,
}

/// Bake the yard and its relief, and add the camera and ball systems.
pub fn install(ctx: &mut PluginContext) {
    let url = page_url();
    let st = ProofPlaymat { mode: mode_of(url.as_deref()), cam: cam_of(url.as_deref()) };
    ctx.resource(st);
    ctx.system(SystemDesc::new("sandbox.proof_playmat.camera", Stage::Frame, hold_camera));
    let before = st.mode == Mode::Before;
    if let Err(e) = evani_material_proc::install(ctx, scene(before)) {
        ctx.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("sandbox: playmat proof: {e}"));
        return;
    }
    ctx.system(SystemDesc::new("sandbox.proof_playmat.ball", Stage::Frame, draw_ball));
    if !before && let Err(e) = relief::install(ctx, ReliefConfig { debug: dbg_of(url.as_deref()), ..config(st.mode) }) {
        ctx.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("sandbox: playmat relief: {e}"));
    }
}

fn hold_camera(world: &mut World) {
    let Some(cam) = world.try_res::<ProofPlaymat>().map(|p| p.cam) else {
        return;
    };
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = camera(cam);
    }
}

/// The stone ball: a sphere in the world material that is no box face, so
/// the relief paints it with its free surface (triplanar).
fn draw_ball(world: &mut World) {
    let Some((ws, base)) = world.try_res::<LookData>().map(|l| world_slot(&l)) else {
        return;
    };
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else {
        return;
    };
    let (c, r) = BALL;
    let mut m = MeshInstance::new(MeshId::SPHERE, [AT[0] + c[0], AT[1] + c[1], AT[2] + c[2]]);
    m.scale = [2.0 * r; 3];
    m.material = ws;
    m.color = white(base);
    rw.draw_mesh(m);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_picks_the_mode_and_camera() {
        assert_eq!(mode_of(None), Mode::Dry);
        assert_eq!(mode_of(Some("http://x/?pm=before&cam=wall")), Mode::Before);
        assert_eq!(mode_of(Some("http://x/?cam=deck&pm=moss#a")), Mode::Moss);
        assert_eq!(cam_of(Some("http://x/?pm=wet&cam=floor")), Cam::Floor);
        assert_eq!(cam_of(Some("http://x/?pm=wet")), Cam::Wide);
        assert_eq!(config(Mode::Wet).layers.wetness, 0.75);
        assert_eq!(config(Mode::Flat).height_strength, 0.0);
    }

    #[test]
    fn before_uses_only_the_plain_surfaces() {
        for bx in scene(true).boxes {
            assert!(bx.surfaces.iter().all(|s| *s < surface::COUNT), "{:?}", bx.surfaces);
        }
        let after = scene(false);
        assert!(after.boxes.iter().all(|bx| bx.surfaces.iter().all(|s| (relief::surface::FIRST..relief::surface::FIRST + relief::surface::COUNT).contains(s))));
    }

    #[test]
    fn the_yard_bakes_and_draws_with_relief() {
        let mut app = crate::build_app();
        app.run_headless(4, 1000.0 / 60.0);
        let reg = app.world.res::<evani_core::params::ParamRegistry>();
        for p in ["material.relief.brick.mortar", "material.relief.moss", "material.relief.wetness", "material.relief.bake_size", "sky.follow_light"] {
            assert!(reg.id(p).is_some(), "no `{p}`");
        }
        drop(reg);
        let rel = app.world.res::<relief::Relief>();
        let base = rel.base().expect("the relief block is placed");
        let km = app.world.res::<evani_render::kit_material::KitMaterial>();
        assert_eq!(km.data()[base][0], relief::MAGIC);
        assert_eq!(km.data().len(), base + rel.block().len());
        assert!(km.wgsl().contains("fn kit_surface("));
        drop((rel, km));
        assert_eq!(app.world.res::<evani_core::overlay::ErrorLog>().error_count(), 0);
    }
}
