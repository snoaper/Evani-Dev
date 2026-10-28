//! The blend graph: three node types only (a clip, a blend of two, a masked
//! additive layer), evaluated into preallocated pose slots. Render-side:
//! the weights and frame positions come from the simulation's integer
//! state ([`crate::Player`], [`crate::Blend1d`]) and nothing flows back.

use crate::clip::{Clips, Cursor};
use crate::math::{QID, Xform, conj, lerp3, nlerp, qmul, qnorm};
use crate::rig::Skeleton;
use crate::Error;

/// Which mask an [`Node::Add`] uses: this value means every joint.
pub const ALL: u8 = u8::MAX;

/// One node. A node's inputs are earlier nodes, by index; the last node is
/// the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Node {
    /// `clip` at frame position `frame` (30 Hz frames: [`crate::frame_at`]),
    /// over the rest pose.
    Clip { clip: u16, frame: f32 },
    /// `a` to `b` by `w` per node: translation and scale lerp, rotation
    /// nlerps. `w <= 0` is `a` exactly, `w >= 1` is `b` exactly.
    Blend { a: u8, b: u8, w: f32 },
    /// `base` plus `w` times the difference from `reference` to `layer`
    /// (a clip and the pose it was made against, often its own frame 0),
    /// weighed per node by `mask` (an index into the masks, or [`ALL`]).
    /// Nodes the mask weighs 0 keep `base` exactly.
    Add { base: u8, layer: u8, reference: u8, mask: u8, w: f32 },
}

/// A weight per node, 0 to 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask(pub Vec<f32>);

impl Mask {
    /// `weight` on every node at or below one of `roots`, 0 elsewhere.
    pub fn below(skeleton: &Skeleton, roots: &[u16], weight: f32) -> Mask {
        Mask((0..skeleton.len() as u16).map(|n| if roots.iter().any(|&r| skeleton.is_below(n, r)) { weight } else { 0.0 }).collect())
    }
}

/// Pose slots and cursors for evaluating node lists of up to `slots` nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    nodes: usize,
    poses: Vec<Xform>,
    cursors: Vec<Cursor>,
}

const BAD: Error = err!("graph", "graph: too many nodes, an input that is not an earlier node, or a clip that isn't there");

impl Graph {
    /// Room for `slots` nodes over `skeleton`, sampling `clips`.
    pub fn new(skeleton: &Skeleton, clips: &Clips, slots: usize) -> Self {
        let n = skeleton.len();
        let mut poses = Vec::with_capacity(n * slots);
        for _ in 0..slots {
            poses.extend_from_slice(skeleton.rest());
        }
        Graph { nodes: n, poses, cursors: (0..slots).map(|_| clips.cursor()).collect() }
    }

    /// Evaluate `nodes` and return the last one's pose (one transform per
    /// node of the skeleton). No allocation.
    pub fn eval(&mut self, skeleton: &Skeleton, clips: &Clips, nodes: &[Node], masks: &[Mask]) -> Result<&[Xform], Error> {
        let n = self.nodes;
        if nodes.is_empty() || nodes.len() > self.cursors.len() || skeleton.len() != n {
            return Err(BAD);
        }
        for (i, node) in nodes.iter().enumerate() {
            let (done, rest) = self.poses.split_at_mut(i * n);
            let out = rest.get_mut(..n).ok_or(BAD)?;
            let slot = |k: u8| -> Result<&[Xform], Error> {
                let k = usize::from(k);
                if k < i { done.get(k * n..(k + 1) * n).ok_or(BAD) } else { Err(BAD) }
            };
            match *node {
                Node::Clip { clip, frame } => {
                    let clip = usize::from(clip);
                    if clip >= clips.len() {
                        return Err(BAD);
                    }
                    out.copy_from_slice(skeleton.rest());
                    let cursor = self.cursors.get_mut(i).ok_or(BAD)?;
                    clips.sample(clip, frame, cursor, out);
                }
                Node::Blend { a, b, w } => blend_poses(slot(a)?, slot(b)?, w, out),
                Node::Add { base, layer, reference, mask, w } => {
                    let m = if mask == ALL { None } else { Some(masks.get(usize::from(mask)).ok_or(BAD)?.0.as_slice()) };
                    add_layer(slot(base)?, slot(layer)?, slot(reference)?, m, w, out);
                }
            }
        }
        let last = nodes.len() - 1;
        self.poses.get(last * n..(last + 1) * n).ok_or(BAD)
    }
}

/// `a` to `b` by `w` into `out` (see [`Node::Blend`]).
pub fn blend_poses(a: &[Xform], b: &[Xform], w: f32, out: &mut [Xform]) {
    if w.is_nan() || w <= 0.0 {
        out.iter_mut().zip(a).for_each(|(o, x)| *o = *x);
        return;
    }
    if w >= 1.0 {
        out.iter_mut().zip(b).for_each(|(o, x)| *o = *x);
        return;
    }
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        *o = Xform { t: lerp3(x.t, y.t, w), r: nlerp(x.r, y.r, w), s: lerp3(x.s, y.s, w) };
    }
}

/// `base` plus `w` x (`layer` - `reference`), per node weighed by `mask`
/// (see [`Node::Add`]). Rotations add on the right: base x (reference^-1 x
/// layer), scales multiply by their ratio.
pub fn add_layer(base: &[Xform], layer: &[Xform], reference: &[Xform], mask: Option<&[f32]>, w: f32, out: &mut [Xform]) {
    for (i, (o, b)) in out.iter_mut().zip(base).enumerate() {
        let k = w * mask.map_or(1.0, |m| m.get(i).copied().unwrap_or(0.0));
        let (Some(l), Some(r)) = (layer.get(i), reference.get(i)) else {
            *o = *b;
            continue;
        };
        if k == 0.0 || k.is_nan() {
            *o = *b;
            continue;
        }
        let dr = nlerp(QID, qmul(conj(r.r), l.r), k);
        let ratio = |j: usize| if r.s[j] == 0.0 { 1.0 } else { l.s[j] / r.s[j] };
        *o = Xform {
            t: [b.t[0] + (l.t[0] - r.t[0]) * k, b.t[1] + (l.t[1] - r.t[1]) * k, b.t[2] + (l.t[2] - r.t[2]) * k],
            r: qnorm(qmul(b.r, dr)),
            s: [b.s[0] * (1.0 + (ratio(0) - 1.0) * k), b.s[1] * (1.0 + (ratio(1) - 1.0) * k), b.s[2] * (1.0 + (ratio(2) - 1.0) * k)],
        };
    }
}
