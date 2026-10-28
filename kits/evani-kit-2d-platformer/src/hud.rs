//! [`PlatformerHud`]: what a game's menus show, refreshed from the
//! [`Game`] by the draw system every frame. The kit draws the in-play HUD
//! itself (pixel glyphs across the top); the demo's DOM menus read this for
//! the title, pause and results screens.

use crate::game::{Game, Phase, Results};

/// World resource.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlatformerHud {
    pub phase: Phase,
    /// Seconds in this phase (menus wait a moment after the goal or the
    /// last life before showing the results).
    pub phase_time: f32,
    pub lives: i32,
    pub coins: u32,
    pub score: u32,
    /// Whole seconds left on the level timer (rounded up).
    pub time_left: u32,
    /// True for the last 30 seconds (the HUD timer flashes).
    pub hurry: bool,
    /// Set once the level is over.
    pub results: Option<Results>,
    pub level_name: String,
    /// Bumped on every new run (menus reset on a change).
    pub run_no: u64,
    /// Checkpoints raised and the level's count.
    pub flags: usize,
    pub flags_total: usize,
    /// Free roam: no lives, timer or goal; the HUD shows a label instead.
    pub free_roam: bool,
}

impl PlatformerHud {
    /// Refresh from the game.
    pub fn update(&mut self, g: &Game) {
        self.phase = g.phase;
        self.phase_time = g.phase_time;
        self.lives = g.lives.max(0);
        self.coins = g.coins;
        self.score = g.score;
        self.time_left = g.time_left.max(0.0).ceil() as u32;
        self.hurry = g.phase == Phase::Playing && g.time_left < 30.0;
        self.results = g.results;
        if self.level_name != g.level.name {
            self.level_name.clone_from(&g.level.name);
        }
        self.run_no = g.run_no;
        self.flags = g.flags.iter().filter(|f| **f).count();
        self.flags_total = g.flags.len();
        self.free_roam = g.free_roam;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Request;
    use crate::level::Level;
    use crate::{Ints, LEVEL_JSON, Tune};
    use std::rc::Rc;

    #[test]
    fn mirrors_the_game() {
        let (t, n) = (Tune::default(), Ints::default());
        let level = Rc::new(Level::from_json(LEVEL_JSON).unwrap());
        let mut g = Game::new(level, &t, &n);
        let mut hud = PlatformerHud::default();
        hud.update(&g);
        assert_eq!(hud.phase, Phase::Title);
        assert_eq!(hud.level_name, "Bramble Hills");
        g.request(Request::Start);
        g.tick(Default::default(), &t, &n, 980.0, 1.0 / 60.0);
        hud.update(&g);
        assert_eq!(hud.phase, Phase::Playing);
        assert_eq!(hud.lives, n.lives);
        assert_eq!(hud.time_left, g.time_left.ceil() as u32);
        assert!(hud.flags_total > 0 && hud.flags == 0);
        assert!(hud.results.is_none());
    }
}
