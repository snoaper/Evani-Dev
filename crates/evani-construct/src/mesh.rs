//! Exact piece meshes: closed, outward-facing solids built from grids of
//! points, never from fields (nothing is melted, so a 4 mm seam stays a
//! seam at every tier).
//!
//! Two builders cover every piece kind: [`slab`] (a top and a bottom grid:
//! boards, planks, blocks, bands) and [`ring_solid`] (rings along a path:
//! sweeps, ropes, spars, solids of revolution, lofts). Faces keep their own
//! vertices so edges stay hard; normals are smooth within a face. A built
//! solid is checked by its signed volume and flipped if it came out
//! inside-out, so any parameter direction gives an outward mesh.

use crate::vec::*;

/// A triangle mesh with per-vertex normals and grain UVs in metres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub pos: Vec<V3>,
    pub nrm: Vec<V3>,
    /// Grain coordinates in metres: `u` along the piece, `v` across.
    pub uv: Vec<[f64; 2]>,
    pub tris: Vec<[u32; 3]>,
}

fn tri_n(a: V3, b: V3, c: V3) -> V3 {
    cross(sub(b, a), sub(c, a))
}

impl Mesh {
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in &self.pos {
            b.add_point(*p);
        }
        b
    }
    /// Signed volume (positive when closed and outward-facing).
    pub fn volume(&self) -> f64 {
        let mut v = 0.0;
        for t in &self.tris {
            let (a, b, c) = (self.pos[t[0] as usize], self.pos[t[1] as usize], self.pos[t[2] as usize]);
            v += dot(a, cross(b, c));
        }
        v / 6.0
    }
    pub fn append(&mut self, o: &Mesh) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(&o.pos);
        self.nrm.extend_from_slice(&o.nrm);
        self.uv.extend_from_slice(&o.uv);
        self.tris.extend(o.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    /// Reverse every triangle and normal.
    pub fn flip(&mut self) {
        for t in &mut self.tris {
            t.swap(1, 2);
        }
        for n in &mut self.nrm {
            *n = neg(*n);
        }
    }
    /// The mesh moved by rotation `q`, then translation `t`; `mirror` first
    /// negates x (and fixes the winding so it stays outward).
    pub fn transformed(&self, q: Quat, t: V3, mirror: bool) -> Mesh {
        let sx = if mirror { -1.0 } else { 1.0 };
        let mut m = Mesh {
            pos: self.pos.iter().map(|p| add(qrot(q, [p[0] * sx, p[1], p[2]]), t)).collect(),
            nrm: self.nrm.iter().map(|n| qrot(q, [n[0] * sx, n[1], n[2]])).collect(),
            uv: self.uv.clone(),
            tris: self.tris.clone(),
        };
        if mirror {
            for tr in &mut m.tris {
                tr.swap(1, 2);
            }
        }
        m
    }
    /// One smooth face from a grid of points (`rows` by `cols`) with UVs:
    /// quads `(r,c) (r+1,c) (r+1,c+1) (r,c+1)`, facing `d_row x d_col`.
    /// Zero-area triangles (a pole) are left out.
    pub fn face(&mut self, grid: &[Vec<V3>], uv: &[Vec<[f64; 2]>]) {
        let rows = grid.len();
        let cols = grid.first().map_or(0, Vec::len);
        if rows < 2 || cols < 2 {
            return;
        }
        let base = self.pos.len() as u32;
        let mut acc = vec![[0.0; 3]; rows * cols];
        for (r, row) in grid.iter().enumerate() {
            for (c, p) in row.iter().enumerate() {
                self.pos.push(*p);
                self.uv.push(uv.get(r).and_then(|x| x.get(c)).copied().unwrap_or([0.0; 2]));
            }
        }
        let id = |r: usize, c: usize| (r * cols + c) as u32;
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let (a, b, cc, d) = (id(r, c), id(r + 1, c), id(r + 1, c + 1), id(r, c + 1));
                for t in [[a, b, cc], [a, cc, d]] {
                    let p = |k: u32| self.pos[(base + k) as usize];
                    let n = tri_n(p(t[0]), p(t[1]), p(t[2]));
                    if len(n) <= 1e-14 {
                        continue;
                    }
                    for k in t {
                        acc[k as usize] = add(acc[k as usize], n);
                    }
                    self.tris.push([base + t[0], base + t[1], base + t[2]]);
                }
            }
        }
        // A vertex no kept triangle uses (a pole copy) takes the face's mean.
        let mean = acc.iter().fold([0.0; 3], |s, n| add(s, *n));
        for a in acc {
            self.nrm.push(norm_or(a, norm_or(mean, [0.0, 1.0, 0.0])));
        }
    }
    /// A fan from `centre` round the closed `ring` (in order), facing
    /// `(ring[i] - centre) x (ring[i+1] - centre)`.
    pub fn fan(&mut self, centre: V3, ring: &[V3], uv_scale: f64) {
        if ring.len() < 3 {
            return;
        }
        let n = norm_or(ring.windows(2).chain(std::iter::once([ring[ring.len() - 1], ring[0]].as_slice())).fold([0.0; 3], |s, w| add(s, tri_n(centre, w[0], w[1]))), [0.0, 1.0, 0.0]);
        let (u, v) = basis(n);
        let base = self.pos.len() as u32;
        let to_uv = |p: V3| {
            let d = sub(p, centre);
            [dot(d, u) * uv_scale, dot(d, v) * uv_scale]
        };
        self.pos.push(centre);
        self.nrm.push(n);
        self.uv.push([0.0; 2]);
        for p in ring {
            self.pos.push(*p);
            self.nrm.push(n);
            self.uv.push(to_uv(*p));
        }
        let m = ring.len() as u32;
        for i in 0..m {
            let (a, b) = (base + 1 + i, base + 1 + (i + 1) % m);
            if len(tri_n(centre, ring[i as usize], ring[((i + 1) % m) as usize])) > 1e-14 {
                self.tris.push([base, a, b]);
            }
        }
    }
    /// Drop vertices no triangle uses (a doubled grid column's copies).
    pub fn compact(&mut self) {
        let mut map = vec![u32::MAX; self.pos.len()];
        let mut keep = Vec::new();
        for t in &mut self.tris {
            for v in t.iter_mut() {
                let i = *v as usize;
                if map[i] == u32::MAX {
                    map[i] = keep.len() as u32;
                    keep.push(i);
                }
                *v = map[i];
            }
        }
        self.pos = keep.iter().map(|&i| self.pos[i]).collect();
        self.nrm = keep.iter().map(|&i| self.nrm[i]).collect();
        self.uv = keep.iter().map(|&i| self.uv[i]).collect();
    }
    /// Flip if the (closed) mesh came out inside-out.
    pub fn make_outward(&mut self) {
        if self.volume() < 0.0 {
            self.flip();
        }
    }
}

/// Two unit axes perpendicular to `n`.
pub fn basis(n: V3) -> (V3, V3) {
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = norm_or(cross(a, n), [1.0, 0.0, 0.0]);
    (u, cross(n, u))
}

fn cum(pts: impl Iterator<Item = V3>) -> Vec<f64> {
    let mut out = Vec::new();
    let mut last: Option<V3> = None;
    let mut s = 0.0;
    for p in pts {
        if let Some(l) = last {
            s += dist(l, p);
        }
        out.push(s);
        last = Some(p);
    }
    out
}

/// A closed solid between two grids of the same shape (`rows` along the
/// piece by `cols` across): `top` and `bottom` faces, two long sides and two
/// end caps, each with hard edges. Boards, planks, blocks and bands.
pub fn slab(top: &[Vec<V3>], bottom: &[Vec<V3>]) -> Mesh {
    slab_creased(top, bottom, &[])
}

/// [`slab`] whose top face has hard edges at the `creases` columns (each
/// run between creases is its own smooth face): chamfered boards.
pub fn slab_creased(top: &[Vec<V3>], bottom: &[Vec<V3>], creases: &[usize]) -> Mesh {
    let mut m = Mesh::default();
    let rows = top.len();
    let cols = top.first().map_or(0, Vec::len);
    if rows < 2 || cols < 2 || bottom.len() != rows || bottom.iter().any(|r| r.len() != cols) {
        return m;
    }
    let col = |g: &[Vec<V3>], c: usize| -> Vec<V3> { g.iter().map(|r| r[c]).collect() };
    let row_uv = |g: &[Vec<V3>]| -> Vec<Vec<[f64; 2]>> {
        let along: Vec<Vec<f64>> = (0..cols).map(|c| cum(g.iter().map(|r| r[c]))).collect();
        g.iter().enumerate().map(|(r, row)| {
            let across = cum(row.iter().copied());
            (0..cols).map(|c| [along[c][r], across[c]]).collect()
        }).collect()
    };
    // Top: rows x cols (split at the creases); bottom: transposed (faces
    // the other way).
    let tuv = row_uv(top);
    let mut cuts: Vec<usize> = creases.iter().copied().filter(|&c| c > 0 && c < cols - 1).collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts.insert(0, 0);
    cuts.push(cols - 1);
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let g: Vec<Vec<V3>> = top.iter().map(|r| r[a..=b].to_vec()).collect();
        let uv: Vec<Vec<[f64; 2]>> = tuv.iter().map(|r| r[a..=b].to_vec()).collect();
        m.face(&g, &uv);
    }
    let bt: Vec<Vec<V3>> = (0..cols).map(|c| col(bottom, c)).collect();
    let buv: Vec<Vec<[f64; 2]>> = row_uv(bottom).iter().fold(vec![Vec::new(); cols], |mut acc, r| {
        for (c, x) in r.iter().enumerate() {
            acc[c].push(*x);
        }
        acc
    });
    m.face(&bt, &buv);
    let depth = |a: V3, b: V3| dist(a, b);
    // Long sides (first and last column) and end caps (first and last row).
    let side = |c: usize, top_first: bool| -> (Vec<Vec<V3>>, Vec<Vec<[f64; 2]>>) {
        let along = cum(top.iter().map(|r| r[c]));
        let g: Vec<Vec<V3>> = (0..rows).map(|r| if top_first { vec![top[r][c], bottom[r][c]] } else { vec![bottom[r][c], top[r][c]] }).collect();
        let uv = (0..rows).map(|r| {
            let d = depth(top[r][c], bottom[r][c]);
            if top_first { vec![[along[r], 0.0], [along[r], d]] } else { vec![[along[r], d], [along[r], 0.0]] }
        }).collect();
        (g, uv)
    };
    let end = |r: usize, top_first: bool| -> (Vec<Vec<V3>>, Vec<Vec<[f64; 2]>>) {
        let across = cum(top[r].iter().copied());
        let g: Vec<Vec<V3>> = (0..cols).map(|c| if top_first { vec![top[r][c], bottom[r][c]] } else { vec![bottom[r][c], top[r][c]] }).collect();
        let uv = (0..cols).map(|c| {
            let d = depth(top[r][c], bottom[r][c]);
            if top_first { vec![[across[c], 0.0], [across[c], d]] } else { vec![[across[c], d], [across[c], 0.0]] }
        }).collect();
        (g, uv)
    };
    for (g, uv) in [side(0, false), side(cols - 1, true), end(0, true), end(rows - 1, false)] {
        m.face(&g, &uv);
    }
    m.make_outward();
    if !creases.is_empty() {
        // A creased slab's back doubles its outer columns: drop the copies.
        m.compact();
    }
    m
}

/// A box of half sizes `half` in a frame (`x` along `b`, `y` along `n`,
/// `z` along `t` of the rotation `q`) at `c`.
pub fn block(c: V3, q: Quat, half: V3) -> Mesh {
    let p = |x: f64, y: f64, z: f64| add(c, qrot(q, [x * half[0], y * half[1], z * half[2]]));
    let top = vec![vec![p(-1.0, 1.0, -1.0), p(1.0, 1.0, -1.0)], vec![p(-1.0, 1.0, 1.0), p(1.0, 1.0, 1.0)]];
    let bot = vec![vec![p(-1.0, -1.0, -1.0), p(1.0, -1.0, -1.0)], vec![p(-1.0, -1.0, 1.0), p(1.0, -1.0, 1.0)]];
    slab(&top, &bot)
}

/// A closed solid through `rings` (each the same number of points, in
/// order round the ring). `smooth`: normals shared round the ring (ropes,
/// spars); otherwise each ring edge is its own flat strip (beams, bands).
/// A ring of coincident points (a pole of a solid of revolution) closes
/// that end; any other end ring is capped with a fan from its centroid.
pub fn ring_solid(rings: &[Vec<V3>], smooth: bool) -> Mesh {
    let mut m = Mesh::default();
    let k = rings.len();
    let n = rings.first().map_or(0, Vec::len);
    if k < 2 || n < 3 || rings.iter().any(|r| r.len() != n) {
        return m;
    }
    let centroid = |r: &[V3]| mul(r.iter().fold([0.0; 3], |s, p| add(s, *p)), 1.0 / r.len() as f64);
    let pole = |r: &[V3]| {
        let c = centroid(r);
        r.iter().all(|p| dist(*p, c) < 1e-9)
    };
    let along = cum(rings.iter().map(|r| centroid(r)));
    let girth: Vec<f64> = {
        let r = &rings[k / 2];
        let mut s = vec![0.0];
        for i in 0..n {
            let l = s[i] + dist(r[i], r[(i + 1) % n]);
            s.push(l);
        }
        s
    };
    if smooth {
        let grid: Vec<Vec<V3>> = rings.iter().map(|r| (0..=n).map(|i| r[i % n]).collect()).collect();
        let uv: Vec<Vec<[f64; 2]>> = (0..k).map(|a| (0..=n).map(|i| [along[a], girth[i]]).collect()).collect();
        m.face(&grid, &uv);
    } else {
        for i in 0..n {
            let grid: Vec<Vec<V3>> = rings.iter().map(|r| vec![r[i], r[(i + 1) % n]]).collect();
            let uv: Vec<Vec<[f64; 2]>> = (0..k).map(|a| vec![[along[a], girth[i]], [along[a], girth[i + 1]]]).collect();
            m.face(&grid, &uv);
        }
    }
    // Weld pole copies so the solid closes there (one vertex per pole).
    for (ri, r) in [(0usize, &rings[0]), (k - 1, &rings[k - 1])] {
        if pole(r) {
            let c = centroid(r);
            let ids: Vec<u32> = (0..m.pos.len() as u32).filter(|&v| dist(m.pos[v as usize], c) < 1e-9).collect();
            if let Some(&keep) = ids.first() {
                let nn = norm_or(ids.iter().fold([0.0; 3], |s, &v| add(s, m.nrm[v as usize])), [0.0, 1.0, 0.0]);
                m.nrm[keep as usize] = nn;
                for t in &mut m.tris {
                    for v in t.iter_mut() {
                        if ids.contains(v) {
                            *v = keep;
                        }
                    }
                }
            }
            let _ = ri;
        }
    }
    // Caps: the side quads traverse the last ring forward and the first ring
    // backward, so the caps go the other way round.
    if !pole(&rings[0]) {
        m.fan(centroid(&rings[0]), &rings[0], 1.0);
    }
    if !pole(&rings[k - 1]) {
        let rev: Vec<V3> = rings[k - 1].iter().rev().copied().collect();
        m.fan(centroid(&rings[k - 1]), &rev, 1.0);
    }
    m.make_outward();
    m
}

/// Rings of a 2D `profile` (x, y in the frame's `b`, `n` axes) along frames.
pub fn sweep_rings(frames: &[crate::curve::Frame], profile: &[[f64; 2]], scale: &dyn Fn(usize) -> f64) -> Vec<Vec<V3>> {
    frames.iter().enumerate().map(|(i, f)| {
        let s = scale(i);
        profile.iter().map(|p| f.local(p[0] * s, p[1] * s, 0.0)).collect()
    }).collect()
}

/// A regular polygon profile of radius `r` (`n` sides).
pub fn circle(r: f64, n: usize) -> Vec<[f64; 2]> {
    let n = n.max(3);
    (0..n).map(|i| {
        let (s, c) = crate::dmath::sin_cos(core::f64::consts::TAU * i as f64 / n as f64);
        [r * c, r * s]
    }).collect()
}

/// A laid-rope profile of outer radius `r`: `strands` round crowns, each
/// `per` points, creased to 72% of the radius between strands.
pub fn rope(r: f64, strands: usize, per: usize) -> Vec<[f64; 2]> {
    let (strands, per) = (strands.max(2), per.max(2));
    let n = strands * per;
    (0..n).map(|i| {
        let a = core::f64::consts::TAU * i as f64 / n as f64;
        // 0 at a crease, 1 at a crown: |sin| of half a strand's turn.
        let k = crate::dmath::sin_cos(a * strands as f64 / 2.0 + core::f64::consts::FRAC_PI_2).0.abs();
        let rr = r * (0.72 + 0.28 * crate::dmath::pow(k, 0.5));
        let (s, c) = crate::dmath::sin_cos(a);
        [rr * c, rr * s]
    }).collect()
}

/// A rectangle profile `w` by `h`, centred, or with its bottom at `y = 0`.
pub fn rect(w: f64, h: f64, centred: bool) -> Vec<[f64; 2]> {
    let (y0, y1) = if centred { (-h / 2.0, h / 2.0) } else { (0.0, h) };
    vec![[-w / 2.0, y0], [w / 2.0, y0], [w / 2.0, y1], [-w / 2.0, y1]]
}

/// Rings of a solid of revolution about y: profile points (radius, height),
/// angles `a0..a1` in `seg` steps (a full turn repeats no ring).
pub fn lathe_rings(profile: &[[f64; 2]], seg: usize, centre: V3) -> Vec<Vec<V3>> {
    let seg = seg.max(3);
    profile.iter().map(|p| {
        (0..seg).map(|i| {
            let (s, c) = crate::dmath::sin_cos(core::f64::consts::TAU * i as f64 / seg as f64);
            add(centre, [p[0] * c, p[1], -p[0] * s])
        }).collect()
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_by_position(m: &Mesh) -> bool {
        // Every edge (by welded position) used once each way.
        let key = |p: V3| [(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64, (p[2] * 1e6).round() as i64];
        let mut e: Vec<([i64; 3], [i64; 3])> = Vec::new();
        for t in &m.tris {
            for k in 0..3 {
                e.push((key(m.pos[t[k] as usize]), key(m.pos[t[(k + 1) % 3] as usize])));
            }
        }
        let mut f = e.clone();
        f.sort();
        e.iter().all(|(a, b)| f.binary_search(&(*b, *a)).is_ok())
    }

    #[test]
    fn slabs_blocks_and_rings_are_closed_and_outward() {
        let b = block([1.0, 2.0, 3.0], quat_axis([0.0, 1.0, 0.0], 0.4), [0.5, 0.1, 2.0]);
        assert!((b.volume() - 0.8).abs() < 1e-9, "{}", b.volume());
        assert_eq!(b.tris.len(), 12);
        assert!(closed_by_position(&b));
        // A bent plank: an arc of 6 rows, 3 columns across, 2 cm thick.
        let top: Vec<Vec<V3>> = (0..6).map(|i| {
            let a = i as f64 * 0.2;
            (0..3).map(|j| [crate::dmath::sin(a), crate::dmath::cos(a), j as f64 * 0.05]).collect()
        }).collect();
        let bot: Vec<Vec<V3>> = top.iter().map(|r| r.iter().map(|p| mul(*p, 0.98)).collect()).collect();
        let s = slab(&top, &bot);
        assert!(s.volume() > 0.0 && closed_by_position(&s));
        // A lathe dome closing at a pole, and a swept rope with caps.
        let dome = ring_solid(&lathe_rings(&[[0.0, 0.0], [0.3, 0.0], [0.3, 0.1], [0.0, 0.25]], 12, [0.0; 3]), true);
        assert!(dome.volume() > 0.0 && closed_by_position(&dome), "{}", dome.volume());
        let path = crate::curve::Path::new(&crate::curve::Curve::Catenary { a: [0.0, 1.0, 0.0], b: [1.0, 1.0, 0.0], sag: 0.2 });
        let rope = ring_solid(&sweep_rings(&path.frames(12, None), &circle(0.02, 8), &|_| 1.0), true);
        assert!(rope.volume() > 0.0 && closed_by_position(&rope));
        let beam = ring_solid(&sweep_rings(&path.frames(6, Some([0.0, 0.0, 1.0])), &rect(0.05, 0.03, true), &|_| 1.0), false);
        assert!(beam.volume() > 0.0 && closed_by_position(&beam));
        // Mirroring keeps a solid outward.
        let mut mm = b.transformed(QID, [0.0; 3], true);
        assert!(mm.volume() > 0.0);
        mm.flip();
        assert!(mm.volume() < 0.0);
    }
}
