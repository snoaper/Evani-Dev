//! Axis-aligned bounding boxes and the ray slab test the BVH uses.

use crate::collide::math::{V3, fmax, fmin, max, min};

/// An axis-aligned box from `min` to `max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Lowest corner.
    pub min: V3,
    /// Highest corner.
    pub max: V3,
}

impl Aabb {
    /// The empty box (grows to fit whatever is added).
    pub const EMPTY: Aabb = Aabb { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] };

    /// A box from two corners in any order.
    pub fn new(a: V3, b: V3) -> Self {
        Aabb { min: min(a, b), max: max(a, b) }
    }

    /// The box of three points (a triangle).
    pub fn of_tri(a: V3, b: V3, c: V3) -> Self {
        Aabb { min: min(min(a, b), c), max: max(max(a, b), c) }
    }

    /// This box grown to hold `o`.
    pub fn union(self, o: Aabb) -> Self {
        Aabb { min: min(self.min, o.min), max: max(self.max, o.max) }
    }

    /// This box grown by `r` on every side.
    pub fn grown(self, r: f32) -> Self {
        Aabb { min: [self.min[0] - r, self.min[1] - r, self.min[2] - r], max: [self.max[0] + r, self.max[1] + r, self.max[2] + r] }
    }

    /// This box swept along `d` (the union of it and itself moved by `d`).
    pub fn swept(self, d: V3) -> Self {
        let moved = Aabb { min: crate::collide::math::add(self.min, d), max: crate::collide::math::add(self.max, d) };
        self.union(moved)
    }

    /// Whether the two boxes touch or overlap.
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min[0] <= o.max[0]
            && self.max[0] >= o.min[0]
            && self.min[1] <= o.max[1]
            && self.max[1] >= o.min[1]
            && self.min[2] <= o.max[2]
            && self.max[2] >= o.min[2]
    }

    /// Whether `p` is inside (or on) the box.
    pub fn contains(&self, p: V3) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }

    /// Centre.
    pub fn centre(&self) -> V3 {
        [(self.min[0] + self.max[0]) * 0.5, (self.min[1] + self.max[1]) * 0.5, (self.min[2] + self.max[2]) * 0.5]
    }

    /// Size along each axis.
    pub fn size(&self) -> V3 {
        crate::collide::math::sub(self.max, self.min)
    }

    /// Entry distance of the ray `o + d t` for `t` in `[0, t_max]`, or `None`
    /// on a miss. A zero component of `d` is handled exactly (no NaN from
    /// `0 * inf`): the ray then hits only if it starts inside that slab.
    pub fn ray_enter(&self, o: V3, d: V3, t_max: f32) -> Option<f32> {
        let mut t0 = 0.0f32;
        let mut t1 = t_max;
        for i in 0..3 {
            if d[i] == 0.0 {
                if o[i] < self.min[i] || o[i] > self.max[i] {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / d[i];
            let (mut a, mut b) = ((self.min[i] - o[i]) * inv, (self.max[i] - o[i]) * inv);
            if a > b {
                core::mem::swap(&mut a, &mut b);
            }
            t0 = fmax(t0, a);
            t1 = fmin(t1, b);
            if t0 > t1 {
                return None;
            }
        }
        Some(t0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_slab_hits_and_misses() {
        let b = Aabb::new([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
        assert_eq!(b.ray_enter([-5.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0), Some(4.0));
        assert_eq!(b.ray_enter([-5.0, 2.0, 0.0], [1.0, 0.0, 0.0], 100.0), None);
        assert_eq!(b.ray_enter([-5.0, 0.0, 0.0], [1.0, 0.0, 0.0], 3.0), None);
        // Starting inside enters at 0.
        assert_eq!(b.ray_enter([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0), Some(0.0));
        // On the slab boundary with a zero component: still a hit, no NaN.
        assert_eq!(b.ray_enter([-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 100.0), Some(4.0));
    }

    #[test]
    fn union_sweep_overlap() {
        let a = Aabb::new([0.0; 3], [1.0; 3]);
        let s = a.swept([2.0, 0.0, 0.0]);
        assert_eq!(s.max, [3.0, 1.0, 1.0]);
        assert!(s.overlaps(&Aabb::new([2.5, 0.5, 0.5], [4.0, 4.0, 4.0])));
        assert!(!a.overlaps(&Aabb::new([1.5, 0.0, 0.0], [2.0, 1.0, 1.0])));
        assert_eq!(Aabb::EMPTY.union(a), a);
    }
}
