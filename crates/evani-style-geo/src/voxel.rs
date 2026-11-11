//! Voxel geometry (WP3, docs/studio/WORK_PACKAGES.md): an asset's source
//! rebuilt from cubes. All our own code; the ideas (conservative
//! triangle-box overlap voxelisation, a thin-part keep pass, cubes rebuilt
//! from a shipped grid) come from the research's papers and articles.
//!
//! 1. **Cube size** from the asset: the style's `cell` (radius-1 units, the
//!    cube-size control already applied) scaled down for an asset whose
//!    thinnest tagged part (`thin: true`: legs, ears, mane, tail, horns) is
//!    narrower than [`THIN_CELLS`] cubes at the style's own size
//!    ([`REF_CELL`]), never below [`MIN_FIT`] of it ([`fit_cell`]).
//! 2. **Conservative voxelisation**: every cell a triangle touches (the
//!    separating-axis box test, inclusive) is solid, so nothing thinner
//!    than a cube can drop out; then the inside is filled (a flood from
//!    outside; what it can't reach is inside).
//! 3. **Regions**: a surface cell takes the region of the source nearest its
//!    centre; a thin region wins a cell it touches within half a cube of
//!    the nearest. With `keep_thin`, every connected piece of a thin region
//!    or a `keep` region keeps at least one cell of its own (an ear can't
//!    turn into coat, an eye can't vanish into the face).
//!    Inside cells take their nearest surface cell's region.
//! 4. **One piece**: 26-connected pieces of cells are bridged to the
//!    largest by a line of cells, so a floating hoof or tail tip can't
//!    appear.
//! 5. **Cubes**: one quad per visible cube face (outer faces only), one part
//!    per region; `samples` is the cube's centre, so the surface step
//!    colours each cube one colour and can bevel it (fragment minus sample
//!    is the place inside the cube; its normal part is half a cube).
//!    Faces aren't merged across cubes: a merged face would need a colour
//!    per cube that a shared vertex can't carry (see DECISIONS.md, "Studio
//!    WP3"); [`VoxelGrid`] is what ships (a few KB, [`VoxelGrid::encode`]),
//!    and [`mesh_grid`] rebuilds the cubes on load.

use std::collections::VecDeque;

use evani_studio::assets::AssetRow;
use evani_studio::geo::{GeometryGenerator, SourceMesh, StyledMesh, StyledPart};
use evani_studio::styles::{Geometry, GeometryKind};

use crate::math::{V3, add, bounds, closest_on_triangle, cross, dot, eigen_sym, scale, sub};

/// Cubes across the thinnest tagged part at the style's own cube size.
/// Three, not two: at two the dog (no horns; its ears, 0.12 wide, are its
/// thinnest part) got cubes of 0.061, its eyes sank into the head and its
/// silhouette fell to 0.62. The horse and the cow sit at [`MIN_FIT`] either
/// way.
pub const THIN_CELLS: f32 = 3.0;
/// The voxel style's own cube size (styles.json WK08), against which an
/// asset's fit is measured, so the cube-size control keeps scaling it.
pub const REF_CELL: f32 = 0.12;
/// The smallest fit: cubes at most this much finer than the style's.
pub const MIN_FIT: f32 = 0.3;
/// A thin piece smaller than this many triangles is a crumb, not a part.
const MIN_PIECE_TRIS: usize = 4;
/// The most cells a grid may hold (about 2 MB of work at most).
const MAX_CELLS: usize = 2_000_000;

#[derive(Debug, Clone, Copy, Default)]
pub struct Voxel;

impl GeometryGenerator for Voxel {
    fn kind(&self) -> GeometryKind {
        GeometryKind::Voxel
    }

    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String> {
        let Geometry::Voxel { cell, keep_thin } = *geometry else {
            return Err(format!("voxel can't build `{}`", geometry.kind().name()));
        };
        src.validate(asset.regions.len())?;
        let size = fit_cell(src, asset, cell);
        let grid = voxelise(src, asset, size, keep_thin)?;
        let out = mesh_grid(&grid);
        out.validate(asset.regions.len())?;
        Ok(out)
    }
}

/// A grid of cubes: `cells[x + dims[0] * (y + dims[1] * z)]` is 0 when
/// empty, else the region index plus one. Cell `(x, y, z)` spans
/// `origin + cell * [x, y, z]` to one cell further.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelGrid {
    pub origin: V3,
    pub cell: f32,
    pub dims: [usize; 3],
    pub cells: Vec<u8>,
}

impl VoxelGrid {
    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    /// The cell at signed coordinates (0 outside the grid).
    pub fn at(&self, x: isize, y: isize, z: isize) -> u8 {
        if x < 0 || y < 0 || z < 0 || x as usize >= self.dims[0] || y as usize >= self.dims[1] || z as usize >= self.dims[2] {
            return 0;
        }
        self.cells[self.index(x as usize, y as usize, z as usize)]
    }

    pub fn centre(&self, x: usize, y: usize, z: usize) -> V3 {
        add(self.origin, scale([x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5], self.cell))
    }

    pub fn solid(&self) -> usize {
        self.cells.iter().filter(|&&c| c != 0).count()
    }

    /// The shipped form: magic `EVX1`, origin and cell (f32 LE), dims (u16
    /// LE), then the cells run-length coded in x-fastest order as
    /// (run - 1, value) byte pairs. Only the outer shell matters to the
    /// eye, but inside cells are kept so the grid is the whole solid.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = b"EVX1".to_vec();
        for v in self.origin.iter().chain([&self.cell]) {
            out.extend(v.to_le_bytes());
        }
        for d in self.dims {
            out.extend((d.min(u16::MAX as usize) as u16).to_le_bytes());
        }
        let mut i = 0;
        while i < self.cells.len() {
            let v = self.cells[i];
            let mut run = 1;
            while run < 256 && i + run < self.cells.len() && self.cells[i + run] == v {
                run += 1;
            }
            out.push((run - 1) as u8);
            out.push(v);
            i += run;
        }
        out
    }

    /// Read [`VoxelGrid::encode`]'s bytes back.
    pub fn decode(b: &[u8]) -> Result<Self, String> {
        let bad = || "voxel grid: bad or truncated data".to_string();
        if b.get(0..4) != Some(b"EVX1") {
            return Err(bad());
        }
        let f = |o: usize| -> Result<f32, String> { Ok(f32::from_le_bytes(b.get(o..o + 4).ok_or_else(bad)?.try_into().map_err(|_| bad())?)) };
        let u = |o: usize| -> Result<usize, String> { Ok(usize::from(u16::from_le_bytes(b.get(o..o + 2).ok_or_else(bad)?.try_into().map_err(|_| bad())?))) };
        let origin = [f(4)?, f(8)?, f(12)?];
        let cell = f(16)?;
        let dims = [u(20)?, u(22)?, u(24)?];
        let n = dims[0] * dims[1] * dims[2];
        if n > MAX_CELLS || !(cell.is_finite() && cell > 0.0) {
            return Err(bad());
        }
        let mut cells = Vec::with_capacity(n);
        for pair in b.get(26..).ok_or_else(bad)?.chunks(2) {
            let [run, v] = *pair else { return Err(bad()) };
            cells.extend(std::iter::repeat_n(v, usize::from(run) + 1));
        }
        if cells.len() != n {
            return Err(bad());
        }
        Ok(Self { origin, cell, dims, cells })
    }
}

/// The connected pieces of each thin region's triangles (vertices welded
/// by position): `(region, triangle indices)` per piece, crumbs dropped.
pub fn thin_pieces(src: &SourceMesh, asset: &AssetRow) -> Vec<(u8, Vec<usize>)> {
    let thin: Vec<bool> = asset.regions.iter().map(|r| r.thin).collect();
    region_pieces(src, &|r| thin.get(usize::from(r)).copied().unwrap_or(false))
        .into_iter()
        .filter(|g| g.1.len() >= MIN_PIECE_TRIS)
        .collect()
}

/// The pieces the geometry stage must keep and the check looks for: those
/// of thin regions (legs, ears, mane, tail, horns) and of regions that keep
/// the source's own colour (eyes), crumbs dropped.
pub fn tagged_pieces(src: &SourceMesh, asset: &AssetRow) -> Vec<(u8, Vec<usize>)> {
    let tagged: Vec<bool> = asset.regions.iter().map(|r| r.thin || r.keep).collect();
    region_pieces(src, &|r| tagged.get(usize::from(r)).copied().unwrap_or(false))
        .into_iter()
        .filter(|g| g.1.len() >= MIN_PIECE_TRIS)
        .collect()
}

/// The connected pieces of the triangles of every region `keep` accepts
/// (vertices welded by position; a piece holds one region), in region
/// order, then by first triangle.
pub fn region_pieces(src: &SourceMesh, keep: &dyn Fn(u8) -> bool) -> Vec<(u8, Vec<usize>)> {
    let tris = src.indices.as_chunks::<3>().0;
    let (weld, _) = weld_ids(&src.positions);
    let region: Vec<u8> = tris.iter().map(|t| tri_region(src, *t)).collect();
    // Triangles join through a welded corner they share within one region
    // (sorted corner list); a corner on a region border joins nothing across.
    let mut parent: Vec<u32> = (0..tris.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    let mut corner: Vec<(u32, u8, u32)> = Vec::with_capacity(tris.len() * 3);
    for (t, tri) in tris.iter().enumerate() {
        if keep(region[t]) {
            for &v in tri {
                corner.push((weld[v as usize], region[t], t as u32));
            }
        }
    }
    corner.sort_unstable();
    for w in corner.windows(2) {
        if w[0].0 == w[1].0 && w[0].1 == w[1].1 {
            let (a, b) = (find(&mut parent, w[0].2), find(&mut parent, w[1].2));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
    }
    let mut groups: Vec<(u8, u32, Vec<usize>)> = Vec::new();
    let mut slot: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (t, &reg) in region.iter().enumerate() {
        if !keep(reg) {
            continue;
        }
        let root = find(&mut parent, t as u32);
        let i = *slot.entry(root).or_insert_with(|| {
            groups.push((reg, root, Vec::new()));
            groups.len() - 1
        });
        groups[i].2.push(t);
    }
    groups.sort_by_key(|g| (g.0, g.1));
    groups.into_iter().map(|g| (g.0, g.2)).collect()
}

/// The region of a triangle: the one most of its corners have (the first
/// corner's on a three-way tie).
pub fn tri_region(src: &SourceMesh, t: [u32; 3]) -> u8 {
    let [a, b, c] = t.map(|v| src.regions[v as usize]);
    if b == c { b } else { a }
}

/// Per vertex: a welded id (same position, same id), and the id count.
fn weld_ids(positions: &[V3]) -> (Vec<u32>, usize) {
    let mut keys: Vec<([i32; 3], u32)> = positions.iter().enumerate().map(|(i, p)| (p.map(|x| (x * 1e5).round() as i32), i as u32)).collect();
    keys.sort_unstable();
    let mut ids = vec![0u32; positions.len()];
    let mut next = 0u32;
    for (i, k) in keys.iter().enumerate() {
        if i > 0 && keys[i - 1].0 != k.0 {
            next += 1;
        }
        ids[k.1 as usize] = next;
    }
    (ids, if positions.is_empty() { 0 } else { next as usize + 1 })
}

/// The cross-section width of a piece: its middle principal extent.
pub fn piece_width(src: &SourceMesh, tris: &[usize]) -> f32 {
    let idx = src.indices.as_chunks::<3>().0;
    let pts: Vec<V3> = tris.iter().flat_map(|&t| idx[t].map(|v| src.positions[v as usize])).collect();
    let n = pts.len() as f64;
    let mean = pts.iter().fold([0.0f64; 3], |m, p| [m[0] + f64::from(p[0]) / n, m[1] + f64::from(p[1]) / n, m[2] + f64::from(p[2]) / n]);
    let mut cov = [[0.0f64; 3]; 3];
    for p in &pts {
        let d = [f64::from(p[0]) - mean[0], f64::from(p[1]) - mean[1], f64::from(p[2]) - mean[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j] / n;
            }
        }
    }
    let (_, axes) = eigen_sym(cov);
    let ax = axes[1].map(|x| x as f32);
    let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        let d = dot(*p, ax);
        (lo.min(d), hi.max(d))
    });
    (hi - lo).max(0.0)
}

/// The cube size for `asset` at the style's (control-scaled) `cell`.
pub fn fit_cell(src: &SourceMesh, asset: &AssetRow, cell: f32) -> f32 {
    let thinnest = thin_pieces(src, asset).iter().map(|(_, t)| piece_width(src, t)).fold(f32::MAX, f32::min);
    if thinnest == f32::MAX {
        return cell;
    }
    let fit = (thinnest / (THIN_CELLS * REF_CELL)).clamp(MIN_FIT, 1.0);
    cell * fit
}

/// The separating-axis test of a triangle (relative to a box centre)
/// against a box of half size `h`, inclusive (touching counts).
fn tri_box(v: [V3; 3], h: f32) -> bool {
    let e = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let f = [sub(v[1], v[0]), sub(v[2], v[1]), sub(v[0], v[2])];
    let eps = h * 1e-4;
    for ei in e {
        for fj in f {
            let a = cross(ei, fj);
            let r = h * (a[0].abs() + a[1].abs() + a[2].abs());
            let p = v.map(|q| dot(q, a));
            let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            if lo > r + eps || hi < -r - eps {
                return false;
            }
        }
    }
    let [v0, v1, v2] = v;
    for ((a, b), c) in v0.iter().zip(v1.iter()).zip(v2.iter()) {
        let (lo, hi) = (a.min(*b).min(*c), a.max(*b).max(*c));
        if lo > h + eps || hi < -h - eps {
            return false;
        }
    }
    let n = cross(f[0], f[1]);
    let r = h * (n[0].abs() + n[1].abs() + n[2].abs());
    dot(n, v[0]).abs() <= r + eps * (n[0].abs() + n[1].abs() + n[2].abs())
}

/// Voxelise `src` at cube size `cell` (module docs, steps 2 to 4).
pub fn voxelise(src: &SourceMesh, asset: &AssetRow, cell: f32, keep_thin: bool) -> Result<VoxelGrid, String> {
    if !(cell.is_finite() && cell > 1e-4) {
        return Err(format!("voxel: cube size {cell} is not usable"));
    }
    let (lo, hi) = bounds(&src.positions);
    // A global grid (cells at multiples of `cell`) with two empty cells
    // round it, so the corner cell the inside fill floods from can't be
    // touched even by a vertex on a cell boundary.
    let origin = lo.map(|x| ((x / cell).floor() - 2.0) * cell);
    let dims = [0, 1, 2].map(|k| ((hi[k] - origin[k]) / cell).floor() as usize + 3);
    let n = dims[0] * dims[1] * dims[2];
    if n > MAX_CELLS {
        return Err(format!("voxel: {n} cells at cube size {cell:.4} is too fine (at most {MAX_CELLS})"));
    }
    let mut g = VoxelGrid { origin, cell, dims, cells: vec![0; n] };
    let thin: Vec<bool> = asset.regions.iter().map(|r| r.thin).collect();
    // Per cell: the nearest source distance and region, and the nearest thin.
    let mut best = vec![(f32::MAX, 0u8); n];
    let mut best_thin = vec![(f32::MAX, 0u8); n];
    let tris = src.indices.as_chunks::<3>().0;
    let h = cell * 0.5;
    // Which cells each triangle touched (for the thin keep pass).
    let mut touched: Vec<Vec<u32>> = vec![Vec::new(); tris.len()];
    for (ti, t) in tris.iter().enumerate() {
        let p = t.map(|v| src.positions[v as usize]);
        let (tlo, thi) = bounds(&p);
        let c0 = [0, 1, 2].map(|k| (((tlo[k] - origin[k]) / cell).floor() as isize - 1).max(0) as usize);
        let c1 = [0, 1, 2].map(|k| ((((thi[k] - origin[k]) / cell).floor() as isize + 1).max(0) as usize).min(dims[k] - 1));
        for z in c0[2]..=c1[2] {
            for y in c0[1]..=c1[1] {
                for x in c0[0]..=c1[0] {
                    let c = g.centre(x, y, z);
                    if !tri_box(p.map(|q| sub(q, c)), h) {
                        continue;
                    }
                    let i = g.index(x, y, z);
                    touched[ti].push(i as u32);
                    let q = closest_on_triangle(c, p[0], p[1], p[2]);
                    let d = dot(sub(q, c), sub(q, c));
                    // The corner nearest the closest point names the region.
                    let k = (0..3).fold(0, |b, k| if dot(sub(p[k], q), sub(p[k], q)) < dot(sub(p[b], q), sub(p[b], q)) { k } else { b });
                    let r = src.regions[t[k] as usize];
                    if d < best[i].0 {
                        best[i] = (d, r);
                    }
                    if thin.get(usize::from(r)).copied().unwrap_or(false) && d < best_thin[i].0 {
                        best_thin[i] = (d, r);
                    }
                }
            }
        }
    }
    for i in 0..n {
        if best[i].0 == f32::MAX {
            continue;
        }
        let near = best[i].0.sqrt();
        let r = if best_thin[i].0 != f32::MAX && best_thin[i].0.sqrt() <= near + h { best_thin[i].1 } else { best[i].1 };
        g.cells[i] = r + 1;
    }
    if keep_thin {
        keep_thin_pieces(&mut g, src, asset, &touched);
    }
    fill_inside(&mut g);
    one_piece(&mut g);
    Ok(g)
}

/// Every connected piece of a thin region, and of a region keeping the
/// source's colour (an eye), keeps at least one cell of its own: if none of
/// the cells it touched took its region, the touched cell nearest the
/// piece's centre does.
fn keep_thin_pieces(g: &mut VoxelGrid, src: &SourceMesh, asset: &AssetRow, touched: &[Vec<u32>]) {
    let tris = src.indices.as_chunks::<3>().0;
    for (r, piece) in tagged_pieces(src, asset) {
        let cells: Vec<u32> = piece.iter().flat_map(|&t| touched[t].iter().copied()).collect();
        if cells.iter().any(|&i| g.cells[i as usize] == r + 1) {
            continue;
        }
        let pts: Vec<V3> = piece.iter().flat_map(|&t| tris[t].map(|v| src.positions[v as usize])).collect();
        let c = scale(pts.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / pts.len().max(1) as f32);
        let centre = |i: u32| {
            let i = i as usize;
            let (x, y, z) = (i % g.dims[0], i / g.dims[0] % g.dims[1], i / (g.dims[0] * g.dims[1]));
            g.centre(x, y, z)
        };
        if let Some(&i) = cells.iter().min_by(|&&a, &&b| {
            let (da, db) = (sub(centre(a), c), sub(centre(b), c));
            dot(da, da).total_cmp(&dot(db, db)).then(a.cmp(&b))
        }) {
            g.cells[i as usize] = r + 1;
        }
    }
}

const SIX: [[isize; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

fn coords(g: &VoxelGrid, i: usize) -> [isize; 3] {
    [(i % g.dims[0]) as isize, (i / g.dims[0] % g.dims[1]) as isize, (i / (g.dims[0] * g.dims[1])) as isize]
}

fn neighbour(g: &VoxelGrid, c: [isize; 3], d: [isize; 3]) -> Option<usize> {
    let q = [c[0] + d[0], c[1] + d[1], c[2] + d[2]];
    if q.iter().zip(g.dims).any(|(&v, n)| v < 0 || v as usize >= n) {
        return None;
    }
    Some(g.index(q[0] as usize, q[1] as usize, q[2] as usize))
}

/// Fill the inside: flood the empty cells from the grid's corner (always
/// empty, the grid has a margin); empty cells the flood can't reach are
/// inside and take the region of their nearest surface cell (a
/// breadth-first spread from the shell, in index order).
fn fill_inside(g: &mut VoxelGrid) {
    let n = g.cells.len();
    let mut outside = vec![false; n];
    let mut q = VecDeque::from([0usize]);
    outside[0] = g.cells[0] == 0;
    while let Some(i) = q.pop_front() {
        let c = coords(g, i);
        for d in SIX {
            if let Some(j) = neighbour(g, c, d)
                && !outside[j]
                && g.cells[j] == 0
            {
                outside[j] = true;
                q.push_back(j);
            }
        }
    }
    let mut q: VecDeque<usize> = (0..n).filter(|&i| g.cells[i] != 0).collect();
    while let Some(i) = q.pop_front() {
        let c = coords(g, i);
        let v = g.cells[i];
        for d in SIX {
            if let Some(j) = neighbour(g, c, d)
                && !outside[j]
                && g.cells[j] == 0
            {
                g.cells[j] = v;
                q.push_back(j);
            }
        }
    }
}

/// The 26-connected pieces of solid cells, largest first (ties by first
/// cell).
pub fn pieces(g: &VoxelGrid) -> Vec<Vec<usize>> {
    let n = g.cells.len();
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for s in 0..n {
        if seen[s] || g.cells[s] == 0 {
            continue;
        }
        let mut piece = vec![s];
        seen[s] = true;
        let mut k = 0;
        while k < piece.len() {
            let c = coords(g, piece[k]);
            k += 1;
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if let Some(j) = neighbour(g, c, [dx, dy, dz])
                            && !seen[j]
                            && g.cells[j] != 0
                        {
                            seen[j] = true;
                            piece.push(j);
                        }
                    }
                }
            }
        }
        out.push(piece);
    }
    out.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    out
}

/// Bridge every smaller piece to the largest with a line of cells (in the
/// smaller piece's region), nearest pair first.
fn one_piece(g: &mut VoxelGrid) {
    for _ in 0..64 {
        let ps = pieces(g);
        if ps.len() <= 1 {
            return;
        }
        let main = &ps[0];
        let small = &ps[ps.len() - 1];
        let mut best = (isize::MAX, 0usize, 0usize);
        for &a in small {
            let ca = coords(g, a);
            for &b in main {
                let cb = coords(g, b);
                let d = (0..3).map(|k| (ca[k] - cb[k]).pow(2)).sum::<isize>();
                if d < best.0 {
                    best = (d, a, b);
                }
            }
        }
        let (ca, cb) = (coords(g, best.1), coords(g, best.2));
        let v = g.cells[best.1];
        let steps = (0..3).map(|k| (ca[k] - cb[k]).abs()).max().unwrap_or(0);
        for s in 0..=steps {
            let t = s as f32 / steps.max(1) as f32;
            let p = [0, 1, 2].map(|k| (ca[k] as f32 + (cb[k] - ca[k]) as f32 * t).round() as isize);
            let i = g.index(p[0] as usize, p[1] as usize, p[2] as usize);
            if g.cells[i] == 0 {
                g.cells[i] = v;
            }
        }
    }
}

/// The cubes of `g`: one quad per visible face, a part per region, in the
/// grid's index order (deterministic).
pub fn mesh_grid(g: &VoxelGrid) -> StyledMesh {
    let mut parts: Vec<StyledPart> = Vec::new();
    let mut slot = [usize::MAX; 256];
    for z in 0..g.dims[2] {
        for y in 0..g.dims[1] {
            for x in 0..g.dims[0] {
                let v = g.cells[g.index(x, y, z)];
                if v == 0 {
                    continue;
                }
                let c = [x as isize, y as isize, z as isize];
                for axis in 0..3 {
                    for sign in [1isize, -1] {
                        let mut d = [0isize; 3];
                        d[axis] = sign;
                        if g.at(c[0] + d[0], c[1] + d[1], c[2] + d[2]) != 0 {
                            continue;
                        }
                        let si = &mut slot[usize::from(v)];
                        if *si == usize::MAX {
                            *si = parts.len();
                            parts.push(StyledPart { region: v - 1, ..StyledPart::default() });
                        }
                        face(&mut parts[*si], g, [x, y, z], axis, sign);
                    }
                }
            }
        }
    }
    StyledMesh { parts }
}

/// One cube face, wound to face out.
fn face(p: &mut StyledPart, g: &VoxelGrid, c: [usize; 3], axis: usize, sign: isize) {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let lo = add(g.origin, scale(c.map(|k| k as f32), g.cell));
    let mut base = lo;
    if sign > 0 {
        base[axis] += g.cell;
    }
    let corner = |a: f32, b: f32| {
        let mut q = base;
        q[u] += a * g.cell;
        q[v] += b * g.cell;
        q
    };
    let mut n = [0.0; 3];
    n[axis] = sign as f32;
    let centre = g.centre(c[0], c[1], c[2]);
    let first = p.positions.len() as u32;
    for (a, b) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        p.positions.push(corner(a, b));
        p.normals.push(n);
        p.samples.push(centre);
    }
    if sign > 0 {
        p.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    } else {
        p.indices.extend([first, first + 2, first + 1, first, first + 3, first + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_box_is_inclusive_and_separates() {
        let t = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]];
        assert!(tri_box(t, 0.5));
        // Far away along each kind of axis.
        assert!(!tri_box(t.map(|p| add(p, [0.0, 0.0, 0.6])), 0.5));
        assert!(!tri_box(t.map(|p| add(p, [2.0, 0.0, 0.0])), 0.5));
        // A sliver crossing a corner region diagonally but missing the box.
        let sliver = [[1.2, 0.0, 0.0], [0.0, 1.2, 0.0], [1.2, 1.2, 0.0]];
        assert!(!tri_box(sliver, 0.5));
        // Touching a face counts.
        assert!(tri_box(t.map(|p| add(p, [0.0, 0.0, 0.5])), 0.5));
    }

    #[test]
    fn a_grid_round_trips_through_its_bytes() {
        let g = VoxelGrid { origin: [-1.0, -0.5, 0.25], cell: 0.1, dims: [3, 2, 2], cells: vec![0, 1, 1, 1, 2, 0, 0, 0, 0, 3, 3, 3] };
        let b = g.encode();
        assert_eq!(VoxelGrid::decode(&b).unwrap(), g);
        assert!(VoxelGrid::decode(&b[..b.len() - 1]).is_err());
    }
}
