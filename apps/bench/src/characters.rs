//! The character phase (GPU skinning, STAGE3_PLAN S3-9): [`CHARACTERS`]
//! copies of the S3-7 rig fixture (`tests/fixtures/models/rig.src.glb`, a
//! skinned tube with six joints and three clips) over the light backdrop,
//! each posed every frame by evani-anim and drawn with its own joint
//! matrices ([`Models::draw_posed`]): the CPU side (sampling, model space,
//! skinning matrices) and the GPU side (one joint buffer upload, the skinned
//! vertex stages, skinned shadows) are both in the measured frames.
//!
//! The rig is compiled in, so the phase needs no served file. Copies cycle
//! through the clips (idle, wave, pose) at staggered times; the frame
//! number alone sets every pose, so two runs draw the same frames.

use evani_anim::{Clips, Graph, NONE, Node, Rig, Skeleton, Skin, Xform, frame_at};
use evani_render::model::{ModelId, ModelInstance, Models};

/// Characters the phase draws on every tier (each posed on its own: 256 x 6
/// joint matrices, 72 KB of joints a frame).
pub const CHARACTERS: u32 = 256;
/// The rig's name in the model table.
pub const RIG_URL: &str = "models/rig.glb";
/// Scale of each copy (the tube is 0.6 m tall at 1).
const SCALE: f32 = 1.5;
/// Ticks (60 Hz) between one copy's clip time and the next's.
const STAGGER: u32 = 13;

const RIG: &[u8] = include_bytes!("../../../tests/fixtures/models/rig.src.glb");
const CLIPS: &[u8] = include_bytes!("../../../tests/fixtures/models/rig.clips.evc1");

/// The rig in the model table and its animation state.
pub struct Crowd {
    pub model: ModelId,
    skeleton: Skeleton,
    skin: Skin,
    clips: Clips,
    graph: Graph,
    rig: Rig,
}

impl Crowd {
    /// Add the rig to `models` and read its skeleton, skin and clips.
    pub fn load(models: &mut Models) -> Result<Self, String> {
        let e = |m: &str| format!("character phase: {m}");
        let g = evani_gltf::Gltf::from_glb(RIG).map_err(|x| e(x.0))?;
        let parents: Vec<u16> = (0..g.nodes.len()).map(|n| g.parent(n).map_or(NONE, |p| p as u16)).collect();
        let rest: Vec<Xform> = g.nodes.iter().map(|n| Xform { t: n.translation, r: n.rotation, s: n.scale }).collect();
        let skeleton = Skeleton::new(&parents, &rest).map_err(|x| e(x.0))?;
        let s = g.skins.first().ok_or_else(|| e("the rig has no skin"))?;
        let flat = match s.inverse_bind_matrices {
            Some(a) => g.read_f32(a).map_err(|x| e(x.0))?,
            None => Vec::new(),
        };
        let ibm: Vec<[f32; 16]> = flat.as_chunks::<16>().0.to_vec();
        let joints: Vec<u16> = s.joints.iter().map(|&j| j as u16).collect();
        let skin = Skin::new(&skeleton, &joints, &ibm).map_err(|x| e(x.0))?;
        let clips = Clips::decode(CLIPS).map_err(|x| e(x.0))?;
        let graph = Graph::new(&skeleton, &clips, 1);
        let rig = Rig::new(&skeleton, joints.len());
        Ok(Self { model: models.add_glb(RIG_URL, RIG), skeleton, skin, clips, graph, rig })
    }

    /// Pose and draw one copy at each of `placements` (`[x, y, z, yaw]`)
    /// for phase frame `frame`. Returns the copies drawn.
    pub fn draw(&mut self, models: &mut Models, placements: &[[f32; 4]], frame: u32) -> u32 {
        let clips = self.clips.len().max(1) as u32;
        let mut n = 0;
        for (i, &[x, y, z, yaw]) in placements.iter().enumerate() {
            let i = i as u32;
            let clip = (i % clips) as u16;
            let at = frame_at(frame.wrapping_add(i.wrapping_mul(STAGGER)), 0.0, self.clips.frames(usize::from(clip)), true);
            let Ok(pose) = self.graph.eval(&self.skeleton, &self.clips, &[Node::Clip { clip, frame: at }], &[]) else {
                continue;
            };
            self.rig.set_pose(pose);
            self.rig.update(&self.skeleton);
            let (s, c) = (yaw * 0.5).sin_cos();
            models.draw_posed(ModelInstance::new(self.model, [x, y, z]).rotation([0.0, s, 0.0, c]).scale([SCALE; 3]), self.rig.skinning(&self.skin));
            n += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rig reads, and every copy draws posed (each with its own six
    /// matrices), cycling the three clips.
    #[test]
    fn crowd_draws_every_copy_posed() {
        let mut models = Models::default();
        let mut c = Crowd::load(&mut models).unwrap();
        assert_eq!(c.clips.len(), 3);
        let at: Vec<[f32; 4]> = (0..CHARACTERS).map(|i| [i as f32, 0.0, 0.0, 0.0]).collect();
        assert_eq!(c.draw(&mut models, &at, 5), CHARACTERS);
    }
}
