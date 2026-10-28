//! Two-bone IK (pole vector, soften) and aim IK, render-only. Both work on
//! a [`Rig`] whose model space is up to date ([`Rig::update`]), write new
//! local rotations for the joints they turn, and update model space below
//! them. Only `+ - * /` and `sqrt` (no trigonometry), so they give the same
//! bits on every machine. The chain's parents are assumed to carry no
//! non-uniform or negative scale (rotations are taken from the local
//! rotations down the chain).

use crate::math::{QID, Quat, Vec3, add, cross, dot, from_to, length, nlerp, perpendicular, qmul, rotate, scale, sub, unit};
use crate::rig::{NONE, Rig, Skeleton};
use crate::Error;

/// A two-bone chain (upper arm, forearm, hand; thigh, shin, foot) to reach
/// a model-space target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoBone {
    pub start: u16,
    pub mid: u16,
    pub end: u16,
    pub target: Vec3,
    /// A model-space point the middle joint bends towards (a knee's front).
    pub pole: Vec3,
    /// Reach at which the chain starts to slow down as it straightens, as a
    /// fraction of its full length: 1 never softens; 0.97 is a gentle
    /// default. The chain then nears full stretch smoothly instead of
    /// snapping straight.
    pub soften: f32,
    /// 0 leaves the pose, 1 applies the solve fully.
    pub weight: f32,
}

/// One joint turned to point an axis at a target (a head, a gun arm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub joint: u16,
    /// The joint's local axis that should point at the target.
    pub forward: Vec3,
    /// The joint's local up axis, turned towards `pole` around the aim
    /// direction; zero leaves the twist to the shortest turn.
    pub up: Vec3,
    pub target: Vec3,
    /// A model-space up direction for `up`.
    pub pole: Vec3,
    pub weight: f32,
}

const CHAIN: Error = err!("ik chain", "ik: the joints are not a chain (start above mid above end) of this skeleton");

/// The model-space rotation of `n`'s parent.
fn parent_rot(rig: &Rig, skeleton: &Skeleton, n: u16) -> Quat {
    let p = skeleton.parent(n);
    if p == NONE { QID } else { rig.rotation(p) }
}

impl Rig {
    /// Solve a two-bone chain. Returns whether the end reaches the target
    /// (false when it is out of reach, or softened short of it: the chain
    /// then points straight at it). Bones keep their lengths; a target at
    /// the start joint, a zero-length bone or a pole on the chain's line
    /// give a finite pose, never NaN.
    pub fn two_bone(&mut self, skeleton: &Skeleton, ik: &TwoBone) -> Result<bool, Error> {
        let n = skeleton.len() as u16;
        if ik.start >= n || ik.mid >= n || ik.end >= n || ik.mid == ik.start || ik.end == ik.mid || !skeleton.is_below(ik.mid, ik.start) || !skeleton.is_below(ik.end, ik.mid) {
            return Err(CHAIN);
        }
        let (a, b, c) = (self.position(ik.start), self.position(ik.mid), self.position(ik.end));
        let (ab, bc) = (sub(b, a), sub(c, b));
        let (la, lb) = (length(ab), length(bc));
        let reach = la + lb;
        // A weight that is not a number is 0: the pose is left alone.
        let w = if ik.weight.is_nan() { 0.0 } else { ik.weight.clamp(0.0, 1.0) };
        if !(la > 1e-6 && lb > 1e-6 && reach.is_finite()) || w == 0.0 || !ik.target.iter().all(|v| v.is_finite()) {
            return Ok(false);
        }
        let at = sub(ik.target, a);
        let want = length(at);
        // Scaled first, so a target too far for f32 squares still has a direction.
        let big = at.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let dir = (if big > 0.0 { unit(scale(at, 1.0 / big)) } else { None }).or_else(|| unit(sub(c, a))).unwrap_or_else(|| scale(ab, 1.0 / la));
        let mut d = if want.is_finite() { want } else { reach };
        let soft = ik.soften.clamp(0.0, 1.0) * reach;
        if ik.soften < 1.0 && d > soft {
            let (x, r) = (d - soft, reach - soft);
            d = soft + r * x / (x + r);
        }
        // max then min, not `clamp`: with bounds that are not constants,
        // `f32::clamp` keeps its min <= max assert, whose message pulls the
        // whole float formatter (about 10 KB gzip) into the wasm.
        // (The low bound is never above `reach`.)
        d = d.max((la - lb).abs().max(reach * 1e-4)).min(reach);
        let reached = want.is_finite() && (want - d).abs() <= reach * 1e-4;
        // The bend plane: the pole's offset from the line, else the current bend.
        let side = |v: Vec3| sub(v, scale(dir, dot(v, dir)));
        let u = unit(side(sub(ik.pole, a))).or_else(|| unit(side(ab))).unwrap_or_else(|| perpendicular(dir));
        // Opposite directions turn about the bend plane's normal.
        let normal = unit(cross(dir, u)).unwrap_or_else(|| perpendicular(dir));
        let x = (la * la - lb * lb + d * d) / (2.0 * d);
        let h = (la * la - x * x).max(0.0).sqrt();
        let b_new = add(a, add(scale(dir, x), scale(u, h)));
        let c_new = add(a, scale(dir, d));
        let q1 = from_to(ab, sub(b_new, a), normal);
        let c1 = add(a, rotate(q1, sub(c, a)));
        let q2 = from_to(sub(c1, b_new), sub(c_new, b_new), normal);
        let (q1, q2) = if w < 1.0 { (nlerp(QID, q1, w), nlerp(QID, q2, w)) } else { (q1, q2) };
        // New model rotations: the start turns by q1; the middle by q1, then q2.
        let start_new = qmul(q1, self.rotation(ik.start));
        let mid_parent_new = if skeleton.parent(ik.mid) == ik.start { start_new } else { qmul(q1, parent_rot(self, skeleton, ik.mid)) };
        let mid_new = qmul(q2, qmul(q1, self.rotation(ik.mid)));
        let start_parent = parent_rot(self, skeleton, ik.start);
        self.set_model_rotation(ik.start, start_new, start_parent);
        self.set_model_rotation(ik.mid, mid_new, mid_parent_new);
        self.update_below(skeleton, ik.start);
        Ok(reached)
    }

    /// Turn one joint so its `forward` axis points at the target (and its
    /// `up` axis leans towards `pole`).
    pub fn aim(&mut self, skeleton: &Skeleton, aim: &Aim) -> Result<(), Error> {
        if usize::from(aim.joint) >= skeleton.len() {
            return Err(CHAIN);
        }
        let w = if aim.weight.is_nan() { 0.0 } else { aim.weight.clamp(0.0, 1.0) };
        let p = self.position(aim.joint);
        let now = self.rotation(aim.joint);
        let (Some(dir), true) = (unit(sub(aim.target, p)), w > 0.0) else { return Ok(()) };
        let mut q = from_to(rotate(now, aim.forward), dir, perpendicular(dir));
        if let (Some(_), Some(_)) = (unit(aim.up), unit(aim.pole)) {
            let side = |v: Vec3| sub(v, scale(dir, dot(v, dir)));
            let up_now = side(rotate(q, rotate(now, aim.up)));
            let want = side(aim.pole);
            if length(up_now) > 1e-6 && length(want) > 1e-6 {
                q = qmul(from_to(up_now, want, dir), q);
            }
        }
        if w < 1.0 {
            q = nlerp(QID, q, w);
        }
        let parent = parent_rot(self, skeleton, aim.joint);
        self.set_model_rotation(aim.joint, qmul(q, now), parent);
        self.update_below(skeleton, aim.joint);
        Ok(())
    }
}
