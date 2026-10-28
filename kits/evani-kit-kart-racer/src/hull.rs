//! A kart's footprint for kart-against-kart contact and for the chase
//! camera's probe: a capsule on the ground (two circles joined by a segment
//! along the kart), so a long kart can no longer slide its nose or tail
//! through another kart the way a single circle of one radius allowed.
//!
//! The kart models are about 3.1 m long and 2.1 m wide; with the default
//! `kart_radius` (1.1 m) the segment between the two circle centres is
//! 2 x [`HALF_SEGMENT`] long, so the capsule is about as long as the model.
//! Pure functions, no allocation.

/// Half the distance between the nose circle's centre and the tail circle's
/// (m): 1.1 m radius + 0.45 m = the kart model's 1.55 m half length.
pub const HALF_SEGMENT: f32 = 0.45;

/// A capsule footprint: the segment `a..b` and a radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hull {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub radius: f32,
}

impl Hull {
    /// The footprint of a kart at `pos` (x and z used) whose body points
    /// along `yaw` (the heading, plus the drift's slip angle when you draw
    /// the body at it).
    pub fn new(pos: [f32; 3], yaw: f32, radius: f32) -> Self {
        let f = [yaw.sin() * HALF_SEGMENT, yaw.cos() * HALF_SEGMENT];
        Self {
            a: [pos[0] - f[0], pos[2] - f[1]],
            b: [pos[0] + f[0], pos[2] + f[1]],
            radius,
        }
    }

    /// The middle of the segment.
    pub fn centre(&self) -> [f32; 2] {
        [(self.a[0] + self.b[0]) * 0.5, (self.a[1] + self.b[1]) * 0.5]
    }

    /// Distance from the point `p` to the hull's segment (not minus the
    /// radius).
    pub fn distance_to_segment(&self, p: [f32; 2]) -> f32 {
        let d = [self.b[0] - self.a[0], self.b[1] - self.a[1]];
        let l2 = d[0] * d[0] + d[1] * d[1];
        let t = if l2 < 1e-9 {
            0.0
        } else {
            (((p[0] - self.a[0]) * d[0] + (p[1] - self.a[1]) * d[1]) / l2).clamp(0.0, 1.0)
        };
        let c = [self.a[0] + d[0] * t, self.a[1] + d[1] * t];
        ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt()
    }
}

/// Two hulls touching: the unit normal pointing from the first to the second
/// and how deep they overlap (m).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub normal: [f32; 2],
    pub depth: f32,
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// The closest points of two segments (Ericson, Real-Time Collision
/// Detection 5.1.9), in 2D.
fn closest_points(p1: [f32; 2], q1: [f32; 2], p2: [f32; 2], q2: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    const EPS: f32 = 1e-7;
    let d1 = [q1[0] - p1[0], q1[1] - p1[1]];
    let d2 = [q2[0] - p2[0], q2[1] - p2[1]];
    let r = [p1[0] - p2[0], p1[1] - p2[1]];
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let (s, t);
    if a <= EPS && e <= EPS {
        (s, t) = (0.0, 0.0);
    } else if a <= EPS {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= EPS {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > EPS { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t) = (s0, t0);
        }
    }
    (
        [p1[0] + d1[0] * s, p1[1] + d1[1] * s],
        [p2[0] + d2[0] * t, p2[1] + d2[1] * t],
    )
}

/// How the capsules `a` and `b` overlap, or `None` when they are apart.
pub fn contact(a: &Hull, b: &Hull) -> Option<Contact> {
    let (ca, cb) = closest_points(a.a, a.b, b.a, b.b);
    let r = a.radius + b.radius;
    let d = [cb[0] - ca[0], cb[1] - ca[1]];
    let d2 = dot(d, d);
    if d2 >= r * r {
        return None;
    }
    let dist = d2.sqrt();
    let normal = if dist > 1e-4 {
        [d[0] / dist, d[1] / dist]
    } else {
        // The segments cross: push along the line between the middles.
        let (m, n) = (a.centre(), b.centre());
        let c = [n[0] - m[0], n[1] - m[1]];
        let l = dot(c, c).sqrt();
        if l > 1e-4 { [c[0] / l, c[1] / l] } else { [1.0, 0.0] }
    };
    Some(Contact { normal, depth: r - dist })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hull(x: f32, z: f32, yaw: f32) -> Hull {
        Hull::new([x, 0.0, z], yaw, 1.1)
    }

    #[test]
    fn hulls_are_as_long_as_the_kart_model() {
        let h = hull(0.0, 0.0, 0.0);
        // Pointing along +z: nose and tail circles a segment apart.
        assert!((h.b[1] - h.a[1] - 2.0 * HALF_SEGMENT).abs() < 1e-5);
        assert!((h.radius + HALF_SEGMENT - 1.55).abs() < 1e-5);
    }

    #[test]
    fn a_nose_into_a_tail_overlaps_where_one_circle_would_not() {
        // 2.8 m apart nose to tail: two plain 1.1 m circles (2.2 m) say clear,
        // but the 3.1 m long karts overlap by 0.3 m.
        let (a, b) = (hull(0.0, 0.0, 0.0), hull(0.0, 2.8, 0.0));
        let c = contact(&a, &b).expect("overlap");
        assert!((c.depth - 0.3).abs() < 1e-4, "{}", c.depth);
        assert!(c.normal[1] > 0.99, "pushed apart along the length");
    }

    #[test]
    fn side_by_side_karts_touch_at_two_radii() {
        assert!(contact(&hull(0.0, 0.0, 0.0), &hull(2.25, 0.3, 0.0)).is_none());
        let c = contact(&hull(0.0, 0.0, 0.0), &hull(2.0, 0.3, 0.0)).expect("touching");
        assert!((c.depth - 0.2).abs() < 1e-4);
        assert!(c.normal[0] > 0.99);
    }

    #[test]
    fn crossed_karts_still_get_a_direction() {
        // A kart square across another one, middles 0.5 m apart.
        let c = contact(&hull(0.0, 0.0, 0.0), &hull(0.0, 0.5, core::f32::consts::FRAC_PI_2)).expect("overlap");
        assert!(c.depth > 2.0);
        assert!((c.normal[0].hypot(c.normal[1]) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_contact_is_symmetric() {
        let (a, b) = (hull(0.0, 0.0, 0.3), hull(1.7, 2.1, -0.4));
        let (ab, ba) = (contact(&a, &b).unwrap(), contact(&b, &a).unwrap());
        assert!((ab.depth - ba.depth).abs() < 1e-5);
        assert!((ab.normal[0] + ba.normal[0]).abs() < 1e-4 && (ab.normal[1] + ba.normal[1]).abs() < 1e-4);
    }

    #[test]
    fn distance_to_the_segment_clamps_at_the_ends() {
        let h = hull(0.0, 0.0, 0.0);
        assert!((h.distance_to_segment([1.0, 0.0]) - 1.0).abs() < 1e-5);
        assert!((h.distance_to_segment([0.0, 2.45]) - 2.0).abs() < 1e-5);
    }
}
