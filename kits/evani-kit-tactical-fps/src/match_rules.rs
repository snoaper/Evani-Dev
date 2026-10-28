//! The match: soldiers, teams, damage, kills, the core hold, the ring and
//! the phase flow. Ported from Hexagon's `runtime.ts` (`spawn`, `kill`,
//! `hurt`, `simulate`, `checkWin`, the phase steps of the tick).
//!
//! One [`Match::step`] per fixed 60 Hz step (Hexagon's sub-step). Bots
//! think every third step (Hexagon's 50 ms tick) and hold that input for
//! the three steps. Hexagon's lobby and network roster are replaced by a
//! local title screen: the player is on Blue and bots fill both teams.

use evani_core::rng::Pcg32;

use crate::arena::Arena;
use crate::body::{Body, Controls};
use crate::bot::{self, Brain};
use crate::ring::{Ring, RingEvent};
use crate::tune::{Tune, WEAPON_NAMES};
use crate::weapons::Projectile;

// The match over evani-net: its wire format and the server's seat changes
// (src/net). A child module, so it reaches the private spawn bookkeeping.
#[cfg(feature = "net")]
mod net;

/// How long a hitscan tracer stays on screen (s); drawing only.
pub const SHOT_LIFE: f32 = 0.12;
/// How long a blast flash stays on screen (s); drawing only.
pub const BLAST_LIFE: f32 = 0.45;
/// Kill feed lines kept for the HUD.
const FEED_MAX: usize = 5;
/// Steps per bot thought (Hexagon's tick is three sub-steps).
pub const THINK_EVERY: u64 = 3;
/// A bot thought's time step (s).
pub const THINK_DT: f32 = 0.05;

/// The match phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Title screen: the fortress turns slowly; Fire or Jump starts.
    Title,
    /// Warm-up at the bases (no damage, no firing).
    Pre,
    /// The match.
    Play,
    /// The result, then back to the title.
    Results,
}

/// Why a match ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinReason {
    /// A team held the enemy core for `hold_s`.
    Core,
    /// A team reached `kills_to_win`.
    Kills,
    /// The match time ran out.
    Time,
}

/// Where a hitscan shot ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotEnd {
    /// A soldier.
    Soldier,
    /// Concrete (it left a hole).
    Concrete,
    /// Metal.
    Metal,
    /// Nothing within range.
    Air,
}

/// A hitscan tracer (drawing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    /// Muzzle.
    pub from: [f32; 3],
    /// End point.
    pub to: [f32; 3],
    /// What it hit.
    pub end: ShotEnd,
    /// Age (s).
    pub age: f32,
    /// Shooter's team.
    pub team: usize,
}

/// A blast flash (drawing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    /// Centre.
    pub p: [f32; 3],
    /// Age (s).
    pub age: f32,
    /// Shooter's team.
    pub team: usize,
}

/// One soldier: the player or a bot.
#[derive(Debug, Clone, PartialEq)]
pub struct Soldier {
    /// Name for the HUD.
    pub name: String,
    /// Team: 0 Blue, 1 Orange.
    pub team: usize,
    /// Body.
    pub body: Body,
    /// Health.
    pub hp: f32,
    /// Time since death (s), `None` while alive.
    pub dead: Option<f32>,
    /// Spawn shield left (s).
    pub shield: f32,
    /// Kills.
    pub kills: u32,
    /// Deaths.
    pub deaths: u32,
    /// Recoil pitch kick still applied (rad).
    pub recoil: f32,
    /// Recoil yaw kick still applied (rad).
    pub recoil_yaw: f32,
    /// The controls held this step (a bot holds its thought for three steps).
    pub hold: Controls,
    /// A bot's brain (`None` for the player).
    pub brain: Option<Brain>,
    /// Times spawned (wrapping). Net builds: an input's aim only applies
    /// to the life it was made for, so a respawn's facing is not undone.
    #[cfg(feature = "net")]
    pub spawns: u8,
}

impl Soldier {
    /// Alive.
    pub fn alive(&self) -> bool {
        self.dead.is_none()
    }
}

/// The player's input for one step (the lib reads it from the actions).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerInput {
    /// Strafe, -1 .. 1.
    pub strafe: f32,
    /// Forward, -1 .. 1.
    pub forward: f32,
    /// Turn this step (rad, right positive).
    pub dyaw: f32,
    /// Look up this step (rad).
    pub dpitch: f32,
    /// Fire held.
    pub fire: bool,
    /// Jump held.
    pub jump: bool,
    /// Next weapon (pressed this step).
    pub next_weapon: bool,
    /// Start (Fire or Jump pressed this step; used on the title).
    pub start: bool,
}

/// What one step changed that the lib must act on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StepOut {
    /// Build a fresh arena from this seed (a new match).
    pub new_arena: Option<u64>,
    /// The match ended this step.
    pub ended: Option<WinReason>,
}

/// A kill feed line.
#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    /// Killer's name (empty for a fall).
    pub by: String,
    /// Victim's name.
    pub victim: String,
    /// Weapon slot, `None` for a fall.
    pub weapon: Option<usize>,
}

/// The match state (a resource; the [`Arena`] is a separate one).
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Phase.
    pub phase: Phase,
    /// Time in this phase (s).
    pub phase_t: f32,
    /// Fixed steps run.
    pub steps: u64,
    /// Free roam: no bots, score, timer or win.
    pub free_roam: bool,
    /// Every soldier; index 0 is the player.
    pub soldiers: Vec<Soldier>,
    /// Rockets and grenades in flight.
    pub projectiles: Vec<Projectile>,
    /// Tracers.
    pub shots: Vec<Shot>,
    /// Blast flashes.
    pub blasts: Vec<Blast>,
    /// Team kills.
    pub score: [u32; 2],
    /// Time each team has held the enemy core (s).
    pub hold: [f32; 2],
    /// Winner: 0 Blue, 1 Orange, 2 draw (set in Results).
    pub winner: Option<usize>,
    /// Why the last match ended.
    pub why: Option<WinReason>,
    /// The middle ring.
    pub ring: Ring,
    /// Next spawn slot per team.
    spawn_next: [usize; 2],
    /// Bots made so far (names and roles).
    bots_made: usize,
    /// Holes version the collapse check last saw.
    pub seen_holes: u64,
    /// Recent kills.
    pub feed: Vec<Feed>,
    /// Net builds, lag compensation: per human shooter this step, where
    /// each soldier stood as that shooter saw them (`None`: use the live
    /// position). Filled and cleared around each server or predicted step.
    #[cfg(feature = "net")]
    pub lag: Vec<(usize, Vec<Option<[f32; 3]>>)>,
}

/// Wrap an angle into -pi .. pi.
pub fn wrap(a: f32) -> f32 {
    a.sin().atan2(a.cos())
}

/// Signed shortest turn from `a` to `b`.
pub fn ang_diff(a: f32, b: f32) -> f32 {
    wrap(b - a)
}

impl Match {
    /// A match at the title (or, in free roam, the player alone in play).
    pub fn new(free_roam: bool, arena: &Arena, t: &Tune) -> Self {
        let mut m = Match {
            phase: Phase::Title,
            phase_t: 0.0,
            steps: 0,
            free_roam,
            soldiers: Vec::new(),
            projectiles: Vec::new(),
            shots: Vec::new(),
            blasts: Vec::new(),
            score: [0; 2],
            hold: [0.0; 2],
            winner: None,
            why: None,
            ring: Ring::new(t.ring_interval_s),
            spawn_next: [0; 2],
            bots_made: 0,
            seen_holes: 0,
            feed: Vec::new(),
            #[cfg(feature = "net")]
            lag: Vec::new(),
        };
        m.add_soldier("You".into(), 0, None, arena, t);
        if free_roam {
            m.phase = Phase::Play;
        }
        m
    }

    fn add_soldier(&mut self, name: String, team: usize, brain: Option<Brain>, arena: &Arena, t: &Tune) {
        self.soldiers.push(Soldier {
            name,
            team,
            body: Body::default(),
            hp: t.max_hp,
            dead: None,
            shield: 0.0,
            kills: 0,
            deaths: 0,
            recoil: 0.0,
            recoil_yaw: 0.0,
            hold: Controls::default(),
            brain,
            #[cfg(feature = "net")]
            spawns: 0,
        });
        let i = self.soldiers.len() - 1;
        self.spawn(i, arena, t);
    }

    /// Put soldier `i` at its team's next spawn point (Hexagon's `spawn`).
    pub fn spawn(&mut self, i: usize, arena: &Arena, t: &Tune) {
        let Some(team) = self.soldiers.get(i).map(|s| s.team) else {
            return;
        };
        let n = self.spawn_next[team & 1];
        self.spawn_next[team & 1] = (n + 1) % 6;
        let (p, yaw) = arena.spawn_point(team, n);
        if let Some(s) = self.soldiers.get_mut(i) {
            let weapon = s.body.weapon;
            s.body = Body::at(p, yaw);
            s.body.weapon = weapon;
            s.hp = t.max_hp;
            s.dead = None;
            s.shield = t.spawn_shield_s;
            s.recoil = 0.0;
            s.recoil_yaw = 0.0;
            s.hold = Controls { yaw, weapon, ..Controls::default() };
            #[cfg(feature = "net")]
            {
                s.spawns = s.spawns.wrapping_add(1);
            }
        }
    }

    /// Fill both teams: the player on Blue, bots up to `team_size` each.
    fn fill_teams(&mut self, arena: &Arena, rng: &mut Pcg32, t: &Tune) {
        self.soldiers.truncate(1);
        let size = t.team_size.max(0) as usize;
        for team in 0..2 {
            let want = if team == 0 { size.saturating_sub(1) } else { size };
            for _ in 0..want {
                let names = &arena.def.bot_names;
                let name = match names.get(self.bots_made % names.len().max(1)) {
                    Some(n) => format!("Bot {n}"),
                    None => format!("Bot {}", self.bots_made + 1),
                };
                let attacker = self.bots_made.is_multiple_of(2);
                self.bots_made += 1;
                let brain = Brain::new(attacker, rng);
                self.add_soldier(name, team, Some(brain), arena, t);
            }
        }
    }

    /// A kill (Hexagon's `kill`): a kill scores for the killer's team; a fall
    /// or a self-kill scores for the other team.
    pub fn kill(&mut self, v: usize, by: Option<usize>, weapon: Option<usize>) {
        let Some(vt) = self.soldiers.get(v).map(|s| s.team) else {
            return;
        };
        let killer = by.filter(|&b| b != v).and_then(|b| self.soldiers.get(b).map(|s| (b, s.team)));
        let team = match killer {
            Some((_, bt)) if bt != vt => bt,
            _ => 1 - (vt & 1),
        };
        if let Some((b, bt)) = killer
            && bt == team
                && let Some(s) = self.soldiers.get_mut(b) {
                    s.kills += 1;
                }
        if !self.free_roam {
            self.score[team] += 1;
        }
        let by_name = killer.and_then(|(b, _)| self.soldiers.get(b)).map(|s| s.name.clone()).unwrap_or_default();
        if let Some(s) = self.soldiers.get_mut(v) {
            s.hp = 0.0;
            s.dead = Some(0.0);
            s.deaths += 1;
            s.body.v = [0.0; 3];
            let victim = s.name.clone();
            self.feed.push(Feed { by: by_name, victim, weapon });
            if self.feed.len() > FEED_MAX {
                self.feed.remove(0);
            }
        }
    }

    /// Damage (Hexagon's `hurt`): only in play, never through a spawn
    /// shield (except one's own blast), never in free roam.
    pub fn hurt(&mut self, v: usize, dmg: f32, by: Option<usize>, weapon: usize, _t: &Tune) {
        if self.phase != Phase::Play || self.free_roam || dmg <= 0.0 {
            return;
        }
        let Some(s) = self.soldiers.get_mut(v) else {
            return;
        };
        if !s.alive() || (s.shield > 0.0 && by != Some(v)) {
            return;
        }
        s.hp -= dmg;
        if s.hp <= 0.0 {
            self.kill(v, by, Some(weapon));
        }
    }

    /// Damage and firing are on (Hexagon's `live`): in play, and always in free roam.
    pub fn live(&self) -> bool {
        self.phase == Phase::Play
    }

    /// One fixed step of the match.
    pub fn step(&mut self, input: &PlayerInput, arena: &mut Arena, rng: &mut Pcg32, t: &Tune, dt: f32) -> StepOut {
        let mut out = StepOut::default();
        self.phase_t += dt;
        self.ring.interval = t.ring_interval_s;
        match self.phase {
            Phase::Title => {
                if input.start {
                    let seed = u64::from(rng.next_u32()) + 1;
                    *arena = Arena::new(arena.def.clone(), seed);
                    self.start_match(arena, rng, t);
                    out.new_arena = Some(seed);
                }
                return out;
            }
            Phase::Results => {
                if self.phase_t >= t.results_s {
                    self.phase = Phase::Title;
                    self.phase_t = 0.0;
                }
                return out;
            }
            Phase::Pre | Phase::Play => {}
        }
        self.simulate(input, arena, rng, t, dt);
        arena.collapse(&mut self.seen_holes, t);
        if self.phase == Phase::Pre {
            if self.phase_t >= t.pre_s {
                self.phase = Phase::Play;
                self.phase_t = 0.0;
                self.projectiles.clear();
                self.ring = Ring::new(t.ring_interval_s);
                arena.set_ring(0.0);
                for i in 0..self.soldiers.len() {
                    self.spawn(i, arena, t);
                }
            }
        } else if !self.free_roam {
            out.ended = self.check_win(arena, t, dt);
        }
        out
    }

    /// A new match on a fresh arena: teams filled, scores cleared, warm-up.
    fn start_match(&mut self, arena: &mut Arena, rng: &mut Pcg32, t: &Tune) {
        self.phase = Phase::Pre;
        self.phase_t = 0.0;
        self.score = [0; 2];
        self.hold = [0.0; 2];
        self.winner = None;
        self.why = None;
        self.projectiles.clear();
        self.shots.clear();
        self.blasts.clear();
        self.feed.clear();
        self.ring = Ring::new(t.ring_interval_s);
        arena.set_ring(0.0);
        self.seen_holes = arena.holes.version;
        self.spawn_next = [0; 2];
        self.fill_teams(arena, rng, t);
        if let Some(p) = self.soldiers.first_mut() {
            p.kills = 0;
            p.deaths = 0;
        }
        self.spawn(0, arena, t);
    }

    /// Hexagon's `simulate`, one sub-step: ring carry, bots, bodies, falls,
    /// projectiles, respawns.
    fn simulate(&mut self, input: &PlayerInput, arena: &mut Arena, rng: &mut Pcg32, t: &Tune, dt: f32) {
        let live = self.live();
        // Bots think once per Hexagon tick and hold their input for it.
        if self.steps.is_multiple_of(THINK_EVERY) {
            for i in 0..self.soldiers.len() {
                if !self.soldiers[i].alive() {
                    continue;
                }
                if let Some(mut brain) = self.soldiers[i].brain.take() {
                    let c = bot::think(self, i, &mut brain, arena, rng, t);
                    self.soldiers[i].hold = c;
                    self.soldiers[i].brain = Some(brain);
                }
            }
        }
        self.steps += 1;
        self.player_controls(input);
        // The ring turns (and carries what stands in its cars).
        let a0 = self.ring.angle(t.ring_rotate_s);
        if live && self.ring.advance(dt, t.ring_rotate_s) == RingEvent::Started {
            self.feed_note("The ring is turning");
        }
        let a1 = self.ring.angle(t.ring_rotate_s);
        let d = ang_diff(a0, a1);
        arena.set_ring(a0);
        if d != 0.0 {
            for s in self.soldiers.iter_mut().filter(|s| s.alive()) {
                if arena.carry(&mut s.body.p, &mut s.body.v, d) {
                    s.body.yaw += d;
                    s.hold.yaw += d;
                }
            }
            for pr in &mut self.projectiles {
                arena.carry(&mut pr.p, &mut pr.v, d);
            }
        }
        arena.set_ring(a1);
        for i in 0..self.soldiers.len() {
            if !self.soldiers[i].alive() {
                continue;
            }
            let c = self.soldiers[i].hold;
            let fired = self.soldiers[i].body.step(arena, &c, t, dt);
            if fired && live {
                self.fire(i, arena, rng, t);
            }
            let s = &self.soldiers[i];
            if s.alive() && s.body.p[1] < t.fall_death_y {
                if self.phase == Phase::Play && !self.free_roam {
                    self.kill(i, None, None);
                } else {
                    self.spawn(i, arena, t);
                }
            }
        }
        self.step_projectiles(arena, rng, t, dt);
        // Recoil settles; tracers and flashes age.
        let settle = (-t.recoil_decay * dt).exp();
        for s in &mut self.soldiers {
            s.recoil *= settle;
            s.recoil_yaw *= settle;
        }
        self.shots.iter_mut().for_each(|s| s.age += dt);
        self.shots.retain(|s| s.age < SHOT_LIFE);
        self.blasts.iter_mut().for_each(|b| b.age += dt);
        self.blasts.retain(|b| b.age < BLAST_LIFE);
        // Shields run down; the dead respawn.
        for i in 0..self.soldiers.len() {
            let s = &mut self.soldiers[i];
            s.shield = (s.shield - dt).max(0.0);
            if let Some(dead) = s.dead.as_mut() {
                *dead += dt;
                if *dead >= t.respawn_s {
                    self.spawn(i, arena, t);
                }
            }
        }
    }

    /// The player's held controls from this step's input.
    fn player_controls(&mut self, input: &PlayerInput) {
        let Some(p) = self.soldiers.first_mut() else {
            return;
        };
        let h = &mut p.hold;
        h.strafe = input.strafe.clamp(-1.0, 1.0);
        h.forward = input.forward.clamp(-1.0, 1.0);
        h.yaw = wrap(h.yaw + input.dyaw);
        h.pitch = (h.pitch + input.dpitch).clamp(-1.4, 1.4);
        h.fire = input.fire;
        h.jump = input.jump;
        if input.next_weapon {
            h.weapon = (h.weapon + 1) % WEAPON_NAMES.len();
        }
    }

    fn feed_note(&mut self, text: &str) {
        self.feed.push(Feed { by: String::new(), victim: text.into(), weapon: None });
        if self.feed.len() > FEED_MAX {
            self.feed.remove(0);
        }
    }

    /// Hexagon's `checkWin`: a team with someone in the enemy core and no
    /// defender there gains hold time (it drains at half speed when nobody
    /// attacks); a full hold or `kills_to_win` wins; at the time limit the
    /// team with more kills wins (or a draw).
    fn check_win(&mut self, arena: &Arena, t: &Tune, dt: f32) -> Option<WinReason> {
        for team in 0..2 {
            let (mut att, mut def) = (0, 0);
            for s in self.soldiers.iter().filter(|s| s.alive()) {
                if !arena.in_core(1 - team, s.body.p) {
                    continue;
                }
                if s.team == team {
                    att += 1;
                } else {
                    def += 1;
                }
            }
            if att > 0 && def == 0 {
                self.hold[team] += dt;
            } else if att == 0 {
                self.hold[team] = (self.hold[team] - dt / 2.0).max(0.0);
            }
        }
        let mut win = None;
        for team in 0..2 {
            if self.hold[team] >= t.hold_s {
                win = Some((team, WinReason::Core));
            } else if self.score[team] >= t.kills_to_win.max(1) as u32 && win.is_none() {
                win = Some((team, WinReason::Kills));
            }
        }
        if win.is_none() && self.phase_t >= t.round_s {
            let team = match self.score[0].cmp(&self.score[1]) {
                core::cmp::Ordering::Greater => 0,
                core::cmp::Ordering::Less => 1,
                core::cmp::Ordering::Equal => 2,
            };
            win = Some((team, WinReason::Time));
        }
        let (team, why) = win?;
        self.winner = Some(team);
        self.why = Some(why);
        self.phase = Phase::Results;
        self.phase_t = 0.0;
        self.projectiles.clear();
        Some(why)
    }

    /// The HUD text for this moment.
    pub fn hud_text(&self, arena: &Arena, t: &Tune) -> String {
        let mut s = String::new();
        let me = self.soldiers.first();
        match self.phase {
            Phase::Title => {
                s.push_str("HEXAGON\nTwo teams, one turning fortress.\nHold the enemy core, or outscore them.\n\nFire or Jump to start");
                return s;
            }
            Phase::Results => {
                let who = match self.winner {
                    Some(0) => "Blue wins",
                    Some(1) => "Orange wins",
                    _ => "A draw",
                };
                let why = match self.why {
                    Some(WinReason::Core) => "core held",
                    Some(WinReason::Kills) => "kills",
                    Some(WinReason::Time) => "time",
                    None => "",
                };
                s.push_str(&format!("{who} ({why})\nBlue {} : {} Orange", self.score[0], self.score[1]));
                if let Some(p) = me {
                    s.push_str(&format!("\nYou: {} kills, {} deaths", p.kills, p.deaths));
                }
                return s;
            }
            Phase::Pre => {
                s.push_str(&format!("Match starts in {:.0}\n", (t.pre_s - self.phase_t).max(0.0).ceil()));
            }
            Phase::Play => {}
        }
        if self.free_roam {
            s.push_str("Free roam\n");
        } else {
            let left = (t.round_s - if self.phase == Phase::Play { self.phase_t } else { 0.0 }).max(0.0) as u32;
            s.push_str(&format!(
                "Blue {} : {} Orange   {}:{:02}\nCore Blue {:.0}%  Orange {:.0}%\n",
                self.score[0],
                self.score[1],
                left / 60,
                left % 60,
                100.0 * self.hold[0] / t.hold_s.max(0.1),
                100.0 * self.hold[1] / t.hold_s.max(0.1),
            ));
        }
        if let Some(p) = me {
            match p.dead {
                Some(d) => s.push_str(&format!("Respawn in {:.0}\n", (t.respawn_s - d).max(0.0).ceil())),
                None => {
                    let w = WEAPON_NAMES.get(p.body.weapon).copied().unwrap_or("");
                    s.push_str(&format!("HP {:.0}   {w}\n", p.hp.max(0.0)));
                    if !self.free_roam && arena.in_core(1, p.body.p) {
                        s.push_str("In the enemy core: hold it!\n");
                    }
                }
            }
        }
        if self.phase == Phase::Play && self.ring.until_turn() < t.ring_warn_s {
            if self.ring.turning.is_some() {
                s.push_str("The ring is turning\n");
            } else {
                s.push_str(&format!("Ring turns in {:.0}\n", self.ring.until_turn().ceil()));
            }
        }
        for f in &self.feed {
            match f.weapon {
                Some(w) => {
                    let wn = WEAPON_NAMES.get(w).copied().unwrap_or("");
                    s.push_str(&format!("{} [{wn}] {}\n", f.by, f.victim));
                }
                None if f.by.is_empty() && self.soldiers.iter().any(|x| x.name == f.victim) => {
                    s.push_str(&format!("{} fell\n", f.victim));
                }
                None => {
                    s.push_str(&f.victim);
                    s.push('\n');
                }
            }
        }
        s
    }
}
