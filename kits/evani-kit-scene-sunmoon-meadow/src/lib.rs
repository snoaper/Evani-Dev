//! `evani-kit-scene-sunmoon-meadow`: the Sun and moon meadow Scene (docs/SCENES.md),
//! built from Asset Studio column 38's best round (round 8, commit f4266d5c):
//! a late-summer meadow of real grass, dark fields, far tree lines and the
//! procedural library's sun and moon discs over them, at golden hour.
//!
//! There is no character, goal, timer or fail state: the player is a
//! free-flying camera.
//!
//! * Keys and mouse: WASD fly (along the view), Space or E up, C or Q down,
//!   Shift faster, arrows turn. Mouse look while the mouse is held, or
//!   through the pointer lock, which is only asked for after the player
//!   clicks inside the game (the page takes it at the next click).
//! * Touch (evani-core `touch`, `touch-look`): a move stick on the left, a
//!   look pad on the right, Up and Down buttons and the pause button, all on
//!   the same actions the keys and mouse drive.
//! * Pause (Esc, P, the touch pause button) holds the flight and the clock.
//! * The day goes round slowly: `kit.scene_sunmoon_meadow.time` is the hour
//!   (it starts at the column's golden hour, the sun 1.9 degrees up) and
//!   `kit.scene_sunmoon_meadow.day_minutes` how many real minutes a whole
//!   day takes (0 holds the hour). The sun, the scene light, the haze and
//!   the ambient follow it through day, dusk and night (moonlight).
//! * The sky dome, the discs and the grass come from the procedural library
//!   (kit feature `sky`, turned on by the app's `procedural` feature, so the
//!   library is optional as the module rules ask); without it the kit still
//!   flies over the fields and woods.

pub mod scene;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, PluginContext, PluginError};
use evani_core::pointer_lock::PointerLock;
use evani_core::scene::Scene;
use evani_core::time::Time;
use evani_core::touch::{Anchor, Button, LookPad, Stick, TouchControls, TouchLayout, Zone};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Scene pushed at start.
pub const MAIN_SCENE: &str = "scene_sunmoon_meadow.main";

/// The column's camera (round 8's reference view): eye height (m), the
/// heading (degrees from +z towards +x: looking along -z), the tilt up that
/// puts the horizon low in the frame, and the vertical field of view.
pub const EYE: f32 = 1.7;
pub const HEADING: f32 = 180.0;
pub const PITCH: f32 = 13.9;
pub const FOV_Y: f32 = 36.0;

/// The hour the scene starts at: the sun 1.9 degrees up, as the column's
/// reference (see [`sun_at`]).
pub const START_HOUR: f32 = 19.855;

/// The actions, read every tick.
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub move_x: ActionId,
    pub move_y: ActionId,
    pub look_x: ActionId,
    pub look_y: ActionId,
    pub aim_x: ActionId,
    pub aim_y: ActionId,
    pub rise: ActionId,
    pub sink: ActionId,
    pub fast: ActionId,
    pub grab: ActionId,
    pub pause: ActionId,
}

/// The kit's tunables.
#[derive(Debug, Clone, Copy)]
pub struct Tune {
    pub time: F32Param,
    pub day_minutes: F32Param,
    pub speed: F32Param,
    pub fast: F32Param,
    pub mouse_sens: F32Param,
    pub look_rate: F32Param,
    pub radius: F32Param,
    pub ceiling: F32Param,
    pub fov: F32Param,
    pub view: F32Param,
}

/// The flying camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flyer {
    pub pos: [f32; 3],
    /// Heading (degrees from +z towards +x).
    pub yaw: f32,
    /// Tilt up (degrees).
    pub pitch: f32,
    /// Paused: the flight and the clock hold.
    pub paused: bool,
    /// The preset view last jumped to (`kit.scene_sunmoon_meadow.view`).
    pub view: f32,
}

impl Default for Flyer {
    fn default() -> Self {
        Self {
            pos: [0.0, EYE, 0.0],
            yaw: HEADING,
            pitch: PITCH,
            paused: false,
            view: 0.0,
        }
    }
}

/// The grass close-up (view 1): low over the meadow, looking down at the
/// blades a few metres ahead, with the low sun in front.
pub const CLOSE_EYE: f32 = 0.6;
pub const CLOSE_PITCH: f32 = -9.0;

impl Flyer {
    /// A preset view: 0 the column's (the start), 1 the grass close-up.
    pub fn preset(view: f32) -> Self {
        let mut f = Self::default();
        if view > 0.5 {
            f.pos[1] = CLOSE_EYE;
            f.pitch = CLOSE_PITCH;
        }
        f.view = view;
        f
    }

    /// The view direction.
    pub fn forward(&self) -> [f32; 3] {
        let (h, p) = (self.yaw.to_radians(), self.pitch.to_radians());
        [h.sin() * p.cos(), p.sin(), h.cos() * p.cos()]
    }
}

/// The kit's handles.
#[derive(Debug, Clone, Copy)]
pub struct SunMoonMeadow {
    pub actions: Actions,
    pub tune: Tune,
}

/// The scene-sunmoon-meadow kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct SceneSunmoonMeadowKitPlugin;

fn action(
    kit: &mut PluginContext,
    def: Result<ActionDef, evani_core::input::BindingParseError>,
    what: &str,
) -> Result<ActionId, PluginError> {
    let def = def.map_err(|e| e.to_string())?;
    Ok(kit.action(def.describe(what)))
}

fn actions(kit: &mut PluginContext) -> Result<Actions, PluginError> {
    Ok(Actions {
        move_x: action(
            kit,
            ActionDef::axis("move_x", &["keys:KeyA/KeyD", "pad:axis:0"]),
            "Fly left and right.",
        )?,
        move_y: action(
            kit,
            ActionDef::axis("move_y", &["keys:KeyW/KeyS", "pad:axis:1"]),
            "Fly forward and back, along the view.",
        )?,
        look_x: action(
            kit,
            ActionDef::axis("look_x", &["keys:ArrowLeft/ArrowRight", "pad:axis:2"]),
            "Turn left and right.",
        )?,
        look_y: action(
            kit,
            ActionDef::axis("look_y", &["keys:ArrowUp/ArrowDown", "pad:axis:3"]),
            "Look up and down.",
        )?,
        aim_x: action(
            kit,
            ActionDef::axis("aim_x", &["mouse:dx"]),
            "Mouse turn (with the mouse held, or the pointer lock).",
        )?,
        aim_y: action(
            kit,
            ActionDef::axis("aim_y", &["mouse:dy"]),
            "Mouse look up and down.",
        )?,
        rise: action(
            kit,
            ActionDef::button("rise", &["key:Space", "key:KeyE", "pad:button:0"]),
            "Fly up.",
        )?,
        sink: action(
            kit,
            ActionDef::button("sink", &["key:KeyC", "key:KeyQ", "pad:button:1"]),
            "Fly down.",
        )?,
        fast: action(
            kit,
            ActionDef::button("fast", &["key:ShiftLeft", "key:ShiftRight", "pad:button:5"]),
            "Fly faster while held.",
        )?,
        grab: action(
            kit,
            ActionDef::button("grab", &["mouse:0"]),
            "Hold to look with the mouse; a click asks for the pointer lock.",
        )?,
        pause: action(
            kit,
            ActionDef::button("pause", &["key:Escape", "key:KeyP", "pad:button:9"]),
            "Pause: hold the flight and the clock.",
        )?,
    })
}

/// Finger movement to mouse pixels on the look pad.
pub const LOOK_PAD_SENSITIVITY: f32 = 2.0;

/// The touch layout: a move stick on the left half, a look pad on the right
/// half (it drives the mouse-look actions), Up and Down on the right, and
/// the pause button top right.
pub fn touch_layout() -> TouchLayout {
    TouchLayout::new()
        .stick(
            Stick::two_axis("move", "move_x", "move_y")
                .dynamic(Zone::LEFT_HALF)
                .at(Anchor::BottomLeft, 8.0, 16.0)
                .label("Fly"),
        )
        .look_pad(
            LookPad::new("look", "aim_x", "aim_y")
                .zone(Zone::RIGHT_HALF)
                .sensitivity(LOOK_PAD_SENSITIVITY),
        )
        .button(Button::hold("up", "rise", "Up").at(Anchor::BottomRight, 4.0, 26.0))
        .button(Button::hold("down", "sink", "Down").at(Anchor::BottomRight, 4.0, 6.0))
        .pause("pause")
}

impl KitPlugin for SceneSunmoonMeadowKitPlugin {
    fn name(&self) -> &'static str {
        "scene-sunmoon-meadow"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let actions = actions(kit)?;
        kit.touch_controls(touch_layout())?;
        kit.pointer_lock();
        let p = |n: &str| format!("kit.scene_sunmoon_meadow.{n}");
        let tune = Tune {
            time: kit.param_f32(ParamDef::f32(&p("time"), START_HOUR).range(0.0, 24.0).unit("h").describe("The hour of the day: the sun, the light, the haze and the moonlit night follow it (19.855: the column's golden hour, the sun 1.9 degrees up)"))?,
            day_minutes: kit.param_f32(ParamDef::f32(&p("day_minutes"), 30.0).range(0.0, 240.0).unit("min").describe("Real minutes for a whole day to go round (0 holds the hour)"))?,
            speed: kit.param_f32(ParamDef::f32(&p("speed"), 8.0).range(0.5, 60.0).unit("m/s").describe("Flying speed"))?,
            fast: kit.param_f32(ParamDef::f32(&p("fast"), 4.0).range(1.0, 10.0).unit("x").describe("Speed multiplier while Shift is held"))?,
            mouse_sens: kit.param_f32(ParamDef::f32(&p("mouse_sens"), 0.12).range(0.01, 1.0).unit("deg/px").describe("Mouse and look pad turn per pixel"))?,
            look_rate: kit.param_f32(ParamDef::f32(&p("look_rate"), 80.0).range(10.0, 360.0).unit("deg/s").describe("Turn rate on the arrow keys and the pad's right stick"))?,
            radius: kit.param_f32(ParamDef::f32(&p("radius"), 80.0).range(10.0, 300.0).unit("m").describe("How far from the middle the camera may fly (the meadow's grass covers it)"))?,
            ceiling: kit.param_f32(ParamDef::f32(&p("ceiling"), 60.0).range(2.0, 300.0).unit("m").describe("How high the camera may fly"))?,
            fov: kit.param_f32(ParamDef::f32(&p("fov_y"), FOV_Y).range(10.0, 100.0).unit("deg").describe("The vertical field of view (36: the column's 60 degrees across at 16:9)"))?,
            view: kit.param_f32(ParamDef::f32(&p("view"), 0.0).range(0.0, 1.0).step(1.0).describe("Jump to a preset view: 0 the column's view (the start), 1 a close look at the grass (for captures and reviews)"))?,
        };
        // Values tuned live and written by bake.export (params.json,
        // curves/*.json, slots/*.expr in this crate).
        kit.baked_files(BAKED);
        kit.resource(SunMoonMeadow { actions, tune });
        kit.resource(Flyer::default());
        scene::build(kit)?;
        kit.system(SystemDesc::new(
            "scene_sunmoon_meadow.fly",
            Stage::Update,
            fly_system,
        ));
        kit.system(SystemDesc::new(
            "scene_sunmoon_meadow.lock",
            Stage::Frame,
            lock_system,
        ));
        kit.system(SystemDesc::new(
            "scene_sunmoon_meadow.draw",
            Stage::Frame,
            scene::draw,
        ));
        kit.scene(MAIN_SCENE, || Box::new(MainScene));
        Ok(())
    }

    fn start_scene(&self) -> Option<&'static str> {
        Some(MAIN_SCENE)
    }

    /// Free roam (docs/PLAYGROUNDS.md, docs/SCENES.md): a Scene has no
    /// goals, timer, lives or fail state at all, so it is the same in both
    /// modes.
    fn free_roam(&self) -> bool {
        true
    }
}

fn touch_active(world: &World) -> bool {
    world.try_res::<TouchControls>().is_some_and(|t| t.active())
}

/// One fixed step of flight and of the clock. Deterministic: the fixed
/// step only.
pub fn fly_system(world: &mut World) {
    let Some(k) = world.try_res::<SunMoonMeadow>().map(|k| *k) else {
        return;
    };
    let dt = world.res::<Time>().fixed_dt;
    let mut f = *world.res::<Flyer>();
    let mouse_look = touch_active(world) || evani_core::pointer_lock::locked(world);
    let (mx, my, lx, ly, ax, ay, rise, sink, fast, grab, pause) = {
        let i = world.res::<Input>();
        let a = k.actions;
        (
            i.value(a.move_x).clamp(-1.0, 1.0),
            i.value(a.move_y).clamp(-1.0, 1.0),
            i.value(a.look_x).clamp(-1.0, 1.0),
            i.value(a.look_y).clamp(-1.0, 1.0),
            i.value(a.aim_x),
            i.value(a.aim_y),
            i.down(a.rise),
            i.down(a.sink),
            i.down(a.fast),
            i.down(a.grab),
            i.just_pressed(a.pause),
        )
    };
    if pause {
        f.paused = !f.paused;
    }
    let (speed, boost, sens, rate, radius, ceiling, day_minutes, hour, view) = {
        let r = world.res::<ParamRegistry>();
        let t = k.tune;
        (
            r.f32(t.speed),
            r.f32(t.fast),
            r.f32(t.mouse_sens),
            r.f32(t.look_rate),
            r.f32(t.radius),
            r.f32(t.ceiling),
            r.f32(t.day_minutes),
            r.f32(t.time),
            r.f32(t.view).round(),
        )
    };
    if (view - f.view).abs() > 0.5 {
        f = Flyer {
            paused: f.paused,
            ..Flyer::preset(view)
        };
    }
    if !f.paused {
        // Look: turning right lowers the heading; mouse and pad down tilt down.
        let (mdx, mdy) = if mouse_look || grab {
            (ax, ay)
        } else {
            (0.0, 0.0)
        };
        f.yaw = (f.yaw - lx * rate * dt - mdx * sens).rem_euclid(360.0);
        f.pitch = (f.pitch - ly * rate * dt - mdy * sens).clamp(-85.0, 85.0);
        // Fly along the view, strafe level, rise and sink straight up.
        let fw = f.forward();
        let h = f.yaw.to_radians();
        let right = [-h.cos(), 0.0, h.sin()];
        let up = f32::from(u8::from(rise)) - f32::from(u8::from(sink));
        let v = speed * if fast { boost } else { 1.0 } * dt;
        let ahead = -my;
        for (i, p) in f.pos.iter_mut().enumerate() {
            *p += (fw[i] * ahead + right[i] * mx) * v + if i == 1 { up * v } else { 0.0 };
        }
        let (x, z) = (f.pos[0], f.pos[2]);
        let d = (x * x + z * z).sqrt();
        if d > radius {
            f.pos[0] = x / d * radius;
            f.pos[2] = z / d * radius;
        }
        f.pos[1] = f.pos[1].clamp(0.4, ceiling.max(0.5));
        // The clock: a whole day in `day_minutes` real minutes.
        if day_minutes > 0.0 {
            let next = (hour + dt * 24.0 / (day_minutes * 60.0)).rem_euclid(24.0);
            let _ = world.res_mut::<ParamRegistry>().set_f32(k.tune.time, next);
        }
    }
    *world.res_mut::<Flyer>() = f;
}

/// The pointer lock, once a frame: asked for only when the player clicks
/// inside the game (the page takes it at the next click), given back on
/// pause. The engine drops it on its own on Esc, blur, a hidden tab and
/// errors; those losses are read and dropped (mouse-drag look still works).
pub fn lock_system(world: &mut World) {
    let Some(k) = world.try_res::<SunMoonMeadow>().map(|k| *k) else {
        return;
    };
    let clicked = world.res::<Input>().just_pressed(k.actions.grab);
    let paused = world.res::<Flyer>().paused;
    let touch = touch_active(world);
    let Some(mut lock) = world.try_res_mut::<PointerLock>() else {
        return;
    };
    let _ = lock.take_loss();
    if paused {
        if lock.wanted() {
            lock.release();
        }
    } else if clicked && !touch && !lock.wanted() && !lock.is_locked() {
        lock.request();
    }
}

/// The sun at `hour`: (elevation, bearing to the left of the column's view),
/// degrees. Sunrise 08:00, sunset 20:00, 50 degrees up at 14:00; at
/// [`START_HOUR`] it stands where the column's reference has it (1.9 up,
/// 0.11 left), and it drifts right as it sets.
pub fn sun_at(hour: f32) -> (f32, f32) {
    let el = 50.0 * (std::f32::consts::TAU * (hour - 8.0) / 24.0).sin();
    let az = scene::SUN_AZ - 12.0 * (hour - START_HOUR);
    (el, az)
}

struct MainScene;

impl Scene for MainScene {
    fn name(&self) -> &str {
        MAIN_SCENE
    }

    fn on_enter(&mut self, world: &mut World) {
        *world.res_mut::<Flyer>() = Flyer::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::play::{PlayMode, ScriptKey, free_roam_test};

    const DT: f64 = 1000.0 / 60.0;

    #[test]
    fn registers_tunables_actions_and_touch() {
        let mut a = App::new("t").with_kit(SceneSunmoonMeadowKitPlugin);
        a.run_headless(3, DT);
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "time",
            "day_minutes",
            "speed",
            "fast",
            "mouse_sens",
            "look_rate",
            "radius",
            "ceiling",
            "fov_y",
            "view",
        ] {
            let name = format!("kit.scene_sunmoon_meadow.{n}");
            let id = reg.id(&name).unwrap();
            assert_eq!(
                reg.def(id).unwrap().owner,
                "kit.scene-sunmoon-meadow",
                "{name}"
            );
        }
        drop(reg);
        let input = a.world.res::<Input>();
        for n in [
            "move_x", "move_y", "aim_x", "aim_y", "rise", "sink", "pause",
        ] {
            assert!(input.action_id(n).is_some(), "{n}");
        }
        drop(input);
        assert!(a.world.try_res::<TouchControls>().is_some());
        assert!(a.world.try_res::<PointerLock>().is_some());
    }

    #[test]
    fn starts_at_the_columns_view_and_golden_hour() {
        let mut a = App::new("t").with_kit(SceneSunmoonMeadowKitPlugin);
        a.run_headless(2, DT);
        let f = *a.world.res::<Flyer>();
        assert!(
            (f.pos[1] - EYE).abs() < 1e-3
                && (f.yaw - HEADING).abs() < 1e-3
                && (f.pitch - PITCH).abs() < 1e-3,
            "{f:?}"
        );
        let (el, az) = sun_at(START_HOUR);
        assert!((el - 1.9).abs() < 0.05, "{el}");
        assert!((az - scene::SUN_AZ).abs() < 1e-4);
        // Day, dusk and night.
        assert!(sun_at(14.0).0 > 45.0);
        assert!(sun_at(20.5).0 < 0.0 && sun_at(20.5).0 > -10.0);
        assert!(sun_at(1.0).0 < -30.0);
    }

    #[test]
    fn flies_forward_up_and_stays_in_bounds() {
        let mut a = App::new("t")
            .with_play_mode(PlayMode::FreeRoam)
            .with_kit(SceneSunmoonMeadowKitPlugin);
        a.start();
        let script = [
            ScriptKey::down(0.1, "KeyW"),
            ScriptKey::down(0.1, "Space"),
            ScriptKey::up(2.1, "Space"),
            ScriptKey::up(30.0, "KeyW"),
        ];
        free_roam_test(&mut a, 31.0, &script).unwrap();
        let f = *a.world.res::<Flyer>();
        // Forward is along -z at the start; Space lifted it.
        assert!(f.pos[2] < -20.0, "{f:?}");
        assert!(f.pos[1] > EYE + 5.0, "{f:?}");
        let d = (f.pos[0] * f.pos[0] + f.pos[2] * f.pos[2]).sqrt();
        assert!(d <= 80.0 + 1e-3, "{d}");
    }

    #[test]
    fn the_clock_turns_and_pause_holds_it() {
        let mut a = App::new("t").with_kit(SceneSunmoonMeadowKitPlugin);
        a.start();
        let hour = |a: &App| {
            a.world
                .res::<ParamRegistry>()
                .f32(a.world.res::<SunMoonMeadow>().tune.time)
        };
        let h0 = hour(&a);
        a.run_headless(600, DT);
        let h1 = hour(&a);
        // Ten seconds of a 30 minute day: 8 minutes of the scene's day.
        assert!(
            (h1 - h0 - 10.0 * 24.0 / 1800.0).abs() < 0.01,
            "{h0} -> {h1}"
        );
        a.world.res_mut::<Flyer>().paused = true;
        a.run_headless(120, DT);
        assert!((hour(&a) - h1).abs() < 1e-6);
    }

    #[test]
    fn the_view_param_jumps_to_the_grass_close_up() {
        let mut a = App::new("t").with_kit(SceneSunmoonMeadowKitPlugin);
        a.run_headless(2, DT);
        let v = a.world.res::<SunMoonMeadow>().tune.view;
        let _ = a.world.res_mut::<ParamRegistry>().set_f32(v, 1.0);
        a.run_headless(2, DT);
        let f = *a.world.res::<Flyer>();
        assert!(
            (f.pos[1] - CLOSE_EYE).abs() < 1e-3 && (f.pitch - CLOSE_PITCH).abs() < 1e-3,
            "{f:?}"
        );
        let _ = a.world.res_mut::<ParamRegistry>().set_f32(v, 0.0);
        a.run_headless(2, DT);
        let f = *a.world.res::<Flyer>();
        assert!(
            (f.pos[1] - EYE).abs() < 1e-3 && (f.pitch - PITCH).abs() < 1e-3,
            "{f:?}"
        );
    }

    /// Free roam: two minutes of flying about on the keys, with no game
    /// over, timer expiry, lost life or error (docs/PLAYGROUNDS.md).
    #[test]
    fn free_roam_holds() {
        let mut app = App::new("t")
            .with_play_mode(PlayMode::FreeRoam)
            .with_kit(SceneSunmoonMeadowKitPlugin);
        let keys = [
            "KeyW",
            "KeyA",
            "KeyS",
            "KeyD",
            "Space",
            "KeyC",
            "ArrowLeft",
            "ArrowUp",
            "ShiftLeft",
            "KeyE",
        ];
        let mut script = Vec::new();
        for i in 0..20 {
            let at = 2.0 + f64::from(i) * 5.0;
            let k = keys[i as usize % keys.len()];
            script.push(ScriptKey::down(at, k));
            script.push(ScriptKey::up(at + 2.0, k));
        }
        let r = free_roam_test(&mut app, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(r.sim_seconds > 119.0, "{r:?}");
        let f = *app.world.res::<Flyer>();
        assert!(f.pos.iter().all(|v| v.is_finite()) && f.pos[1] >= 0.4);
    }
}
