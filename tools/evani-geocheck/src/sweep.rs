//! Convex pieces of a swept mesh (`evani_render::spline::sweep`: a road,
//! kerb or wall built as one continuous mesh).
//!
//! A swept mesh is not convex (it bends along its path), so the exact convex
//! clipping the overlap check uses cannot take it whole. But it is a chain of
//! slabs: between two consecutive rings the cross-section is convex when the
//! profile is, so each segment is a convex polytope. [`decompose`] recognises
//! the layout `sweep` writes (one strip of quads per profile edge, the rings
//! of all strips in step, then the end caps) and returns one convex polytope
//! per segment. Anything else (an imported model, a hand-built ribbon) is
//! refused with a reason, and the caller keeps the mesh as a triangle surface.

use evani_render::mesh::Mesh;

use crate::geom::*;
use crate::shape::polys_volume;

/// A concave mesh split into convex pieces.
#[derive(Debug, Clone, Default)]
pub struct Decomposition {
    /// Convex polytopes (outward-wound polygons), one per ring-to-ring segment.
    pub pieces: Vec<Vec<Vec<V3>>>,
    /// Segments in the sweep (a segment with no volume, two rings on top of
    /// each other, is in this count but has no piece).
    pub segments: usize,
    /// Segments left out of `pieces` because they are not convex (a tight
    /// bend folding the profile over, or a concave profile).
    pub bent: usize,
}

fn centroid(pts: &[V3]) -> V3 {
    let mut c = [0.0; 3];
    for p in pts {
        c = add(c, *p);
    }
    mul(c, 1.0 / pts.len().max(1) as f64)
}

/// Newell normal (length = twice the area).
fn newell(pts: &[V3]) -> V3 {
    let mut n = [0.0; 3];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// The polygon wound so its normal points away from `inside` (`None` when it
/// has no area).
fn outward(mut poly: Vec<V3>, inside: V3, tiny: f64) -> Option<Vec<V3>> {
    let n = newell(&poly);
    if len(n) <= tiny {
        return None;
    }
    if dot(n, sub(centroid(&poly), inside)) < 0.0 {
        poly.reverse();
    }
    Some(poly)
}

/// Faces of the side quad `a b c d` (cyclic): one face when planar, else two
/// triangles split along the diagonal that keeps the piece convex.
fn quad_faces(q: [V3; 4], inside: V3, tiny: f64, flat: f64) -> Vec<Vec<V3>> {
    let n = newell(&q);
    let l = len(n);
    if l > tiny {
        let n = mul(n, 1.0 / l);
        if q.iter().all(|p| dot(n, sub(*p, q[0])).abs() <= flat) {
            return outward(q.to_vec(), inside, tiny).into_iter().collect();
        }
    }
    // Twisted: the diagonal a-c is right when d is behind plane (a, b, c).
    let first = outward(vec![q[0], q[1], q[2]], inside, tiny);
    let behind = first.as_ref().is_some_and(|t| {
        let n = norm(newell(t));
        dot(n, q[3]) - dot(n, t[0]) <= flat
    });
    let tris: [[V3; 3]; 2] = if behind {
        [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
    } else {
        [[q[0], q[1], q[3]], [q[1], q[2], q[3]]]
    };
    tris.into_iter().filter_map(|t| outward(t.to_vec(), inside, tiny)).collect()
}

/// Split a closed swept mesh into its convex segments, or say why it is not
/// a sweep this function understands.
pub fn decompose(m: &Mesh) -> Result<Decomposition, String> {
    let pos: Vec<V3> = m.vertices.iter().map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]).collect();
    let tris = m.indices.as_chunks::<3>().0;
    // Strips of quads: triangles (a, a+2, a+3) and (a, a+3, a+1); a strip
    // continues while the next quad starts two vertices on.
    let mut strips: Vec<Vec<u64>> = Vec::new();
    let mut i = 0;
    while i + 1 < tris.len() {
        let (t0, t1) = (tris[i], tris[i + 1]);
        let a = u64::from(t0[0]);
        let quad = u64::from(t0[1]) == a + 2 && u64::from(t0[2]) == a + 3 && u64::from(t1[0]) == a && u64::from(t1[1]) == a + 3 && u64::from(t1[2]) == a + 1;
        if !quad {
            break;
        }
        match strips.last_mut() {
            Some(s) if s.last().is_some_and(|l| l + 2 == a) => s.push(a),
            _ => strips.push(vec![a]),
        }
        i += 2;
    }
    let s = strips.len();
    if s < 3 {
        return Err("not a swept strip layout (no ring of quad strips)".into());
    }
    let q = strips[0].len();
    if strips.iter().any(|x| x.len() != q) {
        return Err("not a swept strip layout (strips of different lengths)".into());
    }
    let rest = tris.len() - i;
    if rest != 0 && rest != 2 * (s - 2) {
        return Err(format!("not a swept strip layout ({rest} triangles left over after the strips)"));
    }
    if strips.iter().any(|st| st.last().is_some_and(|a| a + 3 >= pos.len() as u64)) {
        return Err("indices past the vertices".into());
    }
    let size = Aabb::from_points(&pos).max_extent().max(1e-9);
    let join_tol = 1e-4 + 1e-5 * size;
    let at = |a: u64| pos[a as usize];

    let mut out = Decomposition {
        segments: q,
        ..Decomposition::default()
    };
    let mut sum = 0.0;
    for r in 0..q {
        // Ring r of every strip, the profile's points in order, and ring r + 1.
        let ring0: Vec<V3> = strips.iter().map(|st| at(st[r])).collect();
        let ring1: Vec<V3> = strips.iter().map(|st| at(st[r] + 2)).collect();
        // The strips must form one closed profile: strip e's far corner is
        // strip e + 1's near corner.
        for e in 0..s {
            if dist(at(strips[e][r] + 1), ring0[(e + 1) % s]) > join_tol || dist(at(strips[e][r] + 3), ring1[(e + 1) % s]) > join_tol {
                return Err("not a swept strip layout (the strips do not join into a profile loop)".into());
            }
        }
        let all: Vec<V3> = ring0.iter().chain(&ring1).copied().collect();
        let inside = centroid(&all);
        let ext = Aabb::from_points(&all).max_extent();
        let tiny = 1e-12 * ext.max(1e-9).powi(2);
        let flat = 1e-5 * ext.max(1e-9);
        let mut polys: Vec<Vec<V3>> = Vec::new();
        polys.extend(outward(ring0.clone(), inside, tiny));
        polys.extend(outward(ring1.clone(), inside, tiny));
        for strip in strips.iter().take(s) {
            let a = strip[r];
            polys.extend(quad_faces([at(a), at(a + 1), at(a + 3), at(a + 2)], inside, tiny, flat));
        }
        let vol = polys_volume(polys.iter().map(Vec::as_slice));
        if polys.len() < 4 || vol <= 1e-12 * ext.powi(3) {
            continue; // two rings on top of each other: nothing there
        }
        // Convex: every corner behind every face.
        let convex = polys.iter().all(|p| {
            let n = norm(newell(p));
            let d = dot(n, p[0]);
            all.iter().all(|c| dot(n, *c) - d <= flat)
        });
        if !convex {
            out.bent += 1;
            continue;
        }
        sum += vol;
        out.pieces.push(polys);
    }
    // The pieces must add up to the mesh, or the layout was misread (a bent
    // segment is allowed to be missing).
    if out.bent == 0 {
        let whole: f64 = tris
            .iter()
            .filter_map(|t| Some(dot(*pos.get(t[0] as usize)?, cross(*pos.get(t[1] as usize)?, *pos.get(t[2] as usize)?))))
            .sum::<f64>()
            / 6.0;
        if (sum - whole).abs() > 0.1 * whole.abs() + 1e-9 {
            return Err(format!("the segments' volume ({sum:.3} m3) does not match the mesh ({whole:.3} m3)"));
        }
    }
    if out.pieces.is_empty() {
        return Err("no segment of the sweep is a convex solid".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_render::spline::{Path, Profile, sweep};

    pub fn road(points: Vec<[f32; 3]>, closed: bool, profile: &Profile) -> Mesh {
        let path = Path::new(points, closed);
        sweep(&path.frames(2.0), profile, closed)
    }

    fn arc(r: f32, n: usize) -> Vec<[f32; 3]> {
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * core::f32::consts::TAU;
                [r * a.cos(), 0.0, r * a.sin()]
            })
            .collect()
    }

    #[test]
    fn a_straight_slab_splits_into_one_box_per_segment() {
        let m = road(vec![[0.0, 0.0, 0.0], [20.0, 0.0, 0.0]], false, &Profile::slab(3.0, 0.3));
        let d = decompose(&m).unwrap();
        assert_eq!(d.segments, 10);
        assert_eq!(d.pieces.len(), 10);
        assert_eq!(d.bent, 0);
        // 20 m x 6 m x 0.3 m.
        let v: f64 = d.pieces.iter().map(|p| polys_volume(p.iter().map(Vec::as_slice))).sum();
        assert!((v - 36.0).abs() < 1e-3, "{v}");
    }

    #[test]
    fn a_closed_loop_and_a_wall_decompose_and_add_up() {
        for (m, closed) in [
            (road(arc(40.0, 12), true, &Profile::slab(7.0, 0.3)), true),
            (road(arc(40.0, 12), true, &Profile::wall(8.0, 0.5, 1.0)), true),
            (road(vec![[0.0, 0.0, 0.0], [30.0, 2.0, 5.0], [60.0, 0.0, 30.0]], false, &Profile::wall(-3.0, 0.4, 1.2)), false),
        ] {
            let d = decompose(&m).unwrap_or_else(|e| panic!("closed {closed}: {e}"));
            assert!(d.bent == 0 && d.pieces.len() == d.segments, "closed {closed}: {} of {} (bent {})", d.pieces.len(), d.segments, d.bent);
        }
    }

    #[test]
    fn other_meshes_are_refused_with_a_reason() {
        let e = decompose(&evani_render::mesh::cube()).unwrap_err();
        assert!(e.contains("not a swept strip"), "{e}");
        assert!(decompose(&Mesh::default()).is_err());
    }
}
