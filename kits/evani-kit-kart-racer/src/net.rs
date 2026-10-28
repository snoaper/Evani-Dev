//! Online play (feature `net`): the race as a lockstep game for evani-net.
//!
//! * [`NetRace`]: the race rules alone (no drawing) in their own small ECS
//!   world, stepped one [`Frame`] per tick. The referee runs one; every
//!   player runs a confirmed copy and a predicted copy; it is the same code
//!   as single-player (`race::tick`), with [`race::Online`] supplying every
//!   kart's controls from the input stream.
//! * [`KartInput`]: what a player sends, 3 bytes per tick (steer, throttle,
//!   buttons: drift, item, brake). Never a position, a speed or a lap.
//! * [`KartBrains`]: the AI racers, run only by the referee from its own
//!   state and its own seeded generator; their inputs go into the same
//!   stream, so clients never run them.
//! * The online flow is part of the rules (so every copy agrees): a race
//!   starts at once with the AI in every seat; a human takes a seat's kart
//!   (the AI gets it back when they leave); the first human into an all-AI
//!   session gets a fresh grid; after the podium the next race starts by
//!   itself. No idle kick, no pause.
//! * [`save_world`] / [`load_world`]: every field the race reads, exactly
//!   (floats as bits), so a loaded copy steps on bit for bit.
//! * [`NetRace::show`]: copy a copy into the game's own world for drawing,
//!   seen from one kart, with only the events not shown yet.

use std::collections::VecDeque;

use evani_core::curve::Curve;
use evani_core::ecs::World;
use evani_core::rng::Pcg32;
use evani_net::lockstep::{Brains, Frame, LockstepGame};
use evani_net::wire::{Reader, WireError, Writer};

use crate::Tune;
use crate::ai::{self, AiState};
use crate::items::{Blast, Bomb, Goo, Item, ItemBox, OddsTable, Seeker, Slot};
use crate::kart::{Controls, Kart, Launch};
use crate::race::{self, Counts, Ctx, Fns, InFlight, Online, Phase, Race, RaceEvent};
use crate::slipstream::Slip;
use crate::track::Track;

/// Seats (karts) in an online race.
pub const SEATS: usize = 8;
/// Seconds the podium shows before the next online race starts by itself.
pub const PODIUM_HOLD_S: f32 = 8.0;
/// The fixed step.
pub const DT: f32 = 1.0 / 60.0;
const STATE_VERSION: u8 = 2;

/// One seat's input for one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KartInput {
    /// -127 (left) .. 127 (right).
    pub steer: i8,
    /// 0 .. 255.
    pub throttle: u8,
    /// Bit 0 drift held, bit 1 item pressed this tick, bit 2 brake.
    pub buttons: u8,
}

impl KartInput {
    pub const DRIFT: u8 = 1;
    pub const ITEM: u8 = 2;
    pub const BRAKE: u8 = 4;

    pub fn from_controls(c: &Controls) -> Self {
        let mut buttons = 0;
        if c.drift {
            buttons |= Self::DRIFT;
        }
        if c.item {
            buttons |= Self::ITEM;
        }
        if c.brake > 0.5 {
            buttons |= Self::BRAKE;
        }
        Self {
            steer: (c.steer.clamp(-1.0, 1.0) * 127.0).round() as i8,
            throttle: (c.throttle.clamp(0.0, 1.0) * 255.0).round() as u8,
            buttons,
        }
    }

    pub fn controls(self) -> Controls {
        Controls {
            steer: f32::from(self.steer.max(-127)) / 127.0,
            throttle: f32::from(self.throttle) / 255.0,
            brake: if self.buttons & Self::BRAKE != 0 { 1.0 } else { 0.0 },
            drift: self.buttons & Self::DRIFT != 0,
            item: self.buttons & Self::ITEM != 0,
        }
    }
}

/// The online tuning: the kit's defaults plus its baked `params.json`, the
/// same numbers in every copy (live tuning never reaches an online race).
pub fn net_tune() -> Tune {
    let mut t = Tune::default();
    if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(include_str!("../params.json")) {
        for (k, v) in m {
            if let Some(v) = v.as_f64() {
                t.set_by_name(&k, v as f32);
            }
        }
    }
    t
}

/// The steering curve and rubber band as plain functions (the registry's
/// defaults: `kit.kart_racer.steer_by_speed`, `kit.kart_racer.rubber_band`).
fn steer_curve() -> Option<Curve> {
    Curve::new(&[[0.0, 0.25], [0.2, 0.8], [0.5, 1.0], [1.0, 0.85], [1.4, 0.75]]).ok()
}

fn rubber(gap: f32, dist: f32, strength: f32) -> f32 {
    (gap / dist).clamp(-1.0, 1.0) * strength
}

/// The race rules for one online session.
pub struct NetRace {
    world: World,
    tune: Tune,
    counts: Counts,
    gravity: f32,
    curve: Option<Curve>,
    /// Events of recent ticks (drawing and sound only; not in the state).
    recent: VecDeque<(u64, RaceEvent)>,
    tick: u64,
}

impl NetRace {
    /// A session from the host's seed. `config` (optional): byte 0 = laps
    /// (1 to 9; default 3).
    pub fn new(seed: u64, config: &[u8]) -> Result<Self, String> {
        let track = Track::from_json(crate::TRACK_JSON)?;
        let odds = OddsTable::from_json(crate::ITEMS_JSON)?;
        let roster = ai::roster_from_json(crate::KARTS_JSON)?;
        let laps = config.first().map_or(3, |&l| u32::from(l.clamp(1, 9)));
        let counts = Counts {
            karts: SEATS,
            laps,
            checkpoints: 8,
        };
        let tune = net_tune();
        let mut world = World::new();
        let mut r = Race::new(track, odds, roster, counts);
        r.seed = race::RACE_SEED ^ seed;
        r.online = Some(Online {
            humans: 0,
            controls: vec![Controls::default(); SEATS],
        });
        world.insert_resource(r);
        race::spawn_objects(&mut world, &tune, counts);
        let mut g = Self {
            world,
            gravity: tune.gravity * tune.gravity_scale,
            tune,
            counts,
            curve: steer_curve(),
            recent: VecDeque::new(),
            tick: 0,
        };
        g.new_race();
        Ok(g)
    }

    /// The race resource (read only).
    pub fn race(&self) -> core::cell::Ref<'_, Race> {
        self.world.res::<Race>()
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn tune(&self) -> &Tune {
        &self.tune
    }

    fn ai_skill(&self, kart: usize) -> f32 {
        let r = self.world.res::<Race>();
        let spec = r.grid.get(kart).and_then(|&e| self.world.get::<Kart>(e).map(|k| k.spec)).unwrap_or(0);
        let skill = r.roster.get(spec).map_or(0.5, |s| s.skill);
        1.0 + (skill - 0.5) * 2.0 * self.tune.ai_skill_spread
    }

    /// Humans drive at the plain speed cap; the AI keeps its skill spread.
    fn apply_seats(&mut self, humans: u32) {
        let grid = self.world.res::<Race>().grid.clone();
        for (i, &e) in grid.iter().enumerate() {
            let m = if i < 32 && humans & (1 << i) != 0 { 1.0 } else { self.ai_skill(i) };
            if let Some(mut k) = self.world.get_mut::<Kart>(e) {
                k.skill_mult = m;
            }
        }
    }

    fn new_race(&mut self) {
        race::reset_mut(&mut self.world, Phase::Countdown, 0, &self.tune, self.counts);
        let humans = self.world.res::<Race>().online.as_ref().map_or(0, |o| o.humans);
        self.apply_seats(humans);
    }

    /// Copy this race into `display` (the game's own world, which has the
    /// kit's entities) for drawing, seen from kart `follow` (the player's
    /// own, or the one a spectator follows), with the events of ticks after
    /// `shown` (returns the tick now shown).
    pub fn show(&self, display: &mut World, follow: Option<usize>, shown: u64) -> Result<u64, WireError> {
        let mut w = Writer::with_capacity(4096);
        save_world(&self.world, &mut w);
        load_world(display, &mut Reader::new(w.as_bytes()))?;
        let grid = display.res::<Race>().grid.clone();
        {
            let mut r = display.res_mut::<Race>();
            r.events = self.recent.iter().filter(|(t, _)| *t > shown).map(|(_, e)| *e).collect();
            match follow.filter(|&f| f < grid.len()) {
                Some(f) => {
                    r.players = 1;
                    r.view_karts = Some([f, f]);
                }
                None => {
                    r.players = 0;
                    r.view_karts = None;
                }
            }
        }
        for (i, &e) in grid.iter().enumerate() {
            if let Some(mut k) = display.get_mut::<Kart>(e) {
                k.player = (Some(i) == follow).then_some(0);
            }
        }
        Ok(self.tick)
    }
}

impl LockstepGame for NetRace {
    type Input = KartInput;
    const SLOTS: usize = SEATS;
    /// "Sugarpeak lockstep 1": bump when the rules or the input change.
    const PROTOCOL: u32 = 0x5350_4B01;

    fn write_input(i: &KartInput, w: &mut Writer) {
        w.u8(i.steer as u8);
        w.u8(i.throttle);
        w.u8(i.buttons);
    }

    fn read_input(r: &mut Reader<'_>) -> Result<KartInput, WireError> {
        Ok(KartInput {
            steer: r.u8()? as i8,
            throttle: r.u8()?,
            buttons: r.u8()?,
        })
    }

    fn sanitize(i: KartInput) -> KartInput {
        KartInput {
            steer: i.steer.max(-127),
            throttle: i.throttle,
            buttons: i.buttons & (KartInput::DRIFT | KartInput::ITEM | KartInput::BRAKE),
        }
    }

    fn held(i: KartInput) -> KartInput {
        KartInput {
            buttons: i.buttons & !KartInput::ITEM,
            ..i
        }
    }

    fn step(&mut self, f: &Frame<KartInput>) {
        let before = self.world.res::<Race>().online.as_ref().map_or(0, |o| o.humans);
        {
            let mut r = self.world.res_mut::<Race>();
            if let Some(on) = r.online.as_mut() {
                on.humans = f.humans;
                on.controls.clear();
                on.controls.extend(f.inputs.iter().map(|i| i.controls()));
            }
            r.events.clear();
        }
        if before != f.humans {
            let phase = self.world.res::<Race>().phase;
            if before == 0 && phase != Phase::Countdown {
                // The first human into an all-AI session gets a fresh grid.
                self.new_race();
            } else {
                self.apply_seats(f.humans);
            }
        }
        let curve = &self.curve;
        let steer = |x: f32| curve.as_ref().map_or(1.0, |c| c.eval(x));
        let fns = Fns {
            steer: &steer,
            rubber: &rubber,
        };
        let c = Ctx {
            t: self.tune,
            counts: self.counts,
            gravity: self.gravity,
            dt: DT,
            inputs: [Controls::default(); 2],
        };
        race::tick(&mut self.world, &c, &fns);
        let next = {
            let r = self.world.res::<Race>();
            r.phase == Phase::Finished && r.phase_time >= crate::finish::CELEBRATE_S + PODIUM_HOLD_S
        };
        if next {
            self.new_race();
        }
        self.tick = f.tick;
        let events: Vec<RaceEvent> = core::mem::take(&mut self.world.res_mut::<Race>().events);
        for e in events {
            self.recent.push_back((f.tick, e));
        }
        while self.recent.len() > 256 {
            self.recent.pop_front();
        }
    }

    fn save(&self, w: &mut Writer) {
        w.varint(self.tick);
        save_world(&self.world, w);
    }

    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), WireError> {
        let tick = r.varint()?;
        load_world(&mut self.world, r)?;
        self.tick = tick;
        // The session's settings (laps) are the loaded race's, not this
        // copy's defaults: the next race starts with them too.
        self.counts = self.world.res::<Race>().counts;
        self.recent.clear();
        Ok(())
    }

    /// Phase, race number, then every kart by place: kart number, human
    /// (0/1), finished (0/1) and its time in seconds.
    fn results(&self, w: &mut Writer) {
        let r = self.world.res::<Race>();
        let karts = race::kart_list(&self.world, &r.grid);
        w.u8(r.phase as u8);
        w.varint(r.race_no);
        w.varint(r.order.len() as u64);
        for &i in &r.order {
            w.u8(i as u8);
            w.u8(u8::from(r.online.as_ref().is_some_and(|o| o.human(i))));
            match karts.get(i).and_then(|k| k.finished) {
                Some(t) => {
                    w.u8(1);
                    w.f32(t);
                }
                None => {
                    w.u8(0);
                    w.f32(0.0);
                }
            }
        }
    }
}

/// The AI racers (the referee's): one brain per seat and their own
/// generator, seeded from the session.
pub struct KartBrains {
    ai: Vec<AiState>,
    rng: Pcg32,
    cache: Option<(u64, Vec<Kart>)>,
}

impl KartBrains {
    pub fn new(seed: u64) -> Self {
        Self {
            ai: vec![AiState::default(); SEATS],
            rng: Pcg32::new(seed, 0xB0B5),
            cache: None,
        }
    }
}

impl Brains<NetRace> for KartBrains {
    fn think(&mut self, g: &NetRace, seat: usize, tick: u64) -> KartInput {
        let r = g.world.res::<Race>();
        if r.phase == Phase::Countdown {
            return KartInput::default();
        }
        if self.cache.as_ref().is_none_or(|c| c.0 != tick) {
            self.cache = Some((tick, race::kart_list(&g.world, &r.grid)));
        }
        let Some((_, karts)) = self.cache.as_ref() else {
            return KartInput::default();
        };
        let Some(k) = karts.get(seat) else {
            return KartInput::default();
        };
        let Some(spec) = r.roster.get(k.spec) else {
            return KartInput::default();
        };
        let around = ai::surroundings(karts, seat, &r.track);
        let noise = self.rng.next_f32();
        let Some(st) = self.ai.get_mut(seat) else {
            return KartInput::default();
        };
        let c = ai::drive(k, spec, st, around, &r.track, &g.tune, noise, DT);
        KartInput::from_controls(&c)
    }

    fn save(&self, w: &mut Writer) {
        let (s, inc) = self.rng.to_raw();
        w.u64_fixed(s);
        w.u64_fixed(inc);
        w.varint(self.ai.len() as u64);
        for a in &self.ai {
            w.f32(a.stuck);
            w.f32(a.reverse);
            w.f32(a.item_wait);
            w.u8(a.armed.map_or(0, |i| item_tag(i) + 1));
        }
    }

    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), WireError> {
        let s = r.u64_fixed()?;
        let inc = r.u64_fixed()?;
        let n = r.list_len(13)?;
        let mut ai = Vec::with_capacity(n);
        for _ in 0..n {
            ai.push(AiState {
                stuck: r.f32()?,
                reverse: r.f32()?,
                item_wait: r.f32()?,
                armed: match r.u8()? {
                    0 => None,
                    t => Some(item_of(t - 1)?),
                },
            });
        }
        self.rng = Pcg32::from_raw(s, inc);
        self.ai = ai;
        self.cache = None;
        Ok(())
    }
}

// ---- exact save and load -------------------------------------------------------

fn item_tag(i: Item) -> u8 {
    Item::ALL.iter().position(|&x| x == i).unwrap_or(0) as u8
}

fn item_of(t: u8) -> Result<Item, WireError> {
    Item::ALL.get(usize::from(t)).copied().ok_or(WireError::Invalid)
}

fn phase_of(t: u8) -> Result<Phase, WireError> {
    Ok(match t {
        0 => Phase::Title,
        1 => Phase::Countdown,
        2 => Phase::Racing,
        3 => Phase::Finished,
        _ => return Err(WireError::Invalid),
    })
}

fn bool_of(r: &mut Reader<'_>) -> Result<bool, WireError> {
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(WireError::Invalid),
    }
}

fn v3(w: &mut Writer, v: [f32; 3]) {
    for x in v {
        w.f32(x);
    }
}

fn rv3(r: &mut Reader<'_>) -> Result<[f32; 3], WireError> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn opt_f32(w: &mut Writer, v: Option<f32>) {
    match v {
        Some(x) => {
            w.u8(1);
            w.f32(x);
        }
        None => w.u8(0),
    }
}

fn ropt_f32(r: &mut Reader<'_>) -> Result<Option<f32>, WireError> {
    Ok(if bool_of(r)? { Some(r.f32()?) } else { None })
}

fn opt_usize(w: &mut Writer, v: Option<usize>) {
    match v {
        Some(x) => w.varint(x as u64 + 1),
        None => w.varint(0),
    }
}

fn ropt_usize(r: &mut Reader<'_>) -> Result<Option<usize>, WireError> {
    Ok(match r.varint()? {
        0 => None,
        n => Some((n - 1) as usize),
    })
}

fn list(w: &mut Writer, v: &[usize]) {
    w.varint(v.len() as u64);
    for &x in v {
        w.varint(x as u64);
    }
}

fn rlist(r: &mut Reader<'_>) -> Result<Vec<usize>, WireError> {
    let n = r.list_len(1)?;
    (0..n).map(|_| Ok(r.varint()? as usize)).collect()
}

fn write_kart(w: &mut Writer, k: &Kart) {
    w.varint(k.spec as u64);
    opt_usize(w, k.player);
    v3(w, k.pos);
    v3(w, k.prev_pos);
    for x in [k.prev_yaw, k.yaw, k.speed, k.hop, k.vy, k.drift, k.charge, k.boost, k.spin, k.shield, k.skill_mult, k.band_mult] {
        w.f32(x);
    }
    match k.slot {
        Slot::Empty => w.u8(0),
        Slot::Rolling { left, item } => {
            w.u8(1);
            w.f32(left);
            w.u8(item_tag(item));
        }
        Slot::Holding(item) => {
            w.u8(2);
            w.u8(item_tag(item));
        }
    }
    w.varint(k.hint as u64);
    w.f32(k.s);
    w.f32(k.lateral);
    w.u8(u8::from(k.on_road));
    w.varint(u64::from(k.laps_done));
    w.varint(u64::from(k.next_gate));
    opt_f32(w, k.finished);
    w.varint(k.place as u64);
    w.f32(k.wrong_way_t);
    w.u8(u8::from(k.wrong_way));
    for x in [k.spin_angle, k.slip, k.tier_flash, k.steer_vis, k.wheel_roll, k.squash] {
        w.f32(x);
    }
    w.u8(match k.launch {
        Launch::Idle => 0,
        Launch::Primed => 1,
        Launch::Early => 2,
    });
    w.u8(u8::from(k.slipstream));
    match k.draft {
        Slip::Behind(i) => {
            w.u8(0);
            w.varint(i as u64);
        }
        Slip::NotDriving => w.u8(1),
        Slip::Turning => w.u8(2),
        Slip::Corner => w.u8(3),
        Slip::NoCone => w.u8(4),
    }
    w.f32(k.cap_base);
    w.f32(k.cap);
}

fn read_kart(r: &mut Reader<'_>) -> Result<Kart, WireError> {
    let spec = r.varint()? as usize;
    let player = ropt_usize(r)?;
    let pos = rv3(r)?;
    let mut k = Kart::new(spec, player, pos, 0.0);
    k.prev_pos = rv3(r)?;
    let mut f = [0.0f32; 12];
    for x in &mut f {
        *x = r.f32()?;
    }
    [k.prev_yaw, k.yaw, k.speed, k.hop, k.vy, k.drift, k.charge, k.boost, k.spin, k.shield, k.skill_mult, k.band_mult] = f;
    k.slot = match r.u8()? {
        0 => Slot::Empty,
        1 => {
            let left = r.f32()?;
            Slot::Rolling { left, item: item_of(r.u8()?)? }
        }
        2 => Slot::Holding(item_of(r.u8()?)?),
        _ => return Err(WireError::Invalid),
    };
    k.hint = r.varint()? as usize;
    k.s = r.f32()?;
    k.lateral = r.f32()?;
    k.on_road = bool_of(r)?;
    k.laps_done = r.varint_u32()?;
    k.next_gate = r.varint_u32()?;
    k.finished = ropt_f32(r)?;
    k.place = r.varint()? as usize;
    k.wrong_way_t = r.f32()?;
    k.wrong_way = bool_of(r)?;
    let mut g = [0.0f32; 6];
    for x in &mut g {
        *x = r.f32()?;
    }
    [k.spin_angle, k.slip, k.tier_flash, k.steer_vis, k.wheel_roll, k.squash] = g;
    k.launch = match r.u8()? {
        0 => Launch::Idle,
        1 => Launch::Primed,
        2 => Launch::Early,
        _ => return Err(WireError::Invalid),
    };
    k.slipstream = bool_of(r)?;
    k.draft = match r.u8()? {
        0 => Slip::Behind(r.varint()? as usize),
        1 => Slip::NotDriving,
        2 => Slip::Turning,
        3 => Slip::Corner,
        4 => Slip::NoCone,
        _ => return Err(WireError::Invalid),
    };
    k.cap_base = r.f32()?;
    k.cap = r.f32()?;
    Ok(k)
}

fn write_controls(w: &mut Writer, c: &Controls) {
    w.f32(c.steer);
    w.f32(c.throttle);
    w.f32(c.brake);
    w.u8(u8::from(c.drift) | (u8::from(c.item) << 1));
}

fn read_controls(r: &mut Reader<'_>) -> Result<Controls, WireError> {
    let (steer, throttle, brake) = (r.f32()?, r.f32()?, r.f32()?);
    let b = r.u8()?;
    if b > 3 {
        return Err(WireError::Invalid);
    }
    Ok(Controls {
        steer,
        throttle,
        brake,
        drift: b & 1 != 0,
        item: b & 2 != 0,
    })
}

/// A pooled item's flight flag; true when it is in flight (its values
/// follow). An idle slot's values are never read by the race (a spawn
/// overwrites them), so they are not sent and load as defaults.
fn write_flight(w: &mut Writer, world: &World, e: evani_core::ecs::Entity) -> bool {
    let f = world.get::<InFlight>(e).map(|f| *f).unwrap_or_default();
    w.u8(u8::from(f.on));
    w.varint(u64::from(f.seq));
    f.on
}

fn read_flight(r: &mut Reader<'_>) -> Result<InFlight, WireError> {
    Ok(InFlight {
        on: bool_of(r)?,
        seq: r.varint_u32()?,
    })
}

/// Every field of the race that the rules read, from `world` (a world the
/// kit's race lives in: a [`NetRace`]'s own, or the game's).
pub fn save_world(world: &World, w: &mut Writer) {
    let r = world.res::<Race>();
    w.u8(STATE_VERSION);
    w.u8(r.phase as u8);
    w.u8(u8::from(r.paused));
    w.varint(r.players as u64);
    w.f32(r.countdown);
    w.f32(r.clock);
    w.f32(r.phase_time);
    w.varint(r.counts.karts as u64);
    w.varint(u64::from(r.counts.laps));
    w.varint(u64::from(r.counts.checkpoints));
    list(w, &r.order);
    list(w, &r.finish_order);
    match &r.final_order {
        Some(o) => {
            w.u8(1);
            list(w, o);
        }
        None => w.u8(0),
    }
    w.varint(r.final_times.len() as u64);
    for &t in &r.final_times {
        opt_f32(w, t);
    }
    let (s, inc) = r.rng.to_raw();
    w.u64_fixed(s);
    w.u64_fixed(inc);
    w.varint(r.race_no);
    w.u8(u8::from(r.autopilot));
    w.u8(u8::from(r.stepping));
    w.u64_fixed(r.seed);
    match &r.online {
        Some(o) => {
            w.u8(1);
            w.varint(u64::from(o.humans));
            w.varint(o.controls.len() as u64);
            for c in &o.controls {
                write_controls(w, c);
            }
        }
        None => w.u8(0),
    }
    w.varint(u64::from(r.pools.next_seq));
    w.varint(r.grid.len() as u64);
    for &e in &r.grid {
        let k = world.get::<Kart>(e).map(|k| k.clone()).unwrap_or_else(|| Kart::new(0, None, [0.0; 3], 0.0));
        write_kart(w, &k);
        write_controls(w, &world.get::<Controls>(e).map(|c| *c).unwrap_or_default());
    }
    w.varint(r.boxes.len() as u64);
    for &e in &r.boxes {
        let b = world.get::<ItemBox>(e).map(|b| *b).unwrap_or_default();
        v3(w, b.pos);
        w.f32(b.hidden);
    }
    w.varint(r.pools.goos.len() as u64);
    for &e in &r.pools.goos {
        if !write_flight(w, world, e) {
            continue;
        }
        let g = world.get::<Goo>(e).map(|g| *g).unwrap_or_default();
        v3(w, g.pos);
        w.f32(g.life);
        w.varint(g.owner as u64);
        w.f32(g.grace);
    }
    w.varint(r.pools.bombs.len() as u64);
    for &e in &r.pools.bombs {
        if !write_flight(w, world, e) {
            continue;
        }
        let b = world.get::<Bomb>(e).map(|b| *b).unwrap_or_default();
        v3(w, b.pos);
        v3(w, b.vel);
        w.f32(b.fuse);
        w.u8(b.bounces);
        w.varint(b.owner as u64);
        w.f32(b.grace);
        w.varint(b.seg as u64);
    }
    w.varint(r.pools.seekers.len() as u64);
    for &e in &r.pools.seekers {
        if !write_flight(w, world, e) {
            continue;
        }
        let s = world.get::<Seeker>(e).map(|s| *s).unwrap_or_default();
        w.f32(s.s);
        w.f32(s.lateral);
        v3(w, s.pos);
        w.varint(s.owner as u64);
        opt_usize(w, s.target);
        w.f32(s.life);
    }
    w.varint(r.pools.blasts.len() as u64);
    for &e in &r.pools.blasts {
        if !write_flight(w, world, e) {
            continue;
        }
        let b = world.get::<Blast>(e).map(|b| *b).unwrap_or_default();
        v3(w, b.pos);
        w.f32(b.radius);
        w.f32(b.t);
    }
}

/// Replace the race in `world` with one [`save_world`] wrote. The world
/// must hold the same kit (same boxes and item pools); the grid is respawned
/// when its size differs. Refused bytes leave nothing half written.
pub fn load_world(world: &mut World, r: &mut Reader<'_>) -> Result<(), WireError> {
    if r.u8()? != STATE_VERSION {
        return Err(WireError::Invalid);
    }
    let phase = phase_of(r.u8()?)?;
    let paused = bool_of(r)?;
    let players = r.varint()? as usize;
    let (countdown, clock, phase_time) = (r.f32()?, r.f32()?, r.f32()?);
    let counts = Counts {
        karts: r.varint()? as usize,
        laps: r.varint_u32()?,
        checkpoints: r.varint_u32()?,
    };
    let order = rlist(r)?;
    let finish_order = rlist(r)?;
    let final_order = if bool_of(r)? { Some(rlist(r)?) } else { None };
    let n = r.list_len(1)?;
    let final_times = (0..n).map(|_| ropt_f32(r)).collect::<Result<Vec<_>, _>>()?;
    let rng = Pcg32::from_raw(r.u64_fixed()?, r.u64_fixed()?);
    let race_no = r.varint()?;
    let autopilot = bool_of(r)?;
    let stepping = bool_of(r)?;
    let seed = r.u64_fixed()?;
    let online = if bool_of(r)? {
        let humans = r.varint_u32()?;
        let n = r.list_len(13)?;
        let controls = (0..n).map(|_| read_controls(r)).collect::<Result<Vec<_>, _>>()?;
        Some(Online { humans, controls })
    } else {
        None
    };
    let next_seq = r.varint_u32()?;
    let n = r.list_len(40)?;
    if n > race::MAX_KARTS {
        return Err(WireError::Invalid);
    }
    let mut karts = Vec::with_capacity(n);
    for _ in 0..n {
        karts.push((read_kart(r)?, read_controls(r)?));
    }
    let n = r.list_len(16)?;
    let mut boxes = Vec::with_capacity(n);
    for _ in 0..n {
        boxes.push(ItemBox {
            pos: rv3(r)?,
            hidden: r.f32()?,
        });
    }
    let n = r.list_len(2)?;
    let mut goos = Vec::with_capacity(n);
    for _ in 0..n {
        let f = read_flight(r)?;
        if !f.on {
            goos.push((f, Default::default()));
            continue;
        }
        goos.push((
            f,
            Goo {
                pos: rv3(r)?,
                life: r.f32()?,
                owner: r.varint()? as usize,
                grace: r.f32()?,
            },
        ));
    }
    let n = r.list_len(2)?;
    let mut bombs = Vec::with_capacity(n);
    for _ in 0..n {
        let f = read_flight(r)?;
        if !f.on {
            bombs.push((f, Default::default()));
            continue;
        }
        bombs.push((
            f,
            Bomb {
                pos: rv3(r)?,
                vel: rv3(r)?,
                fuse: r.f32()?,
                bounces: r.u8()?,
                owner: r.varint()? as usize,
                grace: r.f32()?,
                seg: r.varint()? as usize,
            },
        ));
    }
    let n = r.list_len(2)?;
    let mut seekers = Vec::with_capacity(n);
    for _ in 0..n {
        let f = read_flight(r)?;
        if !f.on {
            seekers.push((f, Default::default()));
            continue;
        }
        seekers.push((
            f,
            Seeker {
                s: r.f32()?,
                lateral: r.f32()?,
                pos: rv3(r)?,
                owner: r.varint()? as usize,
                target: ropt_usize(r)?,
                life: r.f32()?,
            },
        ));
    }
    let n = r.list_len(2)?;
    let mut blasts = Vec::with_capacity(n);
    for _ in 0..n {
        let f = read_flight(r)?;
        if !f.on {
            blasts.push((f, Default::default()));
            continue;
        }
        blasts.push((
            f,
            Blast {
                pos: rv3(r)?,
                radius: r.f32()?,
                t: r.f32()?,
            },
        ));
    }
    if !r.is_empty() {
        return Err(WireError::Invalid);
    }
    {
        let race = world.res::<Race>();
        let p = &race.pools;
        if race.boxes.len() != boxes.len() || p.goos.len() != goos.len() || p.bombs.len() != bombs.len() || p.seekers.len() != seekers.len() || p.blasts.len() != blasts.len() {
            return Err(WireError::Invalid);
        }
    }
    // Everything decoded: write it.
    if world.res::<Race>().grid.len() != karts.len() {
        race::respawn_karts(world, karts.len());
    }
    let mut guard = world.res_mut::<Race>();
    let r = &mut *guard;
    r.phase = phase;
    r.paused = paused;
    r.players = players;
    r.countdown = countdown;
    r.clock = clock;
    r.phase_time = phase_time;
    r.counts = counts;
    r.order = order;
    r.finish_order = finish_order;
    r.final_order = final_order;
    r.final_times = final_times;
    r.rng = rng;
    r.race_no = race_no;
    r.autopilot = autopilot;
    r.stepping = stepping;
    r.seed = seed;
    r.online = online;
    r.pools.next_seq = next_seq;
    r.moment_time = None;
    r.requests.clear();
    for (&e, (k, c)) in r.grid.iter().zip(karts) {
        if let Some(mut m) = world.get_mut::<Kart>(e) {
            *m = k;
        }
        if let Some(mut m) = world.get_mut::<Controls>(e) {
            *m = c;
        }
    }
    for (&e, b) in r.boxes.iter().zip(boxes) {
        if let Some(mut m) = world.get_mut::<ItemBox>(e) {
            *m = b;
        }
    }
    fn put<T: evani_core::ecs::Component>(world: &World, list: &[evani_core::ecs::Entity], vals: Vec<(InFlight, T)>) {
        for (&e, (f, v)) in list.iter().zip(vals) {
            if let Some(mut m) = world.get_mut::<InFlight>(e) {
                *m = f;
            }
            if let Some(mut m) = world.get_mut::<T>(e) {
                *m = v;
            }
        }
    }
    put(world, &r.pools.goos, goos);
    put(world, &r.pools.bombs, bombs);
    put(world, &r.pools.seekers, seekers);
    put(world, &r.pools.blasts, blasts);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_net::lockstep::sync_test;

    fn frame(t: u64, humans: u32) -> Frame<KartInput> {
        let inputs = (0..SEATS)
            .map(|s| KartInput {
                steer: (((t / 40 + s as u64) % 5) as i8 - 2) * 40,
                throttle: 255,
                buttons: if (t + s as u64) % 90 < 30 { KartInput::DRIFT } else { 0 } | if t % 200 == s as u64 { KartInput::ITEM } else { 0 },
            })
            .collect();
        Frame { tick: t, humans, inputs }
    }

    #[test]
    fn the_online_race_is_deterministic_and_saves_exactly() {
        let mut a = NetRace::new(42, &[]).unwrap();
        let mut b = NetRace::new(7, &[]).unwrap();
        sync_test(&mut a, &mut b, 1500, |t| frame(t, 0b0000_0101)).unwrap();
        let r = a.race();
        assert_eq!(r.phase, Phase::Racing);
        assert!(r.clock > 15.0, "{}", r.clock);
    }

    #[test]
    fn an_ai_race_with_the_referees_brains_finishes_and_restarts() {
        let mut g = NetRace::new(1, &[1]).unwrap();
        let mut brains = KartBrains::new(1);
        let mut finished = false;
        for t in 1..=60 * 120 {
            let inputs = (0..SEATS).map(|s| brains.think(&g, s, t)).collect();
            g.step(&Frame { tick: t, humans: 0, inputs });
            let r = g.race();
            if r.phase == Phase::Finished {
                finished = true;
            }
            if finished && r.phase == Phase::Countdown {
                // Race 1 is the title grid the kit spawns; the first online race is 2.
                assert_eq!(r.race_no, 3);
                return;
            }
        }
        panic!("the AI race never finished and restarted (finished: {finished})");
    }

    #[test]
    fn an_input_is_three_bytes_and_controls_round_trip() {
        let c = Controls {
            steer: -0.5,
            throttle: 1.0,
            brake: 1.0,
            drift: true,
            item: true,
        };
        let i = KartInput::from_controls(&c);
        let mut w = Writer::new();
        NetRace::write_input(&i, &mut w);
        assert_eq!(w.len(), 3);
        let back = NetRace::read_input(&mut Reader::new(w.as_bytes())).unwrap();
        assert_eq!(back, i);
        let d = back.controls();
        assert!((d.steer + 0.5).abs() < 0.01 && d.drift && d.item && d.brake == 1.0);
        assert_eq!(NetRace::held(i).buttons & KartInput::ITEM, 0);
        assert_eq!(NetRace::sanitize(KartInput { steer: -128, throttle: 0, buttons: 0xFF }), KartInput { steer: -127, throttle: 0, buttons: 7 });
    }

    #[test]
    fn hostile_state_bytes_are_refused() {
        let mut g = NetRace::new(3, &[]).unwrap();
        let h = g.hash();
        for bad in [&[][..], &[9][..], &[0, 1, 2, 3][..]] {
            assert!(g.load(&mut Reader::new(bad)).is_err());
        }
        let mut w = Writer::new();
        g.save(&mut w);
        let mut cut = w.as_bytes().to_vec();
        cut.truncate(cut.len() / 2);
        assert!(g.load(&mut Reader::new(&cut)).is_err());
        assert_eq!(g.hash(), h, "a refused load changes nothing");
    }

    #[test]
    fn single_player_tuning_is_the_online_tuning() {
        let t = net_tune();
        assert_eq!(t.item_respawn_s, 1.5);
        assert_eq!(t.max_speed, Tune::default().max_speed);
    }
}
