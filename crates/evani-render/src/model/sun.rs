//! Baked sun shadow for models, for the tiers without a shadow map (catalog:
//! Lite and Low have "blob or baked shadows"). Chris's rule is that a phone
//! shows the PC's look: on Medium and up the shadow map darkens the parts of
//! a model its own body hides from the sun (the front of a coat when the sun
//! is behind and above), so Lite and Low must darken them too.
//!
//! Every vertex gets a cone of open sky: of 64
//! directions over the half-sphere in front of its normal, the ones where
//! nothing of the model lies ahead (looked up in one small depth map per
//! direction of an occupancy grid of the whole model, see
//! [`Bake`]), and of a few axes (towards the
//! open rays, away from the blocked ones, between, the normal) the one
//! whose best cosine threshold misclassifies the fewest rays is kept with
//! that threshold. "Away from the blocked ones" is what holds an overhang
//! (blocked above, open round the sides). The cone does not
//! depend on the light, so the light can move and instances can turn and
//! mirror: model.wgsl transforms the direction like a normal and lets the
//! sun through where `dot(direction, sun)` is above the threshold.
//!
//! Which parts shade which: only the parts drawn together. A model with LOD
//! levels (`Part::lod`) draws one level at a time, so a vertex of level k is
//! shaded by the parts shared by every level plus the parts of level k, never
//! by another level's (hidden) parts. A shared part is drawn with every level;
//! it is baked once, against the shared parts plus the lowest level present
//! (LOD0, the level drawn when the model is closest and the shading shows
//! most). The grid's cells are laid on the box of the shared parts and that
//! lowest level, then grown by whole cells to hold the other levels, so cells
//! and bytes are the same for a model without levels, and a converted file
//! bakes its LOD0 on its master's cells. (Laid on the whole model's box, a
//! hidden level 1 cm wider than LOD0, kart_fixture's simplified rear wheels,
//! moved every cell a fifth of a cell; a ray starts two cells off its
//! surface, so the corners whose rays start beside another surface's cells
//! flipped, and the converted file shaded 2% of its sunlit area differently
//! from its master: the quality gate, S3-6.) Each extra level adds its own
//! occupancy layer and its own 64 depth maps (see [`Bake`]).
//!
//! When: not at load. Only the tiers without a shadow map read the cone,
//! so the GPU side bakes a model the first time it draws it on one, a
//! share of [`FRAME_UNITS`] a frame ([`Bake`]): a model loaded on such a
//! tier shows once its cones are in; one loaded on Medium and up shows at
//! once and is baked only if the tier later drops. The result is the same
//! bytes however the work is split.
//!
//! Big triangles: the GPU blends a triangle's three corner cones across it,
//! so on a face many cells wide one corner's sky is spread over metres. A
//! corner where another surface meets the face (a wall standing on a floor)
//! starts its rays inside that surface's cells and sees almost no sky, and
//! blended across a 2 m face that one dark corner drew a diagonal shade the
//! shadow map never draws. So a corner of a triangle longer than [`COARSE`]
//! cells takes its cone from points inside the triangle instead: each such
//! triangle votes, per direction, from the middle of the corner's share of it
//! (weights 1/2, 1/4, 1/4), weighted by area, alongside the corner itself
//! for its small triangles ([`Vote`]). Only faces within 60 degrees of the
//! corner's normal vote (a cylinder's cap sharing its side normals does
//! not), votes within 0.1% of even count as open, and a direction in front
//! of the corner that no face voted on keeps the corner's own ray. A point
//! carried by several parts (a model split over primitives) takes the votes of every part's triangles, so the split
//! does not change the shade. A model of small triangles has no votes and
//! bakes the same bytes as before.
//!
//! Near field: a ray's start, [`START`] cells off its surface, clears the
//! cells its own surface marks, but a layer laid closer than that over the
//! surface (a hair cap 2 mm over a scalp, locks 6 mm over the cap, on a head
//! whose cells are 4.5 mm) lay behind the start and shaded nothing, where a
//! shadow map's bias is about a hundredth of the model: the material
//! viewer's bust read 50% lit baked and 35% with a shadow map (2026-10-26).
//! So each direction's pass also splats the model's points (its vertices,
//! and points at most [`SAMPLE`] cells apart inside its bigger triangles) into a
//! second map that keeps, per texel, the furthest point along the direction
//! and its surface, and the furthest of any other surface ([`Other`]); a
//! vertex the depth maps leave open is shut when another surface's point in
//! its texel lies further along than its near start, [`NEAR_N`] cells off
//! its surface (and [`NEAR_D`] along the ray), as a shadow map's bias starts
//! it. A surface ([`surfaces`]) is the vertices joined by triangles or
//! sharing a point, so a layer laid over another is a surface of its own;
//! the vertex's own surface is left to the depth maps as before, so a model
//! of one surface bakes the same bytes and skips the second map.
//!
//! Stored in the vertex bytes that were free: the direction times
//! `(1 + threshold) / 2` as three snorm8 values, x and y in the position's
//! unused w (bytes 6 and 7), z in the normal's unused w (byte 11). All zero
//! (length 0, threshold -1) is "open everywhere", what an unbaked vertex
//! gets.

use super::{Part, SHARED, VERTEX_BYTES};

/// Cells along the longest side of the occupancy grid, or for a long model
/// along [`WIDE`] times its middle side (see [`Grid::empty`]).
const GRID: f32 = 64.0;
/// A model whose longest side is more than this many times its middle side
/// is long: its cells are laid over this many times the middle side instead
/// of over the longest (see [`Grid::empty`]).
const WIDE: f32 = 2.0;
/// At most this many times [`GRID`] cells along the longest side (a long
/// thin rod).
const LONG: f32 = 8.0;
/// A long model's own surface may cover at most this many times a cube's six
/// faces of [`GRID`] x [`GRID`] cells: what marking and the depth maps cost
/// scales with it.
const SURFACE: f32 = 4.0;
/// Empty cells round the model, so a ray's start (at most [`START`] cells
/// off the surface) is always inside the grid and on its depth maps.
const PAD: f32 = 3.0;
/// A ray starts this many cells off the surface along the normal: past the
/// cells its own surface marks (a cell's diagonal is 1.73).
const START: f32 = 2.0;
/// Sample directions over the sphere (a Fibonacci sphere); a vertex uses
/// the ones in front of its normal, about half.
pub const DIRS: usize = 64;
/// How far past its own texel a marked cell reaches in a direction's depth
/// map, in cells either side: a cell seen at an angle is 1 to 1.7 cells
/// wide, a texel 1.
const SPLAT: f32 = 0.15;
/// Steps of the cosine a cone's threshold is chosen from (see [`fit`]).
const BINS: usize = 64;
/// A triangle with an edge longer than this many cells is too big for its
/// corners alone (see [`Vote`]).
const COARSE: f32 = 4.0;
/// A big triangle votes for a corner only when its face normal is within
/// 60 degrees of the corner's normal (see [`votes`]).
const VOTE_COS: f32 = 0.5;
/// Votes this close to even (a share of their total weight) are a tie,
/// which counts as open (see [`Bake::voted`]).
const TIE: f32 = 1e-3;
/// The near start is this many cells off the surface along the normal, and
/// this many more along the ray: a shadow map's bias, about a hundredth of
/// the model, where [`START`] is about three (see the module notes).
const NEAR_N: f32 = 0.64;
const NEAR_D: f32 = 0.32;
/// The near field's points inside a triangle longer than this many cells are
/// at most this far apart (a texel is a cell; a smaller triangle's corners,
/// the vertices, are close enough), and each reaches [`NEAR_SPLAT`] of a
/// cell either side into the map, so points a cell apart leave no texel of
/// a surface empty. Half a cell apart, the bust's skin and hair made ten
/// times as many points as vertices.
const SAMPLE: f32 = 1.0;
const NEAR_SPLAT: f32 = 0.35;
/// The near field's slope bias grows with the ray's lean (see
/// [`Bake::dir_test`]) up to this many cells: past it a ray that leans
/// further still finds a layer over its surface (uncapped, a cap 1.2 cells
/// over a sphere let 7% of it through), and short of it a surface 0.3 cells
/// under the start is not taken for a layer over it (capped at 2 cells, 15%
/// of a sphere under such a cap went dark; model_wgsl
/// a_layer_close_over_a_surface_shades_it).
const BIAS_CAP: f32 = 3.0;
/// The near field's points are splatted this many at a time, one step each
/// (a fur ruff's long triangles hold hundreds each: all in one step made a
/// direction several frames' work).
const CHUNK: usize = 8192;
/// A surface byte (see [`Bake::code`]) for no point yet, and for every
/// surface past the 254th.
const EMPTY: u8 = u8::MAX;
const MANY: u8 = u8::MAX - 1;

/// Calls `f` with points inside the triangle `c`, at most `gap` apart, in
/// rows along its longest edge (a lattice over the longest edge put
/// hundreds in a fur ruff's long thin strands; rows follow the area), not
/// its corners; returns how many.
fn inner_points(c: [[f32; 3]; 3], gap: f32, f: impl FnMut([f32; 3])) -> u32 {
    rows_of(c, gap, Some(f))
}

/// How many points [`inner_points`] gives the triangle `c`, row by row
/// without making them.
fn inner_count(c: [[f32; 3]; 3], gap: f32) -> u32 {
    rows_of(c, gap, None::<fn([f32; 3])>)
}

/// [`inner_points`], calling `f` only when given.
fn rows_of(c: [[f32; 3]; 3], gap: f32, mut f: Option<impl FnMut([f32; 3])>) -> u32 {
    let len = |a: [f32; 3], b: [f32; 3]| dot([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [b[0] - a[0], b[1] - a[1], b[2] - a[2]]).sqrt();
    // The longest edge a-b and the apex.
    let (a, b, apex) = [(0, 1, 2), (1, 2, 0), (2, 0, 1)].into_iter().map(|(i, j, k)| (c[i], c[j], c[k])).fold((c[0], c[1], c[2]), |best, e| if len(e.0, e.1) > len(best.0, best.1) { e } else { best });
    let long = len(a, b);
    if !(long > 0.0 && gap > 0.0) {
        return 0;
    }
    let x = cross([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [apex[0] - a[0], apex[1] - a[1], apex[2] - a[2]]);
    let height = dot(x, x).sqrt() / long;
    let rows = ((height / gap).ceil() as usize).clamp(1, 4096);
    let mut n = 0;
    for r in 0..rows {
        let t = r as f32 / rows as f32;
        let p = [0, 1, 2].map(|k| a[k] + (apex[k] - a[k]) * t);
        let q = [0, 1, 2].map(|k| b[k] + (apex[k] - b[k]) * t);
        let along = ((long * (1.0 - t) / gap).ceil() as usize).clamp(1, 4096);
        // The first row is the edge, less its two corners.
        n += if r == 0 { along - 1 } else { along + 1 } as u32;
        let Some(f) = f.as_mut() else { continue };
        for i in 0..=along {
            if r == 0 && (i == 0 || i == along) {
                continue;
            }
            let s = i as f32 / along as f32;
            f([0, 1, 2].map(|k| p[k] + (q[k] - p[k]) * s));
        }
    }
    n
}

/// One step's points for the near field's map: vertices `lo..hi` of part
/// `part`, or big triangles `lo..hi` of [`Bake::big`]; `n` points.
#[derive(Clone, Copy)]
struct Src {
    tris: bool,
    part: u32,
    lo: u32,
    hi: u32,
    n: u32,
}

/// A direction's map: its axes across the direction, the corner of texel
/// (0, 0) along them, and its size in texels.
#[derive(Clone, Copy, Default)]
struct Layout {
    u: [f32; 3],
    v: [f32; 3],
    u0: f32,
    v0: f32,
    w: usize,
    h: usize,
}

/// A texel of the near field's map: the furthest point along the direction,
/// the furthest of any surface but that point's, and that point's surface
/// byte. The furthest point of any surface but `s` is the first unless `s`
/// is its surface, then the second: kept exactly as points come in.
#[derive(Clone, Copy)]
struct Other(f32, f32, u8);

impl Other {
    const NONE: Self = Self(f32::MIN, f32::MIN, EMPTY);

    fn add(&mut self, depth: f32, surface: u8) {
        if surface == self.2 {
            self.0 = self.0.max(depth);
        } else if depth > self.0 {
            // The old furthest is another surface's than the new one.
            *self = Self(depth, self.0, surface);
        } else {
            self.1 = self.1.max(depth);
        }
    }

    /// The furthest point along the direction of any surface but `surface`.
    fn beyond(&self, surface: u8) -> f32 {
        if surface == self.2 { self.1 } else { self.0 }
    }
}

/// Which surface each vertex (over the whole model) is on: the vertices
/// joined by triangles, and the copies of one point (the same set and
/// position to a 64th of a cell, whatever their normals: a seam or a hard
/// edge does not cut a surface), share one, numbered by its lowest vertex.
/// A layer laid over another (a hair cap over a scalp, a lid over an eye) is
/// a surface of its own.
/// In steps (ov1, 2026-11-02): the copies a share at a time ([`Firsts`]),
/// then one part's triangles a step, then the roots.
struct SurfaceSteps {
    pos: Vec<[f32; 3]>,
    set: Vec<u32>,
    zero: Vec<[f32; 3]>,
    f: Firsts,
    part: usize,
}

/// The root of `i`, halving the path to it.
fn root(up: &mut [u32], mut i: usize) -> usize {
    while let Some(&u) = up.get(i) {
        if u as usize == i {
            break;
        }
        let uu = up.get(u as usize).copied().unwrap_or(u);
        if let Some(x) = up.get_mut(i) {
            *x = uu;
        }
        i = u as usize;
    }
    i
}

impl SurfaceSteps {
    fn new(spaces: &[Space], place: &[Place], tol: f32) -> Self {
        let pos: Vec<[f32; 3]> = spaces.iter().flat_map(|s| s.0.iter().copied()).collect();
        let set: Vec<u32> = place.iter().flat_map(|pl| pl.range.clone().map(|_| pl.set as u32)).collect();
        let zero = vec![[0.0; 3]; pos.len()];
        let f = Firsts::new(&pos, &zero, &set, tol);
        Self { pos, set, zero, f, part: 0 }
    }

    /// About `left` work units more, taken off it; each vertex's surface
    /// once done.
    fn step(&mut self, parts: &[Part], place: &[Place], left: &mut u64) -> Option<Vec<u32>> {
        if self.f.at < self.f.first.len() {
            let n = (*left / INIT_UNITS.firsts).max(256) as usize;
            let before = self.f.at;
            self.f.step(&self.pos, &self.zero, &self.set, n);
            *left = left.saturating_sub((self.f.at - before) as u64 * INIT_UNITS.firsts);
            return None;
        }
        let up = &mut self.f.first;
        if let (Some(p), Some(pl)) = (parts.get(self.part), place.get(self.part)) {
            self.part += 1;
            *left = left.saturating_sub((p.indices.len() / 3) as u64 * INIT_UNITS.tri);
            for t in p.indices.as_chunks::<3>().0 {
                for k in [t[1], t[2]] {
                    let a = root(up, pl.range.start + t[0] as usize);
                    let b = root(up, pl.range.start + k as usize);
                    let (lo, hi) = (a.min(b), a.max(b));
                    if let Some(x) = up.get_mut(hi) {
                        *x = lo as u32;
                    }
                }
            }
            return None;
        }
        *left = left.saturating_sub(self.pos.len() as u64);
        Some((0..self.pos.len()).map(|i| root(up, i) as u32).collect())
    }
}

/// The fixed sample directions.
pub fn dirs() -> [[f32; 3]; DIRS] {
    let mut d = [[0.0; 3]; DIRS];
    let golden = core::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    for (i, o) in d.iter_mut().enumerate() {
        let y = 1.0 - (i as f32 + 0.5) * 2.0 / DIRS as f32;
        let r = (1.0 - y * y).max(0.0).sqrt();
        let (s, c) = (golden * i as f32).sin_cos();
        *o = [r * c, y, r * s];
    }
    d
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The bits of the directions in front of `n`.
fn front(n: [f32; 3], dirs: &[[f32; 3]; DIRS]) -> u64 {
    dirs.iter().enumerate().filter(|d| dot(*d.1, n) > 0.0).fold(0, |m, d| m | 1 << d.0)
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 1e-12 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0; 3] }
}

/// A part's vertices in model space: positions through the node matrix and
/// normals through its cofactor (turned back out when it mirrors), both as
/// the GPU reads the quantised bytes.
pub fn model_space(part: &Part) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let m = &part.node;
    let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
    let x12 = cross(c[1], c[2]);
    let s = if dot(c[0], x12) < 0.0 { -1.0 } else { 1.0 };
    let cof = [x12, cross(c[2], c[0]), cross(c[0], c[1])];
    let n = part.vertices.len() / VERTEX_BYTES;
    let (mut pos, mut nrm) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for v in part.vertices.as_chunks::<VERTEX_BYTES>().0 {
        let mut l = [0.0; 3];
        for (k, x) in l.iter_mut().enumerate() {
            let q = f32::from(u16::from_le_bytes([v[k * 2], v[k * 2 + 1]])) / 65535.0;
            *x = part.pos_offset[k] + part.pos_scale[k] * q;
        }
        pos.push([0, 1, 2].map(|r| c[0][r] * l[0] + c[1][r] * l[1] + c[2][r] * l[2] + m[12 + r]));
        let ln = [0, 1, 2].map(|k| f32::from(v[8 + k] as i8) / 127.0);
        nrm.push(normalize([0, 1, 2].map(|r| (cof[0][r] * ln[0] + cof[1][r] * ln[1] + cof[2][r] * ln[2]) * s)));
    }
    (pos, nrm)
}

/// Which cells of the model's box hold a surface. Layer 0 holds the parts
/// every LOD level draws, layer `k + 1` the parts of the k-th level present;
/// a level's occluders are layer 0 plus its own layer ([`Self::marked`]).
pub struct Grid {
    lo: [f32; 3],
    cell: f32,
    inv: f32,
    dim: [usize; 3],
    /// The box the cells were laid on: the drawn level's (see [`Self::empty`]).
    view: [[f32; 3]; 2],
    occ: Vec<Vec<bool>>, #[cfg(feature = "sun-exact")] exact: ExactIndex,
}

impl Grid {
    /// The empty grid of `layers` layers (model space): cells laid on the
    /// box of the points `drawn` (the shared parts and the lowest level),
    /// grown by whole cells to hold the points `rest` (the other levels);
    /// [`Self::mark`] fills it. So a hidden level that reaches further out
    /// does not move the drawn level's cells.
    ///
    /// The cell: [`GRID`] cells over the longest side, as before, unless the
    /// model is long (its longest side over [`WIDE`] times its middle one):
    /// then [`GRID`] cells over [`WIDE`] times the middle side, so a long
    /// model is cut as finely across as a short one. Over the longest side
    /// alone, a 9.7 m row of balls 1 m wide and 24 cm apart got 15 cm cells;
    /// a ray starts [`START`] cells (30 cm) off its surface, so a ball's rays
    /// towards its neighbour started inside the neighbour and read blocked
    /// (9 points darker than the shadow map; 3 cm cells now). Never finer
    /// than [`LONG`] times [`GRID`] cells along the longest side, nor so fine
    /// that `area` (the drawn triangles' area) covers more than [`SURFACE`]
    /// times a cube's six faces: a long model holds at most about twice a
    /// cube's cells. A model that is not long keeps its cells byte for byte:
    /// a finer grid on the Frontier Marshal (2.0 x 2.2 x 0.5 m) also sees
    /// cloth layers 1 to 3 cm over its coat that the shadow map's offset
    /// skips, and read up to 6 points darker than the map.
    fn empty<'a>(drawn: impl Iterator<Item = &'a [f32; 3]>, rest: impl Iterator<Item = &'a [f32; 3]>, layers: usize, area: f32) -> Self {
        let grow = |(mut lo, mut hi): ([f32; 3], [f32; 3]), p: &[f32; 3]| {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            (lo, hi)
        };
        let (mut lo, mut hi) = drawn.fold(([f32::MAX; 3], [f32::MIN; 3]), grow);
        if lo[0] > hi[0] {
            (lo, hi) = ([0.0; 3], [0.0; 3]);
        }
        let (all_lo, all_hi) = rest.fold((lo, hi), grow);
        let e = [0, 1, 2].map(|k| hi[k] - lo[k]);
        let ext = e[0].max(e[1]).max(e[2]).max(1e-6);
        // The middle side: the one that is neither longest nor shortest.
        let mid = (e[0] + e[1] + e[2] - ext - e[0].min(e[1]).min(e[2])).max(0.0);
        let side = (WIDE * mid).max(ext / LONG).max((area / (6.0 * SURFACE)).sqrt()).min(ext);
        let cell = side / GRID;
        let view = lo.map(|x| x - PAD * cell);
        let view = [view, [0, 1, 2].map(|k| view[k] + ((hi[k] - view[k]) / cell + PAD).ceil().max(1.0) * cell)];
        // The drawn level's cells, grown by whole cells to hold every level.
        let lo = [0, 1, 2].map(|k| view[0][k] - ((lo[k] - all_lo[k]) / cell).ceil() * cell);
        let dim = [0, 1, 2].map(|k| ((all_hi[k] - lo[k]) / cell + PAD).ceil().max(1.0) as usize);
        Self { lo, cell, inv: 1.0 / cell, dim, view, occ: vec![vec![false; dim[0] * dim[1] * dim[2]]; layers.max(1)], #[cfg(feature = "sun-exact")] exact: ExactIndex::default() }
    }

    /// The cell holding `p`, or `None` outside the grid.
    fn cell_of(&self, p: [f32; 3]) -> Option<[usize; 3]> {
        let mut i = [0usize; 3];
        for k in 0..3 {
            let c = ((p[k] - self.lo[k]) * self.inv).floor();
            if !(c >= 0.0 && c < self.dim[k] as f32) {
                return None;
            }
            i[k] = c as usize;
        }
        Some(i)
    }

    /// Marks the cells under a triangle in `layer` (points at most half a
    /// cell apart); returns the points it tried (its share of the bake's
    /// work).
    fn mark(&mut self, layer: usize, [a, b, c]: [[f32; 3]; 3]) -> u64 {
        let len = |u: [f32; 3], v: [f32; 3]| dot([v[0] - u[0], v[1] - u[1], v[2] - u[2]], [v[0] - u[0], v[1] - u[1], v[2] - u[2]]).sqrt();
        let steps = ((len(a, b).max(len(b, c)).max(len(c, a)) / (0.5 * self.cell)).ceil() as usize).clamp(1, 4096);
        let f = 1.0 / steps as f32;
        for i in 0..=steps {
            for j in 0..=steps - i {
                let (u, v) = (i as f32 * f, j as f32 * f);
                let p = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * u + (c[k] - a[k]) * v);
                if let Some(i) = self.cell_of(p)
                    && let Some(o) = self.occ.get_mut(layer)
                {
                    o[(i[2] * self.dim[1] + i[1]) * self.dim[0] + i[0]] = true;
                }
            }
        }
        ((steps + 1) * (steps + 2) / 2) as u64
    }

    /// The start of a vertex's rays: [`START`] cells off the surface.
    pub fn start(&self, p: [f32; 3], n: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|k| p[k] + n[k] * START * self.cell)
    }

    /// The centre of every cell marked in layer 0 or in `layer`.
    fn marked(&self, layer: Option<usize>) -> Vec<[f32; 3]> {
        let mut marked = Vec::new();
        let own = layer.and_then(|l| self.occ.get(l)).filter(|o| !o.is_empty());
        for (x, _) in self.occ[0].iter().enumerate().filter(|o| *o.1 || own.is_some_and(|w| w[o.0])) {
            let i = [x % self.dim[0], (x / self.dim[0]) % self.dim[1], x / (self.dim[0] * self.dim[1])];
            marked.push([0, 1, 2].map(|k| self.lo[k] + (i[k] as f32 + 0.5) * self.cell));
        }
        marked
    }

    /// The open-sky cone of a vertex with normal `n` (model space) whose
    /// rays' open bits are `mask` (from the depth maps of [`Bake`]):
    /// `(direction, threshold)`. Open everywhere is `([0; 3], -1)`.
    pub fn cone(n: [f32; 3], mask: u64, dirs: &[[f32; 3]; DIRS]) -> ([f32; 3], f32) {
        Self::cone_over(n, front(n, dirs), mask, dirs)
    }

    /// [`Self::cone`] fitted over the directions `seen` (bits) only: those
    /// in front of the normal, or for a corner of big triangles those in
    /// front of any face that voted ([`Vote`]).
    fn cone_over(n: [f32; 3], seen_bits: u64, mask: u64, dirs: &[[f32; 3]; DIRS]) -> ([f32; 3], f32) {
        let mut seen = [([0.0f32; 3], false); DIRS];
        let (mut count, mut open, mut sum) = (0, 0, [0.0f32; 3]);
        for (j, d) in dirs.iter().enumerate().filter(|d| (seen_bits >> d.0) & 1 == 1) {
            let o = (mask >> j) & 1 == 1;
            seen[count] = (*d, o);
            count += 1;
            if o {
                open += 1;
                sum = [sum[0] + d[0], sum[1] + d[1], sum[2] + d[2]];
            }
        }
        if open == count {
            return ([0.0; 3], -1.0);
        }
        if open == 0 {
            return (n, 1.0);
        }
        // Candidate axes: towards the open sky; away from what blocks it
        // (an overhang: blocked above, open round the sides, is a cone
        // round "down"); the line between the two; the normal. Each gets
        // the threshold that misclassifies the fewest rays, and the best
        // one is kept.
        let blocked = [0, 1, 2].map(|k| seen[..count].iter().filter(|s| !s.1).map(|s| s.0[k]).sum::<f32>());
        let towards = normalize(sum);
        let away = normalize(blocked.map(|x| -x));
        let between = normalize([0, 1, 2].map(|k| towards[k] + away[k]));
        let mut best = (usize::MAX, n, -1.0);
        for axis in [towards, away, between, n] {
            if dot(axis, axis) < 0.5 {
                continue;
            }
            let (wrong, t) = fit(&seen[..count], axis, count - open);
            if wrong < best.0 {
                best = (wrong, axis, t);
                if wrong == 0 {
                    break;
                }
            }
        }
        (best.1, best.2)
    }
}

/// The threshold on the cosine to `axis` that misclassifies the fewest of
/// the rays `seen` (everything above it open): `(misclassified, threshold)`.
/// The splits tried are between [`BINS`] equal steps of the cosine (the
/// rays sorted by step, not one by one: no sort per vertex), and the
/// threshold sits halfway between the rays either side of the best split.
fn fit(seen: &[([f32; 3], bool)], axis: [f32; 3], blocked: usize) -> (usize, f32) {
    // Per step: open rays minus blocked rays whose cosine falls in it, and
    // the lowest and highest cosine in it.
    let (mut net, mut lo, mut hi) = ([0i32; BINS], [2.0f32; BINS], [-2.0f32; BINS]);
    for (d, o) in seen {
        let c = dot(*d, axis);
        let b = (((c + 1.0) * 0.5 * BINS as f32) as usize).min(BINS - 1);
        net[b] += if *o { 1 } else { -1 };
        (lo[b], hi[b]) = (lo[b].min(c), hi[b].max(c));
    }
    // Threshold -1: every ray called open, so the blocked ones are wrong;
    // each step the threshold passes turns its open rays wrong and its
    // blocked ones right.
    let mut wrong = blocked as i32;
    let (mut best, mut split) = (wrong, None);
    for (b, n) in net.iter().enumerate() {
        wrong += n;
        if wrong < best {
            (best, split) = (wrong, Some(b));
        }
    }
    let t = split.map_or(-1.0, |b| {
        let below = hi[..=b].iter().fold(-2.0f32, |a, x| a.max(*x));
        let above = lo[b + 1..].iter().fold(2.0f32, |a, x| a.min(*x));
        if above > 1.5 { 1.0 } else { 0.5 * (below + above) }
    });
    (best.max(0) as usize, t)
}

/// Work units of [`Bake::step`]: one is about one splat of one marked cell
/// (or one vertex looked up) in a direction's depth map. Measured on the
/// The proof figure (native, optimised): a point tried while marking costs
/// about 3, a vertex's cone about 170.
const MARK_UNITS: u64 = 3;
const CONE_UNITS: u64 = 170;
/// Work units a frame gives the bake on the GPU side: at most about 2 ms
/// native (about twice that in a desktop browser), so the 27,656-vertex
/// The proof figure takes about 60 frames (a second at 60 fps) and no frame
/// stalls on it (in one go it is 70 ms native, 115 to 135 in a browser).
pub const FRAME_UNITS: u64 = 120_000;

/// A part's model-space positions and normals ([`model_space`]).
type Space = (Vec<[f32; 3]>, Vec<[f32; 3]>);

/// Where a [`Bake`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Marking the cells under triangle `tri` of part `part`.
    Mark { part: usize, tri: usize },
    /// The depth map of direction `dir` of vertex set `set` (one set per LOD
    /// level present, or one for a model without levels).
    Dir { set: usize, dir: usize, sub: usize }, #[cfg(feature = "sun-exact")] Refine { at: usize }, #[cfg(feature = "sun-exact")] Smooth,
    /// The cone of vertex `vertex` of part `part` (`mask`: its index over
    /// the whole model).
    Cone { part: usize, vertex: usize, mask: usize },
    Done,
}

/// The cone bake of one model in steps, so it can be spread over frames
/// ([`Self::step`]) and run only where a tier needs it (no shadow map):
/// the same bytes as [`bake`] however it is split. `parts` must be the
/// same, unchanged, at every step.
pub struct Bake {
    spaces: Vec<Space>,
    grid: Grid,
    dirs: [[f32; 3]; DIRS],
    from: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    marked: Vec<[f32; 3]>,
    masks: Vec<u64>,
    map: Vec<f32>,
    /// Where each part's cells go and which set its vertices belong to.
    place: Vec<Place>,
    /// How many vertices each set holds.
    set_verts: Vec<usize>,
    /// The corners of big triangles vote from inside them, in order of the
    /// first copy of the vertex they speak for; none for a model of small
    /// triangles.
    votes: Vec<Vote>,
    /// Each vertex's first copy (see [`votes`]).
    first: Vec<u32>,
    /// Per first copy, the end of its votes.
    ends: Vec<u32>,
    /// Each vertex's surface byte (the first 254 surfaces in vertex order;
    /// [`MANY`] for the rest), empty for a model of one surface; how many
    /// points the set splats into the near field's map, and the map.
    code: Vec<u8>,
    points: usize,
    other: Vec<Other>,
    /// The set's sources of points, a step each ([`Src`]), the big
    /// triangles (part, triangle, points) they read, and the current
    /// direction's map layout.
    sources: Vec<Src>,
    big: Vec<(u32, u32, u32)>,
    layout: Layout,
    /// The work units the last [`Self::step`] did (for the cost test: one
    /// item, a step of 1, must fit a frame).
    last_units: u64, #[cfg(feature = "sun-exact")] tri_base: Vec<u32>, #[cfg(feature = "sun-exact")] shut: Vec<Shut>, #[cfg(feature = "sun-exact")] coarse: bool, #[cfg(feature = "sun-exact")] smoother: Smoother, #[cfg(feature = "sun-exact")] smoothed: bool,
    stage: Stage,
}

/// One vote for the cone of the vertex `vertex` (its index over the whole
/// model): the open bits `mask` seen from `from` (a ray start inside a big
/// triangle) or, without `from`, the vertex's own, counted `weight` (a third
/// of the triangles' area it stands for). Set `set` holds the vertex (read
/// only with `from`).
#[derive(Clone, Copy)]
struct Vote {
    vertex: usize,
    set: usize,
    from: Option<[f32; 3]>,
    /// The face's normal (turned to the vertex's side): the vote speaks for
    /// the directions in front of it.
    normal: [f32; 3],
    weight: f32,
    mask: u64,
}

/// A triangle's corners, a third of its area, its longest edge and its face normal.
type Tri = ([[f32; 3]; 3], f32, f32, [f32; 3]);

/// A triangle's corners, a third of its area, its longest edge and its face
/// normal; `None` for an index past the positions.
fn tri(pos: &[[f32; 3]], t: &[u32; 3]) -> Option<Tri> {
    let c = [*pos.get(t[0] as usize)?, *pos.get(t[1] as usize)?, *pos.get(t[2] as usize)?];
    let edge = |i: usize, j: usize| [0, 1, 2].map(|k| c[j][k] - c[i][k]);
    let (ab, bc, ca) = (edge(0, 1), edge(1, 2), edge(2, 0));
    let x = cross(ab, bc);
    Some((c, dot(x, x).sqrt() / 6.0, dot(ab, ab).max(dot(bc, bc)).max(dot(ca, ca)).sqrt(), normalize(x)))
}

/// [`Firsts`]: each vertex's first copy: the lowest-index earlier first copy in the same
/// set whose model-space position is within `tol` on every axis and whose
/// normal is the same to a thousandth, else itself. Each part packs its
/// positions to 16 bits over its own box, so one point carried by two parts
/// reads back up to a step apart; `tol` is well over a step. Found by hash
/// buckets of cells `2 tol` wide laid out by counting, in index order
/// within a bucket (no sort or map: they cost kilobytes of download); a
/// point within `tol` lies in one of 8 cells.
/// Found in steps (ov1, 2026-11-02): the buckets are laid out at once
/// (two hashes a vertex), then the copies are looked up a share of the
/// vertices a step, in index order, so the bake's setup spreads over frames.
/// The same slices must be passed at every step.
struct Firsts {
    tol: f32,
    inv: f32,
    mask: usize,
    ends: Vec<u32>,
    order: Vec<u32>,
    first: Vec<u32>,
    /// The next vertex to look up.
    at: usize,
}

impl Firsts {
    fn new(pos: &[[f32; 3]], nrm: &[[f32; 3]], set: &[u32], tol: f32) -> Self {
        let n = pos.len().min(nrm.len()).min(set.len());
        let mask = n.next_power_of_two() - 1;
        let mut f = Self { tol, inv: 0.5 / tol, mask, ends: vec![0u32; mask + 2], order: vec![0u32; n], first: vec![0u32; n], at: 0 };
        for i in 0..n {
            let h = f.hash(set[i], pos[i]);
            f.ends[h + 1] += 1;
        }
        for i in 1..f.ends.len() {
            f.ends[i] += f.ends[i - 1];
        }
        let mut at = f.ends.clone();
        for i in 0..n {
            let h = f.hash(set[i], pos[i]);
            f.order[at[h] as usize] = i as u32;
            at[h] += 1;
        }
        f
    }

    fn hash(&self, s: u32, p: [f32; 3]) -> usize {
        let inv = self.inv;
        [s, (p[0] * inv).floor() as i32 as u32, (p[1] * inv).floor() as i32 as u32, (p[2] * inv).floor() as i32 as u32].iter().fold(0x811c_9dc5u32, |h, x| (h ^ x).wrapping_mul(0x0100_0193)) as usize & self.mask
    }

    /// Up to `max` more vertices looked up; true once every one is.
    fn step(&mut self, pos: &[[f32; 3]], nrm: &[[f32; 3]], set: &[u32], max: usize) -> bool {
        let n = self.first.len();
        let tol = self.tol;
        let near = |a: [f32; 3], b: [f32; 3], t: f32| (0..3).all(|k| (a[k] - b[k]).abs() <= t);
        let end = self.at.saturating_add(max).min(n);
        for i in self.at..end {
            let mut best = i;
            for c in 0..8 {
                let h = self.hash(set[i], [0, 1, 2].map(|k| pos[i][k] + if (c >> k) & 1 == 0 { -tol } else { tol }));
                for &j in &self.order[self.ends[h] as usize..self.ends[h + 1] as usize] {
                    let j = j as usize;
                    if j >= best {
                        break;
                    }
                    if self.first[j] as usize == j && set[j] == set[i] && near(pos[j], pos[i], tol) && near(nrm[j], nrm[i], 1e-3) {
                        best = j;
                        break;
                    }
                }
            }
            self.first[i] = best as u32;
        }
        self.at = end;
        end >= n
    }
}

/// The votes of a model's big triangles (see the module notes), in order of
/// the vertex they speak for: for each corner of a triangle with an edge
/// over [`COARSE`] cells, one from the middle of its share (corner weights
/// 1/2, 1/4, 1/4), and, for a vertex with any, one from the vertex itself
/// for its small triangles. A point's copies (the same set and normal, and
/// the same position to a 64th of a cell: a model split into parts carries
/// one per part, each packed over its part's box) share the first copy's
/// votes ([`firsts`]), so how a model is split does not change its shade.
/// Also each vertex's first copy and, per first copy, the end of its votes.
/// All empty when every triangle is small.
/// In steps ([`VoteSteps`]): the first copies a share at a time, then one
/// part's triangles a step, then the placing.
struct VoteSteps {
    pos: Vec<[f32; 3]>,
    set: Vec<u32>,
    f: Firsts,
    part: usize,
    votes: Vec<Vote>,
    /// Per first copy: a third of its small triangles' area (its own vote),
    /// and how many votes it gets.
    small: Vec<f32>,
    start: Vec<u32>,
}

impl VoteSteps {
    fn new(spaces: &[Space], place: &[Place], grid: &Grid, normals: &[[f32; 3]]) -> Self {
        let pos: Vec<[f32; 3]> = spaces.iter().flat_map(|s| s.0.iter().copied()).collect();
        let set: Vec<u32> = place.iter().flat_map(|pl| pl.range.clone().map(|_| pl.set as u32)).collect();
        let f = Firsts::new(&pos, normals, &set, grid.cell / 64.0);
        Self { pos, set, f, part: 0, votes: Vec::new(), small: vec![0.0f32; normals.len()], start: vec![0u32; normals.len()] }
    }

    /// About `left` work units more ([`INIT_UNITS`]), taken off it; the
    /// votes, first copies and ends once done.
    fn step(&mut self, parts: &[Part], spaces: &[Space], place: &[Place], grid: &Grid, normals: &[[f32; 3]], left: &mut u64) -> Option<(Vec<Vote>, Vec<u32>, Vec<u32>)> {
        if self.f.at < self.f.first.len() {
            let n = (*left / INIT_UNITS.firsts).max(256) as usize;
            let before = self.f.at;
            self.f.step(&self.pos, normals, &self.set, n);
            *left = left.saturating_sub((self.f.at - before) as u64 * INIT_UNITS.firsts);
            return None;
        }
        let first = &self.f.first;
        let (votes, small, start) = (&mut self.votes, &mut self.small, &mut self.start);
        if let (Some(p), Some((pos, _)), Some(pl)) = (parts.get(self.part), spaces.get(self.part), place.get(self.part)) {
            self.part += 1;
            *left = left.saturating_sub((p.indices.len() / 3) as u64 * INIT_UNITS.tri);
            vote_part(p, pos, pl, grid, normals, first, votes, small, start);
            return None;
        }
        Some(finish_votes(core::mem::take(votes), core::mem::take(&mut self.f.first), small, core::mem::take(start), normals))
    }
}

/// One part's triangles into the votes (see [`VoteSteps`]).
#[allow(clippy::too_many_arguments)]
fn vote_part(p: &Part, pos: &[[f32; 3]], pl: &Place, grid: &Grid, normals: &[[f32; 3]], first: &[u32], votes: &mut Vec<Vote>, small: &mut [f32], start: &mut [u32]) {
    {
        for t in p.indices.as_chunks::<3>().0 {
            let Some((c, third, longest, f)) = tri(pos, t) else { continue };
            for k in 0..3 {
                let Some(vertex) = first.get(pl.range.start + t[k] as usize).map(|v| *v as usize) else { continue };
                if longest <= COARSE * grid.cell {
                    if let Some(w) = small.get_mut(vertex) {
                        *w += third;
                    }
                    continue;
                }
                let h = [0, 1, 2].map(|r| 0.5 * c[k][r] + 0.25 * (c[(k + 1) % 3][r] + c[(k + 2) % 3][r]));
                let n = normals.get(vertex).copied().unwrap_or_default();
                let normal = if dot(f, n) < 0.0 { f.map(|x| -x) } else { f };
                // Only a face the vertex shades as itself speaks for it: a
                // cap that shares a cylinder's side normals faces 90 degrees
                // away from them, and its sky is not the vertex's.
                if dot(normal, n) < VOTE_COS {
                    continue;
                }
                votes.push(Vote { vertex, set: pl.set, from: Some(grid.start(h, normal)), normal, weight: third, mask: 0 });
                if let Some(s) = start.get_mut(vertex) {
                    *s += 1;
                }
            }
        }
    }
}

/// The votes' end (see [`VoteSteps`]): each vertex's own vote, then the
/// placing into vertex order.
fn finish_votes(mut votes: Vec<Vote>, first: Vec<u32>, small: &[f32], mut start: Vec<u32>, normals: &[[f32; 3]]) -> (Vec<Vote>, Vec<u32>, Vec<u32>) {
    if votes.is_empty() {
        return (votes, Vec::new(), Vec::new());
    }
    for (vertex, (w, s)) in small.iter().zip(&mut start).enumerate() {
        if *s > 0 && *w > 0.0 {
            *s += 1;
            votes.push(Vote { vertex, set: 0, from: None, normal: normals.get(vertex).copied().unwrap_or_default(), weight: *w, mask: 0 });
        }
    }
    let mut at = 0;
    for s in &mut start {
        (*s, at) = (at, at + *s);
    }
    // Into vertex order, keeping the order made within a vertex (a placing,
    // not a sort: a sort costs kilobytes of download).
    let mut sorted = votes.clone();
    for v in &votes {
        if let Some(s) = start.get_mut(v.vertex) {
            if let Some(o) = sorted.get_mut(*s as usize) {
                *o = *v;
            }
            *s += 1;
        }
    }
    // Each first copy's cursor ended where its votes end.
    (sorted, first, start)
}

/// A part's place in the bake: the [`Grid`] layer it marks, the set of
/// vertices whose depth maps its vertices read (set `k` is the k-th LOD level
/// present; shared parts are in set 0, the lowest level), and its vertices'
/// range over the whole model.
struct Place {
    layer: usize,
    set: usize,
    range: core::ops::Range<usize>,
}

impl Bake {
    /// A bake of `parts`; nothing is marked yet. The grid holds the whole
    /// model, so one part shades another (a hat brim on a face), but only
    /// among the parts drawn together (see the module notes on LOD levels).
    pub fn new(parts: &[Part]) -> Self {
        // The stepped setup ([`Init`]) run to the end.
        let mut init = Init::new(parts);
        loop {
            if let Some(b) = init.step(parts, u64::MAX) {
                return b;
            }
        }
    }

    /// The directions vertex `vertex` (normal `n`, own open bits `own`)
    /// knows about and the open ones among them: its own, or for a corner of
    /// big triangles the weighted majority, direction by direction, of the
    /// votes that face it (its first copy's).
    fn voted(&self, vertex: usize, n: [f32; 3], own: u64) -> (u64, u64) {
        let c = self.first.get(vertex).map_or(usize::MAX, |c| *c as usize);
        let start = c.checked_sub(1).and_then(|p| self.ends.get(p)).map_or(0, |s| *s as usize);
        let end = self.ends.get(c).map_or(0, |e| *e as usize);
        let votes = self.votes.get(start..end).unwrap_or_default();
        if votes.is_empty() {
            return (front(n, &self.dirs), own);
        }
        let (mut seen, mut out) = (0, 0);
        for (j, d) in self.dirs.iter().enumerate() {
            let (mut total, mut open) = (0.0f32, 0.0f32);
            for v in votes.iter().filter(|v| dot(v.normal, *d) > 0.0) {
                total += v.weight;
                if (v.from.map_or(own, |_| v.mask) >> j) & 1 == 1 {
                    open += v.weight;
                }
            }
            if total > 0.0 {
                seen |= 1 << j;
                // A tie (equal faces of a symmetric mesh, half open and half
                // blocked) is open, as an exact tie always was, within a
                // margin, so how the areas round (that differs with how a
                // file packs its positions) cannot flip it.
                if 2.0 * open - total >= -TIE * total {
                    out |= 1 << j;
                }
            } else if dot(n, *d) > 0.0 {
                // No face voted on a direction in front of the vertex: its
                // own ray says.
                seen |= 1 << j;
                out |= own & (1 << j);
            }
        }
        (seen, out)
    }

    /// Collects the occluders of vertex set `set` (the shared cells and its
    /// level's) before its depth maps; returns the work it cost.
    fn enter(&mut self, parts: &[Part], set: usize) -> u64 {
        // A model without levels has the one layer; the shared one.
        self.marked = self.grid.marked(Some(set + 1));
        // Collecting the cells reads the whole grid once.
        let mut work = self.grid.occ[0].len() as u64 / 16;
        // The near field's points, in steps of about [`CHUNK`]: the vertices
        // of the parts drawn with the set, and the big triangles' inner points.
        (self.points, self.sources, self.big) = (0, Vec::new(), Vec::new());
        if self.code.is_empty() {
            return work;
        }
        let gap = SAMPLE * self.grid.cell;
        for (pi, ((p, (pos, _)), _)) in parts.iter().zip(&self.spaces).zip(&self.place).enumerate().filter(|(_, (_, pl))| pl.layer == 0 || pl.layer == set + 1) {
            for lo in (0..pos.len()).step_by(CHUNK) {
                let hi = (lo + CHUNK).min(pos.len());
                self.sources.push(Src { tris: false, part: pi as u32, lo: lo as u32, hi: hi as u32, n: (hi - lo) as u32 });
            }
            work += p.indices.len() as u64 / 3;
            for (ti, t) in p.indices.as_chunks::<3>().0.iter().enumerate() {
                let Some((c, _, longest, _)) = tri(pos, t) else { continue };
                if longest <= gap {
                    continue;
                }
                let n = inner_count(c, gap);
                let k = self.big.len() as u32;
                match self.sources.last_mut() {
                    Some(src) if src.tris && src.hi == k && (src.n + n) as usize <= CHUNK => {
                        src.hi += 1;
                        src.n += n;
                    }
                    _ => self.sources.push(Src { tris: true, part: 0, lo: k, hi: k + 1, n }),
                }
                self.big.push((pi as u32, ti as u32, n));
            }
        }
        self.points = self.sources.iter().map(|s| s.n as usize).sum();
        work + self.big.len() as u64
    }

    /// Splats source `k` of the set's points into the current direction's
    /// near field map (see [`Other`]); returns the work.
    fn splat_source(&mut self, parts: &[Part], d: [f32; 3], k: usize) -> u64 {
        let Some(src) = self.sources.get(k).copied() else { return 1 };
        let Layout { u, v, u0, v0, w, h } = self.layout;
        let inv = self.grid.inv;
        let (code, other) = (&self.code, &mut self.other);
        let mut splat = |q: [f32; 3], s: u8| {
            let (cu, cv) = ((dot(q, u) - u0) * inv, (dot(q, v) - v0) * inv);
            let depth = dot(q, d);
            let row = |x: f32, n: usize| x.floor().clamp(0.0, (n - 1) as f32) as usize;
            for tv in row(cv - NEAR_SPLAT, h)..=row(cv + NEAR_SPLAT, h) {
                for t in other.get_mut(tv * w + row(cu - NEAR_SPLAT, w)..=tv * w + row(cu + NEAR_SPLAT, w)).unwrap_or_default() {
                    t.add(depth, s);
                }
            }
        };
        if !src.tris {
            let (Some((pos, _)), Some(pl)) = (self.spaces.get(src.part as usize), self.place.get(src.part as usize)) else { return 1 };
            for i in src.lo as usize..src.hi as usize {
                if let Some(q) = pos.get(i) {
                    splat(*q, code.get(pl.range.start + i).copied().unwrap_or(MANY));
                }
            }
        } else {
            let gap = SAMPLE * self.grid.cell;
            for (pi, ti, _) in self.big.get(src.lo as usize..src.hi as usize).unwrap_or_default() {
                let (Some(p), Some((pos, _)), Some(pl)) = (parts.get(*pi as usize), self.spaces.get(*pi as usize), self.place.get(*pi as usize)) else { continue };
                let Some(t) = p.indices.as_chunks::<3>().0.get(*ti as usize) else { continue };
                let Some((c, ..)) = tri(pos, t) else { continue };
                let s = code.get(pl.range.start + t[0] as usize).copied().unwrap_or(MANY);
                inner_points(c, gap, |q| splat(q, s));
            }
        }
        u64::from(src.n)
    }

    /// Sets bit `j` of `masks[i]` of the vertices of set `set` where
    /// `dirs[j]` is in front of `normals[i]` and open from `from[i]` (no
    /// marked cell lies ahead along it). One small depth map per direction,
    /// as a shadow map is
    /// made: every marked cell's centre is splatted where it
    /// falls across the direction (a texel is a cell, plus [`SPLAT`] of a
    /// cell either side, about the width of a cell seen at an angle)
    /// keeping the furthest along it, and a point is open when nothing in
    /// its texel lies further along than it does. That is 64 splats of the
    /// model's cells instead of a ray walk per vertex and direction.
    fn dir_map(&mut self, j: usize) {
        let d = self.dirs[j];
        let g = &self.grid;
        let hi = [0, 1, 2].map(|k| g.lo[k] + g.dim[k] as f32 * g.cell);
        let side = if d[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let u = normalize(cross(d, side));
        let v = cross(d, u);
        // The map covers the grid box seen along `d`, its texels laid from
        // the drawn level's box (grown by whole texels), as the cells are.
        let seen = |lo: [f32; 3], hi: [f32; 3]| {
            let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for c in 0..8 {
                let p = [0, 1, 2].map(|k| if (c >> k) & 1 == 0 { lo[k] } else { hi[k] });
                (u0, u1, v0, v1) = (u0.min(dot(p, u)), u1.max(dot(p, u)), v0.min(dot(p, v)), v1.max(dot(p, v)));
            }
            (u0, v0, u1, v1)
        };
        let (vu, vv, ..) = seen(g.view[0], g.view[1]);
        let (u0, v0, u1, v1) = seen(g.lo, hi);
        let (u0, v0) = (vu - ((vu - u0) * g.inv).ceil() * g.cell, vv - ((vv - v0) * g.inv).ceil() * g.cell);
        let w = ((u1 - u0) * g.inv).ceil() as usize + 2;
        let h = ((v1 - v0) * g.inv).ceil() as usize + 2;
        self.layout = Layout { u, v, u0, v0, w, h };
        let (inv, marked, map) = (g.inv, &self.marked, &mut self.map);
        map.clear();
        map.resize(w * h, f32::MIN);
        let clamp = |x: f32, n: usize| x.floor().clamp(0.0, (n - 1) as f32) as usize;
        for c in marked {
            let (cu, cv) = ((dot(*c, u) - u0) * inv, (dot(*c, v) - v0) * inv);
            let depth = dot(*c, d);
            for tv in clamp(cv - SPLAT, h)..=clamp(cv + SPLAT, h) {
                for t in &mut map[tv * w + clamp(cu - SPLAT, w)..=tv * w + clamp(cu + SPLAT, w)] {
                    *t = t.max(depth);
                }
            }
        }
        // The near field's map (see the module notes), when the model has
        // more than one surface: filled a source a step ([`Self::splat_source`]).
        self.other.clear();
        if !self.code.is_empty() {
            self.other.resize(w * h, Other::NONE);
        }
    }

    /// With the direction's maps made ([`Self::dir_map`], then every
    /// source), sets bit `j` of `masks[i]` of the vertices of set `set`
    /// where `dirs[j]` is in front of `normals[i]` and open from `from[i]`
    /// (and the same for the votes).
    fn dir_test(&mut self, set: usize, j: usize) {
        let d = self.dirs[j];
        let Layout { u, v, u0, v0, w, h } = self.layout;
        let (cell, inv) = (self.grid.cell, self.grid.inv);
        let at = |p: [f32; 3]| ((dot(p, u) - u0) * inv, (dot(p, v) - v0) * inv);
        let clamp = |x: f32, n: usize| x.floor().clamp(0.0, (n - 1) as f32) as usize;
        let (from, normals, masks, map, code, other) = (&self.from, &self.normals, &mut self.masks, &self.map, &self.code, &self.other);
        // Open from `f` (a ray start [`START`] cells off a surface along
        // `n`, of surface byte `s`): nothing marked lies further along in its
        // texel, nor another surface's point further along than its near
        // start. A texel is a cell wide, so a surface just under the start
        // reaches past it in the texel when the ray leans: by up to the
        // texel's half width (and the points' splat) times the lean's
        // tangent, the slope bias a shadow map takes too.
        let back = (START - NEAR_N) * cell;
        let open = |f: [f32; 3], n: [f32; 3], s: Option<&u8>| {
            let (fu, fv) = at(f);
            if map.get(clamp(fv, h) * w + clamp(fu, w)).is_some_and(|m| *m > dot(f, d)) {
                return false;
            }
            let (Some(s), false) = (s, other.is_empty()) else { return true };
            let o = [0, 1, 2].map(|k| f[k] - n[k] * back + d[k] * NEAR_D * cell);
            let c = dot(n, d).max(1e-3);
            let bias = ((0.5 + NEAR_SPLAT) * cell * (1.0 - c * c).max(0.0).sqrt() / c).min(BIAS_CAP * cell);
            let (ou, ov) = at(o);
            other.get(clamp(ov, h) * w + clamp(ou, w)).is_none_or(|t| t.beyond(*s) <= dot(o, d) + bias)
        };
        for r in self.place.iter().filter(|p| p.set == set).map(|p| p.range.clone()) {
            for (i, ((m, f), n)) in masks[r.clone()].iter_mut().zip(&from[r.clone()]).zip(&normals[r.clone()]).enumerate().filter(|x| dot(*x.1.1, d) > 0.0) {
                if open(*f, *n, code.get(r.start + i)) {
                    *m |= 1 << j;
                }
            }
        }
        // The votes from inside big triangles, read the same way.
        for vote in self.votes.iter_mut().filter(|v| v.set == set) {
            if let Some(f) = vote.from
                && dot(vote.normal, d) > 0.0
                && open(f, vote.normal, code.get(vote.vertex))
            {
                vote.mask |= 1 << j;
            }
        }
    }

    /// The bytes the bake holds right now beyond the model (its scratch:
    /// grid, cells, depth map, ray starts, masks, votes), for the cost test
    /// (`tests/sun_bake_cost.rs`).
    pub fn scratch_bytes(&self) -> usize {
        use core::mem::size_of;
        let p3 = size_of::<[f32; 3]>();
        let spaces: usize = self.spaces.iter().map(|s| (s.0.capacity() + s.1.capacity()) * p3).sum();
        let occ: usize = self.grid.occ.iter().map(Vec::capacity).sum();
        spaces
            + occ
            + (self.from.capacity() + self.normals.capacity() + self.marked.capacity()) * p3
            + self.masks.capacity() * size_of::<u64>()
            + self.map.capacity() * size_of::<f32>()
            + self.votes.capacity() * size_of::<Vote>()
            + (self.first.capacity() + self.ends.capacity()) * size_of::<u32>()
            + self.code.capacity()
            + self.sources.capacity() * size_of::<Src>()
            + self.big.capacity() * size_of::<(u32, u32, u32)>()
            + self.other.capacity() * size_of::<Other>()
    }

    /// The grid's cell, in model units (for the cost test).
    pub fn cell(&self) -> f32 {
        self.grid.cell
    }

    /// The work units the last [`Self::step`] did.
    pub fn last_units(&self) -> u64 {
        self.last_units
    }

    /// Whether every cone is written.
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// About `units` of work (at least one item: a triangle, a direction
    /// or a vertex), writing each cone into `parts`' spare vertex bytes as
    /// it is found; true once all are written.
    pub fn step(&mut self, parts: &mut [Part], units: u64) -> bool {
        let (done, spent) = self.advance(parts, units);
        self.last_units = spent;
        done
    }

    /// [`Self::step`], and the work it did.
    fn advance(&mut self, parts: &mut [Part], units: u64) -> (bool, u64) {
        let mut spent = 0u64;
        let mut first = true;
        // Each item checks the budget before it starts, so the first one
        // always runs and the bake always moves on.
        let mut go = |cost: u64, spent: &mut u64| {
            let ok = first || *spent < units;
            first = false;
            if ok {
                *spent = spent.saturating_add(cost);
            }
            ok
        };
        loop {
            match self.stage {
                Stage::Done => return (true, spent),
                Stage::Mark { part, tri } => {
                    let Some(p) = parts.get(part) else {
                        #[cfg(feature = "sun-exact")] self.grid.exact_index(); spent = spent.saturating_add(self.enter(parts, 0));
                        self.stage = Stage::Dir { set: 0, dir: 0, sub: 0 };
                        continue;
                    };
                    let Some(t) = p.indices.as_chunks::<3>().0.get(tri) else {
                        self.stage = Stage::Mark { part: part + 1, tri: 0 };
                        continue;
                    };
                    if !go(0, &mut spent) {
                        return (false, spent);
                    }
                    let pos = &self.spaces[part].0;
                    let at = |i: u32| pos.get(i as usize).copied().unwrap_or_default();
                    let layer = self.place.get(part).map_or(0, |p| p.layer);
                    spent = spent.saturating_add(MARK_UNITS * self.grid.mark(layer, [at(t[0]), at(t[1]), at(t[2])])); #[cfg(feature = "sun-exact")] { let id = self.tri_base.get(part).copied().unwrap_or(0) + tri as u32; spent = spent.saturating_add(MARK_UNITS * self.grid.note([at(t[0]), at(t[1]), at(t[2])], id)); }
                    self.stage = Stage::Mark { part, tri: tri + 1 };
                }
                Stage::Dir { set, dir, sub } => {
                    if dir == DIRS {
                        // The set is done: its own layer goes now.
                        if let Some(o) = self.grid.occ.get_mut(set + 1) {
                            *o = Vec::new();
                        }
                        if set + 1 < self.set_verts.len() {
                            spent = spent.saturating_add(self.enter(parts, set + 1));
                            self.stage = Stage::Dir { set: set + 1, dir: 0, sub: 0 };
                        } else {
                            self.marked = Vec::new();
                            (self.points, self.other, self.sources, self.big) = (0, Vec::new(), Vec::new(), Vec::new());
                            self.stage = Stage::Cone { part: 0, vertex: 0, mask: 0 };
                        }
                        continue;
                    }
                    // Per direction: the maps' layout and the marked cells,
                    // then a step per source of the near field's points, then
                    // the vertices and votes.
                    let sources = self.sources.len();
                    let cost = if sub == 0 {
                        self.marked.len()
                    } else if sub <= sources {
                        self.sources.get(sub - 1).map_or(1, |s| s.n as usize)
                    } else {
                        self.set_verts.get(set).copied().unwrap_or(0) + self.votes.len()
                    };
                    if !go(cost as u64, &mut spent) {
                        return (false, spent);
                    }
                    if sub == 0 {
                        self.dir_map(dir);
                    } else if sub <= sources {
                        self.splat_source(parts, self.dirs[dir], sub - 1);
                    } else {
                        #[cfg(not(feature = "sun-exact"))] self.dir_test(set, dir); #[cfg(feature = "sun-exact")] { self.dir_test(set, dir); self.note_shut(set, dir); } // the viewer casts the shut rays exactly later (see the end of the file)
                        self.stage = Stage::Dir { set, dir: dir + 1, sub: 0 };
                        continue;
                    }
                    self.stage = Stage::Dir { set, dir, sub: sub + 1 };
                } #[cfg(feature = "sun-exact")] Stage::Refine { at } => { if self.shut.get(at).is_none() { (self.stage, self.smoothed) = (Stage::Cone { part: 0, vertex: 0, mask: 0 }, false); continue; } else if !go(0, &mut spent) { return (false, spent); } spent = spent.saturating_add(self.refine(parts, at)); self.stage = Stage::Refine { at: at + 1 }; } #[cfg(feature = "sun-exact")] Stage::Smooth => { if !go(SMOOTH_UNITS, &mut spent) || self.smoother.chunk(parts) && self.smoothed_now() { return (false, spent); } } // the viewer's exact casts, a ray at a time, and its smoothing, a chunk at a time (see the end of the file)
                Stage::Cone { part, vertex, mask } => {
                    let Some(p) = parts.get_mut(part) else { #[cfg(feature = "sun-exact")] if self.after_cones() { continue; } // the viewer smooths the cones first (see the end of the file)
                        // Done: the scratch goes now, not with the bake.
                        *self = Self { stage: Stage::Done, ..Self::new(&[]) };
                        return (true, spent);
                    };
                    let Some(v) = p.vertices.as_chunks_mut::<VERTEX_BYTES>().0.get_mut(vertex) else {
                        self.stage = Stage::Cone { part: part + 1, vertex: 0, mask };
                        continue;
                    };
                    if !go(CONE_UNITS, &mut spent) {
                        return (false, spent);
                    }
                    let n = self.spaces[part].1.get(vertex).copied().unwrap_or_default();
                    let (seen, open) = self.voted(mask, n, self.masks.get(mask).copied().unwrap_or(u64::MAX));
                    let (axis, t) = Grid::cone_over(n, seen, open, &self.dirs);
                    // Back to the part's own space. The shader turns it like
                    // a normal (cofactor x sign = |det| x inverse transpose),
                    // so the inverse of that is the plain transpose of the
                    // node's 3x3.
                    let m = p.node;
                    let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
                    let local = normalize([0, 1, 2].map(|k| dot(c[k], axis)));
                    let l = 0.5 * (1.0 + t);
                    [v[6], v[7], v[11]] = local.map(|x| super::snorm8(x * l));
                    self.stage = Stage::Cone { part, vertex: vertex + 1, mask: mask + 1 };
                }
            }
        }
    }
}

/// One uploaded model's cones on the GPU side: its parts (vertices and
/// indices) stay on the CPU until they are baked, so a tier that drops to
/// one without a shadow map mid-game can still bake them.
pub struct Lazy {
    parts: Vec<Part>,
    bake: Option<Box<Bake>>, #[cfg(feature = "sun-exact")] fade: Fade,
    /// The bake's setup while it runs over frames ([`Init`]).
    init: Option<Box<Init>>,
}

impl Lazy {
    pub fn new(parts: Vec<Part>) -> Self {
        Self { parts, bake: None, #[cfg(feature = "sun-exact")] fade: Fade::default(), init: None }
    }

    /// Whether the cones are in (the CPU copy is gone).
    #[cfg(not(feature = "sun-exact"))] pub fn baked(&self) -> bool {
        self.parts.is_empty()
    }

    /// About `units` more of the bake. The frame it finishes, the baked
    /// parts are handed back (to write over the vertex buffers) and the
    /// CPU copy is dropped.
    #[cfg(not(feature = "sun-exact"))] pub fn advance(&mut self, units: u64) -> Option<Vec<Part>> {
        if self.baked() {
            return None;
        }
        if !self.set_up(units) {
            return None;
        }
        let b = self.bake.as_deref_mut()?;
        if !b.step(&mut self.parts, units) {
            return None;
        }
        self.bake = None;
        Some(core::mem::take(&mut self.parts))
    }
}

/// Which model's bake moves on this frame, of models given in load order
/// as `(baked, waiting to show, drawn this frame)`: none when the frame has
/// a shadow map (`cones_read` false: the cones are not read, nothing is
/// spent); otherwise the first unbaked one that is waiting or drawn, one a
/// frame, so a frame never pays for more than [`FRAME_UNITS`].
pub fn pick(cones_read: bool, models: impl IntoIterator<Item = (bool, bool, bool)>) -> Option<usize> {
    if !cones_read {
        return None;
    }
    models.into_iter().position(|(baked, waiting, drawn)| !baked && (waiting || drawn))
}

/// Bakes the cone of every vertex of `parts` into its spare bytes (see the
/// module notes) in one go: what [`Bake`] does over several frames.
pub fn bake(parts: &mut [Part]) {
    let mut b = Bake::new(parts);
    while !b.step(parts, u64::MAX) {}
}

/// The cone the shader reads from a vertex's bytes: `(direction in the
/// part's space, threshold)`; as model.wgsl decodes it.
pub fn decode(v: &[u8]) -> ([f32; 3], f32) {
    let c = [v[6], v[7], v[11]].map(|b| f32::from(b as i8) / 127.0);
    let l = dot(c, c).sqrt();
    (normalize(c), 2.0 * l - 1.0)
}

// Exact sun casts (feature `sun-exact`, the material viewer's; critic round
// 4 of material-view, 2026-10-27: torn dark patches on Low under the shader
// ball's cap, along its cup's rim and on its foot). The depth maps hold whole
// grid cells splatted from their centres (up to 0.9 cell off the surface, a
// texel a cell wide), and a ray starts [`START`] cells off its surface, so
// beside a lip or rim a cell or two away a ray that passes it reads blocked,
// and a corner whose start lands among a neighbour's cells reads blocked every
// way. Measured against exact casts (model_wgsl
// `close_surfaces_do_not_tear_the_baked_sun`), nine in ten wrongly dark
// corners on the ball had their blocker within 2 cells of the ray's start.
// The games' models are tuned against the maps as they are (the Frontier
// Marshal's lit share sits within a point of its limits, and every way of
// skipping the near range lit it past them; its cloth 1 to 3 cm over its coat
// is a real blocker), so this is opt-in: a ray the depth map shuts is cast
// again exactly from the surface point, the viewer's shadow map's own offset
// off ([`MAP_N`] along the normal, [`MAP_D`] along the ray), through the
// grid cells it crosses (each cell's triangles listed while it is marked),
// and is open if that cast finds it so; so is a ray a close layer shuts.
// Without the feature none of it compiles and every model bakes the bytes it
// did. It is appended here, and hooked in on the lines it changes, so no line
// of the shipped bake moves (a ship build carries panic locations).
//
// Time-sliced (2026-10-27): cast inside each direction's test, the exact
// casts ran over a whole vertex set in one step, after the budget was
// checked, so a Lite to Low switch stopped the viewer's frames 332 ms once
// and drew unsteadily for 15 s (the bust's Low bake: one 88 ms step native).
// Now a direction's test is the shipped one and notes the rays it shuts
// ([`Shut`]); the coarse cones are written, smoothed and shown
// ([`Lazy::advance`] hands them back), then each shut ray is cast exactly as
// its own item of the budget ([`Stage::Refine`]), the cones are written
// again from the opened masks and the model fades from the coarse cones to
// them over [`FADE_FRAMES`]; the smoothing, which took 10 ms at once on
// the bust, runs a chunk at a time too ([`Smoother`]), and the viewer gives
// its bake a smaller share of the frame ([`VIEW_SHARE`]). Open is
// `coarse || exact` either way, so the
// final bytes are the ones the casts in the test made. A cast walks only the
// samples inside the grid (the rest find no triangles) and counts them with
// its triangles ([`EXACT_TRI`], [`EXACT_DIV`]).

/// The exact casts' samples along a ray, in cells: the grid cells it crosses.
#[cfg(feature = "sun-exact")]
const WALK: f32 = 0.25;

/// The exact casts start at least this far off the surface along its normal
/// and along the ray (model units, metres at scale 1): the shadow map's own
/// lookup offset ([`crate::shadow::NORMAL_OFFSET`], 6 cm) and depth bias
/// ([`crate::shadow::BIAS`] of the light box's 160 m, 1.6 cm), so Low draws
/// lit what the viewer's Ultra map lights (the owner's call, 2026-10-27: Low
/// may lose fine relief but reads as the same material). Under the shader
/// ball's cap and on its foot the 1% cast still met the cap's lip and the
/// bowl; Ultra's map starts past them.
#[cfg(feature = "sun-exact")]
const MAP_N: f32 = crate::shadow::NORMAL_OFFSET;
#[cfg(feature = "sun-exact")]
const MAP_D: f32 = crate::shadow::BIAS * 2.0 * crate::shadow::DEPTH;

/// Weight of a triangle tested against a sample walked in an exact cast,
/// and the cast's work per work unit of [`Bake::step`] (measured natively on
/// the viewer's bust and shader ball, `tests/sun_exact_chunks.rs`).
#[cfg(feature = "sun-exact")]
const EXACT_TRI: u64 = 4;
#[cfg(feature = "sun-exact")]
const EXACT_DIV: u64 = 5;

/// The viewer's bake runs this share of the units it is given a frame
/// ([`FRAME_UNITS`] from the GPU side): the shipped bake's units are
/// measured on the Frontier Marshal (about 2 ms native), but on the viewer's
/// bust, whose layers fill the near field's map, they ran 7 ms native (about
/// 28 in a browser) a frame. Half while the model waits for its first
/// (coarse) cones, so it shows sooner, then a quarter for the exact casts
/// and the fade, which run while it is drawn: about 2.5 and 1.3 ms native.
#[cfg(feature = "sun-exact")]
pub const VIEW_SHARE_FIRST: u64 = 2;
#[cfg(feature = "sun-exact")]
pub const VIEW_SHARE: u64 = 4;

/// Frames over which a model fades from its coarse cones to the exact ones
/// (half a second at 60 fps): no pop when the exact casts land.
#[cfg(feature = "sun-exact")]
pub const FADE_FRAMES: u32 = 30;

/// A ray the coarse test shut (feature `sun-exact`): vertex `item` (over the
/// whole model) or, with `vote`, vote `item`, towards direction `dir`, read
/// against vertex set `set`'s parts.
#[cfg(feature = "sun-exact")]
#[derive(Clone, Copy)]
struct Shut {
    item: u32,
    dir: u8,
    vote: bool,
    set: u16,
}

#[cfg(feature = "sun-exact")]
impl Bake {
    /// After direction `j`'s test of set `set`: notes every ray in front of
    /// its surface that the test left shut.
    fn note_shut(&mut self, set: usize, j: usize) {
        let Some(d) = self.dirs.get(j).copied() else { return };
        let bit = 1u64 << j;
        for r in self.place.iter().filter(|p| p.set == set).map(|p| p.range.clone()) {
            for i in r {
                if let (Some(n), Some(m)) = (self.normals.get(i), self.masks.get(i))
                    && dot(*n, d) > 0.0
                    && m & bit == 0
                {
                    self.shut.push(Shut { item: i as u32, dir: j as u8, vote: false, set: set as u16 });
                }
            }
        }
        for (k, v) in self.votes.iter().enumerate() {
            if v.set == set && v.from.is_some() && dot(v.normal, d) > 0.0 && v.mask & bit == 0 {
                self.shut.push(Shut { item: k as u32, dir: j as u8, vote: true, set: set as u16 });
            }
        }
    }

    /// Casts shut ray `at` exactly, opening it if the cast finds it so;
    /// returns the work units it took.
    fn refine(&mut self, parts: &[Part], at: usize) -> u64 {
        let Some(Shut { item, dir, vote, set }) = self.shut.get(at).copied() else { return 0 };
        let (i, j) = (item as usize, dir as usize);
        let Some(d) = self.dirs.get(j).copied() else { return 0 };
        let ray = if vote { self.votes.get(i).and_then(|v| Some((v.from?, v.normal))) } else { self.from.get(i).copied().zip(self.normals.get(i).copied()) };
        let Some((f, n)) = ray else { return 0 };
        let work = core::cell::Cell::new(0u64);
        let sc = Scene { grid: &self.grid, place: &self.place, spaces: &self.spaces, tri_base: &self.tri_base, parts, work: &work };
        let cell = self.grid.cell;
        if exact_open(sc, set as usize, [0, 1, 2].map(|k| f[k] - n[k] * START * cell), n, d) {
            let m = if vote { self.votes.get_mut(i).map(|v| &mut v.mask) } else { self.masks.get_mut(i) };
            if let Some(m) = m {
                *m |= 1 << j;
            }
        }
        work.get() / EXACT_DIV
    }

    /// At the end of a cones pass: smooths the cones first ([`Stage::Smooth`];
    /// true then); false once they are smoothed (the bake is done).
    fn after_cones(&mut self) -> bool {
        if self.smoothed {
            return false;
        }
        self.stage = Stage::Smooth;
        true
    }

    /// The smoothing is done: the first time, with rays to cast, the coarse
    /// cones are in (shown meanwhile) and the casts start (true: the step
    /// ends, so they are handed back); otherwise the bake ends.
    fn smoothed_now(&mut self) -> bool {
        self.smoothed = true;
        if self.coarse || self.shut.is_empty() {
            self.stage = Stage::Cone { part: usize::MAX, vertex: 0, mask: 0 };
            return false;
        }
        self.coarse = true;
        self.stage = Stage::Refine { at: 0 };
        true
    }

    /// Whether the coarse cones are written (the exact casts are under way
    /// or done).
    pub fn coarse(&self) -> bool {
        self.coarse
    }

    /// How many rays the coarse test shut (for the chunking test).
    pub fn shut_rays(&self) -> usize {
        self.shut.len()
    }
}

/// The cross-fade from the coarse cones to the exact ones (feature
/// `sun-exact`): the coarse cones as shown, per part and vertex, and the
/// fade's frame (0 before it starts).
#[cfg(feature = "sun-exact")]
#[derive(Default)]
struct Fade {
    from: Vec<Vec<[u8; 3]>>,
    frame: u32,
    shown: bool,
}

#[cfg(feature = "sun-exact")]
impl Lazy {
    /// Whether cones are in to draw: the coarse ones once handed back
    /// (the exact casts may still run), else the final ones.
    pub fn baked(&self) -> bool {
        self.parts.is_empty() || self.fade.shown
    }

    /// Whether everything is in (the exact cones faded in, the CPU copy
    /// gone).
    pub fn settled(&self) -> bool {
        self.parts.is_empty()
    }

    /// About `units` more of the bake. The frame the coarse cones are in,
    /// a copy of the parts is handed back to draw while the exact casts run;
    /// once they are in, the parts are handed back each frame of the fade
    /// ([`FADE_FRAMES`]), the last time with the exact cones, and the CPU copy
    /// is dropped. A model with no ray to cast is handed back once.
    pub fn advance(&mut self, units: u64) -> Option<Vec<Part>> {
        let units = units / if self.fade.shown { VIEW_SHARE } else { VIEW_SHARE_FIRST };
        if self.parts.is_empty() {
            return None;
        }
        if self.bake.is_none() && self.fade.frame > 0 {
            return Some(self.fade_step());
        }
        if !self.set_up(units) {
            return None;
        }
        let b = self.bake.as_deref_mut()?;
        if !b.step(&mut self.parts, units) {
            if b.coarse() && !self.fade.shown {
                self.fade.shown = true;
                self.fade.from = self.parts.iter().map(|p| p.vertices.as_chunks::<VERTEX_BYTES>().0.iter().map(|v| [v[6], v[7], v[11]]).collect()).collect();
                return Some(self.parts.clone());
            }
            return None;
        }
        self.bake = None;
        if !self.fade.shown {
            return Some(core::mem::take(&mut self.parts));
        }
        Some(self.fade_step())
    }

    /// The fade's next frame: the parts with each cone blended from the
    /// coarse one (smoothstep over [`FADE_FRAMES`]); the last frame hands the
    /// exact parts back and drops the CPU copy.
    fn fade_step(&mut self) -> Vec<Part> {
        self.fade.frame += 1;
        if self.fade.frame >= FADE_FRAMES {
            self.fade.from = Vec::new();
            return core::mem::take(&mut self.parts);
        }
        let x = self.fade.frame as f32 / FADE_FRAMES as f32;
        let s = x * x * (3.0 - 2.0 * x);
        let mut out = self.parts.clone();
        for (p, from) in out.iter_mut().zip(&self.fade.from) {
            for (v, c) in p.vertices.as_chunks_mut::<VERTEX_BYTES>().0.iter_mut().zip(from) {
                let to = [v[6], v[7], v[11]];
                let mix = |a: u8, b: u8| super::snorm8(f32::from(a as i8) / 127.0 * (1.0 - s) + f32::from(b as i8) / 127.0 * s);
                [v[6], v[7], v[11]] = [mix(c[0], to[0]), mix(c[1], to[1]), mix(c[2], to[2])];
            }
        }
        out
    }
}

/// Each grid cell's triangles (feature `sun-exact`): while marking, each
/// triangle's cells as `[cell, triangle]` (the triangle numbered over the
/// whole model); then ([`Grid::exact_index`]) the triangles of cell `c`,
/// `tris[first[c]..first[c + 1]]`.
#[cfg(feature = "sun-exact")]
#[derive(Default)]
struct ExactIndex {
    pairs: Vec<[u32; 2]>,
    first: Vec<u32>,
    tris: Vec<u32>,
}

#[cfg(feature = "sun-exact")]
impl Grid {
    /// Notes the cells under triangle `id` as [`Self::mark`] samples them;
    /// returns the points it tried.
    fn note(&mut self, [a, b, c]: [[f32; 3]; 3], id: u32) -> u64 {
        let len = |u: [f32; 3], v: [f32; 3]| dot([v[0] - u[0], v[1] - u[1], v[2] - u[2]], [v[0] - u[0], v[1] - u[1], v[2] - u[2]]).sqrt();
        let steps = ((len(a, b).max(len(b, c)).max(len(c, a)) / (0.5 * self.cell)).ceil() as usize).clamp(1, 4096);
        let f = 1.0 / steps as f32;
        for i in 0..=steps {
            for j in 0..=steps - i {
                let (u, v) = (i as f32 * f, j as f32 * f);
                let p = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * u + (c[k] - a[k]) * v);
                if let Some(i) = self.cell_of(p) {
                    let at = ((i[2] * self.dim[1] + i[1]) * self.dim[0] + i[0]) as u32;
                    if self.exact.pairs.last() != Some(&[at, id]) {
                        self.exact.pairs.push([at, id]);
                    }
                }
            }
        }
        ((steps + 1) * (steps + 2) / 2) as u64
    }

    /// Each cell's triangles from the noted pairs, by counting (a triangle
    /// listed twice in a cell is only tested twice).
    fn exact_index(&mut self) {
        let n = self.dim[0] * self.dim[1] * self.dim[2];
        let mut first = vec![0u32; n + 1];
        for [c, _] in &self.exact.pairs {
            if let Some(f) = first.get_mut(*c as usize + 1) {
                *f += 1;
            }
        }
        for i in 1..first.len() {
            first[i] += first[i - 1];
        }
        let mut tris = vec![0u32; self.exact.pairs.len()];
        let mut at = first.clone();
        for [c, t] in &self.exact.pairs {
            if let Some(a) = at.get_mut(*c as usize)
                && let Some(x) = tris.get_mut(*a as usize)
            {
                *x = *t;
                *a += 1;
            }
        }
        self.exact = ExactIndex { pairs: Vec::new(), first, tris };
    }

    /// The triangles of the cell holding `p` (none outside the grid).
    fn tris_at(&self, p: [f32; 3]) -> &[u32] {
        let Some(i) = self.cell_of(p) else { return &[] };
        let c = (i[2] * self.dim[1] + i[1]) * self.dim[0] + i[0];
        match (self.exact.first.get(c), self.exact.first.get(c + 1)) {
            (Some(a), Some(b)) => self.exact.tris.get(*a as usize..*b as usize).unwrap_or_default(),
            _ => &[],
        }
    }
}

/// Each part's first triangle numbered over the whole model, and one past
/// the last (feature `sun-exact`).
#[cfg(feature = "sun-exact")]
fn tri_base(parts: &[Part]) -> Vec<u32> {
    core::iter::once(0)
        .chain(parts.iter().scan(0u32, |n, p| {
            *n += (p.indices.len() / 3) as u32;
            Some(*n)
        }))
        .collect()
}

/// What an exact cast reads (feature `sun-exact`).
#[cfg(feature = "sun-exact")]
#[derive(Clone, Copy)]
struct Scene<'a> {
    grid: &'a Grid,
    place: &'a [Place],
    spaces: &'a [Space],
    tri_base: &'a [u32],
    parts: &'a [Part],
    /// The samples walked and triangles tested so far ([`EXACT_TRI`] each).
    work: &'a core::cell::Cell<u64>,
}

/// Whether direction `d` is open from the surface point `p` (normal `n`,
/// vertex set `set`) cast exactly (feature `sun-exact`): from `p` the shadow
/// map's offset off ([`MAP_N`] along the normal, [`MAP_D`] along the ray, at
/// least [`NEAR_N`] and [`NEAR_D`] cells),
/// through the grid cells the ray crosses (every [`WALK`] of a cell),
/// against the triangles drawn with the set (Moller-Trumbore).
#[cfg(feature = "sun-exact")]
fn exact_open(sc: Scene, set: usize, p: [f32; 3], n: [f32; 3], d: [f32; 3]) -> bool {
    {
        let Scene { grid, place, spaces, tri_base, parts, work } = sc;
        let cell = grid.cell;
        let len = (grid.dim[0] + grid.dim[1] + grid.dim[2]) as f32 * cell;
        let steps = (len / (WALK * cell)).ceil().max(1.0) as usize;
        let o = [0, 1, 2].map(|k| p[k] + n[k] * (NEAR_N * cell).max(MAP_N) + d[k] * (NEAR_D * cell).max(MAP_D));
        // Only the samples inside the grid can meet a triangle: the ray's
        // span in the box, two samples wider each way (rounding), the grid
        // still deciding each sample, so the result is the full walk's.
        let s = len / steps as f32;
        let (mut k0, mut k1) = (0.0f32, steps as f32);
        for r in 0..3 {
            let v = d[r] * s;
            if v == 0.0 {
                continue;
            }
            let (a, b) = ((grid.lo[r] - o[r]) / v, (grid.lo[r] + grid.dim[r] as f32 * cell - o[r]) / v);
            (k0, k1) = (k0.max(a.min(b)), k1.min(a.max(b)));
        }
        let (k0, k1) = ((k0 - 2.0).floor().max(0.0), (k1 + 2.0).ceil().min(steps as f32));
        if k0 > k1 {
            return true;
        }
        let mut last: *const u32 = core::ptr::null();
        for k in k0 as usize..=k1 as usize {
            let q = [0, 1, 2].map(|r| o[r] + d[r] * len * k as f32 / steps as f32);
            let ids = grid.tris_at(q);
            work.set(work.get() + 1);
            if ids.as_ptr() == last {
                continue;
            }
            last = ids.as_ptr();
            work.set(work.get() + EXACT_TRI * ids.len() as u64);
            for id in ids {
                let pi = tri_base.partition_point(|b| b <= id).saturating_sub(1);
                let (Some(pl), Some(part), Some((pos, _)), Some(base)) = (place.get(pi), parts.get(pi), spaces.get(pi), tri_base.get(pi)) else { continue };
                if pl.layer != 0 && pl.layer != set + 1 {
                    continue;
                }
                let Some(t) = part.indices.as_chunks::<3>().0.get((*id - base) as usize) else { continue };
                let Some((c, ..)) = tri(pos, t) else { continue };
                let e1 = [0, 1, 2].map(|r| c[1][r] - c[0][r]);
                let e2 = [0, 1, 2].map(|r| c[2][r] - c[0][r]);
                let hv = cross(d, e2);
                let det = dot(e1, hv);
                if det.abs() < 1e-12 {
                    continue;
                }
                let f = 1.0 / det;
                let sv = [0, 1, 2].map(|r| o[r] - c[0][r]);
                let bu = f * dot(sv, hv);
                if !(0.0..=1.0).contains(&bu) {
                    continue;
                }
                let qv = cross(sv, e1);
                let bv = f * dot(d, qv);
                if bv >= 0.0 && bu + bv <= 1.0 && f * dot(e2, qv) > 0.0 {
                    return false;
                }
            }
        }
        true
    }
}

// Smoothed cones (feature `sun-exact`, the material viewer's; round 5
// follow-up, 2026-10-27). On Low the shader ball's foot drew the bowl's baked
// shadow with a ragged edge, where Ultra's map draws it smooth: the foot's
// corners along that edge read lit or shut one by one, and a triangle blends
// its three corners' cones, so the edge followed the mesh's zigzag. After
// the bake every corner's packed cone (direction times `(1 + threshold) / 2`)
// is blended with its neighbours' a few times, only across faces that turn
// less than [`SMOOTH_TURN`] (a rim's hard edge keeps its own cones), copies
// of one point (a UV seam) moving together. A blend of two cones that
// disagree is shorter, so its threshold is lower: the edge softens towards
// lit, as the owner's rule wants (Low draws no shade where Ultra has none).
// Linear in the corners; nothing ships it.

/// Smoothing rounds after the bake (feature `sun-exact`).
#[cfg(feature = "sun-exact")]
const SMOOTH_ROUNDS: usize = 3;

/// Neighbours whose normals agree less than this (cosine) are not blended.
#[cfg(feature = "sun-exact")]
const SMOOTH_TURN: f32 = 0.8;

/// Work units of one [`Smoother::chunk`] ([`SMOOTH_CHUNK`] vertices or
/// triangles of the smoothing).
#[cfg(feature = "sun-exact")]
const SMOOTH_UNITS: u64 = 2_000;
#[cfg(feature = "sun-exact")]
const SMOOTH_CHUNK: usize = 256;

/// Blends every part's baked cones with their neighbours' (see above), a
/// chunk at a time ([`Stage::Smooth`]; in one go it took 10 ms native on the
/// viewer's bust): per part, its points (a UV seam's copies are one), then
/// their neighbours, then [`SMOOTH_ROUNDS`] rounds, then the cones written
/// back. The same sums in the same order as in one go.
#[cfg(feature = "sun-exact")]
#[derive(Default)]
struct Smoother {
    part: usize,
    phase: u8,
    at: usize,
    round: usize,
    ids: std::collections::HashMap<[u8; 9], usize>,
    group: Vec<usize>,
    cone: Vec<[f32; 3]>,
    normal: Vec<[f32; 3]>,
    next: Vec<Vec<usize>>,
    prev: Vec<[f32; 3]>,
}

#[cfg(feature = "sun-exact")]
impl Smoother {
    /// One chunk; true once every part is smoothed (it is then ready for
    /// another pass).
    fn chunk(&mut self, parts: &mut [Part]) -> bool {
        let Some(p) = parts.get_mut(self.part) else {
            *self = Self::default();
            return true;
        };
        let (a, b) = (self.at, self.at + SMOOTH_CHUNK);
        match self.phase {
            // One point: the same position and normal bytes (a UV seam's
            // copies); the last copy's cone and normal, as in one go.
            0 => {
                let verts = p.vertices.as_chunks::<VERTEX_BYTES>().0;
                for v in verts.get(a..b.min(verts.len())).unwrap_or_default() {
                    let k = [v[0], v[1], v[2], v[3], v[4], v[5], v[8], v[9], v[10]];
                    let n = self.ids.len();
                    let g = *self.ids.entry(k).or_insert(n);
                    let c = [v[6], v[7], v[11]].map(|x| f32::from(x as i8) / 127.0);
                    let nn = normalize([v[8], v[9], v[10]].map(|x| f32::from(x as i8) / 127.0));
                    if g == self.cone.len() {
                        self.cone.push(c);
                        self.normal.push(nn);
                    } else if let (Some(oc), Some(on)) = (self.cone.get_mut(g), self.normal.get_mut(g)) {
                        (*oc, *on) = (c, nn);
                    }
                    self.group.push(g);
                }
                self.next_phase(b >= verts.len(), self.cone.len());
            }
            1 => {
                let tris = p.indices.as_chunks::<3>().0;
                for t in tris.get(a..b.min(tris.len())).unwrap_or_default() {
                    let tg = t.map(|i| self.group.get(i as usize).copied().unwrap_or(usize::MAX));
                    for (x, y) in [(tg[0], tg[1]), (tg[1], tg[2]), (tg[2], tg[0])] {
                        let (Some(nx), Some(ny)) = (self.normal.get(x), self.normal.get(y)) else { continue };
                        if x == y || dot(*nx, *ny) < SMOOTH_TURN {
                            continue;
                        }
                        for (u, w) in [(x, y), (y, x)] {
                            if let Some(l) = self.next.get_mut(u)
                                && !l.contains(&w)
                            {
                                l.push(w);
                            }
                        }
                    }
                }
                self.next_phase(b >= tris.len(), 0);
            }
            2 => {
                if a == 0 {
                    self.prev.clone_from(&self.cone);
                }
                let g = self.cone.len();
                for k in a..b.min(g) {
                    let (Some(own), Some(c)) = (self.prev.get(k), self.cone.get_mut(k)) else { continue };
                    let around = self.next.get(k).map(Vec::as_slice).unwrap_or(&[]);
                    if around.is_empty() {
                        continue;
                    }
                    let mut s = [0.0f32; 3];
                    for o in around.iter().filter_map(|j| self.prev.get(*j)) {
                        s = [0, 1, 2].map(|r| s[r] + o[r]);
                    }
                    let m = 1.0 / around.len() as f32;
                    *c = [0, 1, 2].map(|r| 0.5 * own[r] + 0.5 * s[r] * m);
                }
                if b < g {
                    self.at = b;
                } else {
                    (self.round, self.at) = (self.round + 1, 0);
                    if self.round >= SMOOTH_ROUNDS {
                        self.next_phase(true, 0);
                    }
                }
            }
            _ => {
                let verts = p.vertices.as_chunks_mut::<VERTEX_BYTES>().0;
                let end = b.min(verts.len());
                for (v, k) in verts.get_mut(a..end).unwrap_or_default().iter_mut().zip(self.group.get(a..end).unwrap_or_default()) {
                    if let Some(c) = self.cone.get(*k) {
                        [v[6], v[7], v[11]] = c.map(super::snorm8);
                    }
                }
                if end >= verts.len() {
                    *self = Self { part: self.part + 1, ..Self::default() };
                } else {
                    self.at = b;
                }
            }
        }
        false
    }

    /// Moves on: to the next phase when `done` (making the neighbour lists
    /// of `points` points after the first), else to the next chunk.
    fn next_phase(&mut self, done: bool, points: usize) {
        if done {
            (self.phase, self.at) = (self.phase + 1, 0);
            if self.phase == 1 {
                self.next = vec![Vec::new(); points];
            }
        } else {
            self.at += SMOOTH_CHUNK;
        }
    }
}

// Appended (ov1, 2026-11-02): the bake's setup in steps. `Bake::new` ran in
// one frame, the first frame a model's cones were asked for: 56 to 285 ms
// for material-view's busts in a dev build (Lite, Low; a phone's shipped
// build pays the same order), the hitch of a first visit to a cone tier.

/// Work units of the setup by item (see [`MARK_UNITS`]): a vertex taken
/// into model space or the grid, a triangle read, a vertex's copies found.
struct InitUnits {
    vertex: u64,
    tri: u64,
    firsts: u64,
}

const INIT_UNITS: InitUnits = InitUnits { vertex: 2, tri: 3, firsts: 9 };

/// What the setup knows once every part is in model space.
struct Base {
    place: Vec<Place>,
    set_verts: Vec<usize>,
    grid: Grid,
    from: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>, #[cfg(feature = "sun-exact")] tri_base: Vec<u32>,
}

/// [`Bake::new`] in steps: one part into model space a step, then the grid,
/// the votes ([`VoteSteps`]) and the surfaces ([`SurfaceSteps`]) a share at
/// a time, each step about the work units it is given. The bake it gives is
/// the one-go bake's, however it is split.
pub struct Init {
    spaces: Vec<Space>,
    base: Option<Box<Base>>,
    votes: Option<Box<VoteSteps>>,
    voted: Option<(Vec<Vote>, Vec<u32>, Vec<u32>)>,
    surfaces: Option<Box<SurfaceSteps>>,
}

fn base(parts: &[Part], spaces: &[Space]) -> Base {
    // Sorted and unique by insertion (no standard sort in the download).
    let mut levels: Vec<u8> = Vec::new();
    for l in parts.iter().map(|p| p.lod).filter(|l| *l != SHARED) {
        if let Err(i) = levels.binary_search(&l) {
            levels.insert(i, l);
        }
    }
    let sets = levels.len().max(1);
    let mut set_verts = vec![0; sets];
    let mut start = 0;
    let place: Vec<Place> = parts
        .iter()
        .zip(spaces)
        .map(|(p, s)| {
            let k = levels.iter().position(|l| *l == p.lod);
            let range = start..start + s.0.len();
            start = range.end;
            let set = k.unwrap_or(0);
            set_verts[set] += range.len();
            Place { layer: k.map_or(0, |k| k + 1), set, range }
        })
        .collect();
    let of_set = |drawn: bool| spaces.iter().zip(&place).filter(move |(_, p)| (p.set == 0) == drawn).flat_map(|(s, _)| s.0.iter());
    // The drawn level's triangles' area (the shared parts and the lowest
    // level), as the cells' box is theirs.
    let area: f32 = parts
        .iter()
        .zip(spaces)
        .zip(&place)
        .filter(|(_, pl)| pl.set == 0)
        .flat_map(|((p, s), _)| p.indices.as_chunks::<3>().0.iter().filter_map(|t| tri(&s.0, t)))
        .map(|t| 3.0 * t.1)
        .sum();
    let grid = Grid::empty(of_set(true), of_set(false), levels.len() + 1, area);
    let from: Vec<[f32; 3]> = spaces.iter().flat_map(|(pos, nrm)| pos.iter().zip(nrm).map(|(p, n)| grid.start(*p, *n))).collect();
    let normals: Vec<[f32; 3]> = spaces.iter().flat_map(|s| s.1.iter().copied()).collect();
    Base { place, set_verts, grid, from, normals, #[cfg(feature = "sun-exact")] tri_base: tri_base(parts) }
}

impl Init {
    pub fn new(parts: &[Part]) -> Self {
        Self { spaces: Vec::with_capacity(parts.len()), base: None, votes: None, voted: None, surfaces: None }
    }

    /// About `units` more of the setup (at least one step); the bake once
    /// it is set up. `parts` must be the same at every step.
    pub fn step(&mut self, parts: &[Part], units: u64) -> Option<Bake> {
        let mut left = units;
        loop {
            if let Some(p) = parts.get(self.spaces.len()) {
                self.spaces.push(model_space(p));
                left = left.saturating_sub((p.vertices.len() / VERTEX_BYTES) as u64 * INIT_UNITS.vertex);
            } else if self.base.is_none() {
                let tris: usize = parts.iter().map(|p| p.indices.len() / 3).sum();
                let verts: usize = self.spaces.iter().map(|s| s.0.len()).sum();
                self.base = Some(Box::new(base(parts, &self.spaces)));
                left = left.saturating_sub(tris as u64 * INIT_UNITS.tri + verts as u64 * INIT_UNITS.vertex);
            } else if let Some(b) = self.base.as_deref() {
                if self.voted.is_none() {
                    let spaces = &self.spaces;
                    let v = self.votes.get_or_insert_with(|| Box::new(VoteSteps::new(spaces, &b.place, &b.grid, &b.normals)));
                    if let Some(r) = v.step(parts, spaces, &b.place, &b.grid, &b.normals, &mut left) {
                        self.voted = Some(r);
                        self.votes = None;
                    }
                } else {
                    let spaces = &self.spaces;
                    let s = self.surfaces.get_or_insert_with(|| Box::new(SurfaceSteps::new(spaces, &b.place, b.grid.cell / 64.0)));
                    if let Some(surface) = s.step(parts, &b.place, &mut left) {
                        return Some(self.finish(&surface));
                    }
                }
            }
            if left == 0 {
                return None;
            }
        }
    }

    /// The bake from the finished setup and each vertex's surface.
    fn finish(&mut self, surface: &[u32]) -> Bake {
        // Each surface's byte, in order of its first vertex; none for one.
        let mut code = Vec::new();
        if surface.iter().enumerate().any(|(i, r)| i > 0 && *r == i as u32) {
            let mut of = vec![EMPTY; surface.len()];
            let mut next = 0u8;
            code = surface
                .iter()
                .map(|r| match of.get_mut(*r as usize) {
                    Some(c) if *c == EMPTY => {
                        *c = next;
                        next = (next + 1).min(MANY);
                        *c
                    }
                    Some(c) => *c,
                    None => MANY,
                })
                .collect();
        }
        let spaces = core::mem::take(&mut self.spaces);
        let Base { place, set_verts, grid, from, normals, #[cfg(feature = "sun-exact")] tri_base } = *self.base.take().unwrap_or_else(|| Box::new(base(&[], &[])));
        let (votes, first, ends) = self.voted.take().unwrap_or_default();
        let masks = vec![0; from.len()];
        Bake { spaces, grid, dirs: dirs(), from, normals, marked: Vec::new(), masks, map: Vec::new(), place, set_verts, votes, first, ends, code, points: 0, other: Vec::new(), sources: Vec::new(), big: Vec::new(), layout: Layout::default(), last_units: 0, #[cfg(feature = "sun-exact")] tri_base, #[cfg(feature = "sun-exact")] shut: Vec::new(), #[cfg(feature = "sun-exact")] coarse: false, #[cfg(feature = "sun-exact")] smoother: Smoother::default(), #[cfg(feature = "sun-exact")] smoothed: false, stage: Stage::Mark { part: 0, tri: 0 } }
    }
}

/// The bake's share of a frame, in ms (ov1, 2026-11-02). [`FRAME_UNITS`]
/// was sized for a shipped desktop build (about 4 ms in a browser); a phone,
/// or a dev build (about 40 ms a frame), paid many times that for seconds,
/// long enough for the live tools' frame-time watch to put a tier switch
/// back. The GPU side measures what a unit costs and gives the bake about
/// this much a frame, never more than [`FRAME_UNITS`], so a fast device
/// bakes as before and a slow one takes more frames, each light.
pub const BAKE_MS: f64 = 4.0;

/// Units for this frame at `rate` units per ms ([`BAKE_MS`] of them, at
/// least a 64th of [`FRAME_UNITS`] so a bake always moves on).
pub fn frame_units(rate: f64) -> u64 {
    ((rate * BAKE_MS) as u64).clamp(FRAME_UNITS / 64, FRAME_UNITS)
}

/// The rate after a frame that spent `ms` on `units` (a running average; a
/// frame too short to time keeps the rate).
pub fn next_rate(rate: f64, units: u64, ms: f64) -> f64 {
    if ms < 0.05 {
        return rate;
    }
    0.7 * rate + 0.3 * (units as f64 / ms)
}

/// The rate a bake starts at: [`FRAME_UNITS`] in [`BAKE_MS`].
pub const START_RATE: f64 = FRAME_UNITS as f64 / BAKE_MS;

impl Lazy {
    /// Whether the bake is set up, running `units` of its setup when not
    /// (the call that finishes it says not yet: the bake starts on the next).
    fn set_up(&mut self, units: u64) -> bool {
        if self.bake.is_some() {
            return true;
        }
        let init = self.init.get_or_insert_with(|| Box::new(Init::new(&self.parts)));
        if let Some(b) = init.step(&self.parts, units) {
            self.init = None;
            self.bake = Some(Box::new(b));
        }
        false
    }
}
