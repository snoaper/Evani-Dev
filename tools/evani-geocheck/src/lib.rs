//! Evani.Dev geometry integrity checks (dev tool; never in a game download).
//!
//! The library is pure geometry and wasm-safe: evani-live uses it (with
//! `default-features = false`) for the in-game geometry debug view, and the
//! `evani-geocheck` command-line tool (feature `cli`) uses it for the full
//! report. See docs/GEOCHECK.md.

pub mod checks;
pub mod geom;
pub mod image;
pub mod meshcheck;
pub mod report;
pub mod scene;
pub mod shape;
pub mod sweep;
pub mod tags;
pub mod zfight;
