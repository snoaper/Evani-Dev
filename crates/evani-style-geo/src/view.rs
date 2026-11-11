//! A small software rasteriser for the geometry stage's own checks and
//! pictures (no GPU, deterministic): orthographic views of a mesh, its
//! silhouette mask, the intersection-over-union of two silhouettes, and a
//! flat-lit colour picture with each voxel cube's edges drawn the way the
//! surface step can (the fragment's place inside its cube is its position
//! minus its sample, the cell centre).

use evani_studio::geo::{SourceMesh, StyledMesh};

use crate::math::{V3, add, cross, dot, normalize, scale, sub};

/// A view direction: `yaw` round the up axis (0 = looking along -Z at the
/// front of a model facing +Z), `pitch` down from level, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub name: &'static str,
    pub yaw: f32,
    pub pitch: f32,
}

/// The three views the checks and pictures use: front, side and
/// three-quarter, each a little from above.
pub const VIEWS: [View; 3] = [View { name: "front", yaw: 0.0, pitch: 8.0 }, View { name: "side", yaw: 90.0, pitch: 8.0 }, View { name: "threeq", yaw: 40.0, pitch: 18.0 }];

/// The camera basis of a view: (right, up, forward into the scene).
fn basis(v: View) -> (V3, V3, V3) {
    let (y, p) = (v.yaw.to_radians(), v.pitch.to_radians());
    // The eye sits at +Z rotated by yaw round Y, raised by pitch.
    let eye = [y.sin() * p.cos(), p.sin(), y.cos() * p.cos()];
    let fwd = scale(eye, -1.0);
    let right = normalize(cross(fwd, [0.0, 1.0, 0.0]), [1.0, 0.0, 0.0]);
    let up = cross(right, fwd);
    (right, up, fwd)
}

/// A triangle soup to draw: positions, per-vertex sample (or the position),
/// and a colour per triangle.
pub struct Soup {
    pub tris: Vec<[V3; 3]>,
    pub samples: Vec<Option<[V3; 3]>>,
    pub region: Vec<u8>,
}

impl Soup {
    pub fn from_source(s: &SourceMesh) -> Self {
        let mut out = Soup { tris: Vec::new(), samples: Vec::new(), region: Vec::new() };
        for t in s.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| s.positions.get(i as usize).copied().unwrap_or_default());
            out.tris.push([a, b, c]);
            out.samples.push(None);
            out.region.push(s.regions.get(t[0] as usize).copied().unwrap_or(0));
        }
        out
    }

    pub fn from_styled(m: &StyledMesh) -> Self {
        let mut out = Soup { tris: Vec::new(), samples: Vec::new(), region: Vec::new() };
        for p in &m.parts {
            for t in p.indices.as_chunks::<3>().0 {
                let get = |v: &Vec<V3>| t.map(|i| v.get(i as usize).copied().unwrap_or_default());
                out.tris.push(get(&p.positions));
                out.samples.push((!p.samples.is_empty()).then(|| get(&p.samples)));
                out.region.push(p.region);
            }
        }
        out
    }
}

/// How a view maps model units to pixels: the centre and the pixels per
/// unit, fitted to a reference mesh so two meshes compare in one frame.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub view: View,
    pub centre: [f32; 2],
    pub px_per_unit: f32,
    pub w: usize,
    pub h: usize,
}

impl Frame {
    /// Fit `soup` in a `w` x `h` image with a margin of `margin` (0 to 0.5).
    pub fn fit(soup: &Soup, view: View, w: usize, h: usize, margin: f32) -> Self {
        let (r, u, _) = basis(view);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for t in &soup.tris {
            for p in t {
                let q = [dot(*p, r), dot(*p, u)];
                for k in 0..2 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
        if lo[0] > hi[0] {
            return Frame { view, centre: [0.0; 2], px_per_unit: 1.0, w, h };
        }
        let span = [(hi[0] - lo[0]).max(1e-6), (hi[1] - lo[1]).max(1e-6)];
        let k = ((w as f32) * (1.0 - 2.0 * margin) / span[0]).min((h as f32) * (1.0 - 2.0 * margin) / span[1]);
        Frame { view, centre: [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5], px_per_unit: k, w, h }
    }

    fn project(&self, p: V3, b: &(V3, V3, V3)) -> [f32; 3] {
        let x = (dot(p, b.0) - self.centre[0]) * self.px_per_unit + self.w as f32 * 0.5;
        let y = self.h as f32 * 0.5 - (dot(p, b.1) - self.centre[1]) * self.px_per_unit;
        [x, y, dot(p, b.2)]
    }
}

/// Rasterise every triangle: `each(pixel index, depth, barycentrics, tri)`
/// for each covered pixel centre (both windings drawn).
fn raster(soup: &Soup, f: &Frame, mut each: impl FnMut(usize, f32, [f32; 3], usize)) {
    let b = basis(f.view);
    for (ti, t) in soup.tris.iter().enumerate() {
        let [p0, p1, p2] = t.map(|p| f.project(p, &b));
        let area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = p0[0].min(p1[0]).min(p2[0]).floor().max(0.0) as usize;
        let x1 = (p0[0].max(p1[0]).max(p2[0]).ceil() as isize).min(f.w as isize - 1);
        let y0 = p0[1].min(p1[1]).min(p2[1]).floor().max(0.0) as usize;
        let y1 = (p0[1].max(p1[1]).max(p2[1]).ceil() as isize).min(f.h as isize - 1);
        if x1 < 0 || y1 < 0 {
            continue;
        }
        for y in y0..=(y1 as usize) {
            for x in x0..=(x1 as usize) {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((p1[0] - px) * (p2[1] - py) - (p1[1] - py) * (p2[0] - px)) / area;
                let w1 = ((p2[0] - px) * (p0[1] - py) - (p2[1] - py) * (p0[0] - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                    continue;
                }
                let z = w0 * p0[2] + w1 * p1[2] + w2 * p2[2];
                each(y * f.w + x, z, [w0, w1, w2], ti);
            }
        }
    }
}

/// The silhouette mask of `soup` in frame `f`.
pub fn silhouette(soup: &Soup, f: &Frame) -> Vec<bool> {
    let mut m = vec![false; f.w * f.h];
    raster(soup, f, |i, _, _, _| m[i] = true);
    m
}

/// Intersection over union of two masks (1 when both are empty).
pub fn iou(a: &[bool], b: &[bool]) -> f32 {
    let (mut i, mut u) = (0usize, 0usize);
    for (x, y) in a.iter().zip(b) {
        i += usize::from(*x && *y);
        u += usize::from(*x || *y);
    }
    if u == 0 { 1.0 } else { i as f32 / u as f32 }
}

/// A flat-lit RGB picture of `soup` in frame `f`; `colour(region)` gives each
/// region's colour. Voxel parts (with samples) get their cube edges darkened.
pub fn picture(soup: &Soup, f: &Frame, colour: &dyn Fn(u8) -> [f32; 3], background: [u8; 3]) -> Vec<u8> {
    let mut depth = vec![f32::MAX; f.w * f.h];
    let mut tri = vec![usize::MAX; f.w * f.h];
    let mut bary = vec![[0.0f32; 3]; f.w * f.h];
    raster(soup, f, |i, z, w, t| {
        if z < depth[i] {
            depth[i] = z;
            tri[i] = t;
            bary[i] = w;
        }
    });
    let (_, _, fwd) = basis(f.view);
    let light = normalize([0.45, 0.8, 0.55], [0.0, 1.0, 0.0]);
    let fill = normalize([-0.6, 0.2, -0.4], [0.0, 1.0, 0.0]);
    let mut img = Vec::with_capacity(f.w * f.h * 3);
    for i in 0..f.w * f.h {
        let t = tri[i];
        if t == usize::MAX {
            img.extend(background);
            continue;
        }
        let [a, b, c] = soup.tris[t];
        let mut n = normalize(cross(sub(b, a), sub(c, a)), [0.0, 1.0, 0.0]);
        if dot(n, fwd) > 0.0 {
            n = scale(n, -1.0);
        }
        let shade = 0.38 + 0.62 * dot(n, light).max(0.0) + 0.18 * dot(n, fill).max(0.0);
        let mut col = scale(colour(soup.region[t]), shade);
        if let Some(s) = soup.samples[t] {
            let w = bary[i];
            let p = add(add(scale(a, w[0]), scale(b, w[1])), scale(c, w[2]));
            let sp = add(add(scale(s[0], w[0]), scale(s[1], w[1])), scale(s[2], w[2]));
            let local = sub(p, sp);
            let half = dot(local, n).abs();
            let along = sub(local, scale(n, dot(local, n)));
            let edge = along.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            if half > 1e-6 && edge / half > 0.84 {
                col = scale(col, 0.62);
            }
        }
        img.extend(col.map(|x| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8));
    }
    img
}
