//! `evani-kit-model-view`: the dev-only model viewer behind the Evani.Model
//! tab (docs/EVANI_MODEL_TAB.md, milestone T1a). It is never a game: the app
//! `apps/model-view` has no `[playground]` and `cargo xtask publish` refuses it.
//!
//! * Shows one version of one model at one tier, drawn by the engine's own
//!   3D path (the `meshes` feature, one mesh per region colour; "Preview
//!   shading" until T4 moves it onto Stage 3's model pipeline).
//! * Turntable on fixed ticks (`kit.model_view.turntable_speed`), orbit and
//!   zoom from pointer input, double-click to frame, a ground grid and a
//!   contact shadow.
//! * Hot swap: a new version or tier loads in the background and replaces
//!   the old one on the frame after it decoded, with the camera kept.
//! * With feature `live`: the `view.*` commands and `view_*` events.

pub mod decode;
pub mod orbit;
pub mod view;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, LookData, PluginContext, PluginError};
use evani_core::time::Time;
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, MeshLibrary, RenderWorld};

pub use view::{Address, Fetches, PointerInput, Status, Viewer};

/// Handles the systems read every frame.
#[derive(Debug, Clone, Copy)]
pub struct ViewParams {
    pub turntable_speed: F32Param,
    pub ground_grey: F32Param,
}

/// Lines for the page's console (the app drains them).
#[derive(Debug, Default)]
pub struct ViewLog(pub Vec<String>);

/// The model viewer kit.
pub struct ModelViewKit;

impl KitPlugin for ModelViewKit {
    fn name(&self) -> &'static str {
        "model-view"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let turntable_speed = kit.param_f32(
            ParamDef::f32("kit.model_view.turntable_speed", 12.0)
                .range(0.0, 120.0)
                .unit("deg/s")
                .describe("How fast the shown model turns on the turntable."),
        )?;
        let ground_grey = kit.param_f32(
            ParamDef::f32("kit.model_view.ground_grey", 0.62)
                .range(0.0, 1.0)
                .describe("Brightness of the viewer's ground."),
        )?;
        kit.resource(ViewParams { turntable_speed, ground_grey });
        kit.resource(Viewer::default());
        kit.resource(ViewLog::default());
        if kit.world.try_res::<MeshLibrary>().is_none() {
            kit.resource(MeshLibrary::default());
        }
        #[cfg(feature = "live")]
        kit.resource(evani_live::AppCommands { prefix: "view.", handle: live::handle });
        kit.system(SystemDesc::new("model_view.update", Stage::Frame, update_system));
        kit.system(SystemDesc::new("model_view.draw", Stage::Frame, draw_system));
        Ok(())
    }
}

/// The turntable speed in degrees per second.
pub fn turntable_speed(world: &World) -> f32 {
    world.try_res::<ViewParams>().map_or(12.0, |p| world.res::<ParamRegistry>().f32(p.turntable_speed))
}

/// Pointer input, the load in flight and the hot swap.
pub fn update_system(world: &mut World) {
    let (frame, now_ms, frame_ms) = {
        let t = world.res::<Time>();
        (t.frame, t.now_ms, t.frame_dt * 1000.0)
    };
    let aspect = {
        let rw = world.res::<RenderWorld>();
        let [w, h] = rw.viewport_css;
        if w > 0.0 && h > 0.0 { w / h } else { 0.0 }
    };
    let out = {
        let mut lib = world.res_mut::<MeshLibrary>();
        let mut v = world.res_mut::<Viewer>();
        v.apply_pointer();
        v.update(&mut lib, aspect, frame, now_ms, frame_ms)
    };
    world.res_mut::<ViewLog>().0.extend(out.log.iter().cloned());
    #[cfg(feature = "live")]
    live::send(world, out);
    #[cfg(not(feature = "live"))]
    let _ = out;
}

/// Ground grid spacing for a model of bounding radius `r` (metres).
pub fn grid_step(r: f32) -> f32 {
    if r < 0.6 {
        0.1
    } else if r < 1.5 {
        0.25
    } else if r < 4.0 {
        0.5
    } else {
        1.0
    }
}

/// Lines either side of the centre.
const GRID_LINES: i32 = 10;

pub fn draw_system(world: &mut World) {
    let tick = world.res::<Time>().tick;
    let speed = turntable_speed(world);
    let grey = world.try_res::<ViewParams>().map_or(0.62, |p| world.res::<ParamRegistry>().f32(p.ground_grey));
    let (body, ground, line) = match world.try_res::<LookData>() {
        Some(look) => {
            let has = |n: &str| look.materials.iter().any(|m| m.name == n);
            let pick = |a: &str, b: &str| RenderWorld::material(&look, if has(a) { a } else { b });
            (pick("kart_body", "accent"), pick("kerb_white", "ground"), pick("road", "ground"))
        }
        None => (0, 0, 0),
    };
    let v = world.res::<Viewer>();
    let angle = v.turn.angle(tick, speed);
    let radius = v.shown.as_ref().map_or(1.0, |s| s.decoded.radius());
    let foot = v.shown.as_ref().map_or(0.5, |s| {
        let d = &s.decoded;
        0.5 * (d.max[0] - d.min[0]).max(d.max[2] - d.min[2])
    });
    let instances = v.instances();
    let pose = v.orbit.pose();
    drop(v);
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = pose;
    // Ground: a wide plane, then a grid of thin decal lines around the model.
    let mut g = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
    g.scale = [400.0, 1.0, 400.0];
    g.material = ground;
    g.color = [grey, grey, grey, 1.0];
    rw.draw_mesh(g);
    let step = grid_step(radius);
    let half = step * GRID_LINES as f32;
    let width = step * 0.04;
    for i in -GRID_LINES..=GRID_LINES {
        let o = i as f32 * step;
        let c = if i == 0 { 0.30 } else { 0.45 };
        for along_x in [true, false] {
            let (pos, scale) = if along_x { ([0.0, 0.0, o], [2.0 * half, 0.002, width]) } else { ([o, 0.0, 0.0], [width, 0.002, 2.0 * half]) };
            let mut l = MeshInstance::new(MeshId::CUBE, pos);
            l.scale = scale;
            l.material = line;
            l.color = [c, c, c, 1.0];
            rw.draw_decal(l);
        }
    }
    // Contact shadow under the model (drawn when the shadow map is off).
    if !instances.is_empty() {
        rw.blobs.push([0.0, 0.0, 0.0, foot * 1.1]);
    }
    let rot = quat_axis_angle([0.0, 1.0, 0.0], angle.to_radians());
    for (mesh, color) in instances {
        let mut m = MeshInstance::new(mesh, [0.0, 0.0, 0.0]);
        m.rotation = rot;
        m.material = body;
        m.color = color;
        rw.draw_mesh(m);
    }
}

/// The live layer: `view.*` commands and `view_*` events.
#[cfg(feature = "live")]
pub mod live {
    use evani_core::ecs::World;
    use evani_core::time::Time;
    use evani_live::{AppCall, LiveError, app_reply, emit_event};
    use serde_json::{Value, json};

    use crate::view::{Outcome, ViewError, Viewer};

    fn live_err(e: ViewError) -> LiveError {
        let mut le = LiveError::app(e.code, e.message);
        if let Some(f) = e.file {
            le.data = Some(json!({ "file": f }));
        }
        le
    }

    fn superseded(world: &World, r: (u64, Value)) {
        let call = AppCall { token: r.0, id: r.1, cmd: String::new(), params: Value::Null };
        app_reply(world, &call, Err(LiveError::app("superseded", "A newer view.load replaced this one before it finished")));
    }

    /// The [`evani_live::AppCommands`] handler for `view.*`.
    pub fn handle(world: &World, call: &AppCall) -> Option<Result<Value, LiveError>> {
        let (tick, frame, now_ms, frame_ms) = {
            let t = world.res::<Time>();
            (t.tick, t.frame, t.now_ms, t.frame_dt * 1000.0)
        };
        let speed = crate::turntable_speed(world);
        let reply = (call.token, call.id.clone());
        let started = {
            let mut v = world.res_mut::<Viewer>();
            match call.cmd.as_str() {
                "view.load" => v.command_load(&call.params, reply, now_ms),
                "view.tier" => v.command_tier(&call.params, reply, now_ms),
                "view.camera" => return Some(v.command_camera(&call.params, tick, speed).map_err(live_err)),
                "view.state" => return Some(Ok(v.state(tick, frame, speed, frame_ms))),
                c => return Some(Err(LiveError::app("unknown_command", format!("no command {c}")))),
            }
        };
        match started {
            Ok(old) => {
                if let Some(r) = old {
                    superseded(world, r);
                }
                None
            }
            Err(e) => Some(Err(live_err(e))),
        }
    }

    /// Send a frame's replies and events.
    pub fn send(world: &World, out: Outcome) {
        for ((token, id), res) in out.replies {
            let call = AppCall { token, id, cmd: String::new(), params: Value::Null };
            app_reply(world, &call, res.map_err(live_err));
        }
        for (name, data) in out.events {
            emit_event(world, name, data);
        }
    }
}

#[cfg(test)]
mod tests;
