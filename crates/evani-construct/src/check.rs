//! Construct's own checks (docs/MODELGEN_PLAN.md 3.11). Construct knows
//! every piece, so it checks each one on its own before anything is merged:
//!
//! - **Check 1, per piece:** indices in range, no degenerate (zero-area)
//!   triangle, no duplicate triangle, unit normals, closed (every edge, by
//!   welded position, used once in each direction, so no hole and no flipped
//!   face) and outward (positive volume).
//! - **Check 4, fracture:** [`crate::model::check_split`] (halves of a split
//!   together equal the whole).
//!
//! Checks 2 (between pieces) and 3 (support) come with the hull package
//! (MG2), which first lays pieces against each other.

use crate::mesh::Mesh;
use crate::model::Model;
use crate::vec::*;

/// One problem found, on the piece with stable id `piece`.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub piece: String,
    pub what: String,
}

/// Welding tolerance for the closed-edge test (1 micrometre).
const WELD: f64 = 1e-6;

fn key(p: V3) -> [i64; 3] {
    [(p[0] / WELD).round() as i64, (p[1] / WELD).round() as i64, (p[2] / WELD).round() as i64]
}

/// Check 1 on one mesh; the problems in plain words (empty when sound).
pub fn piece(m: &Mesh) -> Vec<String> {
    let mut bad = Vec::new();
    let n = m.pos.len();
    if m.tris.is_empty() {
        bad.push("no triangles".to_string());
        return bad;
    }
    if m.nrm.len() != n || m.uv.len() != n {
        bad.push(format!("{} positions but {} normals and {} UVs", n, m.nrm.len(), m.uv.len()));
    }
    if m.tris.iter().flatten().any(|&v| v as usize >= n) {
        bad.push("a triangle points past the vertices".to_string());
        return bad;
    }
    if m.pos.iter().flatten().any(|x| !x.is_finite()) {
        bad.push("a position is not a finite number".to_string());
        return bad;
    }
    let bad_n = m.nrm.iter().filter(|q| (len(**q) - 1.0).abs() > 1e-6).count();
    if bad_n > 0 {
        bad.push(format!("{bad_n} normals are not unit length"));
    }
    let size = len(m.bounds().size()).max(1e-9);
    let mut degenerate = 0;
    let mut tri_keys: Vec<[[i64; 3]; 3]> = Vec::with_capacity(m.tris.len());
    let mut edges: Vec<([i64; 3], [i64; 3])> = Vec::with_capacity(m.tris.len() * 3);
    for t in &m.tris {
        let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
        if len(cross(sub(b, a), sub(c, a))) <= 1e-12 * size * size {
            degenerate += 1;
        }
        let k = [key(a), key(b), key(c)];
        // Rotate so the smallest corner leads: the same triangle, same winding.
        let r = (0..3).min_by_key(|&i| k[i]).unwrap_or(0);
        tri_keys.push([k[r], k[(r + 1) % 3], k[(r + 2) % 3]]);
        for i in 0..3 {
            edges.push((k[i], k[(i + 1) % 3]));
        }
    }
    if degenerate > 0 {
        bad.push(format!("{degenerate} degenerate (zero-area) triangles"));
    }
    tri_keys.sort_unstable();
    let dups = tri_keys.windows(2).filter(|w| w[0] == w[1]).count();
    if dups > 0 {
        bad.push(format!("{dups} duplicate triangles"));
    }
    // Closed: each directed edge's reverse is present as often as itself.
    let mut sorted = edges.clone();
    sorted.sort_unstable();
    let count = |e: &([i64; 3], [i64; 3])| sorted.partition_point(|x| x <= e) - sorted.partition_point(|x| x < e);
    let open = edges.iter().filter(|e| e.0 != e.1 && count(&(e.1, e.0)) != count(e)).count();
    if open > 0 {
        bad.push(format!("not closed: {open} edges without a matching opposite edge (a hole or a flipped face)"));
    } else if m.volume() <= 0.0 {
        bad.push(format!("inside out (volume {:.3e} m3)", m.volume()));
    }
    bad
}

/// Check 1 over every piece of a model, prototypes included.
pub fn pieces(m: &Model) -> Vec<Issue> {
    let mut out = Vec::new();
    let all = m.pieces.iter().chain(m.protos.iter().flat_map(|p| p.pieces.iter()));
    for p in all {
        for what in piece(&p.mesh) {
            out.push(Issue { piece: p.id.clone(), what });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{block, lathe_rings, ring_solid};

    #[test]
    fn sound_solids_pass_and_broken_ones_are_named() {
        let b = block([0.0; 3], QID, [0.5, 0.2, 0.1]);
        assert!(piece(&b).is_empty(), "{:?}", piece(&b));
        let dome = ring_solid(&lathe_rings(&[[0.0, 0.0], [0.3, 0.0], [0.2, 0.2], [0.0, 0.3]], 10, [0.0; 3]), true);
        assert!(piece(&dome).is_empty(), "{:?}", piece(&dome));
        // A hole: drop one triangle.
        let mut holed = b.clone();
        holed.tris.pop();
        assert!(piece(&holed).iter().any(|s| s.starts_with("not closed")));
        // Inside out.
        let mut inv = b.clone();
        inv.flip();
        assert!(piece(&inv).iter().any(|s| s.starts_with("inside out")));
        // A duplicate and a degenerate triangle.
        let mut dup = b.clone();
        dup.tris.push(dup.tris[0]);
        assert!(piece(&dup).iter().any(|s| s.contains("duplicate")));
        let mut deg = b;
        deg.tris.push([0, 0, 1]);
        assert!(piece(&deg).iter().any(|s| s.contains("degenerate")));
    }
}
