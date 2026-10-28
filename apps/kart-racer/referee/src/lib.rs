//! Sugarpeak Rally's multiplayer referee: the same race rules the game runs
//! (`evani-kit-kart-racer` without its `render` part), as a lockstep referee
//! ([`evani_net::lockstep::Referee`]) with the AI racers as its bots.
//!
//! Built for a host's server with `cargo xtask build kart-racer --referee`
//! (wasm32, `--cfg evani_headless`): `referee.wasm` exports the neutral
//! `evani-referee/1` ABI ([`evani_net::referee`]) and imports nothing.
//! README.md says how to run it.

// The only unsafe-flagged code is the `#[unsafe(no_mangle)]` on the ABI
// exports that `export_referee!` writes; nothing here dereferences a pointer.
#![allow(unsafe_code)]

use evani_net::lockstep::{Referee, RefereeConfig};
use evani_net::referee::Session;

pub use evani_kit_kart_racer::net::{KartBrains, KartInput, NetRace, SEATS};

/// The Sugarpeak referee type.
pub type SugarpeakReferee = Referee<NetRace, KartBrains>;

/// A new session. `config`: optional byte 0 = laps (1 to 9, default 3).
pub fn referee(seed: u64, config: &[u8]) -> Result<SugarpeakReferee, String> {
    let game = NetRace::new(seed, config)?;
    Ok(Referee::new(game, KartBrains::new(seed), RefereeConfig::default()))
}

/// The ABI's session maker.
pub fn make_session(seed: u64, config: &[u8]) -> Result<Box<dyn Session>, String> {
    Ok(Box::new(referee(seed, config)?))
}

/// A fresh game for a client (its confirmed and predicted copies): the
/// state is replaced by the first view, so the seed does not matter.
pub fn client_copy() -> Result<NetRace, String> {
    NetRace::new(0, &[])
}

#[cfg(feature = "abi")]
evani_net::export_referee!(make_session);
