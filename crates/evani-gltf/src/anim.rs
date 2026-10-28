//! glTF animations, for the converter's clip baking (S3-7) only: feature
//! `anim-source`, native only. Games read baked clips, never these.
//! Sampler data is read with [`crate::Gltf::read_f32`] (meshopt QUATERNION
//! and EXPONENTIAL filters are already applied at load).

use crate::Error;
use crate::doc::{idx, list, text};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct AnimChannel {
    pub sampler: usize,
    /// Target node (`None`: the channel targets nothing this reader knows).
    pub node: Option<usize>,
    /// `translation`, `rotation`, `scale` or `weights`.
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct AnimSampler {
    /// Key times accessor.
    pub input: usize,
    /// Key values accessor.
    pub output: usize,
    /// `LINEAR` (default), `STEP` or `CUBICSPLINE`.
    pub interpolation: String,
}

#[derive(Clone, Debug)]
pub struct Animation {
    pub name: String,
    pub channels: Vec<AnimChannel>,
    pub samplers: Vec<AnimSampler>,
}

pub(crate) fn parse(root: &Value, nodes: usize, accessors: usize) -> Result<Vec<Animation>, Error> {
    let acc = |v: &Value, k: &str| idx(v, k).filter(|&i| i < accessors).ok_or(err!("A1", "anim: sampler accessor out of range"));
    let mut out = Vec::new();
    for a in list(root, "animations")? {
        let mut samplers = Vec::new();
        for s in list(a, "samplers")? {
            samplers.push(AnimSampler { input: acc(s, "input")?, output: acc(s, "output")?, interpolation: text(s, "interpolation").unwrap_or_else(|| "LINEAR".into()) });
        }
        let mut channels = Vec::new();
        for c in list(a, "channels")? {
            let sampler = idx(c, "sampler").filter(|&i| i < samplers.len()).ok_or(err!("A2", "anim: channel sampler out of range"))?;
            let t = c.get("target").ok_or(err!("A3", "anim: channel without target"))?;
            let node = match t.get("node") {
                None => None,
                Some(_) => Some(idx(t, "node").filter(|&i| i < nodes).ok_or(err!("A4", "anim: channel node out of range"))?),
            };
            channels.push(AnimChannel { sampler, node, path: text(t, "path").ok_or(err!("A5", "anim: channel without path"))? });
        }
        out.push(Animation { name: text(a, "name").unwrap_or_default(), channels, samplers });
    }
    Ok(out)
}
