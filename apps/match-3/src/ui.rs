//! The game's light DOM UI: title, level select, level intro card, the HUD
//! (moves, score and star bar, goal chips, pause button) placed from the
//! kit's layout, floating score text and combo callouts, pause menu,
//! settings (quality tier, effect toggles, volume, stats, key rebinding)
//! and the result screen.
//!
//! The markup and styles live in `index.html`; this module fills it in and
//! shows one screen at a time. Buttons carry `data-cmd` and inputs
//! `data-set`; one delegated listener per event type turns them into
//! [`UiCmd`]s, applied at the next frame boundary (natively the tests apply
//! the same commands). Text is written only when it changed. The kit owns
//! the game ([`Game`] takes [`Request`]s) and fills [`Match3Hud`]; the UI
//! never touches the board.
//!
//! Test hooks (read by the review script, never by the game): `#ui` carries
//! `data-phase`, `data-idle`, `data-best` and `data-worst` (the best and
//! weakest move's two piece centres in CSS pixels), `data-blast` (specials
//! going off: 1 Streak, 2 Pop Bomb, 4 Prism) and `data-cascade`, so a
//! headless browser can play through mouse or touch events like a person
//! and catch the big moments.
//!
//! Saved (SaveData): `render.tier`, `m3.volume`, `m3.overlay`,
//! `m3.bindings` and `m3.stars` (best stars per level).

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::audio::AudioMixer;
use evani_core::ecs::World;
use evani_core::effects::{BudgetStatus, EffectsStatus};
use evani_core::input::{Binding, Input};
use evani_core::overlay::{ErrorLog, Overlay};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::{EngineModule, LookData};
use evani_core::quality::SAVED_TIER_KEY;
use evani_core::save::SaveData;
use evani_core::tiers::Tiers;
use evani_kit_match_3::draw::{DEFAULT_PIECES, NAMES};
use evani_kit_match_3::game::{Game, Phase, Request};
use evani_kit_match_3::hud::{GoalView, Match3Hud};
use evani_kit_match_3::layout::Layout;

pub const KEY_VOLUME: &str = "m3.volume";
pub const KEY_OVERLAY: &str = "m3.overlay";
pub const KEY_BINDINGS: &str = "m3.bindings";
pub const KEY_STARS: &str = "m3.stars";

/// What a player can ask the UI for.
#[derive(Debug, Clone, PartialEq)]
pub enum UiCmd {
    /// Play: the first level without three stars.
    Play,
    OpenLevels,
    CloseLevels,
    Start(usize),
    Next,
    Restart,
    Title,
    Pause,
    Resume,
    OpenSettings,
    CloseSettings,
    Tier(String),
    Effect(String, bool),
    Volume(f32),
    Overlay(bool),
    Bind(String),
    Key(String),
    ResetBindings,
}

impl UiCmd {
    /// A button's `data-cmd`.
    pub fn from_click(cmd: &str) -> Option<Self> {
        Some(match cmd {
            "play" => UiCmd::Play,
            "levels" => UiCmd::OpenLevels,
            "close-levels" => UiCmd::CloseLevels,
            "next" => UiCmd::Next,
            "restart" => UiCmd::Restart,
            "title" => UiCmd::Title,
            "pause" => UiCmd::Pause,
            "resume" => UiCmd::Resume,
            "settings" => UiCmd::OpenSettings,
            "close-settings" => UiCmd::CloseSettings,
            "reset-binds" => UiCmd::ResetBindings,
            c => {
                if let Some(n) = c.strip_prefix("level:") {
                    UiCmd::Start(n.parse().ok()?)
                } else {
                    UiCmd::Bind(c.strip_prefix("bind:")?.to_string())
                }
            }
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
    Levels,
    Intro,
    Play,
    Paused,
    Won,
    Lost,
}

pub fn screen(hud: &Match3Hud, levels_open: bool) -> Screen {
    match hud.phase {
        Phase::Title if levels_open => Screen::Levels,
        Phase::Title => Screen::Title,
        _ if hud.paused => Screen::Paused,
        Phase::Intro => Screen::Intro,
        Phase::Play | Phase::Rush => Screen::Play,
        Phase::Won if hud.result.ready => Screen::Won,
        Phase::Lost if hud.result.ready => Screen::Lost,
        Phase::Won | Phase::Lost => Screen::Play,
    }
}

/// A goal as text: `Cherry 12/16`, `Frosting 5/20`, `Score 3000`.
pub fn goal_text(g: &GoalView) -> String {
    match g.kind {
        "collect" => format!("{}/{}", g.have, g.need),
        "frosting" => format!("{}/{}", g.have, g.need),
        _ => format!("{}", g.need),
    }
}

/// The intro card's goal line.
pub fn goal_sentence(goals: &[GoalView], moves: i32) -> String {
    let parts: Vec<String> = goals
        .iter()
        .map(|g| match g.kind {
            "collect" => format!("collect {} {}", g.need, NAMES.get(g.color as usize).copied().unwrap_or("pieces")),
            "frosting" => "clear all the frosting".to_string(),
            _ => format!("score {}", g.need),
        })
        .collect();
    let mut s = parts.join(" and ");
    if let Some(c) = s.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    format!("{s} in {moves} moves")
}

/// Stars as text: ★★☆.
pub fn stars_text(n: u8) -> String {
    (0..3).map(|i| if i < n { '★' } else { '☆' }).collect()
}

/// Where the title's logo and menu go, so the attract board stays clear:
/// the HUD panel's place (hidden on the title), which is the column above
/// Puff in landscape and the strip above the board in portrait. Returns the
/// box (`[x, y, w, h]`, CSS px), the title unit (`--t`, CSS px; the page
/// sizes everything from it, with minimum font sizes) and whether the keys
/// hint has room (a tall landscape column only).
pub fn title_box(l: &Layout) -> ([f32; 4], f32, bool) {
    let r = l.hud;
    let t = if l.portrait { (r[2] / 16.0).min((r[3] - 22.0) / 7.2) } else { (r[2] / 14.0).min(r[3] / 17.0) };
    (r, t.clamp(12.0, 34.0), !l.portrait && r[3] >= 440.0)
}

/// The result card's place: centred over the board's cells and at most
/// 92 % of the board wide, so it never reaches the HUD panel. Returns its
/// centre and width (CSS px) and the card unit (`--r`, CSS px; the page
/// sizes the text from it, with minimum font sizes, so it fits a phone held
/// sideways).
pub fn result_card(l: &Layout) -> ([f32; 2], f32, f32) {
    let b = l.board;
    let r = (b[2].min(b[3]) / 20.0).clamp(10.0, 40.0);
    ([b[0] + b[2] * 0.5, b[1] + b[3] * 0.5], b[2] * 0.92, r)
}

/// Linear colour to a CSS hex colour.
pub fn css_hex(c: [f32; 4]) -> String {
    let e = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0 + 0.5) as u8
    };
    format!("#{:02x}{:02x}{:02x}", e(c[0]), e(c[1]), e(c[2]))
}

/// A goal icon as inline SVG (piece shapes match the board).
pub fn icon_svg(kind: &str, color: u8, palette: &[[f32; 4]; 6]) -> String {
    let ink = "#1c0f2b";
    let body = match kind {
        "collect" => {
            let fill = css_hex(palette.get(color as usize).copied().unwrap_or([0.8; 4]));
            match color {
                0 => format!("<circle cx='12' cy='12' r='9' fill='{fill}'/>"),
                1 => format!("<rect x='3.5' y='3.5' width='17' height='17' rx='4' fill='{fill}'/>"),
                2 => format!("<path d='M12 2.5l2.9 6 6.6.9-4.8 4.6 1.2 6.5L12 17.4l-5.9 3.1 1.2-6.5L2.5 9.4l6.6-.9z' fill='{fill}'/>"),
                3 => format!("<path d='M12 2.5L21.5 12 12 21.5 2.5 12z' fill='{fill}'/>"),
                4 => format!("<path d='M12 3.5L21 20H3z' fill='{fill}'/>"),
                _ => format!("<circle cx='12' cy='12' r='9' fill='{fill}'/><circle cx='12' cy='12' r='3.6' fill='{ink}' fill-opacity='.55'/>"),
            }
        }
        "frosting" => "<rect x='3' y='5' width='18' height='15' rx='3' fill='#ff9ac4'/><path d='M3 9c3 3 4-2 6 1s4-2 6 1 4-2 6 0V7a3 3 0 0 0-3-3H6a3 3 0 0 0-3 3z' fill='#fff7fb'/>".to_string(),
        _ => "<path d='M12 2.5l2.9 6 6.6.9-4.8 4.6 1.2 6.5L12 17.4l-5.9 3.1 1.2-6.5L2.5 9.4l6.6-.9z' fill='#ffd23f'/>".to_string(),
    };
    format!("<svg viewBox='0 0 24 24' aria-hidden='true'><g stroke='{ink}' stroke-width='2' stroke-linejoin='round'>{body}</g></svg>")
}

/// A key code as a player reads it (`KeyA` → `A`, `ArrowUp` → `Up`).
pub fn key_label(code: &str) -> String {
    if let Some(k) = code.strip_prefix("Key").or_else(|| code.strip_prefix("Digit")) {
        return k.to_string();
    }
    if let Some(k) = code.strip_prefix("Arrow") {
        return k.to_string();
    }
    code.to_string()
}

/// The keyboard bindings of an action, as a label (`Up / W`).
pub fn binding_label(bindings: &[Binding]) -> String {
    let keys: Vec<String> = bindings
        .iter()
        .filter_map(|b| match b {
            Binding::Key(k) => Some(key_label(k.as_str())),
            _ => None,
        })
        .collect();
    if keys.is_empty() { "none".to_string() } else { keys.join(" / ") }
}

/// Rebindable actions and their labels.
pub const BIND_ROWS: [(&str, &str); 7] = [
    ("up", "Cursor up"),
    ("down", "Cursor down"),
    ("left", "Cursor left"),
    ("right", "Cursor right"),
    ("pick", "Pick up a piece"),
    ("cancel", "Put it down"),
    ("pause", "Pause"),
];

/// A player-facing effect name.
pub fn effect_label(name: &str) -> &str {
    match name {
        "juice" => "Bouncy animation",
        "particles" => "Sparkles",
        "shake" => "Screen shake",
        "bloom" => "Glow",
        "vignette" => "Dark corners",
        other => other,
    }
}

pub type CmdQueue = Rc<RefCell<Vec<UiCmd>>>;

/// What the page-level key listener must do: 0 nothing, 1 Esc closes
/// an overlay, 2 every key goes to the rebinding.
pub type Modal = Rc<Cell<u8>>;

#[derive(Default)]
pub struct GameUi {
    queue: CmdQueue,
    modal: Modal,
    pub settings_open: bool,
    pub levels_open: bool,
    /// Rebinding in progress: the action name.
    pub pending: Option<String>,
    defaults: String,
    pub settings_rev: u64,
    /// Best stars per level.
    pub best: Vec<u8>,
    /// The result of this run was saved.
    saved_run: u32,
    #[cfg(target_arch = "wasm32")]
    dom: Option<dom::Dom>,
}

impl GameUi {
    pub fn new() -> Self {
        Self::default()
    }

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
        if let Some(mut g) = world.try_res_mut::<Game>() {
            g.request(r);
        }
    }

    /// Apply one command.
    pub fn apply(&mut self, world: &World, c: UiCmd) {
        match c {
            UiCmd::Play => {
                self.settings_open = false;
                self.levels_open = false;
                let n = world.res::<Match3Hud>().levels.max(1);
                let first = (0..n).find(|&i| self.best.get(i).copied().unwrap_or(0) == 0).unwrap_or(0);
                Self::request(world, Request::Start(first));
            }
            UiCmd::OpenLevels => self.levels_open = true,
            UiCmd::CloseLevels => self.levels_open = false,
            UiCmd::Start(n) => {
                self.settings_open = false;
                self.levels_open = false;
                Self::request(world, Request::Start(n));
            }
            UiCmd::Next => Self::request(world, Request::Next),
            UiCmd::Restart => {
                self.settings_open = false;
                Self::request(world, Request::Restart);
            }
            UiCmd::Title => {
                self.settings_open = false;
                self.levels_open = false;
                Self::request(world, Request::Title);
            }
            UiCmd::Pause => Self::request(world, Request::Pause(true)),
            UiCmd::Resume => Self::request(world, Request::Pause(false)),
            UiCmd::OpenSettings => self.settings_open = true,
            UiCmd::CloseSettings => {
                self.settings_open = false;
                self.pending = None;
            }
            UiCmd::Tier(id) => {
                if world.res_mut::<Tiers>().set_active(&id) {
                    Self::save(world, SAVED_TIER_KEY, &id);
                } else {
                    world.res_mut::<ErrorLog>().error(format!("ui: no tier {id}"));
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
                // fixed(): the same text as {v:.2} without float formatting code.
                Self::save(world, KEY_VOLUME, &evani_core::num::fixed(v, 2).to_string());
            }
            UiCmd::Overlay(on) => {
                world.res_mut::<Overlay>().enabled = on;
                Self::save(world, KEY_OVERLAY, if on { "1" } else { "0" });
                self.settings_rev += 1;
            }
            UiCmd::Bind(action) => {
                self.pending = Some(action);
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
        }
    }

    /// A key arrived while rebinding: it replaces the action's first key.
    fn key(&mut self, world: &World, code: String) {
        let Some(action) = self.pending.take() else {
            return;
        };
        self.settings_rev += 1;
        if code == "Escape" {
            return;
        }
        let Ok(binding) = format!("key:{code}").parse::<Binding>() else {
            world.res_mut::<ErrorLog>().note(format!("ui: cannot bind {code}"));
            return;
        };
        let json = {
            let mut input = world.res_mut::<Input>();
            let Some(id) = input.action_id(&action) else {
                return;
            };
            input.rebind(id, 0, binding);
            input.bindings_json()
        };
        Self::save(world, KEY_BINDINGS, &json);
    }

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
        world.res_mut::<Overlay>().enabled = Self::load(world, KEY_OVERLAY).as_deref() == Some("1");
        self.best = Self::load(world, KEY_STARS)
            .map(|s| s.split(',').filter_map(|v| v.trim().parse::<u8>().ok()).map(|v| v.min(3)).collect())
            .unwrap_or_default();
    }

    /// Remember the best stars when a level is won.
    fn record_result(&mut self, world: &World) {
        let hud = world.res::<Match3Hud>();
        if hud.phase != Phase::Won || !hud.result.ready || self.saved_run == hud.runs {
            return;
        }
        self.saved_run = hud.runs;
        let (lv, stars) = (hud.level, hud.result.stars);
        drop(hud);
        if self.best.len() <= lv {
            self.best.resize(lv + 1, 0);
        }
        if stars > self.best[lv] {
            self.best[lv] = stars;
            let s: Vec<String> = self.best.iter().map(|v| v.to_string()).collect();
            Self::save(world, KEY_STARS, &s.join(","));
        }
    }

    /// Keep the game paused while settings are open over it.
    fn hold_pause(&self, world: &World) {
        if !self.settings_open {
            return;
        }
        let hud = world.res::<Match3Hud>();
        if matches!(hud.phase, Phase::Intro | Phase::Play | Phase::Rush) && !hud.paused {
            drop(hud);
            Self::request(world, Request::Pause(true));
        }
    }

    fn sync_modal(&self) {
        self.modal.set(match (self.settings_open || self.levels_open, &self.pending) {
            (_, Some(_)) => 2,
            (true, None) => 1,
            _ => 0,
        });
    }

    pub fn settings_key(&self, world: &World) -> (u64, usize, BudgetStatus, u64) {
        (
            world.res::<ParamRegistry>().revision(),
            world.res::<Tiers>().active_index(),
            *world.res::<BudgetStatus>(),
            self.settings_rev,
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

    /// The piece colours the board uses (the look's, else the kit's).
    pub fn palette(world: &World) -> [[f32; 4]; 6] {
        let mut p = DEFAULT_PIECES;
        if let Some(look) = world.try_res::<LookData>()
            && let Some(pal) = look.palette("pieces")
        {
            for (d, c) in p.iter_mut().zip(&pal.colors) {
                *d = *c;
            }
        }
        p
    }
}

impl EngineModule for GameUi {
    fn id(&self) -> &'static str {
        "evani.module.match3_ui"
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
        self.record_result(world);
        self.sync_modal();
        #[cfg(target_arch = "wasm32")]
        if let Some(mut d) = self.dom.take() {
            d.sync(world, self);
            self.dom = Some(d);
        }
    }

    fn visibility_changed(&mut self, world: &mut World, hidden: bool) {
        if hidden {
            Self::request(world, Request::Pause(true));
        }
    }

    fn status(&self, world: &World, out: &mut String) {
        let hud = world.res::<Match3Hud>();
        out.push_str(hud.phase.as_str());
        if hud.paused {
            out.push_str(" · paused");
        }
        if self.settings_open {
            out.push_str(" · settings");
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod dom {
    use evani_core::audio::AudioMixer;
    use evani_core::ecs::World;
    use evani_core::input::Input;
    use evani_core::num::fixed;
    use evani_core::overlay::Overlay;
    use evani_core::tiers::Tiers;
    use evani_kit_match_3::game::Phase;
    use evani_kit_match_3::hud::Match3Hud;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

    use super::{
        BIND_ROWS, CmdQueue, GameUi, Modal, Screen, UiCmd, binding_label, effect_label, goal_sentence, goal_text,
        icon_svg, result_card, screen, stars_text, title_box,
    };

    /// One element whose text and class are written only on change.
    struct Text {
        el: Element,
        text: String,
        class: String,
    }

    impl Text {
        fn new(el: Element) -> Self {
            Self {
                el,
                text: String::new(),
                class: String::new(),
            }
        }

        fn set(&mut self, s: &str) {
            if self.text != s {
                self.text.clear();
                self.text.push_str(s);
                self.el.set_text_content(Some(s));
            }
        }

        fn class(&mut self, c: &str) {
            if self.class != c {
                self.class.clear();
                self.class.push_str(c);
                self.el.set_class_name(c);
            }
        }

        fn html(&mut self, s: &str) {
            if self.text != s {
                self.text.clear();
                self.text.push_str(s);
                self.el.set_inner_html(s);
            }
        }

        fn style(&mut self, s: &str) {
            if self.class != s {
                self.class.clear();
                self.class.push_str(s);
                let _ = self.el.set_attribute("style", s);
            }
        }
    }

    const FLOATS: usize = 24;

    pub struct Dom {
        doc: Document,
        ui: Element,
        hud: Text,
        hud_body: Text,
        moves: Text,
        score: Text,
        stars: Text,
        bar: Text,
        marks: Text,
        level: Text,
        goals: Text,
        banner: Text,
        floats: Vec<(Text, Text)>,
        title: Element,
        title_box: (Text, Text),
        levels: Element,
        levels_list: Text,
        intro: Element,
        intro_name: Text,
        intro_goal: Text,
        pause: Element,
        result: Element,
        res_dim: Text,
        res_card: Text,
        res_head: Text,
        res_stars: Text,
        res_sub: Text,
        res_next: Element,
        settings: Element,
        settings_body: Element,
        shown: Option<(Screen, bool)>,
        settings_seen: Option<(u64, usize, evani_core::effects::BudgetStatus, u64)>,
        hooks: (Text, Text, Text, Text, Text, Text),
        goals_seen: Vec<evani_kit_match_3::hud::GoalView>,
        palette: [[f32; 4]; 6],
        levels_seen: Vec<u8>,
    }

    fn by_id(doc: &Document, id: &str) -> Result<Element, String> {
        doc.get_element_by_id(id).ok_or_else(|| format!("index.html has no #{id}"))
    }

    fn text(doc: &Document, id: &str) -> Result<Text, String> {
        by_id(doc, id).map(Text::new)
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
                cb.forget();
            }
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
                            q.borrow_mut().push(UiCmd::CloseLevels);
                        }
                        _ => {}
                    }
                });
                let _ = win.add_event_listener_with_callback_and_bool("keydown", cb.as_ref().unchecked_ref(), true);
                cb.forget();
            }
            // Floating text pool.
            let layer = by_id(&doc, "floats")?;
            let mut floats = Vec::with_capacity(FLOATS);
            for _ in 0..FLOATS {
                let el = doc.create_element("div").map_err(|_| "create div")?;
                el.set_class_name("float");
                let _ = layer.append_child(&el);
                floats.push((Text::new(el.clone()), Text::new(el)));
            }
            Ok(Self {
                hud: text(&doc, "hud")?,
                hud_body: text(&doc, "hud-body")?,
                moves: text(&doc, "moves")?,
                score: text(&doc, "score")?,
                stars: text(&doc, "hud-stars")?,
                bar: text(&doc, "bar-fill")?,
                marks: text(&doc, "bar-marks")?,
                level: text(&doc, "level-name")?,
                goals: text(&doc, "goals")?,
                banner: text(&doc, "banner")?,
                floats,
                title: by_id(&doc, "title")?,
                title_box: (text(&doc, "title-box")?, text(&doc, "title-box")?),
                levels: by_id(&doc, "levels")?,
                levels_list: text(&doc, "levels-list")?,
                intro: by_id(&doc, "intro")?,
                intro_name: text(&doc, "intro-name")?,
                intro_goal: text(&doc, "intro-goal")?,
                pause: by_id(&doc, "pause")?,
                result: by_id(&doc, "result")?,
                res_dim: text(&doc, "res-dim")?,
                res_card: text(&doc, "res-card")?,
                res_head: text(&doc, "res-head")?,
                res_stars: text(&doc, "res-stars")?,
                res_sub: text(&doc, "res-sub")?,
                res_next: by_id(&doc, "res-next")?,
                settings: by_id(&doc, "settings")?,
                settings_body: by_id(&doc, "settings-body")?,
                hooks: (
                    Text::new(ui.clone()),
                    Text::new(ui.clone()),
                    Text::new(ui.clone()),
                    Text::new(ui.clone()),
                    Text::new(ui.clone()),
                    Text::new(ui.clone()),
                ),
                doc,
                ui,
                shown: None,
                settings_seen: None,
                goals_seen: Vec::new(),
                palette: evani_kit_match_3::draw::DEFAULT_PIECES,
                levels_seen: vec![u8::MAX],
            })
        }

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

        fn hook(t: &mut Text, name: &str, v: &str) {
            if t.text != v {
                t.text.clear();
                t.text.push_str(v);
                let _ = t.el.set_attribute(name, v);
            }
        }

        pub fn sync(&mut self, world: &World, ui: &GameUi) {
            if self.goals_seen.is_empty() {
                self.palette = GameUi::palette(world);
            }
            let hud = world.res::<Match3Hud>();
            let scr = screen(&hud, ui.levels_open);
            let now = (scr, ui.settings_open);
            if self.shown != Some(now) {
                self.shown = Some(now);
                self.blur();
                let over = ui.settings_open;
                set_on(&self.title, "screen", scr == Screen::Title && !over);
                set_on(&self.levels, "screen dim", scr == Screen::Levels && !over);
                set_on(&self.intro, "card", scr == Screen::Intro && !over);
                set_on(&self.pause, "screen dim", scr == Screen::Paused && !over);
                set_on(&self.result, "screen dim", matches!(scr, Screen::Won | Screen::Lost) && !over);
                set_on(&self.settings, "screen dim", over);
                if !over {
                    match scr {
                        Screen::Title => self.focus_first(&self.title),
                        Screen::Levels => self.focus_first(&self.levels),
                        Screen::Paused => self.focus_first(&self.pause),
                        Screen::Won | Screen::Lost => self.focus_first(&self.result),
                        _ => {}
                    }
                } else {
                    self.settings_seen = None;
                }
            }
            // Test hooks.
            Self::hook(&mut self.hooks.0, "data-phase", if hud.paused { "paused" } else { hud.phase.as_str() });
            Self::hook(&mut self.hooks.1, "data-idle", if hud.idle { "1" } else { "0" });
            // Numbers through num::fixed: the same text as {:.N}, without
            // shipping core's float formatting code.
            let best = hud.best.map_or(String::new(), |b| format!("{},{},{},{}", fixed(b[0], 0), fixed(b[1], 0), fixed(b[2], 0), fixed(b[3], 0)));
            Self::hook(&mut self.hooks.2, "data-best", &best);
            let worst = hud.worst.map_or(String::new(), |b| format!("{},{},{},{}", fixed(b[0], 0), fixed(b[1], 0), fixed(b[2], 0), fixed(b[3], 0)));
            Self::hook(&mut self.hooks.3, "data-worst", &worst);
            Self::hook(&mut self.hooks.4, "data-blast", &hud.blasting.to_string());
            Self::hook(&mut self.hooks.5, "data-cascade", &hud.cascade.to_string());
            // HUD: shown in play (also under pause, intro and the result).
            let in_level = !matches!(hud.phase, Phase::Title);
            let l = &hud.layout;
            let unit = l.hud_unit();
            // While the result is up the panel stays above its dim (its
            // pause button hidden: the level is over).
            let resulting = matches!(scr, Screen::Won | Screen::Lost) && !ui.settings_open;
            let style = format!(
                "left:{}px;top:{}px;width:{}px;height:{}px;--u:{}px{}",
                fixed(l.hud[0], 0),
                fixed(l.hud[1], 0),
                fixed(l.hud[2], 0),
                fixed(l.hud[3], 0),
                fixed(unit, 1),
                if resulting { ";z-index:2" } else { "" }
            );
            self.hud.style(&style);
            self.hud_body.class(match (in_level, l.portrait, resulting) {
                (false, _, _) => "hud-body",
                (true, true, false) => "hud-body on portrait",
                (true, true, true) => "hud-body on portrait result",
                (true, false, false) => "hud-body on",
                (true, false, true) => "hud-body on result",
            });
            if scr == Screen::Title {
                let (r, t, hint) = title_box(l);
                self.title_box.0.style(&format!(
                    "left:{}px;top:{}px;width:{}px;height:{}px;--t:{}px",
                    fixed(r[0], 0),
                    fixed(r[1], 0),
                    fixed(r[2], 0),
                    fixed(r[3], 0),
                    fixed(t, 1)
                ));
                self.title_box.1.class(match (l.portrait, hint) {
                    (true, _) => "title-box portrait",
                    (false, true) => "title-box hint",
                    (false, false) => "title-box",
                });
            }
            if resulting {
                let (c, w, r) = result_card(l);
                // The dim covers the board and its tray rim only (about 1 % of the board's width).
                let b = l.board;
                let p = b[2] * 0.012;
                self.res_dim.style(&format!(
                    "left:{}px;top:{}px;width:{}px;height:{}px",
                    fixed(b[0] - p, 0),
                    fixed(b[1] - p, 0),
                    fixed(b[2] + 2.0 * p, 0),
                    fixed(b[3] + 2.0 * p, 0)
                ));
                self.res_card.style(&format!(
                    "left:{}px;top:{}px;width:{}px;max-height:{}px;--r:{}px",
                    fixed(c[0], 0),
                    fixed(c[1], 0),
                    fixed(w, 0),
                    fixed(l.board[3], 0),
                    fixed(r, 1)
                ));
            }
            if in_level {
                self.level.set(&format!("Level {} · {}", hud.level + 1, hud.level_name));
                if hud.free_roam {
                    self.moves.set("∞");
                } else {
                    self.moves.set(&hud.moves_left.to_string());
                }
                self.score.set(&hud.score.to_string());
                self.stars.set(&stars_text(hud.stars));
                self.bar.style(&format!("width:{}%", fixed(hud.star_fill * 100.0, 1)));
                if hud.stars_at[2] > 0 {
                    let m: String = hud
                        .stars_at
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            let x = *s as f32 / hud.stars_at[2] as f32 * 100.0;
                            let lit = if (i as u8) < hud.stars { " lit" } else { "" };
                            format!("<i class='mark{lit}' style='left:{}%'>★</i>", fixed(x, 1))
                        })
                        .collect();
                    self.marks.html(&m);
                }
                if hud.goals != self.goals_seen {
                    self.goals_seen = hud.goals.clone();
                    let mut h = String::new();
                    for g in &hud.goals {
                        let done = if g.done { " done" } else { "" };
                        h.push_str(&format!(
                            "<div class='goal{done}'>{}<span>{}</span></div>",
                            icon_svg(g.kind, g.color, &self.palette),
                            if g.done { "✓".to_string() } else { escape(&goal_text(g)) }
                        ));
                    }
                    self.goals.html(&h);
                }
                let (banner, cls) = match hud.phase {
                    Phase::Rush => ("Sugar Rush!", "banner ink on"),
                    Phase::Intro => ("", "banner ink"),
                    _ if hud.moves_left <= 3 && hud.phase == Phase::Play && hud.moves_left > 0 => {
                        (if hud.moves_left == 1 { "Last move!" } else { "Few moves left!" }, "banner ink warn on")
                    }
                    _ => ("", "banner ink"),
                };
                // In portrait a combo callout shows in the banner's strip
                // (kit `hud::callout_anchor`): the banner steps aside for it.
                let calling = l.portrait && hud.floats.iter().any(|f| f.style >= 1);
                let cls = if calling { "banner ink" } else { cls };
                self.banner.set(banner);
                self.banner.class(cls);
                self.intro_name.set(&format!("Level {}: {}", hud.level + 1, hud.level_name));
                if hud.free_roam {
                    self.intro_goal.set("Free play: no goals and no move limit");
                } else {
                    self.intro_goal.set(&goal_sentence(&hud.goals, hud.moves_total));
                }
            }
            // Floating text: only in a level. The title's demo board scores
            // too, and its popups would draw over the title logo.
            for (i, (t, s)) in self.floats.iter_mut().enumerate() {
                match hud.floats.get(i).filter(|_| in_level) {
                    Some(f) => {
                        t.set(&f.text);
                        let cls = match f.style {
                            0 => "float on score",
                            1 => "float on callout ink",
                            _ => "float on callout big ink",
                        };
                        t.class(cls);
                        let k = f.age;
                        let pop = evani_kit_match_3::hud::float_scale(f.style, k);
                        let op = evani_kit_match_3::hud::float_opacity(k);
                        s.style(&format!(
                            "transform:translate({}px,{}px) translate(-50%,-50%) scale({});opacity:{};font-size:{}px",
                            fixed(f.x, 0),
                            fixed(f.y, 0),
                            fixed(pop, 2),
                            fixed(op, 2),
                            fixed(f.font, 1)
                        ));
                    }
                    None => t.class("float"),
                }
            }
            // Level list.
            if scr == Screen::Levels && self.levels_seen != ui.best {
                self.levels_seen = ui.best.clone();
                let mut h = String::new();
                for i in 0..hud.levels {
                    let st = ui.best.get(i).copied().unwrap_or(0);
                    h.push_str(&format!(
                        "<button data-cmd='level:{i}'>Level {}<small>{}</small></button>",
                        i + 1,
                        stars_text(st)
                    ));
                }
                self.levels_list.html(&h);
            }
            // Result.
            if matches!(scr, Screen::Won | Screen::Lost) {
                let r = hud.result;
                if r.won {
                    self.res_head.set("Level complete!");
                    self.res_stars.set(&stars_text(r.stars));
                    // No-break spaces keep each half whole; a narrow card breaks only after the dot.
                    let combo = if r.best_cascade >= 2 { format!("\u{a0}· best\u{a0}cascade\u{a0}x{}", r.best_cascade + 1) } else { String::new() };
                    self.res_sub.set(&format!("Score\u{a0}{}\u{a0}in\u{a0}{}\u{a0}moves{combo}", r.score, r.moves_used));
                } else {
                    self.res_head.set("Out of moves");
                    self.res_stars.set("");
                    let left: Vec<String> = hud
                        .goals
                        .iter()
                        .filter(|g| !g.done)
                        .map(|g| match g.kind {
                            "collect" => format!(
                                "{} more {}",
                                g.need - g.have,
                                evani_kit_match_3::draw::NAMES.get(g.color as usize).copied().unwrap_or("")
                            ),
                            "frosting" => format!("{} frosting left", g.need - g.have),
                            _ => format!("{} more points", g.need - g.have),
                        })
                        .collect();
                    self.res_sub.set(&format!("So close: {}. Try again?", left.join(", ")));
                }
                set_on(&self.res_next, "", r.won);
            }
            drop(hud);
            if ui.settings_open {
                let key = ui.settings_key(world);
                if self.settings_seen != Some(key) {
                    self.settings_seen = Some(key);
                    self.settings_body.set_inner_html(&settings_html(world, ui));
                }
            }
            let _ = &self.ui;
        }
    }

    fn settings_html(world: &World, ui: &GameUi) -> String {
        use core::fmt::Write;
        let mut s = String::with_capacity(4096);
        s.push_str("<h3>Quality</h3><label>Level <select data-set=\"tier\">");
        {
            let tiers = world.res::<Tiers>();
            let active = tiers.active().id.clone();
            for p in tiers.presets() {
                let sel = if p.id == active { " selected" } else { "" };
                let _ = write!(s, "<option value=\"{}\"{sel}>{}</option>", escape(&p.id), escape(&p.label));
            }
        }
        s.push_str("</select></label><h3>Effects</h3>");
        let fx = GameUi::effects(world);
        if fx.is_empty() {
            s.push_str("<p class=\"note\">No effects in this build.</p>");
        }
        for (name, wanted, state) in &fx {
            let chk = if *wanted { " checked" } else { "" };
            let note = match *state {
                "off (budget)" => " (too much for this quality level)",
                "off (vr comfort)" => " (off in VR for comfort)",
                "off (quality)" => " (paused to keep the game smooth)",
                _ => "",
            };
            let _ = write!(
                s,
                "<label><input type=\"checkbox\" data-set=\"fx:{}\"{chk}>{}{note}</label>",
                escape(name),
                effect_label(name)
            );
        }
        let vol = fixed((world.res::<AudioMixer>().volume(None) * 100.0).round(), 0);
        let _ = write!(
            s,
            "<h3>Sound</h3><label>Volume <input type=\"range\" min=\"0\" max=\"100\" value=\"{vol}\" data-set=\"volume\"></label>"
        );
        let ov = if world.res::<Overlay>().enabled { " checked" } else { "" };
        let _ = write!(s, "<label><input type=\"checkbox\" data-set=\"overlay\"{ov}>Show speed stats</label>");
        s.push_str("<h3>Controls</h3><p class=\"note\">Mouse or touch: drag a piece onto a neighbour, or tap two neighbours.</p><table class=\"binds\">");
        {
            let input = world.res::<Input>();
            for (action, label) in BIND_ROWS {
                let text = match &ui.pending {
                    Some(p) if p == action => "press a key".to_string(),
                    _ => input
                        .action_id(action)
                        .map(|id| binding_label(&input.actions()[id.0 as usize].bindings))
                        .unwrap_or_default(),
                };
                let _ = write!(s, "<tr><td>{label}</td><td><button data-cmd=\"bind:{action}\">{}</button></td></tr>", escape(&text));
            }
        }
        s.push_str(
            "</table><p class=\"note\">Click a key, then press the new one (Esc cancels). Gamepads: d-pad moves, A picks, B drops, Start pauses.</p>\
             <p><button data-cmd=\"reset-binds\" class=\"alt\">Reset controls</button></p>",
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    fn started() -> (App, GameUi) {
        let mut app = crate::build_app();
        let mut ui = GameUi::new();
        app.start();
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
        assert_eq!(UiCmd::from_click("play"), Some(UiCmd::Play));
        assert_eq!(UiCmd::from_click("level:2"), Some(UiCmd::Start(2)));
        assert_eq!(UiCmd::from_click("bind:pick"), Some(UiCmd::Bind("pick".into())));
        assert_eq!(UiCmd::from_click("nope"), None);
        assert_eq!(UiCmd::from_change("volume", "50", false), Some(UiCmd::Volume(0.5)));
        assert_eq!(UiCmd::from_change("fx:juice", "", true), Some(UiCmd::Effect("juice".into(), true)));
    }

    #[test]
    fn title_to_play_to_pause_and_back() {
        let (mut app, mut ui) = started();
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Title);
        ui.push(UiCmd::OpenLevels);
        tick(&mut app, &mut ui, 1);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Levels);
        ui.push(UiCmd::Start(1));
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Intro);
        tick(&mut app, &mut ui, 60 * 3);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Play);
        ui.push(UiCmd::Pause);
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Paused);
        ui.push(UiCmd::OpenSettings);
        tick(&mut app, &mut ui, 1);
        ui.push(UiCmd::CloseSettings);
        ui.push(UiCmd::Resume);
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Play);
        ui.push(UiCmd::Title);
        tick(&mut app, &mut ui, 2);
        assert_eq!(screen(&app.world.res::<Match3Hud>(), ui.levels_open), Screen::Title);
    }

    #[test]
    fn a_player_plays_level_one_to_a_win_and_stars_are_saved() {
        let (mut app, mut ui) = started();
        ui.push(UiCmd::Play);
        let mut won = false;
        for _ in 0..60 * 120 {
            tick(&mut app, &mut ui, 1);
            let hud = app.world.res::<Match3Hud>();
            if screen(&hud, false) == Screen::Won {
                won = true;
                break;
            }
            assert_ne!(screen(&hud, false), Screen::Lost);
            let idle = hud.idle;
            drop(hud);
            if idle {
                let m = app.world.res::<evani_kit_match_3::ecs::Grid>().board.best_move();
                if let Some(m) = m {
                    app.world.res_mut::<Game>().request(Request::Swap(m.a, m.b));
                }
            }
        }
        assert!(won);
        tick(&mut app, &mut ui, 2);
        assert!(ui.best.first().copied().unwrap_or(0) >= 1);
        let saved = app.world.res::<SaveData>().load_raw(KEY_STARS).unwrap().unwrap();
        assert!(saved.starts_with(|c: char| c.is_ascii_digit()) && saved != "0", "{saved}");
        // Play now goes to the first level without stars: level 2.
        ui.push(UiCmd::Title);
        tick(&mut app, &mut ui, 2);
        ui.push(UiCmd::Play);
        tick(&mut app, &mut ui, 2);
        assert_eq!(app.world.res::<Match3Hud>().level, 1);
    }

    #[test]
    fn tier_volume_effects_and_bindings_apply_and_save() {
        let (mut app, mut ui) = started();
        ui.push(UiCmd::Tier("low".into()));
        ui.push(UiCmd::Volume(0.25));
        tick(&mut app, &mut ui, 1);
        assert_eq!(app.world.res::<Tiers>().active().id, "low");
        assert!((app.world.res::<AudioMixer>().volume(None) - 0.25).abs() < 1e-6);
        #[cfg(feature = "juice")]
        {
            ui.push(UiCmd::Effect("juice".into(), false));
            tick(&mut app, &mut ui, 2);
            assert!(!app.world.res::<EffectsStatus>().is_on("juice"));
        }
        ui.push(UiCmd::Bind("pick".into()));
        tick(&mut app, &mut ui, 1);
        ui.push(UiCmd::Key("KeyJ".into()));
        tick(&mut app, &mut ui, 1);
        let input = app.world.res::<Input>();
        let id = input.action_id("pick").unwrap();
        assert!(binding_label(&input.actions()[id.0 as usize].bindings).starts_with('J'));
        drop(input);
        assert!(app.world.res::<SaveData>().load_raw(KEY_BINDINGS).unwrap().is_some());
        ui.push(UiCmd::ResetBindings);
        tick(&mut app, &mut ui, 1);
        let input = app.world.res::<Input>();
        let id = input.action_id("pick").unwrap();
        assert!(binding_label(&input.actions()[id.0 as usize].bindings).starts_with("Space"));
    }

    #[test]
    fn title_and_result_keep_clear_of_the_board_and_the_panel() {
        use evani_kit_match_3::layout::{CellLimits, layout};
        for (w, h) in [(1920.0, 1080.0), (1366.0, 768.0), (1280.0, 720.0), (844.0, 390.0), (667.0, 375.0), (390.0, 844.0), (768.0, 1024.0)] {
            let l = layout(w, h, 8, 8, CellLimits::default());
            let b = l.board;
            // The title box is beside (landscape) or above (portrait) the
            // board's cells, never over them.
            let (r, t, hint) = title_box(&l);
            if l.portrait {
                assert!(r[1] + r[3] <= b[1], "{w}x{h}: the title strip reaches the board");
                assert!(!hint, "{w}x{h}: no keys hint on a portrait screen");
            } else {
                assert!(r[0] + r[2] <= b[0], "{w}x{h}: the title column reaches the board");
            }
            assert!((12.0..=34.0).contains(&t), "{w}x{h}: title unit {t}");
            // The result card sits inside the board's columns, so it clears
            // the panel beside it (landscape) or above it (portrait).
            let (c, cw, cr) = result_card(&l);
            assert!(c[0] - cw * 0.5 >= b[0] && c[0] + cw * 0.5 <= b[0] + b[2], "{w}x{h}: the card is wider than the board");
            if l.portrait {
                assert!(c[1] > l.hud[1] + l.hud[3], "{w}x{h}");
            } else {
                assert!(c[0] - cw * 0.5 >= l.hud[0] + l.hud[2], "{w}x{h}: the card reaches the panel");
            }
            assert!((10.0..=40.0).contains(&cr), "{w}x{h}: card unit {cr}");
        }
    }

    #[test]
    fn texts_read_well() {
        assert_eq!(stars_text(2), "★★☆");
        let goals = [
            GoalView {
                kind: "collect",
                color: 0,
                have: 3,
                need: 16,
                done: false,
            },
            GoalView {
                kind: "frosting",
                color: 0,
                have: 0,
                need: 20,
                done: false,
            },
        ];
        assert_eq!(goal_sentence(&goals, 18), "Collect 16 Cherry and clear all the frosting in 18 moves");
        assert_eq!(goal_text(&goals[0]), "3/16");
        assert_eq!(css_hex([1.0, 0.0, 0.0, 1.0]), "#ff0000");
        assert_eq!(css_hex([0.5, 0.5, 0.5, 1.0]), "#bcbcbc");
        assert!(icon_svg("collect", 2, &DEFAULT_PIECES).contains("<path"));
        assert_eq!(key_label("ArrowUp"), "Up");
        assert_eq!(effect_label("particles"), "Sparkles");
    }
}
