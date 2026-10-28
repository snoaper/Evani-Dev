//! The kit's types on the wire. Every float is sent as its bits, so a
//! snapshot read back is the exact state the server stepped (prediction
//! compares checksums of the encoding).
//!
//! The arena is sent as its seed, the hole list, a bitset of fallen cells
//! and the ring angle; the layout itself is rebuilt from the seed (cached
//! per seed, see [`base_arena`]) instead of being sent.

use std::cell::RefCell;

use evani_net::{Reader, Wire, WireError, Writer};

use crate::arena::{Arena, ArenaDef};
use crate::body::{Body, Controls};
use crate::match_rules::{Blast, Feed, Phase, Shot, ShotEnd, Soldier, WinReason};
use crate::ring::Ring;
use crate::weapons::Projectile;

/// A `usize` as a varint `u32` (indices and counts here are small).
pub(crate) fn put_usize(w: &mut Writer, v: usize) {
    w.varint(v as u64);
}

/// A `usize` written by [`put_usize`].
pub(crate) fn get_usize(r: &mut Reader<'_>) -> Result<usize, WireError> {
    usize::try_from(r.varint_u32()?).map_err(|_| WireError::Overflow)
}

/// A small enum as one byte.
macro_rules! wire_enum {
    ($ty:ty { $($v:path = $n:literal),* $(,)? }) => {
        impl Wire for $ty {
            fn write(&self, w: &mut Writer) {
                w.u8(match self { $($v => $n,)* });
            }
            fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
                match r.u8()? {
                    $($n => Ok($v),)*
                    _ => Err(WireError::Invalid),
                }
            }
        }
    };
}

wire_enum!(Phase { Phase::Title = 0, Phase::Pre = 1, Phase::Play = 2, Phase::Results = 3 });
wire_enum!(WinReason { WinReason::Core = 0, WinReason::Kills = 1, WinReason::Time = 2 });
wire_enum!(ShotEnd { ShotEnd::Soldier = 0, ShotEnd::Concrete = 1, ShotEnd::Metal = 2, ShotEnd::Air = 3 });

impl Wire for Controls {
    fn write(&self, w: &mut Writer) {
        w.f32(self.strafe);
        w.f32(self.forward);
        w.f32(self.yaw);
        w.f32(self.pitch);
        w.u8(u8::from(self.fire) | u8::from(self.jump) << 1 | ((self.weapon as u8 & 3) << 2));
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let (strafe, forward, yaw, pitch) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let b = r.u8()?;
        Ok(Controls { strafe, forward, yaw, pitch, fire: b & 1 != 0, jump: b & 2 != 0, weapon: usize::from(b >> 2 & 3) })
    }
}

impl Wire for Body {
    fn write(&self, w: &mut Writer) {
        self.p.write(w);
        self.v.write(w);
        w.f32(self.yaw);
        w.f32(self.pitch);
        w.u8(u8::from(self.grounded) | ((self.weapon as u8 & 3) << 1));
        w.f32(self.cooldown);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let (p, v, yaw, pitch) = (r.get()?, r.get()?, r.f32()?, r.f32()?);
        let b = r.u8()?;
        Ok(Body { p, v, yaw, pitch, grounded: b & 1 != 0, cooldown: r.f32()?, weapon: usize::from(b >> 1 & 3) })
    }
}

impl Wire for Soldier {
    fn write(&self, w: &mut Writer) {
        self.name.write(w);
        w.u8(self.team as u8);
        self.body.write(w);
        w.f32(self.hp);
        self.dead.write(w);
        w.f32(self.shield);
        self.kills.write(w);
        self.deaths.write(w);
        w.f32(self.recoil);
        w.f32(self.recoil_yaw);
        self.hold.write(w);
        w.u8(self.spawns);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Soldier {
            name: r.get()?,
            team: usize::from(r.u8()? & 1),
            body: r.get()?,
            hp: r.f32()?,
            dead: r.get()?,
            shield: r.f32()?,
            kills: r.get()?,
            deaths: r.get()?,
            recoil: r.f32()?,
            recoil_yaw: r.f32()?,
            hold: r.get()?,
            brain: None,
            spawns: r.u8()?,
        })
    }
}

impl Wire for Projectile {
    fn write(&self, w: &mut Writer) {
        w.u8(self.weapon as u8);
        put_usize(w, self.owner);
        w.u8(self.team as u8);
        self.p.write(w);
        self.v.write(w);
        w.f32(self.age);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Projectile {
            weapon: usize::from(r.u8()? & 3),
            owner: get_usize(r)?,
            team: usize::from(r.u8()? & 1),
            p: r.get()?,
            v: r.get()?,
            age: r.f32()?,
        })
    }
}

impl Wire for Shot {
    fn write(&self, w: &mut Writer) {
        self.from.write(w);
        self.to.write(w);
        self.end.write(w);
        w.f32(self.age);
        w.u8(self.team as u8);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Shot { from: r.get()?, to: r.get()?, end: r.get()?, age: r.f32()?, team: usize::from(r.u8()? & 1) })
    }
}

impl Wire for Blast {
    fn write(&self, w: &mut Writer) {
        self.p.write(w);
        w.f32(self.age);
        w.u8(self.team as u8);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Blast { p: r.get()?, age: r.f32()?, team: usize::from(r.u8()? & 1) })
    }
}

impl Wire for Feed {
    fn write(&self, w: &mut Writer) {
        self.by.write(w);
        self.victim.write(w);
        self.weapon.map(|x| x as u8).write(w);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Feed { by: r.get()?, victim: r.get()?, weapon: r.get::<Option<u8>>()?.map(|x| usize::from(x & 3)) })
    }
}

impl Wire for Ring {
    fn write(&self, w: &mut Writer) {
        w.u8(self.index as u8);
        w.f32(self.timer);
        self.turning.write(w);
        w.f32(self.interval);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Ring { index: u32::from(r.u8()?) % 6, timer: r.f32()?, turning: r.get()?, interval: r.f32()? })
    }
}

/// Arenas built so far, by seed (the title's and the current match's are
/// the ones in use). A memo of [`Arena::new`], which depends only on the
/// arena data and the seed, so it never changes what a step computes.
const CACHED_ARENAS: usize = 4;

thread_local! {
    static ARENAS: RefCell<Vec<Arena>> = const { RefCell::new(Vec::new()) };
}

/// The undamaged arena for `seed` (no holes, nothing fallen, ring at 0).
pub(crate) fn base_arena(seed: u64) -> Result<Arena, String> {
    if let Some(a) = ARENAS.with(|c| c.borrow().iter().find(|a| a.seed == seed).cloned()) {
        return Ok(a);
    }
    let def = match ARENAS.with(|c| c.borrow().first().map(|a| a.def.clone())) {
        Some(d) => d,
        None => ArenaDef::load()?,
    };
    let a = Arena::new(def, seed);
    ARENAS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= CACHED_ARENAS {
            c.remove(0);
        }
        c.push(a.clone());
    });
    Ok(a)
}

impl Wire for Arena {
    fn write(&self, w: &mut Writer) {
        self.seed.write(w);
        self.holes.write(w);
        let mut bits = vec![0u8; self.gone.len().div_ceil(8)];
        for (i, _) in self.gone.iter().enumerate().filter(|(_, g)| **g) {
            if let Some(b) = bits.get_mut(i / 8) {
                *b |= 1 << (i % 8);
            }
        }
        put_usize(w, self.gone.len());
        w.bytes(&bits);
        w.f32(self.ang);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let seed: u64 = r.get()?;
        let holes = r.get()?;
        let n = get_usize(r)?;
        let bits = r.take(n.div_ceil(8))?;
        let ang = r.f32()?;
        let mut a = base_arena(seed).map_err(|_| WireError::Invalid)?;
        if n != a.gone.len() {
            return Err(WireError::Invalid);
        }
        a.holes = holes;
        for (i, g) in a.gone.iter_mut().enumerate() {
            *g = bits.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0);
        }
        a.set_ring(ang);
        Ok(a)
    }
}
