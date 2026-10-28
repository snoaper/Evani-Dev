//! The game: phases, the hero controller (coyote time, jump buffering,
//! variable jump height), enemies, moving platforms, coins, checkpoints,
//! lives, score and timer. A fixed-step, deterministic simulation: only
//! `+ - * /`, `sqrt` and comparisons (no `exp`/`sin`), so the attract demo
//! replays identically natively and in the browser.

use evani_core::num::approach;
use std::rc::Rc;

use crate::body::{Body, MoveOpts, PlatformTop, overlap};
use crate::level::{Level, SurfaceKind, TILE};
use crate::{Ints, Tune};

/// Game phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Title screen (the attract demo plays behind it).
    #[default]
    Title,
    Playing,
    Paused,
    /// Reached the castle gate.
    Won,
    /// Out of lives.
    Lost,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Title => "title",
            Phase::Playing => "playing",
            Phase::Paused => "paused",
            Phase::Won => "won",
            Phase::Lost => "lost",
        }
    }
}

/// What the game's menus ask for; applied at the start of the next tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// A new run from the start of the level.
    Start,
    Restart,
    /// Back to the title screen.
    Title,
    Pause(bool),
    TogglePause,
}

/// Something that happened this tick (sounds, shake, HUD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Jump,
    Land,
    Coin,
    Stomp,
    Hurt,
    Checkpoint,
    Life,
    Win,
    Lose,
    Bump,
    Respawn,
}

/// One tick of player input.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Controls {
    /// -1..1.
    pub x: f32,
    pub jump: bool,
    pub jump_pressed: bool,
    pub run: bool,
    pub down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hero {
    pub body: Body,
    pub prev: [f32; 2],
    /// 1 right, -1 left.
    pub facing: f32,
    pub coyote: f32,
    pub buffer: f32,
    pub drop_t: f32,
    /// A jump is rising and can still be cut by releasing jump.
    pub cut_armed: bool,
    pub invuln: f32,
    /// Seconds since death (0 = alive).
    pub dying: f32,
    /// Run cycle phase (pixels travelled).
    pub stride: f32,
    pub air_t: f32,
    /// Seconds since landing (squash).
    pub landed_t: f32,
}

impl Hero {
    pub fn alive(&self) -> bool {
        self.dying <= 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyKind {
    Beetle,
    Hopper,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub body: Body,
    pub prev: [f32; 2],
    pub dir: f32,
    pub awake: bool,
    /// Seconds since squashed (negative while alive).
    pub squashed: f32,
    /// Fell into water or out of the map.
    pub gone: bool,
    /// Hopper: seconds sitting.
    pub timer: f32,
    /// Seconds alive (animation).
    pub age: f32,
}

impl Enemy {
    pub fn alive(&self) -> bool {
        self.squashed < 0.0 && !self.gone
    }
}

/// A short-lived effect in the world (score pops, dust, sparkles). Kept in
/// the simulation so a replay draws exactly the same thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pop {
    pub kind: PopKind,
    pub pos: [f32; 2],
    pub t: f32,
}

/// Floating text (score, "1UP") rises and fades within this many seconds.
pub const POP_TEXT_LIFE: f32 = 0.8;
/// A score pop younger than this takes the next score into its total (a
/// coin line or a stomp chain shows one running combo, not a stack).
pub const POP_MERGE: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopKind {
    Score(u32),
    OneUp,
    Sparkle,
    Dust,
    Puff,
}

impl PopKind {
    pub fn life(self) -> f32 {
        match self {
            PopKind::Score(_) | PopKind::OneUp => POP_TEXT_LIFE,
            PopKind::Sparkle => 0.35,
            PopKind::Dust => 0.3,
            PopKind::Puff => 0.45,
        }
    }
}

/// Results once the level is over.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Results {
    pub won: bool,
    pub coins: u32,
    pub stomps: u32,
    pub time_left: u32,
    pub time_bonus: u32,
    pub score: u32,
}

/// Fail moments counted since the last [`Game::take_fails`]; the kit's sim
/// system hands them to `evani_core::play::PlayOutcomes`. Only Play mode
/// counts any (free roam has none: docs/PLAYGROUNDS.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fails {
    /// Lives lost (a death of any kind).
    pub lives_lost: u32,
    /// The level timer ran out.
    pub timers_expired: u32,
    /// The run ended: out of lives, or the castle gate reached.
    pub game_overs: u32,
}

impl Fails {
    pub fn any(&self) -> bool {
        self.lives_lost + self.timers_expired + self.game_overs > 0
    }
}

/// The 2D camera in world art pixels. `focus[0]` is the x the view centres
/// on; `focus[1]` is the ground line, the y the draw places at
/// `cam_ground_frac` of the view height (clamping to the level and the
/// screen size happen when drawing). `prev` is last tick's focus.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cam {
    pub focus: [f32; 2],
    pub prev: [f32; 2],
    /// Eased look-ahead in x (signed px).
    pub look: f32,
    /// Feet y of the ground the hero last stood on.
    pub anchor: f32,
}

impl Cam {
    /// A camera resting on the hero standing at `feet`.
    pub fn at(feet: [f32; 2]) -> Self {
        Self { focus: feet, prev: feet, look: 0.0, anchor: feet[1] }
    }
}

/// The whole game state (world resource through [`crate::Platformer`]).
#[derive(Debug, Clone)]
pub struct Game {
    pub level: Rc<Level>,
    pub phase: Phase,
    /// Phase to return to when unpausing.
    resume: Phase,
    pub hero: Hero,
    pub enemies: Vec<Enemy>,
    pub coins_taken: Vec<bool>,
    pub flags: Vec<bool>,
    /// Platform top-left now and last tick.
    pub plats: Vec<[f32; 2]>,
    pub plats_prev: Vec<[f32; 2]>,
    pub lives: i32,
    pub coins: u32,
    pub score: u32,
    pub stomps: u32,
    pub time_left: f32,
    pub level_time: f64,
    pub tick: u64,
    pub phase_time: f32,
    respawn: [f32; 2],
    respawn_time: f32,
    pub cam: Cam,
    /// Shake trauma 0..1.
    pub trauma: f32,
    pub events: Vec<Event>,
    pub pops: Vec<Pop>,
    pub results: Option<Results>,
    pub requests: Vec<Request>,
    /// Bumped on every new run.
    pub run_no: u64,
    /// Free roam (`PlayMode::FreeRoam`): no lives, timer, game over or level
    /// goal. A pit, water, spikes or the map's edge put the hero back on the
    /// last safe ground; enemies can be stomped but cannot hurt.
    pub free_roam: bool,
    /// Free roam: times the hero was put back on safe ground this run.
    pub free_respawns: u32,
    /// Last spot the hero stood well on solid ground, or the last
    /// checkpoint (free roam's respawn point).
    safe: [f32; 2],
    fails: Fails,
    /// The [`crate::ecs::Objects`] sync this game's objects were last
    /// written in (0: never; a game replaced as a whole keeps an older one).
    pub ecs_epoch: u64,
}

const DEATH_S: f32 = 1.4;
/// Free roam: floor the hero needs on each side of a spot (beyond its own
/// half width) for it to count as safe ground: two tiles, enough to stand
/// clear of an edge and to get a running start at the gap.
const SAFE_MARGIN: f32 = 2.0 * TILE;
/// Free roam: spacing of the floor probes across that margin (a gap
/// narrower than this cannot hide between two probes).
const SAFE_STEP: f32 = 4.0;
/// How far the hero may sit below the ground line before the camera drags
/// down with it (a fall, a pit).
const CAM_BELOW: f32 = 24.0;

impl Game {
    pub fn new(level: Rc<Level>, t: &Tune, n: &Ints) -> Self {
        let mut g = Self {
            level,
            phase: Phase::Title,
            resume: Phase::Playing,
            hero: Hero::default(),
            enemies: Vec::new(),
            coins_taken: Vec::new(),
            flags: Vec::new(),
            plats: Vec::new(),
            plats_prev: Vec::new(),
            lives: 0,
            coins: 0,
            score: 0,
            stomps: 0,
            time_left: 0.0,
            level_time: 0.0,
            tick: 0,
            phase_time: 0.0,
            respawn: [0.0; 2],
            respawn_time: 0.0,
            cam: Cam::default(),
            trauma: 0.0,
            events: Vec::new(),
            pops: Vec::new(),
            results: None,
            requests: Vec::new(),
            run_no: 0,
            free_roam: false,
            free_respawns: 0,
            safe: [0.0; 2],
            fails: Fails::default(),
            ecs_epoch: 0,
        };
        g.reset(Phase::Title, t, n);
        g
    }

    /// Switch free roam on or off, and start a fresh run: straight into the
    /// level when on (there is no title screen to wait at), the title when off.
    pub fn set_free_roam(&mut self, on: bool, t: &Tune, n: &Ints) {
        self.free_roam = on;
        self.reset(if on { Phase::Playing } else { Phase::Title }, t, n);
    }

    /// The fail moments since the last call (and clear them).
    pub fn take_fails(&mut self) -> Fails {
        core::mem::take(&mut self.fails)
    }

    /// The level's time limit: the param when set, else the data.
    pub fn time_limit(&self, t: &Tune) -> f32 {
        if t.time_limit_s > 0.0 { t.time_limit_s } else { self.level.time_limit_s }
    }

    /// Fresh run of the level in `phase`.
    pub fn reset(&mut self, phase: Phase, t: &Tune, n: &Ints) {
        let l = Rc::clone(&self.level);
        self.phase = phase;
        self.resume = Phase::Playing;
        self.hero = Hero {
            body: Body::new(l.start, t.hero_w * 0.5, t.hero_h),
            prev: l.start,
            facing: 1.0,
            ..Hero::default()
        };
        self.hero.body.grounded = true;
        let enemy = |kind, p: [f32; 2]| {
            let (hw, h) = match kind {
                EnemyKind::Beetle => (5.0, 10.0),
                EnemyKind::Hopper => (5.0, 11.0),
            };
            Enemy {
                kind,
                body: Body::new(p, hw, h),
                prev: p,
                dir: -1.0,
                awake: false,
                squashed: -1.0,
                gone: false,
                timer: 0.0,
                age: 0.0,
            }
        };
        self.enemies = l
            .beetles
            .iter()
            .map(|p| enemy(EnemyKind::Beetle, *p))
            .chain(l.hoppers.iter().map(|p| enemy(EnemyKind::Hopper, *p)))
            .collect();
        self.coins_taken = vec![false; l.coins.len()];
        self.flags = vec![false; l.checkpoints.len()];
        self.level_time = 0.0;
        self.plats = l.platforms.iter().map(|p| p.pos_at(0.0, plat_speed(p.speed, t))).collect();
        self.plats_prev = self.plats.clone();
        self.lives = n.lives.max(1);
        self.coins = 0;
        self.score = 0;
        self.stomps = 0;
        self.time_left = self.time_limit(t);
        self.tick = 0;
        self.phase_time = 0.0;
        self.respawn = l.start;
        self.safe = l.start;
        self.free_respawns = 0;
        self.fails = Fails::default();
        self.respawn_time = self.time_left;
        self.cam = Cam::at(l.start);
        self.trauma = 0.0;
        self.events.clear();
        self.pops.clear();
        self.results = None;
        self.run_no += 1;
    }

    pub fn request(&mut self, r: Request) {
        self.requests.push(r);
    }

    fn apply_requests(&mut self, t: &Tune, n: &Ints) {
        for r in core::mem::take(&mut self.requests) {
            match r {
                Request::Start | Request::Restart => self.reset(Phase::Playing, t, n),
                // Free roam has no title screen: back to the title is a fresh walk.
                Request::Title => self.reset(if self.free_roam { Phase::Playing } else { Phase::Title }, t, n),
                Request::Pause(true) => self.pause(true),
                Request::Pause(false) => self.pause(false),
                Request::TogglePause => {
                    let p = self.phase != Phase::Paused;
                    self.pause(p);
                }
            }
        }
    }

    fn pause(&mut self, on: bool) {
        if on && self.phase == Phase::Playing {
            self.resume = self.phase;
            self.phase = Phase::Paused;
        } else if !on && self.phase == Phase::Paused {
            self.phase = self.resume;
        }
    }

    /// Moving platform tops (now and last tick) for collision.
    pub fn platform_tops(&self) -> Vec<PlatformTop> {
        self.level
            .platforms
            .iter()
            .zip(self.plats.iter().zip(&self.plats_prev))
            .map(|(d, (p, q))| PlatformTop { x0: p[0], x1: p[0] + d.width_px, y: p[1], prev_y: q[1] })
            .collect()
    }

    fn pop(&mut self, kind: PopKind, pos: [f32; 2]) {
        if self.pops.len() < 48 {
            self.pops.push(Pop { kind, pos, t: 0.0 });
        }
    }

    pub(crate) fn add_score(&mut self, pts: u32, at: [f32; 2]) {
        self.score = self.score.saturating_add(pts);
        // Merge into a fresh score pop: the total restarts where the newest
        // points were won.
        if let Some(p) = self.pops.iter_mut().rev().find(|p| matches!(p.kind, PopKind::Score(_)) && p.t < POP_MERGE)
            && let PopKind::Score(total) = p.kind
        {
            *p = Pop { kind: PopKind::Score(total.saturating_add(pts)), pos: at, t: 0.0 };
            return;
        }
        self.pop(PopKind::Score(pts), at);
    }

    /// One fixed step.
    pub fn tick(&mut self, c: Controls, t: &Tune, n: &Ints, gravity: f32, dt: f32) {
        self.apply_requests(t, n);
        // The draw system drains events every frame; keep an undrawn game
        // (headless, the bot's clones) from growing them.
        if self.events.len() > 64 {
            self.events.drain(..32);
        }
        self.cam.prev = self.cam.focus;
        self.hero.prev = self.hero.body.pos;
        for e in &mut self.enemies {
            e.prev = e.body.pos;
        }
        match self.phase {
            Phase::Title | Phase::Paused => {
                self.plats_prev.clone_from(&self.plats);
                return;
            }
            Phase::Won | Phase::Lost => {
                self.phase_time += dt;
                self.age_pops(dt);
                self.trauma = (self.trauma - t.shake_decay * dt).max(0.0);
                if self.phase == Phase::Won && self.hero.alive() {
                    // Walk on into the gate.
                    let goal_x = self.level.goal[0];
                    let h = &mut self.hero;
                    h.body.vel[0] = ((goal_x - h.body.pos[0]) * 4.0).clamp(-40.0, 40.0);
                    h.body.pos[0] += h.body.vel[0] * dt;
                    h.stride += h.body.vel[0].abs() * dt;
                }
                return;
            }
            Phase::Playing => {}
        }
        self.tick += 1;
        self.phase_time += dt;
        self.level_time += dt as f64;
        if !self.free_roam {
            self.time_left -= dt;
        }
        self.age_pops(dt);
        self.trauma = (self.trauma - t.shake_decay * dt).max(0.0);
        // Platforms are a pure function of level time.
        self.plats_prev.clone_from(&self.plats);
        for (i, d) in self.level.platforms.iter().enumerate() {
            self.plats[i] = d.pos_at(self.level_time, plat_speed(d.speed, t));
        }
        let tops = self.platform_tops();
        if self.hero.alive() {
            self.hero_step(c, t, gravity, dt, &tops);
            if !self.free_roam && self.time_left <= 0.0 && self.hero.alive() {
                self.fails.timers_expired += 1;
                self.kill(t);
            }
        } else {
            self.dying_step(t, n, gravity, dt);
        }
        self.enemies_step(t, gravity, dt, &tops);
        if self.hero.alive() {
            self.contacts(t, n);
        }
        self.camera_step(t, dt);
    }

    fn hero_step(&mut self, c: Controls, t: &Tune, gravity: f32, dt: f32, tops: &[PlatformTop]) {
        let level = Rc::clone(&self.level);
        let h = &mut self.hero;
        let was_grounded = h.body.grounded;
        // Carried by a platform first.
        let mut carry = [0.0f32; 2];
        if was_grounded && let Some(SurfaceKind::Platform(i)) = h.body.ground {
            carry = [self.plats[i][0] - self.plats_prev[i][0], self.plats[i][1] - self.plats_prev[i][1]];
            h.body.move_x(&level, carry[0]);
            h.body.pos[1] = tops[i].y;
        }
        // Run.
        let x = c.x.clamp(-1.0, 1.0);
        let top = if c.run { t.run_speed } else { t.walk_speed };
        let target = x * top;
        let vx = h.body.vel[0];
        let accel = if x.abs() > 0.01 {
            if vx * x < 0.0 {
                t.turn_accel
            } else if was_grounded {
                t.ground_accel
            } else {
                t.air_accel
            }
        } else if was_grounded {
            t.ground_friction
        } else {
            t.air_friction
        };
        h.body.vel[0] = approach(vx, target, accel * dt);
        if x.abs() > 0.01 {
            h.facing = x.signum();
        }
        // Jump: buffer and coyote time.
        if c.jump_pressed {
            h.buffer = t.buffer_s;
        } else {
            h.buffer = (h.buffer - dt).max(0.0);
        }
        h.coyote = if was_grounded { t.coyote_s } else { (h.coyote - dt).max(0.0) };
        h.drop_t = (h.drop_t - dt).max(0.0);
        let on_oneway = matches!(h.body.ground, Some(SurfaceKind::OneWay | SurfaceKind::Platform(_)));
        if h.buffer > 0.0 && h.coyote > 0.0 {
            if c.down && was_grounded && on_oneway {
                h.drop_t = t.drop_through_s;
                h.body.grounded = false;
                h.body.ground = None;
            } else {
                let bonus = if h.body.vel[0].abs() > t.walk_speed + 1.0 { t.run_jump_bonus } else { 0.0 };
                h.body.vel[1] = -(t.jump_speed + bonus);
                // Inherit the platform's velocity.
                if carry != [0.0; 2] {
                    h.body.vel[0] += carry[0] / dt;
                    h.body.vel[1] += (carry[1] / dt).min(0.0);
                }
                h.body.grounded = false;
                h.body.ground = None;
                h.cut_armed = true;
                self.events.push(Event::Jump);
                let p = h.body.pos;
                self.pops.push(Pop { kind: PopKind::Dust, pos: p, t: 0.0 });
            }
            h.buffer = 0.0;
            h.coyote = 0.0;
        }
        let h = &mut self.hero;
        // Variable height: releasing jump while rising cuts it.
        if h.cut_armed && h.body.vel[1] < 0.0 && !c.jump {
            h.body.vel[1] *= t.jump_cut;
            h.cut_armed = false;
        }
        if h.body.vel[1] >= 0.0 {
            h.cut_armed = false;
        }
        let g = if h.body.vel[1] > 0.0 { gravity * t.fall_gravity_mult } else { gravity };
        h.body.vel[1] = (h.body.vel[1] + g * dt).min(t.max_fall);
        let falling_speed = h.body.vel[1];
        let opts = MoveOpts { drop_through: h.drop_t > 0.0, snap: t.slope_snap };
        h.body.step(&level, tops, dt, opts);
        if h.body.bonked {
            self.events.push(Event::Bump);
        }
        let h = &mut self.hero;
        if h.body.grounded {
            if !was_grounded && falling_speed > 120.0 {
                h.landed_t = 0.0;
                self.events.push(Event::Land);
                let p = h.body.pos;
                self.pops.push(Pop { kind: PopKind::Dust, pos: p, t: 0.0 });
            }
            if !was_grounded && h.buffer > 0.0 {
                // A jump still buffered on the landing tick fires on the
                // next one (the check runs before the move).
                h.buffer = h.buffer.max(dt * 1.5);
            }
            h.air_t = 0.0;
        } else {
            h.air_t += dt;
        }
        let h = &mut self.hero;
        h.landed_t += dt;
        h.stride += h.body.vel[0].abs() * dt;
        h.invuln = (h.invuln - dt).max(0.0);
        // Hazards: water, spikes, falling out of the map.
        let r = h.body.rect();
        let inset = [r[0] + 2.0, r[1] + 3.0, r[2] - 2.0, r[3] - 1.0];
        let mut hazard = h.body.pos[1] > level.height_px() + 24.0;
        for (x, y) in [(inset[0], inset[3]), (inset[2], inset[3]), (inset[0], inset[1]), (inset[2], inset[1])] {
            match level.tile_at(x, y) {
                crate::level::Tile::Spikes => {
                    // Spikes are the lower 10 px of their tile.
                    hazard |= y - (crate::level::cell(y) as f32 * TILE) > 6.0;
                }
                crate::level::Tile::Water { deep } => {
                    hazard |= deep || y - (crate::level::cell(y) as f32 * TILE) > 5.0;
                }
                _ => {}
            }
        }
        if hazard {
            self.hazard_hit(t);
        } else if self.free_roam {
            self.note_safe(&level);
        }
    }

    /// A pit, water, spikes or the map's edge: a life in Play mode; in free
    /// roam back to the last safe ground at no cost.
    fn hazard_hit(&mut self, t: &Tune) {
        if self.free_roam {
            self.free_respawn(t);
        } else {
            self.kill(t);
        }
    }

    /// Free roam: remember the ground the hero stands well on (solid, not a
    /// moving platform, nothing deadly under it) and with `SAFE_MARGIN`
    /// px of floor on both sides, so a respawn there is a place to stand
    /// still and to take a run-up from, never the lip of the pit it came
    /// from. Narrow ledges are skipped: the last roomy ground stays the
    /// respawn point.
    fn note_safe(&mut self, level: &Level) {
        let b = &self.hero.body;
        if !b.grounded || !matches!(b.ground, Some(SurfaceKind::Solid | SurfaceKind::Slope | SurfaceKind::OneWay)) {
            return;
        }
        if level.tile_at(b.pos[0], b.pos[1] + 2.0).is_hazard() {
            return;
        }
        let mut dx = 0.0;
        while dx <= b.hw + SAFE_MARGIN {
            if !b.floor_at(level, dx) || !b.floor_at(level, -dx) {
                return;
            }
            dx += SAFE_STEP;
        }
        if b.floor_at(level, b.hw + SAFE_MARGIN) && b.floor_at(level, -(b.hw + SAFE_MARGIN)) {
            self.safe = b.pos;
        }
    }

    /// Free roam: put the hero back on the last safe ground, blinking briefly.
    fn free_respawn(&mut self, t: &Tune) {
        let p = self.safe;
        let facing = self.hero.facing;
        self.hero = Hero {
            body: Body::new(p, t.hero_w * 0.5, t.hero_h),
            prev: p,
            facing,
            invuln: t.invuln_s,
            ..Hero::default()
        };
        self.hero.body.grounded = true;
        self.cam = Cam::at(p);
        self.events.push(Event::Respawn);
        self.free_respawns += 1;
        self.pop(PopKind::Sparkle, [p[0], p[1] - 8.0]);
    }

    /// Lose a life: the hero pops up and falls off the screen.
    fn kill(&mut self, t: &Tune) {
        if !self.hero.alive() {
            return;
        }
        self.hero.dying = 1e-4;
        self.hero.body.vel = [0.0, -t.jump_speed * 0.8];
        self.lives -= 1;
        self.fails.lives_lost += 1;
        self.trauma = (self.trauma + t.hurt_trauma).min(1.0);
        self.events.push(Event::Hurt);
    }

    fn dying_step(&mut self, t: &Tune, n: &Ints, gravity: f32, dt: f32) {
        let h = &mut self.hero;
        h.dying += dt;
        if h.dying > 0.35 {
            h.body.vel[1] = (h.body.vel[1] + gravity * dt).min(t.max_fall * 1.5);
            h.body.pos[1] += h.body.vel[1] * dt;
        }
        if h.dying < DEATH_S {
            return;
        }
        if self.lives <= 0 {
            self.finish(false, t, n);
            return;
        }
        // Respawn at the last checkpoint with its time.
        let p = self.respawn;
        self.hero = Hero {
            body: Body::new(p, t.hero_w * 0.5, t.hero_h),
            prev: p,
            facing: 1.0,
            invuln: t.invuln_s,
            ..Hero::default()
        };
        self.hero.body.grounded = true;
        self.time_left = if self.time_left <= 0.0 { self.time_limit(t) } else { self.respawn_time.max(self.time_left) };
        self.cam = Cam::at(p);
        self.events.push(Event::Respawn);
    }

    fn finish(&mut self, won: bool, t: &Tune, n: &Ints) {
        let time_left = self.time_left.max(0.0).ceil() as u32;
        let time_bonus = if won { time_left * n.score_time.max(0) as u32 } else { 0 };
        self.score = self.score.saturating_add(time_bonus);
        self.results = Some(Results {
            won,
            coins: self.coins,
            stomps: self.stomps,
            time_left,
            time_bonus,
            score: self.score,
        });
        self.phase = if won { Phase::Won } else { Phase::Lost };
        self.fails.game_overs += 1;
        self.phase_time = 0.0;
        self.events.push(if won { Event::Win } else { Event::Lose });
        if won {
            self.trauma = (self.trauma + t.goal_trauma).min(1.0);
        }
    }

    fn enemies_step(&mut self, t: &Tune, gravity: f32, dt: f32, tops: &[PlatformTop]) {
        let level = Rc::clone(&self.level);
        let hero = self.hero.body.pos;
        let cam_x = self.cam.focus[0];
        for e in &mut self.enemies {
            if e.gone {
                continue;
            }
            if e.squashed >= 0.0 {
                e.squashed += dt;
                continue;
            }
            if !e.awake {
                if (e.body.pos[0] - cam_x).abs() < t.wake_distance {
                    e.awake = true;
                } else {
                    continue;
                }
            }
            e.age += dt;
            match e.kind {
                EnemyKind::Beetle => {
                    if e.body.grounded && !e.body.floor_ahead(&level, e.dir) {
                        e.dir = -e.dir;
                    }
                    e.body.vel[0] = e.dir * t.beetle_speed;
                }
                EnemyKind::Hopper => {
                    if e.body.grounded {
                        e.body.vel[0] = 0.0;
                        e.timer += dt;
                        let dx = hero[0] - e.body.pos[0];
                        if dx.abs() < t.hopper_sight {
                            e.dir = if dx < 0.0 { -1.0 } else { 1.0 };
                        }
                        if e.timer >= t.hopper_period_s {
                            e.timer = 0.0;
                            e.body.vel[1] = -t.hopper_jump_speed;
                            e.body.vel[0] = e.dir * t.hopper_hop_speed;
                            e.body.grounded = false;
                        }
                    }
                }
            }
            e.body.vel[1] = (e.body.vel[1] + gravity * dt).min(t.max_fall);
            e.body.step(&level, tops, dt, MoveOpts { drop_through: false, snap: t.slope_snap });
            if e.body.wall != 0 {
                e.dir = -(e.body.wall as f32);
                if e.kind == EnemyKind::Hopper {
                    e.body.vel[0] = e.dir * t.hopper_hop_speed * 0.5;
                }
            }
            let p = e.body.pos;
            if p[1] > level.height_px() + 24.0 || matches!(level.tile_at(p[0], p[1] - 3.0), crate::level::Tile::Water { .. }) {
                e.gone = true;
            }
        }
    }

    fn contacts(&mut self, t: &Tune, n: &Ints) {
        let level = Rc::clone(&self.level);
        let hr = self.hero.body.rect();
        let prev_feet = self.hero.prev[1];
        // Enemies: stomp from above, else hurt.
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            if !e.alive() || !e.awake || !overlap(hr, e.body.rect()) {
                continue;
            }
            let top = e.body.top();
            if self.hero.body.vel[1] >= 0.0 && prev_feet <= top + t.stomp_window {
                self.enemies[i].squashed = 0.0;
                self.enemies[i].body.vel = [0.0, 0.0];
                self.stomps += 1;
                let h = &mut self.hero;
                h.body.pos[1] = top;
                h.body.vel[1] = -t.stomp_bounce;
                h.cut_armed = true;
                h.body.grounded = false;
                h.body.ground = None;
                self.trauma = (self.trauma + t.stomp_trauma).min(1.0);
                self.events.push(Event::Stomp);
                self.pop(PopKind::Puff, [e.body.pos[0], top + 2.0]);
                self.add_score(n.score_stomp.max(0) as u32, [e.body.pos[0], top - 10.0]);
            } else if !self.free_roam && self.hero.invuln <= 0.0 {
                // (Free roam: enemies are part of the world, not a danger.)
                self.kill(t);
                return;
            }
        }
        // Coins.
        let (cx, cy) = ((hr[0] + hr[2]) * 0.5, (hr[1] + hr[3]) * 0.5);
        for i in 0..level.coins.len() {
            if self.coins_taken[i] {
                continue;
            }
            let c = level.coins[i];
            if (c[0] - cx).abs() < 5.0 + t.hero_w * 0.5 && (c[1] - cy).abs() < 5.0 + t.hero_h * 0.5 {
                self.coins_taken[i] = true;
                self.coins += 1;
                self.events.push(Event::Coin);
                self.pop(PopKind::Sparkle, c);
                self.add_score(n.score_coin.max(0) as u32, [c[0], c[1] - 8.0]);
                let per = n.coins_per_life.max(1) as u32;
                if self.coins.is_multiple_of(per) {
                    self.lives += 1;
                    self.events.push(Event::Life);
                    self.pop(PopKind::OneUp, [cx, hr[1] - 12.0]);
                }
            }
        }
        // Checkpoints.
        for i in 0..level.checkpoints.len() {
            let f = level.checkpoints[i];
            if !self.flags[i] && overlap(hr, [f[0] - 6.0, f[1] - 32.0, f[0] + 6.0, f[1]]) {
                self.flags[i] = true;
                self.respawn = f;
                self.safe = f;
                self.respawn_time = self.time_left;
                self.events.push(Event::Checkpoint);
                self.pop(PopKind::Sparkle, [f[0], f[1] - 28.0]);
            }
        }
        // The castle gate.
        let g = level.goal;
        // Free roam: the gate is just a place to explore; the run goes on.
        if !self.free_roam && overlap(hr, [g[0] - 10.0, g[1] - 40.0, g[0] + 10.0, g[1]]) {
            self.finish(true, t, n);
        }
    }

    /// The camera (Chris's review, point 1): the ground line eases to the
    /// ground the hero last stood on, so the view rests with that ground in
    /// the lower third and holds still through a jump; it follows up only
    /// once the hero rises `cam_deadzone_y` above it, and down at once when
    /// the hero drops below it. X follows with a dead zone and an eased
    /// look-ahead in the direction of movement.
    fn camera_step(&mut self, t: &Tune, dt: f32) {
        let h = &self.hero;
        if !h.alive() {
            return;
        }
        let c = &mut self.cam;
        let feet = h.body.pos;
        // Look ahead where the hero is going (facing while nearly still).
        let dir = if h.body.vel[0].abs() > 8.0 { h.body.vel[0].signum() } else { h.facing };
        c.look += (dir * t.cam_lookahead - c.look) * (t.cam_smooth * dt).min(1.0);
        let want_x = feet[0] + c.look;
        if want_x > c.focus[0] + t.cam_deadzone_x {
            c.focus[0] = want_x - t.cam_deadzone_x;
        } else if want_x < c.focus[0] - t.cam_deadzone_x {
            c.focus[0] = want_x + t.cam_deadzone_x;
        }
        // Y: the ground line.
        if h.body.grounded {
            c.anchor = feet[1];
        }
        let want = if feet[1] > c.anchor { feet[1] } else { c.anchor };
        c.focus[1] += (want - c.focus[1]) * (t.cam_smooth_y * dt).min(1.0);
        // Hard edges of the vertical dead zone: never more than
        // `cam_deadzone_y` below the hero, nor far above it.
        c.focus[1] = c.focus[1].min(feet[1] + t.cam_deadzone_y).max(feet[1] - CAM_BELOW);
    }

    fn age_pops(&mut self, dt: f32) {
        for p in &mut self.pops {
            p.t += dt;
        }
        self.pops.retain(|p| p.t < p.kind.life());
    }
}

fn plat_speed(data: f32, t: &Tune) -> f32 {
    if data > 0.0 { data } else { t.platform_speed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LEVEL_JSON;

    fn game() -> (Game, Tune, Ints) {
        let t = Tune::default();
        let n = Ints::default();
        let level = Rc::new(Level::from_json(LEVEL_JSON).unwrap());
        let mut g = Game::new(level, &t, &n);
        g.request(Request::Start);
        g.tick(Controls::default(), &t, &n, t.gravity, DT);
        (g, t, n)
    }

    const DT: f32 = 1.0 / 60.0;

    fn step(g: &mut Game, t: &Tune, n: &Ints, c: Controls, ticks: usize) {
        for _ in 0..ticks {
            g.tick(c, t, n, t.gravity * t.gravity_scale, DT);
        }
    }

    fn jump_press() -> Controls {
        Controls { jump: true, jump_pressed: true, ..Controls::default() }
    }

    #[test]
    fn variable_jump_height() {
        let (mut g, t, n) = game();
        step(&mut g, &t, &n, Controls::default(), 10);
        let y0 = g.hero.body.pos[1];
        // Held: a full jump.
        step(&mut g, &t, &n, jump_press(), 1);
        let mut apex = y0;
        for _ in 0..60 {
            step(&mut g, &t, &n, Controls { jump: true, ..Controls::default() }, 1);
            apex = apex.min(g.hero.body.pos[1]);
        }
        let full = y0 - apex;
        assert!((full - t.jump_speed * t.jump_speed / (2.0 * t.gravity)).abs() < 4.0, "{full}");
        // Tapped: cut short.
        step(&mut g, &t, &n, Controls::default(), 30);
        step(&mut g, &t, &n, jump_press(), 1);
        let mut apex2 = y0;
        for _ in 0..60 {
            step(&mut g, &t, &n, Controls::default(), 1);
            apex2 = apex2.min(g.hero.body.pos[1]);
        }
        let short = y0 - apex2;
        assert!(short < full * 0.5, "{short} vs {full}");
    }

    /// Walk off the right end of the first flat stretch.
    fn to_ledge(g: &mut Game, t: &Tune, n: &Ints) {
        // Teleport next to the first gap (column 27 is its last ground).
        let x = 27.0 * TILE + 4.0;
        g.hero.body.pos = [x, 12.0 * TILE];
        g.hero.prev = g.hero.body.pos;
        g.enemies.clear();
        step(g, t, n, Controls::default(), 3);
        assert!(g.hero.body.grounded);
    }

    #[test]
    fn coyote_time_allows_a_late_jump() {
        let (mut g, t, n) = game();
        to_ledge(&mut g, &t, &n);
        let walk = Controls { x: 1.0, ..Controls::default() };
        // Walk until just past the edge (not grounded).
        let mut ticks = 0;
        while g.hero.body.grounded {
            step(&mut g, &t, &n, walk, 1);
            ticks += 1;
            assert!(ticks < 120);
        }
        // Three ticks later (50 ms < 90 ms coyote) a jump still fires.
        step(&mut g, &t, &n, walk, 3);
        step(&mut g, &t, &n, Controls { x: 1.0, ..jump_press() }, 1);
        assert!(g.hero.body.vel[1] < -200.0, "{:?}", g.hero.body.vel);
        // After the window it does not.
        let (mut g, t, n) = game();
        to_ledge(&mut g, &t, &n);
        while g.hero.body.grounded {
            step(&mut g, &t, &n, walk, 1);
        }
        step(&mut g, &t, &n, walk, 8);
        step(&mut g, &t, &n, Controls { x: 1.0, ..jump_press() }, 1);
        assert!(g.hero.body.vel[1] > 0.0, "{:?}", g.hero.body.vel);
    }

    #[test]
    fn jump_buffering_fires_on_landing() {
        let (mut g, t, n) = game();
        step(&mut g, &t, &n, Controls::default(), 5);
        step(&mut g, &t, &n, jump_press(), 1);
        // Fall until about 5 ticks (83 ms < 120 ms buffer) from landing.
        let mut landed_at = None;
        let mut pressed_at = None;
        for i in 0..120 {
            let near = g.hero.body.vel[1] > 0.0 && g.level.start[1] - g.hero.body.pos[1] < 18.0;
            let c = if near && pressed_at.is_none() {
                pressed_at = Some(i);
                jump_press()
            } else {
                Controls { jump: pressed_at.is_some(), ..Controls::default() }
            };
            let was = g.hero.body.grounded;
            step(&mut g, &t, &n, c, 1);
            if !was && g.hero.body.grounded && landed_at.is_none() {
                landed_at = Some(i);
            }
            if landed_at.is_some() && g.hero.body.vel[1] < -200.0 {
                // Jumped again straight away without a new press.
                assert!(i - pressed_at.unwrap() <= 8, "{i} {pressed_at:?}");
                return;
            }
        }
        panic!("buffered jump never fired: pressed {pressed_at:?}, landed {landed_at:?}");
    }

    #[test]
    fn falling_in_a_gap_costs_a_life_and_respawns() {
        let (mut g, t, n) = game();
        to_ledge(&mut g, &t, &n);
        let lives = g.lives;
        step(&mut g, &t, &n, Controls { x: 1.0, ..Controls::default() }, 60);
        step(&mut g, &t, &n, Controls::default(), 120);
        assert_eq!(g.lives, lives - 1);
        assert!(g.hero.alive());
        assert_eq!(g.hero.body.pos, g.level.start);
        assert!(g.hero.invuln > 0.0);
    }

    #[test]
    fn out_of_lives_is_game_over_and_the_timer_costs_a_life() {
        let (mut g, t, n) = game();
        g.lives = 1;
        g.time_left = 0.05;
        step(&mut g, &t, &n, Controls::default(), 120);
        assert_eq!(g.phase, Phase::Lost);
        assert!(g.results.is_some_and(|r| !r.won));
    }

    #[test]
    fn stomping_a_beetle_bounces_and_scores() {
        let (mut g, t, n) = game();
        // Drop the hero onto the first beetle.
        let b = g.enemies.iter().position(|e| e.kind == EnemyKind::Beetle).unwrap();
        g.enemies[b].awake = true;
        let p = g.enemies[b].body.pos;
        g.hero.body.pos = [p[0], p[1] - 40.0];
        g.hero.body.grounded = false;
        g.hero.body.ground = None;
        g.hero.prev = g.hero.body.pos;
        let mut stomped = false;
        for _ in 0..40 {
            step(&mut g, &t, &n, Controls::default(), 1);
            if g.events.contains(&Event::Stomp) {
                stomped = true;
            }
            g.events.clear();
        }
        assert!(stomped);
        assert_eq!(g.score, n.score_stomp as u32);
        assert!(!g.enemies[b].alive());
        assert_eq!(g.lives, n.lives);
    }

    #[test]
    fn walking_into_an_enemy_hurts() {
        let (mut g, t, n) = game();
        let b = g.enemies.iter().position(|e| e.kind == EnemyKind::Beetle).unwrap();
        let lives = g.lives;
        g.hero.body.pos = [g.enemies[b].body.pos[0] - 30.0, g.enemies[b].body.pos[1]];
        step(&mut g, &t, &n, Controls { x: 1.0, ..Controls::default() }, 60);
        assert_eq!(g.lives, lives - 1);
    }

    /// A game in free roam, started the way the kit starts it.
    fn roam() -> (Game, Tune, Ints) {
        let (t, n) = (Tune::default(), Ints::default());
        let level = Rc::new(Level::from_json(LEVEL_JSON).unwrap());
        let mut g = Game::new(level, &t, &n);
        g.set_free_roam(true, &t, &n);
        g.tick(Controls::default(), &t, &n, t.gravity, DT);
        (g, t, n)
    }

    #[test]
    fn free_roam_starts_in_the_level_and_title_means_a_fresh_walk() {
        let (mut g, t, n) = roam();
        assert_eq!(g.phase, Phase::Playing);
        step(&mut g, &t, &n, Controls { x: 1.0, ..Controls::default() }, 60);
        assert!(g.hero.body.pos[0] > g.level.start[0] + 20.0);
        g.request(Request::Title);
        step(&mut g, &t, &n, Controls::default(), 1);
        assert_eq!(g.phase, Phase::Playing);
        assert_eq!(g.hero.body.pos[0], g.level.start[0]);
        // And Play mode still starts at the title.
        let level = Rc::new(Level::from_json(LEVEL_JSON).unwrap());
        assert_eq!(Game::new(level, &t, &n).phase, Phase::Title);
    }

    #[test]
    fn free_roam_a_pit_puts_the_hero_back_on_safe_ground_for_free() {
        let (mut g, _t, n) = fall_into_first_pit();
        assert!(g.free_respawns >= 1, "never fell in");
        assert_eq!(g.lives, n.lives);
        assert!(g.hero.alive() && g.hero.dying == 0.0);
        assert_eq!(g.phase, Phase::Playing);
        // Back on the ground before the gap, not at the level start.
        assert!(g.hero.body.grounded && g.hero.body.pos[0] > 20.0 * TILE && g.hero.body.pos[0] < 28.0 * TILE, "{:?}", g.hero.body.pos);
        assert!(g.hero.body.pos[1] <= 12.0 * TILE + 1.0, "{:?}", g.hero.body.pos);
        assert!(!g.take_fails().any());
    }

    /// Free roam: run from the level start, straight off the first pit's
    /// edge, and let the respawn happen (then settle for half a second).
    fn fall_into_first_pit() -> (Game, Tune, Ints) {
        let (mut g, t, n) = roam();
        g.enemies.clear();
        let run = Controls { x: 1.0, run: true, ..Controls::default() };
        let mut ticks = 0;
        while g.free_respawns == 0 {
            step(&mut g, &t, &n, run, 1);
            ticks += 1;
            assert!(ticks < 900, "never fell in: {:?}", g.hero.body.pos);
        }
        step(&mut g, &t, &n, Controls::default(), 30);
        (g, t, n)
    }

    #[test]
    fn free_roam_pit_respawn_is_a_stable_spot_away_from_the_edge() {
        // The pit spans columns 28..38. A respawn on its lip leaves no run-up
        // and one step from falling again: a trap. The spot has to be a
        // standing place with room on both sides.
        let (mut g, t, n) = fall_into_first_pit();
        let edge = 28.0 * TILE;
        let x = g.hero.body.pos[0];
        assert!(edge - x >= 2.0 * TILE, "respawned {} px from the pit edge: {:?}", edge - x, g.hero.body.pos);
        // Standing still: it stays put, with no second fall.
        let r = g.free_respawns;
        step(&mut g, &t, &n, Controls::default(), 240);
        assert_eq!(g.free_respawns, r, "fell again with no input");
        assert!(g.hero.body.grounded);
        assert!((g.hero.body.pos[0] - x).abs() < 1.0);
    }

    /// Run right for `run_ticks`, jump (held to full height), keep running in
    /// the air until back on the ground. Whether it landed and where.
    fn run_and_jump(g: &mut Game, t: &Tune, n: &Ints, run_ticks: usize) {
        let go = Controls { x: 1.0, run: true, ..Controls::default() };
        step(g, t, n, go, run_ticks);
        step(g, t, n, Controls { jump: true, jump_pressed: true, ..go }, 1);
        step(g, t, n, Controls { jump: true, ..go }, 30);
        for _ in 0..120 {
            if g.hero.body.grounded {
                break;
            }
            step(g, t, n, go, 1);
        }
    }

    #[test]
    fn free_roam_pit_can_be_cleared_from_the_respawn_spot() {
        // The pit is crossed by two wooden planks (the first at 160 px, the
        // second at 144 px, the far ground at 192 px). From the respawn spot
        // some run-up and jump timings reach the first plank, and from it
        // some reach the second plank or the far ground, with no further
        // respawn: the player is never stuck in a loop.
        let (base, t, n) = fall_into_first_pit();
        let r = base.free_respawns;
        let (mut first, mut across) = (0, 0);
        for run1 in (0..60).step_by(2) {
            let mut g = base.clone();
            run_and_jump(&mut g, &t, &n, run1);
            if g.free_respawns != r || !g.hero.body.grounded || (g.hero.body.pos[1] - 10.0 * TILE).abs() > 1.0 {
                continue;
            }
            first += 1;
            for run2 in (0..40).step_by(2) {
                let mut h = g.clone();
                run_and_jump(&mut h, &t, &n, run2);
                if h.free_respawns != r || !h.hero.body.grounded {
                    continue;
                }
                let p = h.hero.body.pos;
                if p[1] == 9.0 * TILE || p[0] > 38.0 * TILE {
                    across += 1;
                }
            }
        }
        assert!(first >= 2, "only {first} run-ups reached the first plank");
        assert!(across >= 2, "only {across} timings crossed the pit");
    }

    #[test]
    fn free_roam_enemies_cannot_hurt_and_stomps_still_work() {
        let (mut g, t, n) = roam();
        let b = g.enemies.iter().position(|e| e.kind == EnemyKind::Beetle).unwrap();
        g.hero.body.pos = [g.enemies[b].body.pos[0] - 30.0, g.enemies[b].body.pos[1]];
        step(&mut g, &t, &n, Controls { x: 1.0, ..Controls::default() }, 120);
        assert_eq!(g.lives, n.lives);
        assert!(g.hero.alive());
        assert_eq!(g.free_respawns, 0);
        assert!(!g.take_fails().any());
        // A stomp from above is still a stomp.
        let (mut g, t, n) = roam();
        let b = g.enemies.iter().position(|e| e.kind == EnemyKind::Beetle).unwrap();
        g.enemies[b].awake = true;
        let p = g.enemies[b].body.pos;
        g.hero.body.pos = [p[0], p[1] - 40.0];
        g.hero.body.grounded = false;
        g.hero.body.ground = None;
        g.hero.prev = g.hero.body.pos;
        step(&mut g, &t, &n, Controls::default(), 40);
        assert_eq!(g.stomps, 1);
    }

    #[test]
    fn free_roam_has_no_timer_and_the_gate_does_not_end_the_run() {
        let (mut g, t, n) = roam();
        g.time_left = 0.05;
        step(&mut g, &t, &n, Controls::default(), 180);
        assert_eq!(g.time_left, 0.05);
        assert_eq!(g.phase, Phase::Playing);
        assert!(g.hero.alive());
        let goal = g.level.goal;
        g.hero.body.pos = goal;
        g.hero.prev = goal;
        step(&mut g, &t, &n, Controls::default(), 240);
        assert_eq!(g.phase, Phase::Playing);
        assert!(g.results.is_none());
        assert!(!g.take_fails().any());
    }

    #[test]
    fn free_roam_a_checkpoint_is_the_new_safe_spot() {
        let (mut g, t, n) = roam();
        let f = g.level.checkpoints[0];
        g.hero.body.pos = f;
        g.hero.prev = f;
        step(&mut g, &t, &n, Controls::default(), 3);
        assert!(g.flags[0]);
        // Dropped off the map: back at the flag.
        g.hero.body.pos = [f[0], g.level.height_px() + 40.0];
        step(&mut g, &t, &n, Controls::default(), 2);
        assert_eq!(g.free_respawns, 1);
        assert!((g.hero.body.pos[0] - f[0]).abs() < 24.0 && g.hero.body.pos[1] <= f[1] + 1.0, "{:?} vs {f:?}", g.hero.body.pos);
    }

    #[test]
    fn play_mode_counts_every_fail_moment() {
        // A pit: one life.
        let (mut g, t, n) = game();
        to_ledge(&mut g, &t, &n);
        step(&mut g, &t, &n, Controls { x: 1.0, ..Controls::default() }, 60);
        step(&mut g, &t, &n, Controls::default(), 120);
        let f = g.take_fails();
        assert_eq!(f, Fails { lives_lost: 1, timers_expired: 0, game_overs: 0 });
        assert!(!g.take_fails().any(), "taken once");
        // The timer: it expires, costs a life, and out of lives is game over.
        let (mut g, t, n) = game();
        g.lives = 1;
        g.time_left = 0.05;
        step(&mut g, &t, &n, Controls::default(), 120);
        assert_eq!(g.take_fails(), Fails { lives_lost: 1, timers_expired: 1, game_overs: 1 });
        // The gate ends the run too.
        let (mut g, t, n) = game();
        let goal = g.level.goal;
        g.hero.body.pos = goal;
        g.hero.prev = goal;
        step(&mut g, &t, &n, Controls::default(), 3);
        assert_eq!(g.phase, Phase::Won);
        assert_eq!(g.take_fails(), Fails { lives_lost: 0, timers_expired: 0, game_overs: 1 });
    }

    #[test]
    fn coins_and_lives_add_up() {
        let (mut g, t, mut n) = game();
        n.coins_per_life = 2;
        let lives = g.lives;
        // The coin row by the start, at walking height (columns 6-9, row
        // 11, on the ground of row 12).
        g.hero.body.pos = [6.5 * TILE, 12.0 * TILE];
        step(&mut g, &t, &n, Controls::default(), 1);
        g.hero.body.pos = [7.5 * TILE, 12.0 * TILE];
        step(&mut g, &t, &n, Controls::default(), 1);
        assert_eq!(g.coins, 2);
        assert_eq!(g.score, 2 * n.score_coin as u32);
        assert_eq!(g.lives, lives + 1);
    }

    #[test]
    fn pause_freezes_the_game() {
        let (mut g, t, n) = game();
        g.request(Request::TogglePause);
        let before = g.time_left;
        step(&mut g, &t, &n, Controls { x: 1.0, ..Controls::default() }, 30);
        assert_eq!(g.phase, Phase::Paused);
        assert_eq!(g.time_left, before);
        g.request(Request::Pause(false));
        step(&mut g, &t, &n, Controls::default(), 2);
        assert_eq!(g.phase, Phase::Playing);
    }
}
