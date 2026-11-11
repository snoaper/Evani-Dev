//! Picking a part in the 3D view: a tap (a short press that didn't move)
//! on the viewport becomes a ray from the camera; the nearest triangle of
//! the drawn shape it meets names a region, and that region's part is
//! selected (`Pick::Part`), exactly as clicking it in the inspector's parts
//! list. The selected part then flashes briefly in the view
//! ([`FLASH_FRAMES`]), from either place.
//!
//! Pure functions and one resource, so it is tested natively; the page
//! (web.rs) only writes [`StudioTap`].

use evani_kit_material_view::studio::evani_studio::geo::StyledMesh;
use evani_core::camera::CameraPose;

/// A tap on the view this frame: where, as a share of the canvas from its
/// top-left (`[0..1, 0..1]`). The scene takes it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StudioTap(pub Option<[f32; 2]>);

/// Frames the selected part glows for after it is picked (about half a
/// second at 60 Hz).
pub const FLASH_FRAMES: u32 = 30;

/// The glow added to a flashing part (linear RGB, warm like the page's
/// accent).
pub const FLASH_GLOW: [f32; 3] = [0.38, 0.29, 0.08];

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 1e-9 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, -1.0] }
}

/// The ray (origin, unit direction) through `at` (a share of the canvas from
/// its top-left) for `cam` at `aspect` (width / height).
pub fn ray(cam: &CameraPose, aspect: f32, at: [f32; 2]) -> ([f32; 3], [f32; 3]) {
    let fwd = norm(sub(cam.target, cam.eye));
    let right = norm(cross(fwd, cam.up));
    let up = cross(right, fwd);
    let t = (cam.fov_y_deg.to_radians() * 0.5).tan();
    let (x, y) = ((at[0] * 2.0 - 1.0) * t * aspect, (1.0 - at[1] * 2.0) * t);
    (cam.eye, norm([fwd[0] + right[0] * x + up[0] * y, fwd[1] + right[1] * x + up[1] * y, fwd[2] + right[2] * x + up[2] * y]))
}

/// The nearest hit of the ray on `mesh` drawn at `at`, scaled by `scale`:
/// (distance, region). Both faces count (thin boards, a voxel's sides).
pub fn hit(mesh: &StyledMesh, at: [f32; 3], scale: f32, origin: [f32; 3], dir: [f32; 3]) -> Option<(f32, u8)> {
    let mut best: Option<(f32, u8)> = None;
    let world = |p: [f32; 3]| [at[0] + p[0] * scale, at[1] + p[1] * scale, at[2] + p[2] * scale];
    for part in &mesh.parts {
        for t in part.indices.as_chunks::<3>().0 {
            let (Some(a), Some(b), Some(c)) = (part.positions.get(t[0] as usize), part.positions.get(t[1] as usize), part.positions.get(t[2] as usize)) else { continue };
            let (a, b, c) = (world(*a), world(*b), world(*c));
            // Möller-Trumbore.
            let (e1, e2) = (sub(b, a), sub(c, a));
            let p = cross(dir, e2);
            let det = dot(e1, p);
            if det.abs() < 1e-12 {
                continue;
            }
            let inv = 1.0 / det;
            let s = sub(origin, a);
            let u = dot(s, p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(dir, q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let d = dot(e2, q) * inv;
            if d > 1e-5 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, part.region));
            }
        }
    }
    best
}

/// The vector helpers, for the scene's tests.
#[cfg(test)]
pub mod tests_support {
    pub fn norm(a: [f32; 3]) -> [f32; 3] {
        super::norm(a)
    }
    pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        super::cross(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_kit_material_view::studio::evani_studio::geo::StyledPart;

    fn quad(region: u8, z: f32) -> StyledPart {
        StyledPart { region, positions: vec![[-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]], normals: vec![[0.0, 0.0, 1.0]; 4], indices: vec![0, 1, 2, 0, 2, 3], samples: Vec::new() }
    }

    /// A tap in the middle of the view meets the nearer of two stacked
    /// quads; a tap in the corner misses; the scale and place count.
    #[test]
    fn a_tap_picks_the_nearest_part_under_it() {
        let cam = CameraPose { eye: [0.0, 0.0, 5.0], target: [0.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 40.0 };
        let mesh = StyledMesh { parts: vec![quad(0, -0.5), quad(3, 0.5)] };
        let (o, d) = ray(&cam, 1.5, [0.5, 0.5]);
        assert_eq!(hit(&mesh, [0.0; 3], 1.0, o, d).map(|h| h.1), Some(3));
        let (o, d) = ray(&cam, 1.5, [0.02, 0.02]);
        assert_eq!(hit(&mesh, [0.0; 3], 1.0, o, d), None);
        // Moved right by 3 and halved: the middle of the view misses, a tap
        // over it (right of centre) meets it.
        assert_eq!(hit(&mesh, [3.0, 0.0, 0.0], 0.5, o, ray(&cam, 1.5, [0.5, 0.5]).1), None);
        let x = 0.5 + 3.0 / 4.5 / (2.0 * (20f32.to_radians().tan()) * 1.5);
        let (o, d) = ray(&cam, 1.5, [x, 0.5]);
        assert_eq!(hit(&mesh, [3.0, 0.0, 0.0], 0.5, o, d).map(|h| h.1), Some(3), "x {x}");
    }
}
