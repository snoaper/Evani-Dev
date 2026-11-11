//! Rivers (`water-rivers`, Asset Studio column 20, 2026-11-07): a block of
//! river valley. A river meanders down a broad valley floor of fields and
//! meadows; each bend has a steep cut bank of bare earth on its outside,
//! where the channel runs deepest, and a broad, pale gravel point bar on its
//! inside, sloping gently up out of shallow water; straight reaches between
//! them have plain grassy banks. Oaks line the banks and the field edges,
//! the valley sides rise into wooded hills and mountains close the valley's
//! head, the river coming down through a gap between them.
//!
//! Everything solid is real geometry in catalogue materials (the bed's creek
//! gravel, wet gravel at the edge, the bars' pale gravel, cut banks of loam,
//! meadow turf, hay and mown fields, rock on the steep mountain faces,
//! granite boulders along the banks, oaks from the oak trees' own generator,
//! crown trees on the hills), so a game that lists the ground, stone,
//! foliage and bark recipes draws the same asset the library shows. The
//! water is the engine's nature water (evani-material-nature, a `WaterRect`
//! at the asset's water level: depth colour, Fresnel, sun glint, ripples);
//! the host hands it to the nature pass with [`water`] and sets its look
//! with [`apply_water_look`]. Where the grass blades of evani-grass are
//! built, [`bank_grass`] gives a patch of long grass on the banks that
//! follows the ground.
//!
//! The layout is designed at the default 600 m and scales with `size`; the
//! channel's width, depth and banks are their own params.
//!
//! The ground is one height function sampled on a grid whose cells near the
//! river are split four by four (1 m at the default size, 4 m elsewhere),
//! the split cells' borders matched to their coarse neighbours so the
//! surface has no cracks.

use std::collections::HashMap;

use evani_procedural::surface::presets as sp;
use evani_procedural::{Output, ParamSpec as P, Part, Surface, Values, geo, noise, shapes as sh};

type Mesh = evani_render::mesh::Mesh;

/// The design size (m): every plan coordinate below is at this size.
const DESIGN: f32 = 600.0;
/// The deepest bed's height over the asset's base (m): nothing of the
/// asset goes under its base.
const FLOOR: f32 = 0.2;
/// The design river width (m): bank and bar widths scale with the width
/// over it.
const DESIGN_WIDTH: f32 = 16.0;

/// The floodplain's meadow green (linear).
pub const GRASS: [f32; 3] = [0.046, 0.058, 0.019];
/// The bars' grey gravel (linear): mid grey in full sun, never white
/// (round 4: 0.17 read almost white from the bank).
pub const GRAVEL: [f32; 3] = [0.12, 0.115, 0.102];
/// Hay and stubble fields (linear).
const HAY: [f32; 3] = [0.16, 0.14, 0.065];
/// Freshly mown fields, a brighter green (linear).
const MOWN: [f32; 3] = [0.06, 0.098, 0.027];
/// Pasture, a darker, bluer green (linear).
const PASTURE: [f32; 3] = [0.03, 0.05, 0.016];
/// The bed seen through the water: brown-olive creek gravel (linear).
const BED: [f32; 3] = [0.11, 0.1, 0.07];
/// Wet gravel and silt at the water's edge (linear).
const WET: [f32; 3] = [0.075, 0.07, 0.058];
/// The cut banks' loam (linear).
const LOAM: [f32; 3] = [0.095, 0.078, 0.06];
/// Weathered grey granite of the boulders (linear).
const GRANITE: [f32; 3] = [0.115, 0.113, 0.106];
/// Granite under grey-green lichen (linear).
const LICHEN: [f32; 3] = [0.17, 0.168, 0.14];
/// Wet stone in the riffles (linear).
const WET_STONE: [f32; 3] = [0.08, 0.078, 0.07];
/// The woods' shaded floor under their trees, dark from afar (linear).
const FOREST: [f32; 3] = [0.025, 0.045, 0.017];
/// Silty sand on the bars (linear).
const SILT: [f32; 3] = [0.085, 0.08, 0.07];
/// Coarse dark gravel on the bar heads (linear).
const COARSE: [f32; 3] = [0.085, 0.082, 0.077];
/// Open grazing on the hill slopes between the woods (linear): a cooler,
/// even meadow green seen from afar.
const HILL_GRASS: [f32; 3] = [0.045, 0.068, 0.026];
/// The bank oaks' leaf sprays: a deeper summer green than the oak trees'
/// default, so they don't read yellow in full sun (linear). Round 12: a
/// shade deeper again (0.02/0.042/0.014 read pale and speckled on the middle
/// trees from the bank, with light crowns dotting the far hills from the air).
const OAK_LEAVES: [f32; 3] = [0.017, 0.037, 0.011];
/// The crowns' foliage core: the leaves' green a shade into their own shade
/// (linear).
const LEAF_CORE: [f32; 3] = [0.021, 0.041, 0.014];

pub const PARAMS: [P; 11] = [
    P::num("size", DESIGN, 60.0, 2000.0).unit("m").describe("Width of the square block of river valley (m)"),
    P::num("width", DESIGN_WIDTH, 2.0, 120.0).unit("m").describe("Width of the river at the water's edge (m)"),
    P::num("depth", 1.8, 0.2, 12.0).unit("m").describe("Water depth in the pools under the outer banks (m)"),
    P::num("bank", 1.6, 0.2, 10.0).unit("m").describe("Height of the floodplain over the water (m)"),
    P::num("meander", 1.0, 0.0, 1.6).describe("How far the river swings from side to side (0: straight)"),
    P::num("bars", 1.0, 0.0, 1.0).describe("Gravel bars on the inside of each bend (0: none)"),
    P::num("trees", 1.0, 0.0, 1.0).describe("Trees along the banks, the field edges and on the hills (0: none)"),
    P::num("hills", 1.0, 0.0, 2.0).describe("Height of the low hills along the valley and the ridges beyond them (0: a flat plain)"),
    P::num("surround", 260.0, 0.0, 1500.0).unit("m").describe("Land round the valley block, out to this far past its edge: low hills, then layered ridges (m; 0: the valley block alone)"),
    P::color("color", GRASS).describe("Grass colour of the floodplain"),
    P::color("gravel", GRAVEL).describe("Colour of the gravel bars"),
];

/// The river's centre line in plan (x right, z towards the viewer of the
/// reference's overview), design metres: in from the near side, bends of
/// unequal reach and tightness down the valley (a broad sweep, a tight
/// loop, a long gentle reach, a double bend), then on into the hills
/// beyond the valley block.
const LINE: [[f32; 2]; 28] = [
    [-60.0, 780.0], [-95.0, 640.0], [-150.0, 520.0], [-175.0, 420.0], [-135.0, 350.0], [-62.0, 292.0],
    [12.0, 262.0], [62.0, 226.0], [74.0, 184.0], [48.0, 148.0], [4.0, 128.0], [-46.0, 104.0],
    [-84.0, 66.0], [-86.0, 26.0], [-48.0, 0.0], [10.0, -14.0], [62.0, -36.0], [92.0, -78.0],
    [84.0, -122.0], [44.0, -140.0], [-6.0, -158.0], [-34.0, -196.0], [-26.0, -236.0], [12.0, -276.0],
    [30.0, -330.0], [10.0, -420.0], [-30.0, -560.0], [-20.0, -800.0],
];

/// A ground surface whose overall colour is `rgb`: the ground presets'
/// colour A is their darkest grain and B the body, so A is set to keep the
/// body at `rgb` (tinting A itself lit the paler stones past white). The
/// shoreline's sand tint (column 10).
fn body_tint(s: Surface, rgb: [f32; 3]) -> Surface {
    let (a, b) = (s.colour(0), s.colour(1));
    s.tint(std::array::from_fn(|c| rgb[c] * a[c] / b[c].max(1e-3)))
}

/// A gravel or soil surface whose overall colour is `rgb` ([`body_tint`]) and whose
/// pale stones (the preset's third colour, which the body tint leaves as it
/// is) are only a little lighter than it: at a sand's or a wet band's scale
/// those pale grains averaged to a near-white strip along every bank and
/// white patches on the bars (rounds 3 to 5).
fn grit(s: Surface, rgb: [f32; 3]) -> Surface {
    body_tint(s, rgb).set_colour(2, rgb.map(|c| c * 1.35))
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The valley: the river's line and the ground's height function.
pub struct Valley {
    /// Size over the design size.
    pub k: f32,
    /// Half the whole plot's width (the valley and its surround) and half
    /// the valley block's (m).
    pub half: f32,
    pub inner: f32,
    /// The river's half width at the water (m).
    hw: f32,
    /// Width over the design width.
    kw: f32,
    depth: f32,
    bank: f32,
    bars: f32,
    hills: f32,
    seed: u32,
    /// The centre line about every 2 m: point, unit tangent, smoothed
    /// signed curvature (1/m; positive turning to the +normal side).
    pts: Vec<[f32; 2]>,
    tan: Vec<[f32; 2]>,
    curv: Vec<f32>,
    /// The lookup grid over the plot: per node, signed lateral distance
    /// (m, + on the normal side), curvature and distance along the line.
    grid: Vec<[f32; 3]>,
    gn: usize,
    gcell: f32,
}

/// Where a point lies against the river.
#[derive(Debug, Clone, Copy)]
pub struct Near {
    /// Signed lateral distance from the centre line (m).
    pub d: f32,
    /// Signed curvature of the line there (1/m).
    pub curv: f32,
    /// Distance along the line (m).
    pub s: f32,
}

impl Valley {
    pub fn new(v: &Values, seed: u32) -> Self {
        let size = v.f("size").max(10.0);
        let k = size / DESIGN;
        let width = v.f("width").max(0.5);
        let meander = v.f("meander").max(0.0);
        // The control points, their swing scaled by `meander`, then a
        // Catmull-Rom curve through them, resampled every 2 m (design).
        let ctl: Vec<[f32; 2]> = LINE.iter().map(|p| [p[0] * meander * k, p[1] * k]).collect();
        let mut dense: Vec<[f32; 2]> = Vec::new();
        let n = ctl.len();
        for i in 0..n.saturating_sub(1) {
            let p0 = ctl[i.saturating_sub(1)];
            let (p1, p2) = (ctl[i], ctl[i + 1]);
            let p3 = ctl[(i + 2).min(n - 1)];
            for j in 0..40 {
                let t = j as f32 / 40.0;
                let (t2, t3) = (t * t, t * t * t);
                let c = |a: usize| 0.5 * (2.0 * p1[a] + (-p0[a] + p2[a]) * t + (2.0 * p0[a] - 5.0 * p1[a] + 4.0 * p2[a] - p3[a]) * t2 + (-p0[a] + 3.0 * p1[a] - 3.0 * p2[a] + p3[a]) * t3);
                dense.push([c(0), c(1)]);
            }
        }
        if let Some(last) = ctl.last() {
            dense.push(*last);
        }
        let step = 2.0 * k;
        let mut pts = vec![dense[0]];
        let mut carry = 0.0;
        for w in dense.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let mut at = step - carry;
            while at <= len {
                let t = at / len.max(1e-6);
                pts.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                at += step;
            }
            carry = len - (at - step);
        }
        let m = pts.len();
        let tan: Vec<[f32; 2]> = (0..m)
            .map(|i| {
                let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(m - 1)]);
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let l = (dx * dx + dz * dz).sqrt().max(1e-6);
                [dx / l, dz / l]
            })
            .collect();
        let raw: Vec<f32> = (0..m)
            .map(|i| {
                let (a, b) = (tan[i.saturating_sub(1)], tan[(i + 1).min(m - 1)]);
                (a[0] * b[1] - a[1] * b[0]) / (2.0 * step)
            })
            .collect();
        // Smoothed over about 30 m (design) so a bend's bar spreads along
        // the whole bend.
        let win = (15.0f32 / 2.0).round() as usize;
        let curv: Vec<f32> = (0..m)
            .map(|i| {
                let (lo, hi) = (i.saturating_sub(win), (i + win).min(m - 1));
                raw[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
            })
            .collect();
        let mut va = Valley {
            k,
            half: size * 0.5 + v.f("surround").max(0.0),
            inner: size * 0.5,
            hw: width * 0.5,
            kw: width / DESIGN_WIDTH,
            depth: v.f("depth").max(0.1),
            bank: v.f("bank").max(0.1),
            bars: v.f("bars").clamp(0.0, 1.0),
            hills: v.f("hills").max(0.0),
            seed,
            pts,
            tan,
            curv,
            grid: Vec::new(),
            gn: 0,
            gcell: 3.0 * k,
        };
        let gn = (2.0 * va.half / va.gcell).ceil() as usize + 1;
        let mut grid = Vec::with_capacity(gn * gn);
        for j in 0..gn {
            for i in 0..gn {
                let (x, z) = (-va.half + i as f32 * va.gcell, -va.half + j as f32 * va.gcell);
                let q = va.exact(x, z);
                grid.push([q.d, q.curv, q.s]);
            }
        }
        va.grid = grid;
        va.gn = gn;
        va
    }

    /// The apexes of the valley's strong bends (inside the valley block):
    /// the line point of each bend's greatest curvature and its outer side
    /// (+1: the +normal side). Each has a cut bank of exposed boulders and
    /// a big oak just downstream of it on the outer bank.
    pub fn apexes(&self) -> Vec<(usize, f32)> {
        let n = self.pts.len();
        let win = (24.0f32).round() as usize;
        let mut out = Vec::new();
        for i in 0..n {
            let c = self.curv[i].abs();
            let p = self.pts[i];
            if p[0].abs() > self.inner * 0.92 || p[1].abs() > self.inner * 0.92 {
                continue;
            }
            if smooth(0.004, 0.017, c * self.k) < 0.6 {
                continue;
            }
            let (lo, hi) = (i.saturating_sub(win), (i + win).min(n - 1));
            if self.curv[lo..=hi].iter().all(|o| o.abs() <= c) {
                out.push((i, if self.curv[i] >= 0.0 { -1.0 } else { 1.0 }));
            }
        }
        out
    }

    /// The water level over the asset's base (m).
    pub fn water_level(&self) -> f32 {
        FLOOR + self.depth
    }

    /// The floodplain's level over the asset's base (m).
    pub fn plain(&self) -> f32 {
        self.water_level() + self.bank
    }

    /// The centre line's points (plan, m) about every 2 m (design).
    pub fn line(&self) -> &[[f32; 2]] {
        &self.pts
    }

    /// The centre line's unit tangent at point `i`.
    pub fn tangent(&self, i: usize) -> [f32; 2] {
        self.tan.get(i).copied().unwrap_or([0.0, 1.0])
    }

    /// The river's half width at the water (m).
    pub fn half_width(&self) -> f32 {
        self.hw
    }

    /// The nearest point of the centre line, exactly.
    fn exact(&self, x: f32, z: f32) -> Near {
        let n = self.pts.len();
        let mut best = (f32::MAX, 0usize);
        let mut i = 0;
        while i < n {
            let p = self.pts[i];
            let d2 = (p[0] - x).powi(2) + (p[1] - z).powi(2);
            if d2 < best.0 {
                best = (d2, i);
            }
            i += 6;
        }
        let (lo, hi) = (best.1.saturating_sub(10), (best.1 + 10).min(n.saturating_sub(1)));
        let mut b = (f32::MAX, lo, 0.0f32);
        for j in lo..hi {
            let (a, c) = (self.pts[j], self.pts[j + 1]);
            let (ex, ez) = (c[0] - a[0], c[1] - a[1]);
            let l2 = (ex * ex + ez * ez).max(1e-9);
            let t = (((x - a[0]) * ex + (z - a[1]) * ez) / l2).clamp(0.0, 1.0);
            let (px, pz) = (a[0] + ex * t, a[1] + ez * t);
            let d2 = (x - px).powi(2) + (z - pz).powi(2);
            if d2 < b.0 {
                b = (d2, j, t);
            }
        }
        let j = b.1.min(n.saturating_sub(1));
        let t = self.tangent(j);
        let p = self.pts[j];
        let (ex, ez) = (self.pts[(j + 1).min(n - 1)][0] - p[0], self.pts[(j + 1).min(n - 1)][1] - p[1]);
        let q = [p[0] + ex * b.2, p[1] + ez * b.2];
        let nrm = [-t[1], t[0]];
        let d = (x - q[0]) * nrm[0] + (z - q[1]) * nrm[1];
        let c = lerp(self.curv[j], self.curv[(j + 1).min(n - 1)], b.2);
        Near { d, curv: c, s: (j as f32 + b.2) * 2.0 * self.k }
    }

    /// Where (x, z) lies against the river (from the lookup grid).
    pub fn near(&self, x: f32, z: f32) -> Near {
        let g = self.gn.max(2);
        let fx = ((x + self.half) / self.gcell).clamp(0.0, (g - 1) as f32 - 1e-3);
        let fz = ((z + self.half) / self.gcell).clamp(0.0, (g - 1) as f32 - 1e-3);
        let (i, j) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - i as f32, fz - j as f32);
        let at = |a: usize, b: usize| self.grid.get(b * g + a).copied().unwrap_or([1e4, 0.0, 0.0]);
        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
        let l = |k: usize| lerp(lerp(a[k], b[k], tx), lerp(c[k], d[k], tx), tz);
        Near { d: l(0), curv: l(1), s: l(2) }
    }

    /// The river's half width at `q`: wider on the bends, and narrowing and
    /// widening along its reaches.
    pub fn half_at(&self, q: Near) -> f32 {
        let n = noise::fbm(q.s / (110.0 * self.k), 0.37, self.seed ^ 0x77, 2, 2.0, 0.5);
        self.hw * (1.0 + 0.3 * self.bend(q)) * (1.0 + 0.32 * n)
    }

    /// How strongly the river bends at `q` (0 straight .. 1 a tight bend).
    fn bend(&self, q: Near) -> f32 {
        smooth(0.004, 0.017, q.curv.abs() * self.k)
    }

    /// A bank's rise `t` (0 at the water .. 1 at the top) cut into small
    /// treads where it has slumped and been cut back by floods: two ledges,
    /// stronger along some stretches than others (`s`, m along the river).
    /// Always rising.
    fn ledges(&self, t: f32, s: f32) -> f32 {
        let a = 0.45 * smooth(-0.15, 0.3, noise::fbm(s / (9.0 * self.k), 0.71, self.seed ^ 0x7a, 3, 2.0, 0.55));
        let w = 2.0 * sh::TAU;
        t - a * (t * w).sin() / w
    }

    /// How much of the bank face at `q` is a fresh scour (0 grassed over ..
    /// 1 bare to its top), on the bend's outside if `outer`: scours of
    /// uneven length (a few metres to some 40) along the outer banks of the
    /// bends, a few short ones on the straights, grassed slumps between.
    /// Round 11: the face's bare share is decided along the river, never by
    /// the grid's slope (see [`Valley::bare_bank`]).
    fn scour(&self, q: Near, outer: bool) -> f32 {
        let side = if q.d >= 0.0 { 0.0 } else { 37.0 };
        let n = noise::fbm(q.s / (19.0 * self.k) + side, 0.43 + side, self.seed ^ 0xa7, 3, 2.0, 0.55);
        let bs = self.bend(q);
        let bend = if outer { smooth(-0.3, 0.12, n) * smooth(0.15, 0.55, bs) } else { 0.0 };
        bend.max(0.75 * smooth(0.18, 0.42, n) * (1.0 - bs))
    }

    /// How shallow the straight at `q` runs (0 a run .. 1 a riffle): riffles
    /// 20 to 40 m long between deeper runs (round 11: the straights all ran
    /// 0.76 m deep, so no stone broke the surface and the water read as one
    /// even sheet of fine ripples).
    fn riffle(&self, q: Near) -> f32 {
        smooth(0.05, 0.4, noise::fbm(q.s / (34.0 * self.k), 0.77, self.seed ^ 0x7c, 2, 2.0, 0.5))
    }

    /// The bank face's bare loam at (x, z, y), as a signed distance (m;
    /// negative bare): from the water up the face to a ragged top, as far up
    /// as the bank's scour reaches there. The line runs along the bank (by
    /// the distance from the water): the slope threshold of rounds 3 to 10
    /// sampled a face only a cell or two wide on the 1 m grid, so the loam
    /// came out as a row of brown and green wedges along every cut bank.
    fn bare_bank(&self, x: f32, z: f32, y: f32) -> f32 {
        let (k, kw) = (self.k, self.kw);
        if y > self.plain() + 0.4 {
            return 1.0;
        }
        let q = self.near(x, z);
        let wob = noise::fbm(x / (14.0 * k), z / (14.0 * k), self.seed ^ 0x44, 3, 2.0, 0.5);
        let d = q.d + 0.9 * kw * wob;
        let outer = (if q.curv >= 0.0 { d } else { -d }) < 0.0;
        let u = d.abs() - self.half_at(q);
        if u > 9.0 * kw {
            return 1.0;
        }
        let bs = self.bend(q);
        let sc = self.scour(q, outer);
        let face = lerp(4.0 * (1.0 - 0.45 * sc), if outer { (5.0 - 3.4 * bs) * (1.0 - 0.6 * sc) } else { 4.0 }, bs) * kw;
        // Its top ragged over a few metres (slumped blocks, root mats), and
        // never out over the flat bank top (the turf's lip overhangs it).
        let rag = noise::fbm(x / (4.5 * k), z / (4.5 * k), self.seed ^ 0xa8, 2, 2.0, 0.5);
        let reach = u - (face * sc * (0.8 + 0.25 * rag) - (1.0 - smooth(0.0, 0.25, sc)) * 2.0 * kw);
        reach.max(y - (self.plain() - (0.12 + 0.12 * rag) * self.bank))
    }

    /// How far (x, z) lies past the water's edge (m; negative in the
    /// water), the bank's own wander included, as `height` builds it.
    fn bank_u(&self, x: f32, z: f32) -> f32 {
        let q = self.near(x, z);
        let wob = noise::fbm(x / (14.0 * self.k), z / (14.0 * self.k), self.seed ^ 0x44, 3, 2.0, 0.5);
        (q.d + 0.9 * self.kw * wob).abs() - self.half_at(q)
    }

    /// The point bar's width at `q` (m from the water's edge).
    fn bar_width(&self, q: Near) -> f32 {
        self.bars * (6.0 + 34.0 * self.bend(q)) * self.kw
    }

    /// The channel's ground height at `q` (m over the base) and how much of
    /// the floodplain's own relief shows there (0 by the water .. 1 away).
    fn channel(&self, q: Near, wob: f32, bump: f32) -> (f32, f32) {
        let (wl, kw) = (self.water_level(), self.kw);
        let bs = self.bend(q);
        let hw = self.half_at(q);
        // The banks wander a little along the river.
        let d = q.d + 0.9 * kw * wob;
        let a = d.abs();
        // Straight: a symmetric bowl between grassy banks.
        let sym = if a <= hw {
            let x = a / hw;
            // The straights run shallow (the pools are in the bends), and
            // shallower still over the riffles.
            wl - self.depth * lerp(0.42, 0.12, self.riffle(q)) * (1.0 - x * x)
        } else {
            // A scoured stretch stands steeper than a grassed one.
            let sc = self.scour(q, false);
            wl + self.bank * self.ledges(smooth(0.0, 4.0 * kw * (1.0 - 0.45 * sc), a - hw), q.s)
        };
        // A bend: the deep pool under the steep outer bank, the bar's
        // gentle slope on the inside.
        let e = if q.curv >= 0.0 { d } else { -d };
        let e0 = -0.45 * bs * hw;
        let wb = self.bar_width(q);
        let wr = 5.0 * kw;
        // The cut bank: steeper where the river has scoured it bare (the
        // banks' own strips of ground draw a face this steep cleanly).
        let wc = (5.0 - 3.4 * bs) * kw * (1.0 - 0.6 * self.scour(q, true));
        let lift = 0.55 * (wb / (3.0 * kw)).min(1.0);
        let asym = if e < -hw {
            wl + self.bank * self.ledges(smooth(0.0, wc, -e - hw), q.s)
        } else if e < e0 {
            let x = (e0 - e) / (hw + e0).max(0.1);
            wl - self.depth * (1.0 - x.powf(2.2))
        } else if e <= hw {
            let x = ((e - e0) / (hw - e0).max(0.1)).clamp(0.0, 1.0);
            wl - self.depth * (1.0 - x).powf(1.6)
        } else {
            let u = e - hw;
            if u < wb {
                // The bar's surface is never flat: low ridges and hollows of
                // washed-up gravel, more of them higher up the bar.
                wl + lift * self.bank * (u / wb.max(0.01)).powf(1.2) + 0.09 * self.kw * bump * smooth(0.0, 0.3, u / wb.max(0.01))
            } else {
                wl + self.bank * (lift + (1.0 - lift) * self.ledges(smooth(wb, wb + wr, u), q.s + 7.0))
            }
        };
        let y = lerp(sym, asym, bs);
        let reach = hw + wb * bs + wr + 12.0 * kw;
        (y, smooth(hw + 2.0 * kw, reach, a))
    }

    /// The floodplain's own relief (m over the plain): gentle swells, low
    /// wooded hills along both sides of the broad floodplain, and out in the
    /// surround layered ridges, band behind band (the nearer band open
    /// where the river comes through, the far one closing the view).
    fn relief(&self, x: f32, z: f32, d: f32) -> f32 {
        let (k, s) = (self.k, self.seed);
        let mut y = 0.45 * noise::fbm(x / (90.0 * k), z / (90.0 * k), s, 4, 2.0, 0.5) * (self.bank / 1.6).min(2.0);
        let wob = noise::fbm(x / (160.0 * k), z / (160.0 * k), s ^ 0x11, 3, 2.0, 0.5);
        let roll = 0.5 + 0.5 * noise::fbm(x / (60.0 * k), z / (60.0 * k), s ^ 0x22, 4, 2.0, 0.5);
        // Low hills beyond about 250 m from the valley's axis (round 4:
        // lower, so they no longer fill the skyline from the bank).
        let side = smooth(190.0 * k, 360.0 * k, x.abs() + 50.0 * k * wob);
        y += self.hills * k * side * (9.0 + 15.0 * roll);
        // The ridges: distance from the valley's middle (stretched along
        // it), three bands each a little higher and further, the nearest
        // open over the river.
        let r = (x * x + (z * 1.15).powi(2)).sqrt() + 60.0 * k * wob;
        let open = smooth(20.0 * k, 110.0 * k, d.abs());
        let band1 = smooth(300.0 * k, 370.0 * k, r);
        let band2 = smooth(380.0 * k, 440.0 * k, r);
        let band3 = smooth(450.0 * k, 510.0 * k, r);
        let ridge1 = 0.5 + 0.6 * noise::fbm(x / (110.0 * k) + 3.1, z / (110.0 * k), s ^ 0x33, 4, 2.0, 0.45);
        let ridge2 = 0.5 + 0.6 * noise::fbm(x / (170.0 * k) - 7.3, z / (170.0 * k), s ^ 0x34, 4, 2.0, 0.45);
        let ridge3 = 0.5 + 0.6 * noise::fbm(x / (240.0 * k) + 1.7, z / (240.0 * k), s ^ 0x35, 4, 2.0, 0.45);
        y += self.hills * k * (open * band1 * (4.0 + 8.0 * ridge1) + band2 * (8.0 + 14.0 * ridge2) + band3 * (14.0 + 26.0 * ridge3));
        y
    }

    /// The ground's height (m over the asset's base) at plan point (x, z).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let q = self.near(x, z);
        let k = self.k;
        let wob = noise::fbm(x / (14.0 * k), z / (14.0 * k), self.seed ^ 0x44, 3, 2.0, 0.5);
        let bump = noise::fbm(x / (2.6 * k), z / (2.6 * k), self.seed ^ 0x45, 3, 2.0, 0.55);
        let (y, w) = self.channel(q, wob, bump);
        // Far out the ridges close over the river (it comes round a bend
        // out of sight), so no slot of sky opens between them.
        let r = (x * x + (z * 1.15).powi(2)).sqrt();
        let closed = smooth(380.0 * k, 440.0 * k, r) * smooth(0.0, 0.5, self.hills);
        let mut h = y + w.max(closed) * self.relief(x, z, q.d);
        // The banks and the water's edge are never smooth: slumps, scours
        // and sediment lumps a metre or two across.
        let (hw, ad) = (self.half_at(q), q.d.abs());
        let zone = smooth(hw - 0.8 * self.kw, hw + 0.3 * self.kw, ad) * (1.0 - smooth(hw + 3.0 * self.kw, hw + 7.0 * self.kw, ad));
        if zone > 0.0 {
            h += 0.13 * self.bank * zone * noise::fbm(x / (1.7 * k), z / (1.7 * k), self.seed ^ 0x5d, 3, 2.0, 0.55);
        }
        // The bed's cobbles and ripples, under the water.
        let wl = self.water_level();
        if h < wl {
            h += 0.08 * self.kw * noise::fbm(x / (3.0 * k), z / (3.0 * k), self.seed ^ 0x55, 2, 2.0, 0.5) * smooth(0.0, 0.3, wl - h);
        }
        h.max(FLOOR * 0.5)
    }

    /// The hill woods: where the slopes over the floodplain carry woodland,
    /// as a signed value (negative inside a wood). Broad patches, the higher
    /// slopes more wooded; nothing on the floodplain.
    fn woods(&self, x: f32, z: f32, y: f32) -> f32 {
        let (k, plain) = (self.k, self.plain());
        let up = smooth(plain + 3.0 * k, plain + 9.0 * k, y);
        if up <= 0.0 {
            return 1.0;
        }
        // Continuous in height, so a wood's edge never jumps at the slope's
        // foot (it drew sheer walls there).
        let raw = -noise::fbm(x / (70.0 * k), z / (70.0 * k), self.seed ^ 0xc2, 3, 2.0, 0.5) + 0.18 - 0.3 * up;
        lerp(1.0, raw, up)
    }

    /// The ground's normal at (x, z) (central differences).
    fn normal(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 0.35 * self.k.max(0.2);
        let (hx0, hx1) = (self.height(x - e, z), self.height(x + e, z));
        let (hz0, hz1) = (self.height(x, z - e), self.height(x, z + e));
        geo::normalize([hx0 - hx1, 2.0 * e, hz0 - hz1])
    }

    /// Whether (x, z) is on a gravel bar (the bar's edge wanders), as a
    /// signed distance: negative on the bar.
    fn bar_edge(&self, x: f32, z: f32, y: f32) -> f32 {
        let q = self.near(x, z);
        let k = self.k;
        let wob = noise::fbm(x / (14.0 * k), z / (14.0 * k), self.seed ^ 0x44, 3, 2.0, 0.5);
        let d = q.d + 0.9 * self.kw * wob;
        let e = if q.curv >= 0.0 { d } else { -d };
        let bs = self.bend(q);
        let hw = self.half_at(q);
        let wb = self.bar_width(q) * bs;
        let n = noise::fbm(x / (9.0 * k), z / (9.0 * k), self.seed ^ 0x66, 3, 2.0, 0.5);
        // Inside the bar's reach on the bend's inner side, and below its
        // top (the grass starts at the rise to the floodplain).
        let reach = (e - hw) - wb * (0.85 + 0.25 * n) - 0.6 * self.kw;
        let top = y - (self.water_level() + self.bank * (0.62 + 0.1 * n));
        // Grass and willow scrub take hold on the bar's upper part in islands.
        let isl = noise::fbm(x / (11.0 * k), z / (11.0 * k), self.seed ^ 0x67, 3, 2.0, 0.5);
        let island = (isl - 0.28) * 4.0 * smooth(self.water_level() + 0.25 * self.bank, self.water_level() + 0.45 * self.bank, y);
        if e <= hw - 1.0 * self.kw || wb < 1.0 * self.kw {
            return 1.0;
        }
        reach.max(top).max(island)
    }
}

/// Split `m` along the zero line of `f` (per vertex: position and normal):
/// (where f < 0, where f >= 0). Triangles crossing the line are cut at it,
/// their new corners interpolated (shared along each cut edge), so the
/// border runs smooth instead of following the triangles. (The shoreline's
/// cut, column 10.)
fn cut(m: &Mesh, f: impl Fn(&[f32; 6]) -> f32) -> (Mesh, Mesh) {
    let vals: Vec<f32> = m.vertices.iter().map(&f).collect();
    #[derive(Default)]
    struct Side {
        b: geo::MeshBuilder,
        old: HashMap<u32, u32>,
        mid: HashMap<(u32, u32), u32>,
    }
    impl Side {
        fn old(&mut self, m: &Mesh, i: u32) -> u32 {
            let b = &mut self.b;
            *self.old.entry(i).or_insert_with(|| {
                let v = m.vertices[i as usize];
                b.vertex([v[0], v[1], v[2]], [v[3], v[4], v[5]])
            })
        }
        fn mid(&mut self, m: &Mesh, vals: &[f32], a: u32, c: u32) -> u32 {
            let b = &mut self.b;
            let (a, c) = (a.min(c), a.max(c));
            *self.mid.entry((a, c)).or_insert_with(|| {
                let (va, vc) = (m.vertices[a as usize], m.vertices[c as usize]);
                let (fa, fc) = (vals[a as usize], vals[c as usize]);
                let t = (fa / (fa - fc)).clamp(0.0, 1.0);
                let l = |i: usize| va[i] + (vc[i] - va[i]) * t;
                b.vertex([l(0), l(1), l(2)], geo::normalize([l(3), l(4), l(5)]))
            })
        }
    }
    let (mut lo, mut hi) = (Side::default(), Side::default());
    for t in m.indices.as_chunks::<3>().0 {
        let neg: Vec<bool> = t.iter().map(|&i| vals[i as usize] < 0.0).collect();
        let count = neg.iter().filter(|x| **x).count();
        if count == 0 || count == 3 {
            let s = if count == 3 { &mut lo } else { &mut hi };
            let ids = [s.old(m, t[0]), s.old(m, t[1]), s.old(m, t[2])];
            s.b.tri(ids[0], ids[1], ids[2]);
            continue;
        }
        let lone = (0..3).find(|&i| neg[i] == (count == 1)).unwrap_or(0);
        let (a, b, c) = (t[lone], t[(lone + 1) % 3], t[(lone + 2) % 3]);
        let (one, two) = if count == 1 { (&mut lo, &mut hi) } else { (&mut hi, &mut lo) };
        let (ab1, ac1, a1) = (one.mid(m, &vals, a, b), one.mid(m, &vals, a, c), one.old(m, a));
        one.b.tri(a1, ab1, ac1);
        let (ab2, ac2, b2, c2) = (two.mid(m, &vals, a, b), two.mid(m, &vals, a, c), two.old(m, b), two.old(m, c));
        two.b.tri(ab2, b2, c2);
        two.b.tri(ab2, c2, ac2);
    }
    (lo.b.finish(), hi.b.finish())
}

/// The ground: cells `cells` a side over the whole plot, each split into
/// `level` x `level` (`fine` by the river inside the valley block, 2 by the
/// river out in the surround, 1 elsewhere). A vertex on a border with a
/// coarser neighbour lies on that neighbour's own edge (its height
/// interpolated between the neighbour's vertices there), so the surface has
/// no cracks.
fn ground(va: &Valley, cells: usize, fine: usize) -> Mesh {
    let c = va.half * 2.0 / cells as f32;
    let near = va.hw * 1.3 + 30.0 * va.kw * va.bars + 14.0 * va.kw;
    let inner = va.inner + 60.0 * va.k;
    let levels: Vec<usize> = (0..cells * cells)
        .map(|idx| {
            let (i, j) = (idx % cells, idx / cells);
            let (x, z) = (-va.half + (i as f32 + 0.5) * c, -va.half + (j as f32 + 0.5) * c);
            let q = va.near(x, z);
            if q.d.abs() >= near + c {
                1
            } else if x.abs() < inner && z.abs() < inner {
                // Round 11: the finest cells only over the channel, the
                // banks and the bars (the bars' ridges and hollows need
                // them); the floodplain beyond at half that (its relief is
                // tens of metres across), which pays for the banks' strips.
                let e = if q.curv >= 0.0 { q.d } else { -q.d };
                let bar = if e > 0.0 { va.bar_width(q) * va.bend(q) } else { 0.0 };
                if q.d.abs() < va.half_at(q) + bar + 8.0 * va.kw + c { fine } else { (fine / 2).max(1) }
            } else {
                2.min(fine)
            }
        })
        .collect();
    let level = |i: isize, j: isize| -> usize {
        if i < 0 || j < 0 || i as usize >= cells || j as usize >= cells { 0 } else { levels[j as usize * cells + i as usize] }
    };
    let at = |i: usize, j: usize| (-va.half + i as f32 * c, -va.half + j as f32 * c);
    // A border vertex `t` (0..1) along an edge from `p0` to `p1` whose
    // neighbour is split `n` times: on the neighbour's own edge.
    let on_edge = |p0: (f32, f32), p1: (f32, f32), t: f32, n: usize| -> f32 {
        let nf = n as f32;
        let kk = ((t * nf).floor() as usize).min(n - 1);
        let (t0, t1) = (kk as f32 / nf, (kk + 1) as f32 / nf);
        let p = |u: f32| (lerp(p0.0, p1.0, u), lerp(p0.1, p1.1, u));
        let (a, b) = (p(t0), p(t1));
        lerp(va.height(a.0, a.1), va.height(b.0, b.1), (t - t0) * nf)
    };
    let mut b = geo::MeshBuilder::new();
    let mut corner: HashMap<(usize, usize), u32> = HashMap::new();
    for j in 0..cells {
        for i in 0..cells {
            let s = levels[j * cells + i];
            let (x0, z0) = at(i, j);
            if s <= 1 {
                let mut ids = [0u32; 4];
                for (n, (a, bb)) in [(i, j), (i, j + 1), (i + 1, j + 1), (i + 1, j)].into_iter().enumerate() {
                    ids[n] = *corner.entry((a, bb)).or_insert_with(|| {
                        let (x, z) = at(a, bb);
                        b.vertex([x, va.height(x, z), z], va.normal(x, z))
                    });
                }
                b.quad(ids[0], ids[1], ids[2], ids[3]);
                continue;
            }
            let (ii, jj) = (i as isize, j as isize);
            let (nl, nr, nb, nf) = (level(ii - 1, jj), level(ii + 1, jj), level(ii, jj - 1), level(ii, jj + 1));
            let (x1, z1) = (x0 + c, z0 + c);
            let row = s + 1;
            let base = b.mesh.vertices.len() as u32;
            let coarser = |n: usize| n >= 1 && n < s;
            for bj in 0..=s {
                for ai in 0..=s {
                    let (u, w) = (ai as f32 / s as f32, bj as f32 / s as f32);
                    let (x, z) = (x0 + u * c, z0 + w * c);
                    let corner_v = (ai == 0 || ai == s) && (bj == 0 || bj == s);
                    let y = if corner_v {
                        va.height(x, z)
                    } else if ai == 0 && coarser(nl) {
                        on_edge((x0, z0), (x0, z1), w, nl)
                    } else if ai == s && coarser(nr) {
                        on_edge((x1, z0), (x1, z1), w, nr)
                    } else if bj == 0 && coarser(nb) {
                        on_edge((x0, z0), (x1, z0), u, nb)
                    } else if bj == s && coarser(nf) {
                        on_edge((x0, z1), (x1, z1), u, nf)
                    } else {
                        va.height(x, z)
                    };
                    b.vertex([x, y, z], va.normal(x, z));
                }
            }
            for bj in 0..s {
                for ai in 0..s {
                    let a = base + (bj * row + ai) as u32;
                    b.quad(a, a + row as u32, a + row as u32 + 1, a + 1);
                }
            }
        }
    }
    b.finish()
}

/// Across each bank's strip of ground (m past the water's edge, over the
/// width scale): from under the water up the face, finest where a scoured
/// face stands steepest, out onto the floodplain.
const BANK_U: [f32; 18] = [-2.0, -1.3, -0.7, -0.3, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.25, 1.6, 2.0, 2.6, 3.4, 4.4, 5.6, 7.0];

/// Whether (x, z) lies in the banks' own strips of ground (negative inside,
/// a little narrower than the strips, so they lap under the grid's cut
/// edge): the grid's ground there is dropped for [`banks`].
fn bank_band(va: &Valley, x: f32, z: f32) -> f32 {
    let lim = va.inner + 60.0 * va.k;
    if x.abs() > lim - 12.0 * va.k || z.abs() > lim - 12.0 * va.k {
        return 1.0;
    }
    let u = va.bank_u(x, z);
    ((BANK_U[0] + 0.4) * va.kw - u).max(u - (BANK_U[BANK_U.len() - 1] - 0.6) * va.kw)
}

/// The banks' own ground (round 11): a strip along each bank, its columns
/// every two thirds of a metre down the river and its rows across the bank
/// ([`BANK_U`]), so a face of any steepness is drawn as it is. On the square
/// grid a face a cell or two wide came out as a row of pointed humps along
/// every bank (its top edge cut across the cells' diagonals), the "chain of
/// wedges" seen from across the river since round 3. Its outer edge sinks a
/// few centimetres under the grid's ground it laps under, so no crack opens
/// between them.
fn banks(va: &Valley, fine: bool) -> Mesh {
    let (k, kw) = (va.k, va.kw);
    let lim = va.inner + 60.0 * k - 12.0 * k;
    let pts = va.line();
    let mut b = geo::MeshBuilder::new();
    let rows = BANK_U.len() as u32;
    // Columns every two thirds of a metre, every 2 m on the lighter tiers.
    let subs = if fine { 3 } else { 1 };
    for side in [-1.0f32, 1.0] {
        let mut prev: Option<u32> = None;
        for i in 0..pts.len().saturating_sub(1) {
            for sub in 0..subs {
                let t = sub as f32 / subs as f32;
                let p = [lerp(pts[i][0], pts[i + 1][0], t), lerp(pts[i][1], pts[i + 1][1], t)];
                if p[0].abs() > lim || p[1].abs() > lim {
                    prev = None;
                    continue;
                }
                let (ta, tb) = (va.tangent(i), va.tangent(i + 1));
                let (tx, tz) = (lerp(ta[0], tb[0], t), lerp(ta[1], tb[1], t));
                let l = (tx * tx + tz * tz).sqrt().max(1e-6);
                let nrm = [-tz / l, tx / l];
                let hw = va.half_at(va.near(p[0], p[1]));
                // The bank's own wander (as `height`): its face where
                // |d + 0.9 kw wob| is the half width.
                let (bx, bz) = (p[0] + nrm[0] * side * hw, p[1] + nrm[1] * side * hw);
                let wob = noise::fbm(bx / (14.0 * k), bz / (14.0 * k), va.seed ^ 0x44, 3, 2.0, 0.5);
                let base = b.mesh.vertices.len() as u32;
                for (r, u) in BANK_U.iter().enumerate() {
                    let o = (hw - side * 0.9 * kw * wob + u * kw).max(0.0);
                    let (x, z) = (p[0] + nrm[0] * side * o, p[1] + nrm[1] * side * o);
                    let sink = if r + 1 == BANK_U.len() { 0.04 } else { 0.0 };
                    b.vertex([x, va.height(x, z) - sink, z], va.normal(x, z));
                }
                if let Some(pb) = prev {
                    for r in 0..rows - 1 {
                        let (a, bb, c, d) = (pb + r, pb + r + 1, base + r + 1, base + r);
                        if side > 0.0 {
                            b.quad(a, bb, c, d);
                        } else {
                            b.quad(a, d, c, bb);
                        }
                    }
                }
                prev = Some(base);
            }
        }
    }
    b.finish()
}

/// A crown of `n` leaf cards (the oak's own spray cards, `size` m across)
/// over an ellipsoid round `c` (half sizes `e`): most on its outer shell,
/// some through it, each facing outwards at its own tilt, lit as a mound
/// (the light normal leans out of the crown and darkens deep inside), so a
/// small or far tree, a bush or a stand on a hill reads as broken foliage
/// with gaps, not a solid ball. Added to `t`'s leaves (one leaf part with
/// the oaks).
fn card_crown(t: &mut crate::oak::Tree, c: [f32; 3], e: [f32; 3], n: u32, size: f32, seed: u32) {
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    for k in 0..n {
        let a = sh::rnd(seed, k, 1) * sh::TAU;
        let up = sh::range(seed, k, 2, -0.55, 1.0);
        let rr = sh::range(seed, k, 3, 0.3, 1.0).sqrt();
        let ring = (1.0 - up * up).max(0.0).sqrt();
        let dir = [ring * a.cos(), up, ring * a.sin()];
        let at = [c[0] + e[0] * rr * dir[0], c[1] + e[1] * rr * dir[1], c[2] + e[2] * rr * dir[2]];
        let j = [sh::range(seed, k, 4, -1.0, 1.0), sh::range(seed, k, 5, -1.0, 1.0), sh::range(seed, k, 6, -1.0, 1.0)];
        let nf = geo::normalize([dir[0] * 0.6 + j[0] * 0.7, dir[1] * 0.6 + j[1] * 0.7, dir[2] * 0.6 + j[2] * 0.7]);
        let g = [0.35 * j[1], 1.0, 0.35 * j[0]];
        let mut vup = [g[0] - nf[0] * dot(g, nf), g[1] - nf[1] * dot(g, nf), g[2] - nf[2] * dot(g, nf)];
        if dot(vup, vup) < 1e-6 {
            vup = [1.0 - nf[0] * nf[0], -nf[0] * nf[1], -nf[0] * nf[2]];
        }
        let vup = geo::normalize(vup);
        let right = cross(vup, nf);
        let sz = size * sh::range(seed, k, 7, 0.75, 1.25);
        let half = [right[0] * sz * 0.5, right[1] * sz * 0.5, right[2] * sz * 0.5];
        let top = [vup[0] * sz, vup[1] * sz, vup[2] * sz];
        let base = [at[0] - vup[0] * sz * 0.5, at[1] - vup[1] * sz * 0.5, at[2] - vup[2] * sz * 0.5];
        let deep = (1.0 - rr) * 0.85;
        let ln = geo::normalize([dir[0] * (1.0 - deep), dir[1] * (1.0 - deep) + 0.25 - deep, dir[2] * (1.0 - deep)]);
        let corners = [
            [base[0] - half[0], base[1] - half[1], base[2] - half[2]],
            [base[0] + half[0], base[1] + half[1], base[2] + half[2]],
            [base[0] + half[0] + top[0], base[1] + half[1] + top[1], base[2] + half[2] + top[2]],
            [base[0] - half[0] + top[0], base[1] - half[1] + top[1], base[2] - half[2] + top[2]],
        ];
        // Cells past 255 mark a card crown's cards: their far leaf mass
        // holds until the leaves are a few pixels across (evani-material-
        // recipes `LEAF_FADE`), so a stand reads as overlapping clumps.
        let cell = (seed.wrapping_add(k * 7) % 211) as f32 + 256.0;
        let uvs = [[cell, 1.0], [cell + 1.0, 1.0], [cell + 1.0, 0.0], [cell, 0.0]];
        let b0 = t.leaves.vertices.len() as u32;
        for (p, uv) in corners.iter().zip(uvs) {
            let o = geo::normalize([p[0] - at[0], p[1] - at[1], p[2] - at[2]]);
            // Round 12: corners splayed 0.2 (0.45 lit each card as its own
            // light and dark facet: coarse angular pieces from the air).
            let cn = geo::normalize([ln[0] + o[0] * 0.2, ln[1] + o[1] * 0.2, ln[2] + o[2] * 0.2]);
            t.leaves.vertices.push([p[0], p[1], p[2], cn[0], cn[1], cn[2]]);
            t.leaf_uv.push(uv);
        }
        t.leaves.indices.extend_from_slice(&[b0, b0 + 1, b0 + 2, b0, b0 + 2, b0 + 3]);
    }
}

/// A broken boulder of half sizes `half` (m) round the origin: an
/// icosphere (`sub` 2, or 3 for the big ones) cut by four to six fracture
/// planes into flat broken faces (each a little uneven), its rounded parts
/// roughened, split by one or two crevices on the big ones and settled
/// flatter underneath. The
/// way the pebbles' angular stones are cut (column 7), simplified for a
/// river's hundreds; its faces flat, their edges softly bevelled.
fn boulder(half: [f32; 3], seed: u32, sub: u32) -> Mesh {
    let mut m = sh::icosphere(sub);
    let np = 4 + (sh::rnd(seed, 0, 1) * 3.0) as u32;
    let planes: Vec<([f32; 3], f32)> = (0..np)
        .map(|i| {
            let a = sh::rnd(seed, i, 2) * sh::TAU;
            let up = sh::range(seed, i, 3, -0.25, 0.85);
            let r = (1.0 - up * up).max(0.0).sqrt();
            ([r * a.cos(), up, r * a.sin()], sh::range(seed, i, 4, 0.6, 0.85))
        })
        .collect();
    let nc = if sub >= 3 { 1 + (sh::rnd(seed, 0, 5) * 2.0) as u32 } else { 0 };
    let tilt = sh::range(seed, 0, 9, -0.35, 0.35);
    let bed = geo::normalize([tilt, 1.0, sh::range(seed, 0, 10, -0.35, 0.35)]);
    let bed_off = sh::rnd(seed, 0, 11);
    let cracks: Vec<([f32; 3], f32)> = (0..nc)
        .map(|i| {
            let a = sh::rnd(seed, i, 6) * sh::TAU;
            let t = sh::range(seed, i, 7, -0.45, 0.45);
            let l = (1.0 + t * t).sqrt();
            ([a.cos() / l, t / l, a.sin() / l], sh::range(seed, i, 8, -0.3, 0.3))
        })
        .collect();
    for v in &mut m.vertices {
        let d = [v[0], v[1], v[2]];
        let mut r = 1.0 + 0.14 * noise::fbm3([d[0] * 2.2, d[1] * 2.2, d[2] * 2.2], seed, 3);
        for (n, o) in &planes {
            let c = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            if c > 1e-3 {
                r = r.min(o / c);
            }
        }
        // Fracture faces are never quite flat.
        r *= 1.0 + 0.035 * noise::fbm3([d[0] * 6.0, d[1] * 6.0, d[2] * 6.0], seed ^ 0x5a, 2);
        // Fine fracture relief and the rock's bedding: shallow grooves in
        // layers across it, as granite parts along its sheeting joints.
        r *= 1.0 + 0.04 * noise::fbm3([d[0] * 11.0, d[1] * 11.0, d[2] * 11.0], seed ^ 0x5b, 2);
        let t = (d[0] * bed[0] + d[1] * bed[1] + d[2] * bed[2]) * 4.5 + bed_off;
        r *= 1.0 - 0.045 * smooth(0.72, 1.0, t - t.floor());
        for (n, o) in &cracks {
            let c = d[0] * n[0] + d[1] * n[1] + d[2] * n[2] - o;
            let w = 0.12;
            if c.abs() < w {
                r *= 1.0 - 0.16 * (1.0 - c.abs() / w);
            }
        }
        let mut p = [d[0] * r * half[0], d[1] * r * half[1], d[2] * r * half[2]];
        let foot = -0.35 * half[1];
        if p[1] < foot {
            p[1] = foot + (p[1] - foot) * 0.3;
        }
        *v = [p[0], p[1], p[2], 0.0, 1.0, 0.0];
    }
    // The big ones flat-faceted at their fine subdivision (every facet a
    // small fracture face), the small ones smooth.
    if sub >= 3 {
        sh::flat_shade(&mut m);
    } else {
        geo::smooth_normals(&mut m);
    }
    m
}

/// A low broadleaf plant (dock, plantain): `n` leaves of `len` m in a
/// rosette round `at`, arching up from the ground.
fn rosette(at: [f32; 3], len: f32, n: u32, seed: u32) -> Mesh {
    let mut m = Mesh::default();
    for j in 0..n {
        let l = len * sh::range(seed, j, 1, 0.7, 1.15);
        let mut lf = sh::leaf(l, l * sh::range(seed, j, 2, 0.35, 0.55));
        sh::rotate(&mut lf, [0.0, 0.0, 1.0], sh::range(seed, j, 3, 0.25, 0.75));
        let yaw = j as f32 * sh::TAU / n as f32 + sh::range(seed, j, 4, -0.3, 0.3);
        geo::merge(&mut m, &sh::place(&lf, 1.0, yaw, 0.0, at));
    }
    m
}

/// Add oak `o` turned by `yaw`, scaled by `scale`, standing at `at`.
fn add_oak(dst: &mut crate::oak::Tree, o: &crate::oak::Tree, at: [f32; 3], yaw: f32, scale: f32) {
    let (sy, cy) = yaw.sin_cos();
    let tr = |v: &[f32; 6]| -> [f32; 6] {
        let p = [v[0] * cy + v[2] * sy, v[1], -v[0] * sy + v[2] * cy];
        let n = [v[3] * cy + v[5] * sy, v[4], -v[3] * sy + v[5] * cy];
        [p[0] * scale + at[0], p[1] * scale + at[1], p[2] * scale + at[2], n[0], n[1], n[2]]
    };
    for (dm, duv, sm, suv, vk) in [(&mut dst.wood, &mut dst.wood_uv, &o.wood, &o.wood_uv, scale), (&mut dst.leaves, &mut dst.leaf_uv, &o.leaves, &o.leaf_uv, 1.0)] {
        let base = dm.vertices.len() as u32;
        dm.vertices.extend(sm.vertices.iter().map(tr));
        dm.indices.extend(sm.indices.iter().map(|i| i + base));
        duv.extend(suv.iter().map(|u| [u[0], u[1] * vk]));
    }
}

/// The field a floodplain point lies in: (signed distance to its cell's
/// border in m, negative inside; the cell's kind 0 meadow, 1 hay, 2 mown,
/// 3 pasture).
fn field(va: &Valley, x: f32, z: f32) -> (f32, u32) {
    let cell = 70.0 * va.k;
    // Round 11: a gentler warp (6 m bent every border into a soft curve, so
    // the fields read as rounded blobs from the air).
    let w = 2.5 * va.k * noise::fbm(x / (40.0 * va.k), z / (40.0 * va.k), va.seed ^ 0x88, 2, 2.0, 0.5);
    let (f1, f2, id) = noise::cells2_id((x + w) / cell, (z - w) / cell, va.seed ^ 0x99);
    let kind = if id < 0.4 {
        0
    } else if id < 0.54 {
        1
    } else if id < 0.82 {
        2
    } else {
        3
    };
    (-(f2 - f1) * cell * 0.5, kind)
}

/// The oak shapes the banks are planted from (height m, crown width over
/// height, low limbs, gnarl): six oaks of the oak trees' own generator,
/// their wood light (the river sets many) and their leaves full.
const OAKS: [(f32, f32, u32, f32); 6] = [(11.0, 1.3, 2, 0.5), (13.0, 1.1, 2, 0.65), (14.5, 1.25, 3, 0.55), (16.0, 1.0, 2, 0.7), (17.5, 1.35, 3, 0.45), (12.0, 0.9, 1, 0.6)];

/// The asset: the ground (bed, wet edge, gravel bars, cut banks, crags,
/// fields and meadow), boulders, cobbles on the bars, wildflowers on the
/// banks, oaks, crown trees and bushes.
pub fn generate(v: &Values, seed: u32) -> Output {
    let va = Valley::new(v, seed);
    let (k, kw) = (va.k, va.kw);
    let density = v.density().clamp(0.2, 1.0);
    let wl = va.water_level();
    let plain = va.plain();
    let cells = ((2.0 * va.half / (8.0 * k)).round() as usize).clamp(40, 260);
    let fine = if density >= 0.6 { 8 } else { 4 };
    // The grid's ground, its banks handed to their own strips (round 11).
    let (_, mut g) = cut(&ground(&va, cells, fine), |p| bank_band(&va, p[0], p[2]));
    geo::merge(&mut g, &banks(&va, fine >= 8));
    let mut parts: Vec<Part> = Vec::new();
    let in_valley = |x: f32, z: f32| x.abs() < va.inner && z.abs() < va.inner;

    let (bed, rest) = cut(&g, |p| p[1] - (wl - 0.03));
    let (wet, rest) = cut(&rest, |p| p[1] - (wl + 0.2 + 0.2 * noise::fbm(p[0] / (4.0 * k), p[2] / (4.0 * k), seed ^ 0xa1, 3, 2.0, 0.6)));
    let (bars, rest) = cut(&rest, |p| va.bar_edge(p[0], p[2], p[1]));
    // The bars' sediment in patches: silty sand in the hollows and lee,
    // coarse dark gravel on the heads, clean gravel between.
    let sed = |p: &[f32; 6]| noise::fbm(p[0] / (7.0 * k), p[2] / (7.0 * k), seed ^ 0xa3, 3, 2.0, 0.5);
    let (sand, bars) = cut(&bars, |p| 0.22 - sed(p));
    let (coarse, bars) = cut(&bars, |p| sed(p) + 0.3);
    // Round 11: damp gravel in broken patches low on the bars, where the
    // last high water left them wet (the bars read as one smooth dry sheet).
    // Round 12: up to half a metre over the water, in tongues and pools.
    let (damp, bars) = cut(&bars, |p| p[1] - (wl + 0.34 + 0.4 * noise::fbm(p[0] / (4.0 * k), p[2] / (4.0 * k), seed ^ 0xa4, 3, 2.0, 0.6)));
    // Cut banks: bare loam up the face from the water to a ragged top, as
    // far as each stretch's scour reaches (`Valley::bare_bank`), slumped
    // sediment in patches over it.
    let (soil, rest) = cut(&rest, |p| va.bare_bank(p[0], p[2], p[1]));
    let (fallen, soil) = cut(&soil, |p| 0.18 - noise::fbm(p[0] / (3.2 * k), p[2] / (3.2 * k), seed ^ 0xa9, 2, 2.0, 0.5));
    // Woodland over the hills and ridges in broad patches, open grazing
    // between: the woods' floor dark under their trees (the trees are
    // placed below), the open slopes a grazing green.
    let (crag, rest) = cut(&rest, |p| va.woods(p[0], p[2], p[1]));
    // The open hill slopes: a soft, even grazing green from afar (the turf's
    // blade-scale sparkle read as stipple over whole hillsides).
    let (hill, rest) = cut(&rest, |p| (plain + 3.0 * k) - p[1]);
    // Fields on the floodplain, away from the river and below the slopes.
    let field_cut = |p: &[f32; 6], want: u32| -> f32 {
        let q = va.near(p[0], p[2]);
        let (b, kind) = field(&va, p[0], p[2]);
        let away = (va.hw + 34.0 * kw) - q.d.abs();
        let low = p[1] - (plain + 2.5 * k.max(0.5));
        let inside = if kind == want { b } else { -b };
        inside.max(away).max(low)
    };
    let (hay, rest) = cut(&rest, |p| field_cut(p, 1));
    let (mown, rest) = cut(&rest, |p| field_cut(p, 2));
    let (pasture, meadow) = cut(&rest, |p| field_cut(p, 3));
    // Round 11: the meadow by the water lusher and darker, the ground in
    // the long bank grass's shade (a strip of uneven width along both
    // banks), and the open meadow growing unevenly over a hundred metres
    // and more, greener in the hollows, drier on the swells.
    let (lush, meadow) = cut(&meadow, |p| {
        let q = va.near(p[0], p[2]);
        (q.d.abs() - va.half_at(q)) - (11.0 + 7.0 * noise::fbm(p[0] / (20.0 * k), p[2] / (20.0 * k), seed ^ 0xab, 2, 2.0, 0.5)) * kw
    });
    // Round 12: the growth zones broad and few (two octaves, not three: the
    // third broke every zone into mottling), the fine patches turned down.
    let (dry, meadow) = cut(&meadow, |p| 0.1 - noise::fbm(p[0] / (130.0 * k), p[2] / (130.0 * k), seed ^ 0xac, 2, 2.0, 0.5));

    let gravel = v.color("gravel");
    let grass = v.color("color");
    let bed_s = grit(sp::GROUND_GRAVEL_CREEK, BED).seed(seed ^ 0x10).scale(0.3);
    // Damp silt, matte: the creek gravel's smoother finish caught the low
    // sky as a white rim along every bank (rounds 3 to 6).
    let wet_s = grit(sp::GROUND_SOIL_LOAM, WET).seed(seed ^ 0x11).scale(0.5);
    // The bars dry, matte gravel (the creek preset's smoother, wet-looking
    // finish glinted white on them in the sun, as on the wet band).
    // Round 11: a coarser grain (0.12 drew a smooth grey sheet from the
    // bank), so the bar reads as pebbles between its stones.
    let bar_s = grit(sp::GROUND_GRAVEL_SCREE, gravel).seed(seed ^ 0x12).scale(0.22);
    let damp_s = grit(sp::GROUND_GRAVEL_SCREE, gravel.map(|c| c * 0.5)).seed(seed ^ 0x1c).scale(0.2);
    let soil_s = grit(sp::GROUND_SOIL_LOAM, LOAM).seed(seed ^ 0x13).scale(0.8);
    let fallen_s = grit(sp::GROUND_GRAVEL_SCREE, [0.085, 0.074, 0.06]).seed(seed ^ 0x1d).scale(0.3);
    // The woods' floor and the open hill grazing in the turf the fields
    // use, darker and cooler (the moss carpet drew the hills grey-white).
    let crag_s = evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(FOREST).seed(seed ^ 0x14));
    let hill_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(HILL_GRASS).seed(seed ^ 0x1b)), 0.22);
    // Silty sand: the creek gravel's grain at a sand's scale (the sand
    // presets drew near-white patches in the sun).
    let sand_s = grit(sp::GROUND_GRAVEL_SCREE, SILT).seed(seed ^ 0x19).scale(0.07);
    let coarse_s = grit(sp::GROUND_GRAVEL_SCREE, COARSE).seed(seed ^ 0x1a).scale(0.35);
    // Round 4: more worn and dry patches in every field (the aerial view
    // read broad, evenly mottled sheets).
    // Round 12: the worn patches turned down by about half (the aerial read
    // repeated mottling over every field), the dry zones a clearer straw.
    let meadow_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(grass).seed(seed ^ 0x15)), 0.1);
    let hay_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(HAY).seed(seed ^ 0x16)), 0.07);
    let mown_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(MOWN).seed(seed ^ 0x17)), 0.06);
    let pasture_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(PASTURE).seed(seed ^ 0x18)), 0.09);
    // The ground under the long bank grass: darker (shaded thatch) and less
    // worn. Round 12 tried its blades three times finer (number 5 at 66):
    // they drew rings of moire round the player's feet, so the lawn's own
    // blade count stays.
    let lush_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(grass.map(|c| c * 0.62)).seed(seed ^ 0x1e)), 0.18);
    let dry_s = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint([grass[0] * 1.5, grass[1] * 1.18, grass[2] * 1.0]).seed(seed ^ 0x1f)), 0.12);
    for (m, s) in [(bed, bed_s), (wet, wet_s), (bars, bar_s), (damp, damp_s), (sand, sand_s), (coarse, coarse_s), (soil, soil_s), (fallen, fallen_s), (crag, crag_s), (hill, hill_s), (hay, hay_s), (mown, mown_s), (pasture, pasture_s), (lush, lush_s), (dry, dry_s), (meadow, meadow_s)] {
        if !m.indices.is_empty() {
            parts.push(Part::surfaced(m, s));
        }
    }

    let line = va.line().to_vec();
    let pick = |i: u32, key: u32| -> ([f32; 2], [f32; 2], Near) {
        let j = (sh::rnd(seed, i, key) * line.len() as f32) as usize % line.len().max(1);
        let p = line[j];
        let t = va.tangent(j);
        (p, [-t[1], t[0]], va.near(p[0], p[1]))
    };

    // Boulders: along both banks and the water's edge (a few in the
    // shallows, some up in the bank grass), grey granite, their tops under
    // pale lichen.
    let mut rock = Mesh::default();
    let nb = (170.0 * density.max(0.5)) as u32;
    for i in 0..nb {
        let (p, nrm, _) = pick(i, 1);
        let side = if sh::rnd(seed, i, 2) < 0.5 { -1.0 } else { 1.0 };
        let off = side * (va.hw + sh::range(seed, i, 3, -1.5, 8.0) * kw);
        let (x, z) = (p[0] + nrm[0] * off, p[1] + nrm[1] * off);
        if !in_valley(x, z) {
            continue;
        }
        // Not on the bars' open gravel (their cobbles are their own).
        if va.bar_edge(x, z, va.height(x, z)) < 0.0 && sh::rnd(seed, i, 4) < 0.8 {
            continue;
        }
        let r = kw * sh::range(seed, i, 5, 0.25, 0.8) * if sh::rnd(seed, i, 6) < 0.2 { 1.8 } else { 1.0 };
        let y = va.height(x, z);
        // Round 12: the fine facets only on the biggest (the budget went to
        // the bank toes' stones and the bars' pebbles).
        let mut m = boulder([r * 1.2, r * 0.8, r], seed.wrapping_add(i * 131), if r > 0.7 * kw { 3 } else { 2 });
        sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 7) * sh::TAU);
        // Set into the ground, not sitting on it.
        geo::transform(&mut m, [1.0; 3], [x, y - r * 0.22, z]);
        geo::merge(&mut rock, &m);
    }
    // Each bend's cut bank: boulders washed out of it, from the water's
    // edge up into the bank grass, a few big ones.
    let apexes = va.apexes();
    for (ai, &(j0, outer)) in apexes.iter().enumerate() {
        for b in 0..12u32 {
            let i = 10_000 + ai as u32 * 64 + b;
            let j = (j0 as f32 + sh::range(seed, i, 1, -7.0, 7.0) / k) as usize;
            let j = j.min(line.len().saturating_sub(1));
            let (p, t) = (line[j], va.tangent(j));
            let nrm = [-t[1], t[0]];
            let q = va.near(p[0], p[1]);
            let off = outer * (va.half_at(q) + sh::range(seed, i, 3, -0.8, 6.0) * kw);
            let (x, z) = (p[0] + nrm[0] * off, p[1] + nrm[1] * off);
            let r = kw * sh::range(seed, i, 5, 0.22, 0.6) * if sh::rnd(seed, i, 6) < 0.25 { 1.9 } else { 1.0 };
            let y = va.height(x, z);
            let mut m = boulder([r * 1.25, r * 0.85, r], seed.wrapping_add(i * 131), if r > 0.5 * kw { 3 } else { 2 });
            sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 7) * sh::TAU);
            geo::transform(&mut m, [1.0; 3], [x, y - r * 0.12, z]);
            geo::merge(&mut rock, &m);
        }
    }
    // Riffle stones: on the shallow straights some break the surface, the
    // current foaming round them.
    let mut riffle = Mesh::default();
    let nrf = (220.0 * density.max(0.5)) as u32;
    for i in 0..nrf * 6 {
        if riffle.indices.len() / 3 > (nrf as usize) * 160 {
            break;
        }
        let (p, nrm, q) = pick(i, 61);
        // Round 11: gathered on the riffles (the straights' shallow
        // stretches), where they break the surface and the current laces
        // white round them; a few on the runs between.
        if va.bend(q) > 0.25 || sh::rnd(seed, i, 65) > 0.12 + 0.88 * va.riffle(q) {
            continue;
        }
        let off = sh::range(seed, i, 62, -0.85, 0.85) * va.half_at(q);
        let (x, z) = (p[0] + nrm[0] * off, p[1] + nrm[1] * off);
        if !in_valley(x, z) {
            continue;
        }
        let y = va.height(x, z);
        let r = kw * sh::range(seed, i, 63, 0.18, 0.5);
        let mut m = sh::blob([r * 1.3, r * 0.85, r], [0.0, 0.0, 0.0], 0.35, seed.wrapping_add(i * 211), 1);
        sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 64) * sh::TAU);
        geo::transform(&mut m, [1.0; 3], [x, y + r * 0.35, z]);
        geo::merge(&mut riffle, &m);
    }
    if !riffle.indices.is_empty() {
        parts.push(Part::surfaced(riffle, sp::ROCK_GRANITE_GREY.tint(WET_STONE).number(0, 0.35).seed(seed ^ 0x22).scale(0.8)));
    }
    if !rock.indices.is_empty() {
        let (lichen, bare) = cut(&rock, |p| 0.45 - (p[4] + 0.45 * noise::fbm3([p[0] * 0.9, p[1] * 0.9, p[2] * 0.9], seed ^ 0xb1, 2)));
        if !bare.indices.is_empty() {
            parts.push(Part::surfaced(bare, sp::ROCK_GRANITE_GREY.tint(GRANITE).seed(seed ^ 0x20).scale(0.3)));
        }
        if !lichen.indices.is_empty() {
            parts.push(Part::surfaced(lichen, sp::ROCK_GRANITE_GREY.tint(LICHEN).seed(seed ^ 0x21).scale(0.3)));
        }
    }

    // Cobbles and stones on the bars and in the shallows beside them: many
    // small, a few big, in three tones, thicker at the water's edge.
    let mut cob: [Mesh; 3] = Default::default();
    let ncob = (2500.0 * density) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < ncob && i < ncob * 12 {
        i += 1;
        let (p, nrm, q) = pick(i, 31);
        let bs = va.bend(q);
        if bs < 0.25 {
            continue;
        }
        let inner = if q.curv >= 0.0 { 1.0 } else { -1.0 };
        let off = inner * (va.half_at(q) + sh::range(seed, i, 32, -2.5, 1.0) * kw + sh::rnd(seed, i, 33).powf(1.6) * va.bar_width(q) * bs);
        let (x, z) = (p[0] + nrm[0] * off, p[1] + nrm[1] * off);
        if !in_valley(x, z) {
            continue;
        }
        let y = va.height(x, z);
        // Some stones lie out in the grass at the bar's margin too.
        if y < wl - 0.35 || va.bar_edge(x, z, y) > 1.2 {
            continue;
        }
        let big = sh::rnd(seed, i, 34) < 0.1;
        let r = kw * if big { sh::range(seed, i, 35, 0.22, 0.5) } else { 0.04 + 0.13 * sh::rnd(seed, i, 35).powi(2) };
        let mut m = sh::blob([r * 1.25, r * 0.6, r], [0.0, 0.0, 0.0], 0.3, seed.wrapping_add(i * 71), u32::from(r > 0.12 * kw));
        sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 36) * sh::TAU);
        geo::transform(&mut m, [1.0; 3], [x, y + r * 0.1, z]);
        geo::merge(&mut cob[(sh::rnd(seed, i, 37) * 3.0) as usize % 3], &m);
        made += 1;
    }
    // Pebbles packed between the stones on the bars: thousands of small
    // ones in the three tones, thickest towards the water.
    // Round 12: half as many again and a little bigger, so the bars read as
    // packed gravel between their stones (a smooth grey sheet in round 11).
    let npb = (2300.0 * density) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < npb && i < npb * 10 {
        i += 1;
        let (p, nrm, q) = pick(i, 91);
        if va.bend(q) < 0.25 {
            continue;
        }
        let inner = if q.curv >= 0.0 { 1.0 } else { -1.0 };
        let off = inner * (va.half_at(q) + sh::range(seed, i, 92, -0.5, 1.0) * kw + sh::rnd(seed, i, 93).powf(1.3) * va.bar_width(q) * va.bend(q));
        let (x, z) = (p[0] + nrm[0] * off + sh::range(seed, i, 94, -1.0, 1.0), p[1] + nrm[1] * off + sh::range(seed, i, 95, -1.0, 1.0));
        if !in_valley(x, z) {
            continue;
        }
        let y = va.height(x, z);
        if y < wl - 0.15 || va.bar_edge(x, z, y) > 0.2 {
            continue;
        }
        let r = kw * sh::range(seed, i, 96, 0.03, 0.085);
        geo::merge(&mut cob[(sh::rnd(seed, i, 97) * 3.0) as usize % 3], &sh::blob([r * 1.3, r * 0.55, r], [x, y + r * 0.1, z], 0.3, seed.wrapping_add(i * 37), 0));
        made += 1;
    }
    // Round 10: stones on the bed wherever the water runs shallow (along
    // both banks, over the riffles, off the bars' toes), every one under
    // the surface, so the clear shallows show a stony bed in their own
    // colours instead of an even brown floor.
    let nbs = (800.0 * density) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < nbs && i < nbs * 14 {
        i += 1;
        let (p, nrm, q) = pick(i, 81);
        let off = sh::range(seed, i, 82, -1.05, 1.05) * va.half_at(q);
        let (x, z) = (p[0] + nrm[0] * off + sh::range(seed, i, 83, -1.0, 1.0), p[1] + nrm[1] * off + sh::range(seed, i, 84, -1.0, 1.0));
        if !in_valley(x, z) {
            continue;
        }
        let y = va.height(x, z);
        let deep = wl - y;
        if !(0.06..0.75).contains(&deep) {
            continue;
        }
        // Its top a few centimetres under the surface at most.
        let r = (kw * sh::range(seed, i, 85, 0.07, 0.22) * if sh::rnd(seed, i, 86) < 0.15 { 1.7 } else { 1.0 }).min((deep - 0.03) / 0.85);
        if r < 0.03 {
            continue;
        }
        geo::merge(&mut cob[(sh::rnd(seed, i, 87) * 3.0) as usize % 3], &sh::blob([r * 1.3, r * 0.6, r], [x, y + r * 0.1, z], 0.35, seed.wrapping_add(i * 53), 0));
        made += 1;
    }
    // Round 12: stones and clods along the banks' toes, in clusters where
    // the face has shed them (the far bank read as one smooth slab from its
    // top to the water): mostly small, a few as big as a fist or two.
    let nts = (750.0 * density) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < nts && i < nts * 12 {
        i += 1;
        let (p, nrm, q) = pick(i, 111);
        let side = if sh::rnd(seed, i, 112) < 0.5 { -1.0 } else { 1.0 };
        let off = side * (va.half_at(q) + sh::range(seed, i, 113, -0.4, 0.9) * kw);
        let (x, z) = (p[0] + nrm[0] * off + sh::range(seed, i, 114, -0.7, 0.7), p[1] + nrm[1] * off + sh::range(seed, i, 115, -0.7, 0.7));
        if !in_valley(x, z) || noise::fbm(x / (6.0 * k), z / (6.0 * k), seed ^ 0xd8, 2, 2.0, 0.5) < -0.05 {
            continue;
        }
        let y = va.height(x, z);
        if y > wl + 0.6 || y < wl - 0.25 || va.bar_edge(x, z, y) < 0.0 {
            continue;
        }
        let r = kw * (0.035 + 0.15 * sh::rnd(seed, i, 116).powi(3));
        let mut m = sh::blob([r * 1.25, r * 0.7, r], [0.0, 0.0, 0.0], 0.35, seed.wrapping_add(i * 43), u32::from(r > 0.11 * kw));
        sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::rnd(seed, i, 117) * sh::TAU);
        geo::transform(&mut m, [1.0; 3], [x, y + r * 0.05, z]);
        geo::merge(&mut cob[(sh::rnd(seed, i, 118) * 3.0) as usize % 3], &m);
        made += 1;
    }
    for (j, (m, c)) in cob.into_iter().zip([[0.2, 0.19, 0.17], [0.32, 0.3, 0.26], [0.12, 0.115, 0.105]]).enumerate() {
        if !m.indices.is_empty() {
            parts.push(Part::surfaced(m, sp::ROCK_GRANITE_GREY.tint(c).seed(seed ^ (0x40 + j as u32)).scale(0.4)));
        }
    }

    // Wildflowers in the bank grass: white umbels, yellow buttercups and a
    // few purple heads, in drifts along both banks.
    let mut flowers: [Mesh; 3] = Default::default();
    let nfl = (500.0 * density) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < nfl && i < nfl * 10 {
        i += 1;
        let (p, nrm, q) = pick(i, 51);
        let side = if sh::rnd(seed, i, 52) < 0.5 { -1.0 } else { 1.0 };
        let off = side * (va.half_at(q) + sh::range(seed, i, 53, 3.0, 40.0) * kw);
        let (x, z) = (p[0] + nrm[0] * off + sh::range(seed, i, 54, -2.0, 2.0), p[1] + nrm[1] * off + sh::range(seed, i, 55, -2.0, 2.0));
        if !in_valley(x, z) {
            continue;
        }
        let drift = noise::fbm(x / (12.0 * k), z / (12.0 * k), seed ^ 0xd1, 2, 2.0, 0.5);
        if drift < 0.05 {
            continue;
        }
        let y = va.height(x, z);
        if y < wl + 0.4 || va.bar_edge(x, z, y) < 0.5 {
            continue;
        }
        let kind = if sh::rnd(seed, i, 56) < 0.55 {
            0
        } else if sh::rnd(seed, i, 57) < 0.8 {
            1
        } else {
            2
        };
        // Round 4: open heads, wider and flatter, so they read as flowers
        // over the grass, not as seeds.
        let r = [0.07, 0.035, 0.04][kind] * sh::range(seed, i, 58, 0.8, 1.3);
        for h in 0..3u32 {
            let hgt = sh::range(seed, i * 3 + h, 59, 0.3, 0.75);
            let (dx, dz) = (sh::range(seed, i * 3 + h, 60, -0.15, 0.15), sh::range(seed, i * 3 + h, 65, -0.15, 0.15));
            geo::merge(&mut flowers[kind], &sh::blob([r, r * 0.3, r], [x + dx, y + hgt, z + dz], 0.5, seed.wrapping_add(i * 17 + h), 0));
        }
        made += 1;
    }
    for (m, c) in flowers.into_iter().zip([[0.66, 0.65, 0.6], [0.7, 0.55, 0.05], [0.35, 0.14, 0.4]]) {
        if !m.indices.is_empty() {
            parts.push(Part::new(m, c));
        }
    }

    // Low broadleaf plants in the bank grass (docks, plantains, clover
    // mats): rosettes of broad leaves in drifts, more by the water.
    let mut plants: [Mesh; 2] = Default::default();
    let npl = (1900.0 * density) as u32;
    let (mut made, mut i) = (0u32, 0u32);
    while made < npl && i < npl * 10 {
        i += 1;
        let (p, nrm, q) = pick(i, 71);
        let side = if sh::rnd(seed, i, 72) < 0.5 { -1.0 } else { 1.0 };
        let off = side * (va.half_at(q) + sh::rnd(seed, i, 73).powf(1.8) * 30.0 * kw + 1.0 * kw);
        let (x, z) = (p[0] + nrm[0] * off + sh::range(seed, i, 74, -2.0, 2.0), p[1] + nrm[1] * off + sh::range(seed, i, 75, -2.0, 2.0));
        if !in_valley(x, z) || noise::fbm(x / (9.0 * k), z / (9.0 * k), seed ^ 0xd5, 2, 2.0, 0.5) < -0.1 {
            continue;
        }
        let y = va.height(x, z);
        if y < wl + 0.3 || va.bar_edge(x, z, y) < 0.5 {
            continue;
        }
        // Round 11: broader leaves (docks and burdock as well as plantain).
        let len = sh::range(seed, i, 76, 0.13, 0.34);
        let n = 5 + (sh::rnd(seed, i, 77) * 4.0) as u32;
        geo::merge(&mut plants[(sh::rnd(seed, i, 78) * 2.0) as usize % 2], &rosette([x, y + 0.01, z], len, n, seed.wrapping_add(i * 29)));
        made += 1;
    }
    for (m, c) in plants.into_iter().zip([[0.035, 0.07, 0.018], [0.05, 0.085, 0.022]]) {
        if !m.indices.is_empty() {
            parts.push(Part::new(m, c));
        }
    }

    // Trees.
    let amount = v.f("trees").clamp(0.0, 1.0);
    if amount > 0.0 {
        let templates: Vec<crate::oak::Tree> = OAKS
            .iter()
            .enumerate()
            .map(|(t, &(height, spread, limbs, gnarl))| {
                // Round 5: leaf cards 1.8 times the oak asset's, so the light
                // oaks' sprays close into full, irregular crowns (round 4's
                // read as bare branches with tiny sprays) at the same count.
                let shape = crate::oak::Shape { height, spread, limbs, leaves: 2.8, gnarl, density: 0.3 * density, wood: 0.05, card: 1.75 };
                crate::oak::tree(&shape, seed.wrapping_add(7919 * (t as u32 + 1)))
            })
            .collect();
        let mut oaks = crate::oak::Tree::default();
        let mut cores = Mesh::default();
        let n_oak = (20.0 * amount) as u32;
        // A big oak on each bend's cut bank, a little downstream of the apex and
        // set back from the edge.
        for (ai, &(j0, outer)) in va.apexes().iter().enumerate() {
            let j = (j0 + (8.0 / k).round() as usize).min(line.len().saturating_sub(1));
            let (p, t) = (line[j], va.tangent(j));
            let q = va.near(p[0], p[1]);
            let off = outer * (va.half_at(q) + 9.0 * kw);
            let (x, z) = (p[0] - t[1] * off, p[1] + t[0] * off);
            let y = va.height(x, z);
            let sc = k.sqrt() * 1.05;
            add_oak(&mut oaks, &templates[4], [x, y - 0.1, z], ai as f32 * 2.1, sc);
            let (h0, spread, _, _) = OAKS[4];
            let h = h0 * sc;
            // Round 8: no solid core (even a small one showed as a dark ball
            // where a crown is thin): an inner shell of bigger leaf cards
            // fills the crown instead, leaf-textured from near and massed
            // from the air.
            card_crown(&mut oaks, [x, y + h * 0.64, z], [h * spread * 0.42, h * 0.28, h * spread * 0.42], 60, h * 0.17, seed.wrapping_add(90_000 + ai as u32));
        }
        let (mut made, mut i) = (0u32, 0u32);
        while made < n_oak && i < n_oak * 30 {
            i += 1;
            let (p, nrm, q0) = pick(i, 11);
            let side = if sh::rnd(seed, i, 12) < 0.5 { -1.0f32 } else { 1.0 };
            let inner = side * q0.curv.signum() > 0.0;
            let bs = va.bend(q0);
            let start = va.half_at(q0) + if inner { va.bar_width(q0) * bs + 6.0 * kw } else { 2.5 * kw };
            let off = side * (start + sh::rnd(seed, i, 13).powf(1.5) * 22.0 * kw);
            let (x, z) = (p[0] + nrm[0] * off, p[1] + nrm[1] * off);
            if !in_valley(x, z) {
                continue;
            }
            // In clumps along the river, some alone.
            let clump = noise::fbm(x / (45.0 * k), z / (45.0 * k), seed ^ 0xc1, 2, 2.0, 0.5);
            if sh::rnd(seed, i, 14) > 0.2 + 0.9 * clump.max(0.0) {
                continue;
            }
            let y = va.height(x, z);
            if y < wl + 0.3 {
                continue;
            }
            let ti = (sh::rnd(seed, i, 15) * OAKS.len() as f32) as usize % OAKS.len();
            let sc = k.sqrt() * sh::range(seed, i, 17, 0.75, 1.15);
            let yaw = sh::rnd(seed, i, 16) * sh::TAU;
            add_oak(&mut oaks, &templates[ti], [x, y - 0.1, z], yaw, sc);
            let (h0, spread, _, _) = OAKS[ti];
            let h = h0 * sc;
            card_crown(&mut oaks, [x, y + h * 0.64, z], [h * spread * 0.42, h * 0.28, h * spread * 0.42], 56, h * 0.17, seed.wrapping_add(i * 389));
            made += 1;
        }
        // Stands, hedgerows and scrub as card crowns (a short trunk, a crown
        // of leaf cards): riparian stands along the river, hedgerow trees and
        // bushes on the field edges, and the hill woods as trees standing
        // close in the woods' patches (round 4's lifted canopy read as bare
        // terraces), thinning to a few in the open grazing.
        let mut trunks = Mesh::default();
        let n_crown = (4800.0 * amount * density.max(0.5)) as u32;
        let reach = va.inner + 330.0 * k;
        let (mut made, mut i) = (0u32, 0u32);
        while made < n_crown && i < n_crown * 30 {
            i += 1;
            let (x, z) = (sh::range(seed, i, 21, -reach, reach), sh::range(seed, i, 22, -reach, reach));
            if x.abs() > va.half * 0.97 || z.abs() > va.half * 0.97 {
                continue;
            }
            let q = va.near(x, z);
            if q.d.abs() < va.half_at(q) + 8.0 * kw {
                continue;
            }
            let y = va.height(x, z);
            if y < wl + 0.3 || va.bar_edge(x, z, y) < 0.5 {
                continue;
            }
            let slope = y - plain;
            let woods = noise::fbm(x / (60.0 * k), z / (60.0 * k), seed ^ 0xc2, 3, 2.0, 0.5);
            let (b, _) = field(&va, x, z);
            let near_river = q.d.abs() < va.half_at(q) + 30.0 * kw;
            let (want, bush) = if slope > 3.0 * k {
                // The hill woods: trees close together inside the woods'
                // patches, their edges ragged, a few alone in the open.
                let w = va.woods(x, z, y);
                (if y > plain + 110.0 * k { 0.0 } else if w < 0.0 { 0.95 } else if w < 0.08 { 0.3 } else { 0.0 }, false)
            } else if near_river {
                // Riparian stands and scrub along the banks, in clumps.
                (0.35 * smooth(-0.15, 0.3, woods), sh::rnd(seed, i, 25) < 0.5)
            } else if b > -5.0 * k {
                // Hedgerows along the field edges: bushes with a tree now and
                // then, broken only in a few gaps.
                (0.9 * smooth(-0.6, -0.2, woods + 0.25), sh::rnd(seed, i, 25) < 0.7)
            } else {
                (0.004, false)
            };
            if sh::rnd(seed, i, 23) > want {
                continue;
            }
            let s2 = seed.wrapping_add(i * 977);
            if bush {
                let r = sh::range(seed, i, 26, 0.9, 2.2) * k.sqrt();
                card_crown(&mut oaks, [x, y + r * 0.55, z], [r, r * 0.6, r * 0.9], 14, r * 0.85, s2);
            } else {
                let h = sh::range(seed, i, 24, 8.0, 17.0) * k.sqrt();
                let r = h * sh::range(seed, i, 27, 0.3, 0.44);
                if slope > 3.0 * k {
                    // A hill wood's tree, seen from a few hundred metres:
                    // big cards, no trunk under the crowns of its
                    // neighbours. Round 11: no core (far cards now draw as
                    // solid leaf masses, so a core only cost triangles),
                    // two more cards, and crowns of uneven build, tall and
                    // narrow to broad and low, so a wood is no row of equal
                    // shapes.
                    let tall = sh::range(seed, i, 29, 0.24, 0.42);
                    let e = [r * (1.25 - tall), h * tall, r * sh::range(seed, i, 28, 0.8, 1.15) * (1.25 - tall)];
                    card_crown(&mut oaks, [x, y + h * 0.6, z], e, 14, h * 0.27, s2);
                    made += 1;
                    continue;
                }
                // Round 11: the trunk shorter and the crown lower and
                // deeper (round 10's tall bare poles under small crowns
                // lined the far fields), half the trees with a second,
                // smaller lobe off to one side and a little lower, so
                // stands and hedgerows read as overlapping canopy.
                geo::merge(&mut trunks, &sh::blob([h * 0.045, h * 0.17, h * 0.045], [x, y + h * 0.13, z], 0.1, s2, 0));
                let e = [r * 1.2, h * 0.36, r * 1.2 * sh::range(seed, i, 28, 0.8, 1.15)];
                if sh::rnd(seed, i, 30) < 0.5 {
                    let a = sh::rnd(seed, i, 31) * sh::TAU;
                    let o = r * sh::range(seed, i, 32, 0.8, 1.15);
                    card_crown(&mut oaks, [x, y + h * 0.53, z], e, 34, h * 0.175, s2);
                    card_crown(&mut oaks, [x + a.cos() * o, y + h * 0.42, z + a.sin() * o], [e[0] * 0.72, e[1] * 0.75, e[2] * 0.72], 22, h * 0.16, s2 ^ 0x9);
                } else {
                    card_crown(&mut oaks, [x, y + h * 0.53, z], e, 50, h * 0.175, s2);
                }
                geo::merge(&mut cores, &sh::blob([e[0] * 0.22, e[1] * 0.22, e[2] * 0.22], [x, y + h * 0.5, z], 0.8, s2 ^ 0x5, 0));
            }
            made += 1;
        }
        // Round 10: hedgerows, lines of trees and bushes along the field
        // borders with a gap here and there (the reference's hedged
        // pasture), on their own budget: the crowns above sample the whole
        // plot, so few landed on the narrow borders and the fields read as
        // bare polygons from the air.
        let n_hedge = (800.0 * amount * density.max(0.5)) as u32;
        let (mut made, mut i) = (0u32, 0u32);
        while made < n_hedge && i < n_hedge * 40 {
            i += 1;
            let (x, z) = (sh::range(seed, i, 41, -reach, reach), sh::range(seed, i, 42, -reach, reach));
            if x.abs() > va.half * 0.97 || z.abs() > va.half * 0.97 {
                continue;
            }
            let q = va.near(x, z);
            if q.d.abs() < va.half_at(q) + 30.0 * kw {
                continue;
            }
            let y = va.height(x, z);
            if y > plain + 2.5 * k.max(0.5) {
                continue;
            }
            let (b, _) = field(&va, x, z);
            if b < -3.0 * k {
                continue;
            }
            // Gaps: gateways and stretches grubbed out.
            if noise::fbm(x / (25.0 * k), z / (25.0 * k), seed ^ 0xc5, 2, 2.0, 0.5) < -0.35 {
                continue;
            }
            let s2 = seed.wrapping_add(i * 1013);
            if sh::rnd(seed, i, 43) < 0.35 {
                let h = sh::range(seed, i, 44, 7.0, 13.0) * k.sqrt();
                let r = h * sh::range(seed, i, 45, 0.3, 0.42);
                geo::merge(&mut trunks, &sh::blob([h * 0.045, h * 0.17, h * 0.045], [x, y + h * 0.13, z], 0.1, s2, 0));
                let e = [r * 1.1, h * 0.37, r * 1.1 * sh::range(seed, i, 46, 0.8, 1.15)];
                card_crown(&mut oaks, [x, y + h * 0.53, z], e, 38, h * 0.19, s2);
            } else {
                let r = sh::range(seed, i, 47, 1.6, 3.2) * k.sqrt();
                card_crown(&mut oaks, [x, y + r * 0.55, z], [r, r * 0.7, r * 0.9], 16, r * 0.8, s2);
            }
            made += 1;
        }
        // Round 12: the bank tops broken by scrub, nettle beds and bramble
        // leaning out over the face (the far bank's top read as one ruled
        // line over a smooth slab): low leafy clumps in drifts along both
        // banks, at each face's top edge.
        let n_lip = (700.0 * amount * density.max(0.5)) as u32;
        let (mut made, mut i) = (0u32, 0u32);
        while made < n_lip && i < n_lip * 12 {
            i += 1;
            let (p, nrm, q) = pick(i, 101);
            let side = if sh::rnd(seed, i, 102) < 0.5 { -1.0f32 } else { 1.0 };
            if !in_valley(p[0], p[1]) {
                continue;
            }
            // The face's top: out from the water's edge to where the
            // ground nears the plain.
            let hw = va.half_at(q);
            let mut u = 0.0;
            while u < 8.0 * kw {
                let o = side * (hw + u);
                if va.height(p[0] + nrm[0] * o, p[1] + nrm[1] * o) > plain - 0.2 * va.bank {
                    break;
                }
                u += 0.25 * kw;
            }
            let o = side * (hw + u + sh::range(seed, i, 103, -0.5, 1.0) * kw);
            let (x, z) = (p[0] + nrm[0] * o + sh::range(seed, i, 106, -0.8, 0.8), p[1] + nrm[1] * o + sh::range(seed, i, 107, -0.8, 0.8));
            if noise::fbm(x / (9.0 * k), z / (9.0 * k), seed ^ 0xd7, 2, 2.0, 0.5) < -0.12 {
                continue;
            }
            let y = va.height(x, z);
            if y < wl + 0.4 || va.bar_edge(x, z, y) < 0.5 {
                continue;
            }
            let r = sh::range(seed, i, 104, 0.35, 0.95) * k.sqrt();
            // Leaning out towards the water.
            let lean = side * sh::range(seed, i, 105, 0.1, 0.6) * r;
            let (cx, cz) = (x - nrm[0] * lean, z - nrm[1] * lean);
            card_crown(&mut oaks, [cx, y + r * 0.3, cz], [r * 1.2, r * 0.55, r], 9, r * 0.75, seed.wrapping_add(i * 563));
            made += 1;
        }
        if !oaks.wood.indices.is_empty() {
            let bark: Surface = sp::BARK_OAK.tint(crate::oak::BARK).number(0, 1.0).number(1, 0.85).number(2, 0.469).number(4, 0.43).number(7, 1.0).seed(seed).scale(3.0);
            // Round 10: extras bit 4 fills the cards once they are far (a
            // fifth of a far card's pixels was kept, blended away, so every
            // crown read as bare branches from the air and thin across the
            // river): see evani-material-recipes `leaf_cut`.
            let leaf: Surface = sp::LEAF_OAK.tint(OAK_LEAVES).variant(0).extras(0b1_1001).number(0, 0.8).number(1, 0.9).number(2, 0.0).number(3, 0.42).number(4, 0.0).number(7, 0.12).seed(seed).scale(1.0).cards();
            parts.push(Part::surfaced_uv(oaks.wood, bark, oaks.wood_uv));
            parts.push(Part::surfaced_uv(oaks.leaves, leaf, oaks.leaf_uv));
        }
        if !trunks.indices.is_empty() {
            parts.push(Part::surfaced(trunks, sp::BARK_OAK.tint(crate::oak::BARK).seed(seed ^ 0x30).scale(2.0)));
        }
        if !cores.indices.is_empty() {
            parts.push(Part::surfaced(cores, evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(LEAF_CORE).seed(seed ^ 0x32))));
        }
    }
    Output::Parts(parts)
}

/// The water's look for the nature pass (`material.nature.*`) and the bank
/// grass's (`material.grass.meadow.*`): a clear lowland river that hides
/// its bed in the pools and shows it in the shallows, reflecting the sky
/// (not the haze), its surface drifting downstream in broad directional
/// ripples with broken highlights; white broken water only over the
/// riffles, where the bed runs shallow and level (round 12: round 11's
/// crests dotted the whole channel at one spacing), calm dark water over the
/// pools and runs between; lace foam where the bed or a stone comes within a
/// hand of the surface (round the riffles' stones and along the waterline); the bank
/// grass a dense summer meadow without seed spears, broken by bare gaps.
/// A host applies it once with [`apply_water_look`] and sets
/// `material.nature.water.flow_direction` to the reach it shows.
pub const WATER_LOOK: [(&str, Look); 43] = [
    ("material.nature.sky.match_background", Look::F(0.0)),
    ("material.nature.sky.horizon", Look::C([0.64, 0.75, 0.88])),
    ("material.nature.sky.zenith", Look::C([0.24, 0.42, 0.76])),
    ("material.nature.water.deep_color", Look::C([0.09, 0.14, 0.09])),
    ("material.nature.water.clarity", Look::F(3.4)),
    ("material.nature.water.scatter_color", Look::C([0.02, 0.06, 0.055])),
    ("material.nature.water.coast_murk", Look::F(0.15)),
    ("material.nature.water.foam_width", Look::F(0.05)),
    ("material.nature.water.foam", Look::F(0.5)),
    ("material.nature.water.coast_lace", Look::F(0.75)),
    ("material.nature.water.wave_scale", Look::F(6.0)),
    ("material.nature.water.wave_speed", Look::F(1.2)),
    ("material.nature.water.wave_strength", Look::F(0.14)),
    ("material.nature.water.glint", Look::F(1.1)),
    ("material.nature.water.refraction", Look::F(0.5)),
    ("material.nature.water.reflection", Look::F(0.32)),
    ("material.nature.water.roughness", Look::F(0.09)),
    ("material.nature.water.swell", Look::F(0.85)),
    ("material.nature.water.choppiness", Look::F(0.55)),
    ("material.nature.water.flow_speed", Look::F(0.9)),
    ("material.nature.water.whitecap_amount", Look::F(0.8)),
    ("material.nature.water.whitecap_threshold", Look::F(0.6)),
    ("material.nature.water.whitecap_depth", Look::F(0.4)),
    ("material.nature.water.coast_swash", Look::F(0.0)),
    ("material.nature.water.coast_caustics", Look::F(0.3)),
    ("material.nature.water.coast_caustic_scale", Look::F(2.5)),
    ("material.nature.water.coast_ripples", Look::F(0.35)),
    ("material.grass.meadow.root", Look::C([0.03, 0.04, 0.016])),
    ("material.grass.meadow.tip", Look::C([0.08, 0.1, 0.038])),
    ("material.grass.meadow.dry", Look::C([0.28, 0.24, 0.13])),
    ("material.grass.meadow.ear_length", Look::F(0.0)),
    ("material.grass.meadow.width", Look::F(0.014)),
    ("material.grass.meadow.density", Look::F(360.0)),
    ("material.grass.meadow.height", Look::F(0.36)),
    ("material.grass.meadow.height_var", Look::F(1.0)),
    ("material.grass.meadow.curl", Look::F(1.0)),
    ("material.grass.meadow.clump", Look::F(1.0)),
    ("material.grass.meadow.bare", Look::F(0.22)),
    ("material.grass.meadow.dryness", Look::F(0.2)),
    ("material.grass.meadow.lean", Look::F(0.7)),
    ("material.grass.meadow.variation", Look::F(0.85)),
    ("material.grass.meadow.droop", Look::F(0.8)),
    ("material.grass.meadow.heads", Look::F(0.0)),
];

/// A water-look value.
#[derive(Debug, Clone, Copy)]
pub enum Look {
    F(f32),
    C([f32; 3]),
}

/// Set [`WATER_LOOK`] in the param registry (names the build lacks are
/// skipped and returned).
pub fn apply_water_look(reg: &mut evani_core::params::ParamRegistry) -> Vec<&'static str> {
    use evani_core::params::ParamValue;
    let mut missing = Vec::new();
    for (name, v) in WATER_LOOK {
        let val = match v {
            Look::F(x) => ParamValue::F32(x),
            Look::C(c) => ParamValue::Color(c),
        };
        match reg.id(name) {
            Some(id) => {
                let _ = reg.set(id, val);
            }
            None => missing.push(name),
        }
    }
    missing
}

/// The asset's live valley (its params and seed from `registry`), for a
/// host placing cameras or grass on it. `None` when it isn't registered.
pub fn valley(registry: &evani_procedural::Registry, reg: &evani_core::params::ParamRegistry) -> Option<Valley> {
    let (v, seed) = registry.values(reg, "water-rivers")?;
    Some(Valley::new(&v, seed as u32))
}

/// The asset's water for the nature pass: a water rectangle over the whole
/// plot at its water level (the ground hides it where it stands above it),
/// moved like the placement (`at`, `yaw`, `scale`).
#[cfg(feature = "host-nature")]
pub fn water(registry: &evani_procedural::Registry, reg: &evani_core::params::ParamRegistry, at: [f32; 3], yaw: f32, scale: f32) -> Option<evani_material_nature::Surface> {
    use evani_material_nature::{Kind, Surface as W};
    let (v, _) = registry.values(reg, "water-rivers")?;
    let half = v.f("size").max(10.0) * 0.5;
    let wl = FLOOR + v.f("depth").max(0.1);
    Some(W::new(Kind::WaterRect, [0.0, wl, 0.0], [half, 0.0, half]).placed(at, yaw, scale))
}

/// Long grass on the banks for evani-grass, over the plan rectangle
/// `min`..`max` (asset metres) of the valley placed at `at`: a meadow patch
/// whose ground follows the valley (`n` x `n` heights), buried where the
/// ground is water, wet edge or gravel bar. A game adds it with
/// `Grass::add_patch_on(patch, n, &heights)`.
#[cfg(feature = "host-grass")]
pub fn bank_grass(va: &Valley, min: [f32; 2], max: [f32; 2], at: [f32; 3]) -> (evani_grass::Patch, usize, Vec<f32>) {
    let n = 113usize;
    let mut h = Vec::with_capacity(n * n);
    let wl = va.water_level();
    for j in 0..n {
        for i in 0..n {
            let x = lerp(min[0], max[0], i as f32 / (n - 1) as f32);
            let z = lerp(min[1], max[1], j as f32 / (n - 1) as f32);
            let y = va.height(x, z);
            // Tufts of grass creep out over the bars' edges, so grass and
            // gravel meet in a ragged margin rather than a cut line.
            let tuft = noise::fbm(x / 0.9, z / 0.9, va.seed ^ 0x6b, 2, 2.0, 0.5);
            let bare = y < wl + 0.08 || va.bar_edge(x, z, y) < 0.15 - 0.9 * tuft;
            h.push(if bare { y - 3.0 } else { y + at[1] + 0.01 });
        }
    }
    let patch = evani_grass::Patch::new(evani_grass::Kind::Meadow, [at[0] + min[0], at[2] + min[1]], [at[0] + max[0], at[2] + max[1]]).feather(0.05).density(0.8);
    (patch, n, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        Values::defaults(&PARAMS)
    }

    fn tris(o: &Output) -> usize {
        match o {
            Output::Parts(p) => p.iter().map(|x| x.mesh.indices.len() / 3).sum(),
            Output::Mesh(m) => m.indices.len() / 3,
            Output::Module => 0,
        }
    }

    /// The water runs in the channel, the floodplain stands over it, and a
    /// bend's outside is deep while its inside is shallow (the bar).
    #[test]
    fn channel_under_the_water_and_banks_over_it() {
        let v = values();
        let va = Valley::new(&v, 1);
        let wl = va.water_level();
        // On the line, mid-river.
        let p = va.line()[va.line().len() / 2];
        assert!(va.height(p[0], p[1]) < wl - 0.3, "mid-river {}", va.height(p[0], p[1]));
        // Well away from the river, on the plain.
        assert!(va.height(150.0, 150.0) > wl + 0.5);
        // A bend: find the point of strongest curvature, then compare the
        // bed a third of the width out on either side.
        let (mut best, mut bi) = (0.0f32, 0usize);
        for (i, c) in va.curv.iter().enumerate() {
            let pt = va.pts[i];
            if c.abs() > best && pt[0].abs() < 250.0 && pt[1].abs() < 250.0 {
                best = c.abs();
                bi = i;
            }
        }
        let (pt, t, c) = (va.pts[bi], va.tangent(bi), va.curv[bi]);
        let nrm = [-t[1], t[0]];
        let inner = c.signum() * va.hw * 0.6;
        let at = |o: f32| va.height(pt[0] + nrm[0] * o, pt[1] + nrm[1] * o);
        assert!(at(-inner) < at(inner), "outer {} inner {}", at(-inner), at(inner));
        // Nothing under the base.
        assert!(va.height(pt[0], pt[1]) >= 0.0);
    }

    /// It builds the ground's bands, boulders and trees, within a budget at
    /// High and lighter on Low, the same each run.
    #[test]
    fn river_builds_light_and_repeatable() {
        let high = generate(&values().with_density(1.0), 3);
        let low = generate(&values().with_density(evani_procedural::detail::Density::for_tier("low")), 3);
        let Output::Parts(parts) = &high else { panic!("parts") };
        assert!(parts.len() >= 10, "{} parts", parts.len());
        let (h, l) = (tris(&high), tris(&low));
        println!("rivers: {h} triangles at High, {l} at Low");
        let each: Vec<usize> = parts.iter().map(|p| p.mesh.indices.len() / 3).collect();
        println!("per part: {each:?}");
        assert!(h < 1_600_000 && l <= h, "{h} / {l}");
        assert_eq!(h, tris(&generate(&values().with_density(1.0), 3)));
    }
}
