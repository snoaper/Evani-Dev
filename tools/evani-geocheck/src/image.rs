//! Render-based checks on captured frames: gaps (the sky, painted magenta,
//! seen where geometry should be solid) and z-fighting flicker (the same
//! view rendered with sub-pixel camera jitter). Plus the camera maths the
//! crops need: pixel rays and projection, with the renderer's conventions
//! (`math::look_at`, `math::perspective`, NEAR 0.1).

use crate::geom::*;
use crate::scene::Scene;

/// The renderer's near plane (evani-render `shaders::NEAR`).
pub const NEAR: f64 = 0.1;
/// The renderer's far plane (evani-render `shaders::FAR`).
pub const FAR: f64 = 1000.0;

/// A camera as the renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: V3,
    pub target: V3,
    pub up: V3,
    pub fov_y_deg: f64,
    /// Width / height.
    pub aspect: f64,
}

impl Camera {
    pub fn from_pose(p: &evani_core::camera::CameraPose, aspect: f64) -> Camera {
        Camera {
            eye: v3(p.eye),
            target: v3(p.target),
            up: v3(p.up),
            fov_y_deg: f64::from(p.fov_y_deg),
            aspect,
        }
    }

    /// Forward, right and up unit vectors (look_at's f, s, u).
    pub fn basis(&self) -> (V3, V3, V3) {
        let f = norm(sub(self.target, self.eye));
        let s = norm(cross(f, self.up));
        let u = cross(s, f);
        (f, s, u)
    }

    fn tan_half(&self) -> f64 {
        (self.fov_y_deg.to_radians() * 0.5).tan()
    }

    /// The view ray through pixel (x, y) (pixel centres at +0.5) of a w x h
    /// image: origin (the eye) and unit direction.
    pub fn pixel_ray(&self, w: u32, h: u32, x: f64, y: f64) -> (V3, V3) {
        let (f, s, u) = self.basis();
        let t = self.tan_half();
        let nx = 2.0 * (x + 0.5) / f64::from(w) - 1.0;
        let ny = 1.0 - 2.0 * (y + 0.5) / f64::from(h);
        let d = add(f, add(mul(s, nx * t * self.aspect), mul(u, ny * t)));
        (self.eye, norm(d))
    }

    /// Pixel position of world point `p` in a w x h image (`None` behind
    /// the near plane).
    pub fn project(&self, w: u32, h: u32, p: V3) -> Option<[f64; 2]> {
        let (f, s, u) = self.basis();
        let r = sub(p, self.eye);
        let z = dot(r, f);
        if z < NEAR {
            return None;
        }
        let t = self.tan_half();
        let nx = dot(r, s) / (z * t * self.aspect);
        let ny = dot(r, u) / (z * t);
        Some([(nx + 1.0) * 0.5 * f64::from(w) - 0.5, (1.0 - ny) * 0.5 * f64::from(h) - 0.5])
    }

    /// Pixel box of a world box (clamped to the image), `None` when it is
    /// wholly behind the camera or off screen.
    pub fn project_box(&self, w: u32, h: u32, b: &Aabb) -> Option<[u32; 4]> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        let mut any = false;
        for i in 0..8 {
            let c = [
                if i & 1 == 0 { b.min[0] } else { b.max[0] },
                if i & 2 == 0 { b.min[1] } else { b.max[1] },
                if i & 4 == 0 { b.min[2] } else { b.max[2] },
            ];
            if let Some(p) = self.project(w, h, c) {
                any = true;
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if !any || hi[0] < 0.0 || hi[1] < 0.0 || lo[0] >= f64::from(w) || lo[1] >= f64::from(h) {
            return None;
        }
        let x0 = lo[0].max(0.0).floor() as u32;
        let y0 = lo[1].max(0.0).floor() as u32;
        let x1 = (hi[0].ceil() as u32).min(w - 1);
        let y1 = (hi[1].ceil() as u32).min(h - 1);
        Some([x0, y0, (x1 - x0 + 1).max(1), (y1 - y0 + 1).max(1)])
    }

    /// A camera looking at `b` from `dir` (unit, from the target towards the
    /// eye), far enough to frame it.
    pub fn framing(b: &Aabb, dir: V3, fov_y_deg: f64, aspect: f64) -> Camera {
        let c = b.center();
        let r = (len(b.extent()) * 0.5).max(0.5);
        let t = (fov_y_deg.to_radians() * 0.5).tan();
        let d = (r / t) * 1.6 + 1.0;
        Camera {
            eye: add(c, mul(norm(dir), d)),
            target: c,
            up: [0.0, 1.0, 0.0],
            fov_y_deg,
            aspect,
        }
    }
}

/// An RGBA8 image.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image {
            w,
            h,
            data: vec![0; (w * h * 4) as usize],
        }
    }

    pub fn px(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    pub fn set(&mut self, x: u32, y: u32, c: [u8; 4]) {
        let i = ((y * self.w + x) * 4) as usize;
        self.data[i..i + 4].copy_from_slice(&c);
    }

    /// A copy of `rect` (x, y, w, h; clamped) grown by `pad` pixels, with
    /// the original rect outlined in `color` (2 px).
    pub fn crop_outlined(&self, rect: [u32; 4], pad: u32, color: [u8; 3]) -> Image {
        let x0 = rect[0].saturating_sub(pad);
        let y0 = rect[1].saturating_sub(pad);
        let x1 = (rect[0] + rect[2] + pad).min(self.w);
        let y1 = (rect[1] + rect[3] + pad).min(self.h);
        let mut out = Image::new(x1 - x0, y1 - y0);
        for y in y0..y1 {
            for x in x0..x1 {
                out.set(x - x0, y - y0, self.px(x, y));
            }
        }
        let (rx0, ry0) = (rect[0].max(x0) - x0, rect[1].max(y0) - y0);
        let rx1 = (rect[0] + rect[2]).min(x1) - x0;
        let ry1 = (rect[1] + rect[3]).min(y1) - y0;
        let c = [color[0], color[1], color[2], 255];
        for k in 0..2u32 {
            for x in rx0..rx1 {
                for y in [ry0 + k, ry1.saturating_sub(1 + k)] {
                    if y < out.h && x < out.w {
                        out.set(x, y, c);
                    }
                }
            }
            for y in ry0..ry1 {
                for x in [rx0 + k, rx1.saturating_sub(1 + k)] {
                    if y < out.h && x < out.w {
                        out.set(x, y, c);
                    }
                }
            }
        }
        out
    }
}

/// A connected set of flagged pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Blob {
    pub pixels: usize,
    /// x, y, w, h.
    pub rect: [u32; 4],
    /// Touches the image border.
    pub border: bool,
    /// Pixel centroid.
    pub center: [f64; 2],
    /// Pixels that passed the per-pixel test (e.g. ray hits the ground).
    pub hits: usize,
    /// Box (x, y, w, h) and centroid of the pixels that passed the test;
    /// the whole blob's when none did.
    pub hit_rect: [u32; 4],
    pub hit_center: [f64; 2],
}

/// Box and centroid of a set of pixel indices in a `w`-wide image.
fn rect_of(idx: &[u32], w: u32) -> Option<([u32; 4], [f64; 2])> {
    if idx.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    let (mut sx, mut sy) = (0.0, 0.0);
    for &i in idx {
        let (x, y) = (i % w, i / w);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
        sx += f64::from(x);
        sy += f64::from(y);
    }
    let n = idx.len() as f64;
    Some(([x0, y0, x1 - x0 + 1, y1 - y0 + 1], [sx / n, sy / n]))
}

/// 4-connected components of `mask` (w x h).
pub fn components(mask: &[bool], w: u32, h: u32) -> Vec<(Blob, Vec<u32>)> {
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start as u32);
        let mut members = Vec::new();
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        let (mut sx, mut sy) = (0.0, 0.0);
        let mut border = false;
        while let Some(i) = stack.pop() {
            members.push(i);
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            sx += f64::from(x);
            sy += f64::from(y);
            border |= x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let mut push = |j: u32| {
                let j = j as usize;
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j as u32);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        let n = members.len();
        let rect = [x0, y0, x1 - x0 + 1, y1 - y0 + 1];
        let center = [sx / n as f64, sy / n as f64];
        out.push((
            Blob {
                pixels: n,
                rect,
                border,
                center,
                hits: 0,
                hit_rect: rect,
                hit_center: center,
            },
            members,
        ));
    }
    out
}

/// Gap search settings.
#[derive(Debug, Clone, Copy)]
pub struct GapParams {
    /// Sky colour of the gap captures (magenta).
    pub sky: [u8; 3],
    /// Per-channel tolerance for "is the sky".
    pub tol: u8,
    /// Enclosed sky islands up to this many pixels are reported as cracks
    /// (rather than holes).
    pub island_max: usize,
    /// Smallest blob worth reporting.
    pub min_pixels: usize,
    /// Ground hits deeper than this (view depth, metres) are past what the
    /// renderer draws: sky there is right. Keep it a little under `FAR`.
    pub far: f64,
    /// A pixel counts only if the ray this many pixels higher in the image
    /// also lands on the ground: near the horizon a pixel or two of pose or
    /// rounding error moves the hit by hundreds of metres.
    pub slack_px: f64,
}

impl Default for GapParams {
    fn default() -> Self {
        Self {
            sky: [255, 0, 255],
            tol: 40,
            island_max: 600,
            min_pixels: 3,
            far: 0.95 * FAR,
            slack_px: 2.0,
        }
    }
}

/// Sky seen where geometry should be solid. A sky pixel is wrong when its
/// view ray points down onto the world's ground footprint (`ground_y`,
/// `footprint` in XZ) within the drawn distance (`p.far`), and still does
/// `p.slack_px` pixels higher (so horizon slivers don't count): the ground
/// should be there. A blob is reported when at least `p.min_pixels` of its
/// pixels are wrong; one that doesn't touch the border and is at most
/// `p.island_max` pixels is a crack. Sky at or above the horizon, or past
/// the ground's edge (seen between trees and signs), is never a hole.
/// Returns blobs with `hits`, `hit_rect` and `hit_center` set from the
/// wrong pixels.
///
/// `alt` is the same view captured with a second sky colour (and that
/// colour): a pixel is sky only when it follows the sky in both, so a bright
/// object that happens to be the sky's colour is not a hole.
pub fn gaps(img: &Image, alt: Option<(&Image, [u8; 3])>, cam: &Camera, ground_y: f64, footprint: &Aabb, p: &GapParams) -> Vec<Blob> {
    let alt = alt.filter(|(im, _)| im.w == img.w && im.h == img.h);
    let mask: Vec<bool> = img
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(i, c)| {
            is_sky(*c, p.sky, p.tol)
                && alt.is_none_or(|(im, s2)| {
                    let k = i * 4;
                    is_sky([im.data[k], im.data[k + 1], im.data[k + 2], 255], s2, p.tol)
                })
        })
        .collect();
    let (f, _, _) = cam.basis();
    // Where the ray through (x, y) meets the ground within the drawn depth.
    let land = |x: f64, y: f64| {
        let (o, d) = cam.pixel_ray(img.w, img.h, x, y);
        ground_hit(o, d, ground_y).filter(|q| dot(sub(*q, o), f) <= p.far)
    };
    let inside = |q: V3| q[0] >= footprint.min[0] && q[0] <= footprint.max[0] && q[2] >= footprint.min[2] && q[2] <= footprint.max[2];
    let mut out = Vec::new();
    for (mut b, members) in components(&mask, img.w, img.h) {
        if b.pixels < p.min_pixels {
            continue;
        }
        let mut hit_idx = Vec::new();
        for &i in &members {
            let (x, y) = (f64::from(i % img.w), f64::from(i / img.w));
            if let (Some(q), Some(q2)) = (land(x, y), land(x, y - p.slack_px))
                && inside(q)
                && inside(q2)
            {
                hit_idx.push(i);
            }
        }
        b.hits = hit_idx.len();
        if let Some((r, c)) = rect_of(&hit_idx, img.w) {
            b.hit_rect = r;
            b.hit_center = c;
        }
        if b.hits >= p.min_pixels {
            out.push(b);
        }
    }
    out
}

/// Pixel `c` is the sky colour `sky` within `tol` per channel.
pub fn is_sky(c: [u8; 4], sky: [u8; 3], tol: u8) -> bool {
    (0..3).all(|k| c[k].abs_diff(sky[k]) <= tol)
}

/// Where the ray meets the plane y = `ground_y` (looking down only).
pub fn ground_hit(o: V3, d: V3, ground_y: f64) -> Option<V3> {
    if d[1] >= -1e-6 || o[1] <= ground_y {
        return None;
    }
    let t = (ground_y - o[1]) / d[1];
    Some(add(o, mul(d, t)))
}

/// Flicker settings.
#[derive(Debug, Clone, Copy)]
pub struct FlickerParams {
    /// Colour change (max channel) that is not explained by the jitter.
    pub threshold: u8,
    /// Flat interior: neighbours in A differ by at most this.
    pub flat: u8,
    pub min_pixels: usize,
}

impl Default for FlickerParams {
    fn default() -> Self {
        Self {
            threshold: 40,
            flat: 24,
            min_pixels: 4,
        }
    }
}

fn cdiff(a: [u8; 4], b: [u8; 4]) -> u8 {
    a[0].abs_diff(b[0]).max(a[1].abs_diff(b[1])).max(a[2].abs_diff(b[2]))
}

/// Z-fighting flicker from three captures of the same frame: `a` at jitter
/// (0, 0), `b` shifted one pixel right (jitter (1, 0)) and `c` half a pixel
/// (jitter (0.5, 0)). Plain geometry moves with the jitter: b(x) = a(x-1)
/// and c(x) is about the mean of a(x) and a(x-1). Where the depth test is
/// undecided the winning surface changes instead. Only flat interiors of
/// `a` count (edges legitimately change under jitter).
pub fn flicker(a: &Image, b: &Image, c: &Image, p: &FlickerParams) -> Vec<Blob> {
    flicker_where(a, b, c, p, &|_, _| false)
}

/// [`flicker`], dropping the changed pixels `outline(x, y)` says lie on an
/// object's outline (see [`Outlines`]): there the coverage changes under
/// jitter (aliasing), which is not z-fighting. Applied pixel by pixel before
/// the blobs are formed and counted.
pub fn flicker_where(a: &Image, b: &Image, c: &Image, p: &FlickerParams, outline: &dyn Fn(u32, u32) -> bool) -> Vec<Blob> {
    let (w, h) = (a.w, a.h);
    let mut mask = vec![false; (w * h) as usize];
    if b.w != w || b.h != h || c.w != w || c.h != h || w < 4 || h < 3 {
        return Vec::new();
    }
    for y in 1..h - 1 {
        for x in 2..w - 1 {
            let ax = a.px(x, y);
            let flat = [(x - 1, y), (x - 2, y), (x + 1, y), (x, y - 1), (x, y + 1), (x - 1, y - 1), (x - 1, y + 1)]
                .iter()
                .all(|&(i, j)| cdiff(a.px(i, j), ax) <= p.flat);
            if !flat {
                continue;
            }
            let shifted = a.px(x - 1, y);
            let mean = [0, 1, 2, 3].map(|k| ((u16::from(ax[k]) + u16::from(shifted[k])) / 2) as u8);
            let bad = cdiff(b.px(x, y), shifted) > p.threshold || cdiff(c.px(x, y), mean) > p.threshold;
            mask[(y * w + x) as usize] = bad && !outline(x, y);
        }
    }
    components(&mask, w, h)
        .into_iter()
        .map(|(b, _)| b)
        .filter(|b| b.pixels >= p.min_pixels)
        .collect()
}

/// Faces this close to parallel (cosine, about 1.8 degrees) can fight in the
/// render test: stricter than the geometry test's `zfight_angle` (see
/// `zfight.rs`), because here only the pixel normals are known.
const PARALLEL: f64 = 0.9995;
/// Within one item, a fold sharper than this (degrees between the faces the
/// rays hit) is an outline too: a crease the renderer's edge effects draw
/// and that aliases under jitter. Gentler than the facets of a 16-sided
/// sphere (22.5 degrees), which are smooth-shaded.
const CREASE_DEG: f64 = 30.0;

/// What a view ray sees first.
#[derive(Debug, Clone, PartialEq)]
pub struct Front {
    /// Sorted ids of the first item hit and of every other item hit within
    /// `tol` metres behind it (along the ray) on a face parallel to it
    /// (within [`PARALLEL`]); decals included. Empty: the sky. Two surfaces
    /// that fight give the same pair on every ray through their shared
    /// patch; surfaces meeting at an angle (a sphere set into a panel, a
    /// kerb on a road) do not merge, so the item seen first flips across
    /// their meeting line.
    pub ids: Vec<usize>,
    /// Normal of the face the first hit lands on.
    pub normal: Option<V3>,
}

/// The face of `shape` that point `p` (on its surface) lies on, facing the
/// ray `d` where there is a choice.
fn hit_normal(shape: &crate::shape::Shape, p: V3, d: V3) -> Option<V3> {
    let mut best: Option<(f64, V3)> = None;
    for f in &shape.faces {
        if !f.bb.contains(p, 1e-3) {
            continue;
        }
        // Prefer front faces: a back face through the same point loses.
        let dist = (dot(f.n, p) - f.d).abs() + if dot(f.n, d) > 0.0 { 1e-3 } else { 0.0 };
        if best.is_none_or(|b| dist < b.0) {
            best = Some((dist, f.n));
        }
    }
    best.map(|b| b.1)
}

/// The [`Front`] of a view ray from `o` along unit `d`.
pub fn front_set(s: &Scene, o: V3, d: V3, tol: f64) -> Front {
    let Some((t0, id0)) = s.ray(o, d, FAR, &|_| false) else {
        return Front { ids: Vec::new(), normal: None };
    };
    let normal = hit_normal(&s.items[id0].shape, add(o, mul(d, t0)), d);
    let mut ids = vec![id0];
    let mut seen = vec![id0];
    while seen.len() < 8 {
        let Some((t, id)) = s.ray(o, d, t0 + tol, &|i| seen.contains(&i)) else { break };
        seen.push(id);
        let n = hit_normal(&s.items[id].shape, add(o, mul(d, t)), d);
        let parallel = match (normal, n) {
            (Some(a), Some(b)) => dot(a, b) >= PARALLEL,
            _ => true,
        };
        if parallel {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    Front { ids, normal }
}

impl Front {
    /// Two samples see the same surface (or the same fighting pair).
    fn same(&self, o: &Front) -> bool {
        if self.ids != o.ids {
            return false;
        }
        match (self.normal, o.normal) {
            (Some(a), Some(b)) => dot(a, b) >= CREASE_DEG.to_radians().cos(),
            _ => true,
        }
    }
}

/// Object outlines in one view of a scene, from the scene's own geometry
/// (an object-ID map made by ray casting, so it needs nothing from the
/// renderer). A pixel is on an outline when the [`Front`] is not the
/// same at every sample within `reach` pixels of its centre (a grid of
/// `step` pixels): another object, the sky, one surface of a fighting pair
/// ending, a line where two surfaces meet at an angle, or a fold of more
/// than 30 degrees. Z-fighting is a pixel inside two surfaces' shared patch,
/// where the set is the same pair all round. Ray results are cached, so
/// neighbouring pixels share their samples.
pub struct Outlines<'a> {
    pub scene: &'a Scene,
    pub cam: Camera,
    pub w: u32,
    pub h: u32,
    /// Depth window (m along the ray) that puts two surfaces in one set.
    pub tol: f64,
    /// How far from the pixel centre to look (px). Sub-pixel jitter of up
    /// to a pixel, plus the multisample footprint, decide a pixel's colour.
    pub reach: f64,
    /// Sample spacing (px): finer than the multisample pattern, so a thin
    /// stick that covers a sample in some shot is seen.
    pub step: f64,
    cache: std::cell::RefCell<std::collections::HashMap<(i64, i64), Front>>,
}

impl<'a> Outlines<'a> {
    pub fn new(scene: &'a Scene, cam: Camera, w: u32, h: u32) -> Outlines<'a> {
        Outlines {
            scene,
            cam,
            w,
            h,
            tol: scene.cfg.zfight_gap,
            reach: 1.5,
            step: 0.25,
            cache: Default::default(),
        }
    }

    fn sample(&self, kx: i64, ky: i64) -> Front {
        if let Some(v) = self.cache.borrow().get(&(kx, ky)) {
            return v.clone();
        }
        let (o, d) = self.cam.pixel_ray(self.w, self.h, kx as f64 * self.step, ky as f64 * self.step);
        let v = front_set(self.scene, o, d, self.tol);
        self.cache.borrow_mut().insert((kx, ky), v.clone());
        v
    }

    /// Pixel (x, y) lies within `reach` of an object's outline.
    pub fn at(&self, x: u32, y: u32) -> bool {
        let n = (self.reach / self.step).round() as i64;
        let (cx, cy) = ((f64::from(x) / self.step).round() as i64, (f64::from(y) / self.step).round() as i64);
        let centre = self.sample(cx, cy);
        for dy in -n..=n {
            for dx in -n..=n {
                if (dx, dy) != (0, 0) && !self.sample(cx + dx, cy + dy).same(&centre) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera {
            eye: [0.0, 10.0, 0.0],
            target: [0.0, 10.0, -10.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
            aspect: 1.5,
        }
    }

    #[test]
    fn pixel_ray_and_project_agree() {
        let c = cam();
        let (o, d) = c.pixel_ray(300, 200, 37.0, 151.0);
        let p = add(o, mul(d, 12.0));
        let q = c.project(300, 200, p).unwrap();
        assert!((q[0] - 37.0).abs() < 1e-6 && (q[1] - 151.0).abs() < 1e-6, "{q:?}");
        // The centre pixel looks at the target.
        let q = c.project(300, 200, c.target).unwrap();
        assert!((q[0] - 149.5).abs() < 1e-6 && (q[1] - 99.5).abs() < 1e-6);
        assert!(c.project(300, 200, [0.0, 20.0, 10.0]).is_none());
    }

    #[test]
    fn magenta_on_the_ground_is_a_gap_but_the_sky_is_not() {
        let c = cam();
        let (w, h) = (60u32, 40u32);
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                // Top rows: real sky (magenta, rays point up); the rest grey.
                let sky = y < 5;
                img.set(x, y, if sky { [255, 0, 255, 255] } else { [90, 90, 90, 255] });
            }
        }
        // A hole in the ground at the bottom (touches the border).
        for y in 30..40 {
            for x in 20..26 {
                img.set(x, y, [250, 10, 250, 255]);
            }
        }
        // A crack island below the horizon (row 20 is the horizon).
        for x in 40..44 {
            img.set(x, 28, [255, 0, 255, 255]);
        }
        // Sky between objects at the horizon: an island, but not a crack.
        for x in 8..12 {
            img.set(x, 19, [255, 0, 255, 255]);
            img.set(x, 20, [255, 0, 255, 255]);
        }
        let fp = Aabb {
            min: [-100.0, -1.0, -100.0],
            max: [100.0, 1.0, 100.0],
        };
        let g = gaps(&img, None, &c, 0.0, &fp, &GapParams::default());
        assert_eq!(g.len(), 2, "{g:?}");
        assert!(g.iter().any(|b| b.rect == [20, 30, 6, 10] && b.hits == 60 && b.hit_rect == b.rect));
        assert!(g.iter().any(|b| b.rect == [40, 28, 4, 1]));
    }

    #[test]
    fn a_sky_coloured_object_is_not_a_hole_when_the_second_sky_disagrees() {
        let c = cam();
        let (w, h) = (60u32, 40u32);
        // Capture A: magenta sky; a hole at the bottom left and a magenta
        // item box at the bottom right. Capture B: green sky, so the hole
        // turns green but the box stays magenta.
        let mut a = Image::new(w, h);
        let mut b = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let hole = (30..40).contains(&y) && (5..11).contains(&x);
                let boxx = (30..40).contains(&y) && (40..46).contains(&x);
                let sky = y < 5 || hole;
                a.set(x, y, if sky || boxx { [255, 0, 255, 255] } else { [90, 90, 90, 255] });
                b.set(
                    x,
                    y,
                    if sky {
                        [0, 255, 0, 255]
                    } else if boxx {
                        [255, 20, 255, 255]
                    } else {
                        [90, 90, 90, 255]
                    },
                );
            }
        }
        let fp = Aabb {
            min: [-100.0, -1.0, -100.0],
            max: [100.0, 1.0, 100.0],
        };
        let g = gaps(&a, None, &c, 0.0, &fp, &GapParams::default());
        assert_eq!(g.len(), 2, "without the second sky the box looks like a hole: {g:?}");
        let g = gaps(&a, Some((&b, [0, 255, 0])), &c, 0.0, &fp, &GapParams::default());
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!(g[0].rect, [5, 30, 6, 10]);
    }

    #[test]
    fn sky_past_the_far_plane_or_at_the_horizon_is_not_a_gap() {
        // High camera looking level: rows just under the horizon (row 20)
        // land on the ground more than FAR away; rows 26+ land within it.
        let c = Camera {
            eye: [0.0, 100.0, 0.0],
            target: [0.0, 100.0, -10.0],
            ..cam()
        };
        let (w, h) = (60u32, 40u32);
        let paint = |rows: std::ops::Range<u32>| {
            let mut img = Image::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let sky = y < 20 || rows.contains(&y);
                    img.set(x, y, if sky { [255, 0, 255, 255] } else { [90, 90, 90, 255] });
                }
            }
            img
        };
        let fp = Aabb {
            min: [-1.0e5, -1.0, -1.0e5],
            max: [1.0e5, 1.0, 1.0e5],
        };
        // Sky down to row 25: everything below the horizon there is past
        // the far plane (or within the slack of it): no gap.
        let g = gaps(&paint(20..26), None, &c, 0.0, &fp, &GapParams::default());
        assert!(g.is_empty(), "{g:?}");
        // Sky down to row 29: rows 26 to 29 are real holes, and the report
        // box covers only them.
        let g = gaps(&paint(20..30), None, &c, 0.0, &fp, &GapParams::default());
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!(g[0].hits, 4 * 60);
        assert_eq!(g[0].hit_rect, [0, 26, 60, 4]);
        assert_eq!(g[0].rect, [0, 0, 60, 30]);
    }

    #[test]
    fn jitter_shift_is_not_flicker_but_a_changed_patch_is() {
        let (w, h) = (40u32, 20u32);
        // A: a vertical edge at x = 10 and flat elsewhere.
        let paint = |shift: f64, fight: bool| {
            let mut im = Image::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let edge = 10.0 + shift;
                    let v = if (x as f64) < edge.floor() {
                        30
                    } else if (x as f64) < edge.ceil() {
                        ((30.0 * (edge.ceil() - edge)) + 200.0 * (edge - edge.floor())) as u8
                    } else {
                        200
                    };
                    let mut c = [v, v, v, 255];
                    if fight && (25..30).contains(&x) && (5..10).contains(&y) {
                        c = [0, 200, 0, 255];
                    }
                    im.set(x, y, c);
                }
            }
            im
        };
        let a = paint(0.0, false);
        let b = paint(1.0, true);
        let c = paint(0.5, false);
        let f = flicker(&a, &b, &c, &FlickerParams::default());
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].rect, [25, 5, 5, 5]);
        assert!(flicker(&a, &paint(1.0, false), &c, &FlickerParams::default()).is_empty());
    }

    use crate::scene::{Config, mesh_name};
    use crate::shape::Shapes;
    use evani_render::mesh::MeshId;
    use evani_render::world::MeshInstance;

    fn slab(p: [f32; 3], s: [f32; 3]) -> MeshInstance {
        let mut m = MeshInstance::new(MeshId::CUBE, p);
        m.scale = s;
        m
    }

    fn scene_of(list: &[MeshInstance]) -> Scene {
        let l: Vec<(usize, MeshInstance)> = list.iter().copied().enumerate().collect();
        Scene::build(&l, &mut Shapes::new(), &|m| mesh_name(m.mesh), Config::default())
    }

    /// A tiny ray-cast "renderer" for the flicker tests: pixel x samples
    /// the view at x - `jx` (what the projection jitter does), one sample
    /// per pixel, each item in its own flat colour, the sky magenta. Where
    /// the ray meets two items at the same depth, `fight` picks the one the
    /// depth test lets through in this shot.
    fn render(s: &Scene, cam: &Camera, w: u32, h: u32, jx: f64, colours: &[[u8; 4]], fight: &dyn Fn(&[usize]) -> usize) -> Image {
        let mut im = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (o, d) = cam.pixel_ray(w, h, f64::from(x) - jx, f64::from(y));
                let set = front_set(s, o, d, 0.05).ids;
                let c = match set.len() {
                    0 => [255, 0, 255, 255],
                    1 => colours[set[0]],
                    _ => colours[fight(&set)],
                };
                im.set(x, y, c);
            }
        }
        im
    }

    #[test]
    fn a_real_z_fight_far_from_the_origin_is_flicker() {
        // Two flush slab tops 950 m out; the depth test lets the small red
        // one through in the (0,0) shots and the big grey one in the
        // jittered shots, over the whole shared patch.
        let (x0, z0) = (900.0f32, 300.0f32);
        let s = scene_of(&[slab([x0, -0.1, z0], [10.0, 0.2, 10.0]), slab([x0, -0.1, z0], [2.0, 0.2, 2.0])]);
        let colours = [[120, 120, 120, 255], [200, 40, 40, 255]];
        let cam = Camera {
            eye: [f64::from(x0), 6.0, f64::from(z0) + 4.0],
            target: [f64::from(x0), 0.0, f64::from(z0)],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
            aspect: 1.5,
        };
        let (w, h) = (120u32, 80u32);
        let shot = |jx: f64, winner: usize| render(&s, &cam, w, h, jx, &colours, &|set| set[winner.min(set.len() - 1)]);
        let (a, b, c) = (shot(0.0, 1), shot(1.0, 0), shot(0.5, 0));
        let outlines = Outlines::new(&s, cam, w, h);
        let f = flicker_where(&a, &b, &c, &FlickerParams::default(), &|x, y| outlines.at(x, y));
        assert_eq!(f.len(), 1, "{f:?}");
        // The patch interior (all but its rim) is reported.
        let full = flicker(&a, &b, &c, &FlickerParams::default());
        assert!(f[0].pixels >= 100 && f[0].pixels * 10 >= full[0].pixels * 6, "{} of {}", f[0].pixels, full[0].pixels);
        let centre = cam.project(w, h, [f64::from(x0), 0.0, f64::from(z0)]).unwrap();
        assert!((f[0].center[0] - centre[0]).abs() < 3.0 && (f[0].center[1] - centre[1]).abs() < 3.0, "{f:?} {centre:?}");
        // No fight (the same winner in every shot): nothing.
        let (a2, b2, c2) = (shot(0.0, 1), shot(1.0, 1), shot(0.5, 1));
        assert!(flicker_where(&a2, &b2, &c2, &FlickerParams::default(), &|x, y| outlines.at(x, y)).is_empty());
    }

    #[test]
    fn a_thin_stick_aliasing_under_jitter_is_not_flicker() {
        // A 3 cm stick 10 m away is a fifth of a pixel wide. It sits on
        // column 30.5, between pixel centres: the (0,0) and (1,0) shots miss
        // it, the half-pixel shot hits it in column 31. That is coverage
        // changing on an outline (aliasing), not two surfaces fighting.
        let (w, h) = (120u32, 80u32);
        let cam = Camera {
            eye: [0.0, 1.5, 10.0],
            target: [0.0, 1.5, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
            aspect: 1.5,
        };
        let (o, d) = cam.pixel_ray(w, h, 30.5, 40.0);
        let sx = (o[0] + d[0] * (-o[2] / d[2])) as f32;
        let s = scene_of(&[slab([0.0, -0.1, 0.0], [40.0, 0.2, 40.0]), slab([sx, 1.5, 0.0], [0.03, 3.0, 0.03])]);
        let colours = [[60, 140, 60, 255], [250, 250, 250, 255]];
        let shot = |jx: f64| render(&s, &cam, w, h, jx, &colours, &|set| set[0]);
        let (a, b, c) = (shot(0.0), shot(1.0), shot(0.5));
        // The old test flags it ...
        let full = flicker(&a, &b, &c, &FlickerParams::default());
        assert!(!full.is_empty() && full.iter().all(|b| b.rect[0] == 31 && b.rect[2] == 1), "{full:?}");
        // ... the outline test sets it aside.
        let outlines = Outlines::new(&s, cam, w, h);
        let f = flicker_where(&a, &b, &c, &FlickerParams::default(), &|x, y| outlines.at(x, y));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn lines_where_surfaces_meet_at_an_angle_are_outlines() {
        // A cube turned 45 degrees about z, half sunk into a slab, seen
        // from straight above: its faces meet the slab along x = +-1.41
        // (the light set into its panel, a kerb on a road) and each other
        // along x = 0 (a 90 degree fold). Those lines alias under jitter;
        // the middle of a face and the open slab do not.
        let mut cube = slab([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let h = std::f64::consts::FRAC_PI_8;
        cube.rotation = [0.0, 0.0, h.sin() as f32, h.cos() as f32];
        let s = scene_of(&[slab([0.0, -0.1, 0.0], [20.0, 0.2, 20.0]), cube]);
        let cam = Camera {
            eye: [0.0, 10.0, 0.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, -1.0],
            fov_y_deg: 40.0,
            aspect: 1.5,
        };
        let (w, h) = (300u32, 200u32);
        let o = Outlines::new(&s, cam, w, h);
        let px = |p: V3| {
            let q = cam.project(w, h, p).unwrap();
            (q[0].round() as u32, q[1].round() as u32)
        };
        let at = |p: V3| {
            let (x, y) = px(p);
            o.at(x, y)
        };
        let r = std::f64::consts::SQRT_2;
        assert!(at([r, 0.0, 0.3]) && at([-r, 0.0, -0.3]), "where the cube enters the slab");
        assert!(at([0.0, r, 0.0]), "the cube's ridge");
        assert!(!at([0.7, 0.7, 0.0]) && !at([-0.7, 0.7, 0.4]), "the middle of a face");
        assert!(!at([4.0, 0.0, 1.0]), "the open slab");
    }

    #[test]
    fn crop_outlines_the_rect() {
        let mut img = Image::new(20, 20);
        img.set(10, 10, [9, 9, 9, 255]);
        let c = img.crop_outlined([8, 8, 5, 5], 2, [255, 0, 0]);
        assert_eq!((c.w, c.h), (9, 9));
        assert_eq!(c.px(2, 2), [255, 0, 0, 255]);
        assert_eq!(c.px(4, 4), [9, 9, 9, 255]);
    }
}
