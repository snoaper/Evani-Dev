//! The rules of the game on plain data, stepped at a fixed 60 Hz. No
//! rendering, no clock and no randomness except the seeded [`Pcg32`] the
//! game holds. The player's game lives in the ECS (see `ecs`): each step
//! starts from the entities and writes every change back.

use std::rc::Rc;

use evani_core::rng::Pcg32;

use crate::level::{Cell, EnemyKind, Level, LinkKind, PickupKind, ROOM_PX_H, ROOM_PX_W, Side, TILE};
use crate::{Ints, Tune};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Title,
    Playing,
    Paused,
    Won,
    Lost,
}

/// One step of input, read from the actions by the sim system.
#[derive(Debug, Clone, Copy, Default)]
pub struct Controls {
    pub x: f32,
    pub y: f32,
    pub attack_pressed: bool,
    pub item_pressed: bool,
    pub pause_pressed: bool,
    /// A seed drawn from the app's seeded `Pcg32` resource for a new run.
    pub seed: u64,
}

/// What the page or a test asks the game to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Start,
    TogglePause,
    ToTitle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Up,
    Down,
    Left,
    Right,
}

impl Facing {
    pub fn dir(self) -> [f32; 2] {
        match self {
            Facing::Up => [0.0, -1.0],
            Facing::Down => [0.0, 1.0],
            Facing::Left => [-1.0, 0.0],
            Facing::Right => [1.0, 0.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player {
    pub pos: [f32; 2],
    pub prev: [f32; 2],
    pub facing: Facing,
    pub hp: i32,
    pub max_hp: i32,
    pub keys: i32,
    pub gems: i32,
    pub has_wand: bool,
    /// Seconds of the current sword swing left (0: not swinging).
    pub swing: f32,
    pub swing_id: u32,
    pub cooldown: f32,
    pub bolt_cd: f32,
    pub invuln: f32,
    pub grace: f32,
    pub knock: [f32; 2],
    pub knock_t: f32,
    pub room: u16,
    /// Where a free-roam respawn puts the hero.
    pub entry: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EState {
    Idle,
    WindUp,
    Charge,
    Rest,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub room: u16,
    pub pos: [f32; 2],
    pub hp: i32,
    pub state: EState,
    pub timer: f32,
    pub dir: [f32; 2],
    pub flash: f32,
    pub knock: [f32; 2],
    pub knock_t: f32,
    pub last_hit: u32,
    pub attacks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pickup {
    pub kind: PickupKind,
    pub room: u16,
    pub pos: [f32; 2],
    /// A hidden pickup (the Dawn Shard before the Warden falls) is neither
    /// drawn nor taken.
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub room: u16,
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub friendly: bool,
    pub life: f32,
}

/// A doorway's state (one per level link).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Door {
    pub link: u16,
    pub open: bool,
}

/// A room switch's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Latch {
    pub room: u16,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopKind {
    Key,
    Heart,
    Gem,
    Container,
    Wand,
    Locked,
    Open,
    Gate,
}

impl PopKind {
    pub fn text(self) -> &'static str {
        match self {
            PopKind::Key => "KEY",
            PopKind::Heart => "+1",
            PopKind::Gem => "GEM",
            PopKind::Container => "MAX+",
            PopKind::Wand => "WAND",
            PopKind::Locked => "LOCKED",
            PopKind::Open => "OPEN",
            PopKind::Gate => "GATE",
        }
    }
}

/// Short floating text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pop {
    pub kind: PopKind,
    pub pos: [f32; 2],
    pub t: f32,
}

/// Game overs reported since the last [`Game::take_fails`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fails {
    pub game_overs: u32,
}

pub struct Game {
    pub level: Rc<Level>,
    pub phase: Phase,
    pub player: Player,
    pub enemies: Vec<Enemy>,
    pub pickups: Vec<Pickup>,
    pub shots: Vec<Shot>,
    pub doors: Vec<Door>,
    pub latches: Vec<Latch>,
    pub pops: Vec<Pop>,
    pub visited: Vec<bool>,
    pub rng: Pcg32,
    pub tick: u64,
    pub free_roam: bool,
    pub free_respawns: u32,
    pub stop: f32,
    pub trauma: f32,
    pub end_wait: f32,
    /// The vectors above are newer than the ECS entities (a reset or a new
    /// game): the next sim step must not gather from them.
    pub dirty: bool,
    /// The Warden's full health and its fan size, from the tunables at reset.
    pub warden_max_hp: i32,
    pub fan_count: i32,
    pending: Option<Request>,
    fails: Fails,
}

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

fn norm(v: [f32; 2]) -> [f32; 2] {
    let l = len(v);
    if l < 1e-4 { [0.0, 0.0] } else { [v[0] / l, v[1] / l] }
}

/// A triangle wave in -1..1 with period 4 (no trig: gameplay stays
/// bit-identical on every platform).
fn tri(t: f32) -> f32 {
    let m = t - 4.0 * (t / 4.0).floor();
    (m - 2.0).abs() - 1.0
}

fn overlap(a: [f32; 2], ah: [f32; 2], b: [f32; 2], bh: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < ah[0] + bh[0] && (a[1] - b[1]).abs() < ah[1] + bh[1]
}

/// Cos and sin of one fan step (0.3 rad), fixed so no trig runs in the sim.
const FAN_COS: f32 = 0.955_336_5;
const FAN_SIN: f32 = 0.295_520_2;

fn rotate(v: [f32; 2], c: f32, s: f32) -> [f32; 2] {
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

/// What a body step ran into.
#[derive(Debug, Clone, Copy, Default)]
struct Hit {
    locked: Option<usize>,
    blocked: bool,
}

impl Game {
    pub fn new(level: Rc<Level>, t: &Tune, n: &Ints) -> Self {
        let player = Self::fresh_player(&level, n);
        let mut g = Self {
            visited: vec![false; level.rooms.len()],
            level,
            phase: Phase::Title,
            player,
            enemies: Vec::new(),
            pickups: Vec::new(),
            shots: Vec::new(),
            doors: Vec::new(),
            latches: Vec::new(),
            pops: Vec::new(),
            rng: Pcg32::new(1, 1),
            tick: 0,
            free_roam: false,
            free_respawns: 0,
            stop: 0.0,
            trauma: 0.0,
            end_wait: 0.0,
            dirty: true,
            warden_max_hp: n.warden_hp,
            fan_count: n.warden_fan_count,
            pending: None,
            fails: Fails::default(),
        };
        g.reset(1, t, n);
        g
    }

    fn fresh_player(level: &Level, n: &Ints) -> Player {
        Player {
            pos: level.start_pos,
            prev: level.start_pos,
            facing: Facing::Down,
            hp: n.max_hp,
            max_hp: n.max_hp,
            keys: 0,
            gems: 0,
            has_wand: false,
            swing: 0.0,
            swing_id: 0,
            cooldown: 0.0,
            bolt_cd: 0.0,
            invuln: 0.0,
            grace: 0.0,
            knock: [0.0; 2],
            knock_t: 0.0,
            room: level.start_room as u16,
            entry: level.start_pos,
        }
    }

    pub fn enemy_hp(kind: EnemyKind, n: &Ints) -> i32 {
        match kind {
            EnemyKind::Slime => n.slime_hp,
            EnemyKind::Bat => n.bat_hp,
            EnemyKind::Husk => n.husk_hp,
            EnemyKind::Spitter => n.spitter_hp,
            EnemyKind::Warden => n.warden_hp,
        }
    }

    /// Rebuild the world from the level data for a new run.
    pub fn reset(&mut self, seed: u64, t: &Tune, n: &Ints) {
        self.rng = Pcg32::new(seed, 1);
        self.warden_max_hp = n.warden_hp;
        self.fan_count = n.warden_fan_count;
        let level = Rc::clone(&self.level);
        self.player = Self::fresh_player(&level, n);
        self.enemies.clear();
        self.pickups.clear();
        for (ri, r) in level.rooms.iter().enumerate() {
            for (k, s) in r.enemies.iter().enumerate() {
                let timer = match s.kind {
                    EnemyKind::Spitter => t.spitter_period_s * (0.5 + 0.25 * (k % 3) as f32),
                    EnemyKind::Warden => t.warden_period_s,
                    _ => 0.0,
                };
                let dir = if s.kind == EnemyKind::Husk { [1.0, 0.0] } else { [0.0, 0.0] };
                self.enemies.push(Enemy {
                    kind: s.kind,
                    room: ri as u16,
                    pos: s.pos,
                    hp: Self::enemy_hp(s.kind, n),
                    state: EState::Idle,
                    timer,
                    dir,
                    flash: 0.0,
                    knock: [0.0; 2],
                    knock_t: 0.0,
                    last_hit: 0,
                    attacks: 0,
                });
            }
            for s in &r.pickups {
                // Free roam has no goal: the Dawn Shard is not placed.
                if s.kind == PickupKind::Crystal && self.free_roam {
                    continue;
                }
                self.pickups.push(Pickup { kind: s.kind, room: ri as u16, pos: s.pos, active: s.kind != PickupKind::Crystal });
            }
        }
        self.shots.clear();
        self.pops.clear();
        self.doors = level.links.iter().enumerate().map(|(i, l)| Door { link: i as u16, open: l.kind == LinkKind::Open }).collect();
        self.latches = level.rooms.iter().enumerate().filter(|(_, r)| r.has_switch).map(|(i, _)| Latch { room: i as u16, on: false }).collect();
        self.visited = vec![false; level.rooms.len()];
        self.visited[level.start_room] = true;
        self.stop = 0.0;
        self.trauma = 0.0;
        self.dirty = true;
    }

    /// Free roam: straight into the dungeon, no Dawn Shard, no lost state.
    pub fn set_free_roam(&mut self, on: bool, t: &Tune, n: &Ints) {
        self.free_roam = on;
        self.reset(1, t, n);
        self.phase = if on { Phase::Playing } else { Phase::Title };
    }

    pub fn request(&mut self, r: Request) {
        self.pending = Some(r);
    }

    pub fn take_fails(&mut self) -> Fails {
        core::mem::take(&mut self.fails)
    }

    pub fn latched(&self, room: usize) -> bool {
        self.latches.iter().any(|l| l.room as usize == room && l.on)
    }

    /// The room the player is in.
    pub fn room(&self) -> usize {
        self.player.room as usize
    }

    fn cell_blocked(&self, room: usize, tx: i32, ty: i32, walker: bool, hit: &mut Hit) -> bool {
        match self.level.cell(room, tx, ty) {
            Cell::Floor | Cell::Switch => false,
            Cell::Wall => true,
            Cell::Pit => walker,
            Cell::Gate => !self.latched(room),
            Cell::Door(l) => {
                if self.doors.get(l).is_some_and(|d| d.open) {
                    false
                } else {
                    hit.locked = Some(l);
                    true
                }
            }
        }
    }

    fn box_blocked(&self, room: usize, c: [f32; 2], half: [f32; 2], walker: bool, exit: bool, hit: &mut Hit) -> bool {
        let (x0, x1) = (((c[0] - half[0]) / TILE).floor() as i32, ((c[0] + half[0] - 0.01) / TILE).floor() as i32);
        let (y0, y1) = (((c[1] - half[1]) / TILE).floor() as i32, ((c[1] + half[1] - 0.01) / TILE).floor() as i32);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let outside = tx < 0 || ty < 0 || tx >= crate::level::ROOM_W as i32 || ty >= crate::level::ROOM_H as i32;
                if outside && !exit {
                    return true;
                }
                if self.cell_blocked(room, tx, ty, walker, hit) {
                    return true;
                }
            }
        }
        false
    }

    /// Move a box by `delta`, one axis at a time; a blocked axis stays put.
    fn move_body(&self, room: usize, pos: [f32; 2], half: [f32; 2], delta: [f32; 2], walker: bool, exit: bool) -> ([f32; 2], Hit) {
        let mut hit = Hit::default();
        let mut p = pos;
        let nx = [p[0] + delta[0], p[1]];
        if self.box_blocked(room, nx, half, walker, exit, &mut hit) {
            if delta[0] != 0.0 {
                hit.blocked = true;
            }
        } else {
            p = nx;
        }
        let ny = [p[0], p[1] + delta[1]];
        if self.box_blocked(room, ny, half, walker, exit, &mut hit) {
            if delta[1] != 0.0 {
                hit.blocked = true;
            }
        } else {
            p = ny;
        }
        (p, hit)
    }

    fn pop(&mut self, kind: PopKind, pos: [f32; 2], t: &Tune) {
        let _ = t;
        // At most a few at once: drop the oldest.
        if self.pops.len() >= 4 {
            self.pops.remove(0);
        }
        self.pops.push(Pop { kind, pos, t: 0.0 });
    }

    /// One fixed step.
    pub fn tick(&mut self, c: Controls, t: &Tune, n: &Ints, dt: f32) {
        self.tick += 1;
        match self.pending.take() {
            Some(Request::Start) => {
                self.reset(c.seed, t, n);
                self.phase = Phase::Playing;
            }
            Some(Request::TogglePause) => match self.phase {
                Phase::Playing => self.phase = Phase::Paused,
                Phase::Paused => self.phase = Phase::Playing,
                _ => {}
            },
            Some(Request::ToTitle) => {
                self.reset(c.seed, t, n);
                self.phase = if self.free_roam { Phase::Playing } else { Phase::Title };
            }
            None => {}
        }
        match self.phase {
            Phase::Title => {
                if c.attack_pressed {
                    self.request(Request::Start);
                }
            }
            Phase::Paused => {
                if c.pause_pressed {
                    self.request(Request::TogglePause);
                }
            }
            Phase::Won | Phase::Lost => {
                self.end_wait -= dt;
                if self.end_wait <= 0.0 && c.attack_pressed {
                    self.request(Request::ToTitle);
                }
            }
            Phase::Playing => {
                if c.pause_pressed {
                    self.request(Request::TogglePause);
                } else {
                    self.step_play(c, t, n, dt);
                }
            }
        }
    }

    fn step_play(&mut self, c: Controls, t: &Tune, n: &Ints, dt: f32) {
        self.trauma = (self.trauma - t.shake_decay * dt).max(0.0);
        for p in &mut self.pops {
            p.t += dt;
        }
        self.pops.retain(|p| p.t < t.pop_life_s);
        if self.stop > 0.0 {
            self.stop -= dt;
            return;
        }
        self.step_player(c, t, n, dt);
        let room = self.room();
        self.step_enemies(room, t, n, dt);
        self.step_shots(room, t, n, dt);
        self.sword_hits(room, t, n);
        self.contact_hits(room, t, n);
        self.take_pickups(room, t, n);
        self.reap(t);
        self.check_end(n);
    }

    fn hero_half(t: &Tune) -> [f32; 2] {
        [t.hero_w * 0.5, t.hero_h * 0.5]
    }

    fn step_player(&mut self, c: Controls, t: &Tune, n: &Ints, dt: f32) {
        let half = Self::hero_half(t);
        let room = self.room();
        {
            let p = &mut self.player;
            p.prev = p.pos;
            p.invuln = (p.invuln - dt).max(0.0);
            p.grace = (p.grace - dt).max(0.0);
            p.cooldown = (p.cooldown - dt).max(0.0);
            p.bolt_cd = (p.bolt_cd - dt).max(0.0);
            p.swing = (p.swing - dt).max(0.0);
        }
        // Movement: a knockback overrides the controls while it lasts.
        let delta = if self.player.knock_t > 0.0 {
            self.player.knock_t -= dt;
            [self.player.knock[0] * dt, self.player.knock[1] * dt]
        } else {
            let mut v = [c.x.clamp(-1.0, 1.0), c.y.clamp(-1.0, 1.0)];
            let l = len(v);
            if l > 1.0 {
                v = [v[0] / l, v[1] / l];
            }
            if l > 0.05 {
                self.player.facing = if v[0].abs() >= v[1].abs() {
                    if v[0] < 0.0 { Facing::Left } else { Facing::Right }
                } else if v[1] < 0.0 {
                    Facing::Up
                } else {
                    Facing::Down
                };
            }
            [v[0] * t.walk_speed * dt, v[1] * t.walk_speed * dt]
        };
        let (np, hit) = self.move_body(room, self.player.pos, half, delta, true, true);
        self.player.pos = np;
        // Pushing against a locked door with a key opens it.
        if let Some(l) = hit.locked
            && (delta[0] != 0.0 || delta[1] != 0.0)
        {
            let locked = self.doors.get(l).is_some_and(|d| !d.open);
            if locked {
                let at = self.player.pos;
                if self.player.keys > 0 {
                    self.player.keys -= 1;
                    self.doors[l].open = true;
                    self.pop(PopKind::Open, at, t);
                } else if self.tick.is_multiple_of(30) {
                    self.pop(PopKind::Locked, at, t);
                }
            }
        }
        self.change_room(t);
        // Attack and item.
        if c.attack_pressed && self.player.cooldown <= 0.0 && self.player.swing <= 0.0 {
            self.player.swing = t.sword_time_s;
            self.player.cooldown = t.sword_time_s + t.sword_cooldown_s;
            self.player.swing_id += 1;
        }
        if c.item_pressed && self.player.has_wand && self.player.bolt_cd <= 0.0 {
            self.player.bolt_cd = t.bolt_cooldown_s;
            let d = self.player.facing.dir();
            let at = [self.player.pos[0] + d[0] * 8.0, self.player.pos[1] + d[1] * 8.0];
            self.shots.push(Shot { room: self.player.room, pos: at, vel: [d[0] * t.bolt_speed, d[1] * t.bolt_speed], friendly: true, life: t.bolt_life_s });
        }
        // A floor switch latches its room's gate.
        let (tx, ty) = ((self.player.pos[0] / TILE).floor() as i32, (self.player.pos[1] / TILE).floor() as i32);
        if self.level.cell(room, tx, ty) == Cell::Switch {
            self.latch(room, t);
        }
        let _ = n;
    }

    fn latch(&mut self, room: usize, t: &Tune) {
        let mut fresh = false;
        for l in &mut self.latches {
            if l.room as usize == room && !l.on {
                l.on = true;
                fresh = true;
            }
        }
        if fresh {
            self.pop(PopKind::Gate, [ROOM_PX_W * 0.5, ROOM_PX_H * 0.5], t);
        }
    }

    /// Step through a doorway: the hero lands inside the neighbour.
    fn change_room(&mut self, t: &Tune) {
        let p = self.player.pos;
        let side = if p[0] < 0.0 {
            Some(Side::W)
        } else if p[0] > ROOM_PX_W {
            Some(Side::E)
        } else if p[1] < 0.0 {
            Some(Side::N)
        } else if p[1] > ROOM_PX_H {
            Some(Side::S)
        } else {
            None
        };
        let Some(side) = side else { return };
        let Some((to, _)) = self.level.neighbour(self.room(), side) else {
            return;
        };
        let (dx, dy) = side.delta();
        let np = [p[0] - dx as f32 * ROOM_PX_W, p[1] - dy as f32 * ROOM_PX_H];
        self.player.pos = np;
        self.player.prev = np;
        self.player.room = to as u16;
        self.player.grace = t.enter_grace_s;
        // Free-roam respawns put the hero a little way inside the doorway.
        self.player.entry = [np[0] - dx as f32 * 20.0, np[1] - dy as f32 * 20.0];
        self.visited[to] = true;
        self.shots.clear();
        self.pops.clear();
    }

    fn step_enemies(&mut self, room: usize, t: &Tune, n: &Ints, dt: f32) {
        for i in 0..self.enemies.len() {
            if self.enemies[i].room as usize != room || self.enemies[i].hp <= 0 {
                continue;
            }
            let mut e = self.enemies[i];
            e.flash = (e.flash - dt).max(0.0);
            if e.knock_t > 0.0 {
                e.knock_t -= dt;
                let h = [e.kind.half(); 2];
                let (np, _) = self.move_body(room, e.pos, h, [e.knock[0] * dt, e.knock[1] * dt], !e.kind.flies(), false);
                e.pos = np;
            } else {
                self.ai(&mut e, room, t, n, dt);
            }
            self.enemies[i] = e;
        }
    }

    fn walk(&self, e: &mut Enemy, room: usize, v: [f32; 2], dt: f32) -> bool {
        let h = [e.kind.half(); 2];
        let (np, hit) = self.move_body(room, e.pos, h, [v[0] * dt, v[1] * dt], !e.kind.flies(), false);
        e.pos = np;
        hit.blocked
    }

    fn ai(&mut self, e: &mut Enemy, room: usize, t: &Tune, _n: &Ints, dt: f32) {
        let pp = self.player.pos;
        let to = [pp[0] - e.pos[0], pp[1] - e.pos[1]];
        match e.kind {
            EnemyKind::Slime => {
                e.timer -= dt;
                if e.timer <= 0.0 {
                    e.dir = match self.rng.below(5) {
                        0 => [0.0, 0.0],
                        1 => [1.0, 0.0],
                        2 => [-1.0, 0.0],
                        3 => [0.0, 1.0],
                        _ => [0.0, -1.0],
                    };
                    e.timer = t.slime_turn_s * (0.6 + 0.8 * self.rng.next_f32());
                }
                let v = [e.dir[0] * t.slime_speed, e.dir[1] * t.slime_speed];
                if self.walk(e, room, v, dt) {
                    e.timer = 0.0;
                }
            }
            EnemyKind::Bat => {
                e.timer -= dt;
                if len(to) < t.bat_sight {
                    // Home in with a sideways wobble.
                    let d = norm(to);
                    let w = tri(self.tick as f32 * 0.07 + e.pos[0] * 0.05) * 0.8;
                    e.dir = norm([d[0] - d[1] * w, d[1] + d[0] * w]);
                } else if e.timer <= 0.0 {
                    let a = self.rng.next_f32() * 4.0;
                    e.dir = norm([tri(a), tri(a + 1.0)]);
                    e.timer = 0.5 + 0.5 * self.rng.next_f32();
                }
                let v = [e.dir[0] * t.bat_speed, e.dir[1] * t.bat_speed];
                if self.walk(e, room, v, dt) {
                    e.dir = [-e.dir[0], -e.dir[1]];
                }
            }
            EnemyKind::Husk => match e.state {
                EState::Idle => {
                    let v = [e.dir[0] * t.husk_speed, e.dir[1] * t.husk_speed];
                    if self.walk(e, room, v, dt) {
                        e.dir = [-e.dir[0], -e.dir[1]];
                    }
                    let (ax, ay) = (to[0].abs(), to[1].abs());
                    if (ay < 10.0 && ax < t.husk_sight) || (ax < 10.0 && ay < t.husk_sight) {
                        e.dir = if ax >= ay { [to[0].signum(), 0.0] } else { [0.0, to[1].signum()] };
                        e.state = EState::WindUp;
                        e.timer = t.husk_windup_s;
                    }
                }
                EState::WindUp => {
                    e.timer -= dt;
                    if e.timer <= 0.0 {
                        e.state = EState::Charge;
                        e.timer = t.husk_charge_s;
                    }
                }
                EState::Charge => {
                    e.timer -= dt;
                    let v = [e.dir[0] * t.husk_charge_speed, e.dir[1] * t.husk_charge_speed];
                    if self.walk(e, room, v, dt) || e.timer <= 0.0 {
                        e.state = EState::Rest;
                        e.timer = t.husk_rest_s;
                    }
                }
                EState::Rest => {
                    e.timer -= dt;
                    if e.timer <= 0.0 {
                        e.state = EState::Idle;
                    }
                }
            },
            EnemyKind::Spitter => match e.state {
                EState::WindUp => {
                    e.timer -= dt;
                    if e.timer <= 0.0 {
                        let d = norm(to);
                        self.shots.push(Shot {
                            room: room as u16,
                            pos: e.pos,
                            vel: [d[0] * t.spit_speed, d[1] * t.spit_speed],
                            friendly: false,
                            life: 4.0,
                        });
                        e.state = EState::Idle;
                        e.timer = t.spitter_period_s;
                    }
                }
                _ => {
                    e.timer -= dt;
                    if e.timer <= 0.0 {
                        e.state = EState::WindUp;
                        e.timer = t.spitter_windup_s;
                    }
                }
            },
            EnemyKind::Warden => self.warden(e, room, t, dt, to),
        }
    }

    fn warden(&mut self, e: &mut Enemy, room: usize, t: &Tune, dt: f32, to: [f32; 2]) {
        let enraged = e.hp * 2 <= self.warden_max_hp;
        match e.state {
            EState::Idle => {
                e.timer -= dt;
                let d = norm(to);
                self.walk(e, room, [d[0] * t.warden_speed, d[1] * t.warden_speed], dt);
                if e.timer <= 0.0 {
                    e.state = EState::WindUp;
                    e.timer = t.warden_windup_s;
                    e.dir = norm(to);
                }
            }
            EState::WindUp => {
                e.timer -= dt;
                e.dir = norm(to);
                if e.timer <= 0.0 {
                    e.attacks += 1;
                    if enraged && e.attacks.is_multiple_of(2) {
                        // A fan of seeds towards the hero.
                        let count = self.fan_count.max(1);
                        let mut shots = vec![e.dir];
                        let (mut l, mut r) = (e.dir, e.dir);
                        for _ in 0..(count / 2) {
                            l = rotate(l, FAN_COS, FAN_SIN);
                            r = rotate(r, FAN_COS, -FAN_SIN);
                            shots.push(l);
                            shots.push(r);
                        }
                        for d in shots {
                            self.shots.push(Shot {
                                room: room as u16,
                                pos: e.pos,
                                vel: [d[0] * t.warden_fan_speed, d[1] * t.warden_fan_speed],
                                friendly: false,
                                life: 4.0,
                            });
                        }
                        e.state = EState::Rest;
                        e.timer = 0.7;
                    } else {
                        e.state = EState::Charge;
                        e.timer = t.warden_dash_s;
                    }
                }
            }
            EState::Charge => {
                e.timer -= dt;
                let v = [e.dir[0] * t.warden_dash_speed, e.dir[1] * t.warden_dash_speed];
                if self.walk(e, room, v, dt) || e.timer <= 0.0 {
                    e.state = EState::Rest;
                    e.timer = 0.6;
                }
            }
            EState::Rest => {
                e.timer -= dt;
                if e.timer <= 0.0 {
                    e.state = EState::Idle;
                    e.timer = t.warden_period_s;
                }
            }
        }
    }

    fn step_shots(&mut self, room: usize, t: &Tune, n: &Ints, dt: f32) {
        let half = Self::hero_half(t);
        let mut shots = core::mem::take(&mut self.shots);
        let mut keep = Vec::with_capacity(shots.len());
        for mut s in shots.drain(..) {
            if s.room as usize != room {
                continue;
            }
            s.life -= dt;
            s.pos = [s.pos[0] + s.vel[0] * dt, s.pos[1] + s.vel[1] * dt];
            if s.life <= 0.0 {
                continue;
            }
            let (tx, ty) = ((s.pos[0] / TILE).floor() as i32, (s.pos[1] / TILE).floor() as i32);
            let mut gone = false;
            match self.level.cell(room, tx, ty) {
                Cell::Wall => gone = true,
                Cell::Gate => gone = !self.latched(room),
                Cell::Door(l) => gone = !self.doors.get(l).is_some_and(|d| d.open),
                Cell::Switch => {
                    if s.friendly {
                        self.latch(room, t);
                        gone = true;
                    }
                }
                Cell::Floor | Cell::Pit => {}
            }
            if gone {
                continue;
            }
            if s.friendly {
                let dir = norm(s.vel);
                let mut hit_enemy = false;
                for i in 0..self.enemies.len() {
                    let e = self.enemies[i];
                    if e.room as usize == room && e.hp > 0 && overlap(s.pos, [2.0, 2.0], e.pos, [e.kind.half(); 2]) {
                        self.hurt_enemy(i, n.bolt_damage, dir, t);
                        hit_enemy = true;
                        break;
                    }
                }
                if hit_enemy {
                    continue;
                }
            } else if overlap(s.pos, [2.0, 2.0], self.player.pos, half) {
                let from = [s.pos[0] - s.vel[0], s.pos[1] - s.vel[1]];
                self.hurt_player(n.shot_damage, from, t);
                continue;
            }
            keep.push(s);
        }
        self.shots = keep;
    }

    fn hurt_enemy(&mut self, i: usize, dmg: i32, dir: [f32; 2], t: &Tune) {
        let e = &mut self.enemies[i];
        e.hp -= dmg;
        e.flash = 0.15;
        let k = if e.kind == EnemyKind::Warden { 0.25 } else { 1.0 };
        e.knock = [dir[0] * t.knockback_speed * k, dir[1] * t.knockback_speed * k];
        e.knock_t = t.knockback_s;
        self.stop = t.hit_stop_s;
    }

    fn hurt_player(&mut self, dmg: i32, from: [f32; 2], t: &Tune) {
        let p = &mut self.player;
        if p.invuln > 0.0 || p.grace > 0.0 || dmg <= 0 {
            return;
        }
        p.hp -= dmg;
        p.invuln = t.hurt_invuln_s;
        let d = norm([p.pos[0] - from[0], p.pos[1] - from[1]]);
        p.knock = [d[0] * t.hero_knockback_speed, d[1] * t.hero_knockback_speed];
        p.knock_t = t.knockback_s;
        self.trauma = (self.trauma + 0.6).min(1.0);
        self.stop = t.hit_stop_s;
    }

    fn sword_hits(&mut self, room: usize, t: &Tune, n: &Ints) {
        if self.player.swing <= 0.0 {
            return;
        }
        let d = self.player.facing.dir();
        let hh = Self::hero_half(t);
        let reach = t.sword_reach;
        let along = hh[0] + reach * 0.5;
        let centre = [self.player.pos[0] + d[0] * along, self.player.pos[1] + d[1] * along];
        let half = if d[0] != 0.0 { [reach * 0.5, t.sword_width * 0.5] } else { [t.sword_width * 0.5, reach * 0.5] };
        let id = self.player.swing_id;
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            if e.room as usize == room && e.hp > 0 && e.last_hit != id && overlap(centre, half, e.pos, [e.kind.half(); 2]) {
                self.enemies[i].last_hit = id;
                self.hurt_enemy(i, n.sword_damage, d, t);
            }
        }
    }

    fn contact_hits(&mut self, room: usize, t: &Tune, n: &Ints) {
        let half = Self::hero_half(t);
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            if e.room as usize != room || e.hp <= 0 || e.kind == EnemyKind::Spitter {
                continue;
            }
            if overlap(self.player.pos, half, e.pos, [e.kind.half() - 1.0; 2]) {
                let dmg = if e.kind == EnemyKind::Warden { n.warden_damage } else { n.contact_damage };
                self.hurt_player(dmg, e.pos, t);
            }
        }
    }

    fn take_pickups(&mut self, room: usize, t: &Tune, n: &Ints) {
        let half = Self::hero_half(t);
        let mut i = 0;
        while i < self.pickups.len() {
            let p = self.pickups[i];
            if p.room as usize != room || !p.active || !overlap(self.player.pos, half, p.pos, [5.0, 5.0]) {
                i += 1;
                continue;
            }
            let at = [p.pos[0], p.pos[1] - 8.0];
            let take = match p.kind {
                PickupKind::Heart => {
                    if self.player.hp < self.player.max_hp {
                        self.player.hp = (self.player.hp + n.heal_hp).min(self.player.max_hp);
                        self.pop(PopKind::Heart, at, t);
                        true
                    } else {
                        false
                    }
                }
                PickupKind::Key => {
                    self.player.keys += 1;
                    self.pop(PopKind::Key, at, t);
                    true
                }
                PickupKind::Gem => {
                    self.player.gems += 1;
                    self.pop(PopKind::Gem, at, t);
                    true
                }
                PickupKind::Container => {
                    self.player.max_hp += n.container_hp;
                    self.player.hp = self.player.max_hp;
                    self.pop(PopKind::Container, at, t);
                    true
                }
                PickupKind::Wand => {
                    self.player.has_wand = true;
                    self.pop(PopKind::Wand, at, t);
                    true
                }
                PickupKind::Crystal => {
                    if !self.free_roam {
                        self.phase = Phase::Won;
                        self.end_wait = 1.0;
                    }
                    true
                }
            };
            if take {
                self.pickups.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Remove dead enemies and roll their drops.
    fn reap(&mut self, t: &Tune) {
        let mut i = 0;
        while i < self.enemies.len() {
            if self.enemies[i].hp > 0 {
                i += 1;
                continue;
            }
            let e = self.enemies.remove(i);
            let room = e.room;
            if e.kind == EnemyKind::Warden {
                // The boss drops hearts and, in a run, reveals the Dawn Shard.
                for dx in [-14.0, 14.0] {
                    self.pickups.push(Pickup { kind: PickupKind::Heart, room, pos: [e.pos[0] + dx, e.pos[1]], active: true });
                }
                for p in &mut self.pickups {
                    if p.kind == PickupKind::Crystal {
                        p.active = true;
                    }
                }
            } else if self.rng.chance(t.drop_chance) {
                let kind = if self.rng.chance(0.7) { PickupKind::Heart } else { PickupKind::Gem };
                self.pickups.push(Pickup { kind, room, pos: e.pos, active: true });
            }
        }
    }

    fn check_end(&mut self, n: &Ints) {
        if self.player.hp > 0 {
            return;
        }
        if self.free_roam {
            // No fail state: back to the room's entry, healed.
            self.player.pos = self.player.entry;
            self.player.prev = self.player.entry;
            self.player.hp = self.player.max_hp.max(n.max_hp);
            self.player.invuln = 1.5;
            self.free_respawns += 1;
        } else if self.phase == Phase::Playing {
            self.phase = Phase::Lost;
            self.end_wait = 1.0;
            self.fails.game_overs += 1;
        }
    }

}
