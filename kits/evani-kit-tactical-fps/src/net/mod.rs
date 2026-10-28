//! Hexagon over evani-net (the kit's `net` feature; evani.toml `net = true`).
//!
//! The match runs through the netcode instead of a plain fixed step:
//!
//! * [`HexInput`]: one player's input for one tick, as held state (stick
//!   axes quantised to i8, absolute aim, buttons, weapon slot) plus the
//!   life it was made for ([`HexInput::epoch`], the soldier's spawn count),
//!   so aim sent before a respawn never undoes the respawn's facing.
//! * [`HexState`]: the whole simulated world: the [`Match`], its [`Arena`]
//!   and the seats (`seats[i]` is the peer playing `soldiers[i]`).
//! * [`HexNet`]: the [`NetGame`]. Its step seats joins and drops leaves,
//!   turns every peer's input into its soldier's held controls, starts a
//!   match on a player's Fire or Jump press, and runs the match's own
//!   [`Match::step`] with the tick's seeded generator ([`StepCtx::rng`]), so
//!   the server, the client's prediction and its replays agree bit for bit.
//! * Lag compensation: before the step, each player holding fire gets the
//!   foes' positions as that player saw them ([`StepCtx::rewind`]);
//!   hitscan tests those ([`Match::lag_target`]).
//! * [`HexPose`]: what remote soldiers are drawn from, interpolated
//!   between snapshots.
//! * [`HexBot`]: a bot on the server, the kit's own brain ([`bot::think`])
//!   reading the authoritative state.
//! * [`session`]: the in-page session (server, local client, loopback, and
//!   with `net-dev` a second local player) and the kit's systems.

pub(crate) mod session;
pub(crate) mod wire;

use evani_core::rng::Pcg32;
use evani_net::{
    Bot, InputSource, Interpolate, NetGame, NetId, PeerId, Reader, StepCtx, Tick, Wire, WireError,
    Writer, hold, lerp_angle,
};

use crate::arena::Arena;
use crate::body::Controls;
use crate::bot::{self, Brain};
use crate::match_rules::{Match, Phase, PlayerInput, Soldier, THINK_EVERY, wrap};
use crate::tune::{Tune, WEAPON_NAMES};

pub use session::{NetMatch, step_system, view_system};

/// Fire held.
const FIRE: u8 = 1;
/// Jump held.
const JUMP: u8 = 2;
/// The pitch limit (as the single-player controls).
const PITCH_MAX: f32 = 1.4;

/// One player's input for one tick, as held state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HexInput {
    /// Strafe, -127 .. 127.
    pub strafe: i8,
    /// Forward, -127 .. 127.
    pub forward: i8,
    /// Aim yaw (rad, absolute).
    pub yaw: f32,
    /// Aim pitch (rad, absolute).
    pub pitch: f32,
    /// [`FIRE`] and [`JUMP`] bits.
    pub buttons: u8,
    /// Weapon slot.
    pub weapon: u8,
    /// The life the aim was made for (the soldier's spawn count); the aim
    /// of an input for another life is not applied.
    pub epoch: u8,
}

fn axis(v: f32) -> i8 {
    (v.clamp(-1.0, 1.0) * 127.0).round() as i8
}

impl HexInput {
    /// The input that holds controls `c` for the life `epoch`.
    pub fn from_controls(c: &Controls, epoch: u8) -> Self {
        HexInput {
            strafe: axis(c.strafe),
            forward: axis(c.forward),
            yaw: c.yaw,
            pitch: c.pitch,
            buttons: (u8::from(c.fire) * FIRE) | (u8::from(c.jump) * JUMP),
            weapon: (c.weapon % WEAPON_NAMES.len()) as u8,
            epoch,
        }
    }

    /// A player's input this tick from the actions (`p`) and their soldier
    /// as last predicted (`s`): the turn is added to the soldier's aim, a
    /// weapon press takes the next slot.
    pub fn from_player(p: &PlayerInput, s: &Soldier) -> Self {
        let h = &s.hold;
        let weapon = if p.next_weapon { (h.weapon + 1) % WEAPON_NAMES.len() } else { h.weapon };
        let c = Controls {
            strafe: p.strafe,
            forward: p.forward,
            yaw: wrap(h.yaw + p.dyaw),
            pitch: (h.pitch + p.dpitch).clamp(-PITCH_MAX, PITCH_MAX),
            fire: p.fire,
            jump: p.jump,
            weapon,
        };
        HexInput::from_controls(&c, s.spawns)
    }

    /// Fire or Jump held (a rising edge starts a match on the title).
    pub fn go(&self) -> bool {
        self.buttons & (FIRE | JUMP) != 0
    }

    /// Hold this input on soldier `s`.
    fn apply(&self, s: &mut Soldier) {
        let h = &mut s.hold;
        h.strafe = f32::from(self.strafe) / 127.0;
        h.forward = f32::from(self.forward) / 127.0;
        h.fire = self.buttons & FIRE != 0;
        h.jump = self.buttons & JUMP != 0;
        h.weapon = usize::from(self.weapon) % WEAPON_NAMES.len();
        if self.epoch == s.spawns && self.yaw.is_finite() && self.pitch.is_finite() {
            h.yaw = wrap(self.yaw);
            h.pitch = self.pitch.clamp(-PITCH_MAX, PITCH_MAX);
        }
    }
}

impl Wire for HexInput {
    fn write(&self, w: &mut Writer) {
        self.strafe.write(w);
        self.forward.write(w);
        w.f32(self.yaw);
        w.f32(self.pitch);
        w.u8(self.buttons);
        w.u8(self.weapon);
        w.u8(self.epoch);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(HexInput {
            strafe: r.get()?,
            forward: r.get()?,
            yaw: r.f32()?,
            pitch: r.f32()?,
            buttons: r.u8()?,
            weapon: r.u8()?,
            epoch: r.u8()?,
        })
    }
}

/// A soldier as drawn for a remote player and as tested by lag
/// compensation: feet, aim, and the life (a respawn is not blended).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HexPose {
    /// Feet position.
    pub p: [f32; 3],
    /// Aim yaw.
    pub yaw: f32,
    /// Aim pitch.
    pub pitch: f32,
    /// Spawn count.
    pub life: u8,
}

impl Interpolate for HexPose {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        if a.life != b.life {
            return hold(a, b, t);
        }
        HexPose {
            p: <[f32; 3]>::interpolate(&a.p, &b.p, t),
            yaw: lerp_angle(a.yaw, b.yaw, t),
            pitch: a.pitch + (b.pitch - a.pitch) * t,
            life: b.life,
        }
    }
}

/// Who plays a soldier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seat {
    /// The peer.
    pub peer: PeerId,
    /// A server-side bot.
    pub bot: bool,
    /// Fire or Jump was held last tick (for the start press).
    pub prev_go: bool,
}

impl Wire for Seat {
    fn write(&self, w: &mut Writer) {
        self.peer.write(w);
        w.u8(u8::from(self.bot) | u8::from(self.prev_go) << 1);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let peer = r.get()?;
        let b = r.u8()?;
        Ok(Seat { peer, bot: b & 1 != 0, prev_go: b & 2 != 0 })
    }
}

/// The networked world.
#[derive(Debug, Clone)]
pub struct HexState {
    /// The match.
    pub m: Match,
    /// Its arena.
    pub arena: Arena,
    /// `seats[i]` plays `m.soldiers[i]`.
    pub seats: Vec<Seat>,
}

impl HexState {
    /// The soldier index `peer` plays.
    pub fn seat_of(&self, peer: PeerId) -> Option<usize> {
        self.seats.iter().position(|s| s.peer == peer)
    }
}

impl Wire for HexState {
    fn write(&self, w: &mut Writer) {
        self.m.write(w);
        self.arena.write(w);
        self.seats.write(w);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let st = HexState { m: r.get()?, arena: r.get()?, seats: r.get()? };
        if st.seats.len() != st.m.soldiers.len() {
            return Err(WireError::Invalid);
        }
        Ok(st)
    }
}

/// A peer's entity id: its peer id.
pub fn net_id(peer: PeerId) -> NetId {
    NetId(u32::from(peer.0))
}

/// The game the netcode runs: the tuning and the fixed step.
#[derive(Debug, Clone, Copy)]
pub struct HexNet {
    /// Every tunable (the session copies the live values in each tick).
    pub tune: Tune,
    /// The fixed step (s).
    pub dt: f32,
}

impl NetGame for HexNet {
    type Input = HexInput;
    type State = HexState;
    type Pose = HexPose;

    const PROTOCOL: &'static str = "hexagon/1";

    fn step(&self, s: &mut HexState, ctx: &StepCtx<'_, HexInput, HexPose>) {
        let t = &self.tune;
        for peer in ctx.left {
            if let Some(k) = s.seat_of(*peer) {
                s.seats.remove(k);
                s.m.unseat(k);
            }
        }
        for &peer in ctx.joined {
            if s.seat_of(peer).is_some() {
                continue;
            }
            let bot = ctx.input(peer).is_some_and(|p| p.source == InputSource::Bot);
            let player = (!bot).then(|| s.seats.iter().filter(|x| !x.bot).count() + 1);
            s.m.seat(player, &s.arena, t);
            s.seats.push(Seat { peer, bot, prev_go: false });
        }
        // Inputs become held controls; a person's Fire or Jump press starts.
        let mut start = false;
        for (i, seat) in s.seats.iter_mut().enumerate() {
            let Some(pi) = ctx.input(seat.peer) else {
                continue;
            };
            let go = pi.input.go();
            start |= go && !seat.prev_go && !seat.bot;
            seat.prev_go = go;
            if let Some(sol) = s.m.soldiers.get_mut(i) {
                pi.input.apply(sol);
            }
        }
        if s.m.phase == Phase::Title {
            s.m.phase_t += self.dt;
            if start {
                let seed = u64::from(ctx.rng(1).next_u32()) + 1;
                if let Ok(a) = wire::base_arena(seed) {
                    s.arena = a;
                    s.m.start_match_net(&mut s.arena, t);
                }
            }
            return;
        }
        lag_targets(s, ctx);
        // Every soldier's controls are set: the match's player input only
        // repeats soldier 0's, so its own input path changes nothing.
        let mirror = s.m.soldiers.first().map_or(PlayerInput::default(), |p| PlayerInput {
            strafe: p.hold.strafe,
            forward: p.hold.forward,
            fire: p.hold.fire,
            jump: p.hold.jump,
            ..PlayerInput::default()
        });
        let mut rng = ctx.rng(0);
        s.m.step(&mirror, &mut s.arena, &mut rng, t, self.dt);
        s.m.lag.clear();
    }

    fn poses(&self, s: &HexState, out: &mut Vec<(NetId, HexPose)>) {
        for (seat, sol) in s.seats.iter().zip(&s.m.soldiers) {
            let b = &sol.body;
            out.push((net_id(seat.peer), HexPose { p: b.p, yaw: b.yaw, pitch: b.pitch, life: sol.spawns }));
        }
    }
}

/// Lag compensation for this tick: every player holding fire gets the
/// soldiers' positions as that player saw them (only for the same life:
/// a soldier who has respawned since is tested where it is).
fn lag_targets(s: &mut HexState, ctx: &StepCtx<'_, HexInput, HexPose>) {
    s.m.lag.clear();
    let mut seen = Vec::new();
    for (i, seat) in s.seats.iter().enumerate() {
        let firing = s.m.soldiers.get(i).is_some_and(|x| x.alive() && x.hold.fire);
        if seat.bot || !firing || ctx.rewind(seat.peer, &mut seen).is_none() {
            continue;
        }
        let at = s
            .seats
            .iter()
            .zip(&s.m.soldiers)
            .map(|(x, sol)| {
                let id = net_id(x.peer);
                seen.iter().find(|(n, p)| *n == id && p.life == sol.spawns).map(|(_, p)| p.p)
            })
            .collect();
        s.m.lag.push((i, at));
    }
}

/// A bot on the server: the kit's brain, thinking every third tick
/// (Hexagon's 50 ms) on the authoritative state and holding its controls
/// in between.
#[derive(Debug, Clone, Copy)]
pub struct HexBot {
    /// Its memory.
    pub brain: Brain,
}

impl HexBot {
    /// The bot's controls for this tick, for soldier `i` of `st`.
    fn controls(&mut self, game: &HexNet, st: &HexState, i: usize, rng: &mut Pcg32) -> Option<HexInput> {
        let sol = st.m.soldiers.get(i)?;
        let m = &st.m;
        let thinking = m.steps.is_multiple_of(THINK_EVERY) && matches!(m.phase, Phase::Pre | Phase::Play) && sol.alive();
        let c = if thinking { bot::think(m, i, &mut self.brain, &st.arena, rng, &game.tune) } else { sol.hold };
        Some(HexInput::from_controls(&c, sol.spawns))
    }
}

impl Bot<HexNet> for HexBot {
    fn think(&mut self, game: &HexNet, state: &HexState, me: PeerId, _tick: Tick, rng: &mut Pcg32) -> HexInput {
        state.seat_of(me).and_then(|i| self.controls(game, state, i, rng)).unwrap_or_default()
    }
}
