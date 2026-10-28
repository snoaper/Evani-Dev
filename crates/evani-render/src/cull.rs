//! CPU frustum culling (feature `frustum-cull`, S5-R2).
//!
//! Before a frame's draws are packed and uploaded, every mesh instance, decal
//! and model draw whose bounding sphere lies outside every frustum that will
//! see it is dropped from the frame's lists. Those frusta are:
//!
//! * the camera of each view (split-screen: an instance is kept when ANY view
//!   sees it, since the forward pass draws one packed list through every
//!   view), and
//! * the shadow map's light box, when this frame has a shadow map: an
//!   off-screen caster can still throw a shadow onto something on screen, so
//!   it is judged by the light's frustum, never the camera's. (The shadow
//!   pass and the forward pass read the same packed list, so a caster kept
//!   only for the light is also submitted to the forward pass, where the GPU
//!   clips it.)
//!
//! Bounds are spheres: built-in meshes use a fixed radius, custom meshes
//! (`MeshLibrary`) one computed from their vertices when the library changes,
//! models the sphere their file was prepared with (each tier's file has its
//! own, so every LOD tier is covered; the sphere spans all LOD levels). A
//! posed (skinned) draw gets its sphere inflated by [`POSED_PAD`], as the
//! stored sphere is the bind pose's. Anything with no known bound (a model
//! not read yet, an unknown mesh id) is kept. Culling is render-only: it
//! never touches the simulation, and the lists are rebuilt by the kit every
//! frame, so nothing needs restoring.

use crate::math::Mat4;
use crate::mesh::{BUILTIN_MESH_COUNT, MeshLibrary};
use crate::shaders::view_proj;
use crate::world::{MeshInstance, RenderWorld};
use evani_core::ecs::World;

/// Six planes `[nx, ny, nz, d]`, inside where `n . p + d >= 0`.
pub type Planes = [[f32; 4]; 6];

/// Views culled against at once (more: the frame is left uncut).
const MAX_VIEWS: usize = 6;
/// Bounding radius of each built-in mesh round its origin (a hair over the
/// true value): cube and bevel cube half the diagonal, plane, cylinder and
/// cone sqrt(0.5), spheres and dome 0.5.
const BUILTIN_RADIUS: [f32; BUILTIN_MESH_COUNT] = [0.87, 0.5, 0.71, 0.87, 0.71, 0.5, 0.71, 0.5];
/// Radius factor of a posed skinned draw (its limbs leave the bind pose).
#[cfg(feature = "skinning")]
const POSED_PAD: f32 = 1.5;
/// Slack on every plane test (metres), against rounding at the edges.
const SLACK: f32 = 0.05;

/// This frame's cull result (a world resource, shown in the module status).
#[derive(Debug, Clone, Default)]
pub struct CullStats {
    /// Mesh instances, decals and model draws dropped this frame.
    pub culled: u32,
    /// Everything that was judged (dropped and kept).
    pub judged: u32,
    /// Custom mesh bounds `[x, y, z, r]` and the library revision they are of.
    bounds: Vec<[f32; 4]>,
    revision: u64,
}

/// The six clip planes of a column-major view-projection (WebGPU depth 0..1,
/// either depth direction): `-w <= x, y <= w` and `0 <= z <= w`.
pub fn planes(m: &Mat4) -> Planes {
    let row = |r: usize| [m[r], m[4 + r], m[8 + r], m[12 + r]];
    let (x, y, z, w) = (row(0), row(1), row(2), row(3));
    let mix = |a: [f32; 4], b: [f32; 4], s: f32| [0, 1, 2, 3].map(|k| a[k] + s * b[k]);
    let mut p = [mix(w, x, 1.0), mix(w, x, -1.0), mix(w, y, 1.0), mix(w, y, -1.0), z, mix(w, z, -1.0)];
    for q in &mut p {
        let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
        if l > 1e-12 {
            *q = q.map(|v| v / l);
        }
    }
    p
}

/// Is the sphere `c`, `r` at least partly inside the planes?
pub fn inside(p: &Planes, c: [f32; 3], r: f32) -> bool {
    p.iter().all(|q| q[0] * c[0] + q[1] * c[1] + q[2] * c[2] + q[3] >= -r - SLACK)
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

/// The world sphere of local sphere `s = [x, y, z, r]` under an instance
/// transform; a zero radius (no bound known) is always seen.
fn seen(see: &dyn Fn([f32; 3], f32) -> bool, pos: [f32; 3], rot: [f32; 4], scale: [f32; 3], s: [f32; 4], pad: f32) -> bool {
    if s[3] <= 0.0 {
        return true;
    }
    let o = rotate(rot, [s[0] * scale[0], s[1] * scale[1], s[2] * scale[2]]);
    let k = scale[0].abs().max(scale[1].abs()).max(scale[2].abs());
    see([pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]], s[3] * k * pad)
}

impl CullStats {
    /// Bounds of custom meshes, rebuilt when the library's revision moved.
    fn refresh(&mut self, lib: Option<&MeshLibrary>) {
        let (rev, meshes) = lib.map_or((0, &[][..]), |l| (l.revision(), l.meshes()));
        if rev == self.revision && meshes.len() == self.bounds.len() {
            return;
        }
        self.revision = rev;
        self.bounds.clear();
        for m in meshes {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &m.vertices {
                lo = [0, 1, 2].map(|k| lo[k].min(v[k]));
                hi = [0, 1, 2].map(|k| hi[k].max(v[k]));
            }
            let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
            let r2 = m.vertices.iter().fold(0.0f32, |a, v| a.max([0, 1, 2].iter().map(|&k| (v[k] - c[k]) * (v[k] - c[k])).sum::<f32>()));
            self.bounds.push(if m.vertices.is_empty() { [0.0; 4] } else { [c[0], c[1], c[2], r2.sqrt()] });
        }
    }

    fn mesh_sphere(&self, id: u16) -> [f32; 4] {
        match usize::from(id).checked_sub(BUILTIN_MESH_COUNT) {
            None => [0.0, 0.0, 0.0, BUILTIN_RADIUS.get(usize::from(id)).copied().unwrap_or(0.0)],
            Some(i) => self.bounds.get(i).copied().unwrap_or([0.0; 4]),
        }
    }
}

/// Drop this frame's draws that no frustum sees. `size` is the render target
/// in pixels and `light_vp` the shadow map's view-projection (`None`: no
/// shadow map this frame). Call it once per frame, after the camera, views
/// and shadow plan are known and before the lists are packed.
pub fn run(world: &World, rw: &mut RenderWorld, size: [u32; 2], light_vp: Option<&Mat4>) {
    let mut stats = world.res_mut::<CullStats>();
    stats.culled = 0;
    stats.judged = 0;
    if rw.views.len() > MAX_VIEWS {
        return;
    }
    let mut cams = [[[0.0f32; 4]; 6]; MAX_VIEWS];
    let n = rw.views.len().max(1);
    if rw.views.is_empty() {
        cams[0] = planes(&view_proj(&rw.camera, size[0] as f32 / size[1].max(1) as f32));
    }
    for (c, v) in cams.iter_mut().zip(&rw.views) {
        let px = v.pixels(size);
        *c = planes(&view_proj(&v.camera, px[2] / px[3].max(1.0)));
    }
    let light = light_vp.map(planes);
    let see = |c: [f32; 3], r: f32| cams[..n].iter().any(|p| inside(p, c, r)) || light.as_ref().is_some_and(|p| inside(p, c, r));
    stats.refresh(world.try_res::<MeshLibrary>().as_deref());
    let before = (rw.meshes.len() + rw.decals.len()) as u32;
    let keep = |m: &MeshInstance| seen(&see, m.position, m.rotation, m.scale, stats.mesh_sphere(m.mesh.0), 1.0);
    rw.meshes.retain(&keep);
    rw.decals.retain(&keep);
    let kept = (rw.meshes.len() + rw.decals.len()) as u32;
    #[cfg_attr(not(feature = "models"), allow(unused_mut))]
    let (mut judged, mut culled) = (before, before - kept);
    #[cfg(feature = "models")]
    if let Some(mut ms) = world.try_res_mut::<crate::model::Models>() {
        judged += ms.draws.len() as u32;
        culled += ms.retain_draws(&mut |d, s| {
            #[cfg(feature = "skinning")]
            let pad = if d.joints != 0 { POSED_PAD } else { 1.0 };
            #[cfg(not(feature = "skinning"))]
            let pad = 1.0;
            seen(&see, d.position, d.rotation, d.scale, s, pad)
        });
    }
    stats.judged = judged;
    stats.culled = culled;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MeshId;
    use evani_core::camera::CameraPose;

    /// `m * [p, 1]` inside the clip volume (WebGPU: `-w..w`, depth `0..w`).
    fn clip_inside(m: &Mat4, p: [f32; 3]) -> bool {
        let v = |r: usize| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r];
        let (x, y, z, w) = (v(0), v(1), v(2), v(3));
        w > 0.0 && x.abs() <= w && y.abs() <= w && (0.0..=w).contains(&z)
    }

    /// The instance's corners and centre as the forward shader places them:
    /// `rotate(q, local * scale) + position`.
    fn points(i: &MeshInstance) -> Vec<[f32; 3]> {
        let ys: &[f32] = if i.mesh == MeshId::PLANE { &[0.0] } else { &[-0.5, 0.5] };
        let mut out = vec![i.position];
        for &x in &[-0.5f32, 0.5] {
            for &y in ys {
                for &z in &[-0.5f32, 0.5] {
                    let o = rotate(i.rotation, [x * i.scale[0], y * i.scale[1], z * i.scale[2]]);
                    out.push([i.position[0] + o[0], i.position[1] + o[1], i.position[2] + o[2]]);
                }
            }
        }
        out
    }

    /// An RTS camera (Cinder Ridge's: 58 degree pitch, 40 degree fov, 52 m
    /// back, a wide 3432 x 1308 canvas) over a 160 m map of terrain runs,
    /// yawed and stretched scenery and one huge ground plane: nothing with
    /// a corner on screen is culled, and most of the off-screen map is.
    #[test]
    fn oblique_camera_never_drops_what_it_sees() {
        let (pitch, d) = (58f32.to_radians(), 52.0);
        let cam = CameraPose { eye: [6.0, d * pitch.sin(), -20.0 + d * pitch.cos()], target: [6.0, 0.0, -20.0], up: [0.0, 1.0, 0.0], fov_y_deg: 40.0 };
        let size = [3432u32, 1308];
        let mut rw = RenderWorld { camera: cam, ..RenderWorld::default() };
        let mut far = MeshInstance::new(MeshId::PLANE, [0.0, -0.3, 0.0]);
        far.scale = [800.0, 1.0, 800.0];
        rw.meshes.push(far);
        for row in 0..80 {
            for run in 0..8 {
                let w = 2.0 + (row * 7 + run * 3) as f32 % 18.0;
                let mut s = MeshInstance::new(MeshId::CUBE, [-80.0 + run as f32 * 20.0 + w * 0.5, -0.5, -80.0 + row as f32 * 2.0 + 1.0]);
                s.scale = [w, 1.0, 2.0];
                rw.meshes.push(s);
            }
        }
        for k in 0..600 {
            let a = k as f32 * 2.399;
            let r = 4.0 + (k as f32 * 0.37) % 110.0;
            let mut p = MeshInstance::new(if k % 3 == 0 { MeshId::PLANE } else { MeshId::CUBE }, [r * a.cos(), 0.5, r * a.sin()]);
            let (h, t) = ((a * 0.5).sin(), (a * 0.5).cos());
            p.rotation = if k % 5 == 0 { [h * 0.6, h * 0.8, 0.0, t] } else { [0.0, h, 0.0, t] };
            p.scale = [1.0 + (k % 7) as f32, 0.5 + (k % 4) as f32 * 3.0, 0.4 + (k % 3) as f32];
            rw.decals.push(p);
        }
        let all: Vec<MeshInstance> = rw.meshes.iter().chain(&rw.decals).copied().collect();
        let mut world = World::new();
        world.insert_resource(CullStats::default());
        run(&world, &mut rw, size, None);
        let vp = view_proj(&cam, size[0] as f32 / size[1] as f32);
        let kept: Vec<MeshInstance> = rw.meshes.iter().chain(&rw.decals).copied().collect();
        let mut seen_count = 0;
        for i in &all {
            let on_screen = points(i).iter().any(|&p| clip_inside(&vp, p));
            seen_count += usize::from(on_screen);
            assert!(!on_screen || kept.contains(i), "culled but on screen: {i:?}");
        }
        let stats = world.res::<CullStats>();
        assert_eq!(stats.judged as usize, all.len());
        assert_eq!(stats.culled as usize, all.len() - kept.len());
        assert!(kept.len() >= seen_count && seen_count > 50, "kept {} seen {seen_count}", kept.len());
        assert!(stats.culled as usize > all.len() / 2, "{stats:?}");
    }
}
