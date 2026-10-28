//! Feature-preserving extraction of a triangle mesh from a field
//! (docs/model_research/13-meshing.md), M1 version: a sparse uniform grid.
//!
//! 1. Blocks of 8^3 cells are culled by the field's Lipschitz bound; corners
//!    are evaluated only in blocks the surface can reach.
//! 2. Every grid edge whose corners differ in sign gets exact Hermite data:
//!    the crossing by a fixed count of bisection steps and one secant step,
//!    the normal from the field's exact gradient.
//! 3. Inside each cell, crossed edges are joined into closed loops face by
//!    face (an ambiguous face is decided by the field at its centre, the same
//!    value for both cells that share it). Each loop is one surface patch and
//!    gets its own vertex, so separate sheets in one cell never fuse.
//! 4. A patch's vertex minimises the distance to the tangent planes of its
//!    crossings (our own 3x3 symmetric eigen solver, small eigenvalues
//!    truncated, measured from the mean crossing point), is clamped to its
//!    cell and then moved onto the surface by bounded Newton steps.
//! 5. Every crossed edge becomes a quad joining the patches of its four cells,
//!    split along the diagonal whose midpoint lies closer to the surface.
//!
//! The octree version with budgeted adaptive refinement (note 13) replaces
//! step 1's uniform grid after M1; the outputs and checks stay the same.

use crate::rng::map_tiles;
use crate::sdf::Node;
use crate::vec::*;

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub pos: Vec<V3>,
    pub nrm: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    /// Owning part of each triangle (from the field's labels).
    pub tri_part: Vec<u32>,
}

impl Mesh {
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in &self.pos {
            b.add_point(*p);
        }
        b
    }
    /// Append `o` (indices shifted).
    pub fn append(&mut self, o: &Mesh) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(&o.pos);
        self.nrm.extend_from_slice(&o.nrm);
        self.tris.extend(o.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        self.tri_part.extend_from_slice(&o.tri_part);
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Cell size in metres.
    pub h: f64,
    pub threads: usize,
    /// Eigenvalues of the tangent-plane system below this share of the
    /// largest are ignored (flat and edge cells stay on their mean point).
    pub truncate: f64,
    /// Merge cells (up to 3 levels: blocks of 8^3) whose patch fits one
    /// vertex within this RMS distance (metres); 0 keeps every cell.
    pub collapse_tol: f64,
}

impl Options {
    pub fn new(h: f64, threads: usize) -> Self {
        Options { h, threads, truncate: 0.02, collapse_tol: 0.0 }
    }
}

const BLOCK: usize = 8;
const BISECT: usize = 12;

struct Grid {
    o: V3,
    h: f64,
    n: [usize; 3],
}

impl Grid {
    fn corners(&self) -> [usize; 3] {
        [self.n[0] + 1, self.n[1] + 1, self.n[2] + 1]
    }
    fn cidx(&self, i: usize, j: usize, k: usize) -> usize {
        let c = self.corners();
        (k * c[1] + j) * c[0] + i
    }
    fn cpos(&self, i: usize, j: usize, k: usize) -> V3 {
        [self.o[0] + i as f64 * self.h, self.o[1] + j as f64 * self.h, self.o[2] + k as f64 * self.h]
    }
    fn cell_idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.n[1] + j) * self.n[0] + i
    }
}

/// Local corner `c` (bits x, y, z) of a cell.
const fn corner_off(c: usize) -> [usize; 3] {
    [c & 1, (c >> 1) & 1, (c >> 2) & 1]
}

/// Local edges: (axis, corner a, corner b); index = axis * 4 + k where k
/// numbers the two other bits (low axis first).
const EDGES: [(usize, usize, usize); 12] = [
    (0, 0, 1),
    (0, 2, 3),
    (0, 4, 5),
    (0, 6, 7),
    (1, 0, 2),
    (1, 1, 3),
    (1, 4, 6),
    (1, 5, 7),
    (2, 0, 4),
    (2, 1, 5),
    (2, 2, 6),
    (2, 3, 7),
];

/// Faces: (axis, side, corners in cyclic order).
const FACES: [(usize, usize, [usize; 4]); 6] = [
    (0, 0, [0, 2, 6, 4]),
    (0, 1, [1, 3, 7, 5]),
    (1, 0, [0, 1, 5, 4]),
    (1, 1, [2, 3, 7, 6]),
    (2, 0, [0, 1, 3, 2]),
    (2, 1, [4, 5, 7, 6]),
];

fn local_edge(a: usize, b: usize) -> usize {
    let (a, b) = (a.min(b), a.max(b));
    EDGES.iter().position(|e| e.1 == a && e.2 == b).unwrap_or(0)
}

#[inline]
fn inside(v: f64) -> bool {
    v < 0.0
}

/// Extract the surface of `field` inside `region`.
pub fn extract(field: &Node, region: Aabb, opt: &Options) -> Mesh {
    let h = opt.h;
    let o = sub(region.min, [1.5 * h; 3]);
    let size = region.size();
    let n = [
        ((size[0] + 3.0 * h) / h).ceil() as usize,
        ((size[1] + 3.0 * h) / h).ceil() as usize,
        ((size[2] + 3.0 * h) / h).ceil() as usize,
    ];
    let g = Grid { o, h, n };
    let cc = g.corners();

    // 1. Corner values, block by block.
    let nb = [n[0].div_ceil(BLOCK), n[1].div_ceil(BLOCK), n[2].div_ceil(BLOCK)];
    let nblocks = nb[0] * nb[1] * nb[2];
    let half_diag = (3.0f64).sqrt() * BLOCK as f64 * h * 0.5;
    let block_lo = |b: usize| -> [usize; 3] {
        let (bx, by, bz) = (b % nb[0], (b / nb[0]) % nb[1], b / (nb[0] * nb[1]));
        [bx * BLOCK, by * BLOCK, bz * BLOCK]
    };
    let centre_d: Vec<f64> = map_tiles(nblocks, opt.threads, |b| {
        let lo = block_lo(b);
        let c = add(g.cpos(lo[0], lo[1], lo[2]), [BLOCK as f64 * h * 0.5; 3]);
        field.dist(c)
    });
    let mut corner = vec![0.0f64; cc[0] * cc[1] * cc[2]];
    let mut active = Vec::new();
    for (b, &d) in centre_d.iter().enumerate() {
        if d.abs() > half_diag * 1.001 + 1e-12 {
            let lo = block_lo(b);
            for k in lo[2]..=(lo[2] + BLOCK).min(n[2]) {
                for j in lo[1]..=(lo[1] + BLOCK).min(n[1]) {
                    for i in lo[0]..=(lo[0] + BLOCK).min(n[0]) {
                        corner[g.cidx(i, j, k)] = d;
                    }
                }
            }
        } else {
            active.push(b);
        }
    }
    let evals: Vec<Vec<f64>> = map_tiles(active.len(), opt.threads, |t| {
        let lo = block_lo(active[t]);
        let mut v = Vec::with_capacity((BLOCK + 1).pow(3));
        for k in lo[2]..=(lo[2] + BLOCK).min(n[2]) {
            for j in lo[1]..=(lo[1] + BLOCK).min(n[1]) {
                for i in lo[0]..=(lo[0] + BLOCK).min(n[0]) {
                    v.push(field.dist(g.cpos(i, j, k)));
                }
            }
        }
        v
    });
    for (t, vals) in evals.iter().enumerate() {
        let lo = block_lo(active[t]);
        let mut it = vals.iter();
        for k in lo[2]..=(lo[2] + BLOCK).min(n[2]) {
            for j in lo[1]..=(lo[1] + BLOCK).min(n[1]) {
                for i in lo[0]..=(lo[0] + BLOCK).min(n[0]) {
                    if let Some(v) = it.next() {
                        corner[g.cidx(i, j, k)] = *v;
                    }
                }
            }
        }
    }

    // 2. Crossed edges (sorted by id = corner * 3 + axis) and Hermite data.
    let mut cross_ids: Vec<u64> = Vec::new();
    for k in 0..cc[2] {
        for j in 0..cc[1] {
            for i in 0..cc[0] {
                let c0 = g.cidx(i, j, k);
                let s0 = inside(corner[c0]);
                let nbrs = [(i + 1 < cc[0], g.cidx(i + 1, j, k)), (j + 1 < cc[1], g.cidx(i, (j + 1).min(cc[1] - 1), k)), (k + 1 < cc[2], g.cidx(i, j, (k + 1).min(cc[2] - 1)))];
                for (a, (ok, c1)) in nbrs.iter().enumerate() {
                    if *ok && inside(corner[*c1]) != s0 {
                        cross_ids.push(c0 as u64 * 3 + a as u64);
                    }
                }
            }
        }
    }
    let unpack = |id: u64| -> ([usize; 3], usize) {
        let a = (id % 3) as usize;
        let c = (id / 3) as usize;
        ([c % cc[0], (c / cc[0]) % cc[1], c / (cc[0] * cc[1])], a)
    };
    const CHUNK: usize = 256;
    let herm_chunks: Vec<Vec<(V3, V3)>> = map_tiles(cross_ids.len().div_ceil(CHUNK), opt.threads, |t| {
        let lo = t * CHUNK;
        let hi = (lo + CHUNK).min(cross_ids.len());
        (lo..hi)
            .map(|e| {
                let (c, a) = unpack(cross_ids[e]);
                let p0 = g.cpos(c[0], c[1], c[2]);
                let mut c1 = c;
                c1[a] += 1;
                let p1 = g.cpos(c1[0], c1[1], c1[2]);
                let (mut v0, mut v1) = (corner[g.cidx(c[0], c[1], c[2])], corner[g.cidx(c1[0], c1[1], c1[2])]);
                let (mut t0, mut t1) = (0.0f64, 1.0f64);
                for _ in 0..BISECT {
                    let tm = 0.5 * (t0 + t1);
                    let vm = field.dist(lerp(p0, p1, tm));
                    if inside(vm) == inside(v0) {
                        t0 = tm;
                        v0 = vm;
                    } else {
                        t1 = tm;
                        v1 = vm;
                    }
                }
                let den = v0 - v1;
                let t = if den.abs() > 0.0 { (t0 + (t1 - t0) * (v0 / den)).clamp(t0, t1) } else { 0.5 * (t0 + t1) };
                let p = lerp(p0, p1, t);
                let s = field.eval(p);
                (p, norm_or(s.g, [0.0, 1.0, 0.0]))
            })
            .collect()
    });
    let herm: Vec<(V3, V3)> = herm_chunks.into_iter().flatten().collect();
    let edge_lookup = |c: [usize; 3], a: usize| -> Option<usize> {
        let id = g.cidx(c[0], c[1], c[2]) as u64 * 3 + a as u64;
        cross_ids.binary_search(&id).ok()
    };

    // 3. Active cells and their patches.
    let mut cells: Vec<usize> = Vec::with_capacity(cross_ids.len() * 2);
    for &id in &cross_ids {
        let (c, a) = unpack(id);
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        for du in 0..2 {
            for dv in 0..2 {
                let mut cell = c;
                if du == 1 {
                    if cell[u] == 0 {
                        continue;
                    }
                    cell[u] -= 1;
                }
                if dv == 1 {
                    if cell[v] == 0 {
                        continue;
                    }
                    cell[v] -= 1;
                }
                if cell[0] < n[0] && cell[1] < n[1] && cell[2] < n[2] {
                    cells.push(g.cell_idx(cell[0], cell[1], cell[2]));
                }
            }
        }
    }
    cells.sort_unstable();
    cells.dedup();
    let cell_coord = |ci: usize| -> [usize; 3] { [ci % n[0], (ci / n[0]) % n[1], ci / (n[0] * n[1])] };

    // Per cell: loop id per local edge, and per loop the vertex position.
    struct CellOut {
        edge_loop: [u8; 12],
        verts: Vec<V3>,
        qefs: Vec<Qef>,
    }
    let cell_chunks: Vec<Vec<CellOut>> = map_tiles(cells.len().div_ceil(CHUNK), opt.threads, |t| {
        let lo = t * CHUNK;
        let hi = (lo + CHUNK).min(cells.len());
        (lo..hi)
            .map(|ci| {
                let c = cell_coord(cells[ci]);
                let val = |lc: usize| -> f64 {
                    let off = corner_off(lc);
                    corner[g.cidx(c[0] + off[0], c[1] + off[1], c[2] + off[2])]
                };
                let crossed: [bool; 12] = core::array::from_fn(|e| inside(val(EDGES[e].1)) != inside(val(EDGES[e].2)));
                // Segments per face.
                let mut adj: [[u8; 2]; 12] = [[u8::MAX; 2]; 12];
                let link = |adj: &mut [[u8; 2]; 12], a: usize, b: usize| {
                    for (x, y) in [(a, b), (b, a)] {
                        if adj[x][0] == u8::MAX {
                            adj[x][0] = y as u8;
                        } else {
                            adj[x][1] = y as u8;
                        }
                    }
                };
                for (fa, side, q) in FACES {
                    let fe = [local_edge(q[0], q[1]), local_edge(q[1], q[2]), local_edge(q[2], q[3]), local_edge(q[3], q[0])];
                    let xs: Vec<usize> = (0..4).filter(|&i| crossed[fe[i]]).collect();
                    match xs.len() {
                        2 => link(&mut adj, fe[xs[0]], fe[xs[1]]),
                        4 => {
                            // Ambiguous: is the face centre on the side of q0 and q2?
                            let mut fc = g.cpos(c[0], c[1], c[2]);
                            for ax in 0..3 {
                                fc[ax] += if ax == fa { side as f64 * h } else { 0.5 * h };
                            }
                            let centre_in = inside(field.dist(fc));
                            if centre_in == inside(val(q[0])) {
                                // q0 and q2 joined through the centre: cut off q1 and q3.
                                link(&mut adj, fe[0], fe[1]);
                                link(&mut adj, fe[2], fe[3]);
                            } else {
                                link(&mut adj, fe[3], fe[0]);
                                link(&mut adj, fe[1], fe[2]);
                            }
                        }
                        _ => {}
                    }
                }
                // Walk the loops.
                let mut edge_loop = [u8::MAX; 12];
                let mut loops: Vec<Vec<usize>> = Vec::new();
                for start in 0..12 {
                    if !crossed[start] || edge_loop[start] != u8::MAX {
                        continue;
                    }
                    let id = loops.len() as u8;
                    let mut lp = Vec::new();
                    let (mut prev, mut cur) = (usize::MAX, start);
                    loop {
                        edge_loop[cur] = id;
                        lp.push(cur);
                        let nx = if adj[cur][0] as usize != prev { adj[cur][0] as usize } else { adj[cur][1] as usize };
                        if nx >= 12 || edge_loop[nx] != u8::MAX {
                            break;
                        }
                        prev = cur;
                        cur = nx;
                    }
                    loops.push(lp);
                }
                let lo_corner = g.cpos(c[0], c[1], c[2]);
                let cell_box = Aabb::new(lo_corner, add(lo_corner, [h; 3]));
                let mut verts = Vec::with_capacity(loops.len());
                let mut qefs = Vec::with_capacity(loops.len());
                for lp in &loops {
                    let pts: Vec<(V3, V3)> = lp
                        .iter()
                        .filter_map(|&e| {
                            let (ax, a, _) = EDGES[e];
                            let off = corner_off(a);
                            edge_lookup([c[0] + off[0], c[1] + off[1], c[2] + off[2]], ax).map(|i| herm[i])
                        })
                        .collect();
                    verts.push(place_vertex(field, &pts, cell_box, opt.truncate));
                    qefs.push(Qef::from_points(&pts));
                }
                CellOut { edge_loop, verts, qefs }
            })
            .collect()
    });
    let cell_out: Vec<CellOut> = cell_chunks.into_iter().flatten().collect();
    let mut first_vert = Vec::with_capacity(cell_out.len());
    let mut pos = Vec::new();
    let mut vcell = Vec::new();
    for (slot, co) in cell_out.iter().enumerate() {
        first_vert.push(pos.len() as u32);
        pos.extend_from_slice(&co.verts);
        let c = cell_coord(cells[slot]);
        let lo = g.cpos(c[0], c[1], c[2]);
        for _ in &co.verts {
            vcell.push(Aabb::new(lo, add(lo, [h; 3])));
        }
    }
    // Merge cells whose patch one vertex can carry (octree simplification).
    let remap: Vec<u32> = if opt.collapse_tol > 0.0 {
        let leaves: Vec<([usize; 3], Option<(u32, Qef)>)> = cells
            .iter()
            .enumerate()
            .map(|(slot, &ci)| {
                let co = &cell_out[slot];
                let v = if co.verts.len() == 1 { Some((first_vert[slot], co.qefs[0])) } else { None };
                (cell_coord(ci), v)
            })
            .collect();
        collapse(field, &g, &corner, &leaves, opt, &mut pos, &mut vcell)
    } else {
        (0..pos.len() as u32).collect()
    };

    // 5. Quads.
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(cross_ids.len() * 2);
    for (ei, &id) in cross_ids.iter().enumerate() {
        let (c, a) = unpack(id);
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        if c[u] == 0 || c[v] == 0 || c[u] > n[u] - 1 + 1 || c[v] > n[v] - 1 + 1 {
            continue;
        }
        // Cells around the edge, counter-clockwise about +axis in the (u, v) plane.
        let ring = [(1usize, 1usize), (0, 1), (0, 0), (1, 0)];
        let mut quad = [0u32; 4];
        let mut ok = true;
        for (q, (du, dv)) in ring.iter().enumerate() {
            let mut cell = c;
            cell[u] -= du;
            cell[v] -= dv;
            if cell[0] >= n[0] || cell[1] >= n[1] || cell[2] >= n[2] {
                ok = false;
                break;
            }
            let ci = g.cell_idx(cell[0], cell[1], cell[2]);
            let Ok(slot) = cells.binary_search(&ci) else {
                ok = false;
                break;
            };
            // The local edge of this cell that is our global edge.
            let mut bits = [0usize; 3];
            bits[u] = *du;
            bits[v] = *dv;
            let la = bits[0] | (bits[1] << 1) | (bits[2] << 2);
            let lb = la | (1 << a);
            let le = local_edge(la, lb);
            let lp = cell_out[slot].edge_loop[le];
            if lp == u8::MAX {
                ok = false;
                break;
            }
            quad[q] = remap[(first_vert[slot] + lp as u32) as usize];
        }
        if !ok {
            continue;
        }
        // Outward winding: if the lower corner is inside, the normal points to +axis.
        let lower_in = inside(corner[g.cidx(c[0], c[1], c[2])]);
        if !lower_in {
            quad = [quad[3], quad[2], quad[1], quad[0]];
        }
        // Merged cells can repeat a vertex: keep the distinct ones in order.
        let mut ring_v: Vec<u32> = Vec::with_capacity(4);
        for &v in &quad {
            if ring_v.last() != Some(&v) && ring_v.first() != Some(&v) {
                ring_v.push(v);
            }
        }
        if ring_v.len() < 3 {
            continue;
        }
        if ring_v.len() == 3 {
            tris.push([ring_v[0], ring_v[1], ring_v[2]]);
            continue;
        }
        let (p0, p1, p2, p3) = (pos[quad[0] as usize], pos[quad[1] as usize], pos[quad[2] as usize], pos[quad[3] as usize]);
        // Score each split: both triangles must face along the crossing's
        // normal and have area; then prefer the diagonal whose midpoint lies
        // closer to the surface.
        let en = herm[ei].1;
        let area_min = h * h * 1e-6;
        let good = |a: V3, b: V3, c: V3| {
            let nn = cross(sub(b, a), sub(c, a));
            len(nn) * 0.5 > area_min && dot(nn, en) > 0.0
        };
        let ok02 = good(p0, p1, p2) && good(p0, p2, p3);
        let ok13 = good(p0, p1, p3) && good(p1, p2, p3);
        let m02 = field.dist(lerp(p0, p2, 0.5)).abs();
        let m13 = field.dist(lerp(p1, p3, 0.5)).abs();
        let use02 = match (ok02, ok13) {
            (true, false) => true,
            (false, true) => false,
            _ => m02 <= m13,
        };
        if use02 {
            tris.push([quad[0], quad[1], quad[2]]);
            tris.push([quad[0], quad[2], quad[3]]);
        } else {
            tris.push([quad[0], quad[1], quad[3]]);
            tris.push([quad[1], quad[2], quad[3]]);
        }
    }

    repair_crossings(field, &mut pos, &tris, &vcell);
    flip_folds(field, &pos, &mut tris);

    // Normals at vertices and parts at triangle centres.
    let nrm: Vec<V3> = map_tiles(pos.len().div_ceil(CHUNK), opt.threads, |t| {
        let lo = t * CHUNK;
        let hi = (lo + CHUNK).min(pos.len());
        (lo..hi).map(|i| norm_or(field.eval(pos[i]).g, [0.0, 1.0, 0.0])).collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect();
    let tri_part: Vec<u32> = map_tiles(tris.len().div_ceil(CHUNK), opt.threads, |t| {
        let lo = t * CHUNK;
        let hi = (lo + CHUNK).min(tris.len());
        (lo..hi)
            .map(|i| {
                let tr = tris[i];
                let c = mul(add(add(pos[tr[0] as usize], pos[tr[1] as usize]), pos[tr[2] as usize]), 1.0 / 3.0);
                field.eval(c).part
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect();
    let mut m = Mesh { pos, nrm, tris, tri_part };
    drop_unused(&mut m);
    m
}

/// Accumulated tangent planes of a patch (the quadric of its Hermite data).
#[derive(Clone, Copy, Debug, Default)]
pub struct Qef {
    ata: [f64; 6],
    atb: V3,
    btb: f64,
    mass: V3,
    n: f64,
}

impl Qef {
    fn from_points(pts: &[(V3, V3)]) -> Qef {
        let mut q = Qef::default();
        for (p, nv) in pts {
            let d = dot(*nv, *p);
            q.ata[0] += nv[0] * nv[0];
            q.ata[1] += nv[0] * nv[1];
            q.ata[2] += nv[0] * nv[2];
            q.ata[3] += nv[1] * nv[1];
            q.ata[4] += nv[1] * nv[2];
            q.ata[5] += nv[2] * nv[2];
            q.atb = add(q.atb, mul(*nv, d));
            q.btb += d * d;
            q.mass = add(q.mass, *p);
            q.n += 1.0;
        }
        q
    }
    fn add(&self, o: &Qef) -> Qef {
        let mut r = *self;
        for i in 0..6 {
            r.ata[i] += o.ata[i];
        }
        r.atb = add(r.atb, o.atb);
        r.btb += o.btb;
        r.mass = add(r.mass, o.mass);
        r.n += o.n;
        r
    }
    fn mat(&self) -> [[f64; 3]; 3] {
        let a = &self.ata;
        [[a[0], a[1], a[2]], [a[1], a[3], a[4]], [a[2], a[4], a[5]]]
    }
    fn error(&self, x: V3) -> f64 {
        let m = self.mat();
        let ax = [dot(m[0], x), dot(m[1], x), dot(m[2], x)];
        dot(x, ax) - 2.0 * dot(x, self.atb) + self.btb
    }
    /// Truncated least-squares point, measured from the mean crossing.
    fn solve(&self, truncate: f64) -> V3 {
        if self.n <= 0.0 {
            return [0.0; 3];
        }
        let m = mul(self.mass, 1.0 / self.n);
        let a = self.mat();
        let am = [dot(a[0], m), dot(a[1], m), dot(a[2], m)];
        let b = sub(self.atb, am);
        let (vals, vecs) = sym_eigen(a);
        let lmax = vals[0].abs().max(vals[1].abs()).max(vals[2].abs());
        let mut y = [0.0; 3];
        for i in 0..3 {
            if vals[i] > truncate * lmax && vals[i] > 0.0 {
                y = add(y, mul(vecs[i], dot(vecs[i], b) / vals[i]));
            }
        }
        add(m, y)
    }
}

/// Is merging the `size`^3 fine cells starting at `lo` safe for topology?
/// The coarse cell must hold one simple patch: each of its edges crosses
/// the surface at most once along the fine grid, its faces are not
/// ambiguous, its face and cube centres agree with a corner, and its corner
/// signs form exactly one loop.
fn topology_ok(g: &Grid, corner: &[f64], lo: [usize; 3], size: usize) -> bool {
    let cc = g.corners();
    if lo[0] + size >= cc[0] || lo[1] + size >= cc[1] || lo[2] + size >= cc[2] {
        return false;
    }
    let cv = |o: [usize; 3]| inside(corner[g.cidx(lo[0] + o[0], lo[1] + o[1], lo[2] + o[2])]);
    let cs = |c: usize| {
        let o = corner_off(c);
        cv([o[0] * size, o[1] * size, o[2] * size])
    };
    // Edges: at most one sign change along the fine corners.
    for (ax, a, _) in EDGES {
        let o = corner_off(a);
        let base = [o[0] * size, o[1] * size, o[2] * size];
        let mut changes = 0;
        let mut prev = cv(base);
        for s in 1..=size {
            let mut p = base;
            p[ax] += s;
            let cur = cv(p);
            if cur != prev {
                changes += 1;
            }
            prev = cur;
        }
        if changes > 1 {
            return false;
        }
    }
    // Faces: not ambiguous, centre agrees with a corner.
    let half = size / 2;
    for (fa, side, q) in FACES {
        let signs: Vec<bool> = q.iter().map(|&c| cs(c)).collect();
        let crossings = (0..4).filter(|&i| signs[i] != signs[(i + 1) % 4]).count();
        if crossings == 4 {
            return false;
        }
        let mut fc = [half; 3];
        fc[fa] = side * size;
        let centre = cv(fc);
        if !signs.contains(&centre) {
            return false;
        }
    }
    let centre = cv([half; 3]);
    let all: Vec<bool> = (0..8).map(cs).collect();
    if !all.contains(&centre) {
        return false;
    }
    // Exactly one loop from the corner signs (no ambiguous faces left).
    let crossed: Vec<bool> = EDGES.iter().map(|e| cs(e.1) != cs(e.2)).collect();
    if !crossed.iter().any(|&x| x) {
        return false;
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); 12];
    for (_, _, q) in FACES {
        let fe = [local_edge(q[0], q[1]), local_edge(q[1], q[2]), local_edge(q[2], q[3]), local_edge(q[3], q[0])];
        let xs: Vec<usize> = fe.iter().copied().filter(|&e| crossed[e]).collect();
        if xs.len() == 2 {
            adj[xs[0]].push(xs[1]);
            adj[xs[1]].push(xs[0]);
        }
    }
    let mut seen = [false; 12];
    let mut loops = 0;
    for s in 0..12 {
        if !crossed[s] || seen[s] {
            continue;
        }
        loops += 1;
        let mut stack = vec![s];
        while let Some(e) = stack.pop() {
            if seen[e] {
                continue;
            }
            seen[e] = true;
            stack.extend(adj[e].iter().copied());
        }
    }
    loops == 1
}

/// Merge cells bottom-up in blocks of 2^3 (up to three levels). Returns the
/// map from fine vertices to the vertices kept; merged vertices are added
/// to `pos` (with their merged cell in `vcell`).
fn collapse(field: &Node, g: &Grid, corner: &[f64], leaves: &[([usize; 3], Option<(u32, Qef)>)], opt: &Options, pos: &mut Vec<V3>, vcell: &mut Vec<Aabb>) -> Vec<u32> {
    use std::collections::BTreeMap;
    let mut remap: Vec<u32> = (0..pos.len() as u32).collect();
    #[derive(Clone)]
    enum St {
        Leaf(Qef, Vec<u32>),
        Blocked,
    }
    let mut level: BTreeMap<[usize; 3], St> = BTreeMap::new();
    for (c, v) in leaves {
        level.insert(*c, match v {
            Some((vi, q)) => St::Leaf(*q, vec![*vi]),
            None => St::Blocked,
        });
    }
    for l in 1..=3usize {
        let mut groups: BTreeMap<[usize; 3], Vec<St>> = BTreeMap::new();
        for (c, st) in &level {
            groups.entry([c[0] >> 1, c[1] >> 1, c[2] >> 1]).or_default().push(st.clone());
        }
        let size = 1usize << l;
        let mut next: BTreeMap<[usize; 3], St> = BTreeMap::new();
        let mut merged_any = false;
        for (pk, kids) in groups {
            if kids.iter().any(|k| matches!(k, St::Blocked)) {
                next.insert(pk, St::Blocked);
                continue;
            }
            let lo = [pk[0] * size, pk[1] * size, pk[2] * size];
            if !topology_ok(g, corner, lo, size) {
                next.insert(pk, St::Blocked);
                continue;
            }
            let mut q = Qef::default();
            let mut members = Vec::new();
            for k in &kids {
                if let St::Leaf(kq, km) = k {
                    q = q.add(kq);
                    members.extend_from_slice(km);
                }
            }
            let lo_p = g.cpos(lo[0], lo[1], lo[2]);
            let cell = Aabb::new(lo_p, add(lo_p, [size as f64 * g.h; 3]));
            let margin = size as f64 * g.h * 0.01;
            let inner = cell.grow(-margin);
            let clamp = |x: V3| [x[0].clamp(inner.min[0], inner.max[0]), x[1].clamp(inner.min[1], inner.max[1]), x[2].clamp(inner.min[2], inner.max[2])];
            let mut x = clamp(q.solve(opt.truncate));
            let rms = (q.error(x).max(0.0) / q.n.max(1.0)).sqrt();
            if rms > opt.collapse_tol {
                next.insert(pk, St::Blocked);
                continue;
            }
            for _ in 0..3 {
                let s = field.eval(x);
                let g2 = dot(s.g, s.g);
                if g2 <= 1e-24 || s.d.abs() < 1e-9 {
                    break;
                }
                x = clamp(sub(x, mul(s.g, s.d / g2)));
            }
            if field.dist(x).abs() > opt.collapse_tol * 2.0 {
                next.insert(pk, St::Blocked);
                continue;
            }
            let vi = pos.len() as u32;
            pos.push(x);
            vcell.push(cell);
            for &m in &members {
                remap[m as usize] = vi;
            }
            next.insert(pk, St::Leaf(q, members));
            merged_any = true;
        }
        level = next;
        if !merged_any {
            break;
        }
    }
    // Vertices merged more than once point at the last (largest) merge.
    let mut out = remap.clone();
    for (i, o) in out.iter_mut().enumerate() {
        let mut v = remap[i];
        while (v as usize) < remap.len() && remap[v as usize] != v {
            v = remap[v as usize];
        }
        *o = v;
    }
    out.resize(pos.len(), 0);
    out
}

/// The validator with local re-placement (note 13): vertices of triangles
/// that cross others, or fold against the field, are moved to the mean of their neighbours, kept in
/// their own cell and projected back onto the surface; repeated with a
/// growing neighbourhood until no crossing is left (at most four rounds).
fn repair_crossings(field: &Node, pos: &mut [V3], tris: &[[u32; 3]], vcell: &[Aabb]) {
    let valid = vec![true; tris.len()];
    // Sorted vertex adjacency.
    let mut pairs: Vec<(u32, u32)> = Vec::with_capacity(tris.len() * 6);
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            pairs.push((a, b));
            pairs.push((b, a));
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    let nbrs = |v: u32| -> &[(u32, u32)] {
        let lo = pairs.partition_point(|p| p.0 < v);
        let hi = pairs.partition_point(|p| p.0 <= v);
        &pairs[lo..hi]
    };
    for round in 0..4 {
        let bad = crate::meshcheck::self_intersections(pos, tris, &valid);
        // Folded triangles: facing against the field at all three corners.
        let folded: Vec<usize> = (0..tris.len())
            .filter(|&i| {
                let t = tris[i];
                let n = norm_or(cross(sub(pos[t[1] as usize], pos[t[0] as usize]), sub(pos[t[2] as usize], pos[t[0] as usize])), [0.0; 3]);
                t.iter().all(|&v| dot(n, norm_or(field.eval(pos[v as usize]).g, [0.0; 3])) < -0.2)
            })
            .collect();
        if bad.is_empty() && folded.is_empty() {
            return;
        }
        let mut verts: Vec<u32> = Vec::new();
        for (a, b) in &bad {
            verts.extend_from_slice(&tris[*a as usize]);
            verts.extend_from_slice(&tris[*b as usize]);
        }
        for &i in &folded {
            verts.extend_from_slice(&tris[i]);
        }
        verts.sort_unstable();
        verts.dedup();
        for _ in 0..round {
            let mut grown = verts.clone();
            for &v in &verts {
                grown.extend(nbrs(v).iter().map(|p| p.1));
            }
            grown.sort_unstable();
            grown.dedup();
            verts = grown;
        }
        let snapshot = pos.to_vec();
        for &v in &verts {
            let nb = nbrs(v);
            if nb.is_empty() {
                continue;
            }
            let mut m = [0.0; 3];
            for p in nb {
                m = add(m, snapshot[p.1 as usize]);
            }
            m = mul(m, 1.0 / nb.len() as f64);
            let cell = vcell[v as usize];
            let margin = (cell.max[0] - cell.min[0]) * 0.01;
            let inner = cell.grow(-margin);
            let clamp = |x: V3| [x[0].clamp(inner.min[0], inner.max[0]), x[1].clamp(inner.min[1], inner.max[1]), x[2].clamp(inner.min[2], inner.max[2])];
            let mut x = clamp(m);
            for _ in 0..3 {
                let s = field.eval(x);
                let g2 = dot(s.g, s.g);
                if g2 <= 1e-24 || s.d.abs() < 1e-9 {
                    break;
                }
                x = clamp(sub(x, mul(s.g, s.d / g2)));
            }
            pos[v as usize] = x;
        }
    }
}

/// Triangles that still fold against the field get their worst edge
/// flipped when the two new triangles both face out (edge flips keep the
/// mesh closed and manifold). Up to three passes, in triangle order.
fn flip_folds(field: &Node, pos: &[V3], tris: &mut [[u32; 3]]) {
    let gn: Vec<V3> = pos.iter().map(|p| norm_or(field.eval(*p).g, [0.0; 3])).collect();
    let tri_n = |t: [u32; 3]| norm_or(cross(sub(pos[t[1] as usize], pos[t[0] as usize]), sub(pos[t[2] as usize], pos[t[0] as usize])), [0.0; 3]);
    let faces_out = |t: [u32; 3]| {
        let n = tri_n(t);
        let area = len(cross(sub(pos[t[1] as usize], pos[t[0] as usize]), sub(pos[t[2] as usize], pos[t[0] as usize])));
        area > 1e-14 && dot(n, add(add(gn[t[0] as usize], gn[t[1] as usize]), gn[t[2] as usize])) > 0.0
    };
    let folded = |t: [u32; 3]| {
        let n = tri_n(t);
        t.iter().all(|&v| dot(n, gn[v as usize]) < -0.2)
    };
    for _ in 0..3 {
        let bad: Vec<usize> = (0..tris.len()).filter(|&i| folded(tris[i])).collect();
        if bad.is_empty() {
            return;
        }
        let mut edges: Vec<(u32, u32, usize)> = Vec::with_capacity(tris.len() * 3);
        for (i, t) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                edges.push((a.min(b), a.max(b), i));
            }
        }
        edges.sort_unstable();
        let find = |edges: &[(u32, u32, usize)], a: u32, b: u32| -> Vec<usize> {
            let key = (a.min(b), a.max(b));
            let lo = edges.partition_point(|e| (e.0, e.1) < key);
            edges[lo..].iter().take_while(|e| (e.0, e.1) == key).map(|e| e.2).collect()
        };
        let mut touched = vec![false; tris.len()];
        for &ti in &bad {
            if touched[ti] {
                continue;
            }
            let t = tris[ti];
            for k in 0..3 {
                let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                let around = find(&edges, a, b);
                let Some(&ui) = around.iter().find(|&&u| u != ti) else { continue };
                if around.len() != 2 || touched[ui] {
                    continue;
                }
                let u = tris[ui];
                let Some(&d) = u.iter().find(|&&v| v != a && v != b) else { continue };
                if !find(&edges, c, d).is_empty() || c == d {
                    continue;
                }
                let (t2, u2) = ([c, a, d], [d, b, c]);
                if faces_out(t2) && faces_out(u2) {
                    tris[ti] = t2;
                    tris[ui] = u2;
                    touched[ti] = true;
                    touched[ui] = true;
                    break;
                }
            }
        }
    }
}

/// Remove vertices no triangle uses (keeps order).
fn drop_unused(m: &mut Mesh) {
    let mut used = vec![false; m.pos.len()];
    for t in &m.tris {
        for &v in t {
            used[v as usize] = true;
        }
    }
    if used.iter().all(|&u| u) {
        return;
    }
    let mut remap = vec![u32::MAX; m.pos.len()];
    let (mut pos, mut nrm) = (Vec::new(), Vec::new());
    for i in 0..m.pos.len() {
        if used[i] {
            remap[i] = pos.len() as u32;
            pos.push(m.pos[i]);
            nrm.push(m.nrm[i]);
        }
    }
    for t in &mut m.tris {
        *t = t.map(|v| remap[v as usize]);
    }
    m.pos = pos;
    m.nrm = nrm;
}

/// Vertex of one patch: tangent-plane fit from the mean crossing point,
/// truncated, clamped to the cell, then moved onto the surface.
fn place_vertex(field: &Node, pts: &[(V3, V3)], cell: Aabb, truncate: f64) -> V3 {
    if pts.is_empty() {
        return cell.centre();
    }
    let k = pts.len() as f64;
    let mut m = [0.0; 3];
    for (p, _) in pts {
        m = add(m, *p);
    }
    m = mul(m, 1.0 / k);
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for (p, nv) in pts {
        let d = dot(*nv, sub(*p, m));
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += nv[i] * nv[j];
            }
            b[i] += nv[i] * d;
        }
    }
    let (vals, vecs) = sym_eigen(a);
    let lmax = vals[0].abs().max(vals[1].abs()).max(vals[2].abs());
    let mut y = [0.0; 3];
    for i in 0..3 {
        if vals[i] > truncate * lmax && vals[i] > 0.0 {
            let v = vecs[i];
            y = add(y, mul(v, dot(v, b) / vals[i]));
        }
    }
    let margin = (cell.max[0] - cell.min[0]) * 0.01;
    let inner = cell.grow(-margin);
    let clamp = |x: V3| [x[0].clamp(inner.min[0], inner.max[0]), x[1].clamp(inner.min[1], inner.max[1]), x[2].clamp(inner.min[2], inner.max[2])];
    let mut x = clamp(add(m, y));
    // Bounded Newton steps onto the surface.
    for _ in 0..3 {
        let s = field.eval(x);
        let g2 = dot(s.g, s.g);
        if g2 <= 1e-24 || s.d.abs() < 1e-9 {
            break;
        }
        x = clamp(sub(x, mul(s.g, s.d / g2)));
    }
    x
}

/// Eigen-decomposition of a symmetric 3x3 matrix by cyclic Jacobi rotations
/// (fixed sweep count, deterministic). Values sorted largest first.
pub fn sym_eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [V3; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..12 {
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            let apq = a[p][q];
            if apq.abs() < 1e-30 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * apq);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let (apk, aqk) = (a[p][k], a[q][k]);
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for row in v.iter_mut() {
                let (vkp, vkq) = (row[p], row[q]);
                row[p] = c * vkp - s * vkq;
                row[q] = s * vkp + c * vkq;
            }
        }
    }
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&i, &j| a[j][j].total_cmp(&a[i][i]));
    let vals = [a[idx[0]][idx[0]], a[idx[1]][idx[1]], a[idx[2]][idx[2]]];
    let col = |c: usize| [v[0][c], v[1][c], v[2][c]];
    (vals, [col(idx[0]), col(idx[1]), col(idx[2])])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meshcheck;

    fn check(m: &Mesh) -> meshcheck::Report {
        meshcheck::check(&m.pos, &m.tris, &meshcheck::Options { ref_normals: Some(m.nrm.clone()), ..Default::default() })
    }

    #[test]
    fn eigen_reconstructs_the_matrix() {
        let a = [[4.0, 1.0, 0.5], [1.0, 3.0, 0.2], [0.5, 0.2, 1.0]];
        let (vals, vecs) = sym_eigen(a);
        for i in 0..3 {
            let av = [dot(a[0], vecs[i]), dot(a[1], vecs[i]), dot(a[2], vecs[i])];
            assert!(len(sub(av, mul(vecs[i], vals[i]))) < 1e-12);
        }
        assert!(vals[0] >= vals[1] && vals[1] >= vals[2]);
    }

    #[test]
    fn sphere_is_closed_manifold_and_accurate() {
        let f = Node::Sphere { r: 0.5 }.prepare();
        let m = extract(&f, f.bounds(), &Options::new(0.05, 2));
        let r = check(&m);
        assert!(r.ok(), "{:?}", r.summary());
        assert_eq!(r.components, 1);
        let vol = 4.0 / 3.0 * core::f64::consts::PI * 0.125;
        assert!((r.volume - vol).abs() / vol < 0.02, "volume {} vs {vol}", r.volume);
        for p in &m.pos {
            assert!((len(*p) - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn box_keeps_sharp_corners() {
        let f = Node::RoundBox { half: [0.4, 0.25, 0.3], r: 0.0 }.at(quat_ypr(0.3, 0.2, 0.1), [0.01, 0.02, 0.03]).prepare();
        let m = extract(&f, f.bounds(), &Options::new(0.04, 2));
        let r = check(&m);
        assert!(r.ok(), "{:?}", r.summary());
        assert!((r.volume - 0.8 * 0.5 * 0.6).abs() < 0.005, "volume {}", r.volume);
    }

    #[test]
    fn thin_and_touching_shapes_stay_manifold() {
        let f = Node::Union(vec![
            Node::RoundBox { half: [0.5, 0.02, 0.5], r: 0.0 },
            Node::Sphere { r: 0.2 }.at(QID, [0.0, 0.25, 0.0]),
            Node::Torus { major: 0.3, minor: 0.04 }.at(quat_ypr(0.0, 1.2, 0.0), [0.3, 0.3, 0.1]),
        ])
        .prepare();
        for h in [0.03, 0.05, 0.08] {
            let m = extract(&f, f.bounds(), &Options::new(h, 2));
            let r = check(&m);
            assert!(r.watertight(), "h {h}: {:?}", r.summary());
        }
    }

    #[test]
    fn merging_cells_cuts_triangles_and_stays_closed() {
        let f = Node::SmoothUnion {
            k: 0.03,
            children: vec![Node::RoundBox { half: [0.6, 0.3, 0.4], r: 0.06 }, Node::Cylinder { r: 0.2, half_h: 0.4, round: 0.04 }.at(QID, [0.2, 0.4, 0.0])],
        }
        .prepare();
        let fine = extract(&f, f.bounds(), &Options::new(0.02, 2));
        let mut o = Options::new(0.02, 2);
        o.collapse_tol = 0.002;
        let merged = extract(&f, f.bounds(), &o);
        let rf = check(&fine);
        let rm = check(&merged);
        assert!(rm.ok(), "{:?}", rm.summary());
        assert!(merged.tris.len() * 2 < fine.tris.len(), "{} vs {}", merged.tris.len(), fine.tris.len());
        assert!((rm.volume - rf.volume).abs() / rf.volume < 0.01, "{} vs {}", rm.volume, rf.volume);
    }

    #[test]
    fn identical_for_any_thread_count() {
        let f = Node::SmoothUnion {
            k: 0.05,
            children: vec![Node::RoundBox { half: [0.4, 0.2, 0.3], r: 0.05 }, Node::Capsule { a: [-0.3, 0.2, 0.0], b: [0.3, 0.4, 0.1], r: 0.1 }],
        }
        .prepare();
        let base = extract(&f, f.bounds(), &Options::new(0.03, 1));
        for t in [2, 4, 16] {
            let m = extract(&f, f.bounds(), &Options::new(0.03, t));
            assert_eq!(m.pos, base.pos);
            assert_eq!(m.tris, base.tris);
        }
    }
}
