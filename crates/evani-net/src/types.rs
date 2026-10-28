//! Ticks, sim time and ids shared by every part of the netcode.

use crate::wire::{Reader, Wire, WireError, Writer};

/// A fixed simulation tick: the engine's 60 Hz step. Tick `t`'s state is the
/// state after `t` steps, before tick `t` is simulated.
pub type Tick = u32;

/// A player slot: a human on a client, or a bot on the server. Given out by
/// the server and stable while the player stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PeerId(pub u16);

/// A networked entity's id, chosen by the game (stable for the entity's
/// life). Pose lists are sorted by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NetId(pub u32);

/// A transport connection. The server end is [`LinkId::SERVER`]; a
/// transport numbers its client connections from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LinkId(pub u32);

impl LinkId {
    pub const SERVER: LinkId = LinkId(0);
}

/// A point in sim time with 1/256-tick resolution (fixed point, `tick *
/// 256 + sub`). Integer maths only, so the server and every client blend
/// poses with exactly the same weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TickTime(pub u64);

impl TickTime {
    /// Sub-tick steps per tick.
    pub const SUBS: u64 = 256;

    /// The start of `tick`.
    pub fn at(tick: Tick) -> Self {
        Self(u64::from(tick) * Self::SUBS)
    }

    pub fn new(tick: Tick, sub: u8) -> Self {
        Self(u64::from(tick) * Self::SUBS + u64::from(sub))
    }

    /// `tick` plus a render alpha in `[0, 1)` (`Time::alpha`).
    pub fn with_alpha(tick: Tick, alpha: f32) -> Self {
        Self::new(tick, alpha_to_sub(alpha))
    }

    /// Whole ticks (saturating at `Tick::MAX`).
    pub fn tick(self) -> Tick {
        Tick::try_from(self.0 / Self::SUBS).unwrap_or(Tick::MAX)
    }

    /// The 1/256 part.
    pub fn sub(self) -> u8 {
        (self.0 % Self::SUBS) as u8
    }

    pub fn add_subs(self, subs: u64) -> Self {
        Self(self.0.saturating_add(subs))
    }

    pub fn sub_subs(self, subs: u64) -> Self {
        Self(self.0.saturating_sub(subs))
    }

    pub fn add_ticks(self, ticks: u32) -> Self {
        self.add_subs(u64::from(ticks) * Self::SUBS)
    }

    pub fn sub_ticks(self, ticks: u32) -> Self {
        self.sub_subs(u64::from(ticks) * Self::SUBS)
    }
}

/// `alpha` in `[0, 1)` as a 1/256 step, rounded down.
pub fn alpha_to_sub(alpha: f32) -> u8 {
    let a = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a * 256.0) as u32).min(255) as u8
}

/// Why a simulation step runs. Effects that must happen once (sounds,
/// particles, hit markers) check [`Role::first_time`]: a rollback replays
/// ticks with [`Role::Resim`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The authority (server, or the in-page host in loopback).
    Server,
    /// A client predicting a tick for the first time.
    Predict,
    /// A client replaying a tick after a correction.
    Resim,
}

impl Role {
    pub fn authoritative(self) -> bool {
        self == Role::Server
    }

    pub fn first_time(self) -> bool {
        self != Role::Resim
    }
}

impl Wire for PeerId {
    fn write(&self, w: &mut Writer) {
        w.varint(u64::from(self.0));
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(u16::read(r)?))
    }
}

impl Wire for NetId {
    fn write(&self, w: &mut Writer) {
        w.varint(u64::from(self.0));
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(u32::read(r)?))
    }
}

impl Wire for TickTime {
    fn write(&self, w: &mut Writer) {
        w.varint(self.0);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(r.varint()?))
    }
}

/// SplitMix64 finaliser: spreads a seed so neighbouring ticks or ids give
/// unrelated generators.
pub fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
