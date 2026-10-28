//! Instant bullets: a ray against the map's real triangles (what it hit,
//! where, which way the surface faces, its tag) and against body hitboxes,
//! nearest first. A wall in front of a body stops the bullet.

use evani_physics::collide::math::{fmax, fmin};
use evani_physics::collide::{StaticWorld, V3};

use crate::health::Zone;

/// A body that can be shot: a head sphere over an upright cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// Lowest point (m).
    pub feet: V3,
    /// Standing (or crouched) height (m).
    pub height: f32,
    /// Eye height over the feet (m); the head sits round it.
    pub eye: f32,
    /// Body cylinder radius (m).
    pub radius: f32,
    /// Head sphere radius (m).
    pub head: f32,
    pub alive: bool,
}

impl Body {
    /// A body with adult proportions (a 0.2 m body radius, a 0.14 m head)
    /// for its height and eye height.
    pub fn new(feet: V3, height: f32, eye: f32) -> Self {
        Body {
            feet,
            height,
            eye,
            radius: 0.2,
            head: 0.14,
            alive: true,
        }
    }

    /// The head sphere's centre.
    pub fn head_centre(&self) -> V3 {
        [self.feet[0], self.feet[1] + self.eye + 0.02, self.feet[2]]
    }

    /// Top of the body cylinder (the neck).
    pub fn neck(&self) -> f32 {
        self.head_centre()[1] - self.head * 0.7
    }

    /// The middle of the chest (what bots and blasts aim at).
    pub fn chest(&self) -> V3 {
        [
            self.feet[0],
            self.feet[1] + self.height * 0.62,
            self.feet[2],
        ]
    }

    /// Which zone a point on the cylinder is in.
    pub fn zone_at(&self, y: f32) -> Zone {
        let k = (y - self.feet[1]) / fmax(self.height, 1e-3);
        if k < 0.42 {
            Zone::Legs
        } else if k < 0.58 {
            Zone::Stomach
        } else {
            Zone::Chest
        }
    }
}

/// What a trace hit first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// The map: hit point, the face normal turned to the shooter, the tag.
    World {
        t: f32,
        point: V3,
        normal: V3,
        tag: u32,
    },
    /// A body (its index in the slice) in a zone.
    Body {
        index: usize,
        t: f32,
        point: V3,
        zone: Zone,
    },
}

impl Hit {
    /// Distance from the origin.
    pub fn t(&self) -> f32 {
        match *self {
            Hit::World { t, .. } | Hit::Body { t, .. } => t,
        }
    }

    pub fn point(&self) -> V3 {
        match *self {
            Hit::World { point, .. } | Hit::Body { point, .. } => point,
        }
    }
}

fn at(o: V3, d: V3, t: f32) -> V3 {
    [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t]
}

/// First `t >= 0` where the ray meets a sphere.
pub fn ray_sphere(o: V3, d: V3, c: V3, r: f32) -> Option<f32> {
    let m = [o[0] - c[0], o[1] - c[1], o[2] - c[2]];
    let b = m[0] * d[0] + m[1] * d[1] + m[2] * d[2];
    let cc = m[0] * m[0] + m[1] * m[1] + m[2] * m[2] - r * r;
    if cc > 0.0 && b > 0.0 {
        return None;
    }
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    Some(fmax(-b - disc.sqrt(), 0.0))
}

/// First `t >= 0` where the ray meets an upright cylinder (side or caps).
pub fn ray_cylinder(o: V3, d: V3, axis_xz: [f32; 2], r: f32, y0: f32, y1: f32) -> Option<f32> {
    let (ox, oz) = (o[0] - axis_xz[0], o[2] - axis_xz[1]);
    let inside_xz = |t: f32| {
        let (x, z) = (ox + d[0] * t, oz + d[2] * t);
        x * x + z * z <= r * r
    };
    let mut best: Option<f32> = None;
    let mut keep = |t: f32| {
        if t >= 0.0 && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    };
    let a = d[0] * d[0] + d[2] * d[2];
    if a > 1e-12 {
        let b = 2.0 * (ox * d[0] + oz * d[2]);
        let c = ox * ox + oz * oz - r * r;
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let t = (-b - disc.sqrt()) / (2.0 * a);
            let y = o[1] + d[1] * t;
            if y >= y0 && y <= y1 {
                keep(t);
            }
        }
    }
    if d[1].abs() > 1e-9 {
        for cap in [y0, y1] {
            let t = (cap - o[1]) / d[1];
            if inside_xz(t) {
                keep(t);
            }
        }
    }
    best
}

/// Where a ray first meets a body, and in which zone.
pub fn ray_body(o: V3, d: V3, b: &Body) -> Option<(f32, Zone)> {
    let head = ray_sphere(o, d, b.head_centre(), b.head).map(|t| (t, Zone::Head));
    let body = ray_cylinder(o, d, [b.feet[0], b.feet[2]], b.radius, b.feet[1], b.neck())
        .map(|t| (t, b.zone_at(o[1] + d[1] * t)));
    match (head, body) {
        (Some(h), Some(c)) => Some(if h.0 <= c.0 { h } else { c }),
        (h, c) => h.or(c),
    }
}

/// Trace a bullet from `origin` along unit `dir` up to `range`: the map
/// first, then every living body except `skip` that is nearer than it.
/// Bodies are tested in slice order and a later one wins only when strictly
/// nearer, so the same scene always gives the same hit.
pub fn trace(
    world: &StaticWorld,
    origin: V3,
    dir: V3,
    range: f32,
    bodies: &[Body],
    skip: Option<usize>,
) -> Option<Hit> {
    let mut best = world.raycast(origin, dir, range).map(|h| Hit::World {
        t: h.t,
        point: h.point,
        normal: h.normal,
        tag: h.tag,
    });
    for (i, b) in bodies.iter().enumerate() {
        if !b.alive || Some(i) == skip {
            continue;
        }
        let Some((t, zone)) = ray_body(origin, dir, b) else {
            continue;
        };
        if t <= range && best.is_none_or(|h| t < h.t()) {
            best = Some(Hit::Body {
                index: i,
                t,
                point: at(origin, dir, t),
                zone,
            });
        }
    }
    best
}

/// The nearest point of a body's upright axis to `p` (between its feet and
/// the top of its head).
pub fn nearest_on_axis(b: &Body, p: V3) -> V3 {
    let top = b.feet[1] + b.height;
    [b.feet[0], fmin(fmax(p[1], b.feet[1]), top), b.feet[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_physics::collide::WorldBuilder;

    fn wall_world() -> StaticWorld {
        let mut b = WorldBuilder::new();
        // A floor and a wall across +z at z = 5..5.3, tag 7.
        b.aabb([-20.0, -0.2, -20.0], [20.0, 0.0, 20.0], 1);
        b.aabb([-5.0, 0.0, 5.0], [5.0, 3.0, 5.3], 7);
        b.build()
    }

    #[test]
    fn hitscan_hits_a_wall_and_stops() {
        let w = wall_world();
        let shooter = [0.0, 1.2, 0.0];
        // A body behind the wall, straight down the line of fire.
        let bodies = [Body::new([0.0, 0.0, 8.0], 1.372, 1.219)];
        let hit = trace(&w, shooter, [0.0, 0.0, 1.0], 100.0, &bodies, None).unwrap();
        match hit {
            Hit::World {
                t,
                point,
                normal,
                tag,
            } => {
                assert_eq!(tag, 7);
                assert!((t - 5.0).abs() < 1e-3 && (point[2] - 5.0).abs() < 1e-3);
                assert!(normal[2] < -0.99, "normal faces the shooter: {normal:?}");
            }
            other => panic!("the bullet went through the wall: {other:?}"),
        }
        // With no wall in the way it reaches the body.
        let open = [Body::new([0.0, 0.0, 3.0], 1.372, 1.219)];
        assert!(matches!(
            trace(&w, shooter, [0.0, 0.0, 1.0], 100.0, &open, None),
            Some(Hit::Body { index: 0, .. })
        ));
    }

    #[test]
    fn head_and_body_zones_and_skip() {
        let w = WorldBuilder::new().build();
        let b = Body::new([0.0, 0.0, 4.0], 1.372, 1.219);
        let head = trace(
            &w,
            [0.0, b.head_centre()[1], 0.0],
            [0.0, 0.0, 1.0],
            50.0,
            &[b],
            None,
        );
        assert!(
            matches!(
                head,
                Some(Hit::Body {
                    zone: Zone::Head,
                    ..
                })
            ),
            "{head:?}"
        );
        let chest = trace(
            &w,
            [0.0, b.chest()[1], 0.0],
            [0.0, 0.0, 1.0],
            50.0,
            &[b],
            None,
        );
        assert!(matches!(
            chest,
            Some(Hit::Body {
                zone: Zone::Chest,
                ..
            })
        ));
        let legs = trace(&w, [0.0, 0.3, 0.0], [0.0, 0.0, 1.0], 50.0, &[b], None);
        assert!(matches!(
            legs,
            Some(Hit::Body {
                zone: Zone::Legs,
                ..
            })
        ));
        assert!(trace(&w, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 50.0, &[b], Some(0)).is_none());
        assert!(trace(&w, [0.5, 1.0, 0.0], [0.0, 0.0, 1.0], 50.0, &[b], None).is_none());
        let dead = Body { alive: false, ..b };
        assert!(trace(&w, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 50.0, &[dead], None).is_none());
    }

    #[test]
    fn nearest_body_wins_and_range_limits() {
        let w = WorldBuilder::new().build();
        let near = Body::new([0.0, 0.0, 4.0], 1.8, 1.6);
        let far = Body::new([0.0, 0.0, 9.0], 1.8, 1.6);
        // Either order in the slice: the nearer body takes the bullet.
        let a = trace(
            &w,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            50.0,
            &[far, near],
            None,
        );
        assert!(matches!(a, Some(Hit::Body { index: 1, .. })), "{a:?}");
        let b = trace(
            &w,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            50.0,
            &[near, far],
            None,
        );
        assert!(matches!(b, Some(Hit::Body { index: 0, .. })), "{b:?}");
        // Out of range: no hit.
        assert!(trace(&w, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 3.0, &[near], None).is_none());
    }
}
