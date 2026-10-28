//! The sandbox adapter: runs the engine sandbox natively (no browser) and
//! records what it draws.
//!
//! - The run: the sandbox's app as `apps/sandbox` builds it, without fx,
//!   live or the debug panel (the scene is the example kit's
//!   `draw::system`), at a 1280 x 720 viewport for one full turn of its
//!   orbit camera. A snapshot system, last in the Frame stage, copies the
//!   frame's draw list and camera every `every` frames.
//! - The spinners turn all the time, so they are movers; the ground, the
//!   terraces, columns and hills (and, without a shadow map, the spinners'
//!   shadow discs) are the static world.
//! - Collision: the sandbox has none (nothing moves through the world), so
//!   the collision check does not apply.

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::App;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::plugin::LookData;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_geocheck::checks::Snapshot;
use evani_geocheck::image::Camera;
use evani_render::mesh::MeshLibrary;
use evani_render::RenderModule;
use evani_render::world::RenderWorld;

/// The example kit's still-world switch: on, the spinners hold still and
/// are not drawn, so the render phase's gap and flicker shots see only the
/// static world (as the kart racer's `kit.kart_racer.static_world`).
pub const STILL_PARAM: &str = "kit.example.static_world";

pub const VIEWPORT: [f32; 2] = [1280.0, 720.0];
pub const ASPECT: f64 = 1280.0 / 720.0;
const DT_MS: f64 = 1000.0 / 60.0;

/// What the native run recorded.
pub struct Recording {
    pub frames: Vec<Snapshot>,
    /// Look material names by index (labels).
    pub materials: Vec<String>,
    /// The tier the run drew with (the render phase uses the same).
    pub tier: String,
    pub library: MeshLibrary,
}

/// The sandbox's orbit camera (the bench path and every scripted moment).
pub fn orbit() -> evani_core::camera::CameraPath {
    evani_kit_example::draw::camera_path()
}

/// The sandbox as `apps/sandbox` builds it, minus the browser-only modules
/// (fx, live, the debug panel): the renderer, the example kit with the
/// sandbox's 400 spinners, the basic look and the kit's scene.
fn sandbox_app() -> App {
    let mut app = App::new("sandbox")
        .with_module(RenderModule::new())
        .with_kit(evani_kit_example::ExampleKitPlugin)
        .with_look(evani_look_basic::BasicLook);
    let mut game = app.context("sandbox");
    game.baked_params(r#"{"kit.example.count":400}"#);
    game.system(evani_kit_example::draw::system());
    app
}

/// Run the sandbox natively for `seconds` and record the draw lists.
pub fn record(seconds: f64, every: u32) -> Result<Recording, String> {
    let mut app = sandbox_app();
    let store: Rc<RefCell<Vec<Snapshot>>> = Rc::new(RefCell::new(Vec::new()));
    let st = store.clone();
    let mut n: u32 = 0;
    {
        let mut ctx = app.context("geocheck");
        ctx.system(
            SystemDesc::new("geocheck.viewport", Stage::Frame, |w: &mut World| {
                if let Some(mut rw) = w.try_res_mut::<RenderWorld>() {
                    rw.viewport_css = VIEWPORT;
                }
            })
            .order(i32::MIN),
        );
        ctx.system(
            SystemDesc::new("geocheck.snapshot", Stage::Frame, move |w: &mut World| {
                n += 1;
                if !n.is_multiple_of(every.max(1)) {
                    return;
                }
                let time = w.try_res::<Time>().map_or(0.0, |t| t.render_seconds());
                let Some(rw) = w.try_res::<RenderWorld>() else { return };
                let mut meshes = rw.meshes.clone();
                let decal_from = meshes.len();
                meshes.extend_from_slice(&rw.decals);
                let camera = Camera::from_pose(&rw.camera, ASPECT);
                st.borrow_mut().push(Snapshot { time, meshes, decal_from, camera });
            })
            .order(i32::MAX),
        );
    }
    app.run_headless((seconds * 60.0).round() as u32, DT_MS);
    let materials = app
        .world
        .try_res::<LookData>()
        .map(|l| l.materials.iter().map(|m| m.name.clone()).collect())
        .unwrap_or_default();
    let tier = app.world.try_res::<Tiers>().map(|t| t.active().id.clone()).unwrap_or_default();
    let library = app.world.try_res::<MeshLibrary>().map(|l| (*l).clone()).unwrap_or_default();
    let frames = std::mem::take(&mut *store.borrow_mut());
    Ok(Recording { frames, materials, tier, library })
}

/// The sandbox has no characters or vehicles.
pub fn role(_m: &evani_render::world::MeshInstance) -> evani_geocheck::checks::Role {
    evani_geocheck::checks::Role::Other
}
