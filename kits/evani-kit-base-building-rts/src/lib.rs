//! `evani-kit-base-building-rts`: "Cinder Ridge", a one-versus-one
//! base-building strategy skirmish (greybox). The design contract is
//! `kits/base-building-rts/SPEC.md`.
//!
//! * [`art`]: the art as data (`data/art.json`): palette, material roles
//!   asked of whatever look is active, and part recipes.
//! * [`data`]: the map and unit/building types from `data/*.json`.
//! * [`grid`]: the 2 m grid, shared flow fields and fog of war.
//! * [`sim`]: the lockstep-safe match; every change is a [`sim::Command`].
//! * [`ai`]: the rival's build-and-attack brain (pushes commands too).
//! * [`control`]: camera, selection and the player's commands (local UI).
//! * [`draw`]: the battlefield drawn through [`art`], and the in-canvas HUD.
//!
//! Systems, one fixed 60 Hz tick: `control` (0), `ai` (5), `sim` (10);
//! drawing runs in `Stage::Frame`.

pub mod ai;
pub mod art;
pub mod control;
pub mod data;
pub mod draw;
pub mod feather;
pub mod grid;
pub mod sim;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::input::{ActionDef, ActionId};
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::play::{PlayMode, PlayOutcomes};
use evani_core::plugin::{KitPlugin, PluginContext, PluginError};
use evani_core::time::Time;
use evani_core::touch::{Anchor, Button, Stick, TouchLayout};

use crate::control::{Phase, Ui};
use crate::data::{MapData, Types, BUILDING_STATS, UNIT_STATS};
use crate::sim::Match;

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

macro_rules! tunables {
    ($( $field:ident = $def:expr, [$min:expr, $max:expr], $unit:literal, $desc:literal; )*) => {
        /// Handles of every `f32` tunable.
        #[derive(Debug, Clone, Copy)]
        pub struct Handles {
            $(pub $field: F32Param,)*
        }

        /// Values of every `f32` tunable for one tick (defaults = SPEC.md).
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Tune {
            $(pub $field: f32,)*
        }

        impl Default for Tune {
            fn default() -> Self {
                Self { $($field: $def,)* }
            }
        }

        impl Handles {
            fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                Ok(Self {
                    $($field: kit.param_f32(
                        ParamDef::f32(concat!("kit.base_building_rts.", stringify!($field)), $def)
                            .range($min, $max)
                            .unit($unit)
                            .describe($desc),
                    )?,)*
                })
            }

            /// Read every tunable.
            pub fn read(&self, reg: &ParamRegistry) -> Tune {
                Tune { $($field: reg.f32(self.$field),)* }
            }
        }
    };
}

tunables! {
    start_crystal = 400.0, [0.0, 100000.0], "crystal", "Crystal each side starts with.";
    pop_cap = 40.0, [1.0, 400.0], "units", "Supply cap: units plus queued units per side.";
    queue_max = 5.0, [1.0, 20.0], "units", "Longest training queue of one building.";
    gather_s = 2.5, [0.1, 30.0], "s", "Mining time for one load.";
    gather_amount = 8.0, [1.0, 200.0], "crystal", "Crystal in one worker load.";
    separation = 0.6, [0.0, 2.0], "x", "How hard units push apart when they overlap.";
    arrive_spread = 0.9, [0.0, 5.0], "m", "Arrival radius per square root of group size (groups spread out).";
    fog_ticks = 6.0, [1.0, 60.0], "ticks", "Ticks between fog of war updates.";
    chase_leash = 16.0, [2.0, 80.0], "m", "How far a unit chases an enemy it picked up by itself.";
    max_builders = 3.0, [1.0, 10.0], "workers", "Workers that speed up one building site.";
    reach = 0.6, [0.0, 4.0], "m", "Gap at which a worker can mine, build or bank.";
    ai_think_s = 1.0, [0.1, 10.0], "s", "Seconds between the rival's decisions.";
    ai_workers = 10.0, [0.0, 40.0], "units", "Workers the rival trains.";
    ai_first_wave_s = 240.0, [0.0, 3600.0], "s", "Earliest time of the rival's first attack wave.";
    ai_wave_size = 6.0, [1.0, 60.0], "units", "Army size of the first wave.";
    ai_wave_growth = 2.0, [0.0, 20.0], "units", "Extra units per later wave.";
    ai_defend_radius = 26.0, [4.0, 100.0], "m", "Player units this close to the rival hub pull its army back.";
    cam_pan_speed = 36.0, [1.0, 200.0], "m/s", "Camera pan speed at the default zoom.";
    cam_zoom_min = 22.0, [5.0, 200.0], "m", "Closest camera distance.";
    cam_zoom_max = 110.0, [10.0, 400.0], "m", "Furthest camera distance.";
    cam_zoom_start = 52.0, [5.0, 400.0], "m", "Camera distance at the start of a match.";
    cam_zoom_step = 1.15, [1.01, 3.0], "x", "Zoom factor per wheel notch or button press.";
    cam_pitch_deg = 58.0, [20.0, 89.0], "deg", "Camera tilt below the horizon.";
    cam_fov_deg = 40.0, [15.0, 90.0], "deg", "Vertical field of view.";
    cam_fog_margin = 4.0, [0.0, 200.0], "m", "How far from the nearest seen cell the camera's centre may go (keeps the view off solid fog).";
    cam_edge_margin = 10.0, [0.0, 60.0], "m", "How far inside the map edge the camera's centre stops.";
    drag_px = 8.0, [1.0, 60.0], "px", "Pointer travel that turns a click into a box drag.";
    pick_px = 18.0, [2.0, 80.0], "px", "How near a click must land to pick a unit.";
    marker_s = 0.8, [0.1, 5.0], "s", "How long a command marker shows on the ground.";
    hud_scale = 1.0, [0.5, 3.0], "x", "HUD size.";
    hud_margin = 12.0, [0.0, 80.0], "px", "HUD margin from the canvas edges.";
    minimap_px = 150.0, [64.0, 400.0], "px", "Minimap side length (scaled by hud_scale).";
}

/// Every action the kit reads.
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub pan_x: ActionId,
    pub pan_y: ActionId,
    pub zoom: ActionId,
    pub zoom_in: ActionId,
    pub zoom_out: ActionId,
    pub command: ActionId,
    pub attack: ActionId,
    pub stop: ActionId,
    pub build: ActionId,
    pub train: ActionId,
    pub train_heavy: ActionId,
    pub select_army: ActionId,
    pub home: ActionId,
    pub groups: [ActionId; 3],
    pub ctrl: ActionId,
    pub shift: ActionId,
    pub start: ActionId,
    pub pause: ActionId,
}

/// The kit's handles (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct RtsKit {
    pub tune: Handles,
    /// `kit.base_building_rts.<unit>_<stat>`, in `units.json` order.
    pub unit_stats: [[F32Param; UNIT_STATS.len()]; 3],
    /// `kit.base_building_rts.<building>_<stat>`.
    pub building_stats: [[F32Param; BUILDING_STATS.len()]; 3],
    pub actions: Actions,
    /// Edge art tunables, defaulting to `data/art.json`.
    pub edges: EdgeParams,
}

/// `kit.base_building_rts.fog_feather_cells`, `.fog_feather_strips` and
/// `.backdrop_near_scale` (defaults from `data/art.json`).
#[derive(Debug, Clone, Copy)]
pub struct EdgeParams {
    pub feather_cells: F32Param,
    pub feather_strips: F32Param,
    pub near_scale: F32Param,
}

/// This frame's edge art values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edges {
    pub feather_cells: i32,
    pub feather_strips: i32,
    pub near_scale: f32,
}

impl EdgeParams {
    pub fn read(&self, reg: &ParamRegistry) -> Edges {
        Edges {
            feather_cells: reg.f32(self.feather_cells).round().clamp(0.0, 8.0) as i32,
            feather_strips: reg.f32(self.feather_strips).round().clamp(1.0, 8.0) as i32,
            near_scale: reg.f32(self.near_scale).clamp(0.05, 1.0),
        }
    }
}

/// The map and the types as the JSON gives them (new matches start from it).
#[derive(Debug, Clone, PartialEq)]
pub struct Assets {
    pub map: MapData,
    pub types: Types,
}

/// Whether the match end has been reported to [`PlayOutcomes`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reported(pub bool);

/// The base-building RTS kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct BaseBuildingRtsKitPlugin;

fn act(kit: &mut PluginContext, def: Result<ActionDef, evani_core::input::BindingParseError>, desc: &str) -> Result<ActionId, PluginError> {
    Ok(kit.action(def.map_err(|e| e.to_string())?.describe(desc)))
}

/// Phones and tablets (README "Touch controls"): every control names an
/// action the keys drive. Taps and drags on the map come from the primary
/// pointer (tap-select, drag-box, tap-command). The touch layer has no
/// pinch, so zoom is the +/- buttons.
/// Button grid offsets (vmin from the bottom-right corner): a step of 15
/// vmin clears a normal button (13 vmin, 48 to 72 px) with a gap, so no
/// two buttons overlap.
const GRID: [f32; 3] = [4.0, 19.0, 34.0];

fn touch_layout() -> TouchLayout {
    TouchLayout::new()
        .stick(Stick::two_axis("pan", "pan_x", "pan_y").fixed().at(Anchor::BottomLeft, 6.0, 6.0).label("Pan"))
        .button(Button::tap("build", "build", "Build").at(Anchor::BottomRight, GRID[0], GRID[0]))
        .button(Button::tap("train", "train", "Train").at(Anchor::BottomRight, GRID[1], GRID[0]))
        .button(Button::tap("heavy", "train_heavy", "Heavy").at(Anchor::BottomRight, GRID[2], GRID[0]))
        .button(Button::tap("attack", "attack", "Attack").at(Anchor::BottomRight, GRID[0], GRID[1]))
        .button(Button::tap("stop", "stop", "Stop").at(Anchor::BottomRight, GRID[1], GRID[1]))
        .button(Button::tap("army", "select_army", "Army").at(Anchor::BottomRight, GRID[2], GRID[1]))
        .button(Button::tap("zoom_in", "zoom_in", "+").at(Anchor::BottomRight, GRID[0], GRID[2]))
        .button(Button::tap("zoom_out", "zoom_out", "-").at(Anchor::BottomRight, GRID[1], GRID[2]))
        .pause_at("pause", Anchor::TopRight, 3.0, 3.0)
}

impl KitPlugin for BaseBuildingRtsKitPlugin {
    fn name(&self) -> &'static str {
        "base-building-rts"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// Free roam (docs/PLAYGROUNDS.md): the same map with no win or lose,
    /// no game over and no AI attack waves; see [`sim::Match::free_roam`].
    fn free_roam(&self) -> bool {
        true
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let tune = Handles::register(kit)?;
        let map = MapData::from_json(data::MAP_JSON)?;
        let types = Types::from_json(data::UNITS_JSON)?;
        let art = art::Art::from_json(art::ART_JSON)?;

        // Every stat in units.json is a live parameter, defaulting to it.
        let mut unit_stats = [[tune.start_crystal; UNIT_STATS.len()]; 3];
        for (row, t) in unit_stats.iter_mut().zip(&types.units) {
            for ((h, name), v) in row.iter_mut().zip(UNIT_STATS).zip(t.stats) {
                let full = format!("kit.base_building_rts.{}_{name}", t.id);
                *h = kit.param_f32(ParamDef::f32(&full, v).range(0.0, 10000.0).describe("A unit stat from data/units.json."))?;
            }
        }
        let mut building_stats = [[tune.start_crystal; BUILDING_STATS.len()]; 3];
        for (row, t) in building_stats.iter_mut().zip(&types.buildings) {
            for ((h, name), v) in row.iter_mut().zip(BUILDING_STATS).zip(t.stats) {
                let full = format!("kit.base_building_rts.{}_{name}", t.id);
                *h = kit.param_f32(ParamDef::f32(&full, v).range(0.0, 10000.0).describe("A building stat from data/units.json."))?;
            }
        }

        let mut p = |name: &str, v: f32, lo: f32, hi: f32, unit: &'static str, desc: &'static str| {
            kit.param_f32(ParamDef::f32(&format!("kit.base_building_rts.{name}"), v).range(lo, hi).unit(unit).describe(desc))
        };
        let edges = EdgeParams {
            feather_cells: p("fog_feather_cells", art.fog.feather_cells, 0.0, 8.0, "cells", "Cells either side of the fog line its gradient spreads over (0 = hard edge).")?,
            feather_strips: p("fog_feather_strips", art.fog.feather_strips, 1.0, 8.0, "strips", "Gradient strips per cell along each axis (more = smoother, more draws).")?,
            near_scale: p("backdrop_near_scale", art.backdrop.near_scale, 0.05, 1.0, "x", "Height of the ridges on the camera's side of the map.")?,
        };

        let actions = Actions {
            pan_x: act(kit, ActionDef::axis("pan_x", &["keys:KeyA/KeyD", "keys:ArrowLeft/ArrowRight", "pad:axis:0"]), "Pan the camera east and west.")?,
            pan_y: act(kit, ActionDef::axis("pan_y", &["keys:KeyW/KeyS", "keys:ArrowUp/ArrowDown", "pad:axis:1"]), "Pan the camera north and south.")?,
            zoom: act(kit, ActionDef::axis("zoom", &["mouse:wheel"]), "Zoom with the wheel.")?,
            zoom_in: act(kit, ActionDef::button("zoom_in", &["key:Equal", "key:NumpadAdd"]), "Zoom in.")?,
            zoom_out: act(kit, ActionDef::button("zoom_out", &["key:Minus", "key:NumpadSubtract"]), "Zoom out.")?,
            command: act(kit, ActionDef::button("command", &["mouse:2"]), "Smart command: move, attack, gather or help build.")?,
            attack: act(kit, ActionDef::button("attack", &["key:KeyF"]), "Arm attack: the next click attacks or attack-moves.")?,
            stop: act(kit, ActionDef::button("stop", &["key:KeyX"]), "Stop the selection (a producer drops its last queued unit).")?,
            build: act(kit, ActionDef::button("build", &["key:KeyB"]), "Place a foundry, press again for a bastion, a third time to cancel.")?,
            train: act(kit, ActionDef::button("train", &["key:KeyE"]), "Train a worker (hub) or trooper (foundry).")?,
            train_heavy: act(kit, ActionDef::button("train_heavy", &["key:KeyR"]), "Train a rumbler (foundry).")?,
            select_army: act(kit, ActionDef::button("select_army", &["key:KeyG"]), "Select every trooper and rumbler.")?,
            home: act(kit, ActionDef::button("home", &["key:KeyH"]), "Centre the camera on your hub.")?,
            groups: [
                act(kit, ActionDef::button("group_1", &["key:Digit1"]), "Recall group 1 (Ctrl stores it).")?,
                act(kit, ActionDef::button("group_2", &["key:Digit2"]), "Recall group 2 (Ctrl stores it).")?,
                act(kit, ActionDef::button("group_3", &["key:Digit3"]), "Recall group 3 (Ctrl stores it).")?,
            ],
            ctrl: act(kit, ActionDef::button("ctrl", &["key:ControlLeft", "key:ControlRight"]), "Hold to store a control group.")?,
            shift: act(kit, ActionDef::button("shift", &["key:ShiftLeft", "key:ShiftRight"]), "Hold to add to the selection.")?,
            start: act(kit, ActionDef::button("start", &["key:Enter", "key:NumpadEnter", "pad:button:0"]), "Start a match.")?,
            pause: act(kit, ActionDef::button("pause", &["key:Escape", "key:KeyP", "pad:button:9"]), "Pause the match.")?,
        };
        kit.touch_controls(touch_layout())?;

        kit.data_file("base_building_rts.map", data::MAP_JSON);
        kit.data_file("base_building_rts.units", data::UNITS_JSON);
        kit.data_file("base_building_rts.art", art::ART_JSON);
        // Values tuned live and written by bake.export.
        kit.baked_files(BAKED);

        let free_roam = kit.world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
        let t = tune.read(&kit.world.res::<ParamRegistry>());
        let m = Match::new(&map, types.clone(), &t, free_roam);
        let ui = Ui::new(&m, &t, free_roam);
        kit.resource(m);
        kit.resource(ui);
        kit.resource(ai::Brain::default());
        kit.resource(Assets { map, types });
        kit.resource(Reported(false));
        kit.resource(art);
        kit.resource(draw::DrawCache::default());
        kit.resource(RtsKit { tune, unit_stats, building_stats, actions, edges });

        let fixed = |name, order, run: fn(&mut World)| SystemDesc::new(name, Stage::Update, run).order(order);
        kit.system(fixed("base_building_rts.control", 0, control::control_system));
        kit.system(fixed("base_building_rts.ai", 5, ai::ai_system));
        kit.system(fixed("base_building_rts.sim", 10, sim_system));
        kit.system(SystemDesc::new("base_building_rts.draw", Stage::Frame, draw::draw_system));
        Ok(())
    }
}

/// This tick's tunables (None before the kit is built).
pub fn tune(world: &World) -> Option<(RtsKit, Tune)> {
    let k = world.try_res::<RtsKit>().map(|k| *k)?;
    let t = k.tune.read(&world.res::<ParamRegistry>());
    Some((k, t))
}

/// A fresh match from the JSON and the live tunables.
pub fn new_match(world: &mut World) {
    let Some((_, t)) = tune(world) else { return };
    let free_roam = world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
    let fresh = world.try_res::<Assets>().map(|a| Match::new(&a.map, a.types.clone(), &t, free_roam));
    if let Some(m) = fresh {
        *world.res_mut::<Match>() = m;
        *world.res_mut::<ai::Brain>() = ai::Brain::default();
        *world.res_mut::<Reported>() = Reported(false);
    }
}

/// Step the match one fixed tick while playing (not paused), with this
/// tick's live stats; report the end once.
pub fn sim_system(world: &mut World) {
    let Some((k, t)) = tune(world) else { return };
    let playing = world.try_res::<Ui>().is_some_and(|u| u.phase == Phase::Playing && !u.paused);
    if !playing {
        return;
    }
    let dt = world.res::<Time>().fixed_dt;
    {
        let reg = world.res::<ParamRegistry>();
        let mut m = world.res_mut::<Match>();
        for (row, ty) in k.unit_stats.iter().zip(m.types.units.iter_mut()) {
            for (h, s) in row.iter().zip(ty.stats.iter_mut()) {
                *s = reg.f32(*h);
            }
        }
        for (row, ty) in k.building_stats.iter().zip(m.types.buildings.iter_mut()) {
            for (h, s) in row.iter().zip(ty.stats.iter_mut()) {
                *s = reg.f32(*h);
            }
        }
        m.step(&t, dt);
    }
    let winner = world.res::<Match>().winner;
    if let Some(w) = winner {
        let first = !world.res::<Reported>().0;
        if first {
            world.res_mut::<Reported>().0 = true;
            if let Some(mut o) = world.try_res_mut::<PlayOutcomes>() {
                o.game_over();
            }
            world.res_mut::<Ui>().finish(w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    /// Free roam: two minutes of play with the army selected, attack armed,
    /// builds and training pressed now and then: no game over, timer expiry,
    /// lost life or error, no winner, and both hubs still standing
    /// (docs/PLAYGROUNDS.md).
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{free_roam_test, PlayMode, ScriptKey};
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(BaseBuildingRtsKitPlugin);
        let keys = ["KeyE", "KeyG", "KeyF", "KeyB", "KeyX", "KeyD", "KeyS", "KeyH"];
        let mut script = Vec::new();
        for i in 0..20 {
            let at = 2.0 + f64::from(i) * 5.0;
            let key = keys[i as usize % keys.len()];
            script.push(ScriptKey::down(at, key));
            script.push(ScriptKey::up(at + 0.5, key));
        }
        let r = free_roam_test(&mut app, 120.0, &script).unwrap();
        assert_eq!(r.keys, script.len());
        assert!(r.sim_seconds > 119.0, "{r:?}");
        let m = app.world.res::<Match>();
        assert!(m.winner.is_none());
        assert!(m.hub(sim::PLAYER).is_some() && m.hub(sim::RIVAL).is_some());
        assert!(m.tick > 7000, "the match ran: {}", m.tick);
    }

    /// At the start each side sees its home crystals (hub sight) and knows
    /// the ground around its hub (start reveal); the far side stays unknown.
    #[test]
    fn start_shows_home_crystals() {
        use grid::{UNEXPLORED, VISIBLE};
        let map = MapData::from_json(data::MAP_JSON).unwrap();
        let types = Types::from_json(data::UNITS_JSON).unwrap();
        let m = Match::new(&map, types, &Tune::default(), false);
        let g = &m.grid;
        for team in [sim::PLAYER, sim::RIVAL] {
            let hub = m.hub(team).unwrap();
            let r = g.rect(hub.corner, hub.size);
            let ctr = [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5];
            let fog = &m.fog[team as usize];
            let home: Vec<_> = m.nodes.iter().filter(|n| grid::dist(n.pos, ctr) < 20.0).collect();
            assert!(home.len() >= 5, "home crystals near the hub: {}", home.len());
            assert!(home.iter().all(|n| fog.at(g, n.cell) == VISIBLE), "team {team} sees its home crystals");
            let near = g.cell_of([ctr[0] + map.start_reveal_m - 2.0, ctr[1]]);
            assert_ne!(fog.at(g, near), UNEXPLORED, "team {team} knows the ground around its hub");
            let far = g.cell_of([-ctr[0], -ctr[1]]);
            assert_eq!(fog.at(g, far), UNEXPLORED, "team {team} does not know the rival's base");
        }
    }

    /// Nearest seen cell centre to `p` and its distance.
    fn nearest_seen(m: &Match, p: [f32; 2]) -> f32 {
        let g = &m.grid;
        let fog = &m.fog[sim::PLAYER as usize];
        (0..g.w * g.h)
            .filter(|i| fog.cells[*i as usize] != grid::UNEXPLORED)
            .map(|i| grid::dist(g.center([i % g.w, i / g.w]), p))
            .fold(f32::MAX, f32::min)
    }

    /// The camera limit: inside the map by `cam_edge_margin`, within
    /// `cam_fog_margin` of a seen cell; seen ground is left alone.
    #[test]
    fn camera_stays_near_seen_ground() {
        let map = MapData::from_json(data::MAP_JSON).unwrap();
        let types = Types::from_json(data::UNITS_JSON).unwrap();
        let t = Tune::default();
        let m = Match::new(&map, types, &t, false);
        let h = m.grid.half();
        let e = t.cam_edge_margin - 1e-3;
        let hub = m.hub(sim::PLAYER).unwrap();
        let home = control::rect_center(m.building_rect(hub));
        assert_eq!(control::clamp_cam(&m, &t, home), home, "the hub is seen ground");
        for p in [[-1e4, -1e4], [1e4, 1e4], [0.0, -1e4], [1e4, 0.0], [home[0], home[1] - 60.0]] {
            let q = control::clamp_cam(&m, &t, p);
            assert!(q[0] >= -h[0] + e && q[0] <= h[0] - e && q[1] >= -h[1] + e && q[1] <= h[1] - e, "{p:?} -> {q:?} vs map {h:?}");
            assert!(nearest_seen(&m, q) <= t.cam_fog_margin + 1e-3, "{p:?} -> {q:?} is {} m from seen ground", nearest_seen(&m, q));
        }
    }

    /// The test pass's defect: holding W for 3 s ran the camera into solid
    /// fog. Now it stops near the seen ground (and did move there).
    #[test]
    fn holding_pan_north_stops_at_the_fog() {
        use evani_core::play::{free_roam_test, PlayMode, ScriptKey};
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(BaseBuildingRtsKitPlugin);
        let script = [ScriptKey::down(1.0, "KeyW"), ScriptKey::up(4.0, "KeyW")];
        free_roam_test(&mut app, 5.0, &script).unwrap();
        let t = tune(&app.world).unwrap().1;
        let m = app.world.res::<Match>();
        let cam = app.world.res::<Ui>().cam;
        assert!(nearest_seen(&m, cam) <= t.cam_fog_margin + 1e-3, "{cam:?} is {} m from seen ground", nearest_seen(&m, cam));
        let home = control::rect_center(m.building_rect(m.hub(sim::PLAYER).unwrap()));
        assert!(grid::dist(cam, home) > 10.0, "the pan moved the camera: {cam:?} from {home:?}");
    }

    /// Minus (and the "-" button's action) moves the camera further away.
    #[test]
    fn minus_zooms_out() {
        use evani_core::play::{free_roam_test, PlayMode, ScriptKey};
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(BaseBuildingRtsKitPlugin);
        let script = [ScriptKey::down(1.0, "Minus"), ScriptKey::up(1.2, "Minus"), ScriptKey::down(1.5, "Minus"), ScriptKey::up(1.7, "Minus")];
        free_roam_test(&mut app, 2.0, &script).unwrap();
        let t = tune(&app.world).unwrap().1;
        let ui = app.world.res::<Ui>();
        assert!(ui.zoom > t.cam_zoom_start * 1.05, "zoomed out: {} from {}", ui.zoom, t.cam_zoom_start);
        let dist = |z: f32| {
            let p = control::pose(ui.cam, z, &t, 16.0 / 9.0);
            let d = [p.eye[0] - p.target[0], p.eye[1] - p.target[1], p.eye[2] - p.target[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        };
        assert!(dist(ui.zoom) > dist(t.cam_zoom_start), "the eye is further from the ground");
    }

    /// The phone's button grid: no two buttons overlap on any screen from
    /// 320 px up (sizes as `ControlSize::css`, offsets in vmin).
    #[test]
    fn touch_buttons_never_overlap() {
        use evani_core::touch::{Control, ControlSize};
        let layout = touch_layout();
        let buttons: Vec<_> = layout
            .controls
            .iter()
            .filter_map(|c| match c {
                Control::Button(b) if b.place.anchor == Anchor::BottomRight => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(buttons.len(), 8);
        for px in [320.0f32, 390.0, 430.0, 600.0, 768.0, 1080.0] {
            let v = px / 100.0;
            let size = |s: ControlSize| match s {
                ControlSize::Small => (9.0 * v).clamp(48.0, 56.0),
                ControlSize::Normal => (13.0 * v).clamp(48.0, 72.0),
                ControlSize::Primary => (18.0 * v).clamp(64.0, 88.0),
            };
            let rect = |b: &Button| {
                let (x, y, s) = (b.place.x * v, b.place.y * v, size(b.size));
                [x, y, x + s, y + s]
            };
            for (i, a) in buttons.iter().enumerate() {
                for b in &buttons[i + 1..] {
                    let (ra, rb) = (rect(a), rect(b));
                    let overlap = ra[0] < rb[2] - 0.5 && rb[0] < ra[2] - 0.5 && ra[1] < rb[3] - 0.5 && rb[1] < ra[3] - 0.5;
                    assert!(!overlap, "{} and {} overlap at {px} px: {ra:?} {rb:?}", a.id, b.id);
                }
            }
        }
    }
}
