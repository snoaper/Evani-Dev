//! Z-fighting by the depth maths (check 4, geometry test).
//!
//! Two surfaces fight where the depth buffer can't tell which is in front:
//! the depth values they write at the same pixel differ by less than the
//! error of the depth they are given. Nothing here is a tuned constant: the
//! error comes from the renderer's real depth setup ([`DepthSetup::engine`]:
//! projection, depth format, the decal depth offset) and from the cameras the
//! game really uses ([`Viewer`], from the recorded title, race and replay
//! frames, or the live camera).
//!
//! **Depth.** A point at view distance `z` (along the camera's forward axis)
//! gets the depth `d(z) = a + b / z` (`a`, `b` from the projection matrix;
//! `b < 0` when depth grows with distance, `b > 0` for reversed depth; the
//! engine uses reversed depth, near 1 and far 0, see `evani_render::depth`).
//! A decal (`RenderWorld::draw_decal`) is drawn `off` closer
//! ([`DepthSetup::decal_offset`], in depth units, the forward shader's
//! `o.pos.z += pad * w` with the pad signed toward the camera: `t off` with
//! `t` = [`DepthSetup::toward`], +1 for reversed depth). The hardware depth
//! bias is not used by the camera passes (only by the shadow map), so it is
//! not part of the sum.
//!
//! **Separation.** At a sample `q` on face A, the eye ray through `q` meets
//! B's plane; `Δ = (d_B + t off_B) − (d_A + t off_A)` there. Along any screen
//! row the inverse distance `1/z` of a plane is affine, so `Δ` is exactly
//! affine in screen space and the patch where it is small is cut exactly.
//!
//! **Error** (all depth units, `u = 2^-24`, f32 rounding):
//! `ε = step(d) + xf_A + xf_B + snap_A + snap_B`
//! - `step(d)`: the spacing of stored depth values at `d` (Depth32Float: one
//!   f32 ulp of `d`; Depth24Plus `2^-24`; Depth16Unorm `2^-16`).
//! - `xf = 2u (S2 + |d| S3) / z`: the vertex transform. Clip `w = f·p − f·eye`
//!   and clip `z = a w + b` are f32 dot products of the world position `p`;
//!   rounding the position and the dot product each costs `u` times the sum of
//!   the magnitudes, `S3 = Σ|f_k p_k| + |f·eye|` for `w` and
//!   `S2 = |a| Σ|f_k p_k| + |b − a f·eye|` for clip z, and the depth
//!   `z_clip / w` carries `(δz_clip + d δw) / w`. This term grows with the
//!   distance of the world from the origin and shrinks with view distance.
//! - `snap = ½ 2^-bits |b| px (|n·s| + |n·u|) / |D|`: the rasteriser snaps
//!   vertices to `2^-bits` of a pixel (`bits` 8), which moves the plane's
//!   depth by its screen gradient (`b` times the gradient of `1/z`,
//!   `(n·s)/D` per unit of tangent; `px = 2 tan(fov/2) / height` per pixel;
//!   `D` the eye-to-plane distance). This is the slope term: a surface seen
//!   at a grazing angle has a steep depth gradient and needs more separation.
//!
//! It fights where `|Δ| < ε`. In metres along the normal, a separation `g`
//! is worth `k g` of depth with `k = |b| (r·f) / (z² |n·r|)` (`r` the unit
//! view ray), so the smallest separable gap there is `ε / k` and the decal
//! offset is worth `off / k` metres.
//!
//! **Pairs.** Faces of different items, parallel within `zfight_angle`,
//! whose projections onto A's plane overlap (polygon clipping) within the
//! search reach, a bound of `ε / k` over the viewers that see the item (the
//! scene's spatial grid finds the candidates). Per viewer the overlap is cut
//! to the near plane, the screen and `|Δ| < ε`; the rest must be at least
//! `zfight_px` pixels and a pixel wide, face the viewer when the item is a
//! closed mesh, convex or not (its back faces are hidden by its front), and
//! be seen: a ray from the eye must not meet a third item first (the worst
//! viewers are tried first). Faces with the same material and colour facing
//! the same way can't show a fight and are skipped.
//!
//! **Thin slabs.** The engine draws back faces ([`DepthSetup::back_faces`]:
//! no face culling) and lights them by their own normal, which faces away
//! from the eye, so they come out dark. An item's own faces that face
//! opposite ways (its top and its underside) are a pair like any other: where
//! the slab is thinner than `ε / k` the dark underside shows through its top
//! in a moving speckle ("thin slab"). Faces of one item that face the same
//! way (a mesh's neighbouring triangles) never pair. A decal's underside gets
//! the same depth offset as its top, so the offset does not help.

use serde_json::{Value, json};

use crate::geom::*;
use crate::image::Camera;
use crate::report::{Check, Issue, Severity};
use crate::scene::{Config, Item, Scene};
use crate::shape::Face;

/// f32 unit roundoff (half an ulp at 1).
pub const UNIT_ROUNDOFF: f64 = 1.0 / 16_777_216.0;

/// Depth buffer format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthFormat {
    Depth32Float,
    /// 24-bit depth (WebGPU `depth24plus`: at least 24 bits; taken as 24-bit
    /// fixed point, the coarser of what it may be).
    Depth24Plus,
    Depth16Unorm,
}

impl DepthFormat {
    pub fn name(self) -> &'static str {
        match self {
            DepthFormat::Depth32Float => "Depth32Float",
            DepthFormat::Depth24Plus => "Depth24Plus",
            DepthFormat::Depth16Unorm => "Depth16Unorm",
        }
    }

    /// Spacing of the stored depth values at `d` (0..1).
    pub fn step(self, d: f64) -> f64 {
        match self {
            DepthFormat::Depth32Float => {
                // One f32 ulp of d (2^-149 at 0, the smallest subnormal).
                let d = d.abs();
                if d < f64::from(f32::MIN_POSITIVE) {
                    return 2f64.powi(-149);
                }
                2f64.powi(d.log2().floor() as i32 - 23)
            }
            DepthFormat::Depth24Plus => 2f64.powi(-24),
            DepthFormat::Depth16Unorm => 2f64.powi(-16),
        }
    }
}

/// The renderer's depth setup, as the checks need it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthSetup {
    /// Depth of a point at view distance z: `a + b / z`.
    pub a: f64,
    pub b: f64,
    pub near: f64,
    pub far: f64,
    /// Depth 1 at the near plane, 0 at the far plane.
    pub reversed: bool,
    pub format: DepthFormat,
    /// How much closer (depth units) a decal is drawn.
    pub decal_offset: f64,
    /// Sub-pixel precision of the rasteriser (bits).
    pub subpixel_bits: u32,
    /// Rounding of the vertex maths (f32).
    pub unit_roundoff: f64,
    /// Back faces are drawn (no face culling) and lit by their own normal, so
    /// an item's underside can fight its own top ("thin slab").
    pub back_faces: bool,
}

impl DepthSetup {
    /// A perspective projection mapping `near`..`far` to depth 0..1 (or 1..0
    /// when `reversed`), with no decal offset.
    pub fn perspective(near: f64, far: f64, reversed: bool, format: DepthFormat) -> DepthSetup {
        let (a, b) = if reversed {
            let b = near * far / (far - near);
            (-b / far, b)
        } else {
            let b = near * far / (near - far);
            (-b / near, b)
        };
        DepthSetup {
            a,
            b,
            near,
            far,
            reversed,
            format,
            decal_offset: 0.0,
            subpixel_bits: 8,
            unit_roundoff: UNIT_ROUNDOFF,
            back_faces: true,
        }
    }

    /// The engine's camera passes (evani-render): `math::perspective` with
    /// `shaders::NEAR` and `FAR` and the convention of `evani_render::depth`
    /// (reversed: depth 1 at near, 0 at far, cleared to 0), a Depth32Float
    /// target compared with `Greater`, no hardware depth bias, no face
    /// culling (back faces lit by their own normal), and decals pulled
    /// `shaders::DECAL_DEPTH_OFFSET` closer. `a` and `b` are read from the
    /// engine's own (f32) matrix; the test `engine_setup_matches_the_renderer`
    /// reads the renderer's source so a change there fails here.
    pub fn engine() -> DepthSetup {
        use evani_render::shaders::{DECAL_DEPTH_OFFSET, FAR, NEAR};
        let m = evani_render::math::perspective(1.0, 1.0, NEAR, FAR);
        // Clip z = m10 z_view + m14 and w = -z_view = z, so depth = -m10 + m14 / z.
        DepthSetup {
            a: -f64::from(m[10]),
            b: f64::from(m[14]),
            near: f64::from(NEAR),
            far: f64::from(FAR),
            reversed: evani_render::depth::REVERSED,
            format: DepthFormat::Depth32Float,
            decal_offset: f64::from(DECAL_DEPTH_OFFSET),
            subpixel_bits: 8,
            unit_roundoff: UNIT_ROUNDOFF,
            back_faces: true,
        }
    }

    /// Sign of a depth change toward the camera (+1 reversed, -1 not): a
    /// decal's offset moves its depth by `toward() * decal_offset`.
    pub fn toward(&self) -> f64 {
        if self.reversed { 1.0 } else { -1.0 }
    }

    /// Depth of a point at view distance `z`.
    pub fn depth(&self, z: f64) -> f64 {
        self.a + self.b / z
    }

    /// Error of one surface's depth at view distance `z`, world point `p`,
    /// from camera forward `f` and eye `eye`: the vertex transform term.
    pub fn transform_error(&self, p: V3, z: f64, f: V3, eye: V3) -> f64 {
        let fe = dot(f, eye);
        let sp = (f[0] * p[0]).abs() + (f[1] * p[1]).abs() + (f[2] * p[2]).abs();
        let s3 = sp + fe.abs();
        let s2 = self.a.abs() * sp + (self.b - self.a * fe).abs();
        let d = self.depth(z);
        2.0 * self.unit_roundoff * (s2 + d.abs() * s3) / z
    }

    /// The smallest separation (m, along the normal) two parallel surfaces
    /// need to be told apart, seen head-on at distance `z` from an eye at
    /// `eye_dist` from the world origin (looking back towards it). For the
    /// docs and the tests; the check itself works per sample.
    pub fn min_separation(&self, z: f64, eye_dist: f64) -> f64 {
        let f = [0.0, 0.0, -1.0];
        let eye = [0.0, 0.0, eye_dist];
        let p = [0.0, 0.0, eye_dist - z];
        let d = self.depth(z);
        let xf = self.transform_error(p, z, f, eye);
        // Head-on the plane's depth has no screen gradient at the centre, so
        // no snapping term.
        let eps = self.format.step(d) + 2.0 * xf;
        eps * z * z / self.b.abs()
    }
}

/// A camera the game shows the world through.
#[derive(Debug, Clone, PartialEq)]
pub struct Viewer {
    pub cam: Camera,
    /// Render height in pixels (the width follows from the aspect).
    pub height_px: f64,
    /// Where it comes from: "title", "race", "live", ...
    pub label: String,
    /// Time of the recorded frame (s).
    pub time: Option<f64>,
}

impl Viewer {
    pub fn new(cam: Camera, height_px: f64, label: &str, time: Option<f64>) -> Viewer {
        Viewer {
            cam,
            height_px: height_px.max(1.0),
            label: label.into(),
            time,
        }
    }
}

/// A viewer's geometry, computed once.
#[derive(Debug, Clone, Copy)]
struct View {
    eye: V3,
    f: V3,
    s: V3,
    u: V3,
    /// Half extents of the screen in tangent units (x, y).
    tx: f64,
    ty: f64,
    /// Tangent units per pixel.
    px: f64,
    /// Cosine of the angle from the axis to a screen corner.
    cos_corner: f64,
}

impl View {
    fn new(v: &Viewer) -> View {
        let (f, s, u) = v.cam.basis();
        let t = (v.cam.fov_y_deg.to_radians() * 0.5).tan();
        let tx = t * v.cam.aspect;
        View {
            eye: v.cam.eye,
            f,
            s,
            u,
            tx,
            ty: t,
            px: 2.0 * t / v.height_px,
            cos_corner: 1.0 / (1.0 + tx * tx + t * t).sqrt(),
        }
    }

    /// The view ray (f·r = 1) through tangent point q.
    fn ray(&self, q: [f64; 2]) -> V3 {
        add(self.f, add(mul(self.s, q[0]), mul(self.u, q[1])))
    }
}

/// A face's plane as one viewer sees it.
#[derive(Debug, Clone, Copy)]
struct Plane {
    n: V3,
    /// Signed eye-to-plane distance `n·(p0 − eye)`.
    dist: f64,
    off: f64,
    snap: f64,
}

impl Plane {
    fn new(set: &DepthSetup, v: &View, face: &Face, off: f64) -> Plane {
        let dist = dot(face.n, sub(face.pts[0], v.eye));
        let snap = 0.5 * 2f64.powi(-(set.subpixel_bits as i32)) * set.b.abs() * v.px * (dot(face.n, v.s).abs() + dot(face.n, v.u).abs())
            / dist.abs().max(1e-12);
        Plane { n: face.n, dist, off, snap }
    }
    /// 1 / view distance of the plane along ray r (f·r = 1).
    fn inv_z(&self, r: V3) -> f64 {
        dot(self.n, r) / self.dist
    }
}

/// Keep the part of convex polygon `p` where `f` is at most 0 (one
/// Sutherland-Hodgman step; exact when `f` is affine).
pub fn clip_half(p: &[[f64; 2]], f: &dyn Fn([f64; 2]) -> f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(p.len() + 1);
    for k in 0..p.len() {
        let (a, b) = (p[k], p[(k + 1) % p.len()]);
        let (fa, fb) = (f(a), f(b));
        if fa <= 0.0 {
            out.push(a);
        }
        if (fa <= 0.0) != (fb <= 0.0) {
            let t = fa / (fa - fb);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

fn perimeter(p: &[[f64; 2]]) -> f64 {
    (0..p.len())
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
        })
        .sum()
}

fn ccw(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if area_2d(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// Maps 2D plane coordinates back onto a face, anchored at one of its own
/// vertices (so it is exact far from the origin).
#[derive(Debug, Clone, Copy)]
pub struct PlanePoint {
    o: V3,
    o2: [f64; 2],
    u: V3,
    v: V3,
}

impl PlanePoint {
    pub fn at(&self, x: f64, y: f64) -> V3 {
        add(self.o, add(mul(self.u, x - self.o2[0]), mul(self.v, y - self.o2[1])))
    }
}

/// Where two near-parallel faces overlap, seen along A's normal.
#[derive(Debug, Clone)]
pub struct Patch {
    /// The overlap within `lim` of B's plane, in A's plane basis (2D, CCW).
    pub poly: Vec<[f64; 2]>,
    /// Area of the whole overlap of the projections (m^2).
    pub overlap: f64,
    /// Area of `poly` (m^2).
    pub area: f64,
    /// Signed separation from A to B's plane along A's normal over `poly`.
    pub sep_min: f64,
    pub sep_max: f64,
    pub to_3d: PlanePoint,
}

/// The patch where faces `fa` and `fb` (normals parallel or opposite within
/// `acos(cos_min)`) overlap and are closer than `lim`. The separation is
/// measured at each point of the overlap, from `fa` to `fb`'s plane along
/// `fa.n`, relative to a vertex of `fb`, so it is the real distance between
/// the faces wherever they are in the world.
pub fn face_patch(fa: &Face, fb: &Face, lim: f64, cos_min: f64) -> Option<Patch> {
    let cosn = dot(fa.n, fb.n);
    if cosn.abs() < cos_min || fa.pts.len() < 3 || fb.pts.len() < 3 {
        return None;
    }
    let anchor = fb.pts[0];
    let sep3 = |p: V3| dot(fb.n, sub(anchor, p)) / cosn;
    // The separation is affine over fa: its range over fa's vertices bounds
    // it (a cheap exact reject).
    let (lo, hi) = fa.pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        let s = sep3(*p);
        (lo.min(s), hi.max(s))
    });
    if lo >= lim || hi <= -lim {
        return None;
    }
    let (u, v) = plane_basis(fa.n);
    let o = fa.pts[0];
    let to_3d = PlanePoint { o, o2: [dot(o, u), dot(o, v)], u, v };
    let pa = ccw(fa.to_2d(u, v));
    let pb = ccw(fb.to_2d(u, v));
    let inter = clip_2d(&pa, &pb);
    if inter.len() < 3 {
        return None;
    }
    let overlap = area_2d(&inter).abs();
    let sep = |q: [f64; 2]| sep3(to_3d.at(q[0], q[1]));
    let poly = clip_half(&inter, &|q| sep(q) - lim);
    let poly = if poly.len() >= 3 { clip_half(&poly, &|q| -sep(q) - lim) } else { poly };
    if poly.len() < 3 {
        return None;
    }
    let area = area_2d(&poly).abs();
    let (mut sep_min, mut sep_max) = (f64::MAX, f64::MIN);
    for q in &poly {
        let s = sep(*q);
        sep_min = sep_min.min(s);
        sep_max = sep_max.max(s);
    }
    Some(Patch {
        poly,
        overlap,
        area,
        sep_min,
        sep_max,
        to_3d,
    })
}

/// What one viewer sees of a patch.
#[derive(Debug, Clone)]
struct Seen {
    viewer: usize,
    /// Fighting area on screen (pixels) and its mean width (pixels).
    px: f64,
    width: f64,
    /// The fighting polygon on A (world).
    pts: Vec<V3>,
    m2: f64,
    centre: V3,
    /// View distance range over it.
    z: (f64, f64),
    /// Depth difference range and the error, at the centre.
    delta: (f64, f64),
    eps: f64,
    /// Depth units per metre of separation, at the centre.
    k: f64,
    /// Separation (m, along A's normal) range over the fight.
    sep: (f64, f64),
}

/// One fight found between two items.
#[derive(Debug, Clone)]
pub struct Found {
    pub a: (usize, usize),
    pub b: (usize, usize),
    pub overlap: f64,
    seen: Seen,
    na: V3,
    nb: V3,
    off: f64,
    /// Every viewer the fight is big enough and in sight for (indices into
    /// the detector's viewers), and the worst such view per viewer label.
    by: Vec<usize>,
    per_label: Vec<Seen>,
    /// The largest epsilon (m along the normal) over every view in `by`:
    /// what a fix must clear so it holds from every camera.
    need: f64,
}

/// The detector: the depth setup, the tolerances and the viewers.
pub struct Detector<'a> {
    pub set: DepthSetup,
    pub cfg: &'a Config,
    pub viewers: &'a [Viewer],
    views: Vec<View>,
    zcap: f64,
    cos_angle: f64,
}

impl<'a> Detector<'a> {
    pub fn new(set: DepthSetup, cfg: &'a Config, viewers: &'a [Viewer]) -> Detector<'a> {
        Detector {
            set,
            cfg,
            viewers,
            views: viewers.iter().map(View::new).collect(),
            zcap: cfg.view_distance.min(set.far),
            cos_angle: cfg.zfight_angle.to_radians().cos(),
        }
    }

    fn off(&self, it: &Item) -> f64 {
        if it.decal { self.set.decal_offset } else { 0.0 }
    }

    /// Farthest view distance of `b` over `views` (capped), or None when no
    /// viewer has it within reach.
    fn reach_z(&self, b: &Aabb, views: &[usize]) -> Option<f64> {
        let mut zmax: Option<f64> = None;
        for &k in views {
            let v = &self.views[k];
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for i in 0..8 {
                let c = [
                    if i & 1 == 0 { b.min[0] } else { b.max[0] },
                    if i & 2 == 0 { b.min[1] } else { b.max[1] },
                    if i & 4 == 0 { b.min[2] } else { b.max[2] },
                ];
                let z = dot(v.f, sub(c, v.eye));
                lo = lo.min(z);
                hi = hi.max(z);
            }
            if hi < self.set.near || lo > self.zcap {
                continue;
            }
            let z = hi.min(self.zcap).max(self.set.near);
            zmax = Some(zmax.map_or(z, |m: f64| m.max(z)));
        }
        zmax
    }

    /// Largest separation (m) at which two faces of `b`'s size could fight
    /// for any of `views`, with decal offsets differing by `doff`: a bound of
    /// `(ε + doff) / k` (see the module docs), used to find candidates.
    fn reach(&self, b: &Aabb, doff: f64, views: &[usize]) -> Option<f64> {
        let z = self.reach_z(b, views)?;
        let set = &self.set;
        let (mut cos_c, mut px, mut eye1) = (1.0f64, 0.0f64, 0.0f64);
        for &k in views {
            let v = &self.views[k];
            cos_c = cos_c.min(v.cos_corner);
            px = px.max(v.px);
            eye1 = eye1.max(v.eye[0].abs() + v.eye[1].abs() + v.eye[2].abs());
        }
        let p1 = (0..3).map(|i| b.min[i].abs().max(b.max[i].abs())).sum::<f64>();
        let s = (set.a.abs() + 1.0) * (p1 + eye1) + set.b.abs();
        let step = set.format.step(1.0).max(set.format.step(set.depth(set.near)));
        let depth_terms = ((step + doff) * z * z + 4.0 * set.unit_roundoff * s * z) / (set.b.abs() * cos_c);
        Some(depth_terms + 2.0 * 2f64.powi(-(set.subpixel_bits as i32)) * px * z)
    }

    /// The fight between faces `fa` (of item A) and `fb` (of B) seen from
    /// viewer `k`, if it is big enough to show.
    #[allow(clippy::too_many_arguments)]
    fn view_patch(&self, k: usize, p: &Patch, fa: &Face, fb: &Face, a: &Item, b: &Item) -> Option<Seen> {
        let v = &self.views[k];
        let set = &self.set;
        let pa = Plane::new(set, v, fa, self.off(a));
        let pb = Plane::new(set, v, fb, self.off(b));
        if pa.dist.abs() < 1e-9 || pb.dist.abs() < 1e-9 {
            return None;
        }
        // Against another item a closed mesh shows only the faces that face
        // the eye (its back faces lie behind its own front faces), convex or
        // not: a concave kerb piece's hidden end cap never fights its
        // neighbour's. Against itself its back face is the one that fights
        // (a thin slab's underside under its top).
        if !std::ptr::eq(a, b) && ((a.shape.closed && pa.dist >= 0.0) || (b.shape.closed && pb.dist >= 0.0)) {
            return None;
        }
        // Between the near plane and the drawn distance.
        let zc = |q: [f64; 2]| dot(v.f, sub(p.to_3d.at(q[0], q[1]), v.eye));
        let poly = clip_half(&p.poly, &|q| set.near - zc(q));
        if poly.len() < 3 {
            return None;
        }
        let poly = clip_half(&poly, &|q| zc(q) - self.zcap);
        if poly.len() < 3 {
            return None;
        }
        // Onto the screen (tangent units), cut to the frame.
        let scr: Vec<[f64; 2]> = poly
            .iter()
            .map(|q| {
                let r = sub(p.to_3d.at(q[0], q[1]), v.eye);
                let z = dot(v.f, r);
                [dot(v.s, r) / z, dot(v.u, r) / z]
            })
            .collect();
        let frame = [[-v.tx, -v.ty], [v.tx, -v.ty], [v.tx, v.ty], [-v.tx, v.ty]];
        let scr = clip_2d(&ccw(scr), &frame);
        if scr.len() < 3 {
            return None;
        }
        let delta = |q: [f64; 2]| {
            let r = v.ray(q);
            let (ia, ib) = (pa.inv_z(r), pb.inv_z(r));
            let t = set.toward();
            (set.a + set.b * ib + t * pb.off) - (set.a + set.b * ia + t * pa.off)
        };
        let eps = |q: [f64; 2]| {
            let r = v.ray(q);
            let (ia, ib) = (pa.inv_z(r), pb.inv_z(r));
            if ia <= 0.0 || ib <= 0.0 {
                return 0.0;
            }
            let (za, zb) = (1.0 / ia, 1.0 / ib);
            let (qa, qb) = (add(v.eye, mul(r, za)), add(v.eye, mul(r, zb)));
            let d = set.depth(za).abs().max(set.depth(zb).abs());
            set.format.step(d) + set.transform_error(qa, za, v.f, v.eye) + set.transform_error(qb, zb, v.f, v.eye) + pa.snap + pb.snap
        };
        let fight = clip_half(&scr, &|q| delta(q) - eps(q));
        if fight.len() < 3 {
            return None;
        }
        let fight = clip_half(&fight, &|q| -delta(q) - eps(q));
        if fight.len() < 3 {
            return None;
        }
        let px = area_2d(&fight).abs() / (v.px * v.px);
        let per = perimeter(&fight) / v.px;
        let width = if per > 0.0 { 2.0 * px / per } else { 0.0 };
        if px < self.cfg.zfight_px || width < 1.0 {
            return None;
        }
        // Back onto A, in the world.
        let on_a = |q: [f64; 2]| {
            let r = v.ray(q);
            add(v.eye, mul(r, 1.0 / pa.inv_z(r)))
        };
        let pts: Vec<V3> = fight.iter().map(|q| on_a(*q)).collect();
        let mut c2 = [0.0, 0.0];
        for q in &fight {
            c2[0] += q[0] / fight.len() as f64;
            c2[1] += q[1] / fight.len() as f64;
        }
        let centre = on_a(c2);
        let m2 = Face::from_pts(pts.clone()).map_or(0.0, |f| f.area());
        let (mut zlo, mut zhi, mut dlo, mut dhi, mut slo, mut shi) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        let cosn = dot(fa.n, fb.n);
        for (q, w) in fight.iter().zip(&pts) {
            let z = dot(v.f, sub(*w, v.eye));
            zlo = zlo.min(z);
            zhi = zhi.max(z);
            let d = delta(*q);
            dlo = dlo.min(d);
            dhi = dhi.max(d);
            let s = dot(fb.n, sub(fb.pts[0], *w)) / cosn;
            slo = slo.min(s);
            shi = shi.max(s);
        }
        let r = norm(v.ray(c2));
        let zm = dot(v.f, sub(centre, v.eye));
        let kd = set.b.abs() * dot(r, v.f) / (zm * zm * dot(fa.n, r).abs().max(1e-9));
        Some(Seen {
            viewer: k,
            px,
            width,
            pts,
            m2,
            centre,
            z: (zlo, zhi),
            delta: (dlo, dhi),
            eps: eps(c2),
            k: kd,
            sep: (slo, shi),
        })
    }
}

impl Detector<'_> {
    /// The worst visible fight between item `ia` of `scenes[sa]` and item
    /// `ib` of `scenes[sb]` over `views`, or None. Occluders are every item of
    /// `scenes` but the two.
    pub fn pair(&self, scenes: &[&Scene], (sa, ia): (usize, usize), (sb, ib): (usize, usize), views: &[usize]) -> Option<Found> {
        let a = &scenes[sa].items[ia];
        let b = &scenes[sb].items[ib];
        // One item against itself: only its faces that face opposite ways,
        // when back faces are drawn (a thin slab's top and underside).
        let own = (sa, ia) == (sb, ib);
        if own && !self.set.back_faces {
            return None;
        }
        let same_look = a.inst.material == b.inst.material && a.inst.color == b.inst.color;
        let doff = (self.off(a) - self.off(b)).abs();
        let reach = self.reach(&a.shape.aabb.union(&b.shape.aabb), doff, views)?;
        if !a.shape.aabb.overlaps(&b.shape.aabb, reach) {
            return None;
        }
        let mut cands: Vec<(Seen, usize, usize, f64)> = Vec::new();
        for (fi, fa) in a.shape.faces.iter().enumerate() {
            for (fj, fb) in b.shape.faces.iter().enumerate() {
                if own && fj <= fi {
                    continue;
                }
                // Same material and colour facing the same way: a fight
                // between them can't be seen.
                if (same_look && dot(fa.n, fb.n) > 0.0) || dot(fa.n, fb.n).abs() < self.cos_angle || !fa.bb.overlaps(&fb.bb, reach) {
                    continue;
                }
                let Some(p) = face_patch(fa, fb, reach, self.cos_angle) else { continue };
                for &k in views {
                    if let Some(s) = self.view_patch(k, &p, fa, fb, a, b) {
                        cands.push((s, fi, fj, p.overlap));
                    }
                }
            }
        }
        cands.sort_by(|x, y| y.0.px.total_cmp(&x.0.px));
        // Tried for sight: the 64 largest views (a bound on the time, not on
        // the maths), and each viewer's largest view, so every camera that
        // sees the fight is counted.
        let mut first = vec![false; self.views.len()];
        let mut tried = Vec::new();
        for (n, c) in cands.iter().enumerate() {
            if n < 64 || !first[c.0.viewer] {
                first[c.0.viewer] = true;
                tried.push(n);
            }
        }
        let mut worst: Option<usize> = None;
        let mut by = Vec::new();
        let mut per_label: Vec<Seen> = Vec::new();
        let mut need: f64 = 0.0;
        for n in tried {
            let s = &cands[n].0;
            if by.contains(&s.viewer) || !self.seen(scenes, (sa, ia), (sb, ib), s) {
                continue;
            }
            worst.get_or_insert(n);
            by.push(s.viewer);
            need = need.max(s.eps / s.k);
            let label = &self.viewers[s.viewer].label;
            if !per_label.iter().any(|p| &self.viewers[p.viewer].label == label) {
                per_label.push(s.clone());
            }
        }
        let n = worst?;
        let (s, fi, fj, overlap) = cands.swap_remove(n);
        by.sort_unstable();
        Some(Found {
            a: (sa, ia),
            b: (sb, ib),
            overlap,
            na: a.shape.faces[fi].n,
            nb: b.shape.faces[fj].n,
            off: doff,
            seen: s,
            by,
            per_label,
            need,
        })
    }

    /// Whether some point of the fight is in sight of its viewer.
    fn seen(&self, scenes: &[&Scene], a: (usize, usize), b: (usize, usize), s: &Seen) -> bool {
        let v = &self.views[s.viewer];
        let mut pts = vec![s.centre];
        pts.extend(s.pts.iter().take(6).map(|p| lerp(s.centre, *p, 0.5)));
        pts.iter().any(|p| {
            let r = sub(*p, v.eye);
            let t = len(r);
            let dir = mul(r, 1.0 / t.max(1e-12));
            let z = dot(v.f, r).max(self.set.near);
            // Within the pair's own depth stack (their separation along the
            // ray plus the decal offset in metres) a third surface is part of
            // the same stack, not an occluder.
            let along = (s.sep.0.abs().max(s.sep.1.abs()) + self.set.decal_offset * z * z / self.set.b.abs()) / dot(dir, v.f).abs().max(1e-6);
            let tmax = t - along - 1e-3;
            if tmax <= 0.0 {
                return true;
            }
            !scenes
                .iter()
                .enumerate()
                .any(|(si, sc)| sc.ray_surface(v.eye, dir, tmax, &|id| (si, id) == a || (si, id) == b).is_some())
        })
    }

    /// Candidate partners in `other` of item `it`: items near enough to
    /// fight it for any of `views`.
    fn near(&self, it: &Item, other: &Scene, views: &[usize], decals: bool) -> Vec<usize> {
        let doff = if it.decal || decals { self.set.decal_offset } else { 0.0 };
        match self.reach(&it.shape.aabb, doff, views) {
            Some(r) => other.grid.query(&it.shape.aabb.grow(r)),
            None => Vec::new(),
        }
    }

    /// Per viewer label: how many viewers see the fight, and the worst of
    /// them (its time, size, distance, epsilon and decal offset there).
    fn seen_from(&self, f: &Found) -> Value {
        let mut m = serde_json::Map::new();
        for p in &f.per_label {
            let vw = &self.viewers[p.viewer];
            let n = f.by.iter().filter(|&&k| self.viewers[k].label == vw.label).count();
            m.insert(
                vw.label.clone(),
                json!({
                    "cameras": n,
                    "worst": {
                        "viewer": p.viewer,
                        "time": vw.time,
                        "fight_px": p.px,
                        "view_distance_m": [p.z.0, p.z.1],
                        "separation_m": [p.sep.0.min(p.sep.1), p.sep.0.max(p.sep.1)],
                        "epsilon_m": p.eps / p.k,
                        "decal_offset_m": f.off / p.k,
                    },
                }),
            );
        }
        m.insert("viewers".into(), json!(f.by));
        Value::Object(m)
    }

    /// Turn a fight into an issue.
    pub fn issue(&self, scenes: &[&Scene], f: &Found) -> Issue {
        let a = &scenes[f.a.0].items[f.a.1];
        let b = &scenes[f.b.0].items[f.b.1];
        let s = &f.seen;
        let vw = &self.viewers[s.viewer];
        let la = scenes[f.a.0].objects[a.object].label.clone();
        let lb = scenes[f.b.0].objects[b.object].label.clone();
        // Coplanar: the gap is within the rounding of f32 vertex positions there.
        let pmax = s.centre.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        let coplanar_tol = 8.0 * self.set.unit_roundoff * (pmax + 1.0);
        let gap = s.sep.0.abs().min(s.sep.1.abs());
        let gap_max = s.sep.0.abs().max(s.sep.1.abs());
        let own = f.a == f.b;
        let kind = match (a.decal, b.decal) {
            _ if own => "thin slab",
            (true, true) => "decals on one plane",
            (false, false) if gap_max <= coplanar_tol => "coplanar faces",
            (false, false) => "near-coplanar faces",
            _ => "decal behind surface",
        };
        let eps_m = s.eps / s.k;
        let off_m = f.off / s.k;
        // A fix must hold from every camera that sees the fight, not only
        // from the view with the most pixels: the largest epsilon among them.
        let need_m = f.need.max(eps_m);
        let mut is = Issue::new(Check::ZFight, kind, s.centre);
        is.objects = vec![la, lb];
        is.items = vec![a.index, b.index];
        is.aabb = Some(Aabb::from_points(&s.pts).grow(0.05));
        is.measure = s.px;
        is.unit = "px".into();
        is.severity = Severity::Error;
        is.time = vw.time;
        let at = format!("{} camera{}", vw.label, vw.time.map_or(String::new(), |t| format!(" at {t:.1} s")));
        let offs = if f.off > 0.0 { format!(" (the decal depth offset is worth {} m there)", d4(off_m)) } else { String::new() };
        is.detail = if own {
            format!(
                "{} is {} to {} m thick over {:.2} m2; seen from the {at} at {:.0} to {:.0} m the depth buffer only separates surfaces {} m apart, so its underside (drawn, as the engine draws back faces, and dark, lit by its own normal facing away) shows through its top over {:.2} m2 ({:.0} px)",
                a.label,
                d4(gap),
                d4(gap_max),
                f.overlap,
                s.z.0,
                s.z.1,
                d4(eps_m),
                s.m2,
                s.px
            )
        } else {
            format!(
                "{} and {} overlap by {:.2} m2 and are {} to {} m apart there; seen from the {at} at {:.0} to {:.0} m the depth buffer only separates surfaces {} m apart{offs}, so {:.2} m2 ({:.0} px) flickers",
                a.label,
                b.label,
                f.overlap,
                d4(s.sep.0.min(s.sep.1)),
                d4(s.sep.0.max(s.sep.1)),
                s.z.0,
                s.z.1,
                d4(eps_m),
                s.m2,
                s.px
            )
        };
        is.fix = match kind {
            "thin slab" => format!(
                "Give it no underside where it lies on a surface (a one-sided top: a flat quad, or its mesh without the bottom; a decal needs none), or make it at least {need_m:.3} m thick there."
            ),
            "decals on one plane" => format!(
                "Two decals on one plane get the same depth offset and fight each other: do not overlap them (merge them into one decal or trim one), or lift one at least {need_m:.3} m above the other."
            ),
            // Far more than a decal's lift: a separate object drawn with the
            // decal offset, not a mark on that surface.
            "decal behind surface" if gap > 0.05 => format!(
                "The decal lies {gap:.3} m behind the other surface, far more than a mark's lift: it is a separate object drawn as a decal, and its depth offset ({off_m:.3} m there) pulls it through. Draw it as a normal object (RenderWorld::draw_mesh, no decal offset)."
            ),
            "decal behind surface" => format!(
                "The decal lies {gap:.3} m on the far side of its surface, where its depth offset ({off_m:.3} m there) just cancels the gap: put it on top of the surface it marks (its height from that surface, not from a smoother curve)."
            ),
            _ => format!(
                "Draw the upper one as a decal (RenderWorld::draw_decal: depth offset), remove the hidden face, merge them into one mesh, or move them at least {:.3} m apart.",
                need_m + gap_max
            ),
        };
        let c = &vw.cam;
        is.data = json!({
            "separation_m": [s.sep.0.min(s.sep.1), s.sep.0.max(s.sep.1)],
            "overlap_m2": f.overlap,
            "fight_m2": s.m2,
            "fight_px": s.px,
            "fight_width_px": s.width,
            "epsilon_m": eps_m,
            "fix_epsilon_m": need_m,
            "epsilon_depth": s.eps,
            "delta_depth": [s.delta.0, s.delta.1],
            "decal_offset_m": off_m,
            "view_distance_m": [s.z.0, s.z.1],
            "viewer": { "label": vw.label, "time": vw.time, "eye": c.eye, "target": c.target, "fov_y_deg": c.fov_y_deg, "aspect": c.aspect, "height_px": vw.height_px },
            "normals": [f.na, f.nb],
            "polygon": s.pts.iter().map(|p| p.map(|x| (x * 1e3).round() / 1e3)).collect::<Vec<_>>(),
            "decal": [a.decal, b.decal],
            "runtime": f.a.0 != 0 || f.b.0 != 0,
            "seen_from": self.seen_from(f),
            "depth": { "format": self.set.format.name(), "near": self.set.near, "far": self.set.far, "reversed": self.set.reversed },
        });
        is
    }
}

/// Four decimals, with no "-0.0000".
fn d4(x: f64) -> String {
    let s = format!("{x:.4}");
    if s == "-0.0000" { "0.0000".into() } else { s }
}

/// Static z-fighting: every pair of items of `s` over every viewer.
pub fn zfight(s: &Scene, set: DepthSetup, viewers: &[Viewer]) -> Vec<Issue> {
    let d = Detector::new(set, &s.cfg, viewers);
    let views: Vec<usize> = (0..viewers.len()).collect();
    let decals = s.items.iter().any(|i| i.decal);
    let scenes = [s];
    let mut out = Vec::new();
    for (i, it) in s.items.iter().enumerate() {
        if let Some(f) = d.pair(&scenes, (0, i), (0, i), &views) {
            out.push(d.issue(&scenes, &f));
        }
        for j in d.near(it, s, &views, decals) {
            if j <= i {
                continue;
            }
            if let Some(f) = d.pair(&scenes, (0, i), (0, j), &views) {
                out.push(d.issue(&scenes, &f));
            }
        }
    }
    out
}

/// Runtime z-fighting: the movers and runtime decals of each recorded frame
/// (`frames`: that frame's dynamic scene and its camera) against the static
/// world and each other, seen from that frame's camera only. Issues of one
/// kind and object class are merged (the worst frame wins; `data.frames`
/// counts the frames it showed in).
pub fn zfight_runtime(stat: &Scene, frames: &[(&Scene, Viewer)], set: DepthSetup) -> Vec<Issue> {
    let mut best: Vec<(Issue, usize)> = Vec::new();
    for (dy, viewer) in frames {
        let vs = std::slice::from_ref(viewer);
        let d = Detector::new(set, &stat.cfg, vs);
        let scenes = [stat, *dy];
        let decals = stat.items.iter().chain(&dy.items).any(|i| i.decal);
        let mut found = Vec::new();
        for (i, it) in dy.items.iter().enumerate() {
            if let Some(f) = d.pair(&scenes, (1, i), (1, i), &[0]) {
                found.push(d.issue(&scenes, &f));
            }
            for j in d.near(it, stat, &[0], decals) {
                if let Some(f) = d.pair(&scenes, (1, i), (0, j), &[0]) {
                    found.push(d.issue(&scenes, &f));
                }
            }
            for j in d.near(it, dy, &[0], decals) {
                if j <= i {
                    continue;
                }
                if let Some(f) = d.pair(&scenes, (1, i), (1, j), &[0]) {
                    found.push(d.issue(&scenes, &f));
                }
            }
        }
        for is in found {
            match best.iter_mut().find(|(b, _)| b.kind == is.kind && b.class() == is.class()) {
                Some((b, n)) => {
                    *n += 1;
                    if is.measure > b.measure {
                        *b = is;
                    }
                }
                None => best.push((is, 1)),
            }
        }
    }
    best.into_iter()
        .map(|(mut is, n)| {
            if let Some(o) = is.data.as_object_mut() {
                o.insert("frames".into(), Value::from(n));
                o.insert("runtime".into(), Value::Bool(true));
                // Its viewer is that frame's camera (`viewer`), not one of a list.
                o.remove("seen_from");
            }
            is
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::mesh_name;
    use crate::shape::Shapes;
    use evani_render::mesh::{Mesh, MeshId, MeshLibrary};
    use evani_render::spline::{Path, Profile, sweep};
    use evani_render::world::MeshInstance;

    fn inst(mesh: MeshId, p: [f32; 3], s: [f32; 3], color: [f32; 4]) -> MeshInstance {
        let mut m = MeshInstance::new(mesh, p);
        m.scale = s;
        m.color = color;
        m
    }

    const GREY: [f32; 4] = [0.3, 0.3, 0.3, 1.0];
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const RED: [f32; 4] = [0.9, 0.1, 0.1, 1.0];
    const BLUE: [f32; 4] = [0.1, 0.1, 0.9, 1.0];

    /// A scene whose instances from `decal_from` on are decals.
    fn scene_of(list: &[MeshInstance], shapes: &mut Shapes, decal_from: usize) -> Scene {
        let l: Vec<(usize, MeshInstance)> = list.iter().copied().enumerate().collect();
        Scene::build_with(&l, shapes, &|m| mesh_name(m.mesh), Config::default(), &|i| i >= decal_from)
    }

    fn scene(list: &[MeshInstance]) -> Scene {
        scene_of(list, &mut Shapes::new(), usize::MAX)
    }

    fn decal_scene(list: &[MeshInstance], decal_from: usize) -> Scene {
        scene_of(list, &mut Shapes::new(), decal_from)
    }

    /// A 60 degree, 16:9, 720 px high camera.
    fn viewer(eye: V3, target: V3) -> Viewer {
        let cam = Camera {
            eye,
            target,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
            aspect: 16.0 / 9.0,
        };
        Viewer::new(cam, 720.0, "test", None)
    }

    /// Looking straight down at (x, 0, z) from height `h`.
    fn above(x: f64, z: f64, h: f64) -> Viewer {
        let mut v = viewer([x, h, z], [x, 0.0, z]);
        v.cam.up = [0.0, 0.0, 1.0];
        v
    }

    fn run(s: &Scene, v: &[Viewer]) -> Vec<Issue> {
        zfight(s, DepthSetup::engine(), v)
    }

    /// The engine's setup with depth 0 at near and 1 at far (what the
    /// renderer drew before reversed-Z): the far-off cases below still need
    /// a coarse depth buffer to show a fight at all.
    fn forward_depth() -> DepthSetup {
        use evani_render::shaders::{FAR, NEAR};
        let f = DepthSetup::perspective(f64::from(NEAR), f64::from(FAR), false, DepthFormat::Depth32Float);
        DepthSetup { a: f.a, b: f.b, reversed: false, ..DepthSetup::engine() }
    }

    fn run_fwd(s: &Scene, v: &[Viewer]) -> Vec<Issue> {
        zfight(s, forward_depth(), v)
    }

    fn num(i: &Issue, k: &str) -> f64 {
        i.data[k].as_f64().unwrap_or_else(|| panic!("{k} in {}", i.data))
    }

    /// A 10 x 40 m road slab with its top at y = 0 and a 0.3 x 6 m line
    /// slab whose top is `lift` m above it.
    fn road_and_line(lift: f32, line_color: [f32; 4]) -> [MeshInstance; 2] {
        [
            inst(MeshId::CUBE, [0.0, -0.1, 0.0], [10.0, 0.2, 40.0], GREY),
            inst(MeshId::CUBE, [0.0, lift - 0.05, 0.0], [0.3, 0.1, 6.0], line_color),
        ]
    }

    /// A 10 x 10 m slab with its top at y = 0, centred `x` m from the
    /// origin, and a 4 x 4 m slab whose top is `lift` m above it, tilted
    /// `tilt_deg` about z (about its own centre).
    fn slab_pair(x: f32, lift: f32, tilt_deg: f32) -> [MeshInstance; 2] {
        let a = inst(MeshId::CUBE, [x, -0.1, 0.0], [10.0, 0.2, 10.0], GREY);
        let mut b = inst(MeshId::CUBE, [x, lift - 0.1, 0.0], [4.0, 0.2, 4.0], RED);
        let h = (tilt_deg.to_radians() * 0.5) as f64;
        b.rotation = [0.0, 0.0, h.sin() as f32, h.cos() as f32];
        [a, b]
    }

    // ------------------------------------------------ the depth maths

    #[test]
    fn engine_setup_is_the_renderers_projection() {
        let e = DepthSetup::engine();
        // The engine draws reversed depth (evani_render::depth).
        let p = DepthSetup::perspective(0.1, 1000.0, true, DepthFormat::Depth32Float);
        // The engine's matrix is f32: equal to f32 rounding.
        assert!((e.a - p.a).abs() < 1e-9 && (e.b - p.b).abs() < 1e-7, "{e:?} {p:?}");
        let (dn, df) = (f64::from(evani_render::depth::NEAR_DEPTH), f64::from(evani_render::depth::FAR_DEPTH));
        assert!((e.depth(0.1) - dn).abs() < 1e-6 && (e.depth(1000.0) - df).abs() < 1e-6);
        assert!(e.reversed && e.b > 0.0 && e.format == DepthFormat::Depth32Float);
        assert_eq!(e.reversed, evani_render::depth::REVERSED);
        assert_eq!(e.toward(), f64::from(evani_render::depth::TOWARD_CAMERA));
        assert_eq!(e.decal_offset, f64::from(evani_render::shaders::DECAL_DEPTH_OFFSET));
        let r = DepthSetup::perspective(0.1, 1000.0, true, DepthFormat::Depth32Float);
        assert!((r.depth(0.1) - 1.0).abs() < 1e-12 && r.depth(1000.0).abs() < 1e-12);
        let f = DepthSetup::perspective(0.1, 1000.0, false, DepthFormat::Depth32Float);
        assert!(f.depth(0.1).abs() < 1e-12 && (f.depth(1000.0) - 1.0).abs() < 1e-12 && f.toward() == -1.0);
    }

    /// An engine change that alters the depth maths must fail here: the
    /// camera passes are Depth32Float compared with `depth::COMPARE`
    /// (`Greater`, reversed depth) and cleared to `depth::CLEAR` (0), the
    /// only hardware depth bias is the shadow map's, and decals are pulled
    /// closer by a constant in the forward shader, signed toward the camera.
    #[test]
    fn engine_setup_matches_the_renderer() {
        let pipes = include_str!("../../../crates/evani-render/src/gpu/pipelines.rs");
        let frame = include_str!("../../../crates/evani-render/src/gpu/frame.rs");
        let forward = include_str!("../../../crates/evani-render/shaders/forward.wgsl");
        let depth = include_str!("../../../crates/evani-render/src/depth.rs");
        let shaders = include_str!("../../../crates/evani-render/src/shaders.rs");
        for src in [pipes, frame] {
            for f in ["Depth24Plus", "Depth24PlusStencil8", "Depth16Unorm", "Depth32FloatStencil8"] {
                assert!(!src.contains(&format!("TextureFormat::{f}")), "the renderer now uses {f}: update DepthSetup::engine");
            }
        }
        let create = pipes.find("fn create_pipeline(").expect("create_pipeline");
        let camera = &pipes[create..];
        let end = camera.find("\n}").expect("end of create_pipeline");
        let camera = &camera[..end];
        assert!(camera.contains("format: wgpu::TextureFormat::Depth32Float"), "camera passes: Depth32Float");
        assert!(camera.contains("depth_compare: Some(crate::depth::COMPARE)"), "camera passes: the shared depth test");
        assert!(depth.contains("pub const COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::Greater;"), "reversed depth: Greater");
        assert!(depth.contains("pub const CLEAR: f32 = FAR_DEPTH;") && depth.contains("pub const FAR_DEPTH: f32 = 0.0;"), "reversed depth: cleared to 0");
        // Every camera-pass depth clear uses the shared value; the only other
        // clear is the shadow map's (its own conventional depth).
        assert_eq!(frame.matches("LoadOp::Clear(crate::depth::CLEAR)").count(), 2, "forward pass and depth resolve clear to depth::CLEAR");
        assert_eq!(frame.matches("LoadOp::Clear(1.0)").count(), 1, "only the shadow map clears to 1");
        assert_eq!(pipes.matches("CompareFunction::Less").count(), 1, "only the shadow map compares Less");
        let particles = include_str!("../../../crates/evani-render/src/gpu/particles.rs");
        assert!(particles.contains("depth_compare: Some(crate::depth::COMPARE)") && !particles.contains("CompareFunction::Less"), "particles: the shared depth test");
        // Decals: the CPU pad is signed by depth::toward_camera.
        assert!(shaders.contains("crate::depth::toward_camera(DECAL_DEPTH_OFFSET)"), "decal pad signed toward the camera");
        assert!(camera.contains("bias: wgpu::DepthBiasState::default()"), "camera passes: no depth bias");
        assert_eq!(pipes.matches("bias: wgpu::DepthBiasState {").count(), 1, "one biased pipeline (the shadow map)");
        let shadow = &pipes[pipes.find("fn shadow_pipeline(").expect("shadow_pipeline")..create];
        assert!(shadow.contains("bias: wgpu::DepthBiasState {"), "the biased pipeline is the shadow map");
        assert!(forward.contains("o.pos.z += in.inst_scale.w * o.pos.w;"), "decals: constant (signed) depth offset in the forward shader");
        assert_eq!(forward.matches("o.pos.z").count(), 1, "no other depth change in the forward shader");
        // DepthSetup::back_faces: no face culling, back faces lit by their
        // own normal (no flip by facing).
        assert!(camera.contains("primitive: wgpu::PrimitiveState::default()"), "camera passes: default primitive state (no culling)");
        assert!(!pipes.contains("cull_mode"), "the renderer now culls faces: update DepthSetup::engine back_faces");
        assert!(!forward.contains("front_facing"), "the forward shader now lights back faces by facing: update DepthSetup::engine back_faces");
        assert!(DepthSetup::engine().back_faces);
    }

    #[test]
    fn the_threshold_follows_the_depth_maths() {
        let f32d = DepthSetup::perspective(0.1, 1000.0, false, DepthFormat::Depth32Float);
        // Grows with the view distance, about as z^2.
        let (s10, s100) = (f32d.min_separation(10.0, 10.0), f32d.min_separation(100.0, 100.0));
        assert!(s100 > 50.0 * s10 && s100 < 200.0 * s10, "{s10} {s100}");
        // A nearer near plane loses precision (b = n f / (n - f)).
        let near = DepthSetup::perspective(0.01, 1000.0, false, DepthFormat::Depth32Float);
        assert!(near.min_separation(50.0, 50.0) > 8.0 * f32d.min_separation(50.0, 50.0));
        // The far plane changes it too (a little, at this ratio).
        let far = DepthSetup::perspective(0.1, 100.0, false, DepthFormat::Depth32Float);
        assert_ne!(far.min_separation(50.0, 50.0), f32d.min_separation(50.0, 50.0));
        // Depth format: 16-bit is far coarser than 24-bit (the vertex rounding
        // is the same for both); 24-bit fixed point
        // is no finer than f32 near depth 1.
        let d24 = DepthSetup::perspective(0.1, 1000.0, false, DepthFormat::Depth24Plus);
        let d16 = DepthSetup::perspective(0.1, 1000.0, false, DepthFormat::Depth16Unorm);
        let z = 50.0;
        assert!(d16.min_separation(z, z) > 20.0 * d24.min_separation(z, z));
        assert!(d24.min_separation(z, z) >= f32d.min_separation(z, z) * 0.99);
        // Reversed f32 depth keeps precision far away.
        let rev = DepthSetup::perspective(0.1, 1000.0, true, DepthFormat::Depth32Float);
        assert!(
            rev.min_separation(z, z) < 0.01 * f32d.min_separation(z, z),
            "{} {}",
            rev.min_separation(z, z),
            f32d.min_separation(z, z)
        );
        // Far from the world origin the vertex rounding costs more.
        assert!(f32d.min_separation(z, 2000.0) > 1.5 * f32d.min_separation(z, z));
        // The f32 step: one ulp of d.
        assert_eq!(DepthFormat::Depth32Float.step(0.75), 2f64.powi(-24));
        assert_eq!(DepthFormat::Depth32Float.step(0.25), 2f64.powi(-25));
    }

    /// The kart racer's far thin parts (flag cloth 7 cm, sign chevrons 11 cm,
    /// kart wings 8 cm, a 3 cm bank step) seen from the title camera at 75 to
    /// 150 m: with depth 0 at near they needed 72 to 240 mm; the engine's
    /// reversed depth separates well under a millimetre anywhere in the world.
    #[test]
    fn reversed_depth_separates_far_thin_parts() {
        let (e, f) = (DepthSetup::engine(), forward_depth());
        for z in [75.0, 110.0, 150.0] {
            for eye in [z, 2.0 * z, 300.0] {
                let (se, sf) = (e.min_separation(z, eye), f.min_separation(z, eye));
                assert!(se < 1e-3, "{z} m from {eye}: {se}");
                assert!(sf > 0.03 && sf > 100.0 * se, "{z} m from {eye}: {sf} vs {se}");
            }
        }
        // A 3 cm step 150 m away: fights with depth 0 at near, not now.
        let mut s = scene(&slab_pair(0.0, 0.03, 0.0));
        s.cfg.view_distance = 1000.0;
        let v = [above(0.0, 0.0, 150.0)];
        assert!(run_fwd(&s, &v).iter().any(|i| i.kind == "near-coplanar faces"));
        assert!(run(&s, &v).is_empty(), "{:?}", run(&s, &v));
        // A real coplanar overlap is still caught at any distance.
        let flush = scene(&slab_pair(0.0, 0.0, 0.0));
        for h in [5.0, 40.0, 150.0] {
            let z = run(&flush, &[above(0.0, 0.0, h)]);
            assert!(z.iter().any(|i| i.kind == "coplanar faces"), "{h}: {z:?}");
        }
    }

    // ------------------------------------------------ pairs

    #[test]
    fn coplanar_overlapping_quads_fight_unless_identical_material() {
        let v = [viewer([0.0, 6.0, -15.0], [0.0, 0.0, 0.0])];
        let z = run(&scene(&road_and_line(0.0, WHITE)), &v);
        assert_eq!(z.len(), 1, "{z:?}");
        let i = &z[0];
        assert_eq!(i.kind, "coplanar faces");
        assert_eq!(i.check, Check::ZFight);
        assert!((num(i, "overlap_m2") - 1.8).abs() < 1e-6, "{}", i.data);
        assert!(num(i, "epsilon_m") > 0.0 && num(i, "fight_px") >= 4.0);
        assert!(i.data["viewer"]["label"] == "test" && i.data["runtime"] == false);
        // Every camera that sees it is counted, with the worst per label.
        let two = [v[0].clone(), viewer([0.0, 4.0, 12.0], [0.0, 0.0, 0.0]), viewer([0.0, 6.0, 200.0], [0.0, 6.0, 300.0])];
        let z2 = run(&scene(&road_and_line(0.0, WHITE)), &two);
        let from = &z2[0].data["seen_from"];
        assert_eq!(from["test"]["cameras"], 2, "{from}");
        assert_eq!(from["viewers"], json!([0, 1]), "{from}");
        assert!(from["test"]["worst"]["epsilon_m"].as_f64().is_some_and(|e| e > 0.0), "{from}");
        // The same look on both: a fight can't show.
        assert!(run(&scene(&road_and_line(0.0, GREY)), &v).is_empty());
        // No viewer: nothing is seen.
        assert!(run(&scene(&road_and_line(0.0, WHITE)), &[]).is_empty());
    }

    #[test]
    fn separation_is_judged_against_the_depth_epsilon_at_the_view_distance() {
        let set = DepthSetup::engine();
        let h = 20.0;
        let v = [above(0.0, 0.0, h)];
        // Head-on there is no slope term: the gap needed is min_separation.
        let g = set.min_separation(h, h);
        assert!(g > set.format.step(set.depth(h)) * h * h / set.b.abs() && g < 1e-2, "{g}");
        let under = run(&scene(&slab_pair(0.0, (0.5 * g) as f32, 0.0)), &v);
        let hit = under.iter().find(|i| i.kind == "near-coplanar faces").unwrap_or_else(|| panic!("{under:?}"));
        assert!((num(hit, "epsilon_m") - g).abs() < 0.05 * g, "{} vs {g}", hit.data);
        assert!((num(hit, "fight_m2") - 16.0).abs() < 0.01, "{}", hit.data);
        // Just under the epsilon still fights.
        let just = run(&scene(&slab_pair(0.0, (0.9 * g) as f32, 0.0)), &v);
        assert!(just.iter().any(|i| i.kind == "near-coplanar faces"), "{just:?}");
        let over = run(&scene(&slab_pair(0.0, (2.0 * g) as f32, 0.0)), &v);
        assert!(over.is_empty(), "{over:?}");
        // The same 2g gap seen from 4x as far (16x the epsilon) fights.
        let far = run(&scene(&slab_pair(0.0, (2.0 * g) as f32, 0.0)), &[above(0.0, 0.0, 4.0 * h)]);
        assert!(far.iter().any(|i| i.kind == "near-coplanar faces"), "{far:?}");
        // A centimetre is plenty at 20 m.
        assert!(run(&scene(&slab_pair(0.0, 0.01, 0.0)), &v).is_empty());
    }

    #[test]
    fn faces_that_only_share_an_edge_and_one_meshs_triangles_do_not_fight() {
        // Two slabs side by side: their tops share an edge (no area), their
        // touching sides face each other (each hidden behind the other).
        let a = inst(MeshId::CUBE, [-0.5, -0.1, 0.0], [1.0, 0.2, 1.0], RED);
        let b = inst(MeshId::CUBE, [0.5, -0.1, 0.0], [1.0, 0.2, 1.0], BLUE);
        let vs = [viewer([0.0, 3.0, -4.0], [0.0, 0.0, 0.0]), viewer([3.0, 0.5, -1.0], [0.0, 0.0, 0.0])];
        let z = run(&scene(&[a, b]), &vs);
        assert!(z.is_empty(), "{z:?}");
        // One swept road: every triangle is coplanar with its neighbours.
        let arc: Vec<[f32; 3]> = (0..16)
            .map(|i| {
                let t = i as f32 / 16.0 * core::f32::consts::TAU;
                [40.0 * t.cos(), 0.0, 40.0 * t.sin()]
            })
            .collect();
        let path = Path::new(arc, true);
        let m: Mesh = sweep(&path.frames(2.0), &Profile::slab(7.0, 0.3), true);
        let mut lib = MeshLibrary::default();
        let id = lib.add(m).expect("room in the library");
        let mut sh = Shapes::new();
        sh.add_library(&lib);
        let s = scene_of(&[MeshInstance::new(id, [0.0; 3])], &mut sh, usize::MAX);
        assert_eq!(s.items.len(), 1);
        assert!(run(&s, &[viewer([0.0, 30.0, -80.0], [0.0, 0.0, 0.0])]).is_empty(), "a mesh does not fight itself");
    }

    #[test]
    fn a_thin_slabs_underside_fights_its_top_from_far_off() {
        // A 2 x 2 m patch 1 mm thick lying on a road (a decal) and a 1 m box.
        let road = inst(MeshId::CUBE, [0.0, -0.1, 0.0], [10.0, 0.2, 40.0], GREY);
        let patch = inst(MeshId::CUBE, [0.0, 0.0005, 0.0], [2.0, 0.001, 2.0], RED);
        let boxy = inst(MeshId::CUBE, [3.0, 0.5, 5.0], [1.0, 1.0, 1.0], BLUE);
        let s = decal_scene(&[road, boxy, patch], 2);
        let far = viewer([0.0, 50.0, -90.0], [0.0, 0.0, 0.0]);
        // The engine's reversed depth separates 1 mm even from 100 m off.
        assert!(run(&s, std::slice::from_ref(&far)).iter().all(|i| i.kind != "thin slab"));
        // With depth 0 at near the underside shows through.
        let z = run_fwd(&s, &[far]);
        let thin: Vec<_> = z.iter().filter(|i| i.kind == "thin slab").collect();
        assert_eq!(thin.len(), 1, "{z:?}");
        assert_eq!(thin[0].items, vec![2, 2], "the patch against itself");
        let (eps, sep) = (num(thin[0], "epsilon_m"), thin[0].data["separation_m"][1].as_f64().unwrap_or(0.0));
        assert!(sep < 0.0011 && eps > sep, "1 mm thick under an epsilon of {eps}");
        // Close by the depth buffer separates 1 mm: no fight.
        assert!(run_fwd(&s, &[viewer([0.0, 2.0, -3.0], [0.0, 0.0, 0.0])]).iter().all(|i| i.kind != "thin slab"));
        // Thick slabs (the road, the box) never fight themselves at 100 m.
        assert!(z.iter().all(|i| i.items != vec![0, 0] && i.items != vec![1, 1]), "{z:?}");
        // With back faces culled the underside is never drawn.
        let mut set = forward_depth();
        set.back_faces = false;
        assert!(zfight(&s, set, &[viewer([0.0, 50.0, -90.0], [0.0, 0.0, 0.0])]).iter().all(|i| i.kind != "thin slab"));
    }

    #[test]
    fn a_fix_clears_the_farthest_camera_not_only_the_largest_view() {
        let road = inst(MeshId::CUBE, [0.0, -0.1, 0.0], [10.0, 0.2, 40.0], GREY);
        let patch = inst(MeshId::CUBE, [0.0, 0.0005, 0.0], [2.0, 0.001, 2.0], RED);
        let s = decal_scene(&[road, patch], 1);
        // A mid camera sees the patch biggest; a far one needs more separation.
        let views = [viewer([0.0, 20.0, -35.0], [0.0, 0.0, 0.0]), viewer([0.0, 60.0, -110.0], [0.0, 0.0, 0.0])];
        assert!(run(&s, &views).iter().all(|i| i.kind != "thin slab"), "reversed depth separates it");
        let z = run_fwd(&s, &views);
        let thin = z.iter().find(|i| i.kind == "thin slab");
        let Some(thin) = thin else { panic!("no thin slab: {z:?}") };
        let (eps, need) = (num(thin, "epsilon_m"), num(thin, "fix_epsilon_m"));
        let far = run_fwd(&s, &views[1..]);
        let far_eps = far.iter().find(|i| i.kind == "thin slab").map_or(0.0, |i| num(i, "epsilon_m"));
        assert!(far_eps > eps, "the far view needs more: {far_eps} vs {eps}");
        assert!((need - far_eps).abs() < 1e-9, "the fix clears the far view: {need} vs {far_eps}");
        assert!(thin.fix.contains(&format!("{need:.3} m thick")), "{}", thin.fix);
        // And the report never prints a minus zero.
        assert_eq!(d4(-0.00001), "0.0000");
    }

    /// Two closed but concave meshes (each two separate boxes in one mesh,
    /// like a run of kerb pieces) that touch end to end: their touching caps
    /// face each other inside the pair, each behind its own mesh's front
    /// faces, so they can't show. The same pair with one mesh left open
    /// still fights: its back faces can be seen through the hole.
    #[test]
    fn touching_closed_concave_meshes_do_not_fight_but_open_ones_do() {
        let cube = evani_render::mesh::builtin(MeshId::CUBE).expect("the cube");
        // Unit boxes centred at the given x (y 0..1), in one mesh.
        let pair = |xs: [f32; 2], open: bool| {
            let mut m = Mesh::default();
            for (k, x) in xs.into_iter().enumerate() {
                let base = m.vertices.len() as u32;
                m.vertices.extend(cube.vertices.iter().map(|v| [v[0] + x, v[1] + 0.5, v[2], v[3], v[4], v[5]]));
                // The open one misses one face of its far box.
                let skip = if open && k == 0 { 6 } else { 0 };
                m.indices.extend(cube.indices.iter().skip(skip).map(|i| i + base));
            }
            m
        };
        let check = |open: bool| {
            let mut lib = MeshLibrary::default();
            let a = lib.add(pair([-2.0, -0.5], open)).expect("room");
            let b = lib.add(pair([0.5, 2.0], false)).expect("room");
            let mut sh = Shapes::new();
            sh.add_library(&lib);
            let list = [inst(a, [0.0; 3], [1.0; 3], RED), inst(b, [0.0; 3], [1.0; 3], BLUE)];
            let s = scene_of(&list, &mut sh, usize::MAX);
            assert!(s.items.iter().all(|i| !i.shape.convex && !i.shape.solid), "both are concave surfaces, not solids");
            run(&s, &[viewer([-6.0, 3.0, -5.0], [0.0, 0.5, 0.0]), viewer([6.0, 3.0, -5.0], [0.0, 0.5, 0.0])])
        };
        let closed = check(false);
        assert!(closed.is_empty(), "{closed:?}");
        let open = check(true);
        assert!(open.iter().any(|i| i.kind == "coplanar faces"), "{open:?}");
    }

    #[test]
    fn hidden_coplanar_patches_do_not_fight() {
        let ground = inst(MeshId::CUBE, [0.0, -0.5, 0.0], [100.0, 1.0, 100.0], GREY);
        // Two boxes whose bottoms share 0.35 m2 of the ground plane.
        let a = inst(MeshId::CUBE, [0.0, 0.5, 0.0], [1.0, 1.0, 1.0], RED);
        let b = inst(MeshId::CUBE, [0.5, 0.75, 0.3], [1.0, 1.5, 1.0], BLUE);
        let top = [viewer([3.0, 4.0, -5.0], [0.25, 0.5, 0.15])];
        assert!(run(&scene(&[ground, a, b]), &top).is_empty());
        // Up in the air and seen from below, the bottoms show, and fight.
        let (mut ua, mut ub) = (a, b);
        ua.position[1] += 3.0;
        ub.position[1] += 3.0;
        let below = [viewer([2.0, 1.0, -5.0], [0.25, 3.0, 0.15])];
        let z = run(&scene(&[ground, ua, ub]), &below);
        assert!(z.iter().any(|i| i.kind == "coplanar faces" && (num(i, "overlap_m2") - 0.35).abs() < 1e-6), "{z:?}");
        // From above they hide their own bottoms.
        assert!(run(&scene(&[ground, ua, ub]), &top).is_empty());
        // Inside a bigger block: nobody sees it.
        let block = inst(MeshId::CUBE, [0.0, 3.5, 0.0], [4.0, 4.0, 4.0], WHITE);
        assert!(run(&scene(&[ground, ua, ub, block]), &below).is_empty());
    }

    #[test]
    fn coplanar_faces_far_from_the_origin_still_fight() {
        for x in [900.0f32, -650.0] {
            let v = [viewer([f64::from(x), 8.0, -20.0], [f64::from(x), 0.0, 0.0])];
            let z = run(&scene(&slab_pair(x, 0.0, 0.0)), &v);
            assert!(z.iter().any(|i| i.kind == "coplanar faces" && (num(i, "overlap_m2") - 16.0).abs() < 1e-3), "{x}: {z:?}");
        }
    }

    #[test]
    fn gaps_far_from_the_origin_are_measured_where_the_faces_overlap() {
        // 140 m out with a 0.03 degree tilt, plane offsets from the origin
        // are off by 0.073 m; the gap is measured where the faces overlap.
        let s = scene(&slab_pair(140.0, 0.01, 0.03));
        let top = |s: &Scene, k: usize| s.items[k].shape.faces.iter().find(|f| f.n[1] > 0.99).cloned().expect("a top");
        let cos = 10f64.to_radians().cos();
        let p = face_patch(&top(&s, 0), &top(&s, 1), 0.05, cos).expect("they overlap");
        // Tilt 0.03 degrees across 4 m: the gap runs 0.01 -/+ 0.001 m.
        assert!((p.sep_min - 0.009).abs() < 2e-4 && (p.sep_max - 0.011).abs() < 2e-4, "{} {}", p.sep_min, p.sep_max);
        assert!((p.area - 16.0).abs() < 0.01 && (p.overlap - 16.0).abs() < 0.01, "{}", p.area);
        let s0 = scene(&slab_pair(0.0, 0.01, 0.03));
        let p0 = face_patch(&top(&s0, 0), &top(&s0, 1), 0.05, cos).expect("they overlap");
        assert!((p0.sep_min - p.sep_min).abs() < 1e-5 && (p0.sep_max - p.sep_max).abs() < 1e-5);
        // From 30 m above, 1 cm is separable there; with depth 0 at near,
        // from 400 m it is not. The engine's reversed depth separates it.
        assert!(run_fwd(&s, &[above(140.0, 0.0, 30.0)]).is_empty());
        let mut far = scene(&slab_pair(140.0, 0.01, 0.03));
        far.cfg.view_distance = 1000.0;
        let z = run_fwd(&far, &[above(140.0, 0.0, 400.0)]);
        assert!(z.iter().any(|i| i.kind == "near-coplanar faces"), "{z:?}");
        assert!(run(&far, &[above(140.0, 0.0, 400.0)]).is_empty());
    }

    #[test]
    fn crossing_faces_fight_only_where_they_are_close() {
        // The small slab tilted 1 degree through the big one's top: they
        // cross along x = 140; only a strip along the crossing fights.
        let s = scene(&slab_pair(140.0, 0.0, 1.0));
        let top = |k: usize| s.items[k].shape.faces.iter().find(|f| f.n[1] > 0.99).cloned().expect("a top");
        let p = face_patch(&top(0), &top(1), 0.05, 10f64.to_radians().cos()).expect("they cross");
        assert!(p.sep_min < 0.0 && p.sep_max > 0.0, "{} {}", p.sep_min, p.sep_max);
        assert!((p.sep_max - p.sep_min - 4.0 * 1f64.to_radians().tan()).abs() < 2e-3, "{} {}", p.sep_min, p.sep_max);
        // (Depth 0 at near: a strip wide enough to measure.)
        let z = run_fwd(&s, &[above(140.0, 0.0, 20.0)]);
        let hit = z.iter().find(|i| i.kind == "near-coplanar faces").unwrap_or_else(|| panic!("{z:?}"));
        // About epsilon / tan(1 deg) either side of the crossing, 4 m long.
        let eps = num(hit, "epsilon_m");
        let want = 2.0 * eps / 1f64.to_radians().tan() * 4.0;
        let got = num(hit, "fight_m2");
        assert!(got < 2.0 && (got - want).abs() < 0.1 * want, "{got} want {want}: {}", hit.data);
    }

    // ------------------------------------------------ decals

    #[test]
    fn decals_on_their_surface_do_not_fight_it() {
        let v = [viewer([0.0, 6.0, -15.0], [0.0, 0.0, 0.0]), above(0.0, 0.0, 60.0)];
        // Flush, 1 cm proud, and a flat plane decal on the road.
        for lift in [0.0f32, 0.01] {
            let z = run(&decal_scene(&road_and_line(lift, WHITE), 1), &v);
            assert!(z.is_empty(), "lift {lift}: {z:?}");
        }
        let road = road_and_line(0.0, WHITE)[0];
        let paint = inst(MeshId::PLANE, [0.0, 0.0, 5.0], [2.0, 1.0, 2.0], RED);
        assert!(run(&decal_scene(&[road, paint], 1), &v).is_empty());
    }

    #[test]
    fn a_decal_behind_its_surface_fights_where_its_offset_cancels_the_gap() {
        let set = DepthSetup::engine();
        let gap = 0.01;
        // Head-on the offset is worth off * z^2 / |b| metres.
        let z0 = (gap * set.b.abs() / set.decal_offset).sqrt();
        assert!((z0 - 15.8).abs() < 0.1, "{z0}");
        let s = decal_scene(&road_and_line(-gap as f32, WHITE), 1);
        let z = run(&s, &[above(0.0, 0.0, z0)]);
        let hit = z.iter().find(|i| i.kind == "decal behind surface").unwrap_or_else(|| panic!("{z:?}"));
        assert!((num(hit, "decal_offset_m") - gap).abs() < 0.001, "{}", hit.data);
        assert!(hit.data["decal"] == json!([false, true]) || hit.data["decal"] == json!([true, false]));
        // Nearer, it is hidden under the road; farther, it shows on top.
        assert!(run(&s, &[above(0.0, 0.0, 0.6 * z0)]).is_empty());
        assert!(run(&s, &[above(0.0, 0.0, 1.6 * z0)]).is_empty());
    }

    #[test]
    fn overlapping_decals_on_one_plane_fight_each_other() {
        let [road, a] = road_and_line(0.0, WHITE);
        let b = inst(MeshId::CUBE, [0.1, -0.05, 1.0], [0.3, 0.1, 6.0], RED);
        let z = run(&decal_scene(&[road, a, b], 1), &[viewer([0.0, 6.0, -15.0], [0.0, 0.0, 0.0])]);
        assert!(z.iter().any(|i| i.kind == "decals on one plane"), "{z:?}");
        assert!(z.iter().all(|i| i.kind == "decals on one plane"), "{z:?}");
    }

    #[test]
    fn runtime_marks_are_checked_from_their_frames_camera() {
        let [road, line] = road_and_line(0.0, WHITE);
        let stat = scene(&[road]);
        // A runtime mark drawn as a plain mesh (no offset) on the road, in
        // two frames.
        let dy = scene(&[line]);
        let mut v1 = viewer([0.0, 6.0, -15.0], [0.0, 0.0, 0.0]);
        v1.time = Some(1.0);
        let mut v2 = viewer([0.0, 6.0, -10.0], [0.0, 0.0, 0.0]);
        v2.time = Some(2.0);
        let z = zfight_runtime(&stat, &[(&dy, v1.clone()), (&dy, v2)], DepthSetup::engine());
        assert_eq!(z.len(), 1, "{z:?}");
        assert_eq!(z[0].data["frames"], 2);
        assert_eq!(z[0].data["runtime"], true);
        // Drawn as a decal it does not fight.
        let dd = decal_scene(&[line], 0);
        assert!(zfight_runtime(&stat, &[(&dd, v1)], DepthSetup::engine()).is_empty());
    }
}
