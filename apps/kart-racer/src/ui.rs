//! The game's light DOM UI: title screen, one HUD per local player (top and
//! bottom halves in split-screen), pause menu, settings (quality tier,
//! effect toggles, volume, stats overlay, key rebinding) and results.
//!
//! The markup and styles live in `index.html`; this module fills it in and
//! shows one screen at a time. Buttons carry `data-cmd` and inputs
//! `data-set`; one delegated listener per event type turns them into
//! [`UiCmd`]s, which are applied at the next frame boundary (natively the
//! same commands are applied by the tests). Text is written only when it
//! changed. The kit owns the race ([`Race`] takes [`Request`]s) and fills
//! [`RaceHud`]; the UI never touches the simulation directly.
//!
//! Saved (SaveData): `render.tier` (the renderer reads it at start),
//! `kart.volume`, `kart.overlay` and `kart.bindings` (bindings JSON).

// Natively there is no DOM, so the parts only the browser half (and the
// tests) call look unused there.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::audio::AudioMixer;
use evani_core::ecs::World;
use evani_core::effects::{BudgetStatus, EffectsStatus};
use evani_core::input::{Binding, Input};
use evani_core::overlay::{ErrorLog, Overlay};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::EngineModule;
use evani_core::quality::{QualityState, SAVED_TIER_KEY};
use evani_core::save::SaveData;
use evani_core::tiers::Tiers;
use evani_kit_kart_racer::hud::{PlayerHud, RaceHud, format_time, ordinal};
use evani_kit_kart_racer::hud_layout::{HudLayout, HudLayoutParams};
use evani_kit_kart_racer::race::{Phase, Race, Request};

pub const KEY_VOLUME: &str = "kart.volume";
pub const KEY_OVERLAY: &str = "kart.overlay";
pub const KEY_BINDINGS: &str = "kart.bindings";

/// What a player can ask the UI for.
#[derive(Debug, Clone, PartialEq)]
pub enum UiCmd {
    Start(usize),
    Restart,
    Title,
    Resume,
    OpenSettings,
    CloseSettings,
    Tier(String),
    Effect(String, bool),
    /// Master volume 0..1.
    Volume(f32),
    Overlay(bool),
    /// Wait for a key for this action (`p1_steer` waits for two).
    Bind(String),
    /// A key pressed while waiting (`KeyboardEvent.code`); Escape cancels.
    Key(String),
    ResetBindings,
    /// Enter VR (the button shows only when WebXR with WebGPU is available).
    EnterVr,
}

impl UiCmd {
    /// A button's `data-cmd`.
    pub fn from_click(cmd: &str) -> Option<Self> {
        Some(match cmd {
            "start1" => UiCmd::Start(1),
            "start2" => UiCmd::Start(2),
            "restart" => UiCmd::Restart,
            "title" => UiCmd::Title,
            "resume" => UiCmd::Resume,
            "settings" => UiCmd::OpenSettings,
            "close-settings" => UiCmd::CloseSettings,
            "reset-binds" => UiCmd::ResetBindings,
            "enter-vr" => UiCmd::EnterVr,
            _ => UiCmd::Bind(cmd.strip_prefix("bind:")?.to_string()),
        })
    }

    /// An input's `data-set` with its value and checked state.
    pub fn from_change(set: &str, value: &str, checked: bool) -> Option<Self> {
        Some(match set {
            "tier" => UiCmd::Tier(value.to_string()),
            "volume" => UiCmd::Volume((value.parse::<f32>().ok()? / 100.0).clamp(0.0, 1.0)),
            "overlay" => UiCmd::Overlay(checked),
            _ => UiCmd::Effect(set.strip_prefix("fx:")?.to_string(), checked),
        })
    }
}

/// Which screen is up (settings is drawn over it when open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Title,
    Race,
    Paused,
    Results,
}

pub fn screen(hud: &RaceHud) -> Screen {
    match hud.phase {
        Phase::Title => Screen::Title,
        // The results panel waits for the podium scene; the celebration
        // before it is the race screen with its finish HUD.
        Phase::Finished if hud.podium => Screen::Results,
        _ if hud.paused => Screen::Paused,
        _ => Screen::Race,
    }
}

/// The text of one player's HUD.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudView {
    pub place: String,
    pub racers: String,
    pub who: String,
    pub lap: String,
    /// Class of the lap line: `lap`, or `lap final` (yellow) on the last lap.
    pub lap_class: &'static str,
    /// The "+1" pop on the position number after an overtake (empty when off).
    pub gain: String,
    /// Class of the pop; it alternates `gain a` / `gain b` per overtake so the
    /// page restarts the animation each time.
    pub gain_class: &'static str,
    pub time: String,
    pub item: String,
    /// Class of the framed item slot (`slot`, `slot rolling`, `slot empty`).
    pub item_class: &'static str,
    /// Inline SVG of the held item (empty when there is none).
    pub icon: &'static str,
    /// "SEEKER INCOMING!" or "BOMB INCOMING!" under the item slot while an item
    /// is coming at the player (empty otherwise).
    pub threat: String,
    /// Class of the warning: `threat ink warn l1` to `l3` (faster blinking as
    /// the item gets closer), or plain `threat ink` when off.
    pub threat_class: &'static str,
    pub mid: String,
    pub mid_class: &'static str,
    /// The final place in large type under the FINISH! banner (empty until
    /// the player is over the line).
    pub big: String,
    pub drift: String,
    pub drift_class: &'static str,
    /// Debug readout of the slipstream on the HUD's `data-slip` attribute:
    /// `on|off speed=<m/s> cap=<m/s> base=<m/s>`.
    pub slip: String,
    /// A short cue in the middle of the screen ("GOOD START!").
    pub cue: String,
    pub speed: String,
    /// Free roam: the page shows the FREE ROAM label instead of the place,
    /// lap and race time.
    pub free_roam: bool,
    /// The controls line under the FREE ROAM label for the first moments
    /// (there is no title screen to show the keys); empty after.
    pub hint: String,
}

/// How long the controls hint shows at the start of a free roam (s).
pub const ROAM_HINT_S: f32 = 12.0;

/// The same line while the on-screen touch controls are up.
pub const ROAM_HINT_TOUCH: &str = "Left thumb steers · buttons on the right";

/// Inline SVG icon (64 x 64 view box, ink outlines in the HUD's candy
/// colours) for an item id, or "" for an unknown one.
pub fn item_icon(id: &str) -> &'static str {
    match id {
        "zoom_pop" => concat!(
            "<svg viewBox=\"0 0 64 64\" aria-hidden=\"true\">",
            "<path d=\"M36 4 12 36h16l-6 24 30-36H34z\" fill=\"#ffd23f\" stroke=\"#2b1240\" stroke-width=\"4\" stroke-linejoin=\"round\"/>",
            "<path d=\"M33 12 21 30\" stroke=\"#fff\" stroke-width=\"3\" stroke-linecap=\"round\" opacity=\".8\"/></svg>"
        ),
        "goo_blob" => concat!(
            "<svg viewBox=\"0 0 64 64\" aria-hidden=\"true\">",
            "<path d=\"M10 46c-4-12 4-24 14-26 4-10 18-12 24-2 10 2 14 16 8 26 0 8-10 12-18 10-6 4-16 4-20-2-6 0-8-2-8-6z\" fill=\"#4fe0b6\" stroke=\"#2b1240\" stroke-width=\"4\" stroke-linejoin=\"round\"/>",
            "<circle cx=\"26\" cy=\"32\" r=\"4\" fill=\"#2b1240\"/><circle cx=\"40\" cy=\"32\" r=\"4\" fill=\"#2b1240\"/>",
            "<ellipse cx=\"22\" cy=\"24\" rx=\"5\" ry=\"3\" fill=\"#fff\" opacity=\".7\"/></svg>"
        ),
        "boing_bomb" => concat!(
            "<svg viewBox=\"0 0 64 64\" aria-hidden=\"true\">",
            "<path d=\"M38 16c4-8 10-8 14-4\" fill=\"none\" stroke=\"#2b1240\" stroke-width=\"4\" stroke-linecap=\"round\"/>",
            "<path d=\"M54 6l2 5 5 1-4 3 1 5-4-3-5 2 2-5-3-4h5z\" fill=\"#ff9a2e\" stroke=\"#2b1240\" stroke-width=\"2\" stroke-linejoin=\"round\"/>",
            "<rect x=\"30\" y=\"14\" width=\"12\" height=\"8\" rx=\"2\" fill=\"#b58cff\" stroke=\"#2b1240\" stroke-width=\"3\"/>",
            "<circle cx=\"30\" cy=\"40\" r=\"20\" fill=\"#5a3a7a\" stroke=\"#2b1240\" stroke-width=\"4\"/>",
            "<ellipse cx=\"22\" cy=\"32\" rx=\"6\" ry=\"4\" fill=\"#fff\" opacity=\".55\"/></svg>"
        ),
        "seeker_star" => concat!(
            "<svg viewBox=\"0 0 64 64\" aria-hidden=\"true\">",
            "<path d=\"M32 4l8 18 20 2-15 13 5 20-18-11-18 11 5-20L4 24l20-2z\" fill=\"#ff5fa8\" stroke=\"#2b1240\" stroke-width=\"4\" stroke-linejoin=\"round\"/>",
            "<circle cx=\"26\" cy=\"32\" r=\"3\" fill=\"#2b1240\"/><circle cx=\"38\" cy=\"32\" r=\"3\" fill=\"#2b1240\"/>",
            "<path d=\"M24 22l6-8\" stroke=\"#fff\" stroke-width=\"3\" stroke-linecap=\"round\" opacity=\".7\"/></svg>"
        ),
        "shield_bubble" => concat!(
            "<svg viewBox=\"0 0 64 64\" aria-hidden=\"true\">",
            "<circle cx=\"32\" cy=\"32\" r=\"26\" fill=\"#8fe3ff\" fill-opacity=\".75\" stroke=\"#2b1240\" stroke-width=\"4\"/>",
            "<path d=\"M32 18l12 5v9c0 8-6 13-12 15-6-2-12-7-12-15v-9z\" fill=\"#49b6ff\" stroke=\"#2b1240\" stroke-width=\"3\" stroke-linejoin=\"round\"/>",
            "<path d=\"M16 24a18 18 0 0 1 12-10\" fill=\"none\" stroke=\"#fff\" stroke-width=\"4\" stroke-linecap=\"round\"/></svg>"
        ),
        _ => "",
    }
}

/// What one player's HUD says this frame.
pub fn hud_view(p: usize, h: &PlayerHud, hud: &RaceHud) -> HudView {
    let mut v = HudView {
        place: ordinal(h.place),
        racers: format!("/{}", h.racers),
        who: if hud.players > 1 {
            format!("Player {} · {}", p + 1, h.name)
        } else {
            h.name.clone()
        },
        lap: format!("Lap {}/{}", h.lap.clamp(1, h.laps.max(1)), h.laps),
        lap_class: if h.final_lap { "lap final" } else { "lap" },
        time: format_time(h.finished.unwrap_or(hud.clock)),
        // fixed(): the same text as {:.0} without float formatting code.
        speed: evani_core::num::fixed(h.speed_kmh, 0).to_string(),
        icon: h.item_id.map_or("", item_icon),
        free_roam: hud.free_roam,
        hint: if hud.free_roam && hud.clock < ROAM_HINT_S {
            "W A S D drive · Shift drift · E item · Esc pause".to_string()
        } else {
            String::new()
        },
        ..HudView::default()
    };
    (v.threat, v.threat_class) = match h.threat {
        Some(t) if t.level >= 0.67 => (t.kind.label().to_string(), "threat ink warn l3"),
        Some(t) if t.level >= 0.34 => (t.kind.label().to_string(), "threat ink warn l2"),
        Some(t) => (t.kind.label().to_string(), "threat ink warn l1"),
        None => (String::new(), "threat ink"),
    };
    if h.gain_left > 0.0 {
        v.gain = format!("+{}", h.gain_by.max(1));
        v.gain_class = if h.gains % 2 == 1 { "gain a" } else { "gain b" };
    } else {
        v.gain_class = "gain";
    }
    (v.item, v.item_class) = match (h.item, h.rolling) {
        (Some(label), true) => (label.to_string(), "slot rolling"),
        (Some(label), false) => (label.to_string(), "slot"),
        (None, _) => ("no item".to_string(), "slot empty"),
    };
    // Each countdown step swaps between two equal pop animations, so the
    // new number pops from 130 % to 100 % (GO follows 1, which is pop-a).
    (v.mid, v.mid_class) = if hud.free_roam {
        // No countdown, GO, laps, wrong way or finish to announce.
        (String::new(), "mid ink")
    } else if hud.countdown > 0 {
        let pop = if hud.countdown % 2 == 1 { "mid ink count pop-a" } else { "mid ink count pop-b" };
        (hud.countdown.to_string(), pop)
    } else if h.finished.is_some() {
        // Styled like FINAL LAP!, with the place big underneath.
        v.big = ordinal(h.place);
        ("FINISH!".to_string(), "mid ink final banner finish")
    } else if hud.since_go < 1.0 {
        ("GO!".to_string(), "mid ink go pop-b")
    } else if h.wrong_way {
        // Rides the lap-banner band (never centred on the player marker).
        ("WRONG WAY".to_string(), "mid ink warn wrong")
    } else if h.lap_flash < 2.0 && h.final_lap {
        // Lap banners sit top-centre (hud-layout: 12 % down) so they never
        // cover the karts.
        ("FINAL LAP!".to_string(), "mid ink final banner")
    } else if h.lap_flash < 1.5 && h.lap > 1 {
        (format!("LAP {}", h.lap), "mid ink banner")
    } else {
        (String::new(), "mid ink")
    };
    (v.drift, v.drift_class) = if h.spun_out {
        ("Spin out!".to_string(), "drift ink warn")
    } else if h.boosting {
        ("BOOST!".to_string(), "drift ink boost")
    } else if h.drifting {
        match h.drift_tier {
            0 => ("Drift...".to_string(), "drift ink"),
            1 => ("Drift: blue".to_string(), "drift ink t1"),
            2 => ("Drift: orange".to_string(), "drift ink t2"),
            _ => ("Drift: pink!".to_string(), "drift ink t3"),
        }
    } else if h.slipstream {
        ("Slipstream".to_string(), "drift ink slip")
    } else if h.shielded {
        ("Shield up".to_string(), "drift ink go")
    } else {
        (String::new(), "drift ink")
    };
    if h.good_start {
        v.cue = "GOOD START!".to_string();
    }
    v.slip = format!(
        "{} speed={} cap={} base={}",
        if h.slipstream { "on" } else { "off" },
        evani_core::num::fixed(h.speed_ms, 2),
        evani_core::num::fixed(h.cap, 2),
        evani_core::num::fixed(h.cap_base, 2)
    );
    v
}

/// One row of the results table: place, name, time, a local player's row,
/// and the kart's colour as a CSS hex (`#ff4fa3` for Pip).
pub type ResultLine = (String, String, String, bool, String);

/// A linear RGB colour (what the roster holds) as a CSS `#rrggbb`.
pub fn css_hex(c: [f32; 3]) -> String {
    let to = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", to(c[0]), to(c[1]), to(c[2]))
}

/// A fingerprint of what the results screen shows (places, karts, times and
/// each local player's place and result), so the page rebuilds it whenever a
/// different race finishes, even straight after another one (a review moment
/// restaging the finish), without building the strings every frame.
pub fn results_key(hud: &RaceHud) -> u64 {
    // FNV-1a over the fields.
    let mut k: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        k ^= v;
        k = k.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(hud.players as u64);
    for r in &hud.results {
        mix(r.place as u64);
        mix(r.kart as u64);
        mix(r.player.map_or(u64::MAX, |p| p as u64));
        mix(r.time.map_or(u64::MAX, |t| u64::from(t.to_bits())));
    }
    for h in hud.players_hud.iter().take(hud.players) {
        mix(u64::from(h.active) | u64::from(h.won) << 1 | (h.place as u64) << 2);
    }
    k
}

/// The results screen: heading, the line under it and the table.
pub fn results_view(hud: &RaceHud) -> (String, String, Vec<ResultLine>) {
    let humans: Vec<(usize, &PlayerHud)> = hud
        .players_hud
        .iter()
        .enumerate()
        .take(hud.players)
        .filter(|(_, h)| h.active)
        .collect();
    let (head, sub) = match humans.as_slice() {
        [(_, h)] if h.won && h.place == 1 => ("You win!".to_string(), "First over the line. Top kart on Sugarpeak.".to_string()),
        [(_, h)] if h.won => ("Podium!".to_string(), format!("{} place: a win. Can you take 1st?", ordinal(h.place))),
        [(_, h)] => ("Race lost".to_string(), format!("{} place. Finish in the top 3 to win.", ordinal(h.place))),
        [] => ("Race over".to_string(), String::new()),
        many => {
            let parts: Vec<String> = many
                .iter()
                .map(|(p, h)| format!("Player {}: {} ({})", p + 1, ordinal(h.place), if h.won { "win" } else { "lose" }))
                .collect();
            let best = many.iter().min_by_key(|(_, h)| h.place).map(|(p, _)| p + 1).unwrap_or(1);
            (format!("Player {best} wins the duel!"), parts.join(" · "))
        }
    };
    let rows = hud
        .results
        .iter()
        .map(|r| {
            let name = match r.player {
                Some(p) if hud.players > 1 => format!("{} (P{})", r.name, p + 1),
                Some(_) => format!("{} (you)", r.name),
                None => r.name.clone(),
            };
            let time = r.time.map_or("--".to_string(), format_time);
            (ordinal(r.place), name, time, r.player.is_some(), css_hex(r.color))
        })
        .collect();
    (head, sub, rows)
}

/// A key code as a player reads it (`KeyA` → `A`, `ShiftLeft` → `Left Shift`).
pub fn key_label(code: &str) -> String {
    if let Some(k) = code.strip_prefix("Key").or_else(|| code.strip_prefix("Digit")) {
        return k.to_string();
    }
    if let Some(k) = code.strip_prefix("Arrow") {
        return k.to_string();
    }
    for side in ["Left", "Right"] {
        if let Some(k) = code.strip_suffix(side) {
            let k = if k == "Control" { "Ctrl" } else { k };
            return format!("{side} {k}");
        }
    }
    code.to_string()
}

/// The keyboard binding of an action, as a label (`A / D`, `E`).
pub fn binding_label(bindings: &[Binding]) -> String {
    for b in bindings {
        match b {
            Binding::Key(k) => return key_label(k.as_str()),
            Binding::KeyAxis { neg, pos } => {
                return format!("{} / {}", key_label(neg.as_str()), key_label(pos.as_str()));
            }
            _ => {}
        }
    }
    "none".to_string()
}

/// Rebindable actions per player: suffix and label.
pub const BIND_ROWS: [(&str, &str); 5] = [
    ("steer", "Steer (left / right)"),
    ("throttle", "Accelerate"),
    ("brake", "Brake, reverse"),
    ("drift", "Drift"),
    ("item", "Use item"),
];

/// A player-facing effect name.
pub fn effect_label(name: &str) -> &str {
    match name {
        "outline" => "Ink outlines",
        "speedlines" => "Speed lines",
        "shake" => "Screen shake",
        "bloom" => "Glow",
        "vignette" => "Dark corners",
        other => other,
    }
}

/// Rebinding in progress: the action and, for steering, the left key.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub action: String,
    pub first: Option<String>,
}

pub type CmdQueue = Rc<RefCell<Vec<UiCmd>>>;

/// What the page-level key listener must do: 0 nothing, 1 Esc closes
/// settings, 2 every key goes to the rebinding.
pub type Modal = Rc<Cell<u8>>;

#[derive(Default)]
pub struct GameUi {
    queue: CmdQueue,
    modal: Modal,
    pub settings_open: bool,
    pub pending: Option<Pending>,
    /// Bindings as the kit registered them (for "Reset controls").
    defaults: String,
    /// Bumped whenever the bindings, volume or overlay change.
    pub settings_rev: u64,
    /// While live holds the sim for a capture (a moment, a bench or a held
    /// frame), the stats overlay is hidden so it never covers the HUD in a
    /// review shot; this keeps the player's setting to put back after.
    overlay_before_capture: Option<bool>,
    #[cfg(target_arch = "wasm32")]
    dom: Option<dom::Dom>,
}

impl GameUi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a command (the DOM does this; tests call it directly).
    #[cfg(test)]
    pub fn push(&self, c: UiCmd) {
        self.queue.borrow_mut().push(c);
    }

    fn save(world: &World, key: &str, value: &str) {
        let r = world.res_mut::<SaveData>().save_raw(key, value);
        if let Err(e) = r {
            world.res_mut::<ErrorLog>().note(format!("ui: save {key}: {e}"));
        }
    }

    fn load(world: &World, key: &str) -> Option<String> {
        world.res::<SaveData>().load_raw(key).ok().flatten()
    }

    fn request(world: &World, r: Request) {
        if let Some(mut race) = world.try_res_mut::<Race>() {
            race.request(r);
        }
    }

    /// Hide the stats overlay while a capture holds the sim clock, and put
    /// the player's setting back when the hold ends (the saved setting is
    /// never touched). Only builds with live ever hold the clock.
    fn hide_overlay_in_captures(&mut self, world: &World) {
        let held = world
            .try_res::<evani_core::capture::CaptureQueue>()
            .is_some_and(|q| q.has_pending() || q.sim_time_override.is_some());
        let mut o = world.res_mut::<Overlay>();
        match (held, self.overlay_before_capture) {
            (true, None) => {
                self.overlay_before_capture = Some(o.enabled);
                o.enabled = false;
            }
            (false, Some(was)) => {
                o.enabled = was;
                self.overlay_before_capture = None;
            }
            _ => {}
        }
    }

    /// Apply one command.
    pub fn apply(&mut self, world: &World, c: UiCmd) {
        match c {
            UiCmd::Start(n) => {
                self.settings_open = false;
                Self::request(world, Request::Start { players: n });
            }
            UiCmd::Restart => {
                self.settings_open = false;
                Self::request(world, Request::Restart);
            }
            UiCmd::Title => {
                self.settings_open = false;
                Self::request(world, Request::Title);
            }
            UiCmd::Resume => Self::request(world, Request::Pause(false)),
            UiCmd::OpenSettings => self.settings_open = true,
            UiCmd::CloseSettings => {
                self.settings_open = false;
                self.pending = None;
            }
            UiCmd::Tier(id) => {
                let r = world.res_mut::<Tiers>().try_set_active(&id);
                match r {
                    Ok(()) => Self::save(world, SAVED_TIER_KEY, &id),
                    Err(e) => world.res_mut::<ErrorLog>().error(format!("ui: tier {id}: {e}")),
                }
            }
            UiCmd::Effect(name, on) => {
                let p = format!("fx.{name}.enabled");
                let r = world.res_mut::<ParamRegistry>().set_by_name(&p, ParamValue::Bool(on));
                if let Err(e) = r {
                    world.res_mut::<ErrorLog>().error(format!("ui: {p}: {e}"));
                }
            }
            UiCmd::Volume(v) => {
                world.res_mut::<AudioMixer>().set_volume(None, v);
                Self::save(world, KEY_VOLUME, &evani_core::num::fixed(v, 2).to_string());
            }
            UiCmd::Overlay(on) => {
                match self.overlay_before_capture.as_mut() {
                    Some(was) => *was = on,
                    None => world.res_mut::<Overlay>().enabled = on,
                }
                Self::save(world, KEY_OVERLAY, if on { "1" } else { "0" });
                self.settings_rev += 1;
            }
            UiCmd::Bind(action) => {
                self.pending = Some(Pending { action, first: None });
                self.settings_rev += 1;
            }
            UiCmd::Key(code) => self.key(world, code),
            UiCmd::ResetBindings => {
                self.pending = None;
                let r = world.res_mut::<Input>().apply_bindings_json(&self.defaults);
                if let Err(e) = r {
                    world.res_mut::<ErrorLog>().error(format!("ui: reset controls: {e}"));
                }
                let _ = world.res_mut::<SaveData>().delete(KEY_BINDINGS);
                self.settings_rev += 1;
            }
            UiCmd::EnterVr => {
                // The immersive session lives in the on-demand VR module
                // (docs/VR_STATUS.md); M1 only detects support.
                world.res_mut::<ErrorLog>().note("Enter VR: the VR module is not in this build yet");
            }
        }
    }

    /// A key arrived while rebinding.
    fn key(&mut self, world: &World, code: String) {
        let Some(mut p) = self.pending.take() else {
            return;
        };
        self.settings_rev += 1;
        if code == "Escape" {
            return;
        }
        let text = if p.action.ends_with("_steer") {
            match p.first.take() {
                None => {
                    p.first = Some(code);
                    self.pending = Some(p);
                    return;
                }
                Some(left) => format!("keys:{left}/{code}"),
            }
        } else {
            format!("key:{code}")
        };
        let Ok(binding) = text.parse::<Binding>() else {
            world.res_mut::<ErrorLog>().note(format!("ui: cannot bind {text}"));
            return;
        };
        let json = {
            let mut input = world.res_mut::<Input>();
            let Some(id) = input.action_id(&p.action) else {
                return;
            };
            // Slot 0 is the keyboard binding (pads follow it).
            input.rebind(id, 0, binding);
            input.bindings_json()
        };
        Self::save(world, KEY_BINDINGS, &json);
    }

    /// Settings as they were saved last time.
    fn load_saved(&mut self, world: &World) {
        self.defaults = world.res::<Input>().bindings_json();
        if let Some(json) = Self::load(world, KEY_BINDINGS) {
            let r = world.res_mut::<Input>().apply_bindings_json(&json);
            if let Err(e) = r {
                world.res_mut::<ErrorLog>().note(format!("ui: saved controls ignored: {e}"));
            }
        }
        if let Some(v) = Self::load(world, KEY_VOLUME).and_then(|s| s.parse::<f32>().ok()) {
            world.res_mut::<AudioMixer>().set_volume(None, v.clamp(0.0, 1.0));
        }
        // The stats overlay is a developer view: off unless switched on.
        world.res_mut::<Overlay>().enabled = Self::load(world, KEY_OVERLAY).as_deref() == Some("1");
    }

    /// Keep the race paused while settings are open over it.
    fn hold_pause(&self, world: &World) {
        if !self.settings_open {
            return;
        }
        let hud = world.res::<RaceHud>();
        if matches!(hud.phase, Phase::Countdown | Phase::Racing) && hud.players > 0 && !hud.paused {
            drop(hud);
            Self::request(world, Request::Pause(true));
        }
    }

    fn sync_modal(&self) {
        self.modal.set(match (self.settings_open, &self.pending) {
            (_, Some(_)) => 2,
            (true, None) => 1,
            _ => 0,
        });
    }

    /// A snapshot that changes whenever the settings panel must be redrawn.
    /// Includes the adaptive-quality revision (an effect paused or resumed)
    /// and the XR flags (what the tier list and Enter VR show).
    pub fn settings_key(&self, world: &World) -> (u64, usize, BudgetStatus, u64, u64, bool, bool) {
        let tiers = world.res::<Tiers>();
        (
            world.res::<ParamRegistry>().revision(),
            tiers.active_index(),
            *world.res::<BudgetStatus>(),
            self.settings_rev,
            world.try_res::<QualityState>().map_or(0, |q| q.revision()),
            tiers.xr_session(),
            tiers.xr_available(),
        )
    }

    /// Every compiled-in effect with its state for the settings panel.
    pub fn effects(world: &World) -> Vec<(String, bool, &'static str)> {
        let reg = world.res::<ParamRegistry>();
        let fx = world.try_res::<EffectsStatus>();
        reg.defs()
            .iter()
            .filter_map(|d| {
                let name = d.name.strip_prefix("fx.")?.strip_suffix(".enabled")?;
                let id = reg.id(&d.name)?;
                let wanted = matches!(reg.value(id), Some(ParamValue::Bool(true)));
                let state = fx.as_ref().and_then(|f| f.get(name)).map_or("", |e| e.state.as_str());
                Some((name.to_string(), wanted, state))
            })
            .collect()
    }
}

impl EngineModule for GameUi {
    fn id(&self) -> &'static str {
        "evani.module.kart_ui"
    }

    fn label(&self) -> &'static str {
        "ui"
    }

    fn start(&mut self, world: &mut World) {
        self.load_saved(world);
        #[cfg(target_arch = "wasm32")]
        {
            match dom::Dom::create(&self.queue, &self.modal) {
                Ok(d) => self.dom = Some(d),
                Err(e) => world.res_mut::<ErrorLog>().error(format!("ui: {e}")),
            }
        }
    }

    fn frame(&mut self, world: &mut World) {
        let cmds = core::mem::take(&mut *self.queue.borrow_mut());
        for c in cmds {
            self.apply(world, c);
        }
        self.hold_pause(world);
        self.hide_overlay_in_captures(world);
        self.sync_modal();
        #[cfg(target_arch = "wasm32")]
        if let Some(mut d) = self.dom.take() {
            d.sync(world, self);
            self.dom = Some(d);
        }
    }

    fn visibility_changed(&mut self, world: &mut World, hidden: bool) {
        // A hidden tab pauses a race (the loop stops anyway; this keeps the
        // pause menu up when the player comes back).
        if hidden {
            Self::request(world, Request::Pause(true));
        }
    }

    fn status(&self, world: &World, out: &mut String) {
        let hud = world.res::<RaceHud>();
        out.push_str(hud.phase.as_str());
        if self.settings_open {
            out.push_str(" · settings");
        }
    }
}

/// The HUD layout tunables (`kit.kart_racer.hud_*`) for this frame, or the
/// code defaults when the kit isn't in the world.
pub fn hud_layout(world: &World) -> HudLayout {
    world
        .try_res::<HudLayoutParams>()
        .map(|p| p.read(&world.res::<ParamRegistry>()))
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
mod dom {
    use evani_core::audio::AudioMixer;
    use evani_core::ecs::World;
    use evani_core::input::Input;
    use evani_core::overlay::Overlay;
    use evani_core::tiers::Tiers;
    use evani_kit_kart_racer::hud::RaceHud;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

    use super::{BIND_ROWS, CmdQueue, GameUi, HudLayout, HudView, Modal, Screen, UiCmd, binding_label, effect_label, hud_layout, hud_view, results_key, results_view, screen};

    /// One text node and the class list last written.
    struct Text {
        el: Element,
        text: String,
        class: &'static str,
    }

    impl Text {
        fn new(el: Element) -> Self {
            Self { el, text: String::new(), class: "" }
        }

        fn set(&mut self, s: &str) {
            if self.text != s {
                self.text.clear();
                self.text.push_str(s);
                self.el.set_text_content(Some(s));
            }
        }

        /// Replace the element's markup (trusted, built-in strings only).
        fn set_html(&mut self, s: &str) {
            if self.text != s {
                self.text.clear();
                self.text.push_str(s);
                self.el.set_inner_html(s);
            }
        }

        fn class(&mut self, c: &'static str) {
            if self.class != c {
                self.class = c;
                self.el.set_class_name(c);
            }
        }
    }

    struct Hud {
        root: Element,
        place: Text,
        racers: Text,
        who: Text,
        lap: Text,
        gain: Text,
        time: Text,
        item: Text,
        slot: Text,
        icon: Text,
        threat: Text,
        mid: Text,
        big: Text,
        drift: Text,
        cue: Text,
        speed: Text,
        hint: Text,
        /// The `data-slip` readout last written on `root`.
        slip: String,
    }

    const HUD_HTML: &str = "<div class=\"tl ink\"><div class=\"roam\"><b>FREE ROAM</b><small data-k=\"hint\"></small></div><div class=\"place\"><span data-k=\"place\"></span><small data-k=\"racers\"></small><span class=\"gain\" data-k=\"gain\"></span></div>\
        <div class=\"lap\" data-k=\"lap\"></div><div class=\"who\" data-k=\"who\"></div></div>\
        <div class=\"tc\"><div class=\"slot empty\" data-k=\"slot\"><div class=\"icon\" data-k=\"icon\"></div>\
        <span class=\"item\" data-k=\"item\"></span></div>\
        <div class=\"threat ink\" data-k=\"threat\"></div></div>\
        <div class=\"tr ink\"><div class=\"time\" data-k=\"time\"></div></div>\
        <div class=\"mid ink\" data-k=\"mid\"></div><div class=\"big ink\" data-k=\"big\"></div><div class=\"cue ink\" data-k=\"cue\"></div>\
        <div class=\"bc\"><div class=\"drift ink\" data-k=\"drift\"></div></div>\
        <div class=\"bl\"><div class=\"speed ink\"><b data-k=\"speed\"></b><small>km/h</small></div></div>";

    pub struct Dom {
        doc: Document,
        ui: Element,
        huds: [Hud; 2],
        title: Element,
        pause: Element,
        results: Element,
        settings: Element,
        settings_body: Element,
        res_head: Element,
        res_sub: Element,
        res_table: Element,
        shown: Option<(Screen, bool, usize)>,
        hud_on: [bool; 2],
        /// The finish HUD (celebrating) is showing on this player's HUD.
        hud_fin: [bool; 2],
        /// Free roam is showing on this player's HUD.
        hud_free: [bool; 2],
        /// Free roam's pause menu wording is on the page (None: not set yet).
        roam_menu: Option<bool>,
        settings_seen: Option<(u64, usize, evani_core::effects::BudgetStatus, u64, u64, bool, bool)>,
        /// [`results_key`] of the results on the page.
        results_seen: Option<u64>,
        /// The HUD layout last written on `#ui` as CSS variables.
        layout_seen: Option<HudLayout>,
    }

    fn by_id(doc: &Document, id: &str) -> Result<Element, String> {
        doc.get_element_by_id(id).ok_or_else(|| format!("index.html has no #{id}"))
    }

    fn part(root: &Element, k: &str) -> Result<Text, String> {
        root.query_selector(&format!("[data-k=\"{k}\"]"))
            .ok()
            .flatten()
            .map(Text::new)
            .ok_or_else(|| format!("hud part {k} missing"))
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    fn set_on(el: &Element, base: &str, on: bool) {
        el.set_class_name(&if on { format!("{base} on") } else { base.to_string() });
    }

    impl Dom {
        pub fn create(queue: &CmdQueue, modal: &Modal) -> Result<Self, String> {
            let win = web_sys::window().ok_or("no window")?;
            let doc = win.document().ok_or("no document")?;
            let ui = by_id(&doc, "ui")?;
            let mut huds = Vec::with_capacity(2);
            for p in 0..2 {
                let root = by_id(&doc, &format!("hud{p}"))?;
                root.set_inner_html(HUD_HTML);
                huds.push(Hud {
                    place: part(&root, "place")?,
                    racers: part(&root, "racers")?,
                    who: part(&root, "who")?,
                    lap: part(&root, "lap")?,
                    gain: part(&root, "gain")?,
                    time: part(&root, "time")?,
                    item: part(&root, "item")?,
                    slot: part(&root, "slot")?,
                    icon: part(&root, "icon")?,
                    threat: part(&root, "threat")?,
                    mid: part(&root, "mid")?,
                    big: part(&root, "big")?,
                    drift: part(&root, "drift")?,
                    cue: part(&root, "cue")?,
                    speed: part(&root, "speed")?,
                    hint: part(&root, "hint")?,
                    slip: String::new(),
                    root,
                });
            }
            let huds: [Hud; 2] = huds.try_into().map_err(|_| "two huds")?;

            // Buttons: one delegated click listener.
            {
                let q = queue.clone();
                let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                    let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                        return;
                    };
                    let Some(b) = t.closest("[data-cmd]").ok().flatten() else {
                        return;
                    };
                    if let Some(c) = b.get_attribute("data-cmd").and_then(|c| UiCmd::from_click(&c)) {
                        q.borrow_mut().push(c);
                    }
                });
                let _ = ui.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
                cb.forget(); // lives as long as the page
            }
            // Inputs: change (selects, checkboxes) and input (the slider).
            for ev in ["change", "input"] {
                let q = queue.clone();
                let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                    let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                        return;
                    };
                    let Some(set) = t.get_attribute("data-set") else {
                        return;
                    };
                    let (value, checked) = if let Some(i) = t.dyn_ref::<HtmlInputElement>() {
                        (i.value(), i.checked())
                    } else if let Some(s) = t.dyn_ref::<web_sys::HtmlSelectElement>() {
                        (s.value(), false)
                    } else {
                        return;
                    };
                    // The slider reports on input; everything else on change.
                    let slider = set == "volume";
                    if (e.type_() == "input") != slider {
                        return;
                    }
                    if let Some(c) = UiCmd::from_change(&set, &value, checked) {
                        q.borrow_mut().push(c);
                    }
                });
                let _ = ui.add_event_listener_with_callback(ev, cb.as_ref().unchecked_ref());
                cb.forget();
            }
            // Keys while rebinding (or Esc in settings) never reach the game:
            // a capture-phase listener on the window runs first.
            {
                let (q, m) = (queue.clone(), modal.clone());
                let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                    let code = e.code();
                    match m.get() {
                        2 => {
                            e.prevent_default();
                            e.stop_propagation();
                            if !e.repeat() {
                                q.borrow_mut().push(UiCmd::Key(code));
                            }
                        }
                        1 if code == "Escape" => {
                            e.stop_propagation();
                            q.borrow_mut().push(UiCmd::CloseSettings);
                        }
                        _ => {}
                    }
                });
                let _ = win.add_event_listener_with_callback_and_bool("keydown", cb.as_ref().unchecked_ref(), true);
                cb.forget();
            }
            Ok(Self {
                title: by_id(&doc, "title")?,
                pause: by_id(&doc, "pause")?,
                results: by_id(&doc, "results")?,
                settings: by_id(&doc, "settings")?,
                settings_body: by_id(&doc, "settings-body")?,
                res_head: by_id(&doc, "res-head")?,
                res_sub: by_id(&doc, "res-sub")?,
                res_table: by_id(&doc, "res-table")?,
                doc,
                ui,
                huds,
                shown: None,
                hud_on: [false; 2],
                hud_fin: [false; 2],
                hud_free: [false; 2],
                roam_menu: None,
                settings_seen: None,
                results_seen: None,
                layout_seen: None,
            })
        }

        /// Focus the first button of a menu so Enter or Space picks it.
        fn focus_first(&self, el: &Element) {
            if let Some(b) = el.query_selector("button").ok().flatten()
                && let Ok(b) = b.dyn_into::<HtmlElement>()
            {
                let _ = b.focus();
            }
        }

        fn blur(&self) {
            if let Some(a) = self.doc.active_element()
                && let Ok(a) = a.dyn_into::<HtmlElement>()
            {
                let _ = a.blur();
            }
        }

        pub fn sync(&mut self, world: &World, ui: &GameUi) {
            let hud = world.res::<RaceHud>();
            let scr = screen(&hud);
            // HUD layout tunables (Look Lab, param.set): the CSS variables
            // `hud.css` reads, written only when a value changed.
            let layout = hud_layout(world);
            if self.layout_seen != Some(layout) {
                self.layout_seen = Some(layout);
                let _ = self.ui.set_attribute("style", &layout.style_attr());
            }
            let now = (scr, ui.settings_open, hud.players);
            if self.shown != Some(now) {
                self.shown = Some(now);
                self.blur();
                set_on(&self.title, "screen", scr == Screen::Title && !ui.settings_open);
                set_on(&self.pause, "screen dim", scr == Screen::Paused && !ui.settings_open);
                set_on(&self.results, "screen dim", scr == Screen::Results && !ui.settings_open);
                set_on(&self.settings, "screen dim", ui.settings_open);
                self.ui.set_class_name(if hud.players > 1 { "two" } else { "" });
                if ui.settings_open {
                    self.settings_seen = None;
                } else if scr == Screen::Title {
                    self.focus_first(&self.title);
                } else if scr == Screen::Paused {
                    self.focus_first(&self.pause);
                }
            }
            // Slipstream debug log (every kart; DevTools' Verbose level).
            for line in &hud.slip_log {
                web_sys::console::debug_1(&line.as_str().into());
            }
            // HUDs.
            let racing = matches!(scr, Screen::Race | Screen::Paused);
            for p in 0..2 {
                let h = &hud.players_hud[p];
                let on = racing && h.active && p < hud.players;
                // Over the line: the finish HUD keeps the time, the banner and the
                // big place and fades the rest.
                let fin = on && h.finished.is_some();
                let free = on && hud.free_roam;
                if self.hud_on[p] != on || self.hud_fin[p] != fin || self.hud_free[p] != free {
                    self.hud_on[p] = on;
                    self.hud_fin[p] = fin;
                    self.hud_free[p] = free;
                    self.huds[p].root.set_class_name(match (on, fin, free) {
                        (true, true, _) => "hud on fin",
                        (true, false, true) => "hud on free",
                        (true, false, false) => "hud on",
                        _ => "hud",
                    });
                }
                if on {
                    let mut v = hud_view(p, h, &hud);
                    // Under touch the controls line names the on-screen controls, not keys.
                    if !v.hint.is_empty() && evani_core::touch::active(world) {
                        v.hint = super::ROAM_HINT_TOUCH.to_string();
                    }
                    write_hud(&mut self.huds[p], &v);
                }
            }
            // Free roam has no race to restart or title to quit to: the pause
            // menu says "Back to the start" and drops "Quit to title".
            if self.roam_menu != Some(hud.free_roam) {
                self.roam_menu = Some(hud.free_roam);
                if let Ok(Some(b)) = self.pause.query_selector("[data-cmd=restart]") {
                    b.set_text_content(Some(if hud.free_roam { "Back to the start" } else { "Restart race" }));
                }
                if let Ok(Some(b)) = self.pause.query_selector("[data-cmd=title]") {
                    let _ = b.set_attribute("style", if hud.free_roam { "display:none" } else { "" });
                }
            }
            // Results, rebuilt once per finished race (and for a different
            // race that finishes straight after, as a restaged review moment).
            let key = if scr == Screen::Results { Some(results_key(&hud)) } else { None };
            if key.is_some() && self.results_seen != key {
                self.results_seen = key;
                let (head, sub, rows) = results_view(&hud);
                self.res_head.set_text_content(Some(&head));
                self.res_sub.set_text_content(Some(&sub));
                let mut html = String::with_capacity(rows.len() * 80);
                for (place, name, time, me, color) in &rows {
                    // Each row in its kart's colour (hex built by css_hex, safe
                    // to put in the attribute); a local player's is highlighted.
                    html.push_str(if *me { "<tr class=\"me\" style=\"color:" } else { "<tr style=\"color:" });
                    html.push_str(color);
                    html.push_str(if *me { ";background:" } else { "\">" });
                    if *me {
                        html.push_str(color);
                        html.push_str("33\">");
                    }
                    for c in [place, name, time] {
                        html.push_str("<td>");
                        html.push_str(&escape(c));
                        html.push_str("</td>");
                    }
                    html.push_str("</tr>");
                }
                self.res_table.set_inner_html(&html);
            } else if key.is_none() {
                self.results_seen = None;
            }
            drop(hud);
            // Settings.
            if ui.settings_open {
                let key = ui.settings_key(world);
                if self.settings_seen != Some(key) {
                    self.settings_seen = Some(key);
                    self.settings_body.set_inner_html(&settings_html(world, ui));
                }
            }
        }
    }

    fn write_hud(h: &mut Hud, v: &HudView) {
        h.place.set(&v.place);
        h.racers.set(&v.racers);
        h.who.set(&v.who);
        h.lap.set(&v.lap);
        h.lap.class(v.lap_class);
        h.gain.set(&v.gain);
        h.gain.class(v.gain_class);
        h.time.set(&v.time);
        h.item.set(&v.item);
        h.slot.class(v.item_class);
        h.icon.set_html(v.icon);
        h.threat.set(&v.threat);
        h.threat.class(v.threat_class);
        h.mid.set(&v.mid);
        h.mid.class(v.mid_class);
        h.big.set(&v.big);
        h.drift.set(&v.drift);
        h.drift.class(v.drift_class);
        h.cue.set(&v.cue);
        h.speed.set(&v.speed);
        h.hint.set(&v.hint);
        if h.slip != v.slip {
            h.slip.clone_from(&v.slip);
            let _ = h.root.set_attribute("data-slip", &v.slip);
        }
    }

    fn settings_html(world: &World, ui: &GameUi) -> String {
        use core::fmt::Write;
        let mut s = String::with_capacity(4096);
        s.push_str("<h3>Quality</h3><label>Level <select data-set=\"tier\">");
        {
            let tiers = world.res::<Tiers>();
            let active = tiers.active().id.clone();
            // Lite to Ultra in a normal tab; VR only inside an XR session.
            for (_, p) in tiers.selectable() {
                let sel = if p.id == active { " selected" } else { "" };
                let _ = write!(s, "<option value=\"{}\"{sel}>{}</option>", escape(&p.id), escape(&p.label));
            }
        }
        s.push_str("</select></label>");
        {
            let tiers = world.res::<Tiers>();
            if tiers.xr_available() && !tiers.xr_session() {
                s.push_str("<button data-cmd=\"enter-vr\">Enter VR</button>");
            }
        }
        s.push_str("<h3>Effects</h3>");
        let fx = GameUi::effects(world);
        if fx.is_empty() {
            s.push_str("<p class=\"note\">No effects in this build.</p>");
        }
        for (name, wanted, state) in &fx {
            let chk = if *wanted { " checked" } else { "" };
            let note = match *state {
                "off (budget)" => " (too much for this quality level)",
                "off (vr comfort)" => " (off in VR for comfort)",
                "on, paused to hold frame rate" => " (on, paused to hold frame rate)",
                _ => "",
            };
            let _ = write!(
                s,
                "<label><input type=\"checkbox\" data-set=\"fx:{}\"{chk}>{}{note}</label>",
                escape(name),
                effect_label(name)
            );
        }
        let vol = evani_core::num::fixed((world.res::<AudioMixer>().volume(None) * 100.0).round(), 0);
        let _ = write!(
            s,
            "<h3>Sound</h3><label>Volume <input type=\"range\" min=\"0\" max=\"100\" value=\"{vol}\" data-set=\"volume\"></label>"
        );
        let ov = if world.res::<Overlay>().enabled { " checked" } else { "" };
        let _ = write!(s, "<label><input type=\"checkbox\" data-set=\"overlay\"{ov}>Show speed stats</label>");
        s.push_str("<h3>Controls</h3><table class=\"binds\"><tr><td></td><td>Player 1</td><td>Player 2</td></tr>");
        {
            let input = world.res::<Input>();
            for (suffix, label) in BIND_ROWS {
                let _ = write!(s, "<tr><td>{label}</td>");
                for p in 1..=2 {
                    let action = format!("p{p}_{suffix}");
                    let text = match &ui.pending {
                        Some(pd) if pd.action == action && pd.first.is_some() => "press RIGHT key".to_string(),
                        Some(pd) if pd.action == action && suffix == "steer" => "press LEFT key".to_string(),
                        Some(pd) if pd.action == action => "press a key".to_string(),
                        _ => input
                            .action_id(&action)
                            .map(|id| binding_label(&input.actions()[id.0 as usize].bindings))
                            .unwrap_or_default(),
                    };
                    let _ = write!(s, "<td><button data-cmd=\"bind:{action}\">{}</button></td>", escape(&text));
                }
                s.push_str("</tr>");
            }
        }
        s.push_str(
            "</table><p class=\"note\">Click a key, then press the new one (Esc cancels). Gamepad 1 drives player 1 and gamepad 2 player 2. \
             Esc or P pauses.</p><p><button data-cmd=\"reset-binds\" class=\"alt\">Reset controls</button></p>",
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_app, build_app_in};
    use evani_core::App;

    fn started() -> (App, GameUi) {
        let mut app = build_app();
        app.start();
        let mut ui = GameUi::new();
        ui.start(&mut app.world);
        (app, ui)
    }

    fn tick(app: &mut App, ui: &mut GameUi, frames: u32) {
        for _ in 0..frames {
            app.run_headless(1, 1000.0 / 60.0);
            ui.frame(&mut app.world);
        }
    }

    #[test]
    fn commands_parse() {
        assert_eq!(UiCmd::from_click("start2"), Some(UiCmd::Start(2)));
        assert_eq!(UiCmd::from_click("bind:p1_drift"), Some(UiCmd::Bind("p1_drift".into())));
        assert_eq!(UiCmd::from_click("nope"), None);
        assert_eq!(UiCmd::from_change("volume", "40", false), Some(UiCmd::Volume(0.4)));
        assert_eq!(UiCmd::from_change("fx:bloom", "on", true), Some(UiCmd::Effect("bloom".into(), true)));
        assert_eq!(UiCmd::from_change("tier", "low", false), Some(UiCmd::Tier("low".into())));
        assert_eq!(UiCmd::from_change("what", "", false), None);
    }

    #[test]
    fn every_item_has_an_icon() {
        for it in evani_kit_kart_racer::items::Item::ALL {
            let svg = item_icon(it.id());
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{}", it.id());
        }
        assert_eq!(item_icon("nope"), "");
    }

    #[test]
    fn the_results_wait_for_the_podium_scene() {
        let mut hud = RaceHud { phase: Phase::Finished, players: 1, ..RaceHud::default() };
        assert_eq!(screen(&hud), Screen::Race, "celebrating");
        hud.paused = true;
        assert_eq!(screen(&hud), Screen::Paused);
        hud.paused = false;
        hud.podium = true;
        assert_eq!(screen(&hud), Screen::Results);
        assert_eq!(screen(&RaceHud { phase: Phase::Racing, ..RaceHud::default() }), Screen::Race);
    }

    #[test]
    fn over_the_line_the_hud_says_finish_in_the_final_lap_style_with_the_place_big() {
        let hud = RaceHud::default();
        let mut h = PlayerHud { active: true, lap: 3, laps: 3, final_lap: true, place: 7, racers: 8, lap_flash: 0.2, ..PlayerHud::default() };
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.mid.as_str(), v.big.as_str()), ("FINAL LAP!", ""));
        h.finished = Some(88.5);
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.mid.as_str(), v.mid_class), ("FINISH!", "mid ink final banner finish"));
        assert_eq!((v.big.as_str(), v.time.as_str()), ("7th", "1:28.50"));
        // The countdown still wins the middle of the screen on a new race.
        let on_grid = RaceHud { countdown: 3, ..RaceHud::default() };
        assert_eq!(hud_view(0, &h, &on_grid).mid, "3");
    }

    #[test]
    fn wrong_way_rides_the_banner_band_the_page_styles_clear_of_the_marker() {
        let hud = RaceHud { since_go: 5.0, ..RaceHud::default() };
        let h = PlayerHud { active: true, lap: 1, laps: 3, wrong_way: true, ..PlayerHud::default() };
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.mid.as_str(), v.mid_class), ("WRONG WAY", "mid ink warn wrong"));
        // The page's `.wrong` rule sets its own top (the hud_wrong_top tunable, 19 u by
        // default: under the item slot and its warning, above the marker's ~37 % line)
        // and not the centred 38 % line.
        let css = include_str!("../hud.css");
        let rule = css.split(".hud .mid.wrong {").nth(1).and_then(|r| r.split('}').next()).expect("a .mid.wrong rule");
        assert!(rule.contains("top: calc(var(--u) * var(--hud-wrong-top, 19))") && rule.contains("margin-top: 0"), "{rule}");
        assert!(css.contains("#ui.two .hud .mid.wrong"), "a short split-screen half needs its own place");
        assert!(include_str!("../index.html").contains("<link data-trunk rel=\"inline\" type=\"css\" href=\"hud.css\">"));
    }

    /// Every `var(--hud-*, N)` in the HUD stylesheet falls back to the kit's
    /// code default, and every layout value the page uses appears there.
    #[test]
    fn the_hud_stylesheet_falls_back_to_the_layout_defaults() {
        use evani_kit_kart_racer::hud_layout::HudLayout;
        let css = include_str!("../hud.css");
        let vars = HudLayout::default().css_vars();
        let mut seen = 0;
        for use_ in css.split("var(--hud-").skip(1) {
            let (name, rest) = use_.split_once(',').expect("a fallback");
            let fallback: f32 = rest.split(')').next().unwrap().trim().parse().expect("a number");
            let name = format!("--hud-{name}");
            let (_, def) = vars.iter().find(|(v, _)| *v == name).unwrap_or_else(|| panic!("{name} isn't a layout tunable"));
            assert_eq!(fallback, *def, "{name}");
            seen += 1;
        }
        for (v, _) in &vars {
            assert!(css.contains(&format!("var({v},")), "{v} unused by the page");
        }
        assert!(seen >= vars.len());
        // The game writes them on #ui; a changed value is a changed attribute.
        let moved = HudLayout { wrong_top: 30.0, ..HudLayout::default() };
        assert!(moved.style_attr().contains("--hud-wrong-top:30;"));
        // The app reads the live values (a param.set) each frame.
        let a = App::new("t").with_kit(evani_kit_kart_racer::KartRacerPlugin);
        assert_eq!(hud_layout(&a.world), HudLayout::default());
        a.world.res_mut::<ParamRegistry>().set_json("kit.kart_racer.hud_wrong_top", &serde_json::json!(30.0)).unwrap();
        assert_eq!(hud_layout(&a.world), moved);
    }

    #[test]
    fn the_lap_turns_yellow_on_the_final_lap_only() {
        let hud = RaceHud::default();
        let mut h = PlayerHud { active: true, lap: 2, laps: 3, place: 4, racers: 8, ..PlayerHud::default() };
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.lap.as_str(), v.lap_class), ("Lap 2/3", "lap"));
        h.lap = 3;
        h.final_lap = true;
        let v = hud_view(0, &h, &hud);
        // `.final` is the page's yellow (#ffd23f).
        assert_eq!((v.lap.as_str(), v.lap_class), ("Lap 3/3", "lap final"));
    }

    #[test]
    fn an_overtake_pops_plus_one_on_the_position_and_restarts_for_the_next() {
        let hud = RaceHud::default();
        let mut h = PlayerHud { active: true, lap: 1, laps: 3, place: 5, racers: 8, ..PlayerHud::default() };
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.gain.as_str(), v.gain_class), ("", "gain"));
        h.track_place(4, true, 0.0);
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.gain.as_str(), v.gain_class), ("+1", "gain a"));
        // The next overtake swaps the class, so the animation plays again.
        h.track_place(3, true, 0.1);
        assert_eq!(hud_view(0, &h, &hud).gain_class, "gain b");
        // After a second it is gone.
        h.track_place(3, true, 1.0);
        let v = hud_view(0, &h, &hud);
        assert_eq!((v.gain.as_str(), v.gain_class), ("", "gain"));
    }

    #[test]
    fn colours_become_css_hex_and_pip_is_pink() {
        assert_eq!(css_hex([1.0, 0.078, 0.366]), "#ff4fa3");
        assert_eq!(css_hex([0.0, 0.0, 0.0]), "#000000");
        assert_eq!(css_hex([1.0, 1.0, 1.0]), "#ffffff");
    }

    #[test]
    fn results_rows_carry_their_kart_colours() {
        let mut hud = RaceHud { phase: Phase::Finished, players: 1, ..RaceHud::default() };
        hud.players_hud[0] = PlayerHud { active: true, place: 2, ..PlayerHud::default() };
        hud.results = vec![
            evani_kit_kart_racer::race::ResultRow { place: 1, kart: 2, name: "Taffy".into(), player: None, time: Some(60.0), color: [1.0, 0.658, 0.0] },
            evani_kit_kart_racer::race::ResultRow { place: 2, kart: 0, name: "Pip".into(), player: Some(0), time: Some(61.0), color: [1.0, 0.078, 0.366] },
        ];
        let (_, _, rows) = results_view(&hud);
        assert_eq!(rows[0].4, "#ffd400");
        assert_eq!(rows[1].4, "#ff4fa3", "Pip's row is pink");
        assert!(rows[1].3 && !rows[0].3);
    }

    #[test]
    fn title_to_race_to_pause_and_back() {
        let (mut app, mut ui) = started();
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<RaceHud>()), Screen::Title);
        ui.push(UiCmd::Start(1));
        tick(&mut app, &mut ui, 3);
        let hud = app.world.res::<RaceHud>().clone();
        assert_eq!(screen(&hud), Screen::Race);
        assert_eq!(hud.countdown, 3);
        let v = hud_view(0, &hud.players_hud[0], &hud);
        assert_eq!(v.mid, "3");
        assert_eq!(v.lap, "Lap 1/3");
        assert_eq!(v.place, "8th");
        // Settings over a race keep it paused; closing leaves the pause menu.
        ui.push(UiCmd::OpenSettings);
        tick(&mut app, &mut ui, 3);
        assert!(app.world.res::<Race>().is_paused());
        ui.push(UiCmd::CloseSettings);
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<RaceHud>()), Screen::Paused);
        ui.push(UiCmd::Resume);
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<RaceHud>()), Screen::Race);
        ui.push(UiCmd::Title);
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<RaceHud>()), Screen::Title);
    }

    /// Free roam: no title, no countdown, and the HUD drops the place, the
    /// lap and the timer for a FREE ROAM label (the controls line under it
    /// goes away after a few seconds).
    #[test]
    fn free_roam_hud_has_a_label_and_no_race_numbers() {
        use evani_core::play::PlayMode;
        let mut app = build_app_in(PlayMode::FreeRoam);
        app.start();
        let mut ui = GameUi::new();
        ui.start(&mut app.world);
        tick(&mut app, &mut ui, 3);
        let hud = app.world.res::<RaceHud>().clone();
        assert_eq!(screen(&hud), Screen::Race);
        assert!(hud.free_roam && hud.countdown == 0 && hud.players == 1);
        let v = hud_view(0, &hud.players_hud[0], &hud);
        assert!(v.free_roam);
        assert_eq!((v.mid.as_str(), v.mid_class), ("", "mid ink"), "no countdown or GO");
        assert!(v.hint.contains("Shift drift"), "{}", v.hint);
        // The label and the hiding rules are in the page.
        let css = include_str!("../hud.css");
        assert!(css.contains(".hud.free .place, .hud.free .lap, .hud.free .tr { display: none; }"));
        assert!(css.contains(".hud.free .roam { display: block; }"));
        // Later the controls line goes.
        tick(&mut app, &mut ui, 60 * 13);
        let hud = app.world.res::<RaceHud>().clone();
        assert_eq!(screen(&hud), Screen::Race);
        assert_eq!(hud_view(0, &hud.players_hud[0], &hud).hint, "");
        // Pause and "quit to title" stay in the world (a fresh start).
        ui.push(UiCmd::Title);
        tick(&mut app, &mut ui, 3);
        assert_eq!(screen(&app.world.res::<RaceHud>()), Screen::Race);
        // The Play-mode HUD is untouched.
        let (mut play, mut pui) = started();
        tick(&mut play, &mut pui, 2);
        pui.push(UiCmd::Start(1));
        tick(&mut play, &mut pui, 3);
        let hud = play.world.res::<RaceHud>().clone();
        let v = hud_view(0, &hud.players_hud[0], &hud);
        assert!(!v.free_roam && v.hint.is_empty() && v.mid == "3" && v.lap == "Lap 1/3");
    }

    #[test]
    fn rebinding_a_key_and_steering_pair_saves_and_resets() {
        let (mut app, mut ui) = started();
        ui.push(UiCmd::OpenSettings);
        ui.push(UiCmd::Bind("p1_drift".into()));
        tick(&mut app, &mut ui, 1);
        assert_eq!(ui.modal.get(), 2);
        ui.push(UiCmd::Key("KeyJ".into()));
        ui.push(UiCmd::Bind("p1_steer".into()));
        ui.push(UiCmd::Key("KeyQ".into()));
        tick(&mut app, &mut ui, 1);
        assert!(ui.pending.as_ref().is_some_and(|p| p.first.as_deref() == Some("KeyQ")));
        ui.push(UiCmd::Key("KeyR".into()));
        tick(&mut app, &mut ui, 1);
        assert_eq!(ui.modal.get(), 1);
        {
            let input = app.world.res::<Input>();
            let label = |n: &str| binding_label(&input.actions()[input.action_id(n).unwrap().0 as usize].bindings);
            assert_eq!(label("p1_drift"), "J");
            assert_eq!(label("p1_steer"), "Q / R");
            assert_eq!(label("p2_drift"), "Right Shift");
        }
        let saved = app.world.res::<SaveData>().load_raw(KEY_BINDINGS).unwrap().unwrap();
        assert!(saved.contains("key:KeyJ"), "{saved}");
        // Esc cancels a rebinding without changing anything.
        ui.push(UiCmd::Bind("p1_item".into()));
        ui.push(UiCmd::Key("Escape".into()));
        ui.push(UiCmd::ResetBindings);
        tick(&mut app, &mut ui, 1);
        let input = app.world.res::<Input>();
        let id = input.action_id("p1_drift").unwrap();
        assert_eq!(binding_label(&input.actions()[id.0 as usize].bindings), "Left Shift");
        let id = input.action_id("p1_item").unwrap();
        assert_eq!(binding_label(&input.actions()[id.0 as usize].bindings), "E");
    }

    #[test]
    fn vr_tier_is_refused_outside_an_xr_session() {
        let (mut app, mut ui) = started();
        let before = app.world.res::<Tiers>().active().id.clone();
        let errors = app.world.res::<ErrorLog>().error_count();
        ui.push(UiCmd::Tier("vr".into()));
        tick(&mut app, &mut ui, 1);
        assert_eq!(app.world.res::<Tiers>().active().id, before);
        assert!(app.world.res::<ErrorLog>().error_count() > errors);
        assert_ne!(app.world.res::<SaveData>().load_raw(SAVED_TIER_KEY).unwrap().as_deref(), Some("vr"));
        assert_eq!(UiCmd::from_click("enter-vr"), Some(UiCmd::EnterVr));
    }

    #[test]
    fn a_good_start_says_so() {
        let h = PlayerHud { good_start: true, boosting: true, ..PlayerHud::default() };
        let v = hud_view(0, &h, &RaceHud::default());
        assert_eq!((v.cue.as_str(), v.drift.as_str()), ("GOOD START!", "BOOST!"));
        let h = PlayerHud { boosting: true, ..PlayerHud::default() };
        let v = hud_view(0, &h, &RaceHud::default());
        assert_eq!((v.cue.as_str(), v.drift.as_str()), ("", "BOOST!"));
    }

    #[test]
    fn an_item_coming_at_the_player_puts_a_warning_under_the_item_slot() {
        use evani_kit_kart_racer::threat::{Threat, ThreatKind};
        let quiet = hud_view(0, &PlayerHud::default(), &RaceHud::default());
        assert_eq!((quiet.threat.as_str(), quiet.threat_class), ("", "threat ink"));
        let at = |kind, level| {
            let h = PlayerHud { threat: Some(Threat { kind, level }), ..PlayerHud::default() };
            hud_view(0, &h, &RaceHud::default())
        };
        let far = at(ThreatKind::Seeker, 0.15);
        assert_eq!((far.threat.as_str(), far.threat_class), ("SEEKER INCOMING!", "threat ink warn l1"));
        assert_eq!(at(ThreatKind::Bomb, 0.5).threat_class, "threat ink warn l2");
        let close = at(ThreatKind::Bomb, 0.9);
        assert_eq!((close.threat.as_str(), close.threat_class), ("BOMB INCOMING!", "threat ink warn l3"));
    }

    #[test]
    fn a_held_capture_hides_the_overlay_and_gives_it_back() {
        // The app runs its own GameUi module; drive it through the app only.
        let mut app = build_app();
        app.start();
        app.world.res_mut::<Overlay>().enabled = true;
        app.run_headless(1, 1000.0 / 60.0);
        app.world.res_mut::<evani_core::capture::CaptureQueue>().sim_time_override = Some(3.0);
        app.run_headless(1, 1000.0 / 60.0);
        assert!(!app.world.res::<Overlay>().enabled, "hidden while held");
        app.world.res_mut::<evani_core::capture::CaptureQueue>().sim_time_override = None;
        app.run_headless(1, 1000.0 / 60.0);
        assert!(app.world.res::<Overlay>().enabled, "back after the hold");
    }

    #[test]
    fn tier_volume_overlay_and_effects_apply_and_save() {
        let (mut app, mut ui) = started();
        assert!(!app.world.res::<Overlay>().enabled, "stats overlay starts hidden");
        ui.push(UiCmd::Tier("low".into()));
        ui.push(UiCmd::Volume(0.25));
        ui.push(UiCmd::Overlay(true));
        tick(&mut app, &mut ui, 2);
        assert_eq!(app.world.res::<Tiers>().active().id, "low");
        assert!((app.world.res::<AudioMixer>().volume(None) - 0.25).abs() < 1e-6);
        assert!(app.world.res::<Overlay>().enabled);
        let save = app.world.res::<SaveData>();
        assert_eq!(save.load_raw(SAVED_TIER_KEY).unwrap().as_deref(), Some("low"));
        assert_eq!(save.load_raw(KEY_VOLUME).unwrap().as_deref(), Some("0.25"));
        drop(save);
        let fx = GameUi::effects(&app.world);
        assert_eq!(fx.iter().any(|(n, ..)| n == "outline"), cfg!(feature = "outline"));
        #[cfg(feature = "speedlines")]
        {
            ui.push(UiCmd::Effect("speedlines".into(), false));
            tick(&mut app, &mut ui, 1);
            let fx = GameUi::effects(&app.world);
            assert!(fx.iter().any(|(n, on, _)| n == "speedlines" && !on));
        }
    }

    /// Done-when 2 end to end: title, countdown, a whole race driven only
    /// through key presses (the AI's own driver decides which keys player 1
    /// holds), then the results screen with a win or a loss.
    #[test]
    fn a_keyboard_player_races_from_title_to_a_result() {
        use evani_core::input::{KeyCode, RawEvent};
        use evani_kit_kart_racer::items::Slot;
        use evani_kit_kart_racer::{KartKit, ai};
        let (mut app, mut ui) = started();
        tick(&mut app, &mut ui, 2);
        ui.push(UiCmd::Start(1));
        tick(&mut app, &mut ui, 2);
        const KEYS: [&str; 6] = ["KeyA", "KeyD", "KeyW", "KeyS", "ShiftLeft", "KeyE"];
        let mut held = [false; 6];
        let mut brain = ai::AiState::default();
        let (mut saw_tier, mut saw_item, mut saw_go) = (0u8, false, false);
        let mut ticks = 0;
        while ticks < 60 * 400 {
            let want = {
                let race = evani_kit_kart_racer::race::RaceView::new(&app.world);
                if race.phase == Phase::Finished {
                    break;
                }
                let i = race.player_kart(0).unwrap();
                let kit = *app.world.res::<KartKit>();
                let t = kit.tune.read(&app.world.res::<ParamRegistry>());
                let around = ai::surroundings(&race.karts, i, &race.track);
                let k = &race.karts[i];
                let mut c = ai::drive(k, &race.roster[k.spec], &mut brain, around, &race.track, &t, 0.5, 1.0 / 60.0);
                // Like a person, aim for a free item box just ahead when the
                // slot is empty (the AI line alone often runs through the
                // boxes the karts in front have just taken).
                let (fx, fz) = (k.yaw.sin(), k.yaw.cos());
                let free_box = race
                    .boxes
                    .iter()
                    .filter(|b| b.hidden <= 0.0)
                    .map(|b| (b.pos[0] - k.pos[0], b.pos[2] - k.pos[2]))
                    .filter(|&(dx, dz)| {
                        let ahead = dx * fx + dz * fz;
                        (2.0..25.0).contains(&ahead) && (dx * fz - dz * fx).abs() < ahead
                    })
                    .min_by(|a, b| (a.0.hypot(a.1)).total_cmp(&b.0.hypot(b.1)));
                if let (Some((dx, dz)), true, false) = (free_box, k.slot == Slot::Empty, c.drift) {
                    let mut err = dx.atan2(dz) - k.yaw;
                    while err > core::f32::consts::PI {
                        err -= core::f32::consts::TAU;
                    }
                    while err < -core::f32::consts::PI {
                        err += core::f32::consts::TAU;
                    }
                    // err > 0 wants the yaw up: steer left (negative).
                    c.steer = if err.abs() < 0.05 { 0.0 } else { -err.signum() };
                }
                [c.steer < -0.25, c.steer > 0.25, c.throttle > 0.5, c.brake > 0.5, c.drift, c.item]
            };
            for (j, code) in KEYS.iter().enumerate() {
                if want[j] != held[j] {
                    held[j] = want[j];
                    app.world.res_mut::<Input>().handle(&RawEvent::Key {
                        code: KeyCode::new(code).unwrap(),
                        down: want[j],
                    });
                }
            }
            tick(&mut app, &mut ui, 1);
            ticks += 1;
            let hud = app.world.res::<RaceHud>();
            let h = &hud.players_hud[0];
            saw_tier = saw_tier.max(h.drift_tier);
            saw_item |= h.item.is_some();
            saw_go |= hud_view(0, h, &hud).mid == "GO!";
        }
        // Over the line: the celebration is the race screen with the FINISH!
        // banner and the big place; the podium and the results panel follow.
        {
            let hud = app.world.res::<RaceHud>().clone();
            assert_eq!(screen(&hud), Screen::Race, "the celebration comes first");
            let me = &hud.players_hud[0];
            let v = hud_view(0, me, &hud);
            assert_eq!((v.mid.as_str(), v.mid_class), ("FINISH!", "mid ink final banner finish"));
            assert_eq!(v.big, ordinal(me.place));
        }
        let mut extra = 0;
        while !app.world.res::<RaceHud>().podium && extra < 60 * 10 {
            tick(&mut app, &mut ui, 1);
            extra += 1;
        }
        let hud = app.world.res::<RaceHud>().clone();
        assert_eq!(screen(&hud), Screen::Results, "no result after {ticks} ticks");
        assert!(saw_go && saw_item && saw_tier >= 1, "go {saw_go} item {saw_item} tier {saw_tier}");
        let me = &hud.players_hud[0];
        assert!(me.finished.is_some());
        let (head, sub, rows) = results_view(&hud);
        assert_eq!(head == "Race lost", !me.won, "{head}: {sub}");
        assert_eq!(rows.len(), 8);
        assert_eq!(rows.iter().filter(|r| r.3).count(), 1);
        // Race again goes straight to a new countdown.
        ui.push(UiCmd::Restart);
        tick(&mut app, &mut ui, 2);
        assert_eq!(app.world.res::<RaceHud>().countdown, 3);
    }

    #[test]
    fn labels_read_well() {
        assert_eq!(key_label("KeyA"), "A");
        assert_eq!(key_label("ArrowLeft"), "Left");
        assert_eq!(key_label("ShiftRight"), "Right Shift");
        assert_eq!(key_label("ControlLeft"), "Left Ctrl");
        assert_eq!(key_label("Enter"), "Enter");
        assert_eq!(key_label("Digit3"), "3");
    }

    #[test]
    fn results_say_win_or_lose() {
        use evani_kit_kart_racer::race::ResultRow;
        let mut hud = RaceHud {
            phase: Phase::Finished,
            players: 1,
            ..RaceHud::default()
        };
        hud.players_hud[0] = PlayerHud {
            active: true,
            place: 2,
            won: true,
            finished: Some(80.0),
            ..PlayerHud::default()
        };
        hud.results = vec![
            ResultRow { place: 1, kart: 3, name: "Rival".into(), player: None, time: Some(79.0), color: [0.0, 0.479, 0.147] },
            ResultRow { place: 2, kart: 7, name: "Me".into(), player: Some(0), time: Some(80.0), color: [1.0, 0.078, 0.366] },
        ];
        let (head, _, rows) = results_view(&hud);
        assert_eq!(head, "Podium!");
        assert_eq!(rows[1], ("2nd".into(), "Me (you)".into(), "1:20.00".into(), true, "#ff4fa3".into()));
        hud.players_hud[0].place = 5;
        hud.players_hud[0].won = false;
        assert_eq!(results_view(&hud).0, "Race lost");
        hud.players = 2;
        hud.players_hud[1] = PlayerHud { active: true, place: 1, won: true, ..PlayerHud::default() };
        let (head, sub, _) = results_view(&hud);
        assert_eq!(head, "Player 2 wins the duel!");
        assert!(sub.contains("Player 1: 5th (lose)"), "{sub}");
    }

    /// A different race finishing straight after another one (the review's
    /// podium moment restaging the finish) changes the results' key, though
    /// the table is just as long; the same results keep it.
    #[test]
    fn results_key_follows_a_restaged_finish() {
        use evani_kit_kart_racer::race::ResultRow;
        let mut hud = RaceHud { phase: Phase::Finished, players: 1, ..RaceHud::default() };
        hud.players_hud[0] = PlayerHud { active: true, place: 7, won: false, ..PlayerHud::default() };
        hud.results = vec![
            ResultRow { place: 1, kart: 3, name: "Rival".into(), player: None, time: Some(27.5), color: [1.0, 0.078, 0.366] },
            ResultRow { place: 2, kart: 0, name: "Me".into(), player: Some(0), time: Some(28.0), color: [1.0, 0.078, 0.366] },
        ];
        let lost = results_key(&hud);
        assert_eq!(results_key(&hud.clone()), lost);
        hud.players_hud[0].place = 1;
        hud.players_hud[0].won = true;
        hud.results = vec![
            ResultRow { place: 1, kart: 0, name: "Me".into(), player: Some(0), time: Some(80.0), color: [1.0, 0.078, 0.366] },
            ResultRow { place: 2, kart: 3, name: "Rival".into(), player: None, time: Some(81.0), color: [1.0, 0.078, 0.366] },
        ];
        assert_ne!(results_key(&hud), lost);
        assert_eq!(results_view(&hud).0, "You win!");
    }
}
