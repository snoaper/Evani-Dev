//! The end-of-race timeline the race rules need (no drawing): when the
//! celebration gives way to the podium, and the roll-out brake of a human
//! who has crossed the line. [`crate::finale`] draws the rest and re-exports
//! these, so the race rules build without the graphics (a headless referee).

use crate::race::Phase;

/// Seconds after the last human crosses the line before the podium scene
/// and the results panel take over (the celebration camera's whole run).
pub const CELEBRATE_S: f32 = 4.5;
/// Brake held by a human kart that has finished (0..1): it rolls to a stop
/// past the line instead of driving on at full speed under the orbit.
pub const ROLL_OUT_BRAKE: f32 = 1.0;

/// Which part of the end of the race the game shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Racing, or the grid.
    Race,
    /// Every human is over the line and the camera is still orbiting.
    Celebrate,
    /// The podium scene with the results panel beside it.
    Podium,
}

/// The stage for a race phase, its seconds in that phase and whether there
/// are local players (an all-AI race has no celebration).
pub fn stage(phase: Phase, phase_time: f32, players: usize) -> Stage {
    match phase {
        Phase::Finished if players == 0 || phase_time >= CELEBRATE_S => Stage::Podium,
        Phase::Finished => Stage::Celebrate,
        _ => Stage::Race,
    }
}
