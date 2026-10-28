//! Swept shapes against the static world: capsule casts (characters) and
//! sphere casts (grenades), overlap tests and depenetration.
//!
//! A cast finds the first time of contact by conservative advancement on
//! each candidate triangle. The distance between a moving convex shape and
//! a fixed triangle is a convex function of the travelled fraction, so its
//! tangent never overshoots the true contact: stepping to the tangent's
//! zero is safe, converges from below, and stops as soon as the shape is
//! moving away (the distance can then never fall again).

use crate::collide::aabb::Aabb;
use crate::collide::closest::segment_triangle;
use crate::collide::math::{V3, add, dot, fmax, len_sq, mad, max, min, normalize, scale, sub};
use crate::collide::world::StaticWorld;

/// Contact tolerance: a cast stops when the gap is below this (m).
pub const CONTACT_EPS: f32 = 1e-4;
/// Most advancement steps per triangle.
const MAX_STEPS: usize = 48;

/// A capsule: the segment `a..b` swollen by `radius` (a sphere when `a == b`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    /// One end of the core segment.
    pub a: V3,
    /// The other end.
    pub b: V3,
    /// Radius.
    pub radius: f32,
}

impl Capsule {
    /// A sphere.
    pub fn sphere(centre: V3, radius: f32) -> Self {
        Capsule { a: centre, b: centre, radius }
    }

    /// An upright capsule standing with its lowest point at `feet`, `height`
    /// tall overall (clamped to at least `2 * radius`).
    pub fn upright(feet: V3, radius: f32, height: f32) -> Self {
        let h = fmax(height, 2.0 * radius);
        Capsule { a: [feet[0], feet[1] + radius, feet[2]], b: [feet[0], feet[1] + h - radius, feet[2]], radius }
    }

    /// The capsule moved by `d`.
    pub fn moved(&self, d: V3) -> Self {
        Capsule { a: add(self.a, d), b: add(self.b, d), radius: self.radius }
    }

    /// Bounds.
    pub fn bounds(&self) -> Aabb {
        Aabb { min: min(self.a, self.b), max: max(self.a, self.b) }.grown(self.radius)
    }
}

/// The first contact of a cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CastHit {
    /// Fraction of the motion travelled before contact (0..=1).
    pub fraction: f32,
    /// Distance travelled before contact (m).
    pub distance: f32,
    /// Contact normal, from the world towards the shape (unit).
    pub normal: V3,
    /// Contact point on the world.
    pub point: V3,
    /// Triangle index.
    pub tri: u32,
    /// The triangle's tag.
    pub tag: u32,
}

/// One overlap found by [`StaticWorld::contacts`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// Push direction out of the triangle (unit).
    pub normal: V3,
    /// How deep the shape is in (m, > 0).
    pub depth: f32,
    /// Triangle index.
    pub tri: u32,
    /// The triangle's tag.
    pub tag: u32,
}

/// Separation normal between the shape's closest point `s` and the
/// triangle's closest point `t`, or the face normal turned towards
/// `towards` when they coincide.
fn separation(s: V3, t: V3, face: V3, towards: V3) -> V3 {
    match normalize(sub(s, t)) {
        Some(n) => n,
        None => {
            if dot(face, towards) >= 0.0 {
                face
            } else {
                scale(face, -1.0)
            }
        }
    }
}

impl StaticWorld {
    /// Sweep `shape` along `motion`; the first contact, or `None` if the whole
    /// motion is free. A shape already touching a triangle stops at 0 only if
    /// the motion pushes into it, so a body resting on a floor or against a
    /// wall can always slide along or move away.
    pub fn cast_capsule(&self, shape: &Capsule, motion: V3) -> Option<CastHit> {
        let total = len_sq(motion).sqrt();
        if total.is_nan() || total <= 1e-9 || !total.is_finite() {
            return None;
        }
        let region = shape.bounds().swept(motion).grown(CONTACT_EPS * 2.0);
        let mut cand = Vec::new();
        self.query_aabb(&region, &mut cand);
        let mut best: Option<CastHit> = None;
        for &ti in &cand {
            let Some(tri) = self.tri(ti) else { continue };
            let Some((fraction, normal, point)) = toi(shape, motion, tri.p, tri.n) else {
                continue;
            };
            let better = best.is_none_or(|b| fraction < b.fraction);
            if better {
                best = Some(CastHit { fraction, distance: fraction * total, normal, point, tri: ti, tag: tri.tag });
            }
        }
        best
    }

    /// Sweep a sphere (a grenade) along `motion`.
    pub fn cast_sphere(&self, centre: V3, radius: f32, motion: V3) -> Option<CastHit> {
        self.cast_capsule(&Capsule::sphere(centre, radius), motion)
    }

    /// Every triangle the shape overlaps by more than [`CONTACT_EPS`],
    /// ascending by triangle.
    pub fn contacts(&self, shape: &Capsule, out: &mut Vec<Contact>) {
        out.clear();
        let mut cand = Vec::new();
        self.query_aabb(&shape.bounds(), &mut cand);
        let mid = scale(add(shape.a, shape.b), 0.5);
        for &ti in &cand {
            let Some(tri) = self.tri(ti) else { continue };
            let c = segment_triangle(shape.a, shape.b, tri.p[0], tri.p[1], tri.p[2]);
            let d = c.dist_sq.sqrt();
            let depth = shape.radius - d;
            if depth > CONTACT_EPS {
                let towards = sub(mid, tri.p[0]);
                out.push(Contact { normal: separation(c.on_seg, c.on_tri, tri.n, towards), depth, tri: ti, tag: tri.tag });
            }
        }
    }

    /// Whether the shape overlaps anything (by more than [`CONTACT_EPS`]).
    pub fn overlaps(&self, shape: &Capsule) -> bool {
        let mut out = Vec::new();
        self.contacts(shape, &mut out);
        !out.is_empty()
    }

    /// Push the shape out of whatever it overlaps: up to `iterations` passes,
    /// each moving it out of its deepest contact. Returns the total push.
    pub fn depenetrate(&self, shape: &Capsule, iterations: usize) -> V3 {
        let mut push = [0.0; 3];
        let mut out = Vec::new();
        for _ in 0..iterations {
            self.contacts(&shape.moved(push), &mut out);
            let Some(deepest) = out.iter().copied().reduce(|a, b| if b.depth > a.depth { b } else { a }) else {
                break;
            };
            push = mad(push, deepest.normal, deepest.depth + CONTACT_EPS);
        }
        push
    }
}

/// Time of impact of `shape` moving by `motion` against one triangle:
/// `(fraction, normal, point on the triangle)`.
fn toi(shape: &Capsule, motion: V3, p: [V3; 3], face: V3) -> Option<(f32, V3, V3)> {
    let mut t = 0.0f32;
    let mid = scale(add(shape.a, shape.b), 0.5);
    for _ in 0..MAX_STEPS {
        let off = scale(motion, t);
        let (a, b) = (add(shape.a, off), add(shape.b, off));
        let c = segment_triangle(a, b, p[0], p[1], p[2]);
        let d = c.dist_sq.sqrt();
        let gap = d - shape.radius;
        let n = separation(c.on_seg, c.on_tri, face, sub(add(mid, off), p[0]));
        // How fast the gap closes per unit fraction (the tangent's slope).
        let closing = -dot(motion, n);
        if gap <= CONTACT_EPS {
            // Touching (or inside): a contact only if the motion pushes in.
            return if closing > 0.0 { Some((t, n, c.on_tri)) } else { None };
        }
        if closing <= 1e-9 {
            // Moving away or parallel: the convex gap can never shrink again.
            return None;
        }
        t += gap / closing;
        if t > 1.0 {
            return None;
        }
    }
    // Still approaching after the step budget (a grazing approach): stop
    // here, which is always before the true contact.
    let off = scale(motion, t);
    let c = segment_triangle(add(shape.a, off), add(shape.b, off), p[0], p[1], p[2]);
    let n = separation(c.on_seg, c.on_tri, face, sub(add(mid, off), p[0]));
    Some((t, n, c.on_tri))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::world::WorldBuilder;

    fn floor_and_wall() -> StaticWorld {
        let mut b = WorldBuilder::new();
        b.aabb([-50.0, -1.0, -50.0], [50.0, 0.0, 50.0], 1); // floor, top at y = 0
        b.aabb([5.0, 0.0, -50.0], [6.0, 4.0, 50.0], 2); // wall, face at x = 5
        b.build()
    }

    #[test]
    fn sphere_hits_wall_face() {
        let w = floor_and_wall();
        let h = w.cast_sphere([0.0, 1.0, 0.0], 0.5, [10.0, 0.0, 0.0]).unwrap();
        assert!((h.distance - 4.5).abs() < 1e-3, "{h:?}");
        assert!((h.normal[0] + 1.0).abs() < 1e-4);
        assert_eq!(h.tag, 2);
        // Short of the wall: free.
        assert!(w.cast_sphere([0.0, 1.0, 0.0], 0.5, [4.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn sphere_hits_box_edge_with_slanted_normal() {
        let mut b = WorldBuilder::new();
        b.aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 0);
        let w = b.build();
        // Passing over the top edge at x = 0: the contact normal leans up and back.
        let h = w.cast_sphere([-2.0, 1.3, 0.5], 0.5, [4.0, 0.0, 0.0]).unwrap();
        assert!(h.normal[0] < 0.0 && h.normal[1] > 0.0, "{h:?}");
        let expect_x = -(0.5f32 * 0.5 - 0.3 * 0.3).sqrt();
        assert!((h.distance - (expect_x + 2.0)).abs() < 2e-3, "{h:?}");
    }

    #[test]
    fn resting_capsule_slides_but_cannot_sink() {
        let w = floor_and_wall();
        let c = Capsule::upright([0.0, 0.0, 0.0], 0.3, 1.37);
        // Resting on the floor: down is blocked at once, sideways is free.
        let down = w.cast_capsule(&c, [0.0, -1.0, 0.0]).unwrap();
        assert!(down.fraction < 1e-3 && down.normal[1] > 0.99, "{down:?}");
        assert!(w.cast_capsule(&c, [2.0, 0.0, 1.0]).is_none());
        // Up and away from the floor is free.
        assert!(w.cast_capsule(&c, [0.0, 0.5, 0.0]).is_none());
        // Into the wall stops at its face minus the radius.
        let h = w.cast_capsule(&c, [10.0, 0.0, 0.0]).unwrap();
        assert!((h.distance - 4.7).abs() < 1e-3, "{h:?}");
    }

    #[test]
    fn fast_cast_never_tunnels_thin_wall() {
        let mut b = WorldBuilder::new();
        b.aabb([10.0, -5.0, -5.0], [10.02, 5.0, 5.0], 0); // 2 cm thick
        let w = b.build();
        let h = w.cast_sphere([0.0; 3], 0.1, [1000.0, 0.0, 0.0]).unwrap();
        assert!((h.distance - 9.9).abs() < 1e-2, "{h:?}");
    }

    #[test]
    fn contacts_and_depenetrate() {
        let w = floor_and_wall();
        // Sunk 0.1 m into the floor.
        let c = Capsule::upright([0.0, -0.1, 0.0], 0.3, 1.37);
        assert!(w.overlaps(&c));
        let push = w.depenetrate(&c, 4);
        assert!((push[1] - 0.1).abs() < 1e-3 && push[0].abs() < 1e-5, "{push:?}");
        assert!(!w.overlaps(&c.moved(push)));
        // Standing clear: no contacts.
        assert!(!w.overlaps(&Capsule::upright([0.0, 0.001, 0.0], 0.3, 1.37)));
    }

    #[test]
    fn casts_are_bit_identical_across_runs() {
        let w = floor_and_wall();
        let c = Capsule::upright([0.1, 0.2, -0.3], 0.3, 1.37);
        let a = w.cast_capsule(&c, [7.0, -0.4, 1.3]);
        for _ in 0..10 {
            assert_eq!(w.cast_capsule(&c, [7.0, -0.4, 1.3]), a);
        }
    }
}
