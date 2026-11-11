//! Evani.Model construct: the second back end of Evani.Model
//! (docs/MODELGEN_PLAN.md section 3). Where the field back end melts soft
//! volumes together, construct builds exact pieces (boards, timbers, ropes,
//! solids of revolution) along curves and surfaces, with seeded variation
//! per piece, and keeps every piece crisp at every render tier.
//!
//! Build and convert time only: nothing here reaches a game download. The
//! crate is std plus serde_json, deterministic (f64 IEEE basic operations in
//! a fixed order, our own transcendental functions in [`dmath`], seeds per
//! piece in [`seed`]) and WebAssembly-clean, so a later runtime module could
//! run the same recipes (plan 3.3, Stage 7).
//!
//! Layers, bottom up: shared maths ([`vec`], [`dmath`], [`rng`], [`expr`],
//! also re-exported by evani-model), [`formula`] (compiled recipe formulas),
//! [`curve`] (curves, arc-length paths, frames), [`surface`] (surfaces,
//! bands, boards), [`mesh`] (closed exact solids), [`model`] (pieces, groups,
//! instances, anchors, stages, the split check) and [`recipe`] (the JSON
//! recipe loader and interpreter).

// Index loops mirror the maths they implement (grids, rings, corners).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments, clippy::type_complexity)]

pub mod check;
pub mod curve;
pub mod dmath;
pub mod expr;
pub mod formula;
pub mod mesh;
pub mod model;
pub mod recipe;
pub mod rng;
pub mod seed;
pub mod surface;
pub mod vec;
