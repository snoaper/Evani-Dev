//! On-screen controls for phones and tablets. They only set the actions the
//! keys already drive (`move_x`, `move_y`, `attack`, `item`, `pause`); there
//! is no separate touch path in the game (README "Touch controls").

use evani_core::touch::{Anchor, Button, Stick, TouchLayout};

/// The touch layout: a fixed move stick bottom left, attack and item buttons
/// bottom right, a pause button top right.
pub fn layout() -> TouchLayout {
    TouchLayout::new()
        .stick(Stick::two_axis("move", "move_x", "move_y").fixed().at(Anchor::BottomLeft, 6.0, 6.0))
        .button(Button::hold("attack", "attack", "A").primary().at(Anchor::BottomRight, 3.0, 5.0))
        .button(Button::hold("item", "item", "B").at(Anchor::BottomRight, 24.0, 3.0))
        .pause_at("pause", Anchor::TopRight, 3.0, 10.0)
}
