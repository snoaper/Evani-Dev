//! The track as continuous meshes (docs/GEOCHECK.md, "Continuous spline
//! meshes"): the road, the grass shoulder with its bank, the kerbs, the
//! rails and the painted lines are each swept along the track's own
//! centreline ([`Track::point_at`], the same points the karts drive on) into
//! one mesh per material. One continuous surface has no seams, so nothing
//! overlaps where straight pieces used to meet on bends and slopes.
//!
//! * The cross-section changes along the track (the bank's slope runs out
//!   further where the road is high), so the sweep here takes a profile per
//!   ring; the engine's `spline::sweep` keeps one profile.
//! * Red and white alternate every [`PIECE_RINGS`] rings on the kerbs and
//!   rails: each colour is its own mesh of short closed pieces that meet
//!   their neighbours face to face.
//! * On the inside of a tight bend everything stays inside [`FOLD`] of the
//!   bend's radius (a smooth envelope), so no ring folds back over the one
//!   before it: the shoulder, the bank and the rail come in closer there.
//! * Gentle stretches leave out every other ring ([`kept`], within
//!   [`SAG`]); every mesh uses the same rings, so their seams still meet.
//! * The painted lines are a decal mesh ([`evani_render::RenderWorld::draw_decal`]):
//!   one-sided tops (no underside) [`ROAD_CLEAR`] over the road mesh's own
//!   triangles, kept in front of the asphalt by that gap and the decal depth
//!   offset together.

use evani_render::mesh::Mesh;

use crate::draw::{BANK_FOOT_STEP, BANK_RUN_MAX, BANK_SLOPE, BANK_SLOPE_MAX};
use crate::track::Track;
pub use crate::trackgeo::{FOLD, KERB_W, RAIL_OUT, RAIL_W, RING_STEP, limits, rail_lat, rings};

/// Mesh slots of [`build`].
pub const ROAD: usize = 0;
pub const GRASS: usize = 1;
pub const RED: usize = 2;
pub const WHITE: usize = 3;
pub const PAINT: usize = 4;
/// The start line's chequered squares (decals), in four meshes so that no
/// two squares of one mesh touch (diagonal neighbours share an edge, which
/// would make it non-manifold): slot `CHEQ + 2 * row + dark`, with `dark`
/// 0 for the light squares and 1 for the dark ones.
pub const CHEQ: usize = 5;
pub const CHEQ_MESHES: usize = 4;
pub const COUNT: usize = CHEQ + CHEQ_MESHES;

/// Rings per kerb and rail piece (one colour each, about 4 m).
pub const PIECE_RINGS: usize = 2;
/// The road slab's thickness (m); the shoulder is cut out under it.
pub const ROAD_THICK: f32 = 0.3;
/// The grass shoulder's top under the road's height (m).
pub const SHOULDER_DROP: f32 = 0.06;
/// Kerb width beside the road edge and top over the road (m).
pub const KERB_TOP: f32 = 0.06;
/// How far kerbs and rails reach down into the grass shoulder they stand
/// on (m): their bottoms never lie in the shoulder's top face.
pub const SEAT: f32 = 0.02;
/// How far the kerbs reach in over the road's edge (m): their inner face
/// never lies in the plane of the grass's face under the road there. Over
/// 5 cm (geocheck's `zfight_gap`), so the two faces are clear of each other
/// at any distance the depth buffer can't separate them, not just near.
pub const KERB_IN: f32 = 0.06;
/// How far the grass reaches up into the road slab from under it (m): its
/// face there is buried in the road, at least 10 cm over the meadow (the
/// road stays 30 cm over it on this track), never level with it.
pub const UNDER_ROAD: f32 = 0.1;
/// Rail top over the road (m); its centre and width are in [`crate::trackgeo`].
pub const RAIL_TOP: f32 = 0.69;
/// Half thickness of the painted marks (m): the painted lines and chequer
/// are one-sided tops now, so it only sizes the other decals' marker
/// thickness ([`crate::draw::DECAL_THICK`]).
pub const PAINT_HALF: f32 = 0.005;
/// Painted edge lines: centre inset from the road edge and width (m).
pub const EDGE_LINE_IN: f32 = 0.35;
pub const LINE_W: f32 = 0.22;
/// Centre-line dash length and width (m).
pub const DASH_LEN: f32 = 2.2;
pub const DASH_W: f32 = 0.25;
/// The start strip's half length along the track (m): the edge lines stop
/// short of it so the two decals never lie on each other.
pub const START_STRIP_HALF: f32 = 0.75;
/// The chequered strip's half length along the track (m): it ends 5 cm
/// short of where the edge lines begin, measured along the road's curve like
/// the lines, so the two decals never lie over each other.
pub const CHEQ_HALF: f32 = START_STRIP_HALF - 0.05;
/// Squares across the road (two rows along it).
pub const CHEQ_CELLS: usize = 10;

/// A rectangle `lat0..lat1` (lateral, left positive) by `y0..y1`, as a
/// counter-clockwise profile in (right, up).
fn rect(lat0: f32, lat1: f32, y0: f32, y1: f32) -> Vec<[f32; 2]> {
    let (x0, x1) = (-lat0.max(lat1), -lat0.min(lat1));
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f32; 3]) -> Option<[f32; 3]> {
    let l = dot(a, a).sqrt();
    (l > 1e-9).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// Sweep a cross-section that may change from ring to ring into `m`.
/// `profile(j)` is ring `j`'s closed outline, counter-clockwise in (right,
/// up) with x the distance to the right of the centreline and y the height
/// over the road there; every ring has the same number of points. A loop
/// (`closed`) joins the last ring to the first; an open run gets end caps
/// (its outline must be convex). The result is closed, manifold and wound
/// outward, each outline edge flat across and smooth along the track.
pub fn sweep(m: &mut Mesh, track: &Track, ss: &[f32], closed: bool, profile: &dyn Fn(usize) -> Vec<[f32; 2]>) {
    let nf = ss.len();
    if nf < 2 {
        return;
    }
    let outlines: Vec<Vec<[f32; 2]>> = (0..nf).map(profile).collect();
    let np = outlines[0].len();
    if np < 3 || outlines.iter().any(|o| o.len() != np) {
        return;
    }
    let ring: Vec<Vec<[f32; 3]>> = (0..nf)
        .map(|j| {
            outlines[j]
                .iter()
                .map(|p| {
                    let c = track.point_at(ss[j], -p[0]);
                    [c[0], c[1] + p[1], c[2]]
                })
                .collect()
        })
        .collect();
    let frame = |j: usize| {
        let t = track.tangent_at(ss[j]);
        ([t[0], 0.0, t[1]], [-t[1], 0.0, t[0]])
    };
    let rings = if closed { nf + 1 } else { nf };
    for e in 0..np {
        let q = (e + 1) % np;
        let base = m.vertices.len() as u32;
        for r in 0..rings {
            let j = r % nf;
            let (prev, next) = if closed { ((j + nf - 1) % nf, (j + 1) % nf) } else { (j.saturating_sub(1), (j + 1).min(nf - 1)) };
            let (pp, pq) = (ring[j][e], ring[j][q]);
            let along = sub(
                [(ring[next][e][0] + ring[next][q][0]) * 0.5, (ring[next][e][1] + ring[next][q][1]) * 0.5, (ring[next][e][2] + ring[next][q][2]) * 0.5],
                [(ring[prev][e][0] + ring[prev][q][0]) * 0.5, (ring[prev][e][1] + ring[prev][q][1]) * 0.5, (ring[prev][e][2] + ring[prev][q][2]) * 0.5],
            );
            // The outline's own outward normal, turned into the world...
            let (a, b) = (outlines[j][e], outlines[j][q]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt().max(1e-12);
            let (_, right) = frame(j);
            let flat = [right[0] * dy / l, -dx / l, right[2] * dy / l];
            // ...tilted with the slope: the face's own normal when it has one.
            let n = match norm(cross(along, sub(pq, pp))) {
                Some(n) if dot(n, flat) > 0.0 => n,
                _ => norm(flat).unwrap_or([0.0, 1.0, 0.0]),
            };
            for v in [pp, pq] {
                m.vertices.push([v[0], v[1], v[2], n[0], n[1], n[2]]);
            }
        }
        for r in 0..rings as u32 - 1 {
            let a = base + 2 * r;
            let (b, d, c) = (a + 1, a + 2, a + 3);
            m.indices.extend_from_slice(&[a, d, c, a, c, b]);
        }
    }
    if !closed {
        for (j, forward) in [(0, false), (nf - 1, true)] {
            let (t, _) = frame(j);
            let n = if forward { t } else { [-t[0], 0.0, -t[2]] };
            let base = m.vertices.len() as u32;
            for v in &ring[j] {
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
}

/// How far the shoulder's top reaches out (`edge`) and where its bank's
/// slope meets the meadow (`foot`), each side `[left, right]`, and the
/// heights (over the road) of the shoulder top, the slope's foot and the
/// bank's bottom, at ring `j`.
pub struct Bank {
    pub edge: [f32; 2],
    pub foot: [f32; 2],
    pub top: f32,
    pub foot_y: f32,
    pub bottom: f32,
}

/// How far every decal on the asphalt (the painted lines, the start
/// chequer, the wear marks, the grid brackets, the item box discs) lies over
/// the road mesh's own triangles at its lowest point (m). Marks laid on the
/// smooth centre line instead dipped under the mesh's flat quads in places
/// and flickered against the asphalt. With reversed-Z depth the buffer
/// separates surfaces far finer than a millimetre out to 150 m, so 1 mm is
/// clear of it. More is not better: at 5 mm the ink outline pass read the
/// step at a mark's edge as a crease and drew a dark line round the road
/// repair patches.
pub const ROAD_CLEAR: f32 = 0.001;

/// The height of the road mesh's top at (`x`, `z`), on the triangles the
/// mesh really has: the quad between kept rings `ring[i]` and `ring[i + 1]`
/// is split along its diagonal from (ring `i`, right edge) to (ring `i + 1`,
/// left edge), as [`sweep`] builds it. `ring` holds the kept rings'
/// distances, ascending, closed by the track length (see
/// [`crate::roadmarks::mesh_rings`]); only the quads within two of the one
/// at `s_hint` are searched. `None` off the road.
pub fn road_top(track: &Track, ring: &[f32], x: f32, z: f32, s_hint: f32) -> Option<f32> {
    let n = ring.len().checked_sub(1)?;
    if n == 0 {
        return None;
    }
    let hw = track.half_width;
    let s = track.wrap_s(s_hint);
    let i0 = ring.partition_point(|&r| r <= s).saturating_sub(1).min(n - 1);
    // Barycentric weights of (x, z) in the triangle (a, b, c), over the
    // ground; `None` outside it (with a hair of slack on the edges).
    let inside = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| -> Option<f32> {
        let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if det.abs() < 1e-9 {
            return None;
        }
        let wa = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / det;
        let wb = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / det;
        let wc = 1.0 - wa - wb;
        let e = -1e-4;
        (wa >= e && wb >= e && wc >= e).then(|| wa * a[1] + wb * b[1] + wc * c[1])
    };
    for d in [0isize, 1, -1, 2, -2] {
        let i = (i0 as isize + d).rem_euclid(n as isize) as usize;
        let (s0, s1) = (ring[i], ring[i + 1]);
        // Right edge is lateral -hw, left +hw (the profile's x is to the right).
        let (a, b) = (track.point_at(s0, -hw), track.point_at(s0, hw));
        let (dd, c) = (track.point_at(s1, -hw), track.point_at(s1, hw));
        if let Some(h) = inside(a, dd, c).or_else(|| inside(a, c, b)) {
            return Some(h);
        }
    }
    None
}

/// `list` (ascending distances along the track, m) with every kept ring
/// that falls strictly inside one of its steps added, so a strip swept along
/// it bends where the road mesh does instead of cutting under a crest.
fn with_rings(track: &Track, list: &[f32], ring: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(list.len());
    for w in list.windows(2) {
        out.push(w[0]);
        for &r in ring {
            for r in [r - track.length, r, r + track.length] {
                if r > w[0] + 0.05 && r < w[1] - 0.05 {
                    out.push(r);
                }
            }
        }
    }
    out.extend(list.last());
    out.sort_by(f32::total_cmp);
    out
}

/// A painted strip from lateral `lat0` to `lat1` along `ss` into `m`: a
/// one-sided top facing up, every vertex [`ROAD_CLEAR`] over the road
/// mesh's triangles there (so there is no underside under the asphalt and
/// no slab whose top and bottom fight far off). Wound like [`sweep`]'s top
/// face (from the right edge to the left), with the same diagonal.
fn top_strip(m: &mut Mesh, track: &Track, ring: &[f32], ss: &[f32], lat0: f32, lat1: f32) {
    let ss = with_rings(track, ss, ring);
    if ss.len() < 2 {
        return;
    }
    let (right, left) = (lat0.min(lat1), lat0.max(lat1));
    let lift = |s: f32, lat: f32| {
        let p = track.point_at(s, lat);
        let y = road_top(track, ring, p[0], p[2], s).unwrap_or(p[1]);
        [p[0], y + ROAD_CLEAR, p[2]]
    };
    let mut pts: Vec<[[f32; 3]; 2]> = ss.iter().map(|&s| [lift(s, right), lift(s, left)]).collect();
    let nf = pts.len();
    // The road quad under a step is creased along its own diagonal, which
    // the strip's triangles don't share: where the crease is a ridge the
    // strip's middle dips towards it. Sample each step's two triangles and
    // lift both its ends by the step's largest shortfall (a point inside
    // then rises by at least that much).
    let need: Vec<f32> = (0..nf - 1)
        .map(|r| {
            let (a, b, d, c) = (pts[r][0], pts[r][1], pts[r + 1][0], pts[r + 1][1]);
            let s_mid = (ss[r] + ss[r + 1]) * 0.5;
            let mut most = 0.0f32;
            for [p0, p1, p2] in [[a, d, c], [a, c, b]] {
                for i in 0..=6 {
                    for k in 0..=6 - i {
                        let (u, w) = (i as f32 / 6.0, k as f32 / 6.0);
                        let p = [0, 1, 2].map(|e| p0[e] + (p1[e] - p0[e]) * u + (p2[e] - p0[e]) * w);
                        if let Some(h) = road_top(track, ring, p[0], p[2], s_mid) {
                            most = most.max(h + ROAD_CLEAR - p[1]);
                        }
                    }
                }
            }
            most
        })
        .collect();
    for (j, pair) in pts.iter_mut().enumerate() {
        let up = need.get(j).copied().unwrap_or(0.0).max(j.checked_sub(1).and_then(|r| need.get(r).copied()).unwrap_or(0.0));
        for v in pair.iter_mut() {
            v[1] += up;
        }
    }
    let base = m.vertices.len() as u32;
    for j in 0..nf {
        let (prev, next) = (j.saturating_sub(1), (j + 1).min(nf - 1));
        let mid = |k: usize| [(pts[k][0][0] + pts[k][1][0]) * 0.5, (pts[k][0][1] + pts[k][1][1]) * 0.5, (pts[k][0][2] + pts[k][1][2]) * 0.5];
        let along = sub(mid(next), mid(prev));
        let across = sub(pts[j][1], pts[j][0]);
        let n = norm(cross(along, across)).filter(|n| n[1] > 0.0).unwrap_or([0.0, 1.0, 0.0]);
        for v in pts[j] {
            m.vertices.push([v[0], v[1], v[2], n[0], n[1], n[2]]);
        }
    }
    for r in 0..nf as u32 - 1 {
        let a = base + 2 * r;
        let (b, d, c) = (a + 1, a + 2, a + 3);
        m.indices.extend_from_slice(&[a, d, c, a, c, b]);
    }
}

/// The shoulder and bank at distance `s` with the side limits `lim`.
pub fn bank(track: &Track, verge: f32, s: f32, lim: [f32; 2]) -> Bank {
    let hw = track.half_width;
    let road = track.point_at(s, 0.0)[1];
    let top = -SHOULDER_DROP;
    let foot_y = track.meadow_y + BANK_FOOT_STEP - road;
    let bottom = track.meadow_y - 0.3 - road;
    // As the old boxes and scenery::bank_reach: a gentle slope, never
    // longer than BANK_RUN_MAX, never steeper than BANK_SLOPE_MAX.
    let drop = (top - foot_y).max(0.005);
    let run = (drop / BANK_SLOPE).min(BANK_RUN_MAX).max(drop / BANK_SLOPE_MAX);
    let nominal = hw + verge + 4.0;
    let side = |cap: f32| {
        let edge = nominal.min(cap - 0.4).max(hw + KERB_W + 0.1);
        let foot = (nominal + run).min(cap).max(edge + 0.1);
        (edge, foot)
    };
    let (le, lf) = side(lim[0]);
    let (re, rf) = side(lim[1]);
    Bank { edge: [le, re], foot: [lf, rf], top, foot_y: foot_y.min(top - 0.005), bottom }
}

/// Piece `k`'s colour: red on even pieces for the kerbs (the rails the
/// other way round, so a kerb and its rail never match).
pub fn kerb_red(k: usize) -> bool {
    k.is_multiple_of(2)
}

/// The centre line has no dashes near the start grid.
pub fn near_grid(track: &Track, s: f32) -> bool {
    s < 8.0 || s > track.length - track.start_offset - 26.0
}

/// How far (m, across the ground) leaving a ring out may move any point of
/// the cross-sections from where the track puts it.
pub const SAG: f32 = 0.05;

/// The lateral distances (left positive) that matter at ring `j`: the
/// bank's foot and edge, the rail's outer side, the road's edges and the
/// centre.
fn key_laterals(track: &Track, verge: f32, ss: &[f32], lim: &[[f32; 2]], j: usize) -> [f32; 9] {
    let hw = track.half_width;
    let b = bank(track, verge, ss[j], lim[j]);
    let r = [rail_lat(track, verge, lim[j][0]), rail_lat(track, verge, lim[j][1])];
    [b.foot[0], b.edge[0], r[0] + RAIL_W * 0.5, hw, 0.0, -hw, -(r[1] + RAIL_W * 0.5), -b.edge[1], -b.foot[1]]
}

/// Which rings the meshes use (indices into `ss`, in order): every even
/// ring (where kerb and rail pieces meet) and each odd ring whose points
/// stray more than [`SAG`] from halfway between its neighbours. On the
/// gentle stretches the pieces are one quad long, which keeps Low's
/// triangle budget; bends keep every ring.
pub fn kept(track: &Track, verge: f32, ss: &[f32], lim: &[[f32; 2]]) -> Vec<usize> {
    let n = ss.len();
    (0..n)
        .filter(|&j| {
            if j.is_multiple_of(2) {
                return true;
            }
            let (a, b) = (j - 1, (j + 1) % n);
            let (la, lb, lj) = (key_laterals(track, verge, ss, lim, a), key_laterals(track, verge, ss, lim, b), key_laterals(track, verge, ss, lim, j));
            (0..9).any(|i| {
                let (p, q, c) = (track.point_at(ss[a], la[i]), track.point_at(ss[b], lb[i]), track.point_at(ss[j], lj[i]));
                let (dx, dz) = ((p[0] + q[0]) * 0.5 - c[0], (p[2] + q[2]) * 0.5 - c[2]);
                dx * dx + dz * dz > SAG * SAG
            })
        })
        .collect()
}

/// Every track mesh, in the slots [`ROAD`] to [`PAINT`].
pub fn build(track: &Track, verge: f32) -> [Mesh; COUNT] {
    let mut out: [Mesh; COUNT] = Default::default();
    let hw = track.half_width;
    let all = rings(track);
    let n = all.len();
    let lim_all = limits(track, &all);
    let keep = kept(track, verge, &all, &lim_all);
    let is_kept: Vec<bool> = {
        let mut v = vec![false; n];
        for &j in &keep {
            v[j] = true;
        }
        v
    };
    let ss: Vec<f32> = keep.iter().map(|&j| all[j]).collect();
    let lim: Vec<[f32; 2]> = keep.iter().map(|&j| lim_all[j]).collect();
    // Road: one slab round the loop.
    sweep(&mut out[ROAD], track, &ss, true, &|_| rect(-hw, hw, -ROAD_THICK, 0.0));
    // Grass: the shoulder (cut out under the road) and each side's slope
    // down to its foot step, one closed outline round the loop.
    sweep(&mut out[GRASS], track, &ss, true, &|j| {
        let b = bank(track, verge, ss[j], lim[j]);
        let [le, re] = b.edge;
        let [lf, rf] = b.foot;
        vec![
            [-lf, b.bottom],
            [rf, b.bottom],
            [rf, b.foot_y],
            [re, b.top],
            [hw, b.top],
            [hw, -ROAD_THICK + UNDER_ROAD],
            [-hw, -ROAD_THICK + UNDER_ROAD],
            [-hw, b.top],
            [-le, b.top],
            [-lf, b.foot_y],
        ]
    });
    // Kerbs and rails, red and white by piece.
    for k in 0..n / PIECE_RINGS {
        // The piece's rings: its ends and the middle one where kept.
        let idx: Vec<usize> = (0..=PIECE_RINGS).map(|i| (k * PIECE_RINGS + i) % n).filter(|&j| is_kept[j]).collect();
        let piece: Vec<f32> = idx.iter().map(|&j| all[j]).collect();
        // The last piece ends on the loop's first ring.
        let piece: Vec<f32> = piece.iter().enumerate().map(|(i, &s)| if i > 0 && s < piece[0] { s + track.length } else { s }).collect();
        let lim = &lim_all;
        let (kerb, rail) = if kerb_red(k) { (RED, WHITE) } else { (WHITE, RED) };
        for side in [1.0f32, -1.0] {
            sweep(&mut out[kerb], track, &piece, false, &|_| rect(side * (hw - KERB_IN), side * (hw + KERB_W), -SHOULDER_DROP - SEAT, KERB_TOP));
            let si = if side > 0.0 { 0 } else { 1 };
            sweep(&mut out[rail], track, &piece, false, &|i| {
                let c = rail_lat(track, verge, lim[idx[i]][si]);
                rect(side * (c - RAIL_W * 0.5), side * (c + RAIL_W * 0.5), -SHOULDER_DROP - SEAT, RAIL_TOP)
            });
        }
    }
    // Painted lines (a decal): white edge lines that stop short of the
    // start strip, and a dashed centre line (none near the start grid).
    // One-sided tops [`ROAD_CLEAR`] over the road mesh (see [`top_strip`]).
    let mut ring = ss.clone();
    ring.push(track.length);
    let mut line: Vec<f32> = vec![START_STRIP_HALF];
    line.extend(ss.iter().copied().filter(|&s| s > START_STRIP_HALF + 0.5 && s < track.length - START_STRIP_HALF - 0.5));
    line.push(track.length - START_STRIP_HALF);
    for side in [1.0f32, -1.0] {
        let c = hw - EDGE_LINE_IN;
        top_strip(&mut out[PAINT], track, &ring, &line, side * (c - LINE_W * 0.5), side * (c + LINE_W * 0.5));
    }
    for k in (0..n / PIECE_RINGS).step_by(2) {
        let mid = all[k * PIECE_RINGS + PIECE_RINGS / 2];
        if near_grid(track, mid) {
            continue;
        }
        let dash = [mid - DASH_LEN * 0.5, mid, mid + DASH_LEN * 0.5];
        top_strip(&mut out[PAINT], track, &ring, &dash, -DASH_W * 0.5, DASH_W * 0.5);
    }
    // The start line's chequer: two rows of squares swept along the same
    // curve as the lines (so the lines start where the squares end, at any
    // bend), in the four decal meshes of [`CHEQ`], one-sided tops like the
    // lines.
    for c in 0..CHEQ_CELLS {
        let lat0 = (c as f32 / CHEQ_CELLS as f32 * 2.0 - 1.0) * hw;
        let lat1 = ((c + 1) as f32 / CHEQ_CELLS as f32 * 2.0 - 1.0) * hw;
        for row in 0..2 {
            let (a, b) = if row == 0 { (-CHEQ_HALF, 0.0) } else { (0.0, CHEQ_HALF) };
            let slot = CHEQ + 2 * row + (c + row) % 2;
            top_strip(&mut out[slot], track, &ring, &[a, b], lat0, lat1);
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    fn track() -> Track {
        Track::from_json(include_str!("../data/track.json")).unwrap()
    }

    /// Closed, manifold, consistently wound (by position); its volume.
    fn closed_volume(m: &Mesh) -> f32 {
        let key = |i: u32| {
            let v = m.vertices[i as usize];
            [(v[0] * 1e3).round() as i64, (v[1] * 1e3).round() as i64, (v[2] * 1e3).round() as i64]
        };
        let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
        let mut vol = 0.0f64;
        for t in m.indices.chunks(3) {
            for k in 0..3 {
                *edges.entry((key(t[k]), key(t[(k + 1) % 3]))).or_default() += 1;
            }
            let p = |i: u32| {
                let v = m.vertices[i as usize];
                [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            vol += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        for ((a, b), n) in &edges {
            assert_eq!(*n, 1, "edge {a:?}-{b:?} used {n} times one way");
            assert_eq!(edges.get(&(*b, *a)), Some(&1), "open edge {a:?}-{b:?}");
        }
        (vol / 6.0) as f32
    }

    /// The road mesh's top triangles (facing up), for brute-force heights.
    pub(crate) fn road_tops(road: &Mesh) -> Vec<[[f32; 3]; 3]> {
        road.indices
            .chunks(3)
            .map(|t| [0, 1, 2].map(|k| {
                let v = road.vertices[t[k] as usize];
                [v[0], v[1], v[2]]
            }))
            .filter(|[a, b, c]| {
                // Wound outward: the top's own normal points up.
                let n = cross(sub(*b, *a), sub(*c, *a));
                n[1] / dot(n, n).sqrt().max(1e-12) > 0.9
            })
            .collect()
    }

    /// The highest road triangle over (x, z), by brute force.
    pub(crate) fn height_over(tops: &[[[f32; 3]; 3]], x: f32, z: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for [a, b, c] in tops {
            let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if det.abs() < 1e-9 {
                continue;
            }
            let wa = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / det;
            let wb = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / det;
            let wc = 1.0 - wa - wb;
            if wa >= -1e-5 && wb >= -1e-5 && wc >= -1e-5 {
                let h = wa * a[1] + wb * b[1] + wc * c[1];
                best = Some(best.map_or(h, |o: f32| o.max(h)));
            }
        }
        best
    }

    #[test]
    fn every_track_mesh_is_closed_and_wound_outward() {
        let t = track();
        let meshes = build(&t, 7.0);
        for (i, m) in meshes.iter().enumerate() {
            assert!(m.triangles() > 0, "mesh {i} empty");
            assert!(m.vertices.iter().all(|v| v.iter().all(|c| c.is_finite())));
            // The painted lines and the chequer are one-sided tops.
            if i >= PAINT {
                continue;
            }
            assert!(closed_volume(m) > 0.0, "mesh {i} is inside out");
        }
        // The road slab: about length x width x thickness.
        let road = closed_volume(&meshes[ROAD]);
        let expect = t.length * 2.0 * t.half_width * ROAD_THICK;
        assert!((road - expect).abs() / expect < 0.05, "{road} vs {expect}");
    }

    /// `road_top` gives the height of the road mesh's real triangles, and
    /// the painted lines and chequer face up and lie [`ROAD_CLEAR`] over
    /// them everywhere (not just at their corners), never under the asphalt
    /// and never floating.
    #[test]
    fn paint_lies_just_over_the_road_triangles() {
        let t = track();
        let meshes = build(&t, 7.0);
        let tops = road_tops(&meshes[ROAD]);
        let ring = crate::roadmarks::mesh_rings(&t, 7.0);
        // road_top against the mesh, across the road and along it.
        let mut checked = 0;
        let mut s = 0.3;
        while s < t.length {
            for lat in [-3.4, -1.7, 0.0, 0.9, 3.4] {
                let p = t.point_at(s, lat);
                let want = height_over(&tops, p[0], p[2]);
                let got = road_top(&t, &ring, p[0], p[2], s);
                assert!(want.is_some() && got.is_some(), "no road at s {s} lat {lat}");
                if let (Some(w), Some(g)) = (want, got) {
                    assert!((w - g).abs() < 1e-4, "road_top {g} vs mesh {w} at s {s} lat {lat}");
                }
                checked += 1;
            }
            s += 1.7;
        }
        assert!(checked > 500);
        for (slot, m) in meshes.iter().enumerate().take(COUNT).skip(PAINT) {
            for tri in m.indices.chunks(3) {
                let v = tri.iter().map(|&i| m.vertices[i as usize]).collect::<Vec<_>>();
                let (a, b, c) = ([v[0][0], v[0][1], v[0][2]], [v[1][0], v[1][1], v[1][2]], [v[2][0], v[2][1], v[2][2]]);
                let n = cross(sub(b, a), sub(c, a));
                assert!(n[1] > 0.0, "slot {slot}: a triangle faces down");
                for (u, w) in (0..=6).flat_map(|i| (0..=6 - i).map(move |k| (i as f32 / 6.0, k as f32 / 6.0))) {
                    let p = [a[0] + (b[0] - a[0]) * u + (c[0] - a[0]) * w, a[1] + (b[1] - a[1]) * u + (c[1] - a[1]) * w, a[2] + (b[2] - a[2]) * u + (c[2] - a[2]) * w];
                    let Some(h) = height_over(&tops, p[0], p[2]) else {
                        // The chequer's outer corners lie on the road's
                        // curved edge, a hair outside its straight chord
                        // (under the kerb's lip, which reaches 6 cm in).
                        let lat = t.locate(p, 0).lateral.abs();
                        assert!(lat > t.half_width - 0.02, "slot {slot}: paint off the road at {p:?}");
                        continue;
                    };
                    // Never closer than ROAD_CLEAR; where the road quad's
                    // crease is a valley under it, a few millimetres more.
                    let gap = p[1] - h;
                    assert!(gap > ROAD_CLEAR - 0.0005 && gap < ROAD_CLEAR + 0.01, "slot {slot}: paint {gap} m over the road at {p:?}");
                }
            }
        }
    }

    #[test]
    fn rings_alternate_evenly_and_the_road_follows_the_driving_line() {
        let t = track();
        let ss = rings(&t);
        assert!(ss.len().is_multiple_of(2 * PIECE_RINGS));
        let road = &build(&t, 7.0)[ROAD];
        // Every top vertex lies on the track's own surface.
        for v in road.vertices.iter().filter(|v| v[4] > 0.9) {
            let at = t.locate([v[0], v[1], v[2]], 0);
            assert!((v[1] - at.height).abs() < 0.02, "road top {} vs track {}", v[1], at.height);
        }
    }

    /// Gentle stretches drop their odd rings, bends keep them, and every
    /// even ring stays (where the kerb and rail pieces meet).
    #[test]
    fn gentle_stretches_use_fewer_rings() {
        let t = track();
        let ss = rings(&t);
        let lim = limits(&t, &ss);
        let keep = kept(&t, 7.0, &ss, &lim);
        assert!((0..ss.len()).step_by(2).all(|j| keep.contains(&j)));
        let dropped = ss.len() - keep.len();
        assert!(dropped > ss.len() / 8 && dropped < ss.len() / 2, "{dropped} of {} rings dropped", ss.len());
        // The tightest bend keeps every ring.
        let reach = |j: usize| lim[j][0].min(lim[j][1]);
        let tightest = (0..ss.len()).min_by(|&a, &b| reach(a).total_cmp(&reach(b))).unwrap();
        let j = tightest | 1;
        assert!(keep.contains(&j), "ring {j} in the tightest bend was dropped");
    }
    /// No ring folds back over the one before: on every outline point the
    /// step to the next ring goes forward along the track.
    #[test]
    fn nothing_folds_on_the_inside_of_the_hairpins() {
        let t = track();
        for verge in [0.0, 7.0, 30.0] {
            let all = rings(&t);
            let lim_all = limits(&t, &all);
            let keep = kept(&t, verge, &all, &lim_all);
            let ss: Vec<f32> = keep.iter().map(|&j| all[j]).collect();
            let lim: Vec<[f32; 2]> = keep.iter().map(|&j| lim_all[j]).collect();
            let n = ss.len();
            let outline = |j: usize| -> Vec<f32> {
                let b = bank(&t, verge, ss[j], lim[j]);
                let r = [rail_lat(&t, verge, lim[j][0]), rail_lat(&t, verge, lim[j][1])];
                vec![b.edge[0], b.foot[0], -b.edge[1], -b.foot[1], r[0] + RAIL_W * 0.5, -(r[1] + RAIL_W * 0.5)]
            };
            for j in 0..n {
                let (a, b) = (outline(j), outline((j + 1) % n));
                let s1 = if j + 1 == n { t.length } else { ss[j + 1] };
                let tan = t.tangent_at((ss[j] + s1) * 0.5);
                for (la, lb) in a.iter().zip(&b) {
                    let (p, q) = (t.point_at(ss[j], *la), t.point_at(s1, *lb));
                    let fwd = (q[0] - p[0]) * tan[0] + (q[2] - p[2]) * tan[1];
                    assert!(fwd > 0.0, "verge {verge}: lateral {la} folds at s {}", ss[j]);
                }
            }
        }
    }
}
