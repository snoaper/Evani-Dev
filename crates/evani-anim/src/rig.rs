//! Skeletons, skins and the per-character pose buffers: bind (rest) pose,
//! local to model space, skinning matrices and attachment joints.
//!
//! Everything here is render-side: a [`Rig`] is filled from the simulation's
//! clip ticks each drawn frame and never written back into the simulation.

use crate::math::{MID, Mat43, QID, Quat, Vec3, Xform, conj, from_cols, mmul, mpoint, mpos, qmul, qnorm};
use crate::Error;

/// "No parent".
pub const NONE: u16 = u16::MAX;

/// A node hierarchy and its rest pose (the tier file's node transforms).
#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    parents: Vec<u16>,
    rest: Vec<Xform>,
    /// Node indices, parents before children.
    order: Vec<u16>,
}

impl Skeleton {
    /// `parents[n]` is node n's parent or [`NONE`]; `rest[n]` its rest
    /// transform. A parent out of range or a cycle is an error.
    pub fn new(parents: &[u16], rest: &[Xform]) -> Result<Self, Error> {
        let n = parents.len();
        if n != rest.len() || n >= usize::from(NONE) {
            return Err(err!("skeleton size", "skeleton: parents and rest differ in length, or 65535 nodes or more"));
        }
        let mut depth = vec![0u16; n];
        for (i, d) in depth.iter_mut().enumerate() {
            let mut p = parents[i];
            while p != NONE {
                if usize::from(p) >= n || usize::from(*d) >= n {
                    return Err(err!("skeleton parent", "skeleton: a parent out of range, or a cycle"));
                }
                *d += 1;
                p = parents[usize::from(p)];
            }
        }
        // By depth, then index (what a stable sort by depth gives), one pass
        // per depth: a few hundred nodes at most, and no sort code in the wasm
        // (std's stable sort is about 4 KB of it).
        let mut order = Vec::with_capacity(n);
        let deepest = depth.iter().copied().max().unwrap_or(0);
        for level in 0..=deepest {
            order.extend((0..n as u16).filter(|&i| depth[usize::from(i)] == level));
        }
        Ok(Skeleton { parents: parents.to_vec(), rest: rest.to_vec(), order })
    }

    pub fn len(&self) -> usize {
        self.parents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parents.is_empty()
    }

    /// Node n's parent, or [`NONE`].
    pub fn parent(&self, n: u16) -> u16 {
        self.parents.get(usize::from(n)).copied().unwrap_or(NONE)
    }

    /// The rest pose, one transform per node.
    pub fn rest(&self) -> &[Xform] {
        &self.rest
    }

    /// Whether `n` is `root` or below it.
    pub fn is_below(&self, n: u16, root: u16) -> bool {
        let mut p = n;
        for _ in 0..=self.len() {
            if p == root {
                return true;
            }
            if p == NONE {
                return false;
            }
            p = self.parent(p);
        }
        false
    }
}

/// A skin: its joints (node indices) and their inverse bind matrices.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    joints: Vec<u16>,
    ibm: Vec<Mat43>,
}

impl Skin {
    /// `inverse_bind` is glTF's column-major 4x4 per joint, or empty for
    /// identities (glTF's default).
    pub fn new(skeleton: &Skeleton, joints: &[u16], inverse_bind: &[[f32; 16]]) -> Result<Self, Error> {
        if joints.iter().any(|&j| usize::from(j) >= skeleton.len()) || (!inverse_bind.is_empty() && inverse_bind.len() != joints.len()) {
            return Err(err!("skin", "skin: a joint out of range, or not one inverse bind matrix per joint"));
        }
        let ibm = if inverse_bind.is_empty() { vec![MID; joints.len()] } else { inverse_bind.iter().map(from_cols).collect() };
        Ok(Skin { joints: joints.to_vec(), ibm })
    }

    pub fn joints(&self) -> &[u16] {
        &self.joints
    }
}

/// One character's pose buffers, sized once: local transforms, model-space
/// matrices and rotations, and the skinning matrices S3-9 uploads.
#[derive(Clone, Debug, PartialEq)]
pub struct Rig {
    local: Vec<Xform>,
    model: Vec<Mat43>,
    mrot: Vec<Quat>,
    skin: Vec<Mat43>,
}

impl Rig {
    /// Buffers for `skeleton` at its rest pose and `joints` skin matrices.
    pub fn new(skeleton: &Skeleton, joints: usize) -> Self {
        let n = skeleton.len();
        let mut r = Rig { local: skeleton.rest.clone(), model: vec![MID; n], mrot: vec![QID; n], skin: vec![MID; joints] };
        r.update(skeleton);
        r
    }

    /// The local pose (one transform per node), to fill before [`Rig::update`].
    pub fn local_mut(&mut self) -> &mut [Xform] {
        &mut self.local
    }

    pub fn local(&self) -> &[Xform] {
        &self.local
    }

    /// Copy a pose in (a [`crate::Graph`] result); extra or missing nodes are left as they are.
    pub fn set_pose(&mut self, pose: &[Xform]) {
        let n = self.local.len().min(pose.len());
        self.local[..n].copy_from_slice(&pose[..n]);
    }

    /// Local to model space for every node, parents first.
    pub fn update(&mut self, skeleton: &Skeleton) {
        for &n in &skeleton.order {
            self.one(skeleton, n);
        }
    }

    fn one(&mut self, skeleton: &Skeleton, n: u16) {
        let i = usize::from(n);
        let Some(l) = self.local.get(i) else { return };
        let m = l.matrix();
        let r = l.r;
        let p = usize::from(skeleton.parent(n));
        let (pm, pr) = match (self.model.get(p), self.mrot.get(p)) {
            (Some(pm), Some(pr)) => (mmul(pm, &m), qmul(*pr, r)),
            _ => (m, r),
        };
        if let (Some(a), Some(b)) = (self.model.get_mut(i), self.mrot.get_mut(i)) {
            (*a, *b) = (pm, pr);
        }
    }

    /// Model space again for `root` and every node below it (after IK).
    pub(crate) fn update_below(&mut self, skeleton: &Skeleton, root: u16) {
        // A walk up per node instead of a marks buffer: chains are short,
        // and there is one buffer less per character and in the wasm.
        for &n in &skeleton.order {
            if skeleton.is_below(n, root) {
                self.one(skeleton, n);
            }
        }
    }

    /// Model-space matrices, one per node.
    pub fn model(&self) -> &[Mat43] {
        &self.model
    }

    /// A node's model-space position.
    pub fn position(&self, n: u16) -> Vec3 {
        self.model.get(usize::from(n)).map_or([0.0; 3], mpos)
    }

    /// A node's model-space rotation (the product of the local rotations
    /// down the chain; scales are left out).
    pub fn rotation(&self, n: u16) -> Quat {
        self.mrot.get(usize::from(n)).copied().unwrap_or(QID)
    }

    /// Give node `n` the model-space rotation `q`, as a new local rotation
    /// under its parent's (current) model rotation. Model space below it is
    /// not updated here.
    pub(crate) fn set_model_rotation(&mut self, n: u16, q: Quat, parent_rot: Quat) {
        if let Some(l) = self.local.get_mut(usize::from(n)) {
            l.r = qnorm(qmul(conj(parent_rot), q));
        }
    }

    /// The skinning matrices: model(joint) x inverse bind, one 3x4 matrix
    /// (48 bytes) per joint of `skin`, in joint order. Call after
    /// [`Rig::update`] (and IK). This is the flat slice S3-9 uploads.
    pub fn skinning(&mut self, skin: &Skin) -> &[Mat43] {
        for ((out, &j), ibm) in self.skin.iter_mut().zip(&skin.joints).zip(&skin.ibm) {
            *out = match self.model.get(usize::from(j)) {
                Some(m) => mmul(m, ibm),
                None => MID,
            };
        }
        &self.skin
    }

    /// An attachment: the model-space matrix of a thing held at `offset`
    /// from node `n` (a sword in a hand, a hat on a head).
    pub fn attach(&self, n: u16, offset: &Xform) -> Mat43 {
        match self.model.get(usize::from(n)) {
            Some(m) => mmul(m, &offset.matrix()),
            None => offset.matrix(),
        }
    }

    /// Where a point at `offset` in node `n`'s space is, in model space.
    pub fn attach_point(&self, n: u16, offset: Vec3) -> Vec3 {
        match self.model.get(usize::from(n)) {
            Some(m) => mpoint(m, offset),
            None => offset,
        }
    }
}
