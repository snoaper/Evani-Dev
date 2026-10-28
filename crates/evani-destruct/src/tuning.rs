//! Every destruction number in one place, with Hexagon's values as the
//! defaults (`hexagon-concrete-engine/src/tuning.js`). Lengths in metres,
//! times in seconds (Hexagon used milliseconds), angles in radians.
//! [`crate::DestructParams::read`] fills one from the live `destruct.*`
//! params each tick.

/// The tuning every part of the module reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    // ---- holes
    /// Most holes one bucket lists before hits there merge or recycle.
    pub hole_cap: usize,
    /// Most holes in a field; past it the field recycles, never dropping a hit.
    pub hole_max: usize,
    /// Largest hole radius, merged holes included.
    pub hole_rmax: f32,

    // ---- structural collapse
    /// Share of a cell's 27 samples that must be left for it to carry load.
    pub cell_min_solid: f32,
    /// Share of a group's footprint the floor must still hold for it to stand.
    pub support_min: f32,
    /// How far the centre of mass may sit past the held hull before it tips.
    pub tip_margin: f32,
    /// Tilt at which a block turning on its edge slides off and falls free.
    pub tip_release: f32,
    /// Largest tilt a falling block reaches.
    pub tip_max: f32,
    /// Starting turn speed of falling concrete (half for edge tips and slabs).
    pub tip_spin: f32,
    /// Floor joins thinner than this carry nothing.
    pub floor_sliver_min: f32,
    /// Floor pieces up to this area (m²) are cut away instead of falling.
    pub floor_crumb_max: f32,
    /// Gravity for falling concrete.
    pub fall_gravity: f32,
    /// Most blocks falling at once; more crumble where they stand.
    pub fall_max: usize,
    /// Below this height a falling block has left the world.
    pub fall_death_y: f32,
    /// Impact crater radius per landed block (times the cube root of its cells).
    pub collapse_hole_r: f32,
    /// Cap on the impact crater radius.
    pub collapse_hole_max: f32,
    /// How far above the touch-down point the impact crater is centred.
    pub collapse_up: f32,
    /// Debris chunks per cell of a landed block (at most 80 per block).
    pub collapse_debris: u32,

    // ---- debris
    /// Chunks per metre of hole radius.
    pub debris_per_m: f32,
    /// Fewest chunks per hole (most: 64).
    pub debris_min: u32,
    /// Smallest gravel.
    pub debris_size_min: f32,
    /// Largest lump (also capped at max(1.6 x min, 0.45 x hole radius)).
    pub debris_size_max: f32,
    /// Debris gravity.
    pub debris_gravity: f32,
    /// Share of speed kept on a bounce.
    pub debris_bounce: f32,
    /// Share of sliding speed and spin kept on every bounce.
    pub debris_friction: f32,
    /// Below this speed on a floor a chunk lies still.
    pub debris_rest_speed: f32,
    /// Time at rest before a chunk fades.
    pub debris_settle: f32,
    /// Sink-and-shrink time.
    pub debris_fade: f32,
    /// Longest life of a chunk (each lives 85 to 100 % of it).
    pub debris_life: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            hole_cap: 24,
            hole_max: 1800,
            hole_rmax: 4.0,
            cell_min_solid: 0.35,
            support_min: 0.3,
            tip_margin: 0.05,
            tip_release: 0.6,
            tip_max: 1.45,
            tip_spin: 0.8,
            floor_sliver_min: 1.0,
            floor_crumb_max: 1.0,
            fall_gravity: 20.0,
            fall_max: 12,
            fall_death_y: -30.0,
            collapse_hole_r: 0.8,
            collapse_hole_max: 1.6,
            collapse_up: 0.5,
            collapse_debris: 10,
            debris_per_m: 24.0,
            debris_min: 3,
            debris_size_min: 0.025,
            debris_size_max: 0.42,
            debris_gravity: 20.0,
            debris_bounce: 0.35,
            debris_friction: 0.6,
            debris_rest_speed: 0.9,
            debris_settle: 2.5,
            debris_fade: 0.9,
            debris_life: 9.0,
        }
    }
}

/// Debris pool size (Hexagon `debrisMax`). Fixed at creation; not live.
pub const DEBRIS_MAX: usize = 512;
/// Most dust puffs alive (Hexagon: 900, shared with its sparks).
pub const DUST_MAX: usize = 900;
