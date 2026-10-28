//! Pose helpers: closed-form joint rotations for common holds.
//!
//! [`curl_finger`] wraps a finger joint chain around a cylinder (a handle,
//! a grip, a bar): every bone hinges about the cylinder's axis, one after
//! another from the knuckle, until the next joint touches the cylinder's
//! surface pushed out by the finger's half thickness. Each hinge angle is
//! solved exactly (no iteration), with Evani.Model's own `dmath`, so the
//! same chain and cylinder give the same bits on every platform.
//!
//! Joints carry positions only (the skeleton's rest frames are the world
//! axes, as in the `.glb`), so the returned local rotations are in the rest
//! axes of the parent joint: `world[i] = world[i - 1] * local[i]`.

use crate::dmath;
use crate::vec::{QID, Quat, V3, add, cross, dot, len, mul, qconj, qmul, qrot, quat_axis, sub};

/// The cylinder a finger closes round, and the finger's limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    /// Any point on the cylinder's axis.
    pub axis_point: V3,
    /// The axis direction (need not be unit length).
    pub axis_dir: V3,
    /// Cylinder radius (m).
    pub radius: f64,
    /// The finger's half thickness (m): joint centres stop this far off
    /// the surface.
    pub pad: f64,
    /// Largest bend at one joint (radians).
    pub max_bend: f64,
}

impl Grip {
    /// A grip round `axis_dir` through `axis_point` of `radius`, with a
    /// 9 mm finger half thickness and a 100 degree joint limit.
    pub fn new(axis_point: V3, axis_dir: V3, radius: f64) -> Self {
        Grip { axis_point, axis_dir, radius, pad: 0.009, max_bend: dmath::deg(100.0) }
    }
}

/// A curled finger.
#[derive(Clone, Debug, PartialEq)]
pub struct Curl {
    /// Per bone (one fewer than joints): rotation in the parent's rest axes.
    pub local: Vec<Quat>,
    /// Per bone: rotation from rest in world axes.
    pub world: Vec<Quat>,
    /// Per bone: the hinge angle (radians, signed about the unit axis).
    pub angles: Vec<f64>,
    /// Every joint after the curl (the first, the knuckle, never moves).
    pub posed: Vec<V3>,
    /// Per bone: true when its far joint touches the cylinder, false when
    /// the joint limit or the bone's reach stopped it first.
    pub contact: Vec<bool>,
}

fn finite(v: V3) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// `x` wrapped into `[0, 2 pi)`.
fn wrap(x: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let r = x % tau;
    if r < 0.0 { r + tau } else { r }
}

/// Curl the joint chain `chain` (knuckle first, fingertip last; at least
/// two joints) round `grip`. Each bone turns the way that brings its far
/// joint towards the axis, by the smallest angle that reaches the padded
/// surface, capped at `grip.max_bend`. A finger that already touches or is
/// inside the surface at a joint does not open: that bone stays at 0.
pub fn curl_finger(chain: &[V3], grip: &Grip) -> Result<Curl, String> {
    if chain.len() < 2 {
        return Err("a finger needs at least two joints".into());
    }
    if !chain.iter().all(|p| finite(*p)) || !finite(grip.axis_point) || !finite(grip.axis_dir) {
        return Err("joint or axis positions are not finite".into());
    }
    let dl = len(grip.axis_dir);
    if dl <= 1e-12 {
        return Err("the grip axis has no direction".into());
    }
    let ok = |x: f64| x.is_finite() && x >= 0.0;
    if !(ok(grip.radius) && ok(grip.pad) && ok(grip.max_bend)) {
        return Err("radius, pad and max_bend must be finite and not negative".into());
    }
    let a = mul(grip.axis_dir, 1.0 / dl);
    let rc = grip.radius + grip.pad;
    // Distance-from-axis vector of a point (its part across the axis).
    let across = |p: V3| {
        let d = sub(p, grip.axis_point);
        sub(d, mul(a, dot(d, a)))
    };
    if len(across(chain[0])) < rc {
        return Err("the knuckle is inside the grip: move the cylinder or shrink its radius".into());
    }
    let mut posed = chain.to_vec();
    let mut world_acc = QID;
    let (mut local, mut world, mut angles, mut contact) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..chain.len() - 1 {
        let p = across(posed[i]);
        let d = across(posed[i + 1]);
        let d = sub(d, p);
        let l2 = dot(d, d);
        let mut theta = 0.0;
        let mut touch = false;
        if l2 > 1e-18 {
            // |p + R(theta) d|^2 = rc^2 with R a turn about `a`:
            // A cos + B sin = K, A = p.d, B = p.(a x d).
            let ca = dot(p, d);
            let cb = dot(p, cross(a, d));
            let k = 0.5 * (rc * rc - dot(p, p) - l2);
            let m = (ca * ca + cb * cb).sqrt();
            // Turning positive changes |.|^2 at a rate 2B: turn the way it falls.
            let s = if cb > 0.0 { -1.0 } else { 1.0 };
            let at_rest = dot(add(p, d), add(p, d));
            if at_rest > rc * rc && m > 1e-18 {
                let phi = dmath::atan2(cb, ca);
                let c = k / m;
                let (t, reached) = if c.abs() <= 1.0 {
                    // theta = phi +- acos(c); acos(c) = atan2(sqrt(1 - c^2), c).
                    let al = dmath::atan2((1.0 - c * c).max(0.0).sqrt(), c);
                    (wrap(s * (phi + al)).min(wrap(s * (phi - al))), true)
                } else {
                    // Out of reach: the closest approach, at A cos + B sin = -M.
                    (wrap(s * (phi + std::f64::consts::PI)), false)
                };
                if t <= grip.max_bend {
                    theta = s * t;
                    touch = reached;
                } else {
                    theta = s * grip.max_bend;
                }
            }
        }
        let qi = quat_axis(a, theta);
        // Turn every joint past this one about this joint.
        let pivot = posed[i];
        for q in posed.iter_mut().skip(i + 1) {
            *q = add(pivot, qrot(qi, sub(*q, pivot)));
        }
        let w = qmul(qi, world_acc);
        local.push(qmul(qconj(world_acc), w));
        world.push(w);
        world_acc = w;
        angles.push(theta);
        contact.push(touch);
    }
    Ok(Curl { local, world, angles, posed, contact })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight index finger along +x lying on top of a 20 mm bar along z.
    fn finger() -> Vec<V3> {
        vec![[0.0, 0.032, 0.0], [0.045, 0.032, 0.004], [0.072, 0.032, 0.006], [0.094, 0.032, 0.007]]
    }

    fn bits(c: &Curl) -> Vec<u64> {
        let mut v: Vec<u64> = c.posed.iter().flat_map(|p| p.map(f64::to_bits)).collect();
        v.extend(c.local.iter().chain(&c.world).flat_map(|q| q.map(f64::to_bits)));
        v.extend(c.angles.iter().map(|a| a.to_bits()));
        v
    }

    #[test]
    fn a_finger_wraps_a_bar_touching_at_every_joint() {
        let chain = finger();
        let g = Grip::new([0.0; 3], [0.0, 0.0, 1.0], 0.02);
        let c = curl_finger(&chain, &g).unwrap();
        let rc = g.radius + g.pad;
        assert_eq!(c.angles.len(), 3);
        assert_eq!(c.posed[0], chain[0]);
        for i in 1..chain.len() {
            let p = c.posed[i];
            assert!(c.contact[i - 1], "joint {i} did not reach: {:?}", c.angles);
            assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - rc).abs() < 1e-9, "joint {i} at {p:?}");
            // Bone lengths and the along-axis spread are kept.
            assert!((len(sub(p, c.posed[i - 1])) - len(sub(chain[i], chain[i - 1]))).abs() < 1e-12);
            assert!((p[2] - chain[i][2]).abs() < 1e-12);
        }
        // All one way (closing over the bar: clockwise about +z seen from
        // +z, i.e. negative), within the joint limit.
        for t in &c.angles {
            assert!(*t < 0.0 && t.abs() <= g.max_bend + 1e-12, "{:?}", c.angles);
        }
        // Deterministic.
        assert_eq!(bits(&c), bits(&curl_finger(&chain, &g).unwrap()));
    }

    #[test]
    fn local_rotations_rebuild_the_pose() {
        let chain = finger();
        let c = curl_finger(&chain, &Grip::new([0.01, 0.0, 0.0], [0.0, 0.2, 1.0], 0.018)).unwrap();
        // Forward kinematics from the rest offsets with the local rotations.
        let mut w = QID;
        let mut at = chain[0];
        for i in 0..c.local.len() {
            w = qmul(w, c.local[i]);
            assert!(w.iter().zip(&c.world[i]).all(|(x, y)| (x - y).abs() < 1e-12));
            at = add(at, qrot(w, sub(chain[i + 1], chain[i])));
            assert!(len(sub(at, c.posed[i + 1])) < 1e-12, "joint {}: {at:?} vs {:?}", i + 1, c.posed[i + 1]);
        }
    }

    #[test]
    fn the_joint_limit_and_reach_stop_short_of_a_far_bar() {
        // A thin bar well below the finger: the bones cannot reach it.
        let chain = finger();
        let mut g = Grip::new([0.03, -0.2, 0.0], [0.0, 0.0, 1.0], 0.005);
        g.max_bend = dmath::deg(40.0);
        let c = curl_finger(&chain, &g).unwrap();
        assert!(c.contact.iter().all(|t| !t), "{:?}", c.contact);
        assert!(c.angles.iter().all(|t| t.abs() <= g.max_bend + 1e-12), "{:?}", c.angles);
        assert!(c.angles.iter().any(|t| (t.abs() - g.max_bend).abs() < 1e-12), "{:?}", c.angles);
        // A max bend of 0 keeps the rest pose exactly.
        g.max_bend = 0.0;
        let c = curl_finger(&chain, &g).unwrap();
        assert_eq!(c.posed, chain);
    }

    #[test]
    fn a_mirrored_hand_curls_the_mirrored_way() {
        let chain = finger();
        let g = Grip::new([0.0; 3], [0.0, 0.0, 1.0], 0.02);
        let m: Vec<V3> = chain.iter().map(|p| [-p[0], p[1], p[2]]).collect();
        let a = curl_finger(&chain, &g).unwrap();
        let b = curl_finger(&m, &g).unwrap();
        for (x, y) in a.angles.iter().zip(&b.angles) {
            assert!((x + y).abs() < 1e-12, "{:?} {:?}", a.angles, b.angles);
        }
        for (p, q) in a.posed.iter().zip(&b.posed) {
            assert!((p[0] + q[0]).abs() < 1e-12 && (p[1] - q[1]).abs() < 1e-12 && (p[2] - q[2]).abs() < 1e-12);
        }
    }

    #[test]
    fn bad_input_is_an_error() {
        let g = Grip::new([0.0; 3], [0.0, 0.0, 1.0], 0.02);
        assert!(curl_finger(&finger()[..1], &g).is_err());
        assert!(curl_finger(&finger(), &Grip::new([0.0; 3], [0.0; 3], 0.02)).is_err());
        assert!(curl_finger(&finger(), &Grip::new([0.0; 3], [0.0, 0.0, 1.0], 0.05)).is_err(), "knuckle inside");
        let mut bad = finger();
        bad[2][1] = f64::NAN;
        assert!(curl_finger(&bad, &g).is_err());
        let mut neg = g;
        neg.radius = -1.0;
        assert!(curl_finger(&finger(), &neg).is_err());
    }
}
