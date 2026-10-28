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
//! Stored in the vertex bytes that were free: the direction times
//! `(1 + threshold) / 2` as three snorm8 values, x and y in the position's
//! unused w (bytes 6 and 7), z in the normal's unused w (byte 11). All zero
//! (length 0, threshold -1) is "open everywhere", what an unbaked vertex
//! gets.

use super::{Part, SHARED, VERTEX_BYTES};

/// Cells along the longest side of the occupancy grid.
const GRID: f32 = 64.0;
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
    occ: Vec<Vec<bool>>,
}

impl Grid {
    /// The empty grid of `layers` layers (model space): cells laid on the
    /// box of the points `drawn` (the shared parts and the lowest level),
    /// grown by whole cells to hold the points `rest` (the other levels);
    /// [`Self::mark`] fills it. So a hidden level that reaches further out
    /// does not move the drawn level's cells.
    fn empty<'a>(drawn: impl Iterator<Item = &'a [f32; 3]>, rest: impl Iterator<Item = &'a [f32; 3]>, layers: usize) -> Self {
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
        let ext = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max).max(1e-6);
        let cell = ext / GRID;
        let view = lo.map(|x| x - PAD * cell);
        let view = [view, [0, 1, 2].map(|k| view[k] + ((hi[k] - view[k]) / cell + PAD).ceil().max(1.0) * cell)];
        // The drawn level's cells, grown by whole cells to hold every level.
        let lo = [0, 1, 2].map(|k| view[0][k] - ((lo[k] - all_lo[k]) / cell).ceil() * cell);
        let dim = [0, 1, 2].map(|k| ((all_hi[k] - lo[k]) / cell + PAD).ceil().max(1.0) as usize);
        Self { lo, cell, inv: 1.0 / cell, dim, view, occ: vec![vec![false; dim[0] * dim[1] * dim[2]]; layers.max(1)] }
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
    Dir { set: usize, dir: usize },
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

/// Each vertex's first copy: the lowest-index earlier first copy in the same
/// set whose model-space position is within `tol` on every axis and whose
/// normal is the same to a thousandth, else itself. Each part packs its
/// positions to 16 bits over its own box, so one point carried by two parts
/// reads back up to a step apart; `tol` is well over a step. Found by hash
/// buckets of cells `2 tol` wide laid out by counting, in index order
/// within a bucket (no sort or map: they cost kilobytes of download); a
/// point within `tol` lies in one of 8 cells.
fn firsts(pos: &[[f32; 3]], nrm: &[[f32; 3]], set: &[u32], tol: f32) -> Vec<u32> {
    let n = pos.len().min(nrm.len()).min(set.len());
    let mask = n.next_power_of_two() - 1;
    let inv = 0.5 / tol;
    let hash = |s: u32, p: [f32; 3]| [s, (p[0] * inv).floor() as i32 as u32, (p[1] * inv).floor() as i32 as u32, (p[2] * inv).floor() as i32 as u32].iter().fold(0x811c_9dc5u32, |h, x| (h ^ x).wrapping_mul(0x0100_0193)) as usize & mask;
    let mut ends = vec![0u32; mask + 2];
    for i in 0..n {
        ends[hash(set[i], pos[i]) + 1] += 1;
    }
    for i in 1..ends.len() {
        ends[i] += ends[i - 1];
    }
    let mut order = vec![0u32; n];
    let mut at = ends.clone();
    for i in 0..n {
        let h = hash(set[i], pos[i]);
        order[at[h] as usize] = i as u32;
        at[h] += 1;
    }
    let near = |a: [f32; 3], b: [f32; 3], t: f32| (0..3).all(|k| (a[k] - b[k]).abs() <= t);
    let mut first = vec![0u32; n];
    for i in 0..n {
        let mut best = i;
        for c in 0..8 {
            let h = hash(set[i], [0, 1, 2].map(|k| pos[i][k] + if (c >> k) & 1 == 0 { -tol } else { tol }));
            for &j in &order[ends[h] as usize..ends[h + 1] as usize] {
                let j = j as usize;
                if j >= best {
                    break;
                }
                if first[j] as usize == j && set[j] == set[i] && near(pos[j], pos[i], tol) && near(nrm[j], nrm[i], 1e-3) {
                    best = j;
                    break;
                }
            }
        }
        first[i] = best as u32;
    }
    first
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
fn votes(parts: &[Part], spaces: &[Space], place: &[Place], grid: &Grid, normals: &[[f32; 3]]) -> (Vec<Vote>, Vec<u32>, Vec<u32>) {
    let mut votes = Vec::new();
    let pos: Vec<[f32; 3]> = spaces.iter().flat_map(|s| s.0.iter().copied()).collect();
    let set: Vec<u32> = place.iter().flat_map(|pl| pl.range.clone().map(|_| pl.set as u32)).collect();
    let first = firsts(&pos, normals, &set, grid.cell / 64.0);
    // Per first copy: a third of its small triangles' area (its own vote),
    // and how many votes it gets.
    let mut small = vec![0.0f32; normals.len()];
    let mut start = vec![0u32; normals.len()];
    for ((p, (pos, _)), pl) in parts.iter().zip(spaces).zip(place) {
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
        let spaces: Vec<_> = parts.iter().map(model_space).collect();
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
            .zip(&spaces)
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
        let grid = Grid::empty(of_set(true), of_set(false), levels.len() + 1);
        let from: Vec<[f32; 3]> = spaces.iter().flat_map(|(pos, nrm)| pos.iter().zip(nrm).map(|(p, n)| grid.start(*p, *n))).collect();
        let normals: Vec<[f32; 3]> = spaces.iter().flat_map(|s| s.1.iter().copied()).collect();
        let masks = vec![0; from.len()];
        let (votes, first, ends) = votes(parts, &spaces, &place, &grid, &normals);
        Self { spaces, grid, dirs: dirs(), from, normals, marked: Vec::new(), masks, map: Vec::new(), place, set_verts, votes, first, ends, stage: Stage::Mark { part: 0, tri: 0 } }
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
    fn enter(&mut self, set: usize) -> u64 {
        // A model without levels has the one layer; the shared one.
        self.marked = self.grid.marked(Some(set + 1));
        // Collecting the cells reads the whole grid once.
        self.grid.occ[0].len() as u64 / 16
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
    fn open_dir(&mut self, set: usize, j: usize) {
        let (g, d, marked, from, normals, masks, map) = (&self.grid, self.dirs[j], &self.marked, &self.from, &self.normals, &mut self.masks, &mut self.map);
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
        map.clear();
        map.resize(w * h, f32::MIN);
        // Where a point falls on the map, in texels, and a texel index.
        let at = |p: [f32; 3]| ((dot(p, u) - u0) * g.inv, (dot(p, v) - v0) * g.inv);
        let clamp = |x: f32, n: usize| x.floor().clamp(0.0, (n - 1) as f32) as usize;
        for c in marked {
            let (cu, cv) = at(*c);
            let depth = dot(*c, d);
            for tv in clamp(cv - SPLAT, h)..=clamp(cv + SPLAT, h) {
                for t in &mut map[tv * w + clamp(cu - SPLAT, w)..=tv * w + clamp(cu + SPLAT, w)] {
                    *t = t.max(depth);
                }
            }
        }
        for r in self.place.iter().filter(|p| p.set == set).map(|p| p.range.clone()) {
            for ((m, f), _) in masks[r.clone()].iter_mut().zip(&from[r.clone()]).zip(&normals[r]).filter(|x| dot(*x.1, d) > 0.0) {
                let (fu, fv) = at(*f);
                if map[clamp(fv, h) * w + clamp(fu, w)] <= dot(*f, d) {
                    *m |= 1 << j;
                }
            }
        }
        // The votes from inside big triangles, read the same way.
        for vote in self.votes.iter_mut().filter(|v| v.set == set) {
            if let Some(f) = vote.from
                && dot(vote.normal, d) > 0.0
            {
                let (fu, fv) = at(f);
                if map[clamp(fv, h) * w + clamp(fu, w)] <= dot(f, d) {
                    vote.mask |= 1 << j;
                }
            }
        }
    }

    /// Whether every cone is written.
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// About `units` of work (at least one item: a triangle, a direction
    /// or a vertex), writing each cone into `parts`' spare vertex bytes as
    /// it is found; true once all are written.
    pub fn step(&mut self, parts: &mut [Part], units: u64) -> bool {
        let mut spent = 0u64;
        let mut first = true;
        // Each item checks the budget before it starts, so the first one
        // always runs and the bake always moves on.
        let mut go = |cost: u64, spent: &mut u64| {
            let ok = first || *spent < units;
            first = false;
            *spent = spent.saturating_add(cost);
            ok
        };
        loop {
            match self.stage {
                Stage::Done => return true,
                Stage::Mark { part, tri } => {
                    let Some(p) = parts.get(part) else {
                        spent = spent.saturating_add(self.enter(0));
                        self.stage = Stage::Dir { set: 0, dir: 0 };
                        continue;
                    };
                    let Some(t) = p.indices.as_chunks::<3>().0.get(tri) else {
                        self.stage = Stage::Mark { part: part + 1, tri: 0 };
                        continue;
                    };
                    if !go(0, &mut spent) {
                        return false;
                    }
                    let pos = &self.spaces[part].0;
                    let at = |i: u32| pos.get(i as usize).copied().unwrap_or_default();
                    let layer = self.place.get(part).map_or(0, |p| p.layer);
                    spent = spent.saturating_add(MARK_UNITS * self.grid.mark(layer, [at(t[0]), at(t[1]), at(t[2])]));
                    self.stage = Stage::Mark { part, tri: tri + 1 };
                }
                Stage::Dir { set, dir } => {
                    if dir == DIRS {
                        // The set is done: its own layer goes now.
                        if let Some(o) = self.grid.occ.get_mut(set + 1) {
                            *o = Vec::new();
                        }
                        if set + 1 < self.set_verts.len() {
                            spent = spent.saturating_add(self.enter(set + 1));
                            self.stage = Stage::Dir { set: set + 1, dir: 0 };
                        } else {
                            self.marked = Vec::new();
                            self.stage = Stage::Cone { part: 0, vertex: 0, mask: 0 };
                        }
                        continue;
                    }
                    if !go((self.marked.len() + self.set_verts.get(set).copied().unwrap_or(0) + self.votes.len()) as u64, &mut spent) {
                        return false;
                    }
                    self.open_dir(set, dir);
                    self.stage = Stage::Dir { set, dir: dir + 1 };
                }
                Stage::Cone { part, vertex, mask } => {
                    let Some(p) = parts.get_mut(part) else {
                        // Done: the scratch goes now, not with the bake.
                        *self = Self { stage: Stage::Done, ..Self::new(&[]) };
                        return true;
                    };
                    let Some(v) = p.vertices.as_chunks_mut::<VERTEX_BYTES>().0.get_mut(vertex) else {
                        self.stage = Stage::Cone { part: part + 1, vertex: 0, mask };
                        continue;
                    };
                    if !go(CONE_UNITS, &mut spent) {
                        return false;
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
    bake: Option<Box<Bake>>,
}

impl Lazy {
    pub fn new(parts: Vec<Part>) -> Self {
        Self { parts, bake: None }
    }

    /// Whether the cones are in (the CPU copy is gone).
    pub fn baked(&self) -> bool {
        self.parts.is_empty()
    }

    /// About `units` more of the bake. The frame it finishes, the baked
    /// parts are handed back (to write over the vertex buffers) and the
    /// CPU copy is dropped.
    pub fn advance(&mut self, units: u64) -> Option<Vec<Part>> {
        if self.baked() {
            return None;
        }
        let b = self.bake.get_or_insert_with(|| Box::new(Bake::new(&self.parts)));
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
