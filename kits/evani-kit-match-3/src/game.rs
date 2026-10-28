//! The game: phases (title with an attract board, level intro, play, Sugar
//! Rush, won, lost), the swap → clear → fall → cascade loop, scoring, goals,
//! hints, shuffles, input (pointer, keys, pad) and the visual-only juice
//! (pops, blasts, sparkles, floating text, trauma, Puff's mood).
//!
//! The game's objects are ECS entities and pools ([`crate::ecs`]); the
//! [`Game`] resource holds the session: level, phase, moves, score, goals,
//! selection, the seeded RNG and the requests from the UI. [`tick`] runs one
//! fixed step on a world that holds them ([`init`] sets one up).
//!
//! Everything advances on the fixed tick with seeded RNGs, so a level plays
//! the same for the same moves. The sparkles have their own RNG stream, so
//! switching effects on or off never changes the game. In free roam
//! ([`evani_core::play::PlayMode::FreeRoam`]) moves cost nothing and there
//! are no goals, Sugar Rush, win or loss: the board keeps refilling.

use core::cell::{Ref, RefMut};

use evani_core::ecs::World;
use evani_core::input::PrimaryPointer;
use evani_core::particles::ParticleQueue;
use evani_core::play::PlayOutcomes;
use evani_core::rng::Pcg32;

use crate::Tune;
use crate::board::{Board, Cleared, Kind, MAX_COLORS, Move, Piece, Resolution, Rules};
use crate::ecs::{Blast, Cell, FloatText, Floats, FxStream, Grid, Motion, Popping, Puff};
use crate::layout::Layout;
use crate::level::{Goal, Level};
use crate::sparks::Sparks;

pub use crate::ecs::Mood;

/// Where the game is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Title screen: the attract board plays itself.
    #[default]
    Title,
    /// Level intro card over the new board.
    Intro,
    Play,
    /// Won: left-over moves fire as Streaks.
    Rush,
    Won,
    Lost,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Title => "title",
            Phase::Intro => "intro",
            Phase::Play => "play",
            Phase::Rush => "rush",
            Phase::Won => "won",
            Phase::Lost => "lost",
        }
    }
}

/// What the board is doing inside a phase.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Step {
    /// Waiting for a move.
    #[default]
    Idle,
    /// Two pieces trading places (already swapped in the board).
    Swap { a: usize, b: usize, t: f32 },
    /// A swap that makes no match: slide and bounce back.
    Invalid { a: usize, b: usize, t: f32 },
    /// Pieces popping; gravity runs at `until`.
    Clear { t: f32, until: f32 },
    /// Pieces falling into place.
    Fall,
    /// Pieces flying to shuffled cells.
    Shuffle { t: f32 },
}

/// What the UI (or a bot) asks for; handled at the start of the next tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Start a level (0-based).
    Start(usize),
    Restart,
    Next,
    Title,
    Pause(bool),
    TogglePause,
    /// Swap the pieces in cells `a` and `b` (a move, when the board is idle).
    Swap(usize, usize),
}

/// One tick's player input.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerInput {
    pub pointer: PrimaryPointer,
    /// Arrow / d-pad presses this tick: up, down, left, right.
    pub dirs: [bool; 4],
    pub pick: bool,
    pub cancel: bool,
}

/// Effect switches and settings the sim needs (from the effects module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxFlags {
    pub juice: bool,
    pub particles: bool,
    pub particle_density: f32,
    /// Cells per second squared.
    pub particle_gravity: f32,
    pub shake_decay: f32,
    /// The six piece colours the sparkles take (linear; from the look).
    pub colors: [[f32; 4]; 6],
}

impl Default for FxFlags {
    fn default() -> Self {
        Self {
            juice: true,
            particles: true,
            particle_density: 1.0,
            particle_gravity: 14.0,
            shake_decay: 1.6,
            colors: crate::draw::DEFAULT_PIECES,
        }
    }
}

/// Game sounds and moments (drained every frame by the draw system).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Select,
    Swap,
    Invalid,
    /// A match popped, at this cascade depth.
    Pop(u32),
    Streak,
    Bomb,
    Prism,
    /// A special piece was made.
    Special,
    Land,
    Frost,
    Shuffle,
    /// A star threshold was passed (1..=3).
    Star(u8),
    GoalDone,
    Win,
    Lose,
    LevelStart,
}

/// Goal progress for the HUD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalState {
    pub goal: Goal,
    pub have: u32,
    pub need: u32,
}

impl GoalState {
    pub fn done(&self) -> bool {
        self.have >= self.need
    }
}

/// Callout words per cascade depth.
pub const CALLOUTS: [&str; 5] = ["Nice!", "Great!", "Superb!", "Fantastic!", "Unstoppable!"];
/// The callout when a board with no move left reshuffles.
pub const SHUFFLE_CALLOUT: &str = "Shuffle!";

/// Formulas the game evaluates. The world is the one the game runs in (the
/// engine's formula registry lives there).
pub struct Fns<'a> {
    /// `match_score(world, pieces, cascade, special)`.
    pub match_score: &'a dyn Fn(&World, f32, f32, f32) -> f32,
}

pub fn default_match_score(pieces: f32, cascade: f32, special: f32) -> f32 {
    pieces * 60.0 * (1.0 + cascade * 0.5) + special * 120.0
}

/// [`default_match_score`] as a [`Fns::match_score`].
pub fn default_match_score_in(_: &World, pieces: f32, cascade: f32, special: f32) -> f32 {
    default_match_score(pieces, cascade, special)
}

/// The session: level, phase, moves, score, goals, selection and the
/// seeded RNG. The board's pieces, pops, blasts, floating text and Puff
/// are entities and pools ([`crate::ecs`]); sparkles go to the shared GPU
/// particle system ([`crate::sparks`]).
#[derive(Debug, Clone, Default)]
pub struct Game {
    pub levels: Vec<Level>,
    pub level: usize,
    pub rules: Rules,
    /// Free roam: no move cost, goals, Sugar Rush, win or loss.
    pub free_roam: bool,
    pub phase: Phase,
    pub step: Step,
    pub paused: bool,
    /// Seconds in this phase (sim time).
    pub phase_t: f32,
    pub moves_left: i32,
    pub moves_used: i32,
    pub score: u32,
    pub stars: u8,
    pub collected: [u32; MAX_COLORS as usize],
    pub cascade: u32,
    pub best_cascade: u32,
    pub selected: Option<usize>,
    pub cursor: usize,
    pub cursor_visible: bool,
    drag_from: Option<usize>,
    pub idle_t: f32,
    pub hint: Option<Move>,
    pub rush_left: i32,
    rush_t: f32,
    attract_t: f32,
    rng: Pcg32,
    pub events: Vec<Event>,
    /// Screen-shake trauma 0..1.
    pub trauma: f32,
    /// Cells that moved last (special placement in cascades).
    prefer: Vec<usize>,
    pending: Vec<Request>,
    /// Layout of the last frame (input mapping).
    pub layout: Layout,
    /// Times a level was started (tests, UI).
    pub runs: u32,
    pub last_fx: FxFlags,
    /// Scratch: pops and blasts that start this tick.
    pop_starts: Vec<(f32, f32, u8)>,
    blast_starts: Vec<Blast>,
}

/// The attract board's seed offset from level 1's.
const ATTRACT_SEED: u64 = 7_771;

impl Game {
    pub fn new(levels: Vec<Level>, rules: Rules) -> Self {
        Self {
            levels,
            rules,
            events: Vec::with_capacity(32),
            pop_starts: Vec::with_capacity(64),
            blast_starts: Vec::with_capacity(16),
            ..Self::default()
        }
    }

    pub fn request(&mut self, r: Request) {
        self.pending.push(r);
    }

    pub fn current(&self) -> &Level {
        &self.levels[self.level.min(self.levels.len() - 1)]
    }

    /// Goal progress on `board` (none in free roam).
    pub fn goals(&self, board: &Board) -> Vec<GoalState> {
        if self.free_roam {
            return Vec::new();
        }
        self.current()
            .goals
            .iter()
            .map(|&goal| {
                let (have, need) = match goal {
                    Goal::Collect { color, count } => (self.collected[color as usize].min(count), count),
                    Goal::Frosting => {
                        let total: u32 = self.current().shape.frost.iter().map(|&f| u32::from(f)).sum();
                        (total - board.frost_left().min(total), total)
                    }
                    Goal::Score(s) => (self.score.min(s), s),
                };
                GoalState { goal, have, need }
            })
            .collect()
    }

    pub fn goals_met(&self, board: &Board) -> bool {
        self.goals(board).iter().all(GoalState::done)
    }

    fn stars_for(&self, score: u32) -> u8 {
        self.current().stars.iter().filter(|&&s| score >= s).count() as u8
    }
}

/// Set up the game in `world`: the components, the [`Game`], [`Grid`],
/// [`FxStream`] and [`Floats`] resources, Puff, and the title's attract
/// board. The particle queue is added when the world has none (a world of
/// its own, in tests and bots; an app always has one).
pub fn init(world: &mut World, levels: Vec<Level>, rules: Rules, free_roam: bool) {
    crate::ecs::register(world);
    let mut g = Game::new(levels, rules);
    g.free_roam = free_roam;
    world.insert_resource(g);
    let puff = world.spawn_with(Puff::default());
    world.insert_resource(Grid::new(puff));
    world.insert_resource(FxStream::default());
    world.insert_resource(Floats::new());
    if !world.has_resource::<ParticleQueue>() {
        world.insert_resource(ParticleQueue::default());
    }
    with_ctx(world, |c| c.reset(Phase::Title, 0));
}

/// One fixed tick of the game in `world` (set up by [`init`]).
pub fn tick(world: &mut World, input: &PlayerInput, t: &Tune, fns: &Fns, fx: &FxFlags, dt: f32) {
    with_ctx(world, |c| c.tick(input, t, fns, fx, dt));
}

/// Run `f` with the game's resources taken out of `world` (no allocation),
/// then put them back.
fn with_ctx(world: &mut World, f: impl FnOnce(&mut Ctx)) {
    let ready = world.has_resource::<Game>()
        && world.has_resource::<Grid>()
        && world.has_resource::<FxStream>()
        && world.has_resource::<Floats>();
    if !ready {
        return;
    }
    let mut c = Ctx {
        g: world.res_get_mut::<Game>().map(core::mem::take).unwrap_or_default(),
        grid: world.res_get_mut::<Grid>().map(core::mem::take).unwrap_or_default(),
        fxs: world.res_get_mut::<FxStream>().map(core::mem::take).unwrap_or_default(),
        floats: world.res_get_mut::<Floats>().map(core::mem::take).unwrap_or_default(),
        w: world,
    };
    c.grid.gather(c.w);
    f(&mut c);
    let Ctx { g, grid, fxs, floats, w } = c;
    if let Some(r) = w.res_get_mut::<Game>() {
        *r = g;
    }
    if let Some(r) = w.res_get_mut::<Grid>() {
        *r = grid;
    }
    if let Some(r) = w.res_get_mut::<FxStream>() {
        *r = fxs;
    }
    if let Some(r) = w.res_get_mut::<Floats>() {
        *r = floats;
    }
}

/// A game in a world of its own (tests, bots, the capture replay).
pub struct SimWorld {
    pub world: World,
}

impl core::fmt::Debug for SimWorld {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimWorld").field("entities", &self.world.entity_count()).finish()
    }
}

impl SimWorld {
    pub fn new(levels: Vec<Level>, rules: Rules) -> Self {
        let mut world = World::new();
        init(&mut world, levels, rules, false);
        Self { world }
    }

    pub fn tick(&mut self, input: &PlayerInput, t: &Tune, fns: &Fns, fx: &FxFlags, dt: f32) {
        tick(&mut self.world, input, t, fns, fx, dt);
    }

    pub fn game(&self) -> Ref<'_, Game> {
        self.world.res::<Game>()
    }

    pub fn game_mut(&self) -> RefMut<'_, Game> {
        self.world.res_mut::<Game>()
    }

    pub fn grid(&self) -> Ref<'_, Grid> {
        self.world.res::<Grid>()
    }

    pub fn request(&self, r: Request) {
        self.game_mut().request(r);
    }
}

/// One tick's working set: the resources taken out of the world, and the
/// world for the entities.
struct Ctx<'w> {
    g: Game,
    grid: Grid,
    /// The sparkles' random stream.
    fxs: FxStream,
    floats: Floats,
    w: &'w mut World,
}

impl Ctx<'_> {
    /// Emit sparkles with `f`: into the world's particle queue (or a
    /// throwaway one in a world without it, so the stream still advances
    /// the same). Nothing is emitted under the title, whose attract board
    /// stays calm so the logo reads.
    fn sparkle(&mut self, fx: &FxFlags, f: impl FnOnce(&mut Sparks)) {
        let mut spare = ParticleQueue::default();
        let mut guard = self.w.try_res_mut::<ParticleQueue>();
        let queue = match guard.as_deref_mut() {
            Some(q) => q,
            None => &mut spare,
        };
        let mut s = Sparks {
            queue,
            rng: &mut self.fxs.0,
            layout: self.g.layout,
            dims: [self.grid.board.w, self.grid.board.h],
            colors: fx.colors,
            gravity: fx.particle_gravity,
        };
        f(&mut s);
    }

    fn mood(&self) -> Mood {
        self.grid.puff(self.w).mood
    }

    fn set_mood(&mut self, m: Mood) {
        if let Some(mut p) = self.grid.puff.and_then(|e| self.w.get_mut::<Puff>(e)) {
            p.mood = m;
            p.mood_t = 0.0;
        }
    }

    fn game_over(&mut self) {
        if let Some(mut o) = self.w.try_res_mut::<PlayOutcomes>() {
            o.game_over();
        }
    }

    /// Set up a fresh board for `level` in `phase`.
    fn reset(&mut self, phase: Phase, level: usize) {
        let g = &mut self.g;
        g.level = level.min(g.levels.len() - 1);
        let lv = g.current().clone();
        let seed = if phase == Phase::Title { lv.seed + ATTRACT_SEED } else { lv.seed };
        g.rng = Pcg32::new(seed, 1);
        self.fxs.0 = Pcg32::new(seed, 99);
        self.grid.clear(self.w);
        self.grid.board = Board::empty(&lv.shape, lv.colors);
        self.grid.board.generate(&mut g.rng, &g.rules);
        self.grid.sync(self.w);
        // Drop the whole board in from above for the intro.
        for i in 0..self.grid.board.len() {
            let (_, y) = self.grid.board.xy(i);
            if let Some(mut m) = self.grid.motion(self.w, i) {
                m.off[1] = -(lv.shape.h as f32) - 1.0 + y as f32 * 0.15;
                m.vy = 0.0;
            }
        }
        let g = &mut self.g;
        g.phase = phase;
        g.step = Step::Fall;
        g.paused = false;
        g.phase_t = 0.0;
        g.moves_left = if phase == Phase::Title { 999 } else { lv.moves };
        g.moves_used = 0;
        g.score = 0;
        g.stars = 0;
        g.collected = [0; MAX_COLORS as usize];
        g.cascade = 0;
        g.best_cascade = 0;
        g.selected = None;
        g.cursor = (0..self.grid.board.len()).find(|&i| self.grid.board.mask[i]).unwrap_or(0);
        g.cursor_visible = false;
        g.drag_from = None;
        g.idle_t = 0.0;
        g.hint = None;
        g.rush_left = 0;
        g.rush_t = 0.0;
        g.attract_t = 0.0;
        // Whatever sparkled before this board is gone with it.
        if let Some(mut q) = self.w.try_res_mut::<ParticleQueue>() {
            q.clear();
        }
        self.floats.items.clear();
        g.trauma = 0.0;
        g.prefer.clear();
        if phase != Phase::Title {
            g.runs += 1;
            g.events.push(Event::LevelStart);
        }
        self.set_mood(Mood::Idle);
    }

    fn tick(&mut self, input: &PlayerInput, t: &Tune, fns: &Fns, fx: &FxFlags, dt: f32) {
        self.g.last_fx = *fx;
        let pending = core::mem::take(&mut self.g.pending);
        for r in pending {
            match r {
                Request::Start(l) => self.reset(Phase::Intro, l),
                Request::Restart => {
                    if self.g.phase != Phase::Title {
                        self.reset(Phase::Intro, self.g.level);
                    }
                }
                Request::Next => {
                    let next = (self.g.level + 1) % self.g.levels.len();
                    self.reset(Phase::Intro, next);
                }
                Request::Title => self.reset(Phase::Title, 0),
                Request::Pause(p) => self.g.paused = p && self.g.phase != Phase::Title,
                Request::TogglePause => self.g.paused = !self.g.paused && self.g.phase != Phase::Title,
                Request::Swap(a, b) => self.try_swap(a, b),
            }
        }
        if self.g.paused {
            return;
        }
        self.g.phase_t += dt;
        if let Some(mut p) = self.grid.puff.and_then(|e| self.w.get_mut::<Puff>(e)) {
            p.mood_t += dt;
        }
        self.g.trauma = (self.g.trauma - fx.shake_decay * dt).max(0.0);
        self.animate(t, fx, dt);

        match self.g.phase {
            Phase::Intro => {
                if self.g.phase_t >= t.intro_s && self.g.step == Step::Idle {
                    self.g.phase = Phase::Play;
                    self.g.phase_t = 0.0;
                }
            }
            Phase::Play => {
                if self.g.step == Step::Idle {
                    self.handle_input(input, t);
                    self.g.idle_t += dt;
                    if self.g.idle_t >= t.hint_delay_s && self.g.hint.is_none() {
                        self.g.hint = self.grid.board.best_move();
                    }
                    if !self.g.free_roam
                        && self.g.moves_left <= 2
                        && self.mood() == Mood::Idle
                        && !self.g.goals_met(&self.grid.board)
                    {
                        self.set_mood(Mood::Worried);
                    }
                } else {
                    // Pick a piece while the board settles; swap when idle.
                    self.handle_selection_only(input);
                }
            }
            Phase::Title => {
                if self.g.step == Step::Idle {
                    self.g.attract_t += dt;
                    if self.g.attract_t >= t.attract_move_s {
                        self.g.attract_t = 0.0;
                        if let Some(m) = self.grid.board.best_move() {
                            self.try_swap(m.a, m.b);
                        }
                    }
                }
            }
            Phase::Rush => {
                if self.g.step == Step::Idle {
                    self.g.rush_t += dt;
                    if self.g.rush_t >= t.rush_step_s {
                        self.g.rush_t = 0.0;
                        if self.g.rush_left > 0 {
                            self.g.rush_left -= 1;
                            self.g.moves_left = self.g.rush_left;
                            self.rush_fire(t, fns);
                        } else {
                            self.finish_win();
                        }
                    }
                }
            }
            Phase::Won | Phase::Lost => {}
        }
        self.advance_step(t, fns, fx, dt);
    }

    fn finish_win(&mut self) {
        self.g.phase = Phase::Won;
        self.g.phase_t = 0.0;
        self.g.stars = self.g.stars_for(self.g.score).max(1);
        self.g.events.push(Event::Win);
        self.set_mood(Mood::Cheer);
        self.g.trauma = (self.g.trauma + 0.3).min(1.0);
        self.game_over();
        let fx = self.g.last_fx;
        if fx.particles {
            // Confetti over the board.
            let n = (90.0 * fx.particle_density) as usize;
            self.sparkle(&fx, |s| s.confetti(n));
        }
    }

    fn rush_fire(&mut self, t: &Tune, fns: &Fns) {
        let board = &mut self.grid.board;
        let normals: Vec<usize> = (0..board.len())
            .filter(|&i| board.piece(i).is_some_and(|p| p.kind == Kind::Normal))
            .collect();
        if normals.is_empty() {
            return;
        }
        let cell = normals[self.g.rng.below(normals.len() as u32) as usize];
        let kind = if self.g.rng.chance(0.5) { Kind::StreakH } else { Kind::StreakV };
        if let Some(p) = board.cells[cell].as_mut() {
            p.kind = kind;
        }
        self.g.cascade = 0;
        let res = board.fire(cell, &self.g.rules);
        self.grid.sync(self.w);
        self.apply(res, t, fns);
    }

    fn handle_selection_only(&mut self, input: &PlayerInput) {
        let p = input.pointer;
        if p.pressed
            && let Some(c) = self.cell_at(p.press_pos)
        {
            self.g.selected = Some(c);
            self.g.drag_from = None;
            self.g.cursor_visible = false;
        }
    }

    fn cell_at(&self, pos: [f32; 2]) -> Option<usize> {
        let (x, y) = self.g.layout.hit(pos)?;
        let i = self.grid.board.at(x, y)?;
        self.grid.board.piece(i).is_some().then_some(i)
    }

    fn neighbour(&self, c: usize, dir: usize) -> Option<usize> {
        let (x, y) = self.grid.board.xy(c);
        let (dx, dy) = [(0, -1), (0, 1), (-1, 0), (1, 0)][dir.min(3)];
        self.grid.board.at(x + dx, y + dy)
    }

    fn handle_input(&mut self, input: &PlayerInput, t: &Tune) {
        let p = input.pointer;
        // Pointer: tap-tap or press-and-drag.
        if p.pressed {
            self.g.cursor_visible = false;
            match self.cell_at(p.press_pos) {
                Some(c) => {
                    if let Some(s) = self.g.selected
                        && s != c
                        && self.grid.board.adjacent(s, c)
                    {
                        self.g.selected = None;
                        self.g.drag_from = None;
                        self.try_swap(s, c);
                        return;
                    }
                    if self.g.selected == Some(c) && !p.down {
                        // Tapping the picked piece again drops it.
                        self.g.selected = None;
                    } else {
                        if self.g.selected != Some(c) {
                            self.g.events.push(Event::Select);
                        }
                        self.g.selected = Some(c);
                    }
                    self.g.drag_from = Some(c);
                }
                None => {
                    self.g.selected = None;
                    self.g.drag_from = None;
                }
            }
        }
        if let Some(from) = self.g.drag_from {
            if p.down || p.released || p.pressed {
                let (fx, fy) = self.grid.board.xy(from);
                let c = self.g.layout.center(fx as f32, fy as f32);
                let d = [p.pos[0] - c[0], p.pos[1] - c[1]];
                let reach = t.swipe_threshold * self.g.layout.cell;
                // Measure from where the press started on the piece.
                let start = [p.press_pos[0] - c[0], p.press_pos[1] - c[1]];
                let m = [d[0] - start[0], d[1] - start[1]];
                if m[0].abs().max(m[1].abs()) >= reach {
                    let dir = if m[0].abs() > m[1].abs() {
                        if m[0] > 0.0 { 3 } else { 2 }
                    } else if m[1] > 0.0 {
                        1
                    } else {
                        0
                    };
                    self.g.drag_from = None;
                    if let Some(to) = self.neighbour(from, dir)
                        && self.grid.board.piece(to).is_some()
                    {
                        self.g.selected = None;
                        self.try_swap(from, to);
                        return;
                    }
                }
            }
            if !p.down {
                self.g.drag_from = None;
            }
        }
        // Keys and pad.
        if let Some(dir) = input.dirs.iter().position(|&d| d) {
            self.g.cursor_visible = true;
            if let Some(s) = self.g.selected {
                self.g.selected = None;
                if let Some(to) = self.neighbour(s, dir) {
                    self.g.cursor = to;
                    self.try_swap(s, to);
                    return;
                }
            } else if let Some(to) = self.neighbour(self.g.cursor, dir) {
                self.g.cursor = to;
            } else {
                // Skip over holes in that direction.
                let (mut x, mut y) = self.grid.board.xy(self.g.cursor);
                let (dx, dy) = [(0, -1), (0, 1), (-1, 0), (1, 0)][dir];
                for _ in 0..12 {
                    x += dx;
                    y += dy;
                    if let Some(c) = self.grid.board.at(x, y) {
                        self.g.cursor = c;
                        break;
                    }
                }
            }
        }
        if input.pick {
            self.g.cursor_visible = true;
            if self.g.selected == Some(self.g.cursor) {
                self.g.selected = None;
            } else if self.grid.board.piece(self.g.cursor).is_some() {
                self.g.selected = Some(self.g.cursor);
                self.g.events.push(Event::Select);
            }
        }
        if input.cancel {
            self.g.selected = None;
        }
    }

    /// Start a swap of `a` onto `b` (valid or the bounce-back).
    fn try_swap(&mut self, a: usize, b: usize) {
        let n = self.grid.board.len();
        if self.g.step != Step::Idle || a >= n || b >= n || !self.grid.board.adjacent(a, b) {
            return;
        }
        self.g.idle_t = 0.0;
        self.g.hint = None;
        self.g.selected = None;
        if !self.grid.board.valid_swap(a, b) {
            self.g.step = Step::Invalid { a, b, t: 0.0 };
            self.g.events.push(Event::Invalid);
            return;
        }
        self.grid.board.swap(a, b);
        self.grid.sync(self.w);
        let (ax, ay) = self.grid.board.xy(a);
        let (bx, by) = self.grid.board.xy(b);
        let d = [(bx - ax) as f32, (by - ay) as f32];
        if let Some(mut m) = self.grid.motion(self.w, a) {
            m.swap_from = d;
            m.swap_t = 0.0;
        }
        if let Some(mut m) = self.grid.motion(self.w, b) {
            m.swap_from = [-d[0], -d[1]];
            m.swap_t = 0.0;
        }
        if self.g.phase != Phase::Title {
            if !self.g.free_roam {
                self.g.moves_left -= 1;
            }
            self.g.moves_used += 1;
        }
        self.g.cascade = 0;
        self.g.step = Step::Swap { a, b, t: 0.0 };
        self.g.events.push(Event::Swap);
    }

    fn advance_step(&mut self, t: &Tune, fns: &Fns, fx: &FxFlags, dt: f32) {
        match self.g.step {
            Step::Idle => {}
            Step::Invalid { a, b, t: s } => {
                let s = s + dt;
                self.g.step = if s >= t.swap_s * 2.0 { Step::Idle } else { Step::Invalid { a, b, t: s } };
            }
            Step::Swap { a, b, t: s } => {
                let s = s + dt;
                if s < t.swap_s {
                    self.g.step = Step::Swap { a, b, t: s };
                    return;
                }
                // The piece the player moved now sits at `b`.
                let rules = self.g.rules;
                let board = &mut self.grid.board;
                let res = match board.resolve_swap_combo(a, b, &rules) {
                    Some(r) => r,
                    None => board.resolve_matches(&[b, a], &rules),
                };
                self.grid.sync(self.w);
                self.apply(res, t, fns);
            }
            Step::Clear { t: s, until } => {
                let s = s + dt;
                if s < until {
                    self.g.step = Step::Clear { t: s, until };
                    return;
                }
                let falls = self.grid.board.gravity(&mut self.g.rng);
                self.grid.sync(self.w);
                self.g.prefer.clear();
                for f in &falls {
                    self.g.prefer.push(f.cell);
                    if let Some(mut m) = self.grid.motion(self.w, f.cell) {
                        m.off[1] = -f.rows;
                        m.vy = if f.new { 2.0 } else { 0.0 };
                        m.swap_t = -1.0;
                    }
                }
                self.g.step = Step::Fall;
            }
            Step::Fall => {
                let mut falling = false;
                self.w.query::<&Motion>().for_each(|_, m| falling |= m.off[1] < 0.0);
                if falling {
                    return;
                }
                if !self.grid.board.groups().is_empty() {
                    self.g.cascade += 1;
                    self.g.best_cascade = self.g.best_cascade.max(self.g.cascade);
                    let prefer = core::mem::take(&mut self.g.prefer);
                    let res = self.grid.board.resolve_matches(&prefer, &self.g.rules);
                    self.g.prefer = prefer;
                    self.g.prefer.clear();
                    self.grid.sync(self.w);
                    self.apply(res, t, fns);
                    self.g.trauma = (self.g.trauma + t.trauma_cascade).min(1.0);
                    if fx.juice {
                        let k = (self.g.cascade as usize - 1).min(CALLOUTS.len() - 1);
                        let (cx, cy) = (self.grid.board.w as f32 * 0.5 - 0.5, self.grid.board.h as f32 * 0.45);
                        self.floats
                            .add(cx, cy, FloatText::new(CALLOUTS[k]), if k >= 2 { 2 } else { 1 }, t.callout_s);
                    }
                    if self.g.cascade >= 2 {
                        self.set_mood(Mood::Excited);
                    }
                    return;
                }
                self.settle(t);
            }
            Step::Shuffle { t: s } => {
                let s = s + dt;
                self.g.step = if s >= t.shuffle_s { Step::Fall } else { Step::Shuffle { t: s } };
            }
        }
    }

    /// The board is still: decide what happens next.
    fn settle(&mut self, t: &Tune) {
        self.g.step = Step::Idle;
        self.g.idle_t = 0.0;
        self.g.prefer.clear();
        match self.g.phase {
            Phase::Play if !self.g.free_roam => {
                if self.g.goals_met(&self.grid.board) {
                    self.g.phase = Phase::Rush;
                    self.g.phase_t = 0.0;
                    self.g.rush_left = self.g.moves_left.max(0);
                    self.g.rush_t = 0.0;
                    self.g.selected = None;
                    self.g.events.push(Event::GoalDone);
                    self.set_mood(Mood::Cheer);
                    return;
                }
                if self.g.moves_left <= 0 {
                    self.g.phase = Phase::Lost;
                    self.g.phase_t = 0.0;
                    self.g.stars = 0;
                    self.g.selected = None;
                    self.g.events.push(Event::Lose);
                    self.set_mood(Mood::Sad);
                    self.game_over();
                    return;
                }
            }
            Phase::Play | Phase::Rush | Phase::Title | Phase::Intro => {}
            Phase::Won | Phase::Lost => return,
        }
        if !self.grid.board.has_move() {
            let moved = self.grid.board.shuffle(&mut self.g.rng);
            self.grid.sync(self.w);
            // Only the fallback recolouring leaves every piece in place
            // (the old arrangement had no move): it makes the pieces anew.
            if moved.iter().all(|&(to, from)| to == from) {
                self.w.query::<&mut Motion>().for_each(|_, m| *m = Motion::new());
            }
            for (to, from) in moved {
                let (tx, ty) = self.grid.board.xy(to);
                let (fx, fy) = self.grid.board.xy(from);
                if let Some(mut m) = self.grid.motion(self.w, to) {
                    m.swap_from = [(fx - tx) as f32, (fy - ty) as f32];
                    m.swap_t = 0.0;
                }
            }
            self.g.events.push(Event::Shuffle);
            self.g.step = Step::Shuffle { t: 0.0 };
            // Say so, in the callout slot off the board (hud::callout_anchor),
            // with or without juice: it explains why every piece moved.
            let (cx, cy) = (self.grid.board.w as f32 * 0.5 - 0.5, self.grid.board.h as f32 * 0.45);
            self.floats.add(cx, cy, FloatText::new(SHUFFLE_CALLOUT), 1, t.callout_s);
        }
    }

    /// A cleared piece starts popping: its entity leaves the board.
    fn pop(&mut self, c: &Cleared, t: f32) {
        let (x, y) = self.grid.board.xy(c.cell);
        let e = match self.grid.take_gone(c.piece.id) {
            Some(e) => {
                self.w.remove::<Cell>(e);
                self.w.remove::<Motion>(e);
                e
            }
            None => self.w.spawn(),
        };
        self.w.insert(e, c.piece);
        self.w.insert(
            e,
            Popping {
                x: x as f32,
                y: y as f32,
                t,
            },
        );
        self.grid.pops.push(e);
    }

    /// Score, goals, visuals and sounds for one resolution step.
    fn apply(&mut self, res: Resolution, t: &Tune, fns: &Fns) {
        let fx = self.g.last_fx;
        let factor = 1.0 + self.g.cascade as f32 * 0.5;
        // How long the pops take: the pieces above fall in right after.
        let until = res.max_delay() * t.chain_delay_s + t.clear_s;
        let mut gained = 0.0f32;
        // Score texts (cell x, cell y, points), one per spot.
        let mut texts: Vec<(f32, f32, f32)> = Vec::new();
        for g in &res.groups {
            let special = if g.special().is_some() { 1.0 } else { 0.0 };
            let s = (fns.match_score)(self.w, g.cells.len() as f32, self.g.cascade as f32, special).max(0.0);
            gained += s;
            if fx.juice {
                let (mut cx, mut cy) = (0.0, 0.0);
                for &c in &g.cells {
                    let (x, y) = self.grid.board.xy(c);
                    cx += x as f32;
                    cy += y as f32;
                }
                let n = g.cells.len().max(1) as f32;
                add_score_text(&mut texts, cx / n, cy / n, s);
            }
        }
        let blasted = res.cleared.iter().filter(|c| c.blasted).count() as f32;
        if blasted > 0.0 {
            let s = blasted * t.blast_score * factor;
            gained += s;
            if fx.juice
                && res.groups.is_empty()
                && let Some(f) = res.fired.first()
            {
                let (x, y) = self.grid.board.xy(f.cell);
                add_score_text(&mut texts, x as f32, y as f32, s);
            }
        }
        // A score text ends a little before the pops do (9/10 of their time;
        // the page fades a text over its last 30 %), so it is gone, not just
        // faint, when the pieces above fall through its spot.
        let life = t.float_s.min(until * 0.9);
        for (x, y, s) in texts {
            self.floats.add(x, y, FloatText::plus(s.round() as u32), 0, life);
        }
        let before = self.g.stars_for(self.g.score);
        self.g.score = self.g.score.saturating_add(gained.round() as u32);
        let after = self.g.stars_for(self.g.score);
        if after > before && self.g.phase != Phase::Title {
            self.g.events.push(Event::Star(after));
        }
        let goals_before: Vec<bool> = self.g.goals(&self.grid.board).iter().map(GoalState::done).collect();
        for c in &res.cleared {
            if let Some(n) = self.g.collected.get_mut(c.piece.color as usize) {
                *n += 1;
            }
            self.pop(c, -c.delay * t.chain_delay_s);
        }
        if self.g.phase == Phase::Play {
            let goals_after: Vec<bool> = self.g.goals(&self.grid.board).iter().map(GoalState::done).collect();
            if goals_after.iter().zip(&goals_before).any(|(a, b)| *a && !*b) {
                self.g.events.push(Event::GoalDone);
            }
        }
        for f in &res.fired {
            let (x, y) = self.grid.board.xy(f.cell);
            let e = self.w.spawn_with(Blast {
                x: x as f32,
                y: y as f32,
                kind: f.kind,
                combo: f.combo,
                color: f.color,
                t: -f.delay * t.chain_delay_s,
            });
            self.grid.blasts.push(e);
            let (ev, tr) = match f.kind {
                Kind::StreakH | Kind::StreakV => (Event::Streak, t.trauma_streak),
                Kind::Bomb => (Event::Bomb, t.trauma_bomb),
                _ => (Event::Prism, t.trauma_prism),
            };
            self.g.events.push(ev);
            let tr = if f.combo.is_some() { tr * 1.5 } else { tr };
            self.g.trauma = (self.g.trauma + tr).min(1.0);
        }
        for c in &res.created {
            // A new special pops in.
            if let Some(mut m) = self.grid.motion(self.w, c.cell) {
                m.born = 0.0;
            }
        }
        if !res.created.is_empty() {
            self.g.events.push(Event::Special);
            if self.g.phase != Phase::Title {
                self.set_mood(Mood::Excited);
            }
        }
        if res.frost_melted > 0 {
            self.g.events.push(Event::Frost);
        }
        if !res.cleared.is_empty() {
            self.g.events.push(Event::Pop(self.g.cascade));
            if matches!(self.mood(), Mood::Idle | Mood::Worried) {
                self.set_mood(Mood::Happy);
            }
        }
        self.g.step = if res.is_empty() { Step::Fall } else { Step::Clear { t: 0.0, until } };
    }

    /// Advance every animation (pieces, pops, blasts, sparkles, text, Puff).
    fn animate(&mut self, t: &Tune, fx: &FxFlags, dt: f32) {
        let mut landed = false;
        let dur = if matches!(self.g.step, Step::Shuffle { .. }) { t.shuffle_s } else { t.swap_s };
        self.w.query::<&mut Motion>().for_each(|_, a| {
            if a.swap_t >= 0.0 {
                a.swap_t += dt;
                if a.swap_t >= dur {
                    a.swap_t = -1.0;
                }
            }
            if a.off[1] < 0.0 {
                a.vy = (a.vy + t.fall_gravity * dt).min(t.fall_speed);
                a.off[1] += a.vy * dt;
                if a.off[1] >= 0.0 {
                    a.off[1] = 0.0;
                    a.vy = 0.0;
                    a.land = t.land_squash_s;
                    landed = true;
                }
            } else if a.land > 0.0 {
                a.land = (a.land - dt).max(0.0);
            }
            if a.born >= 0.0 {
                a.born += dt;
                if a.born > 0.5 {
                    a.born = -1.0;
                }
            }
        });
        if landed {
            self.g.events.push(Event::Land);
        }
        // Pops, oldest first: sparkles start when a delayed pop begins.
        let mut starts = core::mem::take(&mut self.g.pop_starts);
        starts.clear();
        for &e in &self.grid.pops {
            if let (Some(mut p), Some(pc)) = (self.w.get_mut::<Popping>(e), self.w.get::<Piece>(e)) {
                let was = p.t;
                p.t += dt;
                if was <= 0.0 && p.t > 0.0 {
                    starts.push((p.x, p.y, pc.color));
                }
            }
        }
        let clear_s = t.clear_s;
        let w = &mut *self.w;
        self.grid.pops.retain(|&e| {
            let keep = w.get::<Popping>(e).is_some_and(|p| p.t < clear_s);
            if !keep {
                w.despawn(e);
            }
            keep
        });
        let mut blast_starts = core::mem::take(&mut self.g.blast_starts);
        blast_starts.clear();
        for &e in &self.grid.blasts {
            if let Some(mut b) = w.get_mut::<Blast>(e) {
                let was = b.t;
                b.t += dt;
                if was <= 0.0 && b.t > 0.0 {
                    blast_starts.push(*b);
                }
            }
        }
        self.grid.blasts.retain(|&e| {
            let keep = w.get::<Blast>(e).is_some_and(|b| b.t < 0.6);
            if !keep {
                w.despawn(e);
            }
            keep
        });
        if fx.particles && self.g.phase != Phase::Title {
            let (n, nb) = (((7.0 * fx.particle_density) as usize).max(1), ((14.0 * fx.particle_density) as usize).max(2));
            self.sparkle(fx, |s| {
                for &(x, y, c) in &starts {
                    s.burst(x, y, c, n, 5.5);
                }
                for b in &blast_starts {
                    s.blast(b, nb);
                }
            });
        }
        self.g.pop_starts = starts;
        self.g.blast_starts = blast_starts;
        self.floats.step(dt);
        if !fx.juice {
            // Score texts and combo callouts are juice; the Shuffle! notice
            // is not: it explains why every piece moved.
            self.floats.items.retain(|f| f.text.as_str() == SHUFFLE_CALLOUT);
        }
        // Puff calms down after a while (not when the level is over).
        if let Some(mut p) = self.grid.puff.and_then(|e| self.w.get_mut::<Puff>(e)) {
            let calm = match p.mood {
                Mood::Happy => 1.2,
                Mood::Excited => 1.8,
                _ => f32::MAX,
            };
            if p.mood_t > calm {
                p.mood = Mood::Idle;
                p.mood_t = 0.0;
            }
        }
    }
}

/// Is a piece a special that glows?
pub fn glows(p: &Piece) -> bool {
    p.kind.is_special()
}

/// Add a score text at cell `(x, y)` worth `s` points to one step's texts.
/// Texts closer than a text's size (on the narrowest screens a "+1040" is
/// about 1.5 cells wide and half a cell high) become one text with the sum,
/// half-way between, so two scores never print over each other.
fn add_score_text(texts: &mut Vec<(f32, f32, f32)>, x: f32, y: f32, s: f32) {
    let (mut x, mut y, mut s) = (x, y, s);
    while let Some(i) = texts.iter().position(|&(tx, ty, _)| (tx - x).abs() < 1.6 && (ty - y).abs() < 0.75) {
        let (tx, ty, ts) = texts.swap_remove(i);
        (x, y, s) = ((x + tx) * 0.5, (y + ty) * 0.5, s + ts);
    }
    texts.push((x, y, s));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::levels_from_json;

    fn game() -> SimWorld {
        SimWorld::new(levels_from_json(crate::LEVELS_JSON).unwrap(), Rules::default())
    }

    const FNS: Fns<'static> = Fns {
        match_score: &default_match_score_in,
    };

    fn step(g: &mut SimWorld, t: &Tune, input: &PlayerInput) {
        g.tick(input, t, &FNS, &FxFlags::default(), 1.0 / 60.0);
    }

    /// Play until the board is idle (or `max` ticks).
    fn settle(g: &mut SimWorld, t: &Tune, max: u32) {
        for _ in 0..max {
            step(g, t, &PlayerInput::default());
            let gm = g.game();
            if gm.step == Step::Idle && gm.phase != Phase::Intro {
                return;
            }
        }
    }

    /// A bot: plays the best (or worst) move each time the board is idle.
    pub fn play_level(level: usize, best: bool) -> SimWorld {
        let t = Tune::default();
        let mut g = game();
        g.request(Request::Start(level));
        for _ in 0..60 * 60 * 5 {
            step(&mut g, &t, &PlayerInput::default());
            let gm = g.game();
            if matches!(gm.phase, Phase::Won | Phase::Lost) {
                break;
            }
            if gm.phase == Phase::Play && gm.step == Step::Idle {
                drop(gm);
                let grid = g.grid();
                let m = if best { grid.board.best_move() } else { grid.board.worst_move() };
                drop(grid);
                if let Some(m) = m {
                    g.request(Request::Swap(m.a, m.b));
                }
            }
        }
        g
    }

    fn goals(g: &SimWorld) -> Vec<GoalState> {
        g.game().goals(&g.grid().board)
    }

    #[test]
    fn title_attract_board_plays_itself() {
        let t = Tune::default();
        let mut g = game();
        assert_eq!(g.game().phase, Phase::Title);
        for _ in 0..60 * 10 {
            step(&mut g, &t, &PlayerInput::default());
        }
        assert!(g.game().score > 0, "the attract board made matches");
        assert_eq!(g.game().phase, Phase::Title);
    }

    #[test]
    fn pieces_pops_blasts_and_puff_are_entities() {
        let t = Tune::default();
        let mut g = game();
        let board_pieces = g.grid().board.cells.iter().flatten().count();
        assert_eq!(g.world.count::<Cell>(), board_pieces);
        assert_eq!(g.world.count::<Puff>(), 1);
        let mut popped = false;
        for _ in 0..60 * 10 {
            step(&mut g, &t, &PlayerInput::default());
            let grid = g.grid();
            // Every piece on the board is its entity, in its cell.
            for (i, c) in grid.board.cells.iter().enumerate() {
                match (c, grid.slots[i]) {
                    (None, None) => {}
                    (Some(p), Some(e)) => {
                        assert_eq!(*g.world.get::<Piece>(e).unwrap(), *p);
                        assert_eq!(g.world.get::<Cell>(e).unwrap().0 as usize, i);
                    }
                    other => panic!("cell {i}: {other:?}"),
                }
            }
            assert_eq!(g.world.count::<Popping>(), grid.pops.len());
            assert_eq!(g.world.count::<Blast>(), grid.blasts.len());
            assert_eq!(g.world.count::<Motion>(), g.world.count::<Cell>());
            popped |= !grid.pops.is_empty();
        }
        assert!(popped, "the attract board popped pieces as entities");
        // A new level replaces the board's entities; nothing leaks.
        g.request(Request::Start(1));
        step(&mut g, &t, &PlayerInput::default());
        let n = g.grid().board.cells.iter().flatten().count();
        assert_eq!(g.world.count::<Piece>(), n);
        assert_eq!(g.world.entity_count(), n + 1, "the pieces and Puff");
    }

    #[test]
    fn best_bot_wins_level_one_and_worst_bot_loses() {
        let won = play_level(0, true);
        let gm = won.game();
        assert_eq!(gm.phase, Phase::Won, "score {} goals {:?}", gm.score, goals(&won));
        assert!(gm.stars >= 1);
        assert!(gm.goals_met(&won.grid().board));
        drop(gm);
        let lost = play_level(0, false);
        assert_eq!(lost.game().phase, Phase::Lost, "goals {:?}", goals(&lost));
        assert_eq!(lost.game().moves_left, 0);
    }

    #[test]
    fn same_moves_same_game() {
        let a = play_level(0, true);
        let b = play_level(0, true);
        assert_eq!(a.game().score, b.game().score);
        assert_eq!(a.game().moves_used, b.game().moves_used);
    }

    #[test]
    fn invalid_swap_bounces_back_and_costs_nothing() {
        let t = Tune::default();
        let mut g = game();
        g.request(Request::Start(0));
        settle(&mut g, &t, 600);
        settle(&mut g, &t, 600);
        assert_eq!(g.game().phase, Phase::Play);
        // Find an adjacent pair that is not a valid move.
        let before = g.grid().board.clone();
        let (a, b) = (0..before.len())
            .flat_map(|i| [(i, i + 1), (i, i + before.w)])
            .find(|&(a, b)| b < before.len() && before.adjacent(a, b) && !before.valid_swap(a, b))
            .unwrap();
        let moves = g.game().moves_left;
        g.request(Request::Swap(a, b));
        step(&mut g, &t, &PlayerInput::default());
        assert!(matches!(g.game().step, Step::Invalid { .. }));
        settle(&mut g, &t, 120);
        assert_eq!(g.game().moves_left, moves);
        assert_eq!(
            g.grid().board.cells.iter().map(|c| c.map(|p| p.id)).collect::<Vec<_>>(),
            before.cells.iter().map(|c| c.map(|p| p.id)).collect::<Vec<_>>()
        );
        assert!(g.game().events.contains(&Event::Invalid));
    }

    #[test]
    fn pointer_drag_and_tap_tap_swap() {
        let t = Tune::default();
        let mut g = game();
        g.game_mut().layout = crate::layout::layout(1280.0, 720.0, 8, 8, crate::layout::CellLimits::default());
        g.request(Request::Start(0));
        settle(&mut g, &t, 600);
        settle(&mut g, &t, 600);
        assert_eq!(g.game().phase, Phase::Play);
        let centres = |g: &SimWorld| {
            let grid = g.grid();
            let m = grid.board.best_move().unwrap();
            let (ax, ay) = grid.board.xy(m.a);
            let (bx, by) = grid.board.xy(m.b);
            let l = g.game().layout;
            (m, l.center(ax as f32, ay as f32), l.center(bx as f32, by as f32))
        };
        let (m, pa, pb) = centres(&g);
        // Drag: press on a, move towards b (one frame each).
        let press = PlayerInput {
            pointer: PrimaryPointer {
                pos: pa,
                press_pos: pa,
                down: true,
                pressed: true,
                ..PrimaryPointer::default()
            },
            ..PlayerInput::default()
        };
        step(&mut g, &t, &press);
        assert_eq!(g.game().selected, Some(m.a));
        let mid = [(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5];
        let drag = PlayerInput {
            pointer: PrimaryPointer {
                pos: mid,
                press_pos: pa,
                down: true,
                ..PrimaryPointer::default()
            },
            ..PlayerInput::default()
        };
        let moves = g.game().moves_left;
        step(&mut g, &t, &drag);
        assert!(matches!(g.game().step, Step::Swap { .. }), "{:?}", g.game().step);
        assert_eq!(g.game().moves_left, moves - 1);
        settle(&mut g, &t, 1200);
        // Tap-tap with a one-frame touch tap each (down and up in one tick).
        let (m, pa, pb) = centres(&g);
        let tap = |p: [f32; 2]| PlayerInput {
            pointer: PrimaryPointer {
                pos: p,
                press_pos: p,
                down: false,
                pressed: true,
                released: true,
                kind: evani_core::input::PointerKind::Touch,
            },
            ..PlayerInput::default()
        };
        step(&mut g, &t, &tap(pa));
        assert_eq!(g.game().selected, Some(m.a));
        step(&mut g, &t, &PlayerInput::default());
        step(&mut g, &t, &tap(pb));
        assert!(matches!(g.game().step, Step::Swap { .. }), "{:?}", g.game().step);
    }

    #[test]
    fn keyboard_cursor_picks_and_swaps() {
        let t = Tune::default();
        let mut g = game();
        g.request(Request::Start(0));
        settle(&mut g, &t, 600);
        settle(&mut g, &t, 600);
        let m = g.grid().board.best_move().unwrap();
        g.game_mut().cursor = m.a;
        step(&mut g, &t, &PlayerInput { pick: true, ..PlayerInput::default() });
        assert_eq!(g.game().selected, Some(m.a));
        let (ax, ay) = g.grid().board.xy(m.a);
        let (bx, by) = g.grid().board.xy(m.b);
        let dir = match (bx - ax, by - ay) {
            (0, -1) => 0,
            (0, 1) => 1,
            (-1, 0) => 2,
            _ => 3,
        };
        let mut dirs = [false; 4];
        dirs[dir] = true;
        step(&mut g, &t, &PlayerInput { dirs, ..PlayerInput::default() });
        assert!(matches!(g.game().step, Step::Swap { .. }));
    }

    #[test]
    fn pause_freezes_and_effects_never_change_the_game() {
        let t = Tune::default();
        let run = |fx: FxFlags| {
            let mut g = game();
            // A particle system is "in the build" and the board has a layout.
            g.world.res_mut::<ParticleQueue>().attach();
            g.game_mut().layout = crate::layout::layout(1280.0, 720.0, 8, 8, crate::layout::CellLimits::default());
            let mut emitted = 0;
            let mut sink = Vec::new();
            g.request(Request::Start(0));
            for _ in 0..60 * 40 {
                g.tick(&PlayerInput::default(), &t, &FNS, &fx, 1.0 / 60.0);
                g.world.res_mut::<ParticleQueue>().drain_into(&mut sink);
                emitted += sink.len();
                sink.clear();
                let idle = g.game().phase == Phase::Play && g.game().step == Step::Idle;
                let m = g.grid().board.best_move();
                if idle && let Some(m) = m {
                    g.request(Request::Swap(m.a, m.b));
                }
            }
            let gm = g.game();
            (gm.score, gm.moves_used, emitted)
        };
        let on = run(FxFlags::default());
        let off = run(FxFlags {
            juice: false,
            particles: false,
            ..FxFlags::default()
        });
        assert_eq!((on.0, on.1), (off.0, off.1));
        assert_eq!(off.2, 0);
        assert!(on.2 > 100, "a game with pops sparkles: {} emits", on.2);
        let mut g = game();
        g.request(Request::Start(0));
        g.request(Request::Pause(true));
        step(&mut g, &t, &PlayerInput::default());
        let pt = g.game().phase_t;
        step(&mut g, &t, &PlayerInput::default());
        assert_eq!(g.game().phase_t, pt);
        assert!(g.game().paused);
    }

    #[test]
    fn cascades_call_out_and_specials_shake() {
        let won = play_level(0, true);
        assert!(won.game().best_cascade >= 1, "a best-move game cascades at least once");
    }

    #[test]
    fn every_level_is_winnable_by_the_bot() {
        for l in 0..3 {
            let g = play_level(l, true);
            let gm = g.game();
            assert_eq!(gm.phase, Phase::Won, "level {} goals {:?} score {}", l + 1, goals(&g), gm.score);
        }
    }

    /// The review's playtest bot (docs/KIT_PROCESS.md item 21): the best
    /// move each time wins every level.
    #[test]
    fn playtest_win_every_level_with_the_best_moves() {
        for l in 0..3 {
            let g = play_level(l, true);
            let gm = g.game();
            assert_eq!(gm.phase, Phase::Won, "level {} goals {:?} score {}", l + 1, goals(&g), gm.score);
            assert!(gm.stars >= 1, "level {}: no star", l + 1);
            assert!(gm.goals_met(&g.grid().board), "level {}: goals not met", l + 1);
        }
    }

    /// The review's playtest bot: the weakest move each time runs out of
    /// moves on level 1 and loses.
    #[test]
    fn playtest_lose_level_one_with_the_weakest_moves() {
        let g = play_level(0, false);
        let gm = g.game();
        assert_eq!(gm.phase, Phase::Lost, "goals {:?}", goals(&g));
        assert_eq!(gm.moves_left, 0);
        assert!(!gm.goals_met(&g.grid().board));
    }

    #[test]
    fn close_score_texts_merge_into_one() {
        let mut t = Vec::new();
        add_score_text(&mut t, 2.0, 3.0, 50.0);
        // Far apart: two texts.
        add_score_text(&mut t, 6.0, 3.0, 60.0);
        assert_eq!(t.len(), 2);
        // Within a text's size of the first: one text with the sum, half-way.
        add_score_text(&mut t, 3.0, 3.5, 70.0);
        assert_eq!(t.len(), 2);
        let m = t.iter().find(|x| x.2 == 120.0).copied().unwrap();
        assert_eq!((m.0, m.1), (2.5, 3.25));
        // A text between two texts merges with one, then the merged text
        // with the other (a chain): one text with all the points.
        let mut c = Vec::new();
        add_score_text(&mut c, 0.0, 0.0, 10.0);
        add_score_text(&mut c, 2.0, 0.0, 20.0);
        assert_eq!(c.len(), 2);
        add_score_text(&mut c, 1.0, 0.0, 30.0);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].2, 60.0);
        // Same row but further than 1.6 cells, or a row apart: separate.
        let mut u = Vec::new();
        add_score_text(&mut u, 0.0, 0.0, 1.0);
        add_score_text(&mut u, 1.6, 0.0, 1.0);
        add_score_text(&mut u, 0.0, 0.75, 1.0);
        assert_eq!(u.len(), 3);
    }

    /// Score texts are gone (under 10 % opacity, as the page fades them)
    /// before any piece falls, so they never cover the falling pieces the
    /// eye follows; combo callouts sit off the board (hud::callout_anchor).
    #[test]
    fn no_score_text_shows_while_pieces_fall() {
        use crate::ecs::Floats;
        let t = Tune::default();
        for (level, best) in [(0, true), (1, true), (2, true), (0, false)] {
            let mut g = game();
            g.request(Request::Start(level));
            let (mut texts, mut falls) = (0, 0);
            for _ in 0..60 * 60 * 5 {
                step(&mut g, &t, &PlayerInput::default());
                let falling = g.grid().slots.iter().flatten().any(|&e| g.world.get::<Motion>(e).is_some_and(|m| m.off[1] < 0.0));
                let shown: Vec<String> = g
                    .world
                    .res::<Floats>()
                    .items
                    .iter()
                    .filter(|f| f.style == 0 && crate::hud::float_opacity(f.age / f.life.max(0.01)) >= 0.1)
                    .map(|f| f.text.as_str().to_string())
                    .collect();
                texts += usize::from(!shown.is_empty());
                if falling {
                    falls += 1;
                    assert!(shown.is_empty(), "level {} ({best}): {shown:?} still shows while pieces fall", level + 1);
                }
                let (phase, idle) = (g.game().phase, g.game().step == Step::Idle);
                if matches!(phase, Phase::Won | Phase::Lost) {
                    break;
                }
                if phase == Phase::Play && idle {
                    let m = if best { g.grid().board.best_move() } else { g.grid().board.worst_move() };
                    if let Some(m) = m {
                        g.request(Request::Swap(m.a, m.b));
                    }
                }
            }
            assert!(texts > 0 && falls > 0, "level {}: the game showed score texts ({texts}) and falls ({falls})", level + 1);
        }
    }

    #[test]
    fn free_roam_has_no_move_cost_goals_or_end() {
        let t = Tune::default();
        let mut g = game();
        g.game_mut().free_roam = true;
        g.request(Request::Start(0));
        let total = g.world.res::<Game>().levels[0].moves;
        let mut moves = 0;
        // Twice the level's moves, playing the weakest move each time.
        for _ in 0..60 * 60 * 5 {
            step(&mut g, &t, &PlayerInput::default());
            let idle = g.game().phase == Phase::Play && g.game().step == Step::Idle;
            if idle {
                if moves >= total * 2 {
                    break;
                }
                let m = g.grid().board.worst_move();
                if let Some(m) = m {
                    g.request(Request::Swap(m.a, m.b));
                    moves += 1;
                }
            }
        }
        let gm = g.game();
        assert_eq!(moves, total * 2, "the bot kept playing");
        assert_eq!(gm.phase, Phase::Play, "no win, loss or Sugar Rush");
        assert_eq!(gm.moves_left, total, "moves cost nothing");
        assert_eq!(gm.moves_used, total * 2);
        assert!(gm.goals(&g.grid().board).is_empty(), "no goals");
        assert!(!gm.events.iter().any(|e| matches!(e, Event::Win | Event::Lose | Event::GoalDone)));
        // The board keeps refilling: every playable cell holds a piece.
        let grid = g.grid();
        let empty = (0..grid.board.len()).filter(|&i| grid.board.mask[i] && grid.board.cells[i].is_none()).count();
        assert_eq!(empty, 0);
    }
}
