//! Pictures, drawn by Evani.Model itself (docs/model_research/21): a
//! fixed-point rasteriser with four samples per pixel, per-pixel normals
//! (interpolated field gradients, or the exact field gradient when the
//! field is given), the toon look's three-band ramp (data/looks/toon.json,
//! checked against the look's shader constants by the bench tool), outlines
//! from depth and coverage jumps, our own bitmap font and PNG writer.
//! Integer coverage and a fixed operation order make the bytes identical on
//! every machine and thread count.
//!
//! Masks for references and IoU use the same rasteriser: a pixel is inside
//! when at least two of its four samples are covered.

use crate::mesh::Mesh;
use crate::png::Grey;
use crate::rng::map_tiles;
use crate::sdf::Node;
use crate::vec::*;

#[derive(Clone, Copy, Debug)]
pub enum Proj {
    /// Orthographic: `size` metres across the image, centred at `centre`.
    Ortho { size: f64 },
    /// Perspective with a vertical field of view in degrees.
    Persp { fov_deg: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: V3,
    pub fwd: V3,
    pub up: V3,
    pub proj: Proj,
    /// Point at the image centre (orthographic views).
    pub centre: V3,
}

impl Camera {
    fn basis(&self) -> (V3, V3, V3) {
        let f = norm_or(self.fwd, [0.0, 0.0, -1.0]);
        let r = norm_or(cross(f, self.up), [1.0, 0.0, 0.0]);
        let u = cross(r, f);
        (r, u, f)
    }
    /// Image coordinates in [0, 1] (u right, v down) and depth along the view.
    pub fn project(&self, p: V3) -> (f64, f64, f64) {
        let (r, u, f) = self.basis();
        match self.proj {
            Proj::Ortho { size } => {
                let d = sub(p, self.centre);
                (0.5 + dot(d, r) / size, 0.5 - dot(d, u) / size, dot(sub(p, self.eye), f))
            }
            Proj::Persp { fov_deg } => {
                let d = sub(p, self.eye);
                let z = dot(d, f).max(1e-6);
                let t = crate::dmath::tan(crate::dmath::deg(fov_deg * 0.5));
                (0.5 + dot(d, r) / (z * t * 2.0), 0.5 - dot(d, u) / (z * t * 2.0), z)
            }
        }
    }
    /// Front (camera on +z), side (on +x, nose left), top (on +y, nose up)
    /// and three-quarter (front-left, above) orthographic views.
    pub fn named(name: &str, centre: V3, size: f64) -> Camera {
        let far = size * 4.0;
        let (fwd, up): (V3, V3) = match name {
            "front" => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            "side" => ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            "top" => ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            _ => (norm_or([-1.0, -0.65, -1.0], [0.0, 0.0, -1.0]), [0.0, 1.0, 0.0]),
        };
        Camera { eye: sub(centre, mul(fwd, far)), fwd, up, proj: Proj::Ortho { size }, centre }
    }
}

/// Per-sample hits: triangle index (u32::MAX = none) and depth.
pub struct Raster {
    pub w: usize,
    pub h: usize,
    pub tri: Vec<u32>,
    pub depth: Vec<f64>,
    pub bary: Vec<[f64; 3]>,
}

const SUB: i64 = 16;
const SAMPLES: [(i64, i64); 4] = [(4, 4), (12, 4), (4, 12), (12, 12)];

pub fn rasterize(m: &Mesh, cam: &Camera, w: usize, h: usize, threads: usize) -> Raster {
    let proj: Vec<(i64, i64, f64)> = m
        .pos
        .iter()
        .map(|p| {
            let (u, v, z) = cam.project(*p);
            ((u * w as f64 * SUB as f64).round() as i64, (v * h as f64 * SUB as f64).round() as i64, z)
        })
        .collect();
    let bands = 16.min(h.max(1));
    let rows_per = h.div_ceil(bands);
    let parts = map_tiles(bands, threads, |b| {
        let (y0, y1) = (b * rows_per, ((b + 1) * rows_per).min(h));
        let n = (y1 - y0) * w * 4;
        let mut tri = vec![u32::MAX; n];
        let mut depth = vec![f64::INFINITY; n];
        let mut bary = vec![[0.0; 3]; n];
        for (ti, t) in m.tris.iter().enumerate() {
            let (a, bb, c) = (proj[t[0] as usize], proj[t[1] as usize], proj[t[2] as usize]);
            let area = (bb.0 - a.0) * (c.1 - a.1) - (bb.1 - a.1) * (c.0 - a.0);
            if area == 0 {
                continue;
            }
            let (px0, px1) = (a.0.min(bb.0).min(c.0), a.0.max(bb.0).max(c.0));
            let (py0, py1) = (a.1.min(bb.1).min(c.1), a.1.max(bb.1).max(c.1));
            let xs = (px0 / SUB).max(0) as usize;
            let xe = ((px1 / SUB) + 1).min(w as i64);
            let ys = ((py0 / SUB).max(y0 as i64)) as usize;
            let ye = ((py1 / SUB) + 1).min(y1 as i64);
            if xe <= xs as i64 || ye <= ys as i64 {
                continue;
            }
            let s = if area > 0 { 1 } else { -1 };
            let edge = |p: (i64, i64, f64), q: (i64, i64, f64), x: i64, y: i64| ((q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0)) * s;
            for y in ys..ye as usize {
                for x in xs..xe as usize {
                    for (k, (sx, sy)) in SAMPLES.iter().enumerate() {
                        let (fx, fy) = (x as i64 * SUB + sx, y as i64 * SUB + sy);
                        let e0 = edge(bb, c, fx, fy);
                        let e1 = edge(c, a, fx, fy);
                        let e2 = edge(a, bb, fx, fy);
                        if e0 < 0 || e1 < 0 || e2 < 0 {
                            continue;
                        }
                        let sum = (e0 + e1 + e2) as f64;
                        let (b0, b1, b2) = (e0 as f64 / sum, e1 as f64 / sum, e2 as f64 / sum);
                        let z = b0 * a.2 + b1 * bb.2 + b2 * c.2;
                        let i = ((y - y0) * w + x) * 4 + k;
                        if z < depth[i] {
                            depth[i] = z;
                            tri[i] = ti as u32;
                            bary[i] = [b0, b1, b2];
                        }
                    }
                }
            }
        }
        (tri, depth, bary)
    });
    let mut r = Raster { w, h, tri: Vec::with_capacity(w * h * 4), depth: Vec::with_capacity(w * h * 4), bary: Vec::with_capacity(w * h * 4) };
    for (t, d, b) in parts {
        r.tri.extend(t);
        r.depth.extend(d);
        r.bary.extend(b);
    }
    r
}

/// Binary silhouette: 255 where at least two of four samples are covered.
pub fn mask(m: &Mesh, cam: &Camera, size: usize, threads: usize) -> Grey {
    let r = rasterize(m, cam, size, size, threads);
    let mut g = Grey::new(size, size, 0);
    for i in 0..size * size {
        let n = (0..4).filter(|k| r.tri[i * 4 + k] != u32::MAX).count();
        if n >= 2 {
            g.px[i] = 255;
        }
    }
    g
}

/// Intersection over union of two masks of the same size.
pub fn iou(a: &Grey, b: &Grey) -> f64 {
    let (mut i, mut u) = (0usize, 0usize);
    for k in 0..a.px.len().min(b.px.len()) {
        let (x, y) = (a.px[k] > 127, b.px[k] > 127);
        if x && y {
            i += 1;
        }
        if x || y {
            u += 1;
        }
    }
    if u == 0 { 1.0 } else { i as f64 / u as f64 }
}

/// The toon look's ramp (three bands), from our data file.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    pub band_low: f64,
    pub band_high: f64,
    pub softness: f64,
    pub base: f64,
    pub mid: f64,
    pub top: f64,
}

pub fn toon_ramp() -> Ramp {
    let v: serde_json::Value = serde_json::from_str(include_str!("../data/looks/toon.json")).unwrap_or(serde_json::Value::Null);
    let g = |k: &str, d: f64| v["ramp"][k].as_f64().unwrap_or(d);
    Ramp { band_low: g("band_low", 0.02), band_high: g("band_high", 0.45), softness: g("softness", 0.025), base: g("base", 0.25), mid: g("mid", 0.35), top: g("top", 0.4) }
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Ramp {
    pub fn shade(&self, ndl: f64) -> f64 {
        let low = smoothstep(self.band_low - self.softness, self.band_low + self.softness, ndl);
        let high = smoothstep(self.band_high - self.softness, self.band_high + self.softness, ndl);
        self.base + self.mid * low + self.top * high
    }
}

/// A shaded view: toon bands on a white page, dark outlines.
pub fn render(m: &Mesh, field: Option<&Node>, cam: &Camera, w: usize, h: usize, threads: usize) -> Grey {
    let r = rasterize(m, cam, w, h, threads);
    let ramp = toon_ramp();
    let (rt, upv, f) = cam.basis();
    // Light from the upper left, slightly towards the viewer.
    let light = norm_or(add(add(mul(rt, -0.5), mul(upv, 0.7)), mul(f, -0.5)), [0.0, 1.0, 0.0]);
    let shades: Vec<Vec<u8>> = map_tiles(h, threads, |y| {
        let mut row = vec![255u8; w];
        for (x, out) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for k in 0..4 {
                let i = (y * w + x) * 4 + k;
                let t = r.tri[i];
                if t == u32::MAX {
                    acc += 1.0;
                    continue;
                }
                let tr = m.tris[t as usize];
                let b = r.bary[i];
                let n = match field {
                    Some(fl) => {
                        let p = add(add(mul(m.pos[tr[0] as usize], b[0]), mul(m.pos[tr[1] as usize], b[1])), mul(m.pos[tr[2] as usize], b[2]));
                        norm_or(fl.eval(p).g, [0.0, 1.0, 0.0])
                    }
                    None => norm_or(add(add(mul(m.nrm[tr[0] as usize], b[0]), mul(m.nrm[tr[1] as usize], b[1])), mul(m.nrm[tr[2] as usize], b[2])), [0.0, 1.0, 0.0]),
                };
                acc += ramp.shade(dot(n, light).max(-1.0));
            }
            *out = ((acc / 4.0) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        row
    });
    let mut g = Grey::new(w, h, 255);
    for (y, row) in shades.iter().enumerate() {
        g.px[y * w..(y + 1) * w].copy_from_slice(row);
    }
    // Outlines: coverage edges and depth jumps between neighbouring pixels.
    let pix_depth = |x: usize, y: usize| -> Option<f64> {
        let i = (y * w + x) * 4;
        (0..4).filter(|k| r.tri[i + k] != u32::MAX).map(|k| r.depth[i + k]).fold(None, |a: Option<f64>, d| Some(a.map_or(d, |a| a.min(d))))
    };
    let scale = match cam.proj {
        Proj::Ortho { size } => size / w as f64,
        Proj::Persp { .. } => 0.01,
    };
    let mut lines = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let d = pix_depth(x, y);
            for (dx, dy) in [(1usize, 0usize), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= w || ny >= h {
                    continue;
                }
                let e = pix_depth(nx, ny);
                let edge = match (d, e) {
                    (Some(a), Some(b)) => (a - b).abs() > scale * 8.0,
                    (None, None) => false,
                    _ => true,
                };
                if edge {
                    lines[y * w + x] = true;
                }
            }
        }
    }
    for (i, l) in lines.iter().enumerate() {
        if *l {
            g.px[i] = 30;
        }
    }
    g
}

/// 5 x 7 glyphs of our own design (rows top to bottom, 5 bits each).
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '.' => [0, 0, 0, 0, 0, 0x0C, 0x0C],
        ',' => [0, 0, 0, 0, 0x0C, 0x04, 0x08],
        '-' => [0, 0, 0, 0x1F, 0, 0, 0],
        ':' => [0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '%' => [0x19, 0x1A, 0x02, 0x04, 0x08, 0x0B, 0x13],
        '_' => [0, 0, 0, 0, 0, 0, 0x1F],
        '+' => [0, 0x04, 0x04, 0x1F, 0x04, 0x04, 0],
        '=' => [0, 0, 0x1F, 0, 0x1F, 0, 0],
        _ => [0; 7],
    }
}

/// Draw `text` at (x, y) with pixel size `s`.
pub fn text(img: &mut Grey, x: usize, y: usize, s: usize, value: u8, t: &str) {
    for (ci, c) in t.chars().enumerate() {
        let gl = glyph(c);
        for (row, bits) in gl.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) != 0 {
                    for dy in 0..s {
                        for dx in 0..s {
                            img.set(x + (ci * 6 + col) * s + dx, y + row * s + dy, value);
                        }
                    }
                }
            }
        }
    }
}

/// Four views in a 2 x 2 sheet with labels and a caption line.
pub fn blueprint(m: &Mesh, field: Option<&Node>, caption: &str, panel: usize, threads: usize) -> Grey {
    let b = m.bounds();
    let c = b.centre();
    let size = len(b.size()) * 1.05;
    let label_h = 24;
    let mut sheet = Grey::new(panel * 2, panel * 2 + label_h, 255);
    for (i, name) in ["front", "side", "top", "three_quarter"].iter().enumerate() {
        let cam = Camera::named(name, c, size);
        let img = render(m, field, &cam, panel, panel, threads);
        let (px, py) = ((i % 2) * panel, (i / 2) * panel);
        sheet.blit(&img, px, py);
        text(&mut sheet, px + 8, py + 8, 2, 40, &name.replace('_', " "));
        for k in 0..panel {
            sheet.set(px + k, py, 160);
            sheet.set(px, py + k, 160);
        }
    }
    text(&mut sheet, 8, panel * 2 + 6, 2, 20, caption);
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_mesh() -> Mesh {
        let f = Node::RoundBox { half: [0.5, 0.3, 0.4], r: 0.05 }.prepare();
        crate::mesh::extract(&f, f.bounds(), &crate::mesh::Options::new(0.05, 2))
    }

    #[test]
    fn masks_measure_area_and_iou() {
        let m = cube_mesh();
        let cam = Camera::named("front", [0.0; 3], 2.0);
        let g = mask(&m, &cam, 200, 2);
        let covered = g.px.iter().filter(|&&p| p == 255).count() as f64 / (200.0 * 200.0);
        // Front silhouette 1.0 x 0.6 m (rounded corners) in a 2 x 2 m frame.
        assert!((covered - 0.15).abs() < 0.005, "{covered}");
        assert!((iou(&g, &g) - 1.0).abs() < 1e-12);
        let side = mask(&m, &Camera::named("side", [0.0; 3], 2.0), 200, 2);
        let i = iou(&g, &side);
        assert!(i > 0.7 && i < 0.9, "{i}");
    }

    #[test]
    fn identical_pictures_for_any_thread_count() {
        let m = cube_mesh();
        let a = blueprint(&m, None, "TEST 123", 128, 1);
        for t in [2, 4, 16] {
            assert_eq!(blueprint(&m, None, "TEST 123", 128, t), a);
        }
        let png = crate::png::write(&a);
        assert_eq!(crate::png::read(&png).unwrap(), a);
    }
}
