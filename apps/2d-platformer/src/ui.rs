//! The game's light DOM menus: title screen, pause menu, settings (quality
//! tier, effect toggles, volume, stats overlay, key rebinding) and results.
//! The in-play HUD (lives, coins, score, timer) is drawn by the kit in pixel
//! glyphs, so there is no DOM HUD.
//!
//! The markup and styles live in `index.html`; this module fills it in and
//! shows one screen at a time. Buttons carry `data-cmd` and inputs
//! `data-set`; one delegated listener per event type turns them into
//! [`UiCmd`]s, applied at the next frame boundary (natively the tests apply
//! the same commands). The kit owns the game ([`Platformer::game`] takes
//! [`Request`]s) and fills [`PlatformerHud`]; the UI never touches the
//! simulation directly.
//!
//! Saved (SaveData): `render.tier` (the renderer reads it at start),
//! `platformer.volume`, `platformer.overlay` and `platformer.bindings`.

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
use evani_core::quality::SAVED_TIER_KEY;
use evani_core::save::SaveData;
use evani_core::tiers::Tiers;
use evani_kit_2d_platformer::Platformer;
use evani_kit_2d_platformer::game::{Phase, Request};
use evani_kit_2d_platformer::hud::PlatformerHud;

pub const KEY_VOLUME: &str = "platformer.volume";
pub const KEY_OVERLAY: &str = "platformer.overlay";
pub const KEY_BINDINGS: &str = "platformer.bindings";

/// Seconds after the goal (Fennel walks into the gate) or the last life
/// before the results screen comes up.
pub const RESULTS_DELAY_S: f32 = 1.5;

/// What a player can ask the UI for.
#[derive(Debug, Clone, PartialEq)]
pub enum UiCmd {
    Play,
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
    /// Wait for a key for this action (`move` waits for two).
    Bind(String),
    /// A key pressed while waiting (`KeyboardEvent.code`); Escape cancels.
    Key(String),
    ResetBindings,
}

impl UiCmd {
    /// A button's `data-cmd`.
    pub fn from_click(cmd: &str) -> Option<Self> {
        Some(match cmd {
            "play" => UiCmd::Play,
            "restart" => UiCmd::Restart,
            "title" => UiCmd::Title,
            "resume" => UiCmd::Resume,
            "settings" => UiCmd::OpenSettings,
            "close-settings" => UiCmd::CloseSettings,
            "reset-binds" => UiCmd::ResetBindings,
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
    Play,
    Paused,
    Results,
}

pub fn screen(hud: &PlatformerHud) -> Screen {
    match hud.phase {
        Phase::Title => Screen::Title,
        Phase::Paused => Screen::Paused,
        Phase::Won | Phase::Lost if hud.phase_time >= RESULTS_DELAY_S => Screen::Results,
        _ => Screen::Play,
    }
}

/// The results screen: heading, the line under it and the tally rows.
pub fn results_view(hud: &PlatformerHud) -> (String, String, Vec<(String, String)>) {
    let Some(r) = hud.results else {
        return ("Level over".to_string(), String::new(), Vec::new());
    };
    let n = |v: u32| v.to_string();
    if r.won {
        let rows = vec![
            ("Coins".to_string(), n(r.coins)),
            ("Beetles and hoppers stomped".to_string(), n(r.stomps)),
            ("Time left".to_string(), format!("{} s", r.time_left)),
            ("Time bonus".to_string(), format!("+{}", r.time_bonus)),
            ("Score".to_string(), n(r.score)),
        ];
        ("Level clear!".to_string(), format!("Fennel made it through {}.", hud.level_name), rows)
    } else {
        let rows = vec![("Coins".to_string(), n(r.coins)), ("Score".to_string(), n(r.score))];
        let sub = if hud.time_left == 0 {
            "The timer ran out. Run (hold Shift) to reach the castle sooner.".to_string()
        } else {
            "Out of lives. Flags you touch become checkpoints.".to_string()
        };
        ("Game over".to_string(), sub, rows)
    }
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

/// The keyboard binding of an action, as a label (`A / D`, `Space`).
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

/// Rebindable actions: name and label.
pub const BIND_ROWS: [(&str, &str); 5] = [
    ("move", "Walk (left / right)"),
    ("jump", "Jump"),
    ("run", "Run (hold)"),
    ("down", "Down (with jump: drop through planks)"),
    ("pause", "Pause"),
];

/// A player-facing effect name.
pub fn effect_label(name: &str) -> &str {
    match name {
        "parallax" => "Parallax backgrounds",
        "shake" => "Screen shake",
        "bloom" => "Glow",
        "vignette" => "Dark corners",
        other => other,
    }
}

/// Rebinding in progress: the action and, for walking, the left key.
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
        if let Some(mut p) = world.try_res_mut::<Platformer>() {
            p.game.request(r);
        }
    }

    /// Apply one command.
    pub fn apply(&mut self, world: &World, c: UiCmd) {
        match c {
            UiCmd::Play => {
                self.settings_open = false;
                Self::request(world, Request::Start);
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
        let text = if p.action == "move" {
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
            // Slot 0 is the first keyboard binding.
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

    /// Keep the game paused while settings are open over it.
    fn hold_pause(&self, world: &World) {
        if self.settings_open && world.res::<PlatformerHud>().phase == Phase::Playing {
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
}

impl EngineModule for GameUi {
    fn id(&self) -> &'static str {
        "evani.module.platformer_ui"
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
        self.sync_modal();
        #[cfg(target_arch = "wasm32")]
        if let Some(mut d) = self.dom.take() {
            d.sync(world, self);
            self.dom = Some(d);
        }
    }

    fn visibility_changed(&mut self, world: &mut World, hidden: bool) {
        // A hidden tab pauses the game (the loop stops anyway; this keeps the
        // pause menu up when the player comes back).
        if hidden {
            Self::request(world, Request::Pause(true));
        }
    }

    fn status(&self, world: &World, out: &mut String) {
        out.push_str(world.res::<PlatformerHud>().phase.as_str());
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
    use evani_core::overlay::Overlay;
    use evani_core::tiers::Tiers;
    use evani_kit_2d_platformer::hud::PlatformerHud;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

    use super::{BIND_ROWS, CmdQueue, GameUi, Modal, Screen, UiCmd, binding_label, effect_label, results_view, screen};

    pub struct Dom {
        doc: Document,
        title: Element,
        pause: Element,
        results: Element,
        settings: Element,
        settings_body: Element,
        res_head: Element,
        res_sub: Element,
        res_table: Element,
        shown: Option<(Screen, bool)>,
        settings_seen: Option<(u64, usize, evani_core::effects::BudgetStatus, u64)>,
        results_seen: Option<u64>,
    }

    fn by_id(doc: &Document, id: &str) -> Result<Element, String> {
        doc.get_element_by_id(id).ok_or_else(|| format!("index.html has no #{id}"))
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
                    if (e.type_() == "input") != (set == "volume") {
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
                shown: None,
                settings_seen: None,
                results_seen: None,
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

        /// Drop focus so Space (jump) can't press a hidden button.
        fn blur(&self) {
            if let Some(a) = self.doc.active_element()
                && let Ok(a) = a.dyn_into::<HtmlElement>()
            {
                let _ = a.blur();
            }
        }

        pub fn sync(&mut self, world: &World, ui: &GameUi) {
            let hud = world.res::<PlatformerHud>();
            let scr = screen(&hud);
            let now = (scr, ui.settings_open);
            if self.shown != Some(now) {
                self.shown = Some(now);
                self.blur();
                set_on(&self.title, "screen", scr == Screen::Title && !ui.settings_open);
                set_on(&self.pause, "screen dim", scr == Screen::Paused && !ui.settings_open);
                set_on(&self.results, "screen dim", scr == Screen::Results && !ui.settings_open);
                set_on(&self.settings, "screen dim", ui.settings_open);
                if ui.settings_open {
                    self.settings_seen = None;
                } else {
                    match scr {
                        Screen::Title => self.focus_first(&self.title),
                        Screen::Paused => self.focus_first(&self.pause),
                        Screen::Results => self.focus_first(&self.results),
                        Screen::Play => {}
                    }
                }
            }
            // Results, rebuilt once per finished run.
            if scr == Screen::Results && self.results_seen != Some(hud.run_no) {
                self.results_seen = Some(hud.run_no);
                let (head, sub, rows) = results_view(&hud);
                self.res_head.set_text_content(Some(&head));
                self.res_sub.set_text_content(Some(&sub));
                let mut html = String::with_capacity(rows.len() * 60);
                for (k, v) in &rows {
                    html.push_str("<tr><td>");
                    html.push_str(&escape(k));
                    html.push_str("</td><td>");
                    html.push_str(&escape(v));
                    html.push_str("</td></tr>");
                }
                self.res_table.set_inner_html(&html);
            } else if scr != Screen::Results {
                self.results_seen = None;
            }
            drop(hud);
            if ui.settings_open {
                let key = ui.settings_key(world);
                if self.settings_seen != Some(key) {
                    self.settings_seen = Some(key);
                    self.settings_body.set_inner_html(&settings_html(world, ui));
                }
            }
        }
    }

    fn settings_html(world: &World, ui: &GameUi) -> String {
        use core::fmt::Write;
        let mut s = String::with_capacity(3072);
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
        let vol = evani_core::num::fixed((world.res::<AudioMixer>().volume(None) * 100.0).round(), 0);
        let _ = write!(
            s,
            "<h3>Sound</h3><label>Volume <input type=\"range\" min=\"0\" max=\"100\" value=\"{vol}\" data-set=\"volume\"></label>"
        );
        let ov = if world.res::<Overlay>().enabled { " checked" } else { "" };
        let _ = write!(s, "<label><input type=\"checkbox\" data-set=\"overlay\"{ov}>Show speed stats</label>");
        s.push_str("<h3>Controls</h3><table class=\"binds\">");
        {
            let input = world.res::<Input>();
            for (action, label) in BIND_ROWS {
                let text = match &ui.pending {
                    Some(pd) if pd.action == action && pd.first.is_some() => "press RIGHT key".to_string(),
                    Some(pd) if pd.action == action && action == "move" => "press LEFT key".to_string(),
                    Some(pd) if pd.action == action => "press a key".to_string(),
                    _ => input
                        .action_id(action)
                        .map(|id| binding_label(&input.actions()[id.0 as usize].bindings))
                        .unwrap_or_default(),
                };
                let _ = write!(
                    s,
                    "<tr><td>{label}</td><td><button data-cmd=\"bind:{action}\">{}</button></td></tr>",
                    escape(&text)
                );
            }
        }
        s.push_str(
            "</table><p class=\"note\">Click a key, then press the new one (Esc cancels). Arrow keys and a gamepad work too.</p>\
             <p><button data-cmd=\"reset-binds\" class=\"alt\">Reset controls</button></p>",
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_app;
    use evani_core::App;
    use evani_kit_2d_platformer::game::Results;

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

    fn scr(app: &App) -> Screen {
        screen(&app.world.res::<PlatformerHud>())
    }

    #[test]
    fn commands_parse() {
        assert_eq!(UiCmd::from_click("play"), Some(UiCmd::Play));
        assert_eq!(UiCmd::from_click("bind:jump"), Some(UiCmd::Bind("jump".into())));
        assert_eq!(UiCmd::from_click("nope"), None);
        assert_eq!(UiCmd::from_change("volume", "40", false), Some(UiCmd::Volume(0.4)));
        assert_eq!(UiCmd::from_change("fx:parallax", "on", true), Some(UiCmd::Effect("parallax".into(), true)));
        assert_eq!(UiCmd::from_change("tier", "low", false), Some(UiCmd::Tier("low".into())));
        assert_eq!(UiCmd::from_change("what", "", false), None);
    }

    #[test]
    fn title_to_play_to_pause_and_back() {
        let (mut app, mut ui) = started();
        tick(&mut app, &mut ui, 2);
        assert_eq!(scr(&app), Screen::Title);
        ui.push(UiCmd::Play);
        tick(&mut app, &mut ui, 3);
        assert_eq!(scr(&app), Screen::Play);
        // Settings over play pauses the game; closing leaves the pause menu.
        ui.push(UiCmd::OpenSettings);
        tick(&mut app, &mut ui, 3);
        assert_eq!(scr(&app), Screen::Paused);
        ui.push(UiCmd::CloseSettings);
        ui.push(UiCmd::Resume);
        tick(&mut app, &mut ui, 3);
        assert_eq!(scr(&app), Screen::Play);
        ui.push(UiCmd::Title);
        tick(&mut app, &mut ui, 3);
        assert_eq!(scr(&app), Screen::Title);
    }

    #[test]
    fn rebinding_walk_and_jump_saves_and_resets() {
        let (mut app, mut ui) = started();
        ui.push(UiCmd::OpenSettings);
        ui.push(UiCmd::Bind("move".into()));
        ui.push(UiCmd::Key("KeyJ".into()));
        ui.push(UiCmd::Key("KeyL".into()));
        ui.push(UiCmd::Bind("jump".into()));
        ui.push(UiCmd::Key("KeyI".into()));
        tick(&mut app, &mut ui, 1);
        let label = |app: &App, a: &str| {
            let input = app.world.res::<Input>();
            let id = input.action_id(a).unwrap();
            binding_label(&input.actions()[id.0 as usize].bindings)
        };
        assert_eq!(label(&app, "move"), "J / L");
        assert_eq!(label(&app, "jump"), "I");
        assert!(GameUi::load(&app.world, KEY_BINDINGS).is_some());
        // Escape cancels a pending rebind.
        ui.push(UiCmd::Bind("run".into()));
        ui.push(UiCmd::Key("Escape".into()));
        tick(&mut app, &mut ui, 1);
        assert!(ui.pending.is_none());
        assert_eq!(label(&app, "run"), "Left Shift");
        ui.push(UiCmd::ResetBindings);
        tick(&mut app, &mut ui, 1);
        assert_eq!(label(&app, "move"), "A / D");
        assert_eq!(label(&app, "jump"), "Space");
        assert!(GameUi::load(&app.world, KEY_BINDINGS).is_none());
    }

    #[test]
    fn effects_volume_and_overlay_apply_and_save() {
        let (mut app, mut ui) = started();
        let names: Vec<String> = GameUi::effects(&app.world).into_iter().map(|e| e.0).collect();
        if cfg!(feature = "parallax") {
            assert!(names.iter().any(|n| n == "parallax"), "{names:?}");
        }
        ui.push(UiCmd::Effect("shake".into(), false));
        ui.push(UiCmd::Volume(0.3));
        ui.push(UiCmd::Overlay(true));
        tick(&mut app, &mut ui, 1);
        if cfg!(feature = "shake") {
            let fx = GameUi::effects(&app.world);
            assert!(fx.iter().any(|(n, on, _)| n == "shake" && !on));
        }
        assert_eq!(GameUi::load(&app.world, KEY_VOLUME).as_deref(), Some("0.30"));
        assert!(app.world.res::<Overlay>().enabled);
    }

    #[test]
    fn results_wait_for_the_gate_and_say_win_or_lose() {
        let mut hud = PlatformerHud {
            phase: Phase::Won,
            level_name: "Bramble Hills".into(),
            results: Some(Results {
                won: true,
                coins: 12,
                stomps: 2,
                time_left: 80,
                time_bonus: 400,
                score: 920,
            }),
            ..PlatformerHud::default()
        };
        assert_eq!(screen(&hud), Screen::Play);
        hud.phase_time = RESULTS_DELAY_S;
        assert_eq!(screen(&hud), Screen::Results);
        let (head, sub, rows) = results_view(&hud);
        assert_eq!(head, "Level clear!");
        assert!(sub.contains("Bramble Hills"));
        assert_eq!(rows.last().map(|r| r.1.as_str()), Some("920"));
        hud.phase = Phase::Lost;
        hud.time_left = 0;
        if let Some(r) = hud.results.as_mut() {
            r.won = false;
        }
        let (head, sub, _) = results_view(&hud);
        assert_eq!(head, "Game over");
        assert!(sub.contains("timer"));
    }

    #[test]
    fn labels_read_well() {
        assert_eq!(key_label("KeyA"), "A");
        assert_eq!(key_label("ShiftLeft"), "Left Shift");
        assert_eq!(key_label("ArrowUp"), "Up");
        assert_eq!(key_label("Space"), "Space");
        assert_eq!(effect_label("parallax"), "Parallax backgrounds");
    }
}
