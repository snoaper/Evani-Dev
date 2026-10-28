//! Input actions over keyboard, mouse, pointer/touch and gamepads.
//!
//! * Raw device events ([`RawEvent`]) are fed in by the web layer (DOM
//!   listeners, `navigator.getGamepads()` polling) or by tests.
//! * Games read named **actions** ([`ActionState`]), never raw keys.
//! * Bindings are strings (`"key:Space"`, `"pad:button:0"`, ...) stored as JSON
//!   and can be rebound at runtime, including "press a key to bind" capture.
//! * Edges (`just_pressed`) are latched until a simulation tick has consumed
//!   them, so a tap shorter than one frame or between ticks is never lost.
//!
//! Steady state does not allocate: key codes are inline byte arrays.

use core::fmt;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A `KeyboardEvent.code` value ("Space", "KeyW", "ArrowLeft") stored inline.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyCode {
    bytes: [u8; 24],
    len: u8,
}

impl KeyCode {
    /// `None` if empty or longer than 24 bytes.
    pub fn new(code: &str) -> Option<Self> {
        let b = code.as_bytes();
        if b.is_empty() || b.len() > 24 {
            return None;
        }
        let mut bytes = [0u8; 24];
        bytes[..b.len()].copy_from_slice(b);
        Some(Self {
            bytes,
            len: b.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl fmt::Debug for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyCode({})", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAxis {
    Dx,
    Dy,
    Wheel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisSign {
    Both,
    Positive,
    Negative,
}

/// One physical input bound to an action. String form in brackets:
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Binding {
    /// `key:Space`
    Key(KeyCode),
    /// `keys:KeyA/KeyD` (negative/positive) for axis actions.
    KeyAxis { neg: KeyCode, pos: KeyCode },
    /// `mouse:0` (0 left, 1 middle, 2 right, 3/4 extra).
    MouseButton(u8),
    /// `mouse:dx`, `mouse:dy`, `mouse:wheel`.
    Mouse(MouseAxis),
    /// `pointer`: the primary pointer (mouse, pen or first touch) is down.
    Pointer,
    /// `pad:button:N` (standard mapping index; analog triggers give 0..1).
    PadButton(u8),
    /// `pad:axis:N`, `pad:axis:N:+`, `pad:axis:N:-`.
    PadAxis { axis: u8, sign: AxisSign },
    /// `padP:button:N`: button N of one gamepad only (P is 1-based, 1 to
    /// [`MAX_PADS`]; `pad` stores the 0-based index). For local multiplayer.
    PadNButton { pad: u8, button: u8 },
    /// `padP:axis:N`, `padP:axis:N:+`, `padP:axis:N:-` on one gamepad only.
    PadNAxis { pad: u8, axis: u8, sign: AxisSign },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingParseError(pub String);

impl fmt::Display for BindingParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad binding: {}", self.0)
    }
}

impl core::str::FromStr for Binding {
    type Err = BindingParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || BindingParseError(s.to_string());
        let key = |c: &str| KeyCode::new(c).ok_or_else(err);
        let num = |c: &str| c.parse::<u8>().map_err(|_| err());
        let parts: Vec<&str> = s.trim().split(':').collect();
        Ok(match parts.as_slice() {
            ["key", code] => Binding::Key(key(code)?),
            ["keys", pair] => {
                let (neg, pos) = pair.split_once('/').ok_or_else(err)?;
                Binding::KeyAxis {
                    neg: key(neg)?,
                    pos: key(pos)?,
                }
            }
            ["mouse", "dx"] => Binding::Mouse(MouseAxis::Dx),
            ["mouse", "dy"] => Binding::Mouse(MouseAxis::Dy),
            ["mouse", "wheel"] => Binding::Mouse(MouseAxis::Wheel),
            ["mouse", n] => Binding::MouseButton(num(n)?),
            ["pointer"] => Binding::Pointer,
            ["pad", "button", n] => Binding::PadButton(num(n)?),
            ["pad", "axis", n] => Binding::PadAxis {
                axis: num(n)?,
                sign: AxisSign::Both,
            },
            ["pad", "axis", n, "+"] => Binding::PadAxis {
                axis: num(n)?,
                sign: AxisSign::Positive,
            },
            ["pad", "axis", n, "-"] => Binding::PadAxis {
                axis: num(n)?,
                sign: AxisSign::Negative,
            },
            [dev, kind, n, rest @ ..] if dev.len() > 3 && dev.starts_with("pad") => {
                let p = num(&dev[3..])?;
                if p == 0 || usize::from(p) > MAX_PADS {
                    return Err(err());
                }
                let pad = p - 1;
                match (*kind, rest) {
                    ("button", []) => Binding::PadNButton { pad, button: num(n)? },
                    ("axis", []) => Binding::PadNAxis {
                        pad,
                        axis: num(n)?,
                        sign: AxisSign::Both,
                    },
                    ("axis", ["+"]) => Binding::PadNAxis {
                        pad,
                        axis: num(n)?,
                        sign: AxisSign::Positive,
                    },
                    ("axis", ["-"]) => Binding::PadNAxis {
                        pad,
                        axis: num(n)?,
                        sign: AxisSign::Negative,
                    },
                    _ => return Err(err()),
                }
            }
            _ => return Err(err()),
        })
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Binding::Key(k) => write!(f, "key:{}", k.as_str()),
            Binding::KeyAxis { neg, pos } => write!(f, "keys:{}/{}", neg.as_str(), pos.as_str()),
            Binding::MouseButton(b) => write!(f, "mouse:{b}"),
            Binding::Mouse(MouseAxis::Dx) => write!(f, "mouse:dx"),
            Binding::Mouse(MouseAxis::Dy) => write!(f, "mouse:dy"),
            Binding::Mouse(MouseAxis::Wheel) => write!(f, "mouse:wheel"),
            Binding::Pointer => write!(f, "pointer"),
            Binding::PadButton(b) => write!(f, "pad:button:{b}"),
            Binding::PadAxis { axis, sign } => match sign {
                AxisSign::Both => write!(f, "pad:axis:{axis}"),
                AxisSign::Positive => write!(f, "pad:axis:{axis}:+"),
                AxisSign::Negative => write!(f, "pad:axis:{axis}:-"),
            },
            Binding::PadNButton { pad, button } => write!(f, "pad{}:button:{button}", pad + 1),
            Binding::PadNAxis { pad, axis, sign } => match sign {
                AxisSign::Both => write!(f, "pad{}:axis:{axis}", pad + 1),
                AxisSign::Positive => write!(f, "pad{}:axis:{axis}:+", pad + 1),
                AxisSign::Negative => write!(f, "pad{}:axis:{axis}:-", pad + 1),
            },
        }
    }
}

impl Serialize for Binding {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    /// Digital: `down` when value >= 0.5.
    #[default]
    Button,
    /// Analog: value in roughly -1..1 (mouse deltas are unclamped).
    Axis,
}

/// An action and its bindings, as stored in bindings JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDef {
    pub name: String,
    #[serde(default)]
    pub kind: ActionKind,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

impl ActionDef {
    pub fn button(name: &str, bindings: &[&str]) -> Result<Self, BindingParseError> {
        Self::make(name, ActionKind::Button, bindings)
    }

    pub fn axis(name: &str, bindings: &[&str]) -> Result<Self, BindingParseError> {
        Self::make(name, ActionKind::Axis, bindings)
    }

    fn make(name: &str, kind: ActionKind, bindings: &[&str]) -> Result<Self, BindingParseError> {
        Ok(Self {
            name: name.to_string(),
            kind,
            description: String::new(),
            bindings: bindings
                .iter()
                .map(|b| b.parse())
                .collect::<Result<_, _>>()?,
        })
    }

    pub fn describe(mut self, text: &str) -> Self {
        self.description = text.to_string();
        self
    }
}

/// The per-tick state of one action.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ActionState {
    pub value: f32,
    pub down: bool,
    pub just_pressed: bool,
    pub just_released: bool,
}

/// Index of an action, for allocation-free lookups in systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerKind {
    #[default]
    Mouse,
    Pen,
    Touch,
}

/// Raw device events, produced by the web layer or tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawEvent {
    Key { code: KeyCode, down: bool },
    MouseButton { button: u8, down: bool },
    MouseMove { x: f32, y: f32, dx: f32, dy: f32 },
    Wheel { dy: f32 },
    Pointer {
        id: i32,
        x: f32,
        y: f32,
        down: bool,
        primary: bool,
        kind: PointerKind,
    },
    /// Pointer left/cancelled: forget it.
    PointerGone { id: i32 },
    /// Window lost focus: release everything (avoids stuck keys).
    Blur,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct ButtonState {
    down: bool,
    pressed: bool,
    released: bool,
}

impl ButtonState {
    fn set(&mut self, down: bool) {
        if down && !self.down {
            self.pressed = true;
        }
        if !down && self.down {
            self.released = true;
        }
        self.down = down;
    }
    fn clear_edges(&mut self) {
        self.pressed = false;
        self.released = false;
    }
}

/// A tracked pointer (mouse, pen or one touch).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointerState {
    pub id: i32,
    pub active: bool,
    pub x: f32,
    pub y: f32,
    pub down: bool,
    pub primary: bool,
    pub kind: PointerKind,
}

/// The primary pointer as a game reads it: where it is (or was last seen),
/// where the current or last press started, its kind, and the button edges.
/// Unlike [`Input::pointers`], the position outlives the pointer, so a tap
/// whose down, up and gone all arrive in one frame (touch, synthetic events)
/// still says where it happened.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PrimaryPointer {
    /// Last known position, CSS pixels from the top-left of the page.
    pub pos: [f32; 2],
    /// Position of the last press (down edge).
    pub press_pos: [f32; 2],
    pub kind: PointerKind,
    pub down: bool,
    /// Pressed since the last tick (latched until [`Input::end_tick`]).
    pub pressed: bool,
    /// Released since the last tick.
    pub released: bool,
}

pub const MAX_PADS: usize = 4;
pub const PAD_BUTTONS: usize = 17;
pub const PAD_AXES: usize = 4;
const MAX_POINTERS: usize = 10;

/// One gamepad (W3C "standard" mapping).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadState {
    pub connected: bool,
    pub buttons: [f32; PAD_BUTTONS],
    pub axes: [f32; PAD_AXES],
    pressed_latch: [bool; PAD_BUTTONS],
}

/// Pending "press something to bind it" request.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Capture {
    action: ActionId,
    slot: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureResult {
    pub action: ActionId,
    pub slot: usize,
    pub binding: Binding,
}

/// All input state. A world resource.
#[derive(Debug, Clone)]
pub struct Input {
    defs: Vec<ActionDef>,
    states: Vec<ActionState>,
    keys: Vec<(KeyCode, ButtonState)>,
    mouse: [ButtonState; 5],
    mouse_pos: [f32; 2],
    mouse_delta: [f32; 2],
    wheel: f32,
    pointers: [PointerState; MAX_POINTERS],
    primary_pointer: ButtonState,
    primary_pos: [f32; 2],
    primary_press_pos: [f32; 2],
    primary_kind: PointerKind,
    pads: [PadState; MAX_PADS],
    /// Gamepad stick deadzone.
    pub deadzone: f32,
    capture: Option<Capture>,
    last_capture: Option<CaptureResult>,
    /// On-screen touch controls (feature `touch`): one slot per action a
    /// control drives, merged like one more binding.
    #[cfg(feature = "touch")]
    touch: Vec<TouchSlot>,
}

/// What the touch layer sets on one action: a quantised value and a latched
/// press edge (so a tap within one frame is still seen, as with keys).
#[cfg(feature = "touch")]
#[derive(Debug, Clone, Copy, PartialEq)]
struct TouchSlot {
    action: ActionId,
    value: i8,
    pressed: bool, #[doc = " Look pad movement this frame (CSS pixels, quantised), added to the action's value like `mouse:dx`; cleared by [`Input::end_tick`]."] #[cfg(feature = "touch-look")] delta: f32,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            defs: Vec::new(),
            states: Vec::new(),
            keys: Vec::with_capacity(64),
            mouse: Default::default(),
            mouse_pos: [0.0; 2],
            mouse_delta: [0.0; 2],
            wheel: 0.0,
            pointers: Default::default(),
            primary_pointer: ButtonState::default(),
            primary_pos: [0.0; 2],
            primary_press_pos: [0.0; 2],
            primary_kind: PointerKind::Mouse,
            pads: Default::default(),
            deadzone: 0.15,
            capture: None,
            last_capture: None,
            #[cfg(feature = "touch")]
            touch: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    Binding(BindingParseError),
    Json(String),
    UnknownAction(String),
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputError::Binding(e) => write!(f, "{e}"),
            InputError::Json(e) => write!(f, "bindings json: {e}"),
            InputError::UnknownAction(a) => write!(f, "unknown action {a}"),
        }
    }
}

impl Input {
    // ---- actions & bindings ----------------------------------------------

    /// Add an action (or replace the bindings of an existing one).
    pub fn add_action(&mut self, def: ActionDef) -> ActionId {
        if let Some(id) = self.action_id(&def.name) {
            self.defs[id.0 as usize] = def;
            return id;
        }
        let id = ActionId(self.defs.len() as u16);
        self.defs.push(def);
        self.states.push(ActionState::default());
        id
    }

    pub fn action_id(&self, name: &str) -> Option<ActionId> {
        self.defs
            .iter()
            .position(|d| d.name == name)
            .map(|i| ActionId(i as u16))
    }

    pub fn actions(&self) -> &[ActionDef] {
        &self.defs
    }

    pub fn state(&self, id: ActionId) -> ActionState {
        self.states.get(id.0 as usize).copied().unwrap_or_default()
    }

    pub fn down(&self, id: ActionId) -> bool {
        self.state(id).down
    }

    pub fn just_pressed(&self, id: ActionId) -> bool {
        self.state(id).just_pressed
    }

    pub fn value(&self, id: ActionId) -> f32 {
        self.state(id).value
    }

    /// Name-based lookup (convenient; prefer [`ActionId`] in hot loops).
    pub fn state_by_name(&self, name: &str) -> ActionState {
        self.action_id(name).map(|id| self.state(id)).unwrap_or_default()
    }

    /// Replace one binding slot (appends if `slot` is past the end).
    pub fn rebind(&mut self, id: ActionId, slot: usize, binding: Binding) -> bool {
        let Some(def) = self.defs.get_mut(id.0 as usize) else {
            return false;
        };
        if slot < def.bindings.len() {
            def.bindings[slot] = binding;
        } else {
            def.bindings.push(binding);
        }
        true
    }

    /// Bind the next pressed key / mouse button / pad button to this slot.
    pub fn start_capture(&mut self, id: ActionId, slot: usize) {
        self.capture = Some(Capture { action: id, slot });
        self.last_capture = None;
    }

    pub fn cancel_capture(&mut self) {
        self.capture = None;
    }

    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// The last finished capture (cleared by the next `start_capture`).
    pub fn last_capture(&self) -> Option<CaptureResult> {
        self.last_capture
    }

    fn try_capture(&mut self, binding: Binding) -> bool {
        let Some(c) = self.capture.take() else {
            return false;
        };
        self.rebind(c.action, c.slot, binding);
        self.last_capture = Some(CaptureResult {
            action: c.action,
            slot: c.slot,
            binding,
        });
        true
    }

    /// Export all actions with their current bindings.
    pub fn bindings_json(&self) -> String {
        let map: BTreeMap<&str, &Vec<Binding>> = self
            .defs
            .iter()
            .map(|d| (d.name.as_str(), &d.bindings))
            .collect();
        serde_json::to_string_pretty(&map).unwrap_or_default()
    }

    /// Apply `{"action": ["key:Space", ...]}`. Atomic: on any error nothing
    /// changes. Unknown actions are an error.
    pub fn apply_bindings_json(&mut self, json: &str) -> Result<(), InputError> {
        let map: BTreeMap<String, Vec<String>> =
            serde_json::from_str(json).map_err(|e| InputError::Json(e.to_string()))?;
        let mut parsed = Vec::with_capacity(map.len());
        for (name, list) in &map {
            let id = self
                .action_id(name)
                .ok_or_else(|| InputError::UnknownAction(name.clone()))?;
            let bindings = list
                .iter()
                .map(|b| b.parse::<Binding>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(InputError::Binding)?;
            parsed.push((id, bindings));
        }
        for (id, bindings) in parsed {
            self.defs[id.0 as usize].bindings = bindings;
        }
        Ok(())
    }

    /// Load full action definitions (`[{"name":..,"kind":..,"bindings":[..]}]`).
    pub fn load_actions_json(&mut self, json: &str) -> Result<(), InputError> {
        let defs: Vec<ActionDef> =
            serde_json::from_str(json).map_err(|e| InputError::Json(e.to_string()))?;
        for d in defs {
            self.add_action(d);
        }
        Ok(())
    }

    // ---- raw device input ---------------------------------------------------

    fn key_state(&mut self, code: KeyCode) -> &mut ButtonState {
        let i = match self.keys.iter().position(|(c, _)| *c == code) {
            Some(i) => i,
            None => {
                self.keys.push((code, ButtonState::default()));
                self.keys.len() - 1
            }
        };
        &mut self.keys[i].1
    }

    fn key(&self, code: &KeyCode) -> ButtonState {
        self.keys
            .iter()
            .find(|(c, _)| c == code)
            .map(|(_, s)| *s)
            .unwrap_or_default()
    }

    /// Feed one raw event.
    pub fn handle(&mut self, ev: &RawEvent) {
        match *ev {
            RawEvent::Key { code, down } => {
                if down && !self.key(&code).down && self.try_capture(Binding::Key(code)) {
                    return;
                }
                self.key_state(code).set(down);
            }
            RawEvent::MouseButton { button, down } => {
                if down && self.try_capture(Binding::MouseButton(button)) {
                    return;
                }
                if let Some(b) = self.mouse.get_mut(button as usize) {
                    b.set(down);
                }
            }
            RawEvent::MouseMove { x, y, dx, dy } => {
                self.mouse_pos = [x, y];
                self.mouse_delta[0] += dx;
                self.mouse_delta[1] += dy;
            }
            RawEvent::Wheel { dy } => self.wheel += dy,
            RawEvent::Pointer {
                id,
                x,
                y,
                down,
                primary,
                kind,
            } => {
                let slot = self
                    .pointers
                    .iter()
                    .position(|p| p.active && p.id == id)
                    .or_else(|| self.pointers.iter().position(|p| !p.active));
                if let Some(i) = slot {
                    self.pointers[i] = PointerState {
                        id,
                        active: true,
                        x,
                        y,
                        down,
                        primary,
                        kind,
                    };
                }
                if primary {
                    if down && !self.primary_pointer.down {
                        self.primary_press_pos = [x, y];
                    }
                    self.primary_pos = [x, y];
                    self.primary_kind = kind;
                    self.primary_pointer.set(down);
                }
            }
            RawEvent::PointerGone { id } => {
                for p in &mut self.pointers {
                    if p.active && p.id == id {
                        if p.primary {
                            self.primary_pointer.set(false);
                        }
                        *p = PointerState::default();
                    }
                }
            }
            RawEvent::Blur => self.release_all(),
        }
    }

    /// Update one gamepad from a poll (`navigator.getGamepads()`).
    pub fn set_pad(&mut self, index: usize, connected: bool, buttons: &[f32], axes: &[f32]) {
        let Some(mut pad) = self.pads.get(index).copied() else {
            return;
        };
        pad.connected = connected;
        for i in 0..PAD_BUTTONS {
            let v = if connected {
                buttons.get(i).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            if v >= 0.5 && pad.buttons[i] < 0.5 {
                pad.pressed_latch[i] = true;
                if self.try_capture(Binding::PadButton(i as u8)) {
                    pad.pressed_latch[i] = false;
                }
            }
            pad.buttons[i] = v;
        }
        for i in 0..PAD_AXES {
            pad.axes[i] = if connected {
                axes.get(i).copied().unwrap_or(0.0)
            } else {
                0.0
            };
        }
        self.pads[index] = pad;
    }

    /// Touch layer: set an action's touch value (quantised, -127..127 for
    /// -1..1; see `crate::touch`). Going from 0 to non-zero latches a press
    /// edge, like a key going down.
    #[cfg(feature = "touch")]
    pub fn set_touch(&mut self, action: ActionId, value: i8) {
        let slot = self.touch_slot(action);
        if slot.value == 0 && value != 0 {
            slot.pressed = true;
        }
        slot.value = value;
    }

    /// Touch layer: one press edge without holding (a tap-mode button).
    #[cfg(feature = "touch")]
    pub fn tap_touch(&mut self, action: ActionId) {
        self.touch_slot(action).pressed = true;
    } #[doc = " Touch layer (look pad): add pointer movement to an axis action for this frame, in the same units as `mouse:dx` / `mouse:dy` (CSS pixels). It is summed with the action's other bindings by the usual rule (the largest magnitude wins) and cleared when the tick ends, as a mouse delta is."] #[cfg(feature = "touch-look")] pub fn add_touch_delta(&mut self, action: ActionId, delta: f32) { if delta.is_finite() { self.touch_slot(action).delta += delta; } }

    #[cfg(feature = "touch")]
    fn touch_slot(&mut self, action: ActionId) -> &mut TouchSlot {
        let i = match self.touch.iter().position(|t| t.action == action) {
            Some(i) => i,
            None => {
                self.touch.push(TouchSlot { action, value: 0, pressed: false, #[cfg(feature = "touch-look")] delta: 0.0 });
                self.touch.len() - 1
            }
        };
        &mut self.touch[i]
    }

    fn release_all(&mut self) {
        #[cfg(feature = "touch")]
        for t in &mut self.touch {
            t.value = 0; #[cfg(feature = "touch-look")] { t.delta = 0.0; }
        }
        for (_, k) in &mut self.keys {
            k.set(false);
        }
        for b in &mut self.mouse {
            b.set(false);
        }
        self.primary_pointer.set(false);
        for p in &mut self.pointers {
            p.down = false;
        }
    }

    pub fn mouse_pos(&self) -> [f32; 2] {
        self.mouse_pos
    }

    /// The primary pointer (mouse, pen or first touch); see [`PrimaryPointer`].
    pub fn primary_pointer(&self) -> PrimaryPointer {
        PrimaryPointer {
            pos: self.primary_pos,
            press_pos: self.primary_press_pos,
            kind: self.primary_kind,
            down: self.primary_pointer.down,
            pressed: self.primary_pointer.pressed,
            released: self.primary_pointer.released,
        }
    }

    pub fn pointers(&self) -> impl Iterator<Item = &PointerState> {
        self.pointers.iter().filter(|p| p.active)
    }

    pub fn pad(&self, index: usize) -> Option<&PadState> {
        self.pads.get(index)
    }

    fn deadzoned(&self, v: f32) -> f32 {
        if v.abs() < self.deadzone { 0.0 } else { v }
    }

    /// (value, pressed-latch) for one binding.
    fn eval_binding(&self, b: &Binding) -> (f32, bool) {
        match b {
            Binding::Key(k) => {
                let s = self.key(k);
                (if s.down { 1.0 } else { 0.0 }, s.pressed)
            }
            Binding::KeyAxis { neg, pos } => {
                let (n, p) = (self.key(neg), self.key(pos));
                let v = f32::from(u8::from(p.down)) - f32::from(u8::from(n.down));
                (v, n.pressed || p.pressed)
            }
            Binding::MouseButton(i) => {
                let s = self.mouse.get(*i as usize).copied().unwrap_or_default();
                (if s.down { 1.0 } else { 0.0 }, s.pressed)
            }
            Binding::Mouse(MouseAxis::Dx) => (self.mouse_delta[0], false),
            Binding::Mouse(MouseAxis::Dy) => (self.mouse_delta[1], false),
            Binding::Mouse(MouseAxis::Wheel) => (self.wheel, false),
            Binding::Pointer => (
                if self.primary_pointer.down { 1.0 } else { 0.0 },
                self.primary_pointer.pressed,
            ),
            Binding::PadButton(i) => self.eval_pad_button(None, *i),
            Binding::PadAxis { axis, sign } => self.eval_pad_axis(None, *axis, *sign),
            Binding::PadNButton { pad, button } => self.eval_pad_button(Some(*pad), *button),
            Binding::PadNAxis { pad, axis, sign } => self.eval_pad_axis(Some(*pad), *axis, *sign),
        }
    }

    /// Connected pads: all of them, or only pad `only` (0-based).
    fn pads_for(&self, only: Option<u8>) -> impl Iterator<Item = &PadState> {
        self.pads
            .iter()
            .enumerate()
            .filter(move |(i, p)| p.connected && only.is_none_or(|o| usize::from(o) == *i))
            .map(|(_, p)| p)
    }

    fn eval_pad_button(&self, only: Option<u8>, button: u8) -> (f32, bool) {
        let i = button as usize;
        let mut best = (0.0f32, false);
        for p in self.pads_for(only) {
            if let (Some(&v), Some(&l)) = (p.buttons.get(i), p.pressed_latch.get(i)) {
                best.0 = best.0.max(v);
                best.1 |= l;
            }
        }
        best
    }

    fn eval_pad_axis(&self, only: Option<u8>, axis: u8, sign: AxisSign) -> (f32, bool) {
        let mut best = 0.0f32;
        for p in self.pads_for(only) {
            let raw = p.axes.get(axis as usize).copied().unwrap_or(0.0);
            let v = match sign {
                AxisSign::Both => raw,
                AxisSign::Positive => raw.max(0.0),
                AxisSign::Negative => (-raw).max(0.0),
            };
            let v = self.deadzoned(v);
            if v.abs() > best.abs() {
                best = v;
            }
        }
        (best, false)
    }

    /// Recompute action states from raw state. Called once per frame before
    /// the simulation steps. Edges stay latched until [`Input::end_tick`].
    pub fn update(&mut self) {
        for i in 0..self.defs.len() {
            let mut value = 0.0f32;
            let mut latched = false;
            for b in &self.defs[i].bindings {
                let (v, l) = self.eval_binding(b);
                if v.abs() > value.abs() {
                    value = v;
                }
                latched |= l;
            }
            #[cfg(feature = "touch")]
            if let Some(t) = self.touch.iter().find(|t| usize::from(t.action.0) == i) {
                let v = crate::touch::dequantise(t.value); #[cfg(feature = "touch-look")] let v = v + t.delta;
                if v.abs() > value.abs() {
                    value = v;
                }
                latched |= t.pressed;
            }
            let down = match self.defs[i].kind {
                ActionKind::Button => value >= 0.5,
                ActionKind::Axis => value != 0.0,
            };
            let s = &mut self.states[i];
            if (down && !s.down) || latched {
                s.just_pressed = true;
            }
            if !down && s.down {
                s.just_released = true;
            }
            if latched && !down {
                // Pressed and released within one frame: report both edges.
                s.just_released = true;
            }
            s.down = down;
            s.value = value;
        }
    }

    /// Clear edges and per-frame deltas after a simulation tick consumed them.
    pub fn end_tick(&mut self) {
        for s in &mut self.states {
            s.just_pressed = false;
            s.just_released = false;
        }
        for (_, k) in &mut self.keys {
            k.clear_edges();
        }
        for b in &mut self.mouse {
            b.clear_edges();
        }
        self.primary_pointer.clear_edges();
        #[cfg(feature = "touch")]
        for t in &mut self.touch {
            t.pressed = false; #[cfg(feature = "touch-look")] { t.delta = 0.0; }
        }
        for p in &mut self.pads {
            p.pressed_latch = [false; PAD_BUTTONS];
        }
        self.mouse_delta = [0.0; 2];
        self.wheel = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: &str, down: bool) -> RawEvent {
        RawEvent::Key {
            code: KeyCode::new(code).unwrap(),
            down,
        }
    }

    fn setup() -> (Input, ActionId, ActionId) {
        let mut i = Input::default();
        let jump = i
            .add_action(ActionDef::button("jump", &["key:Space", "pad:button:0", "pointer"]).unwrap());
        let mv = i.add_action(ActionDef::axis("move_x", &["keys:KeyA/KeyD", "pad:axis:0"]).unwrap());
        (i, jump, mv)
    }

    #[test]
    fn binding_strings_round_trip() {
        for s in [
            "key:Space",
            "keys:KeyA/KeyD",
            "mouse:0",
            "mouse:dx",
            "mouse:wheel",
            "pointer",
            "pad:button:3",
            "pad:axis:1",
            "pad:axis:1:+",
            "pad:axis:2:-",
            "pad1:button:0",
            "pad2:button:9",
            "pad4:axis:0",
            "pad2:axis:1:+",
            "pad3:axis:3:-",
        ] {
            let b: Binding = s.parse().unwrap();
            assert_eq!(b.to_string(), s);
        }
        assert!("key:".parse::<Binding>().is_err());
        assert!("pad:button:999".parse::<Binding>().is_err());
        for bad in ["pad0:button:0", "pad5:button:0", "padx:axis:0", "pad1:axis:0:x", "pad1:stick:0", "pad1:button:0:+"] {
            assert!(bad.parse::<Binding>().is_err(), "{bad}");
        }
        assert_eq!(
            "pad2:axis:1:-".parse::<Binding>().unwrap(),
            Binding::PadNAxis { pad: 1, axis: 1, sign: AxisSign::Negative }
        );
        assert!("nonsense".parse::<Binding>().is_err());
        assert!("key:ThisCodeIsWayTooLongToBeReal".parse::<Binding>().is_err());
    }

    #[test]
    fn key_press_edges_latch_until_tick() {
        let (mut i, jump, _) = setup();
        i.handle(&key("Space", true));
        i.update();
        assert!(i.down(jump) && i.just_pressed(jump));
        // A frame with no sim tick: edge must survive.
        i.update();
        assert!(i.just_pressed(jump));
        i.end_tick();
        i.update();
        assert!(i.down(jump) && !i.just_pressed(jump));
        i.handle(&key("Space", false));
        i.update();
        assert!(!i.down(jump) && i.state(jump).just_released);
    }

    #[test]
    fn tap_within_one_frame_is_not_lost() {
        let (mut i, jump, _) = setup();
        i.handle(&key("Space", true));
        i.handle(&key("Space", false));
        i.update();
        let s = i.state(jump);
        assert!(s.just_pressed && s.just_released && !s.down);
    }

    #[test]
    fn axis_from_keys_and_pad_with_deadzone() {
        let (mut i, _, mv) = setup();
        i.handle(&key("KeyD", true));
        i.update();
        assert_eq!(i.value(mv), 1.0);
        i.handle(&key("KeyA", true));
        i.update();
        assert_eq!(i.value(mv), 0.0);
        i.handle(&key("KeyA", false));
        i.handle(&key("KeyD", false));
        i.set_pad(0, true, &[], &[0.1, 0.0]);
        i.update();
        assert_eq!(i.value(mv), 0.0, "inside deadzone");
        i.set_pad(0, true, &[], &[-0.8, 0.0]);
        i.update();
        assert!((i.value(mv) + 0.8).abs() < 1e-6);
    }

    #[test]
    fn per_pad_bindings_only_hear_their_pad() {
        let mut i = Input::default();
        let p1 = i.add_action(ActionDef::button("p1_item", &["pad1:button:0"]).unwrap());
        let p2 = i.add_action(ActionDef::button("p2_item", &["pad2:button:0"]).unwrap());
        let s2 = i.add_action(ActionDef::axis("p2_steer", &["pad2:axis:0"]).unwrap());
        let any = i.add_action(ActionDef::button("any", &["pad:button:0"]).unwrap());
        // Pad 2 (index 1) presses A and pushes its stick right.
        i.set_pad(1, true, &[1.0], &[0.9, 0.0]);
        // Pad 1 pushes its stick left: p2_steer must not hear it.
        i.set_pad(0, true, &[0.0], &[-1.0, 0.0]);
        i.update();
        assert!(!i.down(p1));
        assert!(i.just_pressed(p2));
        assert!(i.down(any));
        assert!(i.value(s2) > 0.5);
        i.end_tick();
        i.set_pad(1, false, &[], &[]);
        i.update();
        assert!(!i.down(p2));
        assert_eq!(i.value(s2), 0.0);
    }

    #[test]
    fn pad_button_and_pointer() {
        let (mut i, jump, _) = setup();
        i.set_pad(1, true, &[1.0], &[]);
        i.update();
        assert!(i.just_pressed(jump));
        i.end_tick();
        i.set_pad(1, false, &[], &[]);
        i.update();
        assert!(!i.down(jump));
        i.handle(&RawEvent::Pointer {
            id: 7,
            x: 1.0,
            y: 2.0,
            down: true,
            primary: true,
            kind: PointerKind::Touch,
        });
        i.update();
        assert!(i.down(jump));
        assert_eq!(i.pointers().count(), 1);
        i.handle(&RawEvent::PointerGone { id: 7 });
        i.update();
        assert!(!i.down(jump));
        assert_eq!(i.pointers().count(), 0);
    }

    #[test]
    fn primary_pointer_keeps_a_one_frame_tap() {
        let (mut i, _, _) = setup();
        let p = |x: f32, down: bool| RawEvent::Pointer {
            id: 3,
            x,
            y: 40.0,
            down,
            primary: true,
            kind: PointerKind::Touch,
        };
        // Down, a move, up and gone all inside one frame.
        i.handle(&p(10.0, true));
        i.handle(&p(25.0, true));
        i.handle(&p(25.0, false));
        i.handle(&RawEvent::PointerGone { id: 3 });
        i.update();
        let pp = i.primary_pointer();
        assert_eq!(i.pointers().count(), 0);
        assert!(pp.pressed && pp.released && !pp.down);
        assert_eq!(pp.press_pos, [10.0, 40.0]);
        assert_eq!(pp.pos, [25.0, 40.0]);
        assert_eq!(pp.kind, PointerKind::Touch);
        i.end_tick();
        let pp = i.primary_pointer();
        assert!(!pp.pressed && !pp.released);
        assert_eq!(pp.pos, [25.0, 40.0], "position outlives the pointer");
    }

    #[test]
    fn blur_releases_everything() {
        let (mut i, jump, _) = setup();
        i.handle(&key("Space", true));
        i.update();
        i.handle(&RawEvent::Blur);
        i.update();
        assert!(!i.down(jump));
    }

    #[test]
    fn runtime_rebinding_and_capture() {
        let (mut i, jump, _) = setup();
        i.rebind(jump, 0, "key:KeyW".parse().unwrap());
        i.handle(&key("KeyW", true));
        i.update();
        assert!(i.down(jump));
        i.handle(&key("KeyW", false));
        i.end_tick();

        i.start_capture(jump, 0);
        i.handle(&key("KeyJ", true)); // captured, not delivered as input
        assert!(!i.is_capturing());
        let c = i.last_capture().unwrap();
        assert_eq!(c.binding.to_string(), "key:KeyJ");
        i.handle(&key("KeyJ", false));
        i.handle(&key("KeyJ", true));
        i.update();
        assert!(i.down(jump));
    }

    #[test]
    fn bindings_json_round_trip_and_atomic_errors() {
        let (mut i, jump, _) = setup();
        let json = i.bindings_json();
        assert!(json.contains("key:Space"));
        let before = i.actions().to_vec();
        assert!(
            i.apply_bindings_json(r#"{"jump":["key:KeyZ"],"nope":["key:KeyX"]}"#)
                .is_err()
        );
        assert_eq!(i.actions(), &before[..]);
        assert!(i.apply_bindings_json(r#"{"jump":["bad"]}"#).is_err());
        i.apply_bindings_json(r#"{"jump":["key:KeyZ","mouse:0"]}"#)
            .unwrap();
        assert_eq!(i.actions()[jump.0 as usize].bindings.len(), 2);
        i.apply_bindings_json(&json).unwrap();
        assert_eq!(i.actions(), &before[..]);
        i.load_actions_json(r#"[{"name":"fire","kind":"button","bindings":["mouse:0"]}]"#)
            .unwrap();
        assert!(i.action_id("fire").is_some());
    }
}
