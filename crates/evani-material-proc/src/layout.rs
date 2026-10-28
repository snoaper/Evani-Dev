//! The `kit_data` layout the shader reads, and a CPU mirror of its lookup.
//!
//! Records are `vec4<f32>`; integers are stored as exact floats.
//!
//! | rec | contents |
//! |-----|----------|
//! | 0 | `[grid x0, grid z0, cell size, nx]` |
//! | 1 | `[nz, box_base, list_base, cell_base]` |
//! | 2 | `[nboxes, region_base, nregions, ao_rec_base]` |
//! | 3-6 | the sky dome (feature `sky`: `SkyParams::records`; zero until the draw system fills them) |
//! | 7 | `[tune base, tune count, AO strength, AO warmth]` ([`crate::params::Tuning::header`]) |
//! | tune | 2 per surface id from the tune base: `[colour multiplier rgb, wear]`, `[grime, streaks, 0, 0]` |
//! | regions | 2 each: `[x0, z0, x1, z1]`, `[wall, floor, variant, 0]` (the last is a catch-all) |
//! | boxes | 9 each: `[centre, flags]`, `[quat]`, `[half, seed]`, 6 faces `[ao index or -1, nu, nv, surface + 256 * edges]` |
//! | AO | 4 texels per record; a face's `ao index` is the absolute float index |
//! | cells | 2 per record: `[start, count]` into the list |
//! | list | 4 box ids per record |
//!
//! Box flags: bit 0 world-pattern, bit 1 decal, then `256 * variant`.

use crate::bake::Bake;
use crate::params::Tuning;
use crate::scene::{ProcBox, Scene, V3, rotate_inv};

/// The lookup grid's cell size (m).
pub const CELL: f32 = 2.0;
/// How far each box is grown when it is entered into the lookup grid (m).
pub const LIST_GROW: f32 = 0.04;
/// How far from a face plane (and past its edges) a point still matches it (m).
pub const FACE_EPS: f32 = 0.03;
/// The smallest dominant normal component for a face match.
pub const NORMAL_MIN: f32 = 0.7;
/// Records in the header.
pub const HEADER: usize = 8;
/// The header record with the AO tuning and where the tune records start.
pub const TUNE_HEADER: usize = 7;
/// The first tune record (right after the header).
pub const TUNE_BASE: usize = HEADER;
/// Records per box.
pub const BOX_RECS: usize = 9;

/// A uniform 2D grid over the boxes' floor-plan bounds.
pub(crate) struct Grid {
    pub x0: f32,
    pub z0: f32,
    pub cell: f32,
    pub nx: usize,
    pub nz: usize,
    /// Box ids per cell (row by row: z outer, x inner).
    pub cells: Vec<Vec<u32>>,
}

impl Grid {
    /// The boxes `filter` keeps, each grown by `grow` on every side.
    pub fn new(boxes: &[ProcBox], filter: impl Fn(&ProcBox) -> bool, grow: f32) -> Self {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        let mut kept = Vec::new();
        for (i, b) in boxes.iter().enumerate() {
            if !filter(b) {
                continue;
            }
            let (mn, mx) = b.bounds();
            let r = [mn[0] - grow, mn[2] - grow, mx[0] + grow, mx[2] + grow];
            lo[0] = lo[0].min(r[0]);
            lo[1] = lo[1].min(r[1]);
            hi[0] = hi[0].max(r[2]);
            hi[1] = hi[1].max(r[3]);
            kept.push((i as u32, r));
        }
        if kept.is_empty() {
            return Grid {
                x0: 0.0,
                z0: 0.0,
                cell: CELL,
                nx: 1,
                nz: 1,
                cells: vec![Vec::new()],
            };
        }
        let x0 = (lo[0] / CELL).floor() * CELL;
        let z0 = (lo[1] / CELL).floor() * CELL;
        let nx = (((hi[0] - x0) / CELL).ceil() as usize).max(1);
        let nz = (((hi[1] - z0) / CELL).ceil() as usize).max(1);
        let mut g = Grid {
            x0,
            z0,
            cell: CELL,
            nx,
            nz,
            cells: vec![Vec::new(); nx * nz],
        };
        for (i, r) in kept {
            if let Some((ix0, iz0, ix1, iz1)) = g.range(r[0], r[1], r[2], r[3]) {
                for iz in iz0..=iz1 {
                    for ix in ix0..=ix1 {
                        g.cells[iz * nx + ix].push(i);
                    }
                }
            }
        }
        g
    }

    /// The cell holding `(x, z)`.
    pub fn cell_of(&self, x: f32, z: f32) -> Option<usize> {
        let fx = ((x - self.x0) / self.cell).floor();
        let fz = ((z - self.z0) / self.cell).floor();
        if fx < 0.0 || fz < 0.0 || fx >= self.nx as f32 || fz >= self.nz as f32 {
            return None;
        }
        Some(fz as usize * self.nx + fx as usize)
    }

    /// The cells a rectangle covers `(ix0, iz0, ix1, iz1)`, inclusive and
    /// clamped (`None` when it misses the grid).
    pub fn range(
        &self,
        minx: f32,
        minz: f32,
        maxx: f32,
        maxz: f32,
    ) -> Option<(usize, usize, usize, usize)> {
        let to = |v: f32, o: f32| ((v - o) / self.cell).floor();
        let (ax0, az0, ax1, az1) = (
            to(minx, self.x0),
            to(minz, self.z0),
            to(maxx, self.x0),
            to(maxz, self.z0),
        );
        if ax1 < 0.0 || az1 < 0.0 || ax0 >= self.nx as f32 || az0 >= self.nz as f32 {
            return None;
        }
        let c = |v: f32, n: usize| v.clamp(0.0, (n - 1) as f32) as usize;
        Some((
            c(ax0, self.nx),
            c(az0, self.nz),
            c(ax1, self.nx),
            c(az1, self.nz),
        ))
    }
}

fn f(v: usize) -> f32 {
    v as f32
}

/// The region used where the scene names none.
pub fn fallback_region() -> crate::scene::Region {
    crate::scene::Region {
        rect: [-1e6, -1e6, 1e6, 1e6],
        wall: crate::scene::surface::PLASTER,
        floor: crate::scene::surface::GROUND,
        variant: 0,
    }
}

/// The tune records' range in the packed data.
pub fn tune_range() -> std::ops::Range<usize> {
    TUNE_BASE..TUNE_BASE + 2 * crate::scene::surface::COUNT as usize
}

/// Write `tuning` into packed data (its header record and tune records).
pub fn write_tuning(data: &mut [[f32; 4]], tuning: &Tuning) {
    let r = tune_range();
    if data.len() < r.end {
        return;
    }
    data[TUNE_HEADER] = tuning.header(TUNE_BASE);
    data[r].copy_from_slice(&tuning.records());
}

/// Pack the scene and its bake into `kit_data` records, with the authored
/// tuning ([`Tuning::default`]).
pub fn pack(scene: &Scene, bake: &Bake) -> Result<Vec<[f32; 4]>, String> {
    pack_tuned(scene, bake, &Tuning::default())
}

/// Pack the scene and its bake into `kit_data` records.
pub fn pack_tuned(scene: &Scene, bake: &Bake, tuning: &Tuning) -> Result<Vec<[f32; 4]>, String> {
    if bake.faces.len() != scene.boxes.len() {
        return Err(format!(
            "bake has {} boxes, scene {}",
            bake.faces.len(),
            scene.boxes.len()
        ));
    }
    let grid = Grid::new(&scene.boxes, |_| true, LIST_GROW);
    let mut regions = scene.regions.clone();
    let mut catch_all = scene.default_region.unwrap_or_else(fallback_region);
    catch_all.rect = [-1e6, -1e6, 1e6, 1e6];
    regions.push(catch_all);

    let region_base = tune_range().end;
    let box_base = region_base + 2 * regions.len();
    let ao_rec_base = box_base + BOX_RECS * scene.boxes.len();
    let ao_recs = bake.texels.len().div_ceil(4);
    let cell_base = ao_rec_base + ao_recs;
    let ncells = grid.nx * grid.nz;
    let cell_recs = ncells.div_ceil(2);
    let list_base = cell_base + cell_recs;
    let list_len: usize = grid.cells.iter().map(Vec::len).sum();
    let total = list_base + list_len.div_ceil(4);
    if total > evani_render::kit_material::MAX_DATA {
        return Err(format!(
            "{total} records is more than the {} kit_data allows",
            evani_render::kit_material::MAX_DATA
        ));
    }
    // Floats hold integers exactly only up to 2^24.
    if ao_rec_base * 4 + bake.texels.len() >= 1 << 24 {
        return Err("too many AO texels".into());
    }

    let mut d = vec![[0.0f32; 4]; total];
    d[0] = [grid.x0, grid.z0, grid.cell, f(grid.nx)];
    d[1] = [f(grid.nz), f(box_base), f(list_base), f(cell_base)];
    d[2] = [
        f(scene.boxes.len()),
        f(region_base),
        f(regions.len()),
        f(ao_rec_base),
    ];
    write_tuning(&mut d, tuning);
    for (i, r) in regions.iter().enumerate() {
        d[region_base + 2 * i] = r.rect;
        d[region_base + 2 * i + 1] = [r.wall as f32, r.floor as f32, (r.variant & 255) as f32, 0.0];
    }
    for (i, (b, fb)) in scene.boxes.iter().zip(&bake.faces).enumerate() {
        let o = box_base + BOX_RECS * i;
        let flags =
            u32::from(b.world_pattern) | (u32::from(b.decal) << 1) | ((b.variant & 255) << 8);
        d[o] = [b.centre[0], b.centre[1], b.centre[2], flags as f32];
        d[o + 1] = b.rotation;
        d[o + 2] = [b.half[0], b.half[1], b.half[2], (b.seed & 0xffff) as f32];
        for (k, face) in fb.iter().enumerate() {
            let ao = face.start.map_or(-1.0, |s| f(ao_rec_base * 4 + s as usize));
            let surf = (b.surfaces[k] & 255) | ((face.edges & 15) << 8);
            d[o + 3 + k] = [ao, face.nu as f32, face.nv as f32, surf as f32];
        }
    }
    for (i, t) in bake.texels.iter().enumerate() {
        d[ao_rec_base + i / 4][i % 4] = *t;
    }
    let mut start = 0usize;
    let mut list = Vec::with_capacity(list_len);
    for (ci, c) in grid.cells.iter().enumerate() {
        d[cell_base + ci / 2][(ci % 2) * 2] = f(start);
        d[cell_base + ci / 2][(ci % 2) * 2 + 1] = f(c.len());
        start += c.len();
        list.extend_from_slice(c);
    }
    for (i, b) in list.iter().enumerate() {
        d[list_base + i / 4][i % 4] = *b as f32;
    }
    Ok(d)
}

fn u(v: f32) -> usize {
    v.max(0.0) as usize
}

/// The box and face a point with normal `n` paints as, exactly as the
/// shader finds it: only boxes in the point's cell, decal boxes only when
/// `decal` (and only them then), the dominant local normal axis (> 0.7)
/// with the same sign, within [`FACE_EPS`] of the plane and of the face's
/// edges, nearest plane wins (first on a tie).
pub fn find(data: &[[f32; 4]], p: V3, n: V3, decal: bool) -> Option<(usize, usize)> {
    if data.len() < HEADER {
        return None;
    }
    let (g, h1) = (data[0], data[1]);
    let (nx, nz) = (u(g[3]), u(h1[0]));
    let (box_base, list_base, cell_base) = (u(h1[1]), u(h1[2]), u(h1[3]));
    let fx = ((p[0] - g[0]) / g[2]).floor();
    let fz = ((p[2] - g[1]) / g[2]).floor();
    if fx < 0.0 || fz < 0.0 || fx >= nx as f32 || fz >= nz as f32 {
        return None;
    }
    let ci = fz as usize * nx + fx as usize;
    let cr = data[cell_base + ci / 2];
    let (start, count) = (u(cr[(ci % 2) * 2]), u(cr[(ci % 2) * 2 + 1]));
    let mut best: Option<(usize, usize)> = None;
    let mut best_d = FACE_EPS;
    for k in start..start + count {
        let bi = u(data[list_base + k / 4][k % 4]);
        let o = box_base + BOX_RECS * bi;
        let flags = u(data[o][3]);
        if ((flags & 2) != 0) != decal {
            continue;
        }
        let c = [data[o][0], data[o][1], data[o][2]];
        let q = data[o + 1];
        let half = [data[o + 2][0], data[o + 2][1], data[o + 2][2]];
        let l = rotate_inv(q, [p[0] - c[0], p[1] - c[1], p[2] - c[2]]);
        let ln = rotate_inv(q, n);
        let a = if ln[0].abs() >= ln[1].abs() && ln[0].abs() >= ln[2].abs() {
            0
        } else if ln[1].abs() >= ln[2].abs() {
            1
        } else {
            2
        };
        if ln[a].abs() <= NORMAL_MIN {
            continue;
        }
        let s = if ln[a] > 0.0 { 1.0 } else { -1.0 };
        let dist = (l[a] - s * half[a]).abs();
        if dist >= best_d {
            continue;
        }
        let (ua, va) = crate::scene::uv_axes(a);
        if l[ua].abs() > half[ua] + FACE_EPS || l[va].abs() > half[va] + FACE_EPS {
            continue;
        }
        best_d = dist;
        best = Some((bi, a * 2 + usize::from(s > 0.0)));
    }
    best
}
