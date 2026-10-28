//! Look pads (feature `touch-look`, S5-E1): a drag area whose finger movement
//! drives the same look actions as the mouse. Its own feature so only games
//! that use one (tactical-fps) download it; the hooks into `touch.rs` sit on
//! the ends of existing lines so every other game builds byte for byte as
//! before.

use super::{Control, Resolved, TouchControls, TouchLayout, Zone};
use crate::input::{ActionId, Input};

/// A drag area that looks around: finger movement is added to one or two axis
/// actions every frame, in the same units as `mouse:dx` / `mouse:dy` (CSS
/// pixels), so the kit binds `look_x` to `["mouse:dx", ...]` once and the
/// pad drives that same action. Unlike a [`super::Stick`] it has no centre:
/// only how far the finger moved counts, and lifting it stops the look.
/// Invisible (style it with `#evani-touch [data-id=look] { ... }`). Values
/// follow the mouse: x right positive, y down positive.
#[derive(Debug, Clone, PartialEq)]
pub struct LookPad {
    pub id: String,
    pub x_action: Option<String>,
    pub y_action: Option<String>,
    /// Where a drag can start. Buttons and sticks placed over it win.
    pub zone: Zone,
    /// Pixels of finger movement to action units (1.0 = as a mouse pixel).
    pub sensitivity: f32,
    pub invert_y: bool,
}

impl LookPad {
    /// Look on both axes: `x_action` for left/right, `y_action` for up/down.
    pub fn new(id: &str, x_action: &str, y_action: &str) -> Self {
        Self {
            id: id.to_string(),
            x_action: Some(x_action.to_string()),
            y_action: Some(y_action.to_string()),
            zone: Zone::RIGHT_HALF,
            sensitivity: 1.0,
            invert_y: false,
        }
    }

    /// Left/right only.
    pub fn horizontal(id: &str, x_action: &str) -> Self {
        Self { y_action: None, ..Self::new(id, x_action, x_action) }
    }

    pub fn zone(mut self, zone: Zone) -> Self {
        self.zone = zone;
        self
    }

    /// Scale finger movement (touch usually wants 1.5 to 3 times a mouse).
    pub fn sensitivity(mut self, s: f32) -> Self {
        self.sensitivity = s;
        self
    }

    pub fn invert_y(mut self) -> Self {
        self.invert_y = true;
        self
    }
}

impl TouchLayout {
    /// A look pad (see [`LookPad`]): drag to turn the view, driving the same
    /// actions as the mouse.
    pub fn look_pad(mut self, l: LookPad) -> Self {
        self.controls.push(Control::LookPad(l));
        self
    }
}

/// Look pad movement -> what `Input` sees: 1/16 pixel steps, so the sim is
/// fed the same kind of discrete value as a quantised stick.
pub fn quantise_delta(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    (v.clamp(-4096.0, 4096.0) * 16.0).round() / 16.0
}

/// Resolve a look pad's action names (`find` is `TouchControls::new`'s lookup).
pub(super) fn resolve<F>(l: &LookPad, find: F) -> Result<Resolved, String>
where
    F: Fn(&str) -> Result<ActionId, String> + Copy,
{
    let x = l.x_action.as_deref().map(find).transpose()?;
    let y = l.y_action.as_deref().map(find).transpose()?;
    if x.is_none() && y.is_none() {
        return Err(format!("touch look pad `{}` drives no action", l.id));
    }
    // Look is a per-frame movement, not a held value, so its actions stay
    // out of `actions` (nothing to sum or clear).
    let s = if l.sensitivity.is_finite() { l.sensitivity } else { 1.0 };
    Ok(Resolved::Look { x, y, scale: [s, if l.invert_y { -s } else { s }] })
}

impl TouchControls {
    /// A finger on look pad `i` moved to `at`: feed the movement since the
    /// last position to its actions. False when control `i` is not a look
    /// pad (the caller then treats it as a stick or button).
    pub(super) fn look_drag(&mut self, input: &mut Input, i: usize, at: [f32; 2]) -> bool {
        let Some(&Resolved::Look { x, y, scale }) = self.resolved.get(i) else {
            return false;
        };
        let Some(st) = self.state.get_mut(i) else { return true };
        let d = [(at[0] - st.base[0]) * scale[0], (at[1] - st.base[1]) * scale[1]];
        st.base = at;
        if self.shown && self.enabled {
            if let Some(a) = x {
                input.add_touch_delta(a, quantise_delta(d[0]));
            }
            if let Some(a) = y {
                input.add_touch_delta(a, quantise_delta(d[1]));
            }
        }
        true
    }
}
