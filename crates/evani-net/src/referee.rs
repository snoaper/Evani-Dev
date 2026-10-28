//! The neutral headless referee ABI, `evani-referee/1` (feature `referee`).
//!
//! A game's referee crate (a `cdylib` built for `wasm32-unknown-unknown`
//! with `--cfg evani_headless`, so nothing of the browser is linked) calls
//! [`export_referee!`] with a function that makes its session. The module
//! then has **no imports** and exports `memory` and:
//!
//! | Export | Meaning |
//! |---|---|
//! | `evani_ref_abi() -> u32` | 1 |
//! | `evani_ref_alloc(len) -> ptr` | the scratch buffer for the next call's bytes |
//! | `evani_ref_init(seed_lo, seed_hi, ptr, len) -> i32` | a new session; config bytes; 0 or a negative error |
//! | `evani_ref_join(peer, role) -> i32` | role 0 player, 1 spectator; the seat, or 255 when watching |
//! | `evani_ref_leave(peer) -> i32` | 0, or -3 when not here |
//! | `evani_ref_input(peer, ptr, len) -> i32` | one client packet, exactly as sent; 0 or -3 refused |
//! | `evani_ref_step(ticks) -> i32` | advance whole 1/60 s ticks (3 for a 50 ms host step) |
//! | `evani_ref_view(recipient) -> i64` | `(ptr << 32) \| len` of that peer's view; `0xFFFFFFFF` = the spectator view |
//! | `evani_ref_save() -> i64` / `evani_ref_load(ptr, len) -> i32` | checkpoint out and in |
//! | `evani_ref_results() -> i64` | the game's results bytes, from the referee's own run |
//! | `evani_ref_hash() -> i64` | the state hash (host logs) |
//! | `evani_ref_tick() -> i64` | the referee's tick |
//!
//! Errors: -1 no session, -2 bad pointer or length, -3 refused, -4 the
//! session could not start. Output pointers stay valid until the next call.
//! Every host (a Node or Deno server, a Rust server, a partner's rules
//! runner) drives the same exports; host-specific glue lives outside the
//! engine (for example `bridges/<host>/`).

use std::cell::RefCell;

use crate::lockstep::{Brains, LockstepGame, Referee};
use crate::wire::Writer;

/// This ABI's version.
pub const ABI: u32 = 1;
/// `evani_ref_view`'s recipient for the spectator view.
pub const SPECTATOR: u32 = u32::MAX;

/// A running referee as the ABI sees it (object-safe, so one export set
/// serves any game).
pub trait Session {
    fn join(&mut self, peer: u32, spectator: bool) -> Option<u8>;
    fn leave(&mut self, peer: u32) -> bool;
    fn input(&mut self, peer: u32, packet: &[u8]) -> bool;
    fn step(&mut self, ticks: u32);
    fn view(&self, recipient: Option<u32>, out: &mut Writer);
    fn save(&self, out: &mut Writer);
    fn load(&mut self, data: &[u8]) -> bool;
    fn results(&self, out: &mut Writer);
    fn hash(&self) -> u32;
    fn tick(&self) -> u64;
}

impl<G: LockstepGame, B: Brains<G>> Session for Referee<G, B> {
    fn join(&mut self, peer: u32, spectator: bool) -> Option<u8> {
        Referee::join(self, peer, spectator)
    }
    fn leave(&mut self, peer: u32) -> bool {
        Referee::leave(self, peer)
    }
    fn input(&mut self, peer: u32, packet: &[u8]) -> bool {
        Referee::input(self, peer, packet).is_ok()
    }
    fn step(&mut self, ticks: u32) {
        Referee::step(self, ticks);
    }
    fn view(&self, recipient: Option<u32>, out: &mut Writer) {
        Referee::view(self, recipient, out);
    }
    fn save(&self, out: &mut Writer) {
        out.clear();
        Referee::save(self, out);
    }
    fn load(&mut self, data: &[u8]) -> bool {
        Referee::load(self, data).is_ok()
    }
    fn results(&self, out: &mut Writer) {
        out.clear();
        self.game().results(out);
    }
    fn hash(&self) -> u32 {
        Referee::hash(self)
    }
    fn tick(&self) -> u64 {
        Referee::tick(self)
    }
}

/// Makes a session from the seed and the host's config bytes.
pub type MakeSession = fn(seed: u64, config: &[u8]) -> Result<Box<dyn Session>, String>;

#[derive(Default)]
struct State {
    session: Option<Box<dyn Session>>,
    scratch: Vec<u8>,
    out: Writer,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// `(ptr << 32) | len` of a byte slice in this module's memory.
fn packed(b: &[u8]) -> i64 {
    ((b.as_ptr() as usize as u64 & 0xFFFF_FFFF) << 32 | (b.len() as u64 & 0xFFFF_FFFF)) as i64
}

/// The scratch bytes the host wrote at `ptr` (it must be the pointer
/// `alloc` returned, and `len` at most its size).
fn scratch(st: &State, ptr: u32, len: u32) -> Option<&[u8]> {
    let base = st.scratch.as_ptr() as usize as u32;
    (ptr == base || len == 0).then(|| st.scratch.get(..len as usize)).flatten()
}

pub fn alloc(len: u32) -> u32 {
    STATE.with_borrow_mut(|st| {
        let len = len as usize;
        if st.scratch.len() < len {
            st.scratch.resize(len, 0);
        }
        st.scratch.as_ptr() as usize as u32
    })
}

pub fn init(make: MakeSession, seed_lo: u32, seed_hi: u32, ptr: u32, len: u32) -> i32 {
    STATE.with_borrow_mut(|st| {
        let Some(cfg) = scratch(st, ptr, len).map(<[u8]>::to_vec) else {
            return -2;
        };
        let seed = u64::from(seed_lo) | (u64::from(seed_hi) << 32);
        match make(seed, &cfg) {
            Ok(s) => {
                st.session = Some(s);
                0
            }
            Err(_) => -4,
        }
    })
}

fn with_session<R>(none: R, f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(|st| if st.session.is_some() { f(st) } else { none })
}

pub fn join(peer: u32, role: u32) -> i32 {
    with_session(-1, |st| {
        let Some(s) = st.session.as_mut() else { return -1 };
        if role > 1 {
            return -2;
        }
        s.join(peer, role == 1).map_or(255, i32::from)
    })
}

pub fn leave(peer: u32) -> i32 {
    with_session(-1, |st| match st.session.as_mut() {
        Some(s) => {
            if s.leave(peer) {
                0
            } else {
                -3
            }
        }
        None => -1,
    })
}

pub fn input(peer: u32, ptr: u32, len: u32) -> i32 {
    with_session(-1, |st| {
        let Some(bytes) = scratch(st, ptr, len).map(<[u8]>::to_vec) else {
            return -2;
        };
        match st.session.as_mut() {
            Some(s) => {
                if s.input(peer, &bytes) {
                    0
                } else {
                    -3
                }
            }
            None => -1,
        }
    })
}

pub fn step(ticks: u32) -> i32 {
    with_session(-1, |st| match st.session.as_mut() {
        Some(s) => {
            s.step(ticks.min(600));
            0
        }
        None => -1,
    })
}

fn out_of(st: &mut State, f: impl FnOnce(&dyn Session, &mut Writer)) -> i64 {
    let State { session, out, .. } = st;
    match session.as_deref() {
        Some(s) => {
            f(s, out);
            packed(out.as_bytes())
        }
        None => -1,
    }
}

pub fn view(recipient: u32) -> i64 {
    with_session(-1, |st| out_of(st, |s, out| s.view((recipient != SPECTATOR).then_some(recipient), out)))
}

pub fn save() -> i64 {
    with_session(-1, |st| out_of(st, |s, out| s.save(out)))
}

pub fn results() -> i64 {
    with_session(-1, |st| out_of(st, |s, out| s.results(out)))
}

pub fn load(ptr: u32, len: u32) -> i32 {
    with_session(-1, |st| {
        let Some(bytes) = scratch(st, ptr, len).map(<[u8]>::to_vec) else {
            return -2;
        };
        match st.session.as_mut() {
            Some(s) => {
                if s.load(&bytes) {
                    0
                } else {
                    -3
                }
            }
            None => -1,
        }
    })
}

pub fn hash() -> i64 {
    with_session(-1, |st| st.session.as_ref().map_or(-1, |s| i64::from(s.hash())))
}

pub fn tick() -> i64 {
    with_session(-1, |st| st.session.as_ref().map_or(-1, |s| s.tick() as i64))
}

/// The bytes at a packed `(ptr << 32) | len` result, for native hosts and
/// tests in the same process (a wasm host reads its memory instead).
pub fn last_output() -> Vec<u8> {
    STATE.with_borrow(|st| st.out.as_bytes().to_vec())
}

/// Write `bytes` into the scratch buffer and return `(ptr, len)`, for
/// native hosts and tests in the same process.
pub fn put(bytes: &[u8]) -> (u32, u32) {
    let ptr = alloc(bytes.len() as u32);
    STATE.with_borrow_mut(|st| st.scratch[..bytes.len()].copy_from_slice(bytes));
    (ptr, bytes.len() as u32)
}

/// Export `evani-referee/1` from a game's referee crate. `$make` is a
/// [`MakeSession`]. The crate allows `unsafe_code` for the `no_mangle`
/// export attributes only (nothing else in it is unsafe).
#[macro_export]
macro_rules! export_referee {
    ($make:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_abi() -> u32 {
            $crate::referee::ABI
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_alloc(len: u32) -> u32 {
            $crate::referee::alloc(len)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_init(seed_lo: u32, seed_hi: u32, ptr: u32, len: u32) -> i32 {
            $crate::referee::init($make, seed_lo, seed_hi, ptr, len)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_join(peer: u32, role: u32) -> i32 {
            $crate::referee::join(peer, role)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_leave(peer: u32) -> i32 {
            $crate::referee::leave(peer)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_input(peer: u32, ptr: u32, len: u32) -> i32 {
            $crate::referee::input(peer, ptr, len)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_step(ticks: u32) -> i32 {
            $crate::referee::step(ticks)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_view(recipient: u32) -> i64 {
            $crate::referee::view(recipient)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_save() -> i64 {
            $crate::referee::save()
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_load(ptr: u32, len: u32) -> i32 {
            $crate::referee::load(ptr, len)
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_results() -> i64 {
            $crate::referee::results()
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_hash() -> i64 {
            $crate::referee::hash()
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn evani_ref_tick() -> i64 {
            $crate::referee::tick()
        }
    };
}
