//! Page side of look pads (feature `touch-look`): an invisible drag area
//! over the pad's zone; the finger's movement is the look.

use super::{Part, TouchQueue, css, html, wire};
use crate::touch::LookPad;

/// Add look pad `l` (control `index`) to the layer being built.
pub(super) fn install(
    doc: &web_sys::Document,
    l: &LookPad,
    index: u16,
    q: &TouchQueue,
    errors: &mut Vec<String>,
    zones: &mut Vec<web_sys::HtmlElement>,
    parts: &mut Vec<Part>,
) -> Option<()> {
    let z = html(doc, "div", "et-c et-z")?;
    let _ = z.set_attribute("data-id", &l.id);
    let zn = l.zone;
    let _ = z.set_attribute(
        "style",
        &format!(
            "left:{}%;top:{}%;width:{}%;height:{}%",
            css(zn.x0 * 100.0),
            css(zn.y0 * 100.0),
            css((zn.x1 - zn.x0) * 100.0),
            css((zn.y1 - zn.y0) * 100.0)
        ),
    );
    wire(&z, index, None, q, errors);
    zones.push(z.clone());
    parts.push(Part {
        el: z,
        class: "et-c et-z",
        style: String::new(),
        knob: None,
        dynamic: false,
        held: false,
        knob_drawn: None,
    });
    Some(())
}
