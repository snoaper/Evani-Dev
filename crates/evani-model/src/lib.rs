//! Evani.Model: an original, AI-native 3D modeler (docs/EVANI_MODEL_BRIEF.md).
//!
//! An agent writes intent (`.evmodel`); this library resolves it through a
//! class template and a style profile into a part graph, solves the
//! relationships, builds signed distance fields, meshes them per render tier,
//! checks the result by maths and writes glTF 2.0 with anchors, collision and
//! metadata. Development only: nothing here reaches a game download.
//!
//! Every decision path is deterministic: f64 IEEE basic operations in a fixed
//! order, our own transcendental functions (`dmath`), no hash-map iteration,
//! and parallel work merged in tile order (`rng::map_tiles`).

// Index loops mirror the maths they implement (matrices, grids, corners).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments, clippy::type_complexity)]

pub mod api;
pub mod build;
pub mod collide;
pub mod compare;
pub mod compile;
pub mod dmath;
pub mod expr;
pub mod generator;
pub mod gltf;
pub mod intent;
pub mod mesh;
pub mod meshcheck;
pub mod must;
pub mod png;
pub mod pose;
pub mod preview;
pub mod rig;
pub mod sdf;
pub mod rng;
pub mod sha256;
pub mod solve;
pub mod surface;
pub mod vec;
