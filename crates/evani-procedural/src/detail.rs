//! Realism detail on meshes: the shared steps that make a clean generated
//! shape read as a real, made or weathered thing. Each is small, seeded and
//! deterministic; assets chain them (subdivide, then displace, then split
//! into material masks).
//!
//! - [`subdivide`]: split long edges so relief has vertices to move
//!   (shared midpoints, so the mesh stays closed);
//! - [`displace`]: move vertices along their normal by a field (pores,
//!   dents, erosion); [`perturb`] moves them by a 3-D noise field, which
//!   never tears hard edges;
//! - [`split_by`]: the triangles sorted into parts by a mask (grass on the
//!   flats, rock on the steeps, snow on the tops), each part then in its own
//!   catalogue material;
//! - [`slope`], [`mask_edge`]: the usual masks, with a noisy edge so a
//!   material boundary meanders like a real one.

use std::collections::HashMap;

use evani_render::mesh::Mesh;

use crate::geo::smooth_normals;
use crate::noise::{fbm3p, smoothstep};

/// Split every triangle whose longest edge is over `max_edge` (m) into four,
/// repeatedly (at most `rounds` times), sharing midpoints so neighbours stay
/// joined. Normals are interpolated (renormalised).
pub fn subdivide(m: &mut Mesh, max_edge: f32, rounds: u32) {
    let max2 = max_edge.max(1e-4).powi(2);
    for _ in 0..rounds.min(8) {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut out = Vec::with_capacity(m.indices.len() * 2);
        let mut any = false;
        let tris: Vec<[u32; 3]> = m.indices.as_chunks::<3>().0.to_vec();
        for t in tris {
            let p = |i: u32| {
                let v = m.vertices[i as usize];
                [v[0], v[1], v[2]]
            };
            let d2 = |a: u32, b: u32| {
                let (x, y) = (p(a), p(b));
                (x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)
            };
            let long = d2(t[0], t[1]).max(d2(t[1], t[2])).max(d2(t[2], t[0]));
            if long <= max2 {
                out.extend_from_slice(&t);
                continue;
            }
            any = true;
            let mut midpoint = |a: u32, b: u32| -> u32 {
                let key = (a.min(b), a.max(b));
                *mid.entry(key).or_insert_with(|| {
                    let (va, vb) = (m.vertices[a as usize], m.vertices[b as usize]);
                    let mut v: [f32; 6] = std::array::from_fn(|k| (va[k] + vb[k]) * 0.5);
                    let n = crate::geo::normalize([v[3], v[4], v[5]]);
                    v[3] = n[0];
                    v[4] = n[1];
                    v[5] = n[2];
                    m.vertices.push(v);
                    (m.vertices.len() - 1) as u32
                })
            };
            let (ab, bc, ca) = (midpoint(t[0], t[1]), midpoint(t[1], t[2]), midpoint(t[2], t[0]));
            out.extend_from_slice(&[t[0], ab, ca, ab, t[1], bc, ca, bc, t[2], ab, bc, ca]);
        }
        m.indices = out;
        if !any {
            break;
        }
    }
}

/// Move each vertex along its normal by `f(position, normal)` (m), then
/// recompute smooth normals. For smooth, welded meshes (spheres, terrain,
/// boulders); a mesh with hard edges should use [`perturb`].
pub fn displace(m: &mut Mesh, f: impl Fn([f32; 3], [f32; 3]) -> f32) {
    for v in &mut m.vertices {
        let d = f([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
        v[0] += v[3] * d;
        v[1] += v[4] * d;
        v[2] += v[5] * d;
    }
    smooth_normals(m);
}

/// Move each vertex by a smooth 3-D noise field (`amount` m, features about
/// `size` m across). Vertices at one position move together, so split hard
/// edges stay closed; normals are kept (the move is small next to the
/// shape, only the silhouette and the light along it change).
pub fn perturb(m: &mut Mesh, amount: f32, size: f32, seed: u32) {
    let k = 1.0 / size.max(1e-3);
    for v in &mut m.vertices {
        let p = [v[0] * k, v[1] * k, v[2] * k];
        v[0] += amount * fbm3p(p, seed, 3, 0.5);
        v[1] += amount * fbm3p([p[0] + 31.7, p[1], p[2]], seed ^ 0x9e37, 3, 0.5);
        v[2] += amount * fbm3p([p[0], p[1] + 17.3, p[2]], seed ^ 0x7f4a, 3, 0.5);
    }
}

/// The triangles of `m` sorted into `n` meshes by `class(centroid, face
/// normal)` (an index under `n`; others are dropped). Vertices are copied per
/// part, normals kept, so each part shades exactly as the whole did.
pub fn split_by(m: &Mesh, n: usize, class: impl Fn([f32; 3], [f32; 3]) -> usize) -> Vec<Mesh> {
    let mut parts: Vec<Mesh> = (0..n).map(|_| Mesh::default()).collect();
    let mut maps: Vec<HashMap<u32, u32>> = (0..n).map(|_| HashMap::new()).collect();
    for t in m.indices.as_chunks::<3>().0 {
        let p: [[f32; 3]; 3] = std::array::from_fn(|i| {
            let v = m.vertices[t[i] as usize];
            [v[0], v[1], v[2]]
        });
        let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0, (p[0][2] + p[1][2] + p[2][2]) / 3.0];
        let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let fnrm = crate::geo::normalize([e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]);
        let k = class(c, fnrm);
        if k >= n {
            continue;
        }
        for &i in t {
            let at = *maps[k].entry(i).or_insert_with(|| {
                parts[k].vertices.push(m.vertices[i as usize]);
                (parts[k].vertices.len() - 1) as u32
            });
            parts[k].indices.push(at);
        }
    }
    parts
}

/// How steep a face is: 0 flat, 1 vertical.
pub fn slope(n: [f32; 3]) -> f32 {
    1.0 - n[1].clamp(-1.0, 1.0).abs()
}

/// A threshold with a noisy edge: true where `value` is over `at`, the
/// line moved by up to `wobble` with features about `size` m across at
/// point `p`, so a boundary (grass to rock, sand to soil) meanders.
pub fn mask_edge(value: f32, at: f32, wobble: f32, p: [f32; 3], size: f32, seed: u32) -> bool {
    let k = 1.0 / size.max(1e-3);
    value > at + wobble * fbm3p([p[0] * k, p[1] * k, p[2] * k], seed, 3, 0.55)
}

/// A soft 0..1 mask from `a` to `b` (shorthand for [`smoothstep`]).
pub fn ramp(a: f32, b: f32, x: f32) -> f32 {
    smoothstep(a, b, x)
}


/// Chip a box's edges: vertices of `m` within `width` (m) of an edge of the
/// box `size` (its base centre at `base`, its edges rounded by `bevel`, so
/// the rounded band counts as the edge) are pushed in along their normal,
/// up to `depth` (m), where a seeded cell field says a chip is (about
/// `density` chips per metre of edge). Corners chip most. For a
/// [`crate::build::rounded_box`] or any box-like mesh dense near its edges.
#[allow(clippy::too_many_arguments)]
pub fn chip_box_edges(m: &mut Mesh, size: [f32; 3], base: [f32; 3], bevel: f32, width: f32, depth: f32, density: f32, seed: u32) {
    if width <= 0.0 || depth <= 0.0 || density <= 0.0 {
        return;
    }
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let c = [base[0], base[1] + h[1], base[2]];
    for v in &mut m.vertices {
        let p = [v[0] - c[0], v[1] - c[1], v[2] - c[2]];
        // Distance in from each face; the edge distance is the second
        // smallest (two faces meet at an edge), a corner the third.
        let mut d = [h[0] - p[0].abs(), h[1] - p[1].abs(), h[2] - p[2].abs()];
        d.sort_by(|a, b| a.total_cmp(b));
        // A rounded edge's surface lies up to 0.29 bevel inside the sharp
        // edge, and its band is the bevel wide.
        let width = width + 0.4 * bevel;
        let edge = (d[1] - 0.29 * bevel).max(0.0);
        if edge > width {
            continue;
        }
        // The chip field along the edge: cells about 1/density m long.
        let q = [p[0] * density, p[1] * density, p[2] * density];
        let (f1, _, id) = crate::noise::cells2_id(q[0] + q[2] * 0.7 + 3.1, q[1] + q[2] * 0.4 - 1.7, seed);
        let chip = (1.0 - f1 * 1.6).max(0.0) * if id < 0.55 { 1.0 } else { 0.0 };
        let corner = if d[2] < width * 2.0 { 1.4 } else { 1.0 };
        let fall = (1.0 - edge / width).max(0.0);
        let push = depth * chip * corner * fall * fall.sqrt();
        v[0] -= v[3] * push;
        v[1] -= v[4] * push;
        v[2] -= v[5] * push;
    }
}

/// Merge vertices at the same position (within 0.01 mm), so a mesh built
/// from separate face grids (a rounded box) shades and displaces as one
/// surface. Normals are averaged; call [`smooth_normals`] after moving it.
pub fn weld(m: &mut Mesh) {
    let mut at: HashMap<[i64; 3], u32> = HashMap::new();
    let mut verts: Vec<[f32; 6]> = Vec::with_capacity(m.vertices.len());
    let remap: Vec<u32> = m
        .vertices
        .iter()
        .map(|v| {
            let key = [0, 1, 2].map(|k| (v[k] as f64 * 1e5).round() as i64);
            *at.entry(key).or_insert_with(|| {
                verts.push(*v);
                (verts.len() - 1) as u32
            })
        })
        .collect();
    for i in &mut m.indices {
        *i = remap[*i as usize];
    }
    m.vertices = verts;
    // Drop triangles a weld collapsed.
    let idx = std::mem::take(&mut m.indices);
    m.indices = idx.as_chunks::<3>().0.iter().filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).flatten().copied().collect();
}

/// A made block: a box `size` (base centre at `at`) with edges rounded by
/// `bevel` (m), `seg` grid steps a face (bunched towards the edges), welded
/// into one surface, its edges chipped by `wear` (0..1: none to well-used).
/// The shared base of cast, cut and carved blocks (cubes, plinths, steps,
/// crates, slabs).
///
/// Round 9: the faces keep the rounded box's exact normals (smooth normals
/// over the coarse, edge-bunched grid tilted the flat faces near every edge
/// and spread the tilt across the face: a cushion), and only the surface a
/// chip moved gets new ones. The wear is [`edge_wear`]: real broken chips
/// on a few arrises, notching the silhouette, at the mesh density `density`
/// (1 High, under 1 for the lower tiers: the chips' triangles grow, their
/// number stays).
pub fn made_block(size: [f32; 3], bevel: f32, seg: u32, wear: f32, seed: u32, at: [f32; 3]) -> Mesh {
    made_block_at(size, bevel, seg, wear, seed, at, 1.0)
}

/// [`made_block`] at mesh density `density` (see [`Density`]).
pub fn made_block_at(size: [f32; 3], bevel: f32, seg: u32, wear: f32, seed: u32, at: [f32; 3], density: f32) -> Mesh {
    let mut m = crate::build::rounded_box(size, bevel.max(0.0005), seg, at);
    weld(&mut m);
    box_normals(&mut m, size, at, bevel.max(0.0005));
    // Round 9 (Chris's concrete spec): sharp arrises, intact for the most
    // part; the wear is broken chips concentrated on a few of them and at
    // their corners ([`edge_wear`]), no rubbing on every edge.
    let small = size[0].min(size[1]).min(size[2]);
    let mut ew = EdgeWear::new(wear, seed);
    ew.size = ew.size.min(small * 0.06);
    let moved = edge_wear(&mut m, &box_arrises(size, at, bevel.max(0.0005)), &ew, density);
    smooth_normals_where(&mut m, &moved);
    m
}

/// Procedural edge wear (round 9, Chris's concrete spec; for concrete,
/// stone, plaster and brick): irregular broken chips and the odd missing
/// fragment, concentrated on a few of a shape's arrises and at their corners,
/// deterministic and asymmetric, most edges left intact. Each chip is real
/// geometry ([`spall`]): it changes the silhouette. A shape gives its
/// arrises as segments on the middle of their rounding ([`box_arrises`] for
/// a block; a lathe's rims as rings of segments).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeWear {
    /// How worn (0: new and intact .. 1: well used).
    pub amount: f32,
    /// The share of arrises that take damage (the rest stay intact).
    pub focus: f32,
    /// A typical chip's radius (m).
    pub size: f32,
    pub seed: u32,
}

impl EdgeWear {
    /// Cast concrete or cut stone at wear `amount`: about a third of the
    /// arrises damaged at full wear, chips about 1.5 cm across.
    pub const fn new(amount: f32, seed: u32) -> Self {
        Self { amount, focus: 0.34, size: 0.012, seed }
    }
}

/// The twelve arrises of the box `size` (base centre `base`), each on the
/// middle of its rounding `bevel`.
pub fn box_arrises(size: [f32; 3], base: [f32; 3], bevel: f32) -> Vec<([f32; 3], [f32; 3])> {
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let c = [base[0], base[1] + h[1], base[2]];
    let mut out = Vec::with_capacity(12);
    for axis in 0..3 {
        let (a1, a2) = ((axis + 1) % 3, (axis + 2) % 3);
        let inset = 0.293 * bevel.min(h[a1]).min(h[a2]);
        for e in 0..4 {
            let (s1, s2) = (if e & 1 == 0 { 1.0 } else { -1.0 }, if e & 2 == 0 { 1.0 } else { -1.0 });
            let mut p = c;
            p[a1] += s1 * (h[a1] - inset);
            p[a2] += s2 * (h[a2] - inset);
            let (mut a, mut b) = (p, p);
            a[axis] -= h[axis] - inset;
            b[axis] += h[axis] - inset;
            out.push((a, b));
        }
    }
    out
}

/// Wear the arrises `edges` of the welded mesh `m` by `w`, refined at mesh
/// density `density`. Returns which vertices moved (their normals still
/// to remake: [`smooth_normals_where`]).
pub fn edge_wear(m: &mut Mesh, edges: &[([f32; 3], [f32; 3])], w: &EdgeWear, density: f32) -> Vec<bool> {
    let mut moved = vec![false; m.vertices.len()];
    let chips = edge_wear_chips(edges, w, density);
    if chips.is_empty() {
        return moved;
    }
    let mv = spalls(m, &chips);
    merge_flags(&mut moved, &mv);
    moved
}

/// The chips [`edge_wear`] breaks, in its order, without breaking them (a
/// shape that must stay inside a triangle budget breaks them itself, a few
/// at a time: the concrete cube).
pub fn edge_wear_chips(edges: &[([f32; 3], [f32; 3])], w: &EdgeWear, density: f32) -> Vec<Chip> {
    let amount = w.amount.clamp(0.0, 1.0);
    if amount <= 0.0 || w.size <= 0.0 {
        return Vec::new();
    }
    let h = |k: u32| crate::noise::hash(w.seed.wrapping_mul(0x6c8e_9cf5) ^ k.wrapping_mul(0x9e37_79b9)) as f32 / u32::MAX as f32;
    let dn = density.clamp(0.25, 2.0);
    let mut chips: Vec<Chip> = Vec::new();
    let mut chip = |at: [f32; 3], r: f32, k: u32| {
        chips.push(Chip { at, radius: r, depth: r * (0.4 + 0.35 * h(k ^ 0x5a5a)), fine: (r * 0.28 / dn).max(0.0025), seed: w.seed ^ k.wrapping_mul(0x2545_f491) });
    };
    for (i, (a, b)) in edges.iter().enumerate() {
        let i = i as u32;
        let len = len3(sub3(*b, *a));
        if len <= 1e-4 {
            continue;
        }
        let at = |t: f32| -> [f32; 3] { std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t) };
        if h(i * 16) < w.focus * (0.5 + amount) {
            // A damaged arris: chips in a run or two along it, more towards
            // one end, the odd missing fragment at a corner.
            let n = ((len * 7.0 * amount * (0.4 + h(i * 16 + 1))).round() as u32).max(1);
            let centre = if h(i * 16 + 2) < 0.5 { 0.12 } else { 0.88 };
            for j in 0..n {
                let k = i * 64 + j;
                let t = (centre + (h(k * 3 + 100) - 0.5) * (0.35 + 0.6 * h(i * 16 + 3))).clamp(0.02, 0.98);
                let r = w.size * (0.45 + 1.4 * h(k * 3 + 101) * h(k * 3 + 102));
                chip(at(t), r, k);
            }
            if h(i * 16 + 4) < 0.6 * amount {
                let t = if centre < 0.5 { 0.02 } else { 0.98 };
                chip(at(t), w.size * 2.4, i * 64 + 63);
            }
        } else if h(i * 16 + 5) < 0.25 * amount {
            // An intact arris with one small knock.
            chip(at(0.1 + 0.8 * h(i * 16 + 6)), w.size * 0.5, i * 64 + 62);
        }
    }
    chips
}

/// The rounded box's exact normal at every vertex (box `size`, base centre
/// `base`, edges rounded by `bevel`): the face's own on the flats, the
/// rounding's on the edges.
pub fn box_normals(m: &mut Mesh, size: [f32; 3], base: [f32; 3], bevel: f32) {
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let c = [base[0], base[1] + h[1], base[2]];
    let r = bevel.clamp(0.0, h[0].min(h[1]).min(h[2]));
    for v in &mut m.vertices {
        let q = [v[0] - c[0], v[1] - c[1], v[2] - c[2]];
        let inner: [f32; 3] = std::array::from_fn(|k| q[k].clamp(-(h[k] - r), h[k] - r));
        let d = sub3(q, inner);
        let n = if len3(d) > 1e-7 {
            crate::geo::normalize(d)
        } else {
            // Inside the inner box (a vertex on a flat face of a box with no
            // rounding): the nearest face's normal.
            let a = [h[0] - q[0].abs(), h[1] - q[1].abs(), h[2] - q[2].abs()];
            let k = if a[0] <= a[1] && a[0] <= a[2] { 0 } else if a[1] <= a[2] { 1 } else { 2 };
            let mut n = [0.0; 3];
            n[k] = q[k].signum();
            n
        };
        v[3] = n[0];
        v[4] = n[1];
        v[5] = n[2];
    }
}

/// A seeded point on an arris of the box `size` (base centre `base`, edges
/// rounded by `bevel`), on the rounding's middle: `pick` (0..1) chooses one
/// of the twelve edges, `t` (0..1) how far along. Points near a corner
/// (within 8% of the edge's ends) report `true`.
pub fn box_edge_point(size: [f32; 3], base: [f32; 3], bevel: f32, pick: f32, t: f32) -> ([f32; 3], bool) {
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let c = [base[0], base[1] + h[1], base[2]];
    let e = ((pick * 12.0) as usize).min(11);
    let axis = e / 4;
    let (s1, s2) = (if e & 1 == 0 { 1.0 } else { -1.0 }, if e & 2 == 0 { 1.0 } else { -1.0 });
    let (a1, a2) = ((axis + 1) % 3, (axis + 2) % 3);
    // Bias towards the ends: corners chip most.
    let u = t * 2.0 - 1.0;
    let u = u.signum() * u.abs().powf(0.6);
    let inset = 0.293 * bevel.min(h[a1]).min(h[a2]);
    let mut p = c;
    p[axis] += u * h[axis] * 0.97;
    p[a1] += s1 * (h[a1] - inset);
    p[a2] += s2 * (h[a2] - inset);
    (p, u.abs() > 0.92)
}

/// Break a chip out of a solid, welded mesh at `at` (a point on or near its
/// surface; round 9): a pit up to `depth` (m) deep and about `radius`
/// across whose floor is a few flat fracture facets (the depth falls off
/// linearly over a seeded polytope, so each facet is a plane), with a little
/// roughness on the break, as broken concrete or stone shows. Vertices move
/// in along their own normals, so a chip on an arris notches both faces and
/// the silhouette. The mesh is refined near it first (triangles about
/// `fine` m). Returns which vertices moved.
pub fn spall(m: &mut Mesh, at: [f32; 3], radius: f32, depth: f32, fine: f32, seed: u32) -> Vec<bool> {
    spalls(m, &[Chip { at, radius, depth, fine, seed }])
}

/// One chip for [`spalls`] (see [`spall`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chip {
    pub at: [f32; 3],
    pub radius: f32,
    pub depth: f32,
    pub fine: f32,
    pub seed: u32,
}

/// Several [`spall`]s at once: the mesh is refined round all of them in one
/// pass (one pass each made a shape with a dozen chips slow to remake).
pub fn spalls(m: &mut Mesh, chips: &[Chip]) -> Vec<bool> {
    let chips: Vec<Chip> = chips.iter().copied().filter(|c| c.radius > 0.0 && c.depth > 0.0).collect();
    if chips.is_empty() {
        return vec![false; m.vertices.len()];
    }
    let finest = chips.iter().map(|c| c.fine).fold(f32::MAX, f32::min);
    refine_near(m, finest, 14, |c, e| chips.iter().any(|k| e > k.fine && len3(sub3(c, k.at)) < k.radius * 2.0 + 0.5 * e));
    let mut moved = vec![false; m.vertices.len()];
    for k in &chips {
        let h = |j: u32| crate::noise::hash(k.seed.wrapping_mul(0x85eb_ca6b) ^ j.wrapping_mul(0xc2b2_ae35)) as f32 / u32::MAX as f32;
        // The fracture planes: the six axis directions tilted at random, plus
        // two more, each at its own distance (so the outline is uneven).
        let dirs: [([f32; 3], f32); 8] = std::array::from_fn(|j| {
            let j = j as u32;
            let base: [f32; 3] = match j {
                0 => [1.0, 0.0, 0.0],
                1 => [-1.0, 0.0, 0.0],
                2 => [0.0, 1.0, 0.0],
                3 => [0.0, -1.0, 0.0],
                4 => [0.0, 0.0, 1.0],
                5 => [0.0, 0.0, -1.0],
                _ => [h(40 + j) - 0.5, h(50 + j) - 0.5, h(60 + j) - 0.5],
            };
            let w = [h(10 + j) - 0.5, h(20 + j) - 0.5, h(30 + j) - 0.5];
            (crate::geo::normalize([base[0] + w[0] * 0.9, base[1] + w[1] * 0.9, base[2] + w[2] * 0.9]), k.radius * (0.55 + 0.6 * h(70 + j)))
        });
        let reach = k.radius * 2.0;
        for (i, v) in m.vertices.iter_mut().enumerate() {
            let q = sub3([v[0], v[1], v[2]], k.at);
            let dist = len3(q);
            if dist >= reach {
                continue;
            }
            let g = dirs.iter().map(|(d, r)| dot3(*d, q) / r).fold(f32::MIN, f32::max);
            let rough = 1.0 + 0.18 * fbm3p([v[0] / k.radius * 2.5, v[1] / k.radius * 2.5, v[2] / k.radius * 2.5], k.seed ^ 0x51, 2, 0.5);
            let push = k.depth * (1.0 - g).clamp(0.0, 1.0) * rough * (1.0 - smoothstep(reach * 0.8, reach, dist));
            if push > 0.0 {
                v[0] -= v[3] * push;
                v[1] -= v[4] * push;
                v[2] -= v[5] * push;
                moved[i] = true;
            }
        }
    }
    moved
}

/// `a |= b` per vertex (b may be longer: vertices added since `a` was made).
pub fn merge_flags(a: &mut Vec<bool>, b: &[bool]) {
    if a.len() < b.len() {
        a.resize(b.len(), false);
    }
    for (x, y) in a.iter_mut().zip(b) {
        *x |= *y;
    }
}

/// Smooth normals (area-weighted) for the vertices `moved` flags and every
/// vertex sharing a triangle with one; the rest keep theirs. A carving
/// changes the light only where it is.
pub fn smooth_normals_where(m: &mut Mesh, moved: &[bool]) {
    let mut near = vec![false; m.vertices.len()];
    for t in m.indices.as_chunks::<3>().0 {
        if t.iter().any(|&i| moved.get(i as usize).copied().unwrap_or(false)) {
            for &i in t {
                near[i as usize] = true;
            }
        }
    }
    let keep: Vec<[f32; 3]> = m.vertices.iter().map(|v| [v[3], v[4], v[5]]).collect();
    smooth_normals(m);
    for ((v, k), n) in m.vertices.iter_mut().zip(keep).zip(near) {
        if !n {
            v[3] = k[0];
            v[4] = k[1];
            v[5] = k[2];
        }
    }
}

/// Each triangle of `m` split into three round a new point over its
/// centre, raised along the face's normal by `rise(centre, triangle)` (m;
/// negative presses it in), then flat-shaded: the shallow pyramid facets a
/// cast or cut geodesic form shows, every facet with its own inner ridges
/// catching the light (round 9, the faceted ball's reference).
pub fn kis(m: &Mesh, rise: impl Fn([f32; 3], usize) -> f32) -> Mesh {
    let mut out = Mesh { vertices: m.vertices.clone(), indices: Vec::with_capacity(m.indices.len() * 3) };
    for (k, t) in m.indices.as_chunks::<3>().0.iter().enumerate() {
        let p: [[f32; 3]; 3] = std::array::from_fn(|i| {
            let v = m.vertices[t[i] as usize];
            [v[0], v[1], v[2]]
        });
        let c: [f32; 3] = std::array::from_fn(|i| (p[0][i] + p[1][i] + p[2][i]) / 3.0);
        let n = crate::geo::normalize(cross3(sub3(p[1], p[0]), sub3(p[2], p[0])));
        let r = rise(c, k);
        out.vertices.push([c[0] + n[0] * r, c[1] + n[1] * r, c[2] + n[2] * r, n[0], n[1], n[2]]);
        let ci = (out.vertices.len() - 1) as u32;
        out.indices.extend_from_slice(&[t[0], t[1], ci, t[1], t[2], ci, t[2], t[0], ci]);
    }
    crate::shapes::flat_shade(&mut out);
    out
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// How finely a generator refines its carved relief: 1 at High and Ultra,
/// less on the lower tiers (the module passes it from the active tier as
/// [`crate::Values::density`]); the relief keeps its size and place, only
/// its triangles grow.
pub struct Density;

impl Density {
    /// The density for a tier id (`lite`, `low`, `medium`, `high`, `ultra`;
    /// anything else High's).
    pub fn for_tier(id: &str) -> f32 {
        match id {
            "lite" => 0.3,
            "low" => 0.45,
            "medium" => 0.7,
            _ => 1.0,
        }
    }
}

/// [`subdivide`] only where `near(centroid, longest edge)` says so: the
/// fine triangles a carved detail needs, nowhere else. Midpoints on an edge
/// shared with an unsplit triangle stay on that edge, so the surface stays
/// closed as long as the carving fades to nothing before the refined
/// region's border.
pub fn subdivide_where(m: &mut Mesh, max_edge: f32, rounds: u32, near: impl Fn([f32; 3], f32) -> bool) {
    let max2 = max_edge.max(1e-4).powi(2);
    for _ in 0..rounds.min(10) {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut out = Vec::with_capacity(m.indices.len() * 2);
        let mut any = false;
        let tris: Vec<[u32; 3]> = m.indices.as_chunks::<3>().0.to_vec();
        for t in tris {
            let p = |i: u32| {
                let v = m.vertices[i as usize];
                [v[0], v[1], v[2]]
            };
            let d2 = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let long = d2(a, b).max(d2(b, c)).max(d2(c, a));
            let centre = std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0);
            if long <= max2 || !near(centre, long.sqrt()) {
                out.extend_from_slice(&t);
                continue;
            }
            any = true;
            let mut midpoint = |a: u32, b: u32| -> u32 {
                let key = (a.min(b), a.max(b));
                *mid.entry(key).or_insert_with(|| {
                    let (va, vb) = (m.vertices[a as usize], m.vertices[b as usize]);
                    let mut v: [f32; 6] = std::array::from_fn(|k| (va[k] + vb[k]) * 0.5);
                    let n = crate::geo::normalize([v[3], v[4], v[5]]);
                    v[3] = n[0];
                    v[4] = n[1];
                    v[5] = n[2];
                    m.vertices.push(v);
                    (m.vertices.len() - 1) as u32
                })
            };
            let (ab, bc, ca) = (midpoint(t[0], t[1]), midpoint(t[1], t[2]), midpoint(t[2], t[0]));
            out.extend_from_slice(&[t[0], ab, ca, ab, t[1], bc, ca, bc, t[2], ab, bc, ca]);
        }
        m.indices = out;
        if !any {
            break;
        }
    }
}

/// Refine `m` only where `near(centroid, longest edge)` says so, by
/// splitting long edges in two (round 9): each round marks the longest edge
/// of every near triangle longer than `max_edge` and splits every triangle
/// on a marked edge into two, three or four, so the surface stays closed (no
/// T-junctions) and a long, thin triangle along a rounded arris is cut only
/// along its length ([`subdivide_where`] quarters it both ways each round,
/// which multiplied an arris's slivers into tens of thousands).
pub fn refine_near(m: &mut Mesh, max_edge: f32, rounds: u32, near: impl Fn([f32; 3], f32) -> bool) {
    let max2 = max_edge.max(1e-4).powi(2);
    for _ in 0..rounds.min(16) {
        let pos = |m: &Mesh, i: u32| {
            let v = m.vertices[i as usize];
            [v[0], v[1], v[2]]
        };
        let d2 = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
        let key = |a: u32, b: u32| (a.min(b), a.max(b));
        let tris: Vec<[u32; 3]> = m.indices.as_chunks::<3>().0.to_vec();
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &tris {
            let p = [pos(m, t[0]), pos(m, t[1]), pos(m, t[2])];
            let l = [d2(p[0], p[1]), d2(p[1], p[2]), d2(p[2], p[0])];
            let k = if l[0] >= l[1] && l[0] >= l[2] { 0 } else if l[1] >= l[2] { 1 } else { 2 };
            if l[k] <= max2 {
                continue;
            }
            let c = std::array::from_fn(|j| (p[0][j] + p[1][j] + p[2][j]) / 3.0);
            if near(c, l[k].sqrt()) {
                mid.insert(key(t[k], t[(k + 1) % 3]), u32::MAX);
            }
        }
        if mid.is_empty() {
            break;
        }
        // New vertices in edge order (a hash map's own order changes from
        // run to run: the same seed must give the same vertex list).
        let mut keys: Vec<(u32, u32)> = mid.keys().copied().collect();
        keys.sort_unstable();
        for (a, b) in keys {
            let (va, vb) = (m.vertices[a as usize], m.vertices[b as usize]);
            let mut v: [f32; 6] = std::array::from_fn(|k| (va[k] + vb[k]) * 0.5);
            let n = crate::geo::normalize([v[3], v[4], v[5]]);
            v[3] = n[0];
            v[4] = n[1];
            v[5] = n[2];
            m.vertices.push(v);
            mid.insert((a, b), (m.vertices.len() - 1) as u32);
        }
        let mut out = Vec::with_capacity(m.indices.len() * 2);
        for t in tris {
            let e: [Option<u32>; 3] = std::array::from_fn(|k| mid.get(&key(t[k], t[(k + 1) % 3])).copied());
            let n = e.iter().filter(|x| x.is_some()).count();
            // Rotate so the split edges come first: (a, b) and then (b, c).
            let r = match n {
                1 => (0..3).find(|&k| e[k].is_some()).unwrap_or(0),
                2 => (0..3).find(|&k| e[k].is_some() && e[(k + 1) % 3].is_some()).unwrap_or(0),
                _ => 0,
            };
            let (a, b, c) = (t[r], t[(r + 1) % 3], t[(r + 2) % 3]);
            let (e0, e1, e2) = (e[r], e[(r + 1) % 3], e[(r + 2) % 3]);
            match (n, e0, e1, e2) {
                (1, Some(p), _, _) => out.extend_from_slice(&[a, p, c, p, b, c]),
                (2, Some(p), Some(q), _) => out.extend_from_slice(&[p, b, q, a, p, q, a, q, c]),
                (3, Some(p), Some(q), Some(s)) => out.extend_from_slice(&[a, p, s, p, b, q, s, q, c, p, q, s]),
                _ => out.extend_from_slice(&[a, b, c]),
            }
        }
        m.indices = out;
    }
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn len3(a: [f32; 3]) -> f32 {
    dot3(a, a).sqrt()
}

/// Distance from `p` to the polyline `path` and how far along it (0 at the
/// start, 1 at the end) the nearest point lies.
fn to_path(p: [f32; 3], path: &[[f32; 3]]) -> (f32, f32) {
    let (mut best, mut at, mut run) = (f32::MAX, 0.0, 0.0);
    for w in path.windows(2) {
        let ab = sub3(w[1], w[0]);
        let l2 = dot3(ab, ab);
        let t = (dot3(sub3(p, w[0]), ab) / l2.max(1e-12)).clamp(0.0, 1.0);
        let q = [w[0][0] + ab[0] * t, w[0][1] + ab[1] * t, w[0][2] + ab[2] * t];
        let d = sub3(p, q);
        let d2 = dot3(d, d);
        let l = l2.sqrt();
        if d2 < best {
            best = d2;
            at = run + l * t;
        }
        run += l;
    }
    (best.sqrt(), at / run.max(1e-6))
}

/// A seeded, jagged crack path on the `+z` face of a box `size` (base
/// centre `at`): it starts on the top face `lip` in from the edge, crosses
/// the top edge and runs down and across the face for `length` (m), in
/// short straight runs that change direction a little at each kink, as a
/// shrinkage crack in cast concrete does.
pub fn box_crack_path(size: [f32; 3], at: [f32; 3], length: f32, lip: f32, seed: u32) -> Vec<[f32; 3]> {
    let h = |k: u32| crate::noise::hash(seed.wrapping_mul(0x9e37_79b9) ^ k) as f32 / u32::MAX as f32;
    let (hx, top, front) = (size[0] * 0.5, at[1] + size[1], at[2] + size[2] * 0.5);
    let x0 = at[0] + (h(1) - 0.5) * hx;
    let mut path = vec![[x0, top, front - lip], [x0 + 0.002, top, front], [x0 + 0.003, top - 0.01, front]];
    let (mut x, mut y) = (x0 + 0.003, top - 0.01);
    let lean = (h(2) - 0.5) * 1.2;
    let n = 14;
    for k in 0..n {
        let step = length / n as f32;
        let a = lean + (h(10 + k) - 0.5) * 1.1;
        x = (x + a.sin() * step).clamp(at[0] - hx + 0.05, at[0] + hx - 0.05);
        y = (y - a.cos() * step).max(at[1] + 0.05);
        path.push([x, y, front]);
    }
    path
}

/// Carve a crack along `path` into a smooth, welded mesh: the mesh is made
/// fine near it (triangles down to about half the width), then pressed in along its
/// normals in a V `width` wide at the mouth and `depth` deep, both tapering
/// to nothing along the path, with a shallow spalled shoulder either side.
/// Real geometry: a crack that crosses an edge notches the silhouette.
pub fn carve_crack(m: &mut Mesh, path: &[[f32; 3]], width: f32, depth: f32) {
    if path.len() < 2 || width <= 0.0 || depth <= 0.0 {
        return;
    }
    carve_crack_moved(m, path, width, depth);
}

/// [`carve_crack`], reporting which vertices moved; only the surface near
/// the crack gets new normals (round 9: the whole mesh's were remade, which
/// put the cushion back on a block's flat faces).
pub fn carve_crack_moved(m: &mut Mesh, path: &[[f32; 3]], width: f32, depth: f32) -> Vec<bool> {
    if path.len() < 2 || width <= 0.0 || depth <= 0.0 {
        return vec![false; m.vertices.len()];
    }
    let reach = width * 2.0;
    let fine = (width * 0.75).max(0.0025);
    // The path's box, grown by the reach: most of the mesh is skipped
    // without measuring its distance to every segment.
    let mut bl = [f32::MAX; 3];
    let mut bh = [f32::MIN; 3];
    for q in path {
        for k in 0..3 {
            bl[k] = bl[k].min(q[k] - reach);
            bh[k] = bh[k].max(q[k] + reach);
        }
    }
    let inside = |c: [f32; 3], e: f32| (0..3).all(|k| c[k] >= bl[k] - e && c[k] <= bh[k] + e);
    refine_near(m, fine, 14, |c, e| inside(c, e) && to_path(c, path).0 < reach + 0.5 * e);
    let mut moved = vec![false; m.vertices.len()];
    for (i, v) in m.vertices.iter_mut().enumerate() {
        if !inside([v[0], v[1], v[2]], 0.0) {
            continue;
        }
        let (d, t) = to_path([v[0], v[1], v[2]], path);
        if d >= reach {
            continue;
        }
        // Widest and deepest a little after the start, closing to a hairline.
        let taper = (1.0 - t).powf(0.7) * smoothstep(0.0, 0.08, t + 0.04);
        let w = width * 0.5 * taper;
        let groove = if d < w { 1.0 - d / w.max(1e-6) } else { 0.0 };
        let shoulder = 1.0 - smoothstep(0.0, reach, d);
        let push = depth * taper * (groove * groove.sqrt() + 0.08 * shoulder);
        v[0] -= v[3] * push;
        v[1] -= v[4] * push;
        v[2] -= v[5] * push;
        moved[i] = push > 0.0;
    }
    smooth_normals_where(m, &moved);
    moved
}

/// The point of the rounded box `size` (base centre `base`, edges rounded
/// by `bevel`) nearest `p`: a path drawn near the box laid on its surface
/// (over a rounding, not through it).
pub fn onto_rounded_box(p: [f32; 3], size: [f32; 3], base: [f32; 3], bevel: f32) -> [f32; 3] {
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let c = [base[0], base[1] + h[1], base[2]];
    let r = bevel.clamp(0.0, h[0].min(h[1]).min(h[2]));
    let q = sub3(p, c);
    let inner: [f32; 3] = std::array::from_fn(|k| q[k].clamp(-(h[k] - r), h[k] - r));
    let d = sub3(q, inner);
    let l = len3(d);
    let s = if l > 1e-7 {
        std::array::from_fn(|k| inner[k] + d[k] / l * r)
    } else {
        // Inside the inner box: out to the nearest face.
        let a = [h[0] - q[0].abs(), h[1] - q[1].abs(), h[2] - q[2].abs()];
        let k = if a[0] <= a[1] && a[0] <= a[2] { 0 } else if a[1] <= a[2] { 1 } else { 2 };
        let mut s = q;
        s[k] = q[k].signum() * h[k];
        s
    };
    [s[0] + c[0], s[1] + c[1], s[2] + c[2]]
}

/// A profile with points added so no segment is longer than `step` (m);
/// `skip_base` leaves the segments along y = 0 (a base nobody sees) as
/// they are.
pub fn resample_profile(points: &[[f32; 2]], step: f32, skip_base: bool) -> Vec<[f32; 2]> {
    let step = step.max(1e-3);
    let mut out = Vec::with_capacity(points.len() * 4);
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        out.push(a);
        if skip_base && a[1].abs() < 1e-5 && b[1].abs() < 1e-5 {
            continue;
        }
        let l = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let n = (l / step).ceil() as usize;
        for i in 1..n {
            let t = i as f32 / n as f32;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if let Some(l) = points.last() {
        out.push(*l);
    }
    out
}

/// A lathe or extrusion profile with every inner corner rounded: each
/// corner is replaced by `steps` points on a curve that leaves along both
/// edges `r` from the corner (at most a third of either edge), so a turned
/// or cut shape has the softened rims a made thing has. The ends stay.
pub fn round_profile(points: &[[f32; 2]], r: f32, steps: u32) -> Vec<[f32; 2]> {
    if points.len() < 3 || r <= 0.0 || steps == 0 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    for w in points.windows(3) {
        let (a, p, b) = (w[0], w[1], w[2]);
        let la = ((a[0] - p[0]).powi(2) + (a[1] - p[1]).powi(2)).sqrt();
        let lb = ((b[0] - p[0]).powi(2) + (b[1] - p[1]).powi(2)).sqrt();
        let k = r.min(la / 3.0).min(lb / 3.0);
        if k <= 1e-6 {
            out.push(p);
            continue;
        }
        let s = [p[0] + (a[0] - p[0]) / la * k, p[1] + (a[1] - p[1]) / la * k];
        let e = [p[0] + (b[0] - p[0]) / lb * k, p[1] + (b[1] - p[1]) / lb * k];
        // A quadratic curve from s to e with the corner as its control.
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            out.push([u * u * s[0] + 2.0 * u * t * p[0] + t * t * e[0], u * u * s[1] + 2.0 * u * t * p[1] + t * t * e[1]]);
        }
    }
    if let Some(last) = points.last() {
        out.push(*last);
    }
    out
}

#[cfg(test)]
mod tests {
    /// Round 8: a carved crack is real geometry: it goes into the surface by
    /// its depth near the start, the mesh is fine only near it, and the same
    /// seed carves the same crack.
    #[test]
    fn a_carved_crack_goes_into_the_surface() {
        let size = [1.0, 1.0, 1.0];
        let base = made_block(size, 0.006, 12, 0.0, 7, [0.0; 3]);
        let mut m = base.clone();
        let path = box_crack_path(size, [0.0; 3], 0.4, 0.03, 7);
        carve_crack(&mut m, &path, 0.008, 0.012);
        println!("crack: {} to {} triangles", base.indices.len() / 3, m.indices.len() / 3);
        assert!(m.indices.len() > base.indices.len() && m.indices.len() < base.indices.len() + 36_000, "refined near the crack only: {} to {}", base.indices.len(), m.indices.len());
        let deepest = m.vertices.iter().filter(|v| v[1] < 0.95).map(|v| 0.5 - v[2]).fold(0.0f32, f32::max);
        assert!(deepest > 0.006, "the groove is millimetres deep: {deepest}");
        let mut again = base.clone();
        carve_crack(&mut again, &box_crack_path(size, [0.0; 3], 0.4, 0.03, 7), 0.008, 0.012);
        assert_eq!(again.vertices, m.vertices, "seeded");
    }

    use super::*;

    fn tri() -> Mesh {
        Mesh { vertices: vec![[0.0, 0.0, 0.0, 0.0, 1.0, 0.0], [4.0, 0.0, 0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 4.0, 0.0, 1.0, 0.0]], indices: vec![0, 2, 1] }
    }

    #[test]
    fn subdivide_splits_long_edges_and_shares_midpoints() {
        let mut m = tri();
        subdivide(&mut m, 1.5, 4);
        // Legs 4 -> 2 -> 1 (the long side then 1.41, under 1.5): two rounds,
        // 16 triangles.
        assert_eq!(m.indices.len() / 3, 16);
        // Shared midpoints: a triangular grid of 5 rows has 15 vertices.
        assert_eq!(m.vertices.len(), 15);
    }

    #[test]
    fn split_by_keeps_every_triangle_once() {
        let mut m = tri();
        subdivide(&mut m, 1.5, 4);
        let parts = split_by(&m, 2, |c, _| usize::from(c[0] > 1.0));
        assert_eq!(parts.iter().map(|p| p.indices.len()).sum::<usize>(), m.indices.len());
        assert!(parts.iter().all(|p| !p.indices.is_empty()));
    }

    #[test]
    fn rounded_profiles_keep_their_ends_and_soften_corners() {
        let p = [[0.0, 0.0], [1.0, 0.0], [1.0, 2.0], [0.0, 2.0]];
        let r = round_profile(&p, 0.1, 4);
        assert_eq!((r[0], r[r.len() - 1]), (p[0], p[3]));
        assert_eq!(r.len(), 2 + 2 * 5);
        // No point reaches the sharp corner any more.
        assert!(r.iter().all(|q| (q[0] - 1.0).abs() + q[1].abs() > 0.02));
    }

    #[test]
    fn displacement_is_deterministic() {
        let mut a = tri();
        subdivide(&mut a, 1.0, 4);
        let mut b = a.clone();
        displace(&mut a, |p, _| 0.1 * fbm3p(p, 3, 3, 0.5));
        displace(&mut b, |p, _| 0.1 * fbm3p(p, 3, 3, 0.5));
        assert_eq!(a, b);
    }
}
