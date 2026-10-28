//! First-person models (feature `view-model-models`, ChimpDuty intake):
//! loaded models in the view-model layer (`crate::view_model`), so a held
//! gun and arms draw with the view model's own field of view, near plane
//! and depth buffer, never clipping into a wall the player stands against.
//!
//! * A kit calls [`Models::draw_view_model`] every frame with a
//!   [`ModelInstance`] in camera space (x right, y up, -z forward, metres
//!   from the eye), like [`crate::world::RenderWorld::draw_view_model`] for
//!   meshes. Rotation and scale are camera-relative too.
//! * The renderer turns them into world space through the frame's camera
//!   (the first split-screen view when there are views) and draws them in
//!   the `view_model` pass after the view-model meshes, with the model
//!   pipeline (one sample), the world's sun, shadow map and point lights.
//! * They always draw their finest level (level 0 plus shared parts), cast
//!   no shadow, are not culled and are cleared each frame. Node hiding and
//!   recolours work as for world draws ([`Models::draw_view_model_with`]).
//! * Static parts only: skinned parts of a model are skipped here (pose
//!   arms in the model, or draw them as a static model per pose).

use evani_core::camera::CameraPose;
use evani_core::overlay::RenderStats;

use super::{ModelInstance, ModelState, Models, Part, SHARED, instance_words};
use crate::math;
use crate::view_model::{basis, camera_rotation};

/// This frame's camera-space model draws and their recolours (a field of
/// [`Models`]; cleared each frame).
#[derive(Debug, Default)]
pub struct VmDraws {
    draws: Vec<ModelInstance>,
    paints: Vec<(u32, [f32; 4])>,
}

impl Models {
    /// A first-person model this frame: `i` in camera space (x right, y up,
    /// -z forward). Nothing draws while the model is not ready.
    pub fn draw_view_model(&mut self, i: ModelInstance) {
        self.vm.draws.push(i);
    }

    /// [`Models::draw_view_model`] with recolours, as [`Models::draw_with`].
    pub fn draw_view_model_with(&mut self, mut i: ModelInstance, paints: &[(u32, [f32; 4])]) {
        let at = self.vm.paints.len();
        if at + paints.len() <= usize::from(u16::MAX) {
            i.paints = (at as u16, paints.len() as u16);
            self.vm.paints.extend_from_slice(paints);
        } else {
            i.paints = (0, 0);
        }
        self.vm.draws.push(i);
    }

    /// This frame's first-person model draws (camera space).
    pub fn view_model_draws(&self) -> &[ModelInstance] {
        &self.vm.draws
    }
}

/// `m` (camera space) in world space through `cam`.
pub fn to_world(cam: &CameraPose, m: &ModelInstance) -> ModelInstance {
    let [r, u, f] = basis(cam);
    let p = m.position;
    let at = |i: usize| cam.eye[i] + r[i] * p[0] + u[i] * p[1] - f[i] * p[2];
    ModelInstance {
        position: [at(0), at(1), at(2)],
        rotation: math::quat_mul(camera_rotation(cam), m.rotation),
        ..*m
    }
}

impl VmDraws {
    /// Append the instance words of part `p` of model `mi` for every draw
    /// that shows it (level 0 or shared, not hidden), in world space through
    /// `cam`.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn stage(&self, mi: usize, p: &Part, cam: &CameraPose, out: &mut Vec<f32>) {
        if p.lod != SHARED && p.lod != 0 {
            return;
        }
        for d in &self.draws {
            if d.model.0 as usize == mi && d.hide & p.tags == 0 {
                let a = usize::from(d.paints.0);
                let paints = self.paints.get(a..a + usize::from(d.paints.1)).unwrap_or_default();
                instance_words(&to_world(cam, d), p, paints, out);
            }
        }
    }

    /// Whether any draw this frame is of model `mi`.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn draws_model(&self, mi: usize) -> bool {
        self.draws.iter().any(|d| d.model.0 as usize == mi)
    }

    pub(crate) fn clear(&mut self) {
        self.draws.clear();
        self.paints.clear();
    }
}

/// Native counts of the first-person draws (one view, level 0 and shared
/// parts of each ready model), then clear them.
pub(super) fn count_draws(models: &mut Models, stats: &mut RenderStats) {
    for d in &models.vm.draws {
        let Some(e) = models.entries.get(d.model.0 as usize).filter(|e| e.state == ModelState::Ready) else {
            continue;
        };
        stats.draw_calls += e.shared.0 + e.lod_parts.first().copied().unwrap_or(0);
        let t = e.shared.1.saturating_add(e.lod_triangles.first().copied().unwrap_or(0));
        stats.triangles = stats.triangles.saturating_add(t);
    }
    models.vm.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::count_draws as count_all;
    use crate::world::RenderWorld;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    /// Camera space maps through the camera: a gun low and to the right in
    /// front of a camera looking along +x sits at +x, -z (its right), below.
    #[test]
    fn camera_space_goes_to_world_through_the_camera() {
        let cam = CameraPose { eye: [1.0, 2.0, 3.0], target: [11.0, 2.0, 3.0], up: [0.0, 1.0, 0.0], fov_y_deg: 60.0 };
        let m = ModelInstance::new(super::super::ModelId(0), [0.25, -0.2, -0.5]);
        let w = to_world(&cam, &m);
        assert!(close(w.position, [1.5, 1.8, 3.25]), "{:?}", w.position);
        // Facing -z in camera space faces the camera's forward (+x) in world.
        let fwd = crate::model::lod::rotate(w.rotation, [0.0, 0.0, -1.0]);
        assert!(close(fwd, [1.0, 0.0, 0.0]), "{fwd:?}");
    }

    /// Draws are kept apart from the world's, counted natively once the
    /// model is ready, and cleared every frame.
    #[test]
    fn view_model_draws_count_and_clear() {
        let path = format!("{}/../../tests/fixtures/models/kart.low.glb", env!("CARGO_MANIFEST_DIR"));
        let mut m = Models::default();
        let id = m.add_glb("models/kart.glb", &std::fs::read(path).unwrap());
        let rw = RenderWorld::default();
        let mut s = RenderStats::default();
        count_all(&mut m, &rw, None, &mut s);
        m.draw_view_model_with(ModelInstance::new(id, [0.2, -0.2, -0.5]), &[(1, [1.0, 0.0, 0.0, 1.0])]);
        assert_eq!(m.view_model_draws().len(), 1);
        assert!(m.draws.is_empty(), "not a world draw");
        let mut s = RenderStats::default();
        count_all(&mut m, &rw, None, &mut s);
        let e = &m.entries[0];
        assert_eq!(s.draw_calls, e.shared.0 + e.lod_parts.first().copied().unwrap_or(0));
        assert!(s.triangles > 0);
        assert!(m.view_model_draws().is_empty(), "cleared after the frame");
    }

    /// Staging writes one instance per shown draw, through the camera, and
    /// skips coarse levels.
    #[test]
    fn stage_writes_level_zero_only() {
        let path = format!("{}/../../tests/fixtures/models/kart.low.glb", env!("CARGO_MANIFEST_DIR"));
        let g = evani_gltf::Gltf::from_glb(&std::fs::read(path).unwrap()).unwrap();
        let p = crate::model::prepare(&g).unwrap();
        let mut v = VmDraws::default();
        v.draws.push(ModelInstance::new(super::super::ModelId(0), [0.0, 0.0, -1.0]));
        let cam = CameraPose::default();
        let mut out = Vec::new();
        let mut part = p.parts[0].clone();
        part.lod = 0;
        v.stage(0, &part, &cam, &mut out);
        assert_eq!(out.len(), crate::model::INSTANCE_WORDS);
        part.lod = 1;
        v.stage(0, &part, &cam, &mut out);
        assert_eq!(out.len(), crate::model::INSTANCE_WORDS, "level 1 is not drawn");
        assert!(v.draws_model(0) && !v.draws_model(1));
    }
}
