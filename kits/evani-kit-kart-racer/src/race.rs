//! The race: grid, countdown, laps and checkpoint gates, positions, item
//! boxes and items in flight, kart bumps, wrong-way detection, finish and
//! results.
//!
//! Every gameplay object is an entity in evani-core's ECS:
//!
//! * a kart is an entity with [`Kart`], [`AiState`] (every kart has a brain:
//!   AIs always, humans on the autopilot and after they finish) and
//!   [`Controls`] (this tick's input, human or AI);
//! * an item box is an entity with [`ItemBox`];
//! * items in flight are pooled entities with [`Goo`], [`Bomb`], [`Seeker`]
//!   or [`Blast`] plus [`InFlight`] (on or idle, and spawn order). The pools
//!   are spawned once, so a capture moment can stage a race through `&World`
//!   (structural changes need `&mut World`).
//!
//! The [`Race`] resource keeps race-wide state and the entity lists in a
//! fixed order: [`Race::grid`] (karts in grid order), [`Race::boxes`] and
//! [`Race::pools`]. Kart numbers in events, [`Race::order`], finish order
//! and results are grid indices. Systems always walk these lists, never a
//! storage's dense order, so the sim is deterministic.
//!
//! One fixed tick runs these stages in order (the kit registers each as a
//! system in `Stage::Update`; [`tick`] and [`tick_shared`] run the same list
//! for tests and capture moments): [`flow`], [`control`], [`step_karts`],
//! [`boxes`], [`bumps`], [`items`], [`standings`].

use core::cell::RefMut;
use core::ops::{Deref, DerefMut};

use evani_core::ecs::{Component, Entity, World};
use evani_core::play::PlayOutcomes;
use evani_core::rng::Pcg32;

use crate::Tune;
use crate::ai::{self, AiState, KartSpec};
use crate::items::{Blast, Bomb, Goo, Item, ItemBox, OddsTable, Seeker, Slot, band_value};
use crate::kart::{self, Controls, Kart, Launch};
use crate::track::Track;

/// Race flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Attract race behind the title screen (AI only).
    Title,
    /// 3, 2, 1, GO.
    Countdown,
    Racing,
    /// Every human finished (or, with no humans, every kart): results.
    Finished,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Title => "title",
            Phase::Countdown => "countdown",
            Phase::Racing => "racing",
            Phase::Finished => "finished",
        }
    }
}

/// What the game (menus, DOM UI) asks of the race. Applied at the start of
/// the next tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// New race with 0, 1 or 2 local players (0 = an all-AI race).
    Start { players: usize },
    /// Same players, fresh grid.
    Restart,
    /// Back to the title screen's attract race.
    Title,
    Pause(bool),
    TogglePause,
}

/// Something that happened this tick (sounds, shake, HUD flashes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RaceEvent {
    Countdown(u8),
    Go,
    ItemBox(usize),
    ItemReady(usize),
    ItemUse(usize, Item),
    Hop(usize),
    DriftTier(usize, u8),
    Boost(usize, u8),
    Hit(usize),
    ShieldPop(usize),
    Bump(usize, usize),
    Wall(usize, f32),
    Blast([f32; 3]),
    Lap(usize, u32),
    Finish(usize, usize),
}

/// Integer settings read from params at reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub karts: usize,
    pub laps: u32,
    pub checkpoints: u32,
}

impl Default for Counts {
    fn default() -> Self {
        Self {
            karts: 8,
            laps: 3,
            checkpoints: 8,
        }
    }
}

/// The formulas the race evaluates (curve and expression from the
/// registry, or plain closures in tests).
pub struct Fns<'a> {
    /// `steer_by_speed(speed_frac)`.
    pub steer: &'a dyn Fn(f32) -> f32,
    /// `rubber_band(gap, dist, strength)`.
    pub rubber: &'a dyn Fn(f32, f32, f32) -> f32,
}

/// What one tick runs with: tunables, counts, scaled gravity, the fixed dt
/// and the local players' controls.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub t: Tune,
    pub counts: Counts,
    pub gravity: f32,
    pub dt: f32,
    pub inputs: [Controls; 2],
}

/// One row of the results table.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultRow {
    pub place: usize,
    pub kart: usize,
    pub name: String,
    pub player: Option<usize>,
    pub time: Option<f32>,
    /// The kart's body colour (linear RGB), for the results row.
    pub color: [f32; 3],
}

/// Component on every pooled item entity: in flight or idle, and the spawn
/// order (items update oldest first, and a full pool reuses the oldest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InFlight {
    pub on: bool,
    pub seq: u32,
}

/// Item pool sizes (entities spawned once; a full pool reuses its oldest).
pub const GOO_POOL: usize = 16;
pub const BOMB_POOL: usize = 8;
pub const SEEKER_POOL: usize = 8;
pub const BLAST_POOL: usize = 8;
/// Most karts in a race.
pub const MAX_KARTS: usize = 12;

/// Which item pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    Goo,
    Bomb,
    Seeker,
    Blast,
}

/// The pooled item entities, in slot order.
#[derive(Debug, Clone, Default)]
pub struct ItemPools {
    pub goos: Vec<Entity>,
    pub bombs: Vec<Entity>,
    pub seekers: Vec<Entity>,
    pub blasts: Vec<Entity>,
    pub(crate) next_seq: u32,
}

impl ItemPools {
    /// Pooled entities of every kind.
    pub fn len(&self) -> usize {
        self.goos.len() + self.bombs.len() + self.seekers.len() + self.blasts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn list(&self, p: Pool) -> &[Entity] {
        match p {
            Pool::Goo => &self.goos,
            Pool::Bomb => &self.bombs,
            Pool::Seeker => &self.seekers,
            Pool::Blast => &self.blasts,
        }
    }

    fn all(&self) -> impl Iterator<Item = Entity> + '_ {
        self.goos
            .iter()
            .chain(&self.bombs)
            .chain(&self.seekers)
            .chain(&self.blasts)
            .copied()
    }

    /// Put `value` in flight on a free entity of pool `p` (the oldest in
    /// flight when none is free).
    pub fn spawn<T: Component>(&mut self, w: &World, p: Pool, value: T) {
        let list = match p {
            Pool::Goo => &self.goos,
            Pool::Bomb => &self.bombs,
            Pool::Seeker => &self.seekers,
            Pool::Blast => &self.blasts,
        };
        let mut free = None;
        let mut oldest: Option<(u32, Entity)> = None;
        for &e in list {
            let Some(f) = w.get::<InFlight>(e).map(|f| *f) else {
                continue;
            };
            if !f.on {
                free = Some(e);
                break;
            }
            if oldest.is_none_or(|(s, _)| f.seq < s) {
                oldest = Some((f.seq, e));
            }
        }
        let Some(e) = free.or(oldest.map(|(_, e)| e)) else {
            return;
        };
        if let Some(mut m) = w.get_mut::<T>(e) {
            *m = value;
        }
        if let Some(mut m) = w.get_mut::<InFlight>(e) {
            *m = InFlight {
                on: true,
                seq: self.next_seq,
            };
        }
        self.next_seq = self.next_seq.wrapping_add(1);
    }

    /// Items in flight in pool `p`, oldest first.
    pub fn active<T: Component + Clone>(&self, w: &World, p: Pool) -> Vec<(Entity, T)> {
        let mut v: Vec<(u32, Entity, T)> = self
            .list(p)
            .iter()
            .filter_map(|&e| {
                let f = *w.get::<InFlight>(e)?;
                if !f.on {
                    return None;
                }
                Some((f.seq, e, w.get::<T>(e)?.clone()))
            })
            .collect();
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|(_, e, t)| (e, t)).collect()
    }

    /// Idle again.
    pub fn retire(w: &World, e: Entity) {
        if let Some(mut f) = w.get_mut::<InFlight>(e) {
            f.on = false;
        }
    }

    fn clear(&mut self, w: &World) {
        for e in self.all() {
            if let Some(mut f) = w.get_mut::<InFlight>(e) {
                *f = InFlight::default();
            }
        }
        self.next_seq = 0;
    }
}

/// World resource: race-wide state and the race's entities in a fixed order.
#[derive(Debug, Clone)]
pub struct Race {
    pub track: Track,
    pub odds: OddsTable,
    pub roster: Vec<KartSpec>,
    pub phase: Phase,
    pub paused: bool,
    pub players: usize,
    /// Kart entities in grid order (index = kart number everywhere).
    pub grid: Vec<Entity>,
    /// Item box entities, row by row.
    pub boxes: Vec<Entity>,
    /// Pooled item entities.
    pub pools: ItemPools,
    pub countdown: f32,
    /// Race clock (seconds since GO).
    pub clock: f32,
    /// Seconds in the current phase.
    pub phase_time: f32,
    pub counts: Counts,
    /// Kart indices by place.
    pub order: Vec<usize>,
    pub finish_order: Vec<usize>,
    /// The final places, latched when the celebration ends and the podium
    /// takes over, so the podium and the results panel never shuffle.
    pub final_order: Option<Vec<usize>>,
    /// Each kart's time for the results panel, latched with `final_order`:
    /// its finish time, or for a kart still racing the time it is on pace
    /// for (the race clock scaled by the distance it has left).
    pub final_times: Vec<Option<f32>>,
    pub rng: Pcg32,
    pub race_no: u64,
    /// Events since the last frame drained them (capped).
    pub events: Vec<RaceEvent>,
    /// Camera shake trauma per local player (0..1); the draw system decays it.
    pub trauma: [f32; 2],
    /// The AI drives the local players' karts too (scripted capture moments
    /// and demos); off by default.
    pub autopilot: bool,
    /// The next drawn frame puts the chase camera (and its FOV kick) straight
    /// where it belongs, with no smoothing; the draw system clears it.
    /// Capture moments set it after staging.
    pub cam_snap: bool,
    /// A staged capture moment draws its time-driven looks (sparks, flames,
    /// the blast, shake) at this fixed time while the capture holds the game,
    /// so two captures of one moment match pixel for pixel; the next tick
    /// clears it.
    pub moment_time: Option<f32>,
    /// A staged capture moment's own chase framing (in place of the tuned
    /// chase camera and its rival lift). It outlives [`Race::moment_time`]
    /// by [`Race::moment_cam_left`] seconds of play, so the page and showcase
    /// shots taken just after the capture keep the framing.
    pub moment_cam: Option<MomentCam>,
    /// Seconds of play [`Race::moment_cam`] still holds.
    pub moment_cam_left: f32,
    /// The karts move this tick (set by [`flow`]: not paused, past the
    /// countdown).
    pub stepping: bool,
    /// Free roam (`PlayMode::FreeRoam`, docs/PLAYGROUNDS.md): the player
    /// alone on the track, already driving: no countdown, rivals, laps, race
    /// timer, places or finish, and nothing ends the run. Fixed when the kit
    /// is built.
    pub free_roam: bool,
    /// Seed of every race's RNG stream ([`RACE_SEED`] offline; an online
    /// session's own seed from the host).
    pub seed: u64,
    /// Online play (`net`, src/net.rs): the seats humans hold and every
    /// kart's controls for this tick, from the referee's input stream. None
    /// offline (every single-player path is untouched).
    pub online: Option<Online>,
    /// Drawing only: the karts local view `p` follows when they are not the
    /// back of the grid (an online player's own kart, or the one a
    /// spectator follows). None offline.
    pub view_karts: Option<[usize; 2]>,
    pub(crate) requests: Vec<Request>,
}

/// Online play: who holds which kart and this tick's controls (index =
/// kart number = seat). Humans steer their own kart; the referee's AI drives
/// the rest, and its inputs arrive in the same stream.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Online {
    pub humans: u32,
    pub controls: Vec<Controls>,
}

impl Online {
    pub fn human(&self, kart: usize) -> bool {
        kart < 32 && self.humans & (1 << kart) != 0
    }

    fn controls(&self, kart: usize) -> Controls {
        self.controls.get(kart).copied().unwrap_or_default()
    }
}

/// A capture moment's chase framing: the eye `distance` behind the kart and
/// `height` over it, looking at a point `look_ahead` in front of it and
/// `look_height` over it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MomentCam {
    pub distance: f32,
    pub height: f32,
    pub look_ahead: f32,
    pub look_height: f32,
}

/// Seed of every race's RNG stream (the stream is the race number).
pub const RACE_SEED: u64 = 0x5EED_CA27;
const MAX_EVENTS: usize = 512;
/// A race never runs past this many seconds (15 minutes).
pub const RACE_LIMIT_S: f32 = 900.0;

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Race {
    /// The race resource with no entities yet; [`spawn_objects`] adds them.
    pub fn new(track: Track, odds: OddsTable, roster: Vec<KartSpec>, counts: Counts) -> Self {
        Self {
            track,
            odds,
            roster,
            phase: Phase::Title,
            paused: false,
            players: 0,
            grid: Vec::new(),
            boxes: Vec::new(),
            pools: ItemPools::default(),
            countdown: 0.0,
            clock: 0.0,
            phase_time: 0.0,
            counts,
            order: Vec::new(),
            finish_order: Vec::new(),
            final_order: None,
            final_times: Vec::new(),
            rng: Pcg32::new(RACE_SEED, 0),
            race_no: 0,
            events: Vec::with_capacity(64),
            trauma: [0.0; 2],
            autopilot: false,
            cam_snap: false,
            moment_time: None,
            moment_cam: None,
            moment_cam_left: 0.0,
            stepping: false,
            free_roam: false,
            seed: RACE_SEED,
            online: None,
            view_karts: None,
            requests: Vec::new(),
        }
    }

    /// Queue a request for the next tick.
    pub fn request(&mut self, r: Request) {
        self.requests.push(r);
    }

    /// Kart (grid index) driven by local player `p`: humans fill the back
    /// of the grid.
    pub fn player_kart(&self, p: usize) -> Option<usize> {
        if let Some(v) = self.view_karts {
            return (p < self.players).then(|| v[p.min(1)]).filter(|&i| i < self.grid.len());
        }
        (p < self.players).then(|| self.grid.len() - self.players + p)
    }

    /// True while the sim is frozen by the pause menu.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Item boxes the track lays out.
    pub fn box_count(&self) -> usize {
        self.track.item_rows.len() * self.track.item_boxes_per_row.max(1) as usize
    }

    /// Counts and local players a reset with `players` makes.
    pub fn field(players: usize, counts: Counts) -> (Counts, usize) {
        let c = Counts {
            karts: counts.karts.clamp(players.max(1), MAX_KARTS),
            laps: counts.laps.max(1),
            checkpoints: counts.checkpoints.max(1),
        };
        (c, players.min(2).min(c.karts))
    }

    fn push_event(&mut self, e: RaceEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.drain(..MAX_EVENTS / 2);
        }
        self.events.push(e);
    }

    fn can_pause(&self) -> bool {
        matches!(self.phase, Phase::Countdown | Phase::Racing) && self.players > 0
    }

    /// Distance raced (laps included); negative before the first crossing.
    pub fn progress(&self, k: &Kart) -> f32 {
        let l = self.track.length;
        let g = self.counts.checkpoints;
        if k.next_gate == 0 {
            return self.track.delta_s(k.s, 0.0).min(0.0);
        }
        let base = self.track.gate_s(k.next_gate - 1, g);
        let mut off = self.track.wrap_s(k.s - base);
        if off > l * 0.75 {
            off -= l;
        }
        k.laps_done as f32 * l + base + off
    }

    /// Current lap (1-based, clamped to the lap count) for the HUD.
    pub fn lap_of(&self, k: &Kart) -> u32 {
        (k.laps_done + 1).min(self.counts.laps)
    }

    fn add_trauma(&mut self, player: Option<usize>, amount: f32) {
        if let Some(p) = player {
            let tr = &mut self.trauma[p.min(1)];
            *tr = (*tr + amount).min(1.0);
        }
    }

    fn human_controls(&self, k: &Kart, inputs: &[Controls; 2]) -> Option<Controls> {
        let p = k.player?;
        if self.autopilot || self.phase != Phase::Racing || k.finished.is_some() {
            return None;
        }
        Some(inputs[p.min(1)])
    }

    /// Checkpoint gates and the line for kart `i`, which moved from `prev_s`.
    fn gates(&mut self, i: usize, k: &mut Kart, prev_s: f32) {
        let g = self.counts.checkpoints;
        // Free roam has no laps: the line is just paint.
        if k.finished.is_some() || self.free_roam {
            return;
        }
        let check = if k.next_gate == 0 || k.next_gate >= g { 0 } else { k.next_gate };
        let gs = self.track.gate_s(check, g);
        let before = self.track.delta_s(prev_s, gs);
        let after = self.track.delta_s(k.s, gs);
        if !(before < 0.0 && after >= 0.0 && before > -30.0) {
            return;
        }
        if check != 0 {
            k.next_gate += 1;
            return;
        }
        if k.next_gate == 0 {
            // First crossing of the line: lap 1 starts.
            k.next_gate = 1;
            return;
        }
        k.laps_done += 1;
        k.next_gate = 1;
        if k.laps_done >= self.counts.laps {
            k.finished = Some(self.clock);
            self.finish_order.push(i);
            let place = self.finish_order.len();
            self.push_event(RaceEvent::Finish(i, place));
        } else {
            let lap = k.laps_done + 1;
            self.push_event(RaceEvent::Lap(i, lap));
        }
    }

    fn use_item(&mut self, w: &World, i: usize, k: &mut Kart, item: Item, t: &Tune) {
        self.push_event(RaceEvent::ItemUse(i, item));
        let f = k.forward();
        match item {
            Item::ZoomPop => {
                k.boost = k.boost.max(t.zoom_boost_s);
            }
            Item::GooBlob => {
                let p = [k.pos[0] - f[0] * 2.6, k.pos[1], k.pos[2] - f[1] * 2.6];
                let goo = Goo {
                    pos: p,
                    life: t.goo_lifetime_s,
                    owner: i,
                    grace: 0.8,
                };
                self.pools.spawn(w, Pool::Goo, goo);
            }
            Item::BoingBomb => {
                let v = t.bomb_speed + k.speed.max(0.0) * 0.3;
                let bomb = Bomb {
                    pos: [k.pos[0] + f[0] * 2.0, k.pos[1] + 1.0, k.pos[2] + f[1] * 2.0],
                    vel: [f[0] * v, 6.0, f[1] * v],
                    fuse: t.bomb_fuse_s,
                    bounces: 0,
                    owner: i,
                    grace: 0.4,
                    seg: k.hint,
                };
                self.pools.spawn(w, Pool::Bomb, bomb);
            }
            Item::SeekerStar => {
                let target = if k.place > 0 { self.order.get(k.place - 1).copied() } else { None };
                let seeker = Seeker {
                    s: k.s,
                    lateral: k.lateral,
                    pos: [k.pos[0], k.pos[1] + 1.2, k.pos[2]],
                    owner: i,
                    target,
                    life: if target.is_some() { 12.0 } else { 2.5 },
                };
                self.pools.spawn(w, Pool::Seeker, seeker);
            }
            Item::ShieldBubble => {
                k.shield = t.shield_s;
            }
        }
    }
}

fn wrong_way(k: &mut Kart, track: &Track, t: &Tune, dt: f32) {
    let tan = track.tangent_at(k.s);
    let fw = k.forward();
    let dot = fw[0] * tan[0] + fw[1] * tan[1];
    if dot < -0.35 && k.speed > 4.0 {
        k.wrong_way_t += dt;
        if k.wrong_way_t >= t.wrong_way_s {
            k.wrong_way = true;
        }
    } else if dot > 0.0 {
        k.wrong_way_t = 0.0;
        k.wrong_way = false;
    }
}

// ---- entities -------------------------------------------------------------------

fn blank_kart() -> Kart {
    Kart::new(0, None, [0.0; 3], 0.0)
}

/// Copies of the karts in grid order.
pub fn kart_list(w: &World, grid: &[Entity]) -> Vec<Kart> {
    grid.iter()
        .map(|&e| w.get::<Kart>(e).map_or_else(blank_kart, |k| k.clone()))
        .collect()
}

fn store_karts(w: &World, grid: &[Entity], karts: &[Kart]) {
    for (&e, k) in grid.iter().zip(karts) {
        if let Some(mut m) = w.get_mut::<Kart>(e) {
            m.clone_from(k);
        }
    }
}

fn spawn_pool<T: Component + Default>(w: &mut World, n: usize) -> Vec<Entity> {
    (0..n)
        .map(|_| {
            let e = w.spawn_with(T::default());
            w.insert(e, InFlight::default());
            e
        })
        .collect()
}

/// Spawn the race's entities (item boxes, item pools, the grid) and reset
/// to the attract race. Run once after inserting [`Race`]; again replaces
/// them.
pub fn spawn_objects(world: &mut World, t: &Tune, counts: Counts) {
    let old: Vec<Entity> = {
        let r = world.res::<Race>();
        r.boxes.iter().copied().chain(r.pools.all()).collect()
    };
    for e in old {
        world.despawn(e);
    }
    let n = world.res::<Race>().box_count();
    let boxes = (0..n).map(|_| world.spawn_with(ItemBox::default())).collect();
    let pools = ItemPools {
        goos: spawn_pool::<Goo>(world, GOO_POOL),
        bombs: spawn_pool::<Bomb>(world, BOMB_POOL),
        seekers: spawn_pool::<Seeker>(world, SEEKER_POOL),
        blasts: spawn_pool::<Blast>(world, BLAST_POOL),
        next_seq: 0,
    };
    {
        let mut r = world.res_mut::<Race>();
        r.boxes = boxes;
        r.pools = pools;
    }
    if world.res::<Race>().free_roam {
        reset_roam(world, t, counts);
    } else {
        reset_mut(world, Phase::Title, 0, t, counts);
    }
}

/// New kart entities for a grid of `n` (the old ones are despawned).
pub(crate) fn respawn_karts(world: &mut World, n: usize) {
    let old = core::mem::take(&mut world.res_mut::<Race>().grid);
    for e in old {
        world.despawn(e);
    }
    let grid: Vec<Entity> = (0..n)
        .map(|_| {
            let e = world.spawn_with(blank_kart());
            world.insert(e, AiState::default());
            world.insert(e, Controls::default());
            e
        })
        .collect();
    world.res_mut::<Race>().grid = grid;
}

/// Fresh grid on fresh kart entities. `players` humans start at the back.
pub fn reset_mut(world: &mut World, phase: Phase, players: usize, t: &Tune, counts: Counts) {
    let n = Race::field(players, counts).0.karts;
    respawn_karts(world, n);
    let done = reset(world, phase, players, t, counts);
    debug_assert!(done.is_ok(), "{done:?}");
}

/// Fresh grid on the existing entities (no structural change, so it works
/// through `&World`). Fails if the grid has a different number of kart
/// entities than this reset needs ([`reset_mut`] makes them).
pub fn reset(world: &World, phase: Phase, players: usize, t: &Tune, counts: Counts) -> Result<(), String> {
    let mut guard = world.res_mut::<Race>();
    let r = &mut *guard;
    let (counts, players) = Race::field(players, counts);
    if r.grid.len() != counts.karts {
        return Err(format!(
            "the race has {} kart entities and this reset needs {}; start a new race first",
            r.grid.len(),
            counts.karts
        ));
    }
    r.race_no += 1;
    r.rng = Pcg32::new(r.seed, r.race_no);
    r.phase = phase;
    r.paused = false;
    r.autopilot = false;
    r.stepping = false;
    r.moment_time = None;
    r.moment_cam = None;
    r.moment_cam_left = 0.0;
    r.counts = counts;
    r.players = players;
    r.track.set_line_inset(t.racing_line_inset);
    r.countdown = t.countdown_s;
    r.clock = 0.0;
    r.phase_time = 0.0;
    r.finish_order.clear();
    r.final_order = None;
    r.final_times.clear();
    r.trauma = [0.0; 2];
    r.pools.clear(world);
    let n = counts.karts;
    let hw = r.track.half_width;
    let len = r.track.length;
    let ai_count = n - players;
    for (g, &e) in r.grid.iter().enumerate() {
        // AIs fill the front of the grid, humans the back.
        let (spec, player) = if g < ai_count {
            (players + g, None)
        } else {
            (g - ai_count, Some(g - ai_count))
        };
        let row = (g / 2) as f32;
        let col = g % 2;
        let s = len - (r.track.start_offset + row * 4.5 + col as f32 * 2.0);
        let lat = if col == 0 { hw * 0.45 } else { -hw * 0.45 };
        let tan = r.track.tangent_at(s);
        let mut k = Kart::new(spec % r.roster.len(), player, r.track.point_at(s, lat), tan[0].atan2(tan[1]));
        if player.is_none() {
            let skill = r.roster[k.spec].skill;
            k.skill_mult = 1.0 + (skill - 0.5) * 2.0 * t.ai_skill_spread;
        }
        k.hint = r.track.index_at(s).0;
        k.relocate(&r.track);
        if let Some(mut m) = world.get_mut::<Kart>(e) {
            *m = k;
        }
        if let Some(mut m) = world.get_mut::<AiState>(e) {
            *m = AiState::default();
        }
        if let Some(mut m) = world.get_mut::<Controls>(e) {
            *m = Controls::default();
        }
    }
    // Item box rows across the road.
    let per = r.track.item_boxes_per_row.max(1);
    let mut slot = 0;
    for &row in &r.track.item_rows {
        let s = row * len;
        for b in 0..per {
            let lat = ((b as f32 + 0.5) / per as f32 * 2.0 - 1.0) * hw * 0.75;
            let mut p = r.track.point_at(s, lat);
            p[1] += 1.0;
            if let Some(mut m) = r.boxes.get(slot).and_then(|&e| world.get_mut::<ItemBox>(e)) {
                *m = ItemBox { pos: p, hidden: 0.0 };
            }
            slot += 1;
        }
    }
    drop(guard);
    let mut sim = Sim::load(world);
    sim.update_order();
    if phase == Phase::Countdown {
        sim.r.push_event(RaceEvent::Countdown(t.countdown_s.ceil().max(1.0) as u8));
    }
    sim.store();
    Ok(())
}

/// Recompute everybody's place from where the karts stand (a staged scene
/// that moved the karts by hand).
#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub(crate) fn refresh_places(world: &World) {
    let mut sim = Sim::load(world);
    sim.update_order();
    sim.store();
}

/// Free roam's (re)start: the player's kart alone on the grid, already
/// driving (no countdown, no rivals). Starting, restarting and "back to the
/// title" all come here: there is no title or race to quit.
pub fn reset_roam(world: &mut World, t: &Tune, counts: Counts) {
    let alone = Counts { karts: 1, ..counts };
    reset_mut(world, Phase::Racing, 1, t, alone);
}

/// Apply the queued [`Request`]s (resets make fresh kart entities).
pub fn handle_requests(world: &mut World, t: &Tune, counts: Counts) {
    let reqs = core::mem::take(&mut world.res_mut::<Race>().requests);
    let roam = world.res::<Race>().free_roam;
    for q in reqs {
        if roam && matches!(q, Request::Start { .. } | Request::Restart | Request::Title) {
            reset_roam(world, t, counts);
            continue;
        }
        match q {
            Request::Start { players } => reset_mut(world, Phase::Countdown, players, t, counts),
            Request::Restart => {
                let (phase, players) = {
                    let r = world.res::<Race>();
                    (r.phase, r.players)
                };
                if phase != Phase::Title {
                    reset_mut(world, Phase::Countdown, players, t, counts);
                }
            }
            Request::Title => reset_mut(world, Phase::Title, 0, t, counts),
            Request::Pause(p) => {
                let mut r = world.res_mut::<Race>();
                r.paused = p && r.can_pause();
            }
            Request::TogglePause => {
                let mut r = world.res_mut::<Race>();
                r.paused = !r.paused && r.can_pause();
            }
        }
    }
}

// ---- the stages of one tick ---------------------------------------------------

/// One stage's working set: the race resource and the karts copied out in
/// grid order (pairwise work needs them all at once); [`Sim::store`] writes
/// the karts back.
struct Sim<'w> {
    w: &'w World,
    r: RefMut<'w, Race>,
    karts: Vec<Kart>,
}

impl<'w> Sim<'w> {
    fn load(w: &'w World) -> Self {
        let r = w.res_mut::<Race>();
        let karts = kart_list(w, &r.grid);
        Self { w, r, karts }
    }

    fn store(self) {
        store_karts(self.w, &self.r.grid, &self.karts);
    }

    fn add_trauma(&mut self, i: usize, amount: f32) {
        let p = self.karts[i].player;
        self.r.add_trauma(p, amount);
    }

    fn strike(&mut self, i: usize, t: &Tune) {
        if self.karts[i].hit(t) {
            self.r.push_event(RaceEvent::Hit(i));
            self.add_trauma(i, 0.55);
        } else {
            self.r.push_event(RaceEvent::ShieldPop(i));
        }
    }

    fn update_order(&mut self) {
        let r = &mut *self.r;
        if let Some(fin) = r.final_order.clone() {
            for (place, &i) in fin.iter().enumerate() {
                self.karts[i].place = place;
            }
            r.order = fin;
            return;
        }
        let prog: Vec<f32> = self.karts.iter().map(|k| r.progress(k)).collect();
        let mut order: Vec<usize> = (0..self.karts.len()).collect();
        let fin = |i: usize| r.finish_order.iter().position(|&f| f == i);
        order.sort_by(|&a, &b| match (fin(a), fin(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => core::cmp::Ordering::Less,
            (None, Some(_)) => core::cmp::Ordering::Greater,
            (None, None) => prog[b].total_cmp(&prog[a]).then(a.cmp(&b)),
        });
        for (place, &i) in order.iter().enumerate() {
            self.karts[i].place = place;
        }
        // The podium takes over: freeze the places it shows.
        if r.phase == Phase::Finished && crate::finish::stage(r.phase, r.phase_time, r.players) == crate::finish::Stage::Podium {
            r.final_order = Some(order.clone());
            let (total, clock) = (r.counts.laps as f32 * r.track.length, r.clock);
            r.final_times = self
                .karts
                .iter()
                .zip(&prog)
                .map(|(k, &p)| k.finished.or_else(|| (p > 0.0).then(|| clock * (total / p).max(1.0))))
                .collect();
        }
        r.order = order;
    }

    /// End of race checks; true when the attract race is over and wants a
    /// fresh grid.
    fn check_end(&mut self) -> bool {
        let r = &mut *self.r;
        match r.phase {
            Phase::Title => !r.finish_order.is_empty(),
            // Free roam never ends.
            Phase::Racing if r.free_roam => false,
            Phase::Racing => {
                let done = if let Some(on) = r.online.as_ref().filter(|o| o.humans != 0) {
                    self.karts.iter().enumerate().filter(|(i, _)| on.human(*i)).all(|(_, k)| k.finished.is_some())
                } else if r.players > 0 {
                    self.karts.iter().filter(|k| k.player.is_some()).all(|k| k.finished.is_some())
                } else {
                    self.karts.iter().all(|k| k.finished.is_some())
                };
                // Safety net: a race never runs past 15 minutes.
                let timed_out = r.clock > RACE_LIMIT_S;
                if done || timed_out {
                    r.phase = Phase::Finished;
                    r.phase_time = 0.0;
                    r.paused = false;
                    // The race is over, which free roam never reaches
                    // (`evani_core::play::free_roam_test` fails on these).
                    if let Some(mut o) = self.w.try_res_mut::<PlayOutcomes>() {
                        o.game_over();
                        if timed_out && !done {
                            o.timer_expired();
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn collect_boxes(&mut self, t: &Tune, dt: f32) {
        let n = self.karts.len();
        let reach = t.kart_radius + 0.9;
        let r = &mut *self.r;
        let boxes = r.boxes.clone();
        for e in boxes {
            let Some(mut bx) = self.w.get_mut::<ItemBox>(e) else {
                continue;
            };
            if bx.hidden > 0.0 {
                bx.hidden = (bx.hidden - dt).max(0.0);
                continue;
            }
            let bp = bx.pos;
            for i in 0..n {
                let k = &self.karts[i];
                if dist_xz(k.pos, bp) > reach || (k.pos[1] + 0.5 - bp[1]).abs() > 2.0 {
                    continue;
                }
                bx.hidden = t.item_respawn_s;
                if self.karts[i].slot == Slot::Empty {
                    let v = band_value(self.karts[i].place, n);
                    let item = r.odds.draw(v, &mut r.rng);
                    self.karts[i].slot = Slot::Rolling {
                        left: t.roulette_s,
                        item,
                    };
                }
                r.push_event(RaceEvent::ItemBox(i));
                break;
            }
        }
        for i in 0..n {
            if let Slot::Rolling { left, item } = self.karts[i].slot {
                let left = left - dt;
                if left <= 0.0 {
                    self.karts[i].slot = Slot::Holding(item);
                    self.r.push_event(RaceEvent::ItemReady(i));
                } else {
                    self.karts[i].slot = Slot::Rolling { left, item };
                }
            }
        }
    }

    fn bumps(&mut self, t: &Tune) {
        let n = self.karts.len();
        let e = t.bump_restitution;
        // Two passes: with a pack of karts a push in one pair can press a kart
        // into another, so the second pass settles what the first left. The
        // speed exchange and the bump event happen on the first only.
        for pass in 0..2 {
            for i in 0..n {
                for j in (i + 1)..n {
                    let (a, b) = (&self.karts[i], &self.karts[j]);
                    if (a.pos[1] - b.pos[1]).abs() > 1.5 {
                        continue;
                    }
                    let Some(hit) = crate::hull::contact(&a.hull(t.kart_radius), &b.hull(t.kart_radius)) else {
                        continue;
                    };
                    let nrm = hit.normal;
                    // A little over half each, so the pair ends clear of each
                    // other instead of exactly touching.
                    let push = hit.depth * 0.5 + 0.005;
                    let (fa, fb) = (a.forward(), b.forward());
                    let va = [fa[0] * a.speed, fa[1] * a.speed];
                    let vb = [fb[0] * b.speed, fb[1] * b.speed];
                    let rel = (vb[0] - va[0]) * nrm[0] + (vb[1] - va[1]) * nrm[1];
                    let (mut va, mut vb) = (va, vb);
                    let closing = rel < 0.0 && pass == 0;
                    if closing {
                        let imp = -(1.0 + e) * rel * 0.5;
                        va = [va[0] - nrm[0] * imp, va[1] - nrm[1] * imp];
                        vb = [vb[0] + nrm[0] * imp, vb[1] + nrm[1] * imp];
                    }
                    {
                        let a = &mut self.karts[i];
                        a.pos[0] -= nrm[0] * push;
                        a.pos[2] -= nrm[1] * push;
                        if closing {
                            a.speed = va[0] * fa[0] + va[1] * fa[1];
                        }
                        a.relocate(&self.r.track);
                    }
                    {
                        let b = &mut self.karts[j];
                        b.pos[0] += nrm[0] * push;
                        b.pos[2] += nrm[1] * push;
                        if closing {
                            b.speed = vb[0] * fb[0] + vb[1] * fb[1];
                        }
                        b.relocate(&self.r.track);
                    }
                    if closing && rel < -3.0 {
                        self.r.push_event(RaceEvent::Bump(i, j));
                        let amount = (-rel / 30.0).min(0.35);
                        self.add_trauma(i, amount);
                        self.add_trauma(j, amount);
                    }
                }
            }
        }
    }

    fn update_goo(&mut self, t: &Tune, dt: f32) {
        let reach = t.kart_radius + 0.8;
        for (e, mut goo) in self.r.pools.active::<Goo>(self.w, Pool::Goo) {
            goo.life -= dt;
            goo.grace -= dt;
            let victim = self.karts.iter().enumerate().find_map(|(i, k)| {
                let may = i != goo.owner || goo.grace <= 0.0;
                (may && dist_xz(k.pos, goo.pos) < reach && (k.pos[1] - goo.pos[1]).abs() < 1.5).then_some(i)
            });
            if let Some(v) = victim {
                ItemPools::retire(self.w, e);
                self.strike(v, t);
                continue;
            }
            if goo.life <= 0.0 {
                ItemPools::retire(self.w, e);
                continue;
            }
            if let Some(mut m) = self.w.get_mut::<Goo>(e) {
                *m = goo;
            }
        }
    }

    fn update_bombs(&mut self, t: &Tune, gravity: f32, dt: f32) {
        for (e, mut bomb) in self.r.pools.active::<Bomb>(self.w, Pool::Bomb) {
            bomb.vel[1] -= gravity * dt;
            for a in 0..3 {
                bomb.pos[a] += bomb.vel[a] * dt;
            }
            bomb.fuse -= dt;
            bomb.grace -= dt;
            let loc = self.r.track.locate(bomb.pos, bomb.seg);
            bomb.seg = loc.idx;
            let floor = loc.height + 0.45;
            if bomb.pos[1] < floor {
                bomb.pos[1] = floor;
                if bomb.vel[1] < 0.0 {
                    if bomb.bounces < 2 {
                        bomb.vel[1] = -bomb.vel[1] * 0.6;
                        bomb.vel[0] *= 0.75;
                        bomb.vel[2] *= 0.75;
                    } else {
                        bomb.vel[1] = 0.0;
                        bomb.vel[0] *= 0.9;
                        bomb.vel[2] *= 0.9;
                    }
                    bomb.bounces = bomb.bounces.saturating_add(1);
                }
            }
            let reach = t.kart_radius + 0.7;
            let touched = self.karts.iter().enumerate().any(|(i, k)| {
                (i != bomb.owner || bomb.grace <= 0.0) && dist3([k.pos[0], k.pos[1] + 0.5, k.pos[2]], bomb.pos) < reach
            });
            let off = loc.lateral.abs() > self.r.track.half_width + t.verge_width + 1.0;
            if touched || bomb.fuse <= 0.0 || off {
                ItemPools::retire(self.w, e);
                self.explode(bomb.pos, t);
                continue;
            }
            if let Some(mut m) = self.w.get_mut::<Bomb>(e) {
                *m = bomb;
            }
        }
    }

    /// A Boing Bomb blast at `pos`: spins out every kart in range.
    fn explode(&mut self, pos: [f32; 3], t: &Tune) {
        let blast = Blast {
            pos,
            radius: t.bomb_radius,
            t: 0.0,
        };
        let w = self.w;
        self.r.pools.spawn(w, Pool::Blast, blast);
        self.r.push_event(RaceEvent::Blast(pos));
        for i in 0..self.karts.len() {
            let d = dist3(self.karts[i].pos, pos);
            if d < t.bomb_radius {
                self.strike(i, t);
            }
            let near = (1.0 - d / (t.bomb_radius * 5.0)).clamp(0.0, 1.0);
            if near > 0.0 {
                self.add_trauma(i, near * 0.9);
            }
        }
    }

    fn update_seekers(&mut self, t: &Tune, dt: f32) {
        for (e, mut sk) in self.r.pools.active::<Seeker>(self.w, Pool::Seeker) {
            sk.life -= dt;
            let target = sk.target.filter(|&i| self.karts.get(i).is_some_and(|k| k.finished.is_none()));
            let mut hit = None;
            let track = &self.r.track;
            match target {
                Some(ti) => {
                    let tk = &self.karts[ti];
                    let ahead = track.delta_s(tk.s, sk.s);
                    if ahead > 4.0 {
                        sk.s = track.wrap_s(sk.s + t.seeker_speed * dt);
                        sk.lateral += (tk.lateral - sk.lateral) * (3.0 * dt).min(1.0);
                        let p = track.point_at(sk.s, sk.lateral);
                        sk.pos = [p[0], p[1] + 1.2, p[2]];
                    } else {
                        // Home in directly.
                        let aim = [tk.pos[0], tk.pos[1] + 0.6, tk.pos[2]];
                        let d = dist3(aim, sk.pos).max(1e-4);
                        let step = (t.seeker_speed * dt).min(d);
                        for (p, a) in sk.pos.iter_mut().zip(aim) {
                            *p += (a - *p) / d * step;
                        }
                        let l = track.locate(sk.pos, tk.hint);
                        sk.s = l.s;
                        sk.lateral = l.lateral;
                        if d < 1.6 {
                            hit = Some(ti);
                        }
                    }
                }
                None => {
                    sk.s = track.wrap_s(sk.s + t.seeker_speed * dt);
                    let p = track.point_at(sk.s, sk.lateral);
                    sk.pos = [p[0], p[1] + 1.2, p[2]];
                }
            }
            if let Some(v) = hit {
                ItemPools::retire(self.w, e);
                self.strike(v, t);
                continue;
            }
            if sk.life <= 0.0 {
                ItemPools::retire(self.w, e);
                continue;
            }
            if let Some(mut m) = self.w.get_mut::<Seeker>(e) {
                *m = sk;
            }
        }
    }

    fn update_blasts(&mut self, dt: f32) {
        for (e, mut b) in self.r.pools.active::<Blast>(self.w, Pool::Blast) {
            b.t += dt;
            if b.t >= crate::items::BLAST_LIFE_S {
                ItemPools::retire(self.w, e);
            } else if let Some(mut m) = self.w.get_mut::<Blast>(e) {
                *m = b;
            }
        }
    }
}

fn stepping(w: &World) -> bool {
    w.try_res::<Race>().is_some_and(|r| r.stepping)
}

/// Stage 1, `kart_racer.flow`: pause, phase clock, countdown and GO. Sets
/// [`Race::stepping`] for the stages after it. Requests are applied before
/// it ([`handle_requests`]).
pub fn flow(w: &World, c: &Ctx) {
    let mut guard = w.res_mut::<Race>();
    let r = &mut *guard;
    r.stepping = false;
    r.moment_time = None;
    r.moment_cam_left -= c.dt;
    if r.moment_cam_left <= 0.0 {
        r.moment_cam = None;
    }
    if r.paused {
        return;
    }
    r.phase_time += c.dt;
    match r.phase {
        Phase::Countdown => {
            let before = r.countdown.ceil();
            r.countdown -= c.dt;
            let after = r.countdown.ceil();
            let go = r.countdown <= 0.0;
            let mut good_starts = Vec::new();
            for (i, &e) in r.grid.iter().enumerate() {
                if let Some(mut k) = w.get_mut::<Kart>(e) {
                    k.prev_pos = k.pos;
                    k.prev_yaw = k.yaw;
                    // The start boost: local players' (online: the human
                    // seats') throttle on the grid.
                    let throttle = match &r.online {
                        Some(on) => on.human(i).then(|| on.controls(i).throttle),
                        None => k.player.map(|p| c.inputs[p.min(1)].throttle),
                    };
                    if let (Some(throttle), false) = (throttle, r.autopilot) {
                        let held = throttle > 0.5;
                        k.launch = k.launch.next(held, r.countdown.max(0.0), c.t.start_boost_window_s);
                        if go && k.launch == Launch::Primed && c.t.start_boost_s > 0.0 {
                            k.boost = k.boost.max(c.t.start_boost_s);
                            good_starts.push(i);
                        }
                    }
                }
            }
            if go {
                r.phase = Phase::Racing;
                r.phase_time = 0.0;
                r.push_event(RaceEvent::Go);
                for i in good_starts {
                    r.push_event(RaceEvent::Boost(i, 0));
                }
            } else if after < before {
                r.push_event(RaceEvent::Countdown(after as u8));
            }
        }
        Phase::Title | Phase::Racing | Phase::Finished => {
            if r.phase != Phase::Title {
                r.clock += c.dt;
            }
            r.stepping = true;
        }
    }
}

/// Stage 2, `kart_racer.control`: each kart's [`Controls`] for this tick,
/// from its local player or its [`AiState`] brain, and the AI rubber band.
pub fn control(w: &World, c: &Ctx, f: &Fns) {
    if !stepping(w) {
        return;
    }
    let mut guard = w.res_mut::<Race>();
    let r = &mut *guard;
    let karts = kart_list(w, &r.grid);
    if r.online.is_some() {
        drop(guard);
        control_online(w, &karts, c, f);
        return;
    }
    // Rubber band: distance the AI is behind the best human.
    let best_human = karts
        .iter()
        .filter(|k| k.player.is_some() && k.finished.is_none())
        .map(|k| r.progress(k))
        .fold(None, |m: Option<f32>, p| Some(m.map_or(p, |m| m.max(p))));
    let grid = r.grid.clone();
    for (i, &e) in grid.iter().enumerate() {
        let k = &karts[i];
        let controls = match r.human_controls(k, &c.inputs) {
            Some(ctl) => ctl,
            None => {
                let around = ai::surroundings(&karts, i, &r.track);
                let noise = r.rng.next_f32();
                let spec = &r.roster[k.spec];
                let mut st = w.get::<AiState>(e).map(|s| *s).unwrap_or_default();
                let ctl = ai::drive(k, spec, &mut st, around, &r.track, &c.t, noise, c.dt);
                if let Some(mut m) = w.get_mut::<AiState>(e) {
                    *m = st;
                }
                ctl
            }
        };
        // A human who has crossed the line lets go: the AI keeps the steering
        // and the kart rolls to a stop past the gate under the celebration
        // camera (the brake lets off at a standstill so it never reverses).
        let controls = if k.player.is_some() && k.finished.is_some() {
            Controls {
                throttle: 0.0,
                brake: if k.speed > 0.2 { crate::finish::ROLL_OUT_BRAKE } else { 0.0 },
                drift: false,
                item: false,
                ..controls
            }
        } else {
            controls
        };
        let band = match best_human {
            Some(best) if k.player.is_none() && r.phase == Phase::Racing => {
                let gap = best - r.progress(k);
                1.0 + (f.rubber)(gap, c.t.rubber_band_distance, c.t.rubber_band_strength)
            }
            _ => 1.0,
        };
        if let Some(mut m) = w.get_mut::<Controls>(e) {
            *m = controls;
        }
        if let Some(mut m) = w.get_mut::<Kart>(e) {
            m.band_mult = band.clamp(0.5, 1.5);
        }
    }
}

/// [`control`] online: every kart's controls come from the input stream
/// (humans' own, the referee's AI for the rest); a human over the line rolls
/// out, and the AI karts get the same rubber band as offline.
fn control_online(w: &World, karts: &[Kart], c: &Ctx, f: &Fns) {
    let mut guard = w.res_mut::<Race>();
    let r = &mut *guard;
    let Some(on) = r.online.clone() else { return };
    let best_human = karts
        .iter()
        .enumerate()
        .filter(|(i, k)| on.human(*i) && k.finished.is_none())
        .map(|(_, k)| r.progress(k))
        .fold(None, |m: Option<f32>, p| Some(m.map_or(p, |m| m.max(p))));
    let grid = r.grid.clone();
    for (i, &e) in grid.iter().enumerate() {
        let k = &karts[i];
        let human = on.human(i);
        let mut controls = on.controls(i);
        if human && k.finished.is_some() {
            controls = Controls {
                throttle: 0.0,
                brake: if k.speed > 0.2 { crate::finish::ROLL_OUT_BRAKE } else { 0.0 },
                drift: false,
                item: false,
                ..controls
            };
        }
        let band = match best_human {
            Some(best) if !human && r.phase == Phase::Racing => {
                let gap = best - r.progress(k);
                1.0 + (f.rubber)(gap, c.t.rubber_band_distance, c.t.rubber_band_strength)
            }
            _ => 1.0,
        };
        if let Some(mut m) = w.get_mut::<Controls>(e) {
            *m = controls;
        }
        if let Some(mut m) = w.get_mut::<Kart>(e) {
            m.band_mult = band.clamp(0.5, 1.5);
        }
    }
}

/// Stage 3, `kart_racer.karts`: each kart in grid order uses its item,
/// drives one step ([`kart::step`]), checks wrong way and its gates.
pub fn step_karts(w: &World, c: &Ctx, f: &Fns) {
    if !stepping(w) {
        return;
    }
    let mut guard = w.res_mut::<Race>();
    let r = &mut *guard;
    let grid = r.grid.clone();
    // One snapshot of every kart for the slipstream test, so it does not
    // depend on which karts have already moved this step.
    let before = kart_list(w, &grid);
    for (i, &e) in grid.iter().enumerate() {
        let Some(mut k) = w.get_mut::<Kart>(e) else {
            continue;
        };
        let k = &mut *k;
        let controls = w.get::<Controls>(e).map(|c| *c).unwrap_or_default();
        k.draft = crate::slipstream::check(&before, i, controls.steer, &r.track);
        k.slipstream = k.draft.active();
        if let (true, Slot::Holding(item)) = (controls.item, k.slot) {
            k.slot = Slot::Empty;
            r.use_item(w, i, k, item, &c.t);
        }
        let prev_s = k.s;
        let ev = kart::step(k, controls, &c.t, f.steer, &r.track, c.gravity, c.dt);
        if ev.hopped {
            r.push_event(RaceEvent::Hop(i));
        }
        if let Some(tier) = ev.tier_up {
            r.push_event(RaceEvent::DriftTier(i, tier));
        }
        if let Some(tier) = ev.boost_tier {
            r.push_event(RaceEvent::Boost(i, tier));
        }
        if let Some(loss) = ev.wall {
            r.push_event(RaceEvent::Wall(i, loss));
            r.add_trauma(k.player, (loss / 15.0).min(0.5));
        }
        // Free roam drives any way round: no wrong-way warning.
        if !r.free_roam {
            wrong_way(k, &r.track, &c.t, c.dt);
        }
        r.gates(i, k, prev_s);
    }
}

/// Stage 4, `kart_racer.boxes`: karts pick up item boxes (the roulette
/// starts), hidden boxes count down to respawn, rolling items settle.
pub fn boxes(w: &World, c: &Ctx) {
    if !stepping(w) {
        return;
    }
    let mut s = Sim::load(w);
    s.collect_boxes(&c.t, c.dt);
    s.store();
}

/// Stage 5, `kart_racer.bumps`: kart-against-kart pushes, pair by pair in
/// grid order.
pub fn bumps(w: &World, c: &Ctx) {
    if !stepping(w) {
        return;
    }
    let mut s = Sim::load(w);
    s.bumps(&c.t);
    s.store();
}

/// Stage 6, `kart_racer.items`: Goo Blobs, Boing Bombs, Seeker Stars and
/// blasts (oldest first), then shields wear off.
pub fn items(w: &World, c: &Ctx) {
    if !stepping(w) {
        return;
    }
    let mut s = Sim::load(w);
    s.update_goo(&c.t, c.dt);
    s.update_bombs(&c.t, c.gravity, c.dt);
    s.update_seekers(&c.t, c.dt);
    s.update_blasts(c.dt);
    for k in &mut s.karts {
        k.shield = (k.shield - c.dt).max(0.0);
    }
    s.store();
}

/// Stage 7, `kart_racer.standings`: places and the end of the race. True
/// when the attract race is over and wants a fresh grid (the caller resets).
pub fn standings(w: &World, _c: &Ctx) -> bool {
    if !stepping(w) {
        return false;
    }
    let mut s = Sim::load(w);
    s.update_order();
    let restart = s.check_end();
    s.store();
    restart
}

fn run_stages(w: &World, c: &Ctx, f: &Fns) -> bool {
    flow(w, c);
    control(w, c, f);
    step_karts(w, c, f);
    boxes(w, c);
    bumps(w, c);
    items(w, c);
    standings(w, c)
}

/// One whole fixed tick, the same stages in the same order as the kit's
/// systems (tests and tools).
pub fn tick(world: &mut World, c: &Ctx, f: &Fns) {
    handle_requests(world, &c.t, c.counts);
    if run_stages(world, c, f) {
        reset_mut(world, Phase::Title, 0, &c.t, c.counts);
    }
}

/// One whole fixed tick through `&World` (capture moments): no requests are
/// applied and a reset reuses the kart entities.
pub fn tick_shared(world: &World, c: &Ctx, f: &Fns) -> Result<(), String> {
    if run_stages(world, c, f) {
        reset(world, Phase::Title, 0, &c.t, c.counts)?;
    }
    Ok(())
}

/// A Boing Bomb blast at `pos`: spins out every kart in range.
pub fn explode(world: &World, pos: [f32; 3], t: &Tune) {
    let mut s = Sim::load(world);
    s.explode(pos, t);
    s.store();
}

/// A Seeker Star fired by kart `owner` at kart `target`, starting `gap`
/// metres behind it on the track (capture moments).
pub fn launch_seeker(world: &World, owner: usize, target: usize, gap: f32) {
    let mut s = Sim::load(world);
    let (ts, tl) = (s.karts[target].s, s.karts[target].lateral);
    let from = s.r.track.wrap_s(ts - gap);
    let p = s.r.track.point_at(from, tl);
    let seeker = Seeker {
        s: from,
        lateral: tl,
        pos: [p[0], p[1] + 1.2, p[2]],
        owner,
        target: Some(target),
        life: 12.0,
    };
    let w = s.w;
    s.r.pools.spawn(w, Pool::Seeker, seeker);
    s.store();
}

// ---- read view ------------------------------------------------------------------

/// The race and copies of its objects for one frame (drawing, HUD, tests):
/// karts in grid order, boxes in row order, items in flight oldest first.
/// Derefs to [`Race`].
pub struct RaceView<'w> {
    race: RefMut<'w, Race>,
    pub karts: Vec<Kart>,
    pub boxes: Vec<ItemBox>,
    pub goos: Vec<Goo>,
    pub bombs: Vec<Bomb>,
    pub seekers: Vec<Seeker>,
    pub blasts: Vec<Blast>,
}

impl<'w> RaceView<'w> {
    /// Borrows the [`Race`] resource mutably until dropped.
    pub fn new(w: &'w World) -> Self {
        let race = w.res_mut::<Race>();
        let karts = kart_list(w, &race.grid);
        let boxes = race.boxes.iter().filter_map(|&e| w.get::<ItemBox>(e).map(|b| *b)).collect();
        fn vals<T: Component + Clone>(race: &Race, w: &World, p: Pool) -> Vec<T> {
            race.pools.active::<T>(w, p).into_iter().map(|(_, v)| v).collect()
        }
        let goos = vals::<Goo>(&race, w, Pool::Goo);
        let bombs = vals::<Bomb>(&race, w, Pool::Bomb);
        let seekers = vals::<Seeker>(&race, w, Pool::Seeker);
        let blasts = vals::<Blast>(&race, w, Pool::Blast);
        Self {
            race,
            karts,
            boxes,
            goos,
            bombs,
            seekers,
            blasts,
        }
    }

    /// Results by place: finish times, and once the podium has latched the
    /// places, an on-pace time for every kart still racing.
    pub fn results(&self) -> Vec<ResultRow> {
        self.race
            .order
            .iter()
            .enumerate()
            .filter_map(|(place, &i)| {
                let k = self.karts.get(i)?;
                Some(ResultRow {
                    place: place + 1,
                    kart: i,
                    name: self.race.roster[k.spec].name.clone(),
                    player: k.player,
                    // The latched times once the podium is up, so the panel
                    // never changes under the player.
                    time: self.race.final_times.get(i).copied().flatten().or(k.finished),
                    color: self.race.roster[k.spec].color,
                })
            })
            .collect()
    }
}

impl Deref for RaceView<'_> {
    type Target = Race;
    fn deref(&self) -> &Race {
        &self.race
    }
}

impl DerefMut for RaceView<'_> {
    fn deref_mut(&mut self) -> &mut Race {
        &mut self.race
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::roster_from_json;

    fn world(t: &Tune) -> World {
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let odds = OddsTable::from_json(include_str!("../data/items.json")).unwrap();
        let roster = roster_from_json(include_str!("../data/karts.json")).unwrap();
        let mut w = World::new();
        w.insert_resource(Race::new(track, odds, roster, Counts::default()));
        spawn_objects(&mut w, t, Counts::default());
        w
    }

    fn steer(x: f32) -> f32 {
        (0.25 + x * 4.0).min(1.0)
    }

    fn rubber(gap: f32, dist: f32, strength: f32) -> f32 {
        (gap / dist).clamp(-1.0, 1.0) * strength
    }

    const FNS: Fns<'static> = Fns {
        steer: &steer,
        rubber: &rubber,
    };

    const DT: f32 = 1.0 / 60.0;

    fn ctx(t: &Tune, inputs: [Controls; 2]) -> Ctx {
        Ctx {
            t: *t,
            counts: Counts::default(),
            gravity: 9.81 * 1.8,
            dt: DT,
            inputs,
        }
    }

    fn run(w: &mut World, t: &Tune, ticks: u32, input: [Controls; 2]) {
        for _ in 0..ticks {
            tick(w, &ctx(t, input), &FNS);
        }
    }

    fn karts(w: &World) -> Vec<Kart> {
        let r = w.res::<Race>();
        kart_list(w, &r.grid)
    }

    fn request(w: &World, q: Request) {
        w.res_mut::<Race>().request(q);
    }

    #[test]
    fn grid_puts_humans_at_the_back() {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 2 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        {
            let r = w.res::<Race>();
            assert_eq!(r.phase, Phase::Countdown);
            assert_eq!(r.grid.len(), 8);
            assert_eq!(r.player_kart(0), Some(6));
            assert_eq!(r.player_kart(1), Some(7));
            assert_eq!(r.player_kart(2), None);
        }
        let ks = karts(&w);
        assert_eq!(ks[6].player, Some(0));
        assert_eq!(ks[7].player, Some(1));
        let hw = w.res::<Race>().track.half_width;
        assert!(ks.iter().all(|k| k.next_gate == 0 && k.lateral.abs() < hw));
        // The countdown holds everyone still.
        let p = ks[0].pos;
        run(&mut w, &t, 60, [Controls { throttle: 1.0, ..Default::default() }; 2]);
        assert_eq!(karts(&w)[0].pos, p);
        run(&mut w, &t, 200, [Controls::default(); 2]);
        let r = w.res::<Race>();
        assert_eq!(r.phase, Phase::Racing);
        assert!(r.events.contains(&RaceEvent::Go));
    }

    /// Start a one-player race, hold `idle` ticks, then press the throttle
    /// until GO; the player's kart right after GO.
    fn launch(idle: u32) -> Kart {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 1 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        run(&mut w, &t, idle, [Controls::default(); 2]);
        let gas = [Controls { throttle: 1.0, ..Default::default() }; 2];
        for _ in 0..400 {
            run(&mut w, &t, 1, gas);
            if w.res::<Race>().phase == Phase::Racing {
                break;
            }
        }
        let i = w.res::<Race>().player_kart(0).unwrap();
        karts(&w)[i].clone()
    }

    #[test]
    fn throttle_just_before_go_gives_a_start_boost() {
        let t = Tune::default();
        // 3 s countdown at 60 Hz: pressing with about 0.3 s left.
        let good = launch(160);
        assert_eq!(good.launch, Launch::Primed);
        assert!(good.boost > t.start_boost_s - 0.05, "boost {}", good.boost);
        // Revving from the start of the countdown: no boost.
        let early = launch(0);
        assert_eq!(early.launch, Launch::Early);
        assert_eq!(early.boost, 0.0);
    }

    #[test]
    fn every_object_is_an_entity_and_a_reset_respawns_the_karts() {
        let t = Tune::default();
        let mut w = world(&t);
        let (grid, boxes, pooled) = {
            let r = w.res::<Race>();
            (r.grid.clone(), r.boxes.len(), r.pools.len())
        };
        assert_eq!(grid.len(), 8);
        assert_eq!(boxes, 12);
        assert_eq!(pooled, GOO_POOL + BOMB_POOL + SEEKER_POOL + BLAST_POOL);
        assert_eq!(w.count::<Kart>(), 8);
        assert_eq!(w.count::<AiState>(), 8);
        assert_eq!(w.count::<Controls>(), 8);
        assert_eq!(w.count::<ItemBox>(), 12);
        assert_eq!(w.count::<InFlight>(), pooled);
        assert_eq!(w.entity_count(), 8 + 12 + pooled);
        // A new race makes fresh kart entities; the old ones are gone.
        request(&w, Request::Start { players: 1 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        let new_grid = w.res::<Race>().grid.clone();
        assert!(grid.iter().all(|&e| !w.is_alive(e)));
        assert!(new_grid.iter().all(|&e| w.is_alive(e)));
        assert_eq!(w.entity_count(), 8 + 12 + pooled);
        // A different field size through &World is refused, not half done.
        let small = Counts { karts: 4, ..Counts::default() };
        assert!(reset(&w, Phase::Racing, 1, &t, small).is_err());
        assert_eq!(w.res::<Race>().grid.len(), 8);
        reset_mut(&mut w, Phase::Racing, 1, &t, small);
        assert_eq!(w.count::<Kart>(), 4);
        assert_eq!(w.entity_count(), 4 + 12 + pooled);
    }

    #[test]
    fn all_ai_race_finishes_with_laps_and_gates_in_order() {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 0 });
        let mut laps = Vec::new();
        let mut ticks = 0;
        while w.res::<Race>().phase != Phase::Finished && ticks < 60 * 400 {
            run(&mut w, &t, 1, [Controls::default(); 2]);
            for e in w.res_mut::<Race>().events.drain(..) {
                if let RaceEvent::Lap(k, l) = e {
                    laps.push((k, l));
                }
            }
            ticks += 1;
        }
        let view = RaceView::new(&w);
        assert_eq!(view.phase, Phase::Finished, "race did not finish in {ticks} ticks");
        assert_eq!(view.finish_order.len(), 8);
        let times: Vec<f32> = view.finish_order.iter().map(|&i| view.karts[i].finished.unwrap()).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
        // Every kart started laps 2 and 3, and nobody skipped gates.
        for k in 0..8 {
            let mine: Vec<u32> = laps.iter().filter(|l| l.0 == k).map(|l| l.1).collect();
            assert_eq!(mine, [2, 3], "kart {k}");
        }
        let res = view.results();
        assert_eq!(res[0].place, 1);
        assert_eq!(res[0].kart, view.finish_order[0]);
        // A real race takes a while (three laps of ~700 m).
        assert!(times[0] > 60.0, "{}", times[0]);
    }


    /// The deepest overlap (m) of any two karts' footprints.
    fn deepest_overlap(karts: &[Kart], radius: f32) -> f32 {
        let mut worst = 0.0f32;
        for i in 0..karts.len() {
            for j in (i + 1)..karts.len() {
                if (karts[i].pos[1] - karts[j].pos[1]).abs() > 1.5 {
                    continue;
                }
                if let Some(c) = crate::hull::contact(&karts[i].hull(radius), &karts[j].hull(radius)) {
                    worst = worst.max(c.depth);
                }
            }
        }
        worst
    }

    #[test]
    fn a_nose_in_a_tail_is_pushed_apart() {
        let t = Tune::default();
        let w = world(&t);
        let mut s = Sim::load(&w);
        // Two karts nose to tail 2.8 m apart (plain circles would call them
        // clear), the rear one faster.
        let (a, b) = (0, 1);
        s.karts[a].pos = [0.0, 0.0, 0.0];
        s.karts[a].yaw = 0.0;
        s.karts[a].slip = 0.0;
        s.karts[b].slip = 0.0;
        s.karts[a].speed = 8.0;
        s.karts[b].pos = [0.0, 0.0, 2.8];
        s.karts[b].yaw = 0.0;
        s.karts[b].speed = 5.0;
        for k in s.karts.iter_mut().skip(2) {
            k.pos = [500.0, 0.0, 500.0 + k.spec as f32 * 20.0];
        }
        assert!(deepest_overlap(&s.karts, t.kart_radius) > 0.25);
        s.bumps(&t);
        assert!(deepest_overlap(&s.karts, t.kart_radius) < 0.02, "{}", deepest_overlap(&s.karts, t.kart_radius));
        // Pushed apart along the kart's length, rear one slowed.
        assert!(s.karts[b].pos[2] - s.karts[a].pos[2] > 3.0);
        assert!(s.karts[a].speed < 8.0 && s.karts[b].speed > 5.0);
    }

    #[test]
    fn side_by_side_karts_are_not_pushed_while_clear() {
        let t = Tune::default();
        let w = world(&t);
        let mut s = Sim::load(&w);
        s.karts[0].pos = [0.0, 0.0, 0.0];
        s.karts[1].pos = [2.4, 0.0, 0.5];
        s.karts[0].yaw = 0.0;
        s.karts[1].yaw = 0.0;
        s.karts[0].slip = 0.0;
        s.karts[1].slip = 0.0;
        for k in s.karts.iter_mut().skip(2) {
            k.pos = [500.0, 0.0, 500.0 + k.spec as f32 * 20.0];
        }
        let before = (s.karts[0].pos, s.karts[1].pos);
        s.bumps(&t);
        assert_eq!((s.karts[0].pos, s.karts[1].pos), before);
    }

    #[test]
    fn karts_in_a_pack_never_sink_into_each_other() {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 0 });
        let (mut worst, mut ticks) = (0.0f32, 0);
        while ticks < 60 * 100 {
            run(&mut w, &t, 1, [Controls::default(); 2]);
            let race = RaceView::new(&w);
            worst = worst.max(deepest_overlap(&race.karts, t.kart_radius));
            ticks += 1;
        }
        // The second pass settles what one push leaves: never more than a
        // few centimetres of footprint overlap at the end of a tick (the
        // footprints are a little roomier than the models).
        assert!(worst < 0.12, "karts overlapped by {worst} m");
    }
    #[test]
    fn a_lap_needs_every_gate() {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 1 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        let mut r = w.res_mut::<Race>();
        let i = r.player_kart(0).unwrap();
        let mut k = kart_list(&w, &r.grid)[i].clone();
        let len = r.track.length;
        // Jump across the line: lap 1 starts.
        k.next_gate = 0;
        r.phase = Phase::Racing;
        let place = |r: &mut Race, k: &mut Kart, s: f32| {
            let prev = k.s;
            k.pos = r.track.point_at(s, 0.0);
            k.relocate(&r.track);
            r.gates(i, k, prev);
        };
        place(&mut r, &mut k, len - 1.0);
        place(&mut r, &mut k, 1.0);
        assert_eq!(k.next_gate, 1);
        // Cut back to the line without the other gates: no lap.
        place(&mut r, &mut k, len - 1.0);
        place(&mut r, &mut k, 1.0);
        assert_eq!(k.laps_done, 0);
        // Pass every gate in order, then the line.
        let mut s = 1.0;
        while s < len - 1.0 {
            s += 10.0;
            place(&mut r, &mut k, s.min(len - 1.0));
        }
        place(&mut r, &mut k, 1.0);
        assert_eq!(k.laps_done, 1);
        assert_eq!(k.next_gate, 1);
    }

    #[test]
    fn deterministic_for_the_same_inputs() {
        let t = Tune::default();
        let go = |players| {
            let mut w = world(&t);
            request(&w, Request::Start { players });
            run(&mut w, &t, 60 * 20, [Controls { throttle: 1.0, steer: 0.1, ..Default::default() }; 2]);
            let v = RaceView::new(&w);
            let karts: Vec<_> = v.karts.iter().map(|k| (k.pos, k.yaw, k.slot)).collect();
            (karts, v.goos.clone(), v.bombs.clone(), v.seekers.clone())
        };
        assert_eq!(go(1), go(1));
        assert_eq!(go(0), go(0));
    }

    #[test]
    fn items_hit_and_shields_block() {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 0 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        w.res_mut::<Race>().phase = Phase::Racing;
        let grid = w.res::<Race>().grid.clone();
        // A bomb dropped right on kart 1 explodes and spins it out.
        let p = karts(&w)[1].pos;
        explode(&w, p, &t);
        assert!(karts(&w)[1].spin > 0.0);
        assert_eq!(RaceView::new(&w).blasts.len(), 1);
        // A shielded kart only loses its shield.
        {
            let mut k = w.get_mut::<Kart>(grid[2]).unwrap();
            k.spin = 0.0;
            k.shield = 5.0;
        }
        let p2 = karts(&w)[2].pos;
        explode(&w, p2, &t);
        assert!(w.res::<Race>().events.iter().any(|e| matches!(e, RaceEvent::ShieldPop(2))));
        // Goo behind kart 3 catches the kart that drives into it.
        let goo_pos = karts(&w)[4].pos;
        {
            let mut r = w.res_mut::<Race>();
            r.pools.spawn(&w, Pool::Goo, Goo { pos: goo_pos, life: 5.0, owner: 3, grace: 0.0 });
        }
        {
            let mut k = w.get_mut::<Kart>(grid[4]).unwrap();
            k.spin = 0.0;
            k.shield = 0.0;
        }
        assert_eq!(RaceView::new(&w).goos.len(), 1);
        let mut s = Sim::load(&w);
        s.update_goo(&t, DT);
        s.store();
        assert!(RaceView::new(&w).goos.is_empty());
        assert!(karts(&w)[4].spin > 0.0);
    }

    #[test]
    fn a_full_pool_reuses_its_oldest_item() {
        let t = Tune::default();
        let w = world(&t);
        let mut r = w.res_mut::<Race>();
        for n in 0..GOO_POOL + 3 {
            let goo = Goo { pos: [n as f32, 0.0, 0.0], life: 5.0, owner: 0, grace: 0.0 };
            r.pools.spawn(&w, Pool::Goo, goo);
        }
        let live: Vec<f32> = r.pools.active::<Goo>(&w, Pool::Goo).iter().map(|(_, g)| g.pos[0]).collect();
        assert_eq!(live.len(), GOO_POOL);
        assert_eq!(live[0], 3.0);
        assert_eq!(*live.last().unwrap(), (GOO_POOL + 2) as f32);
    }

    #[test]
    fn item_boxes_roll_an_item() {
        let t = Tune::default();
        let mut w = world(&t);
        request(&w, Request::Start { players: 1 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        w.res_mut::<Race>().phase = Phase::Racing;
        let (e, b0) = {
            let r = w.res::<Race>();
            (r.grid[r.player_kart(0).unwrap()], r.boxes[0])
        };
        let bp = w.get::<ItemBox>(b0).unwrap().pos;
        w.get_mut::<Kart>(e).unwrap().pos = [bp[0], bp[1] - 1.0, bp[2]];
        let collect = |w: &World| {
            let mut s = Sim::load(w);
            s.collect_boxes(&t, DT);
            s.store();
        };
        collect(&w);
        assert!(matches!(w.get::<Kart>(e).unwrap().slot, Slot::Rolling { .. }));
        assert!(w.get::<ItemBox>(b0).unwrap().hidden > 0.0);
        for _ in 0..(t.roulette_s * 60.0) as u32 + 2 {
            collect(&w);
        }
        assert!(matches!(w.get::<Kart>(e).unwrap().slot, Slot::Holding(_)));
    }

    #[test]
    fn pause_freezes_and_requests_switch_phase() {
        let t = Tune::default();
        let mut w = world(&t);
        // No pausing the attract race.
        request(&w, Request::Pause(true));
        run(&mut w, &t, 1, [Controls::default(); 2]);
        assert!(!w.res::<Race>().paused);
        request(&w, Request::Start { players: 1 });
        run(&mut w, &t, 1, [Controls::default(); 2]);
        request(&w, Request::TogglePause);
        run(&mut w, &t, 1, [Controls::default(); 2]);
        assert!(w.res::<Race>().paused);
        let cd = w.res::<Race>().countdown;
        run(&mut w, &t, 30, [Controls::default(); 2]);
        assert_eq!(w.res::<Race>().countdown, cd);
        request(&w, Request::Title);
        run(&mut w, &t, 1, [Controls::default(); 2]);
        assert_eq!(w.res::<Race>().phase, Phase::Title);
        assert!(!w.res::<Race>().paused);
        assert!(karts(&w).iter().all(|k| k.player.is_none()));
    }

    #[test]
    fn the_shared_tick_matches_the_full_tick() {
        // Capture moments step through &World; same stages, same result.
        let t = Tune::default();
        let input = [Controls { throttle: 1.0, ..Default::default() }; 2];
        let mut a = world(&t);
        let b = world(&t);
        reset_mut(&mut a, Phase::Racing, 1, &t, Counts::default());
        reset(&b, Phase::Racing, 1, &t, Counts::default()).unwrap();
        for _ in 0..60 * 15 {
            tick(&mut a, &ctx(&t, input), &FNS);
            tick_shared(&b, &ctx(&t, input), &FNS).unwrap();
        }
        assert_eq!(karts(&a), karts(&b));
    }
}
