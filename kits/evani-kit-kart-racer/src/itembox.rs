//! The item box's own meshes, built for a unit cube (-0.5..0.5) and
//! rebuilt when their live tunables change: a bevelled body (lit, tinted
//! per box from the candy palette) and its decor, a bold rounded "?" and
//! four round rivets on every face (unlit white paint, one instance).
//!
//! The "?" is this kit's own glyph: one thick stroke swept along a hook
//! (an arc over the top that tucks back to the middle), a short stem and
//! a round dot, all with round ends.

use evani_render::mesh::{Mesh, bevel_cube};

/// How far the decor stands off its face (unit box), so it never fights
/// the face for depth.
pub const DECOR_LIFT: f32 = 0.006;

/// Half the height of the "?" at glyph size 1 (face units).
pub const GLYPH_HALF_HEIGHT: f32 = 0.355;

/// Gap kept between the decor and the bevel (face units).
const EDGE_MARGIN: f32 = 0.015;

/// Hook arc of the "?": centre height, radius, from and to angle (deg).
const HOOK_CENTRE_Y: f32 = 0.135;
const HOOK_RADIUS: f32 = 0.145;
const HOOK_FROM_DEG: f32 = 168.0;
const HOOK_TO_DEG: f32 = -52.0;
const HOOK_STEPS: usize = 12;
/// Half the stroke width.
const STROKE: f32 = 0.065;
/// Where the hook tucks into the stem, and the stem's foot.
const STEM_TOP: [f32; 2] = [0.0, -0.06];
const STEM_FOOT: [f32; 2] = [0.0, -0.10];
/// The dot under the stem.
const DOT: [f32; 2] = [0.0, -0.285];
const DOT_RADIUS: f32 = 0.07;

/// Faces of the cube: outward normal and the face's "up" as seen from
/// outside (the glyph's up).
const FACES: [([f32; 3], [f32; 3]); 6] = [
    ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
];

/// The shape tunables the meshes are built from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShape {
    /// Edge chamfer, as a fraction of the box (0.02..0.2).
    pub bevel: f32,
    /// "?" size (1 = 0.71 of the face high, shrunk to fit the flat face).
    pub glyph_size: f32,
    /// Rivet radius, as a fraction of the box (0 = no rivets).
    pub rivet: f32,
}

impl BoxShape {
    pub fn bevel(&self) -> f32 {
        self.bevel.clamp(0.02, 0.2)
    }

    /// Half the flat part of a face.
    fn flat(&self) -> f32 {
        0.5 - self.bevel()
    }

    /// The glyph's scale: as asked, but never past the flat face.
    pub fn glyph_scale(&self) -> f32 {
        let fit = (self.flat() - EDGE_MARGIN) / GLYPH_HALF_HEIGHT;
        self.glyph_size.max(0.0).min(fit)
    }

    pub fn rivet(&self) -> f32 {
        self.rivet.clamp(0.0, 0.08)
    }

    /// Rivet centres sit this far from the face centre on both axes.
    pub fn rivet_inset(&self) -> f32 {
        (self.flat() - self.rivet() - 0.03).max(0.2)
    }
}

/// The box body: a unit cube with chamfered edges (44 triangles).
pub fn body(shape: &BoxShape) -> Mesh {
    bevel_cube(shape.bevel())
}

/// The "?" centreline: hook arc, the tuck into the stem, the stem.
fn glyph_path() -> Vec<[f32; 2]> {
    let mut p = Vec::with_capacity(HOOK_STEPS + 3);
    for k in 0..=HOOK_STEPS {
        let a = (HOOK_FROM_DEG + (HOOK_TO_DEG - HOOK_FROM_DEG) * k as f32 / HOOK_STEPS as f32).to_radians();
        p.push([HOOK_RADIUS * a.cos(), HOOK_CENTRE_Y + HOOK_RADIUS * a.sin()]);
    }
    p.push(STEM_TOP);
    p.push(STEM_FOOT);
    p
}

type Tri = [[f32; 2]; 3];

/// A triangle wound counter-clockwise (front face, seen from outside).
fn ccw(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Tri {
    let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if cross >= 0.0 { [a, b, c] } else { [a, c, b] }
}

fn disc(out: &mut Vec<Tri>, c: [f32; 2], r: f32, sides: usize) {
    for k in 0..sides {
        let a0 = core::f32::consts::TAU * k as f32 / sides as f32;
        let a1 = core::f32::consts::TAU * (k + 1) as f32 / sides as f32;
        out.push(ccw(c, [c[0] + r * a0.cos(), c[1] + r * a0.sin()], [c[0] + r * a1.cos(), c[1] + r * a1.sin()]));
    }
}

/// A stroke of half width `h` along `path`, mitred at the joints, with
/// round ends.
fn stroke(out: &mut Vec<Tri>, path: &[[f32; 2]], h: f32) {
    let n = path.len();
    let dir = |a: [f32; 2], b: [f32; 2]| {
        let (x, y) = (b[0] - a[0], b[1] - a[1]);
        let l = x.hypot(y).max(1e-6);
        [x / l, y / l]
    };
    let side: Vec<([f32; 2], [f32; 2])> = (0..n)
        .map(|i| {
            let din = dir(path[i.saturating_sub(1)], path[i.max(1).min(n - 1)]);
            let dout = dir(path[i.min(n - 2)], path[(i + 1).min(n - 1)]);
            let t = dir([0.0, 0.0], [din[0] + dout[0], din[1] + dout[1]]);
            let nrm = [-t[1], t[0]];
            // Mitre: keep the stroke's width across the bend.
            let m = h / (nrm[0] * -dout[1] + nrm[1] * dout[0]).max(0.5);
            let p = path[i];
            ([p[0] + nrm[0] * m, p[1] + nrm[1] * m], [p[0] - nrm[0] * m, p[1] - nrm[1] * m])
        })
        .collect();
    for i in 0..n - 1 {
        let (l0, r0) = side[i];
        let (l1, r1) = side[i + 1];
        out.push(ccw(l0, r0, r1));
        out.push(ccw(l0, r1, l1));
    }
    // Round ends: a half disc on the outer side of each end only, so no
    // triangle lies over the stroke or over another (the sheet is flat
    // paint; its pieces tile it).
    let first = dir(path[0], path[1]);
    let last = dir(path[n - 2], path[n - 1]);
    half_disc(out, path[0], [-first[0], -first[1]], h, 4);
    half_disc(out, path[n - 1], last, h, 4);
}

/// The half of a disc of radius `r` at `c` that lies towards `away` (a unit
/// direction), as a fan of `steps` triangles from one end of its diameter to
/// the other.
fn half_disc(out: &mut Vec<Tri>, c: [f32; 2], away: [f32; 2], r: f32, steps: usize) {
    let base = away[1].atan2(away[0]) - core::f32::consts::FRAC_PI_2;
    for k in 0..steps {
        let a0 = base + core::f32::consts::PI * k as f32 / steps as f32;
        let a1 = base + core::f32::consts::PI * (k + 1) as f32 / steps as f32;
        out.push(ccw(c, [c[0] + r * a0.cos(), c[1] + r * a0.sin()], [c[0] + r * a1.cos(), c[1] + r * a1.sin()]));
    }
}

/// One face's decor in face units (x right, y up as seen from outside).
pub fn face_decor(shape: &BoxShape) -> Vec<Tri> {
    let mut out = Vec::new();
    let g = shape.glyph_scale();
    if g > 0.0 {
        let mut glyph = Vec::new();
        stroke(&mut glyph, &glyph_path(), STROKE);
        disc(&mut glyph, DOT, DOT_RADIUS, 10);
        out.extend(glyph.into_iter().map(|t| t.map(|v| [v[0] * g, v[1] * g])));
    }
    let r = shape.rivet();
    if r > 0.0 {
        let c = shape.rivet_inset();
        for (x, y) in [(-c, -c), (c, -c), (c, c), (-c, c)] {
            disc(&mut out, [x, y], r, 8);
        }
    }
    out
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The decor of all six faces as one mesh (unit box; draw it with the
/// body's position, rotation and scale).
pub fn decor(shape: &BoxShape) -> Mesh {
    let face = face_decor(shape);
    let mut m = Mesh::default();
    for (n, up) in FACES {
        let right = cross(up, n);
        let d = 0.5 + DECOR_LIFT;
        for t in &face {
            for v in t {
                let p = [
                    n[0] * d + right[0] * v[0] + up[0] * v[1],
                    n[1] * d + right[1] * v[0] + up[1] * v[1],
                    n[2] * d + right[2] * v[0] + up[2] * v[1],
                ];
                m.indices.push(m.vertices.len() as u32);
                m.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> BoxShape {
        BoxShape { bevel: 0.12, glyph_size: 1.0, rivet: 0.045 }
    }

    /// Every decor triangle faces out of its face (counter-clockwise seen
    /// from outside, so back-face culling keeps it).
    #[test]
    fn decor_faces_outward() {
        let m = decor(&shape());
        for t in m.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.vertices[i as usize]);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = cross(e1, e2);
            let d = n[0] * a[3] + n[1] * a[4] + n[2] * a[5];
            assert!(d >= -1e-9, "triangle faces inward: {a:?} {b:?} {c:?}");
        }
    }

    /// The "?" and rivets stay on the flat part of each face, clear of the
    /// bevels, at every bevel, glyph size and rivet size the tunables allow.
    #[test]
    fn decor_stays_on_the_flat_face() {
        for bevel in [0.02, 0.12, 0.2] {
            for glyph_size in [0.5, 1.0, 1.15] {
                for rivet in [0.0, 0.045, 0.08] {
                    let s = BoxShape { bevel, glyph_size, rivet };
                    let flat = 0.5 - bevel;
                    for t in face_decor(&s) {
                        for v in t {
                            assert!(v[0].abs() <= flat + 1e-4 && v[1].abs() <= flat + 1e-4, "{s:?}: {v:?} past the flat face");
                        }
                    }
                }
            }
        }
    }

    /// Rivets sit in the corners, clear of the "?".
    #[test]
    fn rivets_clear_the_glyph() {
        let s = shape();
        let g = s.glyph_scale();
        let mut glyph = Vec::new();
        stroke(&mut glyph, &glyph_path(), STROKE);
        disc(&mut glyph, DOT, DOT_RADIUS, 10);
        let c = s.rivet_inset();
        for t in glyph {
            for v in t {
                let (x, y) = (v[0] * g, v[1] * g);
                let d = (x.abs() - c).hypot(y.abs() - c);
                assert!(d > s.rivet() + 0.02, "glyph point {x},{y} touches a rivet");
            }
        }
    }

    /// Small enough for many boxes on Low.
    #[test]
    fn decor_triangle_count() {
        let m = decor(&shape());
        assert!(m.triangles() <= 600, "{}", m.triangles());
        assert_eq!(body(&shape()).triangles(), 44);
    }
}
