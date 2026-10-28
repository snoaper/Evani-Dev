//! The scenery's own low-poly meshes, built once with the statics and
//! registered after the track meshes ([`crate::trackmesh`]): an eight-sided
//! pole for sticks, posts and trunks (half the triangles of the built-in
//! 16-sided cylinder), a ten-sided dome for mushroom caps, cupcake frosting
//! and clouds, a small dome for sprinkles and windows, a faceted cupcake
//! wrapper and a cherry, a grass blob, a shadow disc and the lollipop swirls'
//! ring washers. The first five are closed convex solids, wound outward, so
//! geocheck measures overlaps on them exactly; the blob and the disc are
//! one-sided tops (see [`BLOB`], [`DISC`]) and the washers are open round
//! their holes (see [`WASHERS`]).

use evani_render::MeshId;
use evani_render::mesh::{Mesh, cylinder, dome, sphere};

/// Eight-sided cylinder of radius 0.5 along Y, -0.5..0.5 (32 triangles).
pub const POLE: usize = 0;
/// Dome of radius 0.5 on y = 0, 10 segments and 3 rings (60 triangles).
pub const CAP: usize = 1;
/// Dome of radius 0.5 on y = 0, 6 segments and 2 rings (24 triangles).
pub const DOT: usize = 2;
/// Twelve flat facets flaring from radius [`WRAPPER_FOOT`] at y = 0 to 0.5
/// at y = 1, closed top and bottom (44 triangles): a cupcake's pleated case.
pub const WRAPPER: usize = 3;
/// Sphere of radius 0.5, 8 segments and 5 rings (64 triangles).
pub const CHERRY: usize = 4;
/// A grass blob: a flat lobed patch reaching at most 0.5 from its middle,
/// rising from its rim at y = 0 to 1 in the middle, with no underside
/// ([`BLOB_RIM`] triangles). Drawn very flat, as a decal on the meadow: a
/// closed blob this thin showed its dark underside through the top from afar
/// (the engine draws back faces), so it is a one-sided top.
pub const BLOB: usize = 5;
/// A shadow disc: a flat 12-sided fan of radius 0.5 on y = 0, facing up,
/// with no underside ([`DISC_SIDES`] triangles). The discs under props,
/// item boxes and stands lie on the ground as decals; a closed disc a
/// centimetre thick showed its dark underside through its top from afar.
pub const DISC: usize = 6;
/// The lollipop swirls' outer and middle rings, as washers
/// ([`WASHER_SIDES`]-sided rings of radius 0.5, y -0.5..0.5, with a hole
/// [`SWIRL_HOLES`] of that wide and no wall round the hole): lollipop tree
/// outer and middle, lollipop outer and middle. Each hole is exactly the
/// next ring's outline (the built-in 16-sided cylinder, whose body fills
/// it), so no ring has a hidden cap 6 cm under the face of the ring in
/// front of it; those caps fought for depth far off.
pub const WASHERS: usize = 7;
/// How many swirl washers there are.
pub const WASHER_COUNT: usize = 4;
/// How many meshes there are.
pub const COUNT: usize = WASHERS + WASHER_COUNT;
/// Sides of [`DISC`].
pub const DISC_SIDES: u32 = 12;
/// Sides of the swirl washers: the built-in cylinder's.
pub const WASHER_SIDES: u32 = evani_render::mesh::CYLINDER_SIDES;
/// Each swirl washer's hole, as a fraction of its width: the next ring's
/// width over its own ([`crate::scenery::TREE_SWIRL`],
/// [`crate::scenery::POP_SWIRL`]).
pub const SWIRL_HOLES: [f32; WASHER_COUNT] = [
    crate::scenery::TREE_SWIRL[1] / crate::scenery::TREE_SWIRL[0],
    crate::scenery::TREE_SWIRL[2] / crate::scenery::TREE_SWIRL[1],
    crate::scenery::POP_SWIRL[1] / crate::scenery::POP_SWIRL[0],
    crate::scenery::POP_SWIRL[2] / crate::scenery::POP_SWIRL[1],
];
/// Rim points of [`BLOB`].
pub const BLOB_RIM: u32 = 16;

/// Segments and rings of [`CAP`] (its facets between two rings are flat).
pub const CAP_SEGMENTS: u32 = 10;
pub const CAP_RINGS: u32 = 3;
/// Sides of [`POLE`].
pub const POLE_SIDES: u32 = 8;
/// Facets of [`WRAPPER`].
pub const WRAPPER_FACETS: u32 = 12;
/// Radius of [`WRAPPER`] at its foot (its top is 0.5).
pub const WRAPPER_FOOT: f32 = 0.4;

/// The placeholder id of prop mesh `k` (after the track meshes; see
/// [`crate::draw::Statics`]).
pub fn id(k: usize) -> MeshId {
    crate::draw::track_mesh_id(crate::trackmesh::COUNT + k)
}

/// Every prop mesh, in id order.
pub fn build() -> Vec<Mesh> {
    let mut v = vec![cylinder(POLE_SIDES), dome(CAP_SEGMENTS, CAP_RINGS), dome(6, 2), wrapper(), sphere(8, 5), blob(), disc()];
    v.extend(SWIRL_HOLES.iter().map(|&h| washer(h)));
    v
}

/// The shadow disc: a fan from the middle to a 12-sided rim, all on y = 0,
/// facing up, with nothing under it.
fn disc() -> Mesh {
    let n = DISC_SIDES;
    let mut m = Mesh::default();
    m.vertices.push([0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    for i in 0..n {
        let a = i as f32 / n as f32 * core::f32::consts::TAU;
        m.vertices.push([0.5 * a.cos(), 0.0, 0.5 * a.sin(), 0.0, 1.0, 0.0]);
    }
    for i in 0..n {
        let (a, b) = (1 + i, 1 + (i + 1) % n);
        // As the blob: middle, i+1, i faces up.
        m.indices.extend_from_slice(&[0, b, a]);
    }
    m
}

/// A swirl ring: the built-in cylinder (radius 0.5, y -0.5..0.5, its
/// corners at the same angles) with a hole `hole` of its width through the
/// middle and no wall round the hole: the outer wall, and a ring-shaped
/// cap top and bottom. The cylinder drawn in the hole fills it.
fn washer(hole: f32) -> Mesh {
    let n = WASHER_SIDES;
    let mut m = Mesh::default();
    let ring = |i: u32| {
        let a = i as f32 / n as f32 * core::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        (c, s)
    };
    // The outer wall, as the cylinder's sides.
    for i in 0..=n {
        let (x, z) = ring(i);
        m.vertices.push([x * 0.5, -0.5, z * 0.5, x, 0.0, z]);
        m.vertices.push([x * 0.5, 0.5, z * 0.5, x, 0.0, z]);
    }
    for i in 0..n {
        let (b0, t0, b1, t1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        m.indices.extend_from_slice(&[b0, t0, b1, b1, t0, t1]);
    }
    // The caps: an outer and an inner ring each, wound as the cylinder's
    // caps (the inner point stands in for its middle).
    let r = 0.5 * hole;
    for (y, ny) in [(0.5f32, 1.0f32), (-0.5, -1.0)] {
        let base = m.vertices.len() as u32;
        for i in 0..n {
            let (x, z) = ring(i);
            m.vertices.push([x * 0.5, y, z * 0.5, 0.0, ny, 0.0]);
            m.vertices.push([x * r, y, z * r, 0.0, ny, 0.0]);
        }
        for i in 0..n {
            let (o0, n0) = (base + 2 * i, base + 2 * i + 1);
            let (o1, n1) = (base + 2 * ((i + 1) % n), base + 2 * ((i + 1) % n) + 1);
            if ny > 0.0 {
                m.indices.extend_from_slice(&[n0, o1, o0, n0, n1, o1]);
            } else {
                m.indices.extend_from_slice(&[n0, o0, o1, n0, o1, n1]);
            }
        }
    }
    m
}

/// Radius of [`BLOB`]'s rim at angle `a` (rad): three soft lobes with a
/// smaller ripple, at most 0.5 and never under 0.3 (so the fans from the
/// middle never fold over).
pub fn blob_radius(a: f32) -> f32 {
    let f = |a: f32| 1.0 + 0.14 * (3.0 * a + 0.5).cos() + 0.07 * (5.0 * a + 2.0).cos();
    // 1.21 is above f's largest value.
    0.5 * f(a) / 1.21
}

/// The grass blob: a shallow fan up to the middle, ending on the lobed rim,
/// with nothing under it. Its normals point straight up, so it is lit like
/// the meadow it lies on.
fn blob() -> Mesh {
    let n = BLOB_RIM;
    let mut m = Mesh::default();
    let rim = |i: u32| {
        let a = i as f32 / n as f32 * core::f32::consts::TAU;
        let r = blob_radius(a);
        [r * a.cos(), 0.0, r * a.sin()]
    };
    m.vertices.push([0.0, 1.0, 0.0, 0.0, 1.0, 0.0]);
    for i in 0..n {
        let p = rim(i);
        m.vertices.push([p[0], p[1], p[2], 0.0, 1.0, 0.0]);
    }
    for i in 0..n {
        let (a, b) = (1 + i, 1 + (i + 1) % n);
        // Angle grows from +X towards +Z: seen from above, middle, i+1, i
        // runs counter-clockwise.
        m.indices.extend_from_slice(&[0, b, a]);
    }
    m
}

/// The cupcake case: flat-shaded facets (each its own four vertices and
/// normal, so the toon bands and the crease outlines pick out the pleats),
/// a fan on top and one underneath.
fn wrapper() -> Mesh {
    let n = WRAPPER_FACETS;
    let mut m = Mesh::default();
    let ring = |i: u32, r: f32, y: f32| {
        let a = i as f32 / n as f32 * core::f32::consts::TAU;
        [r * a.cos(), y, r * a.sin()]
    };
    // The facet's outward normal: its middle direction, tipped up by the
    // flare (radius grows 0.1 over a height of 1).
    let flare = (0.5 - WRAPPER_FOOT) * (core::f32::consts::PI / n as f32).cos();
    for i in 0..n {
        let a = (i as f32 + 0.5) / n as f32 * core::f32::consts::TAU;
        let l = (1.0 + flare * flare).sqrt();
        let nrm = [a.cos() / l, -flare / l, a.sin() / l];
        let base = m.vertices.len() as u32;
        for p in [ring(i, WRAPPER_FOOT, 0.0), ring(i + 1, WRAPPER_FOOT, 0.0), ring(i + 1, 0.5, 1.0), ring(i, 0.5, 1.0)] {
            m.vertices.push([p[0], p[1], p[2], nrm[0], nrm[1], nrm[2]]);
        }
        // Angle grows from +X towards +Z: seen from outside, foot i, top i,
        // top i+1 runs counter-clockwise.
        m.indices.extend_from_slice(&[base, base + 3, base + 2, base, base + 2, base + 1]);
    }
    for (y, r, ny) in [(1.0f32, 0.5f32, 1.0f32), (0.0, WRAPPER_FOOT, -1.0)] {
        let base = m.vertices.len() as u32;
        for i in 0..n {
            let p = ring(i, r, y);
            m.vertices.push([p[0], p[1], p[2], 0.0, ny, 0.0]);
        }
        for i in 1..n - 1 {
            if ny > 0.0 {
                m.indices.extend_from_slice(&[base, base + i + 1, base + i]);
            } else {
                m.indices.extend_from_slice(&[base, base + i, base + i + 1]);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Welded edge counts (positions on a 0.01 mm grid, as geocheck welds).
    fn edges(m: &Mesh) -> HashMap<([i64; 3], [i64; 3]), i32> {
        let key = |i: u32| {
            let v = m.vertices[i as usize];
            [(v[0] * 1e5).round() as i64, (v[1] * 1e5).round() as i64, (v[2] * 1e5).round() as i64]
        };
        let mut e = HashMap::new();
        for t in m.indices.chunks(3) {
            for k in 0..3 {
                let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
                // +1 one way, -1 the other: a closed, consistently wound
                // surface sums to zero on every edge.
                if a < b {
                    *e.entry((a, b)).or_insert(0) += 1;
                } else {
                    *e.entry((b, a)).or_insert(0) -= 1;
                }
            }
        }
        e
    }

    #[test]
    fn prop_meshes_are_closed_outward_and_cheap() {
        let ms = build();
        assert_eq!(ms.len(), COUNT);
        let tris: Vec<u32> = ms.iter().map(Mesh::triangles).collect();
        assert_eq!(tris, [32, 60, 24, 44, 64, BLOB_RIM, DISC_SIDES, 96, 96, 96, 96]);
        // The blob's rim stays within its half-unit reach and never pinches.
        for i in 0..360 {
            let r = blob_radius(i as f32 / 360.0 * core::f32::consts::TAU);
            assert!((0.3..=0.5).contains(&r), "{r}");
        }
        for (k, m) in ms.iter().enumerate() {
            if k == BLOB {
                // A one-sided top: only its rim is an open edge, and every
                // triangle faces up (there is no underside to show through).
                assert_eq!(edges(m).values().filter(|&&c| c != 0).count() as u32, BLOB_RIM, "the blob's only open edges are its rim");
                for t in m.indices.chunks(3) {
                    let p = |i: u32| m.vertices[i as usize];
                    let (a, b, d) = (p(t[0]), p(t[1]), p(t[2]));
                    let ny = (b[2] - a[2]) * (d[0] - a[0]) - (b[0] - a[0]) * (d[2] - a[2]);
                    assert!(ny > 0.0, "a blob triangle faces down: {t:?}");
                    assert!(m.vertices.iter().all(|v| v[4] == 1.0), "a blob normal is not straight up");
                }
                continue;
            }
            if k == DISC {
                // A one-sided top like the blob: open only round its rim.
                assert_eq!(edges(m).values().filter(|&&c| c != 0).count() as u32, DISC_SIDES, "the disc's only open edges are its rim");
                for t in m.indices.chunks(3) {
                    let p = |i: u32| m.vertices[i as usize];
                    let (a, b, d) = (p(t[0]), p(t[1]), p(t[2]));
                    let ny = (b[2] - a[2]) * (d[0] - a[0]) - (b[0] - a[0]) * (d[2] - a[2]);
                    assert!(ny > 0.0, "a disc triangle faces down: {t:?}");
                }
                assert!(m.vertices.iter().all(|v| v[1] == 0.0 && v[4] == 1.0), "the disc is not flat and facing up");
                continue;
            }
            let open = edges(m).values().filter(|&&c| c != 0).count() as u32;
            if (WASHERS..WASHERS + WASHER_COUNT).contains(&k) {
                // Open only round the hole, top and bottom (outward below).
                assert_eq!(open, 2 * WASHER_SIDES, "washer {k} is open somewhere else or wound both ways");
            } else {
                assert_eq!(open, 0, "mesh {k} is open or wound both ways");
            }
            // Outward: every triangle faces away from the mesh's middle.
            let c = {
                let n = m.vertices.len() as f32;
                let s = m.vertices.iter().fold([0.0f32; 3], |a, v| [a[0] + v[0], a[1] + v[1], a[2] + v[2]]);
                [s[0] / n, s[1] / n, s[2] / n]
            };
            for t in m.indices.chunks(3) {
                let p = |i: u32| {
                    let v = m.vertices[i as usize];
                    [v[0], v[1], v[2]]
                };
                let (a, b, d) = (p(t[0]), p(t[1]), p(t[2]));
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
                let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                let area = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                assert!(area > 1e-7, "mesh {k}: a zero-area triangle");
                let out = [a[0] - c[0], a[1] - c[1], a[2] - c[2]];
                assert!(n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0, "mesh {k}: an inward triangle {t:?}");
                // The vertex normal agrees with the face.
                let vn = m.vertices[t[0] as usize];
                assert!(n[0] * vn[3] + n[1] * vn[4] + n[2] * vn[5] > 0.0, "mesh {k}: a normal against its face");
            }
        }
    }

    #[test]
    fn prop_meshes_follow_the_track_meshes() {
        assert_eq!(id(0), crate::draw::track_mesh_id(crate::trackmesh::COUNT));
    }

    #[test]
    fn each_washer_hole_is_the_next_rings_outline() {
        // The hole's corners are the built-in cylinder's, scaled to the next
        // ring's width: the ring drawn in it fills it exactly.
        let cyl = evani_render::mesh::cylinder(evani_render::mesh::CYLINDER_SIDES);
        let ms = build();
        for (w, &hole) in SWIRL_HOLES.iter().enumerate() {
            let m = &ms[WASHERS + w];
            for y in [0.5f32, -0.5] {
                let inner: Vec<[f32; 2]> = m.vertices.iter().filter(|v| v[1] == y && v[4] != 0.0 && (v[0].hypot(v[2]) - 0.5 * hole).abs() < 1e-5).map(|v| [v[0], v[2]]).collect();
                assert_eq!(inner.len() as u32, WASHER_SIDES, "washer {w}");
                for v in cyl.vertices.iter().filter(|v| v[1] == y && v[4] != 0.0 && v[0].hypot(v[2]) > 0.25) {
                    let want = [v[0] * hole, v[2] * hole];
                    assert!(inner.iter().any(|p| (p[0] - want[0]).abs() < 1e-6 && (p[1] - want[1]).abs() < 1e-6), "washer {w}: no hole corner at {want:?}");
                }
            }
        }
    }
}
