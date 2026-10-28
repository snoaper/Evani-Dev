//! `evani-kit-example`: the smallest real kit. It proves the [`KitPlugin`]
//! interface end to end and gives `bake.export` a kit crate to write into.
//!
//! * Params (owner `kit.example`): `kit.example.spin_speed`,
//!   `kit.example.count`, `kit.example.boost`.
//! * Formula: `kit.example.boost_scale(held, boost)`, an expression the AI can
//!   rewrite live.
//! * Input action: `boost` (Space or gamepad A).
//! * Component [`Spinner`], system `example.spin` (fixed `Update` stage),
//!   scene `example.main` that spawns `count` spinners from the seeded RNG.
//! * Data file `example.level` (shipped inside the wasm).
//! * [`place`]: where and how the spinners stand (a grid of bevelled cubes
//!   and ringed orbs, each resting on the ground on its lowest point).
//! * [`draw`]: the scene the sandbox draws (spinners, ground, middle
//!   distance, sprites, orbit camera), here so native tools draw the same.

pub mod draw;
pub mod place;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::formulas::{FormulaId, FormulaJson, Formulas};
use evani_core::input::{ActionDef, ActionId, Input};
use evani_core::params::{BoolParam, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, PluginContext, PluginError};
use evani_core::rng::Pcg32;
use evani_core::scene::Scene;
use evani_core::time::Time;
use evani_core::curve::Curve;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// Scene pushed at start.
pub const MAIN_SCENE: &str = "example.main";

/// Tiny level description, shipped as a data file.
pub const LEVEL_JSON: &str = r#"{"name":"example","ring_radius":4.0}"#;

/// A spinning thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spinner {
    /// Starting angle (radians). The angle at sim time `t` is
    /// [`Spinner::angle_at`]: a pure function of `t`, so a capture at a given
    /// sim time draws the same pose however long the game ran before it.
    pub angle: f32,
    /// Per-entity speed multiplier (0.5..1.5).
    pub rate: f32,
}

impl Spinner {
    /// The angle after the shared clock has turned `turned` radians, for a
    /// spinner whose speed factor (`kit.example.spin_falloff(rate)`) is
    /// `factor`.
    pub fn angle_at(&self, factor: f32, turned: f64) -> f32 {
        (f64::from(self.angle) + f64::from(factor) * turned).rem_euclid(core::f64::consts::TAU) as f32
    }
}

/// How far the spinners have turned, as a function of sim time: radians
/// for a speed factor of 1. The spin system re-anchors it whenever the speed
/// changes (the param, the boost, the still world), so between changes it
/// is a straight line in sim time and every spinner's pose is a pure function
/// of `Time::render_seconds` (smooth between ticks, and the same at a given
/// sim time in every capture).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpinClock {
    /// Sim time of the last anchor (s).
    pub at: f64,
    /// Turned at the anchor (radians).
    pub turned: f64,
    /// Turn rate since the anchor (rad/s).
    pub rate: f64,
}

impl SpinClock {
    /// Radians turned at sim time `t`.
    pub fn turned(&self, t: f64) -> f64 {
        self.turned + self.rate * (t - self.at)
    }

    /// From sim time `t` on, turn at `rate` (rad/s).
    pub fn set_rate(&mut self, t: f64, rate: f64) {
        if rate != self.rate {
            self.turned = self.turned(t);
            self.at = t;
            self.rate = rate;
        }
    }
}

/// Handles the kit's systems read every tick (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct ExampleKit {
    pub spin_speed: F32Param,
    pub count: I32Param,
    pub boost: F32Param,
    pub boost_action: ActionId,
    pub boost_scale: FormulaId,
    /// Curve mapping a spinner's rate (0.5..1.5) to its speed factor.
    pub spin_falloff: FormulaId,
    /// `kit.example.static_world`: the geometry check's still world (the
    /// spinners held and not drawn).
    pub static_world: BoolParam,
}

/// The example kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExampleKitPlugin;

impl KitPlugin for ExampleKitPlugin {
    fn name(&self) -> &'static str {
        "example"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        kit.component::<Spinner>("example.spinner");

        let spin_speed = kit.param_f32(
            ParamDef::f32("kit.example.spin_speed", 1.5)
                .range(0.0, 20.0)
                .unit("rad/s")
                .describe("How fast the example spinners turn."),
        )?;
        let count = kit.param_i32(
            ParamDef::i32("kit.example.count", 64)
                .range(1.0, 10_000.0)
                .describe("How many spinners the example scene spawns."),
        )?;
        let boost = kit.param_f32(
            ParamDef::f32("kit.example.boost", 3.0)
                .range(1.0, 10.0)
                .unit("x")
                .describe("Speed multiplier while the boost action is held."),
        )?;
        let boost_scale = kit.formula(
            "kit.example.boost_scale",
            "Speed scale from the boost button (held 0..1) and the boost param.",
            &["held", "boost"],
            FormulaJson::Expr("mix(1, boost, held)".to_string()),
        )?;
        let boost_action = kit.action(
            ActionDef::button("boost", &["key:Space", "pad:button:0"])?
                .describe("Spin faster while held."),
        );

        let spin_falloff = kit.formula(
            "kit.example.spin_falloff",
            "Speed factor from a spinner's own rate (0.5..1.5); identity by default.",
            &["rate"],
            FormulaJson::Curve(Curve::new(&[[0.5, 0.5], [1.5, 1.5]]).map_err(|e| e.to_string())?),
        )?;
        kit.world
            .res_mut::<Formulas>()
            .set_ranges(spin_falloff, Some([0.5, 1.5]), Some([0.0, 3.0]));

        let static_world = kit.param_bool(
            ParamDef::bool("kit.example.static_world", false).describe(
                "Geometry check: hold the spinners still and draw only the static world (no spinners or their shadow discs), so overlap and flicker captures see static objects only.",
            ),
        )?;

        kit.data_file("example.level", LEVEL_JSON);
        // Values tuned live and written by bake.export (params.json,
        // curves/*.json, slots/*.expr in this crate).
        kit.baked_files(BAKED);
        kit.resource(ExampleKit {
            spin_speed,
            count,
            boost,
            boost_action,
            boost_scale,
            spin_falloff,
            static_world,
        });
        kit.resource(SpinClock::default());
        kit.system(SystemDesc::new("example.spin", Stage::Update, spin_system));
        kit.scene(MAIN_SCENE, || Box::new(MainScene));
        Ok(())
    }

    fn start_scene(&self) -> Option<&'static str> {
        Some(MAIN_SCENE)
    }

    /// Free roam (docs/PLAYGROUNDS.md): the example has no goals, timer,
    /// lives or game over, so it is the same in both modes. A game grown from
    /// it keeps this true: read `PlayMode` (evani_core::play) and skip its
    /// goals and fail state while `FreeRoam`, and report fail events in
    /// `PlayOutcomes` so the free-roam test catches them.
    fn free_roam(&self) -> bool {
        true
    }
}

/// Turn the spinners: set the shared [`SpinClock`]'s rate from the speed
/// param and the boost (zero in the still world). The poses themselves are
/// worked out from the clock wherever they are drawn ([`spinner_angles`]).
/// Deterministic: fixed dt, sim time only.
pub fn spin_system(world: &mut World) {
    let Some(k) = world.try_res::<ExampleKit>().map(|k| *k) else {
        return;
    };
    let t = world.res::<Time>().sim_seconds;
    let (speed, boost, still) = {
        let reg = world.res::<ParamRegistry>();
        (reg.f32(k.spin_speed), reg.f32(k.boost), reg.bool(k.static_world))
    };
    let held = if world.res::<Input>().down(k.boost_action) {
        1.0
    } else {
        0.0
    };
    let scale = world.res::<Formulas>().eval(k.boost_scale, &[held, boost]);
    let rate = if still { 0.0 } else { f64::from(speed * scale) };
    world.res_mut::<SpinClock>().set_rate(t, rate);
}

/// Every spinner's angle at sim time `t`, in entity order.
pub fn spinner_angles(world: &World, t: f64) -> Vec<f32> {
    let (Some(k), Some(clock)) = (world.try_res::<ExampleKit>().map(|k| *k), world.try_res::<SpinClock>().map(|c| *c)) else {
        return Vec::new();
    };
    let turned = clock.turned(t);
    let formulas = world.res::<Formulas>();
    let mut out = Vec::new();
    world.query::<&Spinner>().for_each(|_, s| {
        out.push(s.angle_at(formulas.eval(k.spin_falloff, &[s.rate]), turned));
    });
    out
}

struct MainScene;

impl Scene for MainScene {
    fn name(&self) -> &str {
        MAIN_SCENE
    }

    fn on_enter(&mut self, world: &mut World) {
        let Some(k) = world.try_res::<ExampleKit>().map(|k| *k) else {
            return;
        };
        let n = world.res::<ParamRegistry>().i32(k.count).max(0);
        for _ in 0..n {
            let (angle, rate) = {
                let mut rng = world.res_mut::<Pcg32>();
                (
                    rng.range_f32(0.0, core::f32::consts::TAU),
                    rng.range_f32(0.5, 1.5),
                )
            };
            world.spawn_with(Spinner { angle, rate });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    /// Every spinner's angle now.
    fn angles(app: &App) -> Vec<f32> {
        spinner_angles(&app.world, app.world.res::<Time>().sim_seconds)
    }

    #[test]
    fn registers_tunables_and_spawns() {
        let mut a = App::new("t").with_kit(ExampleKitPlugin);
        a.run_headless(3, 1000.0 / 60.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "kit.example.spin_speed",
            "kit.example.count",
            "kit.example.boost",
        ] {
            let id = reg.id(n).unwrap();
            assert_eq!(reg.def(id).unwrap().owner, "kit.example", "{n}");
        }
        drop(reg);
        assert!(
            a.world
                .res::<Formulas>()
                .id("kit.example.boost_scale")
                .is_some()
        );
        assert!(a.world.res::<Input>().action_id("boost").is_some());
        assert_eq!(angles(&a).len(), 64);
    }

    #[test]
    fn deterministic_for_same_seed() {
        let run = || {
            let mut a = App::new("t").with_kit(ExampleKitPlugin);
            a.run_headless(30, 1000.0 / 60.0);
            angles(&a)
        };
        assert_eq!(run(), run());
    }

    /// The review dump (docs/KIT_PROCESS.md, "Kit cooperation"): the
    /// spinners as the sandbox draws them (400, its count), each with the gap
    /// between its lowest drawn vertex and the ground, measured from the real
    /// mesh. Every spinner has a contact shadow: the shadow map from Medium
    /// up, a shadow disc under it on Low (`place::Parts::disc`). The sandbox
    /// has no racers or units, so no colour palette. Written to
    /// `$EVANI_REVIEW_DUMP/review.json` when that is set.
    #[test]
    fn review_dump() {
        let mut a = App::new("review").with_kit(ExampleKitPlugin);
        a.context("review").baked_params(r#"{"kit.example.count":400}"#);
        a.run_headless(30, 1000.0 / 60.0);
        let now = angles(&a);
        let mut spinners = Vec::new();
        a.world.query::<&Spinner>().for_each(|_, s| spinners.push(*s));
        for (s, angle) in spinners.iter_mut().zip(now) {
            s.angle = angle;
        }
        assert_eq!(spinners.len(), 400);
        let side = place::side(spinners.len());
        let mut objects = Vec::new();
        for (i, s) in spinners.iter().enumerate() {
            let p = place::parts(i as u32, side, s, 0.0);
            let mut low = place::lowest_point(&p.body).expect("built-in mesh");
            if let Some(r) = p.ring {
                low = low.min(place::lowest_point(&r).expect("built-in mesh"));
            }
            // The shadow disc sits on the ground right under the spinner.
            let disc = p.disc.position;
            assert!((disc[0] - p.body.position[0]).abs() < 1e-6 && (disc[2] - p.body.position[2]).abs() < 1e-6);
            let (col, row) = place::cell(i as u32, side);
            let kind = match p.shape {
                place::Shape::Cube => "cube",
                place::Shape::Orb => "orb",
            };
            objects.push(serde_json::json!({
                "name": format!("{kind} spinner {col},{row}{}", if p.glows { " (glowing)" } else { "" }),
                "gap": (f64::from(low) * 1000.0).round() / 1000.0,
                "contact_shadow": true,
            }));
            assert!((0.0..=0.05).contains(&low), "spinner {i}: gap {low}");
        }
        let dump = serde_json::json!({ "colours": null, "objects": objects });
        if let Ok(dir) = std::env::var("EVANI_REVIEW_DUMP") {
            std::fs::create_dir_all(&dir).expect("dump folder");
            let path = std::path::Path::new(&dir).join("review.json");
            std::fs::write(&path, serde_json::to_string_pretty(&dump).expect("json")).expect("write review.json");
            eprintln!("review_dump: wrote {}", path.display());
        }
    }

    #[test]
    fn the_still_world_holds_the_spinners() {
        let mut a = App::new("t").with_kit(ExampleKitPlugin);
        a.run_headless(3, 1000.0 / 60.0);
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("kit.example.static_world", evani_core::params::ParamValue::Bool(true))
            .unwrap();
        a.run_headless(1, 1000.0 / 60.0);
        let before = angles(&a);
        a.run_headless(10, 1000.0 / 60.0);
        assert_eq!(angles(&a), before, "held still");
    }

    /// A capture sets the sim clock and draws: the pose must depend on that
    /// time only, not on how long the game ran before.
    #[test]
    fn the_pose_is_a_pure_function_of_sim_time() {
        let at = |ticks: u32| {
            let mut a = App::new("t").with_kit(ExampleKitPlugin);
            a.run_headless(ticks, 1000.0 / 60.0);
            spinner_angles(&a.world, 1.25)
        };
        let (early, late) = (at(120), at(600));
        assert_eq!(early.len(), 64);
        for (x, y) in early.iter().zip(&late) {
            assert!((x - y).abs() < 1e-4, "{x} vs {y}");
        }
        // And they do turn: a second later every spinner has moved.
        let mut a = App::new("t").with_kit(ExampleKitPlugin);
        a.run_headless(120, 1000.0 / 60.0);
        let later = spinner_angles(&a.world, 2.25);
        assert!(early.iter().zip(&later).all(|(x, y)| (x - y).abs() > 1e-3));
    }

    /// Free roam: two minutes with the boost (Space) pressed now and then,
    /// with no game over, timer expiry, lost life or error, and the world
    /// still there at the end (docs/PLAYGROUNDS.md).
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{free_roam_test, PlayMode, ScriptKey};
        let mut app = App::new("t")
            .with_play_mode(PlayMode::FreeRoam)
            .with_kit(ExampleKitPlugin);
        let mut script = Vec::new();
        for i in 0..20 {
            let at = 2.0 + f64::from(i) * 5.0;
            script.push(ScriptKey::down(at, "Space"));
            script.push(ScriptKey::up(at + 1.5, "Space"));
        }
        let r = free_roam_test(&mut app, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(r.sim_seconds > 119.0, "{r:?}");
        // The world is all still there and still turning.
        let before = angles(&app);
        assert_eq!(before.len(), 64);
        app.run_headless(30, 1000.0 / 60.0);
        assert_ne!(angles(&app), before);
    }

    #[test]
    fn boost_formula_defaults_to_one_when_released() {
        let mut a = App::new("t").with_kit(ExampleKitPlugin);
        a.start();
        let f = a.world.res::<Formulas>();
        let id = f.id("kit.example.boost_scale").unwrap();
        assert!((f.eval(id, &[0.0, 3.0]) - 1.0).abs() < 1e-6);
        assert!((f.eval(id, &[1.0, 3.0]) - 3.0).abs() < 1e-6);
    }
}
