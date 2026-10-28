//! [`Structure`]: one destructible structure. A floor grid (0.5 m samples of
//! nothing, concrete or metal) and cover cells (1 m cubes on an integer
//! grid) over one [`HoleField`]. Its support rules are Hexagon's
//! "concrete never floats":
//!
//! * A cell's mass is the share of its 27 sample points (a 3 x 3 x 3 lattice
//!   at 1/6, 1/2, 5/6) not inside a hole. Cells with at least
//!   `cell_min_solid` carry load and join 6-connected groups; cells with a
//!   little left go with the first group that reaches them; isolated scraps
//!   form groups of their own.
//! * A group stands while the held share of its footprint (4 x 4 points
//!   under each load-carrying bottom cell, held where the foot is not holed
//!   and something solid is under it) is at least `support_min` and its
//!   centre of mass lies within `tip_margin` of the convex hull of the held
//!   points. Otherwise it tips over the nearest hull edge (or goes the way
//!   its mass leans), turning under its own weight, slides off at
//!   `tip_release`, falls and lands when a point under it meets something
//!   solid; a landing blasts an impact crater, which can fail more.
//! * Floor cut off from all metal (through joins at least
//!   `floor_sliver_min` wide) drops as a slab with any cover resting on it;
//!   crumbs up to `floor_crumb_max` vanish.
//!
//! Everything steps at the fixed tick; randomness is the caller's `Pcg32`.

use evani_core::rng::Pcg32;
use evani_render::math::quat_axis_angle;

use crate::holes::HoleField;
use crate::support::{hull, outside};
use crate::tuning::Tuning;

/// The 3 sample fractions per axis inside a cell.
const MSMP: [f32; 3] = [1.0 / 6.0, 0.5, 5.0 / 6.0];
/// Every sample solid.
pub const FULL_CELL: u32 = (1 << 27) - 1;
/// Support samples per side under a cell's bottom face.
const SG: usize = 4;
/// Points under a block's bottom faces (fractions of its square).
const BSMP: [[f32; 2]; 5] = [
    [0.5, 0.5],
    [0.15, 0.15],
    [0.85, 0.15],
    [0.15, 0.85],
    [0.85, 0.85],
];
/// How far under a bottom its support is tested (and where a block touches down).
const BELOW: f32 = 0.05;
/// The 6 grid neighbours (du, dv, dy).
const N6: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// One floor sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorKind {
    Empty,
    /// Breakable; must be connected to metal to hold.
    Concrete,
    /// Never breaks; anchors concrete.
    Metal,
}

/// A floor sampled on a square grid. Sample `(a, b)` is index `a * nz + b`,
/// centred at `origin.x + (a + 0.5) * spacing`, `origin.z + (b + 0.5) * spacing`.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorGrid {
    pub nx: usize,
    pub nz: usize,
    pub spacing: f32,
    pub kinds: Vec<FloorKind>,
}

impl FloorGrid {
    /// An `nx x nz` grid of `kind`.
    pub fn filled(nx: usize, nz: usize, spacing: f32, kind: FloorKind) -> Self {
        Self {
            nx,
            nz,
            spacing,
            kinds: vec![kind; nx * nz],
        }
    }

    /// Set sample `(a, b)` (ignored outside the grid).
    pub fn set(&mut self, a: usize, b: usize, kind: FloorKind) {
        if a < self.nx && b < self.nz {
            self.kinds[a * self.nz + b] = kind;
        }
    }
}

/// What to build.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureDesc {
    /// The floor's top surface is at `origin.y`; the floor grid and cell
    /// `(0, 0, 0)` start at `origin.x`, `origin.z`. Cell `(u, v, y)` fills
    /// `origin + (u, y, v)` to `+1` on each axis.
    pub origin: [f32; 3],
    /// Floor slab thickness (Hexagon 0.5).
    pub slab: f32,
    pub floor: Option<FloorGrid>,
    /// Cover cells `(u, v, y)`, `y >= 0`.
    pub cells: Vec<[i32; 3]>,
    /// The world's ground: everything below this height is solid.
    pub ground_y: Option<f32>,
    /// World `(x, z)` a slab that broke off all round tips toward as it drops.
    pub hub: [f32; 2],
}

/// What a step did, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DestructEvent {
    /// A block let go (`tipping`: it turns on a ledge first).
    Fall {
        id: u32,
        cells: u32,
        samples: u32,
        tipping: bool,
    },
    /// A block landed at `pos` and broke up; an impact crater of radius `r` was blasted.
    Land {
        id: u32,
        pos: [f32; 3],
        blocks: f32,
        r: f32,
    },
    /// A block fell out of the world.
    Out { id: u32 },
    /// A block let go while `fall_max` were falling: it crumbled where it stood.
    Crumble { pos: [f32; 3], blocks: f32 },
    /// A floor crumb was cut away.
    Crumbs { pos: [f32; 3], samples: u32 },
}

/// A block on its way down: cover cells, a slab of floor, or both.
#[derive(Debug, Clone, PartialEq)]
pub struct Fall {
    pub id: u32,
    /// Cells (indices) and their solid-sample masks when it let go.
    cells: Vec<(usize, u32)>,
    /// Floor samples (indices) and their 2 x 2 x 2 masks when it let go.
    samples: Vec<(usize, u8)>,
    foot: Vec<[f32; 3]>,
    /// Drop so far.
    y: f32,
    /// Vertical speed.
    v: f32,
    /// Pivot.
    p: [f32; 3],
    /// Unit tip direction (x, z).
    d: [f32; 2],
    /// Tip angle and angular speed.
    t: f32,
    o: f32,
    /// Centre of mass from the pivot along d, and above it.
    ms: f32,
    mh: f32,
    /// Moment of inertia per mass about the pivot.
    mi: f32,
    /// Slide along d and its speed.
    s: f32,
    h: f32,
    /// Still turning on its ledge.
    pivoting: bool,
    /// The ledge along d: nothing at or behind it on its own floor stops it.
    lg: f32,
}

impl Fall {
    /// Where point `q` (as it was before it moved) is now.
    pub fn transform(&self, q: [f32; 3]) -> [f32; 3] {
        let (ct, st) = (self.t.cos(), self.t.sin());
        let sd = (q[0] - self.p[0]) * self.d[0] + (q[2] - self.p[2]) * self.d[1];
        let hh = q[1] - self.p[1];
        let mv = sd * ct + hh * st - sd + self.s;
        [
            q[0] + self.d[0] * mv,
            self.p[1] - sd * st + hh * ct + self.y,
            q[2] + self.d[1] * mv,
        ]
    }

    /// Its turn as a quaternion (up tips toward d).
    pub fn rotation(&self) -> [f32; 4] {
        quat_axis_angle([self.d[1], 0.0, -self.d[0]], self.t)
    }

    /// Cells it carries: `(cell index, mask)`.
    pub fn cells(&self) -> &[(usize, u32)] {
        &self.cells
    }

    /// Floor samples it carries: `(sample index, mask)`.
    pub fn samples(&self) -> &[(usize, u8)] {
        &self.samples
    }

    /// Tip angle (radians).
    pub fn tilt(&self) -> f32 {
        self.t
    }

    /// Drop so far (m, <= 0).
    pub fn fallen(&self) -> f32 {
        self.y
    }

    /// Cells plus slab blocks (2 x 2 samples each).
    pub fn blocks(&self) -> f32 {
        self.cells.len() as f32 + self.samples.len() as f32 / 4.0
    }
}

/// One destructible structure.
#[derive(Debug, Clone)]
pub struct Structure {
    desc: StructureDesc,
    holes: HoleField,
    gone: Vec<bool>,
    mask: Vec<u32>,
    floor_gone: Vec<bool>,
    floor_mask: Vec<u8>,
    masks_at: Option<u64>,
    /// Cell lookup: bounds and index grid.
    lo: [i32; 3],
    dims: [usize; 3],
    grid: Vec<i32>,
    falls: Vec<Fall>,
    next_id: u32,
    checked: Option<u64>,
}

impl Structure {
    pub fn new(desc: StructureDesc, t: &Tuning) -> Self {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for c in &desc.cells {
            for k in 0..3 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
        if desc.cells.is_empty() {
            lo = [0; 3];
            hi = [-1; 3];
        }
        let dims = [
            (hi[0] - lo[0] + 1).max(0) as usize,
            (hi[1] - lo[1] + 1).max(0) as usize,
            (hi[2] - lo[2] + 1).max(0) as usize,
        ];
        let mut grid = vec![-1; dims[0] * dims[1] * dims[2]];
        for (i, c) in desc.cells.iter().enumerate() {
            let k = ((c[0] - lo[0]) as usize * dims[1] + (c[1] - lo[1]) as usize) * dims[2]
                + (c[2] - lo[2]) as usize;
            if let Some(g) = grid.get_mut(k) {
                *g = i as i32;
            }
        }
        // The bucket grid covers the floor and the cells, 2 m buckets, centred on them.
        let (mut x0, mut x1, mut z0, mut z1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        let o = desc.origin;
        if let Some(f) = &desc.floor {
            (x0, x1, z0, z1) = (
                o[0],
                o[0] + f.nx as f32 * f.spacing,
                o[2],
                o[2] + f.nz as f32 * f.spacing,
            );
        }
        if !desc.cells.is_empty() {
            x0 = x0.min(o[0] + lo[0] as f32);
            x1 = x1.max(o[0] + hi[0] as f32 + 1.0);
            z0 = z0.min(o[2] + lo[1] as f32);
            z1 = z1.max(o[2] + hi[1] as f32 + 1.0);
        }
        if x0 > x1 {
            (x0, x1, z0, z1) = (o[0], o[0], o[2], o[2]);
        }
        let span = (x1 - x0).max(z1 - z0) + 8.0;
        let cells = ((span / 2.0).ceil() as usize).clamp(1, 64);
        let holes = HoleField::new(
            [(x0 + x1) * 0.5, (z0 + z1) * 0.5],
            cells,
            2.0,
            t.hole_cap,
            t.hole_max,
            t.hole_rmax,
        );
        let n = desc.cells.len();
        let nf = desc.floor.as_ref().map_or(0, |f| f.kinds.len());
        Self {
            desc,
            holes,
            gone: vec![false; n],
            mask: vec![FULL_CELL; n],
            floor_gone: vec![false; nf],
            floor_mask: vec![0xFF; nf],
            masks_at: None,
            lo,
            dims,
            grid,
            falls: Vec::new(),
            next_id: 0,
            checked: None,
        }
    }

    /// Back to the undamaged start.
    pub fn reset(&mut self) {
        self.holes.clear();
        self.gone.fill(false);
        self.mask.fill(FULL_CELL);
        self.floor_gone.fill(false);
        self.floor_mask.fill(0xFF);
        self.masks_at = None;
        self.falls.clear();
        self.next_id = 0;
        self.checked = None;
    }

    pub fn desc(&self) -> &StructureDesc {
        &self.desc
    }

    pub fn holes(&self) -> &HoleField {
        &self.holes
    }

    /// Falling blocks.
    pub fn falls(&self) -> &[Fall] {
        &self.falls
    }

    /// Cells still standing.
    pub fn standing(&self) -> usize {
        self.gone.iter().filter(|g| !**g).count()
    }

    /// Floor samples still in place (concrete and metal).
    pub fn floor_left(&self) -> usize {
        let Some(f) = &self.desc.floor else { return 0 };
        (0..f.kinds.len())
            .filter(|&i| f.kinds[i] != FloorKind::Empty && !self.floor_gone[i])
            .count()
    }

    fn fy(&self) -> f32 {
        self.desc.origin[1]
    }

    /// Cell `c`'s min corner (world).
    pub fn cell_corner(&self, c: usize) -> [f32; 3] {
        let o = self.desc.origin;
        let g = self.desc.cells[c];
        [o[0] + g[0] as f32, o[1] + g[2] as f32, o[2] + g[1] as f32]
    }

    fn cell_at(&self, u: i32, v: i32, y: i32) -> Option<usize> {
        let (a, b, c) = (u - self.lo[0], v - self.lo[1], y - self.lo[2]);
        if a < 0
            || b < 0
            || c < 0
            || a as usize >= self.dims[0]
            || b as usize >= self.dims[1]
            || c as usize >= self.dims[2]
        {
            return None;
        }
        let k = (a as usize * self.dims[1] + b as usize) * self.dims[2] + c as usize;
        self.grid.get(k).and_then(|&i| usize::try_from(i).ok())
    }

    /// Floor sample centre (world x, z).
    pub fn sample_centre(&self, i: usize) -> [f32; 2] {
        let Some(f) = &self.desc.floor else {
            return [0.0; 2];
        };
        let (a, b) = (i / f.nz.max(1), i % f.nz.max(1));
        [
            self.desc.origin[0] + (a as f32 + 0.5) * f.spacing,
            self.desc.origin[2] + (b as f32 + 0.5) * f.spacing,
        ]
    }

    fn sample_at(&self, x: f32, z: f32) -> Option<usize> {
        let f = self.desc.floor.as_ref()?;
        let a = ((x - self.desc.origin[0]) / f.spacing).floor();
        let b = ((z - self.desc.origin[2]) / f.spacing).floor();
        if !(a >= 0.0 && b >= 0.0) || a as usize >= f.nx || b as usize >= f.nz {
            return None;
        }
        Some(a as usize * f.nz + b as usize)
    }

    fn kind(&self, i: usize) -> FloorKind {
        self.desc
            .floor
            .as_ref()
            .and_then(|f| f.kinds.get(i).copied())
            .unwrap_or(FloorKind::Empty)
    }

    /// Does floor sample `i` still carry (metal, or concrete in place whose middle is not holed)?
    fn sample_solid(&self, i: usize) -> bool {
        match self.kind(i) {
            FloorKind::Metal => true,
            FloorKind::Concrete => {
                let c = self.sample_centre(i);
                !self.floor_gone[i]
                    && !self
                        .holes
                        .holed([c[0], self.fy() - self.desc.slab * 0.5, c[1]])
            }
            FloorKind::Empty => false,
        }
    }

    /// Is there floor at point `p` (inside the slab band)?
    fn floor_at(&self, p: [f32; 3]) -> bool {
        let Some(i) = self.sample_at(p[0], p[2]) else {
            return false;
        };
        match self.kind(i) {
            FloorKind::Metal => true,
            FloorKind::Concrete => !self.floor_gone[i] && !self.holes.holed(p),
            FloorKind::Empty => false,
        }
    }

    /// The collision query: the ground, the floor and standing cells, minus holes.
    pub fn solid(&self, p: [f32; 3]) -> bool {
        if self.desc.ground_y.is_some_and(|g| p[1] < g) {
            return true;
        }
        let fy = self.fy();
        if p[1] < fy {
            return p[1] >= fy - self.desc.slab && self.floor_at(p);
        }
        let o = self.desc.origin;
        let (u, v, y) = (
            (p[0] - o[0]).floor(),
            (p[2] - o[2]).floor(),
            (p[1] - o[1]).floor(),
        );
        if !(u.abs() < 1e6 && v.abs() < 1e6 && y.abs() < 1e6) {
            return false;
        }
        match self.cell_at(u as i32, v as i32, y as i32) {
            Some(c) => !self.gone[c] && !self.holes.holed(p),
            None => false,
        }
    }

    fn cell_mask(&self, c: usize) -> u32 {
        let o = self.cell_corner(c);
        let mut m = 0u32;
        for (iz, fz) in MSMP.iter().enumerate() {
            for (iy, fyy) in MSMP.iter().enumerate() {
                for (ix, fx) in MSMP.iter().enumerate() {
                    if !self.holes.holed([o[0] + fx, o[1] + fyy, o[2] + fz]) {
                        m |= 1 << (ix + 3 * iy + 9 * iz);
                    }
                }
            }
        }
        m
    }

    fn tile_mask(&self, i: usize) -> u8 {
        let Some(f) = &self.desc.floor else { return 0 };
        if self.kind(i) != FloorKind::Concrete {
            return 0xFF;
        }
        let c = self.sample_centre(i);
        let (q, sl, fy) = (f.spacing * 0.25, self.desc.slab, self.fy());
        let mut m = 0u8;
        for (iz, dz) in [-q, q].iter().enumerate() {
            for (iy, dy) in [-0.25 * sl, -0.75 * sl].iter().enumerate() {
                for (ix, dx) in [-q, q].iter().enumerate() {
                    if !self.holes.holed([c[0] + dx, fy + dy, c[1] + dz]) {
                        m |= 1 << (ix + 2 * iy + 4 * iz);
                    }
                }
            }
        }
        m
    }

    /// Recompute the cached sample masks if the holes changed.
    fn refresh_masks(&mut self) {
        let v = self.holes.version();
        if self.masks_at == Some(v) {
            return;
        }
        for c in 0..self.mask.len() {
            if !self.gone[c] {
                self.mask[c] = self.cell_mask(c);
            }
        }
        for i in 0..self.floor_mask.len() {
            if !self.floor_gone[i] {
                self.floor_mask[i] = self.tile_mask(i);
            }
        }
        self.masks_at = Some(v);
    }

    /// Standing cell `c`'s solid-sample mask (bit `ix + 3 iy + 9 iz`), or `None` if it fell.
    pub fn cell_state(&self, c: usize) -> Option<u32> {
        if self.gone.get(c).copied().unwrap_or(true) {
            None
        } else {
            self.mask.get(c).copied()
        }
    }

    /// Floor sample `i`'s kind and 2 x 2 x 2 mask (bit `ix + 2 iy + 4 iz`, iy 0 = upper half), or `None` if gone.
    pub fn tile_state(&self, i: usize) -> Option<(FloorKind, u8)> {
        let k = self.kind(i);
        if k == FloorKind::Empty || self.floor_gone.get(i).copied().unwrap_or(true) {
            return None;
        }
        self.floor_mask.get(i).map(|m| (k, *m))
    }

    /// Cell `c`'s concrete left, 0..1.
    pub fn cell_mass(&self, c: usize) -> f32 {
        self.cell_mask(c).count_ones() as f32 / 27.0
    }

    /// A bullet hole of radius `r` at `p`. Returns whether it added damage.
    pub fn shoot(&mut self, p: [f32; 3], r: f32, seed: u8) -> bool {
        self.holes.add_hole(p, r, seed)
    }

    /// A blast crater: a hole when the sphere reaches standing concrete.
    pub fn blast(&mut self, p: [f32; 3], r: f32, seed: u8) -> bool {
        if !self.touches(p, r) {
            return false;
        }
        self.holes.add_hole(p, r, seed)
    }

    /// Does the sphere reach concrete floor or a standing cell?
    pub fn touches(&self, p: [f32; 3], r: f32) -> bool {
        let fy = self.fy();
        if let Some(f) = &self.desc.floor
            && p[1] - r < fy
            && p[1] + r > fy - self.desc.slab
        {
            for i in 0..f.kinds.len() {
                if f.kinds[i] == FloorKind::Concrete && !self.floor_gone[i] {
                    let c = self.sample_centre(i);
                    if (c[0] - p[0]).hypot(c[1] - p[2]) < r + f.spacing {
                        return true;
                    }
                }
            }
        }
        (0..self.gone.len()).any(|c| {
            if self.gone[c] {
                return false;
            }
            let o = self.cell_corner(c);
            let (dx, dy, dz) = (o[0] + 0.5 - p[0], o[1] + 0.5 - p[1], o[2] + 0.5 - p[2]);
            (dx * dx + dy * dy + dz * dz).sqrt() < r + 0.87
        })
    }

    /// One fixed tick: step the falling blocks (landings blast craters),
    /// then, if the holes changed, drop failed floor and start every
    /// unsupported cover group falling. Events go into `events` (appended).
    pub fn step(&mut self, dt: f32, t: &Tuning, rng: &mut Pcg32, events: &mut Vec<DestructEvent>) {
        self.refresh_masks();
        let mut falls = core::mem::take(&mut self.falls);
        let mut landed: Vec<([f32; 3], f32, u32)> = Vec::new();
        falls.retain_mut(|f| match self.step_fall(f, dt, t) {
            Step::Falling => true,
            Step::Out => {
                events.push(DestructEvent::Out { id: f.id });
                false
            }
            Step::Landed(p) => {
                landed.push((p, f.blocks(), f.id));
                false
            }
        });
        self.falls = falls;
        for (p, k, id) in landed {
            let r = t.collapse_hole_max.min(t.collapse_hole_r * k.cbrt());
            let seed = rng.below(64) as u8;
            self.blast([p[0], p[1] + t.collapse_up, p[2]], r, seed);
            events.push(DestructEvent::Land {
                id,
                pos: p,
                blocks: k,
                r,
            });
        }
        let v = self.holes.version();
        if self.checked != Some(v) {
            self.collapse(t, events);
            self.checked = Some(self.holes.version());
        }
        self.refresh_masks();
    }

    /// The support check (runs when the holes changed).
    fn collapse(&mut self, t: &Tuning, events: &mut Vec<DestructEvent>) {
        self.refresh_masks();
        // Floor that no longer holds drops as slabs; crumbs are cut away.
        let mut slabs: Vec<(Fall, Vec<usize>)> = Vec::new();
        for isl in self.floor_islands(t) {
            let Some(f) = &self.desc.floor else { break };
            let area = isl.len() as f32 * f.spacing * f.spacing;
            let (mut cx, mut cz) = (0.0, 0.0);
            for &i in &isl {
                let c = self.sample_centre(i);
                cx += c[0];
                cz += c[1];
            }
            cx /= isl.len().max(1) as f32;
            cz /= isl.len().max(1) as f32;
            if area <= t.floor_crumb_max {
                for &i in &isl {
                    self.floor_gone[i] = true;
                }
                events.push(DestructEvent::Crumbs {
                    pos: [cx, self.fy() - self.desc.slab * 0.5, cz],
                    samples: isl.len() as u32,
                });
                continue;
            }
            let slab = self.slab_fall(&isl, [cx, cz], t);
            for &i in &isl {
                self.floor_gone[i] = true;
            }
            slabs.push((slab, isl));
        }
        // Cover with its floor gone: a block that stood on a slab rides down on it; the rest tip and fall.
        let mut cover = Vec::new();
        for f in self.unsupported(t) {
            for &(c, _) in &f.cells {
                self.gone[c] = true;
            }
            match slabs.iter_mut().find(|(_, isl)| self.rests_on(&f, isl)) {
                Some((s, _)) => s.cells.extend_from_slice(&f.cells),
                None => cover.push(f),
            }
        }
        for (s, _) in slabs {
            self.start(s, t, events);
        }
        for f in cover {
            self.start(f, t, events);
        }
    }

    fn start(&mut self, mut f: Fall, t: &Tuning, events: &mut Vec<DestructEvent>) {
        self.next_id += 1;
        f.id = self.next_id;
        f.foot = self.foot(&f);
        if self.falls.len() < t.fall_max {
            events.push(DestructEvent::Fall {
                id: f.id,
                cells: f.cells.len() as u32,
                samples: f.samples.len() as u32,
                tipping: f.pivoting,
            });
            self.falls.push(f);
            return;
        }
        let p = self.fall_centre(&f);
        events.push(DestructEvent::Crumble {
            pos: p,
            blocks: f.blocks(),
        });
    }

    /// The middle of a block (before it moved).
    fn fall_centre(&self, f: &Fall) -> [f32; 3] {
        let (mut x, mut y, mut z, mut k) = (0.0, 0.0, 0.0, 0.0);
        for &(c, _) in &f.cells {
            let o = self.cell_corner(c);
            x += o[0] + 0.5;
            y += o[1] + 0.5;
            z += o[2] + 0.5;
            k += 1.0;
        }
        for &(i, _) in &f.samples {
            let c = self.sample_centre(i);
            x += c[0];
            y += self.fy() - self.desc.slab * 0.5;
            z += c[1];
            k += 1.0;
        }
        let k: f32 = if k > 0.0 { k } else { 1.0 };
        [x / k, y / k + f.y, z / k]
    }

    /// The bottom sample points of a block before it moved.
    fn foot(&self, f: &Fall) -> Vec<[f32; 3]> {
        let mut p = Vec::with_capacity(f.cells.len() * 5 + f.samples.len());
        for &(c, _) in &f.cells {
            let o = self.cell_corner(c);
            for s in BSMP {
                p.push([o[0] + s[0], o[1], o[2] + s[1]]);
            }
        }
        let bottom = self.fy() - self.desc.slab;
        for &(i, _) in &f.samples {
            let c = self.sample_centre(i);
            p.push([c[0], bottom, c[1]]);
        }
        p
    }

    /// Does cover block `f` rest on slab samples `isl`: are most of its lowest cells' middles over it?
    fn rests_on(&self, f: &Fall, isl: &[usize]) -> bool {
        let y0 = f
            .cells
            .iter()
            .map(|&(c, _)| self.desc.cells[c][2])
            .min()
            .unwrap_or(0);
        let (mut k, mut on) = (0, 0);
        for &(c, _) in &f.cells {
            if self.desc.cells[c][2] != y0 {
                continue;
            }
            let o = self.cell_corner(c);
            k += 1;
            if self
                .sample_at(o[0] + 0.5, o[2] + 0.5)
                .is_some_and(|i| isl.binary_search(&i).is_ok())
            {
                on += 1;
            }
        }
        k > 0 && on * 2 >= k
    }

    /// A slab that broke off: it drops with a slight tip toward the hub.
    fn slab_fall(&self, isl: &[usize], c: [f32; 2], t: &Tuning) -> Fall {
        let (hx, hz) = (self.desc.hub[0] - c[0], self.desc.hub[1] - c[1]);
        let r = hx.hypot(hz);
        let d = if r > 1e-3 {
            [hx / r, hz / r]
        } else {
            [1.0, 0.0]
        };
        Fall {
            id: 0,
            cells: Vec::new(),
            samples: isl.iter().map(|&i| (i, self.floor_mask[i])).collect(),
            foot: Vec::new(),
            y: 0.0,
            v: 0.0,
            p: [c[0], self.fy() - self.desc.slab * 0.5, c[1]],
            d,
            t: 0.0,
            o: t.tip_spin * 0.5,
            ms: 0.0,
            mh: 0.0,
            mi: 1.0,
            s: 0.0,
            h: 0.0,
            pivoting: false,
            lg: 1e9,
        }
    }

    /// Islands of concrete floor with no chain (through joins at least
    /// `floor_sliver_min` wide) to metal. Sample indices, ascending.
    fn floor_islands(&self, t: &Tuning) -> Vec<Vec<usize>> {
        let Some(f) = &self.desc.floor else {
            return Vec::new();
        };
        let (nx, nz) = (f.nx, f.nz);
        let n = nx * nz;
        if self.holes.is_empty() {
            return Vec::new();
        }
        let sol: Vec<bool> = (0..n).map(|i| self.sample_solid(i)).collect();
        // Thick: its solid runs along x and along z are both at least the sliver width.
        let k = ((t.floor_sliver_min / f.spacing - 1e-4).ceil().max(1.0)) as usize;
        let run = |i: usize, step_a: bool| -> usize {
            let (a, b) = (i / nz, i % nz);
            let mut len = 1;
            let mut j = 1;
            loop {
                let ok = if step_a {
                    a + j < nx && sol[(a + j) * nz + b]
                } else {
                    b + j < nz && sol[a * nz + b + j]
                };
                if !ok {
                    break;
                }
                len += 1;
                j += 1;
            }
            j = 1;
            loop {
                let ok = if step_a {
                    a >= j && sol[(a - j) * nz + b]
                } else {
                    b >= j && sol[a * nz + b - j]
                };
                if !ok {
                    break;
                }
                len += 1;
                j += 1;
            }
            len
        };
        let thick: Vec<bool> = (0..n)
            .map(|i| sol[i] && run(i, true) >= k && run(i, false) >= k)
            .collect();
        let nb = |i: usize, out: &mut [usize; 4]| -> usize {
            let (a, b) = (i / nz, i % nz);
            let mut m = 0;
            if a + 1 < nx {
                out[m] = i + nz;
                m += 1;
            }
            if a > 0 {
                out[m] = i - nz;
                m += 1;
            }
            if b + 1 < nz {
                out[m] = i + 1;
                m += 1;
            }
            if b > 0 {
                out[m] = i - 1;
                m += 1;
            }
            m
        };
        let mut seen = vec![false; n];
        let mut q: Vec<usize> = Vec::new();
        for (i, k) in f.kinds.iter().enumerate() {
            if *k == FloorKind::Metal {
                seen[i] = true;
                q.push(i);
            }
        }
        let mut buf = [0usize; 4];
        let mut h = 0;
        while h < q.len() {
            let m = nb(q[h], &mut buf);
            for &j in &buf[..m] {
                if thick[j] && !seen[j] {
                    seen[j] = true;
                    q.push(j);
                }
            }
            h += 1;
        }
        // Thin edges touching held floor stay with it.
        let held = seen.clone();
        for i in 0..n {
            if sol[i] && !seen[i] {
                let m = nb(i, &mut buf);
                if buf[..m].iter().any(|&j| held[j]) {
                    seen[i] = true;
                }
            }
        }
        let mut out = Vec::new();
        for i in 0..n {
            if !sol[i] || seen[i] || f.kinds[i] != FloorKind::Concrete {
                continue;
            }
            seen[i] = true;
            let mut g = vec![i];
            let mut h = 0;
            while h < g.len() {
                let m = nb(g[h], &mut buf);
                for &j in &buf[..m] {
                    if sol[j] && !seen[j] && f.kinds[j] == FloorKind::Concrete {
                        seen[j] = true;
                        g.push(j);
                    }
                }
                h += 1;
            }
            g.sort_unstable();
            out.push(g);
        }
        out
    }

    /// Cover that is no longer held up: connected groups of cells, each as a block ready to fall.
    fn unsupported(&self, t: &Tuning) -> Vec<Fall> {
        let n = self.desc.cells.len();
        let m: Vec<f32> = (0..n)
            .map(|c| {
                if self.gone[c] {
                    0.0
                } else {
                    self.mask[c].count_ones() as f32 / 27.0
                }
            })
            .collect();
        let mut grp = vec![usize::MAX; n];
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let nbs = |c: usize| {
            let g = self.desc.cells[c];
            N6.map(|d| self.cell_at(g[0] + d[0], g[1] + d[1], g[2] + d[2]))
        };
        // Cells that still carry load, in connected groups.
        for c in 0..n {
            if m[c] < t.cell_min_solid || grp[c] != usize::MAX {
                continue;
            }
            let gi = groups.len();
            grp[c] = gi;
            let mut g = vec![c];
            let mut h = 0;
            while h < g.len() {
                for e in nbs(g[h]).into_iter().flatten() {
                    if m[e] >= t.cell_min_solid && grp[e] == usize::MAX {
                        grp[e] = gi;
                        g.push(e);
                    }
                }
                h += 1;
            }
            groups.push(g);
        }
        // Cells with only a little left go with the group that reaches them first.
        let mut q: Vec<usize> = groups.iter().flatten().copied().collect();
        let mut h = 0;
        while h < q.len() {
            let gi = grp[q[h]];
            for e in nbs(q[h]).into_iter().flatten() {
                if m[e] > 0.0 && grp[e] == usize::MAX {
                    grp[e] = gi;
                    groups[gi].push(e);
                    q.push(e);
                }
            }
            h += 1;
        }
        // The rest form groups of their own.
        for c in 0..n {
            if m[c] <= 0.0 || grp[c] != usize::MAX {
                continue;
            }
            let gi = groups.len();
            grp[c] = gi;
            let mut g = vec![c];
            let mut h = 0;
            while h < g.len() {
                for e in nbs(g[h]).into_iter().flatten() {
                    if m[e] > 0.0 && grp[e] == usize::MAX {
                        grp[e] = gi;
                        g.push(e);
                    }
                }
                h += 1;
            }
            groups.push(g);
        }
        let mut out = Vec::new();
        for mut g in groups {
            g.sort_unstable();
            if let Some(f) = self.topple(&g, &m, t) {
                out.push(f);
            }
        }
        out
    }

    /// Support of one group: `None` while it stands, else the block that falls and how it tips.
    fn topple(&self, g: &[usize], m: &[f32], t: &Tuning) -> Option<Fall> {
        let fy = self.fy();
        let o = self.desc.origin;
        let (mut mass, mut mu, mut mv, mut ch) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut cols: Vec<(i32, i32)> = Vec::new();
        for &c in g {
            let w = m[c];
            let gc = self.desc.cells[c];
            mass += w;
            mu += w * (gc[0] as f32 + 0.5);
            mv += w * (gc[1] as f32 + 0.5);
            ch += w * (gc[2] as f32 + 0.5);
            if !cols.contains(&(gc[0], gc[1])) {
                cols.push((gc[0], gc[1]));
            }
        }
        if mass <= 0.0 {
            return None;
        }
        mu /= mass;
        mv /= mass;
        ch /= mass;
        // Held points of the footprint (grid u, v).
        let mut pts: Vec<[f32; 2]> = Vec::new();
        for &c in g {
            let gc = self.desc.cells[c];
            if gc[2] != 0 || m[c] < t.cell_min_solid {
                continue;
            }
            for i in 0..SG {
                for j in 0..SG {
                    let (u, v) = (
                        gc[0] as f32 + (i as f32 + 0.5) / SG as f32,
                        gc[1] as f32 + (j as f32 + 0.5) / SG as f32,
                    );
                    let (x, z) = (o[0] + u, o[2] + v);
                    if self.holes.holed([x, fy + 0.1, z]) || !self.solid([x, fy - BELOW, z]) {
                        continue;
                    }
                    pts.push([u, v]);
                }
            }
        }
        let sup = !pts.is_empty();
        let area = pts.len() as f32 / (SG * SG) as f32;
        let mut hl = Vec::new();
        let mut sorted = pts.clone();
        hull(&mut sorted, &mut hl);
        let (out_d, out_q) = outside(&hl, [mu, mv]);
        if sup && area >= t.support_min * cols.len() as f32 && out_d <= t.tip_margin {
            return None;
        }
        let (mut du, mut dv, mut pu, mut pv) = (0.0, 0.0, mu, mv);
        if sup && out_d > t.tip_margin {
            // Its weight hangs past what holds it: it tips over the nearest edge of its support.
            du = mu - out_q[0];
            dv = mv - out_q[1];
            pu = out_q[0];
            pv = out_q[1];
        } else {
            // Too little holds it: it goes the way its mass leans, from what support is left (or its footprint's middle).
            let (mut su, mut sv, mut k) = (0.0, 0.0, 0.0);
            if sup {
                for p in &pts {
                    su += p[0];
                    sv += p[1];
                    k += 1.0;
                }
            } else {
                for &(a, b) in &cols {
                    su += a as f32 + 0.5;
                    sv += b as f32 + 0.5;
                    k += 1.0;
                }
            }
            if k > 0.0 {
                du = mu - su / k;
                dv = mv - sv / k;
            }
        }
        let mut dl = du.hypot(dv);
        if dl < 1e-3 {
            (du, dv, dl) = (1.0, 0.0, 1.0);
        }
        du /= dl;
        dv /= dl;
        if sup && out_d <= t.tip_margin {
            let mut best = f32::MIN;
            for p in &pts {
                let s = p[0] * du + p[1] * dv;
                if s > best {
                    best = s;
                    pu = p[0];
                    pv = p[1];
                }
            }
        }
        let ms = if sup {
            (mu - pu) * du + (mv - pv) * dv
        } else {
            0.0
        };
        let mh = if sup { ch } else { 0.0 };
        let mut gy = 1.0 / 6.0;
        for &c in g {
            let gc = self.desc.cells[c];
            let s = (gc[0] as f32 + 0.5 - mu) * du + (gc[1] as f32 + 0.5 - mv) * dv;
            let hh = gc[2] as f32 + 0.5 - ch;
            gy += m[c] * (s * s + hh * hh) / mass;
        }
        let piv = sup && ms > 0.02;
        let (px, pz) = (o[0] + pu, o[2] + pv);
        Some(Fall {
            id: 0,
            cells: g.iter().map(|&c| (c, self.mask[c])).collect(),
            samples: Vec::new(),
            foot: Vec::new(),
            y: 0.0,
            v: 0.0,
            p: [px, fy + if sup { 0.0 } else { ch }, pz],
            d: [du, dv],
            t: 0.0,
            o: if piv { t.tip_spin * 0.5 } else { t.tip_spin },
            ms,
            mh,
            mi: ms * ms + mh * mh + gy,
            s: 0.0,
            h: 0.0,
            pivoting: piv,
            lg: if sup { px * du + pz * dv } else { -1e9 },
        })
    }

    /// One step of a falling block (it collides with nothing on the way: its
    /// concrete is already gone). While on its ledge it turns about the pivot
    /// under its own weight; past `tip_release` it slides off and falls free,
    /// still turning (up to `tip_max`), in short moves until a point under it
    /// meets something solid.
    fn step_fall(&self, f: &mut Fall, s: f32, t: &Tuning) -> Step {
        let g = t.fall_gravity;
        if f.pivoting {
            let k = ((s / 0.01).ceil() as u32).max(1);
            let hs = s / k as f32;
            for _ in 0..k {
                f.o += g * (f.ms * f.t.cos() + f.mh * f.t.sin()) / f.mi.max(1e-4) * hs;
                f.t += f.o * hs;
                if f.t < t.tip_release {
                    continue;
                }
                // Over the edge: from here it turns about its centre of mass, keeping the speed the turn gave it.
                let (c, sn) = (f.t.cos(), f.t.sin());
                let (rs, rh) = (f.ms * c + f.mh * sn, -f.ms * sn + f.mh * c);
                f.s += rs - f.ms;
                f.y += rh - f.mh;
                f.p[0] += f.d[0] * f.ms;
                f.p[2] += f.d[1] * f.ms;
                f.p[1] += f.mh;
                f.h = (f.o * rh).max(0.0);
                f.v = -f.o * rs;
                f.ms = 0.0;
                f.mh = 0.0;
                f.pivoting = false;
                break;
            }
            return Step::Falling;
        }
        let v1 = (f.v - g * s).max(-40.0);
        let dy = (f.v + v1) * 0.5 * s;
        f.v = v1;
        let (t0, s0, y0) = (f.t, f.s, f.y);
        let t1 = t.tip_max.min(f.t + f.o * s);
        if t1 >= t.tip_max {
            f.o = 0.0;
        }
        let s1 = f.s + f.h * s;
        let band = self.fy() - self.desc.slab - 0.05;
        let steps = ((((-dy).max(s1 - s0).max((t1 - t0) * 2.0)) / 0.2).ceil() as u32).max(1);
        for j in 1..=steps {
            let a = j as f32 / steps as f32;
            f.y = y0 + dy * a;
            f.t = t0 + (t1 - t0) * a;
            f.s = s0 + (s1 - s0) * a;
            let (mut k, mut sum) = (0.0, [0.0f32; 3]);
            for &q in &f.foot {
                let n = f.transform(q);
                if n[1] > band && n[0] * f.d[0] + n[2] * f.d[1] <= f.lg + 0.15 {
                    continue;
                }
                if self.solid([n[0], n[1] - BELOW, n[2]]) {
                    k += 1.0;
                    sum = [sum[0] + n[0], sum[1] + n[1], sum[2] + n[2]];
                }
            }
            if k > 0.0 {
                let b = (j - 1) as f32 / steps as f32;
                f.y = y0 + dy * b;
                f.t = t0 + (t1 - t0) * b;
                f.s = s0 + (s1 - s0) * b;
                return Step::Landed([sum[0] / k, sum[1] / k, sum[2] / k]);
            }
        }
        f.y = y0 + dy;
        f.t = t1;
        f.s = s1;
        if self.fy() + f.y < t.fall_death_y {
            Step::Out
        } else {
            Step::Falling
        }
    }
}

enum Step {
    Falling,
    Landed([f32; 3]),
    Out,
}

#[cfg(test)]
#[path = "structure_tests.rs"]
mod tests;
