//! `evani-kit-material-view`: a game for viewing and tuning materials, and
//! the in-engine home of Evani.Materials (one room per world).
//!
//! The kit holds what every build shares: the data ([`library`]: scenes,
//! material families and their presets, the four light presets, the world
//! rooms), the viewer's state and commands ([`viewer`]), the orbit camera
//! and turntable ([`orbit`]), the keys and the touch controls. The scenes
//! themselves are drawn by the app, one module per material family, each
//! compiled in only when evani.toml lists the family; the page (the scene
//! row, the browser, the generated tuning panel, quality and compare) is
//! the app's `web.rs`.
//!
//! Params (owner `kit.material_view`): `orbit_speed`, `zoom_speed`,
//! `turntable_speed`, `key_orbit_speed`, `split`.
//!
//! The art styles' starter materials are [`styles`]; their params
//! (`kit.material_view.style.*`) are registered only by `styles::install`,
//! which the app calls when built with `material-styles`.
//!
//! The Asset & Style Studio (feature `studio`, docs/studio/) is `studio`:
//! the selection record and its auto-match rule as a resource.
//!
//! A viewer has no goals, timer, lives or fail state, so free roam is the
//! same as normal play.

#[cfg(feature = "catalogue")]
pub mod catalogue;
pub mod library;
pub mod moments;
pub mod orbit;
#[cfg(feature = "studio")]
pub mod studio;
pub mod styles;
pub mod viewer;

#[cfg(test)]
mod tests;

use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, LookData, PluginContext, PluginError};
use evani_core::time::Time;
use evani_core::touch::{Anchor, Button, ControlSize, TouchLayout};
use evani_render::{MeshId, MeshInstance, RenderWorld};

pub use library::Data;
pub use viewer::{Command, Viewer};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// The parsed data files (shared, read-only).
#[derive(Debug, Clone)]
pub struct ViewerData(pub Rc<Data>);

/// Handles the kit's systems read every frame.
#[derive(Debug, Clone, Copy)]
pub struct MaterialViewKit {
    /// Degrees of orbit for a drag the height of the view.
    pub orbit_speed: F32Param,
    /// Distance factor per wheel notch or pinch step.
    pub zoom_speed: F32Param,
    pub turntable_speed: F32Param,
    pub key_orbit_speed: F32Param,
    /// Where the compare line sits (0..1 across the view).
    pub split: F32Param,
    pub actions: Actions,
}

/// The kit's input actions.
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub orbit_x: ActionId,
    pub orbit_y: ActionId,
    pub zoom: ActionId,
    pub reset: ActionId,
    pub turntable: ActionId,
    pub compare: ActionId,
    pub next_scene: ActionId,
    pub light: ActionId,
    pub pause: ActionId,
}

/// The material-view kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct MaterialViewKitPlugin;

fn speed(kit: &mut PluginContext, name: &str, v: f32, range: (f32, f32), unit: &str, what: &str) -> Result<F32Param, PluginError> {
    Ok(kit.param_f32(ParamDef::f32(&format!("kit.material_view.{name}"), v).range(range.0, range.1).unit(unit).describe(what))?)
}

/// The touch controls: every one is an action the keys already drive.
pub fn touch_layout() -> TouchLayout {
    TouchLayout::new()
        .button(Button::tap("reset", "reset", "Reset").size(ControlSize::Small).at(Anchor::TopRight, 14.0, 3.0))
        .button(Button::tap("spin", "turntable", "Spin").size(ControlSize::Small).at(Anchor::TopRight, 25.0, 3.0))
        .pause_at("pause", Anchor::TopRight, 3.0, 3.0)
}

impl KitPlugin for MaterialViewKitPlugin {
    fn name(&self) -> &'static str {
        "material-view"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let data = Data::shipped().map_err(PluginError)?;
        let orbit_speed = speed(kit, "orbit_speed", 220.0, (10.0, 720.0), "deg", "Degrees of orbit for a drag the height of the view.")?;
        let zoom_speed = speed(kit, "zoom_speed", 0.12, (0.01, 0.5), "x", "How much one wheel notch or pinch step moves the camera in or out.")?;
        let turntable_speed = speed(kit, "turntable_speed", 18.0, (0.0, 180.0), "deg/s", "How fast the turntable circles the camera.")?;
        let key_orbit_speed = speed(kit, "key_orbit_speed", 90.0, (5.0, 360.0), "deg/s", "Orbit speed while an arrow key or the stick is held.")?;
        let split = kit.param_f32(
            ParamDef::f32("kit.material_view.split", 0.5)
                .range(0.0, 1.0)
                .step(0.01)
                .describe("Where the compare line sits: A (saved) before it, B (current) after it."),
        )?;
        let axis = |name: &str, b: &[&str], what: &str| ActionDef::axis(name, b).map(|d| d.describe(what));
        let button = |name: &str, b: &[&str], what: &str| ActionDef::button(name, b).map(|d| d.describe(what));
        let actions = Actions {
            orbit_x: kit.action(axis("orbit_x", &["keys:ArrowLeft/ArrowRight", "keys:KeyA/KeyD", "pad:axis:0"], "Orbit left and right.")?),
            orbit_y: kit.action(axis("orbit_y", &["keys:ArrowDown/ArrowUp", "keys:KeyS/KeyW", "pad:axis:1"], "Orbit up and down.")?),
            zoom: kit.action(axis("zoom", &["keys:KeyE/KeyQ", "keys:Equal/Minus", "pad:axis:3"], "Zoom in and out.")?),
            reset: kit.action(button("reset", &["key:KeyR", "pad:button:3"], "Back to the material's own camera.")?),
            turntable: kit.action(button("turntable", &["key:KeyT", "pad:button:2"], "Turntable on or off.")?),
            compare: kit.action(button("compare", &["key:KeyC", "pad:button:1"], "Compare A (saved) and B (current).")?),
            next_scene: kit.action(button("next_scene", &["key:KeyN", "pad:button:5"], "Next scene.")?),
            light: kit.action(button("light", &["key:KeyL", "pad:button:4"], "Next light preset.")?),
            pause: kit.action(button("pause", &["key:Escape", "key:KeyP", "pad:button:9"], "Pause or go on.")?),
        };
        kit.touch_controls(touch_layout())?;
        kit.data_file("material_view.library", library::LIBRARY_JSON);
        kit.data_file("material_view.lights", library::LIGHTS_JSON);
        kit.data_file("material_view.world_rooms", library::ROOMS_JSON);
        kit.baked_files(BAKED);
        kit.resource(MaterialViewKit { orbit_speed, zoom_speed, turntable_speed, key_orbit_speed, split, actions });
        kit.resource(ViewerData(Rc::new(data)));
        kit.resource(Viewer::default());
        kit.system(SystemDesc::new("material_view.keys", Stage::Update, keys_system));
        kit.system(SystemDesc::new("material_view.apply", Stage::Frame, viewer::apply_system));
        kit.system(SystemDesc::new("material_view.camera", Stage::Frame, camera_system));
        Ok(())
    }

    /// Free roam (docs/PLAYGROUNDS.md): a viewer has no goals, timer, lives
    /// or game over, so it is the same in both modes.
    fn free_roam(&self) -> bool {
        true
    }
}

/// Keys and the pad, at the fixed 60 Hz step: held axes turn and zoom,
/// presses become commands.
pub fn keys_system(world: &mut World) {
    let Some(k) = world.try_res::<MaterialViewKit>().map(|k| *k) else { return };
    let (deg, zoom) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(k.key_orbit_speed) / 60.0, reg.f32(k.zoom_speed))
    };
    let a = k.actions;
    let mut cmds = Vec::new();
    {
        let input = world.res::<Input>();
        let (x, y, z) = (input.value(a.orbit_x), input.value(a.orbit_y), input.value(a.zoom));
        if x != 0.0 || y != 0.0 {
            cmds.push(Command::Orbit { daz: -x * deg, del: y * deg });
        }
        if z != 0.0 {
            cmds.push(Command::Zoom(1.0 - z * zoom / 6.0));
        }
        for (id, c) in [
            (a.reset, Command::Reset),
            (a.turntable, Command::Turntable(None)),
            (a.compare, Command::Compare(None)),
            (a.next_scene, Command::NextScene),
            (a.light, Command::NextLight),
            (a.pause, Command::Pause(None)),
        ] {
            if input.just_pressed(id) {
                cmds.push(c);
            }
        }
    }
    if !cmds.is_empty()
        && let Some(mut v) = world.try_res_mut::<Viewer>()
    {
        v.pending.extend(cmds);
    }
}

/// Place the camera; with no material family compiled in, draw a plain ball
/// on a floor so the page still shows something.
pub fn camera_system(world: &mut World) {
    let Some(k) = world.try_res::<MaterialViewKit>().map(|k| *k) else { return };
    let tick = world.res::<Time>().tick;
    let speed = world.res::<ParamRegistry>().f32(k.turntable_speed);
    let Some((pose, bare)) = world.try_res::<Viewer>().map(|v| (v.orbit.pose(v.turn.angle(tick, speed)), v.families_on.is_empty())) else {
        return;
    };
    let (ground, accent) = world
        .try_res::<LookData>()
        .map_or((0, 0), |l| (RenderWorld::material(&l, "ground"), RenderWorld::material(&l, "accent")));
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    rw.camera = pose;
    if bare {
        let mut floor = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
        floor.scale = [40.0, 1.0, 40.0];
        floor.material = ground;
        rw.draw_mesh(floor);
        let mut ball = MeshInstance::new(MeshId::SPHERE, [0.0, 0.45, 0.0]);
        ball.scale = [0.9; 3];
        ball.material = accent;
        ball.color = [0.75, 0.75, 0.78, 1.0];
        rw.draw_mesh(ball);
    }
}
