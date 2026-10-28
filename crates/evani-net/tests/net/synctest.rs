//! The sync test on the arena (it passes), on games broken on purpose (each
//! is caught by the right check), and a pinned state hash.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};

use evani_net::synctest::{Check, SyncTest, TickInputs, first_divergence};
use evani_net::wire::{Reader, Wire, WireError, Writer};
use evani_net::{InputSource, NetGame, NetId, PeerId, PeerInput, StepCtx, Tick, TickTime};

use crate::support::{Arena, ArenaInput, ArenaState, FIRE, arena_parts};

/// Peers 1 to 6 join over the first ticks, 3 leaves at 400 and 7 joins at
/// 500; every input is a function of the tick and the state.
fn arena_inputs(t: Tick, s: &ArenaState, out: &mut TickInputs<ArenaInput>) {
    if t < 6 {
        out.joined.push(PeerId(t as u16 + 1));
    }
    if t == 400 {
        out.left.push(PeerId(3));
    }
    if t == 500 {
        out.joined.push(PeerId(7));
    }
    for p in &s.players {
        if t == 400 && p.peer == 3 {
            continue;
        }
        let k = u32::from(p.peer);
        let dx = ((t / (7 + k)) % 3) as i8 - 1;
        let dy = if p.x > 0.0 { -1 } else { 1 };
        let fire = if (t + k).is_multiple_of(25) { FIRE } else { 0 };
        out.inputs.push(PeerInput {
            peer: PeerId(p.peer),
            input: ArenaInput::new(dx, dy, fire),
            view: TickTime::at(t).sub_ticks(6),
            source: InputSource::Fresh,
        });
    }
}

#[test]
fn the_arena_passes_the_sync_test() {
    let report = SyncTest::new(&Arena, 42, 2000)
        .parts(arena_parts)
        .run(&ArenaState::new(), arena_inputs)
        .unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(report.ticks, 2000);
    assert_eq!(report.hashes.len(), 2000);
    assert_eq!(report.replayed, (2000 - 7) * 8);
    // The run did something: players, pickups, scores.
    let again = SyncTest::new(&Arena, 42, 2000)
        .check_distance(30)
        .run(&ArenaState::new(), arena_inputs)
        .unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(first_divergence(&report.hashes, &again.hashes), None);
    let other = SyncTest::new(&Arena, 43, 2000)
        .run(&ArenaState::new(), arena_inputs)
        .unwrap_or_else(|d| panic!("{d}"));
    assert!(first_divergence(&report.hashes, &other.hashes).is_some());
}

/// The arena's state hash after a fixed run, pinned. It changes only if the
/// wire format, the checksum, the seeded generator or the arena's own rules
/// change; a change here means old recordings and peers on the previous
/// build no longer agree, so it must be deliberate.
#[test]
fn the_state_hash_is_stable() {
    const GOLDEN: u64 = 0x1722_0289_c21c_9e07;
    let report = SyncTest::new(&Arena, 42, 600)
        .check_distance(0)
        .run(&ArenaState::new(), arena_inputs)
        .unwrap_or_else(|d| panic!("{d}"));
    let got = report.final_hash().unwrap_or(0);
    assert_eq!(got, GOLDEN, "arena hash after 600 ticks is {got:#018x}");
    // And the protocol hash peers compare on hello.
    let proto = evani_net::wire::fnv1a64(Arena::PROTOCOL.as_bytes());
    assert_eq!(proto, 0x0bfd_4be0_b925_4267, "protocol hash {proto:#018x}");
}

fn one_player(t: Tick, _s: &Counter, out: &mut TickInputs<ArenaInput>) {
    if t == 0 {
        out.joined.push(PeerId(1));
    }
    out.inputs.push(PeerInput {
        peer: PeerId(1),
        input: ArenaInput::new(1, 0, 0),
        view: TickTime::at(t),
        source: InputSource::Fresh,
    });
}

#[derive(Debug, Clone, PartialEq)]
struct Counter {
    x: f32,
    hidden: u32,
    effects: u32,
}
evani_net::wire_struct!(Counter { x, hidden, effects });

fn counter() -> Counter {
    Counter {
        x: 0.0,
        hidden: 0,
        effects: 0,
    }
}

macro_rules! toy_game {
    ($name:ident, $proto:literal, |$s:ident, $ctx:ident, $me:ident| $body:block) => {
        impl NetGame for $name {
            type Input = ArenaInput;
            type State = Counter;
            type Pose = [f32; 2];
            const PROTOCOL: &'static str = $proto;
            fn step(&self, $s: &mut Counter, $ctx: &StepCtx<'_, ArenaInput, [f32; 2]>) {
                let $me = self;
                $body
            }
            fn poses(&self, s: &Counter, out: &mut Vec<(NetId, [f32; 2])>) {
                out.push((NetId(1), [s.x, 0.0]));
            }
        }
    };
}

/// Keeps a counter in the game value: two runs see different numbers.
struct Leaky {
    calls: Cell<u32>,
}
toy_game!(Leaky, "leaky/1", |s, ctx, me| {
    me.calls.set(me.calls.get() + 1);
    s.x += 1.0 + (me.calls.get() % 2) as f32;
    let _ = ctx;
});

static GLOBAL: AtomicU32 = AtomicU32::new(0);
/// Reads a global that keeps changing (like a clock).
struct Clocky;
toy_game!(Clocky, "clocky/1", |s, ctx, _me| {
    let g = GLOBAL.fetch_add(1, Ordering::Relaxed);
    if ctx.tick == 10 {
        s.x += (g % 2) as f32;
    }
    s.x += 1.0;
});

/// Spawns an effect into the state only the first time a tick runs.
struct EffectInState;
toy_game!(EffectInState, "effects/1", |s, ctx, _me| {
    s.x += 0.5;
    if ctx.tick % 9 == 4 && ctx.role.first_time() {
        s.effects += 1;
    }
});

/// Uses a field its wire encoding leaves out.
#[derive(Debug, Clone, PartialEq)]
struct Secret {
    x: f32,
    secret: u32,
}
impl Wire for Secret {
    fn write(&self, w: &mut Writer) {
        self.x.write(w);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            x: f32::read(r)?,
            secret: 0,
        })
    }
}
struct Forgetful;
impl NetGame for Forgetful {
    type Input = ArenaInput;
    type State = Secret;
    type Pose = [f32; 2];
    const PROTOCOL: &'static str = "forgetful/1";
    fn step(&self, s: &mut Secret, ctx: &StepCtx<'_, ArenaInput, [f32; 2]>) {
        s.secret += 1;
        if ctx.tick >= 3 {
            s.x += s.secret as f32;
        }
    }
    fn poses(&self, s: &Secret, out: &mut Vec<(NetId, [f32; 2])>) {
        out.push((NetId(1), [s.x, 0.0]));
    }
}

/// Writes a value that reads back as another (a lossy float field).
#[derive(Debug, Clone, PartialEq)]
struct Lossy {
    x: f32,
}
impl Wire for Lossy {
    fn write(&self, w: &mut Writer) {
        w.zigzag(i64::from(self.x as i32));
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            x: r.zigzag()? as f32,
        })
    }
}
struct Rounding;
impl NetGame for Rounding {
    type Input = ArenaInput;
    type State = Lossy;
    type Pose = [f32; 2];
    const PROTOCOL: &'static str = "lossy/1";
    fn step(&self, s: &mut Lossy, _ctx: &StepCtx<'_, ArenaInput, [f32; 2]>) {
        s.x += 0.75;
    }
    fn poses(&self, s: &Lossy, out: &mut Vec<(NetId, [f32; 2])>) {
        out.push((NetId(1), [s.x, 0.0]));
    }
}

#[test]
fn hidden_state_in_the_game_value_is_caught() {
    let game = Leaky {
        calls: Cell::new(0),
    };
    let d = SyncTest::new(&game, 1, 100)
        .run(&counter(), one_player)
        .unwrap_err();
    assert_eq!(d.check, Check::Repeat);
    assert_eq!(d.tick, 0);
    assert!(d.to_string().contains("fix:"), "{d}");
}

#[test]
fn a_changing_global_is_caught_at_its_tick() {
    let d = SyncTest::new(&Clocky, 1, 100)
        .run(&counter(), one_player)
        .unwrap_err();
    assert_eq!(d.check, Check::Repeat);
    assert_eq!(d.tick, 10);
    assert!(
        d.first_byte.is_some_and(|b| b < 4),
        "x is the first field: {d:?}"
    );
}

#[test]
fn state_changed_only_on_a_first_run_is_caught_by_the_rollback_replay() {
    let d = SyncTest::new(&EffectInState, 1, 100)
        .check_distance(4)
        .run(&counter(), one_player)
        .unwrap_err();
    assert_eq!(d.check, Check::Rollback);
    assert_eq!(d.tick, 4, "found as soon as a replay covers tick 4");
    // With the replay off it slips through: the check is what finds it.
    assert!(
        SyncTest::new(&EffectInState, 1, 100)
            .check_distance(0)
            .run(&counter(), one_player)
            .is_ok()
    );
}

#[test]
fn a_field_missing_from_the_wire_is_caught() {
    let d = SyncTest::new(&Forgetful, 1, 50)
        .run(&Secret { x: 0.0, secret: 0 }, |_, _, _| {})
        .unwrap_err();
    assert_eq!(d.check, Check::WireRoundTrip);
    assert_eq!(d.tick, 3);
}

#[test]
fn a_lossy_encoding_is_caught() {
    let d = SyncTest::new(&Rounding, 1, 50)
        .run(&Lossy { x: 0.0 }, |_, _, _| {})
        .unwrap_err();
    assert_eq!(d.check, Check::WireRoundTrip);
}

#[test]
fn a_divergence_names_the_part_that_differs() {
    struct BadPickups;
    impl NetGame for BadPickups {
        type Input = ArenaInput;
        type State = ArenaState;
        type Pose = [f32; 2];
        const PROTOCOL: &'static str = "bad-pickups/1";
        fn step(&self, s: &mut ArenaState, ctx: &StepCtx<'_, ArenaInput, [f32; 2]>) {
            Arena.step(s, ctx);
            // Spawns with a generator that is not tied to the tick.
            if ctx.tick == 50 {
                let n = SPAWNS.fetch_add(1, Ordering::Relaxed);
                s.pickups.push((n as f32, 0.0));
            }
        }
        fn poses(&self, s: &ArenaState, out: &mut Vec<(NetId, [f32; 2])>) {
            Arena.poses(s, out);
        }
    }
    static SPAWNS: AtomicU32 = AtomicU32::new(0);
    fn parts(_g: &BadPickups, s: &ArenaState, out: &mut Vec<(&'static str, u64)>) {
        arena_parts(&Arena, s, out);
    }
    let d = SyncTest::new(&BadPickups, 1, 100)
        .parts(parts)
        .run(&ArenaState::new(), arena_inputs)
        .unwrap_err();
    assert_eq!(d.tick, 50);
    assert_eq!(d.part, Some("pickups"));
    assert!(d.to_string().contains("part `pickups`"), "{d}");
}
