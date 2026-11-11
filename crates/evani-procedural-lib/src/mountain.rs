//! Mountains (`terrain-mountains`, Asset Studio column 19, 2026-11-07): a
//! photoreal mountain massif at game scale. A ridged massif with a domed,
//! snow-capped summit rises from rolling foothills; simulated rain (droplet
//! hydraulic erosion, after H. T. Beyer, "Implementation of a method for
//! hydraulic erosion", 2015; written fresh here) carves gullies down its
//! flanks and lays fans of debris at their feet, so the ridges, gullies and
//! fans are real geometry.
//!
//! The ground cover follows the land: snow on the high, gentler ground and
//! down the gullies, bare rock on the steep faces and the high crests, pale
//! scree on the upper slopes and the debris fans, alpine grass just over the
//! tree line, conifer forest (real low-poly spruces over a dark forest floor)
//! on the lower slopes and the foothills, with meadow clearings and
//! boulders. Everything is in catalogue materials (the stone, ground,
//! foliage and ice recipes), so a game that lists them draws what the
//! library shows.
//!
//! The default is a real mountain: 4.4 km across, its summit 1.2 km over the
//! foothills' base, which sits at y = 0 and settles flat at the square's
//! edge, so it joins a world (or another copy of itself, turned).

use evani_procedural::surface::presets as sp;
use evani_procedural::{Output, ParamSpec as P, Part, Surface, Values, geo, noise, shapes as sh};

type Mesh = evani_render::mesh::Mesh;

/// The default rock colour (linear): weathered pale grey granite (round 2:
/// cooler, the reference's blue-grey faces).
pub const ROCK: [f32; 3] = [0.28, 0.285, 0.295];
/// The strata's darker beds and the path's packed soil (linear).
const ROCK_BED: [f32; 3] = [0.24, 0.245, 0.258];
const PATH_SOIL: [f32; 3] = [0.15, 0.12, 0.09];

pub const PARAMS: [P; 13] = [
    P::num("size", 4400.0, 100.0, 12000.0).unit("m").describe("Width of the square of land the mountain stands in (m)"),
    P::num("height", 1200.0, 10.0, 4000.0).unit("m").describe("Summit height over the foothills' base (m)"),
    P::num("ruggedness", 0.6, 0.0, 1.0).describe("How sharp and broken the ridges are"),
    P::color("color", ROCK).describe("Rock colour"),
    P::num("rock", 0.55, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the high ground (1)"),
    P::num("snow", 0.45, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)"),
    P::num("erosion", 0.6, 0.0, 1.0).describe("How deeply rain has carved gullies and fans into the slopes (0: none)"),
    P::num("forest", 0.7, 0.0, 1.0).describe("Conifer forest on the foothills and lower slopes (0: bare)"),
    P::num("tree_line", 0.36, 0.05, 0.95).describe("Where the forest stops, as a share of the summit height"),
    P::num("focus_x", 0.0, -6000.0, 6000.0).unit("m").describe("Where a player stands (x, m from the middle): round it the forest is full, tree by tree, with boulders and shrubs"),
    P::num("focus_z", 0.0, -6000.0, 6000.0).unit("m").describe("Where a player stands (z, m from the middle)"),
    P::num("focus_radius", 0.0, 0.0, 600.0).unit("m").describe("How far round the player's point the forest is full (0: off, the trees spread evenly)"),
    P::num("horizon", 0.0, 0.0, 40000.0)
        .unit("m")
        .describe("Land round the square out to this far from the middle (m; 0: none): the foothills run on into hazy ranges, so the massif stands in a landscape"),
];

/// The library's triangle limit for one asset (src/tests.rs), less a margin.
const BUDGET: usize = 385_000;
/// The longest triangle edge (m) along the snow's edge (round 3).
const EDGE_REFINE: f32 = 16.0;
/// Triangles in one far spruce ([`spruce`]; round 3: 26, was 44).
const TREE_TRIS: usize = 26;

/// Ground-cover albedos (linear).
const ALPINE: [f32; 3] = [0.075, 0.1, 0.035];
/// The forest seen from over it: the dark needle canopy.
const FOREST_FLOOR: [f32; 3] = [0.011, 0.028, 0.013];
/// Round 3: darker and less yellow (a blotchy carpet at a walker's feet).
const MEADOW: [f32; 3] = [0.034, 0.064, 0.022];
/// Snow in the sun: a shade under white (linear).
const SNOW_WHITE: [f32; 3] = [0.68, 0.71, 0.76];
/// Shrubs round a player's point: juniper and dwarf pine greens (round 3:
/// the leaf colour of their sprays).
const SHRUB: [f32; 3] = [0.045, 0.085, 0.025];
/// Round 3: the walker's boulders, pale grey under grey-green lichen.
const LICHEN_ROCK: [f32; 3] = [0.26, 0.26, 0.25];
/// Round 3: the meadow's flowers: buttercup yellow, white, a purple.
const FLOWERS: [[f32; 3]; 3] = [[0.62, 0.42, 0.02], [0.62, 0.62, 0.58], [0.22, 0.1, 0.36]];
/// Round 1: a shade darker (the pale beds read as snow from the air).
const SCREE: [f32; 3] = [0.17, 0.168, 0.16];
const BARK: [f32; 3] = [0.08, 0.055, 0.04];
/// Needle tints: dark green, blue-green, olive.
/// Round 2: the olive tint cooled (lit, it read orange beside the others).
/// Round 3: darker and bluer (lit, round 2's read olive).
const NEEDLES: [[f32; 3]; 3] = [[0.016, 0.036, 0.02], [0.013, 0.033, 0.028], [0.02, 0.038, 0.021]];

/// The cover classes, in part order (round 2: the strata's darker beds and
/// the player's path and gravel apart).
const SNOW: usize = 0;
const ROCKS: usize = 1;
const SCREE_C: usize = 2;
const ALPINE_C: usize = 3;
const FOREST: usize = 4;
const MEADOW_C: usize = 5;
const ROCK_DARK: usize = 6;
const PATH: usize = 7;
const CLASSES: usize = 8;

/// The slider mix the cover reads.
#[derive(Debug, Clone, Copy)]
struct Mix {
    rock: f32,
    snow: f32,
    forest: f32,
    line: f32,
    /// A player's point and radius (m), if set.
    focus: Option<(f32, f32, f32)>,
}

impl Mix {
    fn of(v: &Values) -> Self {
        let r = v.f("focus_radius").max(0.0);
        let focus = (r > 1.0).then(|| (v.f("focus_x"), v.f("focus_z"), r));
        Self { rock: v.f("rock").clamp(0.0, 1.0), snow: v.f("snow").clamp(0.0, 1.0), forest: v.f("forest").clamp(0.0, 1.0), line: v.f("tree_line").clamp(0.02, 1.0), focus }
    }
}

/// The land as a grid: `n` points a side over `size` m, heights (m over
/// the base) and what erosion did at each (m; + debris laid, - carved).
pub struct Land {
    pub size: f32,
    pub n: usize,
    pub h: Vec<f32>,
    pub dep: Vec<f32>,
    /// The summit's height (m).
    pub top: f32,
    /// How hollow the ground is at each point (+ gullies and bowls, where
    /// snow drifts in and lasts; - ribs and crests the wind scours).
    curv: Vec<f32>,
    /// Round 3: how near each grid point is to a ridge's crest (0..1): the
    /// rock runs down the arms and spurs, the forest climbs the hollows.
    spine: Vec<f32>,
    net: Net,
    peak: [f32; 3],
    seed: u32,
    /// Round 3: the land past the square (`horizon`, m from the middle; 0
    /// none), the ruggedness and the metres per unit of [`shape`] there.
    horizon: f32,
    rug: f32,
    unit: f32,
}

/// Ridged multifractal noise in about [0, 0.7] (after F. K. Musgrave): sharp
/// crests, each finer octave weighted by the one before, so the detail
/// gathers on the crests and the hollows stay smooth.
fn rmf(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut weight) = (0.0, 1.0, 0.0, 1.0f32);
    let (mut px, mut py) = (x, y);
    for o in 0..octaves {
        let s = 1.0 - noise::perlin2(px, py, seed.wrapping_add(o * 7919)).abs();
        let s = s * s * weight;
        weight = (s * 2.0).clamp(0.0, 1.0);
        sum += s * amp;
        norm += amp;
        amp *= 0.5;
        let (rx, ry) = (0.8 * px - 0.6 * py, 0.6 * px + 0.8 * py);
        px = rx * 2.0 + 17.3;
        py = ry * 2.0 - 9.1;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// One ridge of the massif in plan (the square's half widths, [-1, 1]): a
/// polyline from the summit (or from a point on a bigger ridge) outwards,
/// its crest height from `h0` at the start to `h1` at the end and its half
/// width from `w0` to `w1` (round 2: the massif is a branching ridge
/// network, arêtes with cirques between them, not a fluted cone).
#[derive(Debug, Clone)]
struct Ridge {
    pts: Vec<[f32; 2]>,
    /// Cumulative length at each point.
    at: Vec<f32>,
    h0: f32,
    h1: f32,
    w0: f32,
    w1: f32,
    seed: u32,
}

impl Ridge {
    fn new(from: [f32; 2], angle: f32, len: f32, (h0, h1): (f32, f32), (w0, w1): (f32, f32), seed: u32) -> Self {
        let steps = 7;
        let mut pts = vec![from];
        let mut at = vec![0.0];
        let (mut a, mut p) = (angle, from);
        for k in 0..steps {
            a += (sh::rnd(seed, k, 3) - 0.5) * 0.5;
            let l = len / steps as f32;
            p = [p[0] + a.cos() * l, p[1] + a.sin() * l];
            pts.push(p);
            at.push(l * (k + 1) as f32);
        }
        Self { pts, at, h0, h1, w0, w1, seed }
    }

    fn len(&self) -> f32 {
        self.at.last().copied().unwrap_or(0.0)
    }

    /// The point `t` (0..1) along the crest.
    fn point(&self, t: f32) -> [f32; 2] {
        let s = t.clamp(0.0, 1.0) * self.len();
        for k in 1..self.pts.len() {
            if s <= self.at[k] || k + 1 == self.pts.len() {
                let f = ((s - self.at[k - 1]) / (self.at[k] - self.at[k - 1]).max(1e-6)).clamp(0.0, 1.0);
                let (a, b) = (self.pts[k - 1], self.pts[k]);
                return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
            }
        }
        self.pts[0]
    }

    /// The crest's height `t` (0..1) along: falling away concave, with
    /// peaks and saddles along it (gendarmes and cols).
    fn crest(&self, t: f32) -> f32 {
        // Round 3: the crest holds its height further out (a broad massif,
        // not a cone), with peaks and cols along it.
        let base = self.h1 + (self.h0 - self.h1) * (1.0 - t.powf(2.1));
        base * (1.0 + 0.08 * noise::perlin2(t * 6.0, 0.5, self.seed ^ 0x1d) * (1.0 - t))
    }

    /// Round 3: how near plan point (`u`, `w`) is to this ridge's crest: 1
    /// on it, 0 at its foot and past its end.
    fn closeness(&self, u: f32, w: f32) -> f32 {
        let (mut best, mut tb) = (f32::MAX, 0.0);
        let total = self.len().max(1e-6);
        for k in 1..self.pts.len() {
            let (a, b) = (self.pts[k - 1], self.pts[k]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = (dx * dx + dz * dz).max(1e-9);
            let f = (((u - a[0]) * dx + (w - a[1]) * dz) / l2).clamp(0.0, 1.0);
            let d = ((u - a[0] - dx * f).powi(2) + (w - a[1] - dz * f).powi(2)).sqrt();
            if d < best {
                best = d;
                tb = (self.at[k - 1] + f * l2.sqrt()) / total;
            }
        }
        if tb >= 0.999 {
            return 0.0;
        }
        let width = self.w0 + (self.w1 - self.w0) * tb;
        (1.0 - best / width.max(1e-4)).max(0.0).powi(4)
    }

    /// The ridge's height at plan point (`u`, `w`): the crest's falling
    /// away to both sides, steep near the crest and easing to its foot.
    fn height(&self, u: f32, w: f32) -> f32 {
        let (mut best, mut tb) = (f32::MAX, 0.0);
        let total = self.len().max(1e-6);
        for k in 1..self.pts.len() {
            let (a, b) = (self.pts[k - 1], self.pts[k]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = (dx * dx + dz * dz).max(1e-9);
            let f = (((u - a[0]) * dx + (w - a[1]) * dz) / l2).clamp(0.0, 1.0);
            let (px, pz) = (a[0] + dx * f, a[1] + dz * f);
            let d = ((u - px).powi(2) + (w - pz).powi(2)).sqrt();
            if d < best {
                best = d;
                tb = (self.at[k - 1] + f * l2.sqrt()) / total;
            }
        }
        // Past the ridge's end it rounds off.
        let end = if tb >= 0.999 {
            let e = self.pts[self.pts.len() - 1];
            ((u - e[0]).powi(2) + (w - e[1]).powi(2)).sqrt()
        } else {
            0.0
        };
        let width = self.w0 + (self.w1 - self.w0) * tb;
        let x = ((best.max(end)) / width.max(1e-4)).min(1.0);
        // Round 3: thick ridges: a sharp crest on broad shoulders (half a
        // concave fall, half a smooth one), not a thin fin.
        let fall = 0.5 * (1.0 - x).powf(1.5) + 0.5 * (1.0 - x * x * (3.0 - 2.0 * x));
        self.crest(tb) * fall
    }
}

/// The massif's ridges for a seed: five or six main arêtes from the summit,
/// each with one or two side ridges, uneven in angle, length and height.
fn ridges(seed: u32) -> (Vec<Ridge>, Vec<[f32; 4]>) {
    let summit = SUMMIT;
    let n = 5 + (sh::rnd(seed, 1, 90) * 2.0) as u32;
    let turn = sh::rnd(seed, 2, 90) * sh::TAU;
    let mut out = Vec::new();
    let mut cirques = Vec::new();
    let mut angles = Vec::new();
    for i in 0..n {
        let a = turn + (i as f32 + (sh::rnd(seed, i, 91) - 0.5) * 0.55) * sh::TAU / n as f32;
        angles.push(a);
    }
    for (i, a) in angles.iter().enumerate() {
        let i = i as u32;
        // Round 3: longer, thicker arms of uneven height (some shoulders
        // low, so the crown stands broad over them rather than as a spire
        // where every arête meets).
        let len = 0.7 + 0.3 * sh::rnd(seed, i, 92);
        let h0 = 0.7 + 0.2 * sh::rnd(seed, i, 93);
        let main = Ridge::new(summit, *a, len, (h0, 0.06), (0.15, 0.44 + 0.1 * sh::rnd(seed, i, 94)), seed ^ (i * 977));
        // Side ridges: two or three, branching off along the arm to one side
        // or the other, each with a spur of its own: a branching network.
        let sides = 2 + (sh::rnd(seed, i, 95) * 1.6) as u32;
        for s in 0..sides {
            let t = 0.22 + 0.24 * s as f32 + 0.08 * sh::rnd(seed, i * 7 + s, 96);
            let side = if sh::rnd(seed, i * 7 + s, 97) < 0.5 { -1.0 } else { 1.0 };
            let ba = a + side * (0.5 + 0.45 * sh::rnd(seed, i * 7 + s, 98));
            let from = main.point(t);
            let bh = main.crest(t) * 0.93;
            let bl = (0.26 + 0.26 * sh::rnd(seed, i * 7 + s, 99)) * (1.0 - 0.45 * t);
            let branch = Ridge::new(from, ba, bl, (bh, 0.04), (0.11, 0.3), seed ^ (i * 977 + s * 31 + 5));
            let st = 0.45 + 0.2 * sh::rnd(seed, i * 7 + s, 101);
            let sa = ba - side * (0.6 + 0.4 * sh::rnd(seed, i * 7 + s, 102));
            out.push(Ridge::new(branch.point(st), sa, bl * 0.5, (branch.crest(st) * 0.92, 0.03), (0.07, 0.18), seed ^ (i * 977 + s * 31 + 17)));
            out.push(branch);
        }
        out.push(main);
        // A cirque between this arête and the next: a bowl under the summit,
        // lopsided (longer down the fall line than across it), its floor at
        // its own height.
        let next = angles[(i as usize + 1) % angles.len()];
        let mut gap = next - a;
        while gap < 0.0 {
            gap += sh::TAU;
        }
        if gap > 0.7 {
            let m = a + gap * (0.4 + 0.2 * sh::rnd(seed, i, 103));
            let r = 0.22 + 0.1 * sh::rnd(seed, i, 100);
            cirques.push([summit[0] + m.cos() * r, summit[1] + m.sin() * r, m, 0.15 + 0.06 * sh::rnd(seed, i, 104)]);
        }
    }
    (out, cirques)
}

/// The summit's plan point (the square's half widths). Round 3: further
/// back (north), so the foothills and the valley in front are wider and a
/// player at the square's south edge sees the massif from 2.4 km.
const SUMMIT: [f32; 2] = [0.04, -0.2];

/// The massif's plan: its ridges, the cirques between them and the line of
/// the main drainage valley (round 2).
struct Net {
    ridges: Vec<Ridge>,
    /// Cirques: centre (u, w), the fall line's angle and the bowl's radius.
    cirques: Vec<[f32; 4]>,
    valley: Vec<[f32; 2]>,
    /// Round 3: side streams joining the main valley from the massif.
    tribs: Vec<Vec<[f32; 2]>>,
}

/// Distance from (`u`, `w`) to the polyline `line`, and how far along it
/// (its `w`).
fn to_line(line: &[[f32; 2]], u: f32, w: f32) -> (f32, f32) {
    let (mut best, mut along) = (f32::MAX, 0.0);
    for k in 1..line.len() {
        let (a, b) = (line[k - 1], line[k]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let f = (((u - a[0]) * dx + (w - a[1]) * dz) / (dx * dx + dz * dz).max(1e-9)).clamp(0.0, 1.0);
        let d = ((u - a[0] - dx * f).powi(2) + (w - a[1] - dz * f).powi(2)).sqrt();
        if d < best {
            best = d;
            along = a[1] + dz * f;
        }
    }
    (best, along)
}

impl Net {
    fn new(seed: u32) -> Self {
        let (ridges, cirques) = ridges(seed);
        // The drainage valley: from the massif's foot on the cameras' side
        // (+w) winding out past the square's edge. Round 3: more bends, and
        // on past the square (the land round it, `horizon`).
        let phase = sh::rnd(seed, 3, 90) * sh::TAU;
        let bend = |w: f32| 0.06 - 0.12 * (w - 0.28) + 0.08 * (w * 7.0 + phase).sin() + 0.03 * (w * 17.0 + 2.0 * phase).sin();
        let valley = (0..40).map(|k| {
            let w = 0.28 + k as f32 * 0.05;
            [bend(w) - 0.2 * (w - 1.0).max(0.0), w]
        }).collect();
        // Two side streams from the massif's foot, one each side, curving
        // down into the main valley.
        let tribs = [-1.0f32, 1.0]
            .iter()
            .enumerate()
            .map(|(i, side)| {
                let w0 = 0.22 + 0.1 * sh::rnd(seed, i as u32, 105);
                let w1 = 0.62 + 0.18 * sh::rnd(seed, i as u32, 106);
                let u0 = side * (0.36 + 0.14 * sh::rnd(seed, i as u32, 107));
                (0..9)
                    .map(|k| {
                        let f = k as f32 / 8.0;
                        let w = w0 + (w1 - w0) * f;
                        let target = bend(w);
                        [u0 + (target - u0) * f.powf(1.6) + 0.025 * (f * 9.0 + phase + i as f32).sin(), w]
                    })
                    .collect()
            })
            .collect();
        Self { ridges, cirques, valley, tribs }
    }

    /// Distance from (`u`, `w`) to the valley's line, and how far along it
    /// (its `w`).
    fn valley(&self, u: f32, w: f32) -> (f32, f32) {
        to_line(&self.valley, u, w)
    }

    /// Distance from (`u`, `w`) to the nearest side stream (round 3).
    fn trib(&self, u: f32, w: f32) -> f32 {
        self.tribs.iter().map(|t| to_line(t, u, w).0).fold(f32::MAX, f32::min)
    }
}

/// The strata's tilt and wobble at plan point (`u`, `w`) (height units).
fn tilt(u: f32, w: f32, seed: u32) -> f32 {
    0.035 * u - 0.02 * w + 0.012 * noise::perlin2(u * 6.0, w * 6.0, seed ^ 0x5e)
}

/// The height between strata (height units: the summit is about 1).
/// Round 3: closer beds, so the faces step in more, lower cliff bands.
const STEP: f32 = 0.06;

/// The massif's shape at plan point (`u`, `w`) in [-1, 1] (the square's
/// half widths): unscaled height. Round 2: a broad snow dome on top of a
/// branching network of arêtes, cirques carved between them under the
/// summit, tilted strata weathered into cliff bands and ledges, broken rock
/// over it, rolling foothills cut by a winding drainage valley. Round 3: a
/// broad crown instead of a spire where the arêtes meet, thick arms with
/// spurs, lopsided cirques, joints breaking the faces into ribs, stronger
/// cliff steps, a wider valley floor and side streams. `settle` flattens the
/// square's border (so copies meet edge to edge); without it the land runs
/// on past the square (`horizon`).
fn shape(u: f32, w: f32, rug: f32, seed: u32, net: &Net, settle: bool) -> f32 {
    let (wu, ww) = noise::warp(u, w, seed ^ 0x3d1, 1.1, 0.04);
    let smax = |a: f32, b: f32, k: f32| {
        let h = (0.5 + 0.5 * (a - b) / k).clamp(0.0, 1.0);
        b + (a - b) * h + k * h * (1.0 - h)
    };
    let smin = |a: f32, b: f32, k: f32| -smax(-a, -b, k);
    // The crown (round 3): a broad, gently domed snowfield about 500 m
    // across before it falls away steeply, its foot spreading wide.
    let s = SUMMIT;
    let r = ((wu - s[0]).powi(2) + (ww - s[1]).powi(2)).sqrt();
    let mut m = 1.0 / (1.0 + (r / 0.23).powi(4)).sqrt();
    for rd in &net.ridges {
        m = smax(m, rd.height(wu, ww), 0.04);
    }
    // Cirques: amphitheatres under the summit, steep back walls (towards
    // the summit) over flatter floors that open out down the fall line.
    for c in &net.cirques {
        let (dx, dz) = (wu - c[0], ww - c[1]);
        let (ca, sa) = (c[2].cos(), c[2].sin());
        let along = dx * ca + dz * sa;
        let across = -dx * sa + dz * ca;
        let a = if along > 0.0 { along / 1.7 } else { along / 0.85 };
        let d = (a * a + across * across).sqrt() / c[3];
        if d < 1.8 {
            let floor = 0.4 + (c[3] - 0.15) * 1.5;
            m = smin(m, floor + 0.5 * d * d, 0.05);
        }
    }
    let t = m.clamp(0.0, 1.0);
    // Broken rock: ridged detail strongest on the mid flanks, crags; the
    // crown kept smooth under its snow.
    let ridged = rmf(wu * 3.4, ww * 3.4, seed ^ 0x77, 7);
    let rw = rug * (0.12 + 0.55 * (1.0 - t * t)) * noise::smoothstep(0.04, 0.3, t);
    let crags = rmf(wu * 9.0, ww * 9.0, seed ^ 0x7b, 5) * noise::smoothstep(0.12, 0.45, t) * (1.0 - 0.85 * t * t);
    let mut massif = m * (1.0 - 0.3 * rw + 0.6 * rw * ridged) + 0.045 * rug * crags;
    // Round 3: joints, two sets of fractures across the faces, weathered
    // into sharp ribs and gullies about 160 m apart, one set or the other
    // in patches (breaking the smooth planar folds).
    let face = noise::smoothstep(0.2, 0.38, t) * (1.0 - noise::smoothstep(0.8, 0.94, t));
    if face > 0.0 {
        let j1 = 1.0 - ((wu * 0.83 + ww * 0.56) * 40.0 + 2.5 * noise::perlin2(wu * 5.0, ww * 5.0, seed ^ 0x3a)).sin().abs();
        let j2 = 1.0 - ((ww * 0.92 - wu * 0.39) * 30.0 + 2.5 * noise::perlin2(wu * 4.0, ww * 4.0, seed ^ 0x3b)).sin().abs();
        let mix = noise::smoothstep(-0.25, 0.25, noise::perlin2(wu * 7.0, ww * 7.0, seed ^ 0x3c));
        massif += rug * face * 0.035 * (j1 * j1 * mix + j2 * j2 * (1.0 - mix) - 0.35);
    }
    // Strata: tilted beds weathered into cliff bands and ledges.
    let tl = tilt(wu, ww, seed);
    let x = (massif + tl) / STEP;
    let terraced = (x.floor() + noise::smoothstep(0.45, 1.0, x.fract())) * STEP - tl;
    let band = noise::smoothstep(0.18, 0.34, t) * (1.0 - noise::smoothstep(0.82, 0.94, t));
    massif += (terraced - massif) * 0.6 * band;
    // Rolling, wooded foothills all round, more deeply cut.
    let foot = 0.075 * evani_procedural::land::rolling(u * 4.0, w * 4.0, seed ^ 0x2f, 5, 0.5) + 0.035 * rmf(u * 7.0, w * 7.0, seed ^ 0x91, 4);
    let mut h = massif + foot * (1.0 - 0.5 * t);
    // The drainage valley: a winding trough with a flat floor (round 3:
    // about 50 m wide, gravel bars and green islands on it), fading in at
    // the massif's foot.
    let (dv, along) = net.valley(u, w);
    let trough = 0.006 + 0.75 * (dv - 0.012).max(0.0).powf(1.25);
    let fade = noise::smoothstep(0.3, 0.46, along);
    h += (smin(h, trough, 0.02) - h) * fade;
    // Round 3: the side streams' ravines, cut into the foothills.
    let dt = net.trib(u, w);
    h -= 0.03 * noise::smoothstep(0.05, 0.0, dt) * (1.0 - 0.6 * t);
    if settle {
        // Settled flat at the square's edge.
        h * noise::smoothstep(1.0, 0.84, u.abs().max(w.abs()))
    } else {
        h.max(0.0)
    }
}

/// Rain on the height grid `hm` (`n` a side, heights about 0..1): `drops`
/// droplets, each running downhill with a little inertia, picking up soil
/// where it speeds up (from a brush three cells round) and dropping it where
/// it slows or climbs. Deterministic (seeded).
fn erode(hm: &mut [f32], n: usize, drops: usize, seed: u32) {
    const INERTIA: f32 = 0.05;
    const CAPACITY: f32 = 4.0;
    const MIN_CAP: f32 = 0.01;
    const ERODE: f32 = 0.3;
    const DEPOSIT: f32 = 0.3;
    const EVAPORATE: f32 = 0.015;
    const GRAVITY: f32 = 4.0;
    const LIFE: usize = 40;
    const RADIUS: i32 = 3;
    if n < 8 {
        return;
    }
    let mut brush: Vec<(i32, i32, f32)> = Vec::new();
    for oy in -RADIUS..=RADIUS {
        for ox in -RADIUS..=RADIUS {
            let d = ((ox * ox + oy * oy) as f32).sqrt();
            if d < RADIUS as f32 {
                brush.push((ox, oy, RADIUS as f32 - d));
            }
        }
    }
    let total: f32 = brush.iter().map(|b| b.2).sum();
    for b in &mut brush {
        b.2 /= total.max(1e-6);
    }
    let sample = |hm: &[f32], x: f32, y: f32| -> (f32, f32, f32) {
        let (xi, yi) = (x as usize, y as usize);
        let (fx, fy) = (x - xi as f32, y - yi as f32);
        let i = yi * n + xi;
        let (a, b, c, d) = (hm[i], hm[i + 1], hm[i + n], hm[i + n + 1]);
        let gx = (b - a) * (1.0 - fy) + (d - c) * fy;
        let gy = (c - a) * (1.0 - fx) + (d - b) * fx;
        (a * (1.0 - fx) * (1.0 - fy) + b * fx * (1.0 - fy) + c * (1.0 - fx) * fy + d * fx * fy, gx, gy)
    };
    let lim = (n - 2) as f32;
    for k in 0..drops as u32 {
        let mut x = 1.0 + sh::rnd(seed, k, 1) * (lim - 1.0);
        let mut y = 1.0 + sh::rnd(seed, k, 2) * (lim - 1.0);
        let (mut dx, mut dy, mut speed, mut water, mut sed) = (0.0f32, 0.0f32, 1.0f32, 1.0f32, 0.0f32);
        for _ in 0..LIFE {
            let (xi, yi) = (x as usize, y as usize);
            let (fx, fy) = (x - xi as f32, y - yi as f32);
            let (h, gx, gy) = sample(hm, x, y);
            dx = dx * INERTIA - gx * (1.0 - INERTIA);
            dy = dy * INERTIA - gy * (1.0 - INERTIA);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-9 {
                break;
            }
            dx /= len;
            dy /= len;
            x += dx;
            y += dy;
            if x < 1.0 || y < 1.0 || x >= lim || y >= lim {
                break;
            }
            let (nh, _, _) = sample(hm, x, y);
            let dh = nh - h;
            let cap = (-dh).max(MIN_CAP) * speed * water * CAPACITY;
            let i = yi * n + xi;
            if sed > cap || dh > 0.0 {
                let amt = if dh > 0.0 { dh.min(sed) } else { (sed - cap) * DEPOSIT };
                sed -= amt;
                hm[i] += amt * (1.0 - fx) * (1.0 - fy);
                hm[i + 1] += amt * fx * (1.0 - fy);
                hm[i + n] += amt * (1.0 - fx) * fy;
                hm[i + n + 1] += amt * fx * fy;
            } else {
                let amt = ((cap - sed) * ERODE).min(-dh);
                for &(ox, oy, wgt) in &brush {
                    let (bx, by) = (xi as i32 + ox, yi as i32 + oy);
                    if bx < 0 || by < 0 || bx >= n as i32 || by >= n as i32 {
                        continue;
                    }
                    let j = by as usize * n + bx as usize;
                    let d = (amt * wgt).min(hm[j]);
                    hm[j] -= d;
                    sed += d;
                }
            }
            speed = (speed * speed - dh * GRAVITY).max(0.0).sqrt();
            water *= 1.0 - EVAPORATE;
        }
    }
}

impl Land {
    /// The land for these values (the grid finer on the higher tiers).
    pub fn new(v: &Values, seed: u32) -> Self {
        let size = v.f("size").max(1.0);
        let top = v.f("height").max(0.1);
        // Round 3: an even count, so the foothills' half-resolution grid
        // (generate) lands on the land's own points.
        let cells = ((160.0 + 96.0 * v.density().min(1.0)) as usize).clamp(96, 256) & !1;
        let n = cells + 1;
        let rug = v.f("ruggedness").clamp(0.0, 1.0);
        let net = Net::new(seed);
        // Round 3: with land round the square, its border runs on into it
        // instead of settling flat.
        let horizon = v.f("horizon").max(0.0);
        let horizon = if horizon > size * 0.5 + 1.0 { horizon } else { 0.0 };
        let mut hm = vec![0.0f32; n * n];
        let mut spine = vec![0.0f32; n * n];
        for j in 0..n {
            for i in 0..n {
                let (u, w) = (i as f32 / cells as f32 * 2.0 - 1.0, j as f32 / cells as f32 * 2.0 - 1.0);
                hm[j * n + i] = shape(u, w, rug, seed, &net, horizon <= 0.0);
                let (wu, ww) = noise::warp(u, w, seed ^ 0x3d1, 1.1, 0.04);
                spine[j * n + i] = net.ridges.iter().map(|r| r.closeness(wu, ww)).fold(0.0, f32::max);
            }
        }
        // The summit at 1 (heights about 0..1 for the rain).
        let hi0 = hm.iter().copied().fold(0.0f32, f32::max).max(1e-4);
        for p in &mut hm {
            *p = (*p / hi0).max(0.0);
        }
        let before = hm.clone();
        // Round 2: less rain, so the ridges, cliff bands and cirques stay
        // crisp (round 1's rain wore the flanks into smooth curtain folds).
        let drops = (v.f("erosion").clamp(0.0, 1.0) * 0.9 * (n * n) as f32) as usize;
        erode(&mut hm, n, drops, seed ^ 0xe7);
        // Back to metres, the summit at `height` again.
        let hi = hm.iter().copied().fold(0.0f32, f32::max).max(1e-4);
        let k = top / hi;
        let dep = hm.iter().zip(&before).map(|(a, b)| (a - b) * k).collect();
        let h: Vec<f32> = hm.iter().map(|p| p.max(0.0) * k).collect();
        // The hollowness: the grid's Laplacian over two cells, as a share
        // of the summit's height, softened once.
        let mut lap = vec![0.0f32; n * n];
        for j in 2..n - 2 {
            for i in 2..n - 2 {
                let c = h[j * n + i];
                lap[j * n + i] = (h[j * n + i - 2] + h[j * n + i + 2] + h[(j - 2) * n + i] + h[(j + 2) * n + i] - 4.0 * c) / top;
            }
        }
        let mut curv = lap.clone();
        for j in 1..n - 1 {
            for i in 1..n - 1 {
                curv[j * n + i] = (lap[j * n + i] * 4.0 + lap[j * n + i - 1] + lap[j * n + i + 1] + lap[(j - 1) * n + i] + lap[(j + 1) * n + i]) / 8.0;
            }
        }
        let (mut best, mut at) = (f32::MIN, 0usize);
        for (i, y) in h.iter().enumerate() {
            if *y > best {
                best = *y;
                at = i;
            }
        }
        let c = (n - 1) as f32;
        let peak = [((at % n) as f32 / c - 0.5) * size, best, ((at / n) as f32 / c - 0.5) * size];
        Self { size, n, h, dep, top, curv, spine, net, peak, seed, horizon, rug, unit: k / hi0 }
    }

    /// Round 3: the land's height (m) at (x, z) past the square, out to
    /// `horizon`: the foothills and the valley running on (the same shape,
    /// unsettled, without the rain), rising into ranges of their own, higher
    /// and wilder further out, the valley's line left open in front, and one
    /// big massif far behind.
    fn far_height(&self, x: f32, z: f32) -> f32 {
        let k = 2.0 / self.size;
        let base = shape(x * k, z * k, self.rug, self.seed, &self.net, false) * self.unit;
        let half = self.size * 0.5;
        let out = (x.abs() - half).max(z.abs() - half).max(0.0);
        let ramp = noise::smoothstep(250.0, 5000.0, out);
        if ramp <= 0.0 {
            return base;
        }
        let (u, w) = (x / 5200.0, z / 5200.0);
        let (wu, ww) = noise::warp(u, w, self.seed ^ 0x9a1, 0.8, 0.4);
        let ridged = rmf(wu, ww, self.seed ^ 0x9a2, 6);
        // Which ranges stand high: broad swells of their own.
        let broad = 0.5 + 0.5 * noise::perlin2(u * 0.3 + 3.1, w * 0.3, self.seed ^ 0x9a3);
        // The valley's corridor stays open towards the cameras' side.
        let (dv, along) = self.net.valley(x * k, z * k);
        let open = if along > 0.9 { 0.25 + 0.75 * noise::smoothstep(0.08, 0.9, dv) } else { 1.0 };
        let (bx, bz) = ((x - 2500.0) / 6500.0, (z + 21000.0) / 6500.0);
        let big = (-(bx * bx + bz * bz)).exp();
        base + self.top * ramp * (open * (0.2 + 0.85 * broad) * 1.7 * ridged + 1.05 * big * (0.55 + 0.6 * ridged))
    }

    /// Round 3: the land round the square out to `horizon` as one mesh (no
    /// trees: from this far the forest is its canopy), its cells growing
    /// with the distance (the square's sixteenth next to it, 8% more each
    /// step out), with a 40 m curtain down the square's own border so no gap
    /// shows between the two.
    fn far_land(&self) -> Mesh {
        let half = self.size * 0.5;
        let reach = self.horizon.max(half + 1.0);
        let cell = self.size / 16.0;
        let mut out = Vec::new();
        let (mut x, mut step) = (half, cell);
        while x < reach - 1.0 {
            x = (x + step).min(reach);
            out.push(x);
            step *= 1.08;
        }
        let mut lines: Vec<f32> = out.iter().rev().map(|x| -x).collect();
        lines.extend((0..=16).map(|i| -half + cell * i as f32));
        lines.extend(out.iter().copied());
        let l = lines.len();
        let mut b = geo::MeshBuilder::new();
        let mut ids = Vec::with_capacity(l * l);
        for z in &lines {
            for x in &lines {
                let y = self.far_height(*x, *z);
                ids.push(b.vertex([*x, y, *z], [0.0, 1.0, 0.0]));
            }
        }
        for j in 0..l - 1 {
            for i in 0..l - 1 {
                let inside = lines[i] >= -half - 0.5 && lines[i + 1] <= half + 0.5 && lines[j] >= -half - 0.5 && lines[j + 1] <= half + 0.5;
                if inside {
                    continue;
                }
                let (a, c) = (ids[j * l + i], ids[j * l + i + 1]);
                let (d, e) = (ids[(j + 1) * l + i], ids[(j + 1) * l + i + 1]);
                b.tri(a, d, e);
                b.tri(a, e, c);
            }
        }
        let mut m = b.finish();
        geo::smooth_normals(&mut m);
        // The curtain down the square's border, at the land grid's points.
        let n = self.n;
        let cs = self.size / (n - 1) as f32;
        let mut cb = geo::MeshBuilder::new();
        for side in 0..4 {
            for i in 0..n - 1 {
                let at = |k: usize| -> (usize, usize) {
                    match side {
                        0 => (k, 0),
                        1 => (n - 1, k),
                        2 => (n - 1 - k, n - 1),
                        _ => (0, n - 1 - k),
                    }
                };
                let ((i0, j0), (i1, j1)) = (at(i), at(i + 1));
                let p = |i: usize, j: usize| [i as f32 * cs - half, self.h[j * n + i], j as f32 * cs - half];
                let (p0, p1) = (p(i0, j0), p(i1, j1));
                let dir = match side {
                    0 => [0.0, 0.0, -1.0],
                    1 => [1.0, 0.0, 0.0],
                    2 => [0.0, 0.0, 1.0],
                    _ => [-1.0, 0.0, 0.0],
                };
                let (a, c) = (cb.vertex(p0, dir), cb.vertex(p1, dir));
                let (d, e) = (cb.vertex([p0[0], p0[1] - 40.0, p0[2]], dir), cb.vertex([p1[0], p1[1] - 40.0, p1[2]], dir));
                // Facing out of the square (the border walked round from
                // -x, -z through +x and +z).
                cb.tri(a, e, d);
                cb.tri(a, c, e);
            }
        }
        geo::merge(&mut m, &cb.finish());
        m
    }

    /// Round 3: the cover of the land past the square at point `c` with its
    /// face normal: forest canopy low down, alpine grass and scree higher,
    /// rock on the steep ground and the high crests, snow on the highest
    /// gentle ground.
    fn far_class(&self, c: [f32; 3], nrm: [f32; 3]) -> usize {
        let hf = c[1] / self.top;
        // The curtain down the square's border: the foothills' own cover.
        if nrm[1].abs() < 0.01 {
            return if hf > 0.45 { ALPINE_C } else { FOREST };
        }
        let steep = 1.0 - nrm[1].abs();
        let f = self.field(c[0], c[2], 900.0, 0x9b);
        // Round 3: calm covers (its cells are up to a kilometre or two
        // across, so a cover change is a big triangle's edge): forest, the
        // alpine grass high up, bare rock only on the high, steep ground
        // near the square, no snow.
        let out = (c[0].abs() - self.size * 0.5).max(c[2].abs() - self.size * 0.5);
        if out < 4000.0 && steep > 0.35 + 0.05 * f && hf > 0.35 {
            ROCKS
        } else if hf > 0.55 + 0.08 * f {
            ALPINE_C
        } else {
            FOREST
        }
    }

    /// Grid coordinates of plan point (x, z).
    fn cell(&self, x: f32, z: f32) -> (usize, usize, f32, f32) {
        let c = (self.n - 1) as f32;
        let gx = ((x / self.size + 0.5) * c).clamp(0.0, c - 1e-3);
        let gz = ((z / self.size + 0.5) * c).clamp(0.0, c - 1e-3);
        (gx as usize, gz as usize, gx.fract(), gz.fract())
    }

    fn lerp(&self, a: &[f32], x: f32, z: f32) -> f32 {
        let (i, j, fx, fz) = self.cell(x, z);
        let n = self.n;
        let k = j * n + i;
        let (p, q) = ((i + 1).min(n - 1), (j + 1).min(n - 1));
        let (b, c, d) = (j * n + p, q * n + i, q * n + p);
        a[k] * (1.0 - fx) * (1.0 - fz) + a[b] * fx * (1.0 - fz) + a[c] * (1.0 - fx) * fz + a[d] * fx * fz
    }

    /// The ground's height (m) at plan point (x, z).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        self.lerp(&self.h, x, z)
    }

    /// What erosion did at (x, z) (m; + debris, - carved).
    fn deposit(&self, x: f32, z: f32) -> f32 {
        self.lerp(&self.dep, x, z)
    }

    /// How hollow the ground is at (x, z) (+ hollows, - ribs).
    fn hollow(&self, x: f32, z: f32) -> f32 {
        self.lerp(&self.curv, x, z)
    }

    /// Distance (m) from (x, z) to the drainage valley's line.
    pub fn river(&self, x: f32, z: f32) -> f32 {
        let k = 2.0 / self.size;
        let (d, along) = self.net.valley(x * k, z * k);
        if along < 0.36 { f32::MAX } else { d / k }
    }

    /// The player's path round a focus (m from its middle line), winding
    /// from behind the player towards the summit; `None` without a focus or
    /// far from it.
    fn path(&self, x: f32, z: f32, m: &Mix) -> Option<f32> {
        let (fx, fz, fr) = m.focus?;
        let (dx, dz) = (x - fx, z - fz);
        if dx * dx + dz * dz > fr * fr {
            return None;
        }
        let s = self.summit();
        let (ax, az) = (s[0] - fx, s[2] - fz);
        let l = (ax * ax + az * az).sqrt().max(1.0);
        let (ux, uz) = (ax / l, az / l);
        // Along and across the line to the summit, turned 12 degrees right
        // (round 3: 8, winding wider, so it leads off ahead of the player).
        let (c, sn) = (0.990f32, 0.139f32);
        let (rx, rz) = (ux * c - uz * sn, ux * sn + uz * c);
        let t = dx * rx + dz * rz;
        if !(-25.0..160.0).contains(&t) {
            return None;
        }
        let across = -dx * rz + dz * rx;
        let wind = 4.5 * (t / 13.0).sin() + 1.6 * (t / 4.7 + 1.3).sin();
        Some((across - wind - 1.0).abs())
    }

    /// The ground's height (m) at (x, z) as the asset draws it: round a
    /// player's point (round 2) with its hummocks and the worn path.
    fn surface(&self, x: f32, z: f32, m: &Mix) -> f32 {
        let y = self.height(x, z);
        let Some((fx, fz, fr)) = m.focus else { return y };
        let r = (fr * 0.45).clamp(40.0, 140.0);
        let d = ((x - fx).powi(2) + (z - fz).powi(2)).sqrt();
        if d >= r + 30.0 {
            return y;
        }
        let fade = noise::smoothstep(r + 25.0, r, d);
        let bump = 0.22 * noise::fbm(x / 3.5, z / 3.5, self.seed ^ 0x77a, 3, 2.0, 0.5) + 0.07 * noise::perlin2(x / 1.2, z / 1.2, self.seed ^ 0x77b);
        let worn = self.path(x, z, m).map_or(0.0, |p| 0.14 * noise::smoothstep(1.3, 0.4, p));
        y + fade * (bump - worn)
    }

    /// Whether (x, z) is open meadow (no forest, rock, gravel or path): where
    /// a scene or a game can stand a player.
    pub fn meadow_at(&self, v: &Values, x: f32, z: f32) -> bool {
        let y = self.height(x, z);
        self.class([x, y, z], [0.0, 1.0, 0.0], &Mix::of(v)) == MEADOW_C
    }

    /// The ground's normal at (x, z).
    pub fn normal(&self, x: f32, z: f32) -> [f32; 3] {
        let e = self.size / (self.n - 1) as f32;
        let dx = self.height(x + e, z) - self.height(x - e, z);
        let dz = self.height(x, z + e) - self.height(x, z - e);
        geo::normalize([-dx, 2.0 * e, -dz])
    }

    /// The ground's normal at (x, z) over about three grid cells (round 3:
    /// the slope the snow reads).
    fn normal_wide(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 1.5 * self.size / (self.n - 1) as f32;
        let dx = self.height(x + e, z) - self.height(x - e, z);
        let dz = self.height(x, z + e) - self.height(x, z - e);
        geo::normalize([-dx, 2.0 * e, -dz])
    }

    /// The summit: its plan point and height (m).
    pub fn summit(&self) -> [f32; 3] {
        self.peak
    }

    /// How thick the forest is at (x, z) for these values (0 open .. 1
    /// dense): where a scene or a game can stand a player in the open.
    pub fn forest_at(&self, v: &Values, x: f32, z: f32) -> f32 {
        let m = Mix::of(v);
        let steep = 1.0 - self.normal(x, z)[1];
        self.forest(x, z, self.height(x, z) / self.top, steep, &m)
    }

    /// Low-frequency noise at (x, z), features about `s` m (about [-1, 1]).
    fn field(&self, x: f32, z: f32, s: f32, k: u32) -> f32 {
        noise::fbm(x / s, z / s, self.seed ^ k, 3, 2.0, 0.5)
    }

    /// The tree line's height share at (x, z), meandering. Round 3: lower
    /// on the ridges' crests and higher in the hollows between them, so the
    /// bare rock and scree run down the arms into the forest.
    fn tree_line(&self, x: f32, z: f32, m: &Mix) -> f32 {
        m.line + 0.05 * self.field(x, z, 260.0, 0x14) + 0.04 - 0.17 * self.lerp(&self.spine, x, z)
    }

    /// The forest's thickness at (x, z) (0 open .. 1 dense): clearings, the
    /// thinning towards the tree line, none on cliffs or in the gravel beds.
    fn forest(&self, x: f32, z: f32, hf: f32, steep: f32, m: &Mix) -> f32 {
        if m.forest <= 0.0 {
            return 0.0;
        }
        let line = self.tree_line(x, z, m) + 0.025 * self.field(x, z, 40.0, 0x6c);
        let clear = self.field(x, z, 300.0, 0xf0) + 0.35 * self.field(x, z, 90.0, 0xf1);
        let thr = 0.1 - 0.9 * m.forest;
        let open = noise::smoothstep(thr, thr + 0.12, clear);
        let below = noise::smoothstep(line, line - 0.05, hf);
        // The forest climbs steeper ground low down (round 1: the massif's
        // lower flanks are wooded, as in the reference, not bare rock).
        let low = lowness(hf, m);
        let flat = noise::smoothstep(0.42 + 0.16 * low, 0.3 + 0.16 * low, steep);
        // No trees in the gravel beds where the rain laid deep debris, nor
        // on the drainage valley's gravel (round 2).
        let bed = noise::smoothstep(0.03, 0.012, self.deposit(x, z) / self.top) * noise::smoothstep(24.0, 40.0, self.river(x, z));
        open * below * flat * bed
    }

    /// The cover class at point `c` (m). The slope is the land's own smooth
    /// normal there (not the face's), so the covers lie in whole patches.
    fn class(&self, c: [f32; 3], _face: [f32; 3], m: &Mix) -> usize {
        let nrm = self.normal(c[0], c[2]);
        let hf = c[1] / self.top;
        let steep = 1.0 - nrm[1].clamp(-1.0, 1.0).abs();
        let dep = self.deposit(c[0], c[2]) / self.top;
        let gully = noise::smoothstep(0.002, 0.012, -dep);
        let fan = noise::smoothstep(0.006, 0.02, dep);
        let line = self.tree_line(c[0], c[2], m);
        // Round 2: hollows (gullies, bowls) and ribs from the land's
        // curvature: snow drifts into the hollows and lasts there, the wind
        // scours the ribs and crests bare.
        let hol = self.hollow(c[0], c[2]);
        let hollow = noise::smoothstep(0.0005, 0.004, hol);
        let rib = noise::smoothstep(0.0008, 0.005, -hol);
        // Fine, ragged wobble on every edge (round 1); round 3 a finer one
        // too (the edges are refined to a few metres, so it shows).
        let rag = self.field(c[0], c[2], 45.0, 0x6a) + 0.3 * self.field(c[0], c[2], 13.0, 0x6b);
        if m.snow > 0.0 {
            // Round 3: the snow reads the slope over about 50 m, so it lies
            // in whole fields rather than a speckle over every crag.
            let steep = 1.0 - self.normal_wide(c[0], c[2])[1].clamp(-1.0, 1.0).abs();
            // Snow by height, lower in the hollows (tapering down the
            // gullies), off the steep ribs; the summit's gentle dome white.
            // Round 3: the bowls hold it, the wind scours the ribs, and no
            // snow clings to the steep walls (round 2's long stripes).
            let snow_line = 1.0 - 0.8 * m.snow + 0.05 * self.field(c[0], c[2], 160.0, 0x5a) + 0.035 * rag - 0.14 * hollow - 0.06 * gully + 0.08 * rib;
            let dome = noise::smoothstep(0.84, 0.94, hf) * 0.22;
            let hold = 0.33 + dome + 0.12 * hollow + 0.04 * gully - 0.14 * rib + 0.05 * self.field(c[0], c[2], 120.0, 0x5b) + 0.05 * rag;
            if hf > snow_line && steep < hold.min(0.5) {
                return SNOW;
            }
        }
        // Round 1: the rock gives way to forest and grass on the lower
        // flanks.
        // Round 3: the ribs and spurs stay bare further down (the forest in
        // the hollows between them), as the reference's rock spreads.
        let rock_slope = 0.3 - 0.22 * m.rock + 0.04 * self.field(c[0], c[2], 160.0, 0x3c) + 0.04 * rag + 0.24 * lowness(hf, m) - 0.1 * rib - 0.08 * self.lerp(&self.spine, c[0], c[2]);
        let crest = line + 0.3 * (1.0 - m.rock) + 0.06;
        if steep > rock_slope || (hf > crest && steep > 0.05 && fan < 0.5) {
            // Round 2: the strata's darker beds in bands across the faces.
            let k = 2.0 / self.size;
            let bed = (hf + tilt(c[0] * k, c[2] * k, self.seed)) / STEP;
            let dark = bed.fract() > 0.64 + 0.18 * self.field(c[0], c[2], 70.0, 0x3e);
            return if dark { ROCK_DARK } else { ROCKS };
        }
        // Talus: the scree fans under the cliffs.
        if hf > line - 0.03 && (fan > 0.4 || (hf > line && steep > rock_slope * 0.6) || hf > line + 0.16 + 0.04 * self.field(c[0], c[2], 200.0, 0x3d)) {
            return SCREE_C;
        }
        if hf > line {
            return ALPINE_C;
        }
        // The drainage valley's gravel bed, and the stream beds' on the
        // valley floors. Round 3: a wider bed of varying width, braided:
        // gravel bars with green islands between the channels; the side
        // streams' narrower beds.
        let river = self.river(c[0], c[2]);
        let bed_w = 26.0 + 20.0 * self.field(c[0], c[2], 110.0, 0x3f) + 8.0 * self.field(c[0], c[2], 30.0, 0x44);
        let braid = self.field(c[0], c[2], 40.0, 0x45) + 0.4 * self.field(c[0], c[2], 12.0, 0x46);
        let island = river > bed_w * 0.2 && braid < 0.0;
        if (river < bed_w && !island) || (dep > 0.05 && hf < line - 0.1) {
            return SCREE_C;
        }
        // Round a player: the worn path and patches of gravel and stones in
        // the meadow (round 3: a wider path, gravel along its edges).
        let open = self.forest(c[0], c[2], hf, steep, m) <= 0.5;
        if open && let Some(d) = self.path(c[0], c[2], m) {
            let edge = 1.05 + 0.3 * self.field(c[0], c[2], 6.0, 0x40);
            if d < edge {
                return PATH;
            }

            let near = m.focus.is_some_and(|(fx, fz, _)| (c[0] - fx).powi(2) + (c[2] - fz).powi(2) < 90.0 * 90.0);
            if near && self.field(c[0], c[2], 7.0, 0x41) > 0.55 {
                return SCREE_C;
            }
        }
        if !open {
            return FOREST;
        }
        MEADOW_C
    }
}

/// The bough tiers of a far spruce (3) and a near one (5): (bottom, top,
/// spread) as shares of the tree's height and its widest bough.
/// Round 3: a far spruce two tiers of five (it is a few pixels; the
/// triangles go to more trees).
const FAR_TIERS: [(f32, f32, f32); 2] = [(0.12, 0.68, 1.0), (0.42, 1.0, 0.62)];
const NEAR_TIERS: [(f32, f32, f32); 5] = [(0.1, 0.42, 1.0), (0.26, 0.58, 0.84), (0.42, 0.73, 0.66), (0.57, 0.87, 0.47), (0.71, 1.0, 0.27)];
/// Triangles in one near spruce (five tiers of eight boughs).
const NEAR_TREE_TRIS: usize = 108;
/// About the triangles in one walker's whorled spruce (round 2; round 3 its
/// sprays saw-toothed, one double-sided sheet each).
const WHORL_TRIS: usize = 1150;

/// How far down the mountain height share `hf` is: 1 on the foothills and
/// the lower flanks, easing to 0 at the tree line.
fn lowness(hf: f32, m: &Mix) -> f32 {
    1.0 - noise::smoothstep(m.line * 0.4, m.line, hf)
}

/// One spruce `h` m tall standing at `at`: a thin trunk and drooping tiers
/// of boughs (`tiers`, `sides` boughs each), each a cone with a hollow
/// underside ([`TREE_TRIS`] triangles far, [`NEAR_TREE_TRIS`] near). Wood
/// into `wood`, needles into `crown`.
#[allow(clippy::too_many_arguments)]
fn spruce(wood: &mut geo::MeshBuilder, crown: &mut geo::MeshBuilder, at: [f32; 3], h: f32, seed: u32, id: u32, tiers: &[(f32, f32, f32)], sides: usize) {
    let yaw = sh::rnd(seed, id, 11) * sh::TAU;
    let tr = h * 0.022;
    let (bt, tt) = (at[1] - 0.6, at[1] + h * 0.3);
    let mut ring = Vec::with_capacity(8);
    let ts = if tiers.len() > 2 { 4 } else { 3 };
    for i in 0..ts {
        let a = yaw + i as f32 * sh::TAU / ts as f32;
        let (s, c) = a.sin_cos();
        let nrm = [c, 0.0, s];
        ring.push((wood.vertex([at[0] + c * tr, bt, at[2] + s * tr], nrm), wood.vertex([at[0] + c * tr, tt, at[2] + s * tr], nrm)));
    }
    for i in 0..ts {
        let (b0, t0) = ring[i];
        let (b1, t1) = ring[(i + 1) % ts];
        wood.quad(b0, t0, t1, b1);
    }
    let rb = h * (0.17 + 0.05 * sh::rnd(seed, id, 12));
    let sides = sides.clamp(3, 12);
    for (k, (yb, yt, rs)) in tiers.iter().enumerate() {
        let r = rb * rs;
        let (y0, y1) = (at[1] + h * yb, at[1] + h * yt);
        let apex = crown.vertex([at[0], y1, at[2]], [0.0, 1.0, 0.0]);
        let inner = crown.vertex([at[0], y0 + (y1 - y0) * 0.22, at[2]], [0.0, -1.0, 0.0]);
        let mut pts = [0u32; 12];
        for (i, p) in pts.iter_mut().take(sides).enumerate() {
            let q = id.wrapping_mul(31).wrapping_add(k as u32 * 13 + i as u32);
            let a = yaw + (i as f32 + 0.4 * sh::rnd(seed, q, 13)) * sh::TAU / sides as f32;
            // Round 3: every other point pulled in, so a tier's outline is
            // ragged boughs, not a cone's rim.
            let notch = if i % 2 == 1 { 0.55 } else { 1.0 };
            let rr = r * notch * (0.7 + 0.6 * sh::rnd(seed, q, 14));
            let droop = h * (0.03 + 0.07 * sh::rnd(seed, q, 15)) * notch;
            let (s, c) = a.sin_cos();
            *p = crown.vertex([at[0] + c * rr, y0 - droop, at[2] + s * rr], geo::normalize([c, 0.6, s]));
        }
        for i in 0..sides {
            let (a, b) = (pts[i], pts[(i + 1) % sides]);
            crown.tri(apex, b, a);
            crown.tri(inner, a, b);
        }
    }
}

/// Round 2: a near spruce as a walker sees one, `h` m tall at `at`: a
/// slender, tapering trunk, bare at the foot, and ten to fourteen irregular
/// whorls of drooping branch sprays with gaps between them (each spray a
/// long, narrow, bent blade, lit on top and darker under), so the silhouette
/// is ragged rather than stacked cones. Returns its triangles.
fn spruce_whorled(wood: &mut geo::MeshBuilder, crown: &mut geo::MeshBuilder, at: [f32; 3], h: f32, seed: u32, id: u32) -> usize {
    let yaw0 = sh::rnd(seed, id, 11) * sh::TAU;
    let mut tris = 0;
    let (r0, r1) = (0.1 + h * 0.01, 0.025);
    let (bt, tt) = (at[1] - 0.5, at[1] + h * 0.95);
    let mut ring = Vec::with_capacity(6);
    for i in 0..6 {
        let a = yaw0 + i as f32 * sh::TAU / 6.0;
        let (s, c) = a.sin_cos();
        let nrm = [c, 0.0, s];
        ring.push((wood.vertex([at[0] + c * r0, bt, at[2] + s * r0], nrm), wood.vertex([at[0] + c * r1, tt, at[2] + s * r1], nrm)));
    }
    for i in 0..6 {
        let (b0, t0) = ring[i];
        let (b1, t1) = ring[(i + 1) % 6];
        wood.quad(b0, t0, t1, b1);
        tris += 2;
    }
    let bare = 0.06 + 0.08 * sh::rnd(seed, id, 12);
    // Round 3: more whorls, closer, so the sprays overlap into a full crown.
    let whorls = 18 + (sh::rnd(seed, id, 13) * 6.0) as u32;
    let reach = h * (0.15 + 0.05 * sh::rnd(seed, id, 14));
    for k in 0..whorls {
        let q = id.wrapping_mul(97).wrapping_add(k * 13);
        let t = bare + (0.97 - bare) * (k as f32 + 0.35 * sh::rnd(seed, q, 15)) / whorls as f32;
        let y = at[1] + h * t;
        let taper = ((1.0 - t) / (1.0 - bare)).max(0.0).powf(0.8);
        let span = (reach * taper).max(0.35);
        // Round 3: the top as full as the rest (round 2's bare tops).
        let n = if t > 0.82 { 6 + (sh::rnd(seed, q, 16) * 2.0) as u32 } else { 8 + (sh::rnd(seed, q, 16) * 4.0) as u32 };
        let turn = yaw0 + sh::rnd(seed, q, 17) * sh::TAU;
        for j in 0..n {
            let qq = q.wrapping_mul(31).wrapping_add(j);
            // Now and then a branch is missing: a gap in the crown.
            if t < 0.85 && sh::rnd(seed, qq, 18) < 0.1 {
                continue;
            }
            let a = turn + (j as f32 + 0.5 * (sh::rnd(seed, qq, 19) - 0.5)) * sh::TAU / n as f32;
            let len = span * (0.65 + 0.55 * sh::rnd(seed, qq, 20));
            let droop = len * (0.15 + 0.35 * sh::rnd(seed, qq, 21) + 0.25 * (1.0 - t));
            tris += spray(crown, [at[0], y, at[2]], a, len, droop, r0 * 0.5, seed, qq);
        }
    }
    // The leader: a thin spike over the top whorl.
    let apex = crown.vertex([at[0], at[1] + h, at[2]], [0.0, 1.0, 0.0]);
    let mut pts = [0u32; 3];
    for (i, p) in pts.iter_mut().enumerate() {
        let a = yaw0 + i as f32 * sh::TAU / 3.0;
        let (s, c) = a.sin_cos();
        *p = crown.vertex([at[0] + c * h * 0.02, at[1] + h * 0.9, at[2] + s * h * 0.02], [c, 0.4, s]);
    }
    for i in 0..3 {
        crown.tri(apex, pts[(i + 1) % 3], pts[i]);
        tris += 1;
    }
    tris
}

/// Round 3: a boulder broken along joints: a lumpy rock `dims` across, cut
/// flat by five to seven seeded fracture planes (sides more than tops), so
/// it shows flat broken faces and sharp-ish edges rather than a smooth lump.
fn fractured(dims: [f32; 3], seed: u32) -> Mesh {
    // The small ones a coarser ball (80 triangles, not 320).
    let mut r = sh::rock(dims, seed, 0.75, dims[0].max(dims[2]) < 1.3);
    let cuts = 5 + (sh::rnd(seed, 0, 81) * 3.0) as u32;
    for k in 0..cuts {
        let a = sh::rnd(seed, k, 82) * sh::TAU;
        let tilt = if k == 0 { 1.2 } else { 0.25 + 0.6 * sh::rnd(seed, k, 83) };
        let n = geo::normalize([a.cos(), tilt, a.sin()]);
        // How far out along it the plane cuts: a share of the rock's extent
        // that way.
        let ext = (n[0] * dims[0] * 0.5).abs() + (n[1] * dims[1] * 0.5).abs() + (n[2] * dims[2] * 0.5).abs();
        let d = ext * (0.48 + 0.25 * sh::rnd(seed, k, 84));
        for v in &mut r.vertices {
            let over = v[0] * n[0] + v[1] * n[1] + v[2] * n[2] - d;
            if over > 0.0 {
                v[0] -= n[0] * over;
                v[1] -= n[1] * over;
                v[2] -= n[2] * over;
            }
        }
    }
    geo::smooth_normals(&mut r);
    r
}

/// Round 3: a low leafy shrub `s` m across at `at`: a few twigs from its
/// foot and leaf sprays (cards, each its own cell of the cut leaf pattern)
/// over a low mound, facing out of it, the inner ones shaded.
#[allow(clippy::too_many_arguments)]
fn bush(leaves: &mut Mesh, uv: &mut Vec<[f32; 2]>, wood: &mut Mesh, at: [f32; 3], s: f32, seed: u32, id: u32, cards: &mut u32) {
    let h = s * (0.4 + 0.25 * sh::rnd(seed, id, 91));
    let count = (8.0 + 16.0 * s) as u32;
    let size = 0.26 + 0.14 * s;
    for k in 0..count {
        let q = id.wrapping_mul(53).wrapping_add(k);
        let a = sh::rnd(seed, q, 92) * sh::TAU;
        let rr = sh::rnd(seed, q, 93).sqrt();
        let up = 0.15 + 0.85 * sh::rnd(seed, q, 94);
        // On a low dome: out from the middle and up its side.
        let dir = geo::normalize([a.cos() * rr, up * (1.0 - 0.5 * rr), a.sin() * rr]);
        let c = [at[0] + a.cos() * rr * s * 0.5, at[1] + h * up * (1.0 - 0.6 * rr * rr) + 0.05, at[2] + a.sin() * rr * s * 0.5];
        // Facing out of the shrub, every spray its own way.
        let jit = [sh::rnd(seed, q, 95) - 0.5, sh::rnd(seed, q, 96) - 0.5, sh::rnd(seed, q, 97) - 0.5];
        let nf = geo::normalize([dir[0] + jit[0], dir[1] + 0.3 + jit[1], dir[2] + jit[2]]);
        let gd = nf[1];
        let mut vup = [-nf[0] * gd, 1.0 - nf[1] * gd, -nf[2] * gd];
        if vup[0].abs() + vup[1].abs() + vup[2].abs() < 1e-3 {
            vup = [1.0, 0.0, 0.0];
        }
        let vup = geo::normalize(vup);
        let right = [vup[1] * nf[2] - vup[2] * nf[1], vup[2] * nf[0] - vup[0] * nf[2], vup[0] * nf[1] - vup[1] * nf[0]];
        let sz = size * (0.8 + 0.4 * sh::rnd(seed, q, 98));
        let base = [c[0] - vup[0] * sz * 0.5, c[1] - vup[1] * sz * 0.5, c[2] - vup[2] * sz * 0.5];
        let corner = |x: f32, y: f32| [base[0] + right[0] * sz * x + vup[0] * sz * y, base[1] + right[1] * sz * x + vup[1] * sz * y, base[2] + right[2] * sz * x + vup[2] * sz * y];
        let corners = [corner(-0.5, 0.0), corner(0.5, 0.0), corner(0.5, 1.0), corner(-0.5, 1.0)];
        // Inner sprays turn their light normal down: the shrub lit outside,
        // dark within.
        let deep = (1.0 - rr) * 0.6;
        let ln = geo::normalize([dir[0] * (1.0 - deep), dir[1] * (1.0 - deep) + 0.35 - deep, dir[2] * (1.0 - deep)]);
        let cell = (*cards % 211) as f32;
        let uvs = [[cell, 1.0], [cell + 1.0, 1.0], [cell + 1.0, 0.0], [cell, 0.0]];
        let b0 = leaves.vertices.len() as u32;
        for (p, t) in corners.iter().zip(uvs) {
            leaves.vertices.push([p[0], p[1], p[2], ln[0], ln[1], ln[2]]);
            uv.push(t);
        }
        leaves.indices.extend_from_slice(&[b0, b0 + 1, b0 + 2, b0, b0 + 2, b0 + 3]);
        *cards += 1;
    }
    // A few twigs from the foot: thin wedges, both faces.
    for k in 0..3u32 {
        let q = id.wrapping_mul(59).wrapping_add(k);
        let a = sh::rnd(seed, q, 99) * sh::TAU;
        let tip = [at[0] + a.cos() * s * 0.35, at[1] + h * 0.8, at[2] + a.sin() * s * 0.35];
        let w = 0.012 + 0.01 * s;
        let (sx, sz) = (-a.sin() * w, a.cos() * w);
        let n = geo::normalize([a.cos(), 0.3, a.sin()]);
        let b0 = wood.vertices.len() as u32;
        for p in [[at[0] - sx, at[1] - 0.05, at[2] - sz], [at[0] + sx, at[1] - 0.05, at[2] + sz], tip] {
            wood.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
        }
        wood.indices.extend_from_slice(&[b0, b0 + 1, b0 + 2, b0, b0 + 2, b0 + 1]);
    }
}

/// Round 3: a flower head `r` m across at `at`: a five-pointed head, a
/// little tilted, facing up (five triangles).
fn flower(m: &mut Mesh, at: [f32; 3], r: f32, seed: u32, id: u32) {
    let turn = sh::rnd(seed, id, 78) * sh::TAU;
    let n = geo::normalize([0.3 * (sh::rnd(seed, id, 79) - 0.5), 1.0, 0.3 * (sh::rnd(seed, id, 80) - 0.5)]);
    let b0 = m.vertices.len() as u32;
    m.vertices.push([at[0], at[1], at[2], n[0], n[1], n[2]]);
    for k in 0..5u32 {
        let a = turn + k as f32 * sh::TAU / 5.0;
        let (x, z) = (a.cos() * r, a.sin() * r);
        let y = -(x * n[0] + z * n[2]) / n[1];
        m.vertices.push([at[0] + x, at[1] + y, at[2] + z, n[0], n[1], n[2]]);
    }
    for k in 0..5u32 {
        // Counter-clockwise seen from above.
        m.indices.extend_from_slice(&[b0, b0 + 1 + (k + 1) % 5, b0 + 1 + k]);
    }
}

/// Round 3: one branch spray of a walker's spruce, from `r` m out of the
/// trunk's axis at `at`, heading `a` (radians) for `len` m and drooping
/// `droop` m by its tip: a spine in three runs, each with a twig to either
/// side whose tip reaches forward and a little down, so the spray's outline
/// is saw-toothed and fine rather than one flat shelf. One sheet, drawn from
/// both sides (the crown is a card surface). Returns its triangles.
#[allow(clippy::too_many_arguments)]
fn spray(crown: &mut geo::MeshBuilder, at: [f32; 3], a: f32, len: f32, droop: f32, r: f32, seed: u32, id: u32) -> usize {
    let (s, c) = a.sin_cos();
    let side = [-s, 0.0, c];
    let p = |t: f32| [at[0] + c * (r + len * t), at[1] + len * 0.06 * t - droop * t * t, at[2] + s * (r + len * t)];
    let spine = [p(0.0), p(0.34), p(0.68), p(1.0)];
    let up = geo::normalize([c * 0.35, 1.0, s * 0.35]);
    let vs = spine.map(|q| crown.vertex(q, up));
    let mut tris = 0;
    for k in 0..3 {
        let (a0, a1) = (spine[k], spine[k + 1]);
        let q = id.wrapping_mul(7).wrapping_add(k as u32);
        let wd = len * (0.3 - 0.07 * k as f32) * (0.75 + 0.5 * sh::rnd(seed, q, 26));
        for (sgn, salt) in [(1.0f32, 27u32), (-1.0, 28)] {
            let f = 0.8 + 0.45 * sh::rnd(seed, q, salt);
            let tip = [a0[0] + (a1[0] - a0[0]) * f + side[0] * wd * sgn, a0[1] + (a1[1] - a0[1]) * f - wd * 0.3, a0[2] + (a1[2] - a0[2]) * f + side[2] * wd * sgn];
            let n = geo::normalize([up[0] + side[0] * sgn * 0.35, up[1], up[2] + side[2] * sgn * 0.35]);
            let vt = crown.vertex(tip, n);
            // Facing up either side of the spine.
            if sgn > 0.0 {
                crown.tri(vs[k], vt, vs[k + 1]);
            } else {
                crown.tri(vs[k], vs[k + 1], vt);
            }
            tris += 1;
        }
    }
    tris
}

/// The asset.
pub fn generate(v: &Values, seed: u32) -> Output {
    let land = Land::new(v, seed);
    let m = Mix::of(v);
    let (size, n) = (land.size, land.n);
    let cells = (n - 1) as u32;
    // Round 3: the low, gentle foothills (under the forest) at half the
    // land grid's resolution, refined back to the full grid on the massif,
    // round a player, along the valley and wherever the cover changes, so
    // the triangles go to the rock and the trees rather than flat forest
    // floor. The cover at every grid point first.
    let c = (n - 1) as f32;
    let cls: Vec<u8> = (0..n * n)
        .map(|k| {
            let (x, z) = (((k % n) as f32 / c - 0.5) * size, ((k / n) as f32 / c - 0.5) * size);
            land.class([x, land.h[k], z], [0.0, 1.0, 0.0], &m) as u8
        })
        .collect();
    let at = |x: f32| ((x / size + 0.5) * c).clamp(0.0, c);
    let mut ground = geo::height_grid([size, size], cells / 2, |x, z| land.height(x, z));
    {
        let cell = size / c;
        let low = land.top * 0.14;
        let focus_r = m.focus.map(|(fx, fz, fr)| (fx, fz, fr + 80.0));
        let full = |p: [f32; 3], _e: f32| -> bool {
            if p[1] > low || 1.0 - land.normal(p[0], p[2])[1] > 0.1 || land.river(p[0], p[2]) < 140.0 {
                return true;
            }
            if focus_r.is_some_and(|(fx, fz, r)| (p[0] - fx).powi(2) + (p[2] - fz).powi(2) < r * r) {
                return true;
            }
            // A cover edge in the 2x2 cells round it.
            let (i0, j0) = ((at(p[0]) - 1.0).max(0.0) as usize, (at(p[2]) - 1.0).max(0.0) as usize);
            let (i1, j1) = ((i0 + 2).min(n - 1), (j0 + 2).min(n - 1));
            let first = cls[j0 * n + i0];
            (j0..=j1).any(|j| (i0..=i1).any(|i| cls[j * n + i] != first))
        };
        evani_procedural::detail::refine_near(&mut ground, cell * 1.45, 2, full);
    }
    // Exactly the grid's own heights (every vertex is one of its points).
    for vx in &mut ground.vertices {
        let i = ((vx[0] / size + 0.5) * c).round().clamp(0.0, c) as usize;
        let j = ((vx[2] / size + 0.5) * c).round().clamp(0.0, c) as usize;
        vx[1] = land.h[j * n + i];
    }
    // Round 2: round a player the ground is refined to about 2.6 m (and
    // 1 m along the path) and given its own small relief, hummocks and the
    // path worn into it, so the path, the gravel and the grass's bare
    // patches have real edges and the meadow is no flat plane at a walker's
    // feet. It fades to the land's own heights at the refined edge.
    if let Some((fx, fz, fr)) = m.focus {
        let r = (fr * 0.45).clamp(40.0, 140.0);
        let reach2 = (r + 30.0).powi(2);
        let d2 = move |p: [f32; 3]| (p[0] - fx).powi(2) + (p[2] - fz).powi(2);
        // Round 3: 4 m out to the refined edge, 2 m within 45 m of the
        // player (the hummocks a walker sees at their feet).
        evani_procedural::detail::refine_near(&mut ground, 4.0, 12, |p, _| d2(p) < reach2);
        evani_procedural::detail::refine_near(&mut ground, 2.0, 4, |p, _| d2(p) < 45.0 * 45.0);
        evani_procedural::detail::refine_near(&mut ground, 1.0, 4, |p, _| d2(p) < r * r && land.path(p[0], p[2], &m).is_some_and(|d| d < 3.0));
        for vx in &mut ground.vertices {
            if d2([vx[0], 0.0, vx[2]]) < reach2 {
                vx[1] = land.surface(vx[0], vx[2], &m);
            }
        }
    }
    // Round 3: the snow's edge refined to a few metres, so it follows the
    // land's own ragged fields instead of the grid's 17 m stair steps
    // (round 2's "angular teeth"). Snow or not at each grid point first,
    // then every triangle whose neighbourhood holds both.
    {
        let snow: Vec<bool> = cls.iter().map(|k| *k as usize == SNOW).collect();
        let low = land.top * 0.2;
        // The grid cell under a triangle's middle: its four corners.
        let edge = |p: [f32; 3], _e: f32| -> bool {
            if p[1] < low {
                return false;
            }
            let (i0, j0) = ((at(p[0]).floor() as usize).min(n - 2), (at(p[2]).floor() as usize).min(n - 2));
            let first = snow[j0 * n + i0];
            [(1, 0), (0, 1), (1, 1)].iter().any(|(di, dj)| snow[(j0 + dj) * n + i0 + di] != first)
        };
        evani_procedural::detail::refine_near(&mut ground, EDGE_REFINE, 6, edge);
    }
    geo::smooth_normals(&mut ground);
    let mut split = evani_procedural::detail::split_by(&ground, CLASSES, |cc, nn| land.class(cc, nn, &m));
    let mut ground_tris = ground.indices.len() / 3;
    // Round 3: the land round the square (`horizon`), in the same covers.
    if land.horizon > 0.0 {
        let far = land.far_land();
        ground_tris += far.indices.len() / 3;
        let fs = evani_procedural::detail::split_by(&far, CLASSES, |cc, nn| land.far_class(cc, nn));
        for (a, b) in split.iter_mut().zip(&fs) {
            geo::merge(a, b);
        }
    }

    // Round 1 (takeover): the rock's grain coarser (at 28 m it read as salt
    // and pepper from the air), the snow a shade under pure white so its
    // sunlit and shaded sides read, the grass without the turf's dust (it
    // turned the dark forest floor khaki), and the forest floor a dark
    // needle canopy (the moss recipe), so the wooded foothills read as
    // forest from the air between the trees.
    let rock_s = sp::ROCK_LIMESTONE.tint(v.color("color")).seed(seed).scale(40.0);
    let surfaces: [Surface; CLASSES] = [
        sp::GROUND_SNOW_PACKED.tint(SNOW_WHITE).seed(seed ^ 0x51).scale(12.0),
        rock_s,
        sp::GROUND_GRAVEL_SCREE.tint(SCREE).seed(seed ^ 0x52).scale(6.0),
        // Round 1: no turf: its blades smeared into long streaks at a
        // walker's eye and its dry patches turned the clearings khaki from
        // the air. The alpine grass (seen from afar) plain.
        sp::GROUND_GRASS_ALPINE.tint(ALPINE).seed(seed ^ 0x53).scale(3.0),
        sp::GROUND_MOSS_CARPET.tint(FOREST_FLOOR).seed(seed ^ 0x54).scale(5.0),
        // The meadow's ground a fine green moss and herb carpet: no blade
        // direction to streak at a walker's eye (real grass blades stand on
        // it round a player with `mountain-grass`).
        sp::GROUND_MOSS_CARPET.tint(MEADOW).seed(seed ^ 0x55).scale(0.6),
        // Round 2: the strata's darker, cooler beds, and the worn path's
        // packed soil.
        sp::ROCK_LIMESTONE.tint(ROCK_BED).seed(seed ^ 0x56).scale(40.0),
        sp::GROUND_SOIL_PACKED.tint(PATH_SOIL).seed(seed ^ 0x57).scale(0.6),
    ];
    let mut parts: Vec<Part> = split.into_iter().zip(surfaces).filter(|(p, _)| !p.indices.is_empty()).map(|(p, s)| Part::surfaced(p, s)).collect();

    // Boulders: clusters on the meadows, alpine grass and scree.
    let mut stones = Mesh::default();
    let clusters = ((size * size / 160_000.0) as u32).clamp(4, 60);
    let half = size * 0.5;
    let mut placed = 0u32;
    for ci in 0..clusters * 6 {
        if placed >= clusters {
            break;
        }
        let (x, z) = ((sh::rnd(seed, ci, 41) - 0.5) * size * 0.9, (sh::rnd(seed, ci, 42) - 0.5) * size * 0.9);
        let (y, nrm) = (land.height(x, z), land.normal(x, z));
        let k = land.class([x, y, z], nrm, &m);
        if !matches!(k, MEADOW_C | ALPINE_C | SCREE_C) {
            continue;
        }
        placed += 1;
        let count = 3 + (sh::rnd(seed, ci, 43) * 5.0) as u32;
        for b in 0..count {
            let id = ci * 16 + b;
            let s = (0.8 + 3.2 * sh::rnd(seed, id, 44).powf(2.0)) * if b == 0 { 1.3 } else { 1.0 };
            let (bx, bz) = (x + (sh::rnd(seed, id, 45) - 0.5) * 24.0, z + (sh::rnd(seed, id, 46) - 0.5) * 24.0);
            if bx.abs() > half * 0.95 || bz.abs() > half * 0.95 {
                continue;
            }
            let dims = [s, s * (0.5 + 0.3 * sh::rnd(seed, id, 47)), s * (0.6 + 0.4 * sh::rnd(seed, id, 48))];
            let mut r = sh::rock(dims, seed ^ id.wrapping_mul(0x9e37_79b9), 0.9, true);
            sh::rotate(&mut r, [0.0, 1.0, 0.0], sh::rnd(seed, id, 49) * sh::TAU);
            geo::transform(&mut r, [1.0; 3], [bx, land.surface(bx, bz, &m) - dims[1] * 0.3, bz]);
            geo::merge(&mut stones, &r);
        }
    }
    // Round 1 (takeover): round a player's point (`focus_*`) the ground has
    // what a walker sees: boulders half sunk in the meadow and alpine grass,
    // low shrubs (juniper, dwarf pine) in clumps, and the forest tree by
    // tree (below).
    let focus = {
        let r = v.f("focus_radius").max(0.0);
        (r > 1.0).then(|| (v.f("focus_x"), v.f("focus_z"), r))
    };
    let mut shrubs = Mesh::default();
    // Round 3: the walker's boulders fractured and lichened, shrubs as leafy
    // sprays (card leaves), flowers in the meadow.
    let mut boulders = Mesh::default();
    let mut leaves = Mesh::default();
    let mut leaf_uv: Vec<[f32; 2]> = Vec::new();
    let mut flowers = [Mesh::default(), Mesh::default(), Mesh::default()];
    if let Some((fx, fz, fr)) = focus {
        // Where an item may stand: in the square, on open ground.
        let open = |x: f32, z: f32| {
            if x.abs() > half * 0.95 || z.abs() > half * 0.95 {
                return None;
            }
            let (y, nrm) = (land.height(x, z), land.normal(x, z));
            matches!(land.class([x, y, z], nrm, &m), MEADOW_C | ALPINE_C | SCREE_C).then(|| land.surface(x, z, &m))
        };
        for b in 0..70u32 {
            let id = 0x4000 + b;
            let a = sh::rnd(seed, id, 51) * sh::TAU;
            let d = 6.0 + (fr * 0.4 - 6.0).max(0.0) * sh::rnd(seed, id, 52);
            let (bx, bz) = (fx + a.cos() * d, fz + a.sin() * d);
            let Some(y) = open(bx, bz) else { continue };
            let s = 0.4 + 2.4 * sh::rnd(seed, id, 53).powf(2.5);
            let dims = [s, s * (0.45 + 0.3 * sh::rnd(seed, id, 54)), s * (0.6 + 0.4 * sh::rnd(seed, id, 55))];
            let mut r = fractured(dims, seed ^ id.wrapping_mul(0x9e37_79b9));
            sh::rotate(&mut r, [0.0, 1.0, 0.0], sh::rnd(seed, id, 56) * sh::TAU);
            geo::transform(&mut r, [1.0; 3], [bx, y - dims[1] * 0.38, bz]);
            geo::merge(&mut boulders, &r);
        }
        // Round 2: a few big lichened boulders close in front of the
        // player, to the sides of the way towards the summit, half sunk.
        let sm = land.summit();
        let face = (sm[2] - fz).atan2(sm[0] - fx);
        for b in 0..7u32 {
            let id = 0x4800 + b;
            let side = if b % 2 == 0 { 1.0 } else { -1.0 };
            let a = face + side * (0.45 + 0.5 * sh::rnd(seed, id, 51));
            let d = 5.0 + 12.0 * sh::rnd(seed, id, 52);
            let (bx, bz) = (fx + a.cos() * d, fz + a.sin() * d);
            let Some(y) = open(bx, bz) else { continue };
            let s = 1.0 + 1.3 * sh::rnd(seed, id, 53);
            let dims = [s * 1.3, s * (0.55 + 0.2 * sh::rnd(seed, id, 54)), s * (0.8 + 0.3 * sh::rnd(seed, id, 55))];
            let mut r = fractured(dims, seed ^ id.wrapping_mul(0x9e37_79b9));
            sh::rotate(&mut r, [0.0, 1.0, 0.0], sh::rnd(seed, id, 56) * sh::TAU);
            geo::transform(&mut r, [1.0; 3], [bx, y - dims[1] * 0.4, bz]);
            geo::merge(&mut boulders, &r);
        }
        // Round 2: stones and cobbles strewn on the gravel, along the path
        // and here and there in the meadow, half sunk.
        for b in 0..160u32 {
            let id = 0x5000 + b;
            let a = sh::rnd(seed, id, 57) * sh::TAU;
            let d = 3.0 + 70.0 * sh::rnd(seed, id, 58).sqrt();
            let (bx, bz) = (fx + a.cos() * d, fz + a.sin() * d);
            let (y0, nrm) = (land.height(bx, bz), land.normal(bx, bz));
            let k = land.class([bx, y0, bz], nrm, &m);
            if !(matches!(k, SCREE_C | PATH) || (k == MEADOW_C && sh::rnd(seed, id, 59) < 0.3)) {
                continue;
            }
            let s = 0.12 + 0.35 * sh::rnd(seed, id, 60).powf(1.5);
            let dims = [s, s * 0.55, s * 0.8];
            let mut r = sh::rock(dims, seed ^ id.wrapping_mul(0x27d4_eb2f), 0.8, true);
            sh::rotate(&mut r, [0.0, 1.0, 0.0], sh::rnd(seed, id, 61) * sh::TAU);
            geo::transform(&mut r, [1.0; 3], [bx, land.surface(bx, bz, &m) - dims[1] * 0.35, bz]);
            geo::merge(&mut stones, &r);
        }
        let mut cards = 0u32;
        for b in 0..340u32 {
            let id = 0x6000 + b;
            // In clumps: every fourth shrub starts a clump, the rest gather.
            let c0 = id - b % 4;
            let a = sh::rnd(seed, c0, 61) * sh::TAU;
            let d = 2.5 + fr * 0.3 * sh::rnd(seed, c0, 62).powf(1.8);
            let (bx, bz) = (fx + a.cos() * d + (sh::rnd(seed, id, 63) - 0.5) * 6.0, fz + a.sin() * d + (sh::rnd(seed, id, 64) - 0.5) * 6.0);
            if land.path(bx, bz, &m).is_some_and(|p| p < 1.8) {
                continue;
            }
            let Some(y) = open(bx, bz) else { continue };
            let s = 0.6 + 1.3 * sh::rnd(seed, id, 65);
            // Round 3: a low leafy shrub (bilberry, dwarf willow): a few
            // twigs and leaf sprays on a low mound.
            bush(&mut leaves, &mut leaf_uv, &mut shrubs, [bx, y, bz], s, seed, id, &mut cards);
        }
        // Round 3: flowers in the meadow round the player, in drifts:
        // yellow (most), white and a few purple, each a five-petalled head
        // over the grass.
        for b in 0..2400u32 {
            let id = 0x7000 + b;
            let c0 = id - b % 12;
            let a = sh::rnd(seed, c0, 71) * sh::TAU;
            let d = 2.0 + 34.0 * sh::rnd(seed, c0, 72);
            let (bx, bz) = (fx + a.cos() * d + (sh::rnd(seed, id, 73) - 0.5) * 4.0, fz + a.sin() * d + (sh::rnd(seed, id, 74) - 0.5) * 4.0);
            let (y0, nrm) = (land.height(bx, bz), land.normal(bx, bz));
            if !matches!(land.class([bx, y0, bz], nrm, &m), MEADOW_C | ALPINE_C) {
                continue;
            }
            let pick = sh::rnd(seed, c0, 75);
            let k = if pick < 0.65 { 0 } else if pick < 0.9 { 1 } else { 2 };
            let r = 0.035 + 0.035 * sh::rnd(seed, id, 76);
            let top = land.surface(bx, bz, &m) + 0.12 + 0.22 * sh::rnd(seed, id, 77);
            flower(&mut flowers[k], [bx, top, bz], r, seed, id);
        }
    }
    let stone_tris = [&stones, &shrubs, &boulders, &leaves, &flowers[0], &flowers[1], &flowers[2]].iter().map(|m| m.indices.len() / 3).sum::<usize>();
    if !stones.indices.is_empty() {
        parts.push(Part::surfaced(stones, sp::ROCK_GRANITE_GREY.tint(v.color("color")).seed(seed ^ 0x61).scale(1.0)));
    }
    // Round 3: the walker's boulders lichened (the catalogue's mossy
    // boulder, its moss a grey-green lichen).
    if !boulders.indices.is_empty() {
        parts.push(Part::surfaced(boulders, sp::ROCK_MOSSY_BOULDER.tint(LICHEN_ROCK).seed(seed ^ 0x63).scale(1.2)));
    }
    // The shrubs' twigs, and their leaves: the catalogue's leaf cut into
    // sprays (the oak trees' leaf cards; a game lists "cutout"), a darker
    // mountain green.
    if !shrubs.indices.is_empty() {
        parts.push(Part::surfaced(shrubs, sp::GROUND_SOIL_FOREST_FLOOR.tint(BARK).seed(seed ^ 0x62).scale(0.3)));
    }
    if !leaves.indices.is_empty() {
        let leaf = sp::LEAF_OAK.tint(SHRUB).variant(0).extras(0b1001).number(0, 0.8).number(1, 0.9).number(2, 0.0).number(3, 0.42).number(4, 0.0).number(7, 0.12).seed(seed ^ 0x64).scale(1.0).cards();
        parts.push(Part::surfaced_uv(leaves, leaf, leaf_uv));
    }
    for (k, f) in flowers.into_iter().enumerate() {
        if !f.indices.is_empty() {
            parts.push(Part::surfaced(f, sp::GROUND_MOSS_CARPET.tint(FLOWERS[k]).seed(seed ^ (0x65 + k as u32)).scale(0.05)));
        }
    }

    // The forest: spruces on a jittered grid, thick in the forest, a few on
    // the meadows and small ones just over the tree line, within the budget.
    // Round a player's point half the budget goes on near spruces (five
    // tiers), tree by tree; the rest spreads over the land.
    let tris_left = BUDGET.saturating_sub(ground_tris + stone_tris);
    if m.forest > 0.0 && tris_left > 0 {
        // The chance of a tree at (x, z), the ground's height there, its
        // height share and the tree line.
        let chance_at = |x: f32, z: f32| {
            let y = land.surface(x, z, &m);
            let nrm = land.normal(x, z);
            let (hf, steep) = (y / land.top, 1.0 - nrm[1]);
            let thick = land.forest(x, z, hf, steep, &m);
            let line = land.tree_line(x, z, &m);
            let chance = if thick > 0.0 {
                thick
            } else if hf < line && steep < 0.3 {
                0.04
            } else if hf < line + 0.04 && steep < 0.35 {
                0.12
            } else {
                0.0
            };
            (chance, y, hf, line)
        };
        let in_focus = |x: f32, z: f32| focus.is_some_and(|(fx, fz, fr)| (x - fx).powi(2) + (z - fz).powi(2) < fr * fr);
        let mut wood = geo::MeshBuilder::new();
        let mut crowns = [geo::MeshBuilder::new(), geo::MeshBuilder::new(), geo::MeshBuilder::new()];
        let mut used = 0usize;
        // Plant a tree at (x, z) if the forest has one there: 0 a far
        // spruce, 1 a near one (five tiers), 2 a walker's near spruce
        // (round 2, whorled); its triangles (0: none).
        let plant = |wood: &mut geo::MeshBuilder, crowns: &mut [geo::MeshBuilder; 3], x: f32, z: f32, id: u32, kind: u32| -> usize {
            let (chance, y, hf, line) = chance_at(x, z);
            if sh::rnd(seed, id, 23) >= chance {
                return 0;
            }
            let near_line = noise::smoothstep(line - 0.08, line + 0.04, hf);
            let th = (13.0 + 15.0 * sh::rnd(seed, id, 24)) * (1.0 - 0.6 * near_line);
            let tint = ((sh::rnd(seed, id, 25) * 3.0) as usize).min(2);
            match kind {
                2 => spruce_whorled(wood, &mut crowns[tint], [x, y, z], th, seed, id),
                1 => {
                    spruce(wood, &mut crowns[tint], [x, y, z], th, seed, id, &NEAR_TIERS, 10);
                    NEAR_TREE_TRIS
                }
                _ => {
                    spruce(wood, &mut crowns[tint], [x, y, z], th, seed, id, &FAR_TIERS, 5);
                    TREE_TRIS
                }
            }
        };
        if let Some((fx, fz, fr)) = focus {
            // Round 3: a little more of it near the player.
            let budget = tris_left * 3 / 5;
            // Round 2: two rings round the player: the walker's whorled
            // spruces within half the radius (60% of the near budget), the
            // five-tier ones beyond.
            let inner = fr * 0.5;
            for (ring, (r0, r1, share, kind, cost)) in [(0.0, inner, 0.6, 2u32, WHORL_TRIS), (inner, fr, 1.0, 1u32, NEAR_TREE_TRIS)].into_iter().enumerate() {
                let cap = (budget as f32 * share) as usize;
                // The wooded share of the ring, from a seeded look at it.
                let mut f = 0.0f32;
                for k in 0..256u32 {
                    let (a, d) = (sh::rnd(seed, k + 512 * ring as u32, 31) * sh::TAU, (r0 * r0 + (r1 * r1 - r0 * r0) * sh::rnd(seed, k + 512 * ring as u32, 32)).sqrt());
                    f += chance_at(fx + a.cos() * d, fz + a.sin() * d).0;
                }
                let area = sh::PI * (r1 * r1 - r0 * r0) * f / 256.0 + 1.0;
                let room = cap.saturating_sub(used) / cost;
                let sp = (area / (room as f32 * 0.85).max(1.0)).sqrt().max(if kind == 2 { 6.5 } else { 4.0 });
                let cols = (2.0 * r1 / sp) as u32 + 1;
                'near: for gj in 0..cols {
                    for gi in 0..cols {
                        if used + cost > cap {
                            break 'near;
                        }
                        let id = 0x10_0000 + 0x4_0000 * ring as u32 + gj * cols + gi;
                        let x = fx - r1 + (gi as f32 + sh::rnd(seed, id, 21)) * sp;
                        let z = fz - r1 + (gj as f32 + sh::rnd(seed, id, 22)) * sp;
                        // In this ring, clear of the player's own spot and
                        // the path, inside the square.
                        let d2 = (x - fx).powi(2) + (z - fz).powi(2);
                        let on_path = land.path(x, z, &m).is_some_and(|d| d < 3.0);
                        if d2 < 144.0 || d2 < r0 * r0 || d2 >= r1 * r1 || on_path || x.abs() > half * 0.97 || z.abs() > half * 0.97 {
                            continue;
                        }
                        used += plant(&mut wood, &mut crowns, x, z, id, kind);
                    }
                }
            }
        }
        let max_trees = tris_left.saturating_sub(used) / TREE_TRIS;
        // The forested share of the land, from a coarse look at the grid.
        let (mut f, mut total) = (0.0f32, 0.0f32);
        let step = 4;
        for j in (0..n).step_by(step) {
            for i in (0..n).step_by(step) {
                let (x, z) = ((i as f32 / c - 0.5) * size, (j as f32 / c - 0.5) * size);
                let y = land.h[j * n + i];
                let steep = 1.0 - land.normal(x, z)[1];
                f += land.forest(x, z, y / land.top, steep, &m);
                total += 1.0;
            }
        }
        let area = (f / total.max(1.0)) * size * size + 1.0;
        let spacing = (area / (max_trees.max(1) as f32 * 0.85)).sqrt().max(6.0);
        let cols = (size / spacing) as u32;
        let mut count = 0usize;
        'grid: for gj in 0..cols {
            for gi in 0..cols {
                if count >= max_trees {
                    break 'grid;
                }
                let id = gj * cols + gi;
                let x = (gi as f32 + sh::rnd(seed, id, 21)) * spacing - half;
                let z = (gj as f32 + sh::rnd(seed, id, 22)) * spacing - half;
                if x.abs() > half * 0.97 || z.abs() > half * 0.97 || in_focus(x, z) {
                    continue;
                }
                if plant(&mut wood, &mut crowns, x, z, id, 0) > 0 {
                    count += 1;
                }
            }
        }
        let wood = wood.finish();
        if !wood.indices.is_empty() {
            parts.push(Part::surfaced(wood, sp::GROUND_SOIL_FOREST_FLOOR.tint(BARK).seed(seed ^ 0x71).scale(0.3)));
        }
        // Round 3: the needles drawn from both sides (cards with no cut: a
        // spray seen from under it is lit from under it).
        for (k, b) in crowns.into_iter().enumerate() {
            let mesh = b.finish();
            if !mesh.indices.is_empty() {
                parts.push(Part::surfaced(mesh, sp::GROUND_MOSS_CARPET.tint(NEEDLES[k]).seed(seed ^ (0x80 + k as u32)).scale(0.25).cards()));
            }
        }
    }
    Output::Parts(parts)
}

/// Real grass on the meadows and alpine grass between `min` and `max` (plan
/// m, the asset's own frame) for evani-grass (feature `mountain-grass`): a
/// long-meadow patch on the land's height grid, the ground under the forest,
/// the rock, scree and snow left bare (sunk out of sight). A game hands it
/// over round its player as the scene does: `Grass::add_patch_on(patch, n,
/// &heights)`, with the placement's offset in `at`.
#[cfg(feature = "mountain-grass")]
pub fn meadow_grass(v: &Values, seed: u32, min: [f32; 2], max: [f32; 2], at: [f32; 3]) -> (evani_grass::Patch, usize, Vec<f32>) {
    let land = Land::new(v, seed);
    let m = Mix::of(v);
    let n = 113usize;
    let mut h = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let x = min[0] + (max[0] - min[0]) * i as f32 / (n - 1) as f32;
            let z = min[1] + (max[1] - min[1]) * j as f32 / (n - 1) as f32;
            let y = land.height(x, z);
            let k = land.class([x, y, z], [0.0, 1.0, 0.0], &m);
            // Round 2: a rocky meadow, not a lawn: the grass thins out in
            // small bare patches between the tufts.
            // Round 3: more bare ground: the grass in patches and tussocks
            // with soil, moss and stones between (round 2 a dense crop).
            let patchy = land.field(x, z, 2.6, 0x42) + 0.45 * land.field(x, z, 0.9, 0x43) > 0.3;
            let grassy = matches!(k, MEADOW_C | ALPINE_C) && !patchy;
            let g = land.surface(x, z, &m);
            h.push(if grassy { g + at[1] + 0.02 } else { g + at[1] - 3.0 });
        }
    }
    let patch = evani_grass::Patch::new(evani_grass::Kind::Meadow, [at[0] + min[0], at[2] + min[1]], [at[0] + max[0], at[2] + max[1]]).feather(0.05).density(0.75);
    (patch, n, h)
}

/// The mountains' ground for a scene or a game (standing the player on
/// it): the land's width, grid points a side and heights row by row from
/// the -x, -z corner.
pub fn ground(v: &Values, seed: u32) -> (f32, usize, Vec<f32>) {
    let l = Land::new(v, seed);
    (l.size, l.n, l.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        Values::defaults(&PARAMS)
    }

    #[test]
    fn the_mountain_stands_on_the_ground_within_its_budget() {
        let o = generate(&values(), 7);
        assert!(o.is_valid());
        let (lo, hi) = o.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo[1] > -1.0, "lowest {}", lo[1]);
        assert!(hi[1] > 1100.0 && hi[1] < 1300.0, "summit {}", hi[1]);
        assert!(o.triangles() < 400_000, "{} triangles", o.triangles());
    }

    #[test]
    fn a_player_focus_fills_the_forest_round_it_within_the_budget() {
        let plain = generate(&values(), 7);
        let mut v = values();
        // An open spot on the foothills south of the summit, as a scene
        // or a game stands its player (in the open, the forest round it).
        let l = Land::new(&v, 7);
        let s = l.summit();
        let (mut best, mut at) = (f32::MAX, (0.0, 1700.0));
        for k in 0..96 {
            let (x, z) = (s[0] - 400.0 + 100.0 * (k % 12) as f32, s[2] + 1400.0 + 60.0 * (k / 12) as f32);
            let open = l.forest_at(&v, x, z) + (1.0 - l.normal(x, z)[1]) * 4.0;
            if open < best {
                best = open;
                at = (x, z);
            }
        }
        v.set("focus_x", evani_procedural::Value::F32(at.0));
        v.set("focus_z", evani_procedural::Value::F32(at.1));
        v.set("focus_radius", evani_procedural::Value::F32(260.0));
        let o = generate(&v, 7);
        assert!(o.is_valid());
        assert!(o.triangles() < 400_000, "{} triangles", o.triangles());
        let Output::Parts(p) = &o else { panic!("parts") };
        // The shrubs are a part of their own.
        let Output::Parts(q) = &plain else { panic!("parts") };
        assert!(p.len() > q.len(), "{} parts with the focus, {} without", p.len(), q.len());
    }

    /// Round 3: with `horizon` the land runs on round the square into
    /// ranges of its own, still within the library's budget; without it the
    /// square's border settles flat, as before.
    #[test]
    fn the_land_runs_on_round_the_square_within_the_budget() {
        let mut v = values();
        v.set("horizon", evani_procedural::Value::F32(26000.0));
        let o = generate(&v, 7);
        assert!(o.is_valid());
        let (lo, hi) = o.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo[0] < -20000.0 && hi[2] > 20000.0, "{lo:?} {hi:?}");
        assert!(o.triangles() < 400_000, "{} triangles", o.triangles());
        let l = Land::new(&values(), 7);
        assert!(l.h[..l.n].iter().all(|y| *y < 1.0), "the border is not settled");
    }

    #[test]
    fn the_rain_carves_gullies_and_lays_fans() {
        let l = Land::new(&values(), 7);
        let carved = l.dep.iter().filter(|d| **d < -1.0).count();
        let laid = l.dep.iter().filter(|d| **d > 1.0).count();
        assert!(carved > 100 && laid > 100, "carved {carved}, laid {laid}");
    }

    #[test]
    fn every_cover_shows_and_the_seed_changes_the_land() {
        let o = generate(&values(), 7);
        let Output::Parts(p) = &o else { panic!("parts") };
        // Six covers, boulders, trunks and three needle tints.
        assert!(p.len() >= 9, "{} parts", p.len());
        let a = Land::new(&values(), 7);
        let b = Land::new(&values(), 8);
        assert!(a.h.iter().zip(&b.h).any(|(x, y)| (x - y).abs() > 1.0));
    }
}
