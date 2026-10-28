//! The bake: per-face ambient occlusion texels and convex-edge flags, both
//! from the occluding boxes only (no triangles, no GPU), deterministic.
//!
//! * A face gets a grid of texels about [`TEXEL`] m across (at most
//!   [`MAX_SIDE`] a side). A texel whose point 2 cm out lies inside another
//!   occluder is hidden; hidden texels are filled from visible neighbours
//!   afterwards, so the bilinear lookup never pulls a dark value out of a
//!   wall onto the open part of a face.
//! * Near occlusion: [`NEAR_RAYS`] cosine-spread rays of [`NEAR_LEN`] m; a
//!   hit at distance `t` occludes `1 - t / NEAR_LEN`. Sky visibility:
//!   [`SKY_RAYS`] fixed rays up into the sky (those facing away from the
//!   face are skipped) of [`SKY_LEN`] m. `ao = near * (0.6 + 0.4 * sky)`, so
//!   corridors and corners read darker than open yards, like a lightmap.
//! * Edges: an edge of a face is convex (worn, chipped) unless the point
//!   3 cm past its midpoint and 3 cm under the face lies inside another
//!   occluder (a wall continuing, a crate standing on the floor).

use crate::layout::Grid;
use crate::scene::{
    ProcBox, Scene, V3, add, cross, dot, normalize, rotate_inv, scale, sub, uv_axes,
};

/// Target texel size (m).
pub const TEXEL: f32 = 0.5;
/// Most texels along one side of a face.
pub const MAX_SIDE: u32 = 192;
/// Near-occlusion rays per texel.
pub const NEAR_RAYS: usize = 12;
/// Near-occlusion ray length (m).
pub const NEAR_LEN: f32 = 1.2;
/// Sky rays per texel.
pub const SKY_RAYS: usize = 8;
/// Sky ray length (m).
pub const SKY_LEN: f32 = 10.0;
/// How far out the hidden test and the rays start (m).
pub const LIFT: f32 = 0.02;
/// The edge test's step past the edge and under the face (m).
pub const EDGE_PROBE: f32 = 0.03;

/// One face's bake.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FaceBake {
    /// Texels along u and v.
    pub nu: u32,
    pub nv: u32,
    /// Where its texels start in [`Bake::texels`] (`None`: no AO, the face
    /// is fully hidden or its box is not baked).
    pub start: Option<u32>,
    /// Convex edges: bit 0 u-min, 1 u-max, 2 v-min, 3 v-max.
    pub edges: u32,
}

/// The whole bake.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bake {
    /// Per box, per face.
    pub faces: Vec<[FaceBake; 6]>,
    /// AO texels, row by row (v outer, u inner), 0..1.
    pub texels: Vec<f32>,
    /// Rays cast.
    pub rays: u64,
    /// Texels baked (visible ones).
    pub visible: u64,
}

/// The occluders and their grid, for point and ray queries.
pub(crate) struct Occluders<'a> {
    boxes: &'a [ProcBox],
    grid: Grid,
    aligned: Vec<bool>,
    stamp: Vec<u32>,
    tick: u32,
}

impl<'a> Occluders<'a> {
    pub(crate) fn new(boxes: &'a [ProcBox]) -> Self {
        let grid = Grid::new(boxes, |b| b.occluder, 0.01);
        let aligned = boxes
            .iter()
            .map(|b| b.rotation == [0.0, 0.0, 0.0, 1.0])
            .collect();
        Occluders {
            boxes,
            grid,
            aligned,
            stamp: vec![0; boxes.len()],
            tick: 0,
        }
    }

    fn local(&self, i: usize, p: V3) -> V3 {
        let b = &self.boxes[i];
        let d = sub(p, b.centre);
        if self.aligned[i] {
            d
        } else {
            rotate_inv(b.rotation, d)
        }
    }

    /// Is `p` inside any occluder by more than 1 mm?
    pub(crate) fn inside(&self, p: V3) -> bool {
        let Some(ci) = self.grid.cell_of(p[0], p[2]) else {
            return false;
        };
        self.grid.cells[ci].iter().any(|&i| {
            let i = i as usize;
            let l = self.local(i, p);
            let h = self.boxes[i].half;
            (0..3).all(|k| l[k].abs() < h[k] - 0.001)
        })
    }

    /// The nearest hit along `o + t d` for `t` in `[0, len]` (`d` unit).
    fn cast(&mut self, o: V3, d: V3, len: f32, any: bool) -> Option<f32> {
        self.tick = self.tick.wrapping_add(1);
        if self.tick == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.tick = 1;
        }
        let e = add(o, scale(d, len));
        let (ix0, iz0, ix1, iz1) = self.grid.range(
            o[0].min(e[0]),
            o[2].min(e[2]),
            o[0].max(e[0]),
            o[2].max(e[2]),
        )?;
        let mut best: Option<f32> = None;
        for iz in iz0..=iz1 {
            for ix in ix0..=ix1 {
                let ci = iz * self.grid.nx + ix;
                for &bi in &self.grid.cells[ci] {
                    let bi = bi as usize;
                    if self.stamp[bi] == self.tick {
                        continue;
                    }
                    self.stamp[bi] = self.tick;
                    let lim = best.unwrap_or(len);
                    if let Some(t) = self.ray_box(bi, o, d, lim) {
                        if any {
                            return Some(t);
                        }
                        best = Some(t);
                    }
                }
            }
        }
        best
    }

    fn ray_box(&self, i: usize, o: V3, d: V3, tmax: f32) -> Option<f32> {
        let b = &self.boxes[i];
        let lo = self.local(i, o);
        let ld = if self.aligned[i] {
            d
        } else {
            rotate_inv(b.rotation, d)
        };
        let (mut t0, mut t1) = (0.0f32, tmax);
        for k in 0..3 {
            if ld[k].abs() < 1e-9 {
                if lo[k].abs() > b.half[k] {
                    return None;
                }
            } else {
                let inv = 1.0 / ld[k];
                let (mut ta, mut tb) = ((-b.half[k] - lo[k]) * inv, (b.half[k] - lo[k]) * inv);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(t0)
    }
}

/// The near-occlusion directions in a tangent frame (z = normal): a fixed
/// cosine-weighted spiral, the same for every texel (smooth gradients).
fn near_dirs() -> [V3; NEAR_RAYS] {
    let mut out = [[0.0; 3]; NEAR_RAYS];
    for (i, d) in out.iter_mut().enumerate() {
        let r = ((i as f32 + 0.5) / NEAR_RAYS as f32).sqrt();
        let phi = i as f32 * 2.399_963 + 0.3;
        *d = [r * phi.cos(), r * phi.sin(), (1.0 - r * r).max(0.0).sqrt()];
    }
    out
}

/// The sky directions (world): two rings, 40 and 65 degrees up.
fn sky_dirs() -> [V3; SKY_RAYS] {
    let mut out = [[0.0; 3]; SKY_RAYS];
    for (i, d) in out.iter_mut().enumerate() {
        let elev = if i % 2 == 0 { 40.0f32 } else { 65.0f32 }.to_radians();
        let az = (i as f32 * 45.0 + 10.0).to_radians();
        *d = [elev.cos() * az.cos(), elev.sin(), elev.cos() * az.sin()];
    }
    out
}

/// A face's texel counts.
pub fn face_texels(b: &ProcBox, f: usize) -> (u32, u32) {
    let (ua, va) = uv_axes(f / 2);
    let n = |size: f32| ((size / TEXEL).ceil() as u32).clamp(1, MAX_SIDE);
    (n(2.0 * b.half[ua]), n(2.0 * b.half[va]))
}

/// The world point of face `f` at face coordinates `(u, v)` measured from
/// its centre, lifted `lift` along the normal.
fn face_point(b: &ProcBox, f: usize, u: f32, v: f32, lift: f32) -> V3 {
    let a = f / 2;
    let s = if f % 2 == 1 { 1.0 } else { -1.0 };
    let (ua, va) = uv_axes(a);
    let mut l = [0.0; 3];
    l[a] = s * (b.half[a] + lift);
    l[ua] = u;
    l[va] = v;
    b.to_world(l)
}

/// Convex-edge flags of every face of box `bi`.
pub(crate) fn edge_flags(occ: &Occluders, b: &ProcBox) -> [u32; 6] {
    let mut out = [0u32; 6];
    for (f, o) in out.iter_mut().enumerate() {
        let (ua, va) = uv_axes(f / 2);
        let (hu, hv) = (b.half[ua], b.half[va]);
        let probes = [
            (-(hu + EDGE_PROBE), 0.0),
            (hu + EDGE_PROBE, 0.0),
            (0.0, -(hv + EDGE_PROBE)),
            (0.0, hv + EDGE_PROBE),
        ];
        for (k, (u, v)) in probes.into_iter().enumerate() {
            let p = face_point(b, f, u, v, -EDGE_PROBE);
            if !occ.inside(p) {
                *o |= 1 << k;
            }
        }
    }
    out
}

/// Bake every face of the scene.
pub fn bake(scene: &Scene) -> Bake {
    let mut occ = Occluders::new(&scene.boxes);
    let near = near_dirs();
    let sky = sky_dirs();
    let mut out = Bake::default();
    for b in &scene.boxes {
        let edges = edge_flags(&occ, b);
        let mut faces = [FaceBake::default(); 6];
        for (f, fb) in faces.iter_mut().enumerate() {
            let (nu, nv) = face_texels(b, f);
            fb.edges = edges[f];
            fb.nu = nu;
            fb.nv = nv;
            if !b.bake_ao {
                continue;
            }
            if let Some(t) = bake_face(&mut occ, b, f, nu, nv, &near, &sky, &mut out) {
                fb.start = Some(out.texels.len() as u32);
                out.texels.extend_from_slice(&t);
            }
        }
        out.faces.push(faces);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn bake_face(
    occ: &mut Occluders,
    b: &ProcBox,
    f: usize,
    nu: u32,
    nv: u32,
    near: &[V3],
    sky: &[V3],
    out: &mut Bake,
) -> Option<Vec<f32>> {
    let (ua, va) = uv_axes(f / 2);
    let (hu, hv) = (b.half[ua], b.half[va]);
    let n = b.face_normal(f);
    // A tangent frame for the near rays.
    let helper = if n[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = normalize(cross(helper, n));
    let bt = cross(n, t);
    let world_near: Vec<V3> = near
        .iter()
        .map(|d| add(add(scale(t, d[0]), scale(bt, d[1])), scale(n, d[2])))
        .collect();
    let sky_use: Vec<V3> = sky.iter().copied().filter(|d| dot(*d, n) > 0.05).collect();
    let (nu_s, nv_s) = (nu as usize, nv as usize);
    let mut texels = vec![f32::NAN; nu_s * nv_s];
    let mut any = false;
    for j in 0..nv_s {
        for i in 0..nu_s {
            let u = -hu + (i as f32 + 0.5) * (2.0 * hu / nu as f32);
            let v = -hv + (j as f32 + 0.5) * (2.0 * hv / nv as f32);
            let p = face_point(b, f, u, v, LIFT);
            if occ.inside(p) {
                continue;
            }
            any = true;
            out.visible += 1;
            let mut occl = 0.0;
            for d in &world_near {
                if let Some(hit) = occ.cast(p, *d, NEAR_LEN, false) {
                    occl += 1.0 - hit / NEAR_LEN;
                }
            }
            let near_v = 1.0 - occl / near.len() as f32;
            let sky_v = if sky_use.is_empty() {
                0.5
            } else {
                let open = sky_use
                    .iter()
                    .filter(|d| occ.cast(p, **d, SKY_LEN, true).is_none())
                    .count();
                open as f32 / sky_use.len() as f32
            };
            out.rays += (world_near.len() + sky_use.len()) as u64;
            texels[j * nu_s + i] = (near_v * (0.6 + 0.4 * sky_v)).clamp(0.0, 1.0);
        }
    }
    if !any {
        return None;
    }
    dilate(&mut texels, nu_s, nv_s);
    Some(texels)
}

/// Fill hidden (NaN) texels from their visible neighbours, a ring at a time;
/// what is left after a few rings becomes 1.
fn dilate(t: &mut [f32], nu: usize, nv: usize) {
    for _ in 0..6 {
        let prev = t.to_vec();
        let mut changed = false;
        for j in 0..nv {
            for i in 0..nu {
                if !prev[j * nu + i].is_nan() {
                    continue;
                }
                let mut sum = 0.0;
                let mut n = 0;
                let mut add_n = |ii: usize, jj: usize| {
                    let v = prev[jj * nu + ii];
                    if !v.is_nan() {
                        sum += v;
                        n += 1;
                    }
                };
                if i > 0 {
                    add_n(i - 1, j);
                }
                if i + 1 < nu {
                    add_n(i + 1, j);
                }
                if j > 0 {
                    add_n(i, j - 1);
                }
                if j + 1 < nv {
                    add_n(i, j + 1);
                }
                if n > 0 {
                    t[j * nu + i] = sum / n as f32;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for v in t.iter_mut() {
        if v.is_nan() {
            *v = 1.0;
        }
    }
}
