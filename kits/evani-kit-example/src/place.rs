//! Where and how the example kit's spinners stand: a square grid of
//! bevelled cubes and ringed orbs, each resting on the ground on its lowest
//! point while it turns about a tilted axis.
//!
//! The shapes are built-in meshes, so this works out the lowest point of the
//! turned mesh exactly (from the mesh's own vertex layout) and lifts the
//! spinner so that point sits [`CLEARANCE`] above the ground: it touches
//! the ground (or its contact-shadow disc) in every frame instead of
//! floating over it. [`lowest_point`] measures the drawn vertices, and the
//! `review_dump` test reports that measure as each spinner's gap.

use evani_render::math::quat_axis_angle;
use evani_render::mesh::{self, MeshId};
use evani_render::MeshInstance;

use crate::Spinner;

/// Distance between grid cells (m).
pub const SPACING: f32 = 1.6;
/// Edge of a cube spinner (m).
pub const CUBE: f32 = 0.9;
/// Diameter of an orb spinner's ball (m).
pub const ORB: f32 = 0.72;
/// An orb's ring: a thin disc through the ball's middle (x, y, z scale of
/// the unit cylinder), so the ball reads as a little gyroscope.
pub const RING: [f32; 3] = [1.3, 0.05, 1.3];
/// Sides of the built-in cylinder the ring is drawn with.
const RING_SIDES: u32 = 16;
/// Height of a spinner's lowest point over the ground (m): resting on its
/// shadow disc, well inside the 3 cm a geometry check allows.
pub const CLEARANCE: f32 = 0.012;
/// The tilted axis every spinner turns about (not unit length).
pub const AXIS: [f32; 3] = [0.3, 1.0, 0.2];
/// Shadow disc: a flat cone lying on the ground under each spinner, its
/// thickness and lift (m). Decals may sit right on their surface.
pub const DISC_THICK: f32 = 0.01;
pub const DISC_LIFT: f32 = 0.002;
/// One spinner in this many glows (the look's `glow` material).
pub const GLOW_EVERY: u32 = 7;

/// The two spinner shapes, alternating like a chessboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A bevelled cube.
    Cube,
    /// A ball with a thin ring round its middle.
    Orb,
}

/// Cells per grid row for `n` spinners: the smallest square that holds them.
pub fn side(n: usize) -> u32 {
    let mut s = (n as f64).sqrt().ceil() as u32;
    while (s as usize) * (s as usize) < n {
        s += 1;
    }
    s.max(1)
}

/// Column and row of spinner `i` in a grid `side` cells wide.
pub fn cell(i: u32, side: u32) -> (u32, u32) {
    (i % side, i / side)
}

/// The shape in a cell.
pub fn shape(col: u32, row: u32) -> Shape {
    if (col + row).is_multiple_of(2) { Shape::Cube } else { Shape::Orb }
}

/// Cheap integer hash (no RNG state): the same answer for the same index.
pub fn hash(i: u32, salt: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

/// Whether spinner `i` glows: about one in [`GLOW_EVERY`], scattered by a
/// hash so the glowing ones never line up in stripes.
pub fn glows(i: u32) -> bool {
    hash(i, 11).is_multiple_of(GLOW_EVERY)
}

/// A spinner's rotation for its angle.
pub fn rotation(angle: f32) -> [f32; 4] {
    quat_axis_angle(AXIS, angle)
}

/// Row y of the rotation matrix of unit quaternion `q`: the world height of
/// each local axis.
fn up_row(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q;
    [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)]
}

/// How far below its centre a turned spinner reaches (m): the depth of its
/// lowest vertex. Exact for the bevelled cube (its lowest vertex is a
/// chamfer corner) and the ring's 16-sided rim; the ball's reach is its
/// radius.
pub fn reach(shape: Shape, q: [f32; 4]) -> f32 {
    let r = up_row(q);
    match shape {
        Shape::Cube => {
            let c = mesh::BEVEL;
            let a = [r[0].abs(), r[1].abs(), r[2].abs()];
            CUBE * ((0.5 - c) * (a[0] + a[1] + a[2]) + c * a[0].max(a[1]).max(a[2]))
        }
        Shape::Orb => {
            let (ra, rb) = (RING[0] * 0.5, RING[1] * 0.5);
            let mut rim = 0.0f32;
            for k in 0..RING_SIDES {
                let (s, c) = (k as f32 / RING_SIDES as f32 * core::f32::consts::TAU).sin_cos();
                rim = rim.max(-(ra * (r[0] * c + r[2] * s)) + rb * r[1].abs());
            }
            (ORB * 0.5).max(rim)
        }
    }
}

/// What one spinner draws. Materials are left at 0: the caller sets them
/// from the look (it knows the look's material ids).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    pub shape: Shape,
    pub glows: bool,
    /// The cube, or the orb's ball.
    pub body: MeshInstance,
    /// The orb's ring (none for a cube).
    pub ring: Option<MeshInstance>,
    /// The shadow disc on the ground under it (a decal).
    pub disc: MeshInstance,
}

/// Spinner `i` of a grid `side` cells wide, centred on the origin, on
/// ground at height `ground_y`.
pub fn parts(i: u32, side: u32, s: &Spinner, ground_y: f32) -> Parts {
    let (col, row) = cell(i % (side * side), side);
    let half = (side as f32 - 1.0) * SPACING * 0.5;
    let (x, z) = (col as f32 * SPACING - half, row as f32 * SPACING - half);
    let shape = shape(col, row);
    let q = rotation(s.angle);
    let y = ground_y + CLEARANCE + reach(shape, q);
    let (body, ring, d) = match shape {
        Shape::Cube => {
            let mut b = MeshInstance::new(MeshId::BEVEL_CUBE, [x, y, z]);
            b.rotation = q;
            b.scale = [CUBE; 3];
            (b, None, CUBE * 1.25)
        }
        Shape::Orb => {
            let mut b = MeshInstance::new(MeshId::SPHERE, [x, y, z]);
            b.rotation = q;
            b.scale = [ORB; 3];
            let mut r = MeshInstance::new(MeshId::CYLINDER, [x, y, z]);
            r.rotation = q;
            r.scale = RING;
            (b, Some(r), RING[0] * 0.95)
        }
    };
    let mut disc = MeshInstance::new(MeshId::CONE, [x, ground_y + DISC_LIFT + DISC_THICK * 0.5, z]);
    disc.scale = [d, DISC_THICK, d];
    Parts { shape, glows: glows(i), body, ring, disc }
}

/// The lowest point (m) of the drawn vertices of `inst`, measured from the
/// built-in mesh itself (none for an unknown mesh).
pub fn lowest_point(inst: &MeshInstance) -> Option<f32> {
    let m = mesh::builtin(inst.mesh)?;
    let r = up_row(inst.rotation);
    m.vertices
        .iter()
        .map(|v| {
            let p = [v[0] * inst.scale[0], v[1] * inst.scale[1], v[2] * inst.scale[2]];
            inst.position[1] + r[0] * p[0] + r[1] * p[1] + r[2] * p[2]
        })
        .reduce(f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_side_holds_every_spinner() {
        assert_eq!(side(400), 20);
        assert_eq!(side(401), 21);
        assert_eq!(side(64), 8);
        assert_eq!(side(1), 1);
        assert_eq!(side(0), 1);
    }

    #[test]
    fn spinners_rest_on_the_ground_at_every_angle() {
        for i in 0..2u32 {
            for k in 0..720 {
                let s = Spinner { angle: k as f32 * 0.0087, rate: 1.0 };
                let p = parts(i, 20, &s, 0.0);
                let mut low = lowest_point(&p.body).unwrap();
                if let Some(r) = p.ring {
                    low = low.min(lowest_point(&r).unwrap());
                }
                let gap = low;
                assert!(gap >= CLEARANCE - 1e-4, "{:?} at {}: sunk, gap {gap}", p.shape, s.angle);
                // The ball is a UV sphere: its lowest vertex can sit a few
                // millimetres above the true sphere.
                assert!(gap <= CLEARANCE + 0.006, "{:?} at {}: floats, gap {gap}", p.shape, s.angle);
            }
        }
    }

    #[test]
    fn glow_pieces_are_scattered_about_one_in_seven() {
        let n = (0..400).filter(|&i| glows(i)).count();
        assert!((40..=75).contains(&n), "{n}");
        // No full row or column of glowing pieces.
        for r in 0..20 {
            assert!((0..20).filter(|c| glows(r * 20 + c)).count() < 8);
        }
    }
}
