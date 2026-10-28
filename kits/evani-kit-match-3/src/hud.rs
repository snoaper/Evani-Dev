//! What a HUD shows, filled every frame from the game (the app's DOM UI
//! reads it; the kit never touches the DOM). Positions are CSS pixels.

use evani_core::ecs::World;

use crate::draw::{Style, entry_reach};
use crate::ecs::{Blast, Floater, Floats, Grid};
use crate::game::{Game, Mood, Phase};
use crate::layout::Layout;
use crate::level::Goal;

/// One goal chip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalView {
    /// `collect`, `frosting` or `score`.
    pub kind: &'static str,
    /// Piece colour for `collect`.
    pub color: u8,
    pub have: u32,
    pub need: u32,
    pub done: bool,
}

/// One floating text (score pop or combo callout).
#[derive(Debug, Clone, PartialEq)]
pub struct FloatView {
    pub id: u32,
    /// Where it rises from (CSS px).
    pub x: f32,
    pub y: f32,
    pub text: String,
    /// 0 score, 1 callout, 2 big callout.
    pub style: u8,
    /// 0..1 of its life.
    pub age: f32,
    /// Font size in CSS pixels (callouts shrink to fit their spot).
    pub font: f32,
}

impl FloatView {
    /// The text's box `[x, y, w, h]` in CSS pixels as the page draws it
    /// (centred on `x, y`, scaled by [`float_scale`]).
    pub fn rect(&self) -> [f32; 4] {
        let s = float_scale(self.style, self.age);
        let w = self.text.chars().count() as f32 * EM_PER_CHAR * self.font * s;
        let h = 1.1 * self.font * s;
        [self.x - w * 0.5, self.y - h * 0.5, w, h]
    }
}

/// Average advance of one character of the page's bold Trebuchet MS, in em.
pub const EM_PER_CHAR: f32 = 0.6;

/// The largest [`float_scale`] a callout reaches (as it pops in).
const CALLOUT_POP: f32 = 1.15;

/// Characters of the shortest combo callout (`game::CALLOUTS`, "Nice!"; a
/// test holds it): the one that gets the biggest font on a given panel.
pub const CALLOUT_MIN_CHARS: usize = 5;

/// Clear space above and below a callout in its landscape slot (CSS px).
const CALLOUT_PAD: f32 = 3.0;

/// The floor of a callout's font (CSS px): portrait and landscape.
const CALLOUT_FLOOR: [[f32; 2]; 2] = [[16.0, 18.0], [22.0, 26.0]];

/// The base font (CSS px) of a callout of `style` (1 or 2) on a screen with
/// `vmin` CSS px on its short side, before it is fitted to its spot.
/// Landscape floors are lower than portrait's: the callout has a slot of its
/// own between the HUD panel and Puff ([`callout_slot`]), which a short
/// screen has little room for.
fn callout_base(style: u8, vmin: f32, portrait: bool) -> f32 {
    let [lo1, lo2] = CALLOUT_FLOOR[usize::from(portrait)];
    if style <= 1 { (5.0 * vmin / 100.0).clamp(lo1, 54.0) } else { (6.0 * vmin / 100.0).clamp(lo2, 64.0) }
}

/// Landscape: the height (CSS px) of the slot a combo callout gets between
/// the HUD panel's shadow and the top of Puff's reach (`layout`): the biggest
/// box any callout can have there ([`callout_base`] of the big style, fitted
/// to the panel's width for the shortest word, 1.1 em high, popped to
/// [`CALLOUT_POP`]) and a little air. The layout keeps Puff out of it, so a
/// callout never covers his sprout star at any size.
pub fn callout_slot(vw: f32, vh: f32, hud_w: f32) -> f32 {
    let base = callout_base(2, vw.min(vh), false);
    let fit = hud_w * 0.9 / (CALLOUT_MIN_CHARS as f32 * EM_PER_CHAR * CALLOUT_POP);
    base.min(fit).max(12.0) * 1.1 * CALLOUT_POP + 2.0 * CALLOUT_PAD
}

/// The result screen.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResultView {
    pub won: bool,
    pub stars: u8,
    pub score: u32,
    pub best_cascade: u32,
    pub moves_used: i32,
    /// The result screen may show (the board has settled a moment).
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Match3Hud {
    pub phase: Phase,
    pub paused: bool,
    pub level: usize,
    pub levels: usize,
    pub level_name: String,
    pub moves_left: i32,
    pub moves_total: i32,
    pub score: u32,
    pub stars_at: [u32; 3],
    /// Stars earned by the score so far.
    pub stars: u8,
    /// Star bar fill 0..1 (the third star's score is full).
    pub star_fill: f32,
    pub goals: Vec<GoalView>,
    pub floats: Vec<FloatView>,
    pub layout: Layout,
    /// The hinted move's two piece centres `[ax, ay, bx, by]`, when shown.
    pub hint: Option<[f32; 4]>,
    /// Centres of the best move right now (also when no hint shows yet):
    /// the review script plays through it like a player would.
    pub best: Option<[f32; 4]>,
    /// The weakest move (the review script plays it to show a lost level).
    pub worst: Option<[f32; 4]>,
    /// Specials going off right now: 1 Streak, 2 Pop Bomb, 4 Prism.
    pub blasting: u8,
    /// The board is waiting for a move.
    pub idle: bool,
    pub cascade: u32,
    pub result: ResultView,
    pub mood: Mood,
    /// Seconds in this phase.
    pub phase_t: f32,
    /// Bumped every time a level starts.
    pub runs: u32,
    /// Free roam: no move limit, goals or result.
    pub free_roam: bool,
    best_key: (u32, i32, bool),
}

impl Default for Match3Hud {
    fn default() -> Self {
        Self {
            phase: Phase::Title,
            paused: false,
            level: 0,
            levels: 0,
            level_name: String::new(),
            moves_left: 0,
            moves_total: 0,
            score: 0,
            stars_at: [0; 3],
            stars: 0,
            star_fill: 0.0,
            goals: Vec::new(),
            floats: Vec::new(),
            layout: Layout::default(),
            hint: None,
            best: None,
            worst: None,
            blasting: 0,
            idle: false,
            cascade: 0,
            result: ResultView::default(),
            mood: Mood::Idle,
            phase_t: 0.0,
            runs: 0,
            free_roam: false,
            best_key: (u32::MAX, 0, false),
        }
    }
}

impl Match3Hud {
    /// Refresh from the game in `w` (its [`Game`], [`Grid`] and [`Floats`];
    /// allocates only when goals or floats change), drawn in `style` (callouts
    /// keep clear of the pieces it draws dropping in).
    pub fn update(&mut self, w: &World, style: &Style, shake_px: [f32; 2], result_delay_s: f32) {
        let (Some(g), Some(grid), Some(floats)) = (w.try_res::<Game>(), w.try_res::<Grid>(), w.try_res::<Floats>()) else {
            return;
        };
        if g.levels.is_empty() {
            return;
        }
        let lv = g.current();
        self.free_roam = g.free_roam;
        self.phase = g.phase;
        self.paused = g.paused;
        self.level = g.level;
        self.levels = g.levels.len();
        if self.level_name != lv.name {
            self.level_name = lv.name.clone();
        }
        self.moves_left = g.moves_left.max(0);
        self.moves_total = lv.moves;
        self.score = g.score;
        self.stars_at = lv.stars;
        self.stars = lv.stars.iter().filter(|&&s| g.score >= s).count() as u8;
        self.star_fill = if lv.stars[2] > 0 { (g.score as f32 / lv.stars[2] as f32).min(1.0) } else { 1.0 };
        let goals: Vec<GoalView> = g
            .goals(&grid.board)
            .iter()
            .map(|s| {
                let (kind, color) = match s.goal {
                    Goal::Collect { color, .. } => ("collect", color),
                    Goal::Frosting => ("frosting", 0),
                    Goal::Score(_) => ("score", 0),
                };
                GoalView {
                    kind,
                    color,
                    have: s.have,
                    need: s.need,
                    done: s.done(),
                }
            })
            .collect();
        if goals != self.goals {
            self.goals = goals;
        }
        self.layout = g.layout;
        let centres = |a: usize, b: usize| {
            let (ax, ay) = grid.board.xy(a);
            let (bx, by) = grid.board.xy(b);
            let pa = g.layout.center(ax as f32, ay as f32);
            let pb = g.layout.center(bx as f32, by as f32);
            [pa[0], pa[1], pb[0], pb[1]]
        };
        self.hint = g.hint.map(|m| centres(m.a, m.b));
        self.idle = g.step == crate::game::Step::Idle && g.phase == Phase::Play;
        // The move search clones the board per candidate: only when it changed.
        let key = (g.runs, g.moves_used, self.idle);
        if key != self.best_key || (self.idle && self.best.is_none()) {
            self.best_key = key;
            self.best = if self.idle { grid.board.best_move().map(|m| centres(m.a, m.b)) } else { None };
            self.worst = if self.idle { grid.board.worst_move().map(|m| centres(m.a, m.b)) } else { None };
        }
        self.cascade = g.cascade;
        self.blasting = grid
            .blasts
            .iter()
            .filter_map(|&e| w.get::<Blast>(e).map(|b| *b))
            .filter(|b| b.t > 0.0)
            .map(|b| match b.kind {
                crate::board::Kind::StreakH | crate::board::Kind::StreakV => 1,
                crate::board::Kind::Bomb => 2,
                crate::board::Kind::Prism => 4,
                crate::board::Kind::Normal => 0,
            })
            .fold(0, |a, b| a | b);
        self.mood = grid.puff(w).mood;
        self.phase_t = g.phase_t;
        self.runs = g.runs;
        let ready = matches!(g.phase, Phase::Won | Phase::Lost) && g.phase_t >= result_delay_s;
        self.result = ResultView {
            won: g.phase == Phase::Won,
            stars: g.stars,
            score: g.score,
            best_cascade: g.best_cascade,
            moves_used: g.moves_used,
            ready,
        };
        // Floating text in CSS pixels, rising as it ages.
        let fl = &floats.items;
        // How high pieces dropping in show.
        let reach = entry_reach(g.layout.cell, style);
        let same = self.floats.len() == fl.len() && self.floats.iter().zip(fl).all(|(a, b)| a.id == b.id);
        if !same {
            self.floats = fl.iter().map(|f| float_view(f, &g.layout, shake_px, reach)).collect();
        } else {
            for (v, f) in self.floats.iter_mut().zip(fl) {
                let n = float_view(f, &g.layout, shake_px, reach);
                v.x = n.x;
                v.y = n.y;
                v.age = n.age;
                v.font = n.font;
            }
        }
    }
}

/// Opacity of a floating text at `age` (0..1 of its life): full, then
/// fading out over the last 30 %.
pub fn float_opacity(age: f32) -> f32 {
    if age > 0.7 { ((1.0 - age) / 0.3).max(0.0) } else { 1.0 }
}

/// Scale of a floating text at `age`: score texts stay at 1; callouts pop
/// in from 0.6 and settle.
pub fn float_scale(style: u8, age: f32) -> f32 {
    if style == 0 { 1.0 } else { 0.6 + 0.4 * (age * 8.0).min(1.0) + 0.15 * (1.0 - age) }
}

/// Font size (CSS px) of a floating text with `chars` characters on layout
/// `l`. Score texts follow the screen (`clamp(18px, 3vmin, 34px)`); callouts
/// too (5 and 6 vmin), but shrink so the whole word fits the HUD panel's
/// width.
pub fn float_font(style: u8, chars: usize, l: &Layout, reach: f32) -> f32 {
    let vmin = l.viewport[0].min(l.viewport[1]);
    if style == 0 {
        return (3.0 * vmin / 100.0).clamp(18.0, 34.0);
    }
    let base = callout_base(style, vmin, l.portrait);
    let fit = l.hud[2] * 0.9 / (chars.max(1) as f32 * EM_PER_CHAR * CALLOUT_POP);
    // Portrait: also as high as the strip it sits in.
    let fit_h = if l.portrait {
        let (top, bottom) = callout_strip(l, reach);
        (bottom - top) / (1.1 * CALLOUT_POP)
    } else {
        f32::MAX
    };
    base.min(fit).min(fit_h).max(12.0)
}

/// Portrait: the strip a combo callout sits in, `(top, bottom)` in CSS px,
/// from the HUD panel's drop shadow (index.html `.hud-body`) down to the
/// highest a piece dropping in shows (`reach` px above the board,
/// [`crate::draw::entry_reach`]).
fn callout_strip(l: &Layout, reach: f32) -> (f32, f32) {
    (l.hud[1] + l.hud[3] + 8.0, l.board[1] - reach)
}

/// Where a combo callout shows (its centre, CSS px), off the board so it
/// never hides the pieces falling in: Puff's shout just under the HUD panel
/// in landscape (above Puff); in portrait, the strip between the HUD and
/// the pieces dropping in (`reach` px above the board; [`float_font`] fits
/// the word to it, and on a strip too thin even for its smallest size it
/// keeps clear of the pieces and overlaps the HUD panel's edge instead).
pub fn callout_anchor(l: &Layout, font: f32, reach: f32) -> [f32; 2] {
    let x = l.hud[0] + l.hud[2] * 0.5;
    if l.portrait {
        let half = 1.1 * font * CALLOUT_POP * 0.5;
        let (top, bottom) = callout_strip(l, reach);
        [x, ((top + bottom) * 0.5).min(bottom - half)]
    } else {
        // The middle of the slot the layout keeps free under the panel's
        // drop shadow (index.html `.hud-body`) and above Puff's reach.
        [x, l.hud[1] + l.hud[3] + 8.0 + l.callout_slot * 0.5]
    }
}

fn float_view(f: &Floater, l: &Layout, shake: [f32; 2], reach: f32) -> FloatView {
    let k = (f.age / f.life.max(0.01)).clamp(0.0, 1.0);
    let text = f.text.as_str().to_string();
    let font = float_font(f.style, text.chars().count(), l, reach);
    // Score texts rise and shake with the board; callouts sit off it and
    // only pop in and fade (rising would take them over the HUD's banner).
    let (c, rise, shake) = if f.style == 0 {
        (l.center(f.x, f.y), l.cell * 0.9 * k, shake)
    } else {
        (callout_anchor(l, font, reach), 0.0, [0.0; 2])
    };
    FloatView {
        id: f.id,
        x: c[0] + shake[0],
        y: c[1] - rise + shake[1],
        text,
        style: f.style,
        age: k,
        font,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{CALLOUTS, SHUFFLE_CALLOUT};
    use crate::layout::{CellLimits, MASCOT_REACH, layout};

    /// Every size the review card shoots plus the usual phones, tablets and
    /// desktops, landscape and portrait.
    const SIZES: [(f32, f32); 19] = [
        (1920.0, 1080.0), (2560.0, 1440.0), (1366.0, 768.0), (1280.0, 720.0), (1024.0, 600.0), (932.0, 430.0), (844.0, 390.0),
        (740.0, 360.0), (667.0, 375.0), (640.0, 360.0), (568.0, 320.0), (390.0, 844.0), (360.0, 640.0), (320.0, 568.0),
        (412.0, 915.0), (430.0, 932.0), (768.0, 1024.0), (1024.0, 1366.0), (800.0, 800.0),
    ];

    fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
        a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    }

    /// Every combo callout, at every moment of its pop, in the style the
    /// game gives it (`game.rs`: the big style from the third word up).
    fn callout_views(l: &Layout, reach: f32) -> Vec<FloatView> {
        let mut words: Vec<(&str, u8)> = CALLOUTS.iter().enumerate().map(|(k, w)| (*w, if k >= 2 { 2 } else { 1 })).collect();
        words.push((SHUFFLE_CALLOUT, 1));
        let mut out = Vec::new();
        for (text, style) in words {
            let font = float_font(style, text.chars().count(), l, reach);
            let [x, y] = callout_anchor(l, font, reach);
            for step in 0..=20 {
                out.push(FloatView { id: 0, x, y, text: text.to_string(), style, age: step as f32 / 20.0, font });
            }
        }
        out
    }

    #[test]
    fn the_shortest_callout_is_the_one_the_slot_is_sized_for() {
        let shortest = CALLOUTS.iter().chain([&SHUFFLE_CALLOUT]).map(|w| w.chars().count()).min();
        assert_eq!(shortest, Some(CALLOUT_MIN_CHARS));
    }

    /// Round 3 of the critic: Puff's sprout star was drawn over "Great!",
    /// "Shuffle!" and "Superb!" on a phone held sideways and at 720p. A
    /// callout (at the peak of its pop) never overlaps Puff's reach, never
    /// the HUD panel and its shadow, and never the board, at any size.
    #[test]
    fn combo_callouts_never_overlap_puff_the_panel_or_the_board() {
        for (vw, vh) in SIZES {
            let l = layout(vw, vh, 8, 8, CellLimits::default());
            let reach = entry_reach(l.cell, &Style::default());
            let [cx, gy, m] = l.mascot;
            // Puff with his arms up and the sprout star at the top of his
            // highest hop: 1.5 of his size wide, `MASCOT_REACH` high.
            let puff = [cx - m * 0.75, gy - m * MASCOT_REACH, m * 1.5, m * MASCOT_REACH];
            let panel = [l.hud[0], l.hud[1], l.hud[2], l.hud[3] + 8.0];
            for v in callout_views(&l, reach) {
                let r = v.rect();
                assert!(!overlap(r, puff), "{vw}x{vh}: {:?} at age {} {r:?} covers Puff {puff:?}", v.text, v.age);
                assert!(!overlap(r, l.board), "{vw}x{vh}: {:?} {r:?} covers the board {:?}", v.text, l.board);
                if !l.portrait {
                    assert!(!overlap(r, panel), "{vw}x{vh}: {:?} {r:?} covers the panel {panel:?}", v.text);
                    // Inside its slot, and inside the panel's width.
                    assert!(r[1] + r[3] <= gy - m * MASCOT_REACH + 0.01, "{vw}x{vh}: {:?} {r:?} below the slot", v.text);
                    assert!(r[0] >= l.hud[0] - 0.01 && r[0] + r[2] <= l.hud[0] + l.hud[2] + 0.01, "{vw}x{vh}: {:?} {r:?} wider than the panel", v.text);
                }
            }
        }
    }
}
