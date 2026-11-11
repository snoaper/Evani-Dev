//! First-person view model (feature `view-model`, S5-E3): the weapon or
//! hands a first-person kit holds in front of the camera.
//!
//! * A kit pushes [`MeshInstance`]s in camera space with
//!   [`RenderWorld::draw_view_model`] every frame: x right, y up, -z forward
//!   (metres from the eye; a gun at `[0.25, -0.2, -0.5]` sits low and to the
//!   right). Rotation and scale are camera-relative too.
//! * The renderer turns them into world space through the frame's camera
//!   (the first split-screen view when there are views) and draws them after
//!   the forward pass into the HDR target, with their own field of view
//!   ([`ViewModel::fov_y_deg`]), a near plane of [`ViewModel::near`] and a
//!   depth buffer of their own: they never clip into a wall the player
//!   stands against, and the world never hides them.
//! * Lit like the world (sun, shadow map, point lights, fog, the look's
//!   materials, kit material); they cast no shadow and are cleared with the
//!   other draws each frame.
//! * Built-in and custom meshes only (no models), drawn without MSAA.
//! * Opt-in ([`ViewModel::mark_depth`], off by default): the pixels they
//!   cover are also written into the scene depth at the nearest value
//!   ([`crate::depth::NEAR_DEPTH`]), so depth-reading post passes (a sky or
//!   haze pass, the outline) treat the held object as near and leave it
//!   alone. See [`MARK_DEPTH_WGSL`].

use evani_core::camera::CameraPose;
use evani_core::num::clamp;

use crate::math::{self, Mat4};
use crate::world::{MeshInstance, RenderWorld};

/// The view model's far plane (metres): anything a kit holds is far nearer.
pub const FAR: f32 = 50.0;

/// What the first-person view model draws this frame (part of
/// [`RenderWorld`]; the meshes are cleared each frame, the lens is kept).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewModel {
    /// Camera-space mesh instances (x right, y up, -z forward).
    pub meshes: Vec<MeshInstance>,
    /// Vertical field of view of the view model's own projection, degrees.
    pub fov_y_deg: f32,
    /// Its near plane, metres (small, so a held object close to the eye is
    /// not cut).
    pub near: f32,
    /// Off by default. When on, after the `view_model` pass the pixels the
    /// view model covered are written into the scene depth at the nearest
    /// value ([`crate::depth::NEAR_DEPTH`], reversed-Z 1.0), so passes that
    /// read the scene depth see them as near. Kept like the lens (not
    /// cleared each frame). Nothing is allocated or drawn while it is off.
    pub mark_depth: bool,
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            meshes: Vec::new(),
            fov_y_deg: 55.0,
            near: 0.01,
            mark_depth: false,
        }
    }
}

impl ViewModel {
    /// Builder for [`ViewModel::mark_depth`].
    pub fn with_mark_depth(mut self, on: bool) -> Self {
        self.mark_depth = on;
        self
    }
}

/// Full-screen pass behind [`ViewModel::mark_depth`]: reads the view
/// model's own depth (cleared to [`crate::depth::CLEAR`], so any value above
/// it means a view-model fragment drew there) and writes
/// [`crate::depth::NEAR_DEPTH`] to the scene depth for those pixels only
/// (compare `Always`, depth write on, no colour target); the rest is
/// discarded and keeps the scene's depth.
pub const MARK_DEPTH_WGSL: &str = "
@group(0) @binding(0) var vm_depth: texture_depth_2d;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) p: vec4f) -> @builtin(frag_depth) f32 {
    if (textureLoad(vm_depth, vec2i(p.xy), 0) <= 0.0) {
        discard;
    }
    return 1.0;
}
";

/// Whether the mark step runs this frame: the option is on and something
/// reads the scene depth (otherwise the depth is not even kept).
pub fn marks_depth(vm: &ViewModel, depth_read: bool) -> bool {
    vm.mark_depth && depth_read
}

/// The mark step on the CPU, pixel for pixel as [`MARK_DEPTH_WGSL`] does it:
/// `scene[i]` becomes [`crate::depth::NEAR_DEPTH`] where `vm[i]` is above
/// the clear value (a view-model fragment drew), else it is left alone.
/// The reference the tests check the shader's words against.
pub fn mark_depth_reference(vm: &[f32], scene: &mut [f32]) {
    for (s, &v) in scene.iter_mut().zip(vm) {
        if crate::depth::is_nearer(v, crate::depth::CLEAR) {
            *s = crate::depth::NEAR_DEPTH;
        }
    }
}

impl RenderWorld {
    /// A view-model mesh: `m` in camera space (x right, y up, -z forward).
    pub fn draw_view_model(&mut self, m: MeshInstance) {
        self.view_model.meshes.push(m);
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The camera's right, up and forward unit vectors, as
/// [`math::look_at`] builds them. When forward runs along `up` (no right
/// vector), a world axis stands in for `up`, so the basis stays whole.
pub fn basis(cam: &CameraPose) -> [[f32; 3]; 3] {
    let f = math::normalize([cam.target[0] - cam.eye[0], cam.target[1] - cam.eye[1], cam.target[2] - cam.eye[2]]);
    let f = if f == [0.0; 3] { [0.0, 0.0, -1.0] } else { f };
    let mut r = math::normalize(cross(f, cam.up));
    if r == [0.0; 3] {
        r = math::normalize(cross(f, [0.0, 0.0, 1.0]));
    }
    if r == [0.0; 3] {
        r = math::normalize(cross(f, [1.0, 0.0, 0.0]));
    }
    [r, cross(r, f), f]
}

/// Unit quaternion `[x, y, z, w]` of the rotation whose matrix has the
/// columns `c0, c1, c2` (orthonormal, right-handed).
fn quat_from_columns(c0: [f32; 3], c1: [f32; 3], c2: [f32; 3]) -> [f32; 4] {
    // m[row][col]
    let (m00, m10, m20) = (c0[0], c0[1], c0[2]);
    let (m01, m11, m21) = (c1[0], c1[1], c1[2]);
    let (m02, m12, m22) = (c2[0], c2[1], c2[2]);
    let trace = m00 + m11 + m22;
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).max(1e-12).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).max(1e-12).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).max(1e-12).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 1e-12 { [q[0] / l, q[1] / l, q[2] / l, q[3] / l] } else { [0.0, 0.0, 0.0, 1.0] }
}

/// The camera's rotation (camera space to world) as a quaternion: the
/// columns right, up and back (`-forward`).
pub fn camera_rotation(cam: &CameraPose) -> [f32; 4] {
    let [r, u, f] = basis(cam);
    quat_from_columns(r, u, [-f[0], -f[1], -f[2]])
}

/// `m` (camera space) in world space through `cam`.
pub fn to_world(cam: &CameraPose, m: &MeshInstance) -> MeshInstance {
    let [r, u, f] = basis(cam);
    let p = m.position;
    let at = |i: usize| cam.eye[i] + r[i] * p[0] + u[i] * p[1] - f[i] * p[2];
    MeshInstance {
        position: [at(0), at(1), at(2)],
        rotation: math::quat_mul(camera_rotation(cam), m.rotation),
        ..*m
    }
}

impl ViewModel {
    /// Every mesh in world space through `cam`, into `out` (cleared first;
    /// no allocation once it has grown).
    pub fn world_instances(&self, cam: &CameraPose, out: &mut Vec<MeshInstance>) {
        out.clear();
        out.extend(self.meshes.iter().map(|m| to_world(cam, m)));
    }

    /// The view model's own view-projection: its field of view and near
    /// plane, far plane [`FAR`], reversed depth like the world's
    /// ([`math::perspective`]), looking through `cam`.
    pub fn view_proj(&self, cam: &CameraPose, aspect: f32) -> Mat4 {
        let fov = clamp(self.fov_y_deg, 1.0, 170.0).to_radians();
        let near = self.near.max(1e-4);
        let proj = math::perspective(fov, aspect, near, FAR.max(near * 2.0));
        let [r, u, f] = basis(cam);
        let e = cam.eye;
        #[rustfmt::skip]
        let view = [
            r[0], u[0], -f[0], 0.0,
            r[1], u[1], -f[1], 0.0,
            r[2], u[2], -f[2], 0.0,
            -dot(r, e), -dot(u, e), dot(f, e), 1.0,
        ];
        math::mul(&proj, &view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depth::{CLEAR, FAR_DEPTH, NEAR_DEPTH, is_nearer};

    /// A stand-in for a depth-reading post pass (a sky and haze pass): it
    /// paints sky wherever the scene depth is at the far plane.
    fn sky_pass(depth: &[f32]) -> Vec<bool> {
        depth.iter().map(|&d| d <= FAR_DEPTH).collect()
    }

    /// A 4x2 frame: the hand covers the two left pixels (depth 0.4 and 0.9
    /// in the view model's own buffer), the world is open sky everywhere
    /// except the bottom-right pixel (a wall at 0.3).
    fn frame() -> (Vec<f32>, Vec<f32>) {
        let vm = vec![0.4, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0];
        let scene = vec![FAR_DEPTH, FAR_DEPTH, FAR_DEPTH, FAR_DEPTH, FAR_DEPTH, FAR_DEPTH, FAR_DEPTH, 0.3];
        (vm, scene)
    }

    #[test]
    fn a_depth_reading_pass_sees_the_view_model_as_near_when_marked() {
        let (vm, mut scene) = frame();
        assert!(sky_pass(&scene)[0], "unmarked, the sky pass paints over the hand");
        assert!(marks_depth(&ViewModel::default().with_mark_depth(true), true));
        mark_depth_reference(&vm, &mut scene);
        let sky = sky_pass(&scene);
        assert!(!sky[0] && !sky[4], "the hand's pixels are no longer sky");
        assert_eq!((scene[0], scene[4]), (NEAR_DEPTH, NEAR_DEPTH));
        assert!(sky[1] && sky[2] && sky[3] && sky[5] && sky[6], "uncovered open sky is still sky");
        assert_eq!(scene[7], 0.3, "uncovered geometry keeps its depth");
        assert!(scene.iter().all(|&d| !is_nearer(d, NEAR_DEPTH)), "nothing is nearer than the nearest value");
    }

    #[test]
    fn the_option_is_off_by_default_and_then_the_depth_is_untouched() {
        let d = ViewModel::default();
        assert!(!d.mark_depth);
        assert!(!marks_depth(&d, true), "off: no mark step");
        assert!(!marks_depth(&d.clone().with_mark_depth(true), false), "on, but nothing reads depth: no mark step");
        assert_eq!(d, ViewModel { mark_depth: false, ..ViewModel::default() });
        let (_, scene) = frame();
        let before = scene.clone();
        // With the option off the renderer never calls the mark step, so the
        // scene depth is exactly what the forward pass left.
        assert_eq!(scene, before);
        assert_eq!(sky_pass(&scene), sky_pass(&before));
    }

    #[test]
    fn the_shader_says_what_the_reference_does() {
        assert_eq!(NEAR_DEPTH, 1.0);
        assert_eq!(CLEAR, 0.0);
        assert!(MARK_DEPTH_WGSL.contains("return 1.0;"));
        assert!(MARK_DEPTH_WGSL.contains("textureLoad(vm_depth, vec2i(p.xy), 0) <= 0.0"));
        assert!(MARK_DEPTH_WGSL.contains("discard;"));
    }

    #[test]
    fn the_mark_shader_validates() {
        let module = naga::front::wgsl::parse_str(MARK_DEPTH_WGSL).unwrap_or_else(|e| panic!("{}", e.emit_to_string(MARK_DEPTH_WGSL)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("mark_depth: {e:?}"));
    }
}
