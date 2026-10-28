//! Static-world collision (`evani-physics` feature `collide`, off unless a
//! game turns it on): a triangle world with a BVH, raycasts that return the
//! surface's tag, line of sight, capsule and sphere casts, and a kinematic
//! character mover.
//!
//! * [`world`]: [`WorldBuilder`] takes triangles, indexed meshes, boxes and
//!   oriented boxes, each with a `u32` tag (a material or brush id the game
//!   chooses); [`StaticWorld`] holds them and a BVH and answers
//!   [`StaticWorld::raycast`], [`StaticWorld::raycast_filtered`] (skip some
//!   tags), [`StaticWorld::line_of_sight`] and
//!   [`StaticWorld::line_of_sight_filtered`].
//! * [`cast`]: [`StaticWorld::cast_capsule`] and [`StaticWorld::cast_sphere`]
//!   (thrown or fast objects; conservative advancement, so nothing tunnels
//!   through a thin wall), [`StaticWorld::contacts`],
//!   [`StaticWorld::overlaps`] and [`StaticWorld::depenetrate`].
//! * [`character`]: [`StaticWorld::move_character`], an upright capsule that
//!   slides along walls and creases, steps up ledges, walks walkable slopes,
//!   treats steeper ones as walls, stops under ceilings and snaps down
//!   stairs and ramps. The game keeps velocity, gravity and input.
//! * [`params`]: the `physics.collide.*` live tunables
//!   ([`params::CollideParams::config`] gives the mover config each step).
//! * [`trig`]: deterministic `sin`, `cos` and `atan2` for building oriented
//!   boxes and ramps.
//!
//! Deterministic: no clock, no randomness, no hashing; the BVH build and
//! every query visit candidates in a fixed order and break ties by triangle
//! index. Only `+ - * /`, `sqrt`, `floor` and plain comparisons (no platform
//! maths library), so native and wasm give the same bits. No dependencies
//! beyond `evani-core` (for the parameters).
//!
//! ```
//! use evani_physics::collide::{CharacterConfig, WorldBuilder};
//! let mut b = WorldBuilder::new();
//! b.aabb([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0], 1); // floor, tag 1
//! b.aabb([3.0, 0.0, -10.0], [3.5, 3.0, 10.0], 2); // wall, tag 2
//! let world = b.build();
//! let hit = world.raycast([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 100.0);
//! assert_eq!(hit.map(|h| h.tag), Some(2));
//! let cfg = CharacterConfig::default();
//! let r = world.move_character([0.0, 0.0, 0.0], 1.8, [0.05, 0.0, 0.0], &cfg, true);
//! assert!(r.grounded);
//! ```

pub mod aabb;
pub mod cast;
pub mod character;
pub mod closest;
pub mod math;
pub mod params;
pub mod trig;
pub mod world;

#[cfg(test)]
mod tests;

pub use aabb::Aabb;
pub use cast::{CONTACT_EPS, Capsule, CastHit, Contact};
pub use character::{CharacterConfig, MoveResult};
pub use math::V3;
pub use params::CollideParams;
pub use world::{CollideError, RayHit, StaticWorld, Tri, WorldBuilder};
