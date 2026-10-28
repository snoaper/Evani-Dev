//! evani-net tests (package N1 of docs/NET_BUILD_PLAN.md): one test binary
//! so the netcode is linked once.
//!
//! * `wire`: encoding round trips, range and length limits, garbage in.
//! * `referee`: the server's determinism, joins and leaves, bots, refusals,
//!   timeouts, hostile packets.
//! * `loopback`: 2 to 8 clients plus bots against the referee over the
//!   loopback with latency, jitter and loss; snapshots and settled
//!   predictions equal the referee's state.
//! * `synctest`: the sync-test guard on the toy game and on games broken
//!   on purpose.
//! * `float_probe`: float results natively and in wasm32, compared.

mod float_kernel;
mod float_probe;
mod loopback;
mod referee;
mod support;
mod synctest;
mod wire;
