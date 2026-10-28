//! Continuous spline meshes for tracks and walls: a Catmull-Rom path through
//! control points, sampled evenly by length, and a cross-section profile
//! swept along it into one mesh. One continuous surface has no seams, so no
//! overlapping slabs (z-fighting, overlaps) and no cracks between pieces.
//!
//! ```ignore
//! let path = Path::new(points, true);
//! let road = sweep(&path.frames(2.0), &Profile::slab(7.0, 0.3), path.closed);
//! let id = world.res_mut::<MeshLibrary>().add(road).unwrap();
//! rw.draw_mesh(MeshInstance::new(id, [0.0; 3]));   // every frame
//! ```
//! Drawing needs the renderer's `meshes` feature (custom meshes).

use crate::mesh::Mesh;

type V = [f32; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, s: f32) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn len(a: V) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn norm(a: V) -> V {
    let l = len(a);
    if l > 1e-12 { mul(a, 1.0 / l) } else { [0.0, 0.0, -1.0] }
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A Catmull-Rom path through control points.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub points: Vec<V>,
    /// A loop (a circuit): the last point joins the first.
    pub closed: bool,
}

/// A point on the path with its frame: `right` is horizontal (banking is
/// left to the profile), `up` completes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub pos: V,
    pub tangent: V,
    pub right: V,
    pub up: V,
    /// Distance along the path (m).
    pub s: f32,
}

impl Path {
    pub fn new(points: Vec<V>, closed: bool) -> Path {
        Path { points, closed }
    }

    fn segments(&self) -> usize {
        let n = self.points.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }

    fn ctrl(&self, i: isize) -> V {
        let n = self.points.len() as isize;
        if self.closed {
            self.points[i.rem_euclid(n) as usize]
        } else if i < 0 {
            // Mirror the ends so the curve starts and stops on them.
            sub(mul(self.points[0], 2.0), self.points[1])
        } else if i >= n {
            sub(mul(self.points[(n - 1) as usize], 2.0), self.points[(n - 2) as usize])
        } else {
            self.points[i as usize]
        }
    }

    /// Point at parameter `t` (0..segments; segment `k` runs from point `k`
    /// to point `k + 1`). Uniform Catmull-Rom: passes through every point.
    pub fn point(&self, t: f32) -> V {
        let segs = self.segments();
        if segs == 0 {
            return self.points.first().copied().unwrap_or([0.0; 3]);
        }
        let t = t.clamp(0.0, segs as f32);
        let k = (t.floor() as usize).min(segs - 1);
        let u = t - k as f32;
        let k = k as isize;
        let (p0, p1, p2, p3) = (self.ctrl(k - 1), self.ctrl(k), self.ctrl(k + 1), self.ctrl(k + 2));
        let (u2, u3) = (u * u, u * u * u);
        let mut out = [0.0; 3];
        for i in 0..3 {
            out[i] = 0.5
                * (2.0 * p1[i]
                    + (-p0[i] + p2[i]) * u
                    + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * u2
                    + (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * u3);
        }
        out
    }

    /// Frames every `step` metres (about; the last step is stretched to end
    /// exactly at the end, or at the start of a loop, which is not repeated).
    pub fn frames(&self, step: f32) -> Vec<Frame> {
        let segs = self.segments();
        if segs == 0 {
            return Vec::new();
        }
        // Fine polyline, then resample by length.
        let fine_n = segs * 32;
        let fine: Vec<V> = (0..=fine_n).map(|i| self.point(i as f32 / 32.0)).collect();
        let mut acc = vec![0.0f32; fine.len()];
        for i in 1..fine.len() {
            acc[i] = acc[i - 1] + len(sub(fine[i], fine[i - 1]));
        }
        let total = acc[acc.len() - 1];
        let n = ((total / step.max(1e-3)).round() as usize).max(1);
        let count = if self.closed { n } else { n + 1 };
        let mut out = Vec::with_capacity(count);
        let mut j = 0;
        for i in 0..count {
            let s = total * i as f32 / n as f32;
            while j + 1 < acc.len() - 1 && acc[j + 1] < s {
                j += 1;
            }
            let span = (acc[j + 1] - acc[j]).max(1e-9);
            let f = ((s - acc[j]) / span).clamp(0.0, 1.0);
            let pos = add(fine[j], mul(sub(fine[j + 1], fine[j]), f));
            // Tangent from the neighbouring fine points (wraps on a loop).
            let (a, b) = if self.closed {
                let m = fine.len() - 1;
                (fine[(j + m - 1) % m], fine[(j + 2) % m])
            } else {
                (fine[j.saturating_sub(1)], fine[(j + 2).min(fine.len() - 1)])
            };
            let tangent = norm(sub(b, a));
            let right = norm(cross(tangent, [0.0, 1.0, 0.0]));
            let up = cross(right, tangent);
            out.push(Frame {
                pos,
                tangent,
                right,
                up,
                s,
            });
        }
        out
    }
}

/// A closed cross-section, counter-clockwise in (right, up) coordinates
/// (x lateral, y height), swept along a path. Convex profiles get end caps
/// on open paths.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub points: Vec<[f32; 2]>,
}

impl Profile {
    /// A road slab: `half_width` either side, top at the path, `thickness`
    /// below it.
    pub fn slab(half_width: f32, thickness: f32) -> Profile {
        let (w, t) = (half_width, thickness);
        Profile {
            points: vec![[-w, -t], [w, -t], [w, 0.0], [-w, 0.0]],
        }
    }

    /// A wall or rail `width` wide and `height` tall, centred `offset` to
    /// the right of the path (negative: left), standing on the path height.
    pub fn wall(offset: f32, width: f32, height: f32) -> Profile {
        let (a, b) = (offset - width * 0.5, offset + width * 0.5);
        Profile {
            points: vec![[a, 0.0], [b, 0.0], [b, height], [a, height]],
        }
    }
}

/// Sweep `profile` along `frames` into one mesh (world space: draw it with
/// an identity instance). Each profile edge is flat across (hard edges) and
/// smooth along the path. A loop (`closed`) joins its last ring to the
/// first with no seam; an open run gets end caps. The result is closed,
/// manifold and wound outward (counter-clockwise from outside).
pub fn sweep(frames: &[Frame], profile: &Profile, closed: bool) -> Mesh {
    let mut m = Mesh::default();
    let pts = &profile.points;
    let np = pts.len();
    let nf = frames.len();
    if np < 3 || nf < 2 {
        return m;
    }
    let at = |f: &Frame, p: [f32; 2]| add(f.pos, add(mul(f.right, p[0]), mul(f.up, p[1])));
    let rings = if closed { nf + 1 } else { nf };
    for e in 0..np {
        let (p, q) = (pts[e], pts[(e + 1) % np]);
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-12);
        let n2 = [dy / l, -dx / l];
        let base = m.vertices.len() as u32;
        for r in 0..rings {
            let f = &frames[r % nf];
            let n = norm(add(mul(f.right, n2[0]), mul(f.up, n2[1])));
            for c in [p, q] {
                let v = at(f, c);
                m.vertices.push([v[0], v[1], v[2], n[0], n[1], n[2]]);
            }
        }
        for r in 0..rings as u32 - 1 {
            let a = base + 2 * r; // ring r, p
            let b = a + 1; // ring r, q
            let d = a + 2; // ring r+1, p
            let c = a + 3; // ring r+1, q
            m.indices.extend_from_slice(&[a, d, c, a, c, b]);
        }
    }
    if !closed {
        // End caps: a fan over the (convex) profile, facing back at the
        // start and forward at the end.
        for (f, forward) in [(&frames[0], false), (&frames[nf - 1], true)] {
            let n = if forward { f.tangent } else { mul(f.tangent, -1.0) };
            let base = m.vertices.len() as u32;
            for &p in pts {
                let v = at(f, p);
                m.vertices.push([v[0], v[1], v[2], n[0], n[1], n[2]]);
            }
            for i in 1..np as u32 - 1 {
                if forward {
                    m.indices.extend_from_slice(&[base, base + i + 1, base + i]);
                } else {
                    m.indices.extend_from_slice(&[base, base + i, base + i + 1]);
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Closed, manifold, consistently wound (by position) and its volume.
    fn closed_volume(m: &Mesh) -> f32 {
        let key = |i: u32| {
            let v = m.vertices[i as usize];
            [(v[0] * 1e3).round() as i64, (v[1] * 1e3).round() as i64, (v[2] * 1e3).round() as i64]
        };
        let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
        let mut vol = 0.0;
        for t in m.indices.chunks(3) {
            for k in 0..3 {
                let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
                *edges.entry((a, b)).or_default() += 1;
            }
            let p = |i: u32| {
                let v = m.vertices[i as usize];
                [v[0], v[1], v[2]]
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            vol += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        for ((a, b), n) in &edges {
            assert_eq!(*n, 1, "edge used {n} times in one direction");
            assert_eq!(edges.get(&(*b, *a)), Some(&1), "open edge");
        }
        vol / 6.0
    }

    fn circle(r: f32, n: usize) -> Vec<V> {
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * core::f32::consts::TAU;
                [r * a.cos(), 0.0, r * a.sin()]
            })
            .collect()
    }

    #[test]
    fn catmull_rom_passes_through_its_points() {
        let p = Path::new(vec![[0.0; 3], [10.0, 0.0, 0.0], [10.0, 0.0, 10.0], [0.0, 2.0, 10.0]], false);
        for (i, q) in p.points.iter().enumerate() {
            let x = p.point(i as f32);
            assert!(len(sub(x, *q)) < 1e-5, "{i}: {x:?}");
        }
    }

    #[test]
    fn frames_are_even_and_orthonormal() {
        let p = Path::new(circle(50.0, 12), true);
        let f = p.frames(2.0);
        let total = 2.0 * core::f32::consts::PI * 50.0;
        assert!((f.len() as f32 - total / 2.0).abs() <= 2.0, "{}", f.len());
        for w in f.windows(2) {
            let d = len(sub(w[1].pos, w[0].pos));
            assert!((d - 2.0).abs() < 0.1, "{d}");
        }
        for fr in &f {
            assert!((len(fr.tangent) - 1.0).abs() < 1e-4 && (len(fr.right) - 1.0).abs() < 1e-4);
            assert!(fr.right[1].abs() < 1e-5, "right stays horizontal");
            let d = fr.tangent[0] * fr.right[0] + fr.tangent[2] * fr.right[2];
            assert!(d.abs() < 1e-4);
        }
    }

    #[test]
    fn a_looped_road_is_one_closed_outward_mesh() {
        let p = Path::new(circle(50.0, 16), true);
        let frames = p.frames(2.0);
        let m = sweep(&frames, &Profile::slab(7.0, 0.3), true);
        let v = closed_volume(&m);
        // About ring length x 14 m x 0.3 m.
        let expect = 2.0 * core::f32::consts::PI * 50.0 * 14.0 * 0.3;
        assert!(v > 0.0, "wound outward");
        assert!((v - expect).abs() / expect < 0.03, "{v} vs {expect}");
        // The top faces up.
        let top = m.vertices.iter().filter(|v| v[4] > 0.99).count();
        assert!(top > 0);
    }

    #[test]
    fn an_open_wall_has_caps_and_is_closed() {
        let p = Path::new(vec![[0.0; 3], [20.0, 0.0, 0.0], [40.0, 0.0, 10.0]], false);
        let m = sweep(&p.frames(1.0), &Profile::wall(8.0, 0.5, 1.0), false);
        assert!(closed_volume(&m) > 0.0);
        // A wall 8 m to the right of a path going +x is at z = +8 there
        // (right of +x is +z with y up).
        let first = m.vertices[0];
        assert!((first[2] - 7.75).abs() < 0.3, "{first:?}");
    }
}
