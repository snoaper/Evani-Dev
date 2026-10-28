//! `evani-kit-kart-racer`: an arcade kart racer. Drift-charge boosts, item
//! boxes with position-weighted odds, AI racers on a racing line with
//! rubber-banding, laps with checkpoint gates and wrong-way detection, one
//! or two local players in split-screen.
//!
//! * Params (owner `kit.kart-racer`): every tunable is `kit.kart_racer.*`
//!   (see [`Tune`] and SPEC.md section 1). `kit.kart_racer.karts` is the
//!   field size; the AI count is `karts - players`.
//! * Formulas: `kit.kart_racer.steer_by_speed(speed_frac)` (curve) and
//!   `kit.kart_racer.rubber_band(gap, dist, strength)` (expression).
//! * Input actions: `p1_*` and `p2_*` (`steer`, `throttle`, `brake`,
//!   `drift`, `item`) and `pause`. With one player both sets drive kart 1.
//! * Data files: `kart_racer.track`, `kart_racer.karts`, `kart_racer.items`.
//! * Resources: [`race::Race`] (race-wide state and the race's entity lists
//!   in a fixed order; the app sends it [`race::Request`]s),
//!   [`hud::RaceHud`] (what a HUD shows), [`KartKit`] (param, formula and
//!   action handles) and [`draw::DrawState`].
//! * Components (every gameplay object is an entity): `kart_racer.kart`
//!   ([`kart::Kart`]), `kart_racer.ai` ([`ai::AiState`], on every kart),
//!   `kart_racer.controls` ([`kart::Controls`]), `kart_racer.item_box`
//!   ([`items::ItemBox`]), `kart_racer.goo`, `kart_racer.bomb`,
//!   `kart_racer.seeker`, `kart_racer.blast` (items in flight, pooled) and
//!   `kart_racer.in_flight` ([`race::InFlight`]). Karts are respawned for
//!   each new race; boxes and item pools are spawned once. Read them in
//!   order through [`race::RaceView`]. Scenery props are entities too
//!   (`kart_racer.prop`, [`scenery::Prop`], listed in order in
//!   [`scenery::Scenery`]); the static meshes are built from them.
//! * Systems (fixed `Update`, in order): `kart_racer.flow` (0),
//!   `kart_racer.control` (10), `kart_racer.karts` (20), `kart_racer.boxes`
//!   (30), `kart_racer.bumps` (40), `kart_racer.items` (50),
//!   `kart_racer.standings` (60), the stages in [`race`]; then
//!   `kart_racer.draw` (`Frame`: cameras, split-screen views, meshes,
//!   minimap, shake, FOV kick, speed-line drive, sounds).
//!
//! The kit asks the look for materials by role name and tints material 0
//! when a look lacks one, so it runs with any 3D look.

// The race rules (no drawing): these build with or without the `render`
// feature, so a headless referee carries exactly the race the game runs.
pub mod ai;
pub mod finish;
pub mod hull;
pub mod items;
pub mod kart;
#[cfg(feature = "net")]
pub mod net;
pub mod race;
pub mod slipstream;
pub mod track;
pub mod trackgeo;

// Drawing, sound, HUD and the plugin's frame systems (`render`, on by default).
#[cfg(feature = "render")]
pub mod chasecam;
#[cfg(feature = "render")]
pub mod draw;
#[cfg(feature = "render")]
pub mod finale;
#[cfg(all(feature = "live", feature = "render"))]
pub mod focus;
#[cfg(feature = "render")]
pub mod hud;
#[cfg(feature = "render")]
pub mod hud_layout;
#[cfg(feature = "render")]
pub mod itembox;
#[cfg(feature = "render")]
pub mod moments;
#[cfg(feature = "render")]
pub mod propmesh;
#[cfg(feature = "render")]
pub mod roadmarks;
#[cfg(all(feature = "models", feature = "render"))]
pub mod rivals;
#[cfg(feature = "render")]
pub mod scenery;
#[cfg(feature = "render")]
pub mod sfx;
#[cfg(feature = "render")]
pub mod threat;
#[cfg(feature = "render")]
pub mod trackmesh;
#[cfg(feature = "render")]
pub mod voices;

#[cfg(test)]
#[cfg(feature = "render")]
mod playtest;
#[cfg(test)]
#[cfg(feature = "render")]
mod review;
#[cfg(test)]
#[cfg(feature = "render")]
mod touchtest;

#[cfg(feature = "render")]
use evani_core::curve::Curve;
use evani_core::ecs::World;
#[cfg(feature = "render")]
use evani_core::ecs::{Stage, SystemDesc};
use evani_core::formulas::{FormulaId, Formulas};
#[cfg(feature = "render")]
use evani_core::formulas::FormulaJson;
#[cfg(feature = "render")]
use evani_core::input::ActionDef;
use evani_core::input::{ActionId, Input};
#[cfg(feature = "render")]
use evani_core::play::PlayMode;
use evani_core::params::{BoolParam, F32Param, I32Param, ParamDef, ParamId, ParamRegistry, ParamValue, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};
#[cfg(feature = "render")]
use evani_core::plugin::KitPlugin;
use evani_core::time::Time;
#[cfg(feature = "render")]
use evani_core::touch::{Anchor, Button, Stick, TouchLayout, Zone};

use crate::kart::Controls;
use crate::race::{Counts, Ctx, Fns, Race, Request};

include!(concat!(env!("OUT_DIR"), "/baked.rs"));

/// The race track (`Sugarpeak Loop`).
pub const TRACK_JSON: &str = include_str!("../data/track.json");
/// The roster of eight original drivers.
pub const KARTS_JSON: &str = include_str!("../data/karts.json");
/// Item odds per position band.
pub const ITEMS_JSON: &str = include_str!("../data/items.json");
/// The rival kart recipes: which model parts each rival is built from
/// (`rivals.rs`; only in games with the models module).
#[cfg(feature = "models")]
pub const RIVALS_JSON: &str = include_str!("../data/rivals.json");

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

        #[cfg(any(test, feature = "net"))]
        impl Tune {
            /// Set a tunable by its full param name; false if unknown.
            pub fn set_by_name(&mut self, name: &str, v: f32) -> bool {
                $(if name == concat!("kit.kart_racer.", stringify!($field)) {
                    self.$field = v;
                    return true;
                })*
                false
            }
        }

        impl Handles {
            #[cfg_attr(not(feature = "render"), allow(dead_code))]
            fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                Ok(Self {
                    $($field: kit.param_f32(
                        ParamDef::f32(concat!("kit.kart_racer.", stringify!($field)), $def)
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
    max_speed = 26.0, [5.0, 80.0], "m/s", "Top speed on the road.";
    accel = 14.0, [1.0, 80.0], "m/s2", "Acceleration with the throttle held.";
    brake = 30.0, [1.0, 120.0], "m/s2", "Braking deceleration.";
    reverse_speed = 8.0, [0.0, 30.0], "m/s", "Top speed in reverse.";
    drag = 0.6, [0.0, 5.0], "1/s", "Coasting slow-down.";
    offroad_factor = 0.55, [0.1, 1.0], "x", "Speed cap multiplier off the road.";
    turn_rate = 2.2, [0.2, 6.0], "rad/s", "Steering yaw rate at full lock.";
    drift_min_speed = 10.0, [0.0, 40.0], "m/s", "Slowest speed a drift can start at.";
    hop_height = 0.35, [0.0, 2.0], "m", "Hop when a drift starts.";
    drift_turn = 1.6, [0.2, 5.0], "rad/s", "Yaw rate while drifting.";
    drift_steer = 0.8, [0.0, 1.0], "x", "How much steering tightens or widens a drift.";
    drift_slip_deg = 22.0, [0.0, 60.0], "deg", "Body slip angle drawn while drifting.";
    drift_charge_rate = 1.0, [0.1, 5.0], "1/s", "Drift charge per second (scaled by steer tightness).";
    drift_tier1 = 0.8, [0.1, 5.0], "s", "Charge for blue sparks (tier 1).";
    drift_tier2 = 1.6, [0.2, 6.0], "s", "Charge for orange sparks (tier 2).";
    drift_tier3 = 2.6, [0.3, 8.0], "s", "Charge for pink sparks (tier 3).";
    boost_tier1_s = 0.6, [0.0, 4.0], "s", "Boost from a tier 1 drift.";
    boost_tier2_s = 1.0, [0.0, 4.0], "s", "Boost from a tier 2 drift.";
    boost_tier3_s = 1.5, [0.0, 4.0], "s", "Boost from a tier 3 drift.";
    boost_speed_mult = 1.35, [1.0, 2.5], "x", "Speed cap multiplier while boosting.";
    boost_accel = 40.0, [1.0, 150.0], "m/s2", "Acceleration while boosting.";
    kart_radius = 1.1, [0.3, 3.0], "m", "Kart collision radius.";
    bump_restitution = 0.5, [0.0, 1.0], "", "Bounciness of kart bumps.";
    verge_width = 7.0, [0.0, 30.0], "m", "Grass between the road edge and the wall.";
    wall_speed_loss = 0.4, [0.0, 1.0], "x", "Speed lost hitting the wall.";
    spinout_s = 1.1, [0.1, 5.0], "s", "Spin-out time when hit by an item.";
    gravity = 9.81, [0.0, 40.0], "m/s2", "Gravity when the physics module has none.";
    gravity_scale = 1.8, [0.1, 5.0], "x", "Arcade gravity multiplier (snappier hops).";
    item_respawn_s = 3.0, [0.1, 30.0], "s", "Item box respawn time.";
    roulette_s = 1.2, [0.0, 5.0], "s", "Item roulette spin before the item is ready.";
    zoom_boost_s = 1.2, [0.1, 5.0], "s", "Zoom Pop boost time.";
    goo_lifetime_s = 20.0, [1.0, 120.0], "s", "How long a Goo Blob stays on the track.";
    bomb_speed = 34.0, [5.0, 100.0], "m/s", "Boing Bomb throw speed.";
    bomb_fuse_s = 2.5, [0.3, 10.0], "s", "Boing Bomb fuse.";
    bomb_radius = 4.0, [0.5, 15.0], "m", "Boing Bomb blast radius.";
    seeker_speed = 40.0, [5.0, 120.0], "m/s", "Seeker Star speed.";
    shield_s = 8.0, [0.5, 30.0], "s", "Shield Bubble time.";
    racing_line_inset = 0.55, [0.0, 0.9], "x", "How far the racing line cuts towards the inside of corners (fraction of half width).";
    ai_lookahead = 11.0, [2.0, 40.0], "m", "How far ahead AI drivers aim on their line.";
    ai_steer_gain = 2.4, [0.2, 10.0], "", "AI steering gain on the heading error.";
    ai_drift_curvature = 0.035, [0.0, 0.2], "1/m", "Corner curvature (next 25 m) where AI drivers drift.";
    rubber_band_distance = 60.0, [1.0, 500.0], "m", "Gap at which rubber-banding is at full strength.";
    rubber_band_strength = 0.12, [0.0, 0.5], "x", "Most an AI's speed cap changes from rubber-banding.";
    ai_skill_spread = 0.06, [0.0, 0.3], "x", "AI speed cap spread by skill.";
    wrong_way_s = 1.0, [0.1, 5.0], "s", "Driving backwards this long shows WRONG WAY.";
    countdown_s = 3.0, [0.5, 10.0], "s", "Countdown before GO.";
    start_boost_window_s = 0.5, [0.05, 2.0], "s", "Start boost: press the throttle this late before GO (and hold it) for a boost.";
    start_boost_s = 0.8, [0.0, 3.0], "s", "Start boost time.";
    cam_distance = 7.0, [2.0, 30.0], "m", "Chase camera distance behind the kart.";
    cam_height = 3.0, [0.5, 20.0], "m", "Chase camera height.";
    cam_look_ahead = 4.0, [0.0, 30.0], "m", "Chase camera look-at point ahead of the kart.";
    cam_smooth = 8.0, [0.5, 40.0], "1/s", "Chase camera follow smoothing.";
    cam_fov = 62.0, [30.0, 110.0], "deg", "Chase camera vertical field of view.";
    // The item box's look (draw only; pick-up and respawn are gameplay above).
    item_box_size = 1.8, [0.3, 5.0], "m", "Item box edge length (looks only; the pick-up radius is unchanged).";
    item_box_lift = 0.7, [0.0, 3.0], "m", "How high the box floats over its pick-up point.";
    item_box_bob_m = 0.2, [0.0, 1.5], "m", "How far the box bobs up and down.";
    item_box_bob_speed = 2.0, [0.0, 12.0], "rad/s", "How fast the box bobs.";
    item_box_spin_speed = 1.5, [0.0, 12.0], "rad/s", "How fast the box turns.";
    item_box_tilt = 0.4, [0.0, 3.0], "x", "How far the spin axis leans off upright (0 = turns flat like a top; low keeps the question marks upright).";
    item_box_color_cycle_s = 4.0, [0.0, 60.0], "s", "Seconds each box shows one palette colour before it fades to the next (0 = every box keeps its colour).";
    item_box_saturation = 1.0, [0.0, 2.0], "x", "Box colour strength (0 = grey, 1 = as the palette, 2 = extra vivid).";
    item_box_brightness = 1.0, [0.0, 3.0], "x", "Brightness of the box colour.";
    item_box_bevel = 0.12, [0.02, 0.2], "x", "How rounded the box edges are (edge bevel, as a fraction of the box).";
    item_box_glyph_size = 1.0, [0.0, 1.3], "x", "Size of the question mark on each face (0 = none; it never grows past the flat face).";
    item_box_glyph_glow = 0.5, [0.0, 3.0], "x", "Glow of the white question marks and rivets: 1 = the bloom the old ribbons had, 0.5 = half, 0 = no bloom.";
    item_box_rivet_size = 0.045, [0.0, 0.08], "x", "Radius of the four rivets on each face, as a fraction of the box (0 = no rivets).";
    item_box_pop_s = 0.4, [0.05, 3.0], "s", "Grow-in time when a box comes back.";
    item_box_shadow_radius = 0.62, [0.0, 2.0], "x", "Shadow disc radius (tiers without a shadow map), as a fraction of the box.";
    item_box_shadow_shade = 0.45, [0.0, 1.0], "x", "Shadow disc brightness (0 = black, 1 = no shadow).";
    // The finale close-up: the podium camera holds the wide shot, then eases in on the winners.
    finale_cam_hold_s = 0.8, [0.0, 10.0], "s", "Seconds the podium camera holds the wide shot before the close-up move starts.";
    finale_cam_move_s = 2.6, [0.1, 15.0], "s", "Seconds the podium camera takes to ease from the wide shot into the winners close-up.";
    finale_cam_distance = 8.5, [3.0, 30.0], "m", "Close-up camera distance in front of the podium.";
    finale_cam_azimuth = 0.28, [-1.5, 1.5], "rad", "Close-up camera turn away from straight in front of the podium.";
    finale_cam_height = 2.4, [0.2, 15.0], "m", "Close-up camera height over the meadow.";
    finale_cam_look_y = 3.4, [0.2, 15.0], "m", "Height over the meadow of the point the close-up looks at (the winners).";
    finale_cam_fov = 44.0, [20.0, 90.0], "deg", "Close-up camera vertical field of view.";
    boost_fov_kick = 12.0, [0.0, 30.0], "deg", "Extra field of view while boosting (at least the speed-lines effect's own kick).";
    // Rival karts built from model parts (rivals.rs; needs the models module). Draw only.
    rival_model_scale = 1.0, [0.5, 2.0], "x", "Size of a rival kart drawn from model parts, as a multiple of the built size (looks only; the collision radius is kart_radius).";
    shadow_soften = 1.0, [0.0, 2.0], "x", "Least shadow filter radius on tiers with a shadow map (0 = each tier's own; 1 = 3x3 taps, so Medium's single tap shows no stair steps; 2 = 5x5). Looks only.";
}

/// The item box question marks' and rivets' colour
/// (`kit.kart_racer.item_box_glyph_color`); [`Tune::item_box_glyph_glow`]
/// sets how bright it shines.
pub const ITEM_BOX_GLYPH_COLOR: [f32; 3] = [1.0, 1.0, 1.0];

/// The item box palette (`kit.kart_racer.item_box_color_1` to `_6`, linear):
/// solid candy colours, saturated but friendly, from the look bible's
/// family: apple green #2fc46a, grape #9b4dff, blueberry #3557d6, cherry
/// #f1506c, lemon #ffd23f and mint teal #18c7c0. Neighbouring boxes always
/// show different ones.
pub const ITEM_BOX_PALETTE: [[f32; 3]; 6] = [
    [0.028, 0.552, 0.144],
    [0.328, 0.074, 1.0],
    [0.036, 0.095, 0.672],
    [0.88, 0.08, 0.15],
    [1.0, 0.644, 0.05],
    [0.009, 0.571, 0.527],
];

/// Handles the kit's systems read every tick (no string lookups in the loop).
#[derive(Debug, Clone, Copy)]
pub struct KartKit {
    pub tune: Handles,
    pub karts: I32Param,
    pub laps: I32Param,
    pub checkpoints: I32Param,
    /// `kit.kart_racer.item_box_glyph_color`.
    pub item_box_glyph_color: Vec3Param,
    /// `kit.kart_racer.item_box_color_1` to `_6`.
    pub item_box_palette: [Vec3Param; 6],
    pub steer_by_speed: FormulaId,
    pub rubber_band: FormulaId,
    pub actions: Actions,
    /// `physics.gravity` when the physics module is in the build.
    pub physics_gravity: Option<ParamId>,
    /// `kit.kart_racer.static_world`: the geometry check's still world (no
    /// karts, items or effects drawn, the race held).
    pub static_world: BoolParam,
    /// `kit.kart_racer.rival_parts`: draw rival karts from their recipes'
    /// model parts (the models module; off draws the primitive kart).
    #[cfg(feature = "models")]
    pub rival_parts: BoolParam,
}

/// Input actions of one player.
#[derive(Debug, Clone, Copy)]
pub struct PlayerActions {
    pub steer: ActionId,
    pub throttle: ActionId,
    pub brake: ActionId,
    pub drift: ActionId,
    pub item: ActionId,
}

#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub players: [PlayerActions; 2],
    pub pause: ActionId,
}

impl KartKit {
    /// Integer settings.
    pub fn counts(&self, reg: &ParamRegistry) -> Counts {
        Counts {
            karts: reg.i32(self.karts).clamp(1, 12) as usize,
            laps: reg.i32(self.laps).clamp(1, 20) as u32,
            checkpoints: reg.i32(self.checkpoints).clamp(1, 32) as u32,
        }
    }

    /// Gravity magnitude for hops and bombs: `physics.gravity` if the
    /// physics module registered it, else the kit's own, times the scale.
    pub fn gravity(&self, reg: &ParamRegistry, t: &Tune) -> f32 {
        let g = match self.physics_gravity.and_then(|id| reg.value(id)) {
            Some(ParamValue::Vec3(v)) => v[1].abs(),
            _ => t.gravity,
        };
        g * t.gravity_scale
    }
}

/// The kart racer kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct KartRacerPlugin;

#[cfg(feature = "render")]
fn player_actions(
    kit: &mut PluginContext,
    p: usize,
    keys: [&str; 5],
) -> Result<PlayerActions, PluginError> {
    let pad = format!("pad{}", p + 1);
    let name = |a: &str| format!("p{}_{a}", p + 1);
    let stick = format!("{pad}:axis:0");
    let steer = kit.action(
        ActionDef::axis(&name("steer"), &[keys[0], &stick])
            .map_err(|e| e.to_string())?
            .describe("Steer left and right."),
    );
    let throttle = kit.action(
        ActionDef::button(&name("throttle"), &[keys[1], &format!("{pad}:button:7"), &format!("{pad}:button:0")])
            .map_err(|e| e.to_string())?
            .describe("Accelerate."),
    );
    let brake = kit.action(
        ActionDef::button(&name("brake"), &[keys[2], &format!("{pad}:button:6"), &format!("{pad}:button:1")])
            .map_err(|e| e.to_string())?
            .describe("Brake, then reverse."),
    );
    let drift = kit.action(
        ActionDef::button(&name("drift"), &[keys[3], &format!("{pad}:button:5"), &format!("{pad}:button:4")])
            .map_err(|e| e.to_string())?
            .describe("Hold while steering to drift; release for a boost."),
    );
    let item = kit.action(
        ActionDef::button(&name("item"), &[keys[4], &format!("{pad}:button:2"), &format!("{pad}:button:3")])
            .map_err(|e| e.to_string())?
            .describe("Use the held item."),
    );
    Ok(PlayerActions {
        steer,
        throttle,
        brake,
        drift,
        item,
    })
}

/// The kart's touch layout: player 1's actions, nothing else.
#[cfg(feature = "render")]
fn touch_layout() -> TouchLayout {
    TouchLayout::new()
        .hold_while_shown("p1_throttle")
        .stick(Stick::horizontal("steer", "p1_steer").dynamic(Zone::LEFT_HALF).at(Anchor::BottomLeft, 8.0, 16.0).label("Steer"))
        .button(Button::hold("drift", "p1_drift", "Drift").primary().at(Anchor::BottomRight, 4.0, 6.0))
        .button(Button::tap("item", "p1_item", "Item").at(Anchor::BottomRight, 28.0, 4.0))
        .button(Button::hold("brake", "p1_brake", "Brake").at(Anchor::BottomRight, 6.0, 30.0))
        .pause_at("pause", Anchor::TopRight, 26.0, 3.0)
}

#[cfg(feature = "render")]
impl KitPlugin for KartRacerPlugin {
    fn name(&self) -> &'static str {
        "kart-racer"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    /// Dev builds only (`live`): the item box, a kart, goo, bomb and
    /// seeker for live asset focus.
    #[cfg(feature = "live")]
    fn focus_assets(&self, assets: &mut evani_core::focus::FocusAssets) {
        focus::register(assets);
    }

    /// Free roam (docs/PLAYGROUNDS.md): under `PlayMode::FreeRoam` the
    /// player drives the track alone: no countdown, rivals, laps, timer,
    /// places or finish; see [`race::Race::free_roam`].
    fn free_roam(&self) -> bool {
        true
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let tune = Handles::register(kit)?;
        let karts = kit.param_i32(
            ParamDef::i32("kit.kart_racer.karts", 8)
                .range(1.0, 12.0)
                .describe("Karts in a race (AI racers = karts - players)."),
        )?;
        let laps = kit.param_i32(
            ParamDef::i32("kit.kart_racer.laps", 3)
                .range(1.0, 20.0)
                .describe("Laps per race."),
        )?;
        let checkpoints = kit.param_i32(
            ParamDef::i32("kit.kart_racer.checkpoints", 8)
                .range(1.0, 32.0)
                .describe("Checkpoint gates per lap (gate 0 is the start line)."),
        )?;
        let item_box_glyph_color = kit.param_vec3(
            ParamDef::color("kit.kart_racer.item_box_glyph_color", ITEM_BOX_GLYPH_COLOR)
                .describe("Colour of the question marks and rivets on the item boxes (item_box_glyph_glow sets how bright they shine)."),
        )?;
        const PALETTE_NAMES: [&str; 6] = [
            "kit.kart_racer.item_box_color_1",
            "kit.kart_racer.item_box_color_2",
            "kit.kart_racer.item_box_color_3",
            "kit.kart_racer.item_box_color_4",
            "kit.kart_racer.item_box_color_5",
            "kit.kart_racer.item_box_color_6",
        ];
        let mut item_box_palette = Vec::with_capacity(6);
        for (name, c) in PALETTE_NAMES.iter().zip(ITEM_BOX_PALETTE) {
            item_box_palette.push(kit.param_vec3(
                ParamDef::color(name, c).describe("One of the six item box colours (boxes fade through them in turn, see item_box_color_cycle_s)."),
            )?);
        }
        let item_box_palette: [Vec3Param; 6] = item_box_palette.try_into().map_err(|_| "item box palette".to_string())?;
        let static_world = kit.param_bool(
            ParamDef::bool("kit.kart_racer.static_world", false).describe(
                "Geometry check: hold the race still and draw only the static world (no karts, items or their effects), so overlap and flicker captures see static objects only.",
            ),
        )?;
        #[cfg(feature = "models")]
        let rival_parts = kit.param_bool(
            ParamDef::bool("kit.kart_racer.rival_parts", true).describe(
                "Draw rival karts from their recipes' model parts (data/rivals.json); off draws every rival as the primitive kart. Draw only.",
            ),
        )?;
        let steer_by_speed = kit.formula(
            "kit.kart_racer.steer_by_speed",
            "Steering factor from the speed fraction (speed / max_speed; above 1 while boosting).",
            &["speed_frac"],
            FormulaJson::Curve(
                Curve::new(&[[0.0, 0.25], [0.2, 0.8], [0.5, 1.0], [1.0, 0.85], [1.4, 0.75]])
                    .map_err(|e| e.to_string())?,
            ),
        )?;
        kit.world
            .res_mut::<Formulas>()
            .set_ranges(steer_by_speed, Some([0.0, 1.4]), Some([0.0, 1.5]));
        let rubber_band = kit.formula(
            "kit.kart_racer.rubber_band",
            "AI speed cap change from its gap (m) behind the best human, the full-strength distance and the strength.",
            &["gap", "dist", "strength"],
            FormulaJson::Expr("clamp(gap / dist, -1, 1) * strength".to_string()),
        )?;

        let p1 = player_actions(
            kit,
            0,
            ["keys:KeyA/KeyD", "key:KeyW", "key:KeyS", "key:ShiftLeft", "key:KeyE"],
        )?;
        let p2 = player_actions(
            kit,
            1,
            [
                "keys:ArrowLeft/ArrowRight",
                "key:ArrowUp",
                "key:ArrowDown",
                "key:ShiftRight",
                "key:Enter",
            ],
        )?;
        let pause = kit.action(
            ActionDef::button("pause", &["key:Escape", "key:KeyP", "pad:button:9"])
                .map_err(|e| e.to_string())?
                .describe("Pause the race."),
        );

        // On-screen controls for phones and tablets (README "Touch controls"):
        // they only set the actions above. Auto-accelerate while they are
        // up (see `ctx` for the brake), steer with the left thumb, drift and
        // item under the right, pause clear of the timer, minimap and speed.
        kit.touch_controls(touch_layout())?;

        kit.data_file("kart_racer.track", TRACK_JSON);
        kit.data_file("kart_racer.karts", KARTS_JSON);
        kit.data_file("kart_racer.items", ITEMS_JSON);
        // Values tuned live and written by bake.export (params.json,
        // curves/*.json, slots/*.expr in this crate).
        kit.baked_files(BAKED);

        let track = track::Track::from_json(TRACK_JSON)?;
        let odds = items::OddsTable::from_json(ITEMS_JSON)?;
        let roster = ai::roster_from_json(KARTS_JSON)?;
        let physics_gravity = kit.world.res::<ParamRegistry>().id("physics.gravity");
        let handles = KartKit {
            tune,
            karts,
            laps,
            checkpoints,
            item_box_glyph_color,
            item_box_palette,
            steer_by_speed,
            rubber_band,
            actions: Actions {
                players: [p1, p2],
                pause,
            },
            physics_gravity,
            static_world,
            #[cfg(feature = "models")]
            rival_parts,
        };
        // Baked values are applied at start; the race re-reads every
        // tunable on each reset and tick.
        let (t, counts) = {
            let reg = kit.world.res::<ParamRegistry>();
            (tune.read(&reg), handles.counts(&reg))
        };
        // Gameplay objects are ECS entities (see the module docs).
        kit.component::<kart::Kart>("kart_racer.kart");
        kit.component::<ai::AiState>("kart_racer.ai");
        kit.component::<Controls>("kart_racer.controls");
        kit.component::<items::ItemBox>("kart_racer.item_box");
        kit.component::<items::Goo>("kart_racer.goo");
        kit.component::<items::Bomb>("kart_racer.bomb");
        kit.component::<items::Seeker>("kart_racer.seeker");
        kit.component::<items::Blast>("kart_racer.blast");
        kit.component::<race::InFlight>("kart_racer.in_flight");
        kit.component::<scenery::Prop>("kart_racer.prop");
        let mut race = Race::new(track, odds, roster, counts);
        // Free roam (docs/PLAYGROUNDS.md): fixed for the whole run.
        race.free_roam = kit.world.try_res::<PlayMode>().is_some_and(|m| m.is_free_roam());
        kit.resource(race);
        race::spawn_objects(kit.world, &t, counts);
        // The scenery: the full layout; the draw system swaps in the thin
        // one on small-budget tiers and follows live verge changes.
        scenery::spawn_props(kit.world, t.verge_width, false);
        kit.resource(hud::RaceHud::default());
        // HUD positions and sizes: live tunables `kit.kart_racer.hud_*`.
        let hud_layout = hud_layout::HudLayoutParams::register(kit)?;
        kit.resource(hud_layout);
        // The track is drawn from custom meshes (crate::trackmesh).
        if kit.world.try_res::<evani_render::MeshLibrary>().is_none() {
            kit.resource(evani_render::MeshLibrary::default());
        }
        kit.resource(draw::DrawState::default());
        // Rival karts from model parts (render only; the recipes are data).
        #[cfg(feature = "models")]
        {
            kit.data_file("kart_racer.rivals", RIVALS_JSON);
            let recipes = rivals::Recipes::from_json(RIVALS_JSON)?;
            recipes.check_roster(&kit.world.res::<Race>().roster)?;
            kit.resource(rivals::Rivals::new(recipes));
        }
        kit.resource(handles);
        // One fixed tick, in this order (race.rs has the stages).
        let fixed = |name, order, run: fn(&mut World)| SystemDesc::new(name, Stage::Update, run).order(order);
        kit.system(fixed("kart_racer.flow", 0, flow_system));
        kit.system(fixed("kart_racer.control", 10, control_system));
        kit.system(fixed("kart_racer.karts", 20, karts_system));
        kit.system(fixed("kart_racer.boxes", 30, boxes_system));
        kit.system(fixed("kart_racer.bumps", 40, bumps_system));
        kit.system(fixed("kart_racer.items", 50, items_system));
        kit.system(fixed("kart_racer.standings", 60, standings_system));
        kit.system(SystemDesc::new("kart_racer.draw", Stage::Frame, draw::draw_system));
        Ok(())
    }
}

fn read_controls(input: &Input, a: &PlayerActions) -> Controls {
    Controls {
        steer: input.value(a.steer).clamp(-1.0, 1.0),
        throttle: input.value(a.throttle).clamp(0.0, 1.0),
        brake: input.value(a.brake).clamp(0.0, 1.0),
        drift: input.down(a.drift),
        item: input.just_pressed(a.item),
    }
}

/// One player: either key set or pad drives kart 1.
fn merge(a: Controls, b: Controls) -> Controls {
    Controls {
        steer: (a.steer + b.steer).clamp(-1.0, 1.0),
        throttle: a.throttle.max(b.throttle),
        brake: a.brake.max(b.brake),
        drift: a.drift || b.drift,
        item: a.item || b.item,
    }
}

/// What this tick runs with (tunables, counts, gravity, the fixed dt, the
/// local players' controls) and whether the pause button was pressed. None
/// before the kit is built, and while the geometry check holds the world
/// still (`kit.kart_racer.static_world`), so no tick changes anything.
pub fn ctx(world: &World) -> Option<(KartKit, Ctx, bool)> {
    let k = world.try_res::<KartKit>().map(|k| *k)?;
    if world.res::<ParamRegistry>().bool(k.static_world) {
        return None;
    }
    // Online the race is stepped by the netcode (src/net.rs), never here.
    if world.try_res::<Race>().is_some_and(|r| r.online.is_some()) {
        return None;
    }
    let dt = world.res::<Time>().fixed_dt;
    let (t, counts, gravity) = {
        let reg = world.res::<ParamRegistry>();
        let t = k.tune.read(&reg);
        (t, k.counts(&reg), k.gravity(&reg, &t))
    };
    let touching = evani_core::touch::active(world);
    let (mut inputs, pause) = {
        let input = world.res::<Input>();
        let mut a = read_controls(&input, &k.actions.players[0]);
        if touching && a.brake > 0.05 {
            // Under touch the throttle is held for the player (auto-accelerate),
            // so the brake button takes it off; a keyboard or pad player is
            // not touching and keeps "throttle beats brake" as before.
            a.throttle = 0.0;
        }
        let b = read_controls(&input, &k.actions.players[1]);
        ([a, b], input.just_pressed(k.actions.pause))
    };
    if world.res::<Race>().players <= 1 {
        let m = merge(inputs[0], inputs[1]);
        inputs = [m, m];
    }
    let c = Ctx {
        t,
        counts,
        gravity,
        dt,
        inputs,
    };
    Some((k, c, pause))
}

/// The local player's controls this tick (player 1's keys, pad and touch,
/// merged with player 2's set as one player does offline). Online play reads
/// its input here (src/net.rs carries it to the referee).
pub fn local_controls(world: &World) -> Option<Controls> {
    let k = world.try_res::<KartKit>().map(|k| *k)?;
    let touching = evani_core::touch::active(world);
    let input = world.try_res::<Input>()?;
    let mut a = read_controls(&input, &k.actions.players[0]);
    if touching && a.brake > 0.05 {
        a.throttle = 0.0;
    }
    let b = read_controls(&input, &k.actions.players[1]);
    Some(merge(a, b))
}

/// Runs `f` with the registry's formulas as plain closures.
fn with_fns<R>(world: &World, k: &KartKit, f: impl FnOnce(&Fns) -> R) -> R {
    let formulas = world.res::<Formulas>();
    let steer = |x: f32| formulas.eval(k.steer_by_speed, &[x]);
    let rubber = |g: f32, d: f32, s: f32| formulas.eval(k.rubber_band, &[g, d, s]);
    f(&Fns {
        steer: &steer,
        rubber: &rubber,
    })
}

/// `kart_racer.flow` (order 0): the first tick's fresh attract grid, the
/// pause button, the queued requests (resets respawn the karts), then
/// [`race::flow`].
pub fn flow_system(world: &mut World) {
    let Some((_, c, pause)) = ctx(world) else {
        return;
    };
    let first = {
        let r = world.res::<Race>();
        r.race_no == 1 && r.phase_time == 0.0
    };
    if first {
        // Rebuild the attract grid with the baked values (applied after
        // build). Free roam skips the title and starts on the track.
        if world.res::<Race>().free_roam {
            race::reset_roam(world, &c.t, c.counts);
        } else {
            race::reset_mut(world, race::Phase::Title, 0, &c.t, c.counts);
        }
    }
    if pause {
        world.res_mut::<Race>().request(Request::TogglePause);
    }
    // The touch controls belong to the race itself: off on the title, the
    // pause and settings menus and the results, which are tapped on the page.
    let racing = {
        let r = world.res::<Race>();
        matches!(r.phase, race::Phase::Countdown | race::Phase::Racing) && !r.paused
    };
    evani_core::touch::set_enabled(world, racing);
    race::handle_requests(world, &c.t, c.counts);
    race::flow(world, &c);
}

/// `kart_racer.control` (order 10): [`race::control`].
pub fn control_system(world: &mut World) {
    if let Some((k, c, _)) = ctx(world) {
        with_fns(world, &k, |f| race::control(world, &c, f));
    }
}

/// `kart_racer.karts` (order 20): [`race::step_karts`].
pub fn karts_system(world: &mut World) {
    if let Some((k, c, _)) = ctx(world) {
        with_fns(world, &k, |f| race::step_karts(world, &c, f));
    }
}

/// `kart_racer.boxes` (order 30): [`race::boxes`].
pub fn boxes_system(world: &mut World) {
    if let Some((_, c, _)) = ctx(world) {
        race::boxes(world, &c);
    }
}

/// `kart_racer.bumps` (order 40): [`race::bumps`].
pub fn bumps_system(world: &mut World) {
    if let Some((_, c, _)) = ctx(world) {
        race::bumps(world, &c);
    }
}

/// `kart_racer.items` (order 50): [`race::items`].
pub fn items_system(world: &mut World) {
    if let Some((_, c, _)) = ctx(world) {
        race::items(world, &c);
    }
}

/// `kart_racer.standings` (order 60): [`race::standings`]; a finished
/// attract race starts a fresh one.
pub fn standings_system(world: &mut World) {
    let Some((_, c, _)) = ctx(world) else {
        return;
    };
    if race::standings(world, &c) {
        race::reset_mut(world, race::Phase::Title, 0, &c.t, c.counts);
    }
}

#[cfg(test)]
#[cfg(feature = "render")]
mod tests {
    use super::*;
    use crate::race::{Phase, RaceView};
    use evani_core::App;

    fn app() -> App {
        App::new("t").with_kit(KartRacerPlugin)
    }

    #[test]
    fn registers_tunables_actions_and_data() {
        let mut a = app();
        a.run_headless(3, 1000.0 / 60.0);
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "kit.kart_racer.max_speed",
            "kit.kart_racer.drift_tier3",
            "kit.kart_racer.rubber_band_strength",
            "kit.kart_racer.karts",
            "kit.kart_racer.laps",
            "kit.kart_racer.cam_fov",
        ] {
            let id = reg.id(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(reg.def(id).unwrap().owner, "kit.kart-racer", "{n}");
        }
        drop(reg);
        let f = a.world.res::<Formulas>();
        let rb = f.id("kit.kart_racer.rubber_band").unwrap();
        assert!((f.eval(rb, &[30.0, 60.0, 0.12]) - 0.06).abs() < 1e-6);
        assert!((f.eval(rb, &[-600.0, 60.0, 0.12]) + 0.12).abs() < 1e-6);
        assert!(f.id("kit.kart_racer.steer_by_speed").is_some());
        drop(f);
        let input = a.world.res::<Input>();
        for n in ["p1_steer", "p1_drift", "p2_item", "pause"] {
            assert!(input.action_id(n).is_some(), "{n}");
        }
        drop(input);
        let data = a.world.res::<evani_core::plugin::DataFiles>();
        assert!(data.get("kart_racer.track").is_some());
        drop(data);
        let race = RaceView::new(&a.world);
        assert_eq!(race.phase, Phase::Title);
        assert_eq!(race.karts.len(), 8);
        drop(race);
        let mut names = Vec::new();
        a.world.component_stats(|n, _| names.push(n));
        for n in ["kart_racer.kart", "kart_racer.ai", "kart_racer.item_box", "kart_racer.goo", "kart_racer.in_flight"] {
            assert!(names.contains(&n), "{n}: {names:?}");
        }
    }

    #[test]
    fn every_gameplay_object_is_an_entity() {
        let mut a = app();
        a.start();
        a.world.res_mut::<Race>().request(Request::Start { players: 1 });
        a.run_headless(60, 1000.0 / 60.0);
        let (karts, boxes, pooled) = {
            let r = a.world.res::<Race>();
            (r.grid.len(), r.boxes.len(), r.pools.len())
        };
        assert_eq!(karts, 8);
        assert!(boxes > 0);
        assert_eq!(a.world.count::<kart::Kart>(), karts);
        assert_eq!(a.world.count::<items::ItemBox>(), boxes);
        assert_eq!(a.world.count::<race::InFlight>(), pooled);
        // The scenery: one entity per prop of the track's layout.
        let props = a.world.res::<scenery::Scenery>().props.len();
        let layout = {
            let r = a.world.res::<Race>();
            let t = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
            scenery::layout(&r.track, t.verge_width, a.world.res::<scenery::Scenery>().thin).len()
        };
        assert!(props > 100, "{props}");
        assert_eq!((a.world.count::<scenery::Prop>(), props), (layout, layout));
        let n = a.world.entity_count();
        assert!(n >= karts + boxes + pooled + props, "{n}");
        // A restart respawns the karts without leaking entities.
        a.world.res_mut::<Race>().request(Request::Restart);
        a.run_headless(2, 1000.0 / 60.0);
        assert_eq!(a.world.entity_count(), n);
    }

    #[test]
    fn props_follow_the_tier_and_the_verge_and_build_the_statics() {
        let mut a = app();
        a.start();
        a.run_headless(1, 1000.0 / 60.0);
        let full = a.world.count::<scenery::Prop>();
        // The small-budget layout keeps fewer props; the old ones are gone.
        assert!(scenery::spawn_props(&mut a.world, 7.0, true));
        let thin = a.world.count::<scenery::Prop>();
        assert!(thin < full && thin > 0, "{thin} of {full}");
        assert!(!scenery::spawn_props(&mut a.world, 7.0, true), "same layout: no respawn");
        // The statics from the entities equal the statics from the layout.
        let mats = draw::resolve_materials(&evani_core::plugin::LookData::default());
        let r = a.world.res::<Race>();
        let (mut from_entities, mut direct) = (draw::Statics::default(), draw::Statics::default());
        draw::build_statics_with(&r.track, &mats, 7.0, &scenery::props(&a.world), true, &mut from_entities);
        draw::add_road_marks(&r.track, &mats, true, 7.0, &mut from_entities);
        draw::build_statics(&r.track, &mats, 7.0, 0, true, &mut direct);
        assert_eq!(from_entities, direct);
    }

    #[test]
    fn the_engine_systems_match_one_race_tick() {
        // The kit's systems, run by the schedule, step the race exactly like
        // race::tick (the list tests and capture moments use).
        let mut a = app();
        a.start();
        a.world.res_mut::<Race>().request(Request::Start { players: 1 });
        a.run_headless(60 * 6, 1000.0 / 60.0);
        let ticks = a.world.res::<Time>().tick;
        assert!(ticks > 60 * 5, "{ticks}");

        let mut b = app();
        b.start();
        b.world.res_mut::<Race>().request(Request::Start { players: 1 });
        let (k, c, _) = ctx(&b.world).unwrap();
        race::reset_mut(&mut b.world, Phase::Title, 0, &c.t, c.counts);
        let formulas = b.world.remove_resource::<Formulas>().unwrap();
        let steer = |x: f32| formulas.eval(k.steer_by_speed, &[x]);
        let rubber = |g: f32, d: f32, s: f32| formulas.eval(k.rubber_band, &[g, d, s]);
        let fns = Fns {
            steer: &steer,
            rubber: &rubber,
        };
        for _ in 0..ticks {
            race::tick(&mut b.world, &c, &fns);
        }
        let ka = RaceView::new(&a.world).karts;
        let kb = RaceView::new(&b.world).karts;
        assert_eq!(ka, kb);
        assert_eq!(a.world.res::<Race>().order, b.world.res::<Race>().order);
    }

    #[test]
    fn tune_defaults_match_the_registry() {
        let mut a = app();
        a.start();
        let k = *a.world.res::<KartKit>();
        let reg = a.world.res::<ParamRegistry>();
        // The defaults, with the values bake.export wrote over them.
        let mut want = Tune::default();
        if let Some((_, json)) = BAKED.iter().find(|(n, _)| *n == "params.json") {
            let baked: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json).unwrap();
            for (name, v) in baked {
                if let Some(v) = v.as_f64() {
                    want.set_by_name(&name, v as f32);
                }
            }
        }
        assert_eq!(k.tune.read(&reg), want);
    }

    #[test]
    fn deterministic_through_the_engine() {
        let run = || {
            let mut a = app();
            a.start();
            a.world.res_mut::<Race>().request(Request::Start { players: 1 });
            a.run_headless(60 * 8, 1000.0 / 60.0);
            // Grid order, whatever the storages' dense order.
            RaceView::new(&a.world).karts
        };
        assert_eq!(run(), run());
    }

    fn free_app() -> App {
        App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(KartRacerPlugin)
    }

    fn press(a: &mut App, code: &str, down: bool) {
        use evani_core::input::{KeyCode, RawEvent};
        let code = KeyCode::new(code).unwrap();
        a.world.res_mut::<Input>().handle(&RawEvent::Key { code, down });
    }

    /// Free roam (docs/PLAYGROUNDS.md): the kit says so, and the player is
    /// alone on the track and already racing, with no title and no countdown;
    /// a Play-mode app still starts at the title with the full field.
    #[test]
    fn free_roam_starts_on_the_track_alone() {
        assert!(KartRacerPlugin.free_roam());
        let mut a = free_app();
        a.start();
        a.run_headless(3, 1000.0 / 60.0);
        assert_eq!(a.play_mode(), PlayMode::FreeRoam);
        {
            let r = RaceView::new(&a.world);
            assert!(r.free_roam);
            assert_eq!((r.phase, r.players, r.karts.len()), (Phase::Racing, 1, 1));
            assert_eq!(r.karts[0].player, Some(0));
            assert!(!r.boxes.is_empty(), "item boxes stay");
        }
        let hud = a.world.res::<hud::RaceHud>();
        assert!(hud.free_roam && hud.countdown == 0 && hud.since_go > 50.0);
        drop(hud);
        // Restart and "back to the title" both mean a fresh start on the track.
        for q in [Request::Restart, Request::Title, Request::Start { players: 2 }] {
            a.world.res_mut::<Race>().request(q);
            a.run_headless(2, 1000.0 / 60.0);
            let r = RaceView::new(&a.world);
            assert_eq!((r.phase, r.players, r.karts.len()), (Phase::Racing, 1, 1), "{q:?}");
        }
        let mut p = app();
        p.start();
        p.run_headless(3, 1000.0 / 60.0);
        let r = RaceView::new(&p.world);
        assert!(!r.free_roam);
        assert_eq!((r.phase, r.karts.len()), (Phase::Title, 8));
    }

    /// No laps and no finish: the line is just paint, driving the track
    /// backwards is fine, and nothing flags wrong way.
    #[test]
    fn free_roam_has_no_laps_finish_or_wrong_way() {
        let mut a = free_app();
        a.start();
        a.run_headless(2, 1000.0 / 60.0);
        press(&mut a, "KeyW", true);
        // Just before the start line, facing it: cross it, and the checkpoint
        // gates after it, well past the point a lap would count.
        let (len, track) = {
            let r = a.world.res::<Race>();
            (r.track.length, r.track.clone())
        };
        let put = |a: &mut App, s: f32, back: bool| {
            let e = a.world.res::<Race>().grid[0];
            let mut k = a.world.get_mut::<kart::Kart>(e).unwrap();
            let tan = track.tangent_at(s);
            k.pos = track.point_at(s, 0.0);
            k.yaw = tan[0].atan2(tan[1]) + if back { std::f32::consts::PI } else { 0.0 };
            k.speed = 10.0;
            k.relocate(&track);
        };
        put(&mut a, len - 8.0, false);
        a.run_headless(60 * 6, 1000.0 / 60.0);
        {
            let r = RaceView::new(&a.world);
            let k = &r.karts[0];
            assert!(k.s < len * 0.5 || k.s > len - 8.0, "{}", k.s);
            assert_eq!((k.laps_done, k.next_gate, k.finished), (0, 0, None));
            assert_eq!(r.phase, Phase::Racing);
        }
        // Backwards for longer than wrong_way_s: no flag, no banner.
        put(&mut a, len * 0.4, true);
        a.run_headless(60 * 4, 1000.0 / 60.0);
        let r = RaceView::new(&a.world);
        assert!(!r.karts[0].wrong_way && r.karts[0].wrong_way_t == 0.0);
        assert!(!a.world.res::<hud::RaceHud>().players_hud[0].wrong_way);
    }

    /// Two minutes of scripted input in free roam: accelerate, steer, drift,
    /// use an item, drive into the walls, reverse out, with no game over,
    /// expired timer, lost life or error, and the kart still driving.
    #[test]
    fn free_roam_holds() {
        use evani_core::play::{free_roam_test, ScriptKey};
        use crate::items::{Item, Slot};
        let mut app = free_app();
        app.start();
        app.run_headless(2, 1000.0 / 60.0);
        let (e, half, verge) = {
            let r = app.world.res::<Race>();
            let k = *app.world.res::<KartKit>();
            let verge = k.tune.read(&app.world.res::<ParamRegistry>()).verge_width;
            (r.grid[0], r.track.half_width, verge)
        };
        // A Goo Blob in the slot, used one second in.
        app.world.get_mut::<kart::Kart>(e).unwrap().slot = Slot::Holding(Item::GooBlob);
        let goos = |app: &App| {
            let r = app.world.res::<Race>();
            r.pools.active::<items::Goo>(&app.world, race::Pool::Goo).len()
        };
        let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let mut keys = 0;
        let mut travelled = 0.0f32;
        let mut last = app.world.get::<kart::Kart>(e).unwrap().pos;
        let mut walled = 0;
        let mut drifted = false;
        // 8 s to use the item and check the blob is on the track, then four
        // 28 s rounds of everything.
        let opening = [ScriptKey::down(0.0, "KeyW"), ScriptKey::down(1.0, "KeyE"), ScriptKey::up(1.1, "KeyE")];
        let r = free_roam_test(&mut app, 8.0, &opening).unwrap();
        keys += r.keys;
        assert_eq!(goos(&app), 1, "the Goo Blob is on the track");
        assert_eq!(app.world.get::<kart::Kart>(e).unwrap().slot, Slot::Empty);
        for _ in 0..4 {
            let round = [
                // Steer right and back left, then a long drift and its boost.
                ScriptKey::down(0.5, "KeyD"),
                ScriptKey::up(2.0, "KeyD"),
                ScriptKey::down(3.0, "KeyA"),
                ScriptKey::up(4.5, "KeyA"),
                ScriptKey::down(5.0, "ShiftLeft"),
                ScriptKey::down(5.1, "KeyD"),
                ScriptKey::up(7.5, "ShiftLeft"),
                ScriptKey::up(7.5, "KeyD"),
                // Off the road: hard right into the wall and hold it there.
                ScriptKey::down(10.0, "KeyD"),
                ScriptKey::up(16.0, "KeyD"),
                // Brake and reverse, turn round, hard left into the other wall.
                ScriptKey::up(16.5, "KeyW"),
                ScriptKey::down(16.5, "KeyS"),
                ScriptKey::up(18.5, "KeyS"),
                ScriptKey::down(18.5, "KeyW"),
                ScriptKey::down(19.0, "KeyA"),
                ScriptKey::up(25.0, "KeyA"),
                // Items from the boxes, whenever one is held.
                ScriptKey::down(22.0, "KeyE"),
                ScriptKey::up(22.1, "KeyE"),
                ScriptKey::down(26.0, "KeyE"),
                ScriptKey::up(26.1, "KeyE"),
            ];
            for step in 0..28 {
                let r = free_roam_test(&mut app, 1.0, &script_second(&round, step)).unwrap();
                keys += r.keys;
                let r = RaceView::new(&app.world);
                let k = &r.karts[0];
                travelled += dist(last, k.pos);
                last = k.pos;
                drifted |= k.drift != 0.0;
                walled += usize::from(k.lateral.abs() > half + verge - 2.0);
                assert!(k.lateral.abs() <= half + verge + 1.0, "left the walled track: {}", k.lateral);
                assert!(k.pos.iter().all(|v| v.is_finite()));
                assert_eq!((r.phase, r.karts.len(), k.finished), (Phase::Racing, 1, None));
            }
        }
        assert!(keys > 20, "{keys}");
        assert!(travelled > 800.0, "the kart only moved {travelled} m");
        assert!(drifted, "the script never drifted");
        assert!(walled > 0, "the script never reached the wall");
        let r = RaceView::new(&app.world);
        assert_eq!((r.karts[0].laps_done, r.karts[0].next_gate), (0, 0));
        assert!(app.world.res::<Time>().sim_seconds > 119.0);
        assert_eq!(*app.world.res::<evani_core::play::PlayOutcomes>(), evani_core::play::PlayOutcomes::default());
    }

    /// The keys of `round` that fall in second `step` (0-based), re-timed to
    /// start at 0 in that second.
    fn script_second(round: &[evani_core::play::ScriptKey], step: u32) -> Vec<evani_core::play::ScriptKey> {
        let t0 = f64::from(step);
        round
            .iter()
            .filter(|k| k.at >= t0 && k.at < t0 + 1.0)
            .map(|k| evani_core::play::ScriptKey { at: k.at - t0, ..*k })
            .collect()
    }

    /// In Play mode a race that ends is reported in `PlayOutcomes`, which is
    /// what lets the free-roam test catch a regression; the safety-net
    /// time limit reports an expired timer too.
    #[test]
    fn play_mode_reports_outcomes() {
        use evani_core::play::PlayOutcomes;
        // An all-AI race runs to its end.
        let mut a = app();
        a.start();
        a.world.res_mut::<Race>().request(Request::Start { players: 0 });
        a.run_headless(60 * 3, 1000.0 / 60.0);
        assert_eq!(*a.world.res::<PlayOutcomes>(), PlayOutcomes::default(), "still racing");
        for _ in 0..60 * 400 {
            a.run_headless(1, 1000.0 / 60.0);
            if a.world.res::<Race>().phase == Phase::Finished {
                break;
            }
        }
        assert_eq!(a.world.res::<Race>().phase, Phase::Finished);
        let o = *a.world.res::<PlayOutcomes>();
        assert_eq!((o.game_overs, o.timers_expired, o.lives_lost), (1, 0, 0));
        // An idle player runs into the time limit.
        let mut b = app();
        b.start();
        b.world.res_mut::<Race>().request(Request::Start { players: 1 });
        b.run_headless(60 * 5, 1000.0 / 60.0);
        assert_eq!(b.world.res::<Race>().phase, Phase::Racing);
        b.world.res_mut::<Race>().clock = race::RACE_LIMIT_S - 0.05;
        b.run_headless(10, 1000.0 / 60.0);
        assert_eq!(b.world.res::<Race>().phase, Phase::Finished);
        let o = *b.world.res::<PlayOutcomes>();
        assert_eq!((o.game_overs, o.timers_expired), (1, 1));
    }

    #[test]
    fn runs_with_the_basic_look_and_draws_split_screen() {
        // Done-when 4: no toon look needed; missing roles fall back to tints.
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(KartRacerPlugin)
            .with_look(evani_look_basic::BasicLook);
        a.start();
        // Headless has no canvas; give the minimap one.
        a.world.res_mut::<evani_render::RenderWorld>().viewport_css = [1280.0, 720.0];
        a.run_headless(10, 1000.0 / 60.0);
        let one = a.world.res::<evani_core::overlay::RenderStats>().triangles;
        assert!(one > 10_000, "{one}");
        a.world.res_mut::<Race>().request(Request::Start { players: 2 });
        a.run_headless(10, 1000.0 / 60.0);
        let stats = a.world.res::<evani_core::overlay::RenderStats>();
        // Two views draw the scene twice.
        assert!(stats.triangles > one * 3 / 2, "{} vs {one}", stats.triangles);
        assert!(stats.sprites > 100, "minimaps: {}", stats.sprites);
        drop(stats);
        let hud = a.world.res::<hud::RaceHud>();
        assert_eq!(hud.players, 2);
        assert_eq!(hud.countdown, 3);
        assert!(hud.players_hud[1].active);
    }

    #[test]
    fn param_changes_reach_the_sim() {
        let mut a = app();
        a.start();
        a.world
            .res_mut::<ParamRegistry>()
            .set_by_name("kit.kart_racer.max_speed", ParamValue::F32(5.0))
            .unwrap();
        a.run_headless(60 * 12, 1000.0 / 60.0);
        let r = RaceView::new(&a.world);
        let fastest = r.karts.iter().map(|k| k.speed).fold(0.0f32, f32::max);
        // Skill and boosts stretch the cap a little, never far.
        assert!(fastest < 5.0 * 1.6, "{fastest}");
    }

    #[test]
    fn the_still_world_holds_the_race_and_draws_no_movers() {
        // The geometry check's switch: no tick moves anything and the karts,
        // items, effects and minimap are not drawn; off again, the race runs.
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(KartRacerPlugin)
            .with_look(evani_look_basic::BasicLook);
        a.start();
        a.world.res_mut::<evani_render::RenderWorld>().viewport_css = [1280.0, 720.0];
        a.world.res_mut::<Race>().request(Request::Start { players: 1 });
        a.run_headless(60 * 5, 1000.0 / 60.0);
        let pos = |a: &App| RaceView::new(&a.world).karts.iter().map(|k| k.pos).collect::<Vec<_>>();
        let stats = |a: &App| {
            let s = a.world.res::<evani_core::overlay::RenderStats>();
            (s.triangles, s.sprites)
        };
        let (tris, sprites) = stats(&a);
        assert!(sprites > 10, "minimap: {sprites}");
        let set = |a: &mut App, on: bool| {
            a.world
                .res_mut::<ParamRegistry>()
                .set_by_name("kit.kart_racer.static_world", ParamValue::Bool(on))
                .unwrap();
        };
        set(&mut a, true);
        a.run_headless(2, 1000.0 / 60.0);
        let held = pos(&a);
        a.run_headless(60, 1000.0 / 60.0);
        assert_eq!(pos(&a), held, "the race moved while held");
        let (still_tris, still_sprites) = stats(&a);
        assert_eq!(still_sprites, 0);
        assert!(still_tris < tris, "{still_tris} vs {tris}");
        set(&mut a, false);
        a.run_headless(30, 1000.0 / 60.0);
        assert_ne!(pos(&a), held, "the race runs again");
    }
}
