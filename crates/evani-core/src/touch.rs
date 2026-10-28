//! On-screen touch controls (feature `touch`; a kit opts in).
//!
//! A kit declares a [`TouchLayout`] of buttons and sticks, each naming an
//! input action the kit already binds to keys and gamepads. A touch only
//! ever sets that action's state (as a key would), so the deterministic
//! simulation can't tell touch from keys. Analog values are quantised to
//! 1/127 steps ([`quantise`]) before [`Input`] sees them.
//!
//! This module is the logic (no browser): layouts, which finger holds which
//! control, stick maths (dead zone, axis lock), the "shown only on touch"
//! rule and the hand-off to [`Input`]. The page side (DOM elements, Pointer
//! Events, CSS) is `web/touch.rs`; it feeds the calls below and draws the
//! state back. See README "Touch controls".

use crate::ecs::World;
use crate::input::{ActionId, Input};

/// Which screen corner a control is placed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Where a control sits: offsets in `vmin` from its anchor corner, inside
/// the safe area (notches and rounded corners are added on top).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub anchor: Anchor,
    pub x: f32,
    pub y: f32,
}

impl Place {
    pub const fn new(anchor: Anchor, x: f32, y: f32) -> Self {
        Self { anchor, x, y }
    }
}

/// Button size class. All sizes are in `vmin` with CSS-pixel floors: never
/// under 48 px; primary actions 64 to 88 px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlSize {
    /// 48 to 56 px (pause, rarely used actions).
    Small,
    /// 48 to 72 px.
    Normal,
    /// 64 to 88 px (jump, drift: the thumb's main button).
    Primary,
}

impl ControlSize {
    /// The CSS width/height expression.
    pub fn css(self) -> &'static str {
        match self {
            ControlSize::Small => "clamp(48px,9vmin,56px)",
            ControlSize::Normal => "clamp(48px,13vmin,72px)",
            ControlSize::Primary => "clamp(64px,18vmin,88px)",
        }
    }
}

/// How a button drives its action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonMode {
    /// Down while the finger is on it (like holding a key).
    Hold,
    /// One press edge per touch (`just_pressed` for one tick), like a key
    /// tapped within a frame. For pause, menus, one-shot items.
    Tap,
}

/// Where a stick's base is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickMode {
    /// The base appears under the thumb where it lands inside the stick's
    /// [`Zone`]; it rests at its [`Place`] when untouched.
    Dynamic,
    /// The base stays at its [`Place`].
    Fixed,
}

/// A screen region in fractions of the viewport (0..1, top-left origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Zone {
    /// The left half below the top quarter (the top is for HUD and pause).
    pub const LEFT_HALF: Zone = Zone { x0: 0.0, y0: 0.25, x1: 0.5, y1: 1.0 };
    /// The right half below the top quarter.
    pub const RIGHT_HALF: Zone = Zone { x0: 0.5, y0: 0.25, x1: 1.0, y1: 1.0 };
}

/// A button bound to one action.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    /// Short id, unique in the layout; becomes `data-id` on the page.
    pub id: String,
    pub action: String,
    /// Text on the button (a word or a glyph).
    pub label: String,
    /// The action value while held: 1.0 for buttons, -1.0 or 1.0 for one
    /// side of an axis action (a left/right pair on `move`).
    pub value: f32,
    pub mode: ButtonMode,
    pub size: ControlSize,
    pub place: Place,
}

impl Button {
    fn make(id: &str, action: &str, label: &str, mode: ButtonMode) -> Self {
        Self {
            id: id.to_string(),
            action: action.to_string(),
            label: label.to_string(),
            value: 1.0,
            mode,
            size: ControlSize::Normal,
            place: Place::new(Anchor::BottomRight, 4.0, 4.0),
        }
    }

    /// Held while the finger is down.
    pub fn hold(id: &str, action: &str, label: &str) -> Self {
        Self::make(id, action, label, ButtonMode::Hold)
    }

    /// One press per touch.
    pub fn tap(id: &str, action: &str, label: &str) -> Self {
        Self::make(id, action, label, ButtonMode::Tap)
    }

    pub fn value(mut self, v: f32) -> Self {
        self.value = v;
        self
    }

    pub fn size(mut self, s: ControlSize) -> Self {
        self.size = s;
        self
    }

    pub fn primary(self) -> Self {
        self.size(ControlSize::Primary)
    }

    pub fn at(mut self, anchor: Anchor, x: f32, y: f32) -> Self {
        self.place = Place::new(anchor, x, y);
        self
    }
}

/// A virtual stick writing one or two axis actions. Values follow the
/// gamepad convention: x right positive, y down positive.
#[derive(Debug, Clone, PartialEq)]
pub struct Stick {
    pub id: String,
    pub x_action: Option<String>,
    pub y_action: Option<String>,
    pub mode: StickMode,
    /// Radial dead zone as a fraction of the stick radius (0.10 to 0.15
    /// suits steering); the rest is rescaled to 0..1 so it isn't jumpy.
    pub dead_zone: f32,
    /// Where a dynamic stick can start.
    pub zone: Zone,
    /// The base's rest (dynamic) or fixed position.
    pub place: Place,
    /// Label under the base (optional, e.g. "Steer").
    pub label: String,
}

impl Stick {
    fn make(id: &str, x: Option<&str>, y: Option<&str>) -> Self {
        Self {
            id: id.to_string(),
            x_action: x.map(str::to_string),
            y_action: y.map(str::to_string),
            mode: StickMode::Dynamic,
            dead_zone: 0.12,
            zone: Zone::LEFT_HALF,
            place: Place::new(Anchor::BottomLeft, 6.0, 6.0),
            label: String::new(),
        }
    }

    /// Horizontal only (axis lock): steering, walking.
    pub fn horizontal(id: &str, x_action: &str) -> Self {
        Self::make(id, Some(x_action), None)
    }

    /// Vertical only (axis lock).
    pub fn vertical(id: &str, y_action: &str) -> Self {
        Self::make(id, None, Some(y_action))
    }

    /// Both axes.
    pub fn two_axis(id: &str, x_action: &str, y_action: &str) -> Self {
        Self::make(id, Some(x_action), Some(y_action))
    }

    pub fn fixed(mut self) -> Self {
        self.mode = StickMode::Fixed;
        self
    }

    pub fn dynamic(mut self, zone: Zone) -> Self {
        self.mode = StickMode::Dynamic;
        self.zone = zone;
        self
    }

    pub fn dead_zone(mut self, dz: f32) -> Self {
        self.dead_zone = dz;
        self
    }

    pub fn at(mut self, anchor: Anchor, x: f32, y: f32) -> Self {
        self.place = Place::new(anchor, x, y);
        self
    }

    pub fn label(mut self, text: &str) -> Self {
        self.label = text.to_string();
        self
    }
} #[cfg(feature = "touch-look")] mod look; #[cfg(feature = "touch-look")] pub use look::{LookPad, quantise_delta};

/// One declared control.
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Button(Button),
    Stick(Stick), #[cfg(feature = "touch-look")] LookPad(LookPad),
}

impl Control {
    pub fn id(&self) -> &str {
        match self {
            Control::Button(b) => &b.id,
            Control::Stick(s) => &s.id, #[cfg(feature = "touch-look")] Control::LookPad(l) => &l.id,
        }
    }
}

/// What a kit declares: its controls, plus actions held for as long as
/// the touch layer is in use (a kart's auto-accelerate).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TouchLayout {
    pub controls: Vec<Control>,
    pub hold_while_shown: Vec<String>,
}

impl TouchLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn button(mut self, b: Button) -> Self {
        self.controls.push(Control::Button(b));
        self
    }

    pub fn stick(mut self, s: Stick) -> Self {
        self.controls.push(Control::Stick(s));
        self
    }

    /// The visible pause button touch players need (there is no Esc key on
    /// a phone): small, top-right, one press per tap. Use
    /// [`TouchLayout::pause_at`] when the game's HUD is in that corner.
    pub fn pause(self, action: &str) -> Self {
        self.pause_at(action, Anchor::TopRight, 3.0, 3.0)
    }

    /// The pause button at a chosen spot, clear of the game's own HUD.
    pub fn pause_at(self, action: &str, anchor: Anchor, x: f32, y: f32) -> Self {
        self.button(Button::tap("pause", action, "II").size(ControlSize::Small).at(anchor, x, y))
    }

    /// Hold `action` down (value 1) while touch controls are shown and
    /// enabled, as if its key were held: auto-accelerate under touch.
    pub fn hold_while_shown(mut self, action: &str) -> Self {
        self.hold_while_shown.push(action.to_string());
        self
    }
}

/// The kit's switch from a system or scene: touch controls on or off (for
/// menus played by tapping the page's buttons). Does nothing without a
/// layout.
pub fn set_enabled(world: &World, on: bool) {
    if let Some(mut t) = world.try_res_mut::<TouchControls>() {
        let mut input = world.res_mut::<Input>();
        t.set_enabled(&mut input, on);
    }
}

/// Whether touch controls are on screen and in use (shown and enabled).
/// For the page and HUD (swap key hints for touch hints); gameplay reads
/// actions only, so the sim never depends on this.
pub fn active(world: &World) -> bool {
    world.try_res::<TouchControls>().is_some_and(|t| t.active())
}

/// Analog value -> the step the sim sees: -127..127 for -1..1.
pub fn quantise(v: f32) -> i8 {
    if v.is_nan() {
        return 0;
    }
    (v.clamp(-1.0, 1.0) * 127.0).round() as i8
}

/// The exact `f32` the sim reads for a quantised step (127 -> 1.0, as a key).
pub fn dequantise(q: i8) -> f32 {
    f32::from(q.max(-127)) / 127.0
}

/// Stick maths: `raw` is the finger offset from the base in radii.
/// Locked axes are dropped first (a diagonal drag still steers fully),
/// the length is clamped to 1, then a radial dead zone with rescale.
/// Returns (knob position for drawing, quantised output).
pub fn stick_output(raw: [f32; 2], x_on: bool, y_on: bool, dead_zone: f32) -> ([f32; 2], [i8; 2]) {
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    let mut v = [
        if x_on { finite(raw[0]) } else { 0.0 },
        if y_on { finite(raw[1]) } else { 0.0 },
    ];
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len > 1.0 {
        v = [v[0] / len, v[1] / len];
    }
    let knob = v;
    let len = len.min(1.0);
    let dz = dead_zone.clamp(0.0, 0.9);
    if len <= dz || len == 0.0 {
        return (knob, [0, 0]);
    }
    let scale = (len - dz) / (1.0 - dz) / len;
    (knob, [quantise(v[0] * scale), quantise(v[1] * scale)])
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct ControlState {
    pointer: Option<i32>,
    /// Sticks: base centre (CSS px), radius (px), knob (radii), output.
    base: [f32; 2],
    radius: f32,
    knob: [f32; 2],
    out: [i8; 2],
}

#[derive(Debug, Clone)]
enum Resolved {
    Button { action: ActionId, q: i8, tap: bool },
    Stick { x: Option<ActionId>, y: Option<ActionId>, dead_zone: f32 }, #[doc = " A look pad: the finger's last position is kept in `ControlState::base`."] #[cfg(feature = "touch-look")] Look { x: Option<ActionId>, y: Option<ActionId>, scale: [f32; 2] },
}

/// The live touch layer: a world resource, inserted by
/// [`crate::plugin::PluginContext::touch_controls`].
#[derive(Debug, Clone)]
pub struct TouchControls {
    layout: TouchLayout,
    resolved: Vec<Resolved>,
    state: Vec<ControlState>,
    hold: Vec<ActionId>,
    /// Every action any control or hold writes (deduplicated).
    actions: Vec<ActionId>,
    shown: bool,
    enabled: bool,
    revision: u64,
}

impl TouchControls {
    /// Resolve the layout's action names against `input`. Unknown actions
    /// and duplicate control ids are errors.
    pub fn new(layout: TouchLayout, input: &Input) -> Result<Self, String> {
        let find = |name: &str| {
            input
                .action_id(name)
                .ok_or_else(|| format!("touch control names unknown action `{name}` (declare it with kit.action first)"))
        };
        let mut resolved = Vec::with_capacity(layout.controls.len());
        let mut actions: Vec<ActionId> = Vec::new();
        let add = |a: ActionId, list: &mut Vec<ActionId>| {
            if !list.contains(&a) {
                list.push(a);
            }
        };
        for (i, c) in layout.controls.iter().enumerate() {
            if c.id().is_empty() || layout.controls[..i].iter().any(|o| o.id() == c.id()) {
                return Err(format!("touch control id `{}` is empty or used twice", c.id()));
            }
            match c {
                Control::Button(b) => {
                    let a = find(&b.action)?;
                    add(a, &mut actions);
                    resolved.push(Resolved::Button {
                        action: a,
                        q: quantise(b.value),
                        tap: b.mode == ButtonMode::Tap,
                    });
                }
                Control::Stick(s) => {
                    let x = s.x_action.as_deref().map(find).transpose()?;
                    let y = s.y_action.as_deref().map(find).transpose()?;
                    if x.is_none() && y.is_none() {
                        return Err(format!("touch stick `{}` drives no action", s.id));
                    }
                    for a in [x, y].into_iter().flatten() {
                        add(a, &mut actions);
                    }
                    resolved.push(Resolved::Stick { x, y, dead_zone: s.dead_zone });
                } #[cfg(feature = "touch-look")] Control::LookPad(l) => resolved.push(look::resolve(l, find)?),
            }
        }
        let mut hold = Vec::new();
        for name in &layout.hold_while_shown {
            let a = find(name)?;
            add(a, &mut actions);
            hold.push(a);
        }
        let n = resolved.len();
        Ok(Self {
            layout,
            resolved,
            state: vec![ControlState::default(); n],
            hold,
            actions,
            shown: false,
            enabled: true,
            revision: 1,
        })
    }

    pub fn layout(&self) -> &TouchLayout {
        &self.layout
    }

    /// Bumped on every change the page has to draw.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Shown by the touch-only rule (touch device, no key/pad since).
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The kit's switch: off on title screens and menus that are played
    /// with taps on the page's own buttons.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Shown and enabled: the controls are on screen and drive actions.
    pub fn active(&self) -> bool {
        self.shown && self.enabled
    }

    /// Whether control `index` is held (buttons) or dragged (sticks).
    pub fn is_held(&self, index: usize) -> bool {
        self.state.get(index).is_some_and(|s| s.pointer.is_some())
    }

    /// A stick's base (CSS px) and knob offset (radii) while dragged.
    pub fn stick_draw(&self, index: usize) -> Option<([f32; 2], [f32; 2])> {
        let s = self.state.get(index)?;
        s.pointer.map(|_| (s.base, s.knob))
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    // ---- the touch-only rule ---------------------------------------------

    /// Start state from the page: `(pointer: coarse)` matched.
    pub fn set_coarse(&mut self, input: &mut Input, coarse: bool) {
        self.set_shown(input, coarse);
    }

    /// A touch went down anywhere: show the controls.
    pub fn on_touch(&mut self, input: &mut Input) {
        self.set_shown(input, true);
    }

    /// A key or gamepad was used: hide the controls and let go of them.
    pub fn on_key_or_pad(&mut self, input: &mut Input) {
        self.set_shown(input, false);
    }

    fn set_shown(&mut self, input: &mut Input, shown: bool) {
        if shown != self.shown {
            self.shown = shown;
            self.release_all(input);
        }
    }

    /// The kit's switch (see [`TouchControls::enabled`]). Turning it off
    /// lets go of every control.
    pub fn set_enabled(&mut self, input: &mut Input, enabled: bool) {
        if enabled != self.enabled {
            self.enabled = enabled;
            self.release_all(input);
        }
    }

    /// Any connected gamepad pressed or tilted: counts as pad input.
    pub fn pad_in_use(input: &Input) -> bool {
        (0..crate::input::MAX_PADS).filter_map(|i| input.pad(i)).any(|p| {
            p.connected && (p.buttons.iter().any(|b| *b >= 0.5) || p.axes.iter().any(|a| a.abs() >= 0.5))
        })
    }

    // ---- fingers -----------------------------------------------------------

    /// Finger `pointer` went down on control `index`. Sticks give their base
    /// centre and radius in CSS px (a dynamic stick's base is the finger).
    /// A control already held by another finger ignores the new one.
    pub fn press(&mut self, input: &mut Input, index: usize, pointer: i32, at: [f32; 2], base: [f32; 2], radius: f32) {
        if !self.active() || self.state.iter().any(|s| s.pointer == Some(pointer)) {
            return;
        }
        let Some(st) = self.state.get_mut(index) else { return };
        if st.pointer.is_some() {
            return;
        }
        st.pointer = Some(pointer);
        if let Some(Resolved::Button { action, tap: true, .. }) = self.resolved.get(index) {
            input.tap_touch(*action);
        }
        if let Some(Resolved::Stick { .. }) = self.resolved.get(index) {
            st.base = base;
            st.radius = radius.max(1.0);
        } #[cfg(feature = "touch-look")] if let Some(Resolved::Look { .. }) = self.resolved.get(index) { st.base = at; }
        self.drag_index(index, at);
        self.apply(input);
    }

    /// Finger `pointer` moved to `at` (CSS px).
    pub fn drag(&mut self, input: &mut Input, pointer: i32, at: [f32; 2]) {
        let Some(i) = self.state.iter().position(|s| s.pointer == Some(pointer)) else {
            return;
        }; #[cfg(feature = "touch-look")] if self.look_drag(input, i, at) { return; }
        let before = self.state[i].out;
        self.drag_index(i, at);
        if self.state[i].out != before {
            self.apply(input);
        }
    }

    fn drag_index(&mut self, i: usize, at: [f32; 2]) {
        let Some(Resolved::Stick { x, y, dead_zone }) = self.resolved.get(i) else {
            return;
        };
        let st = &mut self.state[i];
        let raw = [(at[0] - st.base[0]) / st.radius, (at[1] - st.base[1]) / st.radius];
        let (knob, out) = stick_output(raw, x.is_some(), y.is_some(), *dead_zone);
        if knob != st.knob {
            st.knob = knob;
            self.revision = self.revision.wrapping_add(1);
        }
        st.out = out;
    }

    /// Finger `pointer` lifted, was cancelled or lost capture: let go.
    pub fn release(&mut self, input: &mut Input, pointer: i32) {
        let mut any = false;
        for st in &mut self.state {
            if st.pointer == Some(pointer) {
                *st = ControlState::default();
                any = true;
            }
        }
        if any {
            self.apply(input);
        }
    }

    /// Let go of everything (blur, hidden tab, Esc, layer hidden).
    pub fn release_all(&mut self, input: &mut Input) {
        for st in &mut self.state {
            *st = ControlState::default();
        }
        self.apply(input);
    }

    /// Write the per-action sums into [`Input`] (clamped to -127..127).
    fn apply(&mut self, input: &mut Input) {
        self.bump();
        let active = self.active();
        for &a in &self.actions {
            let mut sum: i32 = 0;
            if active {
                for (r, st) in self.resolved.iter().zip(&self.state) {
                    if st.pointer.is_none() {
                        continue;
                    }
                    match *r {
                        Resolved::Button { action, q, tap: false } if action == a => sum += i32::from(q),
                        Resolved::Stick { x, y, .. } => {
                            if x == Some(a) {
                                sum += i32::from(st.out[0]);
                            }
                            if y == Some(a) {
                                sum += i32::from(st.out[1]);
                            }
                        }
                        _ => {}
                    }
                }
                if self.hold.contains(&a) {
                    sum += 127;
                }
            }
            input.set_touch(a, sum.clamp(-127, 127) as i8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{ActionDef, KeyCode, RawEvent};

    fn input() -> Input {
        let mut i = Input::default();
        i.add_action(ActionDef::button("jump", &["key:Space"]).unwrap());
        i.add_action(ActionDef::axis("move", &["keys:KeyA/KeyD", "pad:axis:0"]).unwrap());
        i.add_action(ActionDef::button("pause", &["key:Escape"]).unwrap());
        i.add_action(ActionDef::button("throttle", &["key:KeyW"]).unwrap());
        i
    }

    fn layout() -> TouchLayout {
        TouchLayout::new()
            .button(Button::hold("left", "move", "<").value(-1.0).at(Anchor::BottomLeft, 4.0, 4.0))
            .button(Button::hold("right", "move", ">").at(Anchor::BottomLeft, 22.0, 4.0))
            .button(Button::hold("jump", "jump", "Jump").primary())
            .stick(Stick::horizontal("steer", "move").dead_zone(0.1))
            .pause("pause")
    }

    fn key(code: &str, down: bool) -> RawEvent {
        RawEvent::Key { code: KeyCode::new(code).unwrap(), down }
    }

    fn shown(i: &mut Input) -> TouchControls {
        let mut t = TouchControls::new(layout(), i).unwrap();
        t.on_touch(i);
        t
    }

    /// Frame by frame, a touch on "jump" gives the same action states as Space.
    #[test]
    fn touch_press_and_release_match_the_key() {
        let mut by_key = input();
        let mut by_touch = input();
        let mut t = shown(&mut by_touch);
        let jump = by_key.action_id("jump").unwrap();
        let mut frames = Vec::new();
        for f in 0..6 {
            match f {
                1 => {
                    by_key.handle(&key("Space", true));
                    t.press(&mut by_touch, 2, 7, [0.0; 2], [0.0; 2], 0.0);
                }
                4 => {
                    by_key.handle(&key("Space", false));
                    t.release(&mut by_touch, 7);
                }
                _ => {}
            }
            by_key.update();
            by_touch.update();
            assert_eq!(by_key.state(jump), by_touch.state(jump), "frame {f}");
            frames.push(by_touch.state(jump));
            by_key.end_tick();
            by_touch.end_tick();
        }
        assert!(frames[1].just_pressed && frames[1].down);
        assert!(frames[4].just_released && !frames[4].down);
        // Left/right buttons on an axis action behave like keys:KeyA/KeyD.
        let mv = by_key.action_id("move").unwrap();
        by_key.handle(&key("KeyA", true));
        t.press(&mut by_touch, 0, 3, [0.0; 2], [0.0; 2], 0.0);
        by_key.update();
        by_touch.update();
        assert_eq!(by_key.state(mv), by_touch.state(mv));
        assert_eq!(by_touch.value(mv), -1.0);
        // Both sides held cancel out, as both keys do.
        by_key.handle(&key("KeyD", true));
        t.press(&mut by_touch, 1, 4, [0.0; 2], [0.0; 2], 0.0);
        by_key.update();
        by_touch.update();
        assert_eq!(by_key.state(mv).value, by_touch.state(mv).value);
    }

    /// A tap shorter than a frame is still seen, like a quick key tap.
    #[test]
    fn sub_frame_tap_and_tap_mode_edges() {
        let mut i = input();
        let mut t = shown(&mut i);
        let jump = i.action_id("jump").unwrap();
        t.press(&mut i, 2, 1, [0.0; 2], [0.0; 2], 0.0);
        t.release(&mut i, 1);
        i.update();
        let s = i.state(jump);
        assert!(s.just_pressed && s.just_released && !s.down);
        i.end_tick();
        // Tap-mode pause: one press edge, never held.
        let pause = i.action_id("pause").unwrap();
        t.press(&mut i, 4, 2, [0.0; 2], [0.0; 2], 0.0);
        i.update();
        assert!(i.just_pressed(pause) && !i.down(pause));
        i.end_tick();
        i.update();
        assert!(!i.just_pressed(pause));
    }

    /// pointercancel / lostpointercapture / blur / Esc all let go.
    #[test]
    fn cancel_blur_and_escape_release() {
        let mut i = input();
        let mut t = shown(&mut i);
        let jump = i.action_id("jump").unwrap();
        t.press(&mut i, 2, 9, [0.0; 2], [0.0; 2], 0.0);
        i.update();
        assert!(i.down(jump));
        t.release(&mut i, 9); // pointercancel
        i.update();
        assert!(!i.down(jump));
        t.press(&mut i, 2, 9, [0.0; 2], [0.0; 2], 0.0);
        i.update();
        assert!(i.down(jump));
        i.handle(&RawEvent::Blur);
        t.release_all(&mut i);
        i.update();
        assert!(!i.down(jump) && !t.is_held(2));
        // Esc is a key: the layer hides and lets go.
        t.on_touch(&mut i);
        t.press(&mut i, 2, 9, [0.0; 2], [0.0; 2], 0.0);
        t.on_key_or_pad(&mut i);
        i.update();
        assert!(!i.down(jump) && !t.shown());
        // Hidden: presses do nothing.
        t.press(&mut i, 2, 9, [0.0; 2], [0.0; 2], 0.0);
        i.update();
        assert!(!i.down(jump));
    }

    /// Multi-touch: one finger per control; a second finger on a held
    /// control is ignored, and releasing one finger keeps the other.
    #[test]
    fn multi_touch_one_finger_per_control() {
        let mut i = input();
        let mut t = shown(&mut i);
        let jump = i.action_id("jump").unwrap();
        let mv = i.action_id("move").unwrap();
        t.press(&mut i, 2, 1, [0.0; 2], [0.0; 2], 0.0);
        t.press(&mut i, 1, 2, [0.0; 2], [0.0; 2], 0.0);
        t.press(&mut i, 2, 3, [0.0; 2], [0.0; 2], 0.0);
        i.update();
        assert!(i.down(jump) && i.value(mv) == 1.0);
        t.release(&mut i, 3);
        i.update();
        assert!(i.down(jump));
        t.release(&mut i, 1);
        i.update();
        assert!(!i.down(jump) && i.value(mv) == 1.0);
    }

    #[test]
    fn quantisation_is_deterministic_and_exact_for_keys() {
        for q in -127..=127i8 {
            assert_eq!(quantise(dequantise(q)), q);
        }
        assert_eq!(dequantise(127), 1.0);
        assert_eq!(dequantise(-127), -1.0);
        assert_eq!(quantise(f32::NAN), 0);
        assert_eq!(quantise(3.0), 127);
        // The same drag always gives the same bits.
        let runs: Vec<[i8; 2]> = (0..3).map(|_| stick_output([0.437, 0.2], true, false, 0.12).1).collect();
        assert!(runs.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(runs[0][1], 0, "axis lock drops y");
    }

    #[test]
    fn stick_dead_zone_rescale_and_clamp() {
        assert_eq!(stick_output([0.1, 0.0], true, false, 0.12).1, [0, 0]);
        assert_eq!(stick_output([2.0, 0.0], true, false, 0.12).1, [127, 0]);
        assert_eq!(stick_output([-5.0, 0.0], true, false, 0.12).1, [-127, 0]);
        // Just past the dead zone starts near zero, not at 0.12.
        let (_, o) = stick_output([0.13, 0.0], true, false, 0.12);
        assert!(o[0] > 0 && o[0] < 4, "{o:?}");
        // Diagonal on a horizontal stick still steers fully.
        assert_eq!(stick_output([1.0, 1.0], true, false, 0.12).1, [127, 0]);
        // Two-axis stick clamps the length to 1.
        let (k, _) = stick_output([3.0, 4.0], true, true, 0.1);
        assert!((k[0] - 0.6).abs() < 1e-6 && (k[1] - 0.8).abs() < 1e-6);
    }

    /// The steer stick drives the same axis the keys do, quantised.
    #[test]
    fn stick_drives_the_axis_quantised() {
        let mut i = input();
        let mut t = shown(&mut i);
        let mv = i.action_id("move").unwrap();
        t.press(&mut i, 3, 5, [100.0, 300.0], [100.0, 300.0], 50.0);
        i.update();
        assert_eq!(i.value(mv), 0.0);
        t.drag(&mut i, 5, [150.0, 280.0]);
        i.update();
        assert_eq!(i.value(mv), 1.0);
        t.drag(&mut i, 5, [75.0, 300.0]);
        i.update();
        let v = i.value(mv);
        assert_eq!(v, dequantise(quantise(v)));
        assert!(v < -0.3 && v > -0.5, "{v}");
        let (base, knob) = t.stick_draw(3).unwrap();
        assert_eq!(base, [100.0, 300.0]);
        assert_eq!(knob, [-0.5, 0.0]);
        t.release(&mut i, 5);
        i.update();
        assert_eq!(i.value(mv), 0.0);
    }

    /// Shown only on touch: starts from `(pointer: coarse)`, a touch shows
    /// it, a key or a gamepad hides it; the kit's switch gates it too.
    #[test]
    fn touch_only_visibility_rule() {
        let mut i = input();
        let mut t = TouchControls::new(layout(), &i).unwrap();
        assert!(!t.shown(), "hidden until the page says coarse or a touch lands");
        t.set_coarse(&mut i, true);
        assert!(t.shown() && t.active());
        t.on_key_or_pad(&mut i);
        assert!(!t.shown());
        t.on_touch(&mut i);
        assert!(t.shown());
        t.set_enabled(&mut i, false);
        assert!(t.shown() && !t.active());
        t.set_enabled(&mut i, true);
        // Gamepad use counts.
        assert!(!TouchControls::pad_in_use(&i));
        i.set_pad(0, true, &[1.0], &[0.0; 4]);
        assert!(TouchControls::pad_in_use(&i));
        i.set_pad(0, true, &[0.0], &[0.0, 0.7, 0.0, 0.0]);
        assert!(TouchControls::pad_in_use(&i));
        i.set_pad(0, true, &[0.0], &[0.2, 0.0, 0.0, 0.0]);
        assert!(!TouchControls::pad_in_use(&i));
    }

    /// Auto-accelerate: held while shown and enabled, released otherwise.
    #[test]
    fn hold_while_shown() {
        let mut i = input();
        let l = layout().hold_while_shown("throttle");
        let mut t = TouchControls::new(l, &i).unwrap();
        let th = i.action_id("throttle").unwrap();
        t.on_touch(&mut i);
        i.update();
        assert!(i.down(th));
        t.set_enabled(&mut i, false);
        i.update();
        assert!(!i.down(th));
        t.set_enabled(&mut i, true);
        t.on_key_or_pad(&mut i);
        i.update();
        assert!(!i.down(th));
    }

    #[test]
    fn bad_layouts_are_errors() {
        let i = input();
        let e = TouchControls::new(TouchLayout::new().button(Button::hold("a", "nope", "A")), &i).unwrap_err();
        assert!(e.contains("nope"));
        let dup = TouchLayout::new().button(Button::hold("a", "jump", "A")).button(Button::hold("a", "jump", "B"));
        assert!(TouchControls::new(dup, &i).is_err());
    }
}
