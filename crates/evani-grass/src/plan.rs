//! The CPU side of a grass frame, plain data so it is tested natively:
//! the tier's budgets ([`TierCfg`]), the culling planes, the tile jobs the
//! generation pass runs ([`plan_jobs`]), the trample window and the uniform
//! words ([`Sim`], [`view_words`]) laid out as the WGSL reads them.

use evani_core::num::clamp;
use evani_render::math::Mat4;

use crate::{Kind, Patch};

/// Tile edge (m): the unit of generation and CPU culling.
pub const TILE: f32 = 4.0;
/// Most patches drawn (the uniform's array).
pub const MAX_PATCHES: usize = 16;
/// Most trample capsules a frame.
pub const MAX_STAMPS: usize = 16;
/// Blades one job (workgroup) generates.
pub const JOB: u32 = 64;
/// Largest full-density blade count of one tile (16 bits in a job).
pub const MAX_TILE_BLADES: u32 = 65_535;
/// Words of the generation uniform (`struct Sim`).
pub const SIM_WORDS: usize = 548;
/// Words of one draw uniform (`struct View`).
pub const VIEW_WORDS: usize = 48;
/// Bytes of one generated blade (5 x vec4).
pub const BLADE_BYTES: u64 = 80;

/// What one quality level draws. Measured costs are in DECISIONS.md
/// ("evani-grass"); these are the budgets, not measurements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierCfg {
    pub level: usize,
    /// Blades are drawn out to here (m).
    pub max_dist: f32,
    /// Full density out to here (m), then thinning with the square.
    pub full_dist: f32,
    /// The near bucket ends here, the middle one at `lod1`.
    pub lod0: f32,
    pub lod1: f32,
    /// Most candidate blades generated a frame (before culling).
    pub budget: u32,
    /// Most blades drawn per bucket (near, middle, far).
    pub caps: [u32; 3],
    /// Segments per blade per bucket (vertices = 2 x segments + 1).
    pub segs: [u32; 3],
    /// Trample window cells across, and their size (m).
    pub trample_n: u32,
    pub trample_cell: f32,
    /// Translucency strength on this level (0 off).
    pub translucency: f32,
    /// Wet gloss and the sun highlight on this level.
    pub gloss: f32,
    pub specular: f32,
    /// Density against each kind's own.
    pub density: f32,
    /// Most a far blade widens to make up for the thinning.
    pub width_cap: f32,
}

/// The levels, Lite to Ultra. Phones (Low): 40k blades drawn at most,
/// 7 / 5 / 3 vertices, a 64 x 64 trample window, no translucency. Medium:
/// 200k. High and Ultra: denser, further, translucency and wet gloss.
pub const TIERS: [TierCfg; 5] = [
    TierCfg { level: 0, max_dist: 18.0, full_dist: 6.0, lod0: 6.0, lod1: 12.0, budget: 50_000, caps: [10_000, 9_000, 6_000], segs: [3, 2, 1], trample_n: 48, trample_cell: 0.3, translucency: 0.0, gloss: 0.5, specular: 0.5, density: 0.45, width_cap: 2.2 },
    TierCfg { level: 1, max_dist: 26.0, full_dist: 8.0, lod0: 8.0, lod1: 16.0, budget: 90_000, caps: [16_000, 14_000, 10_000], segs: [3, 2, 1], trample_n: 64, trample_cell: 0.25, translucency: 0.0, gloss: 0.6, specular: 0.7, density: 0.55, width_cap: 2.4 },
    TierCfg { level: 2, max_dist: 42.0, full_dist: 12.0, lod0: 10.0, lod1: 24.0, budget: 330_000, caps: [70_000, 80_000, 50_000], segs: [5, 3, 2], trample_n: 128, trample_cell: 0.2, translucency: 0.5, gloss: 1.0, specular: 1.0, density: 0.8, width_cap: 2.6 },
    TierCfg { level: 3, max_dist: 62.0, full_dist: 16.0, lod0: 13.0, lod1: 32.0, budget: 560_000, caps: [120_000, 140_000, 100_000], segs: [7, 4, 2], trample_n: 192, trample_cell: 0.16, translucency: 1.0, gloss: 1.0, specular: 1.0, density: 1.0, width_cap: 2.8 },
    TierCfg { level: 4, max_dist: 85.0, full_dist: 22.0, lod0: 16.0, lod1: 40.0, budget: 900_000, caps: [180_000, 220_000, 200_000], segs: [7, 5, 3], trample_n: 256, trample_cell: 0.15, translucency: 1.0, gloss: 1.0, specular: 1.0, density: 1.0, width_cap: 3.0 },
];

impl TierCfg {
    /// This level drawing at most `blades` of its blades (the caps and the
    /// budget; 0.05..1) with `segments` of its segments along a blade
    /// (0.1..1, rounded up, never under one): `material.grass.blades` and
    /// `material.grass.segments`. At 1 and 1 it is the level itself.
    pub fn scaled(self, blades: f32, segments: f32) -> Self {
        let b = clamp(blades, 0.05, 1.0);
        let s = clamp(segments, 0.1, 1.0);
        let cut = |n: u32| ((n as f32 * b).ceil() as u32).clamp(1, n.max(1));
        let seg = |n: u32| ((n as f32 * s).ceil() as u32).clamp(1, n.max(1));
        Self { budget: cut(self.budget), caps: self.caps.map(cut), segs: self.segs.map(seg), ..self }
    }

    /// The most triangles the blades can be in one view: every bucket full
    /// (a blade of `n` segments is a strip of `2n - 1` triangles).
    pub fn max_triangles(&self) -> u64 {
        self.caps.iter().zip(&self.segs).map(|(c, s)| u64::from(*c) * u64::from(2 * (*s).max(1) - 1)).sum()
    }
}

/// The level for a device tier id: Lite 0, Low 1, Medium and VR 2, High 3,
/// Ultra 4.
pub fn level_for_tier(id: &str) -> usize {
    match id {
        "lite" => 0,
        "low" => 1,
        "high" => 3,
        "ultra" => 4,
        _ => 2,
    }
}

/// The level from `material.grass.quality` (-1: by the tier).
pub fn level(quality: f32, tier: &str) -> usize {
    let q = quality.round() as i32;
    if q < 0 { level_for_tier(tier) } else { (q as usize).min(4) }
}

/// [`TIERS`] entry `level` (clamped).
pub fn tier(level: usize) -> TierCfg {
    TIERS[level.min(TIERS.len() - 1)]
}

/// Share of the full density kept at distance `d` (the WGSL `keep_at`).
pub fn keep_at(d: f32, full: f32, far: f32) -> f32 {
    if d >= far {
        0.0
    } else if d <= full {
        1.0
    } else {
        let r = full / d;
        r * r
    }
}

/// Pixels per metre at 1 m from the eye of a view `height_px` tall with
/// vertical field of view `fov_y_deg`.
pub fn px_scale(height_px: f32, fov_y_deg: f32) -> f32 {
    0.5 * height_px / (0.5 * fov_y_deg.to_radians()).tan().max(1e-4)
}

pub fn norm3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-8 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] }
}

/// The six clip planes of `vp` (column-major, WebGPU clip space with z in
/// 0..w), normalised: a point is inside when `n . p + w >= 0` for all six.
pub fn planes(vp: &Mat4) -> [[f32; 4]; 6] {
    let row = |r: usize| [vp[r], vp[4 + r], vp[8 + r], vp[12 + r]];
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    let add = |a: [f32; 4], b: [f32; 4], s: f32| [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2], a[3] + s * b[3]];
    let mut out = [add(r3, r0, 1.0), add(r3, r0, -1.0), add(r3, r1, 1.0), add(r3, r1, -1.0), r2, add(r3, r2, -1.0)];
    for p in &mut out {
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-8);
        *p = p.map(|v| v / l);
    }
    out
}

/// Whether the box `lo..hi` is at least partly inside every plane.
pub fn box_visible(planes: &[[f32; 4]; 6], lo: [f32; 3], hi: [f32; 3]) -> bool {
    planes.iter().all(|p| {
        let x = if p[0] >= 0.0 { hi[0] } else { lo[0] };
        let y = if p[1] >= 0.0 { hi[1] } else { lo[1] };
        let z = if p[2] >= 0.0 { hi[2] } else { lo[2] };
        p[0] * x + p[1] * y + p[2] * z + p[3] >= 0.0
    })
}

/// A patch as the planner sees it: its rectangle, ground, kind and the
/// blades per square metre close up (kind x patch x global x tier).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatchPlan {
    pub patch: Patch,
    pub density: f32,
    /// Tallest blade of the kind (m), for the tile's culling box.
    pub max_height: f32,
    /// How far the kind is drawn against the tier's reach (short grass
    /// less far: [`reach`]).
    pub reach: f32,
}

/// The share of the tier's reach a kind of blade `height` m tall is drawn
/// to: knee-high grass and crops all of it, a lawn under half (its blades
/// are a pixel or two by then; the carpet carries it).
pub fn reach(height: f32) -> f32 {
    clamp((height.max(0.0) / 0.6).sqrt(), 0.45, 1.0)
}

/// Counts of one plan (the overlay and the tests).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlanStats {
    pub tiles: u32,
    pub jobs: u32,
    /// Candidate blades the jobs cover.
    pub candidates: u32,
    /// The smallest budget scale of the three buckets (1 when every
    /// bucket is under its cap).
    pub scale: f32,
    /// Per bucket (near, middle, far): the share of the planned blades
    /// kept so the bucket fits its cap.
    pub scales: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
struct TileRec {
    ix: i32,
    iz: i32,
    patch: u32,
    full: u32,
    issued: f32,
    near: f32,
    /// Buckets the tile spans (nearest to farthest point).
    b0: usize,
    b1: usize,
}

/// The LOD bucket of a blade `d` m from the eye.
pub fn bucket(d: f32, lod0: f32, lod1: f32) -> usize {
    if d < lod0 {
        0
    } else if d < lod1 {
        1
    } else {
        2
    }
}

/// Share of a bucket's planned blades that survive the per-blade culling
/// (the frustum and the LOD keep at each blade's own distance, not the
/// tile's nearest point).
pub const SURVIVE: f32 = 0.7;

/// The generation jobs of one frame into `out` (cleared first): every
/// tile of every patch within reach of `eye` and inside `planes` (all when
/// `None`), nearest first, each split into jobs of [`JOB`] blades, the
/// near share of each tile's blades only (its LOD keep at its nearest
/// point, times its buckets' scale). The scales keep each LOD bucket under
/// its cap and the whole plan under the tier's budget, thinning evenly
/// instead of dropping whole tiles. Deterministic: the same inputs give
/// the same jobs.
pub fn plan_jobs(patches: &[PatchPlan], eye: [f32; 3], planes: Option<&[[f32; 4]; 6]>, cfg: &TierCfg, max_dist: f32, out: &mut Vec<[u32; 4]>) -> PlanStats {
    out.clear();
    let k = max_dist / cfg.max_dist.max(1e-3);
    let full_dist = cfg.full_dist * k;
    let (lod0, lod1) = (cfg.lod0 * k, cfg.lod1 * k);
    let mut tiles: Vec<TileRec> = Vec::new();
    let mut total = 0.0f64;
    for (pi, pp) in patches.iter().enumerate().take(MAX_PATCHES) {
        let p = &pp.patch;
        if pp.density <= 0.0 {
            continue;
        }
        let x0 = p.min[0].max(eye[0] - max_dist);
        let z0 = p.min[1].max(eye[2] - max_dist);
        let x1 = p.max[0].min(eye[0] + max_dist);
        let z1 = p.max[1].min(eye[2] + max_dist);
        if x0 >= x1 || z0 >= z1 {
            continue;
        }
        let (tx0, tz0) = ((x0 / TILE).floor() as i32, (z0 / TILE).floor() as i32);
        let (tx1, tz1) = (((x1 / TILE).ceil() as i32) - 1, ((z1 / TILE).ceil() as i32) - 1);
        let full = ((pp.density * TILE * TILE).ceil() as u32).min(MAX_TILE_BLADES);
        let top = p.ground_y + pp.max_height * 1.3 + 0.1;
        let reach = clamp(pp.reach, 0.05, 1.0);
        let far = max_dist * reach;
        for iz in tz0..=tz1 {
            for ix in tx0..=tx1 {
                let lo = [(ix as f32 * TILE).max(p.min[0]), p.ground_y, (iz as f32 * TILE).max(p.min[1])];
                let hi = [((ix + 1) as f32 * TILE).min(p.max[0]), top, ((iz + 1) as f32 * TILE).min(p.max[1])];
                if lo[0] >= hi[0] || lo[2] >= hi[2] {
                    continue;
                }
                // Nearest point of the tile's box to the eye.
                let q = [clamp(eye[0], lo[0], hi[0]), clamp(eye[1], lo[1], hi[1]), clamp(eye[2], lo[2], hi[2])];
                let d = ((q[0] - eye[0]).powi(2) + (q[1] - eye[1]).powi(2) + (q[2] - eye[2]).powi(2)).sqrt();
                if d >= far {
                    continue;
                }
                if let Some(pl) = planes
                    && !box_visible(pl, lo, hi)
                {
                    continue;
                }
                let issued = full as f32 * keep_at(d / reach, full_dist, max_dist);
                if issued < 0.5 {
                    continue;
                }
                let fx = if (eye[0] - lo[0]).abs() > (eye[0] - hi[0]).abs() { lo[0] } else { hi[0] };
                let fz = if (eye[2] - lo[2]).abs() > (eye[2] - hi[2]).abs() { lo[2] } else { hi[2] };
                let dfar = ((fx - eye[0]).powi(2) + (p.ground_y - eye[1]).powi(2) + (fz - eye[2]).powi(2)).sqrt();
                total += f64::from(issued);
                tiles.push(TileRec { ix, iz, patch: pi as u32, full, issued, near: d, b0: bucket(d, lod0, lod1), b1: bucket(dfar, lod0, lod1) });
            }
        }
    }
    let global = if total > f64::from(cfg.budget) { (f64::from(cfg.budget) / total) as f32 } else { 1.0 };
    // Per bucket: the planned blades whose nearest point falls in it, kept
    // under the cap with room for the blades the GPU culls itself.
    let mut sums = [0.0f64; 3];
    for t in &tiles {
        sums[t.b0] += f64::from(t.issued);
    }
    let mut scales = [global; 3];
    for (b, sc) in scales.iter_mut().enumerate() {
        let room = f64::from(cfg.caps[b]) / f64::from(SURVIVE);
        if sums[b] * f64::from(global) > room {
            *sc = (room / sums[b]) as f32;
        }
    }
    let mut st = PlanStats { scale: scales.iter().copied().fold(1.0, f32::min), scales, ..PlanStats::default() };
    // Nearest first: the GPU fills the buckets roughly in job order, so if a
    // bucket still overflows, the farthest blades are the ones left out.
    tiles.sort_by(|a, b| a.near.total_cmp(&b.near));
    for t in &tiles {
        let sc = scales[t.b0].max(scales[t.b1]);
        let n = ((t.issued * sc).ceil() as u32).min(t.full);
        if n == 0 {
            continue;
        }
        st.tiles += 1;
        st.candidates += n;
        let mut first = 0;
        while first < n {
            let c = (n - first).min(JOB);
            out.push([t.ix as u32, t.iz as u32, t.patch | (first << 8), t.full | (c << 16)]);
            first += c;
            st.jobs += 1;
        }
    }
    st
}

/// The trample window's origin cell for a centre (x, z): the window is
/// `n` cells across with the centre in its middle cell.
pub fn trample_origin(centre: [f32; 2], n: u32, cell: f32) -> [i32; 2] {
    let half = (n / 2) as i32;
    [(centre[0] / cell).floor() as i32 - half, (centre[1] / cell).floor() as i32 - half]
}

/// Relax factor of the trample window over `dt` seconds: half-life
/// `half_life` (s), slower when wet; 1 (none) when it keeps flat.
pub fn relax(dt: f32, half_life: f32, wet: f32, wet_slow: f32, keep_flat: bool) -> f32 {
    if keep_flat {
        return 1.0;
    }
    let hl = half_life.max(0.05) * (1.0 + clamp(wet, 0.0, 1.0) * wet_slow.max(0.0));
    0.5f32.powf(dt.max(0.0) / hl)
}

/// One capsule pressed into the trample window this frame: swept from
/// `from` to `to` (x, z), its footprint radius, strength and the unit
/// motion direction (zero when standing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamp {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub radius: f32,
    pub strength: f32,
    pub motion: [f32; 2],
}

/// The generation uniform (`struct Sim`) as words, field by field.
#[derive(Debug, Clone)]
pub struct Sim {
    pub words: Vec<u32>,
}

/// Word offsets of `struct Sim`'s fields (checked against naga's layout).
pub mod at {
    pub const VIEW_PROJ: usize = 0;
    pub const PLANES: usize = 16;
    pub const EYE: usize = 40;
    pub const WIND: usize = 44;
    pub const WIND2: usize = 48;
    pub const LOD: usize = 52;
    pub const LOD2: usize = 56;
    pub const TRAMPLE: usize = 60;
    pub const TRAMPLE2: usize = 64;
    pub const STAMPS_N: usize = 68;
    pub const CAPS: usize = 72;
    pub const BASES: usize = 76;
    pub const SCALES: usize = 80;
    pub const STAMPS: usize = 84;
    pub const KINDS: usize = 212;
    pub const KIND_WORDS: usize = 36;
    pub const PATCHES: usize = 356;
    pub const PATCH_WORDS: usize = 12;
}

impl Default for Sim {
    fn default() -> Self {
        Self { words: vec![0; SIM_WORDS] }
    }
}

impl Sim {
    pub fn put(&mut self, at: usize, v: &[f32]) {
        for (i, x) in v.iter().enumerate() {
            if let Some(w) = self.words.get_mut(at + i) {
                *w = x.to_bits();
            }
        }
    }

    pub fn put_u(&mut self, at: usize, v: &[u32]) {
        for (i, x) in v.iter().enumerate() {
            if let Some(w) = self.words.get_mut(at + i) {
                *w = *x;
            }
        }
    }

    pub fn f(&self, at: usize) -> f32 {
        self.words.get(at).map_or(0.0, |w| f32::from_bits(*w))
    }

    /// Kind `k`'s nine vec4s.
    pub fn put_kind(&mut self, kind: Kind, rows: &[[f32; 4]; 9]) {
        let base = at::KINDS + kind as usize * at::KIND_WORDS;
        for (r, row) in rows.iter().enumerate() {
            self.put(base + r * 4, row);
        }
    }

    /// Patch `i`: rectangle, info (ground, feather, kind, density scale)
    /// and its height grid (first word in the `ground` buffer, samples a
    /// side; 0: flat).
    pub fn put_patch(&mut self, i: usize, p: &Patch, ground_at: usize, ground_n: usize) {
        if i >= MAX_PATCHES {
            return;
        }
        let base = at::PATCHES + i * at::PATCH_WORDS;
        self.put(base, &[p.min[0], p.min[1], p.max[0], p.max[1]]);
        self.put(base + 4, &[p.ground_y, p.feather, p.kind as u32 as f32, p.density]);
        self.put(base + 8, &[ground_at as f32, ground_n as f32, p.relief, 0.0]);
    }

    /// Stamp `i` (two vec4s).
    pub fn put_stamp(&mut self, i: usize, s: &Stamp) {
        if i >= MAX_STAMPS {
            return;
        }
        let base = at::STAMPS + i * 8;
        self.put(base, &[s.from[0], s.from[1], s.to[0], s.to[1]]);
        self.put(base + 4, &[s.radius, s.strength, s.motion[0], s.motion[1]]);
    }
}

/// The light and look values every draw uniform shares (all but the
/// view-projection, the eye and the bucket's segments).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shade {
    pub sun: [f32; 3],
    pub translucency: f32,
    pub sun_color: [f32; 3],
    pub specular: f32,
    pub ambient: [f32; 3],
    pub wetness: f32,
    pub fog: [f32; 3],
    pub max_dist: f32,
    pub round: f32,
    pub ao: f32,
    pub wet_darken: f32,
    pub wet_gloss: f32,
    pub soil: [f32; 3],
    pub ground: bool,
    pub time: f32,
    pub full_dist: f32,
}

/// One draw uniform (`struct View`): `vp` and `eye` of the view, the
/// bucket's segments, the view's pixels per metre at 1 m (`px_scale`, for
/// the blades' smallest on-screen width) and the shared shading.
pub fn view_words(vp: &Mat4, eye: [f32; 3], segs: u32, bucket: u32, px_scale: f32, s: &Shade) -> [f32; VIEW_WORDS] {
    let mut o = [0.0f32; VIEW_WORDS];
    o[..16].copy_from_slice(vp);
    let rows: [[f32; 4]; 8] = [
        [eye[0], eye[1], eye[2], segs as f32],
        [s.sun[0], s.sun[1], s.sun[2], s.translucency],
        [s.sun_color[0], s.sun_color[1], s.sun_color[2], s.specular],
        [s.ambient[0], s.ambient[1], s.ambient[2], s.wetness],
        [s.fog[0], s.fog[1], s.fog[2], s.max_dist],
        [s.round, s.ao, s.wet_darken, s.wet_gloss],
        [s.soil[0], s.soil[1], s.soil[2], if s.ground { 1.0 } else { 0.0 }],
        [s.time, bucket as f32, s.full_dist, px_scale],
    ];
    for (i, r) in rows.iter().enumerate() {
        o[16 + i * 4..20 + i * 4].copy_from_slice(r);
    }
    o
}
